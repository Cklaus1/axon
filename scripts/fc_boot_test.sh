#!/usr/bin/env bash
# scripts/fc_boot_test.sh — Boot axon-guest-kernel in Firecracker v1.10.1
#
# Usage:
#   ./scripts/fc_boot_test.sh [--policy BASE64_JSON]
#
# Reports the serial output from the guest, including:
#   params=0xXXXXXX  (non-zero = Firecracker passed real boot_params)
#   principal=NAME   (from axon.policy= cmdline, default "root")
#
# Firecracker serial port → Firecracker stdout → captured to a temp file.
# The VM runs until it prints "ready" or the timeout (8 s) fires.

set -euo pipefail
cd "$(dirname "$0")/.."

KERNEL="$(pwd)/target/x86_64-axon-metal/release/axon-guest-kernel"

if [[ ! -f "$KERNEL" ]]; then
    echo "ERROR: kernel not found at $KERNEL" >&2
    echo "Build with:" >&2
    echo "  cargo +nightly build -p axon-guest-kernel \\" >&2
    echo "    --target \"\$(pwd)/crates/axon-guest-kernel/targets/x86_64-axon-metal.json\" \\" >&2
    echo "    -Z build-std=core,compiler_builtins \\" >&2
    echo "    -Z build-std-features=compiler-builtins-mem \\" >&2
    echo "    -Z json-target-spec --release" >&2
    exit 1
fi

# ── Parse arguments ────────────────────────────────────────────────────────────

POLICY_B64=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --policy)   POLICY_B64="$2"; shift 2 ;;
        *)          echo "Unknown arg: $1" >&2; exit 1 ;;
    esac
done

# ── Build boot_args ────────────────────────────────────────────────────────────

BOOT_ARGS="console=ttyS0 reboot=k panic=1"
if [[ -n "$POLICY_B64" ]]; then
    BOOT_ARGS="$BOOT_ARGS axon.policy=$POLICY_B64"
fi

# ── Temp files ─────────────────────────────────────────────────────────────────

SOCK="/tmp/axon-fc-boot-$$.sock"
SERIAL_LOG="/tmp/axon-fc-serial-$$.log"
FC_LOG="/tmp/axon-fc-log-$$.log"     # Firecracker internal logger output (--log-path)
FC_ERR="/tmp/axon-fc-err-$$.log"     # Firecracker process stderr (crashes only)
FC_PID=""

cleanup() {
    if [[ -n "$FC_PID" ]]; then
        kill "$FC_PID" 2>/dev/null || true
        wait "$FC_PID" 2>/dev/null || true
    fi
    rm -f "$SOCK" "$FC_LOG" "$FC_ERR"
}
trap cleanup EXIT

# ── Start Firecracker ──────────────────────────────────────────────────────────

echo "[fc_boot_test] Starting Firecracker (socket=$SOCK)"
touch "$FC_LOG"   # Firecracker requires the log file to already exist
/usr/local/bin/firecracker \
    --api-sock "$SOCK" \
    --no-seccomp \
    --log-path "$FC_LOG" \
    >"$SERIAL_LOG" 2>"$FC_ERR" &
FC_PID=$!

# ── Wait for API socket ────────────────────────────────────────────────────────

DEADLINE=$((SECONDS + 5))
while [[ ! -S "$SOCK" ]]; do
    if ! kill -0 "$FC_PID" 2>/dev/null; then
        echo "ERROR: Firecracker exited early:" >&2
        cat "$FC_ERR" >&2
        exit 1
    fi
    if [[ $SECONDS -ge $DEADLINE ]]; then
        echo "ERROR: Firecracker socket not ready after 5 s" >&2
        cat "$FC_ERR" >&2
        exit 1
    fi
    sleep 0.05
done
echo "[fc_boot_test] Firecracker API socket ready"

# ── Firecracker API: PUT helper (curl over Unix socket) ────────────────────────

