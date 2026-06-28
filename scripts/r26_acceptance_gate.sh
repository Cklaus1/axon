#!/usr/bin/env bash
# r26_acceptance_gate.sh — R26 §10: the pinned acceptance gate for axon-attest + axon-vm attest.
#
# Single source of "done" for R26: Confidential Micro-VM Substrate.
# FAILS if any required §0 check is missing, stubbed, or not green.
# Proves: solver/HW-free verifier; deterministic measurement; mandatory attestation;
# axtcb1 chained; tamper detection; stand-in caveat honest.
#
# Run: scripts/r26_acceptance_gate.sh
# Wire into: gate.sh --strict
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CRATE_ATTEST="crates/axon-attest"
SRC_ATTEST="$CRATE_ATTEST/src/lib.rs"
SRC_VM="crates/axon-vm/src/main.rs"

fail=0

# ─ (1) Presence check ─────────────────────────────────────────────────────────
# Every named check from R26 §0 must exist in the source. Missing any → gate fails.
echo "r26_acceptance_gate: (1) presence check — all §0 named checks must exist…"

REQUIRED=(
  acc_a1_smoke_boot_attest_run_journey
  acc_a2_guest_image_runs_real_job
  acc_a3_quickstart_commands_execute
  acc_a4_device_surface_minimal_and_isolated
  acc_a5_measurement_and_record_byte_identical
  acc_a6_attestation_mandatory_and_chained
  attestation_mismatch_fails_closed
  vm_without_attestation_is_refused
  axtcb1_digest_chained_to_measurement
  tampered_guest_image_fails_attestation
)

for name in "${REQUIRED[@]}"; do
  if ! grep -qsF "fn $name" "$SRC_ATTEST"; then
    echo "  MISSING required check: $name"
    fail=1
  else
    echo "  ✓ found: $name"
  fi
done

# ─ (2) Anti-stub check ────────────────────────────────────────────────────────
# No acceptance test may be #[ignore]d or contain todo!()/unimplemented!()/assert!(true).
echo ""
echo "r26_acceptance_gate: (2) anti-stub check…"

if grep -qsE '#\[ignore\]|todo!\(\)|unimplemented!\(\)' "$SRC_ATTEST"; then
  echo "  FAIL: found #[ignore]/todo!()/unimplemented!() in $SRC_ATTEST"
  grep -nsE '#\[ignore\]|todo!\(\)|unimplemented!\(\)' "$SRC_ATTEST"
  fail=1
else
  echo "  ✓ no #[ignore]/todo!()/unimplemented!() stubs found"
fi

if grep -qsE 'assert!\(\s*true\s*\)' "$SRC_ATTEST"; then
  echo "  FAIL: found no-op assert!(true) stub in $SRC_ATTEST"
  grep -nsE 'assert!\(\s*true\s*\)' "$SRC_ATTEST"
  fail=1
else
  echo "  ✓ no no-op assert!(true) stubs found"
fi

# ─ (3) Solver/HW-free core check ──────────────────────────────────────────────
# The trusted verifier (verify_report in axon-attest) must contain no z3, no hw-attest-gated
# symbols. The CI lane must verify reports without real hardware (R23 discipline applied here).
echo ""
echo "r26_acceptance_gate: (3) solver/HW-free core check…"

# Check for actual code-level z3 or hw-attest usage (not doc comments).
# Strip comment lines (starting with // or //!) before grepping.
STRIPPED_ATTEST="$(grep -vE '^\s*//[/!]?' "$SRC_ATTEST")"
if echo "$STRIPPED_ATTEST" | grep -qE '\bz3\b|feature\s*=\s*"hw-attest"'; then
  echo "  FAIL: z3 or hw-attest feature-gate found in non-comment code in $SRC_ATTEST"
  echo "$STRIPPED_ATTEST" | grep -nE '\bz3\b|feature\s*=\s*"hw-attest"'
  fail=1
else
  echo "  ✓ no z3 or hw-attest feature-gates in verifier non-comment code"
fi

# Confirm z3 is not in the crate's dependency closure
if cargo tree -p axon-attest 2>/dev/null | grep -qi '\bz3\b'; then
  echo "  FAIL: z3 IS in axon-attest dependency closure (TCB breach)"
  fail=1
else
  echo "  ✓ z3 is not in axon-attest dependency closure"
fi

# ─ (4) Honesty check ──────────────────────────────────────────────────────────
# The stand-in path must print the "no memory encryption — use sev-snp/tdx" caveat
# so the gate fails if the stand-in is ever silently presented as the real property.
echo ""
echo "r26_acceptance_gate: (4) honesty check — stand-in caveat must be present in code…"

if grep -qsE 'no memory encryption|software-tpm|stand-in' "$SRC_ATTEST"; then
  echo "  ✓ stand-in caveat present in $SRC_ATTEST"
else
  echo "  FAIL: stand-in caveat ('no memory encryption', 'software-tpm', or 'stand-in') not found"
  fail=1
fi

