#!/usr/bin/env python3
"""Drift check for governance/specs/post-c9-hardening.md (non-normative post-C9 backlog).

What it holds, so the document cannot silently lose what it exists to keep:
  1. every signature of the PSV-1 loop triage table (governance/notes/v022-psv1-loop-triage.md)
     is in the spec's traceability table (Part G), and the table names no signature the triage
     table does not have;
  2. a NARROW-CLAIM finding is `narrowed` and names a PH-A item that exists as a heading; a FIX
     finding is `fixed in amendment 117` (or 121, which closed five of the narrowed ones); the two tables agree on the decision;
  3. every `PH-` heading appears in the Part F backlog table and every Part F id is a heading;
  4. the PSV-1 NON-CLAIMS section of the verdict spec names the spec by path;
  5. the spec says it is non-normative.

`--plant KEY:OLD=>NEW` (KEY: spec | triage | verdict) changes the text of one input IN MEMORY
before checking: the control that shows each rule can fail (crates/axon-core/tests/postc9_spec.rs).
Exit 0 and `check_postc9_spec: PASS` on success; 1 and the reasons on stderr otherwise.
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPEC_REL = "governance/specs/post-c9-hardening.md"
PATHS = {
    "spec": os.path.join(ROOT, SPEC_REL),
    "triage": os.path.join(ROOT, "governance/notes/v022-psv1-loop-triage.md"),
    "verdict": os.path.join(ROOT, "governance/specs/v022-protected-suite-verdict.md"),
}
PLANTS = {}


def read(key):
    t = open(PATHS[key]).read()
    for old, new in PLANTS.get(key, []):
        if old not in t:
            print(f"check_postc9_spec: plant for {key} does not match the text: {old[:60]!r}", file=sys.stderr)
            sys.exit(2)
        t = t.replace(old, new, 1)
    return t


def cells(line):
    return [x.strip() for x in line.strip().strip("|").split("|")]


def check():
    bad = []
    spec, tri, verdict = read("spec"), read("triage"), read("verdict")
    if "not normative" not in spec.split("## How to read")[0]:
        bad.append("the spec does not say, before 'How to read', that it is not normative")

    tm = re.search(r"<!-- BEGIN TRIAGE.*?-->\n(.*?)\n<!-- END TRIAGE -->", tri, re.S)
    if not tm:
        return ["the triage table has no BEGIN TRIAGE / END TRIAGE markers"]
    triage = {}
    for line in tm.group(1).splitlines():
        c = cells(line)
        if len(c) >= 4 and c[0].isdigit():
            triage[c[1].strip("`")] = c[3]
    if len(triage) < 53:
        bad.append(f"the triage table has {len(triage)} findings, the loop confirmed 53")

    items = set(re.findall(r"^#### (PH-[A-Z]\d+)\.", spec, re.M))
    if not items:
        bad.append("the spec has no `#### PH-<part><n>.` item headings")

    gm = re.search(r"<!-- BEGIN TRACE.*?-->\n(.*?)\n<!-- END TRACE -->", spec, re.S)
    trace = {}
    if not gm:
        bad.append("the spec has no BEGIN TRACE / END TRACE table")
    else:
        for line in gm.group(1).splitlines():
            c = cells(line)
            if len(c) >= 4 and c[0].isdigit():
                trace[c[1].strip("`")] = (c[2], c[3])
    for sig, dec in sorted(triage.items()):
        if sig not in trace:
            bad.append(f"the triage finding `{sig}` ({dec}) is not in the spec's traceability table")
            continue
        disp, item = trace[sig]
        if dec == "NARROW-CLAIM":
            if disp != "narrowed":
                bad.append(f"`{sig}` is NARROW-CLAIM in the triage table and `{disp}` in the spec")
            if not re.fullmatch(r"PH-A\d+", item) or item not in items:
                bad.append(f"`{sig}` is narrowed and names `{item}`, which is not a PH-A item heading of the spec")
        elif dec == "FIX":
            if disp not in ("fixed in amendment 117", "fixed in amendment 121"):
                bad.append(f"`{sig}` is FIX in the triage table and `{disp}` in the spec")
            if item != "-" and item not in items:
                bad.append(f"`{sig}` names the residual item `{item}`, which is not a heading of the spec")
        else:
            bad.append(f"`{sig}` has the triage decision {dec!r}, which the spec does not trace")
    for sig in sorted(set(trace) - set(triage)):
        bad.append(f"the spec's traceability table names `{sig}`, which the triage table does not have")

    fm = re.search(r"## Part F\..*?\n(.*?)\n## Part G", spec, re.S)
    backlog = set()
    if not fm:
        bad.append("the spec has no Part F backlog table")
    else:
        for line in fm.group(1).splitlines():
            c = cells(line)
            if c and re.fullmatch(r"PH-[A-Z]\d+", c[0]):
                backlog.add(c[0])
    for i in sorted(items - backlog):
        bad.append(f"the item {i} is not in the Part F backlog table")
    for i in sorted(backlog - items):
        bad.append(f"the Part F backlog table names {i}, which is not an item heading")

    nm = re.search(r"- \*\*NOT claimed\.\*\*(.*?)- \*\*What an honest suite must do", verdict, re.S)
    if not nm:
        bad.append("the verdict spec has no PSV-1 'NOT claimed' section followed by 'What an honest suite must do'")
    elif SPEC_REL not in nm.group(1):
        bad.append(f"the PSV-1 non-claims section does not reference {SPEC_REL}")
    return bad


def main():
    args = sys.argv[1:]
    while "--plant" in args:
        i = args.index("--plant")
        key, _, rest = args[i + 1].partition(":")
        old, sep, new = rest.partition("=>")
        if not sep or key not in PATHS:
            print("check_postc9_spec: --plant KEY:OLD=>NEW with KEY in spec|triage|verdict", file=sys.stderr)
            return 2
        PLANTS.setdefault(key, []).append((old, new))
        del args[i : i + 2]
    if args not in ([], ["--check"]):
        print("usage: check_postc9_spec.py [--check] [--plant KEY:OLD=>NEW]", file=sys.stderr)
        return 2
    bad = check()
    if bad:
        for b in bad:
            print("check_postc9_spec: " + b, file=sys.stderr)
        return 1
    print("check_postc9_spec: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
