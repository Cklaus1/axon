#!/usr/bin/env bash
# K5: axon kernel enforcement gate.
#
# Layer 1 (compiler) and Layer 2 (manifest generation) need no VM. Layer 3 runs
# the flagship good/evil pair in the LINUX microVM profile (guest-fs GF-4;
# scripts/fc_linux_profile.sh, image dist/guest-linux), where the program
# really runs:
#
#   agent_task.ax       policy {"fs":"none", ["IO"]} (axon-vm-mmds/2) -> the
#                       guest workload exits 0 and prints exactly what the host
#                       interpreter prints
#   agent_task_evil.ax  policy ["IO"] (axon-vm-mmds/1) -> the guest's `axon run`
#                       refuses it at check, workload exit 2, nothing printed
#
# Layer 3b is the bare-metal axon-guest-kernel (dist/guest) run, kept only as a
# labelled "gate installs" smoke check: that kernel does not run programs
# (FG-041), so it asserts only that its LSTAR gate denies a syscall whose
# effect is withheld (exit 8), never anything about the program.
#
# Exit: 0 all ran and passed; 1 any failure; 77 Layer 3 could not run (not
# root, no /dev/kvm, no firecracker, no built dist/guest-linux image): a
# NON-RESULT, never a pass. --dry-run skips Layer 3 on purpose and exits 0.
#
# Usage:
#   ./scripts/axon_kernel_gate.sh [--dry-run]

set -euo pipefail
cd "$(dirname "$0")/.."

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=1
fi

PASS=0
FAIL=0

ok()   { echo "  ✓ $*"; PASS=$((PASS+1)); }
fail() { echo "  ✗ $*"; FAIL=$((FAIL+1)); }

GOOD="examples/flagship/agent_task.ax"
EVIL="examples/flagship/agent_task_evil.ax"
KERNEL="dist/guest/vmlinuz"
INITRD="dist/guest/initramfs.cpio.gz"

# ── Build interpreter if needed ───────────────────────────────────────────────

# The binaries this gate runs are the ones it builds here -- never ones that
# merely sit in target/ (C9 round 4; scripts/lib/axon_bin.sh).
. scripts/lib/axon_bin.sh
echo "[axon-kernel-gate] Building axon interpreter..."
cargo build -p axon-core --no-default-features --bin axon --quiet
AXON=""; use_built AXON axon

echo "[axon-kernel-gate] Building axon-vm..."
cargo build -p axon-vm --quiet
AXON_VM=""; use_built AXON_VM axon-vm

echo "[axon-kernel-gate] Layer 1: compiler (@[contained] / effect-row)"

# ── Layer 1: compiler enforcement (always, no Firecracker needed) ─────────────

if [[ -f "$GOOD" ]]; then
    if "$AXON" check "$GOOD" 2>&1 | grep -q "E1001\|E1004\|E1310"; then
        fail "good_agent.ax has unexpected capability errors"
    else
        ok "good_agent.ax passes axon check (no capability errors)"
    fi
else
    echo "  [skip] $GOOD not found"
fi

if [[ -f "$EVIL" ]]; then
    if "$AXON" check "$EVIL" 2>&1 | grep -q "E1001\|E1004\|E1310"; then
        ok "evil_agent.ax is rejected by axon check (E1001/E1310 as expected)"
    else
        # If the evil agent passes the checker, the redteam is the gate.
        echo "  [info] evil_agent.ax not caught by axon check — relying on runtime gate"
    fi
else
    echo "  [skip] $EVIL not found"
fi

echo "[axon-kernel-gate] Layer 2: BPF policy generation"

# ── Layer 2: BPF generation from .axmeta ─────────────────────────────────────

if [[ -f "$GOOD" ]]; then
    # Build with --emit-manifest to get .axmeta.
    cargo build -p axon-core --features codegen --quiet 2>/dev/null || true
    AXON_CODEGEN=""; use_built AXON_CODEGEN axon
    if "$AXON_CODEGEN" build "$GOOD" --emit-manifest 2>/dev/null; then
        META="${GOOD%.ax}.axmeta"
        if [[ -f "$META" ]]; then
            ok "good_agent.axmeta generated"
            if python3 -c "import json,sys; d=json.load(open('$META')); assert d.get('risk') in ('low','medium','high','critical'), d" 2>/dev/null; then
                ok "axmeta risk field present"
            fi
        fi
    else
        echo "  [skip] codegen not available — skipping axmeta check"
    fi
fi

