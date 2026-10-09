#!/usr/bin/env bash
# test_operator_deploy.sh — the operator deployment kit
# (scripts/operator_deploy_protected_host.sh), judged two ways:
#
#  1. DRY RUN on this host (writes nothing; the host's state is compared before
#     and after): it plans every required action with the exact modes, owners
#     and pins, and it REFUSES a dirty clone, a linked worktree, a symlinked
#     .git, a kit that is not the clone's own copy, binaries that are not a
#     clean production build of the clone's commit, and --apply as non-root.
#  2. APPLY inside a private mount namespace (root + unshare only): /etc is a
#     tmpfs COPY of the host's /etc, and /usr/local, /var/lib, /var/log,
#     /var/spool, /var/mail, /run and /srv are empty tmpfs mounts, so the real
#     install path runs (useradd included) and nothing reaches the host. The
#     test then requires: the production ProtectedHost::operator() accepts the
#     deployment, scripts/trust_root_preflight.sh passes in PROTECTED mode, the
#     production custodian starts on the deployed config, a second --apply
#     changes nothing, and the example configs parse with the code's schemas.
#     The trust-root "keys" there are random hex strings and the "attestation
#     key" one byte: fixtures that are never keys, never signed with, and
#     vanish with the namespace. Nothing is signed.
#
# It builds the host binaries itself with the CONTROLLED host build
# (guest_build_env.py host-build: fresh clone, fresh empty target, from scratch
# every run -- minutes, and the network for crates). Without root or
# unshare part 2 is reported NOT_RUN (not a pass).
#
# Exit 0 = every assertion held; 1 = an assertion failed; 2 = could not run.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
WORK=$(mktemp -d "${TMPDIR:-/var/tmp}/axon-opkit-test.XXXXXX") || exit 2
chmod 0755 "$WORK"
trap 'rm -rf "$WORK"' EXIT
fail() { echo "FAIL: $*"; exit 1; }
ok() { echo "ok: $*"; }
G() { git -c user.name=opkit-test -c user.email=opkit-test@example.invalid "$@"; }

# ── a scratch standalone clone carrying THIS tree's kit and a synthetic image ─
CLONE=$WORK/clone
git clone -q "$REPO" "$CLONE" || { echo "cannot clone $REPO"; exit 2; }
for f in scripts/operator_deploy_protected_host.sh scripts/trust_root_preflight.sh \
         scripts/trust_root_guest_probe.sh profiles/protected-host; do
  rm -rf "${CLONE:?}/$f"; cp -a "$REPO/$f" "$CLONE/$f"
done
# dist/ is written into the clone only AFTER the binaries are built: this host
# has no /etc/axon/provenance-allowlist, so an in-tree dist/ would make the
# verifier's build provenance dirty (on the protected host the allowlist
# excuses it; runbook step 1 installs it first).
DIST=$WORK/dist-staging/guest-linux
mkdir -p "$DIST"
head -c 4096 /dev/urandom >"$DIST/vmlinux"
head -c 4096 /dev/urandom >"$DIST/rootfs.sqfs"
# Round 5 (amendment 80): the kit judges the guest build records with the
# freeze's own judge, so the synthetic image carries records that judge accepts:
# built from the clone's own guest_build_env.py (the pinned toolchain, the real
# host tools, a builder-private parent under this user's home) and SIGNED by it
# with a key made for the fixture. Only the artifacts' bytes are synthetic.
# Round 7: the build processes run as an unprivileged uid, so the builder-private
# parent lives where that uid can traverse (/var/lib), never under root's home;
# and the controlled build needs root to start them. Not root: NOT_RUN.
if [ "$(id -u)" != 0 ]; then
  echo "NOT_RUN: the controlled host build starts its build processes as an unprivileged uid and needs root"
  exit 0
fi
BUILD_UID=65534
# Amendment 97: the builder-private parent lives INSIDE the namespaces only (/var/lib is a tmpfs there);
# nothing is created under the host's /var/lib. The fixture hands it out through $KEYSTASH.
KEYPARENT=/var/lib/axon-opkit-keys
# The stash is NOT under $WORK: `chmod -R a+rX "$WORK"` below would make the proof keys world-readable.
STASHDIR=$(mktemp -d "${TMPDIR:-/var/tmp}/axon-opkit-stash.XXXXXX") || exit 2
chmod 0700 "$STASHDIR"
trap 'rm -rf "$WORK" "$STASHDIR"' EXIT
KEYSTASH=$STASHDIR/keys
# Amendment 92: EVERY kit invocation below goes through ns_run (scripts/lib/opkit_ns.sh), which
# runs it under mount+PID+UTS+IPC+NET namespaces with a tmpfs over every destination the kit can reach and REFUSES
# (exit 97, the kit never starts) unless that isolation is proved first. A kit guard that is
# missing or regressed therefore turns the attack into an install INSIDE the namespace.
# Amendment 97: so does every other root step of this test (the synthetic image, the controlled host
# build, the forged record): scripts/opkit_ns_drift.py fails the build if a test script runs the kit,
# `--apply` or a controlled-build verb any other way. The overrides are honoured by test_opkit_ns.sh only.
[ -z "${OPKIT_DESTS_FOR_TEST:-}${OPKIT_NS_PID_FOR_TEST:-}${OPKIT_VIEW_PID_FOR_TEST:-}" ] || { echo "REFUSE: an OPKIT_*_FOR_TEST override is set; the kit test proves the REAL destinations"; exit 2; }
# Amendment 101: the namespace's root is read-only (deny by default); the only host directories a step may
# write are the two this test made for it.
# Amendment 109: ns_run refuses a socket on fds 0-2; an ssh-driven run can hand this script one for stdin (nothing below reads it).
[ ! -S /proc/self/fd/0 ] || exec </dev/null
export OPKIT_LIB=$HERE/lib/opkit_ns.sh OPKIT_SCRATCH=$WORK/scratch OPKIT_RW="$WORK $STASHDIR"
mkdir "$OPKIT_SCRATCH" || exit 2
. "$OPKIT_LIB"
if [ -f /usr/local/bin/firecracker ] && [ -f /usr/local/bin/jailer ]; then
  cp /usr/local/bin/firecracker /usr/local/bin/jailer "$WORK/" && export OPKIT_EXTRA="$WORK/firecracker $WORK/jailer"
fi
HOSTOUT=$WORK/hostbuild
BUILDER_UID=$(id -u)
# The fixture needs the network (the host build downloads its crates): OPKIT_NET=host, the one namespace it shares.
OPKIT_CAPS_KEEP=sys_admin OPKIT_NET=host ns_run bash "$HERE/lib/opkit_fixture.sh" "$CLONE" "$DIST" "$KEYPARENT" "$BUILD_UID" "$HOSTOUT" "$KEYSTASH" "$WORK/commit.txt" \
  || { echo "the synthetic image / controlled host build failed in the namespace"; exit 2; }
COMMIT=$(cat "$WORK/commit.txt")
export OPKIT_RESTORE="$KEYSTASH=$KEYPARENT"
BIN=$HOSTOUT
mkdir -p "$CLONE/dist" && cp -a "$DIST" "$CLONE/dist/guest-linux"

# ── operator data fixtures ─────────────────────────────────────────────────────
OP=$WORK/op
mkdir -p "$OP/suites/bin" "$OP/suites/checks/c1" "$OP/grants"
cp /usr/bin/true "$OP/suites/bin/axon"; echo '// fixture' >"$OP/suites/checks/c1/t.ax"
printf 'profile = "restricted"\n' >"$OP/grants/ci.axgrant"
python3 - "$OP" <<'PY'
import hashlib, json, sys
o = sys.argv[1]
def sha(p): return hashlib.sha256(open(p, "rb").read()).hexdigest()
json.dump({"schema": "cortex-check-registry/1",
           "executors": [{"id": "axon", "path": "bin/axon", "sha256": sha(f"{o}/suites/bin/axon")}],
           "checks": [{"id": "c1", "visibility": "hidden", "root": "checks/c1", "entry": "t.ax",
                       "workspace_version_ref": "acf1:" + "a" * 64}]},
          open(f"{o}/suites/registry.json", "w"))
json.dump({"schema": "axon-fabric-grant-registry/1",
           "grants": [{"grant_ref": "grant:ci", "principal_ref": "principal:ci", "path": "ci.axgrant",
                       "sha256": sha(f"{o}/grants/ci.axgrant")}]}, open(f"{o}/grants/grants.json", "w"))
PY
chmod -R a+rX "$WORK"
SIGNER_PUB=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')
KIT=$CLONE/scripts/operator_deploy_protected_host.sh
ARGS=(--from "$CLONE" --bin-dir "$BIN" --suite-registry "$OP/suites/registry.json"
      --grant-registry "$OP/grants/grants.json" --signer-public-key "$SIGNER_PUB"
      --builder-uid "$BUILDER_UID" --builder-parent "$KEYPARENT" --build-uid "$BUILD_UID")

