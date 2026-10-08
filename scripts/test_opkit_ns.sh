#!/usr/bin/env bash
# test_opkit_ns.sh — the namespace helper (scripts/lib/opkit_ns.sh) refuses when its proof fails.
# Amendment 92, extended by amendment 97. Every assertion attack points the assertion at SCRATCH
# directories and a stand-in for the host (the *_FOR_TEST variables, honoured only because this very
# script is the outermost script); nothing here touches a real destination, and ns_run's own shadowing
# is done inside private namespaces.
# Exit 0 all held; 1 an assertion failed (the message starts "ATTACK:" when a refusal did not happen);
# 77 not root or no unshare.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
SELF=$HERE/$(basename "$0")
LIB=$HERE/lib/opkit_ns.sh

# ── child mode: this script re-executed INSIDE a fresh mount namespace, so the helper sees its caller ──
if [ "${1:-}" = --child ]; then
  mode=$2 W=$3
  # RO=1: the root is made read-only first, as ns_run does, and the tmpfs shadows are laid over it afterwards
  if [ "${RO:-0}" = 1 ]; then . "$LIB"; opkit_ns_make_ro || exit 92; export OPKIT_REQUIRE_RO=1; fi
  mount -t tmpfs tmpfs "$W/a" || exit 90
  if [ "${TMPFS_B:-1}" = 1 ]; then mount -t tmpfs tmpfs "$W/b" || exit 90
  else mkdir -p "$W/disk" && mount --bind "$W/disk" "$W/b" || exit 90   # shadowed by a DISK directory: a different object, not a tmpfs
  fi
  export OPKIT_DESTS_FOR_TEST="$W/a $W/b" OPKIT_VIEW_PID_FOR_TEST=${VIEW:?} OPKIT_NS_PID_FOR_TEST=${NSPID:?}
  [ "$VIEW" = self ] && export OPKIT_VIEW_PID_FOR_TEST=$$
  [ "$NSPID" = self ] && export OPKIT_NS_PID_FOR_TEST=$$
  . "$LIB"
  case "$mode" in
    assert) opkit_ns_assert ;;
    # what a command can WRITE once make_ro and the shadows are in place, judged on its own (not through the proof)
    rowrite) python3 -c '
import os, sys
bad = 0
for d in os.environ.get("PROBE_DIRS", "/opt /home /root /usr/lib /boot /var/cache").split():
    if not os.path.isdir(d):
        continue
    # the mount flag, read without writing a byte to a real place (statvfs: ST_RDONLY)
    if not os.statvfs(d).f_flag & os.ST_RDONLY:
        print("WRITABLE", d); bad = 1
sys.exit(bad)' ;;
    *) exit 91 ;;
  esac
  exit $?
fi

[ "$(id -u)" = 0 ] && command -v unshare >/dev/null || { echo "SKIP: needs root and unshare"; exit 77; }
W=$(mktemp -d "${TMPDIR:-/var/tmp}/axon-opkit-ns-test.XXXXXX") || exit 1
trap 'rm -rf "$W"' EXIT
mkdir -p "$W/a" "$W/b" "$W/scratch"
fail() { echo "FAIL: $*"; exit 1; }

# Runs the assertion in a fresh private namespace with $W/a (and $W/b when TMPFS_B=1) a tmpfs.
assert_in_ns() { # VIEW NSPID -> prints stderr, exits with the assertion's rc
  VIEW=$1 NSPID=$2 TMPFS_B=${TMPFS_B:-1} unshare -m --propagation private bash "$SELF" --child assert "$W" 2>&1
}

o=$(assert_in_ns 1 1); rc=$?
[ $rc = 0 ] || fail "control: a namespace with every destination a tmpfs was refused ($rc): $o"
echo "ok: control: a namespace whose destinations are tmpfs is accepted"

o=$(TMPFS_B=0 assert_in_ns 1 1); rc=$?
[ $rc = 1 ] || fail "ATTACK: a destination that is not a tmpfs was accepted (rc $rc): $o"
grep -q 'is not shadowed by a tmpfs' <<<"$o" || fail "refused for another reason (rc $rc): $o"
echo "ok: a destination that is not a tmpfs is refused"

o=$(assert_in_ns 1 self); rc=$?       # "the host namespace" is our own: the same one
[ $rc = 1 ] || fail "ATTACK: running in the host's own mount namespace was accepted (rc $rc): $o"
grep -q "host's mount namespace" <<<"$o" || fail "refused for another reason (rc $rc): $o"
echo "ok: running in the host's mount namespace is refused"

