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
pub const WRONG_CLASS: &str = "the receipt's evidence class does not match the backend that \
     produced it (a guest verdict is protected or guest-unobserved; a local one is development): \
     the verifier signs a class only where it was derived";
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
///   protected microVM, or its grant gave it no effect at all;
/// * only a receipt whose EVIDENCE CLASS is the one its backend derives
///   (v022-psv-protocol.md §6). The class is inside the signed receipt, so a
///   signature never upgrades it: `protected` needs the guest path, and a
///   local, effect-free check is `development` and nothing else.
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
    let Some(r) = ran_under else {
        return Err(KEY_REACHABLE);
    };
    if r.backend == LINUX_MICROVM_PROTECTED.id {
        match r.evidence_class.as_str() {
            "protected" | "guest-unobserved" => Ok(()),
            _ => Err(WRONG_CLASS),
        }
    } else if r.effect_ceiling.is_empty() {
        match r.evidence_class.as_str() {
            "development" => Ok(()),
            _ => Err(WRONG_CLASS),
        }
    } else {
        Err(KEY_REACHABLE)
    }
}

pub const NOT_AN_EXECUTION: &str =
    "a registered check is attested as a verdict, not as an execution";
pub const NOT_PROTECTED_EXECUTION: &str =
    "only an execution on the protected profile is attested: elsewhere the backend is a claim";

/// Whether Fabric attests that this EXECUTION ran on the protected profile
/// (`attestation::EXECUTION_DOMAIN`). Only a non-replayed execution job that
/// Fabric itself dispatched to the protected Linux profile qualifies: the
/// workload ran in the guest, away from the host key (dev review round
/// wf_336353cb-a2b, PSV-7).
pub fn execution_attestation_decision(
    req: &ComputeRequest,
    replayed: bool,
    ran_under: Option<&RanUnder>,
) -> Result<(), &'static str> {
    if req.job_kind == JobKind::RegisteredCheck {
        return Err(NOT_AN_EXECUTION);
    }
    if replayed {
        return Err(REPLAYED);
    }
    match ran_under {
        Some(r) if r.backend == LINUX_MICROVM_PROTECTED.id => Ok(()),
        Some(_) => Err(NOT_PROTECTED_EXECUTION),
        None => Err(KEY_REACHABLE),
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
        let class = if backend == LINUX_MICROVM_PROTECTED.id {
            "guest-unobserved"
        } else {
            "development"
        };
        ran_as(backend, ceiling, class)
    }

    fn ran_as(backend: &str, ceiling: &str, class: &str) -> RanUnder {
        RanUnder {
            backend: backend.into(),
            effect_ceiling: ceiling.into(),
            evidence_class: class.into(),
        }
    }

    /// §6: a class is signed only where its backend derives it. A local run
    /// labelled protected (or guest-unobserved), a guest receipt labelled
    /// development, and a classless receipt are all refused.
    #[test]
    fn a_class_is_signed_only_where_its_backend_derives_it() {
        let r = req("registered_check", "check:acc@1");
        for (backend, ceiling, class, want) in [
            (LINUX_MICROVM_PROTECTED.id, "", "protected", Ok(())),
            (LINUX_MICROVM_PROTECTED.id, "", "guest-unobserved", Ok(())),
            (
                LINUX_MICROVM_PROTECTED.id,
                "",
                "development",
                Err(WRONG_CLASS),
            ),
            (LINUX_MICROVM_PROTECTED.id, "", "none", Err(WRONG_CLASS)),
            (LOCAL_INTERPRETER.id, "", "development", Ok(())),
            (LOCAL_INTERPRETER.id, "", "protected", Err(WRONG_CLASS)),
            (
                LOCAL_INTERPRETER.id,
                "",
                "guest-unobserved",
                Err(WRONG_CLASS),
            ),
            (LOCAL_INTERPRETER.id, "", "none", Err(WRONG_CLASS)),
            (
                LOCAL_INTERPRETER.id,
                "IO",
                "development",
                Err(KEY_REACHABLE),
            ),
        ] {
            assert_eq!(
                attestation_decision(&r, false, Some(&ran_as(backend, ceiling, class))),
                want,
                "{backend} {ceiling:?} {class}"
            );
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

    /// PSV-7 (dev round wf_336353cb-a2b): Fabric attests an EXECUTION only
    /// when it dispatched it to the protected profile itself, and not replayed.
    #[test]
    fn only_a_protected_profile_execution_is_attested() {
        let exec = req("interpreter_run", "prog.ax");
        let vm = ran_as(LINUX_MICROVM_PROTECTED.id, "IO", "protected");
        assert_eq!(
            execution_attestation_decision(&exec, false, Some(&vm)),
            Ok(())
        );
        let local = ran_as(LOCAL_INTERPRETER.id, "", "development");
        assert_eq!(
            execution_attestation_decision(&exec, false, Some(&local)),
            Err(NOT_PROTECTED_EXECUTION)
        );
        assert_eq!(
            execution_attestation_decision(&exec, true, Some(&vm)),
            Err(REPLAYED)
        );
        assert_eq!(
            execution_attestation_decision(&exec, false, None),
            Err(KEY_REACHABLE)
        );
        assert_eq!(
            execution_attestation_decision(
                &req("registered_check", "check:acc@1"),
                false,
                Some(&vm)
            ),
            Err(NOT_AN_EXECUTION)
        );
    }
}
