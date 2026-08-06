#!/usr/bin/env bash
# R34 acceptance gate — incremental attestation rolling hash chain.
# Validates: the chain-formula tests exist and are real (not stubs), the unit
# suite passes, R31/R26 don't regress, and the real CLI (`axon-vm chain
# stamp`/`chain verify`) demonstrates stamp → verify OK → tamper → BROKEN
# (exit 15) → wrong-genesis → BROKEN (exit 15).
#
# Exit 0 = all checks green. Any failure prints which check failed and exits
# non-zero.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FAIL=0

pass() { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; FAIL=1; }

echo "=== R34 acceptance gate ==="

CHAIN_SRC="$REPO_ROOT/crates/axon-vm/src/chain.rs"
VM_SRC="$REPO_ROOT/crates/axon-vm/src/main.rs"
CHAIN_VERIFY_FAIL_EXIT_CODE=15

# ── 1. Presence check: every named unit test from R34's Gate 4 case list ────
echo ""
echo "1. Normative test name presence check"

REQUIRED_NAMES=(
    "entry_hash_deterministic"
    "different_prog_hash_different_entry_hash"
    "chain_composes_with_r31"
    "verify_ok_three_entries"
    "verify_detects_tampered_entry_hash"
    "verify_empty_chain_ok"
    "verify_wrong_genesis_breaks_at_zero"
    "sha256_known_test_vector"
    "verify_malformed_json_line_is_clear_error"
)

for name in "${REQUIRED_NAMES[@]}"; do
    if grep -q "fn $name" "$CHAIN_SRC" 2>/dev/null; then
        pass "found: $name"
    else
        fail "MISSING test name: $name (check $CHAIN_SRC)"
    fi
done

# ── 2. Anti-stub check: no todo!/unimplemented!/assert!(true) in chain.rs ───
echo ""
echo "2. Anti-stub check"

STUB_VIOLATIONS=$(grep -n 'todo!\|unimplemented!\|assert!(true)' "$CHAIN_SRC" 2>/dev/null || true)
if [ -z "$STUB_VIOLATIONS" ]; then
    pass "no stub patterns (todo!/unimplemented!/assert!(true)) found in chain.rs"
else
    fail "stub patterns found — tests must not be stubbed:
$STUB_VIOLATIONS"
fi

# ── 3. Unit test suite (chain.rs) ────────────────────────────────────────────
echo ""
echo "3. Unit test suite: cargo test -p axon-vm chain::"

if cargo test -p axon-vm --no-default-features --quiet chain:: 2>&1 | tee /tmp/r34_chain_tests.log | tail -5; then
    if grep -q "test result: ok" /tmp/r34_chain_tests.log; then
        pass "cargo test -p axon-vm chain::  — all chain tests pass"
    else
        fail "cargo test -p axon-vm chain::  — did not report 'test result: ok'"
    fi
else
    fail "cargo test -p axon-vm chain::  — FAILED"
fi

# ── 4. R31/R26 regression check (axon-attest untouched by R34) ─────────────
echo ""
echo "4. R31/R26 regression check"

if cargo test -p axon-attest --quiet 2>&1 | tail -5; then
    pass "cargo test -p axon-attest — all pass (no R34 regression of R26/R31)"
else
    fail "cargo test -p axon-attest — FAILED"
fi

# ── 5. Build the real axon-vm CLI ────────────────────────────────────────────
echo ""
echo "5. Build axon-vm"

if cargo build -p axon-vm --no-default-features --quiet 2>&1 | tail -20; then
    pass "cargo build -p axon-vm succeeded"
else
    fail "cargo build -p axon-vm FAILED"
fi

AXON_VM_BIN="$REPO_ROOT/target/debug/axon-vm"
if [ ! -x "$AXON_VM_BIN" ]; then
    fail "axon-vm binary not found at $AXON_VM_BIN"
fi

# ── 6. Real CLI journey: stamp x2, verify OK, corrupt, verify BROKEN,
#       wrong-genesis, verify BROKEN ─────────────────────────────────────────
echo ""
echo "6. Real CLI journey (AXON_CI_NO_KVM=1 mock genesis — no real hardware needed)"

