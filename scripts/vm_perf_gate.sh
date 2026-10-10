#!/usr/bin/env bash
# vm_perf_gate.sh — R50 §10 performance budget for `axon run` under AXON_ENGINE=vm.
#
# Binary: `cargo build --release -p axon-core --no-default-features --bin axon`
# (workspace release profile). Set AXON_BIN=<path> to use an already-built one.
#
# Every measurement runs the program with AXON_ENGINE=vm (the default deferred
# compile: AXON_VM_EAGER and AXON_VM_TRACE are unset, S9), stdin /dev/null,
# under `perf stat -e instructions:u` pinned with `taskset -c $VM_PERF_CPU`
# (default 6), three times; the gate uses the median of the three counts (perf's
# own `-r 3` reports a mean, so the three runs are separate perf invocations).
# Every run's stdout must equal the program's golden
# (crates/axon-core/tests/fixtures/vm_perf/<prog>.golden).
#
# Modes:
#   scripts/vm_perf_gate.sh [--programs SEL]
#       Six compilebench programs (verbatim copies of compilebench's
#       benchmarks/<b>/axon/main.ax in tests/fixtures/vm_perf/). Budget: the
#       CPython 3.14.4 median from compilebench run 20261008T142344Z, except
#       sieve (S10), whose CPython is a C-speed slice assignment: half the S9
#       VM's median from run 20261010T015516Z (Axon 5864c423). A median
#       passes when it is at or below the budget; no rounding. SEL is a
#       comma-separated subset of the programs; without it, all six run.
#       Relative check (always on, R50 §10 S11 "every other program within
#       0.5 % of S10", §4 S12 "fib-recursive, arr-sum and qsort within 0.5 %
#       of the commit S12 is built on"): a program with a REF median (third
#       column of PROGRAMS) also FAILs when its median exceeds REF * 1.005
#       (compared exactly: median * 1000 <= REF * 1005). A REF of `-` has no
#       relative check.
#         --programs mandelbrot,arr-sum,collatz   S7 gate
#
#   scripts/vm_perf_gate.sh --repros [SEL]
#       Per-construct repro rows. SEL is a comma-separated list of slice names
#       and/or row names; a slice name selects every row of that slice. Without
#       SEL, every row runs. Examples (the §13 DAG gates):
#         --repros S1                  S1 gate: loop, call
#         --repros S1,part             S2 gate: S1 rows plus part
#         --repros S1,part,fold,S5     S5 gate
#         --repros S1,part,fold,foldmod  S7 gate
#         (no SEL: every row)          S8 gate; `swap` is its red check
#       Rows (cost = (prog - base) / units; `loop` has no base). From S7
#       `loop.ax` is a `PureLoop`, so the call rows subtract loop_generic.ax
#       (loop.ax plus a never-taken `if`, which call1.ax, mutcall.ax and
#       swapcall.ax carry too, so it cancels):
#         row       slice  prog      base          units  budget
#         loop      S1     loop      -             1M     300   per iteration
#         call      S1     call1     loop_generic  1M     800   one-arg call via dispatch_call -> call_fn_in
#         part      S2     part      part_base     1M     250   per partition iteration
#         fold      S4     fold      fold_base     10M    350   per element, builtin -> closure call
#         fastcall  S5     call1     loop_generic  1M     300   one-arg fast call
#         mutcall   S5     mutcall   loop_generic  1M     600   one-&mut-arg call via call_mut
#         foldmod   S7     foldmod   fold_base     10M    200   per element, arr-sum's body `acc + x % m`
#         swap      S8     swapcall  loop_generic  1M     730   per `swap(&mut a, 0, 1)` call, body included
#       A row passes when (prog - base) <= budget * units, compared exactly.
#
#   scripts/vm_perf_gate.sh --compile
#       S9 row (R50 §4 S9, compilebench AX-58): big-compile (1,002 fns, each
#       entered once; a verbatim copy of compilebench's
#       benchmarks/big-compile/axon/main.ax) on the same binary under three
#       configurations, five runs each, median of five: AXON_ENGINE=tree,
#       AXON_ENGINE=vm (deferred compile) and AXON_ENGINE=vm AXON_VM_EAGER=1.
#       Passes when default <= 1.01 x tree and default <= 0.95 x eager
#       (integer comparison, no rounding). The second check is the red one:
#       eager compile-on-first-entry costs about 1.09 x tree.
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
# program        CPython 3.14.4 median, run 20261008T142344Z (sieve: S10, see top)
# REF: the median of the commit named, release build, measured with
# `AXON_BIN=<that axon> VM_PERF_CPU=<cpu> scripts/vm_perf_gate.sh`: S10
# (944fe056, CPU 11) for every program but fib-recursive, which is S11's own
# target; its REF is the S11 commit S12 is built on (2627dad7, CPU 13; the
# first of two gate runs, whose medians were 1002729957 and 1002729093).
# program        budget        REF
PROGRAMS=(
  # S11: a third of CPython's median, spec §10
  "fib-recursive 1141214164    1002729957"
  "collatz       26058032109   18366495062"
  "mandelbrot    15023124683   5039973411"
  "arr-sum       23624147901   4979381330"
  "qsort         18967575334   10939169020"
  "sieve         26764304040   24568573662"
)
# row       slice prog     base          units     budget
REPROS=(
  "loop      S1    loop     -             1000000   300"
  "call      S1    call1    loop_generic  1000000   800"
  "part      S2    part     part_base     1000000   250"
  "fold      S4    fold     fold_base     10000000  350"
  "fastcall  S5    call1    loop_generic  1000000   300"
  "mutcall   S5    mutcall  loop_generic  1000000   600"
  "foldmod   S7    foldmod  fold_base     10000000  200"
  "swap      S8    swapcall loop_generic  1000000   730"
)
# ────────────────────────────────────────────────────────────────────────────

