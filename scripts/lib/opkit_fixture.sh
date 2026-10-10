#!/usr/bin/env bash
# opkit_fixture.sh -- the synthetic guest image and the CONTROLLED host build that
# scripts/test_operator_deploy.sh deploys. Amendment 97: this used to run as root on the REAL host
# (a builder-private parent made under /var/lib, a host build, a git commit); it now runs only
# inside ns_run, where /var/lib is a tmpfs, and hands its results out through a STASH under an
# unshadowed path (restored into later namespaces by OPKIT_RESTORE).
#
# usage (inside ns_run, OPKIT_NET=host: the host build downloads its crates):
#   opkit_fixture.sh CLONE DIST KEYPARENT BUILD_UID HOSTOUT STASH COMMIT_FILE
set -uo pipefail
. "${OPKIT_LIB:?opkit_fixture.sh runs only under ns_run}"
opkit_ns_assert || { echo "REFUSE(opkit_fixture): not in a proved namespace"; exit 97; }
CLONE=$1 DIST=$2 KEYPARENT=$3 BUILD_UID=$4 HOSTOUT=$5 STASH=$6 COMMIT_FILE=$7
G() { git -c user.name=opkit-test -c user.email=opkit-test@example.invalid "$@"; }
mkdir -p "$KEYPARENT" && chmod 0755 "$KEYPARENT" || exit 2
python3 - "$CLONE" "$DIST" "$KEYPARENT" <<'PY' || { echo "cannot write the synthetic manifest"; exit 2; }
import hashlib, importlib.util, json, os, sys
c, d, parent = sys.argv[1:4]
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("guest_build_env", os.path.join(c, "scripts", "guest_build_env.py"))
g = importlib.util.module_from_spec(spec); spec.loader.exec_module(g)
import hmac as hm
def sha(p): return hashlib.sha256(open(p, "rb").read()).hexdigest()
m = json.load(open(os.path.join(c, "profiles/linux-microvm/manifest.json")))
for n in ("vmlinux", "rootfs.sqfs"):
    m["artifacts"][n]["sha256"] = sha(os.path.join(d, n))
for k, p in (("firecracker_sha256", "/usr/local/bin/firecracker"), ("jailer_sha256", "/usr/local/bin/jailer")):
    if os.path.isfile(p): m["engine"][k] = sha(p)
chan, cargo0, rustc0 = g.toolchain()
anc, why = g.ancestors_of(parent)
assert not why, why
ids = {"build": "axon-guest-build-fixture", "kernel": "axon-kernel-build-fixture"}
def key(i):
    os.makedirs(os.path.join(parent, "keys"), mode=0o700, exist_ok=True)
    with open(os.path.join(parent, "keys", i + ".key"), "w") as f: f.write(os.urandom(32).hex())
    os.chmod(os.path.join(parent, "keys", i + ".key"), 0o400)
for i in ids.values(): key(i)
base = os.path.join(parent, ids["build"])
tdir = os.path.join(base, "toolchains", os.path.basename(os.path.dirname(os.path.dirname(cargo0))), "bin")
cargo, rustc = os.path.join(tdir, "cargo"), os.path.join(tdir, "rustc")  # the per-build private copy's paths (not made: judged only)
env = g.constructed_env(base, cargo, rustc, {})
hosts = g.host_tools(g.CARGO_HOST_TOOLS)
art = {n: m["artifacts"][n]["sha256"] for n in g.DIST_BINARIES}
check = {"origins": [], "foreign": []}
mks = "/usr/bin/mksquashfs"
rec = {"schema": g.SCHEMA, "controlled": True,
       "toolchain": {"channel": chan, "cargo": cargo, "cargo_sha256": sha(cargo0), "cargo_version": "cargo fixture",
                     "rustc": rustc, "rustc_sha256": sha(rustc0), "rustc_vV": os.popen(rustc0 + " -vV").read().strip(),
                     "host_tools": hosts, "source": {"cargo": cargo0, "rustc": rustc0}},
       "measured": {"cargo": sha(cargo0), "rustc": sha(rustc0), "bin": "0" * 64,
                    "tools": {n: t["sha256"] for n, t in hosts.items()}},
       "env": env, "env_allowlist": g.ENV_ALLOWLIST, "proxy_vars": [], "builder_uid": os.geteuid(), "build_uid": 65534,
       "build_parent": parent, "build_parent_ancestors": anc, "src_dir": os.path.join(base, "src"), "src_files": 1,
       "cargo_home": env["CARGO_HOME"], "cargo_home_created_empty": True,
       "target_dir": env["CARGO_TARGET_DIR"], "target_dir_created_empty": True,
       "effective_config": {"origins": [], "foreign": [], "own_config": ".cargo/config.toml"},
       "builds": [{"name": n, "args": a, "rustflags": rf, "config_before": check, "config_after": check}
                  for n, a, rf in g.PROTECTED_BUILDS],
       "artifacts": art,
       "rootfs": {"tool": g.tool_identity(mks, "-version"),
                  "argv": [mks, os.path.join(base, "rootfs-x"), os.path.join(d, "rootfs.sqfs"), *g.MKSQUASHFS_FLAGS],
                  "env": {"HOME": base, "LC_ALL": "C", "PATH": g.TOOL_PATH},
                  "inputs": {**art, "busybox": m["busybox"]["sha256"], "guest-init.sh": m["guest_init"]["sha256"]},
                  "sha256": m["artifacts"]["rootfs.sqfs"]["sha256"]},
       "proof": {"schema": g.PROOF_SCHEMA, "id": ids["build"], "hmac": ""}}
