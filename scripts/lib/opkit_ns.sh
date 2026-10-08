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
# other mount of the namespace (/opt /home /root /usr /boot /var/cache, /sys, ...) is made read-only before
# the shadows are laid down, recursively and atomically (mount_setattr(MOUNT_ATTR_RDONLY, AT_RECURSIVE)).
# The extractor in opkit_ns_drift.py stays as a second layer. The host-root descriptor the proof compares
# through is CLOSED before the command starts; inside the namespace a re-assertion rests on a stamp the
# proof wrote, not on a host view.
#
# Amendment 105 -- WHAT THIS IS AND IS NOT. A read-only mount is a mount FLAG, not a boundary against a
# process that holds CAP_SYS_ADMIN, CAP_MKNOD or CAP_SYS_RAWIO. Round 11 measured root inside the
# namespace writing /dev/kmsg (it landed in the host's dmesg), `umount /proc` (which revealed the HOST's
# /proc, whose /proc/1/root reached the host's /opt), `mount -o remount,rw /` and `dd` to a block
# device. So the command runs with:
#   * a PRIVATE /dev (a tmpfs holding only null zero full random urandom tty, a private devpts, a private
#     /dev/shm; mounted read-only): there is no kmsg, no block device, nothing to dd to;
#   * a PRIVATE /proc: the host's /proc that `unshare --mount-proc` only COVERS is detached, so a
#     `umount /proc` can reveal nothing but the namespace's own (proved: exactly one mount per
#     /proc and /dev, and the one in effect is not the host's);
#   * nodev on every tmpfs and bind the helper makes, so a node made by `mknod` cannot be opened;
#   * a reduced capability BOUNDING SET (OPKIT_DROP_CAPS): no CAP_SYS_ADMIN (no mount, umount, remount,
#     pivot_root, new namespaces), CAP_MKNOD, CAP_SYS_MODULE, CAP_SYS_RAWIO, CAP_SYSLOG, CAP_SYS_PTRACE,
#     CAP_DAC_READ_SEARCH (open_by_handle_at reaches any inode of a filesystem, whatever the mount
#     namespace), CAP_NET_ADMIN/NET_RAW, CAP_SYS_BOOT/TIME, CAP_BPF/PERFMON ... A setuid program the
#     command runs gets no more than that set;
#   * no inherited descriptor above stderr, and nothing on fds 0-2 that is a directory, a block device,
#     or a writable regular file outside the caller's named scratch;
#     (so a caller logs a run through a PIPE -- `... | tee log` -- or puts the log under OPKIT_RW);
#   * OPKIT_RW entries that are real scratch (below a temp root, not a system path, not group/other
#     writable, not a symlink), never "whatever the environment said".
# WHAT REMAINS. A step that is handed CAP_SYS_ADMIN (OPKIT_CAPS_KEEP=sys_admin, used ONLY by the controlled
# build fixture, whose unshare --pid needs it) is NOT contained against a root that means to escape: it can
# `mount -o remount,rw /` and write the host. Nor is any root user namespace-less root against the host
# kernel (a module, a kernel bug). What holds, and what the tests below execute: UNPRIVILEGED build code
# and the kit AS WRITTEN (which does none of these) cannot reach the host through a mount, a device or
# /proc, and a command with the default capability set cannot do so even as root. A user namespace was
# considered and is NOT used: the kit's own tests chown to arbitrary uids, create accounts and exercise a
# setuid launcher, which a one-uid mapping cannot represent.
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
  if [ -z "${OPKIT_DESTS_FOR_TEST:-}" ] || [ "${OPKIT_REQUIRE_PRIVATE:-}" = 1 ]; then
    opkit_private_proof || return 1
  fi
  return 0
}