# ══ 1. DRY RUN ═══════════════════════════════════════════════════════════════
snapshot() {
  { for p in /etc/axon /usr/local/libexec/axon /usr/local/lib/axon /var/lib/axon-fabric /var/lib/axon-custodian \
             /var/lib/axon-observer /var/lib/axon-protected-launcher /var/lib/axon-loop /var/lib/axon-deploy \
             /etc/systemd/system/axon-custodian.socket /etc/systemd/system/axon-custodian.service \
             /etc/systemd/system/axon-observer.socket /etc/systemd/system/axon-observer.service; do
      [ -e "$p" ] && find "$p" -printf '%p %u:%g %m %s %T@\n'; done
    getent passwd axon-fabric axon-custodian axon-observer axon-verifier
    getent group axon-fabric axon-custodian axon-observer axon-verifier
  } 2>/dev/null | sort
}
before=$(snapshot)
OUT=$(ns_run bash "$KIT" "${ARGS[@]}" 2>&1); RC=$?
[ "$(snapshot)" = "$before" ] || fail "the dry run changed the host"
ok "a dry run writes nothing (host state identical before and after)"
case $RC in 0|3) ;; *) fail "dry run exited $RC: $OUT" ;; esac
grep -q "nothing is written" <<<"$OUT" || fail "the dry run does not say it writes nothing"
want=(
  'PLAN\[allowlist\] install /etc/axon/provenance-allowlist root:root 644'
  '\| axon-provenance-allowlist/1$' '\| target/$' '\| dist/$'
  '(PLAN|OK)\[users\] .*axon-fabric' '(PLAN|OK)\[users\] .*axon-custodian' '(PLAN|OK)\[users\] .*axon-verifier'
  '(PLAN|OK)\[users\] .*axon-observer'
  '\[dirs\] dir /var/lib/axon-observer root:root 755'
  '\[dirs\] dir /var/lib/axon-observer/observed axon-observer:axon-observer 700'
  '\[dirs\] dir /var/lib/axon-observer/key axon-observer:axon-observer 700'
  '(PLAN|OK)\[users\] .*axonb263'
  '\[dirs\] dir /var/lib/axon-fabric/runs axon-fabric:axon-fabric 700'
  '\[dirs\] dir /var/lib/axon-protected-launcher root:root 700'
  '\[dirs\] dir /var/lib/axon-custodian/nonces axon-custodian:axon-custodian 700'
  '\[dirs\] dir /var/lib/axon-loop root:root 755'
  'PLAN\[binaries\] install /usr/local/libexec/axon/axon-protected-launcher root:axon-fabric 4750'
  'PLAN\[binaries\] install /usr/local/libexec/axon/axon-custodian root:root 755'
  'PLAN\[binaries\] install /usr/local/libexec/axon/axon-fabric root:root 755'
  'PLAN\[binaries\] install /usr/local/libexec/axon/fc_linux_profile.sh root:root 755'
  'PLAN\[binaries\] install /usr/local/libexec/axon/axon-observer root:root 755'
  'PLAN\[guest\] install /usr/local/lib/axon/guest-linux/vmlinux root:root 644'
  'PLAN\[guest\] install /usr/local/lib/axon/guest-linux/rootfs.sqfs root:root 644'
  'PLAN\[guest\] install /usr/local/lib/axon/guest-linux/manifest.json root:root 644'
  'PLAN\[data\] install /etc/axon/suites/registry.json' 'PLAN\[data\] install /etc/axon/suites/bin/axon root:root 755'
  'PLAN\[data\] install /etc/axon/suites/checks/c1/t.ax' 'PLAN\[data\] install /etc/axon/grants/ci.axgrant'
  'PENDING\[data\] no B263 qualification record'
  'PLAN\[configs\] install /etc/axon/custodian.json root:root 644'
  'PLAN\[configs\] install /etc/axon/observer.json root:root 644' '"schema": "axon-observer/1"' '"caller_uid": 0'
  '"socket": "/run/axon-observer/observer.sock"' '"store": "/var/lib/axon-observer/observed"'
  '"key_path": "/var/lib/axon-observer/key/observer.pk8"'
  'PLAN\[configs\] install /etc/axon/protected-launcher.json root:root 644'
  'PLAN\[configs\] install /etc/axon/protected-host.json root:root 644'
  '"schema": "axon-custodian/1"' '"schema": "axon-protected-launcher/2"' '"schema": "axon-protected-host/1"'
  '"launcher_uid": 0' '"authority_store": "/var/lib/axon-loop/store"' '"socket": "/run/axon-custodian/custodian.sock"'
  'PLAN\[verifier\] install /etc/axon/trust/verifier.json root:root 644' '"schema": "axon-verifier-manifest/1"'
  'PLAN\[loader\] setpriv --reuid axon-fabric'
  'PLAN\[systemd\] install /etc/systemd/system/axon-custodian.socket root:root 644'
  'PLAN\[systemd\] install /etc/systemd/system/axon-custodian.service root:root 644'
  'PLAN\[systemd\] install /etc/systemd/system/axon-observer.socket root:root 644'
  'PLAN\[systemd\] install /etc/systemd/system/axon-observer.service root:root 644'
  '\| SocketMode=0600' '\| SocketUser=root' '\| ListenStream=/run/axon-observer/observer.sock' '\| User=axon-observer'
  '\| ExecStart=/usr/local/libexec/axon/axon-observer'
  'PLAN\[systemd\] systemctl enable --now axon-observer.socket'
  '\| SocketMode=0660' '\| SocketGroup=axon-fabric' '\| User=axon-custodian'
  '\| ExecStart=/usr/local/libexec/axon/axon-custodian'
  'PLAN\[systemd\] systemctl enable --now axon-custodian.socket'
  'PLAN\[toolchain\] install /etc/axon/host-toolchain-pin.json root:root 644' '"schema": "axon-host-toolchain-pin/1"'
  'PLAN\[preflight\] bash .*trust_root_preflight.sh --verifier axon-verifier --custodian axon-custodian --fabric axon-fabric'
  'PLAN\[preflight\] bash .*trust_root_preflight.sh .* --fabric-pid '
  'PLAN\[preflight\] bash .*trust_root_preflight.sh .*--observer axon-observer'
  'BLOCKED\[check\] the observer key /var/lib/axon-observer/key/observer.pk8 is absent'
  'OK\[check\] observer.service in .* pins .*axon-observer.*the host config names no observer program'
  'PENDING\[fabric-unit\] --fabric-unit UNIT is required'
  'OK\[check\] custodian.sha256 in .* is the sha256 of .*axon-custodian, the program the custodian unit starts'
  'OK\[check\] fabric.\{path,sha256,revision\} in .* are the installed axon-fabric.s, and the custodian answers check for uid'
  '\| ExecStartPost=/usr/bin/setfacl -m u:axon-observer:rw /run/axon-custodian/custodian.sock'
  '"observer_uid": '
  '"status": "READ by the freeze \(amendment 65\)'
)
for w in "${want[@]}"; do grep -Eq -- "$w" <<<"$OUT" || fail "the plan lacks /$w/:
$OUT"; done
ok "the dry run plans every required action (${#want[@]} assertions: allowlist, users, dirs, binaries, guest, data, configs, verifier, loader, systemd, toolchain, preflight)"
# Pins in the planned configs are the bytes that would be installed.
python3 - "$OUT" "$(sha256sum "$CLONE/scripts/fc_linux_profile.sh" | cut -c1-64)" \
  "$(sha256sum "$BIN/axon-protected-launcher" | cut -c1-64)" "$(sha256sum "$CLONE/dist/guest-linux/manifest.json" | cut -c1-64)" \
  "$(sha256sum "$BIN/axon-observer" | cut -c1-64)" "$SIGNER_PUB" "$(sha256sum "$BIN/axon-custodian" | cut -c1-64)" \
  "$(sha256sum "$BIN/axon-fabric" | cut -c1-64)" <<'PY' || fail "a planned pin is not the source bytes' digest"
import json, re, sys
out, launcher, helper, manifest, observer, signer, custodian, fabric = sys.argv[1:9]
def doc(path):
    m = re.search(r"  ---- %s \(root:root 644\) ----\n(.*?)\n  ---- end %s ----" % (re.escape(path), re.escape(path)), out, re.S)
    return json.loads("\n".join(l[4:] for l in m.group(1).splitlines()))
h, l = doc("/etc/axon/protected-host.json"), doc("/etc/axon/protected-launcher.json")
assert h["launcher"]["sha256"] == launcher == l["launcher"]["sha256"], "launcher pin"
assert h["privileged_launcher"]["sha256"] == helper, "helper pin"
assert h["profile_manifest"]["sha256"] == manifest == l["profile_manifest"]["sha256"], "manifest pin"
# Amendment 68: the observer is a SERVICE the helper pins by PROGRAM; the host
# config names no observer program (a production Fabric refuses one).
assert l["observer"]["service"] == {"socket": "/run/axon-observer/observer.sock",
                                    "uid": l["observer"]["service"]["uid"], "sha256": observer}, "observer service pin"
assert "command" not in h["observer"] and "interpreter" not in h["observer"], "the host config names an observer program"
o = doc("/etc/axon/observer.json")
assert o["observer_uid"] == l["observer"]["service"]["uid"] != o["fabric_uid"] == l["fabric_uid"], "observer uid"
assert o["caller_uid"] == 0 and o["socket"] == l["observer"]["service"]["socket"] and "test_paths" not in o, "observer config"
assert h["signer"]["public_key"] == signer == l["observer"]["host_signer_public_key"], "signer"
# Amendment 65: the helper pins the custodian PROGRAM; the host config names
# the same socket and uid (helper_agrees compares those two only).
assert l["custodian"]["sha256"] == custodian, "custodian program pin"
assert h["observer"]["custodian"] == {k: l["custodian"][k] for k in ("socket", "uid")}, "one custodian"
# Amendment 79: the helper pins the installed axon-fabric (its path, its bytes, and
# the revision the binary itself states), and the custodian answers `check` for
# the observer's uid.
assert l["fabric"]["sha256"] == fabric and l["fabric"]["path"] == "/usr/local/libexec/axon/axon-fabric", l["fabric"]
assert re.fullmatch("[0-9a-f]{40}", l["fabric"]["revision"]), l["fabric"]
assert doc("/etc/axon/custodian.json")["observer_uid"] == o["observer_uid"], "custodian observer_uid"
assert h["out_root"] == l["out_root"] == "/var/lib/axon-fabric/runs"
assert h["qualification"]["max_age_s"] == 2592000
PY
ok "planned pins: launcher, helper, profile manifest, observer service program, custodian program and host signer are the installed bytes' (and agree across host, helper and observer configs; no observer.command)"

refused() { # label pattern cmd...
  local label=$1 pat=$2; shift 2
  local o rc; o=$("$@" 2>&1); rc=$?
  [ $rc = 2 ] || fail "$label: expected REFUSED (2), got $rc: $o"
  grep -q -- "$pat" <<<"$o" || fail "$label: refused for another reason: $o"
  ok "$label is refused"
}
touch "$CLONE/untracked-file"
refused "a clone with an untracked file" "is dirty" ns_run bash "$KIT" "${ARGS[@]}"
rm -f "$CLONE/untracked-file"
mkdir -p "$CLONE/scripts/__pycache__"; touch "$CLONE/scripts/__pycache__/x.pyc"
refused "a clone with a git-ignored file outside target/ and dist/ (decision C)" "is dirty" ns_run bash "$KIT" "${ARGS[@]}"
rm -rf "$CLONE/scripts/__pycache__"
echo "# edit" >>"$CLONE/README.md"
refused "a clone with a modified tracked file" "is dirty" ns_run bash "$KIT" "${ARGS[@]}"
git -C "$CLONE" checkout -q -- README.md
git -C "$CLONE" worktree add -q "$WORK/linked" HEAD 2>/dev/null || fail "cannot make a linked worktree"
refused "a linked worktree" "linked worktree" ns_run bash "$WORK/linked/scripts/operator_deploy_protected_host.sh" \
  --from "$WORK/linked" --bin-dir "$BIN"
