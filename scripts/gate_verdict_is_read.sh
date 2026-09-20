#!/usr/bin/env bash
# A safety gate that produced NO READABLE VERDICT must not be scored as passed.
#
# `interp.rs` maps a gate function's return value to an exit code:
#
#     Ok(Value::Bool(true))  => 0      Ok(Value::Int(0)) => 0
#     Ok(Value::Bool(false)) => 1      Ok(Value::Int(_)) => 1
#     Ok(_)                  => 0      <-- everything else
#
# That last arm makes Unit, Str, Struct, Option and Result indistinguishable
# from `true`. The gate is not ABSENT — `gates_skipped` stays empty and
# `stages_run` lists it — so the audit record positively attests that the gate
# ran and passed. "An absent red team is not a passed red team" is defeated one
# layer down, by a gate that is present and unreadable.
#
# Two ordinary author shapes reach it, measured:
#
#   fn redteam_check() -> Result<bool, str> { Err("could not evaluate") }
#       -> status "safe" / "deployed", exit 0
#   fn redteam_check() { println("REDTEAM FAILED: blocking deploy") }
#       -> prints that line to the operator, then reports safe, exit 0
#
# The second is the shape CLAUDE.md's own Acid-Test-4 text uses, and
# `Result<T,E>` everywhere is this language's stated design principle — so the
# first is what the docs push authors toward.
#
# RED until `Ok(_)` becomes a distinct "gate produced no verdict" outcome that
# blocks exactly as an absent gate does.
set -uo pipefail
cd "$(dirname "$0")/.."
AXON="${AXON:-$PWD/target/debug/axon}"
[ -x "$AXON" ] || { echo "SKIP: no binary at $AXON"; exit 0; }
WS=$(mktemp -d); trap 'rm -rf "$WS"' EXIT
fails=0

mk() { printf '%s\nfn main() -> i64 { 0 }\n' "$2" > "$WS/$1.ax"; }
mk control     'fn redteam_check() -> bool { false }'
mk cannot_eval 'fn redteam_check() -> Result<bool, str> { Err("could not evaluate") }'
mk no_ret      'fn redteam_check() { println("REDTEAM FAILED: blocking deploy") }'

verdict() { (cd "$WS" && "$AXON" "$1" "$2.ax" --json 2>&1) | grep -o '"status":"[a-z_]*"' | tail -1; }

# THE CONTROL. If an unambiguous `false` is not caught, every result below is
# meaningless and the harness is broken rather than the compiler.
for verb in redteam deploy; do
  got=$(verdict "$verb" control)
  case "$got" in
    '"status":"caught"'|'"status":"blocked_gate"') ;;
    *) echo "CHECKER ERROR: $verb on an unambiguous refusing gate reported $got;"
       echo "  the probe is not reaching the gate, so nothing below can be read"
       exit 2 ;;
  esac
done

for verb in redteam deploy; do
  for case in cannot_eval no_ret; do
    got=$(verdict "$verb" "$case")
    case "$got" in
      '"status":"caught"'|'"status":"blocked_gate"'|'"status":"no_verdict"') ;;
      *) echo "FAIL $verb/$case: gate produced no readable verdict, reported $got"
         fails=$((fails+1)) ;;
    esac
  done
done

# The sharpest form: the gate is listed as RUN, so an auditor cannot tell this
# from a real pass by reading gates_skipped.
if (cd "$WS" && "$AXON" deploy cannot_eval.ax --json 2>&1) \
     | grep -q '"stages_run":\["redteam_check"\]'; then
  echo "FAIL deploy/cannot_eval: stages_run lists redteam_check as having run"
  echo "     and passed — the audit record attests a verdict that was never read"
  fails=$((fails+1))
fi

# SAME CLASS, DIFFERENT VERB. `axon verify` exits 0 when the prover is not
# linked — which is the DEFAULT build, since `smt` is opt-in. The stderr note
# is honest; the exit code is the only MACHINE-readable signal the verb emits,
# and it says verified. Measured on a program whose @[verify] bound is false
# for every input.
printf '@[verify(value > 100)]\nfn always_small() -> i64 { 1 }\nfn main() -> i64 { 0 }\n' \
  > "$WS/refuted.ax"
if (cd "$WS" && "$AXON" verify refuted.ax) >/dev/null 2>&1; then
  echo "FAIL verify/refuted: exit 0 on a @[verify] bound that is false for all"
  echo "     inputs and was never proved — \`axon verify && axon deploy\` proceeds"
  fails=$((fails+1))
fi

echo "gate-verdict readability: $fails unreadable verdict(s) scored as passed"
[ "$fails" -eq 0 ]
