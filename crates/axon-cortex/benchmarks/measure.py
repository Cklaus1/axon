#!/usr/bin/env python3
"""Does Cortex localize a REAL defect in REAL Axon code?

Every claim about localization so far rests on one 20-line fixture written to
be repairable. This injects a defect into each function of the real corpus,
one at a time, and asks whether Cortex names the function that was broken.

Method, and its limits stated up front:
  * The defect is a single operator swap in one function's body. It is a real
    behavioural change that still compiles, which is the class localization is
    for. It is NOT representative of every defect — a missing branch or a wrong
    constant would exercise the same machinery differently.
  * The mutant is KEPT only if the file still compiles and at least one
    non-hidden @[test] now fails. A mutant that breaks the build or that no
    test notices is discarded: neither asks the question.
  * The hidden check is the LAST @[test] in the file, excluded from the
    evidence the same way the production path excludes it.
  * A file is skipped if it does not compile CLEAN before mutation, because
    then the baseline is not "healthy" and the failure is not ours.

Scoring, deliberately four-way rather than pass/fail: a correct name, a WRONG
name, an honest refusal, and an error are four different outcomes with four
different remedies. Collapsing refusals into failures would make a controller
that knows what it does not know look identical to one that guesses.
"""
import os, re, subprocess, sys, json, shutil, tempfile

# Derived from this file's location, never hardcoded: a benchmark that only
# runs from one person's home directory is not reproducible evidence.
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
AXON = os.environ.get("AXON_BIN", f"{ROOT}/target/debug/axon")
CORTEX = os.environ.get("CORTEX_BIN", f"{ROOT}/target/debug/cortex")

SWAPS = [("+", "-"), ("*", "+"), ("<=", "<"), (">=", ">"), ("&&", "||")]

def tests_in(src):
    out = []
    lines = src.split("\n")
    for i, l in enumerate(lines):
        if l.strip().startswith("@[test]"):
            for j in range(i + 1, min(i + 4, len(lines))):
                m = re.match(r"\s*fn ([A-Za-z_][A-Za-z0-9_]*)\(", lines[j])
                if m:
                    out.append(m.group(1))
                    break
    return out

def fn_span(src, name):
    m = re.search(r"\bfn %s\(" % re.escape(name), src)
    if not m: return None
    o = src.find("{", m.start())
    if o < 0: return None
    d = 0
    for i in range(o, len(src)):
        if src[i] == "{": d += 1
        elif src[i] == "}":
            d -= 1
            if d == 0: return (o + 1, i)
    return None

def defined_fns(src):
    return re.findall(r"^\s*fn ([A-Za-z_][A-Za-z0-9_]*)\(", src, re.M)

def run(cmd, cwd=None, timeout=60):
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired:
        return -1, "TIMEOUT"

def failing_tests(path):
    _, t = run([AXON, "test", path])
    return [l.strip()[5:].split(" ... ")[0].strip()
            for l in t.split("\n")
            if l.strip().startswith("test ") and " ... " in l and "FAILED" in l]

if __name__ == "__main__":
    results = []
    files = sorted(sys.argv[1:])
    for f in files:
        src0 = open(f).read()
        checks = tests_in(src0)
        if len(checks) < 2:
            results.append({"file": f, "outcome": "skip", "why": "fewer than 2 checks"}); continue
        hidden = checks[-1]
        rc, _ = run([AXON, "check", f])
        if rc != 0:
            results.append({"file": f, "outcome": "skip", "why": "does not check clean"}); continue
        if failing_tests(f):
            results.append({"file": f, "outcome": "skip", "why": "already failing before mutation"}); continue

        targets = [n for n in defined_fns(src0) if n not in checks and n != "main"]
        done = 0
        for name in targets:
            if done >= 3: break            # cap per file so one large file cannot dominate
            span = fn_span(src0, name)
            if not span: continue
            a, b = span
            body = src0[a:b]
            for old, new in SWAPS:
                if old not in body: continue
                mutant = src0[:a] + body.replace(old, new, 1) + src0[b:]
                ws = tempfile.mkdtemp(prefix="loc_")
                rel = os.path.basename(f)
                open(os.path.join(ws, rel), "w").write(mutant)
                rc, _ = run([AXON, "check", os.path.join(ws, rel)])
                if rc != 0:
                    shutil.rmtree(ws, ignore_errors=True); continue
                fails = [t for t in failing_tests(os.path.join(ws, rel)) if t != hidden]
                if not fails:
                    shutil.rmtree(ws, ignore_errors=True); continue

                rc, out = run([CORTEX, "repair", "--workspace", ws, "--file", rel,
                               "--check", hidden, "--axon", AXON])
                m = re.search(r"localized `([A-Za-z0-9_]+)`", out)
                if m:
                    outcome = "correct" if m.group(1) == name else "wrong"
                    got = m.group(1)
                elif rc == 25:
                    outcome = "refused"
                    got = out.strip().split("\n")[0][:110]
                else:
                    outcome = "error"
                    got = out.strip().split("\n")[0][:110]
                results.append({"file": f, "broke": name, "swap": f"{old}->{new}",
                                "failing": fails[:3], "outcome": outcome, "got": got})
                shutil.rmtree(ws, ignore_errors=True)
                done += 1
                break

    print(json.dumps(results, indent=1))
