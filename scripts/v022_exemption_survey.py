#!/usr/bin/env python3
"""Cheap-kill survey of refusal-coverage exemptions (amendment 91).

An exemption that argues a guard is unreachable, unobservable or not a verdict
property is a CLAIM, and round 7 found five of fifty sampled to be wrong (a test
nobody wrote kills the guard). This tries the cheapest kill for every exemption
of a file set: it finds the exemption's site, takes its guard opener (the line
the gate itself uses), and if that line is an `if COND {` rewrites it to
`if false && (COND) {`, runs the crate's tests, and restores the file. A test
that FAILS is a kill: the exemption is wrong and the site needs a row. A suite
that stays green is the evidence the exemption needs (the claim survived an
attempt to refute it); a non-`if` opener or a build break is reported as such.

    python3 scripts/v022_exemption_survey.py OUT.json FILE... -- cargo-test-args

Run from a clean clone; edits files in place and restores them.
"""
import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import v022_refusal_coverage as rc  # noqa: E402


def main():
    out, rest = sys.argv[1], sys.argv[2:]
    split = rest.index("--")
    files, cargo = rest[:split], rest[split + 1:]
    results = []
    for f, anchor, reason in rc.EXEMPT:
        if f not in files:
            continue
        path = os.path.join(rc.ROOT, f)
        text = open(path).read()
        if text.count(anchor) != 1:
            continue
        lines = rc.code_lines(text)
        at = rc.line_of(text, text.index(anchor))
        # The site the exemption anchors: the first scanned site whose block holds the anchor.
        site = next(((g, i) for g, i, _, k in rc.sites(text, f, []) if g <= at <= i and k == "line"), None)
        if site is None:
            results.append({"file": f, "anchor": anchor[:60], "result": "no line site"})
            continue
        g, _ = site
        raw = text.split("\n")
        m = re.match(r"^(\s*)(\}\s*else\s+)?if\s+(?!let\b)(.*)\s\{\s*$", raw[g])
        if not m:
            results.append({"file": f, "line": g + 1, "anchor": anchor[:60], "result": "opener is not `if COND {`: " + raw[g].strip()[:60]})
            continue
        new = f"{m.group(1)}{m.group(2) or ''}if false && ({m.group(3)}) {{"
        mutated = "\n".join(raw[:g] + [new] + raw[g + 1:])
        open(path, "w").write(mutated)
        try:
            try:
                r = subprocess.run(["cargo", "test", "-q", *cargo], capture_output=True, text=True, timeout=1200)
            except subprocess.TimeoutExpired as e:
                r = subprocess.CompletedProcess([], 124, (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or ""), "hung: the mutated build ran past its bound (a test waited for what the guard refused)")
        finally:
            open(path, "w").write(text)
        tail = (r.stdout + r.stderr)
        failing = sorted(set(re.findall(r"^---- (\S+) stdout ----$", tail, re.M)))
        binary = re.findall(r"Running (?:unittests )?(\S+)", tail)
        built = "could not compile" not in tail and "error[E" not in tail
        results.append({"file": f, "line": g + 1, "anchor": anchor[:60], "opener": raw[g].strip()[:80],
                        "result": ("KILLED (a test failed: the exemption is wrong)" if r.returncode != 0 and built
                                   else "build broke" if not built else "survived (the claim held)"),
                        "failing_tests": failing[:6], "reason_kind": reason[:40]})
        print(results[-1]["result"], f, g + 1, flush=True)
    json.dump(results, open(out, "w"), indent=1)


if __name__ == "__main__":
    main()
