#!/usr/bin/env python3
"""Shared helpers for the localization benchmarks.

Split out of what used to be `measure.py`, which scored the FIRST version of
localization and was committed already broken: it grepped stdout for a message
a later commit had renamed, and it invoked `cortex repair` with no grant and no
generator, so every trial was refused and scored as an error. It could not have
reproduced the numbers published beside it.

The lesson is worth keeping where the next harness author will see it: parsing
a human-readable message couples you to a string nobody thinks of as an
interface. `topk.py` and `oracle.py` read `--json`.
"""
import os
import re
import subprocess

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
AXON = os.environ.get("AXON_BIN", f"{ROOT}/target/debug/axon")

# The defect corpus.
#
# Everything published about localization and repair once rested on a single
# class -- one operator swap -- so "93% top-3" was a claim about arithmetic
# typos and nothing else. These add four more shapes that fail differently: a
# wrong constant carries no operator to spot, a flipped boolean changes a
# decision rather than a value, swapped arguments leave every token in place,
# and a deleted statement removes code rather than altering it.
#
# Being liberal here is safe by construction: a mutant is DISCARDED unless it
# still compiles and at least one check notices it, so a mutator that usually
# produces nonsense costs sample size and never costs correctness.
SWAPS = [("+", "-"), ("*", "+"), ("<=", "<"), (">=", ">"), ("&&", "||")]


def _mut_operator(body):
    for old, new in SWAPS:
        if old in body:
            return body.replace(old, new, 1), f"op:{old}->{new}"
    return None


def _mut_constant(body):
    """Bump the first integer literal. No operator to notice; only behaviour."""
    m = re.search(r"(?<![\w.])(\d+)(?![\w.])", body)
    if not m:
        return None
    v = int(m.group(1))
    return body[: m.start()] + str(v + 1) + body[m.end() :], f"const:{v}->{v+1}"


def _mut_bool(body):
    """Flip a boolean literal: a decision changes, not a value."""
    for old, new in (("true", "false"), ("false", "true")):
        m = re.search(r"(?<![\w])%s(?![\w])" % old, body)
        if m:
            return body[: m.start()] + new + body[m.end() :], f"bool:{old}->{new}"
    return None


def _mut_argswap(body):
    """Swap two simple arguments. Every token stays; only the order moves."""
    m = re.search(r"\b([a-z_][\w]*)\(\s*([a-z_][\w.]*)\s*,\s*([a-z_][\w.]*)\s*\)", body)
    if not m or m.group(2) == m.group(3):
        return None
    rep = f"{m.group(1)}({m.group(3)}, {m.group(2)})"
    return body[: m.start()] + rep + body[m.end() :], f"argswap:{m.group(1)}"


def _mut_drop_stmt(body):
    """Delete a standalone statement -- code removed, not altered.

    Bindings are skipped: dropping a `let` almost always fails to compile, and
    a mutant that does not compile is discarded anyway, so trying is only a
    waste of sample size.
    """
    lines = body.split("\n")
    for i, l in enumerate(lines):
        t = l.strip()
        if not t or t.startswith("//") or t.startswith("let ") or t.startswith("own "):
            continue
        if any(c in t for c in "{}") or "=" not in t:
            continue
        if i == len(lines) - 2:  # the tail expression is the return value
            continue
        return "\n".join(lines[:i] + lines[i + 1 :]), "drop-stmt"
    return None


MUTATORS = [
    ("operator", _mut_operator),
    ("constant", _mut_constant),
    ("boolean", _mut_bool),
    ("argswap", _mut_argswap),
    ("drop-stmt", _mut_drop_stmt),
]


def tests_in(src):
    out = []
    lines = src.split("\n")
    for i, l in enumerate(lines):
        if l.strip().startswith("@[test"):
            for j in range(i + 1, min(i + 4, len(lines))):
                m = re.match(r"\s*fn ([A-Za-z_][A-Za-z0-9_]*)\(", lines[j])
                if m:
                    out.append(m.group(1))
                    break
    return out


def fn_span(src, name):
    m = re.search(r"\bfn %s\(" % re.escape(name), src)
    if not m:
        return None
    o = src.find("{", m.start())
    if o < 0:
        return None
    d = 0
    for i in range(o, len(src)):
        if src[i] == "{":
            d += 1
        elif src[i] == "}":
            d -= 1
            if d == 0:
                return (o + 1, i)
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
    """Which tests FAILED, read from the --json contract.

    The human transcript prints `test NAME [should_fail] ... FAILED`, and that
    annotation appears ONLY on the failure branch -- the branch this function
    exists to read. Parsing the transcript silently produced the name
    `NAME [should_fail]`, and it cost three trials before anyone noticed.
    """
    import json as _json
    _, t = run([AXON, "test", path, "--json"])
    out = []
    for line in t.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            v = _json.loads(line)
        except ValueError:
            continue
        if v.get("status") == "failed" and "name" in v:
            out.append(v["name"])
    return out
