#!/usr/bin/env python3
"""Consumer selection for the paired-disable full-suite cell (amendment 67).

The full-suite cell of a retirement record removes the retired guard ALONE
(edit A: bytes of one file F, in package X when F is under a package
directory) and runs the owner's and the row package's FULL suites -- always --
and then the suites of the other packages ("consumers") whose outcome A could
change. This module decides which consumers those are, from `cargo metadata`
and the tree's own text, never from a list.

UNITS. A consumer package C is judged as units: its crate-wide unit (lib and
bin unit tests, doc tests, the examples `cargo test` builds: src/**,
examples/**, build.rs) and each `tests/*.rs` target (its file and every file
it pulls in by `mod`, `#[path]` or `include!`, plus the crate-wide files). A
relevant crate-wide unit runs C's whole suite; otherwise exactly the relevant
targets run (`--test T ...`); a package with no relevant unit is SKIPPED, and
the record says why.

THE RULE. A unit is RELEVANT to F when any of these holds.

  L  LINK. X is in its link closure: C's normal and build dependencies,
     transitively, plus C's dev-dependencies (tests link them) and their
     closures.
  B  BUILD SCRIPT. A package in that closure has a build script whose re-run
     set covers F: a literal `rerun-if-changed` path above or at F, any
     computed one (`{}`: the whole tree -- axon-fabric watches every top-level
     entry), or, when it declares none, its own package (cargo's default).
  D  DATA. Its TEXT (below) names a path in X (a string literal resolved
     against the workspace root, the naming file's directory and its package
     directory; `{..}`/`$var` segments are one-segment wildcards; an absolute
     path through any suffix that starts at a top-level entry of the tree),
     or names F itself when F is in no package; or a function in its text
     ENUMERATES the root or crates/ (read_dir/WalkDir/os.walk/glob/ls-files/
     find, in the same function as a literal resolving to the root or
     crates/, a root helper, or CARGO_MANIFEST_DIR walked up; any enumeration
     in a script, which runs at the root) -- such a unit can read every file.
  E  EXEC. Its text names a binary built from the tree -- `CARGO_BIN_EXE_<n>`,
     a path ending in a binary's name (`debug/axon`), a `<NAME>_BIN` variable,
     a cargo command line, a package name in a file that runs cargo
     (`-p P`: all of P's binaries), or a bare binary name inside a function
     that locates programs that way or puts a target dir on PATH -- of a
     package P whose binaries are relevant to F: by L/B/D over P's own files
     and closure, or because they exec one that is (a least fixed point).

The TEXT of a unit is its files, the src/** and build.rs of every package in
its link closure (a linked library's code runs inside the unit: axon-psv's
runner execs the interpreter), and every script any of those names,
recursively (a script that runs `cargo build -p P`, names a crate path or
walks the tree is part of the test that runs it). Other non-Rust files are
DATA: reading one is caught by the literal that names it; what it names is not
followed.

PROOF that a skipped unit cannot change its outcome under A. A test's outcome
is a function of (1) the code executing in its process, (2) the code of every
process it spawns, (3) the bytes it reads, and (4) the build that produced (1)
and (2). A changes only the bytes of F. (1) is compiled from the unit's files
and its link closure: F is in neither (not L; and no `mod`/`#[path]`/
`include!` of the unit names F, or D would hold). (2): a process built from
this tree reaches the unit only through cargo's target dirs, which it can
name only in the ways E lists; every package it can so exec is irrelevant to
F (not E, closed under exec); a program found by a bare PATH lookup was not
built from this tree (cells run with AXON/AXON_BIN/CORTEX_BIN removed, the
interpreter pinned by AXON_BIN and byte-restored after every cell). (3): a
library or program reads the paths it names itself (in its text, so D) or the
paths its caller hands it (named in the unit's text, so D); listing a
directory is the only way to reach a file nobody named, and listing the root
or crates/ makes the unit relevant (D). (4): cargo re-runs a build script --
the only way F's bytes enter an artifact other than by L or D -- only when
its re-run set changes; none covers F (not B). So every input of a skipped
unit is byte-identical with and without A, and so is its outcome.

The one input EVERY edit changes for every unit is the tree-state bit
(`git status` non-empty): axon-core's build script turns it into a `-dirty`
version suffix when it re-runs (a test refreshing .git/index makes it).
It is the same bit for every edit of every record, so it carries nothing
about which guard was removed and cannot make a skipped suite witness THIS
guard being load-bearing; the run's interpreter is byte-restored after every
cell as before (amendment 59).

DOUBT resolves to inclusion: literals in prose, wildcards that can match,
whole-crate matching of any named path in X, and every enumeration of the
root count. What the rule cannot follow -- a path assembled at run time from
pieces none of which names X, the root or crates/ -- cannot reach F without
listing a directory above it, which the walker clause includes.
"""
import json
import os
import re
import subprocess