if grep -qsE 'no memory encryption|software-tpm|stand-in' "$SRC_VM"; then
  echo "  ✓ stand-in caveat present in $SRC_VM (attest subcommand)"
else
  echo "  FAIL: stand-in caveat not found in axon-vm attest subcommand"
  fail=1
fi

# ─ (5) Build axon-attest ──────────────────────────────────────────────────────
echo ""
echo "r26_acceptance_gate: (5) build axon-attest…"
if ! cargo build -q -p axon-attest 2>&1; then
  echo "  FAIL: cargo build -p axon-attest failed"
  fail=1
else
  echo "  ✓ axon-attest builds cleanly"
fi

# ─ (6) Run axon-attest tests ──────────────────────────────────────────────────
echo ""
echo "r26_acceptance_gate: (6) cargo test -p axon-attest (all §0 checks)…"
export AXON_CI_NO_KVM=1
if cargo test -q -p axon-attest 2>&1; then
  echo "  ✓ all axon-attest tests pass"
else
  echo "  FAIL: axon-attest test suite failed"
  fail=1
fi

# ─ (7) axon-vm regression check ───────────────────────────────────────────────
echo ""
echo "r26_acceptance_gate: (7) cargo test -p axon-vm (regression — 11 existing tests)…"
if cargo test -q -p axon-vm 2>&1; then
  echo "  ✓ all axon-vm tests pass (no regression)"
else
  echo "  FAIL: axon-vm test suite regressed"
  fail=1
fi

# ─ (8) Build axon-vm binary ───────────────────────────────────────────────────
echo ""
echo "r26_acceptance_gate: (8) build axon-vm binary…"
if ! cargo build -q -p axon-vm --bin axon-vm 2>&1; then
  echo "  FAIL: cargo build -p axon-vm failed"
  fail=1
else
  echo "  ✓ axon-vm binary builds cleanly"
fi

# ─ (9) CLI smoke: axon-vm attest (CI/mock mode) ───────────────────────────────
echo ""
echo "r26_acceptance_gate: (9) CLI smoke — axon-vm attest in AXON_CI_NO_KVM=1 mode…"
AXON_VM="$ROOT/target/debug/axon-vm"

if [ ! -x "$AXON_VM" ]; then
  echo "  SKIP: axon-vm binary not found at $AXON_VM"
else
  export AXON_CI_NO_KVM=1

  # A: attest with a synthetic kernel path (CI mode: file need not exist)
  ATTEST_OUT="$(AXON_CI_NO_KVM=1 "$AXON_VM" attest --kernel /nonexistent/mock-kernel 2>/dev/null)"
  if echo "$ATTEST_OUT" | grep -q '"ok": true'; then
    echo "  ✓ axon-vm attest (mock) produced ok:true"
  else
    echo "  FAIL: axon-vm attest (mock) did not produce ok:true"
    echo "  output: $ATTEST_OUT"
    fail=1
  fi

  # B: schema field present
  if echo "$ATTEST_OUT" | grep -q '"schema": "axon-attest/1"'; then
    echo "  ✓ schema: axon-attest/1 present"
  else
    echo "  FAIL: schema axon-attest/1 missing from output"
    fail=1
  fi

  # C: axtcb1: prefix in report measurement
  if echo "$ATTEST_OUT" | grep -q '"axtcb1": "axtcb1:'; then
    echo "  ✓ axtcb1: prefix present in report measurement"
  else
    echo "  FAIL: axtcb1: prefix missing from report measurement"
    fail=1
  fi

  # D: stand-in caveat on stderr (piped, check separately)
  ATTEST_STDERR="$(AXON_CI_NO_KVM=1 "$AXON_VM" attest --kernel /nonexistent/mock-kernel 2>&1 >/dev/null)"
  if echo "$ATTEST_STDERR" | grep -qi 'stand-in\|no memory encryption\|software-tpm'; then
    echo "  ✓ stand-in caveat printed to stderr"
  else
    echo "  FAIL: stand-in caveat not found on stderr"
    echo "  stderr: $ATTEST_STDERR"
    fail=1
  fi
fi

# ─ (10) Reproducibility — measure_kernel_bytes is deterministic ───────────────
echo ""
echo "r26_acceptance_gate: (10) reproducibility check (acc_a5)…"
if cargo test -q -p axon-attest -- acc_a5_measurement_and_record_byte_identical 2>&1; then
  echo "  ✓ acc_a5_measurement_and_record_byte_identical passes"
else
  echo "  FAIL: reproducibility test failed"
  fail=1
fi

# ─ Summary ────────────────────────────────────────────────────────────────────
echo ""
if [ "$fail" -eq 0 ]; then
  echo "r26_acceptance_gate: OK — all R26 §0 checks present, unstubbed, and green."
  echo "  Attested: software-TPM (stand-in lane); real SEV-SNP/TDX requires hw-attest feature."
  exit 0
else
  echo "r26_acceptance_gate: FAILED — one or more checks did not pass."
  echo "  Review the output above for the specific failures."
  exit 1
fi
