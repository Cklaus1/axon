#!/usr/bin/env bash
# operator_deploy_protected_host.sh — the OPERATOR deployment kit for the Axon
# v0.22 protected host (Candidate C9). DRY-RUN BY DEFAULT.
#
# It installs, from ONE standalone clone at ONE named commit, everything the
# protected host needs that is not a key or a signature:
#
#   allowlist  /etc/axon/provenance-allowlist (amendment 44, decision C)
#   users      the Fabric, custodian, observer, verifier and launch-profile
#              system users (own uid and group each, no login; amendments 45,
#              50, 68)
#   dirs       out root, root-private staging root, custodian store, observer
#              record store and key directory (the observer uid's, 0700),
#              authority store parent, /etc/axon layout, install dirs
#   binaries   axon-fabric (the readiness verifier and Fabric), the setuid-root
#              axon-protected-launcher (04750 root:<fabric group>), axon-custodian,
#              axon-observer (the observer SERVICE, amendment 68), fc_linux_profile.sh
#   guest      vmlinux, rootfs.sqfs and the profile manifest (amendments 54, 56, 63)
#   data       the operator suite registry, grant registry and grant files; the
#              signed B263 record and waivers, when the operator hands them over
#   configs    /etc/axon/custodian.json (axon-custodian/1),
#              /etc/axon/observer.json (axon-observer/1, amendment 68),
#              /etc/axon/protected-launcher.json (axon-protected-launcher/2),
#              /etc/axon/protected-host.json (axon-protected-host/1), every pin
#              computed from the INSTALLED bytes — including the helper config's
#              custodian.sha256, the axon-custodian PROGRAM pin (amendment 65),
#              and its observer.service {socket, uid, sha256}, the axon-observer
#              PROGRAM pin (amendment 68), and its fabric {path, sha256,
#              revision}, the installed axon-fabric PROGRAM pin (amendment 79:
#              the helper serves, and the observer names, only that program;
#              the revision is READ FROM THE INSTALLED FILE by running its own
#              `verifier-manifest`, never typed). The host config's observer
#              section names NO command: a production Fabric refuses one
#   verifier   /etc/axon/trust/verifier.json from the installed verifier's own
#              `verifier-manifest`
#   systemd    axon-custodian.socket + .service and axon-observer.socket +
#              .service, sockets enabled
#   loader     the production ProtectedHost::operator() run as the Fabric uid
#              against what was just deployed (a read; it launches nothing)
#   toolchain  /etc/axon/host-toolchain-pin.json: the host tool identities the
#              guest build recorded (amendment 63's operator item). READ by the
#              freeze since amendment 65: v022_freeze_manifest.py refuses without
#              it (guest_build_env.image_problems, pin_required=True); the kit
#              judges what it installed with the same reader
#   fabric-unit the operator's unit that runs Fabric (--fabric-unit): it must
#              not set NoNewPrivileges or anything that implies it or otherwise
#              keeps the setuid helper from becoming root (amendment 65); its
#              MainPID is what the preflight's --fabric-pid judges
#   check      the trust roots, the attestation key and the observer's key:
#              present, owned, moded, separated — CHECKED, never created; the
#              helper config's custodian and observer program pins against the
#              INSTALLED axon-custodian and axon-observer, and the observer's
#              socket, uid and config agreeing across observer.json, the helper
#              config and the units (a mismatch FAILS); a kernel with
#              SO_PASSPIDFD (Linux >= 6.5)
#   preflight  scripts/trust_root_preflight.sh in protected mode, with
#              --fabric-pid (the running Fabric's pid) and --observer (the
#              observer uid: its key readable by it alone, its socket
#              connectable by no actor); its verdict
#
# It NEVER generates, reads the private half of, or signs with any key. Where a
# key or trust root must be provisioned it checks it and stops with the exact
# instruction.
#
# Usage (run the copy of this script INSIDE the clone you deploy from):
#   operator_deploy_protected_host.sh --from CLONE --bin-dir DIR [options]          (dry run)
#   sudo operator_deploy_protected_host.sh --from CLONE --bin-dir DIR [options] --apply
#
#   --from CLONE            standalone clone (a real .git directory, not a linked
#                           worktree), clean except target/ and dist/, at the commit
#                           to deploy. Its HEAD is recorded.
#   --expect-commit SHA     refuse unless the clone's HEAD is exactly SHA (40 hex)
#   --bin-dir DIR           where `cargo build --release --locked -p axon-fabric --bins`
#                           (production: no test-trust feature) put axon-fabric,
#                           axon-protected-launcher, axon-custodian and
#                           axon-observer, built FROM CLONE at its HEAD (checked
#                           through the verifier's own report)
#   --observer-bin, --observer-interpreter   REFUSED since amendment 68: the
#                           observer is the axon-observer service from --bin-dir;
#                           a production Fabric refuses an in-uid observer program
#   --suite-registry FILE   the operator suite registry (cortex-check-registry/1);
#                           every relative path it names is installed beside it
#   --grant-registry FILE   the operator grant registry (axon-fabric-grant-registry/1)
#                           and the grant files it names
#   --signer-public-key HEX the Fabric attestation key's public half (what
#                           `axon-fabric keygen` printed); never the key itself
#   --signer-key PATH       where the operator put that key (default
#                           /etc/axon/keys/fabric-attest.pk8); checked, never read
#   --signer-issuer-ref REF default verifier:fabric
#   --authority-store PATH  the axon-loop store Fabric reads the epoch from
#                           (default /var/lib/axon-loop/store)
#   --b263-record FILE      an operator-signed axon-b263-evidence/1 (FILE.sig beside it)
#   --b263-waivers FILE     its operator-signed axon-b263-waiver/1 (FILE.sig beside it)
#   --qualification-max-age-s N   default 2592000 (Fabric's own default, materialised)
#   --max-timeout-s N       helper launch timeout ceiling (default 900)
#   --max-input-bytes N     helper input snapshot ceiling (default 268435456)
#   --fabric-user U --custodian-user U --observer-user U --verifier-user U --profile-user U
#                           defaults axon-fabric, axon-custodian, axon-observer,
#                           axon-verifier, axonb263 (five different users)
#   --agent U               an agent uid/user for the preflight (repeatable)
#   --guest-cmd CMD         the preflight's in-guest probe command
#   --fabric-unit UNIT      the systemd unit that runs Fabric as the Fabric uid
#                           (a unit name, judged through `systemctl cat`, or an
#                           absolute path to a unit file, judged as written)
#   --fabric-pid PID        the running Fabric's pid for the preflight (default:
#                           `systemctl show -p MainPID --value UNIT`)
#   --only STEP[,STEP]      run only these steps (names above)
#   --no-systemctl          print the systemctl commands instead of running them
#   --apply                 perform the plan (root only)
#
# Exit: 0 the plan is complete and (with --apply) performed, preflight PASS;
#       1 an action or the preflight FAILED; 2 refused (usage, not root, a dirty
#       or linked clone, binaries not built from it); 3 stopped or incomplete:
#       a BLOCKED item (a key/trust root to provision) or a PENDING one (B263
#       record, preflight inputs) — the summary says which and what to do.
set -uo pipefail
umask 022
export LC_ALL=C PATH=/usr/sbin:/usr/bin:/sbin:/bin

# ── fixed by the code (do not change without changing the code) ───────────────
ETC=/etc/axon
TRUST=$ETC/trust
ALLOWLIST=$ETC/provenance-allowlist
HOST_CONFIG=$ETC/protected-host.json
HELPER_CONFIG=$ETC/protected-launcher.json
CUSTODIAN_CONFIG=$ETC/custodian.json
FIRECRACKER=/usr/local/bin/firecracker
JAILER=/usr/local/bin/jailer
# ── layout (matches profiles/protected-host/*.example and the systemd units) ──
LIBEXEC=/usr/local/libexec/axon
GUEST_DIR=/usr/local/lib/axon/guest-linux
FABRIC_STATE=/var/lib/axon-fabric
OUT_ROOT=$FABRIC_STATE/runs
STAGING=/var/lib/axon-protected-launcher
CUST_STATE=/var/lib/axon-custodian
STORE=$CUST_STATE/nonces
SOCKET_DIR=/run/axon-custodian
SOCKET=$SOCKET_DIR/custodian.sock
# The observer service (amendment 68; profiles/protected-host/observer.json.example
# and systemd/axon-observer.*): its own uid, key, record store and socket.
OBSERVER_CONFIG=$ETC/observer.json
OBS_STATE=/var/lib/axon-observer
OBS_STORE=$OBS_STATE/observed
OBS_KEY_DIR=$OBS_STATE/key
OBS_KEY=$OBS_KEY_DIR/observer.pk8
OBS_SOCKET_DIR=/run/axon-observer
OBS_SOCKET=$OBS_SOCKET_DIR/observer.sock
QUAL_DIR=$ETC/qualification
KEYS_DIR=$ETC/keys
SUITES_DIR=$ETC/suites
GRANTS_DIR=$ETC/grants
TOOLCHAIN_PIN=$ETC/host-toolchain-pin.json
DEPLOY_LOG=/var/lib/axon-deploy
UNIT_DIR=/etc/systemd/system
OBS_MAX_AGE_S=300
NONCE_MAX_AGE_S=300

STEPS="allowlist users dirs binaries guest data configs verifier systemd loader toolchain fabric-unit check preflight"
CLONE="" EXPECT="" BIN_DIR="" SUITE_REG="" GRANT_REG=""
SIGNER_PUB="" SIGNER_KEY=$KEYS_DIR/fabric-attest.pk8 ISSUER_REF="verifier:fabric"
AUTH_STORE=/var/lib/axon-loop/store B263_RECORD="" B263_WAIVERS=""
QUAL_MAX_AGE=2592000 MAX_TIMEOUT=900 MAX_INPUT=268435456
FABRIC_USER=axon-fabric CUSTODIAN_USER=axon-custodian OBSERVER_USER=axon-observer VERIFIER_USER=axon-verifier PROFILE_USER=axonb263
AGENTS=() GUEST_CMD="" ONLY="" NO_SYSTEMCTL=0 APPLY=0 FABRIC_UNIT="" FABRIC_PID=""

refuse() { echo "REFUSED: $*" >&2; echo "operator_deploy: REFUSED — $*"; exit 2; }
need_arg() { [ $# -ge 2 ] && [ -n "$2" ] || refuse "$1 needs a value"; }
while [ $# -gt 0 ]; do
  case "$1" in
    --from) need_arg "$@"; CLONE=$2; shift 2 ;;
    --expect-commit) need_arg "$@"; EXPECT=$2; shift 2 ;;
    --bin-dir) need_arg "$@"; BIN_DIR=$2; shift 2 ;;
    --observer-bin|--observer-interpreter)
      refuse "$1: since amendment 68 the observer is the axon-observer SERVICE (its own uid and key, socket-activated, reached only through the root helper's --observe relay), installed from --bin-dir; an observer PROGRAM runs as the Fabric uid, which could read its key, and a production Fabric refuses a host config naming one (observer.command)" ;;
    --suite-registry) need_arg "$@"; SUITE_REG=$2; shift 2 ;;
    --grant-registry) need_arg "$@"; GRANT_REG=$2; shift 2 ;;
    --signer-public-key) need_arg "$@"; SIGNER_PUB=$2; shift 2 ;;
    --signer-key) need_arg "$@"; SIGNER_KEY=$2; shift 2 ;;
    --signer-issuer-ref) need_arg "$@"; ISSUER_REF=$2; shift 2 ;;
    --authority-store) need_arg "$@"; AUTH_STORE=$2; shift 2 ;;
    --b263-record) need_arg "$@"; B263_RECORD=$2; shift 2 ;;
    --b263-waivers) need_arg "$@"; B263_WAIVERS=$2; shift 2 ;;
    --qualification-max-age-s) need_arg "$@"; QUAL_MAX_AGE=$2; shift 2 ;;
    --max-timeout-s) need_arg "$@"; MAX_TIMEOUT=$2; shift 2 ;;
    --max-input-bytes) need_arg "$@"; MAX_INPUT=$2; shift 2 ;;
    --fabric-user) need_arg "$@"; FABRIC_USER=$2; shift 2 ;;
    --custodian-user) need_arg "$@"; CUSTODIAN_USER=$2; shift 2 ;;
    --observer-user) need_arg "$@"; OBSERVER_USER=$2; shift 2 ;;
    --verifier-user) need_arg "$@"; VERIFIER_USER=$2; shift 2 ;;
    --profile-user) need_arg "$@"; PROFILE_USER=$2; shift 2 ;;
    --agent) need_arg "$@"; AGENTS+=("$2"); shift 2 ;;
    --guest-cmd) need_arg "$@"; GUEST_CMD=$2; shift 2 ;;
    --fabric-unit) need_arg "$@"; FABRIC_UNIT=$2; shift 2 ;;
    --fabric-pid) need_arg "$@"; FABRIC_PID=$2; shift 2 ;;
    --only) need_arg "$@"; ONLY=${2//,/ }; shift 2 ;;
    --no-systemctl) NO_SYSTEMCTL=1; shift ;;
    --apply) APPLY=1; shift ;;
    -h|--help) sed -n '2,/^set -uo/p' "$0" | sed '$d'; exit 0 ;;
    *) refuse "unknown argument $1 (see --help)" ;;
  esac
