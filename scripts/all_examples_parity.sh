#!/usr/bin/env bash
# all_examples_parity.sh — R1 acceptance: EVERY example runs identically under
# the native codegen backend and the interpreter (I-2).
#
# Builds + runs every `examples/*.ax` that has a `fn main` BOTH ways under
# AXON_AI_MOCK=1 + AXON_SEED=42 (so the AI examples are deterministic — the
# native AI path now honors AXON_AI_MOCK, matching the interpreter's stub) and
# asserts byte-identical stdout + identical exit code. This turns the long-
# standing manual "26/28" claim into a gated 28/28: the 2 AI examples used to
# differ ONLY because native codegen ignored AXON_AI_MOCK and hit the real API;
# that gap is now closed, so the sweep is total.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Build the codegen binary up front. When this harness is invoked from INSIDE a
# `cargo test` run (the cli_run wrapper), the parent cargo holds the build lock
# on target/, so a nested `cargo build` here would block/fail — detect that and
# skip cleanly rather than report a false divergence. Prefer an already-built
# binary if present (the gate builds it before running tests).
echo "all_examples_parity: locating codegen axon binary…"
AXON="target/debug/axon"
if [ ! -x "$AXON" ]; then
  if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
    echo "all_examples_parity: codegen build unavailable (LLVM absent or build lock) — skipping"
    exit 0
  fi
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Probe: can this binary actually emit a native build? (The gate's
# `--no-default-features` test run may have left a codegen-less `axon` in place.)
# If a trivial build fails, codegen is unavailable here — skip cleanly.
printf 'fn main() -> i64 { 0 }\n' > "$WORK/probe.ax"
if ! AXON_AI_MOCK=1 "$AXON" build "$WORK/probe.ax" -o "$WORK/probe.bin" --no-cache >/dev/null 2>&1; then
  echo "all_examples_parity: this axon binary cannot emit native builds (no codegen feature) — skipping"
  exit 0
fi

pass=0; diff=0; failbuild=0; total=0
fails=""
for f in examples/*.ax; do
  grep -q "fn main" "$f" || continue
  total=$((total + 1))
  base="$(basename "$f" .ax)"

  I_OUT="$(AXON_AI_MOCK=1 AXON_SEED=42 "$AXON" run "$f" 2>/dev/null)"; I_EXIT=$?
  BIN="$WORK/$base"
  if ! AXON_AI_MOCK=1 "$AXON" build "$f" -o "$BIN" --no-cache >/dev/null 2>&1; then
    failbuild=$((failbuild + 1)); fails="$fails\n  BUILD-FAIL: $base"; continue
  fi
  N_OUT="$(AXON_AI_MOCK=1 AXON_SEED=42 "$BIN" 2>/dev/null)"; N_EXIT=$?

  if [ "$I_OUT" = "$N_OUT" ] && [ "$I_EXIT" = "$N_EXIT" ]; then
    pass=$((pass + 1))
  else
    diff=$((diff + 1))
    fails="$fails\n  DIFFER: $base (exit interp=$I_EXIT native=$N_EXIT)"
  fi
done

echo "all_examples_parity: $pass/$total match, $diff differ, $failbuild build-fail"
if [ "$diff" -ne 0 ] || [ "$failbuild" -ne 0 ]; then
  printf "all_examples_parity: FAIL$fails\n"
  exit 1
fi
echo "all_examples_parity: PASS — all $total examples native==interp under mock"
exit 0
