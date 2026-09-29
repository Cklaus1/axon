//! When the verifier identity may sign a receipt (G01-r22-independent-issuer).
//!
//! One pure decision so every branch — including the protected-microVM one no
//! integration environment here can exercise — is tested directly. The binary
//! signs exactly when this returns `Ok`, and prints the `Err` as
//! `attestation_withheld`.

use crate::backend::LINUX_MICROVM_PROTECTED;
use crate::psv::EvidenceClass;
use crate::submit::RanUnder;
use axon_loop_contracts::{ComputeRequest, ExecutionReceipt, JobKind};

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

pub const UNOBSERVED_EXECUTION: &str = "the receipt carries no protected class, launch-manifest \
     digest and preflight-observation digest: a launch on the protected profile that was not \
     observed is not a protected execution, and is never attested as one";

/// Whether Fabric attests that this EXECUTION ran on the protected profile
/// (`attestation::EXECUTION_DOMAIN`). Only a non-replayed execution job that
/// Fabric itself dispatched to the protected Linux profile qualifies: the
/// workload ran in the guest, away from the host key (dev review round
/// wf_336353cb-a2b, PSV-7) — and only when its RECEIPT carries the observed
/// launch: the protected class, the launch-manifest digest and the preflight
/// observation digest. A protected-profile run is an observed run or nothing;
/// the backend name alone attested an `interpreter_run` launched with no
/// manifest, nonce or observation (PSV-6, C9 dev review round 1; A54).
pub fn execution_attestation_decision(
    req: &ComputeRequest,
    receipt: &ExecutionReceipt,
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
        Some(r) if r.backend == LINUX_MICROVM_PROTECTED.id => {}
        Some(_) => return Err(NOT_PROTECTED_EXECUTION),
        None => return Err(KEY_REACHABLE),
    }
    if !observed_launch(receipt) {
        return Err(UNOBSERVED_EXECUTION);
    }
    Ok(())
}

