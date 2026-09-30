#!/usr/bin/env python3
"""G01 mutation run: each guard LISTED BELOW, removed one at a time.

This is the set of G01 guards a mutation has been written for; it is not a proof
that no other guard exists. A guard added to the G01 paths belongs here.

For each mutation: the named test must PASS on the clean tree (baseline), and
must FAIL once that single guard is removed, with the panic that FAILS it
matching the row's ATTACK MARKER (scripts/v022_attack_markers.py): the row's
own attack got through. A failure on any other panic (another check still
refusing the attack, a reason-string mismatch, a setup panic) is
REFUSED_ELSEWHERE, reported separately and never counted killed (amendment 39). A mutation that no longer
applies (its text is absent or ambiguous) or that stops the crate compiling is
reported as such — never as killed — so a refactor cannot silently turn this
into a pass. Every file is restored and re-hashed after each mutation.

    python3 scripts/v022_g01_mutations.py [--scope=S] [--shard=K/N] [--only=M1,M2] OUT.json

--only runs a SAMPLE of the scope's rows; its output is marked and cannot be
merged as a shard.

Refuses to run on a tree with uncommitted changes under crates/: the result is
evidence about a COMMIT, and it names that commit. Exit 0 only when every
baseline passes and every mutation is KILLED by its own attack.

Each cargo run is contained by scripts/lib_bounded_run.sh (memory ceiling and
deadline).
"""

import hashlib
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
from v022_attack_markers import ATTACK_MARKERS  # noqa: E402  (per-row attack markers)

# Every cargo run this harness (and v022_paired_disable.py, which imports it)
# starts gets a per-run TMPDIR on DISK. The host's /tmp is a 12 GB tmpfs, and
# the full axon-core suite's native-parity harnesses link hundreds of binaries
# there concurrently: they failed on "No space left on device" in every full
# suite cell (M58/M60/M89 read SUITE_BROKEN, C9 round 1b; both pass alone).
# An environment failure must not read as evidence about a guard.
if not os.environ.get("V022_KEEP_TMPDIR"):
    import atexit
    import shutil
    import tempfile
    _tmp = tempfile.mkdtemp(prefix=f"v022-harness-{os.getpid()}-", dir="/var/tmp")
    # /tmp's own mode: tests assume it (the non-dumpable probe runs as nobody
    # and must reach its fixture), and mkdtemp's 0700 broke that one test.
    os.chmod(_tmp, 0o1777)
    os.environ["TMPDIR"] = _tmp
    atexit.register(shutil.rmtree, _tmp, True)

