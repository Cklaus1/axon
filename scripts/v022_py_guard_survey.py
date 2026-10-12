#!/usr/bin/env python3
"""Survey of the Python refusal guards of scripts/guest_build_env.py (C9 round 9,
eqgate5, amendment 98).

    python3 scripts/v022_py_guard_survey.py [--json OUT] [--update] [LINE ...]

For each refusal site `py_sites` finds (scripts/v022_refusal_coverage.py), the
guard is REMOVED (`if C:` -> `if False and (C):`, or, for a site with no guard
of its own or in an `else`, the statement -> `pass`), the tests of
crates/axon-fabric/tests/guest_build_env_guards.rs are run, and the site is
KILLED when a case named `ATTACK: gbe <case>` fails and SURVIVED when every test
passes. The file is restored after each edit (it must be unmodified when the
survey starts). The result is compared with PY_EXEMPT: a site the table calls
OBSERVED must be killed, a REMAINDER one is reported when it is killed (the
table can then be upgraded). Exit 1 on a disagreement. This is a MEASUREMENT of
what the tests observe; a row (scripts/v022_g01_mutations.py) is the claim.
"""
import ast
import importlib.util
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
F = "scripts/guest_build_env.py"


def load_gate():
    spec = importlib.util.spec_from_file_location("rc", os.path.join(ROOT, "scripts/v022_refusal_coverage.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def edit_for(tree, orig, lines, offs, g, a):
    def off(lineno, col):
        return offs[lineno - 1] + len(lines[lineno - 1].encode()[:col].decode())
    for n in ast.walk(tree):
        if (isinstance(n, ast.If) and n.lineno == g + 1 and any(b.lineno <= a + 1 <= b.end_lineno for b in n.body)):
            t = n.test
            s, e = off(t.lineno, t.col_offset), off(t.end_lineno, t.end_col_offset)
            return s, e, "False and (" + orig[s:e] + ")"
    for n in ast.walk(tree):
        if isinstance(n, (ast.Return, ast.Expr, ast.Raise)) and n.lineno == a + 1:
            return off(n.lineno, n.col_offset), off(n.end_lineno, n.end_col_offset), "pass"
    return None


def main():
    args = sys.argv[1:]
    out = None
    if "--json" in args:
        i = args.index("--json")
        out = args[i + 1]
        del args[i:i + 2]
    only = {a for a in args if a.isdigit()}
    os.chdir(ROOT)
    if subprocess.run(["git", "diff", "--quiet", "--", F]).returncode != 0:
        sys.exit(f"refused: {F} has uncommitted changes: the survey edits it in place")
    rc = load_gate()
    orig = open(F).read()
    lines = orig.split("\n")
    offs = [0]
    for l in lines:
        offs.append(offs[-1] + len(l) + 1)
    tree = ast.parse(orig)
    declared = {(e[1], e[2]): e for e in rc.PY_EXEMPT if e[0] == F}
    res, bad = [], []
    for g, a, b, fn, n in rc.py_sites(orig):
        if only and str(a + 1) not in only:
            continue
        ed = edit_for(tree, orig, lines, offs, g, a)
        if ed is None:
            bad.append(f"{F}:{a + 1}: no removal edit for this site")
            continue
        s, e, new = ed
        open(F, "w").write(orig[:s] + new + orig[e:])
        try:
            r = subprocess.run(["cargo", "test", "-q", "-p", "axon-fabric", "--test", "guest_build_env_guards",
                                "--", "--test-threads=4"], capture_output=True, text=True, timeout=900)
            text = r.stdout + r.stderr
        finally:
            open(F, "w").write(orig)
        cases = sorted(set(re.findall(r"ATTACK: gbe (.+?): ", text)))
        verdict = "SURVIVED" if r.returncode == 0 else ("KILLED" if cases else "FAILED-ELSE")
        res.append({"line": a + 1, "function": fn, "n": n, "verdict": verdict, "cases": cases})
        print(f"{a + 1} {fn}#{n} {verdict} {cases}", flush=True)
        d = declared.get((fn, n))
        if d is not None and d[4] == "OBSERVED" and verdict != "KILLED":
            bad.append(f"{F}:{a + 1}: PY_EXEMPT calls {fn}#{n} OBSERVED, but the survey says {verdict}")
        if d is not None and d[4] == "REMAINDER" and verdict == "KILLED":
            print(f"note: {fn}#{n} is REMAINDER in PY_EXEMPT and is KILLED by {cases}: upgrade it")
    if out:
        json.dump(res, open(out, "w"), indent=1)
    for b in bad:
        print("BAD", b)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
