#!/usr/bin/env bash
# clock_parity.sh — gate for the deterministic virtual clock (AXON_CLOCK).
#
# WHY THIS HARNESS EXISTS, AND WHY IT IS NOT OPTIONAL.
#
# `now_ms`/`sleep_ms` exist twice: `crates/axon-core/src/clock.rs` for the
# interpreter and a `vclock` module in `crates/axon-rt/src/lib.rs` for native
# binaries. They are duplicated because axon-core does NOT depend on axon-rt, so
# there is no shared module to put the logic in. Duplicated logic diverges — and
# native has form here: it silently ignored AXON_AI_MOCK until that was found,
# meaning the "deterministic" flag did nothing on one of the two engines.
#
# This harness asserts EXPECTED VALUES, not merely agreement. An agreement oracle
# cannot see a bug both engines share, and "both engines return the wrong time"
# is exactly the shape a duplicated env-var parser fails in (e.g. both defaulting
# tick to 0, or both mis-parsing `start:tick`).
#
# What it checks:
#   1. determinism  — two runs of the same program under one AXON_CLOCK are
#                     byte-identical (the property the feature exists for);
#   2. exact values — the first read IS the configured start, and a sleep moves
#                     the clock by exactly its argument (+tick per read);
#   3. monotonicity — read/sleep/read strictly increases, so elapsed-time logic
#                     is not broken by a "determinism" feature (a FROZEN clock
#                     would silently flip `if t2 > t` and change what programs
#                     compute — see clock.rs);
#   4. parity       — native and interp agree byte-for-byte on all of the above;
#   5. off by default — with no AXON_CLOCK the clock is real, so two runs differ
#                     and the value is a plausible epoch (not 0, not the start).
#
# Exit 0 = pass. Exit 1 = a real divergence or a wrong value. SKIP (exit 0) only
# when codegen is unavailable, and it says so on stdout.

set -uo pipefail
cd "$(dirname "$0")/.."

AXON="${AXON:-./target/debug/axon}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ ! -x "$AXON" ]; then
  echo "clock_parity: SKIP — no axon binary at $AXON"
  exit 0
fi

pass=0; fail=0
ok()   { echo "  OK $1"; pass=$((pass+1)); }
bad()  { echo "FAIL [$1]: $2"; fail=$((fail+1)); }

# The probe program: read, sleep, read. Prints both stamps, the delta, and a
# monotonicity verdict — so one output covers checks 2 and 3 at once.
PROG="$WORK/clock.ax"
cat > "$PROG" <<'AXEOF'
fn main() -> i64 {
    let t = now_ms()
    sleep_ms(250)
    let t2 = now_ms()
    println(to_str(t))
    println(to_str(t2))
    println(to_str(t2 - t))
    if t2 > t { println("monotonic") } else { println("FROZEN") }
    0
}
AXEOF

# Expected, with start=1700000000000 and the default tick=1:
#   read 1 returns the start exactly           -> 1700000000000
#   the read advances by tick, sleep adds 250   -> 1700000000251
#   delta is 251 (250 slept + 1 tick), NOT 250 — a real clock also reports at
#   least the sleep duration, so this is faithful rather than suspiciously exact
WANT="$WORK/want.txt"
cat > "$WANT" <<'EOF'
1700000000000
1700000000251
251
monotonic
EOF

# ── 1/5: interpreter produces the exact expected values ──────────────────────
AXON_CLOCK=1700000000000 "$AXON" run "$PROG" 2>/dev/null > "$WORK/i1.txt"
if cmp -s "$WORK/i1.txt" "$WANT"; then
  ok "interp: exact values (start anchored, sleep advances, monotonic)"
else
  bad interp_values "interp output differs from expected:
$(diff "$WANT" "$WORK/i1.txt" || true)"
fi

# ── 2/5: determinism — a second interpreter run is byte-identical ────────────
AXON_CLOCK=1700000000000 "$AXON" run "$PROG" 2>/dev/null > "$WORK/i2.txt"
if cmp -s "$WORK/i1.txt" "$WORK/i2.txt"; then
  ok "interp: two runs byte-identical"
else
  bad interp_determinism "two runs under the same AXON_CLOCK differed"
fi

# ── 3/5: off by default — the real clock is real ─────────────────────────────
#
# Without AXON_CLOCK the two runs must DIFFER (a real clock moves) and must not
# report the virtual start. If this check ever passes trivially it means the
# virtual clock leaked into unconfigured runs, which would be far worse than the
# bug this feature fixes.
"$AXON" run "$PROG" 2>/dev/null > "$WORK/r1.txt"
"$AXON" run "$PROG" 2>/dev/null > "$WORK/r2.txt"
r1_first="$(head -1 "$WORK/r1.txt")"
if [ "$r1_first" = "1700000000000" ]; then
  bad clock_leaked "an unconfigured run reported the virtual start time"
elif [ "${r1_first:-0}" -lt 1600000000000 ]; then
  bad real_clock "unconfigured run's now_ms ($r1_first) is not a plausible epoch ms"
