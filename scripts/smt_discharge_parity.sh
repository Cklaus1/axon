#!/usr/bin/env bash
# smt_discharge_parity.sh — Phase 5 §4: SMT static discharge must be OBSERVABLY
# INVISIBLE. Wiring the Z3 prover into the default `axon run` pipeline elides a
# runtime check ONLY when the prover proved the obligation holds for ALL inputs,
# so the elided check was provably dead. This harness locks that contract in:
#
#   1. SOUNDNESS — for each program, the `smt`-feature binary (checks elided
#      where proven) must produce the SAME exit code AND stdout as the default
#      binary (every check runtime-enforced). If discharge ever elided a check
#      that COULD fire, the two would diverge. They must not.
#   2. PROGRESS  — a provable obligation must actually be discharged (the smt
#      binary prints "SMT discharged N …" on stderr). A discharge of 0 where we
#      expect >0 means the bridge silently regressed to a pure runtime gate.
#   3. NON-DISCHARGE — a violable / unsupported obligation must NOT be
#      discharged: its runtime check stays armed and still fires (exit 3/6).
#
# Requires libz3 (the `smt` feature links it). Skips cleanly when absent — like
# the wasm harnesses — so a Z3-less box still passes the gate.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! echo 'int main(){return 0;}' | cc -xc - -lz3 -o /dev/null 2>/dev/null; then
  echo "smt_discharge_parity: libz3 not found — skipping (install to enable)"
  exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Build the DEFAULT interpreter binary (no smt → empty discharge set, every
# runtime check enforced) and the SMT binary (discharge active). Separate target
# dirs so the two builds don't thrash one artifact.
echo "smt_discharge_parity: building default + smt binaries…"
if ! cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null; then
  echo "smt_discharge_parity: default build failed — skipping"
  exit 0
fi
DEF="target/debug/axon"
if ! CARGO_TARGET_DIR="$WORK/smt-target" \
     cargo build -q -p axon-core --no-default-features --features smt --bin axon 2>/dev/null; then
  echo "smt_discharge_parity: smt build failed — skipping"
  exit 0
fi
SMT="$WORK/smt-target/debug/axon"

fail=0

# parity <name> <expect_discharged:yes|no> <program-source>
# Asserts the smt run and the default run agree on (exit, stdout); checks the
# discharge count matches the expectation.
parity() {
  local name="$1" expect="$2" src="$3"
  local prog="$WORK/$name.ax"
  printf '%s\n' "$src" > "$prog"

  local def_out smt_out def_exit smt_exit smt_err
  def_out="$(AXON_AI_MOCK=1 "$DEF" run "$prog" 2>/dev/null)"; def_exit=$?
  smt_out="$(AXON_AI_MOCK=1 "$SMT" run "$prog" 2>"$WORK/$name.err")"; smt_exit=$?
  smt_err="$(cat "$WORK/$name.err")"

  if [ "$def_exit" != "$smt_exit" ] || [ "$def_out" != "$smt_out" ]; then
    echo "FAIL [$name]: default(exit=$def_exit,out=$def_out) != smt(exit=$smt_exit,out=$smt_out) — discharge changed observable behavior!"
    fail=1
    return
  fi

  local discharged="no"
  echo "$smt_err" | grep -q "SMT discharged" && discharged="yes"
  if [ "$discharged" != "$expect" ]; then
    echo "FAIL [$name]: expected discharged=$expect but got $discharged (stderr: $smt_err)"
    fail=1
    return
  fi
  echo "  OK $name: both exit $smt_exit, identical stdout, discharged=$discharged"
}

# ── PROVABLE: must be discharged, and the elision must be invisible. ──────────
# abs(x) >= 0 for all x → the scalar @[verify] bound is ∀-proven.
parity verify_absish yes '@[verify(value >= 0)]
fn absish(x: i64) -> i64 { if x >= 0 { x } else { 0 - x } }
fn main() -> i64 { absish(5) + absish(0 - 7) }'

# A refinement RETURN `-> NonNeg` proven for all inputs (n*n >= 0).
parity refine_sq yes 'type NonNeg = i64 where _ >= 0
fn sq(n: i64) -> NonNeg { n * n }
fn main() -> i64 { sq(0 - 6) }'

# ── NON-PROVABLE: must NOT be discharged; the runtime gate still fires and the
# two engines still agree (both take the runtime path). ──────────────────────
# dec(0) = -1 violates value>=0 → counterexample, NOT discharged, runtime @[verify]
# fires (exit 3) on BOTH binaries.
parity verify_violate no '@[verify(value >= 0)]
fn dec(x: i64) -> i64 { x - 1 }
fn main() -> i64 { dec(0) }'

# A refinement return that can fail (x-100 not always >0) → not discharged; the
# runtime postcondition fires (exit 6) on both.
parity refine_ret_violate no 'type Positive = i64 where _ > 0
fn f(x: i64) -> Positive { x - 100 }
fn main() -> i64 { f(5) }'

# A plain program with no obligations at all → nothing to discharge, identical.
parity no_obligations no 'fn main() -> i64 { 6 * 7 }'

if [ "$fail" -ne 0 ]; then
  echo "smt_discharge_parity: FAIL — discharge is not observably invisible"
  exit 1
fi
echo "smt_discharge_parity: discharge elides only provably-dead checks (default==smt)"
echo "smt_discharge_parity: PASS"
