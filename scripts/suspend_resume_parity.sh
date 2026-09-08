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
# Own target dir -- see decimal_parity.sh. A --no-default-features `axon` at the
# shared target/debug path is codegen-less but executable, so later harnesses
# skip with a false "toolchain absent" instead of rebuilding.
if ! CARGO_TARGET_DIR="$WORK/interp-target" \
     cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null; then
  echo "suspend_resume_parity: interpreter build failed — cannot run"; exit 1
fi
AXON="${AXON:-$WORK/interp-target/debug/axon}"

# ── 1. CLI stdin regression: str / loop / EOF (Slice-1 cases, end-to-end) ──────
# Each row is (program, piped-stdin, expected-VALUE). The value is printed, not
# returned from `main`.
#
# These rows were written 2026-06-25, when returning 4 from `main` exited 4.
# `b17e2bc` (2026-08-31, 389 commits later) made a value falling out of `main`
# an ANSWER rather than a process status, so `governance/EXIT_CODES.md` now
# remaps 2..=15 to exit 1 -- and all three expectations here (4, 6, 2) are in
# that band. The harness went red at that commit and stayed red, because it is
# wired into neither `gate.sh` nor `cli_run`.
#
# Verified rather than assumed: piping a 4-character and an 8-character line
# both give exit 1, so the exit code cannot carry this result at all.
cli_case() {
  local label="$1" src="$2" input="$3" want="$4"
  local prog="$WORK/$label.ax"
  # Rename the row's `main` to `probe` and print its result, so the value is
  # compared where it survives.
  printf '%b\n' "${src/fn main() -> i64/fn probe() -> i64}" > "$prog"
  printf 'fn main() { println(to_str(probe())) }\n' >> "$prog"
  # `host_await(prompt)` writes its prompt to stdout with no newline, so the
  # printed result lands on the same line after it ("name? 4"). Take the LAST
  # whitespace-separated field rather than stripping a hardcoded prompt --
  # the prompt is part of the contract under test and should not be silently
  # discarded from the capture.
  local raw; raw="$(printf '%b' "$input" | "$AXON" run "$prog" 2>/dev/null | grep -v '^axon: run-id ')"
  local got; got="${raw##* }"
  ran=$((ran+1))
  if [ "$got" != "$want" ]; then
    echo "suspend_resume_parity: FAIL [$label]: got=[$got] want=[$want]"
    fail=1
  else
    echo "  ok  [$label] (value $got)"
  fi
}

echo "── CLI stdin regression (str / loop / EOF) ───────────────────────────────"
# str round-trip: read one line, print its length.
cli_case str_roundtrip \
  'fn main() -> i64 { let r = host_await("name? ")  str_len(r) }' \
  'abcd' 4
# loop: read 3 lines, accumulate their lengths (2+3+1 = 6).
cli_case loop_three \
  'fn main() -> i64 { let total = 0  let i = 0  while i < 3 { let s = host_await("? ")  total = total + str_len(s)  i = i + 1 }  total }' \
  'ab\ncde\nf' 6
# EOF: host_await_opt stops the loop on end-of-input; 2 lines then EOF => 2.
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
