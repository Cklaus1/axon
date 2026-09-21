//! R22 handoff (R22 §3.5) — verify an ApprovalToken at the run boundary.
//!
//! axon-intent produces a content-binding `<name>.approval` token; this is the
//! enforcement side: axon-os recomputes the digests from the ACTUAL program
//! source + grant (plus the token's own metadata) and refuses to run a job whose
//! program or grant was edited after approval, or whose decision wasn't
//! "approved". A one-byte edit to either invalidates the token.
//!
//! `canonical_grant` here is the SINGLE SOURCE of the grant encoding — axon-intent
//! delegates to it — so the two stacks can never drift on the digest.

use crate::grant::Grant;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

const UNIT: char = '\u{1f}';

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// The canonical, semantic encoding of a grant (fixed field order; formatting
/// cannot change the digest). The one definition both axon-os and axon-intent use.
///
/// EXHAUSTIVE DESTRUCTURING IS THE POINT. A struct pattern without `..` must
/// bind every field, so adding a field to `Grant` is a COMPILE ERROR here until
/// it is encoded or explicitly bound with a stated reason. `..` must never be
/// introduced; that is the whole guarantee.
///
/// It was previously a `format!` over named fields, which encoded 6 of 7 and
/// silently omitted `reproducible`. REPRODUCED: two manifests differing ONLY by
/// `profile = "hermetic"` vs `"restricted"`, one hand-minted token copied
/// byte-identically to both, and BOTH printed "Approval: required and verified
/// (program + grant unedited since sign-off)". The restricted run then loaded
/// and executed operator-ambient code through `AXON_PATH` — a module search
/// path, i.e. a code-injection channel — which `runtime.rs` strips under the
/// hermetic posture along with AXON_AUDIT_LEDGER, AXON_AI_REPLAY and
/// AXON_AI_MOCK.
///
/// The omission was justified in `grant.rs` as "it constrains HOW a run
/// executes, not WHAT it may touch". That is contradicted by `Grant::intersect`
/// in the same file, which ORs `reproducible` precisely because taking the AND
/// "would let a broad supervisor grant relax a hermetic job, which is the
/// direction an intersection must never go". A field an intersection treats as
/// authority is authority, and the AXON_PATH channel settles it: it governs
/// what code the job may LOAD.
///
/// SCHEMA BUMP, deliberately breaking. Including the field changes every
/// existing `grant_digest`, so tokens in flight stop verifying. A stable digest
/// over an incomplete statement is worth less than a breaking change, and
/// accepting `axon-approval/1` as legacy would leave the hole reachable by
/// downgrade — the same reasoning that makes the intersection OR rather than
/// AND.
pub fn canonical_grant(g: &Grant) -> String {
    let Grant {
        fs_read,
        fs_write,
        net,
        exec,
        max_label,
        budget,
        reproducible,
    } = g;
    format!(
        "fs_read={}{UNIT}fs_write={}{UNIT}net={}{UNIT}exec={}{UNIT}max_label={}{UNIT}\
         budget={},{},{}{UNIT}reproducible={}",
        fs_read.join(","),
        fs_write.join(","),
        net.join(","),
        exec.as_str(),
        max_label.as_str(),
        budget.calls,
        budget.tokens,
        budget.cost_micro,
        reproducible,
    )
}

/// The subset of the `axon-approval/1` token axon-os needs to verify (structural
/// JSON deserialization — no shared type required with axon-intent).
#[derive(Deserialize)]
struct TokenView {
    program_digest: String,
    grant_digest: String,
    approved_by: String,
    decision: String,
    risk: String,
    token_digest: String,
}

