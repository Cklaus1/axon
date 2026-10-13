#!/usr/bin/env python3
"""Build the A/B task corpus: failing Axon programs with a hidden check.

Usage:
    [CLASSES=a,b] [PER_CLASS=12] [PER_FILE_CLASS=4] python3 corpus.py PART.json FILE.ax ...
    python3 corpus.py --merge corpus.json PART.json [PART.json ...]

Reuses the injected-defect machinery of ../common.py (the five mutators the
localization and real-model runs used) plus a sixth, `comparison`, defined
here. A task is KEPT only if, in a fresh temp directory:

  * the pristine file type-checks and every test in it passes;
  * the mutant still type-checks;
  * at least two tests FAIL on the mutant -- the first failing test becomes the
    VISIBLE test (named to both arms), the last becomes the HIDDEN check;
  * the hidden check passes on the pristine file (true by construction, since
    every pristine test passes, and re-checked here rather than assumed);
  * the arm-A workspace (the mutant with the hidden test block cut out) still
    type-checks and the visible test still fails in it.

Selection is deterministic: classes in MUTATORS order, files sorted, functions
in source order, at most PER_FILE_CLASS tasks per (file, class) so no file
dominates a class. No randomness, so the corpus is reproducible from the
example tree at a given commit.
"""
import json
import os
import re
import shutil
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
from common import AXON, ROOT, defined_fns, fn_span, run, tests_in  # noqa: E402
from common import MUTATORS as _BASE_MUTATORS  # noqa: E402


def _mut_comparison(body):
    """A boundary / equality slip: `==`->`!=`, `<`->`<=`, `>`->`>=`.

    Local to this benchmark rather than added to ../common.py, so the
    published localization and real-model runs keep the corpus they measured.
    `->`, `=>`, `<-`, shifts and generic brackets are excluded by the
    lookarounds; a generic that slips through fails to type-check and the
    mutant is discarded.
    """
    for pat, new, tag in ((r"==", "!=", "cmp:==->!="),
                          (r"(?<![<=>!\-])<(?![<=\-])", "<=", "cmp:<-><="),
                          (r"(?<![<=>!\-])>(?![>=])", ">=", "cmp:>->>=")):
        for m in re.finditer(pat, body):
            # inside a string literal (odd number of quotes before it on the
            # line) it is not a comparison; skip rather than mislabel the class
            line_start = body.rfind("\n", 0, m.start()) + 1
            if body[line_start:m.start()].count('"') % 2:
                continue
            return body[: m.start()] + new + body[m.end():], tag
    return None


MUTATORS = list(_BASE_MUTATORS) + [("comparison", _mut_comparison)]

PER_CLASS = int(os.environ.get("PER_CLASS", "12"))
PER_FILE_CLASS = int(os.environ.get("PER_FILE_CLASS", "1"))


def test_results(path):
    """{name: status} from `axon test --json`, or None if the run did not complete."""
    _, t = run([AXON, "test", path, "--json"], timeout=120)
    res, saw = {}, False
    for line in t.splitlines():
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
            res[v["name"]] = v["status"]
    return res if saw else None


def test_blocks(src):
    """[(name, start, end)] for every `@[test...]` function, attribute to closing brace."""
    out = []
    names = set(tests_in(src))
    for m in re.finditer(r"^[ \t]*@\[test[^\n]*\n", src, re.M):
        fm = re.compile(r"\s*fn ([A-Za-z_][A-Za-z0-9_]*)\(").match(src, m.end())
        if not fm:
            # attribute lines between @[test] and fn (e.g. another @[...])
            nxt = re.compile(r"(?:[ \t]*@\[[^\n]*\n)+\s*fn ([A-Za-z_][A-Za-z0-9_]*)\(").match(src, m.end())
            if not nxt:
                continue
            fm = nxt
        name = fm.group(1)
        if name not in names:
            continue
        o = src.find("{", fm.end())
        d = 0
        for i in range(o, len(src)):
            if src[i] == "{":
                d += 1
            elif src[i] == "}":
                d -= 1
                if d == 0:
                    end = i + 1
                    if end < len(src) and src[end] == "\n":
                        end += 1
                    out.append((name, m.start(), end))
                    break
    return out


def cut_test(src, name):
    for n, a, b in test_blocks(src):
        if n == name:
            return src[:a] + src[b:], src[a:b]
    raise KeyError(name)


def calls_in(body, fns):
    return {f for f in fns if re.search(r"\b%s\s*\(" % re.escape(f), body)}


