#!/usr/bin/env python3
"""Check governance/specs/v022-psv-negative-matrix.md against the tree.

FAILS when:
* any row A1..A40 is missing;
* a row cites no test;
* a cited `path::function` names a file that does not exist, or a function
  that file does not define as a `fn`;
* a cited script does not exist.

A matrix that points at tests which were renamed or deleted would claim
coverage nobody runs; this makes that a gate failure, not a silent drift.
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOC = os.path.join(ROOT, "governance/specs/v022-psv-negative-matrix.md")


def main():
    text = open(DOC).read()
    rows = {}
    for line in text.splitlines():
        m = re.match(r"\|\s*(A\d+)\s*\|", line)
        if m:
            rows[m.group(1)] = line
    bad = []
    for n in range(1, 41):
        k = f"A{n}"
        if k not in rows:
            bad.append(f"{k}: row missing")
            continue
        cells = [c.strip() for c in rows[k].strip().strip("|").split("|")]
        tests = re.findall(r"`([^`]+)`", cells[-1]) if cells else []
        if not tests:
            bad.append(f"{k}: cites no test")
        for t in tests:
            if "::" in t:
                path, fn = t.split("::", 1)
                p = os.path.join(ROOT, path)
                if not os.path.isfile(p):
                    bad.append(f"{k}: {path} does not exist")
                elif not re.search(rf"\bfn\s+{re.escape(fn)}\s*\(", open(p).read()):
                    bad.append(f"{k}: {path} defines no fn {fn}")
            elif not os.path.isfile(os.path.join(ROOT, t)):
                bad.append(f"{k}: {t} does not exist")
    if bad:
        print("psv_matrix_check: FAIL")
        for b in bad:
            print("  " + b)
        return 1
    n = sum(len(re.findall(r"`[^`]+`", rows[k].split("|")[-2])) for k in rows)
    print(f"psv_matrix_check: PASS — 40 rows, {n} test citations, all resolve")
    return 0


if __name__ == "__main__":
    sys.exit(main())
