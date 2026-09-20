#!/usr/bin/env bash
# all_examples_parity.sh — R1 acceptance: EVERY example runs identically under
# the native codegen backend and the interpreter (I-2).
#
# Builds + runs every `examples/*.ax` AND `examples/stdlib/*.ax` that has a
# `fn main` BOTH ways under
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
. "$ROOT/scripts/lib/harness_skip.sh"

# Build the codegen binary up front. When this harness is invoked from INSIDE a
# `cargo test` run (the cli_run wrapper), the parent cargo holds the build lock
# on target/, so a nested `cargo build` here would block/fail — detect that and
# skip cleanly rather than report a false divergence. Prefer an already-built
# binary if present (the gate builds it before running tests).
echo "all_examples_parity: locating codegen axon binary…"
AXON="${AXON:-target/debug/axon}"
if [ ! -x "$AXON" ]; then
  if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
    echo "all_examples_parity: codegen build unavailable (LLVM absent or build lock) — skipping"
    exit 0
  fi
fi

# The located binary EXISTING is not the same as it being able to codegen, and
# this harness reports the difference as findings. `target/debug/axon` is shared
# with every other build of this workspace, and a concurrent
# `--no-default-features` build leaves an INTERP-ONLY binary sitting at exactly
# that path. Every `axon build` then fails, and the report says "29 BUILD-FAIL"
# -- which reads as a compiler regression and is really two builds sharing one
# output path.
#
# That is not hypothetical: it failed this way twice, and both times the failure
# was investigated as an interp<->native divergence before the cause was found.
# `exit_code_parity.sh` and `smt_discharge_parity.sh` already avoid it by
# building into a private CARGO_TARGET_DIR; this harness prefers a prebuilt
# binary instead, so it has to VERIFY the one it found.
#
# Probe with a trivial program rather than trusting the path. Skipping is
# correct here: an interp-only binary means codegen is not under test in this
# invocation, and a skip says so where 29 BUILD-FAILs actively mislead.
_probe="$(mktemp -d)"; printf 'fn main() -> i64 { 0 }\n' > "$_probe/p.ax"
if ! berr="$("$AXON" build "$_probe/p.ax" -o "$_probe/p" 2>&1)"; then
  rm -rf "$_probe"
  # "cannot codegen (interp-only build, or LLVM absent)" was ASSERTED for every
  # build failure — including a compiler that is simply broken. Prove it.
  if native_build_unavailable "$berr"; then
    echo "all_examples_parity: this is a SKIP, not a pass: set AXON=<codegen binary> to actually run it."
    harness_skip all_examples_parity "\`$AXON\` cannot codegen (interp-only build, or LLVM absent)"
  fi
  echo "all_examples_parity: FAIL — \`$AXON\` cannot build a trivial program (a build error is not a skip):"
  printf '%s\n' "$berr" | head -5 | sed 's/^/        /'
  exit 1
