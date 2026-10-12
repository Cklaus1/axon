#!/usr/bin/env python3
"""Incremental row selection for `v022_g01_mutations.py --changed-since`, and
the shard plan of `v022_g01_mutations.py --shard=K/N`.

LANE FEEDBACK ONLY, NEVER EVIDENCE. A run over the selected rows is a sample
(its OUT.json carries `only` and `changed_since`): `--merge` and
`--check-status` refuse it, and no T7 list cites it. The evidence of record is
the full run at the final head.

Given prior run records (OUT.json of earlier runs, any scope, `--only` lists
included), a row of the universe is selected when ANY of these holds:

  absent      no record ran it;
  not-killed  its record is not good (baseline passed, KILLED by its own
              attack, interpreter restored: the run's `row_good`);
  registry    its registry entry differs from the record: file, old/new text
              digest, package, target, test, or attack marker;
  anchor      its anchored code changed between the record's commit and HEAD:
              --granularity=file  the anchored file's blob differs;
              --granularity=fn    the Rust fn enclosing the anchor (the row's
                                  old text, located on both commits) differs,
                                  or the anchor is not unique on either side;
                                  an anchor outside any fn, or in a file that
                                  is not Rust, falls back to the file;
  test        its killing test's source changed: the files the test is made
              of (`--lib`: the module file the test path names plus what it
              `include!`s, e.g. interp.rs's interp/tests_v022_*.rs; `--test`
              and `--bin`: the target's whole module closure), compared by
              blob (file) or by the test fn's own text (fn). A test fn that
              cannot be located on either commit selects the row.

A row named by several records is judged against each record whose commit is
not an ancestor of another of them (a later incremental run supersedes the
rows it re-ran; it is absent for the rest).

WHAT SELECTION MISSES (why it is never evidence): any change outside the
anchored file/fn and the test's own source that alters what the test observes
-- a callee, a caller, another module the code path crosses (R51: M3493/M3494
went KILLED -> refused_elsewhere at 3921a309 through a debug assert added in
interp/env.rs, a file neither row anchors), a test helper or fixture read at
run time, a dependency crate, Cargo features, the toolchain. --granularity=fn
also misses edits elsewhere in the anchored file or the test's file (helpers,
consts, statics, macros).

Preview or replay a selection without running anything (the registry is read
at REV, the records' rows are compared with REV's tree):

    python3 scripts/v022_mut_select.py --changed-since=A.json[,B.json...]
        [--granularity=file|fn] [--scope=S] [--only=M1,M2] [--head=REV] [--shards=N]

STOPS (`v022_g01_mutations.py --changed-since` runs no cell unless --force;
the preview prints the same lines):
  BROAD SELECTION  more than BROAD_SELECTION (30%) of the rows the records
                   cover (scope and --only) are selected for a reason other
                   than `absent`: the lane run duplicates the full run of
                   those rows at the final head, the evidence of record;
  UNRECORDED       any `absent` row: the records do not cover it, so there is
                   nothing to compare it with; narrow --scope/--only to the
                   recorded rows, or run the unrecorded rows as their own
                   baseline run. Never counted in the ratio above.

SHARD PLAN (--shards=N; --changed-since is then optional): how
`v022_g01_mutations.py --shard=K/N` splits the rows it would run. Rows in the
SERIAL CLASS -- their test runs as root, in a private namespace, or as a
guest's supervisor: package in SERIAL_PACKAGES, or a test fn that reads the
effective uid (a root-gated test), or a test fn that cannot be located -- all
go to shard 0, which holds nothing else; the other rows are dealt round-robin
to shards 1..N-1 (to all N when there is no serial row). Shard 0 runs ALONE:
the harness holds SERIAL_LOCK exclusively for it and shared for the others.
"""

import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
from v022_pd_consumers import _INCLUDE, _norm, module_closure  # noqa: E402  (the include!/mod resolver)
from v022_refusal_coverage import blank_non_code  # noqa: E402  (comment/literal blanking)

GRANULARITIES = ("file", "fn")
REASONS = ("absent", "not-killed", "registry", "anchor", "test")
NOT_EVIDENCE = ("incremental selection (--changed-since): lane feedback, NOT evidence; never merged "
                "as a shard nor cited as a T7 list; the full run at the final head is the evidence of record")
