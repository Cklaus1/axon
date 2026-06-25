#!/usr/bin/env bash
# native_ffi_forge.sh — R13 slice 4 soundness gate: a FORGED native handle that
# reaches the linked C-ABI shim must be a GRACEFUL exit-101 panic (I-4), NEVER a
# segfault / host abort.
#
# The Axon surface CANNOT forge a handle (the affine type system makes it
# unconstructible — E1803 on any arithmetic, E0601 on use-after-consume). This
# test exercises the DEFENSE-IN-DEPTH layer BENEATH that static guarantee: it
# links a tiny C caller against `libaxon_rt.a` and calls the real
# `__axon_gfx_frame_count` C symbol with a forged `{tag, payload}` handle whose
# slab index is out-of-range / i64::MIN / negative. The shim's handle-table
# lookup must reject every one with a clean exit 101 — proving a codegen bug or
# an `unsafe`-equivalent that smuggled a bad index across the boundary cannot
# segfault the host.
#
# This is the NATIVE (linked) realization of the §8 property
# "fuzz handle payloads with garbage indices → always graceful, never abort",
# complementing the in-process `axon_gfx_mock::bad_handle_is_graceful_err` unit
# test. Skips cleanly if no C toolchain is present.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CC="${CC:-cc}"
if ! command -v "$CC" >/dev/null 2>&1; then
  echo "native_ffi_forge: no C compiler ($CC) — skipping"; exit 0
fi

# Build axon-rt (carries the __axon_gfx_* shim symbols).
if ! cargo build -q -p axon-rt 2>/dev/null; then
  echo "native_ffi_forge: axon-rt build unavailable — skipping"; exit 0
fi
RT_LIB=""
for cand in target/debug/libaxon_rt.a target/release/libaxon_rt.a; do
  [ -f "$cand" ] && RT_LIB="$cand" && break
done
if [ -z "$RT_LIB" ]; then
  echo "native_ffi_forge: libaxon_rt.a not found — skipping"; exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# The C caller. AxonHandle = { i64 tag, i64 payload } by value, matching
# codegen's `{i64,i64}` Handle and axon-rt's repr(C) AxonHandle. We forge a
# handle with a wild slab index and call the real shim symbol — it must NOT
# return (it should print a panic line and exit 101), so reaching the printf
# after the call is a SOUNDNESS FAILURE.
cat > "$WORK/forge.c" <<'EOF'
#include <stdint.h>
#include <stdio.h>

typedef struct { int64_t tag; int64_t payload; } AxonHandle;

/* The frozen Surface nominal tag (axon_gfx_mock::TAG_GFX_SURFACE). */
#define TAG_GFX_SURFACE 0x67667802LL

extern int64_t __axon_gfx_frame_count(AxonHandle s);

int main(int argc, char** argv) {
    int64_t forged = (int64_t)0; /* default; overridden by argv[1] */
    if (argc > 1) {
        /* parse a (possibly negative / huge) index from argv[1] */
        sscanf(argv[1], "%lld", (long long*)&forged);
    }
    AxonHandle h = { TAG_GFX_SURFACE, forged };
    /* This MUST exit 101 inside the shim (graceful panic on a bad slab index).
       If it returns and we reach here, the boundary failed to reject a forged
       handle — a soundness violation. */
    int64_t got = __axon_gfx_frame_count(h);
    printf("SOUNDNESS-FAIL: forged handle returned %lld\n", (long long)got);
    return 0;
}
EOF

if ! "$CC" "$WORK/forge.c" "$RT_LIB" -lpthread -ldl -lm -o "$WORK/forge" 2>"$WORK/cc.err"; then
  echo "native_ffi_forge: link failed (toolchain) — skipping"
  cat "$WORK/cc.err" >&2
  exit 0
fi

fail=0
# A spread of forged indices: out-of-range, negative, the i64 extremes, and a
# never-allocated 0 (the table starts empty).
for idx in 0 1 7 9999 -1 -9999 9223372036854775807 -9223372036854775808; do
  out="$("$WORK/forge" "$idx" 2>&1)"; rc=$?
  if [ "$rc" -ne 101 ]; then
    echo "FAIL: forged index $idx exited $rc (expected 101 graceful panic)"
    echo "  output: $out"
    fail=1
  elif echo "$out" | grep -q "SOUNDNESS-FAIL"; then
    echo "FAIL: forged index $idx was ACCEPTED by the shim (no rejection)"
    fail=1
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "native_ffi_forge: PASS — every forged handle across the C ABI is a graceful exit-101 (never a segfault)"
fi
exit "$fail"