TMPDIR_R34="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_R34"' EXIT
CHAIN_FILE="$TMPDIR_R34/chain.jsonl"
PROG="$REPO_ROOT/examples/hello.ax"

export AXON_CI_NO_KVM=1

# Stamp #1
H1="$("$AXON_VM_BIN" chain stamp --prog "$PROG" --store "$CHAIN_FILE" 2>/tmp/r34_stamp1.err)"
if [ -n "$H1" ] && [[ "$H1" == axtcb1-run:* ]]; then
    pass "stamp #1 produced a tip: $H1"
else
    fail "stamp #1 did not produce an axtcb1-run: tip (got: '$H1'); stderr: $(cat /tmp/r34_stamp1.err)"
fi

# Stamp #2
H2="$("$AXON_VM_BIN" chain stamp --prog "$PROG" --store "$CHAIN_FILE" 2>/tmp/r34_stamp2.err)"
if [ -n "$H2" ] && [[ "$H2" == axtcb1-run:* ]] && [ "$H2" != "$H1" ]; then
    pass "stamp #2 produced a different tip: $H2"
else
    fail "stamp #2 did not produce a fresh distinct tip (got: '$H2', prev: '$H1')"
fi

LINE_COUNT="$(wc -l < "$CHAIN_FILE" | tr -d ' ')"
if [ "$LINE_COUNT" = "2" ]; then
    pass "chain file has exactly 2 entries after 2 stamps"
else
    fail "chain file has $LINE_COUNT lines, expected 2"
fi

# Verify (self-consistent — no --genesis pin; should be OK: 2 entries, exit 0)
VERIFY_OUT="$("$AXON_VM_BIN" chain verify --store "$CHAIN_FILE" 2>&1)"
VERIFY_EXIT=$?
if [ "$VERIFY_EXIT" = "0" ] && echo "$VERIFY_OUT" | grep -q "CHAIN OK: 2 entries"; then
    pass "chain verify (2 genuine entries) → '$VERIFY_OUT', exit 0"
else
    fail "chain verify (genuine chain) → exit $VERIFY_EXIT, output: '$VERIFY_OUT' (expected CHAIN OK: 2 entries, exit 0)"
fi

# Corrupt seq 1's entry_hash (flip a hex char), re-verify → BROKEN, exit 15
cp "$CHAIN_FILE" "$CHAIN_FILE.bak"
python3 - "$CHAIN_FILE" <<'PYEOF'
import json, sys
path = sys.argv[1]
with open(path) as f:
    lines = [l for l in f.read().splitlines() if l.strip()]
entry = json.loads(lines[1])
h = entry["entry_hash"]
last = h[-1]
entry["entry_hash"] = h[:-1] + ("0" if last != "0" else "1")
lines[1] = json.dumps(entry)
with open(path, "w") as f:
    f.write("\n".join(lines) + "\n")
PYEOF

CORRUPT_OUT="$("$AXON_VM_BIN" chain verify --store "$CHAIN_FILE" 2>&1)"
CORRUPT_EXIT=$?
if [ "$CORRUPT_EXIT" = "$CHAIN_VERIFY_FAIL_EXIT_CODE" ] && echo "$CORRUPT_OUT" | grep -q "CHAIN BROKEN at seq 1"; then
    pass "tampered chain → '$CORRUPT_OUT', exit $CORRUPT_EXIT (expected $CHAIN_VERIFY_FAIL_EXIT_CODE)"
else
    fail "tampered chain → exit $CORRUPT_EXIT, output: '$CORRUPT_OUT' (expected CHAIN BROKEN at seq 1, exit $CHAIN_VERIFY_FAIL_EXIT_CODE)"
fi

