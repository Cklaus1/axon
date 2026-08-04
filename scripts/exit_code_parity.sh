#!/usr/bin/env bash
# exit_code_parity.sh — the interpreter and native codegen must agree on the
# PROCESS EXIT CODE, not just stdout (I-2 covers observable behavior, and the
# exit code is observable — CI and supervisors branch on it).
#
# History: native `assert(false)` exited 1 while the interpreter exited 101 —
# a real exit-code divergence (div0 already matched at 101). The assert-family
# panic exits were converged to 101 in codegen/builtins.rs; this harness locks
# that in and guards the broader contract:
#   - a runtime crash (assert / assert_eq mismatch / OOB / div0) → 101 on BOTH
#   - a clean program                                            → 0  on BOTH
#   - main's i64 return value                                    → that value, BOTH
#   - behavior native cannot honor (AI tier/budget policy)       → E0910 REFUSAL,
#     via `check_refused` — "refuse, never miscompile" is the parity contract
#     there, and it needs its own assertion shape (see F141 below)
#
# The closing summary states exactly which codes are covered and which are not.
# Do not replace it with a blanket claim — the previous "native==interp on all
# exit codes" line was false, and it is why the exit-5 divergence went unnoticed.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "exit_code_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "exit_code_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

# case <name> <expected_exit> <program-source>
fail=0
check() {
  local name="$1" want="$2" src="$3"
  local prog="$WORK/$name.ax"
  printf '%s\n' "$src" > "$prog"

  AXON_AI_MOCK=1 "$AXON" run "$prog" >/dev/null 2>&1
  local i_exit=$?

  local bin="$WORK/${name}_bin"
  if ! AXON_AI_MOCK=1 "$AXON" build "$prog" -o "$bin" --no-cache >/dev/null 2>&1; then
    # A per-case build failure is a FAILURE, not a skip. This used to `exit 0`,
    # which meant one un-buildable case silently passed the ENTIRE harness —
    # and the summary line below still claimed full parity. If the toolchain is
    # genuinely missing, the `cargo build` above already skipped us out.
    echo "FAIL [$name]: native build failed (the interpreter exited $i_exit)"
    fail=1
    return
  fi
  AXON_AI_MOCK=1 "$bin" >/dev/null 2>&1
  local n_exit=$?

  if [ "$i_exit" != "$n_exit" ]; then
    echo "FAIL [$name]: interp exit=$i_exit but native exit=$n_exit (must match — I-2)"
    fail=1
  elif [ "$i_exit" != "$want" ]; then
    echo "FAIL [$name]: both exited $i_exit but expected $want"
    fail=1
  else
    echo "  OK $name: both exit $i_exit"
  fi
}

# check_refused <name> <interp_exit> <substr> <src>
#   For behavior native codegen cannot reproduce, the contract is not "same exit
#   code" but "REFUSE, never miscompile" (I-2, sound-by-refusal): the interpreter
#   runs it with the given exit code, and the native BUILD must fail with an
#   E0910 naming the reason. This exists because F141 shipped: a fn under
#   @[ai(policy(budget: 1))] exited 5 in the interpreter and 0 natively, with all
#   three calls dispatched. `check` cannot express that case — it compares the
#   exits of two binaries, and here there must be no second binary.
check_refused() {
  local name="$1" want="$2" substr="$3" src="$4"
  local prog="$WORK/$name.ax"
  printf '%s\n' "$src" > "$prog"

  AXON_AI_MOCK=1 "$AXON" run "$prog" >/dev/null 2>&1
  local i_exit=$?

  local out
  out="$(AXON_AI_MOCK=1 "$AXON" build "$prog" -o "$WORK/${name}_bin" --no-cache 2>&1)"
  local b_status=$?

  if [ "$i_exit" != "$want" ]; then
    echo "FAIL [$name]: interp exited $i_exit but expected $want"
    fail=1
  elif [ "$b_status" -eq 0 ]; then
    echo "FAIL [$name]: native BUILD SUCCEEDED — it must refuse what it cannot honor"
    fail=1
  elif ! printf '%s' "$out" | grep -q "E0910"; then
    echo "FAIL [$name]: native build failed but not with E0910: $out"
    fail=1
  elif ! printf '%s' "$out" | grep -qF "$substr"; then
    echo "FAIL [$name]: E0910 did not mention '$substr': $out"
    fail=1
  else
    echo "  OK $name: interp exits $want, native refuses (E0910)"
  fi
}

# Runtime crashes — must be 101 on both engines.
check assert_false   101 'fn main() -> i64 { assert(false)  0 }'
check assert_eq_bad  101 'fn main() -> i64 { assert_eq(1, 2)  0 }'
check div_zero       101 'fn main() -> i64 { let z = 0  10 / z }'
check oob_index      101 'fn main() -> i64 { let a = [1, 2, 3]  a[10] }'

# Clean termination + explicit return value.
check clean_zero     0   'fn main() -> i64 { 0 }'
check return_seven   7   'fn main() -> i64 { 7 }'

