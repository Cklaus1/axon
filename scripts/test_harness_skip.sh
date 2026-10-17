#!/usr/bin/env bash
# test_harness_skip.sh — the shell skip detector (`harness_says_skip` in
# scripts/lib/harness_skip.sh) against the SAME rows as `skip_rule_table` in
# crates/axon-core/tests/cli_run.rs. A harness must not be a SKIP to one reader
# and a PASS to the other: add a row to both tables, or to neither. The two
# tables differ only by the REASON column here: every skip row also pins the
# line `harness_skip_reason` reports (the one parity_all.sh prints). cli_run.rs
# prints no reasons, so that column is shell-only.
# parity_all.sh runs this before any harness; it can also be run alone.
# Exit 0 every row held; 1 a row disagreed (the message names it).
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/lib/harness_skip.sh"

bad=0
# row <expect: skip|noskip> <label> <output (printf %b escapes)> [<reason>]
#   <reason> (skip rows only, required there): the exact line
#   `harness_skip_reason` must return for <output>.
row() {
  local want="$1" label="$2" out got reason
  out="$(printf '%b' "$3")"
  if harness_says_skip "$out"; then got=skip; else got=noskip; fi
  if [ "$got" != "$want" ]; then
    echo "  WRONG $label: expected $want, detector said $got"
    bad=1
    return
  fi
  if [ "$want" = skip ]; then
    if [ "$#" -lt 4 ]; then
      echo "  WRONG $label: a skip row must state its reason line"
      bad=1
      return
    fi
    reason="$(harness_skip_reason "$out")"
    if [ "$reason" != "$4" ]; then
      echo "  WRONG $label: reason expected '$4', harness_skip_reason said '$reason'"
      bad=1
      return
    fi
  fi
  echo "  ok    $label"
}

# Explicit marker (what `harness_skip` emits).
row skip   "explicit marker"            'foo_parity: SKIP — native codegen unavailable\n' \
  'foo_parity: SKIP — native codegen unavailable'
# Elaboration first, marker last.
row skip   "elaboration then marker"    '  error: cannot codegen\nfoo_parity: SKIP — reason\n' \
  'foo_parity: SKIP — reason'
# Grandfathered prose followed by build-error text: THE disagreement case
# (handler_resume_parity / exit_code_parity's catch-all). The reason is the
# skip line, not the build error that follows it.
row skip   "prose skip then build error" 'foo_parity: native build failed — skipping\nerror: IR verification failed\n' \
  'foo_parity: native build failed — skipping'
# A skip that elaborates at column 0 (all_examples_parity's shape). The final
# line decides, so it is the reason (canon: the rule reports the line that
# decided, not the more specific one above it).
row skip   "skip that elaborates"       'all_examples_parity: cannot codegen — skipping\nall_examples_parity: this is a SKIP, not a pass.\n' \
  'all_examples_parity: this is a SKIP, not a pass.'
# Per-case skips are INDENTED and never decide the verdict.
row noskip "indented case skip, PASS"   '  SKIP case_a (native build unavailable)\nfoo_parity: PASS — 12 cases\n'
row noskip "indented case skips, OK"    '  SKIP split(3): codegen unavailable\n  SKIP split(4): codegen unavailable\nfoo_parity: OK — done\n'
# A mid-run note is not a skipped harness.
row noskip "mid-run note then PASS"     'foo_parity: skipping 2 of 40 cases\nfoo_parity: PASS — 38 cases agree\n'
row noskip "plain PASS"                 'foo_parity: PASS — all good\n'
row noskip "empty output"               ''
# The window BOUND: a column-0 skip line four non-empty lines from the end.
row noskip "skip outside the window"    'foo_parity: native build failed — skipping\nfoo_parity: note a\nfoo_parity: note b\nfoo_parity: note c\n'
# A grandfathered prose skip as the FINAL line.
row skip   "final-line prose skip"      'foo_parity: codegen build unavailable (LLVM absent) — skipping\n' \
  'foo_parity: codegen build unavailable (LLVM absent) — skipping'

if [ "$bad" -ne 0 ]; then
  echo "test_harness_skip: FAIL — the skip detector or its reason disagrees with the rule table"
  exit 1
fi
echo "test_harness_skip: PASS — harness_says_skip and harness_skip_reason match every rule-table row"