fc_put() {
    local endpoint="$1"
    local body="$2"
    local resp
    resp=$(curl -s -w "\nHTTP_STATUS:%{http_code}" \
        --unix-socket "$SOCK" \
        -X PUT \
        -H 'Content-Type: application/json' \
        -H 'Accept: */*' \
        -d "$body" \
        "http://localhost${endpoint}")
    local status
    status=$(echo "$resp" | grep "^HTTP_STATUS:" | cut -d: -f2)
    local body_resp
    body_resp=$(echo "$resp" | grep -v "^HTTP_STATUS:")
    if [[ "$status" -lt 200 ]] || [[ "$status" -ge 300 ]]; then
        echo "ERROR: PUT $endpoint returned HTTP $status" >&2
        echo "  response: $body_resp" >&2
        kill "$FC_PID" 2>/dev/null || true
        exit 1
    fi
    echo "[fc_boot_test] PUT $endpoint → $status"
}

# ── Configure VM ───────────────────────────────────────────────────────────────

fc_put /boot-source \
    "{\"kernel_image_path\": \"$KERNEL\", \"boot_args\": \"$BOOT_ARGS\"}"

fc_put /machine-config \
    '{"vcpu_count": 1, "mem_size_mib": 128, "track_dirty_pages": false}'

# ── Start VM ───────────────────────────────────────────────────────────────────

fc_put /actions '{"action_type": "InstanceStart"}'
echo "[fc_boot_test] VM started — waiting for 'ready' output (timeout 8 s)"

# ── Wait for "ready" line in serial output ─────────────────────────────────────

WAIT_DEADLINE=$((SECONDS + 8))
while [[ $SECONDS -lt $WAIT_DEADLINE ]]; do
    if grep -q "ready" "$SERIAL_LOG" 2>/dev/null; then
        # Give it a moment to flush the last line
        sleep 0.2
        break
    fi
    if ! kill -0 "$FC_PID" 2>/dev/null; then
        echo "[fc_boot_test] Firecracker exited"
        break
    fi
    sleep 0.1
done

# ── Report serial output ───────────────────────────────────────────────────────

echo ""
echo "=== Firecracker serial output ==="
cat "$SERIAL_LOG" 2>/dev/null || echo "(empty)"
echo "==================================="

# ── Verify expected lines ──────────────────────────────────────────────────────

PASS=0; FAIL=0
check() {
    local desc="$1"; local pattern="$2"
    if grep -q "$pattern" "$SERIAL_LOG" 2>/dev/null; then
        echo "  PASS: $desc"
        PASS=$((PASS+1))
    else
        echo "  FAIL: $desc (pattern: $pattern)"
        FAIL=$((FAIL+1))
    fi
}

echo ""
echo "=== Checks ==="
check "boot ok"                  "\[axon-kernel\] boot ok"
check "params non-zero"          "params=0x[1-9a-f]"
check "heap ok"                  "\[axon-kernel\] heap ok"
check "policy ok"                "\[axon-kernel\] policy ok"
check "enforce gate active"      "enforce: gate active"
check "syscall gate"             "syscall gate active"
check "hypercall substrate"      "hypercall substrate active"
check "ready"                    "ready.*waiting"

if [[ -n "$POLICY_B64" ]]; then
    check "custom principal (not root)" "principal=test"
fi

echo ""
if [[ $FAIL -eq 0 ]]; then
    echo "RESULT: PASS ($PASS/$((PASS+FAIL)) checks)"
    exit 0
else
    echo "RESULT: FAIL ($FAIL/$((PASS+FAIL)) checks failed)"
    # Show Firecracker logs for diagnosis
    if [[ -s "$FC_LOG" ]]; then
        echo ""
        echo "=== Firecracker log (last 40 lines) ==="
        tail -40 "$FC_LOG"
    fi
    if [[ -s "$FC_ERR" ]]; then
        echo ""
        echo "=== Firecracker stderr ==="
        cat "$FC_ERR"
    fi
    exit 1
fi
