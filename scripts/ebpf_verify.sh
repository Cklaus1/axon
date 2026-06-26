#!/usr/bin/env bash
# R23 eBPF acceptance gate: `axon build --target bpf` emits a .bpf.o that the
# in-kernel BPF verifier ACCEPTS.
#
# Verification strategy (strongest that works on this host):
#   1. Build the object with `axon build --target bpf`.
#   2. Confirm it's a well-formed elf64-bpf object (sections + map relocation).
#   3. KERNEL LOAD: compile scripts/bpfload.c and issue bpf(BPF_PROG_LOAD) — the
#      real in-kernel verifier. Requires root (sudo); `/usr/sbin/bpftool` is a
#      broken per-kernel wrapper under WSL2 and is NOT used. If not root, the
#      structural check (#2) stands as the gate and the kernel load is SKIPPED
#      with a clear note.
#
# Skips gracefully if the BPF toolchain (llvm-objdump + a codegen `axon`) is
# absent. Asserts it actually ran (no vacuous pass).
#
# Usage: scripts/ebpf_verify.sh
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"

skip() { echo "SKIP: $*" >&2; exit 0; }
fail() { echo "FAIL: $*" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# ── Tool checks ──────────────────────────────────────────────────────────────
command -v llvm-objdump  >/dev/null 2>&1 || skip "llvm-objdump not found"
command -v llvm-readelf   >/dev/null 2>&1 || skip "llvm-readelf not found"

AXON_BIN=""
for candidate in "$REPO/target/debug/axon" "$REPO/target/release/axon" "$(command -v axon 2>/dev/null || true)"; do
    if [[ -x "$candidate" ]]; then AXON_BIN="$candidate"; break; fi
done
[[ -n "$AXON_BIN" ]] || skip "axon binary not found (build with: cargo build -p axon-core)"

SRC="$REPO/examples/bpf/counter.ax"
[[ -f "$SRC" ]] || fail "example missing: $SRC"

# Confirm this axon has the bpf target (codegen feature). A no-codegen build
# prints an error and exits non-zero; treat that as SKIP, not FAIL.
OBJ="$WORK/counter.bpf.o"
if ! "$AXON_BIN" build --target bpf "$SRC" --out "$OBJ" >"$WORK/build.log" 2>&1; then
    if grep -qiE "requires building axon with the .codegen. feature|not supported by this LLVM" "$WORK/build.log"; then
        skip "axon lacks the bpf/codegen target ($(head -1 "$WORK/build.log"))"
    fi
    cat "$WORK/build.log" >&2
    fail "axon build --target bpf failed"
fi
[[ -f "$OBJ" ]] || fail "no .bpf.o emitted at $OBJ"

# ── #2 Structural check: well-formed elf64-bpf with the expected sections ─────
RAN_STRUCTURAL=0
fmt="$(llvm-objdump -h "$OBJ" 2>/dev/null)"
echo "$fmt" | grep -qi "socket"  || fail "no 'socket' program section in the object"
echo "$fmt" | grep -qi "maps"    || fail "no 'maps' section in the object"
echo "$fmt" | grep -qi "license" || fail "no 'license' section in the object"
llvm-readelf -r "$OBJ" 2>/dev/null | grep -q "axon_map" \
    || fail "no R_BPF_64_64 relocation on axon_map (map reference missing)"
llvm-objdump -d "$OBJ" 2>/dev/null | grep -q "call 0x1" \
    || fail "no 'call 0x1' (bpf_map_lookup_elem) in the program"
RAN_STRUCTURAL=1
echo "OK structural: elf64-bpf, socket+maps+license sections, axon_map reloc, call 1"

# ── #3 Kernel verifier load (the strong check) ───────────────────────────────
RAN_KERNEL=0
if [[ "$(id -u)" == "0" || -n "$(command -v sudo 2>/dev/null || true)" ]]; then
    if command -v cc >/dev/null 2>&1 && [[ -e /sys/kernel/btf/vmlinux ]]; then
        LOADER="$WORK/bpfload"
        if cc -w "$SCRIPT_DIR/bpfload.c" -o "$LOADER" 2>"$WORK/cc.log"; then
            SUDO=""
            [[ "$(id -u)" != "0" ]] && SUDO="sudo"
            if $SUDO "$LOADER" "$OBJ" socket 1 >"$WORK/load.log" 2>&1; then
                grep -q "VERIFIER ACCEPTED" "$WORK/load.log" \
                    || fail "loader exited 0 but no ACCEPT line: $(cat "$WORK/load.log")"
                echo "OK kernel: in-kernel BPF verifier ACCEPTED the Axon-emitted program"
                grep -E "VERIFIER ACCEPTED|processed" "$WORK/load.log" >&2
                RAN_KERNEL=1
            else
                # A genuine verifier rejection is a real failure; a permission /
                # capability problem is a SKIP of just this stage.
                if grep -qiE "Permission denied|Operation not permitted|EPERM" "$WORK/load.log"; then
                    echo "NOTE: kernel load needs privileges here; structural check stands" >&2
                else
                    cat "$WORK/load.log" >&2
                    fail "in-kernel verifier REJECTED the program"
                fi
            fi
        else
            echo "NOTE: could not build bpfload.c; structural check stands" >&2
        fi
    else
        echo "NOTE: no cc or no kernel BTF; structural check stands" >&2
    fi
else
    echo "NOTE: not root and no sudo; kernel load skipped, structural check stands" >&2
fi

# ── Assert we actually verified something (no vacuous pass) ───────────────────
if [[ "$RAN_STRUCTURAL" != "1" ]]; then
    fail "verification did not run (neither structural nor kernel check executed)"
fi

if [[ "$RAN_KERNEL" == "1" ]]; then
    echo "PASS: Axon→eBPF object built and ACCEPTED by the in-kernel verifier."
else
    echo "PASS: Axon→eBPF object built and structurally valid (kernel load not run here)."
fi
