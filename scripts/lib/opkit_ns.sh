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
#                      exit 97 if the isolation cannot be proved, CMD never ran
#
# OPKIT_CARRY: space-separated directories under a shadowed path that CMD needs
# (the fixtures' builder-private parents); copied in with owners and modes.
# OPKIT_SCRATCH: a scratch directory (never under a shadowed path) for the carry area.
# OPKIT_EXTRA: files to install into the shadow /usr/local/bin.

# The two *_FOR_TEST variables exist only for scripts/test_opkit_ns.sh, which points the assertion at
# scratch directories (never at a real destination) and at a stand-in for PID 1. test_operator_deploy.sh
# refuses to start if either is set.
OPKIT_DESTS=${OPKIT_DESTS_FOR_TEST:-"/etc /usr/local /var/lib /var/log /var/spool /var/mail /run /srv"}
OPKIT_NS_PID=${OPKIT_NS_PID_FOR_TEST:-1}      # whose mount NAMESPACE we must differ from
OPKIT_VIEW_PID=${OPKIT_VIEW_PID_FOR_TEST:-1}  # whose VIEW of the filesystem a canary must not show in

opkit_ns_assert() {
  local d own host c rc=0
  [ "$(id -u)" = 0 ] || { echo "REFUSE(opkit_ns): not root, so no namespace to prove" >&2; return 1; }
  own=$(readlink /proc/self/ns/mnt) host=$(readlink /proc/$OPKIT_NS_PID/ns/mnt)
  [ -n "$own" ] && [ "$own" != "$host" ] \
    || { echo "REFUSE(opkit_ns): this is the host's mount namespace ($own): the host" >&2; return 1; }
  for d in $OPKIT_DESTS; do
    [ -L "$d" ] && continue          # a symlink (e.g. /var/mail) has no mount of its own; its target is covered
    [ -d "$d" ] || continue
    # the LAST mountinfo row for the mount point is the one in effect
    local fs
    fs=$(awk -v m="$d" '$5 == m { f = $0 } END { n = split(f, a, " - "); if (n == 2) { split(a[2], b, " "); print b[1] } }' /proc/self/mountinfo)
    [ "$fs" = tmpfs ] || { echo "REFUSE(opkit_ns): $d is not shadowed by a tmpfs (found: ${fs:-none})" >&2; return 1; }
    c="$d/.opkit-ns-canary.$$"
    : >"$c" 2>/dev/null || { echo "REFUSE(opkit_ns): cannot write a canary under $d" >&2; return 1; }
    if [ -e "/proc/$OPKIT_VIEW_PID/root$c" ]; then rm -f "$c"; echo "REFUSE(opkit_ns): the canary under $d is VISIBLE to the host" >&2; return 1; fi
    rm -f "$c"
  done
  return $rc
}

opkit_ns_isolate() {
  local d keysave=${OPKIT_SCRATCH:?OPKIT_SCRATCH must be a scratch directory} c
  keysave=$keysave/carry.$$
  mkdir -p "$keysave" || return 1
  mount -t tmpfs -o mode=0755 tmpfs "$keysave" || return 1   # a tmpfs of our own: nothing to leak
  for c in ${OPKIT_CARRY:-}; do [ -e "$c" ] && { mkdir -p "$keysave$c" && cp -a "$c/." "$keysave$c/"; } || true; done
  mkdir "$keysave/etc" && mount -t tmpfs -o mode=0755 tmpfs "$keysave/etc" && cp -a /etc/. "$keysave/etc/" \
    && mount --bind "$keysave/etc" /etc || { echo "REFUSE(opkit_ns): cannot shadow /etc" >&2; return 1; }
  for d in /usr/local /var/lib /var/log /var/spool /run /srv; do
    mount -t tmpfs -o mode=0755 tmpfs "$d" || { echo "REFUSE(opkit_ns): cannot shadow $d" >&2; return 1; }
  done
  [ -L /var/mail ] || mount -t tmpfs -o mode=0755 tmpfs /var/mail || { echo "REFUSE(opkit_ns): cannot shadow /var/mail" >&2; return 1; }
  for c in ${OPKIT_CARRY:-}; do
    [ -e "$keysave$c" ] && { mkdir -p "$c" && cp -a "$keysave$c/." "$c/" && chmod 0755 "$c"; } || true
  done
  if [ -n "${OPKIT_EXTRA:-}" ]; then mkdir -p /usr/local/bin && install -m 0755 $OPKIT_EXTRA /usr/local/bin/ || return 1; fi
  opkit_ns_assert
}

# Outer shell. Never runs CMD unless the isolation was proved first.
ns_run() {
  [ "$#" -gt 0 ] || return 2
  OPKIT_LIB=${OPKIT_LIB:?OPKIT_LIB must name opkit_ns.sh} \
  unshare -m --propagation private bash -c '
    . "$OPKIT_LIB"
    opkit_ns_isolate || { echo "REFUSE(ns_run): isolation not proved; the command did not run" >&2; exit 97; }
    exec "$@"' ns_run "$@"
}
