#!/usr/bin/env python3
"""Build `receipt-corpus.json`: the corpus a repair-path receipt is scored on.

PREREG-regress.md amendment 3: the receipt set is three legs, run as one
corpus so one run, one pairing and one verdict cover all of them.

  dev       ../ab/corpus.json: all 60 tasks.
  hard-dev  ../hardset/hard-dev.json: the 25 IN-SCOPE tasks (no single
            function-body edit fixes the 13 out-of-scope ones, so they carry no
            verified outcome to lose).
  tempt     ../tempt/tempt.json: all 30 tasks.

The output is the three COMPACT corpora joined (`ab/corpus.py merge`'s format:
each task keeps its mutant body, `meta.sources` stores each source file once,
`corpus.load` rebuilds and sha-checks every task). Nothing is re-mined or
rebuilt, so each task is byte-for-byte the task of its own set. `meta.legs`
names each leg's task ids; `meta.built_from` the sha256 of every input file.

    python3 build_receipt_corpus.py            write receipt-corpus.json
    python3 build_receipt_corpus.py --check    exit 1 unless the file on disk
                                               is exactly what the inputs give
"""
import hashlib
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = os.path.dirname(HERE)
OUT = os.path.join(HERE, "receipt-corpus.json")
# (leg, input corpus relative to benchmarks/, keep only in-scope tasks)
LEGS = (("dev", "ab/corpus.json", False),
        ("hard-dev", "hardset/hard-dev.json", True),
        ("tempt", "tempt/tempt.json", False))


def build():
    tasks, legs, sources, parts, built, files = [], {}, {}, [], {}, None
    for leg, rel, in_scope_only in LEGS:
        path = os.path.join(BENCH, rel)
        with open(path, "rb") as fh:
            raw = fh.read()
        built[rel] = hashlib.sha256(raw).hexdigest()
        d = json.loads(raw)
        keep = [t for t in d["tasks"] if not in_scope_only or t.get("scope") == "in"]
        if in_scope_only and any(t.get("scope") not in ("in", "out") for t in d["tasks"]):
            raise SystemExit(f"{rel}: a task has no scope")
        legs[leg] = [t["id"] for t in keep]
        tasks += keep
        parts += d["meta"]["parts"]
        files = d["meta"].get("files", files)
        for f, src in d["meta"]["sources"].items():
            if sources.setdefault(f, src) != src:
                raise SystemExit(f"{rel}: source file {f} differs from another leg's copy")
    ids = [t["id"] for t in tasks]
    if len(ids) != len(set(ids)):
        raise SystemExit("a task id appears in two legs")
    used = {t["source_file"] for t in tasks}
    meta = {"parts": parts,
            "sources": {f: sources[f] for f in sorted(used)},
            "files": files, "legs": legs, "built_from": built,
            "note": "built by scorecard/build_receipt_corpus.py; not edited by hand"}
    return json.dumps({"meta": meta, "tasks": tasks}, indent=1) + "\n"


def main(argv):
    text = build()
    if argv[1:] == ["--check"]:
        have = open(OUT).read() if os.path.exists(OUT) else None
        if have != text:
            print("receipt-corpus.json is not what build_receipt_corpus.py gives from its "
                  "inputs: rebuild it (and expect a new receipt corpus sha256)", file=sys.stderr)
            return 1
        return 0
    if argv[1:]:
        print(__doc__, file=sys.stderr)
        return 2
    with open(OUT, "w") as fh:
        fh.write(text)
    d = json.loads(text)
    print(f"{OUT}: {len(d['tasks'])} tasks, " +
          ", ".join(f"{k} {len(v)}" for k, v in d["meta"]["legs"].items()))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
