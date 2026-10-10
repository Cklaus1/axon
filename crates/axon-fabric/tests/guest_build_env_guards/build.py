# Run after shape.py (it defines good()): config keys, the dist record, pinned copies,
# the rootfs and kernel steps up to the first thing that needs the real toolchain, and
# the kernel record's judge.
import tarfile
# key_problem(path, triples): only the tree's committed keys, and only for a triple the build never uses.
WASM = ("target", "wasm32-wasip1", "rustflags")
chk("key_problem a committed key for a triple the build never compiles for", g.key_problem(WASM, {"x86_64-unknown-linux-gnu"}), None)
chk("key_problem a key the tree does not commit", g.key_problem(("build", "rustc-wrapper"), set()), "a setting the guest build would use")
chk("key_problem a committed key's sibling under the same triple", g.key_problem(("target", "wasm32-wasip1", "linker"), set()), "a setting the guest build would use")
chk("key_problem a committed key for a triple this build compiles for", g.key_problem(WASM, {"wasm32-wasip1"}), "which this build compiles for")
saved_keys = g.COMMITTED_KEYS
g.COMMITTED_KEYS = saved_keys | {("target", "cfg(unix)", "rustflags")}
chk("key_problem a committed key under a cfg(...) table", g.key_problem(("target", "cfg(unix)", "rustflags"), set()), "which this build compiles for")
g.COMMITTED_KEYS = saved_keys

# pinned_copy(src, dst, want, label): what is used is what was checked.
pc = S + "/pc"
mkdir(pc)
put(pc + "/src", "pinned bytes")
want = hashlib.sha256(b"pinned bytes").hexdigest()
chk("pinned_copy control", g.pinned_copy(pc + "/src", pc + "/dst", want, "x"), Eq(want))
chk("pinned_copy bytes other than the pinned ones", run(g.pinned_copy, pc + "/src", pc + "/dst2", "0" * 64, "busybox"), "busybox sha256 mismatch")

# dist_record(record_path, dist): the dist is byte for byte what the record says the build produced.
dd = S + "/dd"
mkdir(dd)
for nm in g.DIST_BINARIES + ("rootfs.sqfs",):
    put(dd + "/" + nm, "bytes of " + nm)
def dgg(nm):
    return hashlib.sha256(("bytes of " + nm).encode()).hexdigest()
def drec(**art):
    r = good()
    r["artifacts"] = {nm: dgg(nm) for nm in g.DIST_BINARIES}
    r["artifacts"].update(art)
    r["rootfs"] = {"sha256": dgg("rootfs.sqfs")}
    return r
rp = S + "/dist-rec.json"
put(rp, json.dumps(drec()))
g.dist_record(rp, dd)
chk("dist_record control records the digest of every dist file", sorted(json.load(open(rp))["dist"]), sorted(g.DIST_BINARIES + ("rootfs.sqfs",)))
put(rp, json.dumps(drec(axon="0" * 64)))
chk("dist_record a binary that is not the controlled step's output", run(g.dist_record, rp, dd), "dist/axon (")
r = drec(); r["rootfs"] = {"sha256": "0" * 64}
put(rp, json.dumps(r))
chk("dist_record a rootfs that is not the controlled assembly's output", run(g.dist_record, rp, dd), "dist/rootfs.sqfs (")