git -C "$CLONE" worktree remove --force "$WORK/linked"
mkdir "$WORK/symgit"; cp -a "$CLONE/." "$WORK/symgit/"; rm -rf "$WORK/symgit/.git"; ln -s "$CLONE/.git" "$WORK/symgit/.git"
refused "a symlinked .git" "not a real directory" ns_run bash "$WORK/symgit/scripts/operator_deploy_protected_host.sh" \
  --from "$WORK/symgit" --bin-dir "$BIN"
cp "$KIT" "$WORK/other-kit.sh"; echo "# not the clone's" >>"$WORK/other-kit.sh"
refused "a kit that is not the clone's own copy" "not the clone's own" ns_run bash "$WORK/other-kit.sh" "${ARGS[@]}"
# Amendment 68: no observer PROGRAM, and the observer is its own principal.
refused "ATTACK: --observer-bin (an in-uid observer program)" "since amendment 68" ns_run bash "$KIT" "${ARGS[@]}" --observer-bin /usr/bin/true
refused "ATTACK: --observer-interpreter" "since amendment 68" ns_run bash "$KIT" "${ARGS[@]}" --observer-interpreter /usr/bin/true
refused "ATTACK: an observer user that is the Fabric user" "five different users" ns_run bash "$KIT" "${ARGS[@]}" --observer-user axon-fabric
refused "ATTACK: an observer user that is the custodian user" "five different users" ns_run bash "$KIT" "${ARGS[@]}" --observer-user axon-custodian
# Amendment 86 (round 6): the host binaries are the CONTROLLED host build's. The
# kit installs a --bin-dir only if its host-build.json is signed by the PINNED
# builder (--builder-uid / --builder-parent: the operator's word) and every
# binary is what the record names. Control: every pass above and below is the
# honest directory.
KB="--builder-uid $BUILDER_UID --builder-parent $KEYPARENT --build-uid $BUILD_UID"
copybin() { rm -rf "$WORK/fakebin"; mkdir "$WORK/fakebin"; cp -p "$BIN"/* "$WORK/fakebin/"; }
# (the attacks only the record check catches come first: honest binaries, a bad record;
# the ones a sibling check would also refuse -- no record, a replaced binary -- follow)
copybin; python3 - "$WORK/fakebin/host-build.json" <<'PY'
import json, sys
p = sys.argv[1]; r = json.load(open(p)); r["src_files"] = 99; json.dump(r, open(p, "w"))
PY
refused "ATTACK: a host build record edited after its builder signed it" "does not hold" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$WORK/fakebin" $KB
copybin; python3 - "$WORK/fakebin/host-build.json" <<'PY'
import json, sys
p = sys.argv[1]; r = json.load(open(p)); r.pop("proof"); json.dump(r, open(p, "w"))
PY
refused "ATTACK: a hand-written host build record (no builder proof)" "no builder proof" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$WORK/fakebin" $KB
# The reviewer's forge: another account, with its own private directory and key.
# (made inside a namespace: /var/lib/axon-opkit-forge exists there only, and is handed to the kit's
# namespace through a stash; amendment 97)
copybin; FORGE=/var/lib/axon-opkit-forge; FORGESTASH=$STASHDIR/forge
ns_run python3 - "$WORK/fakebin/host-build.json" "$FORGE" "$KEYPARENT" "$CLONE" "$FORGESTASH" <<'PY' || fail "cannot forge the host record"
import hmac, importlib.util, json, os, subprocess, sys
p, forge, parent, clone, stash = sys.argv[1:6]
sys.dont_write_bytecode = True
os.makedirs(forge, 0o755)
sp = importlib.util.spec_from_file_location("g", os.path.join(clone, "scripts", "guest_build_env.py"))
g = importlib.util.module_from_spec(sp); sp.loader.exec_module(g)
os.chmod(forge, 0o700); os.chown(forge, 4242, 4242)
os.mkdir(forge + "/keys", 0o700); os.chown(forge + "/keys", 4242, 4242)
k = forge + "/keys/f.key"; open(k, "w").write("f" * 64); os.chown(k, 4242, 4242); os.chmod(k, 0o400)
anc, why = g.ancestors_of(forge, 4242); assert not why, why
r = json.loads(json.dumps(json.load(open(p))).replace(parent, forge))
r["builder_uid"] = 4242; r["build_parent_ancestors"] = anc; r["proof"]["id"] = "f"
r["proof"]["hmac"] = hmac.new(b"f" * 64, g.proof_payload(r), "sha256").hexdigest()
json.dump(r, open(p, "w"))
subprocess.run(["rm", "-rf", stash], check=True); os.makedirs(stash)
subprocess.run(["cp", "-a", forge + "/.", stash], check=True)
PY
OPKIT_RESTORE="$KEYSTASH=$KEYPARENT $FORGESTASH=$FORGE" refused "ATTACK: a host build record signed by another account under its own directory" "cannot be checked" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$WORK/fakebin" $KB
rm -rf "$FORGESTASH"
copybin; rm -f "$WORK/fakebin/host-build.json"
refused "ATTACK: binaries with no host-build record (a plain cargo build)" "holds no readable host-build.json" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$WORK/fakebin" $KB
copybin; printf 'built elsewhere' >"$WORK/fakebin/axon-observer"
refused "ATTACK: a binary replaced after the controlled build" "is not the bytes the controlled host build recorded" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$WORK/fakebin" $KB
refused "ATTACK: a judge told another builder uid" "cannot be checked" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$BIN" --builder-uid $((BUILDER_UID + 1)) --builder-parent "$KEYPARENT" --build-uid "$BUILD_UID"
refused "ATTACK: no --builder-uid at all (the record's own word is never taken)" "--builder-uid N is required" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$BIN"
refused "ATTACK: no --build-uid (the uid the build ran as is the operator's word too)" "--build-uid N is required" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$BIN" --builder-uid "$BUILDER_UID" --builder-parent "$KEYPARENT"
refused "ATTACK: a build uid equal to the builder uid (build code as the key holder)" "must differ from --builder-uid" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$BIN" --builder-uid "$BUILD_UID" --builder-parent "$KEYPARENT" --build-uid "$BUILD_UID"
# Amendment 97: the build uid is DEDICATED -- never the uid of any service of the deployment.
refused "ATTACK: a build uid that is the Fabric user's uid (build code as the owner of the attestation key)" "DEDICATED" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$BIN" --builder-uid "$BUILDER_UID" --builder-parent "$KEYPARENT" --build-uid 65534 --fabric-user nobody
refused "ATTACK: a build uid that is the custodian user's uid" "DEDICATED" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$BIN" --builder-uid "$BUILDER_UID" --builder-parent "$KEYPARENT" --build-uid 1 --custodian-user daemon
refused "ATTACK: a build uid that is the observer user's uid" "DEDICATED" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$BIN" --builder-uid "$BUILDER_UID" --builder-parent "$KEYPARENT" --build-uid 2 --observer-user bin
refused "ATTACK: a build uid that is the verifier user's uid" "DEDICATED" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$BIN" --builder-uid "$BUILDER_UID" --builder-parent "$KEYPARENT" --build-uid 1 --verifier-user daemon
refused "ATTACK: a build uid that is the profile user's uid" "DEDICATED" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$BIN" --builder-uid "$BUILDER_UID" --builder-parent "$KEYPARENT" --build-uid 2 --profile-user bin
refused "ATTACK: a build uid other than the one the record was built under" "not the pinned build uid" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$BIN" --builder-uid "$BUILDER_UID" --builder-parent "$KEYPARENT" --build-uid 4243
# Amendment 101: on a FIRST install the service accounts do not exist when the build uid is first checked. The
# uid `useradd --system` allocates next (the highest free below SYS_UID_MAX) is named as the build uid: the users
# step creates the account, and the kit must judge the build uid AGAIN or build code runs as a service.
refused "ATTACK: a build uid that the users step then allocates to a service account (checked before the users existed)" "now that the users exist" \
  ns_run bash -c 'max=$(sed -n "s/^SYS_UID_MAX[[:space:]]*//p" /etc/login.defs); max=${max:-999}; U=$max; while getent passwd "$U" >/dev/null; do U=$((U - 1)); done; exec bash "$1" --from "$2" --only users --apply --no-systemctl --expect-commit "$3" --builder-uid "$4" --build-uid "$U"' \
    x "$KIT" "$CLONE" "$COMMIT" "$BUILDER_UID"
# Amendment 92: run as ROOT, with nothing standing between the attack and a real install but
# the guard under test -- safe because ns_run proves a private namespace first. Were the guard
# missing, this --apply SUCCEEDS inside the namespace (and the test fails on the unexpected
# exit status); it can never reach the host.
refused "ATTACK: --apply without --expect-commit (the commit judges itself)" "--apply needs --expect-commit" \
  ns_run bash "$KIT" "${ARGS[@]}" --no-systemctl --apply
copybin; : >"$WORK/fakebin/extra-file"
refused "ATTACK: a host build directory with a file the build did not make" "files the controlled host build did not make" \
  ns_run bash "$KIT" --from "$CLONE" --bin-dir "$WORK/fakebin" $KB
# The linker the host build recorded must be the one the guest build recorded
# (and, once installed, the operator's pin): the deployed image's manifest names
# another cc digest. (dist/ is excused from the clean-clone rule, so the same
# commit is judged.)
# Amendment 90: after the one copy-aside, the kit reads nothing of the clone's
# WORKING TREE (the builder owns it and ran every build script): every
# `$CLONE/scripts`, `$CLONE/profiles` and `$CLONE/dist` read is the copy-aside
# itself or the cleanliness check. A drift test on the script's own text.
ns_run python3 - "$KIT" <<'PY' || fail "ATTACK: the kit reads the clone's working tree after the copy-aside"
import re, sys
bad = []
for n, line in enumerate(open(sys.argv[1]), 1):
    if line.lstrip().startswith("#"):
        continue
    for m in re.finditer(r'\$\{?CLONE\}?/(scripts|profiles|dist)', line):
        ok = ("cmp -s \"$SELF\"" in line) or ('cp -- "$CLONE/dist/guest-linux/$f"' in line) or ('[ -f "$CLONE/dist/guest-linux/$f" ]' in line)
        if not ok:
            bad.append(f"line {n}: {line.strip()[:120]}")
if bad:
    print("\n".join(bad)); sys.exit(1)
PY
ok "the kit judges, executes, installs and pins only the root-owned copy of what it takes from the clone"
rm -rf "$WORK/linker"; cp -a "$CLONE" "$WORK/linker"
python3 - "$WORK/linker/dist/guest-linux/manifest.json" <<'PY' || fail "cannot edit the dist manifest"
import json, sys
p = sys.argv[1]; m = json.load(open(p))
m["source"]["build_environment"]["toolchain"]["host_tools"]["cc"]["sha256"] = "9" * 64
json.dump(m, open(p, "w"))
PY
refused "ATTACK: a host build whose linker is not the one the guest build recorded" "compiler or linker is not the one" \
  ns_run bash "$WORK/linker/scripts/operator_deploy_protected_host.sh" --from "$WORK/linker" --bin-dir "$BIN" --only binaries $KB
rm -rf "$WORK/linker"
# An ancestor config of the clone naming a compiler wrapper: the kit's own
# config check (in a constructed environment) sees every ancestor.
mkdir -p "$WORK/.cargo"; printf '[build]\nrustc-wrapper = "/usr/bin/sccache"\n' >"$WORK/.cargo/config.toml"
refused "ATTACK: an ancestor cargo config naming a compiler wrapper" "build configuration is not the tree's own" \
  ns_run bash "$KIT" "${ARGS[@]}"
rm -rf "$WORK/.cargo"
# A wrapper or flags in the kit's own environment are not what the binaries
# were built under (the host build constructs its environment and drops them);
# the kit's own config check runs in a constructed one too: control, it passes.
RUSTC_WRAPPER=/usr/bin/true RUSTFLAGS="--cfg evil" PATH_EVIL=1 ns_run bash "$KIT" "${ARGS[@]}" >/dev/null 2>&1; rc=$?
case $rc in 0|3) ok "the caller's wrapper and flags do not reach the kit's judgement (the host build dropped them; control)" ;; *) fail "ambient variables changed the kit's verdict (rc $rc)" ;; esac
# The guest build records are judged BEFORE anything is installed, by the
# freeze's own judge: a record without its runner's proof, or edited after it,
# is not a controlled build's (the control is every guest step above).
tamper_guest() { # LABEL PYTHON-EDIT-OF-m
  local label=$1 edit=$2 T="$WORK/tampered" o rc
  rm -rf "$T"; cp -a "$CLONE" "$T"
  python3 - "$T" "$edit" <<'PY' || fail "cannot tamper the guest manifest"
import json, sys
t, edit = sys.argv[1:3]
for rel in ("dist/guest-linux/manifest.json", "profiles/linux-microvm/manifest.json"):
    p = f"{t}/{rel}"
    m = json.load(open(p))
    exec(edit)
    json.dump(m, open(p, "w"), indent=2)
    open(p, "a").write("\n")
PY
  (cd "$T" && G add -A && G commit -q -m "tampered guest record") || fail "cannot commit the tampered clone"
  o=$(ns_run bash "$T/scripts/operator_deploy_protected_host.sh" --from "$T" --bin-dir "$BIN" --only guest $KB 2>&1); rc=$?
  [ $rc != 0 ] || fail "ATTACK: a guest image whose build record is $label was accepted by the kit: $o"
  grep -q "guest build records are not a controlled build's" <<<"$o" \
    || fail "$label: refused for another reason: $o"
  ok "a guest image whose build record is $label is refused at install"
  rm -rf "$T"
}
tamper_guest "a hand-written one (no builder proof)" "m['source']['build_environment'].pop('proof')"
tamper_guest "edited after its runner signed it" "m['source']['build_environment']['src_files'] = 99"
tamper_guest "a kernel record edited after signing" "m['kernel']['build_environment']['make'][1][2] = '-j1'"
if [ "$(id -u)" = 0 ]; then
  refused "--apply as a non-root uid" "must run as root" ns_run setpriv --reuid=65534 --regid=65534 --clear-groups -- \
    bash "$KIT" "${ARGS[@]}" --no-systemctl --apply
