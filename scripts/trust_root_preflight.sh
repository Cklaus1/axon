#!/usr/bin/env bash
# trust_root_preflight.sh — EXECUTABLE proof that the operator trust root is
# readable by the verifier and modifiable by nobody who must not modify it
# (operator direction 2026-09-28).
#
# Unix mode bits are a claim; this script makes REAL attempts under each
# intended service UID (setpriv) and requires the kernel to refuse them:
#
#   verifier   CAN read every file under qualification/ and verifier.json
#   verifier   cannot create, rename or open-for-write anything, or chmod
#   custodian  cannot either
#   agent(s)   (MiCode, Claude — every UID an agent runs as) cannot either
#   fabric     cannot either; it alone can READ the attestation signing key,
#              which verifier, custodian and every agent must fail to open (A20)
#   …and all of this also over every path the protected-host config pins (O1),
#              as `axon-fabric protected-host-paths` lists them: the list
#              ProtectedHost::load itself ownership-walks, never a copy kept
#              here (C9 dev review round 1: a copy here missed the grant
#              registry and grant files, the observer command, artifacts_dir and
#              the out_root / nonce_store directories)
#   service    out_root belongs to the Fabric UID: no other actor can create
#              in it or chmod it
#   custodian  (amendment 50, decision D6) the observation nonce is issued,
#              stored and spent by the CUSTODIAN as its own uid: its config
#              (/etc/axon/custodian.json) is an operator file; it runs as the
#              --custodian uid, which is neither the --fabric uid nor root; it
#              issues to the --fabric uid and spends for root (the helper)
#              only; its store is its own 0700 directory, which the custodian
#              CAN create in and the Fabric, the verifier and every agent
#              cannot (create or chmod); the directory its socket is bound in
#              is the operator's
#   helper     (operator decision A, amendment 45) Fabric runs NON-ROOT (root is
#              refused as every actor) and reaches a root launch only through
#              the privileged helper `axon-protected-launcher` the host config
#              pins: root-owned, setuid, not group/other-writable, no access
#              for other; its own config (/etc/axon/protected-launcher.json)
#              and every path THAT pins are operator files too, and it admits
#              exactly the --fabric uid. Executed for real: as the Fabric uid
#              `--probe` must report effective uid 0 (setuid honoured, no
#              nosuid mount; a production build in protected mode), and as
#              every other actor the kernel must refuse the exec
#   guest      cannot even ADDRESS the root (--guest-cmd runs
#              trust_root_guest_probe.sh inside the candidate guest)
#   observer   (amendment 68, decisions G/G1) the observer is a SERVICE with its
#              own uid (--observer; required in protected mode): its config
#              (/etc/axon/observer.json) is an operator file naming exactly that
#              uid, the --fabric uid and caller 0, and the helper config's
#              observer.service names the same socket and uid; it is neither the
#              Fabric, the custodian, the verifier, an agent nor root; its key is
#              a 0400 file it owns that IT can open and the Fabric, the
#              verifier, the custodian and every agent cannot; nobody but it
#              creates in (or chmods) its key directory or record store; its
#              socket's directory and its state directory are the operator's;
#              and no actor but root can even CONNECT to its socket (mode 0600
#              root:root — the setuid-root helper is its only client)
#   nnp        (amendment 65) the RUNNING Fabric service (--fabric-pid; required
#              in protected mode) is the --fabric uid and has NoNewPrivs 0. Under
#              NoNewPrivileges the kernel ignores the helper's set-id bit, so
#              every launch is refused; the probe above runs through setpriv
#              without it and cannot see how the service is really started
#
# Every write attempt is NON-DESTRUCTIVE if it unexpectedly succeeds: a probe
# directory is created and removed, a file is opened read-write without being
# written, chmod re-applies the file's own mode. A success is a FAIL either way.
#
# Modes:
#   protected  root is fixed at /etc/axon/trust, and every ancestor from / is
#              probed too. The path list comes from the INSTALLED verifier
#              binary (`path` in /etc/axon/trust/verifier.json). The only mode
#              readiness accepts (readiness.rs: TRUST_PREFLIGHT_SCHEMA, mode
#              "protected").
#   dev        --root DIR (a fixture); only the root and below are probed, and
#              --fabric-bin names the axon-fabric that lists the paths.
#              Proves the mechanism, certifies nothing.
#
# Usage (as root, which is needed to switch UID — never as the actors):
#   trust_root_preflight.sh --verifier UID[:GID] --custodian UID[:GID] --fabric UID[:GID] \
#       --observer UID[:GID] --agent UID[:GID] [--agent …] --guest-cmd 'CMD' [--fabric-pid PID] \
#       [--root DIR --host-config FILE --launcher-config FILE --custodian-config FILE --fabric-bin FILE
#        [--observer-config FILE]] [--out FILE]
# (dev mode: --observer is optional and needs --observer-config and --launcher-config.)
#
# Exit 0 = PASS, 1 = FAIL (a refusal did not happen), 2 = cannot run (usage,
# not root, root missing) — never a pass.
set -uo pipefail