# A --changed-since selection above this share of the RECORDED rows (absent
# rows excluded) is the records' full run in all but name (R51's merge into
# main: 408 of the 444 rows the T7 lists recorded, besides 1855 rows no record
# covered): the run stops unless --force.
BROAD_SELECTION = 0.30
# Packages whose tests run as root, in a private mount/PID namespace, or as a
# guest's PID-1 supervisor. Under parallel shards their rows failed on clean
# baselines (R51 merge: stale opkit-ns.* scratch roots, a signal not forwarded
# within 5 s); run alone, all of them were KILLED.
SERIAL_PACKAGES = ("axon-fabric", "axon-guest-init", "axon-os")
_ROOT_GATE = re.compile(r"\bgeteuid\s*\(")
# Held exclusively by the --shard run that holds the serial class, shared by
# every other --shard run: on one host the serial shard never overlaps another.
SERIAL_LOCK = "/var/tmp/axon-v022-mutation-serial.lock"


class Git:
    """Blob reads through one `git cat-file --batch`, cached by blob id."""

    def __init__(self, root=ROOT):
        self.root = root
        self._cat = None
        self._blobs = {}
        self._trees = {}
        self._anc = {}

    def run(self, *args):
        r = subprocess.run(["git", *args], cwd=self.root, capture_output=True, text=True)
        return r.returncode, r.stdout

    def commit(self, rev):
        rc, out = self.run("rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}")
        return out.strip() if rc == 0 and out.strip() else None

    def blob(self, sha):
        if sha not in self._blobs:
            if self._cat is None:
                self._cat = subprocess.Popen(["git", "cat-file", "--batch"], cwd=self.root,
                                             stdin=subprocess.PIPE, stdout=subprocess.PIPE)
            self._cat.stdin.write(sha.encode() + b"\n")
            self._cat.stdin.flush()
            head = self._cat.stdout.readline().split()
            if len(head) != 3:
                raise SystemExit(f"refused: git cat-file cannot read blob {sha}")
            data = self._cat.stdout.read(int(head[2]) + 1)[:-1]
            self._blobs[sha] = data.decode("utf-8", "replace")
        return self._blobs[sha]

    def tree(self, commit):
        if commit not in self._trees:
            self._trees[commit] = GitTree(self, commit)
        return self._trees[commit]

    def is_ancestor(self, a, b):
        if (a, b) not in self._anc:
            self._anc[(a, b)] = a != b and subprocess.run(
                ["git", "merge-base", "--is-ancestor", a, b], cwd=self.root).returncode == 0
        return self._anc[(a, b)]


class GitTree:
    """A commit's files: `isfile`/`read` (what module_closure reads) and `blob`."""

    def __init__(self, git, commit):
        self.git, self.commit = git, commit
        r = subprocess.run(["git", "ls-tree", "-r", "-z", "--full-tree", commit], cwd=git.root,
                           capture_output=True)
        if r.returncode != 0:
            raise SystemExit(f"refused: cannot list the tree of {commit}")
        self.files = {}
        for ent in r.stdout.split(b"\0"):
            if ent:
                meta, path = ent.split(b"\t", 1)
                kind, sha = meta.split()[1:3]
                if kind == b"blob":
                    self.files[path.decode()] = sha.decode()
        self._crates = None

    def isfile(self, rel):
        return rel in self.files

    def blob(self, rel):
        return self.files.get(rel)

    def read(self, rel):
        sha = self.files.get(rel)
        return None if sha is None else self.git.blob(sha)

    def crate_dirs(self):
        """package name -> crate directory, from every Cargo.toml in the tree."""
        if self._crates is None:
            self._crates = {}
            for p in self.files:
                if p.endswith("Cargo.toml") and not p.startswith("target/"):
                    try:
                        name = tomllib.loads(self.read(p)).get("package", {}).get("name")
                    except tomllib.TOMLDecodeError:
                        continue
                    if name:
                        self._crates[name] = os.path.dirname(p)
        return self._crates


# ── locating Rust fns ───────────────────────────────────────────────────────

_FN_ITEM = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")
_SPANS = {}


def fn_spans(git, sha):
    """(start, end, name) of every fn with a body in a blob: `start` is the
    start of the line holding `fn`, `end` just past its closing brace."""
    if sha in _SPANS:
        return _SPANS[sha]
    text = git.blob(sha)
    clean = blank_non_code(text)
    out = []
    for m in _FN_ITEM.finditer(clean):
        j, depth, n = m.end(), 0, len(clean)
        while j < n:
            c = clean[j]
            if c in "([":
                depth += 1
            elif c in ")]":
                depth -= 1
            elif depth == 0 and c in "{;":
                break
            j += 1
        if j >= n or clean[j] == ";":
            continue
        depth, k = 0, j
        while k < n:
            if clean[k] == "{":
                depth += 1
            elif clean[k] == "}":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        out.append((clean.rfind("\n", 0, m.start()) + 1, min(k + 1, n), m.group(1)))
    _SPANS[sha] = out
    return out


