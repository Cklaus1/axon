#!/usr/bin/env python3
"""Build the hard sets: `hard-dev.json` and `hard-once.json`.

    python3 build.py                         # mine, validate every defect, write both sets
    python3 build.py --check [FILE.json ...] # validate (default: every defects/*.json),
                                             # write nothing; exit 1 if any spec FAILs

Inputs, all in this directory:

* `src/`: snapshot copies of the hard lane's `examples/**/*.ax` files, each the
  original bytes followed by appended `@[test]` functions (committed alone,
  before any defect existed; README.md).
* `defects/*.json`: hand-written defects, `{"defects": [SPEC, ...]}`. A SPEC:

      source   path under src/
      symbol   the function whose pristine body is the oracle (in-scope: the
               only function the defect touches)
      scope    "in"  -- one function body, but finding the fix needs a fact
                        from OUTSIDE that body (`context` says which)
               "out" -- no single function-body edit can repair it
      shape    the defect shape (README.md lists them with counts)
      edits    [{"old": TEXT, "new": TEXT}, ...]: each `old` occurs exactly
               once in the snapshot, outside its test blocks; applied in order
      context  in: the outside fact the fix needs; out: why one body edit
               cannot repair it
      visible, hidden   optional test names (default: first / last failing)

Every spec is checked mechanically, and a spec that fails a check is
REPORTED and left out (never repaired here):

1. the edits apply as stated; the snapshot is green;
2. scope "in": the broken file equals the snapshot with only `symbol`'s body
   replaced, and `symbol` is neither `main` nor a test;
   scope "out": the broken file is NOT any single function-body replacement
   of the snapshot, and putting back the pristine body of ANY one function
   still leaves at least one test failing (no one-body oracle exists);
3. ../ab/corpus.py `task_from_broken` validity, the checks every dev and
   held-out task passed: the broken file type-checks, >= 2 tests fail, the
   arm-A workspace without the hidden check type-checks and fails the visible
   test, the hidden check passes on the pristine file;
4. scope "in": `cortex locate` ranks `symbol` within run_ab.py's
   `--candidates` on the broken file (arm B tries only those, so a defect
   outside them is unreachable for every Cortex configuration);
5. no two specs produce the same (file, broken file), and no two in-scope
   specs plant the same body defect (same pristine body, same broken body)
   in different files (files that carry copies of one function).

REAL bugs: every commit that modified a snapshot's `examples/` original
(`git log -M --follow`) is replayed against the snapshot with
../heldout/mine.py's rules, the snapshot's tests standing in for the missing
ones; `mining.json` records each pair with the first rule it breaks.

SPLIT (deterministic): files that host an out-of-scope task go to hard-dev,
with all their in-scope tasks; the remaining files, in sha256(path) order,
each go to the set with fewer in-scope tasks so far (ties to hard-dev).
Out-of-scope tasks are in hard-dev only. The two sets share no source file.
"""
import concurrent.futures as cf
import glob
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = os.path.dirname(HERE)
sys.path.insert(0, BENCH)
sys.path.insert(0, os.path.join(BENCH, "ab"))
sys.path.insert(0, os.path.join(BENCH, "heldout"))
import corpus  # noqa: E402
import mine  # noqa: E402
from common import AXON, ROOT, defined_fns, fn_span, run, tests_in  # noqa: E402
from run_ab import CANDIDATES  # noqa: E402

SRC = os.path.join(HERE, "src")
DEFECTS = os.path.join(HERE, "defects")
SCOPES = ("in", "out")
USAGE = __doc__.split("\n\n")[1]


def defect_files():
    return sorted(glob.glob(os.path.join(DEFECTS, "*.json")))


def sha256(b):
    return hashlib.sha256(b).hexdigest()


def rel(path):
    return os.path.relpath(path, ROOT)


def snapshots():
    return sorted(glob.glob(os.path.join(SRC, "**", "*.ax"), recursive=True))


def example_of(snap):
    """The `examples/` file a snapshot copies."""
    return os.path.join("examples", os.path.relpath(snap, SRC))


def outside_tests(src, start, end):
    return not any(a <= start and end <= b for _, a, b in corpus.test_blocks(src))


