#!/usr/bin/env bash
# vm_perf_gate.sh — R50 §10 performance budget for `axon run` under AXON_ENGINE=vm.
#
# Binary: `cargo build --release -p axon-core --no-default-features --bin axon`
# (workspace release profile). Set AXON_BIN=<path> to use an already-built one.
#
# Every measurement runs the program with AXON_ENGINE=vm, stdin /dev/null,
# under `perf stat -e instructions:u` pinned with `taskset -c $VM_PERF_CPU`
# (default 6), three times; the gate uses the median of the three counts (perf's
# own `-r 3` reports a mean, so the three runs are separate perf invocations).
# Every run's stdout must equal the program's golden
# (crates/axon-core/tests/fixtures/vm_perf/<prog>.golden).
#
# Modes:
#   scripts/vm_perf_gate.sh
#       The five compilebench programs (verbatim copies of compilebench's
#       benchmarks/<b>/axon/main.ax in tests/fixtures/vm_perf/). Budget: the
#       CPython 3.14.4 median from compilebench run 20261008T142344Z. A median
#       passes when it is at or below the budget; no rounding.
#
#   scripts/vm_perf_gate.sh --repros [SEL]
#       Per-construct repro rows. SEL is a comma-separated list of slice names
#       and/or row names; a slice name selects every row of that slice. Without
#       SEL, every row runs. Examples (the §13 DAG gates):
#         --repros S1         S1 gate: loop, call
#         --repros S1,part    S2 gate: S1 rows plus part
#         --repros            S5 gate: all rows
#       Rows (cost = (prog - base) / units; `loop` has no base):
#         row       slice  prog      base       units  budget
#         loop      S1     loop      -          1M     300   per iteration
#         call      S1     call1     loop       1M     800   one-arg call via dispatch_call -> call_fn_in
#         part      S2     part      part_base  1M     250   per partition iteration
#         fold      S4     fold      fold_base  10M    350   per element, builtin -> closure call
#         fastcall  S5     call1     loop       1M     300   one-arg fast call
#         mutcall   S5     mutcall   loop       1M     600   one-&mut-arg call via call_mut
#       A row passes when (prog - base) <= budget * units, compared exactly.
#
# Exit: 0 every selected median within budget and every output golden;
#       1 any median over budget or any output differing;
#       2 "not measured": perf cannot count instructions:u on that CPU (or the
#         binary could not be built/run). That is a FAILURE, never a skip.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

FIX="$ROOT/crates/axon-core/tests/fixtures/vm_perf"
CPU="${VM_PERF_CPU:-6}"

# ── Budgets (R50 §10, exact) ────────────────────────────────────────────────
# program        CPython 3.14.4 median, run 20261008T142344Z
PROGRAMS=(
  "fib-recursive 3423642492"
  "collatz       26058032109"
  "mandelbrot    15023124683"
  "arr-sum       23624147901"
  "qsort         18967575334"
)
# row       slice prog     base       units     budget
REPROS=(
  "loop      S1    loop     -          1000000   300"
  "call      S1    call1    loop       1000000   800"
  "part      S2    part     part_base  1000000   250"
  "fold      S4    fold     fold_base  10000000  350"
  "fastcall  S5    call1    loop       1000000   300"
  "mutcall   S5    mutcall  loop       1000000   600"
)
# ────────────────────────────────────────────────────────────────────────────

MODE=programs
SEL=""
while [ $# -gt 0 ]; do
  case "$1" in
    --repros)
      MODE=repros
      if [ $# -gt 1 ] && [[ "$2" != --* ]]; then SEL="$2"; shift; fi ;;
    --repros=*) MODE=repros; SEL="${1#--repros=}" ;;
    -h|--help) sed -n '2,45p' "$0"; exit 0 ;;
    *) echo "vm_perf_gate: unknown argument $1" >&2; exit 2 ;;
  esac
  shift
done

not_measured() {
  echo "vm_perf_gate: not measured — $*" >&2
  echo "vm_perf_gate: FAILED (not measured; this is a failure, not a skip)"
  exit 2
}

command -v perf >/dev/null 2>&1 || not_measured "perf is not installed"
command -v taskset >/dev/null 2>&1 || not_measured "taskset is not installed"

if [ -n "${AXON_BIN:-}" ]; then
  AXON="$AXON_BIN"
else
  echo "vm_perf_gate: building axon (release, --no-default-features)…"
  cargo build -q --release -p axon-core --no-default-features --bin axon \
    || not_measured "release build failed"
  AXON="${CARGO_TARGET_DIR:-target}/release/axon"
