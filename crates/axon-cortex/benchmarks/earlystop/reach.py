#!/usr/bin/env python3
"""Zero-cost check of the structural early-stop signal.

For every task of a set, write the broken file to a temp workspace, run
`cortex locate --json` and read `reached_by_all`: the non-test functions that
EVERY visible failing check reaches (the hidden check is excluded, as
`locate` always does). When that list is empty, no single function body is
reached by all the failing checks.

Reports, per set and scope: how many tasks have an empty list, and for the
in-scope tasks (whose planted defect is one function body) whether the
planted symbol is in the list.

  python3 reach.py [--cortex BIN] [--axon BIN] [--out reach.json] SET.json ...
"""
import argparse, json, os, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "ab"))
import corpus  # noqa: E402


def locate(cortex, axon, task):
    with tempfile.TemporaryDirectory(dir=os.environ.get("TMPDIR")) as ws:
        open(os.path.join(ws, task["file"]), "w").write(task["broken"])
        p = subprocess.run([cortex, "locate", "--workspace", ws, "--file", task["file"],
                            "--check", task["hidden"], "--axon", axon, "--json"],
                           capture_output=True, text=True, timeout=300)
    for line in reversed(p.stdout.strip().splitlines()):
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if isinstance(v, dict) and v.get("schema") == "cortex-locate/1":
            return v
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("sets", nargs="+")
    ap.add_argument("--cortex", default=os.environ.get("CORTEX_BIN", "cortex"))
    ap.add_argument("--axon", default=os.environ.get("AXON_BIN", "axon"))
    ap.add_argument("--out")
    a = ap.parse_args()
    rows = []
    for path in a.sets:
        for t in corpus.load(path)["tasks"]:
            v = locate(a.cortex, a.axon, t)
            scope = t.get("scope", "in")
            row = {"set": os.path.basename(path), "id": t["id"], "scope": scope,
                   "shape": t.get("shape") or t.get("class"), "symbol": t["symbol"],
                   "failing": v and v["failing"], "reached_by_all": v and v["reached_by_all"],
                   "ranked": v and [r["symbol"] for r in v["ranked"]]}
            row["empty"] = bool(v) and not v["reached_by_all"]
            row["truth_in_common"] = bool(v) and t["symbol"] in v["reached_by_all"]
            rows.append(row)
    if a.out:
        json.dump(rows, open(a.out, "w"), indent=1)
    keys = sorted({(r["set"], r["scope"]) for r in rows})
    for k in keys:
        rs = [r for r in rows if (r["set"], r["scope"]) == k]
        line = f"{k[0]:16} {k[1]:3} n={len(rs):3} empty={sum(r['empty'] for r in rs):3}"
        if k[1] == "in":
            line += f" truth_in_common={sum(r['truth_in_common'] for r in rs):3}"
        print(line)
    for r in rows:
        if (r["scope"] == "in" and not r["truth_in_common"]) or (r["scope"] == "out" and not r["empty"]):
            print("  note:", r["set"], r["id"], r["scope"], "failing", r["failing"],
                  "common", r["reached_by_all"])


if __name__ == "__main__":
    main()
