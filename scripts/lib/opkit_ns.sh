# opkit_ns.sh — the ONLY way a test may run the operator deployment kit
# (scripts/operator_deploy_protected_host.sh) or any other state-changing verb.
#
# Amendment 92. Incident 2026-10-06: a guard-removal experiment turned a kit
# refusal test into a REAL root --apply on the dev host. A refusal test that
# depends on the guard it tests becomes a real apply the moment that guard
# regresses, so the test must be unable to reach the host WITHOUT the guard.
#
# Why a namespace and not a `--root PREFIX` flag: the kit's destinations
# (/etc/axon, /usr/local/libexec, /var/lib/axon-*, /etc/systemd/system), the
# user database (useradd/getent), the setuid launcher's ownership, the
# production loader that is run as the Fabric uid, and trust_root_preflight.sh
# all resolve ABSOLUTE paths and the system user database; a prefix would make
# the tested kit a different program from the deployed one. So the prefix is the
# private mount namespace itself: every destination the kit can reach is
# shadowed by a tmpfs and the shadowing is PROVED before anything runs.
#
#   opkit_ns_isolate   (inside a fresh `unshare -m --propagation private`)
#                      shadow the destinations, then assert (below)
#   opkit_ns_assert    refuse (return 1) unless this process is in a mount
#                      namespace other than PID 1's and every destination is a
#                      tmpfs mount that does not show through to the host:
#                      a canary written under it is absent from /proc/1/root
#   ns_run CMD...      (outer shell) run CMD in a fresh isolated namespace;
#                      exit 97 if the isolation cannot be proved, CMD never ran.
#                      Amendment 97: the namespace is mount + PID (with its own /proc) + UTS + IPC +
#                      NET. NET is private (loopback only) unless OPKIT_NET=host, which a step that
#                      must reach a package registry (the controlled host build) states explicitly.
#                      Before unsharing, the outer shell opens a handle on the HOST's root and
#                      records the host's namespace ids: a PID namespace hides the host's PID 1, so
#                      the host's view is a descriptor, not /proc/1/root.
#
# OPKIT_RESTORE: "STASH=DEST ..." copy a stash (made earlier, under an UNshadowed path, by a step that
# itself ran under ns_run) back to DEST inside the namespace, with owners and modes.
# OPKIT_SCRATCH: a scratch directory (never under a shadowed path) for the /etc copy.
# OPKIT_EXTRA: files to install into the shadow /usr/local/bin.

# The destinations the kit can write. THE one list: opkit_ns_isolate shadows exactly these, the proof
# asserts exactly these, and scripts/opkit_ns_drift.py fails if a write target of the kit is outside them.
OPKIT_DEFAULT_DESTS="/etc /usr/local /var/lib /var/log /var/spool /var/mail /run /srv"