done
for s in $ONLY; do case " $STEPS " in *" $s "*) ;; *) refuse "--only: unknown step $s (steps: $STEPS)" ;; esac; done
selected() { [ -z "$ONLY" ] || case " $ONLY " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }
for n in "$QUAL_MAX_AGE" "$MAX_TIMEOUT" "$MAX_INPUT"; do
  case "$n" in ''|*[!0-9]*|0) refuse "a numeric option is not a positive integer: $n" ;; esac
done
for u in "$FABRIC_USER" "$CUSTODIAN_USER" "$OBSERVER_USER" "$VERIFIER_USER" "$PROFILE_USER"; do
  case "$u" in ''|*[!a-z0-9_-]*|[!a-z_]*) refuse "user name $u is not a plain system user name" ;; esac
done
# Amendment 68: the observer is its own principal; its key must not be
# readable by the Fabric (or the custodian whose nonces it observes).
[ "$(printf '%s\n' "$FABRIC_USER" "$CUSTODIAN_USER" "$OBSERVER_USER" "$VERIFIER_USER" "$PROFILE_USER" | sort -u | wc -l)" = 5 ] \
  || refuse "the Fabric, custodian, observer, verifier and profile users must be five different users"
if [ -n "$SIGNER_PUB" ]; then
  SIGNER_PUB=$(printf '%s' "$SIGNER_PUB" | tr 'A-F' 'a-f')
  [[ "$SIGNER_PUB" =~ ^[0-9a-f]{64}$ ]] || refuse "--signer-public-key is not 64 hex characters"
