#!/usr/bin/env bash
# v022_micode_gates.sh — run the EXACT MiCode tests that back each v0.22 gate
# whose evidence lives on the MiCode side, recorded in
# governance/cortex_gate_execution_registry.json.
#
# The MiCode counterpart of v022_stage5_gates.sh. Without it, a MiCode-side
# property was vouched for only by "the whole MiCode suite passed" — which names
# no gate, so a renamed or deleted test would drop a gate's evidence silently.
# Each row names the test functions that check its property; the script fails
# if one is missing from the source (renamed evidence) or if the exact run
# executes a different number of tests than listed.
#
# Env: MICODE_DIR (default ../micode-v022-wt, the same peer loop_interop_gate.sh
# pairs with), MICODE_TARGET_DIR (default: CARGO_TARGET_DIR/micode, else
# MiCode's own target/).
#
# Final line:
#   "v022_micode_gates: PASS — N rows (micode <sha>)"   every listed test ran and passed
#   "v022_micode_gates: SKIP — …"                        no MiCode worktree: measured
#                                                        NOTHING (exit 0, FAIL under
#                                                        AXON_HARNESS_STRICT=1, exit 3)
#   "v022_micode_gates: FAIL"                            exit 1
set -uo pipefail
AXON_DIR="$(cd "$(dirname "$0")/.." && pwd)"

if [ -n "${MICODE_DIR:-}" ]; then
  [ -d "$MICODE_DIR" ] || { echo "FATAL: MICODE_DIR=$MICODE_DIR does not exist" >&2; echo "v022_micode_gates: FAIL"; exit 1; }
  MICODE_DIR="$(cd "$MICODE_DIR" && pwd)"
else
  MICODE_DIR="$(cd "$AXON_DIR/../micode-v022-wt" 2>/dev/null && pwd)"
  if [ -z "$MICODE_DIR" ]; then
    if [ "${AXON_HARNESS_STRICT:-}" = 1 ]; then
      echo "v022_micode_gates: FAIL — SKIP under AXON_HARNESS_STRICT=1: no MiCode worktree at $AXON_DIR/../micode-v022-wt (set MICODE_DIR)"
      exit 3
    fi
    echo "v022_micode_gates: SKIP — no MiCode worktree at $AXON_DIR/../micode-v022-wt and MICODE_DIR unset"
    exit 0
  fi
fi
if [ -n "${MICODE_TARGET_DIR:-}" ]; then MICODE_TGT="$MICODE_TARGET_DIR"
elif [ -n "${CARGO_TARGET_DIR:-}" ]; then MICODE_TGT="$CARGO_TARGET_DIR/micode"
else MICODE_TGT="$MICODE_DIR/target"; fi
cd "$MICODE_DIR" || exit 1

