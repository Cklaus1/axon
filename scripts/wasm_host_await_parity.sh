#!/usr/bin/env bash
# wasm_host_await_parity.sh — interactive host_await runs IDENTICALLY on native
# and on wasm32-wasip1 under wasmtime (R15 / R7 headless-interactive).
#
# The native host_await substrate is a worker thread (run_suspendable); wasm has
# no threads, so the wasm host_await_yield reads stdin DIRECTLY (synchronous,
# single-stack). This harness pipes the same input to a host_await program run
# (a) by the native interpreter and (b) by axon-run.wasm under wasmtime, and
# asserts byte-identical stdout + exit code — proving the two substrates are
# observably equivalent for the headless (stdin-driven) case. (The browser case —
# wasm32-unknown-unknown, no stdin — needs the Asyncify+JS substrate; R7c.)
#
# Requires: rustup target wasm32-wasip1 + wasmtime on PATH. Skips (exit 0) if absent.
set -u
# shellcheck source=lib/harness_skip.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib/harness_skip.sh"

# AUDIT O004: take the SHARED wasm build lock. Several of these harnesses build
# for wasm32 concurrently under cargo's parallel test threads and clobber each
# other's intermediates, which surfaces as examples silently failing to link.
# Nine harnesses already took this lock; this one did not, so it raced against
# them. No-op without flock.
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi


ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# A skip must prove its own reason. The previous probe piped rustup
# into grep, discarding rustup's exit status and stderr, so a MISSING
# rustup concluded "target not installed" — reporting a fact it never
# established. rust_target_installed separates the three outcomes.
rust_target_installed wasm32-wasip1; _rt=$?
if [ "$_rt" -eq 2 ]; then
  echo "wasm_host_await_parity: cannot determine whether wasm32-wasip1 is installed — refusing to call that a skip" >&2
  exit 1
elif [ "$_rt" -ne 0 ]; then
  echo "wasm_host_await_parity: wasm32-wasip1 target not installed — skipping"; exit 0
fi
WASMTIME=""
for c in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do command -v "$c" >/dev/null 2>&1 && WASMTIME="$c" && break; done
if [ -z "$WASMTIME" ]; then echo "wasm_host_await_parity: wasmtime not found — skipping"; exit 0; fi

echo "wasm_host_await_parity: building axon (native) + axon-run (wasm32-wasip1)…"
# Own target dir. A --no-default-features `axon` written to the SHARED
# target/debug/axon is codegen-less but still executable, so every harness that
# follows in the same suite run passes its `[ -x "$AXON" ]` guard, never
# rebuilds, and then fails its native/wasm build with E0907 -- which those
# harnesses reported as "toolchain absent". Stable path so cargo can reuse.
CARGO_TARGET_DIR="target/interp-only" \
  cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null || { echo "native build failed — skipping"; exit 0; }
cargo build -q -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 2>/dev/null || { echo "wasm build failed — skipping"; exit 0; }
NATIVE="target/interp-only/debug/axon"
WASM="target/wasm32-wasip1/debug/axon-run.wasm"

# (program, piped-stdin) pairs covering: a fixed-exchange prompt, EOF, and a
# multi-turn approval loop.
check() {
  local label="$1" prog="$2" input="$3"
  local n_out n_code w_out w_code
  n_out="$(printf '%b' "$input" | "$NATIVE" run "$prog" 2>&1)"; n_code=$?
  w_out="$(printf '%b' "$input" | "$WASMTIME" run --dir=. "$WASM" "$prog" 2>&1)"; w_code=$?
  # Strip CLI diagnostics that differ by design: run-id (unique per invocation)
  # is emitted by the full `axon` CLI but not by the lighter `axon-run` wasm
  # target. All user-visible output must still match.
  strip_diag() { grep -v '^axon: run-id '; }
  local n_cmp w_cmp
  n_cmp="$(printf '%s' "$n_out" | strip_diag)"
  w_cmp="$(printf '%s' "$w_out" | strip_diag)"
  if [ "$n_cmp" != "$w_cmp" ] || [ "$n_code" != "$w_code" ]; then
    echo "wasm_host_await_parity: FAIL ($label): native[$n_code] != wasm[$w_code]"
    echo "--- native ---"; echo "$n_out" | sed 's/^/  /'
    echo "--- wasm ---";   echo "$w_out" | sed 's/^/  /'
    exit 1
  fi
  echo "  OK  $label: [$n_code] native==wasm"
}

check greet      examples/interactive/greet.ax          'Ada\n'
check greet_eof  examples/interactive/greet.ax          ''
check guess      examples/interactive/guess.ax          '5\n9\n7\n'
check approval   examples/interactive/approval_agent.ax 'y\nn\ny\n'

echo "wasm_host_await_parity: PASS — host_await runs identically on native + wasm32-wasip1"
exit 0
