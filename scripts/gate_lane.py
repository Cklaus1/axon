#!/usr/bin/env python3
"""The gate's lane machinery (tasks/gate-lane-spec.md GL-2 … GL-4). NOT the gate.

    gate_lane.py check [--audit] [--quiet]   registry governance/gate-lanes.json against gate.sh, the manifest
                                  and cargo; with --audit (the lane's check) also, given the stored audit of an
                                  ancestor, that every audited unit can be selected. The full gate runs it
                                  without --audit: its result never depends on the untracked audit store.
    gate_lane.py units            the derived units, in full-gate order
    gate_lane.py audit            the read-set audit of HEAD (GL-3; non-ship, slow: every unit, one by one)
    gate_lane.py select / expand / footer   called by `gate.sh --lane` and `--lane-units=` (GL-4)

The registry holds only what cannot be derived. Units come from `cargo metadata`: a `per-target` stage gets one
unit per target its cargo test command runs (`--lib`, each `--bin`, `--test`, `--example`, and `--doc`); any other
stage is one unit. Lane logic lives here, not in gate.sh: the clippy-coverage check greps gate.sh's own text for
`-p <crate>`, so crate names written into gate.sh for the lane would fake coverage.
"""
import hashlib
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REGISTRY = "governance/gate-lanes.json"
MANIFEST = "governance/release-verification.json"


def load(root, rel):
    with open(os.path.join(root, rel)) as f:
        return json.load(f)


_META = {}


def cargo_metadata(root):
    """`cargo metadata --no-deps --offline` of the workspace at root (cached per root)."""
    if root not in _META:
        out = subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"],
                             cwd=root, capture_output=True, text=True)
        if out.returncode != 0:
            raise SystemExit(f"gate_lane: cargo metadata failed in {root}:\n{out.stderr[-2000:]}")
        _META[root] = json.loads(out.stdout)
    return _META[root]


def norm(s):
    return " ".join(s.split())


LANE_PREFIX = re.compile(r"^lane_cargo \S+ ")


class Gate:
    """gate.sh as text: its stage blocks (stage_begin … stage_end), par group and joined command lines."""

    def __init__(self, text):
        self.lines = text.split("\n")
        self.blocks = {}   # id -> list of (lineno, line)
        self.labels = {}   # id -> stage_begin label
        self.order = []    # stage ids in file order
        self.par = []      # par group names, in order
        cur = None
        for i, l in enumerate(self.lines):
            m = re.match(r'^\s*stage_begin (\S+) "([^"]*)"', l)
            if m:
                cur = m.group(1)
                self.order.append(cur)
                self.blocks[cur] = []
                self.labels[cur] = m.group(2)
                continue
            if re.match(r"^\s*stage_end(\s|$)", l):
                cur = None
                continue
            if cur:
                self.blocks[cur].append((i + 1, l))
            m = re.match(r'^(?:\s*if lane_runs P; then\s*)?par "([^"]+)" ', l)
            if m and not m.group(1).startswith("selftest"):
                self.par.append(m.group(1))

    def commands(self, sid):
        """The block's command lines: comments dropped, `\\` continuations joined, whitespace normalised, a
        leading `lane_cargo <ID> ` removed."""
        out, acc = [], ""
        for _, l in self.blocks.get(sid, []):
            if re.match(r"^\s*#", l):
                continue
            acc += " " + l.rstrip()
            if acc.endswith("\\"):
                acc = acc[:-1]
                continue
            c = LANE_PREFIX.sub("", norm(acc))
            acc = ""
            if c:
                out.append(c)
        return out

    def packages(self, sid):
        """Every `-p <pkg>` of the block's cargo commands, in order."""
        pk = []
        for c in self.commands(sid):
            if "cargo " in c:
                for p in re.findall(r"(?:^|\s)-p (\S+)", c):
                    if p not in pk:
                        pk.append(p)
        return pk


def read_gate(root):
    with open(os.path.join(root, "scripts", "gate.sh")) as f:
        return Gate(f.read())


def manifest_commands(manifest):
    """(crate, command) the gate's manifest stage runs, in its order (gate.sh's heredoc: class A/B, not
    axon-core)."""
    out = []
    for name, spec in sorted(manifest["crates"].items()):
        if spec.get("class") not in ("A", "B") or name == "axon-core":
            continue
        for c in spec.get("required", []):
            out.append((name, c))
    return out


# ── units ──────────────────────────────────────────────────────────────────

def parse_cargo(cmd):
    """verb, packages, enabled-feature request and explicit target filters of a `cargo …` command."""
    t = cmd.split()
    assert t[0] == "cargo", cmd
    r = {"verb": t[1], "packages": [], "features": [], "no_default": False, "filters": []}
    i = 2
    while i < len(t):
        a = t[i]
        if a == "-p":
            r["packages"].append(t[i + 1]); i += 1
        elif a == "--features":
            r["features"] += t[i + 1].split(","); i += 1
        elif a == "--no-default-features":
            r["no_default"] = True
        elif a in ("--lib", "--doc", "--bins", "--tests", "--examples"):
            r["filters"].append((a[2:], None))
        elif a in ("--bin", "--test", "--example", "--bench"):
            r["filters"].append((a[2:], t[i + 1])); i += 1
        elif a == "--":
            break
        i += 1
    return r


def packages_by_name(meta):
    return {p["name"]: p for p in meta["packages"]}


def enabled_features(pkg, no_default, features):
    on, todo = set(), ([] if no_default else ["default"]) + list(features)
    while todo:
        f = todo.pop()
        if f in pkg["features"] and f not in on:
            on.add(f)
            todo += [x for x in pkg["features"][f] if not x.startswith("dep:") and "/" not in x]
    return on


LIB_KINDS = {"lib", "rlib", "staticlib", "cdylib", "dylib", "proc-macro"}


