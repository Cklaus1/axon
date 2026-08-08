#!/usr/bin/env bash
# wasm_parity.sh — R7 Slice A acceptance harness.
#
# Builds the codegen-free `axon-run` for both native and wasm32-wasip1, then
# runs a corpus of pure-compute `.ax` examples through each and asserts the exit
# code AND stdout are identical. This is the executable form of the R7 §9
# acceptance criterion `wasm_interp_matches_native_on_pure_compute`: the wasm
# interpreter is the *same* interp.rs compiled to a second target, so the only
# divergence surface is the host interface (R7 §4.3) — and pure-compute programs
# touch none of it.
#
# Requires: rustup target wasm32-wasip1, and a wasm runtime (wasmtime) on PATH.
# Skips (exit 0 with a notice) if the wasm toolchain is absent, so it is safe to
# run in environments without it.
#
# Usage:  scripts/wasm_parity.sh
set -u

# AUDIT O004: take the SHARED wasm build lock. Several of these harnesses build
# for wasm32 concurrently under cargo's parallel test threads and clobber each
# other's intermediates, which surfaces as examples silently failing to link.
# Nine harnesses already took this lock; this one did not, so it raced against
# them. No-op without flock.
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi


ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Locate a wasm runtime.
WASMRT=""
for rt in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do
  if command -v "$rt" >/dev/null 2>&1 || [ -x "$rt" ]; then WASMRT="$rt"; break; fi
done
if [ -z "$WASMRT" ]; then
  echo "wasm_parity: no wasm runtime (wasmtime) found — skipping (install: curl https://wasmtime.dev/install.sh | bash)"
  exit 0
fi
if ! rustup target list --installed 2>/dev/null | grep -q wasm32-wasip1; then
  echo "wasm_parity: wasm32-wasip1 target not installed — skipping (rustup target add wasm32-wasip1)"
  exit 0
fi

echo "wasm_parity: building axon-run (native + wasm32-wasip1)…"
cargo build -q -p axon-core --no-default-features --bin axon-run || { echo "native build failed"; exit 1; }
cargo build -q -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 || { echo "wasm build failed"; exit 1; }

NATIVE="target/debug/axon-run"
WASM="target/wasm32-wasip1/debug/axon-run.wasm"

# Pure-compute corpus (R7 §4.1 row 1): AUTO-DISCOVERED — every examples/*.ax with
# a `fn main` that touches NONE of the host interface (fs/env/AI/threads/exec/
# random/time). Those are the programs whose only divergence surface (the host
# interface, R7 §4.3) is empty, so the native and wasm interpreters MUST agree by
# construction (I-2: same interp.rs, two targets). Auto-discovery means a new
# pure-compute example is covered automatically — no hand-maintained list to drift.
# AUDIT T14 (finding P5-03): `env_var` and the `http_*` family were MISSING from
# this alternation, so examples that read the environment or open a socket were
# auto-discovered as "pure compute". anthropic_stream.ax (env_var + http_sse_post),
# http_get.ax and trainloop_stream.ax are all in that class — which is why the
# corpus produced a native-vs-wasm divergence on an unconfigured-network path and
# the suite carried a permanent baseline failure.
#
# Note the comment above promises "no hand-maintained list to drift" while
# defining precisely such a list. It drifted. Deriving this set from
# `builtins::builtin_effect_row` is tracked separately (O009) — this change makes
# the corpus match its own stated invariant now.
# Divergent-effect builtins, grouped by why they can differ native-vs-wasm.
# Derived from `builtins::builtin_effect_row`: anything tagged Net/AI/Random/
# Time/Hal/Bpf/Tee, plus everything `capabilities::classify_call` treats as a
# capability (fs/net/exec/env). Console IO (println/print/eprint) is DELIBERATELY
# absent — stdout is what this harness compares, so excluding it would empty the
# corpus. That distinction is why this set cannot be "every non-empty effect
# row": `builtin_effect_row` lumps console output and filesystem access into the
# same `IO` tag (the same conflation T8 fixed for exec).
#
# `builtin_effect_row_names_are_excluded_from_the_pure_corpus` in cli_run.rs
# fails if any divergent builtin is missing here, so this can no longer drift.
HOST_BUILTINS='read_file|write_file|append_file|file_size|read_line|env_var'\
'|http_get|http_post|http_sse|ai_complete|ai_extract|ai_cost_spent'\
'|exec|spawn|chan_|goal_|agent_detect|agent_uncertainty|agent_trace|zephyr_'\
'|random_|gaussian_sample|beta_sample|categorical_sample'\
'|now_ms|sleep_ms|temporal_now'\
'|atomic_|volatile_|port_in_|port_out_|ptr_from_addr|fn_addr|tee_|bpf_'
CORPUS=()
for f in examples/*.ax; do
  grep -q "fn main" "$f" || continue
  grep -qE "$HOST_BUILTINS" "$f" && continue
  CORPUS+=("$(basename "$f")")
done
echo "wasm_parity: ${#CORPUS[@]} pure-compute examples auto-discovered"

pass=0; fail=0; nocov=0; nocov_names=""
for name in "${CORPUS[@]}"; do
  f="examples/$name"
  [ -f "$f" ] || continue
  # Native.
  n_out="$("$NATIVE" "$f" 2>/dev/null)"; n_code=$?
  # wasm (wasmtime needs an absolute path and a dir grant to read the file).
  w_out="$("$WASMRT" --dir / "$WASM" "$ROOT/$f" 2>/dev/null)"; w_code=$?

  # AUDIT T36 (finding GATE-02). Two legs that BOTH fail before producing any
  # output "agree" trivially — identical empty stdout, identical non-zero exit —
  # and were counted as a pass. That is zero parity evidence dressed as coverage:
  # if a builtin became unsupported on both sides tomorrow, its row would keep
  # printing OK. Note the test is empty-stdout AND non-zero, not merely non-zero:
  # examples/sum_types.ax legitimately exits 47 (its computed total) after
  # printing, and rejecting every non-zero exit would silently delete real rows.
  if [ "$n_code" -ne 0 ] && [ "$w_code" -ne 0 ] && [ -z "$n_out" ] && [ -z "$w_out" ]; then
    echo "  NO-COVERAGE $name (both legs failed with no output: native=$n_code wasm=$w_code)"
    nocov=$((nocov+1))
    nocov_names="$nocov_names $name"
    continue
  fi

  if [ "$n_code" = "$w_code" ] && [ "$n_out" = "$w_out" ]; then
    echo "  OK   $name (exit $n_code)"
    pass=$((pass+1))
  else
    echo "  DIFF $name native=(code $n_code) wasm=(code $w_code)"
    echo "    native stdout: $n_out"
    echo "    wasm   stdout: $w_out"
    fail=$((fail+1))
  fi
done

echo "wasm_parity: $pass passed, $fail differ, $nocov no-coverage"
[ "$fail" -eq 0 ] || exit 1
[ "$pass" -gt 0 ] || { echo "wasm_parity: no corpus files ran"; exit 1; }
if [ "$nocov" -ne 0 ]; then
  echo "wasm_parity: FAILED —$nocov_names produced no parity evidence (both legs failed silently)"
  exit 1
fi
echo "wasm_parity: native and wasm interpreters agree on the pure-compute corpus ✓"
