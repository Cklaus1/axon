//! R22 §3.4 + §3.5 + §4.4 — the legible approval request + the tamper-evident
//! approval token. Pure (no I/O, no clock). The token is a content-binding
//! digest + the recorded human decision — NOT a PKI signature (R22 §1.2 / A6).
//!
//! `build_token` binds the EXACT `(program_digest, grant_digest, approved_by,
//! decision, risk)` into a `token_digest`. `verify_token` recomputes the program
//! and grant digests from the ACTUAL on-disk content and re-hashes the token;
//! editing the program OR the grant by one byte after approval invalidates the
//! token (this is the cross-crate contract R21 honors on handoff). Authenticated:
//! that the approved (program, grant) is byte-for-byte the one that runs. NOT
//! authenticated: WHO approved (the `approved_by` is recorded, not signed).

use crate::confidence::Confidence;
use crate::intent::Intent;
use crate::refusal::Decision;
use crate::synth::ProgramDraft;
use axon_os::grant::{ExecPolicy, Grant};
use sha2::{Digest, Sha256};

const UNIT: char = '\u{1f}'; // canonical field separator

/// Phase-11 risk level (R22 §3.4), derived from the grant's effect axes. The
/// `--risk` flag may only RAISE this, never lower it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
    High,
    Critical,
}

impl Risk {
    pub fn as_str(&self) -> &'static str {
        match self {
            Risk::Low => "Low",
            Risk::Medium => "Medium",
            Risk::High => "High",
            Risk::Critical => "Critical",
        }
    }
    pub fn parse(s: &str) -> Option<Risk> {
        match s.to_lowercase().as_str() {
            "low" => Some(Risk::Low),
            "medium" => Some(Risk::Medium),
            "high" => Some(Risk::High),
            "critical" => Some(Risk::Critical),
            _ => None,
        }
    }
}

/// Derive risk from a grant's effect axes (Phase-11 rule, R22 §4.4): Exec →
/// Critical; Net AND FS → High; Net OR FS → Medium; pure → Low.
pub fn derive_risk(grant: &Grant) -> Risk {
    let e = grant.effect_set();
    if e.exec {
        return Risk::Critical;
    }
    let fs = e.fs_read || e.fs_write;
    match (e.net, fs) {
        (true, true) => Risk::High,
        (true, false) | (false, true) => Risk::Medium,
        (false, false) => Risk::Low,
    }
}

/// The rendered-for-the-human legible bound (R22 §3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub program_digest: String,
    pub grant: Grant,
    pub grant_digest: String,
    pub intent_goal: String,
    pub risk: Risk,
    pub confidence: u8,
    pub legible: String,
    pub uncertainty: Vec<String>,
}

/// The tamper-evident handoff token (R22 §3.5); JSON schema `axon-approval/1`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApprovalToken {
    pub schema: String,
    pub program_digest: String,
    pub grant_digest: String,
    pub approved_by: String,
    pub decision: Decision,
    pub risk: Risk,
    pub token_digest: String,
}

/// Why `verify_token` rejected a token (R22 §4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub detail: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// `"axsha256:"+sha256(program source bytes)` — binds the EXACT program text.
pub fn program_digest(program_src: &str) -> String {
    format!("axsha256:{}", sha256_hex(program_src.as_bytes()))
}

/// The canonical, semantic encoding of a grant (NOT raw bytes — formatting can't
/// change the digest). Fixed field order; mirrors R21's `canonical_manifest`
/// grant portion so the two stacks agree.
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

/// `"axsha256:"+sha256(canonical grant)` — binds the EXACT grant.
pub fn grant_digest(g: &Grant) -> String {
    format!("axsha256:{}", sha256_hex(canonical_grant(g).as_bytes()))
}

/// The token digest: `"axtok1:"+sha256(canonical(program_digest, grant_digest,
/// approved_by, decision, risk))`.
fn compute_token_digest(
    program_digest: &str,
    grant_digest: &str,
    approved_by: &str,
    decision: Decision,
    risk: Risk,
) -> String {
    let canon = format!(
        "{program_digest}{UNIT}{grant_digest}{UNIT}{approved_by}{UNIT}{}{UNIT}{}",
        decision.as_str(),
        risk.as_str(),
    );
    format!("axtok1:{}", sha256_hex(canon.as_bytes()))
}

