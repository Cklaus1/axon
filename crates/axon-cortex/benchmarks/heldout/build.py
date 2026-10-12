#!/usr/bin/env python3
"""Build the two held-out evaluation sets: `heldout.json` and `once.json`.

    python3 build.py              # mine, pool, select, write both corpora
    python3 build.py --pool-only  # stop after the candidate pool (cached)

Both sets are in ../ab/corpus.json's compact format (written by corpus.py's
`merge`, read back by its `load`), so `run_ab.py` arm A and arm B run them
unchanged. Every task passed ../ab/corpus.py's `make_task` validity checks.

Stages, all deterministic (no randomness; same tree, same output):

1. REAL bugs: mine.py walks git history. Every qualifying task goes to the
   held-out set with `origin: "real"`.
2. POOL: for every pool file (POOL_FILES below), every non-test function
   other than `main` whose body is NOT a copy of a function in a dev-corpus
   source (whitespace-normalised comparison), and every class -- the dev
   corpus's six and mutators.py's eight new ones, all site-aware -- every site
   (in body order, up to MAX_SITES) whose mutant passes `make_task`.
3. SELECT: the yielding pool files are split between the two sets
   (HELDOUT_FILES / ONCE_FILES, disjoint; files that share function bodies
   are on the same side; the split keeps the exposure mix of the two sets
   alike). Within a set, tasks are taken round robin over classes in
   CLASS_ORDER, preferring the file this class has used least, then the
   function used least, then file name, source and site order; at most
   PER_FILE_CLASS per (file, class) and PER_SYMBOL per function; a task whose
   function body occurs in the other set's files, whose (original, mutant)
   body pair either set already holds, or whose defective function `cortex
   locate` does not rank within run_ab.py's `--candidates` (unreachable for
   any Cortex configuration; mine.py rule 7), is skipped. Until QUOTA or the
   pool runs dry. `spectrum_rank` records that rank on every task.

The pool cache lives under $TMPDIR (it holds full file texts) and is keyed by
the sha256 of the axon binary, the pool files, the dev corpus and the code
that decides the pool, so a stale pool is never reused.
"""
import concurrent.futures as cf
import hashlib
import inspect
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, BENCH)
sys.path.insert(0, os.path.join(BENCH, "ab"))
import corpus  # noqa: E402
import mine  # noqa: E402
from common import AXON, ROOT, defined_fns, fn_span, tests_in  # noqa: E402
from mine import locate_rank  # noqa: E402
from mutators import DEV_CLASS_MUTATORS, NEW_MUTATORS, context  # noqa: E402
from run_ab import CANDIDATES  # noqa: E402

# Pool files: every tracked `.ax` file outside the dev corpus whose tests all
# pass at HEAD and which has >= 2 `@[test]` functions and >= 1 non-test
# function, MINUS files owned by Cortex itself (crates/axon-cortex/fixtures,
# examples/cortex: Cortex's own tests and demo) and harness-semantics fixtures
# (crates/axon-core/tests/fixtures/r51_s0b: `exit(0)` passes a test there).
# EXPOSURE is what any earlier benchmark did with the file (README.md):
#   none        never named by a benchmark
#   decisions   crates/axon-reflex/benchmarks/decisions builds type-check
#               (axbool) items from it; no repair task, no Cortex call
#   loc-0920    localization-2026-09-20.json planted the ../common.py classes
#               in it; Cortex's locator was measured on those mutants
POOL_FILES = {
    "examples/asi/budget_audit.ax": "none",
    "examples/asi/rank.ax": "none",
    "examples/asi/risk_monitor.ax": "none",
    "examples/asi/safety_stack_demo.ax": "none",
    "examples/asi/taint_flow.ax": "none",
    "examples/fintech/ledger.ax": "none",
    "examples/modular/scorelib.ax": "none",
    "examples/stdlib/asi_prelude.ax": "none",
    "examples/feature_tour.ax": "decisions",
    "examples/should_fail_test.ax": "decisions",
    "examples/stdlib/arrays.ax": "decisions",
    "examples/stdlib/constraint.ax": "decisions",
    "examples/stdlib/principal.ax": "decisions",
    "examples/stdlib/reward.ax": "decisions",
    "examples/stdlib/safe_action.ax": "decisions",
    "examples/stdlib/uncertain.ax": "decisions",
    "examples/stdlib/plan.ax": "loc-0920",
    "examples/stdlib/principal_mint.ax": "loc-0920",
    "examples/stdlib/source.ax": "loc-0920",
    "examples/stdlib/tainted.ax": "loc-0920",
    "examples/stdlib/trace.ax": "loc-0920",
    "examples/stdlib/world.ax": "loc-0920",
}
# Pool files where no planted task survived make_task are in neither set
# (README.md lists them with the reason).
HELDOUT_FILES = {
    "examples/asi/budget_audit.ax",
    "examples/asi/taint_flow.ax",
    "examples/stdlib/constraint.ax",
    "examples/stdlib/plan.ax",
    "examples/stdlib/principal.ax",
    "examples/stdlib/principal_mint.ax",
    "examples/stdlib/reward.ax",
    "examples/stdlib/tainted.ax",
    "examples/stdlib/trace.ax",
}
ONCE_FILES = {
    "examples/asi/rank.ax",
    "examples/asi/safety_stack_demo.ax",
    "examples/stdlib/asi_prelude.ax",
    "examples/stdlib/safe_action.ax",
    "examples/stdlib/source.ax",
    "examples/stdlib/world.ax",
}
assert not HELDOUT_FILES & ONCE_FILES and (HELDOUT_FILES | ONCE_FILES) <= set(POOL_FILES)

