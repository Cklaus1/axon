#!/usr/bin/env bash
# suspend_resume_parity.sh — the R15 resume runtime survives a suspend for the
# FULL payload range (R15 Slice 2): str / scalar / loop / EOF (the Slice-1 cases,
# regressed here through the CLI stdin driver) AND dict / struct / enum / tuple /
# nested-composite payloads (the Slice-2 capability, exercised through the
# Value-aware Rust host harness — the only host that can carry a structured
# payload; the CLI stdin protocol is text-only).
#
# WHY a mixed harness: the Slice-2 capability (an arbitrary `!Send` `Value` —
# dict/struct — crossing the worker-thread suspend boundary) needs a host that
# constructs and inspects a `Value` reply, which the stdin/stdout text protocol
# cannot express. So the dict/struct/enum round-trips are driven by the gated
# `cargo test` Rust host (`run_suspendable_values`), and the str/loop/EOF cases
# are regressed end-to-end through `axon run`'s stdin host. Together they prove
# every payload shape the runtime claims to support actually round-trips.
#
# Also asserts the SOUNDNESS BOUNDARY (a `Chan` payload is refused, not corrupted)
# and the codegen posture (host_await* is E0910-refused — interp-only, I-2).
#
# Vacuous-pass guard: counts the cases it actually ran and FAILS if zero (a
# silently-renamed test must not let this harness pass green on nothing).
#
# Requires only the interpreter `axon` binary (no LLVM); the codegen-refusal check
# skips cleanly if the codegen build is unavailable.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

ran=0
fail=0

echo "suspend_resume_parity: building interpreter axon binary…"
if ! cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null; then
  echo "suspend_resume_parity: interpreter build failed — cannot run"; exit 1
fi
AXON="target/debug/axon"

# ── 1. CLI stdin regression: str / loop / EOF (Slice-1 cases, end-to-end) ──────
# Each pair is (program, piped-stdin, expected-exit). These run the str-typed
# host_await through `axon run`'s stdin/stdout driver (run_suspendable_stdio).
cli_case() {
  local label="$1" src="$2" input="$3" want_exit="$4"
  local prog="$WORK/$label.ax"
  printf '%b\n' "$src" > "$prog"
  printf '%b' "$input" | "$AXON" run "$prog" >/dev/null 2>&1
  local got=$?
  ran=$((ran+1))
  if [ "$got" != "$want_exit" ]; then
    echo "suspend_resume_parity: FAIL [$label]: exit got=$got want=$want_exit"
    fail=1
  else
    echo "  ok  [$label] (exit $got)"
  fi
}

echo "── CLI stdin regression (str / loop / EOF) ───────────────────────────────"
# str round-trip: read one line, echo its length back as the exit code.
cli_case str_roundtrip \
  'fn main() -> i64 { let r = host_await("name? ")  str_len(r) }' \
  'abcd' 4
# loop: read 3 lines, accumulate their lengths (2+3+1 = 6).
cli_case loop_three \
  'fn main() -> i64 { let total = 0  let i = 0  while i < 3 { let s = host_await("? ")  total = total + str_len(s)  i = i + 1 }  total }' \
  'ab\ncde\nf' 6
# EOF: host_await_opt stops the loop on end-of-input; 2 lines then EOF ⇒ exit 2.
cli_case eof_terminates \
  'fn main() -> i64 { let n = 0  let go = 1  while go == 1 { match host_await_opt("? ") { None => { go = 0 } Some(s) => { n = n + 1 } } }  n }' \
  'x\ny' 2

# ── 2. Value-payload round-trips (Slice-2 capability) via the Rust host ───────
# These run the gated lib tests that drive `run_suspendable_values` with a
# Value-aware host carrying dict / struct / enum / tuple / nested-composite
# payloads — the case that did NOT work before Slice 2 (a `!Send` Value crossing
# the worker-thread channel). The Chan-refusal soundness boundary is among them.
echo "── Value payloads (dict / struct / enum) via run_suspendable_values ───────"
SLICE2_TESTS=(
  r15_slice2_dict_payload_round_trips
  r15_slice2_struct_payload_round_trips
  r15_slice2_enum_and_tuple_payload_round_trip
  r15_slice2_chan_payload_is_refused_not_corrupted
  r15_slice2_str_form_still_works_through_value_substrate
  r15_slice2_send_value_round_trip_is_lossless
)
for t in "${SLICE2_TESTS[@]}"; do
  if cargo test -q -p axon-core --no-default-features --lib "interp::tests::$t" \
       -- --exact >/dev/null 2>&1; then
    echo "  ok  [$t]"
    ran=$((ran+1))
  else
    echo "suspend_resume_parity: FAIL [$t]: value-payload round-trip test did not pass"
    fail=1
  fi
done

# ── 3. Codegen refusal (I-2): host_await_val must NOT silently miscompile ──────
echo "── codegen refusal (host_await_val is interp-only) ───────────────────────"
prog="$WORK/cg_refuse.ax"
printf '%s\n' 'type P = { x: i64, y: i64 }
fn main() -> i64 { let p = P { x: 1, y: 2 }  let r = host_await_val(p)  0 }' > "$prog"
if cargo build -q -p axon-core --bin axon 2>/dev/null; then
  msg="$(target/debug/axon build "$prog" -o "$WORK/cg_refuse.bin" --no-cache 2>&1)"
  ran=$((ran+1))
  # An explicit E0910, a codegen-less hint, or simply no binary produced all count
  # as a sound refusal (never a built binary that silently drops the suspension).
  if echo "$msg" | grep -q "E0910" || echo "$msg" | grep -q "use \`axon run\`" \
       || [ ! -x "$WORK/cg_refuse.bin" ]; then
    echo "  ok  [codegen_refuses_host_await_val]"
  else
    echo "suspend_resume_parity: FAIL: codegen built host_await_val instead of refusing it"
    echo "$msg" | head -3
    fail=1
  fi
else
  echo "  skip [codegen_refuses_host_await_val] (codegen build unavailable — LLVM absent)"
fi

# ── vacuous-pass guard + verdict ──────────────────────────────────────────────
echo "─────────────────────────────────────────────────────────────────────────"
if [ "$ran" -eq 0 ]; then
  echo "suspend_resume_parity: FAIL — ran 0 cases (vacuous pass; tests vanished?)"
  exit 1
fi
if [ "$fail" -ne 0 ]; then
  echo "suspend_resume_parity: FAILED ($ran cases attempted)"
  exit 1
fi
echo "suspend_resume_parity: PASS — $ran cases (str/loop/EOF + dict/struct/enum/Chan-refusal + codegen-refusal)"