fi

# ── the Fabric unit (amendment 65 / memo C2): no NoNewPrivileges, nothing implying it ──
UNITS=$WORK/units; mkdir -p "$UNITS"
mkunit() { # NAME LINE... : a [Service] unit for the Fabric user plus LINEs
  local n=$1; shift
  { printf '[Unit]\nDescription=opkit test Fabric runner\n\n[Service]\nUser=axon-fabric\nGroup=axon-fabric\n'
    printf 'ExecStart=/usr/local/libexec/axon/axon-fabric status\nProtectSystem=strict\nPrivateTmp=yes\n'
    for l in "$@"; do printf '%s\n' "$l"; done; } >"$UNITS/$n.service"
}
unit_blocked() { # LABEL PATTERN UNITFILE : the kit must BLOCK the unit (exit 3) for that reason
  local o rc; o=$(ns_run bash "$KIT" --from "$CLONE" --only fabric-unit --fabric-unit "$3" --fabric-pid 1 2>&1); rc=$?
  [ $rc = 3 ] || fail "ATTACK: a Fabric unit with $1 was not refused (exit $rc): $o"
  grep -Eq -- "BLOCKED\[fabric-unit\] Fabric unit .*$2" <<<"$o" || fail "ATTACK: a Fabric unit with $1 was refused for another reason: $o"
}
mkunit clean
o=$(ns_run bash "$KIT" --from "$CLONE" --only fabric-unit --fabric-unit "$UNITS/clean.service" --fabric-pid 1 2>&1); rc=$?
[ $rc = 0 ] && grep -q 'OK\[fabric-unit\] Fabric unit .*no NoNewPrivileges' <<<"$o" \
  || fail "control: a Fabric unit with no NoNewPrivileges was not accepted (exit $rc): $o"
mkunit nnp 'NoNewPrivileges=yes'; unit_blocked "NoNewPrivileges=yes" 'NoNewPrivileges=yes' "$UNITS/nnp.service"
mkunit dyn 'DynamicUser=yes'; unit_blocked "DynamicUser=yes" 'DynamicUser=yes implies NoNewPrivileges' "$UNITS/dyn.service"
n=0
for d in SystemCallFilter=@system-service SystemCallLog=@privileged SystemCallArchitectures=native \
         RestrictAddressFamilies=AF_UNIX RestrictNamespaces=yes PrivateDevices=yes ProtectKernelTunables=yes \
         ProtectKernelModules=yes ProtectKernelLogs=yes ProtectClock=yes ProtectHostname=yes \
         MemoryDenyWriteExecute=yes RestrictRealtime=yes RestrictSUIDSGID=yes LockPersonality=yes; do
  mkunit "imp$n" "$d"; unit_blocked "$d" "${d%%=*}=.* implies NoNewPrivileges" "$UNITS/imp$n.service"; n=$((n + 1))
done
mkunit pu 'PrivateUsers=yes'; unit_blocked "PrivateUsers=yes" 'PrivateUsers=yes: in a user namespace' "$UNITS/pu.service"
mkunit sb 'SecureBits=noroot'; unit_blocked "SecureBits=noroot" 'SecureBits=noroot' "$UNITS/sb.service"
mkunit cbs 'CapabilityBoundingSet=CAP_NET_ADMIN'; unit_blocked "a capability bounding set" 'CapabilityBoundingSet=' "$UNITS/cbs.service"
printf '[Service]\nExecStart=/usr/local/libexec/axon/axon-fabric status\n' >"$UNITS/root.service"
unit_blocked "no User= (root)" 'User=\(unset: root\)' "$UNITS/root.service"
# A drop-in after the unit wins (systemctl cat prints them in that order).
mkunit dropin 'NoNewPrivileges=yes' '' '[Service]' 'NoNewPrivileges=no' 'SystemCallFilter=@system-service' 'SystemCallFilter='
o=$(ns_run bash "$KIT" --from "$CLONE" --only fabric-unit --fabric-unit "$UNITS/dropin.service" --fabric-pid 1 2>&1); rc=$?
[ $rc = 0 ] || fail "control: a drop-in that resets NoNewPrivileges and SystemCallFilter was not honoured (exit $rc): $o"
mkunit dropin2 'NoNewPrivileges=no' '' '[Service]' 'NoNewPrivileges=yes'
unit_blocked "NoNewPrivileges=yes in a drop-in" 'NoNewPrivileges=yes' "$UNITS/dropin2.service"
ok "the Fabric unit: NoNewPrivileges, DynamicUser, the $n settings that imply it, PrivateUsers, SecureBits, a bounding set and a root unit are each BLOCKED; a clean unit and a drop-in reset are accepted"

# ── a B263 record measured on another host (amendment 65: b263_host.py) ─────
b263_fixture() { # FILE MACHINE_ID LABEL
  python3 - "$1" "$2" "$3" "$CLONE/dist/guest-linux/manifest.json" <<'PY'
import hashlib, json, sys
out, mid, label, man = sys.argv[1:5]
if mid == "NULL": mid = None
json.dump({"schema": "axon-b263-evidence/1", "issuer_key_id": "ed25519:0000000000000000", "result": "PASS",
           "host": f"{label} (fixture)", "host_facts": {"machine_id": mid, "host_label": label or None},
           "profile": {"manifest_sha256": hashlib.sha256(open(man, "rb").read()).hexdigest()},
           "source": {"tree_dirty": False, "host_identity_sha256": "a" * 64}, "fixture": "never signed"},
          open(out, "w"))
open(out + ".sig", "w").write('{"fixture":"not a signature"}\n')
PY
}
b263_fixture "$WORK/b263-other.json" 0123456789abcdef0123456789abcdef opkit-host
o=$(ns_run bash "$KIT" "${ARGS[@]}" --only data --b263-record "$WORK/b263-other.json" 2>&1)
grep -q 'PENDING\[data\] B263: the B263 record was measured on machine-id 0123456789abcdef0123456789abcdef, not this host' <<<"$o" \
  || fail "ATTACK: a B263 record measured on another machine was not held back: $o"
