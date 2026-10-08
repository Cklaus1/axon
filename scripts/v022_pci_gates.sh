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
  # Amendment 94: origin/main's interpreter features (Rc arrays and strings, `&mut` write-through,
  # first-class fns, lent closure captures, arr_sort_by) were merged without PCI review; these rows are
  # the routes that merge opened and what now refuses each.
  "am94 &mut edge-back cast|axon-core|lib|interp::tests::a_mut_write_through_value_is_cast_at_the_seal_edge_back interp::tests::a_mut_write_through_value_is_judged_by_what_the_operator_held interp::tests::a_mut_value_is_cast_when_an_operator_handler_aborts_the_call interp::tests::an_annotated_operator_array_lent_as_mut_is_still_cast_at_the_edge_back"
  "am94 &mut operand open in the dispatch analysis|axon-core|lib|interp::tests::a_mut_operand_is_never_determined_by_the_pin_analysis interp::tests::a_mut_operand_is_open_in_the_dispatch_analysis"
  "am94 &mut edge (dispatch and width arms)|axon-psv|sealed_frames|a_mut_write_through_value_is_cast_at_the_seal_edge_back_and_the_operand_is_open"
  "am94 fn-value seal edge|axon-core|lib|interp::tests::a_sealed_frame_cannot_take_an_operator_fn_as_a_value interp::tests::a_candidate_fn_value_the_operator_calls_still_cannot_reach_operator_fns"
  "am94 fn-value seal edge|axon-psv|sealed_frames|a_sealed_frame_cannot_take_an_operator_fn_as_a_value"
  "am94 shared-Rc/COW non-leak|axon-psv|sealed_frames|a_candidates_write_to_its_array_parameter_never_reaches_the_operators_copy"
  # Amendment 96 (round 9): the fourth instance in two rounds of one class -- a builtin result or a lookup that
  # skipped the seal edge -- and the sweep that enumerated the rest.
  "am96 sandbox_run result cast at the crossing|axon-core|lib|interp::tests::sandbox_run_results_are_cast_at_the_seal_crossing"
  "am96 sandbox_run result cast at the crossing|axon-psv|sealed_frames|sandbox_run_hands_the_operator_an_i64_or_nothing"
  "am96 one global-read edge|axon-core|lib|interp::tests::a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path interp::tests::every_global_read_goes_through_global_ref"
  "am96 one global-read edge (runner leg: corroboration only, refused statically by E0004 before the runtime edge)|axon-psv|sealed_frames|a_candidate_calls_its_own_fn_value_and_never_reads_an_operator_global"
  "am96 fn value mark|axon-core|lib|interp::tests::a_candidates_own_fn_value_takes_arguments_and_an_operators_still_does_not"
  "am96 unary width arm|axon-core|lib|interp::tests::operator_unary_arithmetic_never_runs_at_a_width_the_candidate_chose"
  "am96 unary width arm|axon-psv|sealed_frames|operator_negation_never_runs_at_a_width_the_candidate_chose"
  "am96 user-code builtins classified|axon-core|lib|interp::tests::every_builtin_that_runs_user_code_is_classified interp::tests::the_sweep_routes_stay_closed"
  "am96 handler-expression value is undetermined (stated cost)|axon-core|lib|interp::tests::the_value_of_a_with_handler_expression_is_undetermined_until_pinned"
  # Amendment 96's runner leg of "one global-read edge" is CORROBORATION: the candidate's read is refused
  # statically (E0004) before it reaches the runtime edge; the runtime edge is the interpreter unit test's, with
  # the static check bypassed (amendment 100).
  # Amendment 100 (round 10): a handler arm runs under the pin owner of the fn that INSTALLED it, and a name a
  # name-resolving builtin runs must be one the operator chose.
  "am100 handler arm pin owner|axon-core|lib|interp::tests::a_handler_arm_dispatch_runs_under_the_installers_pin_owner interp::tests::a_handler_arm_replay_runs_under_the_installers_pin_owner interp::tests::a_handler_arm_arithmetic_runs_under_the_installers_pin_owner interp::tests::a_closure_made_in_a_handler_arm_is_judged_by_the_installing_fn interp::tests::identical_site_text_in_two_fns_gets_two_verdicts interp::tests::every_frame_that_runs_stored_operator_code_sets_its_owner"
  "am100 handler arm pin owner|axon-psv|sealed_frames|a_handler_arm_dispatch_is_judged_by_the_fn_that_installed_it a_handler_arm_replay_is_judged_by_the_fn_that_installed_it a_handler_arm_arithmetic_is_judged_by_the_fn_that_installed_it"
  "am100 name sinks|axon-core|lib|interp::tests::a_function_name_the_candidate_chose_never_selects_an_operator_fn interp::tests::scheduler_spawn_takes_no_function_name_the_candidate_chose interp::tests::goal_eval_takes_no_function_name_the_candidate_chose interp::tests::a_goal_constraint_and_a_kernel_goal_take_no_function_name_the_candidate_chose interp::tests::every_name_resolving_lookup_is_a_listed_sink"
  "am100 name sinks|axon-psv|sealed_frames|sandbox_run_runs_no_function_name_the_candidate_chose scheduler_spawn_runs_no_function_name_the_candidate_chose goal_eval_runs_no_function_name_the_candidate_chose"
  "am100 existence oracle|axon-core|lib|interp::tests::a_sealed_caller_cannot_tell_an_operator_fn_from_a_missing_one"
  "am100 drift: any globals mention, any runner of user code|axon-core|lib|interp::tests::every_global_read_goes_through_global_ref interp::tests::every_builtin_that_runs_user_code_is_classified"
  # Amendment 102: the runtime taint. Each attack runs with the STATIC pin analysis OFF (only the
  # taint rules on) and again with every rule off, to show it is live; the honest controls run
  # with both layers on. One test per family, one case per hook of the taint.
  "am102 runtime taint: closure picks|axon-core|lib|interp::taint_tests::an_operator_closure_the_candidate_picked_is_never_called"
  "am102 runtime taint: names|axon-core|lib|interp::taint_tests::a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn interp::taint_tests::bypass_shapes_hunted_after_the_first_cut"
  "am102 runtime taint: impl and width|axon-core|lib|interp::taint_tests::a_type_or_width_sealed_code_chose_is_never_dispatched_on"
  "am102 runtime taint: carriers and control|axon-core|lib|interp::taint_tests::taint_survives_every_carrier_a_value_can_travel_by interp::taint_tests::the_remaining_routes_a_selection_can_take interp::taint_tests::every_hook_of_the_taint_has_an_attack_of_its_own"
  "am102 runtime taint: an ordinary run takes none|axon-core|lib|interp::taint_tests::an_ordinary_run_takes_no_taint"
  "am102 drift: classes, writers, dispatch sites, value holders, frames, swept files|axon-core|lib|interp::taint_tests::every_builtin_has_a_taint_class interp::taint_tests::every_dict_builtin_is_a_listed_writer_or_a_reader_that_does_not_write interp::taint_tests::builtins_are_dispatched_only_where_the_taint_is_routed interp::taint_tests::every_type_that_holds_a_value_keeps_its_taint interp::taint_tests::only_fn_and_closure_frames_restore_the_control_taints interp::taint_tests::every_interp_file_is_read_by_the_drift_sweeps"
  "am102 runtime taint (runner leg)|axon-psv|sealed_frames|an_operator_closure_the_candidate_picked_is_never_called a_name_built_out_of_the_candidates_bit_never_selects_an_operator_fn taint_survives_every_carrier_a_value_can_travel_by"
  "am106 shared state keeps its taint: channels, areas not hunted before|axon-core|lib|interp::taint_tests::a_channel_the_candidate_touched_never_selects_an_operator_closure_or_name interp::taint_tests::areas_the_psv3_reviewer_did_not_hunt"
  "am106 every reader of shared state is tainted after a sealed write|axon-core|lib|interp::taint_tests::a_dict_reader_is_tainted_after_a_sealed_write_and_clean_after_the_operators interp::taint_tests::a_kernel_getter_is_tainted_after_a_write_the_candidate_steered interp::taint_tests::a_shared_array_is_tainted_after_a_sealed_write"
  "am106 the existence oracle on every path|axon-core|lib|interp::taint_tests::a_sealed_caller_cannot_tell_an_operator_name_from_a_missing_one_on_any_path"
  "am106 drift: channel methods, dict and kernel builtins, text-rendering builtins|axon-core|lib|interp::taint_tests::every_channel_method_goes_through_the_one_access_helper interp::taint_tests::every_dict_builtin_has_a_taint_routing_row interp::taint_tests::every_kernel_builtin_is_a_tested_getter_or_stated_not_one interp::taint_tests::every_builtin_that_renders_a_value_to_text_is_a_stringifier_or_an_emitter"
  "am106 shared state and the existence oracle (runner leg)|axon-psv|sealed_frames|a_channel_the_candidate_touched_never_selects_operator_code the_taint_name_rule_refuses_what_the_static_name_analysis_lets_through a_sealed_caller_is_refused_in_the_same_words_for_an_operator_name_and_a_missing_one"
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