RULE = "build-graph/1"


# ── reading the tree ────────────────────────────────────────────────────────

SKIP_DIRS = {"target", ".git", "node_modules", "__pycache__"}


def _walk(root, rel):
    out = []
    base = os.path.join(root, rel)
    if not os.path.isdir(base):
        return out
    for d, dirs, files in os.walk(base):
        dirs[:] = sorted(x for x in dirs if x not in SKIP_DIRS)
        for f in sorted(files):
            out.append(os.path.relpath(os.path.join(d, f), root))
    return out


class Tree:
    """The repository as text: `read(rel)` and `exists(rel)`; a test can
    substitute a dict of {path: text}."""

    def __init__(self, root, files=None):
        self.root = root
        self.files = files
        self._cache = {}

    def read(self, rel):
        if rel in self._cache:
            return self._cache[rel]
        if self.files is not None:
            t = self.files.get(rel)
        else:
            try:
                with open(os.path.join(self.root, rel), "rb") as f:
                    t = f.read().decode("utf-8", "replace")
            except OSError:
                t = None
        self._cache[rel] = t
        return t

    def exists(self, rel):
        rel = rel.rstrip("/")
        if self.files is not None:
            return rel == "" or rel in self.files or any(p.startswith(rel + "/") for p in self.files)
        return os.path.exists(os.path.join(self.root, rel))

    def isfile(self, rel):
        if self.files is not None:
            return rel in self.files
        return os.path.isfile(os.path.join(self.root, rel))

    def walk(self, rel):
        if self.files is not None:
            return sorted(p for p in self.files if p.startswith(rel.rstrip("/") + "/"))
        return _walk(self.root, rel)


def cargo_metadata(root):
    r = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=root,
                       capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit(f"refused: cargo metadata failed: {r.stderr.strip()[-300:]}")
    return json.loads(r.stdout)


# ── the workspace model ─────────────────────────────────────────────────────