b263_fixture "$WORK/b263-here.json" "$(head -n1 /etc/machine-id)" opkit-host
o=$(ns_run bash "$KIT" "${ARGS[@]}" --only data --b263-record "$WORK/b263-here.json" 2>&1)
grep -q 'PENDING\[data\] B263' <<<"$o" && fail "control: a B263 record measured on this host was held back: $o"
ok "a B263 record measured on another machine is PENDING; one measured here is not"
# Amendment 92 (M2260): None == None is agreement about nothing. A record with a null machine-id on a
# host that has none (the machine-id file removed INSIDE the namespace) must not be accepted as "measured here".
b263_fixture "$WORK/b263-null.json" NULL opkit-host
o=$(ns_run sh -c 'rm -f /etc/machine-id; exec bash "$@"' sh "$KIT" "${ARGS[@]}" --only data --b263-record "$WORK/b263-null.json" 2>&1)
grep -q 'PENDING\[data\] B263: .*machine-id is missing or empty' <<<"$o" \
  || fail "ATTACK: a B263 record with a null machine-id was accepted on a host with none (None == None): $o"
ok "a null machine-id on either side of the B263 host comparison is refused"

# ══ 2. APPLY in a private mount namespace ════════════════════════════════════
if [ "$(id -u)" != 0 ] || ! command -v unshare >/dev/null; then
  echo "NOT_RUN: the namespace apply test needs root and unshare (dry-run assertions above held; not a full pass)"
  exit 0
fi
cp /usr/local/bin/firecracker /usr/local/bin/jailer "$WORK/" 2>/dev/null || { echo "NOT_RUN: no /usr/local/bin/firecracker+jailer to deploy against"; exit 0; }
cat >"$WORK/ns.sh" <<'NS'
set -uo pipefail
W=$1 KIT=$2 BIN=$3 OP=$4 SIGNER_PUB=$5 CLONE=$6 BUID=$7 BPAR=$8 BBU=$9 CMT=${10}
fail() { echo "FAIL(ns): $*"; exit 1; }
# Isolation first: nothing below may reach the host. ns_run (the caller) already
# shadowed and PROVED the destinations; every kit call below re-asserts it.
. "$OPKIT_LIB"
opkit_ns_assert || fail "not in a proved private namespace"
kit() { opkit_ns_assert || fail "REFUSED to run the kit: namespace isolation not proved"; bash "$KIT" "$@"; }
grep -q axon-fabric /etc/passwd && fail "the shadow /etc already has axon users"
ARGS=(--from "$CLONE" --bin-dir "$BIN" --suite-registry "$OP/suites/registry.json"
      --grant-registry "$OP/grants/grants.json" --signer-public-key "$SIGNER_PUB" --no-systemctl
      --builder-uid "$BUID" --builder-parent "$BPAR" --build-uid "$BBU" --expect-commit "$CMT")
# Amendment 79: the custodian socket unit grants the observer's uid by
# `ExecStartPost=setfacl`, so the kit BLOCKS where `setfacl` (the acl package) is
# absent and systemd would start the unit (not under --no-systemctl, which only
# installs files). ATTACK: exercised where the host really lacks it. CONTROL: the
# full apply below, under --no-systemctl, judges the unit's line as a file.
if ! command -v setfacl >/dev/null 2>&1; then
  SC_ARGS=(); for a in "${ARGS[@]}"; do [ "$a" = --no-systemctl ] || SC_ARGS+=("$a"); done
  kit "${SC_ARGS[@]}" --only systemd >"$W/nofacl.out" 2>&1; r=$?
  [ $r = 3 ] && grep -q 'BLOCKED\[systemd\] setfacl (the acl package) is required' "$W/nofacl.out" \
    || { cat "$W/nofacl.out"; fail "ATTACK: the kit did not BLOCK a host with no setfacl (exit $r)"; }
  echo "ok(ns): a host with no setfacl is BLOCKED where systemd would start the unit"
else
  echo "note(ns): this host has setfacl; the missing-setfacl attack was not exercised"
fi
# Runbook order: allowlist, users and directories, THEN the operator provisions
# keys (here: inert fixtures), THEN the whole kit.
kit "${ARGS[@]}" --only allowlist,users,dirs --apply >"$W/apply1.out" 2>&1 \
  || { r=$?; [ $r = 3 ] && grep -q 'authority store' "$W/apply1.out" || { cat "$W/apply1.out"; fail "first apply exited $r"; }; }
FU=$(id -u axon-fabric) || fail "axon-fabric was not created"
id -u axon-custodian >/dev/null && id -u axon-verifier >/dev/null && id -u axon-observer >/dev/null || fail "users not created"
OU=$(id -u axon-observer); OG=$(id -g axon-observer)
[ "$(getent passwd axon-observer | cut -d: -f7)" = /usr/sbin/nologin ] || fail "observer has a login shell"
[ "$OU" != "$FU" ] && [ "$OU" != "$(id -u axon-custodian)" ] || fail "the observer user shares a uid"
id -nG axon-observer | tr ' ' '\n' | grep -qx axon-fabric && fail "the observer user is in the Fabric group"
[ "$(getent passwd axon-custodian | cut -d: -f7)" = /usr/sbin/nologin ] || fail "custodian has a login shell"
for a in qualification observer verifier; do install -d -o root -g root -m 0755 "/etc/axon/trust/$a"; done
rnd() { head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n'; }
rnd >/etc/axon/trust/qualification/operator.pub
echo "$SIGNER_PUB" >/etc/axon/trust/verifier/host-signer.pub
chmod 0644 /etc/axon/trust/*/*.pub
# Amendment 68: the observer's key is generated by the OPERATOR, AS the observer
# uid, on the host (the kit never makes it): axon-fabric keygen writes it 0400.
# A fixture that vanishes with the namespace; nothing is signed with it.
cp "$BIN/axon-fabric" "$W/keygen" && chmod 0755 "$W/keygen" || fail "cannot stage keygen"
OBS_KEY=/var/lib/axon-observer/key/observer.pk8
okg=$(setpriv --reuid="$OU" --regid="$OG" --clear-groups -- "$W/keygen" keygen --out "$OBS_KEY") || fail "observer keygen"
python3 -c 'import json,sys; print(json.loads(sys.argv[1])["public_key"])' "$okg" >/etc/axon/trust/observer/observer.pub
chmod 0644 /etc/axon/trust/observer/observer.pub
[ "$(stat -c '%U:%G %a' "$OBS_KEY")" = "axon-observer:axon-observer 400" ] || fail "keygen did not write the observer key 0400 as the observer"
printf 'x' >/etc/axon/keys/fabric-attest.pk8   # one byte: a fixture, never a key
chown axon-fabric:axon-fabric /etc/axon/keys/fabric-attest.pk8; chmod 0400 /etc/axon/keys/fabric-attest.pk8
mkdir -p /var/lib/axon-loop/store
# The loader reads only the B263 record's presence and ownership (Fabric's
# qualification() verifies it at each launch); a shaped fixture, unsigned.
python3 - "$W/b263.json" /dev/stdin <<PY
import hashlib, json, sys
man = hashlib.sha256(open("$CLONE/dist/guest-linux/manifest.json", "rb").read()).hexdigest()
mid = open("/etc/machine-id").readline().strip()
json.dump({"schema": "axon-b263-evidence/1", "issuer_key_id": "ed25519:0000000000000000", "result": "PASS",
           "host": "opkit-ns (fixture)", "host_facts": {"machine_id": mid, "host_label": "opkit-ns"},
           "profile": {"manifest_sha256": man}, "source": {"tree_dirty": False, "host_identity_sha256": "a" * 64},
           "fixture": "never signed"},
          open(sys.argv[1], "w"))
PY
echo '{"fixture":"not a signature"}' >"$W/b263.json.sig"
# What systemd does when the socket unit starts (no systemctl in here).
install -d -o root -g root -m 0755 /run/axon-custodian
# ...and the observer's socket, root-only (SocketMode=0600 root:root, the
# installed socket unit's): a bound socket the preflight can really try to
# connect to. No service runs behind it; nothing here is the observer.
install -d -o root -g root -m 0755 /run/axon-observer
python3 -I -c 'import os, socket, time
p = "/run/axon-observer/observer.sock"
s = socket.socket(socket.AF_UNIX); s.bind(p); os.chmod(p, 0o600); s.listen(8); time.sleep(3600)' &
OSPID=$!
for _ in $(seq 50); do [ -S /run/axon-observer/observer.sock ] && break; sleep 0.1; done
[ -S /run/axon-observer/observer.sock ] || fail "the observer socket fixture did not bind"
# The Fabric service: its unit (judged as a file: no systemd in here) and a
# running process as the Fabric uid WITHOUT NoNewPrivileges, as that unit
# would start it. Its pid is the preflight's --fabric-pid (amendment 65).
printf '[Service]\nUser=axon-fabric\nGroup=axon-fabric\nExecStart=/usr/local/libexec/axon/axon-fabric status\nProtectSystem=strict\n' >"$W/fabric.service"
FG=$(id -g axon-fabric)
setpriv --reuid="$FU" --regid="$FG" --clear-groups -- sleep 3600 & FPID=$!
setpriv --reuid="$FU" --regid="$FG" --clear-groups --no-new-privs -- sleep 3600 & NNP_PID=$!
trap 'kill $FPID $NNP_PID $OSPID 2>/dev/null' EXIT
ARGS+=(--fabric-unit "$W/fabric.service")
# Amendment 105: the command has no CAP_SYS_ADMIN, so the guest view (an empty /etc/axon) is a USER namespace's own mount
GUEST="unshare --user --map-root-user --mount --propagation private sh -c 'mount -t tmpfs none /etc/axon && sh $CLONE/scripts/trust_root_guest_probe.sh /etc/axon/trust'"
kit "${ARGS[@]}" --fabric-pid "$FPID" --b263-record "$W/b263.json" --agent 40003 --agent 40004 --guest-cmd "$GUEST" --apply >"$W/apply2.out" 2>&1
r=$?; cat "$W/apply2.out" >&2
[ $r = 0 ] || fail "the full apply exited $r"
grep -q 'OK\[loader\] ProtectedHost::operator() accepted' "$W/apply2.out" || fail "the production loader did not accept the deployment"
grep -q 'PREFLIGHT verdict PASS mode protected' "$W/apply2.out" || fail "the protected-mode preflight did not PASS"
grep -q 'OK\[toolchain\] guest_build_env.toolchain_pin_problems accepts the installed pin' "$W/apply2.out" \
  || fail "the freeze's own reader did not accept the installed toolchain pin"
grep -q 'OK\[check\] custodian.sha256 in /etc/axon/protected-launcher.json is the sha256 of /usr/local/libexec/axon/axon-custodian' "$W/apply2.out" \
  || fail "the installed custodian program pin was not verified"
grep -q 'OK\[check\] fabric.{path,sha256,revision} in /etc/axon/protected-launcher.json are the installed axon-fabric' "$W/apply2.out" \
  || fail "the installed Fabric program pin was not verified"
grep -qx 'ExecStartPost=/usr/bin/setfacl -m u:axon-observer:rw /run/axon-custodian/custodian.sock' /etc/systemd/system/axon-custodian.socket \
  || fail "the installed custodian socket unit does not grant the observer's uid by ACL"
grep -q 'OK\[check\] observer.service in /etc/axon/protected-launcher.json pins /usr/local/libexec/axon/axon-observer' "$W/apply2.out" \
  || fail "the installed observer program pin was not verified"
grep -q -- "--observer axon-observer" "$W/apply2.out" || fail "the preflight was not given the observer uid"
grep -q -- "--fabric-pid $FPID" "$W/apply2.out" || fail "the preflight was not given the Fabric pid"
rep=$(sed -n 's/^PREFLIGHT verdict PASS mode protected report \([^ ]*\) .*/\1/p' "$W/apply2.out")
python3 -c 'import json,sys; r=json.load(open(sys.argv[1])); assert r["mode"]=="protected" and r["verdict"]=="PASS" and r["root"]=="/etc/axon/trust" and len(r["checks"])>40, r["verdict"]; assert [c for c in r["checks"] if c["action"]=="no-new-privs" and c["ok"]], "no NoNewPrivs check"
ob = [c for c in r["checks"] if c["action"] in ("read-observer-key", "connect-observer", "observer-key-mode", "observer-separate", "observer-config-uid")]
assert all(c["ok"] for c in ob), ob
# Amendment 68: every actor but the observer FAILED to open its key and to connect to its socket, by a real attempt.
for who in ("fabric", "verifier", "custodian", "agent:40003", "agent:40004"):
    assert [c for c in ob if c["actor"] == who and c["action"] == "read-observer-key" and c["observed"] == "refused"], ("key", who)
    assert [c for c in ob if c["actor"] == who and c["action"] == "connect-observer" and c["observed"] == "denied"], ("socket", who)
assert [c for c in ob if c["actor"] == "observer" and c["action"] == "read-observer-key" and c["observed"] == "read"], "the observer cannot read its key"' "$rep" \
  || fail "the preflight report is not a protected PASS (with the observer service's checks)"