# Amendment 102, the sweep: every interpreter test that holds with the static pin analysis on
# holds with ONLY the runtime taint on (PSV1T_TAINT_ONLY=1 runs each rule-on test that way),
# except the two programs in which the OPERATOR alone is untyped and the static layer over-refuses
# by design (an unannotated lambda's result; the value of a handler expression). A third failure
# is a hole in the taint, or in the static layer, and fails the gate.
sweep="$(PSV1T_TAINT_ONLY=1 cargo test --locked -q -p axon-core --no-default-features --lib interp:: 2>&1)"
failed="$(printf '%s\n' "$sweep" | sed -n 's/^    \(interp::[A-Za-z0-9_:]*\)$/\1/p' | sort | tr '\n' ' ')"
expected="interp::tests::operator_code_never_dispatches_on_a_value_from_any_untyped_position interp::tests::the_value_of_a_with_handler_expression_is_undetermined_until_pinned "
if [ "$failed" = "$expected" ]; then
  echo "  OK   PCI am102 sweep: with only the taint on, exactly the two static-only programs differ"
else
  printf '%s\n' "$sweep" | tail -20
  echo "  FAIL PCI am102 sweep: failing set is [$failed], expected [$expected]"; fail=1
fi

if [ "$fail" -ne 0 ]; then echo "v022_pci_gates: FAIL"; exit 1; fi
echo "v022_pci_gates: PASS — ${#ROWS[@]} rows"