DEV_CLASSES = [m for m, _ in DEV_CLASS_MUTATORS]
NEW_CLASSES = [m for m, _ in NEW_MUTATORS]
MUTATORS = dict(DEV_CLASS_MUTATORS + NEW_MUTATORS)
CLASS_ORDER = NEW_CLASSES + DEV_CLASSES
MAX_SITES = 16
QUOTA = {"heldout": 50, "once": 30}
PER_FILE_CLASS = 3
PER_SYMBOL = 4


def sha256(b):
    return hashlib.sha256(b).hexdigest()


def candidate(job):
    """Every site (in body order, up to MAX_SITES) whose mutant passes make_task."""
    rel, src0, name, mname = job
    sp = fn_span(src0, name)
    body = src0[sp[0]:sp[1]]
    path = os.path.join(ROOT, rel)
    mfn, ctx = MUTATORS[mname], context(src0, name)
    out = []
    for k in range(MAX_SITES):
        made = mfn(body, k, ctx)
        if made is None:
            break
        t = corpus.make_task(path, src0, name, mname, lambda _b, made=made: made)
        if t:
            out.append(dict(t, site=k))
    return out


def norm(s):
    return re.sub(r"\s+", " ", s).strip()


def fn_bodies(src):
    """{name: whitespace-normalised body} for every function in `src`."""
    out = {}
    for n in defined_fns(src):
        sp = fn_span(src, n)
        if sp:
            out[n] = norm(src[sp[0]:sp[1]])
    return out


def dev_bodies():
    """Every function body in a dev-corpus source. Several pool files carry
    byte-for-byte copies of dev-corpus functions (phase14_tests.ax copies
    stdlib, principal_mint.ax copies budget.ax); a defect planted in such a
    copy is code Cortex was already tuned on, so it is never a candidate."""
    srcs = json.load(open(mine.DEV_CORPUS))["meta"]["sources"].values()
    return {b for s in srcs for b in fn_bodies(s).values()}


def pool(files):
    # keyed by everything that decides the pool, and nothing that only selects
    logic = "".join(inspect.getsource(f) for f in (candidate, pool, fn_bodies, dev_bodies, norm))
    key = sha256(open(AXON, "rb").read() + b"".join(
        open(os.path.join(ROOT, f), "rb").read() for f in sorted(files))
        + f"{logic}{MAX_SITES}{CLASS_ORDER}".encode()
        + b"".join(open(os.path.join(d, f), "rb").read() for d, f in (
            (HERE, "mutators.py"), (BENCH, "common.py"),
            (os.path.join(BENCH, "ab"), "corpus.py"), (os.path.join(BENCH, "ab"), "corpus.json"))))[:16]
    cache = os.path.join(tempfile.gettempdir(), f"cortex-heldout-pool-{key}.json")
    if os.path.exists(cache):
        print(f"pool: cached {cache}", file=sys.stderr)
        return json.load(open(cache))
    seen = dev_bodies()
    jobs, skipped = [], []
    for rel in sorted(files):
        src0 = open(os.path.join(ROOT, rel)).read()
        checks = set(tests_in(src0))
        bodies = fn_bodies(src0)
        for name in [n for n in defined_fns(src0) if n not in checks and n != "main"]:
            if name not in bodies:
                continue
            if bodies[name] in seen:
                skipped.append(f"{rel}::{name}")
                continue
            jobs += [(rel, src0, name, m) for m in CLASS_ORDER]
    print(f"pool: {len(jobs)} (file, function, class) candidates; {len(skipped)} functions "
          f"skipped as copies of dev-corpus code", file=sys.stderr, flush=True)
    with cf.ThreadPoolExecutor(int(os.environ.get("POOL_JOBS", "6"))) as ex:
        res = [t for ts in ex.map(candidate, jobs) for t in ts]
    json.dump(res, open(cache, "w"))
    print(f"pool: {len(res)} valid tasks -> {cache}", file=sys.stderr)
    return res


def eligible(files):
    """Pool-file preconditions, re-checked rather than assumed."""
    for rel in files:
        p = os.path.join(ROOT, rel)
        src0 = open(p).read()
        if rel in json.load(open(mine.DEV_CORPUS))["meta"]["sources"]:
            raise SystemExit(f"{rel} is a dev-corpus source")
        if len(tests_in(src0)) < 2:
            raise SystemExit(f"{rel} has fewer than two @[test] functions")
        r = corpus.test_results(p)
        if r is None or any(s != "ok" for s in r.values()):
            raise SystemExit(f"{rel} is not green at HEAD")


