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
#   …and all of this also over every path the protected-host config pins (O1)
#   guest      cannot even ADDRESS the root (--guest-cmd runs
#              trust_root_guest_probe.sh inside the candidate guest)
#
# Every write attempt is NON-DESTRUCTIVE if it unexpectedly succeeds: a probe
# directory is created and removed, a file is opened read-write without being
# written, chmod re-applies the file's own mode. A success is a FAIL either way.
#
# Modes:
#   protected  root is fixed at /etc/axon/trust, and every ancestor from / is
#              probed too. The only mode readiness accepts
#              (readiness.rs: TRUST_PREFLIGHT_SCHEMA, mode "protected").
#   dev        --root DIR (a fixture); only the root and below are probed.
#              Proves the mechanism, certifies nothing.
#
# Usage (as root, which is needed to switch UID — never as the actors):
#   trust_root_preflight.sh --verifier UID[:GID] --custodian UID[:GID] --fabric UID[:GID] \
#       --agent UID[:GID] [--agent …] --guest-cmd 'CMD' \
#       [--root DIR --host-config FILE] [--out FILE]
#
# Exit 0 = PASS, 1 = FAIL (a refusal did not happen), 2 = cannot run (usage,
# not root, root missing) — never a pass.
set -uo pipefail

OPERATOR_TRUST_ROOT=/etc/axon/trust
SCHEMA=axon-trust-preflight/1
ROOT="" OUT="" GUEST="" VERIFIER="" CUSTODIAN="" FABRIC="" HOST_CONFIG="" SIGNING_KEY=""
O1=()
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
    --host-config) HOST_CONFIG="$2"; shift 2 ;;
    --agent) AGENTS+=("$2"); shift 2 ;;
    *) die "unknown argument $1" ;;
  esac
done
[ "$(id -u)" = 0 ] || die "must run as root to act as each service UID"
command -v setpriv >/dev/null || die "setpriv is required"
[ -n "$VERIFIER" ] && [ -n "$CUSTODIAN" ] && [ -n "$FABRIC" ] && [ ${#AGENTS[@]} -gt 0 ] && [ -n "$GUEST" ] \
  || die "--verifier, --custodian, --fabric, at least one --agent and --guest-cmd are required"
if [ -n "$ROOT" ]; then MODE=dev; else MODE=protected; ROOT=$OPERATOR_TRUST_ROOT; fi
# O1 (v022-psv-protocol.md §2): the protected-host config and every path it
# pins are operator authority too, and its signing key is the Fabric UID's
# alone. Protected mode reads the fixed file; dev mode needs --host-config.
if [ "$MODE" = protected ]; then
  [ -z "$HOST_CONFIG" ] || die "--host-config is dev-only: protected mode reads /etc/axon/protected-host.json"
  HOST_CONFIG=/etc/axon/protected-host.json
fi
[ -n "$HOST_CONFIG" ] && [ -f "$HOST_CONFIG" ] || die "protected-host config ${HOST_CONFIG:-(none)} does not exist"
mapfile -t HC < <(python3 - "$HOST_CONFIG" <<'PY'
import json, sys
c = json.load(open(sys.argv[1]))
print(c["signer"]["key_path"])
for p in (sys.argv[1], c["launcher"]["path"], c["profile_manifest"]["path"],
          c["suite_registry"]["path"], c["qualification"]["record"]):
    print(p)
for k in ("signature", "waivers"):
    if c["qualification"].get(k):
        print(c["qualification"][k])
PY
) || die "protected-host config is not readable as axon-protected-host/1"
SIGNING_KEY=${HC[0]}
O1=("${HC[@]:1}")
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
CHECKS=$(mktemp); trap 'rm -f "$CHECKS"' EXIT
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
# The O1 files, and the directories holding them and the signing key.
FILES+=("${O1[@]}")
for f in "${O1[@]}" "$SIGNING_KEY"; do
  d=$(dirname "$f"); DIRS+=("$d")
  if [ "$MODE" = protected ]; then
    while [ "$d" != / ]; do d=$(dirname "$d"); DIRS+=("$d"); done
  fi
done
mapfile -t DIRS < <(printf '%s\n' "${DIRS[@]}" | sort -u)
mapfile -t QFILES < <(find "$ROOT/qualification" -xdev -type f | sort)

cannot_modify() { # actor uid:gid
  local who=$1 ug=$2 d f probe
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
cannot_modify verifier "$V"
cannot_modify custodian "$C"
F=$(resolve "$FABRIC") || die "fabric: not a non-root user: $FABRIC"
cannot_modify fabric "$F"
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
  cannot_modify "agent:$a" "$A"
  if reads "$A"; then record "agent:$a" "$A" read-key "$SIGNING_KEY" refused SUCCEEDED
  else record "agent:$a" "$A" read-key "$SIGNING_KEY" refused refused; fi
done

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