OPERATOR_TRUST_ROOT=/etc/axon/trust
SCHEMA=axon-trust-preflight/1
ROOT="" OUT="" GUEST="" VERIFIER="" CUSTODIAN="" FABRIC="" HOST_CONFIG="" SIGNING_KEY="" FABRIC_BIN=""
FABRIC_PID="" OBSERVER="" OBSERVER_CONFIG="" HELPER_CONFIG_FILE=""
LAUNCHER_CONFIG="" HELPER="" HELPER_FABRIC_UID="" HELPER_FABRIC_PATH="" HELPER_FABRIC_SHA="" CUSTODIAN_OBSERVER_UID="-"
CUSTODIAN_CONFIG="" CUSTODIAN_STORE="" CUSTODIAN_UID="" CUSTODIAN_FABRIC_UID="" CUSTODIAN_LAUNCHER_UID=""
O1=() O1_DIRS=() SERVICE_DIRS=() SOCKET_DIRS=() AUTHORITY_STORES=()
AGENTS=()
die() { printf '{"schema":"%s","verdict":"NOT_RUN","reason":"%s"}\n' "$SCHEMA" "$1"; exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --root) ROOT="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --guest-cmd) GUEST="$2"; shift 2 ;;
    --verifier) VERIFIER="$2"; shift 2 ;;
    --custodian) CUSTODIAN="$2"; shift 2 ;;
    --fabric) FABRIC="$2"; shift 2 ;;
    --fabric-pid) FABRIC_PID="$2"; shift 2 ;;
    --observer) OBSERVER="$2"; shift 2 ;;
    --observer-config) OBSERVER_CONFIG="$2"; shift 2 ;;
    --host-config) HOST_CONFIG="$2"; shift 2 ;;
    --fabric-bin) FABRIC_BIN="$2"; shift 2 ;;
    --launcher-config) LAUNCHER_CONFIG="$2"; shift 2 ;;
    --custodian-config) CUSTODIAN_CONFIG="$2"; shift 2 ;;
    --agent) AGENTS+=("$2"); shift 2 ;;
    *) die "unknown argument $1" ;;
  esac
done
[ "$(id -u)" = 0 ] || die "must run as root to act as each service UID"
command -v setpriv >/dev/null || die "setpriv is required"
[ -n "$VERIFIER" ] && [ -n "$CUSTODIAN" ] && [ -n "$FABRIC" ] && [ ${#AGENTS[@]} -gt 0 ] && [ -n "$GUEST" ] \
  || die "--verifier, --custodian, --fabric, at least one --agent and --guest-cmd are required"
if [ -n "$ROOT" ]; then MODE=dev; else MODE=protected; ROOT=$OPERATOR_TRUST_ROOT; fi
# Amendment 65: protected mode judges the Fabric service as it really runs.
[ "$MODE" = dev ] || [ -n "$FABRIC_PID" ] \
  || die "--fabric-pid PID (the running Fabric service's main pid, e.g. systemctl show -p MainPID) is required in protected mode"
case "$FABRIC_PID" in ""|*[!0-9]*) [ -z "$FABRIC_PID" ] || die "--fabric-pid must be a pid" ;; esac
# Amendment 68: protected mode judges the observer SERVICE too.
[ "$MODE" = dev ] || [ -n "$OBSERVER" ] \
  || die "--observer UID (the axon-observer service's uid, observer.json's observer_uid) is required in protected mode"
# O1 (v022-psv-protocol.md §2): the protected-host config and every path it
# pins are operator authority too, and its signing key is the Fabric UID's
# alone. Protected mode reads the fixed file; dev mode needs --host-config.
if [ "$MODE" = protected ]; then
  [ -z "$HOST_CONFIG" ] || die "--host-config is dev-only: protected mode reads /etc/axon/protected-host.json"
  [ -z "$FABRIC_BIN" ] || die "--fabric-bin is dev-only: protected mode runs the installed verifier named by $ROOT/verifier.json"
  [ -z "$LAUNCHER_CONFIG" ] || die "--launcher-config is dev-only: protected mode reads /etc/axon/protected-launcher.json"
  [ -z "$CUSTODIAN_CONFIG" ] || die "--custodian-config is dev-only: protected mode reads /etc/axon/custodian.json"
  [ -z "$OBSERVER_CONFIG" ] || die "--observer-config is dev-only: protected mode reads /etc/axon/observer.json"
  HOST_CONFIG=/etc/axon/protected-host.json
  OBSERVER_CONFIG=/etc/axon/observer.json
  HELPER_CONFIG_FILE=/etc/axon/protected-launcher.json
  FABRIC_BIN=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["path"])' "$ROOT/verifier.json" 2>/dev/null) \
    || die "$ROOT/verifier.json names no installed verifier path"