def select(cands, files, quota, prefix, other_files, seen):
    """Round robin over classes; see the module docstring.

    Skips a candidate whose function body also occurs in a file of the OTHER
    set (the sets are disjoint in code, not only in file names), one whose
    (original body, mutant body) pair was already taken by either set (two
    identical functions planted identically are one task, not two), and one
    whose true symbol `cortex locate` does not rank within run_ab.py's
    `--candidates` (arm B's oracle could never reach it, so the B-oracle dry
    run could not verify it -- the dev corpus has the same property: true
    symbol ranked 1-6 on all 60). `seen` is shared between the two calls.
    """
    order = {f: i for i, f in enumerate(sorted(files))}
    other = {b for f in other_files for b in fn_bodies(open(os.path.join(ROOT, f)).read()).values()}
    src_order = {}
    for t in cands:
        src_order.setdefault(t["source_file"], defined_fns(t["pristine"]))
    queues = {c: [t for t in cands if t["class"] == c and t["source_file"] in files
                  and norm(t["oracle_body"]) not in other]
              for c in CLASS_ORDER}
    taken, per_fc, per_sym = [], {}, {}
    progress = True
    while len(taken) < quota and progress:
        progress = False
        for c in CLASS_ORDER:
            if len(taken) >= quota:
                break
            q = queues[c]
            # the file this class has used least, then the function used
            # least, then file name, source and site order
            q.sort(key=lambda t: (per_fc.get((t["source_file"], c), 0),
                                  per_sym.get((t["source_file"], t["symbol"]), 0),
                                  order[t["source_file"]],
                                  src_order[t["source_file"]].index(t["symbol"]), t["site"]))
            for i, t in enumerate(q):
                fc, sym = (t["source_file"], c), (t["source_file"], t["symbol"])
                a, b = fn_span(t["broken"], t["symbol"])
                key = (norm(t["oracle_body"]), norm(t["broken"][a:b]))
                if key in seen or per_fc.get(fc, 0) >= PER_FILE_CLASS \
                        or per_sym.get(sym, 0) >= PER_SYMBOL:
                    continue
                t.setdefault("spectrum_rank", locate_rank(t))
                if not 1 <= t["spectrum_rank"] <= int(CANDIDATES):
                    continue
                seen.add(key)
                per_fc[fc] = per_fc.get(fc, 0) + 1
                per_sym[sym] = per_sym.get(sym, 0) + 1
                taken.append(q.pop(i))
                progress = True
                break
    counts = {}
    out = []
    for t in taken:
        counts[t["class"]] = counts.get(t["class"], 0) + 1
        out.append(dict(t, id=f"{prefix}-{t['class']}-{counts[t['class']]:02d}"))
    return out


def write_set(name, tasks, files, mining):
    for t in tasks:
        t.setdefault("origin", "planted")
        # a real-bug file outside the planted pool was never classified
        t["exposure"] = POOL_FILES.get(t["source_file"], "unclassified")
    part = os.path.join(tempfile.gettempdir(), f"cortex-heldout-{name}-part.json")
    json.dump({"meta": {"set": name, "axon_bin": AXON, "files": sorted(files),
                        "quota": QUOTA[name], "per_file_class": PER_FILE_CLASS,
                        "per_symbol": PER_SYMBOL, "max_sites": MAX_SITES,
                        "class_order": CLASS_ORDER,
                        "real_bug_mining": mining},
               "tasks": tasks}, open(part, "w"))
    out = os.path.join(HERE, f"{name}.json")
    corpus.merge(out, [part])
    os.unlink(part)
    corpus.load(out)  # every derived text rebuilds to its recorded sha256
    print(f"{name}: {len(tasks)} tasks -> {out} sha256 {sha256(open(out, 'rb').read())}",
          file=sys.stderr)


def main():
    eligible(POOL_FILES)
    recs = mine.mine()
    mine.write_report(recs, os.path.join(HERE, "mining.json"))
    real = [dict(r["task"], origin="real", commit=r["commit"], id=f"h-real-{i + 1:02d}")
            for i, r in enumerate(x for x in recs if x["reason"] is None)]
    for t in real:
        if t["source_file"] in ONCE_FILES:
            raise SystemExit(f"real-bug file {t['source_file']} is a once-set file")
    mining = {"pairs": len(recs), "qualifying": len(real)}
    cands = pool(POOL_FILES)
    if "--pool-only" in sys.argv:
        return
    seen = set()
    held = real + select(cands, HELDOUT_FILES, QUOTA["heldout"] - len(real), "h", ONCE_FILES, seen)
    once = select(cands, ONCE_FILES, QUOTA["once"], "o", HELDOUT_FILES, seen)
    write_set("heldout", held, HELDOUT_FILES | {t["source_file"] for t in real}, mining)
    write_set("once", once, ONCE_FILES, mining)


if __name__ == "__main__":
    main()
