#!/usr/bin/env bash
# v022_pci_gates.sh — run the EXACT tests behind each Protected Check Isolation
# surface (governance/specs/v022-protected-check-isolation.md).
#
# PCI is the prerequisite of any protected-verifier claim, and G01 alone is never
# sufficient. This is its regression suite: one row per surface, naming the tests
# that execute it. The script fails if a listed test is missing, if the filtered
# run executes a different number of tests than listed, or if any test fails.
# The mutation proof is `scripts/v022_g01_mutations.py --scope=pci`.
#
# The Fabric rows drive the real `axon` interpreter from the target dir. Build it
# first: cargo build -p axon-core --no-default-features --bin axon
#
# Exit 0 only if every listed test ran and passed.
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
# surface | package | target (`lib` or an integration-test name) | exact test names
ROWS=(
  "1  shadowing|axon-fabric|check_effects|a_candidate_cannot_shadow_a_module_of_the_suite"
  "2  redefinition|axon-core|lib|resolver::tests::duplicate_let_refinement_or_impl_produces_e0002 resolver::tests::duplicate_fn_name_produces_e0002"
  "2b second-trait method|axon-fabric|check_effects|a_candidate_cannot_redefine_a_suite_helpers_impl_or_constant"
  "3  symlink escape|axon-fabric|check_effects|a_candidate_holding_a_symlink_is_refused"
  "4  ambient module path|axon-fabric|check_effects|a_check_loads_no_module_from_outside_the_suite_and_the_candidate a_suite_module_never_resolves_from_the_trial_cache"
  "4  ambient module path|axon-core|pci_isolation|an_exclusive_module_path_never_falls_through_to_ambient_dirs"
  "5  path-list injection|axon-fabric|check_effects|a_state_dir_that_would_split_the_module_path_is_refused"
  "6  verifier environment|axon-fabric|attestation|the_launchers_environment_does_not_steer_a_signed_verdict the_empty_ceiling_is_applied_not_just_intended"
  "7  loop control|axon-core|lib|interp::tests::loop_control_does_not_escape_through_a_predicate interp::tests::an_escaped_break_or_continue_does_not_pass_a_test interp::tests::no_control_transfer_escapes_a_frame"
  "7  loop control|axon-fabric|check_effects|an_escaped_break_does_not_pass_the_operators_test an_escaped_break_cannot_end_the_operators_test_loop"
  "8/11/12 return, exit, Err|axon-core|lib|interp::tests::a_test_completes_only_when_its_body_returns_normally"
  "10 named handlers|axon-core|pci_isolation|a_candidate_cannot_rebind_a_suite_named_handler"
  "13 completion evidence|axon-core|test_completion|only_a_completed_test_is_issued_a_completion_token a_missing_or_short_completion_key_is_refused_before_any_test_runs"
  "13 completion evidence|axon-fabric|check_effects|a_pass_needs_evidence_that_the_test_completed a_candidate_predicate_cannot_end_the_operators_test the_cortex_executor_accepts_only_a_receipt_that_binds_its_candidate"
  "14 exit code|axon-fabric|check_effects|a_named_pass_in_a_run_that_exits_nonzero_is_not_a_pass"
  "17 working directory|axon-fabric|check_effects|a_suites_runtime_fixture_is_the_pinned_one"
  "21 sealed candidate|axon-core|lib|resolver::tests::a_sealed_module_cannot_reach_the_operators_names interp::tests::runtime_sealing_holds_without_the_static_check"
  "21 sealed candidate|axon-fabric|check_effects|a_sealed_candidate_cannot_reach_the_operators_names a_sealed_candidate_cannot_reach_operator_state_by_handle_or_definition"
  # The interpreter changes since the certified 31413ca7 (governance/notes/v022-pci-delta.md).
  # One group per amendment, each with unit tests AND a real-runner test (axon_psv::runner::run),
  # so a delta is exercised through the path a protected verdict takes. To extend: add a row
  # (or names to a row) for the amendment's own named tests; the count check below fails the gate
  # if a named test is absent, renamed, filtered out or #[ignore]d.
  "am53 declared-boundary cast|axon-core|lib|interp::tests::a_confused_scalar_never_crosses_a_declared_return interp::tests::a_confused_struct_never_crosses_as_another_struct interp::tests::a_confused_argument_never_enters_a_declared_parameter interp::tests::a_closures_confused_result_never_crosses_its_declared_type"
  "am53 declared-boundary cast|axon-psv|sealed_frames|the_candidate_never_chooses_the_operators_judging_method"
  "am60 dispatch-key cast|axon-core|lib|interp::tests::a_value_of_another_integer_width_never_crosses_a_declared_integer interp::tests::a_value_of_another_fixed_width_never_crosses_a_declared_fixed_width interp::tests::a_confused_enum_never_crosses_as_another_enum"
  "am60 dispatch-key cast|axon-psv|sealed_frames|the_candidate_never_selects_the_operators_code_by_width_or_by_name"
  "am72 seal-crossing positions|axon-core|lib|interp::tests::a_value_at_an_undetermined_type_parameter_never_crosses_the_seal interp::tests::a_type_parameter_inside_any_shape_is_never_filled_by_the_candidate"
  "am72 seal-crossing positions|axon-psv|sealed_frames|an_undetermined_type_position_never_selects_the_operators_impl"
  "am72 dict snapshot|axon-core|lib|interp::tests::a_dict_the_candidate_mutated_is_verified_at_every_edge_back"
  "am72 dict snapshot|axon-psv|sealed_frames|a_dict_entry_the_operator_held_is_never_retyped_by_the_candidate"
  "am78 held-value judgement|axon-core|lib|interp::tests::a_position_the_operator_held_is_judged_by_what_it_held_when_replaced interp::tests::a_placeholder_the_operator_held_is_not_filled_by_the_candidate interp::conform::walk_bound_tests::a_value_nested_past_the_bound_is_refused_not_left_unvisited"
  "am78 held-value judgement|axon-psv|sealed_frames|a_replaced_or_filled_position_is_judged_by_what_the_operator_held"
  "am72 absent return type is ()|axon-core|lib|interp::tests::a_fn_with_no_declared_return_type_hands_the_operator_unit"
  "am72 channel stamped at creation|axon-core|lib|interp::tests::a_channel_carries_the_element_type_its_creation_states"
  "am72 closure args strict at a crossing|axon-core|lib|interp::tests::an_operator_closure_called_from_sealed_code_takes_only_determined_arguments"
  "am83 arithmetic width arm|axon-core|lib|interp::tests::operator_arithmetic_never_runs_at_a_width_the_candidate_chose"
  "am83 dispatch rule|axon-core|lib|interp::tests::operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict interp::tests::operator_code_never_dispatches_on_a_value_from_any_untyped_position"
  "am83 dispatch rule|axon-psv|sealed_frames|operator_code_never_dispatches_on_an_untyped_read_and_a_pinned_suite_passes"
)