echo "ok(ns): full apply: production loader accepts, protected-mode trust preflight PASS ($(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["checks"]))' "$rep") checks)"
m() { stat -c '%U:%G %a' "$1"; }
[ "$(m /usr/local/libexec/axon/axon-protected-launcher)" = "root:axon-fabric 4750" ] || fail "helper mode $(m /usr/local/libexec/axon/axon-protected-launcher)"
for f in /etc/axon/protected-host.json /etc/axon/protected-launcher.json /etc/axon/custodian.json /etc/axon/provenance-allowlist /etc/axon/trust/verifier.json; do
  [ "$(m "$f")" = "root:root 644" ] || fail "$f is $(m "$f")"
done
[ "$(m /var/lib/axon-fabric/runs)" = "axon-fabric:axon-fabric 700" ] || fail "out root"
[ "$(m /var/lib/axon-protected-launcher)" = "root:root 700" ] || fail "staging root"
[ "$(m /var/lib/axon-custodian/nonces)" = "axon-custodian:axon-custodian 700" ] || fail "store"
# Amendment 68: the observer's uid, key, directories, config, binary and units.
[ "$(m /var/lib/axon-observer/observed)" = "axon-observer:axon-observer 700" ] || fail "observer store"
[ "$(m /var/lib/axon-observer/key)" = "axon-observer:axon-observer 700" ] || fail "observer key directory"
[ "$(m /var/lib/axon-observer/key/observer.pk8)" = "axon-observer:axon-observer 400" ] || fail "observer key"
[ "$(m /var/lib/axon-observer)" = "root:root 755" ] || fail "observer state directory"
[ "$(m /usr/local/libexec/axon/axon-observer)" = "root:root 755" ] || fail "observer program"
for f in /etc/axon/observer.json /etc/systemd/system/axon-observer.socket /etc/systemd/system/axon-observer.service; do
  [ "$(m "$f")" = "root:root 644" ] || fail "$f is $(m "$f")"
done
grep -qx 'SocketMode=0600' /etc/systemd/system/axon-observer.socket && grep -qx 'SocketGroup=root' /etc/systemd/system/axon-observer.socket \
  || fail "the installed observer socket is not root-only"
grep -qx 'User=axon-observer' /etc/systemd/system/axon-observer.service && grep -qx 'ExecStart=/usr/local/libexec/axon/axon-observer' /etc/systemd/system/axon-observer.service \
  || fail "the installed observer unit does not run the installed axon-observer as its user"
python3 - <<'PY' || fail "a config pin is not its installed file's digest"
import hashlib, json
def sha(p): return hashlib.sha256(open(p, "rb").read()).hexdigest()
h = json.load(open("/etc/axon/protected-host.json")); l = json.load(open("/etc/axon/protected-launcher.json"))
for d in (h["launcher"], h["privileged_launcher"], h["profile_manifest"], h["suite_registry"], h["grant_registry"],
          l["interpreter"], l["launcher"], l["profile_manifest"]):
    assert sha(d["path"]) == d["sha256"], d
assert l["custodian"]["sha256"] == sha("/usr/local/libexec/axon/axon-custodian"), "custodian program pin"
assert l["observer"]["service"]["sha256"] == sha("/usr/local/libexec/axon/axon-observer"), "observer program pin"
assert l["fabric"]["sha256"] == sha("/usr/local/libexec/axon/axon-fabric") and l["fabric"]["path"] == "/usr/local/libexec/axon/axon-fabric", "fabric program pin"
assert "command" not in h["observer"], "the host config names an observer program"
o = json.load(open("/etc/axon/observer.json"))
assert o["observer_uid"] == l["observer"]["service"]["uid"] and o["fabric_uid"] == l["fabric_uid"] and o["caller_uid"] == 0, o
assert json.load(open("/etc/axon/custodian.json"))["observer_uid"] == o["observer_uid"], "custodian observer_uid"
assert json.load(open("/etc/axon/host-toolchain-pin.json"))["status"].startswith("READ by the freeze")
v = json.load(open("/etc/axon/trust/verifier.json"))
assert v["path"] == "/usr/local/libexec/axon/axon-fabric" and v["sha256"] == sha(v["path"]), v
PY
echo "ok(ns): modes and owners as the code requires; every pin is its INSTALLED file's digest"
# Idempotent: a second apply plans nothing.
kit "${ARGS[@]}" --fabric-pid "$FPID" --b263-record "$W/b263.json" --agent 40003 --guest-cmd "$GUEST" --apply >"$W/apply3.out" 2>&1 \
  || fail "the second apply failed: $(grep -E '^(FAIL|BLOCKED|PENDING|REFUSED|operator_deploy)' "$W/apply3.out" | tr '\n' '|')"
if grep -E 'PLAN\[[a-z]+\] (install|dir|useradd|groupadd)' "$W/apply3.out"; then fail "the second apply changed something"; fi
echo "ok(ns): a second --apply changes nothing (idempotent)"
# ATTACK (round 7): the operator's toolchain pin names a rustc and a cargo that
# are not the host build's (digests 00..00 and 11..11): the kit compared only cc
# and ld and printed nothing. Control: the installed pin (every apply above).
cp /etc/axon/host-toolchain-pin.json "$W/pin.good"
python3 - <<'PY'
import json
p = "/etc/axon/host-toolchain-pin.json"; d = json.load(open(p))
d["tools"]["rustc"]["sha256"] = "0" * 64; d["tools"]["cargo"]["sha256"] = "1" * 64
json.dump(d, open(p, "w"))
PY
o=$(kit "${ARGS[@]}" --only binaries 2>&1); r=$?
cp "$W/pin.good" /etc/axon/host-toolchain-pin.json
[ $r = 2 ] && grep -q "compiler or linker is not the one" <<<"$o" && grep -q "rustc: the host build recorded" <<<"$o" \
  || fail "ATTACK: a toolchain pin naming another rustc and cargo was accepted for the host build (rc $r): $(echo "$o" | tail -3)"
echo "ok(ns): a toolchain pin whose rustc and cargo are not the host build's is REFUSED (control: the installed pin passes)"
# ATTACK (amendment 65): a Fabric under NoNewPrivileges. The kernel would
# ignore the helper's set-id bit; the kit's preflight must FAIL, not pass.
kit "${ARGS[@]}" --fabric-pid "$NNP_PID" --b263-record "$W/b263.json" --agent 40003 --guest-cmd "$GUEST" \
  --only check,preflight --apply >"$W/nnp.out" 2>&1; r=$?
[ $r = 1 ] && grep -q 'FAIL\[preflight\] trust preflight verdict FAIL' "$W/nnp.out" \
  && grep -q "no-new-privs .*NoNewPrivs 1" "$W/nnp.out" \
  || { cat "$W/nnp.out"; fail "ATTACK: a Fabric process under NoNewPrivileges passed the kit's preflight (exit $r)"; }