def target_units(pkg, req):
    """(kind, name, cargo flags) of every target `cargo test -p pkg` with req's features runs or builds,
    cargo's default selection: lib, bins and benches with test = true, every integration test, every example
    (built), and the lib's doctests."""
    on = enabled_features(pkg, req["no_default"], req["features"])
    out = []
    for t in pkg["targets"]:
        if not set(t.get("required-features", [])) <= on:
            continue
        kinds = set(t["kind"])
        if kinds & LIB_KINDS:
            if t.get("test", True):
                out.append(("lib", None, ["--lib"]))
            if t.get("doctest", False) and kinds & {"lib", "rlib", "proc-macro"}:
                out.append(("doc", None, ["--doc"]))
        elif "bin" in kinds and t.get("test", True):
            out.append(("bin", t["name"], ["--bin", t["name"]]))
        elif "test" in kinds:
            out.append(("test", t["name"], ["--test", t["name"]]))
        elif "example" in kinds:
            out.append(("example", t["name"], ["--example", t["name"]]))
        elif "bench" in kinds and t.get("test", False):
            out.append(("bench", t["name"], ["--bench", t["name"]]))
    return out


def unit_id(stage, pkg, kind, name):
    return ":".join([stage, pkg, kind] + ([name] if name else []))


def split_units(stage, cmd, pkgs):
    """Per-target units of one cargo test command (an explicit target filter keeps only those targets)."""
    req = parse_cargo(cmd)
    out = []
    if req["verb"] != "test":
        for p in req["packages"]:
            out.append({"id": unit_id(stage, p, req["verb"], None), "stage": stage, "pkg": p, "kind": "whole",
                        "packages": req["packages"], "cmd": cmd})
        return out
    for p in req["packages"]:
        for kind, name, flags in target_units(pkgs[p], req):
            if req["filters"] and (kind, name) not in req["filters"] and (kind, None) not in req["filters"]:
                continue
            out.append({"id": unit_id(stage, p, kind, name), "stage": stage, "pkg": p, "kind": kind,
                        "name": name, "flags": flags, "packages": [p], "cmd": cmd})
    return out


def derive(root, reg=None, gate=None, manifest=None, meta=None):
    """Units and build steps in full-gate order: a list of steps, each {"step": "unit"|"build", …}."""
    reg = reg or load(root, REGISTRY)
    gate = gate or read_gate(root)
    manifest = manifest or load(root, MANIFEST)
    pkgs = packages_by_name(meta or cargo_metadata(root))
    builds = {b["stage"]: [] for b in reg["builds"]}
    for b in reg["builds"]:
        builds[b["stage"]].append(b)
    steps, seen = [], set()

    def add_unit(u):
        if u["id"] not in seen:  # S20 runs axon-fabric's `workspace` test twice; one unit
            seen.add(u["id"])
            steps.append(dict(u, step="unit"))

    for s in reg["stages"]:
        sid = s["id"]
        if s["class"] != "selectable":
            continue
        for b in builds.get(sid, []):
            p = parse_cargo(b["cmd"])
            steps.append({"step": "build", "id": b["id"], "stage": sid, "cmd": b["cmd"],
                          "packages": p["packages"], "filters": p["filters"], "standalone": s["split"] != "none"})
        if s["split"] == "none":
            add_unit({"id": sid, "stage": sid, "kind": "whole", "packages": gate.packages(sid),
                      "compile": s.get("compile")})
            continue
        if s.get("lint_unit"):
            lint = [c for c in gate.commands(sid) if c.startswith("cargo clippy")]
            add_unit({"id": sid + ":lint", "stage": sid, "kind": "whole",
                      "packages": [p for c in lint for p in parse_cargo(c)["packages"]]})
        cmds = ([c for _, c in manifest_commands(manifest)] if s["split_cmd"] == "manifest" else [s["split_cmd"]])
        for c in cmds:
            for u in split_units(sid, c, pkgs):
                add_unit(u)
    return steps


def units(steps):
    return [s for s in steps if s["step"] == "unit"]


# ── check ──────────────────────────────────────────────────────────────────

def check(root, reg=None, gate=None, manifest=None, meta=None):
    """Every way the registry can disagree with gate.sh, the manifest or cargo; [] when it agrees."""
    reg = reg or load(root, REGISTRY)
    gate = gate or read_gate(root)
    manifest = manifest or load(root, MANIFEST)
    bad = []
    rows = [s["id"] for s in reg["stages"]]
    s_by_id = {s["id"]: s for s in reg["stages"]}
    for sid in gate.order:
        if sid not in rows:
            bad.append(f"gate.sh stage {sid} has no row in {REGISTRY}")
    for sid in rows:
        if sid not in gate.blocks:
            bad.append(f"{REGISTRY} row {sid} names a stage gate.sh no longer has")
        elif s_by_id[sid].get("banner") != gate.labels[sid]:
            bad.append(f"{REGISTRY} row {sid} banner '{s_by_id[sid].get('banner')}' is not gate.sh's "
                       f"stage_begin label '{gate.labels[sid]}'")
    for n in gate.par:
        if n not in reg["par"]:
            bad.append(f"gate.sh par stage '{n}' has no row in {REGISTRY}")
    for n in reg["par"]:
        if n not in gate.par:
            bad.append(f"{REGISTRY} par row '{n}' names a par stage gate.sh no longer has")
    s20 = next((s for s in reg["stages"] if s.get("split_cmd") == "manifest"), {"crates": []})
    mcr = sorted({c for c, _ in manifest_commands(manifest)})
    for c in mcr:
        if c not in s20["crates"]:
            bad.append(f"manifest crate {c} ({MANIFEST}) has no row in {REGISTRY}")
    for c in s20["crates"]:
        if c not in mcr:
            bad.append(f"{REGISTRY} manifest row {c} names a crate the manifest stage no longer runs")
    for s in reg["stages"]:
        if s.get("split_cmd") and s["split_cmd"] != "manifest" and s["id"] in gate.blocks:
            if not any(c.startswith(norm(s["split_cmd"]) + " ||") for c in gate.commands(s["id"])):
                bad.append(f"{REGISTRY} {s['id']} split_cmd '{s['split_cmd']}' is not gate.sh's text")
    reg_builds = {}
    for b in reg["builds"]:
        reg_builds.setdefault(b["stage"], []).append(norm(b["cmd"]))
        if b["stage"] not in gate.blocks:
            bad.append(f"{REGISTRY} build {b['id']} names stage {b['stage']}, which gate.sh no longer has")
        elif not any(c.startswith(norm(b["cmd"]) + " ||") for c in gate.commands(b["stage"])):
            bad.append(f"{REGISTRY} build {b['id']} '{b['cmd']}' differs from gate.sh's text in {b['stage']}")
    for sid in gate.order:
        for c in gate.commands(sid):
            if c.startswith("cargo build") and not any(c.startswith(r + " ||") for r in reg_builds.get(sid, [])):
                bad.append(f"gate.sh {sid} runs '{c.split(' ||')[0]}', which has no build row in {REGISTRY}")
    return bad