fi
case "$AXON" in /*) ;; *) AXON="$ROOT/$AXON" ;; esac
[ -x "$AXON" ] || not_measured "$AXON is not executable"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# perf_count <out-prefix> <cmd...> — run once under perf; print the count, or
# nothing when perf did not count.
perf_count() {
  local prefix="$1"; shift
  perf stat -x, -e instructions:u -o "$prefix.perf" -- taskset -c "$CPU" "$@" \
    </dev/null >"$prefix.out" 2>"$prefix.err"
  echo $? >"$prefix.rc"
  awk -F, '$3 ~ /^instructions(:u)?$/ && $1 ~ /^[0-9]+$/ { print $1; exit }' "$prefix.perf"
}

# Probe: perf must count instructions:u on this CPU before anything is gated.
probe="$(perf_count "$WORK/probe" true)"
if [ -z "$probe" ] || [ "$(cat "$WORK/probe.rc")" != 0 ]; then
  not_measured "perf cannot count instructions:u on CPU $CPU: $(grep -v '^#' "$WORK/probe.perf" 2>/dev/null | head -2 | tr '\n' ' ')$(head -c 200 "$WORK/probe.err")"
fi

fails=0
declare -A MEDIAN=()

# measure <prog> — three runs; sets MEDIAN[prog]; counts golden mismatches.
measure() {
  local prog="$1" k c counts=() golden="$FIX/$1.golden"
  [ -n "${MEDIAN[$prog]+x}" ] && return 0
  [ -f "$FIX/$prog.ax" ] && [ -f "$golden" ] || not_measured "missing $FIX/$prog.ax or its golden"
  for k in 1 2 3; do
    c="$(AXON_ENGINE=vm perf_count "$WORK/$prog.$k" "$AXON" run "$FIX/$prog.ax")"
    [ -n "$c" ] || not_measured "perf did not count $prog (run $k): $(grep -v '^#' "$WORK/$prog.$k.perf" | head -2 | tr '\n' ' ')"
    if [ "$(cat "$WORK/$prog.$k.rc")" != 0 ] || ! cmp -s "$WORK/$prog.$k.out" "$golden"; then
      echo "vm_perf_gate: $prog run $k: exit $(cat "$WORK/$prog.$k.rc"), output differs from $prog.golden"
      diff "$golden" "$WORK/$prog.$k.out" | head -5 | sed 's/^/      /'
      head -5 "$WORK/$prog.$k.err" | sed 's/^/      stderr: /'
      fails=$((fails + 1))
    fi
    counts+=("$c")
  done
  MEDIAN[$prog]="$(printf '%s\n' "${counts[@]}" | sort -n | sed -n 2p)"
  echo "vm_perf_gate: measured $prog: ${counts[*]} -> median ${MEDIAN[$prog]}"
}

group() { # 1234567 -> 1,234,567
  echo "$1" | sed -E ':a; s/^(-?[0-9]+)([0-9]{3})/\1,\2/; ta'
}

if [ "$MODE" = programs ]; then
  for row in "${PROGRAMS[@]}"; do
    read -r prog budget <<<"$row"
    measure "$prog"
    m="${MEDIAN[$prog]}"
    if [ "$m" -le "$budget" ]; then v="PASS"; else v="OVER"; fails=$((fails + 1)); fi
    printf 'vm_perf_gate: %-14s median %16s  budget %16s  %s\n' "$prog" "$(group "$m")" "$(group "$budget")" "$v"
  done
else
  declare -A want=()
  if [ -z "$SEL" ]; then
    for r in "${REPROS[@]}"; do read -r name _ <<<"$r"; want[$name]=1; done
  else
    IFS=, read -ra toks <<<"$SEL"
    for t in "${toks[@]}"; do
      hit=0
      for r in "${REPROS[@]}"; do
        read -r name slice _ <<<"$r"
        if [ "$t" = "$name" ] || [ "$t" = "$slice" ]; then want[$name]=1; hit=1; fi
      done
      if [ "$hit" = 0 ]; then
        echo "vm_perf_gate: --repros: '$t' is neither a row (loop call part fold fastcall mutcall) nor a slice with rows (S1 S2 S4 S5)" >&2
        exit 2
      fi
    done
  fi
  for r in "${REPROS[@]}"; do
    read -r name slice prog base units budget <<<"$r"
    [ -n "${want[$name]+x}" ] || continue
    measure "$prog"
    delta="${MEDIAN[$prog]}"
    if [ "$base" != - ]; then
      measure "$base"
      delta=$((delta - MEDIAN[$base]))
    fi
    cost="$(awk -v d="$delta" -v u="$units" 'BEGIN { printf "%.1f", d / u }')"
    if [ "$delta" -le $((budget * units)) ]; then v="PASS"; else v="OVER"; fails=$((fails + 1)); fi
    printf 'vm_perf_gate: repro %-8s (%s) %-8s - %-9s / %-8s = %9s per unit  budget %4s  %s\n' \
      "$name" "$slice" "$prog" "$base" "$units" "$cost" "$budget" "$v"
  done
fi

if [ "$fails" -ne 0 ]; then
  echo "vm_perf_gate: FAILED — $fails over budget or output mismatch"
  exit 1
fi
echo "vm_perf_gate: PASS"
exit 0
