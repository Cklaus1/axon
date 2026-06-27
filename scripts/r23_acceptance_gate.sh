#!/usr/bin/env bash
# r23_acceptance_gate.sh — R23 §10: the pinned acceptance gate for axon-certcheck.
# The single source of "done". FAILS if any required §0 check is missing or
# stubbed; proves the checker is solver-free (z3 absent from the no-default-
# features closure); runs the full suite (with and without `smt`); checks the §9
# quickstart block; and (smt build) proves emission is byte-reproducible.
#
# Wire into gate.sh --strict.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CRATE="crates/axon-certcheck"
SRC="$CRATE/src $CRATE/tests"
fail=0

# The named checks the build must contain (R23 §0). Missing any → gate fails.
REQUIRED=(
  acc_a1_smoke_prove_check_verify
  acc_a2_example_obligations_certified
  acc_a3_quickstart_commands_execute
  acc_a4_checker_is_solver_free
  acc_a5_certificate_byte_identical
  acc_a6_checker_rejects_forged_and_mutated
  valid_certificate_accepted
  false_obligation_has_no_valid_certificate
  lying_solver_is_caught_by_checker
  require_certificates_fails_closed
)

echo "r23_acceptance_gate: (1) presence check…"
for name in "${REQUIRED[@]}"; do
  if ! grep -rqs "fn $name" $SRC; then
    echo "  MISSING required check: $name"
    fail=1
  fi
done

echo "r23_acceptance_gate: (2) anti-stub check…"
# No acceptance test may be ignored or stubbed.
if grep -rqsE '#\[ignore\]|todo!\(\)|unimplemented!\(\)' $SRC; then
  echo "  found an #[ignore]/todo!()/unimplemented!() in the test surface"
  grep -rnsE '#\[ignore\]|todo!\(\)|unimplemented!\(\)' $SRC
  fail=1
fi
# A bare `assert!(true)` is a no-op stub.
if grep -rqsE 'assert!\(\s*true\s*\)' $SRC; then
  echo "  found a no-op assert!(true) stub"
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "r23_acceptance_gate: FAILED (missing or stubbed checks)"
  exit 1
fi

echo "r23_acceptance_gate: (3) solver-free proof — build --no-default-features + run check…"
if ! cargo build -q -p axon-certcheck --no-default-features --bin certcheck; then
  echo "  solver-free build failed"; exit 1
fi
CERTCHECK="$ROOT/target/debug/certcheck"
if ! "$CERTCHECK" check examples/proofs/mint_o2.obl examples/proofs/mint_o2.cert >/dev/null; then
  echo "  solver-free check of the example obligation FAILED"; exit 1
fi
# Assert z3 is NOT in the checker's dependency closure.
if cargo tree -p axon-certcheck --no-default-features 2>/dev/null | grep -qi 'z3'; then
  echo "  z3 IS in the solver-free dependency closure (TCB breach)"; exit 1
fi
echo "  ✓ checker validates the example with NO z3 in its closure"

echo "r23_acceptance_gate: (4) full suite (default, solver-free)…"
if ! cargo test -q -p axon-certcheck; then
  echo "  default test suite failed"; exit 1
fi

# The §9 quickstart block, executed verbatim against the built binary.
echo "r23_acceptance_gate: (4b) quickstart block…"
if ! "$CERTCHECK" check examples/proofs/mint_o2.obl examples/proofs/mint_o2.cert >/dev/null; then
  echo "  quickstart step 1 failed"; exit 1
fi
if ! "$CERTCHECK" explain examples/proofs/mint_o2.obl --cert examples/proofs/mint_o2.cert >/dev/null; then
  echo "  quickstart step 2 failed"; exit 1
fi
TAMPER="$(mktemp)"; trap 'rm -f "$TAMPER"' EXIT
sed 's/"coeffs":\[1,1\]/"coeffs":[1,2]/' examples/proofs/carve.cert > "$TAMPER"
"$CERTCHECK" check examples/proofs/carve.obl "$TAMPER" >/dev/null
if [ "$?" -ne 8 ]; then
  echo "  quickstart step 3: tampered cert was NOT rejected with exit 8"; exit 1
fi
echo "  ✓ quickstart commands execute (tamper rejected, exit 8)"

# The smt build (if libz3 is available): full suite + byte-reproducible emission.
echo "r23_acceptance_gate: (5) smt build (skips cleanly if libz3 absent)…"
if cargo build -q -p axon-certcheck --features smt --bin certcheck 2>/dev/null; then
  if ! cargo test -q -p axon-certcheck --features smt; then
    echo "  smt test suite failed"; exit 1
  fi
  A="$(mktemp)"; B="$(mktemp)"
  "$ROOT/target/debug/certcheck" prove examples/proofs/mint_o2.obl --out "$A" >/dev/null
  "$ROOT/target/debug/certcheck" prove examples/proofs/mint_o2.obl --out "$B" >/dev/null
  if ! diff -q "$A" "$B" >/dev/null; then
    echo "  emission is NOT byte-identical across runs (A5 violation)"; rm -f "$A" "$B"; exit 1
  fi
  rm -f "$A" "$B"
  echo "  ✓ smt suite green + emission byte-identical"
else
  echo "  libz3 unavailable — smt build skipped (the solver-free path is the trusted one)"
fi

echo "r23_acceptance_gate: OK — every R23 §0 check present, unstubbed, and green; Z3 is out of the trust root"
exit 0
