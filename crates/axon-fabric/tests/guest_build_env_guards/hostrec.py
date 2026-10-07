# Run after pins.py and shape.py: the host build's record, the dist judge, the rootfs judge.
B3 = (0, bp, 4242)
def bytes_of(name):
    return "host-binary-" + name
def emit(outdir, rec, sign=True, files=True):
    mkdir(outdir)
    if files:
        for n in g.HOST_BINARIES:
            put(outdir + "/" + n, bytes_of(n), 0o755)
    if sign:
        rec["proof"] = {"schema": g.PROOF_SCHEMA, "id": PID, "hmac": ""}
        key = g.proof_key(bp, PID, 0)[0]
        rec["proof"]["hmac"] = hmac.new(key, g.proof_payload(rec), "sha256").hexdigest()
    put(outdir + "/" + g.HOST_RECORD, json.dumps(rec))
def host_rec(**kv):
    r = good()
    r["schema"] = g.HOST_SCHEMA
    nm, args, rf = g.HOST_INVOCATIONS[0]
    cfg = {"origins": None, "foreign": []}
    r["builds"] = [{"name": nm, "args": args, "rustflags": rf, "config_before": cfg, "config_after": cfg}]
    r["source_revision"] = "a" * 40
    r["artifacts"] = {n: hashlib.sha256(bytes_of(n).encode()).hexdigest() for n in g.HOST_BINARIES}
    r.update(kv)
    return r
n = [0]
def outd():
    n[0] += 1
    return S + "/host-out-%d" % n[0]
HR = "the host build record"
o = outd(); emit(o, host_rec())
chk("host_record_problems control", g.host_record_problems(o, B3), "")
chk("host_record_problems control with the revision the kit deploys", g.host_record_problems(o, B3, "a" * 40), "")
chk("host_record_problems another revision than the kit deploys", g.host_record_problems(o, B3, "b" * 40), "the host build was made from revision")
o = outd(); mkdir(o)
chk("host_record_problems no record in the directory", g.host_record_problems(o, B3), "holds no readable host-build.json")
o = outd(); mkdir(o); put(o + "/host-build.json", "{nope")
chk("host_record_problems a record that is not JSON", g.host_record_problems(o, B3), "holds no readable host-build.json")
o = outd(); emit(o, host_rec(schema="x"))
chk("host_record_problems another schema", g.host_record_problems(o, B3), "is not a %s record" % g.HOST_SCHEMA)
o = outd(); emit(o, host_rec(build_uid=0))
chk("host_record_problems a record that is not a controlled build's", g.host_record_problems(o, B3), "is not a controlled build's")
r = host_rec(); r["builds"] = []
o = outd(); emit(o, r)
chk("host_record_problems no host invocation recorded", g.host_record_problems(o, B3), "not exactly the one controlled host invocation")
r = host_rec(); r["builds"].append(dict(r["builds"][0]))
o = outd(); emit(o, r)
chk("host_record_problems the invocation recorded twice", g.host_record_problems(o, B3), "not exactly the one controlled host invocation")
for name, v in (("not a string", 5), ("not hex", "g" * 40), ("short", "a" * 39), ("upper-case", "A" * 40)):
    o = outd(); emit(o, host_rec(source_revision=v))
    chk("host_record_problems a revision that is " + name, g.host_record_problems(o, B3), "the host build was made from revision")
