//! When the verifier identity may sign a receipt (G01-r22-independent-issuer).
//!
//! One pure decision so every branch — including the protected-microVM one no
//! integration environment here can exercise — is tested directly. The binary
//! signs exactly when this returns `Ok`, and prints the `Err` as
//! `attestation_withheld`.

use crate::backend::LINUX_MICROVM_PROTECTED;
use crate::submit::RanUnder;
use axon_loop_contracts::{ComputeRequest, JobKind};

pub const NOT_A_CHECK: &str = "not a registered_check: the verifier signs verdicts, not execution";
pub const CANDIDATE_RUBRIC: &str = "the check is a file of the candidate's tree, not an \
     operator-registered suite (check:<id>): candidate bytes cannot define the rubric, so the \
     verifier does not vouch for it";
pub const REPLAYED: &str = "a replayed receipt is read back from the journal the caller named, \
     which this process did not write and cannot vouch for: the verifier signs only a verdict it \
     just produced";
pub const KEY_REACHABLE: &str = "the admitted grant gives the check workload file, network or \
     exec effects on a backend that cannot path-scope them, so the workload could have read the \
     signing key: no attestation";

/// `Ok` iff the verifier may sign the receipt this call produced for `req`.
///
/// * only a verdict: a `registered_check` …
/// * … of an operator-registered suite (`check:<id>`), never candidate bytes;
/// * only one THIS process produced. A replay returns whatever the named
///   journal holds — and the journal path is the caller's — so signing a
///   replay would sign a receipt the caller wrote (a signing oracle);
/// * only if the workload could not have read the key: it ran in the
///   protected microVM, or its grant gave it no effect at all.
pub fn attestation_decision(
    req: &ComputeRequest,
    replayed: bool,
    ran_under: Option<&RanUnder>,
) -> Result<(), &'static str> {
    if req.job_kind != JobKind::RegisteredCheck {
        return Err(NOT_A_CHECK);
    }
    if !req.argv.first().is_some_and(|a| a.starts_with("check:")) {
        return Err(CANDIDATE_RUBRIC);
    }
    if replayed {
        return Err(REPLAYED);
    }
    match ran_under {
        Some(r) if r.backend == LINUX_MICROVM_PROTECTED.id || r.effect_ceiling.is_empty() => Ok(()),
        _ => Err(KEY_REACHABLE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::LOCAL_INTERPRETER;
    use serde_json::json;

    fn req(kind: &str, argv0: &str) -> ComputeRequest {
        let mut v: serde_json::Value = serde_json::from_str(include_str!(
            "../../axon-loop-contracts/tests/fixtures/acf/request_offline.json"
        ))
        .unwrap();
        v["job_kind"] = json!(kind);
        v["argv"] = json!([argv0, "t"]);
        serde_json::from_value(v).unwrap()
    }

    fn ran(backend: &str, ceiling: &str) -> RanUnder {
        RanUnder {
            backend: backend.into(),
            effect_ceiling: ceiling.into(),
        }
    }

    #[test]
    fn the_microvm_signs_even_with_effects() {
        let r = req("registered_check", "check:acc@1");
        let vm = ran(LINUX_MICROVM_PROTECTED.id, "IO,Exec");
        assert_eq!(attestation_decision(&r, false, Some(&vm)), Ok(()));
    }

    #[test]
    fn an_effectless_local_run_signs_and_an_effectful_one_does_not() {
        let r = req("registered_check", "check:acc@1");
        let pure = ran(LOCAL_INTERPRETER.id, "");
        assert_eq!(attestation_decision(&r, false, Some(&pure)), Ok(()));
        let io = ran(LOCAL_INTERPRETER.id, "IO");
        assert_eq!(
            attestation_decision(&r, false, Some(&io)),
            Err(KEY_REACHABLE)
        );
        assert_eq!(attestation_decision(&r, false, None), Err(KEY_REACHABLE));
    }

    /// The signing oracle: whatever a replayed journal claims the op ran
    /// under — the microVM, no effects — a replay is never signed.
    #[test]
    fn a_replay_is_never_signed_whatever_its_journal_claims() {
        let r = req("registered_check", "check:acc@1");
        for claimed in [
            ran(LINUX_MICROVM_PROTECTED.id, ""),
            ran(LOCAL_INTERPRETER.id, ""),
        ] {
            assert_eq!(
                attestation_decision(&r, true, Some(&claimed)),
                Err(REPLAYED)
            );
        }
    }

    #[test]
    fn only_a_registered_suite_check_is_signed() {
        let vm = ran(LINUX_MICROVM_PROTECTED.id, "");
        assert_eq!(
            attestation_decision(&req("interpreter_run", "check:acc@1"), false, Some(&vm)),
            Err(NOT_A_CHECK)
        );
        assert_eq!(
            attestation_decision(&req("registered_check", "checks/acc.ax"), false, Some(&vm)),
            Err(CANDIDATE_RUBRIC)
        );
    }
}
