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
  # amendment 105: the mechanisms on their own, in a throw-away mount namespace and with no proof in the way
  case "$mode" in
    nsrun) . "$LIB"; shift 3; ns_run "$@"; exit $? ;;       # ns_run with this script as the outermost one (the *_FOR_TEST overrides are honoured)
    mnttmpfs)
      # amendment 109: a tmpfs over the host's $4 (private to this throw-away namespace) is "a host directory outside every temp
      # root" that can be made and written without a byte reaching the real one. $W/rw109 is the one valid scratch directory.
      . "$LIB"; MNTD=$4; mount -t tmpfs -o mode=0755 tmpfs "$MNTD" || exit 90
      mkdir -p "$MNTD/rw" "$MNTD/scr" "$MNTD/tmp1/x" "$W/rw109" "$W/scr109" || exit 90
      M=$W/rw109/ran
      tryrun() { # EXPECT(97|0) LABEL MARKER VAR=val... -> ns_run touch MARKER with those variables set
        local want=$1 label=$2 mark=$3 o rc; shift 3
        rm -f "$mark"
        o=$(env "$@" OPKIT_LIB=$LIB bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$mark" 2>&1 </dev/null); rc=$?
        if [ "$want" = 97 ]; then
          { [ ! -e "$mark" ] && [ $rc = 97 ]; } || { echo "ATTACK: $label was accepted (rc $rc, marker $([ -e "$mark" ] && echo present || echo absent)): $o"; exit 1; }
        else
          { [ -e "$mark" ] && [ $rc = 0 ]; } || { echo "control: $label was refused (rc $rc): $o"; exit 1; }
        fi
        LASTOUT=$o
      }
      before=$(ls /var/tmp | grep -c '^opkit-ns\.' || true)
      # TMPDIR is ignored (the helper never consults it): a TMPDIR naming a host directory changes nothing, and does not make one a temp root
      tryrun 0 "TMPDIR=$MNTD (ignored: the helper never consults it)" "$M" TMPDIR="$MNTD" OPKIT_SCRATCH="$W/scr109" OPKIT_RW="$W/rw109"
      tryrun 97 "TMPDIR=$MNTD OPKIT_RW=$MNTD/rw (the environment named a temp root and a directory under it)" "$MNTD/rw/ran" TMPDIR="$MNTD" OPKIT_RW="$MNTD/rw" OPKIT_SCRATCH="$W/scr109"
      grep -q 'not strictly below /var/tmp:' <<<"$LASTOUT" || { echo "refused for another reason (the temp-root rule alone was to refuse it): $LASTOUT"; exit 1; }
      tryrun 97 "OPKIT_SCRATCH=$MNTD/scr (not below a temp root)" "$M" OPKIT_SCRATCH="$MNTD/scr" OPKIT_RW="$W/rw109"
      grep -q 'OPKIT_SCRATCH' <<<"$LASTOUT" || { echo "refused for another reason (OPKIT_SCRATCH alone was to refuse it): $LASTOUT"; exit 1; }
      ! grep -q 'isolation not proved' <<<"$LASTOUT" || { echo "ATTACK: ns_run mounted and unshared before it validated OPKIT_SCRATCH (the namespace's own check refused it): $LASTOUT"; exit 1; }
      [ -z "$(ls -A "$MNTD/scr")" ] || { echo "ATTACK: ns_run created $(ls -A "$MNTD/scr" | tr '\n' ' ') in a scratch directory the environment named"; exit 1; }
      # each rule of the scratch check on its own (a directory only ONE of them refuses), for OPKIT_SCRATCH
      mkdir -p "$W/scr-open" "$W/scr-real"; chmod 0777 "$W/scr-open"; ln -sfn "$W/scr-real" "$W/scr-link"
      for badscr in "$W/scr-open" "$W/scr-link" /var/tmp /tmp relative/dir "$W/scr-missing"; do
        tryrun 97 "OPKIT_SCRATCH='$badscr'" "$M" OPKIT_SCRATCH="$badscr" OPKIT_RW="$W/rw109"
      done
      # the primitive, called directly: opkit_rw_validate no longer trusts TMPDIR for a temp root; opkit_ns_isolate refuses a scratch the environment named
      o=$(TMPDIR="$MNTD/tmp1" opkit_rw_validate "$MNTD/tmp1/x" 2>&1); rc=$?
      { [ $rc = 1 ] && grep -q 'not strictly below /var/tmp:' <<<"$o"; } || { echo "ATTACK: opkit_rw_validate took \$TMPDIR for a temp root (rc $rc): $o"; exit 1; }
      o=$(OPKIT_NS_PID_FOR_TEST=$STANDIN OPKIT_SCRATCH="$MNTD/scr" opkit_ns_isolate 2>&1); rc=$?
      { [ $rc = 1 ] && [ -z "$(ls -A "$MNTD/scr")" ]; } || { echo "ATTACK: opkit_ns_isolate worked in a scratch directory the environment named (rc $rc, $(ls -A "$MNTD/scr" | tr '\n' ' ')): $o"; exit 1; }
      # controls: /var/tmp itself is a fine TMPDIR; a scratch under it is fine; with no OPKIT_SCRATCH the helper makes its own and leaves nothing
      tryrun 0 "TMPDIR=/var/tmp with a valid scratch and OPKIT_RW" "$M" TMPDIR=/var/tmp OPKIT_SCRATCH="$W/scr109" OPKIT_RW="$W/rw109"
      tryrun 0 "no OPKIT_SCRATCH (the helper makes its own)" "$M" OPKIT_RW="$W/rw109"
      after=$(ls /var/tmp | grep -c '^opkit-ns\.' || true)
      [ "$before" = "$after" ] || { echo "ATTACK: ns_run left its own scratch root behind ($before -> $after opkit-ns.* directories in /var/tmp)"; exit 1; }
      exit 0 ;;
    sockfd)       # W FD MARKER: a unix socket end on descriptor FD, then ns_run (amendment 109)
      python3 -c '
import os, socket, sys
a, b = socket.socketpair()
os.dup2(a.fileno(), int(sys.argv[1])); a.close()
os.execvp("bash", ["bash", "-c", ". \"$OPKIT_LIB\"; ns_run touch \"$1\"", "bash", sys.argv[2]])' "$4" "$5"; exit $? ;;
    ptyfd)        # W MARKER: a pts slave on stdin, then ns_run
      python3 -c '
import os, pty, sys
m, sl = pty.openpty(); os.dup2(sl, 0)
os.execvp("bash", ["bash", "-c", ". \"$OPKIT_LIB\"; ns_run touch \"$1\"", "bash", sys.argv[1]])' "$4"; exit $? ;;
    pre)          # W KIND: the PURE precondition (it mounts nothing) in one context; prints RC=<n> (amendment 113)
      . "$LIB"
      FORGED="mnt=mnt:[1],pid=pid:[1],uts=uts:[1],ipc=ipc:[1],net=net:[1]"
      case "$4" in
        host-bare) opkit_ns_precondition probe; echo "RC=$?" ;;
        host-forged) OPKIT_HOST_NS=$FORGED opkit_ns_precondition probe; echo "RC=$?" ;;
        standin-self) OPKIT_NS_PID_FOR_TEST=$$ opkit_ns_precondition probe; echo "RC=$?" ;;
        standin-host) OPKIT_NS_PID_FOR_TEST=$STANDIN opkit_ns_precondition probe; echo "RC=$?" ;;
        init-forged) OPKIT_HOST_NS=$FORGED opkit_ns_precondition probe; echo "RC=$?" ;;
        noninit-forged) ( OPKIT_HOST_NS=$FORGED opkit_ns_precondition probe; echo "RC=$?" ) ;;
        *) exit 91 ;;
      esac
      exit 0 ;;
    bare)         # W PRIM [forged]: a destructive primitive called with NO proof inside a throw-away mount namespace (amendment 113)
      . "$LIB"; P=$4; SC=$W/bare-$P; mkdir -p "$SC/dmp" || exit 90
      [ "${5:-}" = forged ] && export OPKIT_HOST_NS="mnt=mnt:[1],pid=pid:[1],uts=uts:[1],ipc=ipc:[1],net=net:[1]"
      exec 9</
      before=$(awk '{ print $5, $6 }' /proc/self/mountinfo)
      case "$P" in
        isolate) OPKIT_SCRATCH=$SC opkit_ns_isolate ;;
        make_ro) opkit_ns_make_ro ;;
        fresh_proc) opkit_ns_fresh_proc ;;
        private_dev) opkit_ns_private_dev "$SC/dmp" ;;
        drop_host_fd) OPKIT_HOST_FD=9 opkit_ns_drop_host_fd ;;
        *) exit 91 ;;
      esac
      rc=$?
      after=$(awk '{ print $5, $6 }' /proc/self/mountinfo)
      echo "BARE rc=$rc mounts=$([ "$before" = "$after" ] && echo same || echo CHANGED) scratch=[$(ls -A "$SC" | tr '\n' ' ')] fd9=$([ -e /proc/self/fd/9 ] && echo open || echo CLOSED)"
      exit 0 ;;
    mknodc) mknod "$W/devs/$4" c "$5" "$6" 2>/dev/null; exit $? ;;       # W NAME MAJOR MINOR: a node in scratch, never under /dev
    devpriv)
      . "$LIB"; mkdir -p "$W/devmp" && OPKIT_NS_PID_FOR_TEST=$STANDIN opkit_ns_private_dev "$W/devmp" || exit 93
      echo "DEVLIST $(ls /dev | tr '\n' ' ')"
      { : >/dev/opkit-new-node; } 2>/dev/null && echo CREATED-IN-DEV
      umount -l /dev
      echo "AFTER-UMOUNT [$(ls /dev 2>&1 | tr '\n' ' ')]"
      exit 0 ;;
    procpriv)
      . "$LIB"
      mount -t proc proc /proc || exit 93          # a cover over the host's /proc, as unshare --mount-proc lays one
      OPKIT_NS_PID_FOR_TEST=$STANDIN opkit_ns_fresh_proc || exit 93
      python3 -c '