fail=0
rows=0
# gate_id | package | target ("lib" or an integration test file) | exact test names
# (lib names are module paths: `module::tests::name`)
GATES=(
  # G01-r22-unknown-outcome: only a FINISHED task is checked, so a cancelled or
  # failed one records not_run with its status, never a contract-invalid verdict.
  "G01-r22-unknown-outcome|micode|lib|assembly::acceptance_check_runs_only_for_a_finished_task::only_completed"
  # ...and the bridge STATES why there is no verdict (review wf_849bc606-7e8): a hung,
  # crashed, refusing or unattesting Fabric and a run past its deadline are different
  # markers; only a cancellation is `cancelled`.
  "G01-r22-unknown-outcome|micode-persist|lib|loop_sidecar::tests::a_not_run_reason_is_a_distinct_marker_the_schema_accepts loop_sidecar::tests::only_a_cancellation_is_cancelled fabric_check::tests::a_hung_a_crashed_and_a_refusing_fabric_are_different_failures fabric_check::tests::evidence_about_anything_else_is_refused fabric_check::tests::an_unattested_or_misattested_receipt_is_not_cited"
  "G01-r22-unknown-outcome|micode-core|lib|raw_loop::tests::a_silent_provider_past_its_deadline_ends_as_a_timeout_not_a_drop"
  # B267: the pinned policy's authority is rechecked at tool execution — before
  # the gate AND immediately before the effect, in children, on retries.
  "G03-r22-dispatch-recheck|micode-core|lib|dispatch::tests::a_revoked_pinned_policy_stops_the_next_call_and_every_retry dispatch::tests::an_authority_view_that_cannot_vouch_for_the_pin_refuses_the_call dispatch::tests::a_revocation_during_an_approval_wait_stops_the_effect scope::tests::a_child_must_carry_its_parents_authority_fence_unchanged"
  "G03-r22-dispatch-recheck|micode-delegate|lib|task_tool::tests::a_child_inherits_its_parents_authority_fence"
  "G03-r22-dispatch-recheck|micode|closed_loop_v022_real_binary|a_revoked_policy_is_refused_at_tool_execution_through_the_real_binary"
  # B266: every context clause observed, at task start and at every child spawn.
  "G16-r22-preflight-start|micode-persist|lib|exec_context::tests::the_repository_identity_is_observed_and_a_different_one_refuses exec_context::tests::every_observable_field_is_compared exec_context::tests::a_trial_subject_needs_a_dedicated_worktree_concrete_paths_and_its_own_namespace exec_context::tests::each_roles_write_contract_is_checked_before_the_task_starts"
  "G16-r22-preflight-start|micode|lib|assembly::tests::v022_a_child_whose_context_mismatches_its_task_is_refused_in_preflight"
  "G16-r22-preflight-start|micode|closed_loop_v022_real_binary|a_mismatched_context_is_refused_before_any_provider_call a_different_model_route_is_refused_before_any_provider_call"
  # B266: a result is classified against the parent's CURRENT context, never
  # silently integrated.
  "G16-r22-preflight-return|micode-persist|lib|result_receipt::tests::a_result_is_classified_against_the_parents_current_context"
  "G16-r22-preflight-return|micode|lib|assembly::tests::v022_a_returned_result_is_classified_against_the_parents_current_context"
  # B266: each role's write/build contract, enforced.
  "G16-r22-role-scope|micode-core|lib|scope::tests::a_read_only_role_writes_nothing_and_its_children_stay_read_only scope::tests::the_write_set_bounds_writes_but_not_reads"
  "G16-r22-role-scope|micode-persist|lib|exec_context::tests::each_roles_write_contract_is_checked_before_the_task_starts exec_context::tests::a_trial_subject_needs_a_dedicated_worktree_concrete_paths_and_its_own_namespace"
  "G16-r22-role-scope|micode-delegate|lib|spawner::tests::a_child_scope_not_provably_within_the_parents_is_refused"
  "G16-r22-role-scope|micode|closed_loop_v022_real_binary|a_read_only_role_cannot_write_through_the_real_binary the_declared_write_set_is_enforced_at_the_tool_boundary"
  # B267: a policy selects only from permitted candidates; delegated tools and
  # file effects are held to inherited limits; unknown relations are denied.
  "G16-r22-local-authority|micode-persist|lib|active_policy::tests::invalid_policies_abstain_to_the_incumbent_with_a_reason"
  "G16-r22-local-authority|micode-core|lib|scope::tests::path_globs_cover_exact_paths_and_directory_prefixes_only scope::tests::the_write_set_bounds_writes_but_not_reads scope::tests::a_child_must_be_provably_within_its_parent scope::tests::an_unprovable_pattern_fails_closed_everywhere"
  "G16-r22-local-authority|micode|closed_loop_v022_real_binary|a_delegated_child_is_held_to_the_policys_shortlist_through_the_real_binary"
  # B268: a policy only reorders or narrows; it introduces nothing.
  "G16-r22-candidate-shortlist|micode-persist|lib|active_policy::tests::a_policy_naming_any_other_authority_dimension_abstains_naming_it active_policy::tests::a_valid_policy_pins_its_ref_and_ordered_shortlist"
  "G16-r22-candidate-shortlist|micode|closed_loop_v022_real_binary|a_policy_narrows_the_sent_tools_and_an_invalid_one_abstains_to_the_incumbent"
  # G01 (re-audit 4): MiCode's side of verification authenticity — it cites
  # no verdict Fabric did not attest, and a repository cannot choose its own
  # verification (the `axon.` namespace is closed to project config).
  # Defense in depth: Axon's intake re-verifies either way.
  "G01-r22-independent-issuer|micode-persist|lib|fabric_check::tests::an_unattested_or_misattested_receipt_is_not_cited"
  "G01-r22-independent-issuer|micode|lib|config::tests::a_repository_cannot_choose_its_own_verification config::tests::a_dotenv_cannot_supply_a_closed_key"
)

for row in "${GATES[@]}"; do
  IFS='|' read -r gid pkg target names <<<"$row"
  rows=$((rows + 1))
  want=0
  for n in $names; do
    want=$((want + 1))
    leaf="${n##*::}"
    if [ "$target" = lib ]; then where="crates/$pkg/src"; else where="crates/$pkg/tests/$target.rs"; fi
    if ! grep -rqsE "fn $leaf\(" "$where"; then
      echo "  FAIL $gid: test $leaf not found in $where"; fail=1
    fi
  done
  if [ "$target" = lib ]; then sel=(--lib); else sel=(--test "$target"); fi
  # --exact: run precisely these tests, nothing matched by substring.
  # shellcheck disable=SC2086
  out="$(env -u CARGO_TARGET_DIR CARGO_TARGET_DIR="$MICODE_TGT" \
      cargo test --locked -q -p "$pkg" "${sel[@]}" -- --exact $names 2>&1)"; rc=$?
  ran="$(printf '%s\n' "$out" | sed -n 's/^test result: [a-zA-Z]*\. \([0-9]*\) passed.*/\1/p' | tail -1)"
  if [ "$rc" -ne 0 ]; then
    printf '%s\n' "$out" | tail -15; echo "  FAIL $gid ($pkg/$target): cargo test exited $rc"; fail=1
  elif [ "${ran:-0}" -ne "$want" ]; then
    echo "  FAIL $gid ($pkg/$target): ran ${ran:-0} test(s), expected exactly $want — a test was renamed or filtered away"; fail=1
  else
    echo "  OK   $gid ($pkg/$target): $ran/$want tests passed"
  fi
done

if [ "$fail" -ne 0 ]; then echo "v022_micode_gates: FAIL"; exit 1; fi
echo "v022_micode_gates: PASS — $rows rows (micode $(git -C "$MICODE_DIR" rev-parse --short HEAD))"