# Wrong-genesis case: restore the genuine chain, verify with a deliberately
# wrong --genesis pin → BROKEN at seq 0, exit 15.
cp "$CHAIN_FILE.bak" "$CHAIN_FILE"
WRONG_GENESIS="axtcb1-ext:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
WRONG_OUT="$("$AXON_VM_BIN" chain verify --store "$CHAIN_FILE" --genesis "$WRONG_GENESIS" 2>&1)"
WRONG_EXIT=$?
if [ "$WRONG_EXIT" = "$CHAIN_VERIFY_FAIL_EXIT_CODE" ] && echo "$WRONG_OUT" | grep -q "CHAIN BROKEN at seq 0"; then
    pass "wrong-genesis verify → '$WRONG_OUT', exit $WRONG_EXIT (expected $CHAIN_VERIFY_FAIL_EXIT_CODE)"
else
    fail "wrong-genesis verify → exit $WRONG_EXIT, output: '$WRONG_OUT' (expected CHAIN BROKEN at seq 0, exit $CHAIN_VERIFY_FAIL_EXIT_CODE)"
fi

# ── 7. `axon-vm run --chain-stamp` gates the VM launch on chain verification ─
echo ""
echo "7. run --chain-stamp refuses to launch on a broken chain"

# The chain file at $CHAIN_FILE is currently genuine (restored above) but the
# next check needs a BROKEN one to prove the VM never spawns. Corrupt it again.
python3 - "$CHAIN_FILE" <<'PYEOF'
import json, sys
path = sys.argv[1]
with open(path) as f:
    lines = [l for l in f.read().splitlines() if l.strip()]
entry = json.loads(lines[0])
h = entry["entry_hash"]
last = h[-1]
entry["entry_hash"] = h[:-1] + ("0" if last != "0" else "1")
lines[0] = json.dumps(entry)
with open(path, "w") as f:
    f.write("\n".join(lines) + "\n")
PYEOF

# `run` needs a kernel/initrd file to exist before it even reaches the chain
# gate (both are pre-flight-checked). Use the repo's real dist/guest assets
# if present; else this sub-check is skipped with a clear note (still counts
# as pass/fail neutral — CLI plumbing already proved above via chain verify).
KERNEL_DEFAULT="$REPO_ROOT/dist/guest/vmlinuz"
INITRD_DEFAULT="$REPO_ROOT/dist/guest/initramfs.cpio"
if [ -f "$KERNEL_DEFAULT" ] && { [ -f "$INITRD_DEFAULT" ] || [ -f "$INITRD_DEFAULT.gz" ]; }; then
    RUN_OUT="$("$AXON_VM_BIN" run "$PROG" --chain-stamp "$CHAIN_FILE" --no-attest 2>&1)"
    RUN_EXIT=$?
    if [ "$RUN_EXIT" = "$CHAIN_VERIFY_FAIL_EXIT_CODE" ] && echo "$RUN_OUT" | grep -q "CHAIN BROKEN"; then
        pass "run --chain-stamp refused a broken chain before VM launch, exit $RUN_EXIT"
    else
        fail "run --chain-stamp → exit $RUN_EXIT, output: '$RUN_OUT' (expected CHAIN BROKEN, exit $CHAIN_VERIFY_FAIL_EXIT_CODE)"
    fi
else
    echo "  (skipped: dist/guest/{vmlinuz,initramfs.cpio} not built in this environment —"
    echo "   the CLI-level chain-stamp/verify path is already proven by checks 6 above;"
    echo "   this sub-check only additionally exercises the run-command preflight wiring)"
fi

# ── 8. Slice 6: `chain show`/`export`/`verify-export` CLI subcommands ───────
echo ""
echo "8. Slice 6: chain show/export/verify-export real CLI journey"

CHAIN6="$TMPDIR_R34/chain6.jsonl"
EXPORT6="$TMPDIR_R34/export6.json"

SHOW_EMPTY="$("$AXON_VM_BIN" chain show --store "$CHAIN6" --vm-id gate6 --json 2>/tmp/r34_show1.err)"
if echo "$SHOW_EMPTY" | grep -q '"entries":0' && echo "$SHOW_EMPTY" | grep -q '"vm_id":"gate6"'; then
    pass "chain show (empty store) → entries:0, vm_id present: $SHOW_EMPTY"
