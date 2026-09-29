#!/usr/bin/env python3
"""G01 mutation run: each guard LISTED BELOW, removed one at a time.

This is the set of G01 guards a mutation has been written for; it is not a proof
that no other guard exists. A guard added to the G01 paths belongs here.

For each mutation: the named test must PASS on the clean tree (baseline), and
must FAIL once that single guard is removed (killed). A mutation that no longer
applies (its text is absent or ambiguous) or that stops the crate compiling is
reported as such — never as killed — so a refactor cannot silently turn this
into a pass. Every file is restored and re-hashed after each mutation.

    python3 scripts/v022_g01_mutations.py OUT.json

Refuses to run on a tree with uncommitted changes under crates/: the result is
evidence about a COMMIT, and it names that commit. Exit 0 only when every
baseline passes and every mutation is killed.

Each cargo run is contained by scripts/lib_bounded_run.sh (memory ceiling and
deadline).
"""

import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# (id, guard, file, old, new, package, target, test)
# target: "--lib" or "--test <name>"; test: the exact test path.
MUTATIONS = [
    ("M01", "Fabric never signs a replay",
     "crates/axon-fabric/src/signing.rs",
     "        return Err(CANDIDATE_RUBRIC);\n    }\n    if replayed {\n        return Err(REPLAYED);",
     "        return Err(CANDIDATE_RUBRIC);\n    }\n    if false && replayed {\n        return Err(REPLAYED);",
     "axon-fabric", "--test attestation", "a_replay_is_never_signed_not_even_a_genuine_one"),
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
     "axon-fabric", "--test check_effects", "a_candidate_cannot_shadow_a_module_of_the_suite"),
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
     "axon-loop", "--test protected_class", "each_d3_leg_on_a_development_backend_counts_nothing"),
    ("M20", "D3: the verification leg is checked",
     "crates/axon-loop/src/evl.rs",
     '                "verification",\n                d.verification[1]["backend_profile_ref"].as_str(),',
     '                "verification",\n                None,',
     "axon-loop", "--test protected_class", "each_d3_leg_on_a_development_backend_counts_nothing"),
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
     'axon-loop', '--test intake', 'a_verdict_counts_only_for_what_the_operator_pinned'),
    ('M28', 'intake: the recorded suite is pinned for this verifier',
     'crates/axon-loop/src/intake.rs',
     '        || !pin.check_suites.iter().any(|p| p == recorded)\n',
     '\n',
     'axon-loop', '--test intake', 'a_verdict_counts_only_for_what_the_operator_pinned'),
    ('M29', 'intake: the recorded suite is the one argv named',
     'crates/axon-loop/src/intake.rs',
     '    if !recorded.starts_with(&format!("check-suite:{suite_id}@"))\n        ||',
     '    if false\n        ||',
     'axon-loop', '--test intake', 'a_verdict_counts_only_for_what_the_operator_pinned'),
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
     '.map(|s| format!("check-suite:{}@{}#{}", s.id, s.version, target.file));',
     '.map(|s| format!("check-suite:{}@{}", s.id, s.version));',
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
     'axon-core', '--no-default-features --lib', 'resolver::tests::duplicate_let_refinement_or_impl_produces_e0002'),
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
     'axon-core', '--no-default-features --lib', 'resolver::tests::a_sealed_module_cannot_reach_the_operators_names'),
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
     'axon-core', '--no-default-features --lib', 'interp::tests::runtime_sealing_holds_without_the_static_check'),
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
     'axon-loop', '--test evl_admission', 'an_admitter_holds_no_other_loop_role'),
    ('M115', 'config: one key per role, checked on read as well as write (ADR-002)',
     'crates/axon-loop/src/store.rs',
     ')?;\n                c.check_separation().map_err(crate::error::refused)?;\n                Ok(c)',
     ')?;\n                Ok(c)',
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
    ('M139', 'O1/A17: no caller registry on a protected host', 'crates/axon-fabric/src/bin/axon-fabric.rs', 'if host.is_some() && a.opt("--check-registry").is_some() {', 'if false && host.is_some() && a.opt("--check-registry").is_some() {', 'axon-fabric', '--test protected_host', 'only_the_pinned_operator_registry_defines_suites'),
    ('M140', "O1: the protected signer is the host config's", 'crates/axon-fabric/src/bin/axon-fabric.rs', 'Some(h) => Some(host_signer(h)),', 'Some(_) => signer(&registry_path),', 'axon-fabric', '--test protected_host', 'the_host_signer_key_must_be_private_and_match_its_pin'),
    ('M141', "O1: a protected host loads the operator's registry", 'crates/axon-fabric/src/bin/axon-fabric.rs', 'Some(h) => h.suite_registry.clone(),', 'Some(_) => PathBuf::from(a.req("--check-registry")),', 'axon-fabric', '--test protected_host', 'a_protected_host_loads_the_operators_registry_not_the_callers'),
    ('M142', 'O1: a pinned file is exactly its pinned bytes', 'crates/axon-fabric/src/protected_host.rs', 'if got != pin {', 'if false && got != pin {', 'axon-fabric', '--test protected_host', 'a_replaced_launcher_is_refused_at_load_and_after_load'),
    ('M143', 'O1: every O1 path is operator-owned', 'crates/axon-fabric/src/protected_host.rs', 'Some(base) => check_owned_chain(base, p, entries),', 'Some(_base) => Ok(()),', 'axon-fabric', '--test protected_host', 'every_o1_path_must_be_operator_owned'),
    ('M144', 'O1: the host config schema is exact', 'crates/axon-fabric/src/protected_host.rs', 'if keys != KEYS {', 'if false && keys != KEYS {', 'axon-fabric', '--test protected_host', 'the_config_schema_is_exact_and_every_path_absolute'),
    ('M145', 'O1: every protected path is absolute', 'crates/axon-fabric/src/protected_host.rs', 'if !p.is_absolute() {', 'if false && !p.is_absolute() {', 'axon-fabric', '--test protected_host', 'the_config_schema_is_exact_and_every_path_absolute'),
    ('M146', "O1: the signing key's directory is operator-owned", 'crates/axon-fabric/src/protected_host.rs', 'if let Some(dir) = signer.key_path.parent() {', 'if let Some(dir) = None::<&Path> {', 'axon-fabric', '--test protected_host', 'every_o1_path_must_be_operator_owned'),
    ('M147', 'O1: the artifacts directory is operator-owned', 'crates/axon-fabric/src/protected_host.rs', 'owned(&artifacts_dir, true).map_err(bad)?;', '', 'axon-fabric', '--test protected_host', 'every_o1_path_must_be_operator_owned'),
    ('M148', 'O1: the B263 record is operator-owned', 'crates/axon-fabric/src/protected_host.rs', 'owned(&record, false).map_err(bad)?;', '', 'axon-fabric', '--test protected_host', 'every_o1_path_must_be_operator_owned'),
    ('M149', 'readiness: a certification binds the verifier that made it', 'crates/axon-fabric/src/readiness.rs', 'if doc["readiness_verifier_sha256"] != me["sha256"] {', 'if false && doc["readiness_verifier_sha256"] != me["sha256"] {', 'axon-fabric', '--test readiness', 'another_verifier_binary_does_not_inherit_the_certification'),
    ('M150', 'readiness: the trust preflight must be protected-mode', 'crates/axon-fabric/src/readiness.rs', '|| pf["mode"] != "protected"', '|| false', 'axon-fabric', '--test readiness', 'a_dev_mode_or_uncertified_trust_preflight_is_refused'),
    ('M151', 'readiness: the trust preflight is a certified evidence file', 'crates/axon-fabric/src/readiness.rs', 'let pf = preflight.ok_or(format!(', 'let pf = preflight.or(Some(repo.join("governance/proofs/v022-protected/trust-preflight.json"))).ok_or(format!(', 'axon-fabric', '--test readiness', 'a_dev_mode_or_uncertified_trust_preflight_is_refused'),
    ('M152', "RULE:authority-domain: the signature's domain field", 'crates/axon-loop-contracts/src/operator_trust.rs', 'if sv["domain"] != authority.dir_name() {', 'if false && sv["domain"] != authority.dir_name() {', 'axon-fabric', '--test verify_evidence', 'each_authority_verifies_only_its_own_domain_message'),
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
    ('M180', 'M2/A1: a guest refusal is reported as such', 'crates/axon-fabric/src/psv.rs', 'if v.status == GuestStatus::Refused {', 'if false && v.status == GuestStatus::Refused {', 'axon-fabric', '--test psv_dispatch', 'a_candidate_changed_under_the_guest_is_refused_there'),
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
    ('M191', 'M3: the observer program is pinned', 'crates/axon-fabric/src/observer.rs', 'if got != cfg.command_sha256 {', 'if false && got != cfg.command_sha256 {', 'axon-fabric', '--test psv_dispatch', 'an_unpinned_observer_is_refused'),
    ('M192', 'M3/A9: the OBSERVER domain', 'crates/axon-fabric/src/observer.rs', '        &cfg.trust.dir,\n        TrustAuthority::Observer,', '        &cfg.trust.dir,\n        TrustAuthority::Qualification,', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M193', 'M3: the claimed observer is the signer', 'crates/axon-fabric/src/observer.rs', 'if o.observer_key_id != signer {', 'if false && o.observer_key_id != signer {', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M194', "M3/A8: the observation's epoch", 'crates/axon-fabric/src/observer.rs', 'if o.epoch != epoch {', 'if false && o.epoch != epoch {', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M195', "M3/A8: the observation's age", 'crates/axon-fabric/src/observer.rs', 'if age < 0 || age as u64 > cfg.max_age_s {', 'if false && (age < 0 || age as u64 > cfg.max_age_s) {', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M196', 'M3/A15: a verified observation spends its nonce', 'crates/axon-fabric/src/observer.rs', '    cfg.nonces\n        .consume(&o.nonce, epoch, &cfg.clock, cfg.max_age_s)?;', '', 'axon-fabric', '--test psv_dispatch', 'a_verified_observation_makes_the_guest_verdict_protected'),
    ('M197', 'M3: a nonce is of its epoch', 'crates/axon-fabric/src/observer.rs', 'if rec["epoch"].as_u64() != Some(epoch) {', 'if false && rec["epoch"].as_u64() != Some(epoch) {', 'axon-fabric', '--test psv_dispatch', 'a_nonce_authorizes_exactly_one_launch'),
    ('M198', 'M3/A8: a nonce has a maximum age', 'crates/axon-fabric/src/observer.rs', 'if age < 0 || age as u64 > max_age_s {', 'if false && (age < 0 || age as u64 > max_age_s) {', 'axon-fabric', '--test psv_dispatch', 'a_nonce_authorizes_exactly_one_launch'),
    ('M199', 'M3: a nonce is one this custodian issues', 'crates/axon-fabric/src/observer.rs', 'if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {', 'if false {', 'axon-fabric', '--test psv_dispatch', 'a_nonce_authorizes_exactly_one_launch'),
    ('M200', 'M3/A15: the observation is of THIS manifest', 'crates/axon-psv/src/lib.rs', '                manifest_digest,\n            ),', '                &self.intended_launch_manifest_sha256,\n            ),', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M201', "M3: the observation's nonce is the manifest's", 'crates/axon-psv/src/lib.rs', '("nonce", &self.nonce, &m.observation_nonce),', '("nonce", &self.nonce, &self.nonce),', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M202', "M3/A7: the observed kernel is the launch's", 'crates/axon-psv/src/lib.rs', '                &self.guest.kernel_sha256,\n                &m.guest.kernel_sha256,', '                &self.guest.kernel_sha256,\n                &self.guest.kernel_sha256,', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M203', 'M3: a verified observation makes the verdict protected', 'crates/axon-fabric/src/submit.rs', 'let hv = crate::psv::derive(&launch, &res.out_dir, observation.as_ref());', 'let hv = crate::psv::derive(&launch, &res.out_dir, None);', 'axon-fabric', '--test psv_dispatch', 'a_verified_observation_makes_the_guest_verdict_protected'),
    ('M204', 'M3: a refused observation launches nothing', 'crates/axon-fabric/src/submit.rs', '                Some(o) => crate::observer::observe(\n                    o,\n                    &launch.manifest,\n                    &launch.digest,\n                    &launch.job_dir.join("launch-manifest.json"),\n                    epoch,\n                    &launch.job_dir.with_file_name("observation"),\n                )\n                .map(|v| (launch, Some(v)))\n                .map_err(|e| format!("preflight observation refused: {e}")),', '                Some(o) => {\n                    let r = crate::observer::observe(\n                        o,\n                        &launch.manifest,\n                        &launch.digest,\n                        &launch.job_dir.join("launch-manifest.json"),\n                        epoch,\n                        &launch.job_dir.with_file_name("observation"),\n                    );\n                    Ok((launch, r.ok()))\n                }', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M205', 'O2/A18: protected evidence needs an operator-rooted verifier key', 'crates/axon-loop/src/intake.rs', '    if axon_loop_contracts::protected_evidence::claims_protected(&rc) {', '    if false && axon_loop_contracts::protected_evidence::claims_protected(&rc) {', 'axon-loop', '--test intake', 'a_verifier_key_planted_in_the_store_never_authenticates_protected_evidence'),
    ('M206', 'O2: rooted() requires the key in the operator root', 'crates/axon-loop-contracts/src/operator_trust.rs', 'if keys_in(&dir)?.contains(&want) {', 'if true {', 'axon-loop', '--test intake', 'a_verifier_key_planted_in_the_store_never_authenticates_protected_evidence'),
    ('M207', 'O2: rooted_key consults the operator root', 'crates/axon-loop/src/store.rs', '        axon_loop_contracts::operator_trust::rooted(a, k)?;\n', '', 'axon-loop', '--test protected_class', 'an_observer_key_planted_in_the_store_is_not_authority'),
    ('M208', "O2/A18: EVL's protected context uses an operator-rooted observer key", 'crates/axon-loop/src/evl.rs', 'crate::store::Config::rooted_key(\n        &config.observer_keys,\n        who,\n        axon_loop_contracts::operator_trust::TrustAuthority::Observer,\n    )', 'config.observer_keys.get(who).ok_or_else(String::new)', 'axon-loop', '--test protected_class', 'an_observer_key_planted_in_the_store_is_not_authority'),
    ('M209', 'a counted verdict attributed to a verifier must name one the operator verifier ROOT holds (LOAD-BEARING on the misattribution route; C9 re-audit reinstated it)', 'crates/axon-loop/src/admission.rs', 'crate::store::Config::rooted_key(\n                    &config.verifier_keys,\n                    &v.issuer_ref,\n                    axon_loop_contracts::operator_trust::TrustAuthority::Verifier,\n                )', 'config.verifier_keys.get(&v.issuer_ref).ok_or_else(String::new)', 'axon-loop', '--test protected_attribution', 'a_verdict_attributed_to_a_verifier_the_operator_root_never_held_does_not_count'),
    ('M210', 'a context attributed to an observer must name one the operator observer ROOT holds (LOAD-BEARING on the misattribution route; C9 re-audit reinstated it)', 'crates/axon-loop/src/admission.rs', 'crate::store::Config::rooted_key(\n                            &config.observer_keys,\n                            &c.issuer_ref,\n                            axon_loop_contracts::operator_trust::TrustAuthority::Observer,\n                        )', 'config.observer_keys.get(&c.issuer_ref).ok_or_else(String::new)', 'axon-loop', '--test protected_attribution', 'a_context_attributed_to_an_observer_the_operator_root_never_held_does_not_count'),
    ('M211', 'M4/A13/A14: a protected evaluation counts only protected-class evidence', 'crates/axon-loop/src/evl.rs', '        // M4: a verdict counts in a protected evaluation only as PROTECTED\n', '        // M4 (mutated)\n        #[cfg(any())]\n', 'axon-loop', '--test protected_class', 'only_protected_class_evidence_counts_in_a_protected_evaluation'),
    ('M212', 'M4: the one class must be protected', 'crates/axon-loop-contracts/src/protected_evidence.rs', '["protected"] => {}', '[_] => {}', 'axon-loop', '--test protected_class', 'only_protected_class_evidence_counts_in_a_protected_evaluation'),
    ('M213', 'M4/A13: a protected claim needs a protected backend', 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if !crate::PROTECTED_PROFILES.contains(&rc.backend_profile_ref.as_str()) {', 'if false {', 'axon-loop', '--test intake', 'a_protected_claim_without_every_join_is_refused'),
    ('M214', 'M4/A14: a protected claim names its observation', 'crates/axon-loop-contracts/src/protected_evidence.rs', '    "preflight-observation-sha256:",\n', '    "launch-manifest-sha256:",\n', 'axon-loop', '--test intake', 'a_protected_claim_without_every_join_is_refused'),
    ('M215', "M4/A6: the guest interpreter is the request's pinned executable", 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if req.executable_digest.as_str() != want {', 'if false && req.executable_digest.as_str() != want {', 'axon-loop', '--test intake', 'a_protected_claim_without_every_join_is_refused'),
    ('M216', 'M4: each join exactly once', 'crates/axon-loop-contracts/src/protected_evidence.rs', '            [d] if d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()) => {', '            [d, ..] if d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()) => {', 'axon-loop', '--test intake', 'a_protected_claim_without_every_join_is_refused'),
    ('M217', 'M4: each join is a sha256', 'crates/axon-loop-contracts/src/protected_evidence.rs', '[d] if d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()) =>', '[d] =>', 'axon-loop', '--test intake', 'a_protected_claim_without_every_join_is_refused'),
    ('M218', 'M4: intake holds a protected claim to every join', 'crates/axon-loop/src/intake.rs', '        axon_loop_contracts::protected_evidence::check_bundle(\n            &req,\n            &rc,\n            bundle,\n            ep.authority_epoch.get(),\n        )\n        .map_err(|e| {', '        Ok::<&str, String>(bundle).map(|_| ()).map_err(|e: String| {', 'axon-loop', '--test intake', 'a_protected_claim_without_every_join_is_refused'),
    ('M219', 'B1: a sealed candidate @[test] is never collected', 'crates/axon-core/src/main.rs', 'if !sealed.is_empty() && axon_core::resolver::span_in_sealed(f.span, &sealed) {', 'if false && axon_core::resolver::span_in_sealed(f.span, &sealed) {', 'axon-core', '--no-default-features --test psv_test_selection', 'a_sealed_candidates_own_test_is_never_collected'),
    ('M220', 'B1: the runner selects exactly the registered test', 'crates/axon-psv/src/runner.rs', '        .arg("--exact")\n', '', 'axon-psv', '--test runner', 'a_suite_sibling_does_not_run_beside_the_registered_test'),
    ('M221', 'B3: launch inputs come from the store, never the caller-owned run dir', 'crates/axon-fabric/src/submit.rs', '            let inputs = crate::psv::private_inputs(\n                lx,\n', '            let inputs = match &target.bound { Bound::Version { dir, .. } => Ok::<PathBuf, String>(dir.0.clone()), _ => unreachable!() }; let _ = (\n                lx,\n', 'axon-fabric', '--test psv_dispatch', 'a_run_dir_swapped_under_the_callers_state_changes_nothing'),
    ('M222', 'B3: a manifest candidate tree digest is its WorkspaceVersion', 'crates/axon-psv/src/lib.rs', 'if m.candidate.tree_digest != m.candidate.workspace_version {', 'if false && m.candidate.tree_digest != m.candidate.workspace_version {', 'axon-psv', '--test protocol', 'a_tree_digest_must_be_the_version_it_names'),
    ('M223', 'B3: a manifest suite tree digest is its version', 'crates/axon-psv/src/lib.rs', 'if m.suite.tree_digest != m.suite.version {', 'if false && m.suite.tree_digest != m.suite.version {', 'axon-psv', '--test protocol', 'a_tree_digest_must_be_the_version_it_names'),
    ('M224', 'guest input: nothing the digest omits', 'crates/axon-psv/src/lib.rs', 'if let Some(o) = omitted.first() {', 'if let Some(o) = omitted.first().filter(|_| false) {', 'axon-psv', '--test protocol', 'inputs_with_links_or_omitted_entries_are_refused'),
    ('M225', 'guest input: no symlink', 'crates/axon-psv/src/lib.rs', '.find(|e| e.kind == axon_workspace_recipe::EntryKind::Symlink)', '.find(|e| false && e.kind == axon_workspace_recipe::EntryKind::Symlink)', 'axon-psv', '--test protocol', 'inputs_with_links_or_omitted_entries_are_refused'),
    ('M226', 'a PSV verdict names its one matched check', 'crates/axon-fabric/src/submit.rs', '        r.matched_checks = Some(1);\n', '', 'axon-fabric', '--test psv_dispatch', 'an_operator_suite_passes_through_the_guest_path_as_guest_unobserved'),
    ('M227', 'observation joins the manifest verifier', 'crates/axon-psv/src/lib.rs', '("verifier_sha256", &self.verifier_sha256, &m.verifier_sha256),', '("verifier_sha256", &m.verifier_sha256, &m.verifier_sha256),', 'axon-fabric', '--test psv_dispatch', 'every_defective_observation_refuses_the_launch'),
    ('M228', 'the verify step inherits no caller environment', 'crates/axon-fabric/src/backend.rs', '            // (PATH, …) steers the pinned launcher (review wf_d725935a-7ed).\n            .env_clear()\n', '            // (PATH, …) steers the pinned launcher (review wf_d725935a-7ed).\n', 'axon-fabric', '--test psv_dispatch', 'the_verify_step_inherits_nothing_from_the_caller'),
    ('M229', 'the per-attempt secret is scrubbed when the launcher returns', 'crates/axon-fabric/src/backend.rs', '    if let Some(l) = psv {\n        l.scrub();\n    }\n', '    let _ = &psv;\n', 'axon-fabric', '--test psv_dispatch', 'the_verify_step_inherits_nothing_from_the_caller'),
    ('M230', 'B2: intake joins a protected claim over the bundle', 'crates/axon-loop/src/intake.rs', 'axon_loop_contracts::protected_evidence::check_bundle(\n            &req,\n            &rc,\n            bundle,\n            ep.authority_epoch.get(),\n        )\n        .map_err(|e| {', 'axon_loop_contracts::protected_evidence::check(&req, &rc).map(|_| bundle).map(|_| ()).map_err(|e| {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M231', 'B2: a bundle for a non-protected receipt is refused', 'crates/axon-loop/src/intake.rs', '} else if psv_evidence.is_some() {', '} else if false && psv_evidence.is_some() {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M232', 'B2: the bundle manifest is the receipt manifest', 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if m_sha != want("launch-manifest-sha256:")? {', 'if false && m_sha != want("launch-manifest-sha256:")? {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M233', 'B2: the bundle observation is the receipt observation', 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if o_sha != want("preflight-observation-sha256:")? {', 'if false && o_sha != want("preflight-observation-sha256:")? {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M234', 'B2: the observation names its signer', 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if o.observer_key_id != signer {', 'if false && o.observer_key_id != signer {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M235', 'B2: the observation joins the manifest', 'crates/axon-loop-contracts/src/protected_evidence.rs', '    o.joins(&m, &m_sha)?;\n', '', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M236', 'B2: the manifest joins the request and receipt', 'crates/axon-loop-contracts/src/protected_evidence.rs', '        if manifest != other {', '        if false && manifest != other {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
    ('M237', 'B2: the manifest suite is the receipt check-suite', 'crates/axon-loop-contracts/src/protected_evidence.rs', 'if want("check-suite:")? != suite_ref {', 'if false && want("check-suite:")? != suite_ref {', 'axon-loop', '--test intake', 'each_protected_join_is_verified_over_the_documents'),
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
    ('M254', 'PSV-7: a protected decision re-verifies every counted verdict (LOAD-BEARING — reverify_protected is the sole enforcement site; certifying review wf_bff9835f-4a0 reclassified from equivalent to active)', 'crates/axon-loop/src/admission.rs', 'reverify_protected(tx, &config, eval, arm, t, v, &rc)?;', 'let _ = (eval, &rc);', 'axon-loop', '--test protected_class', 'a_protected_admission_re_verifies_the_execution_leg_from_its_documents'),
    ('M255', 'admission: every counted receipt must CLAIM protected evidence (LOAD-BEARING: the only stop for a store writer substituting a genuinely signed guest-unobserved or development verdict; C8 review wf_ae3a5a74-41e reinstated it from equivalent)', 'crates/axon-loop/src/admission.rs', '    if !axon_loop_contracts::protected_evidence::claims_protected(rc) {', '    if false && !axon_loop_contracts::protected_evidence::claims_protected(rc) {', 'axon-loop', '--test protected_class', 'a_genuinely_signed_unobserved_verdict_cannot_count_in_a_protected_record'),
    ('M256', "PSV-7 (r1): a protected execution leg needs Fabric's execution attestation", 'crates/axon-loop/src/evl.rs', 'if let Err(e) = verify_execution(&d.acf_att, areq, rcpt, config) {', 'if let Err(e) = Ok::<(), String>(()).map(|_| (areq, rcpt)) {', 'axon-loop', '--test protected_class', 'a_relabelled_execution_leg_counts_nothing_in_a_protected_evaluation'),
    ('M257', 'PSV-7 (r1): the execution attestation key is operator-rooted', 'crates/axon-loop/src/evl.rs', '    let key = crate::store::Config::rooted_key(\n        &config.verifier_keys,\n        &issuer,\n        axon_loop_contracts::operator_trust::TrustAuthority::Verifier,\n    )?;', '    let key = config\n        .verifier_keys\n        .get(&issuer)\n        .ok_or_else(|| "no key".to_string())?;', 'axon-loop', '--test protected_class', 'a_relabelled_execution_leg_counts_nothing_in_a_protected_evaluation'),
    ('M258', 'PSV-7 (r1): admission re-verifies the execution leg', 'crates/axon-loop/src/admission.rs', 'crate::evl::verify_execution(&att, &areq, &arc, config).map_err(fail)?;', 'let _ = (&att, &areq, &arc);', 'axon-loop', '--test protected_class', 'a_protected_admission_re_verifies_the_execution_leg_from_its_documents'),
    ('M259', 'PSV-7 (r1): a protected admission cites an execution attestation', 'crates/axon-loop/src/admission.rs', '    let att_ref = v\n        .execution_attestation_ref\n        .as_ref()\n        .ok_or_else(|| fail("it cites no execution attestation".into()))?;', '    let Some(att_ref) = v.execution_attestation_ref.as_ref() else {\n        return Ok(());\n    };', 'axon-loop', '--test protected_class', 'a_protected_admission_re_verifies_the_execution_leg_from_its_documents'),
    ('M260', "PSV-1 (r2): a sealed module's nested use is judged as sealed", 'crates/axon-core/src/lib.rs', '                            in_sealed(&candidate),\n', '                            false,\n', 'axon-core', '--no-default-features --test psv_test_selection', 'a_sealed_module_never_supplies_an_operator_modules_name'),
    ('M261', 'PSV-7 (r2): protected counters are the re-verified trials, over the plan population', 'crates/axon-loop/src/admission.rs', '        check_arm_grounding(tx, frozen, eval)?;', '        let _ = (frozen, eval);', 'axon-loop', '--test protected_class', 'a_protected_decision_counts_only_its_re_verified_trials'),
    ('M262', "PSV-7 (r2): the evaluation class is the frozen plan's", 'crates/axon-loop/src/admission.rs', '    if eval.evaluation_class != frozen.evaluation_class {', '    if false && eval.evaluation_class != frozen.evaluation_class {', 'axon-loop', '--test protected_class', 'an_evaluation_class_other_than_the_frozen_plans_is_refused'),
    ('M263', 'PSV-7 (r2): a counted trial re-verifies against its own episode', 'crates/axon-loop/src/admission.rs', '    if ep.identity.trial_id != t.trial_id || ep.identity.task_id != t.task_id {', '    if false && ep.identity.trial_id != t.trial_id {', 'axon-loop', '--test protected_class', 'a_counted_trial_cannot_borrow_another_trials_verdict'),
    ('M264', 'PSV-7 (r2): a protected clearance re-verifies its stored monitor signature', 'crates/axon-loop/src/admission.rs', '                                    && clearance_verifies(tx, &config, report, signature_ref.as_ref()))', '                                    && signature_ref.is_some() | true)', 'axon-loop', '--test protected_class', 'a_forged_unsigned_clearance_clears_nothing'),
    ('M265', 'PSV-3 (r2): a protected host runs nothing outside the protected profile', 'crates/axon-fabric/src/submit.rs', '        if cfg.protected_host.is_some() && p.id != backend::LINUX_MICROVM_PROTECTED.id {', '        if false && p.id != backend::LINUX_MICROVM_PROTECTED.id {', 'axon-fabric', '--test psv_dispatch', 'a_protected_host_runs_nothing_outside_the_protected_profile'),
    ('M266', "FIELD-ORIGIN (r3): the launcher's Python never imports from the caller's cwd", 'scripts/fc_linux_profile.sh', '    R="$(python3 -I -c \'import json,sys;', '    R="$(python3 -c \'import json,sys;', 'axon-fabric', '--test launcher_isolation', 'the_launchers_python_never_imports_from_the_callers_cwd'),
    ('M267', "PSV-7 (r3): a counted trial's episode ran its arm's policy", 'crates/axon-loop/src/admission.rs', '    if ep.policy_ref != arm.policy_ref {', '    if false && ep.policy_ref != arm.policy_ref {', 'axon-loop', '--test protected_class', 'a_protected_record_must_agree_with_its_re_verified_documents'),
    ('M268', "PSV-7 (r3): a counted trial's outcome is its signed verdict", 'crates/axon-loop/src/admission.rs', '        (crate::evl::Outcome::VerifiedPass, RV::Passed)\n        | (crate::evl::Outcome::Fail, RV::Failed) => {}', '        (_, _) if true => {}', 'axon-loop', '--test protected_class', 'a_protected_record_must_agree_with_its_re_verified_documents'),
    ('M269', "PSV-7 (r3): a counted trial's context signature re-verifies", 'crates/axon-loop/src/admission.rs', '    )\n    .map_err(|e| fail(format!("context signature: {e}")))?;', '    )\n    .ok();', 'axon-loop', '--test protected_class', 'a_protected_record_must_agree_with_its_re_verified_documents'),
    ('M270', "PSV-1: a draw comes from the RUNNING frame's kernel (a sealed frame never draws the operator stream)", 'crates/axon-core/src/interp.rs', '        self.k().rng_next()', '        self.kernels[0].rng_next()', 'axon-core', '--no-default-features --test psv_test_selection', 'sealed_rng_activity_never_moves_the_operators_stream'),
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
    ('M284', 'readiness: git runs with the caller environment cleared', 'crates/axon-fabric/src/readiness.rs', 'c.env_clear()', 'c.env_remove("AXON_NO_SUCH_VAR")', 'axon-fabric', '--test readiness_git_env', 'the_callers_path_and_git_environment_do_not_steer_the_verdict'),
    ('M285', 'readiness: refs/replace/ replacements are refused', 'crates/axon-fabric/src/readiness.rs', 'if let Some(r) = replaced.lines().next() {', 'if let Some(r) = replaced.lines().next().filter(|_| false) {', 'axon-fabric', '--test readiness', 'a_replaced_head_commit_is_not_certified'),
    ('M286', 'readiness: an info/grafts file is refused', 'crates/axon-fabric/src/readiness.rs', 'if std::fs::symlink_metadata(&grafts).is_ok() {', 'if false && std::fs::symlink_metadata(&grafts).is_ok() {', 'axon-fabric', '--test readiness', 'grafted_ancestry_is_not_certified'),
    ('M287', 'readiness: a skip-worktree index entry is refused', 'crates/axon-fabric/src/readiness.rs', "if tag == b'S' || tag == b's' {", 'if false {', 'axon-fabric', '--test readiness', 'a_skip_worktree_entry_is_not_certified'),
    ('M288', 'readiness: an assume-unchanged index entry is refused', 'crates/axon-fabric/src/readiness.rs', 'if tag.is_ascii_lowercase() {', 'if false && tag.is_ascii_lowercase() {', 'axon-fabric', '--test readiness', 'an_assume_unchanged_entry_is_not_certified'),
    ('M289', 'readiness: an object must hash to its name', 'crates/axon-fabric/src/readiness.rs', 'if object_id(want, &body) != oid {', 'if false && object_id(want, &body) != oid {', 'axon-fabric', '--test readiness', 'a_forged_object_under_the_certified_name_is_not_certified'),
    ('M290', 'readiness: working-tree bytes are hashed against the certified tree', 'crates/axon-fabric/src/readiness.rs', 'outside.extend(worktree_differs(repo, &want));', 'outside.extend(worktree_differs(repo, &want).filter(|_| false));', 'axon-fabric', '--test readiness', 'any_change_to_source_scripts_or_manifests_invalidates_it'),
    ('M291', 'protected host: out_root sits under an operator-owned directory', 'crates/axon-fabric/src/protected_host.rs', 'parent_owned(&out_root)?;', 'parent_owned(&out_root).ok();', 'axon-fabric', '--test protected_host', 'the_out_root_and_nonce_store_sit_under_operator_owned_directories'),
    ('M292', 'protected host: observer nonce_store sits under an operator-owned directory', 'crates/axon-fabric/src/protected_host.rs', 'parent_owned(&nonces)?;', 'parent_owned(&nonces).ok();', 'axon-fabric', '--test protected_host', 'the_out_root_and_nonce_store_sit_under_operator_owned_directories'),
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
    # ── C9 round-1 fix wave, workstream INPUTS (PSV-2: extended attributes on a
    # guest input — what neither the tree digest nor the mode check can see).
    ('M350', 'PSV-2 (c9r1): the input ROOT carrying an extended attribute is refused', 'crates/axon-psv/src/lib.rs', '    no_xattr(root, ".")?;\n', '    let _ = no_xattr(root, ".");\n', 'axon-psv', '--test protocol', 'inputs_carrying_an_extended_attribute_are_refused'),
    ('M351', 'PSV-2 (c9r1): an input entry carrying an extended attribute is refused', 'crates/axon-psv/src/lib.rs', '            no_xattr(&d.path(), &path)?;\n', '            let _ = no_xattr(&d.path(), &path);\n', 'axon-psv', '--test protocol', 'inputs_carrying_an_extended_attribute_are_refused'),
    ('M352', 'PSV-2 (c9r1): EVERY xattr namespace is refused, not a list (a user.*-only check misses an ACL)', 'crates/axon-psv/src/lib.rs', '.find(|s| !s.is_empty())', '.find(|s| s.starts_with(b"user."))', 'axon-psv', '--test protocol', 'inputs_carrying_an_extended_attribute_are_refused'),
    ('M353', 'PSV-2 (c9r1): the real runner refuses an ACL-carrying input instead of running it to a keyed Failed', 'crates/axon-psv/src/lib.rs', '            no_xattr(&d.path(), &path)?;\n', '            let _ = no_xattr(&d.path(), &path);\n', 'axon-psv', '--test runner', 'an_input_carrying_an_acl_is_refused_not_run'),
    ('M354', "PSV-2 (c9r1): the launcher's input copy preserves no ACL/xattr", 'scripts/fc_linux_profile.sh', '    cp -R "$2/." "$st/" || return 1\n', '    cp -a "$2/." "$st/" || return 1\n', 'axon-fabric', '--test launcher_isolation', 'the_launchers_input_image_carries_no_extended_attribute'),
    ('M355', "PSV-2 (c9r1): the launcher's mkfs copies no xattr into an input image", 'scripts/fc_linux_profile.sh', '-E root_owner=0:0,no_copy_xattrs -d "$st"', '-E root_owner=0:0 -d "$st"', 'axon-fabric', '--test launcher_isolation', 'the_launchers_input_image_carries_no_extended_attribute'),
    ('M356', 'PSV-2 (c9r1): the guest mounts the candidate input noacl', 'profiles/linux-microvm/guest-init.sh', 'ro,nodev,nosuid,noexec,noacl /dev/vdc', 'ro,nodev,nosuid,noexec /dev/vdc', 'axon-guest-init', '--test b263_profile_wiring', 'guest_init_sh_mounts_every_psv_input_noacl'),
    ('M357', 'PSV-2 (c9r1): the guest mounts the suite input noacl', 'profiles/linux-microvm/guest-init.sh', 'ro,nodev,nosuid,noexec,noacl /dev/vdd', 'ro,nodev,nosuid,noexec /dev/vdd', 'axon-guest-init', '--test b263_profile_wiring', 'guest_init_sh_mounts_every_psv_input_noacl'),
    ('M358', 'PSV-2 (c9r1): the guest mounts the job input noacl', 'profiles/linux-microvm/guest-init.sh', 'ro,nodev,nosuid,noexec,noacl /dev/vde', 'ro,nodev,nosuid,noexec /dev/vde', 'axon-guest-init', '--test b263_profile_wiring', 'guest_init_sh_mounts_every_psv_input_noacl'),
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
#   M204: submit's observe seam — observer-refusal is killed by M192-M195
#         (observer.rs) and the epoch arm by M253.
#   M103: derive check_pins — killed by intake pins M26/M28/M47.
#   M104: derive context_signed_by — killed by M99 and round-3 M269.
#   M209: derive rooted verifier — killed by O2 M205/M206.
#   M210: derive rooted observer — killed by O2 M207/M208.
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
}
EQUIVALENT_DID = set(EQUIV_RECORD)
# M204's historical guard (the submit observe seam) was REFACTORED away by
# amendment 16; it no longer applies. Its property is enforced by M192-M195
# (observe returns Err on a defect) and M253 (epoch recheck), all live killers.
STALE_REFACTORED = {"M204": {"property": "a defective/replayed observation must not launch", "subsumed_by": ["M192", "M253"], "killer": "M253", "how": "the submit observe seam was restructured by amendment 16 (epoch re-read after the observer)"},
                    # C9 re-audit: M176 was listed as a LEGACY EQUIVALENT but its guard no longer
                    # exists (old string absent) -- it became the `Some(0)` of the Passed arm in
                    # runner.rs, which no row mutated. It is STALE; M293 now kills that arm.
                    "M176": {"property": "a pass needs exit 0", "subsumed_by": ["M293"], "killer": "M293", "how": "the exit-0 guard became the Some(0) of the Passed arm in runner.rs (C9 re-audit)"}}

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
PSV_IDS = {f"M{n}" for n in range(137, 319)}
# C9 round-1 fix wave, workstream INPUTS: M350-M359 are PSV rows.
PSV_IDS |= {f"M{n}" for n in range(350, 360)}


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


def kill_line(out, test):
    """The failing test's own panic: location and first message line."""
    lines = out.splitlines()
    for i, l in enumerate(lines):
        # The panic names the test by its full path (`mod::tests::name`) for a
        # lib test and by its bare name for an integration test.
        if "panicked at" in l and (f"'{test}'" in l or f"'{test.split('::')[-1]}'" in l):
            return " | ".join(x.strip() for x in lines[i:i + 2])[:400]
    return None


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
    doc = {"schema": "axon-v022-mutation-run/2", "gate": base["gate"], "scope": base["scope"],
           "commit": base["commit"], "toolchain": [d["toolchain"] for d in docs],
           "merged_from": [{"shard": d["shard"], "all_killed": d["all_killed"]} for d in docs],
           "all_killed": ok, "mutations": rows}
    with open(out, "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    print_evidence_model(rows, base["scope"],
                         extra=f"  (merged from {len(docs)} shards)"
                         + ("" if ok else "; a SHARD reported a failure — see its BAD lines"))
    sys.exit(0 if ok else 1)


def print_evidence_model(rows, scope, extra=""):
    """Operator model (2026-09-28): active-killed, retired equivalents,
    survivors and stale reported SEPARATELY; an equivalent mutant is never
    folded into the killed denominator."""
    active = len(rows)
    killed = sum(r["result"] == "killed" for r in rows)
    survivors = [r["id"] for r in rows if r["result"].startswith("survived")]
    stale = [r["id"] for r in rows if "not_applicable" in r["result"]]
    base_ok = all(r["baseline"] == "passed" for r in rows)
    print(f"Mutation registry: {len(MUTATIONS)} total")
    print(f"Active mutants: {killed}/{active} killed"
          f"{'' if base_ok else ' (baselines NOT all pass)'}{extra}")
    # Retired rows are reported by WHAT they are, and each class is checked
    # against the tree: an equivalent's guard must still exist (else it is not
    # an equivalent but a stale row), a stale row's guard must be gone (else it
    # is a live guard with no mutant). C9 re-audit: M176 sat under "legacy
    # equivalent" for a guard that no longer existed, hidden from the line
    # below, which counts ACTIVE rows only.
    by_id = {r[0]: r for r in MUTATIONS}
    def applies(mid):
        r = by_id[mid]
        try:
            return open(os.path.join(ROOT, r[2])).read().count(r[3]) == 1
        except OSError:
            return False
    eq_gone = sorted(m for m in EQUIVALENT_DID | LEGACY_EQUIV if not applies(m))
    stale_live = sorted(m for m in STALE_REFACTORED if applies(m))
    print(f"Retired EQUIVALENT (four-cell paired-disable, never counted killed): "
          f"{len(EQUIVALENT_DID)} {sorted(EQUIVALENT_DID)}"
          + (f"  <-- NOT EQUIVALENT, guard absent: {eq_gone}" if eq_gone else ""))
    print(f"Retired STALE (guard refactored away; property killed by named live rows): "
          f"{len(STALE_REFACTORED)} {sorted(STALE_REFACTORED)}"
          + (f"  <-- NOT STALE, guard still present: {stale_live}" if stale_live else ""))
    if LEGACY_EQUIV:
        print(f"Retired LEGACY (unaudited): {len(LEGACY_EQUIV)} {sorted(LEGACY_EQUIV)}")
    print(f"Unexpected survivors: {len(survivors)}{(' '+str(survivors)) if survivors else ''}")
    print(f"Active rows stale/unapplied: {len(stale)}{(' '+str(stale)) if stale else ''}")


def main():
    if sys.argv[1:2] == ["--merge"]:
        if len(sys.argv) < 5:
            sys.exit("usage: v022_g01_mutations.py --merge OUT.json SHARD.json SHARD.json…")
        merge(sys.argv[2], sys.argv[3:])
    args = [a for a in sys.argv[1:] if not a.startswith("--scope=") and not a.startswith("--shard=")]
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
        sys.exit("usage: v022_g01_mutations.py [--scope=g01|pci|binding|psv|all] [--shard=K/N] OUT.json")
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
                result = "killed" if outcome == "failed" else f"survived ({outcome})"
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
                         "kill_evidence": evidence})
        print(f"{'OK ' if good else 'BAD'} {mid} baseline={base} {result}  {guard}", flush=True)
    # The run must end on the interpreter it started with.
    if toolchain["axon_bin_sha256"] is not None and sha(axon_bin) != toolchain["axon_bin_sha256"]:
        print(f"BAD interpreter binary changed during the run ({axon_bin})", flush=True)
        ok = False
    doc = {"schema": "axon-v022-mutation-run/2", "gate": "G01" if scope == "g01" else scope,
           "scope": scope, "commit": commit,
           "toolchain": toolchain,
           "shard": None if shard is None else {"index": shard[0], "of": shard[1]},
           "all_killed": ok, "mutations": results}
    with open(sys.argv[1], "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    print_evidence_model(results, scope)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