# S9 row: prog, runs, default <= tree * TREE_PCT / 100, default <= eager * EAGER_PCT / 100
COMPILE_ROW="big-compile 5 101 95"

MODE=programs
SEL=""
while [ $# -gt 0 ]; do
  case "$1" in
    --programs)
      MODE=programs
      if [ $# -gt 1 ] && [[ "$2" != --* ]]; then SEL="$2"; shift; fi ;;
    --programs=*) MODE=programs; SEL="${1#--programs=}" ;;
    --repros)
      MODE=repros
      if [ $# -gt 1 ] && [[ "$2" != --* ]]; then SEL="$2"; shift; fi ;;
    --repros=*) MODE=repros; SEL="${1#--repros=}" ;;
    --compile) MODE=compile ;;
    -h|--help) sed -n '2,68p' "$0"; exit 0 ;;
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

# The shipping configuration: no eager compile, no trace (R50 S9).
unset AXON_VM_EAGER AXON_VM_TRACE

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

# measure_as <key> <prog> <runs> <engine> <eager> — <runs> runs of <prog> under
# AXON_ENGINE=<engine> (AXON_VM_EAGER=<eager>, empty = off); sets MEDIAN[key]
# to the median count; counts golden mismatches.
measure_as() {
  local key="$1" prog="$2" runs="$3" engine="$4" eager="$5" k c counts=() golden="$FIX/$2.golden"
  [ -n "${MEDIAN[$key]+x}" ] && return 0
  [ -f "$FIX/$prog.ax" ] && [ -f "$golden" ] || not_measured "missing $FIX/$prog.ax or its golden"
  for k in $(seq 1 "$runs"); do
    c="$(AXON_ENGINE="$engine" AXON_VM_EAGER="$eager" perf_count "$WORK/$key.$k" "$AXON" run "$FIX/$prog.ax")"
    [ -n "$c" ] || not_measured "perf did not count $key (run $k): $(grep -v '^#' "$WORK/$key.$k.perf" | head -2 | tr '\n' ' ')"
    if [ "$(cat "$WORK/$key.$k.rc")" != 0 ] || ! cmp -s "$WORK/$key.$k.out" "$golden"; then
      echo "vm_perf_gate: $key run $k: exit $(cat "$WORK/$key.$k.rc"), output differs from $prog.golden"
      diff "$golden" "$WORK/$key.$k.out" | head -5 | sed 's/^/      /'
      head -5 "$WORK/$key.$k.err" | sed 's/^/      stderr: /'
      fails=$((fails + 1))
    fi
    counts+=("$c")
  done
  MEDIAN[$key]="$(printf '%s\n' "${counts[@]}" | sort -n | sed -n "$(( (runs + 1) / 2 ))p")"
  echo "vm_perf_gate: measured $key: ${counts[*]} -> median ${MEDIAN[$key]}"
}

