#!/usr/bin/env python3
"""Phase 5 task builder, generalised: scan real stdlib functions, mutate them
mechanically, and keep only mutants the REAL function disagrees with.

Input generators are chosen by trying typed shapes until the ORIGINAL function
accepts them — so the inputs are discovered, not authored.
"""
import inspect, importlib, json, pathlib, random, re, textwrap

HERE = pathlib.Path(__file__).parent
MODULES = ["difflib", "fnmatch", "base64", "email.utils", "json.encoder",
           "quopri", "statistics", "heapq", "calendar", "gettext", "shlex"]

MUTATIONS = [
    (r"(?<![<>=!])==(?!=)", "!="), (r"!=", "=="),
    (r"(?<![<>=!])<=", "<"), (r"(?<![<>=!])>=", ">"),
    (r"(?<![<>=!])<(?![=<])", "<="), (r"(?<![<>=!])>(?![=>])", ">="),
    (r"\bmax\(", "min("), (r"\bmin\(", "max("),
    (r"\+ 1\b", "+ 2"), (r"- 1\b", "- 2"),
    (r"\breversed\(", "iter("), (r"\bTrue\b", "False"),
]

WORDS = ["apple", "banana", "cherry", "grape", "melon", "appel", "bananna"]
SHAPES = [
    lambda r: (r.choice(WORDS),),
    lambda r: (r.choice(WORDS), list(WORDS)),
    lambda r: (r.choice([b"hello", b"abc", b"x" * 7]),),
    lambda r: (r.choice(["a*b", "?x", "[ab]c"]),),
    lambda r: ([r.randint(1, 50) for _ in range(6)], r.randint(1, 4)),
    lambda r: (r.randint(1, 12), r.randint(1, 28)),
    lambda r: ([float(r.randint(1, 20)) for _ in range(6)],
               [float(r.randint(1, 20)) for _ in range(6)]),
    lambda r: (r.choice(["a b 'c d'", "x  y", "'q'"]),),
    lambda r: ([r.choice(WORDS) for _ in range(3)],),
]


def find_shape(fn):
    """Pick the first input shape the REAL function accepts on every trial."""
    r = random.Random(1)
    for gen in SHAPES:
        try:
            for _ in range(4):
                fn(*gen(r))
            return gen
        except Exception:
            continue
    return None


def call(f, args):
    try:
        return ("ok", f(*args))
    except Exception as e:
        return ("exc", type(e).__name__)


def main():
    tasks, seen = [], set()
    for mname in MODULES:
        try:
            mod = importlib.import_module(mname)
        except Exception:
            continue
        for fname, fn in sorted(vars(mod).items()):
            if fname.startswith("_") or not inspect.isfunction(fn):
                continue
            if (mname, fname) in seen:
                continue
            try:
                src = textwrap.dedent(inspect.getsource(fn))
            except Exception:
                continue
            body = [l for l in src.splitlines() if l.strip() and not l.strip().startswith("#")]
            if len(body) < 8 or len(body) > 70:
                continue
            gen = find_shape(fn)
            if gen is None:
                continue
            for pat, rep in MUTATIONS:
                if not re.search(pat, src):
                    continue
                mutant = re.sub(pat, rep, src, count=1)
                if mutant == src:
                    continue
                ns = {}
                try:
                    exec("import re, os, sys, itertools, functools, collections, math\n" + mutant, ns)
                except Exception:
                    continue
                mf = ns.get(fname)
                if mf is None:
                    continue
                r = random.Random(4242)
                witness = None
                for _ in range(80):
                    args = gen(r)
                    if call(fn, args) != call(mf, args):
                        witness = repr(args)[:90]
                        break
                if witness:
                    tasks.append({
                        "name": f"{mname.replace('.', '_')}_{fname}",
                        "module": mname, "func": fname,
                        "broken": mutant,
                        "intent": (inspect.getdoc(fn) or f"{fname} from {mname}").strip().split("\n\n")[0][:400],
                        "shape_index": SHAPES.index(gen),
                        "witness": witness,
                    })
                    seen.add((mname, fname))
                    break
    (HERE / "stdlib_tasks.json").write_text(json.dumps(tasks, indent=2))
    print(f"admitted {len(tasks)} tasks from real stdlib source (code I did not write)")
    for t in tasks:
        print(f"  {t['name']:<34} witness={t['witness'][:48]}")


if __name__ == "__main__":
    main()
