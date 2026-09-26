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
)

for row in "${GATES[@]}"; do
  IFS='|' read -r gid pkg target names <<<"$row"
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