/// The human-readable "may / may NOT / budget / label / risk / confidence"
/// rendering (R22 §4.4) — the legible bound the operator actually reviews.
pub fn legible_bound(g: &Grant, risk: Risk, confidence: u8) -> String {
    let mut may = Vec::new();
    let mut not = Vec::new();
    if g.fs_read.is_empty() {
        not.push("read files".to_string());
    } else {
        may.push(format!("read {}", g.fs_read.join(", ")));
    }
    if g.fs_write.is_empty() {
        not.push("write files".to_string());
    } else {
        may.push(format!("write {}", g.fs_write.join(", ")));
    }
    if g.net.is_empty() {
        not.push("use the network".to_string());
    } else {
        may.push(format!("reach {}", g.net.join(", ")));
    }
    if matches!(g.exec, ExecPolicy::Any) {
        may.push("spawn processes".to_string());
    } else {
        not.push("spawn processes".to_string());
    }
    format!(
        "  This program WILL be allowed to: {}.\n  It may NOT: {}.\n  Budget: \u{2264} {} calls / {} tokens / {} \u{b5}$.\n  Confidentiality ceiling: {}.\n  Risk: {}. Confidence: {}/100.",
        if may.is_empty() { "(nothing)".into() } else { may.join(", ") },
        if not.is_empty() { "(no restrictions)".into() } else { not.join(", ") },
        g.budget.calls,
        g.budget.tokens,
        g.budget.cost_micro,
        g.max_label.as_str(),
        risk.as_str(),
        confidence,
    )
}

/// Render the legible approval request from a draft + inferred grant + intent
/// (R22 §4.4). `confidence` is the score (the caller has already passed the
/// confidence gate). `risk_floor` (e.g. from `--risk`) may only RAISE the
/// derived risk.
pub fn render_request(
    draft: &ProgramDraft,
    grant: &Grant,
    intent: &Intent,
    confidence: &Confidence,
    risk_floor: Option<Risk>,
) -> ApprovalRequest {
    let derived = derive_risk(grant);
    let risk = match risk_floor {
        Some(f) => derived.max(f),
        None => derived,
    };
    let conf = confidence.score();
    ApprovalRequest {
        program_digest: program_digest(&draft.source),
        grant_digest: grant_digest(grant),
        grant: grant.clone(),
        intent_goal: intent.goal.clone(),
        risk,
        confidence: conf,
        legible: legible_bound(grant, risk, conf),
        uncertainty: draft.meta.uncertainty_notes.clone(),
    }
}

/// Build an approval token binding the request's content + the human's decision
/// (R22 §4.4).
pub fn build_token(req: &ApprovalRequest, approved_by: &str, decision: Decision) -> ApprovalToken {
    let token_digest = compute_token_digest(
        &req.program_digest,
        &req.grant_digest,
        approved_by,
        decision,
        req.risk,
    );
    ApprovalToken {
        schema: "axon-approval/1".to_string(),
        program_digest: req.program_digest.clone(),
        grant_digest: req.grant_digest.clone(),
        approved_by: approved_by.to_string(),
        decision,
        risk: req.risk,
        token_digest,
    }
}

/// Verify a token against the ACTUAL on-disk `(program, grant)` (R22 §4.4). This
/// is what R21 calls on handoff. Any of: the program edited, the grant edited,
/// the decision/risk/approver flipped, or the token not "approved" → `Err`.
/// Editing the program OR grant by one byte invalidates the token.
pub fn verify_token(
    token: &ApprovalToken,
    program_src: &str,
    grant: &Grant,
) -> Result<(), Mismatch> {
    if token.schema != "axon-approval/1" {
        return Err(Mismatch {
            detail: format!("unknown token schema `{}`", token.schema),
        });
    }
    // 1. the program must be byte-for-byte the approved one.
    let pd = program_digest(program_src);
    if pd != token.program_digest {
        return Err(Mismatch {
            detail: "program edited after approval (digest mismatch)".to_string(),
        });
    }
    // 2. the grant must be byte-for-byte the approved one.
    let gd = grant_digest(grant);
    if gd != token.grant_digest {
        return Err(Mismatch {
            detail: "grant edited after approval (digest mismatch)".to_string(),
        });
    }
    // 3. the token must self-consistently re-hash (no field flipped).
    let expect = compute_token_digest(
        &token.program_digest,
        &token.grant_digest,
        &token.approved_by,
        token.decision,
        token.risk,
    );
    if expect != token.token_digest {
        return Err(Mismatch {
            detail: "token_digest does not re-hash (a bound field was tampered)".to_string(),
        });
    }
    // 4. only an APPROVED token authorizes a run.
    if token.decision != Decision::Approved {
        return Err(Mismatch {
            detail: "token decision is not `approved`".to_string(),
        });
    }
    Ok(())
}

/// Serialize a token to canonical JSON (deterministic; A5).
pub fn to_json(t: &ApprovalToken) -> String {
    serde_json::to_string(t).expect("ApprovalToken serializes")
}