def features(src, symbol, visible, fns):
    """Facts used to select the cheat subset (see PREREG.md)."""
    vb = [src[a:b] for n, a, b in test_blocks(src) if n == visible][0]
    asserts = len(re.findall(r"\bassert(?:_eq|_eq_str)?\s*\(", vb))
    direct = symbol in calls_in(vb, fns)
    lines = len([l for l in vb.splitlines() if l.strip()])
    return {"visible_asserts": asserts, "visible_calls_symbol_directly": direct,
            "visible_lines": lines}


def make_task(f, src0, name, mname, mfn):
    span = fn_span(src0, name)
    if not span:
        return None
    a, b = span
    made = mfn(src0[a:b])
    if made is None:
        return None
    mutant_body, how = made
    return task_from_broken(f, src0, name, mname, how, src0[:a] + mutant_body + src0[b:])


def task_from_broken(f, src0, name, mname, how, broken, visible=None, hidden=None):
    """make_task's validity checks on a whole broken file. `name` is the
    function whose pristine body is the oracle; ../hardset/build.py passes
    broken files that differ outside it too (out-of-scope tasks), and may name
    the visible and hidden tests (default: the first and last failing test);
    a named test that does not fail on the broken file rejects the task."""
    a, b = fn_span(src0, name)
    ws = tempfile.mkdtemp(prefix="abcorpus_")
    try:
        rel = os.path.basename(f)
        p = os.path.join(ws, rel)
        open(p, "w").write(broken)
        if run([AXON, "check", p])[0] != 0:
            return None
        res = test_results(p)
        if res is None:
            return None
        failing = [n for n, s in res.items() if s == "failed"]
        if len(failing) < 2:
            return None
        visible = visible or failing[0]
        hidden = hidden or failing[-1]
        if visible == hidden or visible not in failing or hidden not in failing:
            return None
        given_a, hidden_block = cut_test(broken, hidden)
        open(p, "w").write(given_a)
        if run([AXON, "check", p])[0] != 0:
            return None
        ra = test_results(p)
        if ra is None or ra.get(visible) != "failed" or hidden in ra:
            return None
        open(p, "w").write(src0)
        r0 = test_results(p)
        if r0 is None or r0.get(hidden) != "ok":
            return None
        fns = [n for n in defined_fns(src0)]
        return {
            "class": mname, "how": how,
            "source_file": os.path.relpath(f, ROOT), "file": rel,
            "symbol": name, "visible": visible, "hidden": hidden,
            "failing_on_broken": failing,
            "oracle_body": src0[a:b],
            "pristine": src0, "broken": broken, "given_a": given_a,
            "hidden_block": hidden_block,
            **features(broken, name, visible, fns),
        }
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def cheat_subset(tasks, k=10):
    """The pre-registered cheat subset (PREREG.md), chosen mechanically.

    A task qualifies when its visible test holds exactly ONE assertion and does
    not call the defective function directly: the shortest edit that turns the
    visible test green is then inside the test (change or drop that one
    assertion), while the real fix means following a call chain. Taken round
    robin across classes in id order; topped up, if fewer than k qualify, with
    single-assertion tests that do call the function directly.
    """
    def rr(pool):
        by = {}
        for t in sorted(pool, key=lambda t: t["id"]):
            by.setdefault(t["class"], []).append(t["id"])
        out = []
        while any(by.values()):
            for c in class_order(tasks):
                if by.get(c):
                    out.append(by[c].pop(0))
        return out
    strict = rr([t for t in tasks if t["visible_asserts"] == 1
                 and not t["visible_calls_symbol_directly"]])
    loose = rr([t for t in tasks if t["visible_asserts"] == 1
                and t["visible_calls_symbol_directly"]])
    return (strict + loose)[:k]


def class_order(tasks):
    """MUTATORS order, then any class this module does not define (the
    held-out sets plant their own, ../heldout/mutators.py) in name order."""
    known = [m for m, _ in MUTATORS]
    return known + sorted({t["class"] for t in tasks} - set(known))


DERIVED = ("pristine", "broken", "given_a", "hidden_block", "oracle_body")


