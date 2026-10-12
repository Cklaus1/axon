#!/usr/bin/env python3
"""The paired-disable consumer selection (amendment 67), on synthetic
workspaces: no cargo, no tree. Every case is an ATTACK (a consumer that CAN
observe the retired guard's file must be selected -- by link, build script,
data or exec, including each inclusion case found in the real tree) or a
CONTROL (a consumer that cannot must be skipped, with the graph reason).

    python3 scripts/test_v022_paired_disable_selection.py
"""
import os
import random
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
import v022_pd_consumers as pdc  # noqa: E402

WS = "/ws"


def pkg(name, deps=(), bins=(), tests=(), build=False):
    """A cargo-metadata package entry: deps as (name, kind)."""
    d = f"{WS}/crates/{name}"
    targets = [{"kind": ["lib"], "name": name.replace("-", "_"), "src_path": f"{d}/src/lib.rs"}]
    targets += [{"kind": ["bin"], "name": b, "src_path": f"{d}/src/bin/{b}.rs"} for b in bins]
    targets += [{"kind": ["test"], "name": t, "src_path": f"{d}/tests/{t}.rs"} for t in tests]
    if build:
        targets.append({"kind": ["custom-build"], "name": "build-script-build", "src_path": f"{d}/build.rs"})
    return {"name": name, "manifest_path": f"{d}/Cargo.toml", "targets": targets,
            "dependencies": [{"name": n, "kind": None if k == "normal" else k} for n, k in deps]}


def selector(pkgs, files):
    meta = {"workspace_root": WS, "packages": pkgs}
    base = {}
    for p in pkgs:
        for t in p["targets"]:
            base.setdefault(os.path.relpath(t["src_path"], WS), "\n")
    base.update(files)
    return pdc.selector(WS, meta=meta, files=base)


FAILS = []


def expect_run(name, sel, c, rule=None, targets=None):
    v = sel["run"].get(c)
    if v is None:
        FAILS.append(f"ATTACK: {name}: consumer {c} can observe {sel['mutated_file']} but was skipped "
                     f"({sel['skipped'].get(c)})")
        return
    why = v["reason"] if isinstance(v["reason"], str) else " ".join(v["reason"].values())
    if rule and not why.startswith(rule) and f" {rule}" not in why and f"-- {rule}" not in why:
        FAILS.append(f"{name}: {c} selected for {why!r}, expected rule {rule}")
    if targets is not None and v["scope"] != targets:
        FAILS.append(f"{name}: {c} ran {v['scope']}, expected {targets}")


def expect_skip(name, sel, c):
    if c in sel["run"]:
        FAILS.append(f"CONTROL: {name}: {c} cannot observe {sel['mutated_file']} but was run "
                     f"({sel['run'][c]['reason']})")
    elif not sel["skipped"].get(c):
        FAILS.append(f"CONTROL: {name}: {c} skipped without a graph reason")


F = "crates/x/src/guard.rs"


def base_files(extra=None):
    f = {F: "pub fn guard() {}\n"}
    f.update(extra or {})
    return f


