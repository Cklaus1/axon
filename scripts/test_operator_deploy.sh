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
# It builds the release binaries itself from a scratch clone (into
# OPKIT_TEST_BUILD_DIR if set, so a re-run is incremental). Without root or
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
python3 - "$CLONE" "$DIST" <<'PY' || { echo "cannot write the synthetic manifest"; exit 2; }
import hashlib, json, os, shutil, sys
c, d = sys.argv[1:3]
def sha(p): return hashlib.sha256(open(p, "rb").read()).hexdigest()
def tool(p): return {"path": p, "realpath": os.path.realpath(p), "sha256": sha(p), "version": "fixture"}
m = json.load(open(os.path.join(c, "profiles/linux-microvm/manifest.json")))
for n in ("vmlinux", "rootfs.sqfs"):
    m["artifacts"][n]["sha256"] = sha(os.path.join(d, n))
for k, p in (("firecracker_sha256", "/usr/local/bin/firecracker"), ("jailer_sha256", "/usr/local/bin/jailer")):
    if os.path.isfile(p): m["engine"][k] = sha(p)
host = [p for p in ("/usr/bin/make", "/usr/bin/gcc", "/usr/bin/as", "/usr/bin/ld") if os.path.isfile(p)]
rustc = shutil.which("rustc") or "/usr/bin/env"
cargo = shutil.which("cargo") or "/usr/bin/env"
m["source"].update({"axon_tree_dirty_at_build": False, "axon_tree_dirty_reasons": [],
    "build_environment": {"schema": "axon-guest-build-env/2", "fixture": True,
        "toolchain": {"channel": "fixture", "rustc": rustc, "rustc_sha256": sha(rustc),
                      "rustc_vV": "rustc fixture", "cargo": cargo, "cargo_sha256": sha(cargo),
                      "cargo_version": "cargo fixture",
                      "host_tools": {os.path.basename(p): tool(p) for p in host[-1:]}},
        "rootfs": {"tool": tool("/usr/bin/env")}}})
m["kernel"]["build_environment"] = {"schema": "axon-guest-kernel-build/1", "fixture": True,
                                    "tools": {os.path.basename(p): tool(p) for p in host}}
for out in (os.path.join(d, "manifest.json"), os.path.join(c, "profiles/linux-microvm/manifest.json")):
    with open(out, "w") as f:
        json.dump(m, f, indent=2); f.write("\n")
PY
(cd "$CLONE" && G add -A && G commit -q -m "opkit test: this tree's kit and a synthetic guest image") \
  || { echo "cannot commit in the scratch clone"; exit 2; }
COMMIT=$(git -C "$CLONE" rev-parse HEAD)

# ── release binaries built from the clone (production: no test-trust) ───────
BUILD_DIR=${OPKIT_TEST_BUILD_DIR:-$WORK/build}
(cd "$CLONE" && env -u RUSTC_WRAPPER CARGO_TARGET_DIR="$BUILD_DIR" cargo build -q --release --locked -p axon-fabric --bins) \
  || { echo "cargo build --release -p axon-fabric --bins failed"; exit 2; }
BIN=$BUILD_DIR/release
python3 -c 'import json,sys; m=json.load(sys.stdin); sys.exit(0 if m["fabric_revision"]==sys.argv[1] and m["source_dirty"] is False else 1)' \
  "$COMMIT" < <("$BIN/axon-fabric" verifier-manifest) || { echo "the build is not a clean build of $COMMIT"; exit 2; }
mkdir -p "$CLONE/dist" && cp -a "$DIST" "$CLONE/dist/guest-linux"

# ── operator data fixtures ─────────────────────────────────────────────────────
OP=$WORK/op
mkdir -p "$OP/suites/bin" "$OP/suites/checks/c1" "$OP/grants"
cp /usr/bin/true "$OP/suites/bin/axon"; echo '// fixture' >"$OP/suites/checks/c1/t.ax"
cp /usr/bin/true "$OP/observer"
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
ARGS=(--from "$CLONE" --bin-dir "$BIN" --observer-bin "$OP/observer" --suite-registry "$OP/suites/registry.json"
      --grant-registry "$OP/grants/grants.json" --signer-public-key "$SIGNER_PUB")