kbase = os.path.join(parent, ids["kernel"])
mk = m["kernel"]
tools = g.host_tools(g.KERNEL_TOOLS)
make = tools["make"]["path"]
krec = {"schema": g.KERNEL_SCHEMA, "controlled": True, "builder_uid": os.geteuid(), "build_uid": 65534, "build_parent": parent,
        "build_parent_ancestors": anc, "base": kbase,
        "pin": {"version": mk["version"], "tarball_sha256": mk["tarball_sha256"],
                "config_sha256": mk["config_sha256"], "overlay_sha256": mk["overlay_sha256"]},
        "env": g.kernel_env(kbase), "make": [[make, "ARCH=x86_64", "olddefconfig"], [make, "ARCH=x86_64", "-j8", "vmlinux"]],
        "tools": tools, "effective_config_sha256": mk["effective_config_sha256"],
        "vmlinux_sha256": m["artifacts"]["vmlinux"]["sha256"],
        "proof": {"schema": g.PROOF_SCHEMA, "id": ids["kernel"], "hmac": ""}}
for r_ in (rec, krec):  # signed as the runner would (the fixture's toolchain copy is not made, so not via g.write)
    k_, why_ = g.proof_key(parent, r_["proof"]["id"], os.geteuid(), judging=False); assert not why_, why_
    r_["proof"]["hmac"] = hm.new(k_, g.proof_payload(r_), "sha256").hexdigest()
m["source"].update({"axon_tree_dirty_at_build": False, "axon_tree_dirty_reasons": [], "build_environment": rec})
m["kernel"]["build_environment"] = krec
assert not g.shape_problems(rec), g.shape_problems(rec)
bld = (os.geteuid(), parent, 65534)
assert not g.image_problems(m, pin_required=False, builder=bld), g.image_problems(m, pin_required=False, builder=bld)
for out in (os.path.join(d, "manifest.json"), os.path.join(c, "profiles/linux-microvm/manifest.json")):
    with open(out, "w") as f:
        json.dump(m, f, indent=2); f.write("\n")
PY
(cd "$CLONE" && G add -A && G commit -q -m "opkit test: this tree's kit and a synthetic guest image") \
  || { echo "cannot commit in the scratch clone"; exit 2; }
COMMIT=$(git -C "$CLONE" rev-parse HEAD)
printf '%s\n' "$COMMIT" >"$COMMIT_FILE"

# ── the host binaries: the CONTROLLED host build of the clone (amendment 86) ──
# A fresh standalone clone, a fresh EMPTY target dir and CARGO_HOME, the constructed environment, one
# fixed invocation, and a record SIGNED by the builder (this user, under $KEYPARENT, which the kit is
# told is the pinned builder). Amendment 97: this namespace is its own PID namespace, so the runner's
# "kill every process of the build uid" sees only the build's. It builds axon-fabric and its
# dependencies from nothing every run.
BUILDER_UID=$(id -u)
(cd "$CLONE" && env -i HOME="$HOME" PATH=/usr/bin:/bin AXON_GUEST_BUILD_PARENT="$KEYPARENT" AXON_GUEST_BUILD_UID="$BUILD_UID" \
    ${http_proxy:+http_proxy="$http_proxy"} ${https_proxy:+https_proxy="$https_proxy"} \
    python3 -B scripts/guest_build_env.py host-build "$HOSTOUT") \
  || { echo "the controlled host build failed"; exit 2; }
python3 -c 'import json,sys; m=json.load(sys.stdin); sys.exit(0 if m["fabric_revision"]==sys.argv[1] and m["source_dirty"] is False and m["build_state"]=="" else 1)' \
  "$COMMIT" < <("$HOSTOUT/axon-fabric" verifier-manifest) || { echo "the build is not a clean build of $COMMIT"; exit 2; }
# hand the builder-private parent (keys included) to the later namespaces, owners and modes intact
rm -rf "$STASH" && mkdir -p "$STASH" && cp -a "$KEYPARENT/." "$STASH/" || exit 2