fi
case "$ISSUER_REF" in *[[:space:]]*|'') refuse "--signer-issuer-ref must be one word" ;; esac
case "$AUTH_STORE" in /*) ;; *) refuse "--authority-store must be absolute" ;; esac
case "$FABRIC_PID" in ''|[1-9]|[1-9]*[0-9]) ;; *) refuse "--fabric-pid must be a pid" ;; esac
case "$FABRIC_PID" in *[!0-9]*) refuse "--fabric-pid must be a pid" ;; esac
case "$FABRIC_UNIT" in
  ''|/*) ;;
  *[!A-Za-z0-9_.@:-]*|.*) refuse "--fabric-unit must be a unit name (NAME.service) or an absolute unit file path" ;;
  *.service) ;;
  *) refuse "--fabric-unit must be a unit name (NAME.service) or an absolute unit file path" ;;
esac
case "$SIGNER_KEY" in /*) ;; *) refuse "--signer-key must be absolute" ;; esac
[ $APPLY = 0 ] || [ "$(id -u)" = 0 ] || refuse "--apply must run as root (it installs root-owned and setuid files)"

MODE=DRY-RUN; [ $APPLY = 1 ] && MODE=APPLY
WORK=$(mktemp -d "${TMPDIR:-/tmp}/axon-deploy.XXXXXX") || refuse "cannot create a work directory"
trap 'rm -rf "$WORK"' EXIT
BLOCKED=() PENDING=() FAILED=0
blocked() { BLOCKED+=("$1"); echo "BLOCKED[$STEP] $1"; }
pending() { PENDING+=("$1"); echo "PENDING[$STEP] $1"; }
note() { echo "NOTE[$STEP] $*"; }
sha() { sha256sum -- "$1" | cut -d' ' -f1; }
uid_of() { getent passwd "$1" | cut -d: -f3; }
gid_of() { getent group "$1" | cut -d: -f3; }
GIT=/usr/bin/git
# The same hardening as git_data.rs's one git (no system/global config, no
# replace objects, no fsmonitor/hooks/excludesFile, any owner).
git_h() { env -i PATH=/usr/bin:/bin LC_ALL=C HOME=/nonexistent GIT_CONFIG_NOSYSTEM=1 \
  GIT_CONFIG_GLOBAL=/dev/null GIT_NO_REPLACE_OBJECTS=1 GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0 \
  GIT_NO_LAZY_FETCH=1 "$GIT" --no-replace-objects -c protocol.allow=never -c core.fsmonitor= \
  -c core.hooksPath=/dev/null -c core.untrackedCache=false -c core.excludesFile=/dev/null \
  -c core.attributesFile=/dev/null -c safe.directory='*' -C "$CLONE" "$@"; }

echo "operator_deploy: $MODE — $(date -u +%Y-%m-%dT%H:%M:%SZ) on $(hostname)"
[ $APPLY = 1 ] || echo "operator_deploy: nothing is written; every action below is what --apply would do"

# ── the clone: standalone, clean, at one commit (decisions C and E) ───────────
STEP=clone
[ -n "$CLONE" ] || refuse "--from CLONE is required"
[ -x "$GIT" ] || refuse "$GIT is required (the hardened git Axon's provenance uses)"
CLONE=$(cd "$CLONE" 2>/dev/null && pwd -P) || refuse "--from: not a directory"
if [ -L "$CLONE/.git" ] || [ ! -d "$CLONE/.git" ]; then
  refuse "$CLONE/.git is not a real directory: a linked worktree (gitfile) or a symlinked .git is never deployed from (decision E; build from a standalone clone)"
fi
top=$(git_h rev-parse --show-toplevel 2>/dev/null) || refuse "$CLONE is not a git checkout"
[ "$top" = "$CLONE" ] || refuse "$CLONE is not the top of its checkout ($top)"
gd=$(git_h rev-parse --path-format=absolute --git-dir 2>/dev/null)
cd_=$(git_h rev-parse --path-format=absolute --git-common-dir 2>/dev/null)
[ "$gd" = "$CLONE/.git" ] && [ "$cd_" = "$CLONE/.git" ] || refuse "$CLONE's git dir is not its own .git (a linked worktree)"
[ ! -e "$CLONE/.git/objects/info/alternates" ] || refuse "$CLONE borrows objects (objects/info/alternates): clone without --shared/--reference"
COMMIT=$(git_h rev-parse --verify 'HEAD^{commit}' 2>/dev/null) || refuse "$CLONE has no HEAD commit"
[ -z "$EXPECT" ] || [ "$COMMIT" = "$EXPECT" ] || refuse "$CLONE is at $COMMIT, not --expect-commit $EXPECT"
# Every object counts, git-ignored or not, except the two directories the
# allowlist excuses (decision C). The authoritative judge is axon-provenance
# (below, once the allowlist is installed); this is the same question asked of
# git so a dirty clone is refused before anything is planned.
dirty=$(git_h status --porcelain=v1 -z --untracked-files=normal --ignored=traditional 2>/dev/null \
  | tr '\0' '\n' | grep -v -E '^!! (target|dist)/' || true)
[ -z "$dirty" ] || refuse "$CLONE is dirty (decision C: only target/ and dist/ may be excused):
$dirty"
SELF=$(cd "$(dirname "$0")" && pwd -P)/$(basename "$0")
cmp -s "$SELF" "$CLONE/scripts/operator_deploy_protected_host.sh" \
  || refuse "this kit ($SELF) is not the clone's own scripts/operator_deploy_protected_host.sh: run the kit at the commit you deploy"
echo "CLONE $CLONE commit $COMMIT (standalone, clean)"

# ── the binaries: production release builds of THIS commit ───────────────────
FABRIC_SRC="" HELPER_SRC="" CUST_SRC="" OBS_SRC=""
need_bins=0
for s in binaries configs verifier loader check preflight; do selected "$s" && need_bins=1; done
if [ $need_bins = 1 ]; then
  [ -n "$BIN_DIR" ] || refuse "--bin-dir DIR is required (the release build of axon-fabric's bins from $CLONE)"
  BIN_DIR=$(cd "$BIN_DIR" 2>/dev/null && pwd -P) || refuse "--bin-dir: not a directory"
  FABRIC_SRC=$BIN_DIR/axon-fabric HELPER_SRC=$BIN_DIR/axon-protected-launcher CUST_SRC=$BIN_DIR/axon-custodian
  OBS_SRC=$BIN_DIR/axon-observer
  for b in "$FABRIC_SRC" "$HELPER_SRC" "$CUST_SRC" "$OBS_SRC"; do
    [ -f "$b" ] && [ -x "$b" ] || refuse "$b is not an executable file"
  done
  vm=$("$FABRIC_SRC" verifier-manifest 2>/dev/null) || refuse "$FABRIC_SRC verifier-manifest failed"
  why=$(printf '%s' "$vm" | python3 -I -c '
import json, sys
m, want = json.load(sys.stdin), sys.argv[1]
bad = [f"{k}={m.get(k)!r}" for k, v in (("build", "production"), ("profile", "release"),
       ("source_dirty", False), ("fabric_revision", want), ("build_state", "")) if m.get(k) != v]
print("; ".join(bad))' "$COMMIT") || refuse "cannot read $FABRIC_SRC's verifier-manifest"
  [ -z "$why" ] || refuse "$FABRIC_SRC is not a clean production release build of $COMMIT ($why)"
  # Round 5 (amendment 80, FIELD-ORIGIN): the host binaries have no constructed
  # build environment, so the ambient one is judged by the guest build's own
  # classifier -- no compiler/wrapper/flag/linker variable, and an effective cargo
  # config from the clone that is only its committed one -- and the verifier's
  # self-report above says build.rs saw no wrapper, rustflags or linker.
  hb=$(python3 -I -B "$CLONE/scripts/guest_build_env.py" check-host-build "$CLONE" 2>&1) \
    || refuse "the host binaries' ambient build environment is not the tree's own: $hb"
  pb=$("$HELPER_SRC" --probe 2>/dev/null | python3 -I -c 'import json,sys; print(json.load(sys.stdin).get("build"))' 2>/dev/null)
  [ "$pb" = production ] || refuse "$HELPER_SRC --probe reports build ${pb:-none}, not production (a test-trust helper accepts a caller's --test-config)"
  # A production custodian has no --test-config (it answers with its usage).
  co=$("$CUST_SRC" --test-config /nonexistent 2>&1)
  case "$co" in *"usage: axon-custodian"*) ;; *) refuse "$CUST_SRC accepts --test-config: a test-trust build ($co)" ;; esac
  # Nor has a production observer (M1548: it reads only /etc/axon/observer.json).
  oo=$("$OBS_SRC" --test-config /nonexistent 2>&1)
  case "$oo" in *"usage: axon-observer"*) ;; *) refuse "$OBS_SRC accepts --test-config: a test-trust build ($oo)" ;; esac
  echo "BINARIES $BIN_DIR: axon-fabric $(sha "$FABRIC_SRC"), helper $(sha "$HELPER_SRC"), custodian $(sha "$CUST_SRC"), observer $(sha "$OBS_SRC") (production release of $COMMIT)"
fi
MANIFEST_SRC=$CLONE/dist/guest-linux/manifest.json
LAUNCHER_SRC=$CLONE/scripts/fc_linux_profile.sh
BASH_PIN=$(readlink -f /usr/bin/bash)

# ── action primitives: print in a dry run, perform with --apply, idempotent ───
# A path that exists as something else (a symlink, another type) is refused,
# never replaced: that is an operator decision, not the kit's.
stat_ugm() { stat -c '%U:%G %a' -- "$1" 2>/dev/null; }
norm_mode() { printf '%o' "$((8#$1))"; }
act_dir() { # PATH OWNER GROUP MODE
  local p=$1 want got
  want="$2:$3 $(norm_mode "$4")"
  if [ -L "$p" ] || { [ -e "$p" ] && [ ! -d "$p" ]; }; then
    echo "FAIL[$STEP] $p exists and is not a plain directory: refusing to replace it"; FAILED=1; return 1
  fi
  got=$(stat_ugm "$p")
  if [ "$got" = "$want" ]; then echo "OK[$STEP] dir $p $want (unchanged)"; return 0; fi
  echo "PLAN[$STEP] dir $p $want${got:+ (now $got)}"
  [ $APPLY = 1 ] || return 0
  if ! { { [ -d "$p" ] || mkdir -- "$p"; } && chown -- "$2:$3" "$p" && chmod -- "$4" "$p"; }; then
    echo "FAIL[$STEP] dir $p"; FAILED=1; return 1
  fi
}
act_install() { # SRC DST OWNER GROUP MODE [show]
  local src=$1 dst=$2 want got tmp
  want="$3:$4 $(norm_mode "$5")"
  if [ -L "$dst" ] || { [ -e "$dst" ] && [ ! -f "$dst" ]; }; then
    echo "FAIL[$STEP] $dst exists and is not a regular file: refusing to replace it"; FAILED=1; return 1
  fi
  got=$(stat_ugm "$dst")
  if [ "$got" = "$want" ] && cmp -s -- "$src" "$dst"; then
    echo "OK[$STEP] file $dst $want sha256 $(sha "$dst") (unchanged)"; return 0
  fi
  echo "PLAN[$STEP] install $dst $want sha256 $(sha "$src") bytes $(stat -c %s -- "$src") from $src"
  if [ "${6:-}" = show ]; then
    echo "  ---- $dst ($want) ----"; sed 's/^/  | /' -- "$src"; echo "  ---- end $dst ----"
  fi
  [ $APPLY = 1 ] || return 0
  # chown BEFORE chmod: a chown clears the set-id bits.
  if ! { tmp=$(mktemp "$(dirname -- "$dst")/.axon-deploy.XXXXXX") && cat -- "$src" >"$tmp" \
    && chown -- "$3:$4" "$tmp" && chmod -- "$5" "$tmp" && mv -fT -- "$tmp" "$dst"; }; then
    rm -f -- "${tmp:-}"; echo "FAIL[$STEP] install $dst"; FAILED=1; return 1
  fi
  cmp -s -- "$src" "$dst" || { echo "FAIL[$STEP] $dst does not hold the bytes installed"; FAILED=1; return 1; }
}
# The digest a config pins: of the INSTALLED file after --apply; in a dry run,
# of the source bytes the install would copy (equal by construction).
pin_of() { # DST SRC
  if [ $APPLY = 1 ] && [ -f "$1" ]; then sha "$1"; else sha "$2"; fi
}
act_group() { # NAME
  if [ -n "$(gid_of "$1")" ]; then echo "OK[$STEP] group $1 gid $(gid_of "$1") (exists)"; return 0; fi
  echo "PLAN[$STEP] groupadd --system $1"
  [ $APPLY = 1 ] || return 0
  groupadd --system "$1" || { echo "FAIL[$STEP] groupadd $1"; FAILED=1; return 1; }
}
act_user() { # NAME COMMENT
  local u=$1 uid sh pg
  uid=$(uid_of "$u")
  if [ -n "$uid" ]; then
    sh=$(getent passwd "$u" | cut -d: -f7); pg=$(getent passwd "$u" | cut -d: -f4)
    [ "$uid" != 0 ] || { echo "FAIL[$STEP] user $u is uid 0"; FAILED=1; return 1; }
    [ "$pg" = "$(gid_of "$u")" ] || { echo "FAIL[$STEP] user $u's primary group is gid $pg, not group $u: its group is what the helper and the socket admit"; FAILED=1; return 1; }
    case "$sh" in */nologin|*/false) ;; *) note "user $u has login shell $sh (a service user should have none)" ;; esac
    echo "OK[$STEP] user $u uid $uid gid $pg (exists)"; return 0
  fi
  echo "PLAN[$STEP] useradd --system --gid $u --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin --comment '$2' $u"
  [ $APPLY = 1 ] || return 0
  useradd --system --gid "$u" --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin \
    --comment "$2" "$u" || { echo "FAIL[$STEP] useradd $u"; FAILED=1; return 1; }
}
# Every ancestor of an operator path: root-owned, not group/other-writable, no symlink.
operator_chain() { # PATH (may not exist yet: its existing ancestors are checked)
  local p=$1 st
  while [ "$p" != / ]; do
    p=$(dirname -- "$p")
    [ -e "$p" ] || continue
    if [ -L "$p" ]; then blocked "$p is a symlink on an operator path: Axon's ownership walk refuses it"; return 1; fi
    st=$(stat -c '%u %a' -- "$p")
    if [ "${st%% *}" != 0 ] || [ $(( 8#${st#* } & 8#022 )) -ne 0 ]; then
      blocked "$p is not root-owned and closed to group/other writes (uid ${st%% *}, mode ${st#* }): every directory above an operator file must be"
      return 1
    fi
  done
}

# Values the configs need, resolved now (placeholders in a dry run on a host
# where a user does not exist yet).
ph_uid() { local u; u=$(uid_of "$1"); printf '%s' "${u:-<uid of $1, allocated by --apply>}"; }

# ═════════════════════════════════════════════════════════════════════════════
STEP=allowlist
if selected allowlist; then
  echo "== allowlist: $ALLOWLIST (amendment 44, decision C)"
  operator_chain "$ALLOWLIST"
  act_dir "$ETC" root root 0755
  printf '%s\n' "axon-provenance-allowlist/1" \
    "# installed by scripts/operator_deploy_protected_host.sh at $COMMIT (decision C)" \
    "# target/: cargo's in-tree output. dist/: the guest image build's output." \
    "target/" "dist/" >"$WORK/allowlist"
  act_install "$WORK/allowlist" "$ALLOWLIST" root root 0644 show
fi

STEP=users
if selected users; then
  echo "== users (own uid and group each, no login)"
  act_group "$FABRIC_USER"; act_user "$FABRIC_USER" "Axon Fabric service (non-root; amendment 45)"
  act_group "$CUSTODIAN_USER"; act_user "$CUSTODIAN_USER" "Axon observation-nonce custodian (amendment 50)"
  act_group "$OBSERVER_USER"; act_user "$OBSERVER_USER" "Axon preflight observer service (amendment 68)"
  act_group "$VERIFIER_USER"; act_user "$VERIFIER_USER" "Axon readiness verifier actor (trust preflight)"
  act_group "$PROFILE_USER"; act_user "$PROFILE_USER" "Axon B263 launch profile (jailer uid)"
fi
FU=$(ph_uid "$FABRIC_USER") CU=$(ph_uid "$CUSTODIAN_USER") OU=$(ph_uid "$OBSERVER_USER")

STEP=dirs
if selected dirs; then
  echo "== directories"
  for p in "$ETC" "$LIBEXEC" "$GUEST_DIR" "$OUT_ROOT" "$STAGING" "$STORE" "$OBS_STORE" "$OBS_KEY_DIR" "$AUTH_STORE" "$UNIT_DIR/x"; do
    operator_chain "$p"
  done
  act_dir "$ETC" root root 0755
  for d in "$TRUST" "$KEYS_DIR" "$QUAL_DIR" "$SUITES_DIR" "$GRANTS_DIR"; do act_dir "$d" root root 0755; done
  for d in /usr/local/libexec "$LIBEXEC" /usr/local/lib /usr/local/lib/axon "$GUEST_DIR"; do act_dir "$d" root root 0755; done
  act_dir "$FABRIC_STATE" root root 0755
  act_dir "$OUT_ROOT" "$FABRIC_USER" "$FABRIC_USER" 0700
  act_dir "$STAGING" root root 0700
  act_dir "$CUST_STATE" root root 0755
  act_dir "$STORE" "$CUSTODIAN_USER" "$CUSTODIAN_USER" 0700
  # Amendment 68: the observer's record store and key directory are its own
  # (0700); their parent is the operator's (axon-observer checks the chain).
  act_dir "$OBS_STATE" root root 0755
  act_dir "$OBS_STORE" "$OBSERVER_USER" "$OBSERVER_USER" 0700
  act_dir "$OBS_KEY_DIR" "$OBSERVER_USER" "$OBSERVER_USER" 0700
  act_dir "$(dirname "$AUTH_STORE")" root root 0755
  [ -d "$AUTH_STORE" ] || pending "authority store $AUTH_STORE does not exist: the axon-loop that owns it creates it (Fabric reads its epoch; the kit places only its root-owned parent)"
  act_dir "$DEPLOY_LOG" root root 0755
  # The helper is setuid: its filesystem must honour that.
  fsopt=$(findmnt -no OPTIONS --target "$(dirname "$LIBEXEC")" 2>/dev/null)
  case ",$fsopt," in *,nosuid,*) blocked "$LIBEXEC is on a nosuid mount ($fsopt): the setuid-root helper would run as the Fabric uid" ;; esac
fi

STEP=binaries
if selected binaries; then
  echo "== binaries (root-owned, not group/other-writable; each pinned by digest where the code pins it)"
  act_install "$FABRIC_SRC" "$LIBEXEC/axon-fabric" root root 0755
  act_install "$CUST_SRC" "$LIBEXEC/axon-custodian" root root 0755
  # Amendment 68: the observer SERVICE, pinned in the helper config.
  act_install "$OBS_SRC" "$LIBEXEC/axon-observer" root root 0755
  # A (amendment 45): setuid-root, executable only by the Fabric group.
  act_install "$HELPER_SRC" "$LIBEXEC/axon-protected-launcher" root "$FABRIC_USER" 4750
  act_install "$LAUNCHER_SRC" "$LIBEXEC/fc_linux_profile.sh" root root 0755
  note "launcher interpreter pinned: $BASH_PIN sha256 $(sha "$BASH_PIN") (the system bash; not installed by the kit)"
  [ "$(stat -c '%u %a' "$BASH_PIN")" != "" ] && [ "$(stat -c %u "$BASH_PIN")" = 0 ] \
    || blocked "$BASH_PIN is not root-owned"
  for e in "$FIRECRACKER" "$JAILER"; do
    if [ ! -f "$e" ]; then blocked "$e is absent: install the pinned Firecracker release (the manifest's engine pins)"; continue; fi
    st=$(stat -c '%u %a' "$e")
    [ "${st%% *}" = 0 ] && [ $(( 8#${st#* } & 8#022 )) -eq 0 ] || blocked "$e is not root-owned and closed to group/other writes ($st)"
  done
fi

STEP=guest
GUEST_OK=0
if selected guest || selected configs || selected toolchain; then
  echo "== guest image (built by AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh in $CLONE)"
  if [ ! -f "$MANIFEST_SRC" ]; then
    blocked "$MANIFEST_SRC is absent: run the FULL controlled guest build in the clone first (runbook step 2)"
  elif ! cmp -s "$MANIFEST_SRC" "$CLONE/profiles/linux-microvm/manifest.json"; then
    blocked "dist/guest-linux/manifest.json is not the committed profiles/linux-microvm/manifest.json: commit the re-pin, then deploy from that commit"
  else
    gw=$(python3 -I - "$MANIFEST_SRC" "$CLONE/dist/guest-linux" "$FIRECRACKER" "$JAILER" "$CLONE" <<'PY'
import hashlib, importlib.util, json, os, sys
man, dist, fc, jl, clone = sys.argv[1:6]
sys.dont_write_bytecode = True
m = json.load(open(man))
def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""): h.update(b)
    return h.hexdigest()
bad = []
for n in ("vmlinux", "rootfs.sqfs"):
    p = os.path.join(dist, n)
    want = ((m.get("artifacts") or {}).get(n) or {}).get("sha256")
    if not os.path.isfile(p): bad.append(f"{n} absent from dist/guest-linux")
    elif sha(p) != want: bad.append(f"{n} sha256 {sha(p)} is not the manifest's {want}")
src = m.get("source") or {}
if src.get("axon_tree_dirty_at_build") is not False or src.get("axon_tree_dirty_reasons") != []:
    bad.append(f"the manifest says the image was built from a dirty tree: {src.get('axon_tree_dirty_reasons')} "
               "(install the allowlist, build from the standalone clone)")
if not src.get("build_environment"): bad.append("no source.build_environment (built before amendment 56: it does not freeze)")
if not (m.get("kernel") or {}).get("build_environment"):
    bad.append("no kernel.build_environment (built before amendment 63, or --rootfs-only: it does not freeze; run the FULL build)")
# Round 5 (amendment 80): the records are judged HERE, before anything is
# installed, by the freeze's own judge (the clone's guest_build_env.py): the
# structure, every host tool the build recorded, and the builder's proof -- so a
# binary built elsewhere with a hand-written record naming its digests is
# refused at install, not only at the freeze.
if not bad:
    gs = importlib.util.spec_from_file_location("guest_build_env", os.path.join(clone, "scripts", "guest_build_env.py"))
    g = importlib.util.module_from_spec(gs)
    gs.loader.exec_module(g)
    why = g.shape_problems(src["build_environment"]) or g.image_problems(m, pin_required=False)
    if why:
        bad.append(f"the guest build records are not a controlled build's: {why}")
eng = m.get("engine") or {}
for p, k in ((fc, "firecracker_sha256"), (jl, "jailer_sha256")):
    if os.path.isfile(p) and sha(p) != eng.get(k):
        bad.append(f"{p} sha256 {sha(p)} is not the manifest's engine pin {eng.get(k)}")
print("\n".join(bad))
PY
)
    if [ -n "$gw" ]; then
      while IFS= read -r l; do blocked "guest: $l"; done <<<"$gw"
    else
      GUEST_OK=1
      echo "GUEST manifest $(sha "$MANIFEST_SRC") clean, controlled build records present, artifacts and engine match their pins"
    fi
  fi
  if selected guest && [ $GUEST_OK = 1 ]; then
    act_install "$CLONE/dist/guest-linux/vmlinux" "$GUEST_DIR/vmlinux" root root 0644
    act_install "$CLONE/dist/guest-linux/rootfs.sqfs" "$GUEST_DIR/rootfs.sqfs" root root 0644
    act_install "$MANIFEST_SRC" "$GUEST_DIR/manifest.json" root root 0644
  fi
fi

STEP=data
# install_tree REGISTRY DESTDIR KIND: the registry and every RELATIVE path it
# names (cortex: executors[].path with a directory part, checks[].root; grants:
# grants[].path), copied root-owned. Absolute paths stay where they are and are
# listed for the operator to own.
install_registry() { # REGISTRY DESTDIR KIND
  local reg=$1 dst=$2 kind=$3 rel src
  [ -f "$reg" ] || refuse "$kind registry $reg is not a file"
  python3 -I - "$reg" "$kind" >"$WORK/$kind.list" <<'PY' || refuse "$kind registry $reg is malformed"
import json, os, sys
reg, kind = sys.argv[1:3]
v = json.load(open(reg))
want = {"suite": "cortex-check-registry/1", "grant": "axon-fabric-grant-registry/1"}[kind]
if v.get("schema") != want: sys.exit(f"schema is not {want}")
paths = []
if kind == "suite":
    for e in v.get("executors") or []:
        p = e.get("path", "")
        if os.path.isabs(p): print("ABS\t" + p)
        elif len(p.split("/")) > 1: paths.append(p)
        else: print("BARE\t" + p)
    for c in v.get("checks") or []:
        p = c.get("root", "")
        if os.path.isabs(p): print("ABS\t" + p)
        else: paths.append(p)
else:
    for g in v.get("grants") or []:
        paths.append(g.get("path", ""))
for p in paths:
    q = p.rstrip("/")
    if not q or q.startswith("/") or any(x in ("", ".", "..", ".git") for x in q.split("/")):
        sys.exit(f"path {p!r} is not a plain relative path")
    p = q
    print("REL\t" + p)
PY
  act_install "$reg" "$dst/$(basename "$reg")" root root 0644
  while IFS=$'\t' read -r k rel; do
    case "$k" in
      ABS) note "$kind registry names the absolute path $rel: it is used where it is; the operator must own it (root, not group/other-writable)" ;;
      BARE) blocked "$kind registry names the bare executor path '$rel': the registry loader resolves a one-component path against the CALLER's working directory, not the registry's; give it a directory part" ;;
      REL)
        src=$(dirname "$reg")/$rel
        local d=$dst part parts=()
        IFS=/ read -r -a parts <<<"$(dirname "$rel")"
        for part in "${parts[@]}"; do [ "$part" = . ] && continue; d=$d/$part; act_dir "$d" root root 0755; done
        if [ -d "$src" ] && [ ! -L "$src" ]; then
          while IFS= read -r -d '' f; do
            local r=${f#"$(dirname "$reg")"/}
            if [ -d "$f" ]; then act_dir "$dst/$r" root root 0755
            elif [ -f "$f" ] && [ ! -L "$f" ]; then
              local m=0644; [ -x "$f" ] && m=0755; act_install "$f" "$dst/$r" root root "$m"
            else refuse "$kind registry tree holds $f, which is neither a directory nor a regular file"; fi
          done < <(find "$src" -print0 | sort -z)
        elif [ -f "$src" ] && [ ! -L "$src" ]; then
          local m=0644; [ -x "$src" ] && m=0755; act_install "$src" "$dst/$rel" root root "$m"
        else refuse "$kind registry names $rel, which is not a regular file or directory beside it"; fi ;;
    esac
  done <"$WORK/$kind.list"
}
if selected data; then
  echo "== operator data"
  if [ -n "$SUITE_REG" ]; then install_registry "$SUITE_REG" "$SUITES_DIR" suite
  else blocked "--suite-registry FILE is required: the operator suite registry (cortex-check-registry/1) the host config pins"; fi
  if [ -n "$GRANT_REG" ]; then install_registry "$GRANT_REG" "$GRANTS_DIR" grant
  else blocked "--grant-registry FILE is required: without a grant registry a protected host authorizes nothing (D1)"; fi
  if [ -n "$B263_RECORD" ]; then
    [ -f "$B263_RECORD" ] && [ -f "$B263_RECORD.sig" ] || refuse "--b263-record needs FILE and FILE.sig (the operator's detached qualification signature)"
    act_install "$B263_RECORD" "$QUAL_DIR/b263.json" root root 0644
    act_install "$B263_RECORD.sig" "$QUAL_DIR/b263.json.sig" root root 0644
  fi
  if [ -n "$B263_WAIVERS" ]; then
    [ -f "$B263_WAIVERS" ] && [ -f "$B263_WAIVERS.sig" ] || refuse "--b263-waivers needs FILE and FILE.sig"
    act_install "$B263_WAIVERS" "$QUAL_DIR/b263-waivers.json" root root 0644
    act_install "$B263_WAIVERS.sig" "$QUAL_DIR/b263-waivers.json.sig" root root 0644
  fi
fi
# The record the host config pins: the one installed, or the one about to be.
B263_SRC=$QUAL_DIR/b263.json; [ -n "$B263_RECORD" ] && B263_SRC=$B263_RECORD
WAIVERS_SRC=""; [ -f "$QUAL_DIR/b263-waivers.json" ] && WAIVERS_SRC=$QUAL_DIR/b263-waivers.json
[ -n "$B263_WAIVERS" ] && WAIVERS_SRC=$B263_WAIVERS
if selected data || selected configs; then
  STEP=data
  if [ ! -f "$B263_SRC" ]; then
    pending "no B263 qualification record at $QUAL_DIR/b263.json: run scripts/b263_qualify.sh --host-label NAME --issuer-key-id ed25519:<16hex> from the clone ON THIS HOST, have the operator sign it, re-run with --b263-record (Fabric refuses every protected launch until then)"
  else
    bw=$(python3 -I - "$B263_SRC" "$MANIFEST_SRC" "${WAIVERS_SRC:-}" <<'PY'
import hashlib, json, os, sys
rec, man, wv = sys.argv[1:4]
b = open(rec, "rb").read(); r = json.loads(b); out = []
if r.get("schema") != "axon-b263-evidence/1": out.append("the B263 record is not axon-b263-evidence/1")
if os.path.isfile(man) and (r.get("profile") or {}).get("manifest_sha256") != hashlib.sha256(open(man, "rb").read()).hexdigest():
    out.append("the B263 record qualified another profile manifest than the one deployed (re-run B263 on this image)")
if (r.get("source") or {}).get("tree_dirty") is not False: out.append("the B263 record was produced from a dirty tree")
if not r.get("issuer_key_id"): out.append("the B263 record names no issuer_key_id (run b263_qualify.sh --issuer-key-id ed25519:<16hex>)")
# Amendment 65: the record states the host it RAN on (scripts/b263_host.py).
# Fabric requires only a non-empty host and compares it to nothing; the kit
# holds the measured machine-id to this host's, so a record qualified
# elsewhere is not installed here unnoticed.
facts = r.get("host_facts") or {}
if not r.get("host") or "machine_id" not in facts or not (r.get("source") or {}).get("host_identity_sha256"):
    out.append("the B263 record does not carry the measured host (amendment 65: host_facts.machine_id, "
               "source.host_identity_sha256): re-qualify with the current b263_qualify.sh --host-label NAME")
else:
    try:
        here = open("/etc/machine-id").readline().strip() or None
    except OSError:
        here = None
    if facts.get("machine_id") != here:
        out.append(f"the B263 record was measured on machine-id {facts.get('machine_id')}, not this host's {here}: "
                   "qualify THIS host (b263_qualify.sh --host-label NAME)")
    if not facts.get("host_label"):
        out.append("the B263 record has no operator host label: re-run b263_qualify.sh with --host-label NAME "
                   "(every protected receipt carries qualification-host:<host> verbatim)")
if r.get("result") == "PASS_WITH_BLOCKED":
    if not wv: out.append("result PASS_WITH_BLOCKED: every BLOCKED assertion needs an operator-signed axon-b263-waiver/1 (--b263-waivers)")
    else:
        w = json.load(open(wv))
        if w.get("evidence_sha256") != hashlib.sha256(b).hexdigest(): out.append("the waiver file is bound to another record (evidence_sha256)")
        blocked = {a["name"] for a in r.get("assertions", []) if a.get("status") == "BLOCKED"}
        covered = {x.get("assertion") for x in w.get("waivers", [])}
        if blocked - covered: out.append(f"BLOCKED assertions without a waiver: {sorted(blocked - covered)}")
elif r.get("result") != "PASS": out.append(f"the B263 result is {r.get('result')!r}: only PASS or PASS_WITH_BLOCKED with waivers qualifies")
print("\n".join(out))
PY
)
    if [ -n "$bw" ]; then while IFS= read -r l; do pending "B263: $l"; done <<<"$bw"
    else note "B263 record $(sha "$B263_SRC") is shaped to qualify (Fabric's qualification() re-verifies the signature and every rule at each launch)"; fi
  fi
fi

STEP=configs
signer_ok=1
if [ -z "$SIGNER_PUB" ]; then
  signer_ok=0
  if selected configs || selected check; then
    blocked "--signer-public-key HEX is required: the public half of the Fabric attestation key (the host signer). Provision it: sudo $LIBEXEC/axon-fabric keygen --out $SIGNER_KEY && sudo chown $FABRIC_USER:$FABRIC_USER $SIGNER_KEY (keygen prints public_key)"
  fi
fi
if [ ! -f "$SIGNER_KEY" ] || [ -L "$SIGNER_KEY" ]; then
  signer_ok=0
  if selected configs || selected check; then
    blocked "the Fabric attestation key $SIGNER_KEY is absent: provision it (operator custody; the kit never makes keys): sudo $LIBEXEC/axon-fabric keygen --out $SIGNER_KEY; sudo chown $FABRIC_USER:$FABRIC_USER $SIGNER_KEY; install its public_key as $TRUST/verifier/host-signer.pub"
  fi
else
  ks=$(stat -c '%u %a' "$SIGNER_KEY")
  # Fabric's key loader (axon-fabric.rs, spec §2 rule 1) requires mode & 0277
  # == 0: readable by its owner alone and writable by no one, so 0600 refuses.
  if [ "${ks%% *}" != "$(uid_of "$FABRIC_USER")" ] || [ $(( 8#${ks#* } & 8#277 )) -ne 0 ]; then
    signer_ok=0
    if selected configs || selected check; then
      blocked "$SIGNER_KEY must be owned by $FABRIC_USER, readable by it alone and writable by no one (A20, mode 0400; is uid ${ks%% *} mode ${ks#* }): sudo chown $FABRIC_USER:$FABRIC_USER $SIGNER_KEY; sudo chmod 0400 $SIGNER_KEY"
    fi
  fi
fi
gen_configs() { # writes $WORK/{custodian,observer,launcher,host}.json from the pins
  python3 -I - "$WORK" <<'PY'
import json, os, sys
w = sys.argv[1]; e = os.environ
def num(v):
    return int(v) if v.isdigit() else v   # a dry-run placeholder stays a visible string
def pin(p, s): return {"path": p, "sha256": s}
custodian = {"schema": "axon-custodian/1", "custodian_uid": num(e["K_CU"]), "fabric_uid": num(e["K_FU"]),
             "launcher_uid": 0, "socket": e["K_SOCKET"], "store": e["K_STORE"],
             "max_age_s": int(e["K_NONCE_AGE"]),
             # Amendment 79: the observer service's uid, the only one the custodian
             # answers `check` (is this nonce outstanding?) for.
             "observer_uid": num(e["K_OU"])}
launcher = {"schema": "axon-protected-launcher/2", "fabric_uid": num(e["K_FU"]),
            # Amendment 79: the installed axon-fabric, by path, sha256 and the
            # build revision its own verifier-manifest states.
            "fabric": {"path": e["K_FABRIC"], "sha256": e["K_FABRIC_SHA"], "revision": e["K_FABRIC_REV"]},
            "interpreter": pin(e["K_BASH"], e["K_BASH_SHA"]),
            "launcher": pin(e["K_LAUNCHER"], e["K_LAUNCHER_SHA"]),
            "profile_manifest": pin(e["K_MANIFEST"], e["K_MANIFEST_SHA"]),
            "artifacts_dir": e["K_GUEST"], "firecracker": e["K_FC"], "jailer": e["K_JL"],
            "out_root": e["K_OUT"], "staging_root": e["K_STAGING"],
            "max_timeout_s": int(e["K_MAX_TIMEOUT"]), "max_input_bytes": int(e["K_MAX_INPUT"]),
            "observer": {"root": e["K_OBS_ROOT"], "max_age_s": int(e["K_OBS_AGE"]),
                         "host_signer_public_key": e["K_SIGNER_PUB"]},
            # Amendment 65: the custodian PROGRAM the helper verifies on every
            # reply (SCM_PIDFD -> /proc/<pid>/exe hashed by descriptor);
            # load_config refuses a production helper config without it.
            "custodian": {"socket": e["K_SOCKET"], "uid": num(e["K_CU"]), "sha256": e["K_CUST_SHA"]}}
# Amendment 68: the helper relays from the observer SERVICE, pinned by
# program; the host config names no observer program (a production Fabric
# refuses observer.command) and keeps only the custodian and the max age.
launcher["observer"]["service"] = {"socket": e["K_OBS_SOCKET"], "uid": num(e["K_OU"]),
                                   "sha256": e["K_OBS_SHA"]}
observer = {"custodian": {"socket": e["K_SOCKET"], "uid": num(e["K_CU"])},
            "max_age_s": int(e["K_OBS_AGE"])}
observer_service = {"schema": "axon-observer/1", "observer_uid": num(e["K_OU"]), "fabric_uid": num(e["K_FU"]),
                    "caller_uid": 0, "socket": e["K_OBS_SOCKET"], "store": e["K_OBS_STORE"],
                    "key_path": e["K_OBS_KEY"]}
host = {"schema": "axon-protected-host/1",
        "launcher": pin(e["K_LAUNCHER"], e["K_LAUNCHER_SHA"]),
        "privileged_launcher": pin(e["K_HELPER"], e["K_HELPER_SHA"]),
        "profile_manifest": pin(e["K_MANIFEST"], e["K_MANIFEST_SHA"]),
        "artifacts_dir": e["K_GUEST"],
        "qualification": {"record": e["K_B263"], "signature": e["K_B263"] + ".sig",
                          "waivers": e.get("K_WAIVERS") or None, "max_age_s": int(e["K_QUAL_AGE"])},
        "suite_registry": pin(e["K_SUITES"], e["K_SUITES_SHA"]),
        "signer": {"issuer_ref": e["K_ISSUER"], "public_key": e["K_SIGNER_PUB"], "key_path": e["K_KEY"]},
        "out_root": e["K_OUT"],
        "observer": observer,
        "grant_registry": pin(e["K_GRANTS"], e["K_GRANTS_SHA"]),
        "authority_store": e["K_AUTH"]}
for name, doc in (("custodian", custodian), ("observer", observer_service), ("launcher", launcher), ("host", host)):
    with open(os.path.join(w, name + ".json"), "w") as f:
        json.dump(doc, f, indent=2); f.write("\n")
PY
}
if selected configs; then
  echo "== configs (root-owned 0644; every pin from the INSTALLED bytes)"
  for p in "$CUSTODIAN_CONFIG" "$OBSERVER_CONFIG" "$HELPER_CONFIG" "$HOST_CONFIG"; do operator_chain "$p"; done
  suite_dst=$SUITES_DIR/$(basename "${SUITE_REG:-registry.json}")
  grant_dst=$GRANTS_DIR/$(basename "${GRANT_REG:-grants.json}")
  stop=0
  [ $signer_ok = 1 ] || stop=1
  [ -n "$SUITE_REG" ] && [ -n "$GRANT_REG" ] || stop=1
  [ $GUEST_OK = 1 ] || stop=1
  if [ $stop = 1 ] && [ $APPLY = 1 ]; then
    blocked "configs not written: a signer, registry or guest item above is BLOCKED (the host config would name what is not there)"
  else
    [ $stop = 0 ] || note "dry run: the configs below carry placeholders for the BLOCKED items"
    ph() { [ -n "$1" ] && [ -f "$1" ] && pin_of "$2" "$1" || printf '<sha256 of %s>' "$2"; }
    K_CU=$CU K_FU=$FU K_SOCKET=$SOCKET K_STORE=$STORE K_NONCE_AGE=$NONCE_MAX_AGE_S
    K_BASH=$BASH_PIN; K_BASH_SHA=$(sha "$BASH_PIN")
    K_LAUNCHER=$LIBEXEC/fc_linux_profile.sh; K_LAUNCHER_SHA=$(pin_of "$K_LAUNCHER" "$LAUNCHER_SRC")
    K_MANIFEST=$GUEST_DIR/manifest.json; K_MANIFEST_SHA=$(ph "$MANIFEST_SRC" "$K_MANIFEST")
    K_GUEST=$GUEST_DIR K_FC=$FIRECRACKER K_JL=$JAILER K_OUT=$OUT_ROOT K_STAGING=$STAGING
    K_MAX_TIMEOUT=$MAX_TIMEOUT K_MAX_INPUT=$MAX_INPUT K_OBS_ROOT=$TRUST/observer K_OBS_AGE=$OBS_MAX_AGE_S
    K_SIGNER_PUB=${SIGNER_PUB:-"<64-hex public key of $SIGNER_KEY>"}
    K_HELPER=$LIBEXEC/axon-protected-launcher; K_HELPER_SHA=$(pin_of "$K_HELPER" "$HELPER_SRC")
    # The custodian PROGRAM pin (amendment 65): the installed axon-custodian's
    # bytes, the file the custodian unit's ExecStart names.
    K_CUST_SHA=$(pin_of "$LIBEXEC/axon-custodian" "$CUST_SRC")
    # The observer PROGRAM pin (amendment 68): the installed axon-observer's
    # bytes, the file the observer unit's ExecStart names.
    K_OBS_SHA=$(pin_of "$LIBEXEC/axon-observer" "$OBS_SRC")
    K_OU=$OU K_OBS_SOCKET=$OBS_SOCKET K_OBS_STORE=$OBS_STORE K_OBS_KEY=$OBS_KEY
    # The Fabric PROGRAM pin (amendment 79): the installed axon-fabric's bytes
    # and the build revision IT states (its own `verifier-manifest`, whose
    # self-reported sha256 must be its bytes'): never a value the operator types.
    K_FABRIC=$LIBEXEC/axon-fabric; K_FABRIC_SHA=$(pin_of "$K_FABRIC" "$FABRIC_SRC")
    fab_vm=$K_FABRIC; { [ $APPLY = 1 ] && [ -x "$K_FABRIC" ]; } || fab_vm=$FABRIC_SRC
    K_FABRIC_REV=$("$fab_vm" verifier-manifest 2>/dev/null | python3 -I -c '
import json, sys
m = json.load(sys.stdin)
if m.get("sha256") != sys.argv[1]:
    sys.exit("verifier-manifest names another sha256 than the bytes pinned")
print(m["fabric_revision"])' "$K_FABRIC_SHA") || { echo "FAIL[$STEP] reading the build revision of $fab_vm (its verifier-manifest must state this binary's own sha256)"; FAILED=1; K_FABRIC_REV="<40-hex fabric_revision of $fab_vm>"; }
    K_B263=$QUAL_DIR/b263.json; K_WAIVERS=""
    [ -z "$WAIVERS_SRC" ] || K_WAIVERS=$QUAL_DIR/b263-waivers.json
    K_QUAL_AGE=$QUAL_MAX_AGE K_SUITES=$suite_dst; K_SUITES_SHA=$(ph "$SUITE_REG" "$suite_dst")
    K_ISSUER=$ISSUER_REF K_KEY=$SIGNER_KEY K_GRANTS=$grant_dst; K_GRANTS_SHA=$(ph "$GRANT_REG" "$grant_dst")
    K_AUTH=$AUTH_STORE
    export K_CU K_FU K_SOCKET K_STORE K_NONCE_AGE K_BASH K_BASH_SHA K_LAUNCHER K_LAUNCHER_SHA K_MANIFEST \
      K_MANIFEST_SHA K_GUEST K_FC K_JL K_OUT K_STAGING K_MAX_TIMEOUT K_MAX_INPUT K_OBS_ROOT K_OBS_AGE \
      K_SIGNER_PUB K_HELPER K_HELPER_SHA K_CUST_SHA K_OBS_SHA K_FABRIC K_FABRIC_SHA K_FABRIC_REV K_OU K_OBS_SOCKET K_OBS_STORE K_OBS_KEY K_B263 \
      K_WAIVERS K_QUAL_AGE K_SUITES K_SUITES_SHA K_ISSUER K_KEY K_GRANTS K_GRANTS_SHA K_AUTH
    gen_configs || { echo "FAIL[$STEP] generating the configs"; FAILED=1; }
    act_install "$WORK/custodian.json" "$CUSTODIAN_CONFIG" root root 0644 show
    act_install "$WORK/observer.json" "$OBSERVER_CONFIG" root root 0644 show
    act_install "$WORK/launcher.json" "$HELPER_CONFIG" root root 0644 show
    act_install "$WORK/host.json" "$HOST_CONFIG" root root 0644 show
    [ -n "$WAIVERS_SRC" ] || note "qualification.waivers is null: b263_qualify.sh records x3/x4 BLOCKED on this host, so a PASS_WITH_BLOCKED record needs a waiver file; re-run with --b263-waivers once it is signed"
  fi
fi

STEP=verifier
if selected verifier; then
  echo "== verifier pin: $TRUST/verifier.json (the INSTALLED verifier describes itself)"
  operator_chain "$TRUST/verifier.json"
  act_dir "$TRUST" root root 0755
  vbin=$LIBEXEC/axon-fabric
  if [ $APPLY = 1 ] && [ -x "$vbin" ]; then
    "$vbin" verifier-manifest >"$WORK/verifier.json" || { echo "FAIL[$STEP] $vbin verifier-manifest"; FAILED=1; }
    [ "$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["sha256"])' "$WORK/verifier.json")" = "$(sha "$vbin")" ] \
      || { echo "FAIL[$STEP] the installed verifier's self-reported sha256 is not its bytes'"; FAILED=1; }
  else
    "$FABRIC_SRC" verifier-manifest | python3 -I -c '
import json, sys; m = json.load(sys.stdin); m["path"] = sys.argv[1]; print(json.dumps(m, indent=2, sort_keys=True))' "$vbin" >"$WORK/verifier.json"
    note "dry run: verifier.json shown from the source binary with path set to $vbin; --apply runs the installed one"
  fi
  act_install "$WORK/verifier.json" "$TRUST/verifier.json" root root 0644 show
fi

STEP=systemd
if selected systemd; then
  echo "== systemd: axon-custodian.socket + axon-custodian.service"
  operator_chain "$UNIT_DIR/axon-custodian.socket"
  sed -e "s|^SocketGroup=.*|SocketGroup=$FABRIC_USER|" -e "s|^ListenStream=.*|ListenStream=$SOCKET|" \
    "$CLONE/profiles/protected-host/systemd/axon-custodian.socket" >"$WORK/axon-custodian.socket"
  sed -e "s|^User=.*|User=$CUSTODIAN_USER|" -e "s|^Group=.*|Group=$CUSTODIAN_USER|" \
    -e "s|^ExecStart=.*|ExecStart=$LIBEXEC/axon-custodian|" -e "s|^StateDirectory=.*|StateDirectory=${STORE#/var/lib/}|" \
    "$CLONE/profiles/protected-host/systemd/axon-custodian.service" >"$WORK/axon-custodian.service"
  grep -qx 'SocketMode=0660' "$WORK/axon-custodian.socket" || blocked "the socket unit template no longer says SocketMode=0660 (the Fabric group must connect, nobody else)"
  # Amendment 79: the observer service asks the custodian whether a nonce is
  # outstanding, as its own uid: one named ACL entry on the socket, granted when
  # systemd creates it (the file mode and group stay the Fabric's).
  # Needed where systemd will start the unit (not under --no-systemctl, which only
  # installs the files for a test to judge).
  if [ $NO_SYSTEMCTL = 0 ]; then
    command -v setfacl >/dev/null 2>&1 || blocked "setfacl (the acl package) is required: the observer's uid is granted the custodian socket by ACL"
  fi
  sed -i -e "s|^ExecStartPost=.*|ExecStartPost=/usr/bin/setfacl -m u:$OBSERVER_USER:rw $SOCKET|" "$WORK/axon-custodian.socket"
  grep -qx "ExecStartPost=/usr/bin/setfacl -m u:$OBSERVER_USER:rw $SOCKET" "$WORK/axon-custodian.socket" \
    || blocked "the custodian socket unit template has no ExecStartPost= line for the observer's ACL (amendment 79)"
  act_install "$WORK/axon-custodian.socket" "$UNIT_DIR/axon-custodian.socket" root root 0644 show
  act_install "$WORK/axon-custodian.service" "$UNIT_DIR/axon-custodian.service" root root 0644 show
  # Amendment 68: the observer service. Its socket is root's alone (0600):
  # the setuid-root helper is its only client; neither the Fabric nor any
  # agent can connect (the observer also refuses them by SO_PEERCRED).
  echo "== systemd: axon-observer.socket + axon-observer.service (amendment 68)"
  operator_chain "$UNIT_DIR/axon-observer.socket"
  sed -e "s|^ListenStream=.*|ListenStream=$OBS_SOCKET|" \
    "$CLONE/profiles/protected-host/systemd/axon-observer.socket" >"$WORK/axon-observer.socket"
  sed -e "s|^User=.*|User=$OBSERVER_USER|" -e "s|^Group=.*|Group=$OBSERVER_USER|" \
    -e "s|^ExecStart=.*|ExecStart=$LIBEXEC/axon-observer|" \
    -e "s|^StateDirectory=.*|StateDirectory=${OBS_STORE#/var/lib/} ${OBS_KEY_DIR#/var/lib/}|" \
    "$CLONE/profiles/protected-host/systemd/axon-observer.service" >"$WORK/axon-observer.service"
  for d in SocketMode=0600 SocketUser=root SocketGroup=root; do
    grep -qx "$d" "$WORK/axon-observer.socket" || blocked "the observer socket unit template no longer says $d (only root, the helper, may connect)"
  done
  act_install "$WORK/axon-observer.socket" "$UNIT_DIR/axon-observer.socket" root root 0644 show
  act_install "$WORK/axon-observer.service" "$UNIT_DIR/axon-observer.service" root root 0644 show
  for c in "systemctl daemon-reload" "systemctl enable --now axon-custodian.socket" "systemctl enable --now axon-observer.socket"; do
    if [ $APPLY = 1 ] && [ $NO_SYSTEMCTL = 0 ]; then
      echo "DO[$STEP] $c"; $c || { echo "FAIL[$STEP] $c"; FAILED=1; }
    else
      echo "PLAN[$STEP] $c"
    fi
  done
fi

STEP=loader
if selected loader; then
  echo "== loader: the production ProtectedHost::operator() as $FABRIC_USER against the deployed files"
  if [ $APPLY = 0 ]; then
    echo "PLAN[$STEP] setpriv --reuid $FABRIC_USER --regid $FABRIC_USER --clear-groups $LIBEXEC/axon-fabric submit --request /nonexistent/axon-deploy-probe.json (accepted iff it refuses only the missing request)"
  elif [ ! -f "$HOST_CONFIG" ]; then
    pending "the loader check needs $HOST_CONFIG (configs step)"
  elif [ ! -d "$SOCKET_DIR" ]; then
    pending "$SOCKET_DIR does not exist: start the custodian socket (systemctl enable --now axon-custodian.socket); Fabric's loader walks the socket's directory"
  else
    lo=$(setpriv --reuid "$(uid_of "$FABRIC_USER")" --regid "$(gid_of "$FABRIC_USER")" --clear-groups --inh-caps=-all -- \
      "$LIBEXEC/axon-fabric" submit --request /nonexistent/axon-deploy-probe.json 2>/dev/null)
    kind=$(printf '%s' "$lo" | python3 -I -c 'import json,sys; print(json.load(sys.stdin).get("kind",""))' 2>/dev/null)
    if [ "$kind" = io ]; then
      echo "OK[$STEP] ProtectedHost::operator() accepted the deployed host, helper and custodian configs (it refused only the absent request)"
    elif printf '%s' "$lo" | grep -q 'b263.json'; then
      pending "the production loader refuses until the B263 record is installed: $lo"
    else
      echo "FAIL[$STEP] the production loader refused the deployment: $lo"; FAILED=1
    fi
  fi
fi

STEP=toolchain
if selected toolchain; then
  echo "== host toolchain pin: $TOOLCHAIN_PIN (amendment 63 operator item; read by the freeze since amendment 65)"
  if [ $GUEST_OK = 1 ]; then
    # ONE extraction: the reader's own (guest_build_env.recorded_host_tools,
    # imported from the clone), so the pin names exactly the tools the freeze
    # compares — path and sha256 — and nothing the reader would not.
    python3 -I -B - "$CLONE/scripts" "$MANIFEST_SRC" "$COMMIT" "$WORK/toolchain.json" <<'PY' || { echo "FAIL[$STEP] cannot compute the toolchain pin"; FAILED=1; }
import hashlib, json, sys
scripts, man, commit, out = sys.argv[1:5]
sys.dont_write_bytecode = True  # never a __pycache__ in the clone (it would be dirty)
sys.path.insert(0, scripts)
import guest_build_env as gbe  # noqa: E402  (the reader the freeze uses)
mb = open(man, "rb").read(); m = json.loads(mb)
def sha(p):
    try:
        h = hashlib.sha256()
        with open(p, "rb") as f:
            for b in iter(lambda: f.read(1 << 20), b""): h.update(b)
        return h.hexdigest()
    except (OSError, TypeError):
        return None
tools = gbe.recorded_host_tools(m)
bad = sorted(n for n, t in tools.items() if not t.get("path") or not t.get("sha256"))
if bad:
    sys.exit(f"the build records name tools without a path or digest: {bad}")
drift = {n: {"recorded": t["sha256"], "now": sha(t["path"])} for n, t in tools.items() if sha(t["path"]) != t["sha256"]}
tc = m["source"]["build_environment"]["toolchain"]
doc = {"schema": gbe.TOOLCHAIN_PIN_SCHEMA,
       "status": ("READ by the freeze (amendment 65): scripts/v022_freeze_manifest.py refuses without this "
                  "file, and guest_build_env.toolchain_pin_problems holds every host tool the image's build "
                  "records name to this pin (path and sha256), and every pinned tool to a record"),
       "commit": commit, "profile_manifest_sha256": hashlib.sha256(mb).hexdigest(),
       "toolchain_channel": tc.get("channel"), "rustc_vV": tc.get("rustc_vV"),
       "tools": tools,
       "guest_side_not_host_tools": {"busybox_sha256": (m.get("busybox") or {}).get("sha256"),
                                     "note": "busybox runs in the guest; it is pinned by profiles/linux-microvm/kernel.pin"},
       "drift_since_build": drift}
json.dump(doc, open(out, "w"), indent=2, sort_keys=True); open(out, "a").write("\n")
for name, d in sorted(drift.items()):
    print(f"DRIFT {name}: recorded {d['recorded']} now {d['now']}")
PY
    act_install "$WORK/toolchain.json" "$TOOLCHAIN_PIN" root root 0644 show
    if [ $APPLY = 1 ] && [ -f "$GUEST_DIR/manifest.json" ]; then
      # The freeze's own reader on what was installed: owner and mode of the
      # pin and of every directory above it, then every recorded tool.
      if tp=$(python3 -I -B "$CLONE/scripts/guest_build_env.py" toolchain-pin "$GUEST_DIR/manifest.json" 2>&1); then
        echo "OK[$STEP] guest_build_env.toolchain_pin_problems accepts the installed pin for the deployed image"
      else
        echo "FAIL[$STEP] the freeze's reader refuses the installed pin: $tp"; FAILED=1
      fi
    else
      note "--apply judges the installed pin with the freeze's own reader (guest_build_env.py toolchain-pin)"
    fi
    note "the freeze reads $TOOLCHAIN_PIN on the host that RUNS the freeze: install the same pin there (or freeze on this host)"
  else
    blocked "the toolchain pin needs the deployed guest manifest's build records (guest step)"
  fi
fi

STEP=fabric-unit
# Amendment 65 / memo condition C2: under NoNewPrivileges the kernel ignores
# the helper's set-id bit, the helper runs as the Fabric uid and refuses every
# launch. The kit judges the unit's TEXT (the unit and its drop-ins, as
# `systemctl cat` prints them; or an operator's file before it is installed);
# the preflight judges the RUNNING process (/proc/<MainPID>/status NoNewPrivs).
if selected fabric-unit || selected preflight; then
  echo "== the Fabric service unit (amendment 65: no NoNewPrivileges, nothing that implies it)"
  unit_txt=""
  if [ -z "$FABRIC_UNIT" ]; then
    pending "--fabric-unit UNIT is required: the systemd unit that runs Fabric (axon-fabric submit) as $FABRIC_USER. The kit judges it for NoNewPrivileges, and its MainPID is the preflight's --fabric-pid"
  elif [ "${FABRIC_UNIT#/}" != "$FABRIC_UNIT" ]; then
    [ -f "$FABRIC_UNIT" ] || refuse "--fabric-unit $FABRIC_UNIT is not a file"
    unit_txt=$(cat -- "$FABRIC_UNIT")
    note "judging the unit file $FABRIC_UNIT as written (drop-ins installed beside it later are not seen: re-run with the unit's NAME once it is installed)"
  elif command -v systemctl >/dev/null && unit_txt=$(systemctl cat -- "$FABRIC_UNIT" 2>/dev/null) && [ -n "$unit_txt" ]; then
    :
  else
    unit_txt=""
    blocked "systemctl cat $FABRIC_UNIT failed: the Fabric unit is not installed (or systemd is not running); install it, or pass its file with --fabric-unit /path/to/unit"
  fi
  if [ -n "$unit_txt" ]; then
    printf '%s\n' "$unit_txt" >"$WORK/fabric-unit.txt"
    uj=$(python3 -I - "$FABRIC_USER" "$WORK/fabric-unit.txt" <<'PY'
import sys
fabric_user, unit_file = sys.argv[1:3]
# Directives in [Service]; a later assignment wins, an empty one resets
# (systemd's own rule for the list-valued settings; for a boolean the last one
# wins anyway). Drop-ins follow the unit in `systemctl cat`'s output.
vals, section = {}, None
lines = open(unit_file).read().splitlines()
i = 0
while i < len(lines):
    raw = lines[i]; i += 1
    while raw.endswith("\\") and i < len(lines):
        raw = raw[:-1] + " " + lines[i]; i += 1
    l = raw.strip()
    if not l or l[0] in "#;":
        continue
    if l.startswith("[") and l.endswith("]"):
        section = l[1:-1]; continue
    if section != "Service" or "=" not in l:
        continue
    k, v = (x.strip() for x in l.split("=", 1))
    if v == "":
        vals.pop(k, None)
    elif k in vals and k in ("SystemCallFilter", "SystemCallLog", "SystemCallArchitectures", "RestrictAddressFamilies",
                             "RestrictNamespaces", "SecureBits", "CapabilityBoundingSet"):
        vals[k] = vals[k] + " " + v
    else:
        vals[k] = v
def true(k):
    return vals.get(k, "").lower() in ("1", "yes", "true", "on")
def set_(k):
    return k in vals and vals[k].lower() not in ("0", "no", "false", "off")
out = []
if true("NoNewPrivileges"):
    out.append("NoNewPrivileges=yes: the kernel then ignores the helper's set-id bit and every launch is refused")
if true("DynamicUser"):
    out.append("DynamicUser=yes implies NoNewPrivileges (systemd.exec; measured on systemd 259)")
# The seccomp-class settings: on systemd versions that install the filter after
# the switch to User=, an unprivileged service gets NoNewPrivileges implied by
# each of these (the classic systemd.exec rule). Measured NOT implied on
# systemd 259; refused anyway: the kit cannot know the version the unit will
# run under, and the cost of omitting them from Fabric's unit is hardening,
# while the cost of keeping them is a host that refuses every launch.
implied = ["SystemCallFilter", "SystemCallLog", "SystemCallArchitectures", "RestrictAddressFamilies",
           "RestrictNamespaces", "PrivateDevices", "ProtectKernelTunables", "ProtectKernelModules",
           "ProtectKernelLogs", "ProtectClock", "ProtectHostname", "MemoryDenyWriteExecute",
           "RestrictRealtime", "RestrictSUIDSGID", "LockPersonality"]
for k in implied:
    if set_(k):
        out.append(f"{k}={vals[k]} implies NoNewPrivileges for a non-root service on systemd versions that "
                   "apply it after the user switch (the classic systemd.exec rule): remove it from Fabric's unit")
if set_("PrivateUsers"):
    out.append(f"PrivateUsers={vals['PrivateUsers']}: in a user namespace the helper's set-id bit does not make it the host's root")
sb = vals.get("SecureBits", "")
if any(b in sb.split() for b in ("noroot", "noroot-locked", "no-setuid-fixup", "no-setuid-fixup-locked")):
    out.append(f"SecureBits={sb}: the set-id exec of the helper would not grant root's capabilities")
if "CapabilityBoundingSet" in vals:
    out.append(f"CapabilityBoundingSet={vals['CapabilityBoundingSet']}: the bounding set also bounds the setuid-root "
               "helper and the launch it runs (jailer, cgroups, network namespace); remove it from Fabric's unit")
u = vals.get("User", "")
if u != fabric_user:
    out.append(f"User={u or '(unset: root)'}, not {fabric_user}: Fabric runs as its own non-root uid (decision A)")
print("\n".join(out))
PY
)
    if [ -n "$uj" ]; then
      while IFS= read -r l; do blocked "Fabric unit $FABRIC_UNIT: $l"; done <<<"$uj"
    else
      echo "OK[$STEP] Fabric unit $FABRIC_UNIT: User=$FABRIC_USER, no NoNewPrivileges and nothing that implies it"
    fi
  fi
  if [ -z "$FABRIC_PID" ] && [ -n "$FABRIC_UNIT" ] && [ "${FABRIC_UNIT#/}" = "$FABRIC_UNIT" ] && command -v systemctl >/dev/null; then
    FABRIC_PID=$(systemctl show -p MainPID --value -- "$FABRIC_UNIT" 2>/dev/null)
    [ "$FABRIC_PID" != 0 ] || FABRIC_PID=""
    [ -n "$FABRIC_PID" ] && note "Fabric's pid for the preflight: $FABRIC_PID (MainPID of $FABRIC_UNIT)"
  fi
  if [ -z "$FABRIC_PID" ]; then
    pending "no running Fabric pid: start the Fabric unit (its MainPID), or pass --fabric-pid PID; the protected-mode preflight requires it"
  fi
fi

STEP=check
TRUST_OK=1
if selected check || selected preflight; then
  echo "== trust roots and keys (CHECKED; provisioned only by the operator)"
  tr=$(python3 -I - "$TRUST" "$SIGNER_PUB" <<'PY'
import os, stat, sys
trust, signer = sys.argv[1:3]
out, keys = [], {}
def chain_ok(p):
    st = os.lstat(p)
    return not stat.S_ISLNK(st.st_mode) and st.st_uid == 0 and not st.st_mode & 0o022
for a, required in (("qualification", True), ("observer", True), ("verifier", True),
                    ("admission", False), ("monitor", False)):
    d = os.path.join(trust, a)
    if not os.path.lexists(d):
        if required:
            out.append(f"BLOCKED {d} is absent: install -d -o root -g root -m 0755 {d}; then install -o root -g root -m 0644 <key>.pub {d}/ "
                       + {"qualification": "(the operator's B263/certification signing key's public half; private half offline)",
                          "observer": "(the axon-observer service key's public half: amendment 68; generated AS the observer uid on this host)",
                          "verifier": "(the Fabric attestation key's public half: host-signer.pub)"}[a])
        continue
    if not chain_ok(d) or not stat.S_ISDIR(os.lstat(d).st_mode):
        out.append(f"BLOCKED {d} must be a root-owned directory closed to group/other writes"); continue
    ks = []
    for n in sorted(os.listdir(d)):
        p = os.path.join(d, n)
        if not chain_ok(p): out.append(f"BLOCKED {p} must be root-owned and closed to group/other writes")
        if not n.endswith(".pub"): continue
        t = open(p).read().strip().lower()
        if len(t) != 64 or any(c not in "0123456789abcdef" for c in t):
            out.append(f"BLOCKED {p} is not a 64-hex Ed25519 public key (it refuses the whole root)")
        ks.append(t)
    if required and not ks: out.append(f"BLOCKED {d} holds no *.pub key")
    keys[a] = ks
seen = {}
for a, ks in keys.items():
    for k in ks:
        if k in seen and seen[k] != a:
            out.append(f"BLOCKED one key is in the {seen[k]} root AND the {a} root: a key serves one authority (ADR-002)")
        seen.setdefault(k, a)
if signer:
    if signer not in keys.get("verifier", []):
        out.append(f"BLOCKED the host signer's public key is not in {trust}/verifier: install it as {trust}/verifier/host-signer.pub")
    for a in ("qualification", "observer", "admission", "monitor"):
        if signer in keys.get(a, []):
            out.append(f"BLOCKED the host signer's public key is in the {a} root: Fabric holds its private half (ADR-002)")
print("\n".join(out))
PY
)
  if [ -n "$tr" ]; then
    TRUST_OK=0
    while IFS= read -r l; do blocked "${l#BLOCKED }"; done <<<"$tr"
  else
    echo "OK[$STEP] trust roots qualification/observer/verifier present, operator-owned, keys well-formed and separated; host signer only in verifier"
  fi
  [ $signer_ok = 1 ] || TRUST_OK=0
  # Amendment 68: the observer's signing key is its OWN uid's alone (0400, in
  # its own 0700 directory). The kit never reads or makes it; axon-observer
  # refuses to start on anything else, and the preflight probes it per actor.
  if [ ! -f "$OBS_KEY" ] || [ -L "$OBS_KEY" ]; then
    TRUST_OK=0
    blocked "the observer key $OBS_KEY is absent: the operator generates it AS the observer uid, on this host (the kit never makes keys): sudo -u $OBSERVER_USER $LIBEXEC/axon-fabric keygen --out $OBS_KEY; then install its public_key as $TRUST/observer/observer.pub (root 0644), in no other $TRUST root"
  else
    oks=$(stat -c '%u %a' -- "$OBS_KEY") okd=$(stat -c '%u %a' -- "$OBS_KEY_DIR")
    if [ "${oks%% *}" != "$(uid_of "$OBSERVER_USER")" ] || [ $(( 8#${oks#* } & 8#277 )) -ne 0 ]; then
      TRUST_OK=0
      blocked "the observer key $OBS_KEY must be owned by $OBSERVER_USER, readable by it alone and writable by no one (mode 0400; is uid ${oks%% *} mode ${oks#* }): a key another uid can read lets that uid mint observations (amendment 68): sudo chown $OBSERVER_USER:$OBSERVER_USER $OBS_KEY; sudo chmod 0400 $OBS_KEY"
    fi
    if [ "${okd%% *}" != "$(uid_of "$OBSERVER_USER")" ] || [ $(( 8#${okd#* } & 8#077 )) -ne 0 ]; then
      TRUST_OK=0
      blocked "the observer key directory $OBS_KEY_DIR must be $OBSERVER_USER's, mode 0700 (is uid ${okd%% *} mode ${okd#* })"
    fi
  fi
  [ -f "$TRUST/verifier.json" ] || { [ $APPLY = 0 ] && selected verifier; } || { TRUST_OK=0; blocked "$TRUST/verifier.json is absent (verifier step)"; }
fi

STEP=check
if selected check || selected preflight; then
  # Amendment 65: the helper spends each nonce only with a custodian whose
  # PROGRAM is the pin in its config; it checks the sender of every reply
  # message (SO_PASSPIDFD / SCM_PIDFD, then /proc/<pid>/exe hashed by
  # descriptor). A pin that is not the installed axon-custodian's bytes refuses
  # every launch; one that is not the program the custodian unit starts, too.
  echo "== custodian program pin (amendment 65)"
  cfg=$HELPER_CONFIG
  # --apply judges what is INSTALLED; a dry run what the plan would install.
  if [ $APPLY = 0 ] && selected configs && [ -f "$WORK/launcher.json" ]; then cfg=$WORK/launcher.json; fi
  cust_now=$LIBEXEC/axon-custodian
  # In a dry run the bytes that WOULD be installed are the source's.
  [ $APPLY = 1 ] || [ -z "$CUST_SRC" ] || cust_now=$CUST_SRC
  unit_now=$UNIT_DIR/axon-custodian.service
  if [ $APPLY = 0 ] && selected systemd && [ -f "$WORK/axon-custodian.service" ]; then unit_now=$WORK/axon-custodian.service; fi
  if [ ! -f "$cfg" ]; then
    pending "no helper config to judge yet ($HELPER_CONFIG; configs step)"
  elif [ ! -f "$cust_now" ]; then
    pending "no axon-custodian at $cust_now to judge the pin against (binaries step)"
  else
    cp_why=$(python3 -I - "$cfg" "$cust_now" "$unit_now" "$LIBEXEC/axon-custodian" <<'PY'
import hashlib, json, os, sys
cfg, prog, unit, want_exec = sys.argv[1:5]
c = json.load(open(cfg)).get("custodian") or {}
pin = c.get("sha256")
have = hashlib.sha256(open(prog, "rb").read()).hexdigest()
out = []
if not isinstance(pin, str) or len(pin) != 64 or any(x not in "0123456789abcdef" for x in pin):
    out.append(f"{cfg}: custodian.sha256 is {pin!r}, not a lowercase sha256 (the helper refuses a production config without it)")
elif pin != have:
    out.append(f"{cfg}: custodian.sha256 {pin} is not the sha256 of {prog} ({have}): every spend would be refused")
if os.path.isfile(unit):
    ex = [l.split("=", 1)[1].strip() for l in open(unit) if l.startswith("ExecStart=")]
    if not ex or not ex[-1].split() or ex[-1].split()[0] != want_exec:
        out.append(f"{unit}: ExecStart={ex[-1] if ex else '(none)'} does not start {want_exec}, the program the pin names")
print("\n".join(out))
PY
)
    if [ -n "$cp_why" ]; then
      while IFS= read -r l; do echo "FAIL[$STEP] custodian program pin: $l"; done <<<"$cp_why"; FAILED=1
    else
      echo "OK[$STEP] custodian.sha256 in $cfg is the sha256 of $cust_now, the program the custodian unit starts"
    fi
  fi
  # Amendment 79: the helper serves, and the observer names as the verifier,
  # only the Fabric PROGRAM pinned in its config. The pin must be the installed
  # axon-fabric's bytes, its path, and the build revision the INSTALLED file
  # states when it describes itself; the custodian answers the observer's
  # `check` for the observer's uid. A mismatch FAILS: the helper would refuse
  # every launch, or (revision) the observer every observation.
  echo "== fabric program pin (amendment 79)"
  fab_now=$LIBEXEC/axon-fabric
  [ $APPLY = 1 ] || [ -z "$FABRIC_SRC" ] || fab_now=$FABRIC_SRC
  ccfg=$CUSTODIAN_CONFIG
  if [ $APPLY = 0 ] && selected configs && [ -f "$WORK/custodian.json" ]; then ccfg=$WORK/custodian.json; fi
  if [ ! -f "$cfg" ]; then
    pending "no helper config to judge the Fabric pin against yet ($HELPER_CONFIG; configs step)"
  elif [ ! -f "$fab_now" ]; then
    pending "no axon-fabric at $fab_now to judge the pin against (binaries step)"
  else
    fp_why=$(python3 -I - "$cfg" "$fab_now" "$LIBEXEC/axon-fabric" "$ccfg" "$(uid_of "$OBSERVER_USER")" <<'PY'
import hashlib, json, os, subprocess, sys
cfg, prog, want_path, ccfg, obs_uid = sys.argv[1:6]
f = json.load(open(cfg)).get("fabric")
out = []
have = hashlib.sha256(open(prog, "rb").read()).hexdigest()
if not isinstance(f, dict):
    out.append(f"{cfg}: fabric is absent: the helper would serve any program of the Fabric uid")
else:
    pin = f.get("sha256")
    if not isinstance(pin, str) or len(pin) != 64 or any(x not in "0123456789abcdef" for x in pin):
        out.append(f"{cfg}: fabric.sha256 is {pin!r}, not a lowercase sha256 (the helper refuses a production config without it)")
    elif pin != have:
        out.append(f"{cfg}: fabric.sha256 {pin} is not the sha256 of {prog} ({have}): every launch and observation would be refused")
    if f.get("path") != want_path:
        out.append(f"{cfg}: fabric.path is {f.get('path')!r}, not the installed {want_path}")
    try:
        vm = json.loads(subprocess.run([prog, "verifier-manifest"], capture_output=True, check=True, timeout=60).stdout)
        if vm.get("sha256") != have:
            out.append(f"{prog} describes itself as sha256 {vm.get('sha256')}, not its bytes' {have}")
        elif f.get("revision") != vm.get("fabric_revision"):
            out.append(f"{cfg}: fabric.revision {f.get('revision')!r} is not the {vm.get('fabric_revision')!r} {prog} states: the observer would sign no manifest of this Fabric")
    except Exception as e:
        out.append(f"cannot read the build revision {prog} states: {e}")
if os.path.isfile(ccfg):
    c = json.load(open(ccfg))
    # In a dry run the observer user does not exist yet (obs_uid is empty) and the
    # planned config carries a placeholder: there is no uid to judge it against.
    if obs_uid and str(c.get("observer_uid")) != obs_uid:
        out.append(f"{ccfg}: observer_uid is {c.get('observer_uid')!r}, not the observer user's uid {obs_uid}: the custodian would answer the observer's check for no one")
print("\n".join(out))
PY
)
    if [ -n "$fp_why" ]; then
      while IFS= read -r l; do echo "FAIL[$STEP] fabric program pin: $l"; done <<<"$fp_why"; FAILED=1
    else
      echo "OK[$STEP] fabric.{path,sha256,revision} in $cfg are the installed axon-fabric's, and the custodian answers check for uid $(uid_of "$OBSERVER_USER") ($fab_now)"
    fi
  fi
  # Amendment 68: the observer SERVICE. The helper relays an observation only
  # from the socket, uid and PROGRAM its config names (observer.service); the
  # observer runs as the uid its own config names, from its socket unit. Every
  # one of those must be the same fact, and the host config names no observer
  # program (a production Fabric refuses observer.command).
  echo "== observer service pins (amendment 68)"
  ocfg=$OBSERVER_CONFIG hcfg=$HOST_CONFIG obs_now=$LIBEXEC/axon-observer
  if [ $APPLY = 0 ] && selected configs; then
    [ -f "$WORK/observer.json" ] && ocfg=$WORK/observer.json
    [ -f "$WORK/host.json" ] && hcfg=$WORK/host.json
  fi
  [ $APPLY = 1 ] || [ -z "$OBS_SRC" ] || obs_now=$OBS_SRC
  osvc=$UNIT_DIR/axon-observer.service osock=$UNIT_DIR/axon-observer.socket
  if [ $APPLY = 0 ] && selected systemd; then
    [ -f "$WORK/axon-observer.service" ] && osvc=$WORK/axon-observer.service
    [ -f "$WORK/axon-observer.socket" ] && osock=$WORK/axon-observer.socket
  fi
  if [ ! -f "$cfg" ] || [ ! -f "$ocfg" ] || [ ! -f "$hcfg" ]; then
    pending "no helper, observer or host config to judge the observer service against yet (configs step)"
  elif [ ! -f "$obs_now" ]; then
    pending "no axon-observer at $obs_now to judge the pin against (binaries step)"
  else
    op_why=$(python3 -I - "$cfg" "$ocfg" "$hcfg" "$obs_now" "$osvc" "$osock" "$LIBEXEC/axon-observer" \
      "$OBS_SOCKET" "$OBS_STORE" "$OBS_KEY" "$OBSERVER_USER" "$(uid_of "$OBSERVER_USER")" "$(uid_of "$FABRIC_USER")" <<'PY'
import hashlib, json, os, sys
(cfg, ocfg, hcfg, prog, svc, sock, want_exec, want_sock, want_store, want_key,
 obs_user, obs_uid, fab_uid) = sys.argv[1:14]
l, o, h = (json.load(open(p)) for p in (cfg, ocfg, hcfg))
have = hashlib.sha256(open(prog, "rb").read()).hexdigest()
out = []
srv = (l.get("observer") or {}).get("service")
if not isinstance(srv, dict):
    out.append(f"{cfg}: observer.service is absent: the helper relays no observation, so no protected launch can be observed")
    srv = {}
pin = srv.get("sha256")
if not isinstance(pin, str) or len(pin) != 64 or any(x not in "0123456789abcdef" for x in pin):
    out.append(f"{cfg}: observer.service.sha256 is {pin!r}, not a lowercase sha256 (the helper refuses it)")
elif pin != have:
    out.append(f"{cfg}: observer.service.sha256 {pin} is not the sha256 of {prog} ({have}): every observation would be refused")
if o.get("schema") != "axon-observer/1":
    out.append(f"{ocfg}: schema is {o.get('schema')!r}, not axon-observer/1")
for k, want in (("socket", want_sock), ("store", want_store), ("key_path", want_key)):
    if o.get(k) != want:
        out.append(f"{ocfg}: {k} is {o.get(k)!r}, not {want}")
if srv.get("socket") != o.get("socket"):
    out.append(f"{cfg}: observer.service.socket {srv.get('socket')!r} is not the observer's socket {o.get('socket')!r}")
if srv.get("uid") != o.get("observer_uid"):
    out.append(f"{cfg}: observer.service.uid {srv.get('uid')!r} is not the observer's uid {o.get('observer_uid')!r}")
if obs_uid and str(o.get("observer_uid")) != obs_uid:
    out.append(f"{ocfg}: observer_uid {o.get('observer_uid')!r} is not {obs_user}'s uid {obs_uid}")
if fab_uid and str(o.get("fabric_uid")) != fab_uid:
    out.append(f"{ocfg}: fabric_uid {o.get('fabric_uid')!r} is not the Fabric's uid {fab_uid}")
if o.get("observer_uid") in (0, o.get("fabric_uid")) or o.get("fabric_uid") == 0:
    out.append(f"{ocfg}: observer_uid {o.get('observer_uid')!r} / fabric_uid {o.get('fabric_uid')!r}: the observer is neither the Fabric nor root (amendment 68)")
if o.get("caller_uid") != 0:
    out.append(f"{ocfg}: caller_uid is {o.get('caller_uid')!r}, not 0 (only the root helper's relay asks)")
if "test_paths" in o:
    out.append(f"{ocfg}: test_paths is a test-config key")
ob = h.get("observer") or {}
for k in ("command", "interpreter"):
    if k in ob:
        out.append(f"{hcfg}: observer.{k} names an observer PROGRAM run as the Fabric uid; a production Fabric refuses it (amendment 68)")
def unit(p):
    v = {}
    if os.path.isfile(p):
        for line in open(p):
            if "=" in line and not line.lstrip().startswith(("#", ";")):
                k, x = line.split("=", 1); v[k.strip()] = x.strip()
    return v
u, s = unit(svc), unit(sock)
if os.path.isfile(svc):
    if not u.get("ExecStart") or u["ExecStart"].split()[0] != want_exec:
        out.append(f"{svc}: ExecStart={u.get('ExecStart')!r} does not start {want_exec}, the program the pin names")
    if u.get("User") != obs_user:
        out.append(f"{svc}: User={u.get('User')!r}, not {obs_user}")
if os.path.isfile(sock):
    if s.get("ListenStream") != o.get("socket"):
        out.append(f"{sock}: ListenStream={s.get('ListenStream')!r} is not the observer's socket {o.get('socket')!r}")
    for k, want in (("SocketMode", "0600"), ("SocketUser", "root"), ("SocketGroup", "root")):
        if s.get(k) != want:
            out.append(f"{sock}: {k}={s.get(k)!r}, not {want}: only root (the helper) may connect")
print("\n".join(out))
PY
)
    if [ -n "$op_why" ]; then
      while IFS= read -r l; do echo "FAIL[$STEP] observer service: $l"; done <<<"$op_why"; FAILED=1
    else
      echo "OK[$STEP] observer.service in $cfg pins $obs_now (sha256 $(sha "$obs_now")), its socket and uid are observer.json's and the units'; the host config names no observer program"
    fi
  fi
  # The RUNNING observer (once its socket has activated it) is the pinned program.
  if [ $APPLY = 1 ] && [ $NO_SYSTEMCTL = 0 ] && command -v systemctl >/dev/null; then
    opid=$(systemctl show -p MainPID --value axon-observer.service 2>/dev/null)
    if [ -n "$opid" ] && [ "$opid" != 0 ]; then
      opin=$(python3 -I -c 'import json,sys; print((json.load(open(sys.argv[1])).get("observer") or {}).get("service",{}).get("sha256",""))' "$cfg" 2>/dev/null)
      if [ "$(sha "/proc/$opid/exe" 2>/dev/null)" = "$opin" ]; then echo "OK[$STEP] the running axon-observer (pid $opid) is the pinned program"
      else echo "FAIL[$STEP] the running axon-observer (pid $opid) is not the program observer.service.sha256 pins"; FAILED=1; fi
    else
      note "axon-observer.service is not running yet (its socket starts it at the helper's first --observe); the helper checks the program on every reply"
    fi
  fi
  # SO_PASSPIDFD / SCM_PIDFD: Linux 6.5. Without it every pinned call is refused.
  kv=$(uname -r); kmaj=${kv%%.*}; kmin=${kv#*.}; kmin=${kmin%%[!0-9]*}
  if [ "${kmaj:-0}" -lt 6 ] || { [ "$kmaj" = 6 ] && [ "${kmin:-0}" -lt 5 ]; }; then
    blocked "kernel $kv has no SO_PASSPIDFD/SCM_PIDFD (Linux >= 6.5): the helper refuses every pinned custodian call (amendment 65)"
  else
    echo "OK[$STEP] kernel $kv has SO_PASSPIDFD/SCM_PIDFD (>= 6.5): the custodian program check can run"
  fi
fi

STEP=preflight
PF_VERDICT=NOT_RUN PF_REPORT=""
if selected preflight; then
  echo "== trust preflight: scripts/trust_root_preflight.sh in PROTECTED mode"
  pf=("$CLONE/scripts/trust_root_preflight.sh" --verifier "$VERIFIER_USER" --custodian "$CUSTODIAN_USER" --fabric "$FABRIC_USER"
      --observer "$OBSERVER_USER")
  for a in "${AGENTS[@]}"; do pf+=(--agent "$a"); done
  PF_REPORT=$DEPLOY_LOG/trust-preflight-$(date -u +%Y%m%dT%H%M%SZ).json
  # Amendment 65: protected mode requires --fabric-pid and FAILS unless that
  # process is the Fabric uid with NoNewPrivs 0 (the fabric-unit step finds it).
  pf+=(--fabric-pid "${FABRIC_PID:-<--fabric-pid>}")
  pf+=(--guest-cmd "${GUEST_CMD:-<--guest-cmd>}" --out "$PF_REPORT")
  if [ ${#AGENTS[@]} = 0 ] || [ -z "$GUEST_CMD" ]; then
    pending "the preflight needs --agent U (every uid an agent runs as: MiCode, Claude) and --guest-cmd CMD (runs scripts/trust_root_guest_probe.sh INSIDE a candidate guest and prints its JSON line)"
  fi
  if [ $APPLY = 0 ]; then
    echo "PLAN[$STEP] bash ${pf[*]}"
  elif [ $TRUST_OK = 0 ] || [ ${#AGENTS[@]} = 0 ] || [ -z "$GUEST_CMD" ] || [ -z "$FABRIC_PID" ] || [ ${#BLOCKED[@]} -gt 0 ]; then
    pending "preflight not run: provision the BLOCKED items above, then re-run (the kit is idempotent)"
  else
    echo "DO[$STEP] bash ${pf[*]}"
    bash "${pf[@]}" >"$WORK/preflight.out" 2>&1; prc=$?
    PF_VERDICT=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1])).get("verdict","?"))' "$PF_REPORT" 2>/dev/null || echo NOT_RUN)
    if [ $prc != 0 ] || [ "$PF_VERDICT" != PASS ]; then
      FAILED=1
      echo "FAIL[$STEP] trust preflight verdict $PF_VERDICT (exit $prc); failing checks:"
      python3 -I -c '
import json, sys
r = json.load(open(sys.argv[1]))
for c in r.get("checks", []):
    if not c.get("ok"): print("  ", c["actor"], c["action"], c["target"], "expected", c["expected"], "observed", c["observed"])' "$PF_REPORT" 2>/dev/null \
        || tail -n 5 "$WORK/preflight.out"
    fi
    echo "PREFLIGHT verdict $PF_VERDICT mode protected report $PF_REPORT ($(sha "$PF_REPORT" 2>/dev/null))"
  fi
fi

# ── record and summary ────────────────────────────────────────────────────────
STEP=record
if [ $APPLY = 1 ] && [ -d "$DEPLOY_LOG" ]; then
  rec=$DEPLOY_LOG/deploy-$(date -u +%Y%m%dT%H%M%SZ).json
  python3 -I - "$rec" "$COMMIT" "$CLONE" "$PF_VERDICT" "$PF_REPORT" "$FAILED" \
    "${BLOCKED[@]+"${BLOCKED[@]}"}" -- "${PENDING[@]+"${PENDING[@]}"}" <<'PY' && echo "RECORD $rec"
import hashlib, json, os, sys
rec, commit, clone, verdict, report, failed, *rest = sys.argv[1:]
i = rest.index("--"); blocked, pending = rest[:i], rest[i + 1:]
paths = ["/etc/axon/provenance-allowlist", "/etc/axon/custodian.json", "/etc/axon/observer.json",
         "/etc/axon/protected-launcher.json",
         "/etc/axon/protected-host.json", "/etc/axon/trust/verifier.json", "/etc/axon/host-toolchain-pin.json",
         "/usr/local/libexec/axon/axon-fabric", "/usr/local/libexec/axon/axon-protected-launcher",
         "/usr/local/libexec/axon/axon-custodian", "/usr/local/libexec/axon/fc_linux_profile.sh",
         "/usr/local/libexec/axon/axon-observer", "/usr/local/lib/axon/guest-linux/vmlinux",
         "/usr/local/lib/axon/guest-linux/rootfs.sqfs", "/usr/local/lib/axon/guest-linux/manifest.json",
         "/etc/systemd/system/axon-custodian.socket", "/etc/systemd/system/axon-custodian.service",
         "/etc/systemd/system/axon-observer.socket", "/etc/systemd/system/axon-observer.service"]
def entry(p):
    st = os.lstat(p)
    return {"sha256": hashlib.sha256(open(p, "rb").read()).hexdigest(), "uid": st.st_uid,
            "gid": st.st_gid, "mode": oct(st.st_mode & 0o7777)}
doc = {"schema": "axon-operator-deploy-record/1", "commit": commit, "clone": clone,
       "installed": {p: entry(p) for p in paths if os.path.isfile(p)},
       "preflight": {"verdict": verdict, "report": report or None},
       "failed": failed == "1", "blocked": blocked, "pending": pending}
json.dump(doc, open(rec, "w"), indent=2); open(rec, "a").write("\n")
PY
fi
echo "== summary ($MODE, commit $COMMIT)"
for b in "${BLOCKED[@]+"${BLOCKED[@]}"}"; do echo "  BLOCKED: $b"; done
for p in "${PENDING[@]+"${PENDING[@]}"}"; do echo "  PENDING: $p"; done
if [ $FAILED = 1 ]; then echo "operator_deploy: FAILED — see FAIL lines above"; exit 1; fi
if [ ${#BLOCKED[@]} -gt 0 ] || [ ${#PENDING[@]} -gt 0 ]; then
  echo "operator_deploy: INCOMPLETE — ${#BLOCKED[@]} blocked, ${#PENDING[@]} pending (operator action above; re-run, it is idempotent)"
  exit 3
fi
if [ $APPLY = 1 ]; then echo "operator_deploy: DEPLOYED — trust preflight $PF_VERDICT"
else echo "operator_deploy: PLAN COMPLETE — review it, then re-run with --apply as root"; fi
exit 0
