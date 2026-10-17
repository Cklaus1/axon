#!/usr/bin/env bash
# test_harness_skip.sh — the shell skip detector (`harness_says_skip` in
# scripts/lib/harness_skip.sh) against the SAME rows as `skip_rule_table` in
# crates/axon-core/tests/cli_run.rs. A harness must not be a SKIP to one reader
# and a PASS to the other: add a row to both tables, or to neither. The two
# tables differ only by the REASON column here: every skip row also pins the
# line `harness_skip_reason` reports (the one parity_all.sh prints). cli_run.rs
# prints no reasons, so that column is shell-only.
# A second section, `wasm`, holds shell-only rows for the wasm link classifier
# (`wasm_link_unavailable`, `wasm_case`). A third, `shape` (below), greps
# scripts/*.sh for shapes the harness rules refuse. Each has its own FAIL line.
# parity_all.sh runs this before any harness, and gate.sh's fast tier runs it;
# it can also be run alone. Exit 0 every row and shape held; 1 otherwise (the
# message names the row and the section).
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

# ── wasm: the `axon target build --target wasm32-*` outcome ─────────────────
# Shell-only (no build): `wasm_link_unavailable` is true ONLY on the
# compiler's `wasm link unavailable:` marker, and `wasm_case` classifies one
# case against scripts/wasm_object_only.txt. Its own FAIL line.
wasm_bad=0
# wrow <expect: true|false> <label> <stderr (printf %b escapes)>
wrow() {
  local got=false
  wasm_link_unavailable "$(printf '%b' "$3")" && got=true
  if [ "$got" = "$1" ]; then echo "  ok    $2"; else echo "  WRONG $2: wasm_link_unavailable said $got"; wasm_bad=1; fi
}
wrow true  "unavailable marker"   'wasm link unavailable: rust-lld not found (searched /h/.rustup/toolchains/*/lib/rustlib/*/bin/rust-lld)\nwasm object: p.wasm (target wasm32-wasip1) — ...\n'
wrow false "runtime not found"    'wasm runtime not found: libaxon_rt.a for wasm32-wasip1 (searched /t/wasm32-wasip1/{debug,release})\nwasm link failed (exit status: 1):\nrust-lld: error: p.wasm: undefined symbol: __axon_rt_seed_rng\n'
wrow false "link failed"          'wasm link failed (exit status: 1):\nrust-lld: error: p.wasm: undefined symbol: foo\n'
wrow false "empty stderr"         ''
wrow false "marker text not at column 0" '  note: wasm link unavailable: quoted\n'
# crow <expect WASM_CASE> <label> <rc> <stderr> <linked: yes|no> — the case
# key is never listed (`gaps2:unlisted`), so object-only is a FAIL.
crow() {
  local linked=/nonexistent/x.linked.wasm
  [ "$5" = yes ] && linked="$HERE/test_harness_skip.sh"
  wasm_case wasm32-wasip1 gaps2:unlisted "$3" "$(printf '%b' "$4")" "$linked" >/dev/null
  if [ "$WASM_CASE" = "$1" ]; then echo "  ok    $2"; else echo "  WRONG $2: wasm_case said $WASM_CASE, expected $1"; wasm_bad=1; fi
}
crow linked  "case: linked"                       0 '' yes
crow skip    "case: unavailable marker"           0 'wasm link unavailable: rustup toolchains not found (searched /x)\n' no
crow fail    "case: runtime not found"            0 'wasm runtime not found: libaxon_rt.a for wasm32-wasip1 (searched /x)\n' no
crow fail    "case: unlisted object-only"         0 'wasm link failed (exit status: 1):\n' no
crow refused "case: E0910 refusal"                2 'codegen error [E0910]: builtin `http_get` is not yet supported\nerror: 1 codegen error(s); wasm build aborted\n' no
crow fail    "case: other build error"            2 'error: IR verification failed\n' no

# ── shape: script shapes the harness rules refuse ───────────────────────────
# Not rule-table rows (cli_run.rs has no counterpart): a grep over the scripts
# for a shape that once let a harness misbehave. Comment lines are ignored,
# and this file is not scanned (its own patterns would match themselves).
shape_bad=0
# shape <label> <ERE> <files...> — no non-comment line of <files> matches <ERE>.
shape() {
  local label="$1" re="$2" hits
  shift 2
  hits="$(grep -HnE -- "$re" "$@" 2>/dev/null | grep -vE '^[^:]+:[0-9]+:[[:space:]]*#')"
  if [ -n "$hits" ]; then
    echo "  WRONG $label:"
    printf '%s\n' "$hits" | sed "s|^$HERE/|        scripts/|"
    shape_bad=1
  else
    echo "  ok    $label"
  fi
}
scripts_sh=()
for f in "$HERE"/*.sh; do
  [ "$f" = "$HERE/test_harness_skip.sh" ] || scripts_sh+=("$f")
done
# H1: a relative CARGO_TARGET_DIR resolves against the script's `cd "$ROOT"`
# and overrides the caller's target dir (a second full build inside the
# worktree). Use `interp_target_dir` / `cargo_target_dir` (lib/axon_bin.sh) or
# an absolute `$WORK/...`.
shape "no relative CARGO_TARGET_DIR= literal in scripts/*.sh" \
  "CARGO_TARGET_DIR=[\"']?[^\"'\$/[:space:]]" "${scripts_sh[@]}"
# H4: a `cargo build … 2>/dev/null` in a parity harness throws away the
# compiler's own error and leaves only a "build failed"/"skipping" line. Use
# `cargo_build_or_skip <name> <prereq> cargo build …` (lib/harness_skip.sh),
# which proves a skip's reason and prints the stderr on a FAIL. Exceptions: none.
parity_sh=()
for f in "$HERE"/*_parity.sh; do parity_sh+=("$f"); done
shape "no \`cargo build … 2>/dev/null\` in scripts/*_parity.sh" \
  'cargo build.*2>/dev/null' "${parity_sh[@]}"

if [ "$bad" -ne 0 ]; then
  echo "test_harness_skip: FAIL [detector] — the skip detector or its reason disagrees with the rule table"
fi
if [ "$wasm_bad" -ne 0 ]; then
  echo "test_harness_skip: FAIL [wasm] — the wasm link classifier disagrees with its rows"
fi
if [ "$shape_bad" -ne 0 ]; then
  echo "test_harness_skip: FAIL [shape] — a script has a shape the harness rules refuse"
fi
[ "$bad" -eq 0 ] && [ "$wasm_bad" -eq 0 ] && [ "$shape_bad" -eq 0 ] || exit 1
echo "test_harness_skip: PASS — harness_says_skip and harness_skip_reason match every rule-table row; the wasm link classifier matches its rows; no refused script shape"
