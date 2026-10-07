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
  mount -t tmpfs tmpfs "$W/a" || exit 90
  [ "${TMPFS_B:-1}" = 1 ] && { mount -t tmpfs tmpfs "$W/b" || exit 90; }
  export OPKIT_DESTS_FOR_TEST="$W/a $W/b" OPKIT_VIEW_PID_FOR_TEST=${VIEW:?} OPKIT_NS_PID_FOR_TEST=${NSPID:?}
  [ "$VIEW" = self ] && export OPKIT_VIEW_PID_FOR_TEST=$$
  [ "$NSPID" = self ] && export OPKIT_NS_PID_FOR_TEST=$$
  . "$LIB"
  case "$mode" in assert) opkit_ns_assert ;; *) exit 91 ;; esac
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
{ [ $rc = 1 ] && grep -q 'is not shadowed by a tmpfs' <<<"$o"; } \
  || fail "ATTACK: a destination that is not a tmpfs was accepted (rc $rc): $o"
echo "ok: a destination that is not a tmpfs is refused"

o=$(assert_in_ns 1 self); rc=$?       # "the host namespace" is our own: the same one
{ [ $rc = 1 ] && grep -q "host's mount namespace" <<<"$o"; } \
  || fail "ATTACK: running in the host's own mount namespace was accepted (rc $rc): $o"
echo "ok: running in the host's mount namespace is refused"

# the canary is visible from the "host" view (here: this very namespace's view of itself)
o=$(assert_in_ns self 1); rc=$?
{ [ $rc = 1 ] && grep -q 'is VISIBLE to the host' <<<"$o"; } \
  || fail "ATTACK: a canary visible from the host's view was accepted (rc $rc): $o"
echo "ok: a canary that shows through to the host is refused"

# amendment 97 (d): an UNREADABLE host view proves nothing, so it refuses (it used to pass vacuously)
o=$(assert_in_ns 999999999 1); rc=$?
{ [ $rc = 1 ] && grep -q 'cannot be read' <<<"$o"; } \
  || fail "ATTACK: an unreadable host view was taken as 'not visible' (rc $rc): $o"
echo "ok: an unreadable host view refuses (it does not pass vacuously)"

# amendment 97 (e): the FOR_TEST overrides are honoured for this script ONLY. Any other caller that
# sets one is refused, never silently weakened.
cp "$LIB" "$W/other-lib.sh"
cat >"$W/other.sh" <<OTHER
. "$LIB"
opkit_ns_assert
OTHER
o=$(OPKIT_VIEW_PID_FOR_TEST=1 OPKIT_NS_PID_FOR_TEST=1 OPKIT_DESTS_FOR_TEST="$W/a" bash "$W/other.sh" 2>&1); rc=$?
{ [ $rc = 1 ] && grep -q 'outside scripts/test_opkit_ns.sh' <<<"$o"; } \
  || fail "ATTACK: an OPKIT_*_FOR_TEST override from another script was honoured (rc $rc): $o"
o=$(OPKIT_VIEW_PID_FOR_TEST=1 bash -c ". '$LIB'; opkit_ns_assert" 2>&1); rc=$?
{ [ $rc = 1 ] && grep -q 'outside scripts/test_opkit_ns.sh' <<<"$o"; } \
  || fail "ATTACK: an OPKIT_*_FOR_TEST override from a bash -c caller was honoured (rc $rc): $o"
echo "ok: the *_FOR_TEST overrides are refused for any caller but this script"

# ns_run never starts its command when the proof fails (here: a file it must install is missing)
M=$W/scratch/ran
o=$(OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch OPKIT_EXTRA=/nonexistent/file bash -c '
    . "$OPKIT_LIB"; ns_run touch "$1"' bash "$M" 2>&1); rc=$?
[ ! -e "$M" ] && [ $rc = 97 ] \
  || fail "ATTACK: ns_run ran its command although the isolation was not proved (rc $rc, marker $([ -e "$M" ] && echo present || echo absent)): $o"
echo "ok: ns_run refuses (97) and never starts the command when the proof fails"
M2=$W/scratch/ran2
o=$(OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M2" 2>&1); rc=$?
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
echo "PASS: opkit namespace helper"
