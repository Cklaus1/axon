# child_exit.sh — refuse to let a child's ABNORMAL death read as a result.
#
# Parity harnesses capture a program's stdout and compare it:
#
#     native_out="$("$BIN" 2>/dev/null)"
#     [ "$interp_out" = "$native_out" ] && echo OK
#
# The exit status of the command substitution was never read. MEASURED on
# exec_parity.sh: a native binary that printed its normal output and was then
# SIGKILLed (exit 137 — the shape of a cgroup OOM kill) produced
# "exec_parity: OK … exec matches the interpreter", harness exit 0. Output
# that was fully written before the kill is byte-identical, so a comparison of
# output alone cannot see the death.
#
# The status CANNOT be classified on its own. A nonzero exit is often a
# legitimate result these harnesses compare on purpose (a panic is 101, a refine
# violation is 6), and — measured, not assumed — an Axon program can itself exit
# 137 or 200: `fn main() -> i64 { 137 }` and `exit(200)` both do, because
# EXIT_CODES.md passes 126..=255 through "as written". So "above 128" is not
# proof of a signal. Two rules follow:
#
#   1. `abnormal_exit` flags 124 (timeout) and every status above 128. A parity
#      harness treats one as a FAILURE even when BOTH engines agree on it: two
#      engines OOM-killed under the same cgroup cap agree perfectly and prove
#      nothing. A harness whose program deliberately exits above 128 must not
#      use `same_exit_or_fail` for that case, and must say so where it opts out.
#   2. Comparing EXIT STATUSES between engines is required alongside stdout,
#      because output written before a kill is byte-identical.
#
# Usage:
#     interp_out="$("$AXON" run "$PROG" 2>/dev/null)"; interp_st=$?
#     native_out="$("$BIN" 2>/dev/null)";             native_st=$?
#     same_exit_or_fail my_parity "$interp_st" "$native_st" || exit 1

# abnormal_exit <status> → 0 (true) if the status is a timeout or a signal death
abnormal_exit() {
  local s="${1:-}"
  case "$s" in ''|*[!0-9]*) return 0 ;; esac   # no status at all is not a result either
  [ "$s" -eq 124 ] && return 0
  [ "$s" -gt 128 ] && return 0
  return 1
}

# describe_exit <status> → a human reason, e.g. "exit 137 = SIGKILL (OOM kill or kill -9)"
describe_exit() {
  local s="${1:-}"
  case "$s" in
    ''|*[!0-9]*) echo "no exit status recorded" ;;
    124) echo "exit 124 = wall-clock timeout" ;;
    137) echo "exit 137 = SIGKILL (OOM kill or kill -9)" ;;
    *)
      if [ "$s" -gt 128 ]; then
        local sig; sig="$(kill -l $(( s - 128 )) 2>/dev/null)"
        echo "exit $s = killed by signal $(( s - 128 ))${sig:+ (SIG$sig)}"
      else
        echo "exit $s"
      fi ;;
  esac
}

# same_exit_or_fail <harness> <interp_status> <native_status>
#   Fails (returns 1, printing why) when the two engines exited differently, or
#   when either exited abnormally without the other matching it exactly.
same_exit_or_fail() {
  local h="$1" i="$2" n="$3"
  if [ "$i" != "$n" ]; then
    echo "$h: FAIL — exit status diverges: interp $(describe_exit "$i"), native $(describe_exit "$n")"
    echo "$h:   identical stdout does not make a killed process a result"
    return 1
  fi
  if abnormal_exit "$i"; then
    echo "$h: FAIL — both engines ended with $(describe_exit "$i"); a timeout or signal death is not a parity result"
    return 1
  fi
  return 0
}
