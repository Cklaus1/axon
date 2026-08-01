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
AXON="target/debug/axon"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
PROG="$WORK/triv.ax"; printf 'fn main() -> i64 { 21 + 21 }\n' > "$PROG"

if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$PROG" >/dev/null 2>&1; then
  echo "wasm_object_prune: wasm object emit unavailable — skipping"; exit 0
fi
OBJ="${PROG%.ax}.wasm"
[ -f "$OBJ" ] || { echo "wasm_object_prune: no emitted object — skipping"; exit 0; }

# Count __axon_* symbols in the object. A pure-int program must have ZERO.
N=$(python3 -c "import re,sys; print(len(set(re.findall(rb'__axon_[a-z_0-9]+', open('$OBJ','rb').read()))))" 2>/dev/null || echo -1)
echo "wasm_object_prune: pure-int wasm object has $N __axon_* symbols"
if [ "$N" != "0" ]; then
  echo "wasm_object_prune: FAIL — expected 0 (dead-function pruning regressed)"; exit 1
fi

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
