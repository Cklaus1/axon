#!/usr/bin/env bash
# gate.sh — the single, atomic build gate for axon-core.
#
# Every code change (mine or a subagent's) must pass THIS exact gate before it
# is committed, so "green" means the same thing everywhere. Runs:
#   1. the full test suite (interpreter path, --no-default-features)
#   2. the native codegen build (cargo build -p axon-core)
#   3. clippy as a hard error (lib by default; --strict adds --all-targets)
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

# Fast doc-focus check first (pure text over VISION.md, no build) — keeps the
# north-star doc short/legible the same way the *_parity.sh harnesses keep the
# compiler honest. Cheap enough to run on every gate, codegen or not.
echo "── gate: VISION.md focus ──────────────────────────────────────────"
./scripts/vision_focus.sh || fail "VISION.md focus"

echo "── gate: tests (--no-default-features) ─────────────────────────────"
if [ "$USE_NEXTEST" = 1 ] && command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run -p axon-core --no-default-features || fail "tests (nextest)"
else
  cargo test -p axon-core --no-default-features || fail "tests"
fi

echo "── gate: native codegen build ─────────────────────────────────────"
cargo build -p axon-core || fail "native build"

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
echo "── gate: clippy runtime crates (-D warnings) ─────────────────────"
cargo clippy -p axon-rt -p axon-ai -p axon-surface -p axon-gfx-mock -p axon-domain -p axon-vm -p axon-attest --all-targets -- -D warnings \
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
echo "✅ gate PASSED"