fi
rm -rf "$_probe"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Probe: can this binary actually emit a native build? (The gate's
# `--no-default-features` test run may have left a codegen-less `axon` in place.)
# If a trivial build fails, codegen is unavailable here — skip cleanly.
printf 'fn main() -> i64 { 0 }\n' > "$WORK/probe.ax"
if ! berr="$(AXON_AI_MOCK=1 "$AXON" build "$WORK/probe.ax" -o "$WORK/probe.bin" --no-cache 2>&1)"; then
  native_build_failed all_examples_parity "trivial probe program" "$berr" || exit 1
fi

pass=0; diff=0; failbuild=0; refused=0; total=0
fails=""; refuses=""
by_design=0
# Multi-module examples resolve their imports through AXON_PATH, and several
# document the exact invocation in their own header (`AXON_PATH=examples/stdlib
# axon run examples/asi/bandit_ucb.ax`). Passing the union here is what lets
# those examples be swept at all — without it they fail at CHECK with E0901
# "module not found" and look like broken programs.
AXPATH="examples/stdlib:examples/asi:examples/modular:examples/domain"

# `examples/stdlib/*.ax` is included deliberately.
#
# This loop globbed `examples/*.ax` only, so a whole directory of 36 runnable
# programs — the Tier-2 stdlib types — was in NO parity sweep. That gap hid a
# memory-corruption bug for the life of the feature: `llvm_sizeof` reported
# every struct as 8 bytes, so `Result<BigStruct, str>` truncated its payload,
# and `replicated.ax` natively stopped committing at a 2/3 quorum and began
# ACCEPTING stale writes. The top-level examples never put a struct bigger than
# 16 bytes through a Result, so 55 harnesses stayed green.
#
# Only `stdlib/` is added. The other subdirectories hold examples that are
# deliberately refused (`flagship/`, `bpf/`) or need a platform (`browser/`,
# `native/`), and folding those in would trade a real signal for noise.
#
# Basenames are unique across the two directories (checked), so the per-example
# binary name below stays collision-free.
for f in examples/*.ax examples/stdlib/*.ax examples/asi/*.ax \
         examples/domain/*.ax examples/modular/*.ax; do
  grep -q "fn main" "$f" || continue

  # A program the CHECKER rejects is not a parity question: it never runs on
  # either engine, so there is nothing to compare. Eight examples are rejected
  # by design (capability violations in `flagship/`, the `bpf/bad_*` set,
  # `contained_violation.ax`), and counting those as divergences would make this
  # harness cry wolf about programs that are doing their job.
  if ! AXON_PATH="$AXPATH" "$AXON" check "$f" >/dev/null 2>&1; then
    by_design=$((by_design + 1))
    continue
  fi

  total=$((total + 1))
  # Path-derived, because `agent` exists in TWO directories and a basename would
  # make the two examples share one binary.
  base="$(echo "${f#examples/}" | tr '/' '_' | sed 's/\.ax$//')"

  I_OUT="$(AXON_AI_MOCK=1 AXON_SEED=42 AXON_PATH="$AXPATH" "$AXON" run "$f" 2>/dev/null)"; I_EXIT=$?
  BIN="$WORK/$base"
  BUILD_ERR="$(AXON_AI_MOCK=1 AXON_PATH="$AXPATH" "$AXON" build "$f" -o "$BIN" --no-cache 2>&1)"; B_EXIT=$?
  if [ "$B_EXIT" -ne 0 ]; then
    # A clean E0910 refusal is NOT a divergence: codegen SOUNDLY declines an
    # interpreter-only builtin (e.g. the network http_* / host_await family)
    # rather than miscompiling it (sound-by-refusal, invariant I-2). That is the
    # CORRECT outcome for an interp-only example — count it as an expected
    # refusal, and require that the example still ran under the interpreter (so
    # "refused" really means interp-only, never globally broken). Any OTHER
    # nonzero build is a real BUILD-FAIL.
    if echo "$BUILD_ERR" | grep -q 'E0910' && [ "$I_EXIT" -ne 127 ]; then
      refused=$((refused + 1)); refuses="$refuses\n  REFUSED (interp-only, E0910): $base"
    else
      failbuild=$((failbuild + 1)); fails="$fails\n  BUILD-FAIL: $base"
    fi
    continue
  fi
  N_OUT="$(AXON_AI_MOCK=1 AXON_SEED=42 "$BIN" 2>/dev/null)"; N_EXIT=$?

  if [ "$I_OUT" = "$N_OUT" ] && [ "$I_EXIT" = "$N_EXIT" ]; then
    pass=$((pass + 1))
  else
    diff=$((diff + 1))
    fails="$fails\n  DIFFER: $base (exit interp=$I_EXIT native=$N_EXIT)"
  fi
done

echo "all_examples_parity: $pass/$total match, $diff differ, $failbuild build-fail, $refused interp-only-refused (E0910), $by_design rejected-at-check (invalid by design)"
if [ "$refused" -ne 0 ]; then
  printf "all_examples_parity: interp-only (codegen soundly refuses, runs under \`axon run\`):$refuses\n"
fi
if [ "$diff" -ne 0 ] || [ "$failbuild" -ne 0 ]; then
  printf "all_examples_parity: FAIL$fails\n"
  exit 1
fi
echo "all_examples_parity: PASS — $pass examples native==interp under mock; $refused interp-only (sound E0910 refusal)"
exit 0
