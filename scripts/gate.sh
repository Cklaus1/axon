#!/usr/bin/env bash
# gate.sh — the single, atomic build gate for axon-core.
#
# Every code change (mine or a subagent's) must pass THIS exact gate before it
# is committed, so "green" means the same thing everywhere. Runs:
#   1. cargo fmt --all --check (text-only, so it runs before anything builds)
#   2. the full test suite (interpreter path, --no-default-features)
#   3. the native codegen build (cargo build -p axon-core)
#   4. clippy as a hard error (lib by default; --strict adds --all-targets)
#
# Determinism: AXON_SEED + AXON_AI_MOCK are pinned so seeded-RNG / AI-call tests
# never flake. Speed: mold linker + sccache rustc cache are used IF installed
# (purely local — never committed to .cargo/config.toml, so contributors/CI
# without them still build fine). The wasm stack-size config in
# .cargo/config.toml is untouched (this only sets the host target's linker).
#
# Usage:
#   scripts/gate.sh            # standard gate (lib clippy)
#   scripts/gate.sh --strict   # also run --all-targets clippy
#   scripts/gate.sh --nextest  # use cargo-nextest as the test runner
#
# Exit 0 iff all stages pass.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

STRICT=0
USE_NEXTEST=0
for arg in "$@"; do
  case "$arg" in
    --strict) STRICT=1 ;;
    --nextest) USE_NEXTEST=1 ;;
    *) echo "gate: unknown flag $arg" >&2; exit 2 ;;
  esac
done

# Deterministic test environment.
export AXON_SEED="${AXON_SEED:-42}"
export AXON_AI_MOCK="${AXON_AI_MOCK:-1}"

# Optional local speedups — only if present, never required.
if command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
fi
if command -v mold >/dev/null 2>&1; then
  # Host-target-scoped so the committed wasm linker config is untouched.
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS:--C link-arg=-fuse-ld=mold}"
fi

fail() { echo ""; echo "❌ gate FAILED at: $1"; exit 1; }

# The harness-skip log (O006b) is APPEND-only across runs, so the coverage notice
# at the end of this script would otherwise report skips from previous runs as if
# they had happened now. Truncate it so the notice describes THIS run only.
SKIPLOG="target/harness-skips.log"
mkdir -p target && : > "$SKIPLOG"

# Fast doc-focus check first (pure text over VISION.md, no build) — keeps the
# north-star doc short/legible the same way the *_parity.sh harnesses keep the
# compiler honest. Cheap enough to run on every gate, codegen or not.
echo "── gate: VISION.md focus ──────────────────────────────────────────"
./scripts/vision_focus.sh || fail "VISION.md focus"

# Formatting. This is deliberately BEFORE the build: it is pure text, costs
# under a second, and a fmt failure needs no compiler to be true. It is also
# --all, not -p axon-core, because per-crate scoping is exactly how 37 files of
# drift accumulated unseen in the crates nobody was checking (2980206). Adding a
# crate to the workspace? --all picks it up with no edit here.
echo "── gate: cargo fmt --all --check ──────────────────────────────────"
cargo fmt --all -- --check || fail "cargo fmt --all --check (run: cargo fmt --all)"

echo "── gate: tests (--no-default-features) ─────────────────────────────"
if [ "$USE_NEXTEST" = 1 ] && command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run -p axon-core --no-default-features || fail "tests (nextest)"
else
  cargo test -p axon-core --no-default-features || fail "tests"
fi

echo "── gate: native codegen build ─────────────────────────────────────"
cargo build -p axon-core || fail "native build"

# AUDIT T15 (finding P5-34). Nothing ever built the serde-json feature — not
# gate.sh, not CI — so lsp.rs rotted silently as new Type variants landed and
# `cargo check --features serde-json` failed outright. That means `axon lsp` and
# `axon parse --json`, both advertised in CLAUDE.md under "Phase 4 ✅ Complete",
# could not be compiled at all. An advertised command with no build gate is a
# command that will eventually stop existing without anyone noticing.
echo "── gate: serde-json feature builds (axon lsp / axon parse --json) ─"
cargo check --no-default-features --features serde-json -p axon-core \
  || fail "serde-json feature check (axon lsp / axon parse --json)"

