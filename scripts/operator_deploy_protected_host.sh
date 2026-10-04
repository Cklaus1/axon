#!/usr/bin/env bash
# operator_deploy_protected_host.sh — the OPERATOR deployment kit for the Axon
# v0.22 protected host (Candidate C9). DRY-RUN BY DEFAULT.
#
# It installs, from ONE standalone clone at ONE named commit, everything the
# protected host needs that is not a key or a signature:
#
#   allowlist  /etc/axon/provenance-allowlist (amendment 44, decision C)
#   users      the Fabric, custodian, verifier and launch-profile system users
#              (own uid and group each, no login; amendments 45, 50)
#   dirs       out root, root-private staging root, custodian store, authority
#              store parent, /etc/axon layout, install dirs
#   binaries   axon-fabric (the readiness verifier and Fabric), the setuid-root
#              axon-protected-launcher (04750 root:<fabric group>), axon-custodian,
#              fc_linux_profile.sh, the operator's observer program
#   guest      vmlinux, rootfs.sqfs and the profile manifest (amendments 54, 56, 63)
#   data       the operator suite registry, grant registry and grant files; the
#              signed B263 record and waivers, when the operator hands them over
#   configs    /etc/axon/custodian.json (axon-custodian/1),
#              /etc/axon/protected-launcher.json (axon-protected-launcher/2),
#              /etc/axon/protected-host.json (axon-protected-host/1), every pin
#              computed from the INSTALLED bytes
#   verifier   /etc/axon/trust/verifier.json from the installed verifier's own
#              `verifier-manifest`
#   systemd    axon-custodian.socket + .service, socket enabled
#   loader     the production ProtectedHost::operator() run as the Fabric uid
#              against what was just deployed (a read; it launches nothing)
#   toolchain  /etc/axon/host-toolchain-pin.json: the host tool identities the
#              guest build recorded (amendment 63's operator item; NO READER yet)
#   check      the trust roots and the attestation key: present, owned, moded,
#              separated — CHECKED, never created
#   preflight  scripts/trust_root_preflight.sh in protected mode; its verdict
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
#                           axon-protected-launcher and axon-custodian, built FROM
#                           CLONE at its HEAD (checked through the verifier's
#                           own report)
#   --observer-bin FILE     the operator's observer program (no production
#                           observer ships in this repository)
#   --observer-interpreter FILE   its interpreter, if it is a #! script
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
#   --fabric-user U --custodian-user U --verifier-user U --profile-user U
#                           defaults axon-fabric, axon-custodian, axon-verifier, axonb263
#   --agent U               an agent uid/user for the preflight (repeatable)
#   --guest-cmd CMD         the preflight's in-guest probe command
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
QUAL_DIR=$ETC/qualification
KEYS_DIR=$ETC/keys
SUITES_DIR=$ETC/suites
GRANTS_DIR=$ETC/grants
TOOLCHAIN_PIN=$ETC/host-toolchain-pin.json
DEPLOY_LOG=/var/lib/axon-deploy
UNIT_DIR=/etc/systemd/system
OBS_MAX_AGE_S=300
NONCE_MAX_AGE_S=300

STEPS="allowlist users dirs binaries guest data configs verifier systemd loader toolchain check preflight"
CLONE="" EXPECT="" BIN_DIR="" OBSERVER_BIN="" OBSERVER_INTERP="" SUITE_REG="" GRANT_REG=""
SIGNER_PUB="" SIGNER_KEY=$KEYS_DIR/fabric-attest.pk8 ISSUER_REF="verifier:fabric"
AUTH_STORE=/var/lib/axon-loop/store B263_RECORD="" B263_WAIVERS=""
QUAL_MAX_AGE=2592000 MAX_TIMEOUT=900 MAX_INPUT=268435456
FABRIC_USER=axon-fabric CUSTODIAN_USER=axon-custodian VERIFIER_USER=axon-verifier PROFILE_USER=axonb263
AGENTS=() GUEST_CMD="" ONLY="" NO_SYSTEMCTL=0 APPLY=0