/// The receipt states an observed protected launch: its one class is
/// `protected`, and it names the launch manifest and the preflight observation.
fn observed_launch(r: &ExecutionReceipt) -> bool {
    let names = |prefix: &str| {
        r.evidence_refs.iter().any(|e| {
            e.as_str()
                .strip_prefix(prefix)
                .is_some_and(|h| !h.is_empty())
        })
    };
    EvidenceClass::of_receipt(r) == Some(EvidenceClass::Protected)
        && names("launch-manifest-sha256:")
        && names("preflight-observation-sha256:")
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
    /// under — the microVM, no effects, a signable class — a replay is never
    /// signed. This is the decision's own contract, and REPLAYED is its only
    /// guard for a replay whose `ran_under` names a class the backend derives
    /// (C9 round 1b, M01). The binary's replay path also stamps the class
    /// `unknown` (`submit::ran_under_of`), which `WRONG_CLASS` refuses: defence
    /// in depth, not a substitute for this rule.
    #[test]
    fn a_replay_is_never_signed_whatever_its_journal_claims() {
        let r = req("registered_check", "check:acc@1");
        for claimed in [
            ran(LINUX_MICROVM_PROTECTED.id, ""),
            ran_as(LINUX_MICROVM_PROTECTED.id, "", "protected"),
            ran(LOCAL_INTERPRETER.id, ""),
        ] {
            let got = attestation_decision(&r, true, Some(&claimed));
            assert!(
                got.is_err(),
                "ATTACK: a replayed receipt was signed (ran_under {claimed:?}): {got:?}"
            );
            assert_eq!(got, Err(REPLAYED));
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

    fn receipt(refs: &[&str]) -> ExecutionReceipt {
        let mut v: serde_json::Value = serde_json::from_str(include_str!(
            "../../axon-loop-contracts/tests/fixtures/acf/receipt_exit_zero_unverified.json"
        ))
        .unwrap();
        v["backend_profile_ref"] = json!(LINUX_MICROVM_PROTECTED.id);
        v["evidence_refs"] = json!(refs);
        serde_json::from_value(v).unwrap()
    }

    /// The refs an OBSERVED protected launch leaves in its receipt.
    const OBSERVED: [&str; 3] = [
        "evidence-class:protected",
        "launch-manifest-sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "preflight-observation-sha256:2222222222222222222222222222222222222222222222222222222222222222",
    ];

    /// PSV-7 (dev round wf_336353cb-a2b): Fabric attests an EXECUTION only
    /// when it dispatched it to the protected profile itself, and not replayed.
    #[test]
    fn only_a_protected_profile_execution_is_attested() {
        let exec = req("interpreter_run", "prog.ax");
        let observed = receipt(&OBSERVED);
        let vm = ran_as(LINUX_MICROVM_PROTECTED.id, "IO", "protected");
        assert_eq!(
            execution_attestation_decision(&exec, &observed, false, Some(&vm)),
            Ok(())
        );
        let local = ran_as(LOCAL_INTERPRETER.id, "", "development");
        assert_eq!(
            execution_attestation_decision(&exec, &observed, false, Some(&local)),
            Err(NOT_PROTECTED_EXECUTION)
        );
        assert_eq!(
            execution_attestation_decision(&exec, &observed, true, Some(&vm)),
            Err(REPLAYED)
        );
        assert_eq!(
            execution_attestation_decision(&exec, &observed, false, None),
            Err(KEY_REACHABLE)
        );
        assert_eq!(
            execution_attestation_decision(
                &req("registered_check", "check:acc@1"),
                &observed,
                false,
                Some(&vm)
            ),
            Err(NOT_AN_EXECUTION)
        );
    }

    /// PSV-6 (C9 dev review round 1; A54): an execution on the protected
    /// profile is attested only when its receipt carries the OBSERVED launch:
    /// the protected class, the launch-manifest digest and the preflight
    /// observation digest. Before this, the backend name alone was enough, so
    /// an `interpreter_run` launched with no manifest, nonce or observation
    /// got an `axon.fabric-execution/1` attestation that EVL counts. Control:
    /// the fully observed receipt is attested (above and at the end here).
    #[test]
    fn a_replayed_execution_is_never_attested_whatever_its_journal_claims() {
        // C9 round 1b: the replay check is the ONLY refusal for a replay whose
        // journal claims the protected backend AND whose receipt carries the
        // observed-launch refs; everything else about it looks attestable.
        let exec = req("interpreter_run", "prog.ax");
        let vm = ran_as(LINUX_MICROVM_PROTECTED.id, "", "protected");
        assert_eq!(
            execution_attestation_decision(&exec, &receipt(&OBSERVED), true, Some(&vm)),
            Err(REPLAYED),
            "ATTACK: a replayed execution claiming an observed protected launch was attested"
        );
        assert_eq!(
            execution_attestation_decision(&exec, &receipt(&OBSERVED), false, Some(&vm)),
            Ok(()),
            "control: the same execution, not replayed, is attested"
        );
    }

    #[test]
    fn an_unobserved_protected_profile_execution_is_never_attested() {
        let exec = req("interpreter_run", "prog.ax");
        let vm = ran_as(LINUX_MICROVM_PROTECTED.id, "", "protected");
        let [class, manifest, observation] = OBSERVED;
        for (what, refs) in [
            (
                "no evidence at all (the pre-fix interpreter_run receipt)",
                vec![],
            ),
            (
                "guest-unobserved class",
                vec!["evidence-class:guest-unobserved", manifest, observation],
            ),
            ("no launch manifest", vec![class, observation]),
            ("no preflight observation", vec![class, manifest]),
            (
                "an empty observation digest",
                vec![class, manifest, "preflight-observation-sha256:"],
            ),
            (
                "two classes",
                vec![
                    class,
                    "evidence-class:guest-unobserved",
                    manifest,
                    observation,
                ],
            ),
        ] {
            let got = execution_attestation_decision(&exec, &receipt(&refs), false, Some(&vm));
            assert_eq!(
                got,
                Err(UNOBSERVED_EXECUTION),
                "ATTACK: an unobserved protected-profile execution ({what}) was attested as a \
                 protected execution"
            );
        }
        assert_eq!(
            execution_attestation_decision(&exec, &receipt(&OBSERVED), false, Some(&vm)),
            Ok(()),
            "control: the observed launch is attested"
        );
    }
}