def all_green(path):
    r = corpus.test_results(path) if run([AXON, "check", path])[0] == 0 else None
    return r is not None and all(s == "ok" for s in r.values()), r


def results_of(text, name):
    ws = tempfile.mkdtemp(prefix="hardset_")
    try:
        p = os.path.join(ws, name)
        open(p, "w").write(text)
        return all_green(p)
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def single_body_owner(pristine, broken):
    """The function whose body alone turns `pristine` into `broken`, or None."""
    for n in dict.fromkeys(defined_fns(pristine)):
        a, b = fn_span(pristine, n)
        tail = len(pristine) - b
        if broken[:a] == pristine[:a] and broken[len(broken) - tail:] == pristine[b:] \
                and len(broken) - tail >= a:
            return n
    return None


def validate(spec):
    """(task, None) or (None, reason): the module docstring's checks 1-4."""
    path = os.path.join(SRC, spec["source"])
    if not os.path.exists(path):
        return None, f"no snapshot src/{spec['source']}"
    if spec.get("scope") not in SCOPES:
        return None, f"scope must be one of {SCOPES}"
    for k in ("symbol", "shape", "edits", "context"):
        if not spec.get(k):
            return None, f"missing `{k}`"
    src0 = open(path).read()
    tests = set(tests_in(src0))
    sym = spec["symbol"]
    if sym in tests or sym == "main" or sym not in defined_fns(src0):
        return None, f"symbol `{sym}` is not a non-test, non-main function of the file"
    broken = src0
    for e in spec["edits"]:
        if broken.count(e["old"]) != 1:
            return None, f"edit `old` occurs {broken.count(e['old'])} times, not once: {e['old']!r}"
        i = broken.index(e["old"])
        if not outside_tests(broken, i, i + len(e["old"])):
            return None, f"edit touches a test block: {e['old']!r}"
        broken = broken[:i] + e["new"] + broken[i + len(e["old"]):]
    if broken == src0:
        return None, "the edits change nothing"
    owner = single_body_owner(src0, broken)
    if spec["scope"] == "in" and owner != sym:
        return None, f"in-scope defect is not confined to `{sym}`'s body (owner: {owner})"
    if spec["scope"] == "out":
        if owner is not None:
            return None, f"out-of-scope defect is one body edit of `{owner}`"
        for n in dict.fromkeys(defined_fns(broken)):
            if n in tests or not fn_span(src0, n):
                continue
            a, b = fn_span(broken, n)
            pa, pb = fn_span(src0, n)
            green, _ = results_of(broken[:a] + src0[pa:pb] + broken[b:], os.path.basename(path))
            if green:
                return None, f"out-of-scope defect is repaired by `{n}`'s pristine body alone"
    t = corpus.task_from_broken(path, src0, sym, spec["shape"], f"hardset:{spec['scope']}",
                                broken, spec.get("visible"), spec.get("hidden"))
    if t is None:
        return None, ("fails corpus.py validity (type-check, >= 2 failing tests, named "
                      "visible/hidden failing, arm-A workspace, hidden ok on pristine)")
    t["spectrum_rank"] = mine.locate_rank(t)
    if spec["scope"] == "in" and not 1 <= t["spectrum_rank"] <= int(CANDIDATES):
        return None, f"`{sym}` ranked {t['spectrum_rank']} by cortex locate, outside the top {CANDIDATES}"
    t.update(scope=spec["scope"], shape=spec["shape"], context=spec["context"],
             origin="planted", example=example_of(path))
    return t, None


def load_specs(paths):
    specs = []
    for p in paths:
        for i, s in enumerate(json.load(open(p))["defects"]):
            specs.append(dict(s, _from=f"{os.path.basename(p)}#{i}"))
    return specs