fi
[ -n "$HOST_CONFIG" ] && [ -f "$HOST_CONFIG" ] || die "protected-host config ${HOST_CONFIG:-(none)} does not exist"
[ "$MODE" = protected ] || HELPER_CONFIG_FILE=$LAUNCHER_CONFIG
# The observer service's facts (amendment 68): its own config and the helper
# config's observer.service, read by the same fields axon-observer and the
# helper read.
OBS_UID="" OBS_FABRIC_UID="" OBS_CALLER_UID="" OBS_SOCKET="" OBS_STORE="" OBS_KEY="" OBS_SVC_SOCKET="" OBS_SVC_UID=""
if [ -n "$OBSERVER" ]; then
  [ -n "$OBSERVER_CONFIG" ] && [ -f "$OBSERVER_CONFIG" ] || die "observer config ${OBSERVER_CONFIG:-(none)} does not exist (dev mode: --observer-config)"
  [ -n "$HELPER_CONFIG_FILE" ] && [ -f "$HELPER_CONFIG_FILE" ] || die "the helper config ${HELPER_CONFIG_FILE:-(none)} does not exist (dev mode: --launcher-config)"
  ofacts=$(python3 -I - "$OBSERVER_CONFIG" "$HELPER_CONFIG_FILE" <<'PY'
import json, sys
o = json.load(open(sys.argv[1])); l = json.load(open(sys.argv[2]))
if o.get("schema") != "axon-observer/1": sys.exit("schema")
s = (l.get("observer") or {}).get("service") or {}
for v in (o.get("observer_uid"), o.get("fabric_uid"), o.get("caller_uid"), o.get("socket"), o.get("store"),
          o.get("key_path"), s.get("socket", "-"), s.get("uid", "-")):
    print("-" if v is None else v)
PY
) || die "the observer config $OBSERVER_CONFIG is not an axon-observer/1 config (or the helper config is unreadable)"
  { read -r OBS_UID; read -r OBS_FABRIC_UID; read -r OBS_CALLER_UID; read -r OBS_SOCKET; read -r OBS_STORE
    read -r OBS_KEY; read -r OBS_SVC_SOCKET; read -r OBS_SVC_UID; } <<<"$ofacts"
fi
[ -n "$FABRIC_BIN" ] && [ -x "$FABRIC_BIN" ] || die "axon-fabric ${FABRIC_BIN:-(none)} is not executable (dev mode: --fabric-bin)"
# The pinned paths, from the SAME list ProtectedHost::load walks.
PATHS=$(mktemp); trap 'rm -f "$PATHS"' EXIT
"$FABRIC_BIN" protected-host-paths --config "$HOST_CONFIG" ${LAUNCHER_CONFIG:+--launcher-config "$LAUNCHER_CONFIG"} \
  ${CUSTODIAN_CONFIG:+--custodian-config "$CUSTODIAN_CONFIG"} \
  >"$PATHS" 2>/dev/null || die "axon-fabric protected-host-paths refused $HOST_CONFIG (or the privileged launcher's or the custodian's config)"
while IFS=$'\t' read -r kind p; do
  case "$kind" in
    operator-file) O1+=("$p") ;;
    operator-dir) O1_DIRS+=("$p") ;;
    signing-key) [ -z "$SIGNING_KEY" ] || die "two signing keys listed"; SIGNING_KEY=$p ;;
    service-dir) SERVICE_DIRS+=("$p") ;;
    # A82: the loop's store; the directory holding it is the operator's.
    authority-store) AUTHORITY_STORES+=("$p") ;;
    privileged-helper) [ -z "$HELPER" ] || die "two privileged helpers listed"; HELPER=$p; O1+=("$p") ;;
    helper-fabric-uid) HELPER_FABRIC_UID=$p ;;
    helper-fabric-path) HELPER_FABRIC_PATH=$p ;;
    helper-fabric-sha256) HELPER_FABRIC_SHA=$p ;;
    custodian-socket) SOCKET_DIRS+=("$(dirname "$p")") ;;
    custodian-store) [ -z "$CUSTODIAN_STORE" ] || die "two custodian stores listed"; CUSTODIAN_STORE=$p ;;
    custodian-uid) CUSTODIAN_UID=$p ;;
    custodian-fabric-uid) CUSTODIAN_FABRIC_UID=$p ;;
    custodian-launcher-uid) CUSTODIAN_LAUNCHER_UID=$p ;;
    custodian-observer-uid) CUSTODIAN_OBSERVER_UID=$p ;;
    *) die "axon-fabric listed an unknown path kind $kind" ;;
  esac