echo "ok(ns): a Fabric process under NoNewPrivileges FAILS the kit's preflight (control: the same uid without it PASSED above)"
# ATTACK (amendment 65): the helper config pins another custodian program.
cp -a /etc/axon/protected-launcher.json "$W/launcher.good"
python3 -c 'import json; p="/etc/axon/protected-launcher.json"; c=json.load(open(p)); c["custodian"]["sha256"]="f"*64; json.dump(c, open(p,"w"), indent=2)'
kit "${ARGS[@]}" --only check >"$W/pin.out" 2>&1; r=$?
cp -a "$W/launcher.good" /etc/axon/protected-launcher.json
[ $r = 1 ] && grep -q "FAIL\[check\] custodian program pin: /etc/axon/protected-launcher.json: custodian.sha256 ffff" "$W/pin.out" \
  || { cat "$W/pin.out"; fail "ATTACK: a helper config pinning another custodian program was not refused (exit $r)"; }
# The production loader refuses it too (load_config, M1485's rule: no pin).
python3 -c 'import json; p="/etc/axon/protected-launcher.json"; c=json.load(open(p)); del c["custodian"]["sha256"]; json.dump(c, open(p,"w"), indent=2)'
lo=$(setpriv --reuid="$FU" --regid="$FG" --clear-groups -- /usr/local/libexec/axon/axon-fabric submit --request /nonexistent/x.json 2>/dev/null)
cp -a "$W/launcher.good" /etc/axon/protected-launcher.json
grep -q 'custodian.sha256 must pin the axon-custodian program' <<<"$lo" \
  || fail "ATTACK: the production loader accepted a helper config with no custodian program pin: $lo"
echo "ok(ns): a helper config pinning another custodian program FAILS the kit's check; one with no pin is refused by the production loader"
# ATTACK (amendment 79): the helper config pins another Fabric program, another
# revision than the installed binary states, or none; the custodian's config
# names another uid for `check` than the observer's. Each is refused for its own
# reason by the kit's check; control: the full apply above held them all.
fabric_attack() { # label python-edit expected-pattern
  python3 -c 'import json,sys; p="/etc/axon/protected-launcher.json"; c=json.load(open(p)); '"$2"'; json.dump(c, open(p,"w"), indent=2)'
  kit "${ARGS[@]}" --only check >"$W/fab.out" 2>&1; r=$?
  cp -a "$W/launcher.good" /etc/axon/protected-launcher.json
  [ $r = 1 ] && grep -Eq -- "$3" "$W/fab.out" \
    || { cat "$W/fab.out"; fail "ATTACK: $1 was not refused by the kit's check (exit $r)"; }
}
fabric_attack "a helper config pinning another Fabric program" 'c["fabric"]["sha256"]="f"*64' \
  "FAIL\[check\] fabric program pin: /etc/axon/protected-launcher.json: fabric.sha256 ffff"
fabric_attack "a helper config naming another revision than the installed Fabric states" 'c["fabric"]["revision"]="1"*40' \
  "FAIL\[check\] fabric program pin: .*fabric.revision '1111111111111111111111111111111111111111' is not the"
fabric_attack "a helper config naming the Fabric program at another path" 'c["fabric"]["path"]="/usr/bin/axon-fabric"' \
  "FAIL\[check\] fabric program pin: .*fabric.path is '/usr/bin/axon-fabric'"
cp -a /etc/axon/custodian.json "$W/custodian.good"
python3 -c 'import json; p="/etc/axon/custodian.json"; c=json.load(open(p)); c["observer_uid"]=int(c["fabric_uid"]); json.dump(c, open(p,"w"), indent=2)'
kit "${ARGS[@]}" --only check >"$W/fab.out" 2>&1; r=$?
cp -a "$W/custodian.good" /etc/axon/custodian.json
[ $r = 1 ] && grep -q "observer_uid is $FU, not the observer user's uid" "$W/fab.out" \
  || { cat "$W/fab.out"; fail "ATTACK: a custodian config answering check for the Fabric uid was not refused by the kit's check (exit $r)"; }
python3 -c 'import json; p="/etc/axon/protected-launcher.json"; c=json.load(open(p)); del c["fabric"]; json.dump(c, open(p,"w"), indent=2)'
lo=$(setpriv --reuid="$FU" --regid="$FG" --clear-groups -- /usr/local/libexec/axon/axon-fabric submit --request /nonexistent/x.json 2>/dev/null)
cp -a "$W/launcher.good" /etc/axon/protected-launcher.json
grep -q 'missing field `fabric`' <<<"$lo" \
  || fail "ATTACK: the production loader accepted a helper config with no Fabric program pin: $lo"
echo "ok(ns): another Fabric pin, revision or path, or a custodian answering check for the wrong uid, FAILS the kit's check; no pin is refused by the production loader"
# ATTACK: a signing key its owner can write (0600). Fabric's loader requires
# mode & 0277 == 0 (0400); the kit must not pass what Fabric refuses.
chmod 0600 /etc/axon/keys/fabric-attest.pk8
kit "${ARGS[@]}" --only check >"$W/key.out" 2>&1; r=$?
chmod 0400 /etc/axon/keys/fabric-attest.pk8
[ $r = 3 ] && grep -Eq 'BLOCKED\[(configs|check)\] /etc/axon/keys/fabric-attest.pk8 must be owned by axon-fabric, readable by it alone and writable by no one' "$W/key.out" \
  || fail "ATTACK: a 0600 signing key passed the kit's check (exit $r): $(grep -E '^(BLOCKED|FAIL|PENDING)' "$W/key.out" | tr '\n' '|')"
echo "ok(ns): a signing key its owner can write (0600) is BLOCKED, as Fabric's loader refuses it"
# ── Amendment 68: the observer SERVICE. Each attack breaks ONE fact and must be
# refused for that reason; the control (the full apply above) held them all.
check_run() { kit "${ARGS[@]}" --only check >"$W/obs.out" 2>&1; echo $?; }
# (1) a key another uid can read (0440 here, so the group reads it).
chmod 0440 "$OBS_KEY"; r=$(check_run); chmod 0400 "$OBS_KEY"
[ $r = 3 ] && grep -Eq 'BLOCKED\[check\] the observer key /var/lib/axon-observer/key/observer.pk8 must be owned by axon-observer, readable by it alone and writable by no one' "$W/obs.out" \
  || { cat "$W/obs.out"; fail "ATTACK: an observer key another uid can read (0440) passed the kit's check (exit $r)"; }
# (2) a key the FABRIC uid owns (its key would be the Fabric's to read).
chown "$FU" "$OBS_KEY"; r=$(check_run); chown "$OU" "$OBS_KEY"
[ $r = 3 ] && grep -Eq 'BLOCKED\[check\] the observer key .* must be owned by axon-observer' "$W/obs.out" \
  || { cat "$W/obs.out"; fail "ATTACK: an observer key the Fabric uid owns passed the kit's check (exit $r)"; }
# (3) a key directory the group can enter.
chmod 0750 /var/lib/axon-observer/key; r=$(check_run); chmod 0700 /var/lib/axon-observer/key
[ $r = 3 ] && grep -Eq 'BLOCKED\[check\] the observer key directory /var/lib/axon-observer/key must be axon-observer.s, mode 0700' "$W/obs.out" \
  || { cat "$W/obs.out"; fail "ATTACK: an observer key directory the group can enter passed the kit's check (exit $r)"; }
# (4) an absent key.
mv "$OBS_KEY" "$W/observer.pk8.away"; r=$(check_run); mv "$W/observer.pk8.away" "$OBS_KEY"
[ $r = 3 ] && grep -Eq 'BLOCKED\[check\] the observer key /var/lib/axon-observer/key/observer.pk8 is absent' "$W/obs.out" \
  || { cat "$W/obs.out"; fail "ATTACK: an absent observer key passed the kit's check (exit $r)"; }
[ "$(check_run)" = 0 ] && grep -q 'OK\[check\] observer.service in' "$W/obs.out" \
  || { cat "$W/obs.out"; fail "control: the restored observer key and configs no longer pass the kit's check"; }
echo "ok(ns): an observer key another uid can read, one the Fabric owns, a group-enterable key directory and an absent key are each BLOCKED (control: the restored state passes)"
# (5) the helper config pins another observer program, or none, or another socket/uid.
cp -a /etc/axon/protected-launcher.json "$W/launcher.good2"
helper_edit() { python3 -c 'import json,sys; p="/etc/axon/protected-launcher.json"; c=json.load(open(p)); exec(sys.argv[1]); json.dump(c, open(p,"w"), indent=2)' "$1"; }
for case in 'c["observer"]["service"]["sha256"]="f"*64|observer.service.sha256 ffff' \
            'del c["observer"]["service"]|observer.service is absent' \
            'c["observer"]["service"]["socket"]="/run/axon-observer/other.sock"|is not the observer.s socket' \
            'c["observer"]["service"]["uid"]=int(c["fabric_uid"])|is not the observer.s uid'; do
  helper_edit "${case%%|*}"; r=$(check_run); cp -a "$W/launcher.good2" /etc/axon/protected-launcher.json
  [ $r = 1 ] && grep -Eq "FAIL\[check\] observer service: /etc/axon/protected-launcher.json: .*${case#*|}" "$W/obs.out" \
    || { cat "$W/obs.out"; fail "ATTACK: a helper config with (${case#*|}) was not refused by the kit (exit $r)"; }
done
# ...and the production helper's own loader refuses a service that is the Fabric's uid.
helper_edit 'c["observer"]["service"]["uid"]=int(c["fabric_uid"])'
lo=$(setpriv --reuid="$FU" --regid="$FG" --clear-groups -- /usr/local/libexec/axon/axon-protected-launcher --observe </dev/null 2>&1 | head -c 600)
cp -a "$W/launcher.good2" /etc/axon/protected-launcher.json
grep -Eqi 'observer|fabric' <<<"$lo" && ! grep -q '"ok":true' <<<"$lo" \
  || fail "ATTACK: the production helper accepted an observer service whose uid is the Fabric's: $lo"
echo "ok(ns): a helper config with the wrong observer program pin, no observer.service, another socket, or the Fabric's uid FAILS the kit's check (the helper refuses the last itself)"
# (6) the host config names an observer PROGRAM: the kit fails it, and the PRODUCTION loader refuses it.
cp -a /etc/axon/protected-host.json "$W/host.good"
python3 -c 'import json; p="/etc/axon/protected-host.json"; c=json.load(open(p)); c["observer"]["command"]={"path":"/usr/local/libexec/axon/axon-observer","sha256":"0"*64}; json.dump(c, open(p,"w"), indent=2)'
r=$(check_run)
lo=$(setpriv --reuid="$FU" --regid="$FG" --clear-groups -- /usr/local/libexec/axon/axon-fabric submit --request /nonexistent/x.json 2>/dev/null)
cp -a "$W/host.good" /etc/axon/protected-host.json
[ $r = 1 ] && grep -q 'FAIL\[check\] observer service: .*observer.command names an observer PROGRAM run as the Fabric uid' "$W/obs.out" \
  || { cat "$W/obs.out"; fail "ATTACK: a host config naming observer.command passed the kit's check (exit $r)"; }