# the "host" view is this very namespace's view of itself: the destination IS the host's directory
o=$(assert_in_ns self 1); rc=$?
[ $rc = 1 ] || fail "ATTACK: a destination that is the host's own directory was accepted (rc $rc): $o"
grep -q "is the HOST's own directory" <<<"$o" || fail "refused for another reason (rc $rc): $o"
echo "ok: a destination that is the host's own directory (same device:inode) is refused"

# amendment 97 (d): an UNREADABLE host view proves nothing, so it refuses (it used to pass vacuously)
o=$(assert_in_ns 999999999 1); rc=$?
[ $rc = 1 ] || fail "ATTACK: an unreadable host view was taken as 'not visible' (rc $rc): $o"
grep -q 'cannot be examined' <<<"$o" || fail "refused for another reason (rc $rc): $o"
echo "ok: an unreadable host view refuses (it does not pass vacuously)"

# amendment 97 (e): the FOR_TEST overrides are honoured for this script ONLY. Any other caller that
# sets one is refused, never silently weakened.
cat >"$W/other.sh" <<OTHER
. "$LIB"
opkit_ns_assert
OTHER
# Each caller runs in a namespace where the override WOULD pass (a fresh mount namespace, $W/a a tmpfs), so a
# refusal can only be the caller check; the control is this script's own child (above), which passes.
other() { # SCRIPT-OR-"bash -c" ...
  unshare -m --propagation private bash -c 'mount -t tmpfs tmpfs "$1/a" && shift && exec env OPKIT_VIEW_PID_FOR_TEST=1 OPKIT_NS_PID_FOR_TEST=1 OPKIT_DESTS_FOR_TEST="$W/a" "$@"' bash "$W" "$@" 2>&1
}
export W
o=$(other bash "$W/other.sh"); rc=$?
[ $rc = 1 ] || fail "ATTACK: an OPKIT_*_FOR_TEST override from another script was honoured (rc $rc): $o"
grep -q 'outside scripts/test_opkit_ns.sh' <<<"$o" || fail "refused for another reason (rc $rc): $o"
o=$(other bash -c ". '$LIB'; opkit_ns_assert"); rc=$?
[ $rc = 1 ] || fail "ATTACK: an OPKIT_*_FOR_TEST override from a bash -c caller was honoured (rc $rc): $o"
grep -q 'outside scripts/test_opkit_ns.sh' <<<"$o" || fail "refused for another reason (rc $rc): $o"
echo "ok: the *_FOR_TEST overrides are refused for any caller but this script"

