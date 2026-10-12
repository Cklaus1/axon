#!/usr/bin/env bash
# Grade the candidates under candidates/ against suite/accept.ax with
# `axon test --seal`, the way an automated grader would: a cleared environment
# (`env -i`), modules only from AXON_PATH (AXON_PATH_EXCLUSIVE=1), NDJSON output.
#
# usage: bash examples/sealed/run.sh [path/to/axon]
#        (default: $AXON, else `axon` on PATH)
#
# Verdicts, read from the exit code and the NDJSON summary line:
#   PASS     exit 0, summary present, "failed":0
#   FAIL     exit 3, summary present (at least one operator test failed)
#   REFUSED  exit 2, no summary (check error such as E0001/E0004, or engine refusal)
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
  out=$(env -i PATH=/usr/bin:/bin AXON_PATH="$SUITE:$cand" AXON_PATH_EXCLUSIVE=1 "$@" \
        "$BIN" test "$SUITE/accept.ax" --json "${seal[@]}" 2>"$ERRF")
  rc=$?
  summary=$(grep '^{"type":"summary"' <<<"$out")
  DETAIL=$summary
  case $rc in
    0) if grep -q '"failed":0' <<<"$summary"; then VERDICT=PASS; else VERDICT="ERROR(exit 0)"; fi ;;
    3) if [ -n "$summary" ]; then VERDICT=FAIL; else VERDICT="ERROR(exit 3, no summary)"; fi ;;
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
  printf '%-8s %-44s expected=%-7s actual=%-7s %s\n' "$mark" "$label" "$want" "$VERDICT" "$DETAIL"
}

row PASS    "honest"                         honest            seal
row FAIL    "wrong"                          wrong             seal
row REFUSED "attack_reference"               attack_reference  seal
row REFUSED "attack_answer_key"              attack_answer_key seal
row FAIL    "attack_own_test"                attack_own_test   seal
row REFUSED "honest with AXON_ENGINE=vm"     honest            seal   AXON_ENGINE=vm
row PASS    "control: attack_reference, no --seal" attack_reference noseal

exit $fail