# Amendment 105: exactly one mount at /proc and one at /dev (a second, covered one is what `umount` would
# reveal), and /dev is the private tmpfs.
opkit_private_proof() {
  local probe n
  for probe in /proc /dev; do
    n=$(awk -v m="$probe" '$5 == m { c++ } END { print c + 0 }' /proc/self/mountinfo)
    [ "$n" = 1 ] || { echo "REFUSE(opkit_ns): $probe has $n mounts (1 expected): a covered mount is what umount would reveal" >&2; return 1; }
  done
  # /proc is writable (a user namespace needs its uid_map), so the paths that reach the host are read-only mounts of their own
  for probe in /proc/sys /proc/sysrq-trigger; do
    [ -e "$probe" ] || continue
    awk -v m="$probe" '$5 == m { split($6, o, ","); for (k in o) if (o[k] == "ro") found = 1 } END { exit !found }' /proc/self/mountinfo \
      || { echo "REFUSE(opkit_ns): $probe is not a read-only mount: a root without any capability can write it" >&2; return 1; }
  done
  [ "$(awk '$5 == "/dev" { split($0, a, " - "); split(a[2], b, " "); print b[1] }' /proc/self/mountinfo)" = tmpfs ] \
    || { echo "REFUSE(opkit_ns): /dev is not the private tmpfs" >&2; return 1; }
  return 0
}

