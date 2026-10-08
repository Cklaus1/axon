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
#                      tmpfs mount that is not the host's own directory (device:inode
#                      compared through the host's view)
#   ns_run CMD...      (outer shell) run CMD in a fresh isolated namespace;
#                      exit 97 if the isolation cannot be proved, CMD never ran.
#                      Amendment 97: the namespace is mount + PID (with its own /proc) + UTS + IPC +
#                      NET. NET is private (loopback only) unless OPKIT_NET=host, which a step that
#                      must reach a package registry (the controlled host build) states explicitly.
#                      Before unsharing, the outer shell opens a handle on the HOST's root and
#                      records the host's namespace ids: a PID namespace hides the host's PID 1, so
#                      the host's view is a descriptor, not /proc/1/root.
#
# Amendment 101: DENY BY DEFAULT. The shadow list above is the set of places a command may WRITE; every
# other mount of the namespace (/opt /home /root /usr /boot /var/cache, /proc /sys /dev, ...) is made
# read-only before the shadows are laid down, recursively and atomically (mount_setattr(MOUNT_ATTR_RDONLY,
# AT_RECURSIVE)), so a write by ANY verb to a place nobody listed fails with EROFS instead of reaching
# the host. The extractor in opkit_ns_drift.py stays as a second layer. The host-root descriptor the proof
# compares through is CLOSED before the command starts (a handle on the host's filesystem inside the
# command is a way around every shadow); inside the namespace a re-assertion rests on a stamp the proof
# wrote, not on a host view.
# OPKIT_RW: space-separated host directories the command may write (scratch the test itself created).
# /tmp is a fresh tmpfs and TMPDIR is /tmp inside.
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
  elif opkit_stamp_ok; then view=stamp         # inside ns_run after the descriptor was closed: the proof's stamp
  else view=/proc/1/root; fi
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
    rm -f "$c"
    # The proof is an IDENTITY comparison, not a negative lookup (a canary "not seen" through an unreadable
    # /proc/1/root passed vacuously): the host's own $d, read through the host's view, must be a
    # DIFFERENT filesystem object (device:inode) from ours, and an unreadable view or an unstatable $d is a refusal.
    [ "$view" = stamp ] && continue   # the identity comparison was made once, by the proof that wrote the stamp
    host_dev=$(stat -L -c '%d:%i' -- "$view$d" 2>/dev/null) \
      || { echo "REFUSE(opkit_ns): the host's $d cannot be examined through the host's view ($view), so its identity cannot be compared with the shadow" >&2; return 1; }
    here_dev=$(stat -c '%d:%i' -- "$d")
    [ "$host_dev" != "$here_dev" ] \
      || { echo "REFUSE(opkit_ns): $d is the HOST's own directory (device:inode $here_dev), not a shadow" >&2; return 1; }
  done
  # DENY BY DEFAULT (amendment 101): outside the self-test overrides the root must be read-only except
  # the shadows; the self-test asks for the same proof with OPKIT_REQUIRE_RO=1
  if [ -z "${OPKIT_DESTS_FOR_TEST:-}" ] || [ "${OPKIT_REQUIRE_RO:-}" = 1 ]; then
    opkit_ro_proof "$dests" || return 1
  fi
  return 0
}

# Every mount of this namespace is read-only unless it lies under a shadowed destination, an OPKIT_RW
# directory or /tmp; and a file cannot be created in the places no list names.
opkit_ro_proof() { # DESTS
  local bad d probe
  bad=$(awk -v ok="$1 ${OPKIT_RW:-} /tmp" 'BEGIN { n = split(ok, a, " ") }
    { mp = $5; skip = 0
      for (i = 1; i <= n; i++) if (mp == a[i] || index(mp, a[i] "/") == 1) skip = 1
      if (skip) next
      split($6, o, ","); ro = 0; for (k in o) if (o[k] == "ro") ro = 1
      if (!ro) print mp }' /proc/self/mountinfo)
  [ -z "$bad" ] || { echo "REFUSE(opkit_ns): writable mounts outside the shadows (the root is not read-only): $(tr '\n' ' ' <<<"$bad")" >&2; return 1; }
  for probe in / /opt /home /root /usr /usr/lib /boot /var /var/cache /bin; do
    [ -d "$probe" ] || continue
    case " $1 ${OPKIT_RW:-} /tmp " in *" $probe "*) continue ;; esac
    d="$probe/.opkit-ro-canary.$$"
    if { : >"$d"; } 2>/dev/null; then rm -f "$d"; echo "REFUSE(opkit_ns): a file can be created under $probe: the root is not read-only" >&2; return 1; fi
  done
  return 0
}

