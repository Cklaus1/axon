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

# One operator swap per function: a real behavioural change that still
# compiles, which is the class localization is for. NOT representative of every
# defect -- a missing branch or a wrong constant exercises the same machinery
# differently, and neither is measured here.
SWAPS = [("+", "-"), ("*", "+"), ("<=", "<"), (">=", ">"), ("&&", "||")]


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
