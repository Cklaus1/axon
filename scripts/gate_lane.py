#!/usr/bin/env python3
"""The gate's lane machinery (tasks/gate-lane-spec.md GL-2 … GL-4). NOT the gate.

    gate_lane.py check            registry governance/gate-lanes.json against gate.sh, the manifest and cargo
    gate_lane.py units            the derived units, in full-gate order

The registry holds only what cannot be derived. Units come from `cargo metadata`: a `per-target` stage gets one
unit per target its cargo test command runs (`--lib`, each `--bin`, `--test`, `--example`, and `--doc`); any other
stage is one unit. Lane logic lives here, not in gate.sh: the clippy-coverage check greps gate.sh's own text for
`-p <crate>`, so crate names written into gate.sh for the lane would fake coverage.
"""
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


def main(argv):
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__)
        return 0 if argv else 2
    cmd = argv[0]
    if cmd == "check":
        bad = check(ROOT)
        for b in bad:
            print(f"gate_lane check: {b}")
        if bad:
            return 1
        print(f"gate_lane check: OK ({len(units(derive(ROOT)))} units)")
        return 0
    if cmd == "units":
        for s in derive(ROOT):
            print(("  build " if s["step"] == "build" else "") + s["id"])
        return 0
    print(f"gate_lane: unknown command {cmd}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