for row in "${ROWS[@]}"; do
  IFS='|' read -r sid pkg target names <<<"$row"
  # axon-core is exercised on the interpreter build, never with codegen.
  feat=(); [ "$pkg" = axon-core ] && feat=(--no-default-features)
  want=0
  for n in $names; do
    want=$((want + 1))
    if [ "$target" = lib ]; then
      # interp::conform::walk_bound_tests::x lives in src/interp/conform.rs: strip the
      # trailing `tests`-style module and the fn, then try the longest path that is a file.
      fn="${n##*::}"; modpath="${n%::*}"; f=""
      while [ -n "$modpath" ]; do
        c="crates/$pkg/src/${modpath//:://}.rs"
        if [ -f "$c" ]; then f="$c"; break; fi
        case "$modpath" in *::*) modpath="${modpath%::*}" ;; *) modpath="" ;; esac
      done
      [ -n "$f" ] || f="crates/$pkg/src/${n%%::*}.rs"
    else
      fn="$n"; f="crates/$pkg/tests/$target.rs"
    fi
    grep -qs "fn $fn(" "$f" || { echo "  FAIL PCI $sid: test $n not found in $f"; fail=1; }
  done
  if [ "$target" = lib ]; then sel=(--lib); else sel=(--test "$target"); fi
  # --exact: precisely these tests, nothing matched by substring.
  # shellcheck disable=SC2086
  out="$(cargo test --locked -q -p "$pkg" "${feat[@]}" "${sel[@]}" -- --exact $names 2>&1)"; rc=$?
  ran="$(printf '%s\n' "$out" | sed -n 's/^test result: [a-zA-Z]*\. \([0-9]*\) passed.*/\1/p' | tail -1)"
  if [ "$rc" -ne 0 ]; then
    printf '%s\n' "$out" | tail -15; echo "  FAIL PCI $sid ($pkg/$target): cargo test exited $rc"; fail=1
  elif [ "${ran:-0}" -ne "$want" ]; then
    echo "  FAIL PCI $sid ($pkg/$target): ran ${ran:-0} test(s), expected exactly $want"; fail=1
  else
    echo "  OK   PCI $sid ($pkg/$target): $ran/$want"
  fi
done

if [ "$fail" -ne 0 ]; then echo "v022_pci_gates: FAIL"; exit 1; fi
echo "v022_pci_gates: PASS — ${#ROWS[@]} rows"
