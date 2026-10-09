#!/usr/bin/env bash
# vm_parity.sh — R50 §8 parity corpus: `axon run` under AXON_ENGINE=vm must be
# byte-identical to the reference tree-walker (AXON_ENGINE=tree).
#
# Binary: `cargo build --release -p axon-core --no-default-features --bin axon`
# (the §10 binary; the tree-walker runs qsort in ~6 s there against ~58 s in
# debug). Set AXON_BIN=<path> to test an already-built binary instead.
#
# Corpus: every `examples/**/*.ax` that defines `fn main`, plus every
# `crates/axon-core/tests/fixtures/**/*.ax` that defines `fn main` and that
# `axon check` accepts, minus the files listed (with a reason each) in
# crates/axon-core/tests/fixtures/vm_parity_skip.txt. An unlisted example that
# `axon check` rejects fails the gate.
#
# Per file, four runs, each in a fresh working directory (always the same path
# per file, recreated empty, so path strings agree between runs) with its own
# TMPDIR, XDG_CACHE_HOME (so its own provenance.jsonl) and AXON_AUDIT_LEDGER,
# AXON_LEARNER_STATE=learner.state, AXON_BANDIT_STATE=bandit.state (relative),
# stdin from /dev/null, AXON_AI_MOCK=1 AXON_SEED=42 AXON_CLOCK=0:1
# AXON_AUDIT_DETERMINISTIC=1, no --verbose, a 30 s wall-clock limit, and an
# absolute AXON_PATH (examples/stdlib:asi:modular:domain):
#   1. AXON_ENGINE=tree, 2. AXON_ENGINE=tree — must agree, else the file keeps
#      state the isolation does not reach (fail);
#   3. AXON_ENGINE=vm — diffed against run 1: stdout, stderr, exit code, audit
#      ledger and provenance.jsonl. The one normalisation: the `ts_ms` and
#      `run_id` keys are dropped from every provenance row (wall-clock values
#      the virtual clock does not reach);
#   4. AXON_ENGINE=vm AXON_VM_TRACE=1 — not diffed (trace lines go to stderr);
#      its `vm: ...` lines give the coverage counts and the lowered-list check.
# Any run hitting the time limit fails the gate.
#
# Lowered-list check: a `vm: tree-op <name> <Variant>[(<shape>)]` line whose
# `<Variant>[(<shape>)]` token is in the selected slice's lowered list (the
# table below) fails the gate. The check is per op, never a corpus total.
#
# Output: one line per failing file, then
#   vm_parity: <n> files, <c> bodies, <l> lowered ops, <t> tree ops, <d> differ
# Exit 0 iff no file differs, times out, is wrongly rejected by check, or hits
# a lowered-list violation; 1 otherwise; 2 on a setup error.
#
# Env:
#   AXON_BIN          binary under test (skips the build)
#   VM_PARITY_SLICE   which slice's lowered list to enforce (default: newest)
#   VM_PARITY_JOBS    parallel files (default: nproc)
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ── Lowered set per slice (R50 §4 "Lowered set per slice") ─────────────────
# Each slice that lands adds ONE row: the `tree-op` tokens its slice lowers,
# spelled exactly as the trace prints them (`<Variant>` for a whole Expr
# variant, `<Variant>(<shape>)` for a shape inside one), and the slices it
# depends on (§13 DAG). The list enforced for slice X is X's row plus the rows
# of all its ancestors. Shapes that stay `Tree` permanently (`Call(struct-lit)`,
# `Call(P)`, `Call(computed)`, `Index(E|Var)`) are never listed; a variant
# listed bare does not cover its shapes, because tokens compare exactly.
SLICES=(S0) # landing order; the last one is the default
declare -A SLICE_DEPS=(
  [S0]=""
)
declare -A SLICE_LOWERED=(
  [S0]="" # S0 lowers nothing: every compiled body is exactly one Tree op
)
# ────────────────────────────────────────────────────────────────────────────

SLICE="${VM_PARITY_SLICE:-${SLICES[-1]}}"
if [ -z "${SLICE_DEPS[$SLICE]+x}" ]; then
  echo "vm_parity: unknown VM_PARITY_SLICE '$SLICE' (known: ${SLICES[*]})" >&2
  exit 2
fi
declare -A LOWERED=()
collect_lowered() { # <slice> — add the slice's row and its ancestors' rows
  local s="$1" tok dep
  for tok in ${SLICE_LOWERED[$s]}; do LOWERED["$tok"]=1; done
  for dep in ${SLICE_DEPS[$s]}; do collect_lowered "$dep"; done
}
collect_lowered "$SLICE"

if [ -n "${AXON_BIN:-}" ]; then
  AXON="$AXON_BIN"
else
  echo "vm_parity: building axon (release, --no-default-features)…"
  cargo build -q --release -p axon-core --no-default-features --bin axon \
    || { echo "vm_parity: build failed" >&2; exit 2; }
  AXON="${CARGO_TARGET_DIR:-target}/release/axon"