# ══ 1. DRY RUN ═══════════════════════════════════════════════════════════════
snapshot() {
  { for p in /etc/axon /usr/local/libexec/axon /usr/local/lib/axon /var/lib/axon-fabric /var/lib/axon-custodian \
             /var/lib/axon-protected-launcher /var/lib/axon-loop /var/lib/axon-deploy \
             /etc/systemd/system/axon-custodian.socket /etc/systemd/system/axon-custodian.service; do
      [ -e "$p" ] && find "$p" -printf '%p %u:%g %m %s %T@\n'; done
    getent passwd axon-fabric axon-custodian axon-verifier; getent group axon-fabric axon-custodian axon-verifier
  } 2>/dev/null | sort
}
before=$(snapshot)
OUT=$(bash "$KIT" "${ARGS[@]}" 2>&1); RC=$?
[ "$(snapshot)" = "$before" ] || fail "the dry run changed the host"
ok "a dry run writes nothing (host state identical before and after)"
case $RC in 0|3) ;; *) fail "dry run exited $RC: $OUT" ;; esac
grep -q "nothing is written" <<<"$OUT" || fail "the dry run does not say it writes nothing"
want=(
  'PLAN\[allowlist\] install /etc/axon/provenance-allowlist root:root 644'
  '\| axon-provenance-allowlist/1$' '\| target/$' '\| dist/$'
  '(PLAN|OK)\[users\] .*axon-fabric' '(PLAN|OK)\[users\] .*axon-custodian' '(PLAN|OK)\[users\] .*axon-verifier'
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
  'PLAN\[configs\] install /etc/axon/protected-launcher.json root:root 644'
  'PLAN\[configs\] install /etc/axon/protected-host.json root:root 644'
  '"schema": "axon-custodian/1"' '"schema": "axon-protected-launcher/2"' '"schema": "axon-protected-host/1"'
  '"launcher_uid": 0' '"authority_store": "/var/lib/axon-loop/store"' '"socket": "/run/axon-custodian/custodian.sock"'
  'PLAN\[verifier\] install /etc/axon/trust/verifier.json root:root 644' '"schema": "axon-verifier-manifest/1"'
  'PLAN\[loader\] setpriv --reuid axon-fabric'
  'PLAN\[systemd\] install /etc/systemd/system/axon-custodian.socket root:root 644'
  'PLAN\[systemd\] install /etc/systemd/system/axon-custodian.service root:root 644'
  '\| SocketMode=0660' '\| SocketGroup=axon-fabric' '\| User=axon-custodian'
  '\| ExecStart=/usr/local/libexec/axon/axon-custodian'
  'PLAN\[systemd\] systemctl enable --now axon-custodian.socket'
  'PLAN\[toolchain\] install /etc/axon/host-toolchain-pin.json root:root 644' '"schema": "axon-host-toolchain-pin/1"'
  'PLAN\[preflight\] bash .*trust_root_preflight.sh --verifier axon-verifier --custodian axon-custodian --fabric axon-fabric'
)
for w in "${want[@]}"; do grep -Eq -- "$w" <<<"$OUT" || fail "the plan lacks /$w/:
$OUT"; done
ok "the dry run plans every required action (${#want[@]} assertions: allowlist, users, dirs, binaries, guest, data, configs, verifier, loader, systemd, toolchain, preflight)"
# Pins in the planned configs are the bytes that would be installed.
python3 - "$OUT" "$(sha256sum "$CLONE/scripts/fc_linux_profile.sh" | cut -c1-64)" \
  "$(sha256sum "$BIN/axon-protected-launcher" | cut -c1-64)" "$(sha256sum "$CLONE/dist/guest-linux/manifest.json" | cut -c1-64)" \
  "$(sha256sum "$OP/observer" | cut -c1-64)" "$SIGNER_PUB" <<'PY' || fail "a planned pin is not the source bytes' digest"
