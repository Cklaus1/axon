# shape_problems / entry_problems: what a recorded controlled build must say.
import copy
CHAN = "nightly-2099-01-01"
put(S + "/rust-toolchain.toml", '[toolchain]\nchannel = "%s"\n' % CHAN)
parent = S + "/bld"
base = parent + "/axon-guest-build-abc"
tdir = base + "/toolchains/" + CHAN + "-x86_64-unknown-linux-gnu/bin"
cargo, rustc = tdir + "/cargo", tdir + "/rustc"
C, R, T1, T2 = "c" * 64, "d" * 64, "1" * 64, "2" * 64
def good_at(base, parent=None):
    parent = parent or os.path.dirname(base)
    tdir = base + "/toolchains/" + CHAN + "-x86_64-unknown-linux-gnu/bin"
    cargo, rustc = tdir + "/cargo", tdir + "/rustc"
    env = g.constructed_env(base, cargo, rustc, {})
    return {
        "schema": g.SCHEMA, "controlled": True,
        "toolchain": {"rustc_vV": "rustc 1.0", "rustc_sha256": R, "cargo_sha256": C, "cargo": cargo,
                      "rustc": rustc, "channel": CHAN,
                      "host_tools": {"cc": {"sha256": T1}, "ld": {"sha256": T2}}},
        "env": env, "proxy_vars": [], "target_dir": env["CARGO_TARGET_DIR"], "cargo_home": env["CARGO_HOME"],
        "build_uid": 4242, "builder_uid": 0,
        "measured": {"cargo": C, "rustc": R, "tools": {"cc": T1, "ld": T2}},
        "target_dir_created_empty": True, "cargo_home_created_empty": True,
        "effective_config": {"foreign": []},
        "build_parent": parent,
        "build_parent_ancestors": [{"path": p, "uid": 0, "mode": "0o755"} for p in g.prefixes(parent)],
        "src_dir": base + "/src", "builds": [],
    }
def good():
    return good_at(base)
def with_(path, v):
    r = good()
    d = r
    for k in path[:-1]:
        d = d[k]
    if v is DEL:
        del d[path[-1]]
    else:
        d[path[-1]] = v
    return r
DEL = object()
IS = "is not a %s or %s record of a controlled build" % (g.SCHEMA, g.HOST_SCHEMA)
chk("shape_problems control", g.shape_problems(good()), "")
chk("shape_problems a record that is not an object", g.shape_problems([]), IS)
chk("shape_problems another schema", g.shape_problems(with_(["schema"], "x")), IS)
chk("shape_problems a host-build schema is a controlled build's too", g.shape_problems(with_(["schema"], g.HOST_SCHEMA)), "")
chk("shape_problems not marked controlled", g.shape_problems(with_(["controlled"], 1)), IS)
chk("shape_problems the controlled mark is missing", g.shape_problems(with_(["controlled"], DEL)), IS)
NOTC = "records no toolchain identity or environment"
chk("shape_problems a toolchain that is not an object", g.shape_problems(with_(["toolchain"], [])), NOTC)
chk("shape_problems an environment that is not an object", g.shape_problems(with_(["env"], [])), NOTC)
for f in ("rustc_vV", "rustc_sha256", "cargo", "rustc"):
    chk("shape_problems the toolchain names no " + f, g.shape_problems(with_(["toolchain", f], "")), NOTC)
NOTCH = "its toolchain is not the pinned channel"
chk("shape_problems another channel than the pinned one", g.shape_problems(with_(["toolchain", "channel"], "nightly-2000-01-01")), NOTCH)
r = good(); r["toolchain"]["rustc"] = tdir + "/../x/rustc"
chk("shape_problems rustc is not beside cargo", g.shape_problems(r), NOTCH)
r = good()
other = parent + "/axon-guest-build-abc/toolchains/stable-x/bin"
r["toolchain"]["cargo"] = other + "/cargo"; r["toolchain"]["rustc"] = other + "/rustc"
chk("shape_problems a toolchain directory not named for the channel", g.shape_problems(r), NOTCH)
r = good(); r["toolchain"]["host_tools"]["cc"] = {}
chk("shape_problems no compiler identity", g.shape_problems(r), NOTCH)
r = good(); r["toolchain"]["host_tools"]["ld"] = {}
chk("shape_problems no linker identity", g.shape_problems(r), NOTCH)
r = good(); del r["toolchain"]["host_tools"]
chk("shape_problems no host tools at all", g.shape_problems(r), NOTCH)
saved_root = g.ROOT
g.ROOT = S + "/no-toolchain-file-here"
chk("shape_problems no pinned channel in the tree", g.shape_problems(good()), NOTCH)
g.ROOT = saved_root
ENVM = "cargo did not run in the environment the build constructs"
r = good(); r["env"]["RUSTFLAGS"] = "-C linker=/tmp/x"
chk("shape_problems an extra variable in cargo's environment", g.shape_problems(r), ENVM)
r = good(); r["env"]["PATH"] = "/tmp:/usr/bin"
chk("shape_problems another PATH than the fixed one", g.shape_problems(r), ENVM)
r = good(); del r["env"]["HOME"]
chk("shape_problems a variable of the constructed environment missing", g.shape_problems(r), ENVM)
r = good(); r["cargo_home"] = base + "/elsewhere"
chk("shape_problems cargo_home is not the constructed one", g.shape_problems(r), ENVM)
r = good(); r["target_dir"] = base + "/elsewhere"
chk("shape_problems the target dir is not the constructed one", g.shape_problems(r), ENVM)
BU = "did not run as an unprivileged uid of their own"
for name, v in (("a string", "4242"), ("a bool", True), ("zero", 0), ("negative", -1)):
    chk("shape_problems build_uid is " + name, g.shape_problems(with_(["build_uid"], v)), BU)
