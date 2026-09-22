#!/usr/bin/env bash
# WASI environment-control gate.
#
# The interpreter-only env controls (AXON_ALLOWED_EFFECTS, AXON_RECORD,
# AXON_REPLAY, AXON_AUDIT_LEDGER) are REFUSED by a compiled binary, because a
# compiled engine cannot honour them and performing the effects anyway — with
# no warning and exit 0 — is indistinguishable from a run the control allowed.
# Native has refused since D-002. wasm32-wasip1 did NOT, and the classification
# in the completeness ledger recorded it as UNTESTED rather than measured.
#
# Measured, one program, one ceiling:
#   axon run  AXON_ALLOWED_EFFECTS=Pure  -> exit 8, sandbox violation
#   wasip1    (same program, same var)   -> "IO HAPPENED", exit 0
# Cause: codegen skipped the refusal prologue for ALL wasm targets on one
# boolean, though only the BROWSER target lacks an environment to refuse.
#
# THE TWO WASM TARGETS ARE ASSERTED SEPARATELY AND DELIBERATELY. Their control
# surfaces genuinely differ, so a single generic "wasm" assertion would be
# wrong about one of them whichever way it was written:
#   * wasm32-wasip1            HAS env vars (WASI) -> must REFUSE
#   * wasm32-unknown-unknown   has no env channel  -> NOT-APPLICABLE, and must
#                                                     NOT carry the refusal
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fail=0
ok()   { echo "  ok    $*"; }
bad()  { echo "  FAIL  $*"; fail=$((fail+1)); }

# ── preconditions, each proving its own reason ──────────────────────────────
WASMRT=""
for rt in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do
  if command -v "$rt" >/dev/null 2>&1 || [ -x "$rt" ]; then WASMRT="$rt"; break; fi
done
if [ -z "$WASMRT" ]; then echo "wasi_env_control_gate: no wasm runtime — skipping"; exit 0; fi

if ! rustup target list --installed 2>/dev/null | grep -q '^wasm32-wasip1$'; then
  rc=$?
  if [ $rc -gt 1 ]; then
    echo "wasi_env_control_gate: cannot determine whether wasm32-wasip1 is installed — refusing to call that a skip" >&2
    exit 1
  fi
  echo "wasi_env_control_gate: wasm32-wasip1 not installed — skipping"; exit 0
fi

AXON="${AXON:-target/debug/axon}"
[ -x "$AXON" ] || { echo "wasi_env_control_gate: $AXON missing — build it first" >&2; exit 1; }

echo "── WASI env-control gate ──────────────────────────────────────────"

# ── LEG 1: wasm32-wasip1 must REFUSE, behaviourally ─────────────────────────
cat > "$WORK/io.ax" <<'AX'
fn main() { println("IO HAPPENED") }
AX
if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$WORK/io.ax" >/dev/null 2>&1 \
   || [ ! -f "$WORK/io.linked.wasm" ]; then
  echo "wasi_env_control_gate: wasip1 link unavailable for the IO probe — skipping"; exit 0
fi

# CONTROL FIRST: an unconstrained run must still work. Without this, a refusal
# that fired unconditionally would pass every case below while breaking every
# real wasip1 program — the assertion would be vacuous.
out="$("$WASMRT" --invoke main "$WORK/io.linked.wasm" 2>/dev/null | grep -vi experimental)"
"$WASMRT" --invoke main "$WORK/io.linked.wasm" >/dev/null 2>&1; rc=$?
if [ $rc -eq 0 ] && printf '%s' "$out" | grep -q "IO HAPPENED"; then
  ok "control: unconstrained wasip1 run still executes (exit 0, output present)"
else
  bad "control: an unconstrained wasip1 run no longer works (exit $rc) — the refusal is too broad"
fi

for var in AXON_ALLOWED_EFFECTS AXON_RECORD AXON_REPLAY AXON_AUDIT_LEDGER; do
  case "$var" in
    AXON_ALLOWED_EFFECTS) val="Pure" ;;
    *)                    val="$WORK/j.journal" ;;
  esac
  err="$("$WASMRT" --env "$var=$val" --invoke main "$WORK/io.linked.wasm" 2>&1 >/dev/null | grep -vi experimental)"
  "$WASMRT" --env "$var=$val" --invoke main "$WORK/io.linked.wasm" >/dev/null 2>&1; rc=$?
  stdout="$("$WASMRT" --env "$var=$val" --invoke main "$WORK/io.linked.wasm" 2>/dev/null | grep -vi experimental)"

  if [ $rc -ne 2 ]; then
    bad "$var: expected exit 2 (refusal), got $rc"
  elif ! printf '%s' "$err" | grep -q "$var"; then
    # Each variable must be named in its OWN refusal. A shared message that
    # named a different control is the exact defect the REFUSED table's
    # per-variable consequence text was introduced to make unrepresentable.
    bad "$var: refused, but the message does not name it"
  elif printf '%s' "$stdout" | grep -q "IO HAPPENED"; then
    bad "$var: refused yet the effect still happened"
  else
    ok "$var: wasip1 refuses (exit 2, names the variable, no effect performed)"
  fi