def audit_problems(audit, us):
    """`lane_every_stage_reachable` on an audit: every audited unit has a non-empty read set (compile ∪
    runtime); one with none could never be selected by a change and would be skipped forever."""
    bad = []
    for u in us:
        a = audit["units"].get(u["id"])
        if a and a["status"] == "ok" and not unit_paths(audit, a)[0] and not a["reads"]:
            bad.append(f"unit {u['id']} has an empty read set in audit {audit['commit'][:12]}")
    return bad


# ── shared: git, the audit store ───────────────────────────────────────────

def git(root, *args):
    out = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)
    if out.returncode != 0:
        raise SystemExit(f"gate_lane: git {' '.join(args)} failed: {out.stderr.strip()}")
    return out.stdout


def toolchain():
    """(rustc -vV text, its sha256[:8])."""
    v = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout
    return v, hashlib.sha256(v.encode()).hexdigest()[:8]


def uid_class():
    return "root" if os.geteuid() == 0 else "user"


def store_dir(root):
    common = git(root, "rev-parse", "--path-format=absolute", "--git-common-dir").strip()
    return os.path.join(common, "axon-gate-lanes")


def find_audit(root):
    """The newest audit of this toolchain and uid class whose commit is an ancestor of HEAD, or None."""
    d = store_dir(root)
    if not os.path.isdir(d):
        return None
    _, tc8 = toolchain()
    best = None
    for f in os.listdir(d):
        m = re.match(r"^([0-9a-f]{40})-([0-9a-f]{8})-(root|user)\.json$", f)
        if not m or m.group(2) != tc8 or m.group(3) != uid_class():
            continue
        a = m.group(1)
        if subprocess.run(["git", "merge-base", "--is-ancestor", a, "HEAD"], cwd=root,
                          capture_output=True).returncode != 0:
            continue
        dist = int(git(root, "rev-list", "--count", f"{a}..HEAD"))
        if best is None or dist < best[0]:
            best = (dist, os.path.join(d, f))
    if best is None:
        return None
    return load("/", best[1])


# ── GL-3: the read-set audit ───────────────────────────────────────────────
#
# A non-ship run: a fresh `git clone --shared` of commit A, every derived step run in full-gate order by
# `gate.sh --lane-units=<step>`, each unit between two sentinels opened in a watched directory. The inotify
# queue is FIFO within one instance, so an event before a unit's end sentinel belongs to that unit or an
# earlier one; events between one step's end and the next step's begin are given to both neighbours.
# It never runs run_managed.sh and never writes a gate receipt: its only output is the audit file.

IN_OPEN, IN_CREATE, IN_Q_OVERFLOW, IN_ISDIR, IN_NONBLOCK = 0x20, 0x100, 0x4000, 0x40000000, 0o4000
GIT_HISTORY = ("HEAD", "ORIG_HEAD", "FETCH_HEAD", "index", "packed-refs")  # and anything under .git/refs/


class Attributor:
    """Per-step observations from an event stream, fed one event at a time (the audit never holds the stream).
    Events (dicts, as in scripts/testdata/gate_lane/): begin/end {step}, open {path}, dir {path},
    exec {where, name}, git {path}, overflow. `obs`: {step: {"reads", "dirs", "execs", "git", "overflow"}}."""

    def __init__(self, steps):
        self.obs = {s: {"reads": set(), "dirs": set(), "execs": set(), "git": False, "overflow": False}
                    for s in steps}
        order = list(steps)
        self.first = order[:1]
        self.nxt = {a: b for a, b in zip(order, order[1:])}
        self.cur, self.prev = None, None  # inside `cur`; or after `prev`'s end, before the next begin

    def targets(self):
        if self.cur is not None:
            return [self.cur]
        if self.prev is None:
            return self.first
        return [self.prev] + ([self.nxt[self.prev]] if self.prev in self.nxt else [])

    def feed(self, e):
        t = e["t"]
        if t == "begin":
            self.cur = e["step"]
            return
        if t == "end":
            self.prev, self.cur = e["step"], None
            return
        for s in self.targets():
            o = self.obs[s]
            if t == "open":
                o["reads"].add(e["path"])
            elif t == "dir":
                o["dirs"].add(e["path"])
            elif t == "exec":
                o["execs"].add((e["where"], e["name"]))
            elif t == "git":
                o["git"] = True
            elif t == "overflow":
                o["overflow"] = True


def attribute(events, steps):
    a = Attributor(steps)
    for e in events:
        a.feed(e)
    return a.obs


def path_deps(meta):
    """package -> workspace packages it depends on by path (normal, dev and build)."""
    root_of = {os.path.dirname(p["manifest_path"]): p["name"] for p in meta["packages"]}
    out = {}
    for p in meta["packages"]:
        out[p["name"]] = sorted({root_of[os.path.realpath(d["path"])] for d in p["dependencies"]
                                 if d.get("path") and os.path.realpath(d["path"]) in root_of})
    return out


def closure(pkgs, deps):
    seen, todo = set(), list(pkgs)
    while todo:
        q = todo.pop()
        if q not in seen:
            seen.add(q)
            todo += deps.get(q, [])
    return sorted(seen)


def bin_owners(meta):
    """(examples?, binary name) -> package, for every workspace bin and example target."""
    out = {}
    for p in meta["packages"]:
        for t in p["targets"]:
            if "bin" in t["kind"]:
                out[(False, t["name"])] = p["name"]
            elif "example" in t["kind"]:
                out[(True, t["name"])] = p["name"]
    return out


