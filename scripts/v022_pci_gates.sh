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
  "21 sealed candidate|axon-fabric|check_effects|a_sealed_candidate_cannot_reach_the_operators_names"
)

for row in "${ROWS[@]}"; do
  IFS='|' read -r sid pkg target names <<<"$row"
  # axon-core is exercised on the interpreter build, never with codegen.
  feat=(); [ "$pkg" = axon-core ] && feat=(--no-default-features)
  want=0
  for n in $names; do
    want=$((want + 1))
    if [ "$target" = lib ]; then
      mod="${n%%::*}"; fn="${n##*::}"
      f="crates/$pkg/src/$mod.rs"
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