def enclosing_fn(git, sha, lo, hi):
    """The innermost fn whose span holds [lo, hi): (name, text), or None."""
    best = None
    for s, e, name in fn_spans(git, sha):
        if s <= lo and hi <= e and (best is None or s >= best[0]):
            best = (s, e, name)
    return None if best is None else (best[2], git.blob(sha)[best[0]:best[1]])


def anchored_fn(tree, path, old):
    """(fn name, fn text) enclosing the row's anchor in `tree`, None when the
    anchor sits outside any fn, or a string saying why it cannot be located."""
    text = tree.read(path)
    if text is None:
        return f"{path} absent"
    n = text.count(old)
    if n != 1:
        return f"anchor occurs {n} times"
    lo = text.index(old)
    return enclosing_fn(tree.git, tree.blob(path), lo, lo + len(old))


# ── a test's source ─────────────────────────────────────────────────────────

def _include_closure(tree, start):
    seen, todo = [], [start]
    while todo:
        f = todo.pop()
        if f in seen or not tree.isfile(f):
            continue
        seen.append(f)
        if f.endswith(".rs"):
            todo += [_norm(os.path.join(os.path.dirname(f), inc)) for inc in _INCLUDE.findall(tree.read(f) or "")]
    return seen


def _target_root(tree, crate, toml, kind, name):
    for t in toml.get(kind, []):
        if t.get("name") == name and t.get("path"):
            return _norm(f"{crate}/{t['path']}")
    if kind == "test":
        cands = [f"{crate}/tests/{name}.rs", f"{crate}/tests/{name}/main.rs"]
    else:
        cands = [f"{crate}/src/bin/{name}.rs", f"{crate}/src/bin/{name}/main.rs"]
        if toml.get("package", {}).get("name") == name:
            cands.append(f"{crate}/src/main.rs")
    return next((c for c in cands if tree.isfile(c)), None)


def test_source(tree, package, target, test):
    """The files a row's test is made of in `tree`, or a string saying why
    they cannot be found. A target that is neither `--test` nor `--bin` is
    the package's library (its test path names a lib module)."""
    crate = tree.crate_dirs().get(package)
    if crate is None:
        return f"package {package} not in the tree"
    toml = tomllib.loads(tree.read(f"{crate}/Cargo.toml"))
    t = target.split()
    for flag, kind in (("--test", "test"), ("--bin", "bin")):
        if flag in t and t.index(flag) + 1 < len(t):
            root = _target_root(tree, crate, toml, kind, t[t.index(flag) + 1])
            return module_closure(tree, root) if root else f"{flag} {t[t.index(flag) + 1]} has no source"
    lib = _norm(f"{crate}/{toml.get('lib', {}).get('path', 'src/lib.rs')}")
    src = os.path.dirname(lib)
    segs = test.split("::")[:-1]
    for k in range(len(segs), 0, -1):
        for cand in (f"{src}/{'/'.join(segs[:k])}.rs", f"{src}/{'/'.join(segs[:k])}/mod.rs"):
            if tree.isfile(cand):
                return _include_closure(tree, cand)
    return _include_closure(tree, lib)


def test_fns(tree, files, test):
    """[(file, text)] of every fn named like the test in `files`."""
    name = test.split("::")[-1]
    out = []
    for f in files:
        if f.endswith(".rs"):
            sha = tree.blob(f)
            text = tree.git.blob(sha)
            out += [(f, text[s:e]) for s, e, n in fn_spans(tree.git, sha) if n == name]
    return out


def located_test(tree, row):
    """(files, test fns) of the row's test, widening a `--lib` module file to
    its whole module closure when the test is not in it; a string if not found."""
    files = test_source(tree, row[5], row[6], row[7])
    if isinstance(files, str):
        return files
    fns = test_fns(tree, files, row[7])
    if not fns and files:
        files = module_closure(tree, files[0])
        fns = test_fns(tree, files, row[7])
    return (files, fns) if fns else f"test fn {row[7].split('::')[-1]} not found"


# ── records and selection ───────────────────────────────────────────────────

