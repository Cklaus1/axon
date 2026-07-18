#!/usr/bin/env bash
# R32 acceptance gate — governance/specs/R32-formal-corrigibility-proof.md
#
# Verifies the R32 formal-corrigibility-proof artifacts:
#   - governance/proofs/R27KillSwitch.tla   (TLA+ model)
#   - governance/proofs/R27KillSwitch.cfg   (TLC model-checker config)
#   - governance/proofs/R27Corrigibility.v  (Coq proof)
#
# Contract (per BUILD_PROTOCOL.md / the 2026-07-18 audit): a "tool not
# installed" SKIP is never printed or counted as a PASS. Every check below
# prints exactly one of PASS / FAIL / SKIPPED, and the gate's own exit code
# reflects only the checks that actually ran — never a silently-green skip.
#
# Usage: scripts/r32_acceptance_gate.sh   (run from repo root or anywhere;
# paths below are relative to the repo root, resolved via git).

set -uo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT" || exit 1

TLA_FILE="governance/proofs/R27KillSwitch.tla"
TLA_CFG="governance/proofs/R27KillSwitch.cfg"
COQ_FILE="governance/proofs/R27Corrigibility.v"

FAIL=0
declare -a RESULTS=()

report() { RESULTS+=("$1|$2|$3"); }

pass() { report "$1" "PASS" "$2"; echo "[PASS]    $1 -- $2"; }
fail() { report "$1" "FAIL" "$2"; echo "[FAIL]    $1 -- $2"; FAIL=1; }
skip() { report "$1" "SKIPPED" "$2"; echo "[SKIPPED] $1 -- $2"; }

echo "== R32 acceptance gate =="
echo "root: $ROOT"
echo

# ── 1. TLA+ file existence + the three named invariants ─────────────────────
echo "-- TLA+ structure --"
if [[ ! -f "$TLA_FILE" ]]; then
  fail "tla_file_exists" "$TLA_FILE does not exist"
else
  pass "tla_file_exists" "$TLA_FILE present"

  for name in KillLatchMonotone ContainedCannotDisableKill KillFiresWithin2Polls; do
    if grep -q "$name" "$TLA_FILE"; then
      pass "tla_has_${name}" "found in $TLA_FILE"
    else
      fail "tla_has_${name}" "MISSING from $TLA_FILE"
    fi
  done

  for kw in MODULE EXTENDS VARIABLES Init Next Spec; do
    if grep -qE "\\b${kw}\\b" "$TLA_FILE"; then
      pass "tla_syntax_${kw}" "keyword present"
    else
      fail "tla_syntax_${kw}" "keyword MISSING"
    fi
  done

  if grep -qE '\\\* *TODO|\\\* *STUB|ASSUME FALSE' "$TLA_FILE"; then
    fail "tla_anti_stub" "found a TODO/STUB/ASSUME FALSE marker -- not allowed"
  else
    pass "tla_anti_stub" "no stub markers found"
  fi
fi
echo

# ── 2. Coq file existence + the three named theorems ─────────────────────────
echo "-- Coq structure --"
if [[ ! -f "$COQ_FILE" ]]; then
  fail "coq_file_exists" "$COQ_FILE does not exist"
else
  pass "coq_file_exists" "$COQ_FILE present"

  for thm in kill_latch_monotone kill_fires_within_2_polls contained_cannot_disable_kill; do
    if grep -qE "(Theorem|Lemma|Corollary)[[:space:]]+${thm}\\b" "$COQ_FILE"; then
      pass "coq_has_${thm}" "found in $COQ_FILE"
    else
      fail "coq_has_${thm}" "MISSING from $COQ_FILE"
    fi
  done

  # NOTE: these two checks match `Admitted.`/`admit.` only when they appear as
  # a standalone Coq command (the real usage -- Admitted/admit end a proof
  # script and conventionally sit alone on their line), NOT as a substring of
  # running prose (e.g. a comment explaining "...never uses `Admitted.`" must
  # NOT trip this check -- that false positive was caught and fixed while
  # building this gate, see the R32 build session notes).
  if grep -qE '^[[:space:]]*Admitted\.[[:space:]]*$' "$COQ_FILE"; then
    fail "coq_no_admitted" "found a standalone 'Admitted.' -- an unproved goal is not allowed"
  else
    pass "coq_no_admitted" "no standalone 'Admitted.' found"
  fi
  if grep -qE '^[[:space:]]*(-|\+|\*)?[[:space:]]*admit\.[[:space:]]*$' "$COQ_FILE"; then
    fail "coq_no_admit_tactic" "found the standalone 'admit.' tactic -- not allowed"
  else
    pass "coq_no_admit_tactic" "no standalone 'admit.' tactic found"
  fi

  # Structural Qed-pairing: every Theorem/Lemma/Corollary block must close
  # with 'Qed.' before the next such block (or EOF). This is the fallback
  # proof-completeness signal when coqc itself isn't available (below).
  QED_REPORT="$(awk '
    /^(Theorem|Lemma|Corollary)[ \t]+[A-Za-z0-9_]+/ {
      if (name != "" && !closed) { print "UNCLOSED:" name }
      name=$2; sub(/[^A-Za-z0-9_].*$/, "", name); closed=0
    }
    /^[ \t]*Qed\.[ \t]*$/       { if (name != "") closed=1 }
    /^[ \t]*Admitted\.[ \t]*$/ { if (name != "") { print "ADMITTED:" name; closed=1 } }
    END { if (name != "" && !closed) { print "UNCLOSED:" name } }
  ' "$COQ_FILE")"
  if [[ -n "$QED_REPORT" ]]; then
    fail "coq_all_blocks_closed_with_qed" "$(echo "$QED_REPORT" | tr '\n' '; ')"
  else
    pass "coq_all_blocks_closed_with_qed" "every Theorem/Lemma/Corollary ends in Qed."
  fi