# Phase 5: refinement-PRECONDITION violations on non-constant args → exit 6 on
# BOTH engines (the spec's Z3-free runtime-check fallback). A constant arg is a
# static E1209; these route a runtime value through an unrefined helper so the
# checker can't fold it, exercising the entry check in interp + codegen.
check refine_neg     6   'fn f(n: i64 where _ >= 0) -> i64 { n }
fn bad(x: i64) -> i64 { f(x) }
fn main() -> i64 { bad(0 - 1) }'
check refine_div0    6   'fn divide(n: i64, d: i64 where _ != 0) -> i64 { n / d }
fn main() -> i64 { let z = 0
 divide(10, z) }'
check refine_strlen  6   'type NonEmpty = str where str_len(_) > 0
fn greet(s: NonEmpty) -> i64 { str_len(s) }
fn caller(x: str) -> i64 { greet(x) }
fn main() -> i64 { caller("") }'
# A SATISFIED non-constant arg must NOT trip the check (no false positive); main
# returns factorial(5) = 120 on both engines.
check refine_ok      120 'fn factorial(n: i64 where _ >= 0) -> i64 { if n <= 1 { 1 } else { n * factorial(n - 1) } }
fn ok(x: i64) -> i64 { factorial(x) }
fn main() -> i64 { ok(5) }'

# Phase 5: refinement RETURN postconditions (the dual). A fn `-> T where P`
# whose non-constant return fails P exits 6 on BOTH engines; a satisfied one
# returns its value. (Constant bad returns are a static E1209, never reached.)
check refine_ret_bad 6   'type Positive = i64 where _ > 0
fn f(x: i64) -> Positive { x - 100 }
fn main() -> i64 { f(5) }'
check refine_ret_ok  105 'type Positive = i64 where _ > 0
fn f(x: i64) -> Positive { x + 100 }
fn main() -> i64 { f(5) }'

# Phase 5: refinement obligations at the remaining sites — struct FIELD,
# WHOLE-STRUCT (`_.lo <= _.hi`), and a `let p: T = …` annotation — all checked at
# runtime for non-constant values, exit 6 on BOTH engines; satisfied cases return
# their value. The whole-struct case exercises native `_.field` lowering.
check refine_field_bad  6  'type Pos = i64 where _ > 0
type Box = { v: Pos }
fn mk(x: i64) -> Box { Box { v: x } }
fn main() -> i64 { let b = mk(0 - 5)
 b.v }'
check refine_struct_bad 6  'type Range = { lo: i64, hi: i64 } where _.lo <= _.hi
fn mk(a: i64, b: i64) -> Range { Range { lo: a, hi: b } }
fn main() -> i64 { let r = mk(10, 2)
 r.hi }'
check refine_struct_ok  10 'type Range = { lo: i64, hi: i64 } where _.lo <= _.hi
fn mk(a: i64, b: i64) -> Range { Range { lo: a, hi: b } }
fn main() -> i64 { let r = mk(2, 10)
 r.hi }'
check refine_let_bad    6  'type Pos = i64 where _ > 0
fn neg(x: i64) -> i64 { 0 - x }
fn main() -> i64 { let p: Pos = neg(5)
 p }'
check refine_let_ok     3  'type Pos = i64 where _ > 0
fn neg(x: i64) -> i64 { 0 - x }
fn main() -> i64 { let p: Pos = neg(0 - 3)
 p }'

# ── AI policy (exit 5) — F141 / P6-EXIT-04 ───────────────────────────────────
# `@[ai(policy(budget: N))]` makes the (N+1)th ai_complete a fatal E1301/exit 5
# in the interpreter. The native runtime has no call meter, so the AOT binary
# used to run every call and exit 0 — I-2 in the unsafe direction (the binary
# keeps spending past a policy stop). Native now refuses; when a native meter
# lands, this becomes a plain `check … 5` row.
check_refused ai_budget 5 'budget: 1' \
'@[ai(policy(budget: 1))]
fn ask() -> i64 { let a = ai_complete("one")
 let b = ai_complete("two")
 3 }
fn main() -> i64 { ask() }'

# A non-`balanced` tier is refused for the same reason (the native ABI carries
# no model, so cheap/strong would silently call sonnet). The interpreter routes
# the tier correctly and runs clean, returning 3 from main.
check_refused ai_tier 3 'balanced' \
'@[ai(policy(tier: strong))]
fn ask() -> i64 { let a = ai_complete("one")
 3 }
fn main() -> i64 { ask() }'

if [ "$fail" -ne 0 ]; then
  echo "exit_code_parity: FAIL — interp↔native exit-code divergence"
  exit 1
fi
# Scope, stated honestly. This line used to read "native==interp on all exit
# codes", which was false: every case was 0/101/6 or a plain main return, so
# codes 3 (verify), 4 (corrigible), 7 (goal-budget) and 8 (sandbox) had ZERO
# coverage while the summary claimed otherwise. A summary that overstates its
# own coverage is how F141 shipped — exit 5 was "covered" by a line of prose.
echo "exit_code_parity: covered — 0, 101 (crash), 6 (refinement), main's return,"
echo "exit_code_parity:            5 (AI policy, via refusal)"
echo "exit_code_parity: NOT covered — 3 (verify), 4 (corrigible), 7 (goal-budget), 8 (sandbox)"
echo "exit_code_parity: PASS"
