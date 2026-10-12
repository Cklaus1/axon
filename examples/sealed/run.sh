#!/usr/bin/env bash
# Grade the candidates under candidates/ against suite/accept.ax with
# `axon test --seal`, the way an automated grader would: a cleared environment
# (`env -i`), modules only from AXON_PATH (AXON_PATH_EXCLUSIVE=1), no effects
# granted (AXON_ALLOWED_EFFECTS empty), a wall-clock limit, NDJSON output.
#
# usage: bash examples/sealed/run.sh [path/to/axon]
#        (default: $AXON, else `axon` on PATH)
#
# Verdicts, read from the exit code and the LAST stdout line (the summary):
#   PASS     exit 0, summary present, "failed":0
#   FAIL     exit 3, summary present (an operator test failed, including a
#            test stopped by an effect refusal such as `exit` without IO)
#   REFUSED  exit 2, no summary (check error such as E0001/E0004, or engine refusal)
# The key rows run with --completion-key-stdin and check per-test tokens
# with check_tokens.py (needs python3).
# Prints one line per row and exits 0 iff every row's verdict is the expected one.
set -u

BIN=${1:-${AXON:-axon}}
BIN=$(command -v "$BIN") || { echo "axon binary not found: ${1:-${AXON:-axon}}" >&2; exit 2; }
BIN=$(readlink -f "$BIN")
HERE=$(cd "$(dirname "$0")" && pwd)
SUITE=$HERE/suite
ERRF=$(mktemp)
trap 'rm -f "$ERRF"' EXIT
fail=0

# grade <candidate> <seal|noseal> [VAR=value ...]
# Sets VERDICT and DETAIL.
grade() {
  local cand=$HERE/candidates/$1 mode=$2; shift 2
  local seal=()
  [ "$mode" = seal ] && seal=(--seal "$cand")
  local out rc summary
  out=$(env -i PATH=/usr/bin:/bin AXON_PATH="$SUITE:$cand" AXON_PATH_EXCLUSIVE=1 \
        AXON_ALLOWED_EFFECTS= "$@" \
        timeout 60 "$BIN" test "$SUITE/accept.ax" --json "${seal[@]}" 2>"$ERRF")
  rc=$?
  # axon prints the real summary as its LAST stdout line. Candidate output
  # (println, if IO were granted) shares stdout and can contain a forged one.
  summary=$(tail -n 1 <<<"$out" | grep '^{"type":"summary"')
  DETAIL=$summary
  case $rc in
    0) if grep -q '"failed":0' <<<"$summary"; then VERDICT=PASS; else VERDICT="ERROR(exit 0)"; fi ;;
    3) if [ -n "$summary" ]; then VERDICT=FAIL; else VERDICT="ERROR(exit 3, no summary)"; fi
       # First failed test's message.
       DETAIL=$(grep -m 1 '"status":"failed"' <<<"$out" | sed -E 's/.*"message":"(.*)"}$/\1/') ;;
    2) if [ -z "$summary" ]; then VERDICT=REFUSED; else VERDICT="ERROR(exit 2 with a summary)"; fi
       # First diagnostic: its code and message, or the plain error line.
       DETAIL=$(head -n 1 "$ERRF" | sed -E 's/.*"code":"([A-Z0-9]+)".*"message":"([^"]*)".*/\1 \2/') ;;
    *) VERDICT="ERROR(exit $rc)"; DETAIL=$(head -c 300 "$ERRF") ;;
  esac
}

# row <expected> <label> <candidate> <seal|noseal> [VAR=value ...]
row() {
  local want=$1 label=$2; shift 2
  grade "$@"
  local mark=ok
  if [ "$VERDICT" != "$want" ]; then mark=MISMATCH; fail=1; fi
  printf '%-8s %-42s expected=%-7s actual=%-7s %s\n' "$mark" "$label" "$want" "$VERDICT" "$DETAIL"
}

row PASS    "honest"                         honest            seal
row FAIL    "wrong"                          wrong             seal
row REFUSED "attack_reference"               attack_reference  seal
row REFUSED "attack_answer_key"              attack_answer_key seal
row FAIL    "attack_own_test"                attack_own_test   seal
row FAIL    "attack_exit"                    attack_exit       seal
row REFUSED "honest with AXON_ENGINE=vm"     honest            seal   AXON_ENGINE=vm
# Controls: the same attacks pass once the grader drops a safeguard.
row PASS    "control: attack_reference, no --seal"   attack_reference noseal
row PASS    "control: attack_exit, IO granted"       attack_exit      seal   AXON_ALLOWED_EFFECTS=IO

# --completion-key-stdin: with a per-run key, a test passes only if its "ok"
# line carries the keyed completion token (check_tokens.py). exit(0) gets no
# token even with IO granted, so the key alone also catches attack_exit.
TESTS="small_inputs_match_the_answer_key negative_and_large_inputs_match_the_reference"
# token_row <expected VERIFIED|NO_TOKEN> <label> <candidate> [VAR=value ...]
token_row() {
  local want=$1 label=$2 cand=$HERE/candidates/$3; shift 3
  local key out got mark=ok
  key=$(python3 -c 'import secrets; print(secrets.token_hex(32))')
  out=$(echo "$key" | env -i PATH=/usr/bin:/bin AXON_PATH="$SUITE:$cand" AXON_PATH_EXCLUSIVE=1 \
        AXON_ALLOWED_EFFECTS= "$@" \
        timeout 60 "$BIN" test "$SUITE/accept.ax" --json --completion-key-stdin --seal "$cand" 2>/dev/null)
  # shellcheck disable=SC2086
  if python3 "$HERE/check_tokens.py" "$key" <(printf '%s\n' "$out") $TESTS >/dev/null; then
    got=VERIFIED
  else
    got=NO_TOKEN
  fi
  if [ "$got" != "$want" ]; then mark=MISMATCH; fail=1; fi
  printf '%-8s %-42s expected=%-8s actual=%-8s\n' "$mark" "$label" "$want" "$got"
}

token_row VERIFIED "key: honest"                        honest
token_row NO_TOKEN "key: attack_exit, IO granted"       attack_exit AXON_ALLOWED_EFFECTS=IO

exit $fail