fi
echo

# ── 3. TLC model-checking (only if TLC is actually available) ────────────────
echo "-- TLC (TLA+ model checker) --"
TLC_JAR=""
for candidate in /usr/local/lib/tla2tools.jar "${HOME:-}/tla2tools.jar" "$ROOT/tla2tools.jar"; do
  [[ -n "$candidate" && -f "$candidate" ]] && TLC_JAR="$candidate" && break
done

TLC_CMD=""
if command -v tlc >/dev/null 2>&1; then
  TLC_CMD="tlc"
elif [[ -n "$TLC_JAR" ]] && command -v java >/dev/null 2>&1; then
  TLC_CMD="java -jar $TLC_JAR"
fi

if [[ -z "$TLC_CMD" ]]; then
  skip "tlc_model_check" "SKIPPED: TLC not installed (no 'tlc' binary, no tla2tools.jar found on this host)"
else
  if [[ -f "$TLA_FILE" && -f "$TLA_CFG" ]]; then
    TLC_OUT="$($TLC_CMD "$TLA_FILE" -config "$TLA_CFG" -workers auto 2>&1)"
    if echo "$TLC_OUT" | grep -qiE "error|invariant.*violated|is violated"; then
      fail "tlc_model_check" "TLC reported a problem: $(echo "$TLC_OUT" | grep -iE 'error|violated' | head -1)"
    elif echo "$TLC_OUT" | grep -qi "No error has been found"; then
      pass "tlc_model_check" "TLC: no invariant violations found"
    else
      fail "tlc_model_check" "TLC ran but did not report success -- inspect output"
    fi
  else
    fail "tlc_model_check" "TLC is available but $TLA_FILE or $TLA_CFG is missing"
  fi
fi
echo

# ── 4. coqc compile (only if coqc is actually available) ─────────────────────
echo "-- coqc (Coq compiler) --"
if ! command -v coqc >/dev/null 2>&1; then
  skip "coqc_compile" "SKIPPED: coqc not installed on this host"
else
  if [[ -f "$COQ_FILE" ]]; then
    COQ_OUT="$(coqc "$COQ_FILE" 2>&1)"
    COQ_EXIT=$?
    if [[ $COQ_EXIT -eq 0 ]]; then
      pass "coqc_compile" "coqc exited 0"
    else
      fail "coqc_compile" "coqc exited $COQ_EXIT: $(echo "$COQ_OUT" | tail -5 | tr '\n' ' ')"
    fi
  else
    fail "coqc_compile" "coqc is available but $COQ_FILE is missing"
  fi
fi
echo

# ── Summary ────────────────────────────────────────────────────────────────
echo "== Summary =="
PASS_N=0; FAIL_N=0; SKIP_N=0
for r in "${RESULTS[@]}"; do
  IFS='|' read -r name status detail <<< "$r"
  printf "  %-38s %s\n" "$name" "$status"
  case "$status" in
    PASS) PASS_N=$((PASS_N+1));;
    FAIL) FAIL_N=$((FAIL_N+1));;
    SKIPPED) SKIP_N=$((SKIP_N+1));;
  esac
done
echo
echo "  PASS=$PASS_N FAIL=$FAIL_N SKIPPED=$SKIP_N"
echo

if [[ $FAIL -ne 0 ]]; then
  echo "R32 GATE: FAIL"
  exit 1
else
  if [[ $SKIP_N -gt 0 ]]; then
    echo "R32 GATE: PASS on all runnable checks (${SKIP_N} check(s) SKIPPED -- TLC/coqc"
    echo "  not installed on this host. SKIPPED is not a substitute for PASS: it is an"
    echo "  honest report that the check did not run at all.)"
  else
    echo "R32 GATE: PASS (all checks ran, none skipped)"
  fi
  exit 0
fi
