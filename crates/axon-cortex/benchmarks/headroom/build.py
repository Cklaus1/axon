#!/usr/bin/env python3
"""Build the hx task pool (spec FU-1a, FU-1b, FU-1c step 1).

    python3 build.py --axon AXON --cortex CORTEX --out hx-pool.json \\
        --seed N --split-seed N [--stratum D|C|all] [--programs-dir DIR] \\
        [--prior-split OLD-POOL.json] [--report REPORT.json] [--jobs N]
    python3 build.py --check CORPUS.json --axon AXON --cortex CORTEX
    python3 build.py --inputs-digest

Both binaries are REQUIRED arguments (no default path is guessed); their
sha256 is recorded in `meta.headroom` and `--check` refuses other binaries.

SOURCES (FU-1b). Stratum D: `STRATUM_D`, the 17 hardset snapshots no hard
set uses plus the tempt snapshot `traits_demo.ax`. Stratum C: every `*.ax`
under `--programs-dir` (default `programs/` beside this file). A program is
used only if it type-checks, has >= 2 tests and every test passes.

CLASSES (FU-1a). Every task is one function body (`symbol`) whose pristine
body repairs it:

  multi   2 or 3 disjoint mutations in one body (knob "2"/"3"); each one alone
          type-checks and fails >= 1 pristine test
  caller  a mutation of callee `g` with g's own tests (the test blocks that
          call g directly) cut from the broken file, so the corpus stores
          `broken_source`; the visible failure text does not name g; knob =
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

Build checks on every task: the broken file is `axon check`-clean and its
check output names no E1201; ../ab/corpus.py `task_from_broken` validity (>= 2
tests fail, the arm-A workspace, the hidden check passes on pristine);
putting back the pristine body, with the pristine test blocks (grading's
view), passes every pristine test; `cortex locate` (the arm-B call) ranks the
symbol within run_ab.py's `--candidates` and lists it in `reached_by_all`.

SELECTION. Candidate order is a seeded shuffle (`--seed`); per program and
cell (class, knob) at most `--per-cell-program` tasks, distinct symbols first;
one broken file serves one class only. Programs with fewer than 3 valid
tasks are dropped. Task fields beyond corpus.load's: `class, knob, stratum,
program, caps_strict: true, seed, split`.

SPLIT (FU-1c step 1). By program: `split_programs`. Programs named in
`--prior-split` (an earlier pool) keep their split; new programs are split by
a fresh seeded shuffle. The per-program cap on test (<= 8) is applied by
calibrate.py after cell selection, with `cap_per_program`.

CHECK. `--check CORPUS` re-runs every task's construction checks on its
stored broken file with the given binaries (whose sha256 must equal the
recorded ones) and compares the rebuilt task with the stored one, ignoring
`spectrum_rank` (and the corpus-level `examples_commit`).
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
CLASSES = ("refine", "caps", "caller", "subtle", "multi")
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


def split_programs(programs, seed, prior=None):
    """{program: "dev"|"test"}. Programs in `prior` keep their split. The rest
    are sorted, shuffled with `seed`, and the first max(10, n // 3) go to dev
    (with no prior), or round(n / 3) of them (when extending a prior split),
    the remainder to test."""
    prior = prior or {}
    out = {p: prior[p] for p in programs if p in prior}
    new = sorted(p for p in set(programs) if p not in prior)
    random.Random(seed).shuffle(new)
    n_dev = min(len(new), max(MIN_DEV, len(new) // 3)) if not prior else round(len(new) / 3)
    for i, p in enumerate(new):
        out[p] = "dev" if i < n_dev else "test"
    return dict(sorted(out.items()))


def split_ok(split):
    n_dev = sum(v == "dev" for v in split.values())
    return n_dev >= MIN_DEV and len(split) - n_dev >= MIN_TEST


def cap_per_program(tasks, cap, seed):
    """At most `cap` tasks per program; a larger program keeps a seeded draw
    (random.Random(f"{seed}:{program}").sample over its sorted ids)."""
    by = {}
    for t in tasks:
        by.setdefault(t["program"], []).append(t["id"])
    keep = set()
    for p, ids in by.items():
        ids = sorted(ids)
        keep |= set(ids if len(ids) <= cap else random.Random(f"{seed}:{p}").sample(ids, cap))
    return [t for t in tasks if t["id"] in keep]


def drop_thin(by_program, min_tasks=MIN_TASKS_PER_PROGRAM):
    kept = {p: v for p, v in by_program.items() if len(v) >= min_tasks}
    return kept, {p: v for p, v in by_program.items() if len(v) < min_tasks}


def inputs_digest():
    """sha256 over the builder inputs (FU-1e): this directory's code and
    programs, ../ab/corpus.py, ../common.py, ../heldout/mutators.py. The
    built sets, PREREG.md and README.md are outputs, not inputs."""
    files = sorted(glob.glob(os.path.join(HERE, "*.py")) +
                   [p for p in glob.glob(os.path.join(HERE, "programs", "**"), recursive=True)
                    if os.path.isfile(p)] +
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
    def __init__(self, h, path, stratum):
        self.path = path
        self.stratum = stratum
        self.source_file = os.path.relpath(path, ROOT)
        self.name = os.path.basename(path)
        self.src = open(path).read()
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
        defs = h.common.defined_fns(self.src)
        self.fns = [f for f in dict.fromkeys(defs) if defs.count(f) == 1 and f not in self.tests
                    and f != "main" and _span(self.src, f) and self.src[slice(*_span(self.src, f))].strip()]
        self.graph = call_graph(self.src, list(dict.fromkeys(defs)))
        self.refine = bool(re.search(r"@\[verify|\bwhere\b", self.src))
        pc = h.caps(self.name, self.src) if not self.reject else None
        self.wide = bool(pc and any(WIDE_KINDS.match(k) for k in pc.get("capabilities", [])))
        blocks = h.corpus.test_blocks(self.src)
        self.direct_tests = {f: [n for n, a, b in blocks
                                 if re.search(r"\b%s\s*\(" % re.escape(f), self.src[a:b])] for f in self.fns}
        self.caps_ok = {f: self.caps_eligible(h, f) is None for f in self.fns} if not self.reject else {}

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
    a, b = _span(prog.src, fn)
    body = prog.src[a:b]
    ctx = M.context(prog.src, fn)
    cands = list(dict.fromkeys((op, mut, how) for op, mut, how in M.sites(body, M.ALL_OPS, ctx, MAX_SITES)))
    random.Random(f"{seed}:{prog.source_file}:{fn}").shuffle(cands)
    out = []
    for op, mut, how in cands[:max_singles]:
        s = eval_single(h, prog, fn, op, mut, how)
        if s:
            out.append(s)
    return out


def candidates(prog, singles, seed, multi_tries):
    """{(class, knob): [candidate]} in seeded order; a candidate is the
    construction a task would be built from (`finish` validates it)."""
    out = {}
    subtle_ops = {n for n, _ in M.SUBTLE_OPS}
    for fn, ss in singles.items():
        for s in ss:
            base = {"fn": fn, "how": s["how"], "broken": s["broken"], "ops": [s["op"]]}
            if s["op"] in subtle_ops and len(s["failing"]) >= 2:
                out.setdefault(("subtle", s["op"]), []).append(dict(base))
            if prog.refine and len(s["failing"]) >= 2:
                vis = next((n for n in s["failing"] if REFINE_RE.search(s["messages"][n])), None)
                if vis:
                    knob = "verify" if "verify failed in" in s["messages"][vis] else "refine"
                    out.setdefault(("refine", knob), []).append(dict(base, visible=vis))
            if prog.caps_ok.get(fn) and len(s["failing"]) >= 2:
                out.setdefault(("caps", "-"), []).append(dict(base))
            cut = prog.direct_tests[fn]
            if cut:
                out.setdefault(("caller", None), []).append(dict(base, cut=cut))
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
                     "ops": [ss[i]["op"] for i in sorted(pick)]})
                made += 1
                if made >= multi_tries:
                    break
    for k, v in out.items():
        random.Random(f"{seed}:{prog.source_file}:{k}").shuffle(v)
    return out


def cut_tests(h, text, names):
    for n in names:
        text, _ = h.corpus.cut_test(text, n)
    return text


def graded_view(h, final, pristine):
    """run_ab grading's file: the final file's test blocks removed, the
    pristine blocks appended."""
    for n, a, b in reversed(h.corpus.test_blocks(final)):
        final = final[:a] + final[b:]
    blocks = "".join(pristine[a:b] for _, a, b in h.corpus.test_blocks(pristine))
    return final.rstrip("\n") + "\n\n" + blocks


def finish(h, prog, cls, cand, seed, visible=None, hidden=None):
    """(task, None) or (None, reason): every build check of FU-1a. The knob
    is derived here (from the ops, the call depth or the failure text), so
    `--check` recomputes it rather than trusting the stored one."""
    ops = list(cand.get("ops") or [])
    if not ops:
        return None, f"{cls}: no mutation operators recorded"
    knob = {"multi": str(len(ops)), "subtle": ops[0], "caps": "-"}.get(cls)
    if (cls == "multi") != (len(ops) > 1) or (cls == "subtle" and ops[0] not in {n for n, _ in M.SUBTLE_OPS}):
        return None, f"{cls}: mutation operators {ops} do not fit the class"
    fn, broken = cand["fn"], cand["broken"]
    if cls == "caller":
        broken = cut_tests(h, broken, cand["cut"])
    clean, _ = h.check(prog.name, broken)
    if not clean:
        return None, "broken does not type-check (or names E1201)"
    res = h.tests(prog.name, broken)
    if res is None:
        return None, "broken test run did not complete"
    failing = [n for n, (s, _) in res.items() if s == "failed"]
    visible = visible or cand.get("visible")
    if cls == "caller":
        if len(failing) < 2:
            return None, "caller: fewer than 2 failing tests once g's tests are cut"
        visible = visible or failing[0]
        if visible not in failing:
            return None, "caller: visible does not fail"
        if re.search(r"\b%s\b" % re.escape(fn), res[visible][1]):
            return None, "caller: the visible failure text names g"
        depth = call_depth(prog.graph, visible, fn)
        if depth is None or depth < 2:
            return None, "caller: no static call path from the visible test to g"
        knob = "1" if depth == 2 else "2"
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
    t = h.corpus.task_from_broken(prog.path, prog.src, fn, cls, cand["how"], broken, visible, hidden)
    if t is None:
        return None, "corpus.task_from_broken validity"
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
    t.update(id=task_id(cls, prog.source_file, t["broken"]), knob=knob, ops=ops, stratum=prog.stratum,
             program=prog.source_file, caps_strict=True, seed=seed, spectrum_rank=rank)
    if cls == "caller":
        t["cut_tests"] = list(cand["cut"])
        t["call_depth"] = depth - 1
    return t, None


def build_program(h, prog, singles, seed, per_cell, multi_tries):
    """(tasks, rejections) for one program: per cell at most `per_cell`
    tasks, distinct symbols first; one broken file serves one class."""
    tasks, rejects, used, taken = [], {}, set(), {}
    cands = candidates(prog, singles, seed, multi_tries)
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


def build(a):
    t0 = time.time()
    h = Harness(a.axon, a.cortex, a.test_timeout)
    progs_in = programs_for(a.stratum, a.programs_dir)
    clash = {os.path.relpath(p, ROOT) for p, _ in progs_in} & used_by_sets()
    if clash:
        raise SystemExit(f"programs already used by a task set: {sorted(clash)}")
    with cf.ThreadPoolExecutor(a.jobs) as ex:
        progs = list(ex.map(lambda ps: Program(h, *ps), progs_in))
    report = {"programs": {}, "binaries": h.shas}
    live = [p for p in progs if not p.reject]
    for p in progs:
        if p.reject:
            report["programs"][p.source_file] = {"stratum": p.stratum, "rejected": p.reject}
    units = [(p, f) for p in live for f in p.fns]
    with cf.ThreadPoolExecutor(a.jobs) as ex:
        res = list(ex.map(lambda u: singles_of(h, u[0], u[1], a.seed, a.max_singles), units))
    singles = {}
    for (p, f), ss in zip(units, res):
        singles.setdefault(p.source_file, {})[f] = ss
    t1 = time.time()
    with cf.ThreadPoolExecutor(a.jobs) as ex:
        built = list(ex.map(lambda p: build_program(h, p, singles.get(p.source_file, {}), a.seed,
                                                    a.per_cell_program, a.multi_tries), live))
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
    kept, dropped = drop_thin(by_prog)
    for p in dropped:
        report["programs"][p]["dropped"] = f"fewer than {MIN_TASKS_PER_PROGRAM} valid tasks"
    prior = None
    if a.prior_split:
        prior = json.load(open(a.prior_split))["meta"]["headroom"]["split"]
    split = split_programs(sorted(kept), a.split_seed, prior)
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
        "split": split, "split_ok": split_ok(split),
    })
    headroom = {**h.shas, "seed": a.seed, "split_seed": a.split_seed, "split": split,
                "split_ok": split_ok(split), "stratum": a.stratum,
                "per_cell_program": a.per_cell_program, "multi_tries": a.multi_tries,
                "max_singles": a.max_singles, "inputs_digest": inputs_digest(),
                "programs": {p: report["programs"][p]["stratum"] for p in sorted(kept)},
                "dropped": sorted(dropped),
                "rejected": {p.source_file: p.reject for p in progs if p.reject}}
    if tasks:
        write_corpus(h, a.out, tasks, sorted(kept), headroom)
        report["out_sha256"] = sha256_file(a.out)
    if a.report:
        with open(a.report, "w") as f:
            json.dump(report, f, indent=1)
    print(json.dumps({k: report[k] for k in ("wall_s", "yield", "split_ok")}, indent=1), file=sys.stderr)
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
            p = progs[t["source_file"]] = Program(h, path, t["stratum"])
            if p.reject:
                return t["id"], f"program: {p.reject}"
        cand = {"fn": t["symbol"], "how": t["how"], "broken": t["broken"], "ops": t.get("ops"),
                "cut": []}  # a caller task stores the broken file after the cut
        t2, why = finish(h, p, t["class"], cand, t["seed"], t["visible"], t["hidden"])
        if t2 is None:
            return t["id"], why
        if t["class"] == "caller":  # the cut is g's direct tests, recomputed
            t2["cut_tests"] = list(p.direct_tests.get(t["symbol"], []))
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
    ap.add_argument("--per-cell-program", type=int, default=2)
    ap.add_argument("--multi-tries", type=int, default=3)
    ap.add_argument("--max-singles", type=int, default=60)
    ap.add_argument("--test-timeout", type=int, default=30)
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--report")
    a = ap.parse_args(argv)
    if a.inputs_digest:
        print(inputs_digest())
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
