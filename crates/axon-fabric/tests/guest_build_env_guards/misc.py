# The tree copy, the clone, the committed file, the pinned tools and the dist judge.
def git(*a, cwd=None):
    subprocess.run(["/usr/bin/git", "-c", "user.name=t", "-c", "user.email=t@example", "-C", cwd or repo, *a],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
repo = S + "/repo"
mkdir(repo)
git("init", "-q", "-b", "main")
put(repo + "/a.txt", "a")
put(repo + "/sub/b.txt", "b")
os.symlink("a.txt", repo + "/lnk")
put(repo + "/gone.txt", "g")
put(repo + "/fifo.txt", "f")
git("add", "-A")
git("commit", "-q", "-m", "fixture")
g.ROOT = repo
os.remove(repo + "/gone.txt")
os.remove(repo + "/fifo.txt")
os.mkfifo(repo + "/fifo.txt")
d1 = S + "/dst1"
mkdir(d1)
chk("copy_tracked_tree a tracked path that is a FIFO", run(g.copy_tracked_tree, d1), "is neither a file nor a symlink")
os.remove(repo + "/fifo.txt")
put(repo + "/fifo.txt", "f")
d2 = S + "/dst2"
mkdir(d2)
chk("copy_tracked_tree control copies tracked files and symlinks, skips a deleted one", g.copy_tracked_tree(d2), 4)
chk("copy_tracked_tree copies a symlink as a symlink", os.path.islink(d2 + "/lnk"), True)
chk("copy_tracked_tree copies file content", open(d2 + "/sub/b.txt").read(), Eq("b"))
chk("copy_tracked_tree copies nothing untracked", os.path.exists(d2 + "/zz"), False)
g.ROOT = S + "/norepo"
mkdir(g.ROOT)
chk("copy_tracked_tree not a git tree", run(g.copy_tracked_tree, S + "/dst3"), "cannot list the tree's tracked files")
chk("git_clone not a git tree", run(g.git_clone, S + "/clone0"), "cannot clone the tree")
g.ROOT = S + "/emptyrepo"
mkdir(g.ROOT)
subprocess.run(["/usr/bin/git", "init", "-q", "-b", "main", g.ROOT], check=True)
chk("git_clone an empty repository has no HEAD to be at", run(g.git_clone, S + "/clone1"), "the clone is not at the tree's HEAD")
g.ROOT = repo
rev = g.git_clone(S + "/clone2")
chk("git_clone control returns the HEAD revision", len(rev), 40)
chk("git_clone control: the clone holds the tracked file", os.path.isfile(S + "/clone2/a.txt"), True)
chk("committed_file control", g.committed_file("a.txt"), b"a")
chk("committed_file a path the committed tree does not hold", run(g.committed_file, "nope.txt"), "cannot read nope.txt from the committed tree")

# private_toolchain(base, chan, cargo, rustc): a root-owned private copy.
tc1 = S + "/tc1"
mkdir(tc1 + "/bin")
put(tc1 + "/bin/cargo", "#!/bin/sh\n", 0o755)
put(tc1 + "/bin/rustc", "#!/bin/sh\n", 0o755)
tc2 = S + "/tc2"
mkdir(tc2 + "/bin")
put(tc2 + "/bin/rustc", "#!/bin/sh\n", 0o755)
b1 = S + "/pb1"
mkdir(b1)
chk("private_toolchain cargo and rustc of two toolchains", run(g.private_toolchain, b1, "c", tc1 + "/bin/cargo", tc2 + "/bin/rustc"), "not of one toolchain directory")
tc3 = S + "/tc3"
mkdir(tc3 + "/bin")
put(tc3 + "/bin/cargo", "#!/bin/sh\n", 0o755)
put(tc3 + "/bin/rustc", "#!/bin/sh\n", 0o755)
put(tc3 + "/bin/evil", "x", 0o775)
b2 = S + "/pb2"
mkdir(b2)
chk("private_toolchain a toolchain a group could write", run(g.private_toolchain, b2, "c", tc3 + "/bin/cargo", tc3 + "/bin/rustc"), "is not root's")
b3 = S + "/pb3"
mkdir(b3)
c, r = g.private_toolchain(b3, "c", tc1 + "/bin/cargo", tc1 + "/bin/rustc")
chk("private_toolchain control puts the copy under base/toolchains", c, b3 + "/toolchains/tc1/bin/cargo")
chk("private_toolchain the copy has the pinned bytes", g.sha256(c) == g.sha256(tc1 + "/bin/cargo") and g.sha256(r) == g.sha256(tc1 + "/bin/rustc"), True)

# run_cargo / load / the command line.
chk("run_cargo without the -- separator", run(g.run_cargo, S + "/none.json", ["build"]), "usage: cargo RECORD")
junk = S + "/junk.json"
put(junk, json.dumps({"schema": "x"}))
chk("load a record that is not a controlled build's", run(g.load, junk), "is not a controlled build record")

def cli(*a):
    r = subprocess.run([sys.executable, "-B", S + "/scripts/guest_build_env.py", *a], capture_output=True, text=True,
                       env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"})
    return r.returncode, r.stdout + r.stderr
rc, t = cli("no-such-verb")
chk("main an unknown verb prints the usage and fails", rc != 0 and t.startswith("The controlled environment"), True)
rc, t = cli("check-host-record", S, "--bogus", "1")
chk("main check-host-record with an unknown option prints the usage", rc != 0 and t.startswith("The controlled environment"), True)
rc, t = cli("check-host-record", S, "--commit")
chk("main check-host-record with an odd argument count prints the usage", rc != 0 and t.startswith("The controlled environment"), True)
rc, t = cli("check-host-record", S, "--builder-uid", "0")
chk("main check-host-record with a partial builder triple", t, "the builder flags come together")
rc, t = cli("check-host-record", S, "--builder-uid", "x", "--builder-parent", "/p", "--build-uid", "65534")
chk("main check-host-record with a builder uid that is not decimal", t, "plain decimal uids")
rc, t = cli("check-host-record", S, "--builder-uid", "0", "--builder-parent", "/p", "--build-uid", "y")
chk("main check-host-record with a build uid that is not decimal", t, "plain decimal uids")
rc, t = cli("check-host-record", S, "--builder-uid", "0", "--builder-parent", "rel", "--build-uid", "65534")
chk("main check-host-record with a relative builder parent", t, "plain decimal uids")
rc, t = cli("check-host-record", S, "--builder-uid", "0", "--builder-parent", "/p", "--build-uid", "65534")
chk("main check-host-record names why a directory is not a host build", t, "holds no readable host-build.json")

# check-build-uid UID BUILDER-UID: the kit's verb, judged by build_uid_problem (root, the builder's own uid, a service uid).
rc, t = cli("check-build-uid", "x", "1000")
chk("main check-build-uid with a uid that is not decimal", rc != 0 and "both uids are plain decimal" in t, True)
rc, t = cli("check-build-uid", "4242", "1000")
chk("main check-build-uid control: a dedicated uid is accepted", rc == 0 and "is dedicated" in t, True)
rc, t = cli("check-build-uid", "0", "1000")
chk("main check-build-uid refuses root", rc != 0 and "it is root" in t, True)
rc, t = cli("check-build-uid", "1000", "1000")
chk("main check-build-uid refuses the builder's own uid", rc != 0 and "it is the builder's own uid" in t, True)
chk("build_uid_problem root", g.build_uid_problem(0, 1000, ()), "it is root")
chk("build_uid_problem the builder's own uid", g.build_uid_problem(1000, 1000, ()), "it is the builder's own uid")
chk("build_uid_problem control: a dedicated uid", g.build_uid_problem(4242, 1000, ()), "")

# toolchain_pin_problems(man, required, pin_path): the image's tools against the operator's pin.
pdir = S + "/tp"
mkdir(pdir)
TP = g.TOOLCHAIN_PIN_SCHEMA
tools = {"cc": {"path": "/usr/bin/cc", "sha256": "1" * 64},
         "rustc": {"path": "/opt/tc/rustc", "sha256": "d" * 64},
         "cargo": {"path": "/opt/tc/cargo", "sha256": "c" * 64}}
man = {"source": {"build_environment": {"toolchain": {
    "host_tools": {"cc": dict(tools["cc"])},
    "rustc": "/x/rustc", "rustc_sha256": "d" * 64, "cargo": "/x/cargo", "cargo_sha256": "c" * 64,
    "source": {"rustc": "/opt/tc/rustc", "cargo": "/opt/tc/cargo"}}}}}
chk("toolchain_pin recorded_host_tools names what the build recorded", sorted(g.recorded_host_tools(man)), ["cargo", "cc", "rustc"])
def tpin(name, body):
    p = pdir + "/" + name
    put(p, body if isinstance(body, str) else json.dumps(body))
    return p
def tp(m, pin_path, required=True):
    return g.toolchain_pin_problems(m, required, pin_path)
chk("toolchain_pin control", tp(man, tpin("ok", {"schema": TP, "tools": tools})), "")
chk("toolchain_pin no pin, one required", tp(man, pdir + "/absent"), "there is no operator host-toolchain pin")
chk("toolchain_pin no pin, none required", tp(man, pdir + "/absent", False), "")
os.symlink(pdir + "/ok", pdir + "/lnk")
chk("toolchain_pin a pin that is not an operator file", tp(man, pdir + "/lnk"), "is not the operator's")
chk("toolchain_pin an unreadable pin", tp(man, tpin("junk", "{nope")), "is unreadable")
NAMING = "naming tools"
chk("toolchain_pin a pin that is not an object", tp(man, tpin("lst", [])), NAMING)
chk("toolchain_pin another pin schema", tp(man, tpin("sch", {"schema": "x", "tools": tools})), NAMING)
chk("toolchain_pin tools that are not an object", tp(man, tpin("tl", {"schema": TP, "tools": []})), NAMING)
chk("toolchain_pin a pin naming no tool", tp(man, tpin("emp", {"schema": TP, "tools": {}})), NAMING)
more = dict(tools); del more["cc"]
chk("toolchain_pin a recorded tool the pin does not name", tp(man, tpin("nocc", {"schema": TP, "tools": more})), "which the operator's pin does not name")
extra = dict(tools); extra["ld"] = {"path": "/usr/bin/ld", "sha256": "2" * 64}
chk("toolchain_pin a pinned tool the build did not record", tp(man, tpin("ld", {"schema": TP, "tools": extra})), "the build recorded None")
notdict = dict(tools); notdict["cc"] = "x"
chk("toolchain_pin a pinned tool that is not an object", tp(man, tpin("nd", {"schema": TP, "tools": notdict})), "which the operator's pin does not name")
other_path = dict(tools); other_path["cc"] = {"path": "/usr/bin/other", "sha256": "1" * 64}
chk("toolchain_pin a tool at another path than the pin's", tp(man, tpin("pth", {"schema": TP, "tools": other_path})), "not the operator's pin")
other_sha = dict(tools); other_sha["cc"] = {"path": "/usr/bin/cc", "sha256": "9" * 64}
chk("toolchain_pin a tool of other bytes than the pin's", tp(man, tpin("sha", {"schema": TP, "tools": other_sha})), "not the operator's pin")
nosha = dict(tools); nosha["cc"] = {"path": "/usr/bin/cc"}
man2 = json.loads(json.dumps(man)); man2["source"]["build_environment"]["toolchain"]["host_tools"]["cc"] = {"path": "/usr/bin/cc"}
chk("toolchain_pin a recorded tool with no digest, though the pin names none either", tp(man2, tpin("nos", {"schema": TP, "tools": nosha})), "not the operator's pin")
# With no flags the pin comes from the operator's builder-pin file; a host that has none refuses (a host that
# has one is judged by it, so the case is made only where the pin is absent: every test host).
if not os.path.lexists(g.BUILDER_PIN):
    rc, t = cli("check-host-record", S)
    chk("main check-host-record with no flags and no operator builder pin", t, "the builder pin")
