#!/usr/bin/env python3
"""Mine REAL single-function defects from git history for the held-out set.

    python3 mine.py [OUT.json]        # default: mining.json beside this file

Every (commit, `.ax` file) pair in the history of HEAD where the file was
modified or renamed is a candidate. A pair QUALIFIES as a real-bug task when:

  1. the post-fix file has at least two `@[test]` functions (the corpus format
     needs a visible failing test and a different hidden one);
  2. exactly one non-test function body changed, and nothing else outside
     test blocks changed (the fix lives in one function, so reverting it is
     one body replacement -- the action Cortex and the grader both speak);
  3. that function existed before the commit (a fix, not an addition);
  4. the file still exists at HEAD with the function body exactly as the fix
     left it, and the HEAD file type-checks with every test passing;
  5. the file is not a dev-corpus source (../ab/corpus.json `meta.sources`);
  6. putting the pre-fix body back into the HEAD file passes ../ab/corpus.py's
     own `make_task` validity checks (still compiles, >= 2 tests fail, the arm-A
     workspace without the hidden check still compiles and still fails the
     visible test, the hidden check passes on the pristine file);
  7. `cortex locate` ranks the fixed function within run_ab.py's
     `--candidates` on the broken file (arm B tries only those names, and a
     ranker can reorder them but never add one, so a defect outside them is
     unreachable for every Cortex configuration and its B-oracle dry run).

Each pair is recorded with the FIRST rule it breaks, so the report counts
rejects by reason; a pair in a dev-corpus file is still run through rule 6 and
its outcome recorded, so "would it have qualified" is visible too. Whether the
commit subject reads as a fix (`fix`, `bug`, ...) is recorded but not used to
qualify: the behavioural witness is the criterion.
"""
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
from common import AXON, ROOT, defined_fns, fn_span, run, tests_in  # noqa: E402
from corpus import make_task, test_blocks, test_results  # noqa: E402
from run_ab import CANDIDATES, CORTEX  # noqa: E402

DEV_CORPUS = os.path.join(BENCH, "ab", "corpus.json")
FIX_WORDS = re.compile(r"\b(fix\w*|bug\w*|wrong|incorrect|off.by.one|regression|broken)\b", re.I)


def git(*args):
    return subprocess.run(["git", "-C", ROOT, *args], capture_output=True, text=True).stdout


def strip_tests(src):
    for _, a, b in sorted(test_blocks(src), key=lambda x: -x[1]):
        src = src[:a] + src[b:]
    return src


def body(src, name):
    s = fn_span(src, name)
    return src[s[0]:s[1]] if s else None


def pairs():
    """[(commit, old_path, new_path)] for every modified or renamed `.ax` file."""
    out = []
    for c in git("log", "--format=%H", "-M", "--diff-filter=MR", "HEAD", "--", "*.ax").split():
        for line in git("diff-tree", "--no-commit-id", "-r", "-M", "--name-status",
                        "--diff-filter=MR", c, "--", "*.ax").splitlines():
            parts = line.split("\t")
            if parts[0].startswith("R"):
                out.append((c, parts[1], parts[2]))
            elif parts[0] == "M":
                out.append((c, parts[1], parts[1]))
    return out


