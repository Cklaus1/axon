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
     "    if replayed {\n        return Err(REPLAYED);",
     "    if false && replayed {\n        return Err(REPLAYED);",
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
    ('M42', 'signing: only if the workload could not reach the key',
     'crates/axon-fabric/src/signing.rs',
     'Some(r) if r.backend == LINUX_MICROVM_PROTECTED.id || r.effect_ceiling.is_empty() => Ok(()),',
     'Some(_) => Ok(()),',
     'axon-fabric', '--lib', 'signing::tests::an_effectless_local_run_signs_and_an_effectful_one_does_not'),
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
    ('M58', 'interpreter: loop control does not cross a function boundary',
     'crates/axon-core/src/interp.rs',
     '            Err(Flow::Break) | Err(Flow::Continue) => {\n                return panic(format!("`break`/`continue` outside a loop in `{}`", f.name))\n            }\n',
     '',
     'axon-core', '--no-default-features --lib', 'interp::tests::an_escaped_break_or_continue_does_not_pass_a_test'),
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
     '    if let Some(role) = crate::admission::other_loop_role(&config, &b.issuer_ref) {',
     '    if let Some(role) = None::<&str> {',
     'axon-loop', '--test pointer', 'a_baseline_issuer_holds_no_other_loop_role'),
    ('M103', 'derive: a counted verdict is still pinned (profile, revision, suite, acceptance)',
     'crates/axon-loop/src/admission.rs',
     'crate::intake::check_pins(&config, &v.issuer_ref, &t.task_id, &req, &rc).map_err(',
     'Ok::<(), LoopError>(()).map_err(',
     'axon-loop', '--test protected_class', 'a_rollback_rechecks_profile_qualification'),
    ('M104', "derive: a protected context's observer and key are still current",
     'crates/axon-loop/src/admission.rs',
     '                ) && !t.context_signed_by.as_ref().is_some_and(|c| {',
     '                ) && false && !t.context_signed_by.as_ref().is_some_and(|c| {',
     'axon-loop', '--test protected_class', 'a_protected_activation_rests_only_on_current_authority'),
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
RETIRED = {"M58"}
BINDING_IDS = {f"M{n}" for n in range(101, 116)}


def in_scope(mid, scope):
    if mid in RETIRED:
        return False
    if scope == "g01":
        return mid not in PCI_IDS and mid not in BINDING_IDS
    if scope == "pci":
        return mid in PCI_IDS
    if scope == "binding":
        return mid in BINDING_IDS
    return True


def sh(cmd):
    return subprocess.run(cmd, cwd=ROOT, shell=True, capture_output=True, text=True)


def cargo_test(package, target, test):
    cmd = (
        "source scripts/lib_bounded_run.sh && "
        f"bounded_run 16G 1800 cargo test -q -p {package} {target} -- --exact {test}"
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


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--scope=")]
    scope = next((a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--scope=")), "g01")
    if len(args) != 1 or scope not in ("g01", "pci", "binding", "all"):
        sys.exit("usage: v022_g01_mutations.py [--scope=g01|pci|binding|all] OUT.json")
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
            ["bash", "-c", "source scripts/lib_bounded_run.sh && bounded_run 16G 1800 "
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
    for (mid, guard, rel, old, new, pkg, target, test) in MUTATIONS:
        if not in_scope(mid, scope):
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
           "all_killed": ok, "mutations": results}
    with open(sys.argv[1], "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    print(f"{sum(r['result'] == 'killed' for r in results)}/{len(results)} killed; baselines "
          f"{'all pass' if all(v == 'passed' for v in baselines.values()) else 'NOT all pass'}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
