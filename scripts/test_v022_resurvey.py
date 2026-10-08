#!/usr/bin/env python3
"""The re-survey record's own refusals (amendment 107), on synthetic records: no cargo, no survey.

A record the FREEZE trusts must be for this head, from a clean tree, recent, drawn by the stated rule over
the entries the gate has NOW, with no survivor. Each case below is an ATTACK (one defect, and the record
must be refused for that reason) or the CONTROL (the good record holds).

    python3 scripts/test_v022_resurvey.py
"""
import copy
import hashlib
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
import v022_resurvey as rs  # noqa: E402

rc, mut = rs.load("v022_refusal_coverage"), rs.load("v022_g01_mutations")
HEAD = rs.head()
NOW = 1_800_000_000


def good(pct=25):
    ents, chosen, pct, total, est = rs.plan(rc, str(pct), HEAD)
    entries, fam = [], {}
    for k, v in ents.items():
        rows = [{"key": key, "family": k, "result": "KILLED"} for key, _ in chosen[k]]
        entries += rows
        fam[k] = {"observed": len(v), "drawn": len(rows), "killed": len(rows), "survived": [], "not_remeasured": 0}
    return {"schema": rs.SCHEMA, "commit": HEAD, "tree_clean": True,
            "gate_sha256": hashlib.sha256(open(os.path.join(ROOT, "scripts/v022_refusal_coverage.py"), "rb").read()).hexdigest(),
            "sample": {"pct": pct, "salt": HEAD, "mode": str(pct), "rule": ""},
            "estimate_seconds": est, "seconds": 1.0, "finished_unix": NOW - 3600, "host": "t",
            "families": fam, "entries": entries}


def problems(doc):
    return rs.problems(doc, HEAD, rc, mut, now=NOW)


def attack(name, edit, want):
    d = copy.deepcopy(good())
    edit(d)
    got = problems(d)
    if not any(want in g for g in got):
        print(f"ATTACK: {name}: the record was not refused for {want!r}: {got}")
        return 1
    return 0


def main():
    bad = 0
    p = problems(good())
    if p:
        print(f"control: the good record must hold: {p}")
        bad += 1
    bad += attack("a record for another commit", lambda d: d.update(commit="0" * 40), "commit")
    bad += attack("a dirty tree", lambda d: d.update(tree_clean=False), "clean tree")
    bad += attack("an old record", lambda d: d.update(finished_unix=NOW - 40 * 86400), "days old")
    bad += attack("another version of the gate", lambda d: d.update(gate_sha256="0" * 64), "another version of the gate")
    bad += attack("a sample below the floor", lambda d: d["sample"].update(pct=5), "at least")
    bad += attack("a different OBSERVED set", lambda d: d["families"]["value"].update(observed=1), "OBSERVED entries, the gate now has")
    bad += attack("a survivor", lambda d: (d["families"]["value"].update(survived=["x"]), None), "now SURVIVE")
    bad += attack("entries the rule does not draw", lambda d: d["entries"].pop(), "not the ones its sample rule draws")
    bad += attack("counts that do not match the entries", lambda d: d["families"]["py"].update(killed=999), "counts 999 killed")
    bad += attack("another schema", lambda d: d.update(schema="x"), "not a")
    # the sample is a function of (salt, key, pct): the same head draws the same entries, another head another
    a = [k for k, _ in rs.plan(rc, "25", "a" * 40)[1]["value"]]
    b = [k for k, _ in rs.plan(rc, "25", "b" * 40)[1]["value"]]
    if a == b or not a:
        print("ATTACK: the sample did not rotate with the head's salt, or drew nothing")
        bad += 1
    if rs.plan(rc, "all", HEAD)[2] != 100 or rs.plan(rc, "auto", HEAD)[2] != 25:
        print("ATTACK: `auto` is not 100% under the budget and 25% over it (estimate %s s)" % rs.plan(rc, "auto", HEAD)[4])
        bad += 1
    print("test_v022_resurvey:", "FAIL" if bad else "PASS")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