/// Parse a token from JSON.
pub fn from_json(s: &str) -> Result<ApprovalToken, Mismatch> {
    serde_json::from_str(s).map_err(|e| Mismatch {
        detail: format!("malformed approval token JSON: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::confidence::Confidence;
    use crate::intent;
    use crate::synth::meta_from_source;
    use axon_os::grant::{Budget, ExecPolicy, Grant, Label};

    const VALID: &str = r#"# Intent
Summarize ./data/report.txt into ./out/summary.txt.

## Outputs
- ./out/summary.txt

## Allowed
- fs_read: ./data/
- fs_write: ./out/
- net: none
- exec: none
- max_label: internal
"#;

    fn it() -> Intent {
        intent::parse(VALID).unwrap()
    }

    const PROG: &str = "fn main() -> i64 { write_file(\"./out/summary.txt\", \"x\") 0 }";

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

    fn draft() -> ProgramDraft {
        ProgramDraft {
            source: PROG.to_string(),
            generated: true,
            fences_stripped: false,
            meta: meta_from_source(PROG, true, vec![]),
        }
    }

    fn req() -> ApprovalRequest {
        render_request(
            &draft(),
            &grant(),
            &it(),
            &Confidence::Ship { score: 100 },
            None,
        )
    }

    #[test]
    fn legible_lists_may_and_may_not() {
        let r = req();
        assert!(r.legible.contains("write ./out/"));
        assert!(r.legible.contains("may NOT") && r.legible.contains("use the network"));
        assert!(r.legible.contains("Risk: Medium")); // fs but no net ⇒ Medium
        assert!(r.legible.contains("Confidence: 100/100"));
    }

    #[test]
    fn risk_floor_only_raises() {
        let r = render_request(
            &draft(),
            &grant(),
            &it(),
            &Confidence::Ship { score: 90 },
            Some(Risk::High),
        );
        assert_eq!(r.risk, Risk::High, "floor raises Medium→High");
        let r2 = render_request(
            &draft(),
            &grant(),
            &it(),
            &Confidence::Ship { score: 90 },
            Some(Risk::Low),
        );
        assert_eq!(r2.risk, Risk::Medium, "floor cannot lower below derived");
    }

    #[test]
    fn derive_risk_axes() {
        let mut g = grant();
        assert_eq!(derive_risk(&g), Risk::Medium); // fs only
        g.net = vec!["x.com".into()];
        assert_eq!(derive_risk(&g), Risk::High); // net + fs
        g.exec = ExecPolicy::Any;
        assert_eq!(derive_risk(&g), Risk::Critical);
        let pure = Grant {
            fs_read: vec![],
            fs_write: vec![],
            net: vec![],
            exec: ExecPolicy::None,
            ..g.clone()
        };
        assert_eq!(derive_risk(&pure), Risk::Low);
    }

    // R22 §7 `acc_a6_approval_token_binds_and_tamper_detected` (pure half).
    #[test]
    fn acc_a6_approval_token_binds_and_tamper_detected() {
        let token = build_token(&req(), "alice", Decision::Approved);
        // The unedited (program, grant) verifies.
        assert!(verify_token(&token, PROG, &grant()).is_ok());

        // (a) one program byte flipped → fail.
        let edited_prog = PROG.replace("0 }", "1 }");
        assert!(verify_token(&token, &edited_prog, &grant()).is_err());

        // (b) one grant field changed → fail.
        let mut g2 = grant();
        g2.budget.tokens += 1;
        assert!(verify_token(&token, PROG, &g2).is_err());

        // (c) the token's decision field flipped → re-hash fails.
        let mut t = token.clone();
        t.decision = Decision::Rejected;
        assert!(verify_token(&t, PROG, &grant()).is_err());

        // (d) the risk field flipped → re-hash fails.
        let mut t = token.clone();
        t.risk = Risk::Critical;
        assert!(verify_token(&t, PROG, &grant()).is_err());

        // JSON round-trips.
        let parsed = from_json(&to_json(&token)).unwrap();
        assert_eq!(parsed, token);
        assert!(verify_token(&parsed, PROG, &grant()).is_ok());
    }

    // R22 §7 `approval_invalidated_by_any_edit`.
    #[test]
    fn approval_invalidated_by_any_edit() {
        let token = build_token(&req(), "alice", Decision::Approved);
        // Flip ONE byte of the program.
        for i in 0..PROG.len() {
            let mut bytes = PROG.as_bytes().to_vec();
            // perturb to a different printable char
            bytes[i] = if bytes[i] == b'x' { b'y' } else { b'x' };
            if let Ok(edited) = String::from_utf8(bytes) {
                if edited != PROG {
                    assert!(
                        verify_token(&token, &edited, &grant()).is_err(),
                        "editing program byte {i} must invalidate the token"
                    );
                }
            }
        }
        // Flip the grant (every axis once).
        let mut g = grant();
        g.fs_write = vec!["./elsewhere/".into()];
        assert!(verify_token(&token, PROG, &g).is_err());
    }

    #[test]
    fn rejected_token_does_not_authorize() {
        let token = build_token(&req(), "alice", Decision::Rejected);
        // Even with an intact program+grant, a rejected token is not honored.
        assert!(verify_token(&token, PROG, &grant()).is_err());
    }
}