# The proof writes this once it holds; a command inside the namespace re-asserts from it (the host
# descriptor is gone by then). Content: our namespace ids and the host's.
OPKIT_STAMP=/run/.opkit-ns-proved
opkit_ns_ids() { echo "mnt=$(readlink /proc/self/ns/mnt),pid=$(readlink /proc/self/ns/pid),uts=$(readlink /proc/self/ns/uts),ipc=$(readlink /proc/self/ns/ipc)"; }
opkit_stamp_ok() {
  local s own host
  [ -f "$OPKIT_STAMP" ] && [ ! -L "$OPKIT_STAMP" ] && [ "$(stat -c %u "$OPKIT_STAMP")" = 0 ] || return 1
  s=$(cat "$OPKIT_STAMP") || return 1
  own=${s%% *}; host=${s#* }
  [ "$own" = "$(opkit_ns_ids)" ] || return 1
  [ "$(sed -n 's/^mnt=\([^,]*\).*/\1/p' <<<"$own")" != "$(sed -n 's/^mnt=\([^,]*\).*/\1/p' <<<"$host")" ]
}

# Make every mount of this namespace read-only in one recursive step. Refuses (1) if it cannot.
opkit_ns_make_ro() {
  python3 -S - <<'PY' || { echo "REFUSE(opkit_ns): cannot make the root read-only (mount_setattr)" >&2; return 1; }
import ctypes, sys
libc = ctypes.CDLL(None, use_errno=True)
class A(ctypes.Structure):
    _fields_ = [("attr_set", ctypes.c_uint64), ("attr_clr", ctypes.c_uint64), ("propagation", ctypes.c_uint64), ("userns_fd", ctypes.c_uint64)]
a = A(1, 0, 0, 0)                        # MOUNT_ATTR_RDONLY
r = libc.syscall(442, -100, b"/", 0x8000, ctypes.byref(a), ctypes.sizeof(a))   # mount_setattr(AT_FDCWD, "/", AT_RECURSIVE)
if r != 0:
    sys.stderr.write("mount_setattr failed: errno %d\n" % ctypes.get_errno()); sys.exit(1)
PY
}

# Any directory descriptor above stderr is a way out of the shadows; prints each and returns 1.
opkit_ns_fd_leak() {
  local f n=0 t
  for f in /proc/self/fd/*; do
    case "${f##*/}" in 0|1|2) continue ;; esac
    [ -d "$f" ] || continue
    t=$(readlink "$f" 2>/dev/null)
    echo "LEAK: descriptor ${f##*/} -> ${t:-?}" >&2; n=$((n + 1))
  done
  [ "$n" = 0 ]
}

# Called once, after the proof: closes the host-root descriptor and checks nothing like it survives.
opkit_ns_drop_host_fd() {
  if [ -n "${OPKIT_HOST_FD:-}" ]; then eval "exec $OPKIT_HOST_FD<&-"; unset OPKIT_HOST_FD; fi
  opkit_ns_fd_leak
}

opkit_ns_isolate() {
  local d keysave=${OPKIT_SCRATCH:?OPKIT_SCRATCH must be a scratch directory} r
  opkit_overrides >/dev/null || return 1
  keysave=$keysave/etc.$$
  mkdir -p "$keysave" || return 1            # before the root is read-only: scratch is the caller's
  # DENY BY DEFAULT: everything is read-only from here on; only the mounts made below are writable
  opkit_ns_make_ro || return 1
  mount -t tmpfs -o mode=0755 tmpfs "$keysave" || return 1   # a tmpfs of our own: nothing to leak
  cp -a /etc/. "$keysave/" && mount --bind "$keysave" /etc || { echo "REFUSE(opkit_ns): cannot shadow /etc" >&2; return 1; }
  umount "$keysave" || { echo "REFUSE(opkit_ns): cannot release the scratch mount of the /etc copy" >&2; return 1; }   # /etc keeps the tmpfs
  for d in $OPKIT_DEFAULT_DESTS; do
    [ "$d" = /etc ] && continue
    if [ -L "$d" ]; then continue; fi          # /var/mail is a symlink on some hosts; its target is covered
    mount -t tmpfs -o mode=0755 tmpfs "$d" || { echo "REFUSE(opkit_ns): cannot shadow $d" >&2; return 1; }
  done
  mount -t tmpfs -o mode=1777 tmpfs /tmp || { echo "REFUSE(opkit_ns): cannot give the command a tmpfs /tmp" >&2; return 1; }
  for d in ${OPKIT_RW:-}; do                   # scratch the caller made for this run, named explicitly
    [ -d "$d" ] && mount --bind "$d" "$d" && mount -o remount,bind,rw "$d" \
      || { echo "REFUSE(opkit_ns): cannot make $d writable" >&2; return 1; }
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
  opkit_ns_assert || return 1
  # the stamp a later re-assertion rests on (the host descriptor is closed before the command starts)
  printf '%s %s\n' "$(opkit_ns_ids)" "$(tr ',' '\n' <<<"${OPKIT_HOST_NS:-}" | grep -v '^net=' | paste -sd,)" >"$OPKIT_STAMP" \
    && chmod 0600 "$OPKIT_STAMP" || return 1
  export TMPDIR=/tmp
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
    # Amendment 101: the descriptor on the host root served the proof; the command must not inherit it
    opkit_ns_drop_host_fd || { echo "REFUSE(ns_run): a directory descriptor outlived the proof; the command did not run" >&2; exit 97; }
    "$@"' ns_run "$@"
  rc=$?
  exec {hfd}<&-
  return $rc
}
