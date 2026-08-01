#!/usr/bin/env bash
# wasm_unknown_interp_builds.sh — the interpreter crate must COMPILE for
# wasm32-unknown-unknown (the WASI-FREE in-browser interpreter target, R7c).
#
# This is the precondition for the R15 browser binding: the interp has to build
# for unknown-unknown before host_await can be driven there (via Asyncify + a JS
# import, R15 §13). The host touchpoints that need a no-thread variant are
# on_deep_stack (already cfg-split) and run_suspendable / run_suspendable_stdio
# (cfg-split: native uses a worker thread, wasm runs the program directly with no
# host driver). A regression — a new unconditional std::thread::spawn / scope in
# the interp — would FAIL this check.
#
# Requires the wasm32-unknown-unknown rustup target. Skips (exit 0) if absent.
set -u

# AUDIT O004: take the SHARED wasm build lock. Several of these harnesses build
# for wasm32 concurrently under cargo's parallel test threads and clobber each
# other's intermediates, which surfaces as examples silently failing to link.
# Nine harnesses already took this lock; this one did not, so it raced against
# them. No-op without flock.
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi


ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
  echo "wasm_unknown_interp_builds: wasm32-unknown-unknown target not installed — skipping"
  echo "  (rustup target add wasm32-unknown-unknown)"
  exit 0
fi

echo "wasm_unknown_interp_builds: cargo check -p axon-core (interp, no codegen) for wasm32-unknown-unknown…"
if cargo check -q -p axon-core --no-default-features --target wasm32-unknown-unknown 2>/tmp/wasm_unknown_check.err; then
  echo "wasm_unknown_interp_builds: PASS — the interpreter compiles for the WASI-free browser target"
  exit 0
else
  echo "wasm_unknown_interp_builds: FAIL — the interpreter no longer compiles for wasm32-unknown-unknown:"
  grep -iE 'error|thread|spawn' /tmp/wasm_unknown_check.err | head -10 | sed 's/^/  /'
  exit 1
fi