def load_records(paths, git):
    """[(path, commit, {id: row record})]; refuses what is not a mutation run
    of a clean tree at a commit this repository holds."""
    out = []
    for p in paths:
        try:
            d = json.load(open(p))
        except (OSError, ValueError) as e:
            raise SystemExit(f"refused: --changed-since record {p}: {e}")
        if not str(d.get("schema", "")).startswith("axon-v022-mutation-run/") or not isinstance(d.get("mutations"), list):
            raise SystemExit(f"refused: --changed-since record {p} is not a mutation run record")
        if d.get("tree_clean") is not True:
            raise SystemExit(f"refused: --changed-since record {p} does not record a clean tree")
        commit = git.commit(str(d.get("commit")))
        if commit is None:
            raise SystemExit(f"refused: --changed-since record {p}: commit {d.get('commit')!r} is not in this repository")
        out.append((p, commit, {r.get("id"): r for r in d["mutations"]}))
    return out


def _registry_diff(rec, row, marker, digest):
    now = {"file": row[2], "package": row[5], "target": row[6], "test": row[7], "attack_marker": marker,
           **digest(row)}
    return [k for k, v in now.items() if rec.get(k) != v]


def _changed_files(then, now, files_then, files_now):
    return sorted(f for f in set(files_then) | set(files_now) if then.blob(f) != now.blob(f))


def judge(row, rec, prior, head, marker, digest, granularity, row_good):
    """Reasons to re-run `row` against one record `rec` made at tree `prior`."""
    out = []
    if not row_good(rec.get("baseline"), rec.get("result"), rec.get("interpreter_not_restored")):
        out.append(f"not-killed: baseline={rec.get('baseline')} result={rec.get('result')}")
    diff = _registry_diff(rec, row, marker, digest)
    if diff:
        out.append(f"registry: {', '.join(diff)} differ from the record")
    span = f"{prior.commit[:8]}..{head.commit[:8]}"
    path, old = row[2], row[3]
    if prior.blob(path) != head.blob(path):
        if granularity == "file":
            out.append(f"anchor: {path} changed {span}")
        elif not path.endswith(".rs"):
            out.append(f"anchor: {path} changed {span} (not Rust: the file decides)")
        else:
            a, b = anchored_fn(prior, path, old), anchored_fn(head, path, old)
            if isinstance(a, str) or isinstance(b, str):
                out.append(f"anchor: {path}: {a if isinstance(a, str) else b} ({prior.commit[:8] if isinstance(a, str) else head.commit[:8]})")
            elif a is None or b is None:
                out.append(f"anchor: {path} changed {span} (anchor outside any fn: the file decides)")
            elif a != b:
                out.append(f"anchor: fn {b[0]} in {path} changed {span}")
    t_prior, t_head = located_test(prior, row), located_test(head, row)
    if isinstance(t_prior, str) or isinstance(t_head, str):
        why = t_prior if isinstance(t_prior, str) else t_head
        out.append(f"test: {row[7]}: {why} ({prior.commit[:8] if isinstance(t_prior, str) else head.commit[:8]})")
    elif granularity == "file":
        changed = _changed_files(prior, head, t_prior[0], t_head[0])
        if changed:
            out.append(f"test: {', '.join(changed)} changed {span}")
    elif t_prior[1] != t_head[1]:
        out.append(f"test: fn {row[7].split('::')[-1]} in {', '.join(sorted({f for f, _ in t_head[1]}))} changed {span}")
    return out


def select(rows, markers, digest, row_good, record_paths, head_rev, granularity, git=None):
    """({id: [reasons]}, [(record path, commit)]): the rows of `rows`
    (registry 8-tuples, in order) that must re-run, and the records judged
    against. A row absent from the dict need not re-run."""
    if granularity not in GRANULARITIES:
        raise SystemExit(f"--granularity={granularity}: want one of {'|'.join(GRANULARITIES)}")
    git = git or Git()
    head_commit = git.commit(head_rev)
    if head_commit is None:
        raise SystemExit(f"refused: {head_rev} is not a commit")
    head = git.tree(head_commit)
    records = load_records(record_paths, git)
    out = {}
    for row in rows:
        mid = row[0]
        having = [(c, recs[mid]) for _, c, recs in records if mid in recs]
        if not having:
            out[mid] = ["absent: no record ran it"]
            continue
        latest = [(c, r) for c, r in having if not any(git.is_ancestor(c, c2) for c2, _ in having)]
        reasons = []
        for c, rec in latest:
            for r in judge(row, rec, git.tree(c), head, markers.get(mid), digest, granularity, row_good):
                if r not in reasons:
                    reasons.append(r)
        if reasons:
            out[mid] = reasons
    return out, [(p, c) for p, c, _ in records]


