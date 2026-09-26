#!/usr/bin/env bash
# v022_stage5_gates.sh — run the EXACT tests that back each v0.22 Stage-5 gate
# recorded in governance/cortex_gate_execution_registry.json.
#
# WHY A NAMED RUNNER. A registry row may move a package gate off NOT_RUN only if
# it names a script that EXISTS and that an invoker (scripts/gate.sh) actually
# CALLS; cortex_honesty_invariant.py greps the invoker for it. `cargo test -p
# axon-fabric` elsewhere in gate.sh runs these tests too, but it names no gate,
# so it cannot vouch for one. Each gate below lists the test functions that
# check its property; the script fails if any of them is missing (a renamed test
# must not silently drop a gate's evidence) or if the filtered run executes
# fewer tests than listed.
#
# Exit 0 only if every listed test ran and passed.
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
# gate_id | package | test target | exact test names (space separated)
GATES=(
  "G13-r22-billing-settlement|axon-fabric|journal|g13_unknown_billing_keeps_the_full_reservation_as_liability_never_zero g13_identical_settlement_receipt_is_idempotent g13_duplicate_receipt_with_different_content_is_refused_and_recorded g13_settlement_without_origin_is_refused_and_writes_nothing g13_a_journal_holding_a_duplicate_settlement_line_is_corrupt"
  "G13-r22-billing-settlement|axon-loop|tel_price|g13_an_unknown_cost_never_produces_a_zero_cost_winner"
  "G10-r22-cohort-denominator|axon-loop|tel_price|g10_cohort_denominator_counts_unknown_as_unresolved_not_free g10_price_schedule_is_pinned_by_content_ref g10_price_schedule_ref_mismatch_is_refused"
  # Whole dedicated test files ('*N' = run every test in the target, require at
  # least N): each file exists only for the gate(s) named, so all of it must pass.
  "G03-r22-workspace-import|axon-fabric|workspace|*27"
  "G28-r22-workspace-not-context|axon-fabric|workspace|*27"
  "G28-r22-workspace-not-context|axon-fabric|check_effects|*8"
  "G03-r22-check-effects|axon-fabric|check_effects|*8"
  "G32-r22-receipt-roles|axon-loop|evl|*8"
  "G32-r22-artifact-recheck|axon-loop|evl|*8"
  "G32-r22-artifact-recheck|axon-fabric|check_effects|*8"
  "G08-r22-logical-branches|axon-fabric|branches|*6"
  "G11-r22-workspace-cas|axon-fabric|branches|*6"
  "G08-r22-branch-cancellation|axon-fabric|branches|*6"
  # B256 (G16-r22-negotiation / closed-wire), Axon half: the contracts rule and
  # the REAL adapter process answering every shared vector.
  "G16-r22-negotiation|axon-loop-contracts|profile|*13"
  "G16-r22-negotiation|cortex-policy-adapter|negotiate|*4"
  "G16-r22-closed-wire|cortex-policy-adapter|negotiate|ambiguous_or_open_offers_are_refused_not_resolved negotiate_is_a_mode_and_cannot_be_mixed_with_a_grant"
  # G3 under D12: intake joins a MiCode acceptance check that ran through
  # Fabric (the real-binary join is loop_interop_gate.sh section 8).
  "G3-d12-intake-join|axon-loop|intake|a_fabric_check_on_the_output_tree_is_recorded_as_the_verification an_unverified_record_serialises_without_the_new_fields verification_that_does_not_join_is_refused_with_the_store_unchanged"
  # B261: the omission-set regression the paired G3 gate found (4430f104).
  "G03-r22-workspace-import|axon-fabric|workspace|the_same_version_imported_with_different_omissions_is_one_version a_tampered_omission_record_is_corrupt a_check_over_a_version_from_a_real_repository_passes"
  # B280 (post-Stage-5 v0.22 work; this runner executes every v0.22 row by name).
  # G13: real process deaths at every submit boundary, inside branch
  # cancellation and inside a pointer transition; no unowned worker.
  "G13-r22-restart-matrix|axon-fabric|restart_matrix|a_pre_launch_crash_is_resumed_once_and_never_repeated a_crash_after_the_launch_record_is_unknown_never_rerun a_crash_between_terminal_and_receipt_is_explicit_and_never_rerun an_orphan_is_not_resumed_under_a_superseded_epoch a_crash_inside_branch_cancellation_converges_on_restart"
  "G13-r22-restart-matrix|axon-fabric|submit|sigkill_after_launch_reconciles_to_outcome_unknown_with_liability"
  "G13-r22-restart-matrix|axon-fabric|journal|sigkill_after_launch_reconciles_to_outcome_unknown_with_liability_kept sigkill_after_intent_only_leaves_an_intended_op_with_no_reservation sigkill_after_reserve_keeps_the_budget_held"
  "G13-r22-restart-matrix|axon-loop|pointer|a_real_crash_inside_a_transition_rolls_forward_exactly_once"
  # G16: peer outage, replay, stale epochs, partial export, schema mismatch.
  "G16-r22-peer-failure-matrix|axon-fabric|peer_failure_matrix|an_unreadable_epoch_store_at_submit_refuses_and_records_nothing an_outage_between_submit_and_dispatch_launches_nothing_and_stays_explicit a_resent_request_after_an_outage_replays_and_never_duplicates authority_that_moved_during_an_outage_is_not_assumed a_request_of_another_schema_version_is_refused_before_the_journal"
  "G16-r22-peer-failure-matrix|axon-loop|intake|a_partial_export_is_refused_then_the_complete_one_is_recorded_once an_episode_of_another_schema_version_is_refused"
  "G16-r22-peer-failure-matrix|axon-loop|pointer|a_replayed_activation_after_authority_moved_does_not_reactivate"
  # B282: joint bypass (routes) and evidence laundering (the whole admission chain).
  "G03-r22-joint-bypass|axon-fabric|joint_bypass|a_repository_cannot_supply_its_own_registries an_outcome_cannot_be_reattached_or_attached_early an_argv_file_outside_the_workspace_is_refused"
  "G03-r22-joint-bypass|axon-fabric|workspace|a_legacy_single_file_swapped_before_launch_never_yields_a_verdict_on_the_original two_trials_caches_do_not_see_each_others_writes"
  "G03-r22-joint-bypass|axon-fabric|grant_authority|*7"
  "G03-r22-joint-bypass|axon-fabric|check_effects|*8"
  "G32-r22-evidence-laundering|axon-loop|evidence_laundering|a_clean_bundle_is_accepted_positive_control laundered_evidence_never_crosses_independent_admission"
  "G32-r22-evidence-laundering|axon-loop|intake|verification_that_does_not_join_is_refused_with_the_store_unchanged"
  # 80-gate sweep: gates whose every clause an existing test asserts (verified
  # clause by clause, .axon-v022/sweep/GATE_SWEEP.json).
  "G13-r22-journal-before-effect|axon-fabric|journal|intent_is_on_disk_before_begin_returns same_operation_id_with_a_different_input_digest_is_a_conflict a_torn_final_line_is_truncated_and_everything_before_it_survives"
  "G13-r22-journal-before-effect|axon-fabric|submit|same_op_different_input_is_a_conflict_with_zero_effects the_submitted_ops_state_is_visible_after_reopen duplicate_op_same_input_returns_the_same_receipt_without_re_execution"
  "G13-r22-journal-before-effect|axon-fabric|restart_matrix|a_pre_launch_crash_is_resumed_once_and_never_repeated a_crash_after_the_launch_record_is_unknown_never_rerun a_crash_between_terminal_and_receipt_is_explicit_and_never_rerun"
  "G21-r22-inconclusive-valid|axon-loop|evl_admission|inconclusive_on_small_sample_liability_and_unknowns reject_on_inferiority_or_no_economic_benefit"
  "G21-r22-inconclusive-valid|axon-loop|redteam|j201_zero_pass_candidate_never_accepted j201_zero_vs_zero_never_accepted g7_plan_shopping_is_refused g6_freeze_is_permanent g7_trials_before_freeze_are_refused"
  "G11-r22-policy-cas|axon-loop|pointer|concurrent_threads_exactly_one_wins replayed_transition_id_is_idempotent_and_conflicting_reuse_refused revoked_active_policy_makes_resolve_pause refusals_change_no_bytes a_replayed_activation_after_authority_moved_does_not_reactivate"
  "G11-r22-policy-cas|axon-loop|candidates|g2_baseline_and_activation_recheck_the_list"
  "G11-r22-policy-cas|axon-loop|cli|cli_two_processes_race_exactly_one_wins"
  "G13-r22-unknown-reconcile|axon-fabric|submit|a_timeout_after_launch_is_unknown_with_liability_and_is_never_retried sigkill_after_launch_reconciles_to_outcome_unknown_with_liability"
  "G13-r22-unknown-reconcile|axon-fabric|journal|g13_unknown_billing_keeps_the_full_reservation_as_liability_never_zero an_unknown_outcome_can_be_settled_but_not_completed"
  "G13-r22-unknown-reconcile|axon-fabric|restart_matrix|a_crash_after_the_launch_record_is_unknown_never_rerun"
  "G26-r22-speculation-disabled|axon-fabric|joint_bypass|a_request_cannot_ask_for_speculative_dispatch"
  "G26-r22-speculation-disabled|axon-fabric|branches|publication_requires_base_epoch_writer_exact_verified_output_and_approval each_branch_carves_within_its_declared_regime_and_is_isolated cancelling_a_losing_branch_keeps_its_record_and_leaves_the_winner_alone"
  "G10-r22-trial-identity|axon-fabric|submit|repeated_trials_of_one_task_are_distinct_and_only_a_transport_retry_replays duplicate_op_same_input_returns_the_same_receipt_without_re_execution same_op_different_input_is_a_conflict_with_zero_effects"
  "G10-r22-trial-identity|axon-loop|intake|a_repeated_trial_of_one_task_and_arm_is_recorded_as_its_own the_same_trial_with_different_bytes_is_a_conflict"
  "G10-r22-trial-identity|axon-loop|redteam|ab10_trial_ids_never_reused_across_experiments"
  "G29-r22-bounded-mutation|axon-loop|plan_evo_tel|evo_proposes_a_bounded_deterministic_candidate evo_regularizes_history_and_exhausts"
  "G29-r22-bounded-mutation|axon-loop|candidates|g2_policy_put_refuses_a_tool_adding_shortlist g2_evo_eligible_must_equal_the_registered_list g2_freeze_refuses_a_candidate_outside_the_list g2_baseline_and_activation_recheck_the_list"
  "G29-r22-bounded-mutation|axon-loop|redteam|o1_o2_non_evo_or_widening_candidate_cannot_be_frozen g6_freeze_is_permanent g7_plan_shopping_is_refused"
)