# measure <prog> — three runs under the shipping configuration; sets MEDIAN[prog].
measure() { measure_as "$1" "$1" 3 vm ""; }

group() { # 1234567 -> 1,234,567
  echo "$1" | sed -E ':a; s/^(-?[0-9]+)([0-9]{3})/\1,\2/; ta'
}

if [ "$MODE" = programs ]; then
  declare -A pick=()
  if [ -n "$SEL" ]; then
    IFS=, read -ra toks <<<"$SEL"
    for t in "${toks[@]}"; do
      hit=0
      for row in "${PROGRAMS[@]}"; do
        read -r prog _ <<<"$row"
        if [ "$t" = "$prog" ]; then pick[$prog]=1; hit=1; fi
      done
      if [ "$hit" = 0 ]; then
        echo "vm_perf_gate: --programs: '$t' is not a program (fib-recursive collatz mandelbrot arr-sum qsort sieve)" >&2
        exit 2
      fi
    done
  fi
  for row in "${PROGRAMS[@]}"; do
    read -r prog budget ref <<<"$row"
    [ -z "$SEL" ] || [ -n "${pick[$prog]+x}" ] || continue
    measure "$prog"
    m="${MEDIAN[$prog]}"
    if [ "$m" -le "$budget" ]; then v="PASS"; else v="OVER"; fails=$((fails + 1)); fi
    printf 'vm_perf_gate: %-14s median %16s  budget %16s  %s\n' "$prog" "$(group "$m")" "$(group "$budget")" "$v"
    if [ "$ref" != "-" ]; then
      if [ $((m * 1000)) -le $((ref * 1005)) ]; then v="PASS"; else v="OVER"; fails=$((fails + 1)); fi
      pct="$(awk -v a="$m" -v b="$ref" 'BEGIN { printf "%+.3f", (a / b - 1) * 100 }')"
      printf 'vm_perf_gate: %-14s median %16s  ref %19s  %s %% (<= +0.5 %%)  %s\n' \
        "$prog" "$(group "$m")" "$(group "$ref")" "$pct" "$v"
    fi
  done
elif [ "$MODE" = compile ]; then
  read -r prog runs tree_pct eager_pct <<<"$COMPILE_ROW"
  measure_as "$prog.tree" "$prog" "$runs" tree ""
  measure_as "$prog.default" "$prog" "$runs" vm ""
  measure_as "$prog.eager" "$prog" "$runs" vm 1
  t="${MEDIAN[$prog.tree]}" d="${MEDIAN[$prog.default]}" e="${MEDIAN[$prog.eager]}"
  if [ $((d * 100)) -le $((t * tree_pct)) ]; then v1="PASS"; else v1="OVER"; fails=$((fails + 1)); fi
  if [ $((d * 100)) -le $((e * eager_pct)) ]; then v2="PASS"; else v2="OVER"; fails=$((fails + 1)); fi
  ratio() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.4f", a / b }'; }
  printf 'vm_perf_gate: compile %s default %s  tree %s  = %s x tree (<= %s/100)  %s\n' \
    "$prog" "$(group "$d")" "$(group "$t")" "$(ratio "$d" "$t")" "$tree_pct" "$v1"
  printf 'vm_perf_gate: compile %s default %s  eager %s  = %s x eager (<= %s/100)  %s\n' \
    "$prog" "$(group "$d")" "$(group "$e")" "$(ratio "$d" "$e")" "$eager_pct" "$v2"
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
        echo "vm_perf_gate: --repros: '$t' is neither a row (loop call part fold fastcall mutcall foldmod swap) nor a slice with rows (S1 S2 S4 S5 S7 S8)" >&2
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