fi
case "$AXON" in /*) ;; *) AXON="$ROOT/$AXON" ;; esac
[ -x "$AXON" ] || { echo "vm_parity: $AXON is not executable" >&2; exit 2; }

export AXON_PATH="$ROOT/examples/stdlib:$ROOT/examples/asi:$ROOT/examples/modular:$ROOT/examples/domain"

SKIP_FILE="$ROOT/crates/axon-core/tests/fixtures/vm_parity_skip.txt"
declare -A skip=()
while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in ''|'#'*) continue ;; esac
  read -r _path _reason <<<"$line"
  if [ -z "${_reason:-}" ]; then
    echo "vm_parity: $SKIP_FILE: entry '$_path' has no reason" >&2
    exit 2
  fi
  [ -f "$ROOT/$_path" ] || echo "vm_parity: warning: skip-listed $_path does not exist" >&2
  skip["$_path"]=1
done <"$SKIP_FILE"

has_main() { grep -qE '(^|[^[:alnum:]_])fn[[:space:]]+main[[:space:]]*\(' "$1"; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/res"

# Corpus: "<idx>\t<kind>\t<repo-relative path>" per line.
idx=0
nskip=0
{
  while IFS= read -r f; do
    has_main "$f" || continue
    if [ -n "${skip[$f]+x}" ]; then nskip=$((nskip + 1)); continue; fi
    idx=$((idx + 1)); printf '%d\texample\t%s\n' "$idx" "$f"
  done < <(find examples -name '*.ax' | LC_ALL=C sort)
  while IFS= read -r f; do
    has_main "$f" || continue
    if [ -n "${skip[$f]+x}" ]; then nskip=$((nskip + 1)); continue; fi
    idx=$((idx + 1)); printf '%d\tfixture\t%s\n' "$idx" "$f"
  done < <(find crates/axon-core/tests/fixtures -name '*.ax' | LC_ALL=C sort)
  echo "$nskip" >"$WORK/nskip"
} >"$WORK/corpus"

# provenance normalisation — the one rule (§8): drop `ts_ms` and `run_id` from
# every row. Every writer (interp/provenance.rs) emits `{"ts_ms":N,` first and
# `,"run_id":"…"` inside the row; a row shaped otherwise keeps its keys and so
# shows up as a difference instead of being hidden.
normalise_provenance() {
  sed -E 's/^\{"ts_ms":-?[0-9]+,/{/; s/,"run_id":"([^"\\]|\\.)*"//'
}

# run_once <src> <base> <tag> <engine> [trace] — one isolated run into <base>/<tag>.
run_once() {
  local src="$1" base="$2" tag="$3" engine="$4" trace="${5:-}"
  local run="$base/run" out="$base/$tag"
  rm -rf "$run" "$out"
  mkdir -p "$run/tmp" "$run/cache" "$out"
  local -a extra=()
  [ -n "$trace" ] && extra=(AXON_VM_TRACE=1)
  (
    cd "$run" &&
      env -u AXON_VM_TRACE -u AXON_DUMP_BINDINGS -u AXON_DUMP_SHAPES -u AXON_RECORD -u AXON_REPLAY \
        TMPDIR="$run/tmp" XDG_CACHE_HOME="$run/cache" AXON_AUDIT_LEDGER="$run/audit.ledger" \
        AXON_LEARNER_STATE=learner.state AXON_BANDIT_STATE=bandit.state \
        AXON_AI_MOCK=1 AXON_SEED=42 AXON_CLOCK=0:1 AXON_AUDIT_DETERMINISTIC=1 \
        AXON_ENGINE="$engine" "${extra[@]}" \
        timeout -k 5 30 "$AXON" run "$src" </dev/null >"$out/stdout" 2>"$out/stderr"
  )
  echo $? >"$out/exit"
  if [ -f "$run/audit.ledger" ]; then cp "$run/audit.ledger" "$out/ledger"; else echo "<absent>" >"$out/ledger"; fi
  if [ -f "$run/cache/axon/provenance.jsonl" ]; then
    normalise_provenance <"$run/cache/axon/provenance.jsonl" >"$out/provenance"
  else
    echo "<absent>" >"$out/provenance"
  fi
  rm -rf "$run"
}

timed_out() { case "$(cat "$1/exit")" in 124|137) return 0 ;; esac; return 1; }

# differs <a> <b> — names of the differing channels, empty when identical.
differs() {
  local ch d=""
  for ch in stdout stderr exit ledger provenance; do
    cmp -s "$1/$ch" "$2/$ch" || d="$d $ch"
  done
  echo "${d# }"
}

# first_diff <a> <b> <channels> — a short excerpt of the first differing channel.
first_diff() {
  local ch="${3%% *}"
  diff "$1/$ch" "$2/$ch" | head -8 | sed 's/^/      /'
}

# check_one "<idx>\t<kind>\t<rel>" — the four runs for one file; result to res/<idx>.
check_one() {
  local idx kind rel
  IFS=$'\t' read -r idx kind rel <<<"$1"
  local base="$WORK/f$idx" res="$WORK/res/$idx" src="$ROOT/$rel" d
  mkdir -p "$base"
  if ! (cd "$base" && timeout -k 5 60 "$AXON" check "$src" </dev/null >"$base/check" 2>&1); then
    if [ "$kind" = fixture ]; then
      echo "EXCL" >"$res"
    else
      { echo "FAIL check $rel: rejected by axon check (unlisted example)"; head -5 "$base/check" | sed 's/^/      /'; } >"$res"
    fi
    rm -rf "$base"; return
  fi
  run_once "$src" "$base" A tree
  if timed_out "$base/A"; then echo "FAIL timeout $rel: tree run hit the 30 s limit" >"$res"; rm -rf "$base"; return; fi
  run_once "$src" "$base" B tree
  if timed_out "$base/B"; then echo "FAIL timeout $rel: tree run hit the 30 s limit" >"$res"; rm -rf "$base"; return; fi
  d="$(differs "$base/A" "$base/B")"
  if [ -n "$d" ]; then
    { echo "FAIL differ $rel: the two tree runs differ ($d): state the isolation does not reach"
      first_diff "$base/A" "$base/B" "$d"; } >"$res"
    rm -rf "$base"; return
  fi
  run_once "$src" "$base" V vm
  if timed_out "$base/V"; then echo "FAIL timeout $rel: vm run hit the 30 s limit" >"$res"; rm -rf "$base"; return; fi
  d="$(differs "$base/A" "$base/V")"
  if [ -n "$d" ]; then
    { echo "FAIL differ $rel: tree and vm differ ($d)"; first_diff "$base/A" "$base/V" "$d"; } >"$res"
    rm -rf "$base"; return
  fi
  run_once "$src" "$base" T vm trace
  if timed_out "$base/T"; then echo "FAIL timeout $rel: vm trace run hit the 30 s limit" >"$res"; rm -rf "$base"; return; fi
  { echo "PASS"; grep '^vm: ' "$base/T/stderr"; } >"$res"
  rm -rf "$base"
}

export -f run_once timed_out differs first_diff check_one normalise_provenance
export WORK AXON ROOT

JOBS="${VM_PARITY_JOBS:-$(nproc)}"
ncorpus="$(wc -l <"$WORK/corpus")"
echo "vm_parity: slice $SLICE, $ncorpus candidate files ($(cat "$WORK/nskip") skip-listed), $JOBS jobs, binary $AXON"
xargs -d '\n' -n 1 -P "$JOBS" bash -c 'check_one "$1"' _ <"$WORK/corpus"

files=0; bodies=0; lowered=0; treeops=0; differ=0; other=0; excluded=0
while IFS=$'\t' read -r i kind rel; do
  res="$WORK/res/$i"
  if [ ! -f "$res" ]; then
    echo "FAIL $rel: no result (worker died)"; other=$((other + 1)); continue
  fi
  first="$(head -1 "$res")"
  case "$first" in
    EXCL) excluded=$((excluded + 1)); continue ;;
    "FAIL differ "*) files=$((files + 1)); differ=$((differ + 1)); cat "$res"; continue ;;
    FAIL*) files=$((files + 1)); other=$((other + 1)); cat "$res"; continue ;;
  esac
  files=$((files + 1))
  while IFS= read -r l; do
    if [[ "$l" =~ ^vm:\ ([^ ]+)\ ([0-9]+)\ ops,\ ([0-9]+)\ tree\ nodes$ ]]; then
      bodies=$((bodies + 1))
      lowered=$((lowered + BASH_REMATCH[2] - BASH_REMATCH[3]))
      treeops=$((treeops + BASH_REMATCH[3]))
    elif [[ "$l" =~ ^vm:\ tree-op\ ([^ ]+)\ ([^ ]+)$ ]]; then
      if [ -n "${LOWERED[${BASH_REMATCH[2]}]+x}" ]; then
        echo "FAIL lowered $rel: '$l' — ${BASH_REMATCH[2]} is lowered in $SLICE"
        other=$((other + 1))
      fi
    elif [[ "$l" =~ ^vm:\ tree\ \<anon\>:\  ]]; then
      : # code instances with compiled: None; not a body (§3)
    elif [[ "$l" =~ ^vm:\ tree\ ([^ ]+):\  ]]; then
      bodies=$((bodies + 1)) # whole-body exclusion
    fi
  done < <(tail -n +2 "$res")
done <"$WORK/corpus"

echo "vm_parity: $excluded fixtures not run (rejected by axon check)"
echo "vm_parity: $files files, $bodies bodies, $lowered lowered ops, $treeops tree ops, $differ differ"
if [ "$differ" -ne 0 ] || [ "$other" -ne 0 ]; then
  echo "vm_parity: FAILED — $differ differ, $other other failure(s) (timeouts, check rejections, lowered-list violations)"
  exit 1
fi
if [ "$files" -eq 0 ]; then
  echo "vm_parity: FAILED — no file ran"
  exit 1
fi
# Explicit verdict line: parity_all.sh globs scripts/*_parity.sh, so this
# script is also one of its harnesses and follows their final-line contract.
echo "vm_parity: PASS — tree and vm identical on every file (slice $SLICE lowered list honoured)"
exit 0