for row in "${GATES[@]}"; do
  IFS='|' read -r gid pkg target names <<<"$row"
  if [ "${names#\*}" != "$names" ]; then
    floor="${names#\*}"
    [ -f "crates/$pkg/tests/$target.rs" ] || { echo "  FAIL $gid: crates/$pkg/tests/$target.rs missing"; fail=1; continue; }
    out="$(cargo test --locked -q -p "$pkg" --test "$target" 2>&1)"; rc=$?
    ran="$(printf '%s\n' "$out" | sed -n 's/^test result: [a-zA-Z]*\. \([0-9]*\) passed.*/\1/p' | tail -1)"
    ign="$(printf '%s\n' "$out" | sed -n 's/^test result:.* \([0-9]*\) ignored.*/\1/p' | tail -1)"
    if [ "$rc" -ne 0 ]; then printf '%s\n' "$out" | tail -15; echo "  FAIL $gid ($pkg/$target): cargo test exited $rc"; fail=1
    elif [ "${ran:-0}" -lt "$floor" ]; then echo "  FAIL $gid ($pkg/$target): ran ${ran:-0}, floor $floor"; fail=1
    elif [ "${ign:-0}" -ne 0 ]; then echo "  FAIL $gid ($pkg/$target): $ign test(s) ignored — an ignored test is not evidence"; fail=1
    else echo "  OK   $gid ($pkg/$target): $ran tests passed (floor $floor)"; fi
    continue
  fi
  want=0
  for n in $names; do
    want=$((want + 1))
    if ! grep -rqs "fn $n(" "crates/$pkg/tests/$target.rs"; then
      echo "  FAIL $gid: test $n not found in crates/$pkg/tests/$target.rs"; fail=1
    fi
  done
  # --exact: run precisely these tests, nothing matched by substring.
  # shellcheck disable=SC2086
  out="$(cargo test --locked -q -p "$pkg" --test "$target" -- --exact $names 2>&1)"; rc=$?
  ran="$(printf '%s\n' "$out" | sed -n 's/^test result: [a-zA-Z]*\. \([0-9]*\) passed.*/\1/p' | tail -1)"
  if [ "$rc" -ne 0 ]; then
    printf '%s\n' "$out" | tail -15; echo "  FAIL $gid ($pkg/$target): cargo test exited $rc"; fail=1
  elif [ "${ran:-0}" -ne "$want" ]; then
    echo "  FAIL $gid ($pkg/$target): ran ${ran:-0} test(s), expected exactly $want — a test was renamed or filtered away"; fail=1
  else
    echo "  OK   $gid ($pkg/$target): $ran/$want tests passed"
  fi
done

if [ "$fail" -ne 0 ]; then echo "v022_stage5_gates: FAIL"; exit 1; fi
echo "v022_stage5_gates: PASS"
