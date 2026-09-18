#!/usr/bin/env bash
# chan_parity.sh — gate for channel semantics across both engines.
#
# WHY THIS HARNESS EXISTS.
#
# Channels are the one Phase-3 concurrency primitive with no parity coverage.
# Of the 53 `*_parity.sh` harnesses, exactly one mentions a channel
# (`recursion_guard_parity.sh`), and there it is a VEHICLE for testing recursion
# depth inside a `spawn`, not the subject. So `send`/`recv`/`spawn` semantics
# were compared between the interpreter and native codegen nowhere.
#
# That gap has already cost something. `Chan::new(n)` — an entry in the BUILTINS
# table, published by `axon reference` — worked in native codegen and panicked
# under the interpreter ("value of type Chan is not callable"), because the
# evaluator recognised only the `chan::<T>` lowering that `chan<T>()` produces.
# The parity suite could not have caught it: nothing ran a channel on both
# engines. Note the direction, which is the unusual part — the INTERPRETER is
# the reference semantics (I-2), and it was the broken side.
#
# This harness asserts EXPECTED VALUES, not merely agreement. An agreement
# oracle cannot see a bug both engines share, and "both drop a message" or "both
# deliver out of order" are exactly the shapes a duplicated queue fails in.
#
# What it checks:
#   1. both SPELLINGS work        — `chan<T>()` and `Chan::new(n)` are the same
#                                   channel, not two kinds;
#   2. exact values               — a sent value arrives unchanged;
#   3. FIFO order                 — three sends arrive in the order sent, which
#                                   is the property a ring buffer can silently
#                                   break while still delivering every message;
#   4. capacity is not a ceiling  — more sends than the declared capacity all
#                                   arrive (spec phase-3 §4: "growable"), so the
#                                   doc and the code agree;
#   5. parity                     — native and interp agree byte-for-byte on all
#                                   of the above.
#
# Exit 0 = pass. Exit 1 = a real divergence or a wrong value. SKIP (exit 0) only
# when codegen is unavailable, and it says so on stdout.

set -uo pipefail
cd "$(dirname "$0")/.."

AXON="${AXON:-./target/debug/axon}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ ! -x "$AXON" ]; then
  echo "chan_parity: SKIP — no axon binary at $AXON"
  exit 0
fi

pass=0; fail=0
ok()  { echo "  OK $1"; pass=$((pass+1)); }
bad() { echo "FAIL [$1]: $2"; fail=$((fail+1)); }

# ── the probe ────────────────────────────────────────────────────────────────
#
# Both spellings, a round trip, FIFO over three messages, and four sends into a
# channel declared with capacity 1. Sends are spawned one per message so the
# ordering assertion is about the QUEUE rather than about one thread's writes.
PROG="$WORK/chan.ax"
cat > "$PROG" <<'AXEOF'
fn main() -> i64 {
    let a = chan<i64>()
    spawn { a.send(42) }
    println(to_str(a.recv()))

    let b = Chan::new(4)
    spawn { b.send(7) }
    println(to_str(b.recv()))

    let c = chan<i64>()
    spawn { c.send(1) }
    spawn { c.send(2) }
    spawn { c.send(3) }
    println(to_str(c.recv()))
    println(to_str(c.recv()))
    println(to_str(c.recv()))

    let d = Chan::new(1)
    spawn { d.send(10) }
    spawn { d.send(20) }
    spawn { d.send(30) }
    spawn { d.send(40) }
    println(to_str(d.recv() + d.recv() + d.recv() + d.recv()))
    0
}
AXEOF

WANT="$WORK/want.txt"
cat > "$WANT" <<'EOF'
42
7
1
2
3
100
EOF

# ── 1/3: the interpreter produces the exact expected values ──────────────────
"$AXON" run "$PROG" 2>/dev/null > "$WORK/interp.txt"
if cmp -s "$WORK/interp.txt" "$WANT"; then
  ok "interp: both spellings, FIFO order, capacity is not a ceiling"
else
  bad interp_values "interp output differs from expected:
$(diff "$WANT" "$WORK/interp.txt" || true)"
fi

# ── 2/3: the native binary produces the same exact values ────────────────────
#
# A build failure is NOT a skip. `no codegen` is a real reason to skip; anything
# else is this harness finding what it exists to find, and calling an invalid
# lowering "skipped" is how a broken engine reports green.
BUILD_LOG="$WORK/build.log"
if "$AXON" build "$PROG" -o "$WORK/chan.bin" >"$BUILD_LOG" 2>&1; then
  # Bounded, and a timeout is a FAIL with a reason — not a hang.
  #
  # The defect this harness was written for (a bounded `send` blocking when the
  # buffer fills) presents as a binary that never terminates. Running it
  # unbounded made the HARNESS hang instead of reporting, which in CI burns the
  # job limit and says nothing about why. Mutation-testing the harness is what
  # exposed that: restoring the blocking send produced "Terminated", not a FAIL.
  if timeout 30 "$WORK/chan.bin" > "$WORK/native.txt" 2>/dev/null; then
    :
  elif [ $? -eq 124 ]; then
    bad native_hang "the native binary did not terminate within 30s — a send \
blocked, which means the channel is bounded on this engine and unbounded on the \
other (the interpreter cannot block at all). Produced so far: \
[$(tr '\n' ' ' < "$WORK/native.txt")]"
  fi
  if cmp -s "$WORK/native.txt" "$WANT"; then
    ok "native: same exact values"
  else
    bad native_values "native output differs from expected:
$(diff "$WANT" "$WORK/native.txt" || true)"
  fi
  # ── 3/3: and the two engines agree byte-for-byte (I-2) ───────────────────
  if cmp -s "$WORK/interp.txt" "$WORK/native.txt"; then
    ok "parity: interp and native byte-identical"
  else
    bad parity "engines disagree:
$(diff "$WORK/interp.txt" "$WORK/native.txt" || true)"
  fi
elif grep -qiE "codegen feature|not compiled with codegen|unknown command" "$BUILD_LOG"; then
  echo "chan_parity: SKIP native half — codegen unavailable"
else
  bad native_build "the native build FAILED, which is a finding, not a skip:
$(tail -5 "$BUILD_LOG")"
fi

echo
if [ "$fail" -eq 0 ]; then
  echo "chan_parity: PASS ($pass checks)"
  exit 0
fi
echo "chan_parity: FAIL ($fail of $((pass+fail)) checks)"
exit 1