refuse() { echo "REFUSED: $*" >&2; echo "operator_deploy: REFUSED — $*"; exit 2; }
need_arg() { [ $# -ge 2 ] && [ -n "$2" ] || refuse "$1 needs a value"; }
while [ $# -gt 0 ]; do
  case "$1" in
    --from) need_arg "$@"; CLONE=$2; shift 2 ;;
    --expect-commit) need_arg "$@"; EXPECT=$2; shift 2 ;;
    --bin-dir) need_arg "$@"; BIN_DIR=$2; shift 2 ;;
    --observer-bin) need_arg "$@"; OBSERVER_BIN=$2; shift 2 ;;
    --observer-interpreter) need_arg "$@"; OBSERVER_INTERP=$2; shift 2 ;;
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
    --verifier-user) need_arg "$@"; VERIFIER_USER=$2; shift 2 ;;
    --profile-user) need_arg "$@"; PROFILE_USER=$2; shift 2 ;;
    --agent) need_arg "$@"; AGENTS+=("$2"); shift 2 ;;
    --guest-cmd) need_arg "$@"; GUEST_CMD=$2; shift 2 ;;
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
for u in "$FABRIC_USER" "$CUSTODIAN_USER" "$VERIFIER_USER" "$PROFILE_USER"; do
  case "$u" in ''|*[!a-z0-9_-]*|[!a-z_]*) refuse "user name $u is not a plain system user name" ;; esac
done
[ "$(printf '%s\n' "$FABRIC_USER" "$CUSTODIAN_USER" "$VERIFIER_USER" "$PROFILE_USER" | sort -u | wc -l)" = 4 ] \
  || refuse "the Fabric, custodian, verifier and profile users must be four different users"
if [ -n "$SIGNER_PUB" ]; then
  SIGNER_PUB=$(printf '%s' "$SIGNER_PUB" | tr 'A-F' 'a-f')
  [[ "$SIGNER_PUB" =~ ^[0-9a-f]{64}$ ]] || refuse "--signer-public-key is not 64 hex characters"