def report(selected, total, granularity):
    """Print each selected row's reasons, then the summary by reason."""
    for mid, reasons in selected.items():
        print(f"SELECT {mid}  " + "; ".join(reasons))
    counts = {k: sum(1 for rs in selected.values() if any(r.startswith(k + ":") for r in rs)) for k in REASONS}
    print(f"changed-since (--granularity={granularity}): {len(selected)}/{total} rows selected; by reason "
          + ", ".join(f"{k} {v}" for k, v in counts.items()) + " (a row counts under each reason it has)")
    print(f"NOT EVIDENCE: {NOT_EVIDENCE}", flush=True)


def stops(selected, rows):
    """The stop lines (module doc: STOPS) for a selection `selected` ({id:
    [reasons]}) of `rows` rows (scope and --only), [] when it may run."""
    absent = sum(1 for rs in selected.values() if rs[0].startswith("absent:"))
    recorded, changed = rows - absent, len(selected) - absent
    out = []
    if changed > BROAD_SELECTION * recorded:
        out.append(f"BROAD SELECTION: {changed}/{recorded} recorded rows selected "
                   f"({100 * changed / recorded:.0f}% > {BROAD_SELECTION:.0%}; absent rows not counted): this lane "
                   "run duplicates the full run of those rows at the final head, which is the evidence of record; "
                   "run that instead")
    if absent:
        out.append(f"UNRECORDED: the --changed-since records do not cover {absent}/{rows} rows of this scope and "
                   "--only (selected as absent): narrow --scope/--only to the recorded rows, or run the unrecorded "
                   "rows as their own baseline run")
    return out


# ── the shard plan ──────────────────────────────────────────────────────────

def serial_class(rows, tree):
    """{id: why} of the rows of `rows` (registry 8-tuples) whose test runs as
    root, in a private namespace or as a guest's supervisor (module doc: SHARD
    PLAN), judged on `tree`."""
    out = {}
    for row in rows:
        if row[5] in SERIAL_PACKAGES:
            out[row[0]] = f"package {row[5]}"
            continue
        loc = located_test(tree, row)
        if isinstance(loc, str):
            out[row[0]] = f"test not located: {loc}"
        elif any(_ROOT_GATE.search(blank_non_code(text)) for _, text in loc[1]):
            out[row[0]] = f"root-gated test: {row[5]} {row[6]} {row[7]}"
    return out


def shard_plan(ids, n, serial):
    """[ids of shard k for k in 0..n-1] of `ids` (registry order): every id of
    `serial` alone in shard 0, the others round-robin over shards 1..n-1 (over
    all n when none is serial, or n == 1)."""
    ser = [m for m in ids if m in serial]
    par = [m for m in ids if m not in serial]
    if n == 1:
        return [list(ids)]
    if not ser:
        return [par[k::n] for k in range(n)]
    return [ser] + [par[k::n - 1] for k in range(n - 1)]


def id_ranges(ids):
    """`ids` in numeric order, a run of consecutive numbers as one range
    (`M2880-M2892`): an id missing from `ids` breaks the range."""
    def num(m):
        d = re.fullmatch(r"M(\d+)", m)
        return int(d.group(1)) if d else None
    ordered = sorted(ids, key=lambda m: (num(m) is None, num(m) or 0, m))
    out = []
    for m in ordered:
        k = num(m)
        if out and k is not None and out[-1][2] is not None and k == out[-1][2] + 1:
            out[-1] = (out[-1][0], m, k)
        else:
            out.append((m, m, k))
    return ", ".join(a if a == b else f"{a}-{b}" for a, b, _ in out)


def plan_report(plan, serial):
    """Print the plan: the serial class (its ids, and why for each row that is
    not in a SERIAL_PACKAGES package), then each shard and how it runs."""
    n = len(plan)
    ids = [m for s in plan for m in s]
    ser = {m: serial[m] for m in ids if m in serial}
    kinds = {}
    for why in ser.values():
        k = why.split(":")[0]
        kinds[k] = kinds.get(k, 0) + 1
    print(f"shard plan, N={n}: {len(ids)} rows; serial class {len(ser)} rows"
          + (" (" + ", ".join(f"{k} {v}" for k, v in sorted(kinds.items())) + ")" if ser else ""))
    for m, why in ser.items():
        if not why.startswith("package "):
            print(f"  SERIAL {m}  {why}")
    if ser:
        print(f"  serial ids: {id_ranges(ser)}")
    for k, s in enumerate(plan):
        if ser and k == 0:
            print(f"shard 0/{n}: SERIAL CLASS, {len(s)} rows: run it ALONE, no other shard or mutation run on "
                  f"this host meanwhile (a --shard run holds {SERIAL_LOCK} exclusively for it, shared for the "
                  "others, so started together they wait for each other)")
        else:
            print(f"shard {k}/{n}: {len(s)} rows, in parallel with the other non-serial shards")
    if n == 1 and ser:
        print("  (N=1: the one shard holds the serial class, so it runs alone)")