def examine(c, old, new, dev):
    pre, post = git("show", f"{c}^:{old}"), git("show", f"{c}:{new}")
    rec = {"commit": c[:12], "path": new, "subject": git("log", "-1", "--format=%s", c).strip(),
           "dev_corpus_file": new in dev}
    rec["fix_subject"] = bool(FIX_WORDS.search(rec["subject"]))
    checks = tests_in(post)
    if not checks:
        return dict(rec, reason="no @[test] functions in the file")
    if len(checks) < 2:
        return dict(rec, reason="only one @[test] function (needs a visible and a hidden check)")
    tests = set(checks) | set(tests_in(pre))
    names = list(dict.fromkeys(defined_fns(pre) + defined_fns(post)))
    changed = [n for n in names if n not in tests and body(pre, n) != body(post, n)]
    rec["changed_fns"] = changed
    if not changed:
        return dict(rec, reason="no non-test function body changed (tests, comments or declarations only)")
    if len(changed) > 1:
        return dict(rec, reason="change spans more than one function")
    fn = changed[0]
    rec["symbol"] = fn
    if body(pre, fn) is None:
        return dict(rec, reason="function added by the commit (no pre-fix body)")
    if body(post, fn) is None:
        return dict(rec, reason="function removed by the commit")
    sp, so = strip_tests(pre), strip_tests(post)
    a = fn_span(sp, fn)
    if sp[:a[0]] + body(post, fn) + sp[a[1]:] != so:
        return dict(rec, reason="non-test code outside the function also changed")
    if not os.path.exists(os.path.join(ROOT, new)):
        return dict(rec, reason="file no longer exists at HEAD")
    head = open(os.path.join(ROOT, new)).read()
    if body(head, fn) != body(post, fn):
        return dict(rec, reason="function body changed again after the fix")
    p = os.path.join(ROOT, new)
    r = test_results(p) if run([AXON, "check", p])[0] == 0 else None
    if r is None or any(s != "ok" for s in r.values()):
        return dict(rec, reason="HEAD file does not type-check with every test passing")
    pre_body = body(pre, fn)
    task = make_task(p, head, fn, "real", lambda _b: (pre_body, f"revert:{c[:12]}"))
    if task is None:
        rec["reason"] = "reverting the fix fails corpus.py validity (compile, >= 2 failing tests, arm-A workspace)"
        return rec
    if new in dev:
        return dict(rec, reason="dev-corpus source file (off limits); would otherwise qualify")
    task["spectrum_rank"] = locate_rank(task)
    if not 1 <= task["spectrum_rank"] <= int(CANDIDATES):
        return dict(rec, reason=f"fixed function outside cortex locate's top {CANDIDATES}")
    return dict(rec, reason=None, task=task)


def locate_rank(t):
    """The true symbol's 1-based rank in `cortex locate` (spectrum order) on
    the broken file, 0 if absent: run_ab.py arm B's own locate call."""
    ws = tempfile.mkdtemp(prefix="heldout_locate_")
    try:
        open(os.path.join(ws, t["file"]), "w").write(t["broken"])
        out = subprocess.run([CORTEX, "locate", "--workspace", ws, "--file", t["file"],
                              "--check", t["hidden"], "--axon", AXON, "--json"],
                             capture_output=True, text=True, timeout=300).stdout
    finally:
        shutil.rmtree(ws, ignore_errors=True)
    for line in reversed(out.strip().splitlines()):
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if isinstance(v, dict) and v.get("schema") == "cortex-locate/1":
            ranked = [r["symbol"] for r in v.get("ranked", [])]
            return ranked.index(t["symbol"]) + 1 if t["symbol"] in ranked else 0
    raise SystemExit(f"cortex locate gave no verdict for {t['source_file']}::{t['symbol']}")


def mine():
    dev = set(json.load(open(DEV_CORPUS))["meta"]["sources"])
    recs = [examine(c, o, n, dev) for c, o, n in pairs()]
    return recs


def write_report(recs, out):
    reasons = {}
    for r in recs:
        k = r["reason"] or "QUALIFIES"
        reasons[k] = reasons.get(k, 0) + 1
    report = {
        "method": "git log -M --diff-filter=MR HEAD -- '*.ax'; one record per (commit, file); "
                  "first broken rule recorded (see mine.py docstring)",
        "head": git("rev-parse", "HEAD").strip(),
        "commits": len({r["commit"] for r in recs}),
        "pairs": len(recs),
        "qualifying": sum(r["reason"] is None for r in recs),
        "by_reason": dict(sorted(reasons.items(), key=lambda kv: -kv[1])),
        "fix_subject_pairs": sum(r["fix_subject"] for r in recs),
        "records": [{k: v for k, v in r.items() if k != "task"} for r in recs],
    }
    json.dump(report, open(out, "w"), indent=1)
    print(json.dumps({k: report[k] for k in ("commits", "pairs", "qualifying", "by_reason")},
                     indent=1), file=sys.stderr)


def main():
    write_report(mine(), sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "mining.json"))


if __name__ == "__main__":
    main()