# The *_FOR_TEST variables exist only for scripts/test_opkit_ns.sh, which points the assertion at scratch
# directories (never at a real destination) and at a stand-in for the host. Amendment 97: they are
# honoured ONLY when the outermost script of this shell is scripts/test_opkit_ns.sh itself; for any other
# caller one that is set makes the proof REFUSE (it is never silently ignored, and never weakens it).
opkit_selftest_caller() {
  local top self here
  top=${BASH_SOURCE[${#BASH_SOURCE[@]} - 1]:-}
  [ -n "$top" ] || return 1
  top=$(realpath -e -- "$top" 2>/dev/null) || return 1
  here=$(dirname -- "$(realpath -e -- "${BASH_SOURCE[0]}")")
  self=$(realpath -e -- "$here/../test_opkit_ns.sh" 2>/dev/null) || return 1
  [ "$top" = "$self" ]
}

opkit_overrides() { # prints the effective "DESTS NS_PID VIEW_PID"; returns 1 (and says why) if overrides are not allowed
  if [ -n "${OPKIT_DESTS_FOR_TEST:-}${OPKIT_NS_PID_FOR_TEST:-}${OPKIT_VIEW_PID_FOR_TEST:-}" ]; then
    opkit_selftest_caller || { echo "REFUSE(opkit_ns): an OPKIT_*_FOR_TEST override is set outside scripts/test_opkit_ns.sh" >&2; return 1; }
    echo "${OPKIT_DESTS_FOR_TEST:-$OPKIT_DEFAULT_DESTS}|${OPKIT_NS_PID_FOR_TEST:-}|${OPKIT_VIEW_PID_FOR_TEST:-}"
  else
    echo "$OPKIT_DEFAULT_DESTS||"
  fi
}

opkit_ns_assert() {
  local ov dests nspid viewpid view d c hostmnt own fs kv k v host_dev here_dev
  [ "$(id -u)" = 0 ] || { echo "REFUSE(opkit_ns): not root, so no namespace to prove" >&2; return 1; }
  ov=$(opkit_overrides) || return 1
  dests=${ov%%|*}; ov=${ov#*|}; nspid=${ov%%|*}; viewpid=${ov#*|}
  # the HOST's view of the filesystem: the descriptor ns_run opened before it unshared (a PID namespace
  # hides the host's PID 1), or, for the self-test, the stand-in PID's root
  if [ -n "$viewpid" ]; then view=/proc/$viewpid/root
  elif [ -n "${OPKIT_HOST_FD:-}" ]; then view=/proc/self/fd/$OPKIT_HOST_FD
  else view=/proc/1/root; fi
  [ -d "$view/." ] && [ -r "$view/." ] \
    || { echo "REFUSE(opkit_ns): the host's view of the filesystem ($view) cannot be read, so a canary cannot be checked against it" >&2; return 1; }
  own=$(readlink /proc/self/ns/mnt)
  if [ -n "$nspid" ]; then hostmnt=$(readlink "/proc/$nspid/ns/mnt")
  elif [ -n "${OPKIT_HOST_NS:-}" ]; then hostmnt=$(tr ',' '\n' <<<"$OPKIT_HOST_NS" | sed -n 's/^mnt=//p')
  else hostmnt=$(readlink /proc/1/ns/mnt); fi
  [ -n "$own" ] && [ -n "$hostmnt" ] && [ "$own" != "$hostmnt" ] \
    || { echo "REFUSE(opkit_ns): this is the host's mount namespace (${own:-unreadable}): the host" >&2; return 1; }
  if [ -z "$nspid" ] && [ -n "${OPKIT_HOST_NS:-}" ]; then
    for kv in $(tr ',' ' ' <<<"$OPKIT_HOST_NS"); do
      k=${kv%%=*} v=${kv#*=}
      [ "$k" = mnt ] && continue
      [ "$k" = net ] && [ "${OPKIT_NET:-private}" = host ] && continue
      [ "$(readlink "/proc/self/ns/$k" 2>/dev/null)" != "$v" ] \
        || { echo "REFUSE(opkit_ns): the $k namespace is the host's ($v)" >&2; return 1; }
    done
  fi
  for d in $dests; do
    [ -L "$d" ] && continue          # a symlink (e.g. /var/mail) has no mount of its own; its target is covered
    [ -d "$d" ] || continue
    # the LAST mountinfo row for the mount point is the one in effect
    fs=$(awk -v m="$d" '$5 == m { f = $0 } END { n = split(f, a, " - "); if (n == 2) { split(a[2], b, " "); print b[1] } }' /proc/self/mountinfo)
    [ "$fs" = tmpfs ] || { echo "REFUSE(opkit_ns): $d is not shadowed by a tmpfs (found: ${fs:-none})" >&2; return 1; }
    c="$d/.opkit-ns-canary.$$"
    : >"$c" 2>/dev/null || { echo "REFUSE(opkit_ns): cannot write a canary under $d" >&2; return 1; }
    if [ -e "$view$c" ]; then rm -f "$c"; echo "REFUSE(opkit_ns): the canary under $d is VISIBLE to the host" >&2; return 1; fi
    rm -f "$c"
    # an independent proof, not a negative lookup: the host's own $d is a DIFFERENT filesystem object
    host_dev=$(stat -L -c '%d:%i' -- "$view$d" 2>/dev/null) \
      || { echo "REFUSE(opkit_ns): the host's $d cannot be examined, so its identity cannot be compared with the shadow" >&2; return 1; }
    here_dev=$(stat -c '%d:%i' -- "$d")
    [ "$host_dev" != "$here_dev" ] \
      || { echo "REFUSE(opkit_ns): $d is the HOST's own directory (device:inode $here_dev), not a shadow" >&2; return 1; }
  done
  return 0
}

opkit_ns_isolate() {
  local d keysave=${OPKIT_SCRATCH:?OPKIT_SCRATCH must be a scratch directory} r
  opkit_overrides >/dev/null || return 1
  keysave=$keysave/etc.$$
  mkdir -p "$keysave" || return 1
  mount -t tmpfs -o mode=0755 tmpfs "$keysave" || return 1   # a tmpfs of our own: nothing to leak
  cp -a /etc/. "$keysave/" && mount --bind "$keysave" /etc || { echo "REFUSE(opkit_ns): cannot shadow /etc" >&2; return 1; }
  for d in $OPKIT_DEFAULT_DESTS; do
    [ "$d" = /etc ] && continue
    if [ -L "$d" ]; then continue; fi          # /var/mail is a symlink on some hosts; its target is covered
    mount -t tmpfs -o mode=0755 tmpfs "$d" || { echo "REFUSE(opkit_ns): cannot shadow $d" >&2; return 1; }
  done
  # OPKIT_RESTORE: "STASH=DEST ..." copies a stash (taken earlier, under an UNshadowed path, by a step that
  # ran in its own namespace) back to DEST with owners and modes. Replaces the old carry-from-the-host
  # mechanism: nothing a test needs lives under a real destination any more.
  for r in ${OPKIT_RESTORE:-}; do
    [ -d "${r%%=*}" ] && { mkdir -p "${r#*=}" && cp -a "${r%%=*}/." "${r#*=}/"; } \
      || { echo "REFUSE(opkit_ns): cannot restore ${r%%=*}" >&2; return 1; }
  done
  if [ -n "${OPKIT_EXTRA:-}" ]; then mkdir -p /usr/local/bin && install -m 0755 $OPKIT_EXTRA /usr/local/bin/ || return 1; fi
  if [ "${OPKIT_NET:-private}" != host ]; then command -v ip >/dev/null && ip link set lo up 2>/dev/null; fi
  opkit_ns_assert
}

# Outer shell. Never runs CMD unless the isolation was proved first.
ns_run() {
  [ "$#" -gt 0 ] || return 2
  local hfd hostns rc
  exec {hfd}</ || { echo "REFUSE(ns_run): cannot open a handle on the host's root" >&2; return 97; }
  hostns="mnt=$(readlink /proc/self/ns/mnt),pid=$(readlink /proc/self/ns/pid),uts=$(readlink /proc/self/ns/uts),ipc=$(readlink /proc/self/ns/ipc),net=$(readlink /proc/self/ns/net)"
  local netflag=--net
  [ "${OPKIT_NET:-private}" = host ] && netflag=
  OPKIT_LIB=${OPKIT_LIB:?OPKIT_LIB must name opkit_ns.sh} OPKIT_HOST_FD=$hfd OPKIT_HOST_NS=$hostns \
  unshare --mount --propagation private --pid --fork --kill-child --mount-proc --uts --ipc $netflag bash -c '
    . "$OPKIT_LIB"
    opkit_ns_isolate || { echo "REFUSE(ns_run): isolation not proved; the command did not run" >&2; exit 97; }
    "$@"' ns_run "$@"
  rc=$?
  exec {hfd}<&-
  return $rc
}