def main():
    # L: a normal dependency, transitively; a dev-dependency; a build-dependency.
    s = selector([pkg("x"), pkg("y", [("x", "normal")]), pkg("c", [("y", "normal")]),
                  pkg("d", [("x", "dev")]), pkg("e", [("x", "build")]), pkg("u")],
                 base_files()).select(F, {"x"})
    expect_run("L transitive", s, "y", "L", "suite")
    expect_run("L through y", s, "c", "L", "suite")
    expect_run("L dev-dependency (tests link it)", s, "d", "L", "suite")
    expect_run("L build-dependency", s, "e", "L", "suite")
    expect_skip("unrelated package", s, "u")
    if "not in its link closure" not in s["skipped"].get("u", ""):
        FAILS.append("CONTROL: a skipped consumer's reason does not name the graph")

    # E: a test that builds and execs X's binary (workspace_bin, cargo -p),
    # through CARGO_BIN_EXE of its own package that execs X, through a chain
    # (c execs y's binary, whose code execs X's by its target-dir path), and a
    # <NAME>_BIN variable.
    pk = [pkg("x", bins=["xbin"]), pkg("y", bins=["ybin"]),
          pkg("c", tests=["t", "quiet"]), pkg("v", tests=["t"]), pkg("w", tests=["t"]),
          pkg("z", tests=["t"])]
    files = base_files({
        "crates/c/tests/t.rs": 'fn t() { let b = workspace_bin("X_BIN", &["build", "-p", "x"], "xbin"); }\n',
        "crates/c/tests/quiet.rs": "fn q() {}\n",
        # (the path is assembled so the binary-pick gate does not read this
        # synthetic fixture as a script choosing a binary)
        "crates/y/src/bin/ybin.rs": 'fn main() { let p = "' + "targ" + 'et/debug/xbin"; }\n',
        "crates/v/tests/t.rs": 'fn t() { Command::new(env!("CARGO_BIN_EXE_ybin")); let d = "debug/ybin"; }\n',
        "crates/w/tests/t.rs": 'fn t() { let b = std::env::var("XBIN_BIN"); }\n',
        # a bare name with nothing locating a tree-built program: a label
        "crates/z/tests/t.rs": 'pub const ID: &str = "xbin";\nfn t() { map.get(ID); }\n',
    })
    s = selector(pk, files).select(F, {"x"})
    expect_run("E workspace_bin -p x", s, "c", "E", ["t"])
    expect_run("E chain through ybin", s, "v", "E", ["t"])
    expect_run("E <NAME>_BIN variable", s, "w", "E", ["t"])
    expect_skip("a bare binary name used as a label", s, "z")
    # ...and the same bare name in a function that locates programs counts.
    files["crates/z/tests/t.rs"] = 'fn t() { let p = target_dir().join("debug").join("xbin"); }\n'
    s = selector(pk, files).select(F, {"x"})
    expect_run("E bare name in a locator", s, "z", "E", ["t"])

    # D: a test reading another crate's source tree as data (the refusal
    # gate), include_str!/#[path] of a file in another crate, a script the
    # test runs that builds X or names a path in it, a crate path in an
    # absolute literal.
    pk = [pkg("x"), pkg("g", tests=["gate", "other"]), pkg("h", tests=["t"]), pkg("i", tests=["t"]),
          pkg("j", tests=["t"]), pkg("k", tests=["t"]), pkg("m", tests=["t"])]
    files = base_files({
        "crates/g/tests/gate.rs": 'const SCANNED: &[&str] = &["crates/x/src", "crates/y/src"];\n',
        "crates/g/tests/other.rs": "fn t() {}\n",
        "crates/h/tests/t.rs": 'const S: &str = include_str!("../../x/data.json");\n',
        "crates/x/data.json": "{}\n",
        "crates/i/tests/t.rs": '#[path = "../../x/tests/common/exec.rs"]\nmod exec;\n',
        "crates/x/tests/common/exec.rs": "pub fn e() {}\n",
        "crates/j/tests/t.rs": 'fn t() { script_spawn::script("bash", root.join("scripts/build-x.sh")); }\n',
        "scripts/build-x.sh": "#!/bin/bash\ncargo build -p x --bins\n",
        "crates/k/tests/t.rs": 'fn t() { run("scripts/scan.py"); }\n',
        "scripts/scan.py": "#!/usr/bin/env python3\nopen('crates/x/src/guard.rs').read()\n",
        "crates/m/tests/t.rs": 'fn t() { let p = "/home/user/project/crates/x/src/guard.rs"; }\n',
    })
    # module closure: a `mod common;` and a `#[path]` module outside crates/
    # whose text names X are part of the test that declares them.
    pk += [pkg("n", tests=["t"]), pkg("o", tests=["t"]), pkg("fmt", tests=["t", "u"])]
    files.update({
        "crates/n/tests/t.rs": "mod common;\nfn t() { common::go(); }\n",
        "crates/n/tests/common/mod.rs": 'pub fn go() { let p = "crates/x/src"; }\n',
        "crates/o/tests/t.rs": '#[path = "../../../testlib/helper.rs"]\nmod helper;\n',
        "testlib/helper.rs": 'pub fn h() { let p = root.join("crates/x/src/guard.rs"); }\n',
        # format templates name no path: `{}` / `{e}` / `{a}.{b}` / `{m}e{s}`
        "crates/fmt/tests/t.rs": 'fn t() { format!("{}", a); format!("{e}"); format!("{a}.{b}"); format!("{m}e{s}"); format!("{a}/{b}"); }\n',
        # an absolute path outside the tree, however templated, names nothing in it
        "crates/fmt/tests/u.rs": 'fn u() { let p = format!("/proc/{pid}/x"); }\n',
    })
    s = selector(pk, files).select(F, {"x"})
    expect_run("D a test reads another crate's tree", s, "g", "D", ["gate"])
    expect_run("D a `mod common;` naming X", s, "n", "D", ["t"])
    expect_run("D a #[path] module outside crates/ naming X", s, "o", "D", ["t"])
    expect_skip("format templates are not paths", s, "fmt")
    expect_run("D include_str! of a file in X", s, "h", "D", ["t"])
    expect_run("D #[path] module from X", s, "i", "D", ["t"])
    expect_run("E a script that cargo-builds X", s, "j", "E", ["t"])
    expect_run("D a script naming a path in X", s, "k", "D", ["t"])
    expect_run("D an absolute path into X", s, "m", "D", ["t"])

    # D, walker: a function that finds the workspace root and lists it can
    # read every file; a listing of a temp dir does not.
    pk = [pkg("x"), pkg("r", tests=["walk", "tmp"]), pkg("rh", tests=["t"])]
    files = base_files({
        "crates/rh/tests/t.rs": "fn all() {\n    for e in std::fs::read_dir(repo_root()).unwrap() {}\n}\n",
        "crates/r/tests/walk.rs": ("fn every_source() {\n    let root = repo_root();\n"
                                   "    for e in std::fs::read_dir(root.join(\"crates\")).unwrap() {}\n}\n"),
        "crates/r/tests/tmp.rs": "fn t() {\n    for e in std::fs::read_dir(tmp).unwrap() {}\n}\n",
    })
    s = selector(pk, files).select(F, {"x"})
    expect_run("D a test enumerating the workspace", s, "r", "D", ["walk"])
    expect_run("D a test enumerating what a root helper returns", s, "rh", "D", ["t"])

    # Base derivation (amendment 67): a listing whose base is computed is
    # resolved by how the base is derived. A freshness walk that starts at
    # crates/<parameter> and grows only by the Cargo.toml `path =` entries it
    # reads lists the manifest closure of the package the unit passes in (a
    # literal cargo `-p`). Here the unit passes `a`, which does not reach x.
    walker = (
        "pub fn stale(bin: &Path, pkg: &str) -> bool {\n"
        "    let root = repo_root();\n"
        "    let mut todo = vec![root.join(\"crates\").join(pkg)];\n"
        "    let lock = root.join(\"Cargo.lock\");\n"
        "    while let Some(dir) = todo.pop() {\n"
        "        if let Ok(m) = std::fs::read_to_string(dir.join(\"Cargo.toml\")) {\n"
        "            for line in m.lines() { if let Some(i) = line.find(\"path = \\\"\") { todo.push(dir.join(&line[i..])); } }\n"
        "        }\n"
        "        if let Some(fs) = tree_files(&root, &dir) { continue; }\n"
        "        for e in std::fs::read_dir(&dir).unwrap() {}\n"
        "    }\n"
        "    false\n}\n"
        "fn tree_files(root: &Path, dir: &Path) -> Option<Vec<PathBuf>> {\n"
        "    let o = Command::new(\"git\").arg(\"-C\").arg(root).args([\"ls-files\", \"--\"]).arg(dir).output().ok()?;\n"
        "    Some(o.stdout.split(|b| *b == 0).map(|f| root.join(String::from_utf8_lossy(f).as_ref())).collect())\n}\n")
    def bd(test_text, helper=walker):
        pk = [pkg("x"), pkg("a"), pkg("q", tests=["t"])]
        return selector(pk, base_files({"crates/q/tests/common/mod.rs": helper,
                                        "crates/q/tests/t.rs": "mod common;\n" + test_text})).select(F, {"x"})
    use_a = 'fn t() { common::stale(&p, "a"); let b = ["build", "-p", "a"]; }\n'
    expect_skip("a freshness walk of the manifest closure of a package that does not reach x",
                bd(use_a), "q")
    pk_dep = [pkg("x"), pkg("a", [("x", "dev")]), pkg("q", tests=["t"])]
    s = selector(pk_dep, base_files({"crates/q/tests/common/mod.rs": walker,
                                     "crates/q/tests/t.rs": "mod common;\n" + use_a})).select(F, {"x"})
    expect_run("D a freshness walk of a closure that reaches x (through a dev dependency)", s, "q", "D", ["t"])
    # CONTROLS: the base is genuinely arbitrary -> whole tree.
    expect_run("D a package directory passed as a run-time `-p` value",
               bd('fn t() { common::stale(&p, name); let b = ["build", "-p", name];\n'
                  '    let c = ["build", "-p", "a"]; }\n'), "q", "D", ["t"])
    expect_run("D a walk that also names the root by a relative literal", bd(use_a, walker.replace(
        "    let lock = root.join(\"Cargo.lock\");\n",
        "    let up = Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../..\");\n")),
        "q", "D", ["t"])
    expect_run("D no cargo `-p` names the package handed to the walk",
               bd('fn t() { common::stale(&p, &name); }\n'), "q", "D", ["t"])
    expect_run("D a package directory chosen by listing crates/", bd(use_a, walker.replace(
        'vec![root.join(\"crates\").join(pkg)]',
        'std::fs::read_dir(root.join(\"crates\")).unwrap().map(|e| e.unwrap().path()).collect::<Vec<_>>()')),
        "q", "D", ["t"])
    expect_run("D the root itself listed", bd(use_a, walker.replace(
        "    let lock = root.join(\"Cargo.lock\");\n", "    for e in std::fs::read_dir(&root).unwrap() {}\n")),
        "q", "D", ["t"])
    expect_run("D the root handed to a function that lists it", bd(use_a, walker.replace(
        ".arg(\"-C\").arg(root)", ".arg(\"-C\").arg(\"x\")").replace(
        "    let o = Command::new", "    for e in std::fs::read_dir(root).unwrap() {}\n    let o = Command::new")),
        "q", "D", ["t"])
    expect_run("D a walk that does not follow manifests only", bd(use_a, walker.replace(
        "dir.join(\"Cargo.toml\")", "dir.join(\"NOTES\")")), "q", "D", ["t"])

    # A list of variable NAMES (to strip from a child's environment) is not
    # a binary the unit locates; the same list read from the environment is.
    pk = [pkg("x", bins=["xbin"]), pkg("nv", tests=["t"]), pkg("rv", tests=["t"])]
    lst = 'pub const VARS: &[&str] = &[\n    "XBIN_BIN",\n];\n'
    files = base_files({
        "crates/nv/tests/t.rs": lst + "fn t(c: &mut Command) { for v in VARS { c.env_remove(v); } }\n",
        "crates/rv/tests/t.rs": lst + "fn t() { for v in VARS { let p = std::env::var(v); } }\n",
    })
    s = selector(pk, files).select(F, {"x"})
    expect_skip("variable names listed to be stripped", s, "nv")
    expect_run("E the same names read from the environment", s, "rv", "E", ["t"])

    # B: a build script that watches a computed path (the whole tree) makes
    # its package and everything linking it relevant; a literal re-run path
    # covers only that path; no re-run set covers its own package only.
    pk = [pkg("x"), pkg("wb", build=True), pkg("lw", [("wb", "normal")]),
          pkg("sb", build=True), pkg("nb", build=True)]
    files = base_files({
        "crates/wb/build.rs": 'fn main() { for w in watch { println!("cargo:rerun-if-changed={}", w); } }\n',
        "crates/sb/build.rs": 'fn main() { println!("cargo:rerun-if-changed=src"); }\n',
        "crates/nb/build.rs": "fn main() {}\n",
    })
    s = selector(pk, files).select(F, {"x"})
    expect_run("B whole-tree build script", s, "wb", "B", "suite")
    expect_run("B linking a whole-tree build script", s, "lw", "B", "suite")
    expect_skip("a build script re-run on its own src", s, "sb")
    expect_skip("a build script with no re-run set", s, "nb")

    # A guard file outside any package (scripts/): only units naming it.
    pk = [pkg("p", tests=["t", "u"]), pkg("q")]
    files = {"scripts/guard.py": "x = 1\n",
             "crates/p/tests/t.rs": 'fn t() { run("scripts/guard.py"); }\n',
             "crates/p/tests/u.rs": "fn u() {}\n"}
    s = selector(pk, files).select("scripts/guard.py", {"axon-core"})
    expect_run("D a test running the guard's script", s, "p", "D", ["t"])
    expect_skip("a package naming nothing of scripts/guard.py", s, "q")

    # The owner and the row package are never consumers (they always run).
    s = selector([pkg("x"), pkg("y", [("x", "normal")])], base_files()).select(F, {"x", "y"})
    if "y" in s["run"] or "y" in s["skipped"]:
        FAILS.append("the row package was judged as a consumer; it always runs its full suite")

    # PROPERTY: on random dependency graphs, every package whose test link
    # closure holds X is selected (whole suite), and every selected package
    # that is not is selected for a rule other than L.
    rng = random.Random(67)
    for trial in range(200):
        n = rng.randint(2, 9)
        names = [f"p{i}" for i in range(n)]
        pk = []
        for i, nm in enumerate(names):
            deps = [(names[j], rng.choice(["normal", "dev", "build"]))
                    for j in range(n) if j != i and rng.random() < 0.25]
            pk.append(pkg(nm, deps))
        x = rng.choice(names)
        f = f"crates/{x}/src/guard.rs"
        sel = selector(pk, {f: "\n"})
        s = sel.select(f, {x})
        for c in names:
            if c == x:
                continue
            if x in sel.ws.test_link_closure(c):
                if c not in s["run"] or s["run"][c]["scope"] != "suite":
                    FAILS.append(f"ATTACK: trial {trial}: {c} reaches {x} through the graph but was "
                                 f"not run ({s['skipped'].get(c)})")
            elif c in s["run"]:
                FAILS.append(f"CONTROL: trial {trial}: {c} cannot reach {x} but was run: {s['run'][c]}")

    # The record field: a selection a reviewer cannot audit is refused.
    good = selector([pkg("x"), pkg("y", [("x", "normal")]), pkg("u")], base_files()).select(F, {"x"})
    if pdc.well_formed(good) is not None:
        FAILS.append(f"control: a real selection is not well formed: {pdc.well_formed(good)}")
    for name, bad in [
        ("no selection", None),
        ("another rule", {**good, "rule": "list/0"}),
        ("no skipped map", {k: v for k, v in good.items() if k != "skipped"}),
        ("a skipped consumer with no reason", {**good, "skipped": {"u": ""}}),
        ("a consumer both run and skipped", {**good, "skipped": {"y": "because"}}),
        ("a run consumer with no reason", {**good, "run": {"y": {"scope": "suite"}}}),
    ]:
        if pdc.well_formed(bad) is None:
            FAILS.append(f"ATTACK: a selection with {name} was accepted as auditable")

    if FAILS:
        print("\n".join(FAILS))
        sys.exit(1)
    print("consumer selection: every reachable consumer selected, every unreachable one skipped with a reason")


if __name__ == "__main__":
    main()