def check_all(specs):
    """Validated specs in order; a spec that repeats an earlier one is
    rejected: the same broken file, or (in-scope) the same pristine body
    broken into the same body in another file (two identical functions planted
    identically are one task, not two -- ../heldout/build.py's rule)."""
    with cf.ThreadPoolExecutor(int(os.environ.get("HARDSET_JOBS", "6"))) as ex:
        res = list(ex.map(validate, specs))
    seen, ok, bad = set(), [], []
    for s, (t, why) in zip(specs, res):
        if t is not None:
            keys = {("file", t["source_file"], t["broken"])}
            if t["scope"] == "in":
                a, b = fn_span(t["broken"], t["symbol"])
                keys.add(("body", norm(t["oracle_body"]), norm(t["broken"][a:b])))
            if keys & seen:
                t, why = None, "duplicate of an earlier spec (same broken file or same body defect)"
            seen |= keys
        (ok if t is not None else bad).append((s, t if t is not None else why))
    return ok, bad


def norm(s):
    return re.sub(r"\s+", " ", s).strip()


def code_only(src):
    """`src` without `//` comments, blank lines and in-line whitespace runs:
    two files equal under it differ in layout or comments only. Line breaks
    stay significant (Axon inserts semicolons at them)."""
    lines = (re.sub(r"[ \t]+", " ", l).strip() for l in re.sub(r"//[^\n]*", "", src).split("\n"))
    return "\n".join(l for l in lines if l)


# ------------------------------------------------------------------ real bugs

def mine_real():
    """mine.py's rules, replayed on the snapshots (module docstring)."""
    recs, real = [], []
    for snap in snapshots():
        ex = example_of(snap)
        head = open(snap).read()
        log = mine.git("log", "-M", "--follow", "--format=%H", "--", ex).split()
        for c in log:
            ns = mine.git("show", "--format=", "-M", "--name-status", c, "--", ex).strip()
            if not ns.startswith(("M", "R")):
                recs.append({"commit": c[:12], "path": ex, "reason": "file added by the commit"})
                continue
            old = ns.split("\t")[1]
            pre, post = mine.git("show", f"{c}^:{old}"), mine.git("show", f"{c}:{ex}")
            rec = {"commit": c[:12], "path": ex,
                   "subject": mine.git("log", "-1", "--format=%s", c).strip()}
            names = list(dict.fromkeys(defined_fns(pre) + defined_fns(post)))
            changed = [n for n in names if mine.body(pre, n) != mine.body(post, n)]
            rec["changed_fns"] = changed
            fn = changed[0] if len(changed) == 1 else None
            a = fn and fn_span(pre, fn) and fn_span(post, fn)
            if code_only(pre) == code_only(post):
                rec["reason"] = "layout or comment change only (no code token changed)"
            elif not changed:
                rec["reason"] = "no function body changed (declarations only)"
            elif len(changed) > 1:
                rec["reason"] = "change spans more than one function"
            elif not a:
                rec["reason"] = "function added or removed by the commit"
            elif single_body_owner(pre, post) != fn:
                rec["reason"] = "code outside the function also changed"
            elif mine.body(head, fn) != mine.body(post, fn):
                rec["reason"] = "function body changed again after the fix"
            elif fn == "main" or fn not in [n for n in defined_fns(head) if n not in tests_in(head)]:
                rec["reason"] = f"the changed function `{fn}` is `main` or not a snapshot function"
            else:
                pre_body = mine.body(pre, fn)
                t = corpus.make_task(snap, head, fn, "real", lambda _b: (pre_body, f"revert:{c[:12]}"))
                if t is None:
                    rec["reason"] = ("reverting the fix in the snapshot fails corpus.py validity "
                                     "(compile, >= 2 failing tests, arm-A workspace)")
                else:
                    t["spectrum_rank"] = mine.locate_rank(t)
                    if not 1 <= t["spectrum_rank"] <= int(CANDIDATES):
                        rec["reason"] = f"fixed function outside cortex locate's top {CANDIDATES}"
                    else:
                        rec["reason"] = None
                        t.update(scope="in", shape="real", origin="real", example=ex,
                                 context=f"historical fix {c[:12]}: {rec['subject']}")
                        real.append(t)
            rec["symbol"] = fn
            recs.append(rec)
    return recs, real


# ------------------------------------------------------------------ sets