import json, re, sys
out, launcher, helper, manifest, observer, signer = sys.argv[1:7]
def doc(path):
    m = re.search(r"  ---- %s \(root:root 644\) ----\n(.*?)\n  ---- end %s ----" % (re.escape(path), re.escape(path)), out, re.S)
    return json.loads("\n".join(l[4:] for l in m.group(1).splitlines()))
h, l = doc("/etc/axon/protected-host.json"), doc("/etc/axon/protected-launcher.json")
assert h["launcher"]["sha256"] == launcher == l["launcher"]["sha256"], "launcher pin"
assert h["privileged_launcher"]["sha256"] == helper, "helper pin"
assert h["profile_manifest"]["sha256"] == manifest == l["profile_manifest"]["sha256"], "manifest pin"
assert h["observer"]["command"]["sha256"] == observer, "observer pin"
assert h["signer"]["public_key"] == signer == l["observer"]["host_signer_public_key"], "signer"
assert h["observer"]["custodian"] == l["custodian"], "one custodian"
assert h["out_root"] == l["out_root"] == "/var/lib/axon-fabric/runs"
assert h["qualification"]["max_age_s"] == 2592000
PY
ok "planned pins: launcher, helper, profile manifest, observer and host signer are the installed bytes' (and agree across host and helper configs)"

refused() { # label pattern cmd...
  local label=$1 pat=$2; shift 2
  local o rc; o=$("$@" 2>&1); rc=$?
  [ $rc = 2 ] || fail "$label: expected REFUSED (2), got $rc: $o"
  grep -q -- "$pat" <<<"$o" || fail "$label: refused for another reason: $o"
  ok "$label is refused"
}
touch "$CLONE/untracked-file"
refused "a clone with an untracked file" "is dirty" bash "$KIT" "${ARGS[@]}"
rm -f "$CLONE/untracked-file"
mkdir -p "$CLONE/scripts/__pycache__"; touch "$CLONE/scripts/__pycache__/x.pyc"
refused "a clone with a git-ignored file outside target/ and dist/ (decision C)" "is dirty" bash "$KIT" "${ARGS[@]}"
rm -rf "$CLONE/scripts/__pycache__"
echo "# edit" >>"$CLONE/README.md"
refused "a clone with a modified tracked file" "is dirty" bash "$KIT" "${ARGS[@]}"
git -C "$CLONE" checkout -q -- README.md
git -C "$CLONE" worktree add -q "$WORK/linked" HEAD 2>/dev/null || fail "cannot make a linked worktree"
refused "a linked worktree" "linked worktree" bash "$WORK/linked/scripts/operator_deploy_protected_host.sh" \
  --from "$WORK/linked" --bin-dir "$BIN"
git -C "$CLONE" worktree remove --force "$WORK/linked"
mkdir "$WORK/symgit"; cp -a "$CLONE/." "$WORK/symgit/"; rm -rf "$WORK/symgit/.git"; ln -s "$CLONE/.git" "$WORK/symgit/.git"
refused "a symlinked .git" "not a real directory" bash "$WORK/symgit/scripts/operator_deploy_protected_host.sh" \
  --from "$WORK/symgit" --bin-dir "$BIN"
cp "$KIT" "$WORK/other-kit.sh"; echo "# not the clone's" >>"$WORK/other-kit.sh"
refused "a kit that is not the clone's own copy" "not the clone's own" bash "$WORK/other-kit.sh" "${ARGS[@]}"
mkdir "$WORK/fakebin"
for b in axon-fabric axon-protected-launcher axon-custodian; do cp "$BIN/$b" "$WORK/fakebin/$b"; done
printf '#!/bin/sh\necho %s\n' "'{\"build\":\"production\",\"profile\":\"release\",\"source_dirty\":false,\"fabric_revision\":\"0000000000000000000000000000000000000000\"}'" \
  >"$WORK/fakebin/axon-fabric"; chmod 0755 "$WORK/fakebin/axon-fabric"
refused "a verifier built from another commit" "not a clean production release build" \
  bash "$KIT" --from "$CLONE" --bin-dir "$WORK/fakebin"
if [ "$(id -u)" = 0 ]; then
  refused "--apply as a non-root uid" "must run as root" setpriv --reuid=65534 --regid=65534 --clear-groups -- \
    bash "$KIT" "${ARGS[@]}" --apply