def parse_dep_info(path, root):
    """In-repo paths (relative to root) a cargo dep-info file names. Entries are absolute (bin .d files) or
    workspace-relative (deps/*.d: `crates/…`, `Cargo.toml`); both resolve against root."""
    out = set()
    root = os.path.normpath(root)
    with open(path, errors="replace") as f:
        for line in f:
            if ":" not in line or line.startswith("#"):
                continue
            for p in re.split(r"(?<!\\) ", line.split(": ", 1)[1] if ": " in line else ""):
                p = p.strip().replace("\\ ", " ")
                if not p:
                    continue
                ap = os.path.normpath(os.path.join(root, p))
                rel = os.path.relpath(ap, root)
                if rel not in (".", "..") and not rel.startswith("../") and "/target/" not in f"/{rel}":
                    out.add(rel)
    return out


def package_compile_sets(meta, root, target):
    """§A per package: dep-info paths of its targets, its build script's rerun-if-changed paths, its Cargo.toml
    and build.rs; `crates/Q/` as a prefix when the target dir holds no dep-info for it."""
    prof = os.path.join(target, "debug")
    deps_dir = os.path.join(prof, "deps")
    listing = os.listdir(deps_dir) if os.path.isdir(deps_dir) else []
    by_crate = {}
    for f in listing:
        if f.endswith(".d"):
            by_crate.setdefault(f.rsplit("-", 1)[0], []).append(os.path.join(deps_dir, f))
    build_dir = os.path.join(prof, "build")
    builds = os.listdir(build_dir) if os.path.isdir(build_dir) else []
    out = {}
    for p in meta["packages"]:
        q = p["name"]
        pdir = os.path.relpath(os.path.dirname(p["manifest_path"]), root)
        files, prefixes, found = {f"{pdir}/Cargo.toml"}, set(), False
        if os.path.exists(os.path.join(root, pdir, "build.rs")):
            files.add(f"{pdir}/build.rs")
        for t in p["targets"]:
            crate = t["name"].replace("-", "_")
            ds = list(by_crate.get(crate, []))
            if "bin" in t["kind"]:
                ds.append(os.path.join(prof, t["name"] + ".d"))
            if "custom-build" in t["kind"]:
                ds = [os.path.join(build_dir, b, f) for b in builds if b.rsplit("-", 1)[0] == q
                      for f in os.listdir(os.path.join(build_dir, b)) if f.endswith(".d")]
            for d in ds:
                if os.path.isfile(d):
                    got = parse_dep_info(d, root)
                    files |= got
                    found = found or any(f.startswith(pdir + "/") for f in got)
        for b in builds:
            o = os.path.join(build_dir, b, "output")
            if b.rsplit("-", 1)[0] != q or not os.path.isfile(o):
                continue
            for line in open(o, errors="replace"):
                m = re.match(r"^cargo::?rerun-if-changed=(.+)$", line.strip())
                if not m:
                    continue
                ap = os.path.normpath(os.path.join(root, pdir, m.group(1)))
                if not ap.startswith(root.rstrip("/") + "/"):
                    continue
                rel = os.path.relpath(ap, root)
                (prefixes.add(rel + "/") if os.path.isdir(ap) else files.add(rel))
        if not found:
            prefixes.add(pdir + "/")
        out[q] = {"files": sorted(files), "prefixes": sorted(prefixes)}
    return out


def unit_cargo_packages(u, gate):
    """Packages a unit builds, tests or lints, and those whose bins it produces (cargo test builds a package's
    bins for its integration tests; a cargo build builds them)."""
    if u["kind"] == "whole" and "pkg" not in u:
        cmds = [parse_cargo(c.split(" ||")[0]) for c in gate.commands(u["stage"]) if c.startswith("cargo ")]
        return u["packages"], sorted({p for r in cmds if r["verb"] in ("build", "test") for p in r["packages"]})
    produces = u["packages"] if u["kind"] in ("test", "whole") else []
    return u["packages"], produces


def build_produces(b, owners, name, example):
    pkg = owners.get((example, name))
    if pkg not in b["packages"]:
        return False
    f = b["filters"]
    if not f:
        return not example
    return (("bins", None) in f and not example) or (("bin", name) in f and not example) \
        or (("example", name) in f and example)


WORKSPACE_BIN = re.compile(r"workspace_bin\s*\((.{0,400}?)\)", re.S)


def static_exec_pkgs(root, u):
    """The second exec source (§B): packages a test target names in a `workspace_bin(…, &["build", "-p", "<pkg>",
    …])` call, in its own file or a `mod <m>;` helper beside it (`tests/<m>.rs`, `tests/<m>/mod.rs`)."""
    if root is None or u.get("kind") != "test" or not u.get("name"):
        return set()
    tdir = os.path.join(root, "crates", u["pkg"], "tests")
    top = os.path.join(tdir, u["name"] + ".rs")
    todo, seen, out = [top], set(), set()
    while todo:
        f = todo.pop()
        if f in seen or not os.path.isfile(f):
            continue
        seen.add(f)
        text = open(f, errors="replace").read()
        for call in WORKSPACE_BIN.findall(text):
            out.update(re.findall(r'"-p",\s*"([A-Za-z0-9_-]+)"', call))
        if f == top:  # its `mod` helpers live beside it
            for m in re.findall(r"^\s*(?:pub\s+)?mod\s+([a-z_][a-z0-9_]*)\s*;", text, re.M):
                todo += [os.path.join(tdir, m + ".rs"), os.path.join(tdir, m, "mod.rs")]
    return out


