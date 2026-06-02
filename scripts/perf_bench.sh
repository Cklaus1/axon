#!/usr/bin/env bash
# perf_bench.sh — R1 Tier-1 performance benchmark.
#
# The R1 acceptance asks for "a native binary ... + a perf-tier benchmark".
# This times the SAME pure-compute `.ax` programs run two ways:
#   (1) the tree-walking interpreter (`axon run`)
#   (2) the native AOT binary (`axon build`, debug — see #38 → run the binary)
# and reports per-program wall-clock + the native speedup ratio. A native
# compiler that doesn't beat its own interpreter on compute isn't Tier-1; this
# is the evidence it does.
#
# Method: min-of-N wall-clock (the cleanest run, least scheduler noise). Programs
# are exit-code workloads (CPU-bound), so timing the process start→exit captures
# the compute. Determinism: AXON_SEED pinned.
#
# Requires the codegen-capable `axon` binary (default build). Skips (exit 0) if
# `axon build` is unavailable.
#
# Usage:  scripts/perf_bench.sh [--trials N]
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TRIALS=5
case "${1:-}" in
  --trials) TRIALS="${2:?--trials needs a number}" ;;
esac

export AXON_SEED="${AXON_SEED:-42}"

AXON="$ROOT/target/debug/axon"
if [ ! -x "$AXON" ]; then
  echo "perf_bench: building the codegen axon binary…"
  cargo build -q -p axon-core || { echo "build failed"; exit 1; }
fi

# Confirm `axon build` (codegen) is available — else skip cleanly.
echo 'fn main() -> i64 { 0 }' > /tmp/_pb_probe.ax
if ! "$AXON" build /tmp/_pb_probe.ax -o /tmp/_pb_probe_bin >/dev/null 2>&1; then
  echo "perf_bench: \`axon build\` unavailable (no codegen) — skipping"
  rm -f /tmp/_pb_probe.ax /tmp/_pb_probe_bin
  exit 0
fi
rm -f /tmp/_pb_probe.ax /tmp/_pb_probe_bin

# Pure-compute corpus (CPU-bound, no I/O — the fair comparison set).
CORPUS=( hello.ax math.ax while.ax algorithms.ax modulo.ax
         logical_ops.ax floats.ax comprehensive.ax algorithms.ax )

# min wall-clock nanoseconds of a command over TRIALS runs.
min_ns() {
  local best=99999999999 t start end
  for _ in $(seq "$TRIALS"); do
    start=$(date +%s%N)
    "$@" >/dev/null 2>&1
    end=$(date +%s%N)
    t=$(( end - start ))
    (( t < best )) && best=$t
  done
  echo "$best"
}

printf "%-20s %12s %12s %9s\n" "program" "interp(ms)" "native(ms)" "speedup"
printf -- "%.0s-" {1..56}; echo
total_speedup=0; n=0
for name in "${CORPUS[@]}"; do
  f="examples/$name"
  [ -f "$f" ] || continue
  bin="/tmp/_pb_$(basename "$name" .ax)"
  # Debug native build (not --release): `axon build --release` currently fails
  # with duplicate-std-symbol link errors because it links BOTH libaxon_rt.a
  # and libaxon_ai.a, each bundling std (BUG_HUNT #38). Debug links one and
  # works; interp-vs-native is still a fair unoptimized comparison.
  "$AXON" build "$f" -o "$bin" >/dev/null 2>&1 || { echo "  build failed: $name"; continue; }

  i_ns=$(min_ns "$AXON" run "$f")
  n_ns=$(min_ns "$bin")
  rm -f "$bin"

  i_ms=$(awk "BEGIN{printf \"%.2f\", $i_ns/1000000}")
  n_ms=$(awk "BEGIN{printf \"%.2f\", $n_ns/1000000}")
  spd=$(awk "BEGIN{ if ($n_ns>0) printf \"%.1fx\", $i_ns/$n_ns; else printf \"n/a\" }")
  printf "%-20s %12s %12s %9s\n" "$name" "$i_ms" "$n_ms" "$spd"

  if [ "$n_ns" -gt 0 ]; then
    total_speedup=$(awk "BEGIN{print $total_speedup + $i_ns/$n_ns}")
    n=$((n+1))
  fi
done

echo
if [ "$n" -gt 0 ]; then
  avg=$(awk "BEGIN{printf \"%.1f\", $total_speedup/$n}")
  echo "perf_bench: native is ${avg}x faster than the interpreter on average ($n programs, min-of-$TRIALS)"
else
  echo "perf_bench: no programs benchmarked"; exit 1
fi