# Every mount of this namespace is read-only unless it lies under a shadowed destination, an OPKIT_RW
# directory or /tmp; and a file cannot be created in the places no list names.
opkit_ro_proof() { # DESTS
  local bad d probe
  bad=$(awk -v ok="$1 ${OPKIT_RW:-} /tmp /dev/shm /dev/pts /proc" 'BEGIN { n = split(ok, a, " ") }
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

# ── amendment 105 ─────────────────────────────────────────────────────────────────────────────────
# The capability bounding set the command runs with (see the header). OPKIT_CAPS_KEEP names capabilities a
# step is handed anyway; the ONLY one that may be named is sys_admin.
OPKIT_DROP_CAPS="sys_admin sys_module sys_rawio sys_boot sys_time syslog mknod dac_read_search net_admin net_raw sys_ptrace bpf perfmon mac_admin mac_override audit_control linux_immutable sys_pacct sys_tty_config checkpoint_restore wake_alarm block_suspend sys_chroot"
opkit_bounding_arg() { # prints the setpriv --bounding-set argument; returns 1 on a capability that may not be kept
  local c k out=""
  for c in $OPKIT_DROP_CAPS; do
    for k in $(tr ',' ' ' <<<"${OPKIT_CAPS_KEEP:-}"); do
      [ "$k" = sys_admin ] || { echo "REFUSE(opkit_ns): OPKIT_CAPS_KEEP may name only sys_admin (got: $k)" >&2; return 1; }
      [ "$k" = "$c" ] && continue 2
    done
    out="$out,-$c"
  done
  echo "${out#,}"
}

# OPKIT_RW entries are scratch the caller made, never "whatever the environment said": absolute, a real
# directory (not a symlink), strictly below a temp root, owned by the caller, neither group- nor other-
# writable (a shared /var/tmp is 1777), and not a system path.
# OPKIT_RW_ROOTS_FOR_TEST: extra temp roots, honoured only for scripts/test_opkit_ns.sh (so its tests can reach the
# system-path guard without a real host directory being involved); any other caller that sets it is refused.
opkit_rw_extra_roots() {
  [ -n "${OPKIT_RW_ROOTS_FOR_TEST:-}" ] || return 0
  opkit_selftest_caller || { echo "REFUSE(opkit_ns): OPKIT_RW_ROOTS_FOR_TEST is set outside scripts/test_opkit_ns.sh" >&2; echo /nonexistent-refused; return 0; }
  echo "$OPKIT_RW_ROOTS_FOR_TEST"
}
opkit_rw_validate() { # DIR...
  local d r root ok mode own
  for d in "$@"; do
    [ -d "$d" ] || { echo "REFUSE(opkit_ns): OPKIT_RW entry '$d' is not a directory" >&2; return 1; }
    r=$(realpath -e -- "$d") || return 1
    # canonical: absolute, no symlink, no `..`, no double slash (one check for all of them)
    [ "$r" = "$d" ] || { echo "REFUSE(opkit_ns): OPKIT_RW entry '$d' is not canonical (it resolves to $r)" >&2; return 1; }
    case "$r" in
      /|/opt|/home|/root|/usr|/etc|/var|/boot|/dev|/proc|/sys|/run|/srv|/bin|/sbin|/lib|/lib64|/var/lib|/var/tmp|/tmp \
      |/opt/*|/usr/*|/etc/*|/boot/*|/dev/*|/proc/*|/sys/*|/root/*|/bin/*|/sbin/*|/lib/*|/lib64/*|/var/lib/*|/var/log/*|/var/spool/*|/var/mail/*|/var/cache/*|/run/*|/srv/*)
        echo "REFUSE(opkit_ns): OPKIT_RW entry '$r' is a system path" >&2; return 1 ;;
    esac
    ok=0
    for root in /tmp /var/tmp "${TMPDIR:-/var/tmp}" $(opkit_rw_extra_roots); do
      root=$(realpath -e -- "$root" 2>/dev/null) || continue
      case "$root" in /|/opt|/home|/root|/usr|/etc|/var|/boot|/dev|/proc|/sys) continue ;; esac
      case "$r" in "$root"/*) ok=1 ;; esac
    done
    [ $ok = 1 ] || { echo "REFUSE(opkit_ns): OPKIT_RW entry '$r' is not below a temp root (/tmp, /var/tmp, \$TMPDIR): it is not scratch this run made" >&2; return 1; }
    mode=$(stat -c %a -- "$r"); own=$(stat -c %u -- "$r")
    [ "$own" = "$(id -u)" ] || { echo "REFUSE(opkit_ns): OPKIT_RW entry '$r' is owned by uid $own, not the caller" >&2; return 1; }
    [ $(( 8#$mode & 8#022 )) = 0 ] || { echo "REFUSE(opkit_ns): OPKIT_RW entry '$r' is group- or other-writable (mode $mode): it is shared, not scratch" >&2; return 1; }
  done
  return 0
}

# A descriptor the command would inherit that is a way around the shadows: prints why and returns 1.
opkit_ns_fd_ok() { # N [ROOTS...]  (a writable regular file is accepted only under one of ROOTS)
  local n=$1 p=/proc/$BASHPID/fd/$1 flags acc t rt typ   # $BASHPID: "self" inside $( ) is the child
  shift
  [ -e "$p" ] || return 0
  if [ -d "$p" ]; then echo "LEAK: descriptor $n is a directory ($(readlink "$p" 2>/dev/null))" >&2; return 1; fi
  if [ -b "$p" ]; then echo "LEAK: descriptor $n is a block device ($(readlink "$p" 2>/dev/null))" >&2; return 1; fi
  if [ -c "$p" ]; then
    typ=$(stat -L -c '%t:%T' -- "$p")
    case "$typ" in
      1:3|1:5|1:7|1:8|1:9|5:0|5:1|5:2|4:*|8[0-9a-f]:*) ;;   # null zero full (u)random tty console ptmx, vt/serial, pts (hex majors)
      *) echo "LEAK: descriptor $n is a character device ($(readlink "$p" 2>/dev/null), $typ)" >&2; return 1 ;;
    esac
    return 0
  fi
  if [ -f "$p" ]; then
    flags=$(sed -n 's/^flags:[[:space:]]*//p' "/proc/$BASHPID/fdinfo/$n" 2>/dev/null)
    [ -n "$flags" ] || { echo "LEAK: descriptor $n: its open mode cannot be read" >&2; return 1; }
    acc=$(( 8#$flags & 3 ))
    if [ "$acc" != 0 ]; then
      t=$(readlink -f -- "$p" 2>/dev/null)
      for rt in "$@"; do
        [ -n "$rt" ] && case "$t" in "$rt"/*) return 0 ;; esac
      done
      echo "LEAK: descriptor $n is a WRITABLE regular file ($t) outside the caller's named scratch" >&2; return 1
    fi
  fi
  return 0
}

# fds 0-2 are checked, every other inherited descriptor is closed (OPKIT_KEEP_FDS names those kept, each
# held to the same check). The caller's named scratch is OPKIT_RW and OPKIT_SCRATCH.
opkit_ns_sanitize_fds() {
  local f n bad=0 roots="${OPKIT_RW:-} ${OPKIT_SCRATCH:-}" keep=" $(tr ',' ' ' <<<"${OPKIT_KEEP_FDS:-}") "
  for n in 0 1 2; do opkit_ns_fd_ok "$n" $roots || bad=1; done
  for f in /proc/self/fd/*; do
    n=${f##*/}
    case "$n" in 0|1|2) continue ;; esac
    [ -e "$f" ] || continue
    case "$keep" in *" $n "*) opkit_ns_fd_ok "$n" $roots || bad=1; continue ;; esac
    eval "exec $n<&-" 2>/dev/null || true
  done
  [ "$bad" = 0 ]
}

# The host's /proc that `unshare --mount-proc` only COVERS is detached, so a `umount /proc` reveals nothing of
# the host; /proc is then mounted fresh with hidepid and the host-affecting paths masked or read-only
# (/proc/sysrq-trigger and /proc/sys are writable by root WITHOUT any capability, so a read-only flag on the
# mount alone would be the only thing between a command and the host -- and a read-only /proc cannot take a
# uid_map write, which a nested user namespace needs). The masks are bind mounts: removing one takes
# CAP_SYS_ADMIN, which the command does not have unless it was handed it.
opkit_ns_fresh_proc() {
  local f
  umount -l -R /proc 2>/dev/null || true       # the namespace's own proc mount, if any ...
  umount -l -R /proc 2>/dev/null || true       # ... and the host's one it covered
  umount -l -R /proc 2>/dev/null || true       # (a third, in case the namespace was set up with more)
  mount -t proc -o nosuid,nodev,hidepid=2 proc /proc || mount -t proc -o nosuid,nodev proc /proc \
    || { echo "REFUSE(opkit_ns): cannot mount a private /proc" >&2; return 1; }
  for f in sys sysrq-trigger irq bus fs; do    # read-only
    [ -e "/proc/$f" ] || continue
    mount --bind "/proc/$f" "/proc/$f" && mount -o remount,bind,ro "/proc/$f" \
      || { echo "REFUSE(opkit_ns): cannot make /proc/$f read-only" >&2; return 1; }
  done
  for f in kcore keys latency_stats timer_list timer_stats sched_debug kmsg; do   # masked
    [ -e "/proc/$f" ] || continue
    mount --bind /dev/null "/proc/$f" || { echo "REFUSE(opkit_ns): cannot mask /proc/$f" >&2; return 1; }
  done
  for f in acpi scsi; do
    [ -d "/proc/$f" ] || continue
    mount -t tmpfs -o ro,nosuid,nodev,noexec,size=0k tmpfs "/proc/$f" || { echo "REFUSE(opkit_ns): cannot mask /proc/$f" >&2; return 1; }
  done
}

# A private minimal /dev: null zero full random urandom tty, a private devpts and /dev/shm. Built under a
# scratch mount point (made before the root was read-only), then moved over /dev after the host's /dev
# was detached. The device nodes are bind mounts taken while the host's /dev is still visible.
opkit_ns_private_dev() { # MOUNTPOINT
  local nd=$1 n
  mount -t tmpfs -o mode=0755,nosuid,noexec tmpfs "$nd" || return 1
  for n in null zero full random urandom tty; do
    : >"$nd/$n" && mount --bind "/dev/$n" "$nd/$n" || { echo "REFUSE(opkit_ns): cannot give the command /dev/$n" >&2; return 1; }
  done
  mkdir "$nd/pts" "$nd/shm" || return 1
  ln -s /proc/self/fd "$nd/fd" && ln -s /proc/self/fd/0 "$nd/stdin" && ln -s /proc/self/fd/1 "$nd/stdout" \
    && ln -s /proc/self/fd/2 "$nd/stderr" && ln -s pts/ptmx "$nd/ptmx" || return 1
  mount -t devpts -o newinstance,ptmxmode=0666,mode=0620,nosuid,noexec devpts "$nd/pts" || return 1
  mount -t tmpfs -o mode=1777,nosuid,nodev,noexec tmpfs "$nd/shm" || return 1
  mount -o remount,ro,bind "$nd" "$nd" 2>/dev/null || mount -o remount,ro "$nd" || return 1
  umount -l -R /dev || { echo "REFUSE(opkit_ns): cannot detach the host's /dev" >&2; return 1; }
  mount --move "$nd" /dev || { echo "REFUSE(opkit_ns): cannot install the private /dev" >&2; return 1; }
}

opkit_ns_isolate() {
  local d keysave=${OPKIT_SCRATCH:?OPKIT_SCRATCH must be a scratch directory} r devmp
  opkit_overrides >/dev/null || return 1
  keysave=$keysave/etc.$$
  devmp=${OPKIT_SCRATCH}/dev.$$
  mkdir -p "$keysave" "$devmp" || return 1    # before the root is read-only: scratch is the caller's
  # DENY BY DEFAULT: everything is read-only from here on; only the mounts made below are writable
  opkit_ns_make_ro || return 1
  opkit_ns_fresh_proc || return 1
  opkit_ns_private_dev "$devmp" || return 1
  mount -t tmpfs -o mode=0755,nodev tmpfs "$keysave" || return 1   # a tmpfs of our own: nothing to leak
  cp -a /etc/. "$keysave/" && mount --bind "$keysave" /etc || { echo "REFUSE(opkit_ns): cannot shadow /etc" >&2; return 1; }
  umount "$keysave" || { echo "REFUSE(opkit_ns): cannot release the scratch mount of the /etc copy" >&2; return 1; }   # /etc keeps the tmpfs
  for d in $OPKIT_DEFAULT_DESTS; do
    [ "$d" = /etc ] && continue
    if [ -L "$d" ]; then continue; fi          # /var/mail is a symlink on some hosts; its target is covered
    mount -t tmpfs -o mode=0755,nodev tmpfs "$d" || { echo "REFUSE(opkit_ns): cannot shadow $d" >&2; return 1; }
  done
  mount -t tmpfs -o mode=1777,nodev,nosuid tmpfs /tmp || { echo "REFUSE(opkit_ns): cannot give the command a tmpfs /tmp" >&2; return 1; }
  for d in ${OPKIT_RW:-}; do                   # scratch the caller made for this run, named explicitly
    [ -d "$d" ] && mount --bind "$d" "$d" && mount -o remount,bind,rw,nodev "$d" \
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
  local hfd hostns rc bset
  opkit_rw_validate ${OPKIT_RW:-} || return 97
  bset=$(opkit_bounding_arg) || return 97
  exec {hfd}</ || { echo "REFUSE(ns_run): cannot open a handle on the host's root" >&2; return 97; }
  hostns="mnt=$(readlink /proc/self/ns/mnt),pid=$(readlink /proc/self/ns/pid),uts=$(readlink /proc/self/ns/uts),ipc=$(readlink /proc/self/ns/ipc),net=$(readlink /proc/self/ns/net)"
  local netflag=--net
  [ "${OPKIT_NET:-private}" = host ] && netflag=
  OPKIT_LIB=${OPKIT_LIB:?OPKIT_LIB must name opkit_ns.sh} OPKIT_HOST_FD=$hfd OPKIT_HOST_NS=$hostns OPKIT_BSET=$bset \
  unshare --mount --propagation private --pid --fork --kill-child --mount-proc --uts --ipc $netflag bash -c '
    . "$OPKIT_LIB"
    opkit_ns_isolate || { echo "REFUSE(ns_run): isolation not proved; the command did not run" >&2; exit 97; }
    # Amendment 101: the descriptor on the host root served the proof; the command must not inherit it
    opkit_ns_drop_host_fd || { echo "REFUSE(ns_run): a directory descriptor outlived the proof; the command did not run" >&2; exit 97; }
    # Amendment 105: nothing else inherited is a way out either (fds 0-2 are checked, the rest closed)
    opkit_ns_sanitize_fds || { echo "REFUSE(ns_run): an inherited descriptor is a way around the shadows; the command did not run" >&2; exit 97; }
    bset=$OPKIT_BSET; unset OPKIT_BSET
    # the command runs WITHOUT the capabilities that make a read-only mount a mere flag
    exec setpriv --bounding-set "$bset" -- "$@"' ns_run "$@"
  rc=$?
  exec {hfd}<&-
  return $rc
}