import os
for p in ("/proc/sysrq-trigger", "/proc/sys/kernel/hostname"):
    try:
        os.close(os.open(p, os.O_WRONLY)); print("OPENED-FOR-WRITE", p)
    except OSError as e:
        print("refused", p, e.errno)'
      umount -l /proc
      echo "AFTER-UMOUNT [$(ls /proc 2>&1 | tr '\n' ' ')]"
      exit 0 ;;
  esac
  # RO=1: the root is made read-only first, as ns_run does, and the tmpfs shadows are laid over it afterwards
  if [ "${RO:-0}" = 1 ]; then . "$LIB"; OPKIT_NS_PID_FOR_TEST=${NSPID:?} opkit_ns_make_ro || exit 92; export OPKIT_REQUIRE_RO=1; fi
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
# amendment 109: ns_run refuses a socket on fds 0-2 (a write through it reaches the peer). An ssh-driven run can hand a script
# one for stdin; the tests' own commands do not read it, so it is replaced here, and the socket tests below hand their own.
[ ! -S /proc/self/fd/0 ] || exec </dev/null
W=$(mktemp -d "/var/tmp/axon-opkit-ns-test.XXXXXX") || exit 1
trap 'rm -rf "$W"' EXIT
mkdir -p "$W/a" "$W/b" "$W/scratch"
fail() { echo "FAIL: $*"; exit 1; }

# the mechanisms, judged on their own (a throw-away mount namespace, no proof, no ns_run): a regression in one cannot hide behind the proof
o=$(STANDIN=$$ unshare -m --propagation private bash "$SELF" --child devpriv "$W" 2>&1); rc=$?
[ $rc = 0 ] && grep -q '^AFTER-UMOUNT' <<<"$o" || fail "setup: the private /dev mechanism did not run (rc $rc): $o"
dl=$(sed -n 's/^DEVLIST //p' <<<"$o")
[ "$(tr ' ' '\n' <<<"$dl" | grep -v '^$' | sort | tr '\n' ' ')" = "fd full null ptmx pts random shm stderr stdin stdout tty urandom zero " ] \
  || fail "ATTACK: the private /dev holds a node nobody listed (or lacks one): $dl"
! grep -q CREATED-IN-DEV <<<"$o" || fail "ATTACK: a file could be created in the private /dev (it is not read-only): $o"
# beneath the private /dev is the root filesystem's own (static) /dev directory; the host's DEVTMPFS must not be beneath it
for w in $(sed -n 's/^AFTER-UMOUNT \[\(.*\)\]$/\1/p' <<<"$o"); do
  case " console fd full null ptmx pts random shm stderr stdin stdout tty urandom zero " in *" $w "*) ;; *) fail "ATTACK: after umount /dev the host's devices show through ($w): $o" ;; esac