def assemble(steps, obs, rcs, meta, gate, pkg_sets, workspace, root=None):
    """Audit units from raw observations: status, closure, runtime/exec sets, git flag and derived `needs`.
    `root` (the audited clone) enables the static workspace_bin scan. Returns (units, errors)."""
    deps, owners = path_deps(meta), bin_owners(meta)
    out, errors, prior_builds = {}, [], []
    for s in steps:
        if s["step"] == "build":
            prior_builds.append(s)
            continue
        o, rc = obs[s["id"]], rcs.get(s["id"], 0)
        uses, produces = unit_cargo_packages(s, gate)
        cl = set(workspace) if s.get("compile") == "workspace" else set(closure(uses, deps))
        if s.get("compile") == "workspace":
            produces = set(workspace)  # its scripts' own `cargo test -p <pkg>` build that package's bins (Q9)
        needs, execs, seen_pkgs = [], [], set()
        for where, name in sorted(o["execs"]):
            example = name.startswith("examples/")
            bname = name.split("/", 1)[1] if example else name
            pkg = owners.get((example, bname))
            if pkg is None:
                continue
            execs.append(f"{where}:{name}")
            seen_pkgs.add(pkg)
            cl |= set(closure([pkg], deps))
            if where != "main":
                continue  # a private target dir: the unit built it itself (workspace_bin and kin)
            b = next((b for b in reversed(prior_builds) if build_produces(b, owners, bname, example)), None)
            if b is not None:
                needs.append(b["id"])
            elif pkg not in produces:
                errors.append(f"unit {s['id']} executes {name} that no preceding build step and not the unit "
                              f"itself produces")
        static = static_exec_pkgs(root, s)
        for pkg in static:
            cl |= set(closure([pkg], deps))
        status, why = "ok", ""
        if o["overflow"]:
            status, why = "unaudited", "inotify queue overflow"
        elif rc == 77:
            status, why = "unaudited", "SKIP (exit 77)"
        elif rc != 0:
            status, why = "unaudited", f"exit {rc} in the audit"
        out[s["id"]] = {"status": status, "why": why, "closure": sorted(cl), "reads": sorted(o["reads"]),
                        "dirs": sorted(o["dirs"]), "execs": execs, "git_history": o["git"],
                        "needs": sorted(set(needs)), "static_unobserved": sorted(static - seen_pkgs)}
    return out, errors


class Watcher:
    """inotify IN_OPEN over the clone (recursive, minus .git and target), .git's top level and refs, and the
    target dirs' profile directories (executed or copied workspace binaries)."""

    def __init__(self, root, target, sentinel_dir, owners, sink):
        import ctypes
        self.libc = ctypes.CDLL("libc.so.6", use_errno=True)
        self.fd = self.libc.inotify_init1(IN_NONBLOCK)
        if self.fd < 0:
            raise SystemExit("gate_lane audit: inotify_init1 failed")
        self.root, self.target, self.sent = root, os.path.realpath(target), sentinel_dir
        self.owners = owners
        self.wd = {}  # wd -> (kind, dir)
        self.sink = sink  # every event, in queue order
        for d, dirs, _ in os.walk(root):
            if d == root:
                dirs[:] = [x for x in dirs if x not in (".git", "target")]
            self.watch(d, "repo", IN_OPEN | IN_CREATE)
        self.watch(os.path.join(root, ".git"), "git", IN_OPEN)
        for d, _, _ in os.walk(os.path.join(root, ".git", "refs")):
            self.watch(d, "git", IN_OPEN | IN_CREATE)
        self.watch(sentinel_dir, "sent", IN_OPEN)
        self.watch(self.target, "tgt", IN_CREATE)
        self.rescan()

    def watch(self, d, kind, mask):
        w = self.libc.inotify_add_watch(self.fd, d.encode(), mask)
        if w >= 0:
            self.wd[w] = (kind, d)

    def rescan(self):
        """Profile dirs of the main target and of every CACHEDIR.TAG dir up to two levels below it."""
        cands = [self.target]
        for e in os.scandir(self.target) if os.path.isdir(self.target) else []:
            if e.is_dir() and e.name not in ("debug", "release", "doc", "tmp") and not e.name.startswith("."):
                cands.append(e.path)
                for e2 in os.scandir(e.path):
                    if e2.is_dir() and e2.name not in ("debug", "release", "deps", "build", "incremental"):
                        cands.append(e2.path)
        known = {d for _, d in self.wd.values()}
        for c in cands:
            if c != self.target and not os.path.isfile(os.path.join(c, "CACHEDIR.TAG")):
                continue
            if c != self.target and c not in known:
                self.watch(c, "tgt", IN_CREATE)
            where = "main" if c == self.target else "private"
            for sub in ("debug", "debug/examples"):
                d = os.path.join(c, sub)
                if os.path.isdir(d) and d not in known:
                    self.watch(d, "exec-" + where, IN_OPEN | IN_CREATE)

    def drain(self, sink):
        import struct
        rescan = False
        while True:
            try:
                buf = os.read(self.fd, 1 << 20)
            except BlockingIOError:
                break
            i = 0
            while i < len(buf):
                w, mask, _, n = struct.unpack_from("iIII", buf, i)
                name = buf[i + 16:i + 16 + n].rstrip(b"\0").decode(errors="replace")
                i += 16 + n
                if mask & IN_Q_OVERFLOW:
                    sink({"t": "overflow"})
                    continue
                kind, d = self.wd.get(w, (None, None))
                full = os.path.join(d, name) if name else d
                if kind is None:
                    continue
                if mask & IN_CREATE:
                    if mask & IN_ISDIR:
                        if kind == "repo":
                            for dd, _, _ in os.walk(full):
                                self.watch(dd, "repo", IN_OPEN | IN_CREATE)
                        elif kind == "git":
                            self.watch(full, "git", IN_OPEN | IN_CREATE)
                        else:
                            rescan = True
                    continue
                if kind == "sent":
                    if name:
                        what, _, step = name.partition("@")
                        sink({"t": what, "step": step})
                elif kind == "repo":
                    rel = os.path.relpath(full, self.root)
                    if mask & IN_ISDIR:
                        if not name:
                            sink({"t": "dir", "path": "" if rel == "." else rel + "/"})
                    else:
                        sink({"t": "open", "path": rel})
                elif kind == "git":
                    top = d == os.path.join(self.root, ".git")
                    if name and not mask & IN_ISDIR and (not top or name in GIT_HISTORY):
                        sink({"t": "git", "path": os.path.relpath(full, self.root)})
                elif kind.startswith("exec-") and name and not mask & IN_ISDIR:
                    ex = d.endswith("/examples")
                    if (ex, name) in self.owners:
                        sink({"t": "exec", "where": kind[5:], "name": ("examples/" if ex else "") + name})
        if rescan:
            self.rescan()

    def mark(self, what, step):
        """Open a sentinel and drain until its own event arrives (FIFO: everything before it is in)."""
        import select
        import time
        p = os.path.join(self.sent, f"{what}@{step}")
        open(p, "w").close()
        deadline = time.time() + 60
        seen = []

        def take(e):
            if e["t"] == what and e.get("step") == step:
                seen.append(e)
            self.sink(e)
        while time.time() < deadline:
            self.drain(take)
            if seen:
                return
            select.select([self.fd], [], [], 0.2)
        self.sink({"t": "overflow"})  # the sentinel never came: nothing after it can be placed