elif cmp -s "$WORK/r1.txt" "$WORK/r2.txt"; then
  bad real_clock_frozen "two unconfigured runs were identical — the real clock is not advancing"
else
  ok "off by default: real clock, two runs differ"
fi

# ── tick=0: the clock moves ONLY on sleep ───────────────────────────────────
#
# A legitimate configuration and the reason "enabled" is a separate flag rather
# than a non-zero sentinel. Both reads return the start; the delta is 0.
AXON_CLOCK=5000:0 "$AXON" run "$PROG" 2>/dev/null > "$WORK/t0.txt"
t0_expect="$(printf '5000\n5250\n250\nmonotonic\n')"
if [ "$(cat "$WORK/t0.txt")" = "$t0_expect" ]; then
  ok "tick=0: clock advances only on sleep"
else
  bad tick_zero "AXON_CLOCK=5000:0 gave:
$(cat "$WORK/t0.txt")
wanted:
$t0_expect"
fi

# ── a malformed value must NOT invent a timeline ─────────────────────────────
#
# `AXON_CLOCK=lol` leaves the clock OFF. Silently picking a start would produce a
# deterministic run whose timeline nobody chose — worse than ignoring the var.
AXON_CLOCK=lol "$AXON" run "$PROG" 2>/dev/null > "$WORK/bad.txt"
bad_first="$(head -1 "$WORK/bad.txt")"
if [ "${bad_first:-0}" -gt 1600000000000 ]; then
  ok "malformed AXON_CLOCK ignored (real clock, no invented timeline)"
else
  bad malformed "AXON_CLOCK=lol produced $bad_first — it should fall back to the real clock"
fi

# ── every clock-reading builtin shares ONE timeline ─────────────────────────
#
# `now_ms` was not the only clock reader: `temporal_now`, `temporal_new` (which
# stamps `created_ms`) and `temporal_is_valid` called the crate's private REAL
# clock helper directly. That was worse than a replay gap — a program mixing
# `now_ms()` with `temporal_*` observed TWO DISAGREEING TIMELINES, one virtual and
# one real, so a `created_ms` compared against a `now_ms()` was arbitrary. Nothing
# reported it. All four now resolve through one function; this check is what keeps
# a fifth clock reader from being added outside it.
TL="$WORK/timeline.ax"
cat > "$TL" <<'AXEOF'
fn main() -> i64 {
    let a = now_ms()
    let b = temporal_now()
    if b - a < 1000 { println("same-timeline") } else { println("TWO-TIMELINES") }
    0
}
AXEOF
tl_out="$(AXON_CLOCK=1000 "$AXON" run "$TL" 2>/dev/null)"
if [ "$tl_out" = "same-timeline" ]; then
  ok "now_ms and temporal_now share one timeline"
else
  bad two_timelines "temporal_now is on a different clock than now_ms (got: '$tl_out') \
— a virtual now_ms compared against a real created_ms is meaningless"
fi

# ── 4/5 + 5/5: native parity ────────────────────────────────────────────────
BUILD_OUT="$("$AXON" build "$PROG" --out "$WORK/clocknat" 2>&1)"
BUILD_EXIT=$?
if [ $BUILD_EXIT -ne 0 ]; then
  # Probe the REAL build's refusal text rather than a --help flag check: the
  # `build` verb is registered regardless of the codegen feature, so a
  # flag-presence probe never skips. (Learned twice in this repo.)
  if echo "$BUILD_OUT" | grep -q "requires building axon with the .codegen. feature"; then
    echo "clock_parity: $pass passed, $fail failed (interp only)"
    # An interp failure is still a FAILURE — only the native half is unavailable.
    [ "$fail" -eq 0 ] || exit 1
    # The FINAL line must contain "skipping": that is how cli_run.rs's
    # `harness_skipped` recognises a partial run, and it deliberately checks the
    # last line only so a mid-run note cannot launder a real failure into a skip.
    echo "clock_parity: native half unavailable (no codegen) — skipping"
    exit 0
  fi
  bad native_build "axon build failed (exit $BUILD_EXIT):
$BUILD_OUT"
else
  AXON_CLOCK=1700000000000 "$WORK/clocknat" 2>/dev/null > "$WORK/n1.txt"
  if cmp -s "$WORK/n1.txt" "$WANT"; then
    ok "native: exact values"
  else
    bad native_values "native output differs from expected:
$(diff "$WANT" "$WORK/n1.txt" || true)"
  fi
  if cmp -s "$WORK/n1.txt" "$WORK/i1.txt"; then
    ok "native == interp (I-2)"
  else
    bad parity "native and interp disagree:
$(diff "$WORK/i1.txt" "$WORK/n1.txt" || true)"
  fi
fi

echo "clock_parity: $pass passed, $fail failed"
[ "$fail" -eq 0 ] || exit 1
# A run that checked NOTHING also has zero failures, and must not read as a pass —
# the vacuous-pass shape this repo has hit repeatedly. Only emit the PASS line that
# the cli_run.rs wrapper asserts on when checks actually ran.
if [ "$pass" -eq 0 ]; then
  echo "clock_parity: SKIP — nothing ran"
  exit 0
fi
echo "clock_parity: PASS"