done

# ── LEG 1b: AXON_SEED must actually seed on wasip1 ──────────────────────────
# Not an env-control REFUSAL (native honours this one, so wasi should too), but
# the same root cause and the same file: the seeding prologue was skipped for
# every wasm target by one boolean. The failure was worse than "the seed is
# ignored" — measured, `random_i64(1, 1000000)` twice returned 1 and 883707 on
# EVERY run and under EVERY seed. A fixed sequence, not an unseedable one, which
# is the half this repo's own notes call the more serious.
cat > "$WORK/rand.ax" <<'AX'
fn main() {
    println(to_str(random_i64(1, 1000000)))
    println(to_str(random_i64(1, 1000000)))
}
AX
if "$AXON" target build --engine codegen --target wasm32-wasip1 "$WORK/rand.ax" >/dev/null 2>&1 \
   && [ -f "$WORK/rand.linked.wasm" ]; then
  r_a="$("$WASMRT" --env AXON_SEED=1  --invoke main "$WORK/rand.linked.wasm" 2>/dev/null | head -2 | tr '\n' ' ')"
  r_b="$("$WASMRT" --env AXON_SEED=42 --invoke main "$WORK/rand.linked.wasm" 2>/dev/null | head -2 | tr '\n' ' ')"
  r_b2="$("$WASMRT" --env AXON_SEED=42 --invoke main "$WORK/rand.linked.wasm" 2>/dev/null | head -2 | tr '\n' ' ')"
  if [ -z "$r_a" ]; then
    bad "AXON_SEED: the wasip1 random probe produced no output"
  elif [ "$r_a" = "$r_b" ]; then
    bad "AXON_SEED: different seeds produced identical output ($r_a) — the seed is ignored"
  elif [ "$r_b" != "$r_b2" ]; then
    bad "AXON_SEED: the same seed produced different output ($r_b vs $r_b2) — not reproducible"
  else
    ok "AXON_SEED: wasip1 honours the seed (differs across seeds, reproduces within one)"
  fi
else
  echo "  skip  AXON_SEED leg: the random probe did not link"
fi

# ── LEG 2: wasm32-unknown-unknown is NOT-APPLICABLE, structurally ───────────
# Asserted on the SAME program built for BOTH targets, so the comparison is
# about the target and not about the program.
if rustup target list --installed 2>/dev/null | grep -q '^wasm32-unknown-unknown$'; then
  cat > "$WORK/pure.ax" <<'AX'
fn main() -> i64 { 41 + 1 }
AX
  REFUSAL_TEXT='is honoured by the interpreter only'
  "$AXON" target build --engine codegen --target wasm32-wasip1 "$WORK/pure.ax" >/dev/null 2>&1
  cp "$WORK/pure.linked.wasm" "$WORK/pure.wasi.wasm" 2>/dev/null
  "$AXON" target build --engine codegen --target wasm32-unknown-unknown "$WORK/pure.ax" >/dev/null 2>&1
  cp "$WORK/pure.linked.wasm" "$WORK/pure.browser.wasm" 2>/dev/null

  if [ -f "$WORK/pure.wasi.wasm" ] && [ -f "$WORK/pure.browser.wasm" ]; then
    w="$(strings "$WORK/pure.wasi.wasm"    2>/dev/null | grep -c "$REFUSAL_TEXT")"
    b="$(strings "$WORK/pure.browser.wasm" 2>/dev/null | grep -c "$REFUSAL_TEXT")"
    [ "$w" -ge 1 ] && ok "wasip1 module carries the env refusal" \
                   || bad "wasip1 module does NOT carry the env refusal"
    # The browser target is exempt because it has no environment channel at
    # all, NOT because the control was waived. Carrying the refusal there
    # would be dead weight policing a channel that does not exist.
    [ "$b" -eq 0 ] && ok "browser module carries NO env refusal (not-applicable: no env channel)" \
                   || bad "browser module carries an env refusal — it has no environment to police"
  else
    # NOT A SKIP. The target is installed, so a missing module means the build
    # or link FAILED, and that is a result rather than an absence. This branch
    # used to print "skip" and let the gate pass: found by mutation — emitting
    # the refusal for the browser target too makes its link fail on an
    # undefined symbol, and the over-broad mutation was reported as PASS with
    # the browser assertion never evaluated. A skip that swallows the failure
    # it exists to catch is a false green.
    bad "browser leg: wasm32-unknown-unknown is installed but produced no linked module — build/link failed"
    "$AXON" target build --engine codegen --target wasm32-unknown-unknown "$WORK/pure.ax" 2>&1 | sed 's/^/        | /' | tail -5
  fi
else
  echo "  skip  browser leg: wasm32-unknown-unknown not installed"
fi

echo "───────────────────────────────────────────────────────────────────"
if [ "$fail" -eq 0 ]; then
  echo "wasi_env_control_gate: PASS"
  exit 0
fi
echo "wasi_env_control_gate: FAIL ($fail check(s))"
exit 1