def split(tasks):
    files = sorted({t["source_file"] for t in tasks}, key=lambda f: sha256(f.encode()))
    dev = {t["source_file"] for t in tasks if t["scope"] == "out"}
    once = set()
    n_in = {f: sum(t["source_file"] == f and t["scope"] == "in" for t in tasks) for f in files}
    count = {"dev": sum(n_in[f] for f in dev), "once": 0}
    for f in files:
        if f in dev:
            continue
        side = "once" if count["once"] < count["dev"] else "dev"
        (once if side == "once" else dev).add(f)
        count[side] += n_in[f]
    return dev, once


def number(tasks, prefix):
    out, counts = [], {}
    for t in sorted(tasks, key=lambda t: (t["scope"], t["shape"], t["source_file"],
                                          t["symbol"], t["broken"])):
        k = (t["scope"], t["shape"])
        counts[k] = counts.get(k, 0) + 1
        tag = t["shape"] if t["scope"] == "in" else "out-" + t["shape"]
        out.append(dict(t, id=f"{prefix}-{tag}-{counts[k]:02d}"))
    return out


def write_set(name, tasks, files, mining):
    part = os.path.join(tempfile.gettempdir(), f"cortex-hardset-{name}-part.json")
    json.dump({"meta": {"set": name, "axon_bin": AXON, "files": sorted(files),
                        "real_bug_mining": mining},
               "tasks": tasks}, open(part, "w"))
    out = os.path.join(HERE, f"{name}.json")
    corpus.merge(out, [part])
    os.unlink(part)
    corpus.load(out)  # every derived text rebuilds to its recorded sha256
    print(f"{name}: {len(tasks)} tasks -> {out} sha256 {sha256(open(out, 'rb').read())}",
          file=sys.stderr)


def report(ok, bad):
    for s, t in ok:
        print(f"OK   {s['_from']} {s['scope']}/{s['shape']} {s['source']}::{s['symbol']} "
              f"visible={t['visible']} hidden={t['hidden']} failing={len(t['failing_on_broken'])} "
              f"rank={t['spectrum_rank']}", file=sys.stderr)
    for s, why in bad:
        print(f"FAIL {s['_from']} {s.get('scope')}/{s.get('shape')} {s.get('source')}::"
              f"{s.get('symbol')}: {why}", file=sys.stderr)


def main():
    """0/None: done; 1: a `--check` spec FAILed; 2: bad arguments. The
    arguments are refused before any binary runs or any file is written: an
    unknown one used to fall through to build mode, which rewrites the sets."""
    args = sys.argv[1:]
    if args in (["-h"], ["--help"]):
        print(USAGE)
        return 0
    if args and (args[0] != "--check" or any(a.startswith("-") for a in args[1:])):
        print(f"usage:\n{USAGE}", file=sys.stderr)
        return 2
    if args:
        ok, bad = check_all(load_specs(args[1:] or defect_files()))
        report(ok, bad)
        return 1 if bad else 0
    for snap in snapshots():
        green, _ = all_green(snap)
        if not green or len(tests_in(open(snap).read())) < 2:
            raise SystemExit(f"{rel(snap)} is not a green snapshot with >= 2 tests")
    recs, real = mine_real()
    json.dump({"pairs": recs, "qualifying": len(real)},
              open(os.path.join(HERE, "mining.json"), "w"), indent=1)
    mining = {"pairs": len(recs), "qualifying": len(real)}
    ok, bad = check_all(load_specs(defect_files()))
    report(ok, bad)
    tasks = real + [t for _, t in ok]
    planted = {(t["source_file"], t["broken"]) for _, t in ok}
    for t in real:
        if (t["source_file"], t["broken"]) in planted:
            raise SystemExit(f"real bug duplicates a planted defect: {t['source_file']}")
    dev_files, once_files = split(tasks)
    dev = number([t for t in tasks if t["source_file"] in dev_files], "hd")
    once = number([t for t in tasks if t["source_file"] in once_files], "ho")
    write_set("hard-dev", dev, dev_files, mining)
    write_set("hard-once", once, once_files, mining)
    print(json.dumps({"rejected_specs": len(bad), "real": len(real),
                      "hard-dev": {"in": sum(t["scope"] == "in" for t in dev),
                                   "out": sum(t["scope"] == "out" for t in dev)},
                      "hard-once": {"in": len(once)}}), file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main())