else
    fail "chain show (empty store) → unexpected output: '$SHOW_EMPTY'; stderr: $(cat /tmp/r34_show1.err)"
fi

"$AXON_VM_BIN" chain stamp --prog "$PROG" --store "$CHAIN6" >/dev/null 2>&1
"$AXON_VM_BIN" chain stamp --prog "$REPO_ROOT/examples/math.ax" --store "$CHAIN6" >/dev/null 2>&1
"$AXON_VM_BIN" chain stamp --prog "$REPO_ROOT/examples/structs.ax" --store "$CHAIN6" >/dev/null 2>&1

SHOW_3="$("$AXON_VM_BIN" chain show --store "$CHAIN6" --vm-id gate6 --json 2>/tmp/r34_show2.err)"
if echo "$SHOW_3" | grep -q '"entries":3' && echo "$SHOW_3" | grep -q '"head":"axtcb1-run:'; then
    pass "chain show (3 entries) → entries:3, head is an axtcb1-run: tip: $SHOW_3"
else
    fail "chain show (3 entries) → unexpected output: '$SHOW_3'; stderr: $(cat /tmp/r34_show2.err)"
fi

EXPORT_OUT="$("$AXON_VM_BIN" chain export --store "$CHAIN6" --out "$EXPORT6" --vm-id gate6 2>&1)"
if [ -f "$EXPORT6" ] && grep -q '"schema": "axon-chain-export/1"' "$EXPORT6" && grep -q '"entries": \[' "$EXPORT6"; then
    pass "chain export wrote a valid axon-chain-export/1 file: $EXPORT_OUT"
else
    fail "chain export did not produce a valid export file: '$EXPORT_OUT'"
fi

VERIFY_EXPORT_OK="$("$AXON_VM_BIN" chain verify-export "$EXPORT6" 2>&1)"
VERIFY_EXPORT_OK_EXIT=$?
if [ "$VERIFY_EXPORT_OK_EXIT" = "0" ] && echo "$VERIFY_EXPORT_OK" | grep -q "EXPORT OK: 3 entries"; then
    pass "chain verify-export (genuine) → '$VERIFY_EXPORT_OK', exit 0"
else
    fail "chain verify-export (genuine) → exit $VERIFY_EXPORT_OK_EXIT, output: '$VERIFY_EXPORT_OK' (expected EXPORT OK: 3 entries, exit 0)"
fi

# Tamper the exported head field only (every individual link still recomputes
# cleanly) — proves verify-export's head-tamper check, not just per-link
# verification (the same class of check acc_a2 exercises for chain verify).
python3 - "$EXPORT6" <<'PYEOF'
import json, sys
path = sys.argv[1]
with open(path) as f:
    d = json.load(f)
h = d["head"]
d["head"] = h[:-1] + ("0" if h[-1] != "0" else "1")
with open(path, "w") as f:
    json.dump(d, f)
PYEOF

VERIFY_EXPORT_BROKEN="$("$AXON_VM_BIN" chain verify-export "$EXPORT6" 2>&1)"
VERIFY_EXPORT_BROKEN_EXIT=$?
if [ "$VERIFY_EXPORT_BROKEN_EXIT" = "$CHAIN_VERIFY_FAIL_EXIT_CODE" ] && echo "$VERIFY_EXPORT_BROKEN" | grep -q "EXPORT BROKEN at seq 3"; then
    pass "chain verify-export (tampered head only) → '$VERIFY_EXPORT_BROKEN', exit $VERIFY_EXPORT_BROKEN_EXIT"
else
    fail "chain verify-export (tampered head) → exit $VERIFY_EXPORT_BROKEN_EXIT, output: '$VERIFY_EXPORT_BROKEN' (expected EXPORT BROKEN at seq 3, exit $CHAIN_VERIFY_FAIL_EXIT_CODE)"
fi

# ── Final result ──────────────────────────────────────────────────────────────
echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "=== R34 acceptance gate: ALL CHECKS PASSED ==="
    exit 0
else
    echo "=== R34 acceptance gate: FAILED (see above) ==="
    exit 1
fi