# amendment 101: make_ro on its own: after it and the shadows, nothing unlisted can be written (judged without the proof;
# placed before every ns_run test, which would refuse a root make_ro left writable before this ran)
o=$(RO=1 VIEW=1 NSPID=1 unshare -m --propagation private bash "$SELF" --child rowrite "$W" 2>&1); rc=$?
[ $rc = 0 ] || fail "ATTACK: make_ro left a place nobody listed writable (rc $rc): $o"
mkdir -p "$W/probe"   # the control probes a scratch directory, never a real place
o=$(PROBE_DIRS="$W/probe" RO=0 VIEW=1 NSPID=1 unshare -m --propagation private bash "$SELF" --child rowrite "$W" 2>&1); rc=$?
[ $rc = 1 ] && grep -q '^WRITABLE' <<<"$o" || fail "control: without make_ro the probe should find writable places (rc $rc): $o"
echo "ok: after make_ro and the shadows no place nobody listed is writable (and the probe sees them without it)"
# ns_run never starts its command when the proof fails (here: a file it must install is missing)
M=$W/scratch/ran
o=$(OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch OPKIT_EXTRA=/nonexistent/file bash -c '
    . "$OPKIT_LIB"; ns_run touch "$1"' bash "$M" 2>&1); rc=$?
[ ! -e "$M" ] && [ $rc = 97 ] \
  || fail "ATTACK: ns_run ran its command although the isolation was not proved (rc $rc, marker $([ -e "$M" ] && echo present || echo absent)): $o"
echo "ok: ns_run refuses (97) and never starts the command when the proof fails"
M2=$W/scratch/ran2
o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M2" 2>&1); rc=$?
{ [ -e "$M2" ] && [ $rc = 0 ]; } || fail "control: ns_run did not run its command in a proved namespace ($rc): $o"
echo "ok: control: ns_run runs its command in a proved namespace"

# amendment 97 (b): the namespace is mount + PID + UTS + IPC + NET, each different from the host's
here="$(readlink /proc/self/ns/mnt) $(readlink /proc/self/ns/pid) $(readlink /proc/self/ns/uts) $(readlink /proc/self/ns/ipc) $(readlink /proc/self/ns/net)"
inside=$(OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"
  ns_run sh -c '"'"'echo "$(readlink /proc/self/ns/mnt) $(readlink /proc/self/ns/pid) $(readlink /proc/self/ns/uts) $(readlink /proc/self/ns/ipc) $(readlink /proc/self/ns/net) pid=$$"'"'"'' 2>&1)
i=0
for h in $here; do
  i=$((i + 1)); n=$(awk -v i=$i '{print $i}' <<<"$inside")
  [ -n "$n" ] && [ "$n" != "$h" ] || fail "ATTACK: ns_run left a namespace shared with the host (field $i: $h; inside: $inside)"
done
echo "ok: ns_run's command runs in its own mount, PID, UTS, IPC and network namespaces"
net=$(OPKIT_NET=host OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run sh -c '"'"'readlink /proc/self/ns/net'"'"'' 2>&1)
[ "$net" = "$(readlink /proc/self/ns/net)" ] || fail "OPKIT_NET=host did not keep the host network (needed by the registry download): $net"
echo "ok: OPKIT_NET=host keeps the host network, and only that"
# ── amendment 101 ───────────────────────────────────────────────────────────────────────────────────
# (1) the descriptor the proof compared through is closed before the command starts: the command lists
# its descriptors and finds no directory, and no OPKIT_HOST_FD to find one by
o=$(OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"
  ns_run bash -c '"'"'echo "HOSTFD=${OPKIT_HOST_FD:-unset}"
    for f in /proc/self/fd/*; do case ${f##*/} in 0|1|2) continue ;; esac; [ -d "$f" ] && echo "DIRFD ${f##*/} $(readlink "$f") $(stat -L -c %d:%i "$f")"; done; echo done'"'"'' 2>&1)
grep -q '^done$' <<<"$o" || fail "setup: the descriptor listing did not run: $o"
grep -q '^HOSTFD=unset$' <<<"$o" || fail "ATTACK: the command still sees OPKIT_HOST_FD: $o"
! grep -q '^DIRFD' <<<"$o" || fail "ATTACK: the command inherited a descriptor on the host's filesystem: $o"
echo "ok: the command inherits no directory descriptor (the host-root handle is closed after the proof)"
# the detector flags a descriptor left open, and is quiet without one
o=$(bash -c '. "$1"; exec 9</; opkit_ns_fd_leak' bash "$LIB" 2>&1); rc=$?
{ [ $rc = 1 ] && grep -q 'LEAK: descriptor 9' <<<"$o"; } || fail "ATTACK: a directory descriptor left open was not flagged (rc $rc): $o"
o=$(bash -c '. "$1"; opkit_ns_fd_leak' bash "$LIB" 2>&1); rc=$?
[ $rc = 0 ] || fail "control: the descriptor check flagged a process with no directory descriptor (rc $rc): $o"
echo "ok: a directory descriptor left open is detected"
# (2) deny by default. make_ro is judged on its own first, then the proof that refuses a writable root, then ns_run end to end
# the proof itself: a namespace whose root is not read-only is refused, a read-only one is accepted
o=$(RO=0 OPKIT_REQUIRE_RO=1 VIEW=1 NSPID=1 unshare -m --propagation private env OPKIT_REQUIRE_RO=1 bash "$SELF" --child assert "$W" 2>&1); rc=$?
[ $rc = 1 ] || fail "ATTACK: a namespace whose root is writable was accepted (rc $rc): $o"
grep -q 'the root is not read-only' <<<"$o" || fail "refused for another reason (rc $rc): $o"
o=$(RO=1 VIEW=1 NSPID=1 unshare -m --propagation private bash "$SELF" --child assert "$W" 2>&1); rc=$?
[ $rc = 0 ] || fail "control: a namespace with a read-only root and writable shadows was refused ($rc): $o"
echo "ok: a namespace whose root is writable is refused; a read-only root with writable shadows is accepted"
# end to end: every place nobody listed is read-only; the shadows, /tmp and OPKIT_RW are writable
mkdir -p "$W/rw" "$W/notrw"
o=$(OPKIT_RW="$W/rw" OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"
  ns_run python3 -c '"'"'
import errno, os, sys
bad = 0
for d in ("/opt", "/home", "/root", "/usr/lib", "/usr", "/boot", "/var/cache", "/var", "/bin", "/sys", "/dev", sys.argv[2]):
    if not os.path.isdir(d):
        continue
    try:
        open(d + "/.opkit-erofs-probe", "w").close()
        print("WRITABLE", d); bad = 1
        os.unlink(d + "/.opkit-erofs-probe")
    except OSError as e:
        print("denied", d, errno.errorcode.get(e.errno)) if e.errno == errno.EROFS else (print("OTHER", d, e), None)
for d in ("/tmp", "/usr/local", "/var/lib", "/etc", "/run", sys.argv[1]):
    try:
        open(d + "/.opkit-rw-probe", "w").close(); os.unlink(d + "/.opkit-rw-probe"); print("writable", d)
    except OSError as e:
        print("NOTWRITABLE", d, e); bad = 1
sys.exit(bad)'"'"' "$OPKIT_RW" "$1"' bash "$W/notrw" 2>&1); rc=$?
[ $rc != 97 ] || fail "ns_run refused (97) instead of making the root read-only (another layer refused it; not the attack): $o"
[ $rc = 0 ] || fail "ATTACK: a place nobody listed is writable inside ns_run (rc $rc): $o"
for d in /opt /home /root /usr/lib; do [ -d "$d" ] && { grep -q "denied $d EROFS" <<<"$o" || fail "ATTACK: $d did not answer EROFS: $o"; }; done
grep -q "denied $W/notrw EROFS" <<<"$o" || fail "ATTACK: a scratch directory the caller did not name is writable: $o"
for d in /opt /home /root /usr/lib /boot /var/cache; do [ ! -e "$d/.opkit-erofs-probe" ] || fail "ATTACK: a probe reached the host's $d"; done
echo "ok: deny by default: /opt /home /root /usr/lib /boot /var/cache and an unnamed scratch dir answer EROFS; the shadows, /tmp and OPKIT_RW stay writable"
# (3) inside the namespace the re-assertion rests on the proof's stamp; without it the host's view is needed and the namespace is refused
o=$(OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"
  ns_run bash -c '"'"'. "$OPKIT_LIB"; opkit_ns_assert && echo REASSERT-OK; rm -f "$OPKIT_STAMP"; opkit_ns_assert && echo ACCEPTED-WITHOUT-STAMP'"'"'' 2>&1)
grep -q REASSERT-OK <<<"$o" || fail "control: the re-assertion inside ns_run was refused: $o"
! grep -q ACCEPTED-WITHOUT-STAMP <<<"$o" || fail "ATTACK: the re-assertion accepted a namespace whose proof stamp was removed: $o"
echo "ok: a re-assertion inside ns_run rests on the proof's stamp and refuses without it"
# a stamp that is not this namespace's proof: another namespace's ids, or one that claims the host's namespace is this one
o=$(OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"
  ns_run bash -c '"'"'. "$OPKIT_LIB"
    s=$(cat "$OPKIT_STAMP"); echo "mnt=mnt:[1],${s#*,}" >"$OPKIT_STAMP"; opkit_ns_assert && echo ACCEPTED-STAMP-OF-ANOTHER-NAMESPACE
    ids=$(opkit_ns_ids); echo "$ids $ids" >"$OPKIT_STAMP"; opkit_ns_assert && echo ACCEPTED-STAMP-NAMING-THE-HOST-AS-THIS-NAMESPACE
    echo done'"'"'' 2>&1)
grep -q '^done$' <<<"$o" || fail "setup: the forged-stamp commands did not run: $o"
! grep -q ACCEPTED-STAMP-OF-ANOTHER-NAMESPACE <<<"$o" || fail "ATTACK: a stamp recording another namespace's ids was accepted: $o"
! grep -q ACCEPTED-STAMP-NAMING-THE-HOST <<<"$o" || fail "ATTACK: a stamp whose host namespace is this namespace was accepted: $o"
echo "ok: a forged stamp (another namespace's ids; the host recorded as this namespace) is refused"
echo "PASS: opkit namespace helper"