class Workspace:
    def __init__(self, meta, tree):
        self.tree = tree
        self._files = {}
        root = meta["workspace_root"]
        self.pkgs = {}
        names = {p["name"] for p in meta["packages"]}
        for p in meta["packages"]:
            pdir = os.path.relpath(os.path.dirname(p["manifest_path"]), root)
            deps = [(d["name"], d.get("kind") or "normal") for d in p["dependencies"]
                    if d["name"] in names]
            targets = [{"kind": t["kind"], "name": t["name"],
                        "src": os.path.relpath(t["src_path"], root)} for t in p["targets"]]
            self.pkgs[p["name"]] = {"dir": pdir, "deps": deps, "targets": targets}
        # binary/example name -> package
        self.bins = {}
        for n, p in self.pkgs.items():
            for t in p["targets"]:
                if set(t["kind"]) & {"bin", "example"}:
                    self.bins.setdefault(t["name"], set()).add(n)

    def package_of(self, path):
        best = None
        for n, p in self.pkgs.items():
            d = p["dir"].rstrip("/") + "/"
            if path.startswith(d) and (best is None or len(d) > len(self.pkgs[best]["dir"]) + 1):
                best = n
        return best

    def lib_closure(self, name):
        """Normal + build dependencies, transitively, and the package itself."""
        seen, todo = set(), [name]
        while todo:
            n = todo.pop()
            if n in seen or n not in self.pkgs:
                continue
            seen.add(n)
            todo += [d for d, k in self.pkgs[n]["deps"] if k in ("normal", "build")]
        return seen

    def manifest_closure(self, name):
        """Every workspace package reachable through ANY dependency edge: what
        a program's freshness check reads (script_spawn::workspace_bin checks
        a binary against every `path =` dependency of its package's manifest,
        dev ones included), a superset of what it links."""
        seen, todo = set(), [name]
        while todo:
            n = todo.pop()
            if n in seen or n not in self.pkgs:
                continue
            seen.add(n)
            todo += [d for d, _ in self.pkgs[n]["deps"]]
        return seen

    def test_link_closure(self, name):
        """What a test of `name` links: its lib closure and its dev
        dependencies' lib closures."""
        out = self.lib_closure(name)
        for d, k in self.pkgs[name]["deps"]:
            if k == "dev":
                out |= self.lib_closure(d)
        return out

    def crate_files(self, name):
        if ("c", name) not in self._files:
            self._files[("c", name)] = self._crate_files(name)
        return self._files[("c", name)]

    def lib_files(self, name):
        if ("l", name) not in self._files:
            self._files[("l", name)] = self._lib_files(name)
        return self._files[("l", name)]

    def _crate_files(self, name):
        """The package's code that runs or is built in every unit: src/**,
        build.rs, examples/** (cargo test builds examples)."""
        d = self.pkgs[name]["dir"]
        files = self.tree.walk(f"{d}/src") + self.tree.walk(f"{d}/examples")
        for t in self.pkgs[name]["targets"]:
            if "custom-build" in t["kind"]:
                files.append(t["src"])
        return sorted(set(files))

    def _lib_files(self, name):
        """What a LINKED package contributes: src/** and build.rs."""
        d = self.pkgs[name]["dir"]
        files = self.tree.walk(f"{d}/src")
        for t in self.pkgs[name]["targets"]:
            if "custom-build" in t["kind"]:
                files.append(t["src"])
        return sorted(set(files))

    def test_targets(self, name):
        return [(t["name"], t["src"]) for t in self.pkgs[name]["targets"] if "test" in t["kind"]]


# ── text references ────────────────────────────────────────────────────────

_STR = re.compile(r'"((?:[^"\\\n]|\\.)*)"')
_SQ = re.compile(r"'((?:[^'\\\n]|\\.)*)'")
_TOK = re.compile(r"[^\s\"'`;|&<>(){}\[\],=]+")
_MOD = re.compile(r'(?:#\[path\s*=\s*"([^"]+)"\]\s*)?(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;')
_INCLUDE = re.compile(r'include(?:_str|_bytes)?!\s*\(\s*(?:concat!\s*\(\s*env!\s*\(\s*"CARGO_MANIFEST_DIR"\s*\)\s*,\s*)?"([^"]+)"')
SCRIPT_EXT = (".sh", ".bash", ".py")
# Primitives that LIST a directory. Naming the root or crates/ is how a path
# is joined onto a base; it READS what is below only when it enumerates it.
ENUMERATES = re.compile(r"read_dir\s*\(|WalkDir::|walkdir::|os\.walk\(|os\.listdir\(|os\.scandir\(|"
                        r"\bglob\.i?glob\(|\.r?glob\(|\.iterdir\(|ls-files|(?:^|[\s;|&(`])find\s+[^\s]|"
                        r"git\s+(?:add|status|diff|stash)")
# A function that locates a program BUILT FROM THE TREE. cargo is the only
# thing that builds one, into a target directory, so a unit can exec a
# tree-built binary only by: CARGO_BIN_EXE_<name>; a path into a target dir
# (`debug/axon`); a `<NAME>_BIN` variable; a cargo build/run of it
# (workspace_bin / script_spawn build with cargo); or by putting a target dir
# on PATH and calling the bare name. A bare name with none of these in its
# function is a PATH lookup, which finds no binary built from this tree (cells
# run with the ambient binary variables removed), or a label (cortex's
# FABRIC_SUBMIT_ID map key).
EXEC_CONTEXT = re.compile(r"CARGO_BIN_EXE|_BIN\b|workspace_bin|script_spawn|\bcargo\b|\"CARGO\"|"
                          r"\"debug\"|\"release\"|target_dir|\"target\"|\"PATH\"")