# rootfs(record_path, out): every input is checked against what the controlled build produced.
BB_OK = '#!/bin/sh\nif [ "$1" = "--list" ]; then echo busybox; echo sh; exit 0; fi\n'
BB_BAD = '#!/bin/sh\nexit 1\n'
def mkrepo(tag, bb_text, pinned_sha=None):
    rr = S + "/rr-" + tag
    mkdir(rr)
    subprocess.run(["/usr/bin/git", "init", "-q", "-b", "main", rr], check=True)
    bb = S + "/busybox-" + tag
    put(bb, bb_text, 0o755)
    sha = pinned_sha or hashlib.sha256(open(bb, "rb").read()).hexdigest()
    put(rr + "/profiles/linux-microvm/kernel.pin", "BUSYBOX_SRC=%s\nBUSYBOX_SHA256=%s\n" % (bb, sha))
    put(rr + "/profiles/linux-microvm/guest-init.sh", "#!/bin/sh\necho init\n")
    subprocess.run(["/usr/bin/git", "-C", rr, "add", "-A"], check=True)
    subprocess.run(["/usr/bin/git", "-C", rr, "-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "x"], check=True)
    put(rr + "/rust-toolchain.toml", '[toolchain]\nchannel = "%s"\n' % CHAN)
    return rr
def rootfs_rec(tag, bad_art=None):
    b = S + "/rf-" + tag
    mkdir(b); mkdir(b + "/target")
    r = good_at(b)
    r["artifacts"] = {}
    for nm in g.ROOTFS_BINARIES:
        put(b + "/target/" + g.MUSL + "/release/" + nm, "binary " + nm, 0o755)
        r["artifacts"][nm] = hashlib.sha256(("binary " + nm).encode()).hexdigest()
    if bad_art:
        r["artifacts"][bad_art] = "0" * 64
    p = b + "/rec.json"
    put(p, json.dumps(r))
    return p, b + "/out.sqfs"
orig_host_tool = g.host_tool
g.ROOT = mkrepo("ok", BB_OK)
p, o = rootfs_rec("a", bad_art="axon-guest-init")
chk("rootfs a binary that is not the controlled build's", run(g.rootfs, p, o), "the rootfs's axon-guest-init")
g.host_tool = lambda name: None
p, o = rootfs_rec("b")
chk("rootfs no mksquashfs in the fixed directories", run(g.rootfs, p, o), "no mksquashfs in")
g.host_tool = lambda name: "/bin/false"
p, o = rootfs_rec("c")
chk("rootfs a mksquashfs that fails", run(g.rootfs, p, o), "mksquashfs failed")
g.host_tool = orig_host_tool
g.ROOT = mkrepo("badsha", BB_OK, "0" * 64)
p, o = rootfs_rec("d")
chk("rootfs a busybox that is not the pinned bytes", run(g.rootfs, p, o), "busybox sha256 mismatch")
g.ROOT = mkrepo("badlist", BB_BAD)
p, o = rootfs_rec("e")
chk("rootfs a busybox that cannot list its applets", run(g.rootfs, p, o), "busybox --list failed")
if orig_host_tool("mksquashfs"):
    g.ROOT = mkrepo("good", BB_OK)
    p, o = rootfs_rec("f")
    chk("rootfs control assembles the image and records every input", run(g.rootfs, p, o), None)
    chk("rootfs control records the inputs", sorted(json.load(open(p))["rootfs"]["inputs"]), sorted(list(g.ROOTFS_BINARIES) + ["busybox", "guest-init.sh"]))

# kernel(record_path, dist, profile_dir): up to the first step that needs the real kernel.
def kpin(tag, tar_bytes, cfg=b"CONFIG_A=y\n", ov=b"CONFIG_B=y\n"):
    pd = S + "/kp-" + tag
    mkdir(pd)
    put(pd + "/k.config", cfg.decode())
    put(pd + "/o.config", ov.decode())
    dist = S + "/kd-" + tag
    mkdir(dist)
    with open(dist + "/linux-9.9.tar.xz", "wb") as f:
        f.write(tar_bytes)
    h = lambda b: hashlib.sha256(b).hexdigest()
    put(pd + "/kernel.pin", "KERNEL_VERSION=9.9\nKERNEL_TARBALL_SHA256=%s\nKERNEL_CONFIG=k.config\nKERNEL_CONFIG_SHA256=%s\nKERNEL_OVERLAY=o.config\nKERNEL_OVERLAY_SHA256=%s\n" % (h(tar_bytes), h(cfg), h(ov)))
    return pd, dist
def tar_with(makefile):
    import io
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w") as t:
        data = makefile.encode()
        ti = tarfile.TarInfo("linux-9.9/Makefile"); ti.size = len(data)
        t.addfile(ti, io.BytesIO(data))
    return buf.getvalue()
STUB = {n: {"path": "/usr/bin/" + n, "sha256": "e" * 64} for n in g.KERNEL_TOOLS}
orig_host_tools = g.host_tools
kp = S + "/kparent"
mkdir(kp, 0o711)
os.environ["AXON_GUEST_BUILD_PARENT"] = kp + "/p"
g.ROOT = S
pd, dist = kpin("a", b"not a tarball")
g.host_tools = lambda names: {n: t for n, t in STUB.items() if n != "gcc"}
chk("kernel a build without a required host tool", run(g.kernel, S + "/krec.json", dist, pd), "the kernel build's host tools")
g.host_tools = lambda names: STUB
chk("kernel a tarball that does not extract", run(g.kernel, S + "/krec.json", dist, pd), "the kernel tarball did not extract")
if os.path.exists("/usr/bin/make") and os.path.exists("/usr/bin/tar"):
    pd, dist = kpin("b", tar_with("olddefconfig:\n\tfalse\nvmlinux:\n\ttrue\n"))
    chk("kernel a make step that fails", run(g.kernel, S + "/krec.json", dist, pd), "failed (")
    pd, dist = kpin("c", tar_with("olddefconfig:\n\ttrue\nvmlinux:\n\ttrue\n"))
    chk("kernel a make that builds no vmlinux", run(g.kernel, S + "/krec.json", dist, pd), "vmlinux not built")
g.host_tools = orig_host_tools
cl = S + "/kclosed"
mkdir(cl, 0o700)
os.environ["AXON_GUEST_BUILD_PARENT"] = cl + "/p"
pd, dist = kpin("d", b"x")
chk("kernel a build parent the build uid cannot traverse", run(g.kernel, S + "/krec.json", dist, pd), "cannot be traversed by uid")
os.environ.pop("AXON_GUEST_BUILD_PARENT")

# kernel_problems(k, man)
kb = S + "/kbuild"
kbase = kb + "/axon-kernel-build-x"
V, E = "a" * 64, "b" * 64
want = {"version": "9.9", "tarball_sha256": "1" * 64, "config_sha256": "2" * 64, "overlay_sha256": "3" * 64}
kman = {"kernel": dict(want, effective_config_sha256=E), "artifacts": {"vmlinux": {"sha256": V}}}
MK = g.host_tool_path("make")
def kgood():
    return {"schema": g.KERNEL_SCHEMA, "controlled": True, "vmlinux_sha256": V, "effective_config_sha256": E,
            "pin": dict(want), "build_uid": 4242, "builder_uid": 0, "base": kbase, "build_parent": kb,
            "build_parent_ancestors": [{"path": p, "uid": 0, "mode": "0o755"} for p in g.prefixes(kb)],
            "env": g.kernel_env(kbase),
            "tools": {n: {"path": g.host_tool_path(n), "sha256": "f" * 64} for n in g.KERNEL_TOOLS_REQUIRED},
            "make": [[MK, "ARCH=x86_64", "olddefconfig"], [MK, "ARCH=x86_64", "-j8", "vmlinux"]]}
def kmod(f):
    k = kgood()
    f(k)
    return k
NOTVML = "its vmlinux is not the bytes a controlled kernel build produced"
NOMAKE = "its kernel's make did not run in a private directory"
chk("kernel_problems control", g.kernel_problems(kgood(), kman), "")
chk("kernel_problems no kernel record", g.kernel_problems(None, kman), NOTVML)
chk("kernel_problems another schema", g.kernel_problems(kmod(lambda k: k.update(schema="x")), kman), NOTVML)
chk("kernel_problems not marked controlled", g.kernel_problems(kmod(lambda k: k.update(controlled=1)), kman), NOTVML)
chk("kernel_problems the manifest names no vmlinux", g.kernel_problems(kgood(), {"kernel": kman["kernel"]}), NOTVML)
chk("kernel_problems another vmlinux than the manifest's", g.kernel_problems(kmod(lambda k: k.update(vmlinux_sha256="0" * 64)), kman), NOTVML)
chk("kernel_problems another effective config than the manifest's", g.kernel_problems(kmod(lambda k: k.update(effective_config_sha256="0" * 64)), kman), NOTVML)
chk("kernel_problems another pin than the manifest's", g.kernel_problems(kmod(lambda k: k["pin"].update(version="1.0")), kman), NOTVML)
for name, v in (("a string", "4242"), ("a bool", True), ("zero", 0)):
    chk("kernel_problems build_uid is " + name, g.kernel_problems(kmod(lambda k: k.update(build_uid=v)), kman), NOMAKE)
chk("kernel_problems build_uid is the builder's own", g.kernel_problems(kmod(lambda k: k.update(builder_uid=4242)), kman), NOMAKE)
chk("kernel_problems the base is not a string", g.kernel_problems(kmod(lambda k: k.update(base=5)), kman), NOMAKE)
chk("kernel_problems the base is not under the build parent", g.kernel_problems(kmod(lambda k: k.update(base=S + "/elsewhere/x")), kman), NOMAKE)
chk("kernel_problems the build parent was another uid's", g.kernel_problems(kmod(lambda k: k["build_parent_ancestors"][-1].update(uid=4243)), kman), NOMAKE)
chk("kernel_problems make ran in another environment", g.kernel_problems(kmod(lambda k: k["env"].update(PATH="/tmp")), kman), NOMAKE)
chk("kernel_problems make is not the fixed directories' make", g.kernel_problems(kmod(lambda k: k["tools"]["make"].update(path="/opt/make")), kman), NOMAKE)
chk("kernel_problems one make step", g.kernel_problems(kmod(lambda k: k.update(make=k["make"][:1])), kman), NOMAKE)
chk("kernel_problems a first step that is not olddefconfig", g.kernel_problems(kmod(lambda k: k["make"][0].__setitem__(2, "defconfig")), kman), NOMAKE)
chk("kernel_problems a second step of three words", g.kernel_problems(kmod(lambda k: k["make"][1].pop()), kman), NOMAKE)
chk("kernel_problems a second step with another ARCH", g.kernel_problems(kmod(lambda k: k["make"][1].__setitem__(1, "ARCH=arm")), kman), NOMAKE)
chk("kernel_problems a second step with no -j count", g.kernel_problems(kmod(lambda k: k["make"][1].__setitem__(2, "-jx")), kman), NOMAKE)
chk("kernel_problems a second step with another target", g.kernel_problems(kmod(lambda k: k["make"][1].__setitem__(3, "all")), kman), NOMAKE)
chk("kernel_problems a required host tool with no digest", g.kernel_problems(kmod(lambda k: k["tools"]["gcc"].pop("sha256")), kman), NOMAKE)
