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
echo "── gate: clippy runtime crates (-D warnings) ─────────────────────"
cargo clippy -p axon-rt -p axon-ai -p axon-surface --all-targets -- -D warnings \
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
fi

echo ""
echo "✅ gate PASSED"