echo "── gate: clippy (lib, -D warnings) ────────────────────────────────"
cargo clippy --no-default-features -p axon-core -- -D warnings || fail "lib clippy"

# BUG_HUNT #35: the runtime crates (axon-rt/axon-ai/axon-surface) used to be
# invisible to the clippy gate (scoped to -p axon-core), hiding ~80 lints. They
# are now clippy-clean under --all-targets (the intentional C-ABI ptr-deref
# seams carry a documented crate-level allow), so the gate enforces them. They
# have no codegen feature, so this is cheap and needs no --no-default-features.
# axon-vm/axon-attest joined 2026-07-19 (found while sizing R33.S2a): same class
# of gap, same fix -- 3 pre-existing findings in axon-vm (a too-many-arguments
# and a manual-range-contains in main.rs, a dead-code AxonManifest struct) fixed
# with #[allow(..)]/a mechanical rewrite, no behavior change; axon-attest was
# already clean.
# axon-ledger joined 2026-07-31 (R18 governance audit): same coverage-gap class
# — the crate landed as a workspace member with 63 tests but was never lint-
# gated; 2 mechanical clippy fixes (a ?-operator rewrite, a redundant &) made
# it clean, no behavior change.
echo "── gate: clippy runtime crates (-D warnings) ─────────────────────"
# O-RLM-04: this list is an ALLOWLIST, not the workspace, so a crate absent from
# it is simply unlinted — and a green gate reads as coverage. Six crates were
# outside it (axon-intent, axon-os, axon-web, axon-audit, axon-certcheck,
# axon-signal); axon-os alone carried ~11 warnings including a dead function.
# Third recorded sighting of this class, so the fix is the list AND this note.
# Adding a new crate to the workspace? Add it here in the same commit.
cargo clippy -p axon-rt -p axon-ai -p axon-surface -p axon-gfx-mock -p axon-domain \
  -p axon-vm -p axon-attest -p axon-ledger -p axon-intent -p axon-os -p axon-web \
  -p axon-audit -p axon-certcheck -p axon-signal --all-targets -- -D warnings \
  || fail "runtime-crate clippy"