fi
case "$ISSUER_REF" in *[[:space:]]*|'') refuse "--signer-issuer-ref must be one word" ;; esac
case "$AUTH_STORE" in /*) ;; *) refuse "--authority-store must be absolute" ;; esac
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
FABRIC_SRC="" HELPER_SRC="" CUST_SRC=""
need_bins=0
for s in binaries configs verifier loader check preflight; do selected "$s" && need_bins=1; done
if [ $need_bins = 1 ]; then
  [ -n "$BIN_DIR" ] || refuse "--bin-dir DIR is required (the release build of axon-fabric's bins from $CLONE)"
  BIN_DIR=$(cd "$BIN_DIR" 2>/dev/null && pwd -P) || refuse "--bin-dir: not a directory"
  FABRIC_SRC=$BIN_DIR/axon-fabric HELPER_SRC=$BIN_DIR/axon-protected-launcher CUST_SRC=$BIN_DIR/axon-custodian
  for b in "$FABRIC_SRC" "$HELPER_SRC" "$CUST_SRC"; do
    [ -f "$b" ] && [ -x "$b" ] || refuse "$b is not an executable file"
  done
  vm=$("$FABRIC_SRC" verifier-manifest 2>/dev/null) || refuse "$FABRIC_SRC verifier-manifest failed"
  why=$(printf '%s' "$vm" | python3 -I -c '
import json, sys
m, want = json.load(sys.stdin), sys.argv[1]
bad = [f"{k}={m.get(k)!r}" for k, v in (("build", "production"), ("profile", "release"),
       ("source_dirty", False), ("fabric_revision", want)) if m.get(k) != v]
print("; ".join(bad))' "$COMMIT") || refuse "cannot read $FABRIC_SRC's verifier-manifest"
  [ -z "$why" ] || refuse "$FABRIC_SRC is not a clean production release build of $COMMIT ($why)"
  pb=$("$HELPER_SRC" --probe 2>/dev/null | python3 -I -c 'import json,sys; print(json.load(sys.stdin).get("build"))' 2>/dev/null)
  [ "$pb" = production ] || refuse "$HELPER_SRC --probe reports build ${pb:-none}, not production (a test-trust helper accepts a caller's --test-config)"
  # A production custodian has no --test-config (it answers with its usage).
  co=$("$CUST_SRC" --test-config /nonexistent 2>&1)
  case "$co" in *"usage: axon-custodian"*) ;; *) refuse "$CUST_SRC accepts --test-config: a test-trust build ($co)" ;; esac
  echo "BINARIES $BIN_DIR: axon-fabric $(sha "$FABRIC_SRC"), helper $(sha "$HELPER_SRC"), custodian $(sha "$CUST_SRC") (production release of $COMMIT)"
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
  act_group "$VERIFIER_USER"; act_user "$VERIFIER_USER" "Axon readiness verifier actor (trust preflight)"
  act_group "$PROFILE_USER"; act_user "$PROFILE_USER" "Axon B263 launch profile (jailer uid)"
fi
FU=$(ph_uid "$FABRIC_USER") CU=$(ph_uid "$CUSTODIAN_USER")

STEP=dirs
if selected dirs; then
  echo "== directories"
  for p in "$ETC" "$LIBEXEC" "$GUEST_DIR" "$OUT_ROOT" "$STAGING" "$STORE" "$AUTH_STORE" "$UNIT_DIR/x"; do
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
  # A (amendment 45): setuid-root, executable only by the Fabric group.
  act_install "$HELPER_SRC" "$LIBEXEC/axon-protected-launcher" root "$FABRIC_USER" 4750
  act_install "$LAUNCHER_SRC" "$LIBEXEC/fc_linux_profile.sh" root root 0755
  note "launcher interpreter pinned: $BASH_PIN sha256 $(sha "$BASH_PIN") (the system bash; not installed by the kit)"
  [ "$(stat -c '%u %a' "$BASH_PIN")" != "" ] && [ "$(stat -c %u "$BASH_PIN")" = 0 ] \
    || blocked "$BASH_PIN is not root-owned"
  if [ -z "$OBSERVER_BIN" ]; then
    blocked "--observer-bin FILE is required: the operator's observer program (PROGRAM --manifest FILE --out DIR writes observation.json + .sig under its OWN key). No production observer ships in this repository; the tests' stand-in proves only the protocol"
  else
    [ -f "$OBSERVER_BIN" ] || refuse "--observer-bin $OBSERVER_BIN is not a file"
    act_install "$OBSERVER_BIN" "$LIBEXEC/axon-observer" root root 0755
    if [ "$(head -c2 "$OBSERVER_BIN")" = '#!' ] && [ -z "$OBSERVER_INTERP" ]; then
      blocked "the observer is a #! script: sealed_exec never runs a script through its #! line; pass --observer-interpreter FILE (it is pinned too)"
    fi
    if [ -n "$OBSERVER_INTERP" ]; then
      [ -f "$OBSERVER_INTERP" ] && [ "$(stat -c %u "$OBSERVER_INTERP")" = 0 ] \
        || blocked "--observer-interpreter $OBSERVER_INTERP is not a root-owned file"
    fi
  fi
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
    gw=$(python3 -I - "$MANIFEST_SRC" "$CLONE/dist/guest-linux" "$FIRECRACKER" "$JAILER" <<'PY'
import hashlib, json, os, sys
man, dist, fc, jl = sys.argv[1:5]
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
    pending "no B263 qualification record at $QUAL_DIR/b263.json: run scripts/b263_qualify.sh from the clone, have the operator sign it, re-run with --b263-record (Fabric refuses every protected launch until then)"
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
  if [ "${ks%% *}" != "$(uid_of "$FABRIC_USER")" ] || [ $(( 8#${ks#* } & 8#077 )) -ne 0 ]; then
    signer_ok=0
    if selected configs || selected check; then
      blocked "$SIGNER_KEY must be owned by $FABRIC_USER and readable by it alone (A20; is uid ${ks%% *} mode ${ks#* }): sudo chown $FABRIC_USER:$FABRIC_USER $SIGNER_KEY; sudo chmod 0400 $SIGNER_KEY"
    fi
  fi
fi
gen_configs() { # writes $WORK/{custodian,launcher,host}.json from the pins
  python3 -I - "$WORK" <<'PY'
import json, os, sys
w = sys.argv[1]; e = os.environ
def num(v):
    return int(v) if v.isdigit() else v   # a dry-run placeholder stays a visible string
def pin(p, s): return {"path": p, "sha256": s}
custodian = {"schema": "axon-custodian/1", "custodian_uid": num(e["K_CU"]), "fabric_uid": num(e["K_FU"]),
             "launcher_uid": 0, "socket": e["K_SOCKET"], "store": e["K_STORE"],
             "max_age_s": int(e["K_NONCE_AGE"])}
launcher = {"schema": "axon-protected-launcher/2", "fabric_uid": num(e["K_FU"]),
            "interpreter": pin(e["K_BASH"], e["K_BASH_SHA"]),
            "launcher": pin(e["K_LAUNCHER"], e["K_LAUNCHER_SHA"]),
            "profile_manifest": pin(e["K_MANIFEST"], e["K_MANIFEST_SHA"]),
            "artifacts_dir": e["K_GUEST"], "firecracker": e["K_FC"], "jailer": e["K_JL"],
            "out_root": e["K_OUT"], "staging_root": e["K_STAGING"],
            "max_timeout_s": int(e["K_MAX_TIMEOUT"]), "max_input_bytes": int(e["K_MAX_INPUT"]),
            "observer": {"root": e["K_OBS_ROOT"], "max_age_s": int(e["K_OBS_AGE"]),
                         "host_signer_public_key": e["K_SIGNER_PUB"]},
            "custodian": {"socket": e["K_SOCKET"], "uid": num(e["K_CU"])}}
observer = {"command": pin(e["K_OBSERVER"], e["K_OBSERVER_SHA"])}
if e.get("K_OBS_INTERP"):
    observer["interpreter"] = pin(e["K_OBS_INTERP"], e["K_OBS_INTERP_SHA"])
observer["custodian"] = {"socket": e["K_SOCKET"], "uid": num(e["K_CU"])}
observer["max_age_s"] = int(e["K_OBS_AGE"])
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
for name, doc in (("custodian", custodian), ("launcher", launcher), ("host", host)):
    with open(os.path.join(w, name + ".json"), "w") as f:
        json.dump(doc, f, indent=2); f.write("\n")
PY
}
if selected configs; then
  echo "== configs (root-owned 0644; every pin from the INSTALLED bytes)"
  for p in "$CUSTODIAN_CONFIG" "$HELPER_CONFIG" "$HOST_CONFIG"; do operator_chain "$p"; done
  suite_dst=$SUITES_DIR/$(basename "${SUITE_REG:-registry.json}")
  grant_dst=$GRANTS_DIR/$(basename "${GRANT_REG:-grants.json}")
  stop=0
  [ $signer_ok = 1 ] || stop=1
  [ -n "$OBSERVER_BIN" ] || stop=1
  [ -n "$SUITE_REG" ] && [ -n "$GRANT_REG" ] || stop=1
  [ $GUEST_OK = 1 ] || stop=1
  if [ $stop = 1 ] && [ $APPLY = 1 ]; then
    blocked "configs not written: a signer, observer, registry or guest item above is BLOCKED (the host config would name what is not there)"
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
    K_OBSERVER=$LIBEXEC/axon-observer; K_OBSERVER_SHA=$(ph "$OBSERVER_BIN" "$K_OBSERVER")
    K_OBS_INTERP=$OBSERVER_INTERP; K_OBS_INTERP_SHA=""
    [ -z "$OBSERVER_INTERP" ] || K_OBS_INTERP_SHA=$(sha "$OBSERVER_INTERP")
    K_B263=$QUAL_DIR/b263.json; K_WAIVERS=""
    [ -z "$WAIVERS_SRC" ] || K_WAIVERS=$QUAL_DIR/b263-waivers.json
    K_QUAL_AGE=$QUAL_MAX_AGE K_SUITES=$suite_dst; K_SUITES_SHA=$(ph "$SUITE_REG" "$suite_dst")
    K_ISSUER=$ISSUER_REF K_KEY=$SIGNER_KEY K_GRANTS=$grant_dst; K_GRANTS_SHA=$(ph "$GRANT_REG" "$grant_dst")
    K_AUTH=$AUTH_STORE
    export K_CU K_FU K_SOCKET K_STORE K_NONCE_AGE K_BASH K_BASH_SHA K_LAUNCHER K_LAUNCHER_SHA K_MANIFEST \
      K_MANIFEST_SHA K_GUEST K_FC K_JL K_OUT K_STAGING K_MAX_TIMEOUT K_MAX_INPUT K_OBS_ROOT K_OBS_AGE \
      K_SIGNER_PUB K_HELPER K_HELPER_SHA K_OBSERVER K_OBSERVER_SHA K_OBS_INTERP K_OBS_INTERP_SHA K_B263 \
      K_WAIVERS K_QUAL_AGE K_SUITES K_SUITES_SHA K_ISSUER K_KEY K_GRANTS K_GRANTS_SHA K_AUTH
    gen_configs || { echo "FAIL[$STEP] generating the configs"; FAILED=1; }
    act_install "$WORK/custodian.json" "$CUSTODIAN_CONFIG" root root 0644 show
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
  act_install "$WORK/axon-custodian.socket" "$UNIT_DIR/axon-custodian.socket" root root 0644 show
  act_install "$WORK/axon-custodian.service" "$UNIT_DIR/axon-custodian.service" root root 0644 show
  for c in "systemctl daemon-reload" "systemctl enable --now axon-custodian.socket"; do
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
  echo "== host toolchain pin: $TOOLCHAIN_PIN (amendment 63 operator item)"
  if [ $GUEST_OK = 1 ]; then
    python3 -I - "$MANIFEST_SRC" "$COMMIT" "$WORK/toolchain.json" <<'PY' || { echo "FAIL[$STEP] cannot compute the toolchain pin"; FAILED=1; }
import hashlib, json, os, sys
man, commit, out = sys.argv[1:4]
mb = open(man, "rb").read(); m = json.loads(mb)
def sha(p):
    try:
        h = hashlib.sha256()
        with open(p, "rb") as f:
            for b in iter(lambda: f.read(1 << 20), b""): h.update(b)
        return h.hexdigest()
    except OSError:
        return None
src = m["source"]["build_environment"]; tc = src["toolchain"]
tools = {}
for name, t in sorted((m["kernel"]["build_environment"].get("tools") or {}).items()):
    tools[name] = dict(t, recorded_by="kernel.build_environment.tools")
for name, t in sorted((tc.get("host_tools") or {}).items()):
    tools.setdefault(name, dict(t, recorded_by="source.build_environment.toolchain.host_tools"))
rt = (src.get("rootfs") or {}).get("tool")
if rt: tools["mksquashfs"] = dict(rt, recorded_by="source.build_environment.rootfs.tool")
tools["rustc"] = {"path": tc["rustc"], "sha256": tc["rustc_sha256"], "version": tc["rustc_vV"].splitlines()[0],
                  "recorded_by": "source.build_environment.toolchain"}
tools["cargo"] = {"path": tc["cargo"], "sha256": tc["cargo_sha256"], "version": tc["cargo_version"],
                  "recorded_by": "source.build_environment.toolchain"}
drift = {}
for name, t in tools.items():
    now = sha(t["path"])
    if now != t["sha256"]:
        drift[name] = {"recorded": t["sha256"], "now": now}
doc = {"schema": "axon-host-toolchain-pin/1",
       "status": "PROPOSED: no Axon code reads this file yet (see governance/notes/v022-operator-runbook.md)",
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
    note "no reader: nothing in the code verifies a build against this pin yet (follow-up in the runbook)"
  else
    blocked "the toolchain pin needs the deployed guest manifest's build records (guest step)"
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
                          "observer": "(the observer program's signing key's public half)",
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
  [ -f "$TRUST/verifier.json" ] || { [ $APPLY = 0 ] && selected verifier; } || { TRUST_OK=0; blocked "$TRUST/verifier.json is absent (verifier step)"; }
fi

STEP=preflight
PF_VERDICT=NOT_RUN PF_REPORT=""
if selected preflight; then
  echo "== trust preflight: scripts/trust_root_preflight.sh in PROTECTED mode"
  pf=("$CLONE/scripts/trust_root_preflight.sh" --verifier "$VERIFIER_USER" --custodian "$CUSTODIAN_USER" --fabric "$FABRIC_USER")
  for a in "${AGENTS[@]}"; do pf+=(--agent "$a"); done
  PF_REPORT=$DEPLOY_LOG/trust-preflight-$(date -u +%Y%m%dT%H%M%SZ).json
  pf+=(--guest-cmd "${GUEST_CMD:-<--guest-cmd>}" --out "$PF_REPORT")
  if [ ${#AGENTS[@]} = 0 ] || [ -z "$GUEST_CMD" ]; then
    pending "the preflight needs --agent U (every uid an agent runs as: MiCode, Claude) and --guest-cmd CMD (runs scripts/trust_root_guest_probe.sh INSIDE a candidate guest and prints its JSON line)"
  fi
  if [ $APPLY = 0 ]; then
    echo "PLAN[$STEP] bash ${pf[*]}"
  elif [ $TRUST_OK = 0 ] || [ ${#AGENTS[@]} = 0 ] || [ -z "$GUEST_CMD" ] || [ ${#BLOCKED[@]} -gt 0 ]; then
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
paths = ["/etc/axon/provenance-allowlist", "/etc/axon/custodian.json", "/etc/axon/protected-launcher.json",
         "/etc/axon/protected-host.json", "/etc/axon/trust/verifier.json", "/etc/axon/host-toolchain-pin.json",
         "/usr/local/libexec/axon/axon-fabric", "/usr/local/libexec/axon/axon-protected-launcher",
         "/usr/local/libexec/axon/axon-custodian", "/usr/local/libexec/axon/fc_linux_profile.sh",
         "/usr/local/libexec/axon/axon-observer", "/usr/local/lib/axon/guest-linux/vmlinux",
         "/usr/local/lib/axon/guest-linux/rootfs.sqfs", "/usr/local/lib/axon/guest-linux/manifest.json",
         "/etc/systemd/system/axon-custodian.socket", "/etc/systemd/system/axon-custodian.service"]
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