NODIG = "names no digest for exactly"
o = outd(); emit(o, host_rec(artifacts=[]))
chk("host_record_problems artifacts that are not an object", g.host_record_problems(o, B3), NODIG)
r = host_rec(); del r["artifacts"]["axon-observer"]
o = outd(); emit(o, r)
chk("host_record_problems a binary without a digest", g.host_record_problems(o, B3), NODIG)
r = host_rec(); r["artifacts"]["extra"] = "0" * 64
o = outd(); emit(o, r)
chk("host_record_problems a digest for a binary that is not one", g.host_record_problems(o, B3), NODIG)
r = host_rec(); r["artifacts"]["axon-fabric"] = 5
o = outd(); emit(o, r)
chk("host_record_problems a digest that is not a string", g.host_record_problems(o, B3), NODIG)
r = host_rec(); r["artifacts"]["axon-fabric"] = "xyz"
o = outd(); emit(o, r)
chk("host_record_problems a digest that is not 64 hex", g.host_record_problems(o, B3), NODIG)
o = outd(); emit(o, host_rec(), sign=False)
chk("host_record_problems an unsigned record", g.host_record_problems(o, B3), "carries no builder proof")
o = outd(); emit(o, host_rec())
rj = json.load(open(o + "/host-build.json")); rj["proof"]["hmac"] = "0" * 64
put(o + "/host-build.json", json.dumps(rj))
chk("host_record_problems a record whose proof does not hold", g.host_record_problems(o, B3), "does not hold")
BYTES = "is not the bytes the controlled host build recorded"
o = outd(); emit(o, host_rec()); os.remove(o + "/axon-fabric")
chk("host_record_problems a binary that is missing", g.host_record_problems(o, B3), BYTES)
o = outd(); emit(o, host_rec()); os.remove(o + "/axon-fabric"); os.symlink(o + "/axon-custodian", o + "/axon-fabric")
chk("host_record_problems a binary that is a symlink", g.host_record_problems(o, B3), BYTES)
o = outd(); emit(o, host_rec()); put(o + "/axon-fabric", "other bytes", 0o755)
chk("host_record_problems a binary of other bytes", g.host_record_problems(o, B3), BYTES)
o = outd(); emit(o, host_rec()); put(o + "/stray", "x")
chk("host_record_problems a file the build did not make", g.host_record_problems(o, B3), "holds files the controlled host build did not make")

# dist_problems(dist, benv, kbuild, builder)
BD = (0, bp, 65534)
DB = g.DIST_BINARIES + ("rootfs.sqfs",)
def dist_dir(tag, extra=None):
    d = S + "/dist-" + tag
    mkdir(d)
    for nme in DB + (("vmlinux", "effective.config") if extra else ()):
        put(d + "/" + nme, "bytes of " + nme)
    return d
def dg(nme):
    return hashlib.sha256(("bytes of " + nme).encode()).hexdigest()
def benv(**kv):
    return signed(dist={nme: dg(nme) for nme in DB}, **kv)
d0 = dist_dir("a")
chk("dist_problems control, no kernel record", g.dist_problems(d0, benv(), None, BD), "")
chk("dist_problems an unsigned build record", g.dist_problems(d0, {"x": 1}, None, BD), "carries no builder proof")
b = benv(); del b["dist"]["rootfs.sqfs"]
b = signed(dist={k: v for k, v in b["dist"].items()})
chk("dist_problems a build record naming not every dist artifact", g.dist_problems(d0, b, None, BD), "names no digest for every dist artifact")
b = signed(dist={nme: dg(nme) for nme in DB + ("extra",)})
chk("dist_problems a build record naming a dist artifact too many", g.dist_problems(d0, b, None, BD), "names no digest for every dist artifact")
d1 = dist_dir("b", True)
kb = signed(vmlinux_sha256=dg("vmlinux"), effective_config_sha256=dg("effective.config"))
chk("dist_problems control with a kernel record", g.dist_problems(d1, benv(), kb, BD), "")
chk("dist_problems an unsigned kernel record", g.dist_problems(d1, benv(), {"vmlinux_sha256": "x"}, BD), "kernel build record carries no builder proof")
chk("dist_problems a kernel record of another vmlinux", g.dist_problems(d1, benv(), signed(vmlinux_sha256="0" * 64, effective_config_sha256=dg("effective.config")), BD), "dist/vmlinux is not the bytes")
chk("dist_problems a kernel record of another effective config", g.dist_problems(d1, benv(), signed(vmlinux_sha256=dg("vmlinux"), effective_config_sha256="0" * 64), BD), "dist/effective.config is not the bytes")
d2 = dist_dir("c"); os.remove(d2 + "/axon")
chk("dist_problems a dist file that is missing", g.dist_problems(d2, benv(), None, BD), "dist/axon is not the bytes")
d3 = dist_dir("d"); put(d3 + "/axon-psv-runner", "tampered")
chk("dist_problems a dist file of other bytes", g.dist_problems(d3, benv(), None, BD), "dist/axon-psv-runner is not the bytes")
d4 = dist_dir("e"); os.remove(d4 + "/rootfs.sqfs"); mkdir(d4 + "/rootfs.sqfs")
chk("dist_problems a dist artifact that is a directory", g.dist_problems(d4, benv(), None, BD), "dist/rootfs.sqfs is not the bytes")