fi

# ══ 2. APPLY in a private mount namespace ════════════════════════════════════
if [ "$(id -u)" != 0 ] || ! command -v unshare >/dev/null; then
  echo "NOT_RUN: the namespace apply test needs root and unshare (dry-run assertions above held; not a full pass)"
  exit 0
fi
cp /usr/local/bin/firecracker /usr/local/bin/jailer "$WORK/" 2>/dev/null || { echo "NOT_RUN: no /usr/local/bin/firecracker+jailer to deploy against"; exit 0; }
cat >"$WORK/ns.sh" <<'NS'
set -uo pipefail
W=$1 KIT=$2 BIN=$3 OP=$4 SIGNER_PUB=$5 CLONE=$6
fail() { echo "FAIL(ns): $*"; exit 1; }
# Isolation first: nothing below may reach the host.
mkdir "$W/etc"; mount -t tmpfs -o mode=0755 tmpfs "$W/etc" && cp -a /etc/. "$W/etc/" && mount --bind "$W/etc" /etc \
  || fail "cannot shadow /etc"
for d in /usr/local /var/lib /var/log /var/spool /run /srv; do
  mount -t tmpfs -o mode=0755 tmpfs "$d" || fail "cannot shadow $d"
done
[ -L /var/mail ] || mount -t tmpfs -o mode=0755 tmpfs /var/mail || fail "cannot shadow /var/mail"
mkdir -p /usr/local/bin && install -m 0755 "$W/firecracker" "$W/jailer" /usr/local/bin/
grep -q axon-fabric /etc/passwd && fail "the shadow /etc already has axon users"
ARGS=(--from "$CLONE" --bin-dir "$BIN" --observer-bin "$OP/observer" --suite-registry "$OP/suites/registry.json"
      --grant-registry "$OP/grants/grants.json" --signer-public-key "$SIGNER_PUB" --no-systemctl)
# Runbook order: allowlist, users and directories, THEN the operator provisions
# keys (here: inert fixtures), THEN the whole kit.
bash "$KIT" "${ARGS[@]}" --only allowlist,users,dirs --apply >"$W/apply1.out" 2>&1 \
  || { r=$?; [ $r = 3 ] && grep -q 'authority store' "$W/apply1.out" || { cat "$W/apply1.out"; fail "first apply exited $r"; }; }
