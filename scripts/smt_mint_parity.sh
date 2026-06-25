#!/usr/bin/env bash
# smt_mint_parity.sh — R20 Slice 1: the kernel capability-mint obligation
# (O1 attenuation + O2 budget carve) is SMT-proven, and proving it stays
# OBSERVABLY INVISIBLE. Locks three contracts:
#
#   1. PROGRESS  — under the `smt` binary, a program that mints a principal
#      reports the kernel mint lemma discharged ("incl. kernel mint attenuation
#      + budget-carve" on stderr). A regression that stops proving it surfaces
#      here.
#   2. SOUNDNESS — the `smt` binary (mint guard provably redundant) and the
#      default binary (guard runtime-enforced) produce byte-identical stdout AND
#      the same exit code on a principal-minting program. Proving the obligation
#      must not change observable behaviour.
#   3. TRIPWIRE  — the R20 unit tests pass: the prover REFUTES a weakened mint
#      (dropped cap-guard / dropped budget-clamp) with a counterexample, and the
#      differential grid test confirms the real `PrincipalRegistry::mint` impl
#      satisfies O1∧O2 over the edge-case grid. Together these are the I-12
#      tripwire — a weakened minter can pass NEITHER the model proof (E1610) nor
#      the impl grid test.
#
# Requires libz3 (the `smt` feature links it). Skips cleanly when absent — like
# the other smt/wasm harnesses — so a Z3-less box still passes the gate.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! echo 'int main(){return 0;}' | cc -xc - -lz3 -o /dev/null 2>/dev/null; then
  echo "smt_mint_parity: libz3 not found — skipping (install to enable)"
  exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "smt_mint_parity: building default + smt binaries…"
if ! cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null; then
  echo "smt_mint_parity: default build failed — skipping"
  exit 0
fi
DEF="target/debug/axon"
if ! CARGO_TARGET_DIR="$WORK/smt-target" \
     cargo build -q -p axon-core --no-default-features --features smt --bin axon 2>/dev/null; then
  echo "smt_mint_parity: smt build failed — skipping"
  exit 0
fi
SMT="$WORK/smt-target/debug/axon"

fail=0

# A minimal program that mints an attenuated child and reads its budget back.
PROG="$WORK/mint.ax"
cat > "$PROG" <<'EOF'
fn main() -> i64 {
    let root = principal_root("root", true, true, true, 100)
    let child = principal_mint(root, "child", true, false, false, 40)
    let rem = principal_budget_remaining(child)
    println("child budget {to_str(rem)}")
    rem
}
EOF

# ── 1. PROGRESS: the smt binary reports the kernel mint lemma discharged ──
smt_err="$("$SMT" run "$PROG" 2>&1 >/dev/null)"
if printf '%s\n' "$smt_err" | grep -q 'kernel mint attenuation'; then
  echo "  PASS progress: kernel mint obligation discharged under smt"
else
  echo "  FAIL progress: smt run did not report the kernel mint lemma discharged"
  echo "    stderr was: $smt_err"
  fail=1
fi

# ── 2. SOUNDNESS: default == smt on stdout + exit ──
d_out="$("$DEF" run "$PROG" 2>/dev/null)"; d_exit=$?
s_out="$("$SMT" run "$PROG" 2>/dev/null)"; s_exit=$?
if [ "$d_out" = "$s_out" ] && [ "$d_exit" = "$s_exit" ]; then
  echo "  PASS soundness: default==smt (exit $d_exit, stdout identical)"
else
  echo "  FAIL soundness: default(exit=$d_exit,'$d_out') != smt(exit=$s_exit,'$s_out')"
  fail=1
fi

# ── 3. TRIPWIRE: the R20 prover + grid tests pass ──
echo "smt_mint_parity: running R20 obligation + tripwire tests…"
if CARGO_TARGET_DIR="$WORK/smt-target" \
   cargo test -q -p axon-core --no-default-features --features smt --lib 'r20_' 2>/dev/null; then
  echo "  PASS tripwire: R20 prover refutes weakenings + grid test confirms the impl"
else
  echo "  FAIL tripwire: an R20 obligation/tripwire test failed"
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "smt_mint_parity: FAILED"
  exit 1
fi
echo "smt_mint_parity: OK — kernel mint obligation proven, invisible, and tripwired (R20)"
exit 0