chk("shape_problems build_uid is the builder's own", g.shape_problems(with_(["builder_uid"], 4242)), BU)
r = good()
mv = parent + "/axon-guest-build-abc/not-toolchains/" + CHAN + "-x/bin"
r["toolchain"]["cargo"] = mv + "/cargo"; r["toolchain"]["rustc"] = mv + "/rustc"
r["env"] = g.constructed_env(base, mv + "/cargo", mv + "/rustc", {})
chk("shape_problems a toolchain that is not the build's private copy", g.shape_problems(r), BU)
MS = "its measurement of the compiler and linker tools"
chk("shape_problems measured cargo is not the toolchain's", g.shape_problems(with_(["measured", "cargo"], "x")), MS)
chk("shape_problems measured rustc is not the toolchain's", g.shape_problems(with_(["measured", "rustc"], "x")), MS)
chk("shape_problems measured tools are not an object", g.shape_problems(with_(["measured", "tools"], [])), MS)
chk("shape_problems the measured compiler is not the recorded one", g.shape_problems(with_(["measured", "tools", "cc"], "x")), MS)
chk("shape_problems the measured linker is not the recorded one", g.shape_problems(with_(["measured", "tools", "ld"], "x")), MS)
r = good(); del r["measured"]
chk("shape_problems nothing measured", g.shape_problems(r), MS)
FRESH = "not a fresh one the build created"
chk("shape_problems the target dir was not created empty", g.shape_problems(with_(["target_dir_created_empty"], False)), FRESH)
chk("shape_problems cargo_home was not created empty", g.shape_problems(with_(["cargo_home_created_empty"], 1)), FRESH)
chk("shape_problems cargo's config held a foreign setting", g.shape_problems(with_(["effective_config", "foreign"], ["x"])), "held settings the build would use")
r = good(); del r["effective_config"]
chk("shape_problems no effective config recorded", g.shape_problems(r), "held settings the build would use")
PRIV = "private copy of the tree in a directory only root or the builder could write"
r = good(); r["build_parent_ancestors"][-1]["uid"] = 4243
chk("shape_problems the build parent was another uid's", g.shape_problems(r), PRIV)
chk("shape_problems the source is not the build's own copy", g.shape_problems(with_(["src_dir"], S + "/elsewhere")), PRIV)
r = good(); r["build_parent"] = S + "/other-parent"
r["build_parent_ancestors"] = [{"path": p, "uid": 0, "mode": "0o755"} for p in g.prefixes(S + "/other-parent")]
chk("shape_problems the build directory is not under the build parent", g.shape_problems(r), PRIV)
# entry_problems
name0, args0, rf0 = g.PROTECTED_BUILDS[0]
def entry(**kv):
    b = {"name": name0, "args": args0, "rustflags": rf0, "config_before": {"origins": None, "foreign": []}, "config_after": {"origins": None, "foreign": []}}
    b.update(kv)
    return b
def with_build(b):
    r = good(); r["builds"] = [b]
    return r
chk("entry_problems control", g.shape_problems(with_build(entry())), "")
chk("entry_problems an entry that is not an object", g.shape_problems(with_build("x")), "a recorded build is not an object")
chk("entry_problems an entry with no name", g.shape_problems(with_build(entry(name=None))), "not its controlled invocation")
chk("entry_problems other args than the table's", g.shape_problems(with_build(entry(args=args0 + ["--features", "x"]))), "not its controlled invocation")
chk("entry_problems other RUSTFLAGS than the table's", g.shape_problems(with_build(entry(rustflags="-C linker=/tmp/x"))), "not its controlled invocation")
chk("entry_problems another name for those args", g.shape_problems(with_build(entry(name="axon-guest-init"))), "not its controlled invocation")
NOCFG = "has no effective-config check equal to begin's"
chk("entry_problems no config check before the build", g.shape_problems(with_build(entry(config_before={"x": 1}))), NOCFG)
chk("entry_problems no config check after the build", g.shape_problems(with_build(entry(config_after={"x": 1}))), NOCFG)
