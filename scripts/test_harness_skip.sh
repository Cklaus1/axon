#!/usr/bin/env bash
# test_harness_skip.sh — the shell skip detector (`harness_says_skip` in
# scripts/lib/harness_skip.sh) against the SAME rows as `skip_rule_table` in
# crates/axon-core/tests/cli_run.rs. A harness must not be a SKIP to one reader
# and a PASS to the other: add a row to both tables, or to neither.
# parity_all.sh runs this before any harness; it can also be run alone.
# Exit 0 every row held; 1 a row disagreed (the message names it).
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/lib/harness_skip.sh"

bad=0
# row <expect: skip|noskip> <label> <output (printf %b escapes)>
row() {
  local want="$1" label="$2" out got
  out="$(printf '%b' "$3")"
  if harness_says_skip "$out"; then got=skip; else got=noskip; fi
  if [ "$got" = "$want" ]; then
    echo "  ok    $label"
  else
    echo "  WRONG $label: expected $want, detector said $got"
    bad=1
  fi
}

# Explicit marker (what `harness_skip` emits).
row skip   "explicit marker"            'foo_parity: SKIP — native codegen unavailable\n'
# Elaboration first, marker last.
row skip   "elaboration then marker"    '  error: cannot codegen\nfoo_parity: SKIP — reason\n'
# Grandfathered prose followed by build-error text: THE disagreement case
# (handler_resume_parity / exit_code_parity's catch-all).
row skip   "prose skip then build error" 'foo_parity: native build failed — skipping\nerror: IR verification failed\n'
# A skip that elaborates at column 0 (all_examples_parity's shape).
row skip   "skip that elaborates"       'all_examples_parity: cannot codegen — skipping\nall_examples_parity: this is a SKIP, not a pass.\n'
# Per-case skips are INDENTED and never decide the verdict.
row noskip "indented case skip, PASS"   '  SKIP case_a (native build unavailable)\nfoo_parity: PASS — 12 cases\n'
row noskip "indented case skips, OK"    '  SKIP split(3): codegen unavailable\n  SKIP split(4): codegen unavailable\nfoo_parity: OK — done\n'
# A mid-run note is not a skipped harness.
row noskip "mid-run note then PASS"     'foo_parity: skipping 2 of 40 cases\nfoo_parity: PASS — 38 cases agree\n'
row noskip "plain PASS"                 'foo_parity: PASS — all good\n'
row noskip "empty output"               ''

if [ "$bad" -ne 0 ]; then
  echo "test_harness_skip: FAIL — harness_says_skip disagrees with the rule table"
  exit 1
fi
echo "test_harness_skip: PASS — harness_says_skip matches every rule-table row"