/// Verify an approval token against the actual program source + grant.
/// `Ok(())` iff: decision == "approved", the program digest matches the source,
/// the grant digest matches the grant, AND the token's own digest re-hashes
/// (internal consistency). Any failure ⇒ `Err(reason)`.
pub fn verify_approval(token_json: &str, program_src: &str, grant: &Grant) -> Result<(), String> {
    let t: TokenView =
        serde_json::from_str(token_json).map_err(|e| format!("malformed approval token: {e}"))?;

    if t.decision != "approved" {
        return Err(format!(
            "approval decision is `{}`, not approved",
            t.decision
        ));
    }
    let pd = format!("axsha256:{}", sha256_hex(program_src.as_bytes()));
    if pd != t.program_digest {
        return Err("program was edited after approval (program digest mismatch)".into());
    }
    let gd = format!("axsha256:{}", sha256_hex(canonical_grant(grant).as_bytes()));
    if gd != t.grant_digest {
        return Err("grant was edited after approval (grant digest mismatch)".into());
    }
    // Internal consistency: the token's own digest must re-hash from its fields.
    let canon = format!(
        "{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}",
        t.program_digest, t.grant_digest, t.approved_by, t.decision, t.risk
    );
    let td = format!("axtok1:{}", sha256_hex(canon.as_bytes()));
    if td != t.token_digest {
        return Err("approval token digest mismatch (tampered metadata)".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grant::{Budget, ExecPolicy, Label};

    fn grant() -> Grant {
        Grant {
            reproducible: false,
            fs_read: vec!["./data/".into()],
            fs_write: vec!["./out/".into()],
            net: vec![],
            exec: ExecPolicy::None,
            max_label: Label::Internal,
            budget: Budget {
                calls: 100,
                tokens: 50000,
                cost_micro: 1000000,
            },
        }
    }

    // A real token (the same format axon-intent emits), built here to prove the
    // digest formulas agree across the two crates without importing axon-intent.
    fn token_for(program_src: &str, g: &Grant) -> String {
        let pd = format!("axsha256:{}", sha256_hex(program_src.as_bytes()));
        let gd = format!("axsha256:{}", sha256_hex(canonical_grant(g).as_bytes()));
        let (approved_by, decision, risk) = ("alice", "approved", "medium");
        let canon = format!("{pd}{UNIT}{gd}{UNIT}{approved_by}{UNIT}{decision}{UNIT}{risk}");
        let td = format!("axtok1:{}", sha256_hex(canon.as_bytes()));
        format!(
            "{{\"schema\":\"axon-approval/1\",\"program_digest\":\"{pd}\",\"grant_digest\":\"{gd}\",\"approved_by\":\"{approved_by}\",\"decision\":\"{decision}\",\"risk\":\"{risk}\",\"token_digest\":\"{td}\"}}"
        )
    }

    #[test]
    fn approved_unedited_token_verifies() {
        let src = "fn main() { 0 }";
        assert!(verify_approval(&token_for(src, &grant()), src, &grant()).is_ok());
    }

    #[test]
    fn edited_program_is_rejected() {
        let tok = token_for("fn main() { 0 }", &grant());
        assert!(verify_approval(&tok, "fn main() { 1 }", &grant()).is_err());
    }

    #[test]
    fn edited_grant_is_rejected() {
        let tok = token_for("fn main() { 0 }", &grant());
        let mut widened = grant();
        widened.net = vec!["evil.com".into()]; // grant edited after approval
        assert!(verify_approval(&tok, "fn main() { 0 }", &widened).is_err());
    }

    #[test]
    fn rejected_decision_is_refused() {
        let tok = token_for("fn main() { 0 }", &grant()).replace("approved", "rejected");
        assert!(verify_approval(&tok, "fn main() { 0 }", &grant()).is_err());
    }
}

/// The three authorization outcomes, recorded so a reader can tell them apart
/// afterward. `RunRecord` previously carried no approval field at all, so an
/// unapproved execution and an approved one archived identically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalStatus {
    /// The job does not require sign-off and no token was present.
    NotRequired,
    /// A valid token was present; the job required it.
    VerifiedRequired,
    /// A valid token was present; the job did not require it.
    VerifiedNotRequired,
    /// Authorization FAILED — the token was missing when required, or present
    /// and invalid. Distinct from `NotRequired`, which the denial branch used
    /// to stamp: the record of a run refused for want of sign-off then said
    /// the job did not need sign-off, contradicting the verdict beside it.
    Denied,
}

impl ApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalStatus::NotRequired => "not_required",
            ApprovalStatus::VerifiedRequired => "verified_required",
            ApprovalStatus::VerifiedNotRequired => "verified_not_required",
            ApprovalStatus::Denied => "denied",
        }
    }
}

/// Authorize a job for execution, from the job file's own `.approval` sibling.
///
/// THE SINGLE AUTHORIZATION SITE. This logic lived in `cmd_run`, so `axon-os
/// replay` — which reaches `supervisor::run` by a different route — executed
/// without it. REPRODUCED: a stored job marked `require_approval = true` with
/// no token anywhere was refused by `run` with exit 8 and no side effect, and
/// re-executed by `replay` with exit 0, the side-effect file's mtime advancing.
/// The public `axon_os::supervise` re-export was a third route with no gate at
/// all.
///
/// It is called from inside `supervisor::run` — the point every execution path
/// converges on, and which already hosts the one check (`gate::admit`) that no
/// caller can skip. A check in a CALLER is opt-in per call site; a check here
/// is not.
pub fn authorize(
    job_path: &Path,
    manifest: &crate::manifest::JobManifest,
) -> Result<ApprovalStatus, String> {
    let approval_path = job_path.with_extension("approval");
    if approval_path.exists() {
        let token = std::fs::read_to_string(&approval_path).unwrap_or_default();
        let program_src = std::fs::read_to_string(&manifest.program).unwrap_or_default();
        // An INVALID token is a failure whether or not policy required one.
        verify_approval(&token, &program_src, &manifest.grant)?;
        Ok(if manifest.require_approval {
            ApprovalStatus::VerifiedRequired
        } else {
            ApprovalStatus::VerifiedNotRequired
        })
    } else if manifest.require_approval {
        Err(format!(
            "approval required but missing — this job sets `require_approval = true` \
             and there is no token at {}",
            approval_path.display()
        ))
    } else {
        Ok(ApprovalStatus::NotRequired)
    }
}
