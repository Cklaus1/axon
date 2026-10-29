#!/usr/bin/env python3
"""Build the hx task pool (spec FU-1a, FU-1b, FU-1c step 1).

    python3 build.py --axon AXON --cortex CORTEX --out hx-pool.json \\
        --seed N --split-seed N [--stratum D|C|all] [--programs-dir DIR] \\
        [--prior-split OLD-POOL.json] [--deal-dev] [--bundles MANIFEST.json] \\
        [--report REPORT.json] [--jobs N]
    python3 build.py --check CORPUS.json --axon AXON --cortex CORTEX
    python3 build.py --inputs-digest [--programs-dir DIR]

Both binaries are REQUIRED arguments (no default path is guessed); their
sha256 is recorded in `meta.headroom` and `--check` refuses other binaries.

SOURCES (FU-1b). Stratum D: `STRATUM_D`, the 17 hardset snapshots no hard
set uses plus the tempt snapshot `traits_demo.ax`. Stratum C: every `*.ax`
under `--programs-dir` (default `programs/` beside this file; the hx3
HX3-C authored programs live in `programs-c/`). A program is
used only if it type-checks, has >= 2 tests, every test passes, and
../ab/corpus.py's `test_blocks` (whose brace counting is not string-aware, and
which run_ab grading uses) delimits every test block as a string-aware scan
does. The same check guards every test cut and the visible and hidden blocks
of each task; a failure rejects the candidate with a reason in the report.

CLASSES (FU-1a). Every task is one function body (`symbol`) whose pristine
body repairs it:

  multi   2 or 3 disjoint mutations in one body (knob "2"/"3"); each one alone
          type-checks and fails >= 1 pristine test
  caller  a mutation of callee `g` with g's own tests (the test blocks that
          call g directly) cut from the broken file, so the corpus stores
          `broken_source`, except the hidden pin (hx2 HX-1, `caller_pin`):
          the sorted-first direct test of g that fails on the mutant and
          calls no other function on a call path to g; no pin rejects. The
          visible failure text does not name g; knob =
          call depth from the visible test to g ("1", or "2" for >= 2)
  subtle  one mutations.SUBTLE_OPS operator (knob = operator id); no
          failing test's message is a type or parse error
  refine  in a program using `where` refinements or `@[verify]`; the visible
          failure text matches `verify failed in` (knob "verify") or
          `violates the refinement` (knob "refine")
  caps    the target is `@[contained]` or capability-narrow (its own
          `axon caps --fn` lists no fs/net/exec kind while the program's
          does); `axon caps --fn` of the pristine target lists no
          fs/net/exec kind (knob "-")
  erase   (hx2 HX-3a) stratum C only: the body replaced by its return
          type's typed default (mutators.erase); knob = the pristine
          function's line count, `fn` line to closing brace ("<8", "8-15",
          ">=16")
  sparse  (hx2 HX-3b) one mutation failing >= 3 tests; hidden = the caller
          pin of the mutated function, visible = the first other failing
          test, every other failing test cut (`sparse_cut`); knob "1"
  masked  (hx3 HX3-A) two disjoint mutations i, j of one body, j's operator
          not in OPAQUE; hidden = the caller pin on F_j, every other test of
          F_j cut, visible = the first of F_ij - F_j (`masked_split`); the
          masking witness: with the cut, the composite fails exactly V + {h}
          and the fault-j-only file exactly {h}; knob "s" (j in SUBTLE_OPS)
          or "o"; `ops`/`edits` in [i, j] order, `how` "masked:<i> ; <j>"
  excerpt (hx3 HX3-B) only on a bundle program (stratum "X", `--bundles`,
          bundle.py), whose target functions are the target program's own:
          one XFN_OPS mutation (knob "xfn") or, for a stratum-C target, the
          erase body (knob "erase"); hidden = the caller pin, visible = the
          first other failing test, nothing cut; the file minus the hidden
          block is over cortex's 24,000-character view cap
          (`excerpt_witness`); fields `target_program`, `target_stratum`,
          `view: "excerpt"`
  erase-long (hx3 HX3-C) stratum C only: the erase body (mutators.erase)
          of a function whose pristine span (`fn_lines`, `fn` line to
          closing brace) is >= ERASE_LONG_MIN = 24 lines (knob ">=24");
          hidden = the caller pin (no pin rejects), visible = the first
          other failing test (`pinned_visible`), nothing cut. Precedence:
          such a function's erase single is an erase-long candidate and
          never an erase candidate (one broken file serves one class), so
          class erase keeps the spans of < 24 lines

erase, erase-long and sparse come after multi in CLASSES, and masked and
excerpt after them, so they never take a broken file an earlier class uses.

Build checks on every task: the broken file is `axon check`-clean and its
check output names no E1201; ../ab/corpus.py `task_from_broken` validity (>= 2
tests fail, the arm-A workspace, the hidden check passes on pristine);
putting back the pristine body, with the pristine test blocks (grading's
view), passes every pristine test; `cortex locate` (the arm-B call) ranks the
symbol within run_ab.py's `--candidates` and lists it in `reached_by_all`.

SELECTION. Candidate order is a seeded shuffle (`--seed`); per program and
cell (class, knob) at most `--per-cell-program` tasks, distinct symbols first;
one broken file serves one class only. Programs whose cluster (`cluster`: a
bundle counts with its target program) has fewer than 3 valid tasks are
dropped. Task fields beyond corpus.load's: `class, knob, stratum, program,
caps_strict: true, seed, split`.

SPLIT (FU-1c step 1). By program: `split_programs`. Programs named in
`--prior-split` (an earlier pool) keep their split; new programs are split by
a fresh seeded shuffle, or, with `--deal-dev` (hx3 HX3-C: authored programs
are dev by construction), all dealt dev; `meta.headroom.deal` records which
("seeded" | "dev"). A bundle program's split is its target program's
(`assign_split`), and `--check` refuses a bundle task whose split is not. The
per-program cap on test (<= 8) is applied by calibrate.py after cell
selection, with `cap_per_program`.

CHECK. `--check CORPUS` re-runs every task's construction checks on its
stored broken file with the given binaries (whose sha256 must equal the
recorded ones) and compares the rebuilt task with the stored one, ignoring
`spectrum_rank` (and the corpus-level `examples_commit`). Pinned classes
recompute their pin: caller and sparse on the rebuilt pre-cut file
(`pinned_cut`), masked from the singles (`masked_cut`), excerpt and
erase-long (nothing cut) on the rebuilt broken file (`pinned_visible`).
"""
import argparse
import concurrent.futures as cf
import glob
import hashlib
import json
import os
import random
import re
import shutil
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = os.path.dirname(HERE)
ROOT = os.path.abspath(os.path.join(BENCH, "..", "..", ".."))
sys.path.insert(0, HERE)
import mutators as M  # noqa: E402  (pure)

# 17 snapshots under ../hardset/src that neither hard-dev nor hard-once uses
# (42 snapshots minus their 25 source files), plus the tempt-lane snapshot
# traits_demo.ax that no tempt task uses.
STRATUM_D = [
    "hardset/src/asi/kernel_orchestrator.ax",
    "hardset/src/browser/demo.ax",
    "hardset/src/enums.ax",
    "hardset/src/feature_tour.ax",
    "hardset/src/fintech/overdraft.ax",
    "hardset/src/flagship/cve/CVE-2024-34359/model_loader.ax",
    "hardset/src/flagship/cve/CVE-2024-5314/list_records.ax",
    "hardset/src/goals/redteam-goal.ax",
    "hardset/src/hello.ax",
    "hardset/src/kernel/hello_kernel_slice3.ax",
    "hardset/src/mobile/compute.ax",
    "hardset/src/options.ax",
    "hardset/src/r27/killable_agent.ax",
    "hardset/src/sealed/candidates/honest/sol.ax",
    "hardset/src/should_fail_test.ax",
    "hardset/src/structs.ax",
    "hardset/src/sum_types.ax",
    "tempt/src/examples/traits_demo.ax",
]
USED_SETS = ("hardset/hard-dev.json", "hardset/hard-once.json", "tempt/tempt.json")
CLASSES = ("refine", "caps", "caller", "subtle", "multi", "erase", "erase-long", "sparse", "masked", "excerpt")
# hx3 HX3-C: the shortest pristine span (lines) of an erase-long target.
ERASE_LONG_MIN = 24
DEALS = ("seeded", "dev")
# hx3 HX3-A E2: a changed literal no visible test exposes is not inferable.
OPAQUE = frozenset({"constant", "float-constant", "string-typo"})
# hx3 HX3-B: the fix needs a callee's parameter order or a struct's fields.
XFN_OPS = frozenset({"argswap-typed", "wrong-field"})
# rank.rs MAX_PROMPT_CHARS: over it cortex shows the candidates alone.
VIEW_CAP = 24_000
MIN_TASKS_PER_PROGRAM = 3
MIN_DEV, MIN_TEST = 10, 20
MAX_SITES = 6
WIDE_KINDS = re.compile(r"^(fs|net|exec)\b")
REFINE_RE = re.compile(r"verify failed in|violates the refinement")
TYPE_PARSE_RE = re.compile(r"\b(type|parse) error\b|\bE\d{4}\b", re.I)
# What `--check` does not compare: spectrum_rank drifts with locate's scoring,
# the rest is the compact encoding of texts compared directly.
CHECK_IGNORED = {"spectrum_rank", "sha256", "mutant_body", "broken_source"}