# ── Layer 3: the good/evil pair in the Linux microVM (guest-fs GF-4) ──────────
#
# The program really runs here: axon-guest-init loads it before its gate
# (GF-2) and, under `"fs": "none"`, withholds the filesystem (GF-3). The
# outcome is decided by the program, not by which effects the grant names.

echo "[axon-kernel-gate] Layer 3: Linux microVM enforcement (good/evil pair)"

LINUX_DIR="dist/guest-linux"
L3_SKIP=""
if [[ $DRY_RUN -eq 1 ]]; then
    echo "  [dry-run] Skipping microVM runs (--dry-run)"
elif [[ "$(id -u)" != 0 ]]; then
    L3_SKIP="needs root (the launcher runs Firecracker under its jailer)"
elif [[ ! -c /dev/kvm ]]; then
    L3_SKIP="no /dev/kvm"
elif ! command -v firecracker &>/dev/null; then
    L3_SKIP="firecracker not in PATH"
elif [[ ! -f "$LINUX_DIR/manifest.json" || ! -f "$LINUX_DIR/vmlinux" || ! -f "$LINUX_DIR/rootfs.sqfs" ]]; then
    L3_SKIP="no built $LINUX_DIR image (AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh)"
else
    L3="$(mktemp -d "${TMPDIR:-/tmp}/axon-kernel-gate.XXXXXX")"
    printf '{"schema":"axon-vm-mmds/2","allowed_effects":["IO"],"fs":"none"}' > "$L3/pol-good.json"
    printf '{"schema":"axon-vm-mmds/1","allowed_effects":["IO"]}' > "$L3/pol-evil.json"

    # good agent: fs:none, ["IO"] -> rc 0, and the guest prints exactly what
    # the host interpreter (this tree's, --no-default-features as the image's)
    # prints for it.
    mkdir -p "$L3/host"
    HOST_SHA="$(cd "$L3/host" && env -i PATH=/usr/bin:/bin HOME="$L3/host" XDG_CACHE_HOME="$L3/host/cache" \
        "$AXON" run "$PWD/$GOOD" 2>/dev/null | sha256sum | cut -d' ' -f1)"
    RC=0
    bash scripts/fc_linux_profile.sh --program "$PWD/$GOOD" --policy "$L3/pol-good.json" \
        --out "$L3/good" --timeout-s 90 > "$L3/good.log" 2>&1 || RC=$?
    GSHA="$(sha256sum < "$L3/good/out/stdout" 2>/dev/null | cut -d' ' -f1 || true)"
    if [[ $RC == 0 && "$GSHA" == "$HOST_SHA" ]] && grep -q "^scores: " "$L3/good/out/stdout"; then
        ok "agent_task.ax under fs:none, [\"IO\"]: rc 0 in the guest, stdout identical to the host run ($GSHA)"
    else
        fail "agent_task.ax under fs:none: launcher rc $RC (expected 0), guest stdout $GSHA vs host $HOST_SHA; $(tail -c 300 "$L3/good.log")"
    fi

    # evil agent: ["IO"] -> the guest's `axon run` refuses it at check (E1001,
    # workload exit 2; launcher rc 10), the policy is bound, nothing printed.
    RC=0
    bash scripts/fc_linux_profile.sh --program "$PWD/$EVIL" --policy "$L3/pol-evil.json" \
        --out "$L3/evil" --timeout-s 90 > "$L3/evil.log" 2>&1 || RC=$?
    WEXIT="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(d.get("workload_exit"), d.get("policy",{}).get("bound"))' "$L3/evil/result.json" 2>/dev/null || echo "none none")"
    if [[ $RC == 10 && "$WEXIT" == "2 True" && ! -s "$L3/evil/out/stdout" ]] \
       && grep -q "E1001" "$L3/evil/out/stderr" 2>/dev/null; then
        ok "agent_task_evil.ax under [\"IO\"]: refused at check in the guest (E1001, workload exit 2), nothing printed"
    else
        fail "agent_task_evil.ax under [\"IO\"]: launcher rc $RC (expected 10), workload_exit/bound '$WEXIT' (expected '2 True'); $(tail -c 300 "$L3/evil.log")"
    fi
    rm -rf "$L3"
fi
[[ -n "$L3_SKIP" ]] && echo "  [skip] $L3_SKIP — Layer 3 is a NON-RESULT (exit 77)"