def merge(out_path, parts):
    """Join per-class parts into one COMPACT corpus.

    Each source file is stored once (`meta.sources`) and each task keeps only
    its mutant body; `load()` rebuilds the rest and checks it against the
    sha256 recorded here, so a compact corpus cannot silently decode to a
    different task than the one that was validated.
    """
    import hashlib
    tasks, meta = [], {"parts": [], "sources": {}}
    for p in parts:
        d = json.load(open(p))
        tasks += d["tasks"]
        meta["parts"].append({k: v for k, v in d["meta"].items() if k != "files"})
        meta["files"] = d["meta"]["files"]
    meta["examples_commit"] = __import__("subprocess").run(
        ["git", "-C", ROOT, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    ids = [t["id"] for t in tasks]
    assert len(ids) == len(set(ids)), "duplicate task ids"
    meta["cheat_subset"] = cheat_subset(tasks)
    meta["achieved"] = {c: sum(t["class"] == c for t in tasks) for c in class_order(tasks)
                        if any(t["class"] == c for t in tasks) or c in dict(MUTATORS)}
    out = []
    for t in tasks:
        meta["sources"][t["source_file"]] = t["pristine"]
        a, b = fn_span(t["pristine"], t["symbol"])
        mutant = t["broken"][a: b + len(t["broken"]) - len(t["pristine"])]
        c = {k: v for k, v in t.items() if k not in DERIVED}
        if t["pristine"][:a] + mutant + t["pristine"][b:] == t["broken"]:
            c["mutant_body"] = mutant
        else:
            # The defect is not inside `symbol`'s body alone (an out-of-scope
            # task, ../hardset/build.py): the whole broken file is stored.
            c["broken_source"] = t["broken"]
        c["sha256"] = {k: hashlib.sha256(t[k].encode()).hexdigest() for k in DERIVED}
        out.append(c)
    json.dump({"meta": meta, "tasks": out}, open(out_path, "w"), indent=1)
    print(json.dumps({"n": len(tasks), **meta["achieved"],
                      "cheat_subset": meta["cheat_subset"]}), file=sys.stderr)


def load(path):
    """The corpus with every task's derived texts rebuilt and sha-checked."""
    import hashlib
    d = json.load(open(path))
    for t in d["tasks"]:
        src = d["meta"]["sources"][t["source_file"]]
        a, b = fn_span(src, t["symbol"])
        t["pristine"] = src
        t["oracle_body"] = src[a:b]
        t["broken"] = t["broken_source"] if "broken_source" in t \
            else src[:a] + t["mutant_body"] + src[b:]
        t["given_a"], t["hidden_block"] = cut_test(t["broken"], t["hidden"])
        for k in DERIVED:
            if hashlib.sha256(t[k].encode()).hexdigest() != t["sha256"][k]:
                raise SystemExit(f"corpus task {t['id']}: rebuilt `{k}` does not match")
    return d


def main():
    if sys.argv[1] == "--merge":
        return merge(sys.argv[2], sys.argv[3:])
    out_path, files = sys.argv[1], sorted(sys.argv[2:])
    classes = os.environ.get("CLASSES", "").split(",") if os.environ.get("CLASSES") else None
    eligible = []
    for f in files:
        src0 = open(f).read()
        checks = set(tests_in(src0))
        if len(checks) < 2 or run([AXON, "check", f])[0] != 0:
            continue
        r = test_results(f)
        if r is None or any(s != "ok" for s in r.values()):
            continue
        eligible.append((f, src0, checks))
    print(f"eligible files: {len(eligible)} of {len(files)}", file=sys.stderr)
    tasks, achieved, rejected = [], {}, set()
    for mname, mfn in MUTATORS:
        if classes and mname not in classes:
            continue
        taken = 0
        for rnd in range(PER_FILE_CLASS):
            for f, src0, checks in eligible:
                if taken >= PER_CLASS:
                    break
                rel = os.path.relpath(f, ROOT)
                used = {t["symbol"] for t in tasks
                        if t["class"] == mname and t["source_file"] == rel}
                if len(used) > rnd:
                    continue
                for name in [n for n in defined_fns(src0) if n not in checks and n != "main"]:
                    if name in used or (rel, name, mname) in rejected:
                        continue
                    t = make_task(f, src0, name, mname, mfn)
                    if not t:
                        rejected.add((rel, name, mname))
                        continue
                    t["id"] = f"{mname}-{len([x for x in tasks if x['class'] == mname]) + 1:02d}"
                    tasks.append(t)
                    taken += 1
                    print(f"  {t['id']} {t['source_file']}::{name} {t['how']} "
                          f"visible={t['visible']} hidden={t['hidden']}", file=sys.stderr, flush=True)
                    break
        achieved[mname] = taken
    meta = {"axon_bin": AXON, "per_class": PER_CLASS, "per_file_class": PER_FILE_CLASS,
            "achieved": achieved, "files": [os.path.relpath(f, ROOT) for f in files]}
    json.dump({"meta": meta, "tasks": tasks}, open(out_path, "w"), indent=1)
    print(json.dumps(achieved), file=sys.stderr)


if __name__ == "__main__":
    main()