def unit_command(step):
    """The one command the audit runs for a step: the gate itself, on that step only, never a receipt."""
    return ["./scripts/gate.sh", f"--lane-units={step['id']}"]


def cmd_audit(args):
    import select
    import time
    if args:
        print("gate_lane audit: takes no arguments; it audits HEAD", file=sys.stderr)
        return 2
    A = git(ROOT, "rev-parse", "HEAD").strip()
    tc_text, tc8 = toolchain()
    tmp = os.environ.get("TMPDIR", "/tmp")
    base = os.path.join(tmp, "axon-gate-lane-audit")
    clone = os.path.join(base, A)
    sent = os.path.join(base, A + ".sentinel")
    log = os.path.join(base, A + ".log")
    os.makedirs(base, exist_ok=True)
    subprocess.run(["rm", "-rf", clone, sent], check=True)
    git(ROOT, "clone", "-q", "--shared", "--no-checkout", ROOT, clone)
    git(clone, "checkout", "-q", "--detach", A)
    os.makedirs(sent)
    meta = cargo_metadata(clone)
    target = meta["target_directory"]  # cargo's answer, CARGO_TARGET_DIR and config included
    os.makedirs(os.path.join(target, "debug", "examples"), exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=target)
    if check(clone):
        print("gate_lane audit: the registry disagrees with gate.sh at A; run `gate_lane.py check`", file=sys.stderr)
        return 1
    steps = derive(clone, meta=meta)
    gate = read_gate(clone)
    att = Attributor([s["id"] for s in steps])
    w = Watcher(clone, target, sent, bin_owners(meta), att.feed)
    rcs, t0 = {}, time.time()
    load0 = os.getloadavg()[0]
    with open(log, "w") as lf:
        for n, s in enumerate(steps):
            w.rescan()
            w.mark("begin", s["id"])
            print(f"[{n + 1}/{len(steps)}] {s['id']}", flush=True)
            p = subprocess.Popen(unit_command(s), cwd=clone, env=env, stdout=lf, stderr=subprocess.STDOUT)
            while p.poll() is None:
                select.select([w.fd], [], [], 0.2)
                w.drain(att.feed)
            rc = p.returncode
            tsv = os.path.join(target, f"gate-stages.{p.pid}.tsv")  # gate.sh's per-run log: its $$ is p.pid
            if rc == 0 and s["step"] == "unit":
                if os.path.isfile(tsv) and any(l.rstrip("\n").split("\t")[-1] == "77" for l in open(tsv)):
                    rc = 77
            if os.path.isfile(tsv):
                os.remove(tsv)
            rcs[s["id"]] = rc
            w.mark("end", s["id"])
            if rc not in (0, 77):
                print(f"  {s['id']}: exit {rc} (unit recorded unaudited; see {log})", flush=True)
    wall = time.time() - t0
    obs = att.obs
    workspace = [p["name"] for p in meta["packages"]]
    pkg_sets = package_compile_sets(meta, clone, target)
    units_, errors = assemble(steps, obs, rcs, meta, gate, pkg_sets, workspace, root=clone)
    for e in errors:
        print(f"gate_lane audit: {e}", file=sys.stderr)
    if errors:
        return 1
    audit = {
        "schema": "axon-gate-lane-audit/1", "commit": A, "toolchain": tc_text.splitlines()[0],
        "toolchain_sha8": tc8, "uid": uid_class(), "wall_s": round(wall, 1), "load_at_start": load0,
        "load_at_end": os.getloadavg()[0], "target_dir": target,
        "universe": git(clone, "ls-tree", "-r", "--name-only", A).splitlines(),
        "packages": pkg_sets,
        "builds": {s["id"]: rcs[s["id"]] for s in steps if s["step"] == "build"},
        "units": units_,
    }
    for b, rc in audit["builds"].items():
        if rc != 0:
            print(f"gate_lane audit: build step {b} failed (exit {rc}); see {log}", file=sys.stderr)
            return 1
    bad = audit_problems(audit, units(steps))
    for b in bad:
        print(f"gate_lane audit: {b}", file=sys.stderr)
    out = os.path.join(store_dir(ROOT), f"{A}-{tc8}-{uid_class()}.json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out + ".tmp", "w") as f:
        json.dump(audit, f)
    os.replace(out + ".tmp", out)
    n_un = sum(1 for u in units_.values() if u["status"] != "ok")
    print(f"gate_lane audit: {len(units_)} units ({n_un} unaudited), wall {wall:.0f} s, load {load0:.1f} → "
          f"{audit['load_at_end']:.1f}; written {out}")
    return 1 if bad else 0


# ── GL-4: selection ────────────────────────────────────────────────────────

GLOBAL_INPUTS = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")
GATE_DEFINITION = ("scripts/gate.sh", "scripts/gate_lane.py", REGISTRY)


def unit_paths(audit, a):
    """(compile files, compile prefixes) of an audited unit: the union over its closure."""
    files, prefixes = set(), set()
    for q in a["closure"]:
        ps = audit["packages"].get(q, {"files": [], "prefixes": []})
        files.update(ps["files"])
        prefixes.update(ps["prefixes"])
    return files, prefixes


def changes(root, A):
    """C versus A: (status, path), status M/A/D, or T for a mode or type change."""
    out = {}
    for line in git(root, "diff", "--name-status", "--no-renames", A).splitlines():
        st, p = line.split("\t", 1)
        out[p] = st[0]
    for line in git(root, "diff", "--summary", "--no-renames", A).splitlines():
        m = re.match(r"^ mode change \d+ => \d+ (.+)$", line)
        if m:
            out[m.group(1)] = "T"
    for p in git(root, "ls-files", "--others", "--exclude-standard").splitlines():
        out[p] = "A"
    return sorted((st, p) for p, st in out.items())


def decide(audit, us, chg, read_text=None):
    """The rules G0–G7 over C. Returns ({unit id: reason or None}, [unread paths])."""
    sel = {u["id"]: None for u in us}

    def pick(uid, why):
        if sel.get(uid, "x") is None:
            sel[uid] = why

    if audit is None:
        for u in us:
            pick(u["id"], "no read-set audit for an ancestor of HEAD")
        return sel, []
    au = audit["units"]
    for u in us:
        a = au.get(u["id"])
        if a is None:
            pick(u["id"], "a target unseen by the audit")
        elif a["status"] != "ok":
            pick(u["id"], f"unaudited ({a['why']})")
        elif a["git_history"]:
            pick(u["id"], "git-history (reads HEAD, index or refs)")
    paths = {u["id"]: unit_paths(audit, au[u["id"]]) for u in us if u["id"] in au}
    universe = set(audit["universe"])
    udirs = {os.path.dirname(p) for p in universe}
    for p in list(udirs):
        while p:
            p = os.path.dirname(p)
            udirs.add(p)
    unread = []
    added_literal = []
    for st, p in chg:
        if p in GLOBAL_INPUTS or p.startswith(".cargo/"):
            for u in us:
                if u["id"] not in au or au[u["id"]]["closure"]:
                    pick(u["id"], "global build input")
        elif p in GATE_DEFINITION or p.startswith("scripts/lib/"):
            for u in us:
                pick(u["id"], "gate definition changed")
        elif st in ("D", "T"):
            for u in us:
                pick(u["id"], f"{'deleted' if st == 'D' else 'mode'} path {p}")
        elif st == "M" and p in universe:
            hit = False
            for u in us:
                if u["id"] not in paths:
                    continue
                files, prefixes = paths[u["id"]]
                a = au[u["id"]]
                if p in files or any(p.startswith(x) for x in prefixes):
                    pick(u["id"], f"compiles {p}"); hit = True
                elif p in a["reads"]:
                    pick(u["id"], f"reads {p}"); hit = True
            if not hit:
                unread.append(p)
        elif st == "A":
            d = os.path.dirname(p)
            while d and d not in udirs:
                d = os.path.dirname(d)
            hit = False
            for u in us:
                if u["id"] not in paths:
                    continue
                a, (files, prefixes) = au[u["id"]], paths[u["id"]]
                owners = [x for x in prefixes if p.startswith(x)] + \
                    [q for q in a["closure"] if any(f.startswith(f"crates/{q}/") and p.startswith(f"crates/{q}/")
                                                    for f in files)]
                if (d + "/" if d else "") in a["dirs"]:
                    pick(u["id"], f"added under enumerated {d or '.'}/"); hit = True
                elif owners:
                    pick(u["id"], f"added in package {owners[0].rstrip('/')}"); hit = True
            added_literal.append(p)
            if hit:
                continue
            if read_text is not None and _named_by(audit, us, paths, p, read_text, pick):
                continue
            for u in us:
                pick(u["id"], f"unclassified path {p}")
        else:
            for u in us:
                pick(u["id"], f"unclassified path {p}")
    if read_text is not None:
        for p in added_literal:
            _named_by(audit, us, paths, p, read_text, pick)
    return sel, unread


def _named_by(audit, us, paths, p, read_text, pick):
    """G5c: units whose compile ∪ runtime set holds a file naming p (repo path or basename) literally."""
    hit, base, cache = False, os.path.basename(p), {}
    for u in us:
        if u["id"] not in paths:
            continue
        files = paths[u["id"]][0] | set(audit["units"][u["id"]]["reads"])
        for f in sorted(files):
            if f not in cache:
                cache[f] = read_text(f) or ""
            if p in cache[f] or base in cache[f]:
                pick(u["id"], f"named by {f}"); hit = True
                break
    return hit


def needed_builds(steps, sel, audit, reg):
    """Build steps the selected units need, in gate order: their audited `needs`, a whole stage's own builds
    when it runs, and every earlier build for a selected unit the audit cannot speak for."""
    whole = {s["id"] for s in reg["stages"] if s.get("split") == "none"}
    need, prior = set(), []
    for s in steps:
        if s["step"] == "build":
            prior.append(s["id"])
            if s["stage"] in whole and sel.get(s["stage"]):
                need.add(s["id"])
            continue
        if not sel.get(s["id"]):
            continue
        a = audit["units"].get(s["id"]) if audit else None
        if a is None or a["status"] != "ok":
            need.update(prior)
        else:
            need.update(a["needs"])
    return need


def make_selection(root, units_arg=None, audit=None, chg=None, meta=None):
    """The lane's selection, written for gate.sh: units, builds and stages to run, and the evidence lines."""
    reg = load(root, REGISTRY)
    steps = derive(root, reg=reg, meta=meta)
    us = units(steps)
    lines = []
    if units_arg is not None:
        want = set(units_arg)
        known = {s["id"] for s in steps}
        unknown = sorted(want - known)
        if unknown:
            raise SystemExit(f"gate_lane: --lane-units names unknown steps: {' '.join(unknown)}")
        sel = {u["id"]: ("listed" if u["id"] in want else None) for u in us}
        whole = {s["id"] for s in reg["stages"] if s.get("split") == "none"}
        builds = {s["id"] for s in steps if s["step"] == "build"
                  and (s["id"] in want or (s["stage"] in whole and s["stage"] in want))}
        always = False
    else:
        sel, unread = decide(audit, us, chg, read_text=lambda f: _read(root, f))
        if "S17" in sel:  # S17 restores the binary S16's build replaced: it runs iff S16 runs
            sel["S17"] = "runs iff S16 runs" if any(sel[u["id"]] for u in us if u["stage"] == "S16") else None
        builds = needed_builds(steps, sel, audit, reg)
        always = True
        lines.append(f"════ LANE — NOT THE GATE, NOT EVIDENCE · audit={audit['commit'][:12] if audit else 'none'} ════")
        lines.append(f"C ({len(chg)} path(s) versus {audit['commit'][:12] if audit else 'no audit'}):")
        lines += [f"  {st} {p}" for st, p in chg] or ["  (none)"]
        for s in reg["stages"]:
            if s["class"] == "always":
                lines.append(f"RUN {s['id']} ← always")
        lines.append("RUN par group ← always")
        for s in steps:
            if s["step"] == "build":
                lines.append(f"{'RUN' if s['id'] in builds else 'SKIP'} {s['id']} "
                             f"{'← needed by a selected unit' if s['id'] in builds else '— no selected unit needs it'}")
            elif sel[s["id"]]:
                lines.append(f"RUN {s['id']} ← {sel[s['id']]}")
            else:
                lines.append(f"SKIP {s['id']} — no path of C is in its read set")
        if unread:
            lines.append("unread by any unit: " + " ".join(unread))
        lines.append("NOT RUN (strict-only): --all-targets clippy, codegen clippy, codegen-gated tests, "
                     "builtin-externs drift, parity suite, WASI refusal, all-examples parity, acceptance gates, "
                     "unwired harnesses, domain round-trips, smt")
        if any(p.startswith("crates/axon-core/src/codegen") for _, p in chg):
            lines.append("hint: parity is --strict only")
    stages = {s["stage"] for s in us if sel[s["id"]]}
    if always:
        stages |= {s["id"] for s in reg["stages"] if s["class"] == "always"} | {"P"}
    n_run = sum(1 for u in us if sel[u["id"]])
    return {"units": {u["id"]: bool(sel[u["id"]]) for u in us}, "builds": sorted(builds),
            "stages": sorted(stages), "lines": lines, "n_run": n_run, "n_skip": len(us) - n_run,
            "audit": audit["commit"] if audit else None}


def _read(root, f):
    try:
        with open(os.path.join(root, f), errors="replace") as fh:
            return fh.read(1 << 22)
    except OSError:
        return None


def expand(selection, sid, cmd, pkgs):
    """The command lines gate.sh runs for `lane_cargo <sid> <cmd>`: ["="] for the original line, [] to skip,
    else per-package lines naming only the selected targets."""
    cmd = norm(cmd)
    if re.match(r"^B\d+$", sid):
        return ["="] if sid in selection["builds"] else []
    if sid in selection["units"]:  # a one-unit line (S09:lint)
        return ["="] if selection["units"][sid] else []
    us = split_units(sid, cmd, pkgs)
    picked = [u for u in us if selection["units"].get(u["id"])]
    if len(picked) == len(us):
        return ["="]
    if not picked:
        return []
    base, t, i = [], cmd.split(), 0
    while i < len(t):
        if t[i] in ("-p", "--bin", "--test", "--example", "--bench"):
            i += 2
            continue
        if t[i] in ("--lib", "--doc", "--bins", "--tests", "--examples"):
            i += 1
            continue
        base.append(t[i])
        i += 1
    out = []
    for p in dict.fromkeys(u["pkg"] for u in us):
        mine = [u for u in us if u["pkg"] == p]
        got = [u for u in picked if u["pkg"] == p]
        if not got:
            continue
        if len(got) == len(mine) and not parse_cargo(cmd)["filters"]:
            out.append(" ".join(base + ["-p", p]))
            continue
        flags = [f for u in got if u["kind"] != "doc" for f in u["flags"]]
        if flags:
            out.append(" ".join(base + ["-p", p] + flags))
        if any(u["kind"] == "doc" for u in got):
            out.append(" ".join(base + ["-p", p, "--doc"]))
    return out


def main(argv):
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__)
        return 0 if argv else 2
    cmd, rest = argv[0], argv[1:]
    if cmd == "check":
        bad = check(ROOT)
        audit = find_audit(ROOT) if not bad and "--audit" in rest else None
        if audit:
            bad += audit_problems(audit, units(derive(ROOT)))
        for b in bad:
            print(f"gate_lane check: {b}")
        if bad:
            return 1
        if "--quiet" not in rest:
            print(f"gate_lane check: OK ({len(units(derive(ROOT)))} units"
                  f"{', audit ' + audit['commit'][:12] if audit else ''})")
        return 0
    if cmd == "units":
        for s in derive(ROOT):
            print(("  build " if s["step"] == "build" else "") + s["id"])
        return 0
    if cmd == "audit":
        return cmd_audit(rest)
    if cmd == "select":
        # select --out FILE [--units=A,B]
        out = rest[rest.index("--out") + 1]
        ua = next((a.split("=", 1)[1] for a in rest if a.startswith("--units=")), None)
        meta = cargo_metadata(ROOT)
        if ua is not None:
            s = make_selection(ROOT, units_arg=[x for x in ua.split(",") if x], meta=meta)
        else:
            audit = find_audit(ROOT)
            s = make_selection(ROOT, audit=audit, chg=changes(ROOT, audit["commit"] if audit else "HEAD"), meta=meta)
        s["pkgs"] = packages_by_name(meta)
        reg = load(ROOT, REGISTRY)
        with open(out, "w") as f:
            json.dump(s, f)
        with open(out + ".stages", "w") as f:
            f.write("".join(x + "\n" for x in s["stages"]))
        with open(out + ".builds", "w") as f:  # stage<TAB>id<TAB>the gate's own command line
            f.write("".join(f"{b['stage']}\t{b['id']}\t{b['cmd']}\n" for b in reg["builds"] if b["id"] in s["builds"]))
        for line in s["lines"]:
            print(line)
        return 0
    if cmd == "expand":
        # expand SELECTION ID CMD...: `RUN =` (the line as written), `RUN <narrowed line>`…, or `SKIP …`
        selection = load("/", os.path.abspath(rest[0]))
        lines = expand(selection, rest[1], " ".join(rest[2:]), selection["pkgs"])
        if lines == ["="]:
            print("RUN =")
        elif not lines:
            print(f"SKIP {rest[1]}: no selected target in: {' '.join(rest[2:])}")
        else:
            for line in lines:
                print("RUN " + line)
        return 0
    if cmd == "footer":
        s = load("/", os.path.abspath(rest[0]))
        print(f"lane: {s['n_run']} run / {s['n_skip']} skipped passed — not the gate; "
              f"ship rule: full ./scripts/gate.sh on the integrated head")
        return 0
    print(f"gate_lane: unknown command {cmd}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
