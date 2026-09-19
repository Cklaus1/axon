#!/usr/bin/env python3
"""Phase 5 — repair tasks from code I did not write, judged by an oracle I did
not author.

Every earlier phase shares one confound: I wrote the broken program, the intent,
and the hidden tests — and, earlier in this branch, several of the compiler
hints the treatment arm reads. That cannot be argued away from inside; it has to
be removed.

Here:
  * the SOURCE is real Python stdlib (`textwrap`, `posixpath`, `difflib`, ...)
  * the DEFECT is a mechanical mutation, not a bug I invented
  * the ORACLE is the REAL stdlib function, called on RANDOM inputs — so the
    expected behaviour is not my opinion either
  * the INTENT shown to the model is the function's own docstring

A task is admitted only if the mutant actually differs from the original on some
random input. A mutation that changes nothing is an equivalent mutant and would
score every arm identically.
"""
import inspect, json, pathlib, random, re, textwrap as _tw

HERE = pathlib.Path(__file__).parent

# (module, function, input generator) — pure functions with stable behaviour.
TARGETS = [
    ("textwrap", "shorten", lambda r: (r.choice([
        "the quick brown fox jumps over the lazy dog",
        "one two three four five six seven",
        "a b c d e f g h i j k l m n o p"]), r.choice([10, 15, 20, 30]))),
    ("textwrap", "wrap", lambda r: (r.choice([
        "alpha beta gamma delta epsilon zeta eta theta",
        "short", "aaa bbb ccc ddd eee fff ggg hhh iii"]), r.choice([5, 10, 20]))),
    ("textwrap", "indent", lambda r: (r.choice(["a\nb\nc\n", "x\n\ny\n", "solo\n"]),
                                      r.choice([">> ", "    ", "# "]))),
    ("posixpath", "normpath", lambda r: (r.choice([
        "a/b/../c", "./x/./y", "/a//b/", "../../q", "a/b/c/../../d", "/"]),)),
    ("posixpath", "relpath", lambda r: (r.choice(["/a/b/c", "/a/x", "/q"]),
                                        r.choice(["/a", "/a/b", "/"]))),
    ("posixpath", "splitext", lambda r: (r.choice([
        "a.txt", "archive.tar.gz", ".bashrc", "noext", "dir/.x", "a."]),)),
    ("posixpath", "commonpath", lambda r: ([r.choice(["/a/b/c", "/a/b/d"]),
                                            r.choice(["/a/b/e", "/a/z"])],)),
    ("difflib", "get_close_matches", lambda r: (r.choice(["appel", "banan", "chery"]),
                                                ["apple", "banana", "cherry", "grape"])),
]

# Mechanical mutations, applied to source I did not write.
MUTATIONS = [
    (r"(?<![<>=!])==(?!=)", "!="), (r"!=", "=="),
    (r"(?<![<>=!])<=", "<"), (r"(?<![<>=!])>=", ">"),
    (r"(?<![<>=!])<(?![=<])", "<="), (r"(?<![<>=!])>(?![=>])", ">="),
    (r"\+ 1\b", "+ 2"), (r"- 1\b", "- 2"),
    (r"\bmax\(", "min("), (r"\bmin\(", "max("),
    (r"\bTrue\b", "False"),
]


def differs(orig_fn, mutant_src, fname, gen, trials=60):
    """Does the mutant disagree with the real function on any random input?"""
    ns = {}
    try:
        exec("import re, os, sys, itertools, functools, collections\n" + mutant_src, ns)
    except Exception:
        return False, "mutant does not import"
    mf = ns.get(fname)
    if mf is None:
        return False, "mutant lacks the function"
    r = random.Random(20250919)
    for _ in range(trials):
        args = gen(r)
        try:
            a = orig_fn(*args)
        except Exception as e:
            a = ("exc", type(e).__name__)
        try:
            b = mf(*args)
        except Exception as e:
            b = ("exc", type(e).__name__)
        if a != b:
            return True, f"differs on {args!r}"
    return False, "equivalent mutant"


def main():
    out, rejected = [], []
    for modname, fname, gen in TARGETS:
        mod = __import__(modname)
        fn = getattr(mod, fname)
        try:
            src = _tw.dedent(inspect.getsource(fn))
        except Exception as e:
            rejected.append((modname, fname, f"no source: {e}")); continue
        doc = (inspect.getdoc(fn) or "").strip().split("\n\n")[0]
        made = False
        for pat, rep in MUTATIONS:
            if not re.search(pat, src):
                continue
            mutant = re.sub(pat, rep, src, count=1)
            if mutant == src:
                continue
            ok, why = differs(fn, mutant, fname, gen)
            if ok:
                out.append({
                    "name": f"{modname}_{fname}_{MUTATIONS.index((pat, rep))}",
                    "module": modname, "func": fname, "broken": mutant,
                    "intent": doc or f"{fname} from the Python {modname} module",
                    "witness": why,
                })
                made = True
                break
        if not made:
            rejected.append((modname, fname, "no non-equivalent mutation found"))
    (HERE / "stdlib_tasks.json").write_text(json.dumps(out, indent=2))
    print(f"admitted {len(out)} tasks from real stdlib source")
    for t in out:
        print(f"  {t['name']:<28} {t['witness'][:56]}")
    for m, f, why in rejected:
        print(f"  drop {m}.{f}: {why}")


if __name__ == "__main__":
    main()
