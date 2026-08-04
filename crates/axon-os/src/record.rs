//! R21 §3.4 + §4.3 — the tamper-evident run record. Pure (no I/O, no clock).
//!
//! Every capability-bearing action becomes an `AuditEvent` in a SHA-256
//! hash-chain seeded from the manifest digest; `record_digest` is the chain
//! head. Any mutation, drop, reorder, or insertion of an event breaks every
//! subsequent `hash` and the head — so a stored record is integrity-checkable
//! (A6). Authenticated: the integrity + ordering of the recorded events and the
//! manifest they ran under. NOT authenticated: *who* produced the record (no
//! signature) — that is a later spec (HW root of trust).

use crate::grant::EffectSet;
use crate::manifest::JobManifest;
use crate::verdict::Verdict;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const UNIT: char = '\u{1f}'; // the field separator for canonical hashing

/// A raw observed action from the runtime, before it is sequenced + chained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEvent {
    pub action: String,
    pub target: String,
    pub caps_used: Vec<String>, // axis names; canonicalized (sorted) on build
    pub label: String,
}

impl RawEvent {
    pub fn new(action: &str, target: &str, caps: EffectSet, label: &str) -> RawEvent {
        RawEvent {
            action: action.to_string(),
            target: target.to_string(),
            caps_used: axes(caps),
            label: label.to_string(),
        }
    }
}

/// A sequenced, hash-chained audit event (R21 §3.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub seq: u64,
    pub action: String,
    pub target: String,
    pub caps_used: Vec<String>,
    pub label: String,
    pub prev_hash: String,
    pub hash: String,
}

/// The tamper-evident run record (R21 §3.4); serialized as `axon-os-record/1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub schema: String,
    pub run_id: String,
    pub manifest_digest: String,
    pub seed: u64,
    pub events: Vec<AuditEvent>,
    pub verdict: Verdict,
    pub record_digest: String,
}

/// Why a record failed integrity verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyMismatch {
    pub detail: String,
}

