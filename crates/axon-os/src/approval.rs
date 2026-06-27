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

const UNIT: char = '\u{1f}';

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// The canonical, semantic encoding of a grant (fixed field order; formatting
/// cannot change the digest). The one definition both axon-os and axon-intent use.
pub fn canonical_grant(g: &Grant) -> String {
    format!(
        "fs_read={}{UNIT}fs_write={}{UNIT}net={}{UNIT}exec={}{UNIT}max_label={}{UNIT}budget={},{},{}",
        g.fs_read.join(","),
        g.fs_write.join(","),
        g.net.join(","),
        g.exec.as_str(),
        g.max_label.as_str(),
        g.budget.calls,
        g.budget.tokens,
        g.budget.cost_micro,
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
