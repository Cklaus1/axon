# v0.22 PSV negative matrix — every row, where it is refused, and its tests

Status: **development evidence for `PSV_PROTOCOL_PROVEN`. It earns no readiness credit.** The
three protected components stay NOT_RUN until the operator-controlled environment certifies them.

Rows are the gap map's A1–A19 plus the protocol spec's A20–A21. Every cited test asserts its row's
SPECIFIC refusal (the reason, not only a non-pass), and runs in the fabric/psv/loop suites or in
`gate.sh`. `scripts/psv_matrix_check.py` fails when a row is missing, cites no test, or cites a
test that does not exist (`file::function`), so this table cannot silently rot.

Where a row is refused at more than one point, every point is listed. Those points are defence in
depth, and each has its own test.

| Row | Case | Refused at | Reason asserted | Tests |
|---|---|---|---|---|
| A1 | wrong candidate tree | guest runner (before execution); Fabric reports it; real guest | `candidate tree is …` | `crates/axon-psv/tests/protocol.rs::inputs_must_match_the_manifest_each_one_named`, `crates/axon-psv/tests/runner.rs::nothing_executes_unless_every_input_is_the_named_one`, `crates/axon-fabric/tests/psv_dispatch.rs::a_candidate_changed_under_the_guest_is_refused_there`, `scripts/psv_guest_boot_test.sh` |
| A2 | wrong suite | guest runner; Fabric; intake pins | `suite tree is …`; `rubric: suite … is not a version pinned` | `crates/axon-psv/tests/protocol.rs::inputs_must_match_the_manifest_each_one_named`, `crates/axon-fabric/tests/psv_dispatch.rs::a_suite_changed_under_the_guest_is_refused_there`, `crates/axon-loop/tests/intake.rs::a_verdict_from_another_pinned_version_of_the_suite_does_not_decide_the_task` |
| A3 | wrong test id | guest runner (identifier); Fabric (no verdict); intake (task acceptance) | `not an identifier`; `produced no verdict`; `acceptance: …` | `crates/axon-psv/tests/runner.rs::nothing_executes_unless_every_input_is_the_named_one`, `crates/axon-fabric/tests/psv_dispatch.rs::a_test_the_suite_does_not_define_has_no_verdict`, `crates/axon-loop/tests/intake.rs::a_verdict_on_one_tasks_check_cannot_decide_another_task` |
| A4 | wrong trial | completion key (bound); intake identity join | key differs; `another trial's evidence` | `crates/axon-psv/tests/protocol.rs::the_completion_key_moves_with_every_bound_identity_and_the_secret`, `crates/axon-loop/tests/intake.rs::each_verification_rule_is_load_bearing_on_its_own` |
| A5 | wrong attempt | completion key (bound); intake identity join | key differs; `operation` | `crates/axon-psv/tests/protocol.rs::the_completion_key_moves_with_every_bound_identity_and_the_secret`, `crates/axon-loop/tests/intake.rs::verification_that_does_not_join_is_refused_with_the_store_unchanged` |
| A6 | wrong guest image | observation join (before launch); loop protected join; launcher pin | `observation guest.… is …`; `guest interpreter … is not the one the request pinned` | `crates/axon-fabric/tests/psv_dispatch.rs::every_defective_observation_refuses_the_launch`, `crates/axon-loop/tests/intake.rs::a_protected_claim_without_every_join_is_refused`, `crates/axon-fabric/tests/submit.rs::a_changed_manifest_makes_the_linux_profile_ineligible_with_no_launch` |
| A7 | wrong kernel / runtime | observation join (kernel); loop join (interpreter) | `observation guest.kernel_sha256 is …`; `is not the one the request pinned` | `crates/axon-fabric/tests/psv_dispatch.rs::every_defective_observation_refuses_the_launch`, `crates/axon-loop/tests/intake.rs::a_protected_claim_without_every_join_is_refused` |
| A8 | stale observer evidence | observation verify; custodian nonce store | `old (max …s)`; `for epoch …` | `crates/axon-fabric/tests/psv_dispatch.rs::every_defective_observation_refuses_the_launch`, `crates/axon-fabric/tests/psv_dispatch.rs::a_nonce_authorizes_exactly_one_launch` |
| A9 | observer from the wrong authority domain | `RULE:authority-domain` (signed message + domain field) | `is for authority …` | `crates/axon-fabric/tests/psv_dispatch.rs::every_defective_observation_refuses_the_launch`, `crates/axon-fabric/tests/verify_evidence.rs::each_authority_verifies_only_its_own_domain_message`, `crates/axon-fabric/tests/readiness.rs::a_signature_for_another_authority_is_not_a_qualification_signature` |
| A10 | wrong verifier | intake attestation authentication | `not authenticated` | `crates/axon-loop/tests/intake.rs::verification_evidence_is_authenticated_not_named` |
| A11 | replayed completion proof | Fabric derives K from its OWN manifest and secret | `without completion evidence` | `crates/axon-psv/tests/runner.rs::a_completion_proof_does_not_replay_across_attempts_or_trials`, `crates/axon-fabric/tests/psv_dispatch.rs::a_previous_attempts_genuine_pass_does_not_replay` |
| A12 | missing completion proof | runner (claim); Fabric (certified parser + K) | `Unknown`; `without completion evidence` | `crates/axon-psv/tests/runner.rs::a_forged_or_duplicated_result_line_is_not_a_pass`, `crates/axon-fabric/tests/psv_dispatch.rs::every_forgery_of_the_returned_evidence_is_unknown_for_its_own_reason` |
| A13 | development backend labelled protected | attestation rule (class per backend); loop protected join (class, backend) | `WRONG_CLASS`; `evidence class is development`; `not a protected profile` | `crates/axon-fabric/src/signing.rs::a_class_is_signed_only_where_its_backend_derives_it`, `crates/axon-loop/tests/protected_class.rs::only_protected_class_evidence_counts_in_a_protected_evaluation`, `crates/axon-loop/tests/intake.rs::a_protected_claim_without_every_join_is_refused`, `crates/axon-fabric/tests/psv_dispatch.rs::a_local_check_is_development_evidence` |
| A14 | guest verdict without a protected preflight | Fabric class (`guest-unobserved`); loop protected join | class `guest-unobserved`; `evidence class is guest-unobserved`; `names no preflight-observation-sha256` | `crates/axon-fabric/tests/psv_dispatch.rs::an_operator_suite_passes_through_the_guest_path_as_guest_unobserved`, `crates/axon-loop/tests/protected_class.rs::only_protected_class_evidence_counts_in_a_protected_evaluation`, `crates/axon-loop/tests/intake.rs::a_protected_claim_without_every_join_is_refused` |
| A15 | replayed observation | observation join (another manifest) + one-use nonce | `intended_launch_manifest_sha256`; `already used` | `crates/axon-fabric/tests/psv_dispatch.rs::an_earlier_observation_does_not_authorize_another_launch`, `crates/axon-fabric/tests/psv_dispatch.rs::a_nonce_authorizes_exactly_one_launch` |
| A16 | replaced launcher | O1 pin at load; eligibility; dispatch recheck | `not its pin`; `RULE:launcher-pinned` | `crates/axon-fabric/tests/protected_host.rs::a_replaced_launcher_is_refused_at_load_and_after_load`, `crates/axon-fabric/tests/submit.rs::a_launcher_replaced_after_eligibility_never_runs` |
| A17 | caller-supplied suite registry | O1: the operator registry only; pinned | `--check-registry is not accepted`; `suite_registry … not its pin` | `crates/axon-fabric/tests/protected_host.rs::only_the_pinned_operator_registry_defines_suites`, `crates/axon-fabric/tests/protected_host.rs::a_protected_host_loads_the_operators_registry_not_the_callers` |
| A18 | verifier (or observer) key planted in the loop store | O2: operator root only, for protected evidence | `operator's verifier root`; `operator's observer root` | `crates/axon-loop/tests/intake.rs::a_verifier_key_planted_in_the_store_never_authenticates_protected_evidence`, `crates/axon-loop/tests/protected_class.rs::an_observer_key_planted_in_the_store_is_not_authority`, `crates/axon-loop/tests/protected_class.rs::a_key_revoked_at_the_operator_root_no_longer_counts` |
| A19 | completion key reachable by the candidate | runner custody: K on stdin then EOF, cleared env, uid drop, secret 0400 | `STDIN-SAW:[]`; `Permission denied` | `crates/axon-psv/tests/runner.rs::neither_the_secret_nor_the_key_reaches_candidate_code_or_the_output`, `scripts/psv_guest_boot_test.sh` |
| A20 | Fabric signing key readable by an agent | trust preflight (real read attempts under each UID) | `read-key` refused for agents; readable by fabric only | `scripts/test_trust_root_preflight.sh`, `crates/axon-fabric/tests/protected_host.rs::the_host_signer_key_must_be_private_and_match_its_pin` |
| A21 | caller `--linux-*` / registry flags in protected mode | `submit` refuses, by flag name | `… is not accepted` | `crates/axon-fabric/tests/protected_host.rs::every_caller_protected_flag_is_refused_by_name`, `crates/axon-fabric/tests/trust_root.rs::a_caller_cannot_choose_the_protected_trust_root` |

## What these rows do NOT prove (PROTECTED_ONLY)

- A measuring observer. The dev observer copies the manifest's facts, which proves the protocol and
  never the measurement.
- The custodian's and the observer's UID separation on a real host.
- The installed verifier binary, `/etc/axon/protected-host.json`, and `/etc/axon/trust/*` on the
  protected host, with a protected-mode trust preflight passing against them.
- A B263 re-qualification of the PSV image (`profiles/linux-microvm/manifest.json` since
  `06ec49e3`), signed by the operator (S3-6).
- The uid-drop leg of A19 runs only as root. A non-root run prints that it was not exercised, and
  does not count as proof of it. `scripts/psv_guest_boot_test.sh` needs root, KVM and a built image,
  and otherwise SKIPs (exit 77), which is a non-result.