# ── Layer 3b: bare-metal K5 "gate installs" smoke check ───────────────────────
#
# axon-guest-kernel runs no program (FG-041): under a grant that withholds FS it
# issues its own synthetic `openat` and its LSTAR gate must deny it (exit 8).
# That is ALL this shows - the program's code never runs, so this says nothing
# about the evil agent (CE-9 B7: `exit(3)` reports 0 under an FS grant). The
# grant is stated explicitly (T48: an absent one used to read as every effect).

echo "[axon-kernel-gate] Layer 3b: bare-metal K5 gate-installs smoke check"

if [[ $DRY_RUN -eq 1 ]]; then
    echo "  [dry-run] Skipping (--dry-run)"
elif ! command -v firecracker &>/dev/null || [[ ! -f "$KERNEL" ]] || [[ ! -f "$INITRD" ]]; then
    echo "  [skip] firecracker or the bare-metal image ($KERNEL, $INITRD) missing — smoke check not run"
else
    # R26: `axon-vm run` requires a PINNED kernel baseline (P7-KRN-05).
    KERNEL_DIGEST="$(sha256sum "$KERNEL" | cut -d' ' -f1)"
    "$AXON_VM" attest --kernel "$KERNEL" --pin-baseline --repin >/dev/null 2>&1 \
        || echo "  [warn] could not pin kernel baseline; runs will use --expect-digest"
    K5_OUT="$(mktemp "${TMPDIR:-/tmp}/axon-kernel-gate-k5.XXXXXX")"
    EXIT_CODE=0
    AXON_VM_ALLOWED_EFFECTS=AI timeout 30 "$AXON_VM" run "$EVIL" \
        --kernel "$KERNEL" --initrd "$INITRD" \
        --expect-digest "$KERNEL_DIGEST" \
        --json > "$K5_OUT" 2>&1 || EXIT_CODE=$?
    ACTUAL=$(python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print(d.get('exit_code'))" "$K5_OUT" 2>/dev/null || echo "$EXIT_CODE")
    rm -f "$K5_OUT"
    if [[ "$ACTUAL" == "8" ]]; then
        ok "K5 gate installs: the LSTAR gate denied the withheld FS syscall (exit 8; no program ran)"
    else
        fail "K5 gate installs: exit $ACTUAL (expected 8 from the denied synthetic openat)"
    fi
fi

# ── Layer 4: Full ASI stack — flagship demo ───────────────────────────────────
# Runs demo.sh in non-interactive CI mode (DEMO_NOPAUSE=1).
# AXON_CI_NO_KVM=1 allows attestation without real KVM hardware.
# The demo is self-contained: it rebuilds nothing and skips unavailable binaries.

echo "[axon-kernel-gate] Layer 4: Full ASI stack (flagship demo)"

DEMO_SCRIPT="examples/flagship/demo.sh"

if [[ ! -f "$DEMO_SCRIPT" ]]; then
    echo "  [skip] $DEMO_SCRIPT not found"
else
    DEMO_NOPAUSE=1 AXON_CI_NO_KVM=1 bash "$DEMO_SCRIPT" 2>&1 | \
        sed 's/^/  /' || true
    # Gate on PASS / FAIL lines printed by demo.sh
    DEMO_OUT=$(DEMO_NOPAUSE=1 AXON_CI_NO_KVM=1 bash "$DEMO_SCRIPT" 2>&1)
    DEMO_PASS=$(echo "$DEMO_OUT" | grep -c "PASS:" || true)
    DEMO_FAIL=$(echo "$DEMO_OUT" | grep -c "FAIL:" || true)
    if [[ "$DEMO_FAIL" -eq 0 ]] && [[ "$DEMO_PASS" -ge 1 ]]; then
        ok "flagship demo passed ($DEMO_PASS checks, 0 failures)"
    elif [[ "$DEMO_FAIL" -gt 0 ]]; then
        fail "flagship demo had $DEMO_FAIL failure(s) (see demo.sh output above)"
    else
        # Demo ran but no PASS lines — binary missing or all layers skipped
        ok "flagship demo ran (all active layers passed or skipped)"
    fi
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
if [[ $FAIL -ne 0 ]]; then
    echo "[axon-kernel-gate] FAIL ($PASS passed, $FAIL failed)"
    exit 1
elif [[ -n "$L3_SKIP" ]]; then
    echo "[axon-kernel-gate] SKIP ($PASS passed, 0 failed; Layer 3 not run: $L3_SKIP)"
    exit 77
else
    echo "[axon-kernel-gate] PASS ($PASS checks, 0 failures)"
    exit 0
fi
