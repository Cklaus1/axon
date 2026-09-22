#!/usr/bin/env bash
# wasm_object_prune.sh — R7: dead-function pruning makes the wasm object linkable.
#
# declare_builtins emits ~119 helper functions unconditionally; on wasm32 the
# unused str/array helpers carry the i64-pointer ABI that clashes with wasm's
# i32 libc at link (`function signature mismatch`). compile_to_wasm_object now
# runs prune_dead_functions first, so a PURE-INTEGER program emits a wasm object
# with ZERO __axon_* imports — and rust-lld links it with NO signature
# mismatches (the remaining wasm gap is only the wasi entry-point ABI).
#
# Skips (exit 0) when codegen / the wasm toolchain is absent.
set -u

# AUDIT O004: take the SHARED wasm build lock. Several of these harnesses build
# for wasm32 concurrently under cargo's parallel test threads and clobber each
# other's intermediates, which surfaces as examples silently failing to link.
# Nine harnesses already took this lock; this one did not, so it raced against
# them. No-op without flock.
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "wasm_object_prune: codegen build unavailable — skipping"; exit 0
fi
AXON="${AXON:-target/debug/axon}"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
PROG="$WORK/triv.ax"; printf 'fn main() -> i64 { 21 + 21 }\n' > "$PROG"

if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$PROG" >/dev/null 2>&1; then
  echo "wasm_object_prune: wasm object emit unavailable — skipping"; exit 0
fi
OBJ="${PROG%.ax}.wasm"
[ -f "$OBJ" ] || { echo "wasm_object_prune: no emitted object — skipping"; exit 0; }

# Count __axon_* symbols in the object, EXCLUDING the ones deliberately kept
# live. The bare "must be ZERO" form was a proxy for the property that actually
# matters — no leftover str/array helper carrying the i64-pointer ABI that
# clashes with wasm's i32 libc — and it stopped being equivalent to it once a
# runtime symbol was intentionally called from every wasip1 prologue.
#
# The allowlist NAMES each expected symbol rather than raising the count to
# "<=1", because a threshold would admit whichever helper happened to survive
# next; an unexpected name still fails. Each entry must be genuinely live (so
# pruning cannot remove it) and pointer-free (so it cannot reintroduce the ABI
# clash this harness exists to catch):
#
#   __axon_rt_refuse_interp_only_env  fn() -> void. Emitted into the wasip1
#     prologue so a compiled artifact REFUSES the interpreter-only env controls
#     rather than silently ignoring them (AXON_ALLOWED_EFFECTS was measured
#     performing the effects and exiting 0 on this target). Never emitted for
#     wasm32-unknown-unknown.
#
#   __axon_rt_seed_rng  fn() -> void. Seeds the C RNG in the wasip1 prologue.
#     Without it the artifact returns ONE FIXED SEQUENCE forever — measured,
#     `random_i64(1, 1000000)` twice gave 1 and 883707 on every run and under
#     every AXON_SEED. Also never emitted for wasm32-unknown-unknown, which has
#     neither a libc nor a clock to seed from.
#
# This allowlist earned its shape immediately: written naming only the first
# symbol, it failed on the second rather than absorbing it.
ALLOWED_LIVE='__axon_rt_refuse_interp_only_env __axon_rt_seed_rng'
UNEXPECTED=$(python3 - "$OBJ" "$ALLOWED_LIVE" <<'PYEOF'
import re, sys
obj, allowed = sys.argv[1], set(sys.argv[2].split())
found = set(m.decode() for m in re.findall(rb'__axon_[a-z_0-9]+', open(obj, 'rb').read()))
print(' '.join(sorted(found - allowed)))
PYEOF
) || UNEXPECTED="<probe-failed>"
if [ -n "$UNEXPECTED" ]; then
  echo "wasm_object_prune: pure-int wasm object has UNEXPECTED __axon_* symbols: $UNEXPECTED"
  echo "wasm_object_prune: FAIL — expected none beyond the allowlist (dead-function pruning regressed)"; exit 1
fi
echo "wasm_object_prune: pure-int wasm object has no __axon_* symbols beyond the allowlist"

# Bonus: if rust-lld + wasi libc are present, confirm it LINKS with no signature
# mismatches (the prune's whole point). A trap at runtime is the entry-point ABI,
# a separate slice — we only assert the link is mismatch-free here.
RUSTLLD="$(find "$HOME/.rustup/toolchains" -name rust-lld -path '*x86_64-unknown-linux-gnu*' 2>/dev/null | head -1)"
WASIDIR="$(find "$HOME/.rustup/toolchains" -type d -path '*wasm32-wasip1/lib/self-contained' 2>/dev/null | head -1)"
if [ -n "$RUSTLLD" ] && [ -n "$WASIDIR" ] && [ -f "$WASIDIR/libc.a" ]; then
  LINKLOG="$WORK/link.log"
  "$RUSTLLD" -flavor wasm "$WASIDIR/crt1-command.o" "$OBJ" "$WASIDIR/libc.a" -o "$WORK/linked.wasm" >"$LINKLOG" 2>&1 || true
  MM=$(grep -c "function signature mismatch" "$LINKLOG" 2>/dev/null)
  [ -n "$MM" ] || MM=0
  echo "wasm_object_prune: link signature mismatches: $MM"
  if [ "$MM" != "0" ]; then
    echo "wasm_object_prune: FAIL — pruning should leave 0 mismatches"; exit 1
  fi
  echo "wasm_object_prune: links clean (no ABI mismatch)"
fi

echo "wasm_object_prune: PASS"
exit 0