done <"$PATHS"
[ -n "$SIGNING_KEY" ] && [ ${#O1[@]} -gt 0 ] || die "axon-fabric listed no signing key or no pinned file"
[ -n "$HELPER" ] && [ -n "$HELPER_FABRIC_UID" ] || die "axon-fabric listed no privileged helper (A: a protected host launches only through one)"
[ -n "$CUSTODIAN_STORE" ] && [ -n "$CUSTODIAN_UID" ] || die "axon-fabric listed no custodian (amendment 50: the nonce is the custodian's)"
case "$ROOT" in /*) ;; *) die "--root must be absolute" ;; esac
[ -d "$ROOT/qualification" ] || die "$ROOT/qualification does not exist"

# uid[:gid] → numeric uid and gid (a name resolves through the host's passwd).
resolve() {
  local u=${1%%:*} g=${1#*:}
  [ "$g" = "$1" ] && g=""
  case "$u" in *[!0-9]*) u=$(id -u "$u" 2>/dev/null) || return 1 ;; esac
  [ -n "$g" ] || g=$(getent passwd "$u" | cut -d: -f4)
  [ -n "$g" ] || g=$u
  case "$g" in *[!0-9]*) g=$(getent group "$g" | cut -d: -f3) || return 1 ;; esac
  [ "$u" != 0 ] || return 1 # root is the operator, never a service actor
  echo "$u:$g"
}
as() { # as UID:GID CMD... — the actor, with NO supplementary groups
  local ug=$1; shift
  setpriv --reuid="${ug%%:*}" --regid="${ug#*:}" --clear-groups --inh-caps=-all -- "$@" \
    >/dev/null 2>&1
}

FAILED=0
CHECKS=$(mktemp); trap 'rm -f "$CHECKS" "$PATHS"' EXIT
record() { # actor uid action target expected observed  (one TSV row each)
  [ "$5" = "$6" ] || FAILED=1
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" "${6//$'\t'/ }" >>"$CHECKS"
}

# Directories an actor must not be able to write: the whole chain from / in
# protected mode, the root and below in dev mode; plus every file.
mapfile -t DIRS < <(find "$ROOT" -xdev -type d | sort)
if [ "$MODE" = protected ]; then
  p=$ROOT
  while [ "$p" != / ]; do p=$(dirname "$p"); DIRS=("$p" "${DIRS[@]}"); done
fi
mapfile -t FILES < <(find "$ROOT" -xdev -type f | sort)
# The O1 files, the operator directories (and their direct entries), and the
# directories holding them, the signing key and each service directory.
FILES+=("${O1[@]}")
for d in "${O1_DIRS[@]}"; do
  DIRS+=("$d")
  while IFS= read -r e; do
    if [ -d "$e" ]; then DIRS+=("$e"); else FILES+=("$e"); fi
  done < <(find "$d" -mindepth 1 -maxdepth 1 | sort)
done
# The custodian's socket directory is the operator's (nobody may bind there);
# the directory ABOVE the custodian's store too.
DIRS+=("${SOCKET_DIRS[@]}")
# Amendment 68: the observer's config is an operator file, its socket's
# directory the operator's, and the directories above its key directory and
# record store too (axon-observer walks its store's parent chain, M1550).
OBS_DIRS=() OBS_CHAIN=()
if [ -n "$OBSERVER" ]; then
  FILES+=("$OBSERVER_CONFIG")
  DIRS+=("$(dirname "$OBS_SOCKET")")
  OBS_DIRS=("$(dirname "$OBS_KEY")" "$OBS_STORE")
  OBS_CHAIN=("$OBSERVER_CONFIG" "${OBS_DIRS[@]}")
fi
for f in "${O1[@]}" "${O1_DIRS[@]}" "$SIGNING_KEY" "${SERVICE_DIRS[@]}" "$CUSTODIAN_STORE" \
  "${AUTHORITY_STORES[@]}" "${OBS_CHAIN[@]}"; do
  d=$(dirname "$f"); DIRS+=("$d")
  if [ "$MODE" = protected ]; then
    while [ "$d" != / ]; do d=$(dirname "$d"); DIRS+=("$d"); done
  fi
done
mapfile -t DIRS < <(printf '%s\n' "${DIRS[@]}" | sort -u)
mapfile -t QFILES < <(find "$ROOT/qualification" -xdev -type f | sort)

cannot_modify() { # actor uid:gid [service: also the Fabric's own directories]
  local who=$1 ug=$2 d f probe
  if [ "${3:-}" = service ]; then
    # out_root / nonce_store are the Fabric UID's: nobody else creates in them
    # (a planted run or psv-inputs tree; erased nonce records) or chmods them.
    for d in "${SERVICE_DIRS[@]}"; do
      probe="$d/.axon-preflight-probe-$$"
      if as "$ug" mkdir "$probe"; then rmdir "$probe"; record "$who" "$ug" create "$d" refused SUCCEEDED
      else record "$who" "$ug" create "$d" refused refused; fi
      if as "$ug" python3 -c 'import os,sys; p=sys.argv[1]; os.chmod(p, os.stat(p).st_mode & 0o7777)' "$d"; then record "$who" "$ug" chmod "$d" refused SUCCEEDED
      else record "$who" "$ug" chmod "$d" refused refused; fi
    done
  fi
  for d in "${DIRS[@]}"; do
    probe="$d/.axon-preflight-probe-$$"
    if as "$ug" mkdir "$probe"; then rmdir "$probe"; record "$who" "$ug" create "$d" refused SUCCEEDED
    else record "$who" "$ug" create "$d" refused refused; fi
  done
  for f in "${FILES[@]}"; do
    # open read-write without writing a byte: the kernel's write permission check.
    if as "$ug" sh -c 'exec 3<>"$1"' _ "$f"; then record "$who" "$ug" open-write "$f" refused SUCCEEDED
    else record "$who" "$ug" open-write "$f" refused refused; fi
    # chmod(2) itself, re-applying the file's own mode. NOT chmod(1): GNU
    # chmod skips the syscall when the mode is unchanged and reports success,
    # so it would be no attempt at all (measured).
    if as "$ug" python3 -c 'import os,sys; p=sys.argv[1]; os.chmod(p, os.stat(p).st_mode & 0o7777)' "$f"; then record "$who" "$ug" chmod "$f" refused SUCCEEDED
    else record "$who" "$ug" chmod "$f" refused refused; fi
  done
}

# Amendment 50: nobody but the custodian creates in (or chmods) its store — a
# planted `.issued` is a nonce it never issued, an erased `.used` a nonce that
# spends twice.
cannot_touch_store() { # actor uid:gid
  local who=$1 ug=$2 probe="$CUSTODIAN_STORE/.axon-preflight-probe-$$"
  if as "$ug" mkdir "$probe"; then rmdir "$probe"; record "$who" "$ug" create "$CUSTODIAN_STORE" refused SUCCEEDED
  else record "$who" "$ug" create "$CUSTODIAN_STORE" refused refused; fi
  if as "$ug" python3 -c 'import os,sys; p=sys.argv[1]; os.chmod(p, os.stat(p).st_mode & 0o7777)' "$CUSTODIAN_STORE"; then record "$who" "$ug" chmod "$CUSTODIAN_STORE" refused SUCCEEDED
  else record "$who" "$ug" chmod "$CUSTODIAN_STORE" refused refused; fi
}

V=$(resolve "$VERIFIER") || die "verifier: not a non-root user: $VERIFIER"
C=$(resolve "$CUSTODIAN") || die "custodian: not a non-root user: $CUSTODIAN"
[ ${#QFILES[@]} -gt 0 ] || record verifier "$V" read "$ROOT/qualification" "a key to read" "empty"
for f in "${QFILES[@]}" "$ROOT/verifier.json"; do
  [ -e "$f" ] || { record verifier "$V" read "$f" read missing; continue; }
  if as "$V" cat "$f"; then record verifier "$V" read "$f" read read
  else record verifier "$V" read "$f" read refused; fi
done
if as "$V" ls "$ROOT/qualification"; then record verifier "$V" list "$ROOT/qualification" read read
else record verifier "$V" list "$ROOT/qualification" read refused; fi
cannot_modify verifier "$V" service
cannot_modify custodian "$C" service
F=$(resolve "$FABRIC") || die "fabric: not a non-root user: $FABRIC"
cannot_modify fabric "$F"
# Amendment 50: the custodian is its OWN uid — not the Fabric's, not root — and
# its config names exactly these actors.
record operator - custodian-config-uid "$CUSTODIAN_UID" "${C%%:*}" "$CUSTODIAN_UID"
cstate=separate
[ "${C%%:*}" != "${F%%:*}" ] || cstate="the Fabric's uid"
[ "$CUSTODIAN_UID" != "$CUSTODIAN_FABRIC_UID" ] || cstate="its config names the Fabric's uid as the custodian"
[ "$CUSTODIAN_UID" != 0 ] || cstate="root"
record operator - custodian-separate "$CUSTODIAN_UID" separate "$cstate"
record operator - custodian-issues-to "$CUSTODIAN_UID" "${F%%:*}" "$CUSTODIAN_FABRIC_UID"
record operator - custodian-spends-for "$CUSTODIAN_UID" 0 "$CUSTODIAN_LAUNCHER_UID"
smode=$(stat -c '%u %a %F' "$CUSTODIAN_STORE" 2>/dev/null)
read -r su sa sf <<<"$smode"
sstate=ok
[ "$sf" = directory ] || sstate="not a directory (${sf:-missing})"
[ "$su" = "${C%%:*}" ] || sstate="owner ${su:-?}, not the custodian ${C%%:*}"
[ $(( 8#${sa:-777} & 8#0077 )) -eq 0 ] || sstate="mode $sa: group/other access"
record operator - store-mode "$CUSTODIAN_STORE" ok "$sstate"
probe="$CUSTODIAN_STORE/.axon-preflight-probe-$$"
if as "$C" mkdir "$probe"; then rmdir "$probe"; record custodian "$C" create "$CUSTODIAN_STORE" created created
else record custodian "$C" create "$CUSTODIAN_STORE" created refused; fi
cannot_touch_store fabric "$F"
cannot_touch_store verifier "$V"
# A (amendment 45): the Fabric actor is not root (resolve refuses uid 0 for
# every actor), and its ONLY route to a root launch is the privileged helper.
record operator - helper-admits "$HELPER" "${F%%:*}" "$HELPER_FABRIC_UID"
# Amendments 79, 85: the helper serves a caller whose executable at that instant is
# the file its config pins (a guard against mistakes, not against same-uid code),
# so the pin must be the installed verifier (the program that runs as Fabric): its
# path, and the sha256 of its bytes.
record operator - helper-fabric-pin-path "$HELPER_CONFIG_FILE" "$FABRIC_BIN" "$HELPER_FABRIC_PATH"
record operator - helper-fabric-pin-sha256 "$HELPER_CONFIG_FILE" "$(sha256sum "$FABRIC_BIN" | cut -d' ' -f1)" "$HELPER_FABRIC_SHA"
hmode=$(stat -c '%u %g %a' "$HELPER" 2>/dev/null)
read -r hu hg ha <<<"$hmode"
hstate=ok
[ "$hu" = 0 ] || hstate="owner $hu, not root"
[ "$hg" = "${F#*:}" ] || hstate="group $hg, not the Fabric's ${F#*:}"
[ $(( 8#${ha:-0} & 8#4000 )) -ne 0 ] || hstate="mode $ha: not setuid"
[ $(( 8#${ha:-0} & 8#0022 )) -eq 0 ] || hstate="mode $ha: group/other-writable"
[ $(( 8#${ha:-0} & 8#0007 )) -eq 0 ] || hstate="mode $ha: other has access"
record operator - helper-mode "$HELPER" ok "$hstate"
probe() { # uid:gid → the helper's --probe JSON, as that actor (no supplementary groups)
  # The exec is made by a shell RUNNING as the actor: setpriv's own exec still
  # holds root's DAC override (measured: it runs a 0700 root file as uid
  # 40004), so exec'ing the helper from setpriv directly proves nothing.
  setpriv --reuid="${1%%:*}" --regid="${1#*:}" --clear-groups --inh-caps=-all -- \
    sh -c 'exec "$0" --probe' "$HELPER" 2>/dev/null
}
want_build=any; [ "$MODE" = protected ] && want_build=production
pj=$(probe "$F")
pv=$(printf '%s' "$pj" | python3 -c '
import json,sys
want, uid = sys.argv[1], sys.argv[2]
try: p = json.load(sys.stdin)
except Exception: print("no probe result"); sys.exit()
bad = []
if p.get("schema") != "axon-protected-launcher-probe/1": bad.append("schema")
if p.get("euid") != 0: bad.append("euid %s (setuid not honoured: nosuid mount, or not setuid)" % p.get("euid"))
if str(p.get("ruid")) != uid: bad.append("ruid %s" % p.get("ruid"))
if want != "any" and p.get("build") != want: bad.append("build %s" % p.get("build"))
print("; ".join(bad) or "root-through-helper")' "$want_build" "${F%%:*}" 2>/dev/null)
record fabric "$F" exec-helper "$HELPER" root-through-helper "${pv:-no probe result}"
# Amendment 65: the RUNNING Fabric is the --fabric uid and not under
# NoNewPrivileges (the kernel would ignore the helper's set-id bit for it and
# every launch would be refused). Read from the kernel's own record of it.
if [ -n "$FABRIC_PID" ]; then
  nst=$(cat "/proc/$FABRIC_PID/status" 2>/dev/null)
  nruid=$(awk '/^Uid:/{print $2}' <<<"$nst"); nnp=$(awk '/^NoNewPrivs:/{print $2}' <<<"$nst")
  nobs=ok
  if [ -z "$nst" ]; then nobs="no process $FABRIC_PID"
  elif [ "$nruid" != "${F%%:*}" ]; then nobs="pid $FABRIC_PID runs as uid $nruid, not the Fabric's ${F%%:*}"
  elif [ "$nnp" != 0 ]; then nobs="NoNewPrivs ${nnp:-unknown}: the kernel ignores the helper's set-id bit for this Fabric"
  fi
  record fabric "$F" no-new-privs "/proc/$FABRIC_PID" ok "$nobs"
fi
for who in "verifier:$V" "custodian:$C"; do
  if probe "${who#*:}" >/dev/null; then record "${who%%:*}" "${who#*:}" exec-helper "$HELPER" refused SUCCEEDED
  else record "${who%%:*}" "${who#*:}" exec-helper "$HELPER" refused refused; fi
done
# A20: the signing key is readable by the Fabric UID and by NO ONE else.
reads() { as "$1" sh -c 'exec 3<"$1"' _ "$SIGNING_KEY"; }
if reads "$F"; then record fabric "$F" read-key "$SIGNING_KEY" read read
else record fabric "$F" read-key "$SIGNING_KEY" read refused; fi
for who in "verifier:$V" "custodian:$C"; do
  if reads "${who#*:}"; then record "${who%%:*}" "${who#*:}" read-key "$SIGNING_KEY" refused SUCCEEDED
  else record "${who%%:*}" "${who#*:}" read-key "$SIGNING_KEY" refused refused; fi
done
for a in "${AGENTS[@]}"; do
  A=$(resolve "$a") || die "agent: not a non-root user: $a"
  cannot_modify "agent:$a" "$A" service
  cannot_touch_store "agent:$a" "$A"
  if probe "$A" >/dev/null; then record "agent:$a" "$A" exec-helper "$HELPER" refused SUCCEEDED
  else record "agent:$a" "$A" exec-helper "$HELPER" refused refused; fi
  if reads "$A"; then record "agent:$a" "$A" read-key "$SIGNING_KEY" refused SUCCEEDED
  else record "agent:$a" "$A" read-key "$SIGNING_KEY" refused refused; fi
done

# Amendment 68: the observer SERVICE.
if [ -n "$OBSERVER" ]; then
  O=$(resolve "$OBSERVER") || die "observer: not a non-root user: $OBSERVER"
  record operator - observer-config-uid "$OBSERVER_CONFIG" "${O%%:*}" "$OBS_UID"
  record operator - observer-config-fabric "$OBSERVER_CONFIG" "${F%%:*}" "$OBS_FABRIC_UID"
  record operator - observer-config-caller "$OBSERVER_CONFIG" 0 "$OBS_CALLER_UID"
  record operator - helper-observer-socket "$HELPER_CONFIG_FILE" "$OBS_SOCKET" "$OBS_SVC_SOCKET"
  record operator - helper-observer-uid "$HELPER_CONFIG_FILE" "${O%%:*}" "$OBS_SVC_UID"
  # Amendment 79: the custodian answers the observer's `check` for the
  # observer's uid, and for no other.
  record operator - custodian-observer-uid "${CUSTODIAN_CONFIG:-/etc/axon/custodian.json}" "${O%%:*}" "$CUSTODIAN_OBSERVER_UID"
  ostate=separate
  [ "${O%%:*}" != "${F%%:*}" ] || ostate="the Fabric's uid"
  [ "${O%%:*}" != "${C%%:*}" ] || ostate="the custodian's uid"
  [ "${O%%:*}" != "${V%%:*}" ] || ostate="the verifier's uid"
  for a in "${AGENTS[@]}"; do
    A=$(resolve "$a") || die "agent: not a non-root user: $a"
    [ "${O%%:*}" != "${A%%:*}" ] || ostate="agent $a's uid"
  done
  record operator - observer-separate "${O%%:*}" separate "$ostate"
  # The key: a regular file the observer owns, 0400, in its own 0700 directory.
  kst=$(stat -c '%u %a %F' -- "$OBS_KEY" 2>/dev/null); read -r ku ka kf <<<"$kst"
  kstate=ok
  [ ! -L "$OBS_KEY" ] || kf="symbolic link"
  [ "$kf" = "regular file" ] || kstate="not a regular file (${kf:-missing})"
  [ "$ku" = "${O%%:*}" ] || kstate="owner ${ku:-?}, not the observer ${O%%:*}"
  [ $(( 8#${ka:-777} & 8#0277 )) -eq 0 ] || kstate="mode $ka: not 0400"
  record operator - observer-key-mode "$OBS_KEY" ok "$kstate"
  for d in "${OBS_DIRS[@]}"; do
    dst=$(stat -c '%u %a %F' -- "$d" 2>/dev/null); read -r du da df <<<"$dst"
    dstate=ok
    [ "$df" = directory ] || dstate="not a directory (${df:-missing})"
    [ "$du" = "${O%%:*}" ] || dstate="owner ${du:-?}, not the observer ${O%%:*}"
    [ $(( 8#${da:-777} & 8#0077 )) -eq 0 ] || dstate="mode $da: group/other access"
    record operator - observer-dir-mode "$d" ok "$dstate"
  done
  # Real attempts: the observer CAN open its key and create in its store;
  # every other actor can do neither, and cannot connect to its socket.
  okey() { as "$1" sh -c 'exec 3<"$1"' _ "$OBS_KEY"; }
  oconnect() { # uid:gid -> denied | connected | missing | error:<kind>
    setpriv --reuid="${1%%:*}" --regid="${1#*:}" --clear-groups --inh-caps=-all -- python3 -I -c '
import socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(5)
try:
    s.connect(sys.argv[1]); print("connected")
except PermissionError: print("denied")
except FileNotFoundError: print("missing")
except Exception as e: print("error:" + type(e).__name__)' "$OBS_SOCKET" 2>/dev/null
  }
  if okey "$O"; then record observer "$O" read-observer-key "$OBS_KEY" read read
  else record observer "$O" read-observer-key "$OBS_KEY" read refused; fi
  probe="$OBS_STORE/.axon-preflight-probe-$$"
  if as "$O" mkdir "$probe"; then rmdir "$probe"; record observer "$O" create "$OBS_STORE" created created
  else record observer "$O" create "$OBS_STORE" created refused; fi
  onames=(fabric verifier custodian) ougs=("$F" "$V" "$C")
  for a in "${AGENTS[@]}"; do onames+=("agent:$a"); ougs+=("$(resolve "$a")"); done
  for i in "${!onames[@]}"; do
    n=${onames[$i]} ug=${ougs[$i]}
    if okey "$ug"; then record "$n" "$ug" read-observer-key "$OBS_KEY" refused SUCCEEDED
    else record "$n" "$ug" read-observer-key "$OBS_KEY" refused refused; fi
    for d in "${OBS_DIRS[@]}"; do
      probe="$d/.axon-preflight-probe-$$"
      if as "$ug" mkdir "$probe"; then rmdir "$probe"; record "$n" "$ug" create "$d" refused SUCCEEDED
      else record "$n" "$ug" create "$d" refused refused; fi
      if as "$ug" python3 -c 'import os,sys; p=sys.argv[1]; os.chmod(p, os.stat(p).st_mode & 0o7777)' "$d"; then record "$n" "$ug" chmod "$d" refused SUCCEEDED
      else record "$n" "$ug" chmod "$d" refused refused; fi
    done
    record "$n" "$ug" connect-observer "$OBS_SOCKET" denied "$(oconnect "$ug")"
  done
fi

# The candidate guest: the probe runs INSIDE it and must find nothing to address.
g=$(bash -c "$GUEST" 2>/dev/null | tail -n 1)
if printf '%s' "$g" | python3 -c 'import json,sys; v=json.load(sys.stdin); sys.exit(0 if v.get("schema")=="axon-trust-guest-probe/1" and v.get("addressable") is False else 1)' 2>/dev/null
then record guest - address "$OPERATOR_TRUST_ROOT" unaddressable unaddressable
else record guest - address "$OPERATOR_TRUST_ROOT" unaddressable "${g:-no probe result}"; fi

verdict=PASS; [ $FAILED = 0 ] || verdict=FAIL
report=$(python3 - "$CHECKS" "$SCHEMA" "$MODE" "$ROOT" "$(hostname)" "$verdict" \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" <<'PY'
import json, sys
f, schema, mode, root, host, verdict, at = sys.argv[1:]
keys = ("actor", "uid", "action", "target", "expected", "observed")
checks = [dict(zip(keys, l.rstrip("\n").split("\t"))) for l in open(f) if l.strip()]
for c in checks:
    c["ok"] = c["expected"] == c["observed"]
print(json.dumps({"schema": schema, "mode": mode, "root": root, "host": host,
                  "verdict": verdict, "at": at, "checks": checks}, indent=1))
PY
)
[ -n "$OUT" ] && printf '%s\n' "$report" >"$OUT"
printf '%s\n' "$report"
[ $FAILED = 0 ]