grep -q 'observer.command: an observer program runs as the Fabric uid' <<<"$lo" \
  || fail "ATTACK: the production loader accepted a host config naming observer.command: $lo"
echo "ok(ns): a host config naming observer.command FAILS the kit's check and is refused by the production loader"
# (7) the observer's own config: the observer is the Fabric's uid; a caller that is not root; the wrong store.
cp -a /etc/axon/observer.json "$W/observer.good"
obs_edit() { python3 -c 'import json,sys; p="/etc/axon/observer.json"; c=json.load(open(p)); exec(sys.argv[1]); json.dump(c, open(p,"w"), indent=2)' "$1"; }
for case in 'c["observer_uid"]=c["fabric_uid"]|neither the Fabric nor root' \
            'c["caller_uid"]=c["fabric_uid"]|caller_uid is .*not 0' \
            'c["store"]="/var/lib/axon-fabric/runs/observed"|store is .*not /var/lib/axon-observer/observed'; do
  obs_edit "${case%%|*}"; r=$(check_run); cp -a "$W/observer.good" /etc/axon/observer.json
  [ $r = 1 ] && grep -Eq "FAIL\[check\] observer service: /etc/axon/observer.json: .*${case#*|}" "$W/obs.out" \
    || { cat "$W/obs.out"; fail "ATTACK: an observer config with (${case#*|}) was not refused by the kit (exit $r)"; }
done
echo "ok(ns): an observer config naming the Fabric uid as the observer, a non-root caller, or another store FAILS the kit's check"
# (8) the installed socket unit lets another uid connect.
cp -a /etc/systemd/system/axon-observer.socket "$W/obs.socket.good"
sed -i 's/^SocketMode=0600/SocketMode=0660/;s/^SocketGroup=root/SocketGroup=axon-fabric/' /etc/systemd/system/axon-observer.socket
r=$(check_run); cp -a "$W/obs.socket.good" /etc/systemd/system/axon-observer.socket
[ $r = 1 ] && grep -Eq 'FAIL\[check\] observer service: .*axon-observer.socket: SocketMode=.0660., not 0600: only root' "$W/obs.out" \
  || { cat "$W/obs.out"; fail "ATTACK: an observer socket unit the Fabric group can connect to passed the kit's check (exit $r)"; }
echo "ok(ns): an observer socket unit that admits the Fabric group FAILS the kit's check"
# (9) the PREFLIGHT, run for real: each attack breaks one fact and its check must FAIL.
PF() { bash "$CLONE/scripts/trust_root_preflight.sh" --verifier axon-verifier --custodian axon-custodian --fabric axon-fabric \
         --observer axon-observer --agent 40003 --agent 40004 --guest-cmd "$GUEST" --fabric-pid "$FPID" --out "$W/pf.json" "$@" >"$W/pf.out" 2>&1; }
pf_has() { python3 -c 'import json,sys; r=json.load(open(sys.argv[1])); a,t,e,o=sys.argv[2:6]; sys.exit(0 if [c for c in r["checks"] if c["actor"]==a and c["action"]==t and c["expected"]==e and c["observed"]==o and not c["ok"]] else 1)' "$W/pf.json" "$@"; }
PF || { cat "$W/pf.out" | tail -n 20; fail "control: the preflight does not pass on the deployed observer service"; }
rc=0; bash "$CLONE/scripts/trust_root_preflight.sh" --verifier axon-verifier --custodian axon-custodian --fabric axon-fabric \
  --agent 40003 --guest-cmd "$GUEST" --fabric-pid "$FPID" >"$W/pf2.out" 2>&1 || rc=$?
[ $rc = 2 ] && grep -q '"verdict":"NOT_RUN"' "$W/pf2.out" && grep -q -- '--observer UID' "$W/pf2.out" \
  || fail "ATTACK: the protected-mode preflight ran without --observer (exit $rc)"
# The key's own 0700 directory already shields a 0444 key from other uids, so the
# mode is caught by observer-key-mode alone; with the directory opened too, the
# real open attempts by the Fabric and an agent SUCCEED and fail the preflight.
chmod 0444 "$OBS_KEY"; PF; rc=$?
[ $rc = 1 ] && pf_has operator observer-key-mode ok "mode 444: not 0400" \
  || { chmod 0400 "$OBS_KEY"; fail "ATTACK: an observer key readable by every uid passed the preflight (exit $rc)"; }
chmod 0755 /var/lib/axon-observer/key; PF; rc=$?; chmod 0400 "$OBS_KEY"; chmod 0700 /var/lib/axon-observer/key
[ $rc = 1 ] && pf_has fabric read-observer-key refused SUCCEEDED && pf_has "agent:40003" read-observer-key refused SUCCEEDED \
  || fail "ATTACK: an observer key every uid can open passed the preflight (exit $rc)"
chmod 0666 /run/axon-observer/observer.sock; PF; rc=$?; chmod 0600 /run/axon-observer/observer.sock
[ $rc = 1 ] && pf_has fabric connect-observer denied connected && pf_has "agent:40004" connect-observer denied connected \
  || fail "ATTACK: an observer socket every uid can connect to passed the preflight (exit $rc)"
chown "$FU" /var/lib/axon-observer/observed; PF; rc=$?
chown "$OU" /var/lib/axon-observer/observed
[ $rc = 1 ] && pf_has operator observer-dir-mode ok "owner $FU, not the observer $OU" \
  || fail "ATTACK: an observer record store the Fabric uid owns passed the preflight (exit $rc)"
cp -a /etc/axon/observer.json "$W/observer.good"
python3 -c 'import json; p="/etc/axon/observer.json"; c=json.load(open(p)); c["observer_uid"]=c["fabric_uid"]; json.dump(c, open(p,"w"), indent=2)'
PF; rc=$?; cp -a "$W/observer.good" /etc/axon/observer.json
[ $rc = 1 ] && pf_has operator observer-config-uid "$OU" "$FU" \
  || fail "ATTACK: an observer config naming the Fabric uid as the observer passed the preflight (exit $rc)"
PF || fail "control: the preflight no longer passes after the observer attacks were undone"
echo "ok(ns): the preflight REQUIRES --observer in protected mode, and FAILS (by real attempts) an observer key every uid can open, a socket every uid can connect to, a store the Fabric owns, and a config naming the Fabric as the observer (control: it PASSES on the deployment)"
# The example configs match the code's schemas (serde, unknown fields denied).
"$BIN/axon-fabric" protected-host-paths --config "$CLONE/profiles/protected-host/protected-host.json.example" \
  --launcher-config "$CLONE/profiles/protected-host/protected-launcher.json.example" \
  --custodian-config "$CLONE/profiles/protected-host/custodian.json.example" >"$W/ex.out" 2>&1 \
  || { cat "$W/ex.out"; fail "an example config does not parse"; }
python3 - "$CLONE/profiles/protected-host" <<'PY' || fail "the example configs and the deployed ones have different shapes"
import json, sys
d = sys.argv[1]
def keys(v, p=""):
    out = set()
    if isinstance(v, dict):
        for k, x in v.items(): out |= {p + k} | keys(x, p + k + ".")
    return out
for ex, live in (("protected-host.json.example", "/etc/axon/protected-host.json"),
                 ("protected-launcher.json.example", "/etc/axon/protected-launcher.json"),
                 ("custodian.json.example", "/etc/axon/custodian.json"),
                 ("observer.json.example", "/etc/axon/observer.json")):
    a, b = keys(json.load(open(f"{d}/{ex}"))), keys(json.load(open(live)))
    assert a == b, (ex, sorted(a ^ b))
assert open(f"{d}/provenance-allowlist.example").read().splitlines()[0] == "axon-provenance-allowlist/1"
PY
echo "ok(ns): the example configs parse with the code's schemas and have the deployed shapes"
# The production custodian starts on the deployed config (socket activation).
if command -v systemd-socket-activate >/dev/null; then
  CU=$(id -u axon-custodian) CG=$(id -g axon-custodian)
  systemd-socket-activate -l /run/axon-custodian/custodian.sock -- setpriv --reuid="$CU" --regid="$CG" --clear-groups \
    /usr/local/libexec/axon/axon-custodian >"$W/cust.out" 2>&1 &
  sp=$!
  for _ in $(seq 50); do [ -S /run/axon-custodian/custodian.sock ] && break; sleep 0.1; done
  sleep 1
  kill -0 "$sp" 2>/dev/null || { cat "$W/cust.out"; fail "the custodian refused the deployed config"; }
  kill "$sp" 2>/dev/null; wait "$sp" 2>/dev/null
  echo "ok(ns): the production custodian accepted the deployed config, store and activated socket"
else
  echo "NOT_RUN(ns): systemd-socket-activate absent; the custodian start was not exercised"
fi
NS
ns_run bash "$WORK/ns.sh" "$WORK" "$KIT" "$BIN" "$OP" "$SIGNER_PUB" "$CLONE" "$BUILDER_UID" "$KEYPARENT" "$BUILD_UID" "$COMMIT" \
  >"$WORK/ns.out" 2>"$WORK/ns.err"
NSRC=$?
grep -E '^(ok|NOT_RUN|FAIL)\(ns\)' "$WORK/ns.out"
if [ $NSRC != 0 ]; then
  echo "---- kit FAIL/BLOCKED/PENDING lines in the namespace ----"
  grep -E '^(FAIL|BLOCKED|PENDING|REFUSED)' "$WORK/ns.err" | head -n 40
  echo "---- kit output tail ----"; tail -n 20 "$WORK/ns.err"
  fail "the namespace apply test failed (rc $NSRC)"
fi
[ "$(snapshot)" = "$before" ] || fail "the namespace test leaked into the host"
ok "the namespace apply left the host untouched (state identical)"
echo "PASS: operator deployment kit"