# (id, guard, file, old, new, package, target, test)
# target: "--lib" or "--test <name>"; test: the exact test path.
MUTATIONS = [
    ("M01", "Fabric never signs a replay",
     "crates/axon-fabric/src/signing.rs",
     "        return Err(CANDIDATE_RUBRIC);\n    }\n    if replayed {\n        return Err(REPLAYED);",
     "        return Err(CANDIDATE_RUBRIC);\n    }\n    if false && replayed {\n        return Err(REPLAYED);",
     # C9 round 1b: the decision's own contract (a replay whose ran_under names
     # a signable class); the integration test's replay carries class
     # `unknown`, which WRONG_CLASS also refuses (defence in depth).
     "axon-fabric", "--lib", "signing::tests::a_replay_is_never_signed_whatever_its_journal_claims"),
    ("M02", "attestation signature verification",
     "crates/axon-loop-contracts/src/attestation.rs",
     "    UnparsedPublicKey::new(&ED25519, &registered)\n        .verify(&bytes, &sig)",
     "    let _ = (&bytes, &sig);\n    Ok::<(), ()>(())",
     "axon-loop-contracts", "--lib", "attestation::tests::only_the_registered_key_is_accepted"),
    ("M03", "attestation binding comparison",
     "crates/axon-loop-contracts/src/attestation.rs",
     "        if &doc[field] != want {",
     "        if false && &doc[field] != want {",
     "axon-loop-contracts", "--lib", "attestation::tests::every_bound_field_is_load_bearing"),
    ("M04", "suite modules resolve BEFORE the candidate's (order reversed)",
     "crates/axon-fabric/src/submit.rs",
     'join(&[dir.0.join("check"), dir.0.join("candidate")])',
     'join(&[dir.0.join("candidate"), dir.0.join("check")])',
     # C9 round 2 (harness): re-anchored on the property only the order
     # guards (the security attack, a planted helper steering a broken
     # candidate to PASS, needs M436 removed too).
     "axon-fabric", "--test check_effects", "an_honest_candidate_holding_a_suite_module_name_is_judged_by_the_suite"),
    ("M05", "intake: the policy's proposer is a subject",
     "crates/axon-loop/src/intake.rs",
     "        .chain(crate::evo::proposer_in(&tx, &ep.scope, &ep.policy_ref))\n",
     "",
     "axon-loop", "--test intake", "the_proposer_of_the_policy_cannot_verify_its_episodes"),
    ("M06", "intake: the recorded suite version is the task's",
     "crates/axon-loop/src/intake.rs",
     " || recorded != acc.check_suite",
     "",
     "axon-loop", "--test intake",
     "a_verdict_from_another_pinned_version_of_the_suite_does_not_decide_the_task"),
    ("M07", "intake: the acceptance pin is THIS task's",
     "crates/axon-loop/src/intake.rs",
     '    let acc = config.task_acceptance.get(task).ok_or_else(', '    let acc = config.task_acceptance.values().next().ok_or_else(',
     "axon-loop", "--test intake", "a_verdict_on_one_tasks_check_cannot_decide_another_task"),
    ("M08", "intake: the attestation must verify",
     "crates/axon-loop/src/intake.rs",
     '    let key_id = axon_loop_contracts::attestation::verify(&att, issuer, &req, &rc, key)\n'
     '        .map_err(|e| refused(format!("verification attestation refused: {e}")))?;',
     '    let key_id = axon_loop_contracts::attestation::verify(&att, issuer, &req, &rc, key)\n'
     '        .unwrap_or_default();',
     "axon-loop", "--test intake", "each_verification_rule_is_load_bearing_on_its_own"),
    ("M09", "intake: the task/trial identity join",
     "crates/axon-loop/src/intake.rs",
     "    let same = req.task_id == id.task_id\n        && rc.task_id == id.task_id\n"
     "        && req.trial_id == id.trial_id\n        && rc.trial_id == id.trial_id\n        && ",
     "    let same = ",
     "axon-loop", "--test intake", "each_verification_rule_is_load_bearing_on_its_own"),
    ("M10", "intake: the issuer is not the subject",
     "crates/axon-loop/src/intake.rs",
     "        .is_some_and(|i| trusted_verifiers.contains(i) && !subject.contains(i))",
     "        .is_some_and(|i| trusted_verifiers.contains(i))",
     "axon-loop", "--test intake", "each_verification_rule_is_load_bearing_on_its_own"),
    ("M11", "EVL: a protected class counts only protected backends (D3)",
     "crates/axon-loop/src/evl.rs",
     "    if class == crate::plan::EvaluationClass::Protected {",
     "    if false && class == crate::plan::EvaluationClass::Protected {",
     "axon-loop", "--test protected_class", "a_protected_plan_counts_only_protected_backends"),
    ("M12", "activation: a protected scope needs a protected evaluation (D3)",
     "crates/axon-loop/src/pointer.rs",
     "    if eval.evaluation_class != crate::plan::EvaluationClass::Protected {",
     "    if false && eval.evaluation_class != crate::plan::EvaluationClass::Protected {",
     "axon-loop", "--test protected_class", "a_protected_scope_promotes_only_on_a_protected_evaluation"),
    ("M13", "freeze records the protected class (D3)",
     "crates/axon-loop/src/plan.rs",
     "        EvaluationClass::Protected\n    } else {",
     "        EvaluationClass::Development\n    } else {",
     "axon-loop", "--test protected_class", "a_protected_plan_counts_only_protected_backends"),
    ("M14", "EVL: a counted verdict cites its evidence",
     "crates/axon-loop/src/evl.rs",
     '                    *authenticated = Some(ev);', '                    let _ = ev;',
     "axon-loop", "--test evidence_laundering",
     "a_counted_verdict_cites_the_evidence_it_was_authenticated_on"),
    ("M15", "EVL: only an intaken episode is evaluated (ADR-001 §8)",
     "crates/axon-loop/src/evl.rs",
     "let ctx_check = if let Err(e) = intake_join(&intaken, d) {",
     "let ctx_check = if let Err(e) = Ok::<(), String>(()) {",
     "axon-loop", "--test evidence_laundering", "an_episode_intake_never_recorded_never_counts"),
    ("M16", "EVL: the trial's observer is a subject (as at intake)",
     "crates/axon-loop/src/evl.rs",
     ".chain([d.ctx.observed_issuer_ref.clone()])",
     ".chain(std::iter::empty::<OpaqueRef>())",
     "axon-loop", "--test evidence_laundering", "a_check_run_as_the_observer_never_counts_at_either_door"),
    ("M17", "D3: a protected scope serves no mechanism-test fixture",
     "crates/axon-loop/src/pointer.rs",
     "    if t.mechanism_test {\n        return Err(refused(\n            \"scope is protected: it serves no mechanism-test fixture",
     "    if false && t.mechanism_test {\n        return Err(refused(\n            \"scope is protected: it serves no mechanism-test fixture",
     "axon-loop", "--test protected_class", "a_protected_class_mechanism_test_is_still_not_served"),
    ("M18", "D3: rollback passes the protected-scope gate",
     "crates/axon-loop/src/pointer.rs",
     "            if config.protected_scopes.contains(scope) {\n                protected_scope_gate(",
     "            if t.kind != TransitionKind::Rollback && config.protected_scopes.contains(scope) {\n                protected_scope_gate(",
     "axon-loop", "--test protected_class", "a_rollback_in_a_protected_scope_needs_a_protected_admission"),
    ("M19", "D3: the execution leg is checked",
     "crates/axon-loop/src/evl.rs",
     '("execution", Some(rcpt.backend_profile_ref.as_str())),', '("execution", None),',
     "axon-loop", "--test protected_class", "an_attested_development_execution_leg_counts_nothing"),
    ("M20", "D3: the verification leg is checked",
     "crates/axon-loop/src/evl.rs",
     '                "verification",\n                d.verification[1]["backend_profile_ref"].as_str(),',
     '                "verification",\n                None,',
     "axon-loop", "--test protected_class", "a_cited_unknown_from_a_development_verification_is_unverifiable"),
    ("M21", "intake: a verdict's checked tree is the episode's output (failed too)",
     "crates/axon-loop/src/intake.rs",
     "        || ep.output_workspace_ref.as_ref() != checked\n",
     "",
     "axon-loop", "--test intake", "verification_that_does_not_join_is_refused_with_the_store_unchanged"),
    ("M22", "intake: the check receipt is supervisor-observed",
     "crates/axon-loop/src/intake.rs",
     "    if rc.evidence_source != EvidenceSource::SupervisorObserved {",
     "    if false && rc.evidence_source != EvidenceSource::SupervisorObserved {",
     "axon-loop", "--test intake", "verification_that_does_not_join_is_refused_with_the_store_unchanged"),
    ("M23", "intake: the receipt records exactly one suite version",
     "crates/axon-loop/src/intake.rs",
     "        [one] => *one,",
     "        [one, ..] => *one,",
     "axon-loop", "--test intake", "verification_that_does_not_join_is_refused_with_the_store_unchanged"),
    ('M24', 'intake: the receipt is the one verifier_ref cites',
     'crates/axon-loop/src/intake.rs',
     '    if &rc_ref != vref {',
     '    if false && &rc_ref != vref {',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M25', 'intake: evidence_refs is exactly the check request',
     'crates/axon-loop/src/intake.rs',
     '    if v.evidence_refs != [req_ref.clone()] {',
     '    if false && v.evidence_refs != [req_ref.clone()] {',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M26', "intake: the verdict's backend profile is pinned",
     'crates/axon-loop/src/intake.rs',
     '    if !pin\n        .backend_profiles',
     '    if false && !pin\n        .backend_profiles',
     'axon-loop', '--test intake', 'a_verdict_counts_only_for_what_the_operator_pinned'),
    ('M27', 'intake: the check is an operator suite (check:<id>)',
     'crates/axon-loop/src/intake.rs',
     '    let Some(suite_id) = entry.strip_prefix("check:") else {',
     '    let Some(suite_id) = entry.strip_prefix("check:").or(Some(entry)) else {',
     'axon-loop', '--test intake', 'a_bare_candidate_file_named_like_the_suite_never_defines_the_rubric'),
    ('M28', 'intake: the recorded suite is pinned for this verifier',
     'crates/axon-loop/src/intake.rs',
     '        || !pin.check_suites.iter().any(|p| p == recorded)\n',
     '\n',
     'axon-loop', '--test intake', 'a_verdict_counts_only_for_what_the_operator_pinned'),
    ('M29', 'intake: the recorded suite is the one argv named',
     'crates/axon-loop/src/intake.rs',
     # C9 r3 (loop, A81): re-anchored; the recorded id is read by the one parser.
     '    if axon_loop_contracts::suite::parse_check_suite_ref(recorded).map(|(id, _, _)| id)\n        != Ok(suite_id)\n        ||',
     '    if false\n        ||',
     'axon-loop', '--test intake', 'a_verdict_recorded_for_a_suite_the_request_did_not_name_is_refused'),
    ('M30', "intake: argv is the task's registered acceptance check",
     'crates/axon-loop/src/intake.rs',
     '    if req.argv != [format!("check:{acc_suite}"), acc.check.clone()] || recorded != acc.check_suite',
     '    if recorded != acc.check_suite',
     'axon-loop', '--test intake', 'each_verification_rule_is_load_bearing_on_its_own'),
    ('M31', 'intake: the verification is a registered_check',
     'crates/axon-loop/src/intake.rs',
     '    if req.job_kind != JobKind::RegisteredCheck {',
     '    if false && req.job_kind != JobKind::RegisteredCheck {',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M32', 'intake: attempt/operation/execution ids join',
     'crates/axon-loop/src/intake.rs',
     '        && req.attempt_id == id.attempt_id\n        && rc.attempt_id == id.attempt_id\n        && req.operation_id == id.operation_id\n        && rc.operation_id == id.operation_id\n        && rc.execution_id == id.execution_id;',
     ';',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M33', "intake: the receipt's input tree is the request's",
     'crates/axon-loop/src/intake.rs',
     '    if Some(&rc.input_workspace_ref) != checked\n        || ',
     '    if false\n        || ',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M34', "intake: the sidecar's verified tree is the request's",
     'crates/axon-loop/src/intake.rs',
     '        || v.output_workspace_ref.as_ref() != checked\n',
     '',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M35', "intake: the sidecar's result is the receipt's",
     'crates/axon-loop/src/intake.rs',
     '    if v.result != from_receipt {',
     '    if false && v.result != from_receipt {',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M36', "intake: matched_checks is the receipt's",
     'crates/axon-loop/src/intake.rs',
     '    if v.matched_checks != rc.matched_checks.unwrap_or(0) {',
     '    if false && v.matched_checks != rc.matched_checks.unwrap_or(0) {',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M37', 'EVL: arm proposers are subjects',
     'crates/axon-loop/src/evl.rs',
     '            subjects.insert(p);',
     '            let _ = p;',
     'axon-loop', '--test redteam', 'ns4p_ns4w_a_subject_is_never_a_trusted_observer_of_its_own_trials'),
    ('M38', 'EVL: the evaluator is not a subject',
     'crates/axon-loop/src/evl.rs',
     '    if subjects.contains(&r.evaluator_ref) {',
     '    if false && subjects.contains(&r.evaluator_ref) {',
     'axon-loop', '--test evl_admission', 'evl_refusals_write_nothing'),
    ('M39', 'EVL: no self-observation',
     'crates/axon-loop/src/evl.rs',
     '                } else if subjects.contains(&d.ctx.observed_issuer_ref) {',
     '                } else if false && subjects.contains(&d.ctx.observed_issuer_ref) {',
     'axon-loop', '--test redteam', 'ns4p_ns4w_a_subject_is_never_a_trusted_observer_of_its_own_trials'),
    ('M40', 'signing: only a registered_check is signed',
     'crates/axon-fabric/src/signing.rs',
     '    if req.job_kind != JobKind::RegisteredCheck {\n        return Err(NOT_A_CHECK);',
     '    if false && req.job_kind != JobKind::RegisteredCheck {\n        return Err(NOT_A_CHECK);',
     'axon-fabric', '--lib', 'signing::tests::only_a_registered_suite_check_is_signed'),
    ('M41', 'signing: only an operator suite is signed',
     'crates/axon-fabric/src/signing.rs',
     '    if !req.argv.first().is_some_and(|a| a.starts_with("check:")) {',
     '    if false && !req.argv.first().is_some_and(|a| a.starts_with("check:")) {',
     'axon-fabric', '--lib', 'signing::tests::only_a_registered_suite_check_is_signed'),
    ('M42', 'signing: only if the workload could not reach the key', 'crates/axon-fabric/src/signing.rs', '    } else if r.effect_ceiling.is_empty() {', '    } else if true {', 'axon-fabric', '--lib', 'signing::tests::an_effectless_local_run_signs_and_an_effectful_one_does_not'),
    ('M43', "Fabric: the suite's entry is in the signed identity",
     'crates/axon-fabric/src/submit.rs',
     # C9 r2 (rows): re-anchored; the ref is now written by runner::check_suite_ref.
     # C9 r3 (loop, A81): re-anchored; written once, where the suite resolves.
     'axon_cortex::runner::check_suite_ref(id, version.as_str(), &c.entry)',
     'Ok::<String, String>(format!("check-suite:{}@{}", id, version))',
     'axon-fabric', '--test attestation', 'fabric_signs_as_the_operators_signer_and_only_when_the_workload_cannot_reach_the_key'),
    ('M44', 'Fabric: a check runs from an empty environment',
     'crates/axon-fabric/src/submit.rs',
     '        .with_clean_env();',
     ';',
     'axon-fabric', '--test attestation', 'the_launchers_environment_does_not_steer_a_signed_verdict'),
    ('M45', "intake: no role upgrade (the check's own documents as execution refs)",
     'crates/axon-loop/src/intake.rs',
     '        if [&ep.context_ref, &ep.acf_request_ref, &ep.acf_receipt_ref].contains(&r) {',
     '        if false && [&ep.context_ref, &ep.acf_request_ref, &ep.acf_receipt_ref].contains(&r) {',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M46', 'intake: the check did not run as a subject principal',
     'crates/axon-loop/src/intake.rs',
     '    if subject.contains(&req.principal_ref) {',
     '    if false && subject.contains(&req.principal_ref) {',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M47', 'intake: the verifier revision (executable and digest) is pinned',
     'crates/axon-loop/src/intake.rs',
     '    if req.registered_executable_ref.as_str() != pin.registered_executable_ref\n        || req.executable_digest.as_str() != pin.executable_digest\n    {',
     '    if false {',
     'axon-loop', '--test intake', 'a_verdict_counts_only_for_what_the_operator_pinned'),
    ('M48', 'intake: the record names the attestation that authenticated it',
     'crates/axon-loop/src/intake.rs',
     '        verification_attestation_ref: attestation_ref,',
     '        verification_attestation_ref: None,',
     'axon-loop', '--test intake', 'a_fabric_check_on_the_output_tree_is_recorded_as_the_verification'),
    ('M49', 'Fabric: a named pass needs exit 0',
     'crates/axon-fabric/src/submit.rs',
     '            if verification == ReceiptVerification::Passed && exit != Some(0) {',
     '            if false && verification == ReceiptVerification::Passed && exit != Some(0) {',
     'axon-fabric', '--test check_effects', 'a_named_pass_in_a_run_that_exits_nonzero_is_not_a_pass'),
    ('M50', "admission: a revoked verifier's verdicts stop counting",
     'crates/axon-loop/src/admission.rs',
     '            if !verifiers.contains(&v.issuer_ref)\n',
     '            if false && !verifiers.contains(&v.issuer_ref)\n',
     'axon-loop', '--test evl_admission', 'a_revoked_verifier_s_verdicts_stop_counting'),
    ('M51', "rollback: the predecessor's authority is re-validated",
     'crates/axon-loop/src/pointer.rs',
     '        if !admitters.contains(&b.issuer_ref) {\n            return Err(refused(\n                "rollback target\'s baseline issuer',
     '        if false && !admitters.contains(&b.issuer_ref) {\n            return Err(refused(\n                "rollback target\'s baseline issuer',
     'axon-loop', '--test protected_class', 'a_rollback_revalidates_its_predecessor'),
    ('M52', 'Fabric: a state dir that would split the module path is refused',
     'crates/axon-fabric/src/submit.rs',
     "    if cfg.state_dir.as_os_str().to_string_lossy().contains(':') {",
     "    if false && cfg.state_dir.as_os_str().to_string_lossy().contains(':') {",
     'axon-fabric', '--test check_effects', 'a_state_dir_that_would_split_the_module_path_is_refused'),
    ('M53', 'Fabric: a check resolves modules only from its module path',
     'crates/axon-fabric/src/submit.rs',
     '        .with_env("AXON_PATH_EXCLUSIVE", "1");',
     ';',
     'axon-fabric', '--test check_effects', 'a_suite_module_never_resolves_from_the_trial_cache'),
    ('M54', 'intake: one verdict decides one trial in one scope',
     'crates/axon-loop/src/intake.rs',
     '                if intake.scope != ep.scope\n',
     '                if false && intake.scope != ep.scope\n',
     'axon-loop', '--test intake', 'one_verdict_decides_one_trial_in_one_scope'),
    ('M55', 'executor: the empty ceiling is applied, not just intended',
     'crates/axon-cortex/src/runner.rs',
     '        if let Some(c) = &self.limits.effect_ceiling {\n            cmd.env("AXON_ALLOWED_EFFECTS", c);',
     '        if let Some(c) = self.limits.effect_ceiling.as_ref().filter(|c| !c.is_empty()) {\n            cmd.env("AXON_ALLOWED_EFFECTS", c);',
     'axon-fabric', '--test attestation', 'the_empty_ceiling_is_applied_not_just_intended'),
    # M56 (the test runner's escaped-break arm) is RETIRED as an equivalent
    # mutant since bcf9c0a7: call_fn now stops loop control at every function
    # boundary, the test fn's own included, so that arm cannot be reached. It
    # stays in the code as defense in depth; M58 is the guard that decides.
    ('M57', 'Fabric: a candidate holding a symlink is refused',
     'crates/axon-fabric/src/submit.rs',
     '    refuse_links(\n        cv.entries\n            .iter()\n            .map(|e| (e.path.as_str(), e.mode == workspace::MODE_LINK)),\n        "the candidate",\n    )?;',
     '    let _ = &cv;',
     'axon-fabric', '--test check_effects', 'a_candidate_holding_a_symlink_is_refused'),
    ('M58', 'interpreter: loop control does not cross a function boundary', 'crates/axon-core/src/interp.rs', '            Err(Flow::Break) | Err(Flow::Continue) => {\n                return panic(format!("`break`/`continue` outside a loop in `{}`", f.name))\n            }\n', '', 'axon-core', '--no-default-features --lib', 'interp::tests::an_escaped_break_or_continue_from_a_function_body_does_not_pass_a_test'),
    ('M59', 'interpreter: loop control is contained at every frame edge (fn, closure, predicate)',
     'crates/axon-core/src/interp.rs',
     '            Flow::Break | Flow::Continue => {\n                panic(format!("`break`/`continue` outside a loop in {site}"))\n            }',
     '            Flow::Break => Err(Flow::Break),\n            Flow::Continue => Err(Flow::Continue),',
     'axon-core', '--no-default-features --lib', 'interp::tests::loop_control_does_not_escape_through_a_predicate'),
    ('M60', 'resolver: a trait impl is unique in the merged program',
     'crates/axon-core/src/resolver.rs',
     '                    if !impls.insert(key) {',
     '                    if false && !impls.insert(key) {',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_second_impl_never_replaces_the_first_impls_method'),
    ('M61', 'resolver: a module-level let is unique in the merged program',
     'crates/axon-core/src/resolver.rs',
     '                        if !matches!(prev, Symbol::Builtin { .. }) {\n                            dup(self, format!("the constant `{name}`"), *span);',
     '                        if false && !matches!(prev, Symbol::Builtin { .. }) {\n                            dup(self, format!("the constant `{name}`"), *span);',
     'axon-core', '--no-default-features --lib', 'resolver::tests::duplicate_let_refinement_or_impl_produces_e0002'),
    ('M62', 'resolver: a named refinement is unique in the merged program',
     'crates/axon-core/src/resolver.rs',
     '                    if !refinements.insert(r.name.as_str()) {',
     '                    if false && !refinements.insert(r.name.as_str()) {',
     'axon-core', '--no-default-features --lib', 'resolver::tests::duplicate_let_refinement_or_impl_produces_e0002'),
    ('M63', 'Fabric: a pass needs completion evidence for the test it rests on',
     'crates/axon-fabric/src/submit.rs',
     '            if let (ReceiptVerification::Passed, Some(key)) = (verification, completion_key) {',
     '            if let (ReceiptVerification::Passed, Some(key)) = (verification, completion_key.filter(|_| false)) {',
     'axon-fabric', '--test check_effects', 'a_pass_needs_evidence_that_the_test_completed'),
    ('M64', 'Fabric: the completion token must verify under the run key (a forged one does not)',
     'crates/axon-fabric/src/submit.rs',
     '                    !rep.completion.iter().any(|(a, t)| a == n && *t == want)',
     '                    !rep.completion.iter().any(|(a, t)| a == n && (*t == want || !t.is_empty()))',
     'axon-fabric', '--test check_effects', 'a_pass_needs_evidence_that_the_test_completed'),
    ('M65', 'interpreter: a test whose body evaluates to Err did not complete',
     'crates/axon-core/src/interp.rs',
     '        Ok(Value::Err(_)) => Ok(TestEnd::EndedEarly(',
     '        Ok(Value::Err(_)) if false => Ok(TestEnd::EndedEarly(',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_test_completes_only_when_its_body_returns_normally'),
    ('M67', 'interpreter: exit(0) inside a test is not completion',
     'crates/axon-core/src/interp.rs',
     '        Err(Flow::Exit(0)) => Ok(TestEnd::EndedEarly(\n            "`exit(0)` ended the test before it completed".to_string(),\n        )),',
     '        Err(Flow::Exit(0)) => Ok(TestEnd::Completed),',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_test_completes_only_when_its_body_returns_normally'),
    ('M68', 'axon test: a completion token is issued only for a completed test',
     'crates/axon-core/src/main.rs',
     '.filter(|_| r.completed)',
     '.filter(|_| true)',
     'axon-core', '--no-default-features --test test_completion', 'only_a_completed_test_is_issued_a_completion_token'),
    ('M69', 'resolver: one method per (dispatch type, name) across all impls',
     'crates/axon-core/src/resolver.rs',
     '                            if !dispatch.insert((tn.clone(), m.name.clone())) {',
     '                            if false && !dispatch.insert((tn.clone(), m.name.clone())) {',
     'axon-core', '--no-default-features --lib', 'resolver::tests::duplicate_let_refinement_or_impl_produces_e0002'),
    ('M70', 'resolver: a trait name is unique in the merged program',
     'crates/axon-core/src/resolver.rs',
     '                    if !traits.insert(t.name.as_str()) {',
     '                    if false && !traits.insert(t.name.as_str()) {',
     'axon-core', '--no-default-features --lib', 'resolver::tests::duplicate_let_refinement_or_impl_produces_e0002'),
    ('M71', 'runner: a check runs in its own workspace, not the launcher\'s cwd',
     'crates/axon-cortex/src/runner.rs',
     '        cmd.current_dir(req.workspace)\n            .arg("test")',
     '        cmd.arg("test")',
     'axon-fabric', '--test check_effects', 'a_suites_runtime_fixture_is_the_pinned_one'),
    ('M72', 'interpreter: a `return` cannot leave its frame (predicates, callee edge)',
     'crates/axon-core/src/interp.rs',
     '            Flow::Return(_) => panic(format!("`return` escaped {site}")),',
     '            Flow::Return(v) => Err(Flow::Return(v)),',
     'axon-core', '--no-default-features --lib', 'interp::tests::no_control_transfer_escapes_a_frame'),
    ('M73', 'interpreter: a `resume` cannot leave its frame (smuggled closure)',
     'crates/axon-core/src/interp.rs',
     '            Flow::Resume(_) => panic(format!("`resume` escaped {site}")),',
     '            Flow::Resume(v) => Err(Flow::Resume(v)),',
     'axon-core', '--no-default-features --lib', 'interp::tests::no_control_transfer_escapes_a_frame'),
    ('M74', 'interpreter: a handler completion is caught only by the `with` that installed it',
     'crates/axon-core/src/interp/eval.rs',
     '            Err(Flow::HandlerDone(v, d)) if d == depth => v,',
     '            Err(Flow::HandlerDone(v, _)) => v,',
     'axon-core', '--no-default-features --lib', 'interp::tests::no_control_transfer_escapes_a_frame'),
    ('M75', 'resolver: a refinement cannot reuse a type name (builtin, struct, enum)',
     'crates/axon-core/src/resolver.rs',
     '                if taken {',
     '                if false && taken {',
     'axon-core', '--no-default-features --lib', 'resolver::tests::duplicate_let_refinement_or_impl_produces_e0002'),
    ('M76', 'interpreter: assert_eq_f64 fails on NaN',
     'crates/axon-core/src/interp/builtins.rs',
     '                if !(a == b || (a - b).abs() <= 1e-9) {',
     '                if (a - b).abs() > 1e-9 {',
     'axon-core', '--no-default-features --lib', 'interp::tests::no_control_transfer_escapes_a_frame'),
    ('M77', 'interpreter: an exit(0) property case is not a pass',
     'crates/axon-core/src/interp/proptest.rs',
     '        Err(Flow::Exit(0)) => {\n            Err("`exit(0)` ended the property case before it completed".to_string())\n        }',
     '        Err(Flow::Exit(0)) => Ok(()),',
     'axon-core', '--no-default-features --lib', 'interp::tests::no_control_transfer_escapes_a_frame'),
    ('M78', 'resolver: a fn name is unique in the merged program',
     'crates/axon-core/src/resolver.rs',
     '                        } else {\n                            // True user↔user duplicate.\n                            self.emit_error(',
     '                        } else if false {\n                            // True user↔user duplicate.\n                            self.emit_error(',
     'axon-core', '--no-default-features --lib', 'resolver::tests::duplicate_fn_name_produces_e0002'),
    ('M79', 'resolver: a sealed module cannot reach a name the operator defines (E0004)',
     'crates/axon-core/src/resolver.rs',
     '    if !sealed.is_empty() {\n        r.check_sealed(program, sealed);\n    }',
     '    let _ = sealed;',
     'axon-core', '--no-default-features --lib', 'resolver::tests::a_sealed_module_cannot_reach_the_operators_names'),
    ('M80', 'Fabric: a registered suite runs with the candidate sealed',
     'crates/axon-fabric/src/submit.rs',
     '            l = l.with_sealed_dir(cand);',
     '            let _ = cand;',
     'axon-fabric', '--test check_effects', 'a_sealed_candidate_cannot_reach_the_operators_names'),
    ('M81', 'runner: the sealed dirs reach `axon test --seal`',
     'crates/axon-cortex/src/runner.rs',
     '            cmd.arg("--seal").arg(d);',
     '            let _ = d;',
     'axon-fabric', '--test check_effects', 'a_sealed_candidate_cannot_reach_the_operators_names'),
    ('M82', 'resolver: a refinement cannot reuse a generic parameter name',
     'crates/axon-core/src/resolver.rs',
     '                    || generic_names.contains(n)\n',
     '',
     'axon-core', '--no-default-features --lib', 'resolver::tests::duplicate_let_refinement_or_impl_produces_e0002'),
    ('M83', 'parser: a synthetic inline refinement is unique per file',
     'crates/axon-core/src/parser.rs',
     '            format!("__refine_{}_{}", self.source.0, self.synthetic_refine_count)',
     '            format!("__refine_{}", self.synthetic_refine_count)',
     'axon-core', '--no-default-features --lib', 'resolver::tests::two_files_inline_refinements_never_share_a_name'),
    ('M84', 'Cortex via Fabric: only a Fabric verdict of passed/failed is a verdict',
     'crates/axon-cortex/src/runner.rs',
     '            Some("passed") | Some("failed") => {}',
     '            Some(_) | None => {}',
     'axon-fabric', '--test check_effects', 'the_cortex_executor_accepts_only_a_receipt_that_binds_its_candidate'),
    ('M85', 'resolver: AXON_PATH_EXCLUSIVE removes the ambient module dirs',
     'crates/axon-core/src/lib.rs',
     '    if std::env::var("AXON_PATH_EXCLUSIVE").as_deref() == Ok("1") {\n        return dirs;\n    }',
     '',
     'axon-core', '--no-default-features --test pci_isolation', 'an_exclusive_module_path_never_falls_through_to_ambient_dirs'),
    ('M86', 'interpreter: the CALL edge — a sealed frame runs only sealed functions',
     'crates/axon-core/src/interp.rs',
     '        self.seal_call(f)?;\n        // The WHOLE call',
     '        // The WHOLE call',
     'axon-core', '--no-default-features --lib', 'interp::tests::runtime_sealing_holds_without_the_static_check'),
    ('M87', 'interpreter: the GLOBAL-READ edge — a sealed frame reads no operator global',
     'crates/axon-core/src/interp.rs',
     '        if self.seal.active && self.frame_sealed.get() && !self.seal.globals.contains(name) {',
     '        if false && self.seal.active && self.frame_sealed.get() && !self.seal.globals.contains(name) {',
     'axon-core', '--no-default-features --lib', 'interp::tests::runtime_sealing_holds_without_the_static_check'),
    ('M88', 'interpreter: the REFINEMENT edge — a candidate refinement never runs in operator code',
     'crates/axon-core/src/interp.rs',
     '        if self.seal.active && !self.frame_sealed.get() && self.seal.refines.contains(rname) {',
     '        if false && self.seal.active && !self.frame_sealed.get() && self.seal.refines.contains(rname) {',
     'axon-core', '--no-default-features --lib', 'interp::tests::runtime_sealing_holds_without_the_static_check'),
    ('M89', 'interpreter: a name a sealed frame queues is seal-checked when resolved',
     'crates/axon-core/src/interp/builtins.rs',
     '                if self.fn_by_name(&fn_name)?.is_none() {',
     '                if !self.fns.contains_key(&fn_name) {',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_sealed_fiber_never_runs_an_operator_function'),
    ('M90', 'interpreter: a closure runs under the provenance of the frame that created it',
     'crates/axon-core/src/interp.rs',
     '        let origin = self.seal.active && captured.borrow().contains_key(SEALED_CLOSURE_MARK);',
     '        let origin = false;',
     'axon-core', '--no-default-features --lib', 'interp::tests::runtime_sealing_holds_without_the_static_check'),
    ('M91', 'interpreter: a closure created in a sealed frame is marked sealed',
     'crates/axon-core/src/interp/eval.rs',
     '                if self.frame_sealed.get() {\n                    cell.insert(',
     '                if false {\n                    cell.insert(',
     'axon-core', '--no-default-features --lib', 'interp::tests::runtime_sealing_holds_without_the_static_check'),
    ('M92', 'interpreter: a handler arm runs under the provenance of its `with`',
     'crates/axon-core/src/interp/eval.rs',
     '            sealed: self.frame_sealed.get(),',
     '            sealed: false,',
     'axon-core', '--no-default-features --lib', 'interp::tests::runtime_sealing_holds_without_the_static_check'),
    ('M93', 'ast: walk_expr visits match-arm guards',
     'crates/axon-core/src/ast.rs',
     '                if let Some(g) = &a.guard {\n                    walk_expr(g, f);\n                }',
     '',
     'axon-core', '--no-default-features --lib', 'resolver::tests::a_sealed_module_cannot_reach_the_operators_names'),
    ('M94', 'resolver: a refinement cannot take a deferred-prefix type name',
     'crates/axon-core/src/resolver.rs',
     '                    || ["Uncertain", "Temporal", "Goal", "Dict"]',
     '                    || [""; 0]',
     'axon-core', '--no-default-features --lib', 'resolver::tests::duplicate_let_refinement_or_impl_produces_e0002'),
    ('M95', 'interpreter: a global initializer runs under its own provenance',
     'crates/axon-core/src/interp.rs',
     '            let sealed = self.seal.active && self.seal.globals.contains(name);',
     '            let sealed = false;',
     'axon-core', '--no-default-features --lib', 'interp::tests::runtime_sealing_holds_without_the_static_check'),
    ('M96', 'interpreter: a sealed frame has its OWN kernel (handles are per provenance)',
     'crates/axon-core/src/interp.rs',
     '        &self.kernels[usize::from(self.frame_sealed.get())]',
     '        &self.kernels[0]',
     'axon-core', '--no-default-features --lib', 'interp::tests::runtime_sealing_holds_without_the_static_check'),
    ('M97', 'interpreter: a whole-struct `where` runs under its type\'s provenance',
     'crates/axon-core/src/interp/eval.rs',
     '                                let sealed = self.frame_sealed.get() || self.seal_type(name);',
     '                                let sealed = self.frame_sealed.get();',
     'axon-core', '--no-default-features --lib', 'interp::tests::runtime_sealing_holds_without_the_static_check'),
    ('M98', 'EVL: a verdict attested before the plan froze does not count (attestation /2 issued_ms)',
     'crates/axon-loop/src/evl.rs',
     '            Ok((_, ev, issued)) if issued < freeze_ms => {', '            Ok((_, ev, issued)) if false && issued < freeze_ms => {',
     'axon-loop', '--test evl_admission', 'a_verdict_attested_before_the_freeze_does_not_count'),
    ('M99', 'EVL: a protected context is authenticated by its observer, not named',
     'crates/axon-loop/src/evl.rs',
     '    if class != crate::plan::EvaluationClass::Protected {\n        return Ok(());\n    }',
     '    if true || class != crate::plan::EvaluationClass::Protected {\n        return Ok(());\n    }',
     'axon-loop', '--test protected_class', 'a_protected_context_is_authenticated_not_named'),
    ('M100', 'intake: the episode is bound to the input workspace its observer saw',
     'crates/axon-loop-contracts/src/checks.rs',
     '    if episode.input_workspace_ref != ctx.observed.workspace_ref {',
     '    if false && episode.input_workspace_ref != ctx.observed.workspace_ref {',
     'axon-loop', '--test intake', 'an_episode_is_bound_to_the_input_workspace_its_observer_saw'),
    # Binding batch candidate 2 (review wf_d788c05a-be2): G11 x3 guards, ADR-002 and
    # the ADR-001 §3.6 population. Scope `binding`; excluded from the registered g01 scope.
    ('M101', 'transition: the issuer is independent of what it promotes',
     'crates/axon-loop/src/admission.rs',
     '    match role {\n        Some(role) => Err(refused(format!(\n            "self-promotion: transition issuer',
     '    match role {\n        _ if true => Ok(()),\n        Some(role) => Err(refused(format!(\n            "self-promotion: transition issuer',
     'axon-loop', '--test pointer', 'no_proposer_evaluator_or_subject_issues_a_promotion'),
    ('M102', 'baseline: the issuer holds no other loop role',
     'crates/axon-loop/src/pointer.rs',
     '    if let Some(role) = crate::admission::other_loop_role(&config, &b.issuer_ref) {\n        return Err(refused(format!(\n            "baseline issuer {} is also', '    if let Some(role) = None::<&str> {\n        return Err(refused(format!(\n            "baseline issuer {} is also',
     'axon-loop', '--test pointer', 'a_baseline_issuer_holds_no_other_loop_role'),
    ('M103', 'derive check_pins: a DEVELOPMENT-class decision re-checks the operator pins at re-derivation (LOAD-BEARING: reverify_protected covers only the protected class; C8 review wf_ae3a5a74-41e reinstated it from equivalent)', 'crates/axon-loop/src/admission.rs', 'crate::intake::check_pins(&config, &v.issuer_ref, &t.task_id, &req, &rc).map_err(', 'Ok::<(), LoopError>(()).map_err(', 'axon-loop', '--test dev_rederivation', 'a_development_activation_after_the_revision_pin_changes_is_refused'),
    ('M104', 'a counted protected trial must carry a CURRENT observer attribution (LOAD-BEARING on the store-writer misattribution route; C9 re-audit reinstated it)', 'crates/axon-loop/src/admission.rs', '                ) && !t.context_signed_by.as_ref().is_some_and(|c| {', '                ) && false && !t.context_signed_by.as_ref().is_some_and(|c| {', 'axon-loop', '--test protected_attribution', 'a_protected_verdict_without_its_context_attribution_does_not_count'),
    ('M105', "derive: a clearance's monitor and key are still current",
     'crates/axon-loop/src/admission.rs',
     '                if t.safety == crate::safety::SafetyState::Clear\n',
     '                if false && t.safety == crate::safety::SafetyState::Clear\n',
     'axon-loop', '--test protected_class', 'a_protected_activation_rests_only_on_current_authority'),
    ('M106', 'evl: the Fabric execution cost component is never omitted',
     'crates/axon-loop/src/evl.rs',
     '                if let Some((req, rcpt, _)) = &d.acf {\n                    exec_usages',
     '                if let Some((req, rcpt, _)) = d.acf.as_ref().filter(|_| false) {\n                    exec_usages',
     'axon-loop', '--test evl_admission', 'an_execution_cost_is_never_omitted_from_the_economics'),
    ('M107', 'evl: only the issued attempt counts',
     'crates/axon-loop/src/evl.rs',
     'if &d.ep.identity.attempt_id != issued_attempt {',
     'if false && &d.ep.identity.attempt_id != issued_attempt {',
     'axon-loop', '--test assignment', 'a_later_attempt_is_never_swapped_in_for_the_issued_one'),
    ('M108', 'evl: the request assigns exactly the journalled population',
     'crates/axon-loop/src/evl.rs',
     '    if requested.len() != issued.len()\n        || requested\n',
     '    if false && (requested.len() != issued.len())\n        && requested\n',
     'axon-loop', '--test assignment', 'a_trial_is_never_assigned_after_outcomes_exist'),
    ('M109', 'evl: a protected trial is preflighted after its issue',
     'crates/axon-loop/src/evl.rs',
     '            && ctx.created_ms < assigned_ms\n',
     '            && false\n',
     'axon-loop', '--test assignment', 'a_protected_trial_runs_only_after_it_is_issued'),
    ('M110', 'assign: no trial is intaken before it is issued',
     'crates/axon-loop/src/plan.rs',
     'if scope == &a.scope && trials.contains(&intake.identity.trial_id) {',
     'if false {',
     'axon-loop', '--test assignment', 'a_population_is_issued_before_any_outcome'),
    ('M111', 'assign: the issuer holds no other loop role',
     'crates/axon-loop/src/plan.rs',
     'if let Some(role) = crate::admission::other_loop_role(&config, &a.issuer_ref) {',
     'if let Some(role) = None::<&str> {',
     'axon-loop', '--test assignment', 'the_population_is_issued_once_by_an_independent_admitter'),
    ('M112', 'assign: the population is exactly manifest x arms x repetitions',
     'crates/axon-loop/src/plan.rs',
     '    crate::evl::check_population(&tx, &frozen, &population)?;\n',
     '    let _ = &population;\n',
     'axon-loop', '--test redteam', 'ab9_the_single_evaluation_covers_exactly_the_manifest'),
    ('M113', 'admit: the admitter holds no other loop role',
     'crates/axon-loop/src/admission.rs',
     '    if let Some(role) = other_loop_role(&tx.store.config()?, admitter) {',
     '    if let Some(role) = None::<&str> {',
     'axon-loop', '--test evl_admission', 'an_admitter_holds_no_other_loop_role'),
    ('M114', 'admit: the proposer (ranker) cannot admit its own candidate',
     'crates/axon-loop/src/admission.rs',
     '    if &proposer == admitter {',
     '    if false && &proposer == admitter {',
     'axon-loop', '--test evl_admission', 'the_proposer_cannot_admit_its_own_candidate_whatever_the_record_lists'),
    ('M115', 'config: one key per role, checked on read as well as write (ADR-002)',
     'crates/axon-loop/src/store.rs',
     # C9 r2 (rows): re-anchored; check_suite_refs now sits between it and Ok(c).
     ')?;\n                c.check_separation().map_err(crate::error::refused)?;\n                c.check_suite_refs().map_err(crate::error::refused)?;\n                Ok(c)',
     ')?;\n                c.check_suite_refs().map_err(crate::error::refused)?;\n                Ok(c)',
     'axon-loop', '--test evl_admission', 'an_observer_key_and_identity_are_its_own'),
    ('M116', "facts: a multi-currency arm's liability is summed, never 0",
     'crates/axon-loop/src/admission.rs',
     '                .map(|c| c.unresolved_liability_micro)\n                .fold(0, u64::saturating_add),',
     '                .map(|c| c.unresolved_liability_micro)\n                .fold(0, |_, _| 0),',
     'axon-loop', '--lib', 'admission::decide_tests::a_multi_currency_arm_is_never_decided_as_known'),
    ('M117', 'decide: an arm spanning currencies is never decided as known',
     'crates/axon-loop/src/admission.rs',
     '        if n != 1 && arm.assigned > arm.missing {',
     '        if false && n != 1 && arm.assigned > arm.missing {',
     'axon-loop', '--lib', 'admission::decide_tests::a_multi_currency_arm_is_never_decided_as_known'),
    ('M118', "evl: a trial's usage is in its execution request's currency",
     'crates/axon-loop/src/evl.rs',
     '        if d.ep.usage.currency.as_str() != req.limits.currency_code.as_str() {',
     '        if false && d.ep.usage.currency.as_str() != req.limits.currency_code.as_str() {',
     'axon-loop', '--test evl_admission', 'a_relabelled_currency_never_hides_a_liability'),
    ('M119', "rollback: the baseline's issuer is independent now",
     'crates/axon-loop/src/pointer.rs',
     '        baseline_issuer_independent(tx, &b)\n            .map_err(',
     '        Ok::<(), LoopError>(())\n            .map_err(',
     'axon-loop', '--test pointer', 'a_baseline_issuer_is_independent_now_and_revocation_too'),
    ('M120', "activate: the baseline's issuer is independent now (route 1)",
     'crates/axon-loop/src/pointer.rs',
     '            baseline_issuer_independent(tx, &b)?;\n',
     '',
     'axon-loop', '--test pointer', 'a_baseline_issuer_is_independent_now_and_revocation_too'),
    ('M121', 'revoke: the issuer holds no other loop role',
     'crates/axon-loop/src/pointer.rs',
     '    if let Some(role) = crate::admission::other_loop_role(&config, issuer) {',
     '    if let Some(role) = None::<&str> {',
     'axon-loop', '--test pointer', 'a_baseline_issuer_is_independent_now_and_revocation_too'),
    ('M122', "derive: a counted trial's context observer is trusted still (any class)",
     'crates/axon-loop/src/admission.rs',
     '                if !observers_now.contains(o) {',
     '                if false && !observers_now.contains(o) {',
     'axon-loop', '--test evl_admission', 'a_development_verdict_rests_on_a_currently_trusted_observer'),
    ('M123', 'EVL: a D12 pass is never counted',
     'crates/axon-loop/src/evl.rs',
     '            } else if d12 {\n                unknown(UnknownKind::Unbound, D12_NOT_COUNTED)\n',
     '            } else if false {\n                unknown(UnknownKind::Unbound, D12_NOT_COUNTED)\n',
     'axon-loop', '--test d12_unknown_outcome', 'every_d12_non_success_keeps_its_kind_and_nothing_passes'),
    ('M124', 'EVL: a D12 failure is never counted',
     'crates/axon-loop/src/evl.rs',
     'VerificationResult::Failed if d12 =>',
     'VerificationResult::Failed if false =>',
     'axon-loop', '--test d12_unknown_outcome', 'every_d12_non_success_keeps_its_kind_and_nothing_passes'),
    ('M125', 'EVL: a cited unknown is authenticated, its kind from the signed check',
     'crates/axon-loop/src/evl.rs',
     '|| (v.result == VerificationResult::Unknown && v.verifier_ref.is_some())',
     '|| false',
     'axon-loop', '--test d12_unknown_outcome', 'a_cited_unknown_takes_its_kind_from_the_signed_check'),
    ('M126', "EVL: a D12 run's cancellation is its kind",
     'crates/axon-loop/src/evl.rs',
     'None => (d.ep.status == EpisodeStatus::Cancelled).then_some(UnknownKind::Cancelled),',
     'None => None,',
     'axon-loop', '--test d12_unknown_outcome', 'every_d12_non_success_keeps_its_kind_and_nothing_passes'),
    ('M127', 'EVL: a completed check with no match is Unmatched',
     'crates/axon-loop/src/evl.rs',
     'ReceiptStatus::Completed if rc.matched_checks.unwrap_or(0) == 0 => {',
     'ReceiptStatus::Completed if false => {',
     'axon-loop', '--test d12_unknown_outcome', 'a_cited_unknown_takes_its_kind_from_the_signed_check'),
    ('M128', 'EVL: a signed canceled check is Cancelled',
     'crates/axon-loop/src/evl.rs',
     '                    ReceiptStatus::Canceled => UnknownKind::Cancelled,\n                    ReceiptStatus::Completed if',
     '                    ReceiptStatus::Completed if',
     'axon-loop', '--test d12_unknown_outcome', 'every_d12_non_success_keeps_its_kind_and_nothing_passes'),
    ('M129', 'EVL: a D12 trial delivers no execution documents',
     'crates/axon-loop/src/evl.rs',
     '.any(|v| !v.is_null())',
     '.any(|_| false)',
     'axon-loop', '--test d12_unknown_outcome', 'a_d12_trial_with_execution_documents_is_refused'),
    ('M130', 'EVL: a protected evaluation refuses a D12 trial as unverifiable (D3)',
     'crates/axon-loop/src/evl.rs',
     '            return unknown(\n                UnknownKind::Unverifiable,\n                "D12 local execution',
     '            return unknown(\n                UnknownKind::Unbound,\n                "D12 local execution',
     'axon-loop', '--test d12_unknown_outcome', 'a_protected_evaluation_never_counts_a_d12_trial'),
    ('M131', 'EVL: a cancelled or timed-out run counts no verdict',
     'crates/axon-loop/src/evl.rs',
     'if let (Some(k), VerificationResult::Passed | VerificationResult::Failed) = (run_end, v.result)',
     'if let (Some(k), VerificationResult::Passed | VerificationResult::Failed) = (run_end.filter(|_| false), v.result)',
     'axon-loop', '--test d12_unknown_outcome', 'a_cancelled_run_does_not_count_its_failed_check'),
    ('M132', 'intake: an uncited verification names no evidence but one not-run marker',
     'crates/axon-loop/src/intake.rs',
     '        && micode_not_run_reason(&ep.verification).is_none()\n',
     '        && false\n',
     'axon-loop', '--test d12_unknown_outcome', 'an_uncited_verification_names_no_other_evidence'),
    ('M133', 'EVL: a stated run or check timeout is TimedOut',
     'crates/axon-loop/src/evl.rs',
     '"run_timed_out" | "check_timed_out" => UnknownKind::TimedOut,',
     '"run_timed_out" | "check_timed_out" => UnknownKind::NotRun,',
     'axon-loop', '--test d12_unknown_outcome', 'a_stated_not_run_reason_keeps_the_kind'),
    ('M134', 'EVL: stated missing check evidence is MissingEvidence',
     'crates/axon-loop/src/evl.rs',
     '"check_evidence_missing" => UnknownKind::MissingEvidence,',
     '"check_evidence_missing" => UnknownKind::NotRun,',
     'axon-loop', '--test d12_unknown_outcome', 'a_stated_not_run_reason_keeps_the_kind'),
    ('M135', 'EVL: a stated unverifiable check is Unverifiable',
     'crates/axon-loop/src/evl.rs',
     '"check_unverifiable" => UnknownKind::Unverifiable,',
     '"check_unverifiable" => UnknownKind::NotRun,',
     'axon-loop', '--test d12_unknown_outcome', 'a_stated_not_run_reason_keeps_the_kind'),
    ('M136', 'EVL: how the run ended outranks what the producer states',
     'crates/axon-loop/src/evl.rs',
     '            run_end.or(stated).unwrap_or(UnknownKind::NotRun),',
     '            stated.or(run_end).unwrap_or(UnknownKind::NotRun),',
     'axon-loop', '--test d12_unknown_outcome', 'a_stated_not_run_reason_keeps_the_kind'),
    # ── PSV protocol (v022-psv-protocol.md; O1, M1-M4, O2). Equivalent mutants
    #    (sort-nested, no-scrub, no-diff-fail) are documented, never rows. ──
    ('M137', 'O1: eligibility requires the launcher at its pin (RULE:launcher-pinned)', 'crates/axon-fabric/src/backend.rs', 'if got != self.launcher_sha256 {', 'if false && got != self.launcher_sha256 {', 'axon-fabric', '--test protected_host', 'a_replaced_launcher_is_refused_at_load_and_after_load'),
    ('M138', 'O1/A21: submit refuses every caller protected flag', 'crates/axon-fabric/src/bin/axon-fabric.rs', 'if a.opt(flag).is_some() {', 'if false && a.opt(flag).is_some() {', 'axon-fabric', '--test protected_host', 'every_caller_protected_flag_is_refused_by_name'),
    ('M139', 'O1/A17: no caller registry on a protected host', 'crates/axon-fabric/src/bin/axon-fabric.rs', 'if host.is_some() && a.opt("--check-registry").is_some() {', 'if false && host.is_some() && a.opt("--check-registry").is_some() {', 'axon-fabric', '--test protected_host', 'a_caller_registry_never_defines_suites_on_a_protected_host'),
    ('M140', "O1: the protected signer is the host config's", 'crates/axon-fabric/src/bin/axon-fabric.rs', 'Some(h) => Some(host_signer(h)),', 'Some(_) => signer(&registry_path),', 'axon-fabric', '--test protected_host', 'the_host_signer_key_must_be_private_and_match_its_pin'),
    # C9 round 1b: re-anchored. The old mutation (`Some(_) => PathBuf::from(a.req("--check-registry"))`)
    # made a protected host fail CLOSED (it demanded a caller registry that M139 refuses): it removed
    # no protection, only liveness. Removing the guard is the caller override below.
    ('M141', "O1: a protected host loads the operator's registry", 'crates/axon-fabric/src/bin/axon-fabric.rs', 'Some(h) => h.suite_registry.clone(),', 'Some(h) => a.opt("--check-registry").map(PathBuf::from).unwrap_or_else(|| h.suite_registry.clone()),', 'axon-fabric', '--test protected_host', 'a_caller_registry_never_defines_suites_on_a_protected_host'),
    ('M142', 'O1: a pinned file is exactly its pinned bytes', 'crates/axon-fabric/src/protected_host.rs', 'if got != pin {', 'if false && got != pin {', 'axon-fabric', '--test protected_host', 'a_replaced_launcher_is_refused_at_load_and_after_load'),
    ('M143', 'O1: every O1 path is operator-owned', 'crates/axon-fabric/src/protected_host.rs', 'Some(base) => check_owned_chain(base, p, entries),', 'Some(_base) => Ok(()),', 'axon-fabric', '--test protected_host', 'every_o1_path_must_be_operator_owned'),
    ('M144', 'O1: the host config schema is exact', 'crates/axon-fabric/src/protected_host.rs', 'if keys != KEYS {', 'if false && keys != KEYS {', 'axon-fabric', '--test protected_host', 'the_config_schema_is_exact_and_every_path_absolute'),
    ('M145', 'O1: every protected path is absolute', 'crates/axon-fabric/src/protected_host.rs', '            if !p.is_absolute() {\n                return Err(bad(format!(', '            if false && !p.is_absolute() {\n                return Err(bad(format!(', 'axon-fabric', '--test protected_host', 'the_config_schema_is_exact_and_every_path_absolute'),
    ('M146', "O1: the signing key's directory is operator-owned", 'crates/axon-fabric/src/protected_host.rs', 'if let Some(dir) = signer.key_path.parent() {', 'if let Some(dir) = None::<&Path> {', 'axon-fabric', '--test protected_host', 'every_o1_path_must_be_operator_owned'),
    ('M147', 'O1: the artifacts directory is operator-owned', 'crates/axon-fabric/src/protected_host.rs', 'owned(&artifacts_dir, true).map_err(bad)?;', '', 'axon-fabric', '--test protected_host', 'every_o1_path_must_be_operator_owned'),
    ('M148', 'O1: the B263 record is operator-owned', 'crates/axon-fabric/src/protected_host.rs', 'owned(&record, false).map_err(bad)?;', '', 'axon-fabric', '--test protected_host', 'every_o1_path_must_be_operator_owned'),
    ('M149', 'readiness: a certification binds the verifier that made it', 'crates/axon-fabric/src/readiness.rs', 'if doc["readiness_verifier_sha256"] != me["sha256"] {', 'if false && doc["readiness_verifier_sha256"] != me["sha256"] {', 'axon-fabric', '--test readiness', 'another_verifier_binary_does_not_inherit_the_certification'),
    ('M150', 'readiness: the trust preflight must be protected-mode', 'crates/axon-fabric/src/readiness.rs', '|| pf["mode"] != "protected"', '|| false', 'axon-fabric', '--test readiness', 'a_dev_mode_or_uncertified_trust_preflight_is_refused'),
    ('M151', 'readiness: the trust preflight is a certified evidence file', 'crates/axon-fabric/src/readiness.rs', '    let (_, _, pf) = named(&evidence, component, &doc, "trust_preflight_sha256")?;\n', '    let pf_fallback = std::fs::read(repo.join("governance/proofs/v022-protected/trust-preflight.json")).unwrap_or_default();\n    let pf: &[u8] = named(&evidence, component, &doc, "trust_preflight_sha256").map(|e| e.2.as_slice()).unwrap_or(&pf_fallback);\n', 'axon-fabric', '--test readiness', 'a_dev_mode_or_uncertified_trust_preflight_is_refused'),
    ('M152', "RULE:authority-domain: the signature's domain field (EQUIVALENT: dominated by the domain-bound message, M153)", 'crates/axon-loop-contracts/src/operator_trust.rs', 'if sv["domain"] != authority.dir_name() {', 'if false && sv["domain"] != authority.dir_name() {', 'axon-fabric', '--test verify_evidence', 'an_unrelabelled_signature_for_another_authority_verifies_nowhere_else'),
    ('M153', 'RULE:authority-domain: the domain is in the signed message', 'crates/axon-loop-contracts/src/operator_trust.rs', '.verify(&evidence_signing_message(authority, bytes), &sig)', '.verify(&evidence_signing_message(TrustAuthority::Qualification, bytes), &sig)', 'axon-fabric', '--test verify_evidence', 'each_authority_verifies_only_its_own_domain_message'),
    ('M154', 'PSV §3: the manifest verifies only under the digest Fabric named', 'crates/axon-psv/src/lib.rs', 'if got != expected_sha256 {', 'if false && got != expected_sha256 {', 'axon-psv', '--test protocol', 'manifest_bytes_are_canonical_and_verify_only_under_their_own_digest'),
    ('M155', 'PSV §3: manifest bytes are canonical', 'crates/axon-psv/src/lib.rs', 'if m.bytes() != bytes {', 'if false && m.bytes() != bytes {', 'axon-psv', '--test protocol', 'manifest_bytes_are_canonical_and_verify_only_under_their_own_digest'),
    ('M156', 'PSV §3: the manifest is for the protected profile', 'crates/axon-psv/src/lib.rs', 'if m.backend_profile != PROTECTED_PROFILE {', 'if false && m.backend_profile != PROTECTED_PROFILE {', 'axon-psv', '--test protocol', 'manifest_bytes_are_canonical_and_verify_only_under_their_own_digest'),
    ('M157', 'PSV §3: the completion scheme', 'crates/axon-psv/src/lib.rs', 'if m.completion.scheme != COMPLETION_SCHEME {', 'if false && m.completion.scheme != COMPLETION_SCHEME {', 'axon-psv', '--test protocol', 'manifest_bytes_are_canonical_and_verify_only_under_their_own_digest'),
    ('M158', 'PSV §3: the manifest schema', 'crates/axon-psv/src/lib.rs', 'if m.schema != LAUNCH_MANIFEST_SCHEMA {', 'if false && m.schema != LAUNCH_MANIFEST_SCHEMA {', 'axon-psv', '--test protocol', 'manifest_bytes_are_canonical_and_verify_only_under_their_own_digest'),
    ('M159', "PSV §4/A1: the candidate tree is the manifest's", 'crates/axon-psv/src/lib.rs', 'if c != m.candidate.tree_digest {', 'if false && c != m.candidate.tree_digest {', 'axon-psv', '--test protocol', 'inputs_must_match_the_manifest_each_one_named'),
    ('M160', "PSV §4/A2: the suite tree is the manifest's", 'crates/axon-psv/src/lib.rs', 'if s != m.suite.tree_digest {', 'if false && s != m.suite.tree_digest {', 'axon-psv', '--test protocol', 'inputs_must_match_the_manifest_each_one_named'),
    ('M161', 'PSV §4/A5: the attempt is bound explicitly', 'crates/axon-psv/src/lib.rs', '"attempt_id": m.attempt_id,', '', 'axon-psv', '--test protocol', 'the_completion_binding_names_every_identity_explicitly'),
    ('M162', 'PSV §4/A3: the test is bound explicitly', 'crates/axon-psv/src/lib.rs', '"test": m.suite.test,', '', 'axon-psv', '--test protocol', 'the_completion_binding_names_every_identity_explicitly'),
    ('M163', 'PSV §4: the manifest digest is bound', 'crates/axon-psv/src/lib.rs', '"launch_manifest_digest": launch_manifest_digest,', '', 'axon-psv', '--test protocol', 'the_completion_key_moves_with_every_bound_identity_and_the_secret'),
    ('M164', 'PSV §4/A1: the candidate tree is bound explicitly', 'crates/axon-psv/src/lib.rs', '"candidate_tree_digest": m.candidate.tree_digest,', '', 'axon-psv', '--test protocol', 'the_completion_binding_names_every_identity_explicitly'),
    ('M165', 'PSV §4/A11: K is derived from the binding', 'crates/axon-psv/src/lib.rs', 'msg.extend(sha256_hex(&b).into_bytes());', 'let _ = &b;', 'axon-psv', '--test protocol', 'the_completion_key_moves_with_every_bound_identity_and_the_secret'),
    ('M166', 'PSV §4: HMAC-SHA256 is RFC 2104', 'crates/axon-psv/src/lib.rs', 'inner.update(k.map(|b| b ^ 0x36));', 'inner.update(k.map(|b| b ^ 0x35));', 'axon-psv', '--test protocol', 'hmac_matches_rfc_4231'),
    ('M167', 'runner: the manifest is exactly the one named on the cmdline', 'crates/axon-psv/src/runner.rs', 'LaunchManifest::verify(&m_bytes, &cfg.expected_manifest_sha256)', 'LaunchManifest::verify(&m_bytes, &sha256_hex(&m_bytes))', 'axon-psv', '--test runner', 'nothing_executes_unless_every_input_is_the_named_one'),
    ('M168', 'runner: nothing executes unless both inputs match', 'crates/axon-psv/src/runner.rs', 'Err((found, why)) => return refused(cfg, &m_sha, found, &test, why),', 'Err((found, _why)) => found,', 'axon-psv', '--test runner', 'nothing_executes_unless_every_input_is_the_named_one'),
    ('M169', 'runner/A3: the test is a plain identifier', 'crates/axon-psv/src/runner.rs', 'if !is_identifier(&test) {', 'if false && !is_identifier(&test) {', 'axon-psv', '--test runner', 'nothing_executes_unless_every_input_is_the_named_one'),
    ('M170', 'runner: the secret is exactly 32 bytes', 'crates/axon-psv/src/runner.rs', 'Ok(b) if b.len() == 32 =>', 'Ok(b) if b.len() >= 5 =>', 'axon-psv', '--test runner', 'nothing_executes_unless_every_input_is_the_named_one'),
    ('M171', 'runner/A19: K is one stdin line, then EOF', 'crates/axon-psv/src/runner.rs', 'writeln!(stdin, "{hex}").map_err(|e| e.to_string())?;', 'writeln!(stdin, "{hex}").map_err(|e| e.to_string())?;\n        writeln!(stdin, "{hex}").map_err(|e| e.to_string())?;', 'axon-psv', '--test runner', 'neither_the_secret_nor_the_key_reaches_candidate_code_or_the_output'),
    ('M172', 'runner/A19: the test runs as the unprivileged uid', 'crates/axon-psv/src/runner.rs', 'if let Some((uid, gid)) = drop {', 'if let Some((uid, gid)) = None::<(u32, u32)> {', 'axon-psv', '--test runner', 'neither_the_secret_nor_the_key_reaches_candidate_code_or_the_output'),
    ('M173', 'runner: only the EXACT test name counts', 'crates/axon-psv/src/runner.rs', 'if v["name"].as_str() != Some(test) {', 'if !v["name"].as_str().unwrap_or("").contains(test) {', 'axon-psv', '--test runner', 'a_forged_or_duplicated_result_line_is_not_a_pass'),
    ('M174', 'runner: a duplicated result line is not a pass', 'crates/axon-psv/src/runner.rs', '(1, 1, 0) if report.completion.len() == 1 => GuestStatus::Passed,', '(_, 1.., 0) if !report.completion.is_empty() => GuestStatus::Passed,', 'axon-psv', '--test runner', 'a_forged_or_duplicated_result_line_is_not_a_pass'),
    ('M175', 'runner/A12: a pass needs a completion token', 'crates/axon-psv/src/runner.rs', '(1, 1, 0) if report.completion.len() == 1 => GuestStatus::Passed,', '(1, 1, 0) => GuestStatus::Passed,', 'axon-psv', '--test runner', 'a_forged_or_duplicated_result_line_is_not_a_pass'),
    ('M176', 'runner: a pass needs exit 0', 'crates/axon-psv/src/runner.rs', 'if status == GuestStatus::Passed && exit_code != Some(0) {', 'if false && status == GuestStatus::Passed && exit_code != Some(0) {', 'axon-psv', '--test runner', 'a_pass_with_a_failing_run_is_unknown'),
    ('M177', "runner: the suite's own modules come first", 'crates/axon-psv/src/runner.rs', 'format!("{}:{}", cfg.suite.display(), cfg.candidate.display()),', 'format!("{}:{}", cfg.candidate.display(), cfg.suite.display()),', 'axon-psv', '--test runner', 'a_candidate_cannot_shadow_the_suites_own_modules'),
    ('M178', 'runner: the candidate runs sealed (PCI E0004)', 'crates/axon-psv/src/runner.rs', '.arg("--seal")\n        .arg(&cfg.candidate)', '', 'axon-psv', '--test runner', 'a_sealed_candidate_cannot_read_the_suites_answer'),
    ('M179', 'M2: the verdict names THIS launch manifest', 'crates/axon-fabric/src/psv.rs', 'if v.launch_manifest_sha256 != launch.digest {', 'if false && v.launch_manifest_sha256 != launch.digest {', 'axon-fabric', '--test psv_dispatch', 'every_forgery_of_the_returned_evidence_is_unknown_for_its_own_reason'),
    ('M180', 'M2/A1: a guest refusal is reported as such', 'crates/axon-fabric/src/psv.rs', 'if v.status == GuestStatus::Refused {', 'if false && v.status == GuestStatus::Refused {', 'axon-fabric', '--test psv_dispatch', 'a_guest_refusal_stands_even_over_a_genuine_passing_run'),
    ('M181', "M2: the verdict's inputs and test are this launch's", 'crates/axon-fabric/src/psv.rs', 'if !v.inputs.matches\n', 'if false && !v.inputs.matches\n', 'axon-fabric', '--test psv_dispatch', 'every_forgery_of_the_returned_evidence_is_unknown_for_its_own_reason'),
    ('M182', 'M2: the output hashes to what the verdict names', 'crates/axon-fabric/src/psv.rs', 'if v.stdout_sha256.as_deref() != Some(axon_psv::sha256_hex(&out).as_str()) {', 'if false && v.stdout_sha256.as_deref() != Some(axon_psv::sha256_hex(&out).as_str()) {', 'axon-fabric', '--test psv_dispatch', 'every_forgery_of_the_returned_evidence_is_unknown_for_its_own_reason'),
    ('M183', "M2/A11/A12: the token verifies under Fabric's own key", 'crates/axon-fabric/src/psv.rs', '.any(|(n, t)| n == test && *t == want)', '.any(|(n, _t)| n == test)', 'axon-fabric', '--test psv_dispatch', 'a_previous_attempts_genuine_pass_does_not_replay'),
    ('M184', 'M2: a pass needs exit 0', 'crates/axon-fabric/src/psv.rs', 'if v.exit_code != Some(0) {', 'if false && v.exit_code != Some(0) {', 'axon-fabric', '--test psv_dispatch', 'a_valid_token_with_a_failing_run_is_not_a_pass'),
    ('M185', "M2: the certified parser decides, never the guest's claim", 'crates/axon-fabric/src/psv.rs', 'let verification = match report.verdict(test) {', 'let verification = match (if v.status == GuestStatus::Passed { CheckVerdict::Passed } else { report.verdict(test) }) {', 'axon-fabric', '--test psv_dispatch', 'a_failing_test_is_failed_whatever_the_guest_claims'),
    ('M186', 'M2/A14: without an observation a guest verdict is guest-unobserved', 'crates/axon-fabric/src/psv.rs', '        None => EvidenceClass::GuestUnobserved,\n    };', '        None => EvidenceClass::Protected,\n    };', 'axon-fabric', '--test psv_dispatch', 'an_operator_suite_passes_through_the_guest_path_as_guest_unobserved'),
    ('M187', 'M2: the protected profile runs only operator suites', 'crates/axon-fabric/src/submit.rs', '&& (target.suite.is_none() || target.filter.is_none())', '&& false', 'axon-fabric', '--test psv_dispatch', 'only_an_operator_suite_runs_on_the_protected_profile'),
    ('M188', 'M2: an inadmissible launch has no verdict', 'crates/axon-fabric/src/submit.rs', 'let launched_ok = matches!(res.outcome, backend::LinuxOutcome::Ok { .. });', 'let launched_ok = true;', 'axon-fabric', '--test psv_dispatch', 'a_valid_verdict_from_an_unbound_launch_counts_for_nothing'),
    ('M189', 'M2/A13: a local run is development evidence', 'crates/axon-fabric/src/submit.rs', '            r.evidence_refs.insert(\n                0,\n                opaque(crate::psv::EvidenceClass::Development.evidence_ref()),\n            );', '', 'axon-fabric', '--test psv_dispatch', 'a_local_check_is_development_evidence'),
    ('M190', '§6/A13: a class is signed only where its backend derives it', 'crates/axon-fabric/src/signing.rs', '"protected" | "guest-unobserved" => Ok(()),\n            _ => Err(WRONG_CLASS),', '_ => Ok(()),', 'axon-fabric', '--lib', 'signing::tests::a_class_is_signed_only_where_its_backend_derives_it'),
    ('M191', 'M3: the observer program is pinned', 'crates/axon-fabric/src/observer.rs', '            sha256: cfg.command_sha256.clone(),\n        },\n        cfg.exec_owner,', '            sha256: crate::backend::sha256_file(&cfg.command).unwrap_or_default(),\n        },\n        cfg.exec_owner,', 'axon-fabric', '--test psv_dispatch', 'an_unpinned_observer_is_refused'),
    ('M192', 'M3/A9: the OBSERVER domain', 'crates/axon-fabric/src/observer.rs', '        &cfg.trust.dir,\n        TrustAuthority::Observer,', '        &cfg.trust.dir,\n        TrustAuthority::Qualification,', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M193', 'M3: the claimed observer is the signer', 'crates/axon-fabric/src/observer.rs', 'if o.observer_key_id != signer {', 'if false && o.observer_key_id != signer {', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M194', "M3/A8: the observation's epoch", 'crates/axon-fabric/src/observer.rs', 'if o.epoch != epoch {', 'if false && o.epoch != epoch {', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_launches_nothing_on_the_direct_route'),
    ('M195', "M3/A8: the observation's age", 'crates/axon-fabric/src/observer.rs', 'if age < 0 || age as u64 > cfg.max_age_s {', 'if false && (age < 0 || age as u64 > cfg.max_age_s) {', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M196', 'M3/A15 -> amendment 50: a verified observation spends its nonce (in the CUSTODIAN, for the root helper)', 'crates/axon-fabric/src/custodian.rs', '                self.store\n                    .consume(nonce, r.epoch, &self.clock, self.cfg.max_age_s)?;', '                let _ = (&self.store, nonce);', 'axon-fabric', '--test psv_dispatch', 'a_verified_observation_makes_the_guest_verdict_protected'),
    ('M197', 'M3: a nonce is of its epoch', 'crates/axon-fabric/src/observer.rs', 'if rec["epoch"].as_u64() != Some(epoch) {', 'if false && rec["epoch"].as_u64() != Some(epoch) {', 'axon-fabric', '--test psv_dispatch', 'a_nonce_authorizes_exactly_one_launch'),
    ('M198', 'M3/A8: a nonce has a maximum age', 'crates/axon-fabric/src/observer.rs', 'if age < 0 || age as u64 > max_age_s {', 'if false && (age < 0 || age as u64 > max_age_s) {', 'axon-fabric', '--test psv_dispatch', 'a_nonce_authorizes_exactly_one_launch'),
    ('M199', 'M3: a nonce is one this custodian issues', 'crates/axon-fabric/src/observer.rs', 'if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {', 'if false {', 'axon-fabric', '--test psv_dispatch', 'a_nonce_authorizes_exactly_one_launch'),
    ('M200', 'M3/A15: the observation is of THIS manifest', 'crates/axon-psv/src/lib.rs', '                manifest_digest,\n            ),', '                &self.intended_launch_manifest_sha256,\n            ),', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M201', "M3: the observation's nonce is the manifest's", 'crates/axon-psv/src/lib.rs', '("nonce", &self.nonce, &m.observation_nonce),', '("nonce", &self.nonce, &self.nonce),', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_launches_nothing_on_the_direct_route'),
    ('M202', "M3/A7: the observed kernel is the launch's", 'crates/axon-psv/src/lib.rs', '                &self.guest.kernel_sha256,\n                &m.guest.kernel_sha256,', '                &self.guest.kernel_sha256,\n                &self.guest.kernel_sha256,', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M203', 'M3: a verified observation makes the verdict protected', 'crates/axon-fabric/src/submit.rs', 'let hv = crate::psv::derive(&launch, &res.out_dir, observation.as_ref());', 'let hv = crate::psv::derive(&launch, &res.out_dir, None);', 'axon-fabric', '--test psv_dispatch', 'a_verified_observation_makes_the_guest_verdict_protected'),
    # C9 round 1: re-anchored on the CURRENT seam (it had been recorded STALE while
    # its guard lived on). The mutant swallows observe's Err and launches with no
    # observation, as the pre-refactor mutant did.
    ('M204', 'M3: a refused observation launches nothing', 'crates/axon-fabric/src/submit.rs', '                .map_err(|e| format!("preflight observation refused: {e}"))\n                .and_then(|v| {\n', '                .map_or_else(|_| Ok::<_, String>(None), |v| Ok(Some(v)))\n                .and_then(|v| {\n                    let Some(v) = v else {\n                        return Ok((launch, None));\n                    };\n', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_launches_nothing_on_the_direct_route'),
    ('M205', 'O2/A18: protected evidence needs an operator-rooted verifier key', 'crates/axon-loop/src/intake.rs', '    if axon_loop_contracts::protected_evidence::claims_protected(&rc) {', '    if false && axon_loop_contracts::protected_evidence::claims_protected(&rc) {', 'axon-loop', '--test intake', 'a_verifier_key_planted_in_the_store_never_authenticates_protected_evidence'),
    ('M206', 'O2: rooted() requires the key in the operator root', 'crates/axon-loop-contracts/src/operator_trust.rs', '    if root_keys_hex(a)?.contains(&want) {', '    if true {', 'axon-loop', '--test intake', 'a_verifier_key_planted_in_the_store_never_authenticates_protected_evidence'),
    ('M207', 'O2: rooted_key consults the operator root', 'crates/axon-loop/src/store.rs', '        axon_loop_contracts::operator_trust::rooted(a, k)?;\n', '', 'axon-loop', '--test protected_class', 'an_observer_key_planted_in_the_store_is_not_authority'),
    ('M208', "O2/A18: EVL's protected context uses an operator-rooted observer key", 'crates/axon-loop/src/evl.rs', 'crate::store::Config::rooted_key(\n        &config.observer_keys,\n        who,\n        axon_loop_contracts::operator_trust::TrustAuthority::Observer,\n    )', 'config.observer_keys.get(who).ok_or_else(String::new)', 'axon-loop', '--test protected_class', 'an_observer_key_planted_in_the_store_is_not_authority'),
    ('M209', 'a counted verdict attributed to a verifier must name one the operator verifier ROOT holds (EQUIVALENT since C9 round 1b: four-cell vs M360, see EQUIV_RECORD)', 'crates/axon-loop/src/admission.rs', 'crate::store::Config::rooted_key(\n                    &config.verifier_keys,\n                    &v.issuer_ref,\n                    axon_loop_contracts::operator_trust::TrustAuthority::Verifier,\n                )', 'config.verifier_keys.get(&v.issuer_ref).ok_or_else(String::new)', 'axon-loop', '--test protected_attribution', 'a_verdict_attributed_to_a_verifier_the_operator_root_never_held_does_not_count'),
    ('M210', 'a context attributed to an observer must name one the operator observer ROOT holds (EQUIVALENT since C9 round 1b: four-cell vs M361, see EQUIV_RECORD)', 'crates/axon-loop/src/admission.rs', 'crate::store::Config::rooted_key(\n                            &config.observer_keys,\n                            &c.issuer_ref,\n                            axon_loop_contracts::operator_trust::TrustAuthority::Observer,\n                        )', 'config.observer_keys.get(&c.issuer_ref).ok_or_else(String::new)', 'axon-loop', '--test protected_attribution', 'a_context_attributed_to_an_observer_the_operator_root_never_held_does_not_count'),
    ('M211', 'M4/A13/A14: a protected evaluation counts only protected-class evidence', 'crates/axon-loop/src/evl.rs', '        // M4: a verdict counts in a protected evaluation only as PROTECTED\n', '        // M4 (mutated)\n        #[cfg(any())]\n', 'axon-loop', '--test protected_class', 'only_protected_class_evidence_counts_in_a_protected_evaluation'),
    ('M212', 'M4: the one class must be protected', 'crates/axon-loop-contracts/src/protected_evidence.rs', '["protected"] => {}', '[_] => {}', 'axon-loop', '--test protected_class', 'only_protected_class_evidence_counts_in_a_protected_evaluation'),
    ('M213', 'M4/A13: a protected claim needs a protected backend', 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if !crate::PROTECTED_PROFILES.contains(&rc.backend_profile_ref.as_str()) {', 'if false {', 'axon-loop', '--test intake', 'a_protected_claim_without_every_join_is_refused'),
    ('M214', 'M4/A14: a protected claim names its observation', 'crates/axon-loop-contracts/src/protected_evidence.rs', '    "preflight-observation-sha256:",\n', '    "launch-manifest-sha256:",\n', 'axon-loop', '--test intake', 'a_protected_claim_without_every_join_is_refused'),
    ('M215', "M4/A6: the guest interpreter is the request's pinned executable", 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if req.executable_digest.as_str() != want {', 'if false && req.executable_digest.as_str() != want {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M216', 'M4: each join exactly once', 'crates/axon-loop-contracts/src/protected_evidence.rs', '            [d] if d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()) => {', '            [d, ..] if d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()) => {', 'axon-loop', '--test intake', 'a_protected_claim_without_every_join_is_refused'),
    ('M217', 'M4: each join is a sha256', 'crates/axon-loop-contracts/src/protected_evidence.rs', '[d] if d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()) =>', '[d] =>', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M218', 'M4: intake holds a protected claim to every join', 'crates/axon-loop/src/intake.rs', '        let o = axon_loop_contracts::protected_evidence::check_bundle(\n            &req,\n            &rc,\n            bundle,\n            ep.authority_epoch.get(),\n            &ep.scope,\n            &config.trusted_observer_keys(),\n        )\n        .map_err(|e| {', '        let o = Ok::<_, String>(axon_loop_contracts::protected_evidence::ObservationSigner { observer_ref: issuer.clone(), key_id: bundle.len().to_string() })\n        .map_err(|e: String| {', 'axon-loop', '--test intake', 'a_protected_claim_without_every_join_is_refused'),
    ('M219', 'B1: a sealed candidate @[test] is never collected', 'crates/axon-core/src/main.rs', 'if !sealed.is_empty() && axon_core::resolver::span_in_sealed(f.span, &sealed) {', 'if false && axon_core::resolver::span_in_sealed(f.span, &sealed) {', 'axon-core', '--no-default-features --test psv_test_selection', 'a_sealed_candidates_own_test_is_never_collected'),
    ('M220', 'B1: the runner selects exactly the registered test', 'crates/axon-psv/src/runner.rs', '        .arg("--exact")\n', '', 'axon-psv', '--test runner', 'a_suite_sibling_does_not_run_beside_the_registered_test'),
    ('M221', 'B3: launch inputs come from the store, never the caller-owned run dir', 'crates/axon-fabric/src/submit.rs', '                crate::psv::private_inputs(\n                    lx,\n                    &cfg.state_dir,\n                    &cfg.epoch.scope().tenant_id,\n                    &req,\n                    &version,\n                )\n', '                { let _ = (lx, &version); match &target.bound { Bound::Version { dir, .. } => Ok::<PathBuf, String>(dir.0.clone()), _ => unreachable!() } }\n', 'axon-fabric', '--test psv_dispatch', 'a_run_dir_swapped_under_the_callers_state_changes_nothing'),
    ('M222', 'B3: a manifest candidate tree digest is its WorkspaceVersion', 'crates/axon-psv/src/lib.rs', 'if m.candidate.tree_digest != m.candidate.workspace_version {', 'if false && m.candidate.tree_digest != m.candidate.workspace_version {', 'axon-psv', '--test protocol', 'a_tree_digest_must_be_the_version_it_names'),
    ('M223', 'B3: a manifest suite tree digest is its version', 'crates/axon-psv/src/lib.rs', 'if m.suite.tree_digest != m.suite.version {', 'if false && m.suite.tree_digest != m.suite.version {', 'axon-psv', '--test protocol', 'a_tree_digest_must_be_the_version_it_names'),
    ('M224', 'guest input: nothing the digest omits', 'crates/axon-psv/src/lib.rs', 'if let Some(o) = omitted.first() {', 'if let Some(o) = omitted.first().filter(|_| false) {', 'axon-psv', '--test protocol', 'inputs_with_links_or_omitted_entries_are_refused'),
    ('M225', 'guest input: no symlink', 'crates/axon-psv/src/lib.rs', '.find(|e| e.kind == axon_workspace_recipe::EntryKind::Symlink)', '.find(|e| false && e.kind == axon_workspace_recipe::EntryKind::Symlink)', 'axon-psv', '--test protocol', 'inputs_with_links_or_omitted_entries_are_refused'),
    ('M226', 'a PSV verdict names its one matched check', 'crates/axon-fabric/src/submit.rs', '        r.matched_checks = Some(1);\n', '', 'axon-fabric', '--test psv_dispatch', 'an_operator_suite_passes_through_the_guest_path_as_guest_unobserved'),
    ('M227', 'observation joins the manifest verifier', 'crates/axon-psv/src/lib.rs', '("verifier_sha256", &self.verifier_sha256, &m.verifier_sha256),', '("verifier_sha256", &m.verifier_sha256, &m.verifier_sha256),', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M228', 'the verify step inherits no caller environment (C9 r2: re-anchored at sealed_exec::command, which builds the envp of every sealed child from exactly its env)', 'crates/axon-fabric/src/sealed_exec.rs', '    let envp = env\n        .iter()\n        .map(|(k, v)| c(format!("{k}={v}").as_bytes()))', '    let envp = std::env::vars()\n        .filter(|(k, _)| !env.iter().any(|(e, _)| e == k))\n        .chain(env.iter().map(|(k, v)| (k.to_string(), v.to_string())))\n        .map(|(k, v)| c(format!("{k}={v}").as_bytes()))', 'axon-fabric', '--test psv_dispatch', 'the_verify_step_inherits_nothing_from_the_caller'),
    ('M229', 'the per-attempt secret is scrubbed when the launcher returns', 'crates/axon-fabric/src/backend.rs', '    psv.scrub();\n    let out2 = out.clone();', '    let _ = &psv;\n    let out2 = out.clone();', 'axon-fabric', '--test psv_dispatch', 'the_verify_step_inherits_nothing_from_the_caller'),
    ('M230', 'B2: intake joins a protected claim over the bundle', 'crates/axon-loop/src/intake.rs', 'axon_loop_contracts::protected_evidence::check_bundle(\n            &req,\n            &rc,\n            bundle,\n            ep.authority_epoch.get(),\n            &ep.scope,\n            &config.trusted_observer_keys(),\n        )\n        .map_err(|e| {', 'axon_loop_contracts::protected_evidence::check(&req, &rc).map(|_| axon_loop_contracts::protected_evidence::ObservationSigner { observer_ref: issuer.clone(), key_id: bundle.len().to_string() })\n        .map_err(|e| {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M231', 'B2: a bundle for a non-protected receipt is refused', 'crates/axon-loop/src/intake.rs', '} else if psv_evidence.is_some() {', '} else if false && psv_evidence.is_some() {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M232', 'B2: the bundle manifest is the receipt manifest', 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if m_sha != want("launch-manifest-sha256:")? {', 'if false && m_sha != want("launch-manifest-sha256:")? {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M233', 'B2: the bundle observation is the receipt observation', 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if o_sha != want("preflight-observation-sha256:")? {', 'if false && o_sha != want("preflight-observation-sha256:")? {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M234', 'B2: the observation names its signer', 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if o.observer_key_id != signer {', 'if false && o.observer_key_id != signer {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M235', 'B2: the observation joins the manifest', 'crates/axon-loop-contracts/src/protected_evidence.rs', '    o.joins(&m, &m_sha)?;\n', '', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M236', 'B2: the manifest joins the request and receipt', 'crates/axon-loop-contracts/src/protected_evidence.rs', '        if manifest != other {', '        if false && manifest != other {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M237', 'B2: the manifest suite is the receipt check-suite', 'crates/axon-loop-contracts/src/protected_evidence.rs', '    if manifest_suite != (sid, sver, sentry) {', '    if false && manifest_suite != (sid, sver, sentry) {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M238', 'B2: the observation is signed under the operator observer root', 'crates/axon-loop-contracts/src/protected_evidence.rs', '        &rooted_keys(TrustAuthority::Observer)?,\n        TrustAuthority::Observer,', '        &serde_json::from_str::<serde_json::Value>(&b.observation_signature).ok().and_then(|v| v["public_key"].as_str().and_then(|h| (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).ok()).collect::<Option<Vec<u8>>>())).into_iter().collect::<Vec<_>>(),\n        TrustAuthority::Observer,', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M239', 'PSV-4 (3b): a Failed verdict needs the keyed failure token', 'crates/axon-fabric/src/psv.rs', 'if keyed != Some(false) {', 'if false && keyed != Some(false) {', 'axon-fabric', '--test psv_dispatch', 'every_forgery_of_the_returned_evidence_is_unknown_for_its_own_reason'),
    ('M240', 'PSV-4 (3b): a Failed verdict needs a failing exit', 'crates/axon-fabric/src/psv.rs', 'if matches!(v.exit_code, Some(0) | None) {', 'if false {', 'axon-fabric', '--test psv_dispatch', 'a_failure_with_a_clean_exit_is_not_a_verdict'),
    ('M241', 'the verdict read is the one the launcher bound', 'crates/axon-fabric/src/psv.rs', 'if bound.as_deref() != Some(axon_psv::sha256_hex(&vbytes).as_str()) {', 'if false && bound.as_deref() != Some(axon_psv::sha256_hex(&vbytes).as_str()) {', 'axon-fabric', '--test psv_dispatch', 'every_forgery_of_the_returned_evidence_is_unknown_for_its_own_reason'),
    ('M242', 'PSV-4 (3b): the guest runner reports Failed only with keyed evidence', 'crates/axon-psv/src/runner.rs', '(GuestStatus::Failed, Some(false), Some(c)) if c != 0 => GuestStatus::Failed,', '(GuestStatus::Failed, _, _) => GuestStatus::Failed,', 'axon-psv', '--test runner', 'a_lone_unkeyed_failure_line_is_not_a_verdict'),
    ('M243', 'PSV-4 (3b): the interpreter keys a failure in its own domain', 'crates/axon-core/src/main.rs', 'failure_token(k, &r.name)', 'completion_token(k, &r.name)', 'axon-core', '--no-default-features --test psv_test_selection', 'a_failure_is_keyed_in_its_own_domain'),
    ('M244', 'PSV-7 (3b): a protected-scope clearance needs an operator-rooted monitor key', 'crates/axon-loop/src/safety.rs', '            if config.protected_scopes.contains(&r.scope) {', '            if false && config.protected_scopes.contains(&r.scope) {', 'axon-loop', '--test protected_class', 'a_monitor_key_planted_in_the_store_never_clears_a_protected_trial'),
    ('M245', 'PSV-7 (3b): admission re-checks a clearance under the monitor root', 'crates/axon-loop/src/admission.rs', '                                    && crate::store::Config::rooted_key(\n                                        &config.monitor_keys,\n                                        &report.issuer_ref,\n                                        axon_loop_contracts::operator_trust::TrustAuthority::Monitor,\n                                    )\n                                    .ok()', '                                    && config.monitor_keys.get(&report.issuer_ref)', 'axon-loop', '--test protected_class', 'a_key_revoked_at_the_operator_root_no_longer_counts'),
    ('M246', 'PSV-3 (c4): a key-holding axon test is non-dumpable', 'crates/axon-core/src/main.rs', 'libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0)', 'libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0)', 'axon-core', '--no-default-features --test psv_test_selection', 'holding_a_completion_key_makes_the_process_non_dumpable'),
    ('M247', 'PSV-3 (c4): a key-holding axon test drops Exec from a granted ceiling', 'crates/axon-core/src/main.rs', '.filter(|e| !e.is_empty() && e != "Exec")', '.filter(|e| !e.is_empty())', 'axon-core', '--no-default-features --test psv_test_selection', 'holding_a_completion_key_spawns_nothing'),
    ('M248', 'PSV-3 (c4): a key-holding axon test with no ceiling still gets none with Exec', 'crates/axon-core/src/main.rs', '.filter(|e| **e != "Exec")', '.filter(|_| true)', 'axon-core', '--no-default-features --test psv_test_selection', 'holding_a_completion_key_spawns_nothing'),
    ('M249', 'PSV-3 (c4): the runner gives the key-holding process no Exec', 'crates/axon-psv/src/runner.rs', '.filter(|e| !e.is_empty() && *e != "Exec")', '.filter(|e| !e.is_empty())', 'axon-psv', '--test runner', 'the_process_holding_k_is_given_no_exec'),
    ('M250', 'PSV-1 (c5): an unreadable module never falls through to a later search dir', 'crates/axon-core/src/lib.rs', '            Err(e) => {\n                errors.push(MergeError {\n                    code: error::E0901,\n                    message: format!(\n                        "module `{path_str}` at {} exists but cannot be read ({e}); the search \\\n                         does not fall through to a later directory",\n                        candidate.display()\n                    ),\n                    file: candidate.display().to_string(),\n                });\n                found = true;\n                break;\n            }', '            Err(_) => {}', 'axon-core', '--no-default-features --test psv_test_selection', 'an_unreadable_suite_module_never_falls_through_to_the_candidate'),
    ('M251', "PSV-1 (r2): a sealed module never supplies an operator module's name", 'crates/axon-core/src/lib.rs', '            if !matches!(op.try_exists(), Ok(false)) {', '            if false && !matches!(op.try_exists(), Ok(false)) {', 'axon-core', '--no-default-features --test psv_test_selection', 'a_sealed_module_never_supplies_an_operator_modules_name'),
    ('M252', "PSV-6 (r1): the observation's epoch joins the trial's", 'crates/axon-loop-contracts/src/protected_evidence.rs', '    if o.epoch != epoch {', '    if false && o.epoch != epoch {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M253', 'PSV-6 (r1): an epoch that moved during observation refuses the launch', 'crates/axon-fabric/src/submit.rs', 'Ok(now) if now == cfg.expected_epoch => Ok((launch, Some(v))),', '_ => Ok((launch, Some(v))),', 'axon-fabric', '--test psv_dispatch', 'an_epoch_that_moves_while_the_observer_runs_refuses_the_launch'),
    ('M254', 'PSV-7: a protected decision re-verifies every counted verdict (LOAD-BEARING — reverify_protected is the sole enforcement site; certifying review wf_bff9835f-4a0 reclassified from equivalent to active)', 'crates/axon-loop/src/admission.rs', '                let signer = reverify_protected(tx, &config, eval, arm, t, v, &rc)?;\n                context_signers.insert((arm.arm_id.clone(), t.trial_id.clone()), signer);', '                if false {\n                    let signer = reverify_protected(tx, &config, eval, arm, t, v, &rc)?;\n                    context_signers.insert((arm.arm_id.clone(), t.trial_id.clone()), signer);\n                }', 'axon-loop', '--test protected_class', 'a_protected_admission_re_verifies_the_execution_leg_from_its_documents'),
    ('M255', 'admission: every counted receipt must CLAIM protected evidence (LOAD-BEARING: the only stop for a store writer substituting a genuinely signed guest-unobserved or development verdict; C8 review wf_ae3a5a74-41e reinstated it from equivalent)', 'crates/axon-loop/src/admission.rs', '    if !axon_loop_contracts::protected_evidence::claims_protected(rc) {', '    if false && !axon_loop_contracts::protected_evidence::claims_protected(rc) {', 'axon-loop', '--test protected_class', 'a_genuinely_signed_unobserved_verdict_cannot_count_in_a_protected_record'),
    ('M256', "PSV-7 (r1): a protected execution leg needs Fabric's execution attestation", 'crates/axon-loop/src/evl.rs', 'if let Err(e) = verify_execution(&d.acf_att, areq, rcpt, config) {', 'if let Err(e) = Ok::<(), String>(()).map(|_| (areq, rcpt)) {', 'axon-loop', '--test protected_class', 'a_relabelled_execution_leg_counts_nothing_in_a_protected_evaluation'),
    ('M257', 'PSV-7 (r1): the execution attestation key is operator-rooted', 'crates/axon-loop/src/evl.rs', '    let key = crate::store::Config::rooted_key(\n        &config.verifier_keys,\n        &issuer,\n        axon_loop_contracts::operator_trust::TrustAuthority::Verifier,\n    )?;', '    let key = config\n        .verifier_keys\n        .get(&issuer)\n        .ok_or_else(|| "no key".to_string())?;', 'axon-loop', '--test protected_class', 'a_relabelled_execution_leg_counts_nothing_in_a_protected_evaluation'),
    ('M258', 'PSV-7 (r1): admission re-verifies the execution leg', 'crates/axon-loop/src/admission.rs', 'crate::evl::verify_execution(&att, &areq, &arc, config).map_err(fail)?;', 'let _ = (&att, &areq, &arc);', 'axon-loop', '--test protected_class', 'a_protected_admission_re_verifies_the_execution_leg_from_its_documents'),
    ('M259', 'PSV-7 (r1): a protected admission cites an execution attestation', 'crates/axon-loop/src/admission.rs', '    let att_ref = v\n        .execution_attestation_ref\n        .as_ref()\n        .ok_or_else(|| fail("it cites no execution attestation".into()))?;', '    let Some(att_ref) = v.execution_attestation_ref.as_ref() else {\n        return Ok(crate::evl::SignedBy {\n            issuer_ref: who,\n            key_id: ctx_key,\n        });\n    };', 'axon-loop', '--test protected_class', 'a_protected_admission_re_verifies_the_execution_leg_from_its_documents'),
    ('M260', "PSV-1 (r2): a sealed module's nested use is judged as sealed", 'crates/axon-core/src/lib.rs', '                            in_sealed(&candidate),\n', '                            false,\n', 'axon-core', '--no-default-features --test psv_test_selection', 'a_sealed_modules_use_never_reaches_an_unimported_suite_module'),
    ('M261', 'PSV-7 (r2): protected counters are the re-verified trials, over the plan population', 'crates/axon-loop/src/admission.rs', '        check_arm_grounding(tx, frozen, eval)?;', '        let _ = (frozen, eval);', 'axon-loop', '--test protected_class', 'a_protected_decision_counts_only_its_re_verified_trials'),
    ('M262', "PSV-7 (r2): the evaluation class is the frozen plan's", 'crates/axon-loop/src/admission.rs', '    if eval.evaluation_class != frozen.evaluation_class {', '    if false && eval.evaluation_class != frozen.evaluation_class {', 'axon-loop', '--test protected_class', 'an_evaluation_class_other_than_the_frozen_plans_is_refused'),
    ('M263', 'PSV-7 (r2): a counted trial re-verifies against its own episode', 'crates/axon-loop/src/admission.rs', '    if ep.identity.trial_id != t.trial_id || ep.identity.task_id != t.task_id {', '    if false && ep.identity.trial_id != t.trial_id {', 'axon-loop', '--test protected_class', 'a_counted_trial_cannot_borrow_another_trials_verdict'),
    ('M264', 'PSV-7 (r2): a protected clearance re-verifies its stored monitor signature', 'crates/axon-loop/src/admission.rs', '                                    && clearance_verifies(tx, &config, report, signature_ref.as_ref()))', '                                    && signature_ref.is_some() | true)', 'axon-loop', '--test protected_class', 'a_forged_unsigned_clearance_clears_nothing'),
    ('M265', 'PSV-3 (r2): a protected host runs nothing outside the protected profile', 'crates/axon-fabric/src/submit.rs', '        if cfg.protected_host.is_some() && p.id != backend::LINUX_MICROVM_PROTECTED.id {', '        if false && p.id != backend::LINUX_MICROVM_PROTECTED.id {', 'axon-fabric', '--test psv_dispatch', 'a_protected_host_runs_nothing_outside_the_protected_profile'),
    ('M266', "FIELD-ORIGIN (r3): the launcher's Python never imports from the caller's cwd", 'scripts/fc_linux_profile.sh', '    R="$(python3 -I -c \'import json,sys;', '    R="$(python3 -c \'import json,sys;', 'axon-fabric', '--test launcher_isolation', 'the_launchers_python_never_imports_from_the_callers_cwd'),
    ('M267', "PSV-7 (r3): a counted trial's episode ran its arm's policy", 'crates/axon-loop/src/admission.rs', '    if ep.policy_ref != arm.policy_ref {', '    if false && ep.policy_ref != arm.policy_ref {', 'axon-loop', '--test protected_class', 'a_protected_record_must_agree_with_its_re_verified_documents'),
    ('M268', "PSV-7 (r3): a counted trial's outcome is its signed verdict", 'crates/axon-loop/src/admission.rs', '        (crate::evl::Outcome::VerifiedPass, RV::Passed)\n        | (crate::evl::Outcome::Fail, RV::Failed) => {}', '        (_, _) if true => {}', 'axon-loop', '--test protected_class', 'a_protected_record_must_agree_with_its_re_verified_documents'),
    ('M269', "PSV-7 (r3): a counted trial's context signature re-verifies", 'crates/axon-loop/src/admission.rs', '    )\n    .map_err(|e| fail(format!("context signature: {e}")))?;', '    )\n    .unwrap_or_else(|_| {\n        t.context_signed_by\n            .clone()\n            .map(|s| s.key_id)\n            .unwrap_or_default()\n    });', 'axon-loop', '--test protected_class', 'a_protected_record_must_agree_with_its_re_verified_documents'),
    ('M270', "PSV-1: a draw comes from the RUNNING frame's kernel (a sealed frame never draws the operator stream)", 'crates/axon-core/src/interp.rs', '        Ok(self.k().rng_next())', '        Ok(self.kernels[0].rng_next())', 'axon-core', '--no-default-features --test psv_test_selection', 'sealed_rng_activity_never_moves_the_operators_stream'),
    ('M271', "PSV-1: srand reseeds only the RUNNING frame's kernel (a sealed srand never reseeds the operator stream)", 'crates/axon-core/src/interp.rs', '        self.k().rng_set(n)', '        self.kernels[0].rng_set(n)', 'axon-core', '--no-default-features --test psv_test_selection', 'a_sealed_candidate_cannot_reseed_the_rng'),
    ('M272', "PSV-1: the sealed kernel's stream is seeded by a one-way derivation (its draws reveal nothing of the operator seed)", 'crates/axon-core/src/interp.rs', '            x = if self.rng_sealed {\n                sealed_rng_seed(s)\n            } else {\n                s\n            };', '            x = s;', 'axon-core', '--no-default-features --test psv_test_selection', 'a_sealed_candidates_own_stream_reveals_nothing_of_the_operators'),
    ('M273', 'D1: a caller --grant-registry is refused on a protected host (every route)', 'crates/axon-fabric/src/bin/axon-fabric.rs', '            if a.has("--grant-registry") {\n                refuse_caller_grant_registry();', '            if false && a.has("--grant-registry") {\n                refuse_caller_grant_registry();', 'axon-fabric', '--test grant_registry_authority', 'a_protected_host_refuses_a_caller_grant_registry_on_every_route'),
    ('M274', 'D1: submit refuses a caller --grant-registry on a protected host before reading the request', 'crates/axon-fabric/src/bin/axon-fabric.rs', '    if host.is_some() && a.has("--grant-registry") {', '    if false && host.is_some() && a.has("--grant-registry") {', 'axon-fabric', '--test grant_registry_authority', 'a_protected_host_refuses_a_caller_grant_registry_on_every_route'),
    ('M275', "D1: submit() refuses a grant registry that is not the protected host's pin", 'crates/axon-fabric/src/submit.rs', '            Some(pin) if !pin.eq_ignore_ascii_case(cfg.grants.sha256()) => {', '            Some(pin) if false && !pin.eq_ignore_ascii_case(cfg.grants.sha256()) => {', 'axon-fabric', '--test grant_registry_authority', 'submit_refuses_any_registry_but_the_protected_hosts_pin'),
    ('M276', 'D1: submit() authorizes nothing on a protected host that pins no grant registry', 'crates/axon-fabric/src/submit.rs', '            None => {\n                return Err(SubmitError::Unauthorized(\n                    crate::protected_host::NO_GRANT_REGISTRY.to_string(),', '            None => if false {\n                return Err(SubmitError::Unauthorized(\n                    crate::protected_host::NO_GRANT_REGISTRY.to_string(),', 'axon-fabric', '--test grant_registry_authority', 'submit_refuses_any_registry_but_the_protected_hosts_pin'),
    ('M277', 'D1: the operator grant registry is parsed only at its pin (every use)', 'crates/axon-fabric/src/grants.rs', '        if !got.eq_ignore_ascii_case(pin) {', '        if false && !got.eq_ignore_ascii_case(pin) {', 'axon-fabric', '--test grant_registry_authority', 'the_operator_grant_registry_is_used_only_at_its_pin'),
    ('M278', 'D1: every grant file the operator registry names is operator-owned', 'crates/axon-fabric/src/protected_host.rs', '                    owned(g, false).map_err(bad)?;', '                    let _ = g;', 'axon-fabric', '--test grant_registry_authority', 'every_grant_file_must_be_operator_owned'),
    ('M279', 'D1 (dev): status/cancel accept only the registry whose sha256 the op recorded', 'crates/axon-fabric/src/bin/axon-fabric.rs', '        if recorded != Some(grants.sha256()) {', '        if false && recorded != Some(grants.sha256()) {', 'axon-fabric', '--test grant_registry_authority', 'development_status_and_cancel_accept_only_the_registry_that_authorized_the_op'),
    ('M280', "D1: the op's intent records the authorizing grant registry's sha256", 'crates/axon-fabric/src/submit.rs', '                "registry_sha256": cfg.grants.sha256(),\n', '', 'axon-fabric', '--test grant_registry_authority', 'development_status_and_cancel_accept_only_the_registry_that_authorized_the_op'),
    ('M281', 'D1: status/cancel write nothing to the journal before authorization', 'crates/axon-fabric/src/bin/axon-fabric.rs', '    let j = Journal::open_unreconciled(PathBuf::from(a.req("--journal")))', '    let _ = Journal::open(PathBuf::from(a.req("--journal")));\n    let j = Journal::open_unreconciled(PathBuf::from(a.req("--journal")))', 'axon-fabric', '--test grant_registry_authority', 'status_and_cancel_authorize_before_any_write_and_reconcile_only_their_scope'),
    ('M282', "D1: status/cancel reconcile only the authorized op's scope", 'crates/axon-fabric/src/journal.rs', '.filter(|v| v.state == s && scope.is_none_or(|sc| &v.intent.scope == sc))', '.filter(|v| v.state == s)', 'axon-fabric', '--test grant_registry_authority', 'status_and_cancel_authorize_before_any_write_and_reconcile_only_their_scope'),
    ('M283', 'D1: an unreconciled journal refuses every append over a torn tail', 'crates/axon-fabric/src/journal.rs', '        if g.torn_at.is_some() {', '        if false && g.torn_at.is_some() {', 'axon-fabric', '--test grant_registry_authority', 'an_unreconciled_journal_refuses_writes_over_a_torn_tail'),
    ('M284', 'readiness: git runs with the caller environment cleared', 'crates/axon-fabric/src/git_data.rs', 'c.env_clear()', 'c.env_remove("AXON_NO_SUCH_VAR")', 'axon-fabric', '--test guest_provenance', 'the_callers_git_dir_does_not_answer_the_development_lineage_check'),
    ('M285', 'readiness: refs/replace/ replacements are refused', 'crates/axon-fabric/src/readiness.rs', 'if let Some(r) = replaced.lines().next() {', 'if let Some(r) = replaced.lines().next().filter(|_| false) {', 'axon-fabric', '--test readiness', 'a_replaced_head_commit_is_not_certified'),
    ('M286', 'readiness: an info/grafts file is refused', 'crates/axon-fabric/src/readiness.rs', 'if std::fs::symlink_metadata(&grafts).is_ok() {', 'if false && std::fs::symlink_metadata(&grafts).is_ok() {', 'axon-fabric', '--test readiness', 'grafted_ancestry_is_not_certified'),
    ('M287', 'readiness: a skip-worktree index entry is refused', 'crates/axon-fabric/src/readiness.rs', "if tag == b'S' || tag == b's' {", 'if false {', 'axon-fabric', '--test readiness', 'a_skip_worktree_entry_is_not_certified'),
    ('M288', 'readiness: an assume-unchanged index entry is refused', 'crates/axon-fabric/src/readiness.rs', 'if tag.is_ascii_lowercase() {', 'if false && tag.is_ascii_lowercase() {', 'axon-fabric', '--test readiness', 'an_assume_unchanged_entry_is_not_certified'),
    ('M289', 'readiness: an object must hash to its name', 'crates/axon-fabric/src/git_data.rs', 'if object_id(want, &body) != oid {', 'if false && object_id(want, &body) != oid {', 'axon-fabric', '--test readiness', 'a_forged_object_under_the_certified_name_is_not_certified'),
    ('M290', 'readiness: working-tree bytes are hashed against the certified tree', 'crates/axon-fabric/src/readiness.rs', 'outside.extend(tree_differs(&top, &want, Some(b"governance"), &allow));', 'outside.extend(tree_differs(&top, &want, Some(b"governance"), &allow).into_iter().filter(|_| false));', 'axon-fabric', '--test readiness', 'any_change_to_source_scripts_or_manifests_invalidates_it'),
    ('M291', 'protected host: out_root sits under an operator-owned directory', 'crates/axon-fabric/src/protected_host.rs', 'parent_owned(&out_root)?;', 'parent_owned(&out_root).ok();', 'axon-fabric', '--test protected_host', 'the_out_root_and_custodian_socket_sit_under_operator_owned_directories'),
    ('M292', 'protected host: the custodian socket (formerly the Fabric nonce_store; amendment 50) sits under an operator-owned directory', 'crates/axon-fabric/src/protected_host.rs', 'parent_owned(&socket)?;', 'parent_owned(&socket).ok();', 'axon-fabric', '--test protected_host', 'the_out_root_and_custodian_socket_sit_under_operator_owned_directories'),
    ('M293', 'runner: a Passed verdict needs the keyed token AND exit 0 (the Some(0) of the Passed arm; replaces stale M176)', 'crates/axon-psv/src/runner.rs', '        (GuestStatus::Passed, Some(true), Some(0)) => GuestStatus::Passed,', '        (GuestStatus::Passed, Some(true), _) => GuestStatus::Passed,', 'axon-psv', '--test runner', 'a_genuine_keyed_pass_from_a_run_that_exits_non_zero_is_not_a_pass'),
    ('M294', 'PSV-5: the manifest operation_id joins the request', 'crates/axon-loop-contracts/src/protected_evidence.rs', '("operation_id", &m.operation_id, req.operation_id.as_str()),', '("operation_id", req.operation_id.as_str(), req.operation_id.as_str()),', 'axon-loop', '--test intake', 'each_manifest_join_to_the_request_and_receipt_refuses_its_own_forgery'),
    ('M295', 'PSV-5: the manifest task_id joins the request', 'crates/axon-loop-contracts/src/protected_evidence.rs', '("task_id", &m.task_id, req.task_id.as_str()),', '("task_id", req.task_id.as_str(), req.task_id.as_str()),', 'axon-loop', '--test intake', 'each_manifest_join_to_the_request_and_receipt_refuses_its_own_forgery'),
    ('M296', 'PSV-5: the manifest guest rootfs joins the receipt ref', 'crates/axon-loop-contracts/src/protected_evidence.rs', '            &m.guest.rootfs_sha256,\n', '            want("guest-rootfs-sha256:")?,\n', 'axon-loop', '--test intake', 'each_manifest_join_to_the_request_and_receipt_refuses_its_own_forgery'),
    ('M297', "PSV-5: the manifest guest axon joins the receipt's pinned axon", 'crates/axon-loop-contracts/src/protected_evidence.rs', '            &m.guest.axon_sha256,\n', '            want("guest-axon-sha256:")?,\n', 'axon-loop', '--test intake', 'each_manifest_join_to_the_request_and_receipt_refuses_its_own_forgery'),
    ('M298', 'PSV-5: the manifest guest init joins the receipt ref', 'crates/axon-loop-contracts/src/protected_evidence.rs', '            &m.guest.init_sha256,\n', '            want("guest-init-sha256:")?,\n', 'axon-loop', '--test intake', 'each_manifest_join_to_the_request_and_receipt_refuses_its_own_forgery'),
    ('M299', "PSV-5: the bundle guest verdict is the receipt's guest-verdict-sha256", 'crates/axon-loop-contracts/src/protected_evidence.rs', '    if v_sha != want("guest-verdict-sha256:")? {', '    if false && v_sha != want("guest-verdict-sha256:")? {', 'axon-loop', '--test intake', 'the_guest_verdict_is_joined_to_the_receipt_and_the_manifest'),
    ('M300', 'PSV-5: the guest verdict is axon-guest-verdict/1', 'crates/axon-loop-contracts/src/protected_evidence.rs', '    if v.schema != axon_psv::GUEST_VERDICT_SCHEMA {', '    if false && v.schema != axon_psv::GUEST_VERDICT_SCHEMA {', 'axon-loop', '--test intake', 'the_guest_verdict_is_joined_to_the_receipt_and_the_manifest'),
    ('M301', "PSV-5: the guest verdict names the bundle's launch manifest", 'crates/axon-loop-contracts/src/protected_evidence.rs', '    if v.launch_manifest_sha256 != m_sha {', '    if false && v.launch_manifest_sha256 != m_sha {', 'axon-loop', '--test intake', 'the_guest_verdict_is_joined_to_the_receipt_and_the_manifest'),
    ('M302', "PSV-5: the guest verdict's test is the manifest's", 'crates/axon-loop-contracts/src/protected_evidence.rs', '    if v.test != m.suite.test {', '    if false && v.test != m.suite.test {', 'axon-loop', '--test intake', 'the_guest_verdict_is_joined_to_the_receipt_and_the_manifest'),
    ('M303', "PSV-5: the guest verdict's inputs are the manifest's", 'crates/axon-loop-contracts/src/protected_evidence.rs', '    if !(v.inputs.matches', '    if false && !(v.inputs.matches', 'axon-loop', '--test intake', 'the_guest_verdict_is_joined_to_the_receipt_and_the_manifest'),
    ('M304', 'PSV-5: the guest verdict claims the outcome the receipt counts', 'crates/axon-loop-contracts/src/protected_evidence.rs', '(RV::Passed, GS::Passed) | (RV::Failed, GS::Failed) => {}', '(RV::Passed, _) | (RV::Failed, GS::Failed) => {}', 'axon-loop', '--test intake', 'the_guest_verdict_is_joined_to_the_receipt_and_the_manifest'),
    ('M305', "PSV-5: Fabric's protected bundle carries the guest verdict bytes", 'crates/axon-fabric/src/psv.rs', '        "guest_verdict": String::from_utf8_lossy(guest_verdict),\n', '', 'axon-fabric', '--test psv_dispatch', 'a_verified_observation_makes_the_guest_verdict_protected'),
    ('M310', 'PSV-6: a protected host with no observer launches no protected check', 'crates/axon-fabric/src/submit.rs', '            && cfg.observer.is_none()\n', '            && false\n', 'axon-fabric', '--test psv_dispatch', 'a_protected_host_with_no_observer_launches_nothing'),
    ('M311', 'PSV-4: the evidence bundle travels only with a PROTECTED verdict', 'crates/axon-fabric/src/submit.rs', 'if let (Some(o), crate::psv::EvidenceClass::Protected) =', 'if let (Some(o), _) =', 'axon-fabric', '--test psv_dispatch', 'an_observed_verdict_that_is_not_protected_carries_no_bundle'),
    ('M312', 'PSV-3: a pass needs exactly one KEYED result line (Fabric pass-side keyed check)', 'crates/axon-fabric/src/psv.rs', 'if keyed != Some(true) {', 'if false && keyed != Some(true) {', 'axon-fabric', '--test psv_dispatch', 'a_second_pass_line_over_a_genuine_pass_is_not_a_pass'),
    ('M313', 'PSV-3: the runner refuses to proceed if it cannot make itself non-dumpable', 'crates/axon-psv/src/bin/axon-psv-runner.rs', 'if make_non_dumpable() != 0 {', 'if make_non_dumpable() != 0 && false {', 'axon-psv', '--bin axon-psv-runner', 'tests::a_runner_that_cannot_become_non_dumpable_never_reads_the_secret'),
    ('M314', 'PSV-2: the guest input check runs the undigested-shape refusal', 'crates/axon-psv/src/lib.rs', '        undigested_shape(root).map_err(|e| format!("{what} input holds {e}: refused"))?;\n', '        let _ = undigested_shape(root);\n', 'axon-psv', '--test protocol', 'inputs_holding_what_the_digest_cannot_see_are_refused'),
    ('M315', 'PSV-2: a directory holding no digested entry is refused', 'crates/axon-psv/src/lib.rs', 'if below == 0 {', 'if false && below == 0 {', 'axon-psv', '--test protocol', 'inputs_holding_what_the_digest_cannot_see_are_refused'),
    ('M316', 'PSV-2: only an EMPTY ROOT lost+found is exempt', 'crates/axon-psv/src/lib.rs', 'if prefix.is_empty() && name == MKFS_LOST_FOUND {', 'if name == MKFS_LOST_FOUND {', 'axon-psv', '--test protocol', 'inputs_holding_what_the_digest_cannot_see_are_refused'),
    ('M317', 'PSV-2: a directory mode the digest does not record is refused', 'crates/axon-psv/src/lib.rs', 'if !mode_is_normalised(true, false, mode) {', 'if false && !mode_is_normalised(true, false, mode) {', 'axon-psv', '--test protocol', 'inputs_holding_what_the_digest_cannot_see_are_refused'),
    ('M318', 'PSV-2: a file mode the digest does not record is refused', 'crates/axon-psv/src/lib.rs', 'if meta.is_file() && !mode_is_normalised(false, exec, mode) {', 'if false && meta.is_file() && !mode_is_normalised(false, exec, mode) {', 'axon-psv', '--test protocol', 'inputs_holding_what_the_digest_cannot_see_are_refused'),
    # ── C9 round-1 fix wave: FABRIC workstream (M320-M334; matrix A54-A57) ──
    ('M320', 'PSV-6/A54: an execution is attested only with the observed launch in its receipt', 'crates/axon-fabric/src/signing.rs', '    if !observed_launch(receipt) {', '    if false && !observed_launch(receipt) {', 'axon-fabric', '--lib', 'signing::tests::an_unobserved_protected_profile_execution_is_never_attested'),
    ('M321', 'PSV-6/A54: the protected profile offers no interpreter_run', 'crates/axon-fabric/src/backend.rs', '    // round 1; A54).\n    job_kinds: &[JobKind::RegisteredCheck],', '    // round 1; A54).\n    job_kinds: &[JobKind::InterpreterRun, JobKind::RegisteredCheck],', 'axon-fabric', '--test psv_dispatch', 'the_protected_profile_is_never_selected_for_an_execution'),
    ('M322', "PSV-4/A55: the bundle is decided from the FINAL receipt's class", 'crates/axon-fabric/src/submit.rs', '.and_then(|(r, _, _)| crate::psv::EvidenceClass::of_receipt(r))', '.and(Some(crate::psv::EvidenceClass::Protected))', 'axon-fabric', '--test psv_dispatch', 'an_inadmissible_observed_launch_is_never_protected_and_carries_no_bundle'),
    ('M323', 'PSV-4/A55: an inadmissible launch is never protected (psv_receipt downgrade)', 'crates/axon-fabric/src/submit.rs', '    if !launched_ok || !privileged {\n        // Whatever derive saw', '    if !privileged {\n        // Whatever derive saw', 'axon-fabric', '--test psv_dispatch', 'an_inadmissible_observed_launch_is_never_protected_and_carries_no_bundle'),
    ('M324', "A56: the out_root LEAF is the service's own and private", 'crates/axon-fabric/src/protected_host.rs', '        leaf_owned(&out_root)?;', '        leaf_owned(&out_root).ok();', 'axon-fabric', '--test protected_host', 'the_out_root_leaf_is_the_services_own_and_private'),
    ('M325', "A56 -> amendment 50: the nonce store is its owner's (now the custodian's) own and private: no group/other access", 'crates/axon-fabric/src/custodian.rs', '    if m.mode() & 0o077 != 0 {', '    if false && m.mode() & 0o077 != 0 {', 'axon-fabric', '--lib', 'custodian::tests::a_nonce_store_others_can_reach_is_refused'),
    ('M326', 'A56: a service leaf is owned by the service euid', 'crates/axon-fabric/src/protected_host.rs', '    if m.uid() != euid {', '    if false && m.uid() != euid {', 'axon-fabric', '--test protected_host', 'the_out_root_leaf_is_the_services_own_and_private'),
    ('M327', 'A56: a service leaf has no group/other access (0700)', 'crates/axon-fabric/src/protected_host.rs', '    if m.mode() & 0o077 != 0 {', '    if false && m.mode() & 0o077 != 0 {', 'axon-fabric', '--test protected_host', 'the_out_root_leaf_is_the_services_own_and_private'),
    ('M328', 'A57: key-role separation is checked when the host config loads', 'crates/axon-fabric/src/protected_host.rs', '                observer_trust.check_separation().map_err(bad)?;', '                let _ = observer_trust.check_separation();', 'axon-fabric', '--test protected_host', 'an_observer_root_sharing_a_key_with_another_role_is_refused_at_load'),
    ('M329', 'A57: key-role separation is re-checked at every observation', 'crates/axon-fabric/src/observer.rs', '    cfg.trust.check_separation()?;', '    let _ = cfg.trust.check_separation();', 'axon-fabric', '--test psv_dispatch', 'an_observer_key_that_holds_another_role_is_refused_at_every_observation'),
    ('M330', "A57: the observer root never holds the host signer's key (C9 r2: the check moved to backend::exclusive_root_keys, one implementation for every Fabric trust read)", 'crates/axon-fabric/src/backend.rs', '        if mine.contains(&signer) {', '        if false && mine.contains(&signer) {', 'axon-fabric', '--test protected_host', 'an_observer_root_sharing_a_key_with_another_role_is_refused_at_load'),
    ('M331', 'A57: an observer key is in no other authority root (C9 r2: moved to backend::exclusive_root_keys)', 'crates/axon-fabric/src/backend.rs', 'mine.iter().find(|k| theirs.contains(k))', 'mine.iter().find(|k| false && theirs.contains(k))', 'axon-fabric', '--test protected_host', 'an_observer_root_sharing_a_key_with_another_role_is_refused_at_load'),
    ('M332', 'protected host: only NotFound means no host config', 'crates/axon-fabric/src/protected_host.rs', '        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),', '        Err(_) => Ok(false),', 'axon-fabric', '--lib', 'protected_host::tests::only_a_missing_host_config_means_not_a_protected_host'),
    ('M333', 'journal: only NotFound means no journal (status/cancel)', 'crates/axon-fabric/src/journal.rs', '            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),', '            Err(_) => return Ok(None),', 'axon-fabric', '--test journal', 'only_a_missing_journal_is_no_journal'),
    ('M334', 'preflight: the probe list carries every grant file load pins', 'crates/axon-fabric/src/protected_host.rs', '                .map(|g| (OperatorFile, g.to_path_buf())),', '                .map(|g| (OperatorFile, g.to_path_buf()))\n                .filter(|_| false),', 'axon-fabric', '--test protected_host', 'the_preflight_probe_list_is_exactly_what_load_enforces'),
    # ── C9 round 1, workstream READINESS (M335-M349): one read per authority
    # decision (PSV-7), readiness attribution joined, build provenance (FIELD-ORIGIN).
    ('M335', 'PSV-7: the operator signature is verified over the SAME read of the record the fields were checked on', 'crates/axon-fabric/src/readiness.rs', '        &rec_bytes,\n        &crate::backend::read_signature("evidence", &sidecar(&rec))?,', '        &read_once(&rec)?,\n        &crate::backend::read_signature("evidence", &sidecar(&rec))?,', 'axon-fabric', '--test one_read', 'a_record_renamed_between_two_reads_certifies_nothing'),
    ('M336', 'PSV-7: the trust preflight is parsed from the read that was hashed into the bundle', 'crates/axon-fabric/src/readiness.rs', '    let (_, _, pf) = named(&evidence, component, &doc, "trust_preflight_sha256")?;\n    let pf: Value = serde_json::from_slice(pf)', '    let (pf_path, _, _) = named(&evidence, component, &doc, "trust_preflight_sha256")?;\n    let pf: Value = serde_json::from_slice(&read_once(pf_path)?)', 'axon-fabric', '--test one_read', 'a_preflight_renamed_between_hash_and_parse_certifies_nothing'),
    ('M337', 'PSV-7 class: the B263 qualification parses the profile manifest bytes it hashed', 'crates/axon-fabric/src/backend.rs', '        let m: serde_json::Value = serde_json::from_slice(&manifest_bytes)', '        let m: serde_json::Value = serde_json::from_slice(&read_regular(&self.manifest)?)', 'axon-fabric', '--test one_read', 'a_manifest_renamed_between_hash_and_parse_does_not_change_the_qualified_guest'),
    ('M338', "FIELD-ORIGIN: the record's verifier_key_id is a key in the operator verifier root", 'crates/axon-fabric/src/readiness.rs', '        ("verifier_key_id", &trust.verifier_dir, "verifier"),\n', '', 'axon-fabric', '--test readiness_attribution', 'a_verifier_key_id_outside_the_verifier_root_is_refused'),
    ('M339', "FIELD-ORIGIN: the observation is signed by the record's observer_key_id", 'crates/axon-fabric/src/readiness.rs', '    if signer != s("observer_key_id") {', '    if false && signer != s("observer_key_id") {', 'axon-fabric', '--test readiness_attribution', 'an_observation_by_another_observer_key_is_refused'),
    ('M340', 'FIELD-ORIGIN: the certified observation verifies under the operator observer root', 'crates/axon-fabric/src/readiness.rs', '        TrustAuthority::Observer,\n    )\n    .map_err(|e| format!("{component}: {e}"))?;', '        TrustAuthority::Observer,\n    )\n    .unwrap_or_else(|_| s("observer_key_id").to_string());', 'axon-fabric', '--test readiness_attribution', 'an_observation_not_signed_by_an_observer_root_key_is_refused'),
    ('M341', 'FIELD-ORIGIN: the observation observed the certified profile, revision and guest', 'crates/axon-fabric/src/readiness.rs', '.find(|(_, got, want)| got != want)', '.find(|(_, got, want)| got != want && false)', 'axon-fabric', '--test readiness_attribution', 'an_observation_of_another_run_is_refused'),
    ('M342', 'FIELD-ORIGIN: b263_qualification_sha256 names a certified evidence file', 'crates/axon-fabric/src/readiness.rs', '    let (b_path, b_sha, b) = named(evidence, component, doc, "b263_qualification_sha256")?;', '    let (b_path, b_sha, b) = named(evidence, component, doc, "b263_qualification_sha256")\n        .or_else(|_| evidence.last().ok_or(String::new()))?;', 'axon-fabric', '--test readiness_attribution', 'a_b263_digest_that_names_no_evidence_file_is_refused'),
    ('M343', 'FIELD-ORIGIN: the certified B263 record verifies under the operator qualification root', 'crates/axon-fabric/src/readiness.rs', '        TrustAuthority::Qualification,\n    )\n    .map_err(|e| format!("{component}: {e}"))?;', '        TrustAuthority::Qualification,\n    )\n    .or_else(|_| serde_json::from_slice::<Value>(b).map(|v| v["issuer_key_id"].as_str().unwrap_or("").to_string()).map_err(|e| e.to_string()))\n    .map_err(|e| format!("{component}: {e}"))?;', 'axon-fabric', '--test readiness_attribution', 'a_b263_record_not_signed_under_the_qualification_root_is_refused'),
    ('M344', 'FIELD-ORIGIN: the certified guest digests are the B263-qualified artifacts', 'crates/axon-fabric/src/readiness.rs', '        if qualified != Some(s(k)) {', '        if false && qualified != Some(s(k)) {', 'axon-fabric', '--test readiness_attribution', 'a_b263_record_of_another_guest_is_refused'),
    ('M345', 'FIELD-ORIGIN: the certified B263 record is axon-b263-evidence/1 of the protected profile', 'crates/axon-fabric/src/readiness.rs', '    if q["schema"] != "axon-b263-evidence/1" || q["profile"]["name"] != PROTECTED_PROFILE {', '    if false && (q["schema"] != "axon-b263-evidence/1" || q["profile"]["name"] != PROTECTED_PROFILE) {', 'axon-fabric', '--test readiness_attribution', 'a_b263_record_of_another_profile_is_refused'),
    ('M346', 'FIELD-ORIGIN: build provenance counts a skip-worktree entry as dirty', 'crates/axon-fabric/src/provenance.rs', "            if tag == b'S' || tag == b's' {", "            if false && (tag == b'S' || tag == b's') {", 'axon-fabric', '--lib', 'provenance::tests::a_skip_worktree_or_assume_unchanged_change_is_dirty'),
    ('M347', 'FIELD-ORIGIN: build provenance counts an untracked file as dirty', 'crates/axon-fabric/src/provenance.rs', '                "--untracked-files=all",', '                "--untracked-files=no",', 'axon-fabric', '--lib', 'provenance::tests::an_untracked_file_is_dirty'),
    ('M348', 'O1 rule 1: the protected signer key is mode 0400 (no write bit)', 'crates/axon-fabric/src/bin/axon-fabric.rs', 'meta.mode() & 0o277 != 0', 'meta.mode() & 0o077 != 0', 'axon-fabric', '--test grant_registry_authority', 'an_owner_writable_signer_key_signs_nothing'),
    ('M349', 'FIELD-ORIGIN: observation_sha256 names a certified evidence file', 'crates/axon-fabric/src/readiness.rs', '    let (obs_path, _, obs) = named(evidence, component, doc, "observation_sha256")?;', '    let (obs_path, _, obs) = named(evidence, component, doc, "observation_sha256")\n        .or_else(|_| evidence.get(2).ok_or(String::new()))?;', 'axon-fabric', '--test readiness_attribution', 'an_observation_digest_that_names_no_evidence_file_is_refused'),
    # ── C9 round-1 fix wave, workstream INPUTS (PSV-2: extended attributes on a
    # guest input — what neither the tree digest nor the mode check can see).
    ('M350', 'PSV-2 (c9r1): the input ROOT carrying an extended attribute is refused', 'crates/axon-psv/src/lib.rs', '    no_xattr(root, ".")?;\n', '    let _ = no_xattr(root, ".");\n', 'axon-psv', '--test protocol', 'inputs_carrying_an_extended_attribute_are_refused'),
    ('M351', 'PSV-2 (c9r1): an input entry carrying an extended attribute is refused', 'crates/axon-psv/src/lib.rs', '            no_xattr(&d.path(), &path)?;\n', '            let _ = no_xattr(&d.path(), &path);\n', 'axon-psv', '--test protocol', 'inputs_carrying_an_extended_attribute_are_refused'),
    ('M352', 'PSV-2 (c9r1): EVERY xattr namespace is refused, not a list (a user.*-only check misses an ACL)', 'crates/axon-psv/src/lib.rs', '.find(|s| !s.is_empty())', '.find(|s| s.starts_with(b"user."))', 'axon-psv', '--test protocol', 'inputs_carrying_an_extended_attribute_are_refused'),
    ('M353', 'PSV-2 (c9r1): the real runner refuses an ACL-carrying input instead of running it to a keyed Failed', 'crates/axon-psv/src/lib.rs', '            no_xattr(&d.path(), &path)?;\n', '            let _ = no_xattr(&d.path(), &path);\n', 'axon-psv', '--test runner', 'an_input_carrying_an_acl_is_refused_not_run'),
    # C9 round 1b, DECISION for the LAYERED rows M354/M355 (+ M403): each
    # row's property is its OWN LAYER'S OUTPUT, so they stay ACTIVE and are
    # KILLED by the test of that output. The launcher images every input and
    # the job drive without xattrs (M354, M355); in the guest, the runner
    # refuses candidate/suite entries (M350-M353) and job files (M403) that
    # carry one. The former guest `noacl` mount rows M356-M358 are GONE: the
    # guest kernel's ext4 has no `noacl` option (the boot test's guest rebooted
    # on "ext4: Unknown parameter 'noacl'"); their textual test passed while the
    # guest could not boot. The end-to-end property is observable only in a
    # booted microVM, so no joint cell is executed and nothing is retired.
    ('M354', "PSV-2 (c9r1): the launcher's input copy preserves no ACL/xattr", 'scripts/fc_linux_profile.sh', '    cp -R "$2/." "$st/" || return 1\n', '    cp -a "$2/." "$st/" || return 1\n', 'axon-fabric', '--test launcher_isolation', 'the_launchers_input_image_carries_no_extended_attribute'),
    ('M355', "PSV-2 (c9r1): the launcher's mkfs copies no xattr into an input image", 'scripts/fc_linux_profile.sh', '-E root_owner=0:0,no_copy_xattrs -d "$st"', '-E root_owner=0:0 -d "$st"', 'axon-fabric', '--test launcher_isolation', 'the_launchers_input_image_carries_no_extended_attribute'),
    # ── C9 round 1, integration: a stat error on the narrowing list is not "absent".
    ('M369', 'FIELD-ORIGIN (A61): a narrowing list that is present but unreadable is not read as absent', 'crates/axon-fabric/src/readiness.rs', '    let narrowed = match std::fs::symlink_metadata(&exp) {', '    let narrowed = match std::fs::metadata(&exp) {', 'axon-fabric', '--test readiness', 'a_narrowing_list_that_cannot_be_read_is_not_read_as_absent'),
    # ── C9 round 1, workstream LOOP (M360-M374): attribution joined to the
    # re-verified signer (A62), key-role separation at the operator roots (A63),
    # a protected manifest names its host (A64).
    # C9 round 1b (LOOP): with M360/M361 in place M209 and M210 are retired
    # EQUIVALENT (four-cell vs M360 / M361; all-paths in EQUIV_RECORD). Their
    # attack (an identity the operator root never held) is refused by the join
    # to the re-verified signer on every path. M104 (presence) stays sole.
    ('M360', 'PSV-5 (A62): a protected verdict\'s recorded verifier and key ARE the signer that re-verified', 'crates/axon-loop/src/admission.rs', '    if ep.verification.issuer_ref.as_ref() != Some(&v.issuer_ref) || signed_key != v.key_id {', '    if false && ep.verification.issuer_ref.as_ref() != Some(&v.issuer_ref) || signed_key != v.key_id && false {', 'axon-loop', '--test protected_attribution', 'a_verdict_attributed_to_another_rooted_verifier_does_not_count'),
    ('M361', 'PSV-5 (A62): a protected context\'s context_signed_by IS the observer and key that re-verified it', 'crates/axon-loop/src/admission.rs', '                    if c != signer {', '                    if false && c != signer {', 'axon-loop', '--test protected_attribution', 'a_context_attributed_to_another_rooted_observer_does_not_count'),
    ('M362', 'PSV-5 (A62): a protected context\'s context_observer_ref IS the observer that signed it', 'crates/axon-loop/src/admission.rs', '    if t.context_observer_ref.as_ref() != Some(&who) {', '    if false && t.context_observer_ref.as_ref() != Some(&who) {', 'axon-loop', '--test protected_attribution', 'a_context_admitted_under_another_trusted_observer_does_not_count'),
    ('M363', 'PSV-6 (A63): rooted_keys refuses a root sharing a key with another operator root (check_bundle observer root)', 'crates/axon-loop-contracts/src/operator_trust.rs', '    for k in &keys {\n        exclusive(a, k)?;\n    }\n', '    for k in &keys {\n        let _ = (a, k);\n    }\n', 'axon-loop', '--test intake', 'an_observer_key_held_by_another_operator_root_authenticates_no_observation'),
    ('M364', 'PSV-6 (A63): rooted refuses a key another operator root also holds (verifier/observer/monitor lookups)', 'crates/axon-loop-contracts/src/operator_trust.rs', '        exclusive(a, &want)\n', '        Ok(())\n', 'axon-loop', '--test intake', 'a_verifier_key_held_by_another_operator_root_authenticates_no_verdict'),
    ('M365', 'PSV-6 (A63): exclusive compares the key with every other operator root', 'crates/axon-loop-contracts/src/operator_trust.rs', '        if root_keys_hex(b)?.contains(&want) {', '        if false && root_keys_hex(b)?.contains(&want) {', 'axon-loop', '--test intake', 'an_observer_key_held_by_another_operator_root_authenticates_no_observation'),
    ('M366', 'PSV-7 (A64): check_bundle refuses a manifest with an all-zero digest', 'crates/axon-loop-contracts/src/protected_evidence.rs', '    names_every_digest(&m)?;\n', '    let _ = names_every_digest(&m);\n', 'axon-loop', '--test intake', 'a_protected_manifest_naming_no_operator_host_is_refused'),
    ('M367', 'PSV-7 (A64): an all-zero sha256 is the refused placeholder', 'crates/axon-loop-contracts/src/protected_evidence.rs', '        if d.bytes().all(|b| b == b\'0\') {', '        if false && d.bytes().all(|b| b == b\'0\') {', 'axon-loop', '--test intake', 'a_protected_manifest_naming_no_operator_host_is_refused'),
    ('M368', 'PSV-7 (A64): nested manifest digests (suite.registry_sha256) are walked too', 'crates/axon-loop-contracts/src/protected_evidence.rs', '                walk(&p, x, out);\n', '                let _ = (&p, x);\n', 'axon-loop', '--test intake', 'a_protected_manifest_naming_no_operator_host_is_refused'),
    # ── C9 round 1b, workstream LOOP (M425-M434): the consumer-side join on
    # the protected EXECUTION leg (class b). verify_execution (EVL's protected
    # leg AND admission's re-derivation) requires the attested receipt itself
    # to state an observed protected launch: a protected backend, the one class
    # `protected`, and the launch-manifest and preflight-observation digests.
    # An attestation a pre-A54 Fabric issued over an unobserved launch still
    # verifies under the same operator-rooted key.
    ('M425', 'C9r1b class-b join: an attested execution counts only if its receipt states the ONE evidence class protected', 'crates/axon-loop/src/evl.rs', '    if classes != ["protected"] {', '    if false && classes != ["protected"] {', 'axon-loop', '--test protected_class', 'an_unobserved_execution_leg_counts_nothing_in_a_protected_evaluation'),
    ('M426', 'C9r1b class-b join: an attested execution counts only if its receipt names its launch manifest', 'crates/axon-loop/src/evl.rs', '    names_one_sha256(rc, "launch-manifest-sha256:")?;\n', '    let _ = names_one_sha256(rc, "launch-manifest-sha256:");\n', 'axon-loop', '--test protected_class', 'an_unobserved_execution_leg_counts_nothing_in_a_protected_evaluation'),
    ('M427', 'C9r1b class-b join: an attested execution counts only if its receipt names its preflight observation', 'crates/axon-loop/src/evl.rs', '    names_one_sha256(rc, "preflight-observation-sha256:")?;\n', '    let _ = names_one_sha256(rc, "preflight-observation-sha256:");\n', 'axon-loop', '--test protected_class', 'an_unobserved_execution_leg_counts_nothing_in_a_protected_evaluation'),
    ('M428', "C9r1b: the protected execution leg's backend is re-checked where admission re-derives (EVL's D3 filter M19 never runs there)", 'crates/axon-loop/src/evl.rs', '    if !axon_loop_contracts::PROTECTED_PROFILES.contains(&rc.backend_profile_ref.as_str()) {', '    if false && !axon_loop_contracts::PROTECTED_PROFILES.contains(&rc.backend_profile_ref.as_str()) {', 'axon-loop', '--test protected_class', 'a_protected_admission_re_checks_the_execution_legs_backend'),
    # ── C9 round 1b, integration: the runner refuses job files carrying an xattr.
    ('M403', 'PSV-2/custody: the runner refuses a job file (dir, secret, manifest) carrying any extended attribute', 'crates/axon-psv/src/runner.rs', '        if let Err(e) = crate::no_xattr(p, what) {', '        if let Err(e) = {\n            let _ = crate::no_xattr(p, what);\n            Ok::<(), String>(())\n        } {', 'axon-psv', '--test runner', 'a_job_file_carrying_an_acl_is_refused_not_run'),
    # ── C9 round 1b, integration: the execution attestation refuses a replay.
    ('M402', 'PSV-6: a replayed execution is never attested, whatever its journal and receipt claim', 'crates/axon-fabric/src/signing.rs', '    if replayed {\n        return Err(REPLAYED);\n    }\n    match ran_under {', '    match ran_under {', 'axon-fabric', '--lib', 'signing::tests::a_replayed_execution_is_never_attested_whatever_its_journal_claims'),
    # ── C9 round 1b, integration: the sealed-module rule holds whoever imports.
    ('M436', "PSV-1: an operator module's use never resolves first to a sealed module's copy of a name an operator dir holds", 'crates/axon-core/src/lib.rs', '    if from_sealed || first_hit_sealed {', '    if from_sealed {', 'axon-core', '--no-default-features --test psv_test_selection', 'an_operator_import_never_resolves_to_a_sealed_modules_copy'),
    # ── C9 round 1, HARNESS workstream (M375-M399) ──────────────────────────
    # check_bundle guards that had no row (C9 dev review, EQUIVALENCE). Each is
    # scored on its OWN attack (scripts/v022_attack_markers.py).
    ('M375', 'PSV-5: the manifest trial_id joins the request', 'crates/axon-loop-contracts/src/protected_evidence.rs', '("trial_id", &m.trial_id, req.trial_id.as_str()),', '("trial_id", req.trial_id.as_str(), req.trial_id.as_str()),', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M376', 'PSV-5: the manifest attempt_id joins the request', 'crates/axon-loop-contracts/src/protected_evidence.rs', '("attempt_id", &m.attempt_id, req.attempt_id.as_str()),', '("attempt_id", req.attempt_id.as_str(), req.attempt_id.as_str()),', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M377', 'PSV-5: the manifest candidate joins the request (alone: EQUIVALENT with M378, see EQUIV_RECORD)', 'crates/axon-loop-contracts/src/protected_evidence.rs', '            &m.candidate.workspace_version,\n            req.workspace_version_ref.as_str(),', '            req.workspace_version_ref.as_str(),\n            req.workspace_version_ref.as_str(),', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M378', 'PSV-5: the manifest candidate joins the receipt (alone: EQUIVALENT with M377, see EQUIV_RECORD)', 'crates/axon-loop-contracts/src/protected_evidence.rs', '            &m.candidate.workspace_version,\n            rc.input_workspace_ref.as_str(),', '            rc.input_workspace_ref.as_str(),\n            rc.input_workspace_ref.as_str(),', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M379', "PSV-5: the manifest candidate joins the request AND the receipt (both joins; the property's live killer for M377/M378)", 'crates/axon-loop-contracts/src/protected_evidence.rs', '        (\n            "candidate",\n            &m.candidate.workspace_version,\n            req.workspace_version_ref.as_str(),\n        ),\n        (\n            "candidate (receipt)",\n            &m.candidate.workspace_version,\n            rc.input_workspace_ref.as_str(),\n        ),\n', '        (\n            "candidate",\n            req.workspace_version_ref.as_str(),\n            req.workspace_version_ref.as_str(),\n        ),\n        (\n            "candidate (receipt)",\n            rc.input_workspace_ref.as_str(),\n            rc.input_workspace_ref.as_str(),\n        ),\n', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M380', "PSV-5: the manifest test joins the request's argv", 'crates/axon-loop-contracts/src/protected_evidence.rs', '            &m.suite.test,\n            req.argv.get(1).map(String::as_str).unwrap_or(""),', '            req.argv.get(1).map(String::as_str).unwrap_or(""),\n            req.argv.get(1).map(String::as_str).unwrap_or(""),', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M381', 'PSV-5: the manifest guest kernel joins the receipt ref', 'crates/axon-loop-contracts/src/protected_evidence.rs', '            &m.guest.kernel_sha256,\n            want("guest-kernel-sha256:")?,', '            want("guest-kernel-sha256:")?,\n            want("guest-kernel-sha256:")?,', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M382', 'PSV-5: the manifest qualification joins the receipt ref', 'crates/axon-loop-contracts/src/protected_evidence.rs', '            &m.qualification_sha256,\n            want("qualification-sha256:")?,', '            want("qualification-sha256:")?,\n            want("qualification-sha256:")?,', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M383', 'B2: the bundle is axon-psv-evidence/2', 'crates/axon-loop-contracts/src/protected_evidence.rs', '    if b.schema != PSV_EVIDENCE_SCHEMA {', '    if false && b.schema != PSV_EVIDENCE_SCHEMA {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    # C9 round 2 (LOOP): ACTIVE again. The retirement's all-paths argument was
    # false (check_bundle runs BEFORE check_pins, and check_pins read the id
    # with split('@')); the killer is the '@'-id attack at check_bundle's own
    # boundary, where this join is the only guard.
    ('M384', "B2: the request's suite (argv[0]) is the manifest's", 'crates/axon-loop-contracts/src/protected_evidence.rs', '    if req.argv.first().and_then(|a| a.strip_prefix("check:")) != Some(sid) {', '    if false && req.argv.first().and_then(|a| a.strip_prefix("check:")) != Some(sid) {', 'axon-loop', '--test intake', 'the_bundle_refuses_a_manifest_suite_the_request_did_not_run'),
    ('M385', 'readiness: git reads no replacement objects (GIT_NO_REPLACE_OBJECTS and --no-replace-objects)', 'crates/axon-fabric/src/git_data.rs', '        .env("GIT_NO_REPLACE_OBJECTS", "1")\n        .env("GIT_CONFIG_NOSYSTEM", "1")\n        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_OPTIONAL_LOCKS", "0")\n        .env("GIT_TERMINAL_PROMPT", "0")\n        .arg("--no-replace-objects")\n', '        .env("GIT_CONFIG_NOSYSTEM", "1")\n        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_OPTIONAL_LOCKS", "0")\n        .env("GIT_TERMINAL_PROMPT", "0")\n', 'axon-fabric', '--test readiness', 'a_replaced_head_commit_is_not_certified'),
    # ── C9 round 1b, workstream PSV (M410-M424): round-1 fixes that had a test
    # but no row, each killed by its own ATTACK where it is the only guard,
    # and the observer_key_id membership check (retired, four-cell).
    ("M410", "PSV-7 class: psv::prepare pins the guest only from the profile manifest bytes the qualification hashed",
     "crates/axon-fabric/src/psv.rs",
     "    if crate::backend::sha256_hex(&pm_bytes) != q.manifest_sha256 {",
     "    if false && crate::backend::sha256_hex(&pm_bytes) != q.manifest_sha256 {",
     "axon-fabric", "--test one_read", "prepare_pins_the_guest_only_from_the_manifest_the_qualification_hashed"),
    ("M411", "PSV-7 class: interpret_linux_result decides from the ONE read of result.json it hashed as evidence",
     "crates/axon-fabric/src/backend.rs",
     "    let r: Option<serde_json::Value> = bytes.and_then(|b| serde_json::from_slice(&b).ok());",
     "    let r: Option<serde_json::Value> = bytes\n        .and(read_regular(&rj).ok())\n        .and_then(|b| serde_json::from_slice(&b).ok());",
     "axon-fabric", "--test one_read", "the_result_json_hashed_as_evidence_is_the_one_interpreted"),
    ("M412", "FIELD-ORIGIN: build provenance counts a refs/replace/ ref as dirty",
     "crates/axon-fabric/src/provenance.rs",
     '                .map(|r| format!("replace ref {r} rewrites what an object id names"))',
     '                .filter(|_| false)\n                .map(|r| format!("replace ref {r} rewrites what an object id names"))',
     "axon-fabric", "--lib", "provenance::tests::a_replace_ref_or_graft_is_dirty"),
    ("M413", "FIELD-ORIGIN: build provenance counts an info/grafts file as dirty",
     "crates/axon-fabric/src/provenance.rs",
     '                .is_ok()\n                .then(|| format!("{g} rewrites ancestry"))',
     '                .is_ok_and(|_| false)\n                .then(|| format!("{g} rewrites ancestry"))',
     "axon-fabric", "--lib", "provenance::tests::a_replace_ref_or_graft_is_dirty"),
    ("M414", "FIELD-ORIGIN: build provenance counts an untracked file ignored by a non-tracked rule as dirty",
     "crates/axon-fabric/src/provenance.rs",
     "        if source.is_empty() || !tracked.contains(&source) {",
     "        if false && (source.is_empty() || !tracked.contains(&source)) {",
     "axon-fabric", "--lib", "provenance::tests::a_file_ignored_by_an_untracked_rule_is_dirty"),
    ("M415", "FIELD-ORIGIN: verify-evidence is never authoritative in a test-trust build",
     "crates/axon-fabric/src/backend.rs",
     "    if test_trust_build {",
     "    if false && test_trust_build {",
     "axon-fabric", "--lib", "backend::tests::verify_evidence_is_authoritative_only_for_the_operators_owned_root_in_production"),
    ("M416", "FIELD-ORIGIN: verify-evidence is authoritative only for the operator's own root, not a caller's --issuers",
     "crates/axon-fabric/src/backend.rs",
     "    if issuers != operator_root {",
     "    if false && issuers != operator_root {",
     "axon-fabric", "--lib", "backend::tests::verify_evidence_is_authoritative_only_for_the_operators_owned_root_in_production"),
    ("M417", "FIELD-ORIGIN: verify-evidence is authoritative only when the operator root passes the ownership walk",
     "crates/axon-fabric/src/backend.rs",
     '    owned.map_err(|e| format!("the operator root fails the ownership walk: {e}"))',
     '    owned\n        .map_err(|e| format!("the operator root fails the ownership walk: {e}"))\n        .or(Ok(()))',
     "axon-fabric", "--lib", "backend::tests::verify_evidence_is_authoritative_only_for_the_operators_owned_root_in_production"),
    ("M418", "FIELD-ORIGIN: the record's observer_key_id is a key in the operator observer root (EQUIVALENT: dominated by M339+M340)",
     "crates/axon-fabric/src/readiness.rs",
     '        ("observer_key_id", &trust.observer_dir, "observer"),\n',
     "",
     "axon-fabric", "--test readiness_attribution", "an_observer_key_id_outside_the_observer_root_is_refused"),
    # ── C9 round 1b, FABRIC workstream rows (M400-M409) ──
    # M400: the protected arm builds no launch for anything but an operator
    # suite with a named test. Replaced a production `expect("checked above")`
    # that PANICKED when M187 was removed (a crash is not a refusal). Mutual
    # pair with M187 (four-cell); the mutation is the historical attack: the
    # candidate's own file judged as the suite.
    ('M400', 'PSV: the protected arm launches only an operator suite with a named test (structured, was a panic)', 'crates/axon-fabric/src/submit.rs',
     '                _ => Err(PROTECTED_SUITE_ONLY.to_string()),',
     '                _ => Ok((file.clone(), req.workspace_version_ref.clone(), filter.clone().unwrap_or_default())),',
     'axon-fabric', '--test psv_dispatch', 'only_an_operator_suite_runs_on_the_protected_profile'),
    # M401: on a protected host the grant registry is the operator's pinned
    # one whatever the caller names (the structural half of D1). Mutual pair
    # with M273 (four-cell).
    ('M401', "D1: a protected host resolves grants from the operator's pinned registry, never a caller path", 'crates/axon-fabric/src/bin/axon-fabric.rs',
     '            h.grants()\n                .unwrap_or_else(|e| refuse("unauthorized", &format!("protected host: {e}"), 7))',
     '            a.opt("--grant-registry").map(|p| axon_fabric::GrantRegistry::load(&PathBuf::from(p))).unwrap_or_else(|| h.grants())\n                .unwrap_or_else(|e| refuse("unauthorized", &format!("protected host: {e}"), 7))',
     'axon-fabric', '--test grant_registry_authority', 'a_protected_host_refuses_a_caller_grant_registry_on_every_route'),
    # ── C9 round 2, KEYS workstream rows (M460-M469; A67, amendment 41) ──
    # One key, one authority root, decided at ONE source: every Fabric and
    # readiness trust-root read goes through backend::exclusive_root_keys,
    # which reads roots with the loop's keys_in (only NotFound is absent).
    ('M460', 'PSV-6 (A67): keys_in reads only a MISSING root as holding nothing; an unreadable root refuses', 'crates/axon-loop-contracts/src/operator_trust.rs',
     '        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,',
     '        Err(e) if e.kind() == std::io::ErrorKind::NotFound || e.kind() == std::io::ErrorKind::PermissionDenied => None,',
     'axon-fabric', '--test trust_root', 'an_unreadable_authority_root_is_never_read_as_holding_no_key'),
    ('M461', "FIELD-ORIGIN (A67): at load, no authority root but the verifier's holds the host signer's key", 'crates/axon-fabric/src/protected_host.rs',
     '            if *a == TrustAuthority::Verifier || (*a == TrustAuthority::Observer && observed) {',
     '            if true {',
     'axon-fabric', '--test protected_host', 'the_host_signer_key_in_any_root_but_the_verifiers_is_refused_at_load'),
    ('M462', "FIELD-ORIGIN (A67): the loaded qualification trust carries the host signer's key, so qualification() re-checks it", 'crates/axon-fabric/src/protected_host.rs',
     '        trust.host_signer_public_key = Some(signer.public_key.clone());',
     '        let _ = &mut trust;',
     'axon-fabric', '--test protected_host', 'a_b263_record_minted_with_the_host_signer_key_never_qualifies'),
    ('M463', "FIELD-ORIGIN (A67): qualification() refuses the host signer's key in the qualification root at every read", 'crates/axon-fabric/src/backend.rs',
     '            self.host_signer_public_key.as_deref(),',
     '            None,',
     'axon-fabric', '--test protected_host', 'a_b263_record_minted_with_the_host_signer_key_never_qualifies'),
    ('M464', 'FIELD-ORIGIN (A67): a trusted issuer key is held by no other authority root (qualification, verify_operator_evidence*)', 'crates/axon-fabric/src/backend.rs',
     '        exclusive_root_keys(a, dir, &sibling_roots(a, dir), owned_from, host_signer)?',
     '        exclusive_root_keys(a, dir, &[], owned_from, host_signer)?',
     'axon-fabric', '--test protected_host', 'a_qualification_key_shared_with_the_verifier_root_never_qualifies'),
    ('M465', 'FIELD-ORIGIN (A67): every readiness trust read is exclusive across the authority roots', 'crates/axon-fabric/src/readiness.rs',
     'crate::backend::exclusive_root_keys(a, dir, &roots, Some(&self.ownership_base), None)',
     'crate::backend::exclusive_root_keys(a, dir, &[], Some(&self.ownership_base), None)',
     'axon-fabric', '--test readiness_attribution', 'a_certification_signed_by_the_host_signer_key_is_refused'),
    ('M466', "PSV-6 (A67): a peer authority root is walked for operator ownership before it decides separation (as the loop's exclusive does)", 'crates/axon-fabric/src/backend.rs',
     '            check_owned_chain(base, peer, true)?;',
     '            let _ = check_owned_chain(base, peer, true);',
     'axon-fabric', '--test readiness_attribution', 'an_agent_owned_peer_root_decides_no_separation'),
    # ── C9 round 2, workstream LOOP (M470-M479): PSV-5 observation signer
    # (A68), manifest digest format (A69), one suite-id parser (M384's source).
    ('M470', "PSV-5 (A68): the observation's signer is a key the store registers for a trusted observer (the store narrows the observer root)", 'crates/axon-loop-contracts/src/protected_evidence.rs',
     '        .filter(|(_, k)| crate::attestation::key_id_of_hex(k).as_deref() == Some(signer.as_str()))',
     '        .filter(|_| true)',
     'axon-loop', '--test intake', 'an_observation_signed_by_a_rooted_key_no_trusted_observer_holds_is_refused'),
    ('M471', "PSV-5 (A68): only TRUSTED observers' registered keys may sign an observation", 'crates/axon-loop/src/store.rs',
     '            .filter(|(who, _)| trusted.contains(*who))',
     '            .filter(|_| true)',
     'axon-loop', '--test intake', 'an_observation_signed_by_a_key_registered_to_an_untrusted_observer_is_refused'),
    ('M472', "PSV-5 (A68): admission joins the recorded observation signer to the one the bundle re-verifies under", 'crates/axon-loop/src/admission.rs',
     '    if v.observation_signed_by != observation_signer {',
     '    if false && v.observation_signed_by != observation_signer {',
     'axon-loop', '--test protected_attribution', 'an_observation_attributed_to_another_trusted_observer_does_not_count'),
    ('M473', "PSV-5 (A69): every protected manifest *sha256 field is 64 lowercase hex", 'crates/axon-loop-contracts/src/protected_evidence.rs',
     '        if !is_sha256_hex(&d) {\n            return Err(format!(\n                "the launch manifest',
     '        if false && !is_sha256_hex(&d) {\n            return Err(format!(\n                "the launch manifest',
     'axon-loop', '--test intake', 'a_protected_manifest_digest_field_that_is_not_a_sha256_is_refused'),
    ('M474', "PSV-5 (A69): Fabric's prepare builds no manifest naming a *sha256 that is not one (verifier 'unknown')", 'crates/axon-fabric/src/psv.rs',
     '        if !is_sha256_hex(&d) {',
     '        if false && !is_sha256_hex(&d) {',
     'axon-fabric', '--test one_read', 'prepare_builds_no_manifest_naming_a_digest_that_is_not_a_sha256'),
    ('M475', "PSV-5 (M384 source): an operator config pinning an ambiguous suite reference is not written", 'crates/axon-loop/src/store.rs',
     '        c.check_suite_refs().map_err(crate::error::refused)?;\n        self.write_json',
     '        self.write_json',
     'axon-loop', '--test intake', 'a_suite_reference_with_a_second_reading_is_never_pinned'),
    ('M476', "PSV-5 (M384 source): a config file pinning an ambiguous suite reference is not read", 'crates/axon-loop/src/store.rs',
     '                c.check_suite_refs().map_err(crate::error::refused)?;\n                Ok(c)',
     '                Ok(c)',
     'axon-loop', '--test intake', 'a_suite_reference_with_a_second_reading_is_never_pinned'),
    ('M477', "PSV-5 (M384 source): Fabric's check registry registers no suite id holding a reference separator", 'crates/axon-cortex/src/runner.rs',
     # C9 r3 (loop, A81): re-anchored; the id is checked by the whole-reference writer.
     '        check_suite_ref(&c.id, &c.workspace_version_ref, &c.entry)?;\n',
     '',
     'axon-cortex', '--test check_executor', 'a_check_suite_id_holding_a_reference_separator_is_never_registered'),
    ('M478', "PSV-5 (A68): an observation key two trusted observers share is attributed to neither", 'crates/axon-loop-contracts/src/protected_evidence.rs',
     '        [one] => (*one).clone(),',
     '        [one, ..] => (*one).clone(),',
     'axon-loop', '--test intake', 'an_observation_key_two_trusted_observers_share_is_attributed_to_neither'),
    # ── C9 round 2, workstream PROVENANCE (M450-M459): how Axon learns
    # whether a tree is the commit it claims to be, from ONE implementation
    # (crates/axon-fabric/src/git_data.rs, shared by readiness, build.rs and
    # the guest-manifest helper). M284/M289/M385 moved there from
    # readiness.rs with the code (same guards, same tests).
    ("M450", "FIELD-ORIGIN: the repository's own git config is refused unless every key is inert",
     "crates/axon-fabric/src/git_data.rs",
     "        if !allowed_key(&key) {",
     "        if false && !allowed_key(&key) {",
     "axon-fabric", "--lib", "provenance::tests::a_filter_driver_in_the_repository_config_never_runs"),
    ("M451", "FIELD-ORIGIN: build provenance hashes the working-tree bytes against HEAD's tree (not git's stat cache)",
     "crates/axon-fabric/src/provenance.rs",
     "    dirty.extend(head_bytes_differ(&top, &revision, allow));",
     "    dirty.extend(head_bytes_differ(&top, &revision, allow).into_iter().filter(|_| false));",
     "axon-fabric", "--lib", "provenance::tests::a_forged_index_stat_entry_does_not_hide_an_edit"),
    ("M452", "PSV-7: git never fetches (no lazy fetch of a missing object, no transport)",
     "crates/axon-fabric/src/git_data.rs",
     '        .env("GIT_NO_LAZY_FETCH", "1")\n        .args(["-c", "protocol.allow=never"])\n',
     "",
     "axon-fabric", "--lib", "git_data::tests::a_git_call_on_a_promisor_repository_fetches_nothing"),
    ("M453", "FIELD-ORIGIN: build provenance refuses a gitfile .git (a repository chosen elsewhere)",
     "crates/axon-fabric/src/git_data.rs",
     "        m if m.is_dir() => Ok(top),",
     "        m if m.is_dir() || m.is_file() => Ok(top),",
     "axon-fabric", "--lib", "git_data::tests::a_gitfile_or_symlinked_git_dir_is_refused"),
    ("M454", "FIELD-ORIGIN: the guest manifest counts the provenance of the tree at manifest time",
     "scripts/linux_profile_manifest.py",
     '    reasons = list(now["dirty"])',
     "    reasons = []",
     "axon-fabric", "--test guest_provenance", "an_edit_during_the_build_is_dirty"),
    ("M455", "FIELD-ORIGIN: a provenance helper that cannot answer reads DIRTY, never clean",
     "scripts/linux_profile_manifest.py",
     '    return {"revision": "unknown", "dirty": [f"cannot tell: {why}"]}',
     '    return {"revision": "unknown", "dirty": []}',
     "axon-fabric", "--test guest_provenance", "a_provenance_helper_that_cannot_run_is_dirty"),
    ("M456", "FIELD-ORIGIN: a guest manifest with no pre-build snapshot is dirty",
     "scripts/linux_profile_manifest.py",
     '        reasons.append("no pre-build provenance snapshot: the tree the artifacts were built from is unknown")',
     "        pass",
     "axon-fabric", "--test guest_provenance", "a_manifest_without_a_pre_build_snapshot_is_dirty"),
    ("M457", "FIELD-ORIGIN: a tree that moved to another revision during the guest build is dirty",
     "scripts/linux_profile_manifest.py",
     "            reasons.append(f\"the tree moved during the build: {pre.get('revision')} -> {now['revision']}\")",
     "            pass",
     "axon-fabric", "--test guest_provenance", "a_tree_that_moved_during_the_build_is_dirty"),
    ("M458", "FIELD-ORIGIN: a tree dirty when the guest build started stays dirty",
     "scripts/linux_profile_manifest.py",
     '        reasons.extend(f"before the build: {r}" for r in pre["dirty"])',
     "        pass",
     "axon-fabric", "--test guest_provenance", "a_tree_dirty_when_the_build_started_is_dirty"),
    ("M459", "FIELD-ORIGIN: the guest build's PCI lineage check refuses an info/grafts file",
     "crates/axon-fabric/src/provenance.rs",
     "    if std::fs::symlink_metadata(top.join(&g)).is_ok() {",
     "    if false && std::fs::symlink_metadata(top.join(&g)).is_ok() {",
     "axon-fabric", "--test guest_provenance", "grafted_ancestry_does_not_pass_the_lineage_check"),
    # ── C9 round 2, workstream BCE (M500-M519): operator decisions C and E
    # (amendment 44). C: every filesystem object in the tree counts for a
    # protected provenance answer; git-ignore rules excuse nothing; only the
    # operator-owned allowlist does (git_data::tree_differs, shared by
    # readiness, build provenance and the guest manifest). E: a gitfile /
    # linked worktree never gives a protected-clean answer. M290 and M451
    # were re-anchored to the rule's call sites (same guards, same tests).
    ("M500", "C (A70): the tree walk reports a file that is not in the tree, however git ignores it",
     "crates/axon-fabric/src/git_data.rs",
     "            } else if !allow.exact.contains(&rel) {",
     "            } else if false && !allow.exact.contains(&rel) {",
     "axon-fabric", "--lib", "provenance::tests::a_gitignored_build_script_is_dirty"),
    ("M501", "C (A70): the tree walk reports a directory that is not in the tree unless the allowlist excuses it",
     "crates/axon-fabric/src/git_data.rs",
     "                if allow.dirs.contains(&rel) {",
     "                if true || allow.dirs.contains(&rel) {",
     "axon-fabric", "--lib", "provenance::tests::a_gitignored_cargo_config_directory_is_dirty"),
    ("M502", "C (A70): an allowlist that is not operator-owned excuses nothing",
     "crates/axon-fabric/src/git_data.rs",
     "        owned_chain(&src.base, &src.path).map_err(|e| {",
     "        owned_chain(&src.base, &src.path).or::<String>(Ok(())).map_err(|e| {",
     "axon-fabric", "--lib", "provenance::tests::an_allowlist_that_is_not_operator_owned_excuses_nothing"),
    ("M503", "C (A70): an allowlist entry that names or holds a tracked path is refused whole",
     "crates/axon-fabric/src/git_data.rs",
     "    if let Some(e) = allow.covers_tracked(tree) {",
     "    if let Some(e) = allow.covers_tracked(tree).filter(|_| false) {",
     "axon-fabric", "--lib", "provenance::tests::an_allowlist_entry_covering_a_source_is_refused"),
    ("M504", "E (A71): readiness certifies only a standalone clone (a gitfile / linked worktree is refused)",
     "crates/axon-fabric/src/readiness.rs",
     '    let found = crate::git_data::discover(&top).map_err(|e| format!("{component}: {e}"))?;',
     '    let found = crate::git_data::discover_linked(&top).map_err(|e| format!("{component}: {e}"))?;',
     "axon-fabric", "--test readiness", "a_linked_worktree_is_not_certified"),
    ("M505", "E (A71): the guest manifest is dirty unless the protected PCI lineage answer descends",
     "scripts/linux_profile_manifest.py",
     """            d["dirty"].append(f"PCI lineage: {lin.get('why')}")""",
     "            pass",
     "axon-fabric", "--test guest_provenance", "a_head_that_does_not_descend_from_the_pci_certification_is_dirty"),
    ("M506", "E (A71): the guest manifest asks for the PCI lineage at all (not only the build's development check)",
     "scripts/linux_profile_manifest.py",
     '    now = provenance(PCI_CERTIFIED)',
     '    now = provenance()',
     "axon-fabric", "--test guest_provenance", "a_head_that_does_not_descend_from_the_pci_certification_is_dirty"),
    # ── C9 round 2, workstream LAUNCHER (M520-M549): operator decisions A
    # (non-root Fabric; the privileged helper axon-protected-launcher) and D
    # (same-byte hash-and-exec). Amendment 45, negative-matrix A72-A75.
    ("M520", "D: an authority program is opened without following a symlink",
     "crates/axon-fabric/src/sealed_exec.rs",
     "        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)",
     "        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)",
     "axon-fabric", "--lib", "sealed_exec::tests::a_symlinked_launcher_is_refused"),
    ("M521", "D: an authority program writable by group or other is refused",
     "crates/axon-fabric/src/sealed_exec.rs",
     "    if id.mode & 0o022 != 0 {",
     "    if false && id.mode & 0o022 != 0 {",
     "axon-fabric", "--lib", "sealed_exec::tests::a_group_writable_launcher_is_refused"),
    ("M522", "D: an authority program owned by neither the operator nor root is refused",
     "crates/axon-fabric/src/sealed_exec.rs",
     "        if id.uid != want && id.uid != 0 {",
     "        if false && id.uid != want && id.uid != 0 {",
     "axon-fabric", "--lib", "sealed_exec::tests::a_launcher_owned_by_another_uid_is_refused"),
    ("M523", "D: an authority program some process holds open for writing is refused at hash time",
     "crates/axon-fabric/src/sealed_exec.rs",
     "        if e.raw_os_error() == Some(libc::EAGAIN) {",
     "        if false && e.raw_os_error() == Some(libc::EAGAIN) {",
     "axon-fabric", "--lib", "sealed_exec::tests::a_launcher_open_for_writing_when_hashed_is_refused"),
    ("M524", "D: a broken read lease (a writer since the hash) refuses the exec",
     "crates/axon-fabric/src/sealed_exec.rs",
     "    if leased && unsafe { libc::fcntl(fd, libc::F_GETLEASE) } != libc::F_RDLCK {",
     "    if false && leased && unsafe { libc::fcntl(fd, libc::F_GETLEASE) } != libc::F_RDLCK {",
     "axon-fabric", "--lib", "sealed_exec::tests::a_launcher_written_in_place_between_hash_and_exec_is_not_executed"),
    ("M525", "D: a script's interpreter reads the VERIFIED descriptor (/dev/fd/N), not the path",
     "crates/axon-fabric/src/sealed_exec.rs",
     '            argv.push(c(format!("/dev/fd/{}", program.fd()).as_bytes())?);',
     "            argv.push(c(program.path.as_os_str().as_bytes())?);",
     "axon-fabric", "--lib", "sealed_exec::tests::a_launcher_swapped_by_rename_between_hash_and_exec_runs_the_verified_bytes"),
    ("M526", "D: a binary is executed from its verified descriptor (execveat AT_EMPTY_PATH), not its path",
     "crates/axon-fabric/src/sealed_exec.rs",
     "                libc::SYS_execveat,\n                self.target,\n                c\"\".as_ptr(),\n                argv.as_ptr(),\n                envp.as_ptr(),\n                libc::AT_EMPTY_PATH,\n",
     "                libc::SYS_execveat,\n                libc::AT_FDCWD,\n                self.argv[0].as_ptr(),\n                argv.as_ptr(),\n                envp.as_ptr(),\n                0,\n",
     "axon-fabric", "--lib", "sealed_exec::tests::a_binary_swapped_by_rename_between_hash_and_exec_runs_the_verified_bytes"),
    ("M527", "D: the child re-checks every verified inode immediately before execveat",
     "crates/axon-fabric/src/sealed_exec.rs",
     "            if check_unchanged(*fd, id, *leased) != 0 {",
     "            if false && check_unchanged(*fd, id, *leased) != 0 {",
     "axon-fabric", "--lib", "sealed_exec::tests::a_launcher_written_in_place_between_hash_and_exec_is_not_executed"),
    ("M528", "D: the descriptor's bytes must be the pinned bytes",
     "crates/axon-fabric/src/sealed_exec.rs",
     "    if v.sha256 != p.sha256 {",
     "    if false && v.sha256 != p.sha256 {",
     "axon-fabric", "--lib", "sealed_exec::tests::bytes_other_than_the_pin_are_refused"),
    ("M529", "D: a #! script never runs through its own (unpinned) interpreter line",
     "crates/axon-fabric/src/sealed_exec.rs",
     "            if program.script {",
     "            if false && program.script {",
     "axon-fabric", "--lib", "sealed_exec::tests::a_script_without_a_pinned_interpreter_is_refused"),
    ("M530", "D: only a regular file is an authority program (a FIFO serves other bytes to each reader)",
     "crates/axon-fabric/src/sealed_exec.rs",
     "    if id.mode & libc::S_IFMT != libc::S_IFREG {",
     "    if false && id.mode & libc::S_IFMT != libc::S_IFREG {",
     "axon-fabric", "--lib", "sealed_exec::tests::a_fifo_at_the_pinned_path_is_refused"),
    ("M531", "A: the helper admits only the configured Fabric uid (kernel-reported real uid)",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "    if caller_uid != c.fabric_uid {",
     "    if false && caller_uid != c.fabric_uid {",
     "axon-fabric", "--test privileged_launcher", "a_non_root_fabric_reaches_a_root_launch_only_through_the_helper"),
    ("M532", "A: every request path is a plainly spelled direct child of the operator's out root",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "    if !plain_name(name) || root.join(name).as_os_str() != p.as_os_str() {",
     "    if false {",
     "axon-fabric", "--lib", "privileged_launcher::tests::a_request_naming_a_path_outside_the_operator_roots_is_refused"),
    ("M534", "A: the three PSV inputs come from ONE inputs dir under the out root",
     "crates/axon-fabric/src/privileged_launcher.rs",
     '            Some(_) => return Err("the three psv inputs are not in one inputs dir".into()),',
     "            Some(_) => {}",
     "axon-fabric", "--lib", "privileged_launcher::tests::a_request_naming_a_path_outside_the_operator_roots_is_refused"),
    ("M535", "A: the request schema is fixed; an unknown field is refused, never ignored",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "#[serde(deny_unknown_fields)]\npub struct LaunchRequest {",
     "pub struct LaunchRequest {",
     "axon-fabric", "--lib", "privileged_launcher::tests::a_request_with_an_unknown_field_is_refused"),
    ("M536", "A: the helper's config never admits root as the Fabric uid",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "    if c.fabric_uid == 0 && !a.test {",
     "    if false && c.fabric_uid == 0 && !a.test {",
     "axon-fabric", "--lib", "privileged_launcher::tests::a_helper_config_admitting_root_as_the_fabric_is_refused"),
    ("M537", "A: the out root is the Fabric uid's private (0700) dir, re-checked on its descriptor",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "    if !is_dir(&st) || st.st_uid != c.fabric_uid || st.st_mode & 0o077 != 0 {",
     "    if !is_dir(&st) || st.st_uid != c.fabric_uid {",
     "axon-fabric", "--test privileged_launcher", "an_out_root_others_can_reach_launches_nothing"),
    ("M538", "A: the root helper copies only inputs the Fabric uid owns",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "        if st.st_uid != owner {",
     "        if false && st.st_uid != owner {",
     "axon-fabric", "--test privileged_launcher", "a_root_owned_file_among_the_inputs_is_never_read_by_the_helper"),
    ("M539", "A: the helper copies no more than the operator's max_input_bytes",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "                if size > *budget {",
     "                if false && size > *budget {",
     "axon-fabric", "--test privileged_launcher", "inputs_over_the_operator_budget_launch_nothing"),
    ("M540", "A: a special file the launch left in the out dir is removed, never handed to Fabric",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "            _ => unsafe { libc::unlinkat(dir, cn.as_ptr(), 0) == 0 },",
     "            _ => true,",
     "axon-fabric", "--test privileged_launcher", "a_special_file_the_launch_leaves_is_never_handed_to_fabric"),
    ("M541", "A: the launcher the helper ran is the one the launch manifest pins",
     "crates/axon-fabric/src/backend.rs",
     "    if report.launcher_sha256.as_deref() != Some(lx.launcher_sha256.as_str()) {",
     "    if false && report.launcher_sha256.as_deref() != Some(lx.launcher_sha256.as_str()) {",
     "axon-fabric", "--test psv_dispatch", "a_helper_that_ran_another_launcher_than_the_pinned_one_yields_no_verdict"),
    ("M542", "A: the helper consumes the Fabric's job files (the secret exists once while the launcher runs)",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "                unsafe { libc::unlinkat(fd.as_raw_fd(), cn.as_ptr(), 0) };",
     "                let _ = &cn;",
     "axon-fabric", "--test privileged_launcher", "the_secret_leaves_fabrics_job_dir_before_the_launcher_runs"),
    ("M543", "A: the snapshot (its copy of the secret) is removed before --verify-result runs",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "    // The snapshot (and its copy of the secret) goes before any further child.\n    let _ = std::fs::remove_dir_all(&p.staging);\n",
     "    // The snapshot (and its copy of the secret) goes before any further child.\n",
     "axon-fabric", "--test privileged_launcher", "the_snapshot_secret_is_gone_before_the_verify_step"),
    ("M544", "A/B: a launch Fabric ran itself (the development route) is never protected",
     "crates/axon-fabric/src/backend.rs",
     "            LaunchRoute::Direct => false,",
     "            LaunchRoute::Direct => true,",
     "axon-fabric", "--test psv_dispatch", "a_dev_route_launch_is_never_attested_protected"),
    ("M545", "A/B: a test-trust helper never attests protected in a production Fabric",
     "crates/axon-fabric/src/backend.rs",
     "            LaunchRoute::Privileged { test_build } => !test_build || fabric_test_build,",
     "            LaunchRoute::Privileged { test_build } => test_build || !test_build || fabric_test_build,",
     "axon-fabric", "--lib", "backend::tests::only_a_production_helper_attests_protected_in_a_production_fabric"),
    ("M546", "A: a protected host refuses a root Fabric",
     "crates/axon-fabric/src/protected_host.rs",
     "    if euid == 0 {",
     "    if false && euid == 0 {",
     "axon-fabric", "--lib", "protected_host::tests::a_root_fabric_is_refused_on_a_protected_host"),
    ("M547", "A: the helper's config runs the launcher the host config pins",
     "crates/axon-fabric/src/protected_host.rs",
     "    } else if helper.launcher.sha256 != host.linux.launcher_sha256 {",
     "    } else if false {",
     "axon-fabric", "--test protected_host", "the_helper_config_must_agree_with_the_host_config"),
    ("M548", "A: the helper's config admits exactly the uid Fabric runs as",
     "crates/axon-fabric/src/protected_host.rs",
     "    let why = if helper.fabric_uid != euid {",
     "    let why = if false {",
     "axon-fabric", "--test protected_host", "the_helper_config_must_agree_with_the_host_config"),
    ("M549", "A: after authenticating the caller the helper is root in every id (the caller cannot signal or trace the launch)",
     "crates/axon-fabric/src/privileged_launcher.rs",
     "            || libc::setresuid(0, 0, 0) != 0",
     "            || libc::setresuid(u32::MAX, 0, 0) != 0",
     "axon-fabric", "--test privileged_launcher", "a_non_root_fabric_reaches_a_root_launch_only_through_the_helper"),
]


# ── C9 round 2, HARNESS workstream (M480-M499; amendment 43) ────────────────
# Load-bearing guards the round-2 EQUIVALENCE review found with no row. Each
# ACTIVE row's test fails on its OWN attack (scripts/v022_attack_markers.py);
# M482 and M487 are retired under the four-cell rule (EQUIV_RECORD below).
_RB = 'crates/axon-fabric/src/backend.rs'
_READ_FLAGS = '        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);'
_SG = 'crates/axon-fabric/src/signing.rs'
_PH = 'crates/axon-fabric/src/protected_host.rs'
_RD = 'crates/axon-fabric/src/readiness.rs'
MUTATIONS += [
    # read_regular, the one reader behind readiness, evidence, signatures,
    # observations and the B263 manifest.
    ('M480', 'PSV-7/readiness: evidence is never read through a symlink (read_regular O_NOFOLLOW)', _RB,
     _READ_FLAGS, '        o.custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);',
     'axon-fabric', '--test one_read', 'a_symlinked_record_or_signature_is_refused'),
    ('M481', 'PSV-7/readiness: a FIFO with no writer cannot hang the verifier (read_regular O_NONBLOCK)', _RB,
     _READ_FLAGS, '        o.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);',
     'axon-fabric', '--test one_read', 'a_signature_fifo_with_no_writer_does_not_hang_readiness'),
    ('M482', 'PSV-7/readiness: nothing read from a FIFO, device or directory is evidence (read_regular is_file)', _RB,
     '    if !md.is_file() {', '    if false && !md.is_file() {',
     'axon-fabric', '--test one_read', 'a_record_served_twice_by_a_fifo_certifies_nothing'),
    # execution_attestation_decision: each arm is the only refusal on its input.
    ('M483', 'PSV-6: a registered check is never attested as an execution', _SG,
     '    if req.job_kind == JobKind::RegisteredCheck {\n        return Err(NOT_AN_EXECUTION);',
     '    if false && req.job_kind == JobKind::RegisteredCheck {\n        return Err(NOT_AN_EXECUTION);',
     'axon-fabric', '--lib', 'signing::tests::a_registered_check_is_never_attested_as_an_execution'),
    ('M484', 'PSV-6: an execution on a non-protected backend is never attested', _SG,
     '        Some(_) => return Err(NOT_PROTECTED_EXECUTION),', '        Some(_) => {}',
     'axon-fabric', '--lib', 'signing::tests::an_execution_on_another_backend_is_never_attested'),
    ('M485', 'PSV-6: an execution whose journal names no backend is never attested', _SG,
     '        None => return Err(KEY_REACHABLE),', '        None => {}',
     'axon-fabric', '--lib', 'signing::tests::an_execution_with_no_journal_backend_is_never_attested'),
    # service_leaf (out_root, nonce_store).
    ('M486', "A56: a service leaf is a real directory", _PH,
     '    if !m.is_dir() {', '    if false && !m.is_dir() {',
     'axon-fabric', '--test protected_host', 'a_regular_file_service_leaf_is_refused'),
    ('M487', "A56: a service leaf is not a symlink", _PH,
     '    if m.file_type().is_symlink() {', '    if false && m.file_type().is_symlink() {',
     'axon-fabric', '--test protected_host', 'a_symlinked_service_leaf_is_refused'),
    # the protected signer key, opened once.
    ('M488', 'O1 rule 1: the protected signer key is never opened through a symlink', 'crates/axon-fabric/src/bin/axon-fabric.rs',
     '            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)',
     '            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)',
     'axon-fabric', '--test grant_registry_authority', 'a_symlinked_signer_key_signs_nothing'),
    # no_xattr fails closed.
    ('M489', 'PSV-2: an entry whose extended attributes cannot be listed is never read as carrying none', 'crates/axon-psv/src/lib.rs',
     '                _ => return Err(unreadable(e)),', '                _ => return Ok(()),',
     'axon-psv', '--lib', 'xattr_tests::an_entry_whose_attributes_cannot_be_listed_is_never_read_as_clean'),
    # readiness: an agent-writable trust root authorizes nothing.
    ('M490', 'readiness: a trust root the verifier can write is refused (require_unwritable enforced)', _RD,
     '                if self.require_unwritable {', '                if false && self.require_unwritable {',
     'axon-fabric', '--lib', 'readiness::tests::a_trust_root_this_process_can_write_authorizes_nothing'),
    ('M491', 'readiness: writable_by_me reports what this process can write', _RD,
     '    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }',
     '    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 && false }',
     'axon-fabric', '--lib', 'readiness::tests::a_trust_root_this_process_can_write_authorizes_nothing'),
    ('M492', 'readiness: the production trust requires unwritable roots', _RD,
     '            require_unwritable: true,', '            require_unwritable: false,',
     'axon-fabric', '--lib', 'readiness::tests::a_trust_root_this_process_can_write_authorizes_nothing'),
]
# ── C9 round 3, integration: readiness judges B263 currency by the host config's
# qualification.max_age_s, one reading with Fabric (M567, A78).
MUTATIONS += [
    ('M567', "PSV-7 (A78): readiness judges B263 currency by the host config's qualification.max_age_s (one reading with Fabric)", 'crates/axon-fabric/src/readiness.rs', '    crate::protected_host::qualification_max_age_s(&v)\n', '    let _ = &v;\n    Ok(DEFAULT_EVIDENCE_MAX_AGE_S)\n', 'axon-fabric', '--test readiness_attribution', 'readiness_judges_b263_currency_by_the_host_configs_maximum_age'),
]
# ── C9 round 3, workstream READINESS (M570-M584; A78-A80, amendment 47) ──
# PSV-7: readiness applies Fabric's B263 acceptance rules (ONE function,
# backend::accept_b263) to the record a certification names, at decision time,
# and joins the observation to it. Decision E by what git acts on (discover's
# common-dir check, the freeze script's), and lineage from hash-checked objects.
_QB = 'crates/axon-fabric/src/backend.rs'
_QR = 'crates/axon-fabric/src/readiness.rs'
_QG = 'crates/axon-fabric/src/git_data.rs'
_QF = 'scripts/v022_freeze_manifest.py'
_QA = '--test readiness_attribution'
MUTATIONS += [
    ('M570', 'PSV-7: a B263 record with a FAIL assertion certifies nothing (RULE:fail-zero, shared)', _QB,
     '    if ev["counts"]["FAIL"].as_u64() != Some(0) || !failed.is_empty() {',
     '    if false && (ev["counts"]["FAIL"].as_u64() != Some(0) || !failed.is_empty()) {',
     'axon-fabric', _QA, 'a_failed_b263_record_is_refused'),
    ('M571', 'PSV-7: only a PASS (or waived PASS_WITH_BLOCKED) B263 record certifies (RULE:result, shared)', _QB,
     '    if !((result == "PASS" && blocked.is_empty())',
     '    if false && !((result == "PASS" && blocked.is_empty())',
     'axon-fabric', _QA, 'a_b263_record_whose_result_is_not_pass_is_refused'),
    ('M572', 'PSV-7: a stale B263 record certifies nothing (RULE:end-fresh, shared)', _QB,
     '    if (now - end) as u64 > max_age_s {',
     '    if false && (now - end) as u64 > max_age_s {',
     'axon-fabric', _QA, 'a_stale_b263_record_is_refused'),
    ('M573', 'PSV-7: a B263 record from a dirty tree certifies nothing (RULE:tree-clean, shared)', _QB,
     '    if ev["source"]["tree_dirty"] != serde_json::Value::Bool(false) {',
     '    if false && ev["source"]["tree_dirty"] != serde_json::Value::Bool(false) {',
     'axon-fabric', _QA, 'a_b263_record_from_a_dirty_tree_is_refused'),
    ('M574', 'PSV-7: a B263 record naming no host certifies nothing (RULE:host, shared)', _QB,
     '    if host.is_empty() {', '    if false && host.is_empty() {',
     'axon-fabric', _QA, 'a_b263_record_that_names_no_host_is_refused'),
    ('M575', 'PSV-7: the observed launch ran the B263-qualified firecracker', _QR,
     '    if o.firecracker_sha256 != b263.firecracker_sha256 {',
     '    if false && o.firecracker_sha256 != b263.firecracker_sha256 {',
     'axon-fabric', _QA, 'a_b263_record_of_another_engine_is_refused'),
    ('M576', 'PSV-7: readiness judges B263 currency at decision time with Fabric\'s maximum age', _QR,
     'now, max_age_s, || {', 'now, u64::MAX, || {',
     'axon-fabric', _QA, 'a_b263_qualification_that_lapsed_after_certification_is_refused'),
    ('M577', 'PSV-7: a certification is not dated before the run it certifies was observed', _QR,
     '    if observed_at > certified_at {', '    if false && observed_at > certified_at {',
     'axon-fabric', _QA, 'a_certification_dated_before_its_observation_is_refused'),
    ('M578', 'PSV-7: a certification is not dated in the future', _QR,
     '    if certified_at > now {', '    if false && certified_at > now {',
     'axon-fabric', _QA, 'a_certification_dated_in_the_future_is_refused'),
    ('M579', 'PSV-7: a certified B263 waiver verifies under the operator qualification root', _QR,
     '            TrustAuthority::Qualification,\n        )\n        .map_err(|e| format!("{component}: {e}"))?;',
     '            TrustAuthority::Qualification,\n        )\n        .ok();',
     'axon-fabric', _QA, 'a_b263_waiver_not_signed_by_the_operator_is_refused'),
    ('M580', 'Decision E (A79): the repository git acts on is top/.git (no disguised linked worktree)', _QG,
     '    if got != want {', '    if false && got != want {',
     'axon-fabric', '--test readiness', 'a_linked_worktree_disguised_as_a_git_directory_is_not_certified'),
    ('M581', 'A80: lineage is walked over hash-checked commits, never git merge-base', _QG,
     '                return if o.reaches(&head, &target)? {',
     '                return if git_cmd(top).args(["merge-base", "--is-ancestor", rev, "HEAD"]).status().is_ok_and(|s| s.success()) {',
     'axon-fabric', '--test guest_provenance', 'a_forged_ancestor_object_does_not_pass_the_pci_lineage'),
    ('M582', 'Decision E (A79): the freeze refuses a root whose repository is not its own .git', _QF,
     '    if not common or os.path.realpath(common) != os.path.realpath(dotgit):', '    if False:',
     'axon-fabric', '--test freeze_manifest', 'a_linked_worktree_disguised_as_a_git_directory_does_not_freeze'),
    ('M583', 'Decision E: the freeze refuses a root whose .git is not a real directory', _QF,
     '    if os.path.islink(dotgit) or not os.path.isdir(dotgit):', '    if False:',
     'axon-fabric', '--test freeze_manifest', 'a_symlinked_git_dir_does_not_freeze'),
    ('M584', 'Decision E: the freeze refuses a guest manifest that is not clean with no reasons', _QF,
     '    if src.get("axon_tree_dirty_at_build") is not False or src.get("axon_tree_dirty_reasons") != []:',
     '    if False:',
     'axon-fabric', '--test freeze_manifest', 'a_dirty_guest_manifest_does_not_freeze'),
]
# profiles/linux-microvm/guest-init.sh had NO row (round-2 review). Its guards,
# and what covers each (amendment 43):
#   input mounts ro,nodev,nosuid,noexec   rows M493-M496, M607-M609 (the script's
#                                         own mount block run on loop devices,
#                                         /proc/mounts read back: C9 round 3) AND
#                                         boot case `mounts` (in effect in a
#                                         real guest, psv_guest_boot_test.sh)
#   one launch-manifest word (ambiguous)  row M497: the script's own block run
#                                         against fake cmdlines
#   PSV runner under env -i + guest-init  row M498 (the non-PSV route: M499); the
#                                         workload block run, the child's
#                                         environ read back (C9 round 3)
#   /work nosuid,nodev; /out bind mount   boot case `mounts`; the verdict on the
#                                         returned drive: boot cases pass/tampered
#   serial digests (LOADED, VERDICT-INIT) boot cases pass/tampered (host joins)
#   policy report                         b263_profile_wiring policy-report tests
#   cgroup pids/memory ceilings           B263 qualification evidence (guest.json),
#                                         not a PSV verdict property: no row
# A mutation row can only pin the script TEXT; that the kernel honours it is
# the boot test's, and a boot SKIP (77) proves nothing.
_GI = 'profiles/linux-microvm/guest-init.sh'
# C9 round 3 (harness): killed by BEHAVIOUR, not the script text: the
# block runs against loop-device ext4 images and /proc/mounts decides.
_GIT = 'guest_init_sh_psv_input_mounts_are_in_effect_read_only'
MUTATIONS += [
    ('M493', 'PSV-2 guest: the candidate drive is mounted read-only', _GI,
     '-o ro,nodev,nosuid,noexec /dev/vdc', '-o nodev,nosuid,noexec /dev/vdc',
     'axon-guest-init', '--test b263_profile_wiring', _GIT),
    ('M494', 'PSV-2 guest: the suite drive is mounted read-only', _GI,
     '-o ro,nodev,nosuid,noexec /dev/vdd', '-o nodev,nosuid,noexec /dev/vdd',
     'axon-guest-init', '--test b263_profile_wiring', _GIT),
    ('M495', 'PSV-2 guest: the job drive is mounted read-only', _GI,
     '-o ro,nodev,nosuid,noexec /dev/vde', '-o nodev,nosuid,noexec /dev/vde',
     'axon-guest-init', '--test b263_profile_wiring', _GIT),
    ('M496', 'PSV-2 guest: the candidate drive is mounted noexec', _GI,
     '-o ro,nodev,nosuid,noexec /dev/vdc', '-o ro,nodev,nosuid /dev/vdc',
     'axon-guest-init', '--test b263_profile_wiring', _GIT),
    ('M497', 'PSV guest: a cmdline naming two launch manifests stops the guest', _GI,
     '[ "$PSV_WORDS" -le 1 ] || fail "psv-ambiguous"', '[ "$PSV_WORDS" -le 9 ] || fail "psv-ambiguous"',
     'axon-guest-init', '--test b263_profile_wiring', 'guest_init_sh_refuses_two_launch_manifest_words'),
    ('M498', 'PSV guest: the key-holding runner is exec\'d under env -i and axon-guest-init', _GI,
     '        exec env -i PATH=/bin:/usr/bin HOME=/tmp XDG_CACHE_HOME=/tmp/cache \\\n            /usr/bin/axon-guest-init /usr/bin/axon-psv-runner',
     '        exec env PATH=/bin:/usr/bin HOME=/tmp XDG_CACHE_HOME=/tmp/cache \\\n            /usr/bin/axon-guest-init /usr/bin/axon-psv-runner',
     'axon-guest-init', '--test b263_profile_wiring', 'guest_init_sh_psv_runner_starts_with_exactly_the_fixed_environment'),
    ('M499', 'B263 guest: the workload is exec\'d under env -i (non-PSV route)', _GI,
     '    exec env -i PATH=/bin:/usr/bin HOME=/work XDG_CACHE_HOME=/tmp/cache \\\n',
     '    exec env PATH=/bin:/usr/bin HOME=/work XDG_CACHE_HOME=/tmp/cache \\\n',
     'axon-guest-init', '--test b263_profile_wiring', 'guest_init_sh_workload_starts_with_exactly_the_fixed_environment'),
]


# ── C9 round 3, workstream CORE (M560-M569; amendment 46, matrix A76-A77) ───
# PSV-1: a sealed handler frame never answers or aborts an operation performed
# under OPERATOR provenance (live frame, replay feed), and operator code a
# sealed frame runs never draws the operator RNG. PSV-3: a test is Completed
# only when its body evaluated to its end; a declared Result/Option fn never
# returns the other constructor. Each row's test fails on its OWN attack.
_CI = 'crates/axon-core/src/interp.rs'
_CL = '--no-default-features --lib'
MUTATIONS += [
    ('M560', 'PSV-1: a sealed handler frame is skipped for an operator operation (run_handler_arm, the one frame selection)',
     'crates/axon-core/src/interp/eval.rs',
     '                self.handler_may_answer(f.sealed, f.operator_frames)',
     '                { let _ = f; true }',
     'axon-core', _CL, 'interp::tests::a_sealed_handler_never_answers_or_aborts_operator_code'),
    ('M561', 'PSV-1: a sealed arm\'s multi-shot replay feed never answers an operator operation',
     'crates/axon-core/src/interp/builtins.rs',
     '                let may = self.handler_may_answer(r.sealed, r.operator_frames);',
     '                let may = true;',
     'axon-core', _CL, 'interp::tests::a_sealed_handler_never_answers_or_aborts_operator_code'),
    ('M562', 'PSV-1: sealed-frame eligibility counts OPERATOR frames entered since install (not only the current frame\'s provenance)',
     _CI,
     '        !sealed || operator_frames == self.operator_frames.get()',
     '        !sealed || self.frame_sealed.get() || operator_frames == usize::MAX',
     'axon-core', _CL, 'interp::tests::a_sealed_handler_never_answers_or_aborts_operator_code'),
    ('M563', 'PSV-1: operator code a sealed frame runs never draws or reseeds the operator RNG (rng_guard)',
     _CI,
     '        if !self.frame_sealed.get() && self.sealed_frames.get() > 0 {',
     '        if false {',
     'axon-core', _CL, 'interp::tests::operator_code_a_sealed_frame_runs_never_draws_the_operator_rng'),
    ('M564', 'PSV-3: a test whose body did not evaluate to its end is EndedEarly, whatever it returned',
     _CI,
     '        Ok(_) if !interp.test_body_finished.get() => Ok(TestEnd::EndedEarly(',
     '        Ok(_) if false => Ok(TestEnd::EndedEarly(',
     'axon-core', _CL, 'interp::tests::a_test_ended_by_question_mark_is_never_completed'),
    ('M565', 'PSV-3: the test frame records whether its body finished (a return or ? did not)',
     _CI,
     '            self.test_body_finished.set(body_result.is_ok());',
     '            self.test_body_finished.set(true);',
     'axon-core', _CL, 'interp::tests::a_test_ended_by_question_mark_is_never_completed'),
    ('M566', 'PSV-3: a fn declared -> Result (Option) never returns an Option (Result) value',
     _CI,
     '            if confused {',
     '            if false && confused {',
     'axon-core', _CL, 'interp::tests::a_declared_result_fn_never_returns_an_option'),
]
# ── C9 round 3, LOOP workstream (M610-M619; amendment 49; matrix A81-A82) ──
# A81 (PSV-5): the suite join reads the receipt's reference with the ONE parser
# and compares (id, version, entry) field by field; the one writer refuses a
# suite whose reference would read as another (Fabric's registry, prepare).
# A82 (PSV-6): the launch manifest names its authority (epoch, tenant, family),
# the loop joins each to its own scope pointer, and a protected host reads the
# launch-time epoch from the store its config pins. M619 is unallocated.
_PE = 'crates/axon-loop-contracts/src/protected_evidence.rs'
MUTATIONS += [
    ('M610', 'PSV-5 (A81): check_bundle joins the suite field by field through the one parser, not by formatting', _PE,
     '    if manifest_suite != (sid, sver, sentry) {',
     '    if format!("{}@{}#{}", m.suite.id, m.suite.version, m.suite.entry) != receipt_suite["check-suite:".len()..] {',
     'axon-loop', '--test intake', 'a_manifest_suite_that_formats_to_the_pin_but_reads_as_another_is_refused'),
    ('M611', 'PSV-5 (A81): the one writer writes only a suite reference that reads back as that suite', 'crates/axon-cortex/src/runner.rs',
     '    if parse_check_suite_ref(&r)? != (id, version, entry) {',
     '    if false && parse_check_suite_ref(&r)? != (id, version, entry) {',
     'axon-cortex', '--test check_executor', 'a_suite_reference_is_written_only_if_it_reads_back_as_that_suite'),
    ('M612', "PSV-5 (A81): Fabric's prepare builds no manifest whose suite reads as another", 'crates/axon-fabric/src/psv.rs',
     '    let suite_ref = axon_cortex::runner::check_suite_ref(i.suite_id, i.suite_version, i.entry)\n        .map_err(|e| format!("launch manifest suite: {e}"))?;',
     '    let suite_ref = format!("check-suite:{}@{}#{}", i.suite_id, i.suite_version, i.entry);',
     'axon-fabric', '--test one_read', 'prepare_builds_no_manifest_whose_suite_reads_as_another'),
    ('M613', "PSV-5 (A81): Fabric's check registry registers no suite version holding a reference separator", 'crates/axon-cortex/src/runner.rs',
     '        check_suite_ref(&c.id, &c.workspace_version_ref, &c.entry)?;\n',
     '        check_suite_ref(&c.id, "v", &c.entry)?;\n',
     'axon-cortex', '--test check_executor', 'a_suite_version_holding_a_reference_separator_is_never_registered'),
    ('M614', "PSV-6 (A82): the launch manifest's authority epoch is the trial's", _PE,
     '    if m.authority.epoch != epoch {',
     '    if false && m.authority.epoch != epoch {',
     'axon-loop', '--test intake', 'a_launch_whose_manifest_names_another_authority_epoch_is_refused'),
    ('M615', "PSV-6 (A82): the launch manifest's tenant is the trial's", _PE,
     '    if m.authority.tenant_id != scope.tenant_id.as_str() {',
     '    if false && m.authority.tenant_id != scope.tenant_id.as_str() {',
     'axon-loop', '--test intake', 'a_launch_whose_manifest_names_another_scope_is_refused'),
    ('M616', "PSV-6 (A82): the launch manifest's task family is the trial's", _PE,
     '    if m.authority.task_family != scope.task_family.as_str() {',
     '    if false && m.authority.task_family != scope.task_family.as_str() {',
     'axon-loop', '--test intake', 'a_launch_whose_manifest_names_another_scope_is_refused'),
    ('M617', 'PSV-6 (A82): a protected launch reads its epoch from the pinned store, never a caller --store naming another', 'crates/axon-fabric/src/protected_host.rs',
     '            if same(c) != same(&pinned) {',
     '            if false && same(c) != same(&pinned) {',
     'axon-fabric', '--test grant_registry_authority', 'a_protected_launch_reads_its_epoch_only_from_the_pinned_store'),
    ('M618', 'PSV-6 (A82): a protected host that pins no authority store launches nothing', 'crates/axon-fabric/src/protected_host.rs',
     '            None => return Err(NO_AUTHORITY_STORE.to_string()),',
     '            None => return caller.map(Path::to_path_buf).ok_or_else(String::new),',
     'axon-fabric', '--test grant_registry_authority', 'a_protected_host_that_pins_no_authority_store_launches_nothing'),
]


# ── C9 round 3, HARNESS workstream (EQUIVALENCE; M585-M609; amendment 48) ──
# Guards of the decision-A/D path (the setuid helper, same-byte exec) that had
# NO row: the round-3 review removed each and the whole axon-fabric suite
# stayed green. Each row's test attacks the route where that guard is the ONLY
# one. scripts/v022_refusal_coverage.py now fails when a refusal site in the
# protected helper files has neither a row nor a reasoned exemption.
# M594/M595 and M596/M597 are retired as mutual PAIRS (EQUIV_RECORD).
_HPL = 'crates/axon-fabric/src/privileged_launcher.rs'
_HSE = 'crates/axon-fabric/src/sealed_exec.rs'
_HBIN = 'crates/axon-fabric/src/bin/axon-protected-launcher.rs'
_HT = '--test privileged_launcher'
MUTATIONS += [
    ('M585', "A: the helper's config file is not writable by another uid (load_config)", _HPL,
     '    if st.st_uid != a.operator_uid || st.st_mode & 0o022 != 0 {\n        return Err(bad(format!(\n            "must be owned by the operator',
     '    if st.st_uid != a.operator_uid || false {\n        return Err(bad(format!(\n            "must be owned by the operator',
     'axon-fabric', _HT, 'a_helper_config_other_uids_can_write_is_never_obeyed'),
    ('M586', "A: the helper's config file is owned by the operator (load_config)", _HPL,
     '    if st.st_uid != a.operator_uid || st.st_mode & 0o022 != 0 {\n        return Err(bad(format!(\n            "must be owned by the operator',
     '    if false || st.st_mode & 0o022 != 0 {\n        return Err(bad(format!(\n            "must be owned by the operator',
     'axon-fabric', _HT, 'a_helper_config_owned_by_another_uid_is_never_obeyed'),
    ('M587', 'A: every directory above a walked operator path is checked (walk_open)', _HPL,
     '        operator_dir(&here, &st, a)?;\n        here.push(name);',
     '        let _ = operator_dir(&here, &st, a);\n        here.push(name);',
     'axon-fabric', _HT, 'an_out_root_below_a_directory_others_can_write_launches_nothing'),
    ('M588', 'A: an operator directory is owned by the operator (operator_dir)', _HPL,
     '    if st.st_uid != a.operator_uid {\n        return Err(format!(\n            "{} is owned by uid {}, not the operator ({})",',
     '    if false {\n        return Err(format!(\n            "{} is owned by uid {}, not the operator ({})",',
     'axon-fabric', _HT, 'an_out_root_below_a_directory_another_uid_owns_launches_nothing'),
    ('M589', 'A: an operator directory is not group/other-writable (operator_dir)', _HPL,
     '    if st.st_mode & 0o022 != 0 {\n        return Err(format!(\n            "{} is group- or other-writable (mode {:o})",',
     '    if false {\n        return Err(format!(\n            "{} is group- or other-writable (mode {:o})",',
     'axon-fabric', _HT, 'an_out_root_below_a_directory_others_can_write_launches_nothing'),
    ('M590', 'A: the helper re-verifies the pinned kernel/rootfs/firecracker/jailer (verify_inputs)', _HPL,
     '            owner,\n            Lease::IfGranted,\n        )?;\n    }\n    Ok(manifest)',
     '            owner,\n            Lease::IfGranted,\n        ).ok();\n    }\n    Ok(manifest)',
     'axon-fabric', _HT, 'a_boot_input_the_helper_cannot_vouch_for_launches_nothing'),
    ('M591', 'D: an object that cannot be leased is refused under Lease::Required', _HSE,
     '        if lease == Lease::Required {',
     '        if false && lease == Lease::Required {',
     'axon-fabric', '--lib', 'sealed_exec::tests::an_authority_program_that_cannot_be_leased_is_refused_in_production'),
    ('M592', 'D: the inode is re-checked after its bytes are hashed (post-hash unchanged)', _HSE,
     "    // The bytes read are the inode's bytes only if it did not change meanwhile.\n    v.unchanged()?;\n",
     "    // The bytes read are the inode's bytes only if it did not change meanwhile.\n",
     'axon-fabric', '--lib', 'sealed_exec::tests::a_file_changed_while_it_was_hashed_is_not_vouched_for'),
    ('M593', 'D: an authority program over MAX_BYTES is never read', _HSE,
     '    if id.size < 0 || id.size as u64 > MAX_BYTES {',
     '    if id.size < 0 {',
     'axon-fabric', '--lib', 'sealed_exec::tests::an_authority_program_over_the_size_bound_is_never_read'),
    ('M594', 'D: an interpreter that is itself a script is refused (command)', _HSE,
     '            if i.script {\n',
     '            if false && i.script {\n',
     'axon-fabric', '--lib', 'sealed_exec::tests::an_interpreter_that_is_a_script_never_runs_its_own_interpreter_line'),
    ('M595', 'D: a verified descriptor is close-on-exec (a script run by descriptor cannot reach its #! line)', _HSE,
     '    let fd = file.as_raw_fd();\n    let id = identity(fd)',
     '    let fd = file.as_raw_fd();\n    unsafe { libc::fcntl(fd, libc::F_SETFD, 0) };\n    let id = identity(fd)',
     'axon-fabric', '--lib', 'sealed_exec::tests::an_interpreter_that_is_a_script_never_runs_its_own_interpreter_line'),
    ('M596', 'A: the staging root is private (0700)', _HPL,
     '    if st.st_mode & 0o077 != 0 {\n        return Err(format!(\n            "staging root',
     '    if false && st.st_mode & 0o077 != 0 {\n        return Err(format!(\n            "staging root',
     'axon-fabric', _HT, 'the_staged_secret_is_never_readable_by_another_uid'),
    ('M597', "A: each launch's staging dir is created private (0700)", _HPL,
     '    std::fs::DirBuilder::new()\n        .mode(0o700)',
     '    std::fs::DirBuilder::new()\n        .mode(0o755)',
     'axon-fabric', _HT, 'the_staged_secret_is_never_readable_by_another_uid'),
    ('M598', 'A: the staging root is an operator directory (the Fabric cannot swap the snapshot)', _HPL,
     '    operator_dir(&c.staging_root, &st, a)?;',
     '    let _ = operator_dir(&c.staging_root, &st, a);',
     'axon-fabric', _HT, 'a_staging_root_the_fabric_owns_launches_nothing'),
    ('M599', "A: the psv inputs dir is the Fabric uid's (snapshot_inputs)", _HPL,
     '    if st.st_uid != c.fabric_uid {\n        return Err(format!(\n            "psv inputs dir is owned',
     '    if false {\n        return Err(format!(\n            "psv inputs dir is owned',
     'axon-fabric', _HT, 'a_root_owned_inputs_dir_is_never_used_by_the_helper'),
    ('M600', 'A: the jail id is [a-zA-Z0-9-]{1,60} (it names the staging dir)', _HPL,
     '    if !id_ok {\n',
     '    if false && !id_ok {\n',
     'axon-fabric', _HT, 'a_jail_id_holding_a_path_never_stages_outside_the_staging_root'),
    ('M601', 'A: --test-config exists only in a test-trust build of the helper (PRODUCTION build)', _HBIN,
     '[f, p] if f == "--test-config" && axon_fabric::backend::TEST_TRUST_BUILD => {',
     '[f, p] if f == "--test-config" => {',
     'axon-fabric', _HT, 'a_production_helper_never_takes_its_config_from_a_path_its_caller_names'),
    ('M602', 'A: the helper refuses unless its euid is 0 (PRODUCTION build)', _HBIN,
     '    if euid != 0 && !authority.test {',
     '    if false && euid != 0 && !authority.test {',
     'axon-fabric', _HT, 'a_production_helper_that_is_not_root_launches_nothing'),
    ('M603', 'PSV-4: a test-trust helper reports its build as test-trust (build_name)', _HPL,
     '    if crate::backend::TEST_TRUST_BUILD {\n        "test-trust"',
     '    if false {\n        "test-trust"',
     'axon-fabric', _HT, 'a_test_trust_helper_never_reports_itself_as_a_production_build'),
    ('M604', "PSV-4: a report not naming the production build puts the launch on a test route", 'crates/axon-fabric/src/backend.rs',
     '            test_build: report.build != "production",',
     '            test_build: false,',
     'axon-fabric', '--lib', 'backend::tests::a_test_trust_helpers_report_never_puts_a_launch_on_a_production_route'),
    ('M605', "PSV-4: this Fabric build's own trust decides whether a test-trust route attests (PRODUCTION build)", 'crates/axon-fabric/src/backend.rs',
     '    route.may_attest_protected(TEST_TRUST_BUILD)',
     '    route.may_attest_protected(true)',
     'axon-fabric', _HT, 'a_production_fabric_never_lets_a_test_trust_helper_attest_protected'),
    ('M606', "PSV-4: psv_receipt asks attests_protected, never assumes", 'crates/axon-fabric/src/submit.rs',
     '    let privileged = crate::backend::attests_protected(res.route);',
     '    let privileged = true;',
     'axon-fabric', '--test psv_dispatch', 'a_dev_route_launch_is_never_attested_protected'),
    ('M607', 'PSV-2 guest: a later `rw` never undoes the candidate drive\'s `ro`', _GI,
     '-o ro,nodev,nosuid,noexec /dev/vdc', '-o ro,nodev,nosuid,noexec,rw /dev/vdc',
     'axon-guest-init', '--test b263_profile_wiring', _GIT),
    ('M608', 'PSV-2 guest: a later `exec` never undoes the suite drive\'s `noexec`', _GI,
     '-o ro,nodev,nosuid,noexec /dev/vdd', '-o ro,nodev,nosuid,noexec,exec /dev/vdd',
     'axon-guest-init', '--test b263_profile_wiring', _GIT),
    ('M609', 'PSV-2 guest: a later `dev,suid` never undoes the job drive\'s `nodev,nosuid`', _GI,
     '-o ro,nodev,nosuid,noexec /dev/vde', '-o ro,nodev,nosuid,noexec,dev,suid /dev/vde',
     'axon-guest-init', '--test b263_profile_wiring', _GIT),
]


# ── C9 round 3, CUSTODIAN workstream (M620-M639; amendment 50, operator
# decision D6): the nonce is issued, stored and spent by the custodian as its
# own uid (negative-matrix A83); the root helper launches only on its ONE
# observation, verified and spent at the root boundary (A84).
_PL = 'crates/axon-fabric/src/privileged_launcher.rs'
_CU = 'crates/axon-fabric/src/custodian.rs'
_CB = 'crates/axon-fabric/src/bin/axon-custodian.rs'
MUTATIONS += [
    ('M620', 'A84: the root helper launches nothing without a verified observation', _PL,
     '    let epoch = verify_observation_at_root(c, a, req, &m)?;',
     '    let epoch = verify_observation_at_root(c, a, req, &m).unwrap_or(0);',
     'axon-fabric', '--test privileged_launcher', 'a_root_launch_without_an_observation_launches_nothing'),
    ('M621', 'A84: the root helper spends the nonce through the custodian (one nonce, one launch)', _PL,
     '    spend_at_root(c, a, &m.observation_nonce, epoch, &req.psv_manifest_sha256)\n}',
     '    let _ = spend_at_root(c, a, &m.observation_nonce, epoch, &req.psv_manifest_sha256);\n    Ok(())\n}',
     'axon-fabric', '--test privileged_launcher', 'one_observation_launches_the_root_launcher_once'),
    ('M622', 'A84: a nonce spends once (the store\'s atomic rename, in the custodian)',
     'crates/axon-fabric/src/observer.rs',
     '        std::fs::rename(&issued, &used).map_err(|_| format!("nonce {nonce} was already used"))',
     '        std::fs::copy(&issued, &used)\n            .map(drop)\n            .map_err(|_| format!("nonce {nonce} was already used"))',
     'axon-fabric', '--test privileged_launcher', 'one_observation_launches_the_root_launcher_once'),
    ('M623', 'A84: the snapshot the root launcher boots is the manifest the request and its observation name', _PL,
     '    if digest != req.psv_manifest_sha256 {',
     '    if false && digest != req.psv_manifest_sha256 {',
     'axon-fabric', '--test privileged_launcher', 'an_observation_of_another_manifest_launches_nothing'),
    ('M624', 'D6: a dev custodian\'s spend never authorizes a root launch (the helper applies the mode rule)', _PL,
     '    if !custodian_mode_launches(mode, a.test) {',
     '    if false && !custodian_mode_launches(mode, a.test) {',
     'axon-fabric', '--test privileged_launcher', 'a_dev_custodian_never_yields_a_protected_launch'),
    ('M625', 'D6: a production helper launches only on a PROTECTED custodian\'s spend (not a test custodian\'s)', _PL,
     '        (Mode::Protected, _) | (Mode::Test, true)\n',
     '        (Mode::Protected, _) | (Mode::Test, _)\n',
     'axon-fabric', '--lib', 'privileged_launcher::tests::only_a_protected_custodian_authorizes_a_production_launch'),
    ('M626', 'A83: a client (the helper, Fabric) accepts a custodian socket only from the custodian uid or root (SO_PEERCRED)', _CU,
     '        if peer != self.uid && peer != 0 {',
     '        if false && peer != self.uid && peer != 0 {',
     'axon-fabric', '--test privileged_launcher', 'a_custodian_socket_the_fabric_serves_is_refused_by_the_helper'),
    ('M627', 'A83: the custodian issues a nonce to the Fabric uid only (SO_PEERCRED)', _CU,
     '                if peer != self.cfg.fabric_uid {',
     '                if false && peer != self.cfg.fabric_uid {',
     'axon-fabric', '--lib', 'custodian::tests::only_fabric_is_issued_and_only_the_launcher_spends'),
    ('M628', 'A83: only the launcher uid (the root helper) spends a nonce; never the Fabric', _CU,
     '                if peer != self.cfg.launcher_uid {',
     '                if false && peer != self.cfg.launcher_uid {',
     'axon-fabric', '--lib', 'custodian::tests::only_fabric_is_issued_and_only_the_launcher_spends'),
    ('M629', 'A83: a custodian config naming the Fabric uid as the custodian is refused', _CU,
     '        if self.custodian_uid == self.fabric_uid {',
     '        if false && self.custodian_uid == self.fabric_uid {',
     'axon-fabric', '--lib', 'custodian::tests::a_custodian_that_is_the_fabric_is_refused'),
    ('M630', 'A83: the nonce store is owned by the custodian euid', _CU,
     '    if m.uid() != euid {',
     '    if false && m.uid() != euid {',
     'axon-fabric', '--lib', 'custodian::tests::a_nonce_store_others_can_reach_is_refused'),
    ('M631', 'A83: the custodian checks its store before serving any request', _CB,
     '    cu::check_store(&cfg.store, euid()).unwrap_or_else(|e| die(&e));',
     '    let _ = cu::check_store(&cfg.store, euid());',
     'axon-fabric', '--test custodian', 'a_custodian_refuses_a_store_others_can_reach'),
    ('M632', 'A83: the custodian runs only as its configured custodian uid', _CB,
     '    if euid() != c.custodian_uid {',
     '    if false && euid() != c.custodian_uid {',
     'axon-fabric', '--test custodian', 'a_custodian_runs_only_as_its_configured_uid'),
    ('M633', 'A83: the helper config names a custodian that is neither the Fabric uid nor root', _PL,
     '    if !a.test && (c.custodian.uid == c.fabric_uid || c.custodian.uid == 0) {',
     '    if false && !a.test && (c.custodian.uid == c.fabric_uid || c.custodian.uid == 0) {',
     'axon-fabric', '--lib', 'privileged_launcher::tests::a_helper_config_admitting_root_as_the_fabric_is_refused'),
    ('M634', 'A83: the protected host config names a custodian that is neither the Fabric uid nor root', _PH,
     '    if custodian_uid == euid || custodian_uid == 0 {',
     '    if false && (custodian_uid == euid || custodian_uid == 0) {',
     'axon-fabric', '--lib', 'protected_host::tests::a_custodian_that_is_the_fabric_uid_is_refused_on_a_protected_host'),
    ('M635', 'A83: a host config giving Fabric a nonce store is refused', _PH,
     '                if ob.get("nonce_store").is_some() {',
     '                if false && ob.get("nonce_store").is_some() {',
     'axon-fabric', '--test protected_host', 'a_host_config_giving_fabric_a_nonce_store_is_refused'),
    ('M636', 'A83: the helper spends through the custodian the host config names', _PH,
     '        !matches!(&o.custodian, crate::custodian::Custodian::Service(r) if *r == helper.custodian)',
     '        { let _ = o; false }',
     'axon-fabric', '--test protected_host', 'the_helper_config_must_agree_with_the_host_config'),
    ('M637', 'A83/ADR-002: the helper config names the host\'s signer (kept out of its observer root)', _PH,
     '    } else if helper.observer.host_signer_public_key != host.signer.public_key {',
     '    } else if false {',
     'axon-fabric', '--test protected_host', 'the_helper_config_must_agree_with_the_host_config'),
    ('M638', 'A84/ADR-002: at the root boundary the observer root may not hold the host signer\'s key', _PL,
     '            host_signer_public_key: Some(c.observer.host_signer_public_key.clone()),',
     '            host_signer_public_key: None,',
     'axon-fabric', '--test privileged_launcher', 'an_observation_signed_with_the_host_signer_launches_nothing'),
    ('M639', 'PSV-6/A84: at the root boundary the observation is of the manifest\'s authority epoch', _PL,
     '    if o.epoch != m.authority.epoch {',
     '    if false && o.epoch != m.authority.epoch {',
     'axon-fabric', '--test privileged_launcher', 'an_observation_whose_epoch_is_not_the_manifests_launches_nothing'),
]

# ── C9 round 3, ROWS workstream (M640-M649): guards the retirement records
# below rest on that had no row. M640: the custodian config's launcher_uid==0
# rule, which M602's four-cell record (vs M628) needs on the path where the
# custodian is configured with another spender. M641: the protected
# custodian's store-parent rule (fixed here: it listed the parent's entries,
# the store among them, so no correctly deployed protected custodian could
# start; the chain alone is the property).
MUTATIONS += [
    ('M640', 'A83: a protected custodian config lets only uid 0 (the setuid-root helper) spend', _CU,
     '        if self.launcher_uid != 0 {',
     '        if false && self.launcher_uid != 0 {',
     'axon-fabric', '--lib', 'custodian::tests::a_protected_custodian_config_lets_only_root_spend'),
    ('M641', 'A83: a protected custodian serves only from a store whose parent chain is the operator\'s', 'crates/axon-fabric/src/bin/axon-custodian.rs',
     '            axon_fabric::backend::check_operator_chain(parent).unwrap_or_else(|e| die(&e));',
     '            let _ = parent;',
     'axon-fabric', '--test privileged_launcher', 'a_protected_custodian_serves_only_from_a_store_the_operator_placed'),
    # M642 (found while retiring M459): the lineage's revision is resolved by
    # hash only; git's own resolution prefers a ref to an abbreviated hash.
    ('M642', 'FIELD-ORIGIN: the lineage names its revision by hash, never through a ref the repository holds', 'crates/axon-fabric/src/git_data.rs',
     '    let mut target = object_named(top, rev)?;',
     '    let mut target = text(top, &["rev-parse", "--verify", "--end-of-options", &format!("{rev}^{{object}}")])?;',
     'axon-fabric', '--test guest_provenance', 'a_branch_named_like_the_certified_abbreviation_does_not_answer_the_lineage'),
]

# C9 round 4 (harness): sccache is installed for DEVELOPMENT evidence runs
# only; a freeze refuses any rustc wrapper. build-guest-image.sh carries the
# same refusal as its first statement; it has no row because executing that
# script without it is a full kernel and rootfs build (refusal exercised by
# hand: exit 2; amendment 52).
MUTATIONS += [
    ('M650', 'EVIDENCE (decision E): a freeze is never made through a compiler wrapper', 'scripts/v022_freeze_manifest.py',
     '    if wrappers:\n',
     '    if False and wrappers:\n',
     'axon-fabric', '--test freeze_manifest', 'a_compiler_wrapper_does_not_freeze'),
]

# ── C9 round 4, CORE workstream (M651-M667; amendment 53; matrix A86) ──
# PSV-1: the candidate never chooses the code that runs under the operator's
# judging method. M651 is the method-dispatch seal edge (no confusion needed:
# the candidate DECLARES its own type). M652-M664/M666/M667 are the
# declared-type CAST at each value boundary (interp/conform.rs): each attack
# carries a confused `true` that selects the operator's OWN lenient impl, so
# no candidate method is involved and the dispatch edge cannot stand in for
# the cast. M665 is the static E0004 walk over type positions.
_CC = 'crates/axon-core/src/interp/conform.rs'
_CE = 'crates/axon-core/src/interp/eval.rs'
_T4 = 'interp::tests::'
MUTATIONS += [
    ('M651', "PSV-1 (A86): in operator code an operator-defined method name never dispatches to the candidate's method", _CI,
     '            && self.seal.operator_methods.contains(&f.name)',
     '            && false',
     'axon-core', _CL, _T4 + 'a_candidates_method_never_runs_under_the_operators_method_name'),
    ('M652', 'PSV-1 (A86): a value is cast to a declared integer type by kind', _CC,
     'return kind_ok(matches!(v, Value::Int(_) | Value::SizedInt { .. }))',
     'return kind_ok(true)',
     'axon-core', _CL, _T4 + 'a_confused_scalar_never_crosses_a_declared_return'),
    ('M653', 'PSV-1 (A86): a struct value is cast to a declared struct type by name', _CC,
     '            if name != n {',
     '            if false {',
     'axon-core', _CL, _T4 + 'a_confused_struct_never_crosses_as_another_struct'),
    ('M654', 'PSV-1 (A86): a declared array type casts every element', _CC,
     '                        self.cast_at(x, inner, cx, d)?;',
     '                        let _ = (x, inner);',
     'axon-core', _CL, _T4 + 'a_confused_element_never_crosses_inside_an_array'),
    ('M655', 'PSV-1 (A86): a declared Option type casts its payload', _CC,
     '            Value::Some(x) => self.cast_at(x, inner, cx, d),',
     '            Value::Some(_) => Ok(()),',
     'axon-core', _CL, _T4 + 'a_confused_payload_never_crosses_inside_an_option'),
    ('M656', 'PSV-1 (A86): a declared tuple type casts every element', _CC,
     '                        self.cast_at(x, t, cx, d)?;',
     '                        let _ = (x, t);',
     'axon-core', _CL, _T4 + 'a_confused_element_never_crosses_inside_a_tuple'),
    ('M657', "PSV-1 (A86): a declared struct type casts the struct's fields", _CC,
     '                    self.cast_at(fv, &tf.ty, &fcx, d)\n                        .map_err(|e| format!("field `{}` of `{n}`: {e}", tf.name))?;',
     '                    self.cast_at(fv, &any(), &fcx, d)\n                        .map_err(|e| format!("field `{}` of `{n}`: {e}", tf.name))?;',
     'axon-core', _CL, _T4 + 'a_confused_field_never_crosses_inside_a_struct'),
    ('M658', 'PSV-1 (A86): a struct literal casts each field to its declared type', _CE,
     'if let Err(why) = self.cast_field(name, fname, &mut fval) {',
     'if let Err(why) = Ok::<(), String>(()) {',
     'axon-core', _CL, _T4 + 'a_confused_field_is_refused_at_construction'),
    ('M659', "PSV-1 (A86): a fn's arguments are cast to its declared parameter types", _CI,
     'if let Err(why) = self.cast(&mut a, &p.ty, &cx) {',
     'if let Err(why) = Ok::<(), String>(()) {',
     'axon-core', _CL, _T4 + 'a_confused_argument_never_enters_a_declared_parameter'),
    ('M660', 'PSV-1 (A86): at a seal crossing a value at a type parameter no argument determined is refused', _CC,
     '            None if cx.strict => Err(format!(',
     '            None if false => Err(format!(',
     'axon-core', _CL, _T4 + 'a_value_at_an_undetermined_type_parameter_never_crosses_the_seal'),
    ('M661', "PSV-1 (A86): a closure's result is cast to every fn type it crossed", _CI,
     '        self.closure_ret_check(&contract, &mut v, crossing)?;',
     '        let _ = crossing;',
     'axon-core', _CL, _T4 + 'a_closures_confused_result_never_crosses_its_declared_type'),
    ('M662', "PSV-1 (A86): a closure's arguments are cast to every fn type it crossed", _CI,
     '        self.closure_args_check(&contract, &mut args)?;',
     '        let _ = &contract;',
     'axon-core', _CL, _T4 + 'a_closures_confused_argument_never_crosses_its_declared_type'),
    ('M663', 'PSV-1 (A86): a value sent on a channel is cast to every element type the channel crossed', _CE,
     '                            self.chan_send_check(q, &mut v)?;',
     '                            let _ = &q;',
     'axon-core', _CL, _T4 + 'a_confused_value_is_never_sent_on_a_declared_channel'),
    ('M664', 'PSV-1 (A86): a let annotation casts the bound value', _CE,
     'if let Err(why) = self.cast(&mut v, t, &Default::default()) {',
     'if let Err(why) = Ok::<(), String>(()) {',
     'axon-core', _CL, _T4 + 'a_let_annotation_is_cast'),
    ('M665', "PSV-1 (A86): E0004 walks type positions (signatures, fields, impl headers, bounds, annotations)", 'crates/axon-core/src/resolver.rs',
     '            for n in names\n                .into_iter()\n                .chain(annotated.iter().map(String::as_str))\n            {',
     '            for n in Vec::<&str>::new() {',
     'axon-core', _CL, 'resolver::tests::a_sealed_module_cannot_name_the_operators_types_or_traits'),
    ('M666', "PSV-1 (A86): a type parameter's trait bounds are cast by an impl of the trait", _CC,
     '                            self.check_impl(v, tr)?;',
     '                            let _ = tr;',
     'axon-core', _CL, _T4 + 'a_type_parameters_trait_bound_is_cast'),
    ('M667', "PSV-1 (A86): a lambda's own parameter annotations are its first contract", _CC,
     '        if params.iter().all(|p| p.ty.is_none()) {',
     '        if true {',
     'axon-core', _CL, _T4 + 'a_lambdas_annotated_parameter_is_cast'),
]

# Protected Check Isolation guards (governance/specs/v022-protected-check-isolation.md):
# candidate code must not alter what the operator's check runs or what PASS
# means. Kept here so nothing is lost, but certified under PCI, not G01
# (user decision 2026-09-26). M58 is equivalent since M59 (whole-call
# containment) and is excluded from every scope's kill list. M66 (a test's
# `Flow::Return(Err)` arm) was removed before any certified run: `call_fn`
# ends a `return` at the callee, so the arm was unreachable and the mutant
# equivalent; the arm now fails closed and M65 guards the one live path.
PCI_IDS = {"M04", "M44", "M49", "M52", "M53", "M57", "M59", "M60", "M61", "M62",
           "M63", "M64", "M65", "M67", "M68", "M69", "M70", "M71",
           "M55", "M72", "M73", "M74", "M75", "M76", "M77", "M78",
           "M79", "M80", "M81", "M82", "M83", "M84", "M85",
           "M86", "M87", "M88", "M89", "M90", "M91", "M92", "M93", "M94", "M95", "M96", "M97"}
# ── Equivalent-by-defence-in-depth (dev review rounds 1-3, recorded for the
# candidate-7 frozen run). Each mutates a guard that a rounds-1-3 fix turned
# into a SECOND, independent check of the same property: with the guard
# mutated the attack is STILL refused (all verified: the mutant test passes),
# and the property is killed by a LIVE sibling row. So each is an equivalent
# mutant, excluded from the kill requirement — the code keeps both checks.
#   M204: submit's observe seam. NOT equivalent and NOT stale: re-anchored
#         ACTIVE in C9 round 1 (its guard lived on at the current seam; a
#         swallowed observe Err launched a defective observation as Passed).
#   M103: derive check_pins — killed by intake pins M26/M28/M47.
#   M104: derive context_signed_by — killed by M99 and round-3 M269.
#   M209: derive rooted verifier — killed by O2 M205/M206. C9 re-audit
#         reinstated it; C9 round 1b retires it again vs M360 (EQUIV_RECORD).
#   M210: derive rooted observer — killed by O2 M207/M208. C9 re-audit
#         reinstated it; C9 round 1b retires it again vs M361 (EQUIV_RECORD).
#   M245: derive monitor key_id scan — killed by M105 and round-2 M264.
#   M254: NOT equivalent — REINSTATED ACTIVE (C8 certifying review
#         wf_bff9835f-4a0). It was listed here as "killed by M261 + M267-M269",
#         but removing the reverify_protected call ALONE breaks three
#         protected-class tests: that call is the sole enforcement site for the
#         properties M258/M259/M267/M268/M269 test, so it is load-bearing. It
#         passed the old matrix only because its assigned test (a relabel
#         attack) is independently refused by M261/M262.
#   M255: reverify claims_protected — killed by M211/M212/M213.
# EQUIVALENT_DID: rows a rounds-1-3 fix turned into an additional independent
# check of a property. Each APPLIES to a real historical guard; with it
# mutated the attack is still refused (the property is enforced elsewhere), and
# scripts/v022_paired_disable.py demonstrates that removing the retired guard
# TOGETHER WITH its subsuming siblings reopens the SAME attack (for M103/M209 a
# single dominating sibling alone reopens). The retired guard removed ALONE
# must also leave the WHOLE package suite green (not only its assigned --exact
# test) — the full-suite condition that exposed M254. An equivalent mutant does
# NOT count as killed. `killer` is a live ACTIVE row that turns the property's
# discriminator red.
EQUIV_RECORD = {
    "M58": {"property": "a break/continue raised in a FUNCTION BODY does not escape into the caller's loop", "subsumed_by": ["M59"], "killer": "M59"},
    "M245": {"property": "a protected clearance is a real monitor signature under the operator monitor root", "subsumed_by": ["M264"], "killer": "M264"},
    # ── C9 round 1 (harness workstream): rows whose recorded kill was ANOTHER
    # check refusing the attack. Each has an all-paths argument (`all_paths`)
    # and a four-cell record in scripts/v022_paired_disable.py. `killer` names
    # the live ACTIVE row(s) for the property, or "joint:" when no single row
    # can reopen it (mutually dominating guards): the four-cell's joint cell is
    # then the executed mutation that removes the property.
    "M27": {"property": "a verdict runs an operator suite (check:<id>), never a candidate file",
            "subsumed_by": ["M30"], "killer": "joint:M27+M30",
            "all_paths": "check_pins is one function with two callers (intake, admission re-derivation) and no early Ok; its acceptance join (M30's clause) requires argv == [check:{acc_suite}, test] exactly, so argv[0] always has the check: prefix M27 tests"},
    "M29": {"property": "the recorded suite is the one the request named",
            "subsumed_by": ["M30"], "killer": "joint:M29+M30",
            "all_paths": "same function: M30's clause requires argv[0] == check:{acc_suite} and recorded == acc.check_suite, and acc_suite is acc.check_suite's own id, so the recorded suite is argv's suite whenever M30 passes"},
    "M214": {"property": "a protected claim names its preflight observation",
             "subsumed_by": ["M233"], "killer": "joint:M214+M233",
             "all_paths": "check() runs only from check_bundle (intake, admission re-verify) and from EVL, where a claim that passes check() goes on to verify_check_evidence -> check_bundle; check_bundle want()s preflight-observation-sha256 (exactly once) unconditionally"},
    "M216": {"property": "each protected join is named exactly once",
             "subsumed_by": ["M299"], "killer": "joint:M216+M299",
             "all_paths": "as M214: every one of the 8 REQUIRED_DIGEST_REFS is want()ed in check_bundle, and one_ref() refuses a prefix named more than once"},
    "M377": {"property": "the launch manifest is for the request's and receipt's candidate",
             "subsumed_by": ["M378"], "killer": "M379",
             "all_paths": "check_bundle is reached only through verify_check_evidence, which joins the receipt's input tree to the request's (M33); with that join, manifest==receipt implies manifest==request"},
    "M378": {"property": "the launch manifest is for the request's and receipt's candidate",
             "subsumed_by": ["M377"], "killer": "M379",
             "all_paths": "as M377, symmetrically: manifest==request and request==receipt (M33) imply manifest==receipt"},
    "M285": {"property": "git never reports a replacement object's content under a certified name",
             "subsumed_by": ["M385", "M289"], "killer": "joint:M285+M385+M289",
             "all_paths": "every git call goes through git_cmd, which clears the environment (no GIT_REPLACE_REF_BASE) and disables replacement objects (M385); every object read is re-hashed to its name (M289)"},
    "M385": {"property": "git never reports a replacement object's content under a certified name",
             "subsumed_by": ["M285", "M289"], "killer": "joint:M285+M385+M289",
             "all_paths": "replacements exist only as refs under refs/replace/ (the environment is cleared, so the ref base is the default); refuse_git_spoofing refuses any such ref (M285) before a verdict; objects are re-hashed (M289)"},
    "M287": {"property": "an uncommitted change hidden by skip-worktree is not certified",
             "subsumed_by": ["M290"], "killer": "joint:M287+M290",
             "all_paths": "the flag only hides worktree-vs-index; worktree_differs hashes every certified path's bytes directly (never the index), a missing file differs, and a path outside the certified tree already differs in the tree diff"},
    "M288": {"property": "an uncommitted change hidden by assume-unchanged is not certified",
             "subsumed_by": ["M290"], "killer": "joint:M288+M290",
             "all_paths": "as M287 (assume-unchanged, the other index flag)"},
    # ── C9 round 1b, workstream LOOP.
    "M19": {"property": "a protected evaluation counts no execution leg that ran on a development backend",
            "subsumed_by": ["M428"], "killer": "M428",
            "all_paths": "M19's filter runs only in judge()'s protected branch, on d.acf's receipt; that branch then calls verify_execution on the SAME (request, receipt) unconditionally (the only exit between them is the verification-leg filter, itself a refusal), and verify_execution refuses a receipt whose backend is not a PROTECTED_PROFILES entry (M428). Admission never runs M19; there M428 is sole and killed on its own route"},
    "M209": {"property": "a counted protected verdict's recorded verifier is one the operator verifier root holds, under the recorded key",
             "subsumed_by": ["M360"], "killer": "joint:M209+M360",
             "all_paths": "derive's rooted-verifier check applies only to a PROTECTED evaluation's counted trials, reached from admit and every rederive (activation, rollback); for each such trial the same loop iteration then runs reverify_protected, which authenticates the verdict under the EPISODE's issuer via verify_check_evidence (a protected claim is required, so rooted_key(issuer) must hold) and requires the record's issuer and key_id to BE that signer (M360). So the record's issuer is rooted with that key whenever M360 passes. The development class never uses the rooted lookup"},
    "M210": {"property": "a counted protected trial's context attribution is an observer the operator observer root holds, under the recorded key",
             "subsumed_by": ["M361"], "killer": "joint:M210+M361",
             "all_paths": "derive's rooted-observer check runs only for a PROTECTED evaluation's counted trials, after the verifier loop, where reverify_protected has returned the context's signer for EVERY such trial (trusted observer, rooted_key held, signature verified under it); M361 then requires context_signed_by to equal that signer. So the attribution is rooted with that key whenever M361 passes. No other reader of context_signed_by exists"},
    # ── C9 round 1b (fabric workstream) ──
    "M139": {"property": "on a protected host the caller's --check-registry never becomes the suite registry",
             "subsumed_by": ["M141"], "killer": "joint:M139+M141",
             "all_paths": "the binary reads --check-registry in exactly two places, both in submit(): M139's refusal and the registry_path match; on a protected host (host is Some) that match's arm is h.suite_registry (M141), which never reads the flag, so with M139 removed the flag is read by nothing"},
    "M141": {"property": "on a protected host the caller's --check-registry never becomes the suite registry",
             "subsumed_by": ["M139"], "killer": "joint:M139+M141",
             "all_paths": "M139's refusal runs before the registry_path match on the same host (Some) and tests a.opt(--check-registry), the same lookup the override would use; nothing between them can succeed without passing it, so the override is reached only with the flag absent"},
    "M183": {"property": "a pass rests on this launch's keyed completion evidence",
             "subsumed_by": ["M312"], "killer": "joint:M183+M312",
             "all_paths": "both checks sit in derive()'s Passed arm; M312 requires exactly one result line naming the test, status ok, whose completion equals outcome_token(K, test, true), which is the same HMAC as completion_token(K, test); parse_axon_test_json records that line's token in report.completion, so M312 passing implies M183's predicate"},
    "M187": {"property": "the protected profile launches only an operator suite with a named test",
             "subsumed_by": ["M400"], "killer": "joint:M187+M400",
             "all_paths": "the protected arm of the dispatch match is the only launch path for the protected profile, and every request reaching it passes M400's spec match, which yields a launch spec only for (Some(suite), Some(test)); M187 is the same predicate before reservation"},
    "M400": {"property": "the protected profile launches only an operator suite with a named test",
             "subsumed_by": ["M187"], "killer": "joint:M187+M400",
             "all_paths": "M187 runs for every request whose selected profile is the protected one, before reservation, and returns Err unless target.suite and target.filter are both Some; the protected arm is reached only with that profile"},
    "M273": {"property": "on a protected host a caller grant registry never authorizes status or cancel",
             "subsumed_by": ["M401"], "killer": "joint:M273+M401",
             "all_paths": "grant_registry() is the one reader of --grant-registry for status/cancel (and submit, where M274 refuses first); on a protected host its arm resolves h.grants(), the operator's pinned registry (M401), which never reads the flag"},
    "M401": {"property": "on a protected host a caller grant registry never authorizes status or cancel",
             "subsumed_by": ["M273"], "killer": "joint:M273+M401",
             "all_paths": "within the same arm, M273's by-name refusal of --grant-registry runs first and tests the flag's presence (has), which any value the override could read implies"},
    # ── C9 round 1b (core workstream). Interpreter/resolver rows whose kill was
    # another check's refusal; the attack is each row's own test, which
    # accepts any refusal and panics "ATTACK: ..." when the attack gets through.
    "M60": {"property": "a second impl never replaces the first impl's method",
            "subsumed_by": ["M69"], "killer": "joint:M60+M69",
            "all_paths": "the ONLY harm of a duplicate impl is the interpreter's method table (keyed (type_name_of(for_type), method), last insert wins) replacing a method; with M60's check off every impl takes the else branch, whose dispatch check keys on the SAME type_name_of and method name and refuses any second definition of that key (E0002), whatever the traits. A duplicate impl sharing no method key replaces nothing (a missing required method is E0502; impl_table is a set)"},
    "M89": {"property": "a sealed frame never gets an operator function run through a fiber",
            "subsumed_by": ["M86", "M96"], "killer": "joint:M89+M86+M96",
            "all_paths": "every fiber runs in builtin_scheduler_run_once (scheduler_run, supervisor_run) through call_fn. A fiber lives in the kernel of the frame that queued it (k() = kernels[frame_sealed], M96), so a sealed frame's fiber is run only from a sealed frame, where call_fn's call edge (M86) refuses an operator function. Executed: M89+M86 reopens the sealed-frame route, M89+M96 reopens the operator-frame route"},
}
# ── C9 round 1b (psv workstream) ──
# ── C9 round 1b, integration: M04 was retired here against the first-match
# rule M436 ("a candidate module never shadows a suite module's name"). C9
# round 2 (harness) REINSTATES it ACTIVE: the order is the only guard of the
# RIGHT verdict for an honest candidate holding a module named like a suite
# module (M436 turns the wrong verdict into none, it does not restore the
# right one), and its full-suite cell now fails on that test
# (an_honest_candidate_holding_a_suite_module_name_is_judged_by_the_suite).
# The guest twin, M177, was scored KILLED on the same shape. (M260 was
# proposed too and is NOT retired: without it a sealed module's use pulls in a
# suite module the entry never imports, a keyed PASS; its full-suite four-cell
# cell caught that.)
EQUIV_RECORD["M346"] = {
    "property": "build provenance never reads a tree with an edited tracked file as clean",
    "subsumed_by": ["M451"], "killer": "joint:M346+M451",
    "all_paths": "provenance() runs head_bytes_differ (M451) on every call, before the ls-files "
                 "tag check: it hashes every blob of HEAD's tree against the working-tree bytes, so "
                 "an edited, deleted or replaced tracked file is dirty whatever the index says. A "
                 "skip-worktree bit hides only worktree-vs-index differences from git status; an "
                 "index that differs from HEAD still shows in status (staged), and a worktree equal "
                 "to HEAD is the committed source. So M346's refusal never stands alone"}
# C9 round 2 (decision C): the filesystem walk reports every object outside
# the tree, however git treats it (M500 files, M501 directories). Each row's
# attack is an input inside an untracked directory, so M501 is the sibling.
EQUIV_RECORD["M347"] = {
    "property": "build provenance never reads an untracked input as clean",
    "subsumed_by": ["M501"], "killer": "joint:M347+M501",
    "all_paths": "provenance() always runs head_bytes_differ -> tree_differs, whose untracked_objects "
                 "walk (M500 files, M501 whole untracked directories) reports every filesystem object not in HEAD's tree and not on the "
                 "operator allowlist, without consulting git. `status --untracked-files=all` (M347) "
                 "only ever ADDS reasons; an untracked non-ignored file is exactly one the walk "
                 "reports. So M347's refusal never stands alone"}
EQUIV_RECORD["M414"] = {
    "property": "build provenance never reads a file hidden by a non-tracked ignore rule as clean",
    "subsumed_by": ["M501"], "killer": "joint:M414+M501",
    "all_paths": "a file ignored by info/exclude, an untracked .gitignore or a config excludesFile is "
                 "an object not in HEAD's tree; the walk (M501 for its untracked directory, M500 for a file) reports it unless the operator "
                 "allowlist names it, and git-ignore plays no part in the walk (decision C). The "
                 "check-ignore source test (M414) only ADDS a reason. So it never stands alone"}
EQUIV_RECORD["M152"] = {
    "property": "a signature made for one authority never verifies for another",
    "subsumed_by": ["M153"], "killer": "joint:M152+M153",
    "all_paths": "verify_evidence_signature is the ONLY reader of the signature's domain field (Fabric "
                 "backend, readiness, loop protected_evidence all call it); after the field check it always "
                 "verifies over evidence_signing_message(authority, bytes) for the CALLER's authority, not the "
                 "field's, so a signature made for another authority verifies only if the message is not "
                 "domain-bound (M153). The relabelled case is M153's own (active) attack"}
EQUIV_RECORD["M418"] = {
    "property": "the observation a certification cites was made by a key in the operator observer root",
    "subsumed_by": ["M339", "M340"], "killer": "joint:M418+M339+M340",
    "all_paths": "attribution() has one caller (the component verdict) and no early Ok; after the membership "
                 "loop it always verifies the observation's signature under keys(observer_dir) (M340, which "
                 "returns the signer's fingerprint) and requires signer == observer_key_id (M339). key_ids and "
                 "keys read the same operator root, so signer in root and signer == observer_key_id put "
                 "observer_key_id in the root"}
# ── C9 round 2, HARNESS workstream: guards subsumed on every path. Each
# test accepts ANY refusal and fails on the joint attack only.
EQUIV_RECORD["M482"] = {
    "property": "readiness decides on ONE set of bytes per file: a FIFO serving each read "
                "differently certifies nothing (the retired check's only unique effect is naming "
                "the FIFO in the reason)",
    "subsumed_by": ["M481", "M335"], "killer": "joint:M482+M481+M335",
    "all_paths": "read_regular is the one reader of every file readiness, the evidence/signature "
                 "readers, the observer and the B263 qualification decide on, and each caller "
                 "decides on the single buffer it returns (M335/M336/M337 for readiness's record, "
                 "preflight and manifest). A FIFO serves one set of bytes per open, so a single "
                 "read of a FIFO is no more than a regular file an agent could have written; the "
                 "two-read substitution needs a second read (M335 off). The non-blocking open (M481) "
                 "is in the set only so the test's feeder can connect: with it on, a FIFO whose "
                 "writer has not connected reads EOF. Executed: M482 alone, M482+M481, and M481+M335 "
                 "each refuse; M482+M481+M335 certifies the agent's commit"}
EQUIV_RECORD["M487"] = {
    "property": "a service leaf (out_root, nonce_store) that is a symlink never loads",
    "subsumed_by": ["M486", "M327"], "killer": "joint:M487+M486+M327",
    "all_paths": "service_leaf is the only check of the two leaves and reads symlink_metadata, "
                 "which never follows: for a symlink, is_dir() is false (M486 refuses it on every "
                 "unix) and st_mode is 0777 on Linux (M327 refuses it). Only with both removed "
                 "does a link owned by the service euid load"}
# C9 round 2 (rows): M217 was REFUSED_ELSEWHERE in the de523f4c run -- the
# round-2 strict manifest digest rule (A69, M473) refused its attack first.
# Four cells executed (this workstream, and scripts/v022_paired_disable.py):
# M217 off -> refused (names_every_digest); M473 off -> refused (check's
# sha256 rule); both off -> the malformed kernel digest is ACCEPTED.
EQUIV_RECORD["M217"] = {
    "property": "each protected receipt join (the eight REQUIRED_DIGEST_REFS) is a sha256",
    "subsumed_by": ["M473"], "killer": "joint:M217+M473",
    "all_paths": "protected_evidence::check has two callers, check_bundle and EVL's pre-check "
                 "(evl.rs), and EVL's pre-check passing implies claims_protected, so "
                 "verify_check_evidence then runs check_bundle as well; admission's "
                 "reverify_protected (the store-writer route) reaches the same check_bundle "
                 "through verify_check_evidence. The development class never calls check. In "
                 "check_bundle every one of the eight refs must EQUAL a value that is a lowercase "
                 "sha256 on every path: launch-manifest, preflight-observation and guest-verdict "
                 "equal sha256_hex of the bundle's bytes (M233/M299 and the manifest join), and "
                 "guest-kernel/rootfs/axon/init and qualification equal the manifest's *sha256 "
                 "fields, which names_every_digest (M473) requires to be 64 lowercase hex before "
                 "any join runs. So a ref that is not a sha256 is refused whenever M217's rule "
                 "would have refused it, and M473 stays ACTIVE and killed"}
# C9 round 3 (harness): two mutual PAIRS in the decision-A/D path. Each guard
# alone is dominated by the other on every path; removing both reopens the
# attack. Four cells executed with scripts/v022_paired_disable.py.
EQUIV_RECORD["M594"] = {
    "property": "an interpreter that is itself a #! script never runs through its own #! line",
    "subsumed_by": ["M595"], "killer": "joint:M594+M595",
    "all_paths": "sealed_exec::command has four callers (privileged_launcher::run twice, "
                 "backend::run_direct twice, observer::observe, backend::run_privileged with no "
                 "interpreter), and none puts the interpreter's descriptor in `inherit`; only "
                 "the PROGRAM's is inherited. open_verified opens every object close-on-exec, so "
                 "execveat(interpreter_fd, \"\", AT_EMPTY_PATH) of a #! script fails with ENOENT "
                 "(the kernel cannot hand /dev/fd/N of a close-on-exec descriptor to the #! "
                 "interpreter; measured). M594 removed alone: the exec fails, nothing runs. "
                 "M595 removed alone: M594 refuses in command()"}
EQUIV_RECORD["M595"] = {
    "property": "an interpreter that is itself a #! script never runs through its own #! line",
    "subsumed_by": ["M594"], "killer": "joint:M594+M595",
    "all_paths": "the only descriptor a script is executed FROM is the interpreter's (a script "
                 "PROGRAM with no interpreter is refused by M529; with one, the program is read "
                 "as /dev/fd/N, never executed); an interpreter that is a script is refused by "
                 "M594 before any fork. So a verified descriptor that is not close-on-exec is "
                 "never executed as a script while M594 stands"}
EQUIV_RECORD["M596"] = {
    "property": "no other uid can read the per-attempt secret in the helper's staging snapshot",
    "subsumed_by": ["M597"], "killer": "joint:M596+M597",
    "all_paths": "new_staging is the only creator of a staging dir and every snapshot file is "
                 "written below the per-launch dir it creates, with mode 0700 (M597) under "
                 "harden()'s umask 022; the staging root is operator-owned and not group/other-"
                 "writable (M598, M589), so no other uid can create, rename or replace entries "
                 "in it. With M596 removed alone (a 0755 staging root) another uid can list "
                 "the root but not enter the 0700 per-launch dir. With M597 removed alone the "
                 "0700 root (M596) stops traversal"}
EQUIV_RECORD["M597"] = {
    "property": "no other uid can read the per-attempt secret in the helper's staging snapshot",
    "subsumed_by": ["M596"], "killer": "joint:M596+M597",
    "all_paths": "the per-launch dir sits directly in the staging root, which must be 0700 "
                 "(M596) and operator-owned (M598): no other uid can traverse into it whatever "
                 "the per-launch dir's own mode"}
# C9 round 3, ROWS workstream: rows the 1084ed1c run scored REFUSED_ELSEWHERE
# or survived, where no route leaves the guard alone. Each four-cell record
# is executed with scripts/v022_paired_disable.py; the other rows of that
# run were re-attacked on the route where their guard IS alone (M72 through
# an operator helper, M194/M201/M204/M310 on the direct route, M284 on the
# development lineage).
EQUIV_RECORD["M186"] = {
    "property": "a guest verdict made without a verified observation is never classed protected",
    "subsumed_by": ["M606", "M620"], "killer": "joint:M186+M606+M620",
    "all_paths": "derive has one caller (submit's protected-profile arm) and receives no observation "
                 "exactly when the config has no observer (observe returns a verified observation or "
                 "Err, and Err launches nothing: M204). Its class reaches the receipt only through "
                 "psv_receipt, which re-labels guest-unobserved every launch that is not admissible "
                 "on a route that attests protected: the DIRECT route never attests (M606, "
                 "attests_protected), and on the PRIVILEGED route an admissible launch needs the "
                 "helper to launch, which it never does without a verified observation (M620; its "
                 "refusal is not launched_ok, which M323's clause downgrades). HostVerdict.class "
                 "itself has no reader. So a verdict with no observation is guest-unobserved on "
                 "every route whatever derive's None arm says. Executed on the direct route"}
EQUIV_RECORD["M286"] = {
    "property": "readiness never certifies a tree whose ancestry an info/grafts file rewrites",
    "subsumed_by": ["M581"], "killer": "joint:M286+M581",
    "all_paths": "refuse_git_spoofing has one caller (the protected_backend component), and the only "
                 "parent-dependent answer readiness then takes is git_data::descends on axon_sha, "
                 "a full commit id (checked 40-hex; object_named takes it as a hash, M642), which "
                 "reads every commit's parents from the hash-checked object bytes (M581) and never "
                 "from git's commit walk, the only reader of grafts. `git diff certified HEAD` and "
                 "the tree walk compare trees, which grafts do not touch. So a grafts file changes "
                 "no answer readiness decides on"}
EQUIV_RECORD["M453"] = {
    "property": "a gitfile .git (a repository chosen elsewhere) never answers a protected provenance question",
    "subsumed_by": ["M580"], "killer": "joint:M453+M580",
    "all_paths": "discover is the protected entry for every caller (readiness, build provenance's "
                 "toplevel, descends_from_protected) and, after the kind check, always runs "
                 "own_repository: git's own --git-common-dir (env cleared) must canonicalize to "
                 "top/.git. For a gitfile, top/.git is a FILE while git's common dir is a "
                 "directory, so the two never match (and a gitfile git cannot follow is Err). A "
                 "symlinked .git stays refused by the kind check with M453 applied. So the kind "
                 "check's refusal of a gitfile never stands alone"}
EQUIV_RECORD["M459"] = {
    "property": "the guest build's PCI lineage never passes through grafted ancestry",
    "subsumed_by": ["M581"], "killer": "joint:M459+M581",
    "all_paths": "lineage() is the one body of descends_from (--descends) and descends_from_protected "
                 "(--lineage, the manifest's); after the grafts refusal it always calls "
                 "git_data::descends, which names the revision by hash only (object_named, M642: no "
                 "ref, no parent navigation) and walks parents parsed from hash-checked commit "
                 "bytes (M581), never git's commit walk, the only reader of grafts. So a grafts "
                 "file cannot change the answer"}
EQUIV_RECORD["M602"] = {
    "property": "a production helper that is not root in every id never launches",
    "subsumed_by": ["M628"], "killer": "joint:M602+M628",
    "all_paths": "every launch goes serve_as -> prepare -> observed_launch, which verifies the "
                 "observation (M620) and then spends its nonce through the operator's custodian "
                 "(M621) before make_out and run; run is reached only from a Prepared. The spend "
                 "is authorized by the custodian's SO_PEERCRED rule: only launcher_uid spends "
                 "(M628). A helper whose euid is not 0 connects with that euid (become_root is a "
                 "no-op without euid 0). A PROTECTED custodian's config must set launcher_uid to 0 "
                 "(CustodianConfig::check(true), M640); a test custodian (launcher_uid unchecked) "
                 "authorizes no production launch (M625), a dev one none at all (M624); the "
                 "helper accepts only a socket its configured custodian uid or root serves (M626). "
                 "So a helper that is not root never spends, and never launches. Executed with the "
                 "production helper and the production (socket-activated) custodian"}
EQUIVALENT_DID = set(EQUIV_RECORD)
# STALE: a row whose old text no longer exists. "The old text is absent" shows
# only that the TEXT changed, not that the guard is gone (C9 dev review: M204
# was recorded stale while its guard lived on, refactored, at submit.rs's
# observe seam, with no row). So a stale row is accepted ONLY with a named
# `replacement`: an ACTIVE row that mutates the guard in its current form and
# is itself KILLED by its own attack. Both harnesses enforce this: the mutation
# run checks the replacement's result, and paired-disable executes the
# replacement's kill. A stale row with no live replacement fails the run.
#
# M204 is ACTIVE again (C9 round 1): re-anchored on the current seam, where
# observe's Err is propagated so nothing launches.
STALE_REFACTORED = {
    # C9 re-audit: M176 was listed as a LEGACY EQUIVALENT but its guard no longer
    # exists (old string absent) -- it became the `Some(0)` of the Passed arm in
    # runner.rs, which no row mutated. Re-audited in C9 round 1 on the new terms:
    # the guard lives on exactly as that arm, and M293 (ACTIVE) mutates it.
    "M176": {"property": "a pass needs exit 0", "replacement": "M293",
             "how": "the exit-0 guard became the Some(0) of the Passed arm in runner.rs (C9 re-audit)"},
}

# M176 (the runner's exit-0 guard) is equivalent since `--exact` (PSV review
# wf_d725935a-7ed, B1): the runner now executes exactly the one registered
# test, so a Passed report for it already implies the interpreter exited 0.
# Its killing test (a failing SIBLING beside a passing named test) described
# the very behaviour B1 removed. The guard stays as defence in depth; M184
# (Fabric's own exit-0 check) remains killed.
# C9 re-audit: the legacy set is EMPTY. M176 is stale (above). M58 had no recorded
# property, subsuming guard or killer, so under the four-cell rule it is ACTIVE.
LEGACY_EQUIV = set()
RETIRED = LEGACY_EQUIV | EQUIVALENT_DID | set(STALE_REFACTORED)
BINDING_IDS = {f"M{n}" for n in range(101, 137)}
# Every id range the PSV rounds allocate (C9 round 1 uses up to M399; round
# 1b allocates M400-M499, round 2 M500-M519). An id outside every scope would silently fall into
# g01.
PSV_IDS = {f"M{n}" for n in range(137, 550)}
# C9 round 3: rows M560-M649 are PSV rows (workstream ranges).
PSV_IDS |= {f"M{n}" for n in range(550, 650)}
# C9 round 4: M650-M699.
PSV_IDS |= {f"M{n}" for n in range(650, 700)}


def in_scope(mid, scope):
    if mid in RETIRED:
        return False
    if scope == "g01":
        return mid not in PCI_IDS and mid not in BINDING_IDS and mid not in PSV_IDS
    if scope == "pci":
        return mid in PCI_IDS
    if scope == "binding":
        return mid in BINDING_IDS
    if scope == "psv":
        return mid in PSV_IDS
    return True


def sh(cmd):
    return subprocess.run(cmd, cwd=ROOT, shell=True, capture_output=True, text=True)


def cargo_test(package, target, test):
    cmd = (
        "source scripts/lib_bounded_run.sh && "
        f"bounded_run {MEM} 1800 cargo test -q -p {package} {target} -- --exact {test}"
    )
    r = subprocess.run(["bash", "-c", cmd], cwd=ROOT, capture_output=True, text=True)
    out = r.stdout + r.stderr
    if "could not compile" in out or "error[E" in out:
        return "compile_error", out
    if r.returncode == 0 and "1 passed" in out:
        return "passed", out
    if r.returncode != 0 and "test result: FAILED" in out and f"{test} --- FAILED" in out:
        return "failed", out
    if r.returncode == 0:
        return "not_run", out  # 0 tests matched: the name drifted
    return "error", out


def _is_test_thread(line, test):
    # The panic names the test by its full path (`mod::tests::name`) for a
    # lib test and by its bare name for an integration test.
    return "panicked at" in line and (f"'{test}'" in line or f"'{test.split('::')[-1]}'" in line)


# A panic block ends where libtest or the next panic starts.
_BLOCK_END = ("thread '", "note: ", "stack backtrace:", "failures:", "---- ", "test result:")


def failing_panic(out, test):
    """The panic that FAILED the test, in full: the LAST `panicked at` block on
    the test's own thread, through its whole message (for assert_eq that
    includes the `left:`/`right:` lines).

    Not the first: a test may catch a deliberate panic (a pre-launch hook, a
    catch_unwind probe) before the assertion that fails it. C9 dev review:
    kill_line() recorded M279/M280's caught `stop before launch` setup panic
    instead of the real failures later in the same test. The panic that ends
    the thread is the last one, because nothing on the thread runs after it."""
    lines = out.splitlines()
    start = None
    for i, l in enumerate(lines):
        if _is_test_thread(l, test):
            start = i
    if start is None:
        return None
    block = [lines[start]]
    for l in lines[start + 1:]:
        if l.startswith(_BLOCK_END):
            break
        block.append(l)
    while block and not block[-1].strip():
        block.pop()
    return "\n".join(block)


def kill_line(out, test):
    """The failing panic's location and message, bounded for the record."""
    b = failing_panic(out, test)
    return None if b is None else " | ".join(x.strip() for x in b.splitlines())[:600]


def attack_succeeded(mid, out, test):
    """True iff the panic that failed the test is this row's OWN attack
    succeeding: its ATTACK_MARKERS regex matches the failing panic block.

    A failure whose panic is another check's refusal, a reason-string
    mismatch or a setup panic is REFUSED_ELSEWHERE: the guard was removed and
    the attack was STILL refused, which is the equivalent shape, never a kill
    (C9 dev review, EQUIVALENCE: M140, M201, M262, M263, M285 ...)."""
    b = failing_panic(out, test)
    m = ATTACK_MARKERS.get(mid)
    return b is not None and m is not None and re.search(m, b, re.S) is not None


def sha(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


# The memory cap for each cargo step. Two shards run side by side on a 23 GB
# host, so each takes a smaller cap (V022_MUT_MEM, e.g. 10G).
MEM = os.environ.get("V022_MUT_MEM", "16G")


def in_shard(i, shard):
    """Row `i` (its position among the scope's rows) belongs to shard (k, n)."""
    return shard is None or i % shard[1] == shard[0]


def merge(out, parts):
    """Combine shard runs of ONE commit and scope into one run. Refuses shards
    that disagree on commit, scope or shard count, that overlap, or that leave
    any row of the scope uncovered: a merged pass is evidence only if it is
    exactly the unsharded pass, split."""
    docs = [json.load(open(p)) for p in parts]
    base = docs[0]
    for d in docs:
        for k in ("commit", "scope"):
            if d[k] != base[k]:
                sys.exit(f"refused: shards disagree on {k}: {d[k]} vs {base[k]}")
        if d.get("only") is not None:
            sys.exit(f"refused: {d['only']} is a sample (--only), not a shard")
        if (d.get("shard") or {}).get("of") != len(docs):
            sys.exit(f"refused: a shard of {(d.get('shard') or {}).get('of')} merged as one of {len(docs)}")
    got = sorted(d["shard"]["index"] for d in docs)
    if got != list(range(len(docs))):
        sys.exit(f"refused: shard indices {got}, not 0..{len(docs) - 1}")
    rows = [r for d in docs for r in d["mutations"]]
    ids = [r["id"] for r in rows]
    want = [m[0] for m in MUTATIONS if in_scope(m[0], base["scope"])]
    if sorted(ids) != sorted(want):
        missing = sorted(set(want) - set(ids))
        extra = sorted(i for i in set(ids) if ids.count(i) > 1 or i not in want)
        sys.exit(f"refused: shards cover the scope wrongly; missing {missing}, duplicate/extra {extra}")
    order = {m: n for n, m in enumerate(want)}
    rows.sort(key=lambda r: order[r["id"]])
    ok = all(d["all_killed"] for d in docs)
    doc = {"schema": "axon-v022-mutation-run/3", "gate": base["gate"], "scope": base["scope"],
           "commit": base["commit"], "toolchain": [d["toolchain"] for d in docs],
           "merged_from": [{"shard": d["shard"], "all_killed": d["all_killed"]} for d in docs],
           "all_killed": ok, "mutations": rows}
    with open(out, "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    ok &= print_evidence_model(rows, base["scope"],
                               extra=f"  (merged from {len(docs)} shards)"
                               + ("" if ok else "; a SHARD reported a failure — see its BAD lines"))
    sys.exit(0 if ok else 1)


def print_evidence_model(rows, scope, extra="", partial=False):
    """Operator model (2026-09-28, amended C9 round 1 / amendment 39): each
    class reported SEPARATELY, and only KILLED is a kill.

      KILLED             the test failed on the row's OWN attack (its marker)
      REFUSED_ELSEWHERE  the test failed, but on another check's refusal or a
                         reason mismatch: the attack was still refused (weak;
                         the equivalent shape, never counted killed)
      EQUIVALENT         retired under the four-cell rule, never counted killed
      STALE              old text gone AND a named ACTIVE replacement killed
      survivors          the test passed with the guard removed

    Returns False when any class other than KILLED/EQUIVALENT/STALE is
    non-empty, or a retirement record does not hold against the tree."""
    active = len(rows)
    killed = [r["id"] for r in rows if r["result"] == "killed"]
    weak = [r["id"] for r in rows if r["result"] == "refused_elsewhere"]
    survivors = [r["id"] for r in rows if r["result"].startswith("survived")]
    stale = [r["id"] for r in rows if "not_applicable" in r["result"]]
    base_ok = all(r["baseline"] == "passed" for r in rows)
    print(f"Mutation registry: {len(MUTATIONS)} total")
    print(f"Active mutants: {len(killed)}/{active} KILLED by their own attack"
          f"{'' if base_ok else ' (baselines NOT all pass)'}{extra}")
    print(f"REFUSED_ELSEWHERE (weak: failed, but the attack was still refused; NOT killed): "
          f"{len(weak)}{(' '+str(weak)) if weak else ''}")
    # Retired rows are reported by WHAT they are, and each class is checked
    # against the tree: an equivalent's guard must still exist (else it is not
    # an equivalent but a stale row). A stale row needs a named ACTIVE
    # replacement that this run KILLED: "old text absent" alone proves only
    # that the text changed (C9 dev review: M204).
    by_id = {r[0]: r for r in MUTATIONS}
    by_run = {r["id"]: r["result"] for r in rows}

    def applies(mid):
        r = by_id[mid]
        try:
            return open(os.path.join(ROOT, r[2])).read().count(r[3]) == 1
        except OSError:
            return False
    eq_gone = sorted(m for m in EQUIVALENT_DID | LEGACY_EQUIV if not applies(m))
    stale_live = sorted(m for m in STALE_REFACTORED if applies(m))
    stale_bad = []
    for m, rec in sorted(STALE_REFACTORED.items()):
        rep = rec.get("replacement")
        if rep not in by_id or rep in RETIRED or not applies(rep):
            stale_bad.append(f"{m}: replacement {rep!r} is not an active applying row")
        elif rep in by_run and by_run[rep] != "killed":
            stale_bad.append(f"{m}: replacement {rep} was not killed ({by_run[rep]})")
        elif rep not in by_run and in_scope(rep, scope) and not partial:
            stale_bad.append(f"{m}: replacement {rep} is in scope but was not run")
    print(f"Retired EQUIVALENT (four-cell paired-disable, never counted killed): "
          f"{len(EQUIVALENT_DID)} {sorted(EQUIVALENT_DID)}"
          + (f"  <-- NOT EQUIVALENT, guard absent: {eq_gone}" if eq_gone else ""))
    print(f"Retired STALE (old text gone; guard's current form mutated by an ACTIVE killed replacement): "
          f"{len(STALE_REFACTORED)} "
          + str({m: r.get("replacement") for m, r in sorted(STALE_REFACTORED.items())})
          + (f"  <-- NOT STALE, guard still present: {stale_live}" if stale_live else "")
          + (f"  <-- STALE RECORD DOES NOT HOLD: {stale_bad}" if stale_bad else ""))
    if LEGACY_EQUIV:
        print(f"Retired LEGACY (unaudited): {len(LEGACY_EQUIV)} {sorted(LEGACY_EQUIV)}")
    print(f"Unexpected survivors: {len(survivors)}{(' '+str(survivors)) if survivors else ''}")
    print(f"Active rows stale/unapplied: {len(stale)}{(' '+str(stale)) if stale else ''}")
    return not (weak or survivors or stale or eq_gone or stale_live or stale_bad or LEGACY_EQUIV)


def main():
    if sys.argv[1:2] == ["--merge"]:
        if len(sys.argv) < 5:
            sys.exit("usage: v022_g01_mutations.py --merge OUT.json SHARD.json SHARD.json…")
        merge(sys.argv[2], sys.argv[3:])
    args = [a for a in sys.argv[1:] if not a.startswith(("--scope=", "--shard=", "--only="))]
    only_arg = next((a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--only=")), None)
    only = None if only_arg is None else set(only_arg.split(","))
    scope = next((a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--scope=")), "g01")
    shard_arg = next((a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--shard=")), None)
    shard = None
    if shard_arg is not None:
        try:
            k, n = (int(x) for x in shard_arg.split("/"))
            assert 0 <= k < n
        except (ValueError, AssertionError):
            sys.exit(f"--shard={shard_arg}: want K/N with 0 <= K < N")
        shard = (k, n)
    if len(args) != 1 or scope not in ("g01", "pci", "binding", "psv", "all"):
        sys.exit("usage: v022_g01_mutations.py [--scope=g01|pci|binding|psv|all] [--shard=K/N] "
                 "[--only=M1,M2,...] OUT.json")
    if only is not None:
        known = {m[0] for m in MUTATIONS}
        unknown = sorted(m for m in only if m not in known or not in_scope(m, scope))
        if unknown:
            sys.exit(f"--only: not active rows of scope {scope}: {unknown}")
    # Every active row names its attack (drift check, both directions): a row
    # with no marker cannot be scored, and a marker for no row is stale.
    missing = [m[0] for m in MUTATIONS if in_scope(m[0], "all") and m[0] not in ATTACK_MARKERS]
    # A retired EQUIVALENT row needs its joint cell's marker too: without one
    # paired-disable can never see its attack succeed (M58/M245 carried from C8
    # with none, C9 round 1b).
    missing += sorted(r for r in EQUIVALENT_DID if r not in ATTACK_MARKERS)
    orphan = sorted(set(ATTACK_MARKERS) - {m[0] for m in MUTATIONS})
    if missing or orphan:
        sys.exit(f"refused: ATTACK_MARKERS drift: active rows with no marker {missing}; "
                 f"markers for no row {orphan} (scripts/v022_attack_markers.py)")
    sys.argv = [sys.argv[0], args[0]]
    dirty = sh("git status --porcelain -- crates").stdout.strip()
    if dirty:
        sys.exit(f"refused: uncommitted changes under crates/ — a mutation run is evidence about a commit\n{dirty}")
    commit = sh("git rev-parse HEAD").stdout.strip()
    # The Fabric integration tests exec the `axon` interpreter from the target
    # dir: build it from THIS tree first, so no baseline or kill rests on a
    # stale binary, and record which one it was.
    def build_interpreter():
        return subprocess.run(
            ["bash", "-c", f"source scripts/lib_bounded_run.sh && bounded_run {MEM} 1800 "
             "cargo build -q -p axon-core --no-default-features --bin axon"],
            cwd=ROOT, capture_output=True, text=True)
    built = build_interpreter()
    if built.returncode != 0:
        sys.exit(f"refused: could not build the axon interpreter\n{built.stderr[-2000:]}")
    target = os.environ.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))
    axon_bin = os.path.join(target, "debug", "axon")
    toolchain = {
        "rustc": sh("rustc -V").stdout.strip(),
        "cargo": sh("cargo -V").stdout.strip(),
        "axon_bin_sha256": sha(axon_bin) if os.path.exists(axon_bin) else None,
    }
    results, ok = [], True
    baselines = {}
    position = -1
    for (mid, guard, rel, old, new, pkg, target, test) in MUTATIONS:
        if not in_scope(mid, scope):
            continue
        position += 1
        if only is not None and mid not in only:
            continue
        if not in_shard(position, shard):
            continue
        key = (pkg, target, test)
        if key not in baselines:
            baselines[key] = cargo_test(pkg, target, test)[0]
        base = baselines[key]
        path = os.path.join(ROOT, rel)
        original = open(path).read()
        before = sha(path)
        n = original.count(old)
        evidence = None
        if n != 1:
            result = f"not_applicable ({n} matches)"
        else:
            try:
                with open(path, "w") as f:
                    f.write(original.replace(old, new))
                outcome, out = cargo_test(pkg, target, test)
                if outcome != "failed":
                    result = f"survived ({outcome})"
                elif attack_succeeded(mid, out, test):
                    result = "killed"
                else:
                    # The guard is removed and the test fails, but NOT on this
                    # row's attack: another check refused it, or a reason
                    # string differed. Never counted as killed.
                    result = "refused_elsewhere"
                evidence = kill_line(out, test) if outcome == "failed" else None
            finally:
                with open(path, "w") as f:
                    f.write(original)
            if sha(path) != before:
                sys.exit(f"FATAL: {rel} not restored after {mid}")
            # An integration-test kill of an axon-core mutant rebuilds the
            # shared interpreter binary FROM THE MUTANT. Rebuild it from the
            # restored tree, or every later Fabric row runs a mutated
            # interpreter.
            if rel.startswith("crates/axon-core/") and "--lib" not in target:
                if build_interpreter().returncode != 0:
                    sys.exit(f"FATAL: could not rebuild the interpreter after {mid}")
        good = base == "passed" and result == "killed"
        ok &= good
        results.append({"id": mid, "guard": guard, "file": rel, "package": pkg,
                         "target": target, "test": test, "baseline": base, "result": result,
                         "attack_marker": ATTACK_MARKERS.get(mid),
                         "kill_evidence": evidence})
        print(f"{'OK ' if good else 'BAD'} {mid} baseline={base} {result}  {guard}", flush=True)
    # The run must end on the interpreter it started with.
    if toolchain["axon_bin_sha256"] is not None and sha(axon_bin) != toolchain["axon_bin_sha256"]:
        print(f"BAD interpreter binary changed during the run ({axon_bin})", flush=True)
        ok = False
    doc = {"schema": "axon-v022-mutation-run/3", "gate": "G01" if scope == "g01" else scope,
           "scope": scope, "commit": commit,
           "toolchain": toolchain,
           "shard": None if shard is None else {"index": shard[0], "of": shard[1]},
           # A sample (--only) is not a run of the scope, and merge refuses it.
           "only": None if only is None else sorted(only),
           "all_killed": ok, "mutations": results}
    with open(sys.argv[1], "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    ok &= print_evidence_model(results, scope, partial=only is not None or shard is not None)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