def registry_at(git, rev):
    """The mutation registry module as committed at `rev` (imported from a
    scratch copy of the two registry files; nothing of it is run)."""
    tree = git.tree(rev)
    with tempfile.TemporaryDirectory(prefix="v022-mut-select-") as d:
        os.mkdir(os.path.join(d, "scripts"))
        for f in ("scripts/v022_g01_mutations.py", "scripts/v022_attack_markers.py"):
            text = tree.read(f)
            if text is None:
                raise SystemExit(f"refused: {f} is not in {rev}")
            with open(os.path.join(d, f), "w") as fh:
                fh.write(text)
        saved = sys.modules.pop("v022_attack_markers", None), list(sys.path), os.environ.get("V022_KEEP_TMPDIR")
        os.environ["V022_KEEP_TMPDIR"] = "1"  # no per-run TMPDIR: this import runs no cargo
        try:
            spec = importlib.util.spec_from_file_location("v022_mut_at_rev", os.path.join(d, "scripts/v022_g01_mutations.py"))
            mod = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(mod)
        finally:
            sys.modules.pop("v022_attack_markers", None)
            if saved[0] is not None:
                sys.modules["v022_attack_markers"] = saved[0]
            sys.path[:] = saved[1]
            if saved[2] is None:
                os.environ.pop("V022_KEEP_TMPDIR", None)
            else:
                os.environ["V022_KEEP_TMPDIR"] = saved[2]
    return mod


def main():
    opts = {}
    for a in sys.argv[1:]:
        k, eq, v = a.partition("=")
        if not eq or k not in ("--changed-since", "--granularity", "--scope", "--only", "--head", "--shards"):
            sys.exit("usage: v022_mut_select.py [--changed-since=A.json[,B.json...]] [--granularity=file|fn] "
                     "[--scope=g01|pci|binding|psv|all] [--only=M1,M2] [--head=REV] [--shards=N]")
        opts[k] = v
    if "--changed-since" not in opts and "--shards" not in opts:
        sys.exit("--changed-since=A.json[,B.json...] or --shards=N is required")
    shards = None
    if "--shards" in opts:
        if not opts["--shards"].isdigit() or int(opts["--shards"]) < 1:
            sys.exit(f"--shards={opts['--shards']}: want N >= 1")
        shards = int(opts["--shards"])
    git = Git()
    head = git.commit(opts.get("--head", "HEAD"))
    if head is None:
        sys.exit(f"refused: {opts.get('--head')} is not a commit")
    reg = registry_at(git, head)
    scope = opts.get("--scope", "g01")
    only = set(opts["--only"].split(",")) if "--only" in opts else None
    if only is not None:
        unknown = sorted(m for m in only if not any(r[0] == m for r in reg.MUTATIONS) or not reg.in_scope(m, scope))
        if unknown:
            sys.exit(f"--only: not active rows of scope {scope} at {head[:8]}: {unknown}")
    scope_rows = [r for r in reg.MUTATIONS if reg.in_scope(r[0], scope)]
    rows = [r for r in scope_rows if only is None or r[0] in only]
    if "--changed-since" in opts:
        gran = opts.get("--granularity", "file")
        sel, records = select(rows, reg.ATTACK_MARKERS, reg.row_digest, reg.row_good,
                              opts["--changed-since"].split(","), head, gran, git)
        print(f"registry and tree at {head}; records at " + ", ".join(f"{p} ({c[:8]})" for p, c in records))
        report(sel, len(rows), gran)
        for line in stops(sel, len(rows)):
            print(line, flush=True)
        rows = [r for r in rows if r[0] in sel]
    if shards is not None:
        print(f"--shard=K/{shards} at {head[:8]} (scope {scope}):")
        serial = serial_class(rows, git.tree(head))
        plan_report(shard_plan([r[0] for r in rows], shards, serial), serial)


if __name__ == "__main__":
    main()
