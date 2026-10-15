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

/// The operator approval key: hex, at least 16 bytes. A DEDICATED variable,
/// not `AXON_ATTEST_KEY`: that key belongs to `axon-vm`, which falls back to an
/// ephemeral one, and reusing it would silently flip axon-os to refusing for
/// any operator who set it for axon-vm. There is deliberately no ephemeral
/// fallback here either: a per-process key cannot verify what a previous
/// process wrote.
///
/// Threat model: the key is SYMMETRIC, so every holder — the approving operator
/// and the enforcing host alike — can mint tokens and seal records. It proves
/// "made by a key holder", not "made by this approver": `approved_by` stays
/// unauthenticated (per-approver signatures are out of scope).
///
/// Setting the key is an opt-in that invalidates unkeyed artifacts: a keyed
/// verifier refuses unkeyed records (downgrade protection), so a run archived
/// before the key was set can no longer be verified or replayed under it.
pub const APPROVAL_KEY_ENV: &str = "AXON_APPROVAL_KEY";

/// The two subkeys derived from the operator approval key, one per use, so a
/// MAC made for a token is never valid as a record seal or vice versa:
/// `HMAC-SHA256(op_key, "axon-approval token v1")` and
/// `HMAC-SHA256(op_key, "axon-os record v1")`. `Debug` is redacted.
#[derive(Clone)]
pub struct ApprovalKey {
    token: [u8; 32],
    record: [u8; 32],
}

impl std::fmt::Debug for ApprovalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalKey(<redacted>)")
    }
}

impl ApprovalKey {
    /// Derive both subkeys from operator key bytes (>= 16 bytes).
    pub fn derive(operator_key: &[u8]) -> Result<ApprovalKey, String> {
        if operator_key.len() < 16 {
            return Err(format!(
                "{APPROVAL_KEY_ENV} is shorter than 16 bytes: refusing to key approvals with it"
            ));
        }
        Ok(ApprovalKey {
            token: axon_attest::hmac_sha256(operator_key, b"axon-approval token v1"),
            record: axon_attest::hmac_sha256(operator_key, b"axon-os record v1"),
        })
    }

    /// `HMAC-SHA256(token_subkey, data)` — the approval-token MAC.
    pub fn token_mac(&self, data: &[u8]) -> [u8; 32] {
        axon_attest::hmac_sha256(&self.token, data)
    }

    /// `HMAC-SHA256(record_subkey, data)` — the run-record seal MAC.
    pub fn record_mac(&self, data: &[u8]) -> [u8; 32] {
        axon_attest::hmac_sha256(&self.record, data)
    }

    /// The key from [`APPROVAL_KEY_ENV`]: unset or blank ⇒ `None` (unkeyed);
    /// set but not hex, or shorter than 16 bytes ⇒ `Err`, never a silent fall
    /// back to unkeyed.
    pub fn from_env() -> Result<Option<ApprovalKey>, String> {
        Self::from_value(std::env::var(APPROVAL_KEY_ENV))
    }

    fn from_value(v: Result<String, std::env::VarError>) -> Result<Option<ApprovalKey>, String> {
        match v {
            Ok(v) if !v.trim().is_empty() => {
                let bytes = decode_hex(v.trim())
                    .ok_or_else(|| format!("{APPROVAL_KEY_ENV} is not valid hex"))?;
                ApprovalKey::derive(&bytes).map(Some)
            }
            Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                Err(format!("{APPROVAL_KEY_ENV} is not valid hex"))
            }
        }
    }
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The token schemas axon-os knows how to verify. Anything else is refused: a
/// field the verifier does not know the meaning of is not evidence.
const KNOWN_TOKEN_SCHEMAS: &[&str] = &["axon-approval/1"];

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
    schema: String,
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
    if !KNOWN_TOKEN_SCHEMAS.contains(&t.schema.as_str()) {
        return Err(format!(
            "approval token schema `{}` is not one axon-os verifies (known: {})",
            t.schema,
            KNOWN_TOKEN_SCHEMAS.join(", ")
        ));
    }

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

    // G5b: a token whose schema the verifier does not know is refused, even
    // when every digest in it is internally consistent.
    #[test]
    fn unknown_token_schema_is_refused() {
        let src = "fn main() { 0 }";
        let tok = token_for(src, &grant()).replace("axon-approval/1", "axon-approval/9");
        let e = verify_approval(&tok, src, &grant())
            .expect_err("ATTACK: a token of an unknown schema was verified");
        assert!(
            e.contains("axon-approval/9"),
            "the refusal names the schema: {e}"
        );
        let no_schema = token_for(src, &grant()).replace("\"schema\":\"axon-approval/1\",", "");
        assert!(
            verify_approval(&no_schema, src, &grant()).is_err(),
            "a token with no schema at all must be refused"
        );
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    // Known answer: op key 00..0f; each subkey is HMAC(op_key, domain), checked
    // through the MAC it keys (HMAC(subkey, "x")), computed independently with
    // Python's hmac module. Pins both domains and that they differ.
    #[test]
    fn approval_key_derivation_known_answer() {
        let k = ApprovalKey::derive(&(0u8..16).collect::<Vec<_>>()).unwrap();
        assert_eq!(
            hex(&k.token_mac(b"x")),
            "8fd45308a38214c9c818302f427aa059121e0a5069b0620de1680be37229ace3",
            "signing KAT approval token subkey derivation label"
        );
        assert_eq!(
            hex(&k.record_mac(b"x")),
            "81d692c315dd0baec42afb3ca84a5c0dc938050c90706da85541609dd0aff7ca",
            "signing KAT approval record subkey derivation label"
        );
        assert_eq!(format!("{k:?}"), "ApprovalKey(<redacted>)");
    }

    #[test]
    fn approval_key_parsing_refuses_malformed_and_short() {
        use std::env::VarError;
        let parse = |s: &str| ApprovalKey::from_value(Ok(s.to_string()));
        assert!(parse("").unwrap().is_none(), "blank ⇒ unkeyed");
        assert!(parse("   ").unwrap().is_none(), "blank ⇒ unkeyed");
        assert!(ApprovalKey::from_value(Err(VarError::NotPresent))
            .unwrap()
            .is_none());
        let e = parse("zz00112233445566778899aabbccddeeff").unwrap_err();
        assert!(e.contains("not valid hex"), "{e}");
        let e = parse("0011223").unwrap_err();
        assert!(e.contains("not valid hex"), "odd length: {e}");
        let e = parse("00112233445566778899aabbccddee").unwrap_err();
        assert!(e.contains("shorter than 16 bytes"), "15 bytes: {e}");
        assert!(parse("00112233445566778899aabbccddeeff").unwrap().is_some());
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
    /// The run was refused BEFORE authorization ran (the isolation check), so
    /// no approval decision exists. Distinct from `unknown`, the default for a
    /// record built outside the supervisor.
    NotChecked,
}

impl ApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalStatus::NotRequired => "not_required",
            ApprovalStatus::VerifiedRequired => "verified_required",
            ApprovalStatus::VerifiedNotRequired => "verified_not_required",
            ApprovalStatus::Denied => "denied",
            ApprovalStatus::NotChecked => "not_checked",
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