# A function that finds the workspace root without a literal: a root helper,
# or CARGO_MANIFEST_DIR walked up.
ROOT_HELPER = re.compile(r"\brepo_root\b|\bworkspace_root\b|\btoplevel\b|show-toplevel")
_FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+)*fn\s", re.M)


def fn_chunks(text):
    starts = [m.start() for m in _FN.finditer(text)] + [len(text)]
    return [text[:starts[0]]] + [text[a:b] for a, b in zip(starts, starts[1:])]


def literals(path, text):
    """Candidate strings a file names: string literals in Rust; in scripts and
    every other file, also every bare token (comments included -- doubt adds)."""
    out = set(_STR.findall(text))
    if not path.endswith(".rs"):
        out |= set(_SQ.findall(text))
        out |= set(_TOK.findall(text))
    return out


def _norm(p):
    parts = []
    for s in p.split("/"):
        if s in ("", "."):
            continue
        if s == "..":
            if parts and parts[-1] != "..":
                parts.pop()
            else:
                parts.append("..")
            continue
        parts.append(s)
    return "/".join(parts)


_WILD = re.compile(r"\{[^}]*\}|\$\{[^}]*\}|\$[A-Za-z_][A-Za-z0-9_]*|<[^>]*>")


def resolve(lit, path, pkg_dir, top=None):
    """Root-relative path patterns a literal can denote (wildcards as `*`).
    An absolute path can name a repository file only through a suffix that
    starts at a top-level entry of the repository (`top`)."""
    s = lit.strip()
    if not s or "\n" in s or len(s) > 400:
        return set()
    s = _WILD.sub("*", s)
    segs = [x for x in s.split("/") if x not in ("", ".")]
    # A literal that names nothing by itself (a format template `{}`, `*`,
    # `{a}.{b}`) is not a path: the value it formats comes from another
    # literal, which is matched on its own. Pure `..` chains are kept.
    if not segs or not any(x == ".." or ("*" not in x and re.search(r"[A-Za-z0-9_]", x)) for x in segs):
        return set()
    out = set()
    fdir = os.path.dirname(path)
    bases = [fdir]
    if pkg_dir:
        bases.append(pkg_dir)
    if s not in (".", "./"):
        bases.append("")  # the workspace root
    elif not path.endswith(".rs"):
        bases.append("")  # a script's `.` is its cwd, the root
    if s.startswith("/"):
        segs = [x for x in s.split("/") if x]
        for i in range(len(segs)):
            if top is None or segs[i] in top:
                out.add(_norm("/".join(segs[i:])))
        return {o for o in out if o and not o.startswith("..")}
    for b in bases:
        n = _norm(os.path.join(b, s) if b else s)
        if not n.startswith(".."):
            out.add(n)
    return out


def names_path(pattern, f):
    """Does a root-relative pattern denote F or a directory above it? The
    empty pattern is the root (reached only by explicit `..`)."""
    if pattern == "":
        return True
    if "*" in pattern:
        rx = re.compile("^" + "[^/]*".join(re.escape(x) for x in pattern.split("*")) + "$")
        segs = f.split("/")
        return any(rx.match("/".join(segs[:i])) for i in range(1, len(segs) + 1))
    return f == pattern or f.startswith(pattern.rstrip("/") + "/")


def module_closure(tree, start):
    """A Rust file and every file it pulls in by `mod`, `#[path]` or
    `include!` (both candidate locations of a module are taken)."""
    seen, todo = [], [start]
    while todo:
        f = todo.pop()
        if f in seen or not tree.isfile(f):
            continue
        seen.append(f)
        text = tree.read(f) or ""
        d = os.path.dirname(f)
        stem = os.path.splitext(os.path.basename(f))[0]
        own = d if stem in ("mod", "lib", "main") or "/tests/" in "/" + f and d.endswith("tests") \
            else os.path.join(d, stem)
        for pathattr, name in _MOD.findall(text):
            if pathattr:
                todo += [_norm(os.path.join(d, pathattr)), _norm(os.path.join(own, pathattr))]
            else:
                for b in (own, d):
                    todo += [_norm(f"{b}/{name}.rs"), _norm(f"{b}/{name}/mod.rs")]
        for inc in _INCLUDE.findall(text):
            todo.append(_norm(os.path.join(d, inc)))
    return seen


