#!/usr/bin/env bash
# test_opkit_ns.sh — the namespace helper (scripts/lib/opkit_ns.sh) refuses when its proof fails.
# Amendment 92. Every attack points the assertion at SCRATCH directories and a stand-in for PID 1
# (the *_FOR_TEST variables); nothing here touches a real destination, and ns_run's own shadowing is
# done inside a private mount namespace.
# Exit 0 all held; 1 an assertion failed (the message starts "ATTACK:" when a refusal did not happen);
# 77 not root or no unshare.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
LIB=$HERE/lib/opkit_ns.sh
[ "$(id -u)" = 0 ] && command -v unshare >/dev/null || { echo "SKIP: needs root and unshare"; exit 77; }
W=$(mktemp -d "${TMPDIR:-/var/tmp}/axon-opkit-ns-test.XXXXXX") || exit 1
trap 'rm -rf "$W"' EXIT
mkdir -p "$W/a" "$W/b" "$W/scratch"
fail() { echo "FAIL: $*"; exit 1; }

# Runs the assertion in a fresh private namespace with $W/a (and $W/b when TMPFS_B=1) a tmpfs.
assert_in_ns() { # VIEW_PID NS_PID -> prints stderr, exits with the assertion's rc
  TMPFS_B=${TMPFS_B:-1} unshare -m --propagation private bash -c '
    mount -t tmpfs tmpfs "$1/a" || exit 90
    [ "$TMPFS_B" = 1 ] && { mount -t tmpfs tmpfs "$1/b" || exit 90; }
    export OPKIT_DESTS_FOR_TEST="$1/a $1/b" OPKIT_VIEW_PID_FOR_TEST=$2 OPKIT_NS_PID_FOR_TEST=$3
    . "$4"; opkit_ns_assert' bash "$W" "$1" "$2" "$LIB" 2>&1
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
o=$(unshare -m --propagation private bash -c '
    mount -t tmpfs tmpfs "$1/a" && mount -t tmpfs tmpfs "$1/b" || exit 90
    export OPKIT_DESTS_FOR_TEST="$1/a $1/b" OPKIT_VIEW_PID_FOR_TEST=$$ OPKIT_NS_PID_FOR_TEST=1
    . "$2"; opkit_ns_assert' bash "$W" "$LIB" 2>&1); rc=$?
{ [ $rc = 1 ] && grep -q 'is VISIBLE to the host' <<<"$o"; } \
  || fail "ATTACK: a canary visible from the host's view was accepted (rc $rc): $o"
echo "ok: a canary that shows through to the host is refused"

# ns_run never starts its command when the proof fails
M=$W/scratch/ran
o=$(OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch OPKIT_VIEW_PID_FOR_TEST=self bash -c '
    . "$OPKIT_LIB"; ns_run touch "$1"' bash "$M" 2>&1); rc=$?
[ ! -e "$M" ] && [ $rc = 97 ] \
  || fail "ATTACK: ns_run ran its command although the isolation was not proved (rc $rc, marker $([ -e "$M" ] && echo present || echo absent)): $o"
echo "ok: ns_run refuses (97) and never starts the command when the proof fails"
M2=$W/scratch/ran2
o=$(OPKIT_LIB=$LIB OPKIT_SCRATCH=$W/scratch bash -c '. "$OPKIT_LIB"; ns_run touch "$1"' bash "$M2" 2>&1); rc=$?
{ [ -e "$M2" ] && [ $rc = 0 ]; } || fail "control: ns_run did not run its command in a proved namespace ($rc): $o"
echo "ok: control: ns_run runs its command in a proved namespace"
echo "PASS: opkit namespace helper"