done
o=$(STANDIN=$$ unshare -m --propagation private bash "$SELF" --child procpriv "$W" 2>&1); rc=$?
[ $rc = 0 ] && grep -q '^AFTER-UMOUNT' <<<"$o" || fail "setup: the private /proc mechanism did not run (rc $rc): $o"
! grep -q OPENED-FOR-WRITE <<<"$o" || fail "ATTACK: a /proc file that reaches the host could be opened for writing (mechanism): $o"
grep -q '^AFTER-UMOUNT \[\]$' <<<"$o" || fail "ATTACK: after umount /proc the host's /proc shows through (mechanism): $o"
echo "ok: the private /dev (exactly the listed nodes, read-only, nothing beneath) and the private /proc (sysrq-trigger and sys read-only, nothing beneath) work on their own"
# ── amendment 113 ───────────────────────────────────────────────────────────────────────────────────
# Every DESTRUCTIVE primitive (opkit_ns_isolate, _make_ro, _fresh_proc, _private_dev, _drop_host_fd) starts with ONE function,
# opkit_ns_precondition, that refuses (97) unless this process is not in the host's mount namespace, with nothing changed. Round 12
# found isolate making `/` read-only BEFORE it refused the host's namespace. The attacks below call the primitives with no proof
# inside a throw-away private mount namespace as root (`unshare -m --propagation private`), so a guard that regressed damages
# that namespace and not the host; the host's own mount table is listed before and after the section.
hostmounts() { awk '{ print $5, $6 }' /proc/self/mountinfo | sort; }
HM_BEFORE=$(hostmounts)
prectx() { # KIND -> prints the child's output; the pure precondition in one context
  case $1 in
    standin-host) STANDIN=$$ unshare -m --propagation private bash "$SELF" --child pre "$W" "$1" 2>&1 ;;
    init-forged|noninit-forged) unshare --pid --fork --mount-proc --propagation private bash "$SELF" --child pre "$W" "$1" 2>&1 ;;
    *) bash "$SELF" --child pre "$W" "$1" 2>&1 ;;
  esac
}
o=$(prectx host-forged); grep -q '^RC=97$' <<<"$o" || fail "ATTACK: a forged OPKIT_HOST_NS on the host's own shell was accepted by the precondition: $o"
o=$(prectx host-bare); grep -q '^RC=97$' <<<"$o" || fail "ATTACK: the precondition accepted a process with no recorded host namespace: $o"
o=$(prectx standin-self); grep -q '^RC=97$' <<<"$o" || fail "ATTACK: the precondition accepted a process whose mount namespace IS the stand-in host's: $o"
o=$(prectx noninit-forged); grep -q '^RC=97$' <<<"$o" || fail "ATTACK: the precondition accepted a process that is not PID 1 of its PID namespace: $o"
o=$(prectx standin-host); grep -q '^RC=0$' <<<"$o" || fail "control: the precondition refused a throw-away mount namespace with a stand-in host (the self-test's own route): $o"
o=$(prectx init-forged); grep -q '^RC=0$' <<<"$o" || fail "control: the precondition refused PID 1 of a private PID namespace that recorded a different host mount namespace: $o"
echo "ok: the precondition refuses the host's own shell (bare or with a forged OPKIT_HOST_NS), the stand-in's own namespace and a non-init process; it passes the init of a private PID namespace that recorded another host namespace, and the self-test's stand-in route"
bare_try() { # PRIM [forged]
  local pr=$1 v=${2:-} o rc line
  o=$(unshare -m --propagation private bash "$SELF" --child bare "$W" "$pr" $v 2>&1); rc=$?
  line=$(grep '^BARE ' <<<"$o")
  [ -n "$line" ] || fail "setup: the bare-primitive probe for $pr $v did not report (rc $rc): $o"
  grep -q "^BARE rc=97 " <<<"$line" || fail "ATTACK: bare opkit_ns_$pr $v was not refused with 97: $line :: $o"
  grep -q "REFUSE(opkit_ns): opkit_ns_$pr: " <<<"$o" || fail "ATTACK: bare opkit_ns_$pr $v was refused, but not by the precondition: $o"
  grep -q ' mounts=same ' <<<"$line" || fail "ATTACK: bare opkit_ns_$pr $v changed the mount table before refusing: $line"
  grep -q ' scratch=\[dmp \] ' <<<"$line" || fail "ATTACK: bare opkit_ns_$pr $v created something in the scratch directory before refusing: $line"
  grep -q ' fd9=open$' <<<"$line" || fail "ATTACK: bare opkit_ns_$pr $v closed a descriptor before refusing: $line"
}
for pr in isolate make_ro fresh_proc private_dev drop_host_fd; do bare_try "$pr"; done
for pr in isolate make_ro fresh_proc private_dev drop_host_fd; do bare_try "$pr" forged; done
[ "$HM_BEFORE" = "$(hostmounts)" ] || fail "ATTACK: the host's mount table changed during the amendment-113 primitive tests"
echo "ok: each destructive primitive called with no proof (and with a forged OPKIT_HOST_NS) refuses 97 by the precondition, changes no mount, creates nothing in its scratch and closes no descriptor; the host's mount table is unchanged"
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
# ── amendment 105 ───────────────────────────────────────────────────────────────────────────────────
# The read-only root is a mount flag; what makes it a boundary is what the command CANNOT do. Every attack the
# round-11 reviewer executed is executed here, inside ns_run, with canaries only: nothing below writes a byte
# to a real place if a guard regresses (a marker line in the host's kernel log, a refused open of a node,
# a file under $W). KEEP=1 hands the step CAP_SYS_ADMIN (the one capability the fixture is allowed to keep).
nsrun() { # [VAR=val ...] -- CMD...   (ns_run with the standard scratch)
  OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run "$@"' bash "$@"
}
# (1) /dev: a private minimal tmpfs. There is no kmsg to write, no block device to open, nothing but the listed nodes.
MARK="OPKIT-NS-TEST-MARKER-$$-$RANDOM"
for keep in "" sys_admin; do
  o=$(OPKIT_CAPS_KEEP=$keep nsrun bash -c '
    echo "DEVLIST $(ls /dev | tr "\n" " ")"
    { echo "'"$MARK"'" >/dev/kmsg; } 2>/dev/null && echo KMSG-WRITTEN || echo kmsg-refused
    [ -e /dev/kmsg ] && echo KMSG-EXISTS
    ls /dev/sd* /dev/nvme* /dev/vd* /dev/loop* /dev/mem /dev/kmem /dev/port 2>/dev/null | sed "s/^/HOSTDEV /"
    echo done' 2>&1); rc=$?
  grep -q '^done$' <<<"$o" || fail "setup: the /dev probe did not run (keep='$keep', rc $rc): $o"
  ! grep -q KMSG-WRITTEN <<<"$o" || fail "ATTACK: the command wrote /dev/kmsg (keep='$keep'): $o"
  ! grep -q KMSG-EXISTS <<<"$o" || fail "ATTACK: /dev/kmsg exists inside ns_run (keep='$keep'): $o"
  ! grep -q '^HOSTDEV' <<<"$o" || fail "ATTACK: a host device node is visible inside ns_run (keep='$keep'): $o"
  for n in null zero full random urandom tty pts shm; do grep -q "^DEVLIST.* $n " <<<"$o" || fail "setup: /dev lacks $n (keep='$keep'): $o"; done
  ! (dmesg 2>/dev/null | grep -q "$MARK") || fail "ATTACK: the marker reached the host's kernel log (keep='$keep')"
done
echo "ok: /dev is private and minimal: no kmsg (the marker never reached the host's log), no block or memory device"
# a node made by mknod (the capability is gone, and the tmpfs is nodev): the host's root device cannot be opened
rootdev=$(stat -c '%d' / ); maj=$(( (rootdev >> 8) & 0xfff )); min=$(( rootdev & 0xff ))
for keep in "" sys_admin; do
  o=$(OPKIT_CAPS_KEEP=$keep nsrun python3 -c '
import os, sys
for d in ("/tmp", "/run", "/dev/shm", "/var/lib"):
    p = d + "/.opkit-node"
    try:
        os.mknod(p, 0o600 | 0o060000, os.makedev(int(sys.argv[1]), int(sys.argv[2])))
    except OSError as e:
        print("mknod-refused", d, e.errno); continue
    try:
        fd = os.open(p, os.O_RDONLY); print("NODE-OPENED", d); os.close(fd)
    except OSError as e:
        print("node-open-refused", d, e.errno)
print("done")' "$maj" "$min" 2>&1); rc=$?
  grep -q '^done$' <<<"$o" || fail "setup: the mknod probe did not run (keep='$keep'): $o"
  ! grep -q 'NODE-OPENED' <<<"$o" || fail "ATTACK: a block device node made inside ns_run could be opened (keep='$keep'): $o"
done
echo "ok: mknod of a block device is refused (no CAP_MKNOD), and a node could not be opened (nodev) -- opened read-only, never written"
# (2) /proc: a private one. `umount /proc` reveals nothing of the host.
o=$(nsrun bash -c 'umount -l /proc 2>&1; echo "UMOUNT-RC $?"; echo "NSPID $(readlink /proc/self/ns/pid)"; echo "INIT $(cat /proc/1/comm)"; echo "PIDS $(ls /proc | grep -c "^[0-9]")"; echo done' 2>&1)
grep -q '^done$' <<<"$o" || fail "setup: the umount probe did not run: $o"
! grep -q '^UMOUNT-RC 0$' <<<"$o" || fail "ATTACK: the command (default capabilities) could umount /proc: $o"
echo "ok: with the default capability set the command cannot umount /proc"
HOSTPIDS=$(ls /proc | grep -c '^[0-9]')
mkdir -p "$W/notrw"      # a SCRATCH directory the namespace holds read-only: the canary for "reached the host" (never a real place)
o=$(OPKIT_CAPS_KEEP=sys_admin nsrun env CAN="$W/notrw/.opkit-umount-canary" bash -c '
  umount -l /proc 2>/dev/null; echo "UMOUNTS $?"
  echo "PROCLIST [$(ls /proc 2>&1 | tr "\n" " ")]"
  [ -e /proc/1 ] && echo "PROC1-EXISTS $(readlink /proc/1/root) $(cat /proc/1/comm 2>/dev/null)"
  { : >"/proc/1/root$CAN"; } 2>/dev/null && echo CANARY-WRITTEN
  echo done' 2>&1)
grep -q '^done$' <<<"$o" || fail "setup: the umount-then-look probe did not run: $o"
grep -q '^UMOUNTS 0$' <<<"$o" || fail "setup: the umount did not run, so nothing was tested (CAP_SYS_ADMIN was kept): $o"
grep -q '^PROCLIST \[\]$' <<<"$o" || fail "ATTACK: after umount /proc something is visible under /proc (the host's proc?): $o"
! grep -q 'PROC1-EXISTS\|CANARY-WRITTEN' <<<"$o" || fail "ATTACK: after umount /proc the host's /proc (and /proc/1/root) was revealed: $o"
[ ! -e "$W/notrw/.opkit-umount-canary" ] || fail "ATTACK: umount /proc then /proc/1/root wrote the host (a read-only scratch directory)"
echo "ok: even with CAP_SYS_ADMIN, umount /proc reveals nothing of the host (the host's /proc was detached; there is no /proc/1/root)"
# the proof refuses a second mount at /proc (the covered mount a umount would reveal)
o=$(OPKIT_CAPS_KEEP=sys_admin OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"
  ns_run bash -c '"'"'. "$OPKIT_LIB"; mount -t proc -o ro proc /proc; opkit_ns_assert && echo ACCEPTED-COVERED-PROC; echo done'"'"'' 2>&1)
grep -q '^done$' <<<"$o" || fail "setup: the covered-proc probe did not run: $o"
! grep -q ACCEPTED-COVERED-PROC <<<"$o" || fail "ATTACK: the proof accepted a /proc with a covered mount beneath it: $o"
grep -q 'has 2 mounts' <<<"$o" || fail "refused for another reason: $o"
echo "ok: the proof refuses a /proc that covers another mount"
# (3) the root cannot be remounted writable, and nothing unlisted can be written, by a command with the default capabilities
o=$(nsrun env CAN="$W/notrw/.opkit-remount-canary" bash -c '
  mount -o remount,rw / 2>/dev/null && echo REMOUNTED-RW
  { : >"$CAN"; } 2>/dev/null && echo CANARY-WRITTEN
  mount --bind /tmp /mnt 2>/dev/null && echo BIND-MOUNTED
  unshare -m true 2>/dev/null && echo NEW-MOUNT-NAMESPACE
  echo done' 2>&1)
grep -q '^done$' <<<"$o" || fail "setup: the remount probe did not run: $o"
! grep -q 'REMOUNTED-RW\|CANARY-WRITTEN\|BIND-MOUNTED\|NEW-MOUNT-NAMESPACE' <<<"$o" || fail "ATTACK: a command with the default capabilities remounted or mounted: $o"
[ ! -e "$W/notrw/.opkit-remount-canary" ] || fail "ATTACK: remount rw / then a write reached the host (a read-only scratch directory)"
echo "ok: remount rw /, a bind mount and a new mount namespace are refused with the default capability set"
# /proc: the paths a root WITHOUT any capability can write (sysrq-trigger, sys) are read-only mounts; opened, never written
o=$(nsrun python3 -c '
import os
for p in ("/proc/sysrq-trigger", "/proc/sys/kernel/hostname", "/proc/sys/vm/drop_caches"):
    try:
        fd = os.open(p, os.O_WRONLY); print("OPENED-FOR-WRITE", p); os.close(fd)
    except OSError as e:
        print("refused", p, e.errno)
print("done")' 2>&1)
grep -q '^done$' <<<"$o" || fail "setup: the /proc probe did not run: $o"
! grep -q OPENED-FOR-WRITE <<<"$o" || fail "ATTACK: a /proc file that reaches the host could be opened for writing: $o"
echo "ok: /proc/sysrq-trigger and /proc/sys cannot be opened for writing"
# (4) the bounding set
for keep in "" sys_admin; do
  o=$(OPKIT_CAPS_KEEP=$keep nsrun bash -c 'capsh --decode=$(sed -n "s/^CapBnd:[[:space:]]*//p" /proc/self/status)' 2>&1)
  for c in sys_module sys_rawio syslog mknod dac_read_search net_admin net_raw sys_ptrace sys_boot sys_time; do
    ! grep -qi "cap_$c\b" <<<"$o" || fail "ATTACK: cap_$c is in the command's bounding set (keep='$keep'): $o"
  done
  if [ -n "$keep" ]; then grep -qi 'cap_sys_admin' <<<"$o" || fail "setup: OPKIT_CAPS_KEEP=sys_admin kept no CAP_SYS_ADMIN: $o"
  else ! grep -qi 'cap_sys_admin' <<<"$o" || fail "ATTACK: cap_sys_admin is in the default bounding set: $o"; fi
  grep -qi 'cap_chown' <<<"$o" && grep -qi 'cap_setuid' <<<"$o" || fail "setup: the kit's own capabilities (chown, setuid) were dropped (keep='$keep'): $o"
done
echo "ok: the bounding set has no sys_admin (unless handed), mknod, sys_rawio, sys_module, syslog, dac_read_search, net_admin, ptrace; chown/setuid stay"
M3=$W/scratch/ran3
o=$(OPKIT_CAPS_KEEP=mknod nsrun touch "$M3" 2>&1); rc=$?
{ [ ! -e "$M3" ] && [ $rc = 97 ]; } || fail "ATTACK: OPKIT_CAPS_KEEP=mknod was honoured (rc $rc): $o"
echo "ok: only sys_admin may be kept (anything else refuses, 97)"
# a setuid-root program does not get back what the bounding set took
o=$(nsrun bash -c 'cp /usr/bin/capsh /srv/capsh-suid && chmod u+s /srv/capsh-suid && setpriv --reuid=65534 --regid=65534 --clear-groups /srv/capsh-suid --print 2>&1 | grep -i "^Bounding"; echo done' 2>&1)
! grep -qi 'cap_sys_admin' <<<"$o" || fail "ATTACK: a setuid-root program regained cap_sys_admin: $o"
echo "ok: a setuid-root program inside the namespace gets no capability the bounding set took"
# (5) descriptors: fds 0-2, a pre-opened writable file, and every inherited descriptor above stderr
M4=$W/scratch/ran4; : >"$W/hostfile"; mkdir -p "$W/rw2"
o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M4" </ 2>&1); rc=$?
{ [ ! -e "$M4" ] && [ $rc = 97 ]; } || fail "ATTACK: stdin set to / (a host-root handle on fd 0) was accepted (rc $rc): $o"
grep -q 'descriptor 0 is a directory' <<<"$o" || fail "refused for another reason: $o"
o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M4" 2>&1 >/dev/null </dev/null); rc=$?
[ -e "$M4" ] && [ $rc = 0 ] || fail "control: stdin from /dev/null was refused (rc $rc): $o"
rm -f "$M4"
# a writable regular file on fd 1 outside the caller's scratch is refused; inside OPKIT_RW it is accepted
HOSTLOG=$W/notscratch.log
o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M4" >"$HOSTLOG" 2>&1 </dev/null); rc=$?
{ [ ! -e "$M4" ] && [ $rc = 97 ]; } || fail "ATTACK: a writable file outside the named scratch on fd 1 was accepted (rc $rc)"
[ ! -s "$HOSTLOG" ] || fail "ATTACK: the helper wrote its refusal into the writable file it had just refused (amendment 118): $(cat "$HOSTLOG")"
o=$(OPKIT_RW="$W/rw2" OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$W/rw2/ran4" >"$W/rw2/log" 2>&1 </dev/null); rc=$?
{ [ -e "$W/rw2/ran4" ] && [ $rc = 0 ]; } || fail "control: a log file under OPKIT_RW on fd 1 was refused (rc $rc): $(cat "$W/rw2/log")"
rm -f "$M4" "$W/rw2/ran4"
# ── amendment 118: the helper never writes its refusal through a descriptor it has just refused ───────────────────────────
# Incident 2026-10-09 01:14:46: fd 2 was the real /etc/passwd opened READ-WRITE without truncation; the helper refused (correct)
# and wrote its refusal text over the file's first lines. Replayed here on FILES THIS SCRIPT MADE under $W, never a host file.
# run118 SPECS SNIPPET [VAR=val ...]: SPECS = FD:MODE:FILE[,FD:MODE:FILE...]; each FILE is opened (MODE: rdwr = O_RDWR without
# truncation, append = O_WRONLY|O_APPEND, rdwr_append) and handed over as that fd of `bash -c '. LIB; SNIPPET'`; the other
# descriptors among 0-2 are a PIPE (stdin /dev/null), so a message written ANYWHERE else is seen; no controlling terminal (new
# session), so the tty fallback cannot print. Prints RC=<rc> then everything that came out of the pipe.
run118() {
  python3 - "$@" <<'PY'
import os, subprocess, sys
specs, snippet = sys.argv[1:3]
extra = dict(a.split("=", 1) for a in sys.argv[3:])
flags = {"rdwr": os.O_RDWR, "append": os.O_WRONLY | os.O_APPEND, "rdwr_append": os.O_RDWR | os.O_APPEND}
r, w = os.pipe()
std = {0: subprocess.DEVNULL, 1: w, 2: w}
for sp in specs.split(","):
    fd, mode, f = sp.split(":", 2)
    std[int(fd)] = os.open(f, flags[mode])
env = dict(os.environ, OPKIT_LIB=os.environ["LIB118"], **extra)
p = subprocess.Popen(["bash", "-c", '. "$OPKIT_LIB"; ' + snippet, "bash"], stdin=std[0], stdout=std[1], stderr=std[2], env=env, start_new_session=True)
os.close(w)
rc = p.wait()
data = b""
while True:
    b = os.read(r, 65536)
    if not b: break
    data += b
sys.stdout.write("RC=%d\n" % rc); sys.stdout.flush(); sys.stdout.buffer.write(data)
PY
}
H118=$W/h118; mkdir -p "$H118/fakebin" "$H118/fakebin2"; M118=$H118/ran
ORIG118=$'root:x:0:0:root:/root:/bin/bash\ndaemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin\nbin:x:2:2:bin:/bin:/usr/sbin/nologin\nsys:x:3:3:sys:/dev:/usr/sbin/nologin\nsync:x:4:65534:sync:/bin:/bin/sync\nrest-of-file\n'
printf '#!/bin/sh\ntouch "%s/unshare-called"\nexit 1\n' "$H118" >"$H118/fakebin/unshare"; chmod 0755 "$H118/fakebin/unshare"
printf '#!/bin/sh\necho TOOL-STDERR-118 >&2\nexec %s "$@"\n' "$(command -v realpath)" >"$H118/fakebin2/realpath"; chmod 0755 "$H118/fakebin2/realpath"
mkvictim() { printf '%s' "$ORIG118" >"$1"; }
for fdn in 0 1 2; do
  for mode in rdwr append rdwr_append; do
    [ "$fdn" = 0 ] && [ "$mode" != rdwr ] && continue
    F=$H118/victim-$fdn-$mode; mkvictim "$F"; rm -f "$M118"
    h0=$(sha256sum <"$F"); s0=$(stat -c '%s:%Y' "$F")
    o=$(LIB118=$LIB OPKIT_RW=$W/scratch OPKIT_SCRATCH=$W/scratch run118 "$fdn:$mode:$F" 'ns_run touch '"$M118")
    [ "$h0" = "$(sha256sum <"$F")" ] || fail "ATTACK: fd $fdn ($mode) a file ns_run refused was WRITTEN (sha256 changed): $(head -c 300 "$F")"
    [ "$s0" = "$(stat -c '%s:%Y' "$F")" ] || fail "ATTACK: fd $fdn ($mode) the refused file's size or mtime changed"
    head -1 <<<"$o" | grep -qx 'RC=97' || fail "ATTACK: fd $fdn ($mode) a writable regular file was not refused with 97: $o"
    [ ! -e "$M118" ] || fail "ATTACK: the command ran with a writable regular file on fd $fdn ($mode)"
    case "$fdn" in
      2) [ "$(wc -c <<<"$o")" -le 7 ] || fail "ATTACK: fd 2 was the refused file, and the helper still said something on the other descriptor: $o" ;;
      *) grep -q "descriptor $fdn is a WRITABLE regular file" <<<"$o" || fail "refused for another reason (fd $fdn $mode): $o" ;;
    esac
  done
done
echo "ok: a writable regular file on fd 0, 1 or 2 (O_RDWR without truncation, O_APPEND, both) is refused with 97 and its bytes (sha256), size and mtime are UNCHANGED; on fd 2 nothing at all is said, on fd 0/1 the reason goes to the vetted fd 2"
# a file whose path merely BEGINS with the scratch path (scratch-118x next to scratch) is not under the scratch
F=$W/scratch-118x; mkvictim "$F"; h0=$(sha256sum <"$F")
o=$(LIB118=$LIB OPKIT_RW=$W/scratch OPKIT_SCRATCH=$W/scratch run118 "2:rdwr:$F" 'ns_run touch '"$M118")
head -1 <<<"$o" | grep -qx 'RC=97' || fail "ATTACK: a file whose path merely begins with the scratch path was accepted as scratch: $o"
[ "$h0" = "$(sha256sum <"$F")" ] || fail "ATTACK: a file whose path merely begins with the scratch path was written"
# the fds are classified BEFORE ns_run unshares (a fake unshare records that it was reached): all three at once, then each alone
for f in 0 1 2; do mkvictim "$H118/u$f"; done
rm -f "$H118/unshare-called"
o=$(LIB118=$LIB OPKIT_RW=$W/scratch OPKIT_SCRATCH=$W/scratch run118 "0:rdwr:$H118/u0,1:rdwr:$H118/u1,2:rdwr:$H118/u2" 'ns_run touch '"$M118" PATH="$H118/fakebin:$PATH")
[ ! -e "$H118/unshare-called" ] || fail "ATTACK: ns_run reached unshare with writable regular files on fds 0, 1 and 2 at once"
head -1 <<<"$o" | grep -qx 'RC=97' || fail "ATTACK: three writable regular files on fds 0-2 were not refused with 97: $o"
for fdn in 0 1 2; do
  rm -f "$H118/unshare-called"
  o=$(LIB118=$LIB OPKIT_RW=$W/scratch OPKIT_SCRATCH=$W/scratch run118 "$fdn:rdwr:$H118/u$fdn" 'ns_run touch '"$M118" PATH="$H118/fakebin:$PATH")
  [ ! -e "$H118/unshare-called" ] || fail "ATTACK: ns_run reached unshare with a writable regular file on fd $fdn"
  head -1 <<<"$o" | grep -qx 'RC=97' || fail "ATTACK: a writable regular file on fd $fdn alone was not refused with 97: $o"
done
# control: with only safe descriptors the fake unshare IS reached (the test would otherwise pass for any reason)
rm -f "$H118/unshare-called"; : >"$W/scratch/log118c"
o=$(LIB118=$LIB OPKIT_RW=$W/scratch OPKIT_SCRATCH=$W/scratch run118 "1:rdwr:$W/scratch/log118c" 'ns_run touch '"$M118" PATH="$H118/fakebin:$PATH")
rm -f "$W/scratch/log118c"
[ -e "$H118/unshare-called" ] || fail "control: ns_run never reached unshare with a log file under OPKIT_RW on fd 1 (the unshare-ordering probe is blind)"
echo "ok: ns_run classifies fds 0, 1 and 2 before it unshares or creates anything (a fake unshare is not reached); with a log under OPKIT_RW it is"
# a TOOL the pre-checks run writes its own stderr (realpath here): captured, never through a writable file on fd 2
mkvictim "$H118/victim-tool"; h0=$(sha256sum <"$H118/victim-tool")
o=$(LIB118=$LIB OPKIT_RW=$W/scratch OPKIT_SCRATCH=$W/scratch run118 "2:rdwr:$H118/victim-tool" 'ns_run touch '"$M118" PATH="$H118/fakebin2:$PATH")
[ "$h0" = "$(sha256sum <"$H118/victim-tool")" ] || fail "ATTACK: a tool the pre-checks ran wrote its stderr through the writable regular file on fd 2: $(head -c 200 "$H118/victim-tool")"
grep -qx 'RC=97' <<<"$(head -1 <<<"$o")" || fail "ATTACK: the tool-stderr probe was not refused with 97: $o"
# the pre-namespace refusals (invalid OPKIT_RW, invalid OPKIT_SCRATCH, not-a-directory) say why only after fds 0-2 pass, and a root
# the helper refused counts for nothing: OPKIT_RW=/var/tmp is refused, so a victim under /var/tmp is not "the caller's scratch"
for bad in OPKIT_RW=/etc OPKIT_RW=/nonexistent-118 OPKIT_SCRATCH=/etc OPKIT_SCRATCH=relative OPKIT_RW=/var/tmp OPKIT_SCRATCH=/var/tmp; do
  F=$H118/victim-pre; mkvictim "$F"; h0=$(sha256sum <"$F")
  o=$(LIB118=$LIB run118 "2:rdwr:$F" 'ns_run touch '"$M118" "$bad"); rc=$(head -1 <<<"$o")
  [ "$rc" = RC=97 ] || fail "ATTACK: $bad with a writable file on fd 2 was not refused with 97: $o"
  [ "$h0" = "$(sha256sum <"$F")" ] || fail "ATTACK: the refusal for $bad was written into the file on fd 2: $(head -c 300 "$F")"
  [ "$(wc -c <<<"$o")" -le 7 ] || fail "ATTACK: output for $bad on the other descriptor: $o"
done
# the library functions called directly (the kit and the tests call them), refusing, with fd 2 a writable file
for snip in 'opkit_scratch_check X /nonexistent-118' 'opkit_rw_validate /etc' 'opkit_ns_fd_ok 2' 'opkit_ns_std_fds_ok' 'opkit_say hello'; do   # (pure ones only: nothing here mounts or creates)
  F=$H118/victim-lib; mkvictim "$F"; h0=$(sha256sum <"$F")
  o=$(LIB118=$LIB run118 "2:rdwr:$F" "$snip")
  [ "$h0" = "$(sha256sum <"$F")" ] || fail "ATTACK: '$snip' wrote through a writable regular file on fd 2: $(head -c 300 "$F")"
  [ "$(wc -c <<<"$o")" -le 7 ] || fail "ATTACK: '$snip' wrote to the other descriptor: $o"
done
# control: the same calls with a pipe on fd 2 DO say why (the message is not lost for a legitimate caller)
o=$(OPKIT_LIB=$LIB bash -c '. "$OPKIT_LIB"; opkit_scratch_check X /nonexistent-118' 2>&1); rc=$?
{ [ $rc = 1 ] && grep -q "is not a directory" <<<"$o"; } || fail "control: the refusal was not said on a pipe (rc $rc): $o"
# control: a log under OPKIT_RW on fd 2 is an accepted log: the run goes ahead, and a refusal for ANOTHER reason (a directory on fd 0) reaches it
F=$W/scratch/log118; : >"$F"
o=$(LIB118=$LIB OPKIT_RW=$W/scratch OPKIT_SCRATCH=$W/scratch run118 "2:rdwr:$F" 'ns_run touch '"$W/scratch/ran118")
{ head -1 <<<"$o" | grep -qx 'RC=0' && [ -e "$W/scratch/ran118" ]; } || fail "control: a log file under OPKIT_RW on fd 2 was refused: $o"
rm -f "$W/scratch/ran118"
o=$(cd / && LIB118=$LIB OPKIT_RW=$W/scratch OPKIT_SCRATCH=$W/scratch bash -c '. "$LIB118"; ns_run touch "$1"' bash "$W/scratch/ran118" </ 2>>"$F"); rc=$?
{ [ $rc = 97 ] && grep -q 'descriptor 0 is a directory' "$F"; } || fail "control: the refusal was not delivered to a legitimate log under OPKIT_RW (rc $rc): $(cat "$F")"
echo "ok: the pre-namespace refusals and the library functions called directly never write through a writable regular file on fd 2; a pipe still gets the reason"
# a block device, or a character device that is not a null/zero/tty/random one, on stdin (opened read-only, never read)
BLK=""; for cand in "$(findmnt -no SOURCE / 2>/dev/null)" /dev/sda /dev/sdb /dev/vda /dev/nvme0n1 /dev/loop0; do [ -b "$cand" ] && { BLK=$cand; break; }; done
if [ -n "$BLK" ]; then
  o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M4" <"$BLK" 2>&1); rc=$?
  { [ ! -e "$M4" ] && [ $rc = 97 ]; } || fail "ATTACK: a block device on stdin was accepted (rc $rc): $o"
  grep -q 'block device' <<<"$o" || fail "refused for another reason: $o"
else echo "SKIP: no block device to hand over on stdin"; fi
if [ -c /dev/kmsg ]; then
  o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M4" </dev/kmsg 2>&1); rc=$?
  { [ ! -e "$M4" ] && [ $rc = 97 ]; } || fail "ATTACK: /dev/kmsg (a character device nobody listed) on stdin was accepted (rc $rc): $o"
  grep -q 'character device' <<<"$o" || fail "refused for another reason: $o"
else echo "SKIP: no /dev/kmsg to hand over on stdin"; fi
# a pre-opened writable regular file above stderr is closed: the command's write fails, the file stays empty
o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch HF=$W/hostfile bash -c '. "$OPKIT_LIB"; exec 7>>"$HF"; ns_run bash -c '"'"'{ echo leaked >&7; } 2>/dev/null && echo WROTE-THROUGH-FD7; ls /proc/self/fd | tr "\n" " "; echo; echo done'"'"'' 2>&1 </dev/null)
grep -q '^done$' <<<"$o" || fail "setup: the pre-opened file probe did not run: $o"
! grep -q WROTE-THROUGH-FD7 <<<"$o" || fail "ATTACK: a pre-opened writable file was still writable inside ns_run: $o"
[ ! -s "$W/hostfile" ] || fail "ATTACK: the host file was written through an inherited descriptor"
echo "ok: fd 0-2 may not be a directory or a writable file outside the named scratch; an inherited writable file above stderr is closed"
# (6) OPKIT_RW is validated
mkdir -p "$W/sub"; M5=$W/sub/ran5
rwref() { # DIR... -> the command did not run, rc 97
  o=$(OPKIT_RW="$*" OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M5" 2>&1 </dev/null); rc=$?
  { [ ! -e "$M5" ] && [ $rc = 97 ]; } || { rm -f "$M5"; fail "ATTACK: OPKIT_RW='$*' was accepted (rc $rc): $o"; }
}
mkdir -p "$W/open777" "$W/sub" "$W/owned" "$W/real"; chmod 0777 "$W/open777"; chown 65534 "$W/owned"; ln -s /opt "$W/optlink"; ln -s "$W/real" "$W/reallink"
for bad in / /opt /home /root /usr /etc /var /var/tmp /tmp /boot /dev /var/lib relative/dir "$W/open777" "$W/optlink" "$W/reallink" "$W/owned" "$W/does-not-exist" "$W/rw $W/../" ; do rwref $bad; done
# each guard on its own: a directory only ONE of them refuses
for cand in /mnt /media; do [ -d "$cand" ] && [ ! -L "$cand" ] && [ "$(stat -c %u "$cand")" = "$(id -u)" ] && [ $(( 8#$(stat -c %a "$cand") & 8#022 )) = 0 ] && { MNT=$cand; break; }; done
if [ -n "${MNT:-}" ]; then
  o=$(OPKIT_RW="$MNT" OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M5" 2>&1 </dev/null); rc=$?
  { [ ! -e "$M5" ] && [ $rc = 97 ]; } || fail "ATTACK: OPKIT_RW=$MNT (a host directory outside every temp root) was accepted (rc $rc): $o"
  grep -q 'not strictly below /var/tmp:' <<<"$o" || fail "ATTACK: OPKIT_RW=$MNT was refused, but not by the /var/tmp-only temp-root rule (its root list is wrong, or another rule refused it): $o"
else echo "SKIP: no /mnt or /media to try as a directory outside every temp root"; fi
for cand in /usr/share/zoneinfo /usr/share/doc /usr/lib/systemd /usr/share/misc; do [ -d "$cand" ] && [ ! -L "$cand" ] && { SYS=$cand; break; }; done
if [ -n "${SYS:-}" ]; then
  o=$(OPKIT_RW_ROOTS_FOR_TEST=$(dirname "$SYS") OPKIT_RW="$SYS" OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash "$SELF" --child nsrun "$W" touch "$M5" 2>&1 </dev/null); rc=$?
  { [ ! -e "$M5" ] && [ $rc = 97 ]; } || fail "ATTACK: OPKIT_RW=$SYS (a system path, below a temp root by the test's own override) was accepted (rc $rc): $o"
  grep -q 'is a system path' <<<"$o" || fail "refused for another reason (the system-path guard alone was to refuse it): $o"
  o=$(OPKIT_RW_ROOTS_FOR_TEST=/nonexistent OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash "$SELF" --child nsrun "$W" true 2>&1 </dev/null); rc=$?
  [ $rc = 0 ] || fail "control: an extra temp root with nothing under it refused the run (rc $rc): $o"
else echo "SKIP: no system directory to try"; fi
# OPKIT_RW is strictly below /var/tmp: an entry under /tmp validated and then failed to bind (/tmp is a fresh tmpfs by then)
TMPRW=$(mktemp -d /tmp/opkit-rw113.XXXXXX) || fail "setup: no directory under /tmp"
M7=$W/scratch/ran7
o=$(OPKIT_RW=$TMPRW OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M7" 2>&1 </dev/null); rc=$?
{ [ ! -e "$M7" ] && [ $rc = 97 ]; } || fail "ATTACK: OPKIT_RW under /tmp was accepted (rc $rc): $o"
grep -q 'not strictly below /var/tmp:' <<<"$o" || fail "ATTACK: OPKIT_RW under /tmp was validated and failed later instead of being refused by its own rule: $o"
! grep -q 'isolation not proved\|cannot make' <<<"$o" || fail "ATTACK: OPKIT_RW under /tmp got past the validation and failed inside the namespace: $o"
o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$TMPRW bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M7" 2>&1 </dev/null); rc=$?
{ [ -e "$M7" ] && [ $rc = 0 ]; } || fail "control: a scratch root under /tmp (OPKIT_SCRATCH) with OPKIT_RW under /var/tmp was refused (rc $rc): $o"
rm -f "$M7"; rm -rf "$TMPRW"
echo "ok: OPKIT_RW under /tmp is refused by its own rule (97, before anything is mounted); a scratch root under /tmp with OPKIT_RW under /var/tmp runs"
# the override is the self-test's alone
o=$(OPKIT_RW_ROOTS_FOR_TEST=/usr/share OPKIT_RW="/usr/share/doc" OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run true' bash 2>&1 </dev/null); rc=$?
[ $rc = 97 ] || fail "ATTACK: an OPKIT_RW_ROOTS_FOR_TEST override from another script was honoured (rc $rc): $o"
echo "ok: OPKIT_RW refuses /, /opt, /home, /root, /usr, /etc, /var, a shared temp root, a relative path, a symlink, a group/other-writable directory, a missing one"
o=$(OPKIT_RW="$W/rw $W/sub" OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M5" 2>&1 </dev/null); rc=$?
{ [ -e "$M5" ] && [ $rc = 0 ]; } || fail "control: scratch directories under a temp root were refused (rc $rc): $o"
echo "ok: control: OPKIT_RW entries that are scratch below a temp root are accepted"

# ── amendment 109 ───────────────────────────────────────────────────────────────────────────────────
# (1) the temp roots are the helper's own: TMPDIR and OPKIT_SCRATCH are the environment's, and the environment does not
# get to name a place the helper mounts over, creates directories in or hands the command as writable. The host
# directory outside every temp root is /mnt (or /media) under a tmpfs of this throw-away namespace, so nothing real is written.
hostlist() { ls -A /opt /home /mnt /media /srv /usr/local /etc/axon /etc/systemd/system 2>&1; }
HL_BEFORE=$(hostlist)
MNTD=""; for cand in /mnt /media; do [ -d "$cand" ] && [ ! -L "$cand" ] && { MNTD=$cand; break; }; done
if [ -n "$MNTD" ]; then
  o=$(STANDIN=$$ unshare -m --propagation private bash "$SELF" --child mnttmpfs "$W" "$MNTD" 2>&1); rc=$?
  [ $rc = 0 ] || fail "(rc $rc) $o"
  echo "ok: TMPDIR is ignored and is no temp root; OPKIT_SCRATCH outside /tmp and /var/tmp is refused (97) before anything is mounted, nothing is created there; the helper's own scratch is removed"
else echo "SKIP: no /mnt or /media to stand in for a host directory outside every temp root"; fi
[ "$HL_BEFORE" = "$(hostlist)" ] || fail "ATTACK: the host listing changed during the TMPDIR/OPKIT_SCRATCH tests"
# (2) descriptors 0-2: a socket, the console, the virtual terminals and serial ports are refused; a pipe and a pts are not
M6=$W/scratch/ran6
for fdn in 0 1 2; do
  rm -f "$M6"
  o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash "$SELF" --child sockfd "$W" $fdn "$M6" 2>&1 </dev/null); rc=$?
  { [ ! -e "$M6" ] && [ $rc = 97 ]; } || fail "ATTACK: a unix socket on fd $fdn was accepted (rc $rc): $o"
  [ $fdn = 2 ] || grep -q 'is a socket' <<<"$o" || fail "refused for another reason (rc $rc): $o"     # (on fd 2 the message went to the socket)
done
rm -f "$M6"
o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; printf x | ns_run touch "$1"' bash "$M6" 2>&1); rc=$?
{ [ -e "$M6" ] && [ $rc = 0 ]; } || fail "control: a pipe on stdin was refused (rc $rc): $o"
rm -f "$M6"
o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash "$SELF" --child ptyfd "$W" "$M6" 2>&1); rc=$?
{ [ -e "$M6" ] && [ $rc = 0 ]; } || fail "control: a pts slave on stdin was refused (rc $rc): $o"
echo "ok: a unix socket on fd 0-2 is refused (97); a pipe and a pts slave are accepted"
mkdir -p "$W/devs"; tested=0
for spec in "console 5 1" "tty0 4 0" "tty1 4 1" "ttyS0 4 64"; do
  set -- $spec
  bash "$SELF" --child mknodc "$W" "$1" "$2" "$3" || continue
  ( exec 8<"$W/devs/$1" ) 2>/dev/null || { echo "SKIP: $1 ($2:$3) cannot be opened here"; continue; }
  rm -f "$M6"
  o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M6" <"$W/devs/$1" 2>&1); rc=$?
  { [ ! -e "$M6" ] && [ $rc = 97 ]; } || fail "ATTACK: the host console/terminal device $1 ($2:$3) on stdin was accepted (rc $rc): $o"
  grep -q 'character device' <<<"$o" || fail "refused for another reason (rc $rc): $o"
  tested=$((tested + 1))
done
[ $tested -gt 0 ] || echo "SKIP: no console or terminal device could be opened to hand over on stdin"
for spec in "null 1 3" "zero 1 5"; do   # controls: the nodes the allowlist keeps
  set -- $spec; bash "$SELF" --child mknodc "$W" "c$1" "$2" "$3" || continue
  rm -f "$M6"
  o=$(OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M6" <"$W/devs/c$1" 2>&1); rc=$?
  { [ -e "$M6" ] && [ $rc = 0 ]; } || fail "control: $1 on stdin was refused (rc $rc): $o"
done
rm -f "$M6"
echo "ok: the console, the virtual terminals and the serial ports on fd 0 are refused (97); null and zero are accepted"
# (3) an ordinary uid cannot make the namespaces: 97 and the command never ran (it used to be unshare's own rc 1); a host
# where unshare fails does the same
LIBTXT=$(cat "$LIB")
rm -f "$M6"
o=$(LIBTXT=$LIBTXT setpriv --reuid 65534 --regid 65534 --clear-groups bash -c 'eval "$LIBTXT"; ns_run touch "$1"' bash "$W/ordinary-uid-ran" 2>&1); rc=$?
{ [ ! -e "$W/ordinary-uid-ran" ] && [ $rc = 97 ]; } || fail "ATTACK: ns_run as an ordinary uid did not refuse with 97 (rc $rc): $o"
mkdir -p "$W/fakebin"; printf '#!/bin/sh\nexit 1\n' >"$W/fakebin/unshare"; chmod 0755 "$W/fakebin/unshare"
o=$(PATH=$W/fakebin:$PATH OPKIT_RW=$W/scratch OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M6" 2>&1 </dev/null); rc=$?
{ [ ! -e "$M6" ] && [ $rc = 97 ]; } || fail "ATTACK: a host that cannot unshare did not refuse with 97 (rc $rc, marker $([ -e "$M6" ] && echo present || echo absent)): $o"
echo "ok: an ordinary uid, or a host where unshare fails, is refused with 97 and the command never runs"
# (4) nodev, read from the mount flags inside ns_run: every tmpfs the helper makes and the OPKIT_RW bind carry it
mkdir -p "$W/rw110"
o=$(OPKIT_RW=$W/rw110 OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run awk -v rw="$1" '"'"'{ n = split($6, o, ","); nd = 0; for (i = 1; i <= n; i++) if (o[i] == "nodev") nd = 1
      if ($5 == "/tmp" || $5 == "/etc" || $5 == "/usr/local" || $5 == "/var/lib" || $5 == "/run" || $5 == "/srv" || $5 == "/var/log" || $5 == "/var/spool" || $5 == rw) print (nd ? "nodev" : "NODEV-MISSING"), $5 }'"'"' /proc/self/mountinfo' bash "$W/rw110" 2>&1 </dev/null); rc=$?
[ $rc = 0 ] && grep -q '^nodev /tmp$' <<<"$o" && grep -q "^\(nodev\|NODEV-MISSING\) $W/rw110\$" <<<"$o" || fail "setup: the mount-flag listing did not run (rc $rc): $o"
! grep -q NODEV-MISSING <<<"$o" || fail "ATTACK: a tmpfs or bind the helper made lacks nodev: $(grep NODEV-MISSING <<<"$o" | tr '\n' ' ')"
echo "ok: every tmpfs the helper makes (/tmp /etc /usr/local /var/lib /run /srv ...) and the OPKIT_RW bind carry nodev"
# (amendment 111) The capabilities the bounding set removed cannot be got back from INSIDE, by the routes that need no escape
# syscall. Executed here, as the default-capability root inside ns_run, with effects on a scratch file only: reading the sets,
# asking for the removed capabilities back (setpriv --inh-caps / --ambient-caps / --bounding-set, prctl), a setuid-root copy of
# a program and a file-capability copy of one. NOT executed by anyone, here or elsewhere: setns, chroot, a nested user
# namespace, open_by_handle_at, mounting the setuid binary, bpf, init_module, reboot (the operator has not authorised them).
CAPPROBE='
import os, sys, ctypes
names = {2: "dac_read_search", 12: "net_admin", 13: "net_raw", 16: "sys_module", 17: "sys_rawio", 19: "sys_ptrace", 21: "sys_admin",
         22: "sys_boot", 25: "sys_time", 27: "mknod", 34: "syslog", 39: "bpf"}
sets = {}
for ln in open("/proc/self/status"):
    for k in ("CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"):
        if ln.startswith(k + ":"):
            sets[k] = int(ln.split()[1], 16)
for k, v in sorted(sets.items()):
    print("SET", k, "%x" % v)
libc = ctypes.CDLL(None, use_errno=True)
for c, n in sorted(names.items()):
    print("BND", n, libc.prctl(23, c, 0, 0, 0))                  # PR_CAPBSET_READ: 1 in the bounding set, 0 not
    r = libc.prctl(47, 2, c, 0, 0)                                # PR_CAP_AMBIENT_RAISE: needs the capability permitted AND inheritable
    print("AMB", n, r, ctypes.get_errno())
print("done")
'
o=$(nsrun python3 -c "$CAPPROBE" 2>&1)
grep -q '^done$' <<<"$o" || fail "setup: the capability probe did not run: $o"
BND=$(sed -n 's/^SET CapBnd //p' <<<"$o"); EFF=$(sed -n 's/^SET CapEff //p' <<<"$o"); PRM=$(sed -n 's/^SET CapPrm //p' <<<"$o")
[ -n "$BND" ] && [ -n "$EFF" ] && [ -n "$PRM" ] || fail "setup: the capability sets were not read: $o"
[ $(( 0x$EFF & ~0x$BND )) = 0 ] && [ $(( 0x$PRM & ~0x$BND )) = 0 ] || fail "ATTACK: the process holds an effective or permitted capability that is not in its bounding set (eff $EFF prm $PRM bnd $BND)"
for n in dac_read_search net_admin net_raw sys_module sys_rawio sys_ptrace sys_admin sys_boot sys_time mknod syslog bpf; do
  grep -q "^BND $n 0\$" <<<"$o" || fail "ATTACK: cap_$n is in the command's bounding set: $(grep "^BND $n " <<<"$o")"
  grep -q "^AMB $n -1 " <<<"$o" || fail "ATTACK: cap_$n could be raised to the ambient set: $(grep "^AMB $n " <<<"$o")"
done
echo "ok: CapEff and CapPrm are within the bounding set, and every removed capability reads 0 in it and cannot be raised ambient (prctl)"
# setpriv asking for a removed capability back: each request must FAIL (rc != 0) and the command must not run
for req in "--inh-caps +sys_admin" "--ambient-caps +sys_admin" "--inh-caps +net_admin --ambient-caps +net_admin" "--bounding-set +sys_admin" "--bounding-set +mknod,+dac_read_search"; do
  rm -f "$W/scratch/cap-ran"
  o=$(nsrun setpriv $req -- touch "$W/scratch/cap-ran" 2>&1); rc=$?
  { [ ! -e "$W/scratch/cap-ran" ] && [ $rc != 0 ]; } || fail "ATTACK: setpriv $req was honoured inside the namespace (rc $rc): $o"
done
echo "ok: setpriv --inh-caps / --ambient-caps / --bounding-set for sys_admin, net_admin, mknod, dac_read_search is refused inside the namespace"
# a setuid-root copy and a file-capability copy of a program: what each one holds is still inside the bounding set
# (/tmp is nosuid, so a setuid copy there proves nothing: the probe lives in /srv, a shadow tmpfs that honours setuid, and a CONTROL
# shows the setuid bit takes effect there -- a setuid-root `id -u` run as uid 65534 prints 0 -- and that the program holds exactly the set)
# (the probe is a file the test writes and the namespace runs: the drift gate judges what is written under /srv by a command that
# is not itself ns_run, and a quoted program is judged line by line)
cat >"$W/suidprobe.sh" <<'EOF'
G=$(command -v grep); I=$(command -v id)
cp "$I" /srv/id-suid && chmod u+s /srv/id-suid
echo "SUIDEUID $(setpriv --reuid=65534 --regid=65534 --clear-groups /srv/id-suid -u)"
cp "$G" /srv/grep-suid && chmod u+s /srv/grep-suid
setpriv --reuid=65534 --regid=65534 --clear-groups /srv/grep-suid -E "^Cap(Eff|Prm|Bnd)" /proc/self/status | sed "s/^/SUID /"
cp "$G" /srv/grep-fcap && setcap cap_sys_admin,cap_mknod,cap_dac_read_search+ep /srv/grep-fcap 2>&1 | sed "s/^/SETCAP /"
setpriv --reuid=65534 --regid=65534 --clear-groups /srv/grep-fcap -E "^Cap(Eff|Prm|Bnd)" /proc/self/status 2>&1 | sed "s/^/FCAP /"
echo done
EOF
o=$(nsrun bash "$W/suidprobe.sh" 2>&1)
grep -q '^done$' <<<"$o" || fail "setup: the setuid / file-capability probe did not run: $o"
grep -q '^SUIDEUID 0$' <<<"$o" || fail "setup: the setuid bit did not take effect in /srv, so the probe would prove nothing: $o"
grep -q '^SUID CapEff' <<<"$o" || fail "setup: the setuid-root copy printed no capability set: $o"
EFFSUID=$(sed -n 's/^SUID CapEff:[[:space:]]*//p' <<<"$o"); BNDSUID=$(sed -n 's/^SUID CapBnd:[[:space:]]*//p' <<<"$o")
python3 - "$EFFSUID" "$BNDSUID" <<'PY' || fail "ATTACK/setup: the setuid-root copy's CapEff $EFFSUID is empty or exceeds the bounding set $BNDSUID"
import sys
e, b = int(sys.argv[1], 16), int(sys.argv[2], 16)
sys.exit(0 if e and not e & ~b else 1)
PY
python3 - "$o" <<'PY' || fail "ATTACK: a setuid-root or file-capability copy of a program gained a capability the bounding set removed"
import sys
removed = (1 << 21) | (1 << 27) | (1 << 2) | (1 << 12) | (1 << 17) | (1 << 16) | (1 << 34)
for ln in sys.argv[1].splitlines():
    p = ln.split()
    if len(p) == 3 and p[0] in ("SUID", "FCAP") and p[1].rstrip(":") in ("CapEff", "CapPrm", "CapBnd"):
        if int(p[2], 16) & removed:
            print("held:", ln); sys.exit(1)
PY
echo "ok: a setuid-root copy and a file-capability copy of a program hold none of sys_admin, mknod, dac_read_search, net_admin, sys_rawio, sys_module, syslog"
# the one kept capability: only sys_admin may be handed over; the benign check is that it is PRESENT and that net_admin is REFUSED
o=$(OPKIT_CAPS_KEEP=sys_admin nsrun python3 -c "$CAPPROBE" 2>&1)
grep -q '^BND sys_admin 1$' <<<"$o" || fail "setup: OPKIT_CAPS_KEEP=sys_admin did not leave CAP_SYS_ADMIN in the bounding set: $o"
grep -q '^BND net_admin 0$' <<<"$o" || fail "ATTACK: OPKIT_CAPS_KEEP=sys_admin also left CAP_NET_ADMIN: $o"
for k in net_admin "sys_admin,net_admin" "sys_admin net_admin" sys_ptrace all; do
  rm -f "$W/scratch/cap-ran"
  o=$(OPKIT_CAPS_KEEP=$k nsrun touch "$W/scratch/cap-ran" 2>&1); rc=$?
  { [ ! -e "$W/scratch/cap-ran" ] && [ $rc = 97 ]; } || fail "ATTACK: OPKIT_CAPS_KEEP='$k' was honoured (rc $rc): $o"
done
echo "ok: OPKIT_CAPS_KEEP=sys_admin leaves exactly that one capability in the bounding set (nothing was done with it); net_admin and the other spellings are refused (97)"
[ "$HL_BEFORE" = "$(hostlist)" ] || fail "ATTACK: the host listing changed during the amendment-109 tests"
echo "ok: the host listing (/opt /home /mnt /media /srv /usr/local /etc/axon /etc/systemd/system) is the same before and after"
echo "PASS: opkit namespace helper"
