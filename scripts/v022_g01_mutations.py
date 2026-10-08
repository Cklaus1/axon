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

Refuses to run on a tree with ANY uncommitted change (C9 round 4: a check of
crates/ alone let a locally edited scripts/ or profiles/ guard -- 24 rows guard
files there -- and a locally edited registry or marker file stamp the commit):
the result is evidence about a COMMIT, and it names that commit. Every run and
shard records the git blobs of the registry and marker files and each row's
old/new text digest; --merge refuses shards that disagree with each other or
with the registry it judges them against. Exit 0 only when every
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
# protected helper files has neither a row, a checkable exemption nor a counted
# REMAINDER entry.
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
     # C9 r4c (sites, amendment 75): re-anchored. The check is its own `if`, so
     # the refusal gate's block for it starts there and no longer swallows the
     # usage arm four lines above (whose exemption it made "covered by M631").
     '    if let Err(e) = cu::check_store(&cfg.store, euid()) {\n        die(&e);\n    }',
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
     # Re-anchored (gaps, amendment 65): the comparison is the socket and uid; the
     # program pin is the helper's own, checked where the nonce is spent (M1483).
     '        !matches!(&o.custodian, crate::custodian::Custodian::Service(r)\n            if r.socket == helper.custodian.socket && r.uid == helper.custodian.uid)',
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
    ('M650', 'EVIDENCE (decision E): the freeze process refuses a compiler-wrapper variable in its own environment (defence in depth; what BUILT the guest is M734-M738)', 'scripts/v022_freeze_manifest.py',
     '    if wrappers:\n',
     '    if False and wrappers:\n',
     'axon-fabric', '--test freeze_manifest', 'a_compiler_wrapper_does_not_freeze'),
]

# C9 round 4 (harness2, amendment 56): EQUIVALENCE (6) -- every check execs
# the binary built from the tree under test (scripts pick a binary only through
# scripts/lib/axon_bin.sh; tests run scripts only through
# crates/axon-core/tests/script_spawn); EQUIVALENCE (5) -- the harnesses
# refuse any uncommitted change and record the registry they ran; FIELD-ORIGIN
# / PSV-2 -- the guest image is built in a constructed environment
# (scripts/guest_build_env.py) and the freeze binds only such an image.
_HB = '--no-default-features --test harness_binaries'
_HI = '--no-default-features --test harness_integrity'
_GE = 'scripts/guest_build_env.py'
_FZ = 'scripts/v022_freeze_manifest.py'
_FT = 'a_guest_image_not_built_in_the_controlled_environment_does_not_freeze'
MUTATIONS += [
    ('M720', 'EQUIVALENCE (6): a harness that builds nothing runs only the binary its caller names (no fallback to target/ or PATH)', 'scripts/lib/axon_bin.sh',
     # (the mutant's text is split so the script drift scanner reads no path here)
     '  v="${!var:-}"\n', '  v="${!var:-$PWD/tar' + 'get/debug/axon}"\n',
     'axon-core', _HB, 'a_harness_that_builds_nothing_runs_no_binary_its_caller_did_not_name'),
    ('M721', 'EQUIVALENCE (6): a building harness runs the file cargo built, at the path cargo resolves', 'scripts/lib/axon_bin.sh',
     "  printf '%s\\n' \"$d\"\n}", "  printf '%s\\n' \"$PWD/target\"\n}",
     'axon-core', _HB, 'a_building_harness_runs_the_binary_cargo_built_not_one_left_in_target'),
    ('M722', 'EQUIVALENCE (6): an ambient binary-naming variable never reaches a script a test runs', 'crates/axon-core/tests/script_spawn/mod.rs',
     '        c.env_remove(v);\n', '        let _ = v;\n',
     'axon-core', _HB, 'an_ambient_binary_variable_never_reaches_a_script'),
    ('M723', 'EQUIVALENCE (6): the spawn helper refuses a script that guesses its binary', 'crates/axon-core/tests/script_spawn/mod.rs',
     '        bad.is_empty(),\n        "refused to run {}', '        true || bad.is_empty(),\n        "refused to run {}',
     'axon-core', _HB, 'the_spawn_helper_refuses_a_script_that_guesses_its_binary'),
    ('M724', 'EQUIVALENCE (6): the workspace drift test flags an interpreter spawned on a script file in any crate', 'crates/axon-core/tests/script_spawn/mod.rs',
     '            if !inline {\n', '            if false && !inline {\n',
     'axon-core', _HB, 'every_script_spawn_in_the_workspace_goes_through_the_helper'),
    ('M725', 'EQUIVALENCE (6): the script drift test flags a binary chosen by a guessed target/ path', 'crates/axon-core/tests/script_spawn/mod.rs',
     '            if !own.contains(&dir) {\n', '            if false && !own.contains(&dir) {\n',
     'axon-core', _HB, 'no_script_picks_a_binary_it_neither_built_nor_was_given'),
    ('M726', 'EQUIVALENCE (5): a mutation run refuses any uncommitted change in the tree, not only under crates/', 'scripts/v022_g01_mutations.py',
     '    dirty = uncommitted()\n    if dirty:\n', '    dirty = ""\n    if dirty:\n',
     'axon-core', _HI, 'a_mutation_run_refuses_a_tree_with_an_uncommitted_change_outside_crates'),
    ('M727', 'EQUIVALENCE (5): a paired-disable run refuses any uncommitted change in the tree', 'scripts/v022_paired_disable.py',
     '    if "--check-stale" not in sys.argv[1:] and mut.uncommitted():\n', '    if False and "--check-stale" not in sys.argv[1:] and mut.uncommitted():\n',
     'axon-core', _HI, 'a_paired_disable_run_refuses_a_tree_with_an_uncommitted_change_outside_crates'),
    ('M728', 'EQUIVALENCE (5): --merge refuses a shard made from another registry or marker file', 'scripts/v022_g01_mutations.py',
     '        if d.get("registry_blobs") != here:\n', '        if False and d.get("registry_blobs") != here:\n',
     'axon-core', _HI, 'a_merge_refuses_a_shard_made_from_another_registry'),
    ('M729', 'EQUIVALENCE (5): --join refuses a shard made from another registry or marker file', 'scripts/v022_paired_disable.py',
     '        if d.get("registry_blobs") != here:\n', '        if False and d.get("registry_blobs") != here:\n',
     'axon-core', _HI, 'a_join_refuses_a_shard_made_from_another_registry'),
    ('M730', "FIELD-ORIGIN: the guest build refuses any effective cargo config setting it would use (ancestor, dotted key, the tree's own; round 5: judged by key path)", _GE,
     '        why = key_problem(path, triples)\n        if why:\n', '        why = None\n        if why:\n',
     'axon-fabric', '--test guest_build_env', 'a_cargo_config_setting_the_guest_build_would_use_is_refused'),
    ('M731', "FIELD-ORIGIN: the guest build runs cargo in the environment it constructs, never the caller's", _GE,
     '    env = dict(rec["env"])\n', '    env = {**os.environ, **rec["env"]}\n',
     'axon-fabric', '--test guest_build_env', 'a_callers_compiler_wrapper_does_not_reach_the_guest_builds_cargo'),
    ('M732', 'FIELD-ORIGIN: the guest toolchain is resolved by rustup under a cleared environment', _GE,
     '    env = {"HOME": home, "PATH": "/usr/bin:/bin", "LC_ALL": "C"}\n', '    env = dict(os.environ, HOME=home)\n',
     'axon-fabric', '--test guest_build_env', 'a_callers_rustup_home_does_not_choose_the_guest_toolchain'),
    ('M733', 'FIELD-ORIGIN: the provenance helper is compiled by the pinned rustc, never $RUSTC', 'scripts/linux_profile_manifest.py',
     '            rustc = pinned_rustc()\n', '            rustc = os.environ.get("RUSTC") or pinned_rustc()\n',
     'axon-fabric', '--test guest_provenance', 'a_callers_rustc_does_not_build_the_provenance_helper'),
    ('M734', 'EVIDENCE: the freeze binds only a guest image with a controlled-build record', _FZ,
     '    if why:\n        sys.exit("refused: the guest image was not built in the controlled build environment "',
     '    if False and why:\n        sys.exit("refused: the guest image was not built in the controlled build environment "',
     'axon-fabric', '--test freeze_manifest', _FT),
    ('M735', "EVIDENCE: a controlled-build record's environment is exactly the constructed one", _GE,
     '    if env != want or rec.get("cargo_home") != want["CARGO_HOME"] or rec.get("target_dir") != want["CARGO_TARGET_DIR"]:\n',
     '    if False:\n',
     'axon-fabric', '--test freeze_manifest', _FT),
    ('M736', 'EVIDENCE: a controlled-build record names a fresh target dir and CARGO_HOME', _GE,
     '    if rec.get("target_dir_created_empty") is not True or rec.get("cargo_home_created_empty") is not True:\n',
     '    if False:\n',
     'axon-fabric', '--test freeze_manifest', _FT),
    ('M737', 'EVIDENCE: a controlled-build record held no cargo config setting the build would use', _GE,
     '    if (rec.get("effective_config") or {}).get("foreign") != []:\n', '    if False:\n',
     'axon-fabric', '--test freeze_manifest', _FT),
    ('M738', 'EVIDENCE: the freeze binds only guest artifacts the controlled build produced', _FZ,
     '    if unbound:\n', '    if False and unbound:\n',
     'axon-fabric', '--test freeze_manifest', 'a_guest_artifact_the_controlled_build_did_not_produce_does_not_freeze'),
    ('M739', 'EQUIVALENCE (currency): a kept paired-disable record is stale once its owner or consumer packages change', 'scripts/v022_paired_disable.py',
     '    if changed:\n        out.append(f"{len(changed)} file(s) of the owner/consumer packages',
     '    if False and changed:\n        out.append(f"{len(changed)} file(s) of the owner/consumer packages',
     'axon-core', _HI, 'a_kept_record_is_stale_once_its_owner_package_changes'),
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
    # Re-anchored ACTIVE in C9 round 4b (amendment 60): the integer arm was
    # split by width; the i64 arm is the guard's current form (its attack, a
    # confused bool at `-> i64`, is unchanged).
    ('M652', 'PSV-1 (A86): a value is cast to a declared integer type (the i64 arm)', _CC,
     '"i64" | "isize" | "usize" => return kind_ok(matches!(v, Value::Int(_))),',
     '"i64" | "isize" | "usize" => return kind_ok(true),',
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
     # C9 r4c (psv1, amendment 72): re-anchored; the call now passes `entering`.
     '        self.closure_args_check(&contract, &mut args, entering)?;',
     '        let _ = (&contract, entering);',
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
     # C9 r4c (psv1c, amendments 72/78): re-anchored. The old edit (`if true`, no
     # contract at all) now makes the operator's closure an UNDETERMINED position,
     # and the strict closure-argument rule (A102) refuses even the honest control
     # first (REFUSED_ELSEWHERE). The guard's job is that the annotation is
     # ENFORCED, so the edit keeps the position determined but replaces the
     # annotation by a type that admits the confused value.
     '                .map(|p| p.ty.clone().unwrap_or_else(any))',
     '                .map(|p| p.ty.clone().map(|_| T::Union(vec![T::Named("i64".into()), T::Named("bool".into())])).unwrap_or_else(any))',
     'axon-core', _CL, _T4 + 'a_lambdas_annotated_parameter_is_cast'),
]


# ── C9 round 4, POLICY workstream (M670-M689; PSV-6 BLOCKER, amendment 54,
# negative matrix A87): the policy a protected launch runs is the policy its
# launch manifest names (policy_sha256, joined by the observation) and states
# an effect ceiling. One rule (axon_psv::protected_policy_ceiling), applied at
# the root helper before the nonce spend (M670-M672), in the guest runner
# before anything runs (M673-M675), in the launcher both routes run
# (M678-M679); the verdict names the policy digest and Fabric (M676) and the
# loop (M677) join it to the manifest.
_PSV_LIB = 'crates/axon-psv/src/lib.rs'
_PSV_RUN = 'crates/axon-psv/src/runner.rs'
MUTATIONS += [
    ('M670', 'PSV-6/A87: the root helper holds the snapshot policy to the manifest before the spend', _PL,
     '    policy_at_root(staging, &m)?;\n',
     '    let _ = policy_at_root(staging, &m);\n',
     'axon-fabric', '--test privileged_launcher', 'a_genuine_observation_of_one_policy_never_launches_another'),
    ('M671', "PSV-6/A87: the protected policy's sha256 is the manifest's policy_sha256 (root helper route)", _PSV_LIB,
     '    if got != m.policy_sha256 {\n',
     '    if false && got != m.policy_sha256 {\n',
     'axon-fabric', '--test privileged_launcher', 'a_genuine_observation_of_one_policy_never_launches_another'),
    ('M672', 'PSV-6/A87: a protected policy with no allowed_effects is refused, never "no ceiling" (root helper route)', _PSV_LIB,
     '        None => Err(NO_CEILING.into()),',
     '        None => Ok(String::new()),',
     'axon-fabric', '--test privileged_launcher', 'a_manifest_policy_naming_no_ceiling_launches_nothing'),
    ('M673', "PSV-6/A87: the guest runner holds the cmdline policy to the manifest's policy_sha256", _PSV_RUN,
     '        .and_then(|p| protected_policy_ceiling(p, &m))\n',
     '        .map(|_| String::new())\n',
     'axon-psv', '--test runner', 'the_guest_runs_only_the_policy_the_manifest_names'),
    ('M674', 'PSV-6/A87: a protected policy with no allowed_effects is refused, never "no ceiling" (guest runner route)', _PSV_LIB,
     '        None => Err(NO_CEILING.into()),',
     '        None => Ok(String::new()),',
     'axon-psv', '--test runner', 'a_policy_naming_no_ceiling_never_runs_unrestricted'),
    ('M675', "PSV-6/A87: the test always runs under the manifest policy's ceiling (an empty one included)", _PSV_RUN,
     '    cmd.env("AXON_ALLOWED_EFFECTS", without_exec(ceiling));\n',
     '',
     'axon-psv', '--test runner', 'a_policy_naming_no_ceiling_never_runs_unrestricted'),
    ('M676', "PSV-6/A87: Fabric joins the guest verdict's policy_sha256 to the manifest's", 'crates/axon-fabric/src/psv.rs',
     '    if v.policy_sha256 != m.policy_sha256 {\n        return unknown(',
     '    if false && v.policy_sha256 != m.policy_sha256 {\n        return unknown(',
     'axon-fabric', '--test psv_dispatch', 'a_guest_under_a_policy_the_manifest_does_not_name_yields_no_verdict'),
    ('M677', "PSV-6/A87: the loop joins the guest verdict's policy_sha256 to the manifest's", 'crates/axon-loop-contracts/src/protected_evidence.rs',
     '    if v.policy_sha256 != m.policy_sha256 {\n        return Err(format!(',
     '    if false && v.policy_sha256 != m.policy_sha256 {\n        return Err(format!(',
     'axon-loop', '--test intake', 'a_guest_verdict_that_ran_another_policy_is_refused'),
    ('M678', "PSV-6/A87: the launcher boots in PSV mode only the policy the launch manifest names", 'scripts/fc_linux_profile.sh',
     'if got != want:\n',
     'if False and got != want:\n',
     'axon-fabric', '--test launcher_isolation', 'the_launcher_boots_only_the_policy_the_manifest_names'),
    ('M679', 'PSV-6/A87: the launcher refuses a PSV policy that states no effect ceiling', 'scripts/fc_linux_profile.sh',
     'if not isinstance(p, dict) or not isinstance(p.get("allowed_effects"), list):\n',
     'if False:\n',
     'axon-fabric', '--test launcher_isolation', 'the_launcher_boots_only_the_policy_the_manifest_names'),
]

# ── C9 round 4, workstream READINESS (M740-M759): the certification record's
# run attribution joined to verified documents (A88) and the certified B263
# record to the qualification the observed launch ran under (A89).
MUTATIONS += [
    ('M740', "FIELD-ORIGIN (A88): verifier_key_id is the key that signed the certified run's receipt attestation", 'crates/axon-fabric/src/readiness.rs',
     '    attestation::verify(att, &issuer, &req, &rc, &key).map_err(|e| {',
     '    Ok::<String, String>(String::new()).map_err(|e: String| {',
     'axon-fabric', '--test readiness_launch', 'the_verifier_key_id_is_the_key_that_attested_the_run'),
    ('M741', 'FIELD-ORIGIN (A88): the attested receipt is protected evidence', 'crates/axon-fabric/src/readiness.rs',
     '    protected_evidence::check(&req, &rc)\n        .map_err(',
     '    protected_evidence::check(&req, &rc)\n        .or(Ok::<(), String>(()))\n        .map_err(',
     'axon-fabric', '--test readiness_launch', 'an_attested_receipt_that_is_not_protected_evidence_is_refused'),
    ('M742', "FIELD-ORIGIN (A88): the attested receipt names the certified launch's manifest, observation, verdict and qualification", 'crates/axon-fabric/src/readiness.rs',
     '.find(|(p, want)| one_ref(&rc, p) != Some(*want))',
     '.find(|(p, want)| one_ref(&rc, p) != Some(*want) && false)',
     'axon-fabric', '--test readiness_launch', 'an_attested_receipt_of_another_launch_is_refused'),
    ('M743', "FIELD-ORIGIN (A88): the attested request/receipt is the launch manifest's operation, trial and candidate", 'crates/axon-fabric/src/readiness.rs',
     '.find(|(_, a, b)| a.as_str() != *b)',
     '.find(|(_, a, b)| a.as_str() != *b && false)',
     'axon-fabric', '--test readiness_launch', 'an_attested_receipt_of_another_trial_is_refused'),
    ('M744', "FIELD-ORIGIN (A88): the certified observation joins the run's launch manifest (host config, registry, verifier, intended manifest)", 'crates/axon-fabric/src/readiness.rs',
     '    o.joins(&m, &m_sha).map_err(|e| {',
     '    o.joins(&m, &m_sha).or(Ok::<(), String>(())).map_err(|e| {',
     'axon-fabric', '--test readiness_launch', 'the_certified_observation_is_of_the_runs_launch'),
    ('M745', 'PSV-7 (A89): the certified B263 record is the qualification the observed launch ran under', 'crates/axon-fabric/src/readiness.rs',
     '    if m.qualification_sha256 != s("b263_qualification_sha256") {',
     '    if false && m.qualification_sha256 != s("b263_qualification_sha256") {',
     'axon-fabric', '--test readiness_launch', 'a_b263_record_the_launch_did_not_run_under_is_refused'),
    ('M746', 'PSV-7 (A89): the certified B263 record was current when the run was observed', 'crates/axon-fabric/src/readiness.rs',
     '    crate::backend::accept_b263(&q, &b_issuer, observed_at, max_age_s, || {',
     '    crate::backend::accept_b263(&q, &b_issuer, now, max_age_s, || {',
     'axon-fabric', '--test readiness_launch', 'a_b263_record_issued_after_the_run_is_refused'),
    ('M747', "FIELD-ORIGIN (A88): the record's suite is the suite the observed launch ran", 'crates/axon-fabric/src/readiness.rs',
     '.find(|(k, launched)| doc["suite"][k].as_str() != Some(launched.as_str()))',
     '.find(|(k, launched)| doc["suite"][k].as_str() != Some(launched.as_str()) && false)',
     'axon-fabric', '--test readiness_launch', 'the_record_suite_is_the_suite_the_launch_ran'),
    ('M748', "FIELD-ORIGIN (A88): the record's candidate_tree_ref is the candidate the observed launch ran", 'crates/axon-fabric/src/readiness.rs',
     '    if s("candidate_tree_ref") != m.candidate.tree_digest {',
     '    if false && s("candidate_tree_ref") != m.candidate.tree_digest {',
     'axon-fabric', '--test readiness_launch', 'the_record_candidate_is_the_candidate_the_launch_ran'),
    ('M749', 'FIELD-ORIGIN (A88): exactly one run document of each kind is in the certified evidence', 'crates/axon-fabric/src/readiness.rs',
     '        [(_, _, b)] => Ok(b),',
     '        [(_, _, b), ..] => Ok(b),',
     'axon-fabric', '--test readiness_launch', 'a_record_carrying_two_runs_is_refused'),
    ('M750', 'FIELD-ORIGIN (A88/A89): readiness requires the run in the certified evidence and joins it', 'crates/axon-fabric/src/readiness.rs',
     '    launched(component, doc, trust, evidence, &o)\n}',
     '    let _ = launched;\n    Ok(())\n}',
     'axon-fabric', '--test readiness_launch', 'a_record_whose_evidence_lacks_the_run_is_refused'),
]

# ── C9 round 4, EQUIVALENCE (ROWS workstream, M690-M719; amendment 55) ─────
# The dev review (round 4) found protected rules enforced in production
# through CALLS with no row (the whole axon-fabric suite stayed green with
# each call removed), and ACTIVE rows killed only by a unit test calling the
# rule function directly. Every row below is killed through the PRODUCTION
# entry: readiness's decision in the installed `verify-readiness`
# (ReadinessTrust::operator(), /etc/axon/trust in a private mount namespace),
# the production `axon-custodian` socket-activated under /etc/axon, a
# production `axon-fabric` whose ProtectedHost::operator() reads
# /etc/axon/protected-host.json, and the setuid-root helper in production
# mode. /etc/axon is a tmpfs in a private mount namespace; the host's /etc is
# never written.
_RDT = '--test readiness'
_RDW = 'a_readiness_run_that_can_write_its_trust_roots_certifies_nothing'
_CUT = 'a_protected_custodian_under_a_config_breaking_a83_serves_nothing'
_ACT = 'a_protected_custodian_serves_only_its_units_activation_on_its_socket'
_PHR = 'a_production_fabric_running_as_root_is_refused_on_a_protected_host'
_PHC = 'a_production_fabric_refuses_a_protected_host_whose_custodian_is_the_fabric'
_PHH = 'a_production_fabric_refuses_a_helper_config_that_disagrees_with_its_host'
_LEA = 'a_production_helper_launches_nothing_it_cannot_lease'
MUTATIONS += [
    # readiness: the decision's own calls and bindings.
    ('M690', "readiness: the certification decision checks the operator's trust roots (trust.check(), incl. unwritable by the verifier)", _RD,
     '    trust.check()?;\n    attribution(', '    let _ = trust.check();\n    attribution(',
     'axon-fabric', _RDT, _RDW),
    ('M691', 'readiness: the evidence bundle is the certified one (evidence_bundle_sha256)', _RD,
     '    if doc["evidence_bundle_sha256"].as_str()\n', '    if false && doc["evidence_bundle_sha256"].as_str()\n',
     'axon-fabric', _RDT, 'an_evidence_file_changed_after_certification_is_not_certified'),
    ('M692', 'readiness: the record certifies THIS component', _RD,
     '    if doc["component"] != component\n', '    if false && doc["component"] != component\n',
     'axon-fabric', _RDT, 'a_record_for_another_component_or_profile_is_not_certified'),
    ('M693', 'readiness: the record certifies the protected host profile', _RD,
     '        || doc["host_profile"] != PROTECTED_PROFILE\n', '        || false\n',
     'axon-fabric', _RDT, 'a_record_for_another_component_or_profile_is_not_certified'),
    ('M694', 'readiness: the record certifies the protected qualification profile', _RD,
     '        || doc["qualification_profile"] != PROTECTED_PROFILE\n', '        || false\n',
     'axon-fabric', _RDT, 'a_record_for_another_component_or_profile_is_not_certified'),
    ('M695', "readiness: the record certifies this tree's PSV spec", _RD,
     '    if doc["psv_spec_sha256"].as_str() != Some(', '    if false && doc["psv_spec_sha256"].as_str() != Some(',
     'axon-fabric', _RDT, 'a_changed_psv_spec_is_not_certified'),
    ('M696', 'readiness: this tree descends from the certified revision (the descends call)', _RD,
     '    if let Err(e) = crate::git_data::descends(&top, certified) {', '    if let Err(e) = Ok::<(), String>(()) {',
     'axon-fabric', _RDT, 'a_history_not_descending_from_the_certified_revision_is_not_certified'),
    ('M697', 'readiness: nothing outside governance/ changed since the certified revision', _RD,
     '    if let Some(f) = outside.first() {', '    if let Some(f) = None::<&String> {',
     'axon-fabric', _RDT, 'a_committed_code_change_is_not_certified'),
    ('M698', 'readiness: the record is of the certification schema', _RD,
     '    if doc["schema"] != CERT_SCHEMA || !missing.is_empty() {', '    if !missing.is_empty() {',
     'axon-fabric', _RDT, 'a_record_of_another_schema_is_not_certified'),
    ('M699', "readiness: the record's attribution is checked at the decision (the attribution call)", _RD,
     '    attribution(component, &doc, trust, &evidence)?;', '    let _ = attribution(component, &doc, trust, &evidence);',
     'axon-fabric', _RDT, 'a_record_attributed_to_an_untrusted_observer_is_not_certified'),
    # the production custodian.
    ('M700', 'A83: the production custodian applies the PROTECTED config rules (load_config -> check(true))', _CU,
     '    c.check(!a.test)\n', '    c.check(false)\n',
     'axon-fabric', _HT, _CUT),
    ('M701', 'A83: a protected custodian config names neither the custodian nor the Fabric as root', _CU,
     '        if self.custodian_uid == 0 || self.fabric_uid == 0 {',
     '        if false && (self.custodian_uid == 0 || self.fabric_uid == 0) {',
     'axon-fabric', _HT, _CUT),
    ('M702', 'D6: a protected custodian serves only a listener its socket unit passed (LISTEN_PID/LISTEN_FDS)', _CU,
     '    if pid != std::process::id().to_string() || fds != "1" {',
     '    if false && (pid != std::process::id().to_string() || fds != "1") {',
     'axon-fabric', _HT, _ACT),
    ('M703', 'D6: a protected custodian serves only on the socket its config names', _CU,
     '    if at.as_pathname() != Some(socket) {', '    if false && at.as_pathname() != Some(socket) {',
     'axon-fabric', _HT, _ACT),
    # ProtectedHost::operator(), the one production caller of each rule.
    ('M704', 'A: a protected host refuses a root Fabric (the fabric_is_not_root call in operator())', _PH,
     '        fabric_is_not_root(euid)?;\n', '        let _ = fabric_is_not_root(euid);\n',
     'axon-fabric', _HT, _PHR),
    ('M705', 'A83: a protected host refuses a custodian that is the Fabric uid or root (the custodian_is_separate call)', _PH,
     '            custodian_is_separate(c.uid, euid)?;', '            let _ = custodian_is_separate(c.uid, euid);',
     'axon-fabric', _HT, _PHC),
    ('M706', "A83: operator() reads the helper's config under the PRODUCTION rules", _PH,
     '            &crate::privileged_launcher::Authority::production(),\n        )?;\n        helper_agrees(',
     '            &crate::privileged_launcher::Authority {\n                test: true,\n'
     '                ..crate::privileged_launcher::Authority::production()\n            },\n        )?;\n        helper_agrees(',
     'axon-fabric', _HT, _PHH),
    ('M707', "A: operator() refuses a helper config describing another launch path (the helper_agrees call)", _PH,
     '        helper_agrees(&helper, &host, euid)?;', '        let _ = helper_agrees(&helper, &host, euid);',
     'axon-fabric', _HT, _PHH),
    # decision D: the production helper's lease policy.
    ('M708', 'D: the production helper opens authority programs under Lease::Required (Authority::lease)', _HPL,
     '        // Root holds CAP_LEASE, so a production helper always gets one.\n        if self.test {',
     '        // Root holds CAP_LEASE, so a production helper always gets one.\n        if true || self.test {',
     'axon-fabric', _HT, _LEA),
    ('M709', "D: the test-trust lease switch is not in a production build (the seam M708's test drives)", _HSE,
     '    #[cfg(feature = "test-trust-root")]\n    if Path::new("/etc/axon/TEST-no-read-lease").exists() {',
     '    #[cfg(all())]\n    if Path::new("/etc/axon/TEST-no-read-lease").exists() {',
     'axon-fabric', _HT, _LEA),
]
# Rows the round-4 review found killed only by a unit test calling the rule
# directly, RE-ANCHORED on the production route above (the unit tests stay as
# controls). Previous test in the comment.
_REANCHOR_R4 = {
    'M490': (_RDT, _RDW),  # was --lib readiness::tests::a_trust_root_this_process_can_write_authorizes_nothing
    'M491': (_RDT, _RDW),  # was the same unit test
    'M492': (_RDT, _RDW),  # was the same unit test
    'M629': (_HT, _CUT),   # was --lib custodian::tests::a_custodian_that_is_the_fabric_is_refused
    'M640': (_HT, _CUT),   # was --lib custodian::tests::a_protected_custodian_config_lets_only_root_spend
    'M546': (_HT, _PHR),   # was --lib protected_host::tests::a_root_fabric_is_refused_on_a_protected_host (EQUIVALENT below)
    'M634': (_HT, _PHC),   # was --lib protected_host::tests::a_custodian_that_is_the_fabric_uid_is_refused_on_a_protected_host (EQUIVALENT below)
    'M547': (_HT, _PHH),   # was --test protected_host the_helper_config_must_agree_with_the_host_config
    'M548': (_HT, _PHH),   # was the same test (helper_agrees called directly)
    'M636': (_HT, _PHH),   # was the same test
    'M637': (_HT, _PHH),   # was the same test
    'M591': (_HT, _LEA),   # was --lib sealed_exec::tests::an_authority_program_that_cannot_be_leased_is_refused_in_production
}
MUTATIONS = [r[:6] + _REANCHOR_R4[r[0]] if r[0] in _REANCHOR_R4 else r for r in MUTATIONS]

# C9 round 4 (harness2, amendment 56 extended): rows for the guards the first
# wave left unrowed, and for the guest serial record cut by the guest's reboot.
_SP = 'crates/axon-core/tests/script_spawn/mod.rs'
_HB2 = '--no-default-features --test harness_binaries'
_HI2 = '--no-default-features --test harness_integrity'
_MC = 'a_mutation_run_leaves_no_mutant_binary_and_hides_the_callers_binary'
_PC = 'a_paired_disable_run_scrubs_counts_skips_and_hides_the_callers_binary'
_FCL = 'scripts/fc_linux_profile.sh'
MUTATIONS += [
    ('M860', 'EQUIVALENCE (6): a binary a harness names for a test to exec is refused when older than its sources', _SP,
     '        assert!(\n            stale.is_none(),\n', '        assert!(\n            true || stale.is_none(),\n',
     'axon-core', _HB2, 'a_named_binary_older_than_its_sources_is_refused'),
    ('M861', 'EQUIVALENCE (6): the drift gate refuses a test that resolves a workspace binary itself', _SP,
     '        if reads_var || names_profile_bin {\n', '        if false && (reads_var || names_profile_bin) {\n',
     'axon-core', _HB2, 'a_test_that_resolves_its_own_binary_is_flagged'),
    ('M862', 'EQUIVALENCE (5): --merge refuses a shard that does not record a clean tree', 'scripts/v022_g01_mutations.py',
     '        if d.get("tree_clean") is not True:\n            sys.exit(f"refused: shard {p} does not record a clean tree")\n        by_id',
     '        if False:\n            sys.exit(f"refused: shard {p} does not record a clean tree")\n        by_id',
     'axon-core', _HI2, 'a_merge_refuses_a_shard_from_a_dirty_tree'),
    ('M863', "EQUIVALENCE (5): --merge refuses a row executed with an edit that is not this registry's row", 'scripts/v022_g01_mutations.py',
     '            if row is None or {k: r.get(k) for k in ("old_sha256", "new_sha256")} != row_digest(row):\n',
     '            if row is None:\n',
     'axon-core', _HI2, 'a_merge_refuses_a_row_run_with_another_edit'),
    ('M864', 'EQUIVALENCE (5): --join refuses a shard that does not record a clean tree', 'scripts/v022_paired_disable.py',
     '        if d.get("tree_clean") is not True:\n            sys.exit(f"refused: shard {p} does not record a clean tree")\n',
     '        if False:\n            sys.exit(f"refused: shard {p} does not record a clean tree")\n',
     'axon-core', _HI2, 'a_join_refuses_a_shard_from_a_dirty_tree'),
    ('M865', "EQUIVALENCE (5): --join refuses a record executed with edits that are not this registry's", 'scripts/v022_paired_disable.py',
     '            if r.get("edits_sha256") != current_edits_digest(r["mutation"]):\n',
     '            if False:\n',
     'axon-core', _HI2, 'a_join_refuses_a_record_run_with_other_edits'),
    ('M866', 'EQUIVALENCE (6d): a mutation run removes binaries a mutated cell built into the workspace target dir', 'scripts/v022_g01_mutations.py',
     '            scrubbed = rel.startswith("crates/") and spawns_scripts(pkg, target)\n',
     '            scrubbed = False\n',
     'axon-core', _HI2, _MC),
    ('M867', "EQUIVALENCE (minor): harness cells run with the caller's AXON/AXON_BIN/CORTEX_BIN removed", 'scripts/v022_g01_mutations.py',
     'UNSET_AMBIENT = "unset " + " ".join(AMBIENT_BINARY_VARS) + "; "\n',
     'UNSET_AMBIENT = ""\n',
     'axon-core', _HI2, _MC),
    # Re-anchored (rows3, amendment 59): after_cell scrubs only when the cell
    # ran a building script, and rebuilds only a prerequisite that changed.
    ('M868', 'EQUIVALENCE (6d): a paired-disable cell on a mutated tree leaves no binary in the workspace target dir', 'scripts/v022_paired_disable.py',
     '    if spawned and any(e[0].startswith("crates/") for e in edits):\n        mut.scrub_workspace_binaries()\n',
     '    if spawned and any(e[0].startswith("crates/") for e in edits):\n        pass\n',
     'axon-core', _HI2, _PC),
    ('M869', 'EQUIVALENCE (minor): a full-suite cell records a test that skipped as a skip, never a silent pass', 'scripts/v022_paired_disable.py',
     '    CELL_SKIPS[(pkg, flags, env)] = skipped_tests(out)\n',
     '    CELL_SKIPS[(pkg, flags, env)] = []\n',
     'axon-core', _HI2, _PC),
    ('M870', 'PSV-2/serial: a serial record is never the unterminated text after the last newline', _FCL,
     '    lines = f.read().decode("utf-8", "replace").split("\\n")[:-1]\n',
     '    lines = f.read().decode("utf-8", "replace").split("\\n")\n',
     'axon-fabric', '--test serial_records', 'an_unterminated_serial_record_is_no_record'),
    ('M871', 'PSV-2/serial: a serial record is a whole line, never a match inside another line', _FCL,
     '    m = re.fullmatch(rx, line[:-1] if line.endswith("\\r") else line)\n',
     '    m = re.search(rx, line[:-1] if line.endswith("\\r") else line)\n',
     'axon-fabric', '--test serial_records', 'a_verdict_record_spliced_with_a_kernel_message_is_no_record'),
    ('M872', 'PSV-2/serial: the guest drains its serial console before every reboot', 'profiles/linux-microvm/guest-init.sh',
     '    stty -F /dev/console -echoprt 2>/dev/null\n',
     '',
     'axon-guest-init', '--test b263_profile_wiring', 'every_guest_reboot_first_drains_the_serial_console'),
    ('M873', 'PSV-2/serial: no reboot of the guest bypasses the console drain', 'profiles/linux-microvm/guest-init.sh',
     'echo "B263-DONE"\nhalt_guest\n',
     'echo "B263-DONE"\nreboot -f\n',
     'axon-guest-init', '--test b263_profile_wiring', 'every_guest_reboot_first_drains_the_serial_console'),
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
                 "only ever ADDS reasons; an untracked non-ignored file is one the walk reports "
                 "unless the operator allowlist names it, and an allowlisted file is excused by "
                 "decision C, which makes the allowlist the authority (so what status would add "
                 "there is not a lost property). So M347's refusal never stands alone"}
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
    "subsumed_by": ["M628", "M1489"], "killer": "joint:M602+M628+M1489",
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
                 "production helper and the production (socket-activated) custodian. Amendment 66 "
                 "(final): since amendment 65 a production helper config must pin the custodian "
                 "program (M1485) and every reply is verified against the pin (M1489, the "
                 "verification as a whole; M1483 its comparison). A helper whose euid is not 0 "
                 "cannot open the custodian's /proc/<pid>/exe (another uid's process: ptrace "
                 "read access) and refuses the reply, so the set is {M628, M1489}: measured at "
                 "78832d1b, with M602 removed the refusal is the pin verification's, and with "
                 "M602+M628+M1489 removed the non-root helper launches"}
# C9 round 4, ROWS workstream (EQUIVALENCE): fabric_is_not_root and
# custodian_is_separate have ONE production caller, ProtectedHost::operator(),
# where each is one of two checks that refuse its attack alone. Their only
# kill was a unit test calling the function; the four cells are executed on
# the production route (a production axon-fabric reading /etc/axon in a
# private mount namespace) with scripts/v022_paired_disable.py, and the unit
# tests are controls only (protected_host::tests).
EQUIV_RECORD["M704"] = {
    "property": "a protected host refuses a Fabric running as root",
    "subsumed_by": ["M548"], "killer": "joint:M704+M548",
    "all_paths": "operator() is the only production caller of fabric_is_not_root and the only "
                 "constructor of a production ProtectedHost (the test-trust --protected-host-config "
                 "route is absent from a production build). After the call it always loads the "
                 "helper's operator config with Authority::production() (M706) and runs "
                 "helper_agrees(helper, host, euid). load_config refuses a helper config with "
                 "fabric_uid 0 in production (M536), and helper_agrees refuses fabric_uid != euid "
                 "(M548). So with euid 0 every path is refused: fabric_uid 0 by M536, any other "
                 "by M548. Executed with the production axon-fabric as root"}
EQUIV_RECORD["M546"] = {
    "property": "a protected host refuses a Fabric running as root",
    "subsumed_by": ["M548"], "killer": "joint:M546+M548",
    "all_paths": "fabric_is_not_root has one caller, operator() (M704's call); M546 disables the "
                 "same refusal inside it, so M704's argument applies unchanged: euid 0 is refused "
                 "by M536 (helper fabric_uid 0) or M548 (any other)"}
EQUIV_RECORD["M705"] = {
    "property": "a protected host refuses a custodian that is the Fabric's own uid or root",
    "subsumed_by": ["M633"], "killer": "joint:M705+M633",
    "all_paths": "operator() is custodian_is_separate's only production caller, reached only for "
                 "a host with an observer (a Service custodian). It then loads the helper config "
                 "under the production rules (M706), whose own rule refuses a helper custodian "
                 "that is the helper's fabric_uid or 0 (M633), and helper_agrees requires the "
                 "helper's custodian to EQUAL the host's (M636) and its fabric_uid to equal euid "
                 "(M548). So a host custodian equal to euid or 0 is refused on every path: by "
                 "M633 when the helper agrees, by M636 or M548 when it does not. A host with no "
                 "observer names no custodian for Fabric to be issued a nonce by. Executed with "
                 "the production axon-fabric"}
EQUIV_RECORD["M634"] = {
    "property": "a protected host refuses a custodian that is the Fabric's own uid or root",
    "subsumed_by": ["M633"], "killer": "joint:M634+M633",
    "all_paths": "custodian_is_separate has one caller, operator() (M705's call); M634 disables "
                 "the same refusal inside it, so M705's argument applies unchanged"}
# C9 round 4 fix wave, ROWS2 wave 2, STRICT: dominated sites retired with
# executed four-cell records (never counted killed).
EQUIV_RECORD["M796"] = {
    "property": "the helper launches only a request whose psv_manifest_sha256 is a lowercase sha256",
    "subsumed_by": ["M623", "M797"], "killer": "joint:M796+M623+M797",
    "all_paths": "validate_request has one production caller, prepare (via serve_as). Every "
                 "prepared request reaches observed_launch before anything is launched: "
                 "snapshot_manifest compares sha256_hex of the snapshot manifest (always 64 "
                 "lowercase hex) with the request's word (M623), so a word that is not lowercase "
                 "hex never equals it; and the nonce is spent with that word, which the "
                 "custodian refuses unless it is 64 lowercase hex (M797). Executed with a "
                 "genuine observation of the uppercase spelling"}
EQUIV_RECORD["M801"] = {
    "property": "each psv input of a request is <out root>/<inputs>/<its fixed leaf>",
    "subsumed_by": ["M800"], "killer": "joint:M801+M800",
    "all_paths": "the leaf-name rule and the exact-spelling rule run on the same path p in the "
                 "same loop iteration. If p's file name is not `leaf`, parent.join(leaf) (a path "
                 "ending in `leaf`) differs from p, so M800 refuses every input M801 refuses"}
EQUIV_RECORD["M803"] = {
    "property": "the root launcher writes only into the out dir the helper created",
    "subsumed_by": ["M802"], "killer": "joint:M803+M802",
    "all_paths": "make_out's only caller is prepare; mkdirat must CREATE the out dir (M802), so "
                 "a pre-existing dir of any owner is refused before the owner re-check. The "
                 "re-check refuses only a dir replaced between mkdirat and openat, a race no "
                 "deterministic test interposes; its four-cell attack is a Fabric-owned dir "
                 "already at the out path"}
EQUIV_RECORD["M804"] = {
    "property": "the out dir of a launch is not its psv inputs dir",
    "subsumed_by": ["M802"], "killer": "joint:M804+M802",
    "all_paths": "validate_request's only caller is prepare, which creates the out dir with "
                 "mkdirat (M802) after snapshot_inputs opened the inputs dir: an out name equal "
                 "to the inputs name names a directory that exists, so mkdirat refuses it"}
EQUIV_RECORD["M809"] = {
    "property": "readiness never reads a failed git as an empty answer",
    "subsumed_by": ["M810"], "killer": "joint:M809+M810",
    "all_paths": "every git() answer feeds one of: the refs/replace/ refusal (M285, retired: "
                 "replacement objects are off, M385), the grafts path (an empty answer names the "
                 "repository itself, which exists, so the graft refusal fires), the index tags "
                 "(M287/M288, retired: the tree is compared by its bytes), the change lists "
                 "(diff, diff-index, ls-files --others), which only ADD to `outside`, and HEAD's "
                 "name for the hash-checked comparison (an empty name makes Objects::entries "
                 "fail). When the change lists add nothing, the comparison from hash-checked "
                 "objects runs (M810) and M697 refuses any difference. Executed with a failing "
                 "`git diff` over a committed code change"}
EQUIV_RECORD["M813"] = {
    "property": "a custodian serves only from a store that is a real directory",
    "subsumed_by": ["M325"], "killer": "joint:M813+M325",
    "all_paths": "check_store has one caller per custodian mode (the binary, before any "
                 "connection). A symlink's lstat mode is 0777 on Linux, so the next rule (no "
                 "group/other access, M325) refuses every symlink; a non-directory the custodian "
                 "owns with mode 0600 fails every issue and spend (ENOTDIR under it), so it "
                 "serves nothing. Executed with the production custodian and a symlink it owns"}
EQUIV_RECORD["M338"] = {
    "property": "the record's verifier_key_id is a key of the operator's verifier root",
    "subsumed_by": ["M812"], "killer": "joint:M338+M812",
    "all_paths": "attribution has one caller (the decision) and always runs launched() at its "
                 "end (M750). launched() looks verifier_key_id up among the verifier root's "
                 "exclusive keys and refuses when none has that id; exclusive_keys is a subset "
                 "of key_ids (it drops keys shared with another root), so every id the "
                 "membership check refuses the lookup also refuses. Since amendment 57"}
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
# ── C9 round 4 fix wave, ROWS2 workstream (M760-M819; amendment 58): the
# refusal sites scripts/v022_refusal_coverage.py names once it scans the
# custodian, its binary, readiness and the protected host config (and the two
# the policy change added to the helper). Each is attacked on the PRODUCTION
# route where it is the only refusal.
_R2T = 'a_protected_custodian_under_a_malformed_config_serves_nothing'
_R2H = 'a_production_fabric_refuses_a_host_config_it_cannot_vouch_for'
_R2V = 'a_production_verifier_built_from_a_dirty_tree_certifies_nothing'
MUTATIONS += [
    ('M760', 'A83: a protected custodian applies its config schema (production custodian)',
     'crates/axon-fabric/src/custodian.rs',
     '        if self.schema != CONFIG_SCHEMA {', '        if false && self.schema != CONFIG_SCHEMA {',
     'axon-fabric', '--test privileged_launcher', _R2T),
    ('M761', 'A83: a protected custodian requires a positive nonce lifetime (production custodian)',
     'crates/axon-fabric/src/custodian.rs',
     '        if self.max_age_s == 0 {', '        if false && self.max_age_s == 0 {',
     'axon-fabric', '--test privileged_launcher', _R2T),
    ('M762', "A84: the helper reads a custodian's refusal of a spend as a refusal (CustodianRef::call)",
     'crates/axon-fabric/src/custodian.rs',
     '        if !r.ok {', '        if false && !r.ok {',
     'axon-fabric', '--test privileged_launcher', 'one_observation_launches_the_root_launcher_once'),
    ('M763', 'D6: --test-config exists only in a test-trust build of the custodian (PRODUCTION build)',
     'crates/axon-fabric/src/bin/axon-custodian.rs',
     '        Some("--test-config") if axon_fabric::backend::TEST_TRUST_BUILD => {',
     '        Some("--test-config") => {',
     'axon-fabric', '--test privileged_launcher',
     'a_production_custodian_never_takes_its_config_from_a_path_its_caller_names'),
    ('M764', 'A: a production Fabric reads only an axon-protected-host/1 config (ProtectedHost::load)',
     'crates/axon-fabric/src/protected_host.rs',
     '        if v["schema"] != PROTECTED_HOST_SCHEMA {', '        if false && v["schema"] != PROTECTED_HOST_SCHEMA {',
     'axon-fabric', '--test privileged_launcher', _R2H),
    ('M765', "A: a production Fabric refuses the helper's test-trust config key (PRODUCTION build)",
     'crates/axon-fabric/src/protected_host.rs',
     '            Some(_) if !crate::backend::TEST_TRUST_BUILD => {', '            Some(_) if false => {',
     'axon-fabric', '--test privileged_launcher', _R2H),
    ('M766', 'A: a host config that cannot be stat\'ed runs nothing (never read as not-a-protected-host)',
     'crates/axon-fabric/src/protected_host.rs',
     '        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),\n        Err(e) => Err(format!(',
     '        Err(_) => Ok(false),\n        #[allow(unreachable_patterns)]\n        Err(e) => Err(format!(',
     'axon-fabric', '--test privileged_launcher', _R2H),
    ('M767', 'PSV-7: a production readiness verifier built from a dirty tree certifies nothing',
     'crates/axon-fabric/src/readiness.rs',
     '    if !TEST_TRUST_BUILD && me["source_dirty"] == true {',
     '    if false && !TEST_TRUST_BUILD && me["source_dirty"] == true {',
     'axon-fabric', '--test readiness', _R2V),
    ('M768', 'readiness: a narrowing list the verifier cannot stat is not read as absent (production decision)',
     'crates/axon-fabric/src/readiness.rs',
     '        Err(e) => return Err(format!("{TRUST_EXPECTATIONS}: {e}")),', '        Err(_) => false,',
     'axon-fabric', '--test readiness', 'a_narrowing_list_the_verifier_cannot_stat_is_not_read_as_absent'),
    ('M769', 'readiness: the repository may narrow the qualification issuers, never add',
     'crates/axon-fabric/src/readiness.rs',
     '            if !list.iter().any(|x| x.as_str() == Some(issuer.as_str())) {',
     '            if false && !list.iter().any(|x| x.as_str() == Some(issuer.as_str())) {',
     'axon-fabric', '--test readiness', 'the_repository_may_narrow_the_issuers_never_add'),
    ('M770', "PSV-4: the readiness verifier's identity names a test-trust build as such (the relay requires production)",
     'crates/axon-fabric/src/readiness.rs',
     '        "sha256": sha,\n        "build": if TEST_TRUST_BUILD {', '        "sha256": sha,\n        "build": if false {',
     'axon-fabric', '--test readiness', _R2V),
    ('M771', "PSV-4: the readiness report names a test-trust build as such",
     'crates/axon-fabric/src/readiness.rs',
     '        // A build carrying the test trust constructors never earns readiness.\n        "build": if TEST_TRUST_BUILD {',
     '        // A build carrying the test trust constructors never earns readiness.\n        "build": if false {',
     'axon-fabric', '--test readiness', _R2V),
    ('M772', "A: the root helper reads the guest policy only as a regular file of the Fabric uid",
     'crates/axon-fabric/src/privileged_launcher.rs',
     '    if st.st_mode & libc::S_IFMT != libc::S_IFREG || st.st_uid != owner {',
     '    if false && (st.st_mode & libc::S_IFMT != libc::S_IFREG || st.st_uid != owner) {',
     'axon-fabric', '--test privileged_launcher', 'a_root_owned_policy_is_never_read_by_the_helper'),
    ('M773', 'A84: the observation joins the launch manifest field for field (the comparison of every pair)',
     'crates/axon-psv/src/lib.rs',
     '            if observed != launch {', '            if false && observed != launch {',
     'axon-fabric', '--test privileged_launcher', 'an_observation_of_another_manifest_launches_nothing'),
    ('M774', "A84: the observation's guest init is the launch manifest's",
     'crates/axon-psv/src/lib.rs',
     '        if self.guest.init_sha256 != m.guest.init_sha256 {',
     '        if false && self.guest.init_sha256 != m.guest.init_sha256 {',
     'axon-fabric', '--test privileged_launcher', 'an_observation_of_another_guest_init_launches_nothing'),
]

# ── C9 round 4 fix wave, ROWS2 wave 2 (M775-M794; amendment 58): the sites the
# extended site pattern names in psv.rs, runner.rs and protected_evidence.rs.
MUTATIONS += [
    ('M775', 'PSV-4: a check that produced no verdict is never receipted as one (CheckVerdict::NotRun)', 'crates/axon-fabric/src/psv.rs', '        CheckVerdict::NotRun => {\n            return unknown(', '        CheckVerdict::NotRun => ReceiptVerification::Passed,\n        #[allow(unreachable_patterns)]\n        CheckVerdict::NotRun => {\n            return unknown(', 'axon-fabric', '--test psv_dispatch', 'a_check_that_produced_no_verdict_is_never_receipted_as_one'),
    ('M776', 'M4: a protected receipt states an evidence class (none is not protected)', 'crates/axon-loop-contracts/src/protected_evidence.rs', '        [] => return Err("the receipt states no evidence class: not protected evidence".into()),', '        [] => {}', 'axon-fabric', '--test readiness_launch', 'an_attested_receipt_stating_no_evidence_class_is_refused'),
    ('M777', 'M4: a protected receipt states exactly one evidence class', 'crates/axon-loop-contracts/src/protected_evidence.rs', '        many => {\n            return Err(format!(\n                "the receipt states {} evidence classes",\n                many.len()\n            ))\n        }', '        many => {\n            let _ = many;\n        }', 'axon-fabric', '--test readiness_launch', 'an_attested_receipt_stating_two_evidence_classes_is_refused'),
    ('M778', 'M4: a protected receipt names every required digest ref', 'crates/axon-loop-contracts/src/protected_evidence.rs', '            [] => return Err(format!("a protected receipt names no {prefix}…")),', '            [] => {}', 'axon-fabric', '--test readiness_launch', 'an_attested_receipt_naming_no_guest_kernel_is_refused'),
    ('M779', 'PSV-5: the receipt counts the outcome the guest verdict claims (a counted failure of a guest pass)', 'crates/axon-loop-contracts/src/protected_evidence.rs', '        (RV::Passed, GS::Passed) | (RV::Failed, GS::Failed) => {}\n        (counted, claimed) => {', '        (RV::Passed, GS::Passed) | (RV::Failed, _) => {}\n        (counted, claimed) => {', 'axon-loop', '--test intake', 'a_receipt_counting_a_guest_pass_as_a_failure_is_refused'),
    ('M780', 'A3: the runner runs only a suite entry that is a file in the suite tree', 'crates/axon-psv/src/runner.rs', '    if axon_workspace_recipe::check_path(&m.suite.entry, &Quota::default()).is_err()', '    if false && axon_workspace_recipe::check_path(&m.suite.entry, &Quota::default()).is_err()', 'axon-psv', '--test runner', 'a_suite_entry_outside_the_suite_tree_never_runs'),
    ('M781', "PSV limits: a run whose output exceeded the manifest's limit yields no verdict", 'crates/axon-psv/src/runner.rs', '    if oo || eo {', '    if false && (oo || eo) {', 'axon-psv', '--test runner', 'a_run_that_exceeded_its_output_limit_yields_no_verdict'),
]

# ── C9 round 4 fix wave, ROWS2 wave 2, STRICT (M795-M814) ──────────────────
# Refusal sites exempt as "dominated" (or with no stated kind) in
# scripts/v022_refusal_coverage.py, under the integrator's strict reading:
# each is an ACTIVE row killed by an attack that reaches it alone, or
# EQUIVALENT_DID (EQUIV_RECORD below) with an executed four-cell record.
_S2PL = 'crates/axon-fabric/src/privileged_launcher.rs'
_S2CU = 'crates/axon-fabric/src/custodian.rs'
_S2PH = 'crates/axon-fabric/src/protected_host.rs'
_S2RD = 'crates/axon-fabric/src/readiness.rs'
_S2HT = '--test privileged_launcher'
MUTATIONS += [
    ('M795', 'A: the helper takes one request schema', _S2PL,
     '    if r.schema != REQUEST_SCHEMA {', '    if false && r.schema != REQUEST_SCHEMA {',
     'axon-fabric', _S2HT, 'a_request_of_another_schema_launches_nothing'),
    ('M796', "A: the request's psv_manifest_sha256 is a lowercase sha256", _S2PL,
     '    if !is_hex64(&r.psv_manifest_sha256) {', '    if false && !is_hex64(&r.psv_manifest_sha256) {',
     'axon-fabric', _S2HT, 'a_request_naming_its_manifest_digest_in_another_spelling_launches_nothing'),
    ('M797', 'A84: a custodian spend names its launch manifest as a lowercase sha256 (production custodian)', _S2CU,
     '                    .filter(|m| is_hex(m, 64))', '                    .filter(|_| true)',
     'axon-fabric', _S2HT, 'a_protected_custodian_spends_nothing_for_a_spend_naming_no_manifest'),
    ('M798', "A: a request's timeout is bounded by the operator's max_timeout_s", _S2PL,
     '    if r.timeout_s == 0 || r.timeout_s > c.max_timeout_s {', '    if r.timeout_s == 0 {',
     'axon-fabric', _S2HT, 'a_request_asking_for_more_time_than_the_operator_allows_launches_nothing'),
    ('M799', 'A: the guest policy the helper copies is at most MAX_POLICY bytes', _S2PL,
     '    if bytes.len() > MAX_POLICY {', '    if false && bytes.len() > MAX_POLICY {',
     'axon-fabric', _S2HT, 'a_policy_over_the_helpers_bound_launches_nothing'),
    ('M800', 'A: each psv input is spelled exactly <out root>/<inputs>/<leaf>', _S2PL,
     '        if parent.join(leaf).as_os_str() != p.as_os_str() {',
     '        if false && parent.join(leaf).as_os_str() != p.as_os_str() {',
     'axon-fabric', _S2HT, 'a_psv_input_spelled_another_way_launches_nothing'),
    ('M801', "A: each psv input names its fixed leaf", _S2PL,
     '        if p.file_name() != Some(OsStr::new(leaf)) {', '        if false && p.file_name() != Some(OsStr::new(leaf)) {',
     'axon-fabric', _S2HT, 'a_psv_input_naming_another_leaf_launches_nothing'),
    ('M802', 'A: the root launcher writes only into an out dir the helper created (mkdirat must create it)', _S2PL,
     '    if unsafe { libc::mkdirat(root, cn.as_ptr(), 0o700) } != 0 {',
     '    if unsafe { libc::mkdirat(root, cn.as_ptr(), 0o700) } != 0 && false {',
     'axon-fabric', _S2HT, 'a_root_owned_out_dir_that_already_exists_is_never_launched_into'),
    ('M803', 'A: the out dir opened is the one the helper created (owner re-check)', _S2PL,
     '    if st.st_uid != unsafe { libc::geteuid() } {\n        return Err("the out dir was replaced',
     '    if false {\n        return Err("the out dir was replaced',
     'axon-fabric', _S2HT, 'a_fabric_owned_out_dir_that_already_exists_is_never_launched_into'),
    ('M804', 'A: the out dir is not the psv inputs dir', _S2PL,
     '    if inputs_name == out_name {', '    if false && inputs_name == out_name {',
     'axon-fabric', _S2HT, 'an_out_dir_that_is_the_inputs_dir_launches_nothing'),
    ('M805', "A: the helper reads a psv input only from a directory the Fabric uid owns", _S2PL,
     '        if st.st_uid != c.fabric_uid {\n            return Err(format!(\n                "psv input {leaf} is owned',
     '        if false {\n            return Err(format!(\n                "psv input {leaf} is owned',
     'axon-fabric', _S2HT, 'a_root_owned_candidate_directory_is_never_read_by_the_helper'),
    ('M806', 'D6: a protected custodian answers only its request schema (production custodian)', _S2CU,
     '        if r.schema != REQUEST_SCHEMA {\n            return Err(format!("request schema is not',
     '        if false {\n            return Err(format!("request schema is not',
     'axon-fabric', _S2HT, 'a_protected_custodian_answers_no_request_of_another_schema'),
    ('M807', 'the preflight probe list refuses a relative path, as load does', _S2PH,
     '        if !p.is_absolute() {\n            return Err(bad(format!("{ptr} is not absolute")));',
     '        if false {\n            return Err(bad(format!("{ptr} is not absolute")));',
     'axon-fabric', _S2HT, 'the_probe_list_is_refused_for_a_host_config_load_refuses'),
    ('M808', 'the preflight probe list refuses a host config of another schema, as load does', _S2PH,
     '    if v["schema"] != PROTECTED_HOST_SCHEMA {\n        return Err(bad(format!("schema is not {PROTECTED_HOST_SCHEMA}")));\n    }\n    let path_at',
     '    if false {\n        return Err(bad(format!("schema is not {PROTECTED_HOST_SCHEMA}")));\n    }\n    let path_at',
     'axon-fabric', _S2HT, 'the_probe_list_is_refused_for_a_host_config_load_refuses'),
    ('M809', 'readiness: a failed git is never read as an empty answer (the git() primitive)', _S2RD,
     '    if !out.status.success() {\n        return Err(format!(\n            "{component}: git {} failed',
     '    if false {\n        return Err(format!(\n            "{component}: git {} failed',
     'axon-fabric', '--test readiness', 'a_failed_git_is_never_read_as_no_change'),
    ('M810', 'readiness: when git reports no change, the change set is recomputed from hash-checked objects', _S2RD,
     '    if outside.is_empty() {\n        let head = git(', '    if false {\n        let head = git(',
     'axon-fabric', '--test readiness', 'a_gitignored_input_is_not_certified'),
    ('M811', "psv: an observer that exits non-zero authorizes nothing, whatever it left", 'crates/axon-fabric/src/observer.rs',
     '    if !status.success() {\n        return Err(format!("observer exited',
     '    if false {\n        return Err(format!("observer exited',
     'axon-fabric', '--test psv_dispatch', 'every_defective_observation_launches_nothing_on_the_direct_route'),
    ('M812', "FIELD-ORIGIN (A88): the attestation is verified under the key verifier_key_id names", _S2RD,
     '        .find(|k| attestation::key_id_of_hex(k).as_deref() == Some(s("verifier_key_id")))',
     '        .find(|k| {\n            let att = &run["receipt_attestation"];\n            OpaqueRef::new(att["issuer_ref"].as_str().unwrap_or(""))\n                .is_ok_and(|i| attestation::verify(att, &i, &req, &rc, k).is_ok())\n        })',
     'axon-fabric', '--test readiness_launch', 'the_verifier_key_id_is_the_key_that_attested_the_run'),
    ('M813', "D6: a custodian's store is a real directory, never a symlink", _S2CU,
     '    if m.file_type().is_symlink() || !m.is_dir() {', '    if false && (m.file_type().is_symlink() || !m.is_dir()) {',
     'axon-fabric', _S2HT, 'a_protected_custodian_never_serves_from_a_symlinked_store'),
    ('M814', 'E: readiness certifies only the TOP of a standalone clone', _S2RD,
     '    if found != top {', '    if false && found != top {',
     'axon-fabric', '--test readiness', 'a_directory_inside_a_clone_is_not_certified'),
]
# ── C9 round 4 fix wave, ROWS2 wave 2, LOOP (M815-M859) ──
# The refusal sites of admission.rs and intake.rs that had no row: each is
# attacked through the production entry (admission::admit, activation through
# pointer::transition, intake::intake_episode) where it alone refuses; M852-M856
# are retired EQUIVALENT with executed four-cell records (EQUIV_RECORD below).
_R2LA = 'crates/axon-loop/src/admission.rs'
_R2LI = 'crates/axon-loop/src/intake.rs'
_R2EA = '--test evl_admission'
_R2IN = '--test intake'
_R2PC = '--test protected_class'
_R2BIND = 'each_admission_binding_refuses_its_own_forgery'
_R2JOIN = 'each_intake_join_refuses_its_own_defect'
MUTATIONS += [
 ('M815', "admission: the evaluation's scope is the frozen plan's", _R2LA,
  '    if eval.scope != plan.scope {', '    if false && eval.scope != plan.scope {', 'axon-loop', _R2EA, _R2BIND),
 ('M816', "admission: the evaluation belongs to the plan's experiment", _R2LA,
  '    if eval.experiment_id != plan.experiment_id {', '    if false && eval.experiment_id != plan.experiment_id {', 'axon-loop', _R2EA, _R2BIND),
 ('M817', "admission: the evaluation was made under the frozen plan", _R2LA,
  '    if eval.plan_ref != frozen.plan_ref {', '    if false && eval.plan_ref != frozen.plan_ref {', 'axon-loop', _R2EA, _R2BIND),
 ('M818', "admission: the evaluation was made under this freeze", _R2LA,
  '    if eval.freeze_seq != frozen.freeze_seq {', '    if false && eval.freeze_seq != frozen.freeze_seq {', 'axon-loop', _R2EA, _R2BIND),
 ('M819', "admission: exactly an incumbent and a candidate arm", _R2LA,
  '    if eval.arms.len() != 2 {', '    if false && eval.arms.len() != 2 {', 'axon-loop', _R2EA, _R2BIND),
 ('M820', "admission: the admitter is not a subject issuer of the evaluation", _R2LA,
  '    if eval.subject_issuers.contains(admitter) {', '    if false && eval.subject_issuers.contains(admitter) {', 'axon-loop', _R2EA, _R2BIND),
 ('M821', "admission: the admitter is not the evaluator", _R2LA,
  '    if &eval.evaluator_ref == admitter {', '    if false && &eval.evaluator_ref == admitter {', 'axon-loop', _R2EA, _R2BIND),
 ('M822', "admission: only a trusted admitter admits", _R2LA,
  '    if !admitters.contains(admitter) {', '    if false && !admitters.contains(admitter) {', 'axon-loop', _R2EA,
  'an_admitter_the_operator_does_not_trust_admits_nothing'),
 ('M823', "admission: a counted verdict records the verifier that authenticated it", _R2LA,
  '            let v = t.verification.as_ref().ok_or_else(|| {\n',
  '            let Some(v) = t.verification.as_ref() else { continue };\n            Some(()).ok_or_else(|| {\n',
  'axon-loop', _R2EA, 'a_counted_verdict_with_no_recorded_verifier_is_refused'),
 ('M824', "admission: a mechanism-test admission counts only mechanism-test evidence", _R2LA,
  '                if role != want {', '                if false && role != want {', 'axon-loop', _R2EA,
  'a_mechanism_test_admission_counts_no_confirmation_trial'),
 ('M825', "admission: an evaluation journalled before its freeze is refused", _R2LA,
  '    if eval_seq <= frozen.freeze_seq {', '    if false && eval_seq <= frozen.freeze_seq {', 'axon-loop', _R2EA,
  'an_evaluation_journalled_before_its_freeze_is_refused'),
 ('M826', "activation: the admission was journalled by `admit`", _R2LA,
  '    if tx.admission_event(adm_ref).is_none() {', '    if false && tx.admission_event(adm_ref).is_none() {', 'axon-loop', _R2EA,
  'activation_rests_only_on_a_journalled_admission_that_re_derives'),
 ('M827', "activation: the stored admission re-derives identically", _R2LA,
  '    if again != stored {', '    if false && again != stored {', 'axon-loop', _R2EA,
  'activation_rests_only_on_a_journalled_admission_that_re_derives'),
 ('M828', "activation: only an ACCEPT activates", _R2LA,
  '    if again.decision != Decision::Accept {', '    if false && again.decision != Decision::Accept {', 'axon-loop', _R2EA,
  'only_an_accepted_currently_authorized_admission_activates'),
 ('M829', "PSV-7: a protected arm's counters are its trials' outcomes", _R2LA,
  '        if arm.assigned != arm.trials.len() as u64\n            || arm.verified_pass != n(Outcome::VerifiedPass)\n            || arm.fail != n(Outcome::Fail)\n            || arm.unknown != n(Outcome::Unknown)\n            || arm.missing != missing\n        {',
  '        if false\n            && (arm.assigned != arm.trials.len() as u64\n                || arm.verified_pass != n(Outcome::VerifiedPass)\n                || arm.fail != n(Outcome::Fail)\n                || arm.unknown != n(Outcome::Unknown)\n                || arm.missing != missing)\n        {',
  'axon-loop', _R2PC, 'a_protected_arm_whose_counters_are_not_its_trials_is_refused'),
 ('M830', "intake: the context receipt is the one the episode names (EQUIVALENT: four-cell vs M857)", _R2LI,
  '    if ctx_ref != ep.context_ref {', '    if false && ctx_ref != ep.context_ref {', 'axon-loop', _R2IN, _R2JOIN),
 ('M831', "intake: only a bound (started) task is an episode", _R2LI,
  '    if ctx.expected != ctx.observed {', '    if false && ctx.expected != ctx.observed {', 'axon-loop', _R2IN, _R2JOIN),
 ('M832', "intake: a refused sidecar is not an episode", _R2LI,
  '    if ep.status == EpisodeStatus::Refused {', '    if false && ep.status == EpisodeStatus::Refused {', 'axon-loop', _R2IN, _R2JOIN),
 ('M834', "intake: an ack has exactly the ack's fields", _R2LI,
  '    if keys != want {\n        return Err(shape(format!(\n            "ack: fields',
  '    if false && keys != want {\n        return Err(shape(format!(\n            "ack: fields', 'axon-loop', _R2IN, _R2JOIN),
 ('M835', "intake: the ack records a pinned policy", _R2LI,
  '    if pin["state"] != "pinned" {', '    if false && pin["state"] != "pinned" {', 'axon-loop', _R2IN, _R2JOIN),
 ('M836', "intake: the ack pins the episode's policy", _R2LI,
  '    if pin["policy_ref"] != ep.policy_ref.as_str() || pin["policy_id"] != policy.policy_id.as_str()\n',
  '    if false && (pin["policy_ref"] != ep.policy_ref.as_str() || pin["policy_id"] != policy.policy_id.as_str())\n',
  'axon-loop', _R2IN, _R2JOIN),
 ('M837', "intake: the ack's shortlist is the stored policy's", _R2LI,
  '    if pin["shortlist"] != json!(shortlist) {', '    if false && pin["shortlist"] != json!(shortlist) {', 'axon-loop', _R2IN, _R2JOIN),
 ('M838', "intake: the ack's candidate list digests to its candidate_set_ref", _R2LI,
  '    if digest_value(cands)? != ep.candidate_set_ref {', '    if false && digest_value(cands)? != ep.candidate_set_ref {', 'axon-loop', _R2IN, _R2JOIN),
 ('M839', "intake: an ambiguous ack is refused, never guessed", _R2LI,
  '        many => Err(refused(format!(\n            "ambiguous ack',
  '        [(_, t), ..] => Ok(t),\n        #[allow(unreachable_patterns)]\n        many => Err(refused(format!(\n            "ambiguous ack',
  'axon-loop', _R2IN, _R2JOIN),
 ('M840', "intake: the projection is the one the episode names", _R2LI,
  '    if &r != want {', '    if false && &r != want {', 'axon-loop', _R2IN, _R2JOIN),
 ('M841', "intake: the projection maps the episode's policy", _R2LI,
  '    if proj.sidecar_policy_ref != ep.policy_ref {', '    if false && proj.sidecar_policy_ref != ep.policy_ref {', 'axon-loop', _R2IN, _R2JOIN),
 ('M842', "intake: verification evidence the episode does not cite is refused", _R2LI,
  '        if input.verification_request.is_some()\n',
  '        if false && input.verification_request.is_some()\n', 'axon-loop', _R2IN, _R2JOIN),
 ('M843', "intake: the canonical episode is the one the sidecar names", _R2LI,
  '    if r != ep.source_episode_ref {', '    if false && r != ep.source_episode_ref {', 'axon-loop', _R2IN, _R2JOIN),
 ('M844', "intake: a known canonical spend is never dropped", _R2LI,
  '        (Some(mc), None) => {\n            return Err(',
  '        (Some(_), None) => {}\n        #[allow(unreachable_patterns)]\n        (Some(mc), None) => {\n            return Err(',
  'axon-loop', _R2IN, _R2JOIN),
 ('M845', "intake: a cost the canonical episode does not know is never substituted", _R2LI,
  '        (None, Some(c)) => {\n            return Err(',
  '        (None, Some(_)) => {}\n        #[allow(unreachable_patterns)]\n        (None, Some(c)) => {\n            return Err(',
  'axon-loop', _R2IN, _R2JOIN),
 ('M846', "intake: the sidecar's cost is the canonical spend under the round-up rule", _R2LI,
  '            if c != want {', '            if false && c != want {', 'axon-loop', _R2IN, _R2JOIN),
 ('M847', "intake: a MiCode not-produced marker is never a policy reference", _R2LI,
  '        if *named == marker {', '        if false && *named == marker {', 'axon-loop', _R2IN,
  'a_not_produced_marker_is_never_a_policy_reference'),
 ('M848', "intake: a revoked policy records no episode", _R2LI,
  '    if tx.is_revoked(&ep.scope, &ep.policy_ref) {', '    if false && tx.is_revoked(&ep.scope, &ep.policy_ref) {', 'axon-loop', _R2IN,
  'an_episode_of_a_revoked_policy_is_refused'),
 ('M849', "intake: one trial identity, one set of bytes", _R2LI,
  '            if intake.scope == ep.scope && intake.identity == ep.identity {',
  '            if false && intake.scope == ep.scope && intake.identity == ep.identity {', 'axon-loop', _R2IN,
  'a_second_episode_for_a_recorded_trial_is_a_conflict'),
 ('M850', "intake: the receipt records exactly one check suite version", _R2LI,
  '        [one] => *one,\n        _ => {',
  '        [one] => *one,\n        [first, ..] => *first,\n        _ => {', 'axon-loop', _R2IN,
  'a_receipt_recording_two_suite_versions_decides_nothing'),
 ('M851', "intake: the check is this attempt's Fabric operation (the `same` refusal)", _R2LI,
  '    if !same {', '    if false && !same {', 'axon-loop', _R2IN, 'each_verification_rule_is_load_bearing_on_its_own'),
 ('M857', "EVL: a delivered trial's context is the one its episode names (bind_episode's context binding)",
  'crates/axon-loop-contracts/src/checks.rs',
  '    if episode.policy_ref != digest(policy)? || episode.context_ref != digest(ctx)? {',
  '    if episode.policy_ref != digest(policy)? {',
  'axon-loop', _R2EA, 'a_trial_delivered_with_a_context_other_than_its_episodes_counts_nothing'),
 ('M852', "PSV-7: a protected context re-verifies under an observer the operator trusts now (EQUIVALENT: four-cell vs M122+M104)", _R2LA,
  '    if !config.observers().contains(&who) {', '    if false && !config.observers().contains(&who) {', 'axon-loop', _R2PC,
  'a_context_observer_the_operator_withdrew_counts_nothing_at_activation'),
 ('M853', "intake: check_ack refuses an ack of another schema (EQUIVALENT pair with M854)", _R2LI,
  '    if obj["schema"] != ACK_SCHEMA {', '    if false && obj["schema"] != ACK_SCHEMA {', 'axon-loop', _R2IN,
  'an_ack_of_another_schema_or_view_is_never_joined'),
 ('M854', "intake: select_ack passes over an ack of another schema (EQUIVALENT pair with M853)", _R2LI,
  '        if v.get("schema").and_then(Value::as_str) != Some(ACK_SCHEMA) {',
  '        if false && v.get("schema").and_then(Value::as_str) != Some(ACK_SCHEMA) {', 'axon-loop', _R2IN,
  'an_ack_of_another_schema_or_view_is_never_joined'),
 ('M855', "intake: check_ack refuses an ack over another candidate view (EQUIVALENT pair with M856)", _R2LI,
  '    if obj["candidate_set_ref"] != ep.candidate_set_ref.as_str() {',
  '    if false && obj["candidate_set_ref"] != ep.candidate_set_ref.as_str() {', 'axon-loop', _R2IN,
  'an_ack_of_another_schema_or_view_is_never_joined'),
 ('M856', "intake: select_ack passes over an ack over another candidate view (EQUIVALENT pair with M855)", _R2LI,
  '        if pin_ref != Some(ep.policy_ref.as_str()) || csr != Some(ep.candidate_set_ref.as_str()) {',
  '        if pin_ref != Some(ep.policy_ref.as_str()) {\n            let _ = csr;', 'axon-loop', _R2IN,
  'an_ack_of_another_schema_or_view_is_never_joined'),
]

# The four-cell retirements of the block above (executed with
# scripts/v022_paired_disable.py; never counted killed).
EQUIV_RECORD["M852"] = {
    "property": "a counted protected context rests on an observer the operator trusts at the derivation",
    "subsumed_by": ["M122", "M104"], "killer": "joint:M852+M122+M104",
    "all_paths": "reverify_protected has one caller, derive, which calls it for every counted trial of a "
                 "protected evaluation and, with no early Ok after it, then runs (a) the any-class loop "
                 "refusing a trial whose context_observer_ref is not in config.observers() (M122) and "
                 "(b) the protected loop refusing a trial whose context_signed_by issuer is not in "
                 "config.observers() (M104). reverify_protected itself refuses unless "
                 "t.context_observer_ref == Some(who) (M362), where `who` is the observer M852 checks, "
                 "so on every path a `who` outside config.observers() is also a context_observer_ref "
                 "outside it (M122); M104 refuses the same trial through its signer attribution"}
EQUIV_RECORD["M830"] = {
    "property": "intake records an episode only with the context receipt it names",
    "subsumed_by": ["M857"], "killer": "joint:M830+M857",
    "all_paths": "intake_episode is the only caller; after this check it always calls bind_episode "
                 "with the same episode and context (no early return between them but refusals), and "
                 "bind_episode refuses `episode.context_ref != digest(ctx)` (M857), the identical "
                 "predicate: ctx_ref is digest(&ctx) of the same parsed context"}
EQUIV_RECORD["M853"] = {
    "property": "an ack of another schema never joins an episode",
    "subsumed_by": ["M854"], "killer": "joint:M853+M854",
    "all_paths": "check_ack has one caller, intake_episode, which hands it only the text select_ack "
                 "returns; select_ack passes over every text whose schema is not ACK_SCHEMA (M854), so "
                 "the text check_ack sees always has that schema"}
EQUIV_RECORD["M854"] = {
    "property": "an ack of another schema never joins an episode",
    "subsumed_by": ["M853"], "killer": "joint:M853+M854",
    "all_paths": "select_ack's only caller hands its result to check_ack, which refuses any schema "
                 "but ACK_SCHEMA (M853) before anything is recorded"}
EQUIV_RECORD["M855"] = {
    "property": "an ack over another candidate view never joins an episode",
    "subsumed_by": ["M856"], "killer": "joint:M855+M856",
    "all_paths": "check_ack has one caller, intake_episode, which hands it only the text select_ack "
                 "returns; select_ack passes over every ack whose candidate_set_ref is not the "
                 "episode's (M856), so the ack check_ack sees always names the episode's view"}
EQUIV_RECORD["M856"] = {
    "property": "an ack over another candidate view never joins an episode",
    "subsumed_by": ["M855"], "killer": "joint:M855+M856",
    "all_paths": "select_ack's only caller hands its result to check_ack, which refuses an ack whose "
                 "candidate_set_ref is not the episode's (M855) before anything is recorded"}
EQUIVALENT_DID |= {"M830", "M852", "M853", "M854", "M855", "M856"}
RETIRED |= {"M830", "M852", "M853", "M854", "M855", "M856"}

BINDING_IDS = {f"M{n}" for n in range(101, 137)}
# Every id range the PSV rounds allocate (C9 round 1 uses up to M399; round
# 1b allocates M400-M499, round 2 M500-M519). An id outside every scope would silently fall into
# g01.
# C9 round 4 fix wave, rows3 (amendment 59): a cell's build is judged by its
# own cargo invocation, never by compiler text a TEST printed (axon-cortex's
# compile_fail doctests under --show-output made M58's consumer baseline read
# CONSUMER_BASELINE_BROKEN), and every cell that did not pass keeps its whole
# output, compile errors included. Judged through the real harnesses on a
# miniature workspace (crates/axon-core/tests/harness_integrity.rs).
_HN_MUT = 'a_mutation_run_does_not_take_a_tests_nested_build_output_for_a_compile_error'
_HN_PD = 'a_paired_disable_run_does_not_take_a_tests_nested_build_output_for_a_compile_error'
_HK_MUT = 'a_mutation_baseline_that_does_not_build_keeps_its_output'
_HK_PD = 'a_paired_disable_cell_that_does_not_build_keeps_its_output'
_HR_MUT = 'a_mutation_run_restores_the_interpreter_after_every_row'
_TEXT_RULE = '    if "could not compile" in out or "error[E" in out:\n'
MUTATIONS += [
    ('M880', 'EVIDENCE (rows3): a mutation cell is a compile error only when its own build fails, not when a test prints compiler text',
     'scripts/v022_g01_mutations.py',
     "    # The tests are built: compiler text in this output is the TEST's (a\n",
     _TEXT_RULE + '        return "compile_error", out\n'
     "    # The tests are built: compiler text in this output is the TEST's (a\n",
     'axon-core', _HI2, _HN_MUT),
    ('M881', 'EVIDENCE (rows3): a paired-disable cell is a broken build only when its own build fails',
     'scripts/v022_paired_disable.py',
     '    passed = r.returncode == 0 and "1 passed" in out\n',
     _TEXT_RULE + '        return None, out\n    passed = r.returncode == 0 and "1 passed" in out\n',
     'axon-core', _HI2, _HN_PD),
    ('M882', 'EVIDENCE (rows3): a full-suite or consumer cell is a broken build only when its own build fails (M58 CONSUMER_BASELINE_BROKEN)',
     'scripts/v022_paired_disable.py',
     "    # The tests are built: compiler text in this output is a TEST's (a nested\n",
     _TEXT_RULE + '        return None, out\n'
     "    # The tests are built: compiler text in this output is a TEST's (a nested\n",
     'axon-core', _HI2, _HN_PD),
    ('M883', 'EVIDENCE (rows3): a mutation baseline that did not pass keeps its whole output',
     'scripts/v022_g01_mutations.py',
     '                              else keep_output(f"baseline-{mid}", b_out))\n',
     '                              else None)\n',
     'axon-core', _HI2, _HK_MUT),
    ('M884', 'EVIDENCE (rows3): a mutated cell that did not pass keeps its whole output',
     'scripts/v022_g01_mutations.py',
     '                    kept = keep_output(f"cell-{mid}", out)\n',
     '                    kept = None\n',
     'axon-core', _HI2, _HK_MUT),
    ('M885', "EVIDENCE (rows3): a paired-disable cell whose test did not build keeps the build's output",
     'scripts/v022_paired_disable.py',
     '        mut.keep_output(f"pd-cell-build-{pkg}", bout)\n',
     '        pass\n',
     'axon-core', _HI2, _HK_PD),
    ('M886', "EVIDENCE (rows3): a full-suite cell that did not build keeps the build's output (it returned before the keep)",
     'scripts/v022_paired_disable.py',
     '        mut.keep_output(f"pd-suite-build-{pkg}", bout)\n',
     '        pass\n',
     'axon-core', _HI2, _HK_PD),
    ('M887', 'EVIDENCE (rows3): a paired-disable cell whose test failed keeps its whole output',
     'scripts/v022_paired_disable.py',
     "        mut.keep_output(f\"pd-cell-{pkg}-{test.split('::')[-1]}\", out)\n",
     '        pass\n',
     'axon-core', _HI2, _HN_PD),
    ('M888', "EQUIVALENCE (rows3): every row ends on the run's interpreter, byte-compared, or fails itself (49eb3765 shard 1)",
     'scripts/v022_g01_mutations.py', '        unrestored = restore_interpreter(axon_bin, toolchain["axon_bin_sha256"])\n', '        unrestored = None\n', 'axon-core', _HI2, _HR_MUT),
    ('M889', "EQUIVALENCE (rows3): restoring the interpreter re-runs axon-core's build script on the restored tree",
     'scripts/v022_g01_mutations.py', '            rerun_core_build_script()\n        built = build_interpreter()\n        if built.returncode != 0:\n            return f"the restored tree\'s interpreter did not build', '            pass\n        built = build_interpreter()\n        if built.returncode != 0:\n            return f"the restored tree\'s interpreter did not build', 'axon-core', _HI2, _HR_MUT),
    ('M890', "EQUIVALENCE (rows3): the restored interpreter is compared to the run's, never assumed",
     'scripts/v022_g01_mutations.py', '        if os.path.exists(path) and sha(path) == expected:\n            return None', '        if True:\n            return None', 'axon-core', _HI2, 'a_restore_that_leaves_a_foreign_interpreter_says_so'),
    ('M891', "EVIDENCE (rows3): a paired-disable cell's changed prerequisite is rebuilt and byte-compared",
     'scripts/v022_paired_disable.py',
     '        if what == "axon" or all(\n',
     '        if True or all(\n',
     'axon-core', _HI2, 'a_paired_disable_cell_restores_a_prerequisite_it_changed'),
    ('M892', 'EQUIVALENCE (6d): a cell that ran a building script on a mutated tree is scrubbed after',
     'scripts/v022_paired_disable.py',
     '    if spawned and any(e[0].startswith("crates/") for e in edits):\n',
     '    if False and any(e[0].startswith("crates/") for e in edits):\n',
     'axon-core', _HI2, _PC),
    ('M893', 'EQUIVALENCE (6d): a full-suite cell scrubs when ANY suite it ran spawns building scripts',
     'scripts/v022_paired_disable.py',
     '                                      for p in [pkg, owner_crate, *consumers]))\n',
     '                                      for p in []))\n',
     'axon-core', _HI2, _PC),
    ('M894', 'EQUIVALENCE (rows3): a git-ignored output a test wrote into a crate is not a source a named binary is stale against',
     _SP, '    if !o.status.success() {\n        return None;\n    }\n    Some(\n        o.stdout', '    if true || !o.status.success() {\n        return None;\n    }\n    Some(\n        o.stdout',
     'axon-core', _HB2, 'an_ignored_output_written_into_a_crate_is_not_a_source'),
]

# ── C9 round 4b, CORE2 workstream (M920-M939, M1140-M1179; amendment 60;
# matrix A86) ── PSV-1: the cast's notion of "the same type" is the key a
# method call dispatches on (`Value::type_name`), and a call through a local
# never resolves the name elsewhere. M920-M931 are the round-4b PSV-1 and
# EQUIVALENCE blockers; M932-M939 and M1140-M1156 are the EQUIVALENCE audit
# of every refusal arm of the cast, its call sites and the seal edges (each
# reached ALONE by its own attack; the arms with no consequence are exempt in
# scripts/v022_refusal_coverage.py with the reason).
_EV = 'interp::tests::every_declared_type_refuses_a_value_of_another_type'
_RA = 'interp::tests::the_remaining_cast_arms_refuse_a_value_of_another_type'
_KIND = 'return kind_ok(matches!(v, Value::{}))'
MUTATIONS += [
    ('M920', 'PSV-1 (A86): at a declared i64 a value of another integer width is refused (width, not kind)', _CC,
     '"i64" | "isize" | "usize" => return kind_ok(matches!(v, Value::Int(_))),',
     '"i64" | "isize" | "usize" => return kind_ok(matches!(v, Value::Int(_) | Value::SizedInt { .. })),',
     'axon-core', _CL, _T4 + 'a_value_of_another_integer_width_never_crosses_a_declared_integer'),
    ('M921', 'PSV-1 (A86): at a declared fixed width a value of another fixed width is refused', _CC,
     '                    Value::SizedInt { ty: w, .. } if *w == width => Ok(()),',
     '                    Value::SizedInt { .. } => Ok(()),',
     'axon-core', _CL, _T4 + 'a_value_of_another_fixed_width_never_crosses_a_declared_fixed_width'),
    ('M922', 'PSV-1 (A86): an i64 at a declared fixed width is converted to that width (it dispatches as declared)', _CC,
     '                        *v = Value::SizedInt { val, ty: width };',
     '                        let _ = (val, width);',
     'axon-core', _CL, _T4 + 'an_i64_at_a_declared_fixed_width_takes_the_width'),
    ('M923', 'PSV-1 (A86): only a closure crosses a declared fn type (no named fn is a value)', _CC,
     '                _ => Err(mismatch(ty, v)),\n            },\n            T::Chan(elem) => match v {',
     '                _ => Ok(()),\n            },\n            T::Chan(elem) => match v {',
     'axon-core', _CL, _T4 + 'a_non_closure_never_crosses_a_declared_fn_type'),
    ('M924', 'PSV-1 (A86): a call through a local goes through its value, never to a builtin or fn of that name', _CE,
     '            if let Some(c) = env.get(name) {\n                let c = c.clone();\n                return self.call_local_closure(c, argv);',
     '            if let Some(c @ Value::Closure { .. }) = env.get(name) {\n                let c = c.clone();\n                return self.call_local_closure(c, argv);',
     'axon-core', _CL, _T4 + 'a_call_through_a_local_never_resolves_the_name_elsewhere'),
    ('M925', 'PSV-1 (A86): only a dict crosses a declared Dict', _CC,
     '"Dict" => return kind_ok(matches!(v, Value::Dict(_))),',
     '"Dict" => return kind_ok(true),',
     'axon-core', _CL, _EV),
    ('M926', 'PSV-1 (A86): a soft wrapper at a declared plain type is replaced by its inner value', _CC,
     '                    *v = inner;',
     '                    let _ = &inner;',
     'axon-core', _CL, _T4 + 'a_soft_wrapper_takes_the_declared_plain_type'),
    ('M927', 'PSV-1 (A86): at a declared soft type only the wrapper of that name crosses', _CC,
     '                if name != base {',
     '                if false {',
     'axon-core', _CL, _T4 + 'a_declared_soft_type_casts_its_wrapper_and_its_inner_value'),
    ('M928', "PSV-1 (A86): a declared soft type casts its wrapper's inner value", _CC,
     '                    (Some(t), Some(x)) => self.cast_at(x, t, cx, d),',
     '                    (Some(_), Some(_)) => Ok(()),',
     'axon-core', _CL, _T4 + 'a_declared_soft_type_casts_its_wrapper_and_its_inner_value'),
    ('M929', 'PSV-1 (A86): a plain value at a declared soft type is cast to its inner type', _CC,
     '                Some(t) => self.cast_at(v, t, cx, d),\n                None => Err(mismatch(ty, v)),',
     '                Some(_) => Ok(()),\n                None => Err(mismatch(ty, v)),',
     'axon-core', _CL, _T4 + 'a_declared_soft_type_casts_its_wrapper_and_its_inner_value'),
    ('M930', 'PSV-1 (A86): a bare soft name takes only its wrapper', _CC,
     '                Some(t) => self.cast_at(v, t, cx, d),\n                None => Err(mismatch(ty, v)),',
     '                Some(t) => self.cast_at(v, t, cx, d),\n                None => Ok(()),',
     'axon-core', _CL, _T4 + 'a_declared_soft_type_casts_its_wrapper_and_its_inner_value'),
    ('M931', 'PSV-1 (A86): an enum value is cast to a declared enum type by name (round-4b EQUIVALENCE blocker)', _CC,
     '            if enum_name != n {',
     '            if false {',
     'axon-core', _CL, _T4 + 'a_confused_enum_never_crosses_as_another_enum'),
    ('M932', 'PSV-1 (A86): only an array crosses a declared array type', _CC,
     '                    Ok(())\n                }\n                _ => Err(mismatch(ty, v)),\n            },\n            T::Tuple(ts) => match v {',
     '                    Ok(())\n                }\n                _ => Ok(()),\n            },\n            T::Tuple(ts) => match v {',
     'axon-core', _CL, _EV),
    ('M933', 'PSV-1 (A86): only a tuple (of the declared arity) crosses a declared tuple type', _CC,
     '                _ => Err(mismatch(ty, v)),\n            },\n            T::Union(ts) => {',
     '                _ => Ok(()),\n            },\n            T::Union(ts) => {',
     'axon-core', _CL, _EV),
    ('M934', 'PSV-1 (A86): a value crosses a declared union only as one of its members', _CC,
     '                Err(mismatch(ty, v))\n            }\n            T::DynTrait(tr) => self.check_impl(v, tr),',
     '                Ok(())\n            }\n            T::DynTrait(tr) => self.check_impl(v, tr),',
     'axon-core', _CL, _EV),
    ('M935', 'PSV-1 (A86): only a channel crosses a declared Chan type', _CC,
     '                Value::Chan(q) => self.stamp_chan(q, elem, cx),\n                _ => Err(mismatch(ty, v)),',
     '                Value::Chan(q) => self.stamp_chan(q, elem, cx),\n                _ => Ok(()),',
     'axon-core', _CL, _EV),
    ('M936', 'PSV-1 (A86): only Some/None cross a declared Option type', _CC,
     '            Value::Some(x) => self.cast_at(x, inner, cx, d),\n            _ => Err(mismatch(ty, v)),',
     '            Value::Some(x) => self.cast_at(x, inner, cx, d),\n            _ => Ok(()),',
     'axon-core', _CL, _EV),
    ('M937', 'PSV-1 (A86): only Ok/Err cross a declared Result type', _CC,
     '            Value::Err(x) => self.cast_at(x, err, cx, d),\n            _ => Err(mismatch(ty, v)),',
     '            Value::Err(x) => self.cast_at(x, err, cx, d),\n            _ => Ok(()),',
     'axon-core', _CL, _EV),
    ('M938', 'PSV-1 (A86): only a float crosses a declared f64/f32', _CC,
     '"f64" | "f32" => ' + _KIND.format('Float(_)') + ',',
     '"f64" | "f32" => return kind_ok(true),',
     'axon-core', _CL, _EV),
    ('M939', 'PSV-1 (A86): only a bool crosses a declared bool', _CC,
     '"bool" => ' + _KIND.format('Bool(_)') + ',',
     '"bool" => return kind_ok(true),',
     'axon-core', _CL, _EV),
    ('M1140', 'PSV-1 (A86): only a str crosses a declared str', _CC,
     '"str" | "String" => ' + _KIND.format('Str(_)') + ',',
     '"str" | "String" => return kind_ok(true),',
     'axon-core', _CL, _EV),
    ('M1141', 'PSV-1 (A86): only () crosses a declared ()', _CC,
     '"()" => ' + _KIND.format('Unit') + ',',
     '"()" => return kind_ok(true),',
     'axon-core', _CL, _EV),
    ('M1142', 'PSV-1 (A86): only a Decimal crosses a declared Decimal', _CC,
     '"Decimal" => ' + _KIND.format('Decimal(_)') + ',',
     '"Decimal" => return kind_ok(true),',
     'axon-core', _CL, _EV),
    ('M1143', 'PSV-1 (A86): only a struct crosses a declared struct type', _CC,
     '            let Value::Struct { name, fields } = v else {\n                return Err(mismatch(ty, v));',
     '            let Value::Struct { name, fields } = v else {\n                return Ok(());',
     'axon-core', _CL, _EV),
    ('M1144', 'PSV-1 (A86): only an enum value crosses a declared enum type', _CC,
     '            } = v\n            else {\n                return Err(mismatch(ty, v));',
     '            } = v\n            else {\n                return Ok(());',
     'axon-core', _CL, _EV),
    ('M1145', 'PSV-1 (A86): a value at a type parameter the arguments bound is cast to the binding', _CC,
     # C9 r4c (psv1, amendment 72): re-anchored; the binding is read in binding_cx().
     '                self.cast_at(v, &t, &cx.binding_cx(), d)?;',
     '                let _ = (&t, d);',
     'axon-core', _CL, _RA),
    ('M1146', 'PSV-1 (A86): a value at a declared dyn Trait implements the trait', _CC,
     '            T::DynTrait(tr) => self.check_impl(v, tr),',
     '            T::DynTrait(_) => Ok(()),',
     'axon-core', _CL, _RA),
    ('M1147', "PSV-1 (A86): a declared enum type casts the variant's fields", _CC,
     '                    self.cast_at(fv, &tf.ty, &fcx, d)\n                        .map_err(|e| format!("field `{}` of `{n}::{variant}`: {e}", tf.name))?;',
     '                    self.cast_at(fv, &any(), &fcx, d)\n                        .map_err(|e| format!("field `{}` of `{n}::{variant}`: {e}", tf.name))?;',
     'axon-core', _CL, _RA),
    ('M1148', 'PSV-1 (A86): a channel crossing Chan<T> casts the values already queued', _CC,
     # C9 r4c (psv1, amendment 72): re-anchored; the queued values keep the crossing's strictness.
     '        for x in q.borrow_mut().iter_mut() {\n            self.cast(x, &elem, &ecx.strict(cx.strict))?;',
     '        for x in q.borrow_mut().iter_mut() {\n            let _ = (x, &ecx);',
     'axon-core', _CL, _RA),
    ('M1149', "PSV-1 (A86): a declared refinement casts to its base type", _CC,
     '            return self.cast_at(v, base, cx, d);',
     '            let _ = base;\n            return Ok(());',
     'axon-core', _CL, _RA),
    ('M1150', "PSV-1 (A86): check_impl refuses a value whose type does not implement the trait", _CC,
     '            && !self.trait_impls.contains(&(tn.clone(), tr.to_string()))',
     '            && false',
     'axon-core', _CL, _T4 + 'a_type_parameters_trait_bound_is_cast'),
    ('M1151', 'PSV-1 (A86): a scalar kind check refuses when the kind differs (kind_ok)', _CC,
     '        let kind_ok = |ok: bool| if ok { Ok(()) } else { Err(mismatch(ty, v)) };',
     '        let kind_ok = |ok: bool| if ok || true { Ok(()) } else { Err(mismatch(ty, v)) };',
     'axon-core', _CL, _T4 + 'a_confused_scalar_never_crosses_a_declared_return'),
    ('M1152', 'PSV-1 (A86): a send is refused when the value does not cast to the channel element type', _CC,
     # C9 r4c (psv1, amendment 72): re-anchored; each element type is cast with its strictness.
     '            if let Err(why) = self.cast(v, &c.ret, &c.cx.strict(strict)) {',
     '            if let Err(why) = Ok::<(), String>(()) {',
     'axon-core', _CL, _T4 + 'a_confused_value_is_never_sent_on_a_declared_channel'),
    ('M1153', "PSV-1 (A86): a closure call is refused when an argument does not cast to a fn type it crossed", _CC,
     # C9 r4c (psv1, amendment 72): re-anchored; each layer is cast with its strictness.
     '                if let Err(why) = self.cast(a, t, &k.cx.strict(strict)) {',
     '                if let Err(why) = Ok::<(), String>(()) {',
     'axon-core', _CL, _T4 + 'a_closures_confused_argument_never_crosses_its_declared_type'),
    ('M1154', "PSV-1 (A86): a closure's result is refused when it does not cast to a fn type it crossed", _CC,
     '            if let Err(why) = self.cast(v, &k.ret, &k.cx.strict(strict)) {',
     '            if let Err(why) = Ok::<(), String>(()) {',
     'axon-core', _CL, _T4 + 'a_closures_confused_result_never_crosses_its_declared_type'),
    ('M1155', "PSV-1 (A86): a fn's result is cast to its declared return type (the return site)", _CI,
     '                Some(rt) => self.cast(&mut result, rt, &cx.strict(crossing)).err(),',
     '                Some(_) => None::<String>,',
     'axon-core', _CL, _T4 + 'a_confused_scalar_never_crosses_a_declared_return'),
    ('M1157', 'PSV-1 (A86): at a declared fixed width a non-integer is refused', _CC,
     '                    _ => Err(mismatch(ty, v)),\n                };\n            }\n            // One runtime float',
     '                    _ => Ok(()),\n                };\n            }\n            // One runtime float',
     'axon-core', _CL, _RA),
    ('M1156', 'interpreter: the CALL edge refuses an operator fn in a sealed frame (seal_call)', _CI,
     '        if self.seal.active && self.frame_sealed.get() && !self.fn_is_sealed(f) {',
     '        if false && self.seal.active && self.frame_sealed.get() && !self.fn_is_sealed(f) {',
     'axon-core', _CL, _T4 + 'runtime_sealing_holds_without_the_static_check'),
]

# ── C9 round 4b, harness3 workstream (M1180-M1198; amendment 63) ──
# FIELD-ORIGIN major-adjacents on the guest image's build provenance:
# (1) cargo's effective config was checked once, at begin, while cargo re-reads
# it per invocation from the clone under /var/tmp; (2) the judge ignored the
# recorded cargo args and RUSTFLAGS; (3) vmlinux and rootfs.sqfs were made
# outside the controlled environment. scripts/guest_build_env.py now builds on
# a private copy under a builder-only parent, holds the config to begin's
# before AND after every invocation, runs only its table's invocations, builds
# the kernel and the rootfs itself, and image_problems judges the whole image.
_GBT = '--test guest_build_env'
_FMT = '--test freeze_manifest'
_FCO = 'a_guest_component_built_outside_the_controlled_environment_does_not_freeze'
MUTATIONS += [
    ('M1180', "FIELD-ORIGIN (4b-1): the guest build's parent must have no ancestor another uid can write", _GE,
     '    if why:\n        fail(f"the guest build\'s parent directory is not private to the builder: {why}")\n',
     '    if False and why:\n        fail(f"the guest build\'s parent directory is not private to the builder: {why}")\n',
     'axon-fabric', _GBT, 'a_guest_build_under_a_directory_another_uid_can_write_is_refused'),
    ('M1181', "FIELD-ORIGIN (4b-1): cargo runs on the build's private copy of the tree, never in the clone", _GE,
     '    r = subprocess.run(as_build_uid([rec["toolchain"]["cargo"], *args]), env=env, cwd=rec["src_dir"])\n',
     '    r = subprocess.run(as_build_uid([rec["toolchain"]["cargo"], *args]), env=env, cwd=ROOT)\n',
     'axon-fabric', _GBT, 'an_ancestor_config_written_after_begin_does_not_reach_the_guest_build'),
    ('M1182', "FIELD-ORIGIN (4b-1): cargo's effective config is held to begin's BEFORE every invocation", _GE,
     '    if before != config_at_begin(rec):\n', '    if False:\n',
     'axon-fabric', _GBT, 'a_cargo_config_planted_beside_the_private_copy_after_begin_is_refused'),
    ('M1183', "FIELD-ORIGIN (4b-1): cargo's effective config is held to begin's AFTER every invocation", _GE,
     '    if after != config_at_begin(rec):\n', '    if False:\n',
     'axon-fabric', _GBT, 'a_cargo_config_written_during_a_guest_build_step_fails_that_step'),
    ('M1184', 'FIELD-ORIGIN (4b-2): a guest cargo step runs only an invocation of the table, exactly (args and RUSTFLAGS)', _GE,
     '    if name is None:\n        kind = "host"', '    if False:\n        kind = "host"',
     'axon-fabric', _GBT, 'a_guest_cargo_step_with_extra_flags_or_args_is_refused'),
    ('M1185', "EVIDENCE (4b-2): the judge holds every recorded cargo invocation to its table entry", _GE,
     '    if b.get("name") is None or invocation(b.get("args"), b.get("rustflags"), table_of(rec)) != b.get("name"):\n',
     '    if False:\n',
     'axon-fabric', _FMT, 'a_recorded_guest_build_with_other_args_or_flags_does_not_freeze'),
    ('M1186', "EVIDENCE (4b-1): the judge requires each invocation's config check before AND after, equal to begin's", _GE,
     '    if b.get("config_before") != config_at_begin(rec) or b.get("config_after") != config_at_begin(rec):\n',
     '    if False:\n',
     'axon-fabric', _FMT, 'a_guest_build_record_without_a_config_check_around_each_invocation_does_not_freeze'),
    ('M1187', 'EVIDENCE (4b-2): the image\'s binaries are exactly the protected builds, in order', _GE,
     '    if names != [n for n, _, _ in PROTECTED_BUILDS]:\n', '    if False:\n',
     'axon-fabric', _FMT, 'a_guest_image_whose_binaries_were_not_exactly_the_protected_builds_does_not_freeze'),
    ('M1188', "EVIDENCE (4b-2): the recorded toolchain is the pinned channel's, with the linker's identity", _GE,
     '    if (chan is None or tc.get("channel") != chan or os.path.dirname(str(tc["rustc"])) != tdir\n',
     '    if False and (chan is None or tc.get("channel") != chan or os.path.dirname(str(tc["rustc"])) != tdir\n',
     'axon-fabric', _FMT, 'a_guest_build_record_not_of_the_pinned_toolchain_does_not_freeze'),
    ('M1189', 'FIELD-ORIGIN (4b-3): the rootfs installs only the bytes the controlled build recorded', _GE,
     '        if got != rec["artifacts"].get(name):\n', '        if False:\n',
     'axon-fabric', _GBT, 'a_rootfs_from_a_binary_the_controlled_build_did_not_produce_is_refused'),
    ('M1190', 'EVIDENCE (4b-1): the judge requires cargo to have run on a private copy under a builder-only parent', _GE,
     '    if rec.get("src_dir") != os.path.join(base, "src") or os.path.dirname(base) != rec.get("build_parent") or why:\n',
     '    if False:\n',
     'axon-fabric', _FMT, 'a_guest_build_record_not_made_in_a_private_copy_does_not_freeze'),
    ('M1191', "FIELD-ORIGIN (4b-3): the rootfs is made by /usr/bin's mksquashfs, never the caller's PATH's", _GE,
     '    mks = host_tool("mksquashfs")\n', '    mks = shutil.which("mksquashfs") or host_tool("mksquashfs")\n',
     'axon-fabric', _GBT, 'a_callers_mksquashfs_or_busybox_does_not_make_the_rootfs'),
    ('M1192', "FIELD-ORIGIN (4b-3): the kernel's make runs in a constructed environment (no caller KCFLAGS/CC/CROSS_COMPILE)", _GE,
     '        kenv = kernel_env(base)\n', '        kenv = dict(os.environ, **kernel_env(base))\n',
     'axon-fabric', _GBT, 'a_callers_kcflags_cc_or_path_do_not_reach_the_kernel_build'),
    ('M1193', 'FIELD-ORIGIN (4b-3): a pinned input (kernel tarball, config, overlay, busybox) is verified as the copy used', _GE,
     '    if got != want:\n        fail(f"{label} sha256 mismatch', '    if False:\n        fail(f"{label} sha256 mismatch',
     'axon-fabric', _GBT, 'a_kernel_from_sources_that_are_not_the_pinned_ones_is_refused'),
    ('M1194', 'EVIDENCE (4b-3): the freeze applies the whole-image judge', _FZ,
     '    if outside:\n', '    if False and outside:\n',
     'axon-fabric', _FMT, _FCO),
    ('M1195', "EVIDENCE (4b-3): vmlinux is the bytes a controlled kernel build made from the manifest's pins", _GE,
     '    if (not isinstance(k, dict) or k.get("schema") != KERNEL_SCHEMA or k.get("controlled") is not True\n',
     '    if False and (not isinstance(k, dict) or k.get("schema") != KERNEL_SCHEMA or k.get("controlled") is not True\n',
     'axon-fabric', _FMT, _FCO),
    ('M1196', "EVIDENCE (4b-3): the kernel's make ran privately, in the constructed environment, with the recorded toolchain", _GE,
     '    if (not isinstance(bu, int) or isinstance(bu, bool) or bu <= 0 or bu == k.get("builder_uid")\n',
     '    if False and (not isinstance(bu, int) or isinstance(bu, bool) or bu <= 0 or bu == k.get("builder_uid")\n',
     'axon-fabric', _FMT, _FCO),
    ('M1197', 'EVIDENCE (4b-3): rootfs.sqfs is the bytes the controlled assembly made from the controlled artifacts and pins', _GE,
     '    if (not isinstance(r, dict) or not sq or r.get("sha256") != sq\n',
     '    if False and (not isinstance(r, dict) or not sq or r.get("sha256") != sq\n',
     'axon-fabric', _FMT, _FCO),
    ('M1198', "EVIDENCE (4b-3): the rootfs was made by /usr/bin's mksquashfs, its exact flags, in the constructed environment", _GE,
     '    if (tool.get("path") != host_tool_path("mksquashfs") or not tool.get("sha256")\n',
     '    if False and (tool.get("path") != host_tool_path("mksquashfs") or not tool.get("sha256")\n',
     'axon-fabric', _FMT, _FCO),
]

# C9 round 4b fix wave, rows4a (amendment 61): the refusal-site gate's file set
# is a rule (every source of the protected crates), and the loop side's sites
# are rowed or exempt. M940-M1019.
_EVL = 'crates/axon-loop/src/evl.rs'
_EXEC_TRUST = '    if !config.verifiers().contains(&issuer) {\n        return Err(format!("{issuer} is not a trusted verifier"));'
_EXEC_QUAL = '    if !qualified {\n        return Err(format!(\n            "{issuer} is not qualified by the operator'
MUTATIONS += [
    ('M940', "PSV-7 (4b): the execution attestation's signer is a verifier the operator trusts (EVL)",
     _EVL, _EXEC_TRUST, _EXEC_TRUST.replace('    if !', '    if false && !', 1),
     'axon-loop', '--test protected_class', 'a_revoked_verifiers_execution_attestation_counts_nothing'),
    ('M941', "PSV-7 (4b): the execution attestation's signer is a verifier the operator trusts NOW (admission re-derivation)",
     _EVL, _EXEC_TRUST, _EXEC_TRUST.replace('    if !', '    if false && !', 1),
     'axon-loop', '--test protected_class', 'a_protected_admission_refuses_an_execution_attester_the_operator_withdrew'),
    ('M942', "A92: the execution attestation's signer is pinned by the operator for the profile it attests (EVL)",
     _EVL, _EXEC_QUAL, _EXEC_QUAL.replace('    if !', '    if false && !', 1),
     'axon-loop', '--test protected_class', 'an_execution_attested_by_a_verifier_not_qualified_for_its_profile_counts_nothing'),
    ('M943', "A92: the execution attestation's signer is pinned for that profile NOW (admission re-derivation)",
     _EVL, _EXEC_QUAL, _EXEC_QUAL.replace('    if !', '    if false && !', 1),
     'axon-loop', '--test protected_class', 'a_protected_admission_refuses_an_execution_attester_no_longer_qualified'),
    ('M944', "RULE:issuer-trusted: an evidence signature's key is one the operator root trusts (readiness, B263)",
     'crates/axon-loop-contracts/src/operator_trust.rs', '    if !trusted.contains(&pk) {', '    if false && !trusted.contains(&pk) {',
     'axon-fabric', '--test readiness_attribution', 'a_b263_record_not_signed_under_the_qualification_root_is_refused'),
]

_SHA_NONE = '        [] => Err(format!(\n            "the execution receipt names no {prefix}…: the launch was not observed"\n        )),'
_SHA_BAD = '        [d] => Err(format!(\n            "the execution receipt\'s {prefix}{d} is not a sha256"\n        )),'
_SHA_MANY = '        _ => Err(format!(\n            "the execution receipt names {prefix}… more than once"\n        )),'
_OT = 'crates/axon-loop-contracts/src/operator_trust.rs'
_OWN_T = '--test trust_root_ownership'
MUTATIONS += [
    ('M945', 'C9r1b class-b join (4b): an attested execution receipt that names no launch digest is unobserved',
     _EVL, _SHA_NONE, '        [] => Ok(()),', 'axon-loop', '--test protected_class',
     'an_unobserved_execution_leg_counts_nothing_in_a_protected_evaluation'),
    ('M946', 'C9r1b class-b join (4b): a launch digest that is not a sha256 is no observation',
     _EVL, _SHA_BAD, '        [_d] => Ok(()),', 'axon-loop', '--test protected_class',
     'an_unobserved_execution_leg_counts_nothing_in_a_protected_evaluation'),
    ('M947', 'C9r1b class-b join (4b): a receipt naming a launch digest twice is ambiguous, not observed',
     _EVL, _SHA_MANY, '        _ => Ok(()),', 'axon-loop', '--test protected_class',
     'an_unobserved_execution_leg_counts_nothing_in_a_protected_evaluation'),
    ('M948', 'O2 ownership walk (4b): a trust-root entry that is a symlink authorizes nothing (EQUIVALENT on Linux: M950)',
     _OT, '        if m.file_type().is_symlink() {\n            return Err(format!(\n                "{} is a symlink: a trust root is never redirected",',
     '        if false && m.file_type().is_symlink() {\n            return Err(format!(\n                "{} is a symlink: a trust root is never redirected",',
     'axon-fabric', _OWN_T, 'a_symlinked_trust_root_key_authorizes_nothing'),
    ('M949', 'O2 ownership walk (4b): a trust-root entry owned by another uid authorizes nothing',
     _OT, '        if m.uid() != 0 {', '        if false && m.uid() != 0 {',
     'axon-fabric', _OWN_T, 'a_trust_root_key_owned_by_another_uid_authorizes_nothing'),
    ('M950', 'O2 ownership walk (4b): a group/other-writable trust-root entry authorizes nothing',
     _OT, '        if m.mode() & 0o022 != 0 {\n            return Err(format!(\n                "{} is group- or other-writable (mode {:o}): it authorizes nothing",',
     '        if false && m.mode() & 0o022 != 0 {\n            return Err(format!(\n                "{} is group- or other-writable (mode {:o}): it authorizes nothing",',
     'axon-fabric', _OWN_T, 'an_other_writable_trust_root_authorizes_nothing'),
    ('M951', 'verify_document (4b): a detached signature verifies under the registered key over the caller\'s binding',
     'crates/axon-loop-contracts/src/attestation.rs',
     '        .verify(&crate::canonical_bytes(&want)?, &s)\n        .map_err(|_| shape(format!("signature does not verify under {key_id}")))?;',
     '        .verify(&crate::canonical_bytes(&want)?, &s)\n        .or(Ok::<(), ring::error::Unspecified>(()))\n        .map_err(|_| shape(format!("signature does not verify under {key_id}")))?;',
     'axon-loop', '--test protected_class', 'a_context_signature_not_made_by_the_key_it_presents_counts_nothing'),
]

EQUIV_RECORD["M948"] = {
    "property": "a symlink in an operator trust root authorizes nothing",
    "subsumed_by": ["M950"], "killer": "joint:M948+M950",
    "all_paths": "check_owned_chain applies its one `check` closure to the base, every component below "
                 "it and (with entries) every entry; `check` lstat()s the path (symlink_metadata) and, "
                 "with no return between them but refusals, tests the symlink type (M948), the owner "
                 "(M949) and the mode (M950). On Linux, the only platform the protected profile runs "
                 "on, a symlink's own mode is always 0777, so `mode & 0o022 != 0` (M950) refuses every "
                 "path M948 refuses"}
EQUIVALENT_DID |= {"M948"}
RETIRED |= {"M948"}

# rows4a (amendment 61): evl.rs refusal sites, check_population, and the siblings
# their four-cell retirements name.
MUTATIONS += [
    ('M952', 'EVL (4b): no evaluation is recorded with no trusted verifier', 'crates/axon-loop/src/evl.rs', '    if verifiers.is_empty() {', '    if false && verifiers.is_empty() {', 'axon-loop', '--test evl_refusal_sites', 'an_evaluation_with_no_trusted_verifier_is_refused'),
    ('M953', "EVL (4b): the evaluated policies are exactly the frozen plan's arms", 'crates/axon-loop/src/evl.rs', '    if supplied != plan_arms {', '    if false && supplied != plan_arms {', 'axon-loop', '--test evl_refusal_sites', 'an_evaluation_supplying_a_policy_outside_the_plan_is_refused'),
    ('M954', "EVL (4b): the evaluation's scope is its plan's (EQUIVALENT: M955)", 'crates/axon-loop/src/evl.rs', '    if frozen.plan.scope != r.scope {', '    if false && frozen.plan.scope != r.scope {', 'axon-loop', '--test evl_refusal_sites', 'an_evaluation_under_another_scope_is_refused'),
    ('M955', "EVL (4b): each supplied policy is the evaluation's scope (EQUIVALENT: M954)", 'crates/axon-loop/src/evl.rs', '        if p.scope != r.scope {', '        if false && p.scope != r.scope {', 'axon-loop', '--test evl_refusal_sites', 'an_evaluation_under_another_scope_is_refused'),
    ('M956', 'EVL (4b) AB9/AB10: one evaluation per frozen experiment (EQUIVALENT: M957)', 'crates/axon-loop/src/evl.rs', '    if let Some((_, prior)) = tx.evaluations_of(&r.experiment_id).first() {', '    if let Some((_, prior)) = tx.evaluations_of(&r.experiment_id).first().filter(|_| false) {', 'axon-loop', '--test evl_refusal_sites', 'a_second_evaluation_of_an_experiment_is_refused'),
    ('M957', 'EVL (4b) AB9/AB10: a trial id is evaluated once in the scope (EQUIVALENT: M956)', 'crates/axon-loop/src/evl.rs', '            .find(|t| trial_ids.contains(&t.trial_id))', '            .find(|t| false && trial_ids.contains(&t.trial_id))', 'axon-loop', '--test evl_refusal_sites', 'a_second_evaluation_of_an_experiment_is_refused'),
    ('M958', 'EVL (4b): an unassigned delivered trial refuses the evaluation', 'crates/axon-loop/src/evl.rs', '        if !assigned_keys.contains(&key) {', '        if false && !assigned_keys.contains(&key) {', 'axon-loop', '--test evl_refusal_sites', 'an_unassigned_trial_never_rides_into_an_evaluation'),
    ('M959', 'EVL (4b): a future-dated preflight refuses the evaluation', 'crates/axon-loop/src/evl.rs', '        if ctx.created_ms > now {', '        if false && ctx.created_ms > now {', 'axon-loop', '--test evl_refusal_sites', 'a_future_dated_preflight_refuses_the_evaluation'),
    ('M960', 'EVL (4b) G33: a trial preflighted before the freeze refuses the evaluation', 'crates/axon-loop/src/evl.rs', '        if ctx.created_ms < frozen.freeze_ms {', '        if false && ctx.created_ms < frozen.freeze_ms {', 'axon-loop', '--test evl_refusal_sites', 'a_preflight_before_the_freeze_refuses_the_evaluation'),
    ('M961', 'EVL (4b): a trial delivered twice refuses the evaluation', 'crates/axon-loop/src/evl.rs', '            .is_some()\n        {\n            return Err(refused(format!("trials[{i}]: trial delivered twice")));', '            .is_some()\n            && false\n        {\n            return Err(refused(format!("trials[{i}]: trial delivered twice")));', 'axon-loop', '--test evl_refusal_sites', 'a_trial_delivered_twice_refuses_the_evaluation'),
    ('M962', 'EVL (4b): cross-tenant evidence never joins (EQUIVALENT: M15 + M963)', 'crates/axon-loop/src/evl.rs', '                } else if d.ep.scope != r.scope || d.ctx.scope != r.scope {', '                } else if false && (d.ep.scope != r.scope || d.ctx.scope != r.scope) {', 'axon-loop', '--test evl_refusal_sites', 'a_cross_tenant_context_never_counts'),
    ('M963', 'bind_episode (4b): an episode binds only a context and policy of its own scope (intake)', 'crates/axon-loop-contracts/src/checks.rs', '    if episode.scope != ctx.scope || episode.scope != policy.scope {', '    if false && (episode.scope != ctx.scope || episode.scope != policy.scope) {', 'axon-loop', '--test evl_refusal_sites', 'an_episode_bound_to_another_tenants_context_is_never_intaken'),
    ('M964', 'EVL (4b): a trial counts only for the arm whose policy it ran (EQUIVALENT: M965)', 'crates/axon-loop/src/evl.rs', '    if &d.ep.policy_ref != policy_ref {', '    if false && &d.ep.policy_ref != policy_ref {', 'axon-loop', '--test evl_refusal_sites', 'an_episode_of_another_policy_never_counts_for_an_arm'),
    ('M965', "bind_episode (4b): the episode ran the policy it is bound to (the library primitive's own contract; its production callers decide it first, M964)", 'crates/axon-loop-contracts/src/checks.rs', '    if episode.policy_ref != digest(policy)? || episode.context_ref != digest(ctx)? {', '    if episode.context_ref != digest(ctx)? {', 'axon-loop-contracts', '--test fixtures', 'bind_episode_refuses_mismatches'),
    ('M966', 'check_population (4b): one policy per arm (issued by plan::assign)', 'crates/axon-loop/src/evl.rs', '        if arm_policy[&a.arm_id] != &a.policy_ref {', '        if false && arm_policy[&a.arm_id] != &a.policy_ref {', 'axon-loop', '--test evl_refusal_sites', 'each_population_defect_is_never_issued'),
    ('M967', 'check_population (4b): both arms are assigned', 'crates/axon-loop/src/evl.rs', '    if arms_seen.len() != 2 {', '    if false && arms_seen.len() != 2 {', 'axon-loop', '--test evl_refusal_sites', 'each_population_defect_is_never_issued'),
    ('M968', 'check_population (4b) AB9: each arm covers exactly the task manifest', 'crates/axon-loop/src/evl.rs', '        if tasks != manifest.task_set() {', '        if false && tasks != manifest.task_set() {', 'axon-loop', '--test evl_refusal_sites', 'each_population_defect_is_never_issued'),
    ('M969', 'check_population (4b): each (arm, task) is assigned exactly `repetitions` times', 'crates/axon-loop/src/evl.rs', '    if let Some(((arm, task), n)) = per_arm_task.iter().find(|(_, n)| **n != reps) {', '    if let Some(((arm, task), n)) = per_arm_task.iter().find(|(_, n)| false && **n != reps) {', 'axon-loop', '--test evl_refusal_sites', 'each_population_defect_is_never_issued'),
    ('M970', "check_population (4b): only the plan's arm policies (EQUIVALENT: M971)", 'crates/axon-loop/src/evl.rs', '        if !plan_arms.contains(&a.policy_ref) {', '        if false && !plan_arms.contains(&a.policy_ref) {', 'axon-loop', '--test evl_refusal_sites', 'a_population_naming_a_policy_outside_the_plan_is_never_issued'),
    ('M971', "plan::assign (4b): only the plan's arm policies (EQUIVALENT: M970)", 'crates/axon-loop/src/plan.rs', '        if !arms.contains(&t.policy_ref) {', '        if false && !arms.contains(&t.policy_ref) {', 'axon-loop', '--test evl_refusal_sites', 'a_population_naming_a_policy_outside_the_plan_is_never_issued'),
    ('M972', 'check_population (4b): a trial id once in the population (EQUIVALENT: M973)', 'crates/axon-loop/src/evl.rs', '        if !trial_ids.insert(&a.trial_id) {', '        if !trial_ids.insert(&a.trial_id) && false {', 'axon-loop', '--test evl_refusal_sites', 'a_trial_id_issued_for_two_tasks_is_never_issued'),
    ('M973', 'plan::assign (4b): a trial is issued once (EQUIVALENT: M972)', 'crates/axon-loop/src/plan.rs', '        if !trials.insert(&t.trial_id) || !attempts.insert((&t.trial_id, &t.attempt_id)) {', '        if (!trials.insert(&t.trial_id) || !attempts.insert((&t.trial_id, &t.attempt_id))) && false {', 'axon-loop', '--test evl_refusal_sites', 'a_trial_id_issued_for_two_tasks_is_never_issued'),
    ('M974', 'check_population (4b): a (task, arm, trial) once (EQUIVALENT: M972 + M973)', 'crates/axon-loop/src/evl.rs', '        if !assigned_keys.insert((a.task_id.clone(), a.arm_id.clone(), a.trial_id.clone())) {', '        if !assigned_keys.insert((a.task_id.clone(), a.arm_id.clone(), a.trial_id.clone())) && false {', 'axon-loop', '--test evl_refusal_sites', 'a_trial_assigned_twice_is_never_issued'),
    ('M975', 'EVL (4b): only a journalled evaluation is read as evidence', 'crates/axon-loop/src/evl.rs', '    let (seq, _) = tx.evaluation_event(r).ok_or_else(|| {\n        refused(format!(\n            "evaluation {r} was never journalled by `evl evaluate`"\n        ))\n    })?;', '    let seq = tx.evaluation_event(r).map(|(s, _)| s).unwrap_or(u64::MAX);', 'axon-loop', '--test evl_refusal_sites', 'an_unjournalled_evaluation_is_never_admitted'),
]

EQUIV_RECORD["M954"] = {
    "property": "an evaluation is never recorded under another scope than its plan's",
    "subsumed_by": ["M955"], "killer": "joint:M954+M955",
    "all_paths": "evaluate's only path to a record passes both checks, then `supplied != plan_arms` "
                 "(M953): every supplied policy IS one of the plan's two arms (keys are digests), and a "
                 "PolicyEnvelope's digest covers its scope, so each supplied p.scope is the plan's scope; "
                 "hence p.scope != r.scope (M955) holds exactly when plan.scope != r.scope (M954)"}
EQUIV_RECORD["M955"] = {
    "property": "an evaluation is never recorded under another scope than its plan's",
    "subsumed_by": ["M954"], "killer": "joint:M954+M955",
    "all_paths": "M954 runs first on every call of evaluate and refuses r.scope != plan.scope; after it "
                 "r.scope is the plan's, and a policy of another scope has another digest, so it is "
                 "refused by M953 or never supplied"}
EQUIV_RECORD["M956"] = {
    "property": "a frozen experiment has one evaluation (no REJECT re-rolled)",
    "subsumed_by": ["M957"], "killer": "joint:M956+M957",
    "all_paths": "a second evaluation of the experiment must assign exactly the journalled population "
                 "(requested == issued, M108, against the ONE assignment plan::assign journals per "
                 "experiment), so its trial ids are the first evaluation's, which the scope-wide trial-id "
                 "check (M957) refuses"}
EQUIV_RECORD["M957"] = {
    "property": "a frozen experiment has one evaluation (no REJECT re-rolled)",
    "subsumed_by": ["M956"], "killer": "joint:M956+M957",
    "all_paths": "a trial id is issued to one experiment only (plan::assign refuses an id issued to another "
                 "experiment of the scope), so a prior evaluation holding one of this request's trial ids "
                 "is an evaluation of this experiment, which M956 refuses first"}
EQUIV_RECORD["M962"] = {
    "property": "evidence minted for another scope never counts",
    "subsumed_by": ["M15", "M963"], "killer": "joint:M962+M15+M963",
    "all_paths": "M962 sits in the else-if chain after intake_join (M15): the delivered episode is one intaken "
                 "in THIS scope; intake records an episode only through bind_episode, whose scope join "
                 "(M963) refuses an episode, context or policy of different scopes, and judge calls the same "
                 "bind_episode on the delivered context, whose bytes the episode names (M857), so a "
                 "context or episode outside r.scope is refused by M15 or M963 on every path"}
EQUIV_RECORD["M964"] = {
    "property": "a trial counts only for the arm whose policy it ran",
    "subsumed_by": ["M965"], "killer": "joint:M964+M965",
    "all_paths": "judge's next statement calls bind_episode with policy = policies[a.policy_ref], a map keyed "
                 "by the policy's own digest, so digest(policy) == policy_ref and bind_episode's "
                 "`episode.policy_ref != digest(policy)` (M965) is M964's predicate, with no return between"}
EQUIV_RECORD["M970"] = {
    "property": "a population names only the plan's two arm policies",
    "subsumed_by": ["M971"], "killer": "joint:M970+M971",
    "all_paths": "check_population's callers: plan::assign, which then refuses the same predicate over the "
                 "same trials (`!arms.contains(&t.policy_ref)`, M971) before journalling; and evaluate, "
                 "whose population equals the journalled one (M108), which passed M971"}
EQUIV_RECORD["M971"] = {
    "property": "a population names only the plan's two arm policies",
    "subsumed_by": ["M970"], "killer": "joint:M970+M971",
    "all_paths": "plan::assign calls check_population over the same trials before M971, and its arm-policy "
                 "check (M970) refuses the identical predicate (plan_arms is the same two refs)"}
EQUIV_RECORD["M972"] = {
    "property": "a trial id is issued once in a population",
    "subsumed_by": ["M973"], "killer": "joint:M972+M973",
    "all_paths": "check_population's callers: plan::assign, which then refuses a repeated trial id over the "
                 "same trials (`!trials.insert(&t.trial_id)`, M973) before journalling; evaluate, whose "
                 "population equals the journalled one (M108)"}
EQUIV_RECORD["M973"] = {
    "property": "a trial id is issued once in a population",
    "subsumed_by": ["M972"], "killer": "joint:M972+M973",
    "all_paths": "plan::assign calls check_population over the same trials before M973, and its trial-id check "
                 "(M972) refuses every repeated id; a repeated (trial, attempt) pair is a repeated trial id"}
EQUIV_RECORD["M974"] = {
    "property": "a (task, arm, trial) is assigned once",
    "subsumed_by": ["M972", "M973"], "killer": "joint:M974+M972+M973",
    "all_paths": "a repeated (task, arm, trial) key repeats its trial id, which check_population's trial-id "
                 "check (M972, same function, no return between but refusals) and plan::assign's (M973) "
                 "each refuse on every path"}
EQUIVALENT_DID |= {"M954", "M955", "M956", "M957", "M962", "M964", "M970", "M971", "M972", "M973", "M974"}
RETIRED |= {"M954", "M955", "M956", "M957", "M962", "M964", "M970", "M971", "M972", "M973", "M974"}

# rows4a (amendment 61): store.rs.
MUTATIONS += [
    ('M976', 'ADR-002 (4b): one public key is never registered for two roles', 'crates/axon-loop/src/store.rs', '                    if prev != role {', '                    if false && prev != role {', 'axon-loop', '--test evl_admission', 'an_observer_key_and_identity_are_its_own'),
    ('M977', 'ADR-002 (4b): a preflight observer holds no other loop role', 'crates/axon-loop/src/store.rs', '                if set.contains(o) {', '                if false && set.contains(o) {', 'axon-loop', '--test evl_admission', 'an_observer_key_and_identity_are_its_own'),
    ('M978', 'store (4b): a CAS record is read only if its content digests to its name', 'crates/axon-loop/src/store.rs', '        if &d != r {', '        if false && &d != r {', 'axon-loop', '--test store_integrity', 'a_journalled_evaluation_edited_in_place_is_never_admitted'),
    ('M979', 'store (4b): no store path crosses a symlink (guard; EQUIVALENT for writes: M980)', 'crates/axon-loop/src/store.rs', '                Ok(m) if m.file_type().is_symlink() => return Err(symlink_err(&cur)),', '                Ok(m) if false && m.file_type().is_symlink() => return Err(symlink_err(&cur)),', 'axon-loop', '--test store_integrity', 'a_store_directory_replaced_by_a_symlink_is_never_written_through'),
    ('M980', 'store (4b): ensure_dir re-checks each component is no symlink (EQUIVALENT: M979)', 'crates/axon-loop/src/store.rs', '            if m.file_type().is_symlink() {\n                return Err(symlink_err(&cur));', '            if false && m.file_type().is_symlink() {\n                return Err(symlink_err(&cur));', 'axon-loop', '--test store_integrity', 'a_store_directory_replaced_by_a_symlink_is_never_written_through'),
]

EQUIV_RECORD["M979"] = {
    "property": "nothing is written through a symlink inside the store",
    "subsumed_by": ["M980", "M998"], "killer": "joint:M979+M980+M998",
    "all_paths": "every store write goes through write_atomic, which first calls ensure_dir(parent): ensure_dir "
                 "walks every component below the root (create_dir, then lstat) and refuses a symlink (M980) "
                 "and, since lstat reports a symlink as no directory, any component that is not a real "
                 "directory (M998), before anything is written; a READ through a symlinked directory returns only bytes that must "
                 "still digest to their name (check_name, M978) or that the ledger verifies"}
EQUIV_RECORD["M980"] = {
    "property": "nothing is written through a symlink inside the store",
    "subsumed_by": ["M979", "M998"], "killer": "joint:M979+M980+M998",
    "all_paths": "ensure_dir's first statement is guard(dir), which lstat()s every existing component and refuses a "
                 "symlink (M979); M980 differs only for a component that became a symlink between the two calls "
                 "(a concurrent writer), which write_atomic's guard(path) after ensure_dir refuses again; and "
                 "the next statement refuses a component lstat does not report as a directory (M998), "
                 "which a symlink never is"}
EQUIVALENT_DID |= {"M979", "M980"}
RETIRED |= {"M979", "M980"}

MUTATIONS += [
    ('M981', 'EVL (4b): one arm per policy when a record is read by policy (EQUIVALENT: M819)', 'crates/axon-loop/src/evl.rs', '        if it.next().is_some() {', '        if false && it.next().is_some() {', 'axon-loop', '--test evl_refusal_sites', 'a_policy_split_across_two_arms_is_never_admitted_on_half_its_trials'),
]
EQUIV_RECORD["M981"] = {
    "property": "a candidate is never admitted on part of its trials split into another arm",
    "subsumed_by": ["M819"], "killer": "joint:M981+M819",
    "all_paths": "arm_for_policy's callers: admission's derive, which first refuses a record that does not "
                 "have exactly two arms (M819), and the plan freezes two distinct policies, so two arms "
                 "sharing one policy leave none for the other; and pointer::safety_still_holds, which reads "
                 "only an admitted evaluation's record (load_journalled of adm.evaluation_ref), one derive "
                 "accepted under M819"}
EQUIVALENT_DID |= {"M981"}
RETIRED |= {"M981"}

# rows4a (amendment 61): safety.rs.
MUTATIONS += [
    ('M982', 'ADR-001 §5 (4b): only a trusted monitor or a subject may report a trial unsafe', 'crates/axon-loop/src/safety.rs', '            if !is_monitor && !is_subject {', '            if false && !is_monitor && !is_subject {', 'axon-loop', '--test safety_sites', 'a_strangers_violation_never_vetoes_a_candidate'),
    ('M983', 'ADR-001 §5 (4b): a clearance comes from a trusted monitor independent of the trial', 'crates/axon-loop/src/safety.rs', '            if !is_monitor || is_subject {', '            if false && (!is_monitor || is_subject) {', 'axon-loop', '--test safety_sites', 'a_subject_keyed_as_a_monitor_never_clears_its_own_trial'),
    ('M984', "FG-050 (4b): a clearance's monitor signature verifies", 'crates/axon-loop/src/safety.rs', '            .map_err(|e| refused(format!("clearance signature refused: {e}")))?;', '            .ok();', 'axon-loop', '--test safety_sites', 'a_clearance_whose_signature_does_not_verify_is_never_recorded'),
]

# rows4a (amendment 61): tasks.rs, candidates.rs.
MUTATIONS += [
    ('M985', 'task manifest (4b): 1..=100000 tasks', 'crates/axon-loop/src/tasks.rs', '        if self.tasks.is_empty() || self.tasks.len() > 100_000 {', '        if false && (self.tasks.is_empty() || self.tasks.len() > 100_000) {', 'axon-loop', '--test registries', 'each_defective_task_manifest_is_never_registered'),
    ('M986', 'task manifest (4b): one spelling (sorted, no repeats)', 'crates/axon-loop/src/tasks.rs', '        if !self.tasks.windows(2).all(|w| w[0] < w[1]) {', '        if false && (!self.tasks.windows(2).all(|w| w[0] < w[1])) {', 'axon-loop', '--test registries', 'each_defective_task_manifest_is_never_registered'),
    ('M987', 'task manifest (4b): registered only by a trusted admitter', 'crates/axon-loop/src/tasks.rs', '    if !store.config()?.admitters().contains(&m.issuer_ref) {', '    if false && (!store.config()?.admitters().contains(&m.issuer_ref)) {', 'axon-loop', '--test registries', 'each_defective_task_manifest_is_never_registered'),
    ('M988', 'task manifest (4b): a plan rests only on a REGISTERED manifest', 'crates/axon-loop/src/tasks.rs', '    if !tx.task_manifest_event(scope, r) {', '    if false && (!tx.task_manifest_event(scope, r)) {', 'axon-loop', '--test registries', 'a_planted_task_manifest_never_freezes_a_plan'),
    ('M989', "task manifest (4b): a registered manifest's file is the manifest it names", 'crates/axon-loop/src/tasks.rs', '    if &m.manifest_ref()? != r || &m.scope != scope {', '    if false && (&m.manifest_ref()? != r || &m.scope != scope) {', 'axon-loop', '--test registries', 'a_task_manifest_edited_in_place_never_decides_a_population'),
    ('M990', 'candidate list (4b): 1..=4096 candidates', 'crates/axon-loop/src/candidates.rs', '        if self.candidates.is_empty() || self.candidates.len() > 4096 {', '        if false && (self.candidates.is_empty() || self.candidates.len() > 4096) {', 'axon-loop', '--test registries', 'each_defective_candidate_list_is_never_registered'),
    ('M991', 'candidate list (4b): one spelling (sorted, no repeats)', 'crates/axon-loop/src/candidates.rs', '        if !self.candidates.windows(2).all(|w| w[0] < w[1]) {', '        if false && (!self.candidates.windows(2).all(|w| w[0] < w[1])) {', 'axon-loop', '--test registries', 'each_defective_candidate_list_is_never_registered'),
    ('M992', 'candidate list (4b): registered only by a trusted admitter', 'crates/axon-loop/src/candidates.rs', '    if !store.config()?.admitters().contains(&c.issuer_ref) {', '    if false && (!store.config()?.admitters().contains(&c.issuer_ref)) {', 'axon-loop', '--test registries', 'each_defective_candidate_list_is_never_registered'),
    ('M993', 'candidate list (4b): a policy rests only on a REGISTERED list', 'crates/axon-loop/src/candidates.rs', '    if !tx.candidate_set_event(scope, r) {', '    if false && (!tx.candidate_set_event(scope, r)) {', 'axon-loop', '--test registries', 'a_planted_candidate_list_never_admits_a_policy'),
    ('M994', "candidate list (4b): a registered list's file is the list it names", 'crates/axon-loop/src/candidates.rs', '    if &c.candidate_set_ref()? != r || &c.scope != scope {', '    if false && (&c.candidate_set_ref()? != r || &c.scope != scope) {', 'axon-loop', '--test registries', 'a_candidate_list_edited_in_place_never_admits_a_policy'),
]

# rows4a (amendment 61): rules.rs.
MUTATIONS += [
    ('M995', 'plan rules (4b): a word rule is exactly the one the code executes', 'crates/axon-loop/src/rules.rs', '        Some(x) => Err(format!("{field} {x:?} is not executable (expected `{w}`)")),', '        Some(_x) => Ok(()),', 'axon-loop', '--test registries', 'each_unexecutable_rule_never_freezes'),
    ('M996', 'plan rules (4b): a quality margin below 100%', 'crates/axon-loop/src/rules.rs', '        if margin_ppm >= PPM {', '        if false && margin_ppm >= PPM {', 'axon-loop', '--test registries', 'each_unexecutable_rule_never_freezes'),
    ('M997', 'plan rules (4b): an economic threshold at most 100%', 'crates/axon-loop/src/rules.rs', '            if n > PPM {', '            if false && n > PPM {', 'axon-loop', '--test registries', 'each_unexecutable_rule_never_freezes'),
]

MUTATIONS += [
    ('M998', 'store (4b): every store path component is a real directory (lstat; EQUIVALENT: M979 + M980)', 'crates/axon-loop/src/store.rs', '            if !m.is_dir() {', '            if false && !m.is_dir() {', 'axon-loop', '--test store_integrity', 'a_store_directory_replaced_by_a_symlink_is_never_written_through'),
]
EQUIV_RECORD["M998"] = {
    "property": "nothing is written through a symlink inside the store",
    "subsumed_by": ["M979", "M980"], "killer": "joint:M979+M980+M998",
    "all_paths": "ensure_dir's first statement is guard(dir), which lstat()s every existing component and "
                 "refuses a symlink (M979), and the statement before M998 refuses a component lstat "
                 "reports as a symlink (M980); a regular file in the path (the only other non-directory) "
                 "makes the write below it fail with ENOTDIR"}
EQUIVALENT_DID |= {"M998"}
RETIRED |= {"M998"}

# rows4a (amendment 61): evl.rs rows killed by existing production-route tests.
MUTATIONS += [
    ('M999', "EVL (4b): a trial's context check includes its protected authentication", 'crates/axon-loop/src/evl.rs', '                } else if let Err(e) = authenticated_context(&config, frozen.evaluation_class, d) {', '                } else if let Some(e) =\n                    authenticated_context(&config, frozen.evaluation_class, d).err().filter(|_| false)\n                {', 'axon-loop', '--test protected_class', 'a_protected_context_is_authenticated_not_named'),
    ('M1000', 'EVL (4b): a context refusal makes the trial Unknown (TASK_NOT_STARTED evidence)', 'crates/axon-loop/src/evl.rs', '                    match ctx_check {\n                        Err(e) => unknown(', '                    match ctx_check.or(Ok::<(), String>(())) {\n                        Err(e) => unknown(', 'axon-loop', '--test evidence_laundering', 'an_episode_intake_never_recorded_never_counts'),
    ('M1001', 'EVL (4b): an unbound episode never counts', 'crates/axon-loop/src/evl.rs', '    if let Err(e) = bind_episode(&d.ep, policy, &d.ctx, epoch, verifiers, subjects) {', '    if let Some(e) = bind_episode(&d.ep, policy, &d.ctx, epoch, verifiers, subjects).err().filter(|_| false) {', 'axon-loop', '--test evl_admission', 'a_trial_delivered_with_a_context_other_than_its_episodes_counts_nothing'),
]

# rows4a (amendment 61): plan.rs.
MUTATIONS += [
    ('M1006', 'plan (4b): a frozen plan is never registered again', 'crates/axon-loop/src/plan.rs', '    if tx.freeze_of(&plan.experiment_id).is_some() {', '    if false && (tx.freeze_of(&plan.experiment_id).is_some()) {', 'axon-loop', '--test plan_sites', 'a_frozen_plan_is_never_registered_again'),
    ('M1007', 'plan (4b): a stored plan is read only if it digests to its name', 'crates/axon-loop/src/plan.rs', '    if &p.digest()? != r {', '    if false && (&p.digest()? != r) {', 'axon-loop', '--test plan_sites', 'a_frozen_plans_file_edited_in_place_never_decides'),
    ('M1008', 'plan (4b): no operator field is unset at the freeze', 'crates/axon-loop/src/plan.rs', '    if !unset.is_empty() {', '    if false && (!unset.is_empty()) {', 'axon-loop', '--test plan_sites', 'a_plan_with_an_operator_field_unset_never_freezes'),
    ('M1009', 'plan (4b) no plan shopping: a candidate is frozen in one experiment', 'crates/axon-loop/src/plan.rs', '            if candidate_policy_ref == &cand {', '            if false && (candidate_policy_ref == &cand) {', 'axon-loop', '--test plan_sites', 'a_candidate_frozen_once_never_freezes_in_a_second_experiment'),
    ('M1010', 'plan (4b) AB9: the manifest holds the planned independent units', 'crates/axon-loop/src/plan.rs', '    if (manifest.tasks.len() as u64) < units {', '    if false && ((manifest.tasks.len() as u64) < units) {', 'axon-loop', '--test plan_sites', 'a_manifest_smaller_than_the_planned_units_never_freezes'),
    ('M1011', "check_candidate (4b): the candidate was proposed by EVO in the plan's scope", 'crates/axon-loop/src/plan.rs', '    if crate::evo::proposer_in(tx, &plan.scope, cand).is_none() {', '    if false && (crate::evo::proposer_in(tx, &plan.scope, cand).is_none()) {', 'axon-loop', '--test plan_sites', 'a_candidate_evo_never_proposed_never_freezes'),
    ('M1012', "check_candidate (4b): the candidate's parent is the plan's incumbent", 'crates/axon-loop/src/plan.rs', '    if &ce.parent_policy_ref != inc {', '    if false && (&ce.parent_policy_ref != inc) {', 'axon-loop', '--test plan_sites', 'a_candidate_of_another_parent_never_freezes'),
    ('M1013', "check_candidate (4b): the plan's controls are its policies'", 'crates/axon-loop/src/plan.rs', '    if Some(&ce.controls_ref) != plan.controls_ref.as_ref() || ce.controls_ref != ie.controls_ref {', '    if false && (Some(&ce.controls_ref) != plan.controls_ref.as_ref() || ce.controls_ref != ie.controls_ref) {', 'axon-loop', '--test plan_sites', 'a_plan_whose_controls_are_not_its_policies_never_freezes'),
    ('M1014', 'plan (4b): a plan with a start blocker never starts', 'crates/axon-loop/src/plan.rs', '    if !b.is_empty() {', '    if false && (!b.is_empty()) {', 'axon-loop', '--test plan_sites', 'a_blocked_plan_never_starts'),
    ('M1015', "assign (4b): the assignment is for the plan's scope", 'crates/axon-loop/src/plan.rs', '    if a.scope != frozen.plan.scope {', '    if false && (a.scope != frozen.plan.scope) {', 'axon-loop', '--test plan_sites', 'each_assignment_defect_is_never_issued'),
    ('M1016', 'assign (4b): the assignment is issued by a trusted admitter', 'crates/axon-loop/src/plan.rs', '    if !config.admitters().contains(&a.issuer_ref) {', '    if false && (!config.admitters().contains(&a.issuer_ref)) {', 'axon-loop', '--test plan_sites', 'each_assignment_defect_is_never_issued'),
    ('M1017', "assign (4b) G11: the assigner is not the scope's EVO proposer", 'crates/axon-loop/src/plan.rs', '    if tx.hypotheses(&a.scope, None).iter().any(|h| {', '    if false && tx.hypotheses(&a.scope, None).iter().any(|h| {', 'axon-loop', '--test plan_sites', 'each_assignment_defect_is_never_issued'),
    ('M1018', 'assign (4b): the population is issued once per experiment', 'crates/axon-loop/src/plan.rs', '    if let Some((_, existing)) = tx.assignment_of(&a.experiment_id) {\n        if existing == r {', '    if let Some((_, existing)) = tx.assignment_of(&a.experiment_id).filter(|(_, e)| e == &r) {\n        if existing == r {', 'axon-loop', '--test plan_sites', 'each_assignment_defect_is_never_issued'),
    ('M1019', 'assign (4b): a trial id is issued to one experiment of the scope', 'crates/axon-loop/src/plan.rs', '                if let Some(t) = other.trials.iter().find(|t| trials.contains(&t.trial_id)) {', '                if let Some(t) = other.trials.iter().find(|t| false && trials.contains(&t.trial_id)) {', 'axon-loop', '--test plan_sites', 'a_trial_id_of_another_experiment_is_never_issued'),
]

# rows4a (amendment 61): evl.rs, the judge's remaining refusals.
MUTATIONS += [
    ('M1002', 'EVL (4b): a trial whose ACF evidence does not bind never counts', 'crates/axon-loop/src/evl.rs', '        if let Err(e) = bind_acf(&d.ep, req, rcpt, proj) {', '        if let Some(e) = bind_acf(&d.ep, req, rcpt, proj).err().filter(|_| false) {', 'axon-loop', '--test evidence_laundering', 'laundered_evidence_never_crosses_independent_admission'),
    ('M1003', 'EVL (4b) D3: a protected evaluation refuses a development backend on either leg', 'crates/axon-loop/src/evl.rs', '            if let Some(b) = b.filter(|b| !axon_loop_contracts::PROTECTED_PROFILES.contains(b)) {', '            if let Some(b) = b.filter(|b| false && !axon_loop_contracts::PROTECTED_PROFILES.contains(b)) {', 'axon-loop', '--test protected_class', 'a_cited_unknown_from_a_development_verification_is_unverifiable'),
    ('M1004', "EVL (4b) M4: a protected verdict counts only as protected evidence (the join's refusal)", 'crates/axon-loop/src/evl.rs', '            if let Err(e) = joined {', '            if let Some(e) = joined.err().filter(|_| false) {', 'axon-loop', '--test protected_class', 'only_protected_class_evidence_counts_in_a_protected_evaluation'),
    ('M1005', 'EVL (4b): a verdict counts only on evidence that authenticates in this evaluation', 'crates/axon-loop/src/evl.rs', '            Err((kind, e)) => return unknown(kind, format!("unauthenticated verification: {e}")),', '            Err((_kind, _e)) => {}', 'axon-loop', '--test evl_refusal_sites', 'a_verdict_whose_delivered_attestation_does_not_verify_never_counts'),
]

# ── C9 round 4b, INTEGRATE (amendment 64) ── the refusal-site gate's own guards:
# the re-measured NOT_YET_SCANNED count and the freeze reading (amendment 61,
# left unrowed by rows4a), the interp.rs region UNION and its anchor
# uniqueness (this integration), and the freeze's consultation of the gate.
# Each is killed by the REAL gate (or the real freeze) run over a copy of the
# tree, edited in the attack's one way.
_RCG = 'scripts/v022_refusal_coverage.py'
_RCT = '--no-default-features --test refusal_coverage_gate'
MUTATIONS += [
    ('M1392', "COVERAGE GATE (4b): a NOT YET SCANNED file's listed count is re-measured, so a site added to it is reported", _RCG,
     '            if len(uncovered) != listed:\n', '            if False and len(uncovered) != listed:\n',
     'axon-core', _RCT, 'a_new_site_in_a_not_yet_scanned_file_is_reported'),
    ('M1393', 'COVERAGE GATE (4b): the freeze reading refuses while any in-scope file is NOT YET SCANNED', _RCG,
     '            if freeze:\n', '            if False and freeze:\n',
     'axon-core', _RCT, 'a_freeze_reading_refuses_a_not_yet_scanned_file'),
    ('M1394', "COVERAGE GATE (4b): interp.rs is scanned over the union of the seal fns and core2's anchored region", _RCG,
     '                regions.append(r)\n', '                pass\n',
     'axon-core', _RCT, 'a_site_in_the_anchored_region_outside_a_seal_fn_is_scanned'),
    ('M1395', 'COVERAGE GATE (4b): a REGIONS anchor names exactly one place', _RCG,
     '    if text.count(a) != 1 or text.count(b) != 1:\n', '    if text.count(a) < 1 or text.count(b) < 1:\n',
     'axon-core', _RCT, 'a_region_anchor_that_is_not_unique_is_refused'),
    ('M1396', 'EVIDENCE (4b): no freeze binds evidence while the refusal-site gate does not hold at a freeze', 'scripts/v022_freeze_manifest.py',
     '    if coverage_problems:\n', '    if False and coverage_problems:\n',
     'axon-fabric', '--test freeze_manifest', 'a_freeze_is_refused_while_a_protected_file_is_not_yet_scanned'),
    ('M1397', "EVIDENCE (4b): the freeze asks the refusal-site gate for its FREEZE reading", 'scripts/v022_freeze_manifest.py',
     '    coverage_problems = cov.check(freeze=True, out=lambda *_: None)\n', '    coverage_problems = cov.check(freeze=False, out=lambda *_: None)\n',
     'axon-fabric', '--test freeze_manifest', 'a_freeze_asks_the_gate_for_its_freeze_reading'),
]
# ── C9 round 4b, INTEGRATE-A (amendment 64) ── the contract crate's refusal sites
# (canonical, compute, episode, ids, lib, policy, receipt, schema), each judged on the
# production route that reads the attacker's bytes (crates/axon-loop/tests/contract_sites.rs:
# intake_episode, evl::evaluate, PinnedSchedule::pin, CandidateSet::parse + put, the CLI's
# parse + put_policy / pointer::transition). M1200-M1269.
_CS = '--test contract_sites'
MUTATIONS += [
    ('M1200', "EVIDENCE (4b, integrate-A): a document over the profile's byte limit is never read", 'crates/axon-loop-contracts/src/canonical.rs',
     '    if json.len() > MAX_BYTES {',
     '    if false && json.len() > MAX_BYTES {',
     'axon-loop', _CS, 'a_document_over_the_byte_limit_is_never_read'),
    ('M1201', 'EVIDENCE (4b, integrate-A): container nesting past MAX_DEPTH is refused before the parser recurses', 'crates/axon-loop-contracts/src/canonical.rs',
     '            if depth > MAX_DEPTH {\n                return Err(Refusal::TooDeep);',
     '            if false && depth > MAX_DEPTH {\n                return Err(Refusal::TooDeep);',
     'axon-loop', _CS, 'an_empty_container_nested_past_the_limit_is_never_read'),
    ('M1202', 'EVIDENCE (4b, integrate-A): a value past MAX_DEPTH is never read (EQUIVALENT: M1201)', 'crates/axon-loop-contracts/src/canonical.rs',
     '    if depth > MAX_DEPTH {\n        return Err(Refusal::TooDeep);',
     '    if false && depth > MAX_DEPTH {\n        return Err(Refusal::TooDeep);',
     'axon-loop', _CS, 'a_value_nested_past_the_limit_is_never_read'),
    ('M1203', "EVIDENCE (4b, integrate-A): an object's keys count a level (EQUIVALENT: M1202+M1201)", 'crates/axon-loop-contracts/src/canonical.rs',
     '            if !o.is_empty() && depth + 1 > MAX_DEPTH {',
     '            if false && !o.is_empty() && depth + 1 > MAX_DEPTH {',
     'axon-loop', _CS, 'an_object_with_keys_at_the_depth_limit_is_never_read'),
    ('M1204', 'EVIDENCE (4b, integrate-A): an integer with |n| > 2^53-1 is never read', 'crates/axon-loop-contracts/src/canonical.rs',
     '        if i.unsigned_abs() > MAX_INTEGER {',
     '        if false && i.unsigned_abs() > MAX_INTEGER {',
     'axon-loop', _CS, 'an_integer_past_two_to_the_53_is_never_read'),
    ('M1205', 'EVIDENCE (4b, integrate-A): an integer past i64 is never read', 'crates/axon-loop-contracts/src/canonical.rs',
     '        if u > MAX_INTEGER {',
     '        if false && u > MAX_INTEGER {',
     'axon-loop', _CS, 'an_integer_past_i64_is_never_read'),
    ('M1206', 'EVIDENCE (4b, integrate-A): a float is never read (the profile has none)', 'crates/axon-loop-contracts/src/canonical.rs',
     '        // outside the profile.\n        Err(Refusal::Float)',
     '        // outside the profile.\n        Ok(())',
     'axon-loop', _CS, 'a_float_is_never_read'),
    ('M1207', 'EVIDENCE (4b, integrate-A): a canonical form over the byte limit is never digested (EQUIVALENT: M1200)', 'crates/axon-loop-contracts/src/canonical.rs',
     '    if out.len() > MAX_BYTES {',
     '    if false && out.len() > MAX_BYTES {',
     'axon-loop', _CS, 'a_value_whose_canonical_form_is_over_the_byte_limit_is_never_digested'),
    ('M1210', "EVIDENCE (4b, integrate-A) D1: a struct spelled as a positional array is refused by the schema's type", 'crates/axon-loop-contracts/src/schema.rs',
     '    Err(fail(path, format!("{} is not of type {}", kind(v), t)))',
     '    Ok(())',
     'axon-loop', _CS, 'a_struct_spelled_as_a_positional_array_is_never_recorded'),
    ('M1211', "EVIDENCE (4b, integrate-A) D2: a unit variant spelled as a map is refused by the schema's enum", 'crates/axon-loop-contracts/src/schema.rs',
     '        if !opts.contains(v) {',
     '        if false && !opts.contains(v) {',
     'axon-loop', _CS, 'an_enum_spelled_as_a_variant_map_is_never_recorded'),
    ('M1212', "EVIDENCE (4b, integrate-A) D2: a const spelled as a variant map is refused by the schema's const", 'crates/axon-loop-contracts/src/schema.rs',
     '        if c != v {',
     '        if false && c != v {',
     'axon-loop', _CS, 'a_policy_mode_spelled_as_a_variant_map_is_never_stored'),
    ('M1213', 'EVIDENCE (4b, integrate-A): a value matching no anyOf branch is refused', 'crates/axon-loop-contracts/src/schema.rs',
     '        if !subs.iter().any(|sub| walk(sub, v, path).is_ok()) {',
     '        if false && !subs.iter().any(|sub| walk(sub, v, path).is_ok()) {',
     'axon-loop', _CS, 'a_plan_with_zero_independent_units_is_never_registered'),
    ('M1214', 'EVIDENCE (4b, integrate-A): a value matching no oneOf branch is refused (EQUIVALENT: M1234)', 'crates/axon-loop-contracts/src/schema.rs',
     '        if n != 1 {',
     '        if false && n != 1 {',
     'axon-loop', _CS, 'an_acf_request_with_an_empty_approval_is_never_counted'),
    ('M1215', 'EVIDENCE (4b, integrate-A): a string under minLength is refused', 'crates/axon-loop-contracts/src/schema.rs',
     '        if n < min {',
     '        if false && n < min {',
     'axon-loop', _CS, 'a_plan_with_an_empty_independent_unit_is_never_registered'),
    ('M1216', 'EVIDENCE (4b, integrate-A): a string over maxLength is refused', 'crates/axon-loop-contracts/src/schema.rs',
     '        if n > max {',
     '        if false && n > max {',
     'axon-loop', _CS, 'a_plan_with_an_independent_unit_over_512_characters_is_never_registered'),
    ('M1217', 'EVIDENCE (4b, integrate-A): a string not matching its pattern is refused (EQUIVALENT: M1233)', 'crates/axon-loop-contracts/src/schema.rs',
     '        if !ok {',
     '        if false && !ok {',
     'axon-loop', _CS, 'an_acf_reference_of_another_scheme_is_never_recorded'),
    ('M1218', 'EVIDENCE (4b, integrate-A): an integer under its minimum is refused', 'crates/axon-loop-contracts/src/schema.rs',
     '        if x < min {',
     '        if false && x < min {',
     'axon-loop', _CS, 'a_plan_with_zero_independent_units_is_never_registered'),
    ('M1219', 'EVIDENCE (4b, integrate-A): an integer over its maximum is refused (EQUIVALENT: M1204+M1240)', 'crates/axon-loop-contracts/src/schema.rs',
     '        if x > max {',
     '        if false && x > max {',
     'axon-loop', _CS, 'an_acf_request_with_a_limit_past_two_to_the_53_is_never_counted'),
    ('M1220', 'EVIDENCE (4b, integrate-A): an array under minItems is refused (EQUIVALENT: M1238)', 'crates/axon-loop-contracts/src/schema.rs',
     '        if a.len() < min {',
     '        if false && a.len() < min {',
     'axon-loop', _CS, 'an_episode_naming_no_attempt_is_never_recorded'),
    ('M1221', 'EVIDENCE (4b, integrate-A): an array over maxItems is refused', 'crates/axon-loop-contracts/src/schema.rs',
     '        if a.len() > max {',
     '        if false && a.len() > max {',
     'axon-loop', _CS, 'a_plan_with_more_than_256_live_evidence_refs_is_never_registered'),
    ('M1222', 'EVIDENCE (4b, integrate-A): a uniqueItems array with a duplicate is refused', 'crates/axon-loop-contracts/src/schema.rs',
     '            if a[..i].contains(x) {',
     '            if false && a[..i].contains(x) {',
     'axon-loop', _CS, 'a_plan_naming_a_live_evidence_ref_twice_is_never_registered'),
    ('M1223', 'EVIDENCE (4b, integrate-A): a missing required field is refused by the schema (EQUIVALENT: M1225)', 'crates/axon-loop-contracts/src/schema.rs',
     '            if !o.contains_key(r) {',
     '            if false && !o.contains_key(r) {',
     'axon-loop', _CS, 'an_episode_missing_a_required_field_is_never_recorded'),
    ('M1224', 'EVIDENCE (4b, integrate-A): a field no schema names is refused by the schema (EQUIVALENT: M1226)', 'crates/axon-loop-contracts/src/schema.rs',
     '                if !props.is_some_and(|p| p.contains_key(k)) {',
     '                if false && !props.is_some_and(|p| p.contains_key(k)) {',
     'axon-loop', _CS, 'an_episode_with_a_field_no_schema_names_is_never_recorded'),
    ('M1225', 'EVIDENCE (4b, integrate-A): the typed layer refuses a missing field (no implicit default) (EQUIVALENT: M1223)', 'crates/axon-loop-contracts/src/episode.rs',
     '    #[serde(deserialize_with = "nullable")]\n    pub projection_ref: Option<Ref>,',
     '    #[serde(default, deserialize_with = "nullable")]\n    pub projection_ref: Option<Ref>,',
     'axon-loop', _CS, 'an_episode_missing_a_required_field_is_never_recorded'),
    ('M1226', 'EVIDENCE (4b, integrate-A): the typed layer refuses an unknown field (deny_unknown_fields) (EQUIVALENT: M1224)', 'crates/axon-loop-contracts/src/episode.rs',
     '#[serde(deny_unknown_fields)]\npub struct Usage {',
     '#[serde()]\npub struct Usage {',
     'axon-loop', _CS, 'an_episode_with_a_field_no_schema_names_is_never_recorded'),
    ('M1228', "EVIDENCE (4b, integrate-A): every validated id's constructor (and Deserialize) applies its check", 'crates/axon-loop-contracts/src/ids.rs',
     '                $check(&s).map_err(|why| {\n                    shape(format!(concat!(stringify!($name), " {:?}: {}"), s, why))\n                })?;',
     '                let _ = $check(&s).map_err(|why| {\n                    shape(format!(concat!(stringify!($name), " {:?}: {}"), s, why))\n                });',
     'axon-loop', _CS, 'a_candidate_id_outside_the_charset_is_never_registered'),
    ('M1229', 'EVIDENCE (4b, integrate-A): an id is 1..=128 bytes', 'crates/axon-loop-contracts/src/ids.rs',
     '    if b.is_empty() || b.len() > 128 {',
     '    if b.is_empty() || false && b.len() > 128 {',
     'axon-loop', _CS, 'a_candidate_id_over_128_bytes_is_never_registered'),
    ('M1230', 'EVIDENCE (4b, integrate-A): an id starts alphanumeric', 'crates/axon-loop-contracts/src/ids.rs',
     '    if !b[0].is_ascii_alphanumeric() {',
     '    if false && !b[0].is_ascii_alphanumeric() {',
     'axon-loop', _CS, 'a_candidate_id_not_starting_alphanumeric_is_never_registered'),
    ('M1231', "EVIDENCE (4b, integrate-A): an id's charset is [A-Za-z0-9._:-]", 'crates/axon-loop-contracts/src/ids.rs',
     '    if !b\n        .iter()',
     '    if false && !b\n        .iter()',
     'axon-loop', _CS, 'a_candidate_id_outside_the_charset_is_never_registered'),
    ('M1232', 'EVIDENCE (4b, integrate-A): a digest is 64 lowercase hex (EQUIVALENT: M1213)', 'crates/axon-loop-contracts/src/ids.rs',
     "    if h.len() == 64 && h.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {",
     "    if true || h.len() == 64 && h.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {",
     'axon-loop', _CS, 'a_reference_with_uppercase_hex_is_never_recorded'),
    ('M1233', 'EVIDENCE (4b, integrate-A): an ACF reference is acf1: (EQUIVALENT: M1217)', 'crates/axon-loop-contracts/src/ids.rs',
     '        None => Err("expected acf1:<hex>"),',
     '        None => Ok(()),',
     'axon-loop', _CS, 'an_acf_reference_of_another_scheme_is_never_recorded'),
    ('M1234', 'EVIDENCE (4b, integrate-A): an opaque label is 1..=512 characters', 'crates/axon-loop-contracts/src/ids.rs',
     '    if (1..=512).contains(&n) {',
     '    if true || (1..=512).contains(&n) {',
     'axon-loop', _CS, 'an_evaluation_naming_an_empty_subject_issuer_is_never_recorded'),
    ('M1235', 'EVIDENCE (4b, integrate-A): a currency is ^[A-Z]{3}$', 'crates/axon-loop-contracts/src/ids.rs',
     '    if s.len() == 3 && s.bytes().all(|c| c.is_ascii_uppercase()) {',
     '    if true || s.len() == 3 && s.bytes().all(|c| c.is_ascii_uppercase()) {',
     'axon-loop', _CS, 'a_schedule_in_a_currency_that_is_no_iso_code_is_never_pinned'),
    ('M1237', "EVIDENCE (4b, integrate-A): a document's schema tag is its type's (EQUIVALENT: M1212)", 'crates/axon-loop-contracts/src/lib.rs',
     '                if s == $tag {',
     '                if true || s == $tag {',
     'axon-loop', _CS, 'a_policy_of_another_schema_version_is_never_stored'),
    ('M1238', "EVIDENCE (4b, integrate-A): an array's cardinality is the schema's (EQUIVALENT: M1220)", 'crates/axon-loop-contracts/src/lib.rs',
     '    if items.len() < min || items.len() > max {',
     '    if false && (items.len() < min || items.len() > max) {',
     'axon-loop', _CS, 'an_episode_naming_no_attempt_is_never_recorded'),
    ('M1239', 'EVIDENCE (4b, integrate-A): a unique array holds no duplicate (EQUIVALENT: M1222)', 'crates/axon-loop-contracts/src/lib.rs',
     '            if items[..i].contains(a) {',
     '            if false && items[..i].contains(a) {',
     'axon-loop', _CS, 'an_episode_naming_an_attempt_twice_is_never_recorded'),
    ('M1240', 'EVIDENCE (4b, integrate-A): a bounded integer is in min..=2^53-1 (EQUIVALENT: M1218)', 'crates/axon-loop-contracts/src/lib.rs',
     '    if v < min || v > MAX_INTEGER {',
     '    if false && (v < min || v > MAX_INTEGER) {',
     'axon-loop', _CS, 'an_acf_request_with_a_zero_limit_is_never_counted'),
    ('M1242', 'EVIDENCE (4b, integrate-A): an ACF request carries at most 128 arguments (EQUIVALENT: M1221)', 'crates/axon-loop-contracts/src/compute.rs',
     '        if self.argv.len() > 128 {',
     '        if false && self.argv.len() > 128 {',
     'axon-loop', _CS, 'an_acf_request_with_more_than_128_arguments_is_never_counted'),
    ('M1243', 'EVIDENCE (4b, integrate-A): an ACF argument is at most 8192 characters (EQUIVALENT: M1216)', 'crates/axon-loop-contracts/src/compute.rs',
     '        if self.argv.iter().any(|a| a.chars().count() > 8192) {',
     '        if false && self.argv.iter().any(|a| a.chars().count() > 8192) {',
     'axon-loop', _CS, 'an_acf_request_argument_over_8192_characters_is_never_counted'),
    ('M1245', 'EVIDENCE (4b, integrate-A): an unknown usage states no cost (EQUIVALENT: M1210)', 'crates/axon-loop-contracts/src/episode.rs',
     '            UsageState::Unknown if self.cost_micro.is_some() => {',
     '            UsageState::Unknown if false && self.cost_micro.is_some() => {',
     'axon-loop', _CS, 'an_unknown_usage_with_a_cost_is_never_recorded'),
    ('M1246', 'EVIDENCE (4b, integrate-A): a final usage states a known cost (EQUIVALENT: M1210)', 'crates/axon-loop-contracts/src/episode.rs',
     '            UsageState::Final if self.cost_micro.is_none() => Err(shape(',
     '            UsageState::Final if false && self.cost_micro.is_none() => Err(shape(',
     'axon-loop', _CS, 'a_final_usage_without_a_cost_is_never_recorded'),
    ('M1247', 'EVIDENCE (4b, integrate-A): a final usage leaves no liability (EQUIVALENT: M1208)', 'crates/axon-loop-contracts/src/episode.rs',
     '            UsageState::Final if self.unresolved_liability_micro != 0 => Err(shape(',
     '            UsageState::Final if false && self.unresolved_liability_micro != 0 => Err(shape(',
     'axon-loop', _CS, 'a_final_usage_with_liability_is_never_recorded'),
    ('M1248', 'EVIDENCE (4b, integrate-A): a passed episode completed (EQUIVALENT: M1209)', 'crates/axon-loop-contracts/src/episode.rs',
     '            if self.status != EpisodeStatus::Completed {',
     '            if false && self.status != EpisodeStatus::Completed {',
     'axon-loop', _CS, 'a_passed_episode_that_did_not_complete_is_never_counted'),
    ('M1249', 'EVIDENCE (4b, integrate-A): a passed episode matched a check (EQUIVALENT: M1218)', 'crates/axon-loop-contracts/src/episode.rs',
     '            if v.matched_checks == 0 {',
     '            if false && v.matched_checks == 0 {',
     'axon-loop', _CS, 'a_passed_episode_with_no_matched_check_is_never_counted'),
    ('M1250', 'EVIDENCE (4b, integrate-A): a passed episode names its checked output, issuer and verifier (EQUIVALENT: M1269)', 'crates/axon-loop-contracts/src/episode.rs',
     '            if self.output_workspace_ref.is_none()\n                || v.output_workspace_ref.is_none()\n                || v.issuer_ref.is_none()\n                || v.verifier_ref.is_none()\n            {',
     '            if false\n                && (self.output_workspace_ref.is_none()\n                    || v.output_workspace_ref.is_none()\n                    || v.issuer_ref.is_none()\n                    || v.verifier_ref.is_none())\n            {',
     'axon-loop', _CS, 'a_passed_episode_with_no_checked_output_is_never_counted'),
    ('M1251', 'EVIDENCE (4b, integrate-A): a passed episode cites evidence (EQUIVALENT: M1220)', 'crates/axon-loop-contracts/src/episode.rs',
     '            if v.evidence_refs.is_empty() {',
     '            if false && v.evidence_refs.is_empty() {',
     'axon-loop', _CS, 'a_passed_episode_with_no_evidence_is_never_counted'),
    ('M1252', 'EVIDENCE (4b, integrate-A): an outcome-unknown episode carries no verdict (EQUIVALENT: M1211)', 'crates/axon-loop-contracts/src/episode.rs',
     '        if self.status == EpisodeStatus::OutcomeUnknown\n',
     '        if false && self.status == EpisodeStatus::OutcomeUnknown\n',
     'axon-loop', _CS, 'an_outcome_unknown_episode_with_a_verdict_is_never_recorded'),
    ('M1254', 'EVIDENCE (4b, integrate-A): a receipt cites at most 128 evidence refs (EQUIVALENT: M1221)', 'crates/axon-loop-contracts/src/receipt.rs',
     '        if self.evidence_refs.len() > 128 {',
     '        if false && self.evidence_refs.len() > 128 {',
     'axon-loop', _CS, 'a_receipt_with_more_than_128_evidence_refs_is_never_counted'),
    ('M1255', 'EVIDENCE (4b, integrate-A): a passed receipt completed (EQUIVALENT: M1227)', 'crates/axon-loop-contracts/src/receipt.rs',
     '            if self.status != ReceiptStatus::Completed {',
     '            if false && self.status != ReceiptStatus::Completed {',
     'axon-loop', _CS, 'a_passed_receipt_that_did_not_complete_is_never_counted'),
    ('M1256', 'EVIDENCE (4b, integrate-A): a passed receipt exited 0 (EQUIVALENT: M1236)', 'crates/axon-loop-contracts/src/receipt.rs',
     '            if self.process_exit_code != Some(0) {',
     '            if false && self.process_exit_code != Some(0) {',
     'axon-loop', _CS, 'a_passed_receipt_with_a_nonzero_exit_is_never_counted'),
    ('M1257', 'EVIDENCE (4b, integrate-A): a passed receipt matched a check (EQUIVALENT: M1218)', 'crates/axon-loop-contracts/src/receipt.rs',
     '            if !matches!(self.matched_checks, Some(n) if n >= 1) {',
     '            if false && !matches!(self.matched_checks, Some(n) if n >= 1) {',
     'axon-loop', _CS, 'a_passed_receipt_with_no_matched_check_is_never_counted'),
    ('M1258', "EVIDENCE (4b, integrate-A): a passed receipt's evidence was supervisor-observed (EQUIVALENT: M1241)", 'crates/axon-loop-contracts/src/receipt.rs',
     '            if self.evidence_source != EvidenceSource::SupervisorObserved {',
     '            if false && self.evidence_source != EvidenceSource::SupervisorObserved {',
     'axon-loop', _CS, 'a_passed_receipt_reported_by_the_worker_is_never_counted'),
    ('M1259', 'EVIDENCE (4b, integrate-A): a passed receipt cites evidence (EQUIVALENT: M1220)', 'crates/axon-loop-contracts/src/receipt.rs',
     '            if self.evidence_refs.is_empty() {',
     '            if false && self.evidence_refs.is_empty() {',
     'axon-loop', _CS, 'a_passed_receipt_with_no_evidence_is_never_counted'),
    ('M1260', 'EVIDENCE (4b, integrate-A): an outcome-unknown receipt carries no verdict (EQUIVALENT: M1211)', 'crates/axon-loop-contracts/src/receipt.rs',
     '            if !matches!(\n                self.verification,',
     '            if false && !matches!(\n                self.verification,',
     'axon-loop', _CS, 'an_outcome_unknown_receipt_with_a_verdict_is_never_counted'),
    ('M1261', 'EVIDENCE (4b, integrate-A): an outcome-unknown receipt has no exit code (EQUIVALENT: M1244)', 'crates/axon-loop-contracts/src/receipt.rs',
     '            if self.process_exit_code.is_some() {',
     '            if false && self.process_exit_code.is_some() {',
     'axon-loop', _CS, 'an_outcome_unknown_receipt_with_an_exit_code_is_never_counted'),
    ('M1262', 'EVIDENCE (4b, integrate-A): an unknown receipt usage states no cost (EQUIVALENT: M1253)', 'crates/axon-loop-contracts/src/receipt.rs',
     '        if self.usage_state == ReceiptUsageState::Unknown && self.cost_micro.is_some() {',
     '        if false && self.usage_state == ReceiptUsageState::Unknown && self.cost_micro.is_some() {',
     'axon-loop', _CS, 'a_receipt_with_unknown_usage_and_a_cost_is_never_counted'),
    ('M1265', 'EVIDENCE (4b, integrate-A): a pause names no target policy (EQUIVALENT: M1263)', 'crates/axon-loop-contracts/src/policy.rs',
     '                if self.target_policy_ref.is_some() {',
     '                if false && self.target_policy_ref.is_some() {',
     'axon-loop', _CS, 'a_pause_naming_a_target_is_never_applied'),
    ('M1266', "EVIDENCE (4b, integrate-A): a transition's fence is contiguous (next = expected + 1) (EQUIVALENT: M1330)", 'crates/axon-loop-contracts/src/policy.rs',
     '        if self.next_epoch.get() != self.expected_epoch.get() + 1 {',
     '        if false && self.next_epoch.get() != self.expected_epoch.get() + 1 {',
     'axon-loop', _CS, 'a_transition_that_skips_an_epoch_is_never_applied'),
    ('M1267', "EVIDENCE (4b, integrate-A): a transition's next epoch is at least 1 (EQUIVALENT: M1218+M1266+M1330)", 'crates/axon-loop-contracts/src/policy.rs',
     '        if self.next_epoch.get() < 1 {',
     '        if false && self.next_epoch.get() < 1 {',
     'axon-loop', _CS, 'a_transition_to_epoch_zero_is_never_applied'),
    ('M1208', "EVIDENCE (4b, integrate-A): the episode schema: a final usage leaves no liability (SIBLING-ONLY: a member of M1247's set)", 'crates/axon-loop-contracts/schemas/closed-loop-episode.schema.json',
     '              "unresolved_liability_micro": {\n                "const": 0\n              }',
     '              "unresolved_liability_micro": {}',
     'axon-loop', _CS, 'a_final_usage_with_liability_is_never_recorded'),
    ('M1209', "EVIDENCE (4b, integrate-A): the episode schema: a passed episode completed (SIBLING-ONLY: a member of M1248's set)", 'crates/axon-loop-contracts/schemas/closed-loop-episode.schema.json',
     '      "then": {\n        "properties": {\n          "status": {\n            "const": "completed"\n          },\n          "output_workspace_ref": {',
     '      "then": {\n        "properties": {\n          "status": {},\n          "output_workspace_ref": {',
     'axon-loop', _CS, 'a_passed_episode_that_did_not_complete_is_never_counted'),
    ('M1227', "EVIDENCE (4b, integrate-A): the receipt schema: a passed receipt completed (SIBLING-ONLY: a member of M1255's set)", 'crates/axon-loop-contracts/schemas/acf-execution-receipt.schema.json',
     '          "status": {\n            "const": "completed"\n          },\n          "process_exit_code": {',
     '          "status": {},\n          "process_exit_code": {',
     'axon-loop', _CS, 'a_passed_receipt_that_did_not_complete_is_never_counted'),
    ('M1236', "EVIDENCE (4b, integrate-A): the receipt schema: a passed receipt exited 0 (SIBLING-ONLY: a member of M1256's set)", 'crates/axon-loop-contracts/schemas/acf-execution-receipt.schema.json',
     '          "process_exit_code": {\n            "const": 0\n          },',
     '          "process_exit_code": {},',
     'axon-loop', _CS, 'a_passed_receipt_with_a_nonzero_exit_is_never_counted'),
    ('M1241', "EVIDENCE (4b, integrate-A): the receipt schema: a passed receipt's evidence was supervisor-observed (SIBLING-ONLY: a member of M1258's set)", 'crates/axon-loop-contracts/schemas/acf-execution-receipt.schema.json',
     '          "evidence_source": {\n            "const": "supervisor_observed"\n          },',
     '          "evidence_source": {},',
     'axon-loop', _CS, 'a_passed_receipt_reported_by_the_worker_is_never_counted'),
    ('M1244', "EVIDENCE (4b, integrate-A): the receipt schema: an outcome-unknown receipt has no exit code (SIBLING-ONLY: a member of M1261's set)", 'crates/axon-loop-contracts/schemas/acf-execution-receipt.schema.json',
     '          "process_exit_code": {\n            "type": "null"\n          }\n        }\n      }\n    },',
     '          "process_exit_code": {}\n        }\n      }\n    },',
     'axon-loop', _CS, 'an_outcome_unknown_receipt_with_an_exit_code_is_never_counted'),
    ('M1253', "EVIDENCE (4b, integrate-A): the receipt schema: an unknown usage states no cost (SIBLING-ONLY: a member of M1262's set)", 'crates/axon-loop-contracts/schemas/acf-execution-receipt.schema.json',
     '          "cost_micro": {\n            "type": "null"\n          }',
     '          "cost_micro": {}',
     'axon-loop', _CS, 'a_receipt_with_unknown_usage_and_a_cost_is_never_counted'),
    ('M1263', "EVIDENCE (4b, integrate-A): the transition schema: a pause names no target (SIBLING-ONLY: a member of M1265's set)", 'crates/axon-loop-contracts/schemas/closed-loop-transition.schema.json',
     '          "target_policy_ref": {\n            "type": "null"\n          }',
     '          "target_policy_ref": {}',
     'axon-loop', _CS, 'a_pause_naming_a_target_is_never_applied'),
    ('M1269', "EVIDENCE (4b, integrate-A): the episode schema: a passed episode names its checked output (SIBLING-ONLY: a member of M1250's set)", 'crates/axon-loop-contracts/schemas/closed-loop-episode.schema.json',
     '              "output_workspace_ref": {\n                "type": "string",\n                "pattern": "^acf1:[0-9a-f]{64}$"\n              },\n              "evidence_refs": {',
     '              "output_workspace_ref": {},\n              "evidence_refs": {',
     'axon-loop', _CS, 'a_passed_episode_with_no_checked_output_is_never_counted'),
]
# INTEGRATE-A retirements (four-cell, scripts/v022_paired_disable.py --only=...).
EQUIV_RECORD['M1202'] = {
    "property": 'a value nested past MAX_DEPTH is never read',
    "subsumed_by": ['M1201'], "killer": 'joint:M1202+M1201',
    "all_paths": 'a value at depth d > 32 lies inside d >= 33 containers, so the text holds 33 open brackets before it and bounded_depth (M1201) refuses every parsed text this walk refuses; on a value the loop BUILDS (canonical_bytes of a record), the record is read back only through parse_value (strict_record, get_contract, the ledger), whose pre-scan refuses the same nesting'}
EQUIV_RECORD['M1203'] = {
    "property": "an object's keys count a nesting level",
    "subsumed_by": ['M1202', 'M1201'], "killer": 'joint:M1203+M1202+M1201',
    "all_paths": 'a non-empty object at depth 32 has a value at depth 33, which the value rule (M1202) refuses in the same walk on every path, parsed or built; on parsed text the pre-scan (M1201) also refuses its 33 open brackets'}
EQUIV_RECORD['M1207'] = {
    "property": 'a canonical form over MAX_BYTES is never digested',
    "subsumed_by": ['M1200'], "killer": 'joint:M1207+M1200',
    "all_paths": "the canonical form of a parsed value is never longer than its text (no whitespace, sorted keys, escapes no longer than the input's, -0 -> 0), so parse_value's limit (M1200) refuses every parsed value this refuses; a record the loop builds over the limit is refused where it is read back, by parse_value (M1200)"}
EQUIV_RECORD['M1214'] = {
    "property": 'a value matching no oneOf branch is refused',
    "subsumed_by": ['M1234'], "killer": 'joint:M1214+M1234',
    "all_paths": "the schema walk runs only inside axon_loop_contracts::parse (and plan.rs's pilot schema, whose PilotPlan fields this keyword does not bound without the typed twin), and on the same value typed serde and validate() then run; every checked-in schema bound of this keyword is restated by the typed layer (lib.rs check_array/check_int, the ids.rs newtypes, the contract's validate()), which refuses the same values; the oneOf members in the checked-in schemas are a bounded string or null, both restated by Option<OpaqueRef>/Option<Acf1Ref> (check_opaque, check_acf1)"}
EQUIV_RECORD['M1217'] = {
    "property": 'a string not matching its schema pattern is refused',
    "subsumed_by": ['M1233'], "killer": 'joint:M1217+M1233',
    "all_paths": "the schema walk runs only inside axon_loop_contracts::parse (and plan.rs's pilot schema, whose PilotPlan fields this keyword does not bound without the typed twin), and on the same value typed serde and validate() then run; every checked-in schema bound of this keyword is restated by the typed layer (lib.rs check_array/check_int, the ids.rs newtypes, the contract's validate()), which refuses the same values; the five patterns are restated by TaskId-family check_id, check_ref, check_acf1, check_currency and the profile id (profile.rs, OUT_OF_SCOPE)"}
EQUIV_RECORD['M1219'] = {
    "property": 'an integer over its schema maximum is refused',
    "subsumed_by": ['M1204', 'M1240'], "killer": 'joint:M1219+M1204+M1240',
    "all_paths": 'every maximum in the checked-in schemas is 2^53-1, which parse_value (M1204) enforces on every number before the walk, or 255 for process_exit_code, typed u8 (serde refuses more); the typed check_int (M1240) restates 2^53-1'}
EQUIV_RECORD['M1220'] = {
    "property": 'an array under its schema minItems is refused',
    "subsumed_by": ['M1238'], "killer": 'joint:M1220+M1238',
    "all_paths": "the schema walk runs only inside axon_loop_contracts::parse (and plan.rs's pilot schema, whose PilotPlan fields this keyword does not bound without the typed twin), and on the same value typed serde and validate() then run; every checked-in schema bound of this keyword is restated by the typed layer (lib.rs check_array/check_int, the ids.rs newtypes, the contract's validate()), which refuses the same values (check_array's minimum; the passed-verdict conditionals by episode.rs/receipt.rs)"}
EQUIV_RECORD['M1223'] = {
    "property": 'a missing required field is refused',
    "subsumed_by": ['M1225'], "killer": 'joint:M1223+M1225',
    "all_paths": 'every required field of every checked-in schema is a non-defaulted field of its contract type (no #[serde(default)] in the contract crate), so typed serde refuses it missing; the row M1225 gives one field a default to show the two are jointly load-bearing'}
EQUIV_RECORD['M1224'] = {
    "property": 'a field no schema names is refused',
    "subsumed_by": ['M1226'], "killer": 'joint:M1224+M1226',
    "all_paths": 'every additionalProperties:false object of the checked-in schemas is a deny_unknown_fields struct of its contract type, so typed serde refuses the same field; the row M1226 drops deny_unknown_fields from Usage to show the two are jointly load-bearing'}
EQUIV_RECORD['M1225'] = {
    "property": 'the typed layer refuses a missing field (no implicit default)',
    "subsumed_by": ['M1223'], "killer": 'joint:M1225+M1223',
    "all_paths": 'a contract document is typed only after the schema walk, whose `required` (M1223) lists every field of every contract type'}
EQUIV_RECORD['M1226'] = {
    "property": 'the typed layer refuses an unknown field',
    "subsumed_by": ['M1224'], "killer": 'joint:M1226+M1224',
    "all_paths": 'a contract document is typed only after the schema walk, whose additionalProperties:false (M1224) closes every contract object'}
EQUIV_RECORD['M1232'] = {
    "property": 'a digest is 64 lowercase hex',
    "subsumed_by": ['M1213'], "killer": 'joint:M1232+M1213',
    "all_paths": "in a contract document every Ref/Acf1Ref field is also bounded by the schema's anyOf/pattern (M1213, M1217) on the same value; a Ref read by strict_record (evl, assign, admit requests) is compared for equality with a store-computed lowercase digest or names a CAS file by its hex, neither of which a non-lowercase value equals or finds"}
EQUIV_RECORD['M1233'] = {
    "property": 'an ACF reference is acf1:',
    "subsumed_by": ['M1217'], "killer": 'joint:M1233+M1217',
    "all_paths": 'every Acf1Ref field of a contract document is bounded by the schema pattern ^acf1: (M1217) on the same value; Acf1Ref is not read outside contract documents'}
EQUIV_RECORD['M1237'] = {
    "property": "a document's schema tag is its type's",
    "subsumed_by": ['M1212'], "killer": 'joint:M1237+M1212',
    "all_paths": "every contract's schema tag field is a const in its checked-in schema (M1212) on the same value; a record tag read by strict_record is a version label of a deny_unknown_fields type whose bytes are read back by digest"}
EQUIV_RECORD['M1238'] = {
    "property": "an array's cardinality is the schema's",
    "subsumed_by": ['M1220'], "killer": 'joint:M1238+M1220',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the schema's minItems/maxItems (M1220, M1221) bound the same arrays"}
EQUIV_RECORD['M1239'] = {
    "property": 'a unique array holds no duplicate',
    "subsumed_by": ['M1222'], "killer": 'joint:M1239+M1222',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the schema's uniqueItems (M1222) bounds the same arrays"}
EQUIV_RECORD['M1240'] = {
    "property": 'a bounded integer is in min..=2^53-1',
    "subsumed_by": ['M1218'], "killer": 'joint:M1240+M1218',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the schema's minimum (M1218) and parse_value's integer rule (M1204) bound the same fields"}
EQUIV_RECORD['M1242'] = {
    "property": 'an ACF request carries at most 128 arguments',
    "subsumed_by": ['M1221'], "killer": 'joint:M1242+M1221',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the schema's argv maxItems 128 (M1221)"}
EQUIV_RECORD['M1243'] = {
    "property": 'an ACF argument is at most 8192 characters',
    "subsumed_by": ['M1216'], "killer": 'joint:M1243+M1216',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the schema's argv items maxLength 8192 (M1216)"}
EQUIV_RECORD['M1245'] = {
    "property": 'an unknown usage states no cost',
    "subsumed_by": ['M1210'], "killer": 'joint:M1245+M1210',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the episode schema's conditional (usage.state unknown -> cost_micro type null, M1210)"}
EQUIV_RECORD['M1246'] = {
    "property": 'a final usage states a known cost',
    "subsumed_by": ['M1210'], "killer": 'joint:M1246+M1210',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the episode schema's conditional (final -> cost_micro type integer, M1210)"}
EQUIV_RECORD['M1247'] = {
    "property": 'a final usage leaves no liability',
    "subsumed_by": ['M1208'], "killer": 'joint:M1247+M1208',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the episode schema's conditional (final -> unresolved_liability_micro const 0, M1208)"}
EQUIV_RECORD['M1248'] = {
    "property": 'a passed episode completed',
    "subsumed_by": ['M1209'], "killer": 'joint:M1248+M1209',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the episode schema's conditional (passed -> status const completed, M1209)"}
EQUIV_RECORD['M1249'] = {
    "property": 'a passed episode matched a check',
    "subsumed_by": ['M1218'], "killer": 'joint:M1249+M1218',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the episode schema's conditional (passed -> matched_checks minimum 1, M1218)"}
EQUIV_RECORD['M1250'] = {
    "property": 'a passed episode names its checked output, issuer and verifier',
    "subsumed_by": ['M1269'], "killer": 'joint:M1250+M1269',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the episode schema's conditional (passed -> each a string; the checked output's clause is M1269)"}
EQUIV_RECORD['M1251'] = {
    "property": 'a passed episode cites evidence',
    "subsumed_by": ['M1220'], "killer": 'joint:M1251+M1220',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the episode schema's conditional (passed -> evidence_refs minItems 1, M1220)"}
EQUIV_RECORD['M1252'] = {
    "property": 'an outcome-unknown episode carries no verdict',
    "subsumed_by": ['M1211'], "killer": 'joint:M1252+M1211',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the episode schema's conditional (outcome_unknown -> result enum not_run/unknown, M1211)"}
EQUIV_RECORD['M1254'] = {
    "property": 'a receipt cites at most 128 evidence refs',
    "subsumed_by": ['M1221'], "killer": 'joint:M1254+M1221',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the receipt schema's evidence_refs maxItems 128 (M1221)"}
EQUIV_RECORD['M1255'] = {
    "property": 'a passed receipt completed',
    "subsumed_by": ['M1227'], "killer": 'joint:M1255+M1227',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the receipt schema's conditional (passed -> status const completed, M1227)"}
EQUIV_RECORD['M1256'] = {
    "property": 'a passed receipt exited 0',
    "subsumed_by": ['M1236'], "killer": 'joint:M1256+M1236',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the receipt schema's conditional (passed -> process_exit_code const 0, M1236)"}
EQUIV_RECORD['M1257'] = {
    "property": 'a passed receipt matched a check',
    "subsumed_by": ['M1218'], "killer": 'joint:M1257+M1218',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the receipt schema's conditional (passed -> matched_checks minimum 1, M1218)"}
EQUIV_RECORD['M1258'] = {
    "property": "a passed receipt's evidence was supervisor-observed",
    "subsumed_by": ['M1241'], "killer": 'joint:M1258+M1241',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the receipt schema's conditional (passed -> evidence_source const, M1241)"}
EQUIV_RECORD['M1259'] = {
    "property": 'a passed receipt cites evidence',
    "subsumed_by": ['M1220'], "killer": 'joint:M1259+M1220',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the receipt schema's conditional (passed -> evidence_refs minItems 1, M1220)"}
EQUIV_RECORD['M1260'] = {
    "property": 'an outcome-unknown receipt carries no verdict',
    "subsumed_by": ['M1211'], "killer": 'joint:M1260+M1211',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the receipt schema's conditional (outcome_unknown -> verification enum, M1211)"}
EQUIV_RECORD['M1261'] = {
    "property": 'an outcome-unknown receipt has no exit code',
    "subsumed_by": ['M1244'], "killer": 'joint:M1261+M1244',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the receipt schema's conditional (outcome_unknown -> process_exit_code type null, M1244)"}
EQUIV_RECORD['M1262'] = {
    "property": 'an unknown receipt usage states no cost',
    "subsumed_by": ['M1253'], "killer": 'joint:M1262+M1253',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the receipt schema's conditional (usage_state unknown -> cost_micro type null, M1253)"}
EQUIV_RECORD['M1265'] = {
    "property": 'a pause names no target policy',
    "subsumed_by": ['M1263'], "killer": 'joint:M1265+M1263',
    "all_paths": "the typed rule runs only on a value the schema walk already judged: every contract document is admitted through axon_loop_contracts::parse (canonical.rs: parse_value, validate_against the checked-in schema, typed serde, validate()), and validate() is called elsewhere only by tel::join on documents parse admitted and by evo::propose on a candidate it builds by remove/swap from an admitted policy (never larger, never empty, no duplicate, evidence deduplicated and capped at 256 by EVO itself), which this rule never refuses; the transition schema's conditional (pause -> target_policy_ref type null, M1263)"}
EQUIV_RECORD['M1266'] = {
    "property": "a transition's fence is contiguous (next = expected + 1)",
    "subsumed_by": ['M1330'], "killer": 'joint:M1266+M1330',
    "all_paths": 'a PolicyTransition is only ever produced by parse (no constructor in crates/*/src) and only consumed by pointer::transition, which requires expected_epoch == current (CAS) and next_epoch == current + 1 (M1330) before it applies anything, together exactly this rule'}
EQUIV_RECORD['M1267'] = {
    "property": "a transition's next epoch is at least 1",
    "subsumed_by": ['M1218', 'M1266', 'M1330'], "killer": 'joint:M1267+M1218+M1266+M1330',
    "all_paths": "next_epoch = expected_epoch + 1 >= 1 whenever the fence holds (M1266, M1330 at the pointer), and the schema's minimum 1 (M1218) bounds the same field"}
EQUIVALENT_DID |= {'M1257', 'M1254', 'M1226', 'M1245', 'M1217', 'M1240', 'M1246', 'M1259', 'M1248', 'M1242', 'M1262', 'M1207', 'M1238', 'M1237', 'M1256', 'M1225', 'M1261', 'M1203', 'M1232', 'M1243', 'M1252', 'M1220', 'M1250', 'M1251', 'M1260', 'M1223', 'M1219', 'M1258', 'M1233', 'M1249', 'M1202', 'M1247', 'M1265', 'M1214', 'M1224', 'M1239', 'M1266', 'M1267', 'M1255'}
RETIRED |= {'M1257', 'M1254', 'M1226', 'M1245', 'M1217', 'M1240', 'M1246', 'M1259', 'M1248', 'M1242', 'M1262', 'M1207', 'M1238', 'M1237', 'M1256', 'M1225', 'M1261', 'M1203', 'M1232', 'M1243', 'M1252', 'M1220', 'M1250', 'M1251', 'M1260', 'M1223', 'M1219', 'M1258', 'M1233', 'M1249', 'M1202', 'M1247', 'M1265', 'M1214', 'M1224', 'M1239', 'M1266', 'M1267', 'M1255'}
# ── end INTEGRATE-A ──
# ── C9 round 4b, INTEGRATE-B (amendment 64) ── checks.rs, the cross-document joins.
# M1270-M1293: each refusal is the FIRST on its production route (evl::evaluate for the
# context preflight and the ACF join, intake_episode for the episode join, policy put for the
# shortlist) and is killed there (crates/axon-loop/tests/checks_sites.rs). M1294/M1296 are
# four-cell retirements on the production route. M1295/M1297/M1298/M1299 are LIBRARY-PRIMITIVE
# rows: dominated on every production route (four cells hold there with the named sibling), but
# the primitive's own direct test fails with the guard removed alone, so the full-suite
# condition forbids retiring them; their kill is the direct test (flagged for the integrator).
MUTATIONS += [
    ('M1270', 'checks (4b): a shortlist never names a candidate outside the registered list (policy put)', 'crates/axon-loop-contracts/src/checks.rs', '    if let Some(c) = policy.shortlist.iter().find(|c| !eligible.contains(*c)) {\n', '    if let Some(c) = policy.shortlist.iter().find(|c| false && !eligible.contains(*c)) {\n', 'axon-loop', '--test checks_sites', 'a_policy_shortlisting_an_unregistered_candidate_is_never_stored'),
    ('M1271', 'checks (4b): a context grants only concrete paths (no backslash/NUL/empty)', 'crates/axon-loop-contracts/src/checks.rs', "    if path.is_empty() || path.contains('\\\\') || path.contains('\\0') {\n", "    if false && (path.is_empty() || path.contains('\\\\') || path.contains('\\0')) {\n", 'axon-loop', '--test checks_sites', 'a_context_granting_a_backslash_path_never_counts'),
    ('M1272', 'checks (4b): a context grants no pattern or drive path', 'crates/axon-loop-contracts/src/checks.rs', "    if path.contains(['*', '?', '[', ']', ':']) {\n", "    if false && path.contains(['*', '?', '[', ']', ':']) {\n", 'axon-loop', '--test checks_sites', 'a_context_granting_a_pattern_never_counts'),
    ('M1273', 'checks (4b): a context grants no absolute or noncanonical relative path', 'crates/axon-loop-contracts/src/checks.rs', '        return Err(semantic(format!(\n            "unsafe or noncanonical relative path {path:?}"\n        )));\n', '        let _ = format!("unsafe or noncanonical relative path {path:?}");\n', 'axon-loop', '--test checks_sites', 'a_context_granting_a_parent_path_never_counts'),
    ('M1274', "checks (4b): a trial counts only inside its context's validity window", 'crates/axon-loop-contracts/src/checks.rs', '    if !(ctx.created_ms <= now_ms && now_ms < ctx.expires_ms) {\n', '    if false && (!(ctx.created_ms <= now_ms && now_ms < ctx.expires_ms)) {\n', 'axon-loop', '--test checks_sites', 'an_expired_context_never_counts'),
    ('M1275', 'checks (4b): a preflight observed by its expecting parent is no observation', 'crates/axon-loop-contracts/src/checks.rs', '    if ctx.observed_issuer_ref == ctx.expected_issuer_ref {\n', '    if false && ctx.observed_issuer_ref == ctx.expected_issuer_ref {\n', 'axon-loop', '--test checks_sites', 'a_context_its_own_parent_observed_never_counts'),
    ('M1276', 'checks (4b): a preflight counts only from a recognized observer', 'crates/axon-loop-contracts/src/checks.rs', '    if !trusted_observers.contains(&ctx.observed_issuer_ref) {\n', '    if false && !trusted_observers.contains(&ctx.observed_issuer_ref) {\n', 'axon-loop', '--test checks_sites', 'a_context_observed_by_an_unrecognized_observer_never_counts'),
    ('M1277', 'checks (4b): the primary integration checkout is never a trial', 'crates/axon-loop-contracts/src/checks.rs', '    if o.is_primary_worktree {\n', '    if false && o.is_primary_worktree {\n', 'axon-loop', '--test checks_sites', 'a_trial_in_the_primary_checkout_never_counts'),
    ('M1278', 'checks (4b): a critic/verifier role writes nothing', 'crates/axon-loop-contracts/src/checks.rs', '    if matches!(o.role, Role::Critic | Role::Verifier) && !o.write_paths.is_empty() {\n', '    if false && (matches!(o.role, Role::Critic | Role::Verifier) && !o.write_paths.is_empty()) {\n', 'axon-loop', '--test checks_sites', 'a_critic_that_writes_never_counts'),
    ('M1279', 'checks (4b): an implementation role has an explicit write set', 'crates/axon-loop-contracts/src/checks.rs', '    if matches!(o.role, Role::Implementation | Role::Documentation) && o.write_paths.is_empty() {\n', '    if false && (matches!(o.role, Role::Implementation | Role::Documentation) && o.write_paths.is_empty()) {\n', 'axon-loop', '--test checks_sites', 'an_implementation_with_no_write_set_never_counts'),
    ('M1280', 'checks (4b): an episode binds only the context of its own trial identity', 'crates/axon-loop-contracts/src/checks.rs', '    if episode.identity != ctx.identity {\n', '    if false && episode.identity != ctx.identity {\n', 'axon-loop', '--test checks_sites', 'an_episode_bound_to_another_trials_context_never_counts'),
    ('M1281', 'checks (4b): an episode counts only at the current authority epoch', 'crates/axon-loop-contracts/src/checks.rs', '    if episode.authority_epoch != current_epoch || ctx.authority_epoch != current_epoch {\n', '    if false && (episode.authority_epoch != current_epoch || ctx.authority_epoch != current_epoch) {\n', 'axon-loop', '--test checks_sites', 'an_episode_of_another_authority_epoch_never_counts'),
    ('M1282', "checks (4b): the ACF projection maps the episode's own policy", 'crates/axon-loop-contracts/src/checks.rs', '    if projection.sidecar_policy_ref != episode.policy_ref {\n', '    if false && projection.sidecar_policy_ref != episode.policy_ref {\n', 'axon-loop', '--test checks_sites', 'a_projection_of_another_policy_never_counts'),
    ('M1283', 'checks (4b): the execution ran under the supervisor policy the projection maps', 'crates/axon-loop-contracts/src/checks.rs', '    if request.policy_digest != projection.acf_policy_digest\n        || receipt.policy_digest != projection.acf_policy_digest\n    {\n', '    if false && (request.policy_digest != projection.acf_policy_digest\n        || receipt.policy_digest != projection.acf_policy_digest)\n    {\n', 'axon-loop', '--test checks_sites', 'a_request_under_another_supervisor_policy_never_counts'),
    ('M1284', 'checks (4b): the execution documents are the ones the episode names', 'crates/axon-loop-contracts/src/checks.rs', '    if episode.acf_request_ref != digest(request)? || episode.acf_receipt_ref != digest(receipt)? {\n', '    if false && (episode.acf_request_ref != digest(request)? || episode.acf_receipt_ref != digest(receipt)?) {\n', 'axon-loop', '--test checks_sites', 'a_request_other_than_the_one_the_episode_names_never_counts'),
    ('M1285', "checks (4b): the execution is of the episode's task", 'crates/axon-loop-contracts/src/checks.rs', '    if request.task_id != id.task_id || receipt.task_id != id.task_id {\n', '    if false && (request.task_id != id.task_id || receipt.task_id != id.task_id) {\n', 'axon-loop', '--test checks_sites', 'a_request_of_another_task_never_counts'),
    ('M1286', "checks (4b): the execution is of the episode's trial", 'crates/axon-loop-contracts/src/checks.rs', '    if request.trial_id != id.trial_id || receipt.trial_id != id.trial_id {\n', '    if false && (request.trial_id != id.trial_id || receipt.trial_id != id.trial_id) {\n', 'axon-loop', '--test checks_sites', 'a_request_of_another_trial_never_counts'),
    ('M1287', "checks (4b): the execution is of the episode's attempt", 'crates/axon-loop-contracts/src/checks.rs', '    if request.attempt_id != id.attempt_id || receipt.attempt_id != id.attempt_id {\n', '    if false && (request.attempt_id != id.attempt_id || receipt.attempt_id != id.attempt_id) {\n', 'axon-loop', '--test checks_sites', 'a_request_of_another_attempt_never_counts'),
    ('M1288', "checks (4b): the execution is of the episode's operation", 'crates/axon-loop-contracts/src/checks.rs', '    if request.operation_id != id.operation_id || receipt.operation_id != id.operation_id {\n', '    if false && (request.operation_id != id.operation_id || receipt.operation_id != id.operation_id) {\n', 'axon-loop', '--test checks_sites', 'a_request_of_another_operation_never_counts'),
    ('M1289', "checks (4b): the receipt is of the episode's execution", 'crates/axon-loop-contracts/src/checks.rs', '    if receipt.execution_id != id.execution_id {\n', '    if false && receipt.execution_id != id.execution_id {\n', 'axon-loop', '--test checks_sites', 'a_receipt_of_another_execution_never_counts'),
    ('M1290', "checks (4b): the execution ran on the episode's input workspace", 'crates/axon-loop-contracts/src/checks.rs', '    if request.workspace_version_ref != episode.input_workspace_ref\n        || receipt.input_workspace_ref != episode.input_workspace_ref\n    {\n', '    if false && (request.workspace_version_ref != episode.input_workspace_ref\n        || receipt.input_workspace_ref != episode.input_workspace_ref)\n    {\n', 'axon-loop', '--test checks_sites', 'an_execution_from_another_input_never_counts'),
    ('M1291', "checks (4b): the execution left the episode's output", 'crates/axon-loop-contracts/src/checks.rs', '    if receipt.output_workspace_ref != episode.output_workspace_ref {\n', '    if false && receipt.output_workspace_ref != episode.output_workspace_ref {\n', 'axon-loop', '--test checks_sites', 'a_failure_resting_on_another_output_never_counts'),
    ('M1292', "checks (4b): the episode's status is the execution receipt's (timed_out is never success)", 'crates/axon-loop-contracts/src/checks.rs', '    if project_receipt_status(receipt.status) != episode.status {\n', '    if false && project_receipt_status(receipt.status) != episode.status {\n', 'axon-loop', '--test checks_sites', 'a_pass_whose_execution_failed_never_counts'),
    ('M1293', 'checks (4b): a completion rests on a supervisor-observed execution receipt', 'crates/axon-loop-contracts/src/checks.rs', '    if episode.status == EpisodeStatus::Completed\n        && receipt.evidence_source', '    if false && episode.status == EpisodeStatus::Completed\n        && receipt.evidence_source', 'axon-loop', '--test checks_sites', 'a_completion_resting_on_a_worker_report_never_counts'),
    ('M1294', "checks (4b): an episode ran under its policy's controls and candidate view (EQUIVALENT: M1295 at intake)", 'crates/axon-loop-contracts/src/checks.rs', '    if episode.controls_ref != policy.controls_ref\n        || episode.candidate_set_ref != policy.candidate_set_ref\n    {\n', '    if false && (episode.controls_ref != policy.controls_ref\n        || episode.candidate_set_ref != policy.candidate_set_ref)\n    {\n', 'axon-loop', '--test checks_sites', 'an_episode_under_other_controls_never_counts'),
    ('M1295', "checks (4b): a shortlist is applied only under the pilot's controls (library primitive; production: four cells with M1294)", 'crates/axon-loop-contracts/src/checks.rs', '    if &policy.controls_ref != controls_ref {\n', '    if false && &policy.controls_ref != controls_ref {\n', 'axon-loop-contracts', '--test fixtures', 'shortlist_must_be_a_subset_of_the_eligible_view'),
    ('M1296', "checks (4b): a pass is checked on the episode's own output (EQUIVALENT: M34 + M1291)", 'crates/axon-loop-contracts/src/checks.rs', '        if v.output_workspace_ref != episode.output_workspace_ref {\n', '        if false && v.output_workspace_ref != episode.output_workspace_ref {\n', 'axon-loop', '--test checks_sites', 'a_pass_checked_on_another_output_never_counts'),
    ('M1297', 'checks (4b): a paired-trial preflight binds only when observed == expected (library primitive; production: four cells with M831)', 'crates/axon-loop-contracts/src/checks.rs', '    if ctx.expected != ctx.observed {\n', '    if false && ctx.expected != ctx.observed {\n', 'axon-loop-contracts', '--test fixtures', 'paired_trial_requires_exact_context_equality'),
    ('M1298', 'checks (4b): a preflight counts only at the current authority epoch (library primitive; production: four cells with M1281)', 'crates/axon-loop-contracts/src/checks.rs', '    if ctx.authority_epoch != current_epoch {\n', '    if false && ctx.authority_epoch != current_epoch {\n', 'axon-loop-contracts', '--test fixtures', 'context_currency_and_roles'),
    ('M1299', 'checks (4b): only a trusted verifier independent of the subject establishes an outcome (library primitive; production: four cells with M10)', 'crates/axon-loop-contracts/src/checks.rs', '        if !issuer.is_some_and(|i| trusted_verifiers.contains(i) && !subject_issuers.contains(i)) {\n', '        if false && !issuer.is_some_and(|i| trusted_verifiers.contains(i) && !subject_issuers.contains(i)) {\n', 'axon-loop-contracts', '--test fixtures', 'bind_episode_refuses_mismatches'),
]
EQUIV_RECORD["M1294"] = {
    "property": "an episode is recorded and counted only under its policy's controls",
    "subsumed_by": ["M1295"], "killer": "joint:M1294+M1295",
    "all_paths": "bind_episode's two callers: intake_episode, whose check_ack then applies the same "
                 "policy through check_shortlist with controls_ref = the episode's (M1295 refuses the "
                 "identical predicate, no return between but refusals); and evl judge, which counts only "
                 "an episode intake recorded (intake_join, M15) under the policy keyed by its own digest "
                 "(M964), i.e. one that passed check_ack"}
EQUIV_RECORD["M1296"] = {
    "property": "a pass counts only when the verifier checked the episode's own output",
    "subsumed_by": ["M34", "M1291"], "killer": "joint:M1296+M34+M1291",
    "all_paths": "bind_episode's callers: intake_episode, whose step 8 (verify_check_evidence) refuses a "
                 "verification output other than the tree the check ran on (M34) after bind; evl judge, "
                 "which counts only an intaken episode (M15) and re-runs verify_check_evidence, and whose "
                 "bind_acf refuses an execution output other than the episode's (M1291); a verified "
                 "output that differs from the episode's either differs from the checked tree (M34) or "
                 "the execution receipt differs from one of them (M1291, or bind_acf's bytes-changed rule)"}
EQUIVALENT_DID |= {"M1294", "M1296"}
RETIRED |= {"M1294", "M1296"}

# ── C9 round 4b, INTEGRATE-C (amendment 64) ──
# The ledger's and the pointer's refusal sites (ledger.rs, pointer.rs), each
# killed by its own attack on the production route: a store tampered on disk
# then opened by the operation a caller runs (tests/ledger_sites.rs), and the
# public baseline / transition / revoke / resolve operations
# (tests/pointer_sites.rs). M1300-M1344.
MUTATIONS += [
    ('M1300', 'ledger (C): under a key, a line without a valid entry MAC is never rolled forward', 'crates/axon-loop/src/ledger.rs',
     '        (Some(_), _) => Err(corrupt(format!(\n            "ledger entry seq {} is not authenticated under the operator key \\\n             (forged, or written without {LEDGER_KEY_ENV})",\n            e.seq\n        ))),\n',
     '        (Some(_), _) => Ok(()),\n',
     'axon-loop', '--test ledger_sites', 'an_unauthenticated_line_is_never_rolled_forward_under_a_key'),
    ('M1301', 'ledger (C): a keyed ledger (entry MACs) is never read without the key', 'crates/axon-loop/src/ledger.rs',
     '        (None, Some(_)) => Err(corrupt(format!(\n            "ledger is keyed but no key is configured: set {LEDGER_KEY_ENV}"\n        ))),\n',
     '        (None, Some(_)) => Ok(()),\n',
     'axon-loop', '--test ledger_sites', 'a_keyed_ledger_is_never_read_without_its_key'),
    ('M1302', 'ledger (C): under a key, the head (the authenticated count) carries a valid MAC', 'crates/axon-loop/src/ledger.rs',
     '        (Some(_), _) => Err(corrupt(\n            "ledger head is not authenticated under the operator key \\\n             (rewritten, or written without the key)",\n        )),\n',
     '        (Some(_), _) => Ok(()),\n',
     'axon-loop', '--test ledger_sites', 'a_truncated_keyed_ledger_with_a_rewritten_head_is_refused'),
    ('M1303', 'ledger (C): a keyed head is never read without the key', 'crates/axon-loop/src/ledger.rs',
     '        (None, Some(_)) => Err(corrupt(format!(\n            "ledger head is keyed but no key is configured: set {LEDGER_KEY_ENV}"\n        ))),\n',
     '        (None, Some(_)) => Ok(()),\n',
     'axon-loop', '--test ledger_sites', 'a_keyed_head_is_never_read_without_its_key'),
    ('M1304', 'ledger (C): every entry is numbered by its position', 'crates/axon-loop/src/ledger.rs',
     '            if e.seq != i as u64 + 1 {\n',
     '            if false && e.seq != i as u64 + 1 {\n',
     'axon-loop', '--test ledger_sites', 'a_renumbered_ledger_entry_is_refused'),
    ('M1305', "ledger (C): every entry's prev is the previous entry's digest (the chain)", 'crates/axon-loop/src/ledger.rs',
     '            if e.prev != prev {\n',
     '            if false && e.prev != prev {\n',
     'axon-loop', '--test ledger_sites', 'an_edited_ledger_entry_breaks_the_chain'),
    ('M1306', 'ledger (C): a ledger truncated below its head is refused as corrupt', 'crates/axon-loop/src/ledger.rs',
     '                if h.seq > n {\n',
     '                if false && h.seq > n {\n',
     'axon-loop', '--test ledger_sites', 'a_ledger_truncated_below_its_head_is_refused_as_corrupt'),
    ('M1307', 'ledger (C): the head names the entry at its seq', 'crates/axon-loop/src/ledger.rs',
     '                if h.seq == 0 || tx.refs[h.seq as usize - 1] != h.entry_ref {\n',
     '                if false && (h.seq == 0 || tx.refs[h.seq as usize - 1] != h.entry_ref) {\n',
     'axon-loop', '--test ledger_sites', 'a_replaced_last_entry_does_not_match_its_head'),
    ('M1308', 'ledger (C): at most one unacknowledged entry (a crash tail) past the head', 'crates/axon-loop/src/ledger.rs',
     '                if n > h.seq + 1 {\n',
     '                if false && n > h.seq + 1 {\n',
     'axon-loop', '--test ledger_sites', 'two_unacknowledged_lines_are_never_served'),
    ('M1309', 'ledger (C): a ledger of more than one entry has a head (EQUIVALENT: M1310+M1311)', 'crates/axon-loop/src/ledger.rs',
     '            None if n > 1 => return Err(corrupt("ledger head missing")),\n',
     '            None if false && n > 1 => return Err(corrupt("ledger head missing")),\n',
     'axon-loop', '--test ledger_sites', 'a_ledger_truncated_with_its_head_deleted_is_refused'),
    ('M1310', 'ledger (C): a store whose anchor remains never starts a new ledger (EQUIVALENT: M1312)', 'crates/axon-loop/src/ledger.rs',
     '                if std::fs::symlink_metadata(anchor_path(store)).is_ok() {\n',
     '                if false && std::fs::symlink_metadata(anchor_path(store)).is_ok() {\n',
     'axon-loop', '--test ledger_sites', 'a_deleted_history_is_refused_while_its_anchor_remains'),
    ('M1311', 'ledger (C): a store with a ledger-dependent directory never starts a new ledger', 'crates/axon-loop/src/ledger.rs',
     '                    if std::fs::symlink_metadata(&p).is_ok() {\n',
     '                    if false && std::fs::symlink_metadata(&p).is_ok() {\n',
     'axon-loop', '--test ledger_sites', 'a_deleted_history_is_refused_while_a_dependent_directory_remains'),
    ('M1312', "ledger (C): the ledger's first entry is the one its anchor names", 'crates/axon-loop/src/ledger.rs',
     '            if tx.refs.first() != Some(&a.first_entry_ref) {\n',
     '            if false && tx.refs.first() != Some(&a.first_entry_ref) {\n',
     'axon-loop', '--test ledger_sites', 'a_replaced_ledger_does_not_match_its_anchor'),
    ('M1313', 'ledger (C): no tenant directory of the projection is a symlink', 'crates/axon-loop/src/ledger.rs',
     '                if t.file_type()?.is_symlink() {\n',
     '                if false && t.file_type()?.is_symlink() {\n',
     'axon-loop', '--test ledger_sites', 'a_symlinked_tenant_directory_is_refused'),
    ('M1314', 'ledger (C): no family directory of the projection is a symlink (EQUIVALENT: M979)', 'crates/axon-loop/src/ledger.rs',
     '                    if f.file_type()?.is_symlink() {\n',
     '                    if false && f.file_type()?.is_symlink() {\n',
     'axon-loop', '--test ledger_sites', 'a_symlinked_family_directory_is_refused'),
    ('M1315', 'ledger (C): every pointer.json is projected by a ledger transition', 'crates/axon-loop/src/ledger.rs',
     '                (Some(_), None) => {\n                    return Err(corrupt(format!(\n                        "pointer.json for {t}/{f} has no transition in the ledger"\n                    )))\n                }\n',
     '                (Some(_), None) => {}\n',
     'axon-loop', '--test ledger_sites', 'a_projection_with_no_transition_is_refused'),
    ('M1316', 'ledger (C): every transitioned scope has its projection', 'crates/axon-loop/src/ledger.rs',
     '                (None, Some(_)) => {\n                    return Err(corrupt(format!("pointer.json for {t}/{f} is missing")))\n                }\n',
     '                (None, Some(_)) => {}\n',
     'axon-loop', '--test ledger_sites', 'a_missing_projection_is_refused'),
    ('M1317', "ledger (C): pointer.json is exactly the ledger's projection", 'crates/axon-loop/src/ledger.rs',
     '                    if d.pointer != rec\n                        || d.ledger_seq != seq\n                        || d.entry_ref != self.refs[seq as usize - 1]\n                    {\n',
     '                    if false\n                        && (d.pointer != rec\n                            || d.ledger_seq != seq\n                            || d.entry_ref != self.refs[seq as usize - 1])\n                    {\n',
     'axon-loop', '--test ledger_sites', 'an_edited_projection_is_refused'),
    ('M1318', 'pointer (C): only a trusted admitter revokes', 'crates/axon-loop/src/pointer.rs',
     '    if !config.admitters().contains(issuer) {\n',
     '    if false && !config.admitters().contains(issuer) {\n',
     'axon-loop', '--test pointer_sites', 'an_untrusted_issuer_never_revokes'),
    ('M1319', 'pointer (C): only a trusted admitter designates the incumbent-of-record', 'crates/axon-loop/src/pointer.rs',
     '    if !config.admitters().contains(&b.issuer_ref) {\n',
     '    if false && !config.admitters().contains(&b.issuer_ref) {\n',
     'axon-loop', '--test pointer_sites', 'an_untrusted_issuer_never_designates_the_incumbent_of_record'),
    ('M1320', 'pointer (C): an EVO proposer in the scope never designates the incumbent-of-record', 'crates/axon-loop/src/pointer.rs',
     '    if tx.hypotheses(&b.scope, None).iter().any(|h| {\n        matches!(h, crate::evo::Hypothesis::Proposed { proposer_ref, .. } if proposer_ref == &b.issuer_ref)\n    }) {\n        return Err(refused(format!(\n            "baseline issuer {} is an EVO proposer',
     '    if false && tx.hypotheses(&b.scope, None).iter().any(|h| {\n        matches!(h, crate::evo::Hypothesis::Proposed { proposer_ref, .. } if proposer_ref == &b.issuer_ref)\n    }) {\n        return Err(refused(format!(\n            "baseline issuer {} is an EVO proposer',
     'axon-loop', '--test pointer_sites', 'an_evo_proposer_never_designates_the_incumbent_of_record'),
    ('M1321', 'pointer (C): a scope has one incumbent-of-record', 'crates/axon-loop/src/pointer.rs',
     '        return Err(refused("the scope already has an incumbent-of-record"));\n',
     '',
     'axon-loop', '--test pointer_sites', 'a_second_incumbent_of_record_is_never_designated'),
    ('M1322', "pointer (C): the incumbent-of-record's policy is of the baseline's scope", 'crates/axon-loop/src/pointer.rs',
     '    if env.scope != b.scope {\n',
     '    if false && env.scope != b.scope {\n',
     'axon-loop', '--test pointer_sites', 'a_policy_of_another_scope_is_never_the_incumbent_of_record'),
    ('M1323', 'pointer (C): an EVO candidate is never the incumbent-of-record', 'crates/axon-loop/src/pointer.rs',
     '    if crate::evo::proposer_in(&tx, &b.scope, &b.policy_ref).is_some() {\n',
     '    if false && crate::evo::proposer_in(&tx, &b.scope, &b.policy_ref).is_some() {\n',
     'axon-loop', '--test pointer_sites', 'an_evo_candidate_is_never_the_incumbent_of_record'),
    ('M1324', 'pointer (C): a revoked policy is never the incumbent-of-record', 'crates/axon-loop/src/pointer.rs',
     '    if tx.is_revoked(&b.scope, &b.policy_ref) {\n',
     '    if false && tx.is_revoked(&b.scope, &b.policy_ref) {\n',
     'axon-loop', '--test pointer_sites', 'a_revoked_policy_is_never_the_incumbent_of_record'),
    ('M1325', 'pointer (C): a revoked active policy is never resolved for a new task', 'crates/axon-loop/src/pointer.rs',
     '    if tx.is_revoked(scope, &active) {\n',
     '    if false && tx.is_revoked(scope, &active) {\n',
     'axon-loop', '--test pointer_sites', 'a_revoked_active_policy_is_never_resolved'),
    ('M1326', 'pointer (C): a transition id is used for one content only', 'crates/axon-loop/src/pointer.rs',
     '                return Err(LoopError::Conflict(format!(\n                    "transition id {} was already used for different content",\n                    t.transition_id\n                )));\n',
     '',
     'axon-loop', '--test pointer_sites', 'a_transition_id_is_never_reused_for_other_content'),
    ('M1327', 'pointer (C): only a trusted admitter issues a transition', 'crates/axon-loop/src/pointer.rs',
     '    if !config.admitters().contains(&t.issuer_ref) {\n',
     '    if false && !config.admitters().contains(&t.issuer_ref) {\n',
     'axon-loop', '--test pointer_sites', 'an_untrusted_issuer_never_moves_the_pointer'),
    ('M1328', 'pointer (C): a transition issuer holds no other loop role', 'crates/axon-loop/src/pointer.rs',
     '    if let Some(role) = crate::admission::other_loop_role(&config, &t.issuer_ref) {\n',
     '    if let Some(role) = crate::admission::other_loop_role(&config, &t.issuer_ref).filter(|_| false) {\n',
     'axon-loop', '--test pointer_sites', 'a_trusted_verifier_never_moves_the_pointer'),
    ('M1329', 'pointer (C): a transition is built on the current epoch (EQUIVALENT: M1330)', 'crates/axon-loop/src/pointer.rs',
     '    if t.expected_epoch != cur.epoch {\n',
     '    if false && t.expected_epoch != cur.epoch {\n',
     'axon-loop', '--test pointer_sites', 'a_transition_on_a_stale_epoch_is_refused'),
    ('M1330', 'pointer (C): a transition moves the epoch by exactly one (EQUIVALENT: M1329)', 'crates/axon-loop/src/pointer.rs',
     '    if t.next_epoch != cur.epoch.next()? {\n',
     '    if false && t.next_epoch != cur.epoch.next()? {\n',
     'axon-loop', '--test pointer_sites', 'a_transition_on_a_stale_epoch_is_refused'),
    ('M1331', 'pointer (C): a transition names the active policy', 'crates/axon-loop/src/pointer.rs',
     '    if t.expected_policy_ref != cur.expected_ref() {\n',
     '    if false && t.expected_policy_ref != cur.expected_ref() {\n',
     'axon-loop', '--test pointer_sites', 'a_transition_expecting_another_policy_is_refused'),
    ('M1332', 'pointer (C): the active policy is never activated again', 'crates/axon-loop/src/pointer.rs',
     '            if cur.active_policy_ref.as_ref() == Some(&target) {\n',
     '            if false && cur.active_policy_ref.as_ref() == Some(&target) {\n',
     'axon-loop', '--test pointer_sites', 'the_active_policy_is_never_activated_again'),
    ('M1333', 'pointer (C): a revoked policy is never made active', 'crates/axon-loop/src/pointer.rs',
     '            if tx.is_revoked(scope, &target) {\n',
     '            if false && tx.is_revoked(scope, &target) {\n',
     'axon-loop', '--test pointer_sites', 'a_revoked_candidate_is_never_activated'),
    ('M1334', "pointer (C): the activated envelope is of the transition's scope (EQUIVALENT: M1322)", 'crates/axon-loop/src/pointer.rs',
     '            if env.scope != *scope {\n',
     '            if false && env.scope != *scope {\n',
     'axon-loop', '--test pointer_sites', 'a_policy_of_another_scope_is_never_activated'),
    ('M1335', "pointer (C): the incumbent-of-record's issuer holds no other loop role NOW", 'crates/axon-loop/src/pointer.rs',
     '    if let Some(role) = crate::admission::other_loop_role(&config, &b.issuer_ref) {\n        return Err(refused(format!(\n            "the baseline\'s issuer',
     '    if let Some(role) = crate::admission::other_loop_role(&config, &b.issuer_ref).filter(|_| false) {\n        return Err(refused(format!(\n            "the baseline\'s issuer',
     'axon-loop', '--test pointer_sites', 'a_baseline_whose_issuer_became_a_verifier_is_never_activated'),
    ('M1336', "pointer (C): the incumbent-of-record's issuer is no EVO proposer in the scope NOW", 'crates/axon-loop/src/pointer.rs',
     '    if tx.hypotheses(&b.scope, None).iter().any(|h| {\n        matches!(h, crate::evo::Hypothesis::Proposed { proposer_ref, .. } if proposer_ref == &b.issuer_ref)\n    }) {\n        return Err(refused(format!(\n            "the baseline\'s issuer {} is an EVO proposer',
     '    if false && tx.hypotheses(&b.scope, None).iter().any(|h| {\n        matches!(h, crate::evo::Hypothesis::Proposed { proposer_ref, .. } if proposer_ref == &b.issuer_ref)\n    }) {\n        return Err(refused(format!(\n            "the baseline\'s issuer {} is an EVO proposer',
     'axon-loop', '--test pointer_sites', 'a_baseline_whose_issuer_became_a_proposer_is_never_activated'),
    ('M1337', 'pointer (C): a rollback cites the authority its predecessor was active under', 'crates/axon-loop/src/pointer.rs',
     '    if &h.admission_ref != adm_ref {\n',
     '    if false && &h.admission_ref != adm_ref {\n',
     'axon-loop', '--test pointer_sites', 'a_rollback_cites_its_predecessors_own_authority'),
    ('M1338', "pointer (C): a rollback keeps its predecessor's mechanism-test label", 'crates/axon-loop/src/pointer.rs',
     '    if h.mechanism_test != t.mechanism_test {\n',
     '    if false && h.mechanism_test != t.mechanism_test {\n',
     'axon-loop', '--test pointer_sites', 'a_rollback_keeps_its_predecessors_label'),
    ('M1339', 'pointer (C): a violation recorded since the evaluation blocks activation', 'crates/axon-loop/src/pointer.rs',
     '        if let Some(crate::safety::SafetyState::Violation { code }) = now.get(&key) {\n',
     '        if let Some(crate::safety::SafetyState::Violation { code }) = now.get(&key).filter(|_| false) {\n',
     'axon-loop', '--test pointer_sites', 'a_candidate_reported_unsafe_after_admission_is_never_activated'),
    ('M1340', 'pointer (C): the incumbent-of-record is activated only from paused', 'crates/axon-loop/src/pointer.rs',
     '            if cur.active_policy_ref.is_some() {\n',
     '            if false && cur.active_policy_ref.is_some() {\n',
     'axon-loop', '--test pointer_sites', 'the_incumbent_of_record_is_activated_only_from_paused'),
    ('M1341', "pointer (C): the incumbent-of-record route activates the baseline's own policy", 'crates/axon-loop/src/pointer.rs',
     '            if &b_policy != target {\n',
     '            if false && &b_policy != target {\n',
     'axon-loop', '--test pointer_sites', 'the_baseline_never_activates_another_policy'),
    ('M1342', 'pointer (C): the incumbent-of-record is never a mechanism-test activation', 'crates/axon-loop/src/pointer.rs',
     '            if t.mechanism_test {\n                return Err(refused("an incumbent-of-record is not a mechanism test"));\n',
     '            if false && t.mechanism_test {\n                return Err(refused("an incumbent-of-record is not a mechanism test"));\n',
     'axon-loop', '--test pointer_sites', 'the_incumbent_of_record_is_never_a_mechanism_test'),
    ('M1343', "pointer (C): the incumbent-of-record's issuer is still trusted", 'crates/axon-loop/src/pointer.rs',
     '            if !admitters.contains(&b.issuer_ref) {\n                return Err(refused("baseline issuer is no longer trusted"));\n',
     '            if false && !admitters.contains(&b.issuer_ref) {\n                return Err(refused("baseline issuer is no longer trusted"));\n',
     'axon-loop', '--test pointer_sites', 'a_baseline_whose_issuer_is_no_longer_trusted_is_never_activated'),
    ('M1344', 'pointer (C): a mechanism-test fixture is no incumbent for a real activation', 'crates/axon-loop/src/pointer.rs',
     '    if cur.active_mechanism_test && !t.mechanism_test {\n',
     '    if false && cur.active_mechanism_test && !t.mechanism_test {\n',
     'axon-loop', '--test pointer_sites', 'a_mechanism_test_fixture_is_no_incumbent_for_a_real_activation'),
]
EQUIV_RECORD['M1309'] = {
    "property": 'a ledger whose head was deleted is never served (truncation behind a deleted head)',
    "subsumed_by": ['M1310', 'M1311'], "killer": 'joint:M1309+M1310+M1311',
    "all_paths": 'Tx::begin is the only reader of the ledger; with no head and n > 1 the head-missing arm refuses, and without it the match falls to the no-head arm, which refuses while ledger.anchor exists (M1310: written before the first head, never rewritten) or any ledger-dependent directory exists (M1311: a store with more than one entry has written at least its baseline/scopes); only with all three removed is the truncated ledger served'}
EQUIV_RECORD['M1310'] = {
    "property": 'a store whose ledger was deleted while its anchor remains never starts a new ledger',
    "subsumed_by": ['M1312'], "killer": 'joint:M1310+M1312',
    "all_paths": 'with no ledger (n = 0) the anchor-presence rule refuses; without it Tx::begin reads the anchor next and compares its first_entry_ref with refs.first(), which is None for an empty ledger, so the anchor binding (M1312) refuses every input this rule refuses; only both removed reissue the history'}
EQUIV_RECORD['M1314'] = {
    "property": 'the projection is never read through a symlinked scope family directory',
    "subsumed_by": ['M979'], "killer": 'joint:M1314+M979',
    "all_paths": 'every family directory the walk visits names a scope whose pointer.json is then read by Store::read_json -> read_text -> guard, which lstat()s every component and refuses the symlink (M979); a symlinked family whose name is no TaskFamily is refused as a bad scope dir; only both removed read the projection through the link'}
EQUIV_RECORD['M1329'] = {
    "property": 'a transition is applied only on the current epoch (the fence)',
    "subsumed_by": ['M1330'], "killer": 'joint:M1329+M1330',
    "all_paths": 'every PolicyTransition reaches pointer::transition through parse -> validate, which refuses next_epoch != expected_epoch + 1 (policy.rs); so expected_epoch != cur.epoch (this check) holds exactly when next_epoch != cur.epoch + 1 (M1330), the statement after it with no return between; the two predicates are one, and only both removed apply a stale transition'}
EQUIV_RECORD['M1330'] = {
    "property": 'a transition is applied only on the current epoch (the fence)',
    "subsumed_by": ['M1329'], "killer": 'joint:M1329+M1330',
    "all_paths": 'as M1329: under the parse rule next_epoch == expected_epoch + 1 this predicate equals expected_epoch != cur.epoch, which M1329 refuses two statements earlier'}
EQUIV_RECORD['M1334'] = {
    "property": 'a policy of another scope is never made active',
    "subsumed_by": ['M1322'], "killer": 'joint:M1334+M1322',
    "all_paths": "a target reaches the envelope check only through check_activate or check_rollback: route 1 (the incumbent-of-record) requires target == the baseline's policy, whose envelope designate_baseline refused unless of the baseline's scope (M1322), and the baseline is the scope's own (baseline_of(&t.scope)); route 2 requires a re-derived admission whose scope is the transition's (check_activate's adm.scope check) and whose plan froze candidate and incumbent envelopes of the plan's scope (plan.rs freeze); a rollback target is a history entry, which was activated through one of those routes in this scope"}
EQUIVALENT_DID |= {'M1310', 'M1334', 'M1314', 'M1309', 'M1330', 'M1329'}
RETIRED |= {'M1310', 'M1334', 'M1314', 'M1309', 'M1330', 'M1329'}
# ── end INTEGRATE-C ──
# ── C9 round 4b, INTEGRATE-D (amendment 64) ── price.rs, tel.rs, plan.rs's
# dominated candidate checks, rules.rs's unset word rule, and the parse layers
# (schema walk, typed validate, PolicyEnvelope's authority rule) that the
# four-cell retirements below name as siblings. M1345-M1371.
_PR = 'crates/axon-loop/src/price.rs'
_TL = 'crates/axon-loop/src/tel.rs'
_PN = 'crates/axon-loop/src/plan.rs'
_PTS = '--test price_tel_sites'
_PSS = '--test plan_strict_sites'
_PLS = '--test plan_sites'
MUTATIONS += [
    ('M1345', 'price (4b-I): a schedule prices only if its content digests to the pinned ref', _PR,
     '        if actual != *expected {', '        if false && actual != *expected {',
     'axon-loop', _PTS, 'a_schedule_whose_content_is_not_its_ref_never_prices'),
    ('M1346', 'price (4b-I): a schedule is pinned only by a cl22 content ref (EQUIVALENT: M1345)', _PR,
     '        if expected.scheme() != RefScheme::Cl22 {', '        if false && expected.scheme() != RefScheme::Cl22 {',
     'axon-loop', _PTS, 'a_schedule_pinned_by_a_non_cl22_ref_never_prices'),
    ('M1347', 'price (4b-I): a schedule of another schema never prices', _PR,
     '        if doc.schema != PRICE_SCHEDULE_SCHEMA {', '        if false && doc.schema != PRICE_SCHEDULE_SCHEMA {',
     'axon-loop', _PTS, 'a_schedule_of_another_schema_never_prices'),
    ('M1348', 'price (4b-I): a schedule covering nothing never prices', _PR,
     '        if doc.covers.is_empty() {', '        if false && doc.covers.is_empty() {',
     'axon-loop', _PTS, 'a_schedule_covering_nothing_never_prices'),
    ('M1349', 'price (4b-I): a schedule repeating a coverage never prices', _PR,
     '        if covers.len() != doc.covers.len() {', '        if false && covers.len() != doc.covers.len() {',
     'axon-loop', _PTS, 'a_schedule_repeating_a_coverage_never_prices'),
    ('M1350', 'price (4b-I) D10: a schedule claiming execution coverage never prices', _PR,
     '        if covers.contains(&Coverage::Execution) {', '        if false && covers.contains(&Coverage::Execution) {',
     'axon-loop', _PTS, 'a_schedule_claiming_execution_coverage_never_prices'),
    ('M1351', "price (4b-I) G10: a request's schedule is the pinned one", _PR,
     '        if r != self.reference {', '        if false && r != self.reference {',
     'axon-loop', _PTS, 'a_request_naming_another_schedule_never_joins'),
    ('M1352', "price (4b-I) G10: resolve_opaque reads only a cl22 ref, byte for byte (the public primitive; its "
     "one production caller then compares with the pinned ref, M1351)", _PR,
     '    if r.scheme() != RefScheme::Cl22 || r.as_str() != o.as_str() {',
     '    if false && (r.scheme() != RefScheme::Cl22 || r.as_str() != o.as_str()) {',
     'axon-loop', _PTS, 'resolve_opaque_never_reads_another_scheme_as_a_content_ref'),
    ('M1353', "price (4b-I) G10: a request is in the pinned schedule's currency", _PR,
     '        if req.limits.currency_code != self.currency {', '        if false && req.limits.currency_code != self.currency {',
     'axon-loop', _PTS, 'a_request_in_another_currency_never_joins'),
    ('M1354', 'price (4b-I) G10: a usage names the pinned schedule', _PR,
     '        if u.price_schedule_ref != self.reference {', '        if false && u.price_schedule_ref != self.reference {',
     'axon-loop', _PTS, 'a_usage_naming_another_schedule_never_prices'),
    ('M1355', "price (4b-I) G10: a usage is in the pinned schedule's currency", _PR,
     '        if u.currency != self.currency {', '        if false && u.currency != self.currency {',
     'axon-loop', _PTS, 'a_usage_in_another_currency_never_prices'),
    ('M1356', 'tel (4b-I): an attempt is accounted once per component', _TL,
     '            if !seen.insert(a) {', '            if !seen.insert(a) && false {',
     'axon-loop', _PTS, 'an_attempt_named_by_two_usages_is_never_counted_twice'),
    ('M1357', "tel (4b-I): a receipt joins only its own request's identity", _TL,
     '        if req.operation_id != rc.operation_id\n            || req.attempt_id != rc.attempt_id\n'
     '            || req.task_id != rc.task_id\n            || req.trial_id != rc.trial_id\n        {',
     '        if false {',
     'axon-loop', _PTS, 'a_receipt_of_another_attempt_never_joins_its_request'),
    ('M1358', 'tel (4b-I) G13: one receipt is never paired with two requests', _TL,
     '            if *q != qref {', '            if false && *q != qref {',
     'axon-loop', _PTS, 'a_receipt_paired_with_two_requests_never_joins'),
    ('M1359', 'tel (4b-I) G13: one attempt is never reported by two receipts', _TL,
     '        if let Some(other) = by_attempt.get(&key) {', '        if let Some(other) = by_attempt.get(&key).filter(|_| false) {',
     'axon-loop', _PTS, 'an_attempt_reported_by_two_receipts_is_never_counted_twice'),
    ('M1360', "tel (4b-I): a joined request satisfies its contract (EQUIVALENT: M1362+M1363)", _TL,
     '        req.validate()\n            .map_err(|e| refused(format!("fabric_attempts[{i}].request: {e}")))?;',
     '        let _ = req.validate();',
     'axon-loop', _PTS, 'a_request_breaking_its_contract_never_joins'),
    ('M1361', "tel (4b-I): a joined receipt satisfies its contract (EQUIVALENT: M1362+M1363)", _TL,
     '        rc.validate()\n            .map_err(|e| refused(format!("fabric_attempts[{i}].receipt: {e}")))?;',
     '        let _ = rc.validate();',
     'axon-loop', _PTS, 'a_receipt_breaking_its_contract_never_joins'),
    ('M1362', "contracts (4b-I): parse holds a document to its checked-in schema (the schema walk's call)",
     'crates/axon-loop-contracts/src/canonical.rs',
     '    crate::schema::validate_against(&schema, &value)?;', '    let _ = crate::schema::validate_against(&schema, &value);',
     'axon-loop', _PTS, 'an_episode_in_a_non_schema_shape_is_never_summarized'),
    ('M1363', "contracts (4b-I): parse holds a document to its typed rules (the library primitive; the "
     "pointer re-judges the one rule only this layer states, a contiguous fence)",
     'crates/axon-loop-contracts/src/canonical.rs',
     '    typed.validate()?;', '    let _ = typed.validate();',
     'axon-loop-contracts', '--test parse_validate_sites', 'parse_never_admits_a_noncontiguous_fence'),
    ('M1364', 'freeze (4b-I): a plan never compares a policy with itself (EQUIVALENT: M1012; ruling R3, amendment 64)', _PN,
     '    if inc == cand {', '    if false && inc == cand {',
     'axon-loop', _PLS, 'a_plan_comparing_a_policy_with_itself_never_freezes'),
    ('M1365', "check_candidate (4b-I): both policies are the plan's scope (EQUIVALENT: M1011)", _PN,
     '    if ce.scope != plan.scope || ie.scope != plan.scope {',
     '    if false && (ce.scope != plan.scope || ie.scope != plan.scope) {',
     'axon-loop', _PLS, 'a_candidate_of_another_scope_never_freezes'),
    ('M1366', "check_candidate (4b-I): the candidate keeps the incumbent's view and mode (EQUIVALENT: M1011)", _PN,
     '    if ce.candidate_set_ref != ie.candidate_set_ref || ce.mode != ie.mode {',
     '    if false && (ce.candidate_set_ref != ie.candidate_set_ref || ce.mode != ie.mode) {',
     'axon-loop', _PLS, 'a_candidate_over_another_candidate_view_never_freezes'),
    ('M1367', "check_candidate (4b-I): the candidate's shortlist only subtracts (EQUIVALENT: M1011)", _PN,
     '    if let Some(c) = ce.shortlist.iter().find(|c| !ie.shortlist.contains(c)) {',
     '    if let Some(c) = ce.shortlist.iter().find(|c| !ie.shortlist.contains(c)).filter(|_| false) {',
     'axon-loop', _PLS, 'a_candidate_adding_a_tool_never_freezes'),
    ('M1368', 'check_candidate (4b-I): a candidate claiming an authority expansion never freezes '
     '(EQUIVALENT: M1369+M1362+M1011)', _PN,
     '    if ce.authority_expansion {', '    if false && ce.authority_expansion {',
     'axon-loop', _PSS, 'a_candidate_claiming_an_authority_expansion_never_freezes'),
    ('M1369', "contracts (4b-I): a policy claiming an authority expansion is refused by its typed rule "
     "(the library primitive, for values built in code; every production reader parses first, M1362)",
     'crates/axon-loop-contracts/src/policy.rs',
     '        if self.authority_expansion {', '        if false && self.authority_expansion {',
     'axon-loop-contracts', '--test parse_validate_sites', 'a_policy_built_in_code_claiming_an_expansion_never_validates'),
    ('M1370', 'freeze (4b-I): a frozen experiment is never reported frozen under a re-registered plan '
     '(EQUIVALENT: M1006)', _PN,
     '        if f.plan_ref != r {', '        if false && f.plan_ref != r {',
     'axon-loop', _PSS, 'a_re_registered_frozen_plan_is_never_frozen_under_its_new_plan'),
    ('M1371', 'plan rules (4b-I): an unset word rule is not executable (EQUIVALENT: M1008)',
     'crates/axon-loop/src/rules.rs',
     '        None => Err(format!("{field} unset")),', '        None => Ok(()),',
     'axon-loop', _PSS, 'a_plan_with_no_missing_data_rule_never_freezes'),
]
EQUIV_RECORD["M1346"] = {
    "property": "a price schedule is pinned only by a cl22 content ref",
    "subsumed_by": ["M1345"], "killer": "joint:M1346+M1345",
    "all_paths": "pin's only production caller is `tel summarize` (grep PinnedSchedule::pin); the next check "
                 "compares `expected` with digest_value(document), which is always a cl22 Ref (canonical.rs "
                 "digest_value), so a non-cl22 `expected` never equals it (M1345) with no return between"}
EQUIV_RECORD["M1360"] = {
    "property": "a request joined by tel satisfies its contract",
    "subsumed_by": ["M1362", "M1363"], "killer": "joint:M1360+M1362+M1363",
    "all_paths": "tel::join's only production caller is `tel summarize`, which builds every request by "
                 "contract_from_value -> parse, which runs the checked-in schema walk (M1362) and the same "
                 "ComputeRequest::validate (M1363) on the same value, unmodified before join"}
EQUIV_RECORD["M1361"] = {
    "property": "a receipt joined by tel satisfies its contract",
    "subsumed_by": ["M1362", "M1363"], "killer": "joint:M1361+M1362+M1363",
    "all_paths": "as M1360: every receipt reaching join was parsed by contract_from_value (schema walk M1362, "
                 "ExecutionReceipt::validate M1363) and is unmodified before join"}
EQUIV_RECORD["M1365"] = {
    "property": "the candidate and the incumbent are the plan's scope",
    "subsumed_by": ["M1011"], "killer": "joint:M1365+M1011",
    "all_paths": "check_candidate's earlier proposer_in(tx, &plan.scope, cand) (M1011) admits only a "
                 "candidate EVO proposed in the plan's scope, and EVO proposes only children of the scope's "
                 "own policies (same scope); the incumbent is the scope's (freeze binds plan.scope); "
                 "check_candidate's only callers are freeze and admission's re-check of a frozen plan"}
EQUIV_RECORD["M1366"] = {
    "property": "the candidate keeps the incumbent's candidate view and mode",
    "subsumed_by": ["M1011"], "killer": "joint:M1366+M1011",
    "all_paths": "as M1365: an EVO proposal (M1011) copies its parent's candidate_set_ref and mode and only "
                 "reorders/removes shortlist entries (evo.rs), and its parent is the incumbent (M1012)"}
EQUIV_RECORD["M1367"] = {
    "property": "the candidate's shortlist only subtracts from the incumbent's",
    "subsumed_by": ["M1011"], "killer": "joint:M1367+M1011",
    "all_paths": "as M1366: an EVO proposal is a subtractive mutation of its parent's shortlist"}
EQUIV_RECORD["M1368"] = {
    "property": "a candidate claiming an authority expansion never freezes",
    "subsumed_by": ["M1369", "M1362", "M1011"], "killer": "joint:M1368+M1369+M1362+M1011",
    "all_paths": "check_candidate reads both policies through get_contract -> parse, whose schema walk "
                 "(`const false`, M1362) and PolicyEnvelope::validate (M1369) refuse the claim, and "
                 "require_shortlist's check_shortlist calls the same validate; the proposer record (M1011) "
                 "names only EVO proposals, which never set the flag"}
EQUIV_RECORD["M1370"] = {
    "property": "a frozen experiment is never reported frozen under another plan",
    "subsumed_by": ["M1006"], "killer": "joint:M1370+M1006",
    "all_paths": "plan::register (the only writer of a registration) refuses an experiment id that has a "
                 "Freeze (M1006), so load_registered's latest registration is the frozen one"}
EQUIV_RECORD["M1371"] = {
    "property": "an unset word rule never freezes",
    "subsumed_by": ["M1008"], "killer": "joint:M1371+M1008",
    "all_paths": "Rules::parse's callers: freeze, after unset_fields (M1008) refused any of the five word "
                 "fields unset, and admission, which parses only a frozen plan"}
EQUIVALENT_DID |= {"M1346", "M1360", "M1361", "M1365", "M1366", "M1367", "M1368",
                   "M1370", "M1371"}
RETIRED |= {"M1346", "M1360", "M1361", "M1365", "M1366", "M1367", "M1368",
            "M1370", "M1371"}
# ── C9 round 4b, INTEGRATE-E (amendment 64) ── the STRICT REWORK of the
# refusal-site exemptions that rested only on another check refusing the same
# input first. Each became an ACTIVE row killed by its own attack on the
# production route (M1376), or an EQUIVALENT_DID retirement whose four cells
# were executed against the check that dominates it (M1375, M1377-M1380). Where
# no four cells exist because the value the site reads is absent on every input
# (the null attestation / context signature, the issuer, the issued_ms), the
# executed cells are recorded in scripts/v022_refusal_coverage.py instead.
_IE = '--test strict_sites'
_IEP = '--test strict_protected_sites'
_IE_EV = 'crates/axon-loop/src/evl.rs'
_IE_ST = 'crates/axon-loop/src/store.rs'
_IE_LI = 'crates/axon-loop/src/intake.rs'
MUTATIONS += [
    ('M1375', 'EVL (4b, integrate-E): a trial delivered and requested but never issued is never judged (EQUIVALENT: M108)', _IE_EV,
     '                let (issued_attempt, _) = issued.get(&key).ok_or_else(|| {\n',
     '                let fallback = (d.ep.identity.attempt_id.clone(), d.ep.policy_ref.clone());\n'
     '                let (issued_attempt, _) = issued.get(&key).or(Some(&fallback)).ok_or_else(|| {\n',
     'axon-loop', _IE, 'a_trial_requested_and_delivered_but_never_issued_is_never_judged'),
    ('M1376', 'store (4b, integrate-E): a ledger line that is not an entry, anywhere but a torn last line, is corruption', _IE_ST,
     '            Err(_) if last && !complete => break,\n            Err(e) => {\n',
     '            Err(_) if last && !complete => break,\n            Err(_) => continue,\n'
     '            #[allow(unreachable_patterns)]\n            Err(e) => {\n',
     'axon-loop', _IE, 'a_ledger_with_a_line_that_is_not_an_entry_is_never_read'),
    ('M1377', "store (4b, integrate-E): a record is read only in its closed canonical shape (EQUIVALENT: M978)", _IE_ST,
     '    if axon_loop_contracts::canonical_bytes(&back)? != axon_loop_contracts::canonical_bytes(&v)? {',
     '    if false && axon_loop_contracts::canonical_bytes(&back)? != axon_loop_contracts::canonical_bytes(&v)? {',
     'axon-loop', _IE, 'a_stored_record_edited_and_re_encoded_is_never_admitted'),
    ('M1378', "intake (4b, integrate-E): an episode's policy is named by its cl22 digest (EQUIVALENT: M1379+M978+M965)", _IE_LI,
     '    if ep.policy_ref.scheme() != RefScheme::Cl22 {',
     '    if false && ep.policy_ref.scheme() != RefScheme::Cl22 {',
     'axon-loop', _IE, 'an_episode_naming_its_policy_by_a_non_cl22_alias_is_never_intaken'),
    ('M1379', "store (4b, integrate-E): a CAS record is named only by a cl22 reference (EQUIVALENT: M1378+M978+M965)", _IE_ST,
     '        if r.scheme() != RefScheme::Cl22 {',
     '        if false && r.scheme() != RefScheme::Cl22 {',
     'axon-loop', _IE, 'an_episode_naming_its_policy_by_a_non_cl22_alias_is_never_intaken'),
    ('M1380', "store (4b, integrate-E): a stored document's exact text is read only if it digests to its name (EQUIVALENT: M02)", _IE_ST,
     '        if axon_loop_contracts::digest(&v)? != *r {',
     '        if false && axon_loop_contracts::digest(&v)? != *r {',
     'axon-loop', _IEP, 'a_stored_attestation_edited_in_place_is_never_admitted_on'),
]
EQUIV_RECORD["M1375"] = {
    "property": "only a trial the admitter issued before execution is judged",
    "subsumed_by": ["M108"], "killer": "joint:M1375+M108",
    "all_paths": "the lookup runs only for a key in `requested` (the arms loop visits requested keys); evaluate "
                 "refuses, with only refusals between, every request whose keyed population is not exactly the "
                 "issued one (M108), so every requested key is issued whenever the lookup runs"}
EQUIV_RECORD["M1377"] = {
    "property": "a stored record is read only as the bytes its name was computed over",
    "subsumed_by": ["M978"], "killer": "joint:M1377+M978",
    "all_paths": "every stored record read through strict_record by name goes through get_record, which then "
                 "re-digests the typed value (check_name, M978); an alternative encoding of the SAME value "
                 "selects nothing (the typed value is what every reader uses), and one of an edited value "
                 "digests to another name. The other strict_record callers parse request documents, not "
                 "stored records, where the typed value is the request"}
_IE_CL = ("no stored record is named but by the cl22 digest of its content: put_cas computes that name, "
          "the CAS path is built only for a cl22 ref (M1379), the record read back is re-digested to its "
          "name (M978), and bind_episode compares the episode's policy_ref with the digest of the bytes it "
          "ran (M965); intake refuses a non-cl22 policy_ref first (M1378). Executed: each alone refuses the "
          "alias attack; all four removed, it is intaken")
EQUIV_RECORD["M1378"] = {
    "property": "an episode's policy is named by the cl22 digest of the stored policy",
    "subsumed_by": ["M1379", "M978", "M965"], "killer": "joint:M1378+M1379+M978+M965",
    "all_paths": _IE_CL}
EQUIV_RECORD["M1379"] = {
    "property": "an episode's policy is named by the cl22 digest of the stored policy",
    "subsumed_by": ["M1378", "M978", "M965"], "killer": "joint:M1379+M1378+M978+M965",
    "all_paths": _IE_CL + "; the store's other readers (get_cas_text, get_record) re-digest to the name "
                 "(M1380, M978), which a non-cl22 ref never equals"}
EQUIV_RECORD["M1380"] = {
    "property": "a stored document is read only as the bytes its name was computed over",
    "subsumed_by": ["M02"], "killer": "joint:M1380+M02",
    "all_paths": "get_cas_text's callers (admission's protected re-derivation, clearance signatures) each "
                 "authenticate the returned text over its exact bytes before use: verification and execution "
                 "attestations by attestation::verify (signature M02), clearance and context signatures by "
                 "verify_document (M951); an edited signed document fails that signature"}
EQUIVALENT_DID |= {"M1375", "M1377", "M1378", "M1379", "M1380"}
RETIRED |= {"M1375", "M1377", "M1378", "M1379", "M1380"}

# ── C9 round 4b, INTEGRATE-2 (amendment 64) ── the merge of integrate-A..E and
# the last refusal sites: check_activate's admission route (pointer.rs), the
# rest of checks.rs, evl's vacuous-pass check (integrator ruling on E's flag),
# and the two statuses of rulings R1 and the schema ruling.
_I2_PT = 'crates/axon-loop/src/pointer.rs'
_I2_CK = 'crates/axon-loop-contracts/src/checks.rs'
_I2_AS = '--test activation_sites'
_I2_CR = '--test checks_rest_sites'
_I2_HI = '--no-default-features --test harness_integrity'
MUTATIONS += [
    ('M1372', "EVL (4b, integrate-2): a pass over zero matched checks never counts (EQUIVALENT: M1249+M1257+M1218; integrator ruling on E's flag)",
     'crates/axon-loop/src/evl.rs',
     '            if v.matched_checks == 0 {\n                unknown(',
     '            if false && v.matched_checks == 0 {\n                unknown(',
     'axon-loop', '--test strict_sites', 'a_pass_over_zero_matched_checks_never_counts'),
    ('M1373', 'HARNESS (4b, integrate-2): a LIBRARY_PRIMITIVE row is never counted among the killed active rows (ruling R1)',
     'scripts/v022_g01_mutations.py',
     '    lib_rows = [r for r in rows if r["id"] in LIBRARY_PRIMITIVE]\n',
     '    lib_rows = []\n',
     'axon-core', _I2_HI, 'a_library_primitive_row_is_never_reported_as_killed'),
    ('M1374', 'HARNESS (4b, integrate-2): a SIBLING-ONLY edit is never an active row (schema ruling)',
     'scripts/v022_g01_mutations.py',
     '    if mid in RETIRED or mid in SIBLING_ONLY:\n',
     '    if mid in RETIRED:\n',
     'axon-core', _I2_HI, 'a_sibling_only_edit_is_never_an_active_row'),
    ('M1400', 'checks (4b, integrate-2): a shortlist policy is never empty (EQUIVALENT: M1220+M1238)', _I2_CK,
     '    if policy.shortlist.is_empty() {\n', '    if false && policy.shortlist.is_empty() {\n',
     'axon-loop', _I2_CR, 'a_policy_with_an_empty_shortlist_is_never_stored'),
    ('M1401', "checks (4b, integrate-2): a shortlist is applied only in its own scope and candidate view (LIBRARY_PRIMITIVE)", _I2_CK,
     '    if &policy.scope != scope || &policy.candidate_set_ref != candidate_set_ref {\n',
     '    if false && (&policy.scope != scope || &policy.candidate_set_ref != candidate_set_ref) {\n',
     'axon-loop-contracts', '--test fixtures', 'shortlist_must_be_a_subset_of_the_eligible_view'),
    ('M1402', "checks (4b, integrate-2): a verification never cites the context or execution documents it judges (EQUIVALENT: M45)", _I2_CK,
     '    if v.verifier_ref\n        .iter()\n', '    if false && v.verifier_ref\n        .iter()\n',
     'axon-loop', _I2_CR, 'a_check_request_doubling_as_the_execution_request_never_counts'),
    ('M1403', "checks (4b, integrate-2): a pass binds only when the verified bytes are the execution's output (LIBRARY_PRIMITIVE)", _I2_CK,
     '    if v.result == VerificationResult::Passed\n        && (receipt.output_workspace_ref.is_none()',
     '    if false && v.result == VerificationResult::Passed\n        && (receipt.output_workspace_ref.is_none()',
     'axon-loop', '--test evl', 'bind_acf_alone_refuses_a_pass_over_other_bytes'),
    ('M1386', 'pointer (4b, integrate-2): an admission activates only the policy it admitted', _I2_PT,
     '    if &adm.target_policy_ref != target {', '    if false && &adm.target_policy_ref != target {',
     'axon-loop', _I2_AS, 'an_admission_never_activates_a_policy_it_did_not_admit'),
    ('M1387', "pointer (4b, integrate-2): an admission activates only in its own scope (EQUIVALENT: M1388+M1334)", _I2_PT,
     '    if adm.scope != t.scope {', '    if false && adm.scope != t.scope {',
     'axon-loop', _I2_AS, 'an_admission_of_another_scope_never_activates_its_candidate_here'),
    ('M1388', 'pointer (4b, integrate-2): H1, an admission displaces only the incumbent it was compared against', _I2_PT,
     '    if &adm.incumbent_policy_ref != active {', '    if false && &adm.incumbent_policy_ref != active {',
     'axon-loop', _I2_AS, 'an_admission_against_a_parked_incumbent_never_displaces_the_active_policy'),
    ('M1389', 'pointer (4b, integrate-2): K1, evidence evaluated at an older epoch never activates', _I2_PT,
     '    if adm.evaluated_at_epoch != cur.epoch {', '    if false && adm.evaluated_at_epoch != cur.epoch {',
     'axon-loop', _I2_AS, 'an_admission_evaluated_at_an_older_epoch_never_activates'),
    ('M1390', 'pointer (4b, integrate-2): an activation keeps its admission\'s mechanism-test label', _I2_PT,
     '    if adm.mechanism_test != t.mechanism_test {', '    if false && adm.mechanism_test != t.mechanism_test {',
     'axon-loop', _I2_AS, 'fixture_evidence_never_activates_a_policy_as_a_real_one'),
    ('M1391', 'pointer (4b, integrate-2): a plan frozen with deployment disabled never activates', _I2_PT,
     '    if !adm.deployment_enabled {', '    if false && !adm.deployment_enabled {',
     'axon-loop', _I2_AS, 'a_plan_with_deployment_disabled_never_activates_its_candidate'),
]
EQUIV_RECORD["M1372"] = {
    "property": "a pass over zero matched checks never counts",
    "subsumed_by": ["M1249", "M1257", "M1218"], "killer": "joint:M1372+M1249+M1257+M1218",
    "all_paths": "evl's judge reads `v`, the verification of an episode it counts only if intake recorded "
                 "it (intake_join, M15); every episode and check receipt reaches intake and evaluate through "
                 "axon_loop_contracts::parse, whose schema walk refuses a passed verification or receipt with "
                 "matched_checks below the schemas' `minimum: 1` (the walker's minimum check, schema.rs, M1218) "
                 "and whose typed rules refuse the same (LoopEpisode::validate M1249, ExecutionReceipt::validate "
                 "M1257). The schemas' own `minimum: 1` clauses are NOT set members: the checked-in schema bytes "
                 "are the MiCode package's, digest-pinned by axon-loop-contracts tests/fixtures.rs "
                 "checked_in_schemas_are_the_package_bytes, so an edit of them cannot be a standalone row "
                 "(integrator ruling, amendment 64); the walker's check that enforces them is the code member"}
EQUIV_RECORD["M1400"] = {
    "property": "a shortlist policy is never empty",
    "subsumed_by": ["M1220", "M1238"], "killer": "joint:M1400+M1220+M1238",
    "all_paths": "check_shortlist's first statement is policy.validate() on the same value, whose check_array("
                 "\"shortlist\", .., 1, 256) (lib.rs, M1238) refuses an empty list before the emptiness check "
                 "runs, on every caller (put_policy/require_shortlist, intake's ack, evo, axon-reflex); a parsed "
                 "policy is also refused by the schema's minItems (the walker, schema.rs, M1220) earlier in parse"}
EQUIV_RECORD["M1402"] = {
    "property": "a verification never cites the context or execution documents of the trial it judges",
    "subsumed_by": ["M45"], "killer": "joint:M1402+M45",
    "all_paths": "bind_acf's only production caller is evl's judge, which counts only an episode intake recorded "
                 "(intake_join, M15). Intake's verify_check_evidence requires verifier_ref == cl22 of the check "
                 "receipt and evidence_refs == [cl22 of the check request] and then refuses either document's "
                 "ref among the episode's context/request/receipt refs (M45): exactly this predicate for an "
                 "intaken episode. Executed (integrate-2): M45 removed -> this check refuses; both removed -> "
                 "the trial whose check request doubles as its execution request counts"}
EQUIV_RECORD["M1387"] = {
    "property": "an admission activates only in its own scope",
    "subsumed_by": ["M1388", "M1334"], "killer": "joint:M1387+M1388+M1334",
    "all_paths": "after the target check (M1386) the admission's target is the transition's target, whose "
                 "envelope derive's check_candidate (re-run by rederive) requires to be of the admission's scope; "
                 "so another scope's admission names an incumbent of that scope, never this scope's active "
                 "policy (H1, M1388), and a target envelope of that scope, which the activation's envelope check "
                 "refuses (M1334). Executed (integrate-2): each removed alone refuses; all three removed, scope "
                 "B serves scope A's candidate"}
EQUIV_RECORD["M1364"] = {
    "property": "a plan never compares a policy with itself",
    "subsumed_by": ["M1012"], "killer": "joint:M1364+M1012",
    "all_paths": "freeze's check_candidate runs after this check on the same refs with only refusals between: a "
                 "candidate with no EVO proposer is refused (M1011), and an EVO candidate's envelope names its "
                 "parent by digest, which is never its own digest, so `parent == inc` fails when cand == inc "
                 "(M1012); admission re-runs check_candidate and ready() refuses the same plan (start_blockers). "
                 "plan_evo_tel's freeze_refuses_candidate_equal_incumbent pins that the freeze is REFUSED, not "
                 "which of the two refusals answers (ruling R3, precedent M487)"}
EQUIVALENT_DID |= {"M1372", "M1400", "M1402", "M1387", "M1364"}
RETIRED |= {"M1372", "M1400", "M1402", "M1387", "M1364"}

# LIBRARY_PRIMITIVE (ruling R1, amendment 64): a guard of a PUBLIC primitive
# that is dominated on EVERY production route -- every production input it
# refuses is refused on the same route by another named guard, so no
# production-route attack reaches it alone -- and whose retirement fails ONLY
# because a direct library test (one that calls the primitive itself) needs it.
# The row stays in the mutation run so the library test's kill is executed and
# recorded, but it is reported in its own class and NEVER counted killed. Each
# record names, per production route, the guard that refuses first (or later,
# on the same route and to the same outcome), and the library test.
LIB_RECORD = {
    "M965": {
        "property": "an episode is bound only to the policy whose bytes it ran",
        "routes": {
            "evl::evaluate (judge)": "M964 (evl.rs `&d.ep.policy_ref != policy_ref`), the statement before "
                                     "bind_episode; the arm's policy is keyed by its own digest",
            "intake::intake_episode": "impossible: the policy is store.get_contract(\"policies\", &ep.policy_ref), "
                                      "re-digested to that name by check_name (M978)"},
        "library_test": "axon-loop-contracts --test fixtures bind_episode_refuses_mismatches"},
    "M1295": {
        "property": "a shortlist is applied only under the pilot's controls",
        "routes": {
            "intake::intake_episode (check_ack)": "M1294 (bind_episode's controls clause), earlier on the same "
                                                  "policy and episode with only refusals between",
            "candidates::require_shortlist, evo::propose (x2), axon-reflex shortlist": "impossible: each passes "
                "the envelope's own controls_ref (or the request's, which it copied into the envelope)"},
        "library_test": "axon-loop-contracts --test fixtures shortlist_must_be_a_subset_of_the_eligible_view"},
    "M1297": {
        "property": "a paired-trial preflight binds only when observed == expected",
        "routes": {
            "intake::intake_episode": "M831 (intake.rs, the identical predicate on the same context, earlier); "
                                      "intake stores check_paired_trial_context's result, it does not refuse on it",
            "evl::evaluate": "LATER on the same trial: bind_episode's context binding (M857) refuses a delivered "
                             "context other than the intaken episode's, which passed M831; same outcome "
                             "(Unknown, Unbound), another reason"},
        "library_test": "axon-loop-contracts --test fixtures paired_trial_requires_exact_context_equality"},
    "M1298": {
        "property": "a preflight counts only at the current authority epoch",
        "routes": {
            "intake::intake_episode": "impossible: intake passes the context's own epoch as current",
            "evl::evaluate": "LATER on the same trial: bind_episode's epoch clause (M1281) with the same epoch "
                             "and context; same outcome (Unknown, Unbound), another reason"},
        "library_test": "axon-loop-contracts --test fixtures context_currency_and_roles"},
    "M1299": {
        "property": "only a trusted verifier independent of the subject establishes an outcome",
        "routes": {
            "intake::intake_episode": "LATER: verify_check_evidence's independence check (M10) with the same "
                                      "verifier and subject sets; Refused either way",
            "evl::evaluate": "LATER: verify_check_evidence (M10) in the same judge; the trial is Unknown either "
                             "way (the Unknown's kind differs: Unverifiable instead of Unbound)"},
        "library_test": "axon-loop-contracts --test fixtures bind_episode_refuses_mismatches"},
    "M1352": {
        "property": "resolve_opaque reads only a cl22 ref, byte for byte",
        "routes": {
            "tel::join -> PinnedSchedule::check_request (bin axon-loop tel join)": "the next statement, M1351 "
                "(`r != self.reference`): the pinned reference is cl22 (PinnedSchedule::pin), so a ref of "
                "another scheme never equals it; Refused either way. The `r.as_str() != o.as_str()` clause "
                "is always false (Ref::new keeps its input)"},
        "library_test": "axon-loop --test price_tel_sites resolve_opaque_never_reads_another_scheme_as_a_content_ref"},
    "M1363": {
        "property": "parse holds a document to its typed rules",
        "routes": {
            "every parse caller": "the schema walk earlier in the same parse (M1362) states every typed rule "
                                  "except two: the transition fence (next = expected + 1), which "
                                  "pointer::transition refuses LATER (M1329/M1330, Conflict), and ProfileOffer's "
                                  "list rules, which negotiate re-validates LATER (profile.rs)"},
        "library_test": "axon-loop-contracts --test parse_validate_sites parse_never_admits_a_noncontiguous_fence"},
    "M1369": {
        "property": "a policy claiming an authority expansion is refused by its typed rule",
        "routes": {
            "every parsed envelope (CLI, get_contract, contract_from_value)": "the schema walk's const check "
                "(M1212) earlier in the same parse",
            "envelopes built in code (evo.rs, axon-reflex shortlist.rs)": "impossible: both set "
                "authority_expansion: false literally; nothing in crates/*/src assigns the field"},
        "library_test": "axon-loop-contracts --test parse_validate_sites "
                        "a_policy_built_in_code_claiming_an_expansion_never_validates"},
    "M1401": {
        "property": "a shortlist is applied only in its own scope and candidate view",
        "routes": {
            "intake::intake_episode (check_ack)": "bind_episode's scope clause (M963) and candidate-view clause "
                                                  "(M1294), earlier on the same policy and episode",
            "candidates::require_shortlist, evo::propose (x2), axon-reflex shortlist": "impossible: each passes "
                "the envelope's own scope and candidate_set_ref"},
        "library_test": "axon-loop-contracts --test fixtures shortlist_must_be_a_subset_of_the_eligible_view"},
    "M1403": {
        "property": "a pass binds only when the verified bytes are the execution's output",
        "routes": {
            "evl::evaluate (judge, bind_acf's only production caller)": "bind_episode's checked-output clause "
                "(M1296) and bind_acf's output join (M1291), earlier: v.output == episode.output == "
                "receipt.output, and a passed episode names its output (LoopEpisode::validate)"},
        "library_test": "axon-loop --test evl bind_acf_alone_refuses_a_pass_over_other_bytes"},
}
LIBRARY_PRIMITIVE = set(LIB_RECORD)

# SIBLING_ONLY (integrator ruling, amendment 64): an edit that exists only as a
# member of a retired row's guard set. It is never an ACTIVE row, never retired
# and never counted killed; it needs no full-suite cell of its own because it
# is not retired (the full-suite condition applies to the RETIRED guard alone).
# These are integrate-A's schema clauses: the checked-in schema bytes are the
# MiCode package's, digest-pinned by axon-loop-contracts tests/fixtures.rs
# checked_in_schemas_are_the_package_bytes, so they cannot be mutated as
# standalone rows; each is a member of the named code row's set.
_PIN = ("the checked-in schema bytes are the MiCode package's, digest-pinned "
        "(fixtures.rs checked_in_schemas_are_the_package_bytes)")
SIBLING_RECORD = {r: {"member_of": m, "why": _PIN} for r, m in [
    ("M1208", ["M1247"]), ("M1209", ["M1248"]), ("M1227", ["M1255"]), ("M1236", ["M1256"]),
    ("M1241", ["M1258"]), ("M1244", ["M1261"]), ("M1253", ["M1262"]), ("M1263", ["M1265"]),
    ("M1269", ["M1250"])]}
SIBLING_ONLY = set(SIBLING_RECORD)
_lib_bad = (LIBRARY_PRIMITIVE & (RETIRED | SIBLING_ONLY)) | (SIBLING_ONLY & RETIRED)
assert not _lib_bad, f"a row in two classes: {sorted(_lib_bad)}"
assert all({"property", "routes", "library_test"} <= set(v) for v in LIB_RECORD.values())
# ── end INTEGRATE-2 ──

# ── C9 round 4b, INTEGRATE-3 (amendment 64) ── triage of the final 6-shard
# paired-disable at 1cf94ffc (144/155 held; each of the 11 failures had its
# four cells and failed only the full-suite cell).
#
# Five retirements fail ONLY on a direct library test of the primitive (the
# walker's keyword rules, the id newtypes' own tests) and are dominated on
# every production route: they become LIBRARY_PRIMITIVE (ruling R1), killed
# by that library test, never counted killed. M1232's retirement was FALSE:
# `axon-loop pointer revoke --policy` takes a Ref from a CLI flag, which no
# schema walk sees, and records it without comparing it with any digest, so
# check_hex64 is the only refusal on that route. It is ACTIVE again, killed
# on that route (tests/cli.rs).
_I3_PARSE = ("canonical::parse, the one entry of every Contract document (CLI --in documents, "
             "Store::get_contract, store::contract_from_value, intake/evl parse, fabric readiness "
             "parse_bytes)")
_I3_PILOT = "plan::PilotPlan::from_value (the only other caller of validate_against)"
_I3_ROWS = {
    "M1214": ("EVIDENCE (4b, integrate-A): a value matching no oneOf branch is refused (LIBRARY_PRIMITIVE, integrate-3)",
              "axon-loop-contracts", "--lib", "schema::tests::structural_rules"),
    "M1217": ("EVIDENCE (4b, integrate-A): a string not matching its pattern is refused (LIBRARY_PRIMITIVE, integrate-3)",
              "axon-loop-contracts", "--lib", "schema::tests::profile_id_pattern_is_exact"),
    "M1223": ("EVIDENCE (4b, integrate-A): a missing required field is refused by the schema (LIBRARY_PRIMITIVE, integrate-3)",
              "axon-loop-contracts", "--lib", "schema::tests::structural_rules"),
    "M1224": ("EVIDENCE (4b, integrate-A): a field no schema names is refused by the schema (LIBRARY_PRIMITIVE, integrate-3)",
              "axon-loop-contracts", "--lib", "schema::tests::structural_rules"),
    "M1233": ("EVIDENCE (4b, integrate-A): an ACF reference is acf1: (LIBRARY_PRIMITIVE, integrate-3)",
              "axon-loop-contracts", "--lib", "ids::tests::deserialize_validates_too"),
    "M1232": ("EVIDENCE (4b, integrate-A): a digest is 64 lowercase hex (ACTIVE, integrate-3: its retirement vs M1213 "
              "was false on the CLI revoke route)",
              "axon-loop", "--test cli", "a_revocation_naming_a_reference_that_is_not_a_digest_is_never_recorded"),
}
MUTATIONS = [(r[0], _I3_ROWS[r[0]][0]) + tuple(r[2:5]) + _I3_ROWS[r[0]][1:] if r[0] in _I3_ROWS else r
             for r in MUTATIONS]
for _r in _I3_ROWS:
    EQUIVALENT_DID.discard(_r)
    RETIRED.discard(_r)
    EQUIV_RECORD.pop(_r)
LIB_RECORD.update({
    "M1214": {
        "property": "a value matching no oneOf branch is refused",
        "routes": {
            _I3_PARSE: "typed serde, LATER in the same parse and on the same value: the checked-in oneOfs "
                       "are acf-compute-request approval_ref/semantic_state_ref (1..=512 string | null: "
                       "Option<OpaqueRef>, check_opaque M1234), acf-execution-receipt output_workspace_ref "
                       "(acf1 | null: Option<Acf1Ref>, check_acf1/check_hex64) and matched_checks/cost_micro "
                       "(integer 0..=2^53-1 | null: Option<u64> under parse_value's integer bound M1204), and "
                       "local-policy-pin ack (two closed objects: PinAck, internally tagged with "
                       "deny_unknown_fields; redteam a4/s07 passed in the 1cf94ffc full-suite cell with M1214 "
                       "removed). No checked-in oneOf has overlapping branches, so `n == 2` never occurs on a "
                       "production document",
            _I3_PILOT: "impossible: the pilot schema has no oneOf"},
        "library_test": "axon-loop-contracts --lib schema::tests::structural_rules (a two-branch overlap no "
                        "checked-in schema has)"},
    "M1217": {
        "property": "a string not matching its schema pattern is refused",
        "routes": {
            _I3_PARSE: "the typed newtype of the same field, LATER in the same parse: the id pattern -> "
                       "check_id (TaskId/ArmId/TrialId/AttemptId/OperationId/ExecutionId/CandidateId/PolicyId/"
                       "ContextId/TransitionId/TenantId/TaskFamily/RepoId/WorktreeId), the ref pattern -> "
                       "check_ref (Ref), ^acf1: -> check_acf1 (Acf1Ref, M1233), ^[A-Z]{3}$ -> check_currency "
                       "(Currency), the profile/peer patterns -> ProfileId/PeerId::new, which call "
                       "schema::pattern_matches directly, never this `if !ok`",
            _I3_PILOT: "strict_record's typed serde (Scope ids, Vec<Ref>/Option<Ref>) and, for experiment_id "
                       "(a String field), the TaskId::new check that follows in from_value"},
        "library_test": "axon-loop-contracts --lib schema::tests::profile_id_pattern_is_exact. M1217 and "
                        "M1233 dominate each other on the contract route (the pd6 four-cell pair at 1cf94ffc: "
                        "each removed alone refused, both removed admitted a cl22: workspace ref); neither is "
                        "counted killed"},
    "M1223": {
        "property": "a missing required field is refused",
        "routes": {
            _I3_PARSE: "typed serde, LATER in the same parse: every required field of every checked-in "
                       "schema (top level, nested objects, the profile `then` lists, the pin ack branches) is "
                       "a non-defaulted field of its contract type -- crates/axon-loop-contracts/src has no "
                       "#[serde(default)], and a nullable field is `deserialize_with = nullable` without "
                       "`default`, which serde requires present; M1225 (retired, joint with M1223) is the "
                       "defaulted-field row. A `required` inside an `if` only selects a `then`; the field it "
                       "names is required at top level too",
            _I3_PILOT: "strict_record: PilotPlan has no defaulted field (every nullable field is "
                       "`deserialize_with`), and the canonical round-trip refuses a defaulted one"},
        "library_test": "axon-loop-contracts --lib schema::tests::structural_rules"},
    "M1224": {
        "property": "a field no schema names is refused",
        "routes": {
            _I3_PARSE: "typed serde, LATER in the same parse: every additionalProperties:false object of the "
                       "checked-in schemas is a deny_unknown_fields struct or enum of its contract type "
                       "(PinAck's acknowledged-with-extra-field case: redteam s07/a4 passed in the 1cf94ffc "
                       "full-suite cell with M1224 removed); M1226 (retired, joint with M1224) is the "
                       "dropped-deny_unknown_fields row",
            _I3_PILOT: "strict_record: PilotPlan and Scope are deny_unknown_fields, and the canonical "
                       "round-trip refuses any field the typed value does not re-emit"},
        "library_test": "axon-loop-contracts --lib schema::tests::structural_rules"},
    "M1233": {
        "property": "an ACF reference is acf1:",
        "routes": {
            _I3_PARSE: "the schema pattern ^acf1: (M1217) EARLIER in the same parse, on every Acf1Ref field "
                       "(output_workspace_ref's oneOf/anyOf branches carry the same pattern)",
            "evl::judge's protected-evidence join (serde_json::from_value of the verification request and "
            "receipt)": "LATER on the same documents: verify_check_evidence parses both through "
                        "canonical::parse (M1217), same outcome (Unknown)",
            "axon-fabric (workspace.rs reference, submit.rs fabric_*_digest, branches.rs/submit.rs journal "
            "reads)": "impossible: every Acf1Ref it holds is a digest it computed, or one it wrote to its own "
                      "journal or state"},
        "library_test": "axon-loop-contracts --lib ids::tests::deserialize_validates_too"},
})
LIBRARY_PRIMITIVE = set(LIB_RECORD)
# C9 round 4c triage (gpumaster paired-disable at 16500980): M1266's retirement claimed the
# full package suite stays green with the guard removed. It does not: fixtures.rs
# transition_epoch_must_be_contiguous and parse_validate_sites.rs
# parse_never_admits_a_noncontiguous_fence both go red (each calls parse directly). Dominated on
# the ONE production route (bin axon-loop pointer transition: parse, then pointer::transition,
# whose CAS M1329 and next == current + 1 rule M1330 imply this one), so it is a library
# primitive (ruling R1, as M1214/M1217/M1223/M1224/M1233), not a retirement.
MUTATIONS = [r if r[0] != 'M1266' else
             ('M1266', "EVIDENCE (4b, integrate-A): a transition's fence is contiguous (next = expected + 1) (LIBRARY_PRIMITIVE, triage)")
             + tuple(r[2:5]) + ('axon-loop-contracts', '--test fixtures', 'transition_epoch_must_be_contiguous')
             for r in MUTATIONS]
EQUIVALENT_DID.discard('M1266')
RETIRED.discard('M1266')
EQUIV_RECORD.pop('M1266')
LIB_RECORD['M1266'] = {
    "property": "a transition's fence is contiguous (next = expected + 1)",
    "routes": {
        "every parse caller (the only one is bin axon-loop pointer transition)":
            "LATER on the same route: pointer::transition requires expected_epoch == current (M1329) and "
            "next_epoch == current + 1 (M1330), nothing written between; Conflict (exit 5) where parse "
            "gives Refused (4)",
        "any other route": "impossible: no other code constructs, parses or reads a PolicyTransition "
                           "(grep crates/*/src)"},
    "library_test": "axon-loop-contracts --test fixtures transition_epoch_must_be_contiguous (also "
                    "parse_validate_sites parse_never_admits_a_noncontiguous_fence)"}
LIBRARY_PRIMITIVE = set(LIB_RECORD)

# A real defect found by the M58 triage (it is NOT M58's): `axon-os run`
# reset every kill latch to clear as it started, so a kill ARMED before the
# run (which `axon-os kill` promises "a run starting with this id will pick
# it up") was discarded and the job ran to its timeout, exit 8. That was
# acc_a1_smoke_kill_journey's exit 8 under load; reproduced by arming first.
MUTATIONS += [
    ("M1450", "axon-os (4b, integrate-3): a kill armed before its run starts stops the run (the latch is "
              "created clear only when absent)",
     "crates/axon-os/src/cli.rs",
     "            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}\n",
     "            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {\n"
     "                let _ = std::fs::write(&kf, r#\"{\"latch\":\"clear\"}\"#);\n"
     "            }\n",
     "axon-os", "--test r27_acceptance", "a_kill_armed_before_its_run_starts_stops_the_run"),
]
_lib_bad = (LIBRARY_PRIMITIVE & (RETIRED | SIBLING_ONLY)) | (SIBLING_ONLY & RETIRED)
assert not _lib_bad, f"a row in two classes: {sorted(_lib_bad)}"
assert set(_I3_ROWS) - {"M1232"} <= LIBRARY_PRIMITIVE and not set(_I3_ROWS) & (RETIRED | EQUIVALENT_DID)
# ── end INTEGRATE-3 ──

PSV_IDS = {f"M{n}" for n in range(137, 550)}
# C9 round 3: rows M560-M649 are PSV rows (workstream ranges).
PSV_IDS |= {f"M{n}" for n in range(550, 650)}
# C9 round 4: M650-M699.
PSV_IDS |= {f"M{n}" for n in range(650, 700)}
# C9 round 4 (harness2): M720-M739.
PSV_IDS |= {f"M{n}" for n in range(720, 740)}
# C9 round 4 (harness2, second wave): M860-M879.
PSV_IDS |= {f"M{n}" for n in range(860, 880)}
# C9 round 4 fix wave: readiness M740-M759.
PSV_IDS |= {f"M{n}" for n in range(740, 760)}

# C9 round 4: M650-M699, and the rows workstream's M690-M719.
PSV_IDS |= {f"M{n}" for n in range(650, 720)}
# C9 round 4 fix wave: rows2 M760-M819, and wave 2 up to M859.
PSV_IDS |= {f"M{n}" for n in range(760, 860)}
# C9 round 4 fix wave: rows3 M880-M899 (amendment 59).
PSV_IDS |= {f"M{n}" for n in range(880, 900)}
# C9 round 4b fix wave: core2 M920-M939 and M1140-M1179 (amendment 60).
PSV_IDS |= {f"M{n}" for n in range(920, 940)}
PSV_IDS |= {f"M{n}" for n in range(1140, 1180)}
# C9 round 4b fix wave: harness3 M1180-M1199 (amendment 63).
PSV_IDS |= {f"M{n}" for n in range(1180, 1200)}

# ── C9 round 4b, workstream ROWS4B (M1020-M1139; amendment 62): EQUIVALENCE (4),
# Fabric side. Round 4b found the protected Fabric decision files outside the
# refusal-site scan, with refusals no row covered: the root helper's REPORT
# (schema, exit, post-launch error, the same-byte property of decisions A/D),
# the protected executable, and accept_b263 / the qualification's own rules;
# with all five report/executable refusals removed the whole axon-fabric
# suite stayed green. Each ACTIVE row below is killed by its OWN attack through
# a production route (submit on the helper route; Fabric's qualification AND
# readiness's B263 decision for the shared accept_b263 rules; the built
# axon-provenance for the lineage naming; the axon-fabric binary). Rows
# retired EQUIVALENT_DID carry a four-cell record (EQUIV_RECORD below).
_FB = 'crates/axon-fabric/src/backend.rs'
_FS = 'crates/axon-fabric/src/submit.rs'
_FG = 'crates/axon-fabric/src/git_data.rs'
_FP = 'crates/axon-fabric/src/provenance.rs'
_FC = 'crates/axon-fabric/src/bin/axon-fabric.rs'
_FW = 'crates/axon-fabric/src/workspace.rs'
_OT = 'crates/axon-loop-contracts/src/operator_trust.rs'
_TP = '--test psv_dispatch'
_TR = '--test readiness_attribution'
_TQ = '--test qualification'
_TS = '--test submit'
_TG = '--test guest_policy'
_TV = '--test guest_provenance'
MUTATIONS += [
    # ── the privileged helper's report (run_privileged) ──
    ('M1020', "A (rows4b): a helper report of another schema yields no verdict", _FB,
     '    if report.schema != pl::REPORT_SCHEMA {', '    if false && report.schema != pl::REPORT_SCHEMA {',
     'axon-fabric', _TP, 'a_helper_report_of_another_schema_yields_no_verdict'),
    ('M1021', "A (rows4b): a helper that did not exit EXIT_LAUNCHED yields no verdict", _FB,
     '    if status != Some(pl::EXIT_LAUNCHED) || report.error.is_some() {',
     '    if false || report.error.is_some() {',
     'axon-fabric', _TP, 'a_helper_exit_other_than_launched_yields_no_verdict'),
    ('M1022', "A (rows4b): a helper error after the launch yields no verdict", _FB,
     '    if status != Some(pl::EXIT_LAUNCHED) || report.error.is_some() {',
     '    if status != Some(pl::EXIT_LAUNCHED) || false {',
     'axon-fabric', _TP, 'a_helper_error_after_the_launch_yields_no_verdict'),
    ('M1023', "D (rows4b): a launcher or interpreter that changed during the launch (report.unchanged) yields no verdict", _FB,
     '    if !report.unchanged || helper.unchanged().is_err() {',
     '    if false || helper.unchanged().is_err() {',
     'axon-fabric', _TP, 'a_launcher_that_changed_during_the_launch_yields_no_verdict'),
    ('M1024', "D (rows4b): a helper that changed during the launch yields no verdict", _FB,
     '    if !report.unchanged || helper.unchanged().is_err() {',
     '    if !report.unchanged || false {',
     'axon-fabric', _TP, 'a_helper_that_changed_during_the_launch_yields_no_verdict'),
    # ── accept_b263: ONE implementation, both routes ──
    ('M1025', "PSV-7 (rows4b): RULE:issuer-claimed, Fabric and readiness", _FB,
     '    if ev["issuer_key_id"].as_str() != Some(issuer) {', '    if false && ev["issuer_key_id"].as_str() != Some(issuer) {',
     'axon-fabric', _TR, 'a_b263_record_naming_another_issuer_qualifies_nothing_on_either_route'),
    ('M1026', "PSV-7 (rows4b): RULE:pass-count, Fabric and readiness", _FB,
     '    if ev["counts"]["PASS"].as_u64().unwrap_or(0) == 0 {', '    if false && ev["counts"]["PASS"].as_u64().unwrap_or(0) == 0 {',
     'axon-fabric', _TR, 'a_b263_record_counting_no_pass_qualifies_nothing_on_either_route'),
    ('M1027', "PSV-7 (rows4b): RULE:blocked-count, Fabric and readiness", _FB,
     '    if ev["counts"]["BLOCKED"].as_u64() != Some(blocked.len() as u64) {',
     '    if false && ev["counts"]["BLOCKED"].as_u64() != Some(blocked.len() as u64) {',
     'axon-fabric', _TR, 'a_b263_record_whose_blocked_count_disagrees_qualifies_nothing_on_either_route'),
    ('M1028', "PSV-7 (rows4b): RULE:blocked-unwaived, Fabric and readiness", _FB,
     '        if !waivers.contains_key(name) {', '        if false && !waivers.contains_key(name) {',
     'axon-fabric', _TR, 'an_unwaived_blocked_assertion_qualifies_nothing_on_either_route'),
    ('M1029', "PSV-7 (rows4b): RULE:waiver-reason, Fabric and readiness", _FB,
     '        if waivers.get(name).is_some_and(|w| w.reason.is_empty()) {',
     '        if false && waivers.get(name).is_some_and(|w| w.reason.is_empty()) {',
     'axon-fabric', _TR, 'a_waiver_stating_no_reason_qualifies_nothing_on_either_route'),
    ('M1030', "PSV-7 (rows4b): RULE:waiver-expiry, Fabric and readiness", _FB,
     '            .is_some_and(|w| w.expires.is_none_or(|t| now >= t))', '            .is_some_and(|_| false)',
     'axon-fabric', _TR, 'an_expired_waiver_qualifies_nothing_on_either_route'),
    ('M1031', "PSV-7 (rows4b): RULE:end-not-future (the only rule under no maximum age)", _FB,
     '    if end > now {', '    if false && end > now {',
     'axon-fabric', _TQ, 'a_record_from_the_future_never_qualifies_even_with_no_maximum_age'),
    ('M1032', "PSV-7 (rows4b): RULE:engine-digests (readiness joins only firecracker)", _FB,
     '    if !is_hex64(&eng["firecracker_sha256"]) || !is_hex64(&eng["jailer_sha256"]) {',
     '    if false && (!is_hex64(&eng["firecracker_sha256"]) || !is_hex64(&eng["jailer_sha256"])) {',
     'axon-fabric', _TR, 'a_b263_record_naming_no_jailer_qualifies_nothing_on_either_route'),
    ('M1033', "PSV-7 (rows4b): RULE:caveat, Fabric and readiness", _FB,
     '    if caveat.is_empty() {', '    if false && caveat.is_empty() {',
     'axon-fabric', _TR, 'a_b263_record_stating_no_caveat_qualifies_nothing_on_either_route'),
    ('M1034', "PSV-7 (rows4b): RULE:waiver-bound (parse_waivers), Fabric and readiness", _FB,
     '    if w["evidence_sha256"].as_str() != Some(evidence_sha256) {',
     '    if false && w["evidence_sha256"].as_str() != Some(evidence_sha256) {',
     'axon-fabric', _TR, 'a_waiver_bound_to_another_record_qualifies_nothing_on_either_route'),
    ('M1035', "PSV-7 (rows4b): RULE:issuer-trusted (verify_evidence_signature), Fabric and readiness", _OT,
     '    if !trusted.contains(&pk) {', '    if false && !trusted.contains(&pk) {',
     'axon-fabric', _TR, 'a_b263_record_signed_by_an_untrusted_key_naming_itself_qualifies_nothing'),
    # ── the qualification's own rules (this host's manifest) ──
    ('M1036', "PSV-7 (rows4b): the qualification record's schema", _FB,
     '        if ev["schema"] != "axon-b263-evidence/1" {', '        if false && ev["schema"] != "axon-b263-evidence/1" {',
     'axon-fabric', _TQ, 'a_record_of_another_schema_never_qualifies'),
    ('M1037', "PSV-7 (rows4b): the qualification record's profile", _FB,
     '        if ev["profile"]["name"] != LINUX_MICROVM_PROTECTED.id {',
     '        if false && ev["profile"]["name"] != LINUX_MICROVM_PROTECTED.id {',
     'axon-fabric', _TQ, 'a_record_for_another_profile_never_qualifies'),
    ('M1038', "PSV-7 (rows4b): RULE:engine-pin required (retired: a missing pin never equals the record's digest, M1039)", _FB,
     '        if !is_hex64(&pin["firecracker_sha256"]) || !is_hex64(&pin["jailer_sha256"]) {',
     '        if false && (!is_hex64(&pin["firecracker_sha256"]) || !is_hex64(&pin["jailer_sha256"])) {',
     'axon-fabric', _TQ, 'a_manifest_pinning_no_engine_never_qualifies'),
    ('M1039', "PSV-7 (rows4b): RULE:engine-pin equality", _FB,
     '        if pin["firecracker_sha256"] != eng["firecracker_sha256"]\n            || pin["jailer_sha256"] != eng["jailer_sha256"]\n        {',
     '        if false\n        {',
     'axon-fabric', _TQ, 'engine_digests_other_than_the_manifests_pins_never_qualify'),
    ('M1040', "PSV-7 (rows4b): RULE:manifest-clean", _FB,
     '        if m["source"]["axon_tree_dirty_at_build"] != serde_json::Value::Bool(false) {',
     '        if false && m["source"]["axon_tree_dirty_at_build"] != serde_json::Value::Bool(false) {',
     'axon-fabric', _TQ, 'a_manifest_built_from_a_dirty_tree_never_qualifies'),
    ('M1041', "PSV-7 (rows4b): the record qualifies THIS manifest", _FB,
     '        if evidence_manifest_sha256 != manifest_sha256 {', '        if false && evidence_manifest_sha256 != manifest_sha256 {',
     'axon-fabric', _TQ, 'a_record_qualifying_another_manifest_never_qualifies_this_one'),
    ('M1042', "PSV-7 (rows4b): a waiver file is an axon-b263-waiver/1 (Fabric's configured waivers)", _FB,
     '    if w["schema"] != WAIVER_SCHEMA {', '    if false && w["schema"] != WAIVER_SCHEMA {',
     'axon-fabric', _TQ, 'a_waiver_file_of_another_schema_waives_nothing'),
    ('M1043', "PSV-7 (rows4b): an empty qualification root trusts nothing (retired: RULE:issuer-trusted, M1035)", _FB,
     '    if keys.is_empty() {', '    if false && keys.is_empty() {',
     'axon-fabric', _TQ, 'an_empty_qualification_root_qualifies_nothing'),
    # ── backend selection ──
    ('M1044', "select (rows4b): an architecture the backend does not offer", _FB,
     '    if !p.architectures.contains(&r.architecture) {', '    if false && !p.architectures.contains(&r.architecture) {',
     'axon-fabric', _TS, 'an_architecture_no_backend_offers_never_runs_on_the_host'),
    ('M1045', "select (rows4b): a checkpoint kind the backend does not offer", _FB,
     '    if !p.checkpoint_kinds.contains(&r.checkpoint_kind) {', '    if false && !p.checkpoint_kinds.contains(&r.checkpoint_kind) {',
     'axon-fabric', _TS, 'a_checkpoint_kind_no_backend_offers_never_runs_on_the_host'),
    ('M1046', "select (rows4b): a reproducible (hermetic) grant is never run", _FB,
     '    if needs.reproducible {', '    if false && needs.reproducible {',
     'axon-fabric', _TS, 'a_reproducible_grant_never_runs_non_reproducibly'),
    ('M1047', "select (rows4b): a brokered network request is never run unbrokered", _FB,
     '    if r.network_mode == NetworkMode::Brokered {', '    if false && r.network_mode == NetworkMode::Brokered {',
     'axon-fabric', _TS, 'a_brokered_network_request_never_runs_unbrokered'),
    ('M1048', "select (rows4b): only the Axon interpreter engine is offered", _FB,
     '    if r.engine != Engine::AxonInterpreter {', '    if false && r.engine != Engine::AxonInterpreter {',
     'axon-fabric', _TS, 'another_engine_is_never_substituted_by_the_interpreter'),
    ('M1049', "select (rows4b): x1, a restricting grant needs the qualified guest policy channel", _FB,
     '        if needs.guest_policy_channel && !q.guest_policy_channel {',
     '        if false && needs.guest_policy_channel && !q.guest_policy_channel {',
     'axon-fabric', _TG, 'a_restricting_grant_is_never_launched_without_an_x1_pass'),
    ('M1050', "select (rows4b): x2, a path-scoped grant on the protected profile", _FB,
     '        if needs.path_scoped_grant {\n            return Err(Unsupported(format!(\n                "{}: the grant is path-scoped',
     '        if false && needs.path_scoped_grant {\n            return Err(Unsupported(format!(\n                "{}: the grant is path-scoped',
     'axon-fabric', _TG, 'a_path_scoped_grant_is_never_launched_on_the_protected_profile'),
    ('M1051', "select (rows4b): os=linux without hardware isolation never falls to the host", _FB,
     '    if r.os == Os::Linux {', '    if false && r.os == Os::Linux {',
     'axon-fabric', _TS, 'os_linux_without_hardware_isolation_never_runs_on_the_host'),
    ('M1052', "select (rows4b): hardware isolation is never dropped to the host interpreter", _FB,
     '    if r.hardware_isolation {\n        let p = FIRECRACKER_AXON_KERNEL;',
     '    if false && r.hardware_isolation {\n        let p = FIRECRACKER_AXON_KERNEL;',
     'axon-fabric', _TS, 'hardware_isolation_linux_is_refused_without_a_qualified_profile'),
    ('M1053', "select (rows4b): a path-scoped grant never runs under the host's coarse ceiling", _FB,
     '    if needs.path_scoped_grant {\n        // The host interpreter',
     '    if false && needs.path_scoped_grant {\n        // The host interpreter',
     'axon-fabric', _TS, 'a_path_scoped_grant_never_runs_under_the_hosts_coarse_ceiling'),
    ('M1054', "select (rows4b): the host backend runs registered checks only", _FB,
     '    if !p.job_kinds.contains(&req.job_kind) {\n        return Err(Unsupported(format!(\n            "{}: job_kind {:?} unsupported (it runs registered checks only)",',
     '    if false && !p.job_kinds.contains(&req.job_kind) {\n        return Err(Unsupported(format!(\n            "{}: job_kind {:?} unsupported (it runs registered checks only)",',
     'axon-fabric', _TS, 'an_interpreter_run_never_runs_on_the_host_backend'),
    ('M1055', "x1 (rows4b): a guest policy the cmdline would truncate is never launched", _FB,
     '        if word > GUEST_POLICY_WORD_MAX {', '        if false && word > GUEST_POLICY_WORD_MAX {',
     'axon-fabric', _TG, 'a_policy_the_guest_cmdline_would_truncate_is_never_launched'),
    # ── submit ──
    ('M1056', "PSV-6 (rows4b): the protected profile runs only the rootfs interpreter", _FS,
     '        if id != backend::LINUX_GUEST_AXON_ID {', '        if false && id != backend::LINUX_GUEST_AXON_ID {',
     'axon-fabric', _TP, 'a_protected_request_naming_another_executable_is_refused'),
    ('M1057', "PSV-6 (rows4b): the protected request's executable digest is the qualified guest interpreter's", _FS,
     '        if req.executable_digest != executable_digest(id, &e) {', '        if false && req.executable_digest != executable_digest(id, &e) {',
     'axon-fabric', _TP, 'a_protected_request_with_another_executable_digest_is_refused'),
    ('M1058', "PSV-6 (rows4b): an absent authority store is not epoch 0", _FS,
     '                if !store.join("config.json").is_file() {', '                if false && !store.join("config.json").is_file() {',
     'axon-fabric', '--test peer_failure_matrix', 'an_unreadable_epoch_store_at_submit_refuses_and_records_nothing'),
    ('M1059', "submit (rows4b): a registered check's argv is [file] or [file, filter], nothing ignored", _FS,
     '        (JobKind::RegisteredCheck, _) => {',
     '        (JobKind::RegisteredCheck, [f, flt, ..]) => (f.clone(), Some(flt.clone())),\n        (JobKind::RegisteredCheck, _) => {',
     'axon-fabric', _TP, 'a_protected_check_with_an_extra_argument_is_refused'),
    ('M1060', "submit (rows4b): no symbolic link in a candidate or suite (refuse_links, every call)", _FS,
     '        if is_link {', '        if false && is_link {',
     'axon-fabric', '--test check_effects', 'a_candidate_holding_a_symlink_is_refused'),
    ('M1061', "PSV-5 (rows4b): an operator suite judges only at its registered version", _FS,
     '    if tree.reference().as_str() != c.workspace_version_ref {', '    if false && tree.reference().as_str() != c.workspace_version_ref {',
     'axon-fabric', _TP, 'a_suite_edited_after_registration_never_judges'),
    ('M1062', "submit (rows4b): the axon-os supervisor's refusal is final", _FS,
     '        other => Err(format!("axon-os supervisor refused: {other:?}")),', '        _ => Ok(rec.approval),',
     'axon-fabric', '--test grant_authority', 'admission_uses_the_request_grant_and_denies_before_launch'),
    ('M1063', "submit (rows4b): one op id, one input", _FS,
     '        if v.intent.input_digest != input_digest {', '        if false && v.intent.input_digest != input_digest {',
     'axon-fabric', _TS, 'same_op_different_input_is_a_conflict_with_zero_effects'),
    ('M1064', "PSV-6 (rows4b): an orphan is never resumed under a superseded epoch", _FS,
     '                if v.intent.authority_epoch != cfg.expected_epoch {', '                if false && v.intent.authority_epoch != cfg.expected_epoch {',
     'axon-fabric', '--test restart_matrix', 'an_orphan_is_not_resumed_under_a_superseded_epoch'),
    ('M1065', "PSV-6 (rows4b): a stale-epoch request is never journalled", _FS,
     '    if current.as_ref().ok() != Some(&cfg.expected_epoch) {', '    if false && current.as_ref().ok() != Some(&cfg.expected_epoch) {',
     'axon-fabric', _TS, 'epoch_mismatch_at_submit_is_refused_with_zero_effects'),
    ('M1066', "B271 (rows4b): a cancelled branch never runs again", _FS,
     '        if branches.is_cancelled(&exp.experiment_id, &br.arm_id) {\n            // A pre-launch orphan of a cancelled branch is released here: a\n            // crash inside `Branches::cancel` (marker written, ops not yet\n            // cancelled) must not leave its reservation held until someone\n            // happens to re-run the cancel. Nothing launched, so nothing to bill.\n            if resume.is_some() {',
     '        if false && branches.is_cancelled(&exp.experiment_id, &br.arm_id) {\n            // A pre-launch orphan of a cancelled branch is released here: a\n            // crash inside `Branches::cancel` (marker written, ops not yet\n            // cancelled) must not leave its reservation held until someone\n            // happens to re-run the cancel. Nothing launched, so nothing to bill.\n            if resume.is_some() {',
     'axon-fabric', '--test branches', 'a_cancelled_branch_never_runs_again'),
    ('M1067', "PSV-6 (rows4b): the epoch is re-read before the launch (every route)", _FS,
     '    if now.as_ref().ok() != Some(&cfg.expected_epoch) {', '    if false && now.as_ref().ok() != Some(&cfg.expected_epoch) {',
     'axon-fabric', _TS, 'an_epoch_change_between_submit_and_launch_is_refused_before_the_launch_record'),
    ('M1068', "submit (rows4b): a request never spends more than its grant's budget", _FS,
     '    if i64::try_from(req.limits.max_cost_micro).map_or(true, |c| c > cap) {',
     '    if false && i64::try_from(req.limits.max_cost_micro).map_or(true, |c| c > cap) {',
     'axon-fabric', '--test grant_authority', 'admission_uses_the_request_grant_and_denies_before_launch'),
    ('M1069', "submit (rows4b): the all-zero placeholder policy governs nothing", _FS,
     '    if req.policy_digest.as_str() == PLACEHOLDER_POLICY_DIGEST {', '    if false && req.policy_digest.as_str() == PLACEHOLDER_POLICY_DIGEST {',
     'axon-fabric', '--test grant_authority', 'the_all_zero_policy_digest_placeholder_is_refused_with_zero_effects'),
    # ── provenance / lineage (git_data, provenance) ──
    ('M1070', "FIELD-ORIGIN (rows4b): the provenance allowlist chain is root-owned", _FG,
     '        if m.uid() != 0 {', '        if false && m.uid() != 0 {',
     'axon-fabric', '--lib', 'provenance::tests::an_allowlist_that_is_not_operator_owned_excuses_nothing'),
    ('M1071', "FIELD-ORIGIN (rows4b): the provenance allowlist chain is not group/other-writable", _FG,
     '        if m.mode() & 0o022 != 0 {', '        if false && m.mode() & 0o022 != 0 {',
     'axon-fabric', '--lib', 'provenance::tests::an_allowlist_that_is_not_operator_owned_excuses_nothing'),
    ('M1072', "FIELD-ORIGIN (rows4b): a symlinked .git is no clone (discover; retired vs M1085)", _FG,
     '        _ => Err(format!(\n            "{} is not a directory (a gitfile or symlink',
     '        _ if true => Ok(top),\n        _ => Err(format!(\n            "{} is not a directory (a gitfile or symlink',
     'axon-fabric', _TV, 'a_symlinked_git_dir_is_never_a_clean_build_tree'),
    ('M1073', "FIELD-ORIGIN (rows4b): a certified revision naming no commit is no lineage", _FG,
     '            other => return Err(format!("{rev} is a {other}, not a commit")),', '            _ => return Ok(()),',
     'axon-fabric', _TV, 'a_certified_revision_naming_a_blob_is_no_lineage'),
    ('M1074', "FIELD-ORIGIN (rows4b): a tag chain past the peel limit is no lineage", _FG,
     '    Err(format!("{rev}: too many nested tags"))', '    Ok(())',
     'axon-fabric', _TV, 'a_tag_chain_deeper_than_the_peel_limit_is_no_lineage'),
    ('M1075', "FIELD-ORIGIN (rows4b): an ambiguous abbreviation is no lineage", _FG,
     '        _ => Err(format!(\n            "{rev} is ambiguous',
     '        (Some(one), _) => Ok(one.to_string()),\n        _ => Err(format!(\n            "{rev} is ambiguous',
     'axon-fabric', _TV, 'an_ambiguous_abbreviation_is_no_lineage'),
    ('M1076', "FIELD-ORIGIN (rows4b): build provenance calls the repository-config refusal", _FP,
     '    if let Err(e) = git_data::refuse_config(&top) {\n        return unknown(e);\n    }',
     '    let _ = git_data::refuse_config(&top);',
     'axon-fabric', '--lib', 'provenance::tests::a_filter_driver_in_the_repository_config_never_runs'),
    # ── the axon-fabric binary ──
    ('M1077', "D1 (rows4b): status/cancel serve an op only to the principal|grant it was submitted under", _FC,
     '    if v.intent.authority_ref != format!("{principal}|{grant}") {', '    if false && v.intent.authority_ref != format!("{principal}|{grant}") {',
     'axon-fabric', '--test grant_registry_authority', 'status_and_cancel_authorize_before_any_write_and_reconcile_only_their_scope'),
    ('M1078', "O1 (rows4b): the protected signer key derives its pinned public key", _FC,
     '    if pk != field("public_key") {', '    if false && pk != field("public_key") {',
     'axon-fabric', '--test protected_host', 'the_host_signer_key_must_be_private_and_match_its_pin'),
    ('M1079', "O1 rule 1 (rows4b): the protected signer key is readable by no one else", _FC,
     'meta.mode() & 0o277 != 0', 'meta.mode() & 0o200 != 0',
     'axon-fabric', '--test protected_host', 'the_host_signer_key_must_be_private_and_match_its_pin'),
    ('M1080', "O1 rule 1 (rows4b): the protected signer key is owned by the Fabric uid", _FC,
     'if meta.uid() != euid || ', 'if false || ',
     'axon-fabric', '--test protected_host', 'the_host_signer_key_must_be_private_and_match_its_pin'),
    # ── the workspace store (retired as mutual pairs, four-cell records) ──
    ('M1081', "PSV-5 (rows4b): a blob is re-verified against its name before it is materialized (retired vs M1082)", _FW,
     '            if sha256_hex(&content) != e.sha256 || content.len() as u64 != e.size {', '            if false {',
     'axon-fabric', '--test workspace', 'a_blob_holding_other_bytes_never_materializes'),
    ('M1082', "PSV-5 (rows4b): the materialized tree re-derives to its reference (retired vs M1081)", _FW,
     '        if t.reference() != *r {', '        if false && t.reference() != *r {',
     'axon-fabric', '--test workspace', 'a_blob_holding_other_bytes_never_materializes'),
    ('M1085', "FIELD-ORIGIN (rows4b): refuse_config locates no git dir behind a symlinked .git (retired vs M1072)", _FG,
     '        _ => return Err(format!("{} is not a git directory", dotgit.display())),', '        _ => dotgit,',
     'axon-fabric', _TV, 'a_symlinked_git_dir_is_never_a_clean_build_tree'),
    # A91: the protected lineage names the certified revision by its whole hash.
    ('M1084', "FIELD-ORIGIN (rows4b, A91): the protected PCI lineage takes only a full commit id", _FP,
     '    if !git_data::is_oid(rev) {', '    if false && !git_data::is_oid(rev) {',
     'axon-fabric', _TV, 'an_abbreviated_certified_revision_never_answers_the_protected_lineage'),
    ('M1083', "PSV-5 (rows4b): a manifest hashes to its reference (retired vs M1082)", _FW,
     '        if workspace_version_ref(&manifest) != r.as_str() {', '        if false && workspace_version_ref(&manifest) != r.as_str() {',
     'axon-fabric', '--test workspace', 'a_manifest_that_is_not_its_versions_never_materializes'),
]
PSV_IDS |= {f"M{n}" for n in range(1020, 1140)}
# Retired EQUIVALENT_DID (rows4b): each has a dominating guard on EVERY path,
# with an executed four-cell record (v022_paired_disable.py --only=...).
EQUIV_RECORD["M1038"] = {
    "property": "a qualification binds the record's engine digests to pins the host manifest states",
    "subsumed_by": ["M1039"], "killer": "joint:M1038+M1039",
    "all_paths": "qualification() is the only reader of the manifest's engine block; accept_b263 has "
                 "already required both record digests to be 64 hex (RULE:engine-digests, M1032) before "
                 "this check, and the next statement (M1039) requires each pin to EQUAL the record's "
                 "digest: a pin that is absent or not 64 hex is never equal to a 64-hex string, so the "
                 "equality refuses every manifest this rule refuses"}
EQUIV_RECORD["M1043"] = {
    "property": "a qualification root holding no key qualifies nothing",
    "subsumed_by": ["M1035"], "killer": "joint:M1043+M1035",
    "all_paths": "trusted_issuers() returns its keys only to verify_detached / "
                 "verify_evidence_signature (callers: QualificationTrust::trusted_keys -> qualification() "
                 "for the record and its waivers; verify_operator_evidence; "
                 "verify_operator_evidence_signed; verify_operator_evidence_bytes); each passes them as "
                 "`trusted` to operator_trust::verify_evidence_signature, whose RULE:issuer-trusted "
                 "(M1035) refuses every signature whose key is not in `trusted`, which an empty list "
                 "never contains"}
EQUIV_RECORD["M1081"] = {
    "property": "the store never materializes a blob that does not hold its name's bytes",
    "subsumed_by": ["M1082"], "killer": "joint:M1081+M1082",
    "all_paths": "tree() is the only reader of blob content (materialize and materialize_projection "
                 "call it); after this check it builds the tree from the bytes READ and requires its "
                 "re-derived reference to equal r (M1082): the reference covers every entry's sha256 "
                 "and size, so a blob whose bytes are not its name's changes the reference"}
EQUIV_RECORD["M1082"] = {
    "property": "the store never materializes a blob that does not hold its name's bytes",
    "subsumed_by": ["M1081"], "killer": "joint:M1081+M1082",
    "all_paths": "tree() reads entries from load() (the manifest hashes to r, M1083) and refuses any "
                 "blob whose bytes do not hash to the manifest's sha256 or size (M1081) before "
                 "re-deriving: the re-derived reference is then the manifest's own, which hashes to r"}
EQUIV_RECORD["M1083"] = {
    "property": "the store never materializes a manifest that is not its version's",
    "subsumed_by": ["M1082"], "killer": "joint:M1083+M1082",
    "all_paths": "a load()ed manifest reaches the guest or a run only through tree() (materialize, "
                 "materialize_projection); the callers that only load (submit's refuse_links and argv "
                 "entry checks) then materialize through tree(), which re-derives the reference from "
                 "the parsed entries and the bytes read and refuses one that is not r (M1082)"}
EQUIV_RECORD["M1072"] = {
    "property": "a tree whose .git is a symlink is never a clean build tree or a protected lineage",
    "subsumed_by": ["M1085"], "killer": "joint:M1072+M1085",
    "all_paths": "discover() has three callers (provenance's toplevel, descends_from_protected, "
                 "readiness's protected_components), and each calls refuse_config on the top discover "
                 "returned before it reads anything from the repository (provenance.rs `if let Err(e) "
                 "= git_data::refuse_config(&top)`, lineage()'s `refuse_config(&top)?`, readiness's "
                 "`refuse_config(&found)`); refuse_config's symlink_metadata of `.git` is neither a "
                 "directory nor a file for a symlink, so it refuses (M1085)"}
EQUIV_RECORD["M1085"] = {
    "property": "a tree whose .git is a symlink is never a clean build tree or a protected lineage",
    "subsumed_by": ["M1072"], "killer": "joint:M1072+M1085",
    "all_paths": "refuse_config's production callers (provenance, lineage, readiness) all pass the top "
                 "discover() returned, and discover refuses a `.git` that is not a real directory "
                 "(M1072) before it returns"}
EQUIVALENT_DID |= {"M1038", "M1043", "M1081", "M1082", "M1083", "M1072", "M1085"}
RETIRED |= {"M1038", "M1043", "M1081", "M1082", "M1083", "M1072", "M1085"}

# ── C9 round 4b, ROWS4B wave 2 (M1086-M1098; amendment 62, strict reading):
# exemptions that rested on "another check refuses it" become ACTIVE rows
# whose attack reaches the guard alone, or EQUIVALENT_DID retirements with an
# executed four-cell record. Rows guarding the Fabric files above.
_TPL = '--lib'
_TGA = '--test grant_registry_authority'
MUTATIONS += [
    ('M1086', "FIELD-ORIGIN (rows4b): a git that fails is refused, never read as an empty answer (retired vs M451)", _FG,
     '    if !o.status.success() {\n        return Err(format!("git {} failed"',
     '    if false && !o.status.success() {\n        return Err(format!("git {} failed"',
     'axon-fabric', _TPL, 'provenance::tests::a_git_that_fails_is_never_read_as_a_clean_tree'),
    ('M1087', "FIELD-ORIGIN (rows4b): a per-worktree config is refused (retired vs M450)", _FG,
     '    if std::fs::symlink_metadata(&wt).is_ok() {', '    if false && std::fs::symlink_metadata(&wt).is_ok() {',
     'axon-fabric', _TPL, 'provenance::tests::a_filter_driver_in_the_worktree_config_never_runs'),
    ('M1089', "FIELD-ORIGIN (rows4b): check-ignore must account for every ignored path (retired vs M500)", _FP,
     '    if matched < ignored.len() {', '    if false && matched < ignored.len() {',
     'axon-fabric', _TPL, 'provenance::tests::an_ignored_file_check_ignore_cannot_name_is_dirty'),
    ('M1090', "PSV-5 (rows4b): a version naming one path twice never materializes", _FW,
     '        if let Some(w) = entries.windows(2).find(|w| w[0].path == w[1].path) {',
     '        if let Some(w) = entries.windows(2).find(|w| false && w[0].path == w[1].path) {',
     'axon-fabric', '--test workspace', 'a_version_naming_one_path_twice_never_materializes'),
    ('M1091', "PSV-5 (rows4b): publication never accepts a stored file holding other bytes (retired vs M1081+M1082)", _FW,
     '            if std::fs::read(dest)? != bytes {', '            if false && std::fs::read(dest)? != bytes {',
     'axon-fabric', '--test workspace', 'a_blob_planted_before_publication_never_materializes'),
    ('M1092', "D1 (rows4b): status/cancel resolve the grant against the operator registry at decision time", _FC,
     '        .unwrap_or_else(|e| refuse("unauthorized", &e, 7));\n    let op',
     '        .ok();\n    let op',
     'axon-fabric', _TGA, 'a_grant_revoked_after_submission_serves_no_status_or_cancel'),
    ('M1093', "D1 (rows4b): a grant authorizes only the principal it is bound to", 'crates/axon-fabric/src/grants.rs',
     '        if e.principal_ref != principal_ref {', '        if false && e.principal_ref != principal_ref {',
     'axon-fabric', '--test grant_authority', 'an_unknown_or_unbound_or_edited_grant_is_refused_with_zero_effects'),
    ('M1094', "O1 (rows4b): a protected-host config that exists but does not load is never a development host", _FC,
     '    ProtectedHost::operator()\n        .unwrap_or_else(|e| refuse("unregistered", &format!("protected host: {e}"), 4))\n}',
     '    ProtectedHost::operator().unwrap_or(None)\n}',
     'axon-fabric', _TGA, 'an_unloadable_protected_host_config_is_never_a_development_host'),
    ('M1095', "submit (rows4b): an interpreter_run's argv is [program.ax] (retired vs M1054)", _FS,
     '        (JobKind::InterpreterRun, _) => {',
     '        (JobKind::InterpreterRun, [f, ..]) => (f.clone(), None),\n        (JobKind::InterpreterRun, _) => {',
     'axon-fabric', _TS, 'an_interpreter_run_with_an_ignored_argument_never_runs'),
    ('M1096', "submit (rows4b): an operator suite runs only as a registered check (retired vs M1054)", _FS,
     '    if req.job_kind != JobKind::RegisteredCheck {', '    if false && req.job_kind != JobKind::RegisteredCheck {',
     'axon-fabric', _TS, 'an_interpreter_run_never_runs_an_operator_suite'),
    ('M1097', "FIELD-ORIGIN (rows4b): the allowlist chain has no symlink (retired vs M1071)", _FG,
     '        if m.file_type().is_symlink() {', '        if false && m.file_type().is_symlink() {',
     'axon-fabric', _TPL, 'provenance::tests::an_allowlist_reached_through_a_symlink_excuses_nothing'),
    ('M1098', "D1 (rows4b): a grant file is used only at the bytes the registry pins", 'crates/axon-fabric/src/grants.rs',
     '        if found != e.sha256 {', '        if false && found != e.sha256 {',
     'axon-fabric', '--test grant_authority', 'an_unknown_or_unbound_or_edited_grant_is_refused_with_zero_effects'),
]
MUTATIONS += [
    ('M1099', "PSV-6 (rows4b): a local receipt is signed only as development", 'crates/axon-fabric/src/signing.rs',
     '            "development" => Ok(()),\n            _ => Err(WRONG_CLASS),', '            _ => Ok(()),',
     'axon-fabric', '--lib', 'signing::tests::a_class_is_signed_only_where_its_backend_derives_it'),
    ('M1100', "PSV-6 (rows4b): an effectful local check is never signed (the workload could read the key)", 'crates/axon-fabric/src/signing.rs',
     '    } else {\n        Err(KEY_REACHABLE)\n    }', '    } else {\n        Ok(())\n    }',
     'axon-fabric', '--lib', 'signing::tests::an_effectless_local_run_signs_and_an_effectful_one_does_not'),
]
EQUIV_RECORD["M1086"] = {
    "property": "a working tree whose git fails is never described as clean",
    "subsumed_by": ["M451"], "killer": "joint:M1086+M451",
    "all_paths": "provenance_with always runs head_bytes_differ (M451) after dirty_reasons, whatever git "
                 "answered: it hashes the working tree against HEAD's tree read from hash-checked objects, "
                 "so an edit or an untracked file is reported without any git command's answer; every other "
                 "caller of run() (descends, own_repository, object_named) refuses the empty answer a failed "
                 "git would leave (no object is named '', '' is no commit, '' does not canonicalize)"}
EQUIV_RECORD["M1087"] = {
    "property": "no per-worktree git configuration takes effect in a provenance answer",
    "subsumed_by": ["M450"], "killer": "joint:M1087+M450",
    "all_paths": "git reads config.worktree only when the repository config sets "
                 "extensions.worktreeConfig, and refuse_config refuses every key allowed_key does not list, "
                 "extensions.* among them (M450), before any git call that could read it"}
EQUIV_RECORD["M1089"] = {
    "property": "an ignored untracked file is never hidden from build provenance",
    "subsumed_by": ["M500"], "killer": "joint:M1089+M500",
    "all_paths": "provenance_with always runs head_bytes_differ, whose tree walk reports every file not in "
                 "HEAD's tree unless the operator allowlist names it exactly (M500), whatever git's ignore "
                 "machinery or check-ignore answered"}
EQUIV_RECORD["M1091"] = {
    "property": "a published version never materializes bytes other than the ones it names",
    "subsumed_by": ["M1081", "M1082"], "killer": "joint:M1091+M1081+M1082",
    "all_paths": "a stored version reaches a run only through tree() (materialize, "
                 "materialize_projection), which re-verifies every blob against its name (M1081) and "
                 "re-derives the reference from the bytes read (M1082): a blob planted before publication "
                 "fails both"}
EQUIV_RECORD["M1095"] = {
    "property": "an interpreter_run never runs on a backend here",
    "subsumed_by": ["M1054"], "killer": "joint:M1095+M1054",
    "all_paths": "check_target's only caller is submit, after backend::select succeeded; select refuses every "
                 "interpreter_run (the host backend's job kinds, M1054; the protected profile's, M321; the "
                 "Axon-kernel VM is refused outright)"}
EQUIV_RECORD["M1096"] = {
    "property": "an interpreter_run never runs on a backend here",
    "subsumed_by": ["M1054"], "killer": "joint:M1096+M1054",
    "all_paths": "as M1095: check_suite_target is reached only through check_target after select, which "
                 "refuses every interpreter_run"}
EQUIV_RECORD["M1097"] = {
    "property": "the provenance allowlist is read only through a root-owned, unwritable, symlink-free chain",
    "subsumed_by": ["M1071"], "killer": "joint:M1097+M1071",
    "all_paths": "the mode rule runs on the same lstat metadata in the same closure, for every component; "
                 "on Linux a symlink's own mode is always 0777, so the group/other-write rule (M1071) "
                 "refuses every symlink this rule refuses"}
# M1088 (the unreadable-config refusal) is unallocated: its four-cell run showed the joint
# removal still refused (UNREACHABLE, see v022_refusal_coverage.py).
EQUIVALENT_DID |= {"M1086", "M1087", "M1089", "M1091", "M1095", "M1096", "M1097"}
RETIRED |= {"M1086", "M1087", "M1089", "M1091", "M1095", "M1096", "M1097"}
# C9 round 4b fix wave: rows4a M940-M1019 (amendment 61).
PSV_IDS |= {f"M{n}" for n in range(940, 1020)}
# C9 round 4b, integrate (amendment 64): M1200-M1399.
PSV_IDS |= {f"M{n}" for n in range(1200, 1400)}
# C9 round 4b, integrate-2 (amendment 64): M1400-M1449.
PSV_IDS |= {f"M{n}" for n in range(1400, 1450)}
# C9 round 4b, integrate-3 (amendment 64): M1450-M1469.
PSV_IDS |= {f"M{n}" for n in range(1450, 1470)}

# ── C9 round 4b, workstream GAPS (M1470-M1499; amendment 65) ──────────────────
# (Assigned M1450-M1499; M1450-M1469 are integrate-3's, so these use M1470 up.)
# The operator deployment kit (c9r4b/opkit) found code gaps: the setuid
# helper's harden() had no test and no row (each inherited-state reset below
# was removable with the suite green); the axon-custodian PROGRAM was pinned
# nowhere (the helper trusted any program serving its socket as the custodian
# uid); the B263 record stated a constant host; the operator's host-toolchain
# pin had no reader. Each row is killed by its OWN attack through the real
# binary or script: the setuid-root helper driven by a hostile Fabric caller,
# an impostor custodian on the socket (test-trust and production routes),
# b263_host.py, and the freeze run with the operator pin in a private mount
# namespace.
PSV_IDS |= {f"M{n}" for n in range(1470, 1500)}
# C9 round 4b (observer, amendment 68): M1520-M1559.
PSV_IDS |= {f"M{n}" for n in range(1520, 1560)}
_PL = 'crates/axon-fabric/src/privileged_launcher.rs'
_CU = 'crates/axon-fabric/src/custodian.rs'
_GB = 'scripts/guest_build_env.py'
_HARDEN = 'a_callers_process_state_never_reaches_the_root_helper_or_its_launcher'
_FRZ = 'a_guest_image_not_built_with_the_operators_pinned_tools_does_not_freeze'
MUTATIONS += [
    ('M1474', "A/harden (gaps): a caller's ignored signals are reset before the root launch", _PL,
     '                libc::signal(sig, libc::SIG_DFL);', '                let _ = sig;',
     'axon-fabric', '--test privileged_launcher', _HARDEN),
    ('M1475', "A/harden (gaps): a caller's blocked signal mask is cleared in the root helper", _PL,
     '        libc::sigprocmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());', '        let _ = &set;',
     'axon-fabric', '--test privileged_launcher', _HARDEN),
    ('M1476', "A/harden (gaps): the root launcher never runs under its caller's umask", _PL,
     '        libc::umask(0o022);', '        let _ = 0o022;',
     'axon-fabric', '--test privileged_launcher', _HARDEN),
    ('M1477', "A/harden (gaps): the root helper never keeps its caller's working directory", _PL,
     '        libc::chdir(c"/".as_ptr());', '        let _ = c"/";',
     'axon-fabric', '--test privileged_launcher', _HARDEN),
    ('M1478', "A/harden (gaps): no descriptor the caller left open reaches the root helper or launcher", _PL,
     '        libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32);', '        let _ = libc::SYS_close_range;',
     'axon-fabric', '--test privileged_launcher', _HARDEN),
    ('M1479', "A/harden (gaps): the root launch cannot dump core (RLIMIT_CORE 0)", _PL,
     '        lim(libc::RLIMIT_CORE, 0);', '        let _ = libc::RLIMIT_CORE;',
     'axon-fabric', '--test privileged_launcher', _HARDEN),
    ('M1480', "A/harden (gaps): limits a caller lowered (CPU, FSIZE, DATA, AS, NPROC) are reset", _PL,
     '            lim(r, libc::RLIM_INFINITY);', '            let _ = r;',
     'axon-fabric', '--test privileged_launcher', _HARDEN),
    ('M1481', "A/harden (gaps): the root helper ignores SIGPIPE whatever its caller left", _PL,
     '        libc::signal(libc::SIGPIPE, libc::SIG_IGN);', '        let _ = libc::SIGPIPE;',
     'axon-fabric', '--test privileged_launcher', _HARDEN),
    ('M1482', "A/harden (gaps): the open-file limit is the helper's, not its caller's", _PL,
     '        lim(libc::RLIMIT_NOFILE, 65536);', '        let _ = libc::RLIMIT_NOFILE;',
     'axon-fabric', '--test privileged_launcher', _HARDEN),
    ('M1483', "FIELD-ORIGIN (gaps): a nonce is spent only through the custodian PROGRAM the operator pinned", _CU,
     '    if got != pin {', '    if false && got != pin {',
     'axon-fabric', '--test privileged_launcher', 'a_custodian_program_the_operator_never_pinned_spends_nothing'),
    ('M1484', "FIELD-ORIGIN (gaps): a custodian executable another uid can rewrite is never the pinned program", _CU,
     '    if !m.is_file() || (m.uid() != 0 && m.uid() != me) || m.mode() & 0o022 != 0 {',
     '    if !m.is_file() || (m.uid() != 0 && m.uid() != me) {',
     'axon-fabric', '--test privileged_launcher', 'a_custodian_executable_another_uid_can_rewrite_is_refused'),
    ('M1485', "FIELD-ORIGIN (gaps): a production helper config must pin the custodian program", _PL,
     '        None if a.test => {}\n        _ => {', '        None => {}\n        _ => {',
     'axon-fabric', '--test privileged_launcher', 'a_production_helper_spends_only_through_the_pinned_custodian_program'),
    ('M1486', "FIELD-ORIGIN (gaps): the B263 record states the host it was measured on", 'scripts/b263_host.py',
     '    host = f"{label} ({measured})" if label else measured', '    host = "WSL2-nested"',
     'axon-fabric', '--test qualification', 'the_b263_record_states_the_host_it_ran_on'),
    ('M1470', "FIELD-ORIGIN (gaps): a freeze binds only host tools at the operator's pinned path and digest", _GB,
     '        if g is None or g.get("path") != w.get("path") or not g.get("sha256") or g.get("sha256") != w.get("sha256"):',
     '        if g is None:',
     'axon-fabric', '--test freeze_manifest', _FRZ),
    ('M1471', "FIELD-ORIGIN (gaps): a freeze refuses a recorded host tool the operator's pin does not name", _GB,
     '            return f"the build recorded host tool {name} {g}, which the operator\'s pin does not name"',
     '            continue',
     'axon-fabric', '--test freeze_manifest', _FRZ),
    ('M1472', "FIELD-ORIGIN (gaps): a freeze requires the operator's host-toolchain pin", _GB,
     '                "a freeze binds only a build made with the operator\'s pinned tools)") if required else ""',
     '                "a freeze binds only a build made with the operator\'s pinned tools)") if False else ""',
     'axon-fabric', '--test freeze_manifest', _FRZ),
    ('M1473', "FIELD-ORIGIN (gaps): the host-toolchain pin is read only as a root-owned, unwritable file", _GB,
     '        if st.st_uid != 0 or st.st_mode & 0o022:\n            return (f"{cur} (uid', '        if False:\n            return (f"{cur} (uid',
     'axon-fabric', '--test freeze_manifest', _FRZ),
]

# ── C9 round 4b, workstream FINAL (M1487; amendment 66) ──────────────────────
# Amendment 65 exempted harden()'s ENVIRONMENT clear as dominated by
# sealed_exec::command's explicit envp (M228). Under the strict ruling it is
# not: the helper's OWN Rust runtime reads RUST_BACKTRACE when it panics, and
# the caller can make `--probe` panic (println! to a report pipe whose read end
# is closed; SIGPIPE is ignored, M1481). Measured with the clear removed: the
# setuid-root helper printed its full stack, every frame's address (binary and
# libc), to the Fabric-uid caller's stderr. sealed_exec is not on that route,
# so this row is killed by its OWN attack with the clear its only guard.
# PR_SET_DUMPABLE stays a measured exemption (amendment 65).
MUTATIONS += [
    ('M1487', "A/harden (final): the caller's environment never reaches the root helper's own runtime (RUST_BACKTRACE: no address layout to the caller)", _PL,
     '        std::env::remove_var(k);', '        let _ = k;',
     'axon-fabric', '--test privileged_launcher', 'the_root_helpers_address_layout_never_reaches_its_caller'),
    # The gaps workstream ran the freeze in a private mount namespace by taking
    # the helper's command APART (get_program/get_args) and b263_host.py with a
    # bare python3: axon-core's workspace drift gate failed at d39ab3ad (found
    # by the final paired-disable's clean baseline). The helper now owns the
    # wrapped form (script_under); this row: the wrapped form is the checked
    # command, never a rebuilt one.
    ('M1488', "EQUIVALENCE (6, final): a script run under a wrapper (script_under) is checked and stripped by the helper", 'crates/axon-core/tests/script_spawn/mod.rs',
     '    let inner = script(interpreter, path, bins);\n',
     '    let inner = {\n        let _ = bins;\n        let mut c = Command::new(interpreter);\n        c.arg(path.as_ref());\n        c\n    };\n',
     'axon-core', '--no-default-features --test harness_binaries', 'a_wrapped_script_is_checked_and_stripped_like_any_other'),
    # The custodian-program verification AS A WHOLE (M1483 mutates only its
    # comparison): every reply's sender is identified and its executable
    # opened and hashed. Killed by the impostor attack; also a member of
    # M602's guard set (a non-root helper cannot open the custodian's exe).
    ('M1489', "FIELD-ORIGIN (final): every custodian reply's sender is verified against the program pin at all", _CU,
     '    let pid = pidfd_pid(pidfd).ok_or("its sender has exited")?;\n',
     '    let pid = pidfd_pid(pidfd).ok_or("its sender has exited")?;\n    if true {\n        let _ = (pin, seen);\n        return Ok(pid);\n    }\n',
     'axon-fabric', '--test privileged_launcher', 'a_custodian_program_the_operator_never_pinned_spends_nothing'),
]

# ── C9 round 4c, workstream HARDEN (M1600-M1629; amendment 73; matrix A110-A117):
# harden() resets every process attribute a set-id exec preserves (credentials(7),
# execve(2), prctl(2)). Each row removes ONE reset and is killed by its own attack
# (tests/privileged_launcher.rs: the state armed by a non-root caller, a witness
# proving it survived an ordinary exec, a control launch, the setuid helper).
PSV_IDS |= {f"M{n}" for n in range(1600, 1630)}
_H_TIMERS = 'a_callers_interval_timers_never_signal_the_root_helper'
_H_LIMITS = 'a_callers_other_resource_limits_never_reach_the_root_launch'
_H_SCHED = 'a_callers_scheduling_state_never_reaches_the_root_launch'
_H_KERNEL = 'a_callers_oom_slack_and_subreaper_state_never_reaches_the_root_launch'
_H_SETSID = 'a_terminal_its_caller_owns_never_signals_the_root_helper'
_H_LEADER = 'a_helper_that_could_not_leave_its_callers_session_launches_nothing'
_H_PERSONA = 'a_callers_personality_never_reaches_the_root_launch'
def _hrow(mid, what, old, new, test):
    return (mid, "A/harden (c4c): " + what, _PL, old, new, 'axon-fabric', '--test privileged_launcher', test)
MUTATIONS += [
    _hrow('M1600', "a caller's ITIMER_REAL is disarmed before the root helper runs",
          '        libc::setitimer(libc::ITIMER_REAL, &off, std::ptr::null_mut());',
          '        let _ = libc::ITIMER_REAL;', _H_TIMERS),
    _hrow('M1601', "a caller's ITIMER_VIRTUAL is disarmed",
          '        libc::setitimer(libc::ITIMER_VIRTUAL, &off, std::ptr::null_mut());',
          '        let _ = libc::ITIMER_VIRTUAL;', _H_TIMERS),
    _hrow('M1602', "a caller's ITIMER_PROF is disarmed",
          '        libc::setitimer(libc::ITIMER_PROF, &off, std::ptr::null_mut());',
          '        let _ = libc::ITIMER_PROF;', _H_TIMERS),
    _hrow('M1603', "the helper leaves its caller's session (setsid): no terminal signals it",
          '        if libc::setsid() < 0 {', '        if false {', _H_SETSID),
    _hrow('M1604', "a helper that could not leave its caller's session launches nothing",
          '    if SESSION_NOT_LEFT.load(std::sync::atomic::Ordering::SeqCst) {', '    if false {', _H_LEADER),
    _hrow('M1605', "RLIMIT_STACK is the helper's",
          '        lim2(libc::RLIMIT_STACK, 8 << 20, libc::RLIM_INFINITY);',
          '        let _ = libc::RLIMIT_STACK;', _H_LIMITS),
    _hrow('M1606', "RLIMIT_RSS is the helper's",
          '        lim2(libc::RLIMIT_RSS, libc::RLIM_INFINITY, libc::RLIM_INFINITY);',
          '        let _ = libc::RLIMIT_RSS;', _H_LIMITS),
    _hrow('M1607', "RLIMIT_MEMLOCK is the helper's",
          '        lim2(libc::RLIMIT_MEMLOCK, 8 << 20, 8 << 20);',
          '        let _ = libc::RLIMIT_MEMLOCK;', _H_LIMITS),
    _hrow('M1608', "RLIMIT_LOCKS is the helper's",
          '        lim2(libc::RLIMIT_LOCKS, libc::RLIM_INFINITY, libc::RLIM_INFINITY);',
          '        let _ = libc::RLIMIT_LOCKS;', _H_LIMITS),
    _hrow('M1609', "RLIMIT_SIGPENDING is the helper's",
          '        lim2(\n            libc::RLIMIT_SIGPENDING,\n            libc::RLIM_INFINITY,\n            libc::RLIM_INFINITY,\n        );',
          '        let _ = libc::RLIMIT_SIGPENDING;', _H_LIMITS),
    _hrow('M1610', "RLIMIT_MSGQUEUE is the helper's",
          '        lim2(libc::RLIMIT_MSGQUEUE, 819200, 819200);',
          '        let _ = libc::RLIMIT_MSGQUEUE;', _H_LIMITS),
    _hrow('M1611', "RLIMIT_NICE is the helper's",
          '        lim2(libc::RLIMIT_NICE, 0, 0);', '        let _ = libc::RLIMIT_NICE;', _H_LIMITS),
    _hrow('M1612', "RLIMIT_RTPRIO is the helper's",
          '        lim2(libc::RLIMIT_RTPRIO, 0, 0);', '        let _ = libc::RLIMIT_RTPRIO;', _H_LIMITS),
    _hrow('M1613', "RLIMIT_RTTIME is the helper's",
          '        lim2(\n            libc::RLIMIT_RTTIME,\n            libc::RLIM_INFINITY,\n            libc::RLIM_INFINITY,\n        );',
          '        let _ = libc::RLIMIT_RTTIME;', _H_LIMITS),
    _hrow('M1614', "the nice value is the kernel default, not the caller's",
          '        libc::setpriority(libc::PRIO_PROCESS, 0, 0);', '        let _ = libc::PRIO_PROCESS;', _H_SCHED),
    _hrow('M1615', "the I/O scheduling class is the default, not the caller's",
          '        libc::syscall(\n            libc::SYS_ioprio_set,\n            1 as libc::c_long,\n            0 as libc::c_long,\n            0 as libc::c_long,\n        );',
          '        let _ = libc::SYS_ioprio_set;', _H_SCHED),
    _hrow('M1616', "the scheduling policy is SCHED_OTHER, not the caller's",
          '        libc::sched_setscheduler(0, libc::SCHED_OTHER, &sp);', '        let _ = &sp;', _H_SCHED),
    _hrow('M1617', "the CPU affinity is every CPU, not the caller's pinning",
          '        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &all);',
          '        let _ = &all;', _H_SCHED),
    _hrow('M1618', "oom_score_adj is the default, not the caller's",
          '        let _ = std::fs::write("/proc/self/oom_score_adj", "0");', '        let _ = "0";', _H_KERNEL),
    _hrow('M1619', "the timer slack is the default value, not the caller's",
          '        libc::prctl(\n            PR_SET_TIMERSLACK,\n            50_000 as libc::c_ulong,\n            0 as libc::c_ulong,\n            0 as libc::c_ulong,\n            0 as libc::c_ulong,\n        );',
          '        let _ = PR_SET_TIMERSLACK;', _H_KERNEL),
    _hrow('M1620', "the personality is PER_LINUX, not the caller's",
          '        libc::personality(0);', '        let _ = 0;', _H_PERSONA),
    _hrow('M1622', "the helper is not its caller's child subreaper",
          '        libc::prctl(\n            libc::PR_SET_CHILD_SUBREAPER,\n            0 as libc::c_ulong,\n            0 as libc::c_ulong,\n            0 as libc::c_ulong,\n            0 as libc::c_ulong,\n        );',
          '        let _ = libc::PR_SET_CHILD_SUBREAPER;', _H_KERNEL),
]

# ── C9 round 4b, OBSERVER workstream (M1520-M1559; amendment 68; matrix A94):
# operator decisions G and G1 = A. The observer is its own-uid SERVICE
# (`axon-observer`, observer_service.rs); Fabric obtains an observation only
# through the setuid-root helper's `--observe` relay, which authenticates the
# observer PROGRAM per reply message and measures the running Fabric. Each row
# is killed by its own attack (tests/observer_service.rs unless named).
_OS = 'crates/axon-fabric/src/observer_service.rs'
_OB = 'crates/axon-fabric/src/bin/axon-observer.rs'
_TO = '--test observer_service'
_TOM = 'the_observer_signs_nothing_it_did_not_measure'
MUTATIONS += [
    ('M1520', "A94 (observer): a protected observer config is three principals (observer != Fabric, neither root)", _OS,
     '        if self.observer_uid == self.fabric_uid || self.observer_uid == 0 || self.fabric_uid == 0 {',
     '        if false {',
     'axon-fabric', '--lib', 'observer_service::tests::an_observer_that_is_the_fabric_is_refused'),
    ('M1521', "A94 (observer): a protected observer observes only for uid 0 (the root helper's relay)", _OS,
     '        if self.caller_uid != 0 {', '        if false && self.caller_uid != 0 {',
     'axon-fabric', '--lib', 'observer_service::tests::a_protected_observer_observes_only_for_the_root_helper'),
    ('M1522', "A94 (observer): the observer key is readable by no other uid (mode 0400; an ACL's mask is the group bits)", _OS,
     '    if !meta.file_type().is_file() || meta.uid() != euid || meta.mode() & 0o277 != 0 {',
     '    if !meta.file_type().is_file() || meta.uid() != euid {',
     'axon-fabric', _TO, 'an_observer_key_another_uid_can_read_is_refused'),
    ('M1523', "A94 (observer): the observer key is owned by the observer uid", _OS,
     '    if !meta.file_type().is_file() || meta.uid() != euid || meta.mode() & 0o277 != 0 {',
     '    if !meta.file_type().is_file() || meta.mode() & 0o277 != 0 {',
     'axon-fabric', _TO, 'an_observer_key_owned_by_another_uid_is_refused'),
    ('M1524', "A94 (observer): the observer key is one the operator observer root trusts", _OS,
     '    if !keys.iter().any(|k| k == public) {', '    if false && !keys.iter().any(|k| k == public) {',
     'axon-fabric', _TO, 'an_observer_key_the_observer_root_does_not_hold_is_refused'),
    ('M1525', "A94 (observer): the observer serves only from its own private record store", _OB,
     '    cu::check_store(&cfg.store, euid()).unwrap_or_else(|e| die(&e));',
     '    let _ = cu::check_store(&cfg.store, euid());',
     'axon-fabric', _TO, 'an_observer_refuses_a_store_others_can_reach'),
    ('M1526', "A94 (observer): the observer runs only as its configured uid", _OB,
     '    if euid() != cfg.observer_uid {', '    if false && euid() != cfg.observer_uid {',
     'axon-fabric', _TO, 'an_observer_runs_only_as_its_configured_uid'),
    ('M1527', "A94 (observer): the observer observes only for its configured caller uid (SO_PEERCRED)", _OS,
     '        if peer != self.cfg.caller_uid {', '        if false && peer != self.cfg.caller_uid {',
     'axon-fabric', _TO, 'an_observer_observes_only_for_its_caller_uid'),
    ('M1528', "A94 (observer): the observer's request is one fixed schema", _OS,
     '        if r.schema != REQUEST_SCHEMA {', '        if false && r.schema != REQUEST_SCHEMA {',
     'axon-fabric', _TO, 'an_observer_request_of_another_schema_is_refused'),
    ('M1529', "A94 (observer): the observer signs only a canonical protected launch manifest", _OS,
     '        let m = axon_psv::LaunchManifest::verify(bytes, &digest)\n            .map_err(|e| format!("not a protected launch manifest: {e}"))?;',
     '        let m: axon_psv::LaunchManifest =\n            serde_json::from_slice(bytes).map_err(|e| format!("not a launch manifest: {e}"))?;',
     'axon-fabric', _TO, 'an_observer_never_observes_a_manifest_that_is_not_a_protected_launch'),
    ('M1530', "A94 (observer): the nonce naming the observer's record is a custodian nonce, never a path", _OS,
     '        if !is_hex(&m.observation_nonce, 32) {', '        if false && !is_hex(&m.observation_nonce, 32) {',
     'axon-fabric', _TO, 'an_observer_never_records_a_nonce_that_names_a_path'),
    # The nine MEASURED rows: each replaces one measurement by the manifest's
    # own claim (what a signing oracle does); the field-named attack kills it.
    ('M1531', "A94 (observer): host_config_sha256 is measured, not told", _OS,
     '                axon_psv::sha256_hex(&op.host),\n', '                m.host_config_sha256.clone(),\n',
     'axon-fabric', _TO, _TOM),
    ('M1532', "A94 (observer): launcher_sha256 is measured, not told", _OS,
     '                file(hv, "/launcher/path")?,\n', '                m.launcher_sha256.clone(),\n',
     'axon-fabric', _TO, _TOM),
    ('M1533', "A94 (observer): firecracker_sha256 is measured, not told", _OS,
     '                file(lv, "/firecracker")?,\n', '                m.firecracker_sha256.clone(),\n',
     'axon-fabric', _TO, _TOM),
    ('M1534', "A94 (observer): guest.kernel_sha256 is measured, not told", _OS,
     '                digest_of(&artifacts.join("vmlinux"))?,\n', '                m.guest.kernel_sha256.clone(),\n',
     'axon-fabric', _TO, _TOM),
    ('M1535', "A94 (observer): guest.rootfs_sha256 is measured, not told", _OS,
     '                digest_of(&artifacts.join("rootfs.sqfs"))?,\n', '                m.guest.rootfs_sha256.clone(),\n',
     'axon-fabric', _TO, _TOM),
    ('M1536', "A94 (observer): suite.registry_sha256 is measured, not told", _OS,
     '                file(hv, "/suite_registry/path")?,\n', '                m.suite.registry_sha256.clone(),\n',
     'axon-fabric', _TO, _TOM),
    ('M1537', "A94 (observer): qualification_sha256 is measured, not told", _OS,
     '                file(hv, "/qualification/record")?,\n', '                m.qualification_sha256.clone(),\n',
     'axon-fabric', _TO, _TOM),
    ('M1538', "A94 (observer): profile_manifest_sha256 is measured, not told", _OS,
     '                pm_digest,\n', '                m.profile_manifest_sha256.clone(),\n',
     'axon-fabric', _TO, _TOM),
    ('M1539', "A94/A131 (observer): verifier_sha256 is the Fabric program the operator pinned, which the root helper measured the running Fabric to be (amendment 79), not told", _OS,
     '            ("verifier_sha256", pinned.clone(), &m.verifier_sha256),\n', '            ("verifier_sha256", m.verifier_sha256.clone(), &m.verifier_sha256),\n',
     'axon-fabric', _TO, _TOM),
    ('M1540', "A94 (observer): one observation per nonce (the record is created, never replaced)", _OS,
     '            .create_new(true)\n', '            .create(true)\n            .truncate(true)\n',
     'axon-fabric', _TO, 'the_observer_service_observes_through_the_helper_once_per_nonce'),
    ('M1541', "A94 (observer): the helper relays only from a socket the observer uid (or root's activation) serves", _OS,
     '        if peer != self.uid && peer != 0 {', '        if false && peer != self.uid && peer != 0 {',
     'axon-fabric', _TO, 'an_observer_socket_another_uid_serves_is_never_relayed'),
    ('M1542', "A94 (observer): every relayed reply's sender is the observer PROGRAM the operator pinned", _OS,
     '        let text = crate::custodian::read_from_pinned(\n            &s,\n            &self.sha256,\n            &self.socket,\n            "observer",\n            MAX_REPLY,\n        )?;',
     '        let text = {\n            let mut t = Vec::new();\n            (&s).take(MAX_REPLY)\n                .read_to_end(&mut t)\n                .map_err(|e| e.to_string())?;\n            t\n        };',
     'axon-fabric', _TO, 'an_observer_program_the_operator_never_pinned_is_never_relayed'),
    ('M1543', "A94/D6 (observer): only a protected observer's observation is relayed (a test one by a test-trust helper)", _PL,
     '    if !custodian_mode_launches(got.mode, a.test) {', '    if false && !custodian_mode_launches(got.mode, a.test) {',
     'axon-fabric', _TO, 'a_dev_observer_is_never_relayed'),
    ('M1544', "A94 (observer): the helper measures the RUNNING Fabric: its parent (by pidfd) runs as the Fabric uid", _PL,
     '    if uids.len() != 4 || uids.iter().any(|u| *u != fabric_uid) {', '    if false {',
     'axon-fabric', _TO, 'an_observe_relay_whose_parent_is_not_the_fabric_relays_nothing'),
    ('M1545', "A94 (observer): the helper's observe request is one fixed schema", _PL,
     '    if r.schema != OBSERVE_REQUEST_SCHEMA {', '    if false && r.schema != OBSERVE_REQUEST_SCHEMA {',
     'axon-fabric', _TO, 'an_observe_request_of_another_schema_relays_nothing'),
    ('M1546', "A94 (observer): a production helper config's observer service is neither the Fabric uid nor root", _PL,
     '        if !a.test && (s.uid == c.fabric_uid || s.uid == 0) {', '        if false {',
     'axon-fabric', '--lib', 'privileged_launcher::tests::a_helper_config_whose_observer_service_is_the_fabric_is_refused'),
    ('M1547', "A94 (observer): a production Fabric refuses a host config naming an in-uid observer program", _PH,
     '                    Some(_) if crate::backend::TEST_TRUST_BUILD => {',
     '                    Some(_) if true => {',
     'axon-fabric', '--test privileged_launcher', 'a_production_fabric_refuses_an_observer_program_on_a_protected_host'),
    ('M1548', "A94 (observer): a production observer never takes its config from a caller-named path", _OB,
     '        ["--test-config", _] if axon_fabric::backend::TEST_TRUST_BUILD => {',
     '        ["--test-config", _] if true => {',
     'axon-fabric', '--test privileged_launcher', 'a_production_observer_never_takes_its_config_from_a_path_its_caller_names'),
    # The comparison every MEASURED row (M1531-M1539) feeds: removed, the
    # observer signs whatever the manifest claims (a signing oracle).
    ('M1549', "A94 (observer): every measured digest must EQUAL the manifest's claim before anything is signed", _OS,
     '            if got != claimed {', '            if false && got != claimed {',
     'axon-fabric', _TO, _TOM),
]


# ── C9 round 4b, workstream SMALLFIX (M1580-M1599; amendment 70) ─────────────
# The operator-kit review: (1) the B263 record's x3 reason, inside the bytes
# the operator SIGNS, was a constant asserting WSL2/Hyper-V of every host; it
# is now derived from the measured host (b263_host.x3_reason). (2) A loop-store
# public key in uppercase passed the operator-root lookup (which lowercases)
# and failed every verification (which reads lowercase only); the store now
# refuses any key not in the canonical 64-lowercase-hex form where it reads
# its keys (Config::check_separation, on write and on every read).
PSV_IDS |= {f"M{n}" for n in range(1580, 1600)}
_X3 = 'the_x3_reason_states_only_what_the_host_measured'
MUTATIONS += [
    ('M1580', "FIELD-ORIGIN (smallfix): the signed x3 reason is the one b263_host.py derived from the measured host", 'scripts/b263_qualify.sh',
     'record x3_l0_hypervisor_boundary BLOCKED "$X3_REASON" "G03-r22-physical-isolation"',
     'record x3_l0_hypervisor_boundary BLOCKED "Host is WSL2 with nested KVM under Hyper-V (operator decision D2). The L0 hypervisor and the WSL2 utility VM are outside the qualified boundary; no assertion here covers a guest escape through L0/L1." "G03-r22-physical-isolation"',
     'axon-fabric', '--test qualification', _X3),
    ('M1581', "FIELD-ORIGIN (smallfix): the x3 reason makes no WSL2/Hyper-V claim of a host not measured as WSL", 'scripts/b263_host.py',
     '    if facts.get("wsl"):', '    if True:',
     'axon-fabric', '--test qualification', _X3),
    ('M1582', "ADR-002 (smallfix): a store public key is refused unless in its canonical form (64 lowercase hex)", 'crates/axon-loop/src/store.rs',
     '                if !axon_loop_contracts::attestation::is_canonical_public_key_hex(k) {',
     '                if false && !axon_loop_contracts::attestation::is_canonical_public_key_hex(k) {',
     'axon-loop', '--test evl_admission', 'an_uppercase_public_key_is_refused_where_the_store_reads_it'),
]

# ── C9 round 4b, workstream PDFAST (M1500-M1519; amendment 67) ──────────────
# Paired-disable selects the consumers of a record's full-suite cell from the
# build graph and the tree's text (scripts/v022_pd_consumers.py), and the
# record names every consumer it ran and skipped. --join and the currency rule
# accept a record only with that selection, and only when it is the rule's at
# this commit: a consumer the graph reaches is never skipped.
_PDC = 'scripts/v022_pd_consumers.py'
_PDH = 'scripts/v022_paired_disable.py'
MUTATIONS += [
    ('M1500', 'EQUIVALENCE (pdfast): --join refuses a record that does not carry the rule\'s consumer selection', _PDH,
     "            why = selection_problem(r)\n            if why:\n                sys.exit(f\"refused: shard {k}/{n} record {r['mutation']}: {why}\")\n",
     "            why = selection_problem(r)\n            if False:\n                sys.exit(f\"refused: shard {k}/{n} record {r['mutation']}: {why}\")\n",
     'axon-core', _HI2, 'a_join_refuses_a_record_without_a_consumer_selection'),
    ('M1501', 'EQUIVALENCE (pdfast): a skipped consumer must carry its graph reason', _PDC,
     '        if not isinstance(v, str) or not v:\n            return f"skipped consumer {c} has no graph reason"\n',
     '        if False:\n            return f"skipped consumer {c} has no graph reason"\n',
     'axon-core', _HI2, 'a_join_refuses_a_skipped_consumer_without_a_reason'),
    ('M1502', 'EQUIVALENCE (pdfast): a consumer the build graph reaches is never skipped', _PDH,
     '        if got is None:\n            return (f"consumer {c} is reachable through the graph "\n',
     '        if False:\n            return (f"consumer {c} is reachable through the graph "\n',
     'axon-core', _HI2, 'a_join_refuses_a_record_that_skips_a_reachable_consumer'),
    ('M1503', 'EQUIVALENCE (pdfast): a passing full-suite cell ran exactly the consumers it selected', _PDH,
     '        if ran != set(sel["run"]):\n',
     '        if False:\n',
     'axon-core', _HI2, 'a_join_refuses_a_passing_cell_that_ran_none_of_its_consumers'),
    ('M1504', 'EQUIVALENCE (pdfast, currency): a kept record is stale once its consumer selection is not the rule\'s', _PDH,
     '    why = selection_problem(record)\n    if why:\n        out.append(',
     '    why = selection_problem(record)\n    if False:\n        out.append(',
     'axon-core', _HI2, 'a_kept_record_is_stale_once_a_new_consumer_reaches_it'),
    ('M1505', 'EQUIVALENCE (pdfast): --join refuses records run on different toolchains', _PDH,
     '    why = mut.shard_toolchain_problem([\n        (r["mutation"], (r.get("environment") or {}).get("host"), None) for r in records])\n    if why:\n        sys.exit("refused: " + why)\n    for r in records:\n        h = ',
     '    why = mut.shard_toolchain_problem([\n        (r["mutation"], (r.get("environment") or {}).get("host"), None) for r in records])\n    if False:\n        sys.exit("refused: " + why)\n    for r in records:\n        h = ',
     'axon-core', _HI2, 'a_join_refuses_records_from_two_toolchains'),
]
PSV_IDS |= {f"M{n}" for n in range(1500, 1520)}

# ── C9 round 4c, r4c-fixes part 1 (M1655-M1659; amendment 71) ───────────────
# SENTINEL MINOR: submit's RunDir is created NEW (create_dir, C9 round 4b),
# named <op16>-<pid>-<seq>. After a crash, a restarted Fabric with the same
# pid (PID 1 in a container) met its predecessor's leftover at the same name
# and refused every retry of the operation. The name now carries 64 random
# bits; create_dir (the refusal of an existing dir) is unchanged. Killed by
# the retry run over every leftover name the crashed process could have left.
PSV_IDS |= {f"M{n}" for n in range(1655, 1660)}
MUTATIONS += [
    ('M1655', "availability (r4c-fixes): a run dir name never meets a crashed same-pid predecessor's leftover",
     'crates/axon-fabric/src/submit.rs',
     '        let suffix = format!(\n            "-{}",\n            rnd.iter().map(|b| format!("{b:02x}")).collect::<String>()\n        );\n',
     '        let suffix = {\n            let _ = rnd;\n            String::new()\n        };\n',
     'axon-fabric', '--test check_effects',
     'a_leftover_run_dir_of_a_crashed_process_with_the_same_pid_does_not_refuse_the_retry'),
]

# ── C9 round 4c, r4c-fixes part 2 (amendment 71) ────────────────────────────
# Round 4c (SENTINEL) found decision code on the protected path the refusal
# gate could not see: evo.rs OUT_OF_SCOPE with a reason the code contradicts
# (evo::propose's role refusal is the ONLY reader of the discovery evidence's
# corpus role), the guest's PID-1 supervisor, the workspace recipe, and refusal
# forms the SITE pattern missed. The gate's crate set is now a rule (amendment
# 71) and these rows are the guards it brought in that carry the property.
# Each is killed by its OWN attack on the production route: evo::propose (the
# `evo propose` verb's function); the real axon-guest-init binary under a
# replaced /proc/cmdline (root); the store's import_dir / materialize; and the
# real gate over a scratch tree.
PSV_IDS |= {f"M{n}" for n in range(1630, 1655)}
_EVO = 'crates/axon-loop/src/evo.rs'
_GI = 'crates/axon-guest-init/src/main.rs'
_WR = 'crates/axon-workspace-recipe/src/lib.rs'
_RCG = 'scripts/v022_refusal_coverage.py'
_RCT = '--no-default-features --test refusal_coverage_gate'
_GIT = 'an_untrustworthy_cmdline_policy_starts_no_workload'
_GIS = 'a_seccomp_filter_that_does_not_apply_starts_no_workload'
MUTATIONS += [
    ('M1630', "B281 (r4c part 2): a Confirmation/Reporting-role episode never reaches the EVO proposer", _EVO,
     '        if matches!(\n            ep.corpus_role,\n            CorpusRole::Confirmation | CorpusRole::Reporting\n        ) {',
     '        if false && matches!(\n            ep.corpus_role,\n            CorpusRole::Confirmation | CorpusRole::Reporting\n        ) {',
     'axon-loop', '--test evo_b281', 'a_protected_role_episode_never_reaches_the_proposer'),
    ('M1631', "B281 (r4c part 2): an episode of another scope or candidate view never feeds the proposer", _EVO,
     '        if ep.scope != scope || ep.candidate_set_ref != incumbent.candidate_set_ref {',
     '        if false && (ep.scope != scope || ep.candidate_set_ref != incumbent.candidate_set_ref) {',
     'axon-loop', '--test evo_b281', 'an_episode_of_another_scope_never_reaches_the_proposer'),
    ('M1632', "B281 (r4c part 2): EVO proposes only from learning-eligible discovery evidence", _EVO,
     '    if evidence.is_empty() {', '    if false && evidence.is_empty() {',
     'axon-loop', '--test evo_b281', 'a_proposal_needs_eligible_discovery_evidence'),
    ('M1633', "COVERAGE GATE (r4c part 2): a crate a protected crate links is in scope (dependency closure)", _RCG,
     '        todo.extend(_normal_path_deps(dirs[n], dirs, default_features))', '        todo.extend(())',
     'axon-core', _RCT, 'a_crate_a_protected_crate_links_is_scanned'),
    ('M1634', "COVERAGE GATE (r4c part 2): a package the guest image builds is in scope", _RCG,
     '    return set(re.findall(r"\\s-p\\s+([A-Za-z0-9_-]+)", open(p).read()))', '    return {"axon-guest-init", "axon-psv", "axon-core", "axon-guest-kernel"}',
     'axon-core', _RCT, 'a_package_the_guest_image_builds_is_scanned'),
    ('M1635', "COVERAGE GATE (r4c part 2): a non-zero process exit and a compile refusal are refusal sites", _RCG,
     '    if SITE.search(l) or DIAG.search(l) or EXIT.search(l):', '    if SITE.search(l):',
     'axon-core', _RCT, 'an_exit_or_a_compile_refusal_is_a_site'),
    ('M1636', "COVERAGE GATE (r4c part 2): a call of a file-local refusal constructor is a site", _RCG,
     '    ctors = local_ctors(lines)\n', '    ctors = set()\n',
     'axon-core', _RCT, 'a_call_of_a_local_refusal_constructor_is_a_site'),
    ('M1637', "GUEST PID 1 (r4c part 2): a refused cmdline policy starts no workload", _GI,
     '            process::exit(1);\n        }\n    };\n\n    // 3. Fork.', '            None\n        }\n    };\n\n    // 3. Fork.',
     'axon-guest-init', '--test policy_refusals', _GIT),
    ('M1638', "GUEST PID 1 (r4c part 2): a cmdline that may have been truncated is not a policy", _GI,
     '    if cmdline.len() > CMDLINE_MAX_SAFE {', '    if false && cmdline.len() > CMDLINE_MAX_SAFE {',
     'axon-guest-init', '--test policy_refusals', _GIT),
    ('M1639', "GUEST PID 1 (r4c part 2): two policy words are ambiguous, neither is used", _GI,
     '    if values.next().is_some() {', '    if false && values.next().is_some() {',
     'axon-guest-init', '--test policy_refusals', _GIT),
    ('M1640', "GUEST PID 1 (r4c part 2): a policy that constrains nothing is not a policy", _GI,
     '    if !payload.constrains_anything() {', '    if false && !payload.constrains_anything() {',
     'axon-guest-init', '--test policy_refusals', _GIT),
    ('M1641', "GUEST PID 1 (r4c part 2): a cmdline policy of another schema is refused", _GI,
     '    if payload.schema.as_deref() != Some(POLICY_SCHEMA) {', '    if false && payload.schema.as_deref() != Some(POLICY_SCHEMA) {',
     'axon-guest-init', '--test policy_refusals', _GIT),
    ('M1642', "GUEST PID 1 (r4c part 2): a policy with a repeated key is refused (no last-wins)", _GI,
     '                    if out.contains_key(&k) {', '                    if false && out.contains_key(&k) {',
     'axon-guest-init', '--test policy_refusals', _GIT),
    ('M1643', "GUEST PID 1 (r4c part 2): a seccomp filter that fails to apply starts no workload", _GI,
     '                eprintln!("[axon-guest-init] seccomp apply failed: {e}");\n                process::exit(1);',
     '                eprintln!("[axon-guest-init] seccomp apply failed: {e}");',
     'axon-guest-init', '--test policy_refusals', _GIS),
    ('M1644', "GUEST PID 1 (r4c part 2): a seccomp program that is not whole instructions is never installed in part", _GI,
     '    if bpf_bytes.len() % 8 != 0 {', '    if false && bpf_bytes.len() % 8 != 0 {',
     'axon-guest-init', '--test policy_refusals', _GIS),
    ('M1645', "GUEST PID 1 (r4c part 2): a seccomp filter the kernel rejects is a refusal, not a skipped filter", _GI,
     '            0usize,\n        );\n        if r != 0 {', '            0usize,\n        );\n        if false && r != 0 {',
     'axon-guest-init', '--test policy_refusals', _GIS),
    ('M1646', "RECIPE (r4c part 2): a path that climbs out of the tree is refused (materialize)", _WR,
     '        .any(|c| c.is_empty() || c == "." || c == "..")', '        .any(|c| c.is_empty() || c == ".")',
     'axon-fabric', '--test recipe_refusals', 'a_stored_version_that_climbs_out_never_materializes'),
    ('M1647', "RECIPE (r4c part 2): a name with a control character is refused (line-based manifest)", _WR,
     '    if path.chars().any(char::is_control) {', '    if false && path.chars().any(char::is_control) {',
     'axon-fabric', '--test recipe_refusals', 'a_name_with_a_control_character_is_refused'),
    ('M1648', "RECIPE (r4c part 2): a symlink to an absolute target is refused", _WR,
     "    if t.is_empty() || t.starts_with('/') {", "    if t.is_empty() {",
     'axon-fabric', '--test recipe_refusals', 'an_absolute_symlink_target_is_refused'),
    ('M1649', "RECIPE (r4c part 2): a non-UTF-8 name refuses the tree, never silently dropped", _WR,
     '        let Some(name) = name.to_str() else {\n            return Err(ImportRefusal::NonUtf8(if prefix.is_empty() {',
     '        let Some(name) = name.to_str() else {\n            if true {\n                continue;\n            }\n            return Err(ImportRefusal::NonUtf8(if prefix.is_empty() {',
     'axon-fabric', '--test recipe_refusals', 'a_non_utf8_name_is_refused_not_dropped'),
    ('M1650', "RECIPE (r4c part 2): a special file refuses the tree, never silently dropped", _WR,
     '            return Err(ImportRefusal::SpecialFile(path));', '            continue;',
     'axon-fabric', '--test recipe_refusals', 'a_special_file_is_refused_not_dropped'),
    ('M1651', "RECIPE (r4c part 2): an import root that is a symlink is never followed", _WR,
     '    if !meta.is_dir() {', '    if false && !meta.is_dir() {',
     'axon-fabric', '--test recipe_refusals', 'an_import_root_that_is_a_symlink_is_refused'),
]

# ── C9 round 4c, INTEGRATE (c9r4c/integrate; amendment 71, integration) ─────
# Amendment 71's diverging-constructor form brought axon-observer.rs's `die`
# calls under the refusal-site gate; one of them is a guard no row carried:
# the protected observer's store-parent operator chain (the custodian's is
# M641). The check moved out of the match arm into its own `if mode ==
# Mode::Protected` block after the uid check (same mode, same check; the
# arm's guard block otherwise swallowed load_config's exemption). Id from the
# observer workstream's unused M1550-M1559 (already PSV).
MUTATIONS += [
    ('M1550', "A94 (observer, integration): a protected observer serves only from a store whose parent chain is the operator's",
     'crates/axon-fabric/src/bin/axon-observer.rs',
     '        axon_fabric::backend::check_operator_chain(parent).unwrap_or_else(|e| die(&e));',
     '        let _ = parent;',
     'axon-fabric', '--test privileged_launcher', 'a_protected_observer_serves_only_from_a_store_the_operator_placed'),
]


# ── C9 round 4c, workstream SITES (M1760-M1999; amendment 75) ───────────────
# The crate rule (amendment 71) brought 118 refusal sites in 18 dependency
# files into scope, NOT YET SCANNED. Each was judged by reading its callers on
# the protected route (governance/notes/v022-dependency-sites.md). These rows
# are the sites that DECIDE something there; each is killed by its OWN attack
# on the production route: Fabric's psv::derive through submit on the helper
# route (the certified parser's no-summary refusal); the registry file the
# axon-fabric binary loads (CheckRegistry::load); the loop's config writer
# and intake (one suite reading, one key reading); the `axon test` binary in
# the runner's exact invocation (type-check abort, failing exit); and
# submit's axon-os admission (the approval token's four bindings).
PSV_IDS |= {f"M{n}" for n in range(1760, 2000)}
_CR = 'crates/axon-cortex/src/runner.rs'
_CL = 'crates/axon-cortex/src/lib.rs'
_CM = 'crates/axon-core/src/main.rs'
_OA = 'crates/axon-os/src/approval.rs'
_PSEL = '--no-default-features --test psv_test_selection'
_APPR = 'an_approval_token_admits_only_what_it_approved'
MUTATIONS += [
    ('M1760', "PSV-4 (sites): an output with no summary is no report (the certified parser)", _CR,
     '    let Some(total) = total else {', '    let Some(total) = total.or(Some(0)) else {',
     'axon-fabric', '--test psv_dispatch', 'an_output_with_no_summary_is_no_verdict'),
    ('M1761', "PSV-5 (sites): a suite id holding a separator is not an id (the one id rule)", _CR,
     "    if id.is_empty()\n        || id\n            .chars()\n            .any(|c| matches!(c, '@' | '#' | ':' | '/') || c.is_whitespace() || c.is_control())\n    {",
     "    if false {",
     'axon-cortex', '--test check_executor', 'a_registry_file_never_registers_a_suite_id_the_id_rule_refuses'),
    ('M1762', "PSV-5 (sites): a suite reference whose version holds a separator, or whose version or entry is empty, has no reading", _CR,
     "    if version.is_empty() || version.contains(['@', '#']) || entry.is_empty() {",
     "    if false && (version.is_empty() || version.contains(['@', '#']) || entry.is_empty()) {",
     'axon-loop', '--test intake', 'a_suite_reference_with_a_second_reading_is_never_pinned'),
    ('M1763', "LOOP (sites): a document holding one key twice is refused on its raw bytes (parse_strict)", _CL,
     '                    if out.contains_key(&k) {', '                    if false && out.contains_key(&k) {',
     'axon-loop', '--test intake', 'an_episode_holding_one_key_twice_is_never_recorded'),
    ('M1764', "PSV-3 (sites): `axon test` runs no test of a program that does not type-check", _CM,
     '        eprintln!("error: {} type error(s); tests aborted", type_errors.len());\n        process::exit(2);',
     '        eprintln!("error: {} type error(s); tests aborted", type_errors.len());',
     'axon-core', _PSEL, 'a_candidate_that_does_not_type_check_is_never_tested'),
    ('M1765', "PSV-4 (sites): `axon test` exits non-zero when a test failed (a failure is never read as no verdict)", _CM,
     '    process::exit(if failed == 0 { 0 } else { 3 });', '    process::exit(0);',
     'axon-core', _PSEL, 'a_run_whose_test_failed_exits_nonzero'),
    ('M1766', "ADMISSION (sites): an approval token admits only a decision of `approved`", _OA,
     '    if t.decision != "approved" {', '    if false && t.decision != "approved" {',
     'axon-fabric', '--test grant_authority', _APPR),
    ('M1767', "ADMISSION (sites): an approval token admits only the program it approved", _OA,
     '    if pd != t.program_digest {', '    if false && pd != t.program_digest {',
     'axon-fabric', '--test grant_authority', _APPR),
    ('M1768', "ADMISSION (sites): an approval token admits only under the grant it approved", _OA,
     '    if gd != t.grant_digest {', '    if false && gd != t.grant_digest {',
     'axon-fabric', '--test grant_authority', _APPR),
    ('M1769', "ADMISSION (sites): an approval token whose metadata changed after its digest admits nothing", _OA,
     '    if td != t.token_digest {', '    if false && td != t.token_digest {',
     'axon-fabric', '--test grant_authority', _APPR),
]

# ── C9 round 4c, workstream GATE (amendment 74; M1720-M1759) ─────────────────
# Round 4c (EQUIVALENCE) found the PSV-4 keyed-evidence primitive
# axon_psv::keyed_outcome with no row and invisible to the refusal gate (it
# refuses by `return None` / `.then_some`), and the gate counting a site
# covered by a row whose edit never touched it (an `old` text ending in a
# newline reached the next line). The gate now judges a function that decides
# by bool/Option as a site of its own and covers a site only by a row whose
# edit CHANGES a line of its guard block; the rows below are the guards that
# exposed, each killed by its own attack on the production route.
PSV_IDS |= {f"M{n}" for n in range(1720, 1760)}
_PSL = 'crates/axon-psv/src/lib.rs'
_KEYED_CMP = '    (v["completion"].as_str() == Some(outcome_token(key, test, passed).as_str())).then_some(passed)'
MUTATIONS += [
    # The token comparison, judged on the guest runner (the verdict the guest
    # writes) and on Fabric's derive (the only keyed check of a FAILURE there).
    ('M1720', "PSV-4 (gate): a result line counts only with the token K issues for its outcome (guest runner)", _PSL,
     _KEYED_CMP, '    Some(passed)',
     'axon-psv', '--test runner', 'a_lone_unkeyed_failure_line_is_not_a_verdict'),
    ('M1721', "PSV-4 (gate): a result line counts only with the token K issues for its outcome (Fabric derive)", _PSL,
     _KEYED_CMP, '    Some(passed)',
     'axon-fabric', '--test psv_dispatch', 'every_forgery_of_the_returned_evidence_is_unknown_for_its_own_reason'),
    # The exactly-one-line rule: the attack is a candidate's line printed
    # before the interpreter's own; "the last line counts" is the removal it
    # would exploit (the first line is the candidate's, never keyed).
    ('M1722', "PSV-3/4 (gate): exactly one result line names the test, or there is no keyed outcome", _PSL,
     '    let (Some(v), None) = (lines.next(), lines.next()) else {',
     '    let (Some(v), None) = (lines.last(), None::<serde_json::Value>) else {',
     'axon-fabric', '--test psv_dispatch', 'a_second_pass_line_over_a_genuine_pass_is_not_a_pass'),
    # EVL's own independence rule for a pass. Retired EQUIVALENT_DID below
    # (four cells against M10 + M1299): a ROW, so the gate sees it is judged.
    ('M1726', "EVL (gate): an untrusted or subject verifier cannot establish a pass", 'crates/axon-loop/src/evl.rs',
     '            } else if !issuer_ok {', '            } else if false && !issuer_ok {',
     'axon-loop', '--test evl_admission', 'a_pass_verified_by_a_subject_issuer_never_counts'),
    # The one-process rule of a pinned custodian's reply (M1489 removes the
    # whole verification; its kill evidences the pin comparison only).
    ('M1727', "A (gate): a custodian reply two processes wrote is refused even when both run the pinned program",
     'crates/axon-fabric/src/custodian.rs',
     '    if seen.is_some_and(|s| s != pid) {', '    if false && seen.is_some_and(|s| s != pid) {',
     'axon-fabric', '--test privileged_launcher',
     'a_reply_two_processes_wrote_is_refused_even_when_both_run_the_pinned_program'),
]
# The predicate primitives the per-function rule exposed whose removal no row
# made: the PCI provenance rule itself (span_in_sealed: the ONE rule the
# static check and the runtime edge share; fn_is_sealed: the runtime's), the
# operator-global table of the static check, the guest's production "no
# bypass" constant, the reference grammar's scheme rule, and the class a
# freeze keeps when it is read back from the ledger.
_CLS = 'interp::tests::runtime_sealing_holds_without_the_static_check'
MUTATIONS += [
    ('M1730', "PCI (gate): a definition in a sealed module's file is sealed (the one provenance rule)",
     'crates/axon-core/src/resolver.rs',
     '        sealed.iter().any(|d| p.starts_with(d))', '        sealed.iter().any(|d| p.starts_with(d)) && false',
     'axon-core', '--no-default-features --lib', _CLS),
    ('M1731', "PCI (gate): a function defined in a sealed module runs sealed", 'crates/axon-core/src/interp.rs',
     '        self.seal.active && self.seal.fns.contains(&(f as *const FnDef as usize))',
     '        false && self.seal.active && self.seal.fns.contains(&(f as *const FnDef as usize))',
     'axon-core', '--no-default-features --lib', _CLS),
    ('M1732', "REF (gate): a reference names content only under the cl22/acf1/sha256 schemes",
     'crates/axon-loop-contracts/src/ids.rs',
     '            _ => None,\n        }\n    }\n}\n\nimpl Ref {',
     '            _ => Some(RefScheme::Cl22),\n        }\n    }\n}\n\nimpl Ref {',
     'axon-loop', '--test cli', 'a_reference_of_an_unknown_scheme_is_never_recorded'),
    ('M1733', "GUEST PID 1 (gate): a production guest has no unpoliced bypass", 'crates/axon-guest-init/src/main.rs',
     'fn allow_unpoliced() -> bool {\n    false\n}', 'fn allow_unpoliced() -> bool {\n    true\n}',
     'axon-guest-init', '--test policy_refusals', 'an_untrustworthy_cmdline_policy_starts_no_workload'),
    ('M1734', "PCI (gate): an operator constant is an operator global the sealed module cannot name",
     'crates/axon-core/src/resolver.rs',
     '                Item::LetDef { name, .. } => Some(name.as_str()),', '                Item::LetDef { .. } => None,',
     'axon-core', '--no-default-features --lib', 'resolver::tests::a_sealed_module_cannot_reach_the_operators_names'),
    ('M1735', "D3 (gate): a protected freeze reads back from the ledger as protected", 'crates/axon-loop/src/plan.rs',
     '        *self == EvaluationClass::Development', '        true',
     'axon-loop', '--test protected_class', 'a_protected_plan_counts_only_protected_backends'),
]
_ALB = 'crates/axon-loop/src/bin/axon-loop.rs'
MUTATIONS += [
    ('M1736', "TEL CLI (gate): a telemetry request of another schema is refused", _ALB,
     '            if req.schema != "axon.loop.tel-request/1" {', '            if false && req.schema != "axon.loop.tel-request/1" {',
     'axon-loop', '--test price_tel_sites', 'a_request_of_another_schema_is_never_summarized'),
    ('M1737', "TEL CLI / G10 (gate): Fabric attempts need a pinned price schedule", _ALB,
     '                (None, Some(_)) => Err(LoopError::Refused(\n                    "fabric_attempts require a pinned price_schedule (G10)".into(),\n                )),',
     '                (None, Some(_)) => Ok(json!({"schema":"axon.loop.tel-summary/1"})),',
     'axon-loop', '--test price_tel_sites', 'fabric_attempts_without_a_pinned_schedule_are_never_summarized'),
]
_GT = '--no-default-features --test refusal_coverage_gate'
_HI = '--no-default-features --test harness_integrity'
MUTATIONS += [
    ('M1738', "COVERAGE GATE (gate): a function that decides by bool/Option is a site", _RCG,
     '    for a, b in predicate_fns(lines):', '    for a, b in []:',
     'axon-core', _GT, 'a_function_that_decides_by_bool_or_option_is_a_site'),
    ('M1739', "COVERAGE GATE (gate): a row covers only the lines its edit changes", _RCG,
     '        spans.append((r[0], changed_lines(text, r[3], r[4])))',
     '        _a = line_of(text, text.index(r[3]))\n        spans.append((r[0], set(range(_a, _a + r[3].count("\\n") + 1))))',
     'axon-core', _GT, 'a_row_covers_only_what_its_edit_changes'),
    ('M1740', "COVERAGE GATE (gate): a predicate function is exempted only at its head line", _RCG,
     '        ex_hit = [e for e in ex if ((g <= e[0] <= i) if kind in ("line", "const", "form") else e[0] == g)]',
     '        ex_hit = [e for e in ex if g <= e[0] <= i]',
     'axon-core', _GT, 'an_exemption_in_a_predicate_fns_body_does_not_exempt_the_fn'),
    ('M1741', "COVERAGE GATE (gate): a let-else is the opener of its refusal", _RCG,
     '|let\\b.*\\belse\\s*\\{\\s*$)|=>")', ')|=>")',
     'axon-core', _GT, 'a_let_else_is_the_opener_of_its_refusal'),
    ('M1742', "COVERAGE GATE (gate): a guard block never crosses a function boundary", _RCG,
     '        if j < i and FN_HEAD.match(lines[j]):\n            break\n', '',
     'axon-core', _GT, 'a_guard_block_does_not_cross_a_function_boundary'),
    ('M1743', "HARNESS (gate): --join derives HOLDS from the recorded cells, not the label", 'scripts/v022_paired_disable.py',
     '            if bool(r.get("holds")) != want and not (r.get("status") == "STALE_REFACTORED" and not r.get("holds")):',
     '            if False:',
     'axon-core', _HI, 'a_join_refuses_a_record_whose_label_is_not_its_cells'),
    ('M1744', "HARNESS (gate): --merge derives all_killed from the recorded rows, not the label", 'scripts/v022_g01_mutations.py',
     # The old text is built from two pieces: this registry is the file the row edits, and
     # its own source must not contain the guard's text a second time.
     '        if d["all_killed"] and' + ' notgood:', '        if False and d["all_killed"] and' + ' notgood:',
     'axon-core', _HI, 'a_merge_refuses_a_shard_claiming_all_killed_over_a_survivor'),
]
MUTATIONS += [
    ('M1745', "JOURNAL (triage): a contended journal lock is retried long enough to outlast a loaded host's fork-to-exec window",
     'crates/axon-fabric/src/journal.rs',
     'const LOCK_RETRY_WINDOW: std::time::Duration = std::time::Duration::from_millis(5000);',
     'const LOCK_RETRY_WINDOW: std::time::Duration = std::time::Duration::from_millis(1);',
     'axon-fabric', '--test journal', 'a_lock_held_only_by_a_forks_inherited_description_is_waited_out'),
]
MUTATIONS += [
    ('M1746', "APPROVAL (gate): a job whose policy requires approval does not run without a token", 'crates/axon-os/src/approval.rs',
     '    } else if manifest.require_approval {\n        Err(format!(\n            "approval required but missing',
     '    } else if false && manifest.require_approval {\n        Err(format!(\n            "approval required but missing',
     'axon-fabric', '--test grant_authority', 'the_grants_require_approval_policy_is_enforced'),
    ('M1747', "COVERAGE GATE (gate): only a cfg(test) item's own extent is hidden, never the code after it", _RCG,
     '    return ["" if i in hidden else l for i, l in enumerate(lines)]',
     '    return lines[:min(hidden)] if hidden else lines',
     'axon-core', _GT, 'production_code_after_a_test_module_is_scanned'),
]
EQUIV_RECORD["M1726"] = {
    "property": "a pass counts only when a trusted verifier independent of the subject verified it",
    "subsumed_by": ["M10", "M1299"], "killer": "joint:M1726+M10+M1299",
    "all_paths": "evl::judge is the one place a Passed verification becomes VerifiedPass. Before the "
                 "`!issuer_ok` arm, on every path that reaches it: bind_episode (checks.rs, M1299) "
                 "refuses a passed episode whose issuer is not in `verifiers` or is in `subjects`, and "
                 "the Passed arm is entered only after verify_check_evidence (intake.rs, M10) returned "
                 "Ok, which refuses ANY cited result whose issuer is not trusted_verifiers "
                 "(config.verifiers(), the same set judge's Bench carries) or is in `subject` (judge "
                 "passes its own per-trial `subjects`). issuer_ok is the same predicate over the same "
                 "issuer (`v.issuer_ref`, the episode's verification) and the same two sets, so it is "
                 "true whenever either sibling held. Measured by hand before the harness run: the "
                 "attack (a trusted verifier listed as a subject issuer) is refused with only the arm "
                 "removed and with only the siblings removed, and counts with all three removed"}
EQUIVALENT_DID |= {"M1726"}
RETIRED |= {"M1726"}

# ── C9 round 4c, workstream ADMIT (M1770-M1829; amendment 76) ────────────────
# The refusal-site gate could not SEE a refusal expressed as a returned verdict
# (`Admission::Deny {..}`, `Verdict::Denied {..}`): axon-os's admission chain
# (gate::admit and the supervisor around it) had no site and no row. The gate
# now derives the verdict types from the in-scope code and reads five forms of
# them (v022_refusal_coverage.py, "Amendment 76"); the rows below are the gate's
# own guards, each killed by a planted production-shaped refusal it must name.
PSV_IDS |= {f"M{n}" for n in range(1770, 1830)}
MUTATIONS += [
    ('M1770', "COVERAGE GATE (admit): a built negative verdict variant is a site", _RCG,
     '    for _, i in verdict_constructions(clean, enums):\n        if not in_helper(i) and in_region(i):',
     '    for _, i in []:\n        if not in_helper(i) and in_region(i):',
     'axon-core', _GT, 'a_built_negative_verdict_variant_is_a_site'),
    ('M1771', "COVERAGE GATE (admit): `Self::Variant` in the verdict enum's own impl is the same construction", _RCG,
     '            enum = next((n for a, b, n in selfmap if a <= m.start() < b), enum)',
     '            enum = enum',
     'axon-core', _GT, 'a_self_variant_in_the_verdicts_own_impl_is_a_site'),
    ('M1772', "COVERAGE GATE (admit): a function returning a verdict is a site", _RCG,
     '    for a, b, n in decides:', '    for a, b, n in []:',
     'axon-core', _GT, 'a_function_that_returns_a_verdict_is_a_site'),
    ('M1773', "COVERAGE GATE (admit): a verdict inside a tuple return is a verdict return", _RCG,
     '        return any(_decides_return(x, enums, structs) for x in parts + [cur])',
     '        return False',
     'axon-core', _GT, 'a_function_that_returns_a_verdict_is_a_site'),
    ('M1774', "COVERAGE GATE (admit): a verdict inside a Result/Option return is a verdict return", _RCG,
     '    m = re.match(r"(?:Result|Option|Vec|Box)\\s*<(.*)>\\s*$", t, re.S)',
     '    m = re.match(r"(?:NoSuchWrapper)\\s*<(.*)>\\s*$", t, re.S)',
     'axon-core', _GT, 'a_function_that_returns_a_verdict_is_a_site'),
    ('M1775', "COVERAGE GATE (admit): a call of a verdict helper constructor is a site", _RCG,
     '                if built and all(m.group(2) in enums[m.group(1)] for m in built):',
     '                if False and built and all(m.group(2) in enums[m.group(1)] for m in built):',
     'axon-core', _GT, 'a_call_of_a_verdict_helper_constructor_is_a_site'),
    ('M1776', "COVERAGE GATE (admit): a call of an Err helper constructor is a site", _RCG,
     '            elif (re.search(r"\\bErr\\(", body)',
     '            elif (False and re.search(r"\\bErr\\(", body)',
     'axon-core', _GT, 'a_call_of_an_err_helper_constructor_is_a_site'),
    ('M1777', "COVERAGE GATE (admit): a closure predicate refused through ok_or is a site", _RCG,
     '    for a, b in inline_predicate_sites(cl):', '    for a, b in []:',
     'axon-core', _GT, 'an_inline_predicate_refused_through_ok_or_is_a_site'),
    ('M1778', "COVERAGE GATE (admit): an enum named for deciding is a verdict enum", _RCG,
     '            if strong or VERDICT_NAME.search(name):', '            if strong:',
     'axon-core', _GT, 'a_built_negative_verdict_variant_is_a_site'),
    ('M1779', "COVERAGE GATE (admit): a variant in a match arm's pattern (a tuple pattern too) is matched, not built", _RCG,
     '    if any(a <= p < b for a, b in arms):\n        return True',
     '    if False:\n        return True',
     'axon-core', _GT, 'a_built_negative_verdict_variant_is_a_site'),
    ('M1780', "COVERAGE GATE (admit): a variant inside matches!(..) is matched, not built", _RCG,
     '    if k >= 0 and ";" not in clean[k:p] and _match_close(clean, k + len("matches!")) > p:',
     '    if False and k >= 0 and ";" not in clean[k:p] and _match_close(clean, k + len("matches!")) > p:',
     'axon-core', _GT, 'a_built_negative_verdict_variant_is_a_site'),
    ('M1781', "COVERAGE GATE (admit): a cfg(all(test, ..)) item is test code", _RCG,
     'CFG_TEST = re.compile(r"^[ \\t]*#\\[cfg\\((?:test|all\\(test,[^\\]\\n]*\\))\\)\\][ \\t]*$", re.M)',
     'CFG_TEST = re.compile(r"^[ \\t]*#\\[cfg\\((?:test)\\)\\][ \\t]*$", re.M)',
     'axon-core', _GT, 'a_cfg_all_test_item_is_test_code_and_a_cfg_any_test_item_is_not'),
]
_AV = '--test admit_verdict_sites'
MUTATIONS += [
    ('M1782', "GUEST PID 1 (admit): a guest with no policy and no bypass refuses to start (the decision arm)",
     'crates/axon-guest-init/src/main.rs',
     '        (false, false) => PolicyDecision::Refuse,',
     '        (false, false) => PolicyDecision::ProceedUnpoliced,',
     'axon-guest-init', '--test policy_refusals', 'an_untrustworthy_cmdline_policy_starts_no_workload'),
    ('M1783', "EVL (admit): a verifier-reported failure is a Fail, never a pass", 'crates/axon-loop/src/evl.rs',
     '        VerificationResult::Failed => (Outcome::Fail, "verifier reported failure".into(), None),',
     '        VerificationResult::Failed => (\n            Outcome::VerifiedPass,\n            "verifier reported failure".into(),\n            None,\n        ),',
     'axon-loop', _AV, 'a_verifier_reported_failure_is_a_fail_never_a_pass'),
    ('M1784', "EVL (admit): a trial no episode was delivered for is Unknown, never a pass", 'crates/axon-loop/src/evl.rs',
     '                arm.missing += 1;\n                (\n                    Outcome::Unknown,\n                    "missing: no episode delivered".to_string(),',
     '                (\n                    Outcome::VerifiedPass,\n                    "missing: no episode delivered".to_string(),',
     'axon-loop', _AV, 'a_trial_with_no_delivered_episode_is_never_a_pass'),
]
_GA = 'crates/axon-os/src/gate.rs'
_SUP = 'crates/axon-os/src/supervisor.rs'
_RT = 'crates/axon-os/src/runtime.rs'
_AR = '--test admit_route'
_AXES = 'a_program_using_an_axis_the_grant_withholds_is_denied_on_every_axis'
_SCAN = 'the_effects_scan_is_not_evaded_by_spacing_or_an_import'
MUTATIONS += [
    ('M1785', "ADMISSION (admit): a program using fs_read is denied under a grant withholding fs_read (the axis row)", _GA,
     '        (declared.row.fs_read, permitted.fs_read, "fs_read"),', '        (declared.row.fs_read, true, "fs_read"),',
     'axon-fabric', _AR, _AXES),
    ('M1786', "ADMISSION (admit): a program using fs_write is denied under a grant withholding fs_write (the axis row)", _GA,
     '        (declared.row.fs_write, permitted.fs_write, "fs_write"),', '        (declared.row.fs_write, true, "fs_write"),',
     'axon-fabric', _AR, _AXES),
    ('M1787', "ADMISSION (admit): a program using net is denied under a grant withholding net (the axis row)", _GA,
     '        (declared.row.net, permitted.net, "net"),', '        (declared.row.net, true, "net"),',
     'axon-fabric', _AR, _AXES),
    ('M1788', "ADMISSION (admit): a program using exec is denied under a grant withholding exec (the axis row)", _GA,
     '        (declared.row.exec, permitted.exec, "exec"),', '        (declared.row.exec, true, "exec"),',
     'axon-fabric', _AR, _AXES),
    ('M1789', "ADMISSION (admit): gate::admit denies on the first effect axis the grant withholds", _GA,
     '        if needs && !has {\n            return Admission::Deny {', '        if false && needs && !has {\n            return Admission::Deny {',
     'axon-fabric', _AR, _AXES),
    ('M1790', "ADMISSION (admit): gate::admit denies a program above the grant's confidentiality ceiling", _GA,
     '    if declared.max_label > grant.max_label {', '    if false && declared.max_label > grant.max_label {',
     'axon-fabric', _AR, 'a_program_above_the_grants_confidentiality_ceiling_is_denied'),
    ('M1791', "ADMISSION (admit): the supervisor honours gate::admit's denial", _SUP,
     '    if let Admission::Deny { reason, axis } = admit(&declared, &eff) {',
     '    if let Admission::Deny { reason, axis } = Admission::Admit {',
     'axon-fabric', _AR, _AXES),
    ('M1792', "ADMISSION (admit): the supervisor honours approval::authorize's denial", _SUP,
     '        Err(reason) => {\n            let denial = RawEvent::new("denied", "approval", EffectSet::default(), "");\n            let mut rec = build(\n                run_id,\n                manifest,\n                manifest.seed,\n                std::slice::from_ref(&denial),\n                Verdict::Denied {\n                    reason,\n                    axis: "approval".to_string(),\n                },\n            );\n            rec.approval = crate::approval::ApprovalStatus::Denied.as_str().to_string();\n            return rec;\n        }',
     '        Err(_reason) => crate::approval::ApprovalStatus::NotRequired,',
     'axon-fabric', '--test grant_authority', 'the_grants_require_approval_policy_is_enforced'),
    ('M1793', "ADMISSION (admit): the effects scan sees a call with whitespace before its parenthesis", _RT,
     '        while i < bytes.len() && (bytes[i] as char).is_whitespace() {\n            i += 1;\n        }\n', '',
     'axon-fabric', _AR, _SCAN),
    ('M1794', "ADMISSION (admit): the effects scan sees a call of a name", _RT,
     "        if i < bytes.len() && bytes[i] == b'(' {\n            return true;\n        }",
     "        if false && i < bytes.len() && bytes[i] == b'(' {\n            return true;\n        }",
     'axon-fabric', _AR, _SCAN),
    ('M1795', "ADMISSION (admit): the effects scan sees a bare `mod` declaration (effects in a file it cannot read)", _RT,
     '        if name == "mod" {\n            return true;\n        }', '        if false && name == "mod" {\n            return true;\n        }',
     'axon-fabric', _AR, _SCAN),
]
_HWTEST = 'hardware_isolation_is_never_dropped_to_the_host_interpreter'
_HWTEST2 = 'a_hardware_isolation_requirement_is_never_met_by_a_process_scoped_runtime'
MUTATIONS += [
    ('M1796', "ADMISSION (admit): the supervisor refuses a request whose required isolation the runtime does not provide (library-tested; select dominates it on the route, M1052)", _SUP,
     '    if !required.satisfied_by(iso) {', '    if false && !required.satisfied_by(iso) {',
     'axon-os', '--test admit_isolation', _HWTEST2),
    ('M1797', "ADMISSION (admit): a process-scoped runtime does not satisfy a hardware-isolation requirement (library-tested; select dominates it on the route, M1052)", _RT,
     '            (IsolationRequirement::HardwareIsolated, Isolation::ProcessScoped) => false,',
     '            (IsolationRequirement::HardwareIsolated, Isolation::ProcessScoped) => true,',
     'axon-os', '--test admit_isolation', _HWTEST2),
]
# The isolation requirement is checked twice on Fabric's route: backend::select
# refuses a request for hardware isolation with os=none before anything is
# admitted (M1052), and supervise_requiring refuses it again on the isolation
# axis (M1796 the guard, M1797 the predicate's ProcessScoped arm). For the
# question "did it run" select dominates them on every route Fabric has, so they
# are LIBRARY_PRIMITIVE (ruling R1): killed only by a direct call of
# supervise_requiring (axon-os tests/admit_isolation.rs), never counted killed.
# M1052's recorded kill was the SUPERVISOR's refusal (assert_never_runs read any
# receipt that was not Unsupported as "it ran"); assert_never_runs now judges the
# attack by effect, and M1052 stays ACTIVE on what only selection answers, the
# receipt contract (Unsupported, never another status:
# hardware_isolation_linux_is_refused_without_a_qualified_profile). A four-cell
# retirement of the pair was executed and REFUSED: the supervisor's rows are
# pinned by axon-os's own suite (a retirement needs the full suite green).
_ISO_ROUTES = {
    "submit::supervisor_admits (the one production caller of supervise_requiring)": "backend::select, M1052: it "
        "returns no profile for hardware isolation with os=none and refuses every os=linux request without it "
        "(M1051), and for hardware_isolation+linux returns only LINUX_MICROVM_PROTECTED, whose isolation "
        "satisfies MicroVm; supervisor_admits runs after select with the SELECTED profile, so the requirement "
        "it derives is always satisfied by the runtime it is given",
}
for _m, _what in (("M1796", "the supervisor's isolation guard"), ("M1797", "satisfied_by's ProcessScoped arm")):
    LIB_RECORD[_m] = {
        "property": "a request requiring hardware isolation is never run on a process-scoped runtime (" + _what + ")",
        "routes": _ISO_ROUTES,
        "library_test": "axon-os --test admit_isolation " + _HWTEST2}
LIBRARY_PRIMITIVE |= {"M1796", "M1797"}
_LA = 'crates/axon-loop/src/admission.rs'
MUTATIONS += [
    ('M1798', "ADMISSION (admit): a candidate with an unsafe attempt is vetoed, never accepted", _LA,
     '        return (Decision::Vetoed, vetoes);', '        return (Decision::Accept, vetoes);',
     'axon-loop', _AV, 'a_candidate_with_an_unsafe_attempt_is_never_accepted'),
    ('M1799', "ADMISSION (admit): a candidate established inferior is rejected, never accepted", _LA,
     '        (Decision::Reject, reject)', '        (Decision::Accept, reject)',
     'axon-loop', _AV, 'a_candidate_inferior_on_quality_is_never_accepted'),
    ('M1800', "ADMISSION (admit): a candidate that cannot be established is inconclusive, never accepted", _LA,
     '        (Decision::Inconclusive, inconclusive)', '        (Decision::Accept, inconclusive)',
     'axon-loop', _AV, 'a_candidate_whose_noninferiority_cannot_be_established_is_never_accepted'),
    ('M1801', "ADMISSION (admit): a REJECT is recorded as a REJECT verdict", _LA,
     '            Decision::Reject => Verdict::Reject,', '            Decision::Reject => Verdict::Accept,',
     'axon-loop', _AV, 'each_admission_disposition_is_recorded_as_its_own_verdict'),
    ('M1802', "ADMISSION (admit): an INCONCLUSIVE is recorded as an INCONCLUSIVE verdict", _LA,
     '            Decision::Inconclusive => Verdict::Inconclusive,', '            Decision::Inconclusive => Verdict::Accept,',
     'axon-loop', _AV, 'each_admission_disposition_is_recorded_as_its_own_verdict'),
    ('M1803', "ADMISSION (admit): a VETO is recorded as a VETOED verdict", _LA,
     '            Decision::Vetoed => Verdict::Vetoed,', '            Decision::Vetoed => Verdict::Accept,',
     'axon-loop', _AV, 'each_admission_disposition_is_recorded_as_its_own_verdict'),
]
_CK = 'crates/axon-loop-contracts/src/checks.rs'
_CS = '--test checks_sites'
MUTATIONS += [
    ('M1804', "INTAKE (admit): a failed check receipt is read as a failed verification, never a pass", 'crates/axon-loop/src/intake.rs',
     '        (ReceiptStatus::Completed, ReceiptVerification::Failed) => VerificationResult::Failed,',
     '        (ReceiptStatus::Completed, ReceiptVerification::Failed) => VerificationResult::Passed,',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M1805', "INTAKE (admit): a check receipt that reached no verdict is read as an unknown verification, never a pass", 'crates/axon-loop/src/intake.rs',
     '        ) => VerificationResult::Unknown,', '        ) => VerificationResult::Passed,',
     'axon-loop', '--test intake', 'verification_that_does_not_join_is_refused_with_the_store_unchanged'),
    ('M1806', "TEL (admit): a cost total beyond 2^53-1 is refused, never summarized", _TL,
     '        .filter(|s| *s <= MAX_INTEGER)', '        .filter(|_s| true)',
     'axon-loop', _PTS, 'a_cost_total_beyond_2_53_is_never_summarized'),
    ('M1807', "SAFETY (admit): a reported violation marks the trial a Violation", 'crates/axon-loop/src/safety.rs',
     '            (_, Finding::Violation, Some(code)) => SafetyState::Violation { code },',
     '            (_, Finding::Violation, Some(_code)) => SafetyState::Clear,',
     'axon-loop', _AV, 'a_candidate_with_an_unsafe_attempt_is_never_accepted'),
    ('M1808', "CHECKS (admit): a Failed execution receipt projects to a Failed episode status", _CK,
     '        ReceiptStatus::Failed => EpisodeStatus::Failed,', '        ReceiptStatus::Failed => EpisodeStatus::Completed,',
     'axon-loop', _CS, 'a_pass_whose_execution_failed_never_counts'),
    ('M1809', "CHECKS (admit): a Canceled execution receipt projects to a Cancelled episode status", _CK,
     '        ReceiptStatus::Canceled => EpisodeStatus::Cancelled,', '        ReceiptStatus::Canceled => EpisodeStatus::Completed,',
     'axon-loop-contracts', '--test admit_verdict_sites', 'a_canceled_execution_receipt_never_projects_to_a_completed_episode'),
    ('M1810', "CHECKS (admit): a Denied execution receipt projects to a Refused episode status", _CK,
     '        ReceiptStatus::Denied => EpisodeStatus::Refused,', '        ReceiptStatus::Denied => EpisodeStatus::Completed,',
     'axon-loop', _CS, 'a_pass_whose_execution_was_denied_never_counts'),
    ('M1811', "CHECKS (admit): an Unsupported execution receipt projects to an Unsupported episode status", _CK,
     '        ReceiptStatus::Unsupported => EpisodeStatus::Unsupported,', '        ReceiptStatus::Unsupported => EpisodeStatus::Completed,',
     'axon-loop', _CS, 'a_pass_whose_execution_was_unsupported_never_counts'),
    ('M1812', "CHECKS (admit): an unknown or timed-out execution receipt projects to an OutcomeUnknown episode status", _CK,
     '        ReceiptStatus::OutcomeUnknown | ReceiptStatus::TimedOut => EpisodeStatus::OutcomeUnknown,',
     '        ReceiptStatus::OutcomeUnknown | ReceiptStatus::TimedOut => EpisodeStatus::Completed,',
     'axon-loop', _CS, 'a_pass_whose_execution_outcome_is_unknown_never_counts'),
]
_PR2 = 'crates/axon-psv/src/runner.rs'
_BK = 'crates/axon-fabric/src/backend.rs'
_SB = 'crates/axon-fabric/src/submit.rs'
_LRN = 'a_launcher_result_that_is_not_a_clean_bound_success_is_never_receipted_completed'
MUTATIONS += [
    ('M1813', "GUEST RUNNER (admit): a result without K's keyed token for its outcome is Unknown, whatever the first reading said", _PR2,
     '        (GuestStatus::Failed, Some(false), Some(c)) if c != 0 => GuestStatus::Failed,\n        _ => GuestStatus::Unknown,',
     '        (GuestStatus::Failed, Some(false), Some(c)) if c != 0 => GuestStatus::Failed,\n        _ => status,',
     'axon-psv', '--test runner', 'a_lone_unkeyed_failure_line_is_not_a_verdict'),
    ('M1814', "GUEST RUNNER (admit): a refusal is reported as a refusal, never a pass", _PR2,
     '        status: GuestStatus::Refused,', '        status: GuestStatus::Passed,',
     'axon-psv', '--test runner', 'a_refusal_is_never_reported_as_a_pass'),
    ('M1815', "LINUX RESULT (admit): a result.json of another schema is no result", _BK,
     '    if r["schema"] != "axon-linux-microvm-result/1" {', '    if false && r["schema"] != "axon-linux-microvm-result/1" {',
     'axon-fabric', '--test submit', _LRN),
    ('M1816', "LINUX RESULT (admit): a cleanup not confirmed complete leaves the outcome unknown", _BK,
     '    if cleanup_complete != Some(true) {', '    if false && cleanup_complete != Some(true) {',
     'axon-fabric', '--test submit', _LRN),
    ('M1817', "LINUX RESULT (admit): an output the result says is not bound is no success", _BK,
     '            if r["output_bound"].as_bool() != Some(true) {', '            if false && r["output_bound"].as_bool() != Some(true) {',
     'axon-fabric', '--test submit', _LRN),
    ('M1818', "LINUX RESULT (admit): an output --verify-result did not re-bind is no success", _BK,
     '                other => {\n                    return (\n                        LinuxOutcome::Unknown,\n                        format!("--verify-result did not re-bind the output (exit {other:?})"),\n                        evidence,\n                    )\n                }',
     '                other => {\n                    let _ = other;\n                }',
     'axon-fabric', '--test submit', _LRN),
    ('M1819', "LINUX RESULT (admit): a result with no workload_exit is no success", _BK,
     '            let Some(w) = r["workload_exit"].as_i64() else {\n                return (LinuxOutcome::Unknown, "no workload_exit".into(), evidence);\n            };',
     '            let w = r["workload_exit"].as_i64().unwrap_or(0);',
     'axon-fabric', '--test submit', _LRN),
    ('M1820', "LINUX RESULT (admit): a launcher exit 0 over a workload that did not exit 0 is no success", _BK,
     '            if exit == Some(0) && w == 0 {', '            if exit == Some(0) {',
     'axon-fabric', '--test submit', _LRN),
    ('M1821', "RECEIPT (admit): an unknown launcher outcome is receipted OutcomeUnknown, never completed", _SB,
     '        backend::LinuxOutcome::Unknown => (\n            ReceiptStatus::OutcomeUnknown,\n            None,\n            ReceiptVerification::Unknown,\n        ),',
     '        backend::LinuxOutcome::Unknown => (\n            ReceiptStatus::Completed,\n            Some(0),\n            ReceiptVerification::NotRequested,\n        ),',
     'axon-fabric', '--test submit', _LRN),
    ('M1822', "RECEIPT (admit): a refused launch is receipted Denied, never completed", _SB,
     '        backend::LinuxOutcome::Refused => {\n            (ReceiptStatus::Denied, None, ReceiptVerification::NotRun)\n        }',
     '        backend::LinuxOutcome::Refused => {\n            (ReceiptStatus::Completed, Some(0), ReceiptVerification::NotRequested)\n        }',
     'axon-fabric', '--test submit', _LRN),
    ('M1823', "RECEIPT (admit): a timed-out launch is receipted TimedOut, never completed", _SB,
     '        backend::LinuxOutcome::TimedOut => {\n            (ReceiptStatus::TimedOut, None, ReceiptVerification::Unknown)\n        }',
     '        backend::LinuxOutcome::TimedOut => {\n            (ReceiptStatus::Completed, Some(0), ReceiptVerification::NotRequested)\n        }',
     'axon-fabric', '--test submit', _LRN),
    ('M1826', "RECEIPT (admit): a verdict over bytes the run did not judge is no verdict", _SB,
     '            if problem.is_some()\n                && matches!(', '            if false\n                && matches!(',
     'axon-fabric', '--test workspace', 'a_verdict_over_bytes_the_run_did_not_judge_is_never_receipted'),
]
LIB_RECORD["M1809"] = {
    "property": "a canceled execution receipt projects to a Cancelled episode status, never a Completed one",
    "routes": {
        "evl::evaluate (judge, bind_acf's only production caller)": "M131 (evl.rs `run_end`): a receipt "
            "that ended Canceled makes a Passed or Failed verdict Unknown (\"the run ended Cancelled\"), "
            "whatever the episode says, before any count; measured: with the arm mapped to Completed, "
            "tests/checks_sites.rs a_pass_whose_execution_was_canceled_never_counts still reads the trial "
            "Unknown (reason: the run ended Cancelled)",
        "tel::join (project_receipt_status for the usage's episode status)": "decides no verdict: the status "
            "only counts `non_completed_records` in a cost summary"},
    "library_test": "axon-loop-contracts --test admit_verdict_sites "
                    "a_canceled_execution_receipt_never_projects_to_a_completed_episode"}
LIBRARY_PRIMITIVE |= {"M1809"}


# ── C9 round 5, INTEGRATE (amendment 81 follow-up): the arms of v022_paired_disable.status_problems
# that eqgate's freeze test exercises but its id range could not row (M1900-M1959). Each is killed by
# a defect case of harness_integrity::a_status_file_is_accepted_only_as_the_joined_evidence_for_the_
# freeze_commit, which names the defect in its ATTACK text. M1969/M1972 edit the arm AND the arm it
# would otherwise fall through to, so the attack is not refused by a sibling (REFUSED_ELSEWHERE).
PSV_IDS |= {f"M{n}" for n in range(1960, 1990)}
MUTATIONS += [
    ('M1960', 'FREEZE (integrate): a status file with schema is refused', 'scripts/v022_paired_disable.py',
     '    if doc.get("schema") != "axon-v022-paired-disable/2":',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1961', 'FREEZE (integrate): a status file with a tree that was not clean is refused', 'scripts/v022_paired_disable.py',
     '    if doc.get("tree_clean") is not True:',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1962', "FREEZE (integrate): a status file with a registry other than this tree's is refused", 'scripts/v022_paired_disable.py',
     '    if doc.get("registry_blobs") != mut.registry_blobs():',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1963', 'FREEZE (integrate): a status file with a file with no records list is refused', 'scripts/v022_paired_disable.py',
     '    if not isinstance(doc, dict) or not isinstance(doc.get("records"), list):',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1964', 'FREEZE (integrate): a status file with records for rows that are not retirements is refused', 'scripts/v022_paired_disable.py',
     '    if extra:\n        out.append(f"records for rows that are not retirements here',
     '    if False:\n        out.append(f"records for rows that are not retirements here',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1965', 'FREEZE (integrate): a status file with duplicate records is refused', 'scripts/v022_paired_disable.py',
     '    if dup:\n        out.append(f"duplicate records',
     '    if False:\n        out.append(f"duplicate records',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1966', 'FREEZE (integrate): a status file with records stamped at another commit is refused', 'scripts/v022_paired_disable.py',
     '    if stamped != {commit}:',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1967', 'FREEZE (integrate): a status file with records that executed other edits is refused', 'scripts/v022_paired_disable.py',
     '        if r.get("edits_sha256") != current_edits_digest(rid):',
     '        if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1968', 'FREEZE (integrate): a status file with a record that says holds over cells that hold is refused', 'scripts/v022_paired_disable.py',
     '        elif r.get("holds") is not True:',
     '        elif False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1969', 'FREEZE (integrate): a status file with a record with no cells to derive a verdict from is refused', 'scripts/v022_paired_disable.py',
     '        if want is None:\n            out.append(f"{rid}: carries no matrix or replacement state to derive its verdict from")\n        elif not want:',
     '        if False:\n            out.append(f"{rid}: carries no matrix or replacement state to derive its verdict from")\n        elif want is False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1970', 'FREEZE (integrate): a status file with a record that names no consumer_selection is refused', 'scripts/v022_paired_disable.py',
     '        if why:\n            out.append(f"{rid}: {why}")',
     '        if False:\n            out.append(f"{rid}: {why}")',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1971', 'FREEZE (integrate): a status file with all_hold false is refused', 'scripts/v022_paired_disable.py',
     '    if doc.get("all_hold") is not True:',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1972', 'FREEZE (integrate): a status file with a commit that is not in the repository is refused', 'scripts/v022_mutation_status.py',
     '    if not isinstance(commit, str) or mut.sh(f"git cat-file -e {commit}^{{commit}}").returncode != 0:\n        return [f"its commit {commit!r} is not in this repository"]\n    if mut.sh(f"git merge-base --is-ancestor {commit} {head}").returncode != 0:',
     '    if False:\n        return [f"its commit {commit!r} is not in this repository"]\n    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1973', 'FREEZE (integrate): a status file with a commit that is not an ancestor of the freeze commit is refused', 'scripts/v022_mutation_status.py',
     '    if mut.sh(f"git merge-base --is-ancestor {commit} {head}").returncode != 0:',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1974', "FREEZE (integrate): a status file with a file whose toolchain is not its records' is refused", 'scripts/v022_paired_disable.py',
     '    elif recs and (recs[0]["environment"]["host"]["toolchain"] != doc.get("toolchain")):',
     '    elif False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
]

# ── C9 round 6, INTEGRATE (amendment 83 review): the dispatch rule's OFF switch must stay a unit-test
# facility. Removing #[cfg(test)] from its declaration makes it a production static, which the test
# (a scan of interp.rs) refuses. (Rows M1975-M1989 are free.)
MUTATIONS += [
    ('M1975', "PSV-1 (integrate): the dispatch rule's off switch is compiled only into unit tests",
     'crates/axon-core/src/interp.rs',
     '#[cfg(test)]\npub(crate) static DISPATCH_RULE_OFF: std::sync::atomic::AtomicBool =',
     'pub(crate) static DISPATCH_RULE_OFF: std::sync::atomic::AtomicBool =',
     'axon-core', '--no-default-features --test dispatch_rule_switch_is_test_only',
     'the_dispatch_rule_off_switch_is_compiled_only_into_unit_tests'),
]

# C9 round 8, EXROWS (amendment 93): exemptions whose guard a test already killed, now rowed.
MUTATIONS += [
    ('M2340', 'JOURNAL (exrows): an intent in an undeclared scope is refused', 'crates/axon-fabric/src/journal.rs',
     '                if !self.scopes.contains_key(&intent.scope) {',
     '                if false && !self.scopes.contains_key(&intent.scope) {',
     'axon-fabric', '--test journal', 'an_undeclared_scope_or_a_redeclared_ceiling_is_refused'),
    ('M2341', 'JOURNAL (exrows): a launched record for an op that was never reserved is corruption', 'crates/axon-fabric/src/journal.rs',
     '                if v.state != OpState::Reserved {\n                    return Err(bad(&v, "launched"));',
     '                if false && v.state != OpState::Reserved {\n                    return Err(bad(&v, "launched"));',
     'axon-fabric', '--test journal', 'a_record_that_violates_the_state_machine_is_corruption'),
    ('M2342', 'JOURNAL (exrows): only a launched op completes', 'crates/axon-fabric/src/journal.rs',
     '            Rec::Completed { op, billing } => {\n                let mut v = get(op)?;\n                if v.state != OpState::Launched {',
     '            Rec::Completed { op, billing } => {\n                let mut v = get(op)?;\n                if false && v.state != OpState::Launched {',
     'axon-fabric', '--test journal', 'sigkill_after_launch_reconciles_to_outcome_unknown_with_liability_kept'),
    ('M2343', 'JOURNAL (exrows): a settlement receipt names its origin', 'crates/axon-fabric/src/journal.rs',
     '        if r.origin.is_empty() {',
     '        if false && r.origin.is_empty() {',
     'axon-fabric', '--test journal', 'g13_settlement_without_origin_is_refused_and_writes_nothing'),
    ('M2344', 'JOURNAL (exrows): only an op of unknown cost is settled', 'crates/axon-fabric/src/journal.rs',
     '        if !(v.state.is_terminal() && v.billing == Some(Billing::Unknown)) {',
     '        if false && !(v.state.is_terminal() && v.billing == Some(Billing::Unknown)) {',
     'axon-fabric', '--test journal', 'failed_and_cancelled_work_is_charged_or_held_never_dropped'),
    ('M2345', 'JOURNAL (exrows): the same operation id with another input is a conflict', 'crates/axon-fabric/src/journal.rs',
     '                if v.intent == intent {\n                    return Ok(Begin::AlreadyRecorded',
     '                if true || v.intent == intent {\n                    return Ok(Begin::AlreadyRecorded',
     'axon-fabric', '--test journal', 'same_operation_id_with_a_different_input_digest_is_a_conflict'),
    ('M2346', 'BRANCHES (exrows): an experiment needs a published base', 'crates/axon-fabric/src/branches.rs',
     '        if !store.contains(base) {',
     '        if false && !store.contains(base) {',
     'axon-fabric', '--test branches', 'an_experiment_needs_a_durable_base_two_arms_and_independent_approval'),
    ('M2347', 'BRANCHES (exrows): an experiment needs two arms', 'crates/axon-fabric/src/branches.rs',
     '        if arms.len() < 2 {',
     '        if false && arms.len() < 2 {',
     'axon-fabric', '--test branches', 'an_experiment_needs_a_durable_base_two_arms_and_independent_approval'),
    ('M2348', 'BRANCHES (exrows): a writer is not an approver', 'crates/axon-fabric/src/branches.rs',
     '            if approvers.contains(w) {',
     '            if false && approvers.contains(w) {',
     'axon-fabric', '--test branches', 'an_experiment_needs_a_durable_base_two_arms_and_independent_approval'),
    ('M2349', 'BRANCHES (exrows): an experiment id reopened with different content exists', 'crates/axon-fabric/src/branches.rs',
     '            if std::fs::read(&file)? == bytes {',
     '            if true || std::fs::read(&file)? == bytes {',
     'axon-fabric', '--test branches', 'branches_start_from_one_frozen_base_with_independent_run_identities'),
    ('M2350', 'BRANCHES (exrows): a cancelled branch accepts no publication', 'crates/axon-fabric/src/branches.rs',
     '        if self.is_cancelled(exp_id, arm) {\n            return Err(BranchError::Cancelled(format!(\n                "branch {exp_id}/{arm} was cancelled"\n            )));\n        }\n        // Writer',
     '        if false && self.is_cancelled(exp_id, arm) {\n            return Err(BranchError::Cancelled(format!(\n                "branch {exp_id}/{arm} was cancelled"\n            )));\n        }\n        // Writer',
     'axon-fabric', '--test branches', 'a_cancelled_branch_refuses_a_publication_that_would_otherwise_succeed'),
    ('M2351', "BRANCHES (exrows): only the branch's writer publishes", 'crates/axon-fabric/src/branches.rs',
     '        if req.writer != br.writer {',
     '        if false && req.writer != br.writer {',
     'axon-fabric', '--test branches', 'publication_requires_base_epoch_writer_exact_verified_output_and_approval'),
    ('M2352', 'BRANCHES (exrows): the approver is independent and declared', 'crates/axon-fabric/src/branches.rs',
     '        if req.approver == req.writer || !exp.approvers.contains(&req.approver) {',
     '        if false && (req.approver == req.writer || !exp.approvers.contains(&req.approver)) {',
     'axon-fabric', '--test branches', 'publication_requires_base_epoch_writer_exact_verified_output_and_approval'),
    ('M2353', 'BRANCHES (exrows): a publication is fenced by the epoch', 'crates/axon-fabric/src/branches.rs',
     '        if now != req.epoch {',
     '        if false && now != req.epoch {',
     'axon-fabric', '--test branches', 'publication_requires_base_epoch_writer_exact_verified_output_and_approval'),
    ('M2354', 'BRANCHES (exrows): the verifying operation ran on this branch', 'crates/axon-fabric/src/branches.rs',
     '        if v.intent.scope != self.scope || v.intent.trial_id != br.run_id {',
     '        if false && (v.intent.scope != self.scope || v.intent.trial_id != br.run_id) {',
     'axon-fabric', '--test branches', 'publication_requires_base_epoch_writer_exact_verified_output_and_approval'),
    ('M2355', 'BRANCHES (exrows): a publication names the head it was made from', 'crates/axon-fabric/src/branches.rs',
     '        if head.seq != req.expected_seq || head.version != req.expected_version {',
     '        if false && (head.seq != req.expected_seq || head.version != req.expected_version) {',
     'axon-fabric', '--test branches', 'publication_requires_base_epoch_writer_exact_verified_output_and_approval'),
    ('M2356', 'AUDIT (exrows): a live ledger detects truncation of its own file', 'crates/axon-audit/src/lib.rs',
     '        if on_disk < expected {',
     '        if false && on_disk < expected {',
     'axon-audit', '--lib', 'truncation_tests::a_live_ledger_detects_truncation_of_its_own_file'),
    ('M2357', 'AUDIT (exrows): a live ledger detects records it never wrote', 'crates/axon-audit/src/lib.rs',
     '        if on_disk > expected {',
     '        if false && on_disk > expected {',
     'axon-audit', '--lib', 'truncation_tests::a_live_ledger_detects_records_it_never_wrote'),
    ('M2358', 'AUDIT (exrows): a ledger verifier recomputes every entry hash (an edited body behind an intact chain)', 'crates/axon-audit/src/lib.rs',
     '        if entry.entry_hash != expected_hash {',
     '        if false && entry.entry_hash != expected_hash {',
     'axon-audit', '--lib', 'tests::ledger_tamper_fails_verification'),
    ('M2359', 'OS LEDGER (exrows): a budget carve past the cap is refused', 'crates/axon-os/src/ledger.rs',
     '        if self.budget_used.saturating_add(c.budget) > self.budget_cap {\n            return Err',
     '        if false && self.budget_used.saturating_add(c.budget) > self.budget_cap {\n            return Err',
     'axon-os', '--lib', 'ledger::tests::mint_beyond_grant_refused_R20'),
    ('M2360', 'OS LEDGER (exrows): a persist carve past the cap is refused', 'crates/axon-os/src/ledger.rs',
     '        if self.persist_bytes_used.saturating_add(c.persist_bytes) > self.persist_bytes_cap {\n            return Err',
     '        if false && self.persist_bytes_used.saturating_add(c.persist_bytes) > self.persist_bytes_cap {\n            return Err',
     'axon-os', '--test r27_acceptance', 'weight_exfil_egress_denied_R25'),
]


def in_scope(mid, scope):
    # A SIBLING-ONLY edit exists only as a member of a retired row's guard set
    # (amendment 64): it is never an active row of any scope.
    if mid in RETIRED or mid in SIBLING_ONLY:
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


def cargo_target_dir():
    """The directory cargo builds into from ROOT, as CARGO resolves it
    (CARGO_TARGET_DIR, then any config's build.target-dir, then ROOT/target):
    never a guess (scripts/lib/axon_bin.sh, C9 round 4)."""
    r = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT,
                       capture_output=True, text=True)
    try:
        return json.loads(r.stdout)["target_directory"]
    except (ValueError, KeyError):
        sys.exit(f"refused: cannot tell where cargo builds (cargo metadata: {r.stderr.strip()[-300:]})")


def build_interpreter():
    """`cargo build` the run's interpreter (`axon`, in the cargo target dir)
    from the tree as it is now."""
    return subprocess.run(
        ["bash", "-c", f"source scripts/lib_bounded_run.sh && bounded_run {MEM} 1800 "
         "cargo build -q -p axon-core --no-default-features --bin axon"],
        cwd=ROOT, capture_output=True, text=True)


def interpreter_version(path):
    r = subprocess.run([path, "--version"], capture_output=True, text=True)
    return r.stdout.strip()


def rerun_core_build_script():
    """Make axon-core's build script run again on the tree as it is NOW.

    It embeds `<sha>-dirty` when `git status` reports a change, and cargo
    re-runs it only when .git/HEAD, .git/index or src/ change. Its own `git
    status` refreshes .git/index, so the NEXT build re-runs it -- and when that
    build is a mutated cell's, with the edit outside src/ (M860's
    tests/script_spawn), `-dirty` is baked in, and the interpreter rebuilt
    from the RESTORED tree keeps it: nothing the build script watches changed
    again. Measured at 49eb3765 (a clone, CARGO_INCREMENTAL=0, sccache): the
    restored tree's `axon --version` read `49eb3765-dirty`, another sha256
    than the run's interpreter, until build.rs was touched; then it was
    byte-identical to it again (amendment 59). Touching changes no content,
    so the tree stays clean."""
    build_rs = os.path.join(ROOT, "crates", "axon-core", "build.rs")
    if os.path.exists(build_rs):
        os.utime(build_rs)


def restore_interpreter(path, expected):
    """None once `path` is again the interpreter the run started with
    (`expected`, its sha256), rebuilt from the restored tree; otherwise why
    not. Called after EVERY row (mutation run) or cell (paired-disable), so
    each row is judged by the interpreter built from the tree under test and
    a failure to restore one is that row's, never discovered only at the end
    of the run (49eb3765 shard 1)."""
    for force in (False, True):
        if force:
            rerun_core_build_script()
        built = build_interpreter()
        if built.returncode != 0:
            return f"the restored tree's interpreter did not build: {built.stderr.strip()[-300:]}"
        if os.path.exists(path) and sha(path) == expected:
            return None
    got = sha(path) if os.path.exists(path) else "absent"
    return (f"rebuilt from the restored tree, {path} is {got[:16]} "
            f"({interpreter_version(path)}), not the run's {expected[:16]}")


def clean_interpreter(path):
    """Build the run's interpreter from the CLEAN tree and return its sha256.
    A build-script output left `-dirty` by an earlier run in this target dir
    is re-run first; an interpreter that still names itself dirty is refused
    (the tree was required clean)."""
    for force in (False, True):
        if force:
            rerun_core_build_script()
        built = build_interpreter()
        if built.returncode != 0:
            sys.exit(f"refused: could not build the axon interpreter\n{built.stderr[-2000:]}")
        if not interpreter_version(path).endswith("-dirty)"):
            return sha(path)
    sys.exit(f"refused: the interpreter built from the clean tree names itself "
             f"{interpreter_version(path)!r}")


# The files that define what a run IS: the registry and the attack markers.
# Their git blobs are recorded in every run and shard, so a run made from a
# locally edited registry cannot pass for one made from the commit it names.
REGISTRY_FILES = ("scripts/v022_g01_mutations.py", "scripts/v022_attack_markers.py")


def uncommitted():
    """Every uncommitted change in the WHOLE tree (tracked edits, staged
    changes, untracked non-ignored files), or "" for a clean tree."""
    return sh("git status --porcelain --untracked-files=all").stdout.strip()


def registry_blobs():
    return {f: sh(f"git hash-object -- {f}").stdout.strip() for f in REGISTRY_FILES}


def row_digest(row):
    """sha256 of a row's old and new text: the edit a result was made with."""
    return {"old_sha256": hashlib.sha256(row[3].encode()).hexdigest(),
            "new_sha256": hashlib.sha256(row[4].encode()).hexdigest()}


def workspace_target_dir():
    """Where a script spawned by a test builds: the WORKSPACE's own target
    dir, as cargo resolves it with CARGO_TARGET_DIR removed (the spawn helper,
    crates/axon-core/tests/script_spawn, removes it)."""
    env = {k: v for k, v in os.environ.items() if k != "CARGO_TARGET_DIR"}
    r = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT,
                       env=env, capture_output=True, text=True)
    try:
        return json.loads(r.stdout)["target_directory"]
    except (ValueError, KeyError):
        sys.exit("refused: cannot tell where a script's build lands (cargo metadata failed)")


def scrub_workspace_binaries():
    """Remove every executable a script built into the workspace target dir.

    A cell runs the MUTATED tree, and a building script under it (clock_parity,
    the parity harnesses ...) writes <workspace>/target/<profile>/<bin> from
    that tree. Scripts no longer fall back to such a file (scripts/lib/
    axon_bin.sh), but a mutant binary left behind is still a binary not built
    from the tree the NEXT cell judges (C9 round 4, EQUIVALENCE (6d)); so it is
    removed after every cell. Returns how many files were removed."""
    base = workspace_target_dir()
    n = 0
    profiles = [os.path.join(base, p) for p in ("debug", "release")]
    try:
        for sub in os.listdir(base):
            for p in ("debug", "release"):
                profiles.append(os.path.join(base, sub, p))
    except OSError:
        return 0
    for prof in profiles:
        for d in (prof, os.path.join(prof, "examples")):
            try:
                names = os.listdir(d)
            except OSError:
                continue
            for name in names:
                f = os.path.join(d, name)
                if os.path.isfile(f) and not os.path.islink(f) and os.access(f, os.X_OK):
                    os.unlink(f)
                    n += 1
    return n


def spawns_scripts(package, target):
    """Whether a row's test target runs repository scripts (it calls the one
    spawn helper, crates/axon-core/tests/script_spawn)."""
    t = target.split()
    if "--test" not in t:
        return False
    path = os.path.join(ROOT, "crates", package, "tests", t[t.index("--test") + 1] + ".rs")
    try:
        return "script_spawn::script(" in open(path).read()
    except OSError:
        return False


def package_spawns_scripts(package):
    """Whether ANY test target of `package` runs repository scripts: what a
    full-suite cell runs. A script reaches the spawn helper or the workspace
    drift test (M724) refuses it, so the helper's call is the whole fact."""
    d = os.path.join(ROOT, "crates", package, "tests")
    try:
        names = os.listdir(d)
    except OSError:
        return False
    for n in names:
        if n.endswith(".rs"):
            try:
                if "script_spawn::script(" in open(os.path.join(d, n)).read():
                    return True
            except OSError:
                continue
    return False


# Binary-naming variables a caller's shell may carry. Every cell runs with them
# REMOVED, so a test's workspace_bin() finds the binary this run built, never
# one an ambient AXON_BIN / CORTEX_BIN names (C9 round 4).
AMBIENT_BINARY_VARS = ("AXON", "AXON_BIN", "CORTEX_BIN")
UNSET_AMBIENT = "unset " + " ".join(AMBIENT_BINARY_VARS) + "; "


# Where a cell's whole output is kept when it did not pass (C9 round 4,
# amendment 59): a label alone ("compile_error", CONSUMER_BASELINE_BROKEN) is
# not evidence, and an environmental failure that does not reproduce is
# diagnosable only from what it printed.
KEEP_DIR = "/var/tmp/v022-cells"


def keep_output(kind, out):
    """Write `out` under KEEP_DIR, print where, and return the path."""
    os.makedirs(KEEP_DIR, exist_ok=True)
    tag = hashlib.sha256(f"{kind}\n{out}".encode()).hexdigest()[:12]
    path = os.path.join(KEEP_DIR, f"{re.sub(r'[^A-Za-z0-9_.-]+', '_', kind)}-{tag}.log")
    with open(path, "w") as fh:
        fh.write(out)
    print(f"    output kept: {path}", flush=True)
    return path



# ── C9 round 4c, workstream r4c-psv1 (M1660-M1719; amendment 72) ──
# PSV-1: at a seal crossing every position of the type a value is cast to is
# determined from the operator side, or the crossing is refused. Each row's
# attack is the review's (or a variant's) laundered u8 reaching the operator's
# lenient `impl Judge for u8`; each marker is that attack COMPLETING.
_CK = 'crates/axon-core/src/checker.rs'
_T72 = 'interp::tests::'
MUTATIONS += [
    ('M1660', 'PSV-1 (A96): a channel is stamped at creation with the element type its chan<T>() states', _CC,
     '        if let Some(t) = stamp {\n            entry.1.push(Rc::new(Contract {',
     '        if let Some(t) = stamp.filter(|_| false) {\n            entry.1.push(Rc::new(Contract {',
     'axon-core', _CL, _T72 + 'a_channel_carries_the_element_type_its_creation_states'),
    ('M1661', "PSV-1 (A97): a sealed send on an operator-created channel needs an operator-determined element type", _CC,
     '        if sealed && operator_side && !contracts.iter().any(|c| determined(&c.ret, &c.cx)) {',
     '        if false && sealed && operator_side && !contracts.iter().any(|c| determined(&c.ret, &c.cx)) {',
     'axon-core', _CL, _T72 + 'a_sealed_send_on_an_operator_channel_needs_a_determined_element_type'),
    ('M1662', 'PSV-1 (A98): a channel crossing Chan<T> at a strict crossing with T undetermined is refused', _CC,
     '        if mentions_tparam(&elem, &root) {\n            if cx.strict {',
     '        if mentions_tparam(&elem, &root) {\n            if false && cx.strict {',
     'axon-core', _CL, _T72 + 'a_channel_returned_at_an_undetermined_element_type_is_refused'),
    ('M1663', "PSV-1 (A99): a generic struct/enum's type argument is never erased in the field environment", _CC,
     '        .map(|(i, g)| (g.clone(), args.get(i).cloned().unwrap_or_else(undet)))',
     '        .map(|(i, g)| (g.clone(), args.get(i).map(|a| subst(a, outer, true)).unwrap_or_else(undet)))',
     'axon-core', _CL, _T72 + 'a_generic_struct_or_enum_argument_binds_its_type_parameter'),
    ('M1664', 'PSV-1 (A100): a value at a position nothing determined is refused at a strict crossing', _CC,
     '            UNDET if cx.strict => {',
     '            UNDET if false && cx.strict => {',
     'axon-core', _CL, _T72 + 'a_value_at_an_undetermined_position_never_crosses_a_seal'),
    ('M1665', "PSV-1 (A100): a strict crossing stays strict through a type parameter's binding", _CC,
     '            None => Cx::default().strict(self.strict),',
     '            None => Cx::default(),',
     'axon-core', _CL, _T72 + 'a_value_at_an_undetermined_position_never_crosses_a_seal'),
    ('M1666', 'PSV-1 (A101): a fn with no declared return type hands operator code () at a seal crossing', _CI,
     '                None if crossing => {\n                    result = Value::Unit;',
     '                None if false && crossing => {\n                    result = Value::Unit;',
     'axon-core', _CL, _T72 + 'a_fn_with_no_declared_return_type_hands_the_operator_unit'),
    ('M1667', 'PSV-1 (A102): a sealed frame calls an operator closure only with arguments at determined positions', _CC,
     '                if !ok {\n                    return panic(format!(\n                        "sealed code called an operator closure',
     '                if false && !ok {\n                    return panic(format!(\n                        "sealed code called an operator closure',
     'axon-core', _CL, _T72 + 'an_operator_closure_called_from_sealed_code_takes_only_determined_arguments'),
    ('M1668', 'PSV-1 (A103): a type parameter bound to a native handle admits only that handle', _CC,
     '            return if matches!(v, Value::Handle { .. }) && v.type_name() == h {',
     '            return if matches!(v, Value::Handle { .. }) || v.type_name() != h {',
     'axon-core', _CL, _T72 + 'a_handle_binding_admits_only_that_handle'),
    ('M1669', 'PSV-1 (A104, checker): a method call on () with no impl for () is refused (E0403)', _CK,
     '        Type::Unit => Some("()"),',
     '        Type::Unit => None,',
     'axon-core', '--no-default-features --test cli_run', 'a_method_call_on_unit_without_an_impl_is_e0403'),
    ('M1670', 'PSV-1 (A105, checker): an impl for f32/isize/usize, which never runs, is refused (E0505)', _CK,
     '                        self.errors.push(\n                            CheckError::new(\n                                E0505,',
     '                        drop(\n                            CheckError::new(\n                                E0505,',
     'axon-core', '--no-default-features --test cli_run', 'an_impl_for_a_type_the_runtime_represents_as_another_is_e0505'),
]
MUTATIONS += [
    ('M1671', 'PSV-1 (A109): a dict over the snapshot bound is refused at the crossing, never skipped', _CC, '        if len > DICT_SNAP_MAX {', '        if false && len > DICT_SNAP_MAX {', 'axon-core', _CL, _T72 + 'a_dict_over_the_snapshot_bound_is_refused_not_skipped'),
    ('M1672', 'PSV-1 (A106): a key the operator held may not come back with a value of another type', _CC, '                    if let Err(why) = self.replaced_ok(old, now, &mut seen, 0, true) {', '                    if let Err(why) = Ok::<(), String>(()) {', 'axon-core', _CL, _T72 + 'sealed_code_cannot_retype_a_dict_entry_the_operator_held'),
    ('M1673', "PSV-1 (A108): a candidate closure may not replace an operator closure in the operator's dict", _CC, '                return if !cand(oc) && cand(nc) {', '                return if false && !cand(oc) && cand(nc) {', 'axon-core', _CL, _T72 + 'a_dict_the_candidate_mutated_is_verified_at_every_edge_back'),
    ('M1674', "PSV-1 (A106): a mutation by sealed code marks the operator's dict for verification", _CC, '                s.dirty = true;', '                s.dirty = false;', 'axon-core', _CL, _T72 + 'sealed_code_cannot_retype_a_dict_entry_the_operator_held'),
    ('M1675', "PSV-1 (A106): the dicts in a candidate fn's arguments are snapshotted when the operator hands them over", _CI, '        if crossing {\n            for a in &args {\n                self.dict_edge_in(a)?;\n            }\n        }\n        let r = self.with_frame(callee', '        if false && crossing {\n            for a in &args {\n                self.dict_edge_in(a)?;\n            }\n        }\n        let r = self.with_frame(callee', 'axon-core', _CL, _T72 + 'sealed_code_cannot_retype_a_dict_entry_the_operator_held'),
    ('M1676', 'PSV-1 (A106): a candidate fn returning to operator code has its mutated dicts verified', _CI, '        if crossing && r.is_ok() {\n            self.dict_edge_out()?;', '        if false && crossing && r.is_ok() {\n            self.dict_edge_out()?;', 'axon-core', _CL, _T72 + 'sealed_code_cannot_retype_a_dict_entry_the_operator_held'),
    ('M1677', 'PSV-1 (A107): a dict nested in a handed dict is snapshotted too', _CC, '                out.push(m.clone());\n                for x in m.borrow().values() {\n                    self.walk_fresh(x, seen, out, d + 1)?;\n                }', '                out.push(m.clone());\n                for _x in m.borrow().values() {}', 'axon-core', _CL, _T72 + 'sealed_code_cannot_retype_a_dict_entry_the_operator_held'),
    ('M1678', 'PSV-1 (A107): a dict inside an Option/Result handed over is snapshotted', _CC, '            Value::Some(x) | Value::Ok(x) | Value::Err(x) => {\n                self.walk_fresh(x, seen, out, d + 1)?\n            }', '            Value::Some(_) | Value::Ok(_) | Value::Err(_) => {}', 'axon-core', _CL, _T72 + 'sealed_code_cannot_retype_a_dict_entry_the_operator_held'),
    ('M1679', 'PSV-1 (A107): a dict in a struct or enum field handed over is snapshotted', _CC, '            Value::Struct { fields, .. } | Value::Enum { fields, .. } => {\n                for x in fields.values() {\n                    self.walk_fresh(x, seen, out, d + 1)?;\n                }\n            }', '            Value::Struct { .. } | Value::Enum { .. } => {}', 'axon-core', _CL, _T72 + 'sealed_code_cannot_retype_a_dict_entry_the_operator_held'),
    ('M1680', 'PSV-1 (A107): a dict in an array or tuple handed over is snapshotted', _CC, '            Value::Array(_) | Value::Tuple(_) => {\n                let xs: &[Value] = match v {\n                    Value::Array(a) => a.as_slice(),\n                    Value::Tuple(t) => t.as_slice(),\n                    _ => unreachable!(),\n                };\n                for x in xs {\n                    self.walk_fresh(x, seen, out, d + 1)?;\n                }\n            }', '            Value::Array(_) | Value::Tuple(_) => {}', 'axon-core', _CL, _T72 + 'sealed_code_cannot_retype_a_dict_entry_the_operator_held'),
]
MUTATIONS += [
    ('M1681', 'PSV-1 (A106): a dict entry sealed code retyped is refused at the edge back (the verdict is acted on)', _CC, '            if let Some(why) = verdict {', '            if let Some(why) = verdict.filter(|_| false) {', 'axon-core', _CL, _T72 + 'sealed_code_cannot_retype_a_dict_entry_the_operator_held'),
]
MUTATIONS += [
    ('M1840', 'PSV-1 (A127): a dict that replaces a held dict is judged against the held entries (a key in both keeps its type)', _CC, '                    if let Some(nv) = nv {', '                    if let Some(nv) = nv.filter(|_| false) {', 'axon-core', _CL, _T72 + 'a_position_the_operator_held_is_judged_by_what_it_held_when_replaced'),
    ('M1841', 'PSV-1 (A128): an array or tuple that replaces a held one is judged element by element (a dict in it included)', _CC, '                for (x, y) in a.iter().zip(b) {\n                    self.replaced_ok(x, y, seen, d + 1, strict)?;\n                }', '                for (_x, _y) in a.iter().zip(b) {}', 'axon-core', _CL, _T72 + 'a_position_the_operator_held_is_judged_by_what_it_held_when_replaced'),
    ('M1842', 'PSV-1 (A128): a struct that replaces a held one is judged field by field (a dict in it included)', _CC, '            ) if n1 == n2 => {\n                for (k, x) in f1 {', '            ) if n1 == n2 && false => {\n                for (k, x) in f1 {', 'axon-core', _CL, _T72 + 'a_position_the_operator_held_is_judged_by_what_it_held_when_replaced'),
    ('M1843', 'PSV-1 (A128): an Option/Result that replaces a held one is judged through its payload (a dict in it included)', _CC, '            | (Value::Err(x), Value::Err(y)) => self.replaced_ok(x, y, seen, d + 1, strict)?,', '            | (Value::Err(x), Value::Err(y)) => {\n                let _ = (x, y);\n            }', 'axon-core', _CL, _T72 + 'a_position_the_operator_held_is_judged_by_what_it_held_when_replaced'),
    ('M1844', 'PSV-1 (A129): a store at a position the operator held UNDETERMINED (None, an empty array) is strict', _CC, 'self.cast(&mut c, &t, &Cx::default().strict(strict))', 'self.cast(&mut c, &t, &Cx::default())', 'axon-core', _CL, _T72 + 'a_placeholder_the_operator_held_is_not_filled_by_the_candidate'),
    ('M1845', 'PSV-1 (A129): a scalar held in a container may not come back as another type', _CC, '        if let Err(why) = self.cast(&mut c, &t, &Cx::default().strict(strict)) {', '        if let Err(why) = Ok::<(), String>(()) {', 'axon-core', _CL, _T72 + 'a_position_the_operator_held_is_judged_by_what_it_held_when_replaced'),
    ('M1848', 'PSV-1 (A129): a closure the operator held may not be replaced by a non-closure', _CC, '            (Value::Closure { .. }, _) => {', '            (Value::Closure { .. }, _) if false => {', 'axon-core', _CL, _T72 + 'a_dict_the_candidate_mutated_is_verified_at_every_edge_back'),
    ('M1847', 'PSV-1 (A130): a value nested past the cast bound is refused by the dict walk, never left unvisited', _CC, '    pub(crate) fn walk_depth_ok(d: usize) -> Result<(), Flow> {\n        if d > MAX_CAST_DEPTH {', '    pub(crate) fn walk_depth_ok(d: usize) -> Result<(), Flow> {\n        if false && d > MAX_CAST_DEPTH {', 'axon-core', _CL, 'interp::conform::walk_bound_tests::a_value_nested_past_the_bound_is_refused_not_left_unvisited'),
]
_PN = 'crates/axon-core/src/interp/pin.rs'
MUTATIONS += [
    ('M1990', 'PSV-1 (A145): operator code never dispatches an operator impl on a receiver nothing on the operator side determined', _CI, '        if !self.pins.determined(self.pin_fn.get(), receiver, &f.name) {', '        if false && !self.pins.determined(self.pin_fn.get(), receiver, &f.name) {', 'axon-core', _CL, _T72 + 'operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict'),
    ('M1991', 'PSV-1 (A145): the dispatch rule applies wherever two or more operator impl types define the method', _CI, '        if !self.pins.selects_between_impls(&f.name) {', '        if true {', 'axon-core', _CL, _T72 + 'operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict'),
    ('M1992', 'PSV-1 (A146): a call site whose receiver the analysis cannot determine is recorded as undetermined', _PN, '                if ctx.det(receiver, &local, &bound) {\n                    out.insert((owner, call_key(receiver, method)));', '                if true {\n                    out.insert((owner, call_key(receiver, method)));', 'axon-core', _CL, _T72 + 'operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict'),
    ('M1993', 'PSV-1 (A146): a builtin whose declared return names a type variable (dict_get) is an untyped read', _PN, '                        !builtin_ret_open(r)', '                        true', 'axon-core', _CL, _T72 + 'operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict'),
    ('M1994', 'PSV-1 (A146): an unannotated lambda parameter is undetermined', _PN, '                            _ => Fact::Unpinned,', '                            _ => Fact::Pinned,', 'axon-core', _CL, _T72 + 'operator_code_never_dispatches_on_a_value_from_any_untyped_position'),
    ('M1995', 'PSV-1 (A146): a match binding is determined only when its subject is', _PN, '                        facts.push((n, Fact::From(subject)));', '                        facts.push((n, Fact::Pinned));', 'axon-core', _CL, _T72 + 'operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict'),
    ('M1996', 'PSV-1 (A147): operator arithmetic on a fixed-width integer nothing determined is refused', _CI, '        if !self\n            .pins\n            .determined_arith(self.pin_fn.get(), op, left, right)\n        {', '        if false\n            && !self\n                .pins\n                .determined_arith(self.pin_fn.get(), op, left, right)\n        {', 'axon-core', _CL, _T72 + 'operator_arithmetic_never_runs_at_a_width_the_candidate_chose'),
    ('M1997', 'PSV-1 (A148): the first sealed mutation retakes the snapshot from what the operator holds now', _CC, '                if !s.dirty {', '                if false && !s.dirty {', 'axon-core', _CL, _T72 + 'a_closure_that_captured_the_operators_dict_cannot_retype_after_an_operator_write'),
]
MUTATIONS += [
    ('M2171', 'PSV-1 (A162): a type a CANDIDATE fn declares never determines a call (round 7 B1)', _PN, '                Item::FnDef(f) if sealed(f.span) => {', '                Item::FnDef(f) if false && sealed(f.span) => {', 'axon-core', _CL, 'interp::tests::' + 'a_type_the_candidate_declared_does_not_determine_the_receiver'),
    ('M2172', 'PSV-1 (A162): a type a sealed module defines is open, never an operator type (round 7 B1)', _PN, '            tys.open.insert(n.clone());\n            tys.structs.remove(n);\n            tys.enums.remove(n);\n            tys.refines.remove(n);', '            tys.refines.insert(n.clone());', 'axon-core', _CL, 'interp::tests::' + 'a_type_the_candidate_declared_does_not_determine_the_receiver'),
    ('M2173', 'PSV-1 (A163): a local binding named like an operator fn is not judged by that fn (round 7 B2)', _PN, '                    if bound.contains(name) || self.sealed_names.contains(name.as_str()) {', '                    if self.sealed_names.contains(name.as_str()) {', 'axon-core', _CL, 'interp::tests::' + 'a_local_binding_named_like_an_operator_fn_is_not_judged_by_it'),
    ('M2174', 'PSV-1 (A164): a trait name annotation does not pin the runtime type (round 7 B3)', _PN, '                    tys.open.insert(t.name.clone());', '                    tys.refines.insert(t.name.clone());', 'axon-core', _CL, 'interp::tests::' + 'a_trait_annotation_does_not_pin_the_runtime_type'),
    ('M2175', 'PSV-1 (A164): a dyn annotation does not pin the runtime type (round 7 B3)', _PN, '            T::DynTrait(_) => false,', '            T::DynTrait(_) => true,', 'axon-core', _CL, 'interp::tests::' + 'a_trait_annotation_does_not_pin_the_runtime_type'),
    ('M2176', 'PSV-1 (A165): a verdict belongs to the fn that owns the site (the key carries the owner)', _PN, '        self.determined\n            .contains(&(owner, call_key(receiver, method)))', '        self.determined\n            .contains(&(owner, call_key(receiver, method))) || self.determined.iter().any(|(_, k)| *k == call_key(receiver, method))', 'axon-core', _CL, 'interp::tests::' + 'a_candidates_global_and_a_sibling_fns_site_determine_nothing'),
    ('M2177', 'PSV-1 (A166): an operator method named like a channel method does not make a channel read determined', _PN, '                !CHAN_METHODS.contains(&method.as_str())\n                    && ', '                ', 'axon-core', _CL, 'interp::tests::' + 'an_operator_method_named_like_a_channel_method_does_not_determine_a_recv'),
    ('M2178', 'PSV-1 (A162): a candidate module-level let determines nothing', _PN, '                Item::LetDef { name, value, span } if !sealed(*span) => lets.push((name, value)),', '                Item::LetDef { name, value, .. } => lets.push((name, value)),', 'axon-core', _CL, 'interp::tests::' + 'a_candidates_global_and_a_sibling_fns_site_determine_nothing'),
]
PSV_IDS |= {f"M{n}" for n in range(1660, 2220)}
# ── end r4c-psv1 ──

# ── C9 round 4c, workstream SHARDFLAKE (amendment 77; M1830-M1849) ───────────
# A sharded suite relinks a test binary under running siblings; the verifier's
# identity was the digest of the file at `current_exe()`'s PATH, which then
# names "... (deleted)", so it read "unknown" and a certification could not
# bind it. The identity is now the digest of the image the process RUNS
# (`/proc/self/exe`). M1830 puts the path back; the test unlinks a copy of its
# own binary and asks for the identity.
PSV_IDS |= {f"M{n}" for n in range(1830, 1850)}
MUTATIONS += [
    ('M1830', "PSV-7 (shardflake): the verifier's identity is the image it RUNS, not the path it started from",
     'crates/axon-fabric/src/readiness.rs',
     '    let bytes = std::fs::read(running_image()).ok()?;',
     '    let bytes = std::fs::read(std::env::current_exe().ok()?).ok()?;',
     'axon-fabric', '--test verifier_identity_replaced', 'the_identity_survives_replacement_of_the_executable_file'),
]


# ── C9 round 5, workstream OBSBIND (M1850-M1879; amendment 79; matrix A131-A134).
# The reviewer's PSV-6 finding: the observer countersigned Fabric-authored values
# (Fabric revision, init and axon digests, epoch, policy) and its `verifier_sha256`
# was whatever Fabric-uid program called. Now: the helper config PINS the Fabric
# program (path, sha256, build revision), the helper serves and the observer names
# only it; the guest's init and axon digests are what the measured profile
# manifest names; the observer observes only a nonce the custodian issued for that
# epoch; the custodian and the observer's stores are bounded.
PSV_IDS |= {f"M{n}" for n in range(1850, 1880)}
_PLT = '--lib'
_PLS = 'crates/axon-fabric/src/privileged_launcher.rs'
_CUS = 'crates/axon-fabric/src/custodian.rs'
_NS = 'crates/axon-fabric/src/observer.rs'
MUTATIONS += [
    # A131: what the observer signs is measured or the operator's, not told.
    ('M1850', "A131 (obsbind): fabric_revision is the pinned program's build revision, not told", _OS,
     '                text(lv, "/fabric/revision")?,\n', '                m.fabric_revision.clone(),\n',
     'axon-fabric', _TO, _TOM),
    ('M1851', "A131 (obsbind): guest.axon_sha256 is what the measured profile manifest names, not told", _OS,
     '            ("guest.axon_sha256", named("axon")?, &m.guest.axon_sha256),\n',
     '            ("guest.axon_sha256", m.guest.axon_sha256.clone(), &m.guest.axon_sha256),\n',
     'axon-fabric', _TO, _TOM),
    ('M1852', "A131 (obsbind): guest.init_sha256 is what the measured profile manifest names, not told", _OS,
     '                named("axon-guest-init")?,\n', '                m.guest.init_sha256.clone(),\n',
     'axon-fabric', _TO, _TOM),
    ('M1853', "A131 (obsbind): the observer names only the Fabric program the operator pinned (the caller the helper measured must BE the pin)", _OS,
     '        if caller_sha256 != pinned {\n', '        if false && caller_sha256 != pinned {\n',
     'axon-fabric', _TO, 'an_observer_names_only_the_fabric_program_the_operator_pinned'),
    # A132: the helper serves only the pinned Fabric program.
    ('M1854', "A132 (obsbind): a helper config must pin the Fabric program (a lowercase sha256)", _PLS,
     '    if !is_hex64(&c.fabric.sha256) {\n', '    if false && !is_hex64(&c.fabric.sha256) {\n',
     'axon-fabric', _PLT, 'privileged_launcher::tests::a_helper_config_must_pin_the_fabric_program'),
    ('M1855', "A132 (obsbind): a helper config's Fabric revision is a build revision", _PLS,
     '    if !revision_ok(&c.fabric.revision, a.test) {\n', '    if false && !revision_ok(&c.fabric.revision, a.test) {\n',
     'axon-fabric', _PLT, 'privileged_launcher::tests::a_helper_config_must_pin_the_fabric_program'),
    ('M1856', "A132 (obsbind): on a production host that revision is the 40 hex of a commit", _PLS,
     "        rev.len() == 40 && rev.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))\n",
     '        !rev.is_empty()\n',
     'axon-fabric', _PLT, 'privileged_launcher::tests::a_helper_config_must_pin_the_fabric_program'),
    ('M1857', "A132 (obsbind): the helper serves a launch or a relay only for the Fabric program the operator pinned", _PLS,
     '    if running != c.fabric.sha256 {\n', '    if false && running != c.fabric.sha256 {\n',
     'axon-fabric', '--test privileged_launcher', 'a_helper_launches_only_for_the_fabric_program_the_operator_pinned'),
    # A133: the observer observes only a nonce the custodian issued, for that epoch.
    ('M1858', "A133 (obsbind): the observer observes only a nonce the custodian issued", _OS,
     '        let (cmode, expires) = custodian\n            .check(&m.observation_nonce, m.authority.epoch)\n            .map_err(|e| format!("the custodian does not honour this nonce: {e}"))?;\n',
     '        let (cmode, expires) = (Mode::Test, i64::MAX);\n',
     'axon-fabric', _TO, 'an_observer_signs_nothing_for_a_nonce_the_custodian_never_issued'),
    ('M1859', "A133 (obsbind): ... for the epoch the manifest names", _OS,
     '            .check(&m.observation_nonce, m.authority.epoch)\n', '            .check(&m.observation_nonce, 0)\n',
     'axon-fabric', _TO, 'an_observer_signs_nothing_for_a_nonce_issued_for_another_epoch'),
    ('M1860', "A133 (obsbind): the custodian answers `check` only for the observer's uid", _CUS,
     '                if self.cfg.observer_uid != Some(peer) {\n', '                if false && self.cfg.observer_uid != Some(peer) {\n',
     'axon-fabric', _PLT, 'custodian::tests::only_the_observer_checks_a_nonce'),
    ('M1861', "A133 (obsbind): a protected custodian config names the observer's own uid", _CUS,
     '            _ => {\n                return Err(format!(\n                    "observer_uid {:?} must name', '            _ => {\n                return Ok(());\n                #[allow(unreachable_code)]\n                return Err(format!(\n                    "observer_uid {:?} must name',
     'axon-fabric', _PLT, 'custodian::tests::a_protected_custodian_names_the_observer_as_its_own_uid'),
    ('M1862', "A133 (obsbind): the observer takes no nonce from a custodian weaker than itself (call site)", _OS,
     '        if !custodian_admissible(self.mode, cmode) {\n', '        if false && !custodian_admissible(self.mode, cmode) {\n',
     'axon-fabric', _TO, 'an_observer_takes_no_nonce_from_a_dev_custodian'),
    ('M1863', "A133 (obsbind): ... and the rule itself (a protected observer takes only a protected custodian's nonce)", _OS,
     '    crate::privileged_launcher::custodian_mode_launches(custodian, observer != Mode::Protected)\n', '    true\n',
     'axon-fabric', _PLT, 'observer_service::tests::a_protected_observer_takes_only_a_protected_custodians_nonce'),
    # A134: availability bounds, uid separation, deadlines.
    ('M1864', "A134 (obsbind): the observer drops the record of a nonce the custodian no longer honours", _OS,
     '            if expires.is_none_or(|t| t < now) {\n', '            if false && expires.is_none_or(|t| t < now) {\n',
     'axon-fabric', _TO, 'an_observer_drops_the_records_of_nonces_the_custodian_no_longer_honours'),
    ('M1865', "A134 (obsbind): the custodian drops the records of nonces past their max age", _NS,
     '            if now - at > max_age_s as i64 {\n', '            if false && now - at > max_age_s as i64 {\n',
     'axon-fabric', _PLT, 'custodian::tests::expired_nonce_records_do_not_accumulate'),
    ('M1866', "A134 (obsbind): the custodian holds at most MAX_OUTSTANDING nonces", _NS,
     '        if outstanding >= max_outstanding {\n', '        if false && outstanding >= max_outstanding {\n',
     'axon-fabric', _PLT, 'custodian::tests::the_custodian_bounds_the_nonces_it_holds_outstanding'),
    ('M1867', "A134 (obsbind): one connection has an absolute deadline for its request", _OS,
     '                    let _ = s.set_read_timeout(Some(left));\n', '                    let _ = s.set_read_timeout(Some(IO_TIMEOUT));\n',
     'axon-fabric', _PLT, 'observer_service::tests::a_connection_that_does_not_finish_its_request_is_cut_off'),
    ('M1868', "A134 (obsbind): the observer service and the custodian are two uids (helper config)", _PLS,
     '        if !a.test && s.uid == c.custodian.uid {\n', '        if false {\n',
     'axon-fabric', _PLT, 'privileged_launcher::tests::a_helper_config_whose_observer_is_the_custodian_is_refused'),
    ('M1869', "A134 (obsbind): the observer's reply timeout grows with what it must hash", _OS,
     '    IO_TIMEOUT + Duration::from_secs(artifact_bytes / MIN_HASH_RATE)\n', '    IO_TIMEOUT\n',
     'axon-fabric', _PLT, 'privileged_launcher::tests::an_observation_waits_as_long_as_its_measurement_can_take'),
    ('M1870', "A134 (obsbind): the helper sizes the artifacts the observer must hash", _PLS,
     '        .map(|m| m.len())\n        .sum()\n', '        .map(|_| 0u64)\n        .sum()\n',
     'axon-fabric', _PLT, 'privileged_launcher::tests::the_helper_sizes_what_the_observer_must_hash'),
]


# ── C9 round 4c, workstream BUILDENV (amendment 80; M1880-M1899) ──────────────
# FIELD-ORIGIN (round 5): the guest build judged cargo's effective config by
# splitting the text cargo PRINTS, so a single-quoted `cfg(all(..="..."))`
# target (cargo's own spelling for a key holding double quotes) read as a
# harmless triple and a committed linker linked the guest binaries. The judge is
# now the structured key path against the tree's committed keys. Plus the
# host binaries' build identity (build.rs) and the builder's proof on the build
# records. Each row is killed by the committed-config / record form it lets
# through (`ATTACK:` panics of tests/guest_build_env.rs, freeze_manifest.rs,
# build_state.rs).
PSV_IDS |= {f"M{n}" for n in range(1880, 1900)}
_BE = '--test guest_build_env'
_BS = 'crates/axon-fabric/src/build_state.rs'
_RP = 'a_build_record_its_runner_did_not_sign_is_refused'
_CFG = 'a_config_key_is_judged_by_its_structured_path_not_by_the_text_cargo_prints'
_HOST = 'the_host_build_configuration_is_judged_in_a_constructed_environment'
_DIST = 'a_dist_file_the_controlled_runner_did_not_produce_is_refused'
_FSG = 'a_guest_build_record_its_runner_did_not_sign_does_not_freeze'
_PRODB = 'a_production_build_under_a_wrapper_rustflags_or_linker_does_not_happen'
MUTATIONS += [
    ('M1880', 'FIELD-ORIGIN (5): a committed cargo key is tolerated only if it is exactly one of the tree\'s committed keys', _GE,
     '    if path not in COMMITTED_KEYS:\n        return "a setting the guest build would use"\n',
     '    if path not in COMMITTED_KEYS:\n        return None\n',
     'axon-fabric', _BE, 'a_committed_cargo_config_cannot_name_a_program_in_any_spelling'),
    ('M1881', 'FIELD-ORIGIN (5): a tolerated key never names a cfg(...) table or a triple the build compiles for', _GE,
     '    if path[1].startswith("cfg(") or path[1] in triples:\n',
     '    if False:\n',
     'axon-fabric', _BE, _CFG),
    ('M1882', 'FIELD-ORIGIN (5): a variable that names a compiler, wrapper, flags or linker in the build environment is refused', _GE,
     '    foreign = [f"environment variable {n} is set: it could stand between the sources and the bytes"\n               for n in sorted(env) if not env_ok(n)]\n',
     '    foreign = []\n',
     'axon-fabric', _BE, _CFG),
    ('M1883', 'FIELD-ORIGIN (5): the host triple is the one `rustc -vV` reports, not a constant', _GE,
     '            return line[len("host: "):].strip() or None\n',
     '            return "x86_64-unknown-linux-gnu"\n',
     'axon-fabric', _BE, _CFG),
    ('M1884', 'FIELD-ORIGIN (5, retargeted in round 6): the host build\'s config check applies the effective-config classifier, in a constructed environment', _GE,
     '        return effective_config(cargo, env, clone)[1]\n', '        return []\n',
     'axon-fabric', _BE, _HOST),
    ('M1885', 'FIELD-ORIGIN (5): a cargo that cannot print its config refuses the build (never reads as no config)', _GE,
     '    if not isinstance(tree, dict):\n        return [], sorted(set(foreign + [f"cargo config get --format json failed: {j.stderr.strip()[-300:]}"]))\n',
     '    if not isinstance(tree, dict):\n        tree = {}\n',
     'axon-fabric', _BE, _CFG),
    ('M1886', 'BUILD-RECORD (5): a record edited after its runner signed it fails the proof', _GE,
     '    if not hmac.compare_digest(hmac.new(key, proof_payload(rec), "sha256").hexdigest(), pr["hmac"]):\n',
     '    if False and not hmac.compare_digest(hmac.new(key, proof_payload(rec), "sha256").hexdigest(), pr["hmac"]):\n',
     'axon-fabric', _BE, _RP),
    ('M1887', 'BUILD-RECORD (5): the proof key is the builder\'s alone: a key other uids can read is no key', _GE,
     '        if (not stat.S_ISDIR(dst.st_mode) or dst.st_uid != builder_uid or dst.st_mode & 0o077\n                or not stat.S_ISREG(st.st_mode) or st.st_uid != builder_uid or st.st_mode & 0o177):\n',
     '        if (not stat.S_ISDIR(dst.st_mode) or dst.st_uid != builder_uid or dst.st_mode & 0o077\n                or not stat.S_ISREG(st.st_mode) or st.st_uid != builder_uid):\n',
     'axon-fabric', _BE, _RP),
    ('M1888', 'BUILD-RECORD (5): the proof key is read without following a symlink', _GE,
     '        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)\n',
     '        fd = os.open(path, os.O_RDONLY)\n',
     'axon-fabric', _BE, _RP),
    ('M1889', 'BUILD-RECORD (5): the proof key\'s directory is closed to other uids', _GE,
     '        if (not stat.S_ISDIR(dst.st_mode) or dst.st_uid != builder_uid or dst.st_mode & 0o077\n',
     '        if (not stat.S_ISDIR(dst.st_mode) or dst.st_uid != builder_uid\n',
     'axon-fabric', _BE, _RP),
    ('M1890', 'BUILD-RECORD (5): a key in a build parent another uid can write proves nothing', _GE,
     '    _anc, why = ancestors_of(parent, builder_uid) if judging else ([], "")\n    if why:\n',
     '    _anc, why = ancestors_of(parent, builder_uid) if judging else ([], "")\n    if False:\n',
     'axon-fabric', _BE, _RP),
    ('M1891', 'BUILD-RECORD (5): `dist` refuses a binary that is not the controlled step\'s output', _GE,
     '        if got != want:\n            fail(f"dist/{name} ({got}) is not the bytes the controlled build produced ({want})")\n',
     '        if False:\n            fail(f"dist/{name} ({got}) is not the bytes the controlled build produced ({want})")\n',
     'axon-fabric', _BE, _DIST),
    ('M1892', 'BUILD-RECORD (5): the manifest refuses a dist file that differs from its record\'s digest', _GE,
     '        if not os.path.isfile(p) or sha256(p) != digest:\n',
     '        if not os.path.isfile(p):\n',
     'axon-fabric', _BE, _DIST),
    ('M1893', 'BUILD-RECORD (5): the manifest takes dist digests only from a record carrying its runner\'s proof', _GE,
     '    why = proof_problems(benv, "build", builder)\n    if why:\n        return why\n    want = dict(benv.get("dist") or {})\n',
     '    want = dict(benv.get("dist") or {})\n',
     'axon-fabric', _BE, _DIST),
    ('M1894', 'BUILD-RECORD (5): the freeze refuses a build record its runner did not sign', _GE,
     '    return (proof_problems(rec, "build", builder)\n            or proof_problems((man.get("kernel") or {}).get("build_environment"), "kernel build", builder))\n',
     '    return proof_problems((man.get("kernel") or {}).get("build_environment"), "kernel build", builder)\n',
     'axon-fabric', '--test freeze_manifest', _FSG),
    ('M1895', 'BUILD-RECORD (5): the freeze refuses a kernel build record its runner did not sign', _GE,
     '    return (proof_problems(rec, "build", builder)\n            or proof_problems((man.get("kernel") or {}).get("build_environment"), "kernel build", builder))\n',
     '    return proof_problems(rec, "build", builder)\n',
     'axon-fabric', '--test freeze_manifest', _FSG),
    ('M1896', 'BUILD-RECORD (5): the proof covers every field of the record', _GE,
     '    body = {k: v for k, v in rec.items() if k != "proof"}\n',
     '    body = {k: v for k, v in rec.items() if k not in ("proof", "artifacts")}\n',
     'axon-fabric', _BE, _RP),
    ('M1897', 'HOST-BUILD (5): a production build under a wrapper, rustflags or linker is refused', _BS,
     '    if state.is_empty() || !production {\n        return None;\n',
     '    if true {\n        return None;\n',
     'axon-fabric', '--test build_state', _PRODB),
    ('M1898', 'HOST-BUILD (5): a workspace wrapper counts as a wrapper', _BS,
     '    "RUSTC_WORKSPACE_WRAPPER",\n    "CARGO_ENCODED_RUSTFLAGS",\n',
     '    "RUSTC_WRAPPER",\n    "CARGO_ENCODED_RUSTFLAGS",\n',
     'axon-fabric', '--test build_state', _PRODB),
    ('M1899', 'HOST-BUILD (5): build.rs stops a refused build', 'crates/axon-fabric/build.rs',
     '        eprintln!("error: {why}");\n        std::process::exit(1);\n',
     '        eprintln!("error: {why}");\n',
     'axon-fabric', '--test build_state', _PRODB),
]
# ── end buildenv ──


# ── C9 round 4c, workstream EQGATE (amendment 81; M1900-M1959) ───────────────
# --merge never compared the toolchain across shards (only --join did); the
# comparison is now ONE helper (shard_toolchain_problem) both call. Each row
# below removes one arm of it or one call of it.
_GM = 'scripts/v022_g01_mutations.py'
PSV_IDS |= {f"M{n}" for n in range(1900, 1960)}
MUTATIONS += [
    ('M1900', "EQUIVALENCE (eqgate): --merge refuses shards run on different toolchains", _GM,
     '    if why:\n        sys.exit("refused: " + why)\n    got = sorted(d["shard"]["index"]',
     '    if False:\n        sys.exit("refused: " + why)\n    got = sorted(d["shard"]["index"]',
     'axon-core', _HI2, 'a_merge_refuses_shards_from_two_toolchains'),
    ('M1901', "EQUIVALENCE (eqgate): the shared helper refuses a second toolchain (--join side)", _GM,
     '    if len(groups) > 1:\n        return "records ran on different toolchains: "',
     '    if len(groups) > 99:\n        return "records ran on different toolchains: "',
     'axon-core', _HI2, 'a_join_refuses_records_from_two_toolchains'),
    ('M1902', "EQUIVALENCE (eqgate): a shard that does not record its host is no evidence", _GM,
     '        if not isinstance(host, dict) or not isinstance(host.get("toolchain"), dict):\n            return f"record {label}',
     '        if False:\n            return f"record {label}',
     'axon-core', _HI2, 'a_merge_refuses_a_shard_that_does_not_record_its_host'),
    ('M1903', "EQUIVALENCE (eqgate): --merge pins the interpreter binary's digest across shards", _GM,
     '         {"axon_bin_sha256": (d.get("toolchain") or {}).get("axon_bin_sha256", "unrecorded"),\n',
     '         {"axon_bin_sha256": None,\n',
     'axon-core', _HI2, 'a_merge_refuses_shards_run_on_two_interpreters'),
    ('M1904', "EQUIVALENCE (eqgate): --merge pins the uid the shards ran under", _GM,
     '          "euid": (d.get("environment") or {}).get("euid"),\n',
     '          "euid": None,\n',
     'axon-core', _HI2, 'a_merge_refuses_shards_run_under_different_uids'),
    ('M1905', "EQUIVALENCE (eqgate): a partial --only write recomputes a kept record's HOLDS from its cells", _PDH,
     '        if bool(r.get("holds")) != want and not (r.get("status") == "STALE_REFACTORED" and not r.get("holds")):\n            return (f"kept record',
     '        if False:\n            return (f"kept record',
     'axon-core', _HI2, 'a_partial_paired_disable_write_judges_its_kept_records'),
    ('M1906', "EQUIVALENCE (eqgate): a partial --only write compares the kept records' toolchain", _PDH,
     '    if why:\n        return why + "; re-execute the kept records',
     '    if False:\n        return why + "; re-execute the kept records',
     'axon-core', _HI2, 'a_partial_paired_disable_write_judges_its_kept_records'),
]


# ── C9 round 4c, workstream EQGATE part 2 (amendment 81): guards expressed as an
# OPEN FLAG, a decision returned as Some(reason), a status returned as a
# Result<bool>/i32/ExitCode, and the gate forms that now see them. Each row is
# killed by a test whose attack is the shape the guard defeats (a symlink or an
# existing file for a flag; a wrong exclusion/promotion/verdict for the rest).
MUTATIONS += [
    ('M1907', "OPEN FLAG (eqgate): the ownership walk's base is opened without following a symlink", 'crates/axon-fabric/src/privileged_launcher.rs',
     '        self.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)',
     '        self.custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)',
     'axon-fabric', '--lib', 'privileged_launcher::tests::a_walk_base_that_is_a_symlink_is_never_followed'),
    ('M1908', "OPEN FLAG (eqgate): the root helper's input snapshot never follows a symlink", 'crates/axon-fabric/src/privileged_launcher.rs',
     '            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,\n        )\n        .map_err(|e| {\n            if e.raw_os_error() == Some(libc::ELOOP) {\n                format!(\n                    "{} is a symlink: an input',
     '            libc::O_RDONLY | libc::O_NONBLOCK,\n        )\n        .map_err(|e| {\n            if e.raw_os_error() == Some(libc::ELOOP) {\n                format!(\n                    "{} is a symlink: an input',
     'axon-fabric', '--lib', 'privileged_launcher::tests::the_inputs_snapshot_never_follows_a_symlink'),
    ('M1909', 'OPEN FLAG (eqgate): every file of the input snapshot is created new, never written through', 'crates/axon-fabric/src/privileged_launcher.rs',
     '                    .create_new(true)\n                    .mode(mode)',
     '                    .create(true)\n                    .mode(mode)',
     'axon-fabric', '--lib', 'privileged_launcher::tests::the_inputs_snapshot_never_writes_over_an_existing_file'),
    ('M1910', 'OPEN FLAG (eqgate): the policy snapshot never follows a symlink', 'crates/axon-fabric/src/privileged_launcher.rs',
     '        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,\n    )\n    .map_err(|e| {\n        if e.raw_os_error() == Some(libc::ELOOP) {\n            "the psv policy',
     '        libc::O_RDONLY | libc::O_NONBLOCK,\n    )\n    .map_err(|e| {\n        if e.raw_os_error() == Some(libc::ELOOP) {\n            "the psv policy',
     'axon-fabric', '--lib', 'privileged_launcher::tests::the_policy_snapshot_never_follows_a_symlink'),
    ('M1911', 'OPEN FLAG (eqgate): an operator file is read without following a symlink at its leaf', 'crates/axon-fabric/src/privileged_launcher.rs',
     '        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,\n    )\n    .map_err(|e| bad(e.to_string()))?;',
     '        libc::O_RDONLY | libc::O_NONBLOCK,\n    )\n    .map_err(|e| bad(e.to_string()))?;',
     'axon-fabric', '--lib', 'privileged_launcher::tests::an_operator_file_that_is_a_symlink_is_never_read'),
    ('M1912', 'OPEN FLAG (eqgate): the out-dir hand-over classifies an entry without following it', 'crates/axon-fabric/src/privileged_launcher.rs',
     'libc::fstatat(dir, cn.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW)',
     'libc::fstatat(dir, cn.as_ptr(), &mut st, 0)',
     'axon-fabric', '--lib', 'privileged_launcher::tests::the_hand_over_never_follows_a_symlink'),
    ('M1913', 'OPEN FLAG (eqgate): the out-dir hand-over chowns a symlink itself, never its target', 'crates/axon-fabric/src/privileged_launcher.rs',
     'libc::fchownat(dir, cn.as_ptr(), uid, u32::MAX, libc::AT_SYMLINK_NOFOLLOW) == 0',
     'libc::fchownat(dir, cn.as_ptr(), uid, u32::MAX, 0) == 0',
     'axon-fabric', '--lib', 'privileged_launcher::tests::the_hand_over_never_follows_a_symlink'),
    ('M1914', "OPEN FLAG (eqgate): the observer's signing key is read through one O_NOFOLLOW open", 'crates/axon-fabric/src/observer_service.rs',
     '        .read(true)\n        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)\n        .open(path)',
     '        .read(true)\n        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)\n        .open(path)',
     'axon-fabric', '--lib', 'observer_service::tests::the_observer_key_is_never_read_through_a_symlink'),
    ('M1915', 'OPEN FLAG (eqgate): the installed kernel/rootfs is measured through one O_NOFOLLOW open', 'crates/axon-fabric/src/observer_service.rs',
     '        .read(true)\n        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)\n        .open(p)',
     '        .read(true)\n        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)\n        .open(p)',
     'axon-fabric', '--lib', 'observer_service::tests::an_installed_artifact_that_is_a_symlink_is_never_measured'),
    ('M1916', 'OPEN FLAG (eqgate): a published workspace file is renamed in without replacing (RENAME_NOREPLACE)', 'crates/axon-fabric/src/workspace.rs',
     '            libc::RENAME_NOREPLACE,',
     '            0,',
     'axon-fabric', '--lib', 'workspace::open_flag_tests::a_published_file_is_never_replaced_by_other_bytes'),
    ('M1917', 'OPEN FLAG (eqgate): a store write is a create (create_new) at its temporary name', 'crates/axon-loop/src/store.rs',
     '            let mut f = nofollow()\n                .create_new(true)',
     '            let mut f = nofollow()\n                .create(true)',
     'axon-loop', '--test open_flag_sites', 'a_store_write_never_writes_through_a_file_planted_at_its_temporary_name'),
    ('M1918', 'OPEN FLAG (eqgate): prepare writes the guest policy exclusively (create_new)', 'crates/axon-fabric/src/psv.rs',
     '        .create_new(true)\n        .open(i.job_dir.with_file_name("policy.json"))',
     '        .create(true)\n        .open(i.job_dir.with_file_name("policy.json"))',
     'axon-fabric', '--test one_read', 'prepare_never_writes_the_policy_over_a_file_already_there'),
    ('M1919', 'OPEN FLAG (eqgate): keygen creates the key file (create_new), never overwrites one', 'crates/axon-fabric/src/bin/axon-fabric.rs',
     '            .create_new(true)\n            .mode(0o400)\n            .open(&out)',
     '            .create(true)\n            .mode(0o400)\n            .open(&out)',
     'axon-fabric', '--test attestation', 'keygen_never_overwrites_an_existing_key_file'),
    ('M1920', 'SOME(REASON) (eqgate): a trusted verifier holds another loop role', 'crates/axon-loop/src/admission.rs',
     '    if config.verifiers().contains(who) {\n        Some(',
     '    if false && config.verifiers().contains(who) {\n        Some(',
     'axon-loop', '--lib', 'admission::role_arms::a_trusted_verifier_is_named_as_one'),
    ('M1921', 'SOME(REASON) (eqgate): a context observer holds another loop role', 'crates/axon-loop/src/admission.rs',
     '    } else if config.observers().contains(who) {',
     '    } else if false && config.observers().contains(who) {',
     'axon-loop', '--lib', 'admission::role_arms::a_context_observer_is_named_as_one'),
    ('M1922', 'SOME(REASON) (eqgate): a safety monitor holds another loop role', 'crates/axon-loop/src/admission.rs',
     '    } else if config.trusted_monitors.contains(who) {',
     '    } else if false && config.trusted_monitors.contains(who) {',
     'axon-loop', '--lib', 'admission::role_arms::a_safety_monitor_is_named_as_one'),
    ('M1923', 'SOME(REASON) (eqgate): the evaluator cannot issue the promotion it evaluated', 'crates/axon-loop/src/admission.rs',
     '    } else if &adm.evaluator_ref == who {',
     '    } else if false && &adm.evaluator_ref == who {',
     'axon-loop', '--test pointer', 'no_proposer_evaluator_or_subject_issues_a_promotion'),
    ('M1924', 'SOME(REASON) (eqgate): a subject issuer cannot issue the promotion', 'crates/axon-loop/src/admission.rs',
     '    } else if eval.subject_issuers.contains(who) {',
     '    } else if false && eval.subject_issuers.contains(who) {',
     'axon-loop', '--test pointer', 'no_proposer_evaluator_or_subject_issues_a_promotion'),
    ('M1925', 'SOME(REASON) (eqgate): a candidate with an unsafe attempt is vetoed', 'crates/axon-loop/src/admission.rs',
     '            crate::safety::SafetyState::Violation { code } => Some(format!(',
     '            crate::safety::SafetyState::Violation { code } if { let _ = &code; false } => Some(format!(',
     'axon-loop', '--test admit_verdict_sites', 'a_candidate_with_an_unsafe_attempt_is_never_accepted'),
    ('M1926', 'RESULT<BOOL> (eqgate): only discovery/tuning episodes feed learning (Confirmation, Reporting)', 'crates/axon-loop-contracts/src/checks.rs',
     'matches!(episode.corpus_role, Discovery | Tuning)',
     'matches!(episode.corpus_role, Discovery | Tuning | Confirmation | Reporting)',
     'axon-loop-contracts', '--test learning_eligible_conjuncts', 'only_discovery_and_tuning_episodes_feed_learning'),
    ('M1927', 'RESULT<BOOL> (eqgate): a mechanism_test episode never feeds learning', 'crates/axon-loop-contracts/src/checks.rs',
     'matches!(episode.corpus_role, Discovery | Tuning)',
     'matches!(episode.corpus_role, Discovery | Tuning | MechanismTest)',
     'axon-loop-contracts', '--test learning_eligible_conjuncts', 'a_mechanism_test_episode_never_feeds_learning'),
    ('M1928', 'RESULT<BOOL> (eqgate): only a PASSED verification feeds learning', 'crates/axon-loop-contracts/src/checks.rs',
     '        && episode.verification.result == VerificationResult::Passed\n',
     '        && true\n',
     'axon-loop-contracts', '--test learning_eligible_conjuncts', 'an_episode_that_did_not_pass_verification_never_feeds_learning'),
    ('M1929', 'RESULT<BOOL> (eqgate): only FINAL usage feeds learning', 'crates/axon-loop-contracts/src/checks.rs',
     '        && episode.usage.state == UsageState::Final)',
     '        && true)',
     'axon-loop-contracts', '--test learning_eligible_conjuncts', 'an_episode_with_estimated_usage_never_feeds_learning'),
    ('M1930', 'SOME(REASON) (eqgate): evo excludes a mechanism_test episode under its own reason', 'crates/axon-loop/src/evo.rs',
     '        let reason = if ep.corpus_role == CorpusRole::MechanismTest {',
     '        let reason = if false && ep.corpus_role == CorpusRole::MechanismTest {',
     'axon-loop', '--test evo_learning_arms', 'a_mechanism_test_episode_is_excluded_as_one'),
    ('M1931', 'SOME(REASON) (eqgate): evo excludes an episode that is not learning-eligible', 'crates/axon-loop/src/evo.rs',
     '        } else if !learning_eligible(&ep)? {',
     '        } else if false && !learning_eligible(&ep)? {',
     'axon-loop', '--test evo_learning_arms', 'an_episode_that_is_not_learning_eligible_is_excluded_as_such'),
    ('M1932', 'SOME(REASON) (eqgate): evo excludes an episode the proposer verified itself', 'crates/axon-loop/src/evo.rs',
     '        } else if issuer == Some(&req.proposer_ref) {',
     '        } else if false && issuer == Some(&req.proposer_ref) {',
     'axon-loop', '--test evo_learning_arms', 'an_episode_the_proposer_verified_itself_is_excluded_even_if_it_is_a_trusted_verifier'),
    ('M1933', 'SOME(REASON) (eqgate): evo excludes an episode whose verifier is not trusted', 'crates/axon-loop/src/evo.rs',
     '        } else if !issuer.is_some_and(|i| verifiers.contains(i)) {',
     '        } else if false && !issuer.is_some_and(|i| verifiers.contains(i)) {',
     'axon-loop', '--test evo_learning_arms', 'an_episode_whose_verifier_is_not_trusted_is_excluded'),
    ('M1934', 'SOME(REASON) (eqgate): a check suite that changed during the run yields no verdict', 'crates/axon-fabric/src/submit.rs',
     '            Ok(tr) => {\n                problem = Some(format!(\n                    "check suite `{}` changed',
     '            Ok(tr) if tr.reference() != s.version => {}\n            Ok(tr) => {\n                problem = Some(format!(\n                    "check suite `{}` changed',
     'axon-fabric', '--test check_effects', 'a_suite_that_changed_during_the_run_yields_no_verdict'),
    ('M1935', 'SOME(REASON) (eqgate): a check suite unreadable after the run yields no verdict', 'crates/axon-fabric/src/submit.rs',
     '            Err(e) => {\n                problem = Some(format!(\n                    "check suite `{}` unreadable',
     '            Err(_) => {}\n            Err(e) => {\n                problem = Some(format!(\n                    "check suite `{}` unreadable',
     'axon-fabric', '--test check_effects', 'a_suite_that_cannot_be_read_after_the_run_yields_no_verdict'),
    ('M1936', 'SOME(REASON) (eqgate): `axon test` fails a should_fail property that held', 'crates/axon-core/src/main.rs',
     '                    PropertyOutcome::Passed { cases } if *should_fail => (\n                        false,',
     '                    PropertyOutcome::Passed { cases } if *should_fail => (\n                        true,',
     'axon-core', '--no-default-features --test axon_test_outcomes', 'a_should_fail_property_that_holds_is_a_failure'),
    ('M1937', 'SOME(REASON) (eqgate): `axon test` fails a property that failed', 'crates/axon-core/src/main.rs',
     '                    PropertyOutcome::Failed { counterexample, message, seed } => (\n                        false,',
     '                    PropertyOutcome::Failed { counterexample, message, seed } => (\n                        true,',
     'axon-core', '--no-default-features --test axon_test_outcomes', 'a_property_that_fails_is_a_failure'),
    ('M1938', 'SOME(REASON) (eqgate): `axon test` fails a should_fail test that did not panic', 'crates/axon-core/src/main.rs',
     '                    Ok(_) if *should_fail => (\n                        false,',
     '                    Ok(_) if *should_fail => (\n                        true,',
     'axon-core', '--no-default-features --test axon_test_outcomes', 'a_should_fail_test_that_does_not_panic_is_a_failure'),
    ('M1939', 'RESULT<BOOL> (eqgate): create_once never replaces an existing branch record', 'crates/axon-fabric/src/branches.rs',
     '    let r = std::fs::hard_link(&tmp, dest);',
     '    let r = std::fs::rename(&tmp, dest).map(|_| ());',
     'axon-fabric', '--lib', 'branches::create_once_tests::create_once_never_replaces_what_is_there'),
    ('M1940', 'RESULT<BOOL> (eqgate): `reaches` answers false for a commit its walk never meets', 'crates/axon-fabric/src/git_data.rs',
     '        Ok(false)\n    }\n}\n\nimpl Drop for Objects {',
     '        Ok(true)\n    }\n}\n\nimpl Drop for Objects {',
     'axon-fabric', '--test readiness', 'a_history_not_descending_from_the_certified_revision_is_not_certified'),
    ('M1941', 'RESULT<BOOL> (eqgate): the schema walker does not take a boolean for an integer', 'crates/axon-loop-contracts/src/schema.rs',
     '        "integer" => v.is_i64() || v.is_u64(),',
     '        "integer" => v.is_i64() || v.is_u64() || v.is_boolean(),',
     'axon-loop-contracts', '--lib', 'schema::tests::structural_rules'),
    ('M1942', 'RESULT<BOOL> (eqgate): the schema walker names the reference schemes exactly', 'crates/axon-loop-contracts/src/schema.rs',
     '            .is_some_and(|(sch, h)| matches!(sch, "cl22" | "acf1" | "sha256") && hex64(h)),',
     '            .is_some_and(|(_sch, h)| hex64(h)),',
     'axon-loop-contracts', '--lib', 'schema::tests::reference_patterns_are_exact'),
    ('M1943', 'COVERAGE GATE (eqgate): O_NOFOLLOW is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\bO_NOFOLLOW\\b|"\n',
     '    r"\\bNO_SUCH_FLAG_O_N\\b|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_open_flag_form_is_a_site'),
    ('M1944', 'COVERAGE GATE (eqgate): O_EXCL is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\bO_EXCL\\b|"\n',
     '    r"\\bNO_SUCH_FLAG_O_E\\b|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_open_flag_form_is_a_site'),
    ('M1945', 'COVERAGE GATE (eqgate): create_new(true) is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\bcreate_new\\(\\s*true\\s*\\)|"\n',
     '    r"\\bNO_SUCH_FLAG_cre\\b|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_open_flag_form_is_a_site'),
    ('M1946', 'COVERAGE GATE (eqgate): RENAME_NOREPLACE is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\bRENAME_NOREPLACE\\b|"\n',
     '    r"\\bNO_SUCH_FLAG_REN\\b|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_open_flag_form_is_a_site'),
    ('M1947', 'COVERAGE GATE (eqgate): AT_SYMLINK_NOFOLLOW is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\bAT_SYMLINK_NOFOLLOW\\b|"\n',
     '    r"\\bNO_SUCH_FLAG_AT_\\b|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_open_flag_form_is_a_site'),
    ('M1948', 'COVERAGE GATE (eqgate): O_DIRECTORY is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\bO_DIRECTORY\\b|"\n',
     '    r"\\bNO_SUCH_FLAG_O_D\\b|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_open_flag_form_is_a_site'),
    ('M1949', 'COVERAGE GATE (eqgate): a mount flag (MS_NOSUID ...) is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\bMS_(?:NOSUID|NODEV|NOEXEC|RDONLY|BIND|PRIVATE|SLAVE|REC)\\b"',
     '    r"\\bMS_NO_SUCH_FLAG\\b"',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_open_flag_form_is_a_site'),
    ('M1950', 'COVERAGE GATE (eqgate): a decision returned as Some(reason) is a site', 'scripts/v022_refusal_coverage.py',
     ' or OKOR_FORM.search(c) or _some_reason_is_value(c))',
     ' or OKOR_FORM.search(c) or False)',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_some_reason_value_is_a_site_and_a_pattern_is_not'),
    ('M1951', 'COVERAGE GATE (eqgate): matching or comparing Some("..") is not a decision', 'scripts/v022_refusal_coverage.py',
     '        if re.match(r"(=>|\\|)", rest):\n            continue\n',
     '        if False:\n            continue\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_some_reason_value_is_a_site_and_a_pattern_is_not'),
    ('M1952', 'COVERAGE GATE (eqgate): a function deciding by Result<bool, _> is a site', 'scripts/v022_refusal_coverage.py',
     'PRED_RET = re.compile(r"^(bool|Option\\s*<|Result\\s*<\\s*bool\\s*[,>]|i32\\b|(?:std::process::)?ExitCode\\b)")',
     'PRED_RET = re.compile(r"^(bool|Option\\s*<|NoResultBool|i32\\b|(?:std::process::)?ExitCode\\b)")',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_function_deciding_by_result_bool_i32_or_exitcode_is_a_site'),
    ('M1953', 'COVERAGE GATE (eqgate): a function returning an i32 status is a site', 'scripts/v022_refusal_coverage.py',
     'PRED_RET = re.compile(r"^(bool|Option\\s*<|Result\\s*<\\s*bool\\s*[,>]|i32\\b|(?:std::process::)?ExitCode\\b)")',
     'PRED_RET = re.compile(r"^(bool|Option\\s*<|Result\\s*<\\s*bool\\s*[,>]|NoI32\\b|(?:std::process::)?ExitCode\\b)")',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_function_deciding_by_result_bool_i32_or_exitcode_is_a_site'),
    ('M1954', 'COVERAGE GATE (eqgate): a function returning an ExitCode is a site', 'scripts/v022_refusal_coverage.py',
     'PRED_RET = re.compile(r"^(bool|Option\\s*<|Result\\s*<\\s*bool\\s*[,>]|i32\\b|(?:std::process::)?ExitCode\\b)")',
     'PRED_RET = re.compile(r"^(bool|Option\\s*<|Result\\s*<\\s*bool\\s*[,>]|i32\\b|NoExitCode\\b)")',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_function_deciding_by_result_bool_i32_or_exitcode_is_a_site'),
]


# EQGATE part 3 (amendment 81): the freeze judges the paired-disable status file.
MUTATIONS += [
    ('M1955', 'FREEZE (eqgate): the freeze refuses a paired-disable status file that is not the joined evidence', 'scripts/v022_freeze_manifest.py',
     '    if status_problems:\n        shown = ',
     '    if False:\n        shown = ',
     'axon-fabric', '--test freeze_manifest', 'a_paired_disable_status_that_is_not_the_joined_evidence_does_not_freeze'),
    ('M1956', 'FREEZE (eqgate): a status file that lacks retirement records is refused', 'scripts/v022_paired_disable.py',
     '    if missing:\n        out.append(f"{len(missing)} of {len(universe)} retirement records are missing "',
     '    if False:\n        out.append(f"{len(missing)} of {len(universe)} retirement records are missing "',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1957', 'FREEZE (eqgate): a status file older than a source change is refused', 'scripts/v022_mutation_status.py',
     '    if changed:\n        return [f"its commit {commit[:8]} is not the freeze commit',
     '    if False:\n        return [f"its commit {commit[:8]} is not the freeze commit',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_may_trail_the_freeze_commit_by_evidence_files_only'),
    ('M1958', 'FREEZE (eqgate): a status record whose cells do not hold is refused whatever it says', 'scripts/v022_paired_disable.py',
     '        elif not want:\n            out.append(f"{rid}: its recorded cells do not hold")',
     '        elif False:\n            out.append(f"{rid}: its recorded cells do not hold")',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
    ('M1959', 'FREEZE (eqgate): a status file the harness did not --join is refused', 'scripts/v022_paired_disable.py',
     '    if "shard" in doc or not isinstance(doc.get("hosts"), dict) or not isinstance(doc.get("toolchain"), dict):',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit'),
]


# ── C9 round 6, workstream OBSBIND2 (M2050-M2079; amendment 85; matrix A149-A151).
# The exec race: a Fabric-uid program that is not the pinned one spawns the helper
# with a pipe a worker reads and execs the pinned file; the helper measured the
# pinned file. The reply pipe must have no holder but the helper and its parent.
PSV_IDS |= {f"M{n}" for n in range(2050, 2080)}
_PLS2 = 'crates/axon-fabric/src/privileged_launcher.rs'
_CUS2 = 'crates/axon-fabric/src/custodian.rs'
_RACE = 'a_program_that_execs_the_pinned_file_after_spawning_the_helper_gets_no_observation'
MUTATIONS += [
    ('M2050', "A149 (obsbind2): the reply pipe has no holder but the helper and its parent (the rule)", _PLS2,
     '    if let Some(p) = holders.iter().find(|p| **p != ppid && **p != me) {\n',
     '    if let Some(p) = holders.iter().find(|_| false) {\n',
     'axon-fabric', '--lib', 'privileged_launcher::tests::a_reply_pipe_another_process_holds_is_refused'),
    ('M2051', "A151 (obsbind2): a production helper's stdout must be a pipe", _PLS2,
     '        if production {\n', '        if false && production {\n',
     'axon-fabric', '--lib', 'privileged_launcher::tests::a_reply_pipe_another_process_holds_is_refused'),
    ('M2052', "A150 (obsbind2): one custodian connection has an absolute deadline for its request", _CUS2,
     '                    let _ = s.set_read_timeout(Some(left));\n', '                    let _ = s.set_read_timeout(Some(IO_TIMEOUT));\n',
     'axon-fabric', '--lib', 'custodian::tests::a_custodian_connection_that_does_not_finish_its_request_is_cut_off'),
    ('M2053', "A149 (obsbind2): the scan finds every process holding the reply pipe", _PLS2,
     '            if let Ok(m) = std::fs::metadata(fd.path()) {\n', '            if let Ok(m) = std::fs::metadata("/nonexistent") {\n',
     'axon-fabric', '--test observer_service', _RACE),
    ('M2054', "A149 (obsbind2): the helper applies the reply-channel rule to every request it serves (the one caller gate)", _PLS2,
     '    if !a.test || c.private_reply_channel {\n', '    if false {\n',
     'axon-fabric', '--test observer_service', _RACE),
]


# ── C9 round 6, workstream BUILDENV2 (amendment 86; M2080-M2119) ─────────────
# The build-record proof took the builder's uid and key directory FROM THE
# RECORD it judged (a record naming uid 65534 and its own private directory
# passed); the judge now looks the key up under the OPERATOR's pinned parent,
# owned by the pinned uid. The host binaries are built by a controlled step
# (a retry in the same target dir used to link dependencies a wrapper had
# compiled) and judged through a signed host-build record.
PSV_IDS |= {f"M{n}" for n in range(2080, 2120)}
_HF = 'a_host_build_directory_the_pinned_builder_did_not_make_is_refused'
_HE = 'a_callers_wrapper_flags_or_planted_cc_never_reach_the_controlled_host_build'
_FA = 'a_guest_build_record_of_another_account_or_a_wrong_pin_does_not_freeze'
MUTATIONS += [
    ('M2080', 'BUILD-RECORD (6): the proof key is looked up under the PINNED parent and uid, never the record\'s own', _GE,
     '    key, why = proof_key(builder[1], pr.get("id"), builder[0])\n',
     '    key, why = proof_key(rec.get("build_parent"), pr.get("id"), rec.get("builder_uid"))\n',
     'axon-fabric', _BE, _RP),
    ('M2081', 'BUILD-RECORD (6): the builder pin is an operator file (root-owned chain, not group/other-writable)', _GE,
     '    why = operator_file_problem(path)\n    if why:\n        return None, f"the builder pin {path} is not the operator\'s: {why}"\n',
     '    why = ""\n    if why:\n        return None, f"the builder pin {path} is not the operator\'s: {why}"\n',
     'axon-fabric', '--test freeze_manifest', _FA),
    ('M2082', 'HOST-BUILD (6): a host build record is exactly the one controlled invocation', _GE,
     '    if [b.get("name") for b in rec.get("builds") or []] != [n for n, _, _ in HOST_INVOCATIONS]:\n',
     '    if False:\n',
     'axon-fabric', _BE, _HF),
    ('M2083', 'HOST-BUILD (6): the host build is of the revision the kit deploys', _GE,
     '    if not isinstance(rev, str) or not re.fullmatch(r"[0-9a-f]{40}", rev) or (commit and rev != commit):\n',
     '    if not isinstance(rev, str) or not re.fullmatch(r"[0-9a-f]{40}", rev):\n',
     'axon-fabric', _BE, _HF),
    ('M2084', 'HOST-BUILD (6): the host record names a digest for exactly the four binaries', _GE,
     '    if (not isinstance(arts, dict) or sorted(arts) != sorted(HOST_BINARIES)\n',
     '    if (not isinstance(arts, dict) or False\n',
     'axon-fabric', _BE, _HF),
    ('M2085', 'HOST-BUILD (6): each host binary is the bytes the record names', _GE,
     '        if not os.path.isfile(p) or os.path.islink(p) or sha256(p) != d:\n',
     '        if not os.path.isfile(p) or os.path.islink(p):\n',
     'axon-fabric', _BE, _HF),
    ('M2086', 'HOST-BUILD (6): a host binary is a regular file, never a symlink', _GE,
     '        if not os.path.isfile(p) or os.path.islink(p) or sha256(p) != d:\n',
     '        if not os.path.isfile(p) or sha256(p) != d:\n',
     'axon-fabric', _BE, _HF),
    ('M2087', 'HOST-BUILD (6): a host build directory holds only the binaries and the record', _GE,
     '    if extra:\n        return f"{outdir} holds files the controlled host build did not make: {extra}"\n',
     '    if False:\n        return f"{outdir} holds files the controlled host build did not make: {extra}"\n',
     'axon-fabric', _BE, _HF),
    ('M2088', 'HOST-BUILD (6): the host record carries the pinned builder\'s proof', _GE,
     '    why = proof_problems(rec, "host build", builder)\n    if why:\n        return why\n',
     '    why = ""\n    if why:\n        return why\n',
     'axon-fabric', _BE, _HF),
    ('M2089', 'HOST-BUILD (6): cargo runs in the environment the build constructs, never the caller\'s (host build)', _GE,
     '    env = dict(rec["env"])\n', '    env = {**os.environ, **rec["env"]}\n',
     'axon-fabric', _BE, _HE),
    ('M2090', 'HOST-BUILD (6): the linker is found on a FIXED PATH, never the caller\'s (a planted cc)', _GE,
     '"HOME": base, "LC_ALL": "C", "PATH": TOOL_PATH, "RUSTC": rustc}',
     '"HOME": base, "LC_ALL": "C", "PATH": os.environ.get(\'PATH\', \'\'), "RUSTC": rustc}',
     'axon-fabric', _BE, _HE),
    ('M2091', 'BUILD-RECORD (6): the proof key must be owned by the pinned builder', _GE,
     '                or not stat.S_ISREG(st.st_mode) or st.st_uid != builder_uid or st.st_mode & 0o177):\n',
     '                or not stat.S_ISREG(st.st_mode) or st.st_mode & 0o177):\n',
     'axon-fabric', _BE, _RP),
    ('M2092', 'BUILD-RECORD (6): the proof key\'s directory must be owned by the pinned builder', _GE,
     '        if (not stat.S_ISDIR(dst.st_mode) or dst.st_uid != builder_uid or dst.st_mode & 0o077\n',
     '        if (not stat.S_ISDIR(dst.st_mode) or dst.st_mode & 0o077\n',
     'axon-fabric', _BE, _RP),
]
# ── end buildenv2 ──


# ── C9 round 6, EQGATE2 (amendment 87; M2120-M2169) ──────────────────────────
# Guards expressed as a permission MODE, a prctl flag, a privilege drop, a limit,
# a signal or a pre_exec hook (round 6 weakened four, one at a time, with every
# suite green), the gate forms that now see them, and the freeze's judgement of
# the mutation run. Each row is killed by a test whose attack is the weaker mode
# or the missing call, observed through stat or /proc.
PSV_IDS |= {f"M{n}" for n in range(2120, 2170)}
MUTATIONS += [
    ('M2120', 'COVERAGE GATE (eqgate2): a permission mode is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\.mode\\(\\s*[^)\\s]|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2121', 'COVERAGE GATE (eqgate2): a set_permissions or set_mode call is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\bset_permissions\\(|\\bPermissions::from_mode\\(|(?<!fn )\\bset_mode\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2122', 'COVERAGE GATE (eqgate2): a chmod or chown is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\blibc::f?chmod(?:at)?\\(|\\blibc::[lf]?chown(?:at)?\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2123', 'COVERAGE GATE (eqgate2): a mkdirat or umask is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\blibc::mkdirat\\(|\\blibc::umask\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2124', 'COVERAGE GATE (eqgate2): a prctl is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\blibc::prctl\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2125', 'COVERAGE GATE (eqgate2): a resource limit is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\blibc::setrlimit\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2126', 'COVERAGE GATE (eqgate2): a session or process group is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\blibc::(?:setsid|setpgid|setpgrp)\\(|\\.process_group\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2127', 'COVERAGE GATE (eqgate2): a privilege drop is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\blibc::set(?:re|res)?[ug]id\\(|\\blibc::(?:setgroups|initgroups)\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2128', 'COVERAGE GATE (eqgate2): a signal disposition is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\blibc::(?:signal|sigaction|sigprocmask|pthread_sigmask|kill|killpg)\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2129', 'COVERAGE GATE (eqgate2): an fcntl is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\blibc::fcntl\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2130', 'COVERAGE GATE (eqgate2): a pre_exec hook is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\.pre_exec\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2131', 'COVERAGE GATE (eqgate2): a namespace, mount or capability call is a refusal site', 'scripts/v022_refusal_coverage.py',
     '    r"\\blibc::(?:unshare|setns|chroot|pivot_root|mount|umount2?|capset|seccomp)\\(|\\bSECCOMP_MODE_"\n',
     '    r"(?!x)x"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_permission_privilege_or_process_form_is_a_site'),
    ('M2132', 'FREEZE (eqgate2): the freeze refuses a mutation run with defects', 'scripts/v022_freeze_manifest.py',
     '    if run_problems:\n        shown = ',
     '    if False:\n        shown = ',
     'axon-fabric', '--test freeze_manifest', 'a_freeze_needs_the_merged_mutation_run'),
    ('M2133', 'FREEZE (eqgate2): the freeze refuses an absent mutation run', 'scripts/v022_freeze_manifest.py',
     '    except FileNotFoundError:\n        sys.exit(f"refused: {ms.STATUS_PATH} is absent',
     '    except ZeroDivisionError:\n        sys.exit(f"refused: {ms.STATUS_PATH} is absent',
     'axon-fabric', '--test freeze_manifest', 'a_freeze_needs_the_merged_mutation_run'),
    ('M2134', 'FREEZE (eqgate2): a single shard or a sample is not the merged run', 'scripts/v022_mutation_status.py',
     '    if (not isinstance(merged, list) or not merged or "shard" in doc or doc.get("only") is not None):',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_mutation_run_is_accepted_only_as_the_merged_killed_run_for_the_freeze_commit'),
    ('M2135', 'FREEZE (eqgate2): the run is over --scope=all', 'scripts/v022_mutation_status.py',
     '    if doc.get("scope") != "all":',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_mutation_run_is_accepted_only_as_the_merged_killed_run_for_the_freeze_commit'),
    ('M2136', 'FREEZE (eqgate2): a run made before a source change is stale', 'scripts/v022_mutation_status.py',
     '    if changed:\n        return [f"its commit',
     '    if False:\n        return [f"its commit',
     'axon-core', '--no-default-features --test harness_integrity', 'a_mutation_run_may_trail_the_freeze_commit_by_evidence_files_only'),
    ('M2137', 'FREEZE (eqgate2): a run from other registry blobs is refused', 'scripts/v022_mutation_status.py',
     '    if doc.get("registry_blobs") != mut.registry_blobs():',
     '    if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_mutation_run_is_accepted_only_as_the_merged_killed_run_for_the_freeze_commit'),
    ('M2138', 'FREEZE (eqgate2): an active row with no result is refused', 'scripts/v022_mutation_status.py',
     '    if missing:\n        out.append(f"{len(missing)} of {len(want)} active rows are missing',
     '    if False:\n        out.append(f"{len(missing)} of {len(want)} active rows are missing',
     'axon-core', '--no-default-features --test harness_integrity', 'a_mutation_run_is_accepted_only_as_the_merged_killed_run_for_the_freeze_commit'),
    ('M2139', 'FREEZE (eqgate2): a REFUSED_ELSEWHERE row is named, never a kill', 'scripts/v022_mutation_status.py',
     '        elif res == "refused_elsewhere":',
     '        elif False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_mutation_run_is_accepted_only_as_the_merged_killed_run_for_the_freeze_commit'),
    ('M2140', 'FREEZE (eqgate2): a SURVIVED row is named', 'scripts/v022_mutation_status.py',
     '        elif res.startswith("survived"):',
     '        elif False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_mutation_run_is_accepted_only_as_the_merged_killed_run_for_the_freeze_commit'),
    ('M2141', 'FREEZE (eqgate2): all_killed is recomputed from the rows', 'scripts/v022_mutation_status.py',
     '    elif not recomputed:',
     '    elif False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_mutation_run_is_accepted_only_as_the_merged_killed_run_for_the_freeze_commit'),
    ('M2142', 'FREEZE (eqgate2): a LIBRARY_PRIMITIVE row is flagged as the registry says', 'scripts/v022_mutation_status.py',
     '        if (r.get("status") == "LIBRARY_PRIMITIVE") != lib:',
     '        if False:',
     'axon-core', '--no-default-features --test harness_integrity', 'a_mutation_run_is_accepted_only_as_the_merged_killed_run_for_the_freeze_commit'),
    ('M2143', 'FREEZE (eqgate2): shards of one run share one toolchain', 'scripts/v022_mutation_status.py',
     '        if why:\n            out.append(why)\n    rows =',
     '        if False:\n            out.append(why)\n    rows =',
     'axon-core', '--no-default-features --test harness_integrity', 'a_mutation_run_is_accepted_only_as_the_merged_killed_run_for_the_freeze_commit'),
    ('M2144', 'PERMISSION/PRIVILEGE (eqgate2): the completion secret is written 0400', 'crates/axon-fabric/src/psv.rs',
     '        .mode(0o400)\n        .open(p)',
     '        .mode(0o644)\n        .open(p)',
     'axon-fabric', '--lib', 'psv::mode_tests::the_completion_secret_is_written_0400'),
    ('M2145', 'PERMISSION/PRIVILEGE (eqgate2): the Fabric-private inputs dir is created 0700', 'crates/axon-fabric/src/psv.rs',
     '        .mode(0o700)\n        .create(&dir)',
     '        .mode(0o755)\n        .create(&dir)',
     'axon-fabric', '--test submit', 'the_private_inputs_dir_is_created_0700'),
    ('M2146', 'PERMISSION/PRIVILEGE (eqgate2): keygen writes the private key 0400', 'crates/axon-fabric/src/bin/axon-fabric.rs',
     '            .mode(0o400)\n            .open(&out)',
     '            .mode(0o644)\n            .open(&out)',
     'axon-fabric', '--test attestation', 'keygen_writes_the_key_0400'),
    ('M2147', "PERMISSION/PRIVILEGE (eqgate2): a trial's cache root is 0700", 'crates/axon-fabric/src/workspace.rs',
     '        set_mode(&root, 0o700)?;',
     '        set_mode(&root, 0o755)?;',
     'axon-fabric', '--test workspace', 'the_materialized_modes_and_the_trial_cache_root_are_what_the_store_says'),
    ('M2148', "PERMISSION/PRIVILEGE (eqgate2): a read-only materialization's directories are 0555", 'crates/axon-fabric/src/workspace.rs',
     '                set_mode(&d, 0o555)?;',
     '                set_mode(&d, 0o755)?;',
     'axon-fabric', '--test workspace', 'the_materialized_modes_and_the_trial_cache_root_are_what_the_store_says'),
    ('M2149', 'PERMISSION/PRIVILEGE (eqgate2): a materialized file carries its recorded mode', 'crates/axon-fabric/src/workspace.rs',
     '                    set_mode(\n                        &p,\n                        match (executable, read_only) {',
     '                    (|p: &Path, m: u32| set_mode(p, m | 0o200))(\n                        &p,\n                        match (executable, read_only) {',
     'axon-fabric', '--test workspace', 'the_materialized_modes_and_the_trial_cache_root_are_what_the_store_says'),
    ('M2150', 'PERMISSION/PRIVILEGE (eqgate2): the permission helper applies the mode it is given', 'crates/axon-fabric/src/workspace.rs',
     '    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))\n}\n#[cfg(not(unix))]',
     '    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode | 0o222))\n}\n#[cfg(not(unix))]',
     'axon-fabric', '--test workspace', 'the_materialized_modes_and_the_trial_cache_root_are_what_the_store_says'),
    ('M2152', 'PERMISSION/PRIVILEGE (eqgate2): the check child runs inside its pre_exec hardening', 'crates/axon-psv/src/runner.rs',
     '            cmd.pre_exec(move || {\n',
     '            cmd.pre_exec(move || {\n                if true { return Ok(()); }\n',
     'axon-psv', '--test runner', 'the_check_childs_pre_exec_hook_runs'),
    ('M2153', 'PERMISSION/PRIVILEGE (eqgate2): the check child cannot gain privilege (PR_SET_NO_NEW_PRIVS)', 'crates/axon-psv/src/runner.rs',
     '                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0\n                    || libc::prctl(',
     '                if false\n                    || libc::prctl(',
     'axon-psv', '--test runner', 'the_check_child_cannot_gain_privilege_and_dies_with_the_runner'),
    ('M2154', 'PERMISSION/PRIVILEGE (eqgate2): the check child dies with the runner (PR_SET_PDEATHSIG)', 'crates/axon-psv/src/runner.rs',
     '                    || libc::prctl(\n                        libc::PR_SET_PDEATHSIG,',
     '                    || false && libc::prctl(\n                        libc::PR_SET_PDEATHSIG,',
     'axon-psv', '--test runner', 'the_check_child_cannot_gain_privilege_and_dies_with_the_runner'),
    ('M2155', 'PERMISSION/PRIVILEGE (eqgate2): the check child drops its groups', 'crates/axon-psv/src/runner.rs',
     '                    if libc::setgroups(0, std::ptr::null()) != 0\n                        || libc::setgid(gid) != 0',
     '                    if false\n                        || libc::setgid(gid) != 0',
     'axon-psv', '--test runner', 'the_check_child_runs_as_the_check_uid_with_no_groups'),
    ('M2156', 'PERMISSION/PRIVILEGE (eqgate2): the check child drops its gid', 'crates/axon-psv/src/runner.rs',
     '                        || libc::setgid(gid) != 0',
     '                        || false && libc::setgid(gid) != 0',
     'axon-psv', '--test runner', 'the_check_child_runs_as_the_check_uid_with_no_groups'),
    ('M2157', 'PERMISSION/PRIVILEGE (eqgate2): the check child drops its uid', 'crates/axon-psv/src/runner.rs',
     '                        || libc::setuid(uid) != 0',
     '                        || false && libc::setuid(uid) != 0',
     'axon-psv', '--test runner', 'the_check_child_runs_as_the_check_uid_with_no_groups'),
    ('M2158', "PERMISSION/PRIVILEGE (eqgate2): the snapshot's directories are 0755", 'crates/axon-fabric/src/privileged_launcher.rs',
     '                set_mode(&shown, 0o755)?;',
     '                set_mode(&shown, 0o777)?;',
     'axon-fabric', '--test privileged_launcher', 'the_snapshot_and_the_hand_over_keep_their_modes_and_owners'),
    ('M2159', "PERMISSION/PRIVILEGE (eqgate2): the snapshot's files carry the recorded mode", 'crates/axon-fabric/src/privileged_launcher.rs',
     '                set_mode(&shown, mode)?;',
     '                set_mode(&shown, mode | 0o022)?;',
     'axon-fabric', '--test privileged_launcher', 'the_snapshot_and_the_hand_over_keep_their_modes_and_owners'),
    ('M2160', "PERMISSION/PRIVILEGE (eqgate2): the helper's set_mode applies the mode it is given", 'crates/axon-fabric/src/privileged_launcher.rs',
     '    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))\n        .map_err(',
     '    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode | 0o022))\n        .map_err(',
     'axon-fabric', '--test privileged_launcher', 'the_snapshot_and_the_hand_over_keep_their_modes_and_owners'),
    ('M2161', "PERMISSION/PRIVILEGE (eqgate2): the hand-over keeps a directory's mode", 'crates/axon-fabric/src/privileged_launcher.rs',
     '                // SAFETY: fchmod/fchown on a descriptor we hold.\n                unsafe {\n                    libc::fchmod(fd.as_raw_fd(), st.st_mode & 0o777) == 0',
     '                // SAFETY: fchmod/fchown on a descriptor we hold.\n                unsafe {\n                    libc::fchmod(fd.as_raw_fd(), 0o777) == 0',
     'axon-fabric', '--test privileged_launcher', 'the_snapshot_and_the_hand_over_keep_their_modes_and_owners'),
    ('M2162', 'PERMISSION/PRIVILEGE (eqgate2): the hand-over gives a directory to the Fabric uid', 'crates/axon-fabric/src/privileged_launcher.rs',
     '                        && libc::fchown(fd.as_raw_fd(), uid, u32::MAX) == 0\n                }\n            }\n            libc::S_IFREG',
     '                        && libc::fchown(fd.as_raw_fd(), 0, u32::MAX) == 0\n                }\n            }\n            libc::S_IFREG',
     'axon-fabric', '--test privileged_launcher', 'the_snapshot_and_the_hand_over_keep_their_modes_and_owners'),
    ('M2163', "PERMISSION/PRIVILEGE (eqgate2): the hand-over keeps a file's mode", 'crates/axon-fabric/src/privileged_launcher.rs',
     '                .map_err(|e| e.to_string())?;\n                unsafe {\n                    libc::fchmod(fd.as_raw_fd(), st.st_mode & 0o777) == 0',
     '                .map_err(|e| e.to_string())?;\n                unsafe {\n                    libc::fchmod(fd.as_raw_fd(), 0o777) == 0',
     'axon-fabric', '--test privileged_launcher', 'the_snapshot_and_the_hand_over_keep_their_modes_and_owners'),
    ('M2164', 'PERMISSION/PRIVILEGE (eqgate2): the hand-over gives a file to the Fabric uid', 'crates/axon-fabric/src/privileged_launcher.rs',
     '                        && libc::fchown(fd.as_raw_fd(), uid, u32::MAX) == 0\n                }\n            }\n            libc::S_IFLNK',
     '                        && libc::fchown(fd.as_raw_fd(), 0, u32::MAX) == 0\n                }\n            }\n            libc::S_IFLNK',
     'axon-fabric', '--test privileged_launcher', 'the_snapshot_and_the_hand_over_keep_their_modes_and_owners'),
    ('M2165', 'PERMISSION/PRIVILEGE (eqgate2): the out dir is handed over 0700', 'crates/axon-fabric/src/privileged_launcher.rs',
     'libc::fchmod(p.out.as_raw_fd(), 0o700) == 0',
     'libc::fchmod(p.out.as_raw_fd(), 0o777) == 0',
     'axon-fabric', '--test privileged_launcher', 'the_snapshot_and_the_hand_over_keep_their_modes_and_owners'),
    ('M2166', 'PERMISSION/PRIVILEGE (eqgate2): the out dir is handed over to the Fabric uid', 'crates/axon-fabric/src/privileged_launcher.rs',
     'libc::fchown(p.out.as_raw_fd(), c.fabric_uid, u32::MAX) == 0',
     'libc::fchown(p.out.as_raw_fd(), 0, u32::MAX) == 0',
     'axon-fabric', '--test privileged_launcher', 'the_snapshot_and_the_hand_over_keep_their_modes_and_owners'),
    ('M2167', "PERMISSION/PRIVILEGE (eqgate2): the root launcher's open-file and core limits are set", 'crates/axon-fabric/src/privileged_launcher.rs',
     '            };\n            if libc::setrlimit(r, &l) != 0 {\n                // Not root',
     '            };\n            if false && libc::setrlimit(r, &l) != 0 {\n                // Not root',
     'axon-fabric', '--test privileged_launcher', 'a_callers_process_state_never_reaches_the_root_helper_or_its_launcher'),
    ('M2168', "PERMISSION/PRIVILEGE (eqgate2): the root helper drops its caller's groups", 'crates/axon-fabric/src/privileged_launcher.rs',
     '        if libc::setgroups(0, std::ptr::null()) != 0\n            || libc::setresgid(0, 0, 0) != 0',
     '        if false\n            || libc::setresgid(0, 0, 0) != 0',
     'axon-fabric', '--test privileged_launcher', 'the_root_helper_takes_roots_identity_not_its_callers_groups'),
    ('M2169', "PERMISSION/PRIVILEGE (eqgate2): the root helper takes root's gid", 'crates/axon-fabric/src/privileged_launcher.rs',
     '            || libc::setresgid(0, 0, 0) != 0',
     '            || false && libc::setresgid(0, 0, 0) != 0',
     'axon-fabric', '--test privileged_launcher', 'the_root_helper_takes_roots_identity_not_its_callers_groups'),
]

# ── C9 round 7, workstream BUILDENV3 (amendment 90; M2220-M2259) ─────────────
# What the build processes can and cannot touch: they run as an unprivileged
# uid (setpriv), the toolchain is a root-owned private copy not on PATH, the
# tools are re-measured before anything is signed, and the proof key's directory
# is root's. The reviewer's attack: a build script planted `cc` next to rustc
# (every host binary was linked through it) and read the 0400 proof key.
PSV_IDS |= {f"M{n}" for n in range(2220, 2260)}
_PLANT = 'a_build_script_cannot_plant_a_linker_or_read_the_proof_key'
_RO = 'the_build_processes_run_as_an_unprivileged_uid_that_cannot_reach_the_key_or_the_toolchain'
_TREE = 'a_toolchain_tree_another_uid_or_group_can_write_is_refused'
_PRIV = 'the_private_toolchain_copy_is_refused_for_a_tree_another_uid_can_write'
_NOSIGN = 'a_change_to_the_compiler_or_linker_tools_is_never_signed'
_ISO = 'a_guest_build_record_that_does_not_show_isolated_build_processes_does_not_freeze'
_FM = '--test freeze_manifest'
_SETPRIV = (
    '    return [UNSHARE, "--pid", "--fork", "--mount-proc", "--kill-child", "--",\n'
    '            "/usr/bin/setpriv", f"--reuid={uid}", f"--regid={gid}", "--clear-groups", "--no-new-privs",\n'
    '            "--", *argv]')
MUTATIONS += [
    ('M2220', "BUILD-ENV (7): the build's PATH is the fixed system directories only (the toolchain directory is not on it)", _GE,
     '"HOME": base, "LC_ALL": "C", "PATH": TOOL_PATH, "RUSTC": rustc}',
     '"HOME": base, "LC_ALL": "C", "PATH": f"{os.path.dirname(cargo)}:{TOOL_PATH}", "RUSTC": rustc}',
     'axon-fabric', _BE, _PLANT),
    ('M2221', 'BUILD-ENV (7): every build process runs as the unprivileged build uid', _GE,
     _SETPRIV, '    return list(argv)',
     'axon-fabric', _BE, _RO),
    ('M2222', 'BUILD-ENV (7): the toolchain the build copies must be root-owned and closed to group/other writes', _GE,
     '    why = toolchain_tree_problem(src)\n    if why:\n', '    why = toolchain_tree_problem(src)\n    if False:\n',
     'axon-fabric', _BE, _PRIV),
    ('M2223', 'BUILD-ENV (7): a toolchain entry another group or world can write is refused', _GE,
     '            if st.st_uid != 0 or st.st_mode & 0o022 != 0:\n', '            if st.st_uid != 0:\n',
     'axon-fabric', _BE, _TREE),
    ('M2224', 'BUILD-ENV (7): a toolchain entry another uid owns is refused', _GE,
     '            if st.st_uid != 0 or st.st_mode & 0o022 != 0:\n', '            if st.st_mode & 0o022 != 0:\n',
     'axon-fabric', _BE, _TREE),
    ('M2225', 'BUILD-ENV (7): a record whose tools changed since begin is never signed', _GE,
     '        why = measure_problem(rec)\n        if why:\n            fail(why)\n        key, why = proof_key(',
     '        why = measure_problem(rec)\n        if False:\n            fail(why)\n        key, why = proof_key(',
     'axon-fabric', _BE, _NOSIGN),
    ('M2226', "BUILD-ENV (7): the toolchain bin directory's listing is measured (a planted cc beside rustc)", _GE,
     '            "bin": bin_listing_sha256(os.path.dirname(tc["cargo"])),\n', '            "bin": "",\n',
     'axon-fabric', _BE, _NOSIGN),
    ('M2227', "BUILD-ENV (7): the linker tools are measured live, as the build's PATH resolves them", _GE,
     '            "tools": {n: t["sha256"] for n, t in host_tools(CARGO_HOST_TOOLS).items()}}',
     '            "tools": {n: t["sha256"] for n, t in tc["host_tools"].items()}}',
     'axon-fabric', _BE, 'a_changed_linker_is_not_signed_over'),
    ('M2228', 'BUILD-ENV (7): GIT_CEILING_DIRECTORIES hides an enclosing repository from the build', _GE,
     '"GIT_CEILING_DIRECTORIES": base,\n', '',
     'axon-fabric', _BE, 'a_git_repository_enclosing_the_build_directory_is_invisible_to_the_build'),
    ('M2229', 'BUILD-ENV (7): the proof holds only for a record built under the PINNED build uid', _GE,
     '    if len(builder) == 3 and rec.get("build_uid") != builder[2]:\n', '    if False and len(builder) == 3 and rec.get("build_uid") != builder[2]:\n',
     'axon-fabric', _FM, 'a_builder_pin_without_an_unprivileged_build_uid_or_another_one_does_not_freeze'),
    ('M2231', 'BUILD-ENV (7): the judge requires the toolchain to be the private copy made for the build', _GE,
     '            or not str(tc["cargo"]).startswith(os.path.join(base, "toolchains") + "/")):\n', '            or False):\n',
     'axon-fabric', _FM, _ISO),
    ('M2232', 'BUILD-ENV (7): the judge holds the measured compiler to the recorded toolchain', _GE,
     '    if (rec.get("measured") or {}).get("cargo") != tc.get("cargo_sha256") or (rec.get("measured") or {}).get("rustc") != tc.get("rustc_sha256") \\\n',
     '    if False or (rec.get("measured") or {}).get("rustc") != tc.get("rustc_sha256") \\\n',
     'axon-fabric', _FM, _ISO),
    ('M2233', 'BUILD-ENV (7): the judge holds the measured linker to the recorded host tools', _GE,
     '            or {n: (tc.get("host_tools") or {}).get(n, {}).get("sha256") for n in CARGO_HOST_TOOLS} != {n: (rec["measured"]["tools"]).get(n) for n in CARGO_HOST_TOOLS}:\n',
     '            or False:\n',
     'axon-fabric', _FM, _ISO),
    ('M2235', 'BUILD-ENV (7): check-host-record takes its builder flags together or not at all', _GE,
     '        if given and len(given) != 3:\n', '        if False:\n',
     'axon-fabric', _BE, 'a_partial_or_malformed_builder_flag_set_is_refused'),
    ('M2236', "BUILD-ENV (7): check-host-record's uid flags are plain ASCII decimal (not str.isdigit)", _GE,
     '            if (not re.fullmatch(r"[0-9]{1,9}", opts["--builder-uid"] + "") or',
     '            if (not opts["--builder-uid"].isdigit() or',
     'axon-fabric', _BE, 'a_partial_or_malformed_builder_flag_set_is_refused'),
    ('M2237', "BUILD-ENV (7): the kernel's make runs as the unprivileged build uid", _GE,
     '            r = subprocess.run(as_build_uid(argv), env=kenv, cwd=ksrc, stdout=subprocess.DEVNULL)',
     '            r = subprocess.run(argv, env=kenv, cwd=ksrc, stdout=subprocess.DEVNULL)',
     'axon-fabric', _BE, 'a_callers_kcflags_cc_or_path_do_not_reach_the_kernel_build'),
    ('M2238', 'BUILD-ENV (7): the judge holds the recorded environment to the one the build constructs (git ceiling, fixed PATH)', _GE,
     '    if env != want or rec.get("cargo_home") != want["CARGO_HOME"] or rec.get("target_dir") != want["CARGO_TARGET_DIR"]:\n',
     '    if False:\n',
     'axon-fabric', _FM, _ISO),
]
# ── end buildenv3 ──
# ── C9 round 8, BUILDENV4 (amendment 92; M2260-M2269) ──────────────────────────
# What the build uid leaves running or owning after a step, where the rootfs reads its inputs, and
# the one door a test may use to run the operator kit. (M2260, the kit's null machine-id refusal, is
# a kit guard: removed alone by hand INSIDE the namespace helper, see amendment 92; not a cargo row.)
_OE = '--test operator_examples'
MUTATIONS += [
    ('M2261', 'BUILD-ENV (92): after a step every process of the build uid is killed before anything is signed', _GE,
     '            reap_build_processes()\n            lock_from_build(rec)\n',
     '            lock_from_build(rec)\n',
     'axon-fabric', _GBT, 'a_detached_build_process_does_not_outlive_its_step_or_rewrite_what_is_signed'),
    ('M2262', 'BUILD-ENV (92): after a step the source copy, CARGO_HOME and target dir are root-owned and not writable by the build uid', _GE,
     '            lock_from_build(rec)\n    finally:\n        os.close(lockfd)\n',
     '            pass\n    finally:\n        os.close(lockfd)\n',
     'axon-fabric', _GBT, 'the_trees_a_step_leaves_are_root_owned_and_not_writable_by_the_build_uid'),
    ('M2263', "BUILD-ENV (92): the rootfs reads kernel.pin from the committed tree, not the builder's copy", _GE,
     '    pin = read_pin(None, committed_file("profiles/linux-microvm/kernel.pin").decode())\n',
     '    pin = read_pin(os.path.join(rec["src_dir"], "profiles", "linux-microvm"))\n',
     'axon-fabric', _GBT, 'the_rootfs_inputs_come_from_the_committed_tree_not_the_builders_copy'),
    ('M2264', "BUILD-ENV (92): the rootfs reads guest-init.sh from the committed tree, not the builder's copy", _GE,
     '        f.write(committed_file("profiles/linux-microvm/guest-init.sh"))\n',
     '        f.write(open(os.path.join(rec["src_dir"], "profiles", "linux-microvm", "guest-init.sh"), "rb").read())\n',
     'axon-fabric', _GBT, 'the_rootfs_inputs_come_from_the_committed_tree_not_the_builders_copy'),
    ('M2265', 'KIT-TEST (92): a test script running the operator kit or --apply outside ns_run is refused', 'scripts/opkit_ns_drift.py',
     '            if kind and not wrapped:\n                bad.append(f"{path}:{n}: [{kind}] not itself',
     '            if False and kind and not wrapped:\n                bad.append(f"{path}:{n}: [{kind}] not itself',
     'axon-fabric', _OE, 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2266', 'KIT-TEST (92): the namespace helper refuses a destination that is not shadowed by a tmpfs', 'scripts/lib/opkit_ns.sh',
     '    [ "$fs" = tmpfs ] || {',
     '    [ "$fs" = tmpfs ] || true || {',
     'axon-fabric', _OE, 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2267', "KIT-TEST (92): the namespace helper refuses to run in the host's own mount namespace", 'scripts/lib/opkit_ns.sh',
     '  [ -n "$own" ] && [ -n "$hostmnt" ] && [ "$own" != "$hostmnt" ] \\\n',
     '  [ -n "$own" ] \\\n',
     'axon-fabric', _OE, 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2268', "KIT-TEST (92, 97): the namespace helper refuses a destination that is the host's own directory (device:inode through the host's view)", 'scripts/lib/opkit_ns.sh',
     '    [ "$host_dev" != "$here_dev" ] \\\n',
     '    true \\\n',
     'axon-fabric', _OE, 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2269', 'KIT-TEST (92): ns_run never starts its command when the isolation is not proved', 'scripts/lib/opkit_ns.sh',
     '    opkit_ns_isolate || { echo "REFUSE(ns_run): isolation not proved; the command did not run" >&2; exit 97; }',
     '    opkit_ns_isolate || { echo "REFUSE(ns_run): isolation not proved; the command did not run" >&2; }',
     'axon-fabric', _OE, 'the_namespace_helper_refuses_when_its_proof_fails'),
]

# ── C9 round 7, EQGATE3 (amendment 91; M2270-M2339) ──────────────────────────
# What a child is BUILT with (environment, working directory, stdio), what bounds
# a read, the git option list, a close-on-exec flag, a signer file's kind, the
# guard forms that see them, and refusals the old rows exempted as unreachable or
# unobservable and a test nobody wrote kills. Each row is killed by a test whose
# attack is the weaker environment, redirection, cap or option, observed through
# the child's own /proc state, a file, memory growth or a hostile repository.
PSV_IDS |= {f"M{n}" for n in range(2270, 2340)}
MUTATIONS += [
    ('M2270', "COVERAGE GATE (eqgate3): a Command's environment, working directory or null stdio is a site", 'scripts/v022_refusal_coverage.py',
     '    r"\\.(?:env_clear|env_remove|envs)\\(|(?<![\\w:])(?:cmd|c|command)\\.env\\(|^\\s*\\.env\\(|\\.current_dir\\(|\\.(?:stdin|stdout|stderr)\\(\\s*(?:std::process::)?Stdio::(?:null|inherit)\\(|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_child_build_and_a_size_cap_are_sites'),
    ('M2273', 'COVERAGE GATE (eqgate3): a git -c option or GIT_* variable is a site', 'scripts/v022_refusal_coverage.py',
     '    r"\\"-c\\"|\\bcore\\.(?:fsmonitor|hooksPath|excludesFile|attributesFile|checkStat|trustCtime)\\b|\\bprotocol\\.allow\\b|\\bsafe\\.directory\\b|\\"--no-includes\\"|\\"GIT_[A-Z_]+\\"|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_child_build_and_a_size_cap_are_sites'),
    ('M2275', 'COVERAGE GATE (eqgate3): a process-state libc call or a close-on-exec flag is a site', 'scripts/v022_refusal_coverage.py',
     '    r"\\blibc::(?:setitimer|chdir|fchdir|close_range|setpriority|sched_\\w+|personality|flock|setsockopt|dup[23]?|pipe2|socketpair|accept4|socket|unlinkat?|renameat2?|linkat?)\\(|\\bSYS_close_range\\b|\\b(?:O_CLOEXEC|SOCK_CLOEXEC|FD_CLOEXEC|SOCK_NONBLOCK)\\b|"\n',
     '    r"(?!x)x|"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_child_build_and_a_size_cap_are_sites'),
    ('M2276', 'LAUNCHER (eqgate3): every lim2 limit is set by the closure that carries it', 'crates/axon-fabric/src/privileged_launcher.rs',
     '            };\n            if libc::setrlimit(r, &l) != 0 {\n                let mut cur: libc::rlimit = std::mem::zeroed();',
     '            };\n            if false && libc::setrlimit(r, &l) != 0 {\n                let mut cur: libc::rlimit = std::mem::zeroed();',
     'axon-fabric', '--test privileged_launcher', 'a_callers_other_resource_limits_never_reach_the_root_launch'),
    ('M2277', 'COVERAGE GATE (eqgate3): a `.min(room)` or `.take(bound)` or `.take(MAX_*)` cap is a site', 'scripts/v022_refusal_coverage.py',
     '    r"\\.min\\(\\s*(?:room|cap|limit|bound|max)\\w*\\s*\\)|\\.take\\(\\s*[a-z_]*(?:limit|cap|bound|max)\\w*|\\.(?:min|take|truncate)\\([^)]*\\bMAX_[A-Z_]+"\n',
     '    r"(?!x)x"\n',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_child_build_and_a_size_cap_are_sites'),
    ('M2279', 'COVERAGE GATE (eqgate3): a diverging closure that delegates to a refusal is a constructor', 'scripts/v022_refusal_coverage.py',
     '            if any(c in out for c in calls):',
     '            if False:',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_diverging_closure_that_delegates_to_a_refusal_is_a_constructor'),
    ('M2280', 'COVERAGE GATE (eqgate3): a diverging fn that exits with a computed code is a constructor', 'scripts/v022_refusal_coverage.py',
     '        if any(EXIT_NONZERO.search(x) or EXIT_COMPUTED.search(x) for x in lines[i:i + DIVERGING_BODY]):',
     '        if any(EXIT_NONZERO.search(x) for x in lines[i:i + DIVERGING_BODY]):',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_diverging_closure_that_delegates_to_a_refusal_is_a_constructor'),
    ('M2281', 'SIGNER (eqgate3): a signer key that is not a regular file (a FIFO) is refused', 'crates/axon-fabric/src/bin/axon-fabric.rs',
     '        if !meta.file_type().is_file() {',
     '        if false && !meta.file_type().is_file() {',
     'axon-fabric', '--test attestation', 'a_signer_the_operator_did_not_provision_properly_signs_nothing'),
    ('M2282', 'SIGNER (eqgate3): a signer naming more than issuer_ref, key_path, public_key is refused', 'crates/axon-fabric/src/bin/axon-fabric.rs',
     '    if keys != ["issuer_ref", "key_path", "public_key"] {',
     '    if false && keys != ["issuer_ref", "key_path", "public_key"] {',
     'axon-fabric', '--test attestation', 'a_signer_the_operator_did_not_provision_properly_signs_nothing'),
    ('M2283', 'GIT (eqgate3): git runs from an empty environment', 'crates/axon-fabric/src/git_data.rs',
     '    c.env_clear()\n        .env("PATH", "/usr/bin:/bin")\n        .env("LC_ALL", "C")\n        .env("GIT_NO_REPLACE_OBJECTS", "1")',
     '    c.env("PATH", "/usr/bin:/bin")\n        .env("LC_ALL", "C")\n        .env("GIT_NO_REPLACE_OBJECTS", "1")',
     'axon-fabric', '--lib', 'git_data::tests::the_callers_git_environment_does_not_steer_a_git_call'),
    ('M2284', 'GIT (eqgate3): a read-only git call does not write the index', 'crates/axon-fabric/src/git_data.rs',
     '        .env("GIT_OPTIONAL_LOCKS", "0")\n',
     '        .env("GIT_EQ_NOOP", "0")\n',
     'axon-fabric', '--lib', 'git_data::tests::git_does_not_write_the_index_of_the_tree_it_reads'),
    ('M2285', "GIT (eqgate3): git never runs the repository's fsmonitor", 'crates/axon-fabric/src/git_data.rs',
     '.args(["-c", "core.fsmonitor=", "-c", "core.hooksPath=/dev/null"])',
     '.args(["-c", "core.hooksPath=/dev/null"])',
     'axon-fabric', '--lib', 'git_data::tests::git_runs_no_fsmonitor_or_hook_the_repository_configures'),
    ('M2286', "GIT (eqgate3): git never runs a hook from the repository's hooksPath", 'crates/axon-fabric/src/git_data.rs',
     '.args(["-c", "core.fsmonitor=", "-c", "core.hooksPath=/dev/null"])',
     '.args(["-c", "core.fsmonitor="])',
     'axon-fabric', '--lib', 'git_data::tests::git_runs_no_fsmonitor_or_hook_the_repository_configures'),
    ('M2287', 'GIT (eqgate3): git trusts no excludesFile of the repository', 'crates/axon-fabric/src/git_data.rs',
     '.args(["-c", "core.excludesFile=/dev/null"])',
     '.args(["-c", "core.eqnoop=1"])',
     'axon-fabric', '--lib', 'git_data::tests::git_trusts_neither_the_repositorys_ignore_nor_attribute_files'),
    ('M2288', 'GIT (eqgate3): git trusts no attributesFile of the repository', 'crates/axon-fabric/src/git_data.rs',
     '.args(["-c", "core.attributesFile=/dev/null"])',
     '.args(["-c", "core.eqnoop=1"])',
     'axon-fabric', '--lib', 'git_data::tests::git_trusts_neither_the_repositorys_ignore_nor_attribute_files'),
    ('M2289', 'GIT (eqgate3): git compares every stat field (checkStat)', 'crates/axon-fabric/src/git_data.rs',
     '.args(["-c", "core.checkStat=default", "-c", "core.trustCtime=true"])',
     '.args(["-c", "core.trustCtime=true"])',
     'axon-fabric', '--lib', 'git_data::tests::git_compares_every_stat_field_whatever_the_repository_says'),
    ('M2290', 'GIT (eqgate3): git trusts ctime whatever the repository says', 'crates/axon-fabric/src/git_data.rs',
     '.args(["-c", "core.checkStat=default", "-c", "core.trustCtime=true"])',
     '.args(["-c", "core.checkStat=default"])',
     'axon-fabric', '--lib', 'git_data::tests::git_compares_every_stat_field_whatever_the_repository_says'),
    ('M2291', 'GIT (eqgate3): git reads a repository another uid owns', 'crates/axon-fabric/src/git_data.rs',
     '.args(["-c", "safe.directory=*"])',
     '.args(["-c", "core.eqnoop=1"])',
     'axon-fabric', '--lib', 'git_data::tests::git_reads_a_repository_another_uid_owns'),
    ('M2292', "STDIO (eqgate3): the root launcher's stdin is /dev/null", 'crates/axon-fabric/src/privileged_launcher.rs',
     '        cmd.stdin(std::process::Stdio::null())\n',
     '        cmd.stdin(std::process::Stdio::inherit())\n',
     'axon-fabric', '--test privileged_launcher', 'the_root_launcher_runs_with_null_stdio_in_the_root_directory'),
    ('M2293', "STDIO (eqgate3): the root launcher's stdout is /dev/null", 'crates/axon-fabric/src/privileged_launcher.rs',
     '            .stdout(std::process::Stdio::null())\n            .stderr(std::process::Stdio::null())\n            .current_dir("/")',
     '            .stdout(std::process::Stdio::inherit())\n            .stderr(std::process::Stdio::null())\n            .current_dir("/")',
     'axon-fabric', '--test privileged_launcher', 'the_root_launcher_runs_with_null_stdio_in_the_root_directory'),
    ('M2294', "STDIO (eqgate3): the root launcher's stderr is /dev/null", 'crates/axon-fabric/src/privileged_launcher.rs',
     '            .stderr(std::process::Stdio::null())\n            .current_dir("/")',
     '            .stderr(std::process::Stdio::inherit())\n            .current_dir("/")',
     'axon-fabric', '--test privileged_launcher', 'the_root_launcher_runs_with_null_stdio_in_the_root_directory'),
    ('M2295', "STDIO (eqgate3): the pinned launcher's stdin is /dev/null", 'crates/axon-fabric/src/backend.rs',
     '            c.stdin(std::process::Stdio::null())\n                .stdout(std::process::Stdio::null())',
     '            c.stdin(std::process::Stdio::inherit())\n                .stdout(std::process::Stdio::null())',
     'axon-fabric', '--test submit', 'the_pinned_launcher_and_its_verify_step_run_with_null_stdio'),
    ('M2296', "STDIO (eqgate3): the pinned launcher's stdout is /dev/null", 'crates/axon-fabric/src/backend.rs',
     '                .stdout(std::process::Stdio::null())\n                .stderr(std::process::Stdio::null())\n                .status()',
     '                .stdout(std::process::Stdio::inherit())\n                .stderr(std::process::Stdio::null())\n                .status()',
     'axon-fabric', '--test submit', 'the_pinned_launcher_and_its_verify_step_run_with_null_stdio'),
    ('M2297', "STDIO (eqgate3): the pinned launcher's stderr is /dev/null", 'crates/axon-fabric/src/backend.rs',
     '                .stderr(std::process::Stdio::null())\n                .status()',
     '                .stderr(std::process::Stdio::inherit())\n                .status()',
     'axon-fabric', '--test submit', 'the_pinned_launcher_and_its_verify_step_run_with_null_stdio'),
    ('M2298', "STDIO (eqgate3): the observer program's stdin is /dev/null", 'crates/axon-fabric/src/observer.rs',
     '    .stdin(std::process::Stdio::null())\n',
     '    .stdin(std::process::Stdio::inherit())\n',
     'axon-fabric', '--test psv_dispatch', 'the_observer_program_runs_with_null_stdio'),
    ('M2299', "STDIO (eqgate3): the observer program's stdout is /dev/null", 'crates/axon-fabric/src/observer.rs',
     '    .stdout(std::process::Stdio::null())\n',
     '    .stdout(std::process::Stdio::inherit())\n',
     'axon-fabric', '--test psv_dispatch', 'the_observer_program_runs_with_null_stdio'),
    ('M2300', "STDIO (eqgate3): the observer program's stderr is /dev/null", 'crates/axon-fabric/src/observer.rs',
     '    .stdout(std::process::Stdio::null())\n    .stderr(std::process::Stdio::null())\n',
     '    .stdout(std::process::Stdio::null())\n    .stderr(std::process::Stdio::inherit())\n',
     'axon-fabric', '--test psv_dispatch', 'the_observer_program_runs_with_null_stdio'),
    ('M2301', "CAP (eqgate3): the PSV runner buffers at most the manifest's output bound", 'crates/axon-psv/src/runner.rs',
     '                        kept.extend_from_slice(&buf[..n.min(room)]);',
     '                        kept.extend_from_slice(&buf[..n]);',
     'axon-psv', '--test runner', 'the_runner_never_buffers_more_than_the_manifests_output_bound'),
    ('M2302', 'CAP (eqgate3): the local check executor buffers at most its capture bound', 'crates/axon-cortex/src/runner.rs',
     '                        kept.extend_from_slice(&buf[..n.min(room)]);',
     '                        kept.extend_from_slice(&buf[..n]);',
     'axon-cortex', '--test check_executor', 'the_local_check_child_cannot_make_the_launcher_buffer_its_whole_output'),
    ('M2303', "CAP (eqgate3): a generator's stdout is read through its cap", 'crates/axon-cortex/src/generate.rs',
     '                    let _ = p.take(MAX_GENERATOR_READ as u64).read_to_end(&mut buf);\n                }\n                buf\n            })\n        };',
     '                    let _ = p.take(u64::MAX).read_to_end(&mut buf);\n                }\n                buf\n            })\n        };',
     'axon-cortex', '--test generator_cap', 'a_generators_stdout_is_never_buffered_past_its_cap'),
    ('M2304', "CAP (eqgate3): a generator's stderr is read through its cap", 'crates/axon-cortex/src/generate.rs',
     '                let _ = p.take(MAX_GENERATOR_READ as u64).read_to_end(&mut buf);\n            }\n            buf\n        });',
     '                let _ = p.take(u64::MAX).read_to_end(&mut buf);\n            }\n            buf\n        });',
     'axon-cortex', '--test generator_cap', 'a_generators_stderr_is_never_buffered_past_its_cap'),
    ('M2305', 'CAP (eqgate3): a custodian request line is read through MAX_MESSAGE', 'crates/axon-fabric/src/custodian.rs',
     '                let mut r = (&s).take(MAX_MESSAGE);\n                let mut byte',
     '                let mut r = &s;\n                let mut byte',
     'axon-fabric', '--lib', 'custodian::tests::a_custodian_request_line_is_cut_off_at_its_size_bound'),
    ('M2306', 'CAP (eqgate3): an observer request line is read through MAX_REQUEST', 'crates/axon-fabric/src/observer_service.rs',
     '                let mut r = (&s).take(MAX_REQUEST);',
     '                let mut r = &s;',
     'axon-fabric', '--lib', 'observer_service::tests::an_observer_request_line_is_cut_off_at_its_size_bound'),
    ('M2307', 'CAP (eqgate3): a measured file is read through max + 1', 'crates/axon-fabric/src/observer_service.rs',
     '    f.take(max + 1)\n        .read_to_end(&mut bytes)',
     '    f.take(u64::MAX)\n        .read_to_end(&mut bytes)',
     'axon-fabric', '--lib', 'observer_service::tests::a_measured_file_past_its_bound_is_never_buffered'),
    ('M2308', 'OPEN FLAG (eqgate3): every descriptor the helper opens for itself is close-on-exec', 'crates/axon-fabric/src/privileged_launcher.rs',
     'flags | libc::O_CLOEXEC, 0)',
     'flags, 0)',
     'axon-fabric', '--test privileged_launcher', 'the_root_launcher_inherits_only_the_descriptors_it_is_handed'),
    ('M2309', 'ENV (eqgate3): the PSV check child starts from an empty environment', 'crates/axon-psv/src/runner.rs',
     '        .env_clear()\n        .env("PATH", "/usr/bin:/bin")',
     '        .env("PATH", "/usr/bin:/bin")',
     'axon-psv', '--test runner', 'the_check_child_runs_in_the_suite_with_only_its_own_environment_and_stdio'),
    ('M2310', 'ENV (eqgate3): the PSV check child runs in the suite directory', 'crates/axon-psv/src/runner.rs',
     '    cmd.current_dir(&cfg.suite)',
     '    cmd.current_dir("/")',
     'axon-psv', '--test runner', 'the_check_child_runs_in_the_suite_with_only_its_own_environment_and_stdio'),
    ('M2311', "ENV (eqgate3): the PSV check child's module path is exclusive", 'crates/axon-psv/src/runner.rs',
     '        .env("AXON_PATH_EXCLUSIVE", "1")\n',
     '        .env("AXON_EQ_NOOP", "1")\n',
     'axon-psv', '--test runner', 'the_check_child_runs_in_the_suite_with_only_its_own_environment_and_stdio'),
    ('M2312', 'ENV (eqgate3): the local check child starts from an empty environment (clean_env)', 'crates/axon-cortex/src/runner.rs',
     '        if self.limits.clean_env {\n            cmd.env_clear();',
     '        if self.limits.clean_env {\n            cmd.env("EQ_NOOP", "1");',
     'axon-cortex', '--test check_executor', 'the_local_check_child_gets_its_workspace_and_only_the_environment_it_is_given'),
    ('M2313', 'ENV (eqgate3): the local check child runs under its effect ceiling', 'crates/axon-cortex/src/runner.rs',
     '            cmd.env("AXON_ALLOWED_EFFECTS", c);',
     '            cmd.env("AXON_EQ_NOOP", c);',
     'axon-cortex', '--test check_executor', 'the_local_check_child_gets_its_workspace_and_only_the_environment_it_is_given'),
    ('M2314', 'ENV (eqgate3): the local check child gets the environment it is given', 'crates/axon-cortex/src/runner.rs',
     '            cmd.env(k, v);\n        }\n        let (text, code, truncated)',
     '            let _ = (k, v);\n        }\n        let (text, code, truncated)',
     'axon-cortex', '--test check_executor', 'the_local_check_child_gets_its_workspace_and_only_the_environment_it_is_given'),
    ('M2315', 'ENV (eqgate3): the interpreter child starts from an empty environment', 'crates/axon-os/src/runtime.rs',
     '        cmd.env_clear();\n        cmd.env("AXON_SEED"',
     '        cmd.env("EQ_NOOP", "1");\n        cmd.env("AXON_SEED"',
     'axon-os', '--test legacy_adapter', 'the_interpreter_child_is_built_from_an_empty_environment_and_the_jobs_directory'),
    ('M2316', "ENV (eqgate3): the interpreter child gets the job's seed", 'crates/axon-os/src/runtime.rs',
     '        cmd.env("AXON_SEED", seed.to_string());',
     '        cmd.env("AXON_EQ_NOOP", seed.to_string());',
     'axon-os', '--test legacy_adapter', 'the_interpreter_child_is_built_from_an_empty_environment_and_the_jobs_directory'),
    ('M2317', "ENV (eqgate3): the interpreter child gets the operator's AXON_* controls (non-hermetic)", 'crates/axon-os/src/runtime.rs',
     '                if let Some(v) = std::env::var_os(key) {\n                    cmd.env(key, v);',
     '                if let Some(v) = std::env::var_os(key) {\n                    let _ = v;',
     'axon-os', '--test legacy_adapter', 'the_interpreter_child_is_built_from_an_empty_environment_and_the_jobs_directory'),
    ('M2318', 'EXEMPTION REFUTED (eqgate3): a journal holding a duplicate settlement line is corrupt', 'crates/axon-fabric/src/journal.rs',
     '            if matches!(change, Change::Duplicate) {',
     '            if false && matches!(change, Change::Duplicate) {',
     'axon-fabric', '--test journal', 'g13_a_journal_holding_a_duplicate_settlement_line_is_corrupt'),
    ('M2319', "ENV (eqgate3): the interpreter child runs in the job's directory", 'crates/axon-os/src/runtime.rs',
     '                cmd.current_dir(dir);',
     '                let _ = dir;',
     'axon-os', '--test legacy_adapter', 'the_interpreter_child_is_built_from_an_empty_environment_and_the_jobs_directory'),
    ('M2320', 'DUMPABLE (eqgate3): the PSV runner makes itself non-dumpable', 'crates/axon-psv/src/bin/axon-psv-runner.rs',
     '    unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) }',
     '    0',
     'axon-psv', '--bin axon-psv-runner', 'tests::the_real_prctl_makes_the_runner_non_dumpable'),
    ('M2321', 'DUMPABLE (eqgate3): the root helper makes itself non-dumpable', 'crates/axon-fabric/src/privileged_launcher.rs',
     '        libc::prctl(\n            libc::PR_SET_DUMPABLE,\n            0 as libc::c_ulong,',
     '        let _ = 0;\n        libc::prctl(\n            libc::PR_SET_DUMPABLE,\n            1 as libc::c_ulong,',
     'axon-fabric', '--lib', 'privileged_launcher::tests::harden_makes_the_helper_non_dumpable'),
    ('M2322', 'PREPARE (eqgate3): a candidate tree is the version the request names', 'crates/axon-fabric/src/psv.rs',
     '    if cand_tree != req.workspace_version_ref.as_str() {',
     '    if false && cand_tree != req.workspace_version_ref.as_str() {',
     'axon-fabric', '--test one_read', 'prepare_refuses_a_tree_that_is_not_the_version_it_is_told_it_is'),
    ('M2323', 'PREPARE (eqgate3): a suite tree is the registered suite version', 'crates/axon-fabric/src/psv.rs',
     '    if suite_tree != i.suite_version {',
     '    if false && suite_tree != i.suite_version {',
     'axon-fabric', '--test one_read', 'prepare_refuses_a_tree_that_is_not_the_version_it_is_told_it_is'),
    ('M2324', "JOURNAL (eqgate3): a journal's sequence numbers do not skip", 'crates/axon-fabric/src/journal.rs',
     '            if line.seq != seq + 1 {',
     '            if false && line.seq != seq + 1 {',
     'axon-fabric', '--test journal', 'g13_a_journal_with_a_gap_in_its_sequence_is_corrupt'),
    ('M2325', 'STORE (eqgate3): the store guard checks every component for `..`', 'crates/axon-loop/src/store.rs',
     '        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {',
     '        if false && rel.components().any(|c| !matches!(c, Component::Normal(_))) {',
     'axon-loop', '--test open_flag_sites', 'the_store_guard_refuses_a_path_that_leaves_the_store'),
    ('M2326', 'LAUNCHER (eqgate3): an operator directory is a directory', 'crates/axon-fabric/src/privileged_launcher.rs',
     '    if !is_dir(st) {\n        return Err(format!("{} is not a directory"',
     '    if false && !is_dir(st) {\n        return Err(format!("{} is not a directory"',
     'axon-fabric', '--lib', 'privileged_launcher::tests::an_operator_directory_that_is_a_file_is_refused'),
    ('M2327', 'SCHEMA (eqgate3): the schema walker refuses a non-integer number', 'crates/axon-loop-contracts/src/schema.rs',
     '    let Some(x) = as_i128(n) else {\n        return Err(fail(path, "non-integer number"));',
     '    let Some(x) = as_i128(n) else {\n        return Ok(());',
     'axon-loop-contracts', '--lib', 'schema::tests::the_walker_refuses_a_non_integer_number'),
    ('M2328', 'EPOCH (eqgate3): an authority epoch past 2^53-1 is refused', 'crates/axon-loop-contracts/src/ids.rs',
     '        if v > MAX_INTEGER {\n            return Err(shape(format!("authority epoch',
     '        if false && v > MAX_INTEGER {\n            return Err(shape(format!("authority epoch',
     'axon-loop-contracts', '--lib', 'ids::tests::an_authority_epoch_past_the_json_safe_range_is_refused'),
    ('M2329', 'GUEST (eqgate3): the guest installs no-new-privs before its seccomp filter', 'crates/axon-guest-init/src/main.rs',
     '        let r = libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1usize, 0usize, 0usize, 0usize);',
     '        let r = 0;',
     'axon-guest-init', '--bin axon-guest-init', 'tests::a_seccomp_filter_is_installed_with_no_new_privs'),
    ('M2330', 'GUEST (eqgate3): the guest installs its seccomp filter', 'crates/axon-guest-init/src/main.rs',
     '        let r = libc::prctl(\n            libc::PR_SET_SECCOMP,\n            libc::SECCOMP_MODE_FILTER as libc::c_ulong,',
     '        let _ = 0;\n        let r = libc::prctl(\n            libc::PR_SET_SECCOMP,\n            libc::SECCOMP_MODE_STRICT as libc::c_ulong - 1,',
     'axon-guest-init', '--bin axon-guest-init', 'tests::a_seccomp_filter_is_installed_with_no_new_privs'),
    ('M2331', 'GUEST (eqgate3): the PID-1 supervisor forwards a signal to its child', 'crates/axon-guest-init/src/main.rs',
     '        unsafe { libc::kill(pid, sig) };',
     '        let _ = (pid, sig);',
     'axon-guest-init', '--bin axon-guest-init', 'tests::the_supervisor_forwards_term_and_int_to_its_child'),
    ('M2332', 'GUEST (eqgate3): the PID-1 supervisor handles SIGTERM', 'crates/axon-guest-init/src/main.rs',
     '        libc::signal(\n            libc::SIGTERM,\n            forward_signal as *const () as libc::sighandler_t,\n        );',
     '        let _ = libc::SIGTERM;',
     'axon-guest-init', '--bin axon-guest-init', 'tests::the_supervisor_forwards_term_and_int_to_its_child'),
]

MUTATIONS += [
    ('M2333', '(eqgate3) GUEST: the PID-1 supervisor handles SIGINT', 'crates/axon-guest-init/src/main.rs',
     '        libc::signal(\n            libc::SIGINT,\n            forward_signal as *const () as libc::sighandler_t,\n        );',
     '        let _ = libc::SIGINT;',
     'axon-guest-init', '--bin axon-guest-init', 'tests::the_supervisor_forwards_term_and_int_to_its_child'),
    ('M2334', '(eqgate3) LAUNCHER: is_dir says a directory is one', 'crates/axon-fabric/src/privileged_launcher.rs',
     '    st.st_mode & libc::S_IFMT == libc::S_IFDIR',
     '    st.st_mode & libc::S_IFMT != libc::S_IFDIR',
     'axon-fabric', '--lib', 'privileged_launcher::tests::an_operator_directory_that_is_a_file_is_refused'),
    ('M2335', '(eqgate3) LAUNCHER: the helper prints its reply', 'crates/axon-fabric/src/bin/axon-protected-launcher.rs',
     '        let _ = writeln!(out, "{r}");',
     '        let _ = &r;',
     'axon-fabric', '--test privileged_launcher', 'the_root_launcher_runs_with_null_stdio_in_the_root_directory'),
    ('M2336', '(eqgate3) CUSTODIAN: pass_pidfd arms SO_PASSPIDFD', 'crates/axon-fabric/src/custodian.rs',
     '        libc::setsockopt(\n            fd,\n            libc::SOL_SOCKET,\n            libc::SO_PASSPIDFD,\n            &one',
     '        let _ = 0;\n        libc::setsockopt(\n            fd,\n            libc::SOL_SOCKET,\n            libc::SO_KEEPALIVE,\n            &one',
     'axon-fabric', '--lib', 'custodian::tests::pass_pidfd_arms_so_passpidfd_on_the_socket'),
    ('M2337', '(eqgate3) JOURNAL: a second writer is locked out', 'crates/axon-fabric/src/journal.rs',
     '        let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };',
     '        let rc = 0 * unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };',
     'axon-fabric', '--test journal', 'a_second_writer_is_locked_out'),
    ('M2338', 'COVERAGE GATE (eqgate3): an exemption every covered site makes stale is refused', 'scripts/v022_refusal_coverage.py',
     '        elif cov and not sole:\n            bad.append(',
     '        elif False:\n            bad.append(',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_exemption_inside_a_site_a_row_covers_is_stale'),
    ('M2339', "(eqgate3) ENV: the PSV check child's module path is its suite then its candidate", 'crates/axon-psv/src/runner.rs',
     '        .env(\n            "AXON_PATH",',
     '        .env_remove("AXON_PATH")\n        .env(\n            "AXON_EQ_NOOP",',
     'axon-psv', '--test runner', 'the_check_child_runs_in_the_suite_with_only_its_own_environment_and_stdio'),
]

MUTATIONS += [
    ('M2271', 'COVERAGE GATE (eqgate3): an exemption may cite only a row the registry holds', 'scripts/v022_refusal_coverage.py',
     '        if gone:\n            bad.append(',
     '        if False and gone:\n            bad.append(',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_exemption_citing_a_row_that_does_not_exist_is_refused'),
]

MUTATIONS += [
    ('M2272', 'EXEMPTION REFUTED (eqgate3): an import over its byte quota is refused', 'crates/axon-fabric/src/workspace.rs',
     '            if bytes > quota.bytes {',
     '            if false && bytes > quota.bytes {',
     'axon-fabric', '--test workspace', 'refuses_byte_quota_overflow'),
]

MUTATIONS += [
    ('M2274', 'EXEMPTION REFUTED (eqgate3): materialize never writes into an existing destination', 'crates/axon-fabric/src/workspace.rs',
     '        if dest.exists() || std::fs::symlink_metadata(dest).is_ok() {',
     '        if false && (dest.exists() || std::fs::symlink_metadata(dest).is_ok()) {',
     'axon-fabric', '--test workspace', 'publish_is_write_once_and_materialize_round_trips'),
]

MUTATIONS += [
    ('M2278', 'EXEMPTION REFUTED (eqgate3): an overflowing reservation is refused, not wrapped', 'crates/axon-fabric/src/journal.rs',
     '            if used.checked_add(want).is_none() {',
     '            if false && used.checked_add(want).is_none() {',
     'axon-fabric', '--test journal', 'an_overflowing_reservation_is_refused_not_wrapped'),
]


MUTATIONS += [
    ('M2370', '(PSV1F, am94) a `&mut` write-through value is cast to its declared parameter type at the seal edge back', 'crates/axon-core/src/interp.rs',
     '                if let Err(why) = self.cast(&mut outs[i], &p.ty, &cx) {',
     '                if let Err(why) = self.cast(&mut outs[i], &p.ty, &cx).or(Ok::<(), String>(())) {',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_mut_write_through_value_is_cast_at_the_seal_edge_back'),
    ('M2371', '(PSV1F, am94) a `&mut` write-through value is judged by what the operator held there', 'crates/axon-core/src/interp.rs',
     'self.replaced_ok_top(old, &outs[i])',
     'self.replaced_ok_top(old, &outs[i]).or(Ok::<(), String>(()))',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_mut_write_through_value_is_judged_by_what_the_operator_held'),
    ('M2372', '(PSV1F, am94) the `&mut` edge-back cast runs on every outcome, not only a normal return', 'crates/axon-core/src/interp.rs',
     '        if crossing {\n            let cx = self.fn_cx(f).strict(true);',
     '        if crossing && result.is_ok() {\n            let cx = self.fn_cx(f).strict(true);',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_mut_value_is_cast_when_an_operator_handler_aborts_the_call'),
    ('M2373', '(PSV1F, am94) the `&mut` edge-back cast is strict (an undetermined type parameter is refused)', 'crates/axon-core/src/interp.rs',
     '            let cx = self.fn_cx(f).strict(true);',
     '            let cx = self.fn_cx(f).strict(false);',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_mut_write_through_value_is_cast_at_the_seal_edge_back'),
    ('M2374', '(PSV1F, am94) a variable lent as `&mut x` is open in the dispatch analysis', 'crates/axon-core/src/interp/pin.rs',
     '                            facts.push((r.to_string(), Fact::Unpinned));',
     '                            let _ = r;',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_mut_operand_is_never_determined_by_the_pin_analysis'),
    ('M2375', '(PSV1F, am94) a union annotation does not pin the receiver type', 'crates/axon-core/src/interp/pin.rs',
     '            T::Union(_) => false,',
     '            T::Union(xs) => xs.iter().all(|a| go(a, seen)),',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_mut_operand_is_never_determined_by_the_pin_analysis'),
    ('M2376', '(PSV1F, am94) a sealed frame cannot take an operator fn as a value', 'crates/axon-core/src/interp/eval.rs',
     '                    self.seal_call(f)?;\n                    Ok(fn_value(\n',
     '                    Ok(fn_value(\n',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_sealed_frame_cannot_take_an_operator_fn_as_a_value'),
    ('M2470', '(PSV1G, am96) the identifier arm reads a global through the global-read edge', 'crates/axon-core/src/interp/eval.rs',
     '                } else if let Some(v) = self.global_ref(name)? {\n                    Ok(v.clone())',
     '                } else if let Some(v) = self.globals.get(name) {\n                    Ok(v.clone())',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path'),
    ('M2471', '(PSV1G, am96) the field-access fast path reads a global through the global-read edge', 'crates/axon-core/src/interp/eval.rs',
     '                        None => match self.global_ref(name)? {\n                            Some(v) => v,',
     '                        None => match self.globals.get(name) {\n                            Some(v) => v,',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path'),
    ('M2472', '(PSV1G, am96) the index fast path reads a global through the global-read edge', 'crates/axon-core/src/interp/eval.rs',
     '                            None => self.global_ref(name)?,\n',
     '                            None => self.globals.get(name),\n',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path'),
    ('M2473', '(PSV1G, am96) a call of a module-level closure constant reads it through the global-read edge', 'crates/axon-core/src/interp/eval.rs',
     'if let Some(c @ Value::Closure { .. }) = self.global_ref(name)? {',
     'if let Some(c @ Value::Closure { .. }) = self.globals.get(name) {',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path'),
    ('M2474', '(PSV1G, am96) the one global lookup applies the global-read edge', 'crates/axon-core/src/interp.rs',
     '            Some(v) => {\n                self.seal_global(name)?;\n                Ok(Some(v))',
     '            Some(v) => {\n                Ok(Some(v))',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path'),
    ('M2475', "(PSV1G, am96) a candidate callee's sandbox_run result is cast to the declared i64 at the seal crossing", 'crates/axon-core/src/interp/builtins.rs',
     '                    Ok(v) if self.seal.active && !self.frame_sealed.get() && self.fn_is_sealed(f) => {',
     '                    Ok(v) if false && self.seal.active && !self.frame_sealed.get() && self.fn_is_sealed(f) => {',
     'axon-core', '--no-default-features --lib', 'interp::tests::sandbox_run_results_are_cast_at_the_seal_crossing'),
    ('M2476', "(PSV1G, am96) an operator fn value is not exempt from the closure-argument edge (only a sealed frame's own is marked)", 'crates/axon-core/src/interp/eval.rs',
     '                        self.seal.active && self.frame_sealed.get(),\n                    ))',
     '                        self.seal.active,\n                    ))',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_candidates_own_fn_value_takes_arguments_and_an_operators_still_does_not'),
    ('M2477', '(PSV1G, am96) a candidate fn value does not replace an operator closure a dict held', 'crates/axon-core/src/interp/conform.rs',
     '                    c.contains_key(SEALED_CLOSURE_MARK) || c.contains_key(SEALED_FNVAL_MARK)',
     '                    c.contains_key(SEALED_CLOSURE_MARK)',
     'axon-core', '--no-default-features --lib', 'interp::tests::a_candidates_own_fn_value_takes_arguments_and_an_operators_still_does_not'),
    ('M2478', '(PSV1G, am96) a unary `-x` / `~x` on a fixed-width integer is judged by the width arm', 'crates/axon-core/src/interp.rs',
     '        if !matches!(v, Value::SizedInt { .. }) || !matches!(op, UnaryOp::Neg | UnaryOp::BitNot) {',
     '        if true {',
     'axon-core', '--no-default-features --lib', 'interp::tests::operator_unary_arithmetic_never_runs_at_a_width_the_candidate_chose'),
    ('M2479', '(PSV1G, am96) a unary site is determined only when its operand is', 'crates/axon-core/src/interp/pin.rs',
     '            Expr::UnaryOp { op, operand } if ctx.det(operand, &local, &bound) => {',
     '            Expr::UnaryOp { op, operand } if true => {',
     'axon-core', '--no-default-features --lib', 'interp::tests::operator_unary_arithmetic_never_runs_at_a_width_the_candidate_chose'),
    ('M2480', "(PSV1G, am96) the unary width arm refuses an undetermined operand", 'crates/axon-core/src/interp.rs',
     '        if !self.pins.determined_unary(self.pin_fn.get(), op, operand) {',
     '        if false && !self.pins.determined_unary(self.pin_fn.get(), op, operand) {',
     'axon-core', '--no-default-features --lib', 'interp::tests::operator_unary_arithmetic_never_runs_at_a_width_the_candidate_chose'),
]


def cargo_build_tests(package, target, env=""):
    """Build the tests a cell will run, ALONE: (ok, output). A compile error is
    the outcome of THIS cargo invocation, never a string found in a test's
    output. Tests build and exec workspace binaries themselves
    (script_spawn::workspace_bin), and a nested build that fails prints
    `could not compile` / `error[E…]` into the test's output; a label taken
    from the whole output read that as the row's own compile error (M278's
    baseline, C9 round 4; amendment 59)."""
    cmd = (
        f"source scripts/lib_bounded_run.sh && {UNSET_AMBIENT}"
        f"{env}bounded_run {MEM} 1800 cargo test -q -p {package} {target} --no-run"
    )
    r = subprocess.run(["bash", "-c", cmd], cwd=ROOT, capture_output=True, text=True)
    return r.returncode == 0, r.stdout + r.stderr


def cargo_test(package, target, test):
    built, bout = cargo_build_tests(package, target)
    if not built:
        # The build itself failed: a compiler error, or the build was killed
        # (memory bound, timeout) — named apart, so an environment failure
        # does not read as a broken edit.
        kind = "compile_error" if ("could not compile" in bout or "error[E" in bout) else "build_failed"
        return kind, bout
    cmd = (
        f"source scripts/lib_bounded_run.sh && {UNSET_AMBIENT}"
        f"bounded_run {MEM} 1800 cargo test -q -p {package} {target} -- --exact {test}"
    )
    r = subprocess.run(["bash", "-c", cmd], cwd=ROOT, capture_output=True, text=True)
    out = r.stdout + r.stderr
    # The tests are built: compiler text in this output is the TEST's (a
    # nested build it ran), judged as the test's pass or failure below.
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


def row_good(baseline, result, unrestored=None):
    """A row is good when its baseline passed, it was KILLED by its own attack,
    and the interpreter was restored (amendment 74: the one definition the run
    and --merge share)."""
    return baseline == "passed" and result == "killed" and not unrestored


_HOST = {}


def host_identity():
    """The machine and toolchain a run executed on (amendment 67/81). ONE
    definition for the mutation run and the paired-disable run: `toolchain` is
    what must agree for shards to combine -- rustc and cargo (with their LLVM)
    and the system LLVM inkwell links; hostname, kernel, cores and memory are
    recorded, not compared."""
    if not _HOST:
        def out(cmd):
            r = subprocess.run(cmd, shell=True, cwd=ROOT, capture_output=True, text=True)
            return r.stdout.strip() if r.returncode == 0 else f"unavailable ({cmd})"
        mem = out("awk '/MemTotal/ {print $2 \" kB\"}' /proc/meminfo")
        _HOST.update({
            "hostname": os.uname().nodename, "kernel": f"{os.uname().sysname} {os.uname().release}",
            "nproc": os.cpu_count(), "mem_total": mem,
            "toolchain": {"rustc": out("rustc -vV"), "cargo": out("cargo -V"),
                          "llvm_system": out("llvm-config-17 --version || llvm-config --version")},
        })
    return dict(_HOST)


def shard_toolchain_problem(entries):
    """Amendment 81: the ONE refusal both `--merge` (mutation shards) and
    `--join` (paired-disable records) apply. `entries` is [(label, host,
    pinned)]: `host` the record's environment.host (None/ill-formed is itself
    refused: a shard that does not say where it ran is not evidence), `pinned`
    extra facts that must agree too (the interpreter's digest, euid, ...).
    Shards may run on several HOSTS, never on several toolchains. Returns the
    refusal text, or None."""
    groups = {}
    for label, host, pinned in entries:
        if not isinstance(host, dict) or not isinstance(host.get("toolchain"), dict):
            return f"record {label} does not record the host and toolchain it ran on"
        key = json.dumps({"toolchain": host["toolchain"], **(pinned or {})}, sort_keys=True)
        groups.setdefault(key, []).append(label)
    if len(groups) > 1:
        return "records ran on different toolchains: " + "; ".join(
            f"{json.loads(k)} for {sorted(v)[:5]}" for k, v in groups.items())
    return None


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
    # The registry each shard ran from must be this one: the same blobs, and
    # every row's old/new text the same as the row judged here (C9 round 4).
    here = registry_blobs()
    if uncommitted():
        sys.exit("refused: --merge judges the shards against this tree's registry; the tree is not clean")
    for p, d in zip(parts, docs):
        if d.get("registry_blobs") != here:
            sys.exit(f"refused: shard {p} ran from registry/marker blobs {d.get('registry_blobs')}, "
                     f"not this tree's {here}")
        if d.get("tree_clean") is not True:
            sys.exit(f"refused: shard {p} does not record a clean tree")
        by_id = {m[0]: m for m in MUTATIONS}
        for r in d.get("mutations", []):
            row = by_id.get(r.get("id"))
            if row is None or {k: r.get(k) for k in ("old_sha256", "new_sha256")} != row_digest(row):
                sys.exit(f"refused: shard {p} ran {r.get('id')} with an edit that is not this registry's row")
        if d.get("only") is not None:
            sys.exit(f"refused: {d['only']} is a sample (--only), not a shard")
        if (d.get("shard") or {}).get("of") != len(docs):
            sys.exit(f"refused: a shard of {(d.get('shard') or {}).get('of')} merged as one of {len(docs)}")
    # Amendment 81: shards agree on the toolchain (rustc, cargo, LLVM), the
    # interpreter binary's digest and the uid/etc-axon they ran under, and
    # each records the host it ran on -- the same refusal --join applies.
    why = shard_toolchain_problem([
        (p, (d.get("environment") or {}).get("host"),
         {"axon_bin_sha256": (d.get("toolchain") or {}).get("axon_bin_sha256", "unrecorded"),
          "rustc": (d.get("toolchain") or {}).get("rustc"), "cargo": (d.get("toolchain") or {}).get("cargo"),
          "euid": (d.get("environment") or {}).get("euid"),
          "etc_axon_present": (d.get("environment") or {}).get("etc_axon_present")})
        for p, d in zip(parts, docs)])
    if why:
        sys.exit("refused: " + why)
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
    # Amendment 74: a shard's `all_killed` is recomputed from its recorded rows
    # with the run's own predicate (row_good), never taken from the label. A
    # shard claiming all_killed over a row that is not killed is refused.
    for p, d in zip(parts, docs):
        notgood = [r["id"] for r in d["mutations"]
                   if not row_good(r.get("baseline"), r.get("result"), r.get("interpreter_not_restored"))]
        if d["all_killed"] and notgood:
            sys.exit(f"refused: shard {p} claims all_killed but its recorded rows {notgood} are not "
                     "killed from a passing baseline")
    ok = all(d["all_killed"] for d in docs)
    doc = {"schema": "axon-v022-mutation-run/3", "gate": base["gate"], "scope": base["scope"],
           "commit": base["commit"], "registry_blobs": here, "tree_clean": True,
           "environment": [d.get("environment") for d in docs],
           "toolchain": [d["toolchain"] for d in docs],
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

      LIBRARY_PRIMITIVE  (ruling R1, amendment 64) a guard of a public
                         primitive, dominated on every production route,
                         killed only by a direct library test: run, and its
                         library-test kill required, but reported in its own
                         class and NEVER counted among the killed
      SIBLING_ONLY       (amendment 64) an edit that is only a member of a
                         retired row's guard set; never run as a row

    Returns False when any class other than KILLED/EQUIVALENT/STALE is
    non-empty, a LIBRARY_PRIMITIVE row's library test did not kill it, or a
    retirement record does not hold against the tree."""
    lib_rows = [r for r in rows if r["id"] in LIBRARY_PRIMITIVE]
    rows = [r for r in rows if r not in lib_rows]
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
    lib_killed = sorted(r["id"] for r in lib_rows if r["result"] == "killed" and r["baseline"] == "passed")
    lib_bad = sorted(r["id"] for r in lib_rows if r["id"] not in lib_killed)
    lib_gone = sorted(m for m in LIBRARY_PRIMITIVE if not applies(m))
    print(f"LIBRARY_PRIMITIVE (ruling R1: killed only by a direct library test; NEVER counted killed): "
          f"{len(lib_killed)}/{len(lib_rows)} library-test kills {lib_killed}"
          + (f"  <-- LIBRARY TEST DID NOT KILL: {lib_bad}" if lib_bad else "")
          + (f"  <-- guard absent: {lib_gone}" if lib_gone else ""))
    sib_gone = sorted(m for m in SIBLING_ONLY if not applies(m))
    print(f"SIBLING_ONLY (a guard-set member only; never a row, never counted killed): "
          f"{len(SIBLING_ONLY)} {sorted(SIBLING_ONLY)}"
          + (f"  <-- edit does not apply: {sib_gone}" if sib_gone else ""))
    if LEGACY_EQUIV:
        print(f"Retired LEGACY (unaudited): {len(LEGACY_EQUIV)} {sorted(LEGACY_EQUIV)}")
    print(f"Unexpected survivors: {len(survivors)}{(' '+str(survivors)) if survivors else ''}")
    print(f"Active rows stale/unapplied: {len(stale)}{(' '+str(stale)) if stale else ''}")
    return not (weak or survivors or stale or eq_gone or stale_live or stale_bad or LEGACY_EQUIV
                or lib_bad or lib_gone or sib_gone)


def main():
    if sys.argv[1:2] == ["--check-status"]:
        import v022_mutation_status as ms
        path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(ROOT, ms.STATUS_PATH)
        try:
            found = ms.problems(json.load(open(path)), sh("git rev-parse HEAD").stdout.strip(), sys.modules[__name__])
        except (OSError, ValueError) as e:
            found = [f"cannot read {path}: {e}"]
        for p_ in found:
            print(f"BAD {p_}")
        print(f"mutation-run status {path}: {'REFUSED' if found else 'a merged, current, complete run'}")
        sys.exit(1 if found else 0)
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
    dirty = uncommitted()
    if dirty:
        sys.exit(f"refused: uncommitted changes in the tree — a mutation run is evidence about a commit\n{dirty}")
    blobs = registry_blobs()
    commit = sh("git rev-parse HEAD").stdout.strip()
    # The Fabric integration tests exec the `axon` interpreter from the target
    # dir: build it from THIS tree first, so no baseline or kill rests on a
    # stale binary, and record which one it was.
    axon_bin = os.path.join(cargo_target_dir(), "debug", "axon")
    toolchain = {
        "rustc": sh("rustc -V").stdout.strip(),
        "cargo": sh("cargo -V").stdout.strip(),
        "axon_bin_sha256": clean_interpreter(axon_bin),
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
            b_outcome, b_out = cargo_test(pkg, target, test)
            baselines[key] = (b_outcome, None if b_outcome == "passed"
                              else keep_output(f"baseline-{mid}", b_out))
        base, base_kept = baselines[key]
        kept = None
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
                if outcome != "passed":
                    kept = keep_output(f"cell-{mid}", out)
            finally:
                with open(path, "w") as f:
                    f.write(original)
            if sha(path) != before:
                sys.exit(f"FATAL: {rel} not restored after {mid}")
            # An integration-test kill of an axon-core mutant rebuilds the
            # shared interpreter binary FROM THE MUTANT. Rebuild it from the
            # restored tree, or every later Fabric row runs a mutated
            # interpreter. A test that ran scripts may also have left mutant
            # binaries in the WORKSPACE target dir: remove them.
            scrubbed = rel.startswith("crates/") and spawns_scripts(pkg, target)
            if scrubbed:
                scrub_workspace_binaries()
            # Any axon-core integration test run rebuilds `axon` with the test
            # build's feature set (dev-dependency unification), whichever file
            # the row guards (a scripts/ row judged by an axon-core test too).
            # (The interpreter is rebuilt and byte-compared after EVERY row,
            # below: restore_interpreter.)
            if scrubbed and os.path.realpath(workspace_target_dir()) == os.path.realpath(cargo_target_dir()):
                # The scrub emptied THIS run's target dir too: rebuild what its
                # later cells exec (the cortex CLI tests, the PSV dev tool).
                for cmd in ("cargo build -q -p axon-cortex --bins", "cargo build -q -p axon-psv --example psv_dev"):
                    r = subprocess.run(["bash", "-c", f"source scripts/lib_bounded_run.sh && bounded_run {MEM} 1800 {cmd}"],
                                       cwd=ROOT, capture_output=True, text=True)
                    if r.returncode != 0:
                        sys.exit(f"FATAL: could not rebuild ({cmd}) after {mid}")
        # Every row ends on the run's interpreter, rebuilt from the restored
        # tree and byte-compared to the one the run started with (amendment
        # 59). One that cannot be restored is THIS row's failure, and no later
        # row is judged on another interpreter: the run stops here.
        unrestored = restore_interpreter(axon_bin, toolchain["axon_bin_sha256"])
        good = row_good(base, result, unrestored)
        ok &= good
        results.append({"id": mid, "guard": guard, "file": rel, "package": pkg,
                        **({"status": "LIBRARY_PRIMITIVE"} if mid in LIBRARY_PRIMITIVE else {}),
                         **row_digest((mid, guard, rel, old, new)),
                         "target": target, "test": test, "baseline": base, "result": result,
                         "attack_marker": ATTACK_MARKERS.get(mid),
                         "kill_evidence": evidence,
                         **({"baseline_output": base_kept} if base_kept else {}),
                         **({"cell_output": kept} if kept else {}),
                         **({"interpreter_not_restored": unrestored} if unrestored else {})})
        print(f"{'OK ' if good else 'BAD'} {mid} baseline={base} {result}"
              f"{' (LIBRARY_PRIMITIVE: not counted killed)' if mid in LIBRARY_PRIMITIVE else ''}  {guard}"
              + (f"  INTERPRETER NOT RESTORED: {unrestored}" if unrestored else ""), flush=True)
        if unrestored:
            break
    # The run must end on the interpreter it started with.
    if toolchain["axon_bin_sha256"] is not None and sha(axon_bin) != toolchain["axon_bin_sha256"]:
        print(f"BAD interpreter binary changed during the run ({axon_bin})", flush=True)
        ok = False
    doc = {"schema": "axon-v022-mutation-run/3", "gate": "G01" if scope == "g01" else scope,
           "scope": scope, "commit": commit, "registry_blobs": blobs,
           "tree_clean": True,  # refused at the start otherwise
           "environment": {"euid": os.geteuid(), "etc_axon_present": os.path.isdir("/etc/axon"),
                           "unset": list(AMBIENT_BINARY_VARS), "host": host_identity()},
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

# ── C9 round 8, EQGATE4 (amendment 95; M2400-M2469) ──────────────────────────
# A decision expressed as a VALUE (the uid a check runs as, the digest a runner is
# held to, the environment a guest's child is given, an age bound, a table), as an
# atomic directory creation, or as one TERM of a compound guard. Each row's test
# observes the production value or the guard itself; its marker is the attack
# getting through, never a setup failure.
_PRN = 'crates/axon-psv/src/bin/axon-psv-runner.rs'
_GIR = '--test policy_refusals'
_GIENV = 'the_workload_receives_exactly_the_policys_environment'
_PRT = 'tests::the_guest_runs_the_check_as_nobody_and_holds_the_manifest_to_the_cmdline_digest'
MUTATIONS += [
    ('M2400', "VALUE (eqgate4): the guest runner's check identity is nobody (65534), not root", _PRN,
     'const TEST_UID: u32 = 65534;', 'const TEST_UID: u32 = 0;',
     'axon-psv', '--bin axon-psv-runner', _PRT),
    ('M2401', "VALUE (eqgate4): the guest runner drops privilege for the check (drop is Some)", _PRN,
     '        drop: Some((TEST_UID, TEST_GID)),', '        drop: None,',
     'axon-psv', '--bin axon-psv-runner', _PRT),
    ('M2402', "VALUE (eqgate4): the guest runner is held to the manifest digest the cmdline names", _PRN,
     '        expected_manifest_sha256: expected,', '        expected_manifest_sha256: String::new(),',
     'axon-psv', '--bin axon-psv-runner', _PRT),
    ('M2403', "ENV (eqgate4): the guest's workload gets the policy's AXON_BUDGET_TOKENS cap", _GI,
     '        if let Some(tokens) = p.budget_tokens {\n            env::set_var("AXON_BUDGET_TOKENS", tokens.to_string());\n        }\n', '',
     'axon-guest-init', _GIR, _GIENV),
    ('M2404', "ENV (eqgate4): the guest's workload gets the policy's AXON_PRINCIPAL", _GI,
     '        if let Some(principal) = &p.principal {\n            env::set_var("AXON_PRINCIPAL", principal);\n        }\n', '',
     'axon-guest-init', _GIR, _GIENV),
    ('M2405', "ENV (eqgate4): the guest's workload gets the policy's AXON_RUN_ID", _GI,
     '        if let Some(run_id) = &p.run_id {\n            env::set_var("AXON_RUN_ID", run_id);\n        }\n', '',
     'axon-guest-init', _GIR, _GIENV),
    ('M2406', "ENV (eqgate4): the guest's workload gets the policy's AXON_ALLOWED_EFFECTS ceiling", _GI,
     '        if let Some(effects) = &p.allowed_effects {\n            env::set_var("AXON_ALLOWED_EFFECTS", effects.join(","));\n        }\n', '',
     'axon-guest-init', _GIR, _GIENV),
    ('M2407', "ENV (eqgate4): the guest's workload gets the policy's AXON_SOURCE_HASH", _GI,
     '        if let Some(hash) = &p.source_hash {\n            env::set_var("AXON_SOURCE_HASH", hash);\n        }\n', '',
     'axon-guest-init', _GIR, _GIENV),
    ('M2408', 'DIR (eqgate4): a run dir is created NEW (create_dir, not create_dir_all)', _FS,
     '        let p = runs.join(key);\n        std::fs::create_dir(&p)\n', '        let p = runs.join(key);\n        std::fs::create_dir_all(&p)\n',
     'axon-fabric', '--lib', 'submit::tests::a_run_dir_is_never_made_over_an_existing_directory'),
    ('M2409', "DIR (eqgate4): prepare creates the guest job dir NEW", 'crates/axon-fabric/src/psv.rs',
     '    std::fs::create_dir(i.job_dir).map_err(|e| format!("job dir: {e}"))?;',
     '    std::fs::create_dir_all(i.job_dir).map_err(|e| format!("job dir: {e}"))?;',
     'axon-fabric', '--test one_read', 'prepare_never_makes_the_job_dir_over_a_directory_already_there'),
    ('M2410', "DIR (eqgate4): the helper's snapshot creates each staging leaf NEW", _PL,
     '        std::fs::create_dir(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;',
     '        std::fs::create_dir_all(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;',
     'axon-fabric', '--lib', 'privileged_launcher::tests::a_snapshot_of_the_inputs_never_enters_an_existing_staging_leaf'),
    ('M2411', "DIR (eqgate4): the helper's tree copy creates each directory NEW", _PL,
     '                std::fs::create_dir(&shown).map_err(|e| format!("{}: {e}", shown.display()))?;',
     '                std::fs::create_dir_all(&shown).map_err(|e| format!("{}: {e}", shown.display()))?;',
     'axon-fabric', '--lib', 'privileged_launcher::tests::a_snapshot_never_copies_into_a_directory_that_already_exists'),
    ('M2412', "DIR (eqgate4): the in-uid observer program's work dir is created NEW", 'crates/axon-fabric/src/observer.rs',
     '    std::fs::create_dir(work).map_err(|e| format!("observation dir: {e}"))\n',
     '    std::fs::create_dir_all(work).map_err(|e| format!("observation dir: {e}"))\n',
     'axon-fabric', '--lib', 'observer::tests::the_observer_programs_work_directory_is_never_an_existing_one'),
    ('M2413', "TERM (eqgate4): a custodian executable a STRANGER uid owns is never the pinned program", _CU,
     '    if !m.is_file() || (m.uid() != 0 && m.uid() != me) || m.mode() & 0o022 != 0 {',
     '    if !m.is_file() || false || m.mode() & 0o022 != 0 {',
     'axon-fabric', '--test privileged_launcher', 'a_custodian_executable_owned_by_a_stranger_uid_is_refused'),
    ('M2414', "TERM (eqgate4): the observer key must be a regular file (a FIFO holding the key is refused)", _OS,
     '    if !meta.file_type().is_file() || meta.uid() != euid || meta.mode() & 0o277 != 0 {',
     '    if false || meta.uid() != euid || meta.mode() & 0o277 != 0 {',
     'axon-fabric', _TO, 'an_observer_key_that_is_a_fifo_is_refused_even_when_it_holds_the_key'),
    ('M2415', "CONST (eqgate4): an observer that names no max_age_s gets the five-minute default", 'crates/axon-fabric/src/observer.rs',
     'pub const DEFAULT_OBSERVATION_MAX_AGE_S: u64 = 300;', 'pub const DEFAULT_OBSERVATION_MAX_AGE_S: u64 = 86_400_000;',
     'axon-fabric', '--test protected_host', 'an_observer_that_names_no_max_age_gets_the_five_minute_default'),
    ('M2416', "CONST (eqgate4): every field of CERT_FIELDS is bound (a duplicate entry leaves one unbound)", 'crates/axon-fabric/src/readiness.rs',
     '    "observation_sha256",\n    "verifier_key_id",', '    "observer_key_id",\n    "verifier_key_id",',
     'axon-fabric', '--test readiness', 'a_record_omitting_any_bound_field_is_refused_as_missing_it'),
    ('M2417', "VALUE (eqgate4): the guest runner's check group is nobody (65534), not root", _PRN,
     'const TEST_GID: u32 = 65534;', 'const TEST_GID: u32 = 0;',
     'axon-psv', '--bin axon-psv-runner', _PRT),
    ('M2418', 'DIR (eqgate4): the Fabric-private inputs dir is created NEW (not recursive)', 'crates/axon-fabric/src/psv.rs',
     '        .mode(0o700)\n        .create(&dir)\n        .map_err(|e| format!("private inputs dir {}: {e}", dir.display()))?;',
     '        .mode(0o700)\n        .recursive(true)\n        .create(&dir)\n        .map_err(|e| format!("private inputs dir {}: {e}", dir.display()))?;',
     'axon-fabric', '--test submit', 'the_private_inputs_dir_is_never_an_existing_one'),
]

_RCGT = 'a_value_handed_to_a_privilege_primitive_or_a_directory_creation_is_a_site'
MUTATIONS += [
    ('M2420', 'COVERAGE GATE (eqgate4): create_dir / create_dir_all is a site', _RCG,
     '    r"\\bcreate_dir(?:_all)?\\(|"\n', '    r"\\bcreate_dir_never\\(|"\n',
     'axon-core', _RCT, _RCGT),
    ('M2421', 'COVERAGE GATE (eqgate4): create_dir_all (the form that accepts an existing directory) is a site', _RCG,
     '    r"\\bcreate_dir(?:_all)?\\(|"\n', '    r"\\bcreate_dir\\(|"\n',
     'axon-core', _RCT, _RCGT),
    ('M2422', 'COVERAGE GATE (eqgate4): DirBuilder is a site', _RCG,
     '    r"\\bDirBuilder\\b|\\blibc::mkdir\\(|"\n', '    r"\\blibc::mkdir\\(|"\n',
     'axon-core', _RCT, _RCGT),
    ('M2423', 'COVERAGE GATE (eqgate4): a `drop:` field is a site', _RCG,
     '    r"\\bdrop:\\s*(?:Some\\(|None\\b)|"\n', '    r"\\bdrop_never:|"\n',
     'axon-core', _RCT, _RCGT),
    ('M2424', 'COVERAGE GATE (eqgate4): a uid/gid constant is a site', _RCG,
     '    r"\\b[A-Z][A-Z0-9_]*(?:UID|GID)\\b|"\n', '    r"\\bNEVER_UID_NAMED\\b|"\n',
     'axon-core', _RCT, _RCGT),
    ('M2425', 'COVERAGE GATE (eqgate4): an expected_*sha256 field set in a literal is a site', _RCG,
     '    r"^(?!\\s*pub\\b)\\s*expected_\\w*(?:sha256|digest|hash)\\w*\\s*:', '    r"^(?!\\s*pub\\b)\\s*never_expected_\\w*(?:sha256|digest|hash)\\w*\\s*:',
     'axon-core', _RCT, _RCGT),
    ('M2426', 'COVERAGE GATE (eqgate4): env::set_var is a site', _RCG,
     '    r"\\benv::set_var\\(|"\n', '    r"\\benv::set_never\\(|"\n',
     'axon-core', _RCT, _RCGT),
    ('M2427', 'COVERAGE GATE (eqgate4): env::remove_var is a site', _RCG,
     '    r"\\benv::remove_var\\(|"\n', '    r"\\benv::remove_never\\(|"\n',
     'axon-core', _RCT, _RCGT),
    ('M2428', 'COVERAGE GATE (eqgate4): a single-line .ok_or(..)? is a site', _RCG,
     'OKOR_FORM = re.compile(r"\\.ok_or(?:_else)?\\(.*\\)\\s*\\?")', 'OKOR_FORM = re.compile(r"\\.ok_or_never\\(")',
     'axon-core', _RCT, 'a_single_line_ok_or_refusal_is_a_site'),
    ('M2429', 'COVERAGE GATE (eqgate4): a compound guard is judged per term', _RCG,
     '    terms = condition_terms(clean, offs[g])\n    if not terms:\n        return []\n', '    return []\n',
     'axon-core', _RCT, 'a_compound_guard_is_judged_per_term'),
    ('M2430', 'COVERAGE GATE (eqgate4): a constant a refusing function reads is a site', _RCG,
     '        if m and (f, m.group(1)) in names:\n', '        if False and m and (f, m.group(1)) in names:\n',
     'axon-core', _RCT, 'a_constant_a_guard_reads_is_a_site'),
    ('M2431', 'COVERAGE GATE (eqgate4): the exemption audit reads a row id of any number of digits', _RCG,
     '(r"\\bM\\d+\\b", reason)', '(r"\\bM\\d{1,4}\\b", reason)',
     'axon-core', _RCT, 'an_exemption_citing_a_five_digit_row_that_does_not_exist_is_refused'),
    ('M2432', 'COVERAGE GATE (eqgate4): a raw libc::syscall is a site', _RCG,
     '    r"\\blibc::syscall\\(|"\n', '    r"\\blibc::syscall_never\\(|"\n',
     'axon-core', _RCT, 'a_raw_syscall_and_an_oom_score_write_are_sites'),
    ('M2433', 'COVERAGE GATE (eqgate4): a write of oom_score_adj is a site', _RCG,
     '    r"\\boom_score_adj\\b|"\n', '    r"\\boom_never\\b|"\n',
     'axon-core', _RCT, 'a_raw_syscall_and_an_oom_score_write_are_sites'),
    ('M2434', 'COVERAGE GATE (eqgate4): `false && a || b` is not a constant condition', _RCG,
     '        return True if any(x is True for x in v) else (False if all(x is False for x in v) else None)',
     '        return True if any(x is True for x in v) else (False if any(x is False for x in v) else None)',
     'axon-core', _RCT, 'a_compound_guard_is_judged_per_term'),
    ('M2435', 'COVERAGE GATE (eqgate4): a form is credited only by a row that changes its own line', _RCG,
     '        lo = i if kind == "form" else g\n', '        lo = g\n',
     'axon-core', _RCT, 'a_form_is_credited_only_by_a_row_that_changes_its_own_line'),
    ('M2436', 'COVERAGE GATE (eqgate4): a term exemption that matches no uncredited term is refused', _RCG,
     '        if t[3] == 0:\n', '        if False and t[3] == 0:\n',
     'axon-core', _RCT, 'a_compound_guard_is_judged_per_term'),
]

_PSVD = '--test psv_dispatch'
_PSVF2 = 'crates/axon-fabric/src/psv.rs'
_FORG = 'every_forgery_of_the_returned_evidence_is_unknown_for_its_own_reason'
_INTK = 'crates/axon-loop/src/intake.rs'
_INTT = 'verification_evidence_of_any_kind_is_refused_when_the_episode_cites_no_verifier'
_RDY = 'crates/axon-fabric/src/readiness.rs'
_RDYT = 'a_trust_preflight_of_another_schema_or_with_a_failing_verdict_is_not_certified'
_ADM = 'crates/axon-loop/src/admission.rs'
_REC = 'crates/axon-workspace-recipe/src/lib.rs'
MUTATIONS += [
    ('M2437', "TERM (eqgate4): a launch request's timeout is at least one second", _PL,
     '    if r.timeout_s == 0 || r.timeout_s > c.max_timeout_s {', '    if false || r.timeout_s > c.max_timeout_s {',
     'axon-fabric', '--lib', 'privileged_launcher::tests::a_request_with_a_bad_id_digest_or_timeout_is_refused'),
    ('M2438', "TERM (eqgate4): the out root belongs to the Fabric uid", _PL,
     '    if !is_dir(&st) || st.st_uid != c.fabric_uid || st.st_mode & 0o077 != 0 {',
     '    if !is_dir(&st) || false || st.st_mode & 0o077 != 0 {',
     'axon-fabric', '--lib', 'privileged_launcher::tests::an_out_root_another_uid_owns_is_refused'),
    ('M2439', "TERM (eqgate4): the guest verdict names this launch's candidate tree", _PSVF2,
     '        || v.inputs.candidate_tree_digest != m.candidate.tree_digest\n', '        || false\n',
     'axon-fabric', _PSVD, _FORG),
    ('M2440', "TERM (eqgate4): the guest verdict names this launch's suite tree", _PSVF2,
     '        || v.inputs.suite_tree_digest != m.suite.tree_digest\n', '        || false\n',
     'axon-fabric', _PSVD, _FORG),
    ('M2441', "TERM (eqgate4): the guest verdict names this launch's test", _PSVF2,
     '        || v.test != m.suite.test\n', '        || false\n',
     'axon-fabric', _PSVD, _FORG),
    ('M2442', "TERM (eqgate4): the trust preflight is of the certified schema", _RDY,
     '    if pf["schema"] != TRUST_PREFLIGHT_SCHEMA\n', '    if false\n',
     'axon-fabric', '--test readiness', _RDYT),
    ('M2443', "TERM (eqgate4): the trust preflight's verdict is PASS", _RDY,
     '        || pf["verdict"] != "PASS"\n', '        || false\n',
     'axon-fabric', '--test readiness', _RDYT),
    ('M2444', "TERM (eqgate4): an empty id is refused with an error (not a panic)", 'crates/axon-loop-contracts/src/ids.rs',
     '    if b.is_empty() || b.len() > 128 {', '    if b.len() > 128 {',
     'axon-loop-contracts', '--lib', 'ids::tests::an_empty_id_is_an_error_not_a_panic'),
    ('M2445', "TERM (eqgate4): a verifier that is also a subject issuer no longer counts at admission", _ADM,
     '                || eval.subject_issuers.contains(&v.issuer_ref)\n', '                || false\n',
     'axon-loop', '--test evl_admission', 'a_verifier_that_is_also_a_subject_issuer_no_longer_counts'),
    ('M2446', "TERM (eqgate4): a verdict counts only under the verifier key the operator still holds", _ADM,
     '                || key_now.as_deref() != Some(v.key_id.as_str())\n', '                || false\n',
     'axon-loop', '--test evl_admission', 'a_revoked_verifier_s_verdicts_stop_counting'),
    ('M2447', "TERM (eqgate4): a verification receipt the episode does not cite is refused", _INTK,
     '            || input.verification_receipt.is_some()\n', '            || false\n',
     'axon-loop', '--test intake', _INTT),
    ('M2448', "TERM (eqgate4): a verification attestation the episode does not cite is refused", _INTK,
     '            || input.verification_attestation.is_some()\n', '            || false\n',
     'axon-loop', '--test intake', _INTT),
    ('M2449', "TERM (eqgate4): a PSV evidence document the episode does not cite is refused", _INTK,
     '            || input.verification_psv_evidence.is_some()\n', '            || false\n',
     'axon-loop', '--test intake', _INTT),
    ('M2450', "TERM (eqgate4): the runner runs only a suite entry that is a file in the suite tree", 'crates/axon-psv/src/runner.rs',
     '        || !std::fs::symlink_metadata(cfg.suite.join(&m.suite.entry)).is_ok_and(|md| md.is_file())\n', '        || false\n',
     'axon-psv', '--test runner', 'a_suite_entry_that_is_not_a_file_in_the_suite_never_runs'),
    ('M2451', "TERM (eqgate4): a recipe symlink with an empty target is refused", _REC,
     "    if t.is_empty() || t.starts_with('/') {", "    if t.starts_with('/') {",
     'axon-workspace-recipe', '--lib', 'tests::a_link_with_an_empty_or_absolute_or_escaping_target_is_refused'),
    ('M2452', "CONST (eqgate4): the default import quota's bytes are decision D11's", _REC,
     'pub const MAX_BYTES: u64 = 268_435_456;', 'pub const MAX_BYTES: u64 = 1 << 40;',
     'axon-workspace-recipe', '--lib', 'tests::the_default_import_quota_is_decision_d11'),
    ('M2453', "CONST (eqgate4): the default import quota's depth is decision D11's", _REC,
     'pub const MAX_DEPTH: usize = 32;', 'pub const MAX_DEPTH: usize = 1 << 20;',
     'axon-workspace-recipe', '--lib', 'tests::the_default_import_quota_is_decision_d11'),
    ('M2454', "CONST (eqgate4): the recipe skips every top-level runtime-state directory", _REC,
     'pub const SKIPPED_TOP_LEVEL: [&str; 2] = [".git", ".micode"];', 'pub const SKIPPED_TOP_LEVEL: [&str; 2] = [".git", ".git"];',
     'axon-workspace-recipe', '--lib', 'tests::each_top_level_runtime_state_directory_is_skipped_and_a_nested_one_is_not'),
    ('M2455', "CONST (eqgate4): the contract's byte limit is 1 MiB", 'crates/axon-loop-contracts/src/lib.rs',
     'pub const MAX_BYTES: usize = 1_048_576;', 'pub const MAX_BYTES: usize = 1 << 30;',
     'axon-loop-contracts', '--test fixtures', 'the_contract_limits_are_the_documented_ones'),
    ('M2456', "CONST (eqgate4): the contract's nesting limit is 32", 'crates/axon-loop-contracts/src/lib.rs',
     'pub const MAX_DEPTH: usize = 32;', 'pub const MAX_DEPTH: usize = 1 << 20;',
     'axon-loop-contracts', '--test fixtures', 'the_contract_limits_are_the_documented_ones'),
    ('M2457', "CONST (eqgate4): the contract's integer limit is 2^53 - 1", 'crates/axon-loop-contracts/src/lib.rs',
     'pub const MAX_INTEGER: u64 = 9_007_199_254_740_991;', 'pub const MAX_INTEGER: u64 = 1 << 60;',
     'axon-loop-contracts', '--test fixtures', 'the_contract_limits_are_the_documented_ones'),
]

MUTATIONS += [
    ('M2458', "FORM (eqgate4): the operator-file reader is bounded by MAX_REQUEST", _PL,
     '        .take(MAX_REQUEST)\n        .read_to_end(&mut bytes)', '        .read_to_end(&mut bytes)',
     'axon-fabric', '--lib', 'privileged_launcher::tests::an_operator_file_is_read_at_most_to_the_request_bound'),
    ('M2460', "FORM (eqgate4): a staged input directory is 0755", _PL,
     '        set_mode(&dst, 0o755)?;', '        set_mode(&dst, 0o777)?;',
     'axon-fabric', '--lib', 'privileged_launcher::tests::a_snapshot_of_the_inputs_never_enters_an_existing_staging_leaf'),
    ('M2461', "ENV (eqgate4): a key-holding axon test runs under the ceiling with Exec removed (the env var is set)", 'crates/axon-core/src/main.rs',
     '        std::env::set_var("AXON_ALLOWED_EFFECTS", ceiling.join(","));\n        Some(key)', '        let _ = &ceiling;\n        Some(key)',
     'axon-core', '--no-default-features --test psv_test_selection', 'holding_a_completion_key_spawns_nothing'),
    ('M2462', "CONST (eqgate4): a custodian request line is bounded by MAX_MESSAGE (4 KiB)", _CU,
     'const MAX_MESSAGE: u64 = 4096;', 'const MAX_MESSAGE: u64 = 1 << 30;',
     'axon-fabric', '--lib', 'custodian::tests::a_custodian_request_line_is_cut_off_at_its_size_bound'),
    ('M2463', "CONST (eqgate4): the root helper's policy bound is 64 KiB", _PL,
     'const MAX_POLICY: usize = 64 << 10;', 'const MAX_POLICY: usize = 1 << 30;',
     'axon-fabric', '--test privileged_launcher', 'a_policy_over_the_helpers_bound_launches_nothing'),
    ('M2464', "CONST (eqgate4): the default evidence age is 30 days", 'crates/axon-fabric/src/backend.rs',
     'pub const DEFAULT_EVIDENCE_MAX_AGE_S: u64 = 30 * 24 * 3600;', 'pub const DEFAULT_EVIDENCE_MAX_AGE_S: u64 = 1 << 40;',
     'axon-fabric', '--test readiness_attribution', 'a_b263_qualification_that_lapsed_after_certification_is_refused'),
]

MUTATIONS += [
    ('M2465', "CONST (eqgate4): the guest's cmdline policy is refused as possibly truncated at the safe limit", _GI,
     'const CMDLINE_MAX_SAFE: usize = X86_COMMAND_LINE_SIZE - 2;', 'const CMDLINE_MAX_SAFE: usize = 1 << 30;',
     'axon-guest-init', '--test policy_refusals', 'an_untrustworthy_cmdline_policy_starts_no_workload'),
    ('M2466', "CONST (eqgate4): the guest policy word has the launcher's reserved cmdline budget", 'crates/axon-fabric/src/backend.rs',
     'pub const GUEST_POLICY_WORD_MAX: usize = GUEST_CMDLINE_MAX_SAFE - LAUNCHER_CMDLINE_RESERVE;', 'pub const GUEST_POLICY_WORD_MAX: usize = 1 << 30;',
     'axon-fabric', '--test guest_policy', 'the_size_rule_is_measured_on_the_encoded_cmdline_word'),
]


# ── C9 round 9, BUILDENV5 (amendment 97; M2500-M2519) ──────────────────────────
# A build-uid process the /proc State filter cannot see, the dedicated build uid, the per-uid lock, and
# the namespace helper's remaining gaps. Rows that are shell guards with no cargo-reachable attack
# (the kit's own --build-uid refusals, ns_run's namespace FLAGS, which the proof also asserts) are not
# here: amendment 97 gives their hand-removal evidence and says so.
_NS = 'scripts/lib/opkit_ns.sh'
MUTATIONS += [
    ('M2500', 'BUILD-ENV (97): the reaper judges each THREAD, not the process (a leader that has exited reads State Z)', _GE,
     '            tids = os.listdir(f"/proc/{ent}/task")\n',
     '            tids = [ent]\n',
     'axon-fabric', _GBT, 'the_reaper_lists_a_process_whose_main_thread_has_exited'),
    ('M2501', 'BUILD-ENV (97): a build step runs in its OWN PID namespace', _GE,
     '    return [UNSHARE, "--pid", "--fork", "--mount-proc", "--kill-child", "--",\n            "/usr/bin/setpriv"',
     '    return ["/usr/bin/setpriv"',
     'axon-fabric', _GBT, 'a_build_step_runs_in_its_own_pid_namespace'),
    ('M2502', 'BUILD-ENV (97): the build uid is refused when it is a service uid', _GE,
     '    why = build_uid_problem(int(raw))\n    if why:',
     '    why = ""\n    if why:',
     'axon-fabric', _GBT, 'the_build_uid_may_not_be_a_service_uid'),
    ('M2503', 'BUILD-ENV (97): the User= of an installed axon-*.service is a service uid', _GE,
     '                if n.startswith("axon-") or re.search(r"^\\s*Exec\\w*\\s*=.*axon", text, re.M):\n',
     '                if re.search(r"^\\s*Exec\\w*\\s*=.*axon", text, re.M):\n',
     'axon-fabric', _GBT, 'the_build_uid_may_not_be_a_service_uid'),
    ('M2504', 'BUILD-ENV (97): the uid fields of the deployed configs are service uids', _GE,
     '                        _id_fields(json.loads(_read_small(path)), path, uids, gids)\n',
     '                        json.loads(_read_small(path))\n',
     'axon-fabric', _GBT, 'the_build_uid_may_not_be_a_service_uid'),
    ('M2505', 'BUILD-ENV (97): the named service accounts are service uids', _GE,
     '    for name in (SERVICE_USERS if users is None else users):',
     '    for name in (() if users is None else users):',
     'axon-fabric', _GBT, 'the_build_uid_may_not_be_a_service_uid'),
    ('M2506', "BUILD-ENV (97): the kit's check-build-uid verb judges the build uid", _GE,
     '        why = build_uid_problem(int(a[1]), int(a[2]), users)\n',
     '        why = ""\n',
     'axon-fabric', _GBT, 'the_build_uid_may_not_be_a_service_uid'),
    ('M2507', 'BUILD-ENV (97): the lock directory must be root-owned and not writable by others', _GE,
     '        if st.st_uid != 0 or st.st_mode & 0o022:\n            fail(f"{LOCK_DIR}',
     '        if False:\n            fail(f"{LOCK_DIR}',
     'axon-fabric', _GBT, 'the_build_uid_lock_is_root_owned_and_begin_holds_it'),
    ('M2508', 'BUILD-ENV (97): a pre-existing lock file with another owner or a loose mode is refused', _GE,
     '    if not stat.S_ISREG(ls.st_mode) or ls.st_uid != 0 or ls.st_mode & 0o077:\n',
     '    if False:\n',
     'axon-fabric', _GBT, 'the_build_uid_lock_is_root_owned_and_begin_holds_it'),
    ('M2509', 'BUILD-ENV (97): begin holds the per-uid lock while it builds the trees', _GE,
     '    lockfd = build_uid_lock()\n    try:\n        _begin(record_path, host)\n    finally:\n        os.close(lockfd)\n',
     '    _begin(record_path, host)\n',
     'axon-fabric', _GBT, 'the_build_uid_lock_is_root_owned_and_begin_holds_it'),
    ('M2510', 'BUILD-ENV (97): begin returns the trees to root before it returns', _GE,
     '    lock_from_build(rec)\n    rec["measured"] = measure(rec)\n',
     '    rec["measured"] = measure(rec)\n',
     'axon-fabric', _GBT, 'begin_leaves_the_three_trees_root_owned'),
    ('M2511', "KIT-TEST (97): a host directory that cannot be examined through the host's view is a refusal, not a vacuous pass", _NS,
     '      || { echo "REFUSE(opkit_ns): the host\'s $d cannot be examined',
     '      || true # { echo "REFUSE(opkit_ns): the host\'s $d cannot be examined',
     'axon-fabric', _OE, 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2512', 'KIT-TEST (97): the *_FOR_TEST overrides are honoured only for scripts/test_opkit_ns.sh', _NS,
     '    opkit_selftest_caller || { echo',
     '    true || { echo',
     'axon-fabric', _OE, 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2513', 'KIT-TEST (97): the drift check requires the kit invocation ITSELF to be the ns_run command (ns_run true; kit is refused)', 'scripts/opkit_ns_drift.py',
     '            kind, wrapped = command_class(w, ctx)\n            if not kind and not wrapped and not ns_body:',
     '            kind, wrapped = command_class(w, ctx)\n            wrapped = wrapped or "ns_run" in line\n            if not kind and not wrapped and not ns_body:',
     'axon-fabric', _OE, 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2514', 'KIT-TEST (97): every write target of the kit lies under a destination ns_run shadows', 'scripts/opkit_ns_drift.py',
     '        if not any(t == d or t.startswith(d + "/") for d in dests):',
     '        if False and not any(t == d or t.startswith(d + "/") for d in dests):',
     'axon-fabric', _OE, 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2515', 'KIT-TEST (97): a controlled-build verb outside ns_run is refused by the drift check', 'scripts/opkit_ns_drift.py',
     '    if kind is None and BUILD_VERB.search(flat):',
     '    if False and BUILD_VERB.search(flat):',
     'axon-fabric', _OE, 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
]

# ── C9 round 9, EQGATE5 (amendment 98; M2520-M2569) ──────────────────────────
MUTATIONS += [
    ('M2520', "VALUE (eqgate5): guest_config hands the runner the documented manifest path", _PRN,
     '        manifest: PathBuf::from("/in/job/launch-manifest.json"),', '        manifest: PathBuf::from("/in/candidate/launch-manifest.json"),',
     'axon-psv', '--bin axon-psv-runner', 'tests::every_path_the_guest_hands_the_runner_is_the_documented_one'),
    ('M2521', "VALUE (eqgate5): guest_config hands the runner the documented secret path", _PRN,
     '        secret: PathBuf::from("/in/job/completion-secret"),', '        secret: PathBuf::from("/in/candidate/completion-secret"),',
     'axon-psv', '--bin axon-psv-runner', 'tests::every_path_the_guest_hands_the_runner_is_the_documented_one'),
    ('M2522', "VALUE (eqgate5): guest_config hands the runner the documented candidate path", _PRN,
     '        candidate: PathBuf::from("/in/candidate"),', '        candidate: PathBuf::from("/in/suite"),',
     'axon-psv', '--bin axon-psv-runner', 'tests::every_path_the_guest_hands_the_runner_is_the_documented_one'),
    ('M2523', "VALUE (eqgate5): guest_config hands the runner the documented suite path", _PRN,
     '        suite: PathBuf::from("/in/suite"),', '        suite: PathBuf::from("/in/candidate"),',
     'axon-psv', '--bin axon-psv-runner', 'tests::every_path_the_guest_hands_the_runner_is_the_documented_one'),
    ('M2524', "VALUE (eqgate5): guest_config hands the runner the documented out path", _PRN,
     '        out: PathBuf::from("/out"),', '        out: PathBuf::from("/in/candidate"),',
     'axon-psv', '--bin axon-psv-runner', 'tests::every_path_the_guest_hands_the_runner_is_the_documented_one'),
    ('M2525', "VALUE (eqgate5): guest_config hands the runner the documented interpreter path", _PRN,
     '        axon: PathBuf::from("/usr/bin/axon"),', '        axon: PathBuf::from("/in/candidate/axon"),',
     'axon-psv', '--bin axon-psv-runner', 'tests::every_path_the_guest_hands_the_runner_is_the_documented_one'),
    ('M2526', "VALUE (eqgate5): guest_config hands the runner the documented runner_exe path", _PRN,
     '        runner_exe: PathBuf::from("/proc/self/exe"),', '        runner_exe: PathBuf::from("/usr/bin/axon"),',
     'axon-psv', '--bin axon-psv-runner', 'tests::every_path_the_guest_hands_the_runner_is_the_documented_one'),
    ('M2527', "VALUE (eqgate5): guest_config hands the runner the documented policy", _PRN,
     '        policy: policy_from_cmdline(cmdline),', '        policy: None,',
     'axon-psv', '--bin axon-psv-runner', 'tests::every_path_the_guest_hands_the_runner_is_the_documented_one'),
]

MUTATIONS += [
    ('M2528', "VALUE (eqgate5): the operator's provenance allowlist is walked from `/`", 'crates/axon-fabric/src/git_data.rs',
     '            path: PathBuf::from(ALLOWLIST_PATH),\n            base: PathBuf::from("/"),', '            path: PathBuf::from(ALLOWLIST_PATH),\n            base: PathBuf::from("/tmp"),',
     'axon-fabric', '--lib', 'git_data::tests::the_operators_allowlist_is_walked_from_the_root'),
    ('M2529', "VALUE (eqgate5): the operator's provenance allowlist is read from its documented path", 'crates/axon-fabric/src/git_data.rs',
     '            path: PathBuf::from(ALLOWLIST_PATH),\n            base: PathBuf::from("/"),', '            path: PathBuf::from("/tmp/provenance-allowlist"),\n            base: PathBuf::from("/"),',
     'axon-fabric', '--lib', 'git_data::tests::the_operators_allowlist_is_walked_from_the_root'),
    ('M2530', 'VALUE (eqgate5): the production authority walks ownership from `/`', 'crates/axon-fabric/src/privileged_launcher.rs',
     '            walk_base: PathBuf::from("/"),\n            test: false,', '            walk_base: PathBuf::from("/tmp"),\n            test: false,',
     'axon-fabric', '--lib', 'privileged_launcher::tests::the_production_authority_is_roots_and_walks_from_the_root'),
    ('M2531', "VALUE (eqgate5): the production authority is root's", 'crates/axon-fabric/src/privileged_launcher.rs',
     '            operator_uid: 0,\n            walk_base: PathBuf::from("/"),', '            operator_uid: 1,\n            walk_base: PathBuf::from("/"),',
     'axon-fabric', '--lib', 'privileged_launcher::tests::the_production_authority_is_roots_and_walks_from_the_root'),
    ('M2532', 'VALUE (eqgate5): the production authority is not a test configuration', 'crates/axon-fabric/src/privileged_launcher.rs',
     '            walk_base: PathBuf::from("/"),\n            test: false,', '            walk_base: PathBuf::from("/"),\n            test: true,',
     'axon-fabric', '--lib', 'privileged_launcher::tests::the_production_authority_is_roots_and_walks_from_the_root'),
    ('M2533', "VALUE (eqgate5): the operator's readiness trust walks ownership from `/`", 'crates/axon-fabric/src/readiness.rs',
     '            ownership_base: PathBuf::from("/"),\n            require_unwritable: true,', '            ownership_base: PathBuf::from("/tmp"),\n            require_unwritable: true,',
     'axon-fabric', '--lib', 'readiness::tests::the_operators_readiness_trust_walks_from_the_root_and_reads_the_operators_allowlist'),
    ('M2534', "VALUE (eqgate5): the operator's readiness trust reads the operator's allowlist", 'crates/axon-fabric/src/readiness.rs',
     '            allowlist: AllowlistSource::operator(),\n            clock: Clock::System,', '            allowlist: AllowlistSource::test(Path::new("/tmp"), Path::new("/tmp/allowlist")),\n            clock: Clock::System,',
     'axon-fabric', '--lib', 'readiness::tests::the_operators_readiness_trust_walks_from_the_root_and_reads_the_operators_allowlist'),
]

MUTATIONS += [
    ('M2535', 'FAIL-OPEN (eqgate5): a plan with no independent_units is refused, not read as no minimum sample', 'crates/axon-loop/src/rules.rs',
     'p.independent_units.ok_or("independent_units unset")?,', 'p.independent_units.unwrap_or(0),',
     'axon-loop', '--test rules_sites', 'a_plan_without_independent_units_yields_no_rules'),
    ('M2536', 'VALUE (eqgate5): a plan with no candidate_budget is refused, not read as a budget of 0', 'crates/axon-loop/src/rules.rs',
     'p.candidate_budget.ok_or("candidate_budget unset")?,', 'p.candidate_budget.unwrap_or(0),',
     'axon-loop', '--test rules_sites', 'a_plan_without_a_candidate_budget_yields_no_rules'),
    ('M2537', 'VALUE (eqgate5): an unset keyed rule is refused by name, not parsed as the empty string', 'crates/axon-loop/src/rules.rs',
     'v.as_deref().ok_or(format!("{field} unset"))?;', 'v.as_deref().unwrap_or("");',
     'axon-loop', '--test rules_sites', 'a_plan_with_an_unset_keyed_rule_names_the_field'),
    ('M2538', 'FAIL-OPEN (eqgate5): a digest with no scheme is not a Ref (the missing colon is not defaulted)', 'crates/axon-loop-contracts/src/ids.rs',
     's.split_once(\':\').ok_or("expected <scheme>:<hex>")?;', 's.split_once(\':\').unwrap_or(("cl22", s));',
     'axon-loop-contracts', '--lib', 'ids::tests::ref_parts'),
    ('M2539', 'FAIL-OPEN (eqgate5): a Ref of a scheme other than cl22, acf1 or sha256 is refused', 'crates/axon-loop-contracts/src/ids.rs',
     'RefScheme::from_prefix(scheme).ok_or("scheme must be cl22, acf1 or sha256")?;', 'let _ = RefScheme::from_prefix(scheme);',
     'axon-loop-contracts', '--lib', 'ids::tests::ref_parts'),
]

MUTATIONS += [
    ('M2540', 'BUILD ENV (eqgate5): the build uid is a plain decimal of at most nine digits', 'scripts/guest_build_env.py',
     '    if not re.fullmatch(r"[0-9]{1,9}", raw) or int(raw) == 0 or int(raw) == os.geteuid():', '    if False or int(raw) == 0 or int(raw) == os.geteuid():',
     'axon-fabric', '--test guest_build_env_guards', 'the_build_uid_is_unprivileged_and_not_the_builders_own'),
    ('M2541', 'BUILD ENV (eqgate5): the build uid is not root', 'scripts/guest_build_env.py',
     '    if not re.fullmatch(r"[0-9]{1,9}", raw) or int(raw) == 0 or int(raw) == os.geteuid():', '    if not re.fullmatch(r"[0-9]{1,9}", raw) or False or int(raw) == os.geteuid():',
     'axon-fabric', '--test guest_build_env_guards', 'the_build_uid_is_unprivileged_and_not_the_builders_own'),
    ('M2542', "BUILD ENV (eqgate5): the build uid is not the builder's own", 'scripts/guest_build_env.py',
     '    if not re.fullmatch(r"[0-9]{1,9}", raw) or int(raw) == 0 or int(raw) == os.geteuid():', '    if not re.fullmatch(r"[0-9]{1,9}", raw) or int(raw) == 0 or False:',
     'axon-fabric', '--test guest_build_env_guards', 'the_build_uid_is_unprivileged_and_not_the_builders_own'),
    ('M2543', 'BUILD ENV (eqgate5): the controlled build is refused to anyone but root', 'scripts/guest_build_env.py',
     '    if os.geteuid() != 0:\n        fail("the controlled build runs as root', '    if False and os.geteuid() != 0:\n        fail("the controlled build runs as root',
     'axon-fabric', '--test guest_build_env_guards', 'the_build_uid_is_unprivileged_and_not_the_builders_own'),
    ('M2544', 'BUILD ENV (eqgate5): an ancestor of the build parent that a group or other can write is refused', 'scripts/guest_build_env.py',
     '        if st.st_uid not in (0, me) or st.st_mode & 0o022:', '        if st.st_uid not in (0, me) or False:',
     'axon-fabric', '--test guest_build_env_guards', 'the_build_parent_and_the_operator_files_are_judged_by_who_can_write_them'),
    ('M2545', 'BUILD ENV (eqgate5): an ancestor of the build parent owned by another uid is refused', 'scripts/guest_build_env.py',
     '        if st.st_uid not in (0, me) or st.st_mode & 0o022:', '        if False or st.st_mode & 0o022:',
     'axon-fabric', '--test guest_build_env_guards', 'the_build_parent_and_the_operator_files_are_judged_by_who_can_write_them'),
    ('M2546', 'BUILD ENV (eqgate5): a RECORDED ancestor mode that a group or other can write is refused', 'scripts/guest_build_env.py',
     '                or int(mode, 8) & 0o022):', '                or False):',
     'axon-fabric', '--test guest_build_env_guards', 'the_build_parent_and_the_operator_files_are_judged_by_who_can_write_them'),
    ('M2547', 'BUILD ENV (eqgate5): a builder pin whose build uid is root is refused', 'scripts/guest_build_env.py',
     '            or not isuid(bu) or bu == 0 or bu == uid or not isinstance(parent, str)', '            or not isuid(bu) or False or bu == uid or not isinstance(parent, str)',
     'axon-fabric', '--test guest_build_env_guards', 'a_record_is_judged_by_its_pin_its_proof_its_shape_and_its_bytes'),
    ('M2548', "BUILD ENV (eqgate5): a builder pin whose build uid is the builder's own is refused", 'scripts/guest_build_env.py',
     '            or not isuid(bu) or bu == 0 or bu == uid or not isinstance(parent, str)', '            or not isuid(bu) or bu == 0 or False or not isinstance(parent, str)',
     'axon-fabric', '--test guest_build_env_guards', 'a_record_is_judged_by_its_pin_its_proof_its_shape_and_its_bytes'),
    ('M2549', 'BUILD ENV (eqgate5): a record is judged only under a build parent no other uid can write', 'scripts/guest_build_env.py',
     '    _anc, why = ancestors_of(parent, builder_uid) if judging else ([], "")', '    _anc, why = ([], "")',
     'axon-fabric', '--test guest_build_env_guards', 'a_record_is_judged_by_its_pin_its_proof_its_shape_and_its_bytes'),
    ('M2550', "BUILD ENV (eqgate5): a record whose build uid is the builder's own is not a controlled build's", 'scripts/guest_build_env.py',
     '    if (not isinstance(bu, int) or isinstance(bu, bool) or bu <= 0 or bu == rec.get("builder_uid")', '    if (not isinstance(bu, int) or isinstance(bu, bool) or bu <= 0 or False',
     'axon-fabric', '--test guest_build_env_guards', 'a_record_is_judged_by_its_pin_its_proof_its_shape_and_its_bytes'),
    ('M2551', "BUILD ENV (eqgate5): a record's build uid is held to the operator pin's build uid", 'scripts/guest_build_env.py',
     '    if len(builder) == 3 and rec.get("build_uid") != builder[2]:', '    if False and rec.get("build_uid") != builder[2]:',
     'axon-fabric', '--test guest_build_env_guards', 'a_record_is_judged_by_its_pin_its_proof_its_shape_and_its_bytes'),
    ('M2552', "BUILD ENV (eqgate5): cargo's PATH is the fixed system directories, never the caller's", 'scripts/guest_build_env.py',
     '"PATH": TOOL_PATH, "RUSTC": rustc}', '"PATH": os.environ.get("PATH", TOOL_PATH), "RUSTC": rustc}',
     'axon-fabric', '--test guest_build_env_guards', 'the_constructed_environments_and_the_host_tool_identities_are_the_documented_ones'),
    ('M2553', 'BUILD ENV (eqgate5): a host tool is taken from the fixed directories only', 'scripts/guest_build_env.py',
     '    for d in TOOL_PATH.split(":"):\n        p = os.path.join(d, name)', '    for d in (os.environ.get("PATH") or TOOL_PATH).split(":"):\n        p = os.path.join(d, name)',
     'axon-fabric', '--test guest_build_env_guards', 'the_constructed_environments_and_the_host_tool_identities_are_the_documented_ones'),
]

MUTATIONS += [
    ('M2554', 'COVERAGE GATE (eqgate5): an absolute path literal handed to a config field is a site', 'scripts/v022_refusal_coverage.py',
     '    r"\\benv::remove_var\\(|"\n    r"^\\s*\\w+\\s*:\\s*(?:std::path::)?(?:PathBuf::from|Path::new)\\(\\s*\\"/"\n)', '    r"\\benv::remove_var\\(|"\n    r"^\\s*\\w+\\s*:\\s*(?:std::path::)?(?:PathBuf::from|Path::new)_never\\(\\s*\\"/"\n)',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_absolute_path_literal_handed_to_a_config_field_is_a_site'),
    ('M2555', 'COVERAGE GATE (eqgate5): a Rust REMAINDER exemption is counted', 'scripts/v022_refusal_coverage.py',
     '                    REMAINDER_SITES.append((f, g + 1, remainder_category(e[2])))\n                    break\n                if e[2].startswith("OBSERVED-NOT-ROWED"):', '                    pass\n                    break\n                if e[2].startswith("OBSERVED-NOT-ROWED"):',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_remainder_exemption_is_counted_and_never_claimed_covered'),
    ('M2556', 'COVERAGE GATE (eqgate5): a Python fail(..) is a refusal site', 'scripts/v022_refusal_coverage.py',
     'isinstance(f, ast.Name) and f.id in ("fail", "die", "refuse"):', 'isinstance(f, ast.Name) and f.id in ("die", "refuse"):',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_python_refusal_is_a_site_judged_by_its_own_guard'),
    ('M2557', 'COVERAGE GATE (eqgate5): a Python exemption whose fragment is not in its site is refused', 'scripts/v022_refusal_coverage.py',
     '            if hit[0][3] not in site_text:', '            if False and hit[0][3] not in site_text:',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_python_refusal_is_a_site_judged_by_its_own_guard'),
    ('M2558', 'COVERAGE GATE (eqgate5): a Python REMAINDER exemption is counted', 'scripts/v022_refusal_coverage.py',
     '                REMAINDER_SITES.append((f, g + 1, "py_guard"))', '                pass',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_python_guard_exemption_is_counted_by_kind'),
    ('M2559', 'COVERAGE GATE (eqgate5): a Python OBSERVED exemption is counted apart', 'scripts/v022_refusal_coverage.py',
     '                OBSERVED_SITES.append((f, g + 1, kind))', '                pass',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_python_guard_exemption_is_counted_by_kind'),
]

MUTATIONS += [
    ('M2560', 'FAIL-OPEN (eqgate5): an evidence record with no assertions list is refused, not read as an empty one', 'crates/axon-fabric/src/backend.rs',
     '        .as_array()\n        .ok_or("evidence record has no assertions list")?;', '        .as_array()\n        .map_or(&[][..], |a| a.as_slice());',
     'axon-fabric', '--test qualification', 'a_record_with_no_assertions_list_is_refused_as_that'),
    ('M2561', 'FAIL-OPEN (eqgate5): a manifest that pins no guest interpreter digest is refused, not read as the empty digest', 'crates/axon-fabric/src/backend.rs',
     '            .ok_or("manifest has no artifacts.axon.sha256")?', '            .unwrap_or("")',
     'axon-fabric', '--test qualification', 'a_manifest_naming_no_guest_interpreter_digest_is_refused_as_that'),
    ('M2562', 'FAIL-OPEN (eqgate5): a custodian answer that names no expiry is refused, not read as never expiring', 'crates/axon-fabric/src/custodian.rs',
     '            .ok_or("custodian\'s check names no expiry for the nonce")?;', '            .unwrap_or(i64::MAX);',
     'axon-fabric', '--test custodian', 'a_custodian_check_that_names_no_expiry_is_refused'),
    ('M2563', 'FAIL-OPEN (eqgate5): an observation whose time is not a UTC timestamp is refused, not read as taken now', 'crates/axon-fabric/src/observer.rs',
     '        .ok_or_else(|| format!("observed_at {:?} is not a UTC timestamp", o.observed_at))?;', '        .unwrap_or(cfg.clock.now_unix());',
     'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M2564', 'FAIL-OPEN (eqgate5): an observer max_age_s that is not a number is refused, not read as no limit', 'crates/axon-fabric/src/protected_host.rs',
     '.ok_or_else(|| bad("observer.max_age_s is not a number".into()))?,', '.unwrap_or(u64::MAX),',
     'axon-fabric', '--test protected_host', 'an_observer_section_value_of_the_wrong_kind_is_refused_as_that'),
    ('M2565', 'FAIL-OPEN (eqgate5): the flags a caller may never pass to a protected host are the documented nine', 'crates/axon-fabric/src/protected_host.rs',
     '    "--linux-launcher",\n', '    "--linux-launcher-x",\n',
     'axon-fabric', '--test protected_host', 'the_flags_a_caller_may_never_pass_are_the_documented_nine'),
    ('M2566', 'FAIL-OPEN (eqgate5): a tree entry whose object id is cut short is refused as malformed, not read with a zero id', 'crates/axon-fabric/src/git_data.rs',
     '                let id = b.get(nul + 1..nul + 21).ok_or_else(bad)?;', '                let id = b.get(nul + 1..nul + 21).unwrap_or(&[0u8; 20][..]);',
     'axon-fabric', '--lib', 'git_data::tests::a_tree_entry_that_is_not_what_git_writes_is_refused_as_malformed'),
    ('M2567', 'FAIL-OPEN (eqgate5): a tree entry whose mode is not octal is refused as malformed, not read as a regular file', 'crates/axon-fabric/src/git_data.rs',
     '                    .and_then(|m| u32::from_str_radix(m, 8).ok())\n                    .ok_or_else(bad)?;', '                    .and_then(|m| u32::from_str_radix(m, 8).ok())\n                    .unwrap_or(0o100644);',
     'axon-fabric', '--lib', 'git_data::tests::a_tree_entry_that_is_not_what_git_writes_is_refused_as_malformed'),
    ('M2568', 'CONST (eqgate5): an authority program over 256 MiB is refused before it is read', 'crates/axon-fabric/src/sealed_exec.rs',
     'const MAX_BYTES: u64 = 256 << 20;', 'const MAX_BYTES: u64 = 1 << 40;',
     'axon-fabric', '--lib', 'sealed_exec::tests::an_authority_program_over_the_size_bound_is_never_read'),
    ('M2569', 'CONST (eqgate5): each of the nine ledger-dependent directories refuses a deleted history on its own', 'crates/axon-loop/src/ledger.rs',
     'const DEPENDENT: &[&str] = &[\n    "plans",\n', 'const DEPENDENT: &[&str] = &[\n',
     'axon-loop', '--test ledger_sites', 'each_dependent_directory_refuses_a_deleted_history_on_its_own'),
]

# INTEG8 (amendment 99): rows for the sites buildenv5's rewrite of guest_build_env.py added or moved.
MUTATIONS += [
    ('M2570', 'BUILD ENV (integ8): build_uid_problem refuses root (the kit verb has no other judge)', 'scripts/guest_build_env.py',
     '    if uid == 0:\n        return "it is root"', '    if False:\n        return "it is root"',
     'axon-fabric', '--test guest_build_env_guards', 'the_tree_copy_the_clone_the_toolchain_pin_and_the_command_line_refuse'),
    ('M2571', "BUILD ENV (integ8): build_uid_problem refuses the builder's own uid", 'scripts/guest_build_env.py',
     '    if uid == (os.geteuid() if builder_uid is None else builder_uid):\n', '    if False:\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_tree_copy_the_clone_the_toolchain_pin_and_the_command_line_refuse'),
    ('M2572', 'BUILD ENV (integ8): build_uid_problem refuses a service uid', 'scripts/guest_build_env.py',
     '    if uid in svc:\n', '    if False and uid in svc:\n',
     'axon-fabric', '--test guest_build_env', 'the_build_uid_may_not_be_a_service_uid'),
    ('M2573', 'BUILD ENV (integ8): a build step is refused when the PID-namespace tool is missing', 'scripts/guest_build_env.py',
     '    if not os.access(UNSHARE, os.X_OK):\n', '    if False and not os.access(UNSHARE, os.X_OK):\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_controlled_build_runs_only_as_root_as_an_unprivileged_uid'),
    ('M2574', 'BUILD ENV (integ8): the kit verb check-build-uid takes plain decimal uids only', 'scripts/guest_build_env.py',
     '        if not (re.fullmatch(r"[0-9]{1,9}", a[1]) and re.fullmatch(r"[0-9]{1,9}", a[2])):\n',
     '        if False and not (re.fullmatch(r"[0-9]{1,9}", a[1]) and re.fullmatch(r"[0-9]{1,9}", a[2])):\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_tree_copy_the_clone_the_toolchain_pin_and_the_command_line_refuse'),
    ('M2575', "BUILD ENV (integ8): the kit verb check-build-uid fails on build_uid_problem's verdict", 'scripts/guest_build_env.py',
     '        why = build_uid_problem(int(a[1]), int(a[2]), users)\n        if why:\n', '        why = build_uid_problem(int(a[1]), int(a[2]), users)\n        if False:\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_tree_copy_the_clone_the_toolchain_pin_and_the_command_line_refuse'),
    ('M2576', "BUILD ENV (integ8): build_ids fails on build_uid_problem's verdict", 'scripts/guest_build_env.py',
     '    why = build_uid_problem(int(raw))\n    if why:\n        fail(f"AXON_GUEST_BUILD_UID', '    why = build_uid_problem(int(raw))\n    if False:\n        fail(f"AXON_GUEST_BUILD_UID',
     'axon-fabric', '--test guest_build_env', 'the_build_uid_may_not_be_a_service_uid'),
]


# BUILDENV6 (amendment 101; M2630-M2659): deny-by-default root, the closed host descriptor, the hardened drift gate,
# service-account discovery that fails closed.
MUTATIONS += [
    ('M2630', 'NS-ROOT (101): ns_run closes the host-root descriptor (and checks no directory descriptor survives) before the command starts', 'scripts/lib/opkit_ns.sh',
     '  if [ -n "${OPKIT_HOST_FD:-}" ]; then eval "exec $OPKIT_HOST_FD<&-"; unset OPKIT_HOST_FD; fi\n  opkit_ns_fd_leak\n',
     '  :\n',
     'axon-fabric', '--test operator_examples', 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2631', 'NS-ROOT (101): the descriptor check reports a directory descriptor left open', 'scripts/lib/opkit_ns.sh',
     '  [ "$n" = 0 ]\n}\n',
     '  true\n}\n',
     'axon-fabric', '--test operator_examples', 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2632', 'NS-ROOT (101): make_ro sets MOUNT_ATTR_RDONLY on the whole tree', 'scripts/lib/opkit_ns.sh',
     'a = A(1, 0, 0, 0)',
     'a = A(0, 0, 0, 0)',
     'axon-fabric', '--test operator_examples', 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2633', 'NS-ROOT (101): the proof refuses a namespace whose root is not read-only', 'scripts/lib/opkit_ns.sh',
     '    opkit_ro_proof "$dests" || return 1\n',
     '    true\n',
     'axon-fabric', '--test operator_examples', 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2634', "NS-ROOT (101): a re-assertion inside ns_run needs the proof's stamp", 'scripts/lib/opkit_ns.sh',
     'opkit_stamp_ok() {\n  local s own host\n',
     'opkit_stamp_ok() {\n  return 0\n  local s own host\n',
     'axon-fabric', '--test operator_examples', 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2635', "NS-ROOT (101): the stamp must record THIS namespace's ids", 'scripts/lib/opkit_ns.sh',
     '  [ "$own" = "$(opkit_ns_ids)" ] || return 1\n',
     '',
     'axon-fabric', '--test operator_examples', 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2636', "NS-ROOT (101): the stamp's host namespace must differ from this one", 'scripts/lib/opkit_ns.sh',
     '  [ "$(sed -n \'s/^mnt=\\([^,]*\\).*/\\1/p\' <<<"$own")" != "$(sed -n \'s/^mnt=\\([^,]*\\).*/\\1/p\' <<<"$host")" ]\n',
     '  true\n',
     'axon-fabric', '--test operator_examples', 'the_namespace_helper_refuses_when_its_proof_fails'),
    ('M2637', 'DRIFT (101): deny by mention: any unlisted wrapper, eval or interpreter that names the kit is refused', 'scripts/opkit_ns_drift.py',
     '    elif kit_re.search(flat) and cmd0 not in BENIGN and not (cmd0 in ("python3", "python") and rest[1:2] == ["-"]):\n',
     '    elif False and kit_re.search(flat) and cmd0 not in BENIGN and not (cmd0 in ("python3", "python") and rest[1:2] == ["-"]):\n',
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2638', "DRIFT (101): a variable assigned the kit's path is the kit", 'scripts/opkit_ns_drift.py',
     '                        self.kit.add(name)\n',
     '                        pass\n',
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2639', 'DRIFT (101): an assignment that builds a command string out of the kit / --apply is refused where it is made', 'scripts/opkit_ns_drift.py',
     "        if (re.search(r'\\s', v) and (kit_re.search(v)",
     "        if (False and re.search(r'\\s', v) and (kit_re.search(v)",
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2640', 'DRIFT (101): python calling the controlled-build API outside ns_run is refused', 'scripts/opkit_ns_drift.py',
     '    if kind is None and BUILD_API.search(flat) and cmd0 not in ("echo", "printf", "grep"):\n',
     '    if False and BUILD_API.search(flat) and cmd0 not in ("echo", "printf", "grep"):\n',
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2641', 'DRIFT (101): a variable holding a real destination path is a destination when a mutator names it', 'scripts/opkit_ns_drift.py',
     '                    self.real.add(name)\n',
     '                    pass\n',
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2642', 'DRIFT (101): a redirection into a real destination is a write (the outer shell opens it)', 'scripts/opkit_ns_drift.py',
     '        if ctx.real_arg(t):\n            out.append(t)\n',
     '        if False and ctx.real_arg(t):\n            out.append(t)\n',
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2643', 'DRIFT (101): python -c naming a real destination as a write target is refused outside ns_run', 'scripts/opkit_ns_drift.py',
     'strip_helper(w)[0] in ("python3", "python", "bash", "sh") and DEST_LITERAL.search(cmd):\n',
     'strip_helper(w)[0] in ("python3", "python", "bash", "sh") and False and DEST_LITERAL.search(cmd):\n',
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2644', 'DRIFT (101): --apply under ANY unlisted wrapper is refused', 'scripts/opkit_ns_drift.py',
     '    if "--apply" in rest and cmd0 not in ("echo", "printf", "grep", "[", "[[", "test"):\n',
     '    if "--apply" in rest and (cmd0 in RUNNERS or KIT_TOKEN.search(flat) or "ARGS" in flat or cmd0 == "setpriv"):\n',
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2645', 'DRIFT (101): the extractor sees a python open() for writing as a kit write target', 'scripts/opkit_ns_drift.py',
     '            if m.group(1) == "open" and not (m.group(4) and re.search(r\'[wax+]\', m.group(4))):\n                continue\n',
     '            continue\n',
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2646', 'DRIFT (101): the extractor sees sed -i as a kit write target', 'scripts/opkit_ns_drift.py',
     '            elif c0 == "sed" and any(a.startswith("-i") or a == "--in-place" for a in w[1:]) and len(args) > 1:\n',
     '            elif False and c0 == "sed" and any(a.startswith("-i") or a == "--in-place" for a in w[1:]) and len(args) > 1:\n',
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2647', 'DRIFT (101): the extractor sees tar -x -C as a kit write target', 'scripts/opkit_ns_drift.py',
     '            elif c0 == "tar" and any(',
     '            elif False and c0 == "tar" and any(',
     'axon-fabric', '--test operator_examples', 'no_test_script_runs_the_operator_kit_outside_the_namespace_helper'),
    ('M2648', 'BUILD-ENV (101): a uid given as a decimal string is a uid', 'scripts/guest_build_env.py',
     '    if isinstance(v, str) and re.fullmatch(r"[0-9]{1,9}", v.strip()):\n',
     '    if False and isinstance(v, str) and re.fullmatch(r"[0-9]{1,9}", v.strip()):\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2649', 'BUILD-ENV (101): a plural uid list is read element by element', 'scripts/guest_build_env.py',
     '            for x in (v if isinstance(v, list) else [v]):\n',
     '            for x in [v]:\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2650', 'BUILD-ENV (101): /etc/axon is read recursively', 'scripts/guest_build_env.py',
     '        walked = list(os.walk(etc, followlinks=False, onerror=errs.append))\n',
     '        walked = list(os.walk(etc, followlinks=False, onerror=errs.append))[:1]\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2651', 'BUILD-ENV (101): a config whose name does not end in .json is read when it parses', 'scripts/guest_build_env.py',
     '                    _id_fields(doc, path, uids, gids)\n',
     '                    pass\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2652', 'BUILD-ENV (101): an unparsable .json under /etc/axon refuses (it is not skipped)', 'scripts/guest_build_env.py',
     '                        raise DiscoveryRefused(f"{path} is not valid JSON: it may name a service account")\n',
     '                        continue\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2653', 'BUILD-ENV (101): a .json over the size bound refuses (it is not skipped)', 'scripts/guest_build_env.py',
     '    if len(data) > MAX_SERVICE_FILE:\n',
     '    if False and len(data) > MAX_SERVICE_FILE:\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2654', 'BUILD-ENV (101): a quoted User= is read', 'scripts/guest_build_env.py',
     'key, u = m.group(1), m.group(2).strip().strip("\\"\'").strip()',
     'key, u = m.group(1), m.group(2).strip()',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2655', 'BUILD-ENV (101): the drop-ins of an axon unit are read', 'scripts/guest_build_env.py',
     '            if os.path.isdir(dd):\n                for c in sorted(os.listdir(dd)):',
     '            if False and os.path.isdir(dd):\n                for c in sorted(os.listdir(dd)):',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2656', 'BUILD-ENV (101): a unit not named axon-* that runs an axon binary is a service', 'scripts/guest_build_env.py',
     '                if n.startswith("axon-") or re.search(r"^\\s*Exec\\w*\\s*=.*axon", text, re.M):\n',
     '                if n.startswith("axon-"):\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2657', 'BUILD-ENV (101): DynamicUser on a service unit refuses', 'scripts/guest_build_env.py',
     '    if re.search(r"^\\s*DynamicUser\\s*=\\s*(yes|true|1|on)\\b", text, re.M | re.I):\n',
     '    if False and re.search(r"^\\s*DynamicUser\\s*=\\s*(yes|true|1|on)\\b", text, re.M | re.I):\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2658', 'BUILD-ENV (101): the build GID (= uid) may not be a service gid', 'scripts/guest_build_env.py',
     '    if uid in sgids:                      # build_ids() runs the build as gid == uid\n',
     '    if False and uid in sgids:                      # build_ids() runs the build as gid == uid\n',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
    ('M2659', 'BUILD-ENV (101): a build uid that already owns processes is refused', 'scripts/guest_build_env.py',
     '    pids = build_uid_pids(uid)\n    return f"it already owns',
     '    pids = []\n    return f"it already owns',
     'axon-fabric', '--test guest_build_env_guards', 'the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses'),
]

# PSV1H (amendment 100): handler arms run under the installer's pin owner; name-resolving builtins take only name-pure names.
# WITHDRAWN by amendment 102 (the runtime taint): M2603, M2604, M2605 and M2607, the RUNNER legs of M2600, M2601, M2602 and M2606 (the SAME edits). At the runner the
# taint refuses the same attacks the static guard does, so a removed static guard SURVIVES there (measured: the four runner tests pass with each edit applied);
# the static guards stay rowed at the unit level (M2600-M2602, M2606), where the layers are judged separately.
MUTATIONS += [
    ('M2600',
     'PIN OWNER (psv1h): a bare tail-resume handler arm runs under the owner of the fn that installed the handler', 'crates/axon-core/src/interp/eval.rs',
     '            self.handler_edge_into(arm_sealed, &payload)?;\n            let _pin_guard = crate::interp::PinGuard {\n                cell: &self.pin_fn,\n                prev: self.pin_fn.replace(pin_owner),\n            };', '            self.handler_edge_into(arm_sealed, &payload)?;\n            let _pin_guard = crate::interp::PinGuard {\n                cell: &self.pin_fn,\n                prev: self.pin_fn.replace(self.pin_fn.get()),\n            };',
     'axon-core', '--lib', 'interp::tests::a_handler_arm_dispatch_runs_under_the_installers_pin_owner'),
    ('M2601',
     'PIN OWNER (psv1h): a general (replay) handler arm runs under the owner of the fn that installed the handler', 'crates/axon-core/src/interp/eval.rs',
     '        self.handler_edge_into(arm_sealed, &payload)?;\n        let _pin_guard = crate::interp::PinGuard {\n            cell: &self.pin_fn,\n            prev: self.pin_fn.replace(pin_owner),\n        };', '        self.handler_edge_into(arm_sealed, &payload)?;\n        let _pin_guard = crate::interp::PinGuard {\n            cell: &self.pin_fn,\n            prev: self.pin_fn.replace(self.pin_fn.get()),\n        };',
     'axon-core', '--lib', 'interp::tests::a_handler_arm_replay_runs_under_the_installers_pin_owner'),
    ('M2602',
     "PIN OWNER (psv1h): a bare tail-resume arm's ARITHMETIC is judged under the installing fn's owner", 'crates/axon-core/src/interp/eval.rs',
     '            self.handler_edge_into(arm_sealed, &payload)?;\n            let _pin_guard = crate::interp::PinGuard {\n                cell: &self.pin_fn,\n                prev: self.pin_fn.replace(pin_owner),\n            };', '            self.handler_edge_into(arm_sealed, &payload)?;\n            let _pin_guard = crate::interp::PinGuard {\n                cell: &self.pin_fn,\n                prev: self.pin_fn.replace(self.pin_fn.get()),\n            };',
     'axon-core', '--lib', 'interp::tests::a_handler_arm_arithmetic_runs_under_the_installers_pin_owner'),
    ('M2606',
     'NAME SINK (psv1h): an undetermined name argument of a name-resolving builtin is refused', 'crates/axon-core/src/interp.rs',
     '                if !self.pins.name_determined(self.pin_fn.get(), a) {', '                if false && !self.pins.name_determined(self.pin_fn.get(), a) {',
     'axon-core', '--lib', 'interp::tests::a_function_name_the_candidate_chose_never_selects_an_operator_fn'),
    ('M2608',
     'NAME SINK (psv1h): the call site consults the name rule before a name-resolving builtin runs', 'crates/axon-core/src/interp/eval.rs',
     '                    self.seal_name_args(name, args)?;\n', '',
     'axon-core', '--lib', 'interp::tests::a_function_name_the_candidate_chose_never_selects_an_operator_fn'),
    ('M2609',
     'NAME SINK (psv1h): scheduler_spawn is a listed name sink', 'crates/axon-core/src/interp/pin.rs',
     '    ("scheduler_spawn", &[0]),\n', '',
     'axon-core', '--lib', 'interp::tests::scheduler_spawn_takes_no_function_name_the_candidate_chose'),
    ('M2610',
     'NAME SINK (psv1h, runner): scheduler_spawn is a listed name sink', 'crates/axon-core/src/interp/pin.rs',
     '    ("scheduler_spawn", &[0]),\n', '',
     'axon-psv', '--test sealed_frames', 'scheduler_spawn_runs_no_function_name_the_candidate_chose'),
    ('M2611',
     'NAME SINK (psv1h): goal_eval is a listed name sink', 'crates/axon-core/src/interp/pin.rs',
     '    ("goal_eval", &[0]),\n', '',
     'axon-core', '--lib', 'interp::tests::goal_eval_takes_no_function_name_the_candidate_chose'),
    ('M2612',
     'NAME SINK (psv1h, runner): goal_eval is a listed name sink', 'crates/axon-core/src/interp/pin.rs',
     '    ("goal_eval", &[0]),\n', '',
     'axon-psv', '--test sealed_frames', 'goal_eval_runs_no_function_name_the_candidate_chose'),
    ('M2613',
     'NAME SINK (psv1h): sandbox_run is a listed name sink', 'crates/axon-core/src/interp/pin.rs',
     '    ("sandbox_run", &[1]),\n', '',
     'axon-core', '--lib', 'interp::tests::a_function_name_the_candidate_chose_never_selects_an_operator_fn'),
    ('M2614',
     "NAME SINK (psv1h): goal_run_constrained's CONSTRAINT name is a sink as well as its metric", 'crates/axon-core/src/interp/pin.rs',
     '    ("goal_run_constrained", &[0, 1]),', '    ("goal_run_constrained", &[0]),',
     'axon-core', '--lib', 'interp::tests::a_goal_constraint_and_a_kernel_goal_take_no_function_name_the_candidate_chose'),
    ('M2615',
     'NAME SINK (psv1h): kernel_goal_create is a listed name sink', 'crates/axon-core/src/interp/pin.rs',
     '    ("kernel_goal_create", &[1]),\n', '',
     'axon-core', '--lib', 'interp::tests::a_goal_constraint_and_a_kernel_goal_take_no_function_name_the_candidate_chose'),
    ('M2616',
     'NAME PURITY (psv1h): a parameter is never a name the operator chose', 'crates/axon-core/src/interp/pin.rs',
     '            if !name_mode && ctx.tys.is_closed(ty, gp) {', '            if ctx.tys.is_closed(ty, gp) {',
     'axon-core', '--lib', 'interp::tests::a_function_name_the_candidate_chose_never_selects_an_operator_fn'),
    ('M2617',
     'NAME PURITY (psv1h): a `str` annotation pins no name', 'crates/axon-core/src/interp/pin.rs',
     '        let pinned = |t: &T| !name_mode && tys.is_closed(t, gp);', '        let pinned = |t: &T| tys.is_closed(t, gp);',
     'axon-core', '--lib', 'interp::tests::a_function_name_the_candidate_chose_never_selects_an_operator_fn'),
    ('M2618',
     'NAME PURITY (psv1h): the INDEX of an element read is a name-pure source too', 'crates/axon-core/src/interp/pin.rs',
     '            Expr::Index { receiver, index } => d(receiver) && d(index),', '            Expr::Index { receiver, .. } => d(receiver),',
     'axon-core', '--lib', 'interp::tests::a_function_name_the_candidate_chose_never_selects_an_operator_fn'),
    ('M2619',
     "NAME PURITY (psv1h): an operator fn's `return x` values count toward its name-purity", 'crates/axon-core/src/interp/pin.rs',
     '    ctx.npure(body, &local, &bound) && rets.iter().all(|r| ctx.npure(r, &local, &bound))', '    ctx.npure(body, &local, &bound)',
     'axon-core', '--lib', 'interp::tests::a_function_name_the_candidate_chose_never_selects_an_operator_fn'),
    ('M2620',
     'NAME PURITY (psv1h): a loop variable is name-pure only when both bounds of its range are', 'crates/axon-core/src/interp/pin.rs',
     '                    facts.push((var.clone(), Fact::From(start)));\n                    facts.push((var.clone(), Fact::From(end)));', '                    facts.push((var.clone(), Fact::Pinned));',
     'axon-core', '--lib', 'interp::tests::a_function_name_the_candidate_chose_never_selects_an_operator_fn'),
    ('M2621',
     'NAME PURITY (psv1h): a name-pure builtin needs name-pure arguments', 'crates/axon-core/src/interp/pin.rs',
     'NAME_PURE_BUILTINS.contains(&name.as_str()) && args.iter().all(d)', 'NAME_PURE_BUILTINS.contains(&name.as_str())',
     'axon-core', '--lib', 'interp::tests::a_function_name_the_candidate_chose_never_selects_an_operator_fn'),
    ('M2622',
     'EXISTENCE ORACLE (psv1h): a missing fn reads to sealed code as an operator fn does', 'crates/axon-core/src/interp.rs',
     '        if self.seal.active && self.frame_sealed.get() {\n            return panic(Self::sealed_no_fn_msg(name));\n        }\n        panic(plain)', '        panic(plain)',
     'axon-core', '--lib', 'interp::tests::a_sealed_caller_cannot_tell_an_operator_fn_from_a_missing_one'),
    ('M2623',
     'EXISTENCE ORACLE (psv1h): an operator fn reads to sealed code as a missing one does', 'crates/axon-core/src/interp.rs',
     '            return panic(Self::sealed_no_fn_msg(&f.name));', '            return panic(format!("sealed code (the candidate under test) cannot run `{}`, which the operator defines", f.name));',
     'axon-core', '--lib', 'interp::tests::a_sealed_caller_cannot_tell_an_operator_fn_from_a_missing_one'),
    ('M2624',
     "PIN OWNER (psv1h): a closure made inside a general handler arm remembers the installing fn's owner", 'crates/axon-core/src/interp/eval.rs',
     '        self.handler_edge_into(arm_sealed, &payload)?;\n        let _pin_guard = crate::interp::PinGuard {\n            cell: &self.pin_fn,\n            prev: self.pin_fn.replace(pin_owner),\n        };', '        self.handler_edge_into(arm_sealed, &payload)?;\n        let _pin_guard = crate::interp::PinGuard {\n            cell: &self.pin_fn,\n            prev: self.pin_fn.replace(self.pin_fn.get()),\n        };',
     'axon-core', '--lib', 'interp::tests::a_closure_made_in_a_handler_arm_is_judged_by_the_installing_fn'),
    ('M2800',
     'PARSE (eqgate6): a suite reference lacking the check-suite: prefix is refused as such', 'crates/axon-cortex/src/runner.rs',
     '        .ok_or_else(|| bad("is not check-suite:<id>@<version>#<entry>"))?;', '        .unwrap_or(r);',
     'axon-cortex', '--test check_executor', 'a_suite_reference_or_registry_missing_a_part_is_refused_by_that_part'),
    ('M2801',
     'PARSE (eqgate6): a suite reference naming no version is refused as such', 'crates/axon-cortex/src/runner.rs',
     '        .ok_or_else(|| bad("names no version"))?;', '        .unwrap_or((rest, "x#y"));',
     'axon-cortex', '--test check_executor', 'a_suite_reference_or_registry_missing_a_part_is_refused_by_that_part'),
    ('M2802',
     'PARSE (eqgate6): a suite reference naming no entry is refused as such', 'crates/axon-cortex/src/runner.rs',
     '    let (version, entry) = rest.split_once(\'#\').ok_or_else(|| bad("names no entry"))?;', '    let (version, entry) = rest.split_once(\'#\').unwrap_or((rest, ""));',
     'axon-cortex', '--test check_executor', 'a_suite_reference_or_registry_missing_a_part_is_refused_by_that_part'),
    ('M2803',
     'REGISTRY (eqgate6): a check registry naming no executors array is refused', 'crates/axon-cortex/src/runner.rs',
     '            .ok_or("check registry has no `executors` array")?', '            .unwrap_or(&Vec::new())',
     'axon-cortex', '--test check_executor', 'a_suite_reference_or_registry_missing_a_part_is_refused_by_that_part'),
    ('M2804',
     'VALUES (eqgate6): the production qualification trust accepts evidence of 30 days at most', 'crates/axon-fabric/src/backend.rs',
     '            issuers_dir: TrustAuthority::Qualification.operator_dir(),\n            max_age_s: DEFAULT_EVIDENCE_MAX_AGE_S,', '            issuers_dir: TrustAuthority::Qualification.operator_dir(),\n            max_age_s: DEFAULT_EVIDENCE_MAX_AGE_S + 1,',
     'axon-fabric', '--test trust_root', 'production_trust_is_the_operator_root_and_must_be_operator_owned'),
    ('M2805',
     'VALUES (eqgate6): the production qualification trust names no host signer before the host config loads', 'crates/axon-fabric/src/backend.rs',
     '            operator_owned: true,\n            host_signer_public_key: None,\n        }\n    }\n\n    /// TESTS ONLY (feature', '            operator_owned: true,\n            host_signer_public_key: Some("0".repeat(64)),\n        }\n    }\n\n    /// TESTS ONLY (feature',
     'axon-fabric', '--test trust_root', 'production_trust_is_the_operator_root_and_must_be_operator_owned'),
    ('M2806',
     'VALUES (eqgate6): the production observer root is required to be operator-owned', 'crates/axon-fabric/src/observer.rs',
     '            dir: TrustAuthority::Observer.operator_dir(),\n            operator_owned: true,', '            dir: TrustAuthority::Observer.operator_dir(),\n            operator_owned: false,',
     'axon-fabric', '--test trust_root', 'the_production_observer_trust_is_the_operators_and_separate_from_every_other_root'),
    ('M2807',
     'VALUES (eqgate6): the production observer trust names no host signer before the host config loads', 'crates/axon-fabric/src/observer.rs',
     '                &TrustAuthority::Observer.operator_dir(),\n            ),\n            host_signer_public_key: None,', '                &TrustAuthority::Observer.operator_dir(),\n            ),\n            host_signer_public_key: Some("0".repeat(64)),',
     'axon-fabric', '--test trust_root', 'the_production_observer_trust_is_the_operators_and_separate_from_every_other_root'),
    ('M2808',
     'HAND-OVER (eqgate6): the group of a handed-over directory is left as the launcher set it', 'crates/axon-fabric/src/privileged_launcher.rs',
     '                    libc::fchmod(fd.as_raw_fd(), st.st_mode & 0o777) == 0\n                        && libc::fchown(fd.as_raw_fd(), uid, u32::MAX) == 0\n                }\n            }\n            libc::S_IFREG => {', '                    libc::fchmod(fd.as_raw_fd(), st.st_mode & 0o777) == 0\n                        && libc::fchown(fd.as_raw_fd(), uid, 0) == 0\n                }\n            }\n            libc::S_IFREG => {',
     'axon-fabric', '--test privileged_launcher', 'the_hand_over_changes_the_owner_of_the_out_tree_and_never_its_group'),
    ('M2809',
     'HAND-OVER (eqgate6): the group of a handed-over regular file is left as the launcher set it', 'crates/axon-fabric/src/privileged_launcher.rs',
     '                    libc::fchmod(fd.as_raw_fd(), st.st_mode & 0o777) == 0\n                        && libc::fchown(fd.as_raw_fd(), uid, u32::MAX) == 0\n                }\n            }\n            libc::S_IFLNK => unsafe {', '                    libc::fchmod(fd.as_raw_fd(), st.st_mode & 0o777) == 0\n                        && libc::fchown(fd.as_raw_fd(), uid, 0) == 0\n                }\n            }\n            libc::S_IFLNK => unsafe {',
     'axon-fabric', '--test privileged_launcher', 'the_hand_over_changes_the_owner_of_the_out_tree_and_never_its_group'),
    ('M2810',
     'HAND-OVER (eqgate6): the group of a handed-over symlink is left as the launcher set it', 'crates/axon-fabric/src/privileged_launcher.rs',
     'libc::fchownat(dir, cn.as_ptr(), uid, u32::MAX, libc::AT_SYMLINK_NOFOLLOW) == 0', 'libc::fchownat(dir, cn.as_ptr(), uid, 0, libc::AT_SYMLINK_NOFOLLOW) == 0',
     'axon-fabric', '--test privileged_launcher', 'the_hand_over_changes_the_owner_of_the_out_tree_and_never_its_group'),
    ('M2811',
     'HAND-OVER (eqgate6): the group of the handed-over out dir is left as the launcher set it', 'crates/axon-fabric/src/privileged_launcher.rs',
     '                && libc::fchown(p.out.as_raw_fd(), c.fabric_uid, u32::MAX) == 0', '                && libc::fchown(p.out.as_raw_fd(), c.fabric_uid, 0) == 0',
     'axon-fabric', '--test privileged_launcher', 'the_hand_over_changes_the_owner_of_the_out_tree_and_never_its_group'),
    ('M2812',
     'LAUNCHER OWNER (eqgate6): the pinned launcher program must be owned by the operator', 'crates/axon-fabric/src/privileged_launcher.rs',
     '    let owner = Some(a.operator_uid);\n    let launcher = sealed_exec::open_verified(&pin(&c.launcher), owner, a.lease())?;', '    let owner = None;\n    let launcher = sealed_exec::open_verified(&pin(&c.launcher), owner, a.lease())?;',
     'axon-fabric', '--test privileged_launcher', 'a_launcher_program_another_uid_owns_launches_nothing'),
    ('M2813',
     'GIT ENV (eqgate6): git runs in the C locale', 'crates/axon-fabric/src/git_data.rs',
     '        .env("LC_ALL", "C")\n        .env("GIT_NO_REPLACE_OBJECTS", "1")', '        .env("LC_ALL", "en_US.UTF-8")\n        .env("GIT_NO_REPLACE_OBJECTS", "1")',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_this_environment'),
    ('M2814',
     'GIT ENV (eqgate6): git reads no system config', 'crates/axon-fabric/src/git_data.rs',
     '        .env("GIT_CONFIG_NOSYSTEM", "1")\n        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_OPTIONAL_LOCKS", "0")', '        .env("GIT_CONFIG_NOSYSTEM", "0")\n        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_OPTIONAL_LOCKS", "0")',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_this_environment'),
    ('M2815',
     'GIT ENV (eqgate6): git reads no global config', 'crates/axon-fabric/src/git_data.rs',
     '        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_OPTIONAL_LOCKS", "0")', '        .env("GIT_CONFIG_GLOBAL", "/tmp/gitconfig")\n        .env("GIT_OPTIONAL_LOCKS", "0")',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_this_environment'),
    ('M2816',
     'GIT ENV (eqgate6): git never prompts on a terminal', 'crates/axon-fabric/src/git_data.rs',
     '        .env("GIT_TERMINAL_PROMPT", "0")\n        .arg("--no-replace-objects")', '        .env("GIT_TERMINAL_PROMPT", "1")\n        .arg("--no-replace-objects")',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_this_environment'),
    ('M2817',
     'GIT ARGS (eqgate6): git keeps no untracked cache', 'crates/axon-fabric/src/git_data.rs',
     '            "core.untrackedCache=false",', '            "core.untrackedCache=true",',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_these_arguments'),
    ('M2818',
     "GIT STREAMS (eqgate6): git reads no stdin of Fabric's", 'crates/axon-fabric/src/git_data.rs',
     '        .stdin(Stdio::null())\n        .stderr(Stdio::null());\n    c\n}', '        .stdin(Stdio::inherit())\n        .stderr(Stdio::null());\n    c\n}',
     'axon-fabric', '--test git_cmd', 'git_inherits_neither_stdin_nor_stderr'),
    ('M2819',
     "GIT STREAMS (eqgate6): git writes no stderr of Fabric's", 'crates/axon-fabric/src/git_data.rs',
     '        .stdin(Stdio::null())\n        .stderr(Stdio::null());\n    c\n}', '        .stdin(Stdio::null())\n        .stderr(Stdio::inherit());\n    c\n}',
     'axon-fabric', '--test git_cmd', 'git_inherits_neither_stdin_nor_stderr'),
    ('M2820',
     'WORKTREE (eqgate6): a tracked symlink is judged by its target', 'crates/axon-fabric/src/git_data.rs',
     '                0o120000 if md.file_type().is_symlink() => std::fs::read_link(&p)', '                0o120001 if md.file_type().is_symlink() => std::fs::read_link(&p)',
     'axon-fabric', '--lib', 'git_data::tests::a_tracked_symlink_is_judged_by_its_target_and_by_nothing_else'),
    ('M2821',
     'CHECK CHILD (eqgate6): the check child runs with exactly PATH=/usr/bin:/bin', 'crates/axon-psv/src/runner.rs',
     '.env("PATH", "/usr/bin:/bin")', '.env("PATH", "/in/candidate:/usr/bin:/bin")',
     'axon-psv', '--test runner', 'the_check_child_runs_in_the_suite_with_only_its_own_environment_and_stdio'),
    ('M2822',
     "CHECK CHILD (eqgate6): the check child's module path is exclusive", 'crates/axon-psv/src/runner.rs',
     '.env("AXON_PATH_EXCLUSIVE", "1")', '.env("AXON_PATH_EXCLUSIVE", "0")',
     'axon-psv', '--test runner', 'the_check_child_runs_in_the_suite_with_only_its_own_environment_and_stdio'),
    ('M2823',
     'CHECK CHILD (eqgate6): the check child runs `axon test`', 'crates/axon-psv/src/runner.rs',
     '        .arg("test")\n', '        .arg("check")\n',
     'axon-psv', '--test runner', 'the_check_child_runs_in_the_suite_with_only_its_own_environment_and_stdio'),
    ('M2824',
     'CHECK CHILD (eqgate6): the check child asks for JSON', 'crates/axon-psv/src/runner.rs',
     '        .arg("--json")\n', '        .arg("--jsonx")\n',
     'axon-psv', '--test runner', 'the_check_child_runs_in_the_suite_with_only_its_own_environment_and_stdio'),
    ('M2825',
     'CHECK CHILD (eqgate6): the check child names its test by --filter', 'crates/axon-psv/src/runner.rs',
     '        .arg("--filter")\n', '        .arg("--only")\n',
     'axon-psv', '--test runner', 'the_check_child_runs_in_the_suite_with_only_its_own_environment_and_stdio'),
    ('M2826',
     'CHECK CHILD (eqgate6): the check child reads the completion key from stdin', 'crates/axon-psv/src/runner.rs',
     '        .arg("--completion-key-stdin")\n', '        .arg("--completion-key-file")\n',
     'axon-psv', '--test runner', 'the_check_child_runs_in_the_suite_with_only_its_own_environment_and_stdio'),
    ('M2827',
     "HOST CONFIG (eqgate6): the loaded host's observer takes only a program of the executable owner", 'crates/axon-fabric/src/protected_host.rs',
     '                    interpreter,\n                    exec_owner: Some(exec_owner),', '                    interpreter,\n                    exec_owner: None,',
     'axon-fabric', '--test protected_host', 'a_loaded_host_requires_the_executable_owner_of_every_program_it_pins'),
    ('M2828',
     "HOST CONFIG (eqgate6): the loaded host's Linux profile takes only a program of the executable owner", 'crates/axon-fabric/src/protected_host.rs',
     '                exec_owner: Some(exec_owner),\n                interpreter: None,', '                exec_owner: None,\n                interpreter: None,',
     'axon-fabric', '--test protected_host', 'a_loaded_host_requires_the_executable_owner_of_every_program_it_pins'),
    ('M2829',
     'OWNER ARGUMENT (eqgate6): Fabric opens the privileged helper requiring the configured owner', 'crates/axon-fabric/src/backend.rs',
     '        &h.helper,\n        Some(h.owner),\n        crate::sealed_exec::Lease::IfGranted,\n    ) {', '        &h.helper,\n        None,\n        crate::sealed_exec::Lease::IfGranted,\n    ) {',
     'axon-fabric', '--test psv_dispatch', 'a_privileged_helper_owned_by_a_stranger_launches_nothing'),
    ('M2830',
     'OWNER ARGUMENT (eqgate6): Fabric opens the observe relay helper requiring the configured owner', 'crates/axon-fabric/src/observer.rs',
     '        &h.helper,\n        Some(h.owner),\n        crate::sealed_exec::Lease::IfGranted,\n    )\n    .map_err(|e| format!("privileged launcher {e}"))?;', '        &h.helper,\n        None,\n        crate::sealed_exec::Lease::IfGranted,\n    )\n    .map_err(|e| format!("privileged launcher {e}"))?;',
     'axon-fabric', '--test observer_service', 'an_observe_relay_helper_owned_by_a_stranger_is_never_executed'),
    ('M2831',
     'COVERAGE GATE (eqgate6): a literal handed to `.env(K, V)` is a value site', 'scripts/v022_refusal_coverage.py',
     '_VALUE_CALLS = re.compile(\n    r"\\.(env|env_remove|', '_VALUE_CALLS = re.compile(\n    r"\\.(env_remove|',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_literal_handed_to_a_spawn_builder_is_a_site_of_its_own'),
    ('M2832',
     'COVERAGE GATE (eqgate6): a row credits a value only by an edit of THAT value', 'scripts/v022_refusal_coverage.py',
     '        elif x < b and y > a:\n            return True', '        elif True:\n            return True',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_value_is_credited_only_by_an_edit_of_that_value'),
    ('M2833',
     'COVERAGE GATE (eqgate6): a `Some(<x>.owner|uid|gid)` owner argument is a value site', 'scripts/v022_refusal_coverage.py',
     '_OWNER_ARG = re.compile(r"\\bSome\\(', '_OWNER_ARG = re.compile(r"\\bSomeNever\\(',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_owner_argument_a_privilege_argument_and_a_mode_literal_are_sites'),
    ('M2834',
     'COVERAGE GATE (eqgate6): a standalone permission mode literal is a value site', 'scripts/v022_refusal_coverage.py',
     '_OCTAL = re.compile(r"\\b0o[0-7_]+\\b")', '_OCTAL = re.compile(r"\\b0o[0-7_]+X\\b")',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_owner_argument_a_privilege_argument_and_a_mode_literal_are_sites'),
    ('M2835',
     'COVERAGE GATE (eqgate6): the mode argument of mkdir/chmod/umask/.mode( is a value site', 'scripts/v022_refusal_coverage.py',
     '_MODE_CALLS = re.compile(\n    r"(?<![\\w:.])(?:libc::)?(f?chmod(?:at)?|mkdirat|mkdir|umask)\\(', '_MODE_CALLS = re.compile(\n    r"(?<![\\w:.])(?:libc::)?(f?chmodX(?:at)?|mkdirat|mkdir|umask)\\(',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_owner_argument_a_privilege_argument_and_a_mode_literal_are_sites'),
    ('M2836',
     'COVERAGE GATE (eqgate6): a uid/gid argument of chown/setuid is a value site', 'scripts/v022_refusal_coverage.py',
     '(f?l?chown(?:at)?|setuid|', '(f?l?chownX(?:at)?|setuid|',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_owner_argument_a_privilege_argument_and_a_mode_literal_are_sites'),
    ('M2837',
     'COVERAGE GATE (eqgate6): a mode inside a message macro is not a decision', 'scripts/v022_refusal_coverage.py',
     '                and not any(a <= m.start() < b for a, b in msgs)):', '                and True):',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'an_owner_argument_a_privilege_argument_and_a_mode_literal_are_sites'),
    ('M2838',
     'COVERAGE GATE (eqgate6): a literal field of a Config/Policy/... struct literal is a value site', 'scripts/v022_refusal_coverage.py',
     '_STRUCT_WORDS = ("Config", ', '_STRUCT_WORDS = ("ConfigX", ',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_literal_field_of_a_config_or_policy_struct_is_a_site'),
    ('M2839',
     'COVERAGE GATE (eqgate6): a #[cfg(test)] item holds no value site', 'scripts/v022_refusal_coverage.py',
     '            if chars[k] != "\\n":\n                chars[k] = " "\n        pos = end\n    return "".join(chars)', '            if chars[k] != "\\n":\n                pass\n        pos = end\n    return "".join(chars)',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_literal_handed_to_a_spawn_builder_is_a_site_of_its_own'),
    ('M2840',
     'COVERAGE GATE (eqgate6): a `.stdin/.stdout/.stderr(Stdio::..)` is a value site', 'scripts/v022_refusal_coverage.py',
     '                if "Stdio::" in clean[a:b]:', '                if False:',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_literal_handed_to_a_spawn_builder_is_a_site_of_its_own'),
    ('M2841',
     'COVERAGE GATE (eqgate6): a literal `.current_dir(V)` is a value site', 'scripts/v022_refusal_coverage.py',
     '        elif name == "current_dir":', '        elif name == "current_dirX":',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_literal_handed_to_a_spawn_builder_is_a_site_of_its_own'),
    ('M2842',
     'COVERAGE GATE (eqgate6): a literal `.arg(V)` is a value site', 'scripts/v022_refusal_coverage.py',
     '        elif name == "arg":', '        elif name == "argX":',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_literal_handed_to_a_spawn_builder_is_a_site_of_its_own'),
    ('M2843',
     'COVERAGE GATE (eqgate6): a value exemption naming a fragment that is not the value is refused', 'scripts/v022_refusal_coverage.py',
     '            if hit[0][3] not in " ".join(frag.split()) and hit[0][3] not in frag:', '            if False:',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_value_is_credited_only_by_an_edit_of_that_value'),
    ('M2844',
     'COVERAGE GATE (eqgate6): a REMAINDER value exemption is counted by its val_* category', 'scripts/v022_refusal_coverage.py',
     '                REMAINDER_SITES.append((f, line_of(text, a) + 1, label))', '                pass',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'a_value_is_credited_only_by_an_edit_of_that_value'),
    ('M2845',
     'COVERAGE GATE (eqgate6): the gate prints what it still cannot see', 'scripts/v022_refusal_coverage.py',
     '        print("STILL BLIND: " + item)', '        pass',
     'axon-core', '--no-default-features --test refusal_coverage_gate', 'the_gate_prints_what_it_still_cannot_see'),
    ('M2846',
     'GIT PAIR MEMBER (eqgate6): GIT_NO_LAZY_FETCH alone (its partner protocol.allow=never still holds)', 'crates/axon-fabric/src/git_data.rs',
     '        .env("GIT_NO_LAZY_FETCH", "1")', '        .env("GIT_NO_LAZY_FETCH", "0")',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_this_environment'),
    ('M2847',
     'GIT PAIR MEMBER (eqgate6): protocol.allow=never alone (its partner GIT_NO_LAZY_FETCH still holds)', 'crates/axon-fabric/src/git_data.rs',
     '        .args(["-c", "protocol.allow=never"])', '        .args(["-c", "protocol.allow=always"])',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_these_arguments'),
    ('M2848',
     'GIT PAIR MEMBER (eqgate6): GIT_NO_REPLACE_OBJECTS alone (its partner --no-replace-objects still holds)', 'crates/axon-fabric/src/git_data.rs',
     '        .env("GIT_NO_REPLACE_OBJECTS", "1")\n        .env("GIT_CONFIG_NOSYSTEM", "1")', '        .env("GIT_NO_REPLACE_OBJECTS", "0")\n        .env("GIT_CONFIG_NOSYSTEM", "1")',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_this_environment'),
    ('M2849',
     'GIT PAIR MEMBER (eqgate6): --no-replace-objects alone (its partner GIT_NO_REPLACE_OBJECTS still holds)', 'crates/axon-fabric/src/git_data.rs',
     '        .arg("--no-replace-objects")', '        .arg("--no-advice")',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_these_arguments'),
    ('M2850',
     'GIT HARDENING (eqgate6): core.hooksPath is /dev/null', 'crates/axon-fabric/src/git_data.rs',
     '"core.hooksPath=/dev/null"])', '"core.hooksPath=/tmp"])',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_these_arguments'),
    ('M2851',
     'GIT HARDENING (eqgate6): git runs with PATH=/usr/bin:/bin', 'crates/axon-fabric/src/git_data.rs',
     '        .env("PATH", "/usr/bin:/bin")\n        .env("LC_ALL", "C")\n        .env("GIT_NO_REPLACE_OBJECTS", "1")', '        .env("PATH", "/tmp:/usr/bin:/bin")\n        .env("LC_ALL", "C")\n        .env("GIT_NO_REPLACE_OBJECTS", "1")',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_this_environment'),
    ('M2852',
     'GIT HARDENING (eqgate6): git never takes the optional locks', 'crates/axon-fabric/src/git_data.rs',
     '        .env("GIT_OPTIONAL_LOCKS", "0")', '        .env("GIT_OPTIONAL_LOCKS", "1")',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_this_environment'),
    ('M2853',
     "GIT HARDENING (eqgate6): git is the operator's /usr/bin/git, not one found on a PATH", 'crates/axon-fabric/src/git_data.rs',
     'pub const GIT_BIN: &str = "/usr/bin/git";', 'pub const GIT_BIN: &str = "git";',
     'axon-fabric', '--test git_cmd', 'git_is_run_with_exactly_this_environment'),
]

# PSV1T (amendment 102): the runtime taint at the selection primitives and at every carrier a value travels by.
MUTATIONS += [
    ('M2700', 'TAINT (am102): operator code does not call an operator closure the candidate picked: the call site', 'crates/axon-core/src/interp.rs', '        self.t_check_call_picked(&captured)?;\n', '', 'axon-core', '--lib', 'interp::taint_tests::an_operator_closure_the_candidate_picked_is_never_called'),
    ('M2701', 'TAINT (am102): a picked closure is refused', 'crates/axon-core/src/interp/taint.rs', '        if self.t_is_picked(captured) {', '        if false && self.t_is_picked(captured) {', 'axon-core', '--lib', 'interp::taint_tests::an_operator_closure_the_candidate_picked_is_never_called'),
    ('M2702', 'TAINT (am102): a closure that leaves an evaluation carrying VAL is picked', 'crates/axon-core/src/interp/taint.rs', '                    i.taint\n                        .picked\n                        .borrow_mut()\n                        .entry(a)\n                        .or_insert_with(|| v.clone());', '                    let _ = a;', 'axon-core', '--lib', 'interp::taint_tests::an_operator_closure_the_candidate_picked_is_never_called'),
    ('M2703', 'TAINT (am102): a closure read out of a tainted binding for a call by name is picked', 'crates/axon-core/src/interp/eval.rs', '                    if bt & taint::VAL != 0 {\n                        self.t_note(c, bt);\n                    }', '                    let _ = bt;', 'axon-core', '--lib', 'interp::taint_tests::an_operator_closure_the_candidate_picked_is_never_called'),
    ('M2704', 'TAINT (am102): the name argument of a name-resolving builtin is judged at the call site', 'crates/axon-core/src/interp/eval.rs', '                    self.t_check_names(name, &ats)?;\n', '', 'axon-core', '--lib', 'interp::taint_tests::a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn'),
    ('M2705', 'TAINT (am102): a name that carries VAL is refused', 'crates/axon-core/src/interp/taint.rs', '            if ats.get(i).copied().unwrap_or(0) & VAL != 0 {', '            if false && ats.get(i).copied().unwrap_or(0) & VAL != 0 {', 'axon-core', '--lib', 'interp::taint_tests::a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn'),
    ('M2706', 'TAINT (am102): a method call is judged by the taint of its receiver', 'crates/axon-core/src/interp/eval.rs', '                    self.t_check_dispatch(f, &argv[0], rt, &tn)?;\n', '', 'axon-core', '--lib', 'interp::taint_tests::a_type_or_width_sealed_code_chose_is_never_dispatched_on'),
    ('M2707', 'TAINT (am102): fixed-width arithmetic is judged by the taint of its left operand', 'crates/axon-core/src/interp/eval.rs', '            if lt & taint::TYP != 0 {\n                self.t_check_width(&l, lt, &format!("{op:?}"))?;\n            }', '', 'axon-core', '--lib', 'interp::taint_tests::a_type_or_width_sealed_code_chose_is_never_dispatched_on'),
    ('M2708', 'TAINT (am102): fixed-width arithmetic is judged by the taint of its right operand', 'crates/axon-core/src/interp/eval.rs', '            if rt & taint::TYP != 0 {\n                self.t_check_width(&r, rt, &format!("{op:?}"))?;\n            }', '', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2709', 'TAINT (am102): unary arithmetic is judged by the taint of its operand', 'crates/axon-core/src/interp/eval.rs', '                if vt & taint::TYP != 0 && matches!(op, UnaryOp::Neg | UnaryOp::BitNot) {\n                    self.t_check_width(&v, vt, &format!("{op:?}"))?;\n                }', '', 'axon-core', '--lib', 'interp::taint_tests::a_type_or_width_sealed_code_chose_is_never_dispatched_on'),
    ('M2710', 'TAINT (am102): everything a sealed frame computes carries both taints', 'crates/axon-core/src/interp/taint.rs', '            tn.acc.set(ALL);\n            tn.last.set(ALL);', '            tn.acc.set(0);\n            tn.last.set(0);', 'axon-core', '--lib', 'interp::taint_tests::a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn'),
    ('M2711', 'TAINT (am102): a subexpression merges its taint into the expression around it', 'crates/axon-core/src/interp/taint.rs', '        tn.acc.set(saved | mine);', '        tn.acc.set(saved);', 'axon-core', '--lib', 'interp::taint_tests::a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn'),
    ('M2712', 'TAINT (am102): reading a name takes the taint of its binding', 'crates/axon-core/src/interp/taint.rs', '            Expr::Ident(n) => self.t_touch(self.t_holder(n, env)),', '            Expr::Ident(_) => {}', 'axon-core', '--lib', 'interp::taint_tests::a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn'),
    ('M2713', 'TAINT (am102): reading a field of a binding takes the binding taint', 'crates/axon-core/src/interp/taint.rs', '            Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } => {\n                if let Expr::Ident(n) = receiver.as_ref() {', '            Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } => {\n                if let (Expr::Ident(n), false) = (receiver.as_ref(), matches!(expr, Expr::FieldAccess { .. })) {', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2714', 'TAINT (am102): reading an element of a binding takes the binding taint', 'crates/axon-core/src/interp/taint.rs', '            Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } => {\n                if let Expr::Ident(n) = receiver.as_ref() {', '            Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } => {\n                if let (Expr::Ident(n), false) = (receiver.as_ref(), matches!(expr, Expr::Index { .. })) {', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2715', 'TAINT (am102): a module-level let carries the taint of its initialiser', 'crates/axon-core/src/interp/taint.rs', '        self.taint.lets.borrow().get(name).copied().unwrap_or(0)', '        0', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2716', 'TAINT (am102): the initialiser of a module-level let records its taint', 'crates/axon-core/src/interp.rs', '                self.t_set_global(name, self.taint.last.get());', '', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2717', 'TAINT (am102): a `let` binding carries the taint of its value', 'crates/axon-core/src/interp/eval.rs', '                env.define(name.clone(), v, vt);\n                Ok(Value::Unit)', '                env.define(name.clone(), v, 0);\n                Ok(Value::Unit)', 'axon-core', '--lib', 'interp::taint_tests::a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn'),
    ('M2718', 'TAINT (am102): an assignment stores the taint of its value', 'crates/axon-core/src/interp/eval.rs', '                    self.ts::<T>(self.tl::<T>())\n                } else {\n                    0\n                };\n                if env.assign(name, v, vt) {', '                    0\n                } else {\n                    0\n                };\n                if env.assign(name, v, vt) {', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2719', 'TAINT (am102): a write into a slot taints the binding that holds it', 'crates/axon-core/src/interp/eval.rs', '                env.taint_or(&base, wt);', '                let _ = wt;', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2720', 'TAINT (am102): an append taints the binding it grows', 'crates/axon-core/src/interp/eval.rs', '            env.taint_or(name, yt);', '            let _ = yt;', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2721', 'TAINT (am102): a channel send marks the channel with the sent taint', 'crates/axon-core/src/interp/eval.rs', '                                self.t_mark_obj(&recv, st);', '                                let _ = st;', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2722', 'TAINT (am102): a channel receive takes the channel taint', 'crates/axon-core/src/interp/eval.rs', '                        "recv" => {\n                            if T {\n                                self.t_touch(self.t_obj(&recv));\n                            }', '                        "recv" => {', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2723', 'TAINT (am102): a non-blocking receive takes the channel taint', 'crates/axon-core/src/interp/eval.rs', '                        "try_recv" => {\n                            if T {\n                                self.t_touch(self.t_obj(&recv));\n                            }', '                        "try_recv" => {', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2724', 'TAINT (am102): a builtin takes the taint of the shared objects it is given', 'crates/axon-core/src/interp/taint.rs', '        for a in args {\n            self.t_touch(self.t_obj(a));\n        }', '', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2725', 'TAINT (am102): a dict writer marks its dict', 'crates/axon-core/src/interp/taint.rs', '                self.t_mark_obj(d, self.t_stored(a));', '                let _ = d;', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2726', 'TAINT (am102): a kernel builtin reads the kernel taint', 'crates/axon-core/src/interp/taint.rs', '            Some(Class::Kernel) => self.t_touch(self.taint.kernel.get()),', '            Some(Class::Kernel) => {}', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2727', 'TAINT (am102): a kernel builtin writes the kernel taint', 'crates/axon-core/src/interp/taint.rs', '                if !sealed {\n                    self.taint.kernel.set(self.taint.kernel.get() | a);\n                }\n            }\n            Some(Class::World)', '            }\n            Some(Class::World)', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2728', 'TAINT (am102): a world builtin reads the world taint', 'crates/axon-core/src/interp/taint.rs', '            Some(Class::World) => self.t_touch(self.taint.world.get()),', '            Some(Class::World) => {}', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2729', 'TAINT (am102): a world builtin writes the world taint', 'crates/axon-core/src/interp/taint.rs', '            Some(Class::World) => self.taint.world.set(self.taint.world.get() | a),', '            Some(Class::World) => {}', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2730', 'TAINT (am102): a builtin with an open return keeps the type taint', 'crates/axon-core/src/interp/taint.rs', '                        untype: closed_scalar_ret(b.ret),', '                        untype: true,', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2731', 'TAINT (am102): only a closed annotation pins a type', 'crates/axon-core/src/interp/pin.rs', '        self.tys.is_closed(t, &[])', '        {\n            let _ = t;\n            true\n        }', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2732', 'TAINT (am102): a call carries the taint its body produced', 'crates/axon-core/src/interp.rs', '            self.taint.acc.set(entry_t | body_t);\n        }\n        Ok(result)', '            self.taint.acc.set(entry_t);\n        }\n        Ok(result)', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2733', 'TAINT (am102): a body value carries its own taint', 'crates/axon-core/src/interp.rs', '            } else {\n                self.taint.last.get()\n            }\n        } else {\n            0\n        };\n        if capture', '            } else {\n                0\n            }\n        } else {\n            0\n        };\n        if capture', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2734', 'TAINT (am102): a `return` carries its own taint out of the frame', 'crates/axon-core/src/interp.rs', '            if matches!(body_result, Err(Flow::Return(_))) {\n                self.taint.ret.get()', '            if matches!(body_result, Err(Flow::Return(_))) {\n                0', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2735', 'TAINT (am102): a parameter binds the taint of the argument', 'crates/axon-core/src/interp.rs', '                pt = entry_t | self.taint.pc.get();', '                pt = 0;', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2736', 'TAINT (am102): a closure parameter binds the taint of the argument', 'crates/axon-core/src/interp.rs', '            entry_t | self.taint.pc.get()\n        };\n        // A closure runs', '            0\n        };\n        // A closure runs', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2737', 'TAINT (am102): an argument sealed code gives an operator closure carries both taints', 'crates/axon-core/src/interp.rs', '        } else if self.frame_sealed.get() {\n            taint::ALL\n        } else {\n            entry_t', '        } else if self.frame_sealed.get() {\n            0\n        } else {\n            entry_t', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2738', 'TAINT (am102): an untyped closure parameter is not a pin', 'crates/axon-core/src/interp.rs', '                && Self::param_types(&contract, i)\n                    .iter()\n                    .any(|ty| self.t_pins(ty))', '                && true', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2739', 'TAINT (am102): a closure result carries its own taint', 'crates/axon-core/src/interp.rs', '                            ret_t = self.taint.last.get();', '                            ret_t = 0;', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2740', 'TAINT (am102): a closure `return` carries its own taint', 'crates/axon-core/src/interp.rs', '                            ret_t = self.taint.ret.get();', '                            ret_t = 0;', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2741', 'TAINT (am102): a closure body writes the taint of its captured names back (shared cell)', 'crates/axon-core/src/interp.rs', '                        if *t != 0 {\n                            cell.insert(ck, Value::Int(i64::from(*t)));', '                        if false {\n                            cell.insert(ck, Value::Int(i64::from(*t)));', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2742', 'TAINT (am102): a closure body writes the taint of its captured names back (lent cell)', 'crates/axon-core/src/interp.rs', '            if t != 0 {\n                cell.insert(format!("{}{k}", taint::CAP_T), Value::Int(i64::from(t)));', '            if false {\n                cell.insert(format!("{}{k}", taint::CAP_T), Value::Int(i64::from(t)));', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2743', 'TAINT (am102): a closure loads the taint of the names it captured', 'crates/axon-core/src/interp.rs', '                if let Value::Int(t) = v {\n                    comp.push((name.to_string(), t as u8));\n                }', '                let _ = v;', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2744', 'TAINT (am102): a closure records the taint of the names it captures', 'crates/axon-core/src/interp.rs', '                if *t != 0 {\n                    out.insert(ck, Value::Int(i64::from(*t)));', '                if false {\n                    out.insert(ck, Value::Int(i64::from(*t)));', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2745', 'TAINT (am102): a `&mut` argument a candidate fn wrote carries both taints back', 'crates/axon-core/src/interp.rs', '                (crate::ast::AxonType::RefMut(_), true) if crossing => taint::ALL,', '                (crate::ast::AxonType::RefMut(_), true) if crossing => 0,', 'axon-core', '--lib', 'interp::taint_tests::taint_survives_every_carrier_a_value_can_travel_by'),
    ('M2746', 'TAINT (am102): a `&mut` argument takes back the taint the callee left in it', 'crates/axon-core/src/interp/eval.rs', '                env.taint_or(name, out_ts[i]);', '                let _ = out_ts[i];', 'axon-core', '--lib', 'interp::taint_tests::taint_survives_every_carrier_a_value_can_travel_by'),
    ('M2747', 'TAINT (am102): a `&mut` argument carries its taint into the callee', 'crates/axon-core/src/interp/eval.rs', '            for (_, name) in &borrowed {\n                self.t_touch(env.taint_of(name));\n            }', '', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2748', 'TAINT (am102): a tainted branch taints what is stored in it', 'crates/axon-core/src/interp/taint.rs', '            let _g = self.t_pc(ct);\n', '', 'axon-core', '--lib', 'interp::taint_tests::a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn'),
    ('M2749', 'TAINT (am102): a store carries the control taint of the branch it ran in', 'crates/axon-core/src/interp/taint.rs', '        value_taint | ((self.taint.pc.get() | self.taint.sticky.get()) & VAL)', '        value_taint | (self.taint.sticky.get() & VAL)', 'axon-core', '--lib', 'interp::taint_tests::a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn'),
    ('M2750', 'TAINT (am102): a store after an early exit carries the exit condition', 'crates/axon-core/src/interp/taint.rs', '        value_taint | ((self.taint.pc.get() | self.taint.sticky.get()) & VAL)', '        value_taint | (self.taint.pc.get() & VAL)', 'axon-core', '--lib', 'interp::taint_tests::taint_survives_every_carrier_a_value_can_travel_by'),
    ('M2751', 'TAINT (am102): a branch that can exit early leaves the exit condition raised', 'crates/axon-core/src/interp/taint.rs', '        if exits {\n            let st = &self.taint.sticky;', '        if false && exits {\n            let st = &self.taint.sticky;', 'axon-core', '--lib', 'interp::taint_tests::taint_survives_every_carrier_a_value_can_travel_by'),
    ('M2752', 'TAINT (am102): a `break` is an early exit', 'crates/axon-core/src/interp/taint.rs', '        hit |= matches!(x, Expr::Return(_) | Expr::Break | Expr::Continue)', '        hit |= matches!(x, Expr::Return(_) | Expr::Continue)', 'axon-core', '--lib', 'interp::taint_tests::taint_survives_every_carrier_a_value_can_travel_by'),
    ('M2753', 'TAINT (am102): a `continue` is an early exit', 'crates/axon-core/src/interp/taint.rs', '        hit |= matches!(x, Expr::Return(_) | Expr::Break | Expr::Continue)', '        hit |= matches!(x, Expr::Return(_) | Expr::Break)', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2754', 'TAINT (am102): a `return` is an early exit', 'crates/axon-core/src/interp/taint.rs', '        hit |= matches!(x, Expr::Return(_) | Expr::Break | Expr::Continue)', '        hit |= matches!(x, Expr::Break | Expr::Continue)', 'axon-core', '--lib', 'interp::taint_tests::taint_survives_every_carrier_a_value_can_travel_by'),
    ('M2755', 'TAINT (am102): a fn result carries the condition of an early exit', 'crates/axon-core/src/interp.rs', '            body_t |= self.taint.sticky.get() & taint::VAL;\n', '', 'axon-core', '--lib', 'interp::taint_tests::taint_survives_every_carrier_a_value_can_travel_by'),
    ('M2756', 'TAINT (am102): a closure result carries the condition of an early exit', 'crates/axon-core/src/interp.rs', '        ret_t |= self.taint.sticky.get() & taint::VAL;\n', '', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2757', 'TAINT (am102): a `return` value carries its taint', 'crates/axon-core/src/interp/eval.rs', '                    self.taint.ret.set(self.ts::<T>(self.ta::<T>()));', '                    self.taint.ret.set(0);', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2758', 'TAINT (am102): a `?` that returns early carries its taint', 'crates/axon-core/src/interp/eval.rs', '                    self.taint.ret.set(self.ts::<T>(self.tl::<T>()));\n                }\n                match r {', '                    self.taint.ret.set(0);\n                }\n                match r {', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2759', 'TAINT (am102): a handler arm is bound the taint of the operation it answers', 'crates/axon-core/src/interp/eval.rs', '        let pt = self.t_stored(self.taint.acc.get());', '        let pt = 0;', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2760', "TAINT (am102): the value of a `with` block is its return arm's input taint", 'crates/axon-core/src/interp/eval.rs', '        let mut vt = self.t_last();\n        if matches!(result, Err(Flow::HandlerDone(_, d)) if d == depth) {', '        let mut vt = 0;\n        if matches!(result, Err(Flow::HandlerDone(_, d)) if d == depth) {', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2761', 'TAINT (am102): a handler arm that completes carries its value taint to the block', 'crates/axon-core/src/interp/eval.rs', '            vt = self.taint.acc.get();', '            vt = 0;', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2762', 'TAINT (am102): a name bound by a pattern carries the taint of the matched value', 'crates/axon-core/src/interp/eval.rs', '                    if self.match_pattern(&arm.pattern, &v, env, self.ts::<T>(st))? {', '                    if self.match_pattern(&arm.pattern, &v, env, 0)? {', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2763', 'TAINT (am102): a loop variable carries the taint of the range the candidate sized', 'crates/axon-core/src/interp/eval.rs', '                    env.define(var.clone(), Value::Int(i), self.ts::<T>(bt) & taint::VAL);', '                    env.define(var.clone(), Value::Int(i), 0);', 'axon-core', '--lib', 'interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own'),
    ('M2764', 'TAINT (am102, runner): the call of an operator closure the candidate picked is refused', 'crates/axon-core/src/interp.rs', '        self.t_check_call_picked(&captured)?;\n', '', 'axon-psv', '--test sealed_frames', 'an_operator_closure_the_candidate_picked_is_never_called'),
    ('M2765', 'TAINT (am102, runner): a picked operator closure is refused', 'crates/axon-core/src/interp/taint.rs', '        if self.t_is_picked(captured) {', '        if false && self.t_is_picked(captured) {', 'axon-psv', '--test sealed_frames', 'an_operator_closure_the_candidate_picked_is_never_called'),
    ('M2766', 'TAINT (am102, runner): a closure that leaves an evaluation carrying VAL is picked', 'crates/axon-core/src/interp/taint.rs', '                    i.taint\n                        .picked\n                        .borrow_mut()\n                        .entry(a)\n                        .or_insert_with(|| v.clone());', '                    let _ = a;', 'axon-psv', '--test sealed_frames', 'an_operator_closure_the_candidate_picked_is_never_called'),
    ('M2767', 'TAINT (am102, runner): a closure read out of a tainted binding for a call by name is picked', 'crates/axon-core/src/interp/eval.rs', '                    if bt & taint::VAL != 0 {\n                        self.t_note(c, bt);\n                    }', '                    let _ = bt;', 'axon-psv', '--test sealed_frames', 'an_operator_closure_the_candidate_picked_is_never_called'),
]


# ── Amendment 107 (C9 round 11, eqgate7): the child's argv/environ, the owner at every consumer ──
MUTATIONS += [
    ('M2960',
     "ROOT LAUNCH PATH (eq7): the root helper's PATH for its launcher child is exactly the system directories", 'crates/axon-fabric/src/privileged_launcher.rs',
     'const PATH_ENV: &str = "/usr/sbin:/usr/bin:/sbin:/bin";', 'const PATH_ENV: &str = "/tmp:/usr/sbin:/usr/bin:/sbin:/bin";',
     'axon-fabric', '--test privileged_launcher', 'the_root_helper_hands_its_launcher_exactly_its_flags_and_its_path'),
    ('M2961',
     "ROOT LAUNCH PATH (eq7): Fabric's PATH for the privileged helper is exactly the system directories", 'crates/axon-fabric/src/backend.rs',
     'const LAUNCH_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin:/usr/local/bin";', 'const LAUNCH_PATH: &str = "/tmp:/usr/sbin:/usr/bin:/sbin:/bin:/usr/local/bin";',
     'axon-fabric', '--test psv_dispatch', 'fabric_hands_the_privileged_helper_exactly_its_flag_and_its_path'),
    ('M2962',
     "ROOT LAUNCH ARGV (eq7): the root helper names its launcher's timeout flag --timeout-s", 'crates/axon-fabric/src/privileged_launcher.rs',
     '        "--timeout-s".into(),', '        "--timeout-sx".into(),',
     'axon-fabric', '--test privileged_launcher', 'the_root_helper_hands_its_launcher_exactly_its_flags_and_its_path'),
    ('M2963',
     'ROOT LAUNCH ARGV (eq7): the root helper names the firecracker flag --fc-bin', 'crates/axon-fabric/src/privileged_launcher.rs',
     '        "--fc-bin".into(),', '        "--fc-binx".into(),',
     'axon-fabric', '--test privileged_launcher', 'the_root_helper_hands_its_launcher_exactly_its_flags_and_its_path'),
    ('M2964',
     'ROOT LAUNCH ARGV (eq7): the root helper names the jailer flag --jailer-bin', 'crates/axon-fabric/src/privileged_launcher.rs',
     '        "--jailer-bin".into(),', '        "--jailer-binx".into(),',
     'axon-fabric', '--test privileged_launcher', 'the_root_helper_hands_its_launcher_exactly_its_flags_and_its_path'),
    ('M2965',
     'ROOT LAUNCH ARGV (eq7): the firecracker and jailer paths are handed under their own flags', 'crates/axon-fabric/src/privileged_launcher.rs',
     '        "--fc-bin".into(),\n        s(&c.firecracker),\n        "--jailer-bin".into(),\n        s(&c.jailer),', '        "--fc-bin".into(),\n        s(&c.jailer),\n        "--jailer-bin".into(),\n        s(&c.firecracker),',
     'axon-fabric', '--test privileged_launcher', 'the_root_helper_hands_its_launcher_exactly_its_flags_and_its_path'),
    ('M2966',
     'OWNER FIELD (eq7): the direct route opens its launcher requiring the configured executable owner', 'crates/axon-fabric/src/backend.rs',
     '        lx.exec_owner,\n        Lease::IfGranted,\n    ) {\n        Ok(v) => v,\n        Err(e) => return refused(format!("launcher: {e}")),', '        None,\n        Lease::IfGranted,\n    ) {\n        Ok(v) => v,\n        Err(e) => return refused(format!("launcher: {e}")),',
     'axon-fabric', '--test psv_dispatch', 'a_pinned_program_another_uid_owns_is_refused_at_every_consumer_of_the_owner'),
    ('M2967',
     "OWNER FIELD (eq7): the direct route opens its launcher's interpreter requiring the configured executable owner", 'crates/axon-fabric/src/backend.rs',
     'sealed_exec::open_verified(&p, lx.exec_owner, Lease::IfGranted))', 'sealed_exec::open_verified(&p, None, Lease::IfGranted))',
     'axon-fabric', '--test psv_dispatch', 'a_pinned_program_another_uid_owns_is_refused_at_every_consumer_of_the_owner'),
    ('M2968',
     'OWNER FIELD (eq7): the observer program is opened requiring the configured executable owner', 'crates/axon-fabric/src/observer.rs',
     '        cfg.exec_owner,\n        Lease::IfGranted,\n    )\n    .map_err(|e| format!("observer {e}"))?;', '        None,\n        Lease::IfGranted,\n    )\n    .map_err(|e| format!("observer {e}"))?;',
     'axon-fabric', '--test psv_dispatch', 'a_pinned_program_another_uid_owns_is_refused_at_every_consumer_of_the_owner'),
    ('M2969',
     "OWNER FIELD (eq7): the observer's interpreter is opened requiring the configured executable owner", 'crates/axon-fabric/src/observer.rs',
     'sealed_exec::open_verified(p, cfg.exec_owner, Lease::IfGranted)', 'sealed_exec::open_verified(p, None, Lease::IfGranted)',
     'axon-fabric', '--test psv_dispatch', 'a_pinned_program_another_uid_owns_is_refused_at_every_consumer_of_the_owner'),
    ('M2970',
     "OWNER ARGUMENT (eq7): the root helper opens the launcher's interpreter requiring the operator's uid", 'crates/axon-fabric/src/privileged_launcher.rs',
     'sealed_exec::open_verified(&pin(&c.interpreter), owner, a.lease())?;', 'sealed_exec::open_verified(&pin(&c.interpreter), None, a.lease())?;',
     'axon-fabric', '--test privileged_launcher', 'an_interpreter_another_uid_owns_launches_nothing_as_root'),
    ('M2971',
     'OPEN FLAGS (eq7): the ownership walk opens every directory component without following a symlink', 'crates/axon-fabric/src/privileged_launcher.rs',
     'const DIR_FLAGS: libc::c_int = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW;', 'const DIR_FLAGS: libc::c_int = libc::O_RDONLY | libc::O_DIRECTORY;',
     'axon-fabric', '--lib', 'privileged_launcher::tests::a_walk_through_a_symlinked_directory_is_never_followed'),
    ('M2972',
     'PROFILE (eq7): the protected Linux microVM profile reports hardware isolation', 'crates/axon-fabric/src/backend.rs',
     '    hardware_isolation: true,\n    isolation: Isolation::LinuxMicroVmProtected,', '    hardware_isolation: false,\n    isolation: Isolation::LinuxMicroVmProtected,',
     'axon-fabric', '--lib', 'backend::tests::profiles_are_truthful'),
    ('M2973',
     "BOUND (eq7): the custodian's outstanding-nonce bound is the decision value 1024", 'crates/axon-fabric/src/custodian.rs',
     'pub const MAX_OUTSTANDING: usize = 1024;', 'pub const MAX_OUTSTANDING: usize = 1_000_000;',
     'axon-fabric', '--lib', 'custodian::tests::the_custodian_bounds_the_nonces_it_holds_outstanding'),
    ('M2974',
     "READ BOUND (eq7): read_regular's bound is 256 MiB exactly", 'crates/axon-fabric/src/backend.rs',
     '    const MAX: u64 = 256 << 20;\n    let mut o = std::fs::OpenOptions::new();', '    const MAX: u64 = 257 << 20;\n    let mut o = std::fs::OpenOptions::new();',
     'axon-fabric', '--lib', 'backend::tests::read_regular_refuses_a_file_one_byte_over_256_mib_and_reads_one_at_it'),
    ('M2975',
     'ROOT LAUNCH ARGV (eq7): Fabric rounds the wall time UP to whole seconds for the direct launcher', 'crates/axon-fabric/src/backend.rs',
     '    let timeout_s = req.limits.wall_time_ms.div_ceil(1000).max(1);\n    let o = |p: &Path|', '    let timeout_s = req.limits.wall_time_ms.div_ceil(1001).max(1);\n    let o = |p: &Path|',
     'axon-fabric', '--test psv_dispatch', 'fabric_runs_its_direct_launcher_and_its_observer_with_exactly_their_flags_and_path'),
    ('M2976',
     "OWNER ARGUMENT (eq7): the root helper opens the profile manifest requiring the operator's uid", 'crates/axon-fabric/src/privileged_launcher.rs',
     '            sha256: c.profile_manifest.sha256.clone(),\n        },\n        owner,\n        a.lease(),', '            sha256: c.profile_manifest.sha256.clone(),\n        },\n        None,\n        a.lease(),',
     'axon-fabric', '--test privileged_launcher', 'a_profile_manifest_another_uid_owns_launches_nothing'),
    ('M2977',
     'OWNER FIELD (eq7): the loaded host opens the privileged helper requiring the executable owner', 'crates/axon-fabric/src/protected_host.rs',
     '            owner: exec_owner,\n            test_config: helper_test_config,', '            owner: exec_owner.wrapping_add(1),\n            test_config: helper_test_config,',
     'axon-fabric', '--test protected_host', 'a_loaded_host_requires_the_executable_owner_of_every_program_it_pins'),
]


if __name__ == "__main__":
    main()