def is_script(tree, path):
    """A file a test can RUN: a shell or Python script (by extension or a
    `#!` line). Any other non-Rust file is data: what it names is not read."""
    if path.endswith(".rs"):
        return False
    if path.endswith(SCRIPT_EXT):
        return True
    t = tree.read(path)
    return bool(t) and t.startswith("#!")


class Selector:
    def __init__(self, ws):
        self.ws = ws
        if ws.tree.files is not None:
            self.top = {p.split("/")[0] for p in ws.tree.files}
        else:
            self.top = set(os.listdir(ws.tree.root))
        self._refs = {}
        self._units = {}
        self._unit_text = {}
        self._mods = {}
        self._reach = {}

    def _pkg_dir_of(self, f):
        p = self.ws.package_of(f)
        return self.ws.pkgs[p]["dir"] if p else None

    # ── what a set of files names ─────────────────────────────────────────

    def file_refs(self, f):
        """(paths, binaries, walks_base, scripts) one file names. Cached."""
        if f in self._refs:
            return self._refs[f]
        tree = self.ws.tree
        text = tree.read(f)
        paths, bins, scripts = set(), set(), set()
        walks = False
        rust = f.endswith(".rs")
        if text is not None and (rust or is_script(tree, f)):
            pdir = self._pkg_dir_of(f)
            walks = self.walks_base(f, text, pdir)
            runs_cargo = re.search(r"\bcargo\b|\"CARGO\"", text) is not None
            if rust:
                for chunk in fn_chunks(text):
                    code = re.sub(r"(?m)^\s*//.*$", "", chunk)  # comments are not code
                    ctx = EXEC_CONTEXT.search(code) is not None
                    for lit in _STR.findall(chunk):
                        bins |= self.binaries_named(f, lit, runs_cargo, ctx)
            for lit in literals(f, text):
                if not rust:
                    bins |= self.binaries_named(f, lit, runs_cargo, True)
                for pat in resolve(lit, f, pdir, self.top):
                    paths.add(pat)
                    if "*" not in pat and tree.isfile(pat) and is_script(tree, pat):
                        scripts.add(pat)
        named = {}
        for pat in sorted(paths):
            if pat == "":
                continue
            for x in self.ws.pkgs:
                if x not in named and self.in_package(pat, x):
                    named[x] = pat
        res = (paths, bins, walks, scripts, named)
        self._refs[f] = res
        return res

    def refs(self, files):
        """Union of file_refs over `files` and every script they name,
        recursively: (paths, binaries, the files that walk the root, and
        {package: a path naming it}). Cached per file set."""
        key = tuple(sorted(files))
        if key in self._units:
            return self._units[key]
        paths, bins, walkers, seen, named = set(), set(), [], set(), {}
        todo = list(files)
        while todo:
            f = todo.pop()
            if f in seen:
                continue
            seen.add(f)
            p, b, w, sc, nm = self.file_refs(f)
            paths |= p
            bins |= b
            for x, pat in nm.items():
                named.setdefault(x, pat)
            if w:
                walkers.append(f)
            todo += sorted(sc - seen)
        res = (paths, bins, sorted(walkers), named)
        self._units[key] = res
        return res

    def binaries_named(self, f, lit, runs_cargo, ctx):
        """(package, binary) pairs a literal can make the unit exec. A Rust
        literal names a tree-built binary when it is a path ending in its
        name, `CARGO_BIN_EXE_<name>`, a `<NAME>_BIN` variable, a cargo command
        line naming it, or its bare name inside a function that locates
        programs (EXEC_CONTEXT); a package name counts as `cargo build -p P`
        in a file that runs cargo. In a script every word counts."""
        out = set()
        s = lit.strip()
        if f.endswith(".rs"):
            words = [s.rsplit("/", 1)[-1]] if "/" in s else ([s] if ctx else [])
            words += [w for w in re.findall(r"[A-Za-z0-9_][A-Za-z0-9_.-]*", s)
                      if w.startswith("CARGO_BIN_EXE_") or w.endswith("_BIN")]
            if re.search(r"\bcargo\b", s):
                words += re.findall(r"[A-Za-z0-9_][A-Za-z0-9_.-]*", s)
        else:
            words = re.findall(r"[A-Za-z0-9_][A-Za-z0-9_.-]*", s) + [s.rsplit("/", 1)[-1]]
        for tok in words:
            if tok.startswith("CARGO_BIN_EXE_"):
                tok = tok[len("CARGO_BIN_EXE_"):]
            if tok in self.ws.bins:
                out |= {(p, tok) for p in self.ws.bins[tok]}
            if tok in self.ws.pkgs and (runs_cargo or not f.endswith(".rs")):
                out.add((tok, "*"))  # `cargo build -p P`: all of P
            if tok.endswith("_BIN") and tok[:-4]:
                nm = tok[:-4].lower().replace("_", "-")
                for cand in (nm, nm.replace("-", "_")):
                    if cand in self.ws.bins:
                        out |= {(p, cand) for p in self.ws.bins[cand]}
        return out

    def is_base(self, pat):
        """The root or a directory above packages (crates/): inside no package."""
        if "*" in pat:
            return False
        dirs = [p["dir"] for p in self.ws.pkgs.values()]
        if any(pat == d or pat.startswith(d + "/") for d in dirs):
            return False
        return pat == "" or any(d.startswith(pat + "/") for d in dirs)

    def walks_base(self, f, text, pdir):
        """Does this file ENUMERATE the root or crates/? A script runs at the
        root, so any enumeration in it does. In Rust, a function that
        enumerates and, in the same function, names the root or crates/ (a
        literal resolving there, a root helper, CARGO_MANIFEST_DIR walked up)."""
        if not f.endswith(".rs"):
            return bool(ENUMERATES.search(text))
        for c in fn_chunks(text):
            if not ENUMERATES.search(c):
                continue
            if ROOT_HELPER.search(c) or ("CARGO_MANIFEST_DIR" in c and re.search(r"\.ancestors\(|\.parent\(", c)):
                return True
            for lit in _STR.findall(c):
                if any(self.is_base(pat) for pat in resolve(lit, f, pdir, self.top)):
                    return True
        return False

    def in_package(self, pat, x):
        """Does a path pattern name something inside package X's directory?"""
        d = self.ws.pkgs[x]["dir"]
        if "*" not in pat:
            return pat == d or pat.startswith(d + "/")
        rx = "^" + "[^/]*".join(re.escape(y) for y in pat.split("*"))
        # the pattern can denote X's directory, something below it, or a
        # directory above it (a walk of crates/*)
        return (re.match(rx + "(/|$)", d) is not None or names_path(pat, d)
                or any(re.match(rx + "$", g) for g in [d + "/x"]))

    def build_script_covers(self, pkg, f):
        """Does `pkg`'s build script re-run set cover F (rule B)?"""
        for t in self.ws.pkgs[pkg]["targets"]:
            if "custom-build" not in t["kind"]:
                continue
            text = self.ws.tree.read(t["src"]) or ""
            pdir = self.ws.pkgs[pkg]["dir"]
            lines = re.findall(r"rerun-if-changed=([^\"\n]*)", text)
            if not lines:
                if names_path(pdir, f):
                    return "its build script declares no re-run set, so cargo re-runs it on any change in its package"
                continue
            for arg in lines:
                arg = arg.strip()
                if not arg or "{" in arg:
                    return "its build script watches a computed path (the whole tree, conservatively)"
                if names_path(_norm(os.path.join(pdir, arg)), f):
                    return f"its build script re-runs on `{arg}`"
        return None

    # ── relevance ─────────────────────────────────────────────────────────

    def direct(self, link, files, f, x):
        """Why a unit (link closure, own files) observes F by L, B or D; and
        the binaries it execs (for E)."""
        if x is not None and x in link:
            return f"L: links {x}", set()
        for p in sorted(link):
            why = self.build_script_covers(p, f)
            if why:
                return f"B: {p} -- {why}", set()
        ukey = (tuple(sorted(link)), tuple(files))
        if ukey not in self._unit_text:
            text = set(files)
            for p in link:
                text |= set(self.ws.lib_files(p))
            self._unit_text[ukey] = self.refs(sorted(text))
        paths, bins, walkers, named = self._unit_text[ukey]
        if walkers:
            return (f"D: {walkers[0]} enumerates the workspace root or crates/ "
                    f"(it can read every file of the tree)"), set()
        if x is not None and x in named:
            return f"D: names `{named[x]}`, a path in {x}", set()
        if x is None:
            for pat in sorted(paths):
                if pat != "" and names_path(pat, f):
                    return f"D: names `{pat}`", set()
        return None, {p for p, _ in bins}

    def program_reach(self, f, x):
        """{package: why its binaries observe F} for every package, as the
        least fixed point of: direct, or execs a package that does."""
        key = (f, x)
        if key in self._reach:
            return self._reach[key]
        direct, execs = {}, {}
        for p in self.ws.pkgs:
            why, b = self.direct(self.ws.manifest_closure(p), self.ws.crate_files(p), f, x)
            direct[p] = why
            execs[p] = b - {p}
        reach = {p: w for p, w in direct.items() if w}
        changed = True
        while changed:
            changed = False
            for p in self.ws.pkgs:
                if p in reach:
                    continue
                for q in sorted(execs[p]):
                    if q in reach:
                        reach[p] = f"E: execs {q} -- {reach[q]}"
                        changed = True
                        break
        self._reach[key] = reach
        return reach

    def unit_reason(self, link, files, f, x, reach, own):
        why, execs = self.direct(link, files, f, x)
        if why:
            return why
        for q in sorted(execs - {own}):
            if q in reach:
                return f"E: execs {q} -- {reach[q]}"
        return None

    def select(self, f, always):
        """{"rule", "mutated_file", "mutated_crate", "always", "run": {pkg: {"scope", "reason"}},
        "skipped": {pkg: reason}} for a mutation of F; `always` (owner, row
        package) run their full suites regardless and are not consumers."""
        x = self.ws.package_of(f)
        reach = self.program_reach(f, x)
        run, skipped = {}, {}
        for c in sorted(self.ws.pkgs):
            if c in always:
                continue
            link = self.ws.test_link_closure(c)
            why = self.unit_reason(link, self.ws.crate_files(c), f, x, reach, c)
            if why:
                run[c] = {"scope": "suite", "reason": why}
                continue
            targets = {}
            for name, src in self.ws.test_targets(c):
                if src not in self._mods:
                    self._mods[src] = module_closure(self.ws.tree, src) + self.ws.crate_files(c)
                tw = self.unit_reason(link, self._mods[src],
                                      f, x, reach, c)
                if tw:
                    targets[name] = tw
            if targets:
                run[c] = {"scope": sorted(targets), "reason": targets}
            else:
                skipped[c] = (f"no unit of {c} can observe {f}: {x or 'no package'} is not in its "
                              f"link closure {sorted(link)}, no binary it execs is built from or reads "
                              f"it, no path it names is in {x or f}, it enumerates no base directory, "
                              f"and no build script it builds re-runs on it")
        return {"rule": RULE, "mutated_file": f, "mutated_crate": x, "always": sorted(always),
                "run": run, "skipped": skipped}


def selector(root, meta=None, files=None):
    return Selector(Workspace(meta or cargo_metadata(root), Tree(root, files)))


def well_formed(sel):
    """Why a record's consumer_selection is not a reviewable selection (None:
    it is). --join refuses a record for which this is not None."""
    if not isinstance(sel, dict):
        return "no consumer_selection"
    if sel.get("not_applicable"):
        return None
    if sel.get("rule") != RULE:
        return f"consumer_selection rule {sel.get('rule')!r} is not {RULE!r}"
    for k in ("mutated_file", "run", "skipped", "always"):
        if k not in sel:
            return f"consumer_selection has no {k!r}"
    if not isinstance(sel["run"], dict) or not isinstance(sel["skipped"], dict):
        return "consumer_selection run/skipped are not maps"
    if set(sel["run"]) & set(sel["skipped"]):
        return "a consumer is both run and skipped"
    for c, v in sel["skipped"].items():
        if not isinstance(v, str) or not v:
            return f"skipped consumer {c} has no graph reason"
    for c, v in sel["run"].items():
        if not isinstance(v, dict) or "scope" not in v or not v.get("reason"):
            return f"run consumer {c} has no scope/reason"
    return None