# ------------------------------------------------------------------ pure helpers

def sha256_bytes(b):
    return hashlib.sha256(b).hexdigest()


def sha256_file(path):
    with open(path, "rb") as f:
        return sha256_bytes(f.read())


def split_programs(programs, seed, prior=None, deal="seeded"):
    """{program: "dev"|"test"}. Programs in `prior` keep their split. The rest
    are sorted, shuffled with `seed`, and the first max(10, n // 3) go to dev
    (with no prior), or round(n / 3) of them (when extending a prior split),
    the remainder to test. `deal` "dev" (hx3 HX3-C `--deal-dev`) deals every
    one of the rest to dev instead."""
    if deal not in DEALS:
        raise ValueError(f"deal {deal!r} is not one of {DEALS}")
    prior = prior or {}
    out = {p: prior[p] for p in programs if p in prior}
    new = sorted(p for p in set(programs) if p not in prior)
    random.Random(seed).shuffle(new)
    n_dev = min(len(new), max(MIN_DEV, len(new) // 3)) if not prior else round(len(new) / 3)
    if deal == "dev":
        n_dev = len(new)
    for i, p in enumerate(new):
        out[p] = "dev" if i < n_dev else "test"
    return dict(sorted(out.items()))


def split_ok(split):
    n_dev = sum(v == "dev" for v in split.values())
    return n_dev >= MIN_DEV and len(split) - n_dev >= MIN_TEST


def recorded_split(prior, split):
    """The split a pool records: every program of the prior split (kept in
    this pool or not) plus this pool's, so a chained extension never deals a
    prior dev program to test (FU-1c step 4)."""
    return dict(sorted({**(prior or {}), **split}.items()))


def assign_split(programs, seed, prior, targets, deal="seeded"):
    """`split_programs` (with `deal`) for every program but the bundles; a
    bundle program (`targets` = {bundle: target program}) takes its target's
    split, from this split or the prior one (hx3 HX3A-2: never a fresh
    deal)."""
    out = split_programs([p for p in programs if p not in targets], seed, prior, deal)
    known = recorded_split(prior, out)
    for b in programs:
        if b in targets:
            if targets[b] not in known:
                raise SystemExit(f"bundle {b}: its target {targets[b]} has no split")
            out[b] = known[targets[b]]
    return dict(sorted(out.items()))


def bundle_split_reason(task, split):
    """`--check`'s HX3A-2 rule: None unless the task is on a bundle whose
    split is not its target program's recorded split."""
    tp = task.get("target_program")
    if tp and split.get(tp) != task.get("split"):
        return f"excerpt: split {task.get('split')} is not target {tp}'s {split.get(tp)}"
    return None


def cluster(t):
    """The program unit every per-program count keys on (hx3 HX3A-3): a
    bundle task counts with its target program."""
    return t.get("target_program", t["program"])


def cap_per_program(tasks, cap, seed):
    """At most `cap` tasks per program cluster (`cluster`); a larger one keeps
    a seeded draw (random.Random(f"{seed}:{cluster}").sample over its sorted
    ids)."""
    by = {}
    for t in tasks:
        by.setdefault(cluster(t), []).append(t["id"])
    keep = set()
    for p, ids in by.items():
        ids = sorted(ids)
        keep |= set(ids if len(ids) <= cap else random.Random(f"{seed}:{p}").sample(ids, cap))
    return [t for t in tasks if t["id"] in keep]


def drop_thin(by_program, min_tasks=MIN_TASKS_PER_PROGRAM, cluster_of=None):
    """(kept, dropped) {program: tasks}: a program is dropped when its cluster
    (`cluster_of` maps a bundle to its target program) has < `min_tasks`."""
    cluster_of = cluster_of or {}
    n = {}
    for p, v in by_program.items():
        n[cluster_of.get(p, p)] = n.get(cluster_of.get(p, p), 0) + len(v)
    thin = {p for p in by_program if n[cluster_of.get(p, p)] < min_tasks}
    return ({p: v for p, v in by_program.items() if p not in thin},
            {p: v for p, v in by_program.items() if p in thin})


def inputs_digest(programs_dir=None):
    """sha256 over the builder inputs (FU-1e): this directory's code, every
    program directory beside it (`programs*/`: programs/, programs-c/), the
    given `programs_dir` (--programs-dir) wherever it is, ../ab/corpus.py,
    ../common.py, ../heldout/mutators.py. The built sets, PREREG.md and
    README.md are outputs, not inputs."""
    dirs = sorted(glob.glob(os.path.join(HERE, "programs*", ""))) + ([programs_dir] if programs_dir else [])
    progs = {os.path.abspath(p) for d in dirs for p in glob.glob(os.path.join(d, "**"), recursive=True)
             if os.path.isfile(p)}
    files = sorted(glob.glob(os.path.join(HERE, "*.py")) + list(progs) +
                   [os.path.join(BENCH, "ab", "corpus.py"), os.path.join(BENCH, "common.py"),
                    os.path.join(BENCH, "heldout", "mutators.py")])
    lines = "".join(f"{os.path.relpath(p, BENCH)}\0{sha256_file(p)}\n" for p in files)
    return sha256_bytes(lines.encode())


def task_id(cls, source_file, broken):
    return f"hx-{cls}-{sha256_bytes((source_file + chr(0) + broken).encode())[:10]}"


def call_graph(src, fns):
    g = {}
    for f in fns:
        s = _span(src, f)
        body = src[s[0]:s[1]] if s else ""
        g[f] = {h for h in fns if h != f and re.search(r"\b%s\s*\(" % re.escape(h), body)}
    return g


def call_depth(graph, start, target):
    """Edges on the shortest static call path start -> target, or None."""
    seen, frontier, d = {start}, [start], 0
    while frontier:
        d += 1
        nxt = []
        for f in frontier:
            for h in graph.get(f, ()):
                if h == target:
                    return d
                if h not in seen:
                    seen.add(h)
                    nxt.append(h)
        frontier = nxt
    return None


def caller_pin(direct, failing, calls, reach):
    """hx2 HX-1: the hidden pin of a caller (or sparse) task on g, or None:
    the first name in sorted(`direct`) (g's direct tests) that is in
    `failing` (the single mutant's failing tests, before any cut) and whose
    test block calls no function in `reach` (the functions h != g with a
    static call path to g). `calls[t]` = the functions block t calls."""
    fails = set(failing)
    return next((t for t in sorted(direct) if t in fails and not calls.get(t, set()) & reach), None)


def caller_hidden_ok(task, direct, failing, calls, reach):
    """`--check`'s HX-1 rule: None when the caller task's hidden test is the
    pin recomputed on the rebuilt pre-cut file (`failing`), else the reason."""
    hidden = task.get("hidden")
    if hidden not in direct:
        return f"caller: hidden `{hidden}` is not a direct test of g"
    pin = caller_pin(direct, failing, calls, reach)
    if hidden != pin:
        return f"caller: hidden `{hidden}` is not the recomputed pin `{pin}`"
    return None


def sparse_cut(failing, direct, calls, reach):
    """hx2 HX-3b: (visible, hidden, cut) of a sparse task, or None. Needs
    >= 3 failing tests (before any cut); hidden = `caller_pin` (no pin
    rejects: a caller test as hidden re-creates the O-FU-08 shape); visible
    = the first other failing test; every other failing test is cut."""
    if len(failing) < 3:
        return None
    hidden = caller_pin(direct, failing, calls, reach)
    if hidden is None:
        return None
    rest = [n for n in failing if n != hidden]
    return rest[0], hidden, rest[1:]


def masked_split(f_i, f_j, f_ij, direct, calls, reach):
    """hx3 HX3-A: (visible, hidden, cut) of a masked task, or None. f_i, f_j,
    f_ij = the failing tests of fault i alone, fault j alone and the
    composite. hidden = `caller_pin` on f_j and must fail on the composite;
    cut = f_j - {hidden}; visible = the first of V = f_ij - f_j, which must
    be non-empty (as must f_i - f_j)."""
    if not set(f_i) - set(f_j):
        return None
    hidden = caller_pin(direct, f_j, calls, reach)
    if hidden is None or hidden not in f_ij:
        return None
    vis = [n for n in f_ij if n not in set(f_j)]
    if not vis:
        return None
    return vis[0], hidden, [n for n in f_j if n != hidden]


def excerpt_witness(given):
    """hx3 HX3-B: the file the model works on (the broken file minus the
    hidden block) is over cortex's view cap, so its view is an excerpt."""
    return len(given) > VIEW_CAP


def fn_lines(src, fn):
    """Lines of `fn` from its `fn` keyword to its closing brace."""
    m = re.search(r"\bfn %s\(" % re.escape(fn), src)
    return src[m.start():_span(src, fn)[1] + 1].count("\n") + 1


def erase_knob(src, fn):
    """hx2 HX-3a knob: `fn_lines` ("<8", "8-15", ">=16")."""
    n = fn_lines(src, fn)
    return "<8" if n < 8 else ("8-15" if n < 16 else ">=16")


def erase_long_knob(src, fn):
    """hx3 HX3-C knob: ">=24" when `fn_lines` >= ERASE_LONG_MIN, else None
    (no erase-long task)."""
    return f">={ERASE_LONG_MIN}" if fn_lines(src, fn) >= ERASE_LONG_MIN else None


def pinned_visible(failing, direct, calls, reach):
    """(visible, hidden) of an excerpt (HX3-B) or erase-long (HX3-C) task,
    or None: hidden = `caller_pin` on `failing` (nothing is cut, so these
    are the broken file's failing tests), visible = the first other failing
    test; needs >= 2 failing tests and a pin."""
    hidden = caller_pin(direct, failing, calls, reach)
    if hidden is None or len(failing) < 2:
        return None
    return next(n for n in failing if n != hidden), hidden


def _span(src, name):
    m = re.search(r"\bfn %s\(" % re.escape(name), src)
    if not m:
        return None
    o = src.find("{", m.start())
    d = 0
    for i in range(o, len(src)):
        if src[i] == "{":
            d += 1
        elif src[i] == "}":
            d -= 1
            if d == 0:
                return (o + 1, i)
    return None


# ------------------------------------------------------------------ harness

class Harness:
    """Every binary call of the builder. `corpus`/`common` read the axon path
    from AXON_BIN when imported, so they are imported here, after it is set."""

    def __init__(self, axon, cortex, test_timeout):
        for b in (axon, cortex):
            if not (os.path.isfile(b) and os.access(b, os.X_OK)):
                raise SystemExit(f"not an executable: {b}")
        self.axon, self.cortex = os.path.abspath(axon), os.path.abspath(cortex)
        os.environ["AXON_BIN"] = self.axon
        sys.path.insert(0, BENCH)
        sys.path.insert(0, os.path.join(BENCH, "ab"))
        import common
        import corpus
        from run_ab import CANDIDATES
        assert common.AXON == self.axon and corpus.AXON == self.axon
        self.common, self.corpus, self.candidates = common, corpus, int(CANDIDATES)
        self.test_timeout = test_timeout
        self.shas = {"axon_sha256": sha256_file(self.axon), "cortex_sha256": sha256_file(self.cortex)}

    def _in_ws(self, name, text, fn):
        ws = tempfile.mkdtemp(prefix="hx_build_")
        try:
            p = os.path.join(ws, name)
            with open(p, "w") as f:
                f.write(text)
            return fn(ws, p)
        finally:
            shutil.rmtree(ws, ignore_errors=True)

    def check(self, name, text):
        """(clean, output): exit 0 and no E1201 in what the check printed."""
        def go(ws, p):
            rc, out = self.common.run([self.axon, "check", p], timeout=self.test_timeout)
            return rc == 0 and "E1201" not in out, out
        return self._in_ws(name, text, go)

    def tests(self, name, text):
        """{test: (status, message)} or None when the run did not complete."""
        def go(ws, p):
            _, out = self.common.run([self.axon, "test", p, "--json"], timeout=self.test_timeout)
            res, saw = {}, False
            for line in out.splitlines():
                line = line.strip()
                if not line.startswith("{"):
                    continue
                try:
                    v = json.loads(line)
                except ValueError:
                    continue
                if v.get("type") == "summary":
                    saw = True
                elif "name" in v and "status" in v:
                    res[v["name"]] = (v["status"], v.get("message") or "")
            return res if saw else None
        return self._in_ws(name, text, go)

    def caps(self, name, text, fn=None):
        def go(ws, p):
            cmd = [self.axon, "caps", "--json", p] + (["--fn", fn] if fn else [])
            rc, out = self.common.run(cmd, timeout=self.test_timeout)
            for line in out.splitlines():
                if '"axon-caps/1"' in line:
                    return json.loads(line)
            return None
        return self._in_ws(name, text, go)

    def locate(self, name, text, hidden):
        def go(ws, p):
            rc, out = self.common.run([self.cortex, "locate", "--workspace", ws, "--file", name,
                                       "--check", hidden, "--axon", self.axon, "--json"], timeout=300)
            for line in out.splitlines():
                if '"cortex-locate/1"' in line:
                    return json.loads(line)
            return None
        return self._in_ws(name, text, go)


# ------------------------------------------------------------------ programs

class Program:
    def __init__(self, h, path, stratum, target=None, target_stratum=None):
        """`stratum` "X" is a bundle (bundle.py) of target program `target`
        (relative to ROOT, of `target_stratum`): only functions in the
        target's own text, which the bundle starts with, are targets."""
        self.path = path
        self.stratum = stratum
        self.target, self.target_stratum = target, target_stratum
        self.source_file = os.path.relpath(path, ROOT)
        self.name = os.path.basename(path)
        self.src = open(path).read()
        head = len(self.src)
        if stratum == "X":
            tsrc = open(os.path.join(ROOT, target)).read()
            head = len(tsrc) if self.src.startswith(tsrc) else 0
        self.tests = h.common.tests_in(self.src)
        self.reject = None
        clean, out = h.check(self.name, self.src)
        res = h.tests(self.name, self.src) if clean else None
        if not clean:
            self.reject = "pristine does not type-check (or names E1201)"
        elif len(self.tests) < 2:
            self.reject = "fewer than 2 tests"
        elif res is None or any(s != "ok" for s, _ in res.values()) or set(res) != set(self.tests):
            self.reject = "pristine tests do not all pass"
        else:
            why = blocks_reason(h.corpus, self.src)
            if why:
                self.reject = f"ungradeable: {why}"
        if not self.reject and stratum == "X" and not head:
            self.reject = "bundle: does not start with its target program's text"
        defs = h.common.defined_fns(self.src)
        self.fns = [f for f in dict.fromkeys(defs) if defs.count(f) == 1 and f not in self.tests
                    and f != "main" and _span(self.src, f) and self.src[slice(*_span(self.src, f))].strip()
                    and _span(self.src, f)[0] < head]
        self.graph = call_graph(self.src, list(dict.fromkeys(defs)))
        self.refine = bool(re.search(r"@\[verify|\bwhere\b", self.src))
        pc = h.caps(self.name, self.src) if not self.reject and stratum != "X" else None
        self.wide = bool(pc and any(WIDE_KINDS.match(k) for k in pc.get("capabilities", [])))
        blocks = h.corpus.test_blocks(self.src)
        self.direct_tests = {f: [n for n, a, b in blocks
                                 if re.search(r"\b%s\s*\(" % re.escape(f), self.src[a:b])] for f in self.fns}
        tests = set(self.tests)
        self.test_calls = {n: {f for f in self.graph if f not in tests
                               and re.search(r"\b%s\s*\(" % re.escape(f), self.src[a:b])} for n, a, b in blocks}
        self.caps_ok = ({f: self.caps_eligible(h, f) is None for f in self.fns}
                        if not self.reject and stratum != "X" else {})

    def pin_inputs(self, fn):
        """(direct, calls, reach) for `caller_pin` on g = `fn`: g's direct
        tests, the non-test functions each of their blocks calls (g left
        out), and the functions h != g with a static call path to g."""
        direct = self.direct_tests.get(fn, [])
        calls = {t: self.test_calls.get(t, set()) - {fn} for t in direct}
        reach = {f for f in self.graph if f != fn and call_depth(self.graph, f, fn) is not None}
        return direct, calls, reach

    def contained(self, fn):
        return bool(re.search(r"@\[contained[^\]]*\]\s*(?:@\[[^\]]*\]\s*)*(?:pub\s+)?fn\s+%s\(" % re.escape(fn),
                              self.src))

    def caps_eligible(self, h, fn):
        """None when `fn` can carry a caps task, else the reason it cannot."""
        pc = h.caps(self.name, self.src, fn)
        if pc is None or pc.get("unknown") or any(WIDE_KINDS.match(k) for k in pc.get("capabilities", [])):
            return "caps: pristine `axon caps --fn` lists an fs/net/exec kind (or unknown)"
        if not (self.contained(fn) or self.wide):
            return "caps: target neither @[contained] nor narrower than its program"
        return None


def eval_single(h, prog, fn, op, mutant, how):
    a, b = _span(prog.src, fn)
    broken = prog.src[:a] + mutant + prog.src[b:]
    clean, _ = h.check(prog.name, broken)
    if not clean:
        return None
    res = h.tests(prog.name, broken)
    if res is None:
        return None
    failing = [n for n, (s, _) in res.items() if s == "failed"]
    if not failing:
        return None
    return {"fn": fn, "op": op, "how": how, "mutant": mutant, "broken": broken,
            "edit": M.edit_of(prog.src[a:b], mutant), "failing": failing,
            "messages": {n: m for n, (s, m) in res.items() if s == "failed"}}


def singles_of(h, prog, fn, seed, max_singles):
    """`fn`'s single mutants; on a bundle (stratum X) XFN_OPS operators only,
    the only ones class excerpt takes."""
    a, b = _span(prog.src, fn)
    body = prog.src[a:b]
    ctx = M.context(prog.src, fn)
    ops = [o for o in M.ALL_OPS if o[0] in XFN_OPS] if prog.stratum == "X" else M.ALL_OPS
    cands = list(dict.fromkeys((op, mut, how) for op, mut, how in M.sites(body, ops, ctx, MAX_SITES)))
    random.Random(f"{seed}:{prog.source_file}:{fn}").shuffle(cands)
    out = []
    for op, mut, how in cands[:max_singles]:
        s = eval_single(h, prog, fn, op, mut, how)
        if s:
            out.append(s)
    return out


def erase_of(h, prog, fn):
    """hx2 HX-3a: `fn`'s erase single (eval_single's dict) or None: stratum
    C (or a bundle of a stratum-C target, hx3 HX3-B), a return type with a
    typed default, and an erased file that type-checks and fails a test."""
    ret = M.return_type(prog.src, fn)
    a, b = _span(prog.src, fn)
    stub = M.erase(prog.src[a:b], ret)
    if (prog.target_stratum if prog.stratum == "X" else prog.stratum) != "C" or stub is None \
            or stub == prog.src[a:b]:
        return None
    return eval_single(h, prog, fn, "erase", stub, "erase:" + ret)


def candidates(prog, singles, seed, multi_tries, erased=None):
    """{(class, knob): [candidate]} in seeded order; a candidate is the
    construction a task would be built from (`finish` validates it).
    `erased`: {fn: erase_of's single} (HX-3a). A bundle program (stratum X)
    gets excerpt candidates only, and no other program gets any."""
    if prog.stratum == "X":
        return excerpt_candidates(prog, singles, seed, erased)
    out = {}
    subtle_ops = {n for n, _ in M.SUBTLE_OPS}
    for fn, ss in singles.items():
        direct, calls, reach = prog.pin_inputs(fn)
        for s in ss:
            base = {"fn": fn, "how": s["how"], "broken": s["broken"], "ops": [s["op"]], "edits": [list(s["edit"])]}
            if s["op"] in subtle_ops and len(s["failing"]) >= 2:
                out.setdefault(("subtle", s["op"]), []).append(dict(base))
            if prog.refine and len(s["failing"]) >= 2:
                vis = next((n for n in s["failing"] if REFINE_RE.search(s["messages"][n])), None)
                if vis:
                    knob = "verify" if "verify failed in" in s["messages"][vis] else "refine"
                    out.setdefault(("refine", knob), []).append(dict(base, visible=vis))
            if prog.caps_ok.get(fn) and len(s["failing"]) >= 2:
                out.setdefault(("caps", "-"), []).append(dict(base))
            if direct:
                hidden = caller_pin(direct, s["failing"], calls, reach)
                out.setdefault(("caller", None), []).append(
                    dict(base, hidden=hidden, cut=[n for n in direct if n != hidden]))
            sc = sparse_cut(s["failing"], direct, calls, reach)
            if sc:
                out.setdefault(("sparse", "1"), []).append(dict(base, visible=sc[0], hidden=sc[1], cut=sc[2]))
        rng = random.Random(f"{seed}:{prog.source_file}:{fn}:multi")
        for n in (2, 3):
            if len(ss) < n:
                continue
            seen, made = set(), 0
            for _ in range(multi_tries * 4):
                pick = rng.sample(range(len(ss)), n)
                key = tuple(sorted(pick))
                if key in seen:
                    continue
                seen.add(key)
                edits = [ss[i]["edit"] for i in pick]
                if not M.disjoint(edits):
                    continue
                a, b = _span(prog.src, fn)
                body = M.compose(prog.src[a:b], edits)
                out.setdefault(("multi", str(n)), []).append(
                    {"fn": fn, "how": "multi:" + " ; ".join(ss[i]["how"] for i in sorted(pick)),
                     "broken": prog.src[:a] + body + prog.src[b:],
                     "ops": [ss[i]["op"] for i in sorted(pick)],
                     "edits": [list(ss[i]["edit"]) for i in sorted(pick)]})
                made += 1
                if made >= multi_tries:
                    break
        masked_candidates(out, prog, fn, ss, seed, multi_tries, (direct, calls, reach), subtle_ops)
    for fn, e in sorted((erased or {}).items()):
        base = {"fn": fn, "how": e["how"], "broken": e["broken"], "ops": ["erase"], "edits": [list(e["edit"])]}
        long_knob = erase_long_knob(prog.src, fn)
        if long_knob:
            pv = pinned_visible(e["failing"], *prog.pin_inputs(fn))
            if pv:
                out.setdefault(("erase-long", long_knob), []).append(dict(base, visible=pv[0], hidden=pv[1]))
        elif len(e["failing"]) >= 2:
            out.setdefault(("erase", erase_knob(prog.src, fn)), []).append(base)
    for k, v in out.items():
        random.Random(f"{seed}:{prog.source_file}:{k}").shuffle(v)
    return out


def masked_candidates(out, prog, fn, ss, seed, multi_tries, pin, subtle_ops):
    """hx3 HX3-A: ordered pairs (i, j) of `fn`'s disjoint singles, shuffled
    with seed `{seed}:{file}:{fn}:masked`; j's operator is not OPAQUE, F_j
    has a caller pin and F_i - F_j is non-empty; at most `multi_tries`."""
    direct, calls, reach = pin
    pairs = [(i, j) for i in range(len(ss)) for j in range(len(ss)) if i != j]
    random.Random(f"{seed}:{prog.source_file}:{fn}:masked").shuffle(pairs)
    a, b = _span(prog.src, fn)
    made = 0
    for i, j in pairs:
        si, sj = ss[i], ss[j]
        if sj["op"] in OPAQUE or not M.disjoint([si["edit"], sj["edit"]]):
            continue
        if caller_pin(direct, sj["failing"], calls, reach) is None or not set(si["failing"]) - set(sj["failing"]):
            continue
        out.setdefault(("masked", "s" if sj["op"] in subtle_ops else "o"), []).append(
            {"fn": fn, "how": f"masked:{si['how']} ; {sj['how']}",
             "broken": prog.src[:a] + M.compose(prog.src[a:b], [si["edit"], sj["edit"]]) + prog.src[b:],
             "ops": [si["op"], sj["op"]], "edits": [list(si["edit"]), list(sj["edit"])],
             "f_i": list(si["failing"]), "f_j": list(sj["failing"])})
        made += 1
        if made >= multi_tries:
            break


def excerpt_candidates(prog, singles, seed, erased):
    """hx3 HX3-B on a bundle: each XFN_OPS single (knob "xfn") and, for a
    stratum-C target, each erase single (knob "erase") failing >= 2 tests
    with a caller pin; visible = the first other failing test."""
    out = {}
    for knob, fn, s in ([("xfn", fn, s) for fn, ss in singles.items() for s in ss if s["op"] in XFN_OPS] +
                        [("erase", fn, e) for fn, e in sorted((erased or {}).items())]):
        pv = pinned_visible(s["failing"], *prog.pin_inputs(fn))
        if pv is None:
            continue
        out.setdefault(("excerpt", knob), []).append(
            {"fn": fn, "how": s["how"], "broken": s["broken"], "ops": [s["op"]], "edits": [list(s["edit"])],
             "hidden": pv[1], "visible": pv[0]})
    for k, v in out.items():
        random.Random(f"{seed}:{prog.source_file}:{k}").shuffle(v)
    return out


def code_end(src, o):
    """Index just past the brace that closes `src[o] == "{"`, skipping string
    literals (with backslash escapes) and `//` comments; None if unclosed."""
    d, i, n = 0, o, len(src)
    while i < n:
        c = src[i]
        if c == '"':
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == "\\" else 1
        elif src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
            continue
        elif c == "{":
            d += 1
        elif c == "}":
            d -= 1
            if d == 0:
                return i + 1
        i += 1
    return None


def cut_tests(corpus, text, names):
    """(text without the named tests, None) or (None, reason). The cut is
    ../ab/corpus.py's `cut_test` (what grading and corpus.load see), whose
    brace counting is not string-aware: a block it cannot find, or delimits
    differently from a string-aware scan, rejects the candidate."""
    for n in names:
        blocks = {b[0]: b for b in corpus.test_blocks(text)}
        if n not in blocks:
            return None, f"corpus.test_blocks cannot delimit test `{n}` (brace in a string?)"
        _, a, b = blocks[n]
        m = re.compile(r"[ \t]*@\[test[^\n]*\n(?:[ \t]*@\[[^\n]*\n)*\s*fn %s\(" % re.escape(n)).match(text, a)
        end = code_end(text, text.find("{", m.end())) if m else None
        if end is None or not (b == end or (b == end + 1 and text[end] == "\n")):
            return None, f"corpus.test_blocks mis-delimits test `{n}` (brace in a string?)"
        text = text[:a] + text[b:]
    return text, None


def blocks_reason(corpus, src):
    """None when ../ab/corpus.py's test_blocks delimits every test of `src`
    as a string-aware scan does, else the reason. run_ab grading strips and
    re-appends test blocks with test_blocks, so a program failing this is
    ungradeable."""
    for n in corpus.tests_in(src):
        why = cut_tests(corpus, src, [n])[1]
        if why:
            return why
    return None


def graded_view(h, final, pristine):
    """run_ab grading's file: the final file's test blocks removed, the
    pristine blocks appended."""
    for n, a, b in reversed(h.corpus.test_blocks(final)):
        final = final[:a] + final[b:]
    blocks = "".join(pristine[a:b] for _, a, b in h.corpus.test_blocks(pristine))
    return final.rstrip("\n") + "\n\n" + blocks


def rebuild(src, fn, cls, ops, edits, how, stratum=None, target_stratum=None):
    """(broken file before any test cut, None) or (None, reason), from the
    pristine `src` and a task's recorded construction alone: one edit per
    operator, 2-3 for `multi`, exactly 2 for `masked` and 1 otherwise,
    `subtle` only with a SUBTLE_OPS operator; each edit must be what its
    operator makes at one of its first MAX_SITES sites of `fn`'s pristine
    body (as M.edit_of gives it), the edits disjoint, and `how` their labels.
    Class `erase` (HX-3a) takes exactly ops ["erase"], the edit that makes the
    body `M.erase(body, return type)`, and `how` "erase:<return type>"; so
    does `erase-long` (HX3-C), on a pristine span of >= ERASE_LONG_MIN lines
    only; no other class but `excerpt` takes the erase operator. hx3: `masked`
    (HX3-A) stores [i, j] with `how` "masked:<i> ; <j>" and j not OPAQUE;
    `excerpt` (HX3-B) is the only class of a `stratum` "X" program, with one
    XFN_OPS edit or, for a stratum-C `target_stratum`, the erase edit."""
    n = len(edits)
    if n == 0 or len(ops) != n:
        return None, f"{cls}: {len(ops)} operators for {n} edits"
    if n != {"multi": n if 2 <= n <= 3 else 0, "masked": 2}.get(cls, 1):
        return None, f"{cls}: {n} edits do not fit the class"
    if cls == "subtle" and ops[0] not in dict(M.SUBTLE_OPS):
        return None, f"subtle: operator {ops[0]} is not a subtle operator"
    if cls == "masked" and ops[1] in OPAQUE:
        return None, f"masked: the masked operator {ops[1]} is OPAQUE"
    if (cls == "excerpt") != (stratum == "X"):
        return None, f"{cls}: class excerpt is the only class of a bundle program (stratum {stratum})"
    if cls == "excerpt" and ops != ["erase"] and ops[0] not in XFN_OPS:
        return None, f"excerpt: operator {ops[0]} is not an XFN_OPS operator"
    if cls == "excerpt" and ops == ["erase"] and target_stratum != "C":
        return None, f"excerpt: erase needs a stratum-C target (not {target_stratum})"
    erase_classes = ("erase", "erase-long", "excerpt")
    if (ops == ["erase"] and cls not in erase_classes) or (cls in erase_classes[:2] and ops != ["erase"]):
        return None, f"{cls}: the erase operator belongs to classes erase, erase-long (and excerpt) alone"
    span = _span(src, fn)
    if not span:
        return None, f"{cls}: no body for `{fn}`"
    a, b = span
    if cls == "erase-long" and fn_lines(src, fn) < ERASE_LONG_MIN:
        return None, f"erase-long: `{fn}` spans {fn_lines(src, fn)} < {ERASE_LONG_MIN} lines"
    if ops == ["erase"]:
        ret = M.return_type(src, fn)
        stub = M.erase(src[a:b], ret)
        if stub is None:
            return None, f"erase: return type {ret!r} has no typed default"
        if tuple(edits[0]) != M.edit_of(src[a:b], stub) or how != "erase:" + ret:
            return None, f"erase: edit {list(edits[0])} / `how` {how!r} is not the typed-default body"
        return src[:a] + stub + src[b:], None
    body, ctx, all_ops = src[a:b], M.context(src, fn), dict(M.ALL_OPS)
    prefix = {"multi": "multi:", "masked": "masked:"}.get(cls)
    labels = [how] if n == 1 else (how[len(prefix):].split(" ; ") if prefix and how.startswith(prefix) else [])
    if len(labels) != n:
        return None, f"{cls}: `how` {how!r} does not list {n} mutations"
    edits = [tuple(e) for e in edits]
    for op, e, label in zip(ops, edits, labels):
        if op not in all_ops or len(e) != 3 or not 0 <= e[0] <= e[1] <= len(body):
            return None, f"{cls}: unknown operator {op} or malformed edit {list(e)}"
        mutant = body[:e[0]] + e[2] + body[e[1]:]
        if M.edit_of(body, mutant) != e:
            return None, f"{cls}: edit {list(e)} is not in canonical form"
        made = {hw for _, m, hw in M.sites(body, [(op, all_ops[op])], ctx, MAX_SITES) if m == mutant}
        if label not in made:
            return None, f"{cls}: edit {list(e)} labelled {label!r} is not a site of operator {op}"
    if not M.disjoint(edits):
        return None, f"{cls}: edits overlap"
    return src[:a] + M.compose(body, edits) + src[b:], None


def names_symbol(res, hidden, fn):
    """The first failing test other than `hidden` (so one the model sees)
    whose message names `fn`, or None."""
    pat = re.compile(r"\b%s\b" % re.escape(fn))
    return next((n for n, (s, m) in sorted(res.items()) if s == "failed" and n != hidden and pat.search(m)), None)


def singles_fail(h, prog, fn, edits):
    """`--check`'s re-run of FU-1a's per-mutation check: each edit alone on
    the pristine file type-checks and fails >= 1 pristine test."""
    a, b = _span(prog.src, fn)
    body = prog.src[a:b]
    for e in edits:
        e = tuple(e)
        if len(e) != 3 or not 0 <= e[0] <= e[1] <= len(body):
            return f"malformed edit {list(e)}"
        single = prog.src[:a] + body[:e[0]] + e[2] + body[e[1]:] + prog.src[b:]
        if not h.check(prog.name, single)[0]:
            return f"edit {list(e)} alone does not type-check"
        res = h.tests(prog.name, single)
        if res is None or not any(s == "failed" for s, _ in res.values()):
            return f"edit {list(e)} alone fails no pristine test"
    return None


def single_failing(h, prog, fn, edit):
    """The failing tests of `fn`'s pristine body with one edit applied, or
    None when the run does not complete."""
    a, b = _span(prog.src, fn)
    e = tuple(edit)
    res = h.tests(prog.name, prog.src[:a] + prog.src[a:b][:e[0]] + e[2] + prog.src[a:b][e[1]:] + prog.src[b:])
    return None if res is None else [n for n, (s, _) in res.items() if s == "failed"]


def masked_cut(h, prog, fn, broken, edits, f_i, f_j, visible, hidden):
    """hx3 HX3-A: ((visible, hidden, cut, V), None) or (None, reason). The
    split is `masked_split` on the composite's failing tests; f_i / f_j (the
    singles' failing tests) are recomputed when not given. The masking
    witness's fault-j-only half: that file with the cut fails exactly {h}."""
    if f_i is None or f_j is None:
        f_i, f_j = (single_failing(h, prog, fn, e) for e in edits)
        if f_i is None or f_j is None:
            return None, "masked: a single's test run did not complete"
    pre = h.tests(prog.name, broken)
    if pre is None:
        return None, "masked: the composite test run did not complete"
    f_ij = [n for n, (s, _) in pre.items() if s == "failed"]
    direct, calls, reach = prog.pin_inputs(fn)
    ms = masked_split(f_i, f_j, f_ij, direct, calls, reach)
    if ms is None:
        return None, "masked: no masked split (a pin on F_j failing on the composite, F_ij - F_j and F_i - F_j)"
    if (visible or ms[0], hidden or ms[1]) != ms[:2]:
        return None, f"masked: visible/hidden {visible}/{hidden} are not masked_split's {ms[:2]}"
    a, b = _span(prog.src, fn)
    e = tuple(edits[1])
    jonly = prog.src[:a] + prog.src[a:b][:e[0]] + e[2] + prog.src[a:b][e[1]:] + prog.src[b:]
    jonly, why = cut_tests(h.corpus, jonly, ms[2])
    if why:
        return None, why
    r = h.tests(prog.name, jonly)
    if r is None or sorted(n for n, (s, _) in r.items() if s == "failed") != [ms[1]]:
        return None, "masked: the fault-j-only file with the cut must fail exactly the hidden test"
    return (*ms, [n for n in f_ij if n not in set(f_j)]), None


def finish(h, prog, cls, cand, seed, visible=None, hidden=None):
    """(task, None) or (None, reason): every build check of FU-1a. The broken
    file is rebuilt from the pristine source and the recorded edits
    (`rebuild`), and the knob derived (from the operators, the call depth or
    the failure text), so `--check` re-validates both. Caller and sparse
    tasks carry their hidden pin (`cand["hidden"]`, HX-1/HX-3b); masked
    (HX3-A), excerpt (HX3-B) and erase-long (HX3-C) recompute theirs here."""
    fn, ops = cand["fn"], list(cand.get("ops") or [])
    broken, why = rebuild(prog.src, fn, cls, ops, list(cand.get("edits") or []), cand["how"],
                          prog.stratum, prog.target_stratum)
    if why:
        return None, why
    knob = {"multi": str(len(ops)), "subtle": ops[0], "caps": "-", "sparse": "1"}.get(cls)
    hidden = hidden or cand.get("hidden")
    visible = visible or cand.get("visible")
    if cls == "caller" and not hidden:
        return None, "caller: no direct test of g fails on the mutant; its contract cannot be pinned"
    if cls in ("erase", "erase-long"):
        if prog.stratum != "C":
            return None, f"{cls}: stratum C only (a public stratum-D body may be memorized)"
        knob = erase_knob(prog.src, fn) if cls == "erase" else erase_long_knob(prog.src, fn)
    cut = cand.get("cut")
    if cls == "masked":
        ms, why = masked_cut(h, prog, fn, broken, cand["edits"], cand.get("f_i"), cand.get("f_j"), visible, hidden)
        if why:
            return None, why
        visible, hidden, cut, masked_v = ms
        knob = "s" if ops[1] in dict(M.SUBTLE_OPS) else "o"
    if cls == "excerpt":
        knob = "erase" if ops == ["erase"] else "xfn"
    if cls in ("caller", "sparse", "masked"):
        broken, why = cut_tests(h.corpus, broken, cut)
        if why:
            return None, why
    clean, _ = h.check(prog.name, broken)
    if not clean:
        return None, "broken does not type-check (or names E1201)"
    res = h.tests(prog.name, broken)
    if res is None:
        return None, "broken test run did not complete"
    failing = [n for n, (s, _) in res.items() if s == "failed"]
    if cls == "masked" and sorted(failing) != sorted(set(masked_v) | {hidden}):
        return None, "masked: the composite with the cut must fail exactly V and the hidden test"
    if cls in ("excerpt", "erase-long"):
        pv = pinned_visible(failing, *prog.pin_inputs(fn))
        if pv is None or (visible or pv[0], hidden or pv[1]) != pv:
            return None, f"{cls}: visible/hidden {visible}/{hidden} are not the pin's {pv}"
        visible, hidden = pv
    if cls == "caller":
        if len(failing) < 2:
            return None, "caller: fewer than 2 failing tests once g's tests are cut"
        visible = visible or next((n for n in failing if n != hidden), None)
        if visible not in failing:
            return None, "caller: visible does not fail"
        if re.search(r"\b%s\b" % re.escape(fn), res[visible][1]):
            return None, "caller: the visible failure text names g"
        depth = call_depth(prog.graph, visible, fn)
        if depth is None or depth < 2:
            return None, "caller: no static call path from the visible test to g"
        knob = "1" if depth == 2 else "2"
    if cls == "sparse" and sorted(failing) != sorted({visible, hidden}):
        return None, "sparse: the broken file must fail exactly the visible and the hidden test once the rest are cut"
    if cls == "subtle" and any(TYPE_PARSE_RE.search(res[n][1]) for n in failing):
        return None, "subtle: a failing test reports a type or parse error"
    if cls == "refine":
        if visible not in failing or not REFINE_RE.search(res[visible][1]):
            return None, "refine: the visible failure is not a refinement or verify failure"
        knob = "verify" if "verify failed in" in res[visible][1] else "refine"
    if cls == "caps":
        why = prog.caps_eligible(h, fn)
        if why:
            return None, why
    try:
        t = h.corpus.task_from_broken(prog.path, prog.src, fn, cls, cand["how"], broken, visible, hidden)
    except (KeyError, IndexError) as e:
        return None, f"corpus.task_from_broken cannot delimit a test block ({type(e).__name__})"
    if t is None:
        return None, "corpus.task_from_broken validity"
    if cls == "excerpt" and not excerpt_witness(t["given_a"]):
        return None, f"excerpt: the file without the hidden block is {len(t['given_a'])} <= {VIEW_CAP} characters"
    for n in (t["visible"], t["hidden"]):  # the blocks corpus.load and grading cut or read
        why = cut_tests(h.corpus, broken, [n])[1]
        if why:
            return None, why
    a, b = _span(broken, fn)
    restored = broken[:a] + t["oracle_body"] + broken[b:]
    view = graded_view(h, restored, prog.src)
    if restored != prog.src:
        r = h.tests(prog.name, view)
        if r is None or set(r) != set(prog.tests) or any(s != "ok" for s, _ in r.values()):
            return None, "restoring the pristine body does not pass every pristine test"
    loc = h.locate(t["file"], t["broken"], t["hidden"])
    if loc is None:
        return None, "cortex locate gave no verdict"
    ranked = [r["symbol"] for r in loc.get("ranked", [])]
    rank = ranked.index(fn) + 1 if fn in ranked else 0
    if not 1 <= rank <= h.candidates:
        return None, f"locate ranks the symbol {rank}, outside the top {h.candidates}"
    if fn not in (loc.get("reached_by_all") or []):
        return None, "the symbol is not in reached_by_all of the failing visible checks"
    if cls == "caller":
        g = names_symbol(res, t["hidden"], fn)
        if g:
            return None, f"caller: failing test `{g}`, which the model sees, names g"
    t.update(id=task_id(cls, prog.source_file, t["broken"]), knob=knob, ops=ops,
             edits=[list(e) for e in cand["edits"]], stratum=prog.stratum,
             program=prog.source_file, caps_strict=True, seed=seed, spectrum_rank=rank)
    if cls in ("caller", "sparse", "masked"):
        t["cut_tests"] = list(cut)
    if cls == "masked":
        t["masked_op"] = ops[1]
    if cls == "excerpt":
        t.update(target_program=prog.target, target_stratum=prog.target_stratum, view="excerpt")
    if cls == "caller":
        t["call_depth"] = depth - 1
    return t, None


def build_program(h, prog, singles, seed, per_cell, multi_tries, erased=None):
    """(tasks, rejections) for one program: per cell at most `per_cell`
    tasks, distinct symbols first; one broken file serves one class."""
    tasks, rejects, used, taken = [], {}, set(), {}
    cands = candidates(prog, singles, seed, multi_tries, erased)
    for cls in CLASSES:
        for key in [k for k in cands if k[0] == cls]:
            knobs = [key[1]] if key[1] is not None else ["1", "2"]  # caller: depth known after checks

            def full():
                return all(taken.get((cls, k), 0) >= per_cell for k in knobs)
            for distinct in (True, False):
                for c in cands[key]:
                    if full():
                        break
                    if c.get("_done") or c["broken"] in used:
                        continue
                    syms = {t["symbol"] for t in tasks if t["class"] == cls and t["knob"] in knobs}
                    if distinct and c["fn"] in syms:
                        continue
                    c["_done"] = True
                    t, why = finish(h, prog, cls, c, seed)
                    if t is None:
                        why = why if why.startswith(cls + ":") else f"{cls}: {why}"
                        rejects[why] = rejects.get(why, 0) + 1
                        continue
                    cell = (cls, t["knob"])
                    if taken.get(cell, 0) >= per_cell:
                        continue
                    taken[cell] = taken.get(cell, 0) + 1
                    used.add(c["broken"])
                    tasks.append(t)
    return tasks, rejects


def programs_for(stratum, programs_dir):
    out = []
    if stratum in ("D", "all"):
        out += [(os.path.join(BENCH, p), "D") for p in STRATUM_D]
    if stratum in ("C", "all") and programs_dir and os.path.isdir(programs_dir):
        out += [(p, "C") for p in sorted(glob.glob(os.path.join(programs_dir, "**", "*.ax"), recursive=True))]
    return out


def used_by_sets():
    used = set()
    for s in USED_SETS:
        p = os.path.join(BENCH, s)
        if os.path.exists(p):
            used |= {t["source_file"] for t in json.load(open(p))["tasks"]}
    return used


def bundle_programs(manifest, progs_in):
    """[(path, "X", target, target stratum)] for every bundle of the bundle.py
    manifest that reached its floor (hx3 HX3-B); the manifest must pass
    `bundle.check_manifest` and every target must be an input program."""
    import bundle
    bad = bundle.check_manifest(manifest, ROOT)
    if bad:
        raise SystemExit(f"--bundles {manifest}: {bad[:3]}")
    stratum_of = {os.path.relpath(p, ROOT): s for p, s in progs_in}
    out = []
    for b in json.load(open(manifest))["bundles"]:
        if not b["reached"]:
            continue
        if b["program"] not in stratum_of:
            raise SystemExit(f"--bundles: target {b['program']} is not an input program")
        out.append((os.path.join(os.path.dirname(os.path.abspath(manifest)), b["path"]), "X",
                    b["program"], stratum_of[b["program"]]))
    return out


def build(a):
    t0 = time.time()
    h = Harness(a.axon, a.cortex, a.test_timeout)
    progs_in = programs_for(a.stratum, a.programs_dir)
    clash = {os.path.relpath(p, ROOT) for p, _ in progs_in} & used_by_sets()
    if clash:
        raise SystemExit(f"programs already used by a task set: {sorted(clash)}")
    if a.bundles:
        progs_in += bundle_programs(a.bundles, progs_in)
    with cf.ThreadPoolExecutor(a.jobs) as ex:
        progs = list(ex.map(lambda ps: Program(h, *ps), progs_in))
    targets = {p.source_file: p.target for p in progs if p.stratum == "X"}
    report = {"programs": {}, "binaries": h.shas}
    live = [p for p in progs if not p.reject]
    for p in progs:
        if p.reject:
            report["programs"][p.source_file] = {"stratum": p.stratum, "rejected": p.reject}
    units = [(p, f) for p in live for f in p.fns]
    with cf.ThreadPoolExecutor(a.jobs) as ex:
        res = list(ex.map(lambda u: (singles_of(h, u[0], u[1], a.seed, a.max_singles), erase_of(h, *u)), units))
    singles, erased = {}, {}
    for (p, f), (ss, e) in zip(units, res):
        singles.setdefault(p.source_file, {})[f] = ss
        if e:
            erased.setdefault(p.source_file, {})[f] = e
    t1 = time.time()
    with cf.ThreadPoolExecutor(a.jobs) as ex:
        built = list(ex.map(lambda p: build_program(h, p, singles.get(p.source_file, {}), a.seed,
                                                    a.per_cell_program, a.multi_tries,
                                                    erased.get(p.source_file)), live))
    by_prog, rejects = {}, {}
    for p, (tasks, rej) in zip(live, built):
        by_prog[p.source_file] = tasks
        report["programs"][p.source_file] = {
            "stratum": p.stratum, "functions": len(p.fns),
            "singles": sum(len(v) for v in singles.get(p.source_file, {}).values()),
            "tasks": len(tasks), "by_class": {c: sum(t["class"] == c for t in tasks) for c in CLASSES},
            "rejections": rej}
        for k, v in rej.items():
            rejects[k] = rejects.get(k, 0) + v
    kept, dropped = drop_thin(by_prog, cluster_of=targets)
    for p in dropped:
        report["programs"][p]["dropped"] = f"its cluster has fewer than {MIN_TASKS_PER_PROGRAM} valid tasks"
    prior = None
    if a.prior_split:
        prior = json.load(open(a.prior_split))["meta"]["headroom"]["split"]
    deal = "dev" if a.deal_dev else "seeded"
    split = assign_split(sorted(kept), a.split_seed, prior, targets, deal)
    own = {p: v for p, v in split.items() if p not in targets}  # split_ok counts real programs only
    tasks = []
    for p in sorted(kept):
        for t in sorted(kept[p], key=lambda t: t["id"]):
            t["split"] = split[p]
            tasks.append(t)
    ids = [t["id"] for t in tasks]
    assert len(ids) == len(set(ids)), "duplicate task ids"
    report.update({
        "wall_s": {"total": round(time.time() - t0, 1), "singles": round(t1 - t0, 1),
                   "tasks": round(time.time() - t1, 1)},
        "yield": {"tasks": len(tasks), "programs": len(kept), "dropped_programs": len(dropped),
                  "by_class": {c: sum(t["class"] == c for t in tasks) for c in CLASSES},
                  "by_cell": {f"{c}:{k}": n for (c, k), n in sorted(
                      _count((t["class"], t["knob"]) for t in tasks).items())},
                  "by_stratum": _count(t["stratum"] for t in tasks)},
        "rejections": dict(sorted(rejects.items())),
        "split": split, "split_ok": split_ok(own),
    })
    recorded = recorded_split(prior, split)
    headroom = {**h.shas, "seed": a.seed, "split_seed": a.split_seed, "deal": deal, "split": recorded,
                "split_ok": split_ok(own), "stratum": a.stratum,
                "per_cell_program": a.per_cell_program, "multi_tries": a.multi_tries,
                "max_singles": a.max_singles, "inputs_digest": inputs_digest(a.programs_dir),
                "programs": {p: report["programs"][p]["stratum"] for p in sorted(kept)},
                "dropped": sorted(dropped),
                "rejected": {p.source_file: p.reject for p in progs if p.reject}}
    if a.bundles:
        headroom["bundles"] = {"manifest": os.path.relpath(os.path.abspath(a.bundles), ROOT),
                               "sha256": sha256_file(a.bundles), "targets": dict(sorted(targets.items()))}
    refuse = None
    if not split_ok(own) and not a.allow_short_split:
        refuse = (f"split has {sum(v == 'dev' for v in own.values())} dev / "
                  f"{sum(v == 'test' for v in own.values())} test programs, below {MIN_DEV} / {MIN_TEST} "
                  "(FU-1c step 1); no pool written (--allow-short-split for a scratch build)")
        report["refused"] = refuse
    elif tasks:
        write_corpus(h, a.out, tasks, sorted(kept), headroom)
        report["out_sha256"] = sha256_file(a.out)
    if a.report:
        with open(a.report, "w") as f:
            json.dump(report, f, indent=1)
    print(json.dumps({k: report[k] for k in ("wall_s", "yield", "split_ok")}, indent=1), file=sys.stderr)
    if refuse:
        print("hx build: REFUSED: " + refuse, file=sys.stderr)
        return 1
    return 0


def _count(it):
    out = {}
    for x in it:
        out[x] = out.get(x, 0) + 1
    return out


def write_corpus(h, out, tasks, files, headroom):
    part = tempfile.mktemp(prefix="hx-pool-part-", suffix=".json")
    with open(part, "w") as f:
        json.dump({"meta": {"set": "hx-pool", "files": files}, "tasks": tasks}, f)
    try:
        h.corpus.merge(out, [part])
    finally:
        os.unlink(part)
    d = json.load(open(out))
    d["meta"]["set"] = "hx-pool"
    d["meta"]["headroom"] = headroom
    with open(out, "w") as f:
        json.dump(d, f, indent=1)
    h.corpus.load(out)


def pinned_cut(h, prog, t):
    """`--check`'s HX-1/HX-3b rule: (cut, None) or (None, reason). The pin is
    recomputed on the rebuilt pre-cut file: a caller task's hidden must be it
    (`caller_hidden_ok`); a sparse task's visible and hidden must be
    `sparse_cut`'s."""
    pre, why = rebuild(prog.src, t["symbol"], t["class"], t.get("ops") or [], t.get("edits") or [], t["how"],
                       prog.stratum, prog.target_stratum)
    if why:
        return None, why
    res = h.tests(prog.name, pre)
    if res is None:
        return None, f"{t['class']}: the pre-cut test run did not complete"
    failing = [n for n, (s, _) in res.items() if s == "failed"]
    direct, calls, reach = prog.pin_inputs(t["symbol"])
    if t["class"] == "caller":
        why = caller_hidden_ok(t, direct, failing, calls, reach)
        return (None, why) if why else ([n for n in direct if n != t["hidden"]], None)
    sc = sparse_cut(failing, direct, calls, reach)
    if sc is None or sc[:2] != (t["visible"], t["hidden"]):
        return None, f"sparse: visible/hidden {t['visible']}/{t['hidden']} are not sparse_cut's {sc and sc[:2]}"
    return sc[2], None


def check(a):
    """`--check CORPUS` (module docstring); exit 1 on any difference."""
    h = Harness(a.axon, a.cortex, a.test_timeout)
    raw = json.load(open(a.check))
    hm = raw["meta"].get("headroom", {})
    bad = []
    for k, v in h.shas.items():
        if hm.get(k) != v:
            bad.append(f"{k}: given binary {v[:12]} is not the recorded {str(hm.get(k))[:12]}")
    d = h.corpus.load(a.check)
    progs = {}

    def one(t):
        path = os.path.join(ROOT, t["source_file"])
        p = progs.get(t["source_file"])
        if p is None:
            if not os.path.exists(path) or open(path).read() != t["pristine"]:
                return t["id"], "the program file differs from the recorded source"
            if t["stratum"] == "X":
                p = Program(h, path, "X", t.get("target_program"), t.get("target_stratum"))
            else:
                p = Program(h, path, t["stratum"])
            progs[t["source_file"]] = p
            if p.reject:
                return t["id"], f"program: {p.reject}"
        why = bundle_split_reason(t, hm.get("split", {}))
        if why:
            return t["id"], why
        cand = {"fn": t["symbol"], "how": t["how"], "ops": t.get("ops") or [], "edits": t.get("edits") or [],
                "cut": []}
        if t["class"] in ("caller", "sparse"):
            cand["cut"], why = pinned_cut(h, p, t)
            if why:
                return t["id"], why
        why = singles_fail(h, p, t["symbol"], cand["edits"])
        if why:
            return t["id"], why
        t2, why = finish(h, p, t["class"], cand, t["seed"], t["visible"], t["hidden"])
        if t2 is None:
            return t["id"], why
        t2["split"] = t.get("split")
        diff = sorted(k for k in set(t) | set(t2) if k not in CHECK_IGNORED and t.get(k) != t2.get(k))
        return t["id"], (f"rebuilt task differs in {diff}" if diff else None)
    for t in d["tasks"]:  # sequential per program: Program() is cached
        tid, why = one(t)
        if why:
            bad.append(f"{tid}: {why}")
    for b in bad:
        print("FAIL " + b, file=sys.stderr)
    print(f"hx check: {len(d['tasks'])} tasks, {len(bad)} failures", file=sys.stderr)
    return 1 if bad else 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--axon")
    ap.add_argument("--cortex")
    ap.add_argument("--out")
    ap.add_argument("--check", metavar="CORPUS")
    ap.add_argument("--inputs-digest", action="store_true")
    ap.add_argument("--stratum", choices=("D", "C", "all"), default="all")
    ap.add_argument("--programs-dir", default=os.path.join(HERE, "programs"))
    ap.add_argument("--seed", type=int, help="mutation-candidate order seed")
    ap.add_argument("--split-seed", type=int, help="program split seed")
    ap.add_argument("--prior-split", metavar="POOL", help="an earlier pool whose program split is inherited")
    ap.add_argument("--deal-dev", action="store_true",
                    help="deal every program not in --prior-split to dev (hx3 HX3-C authored programs)")
    ap.add_argument("--bundles", metavar="MANIFEST", help="bundle.py manifest: bundle programs for class excerpt")
    ap.add_argument("--per-cell-program", type=int, default=2)
    ap.add_argument("--multi-tries", type=int, default=3)
    ap.add_argument("--max-singles", type=int, default=60)
    ap.add_argument("--test-timeout", type=int, default=30)
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--report")
    ap.add_argument("--allow-short-split", action="store_true",
                    help="write the pool even when the split misses >= 10 dev / >= 20 test programs "
                         "(scratch builds and extensions only)")
    a = ap.parse_args(argv)
    if a.inputs_digest:
        print(inputs_digest(a.programs_dir))
        return 0
    if not a.axon or not a.cortex:
        ap.error("--axon and --cortex are required (no default binary is guessed)")
    if a.check:
        return check(a)
    if not a.out or a.seed is None or a.split_seed is None:
        ap.error("--out, --seed and --split-seed are required to build")
    return build(a)


if __name__ == "__main__":
    sys.exit(main())