# rootfs_problems(rec, man): the rootfs is the bytes `rootfs` made from this record's artifacts.
RBASE = S + "/rb"
SQ, BB, GIH = "5" * 64, "6" * 64, "7" * 64
arts = {nm: ("%d" % i) * 64 for i, nm in enumerate(g.ROOTFS_BINARIES)}
def rfs(**kv):
    want = dict(arts); want["busybox"] = BB; want["guest-init.sh"] = GIH
    mk = g.host_tool_path("mksquashfs")
    r = {"sha256": SQ, "inputs": want, "tool": {"path": mk, "sha256": "8" * 64},
         "argv": [mk, "in", "out"] + g.MKSQUASHFS_FLAGS, "env": {"HOME": RBASE, "LC_ALL": "C", "PATH": g.TOOL_PATH}}
    r.update(kv)
    return r
def rrec(r=None):
    return {"rootfs": r if r is not None else rfs(), "artifacts": arts, "target_dir": RBASE + "/target"}
rman = {"artifacts": {"rootfs.sqfs": {"sha256": SQ}}, "busybox": {"sha256": BB}, "guest_init": {"sha256": GIH}}
NOTMADE = "its rootfs.sqfs is not the bytes the controlled rootfs assembly produced"
chk("rootfs_problems control", g.rootfs_problems(rrec(), rman), "")
chk("rootfs_problems no rootfs recorded", g.rootfs_problems({"artifacts": arts}, rman), NOTMADE)
chk("rootfs_problems no rootfs.sqfs in the manifest", g.rootfs_problems(rrec(), {"busybox": {"sha256": BB}, "guest_init": {"sha256": GIH}}), NOTMADE)
chk("rootfs_problems another rootfs.sqfs than the record's", g.rootfs_problems(rrec(rfs(sha256="0" * 64)), rman), NOTMADE)
chk("rootfs_problems inputs that are not the artifacts and pins", g.rootfs_problems(rrec(rfs(inputs={"axon": "x"})), rman), NOTMADE)
chk("rootfs_problems no busybox pin in the manifest", g.rootfs_problems(rrec(rfs(inputs=dict(rfs()["inputs"], busybox=None))), dict(rman, busybox={})), NOTMADE)
NOTBY = "its rootfs was not made by"
mk = g.host_tool_path("mksquashfs")
chk("rootfs_problems a tool at another path than the system's", g.rootfs_problems(rrec(rfs(tool={"path": "/opt/mksquashfs", "sha256": "8" * 64})), rman), NOTBY)
chk("rootfs_problems a tool with no digest", g.rootfs_problems(rrec(rfs(tool={"path": mk})), rman), NOTBY)
chk("rootfs_problems an argv that does not start with the tool", g.rootfs_problems(rrec(rfs(argv=["/opt/x", "in", "out"] + g.MKSQUASHFS_FLAGS)), rman), NOTBY)
chk("rootfs_problems an argv with other flags", g.rootfs_problems(rrec(rfs(argv=[mk, "in", "out"] + g.MKSQUASHFS_FLAGS + ["-extra"])), rman), NOTBY)
chk("rootfs_problems an environment that is not the constructed one", g.rootfs_problems(rrec(rfs(env={"HOME": RBASE, "LC_ALL": "C", "PATH": "/tmp"})), rman), NOTBY)

# image_problems(man, pin_required, builder): the first judgements.
chk("image_problems no build environment", g.image_problems({}, False, B3), "it records no build environment")
chk("image_problems builds other than the protected ones in order", g.image_problems({"source": {"build_environment": {"builds": [{"name": "axon"}]}}}, False, B3), "not exactly the protected builds in order")