fn axes(e: EffectSet) -> Vec<String> {
    // Deterministic order (the struct's field order), so the canonical form is stable.
    let mut v = Vec::new();
    if e.fs_read {
        v.push("fs_read".to_string());
    }
    if e.fs_write {
        v.push("fs_write".to_string());
    }
    if e.net {
        v.push("net".to_string());
    }
    if e.exec {
        v.push("exec".to_string());
    }
    v
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// The canonical, semantic encoding of a manifest (NOT its raw file bytes — so
/// formatting/whitespace can't change the digest). Fixed field order.
pub fn canonical_manifest(m: &JobManifest) -> String {
    let g = &m.grant;
    format!(
        "program={}{UNIT}seed={}{UNIT}fs_read={}{UNIT}fs_write={}{UNIT}net={}{UNIT}exec={}{UNIT}max_label={}{UNIT}budget={},{},{}",
        m.program.display(),
        m.seed,
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

/// The per-event hash: sha256(prev_hash ‖ canonical(seq,action,target,caps,label)).
fn event_hash(prev: &str, seq: u64, e: &RawEvent) -> String {
    let canon = format!(
        "{seq}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}",
        e.action,
        e.target,
        e.caps_used.join(","),
        e.label
    );
    sha256_hex(format!("{prev}{UNIT}{canon}").as_bytes())
}

/// The canonical encoding of a verdict for the seal (AUDIT T47). Fixed field
/// order, every payload field included — a verdict whose `reason`/`axis`/value
/// can be rewritten without breaking the digest is only half-sealed.
fn canonical_verdict(v: &Verdict) -> String {
    match v {
        Verdict::Completed { value } => format!("Completed{UNIT}{value}"),
        Verdict::Malformed { reason } => format!("Malformed{UNIT}{reason}"),
        Verdict::Halted { reason } => format!("Halted{UNIT}{reason}"),
        Verdict::RefineViolation { reason } => format!("RefineViolation{UNIT}{reason}"),
        Verdict::BudgetExhausted { axis } => format!("BudgetExhausted{UNIT}{axis}"),
        Verdict::Denied { reason, axis } => format!("Denied{UNIT}{reason}{UNIT}{axis}"),
        Verdict::ResourceBound { axis, used, cap } => {
            format!("ResourceBound{UNIT}{axis}{UNIT}{used}{UNIT}{cap}")
        }
        Verdict::CoalitionBound {
            axis,
            rollup,
            ceiling,
        } => format!("CoalitionBound{UNIT}{axis}{UNIT}{rollup}{UNIT}{ceiling}"),
        Verdict::VerifyMismatch { detail } => format!("VerifyMismatch{UNIT}{detail}"),
    }
}

/// The terminal SEAL of the chain (AUDIT T47, finding P6-EXIT-03).
///
/// The chain used to run manifest_digest → events → record_digest, and stop.
/// `run_id`, `seed` and — most importantly — `verdict` sat OUTSIDE it. Executed
/// against a real sealed record: rewriting `verdict` from `Completed{value:3}`
/// to `Denied{axis:"sandbox"}`, `seed` from 42 to 999 and `run_id` to another
/// run's id still verified `✓ intact` with a byte-identical digest and exit 0,
/// while tampering any chained EVENT field correctly gave exit 11.
///
/// So the tamper-evident record was tamper-evident for the fields nobody needs
/// to forge, and silent on the one that decides whether the run was allowed.
/// Folding them in as a terminal pseudo-event keeps the chain shape (each link
/// still hashes its predecessor) and needs no schema field.
fn seal_hash(prev: &str, run_id: &str, seed: u64, verdict: &Verdict) -> String {
    let canon = format!(
        "seal{UNIT}{run_id}{UNIT}{seed}{UNIT}{}",
        canonical_verdict(verdict)
    );
    sha256_hex(format!("{prev}{UNIT}{canon}").as_bytes())
}

/// Build a hash-chained record (R21 §4.3). `manifest_digest` seeds the chain
/// (it is the `prev_hash` of event 0); `record_digest` is the chain head, which
/// since T47 includes a terminal seal over run_id/seed/verdict.
pub fn build(
    run_id: &str,
    manifest: &JobManifest,
    seed: u64,
    raw: &[RawEvent],
    verdict: Verdict,
) -> RunRecord {
    let manifest_digest = format!(
        "axsha256:{}",
        sha256_hex(canonical_manifest(manifest).as_bytes())
    );
    let mut events = Vec::with_capacity(raw.len());
    let mut prev = manifest_digest.clone();
    for (seq, e) in raw.iter().enumerate() {
        let seq = seq as u64;
        let hash = event_hash(&prev, seq, e);
        events.push(AuditEvent {
            seq,
            action: e.action.clone(),
            target: e.target.clone(),
            caps_used: e.caps_used.clone(),
            label: e.label.clone(),
            prev_hash: prev.clone(),
            hash: hash.clone(),
        });
        prev = hash;
    }
    // T47: seal run_id/seed/verdict into the chain head. The `axrec2:` prefix
    // marks the change — an `axrec1:` record is REFUSED by `verify` rather than
    // accepted, because accepting it would be a trivial downgrade: strip the
    // seal, present the old format, forge the verdict freely.
    let record_digest = format!("axrec2:{}", seal_hash(&prev, run_id, seed, &verdict));
    RunRecord {
        schema: "axon-os-record/1".to_string(),
        run_id: run_id.to_string(),
        manifest_digest,
        seed,
        events,
        verdict,
        record_digest,
    }
}

/// Verify a record's integrity (R21 §4.3): recompute the chain from the stored
/// events + manifest_digest and assert every `hash`, every `prev_hash` link, and
/// the `record_digest` match. Any tamper → `Err`. Pure.
pub fn verify(rec: &RunRecord) -> Result<(), VerifyMismatch> {
    let mut prev = rec.manifest_digest.clone();
    for (i, ev) in rec.events.iter().enumerate() {
        if ev.seq != i as u64 {
            return Err(VerifyMismatch {
                detail: format!("event {i}: seq is {} (expected {i})", ev.seq),
            });
        }
        if ev.prev_hash != prev {
            return Err(VerifyMismatch {
                detail: format!("event {i}: prev_hash link broken"),
            });
        }
        let raw = RawEvent {
            action: ev.action.clone(),
            target: ev.target.clone(),
            caps_used: ev.caps_used.clone(),
            label: ev.label.clone(),
        };
        let expect = event_hash(&prev, ev.seq, &raw);
        if ev.hash != expect {
            return Err(VerifyMismatch {
                detail: format!("event {i}: hash mismatch (tampered field)"),
            });
        }
        prev = ev.hash.clone();
    }
    // T47: refuse the pre-seal format outright. Verifying it "successfully"
    // would mean accepting a record whose verdict is unauthenticated, and an
    // attacker can always choose which format to present.
    if rec.record_digest.starts_with("axrec1:") {
        return Err(VerifyMismatch {
            detail: "record uses the pre-seal `axrec1:` digest, whose chain does not cover \
                     run_id/seed/verdict — re-run the job to seal it (records in that format \
                     cannot attest their own verdict)"
                .to_string(),
        });
    }
    let expect_seal = seal_hash(&prev, &rec.run_id, rec.seed, &rec.verdict);
    if rec.record_digest != format!("axrec2:{expect_seal}") {
        return Err(VerifyMismatch {
            detail: "record_digest does not match the chain head (run_id, seed, verdict or an \
                     event was tampered)"
                .to_string(),
        });
    }
    Ok(())
}

/// Serialize a record to its canonical JSON (deterministic: serde emits struct
/// fields in declaration order with no whitespace).
pub fn to_json(rec: &RunRecord) -> String {
    serde_json::to_string(rec).expect("RunRecord serializes")
}

/// Parse a record from JSON.
pub fn from_json(s: &str) -> Result<RunRecord, VerifyMismatch> {
    serde_json::from_str(s).map_err(|e| VerifyMismatch {
        detail: format!("malformed record JSON: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grant::{Budget, ExecPolicy, Grant, Label};
    use std::path::PathBuf;

    fn manifest() -> JobManifest {
        JobManifest {
            program: PathBuf::from("/jobs/x.ax"),
            intent: "demo".into(),
            seed: 42,
            grant: Grant {
                fs_read: vec!["./data/".into()],
                fs_write: vec!["./out/".into()],
                net: vec![],
                exec: ExecPolicy::None,
                max_label: Label::Internal,
                budget: Budget {
                    calls: 10,
                    tokens: 10,
                    cost_micro: 10,
                },
            },
        }
    }

    fn events() -> Vec<RawEvent> {
        vec![
            RawEvent::new(
                "fs_read",
                "./data/report.txt",
                EffectSet {
                    fs_read: true,
                    ..Default::default()
                },
                "internal",
            ),
            RawEvent::new(
                "fs_write",
                "./out/summary.txt",
                EffectSet {
                    fs_write: true,
                    ..Default::default()
                },
                "internal",
            ),
        ]
    }

    #[test]
    fn build_then_verify_roundtrips() {
        let rec = build(
            "demo",
            &manifest(),
            42,
            &events(),
            Verdict::Completed { value: 0 },
        );
        assert!(verify(&rec).is_ok());
        assert_eq!(rec.events.len(), 2);
        assert_eq!(rec.events[0].prev_hash, rec.manifest_digest);
        assert_eq!(rec.events[1].prev_hash, rec.events[0].hash);
        assert!(rec.record_digest.starts_with("axrec2:"));
        // JSON round-trips and re-verifies.
        let parsed = from_json(&to_json(&rec)).unwrap();
        assert_eq!(parsed, rec);
        assert!(verify(&parsed).is_ok());
    }

    #[test]
    fn equal_inputs_give_equal_digest() {
        let a = build(
            "r",
            &manifest(),
            42,
            &events(),
            Verdict::Completed { value: 1 },
        );
        let b = build(
            "r",
            &manifest(),
            42,
            &events(),
            Verdict::Completed { value: 1 },
        );
        assert_eq!(a.record_digest, b.record_digest);
        assert_eq!(to_json(&a), to_json(&b)); // byte-identical (A5 building block)
    }

    // R21 §7 `acc_a6_record_tamper_detected` (the pure half).
    #[test]
    fn acc_a6_record_tamper_detected() {
        let good = build(
            "r",
            &manifest(),
            42,
            &events(),
            Verdict::Completed { value: 0 },
        );

        // (a) mutate a field of a mid-chain event.
        let mut m = good.clone();
        m.events[0].target = "./data/SECRET.txt".into();
        assert!(verify(&m).is_err(), "mutated target must be detected");

        // (b) mutate the recorded hash directly.
        let mut m = good.clone();
        m.events[1].hash = "deadbeef".into();
        assert!(verify(&m).is_err(), "mutated hash must be detected");

        // (c) drop an event.
        let mut m = good.clone();
        m.events.remove(0);
        assert!(verify(&m).is_err(), "dropped event must be detected");

        // (d) reorder events.
        let mut m = good.clone();
        m.events.swap(0, 1);
        assert!(verify(&m).is_err(), "reordered events must be detected");

        // (e) insert a forged event with a recomputed-looking hash.
        let mut m = good.clone();
        let forged = m.events[0].clone();
        m.events.insert(1, forged);
        assert!(verify(&m).is_err(), "inserted event must be detected");

        // (f) tamper the record_digest head.
        let mut m = good.clone();
        m.record_digest = "axrec2:0".into();
        assert!(verify(&m).is_err(), "tampered head must be detected");

        // (g) AUDIT T47 (P6-EXIT-03): the head must also seal run_id, seed and
        // VERDICT. Each of these used to sit outside the chain entirely, so a
        // record could be rewritten from a denial to a completion and still
        // verify `✓ intact` with a byte-identical digest.
        // `good` is Completed{0}; rewrite it to a denial and back the other way,
        // so neither direction of forgery passes.
        let mut m = good.clone();
        m.verdict = Verdict::Denied {
            reason: "forged".into(),
            axis: "sandbox".into(),
        };
        assert!(
            verify(&m).is_err(),
            "a rewritten VERDICT must be detected — this is the field the record exists to attest"
        );

        // The dangerous direction: a real denial laundered into a completion.
        let denied = build(
            "r",
            &manifest(),
            42,
            &events(),
            Verdict::Denied {
                reason: "sandbox violation".into(),
                axis: "sandbox".into(),
            },
        );
        let mut m = denied.clone();
        m.verdict = Verdict::Completed { value: 0 };
        assert!(
            verify(&m).is_err(),
            "a denial rewritten into a completion must be detected"
        );

        // Verdict PAYLOADS are sealed too, not just the discriminant.
        let mut m = denied.clone();
        if let Verdict::Denied { reason, axis } = &mut m.verdict {
            *reason = "something harmless".into();
            *axis = "time".into();
        }
        assert!(
            verify(&m).is_err(),
            "a rewritten verdict payload must be detected"
        );

        let mut m = good.clone();
        m.seed = 999;
        assert!(verify(&m).is_err(), "a rewritten seed must be detected");

        let mut m = good.clone();
        m.run_id = "someone-elses-run".into();
        assert!(verify(&m).is_err(), "a rewritten run_id must be detected");

        // (h) The pre-seal format is REFUSED, not accepted: an attacker chooses
        // which format to present, so accepting `axrec1:` is a free downgrade.
        let mut m = good.clone();
        m.record_digest = format!("axrec1:{}", &good.record_digest[7..]);
        let e = verify(&m).expect_err("an axrec1 record must be refused");
        assert!(
            e.detail.contains("axrec1"),
            "the refusal must say why: {}",
            e.detail
        );
    }

    #[test]
    fn empty_run_has_head_equal_to_manifest_digest() {
        let rec = build(
            "r",
            &manifest(),
            42,
            &[],
            Verdict::Denied {
                reason: "gate".into(),
                axis: "net".into(),
            },
        );
        // T47: the head is now the SEAL over (manifest_digest, run_id, seed,
        // verdict) rather than the bare manifest digest — an empty run still has
        // a verdict, and that verdict must be attested.
        assert_eq!(
            rec.record_digest,
            format!(
                "axrec2:{}",
                seal_hash(&rec.manifest_digest, "r", 42, &rec.verdict)
            )
        );
        assert!(verify(&rec).is_ok());
    }
}