if [ "$STRICT" = 1 ]; then
  echo "── gate: clippy (--all-targets, -D warnings) ─────────────────────"
  cargo clippy --no-default-features -p axon-core --all-targets -- -D warnings || fail "all-targets clippy"

  # BUG_HUNT #35 follow-on: the codegen feature (axon-core WITH default features)
  # was never clippy-gated — the lib clippy above uses --no-default-features, so
  # the entire IR-emitter path (codegen/*.rs) was invisible. It is now clean
  # (~86 mechanical .into()/let-_/&Path lints fixed, verified native==interp via
  # the parity harnesses), so --strict enforces it going forward. Only under
  # --strict because it links LLVM (slower than the interp-only passes).
  echo "── gate: clippy codegen feature (--all-targets, -D warnings) ─────"
  cargo clippy -p axon-core --all-targets -- -D warnings || fail "codegen-feature clippy"

  # Coverage gap closed: the test stage above runs --no-default-features, so any
  # `#[cfg(feature = "codegen")]` integration test (e.g. the end-to-end runtime
  # `@[verify]` enforcement test) was NEVER executed by the gate — a regression
  # there stayed green. --strict now also runs the codegen-gated integration
  # tests. Scoped to the integration_fixtures target (the only home of codegen-
  # gated tests today); the rest already run under the interp pass / via the
  # CARGO_BIN_EXE harnesses. Under --strict only because it links LLVM.
  echo "── gate: codegen-gated integration tests ────────────────────────"
  cargo test -p axon-core --test integration_fixtures || fail "codegen integration tests"

  # R1d Slice-3 drift kill-gate (governance/specs/R1d-single-source-builtins.md):
  # the builtin_externs drift tests live behind #[cfg(feature = "codegen")], so
  # the standard-gate `cargo test --no-default-features` at the top compiles them
  # out (0 run) and the integration_fixtures line above skips --lib entirely — a
  # BUILTIN_EXTERNS/STR_OUT_EXTERNS table-drift regression passed both gates
  # green (same class as the clippy allowlist gap above). Run them explicitly
  # with default features (codegen on). Cheap: same build as the two stages
  # above, 5 tests, ~0s.
  echo "── gate: builtin-externs drift tests (R1d slice 3) ──────────────"
  cargo test -p axon-core --lib codegen::builtin_externs || fail "builtin-externs drift tests"

  # The two-engine invariant (I-2): native codegen + AOT-wasm must match the
  # interpreter oracle byte-for-byte. ~22 scripts/*_parity.sh harnesses assert
  # this, but were historically run ad hoc — which is how the silent-divergence
  # bugs (#27/#36/#38/#39/parse_*_or) reached main. parity_all.sh runs the whole
  # suite; a real divergence fails the gate, toolchain-absent harnesses skip
  # cleanly. Under --strict only (links LLVM + may run wasmtime; ~2 min).
  echo "── gate: parity suite (interp ↔ codegen / AOT-wasm) ─────────────"
  ./scripts/parity_all.sh --quiet || fail "parity suite"

  # Coverage gap closed (the [[coverage-vacuous-pass-guard]] class): the entire
  # `smt` feature — Phase 5 §4's Z3-backed @[verify] + refinement-return prover
  # (smt.rs, 18 unit tests) — is behind `#[cfg(feature = "smt")]` and so was
  # NEVER built or tested by any gate stage. A regression in the prover stayed
  # green. --strict now clippy-gates and tests it. The feature links the system
  # libz3 dynamically; when libz3 isn't installed we SKIP cleanly (like the wasm
  # harnesses) rather than fail, so the gate still works on a Z3-less box.
  if echo 'int main(){return 0;}' | cc -xc - -lz3 -o /dev/null 2>/dev/null; then
    echo "── gate: clippy + tests (smt feature, Z3) ───────────────────────"
    cargo clippy --no-default-features -p axon-core --features smt --all-targets -- -D warnings \
      || fail "smt-feature clippy"
    cargo test --no-default-features -p axon-core --features smt --lib smt \
      || fail "smt unit tests"
  else
    echo "── gate: smt feature SKIPPED (libz3 not found; install to enable) ─"
  fi
fi

echo ""

# AUDIT T36 (finding GATE-03). The default gate's test stage runs
# --no-default-features, so every codegen-dependent parity wrapper in cli_run.rs
# reports `ok` while asserting nothing ("codegen unavailable — parity skipped"),
# and parity_all.sh runs under --strict only. A non-strict run therefore proves
# NOTHING about invariant I-2 while printing "✅ gate PASSED" — the same vacuous-
# pass shape this repo has now hit three times.
#
# Deliberately NOT silently changed: promoting parity_all.sh into the default
# path costs 7m49s on this box (measured 2026-08-04: 44 passed / 5 skipped of
# 49). That is a real decision about gate latency, not one to make as a side
# effect of a bug fix. What this does is refuse to let the vacuity be silent.
# ($SKIPLOG is truncated at the top of this script so this reflects THIS run.)
if [ "$STRICT" != 1 ]; then
  echo "── gate: coverage notice ───────────────────────────────────────────"
  if [ -s "$SKIPLOG" ]; then
    n_skips=$(sort -u "$SKIPLOG" | wc -l | tr -d ' ')
    echo "  $n_skips harness(es) SKIPPED — these gates measured nothing:"
    sort -u "$SKIPLOG" | sed 's/^/    · /'
  fi
  echo "  This run did NOT verify interp↔codegen / AOT-wasm parity (invariant I-2)."
  echo "  The test stage is --no-default-features, so the codegen parity wrappers"
  echo "  cannot assert. To actually check I-2:"
  echo "      ./scripts/gate.sh --strict      # full gate incl. the parity suite"
  echo "      ./scripts/parity_all.sh         # parity suite alone (~8 min)"
  echo "      AXON_HARNESS_STRICT=1 ...       # make any skipped harness FATAL"
fi

echo ""
echo "✅ gate PASSED"
