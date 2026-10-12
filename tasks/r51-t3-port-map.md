# T3 port map (R51)

Which slice ports each v0.22-only test. Source: scout survey at r51/unify 56474dea against v0.22 c9ed2ddd; rows assigned by name only are marked [UNVERIFIED].

LEGEND
- slice: where spec §11 ports the mechanism.
- run@S6: the test's control is a sealed run, so it cannot pass before S6.
- diff: V = verbatim; M = mechanical rewrite (harness, Sym/Fields renames); R = redesign (sweep or table re-derived for main's model and vm/).
- H-seal: the shared sealed-run harness. It uses Program{items}, parse_source_in, intern_source, set_sealed_module_dirs and SEALED_DIRS_TEST_LOCK (all exist on r51), plus TestEnd/run_test_fn_outcome (S0b), DISPATCH_RULE_OFF (S4) and TAINT_FORCE_ON (S2), which do not exist on r51 yet.

A. LIB TESTS (144)

1. interp::conform::walk_bound_tests (1)
- a_value_nested_past_the_bound_is_refused_not_left_unvisited | conform.rs:1780 | S3 | Interp::walk_depth_ok, MAX_CAST_DEPTH | M | absent on r51

2. interp::taint_tests (39)
File interp/taint_tests.rs; none are on r51. Behavioural rows use H-seal with Rules Both/TaintOnly/Neither, which also pulls in S5 (pins) and S4 (rule statics).

Behavioural rows (all run@S6):
- an_operator_closure_the_candidate_picked_is_never_called (234) | S4 PICK | M
- a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn (305) | S2 | M
- an_operator_value_the_candidate_picked_is_never_dispatched_on (369) | S4, named in spec T5 | M
- a_type_or_width_sealed_code_chose_is_never_dispatched_on (424) | S2 | M
- taint_survives_every_carrier_a_value_can_travel_by (456) | S2 | carriers: Struct fields, closure captures, tuple, channel, dict | M
- bypass_shapes_hunted_after_the_first_cut (542) | S2 | M
- every_hook_of_the_taint_has_an_attack_of_its_own (623) | S2 | M
- the_remaining_routes_a_selection_can_take (630) | S2 | M
- a_channel_the_candidate_touched_never_selects_an_operator_closure_or_name (920) | S2 | M
- a_dict_reader_is_tainted_after_a_sealed_write_and_clean_after_the_operators (1088) | S2 | M
- a_kernel_getter_is_tainted_after_a_write_the_candidate_steered (1285) | S2, plus the S3 Kernel split | M
- a_shared_array_is_tainted_after_a_sealed_write (1357) | S2 | M
- areas_the_psv3_reviewer_did_not_hunt (1421) | S2 | M
- a_sealed_caller_cannot_tell_an_operator_name_from_a_missing_one_on_any_path (1531) | S3 no_such_fn | M
- a_sealed_caller_cannot_tell_an_operator_method_from_a_missing_one (1591) | S3 | M
- comparing_or_searching_a_dict_the_candidate_wrote_is_a_read_of_it (1690) | S2 | M
- every_builtin_that_can_read_a_container_of_a_tainted_dict_taints_its_result (1702) | S2 | M
- a_native_registry_a_sealed_frame_wrote_taints_what_the_operator_reads_back (1803) | S2 | M
- operator_side_control_flow_on_candidate_data_carries_its_taint_into_what_it_runs (2123) | S2 | M

Unsealed:
- an_ordinary_run_takes_no_taint (465) | S2 | V

Static and drift rows (not run@S6):
- every_builtin_has_a_taint_class (650) | S2 | taint::info / Class tables against BUILTINS (main's builtin union is larger) | M
- every_dict_builtin_is_a_listed_writer_or_a_reader_that_does_not_write (697) | S2 | M
- builtins_are_dispatched_only_where_the_taint_is_routed (736) | S2 | source sweep; must cover vm | R
- every_type_that_holds_a_value_keeps_its_taint (764) | S2 | its HOLDERS table names v0.22 types (Env vars tuple, HashMap captures/fields); must name Bindings, Fields, ClosureCode, vm frames | R
- only_fn_and_closure_frames_restore_the_control_taints (830) | S2 | R
- every_interp_file_is_read_by_the_drift_sweeps (865) | S2 | must cover sym.rs and vm/ but exclude vm/tests.rs | R
- every_channel_method_goes_through_the_one_access_helper (930) | S3 channel contracts | eval.rs text | R
- every_dict_builtin_has_a_taint_routing_row (1099) | S2 | M
- every_kernel_builtin_is_a_tested_getter_or_stated_not_one (1293) | S2 | M
- every_builtin_that_renders_a_value_to_text_is_a_stringifier_or_an_emitter (1431) | S2 | builtins.rs arm text | R
- every_comparison_and_every_builtin_argument_is_walked_deep (1754) | S2 | eval_binop / t_obj_deep text; main has binop_operands and vm | R
- every_conditional_evaluation_form_in_the_ast_has_a_row_with_an_attack (2134) | S2 | include_str of ast.rs Expr variants | M
- no_arm_of_a_pure_builtin_reads_or_advances_ambient_state (2427) | S2 | R
- the_durable_store_is_world_state_not_kernel_state (2521) | S2 | V
- every_builtin_that_takes_a_closure_has_a_callback_row_and_calls_through_call_cb (2543) | S2 | arr_fold VM entry | R
- every_rust_loop_that_runs_operator_code_raises_the_control_taint_after_each_run (2667) | S2 | goal.rs text | R
- every_kernel_builtin_that_draws_the_rng_is_coupled_to_the_world_cell (2722) | S2, plus S3 RNG | M/R
- the_holder_reasons_name_things_that_exist (2777) | S2 | R
- both_provenance_writes_of_a_zoned_call_are_marked (2818) | S2 | literal t_provenance_write text | M

3. interp::tests (96)
File interp.rs; none are on r51.

S0b (7):
- loop_control_does_not_escape_through_a_predicate (5602) | unsealed run_test_fn | V
- a_second_impl_never_replaces_the_first_impls_method (5653) | check_pipeline E0002 + run_test_fn | V | slice [UNVERIFIED]
- a_test_ended_by_question_mark_is_never_completed (~5940) | sealed_outcome + TestEnd::EndedEarly | M | run@S6
- a_test_completes_only_when_its_body_returns_normally (~7350) | run_test_fn_outcome | V
- no_control_transfer_escapes_a_frame (7227) | contain_frame | V
- an_escaped_break_or_continue_does_not_pass_a_test | V
- an_escaped_break_or_continue_from_a_function_body_does_not_pass_a_test | V

S5 (14), pin analysis and name analysis:
- a_mut_operand_is_never_determined_by_the_pin_analysis (~9548) | Pins::build, walk_expr, FnDef-pointer keys | M
- a_mut_operand_is_open_in_the_dispatch_analysis | M
- identical_site_text_in_two_fns_gets_two_verdicts (10929) | M
- the_value_of_a_with_handler_expression_is_undetermined_until_pinned | judged_on | M | run@S6
- a_local_binding_named_like_an_operator_fn_is_not_judged_by_it | M | run@S6
- a_call_through_a_local_never_resolves_the_name_elsewhere | M | run@S6
- a_function_name_the_candidate_chose_never_selects_an_operator_fn | M | run@S6
- goal_eval_takes_no_function_name_the_candidate_chose | M | run@S6
- scheduler_spawn_takes_no_function_name_the_candidate_chose | M | run@S6
- a_goal_constraint_and_a_kernel_goal_take_no_function_name_the_candidate_chose | M | run@S6
- a_candidates_global_and_a_sibling_fns_site_determine_nothing | M
- a_verdict_belongs_to_its_own_fn_and_the_candidate_steers_none | M | run@S6
- a_trait_annotation_does_not_pin_the_runtime_type | M | run@S6
- every_builtin_that_runs_user_code_is_classified (~9842) | the pin.rs Disp TABLE against BUILTINS | M/R
No row was read individually, so each S5 assignment is [UNVERIFIED].

S4 (14), receiver dispatch rule and pin owner; all run@S6, all M:
- a_determined_receiver_dispatches_and_so_does_an_unambiguous_method
- a_type_the_candidate_declared_does_not_determine_the_receiver
- operator_code_never_dispatches_on_a_value_from_any_untyped_position
- operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict
- interpolation_and_comparison_of_an_untyped_read_select_no_operator_impl
- an_operator_method_named_like_a_channel_method_does_not_determine_a_recv
- operator_arithmetic_never_runs_at_a_width_the_candidate_chose
- operator_unary_arithmetic_never_runs_at_a_width_the_candidate_chose
- operator_only_polymorphism_dispatches_and_a_candidate_influenced_one_does_not
- a_candidates_method_never_runs_under_the_operators_method_name
- a_handler_arm_dispatch_runs_under_the_installers_pin_owner (10841) | also needs the S3 handler work
- a_handler_arm_replay_runs_under_the_installers_pin_owner (10869)
- a_handler_arm_arithmetic_runs_under_the_installers_pin_owner (10906)
- a_closure_made_in_a_handler_arm_is_judged_by_the_installing_fn (10882) | v0.22 stores PIN_FN_MARK as a key in the HashMap closure capture; main captures as Vec<(Sym,Value)>, so this needs the Fork 3 mark representation

S3 (61). Cast and crossing rows use judged / judged8 / live8 / honest8 over Axon source; all are M and run@S6:
- a_candidate_fn_value_the_operator_calls_still_cannot_reach_operator_fns
- a_candidates_own_fn_value_takes_arguments_and_an_operators_still_does_not
- a_channel_carries_the_element_type_its_creation_states
- a_channel_returned_at_an_undetermined_element_type_is_refused
- a_closure_that_captured_the_operators_dict_cannot_retype_after_an_operator_write
- a_closures_confused_argument_never_crosses_its_declared_type
- a_closures_confused_result_never_crosses_its_declared_type
- a_confused_argument_never_enters_a_declared_parameter
- a_confused_element_never_crosses_inside_a_tuple
- a_confused_element_never_crosses_inside_an_array
- a_confused_enum_never_crosses_as_another_enum
- a_confused_field_is_refused_at_construction
- a_confused_field_never_crosses_inside_a_struct
- a_confused_payload_never_crosses_inside_an_option
- a_confused_scalar_never_crosses_a_declared_return
- a_confused_struct_never_crosses_as_another_struct
- a_confused_value_is_never_sent_on_a_declared_channel
- a_declared_result_fn_never_returns_an_option
- a_declared_soft_type_casts_its_wrapper_and_its_inner_value
- a_dict_over_the_snapshot_bound_is_refused_not_skipped
- a_dict_the_candidate_adds_to_or_builds_still_crosses
- a_dict_the_candidate_mutated_is_verified_at_every_edge_back
- a_fn_with_no_declared_return_type_hands_the_operator_unit
- a_generic_struct_or_enum_argument_binds_its_type_parameter
- a_handle_binding_admits_only_that_handle
- a_host_await_crossing_is_unavailable_inside_a_sealed_test_run
- a_lambdas_annotated_parameter_is_cast
- a_let_annotation_is_cast
- a_mut_value_is_cast_when_an_operator_handler_aborts_the_call
- a_mut_write_through_value_is_cast_at_the_seal_edge_back
- a_mut_write_through_value_is_judged_by_what_the_operator_held
- a_non_closure_never_crosses_a_declared_fn_type
- a_placeholder_the_operator_held_is_not_filled_by_the_candidate
- a_position_the_operator_held_is_judged_by_what_it_held_when_replaced
- a_soft_wrapper_takes_the_declared_plain_type
- a_type_parameter_inside_any_shape_is_never_filled_by_the_candidate
- a_type_parameters_trait_bound_is_cast
- a_value_at_an_undetermined_position_never_crosses_a_seal
- a_value_at_an_undetermined_type_parameter_never_crosses_the_seal
- a_value_of_another_fixed_width_never_crosses_a_declared_fixed_width
- a_value_of_another_integer_width_never_crosses_a_declared_integer
- an_annotated_operator_array_lent_as_mut_is_still_cast_at_the_edge_back
- an_i64_at_a_declared_fixed_width_takes_the_width
- an_operator_closure_called_from_sealed_code_takes_only_determined_arguments
- every_declared_type_refuses_a_value_of_another_type
- the_remaining_cast_arms_refuse_a_value_of_another_type
- width_correct_values_cross_unchanged
- sealed_code_cannot_retype_a_dict_entry_the_operator_held
- sandbox_run_results_are_cast_at_the_seal_crossing

S3 seal-edge and frame rows (M, run@S6):
- a_sealed_caller_cannot_tell_an_operator_fn_from_a_missing_one
- a_sealed_fiber_never_runs_an_operator_function
- a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path | global_ref against main's slotted globals
- a_sealed_frame_cannot_take_an_operator_fn_as_a_value
- a_sealed_handler_never_answers_or_aborts_operator_code
- a_sealed_send_on_an_operator_channel_needs_a_determined_element_type
- operator_code_a_sealed_frame_runs_never_draws_the_operator_rng
- runtime_sealing_holds_without_the_static_check

S3 drift rows (R, not run@S6):
- every_frame_that_runs_stored_operator_code_sets_its_owner | must add the vm call paths
- every_global_read_goes_through_global_ref | must sweep vm/
- every_name_resolving_lookup_is_a_listed_sink | must list fn_of_sym, fn_of_def, fn_table, callees
- the_sweep_routes_stay_closed

4. Non-interp lib tests (8)
All are on r51 and not ignored; whether they pass there is [UNVERIFIED]. All V.
- kernel::tests::a_sealed_registry_drawing_tokens_does_not_move_the_operators_stream | kernel.rs:802
- parser::tests::an_item_that_ends_the_file_has_a_real_span_in_its_own_file | parser.rs:3574
- resolver::tests::duplicate_let_refinement_or_impl_produces_e0002 | resolver.rs:3320
- resolver::tests::two_files_inline_refinements_never_share_a_name | resolver.rs:3407
- resolver::tests::a_diagnostic_about_a_sealed_item_names_the_sealed_file_wherever_the_item_sits | resolver.rs:3447
- resolver::tests::a_sealed_module_cannot_reach_the_operators_names | resolver.rs:3499
- resolver::tests::a_sealed_module_cannot_name_the_operators_types_or_traits | resolver.rs:3542
- seal_split::tests::a_spanless_merged_diagnostic_is_not_shown_beside_the_sealed_ones | seal_split.rs:55

B. INTEGRATION FILES
All 10 are present on r51.
- axon_test_outcomes.rs: 4 tests, none ignored.
- dispatch_rule_switch_is_test_only.rs: its one test is ignored until S4.
- harness_binaries.rs, harness_integrity.rs, pci_delta_note.rs, pci_isolation.rs, postc9_spec.rs: no ignores seen.
- psv_test_selection.rs: 2 tests ignored until S0b, 3 until S3 (RNG), at least 6 until S6.
- refusal_coverage_gate.rs: about 50+ tests ignored until S3 (conform.rs).
- test_completion.rs: one test ignored until S0b.

C. DEFERRED REGISTRY (192)
Every entry is already on r51 as an ignored test, and un-ignoring one is a verbatim change (V).
- S0b (18): 2 in cli_run (Uncertain source_tag); 2 in psv_test_selection; 1 in test_completion; 1 in fabric attestation; 4 in fabric check_effects; 3 in fabric psv_dispatch; 1 in fabric workspace; 3 in psv runner; 1 in psv sealed_frames.
- S5 (2): psv sealed_frames operator_code_never_dispatches_on_an_untyped_read_and_a_pinned_suite_passes and sandbox_run_runs_no_function_name_the_candidate_chose.
- S2 (8): all in psv sealed_frames (clock, name bit, dict compare, negation width, control flow, carriers, width/name, taint name rule).
- S3 (78): 3 RNG tests in psv_test_selection; refusal_coverage_gate (the bulk); 4 in fabric check_effects; 2 in psv rng_isolation; 14 in psv sealed_frames, including the T4 oracle.
- S4 (6): the dispatch-rule switch test plus 5 in psv sealed_frames (PICK and receiver rule).
- S6 (80): psv_test_selection, plus fabric attestation, branches, check_effects and psv_dispatch, plus psv runner and sealed_frames. Each needs only the S6 flip.

D. COUNTS
Lib tests:
- S0 already present: 8
- S0b: 7
- S5: 14
- S2: 34
- S3: 65 (61 interp::tests, 3 taint_tests, 1 conform)
- S4: 16 (14 interp::tests, 2 taint_tests)
- Total: 144
Roughly 108 of these are run@S6 [approximate]. Difficulty is roughly V 10, M 105, R 29.
Registry: S0b 18, S5 2, S2 8, S3 78, S4 6, S6 80 (total 192).

E. RISKS
- The interp::tests rows I did not read individually rest on [UNVERIFIED] assignments; the S5/S4/S2 boundaries are the soft spots.
- a_second_impl_never_replaces_the_first_impls_method may already pass on main.
- None of the 8 present tests were run.
- The exact refusal_coverage_gate and S6 membership lives in registry JSON lines 200-540 and 750-1150.