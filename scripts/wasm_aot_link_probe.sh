#!/usr/bin/env bash
# wasm_aot_link_probe.sh — R7 Slice B: a reproducible probe of the AOT-wasm
# *link* boundary, capturing the empirically-established diagnosis (2026-06-03)
# so it cannot silently drift.
#
# FINDING (see governance/specs/R7-targets.md §12 Q6): the runnable-.wasm link is
# NOT blocked by missing libc symbols (the wasi `libc.a` + a wasm32-wasip1 build
# of axon-rt supply them) — it is blocked by a real **i64↔i32 pointer-width ABI
# mismatch**. Codegen bakes i64 ptr/len into the str/array IR (the AxonStr
# `{i64 len, i64 ptr}` ABI); wasm32 libc + the wasm axon-rt use i32 pointers, so
# `rust-lld` reports `function signature mismatch` (on `__axon_str_*`,
# `__axon_parse_int_*`, and even `memcmp`/`write`/`strlen`) and the linked module
# TRAPS under wasmtime. The fix is a wasm32 codegen ABI retarget (multi-slice),
# not a link step.
#
# This probe is INFORMATIONAL — it documents/verifies the boundary and prints
# what it found. It EXITS 0 whether or not the toolchain is present (so it is
# safe in any CI), and is the harness the eventual i64→i32 retarget slice will
# evolve into a real pass/fail wasm-AOT-parity gate. It does NOT assert success
# today (there is none to assert); it asserts the diagnosis is still accurate.
set -u

# AUDIT O004: take the SHARED wasm build lock. Several of these harnesses build
# for wasm32 concurrently under cargo's parallel test threads and clobber each
# other's intermediates, which surfaces as examples silently failing to link.
# Nine harnesses already took this lock; this one did not, so it raced against
# them. No-op without flock.
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi


ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

say() { echo "wasm_aot_link_probe: $*"; }

# Locate the wasm link toolchain (best-effort; skip cleanly if absent).
RUSTLLD="$(find "$HOME/.rustup/toolchains" -name rust-lld -path '*x86_64-unknown-linux-gnu*' 2>/dev/null | head -1)"
WASIDIR="$(find "$HOME/.rustup/toolchains" -type d -path '*wasm32-wasip1/lib/self-contained' 2>/dev/null | head -1)"
WASMTIME="$(command -v wasmtime || echo "$HOME/.wasmtime/bin/wasmtime")"

if [ -z "$RUSTLLD" ] || [ -z "$WASIDIR" ] || [ ! -f "$WASIDIR/libc.a" ]; then
  say "wasm link toolchain (rust-lld + wasi libc.a) not found — skipping (exit 0)"
  exit 0
fi

# 1. The wasm axon-rt must build for wasm32-wasip1 (the libc/sysroot half — SOLVED).
say "building axon-rt for wasm32-wasip1…"
if ! cargo build -q -p axon-rt --target wasm32-wasip1 2>/dev/null; then
  say "axon-rt wasm32-wasip1 build unavailable — skipping (exit 0)"
  exit 0
fi
RTLIB="target/wasm32-wasip1/debug/libaxon_rt.a"
[ -f "$RTLIB" ] || { say "no $RTLIB — skipping"; exit 0; }
say "OK: wasm axon-rt staticlib built ($(wc -c <"$RTLIB") bytes) — libc/sysroot half is solved"

# 2. Emit a wasm object for a trivial program.
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  say "codegen axon binary unavailable — skipping (exit 0)"
  exit 0
fi
AXON="target/debug/axon"
PROG="$WORK/w.ax"
printf 'fn main() -> i64 { 21 + 21 }\n' > "$PROG"
if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$PROG" >/dev/null 2>&1; then
  say "wasm object emit failed — skipping (exit 0)"
  exit 0
fi
OBJ="$WORK/w.wasm"
[ -f "$OBJ" ] || OBJ="${PROG%.ax}.wasm"
[ -f "$OBJ" ] || { say "no emitted object found — skipping"; exit 0; }

# 3. Attempt the link and CAPTURE the signature-mismatch diagnosis.
say "linking object + wasi libc + wasm axon-rt (expecting i64↔i32 ABI mismatch)…"
LINKLOG="$WORK/link.log"
"$RUSTLLD" -flavor wasm "$WASIDIR/crt1-command.o" "$OBJ" "$WASIDIR/libc.a" "$RTLIB" \
  -o "$WORK/linked.wasm" >"$LINKLOG" 2>&1 || true

MISMATCHES=$(grep -c "function signature mismatch" "$LINKLOG" 2>/dev/null || echo 0)
if [ "$MISMATCHES" -gt 0 ]; then
  say "CONFIRMED: $MISMATCHES function-signature mismatches — the i64↔i32 ABI gap is still present"
  grep "function signature mismatch" "$LINKLOG" | sed 's/^/    /' | head -6
  say "DIAGNOSIS HOLDS: Slice B needs a wasm32 codegen ABI retarget (i64→i32), not a link step."
  say "(see governance/specs/R7-targets.md §12 Q6)"
  exit 0
fi

# If we ever get here with ZERO mismatches, the ABI retarget may have landed —
# try to actually RUN it and report (this is the future success path).
say "NO signature mismatches — the ABI gap may be resolved; attempting wasmtime run…"
if [ -x "$WASMTIME" ] && [ -f "$WORK/linked.wasm" ]; then
  if OUT=$("$WASMTIME" "$WORK/linked.wasm" 2>&1); then
    say "RUNNABLE WASM: wasmtime ran it (exit 0). The AOT-wasm link path may now work — \
upgrade this probe into a real wasm-AOT parity gate."
  else
    say "linked but traps/non-zero under wasmtime: $OUT"
  fi
else
  say "wasmtime not found; cannot run — but the link produced no mismatch warnings."
fi
exit 0