FU=$(id -u axon-fabric) || fail "axon-fabric was not created"
id -u axon-custodian >/dev/null && id -u axon-verifier >/dev/null || fail "users not created"
[ "$(getent passwd axon-custodian | cut -d: -f7)" = /usr/sbin/nologin ] || fail "custodian has a login shell"
for a in qualification observer verifier; do install -d -o root -g root -m 0755 "/etc/axon/trust/$a"; done
rnd() { head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n'; }
rnd >/etc/axon/trust/qualification/operator.pub; rnd >/etc/axon/trust/observer/observer.pub
echo "$SIGNER_PUB" >/etc/axon/trust/verifier/host-signer.pub
chmod 0644 /etc/axon/trust/*/*.pub
printf 'x' >/etc/axon/keys/fabric-attest.pk8   # one byte: a fixture, never a key
chown axon-fabric:axon-fabric /etc/axon/keys/fabric-attest.pk8; chmod 0400 /etc/axon/keys/fabric-attest.pk8
mkdir -p /var/lib/axon-loop/store
# The loader reads only the B263 record's presence and ownership (Fabric's
# qualification() verifies it at each launch); a shaped fixture, unsigned.
python3 - "$W/b263.json" /dev/stdin <<PY
import hashlib, json, sys
man = hashlib.sha256(open("$CLONE/dist/guest-linux/manifest.json", "rb").read()).hexdigest()
json.dump({"schema": "axon-b263-evidence/1", "issuer_key_id": "ed25519:0000000000000000", "result": "PASS",
           "profile": {"manifest_sha256": man}, "source": {"tree_dirty": False}, "fixture": "never signed"},
          open(sys.argv[1], "w"))
PY
echo '{"fixture":"not a signature"}' >"$W/b263.json.sig"
# What systemd does when the socket unit starts (no systemctl in here).
install -d -o root -g root -m 0755 /run/axon-custodian
GUEST="unshare --mount --propagation private sh -c 'mount -t tmpfs none /etc/axon && sh $CLONE/scripts/trust_root_guest_probe.sh /etc/axon/trust'"
bash "$KIT" "${ARGS[@]}" --b263-record "$W/b263.json" --agent 40003 --agent 40004 --guest-cmd "$GUEST" --apply >"$W/apply2.out" 2>&1
r=$?; cat "$W/apply2.out" >&2
[ $r = 0 ] || fail "the full apply exited $r"
grep -q 'OK\[loader\] ProtectedHost::operator() accepted' "$W/apply2.out" || fail "the production loader did not accept the deployment"
grep -q 'PREFLIGHT verdict PASS mode protected' "$W/apply2.out" || fail "the protected-mode preflight did not PASS"
rep=$(sed -n 's/^PREFLIGHT verdict PASS mode protected report \([^ ]*\) .*/\1/p' "$W/apply2.out")
python3 -c 'import json,sys; r=json.load(open(sys.argv[1])); assert r["mode"]=="protected" and r["verdict"]=="PASS" and r["root"]=="/etc/axon/trust" and len(r["checks"])>40, r["verdict"]' "$rep" \
  || fail "the preflight report is not a protected PASS"
echo "ok(ns): full apply: production loader accepts, protected-mode trust preflight PASS ($(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["checks"]))' "$rep") checks)"
m() { stat -c '%U:%G %a' "$1"; }
[ "$(m /usr/local/libexec/axon/axon-protected-launcher)" = "root:axon-fabric 4750" ] || fail "helper mode $(m /usr/local/libexec/axon/axon-protected-launcher)"
for f in /etc/axon/protected-host.json /etc/axon/protected-launcher.json /etc/axon/custodian.json /etc/axon/provenance-allowlist /etc/axon/trust/verifier.json; do
  [ "$(m "$f")" = "root:root 644" ] || fail "$f is $(m "$f")"
done
[ "$(m /var/lib/axon-fabric/runs)" = "axon-fabric:axon-fabric 700" ] || fail "out root"
[ "$(m /var/lib/axon-protected-launcher)" = "root:root 700" ] || fail "staging root"
[ "$(m /var/lib/axon-custodian/nonces)" = "axon-custodian:axon-custodian 700" ] || fail "store"
python3 - <<'PY' || fail "a config pin is not its installed file's digest"
import hashlib, json
def sha(p): return hashlib.sha256(open(p, "rb").read()).hexdigest()
h = json.load(open("/etc/axon/protected-host.json")); l = json.load(open("/etc/axon/protected-launcher.json"))
for d in (h["launcher"], h["privileged_launcher"], h["profile_manifest"], h["suite_registry"], h["grant_registry"],
          h["observer"]["command"], l["interpreter"], l["launcher"], l["profile_manifest"]):
    assert sha(d["path"]) == d["sha256"], d
v = json.load(open("/etc/axon/trust/verifier.json"))
assert v["path"] == "/usr/local/libexec/axon/axon-fabric" and v["sha256"] == sha(v["path"]), v
PY
echo "ok(ns): modes and owners as the code requires; every pin is its INSTALLED file's digest"
# Idempotent: a second apply plans nothing.
bash "$KIT" "${ARGS[@]}" --b263-record "$W/b263.json" --agent 40003 --guest-cmd "$GUEST" --apply >"$W/apply3.out" 2>&1 \
  || { cat "$W/apply3.out"; fail "the second apply failed"; }
if grep -E 'PLAN\[[a-z]+\] (install|dir|useradd|groupadd)' "$W/apply3.out"; then fail "the second apply changed something"; fi
echo "ok(ns): a second --apply changes nothing (idempotent)"
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
                 ("custodian.json.example", "/etc/axon/custodian.json")):
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
unshare -m --propagation private bash "$WORK/ns.sh" "$WORK" "$KIT" "$BIN" "$OP" "$SIGNER_PUB" "$CLONE" \
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
