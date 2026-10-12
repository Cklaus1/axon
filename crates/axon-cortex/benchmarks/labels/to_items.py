#!/usr/bin/env python3
"""Turn `cortex repair --label-log` rows into localization benchmark items.

    python3 to_items.py convert LABELS.jsonl [--out DIR]   # -> DIR/locrank_items.jsonl,
                                                           #    DIR/decision_loc.jsonl,
                                                           #    DIR/summary.json
    python3 to_items.py selftest                           # $0, no network, no binaries

A `cortex-label/1` row whose outcome is `verified_done` names, in `truth`, the
function whose new body the hidden check accepted. With the pre-run source
(`--label-source`) that is a localization question with a checked answer:

  locrank_items.jsonl  {"id", "task", "loc"}: `task` and `loc` have the shapes
                       `../locrank/locrank.py` reads from `ab/corpus.json` and
                       `results/locate.jsonl`, so `locrank.rank_task(task, loc,
                       call, provider)` and `locrank.ranker_prompt(task,
                       loc["eligible"])` take them unchanged.
  decision_loc.jsonl   DecisionBench `loc` items, the shape of `cat == "loc"`
                       in crates/axon-reflex/benchmarks/decisions/decision_set.jsonl.

Every row that does not become an item is COUNTED with its reason in
summary.json (and on stderr); none is dropped silently. Stdlib only.
"""
import argparse
import collections
import hashlib
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(BENCH, "ab"))
sys.path.insert(0, BENCH)
from common import fn_span  # noqa: E402
from corpus import cut_test, test_blocks  # noqa: E402

SCHEMA = "cortex-label/1"
# DecisionBench: a state above this many characters of source is cut to an
# excerpt (the candidates and the failing test). Same value and rule as
# crates/axon-reflex/benchmarks/decisions/build_set.py FULL_SOURCE_MAX.
FULL_SOURCE_MAX = 12000
LOC_INSTR = ("Exactly one candidate function in `source` contains an injected bug that makes "
             "`failing_test` fail. Which candidate contains the bug?")


def sha256(s):
    return hashlib.sha256(s.encode() if isinstance(s, str) else s).hexdigest()


# ------------------------------------------- source views (as build_set.py)

def fn_text(src, name):
    """`fn name(...) {...}` with the comment and attribute lines directly above it."""
    m = re.search(r"^[ \t]*(?:pub[ \t]+)?fn %s\(" % re.escape(name), src, re.M)
    span = fn_span(src, name)
    if not m or not span:
        raise KeyError(name)
    start = m.start()
    while True:
        prev = src.rfind("\n", 0, start - 1) + 1 if start > 0 else 0
        line = src[prev:start].strip()
        if start > 0 and (line.startswith("//") or line.startswith("@[")):
            start = prev
        else:
            break
    return src[start:span[1] + 1]


def signature(src, name):
    m = re.search(r"^[ \t]*((?:pub[ \t]+)?fn %s\([^{]*)" % re.escape(name), src, re.M)
    return " ".join(m.group(1).split()) if m else f"fn {name}"


def source_view(src, names, test_name):
    """(text, note). The whole file when small, else the named functions and the test."""
    if len(src) <= FULL_SOURCE_MAX:
        return src, "complete file (hidden check removed)"
    parts = [fn_text(src, n) for n in names]
    blocks = {n: src[a:b] for n, a, b in test_blocks(src)}
    parts.append(blocks[test_name].rstrip())
    return ("\n\n".join(parts) + "\n",
            f"EXCERPT of a {len(src)}-character file: only the functions named here "
            "and the failing test are shown")


# ------------------------------------------------------------- one row

class Skip(Exception):
    """A row that cannot become an item, with the reason it is counted under."""


def usable(row):
    """(source, given_a, visible, loc_view) for a row that can label a
    localization question, else raises Skip(reason)."""
    if row.get("schema") != SCHEMA:
        raise Skip(f"schema is {row.get('schema')!r}, not {SCHEMA}")
    if row.get("outcome") != "verified_done" or row.get("truth") is None:
        raise Skip(f"no truth: outcome {row.get('outcome')}")
    if row.get("localization") is None:
        raise Skip("no localization: the run was given --symbol")
    src = row.get("source")
    if src is None:
        raise Skip("no source: the run did not pass --label-source")
    if sha256(src) != row.get("file_sha256"):
        raise Skip("source does not match file_sha256")
    loc = row["localization"]
    truth = row["truth"]
    if truth not in loc["eligible"]:
        raise Skip("truth outside the eligible set")
    try:
        given_a, _ = cut_test(src, row["check"])
    except KeyError:
        raise Skip("the adjudicating check is not a test block in the source")
    blocks = {n for n, _, _ in test_blocks(given_a)}
    visible = next((f for f in loc["failing"] if f in blocks), None)
    if visible is None:
        raise Skip("no visible failing check is a test block in the source")
    return src, given_a, visible, loc


def item_id(row):
    return f"label-{row['file_sha256'][:12]}-{row['check']}"


def locrank_item(row, src, given_a, visible, loc):
    ranked = [(r["symbol"], r["score"]) for r in loc["ranked"]]
    names = [s for s, _ in ranked]
    truth = row["truth"]
    rank = names.index(truth) + 1 if truth in names else 0
    top = ranked[0][1] if ranked else None
    tscore = ranked[rank - 1][1] if rank else None
    iid = item_id(row)
    task = {
        "id": iid, "class": "field", "file": os.path.basename(row["file"]),
        "symbol": truth, "visible": visible, "hidden": row["check"],
        "broken": src, "given_a": given_a,
    }
    locv = {
        "id": iid, "cls": "field", "truth": truth,
        "ranked": [{"symbol": s, "score": sc} for s, sc in ranked],
        "eligible": list(loc["eligible"]),
        "spectrum_rank": rank, "in_eligible": 0 < rank <= len(loc["eligible"]),
        "tied_at_top": sum(1 for _, sc in ranked if sc == top),
        "tied_with_truth": sum(1 for _, sc in ranked if sc == tscore) if rank else 0,
        "failing_visible_to_cortex": list(loc["failing"]),
    }
    return {"id": iid, "task": task, "loc": locv, "provenance": provenance(row)}


def provenance(row):
    audit = row.get("audit") or {}
    return {
        "label": (f"cortex repair verified_done: hidden check `{row['check']}` accepted a new "
                  f"body of `{row['truth']}` (cortex-label/1 row {row['ts']})"),
        "candidates": f"cortex repair localization `eligible` (spectrum top-{row['localization']['candidates']})",
        "file": row["file"], "file_sha256": row["file_sha256"],
        "cortex": row.get("cortex"), "attempted": row.get("attempted"),
        "ranker": (row.get("ranker") or {}).get("used"),
        "audit": audit.get("verdict"),
    }


def decision_item(row, src, given_a, visible, loc):
    el = loc["eligible"]
    if len(el) < 2:
        raise Skip("single candidate: not a decision")
    blocks = {n: given_a[a:b] for n, a, b in test_blocks(given_a)}
    text, note = source_view(given_a, el, visible)
    names = [r["symbol"] for r in loc["ranked"]]
    prov = provenance(row)
    prov["spectrum_rank"] = names.index(row["truth"]) + 1
    prov["class"] = "field"
    return {
        "id": f"loc/{item_id(row)}", "cat": "loc", "qid": "buggy_function", "qtype": "choice",
        "state": {"file": os.path.basename(row["file"]), "source_note": note, "source": text,
                  "failing_test": {"name": visible, "code": blocks[visible].rstrip()}},
        "question": {"type": "choice", "instructions": LOC_INSTR,
                     "criteria": {c: signature(given_a, c) for c in el}},
        "label": row["truth"],
        "incumbent": {"name": "cortex locate spectrum", "order": list(el)},
        "provenance": prov,
    }


# --------------------------------------------------------------- convert

def convert(rows):
    """(locrank_items, decision_items, summary). `rows` are parsed JSON objects
    or `(line_no, error)` tuples for lines that were not JSON."""
    lr, dl = [], []
    skipped = {"locrank": collections.Counter(), "decision_loc": collections.Counter()}
    seen = set()
    for row in rows:
        if isinstance(row, tuple):
            for k in skipped:
                skipped[k][f"not JSON: {row[1]}"] += 1
            continue
        try:
            src, given_a, visible, loc = usable(row)
            key = (row["file_sha256"], row["check"])
            if key in seen:
                raise Skip("duplicate of an earlier row (same file_sha256 and check)")
            seen.add(key)
        except Skip as e:
            for k in skipped:
                skipped[k][str(e)] += 1
            continue
        lr.append(locrank_item(row, src, given_a, visible, loc))
        try:
            dl.append(decision_item(row, src, given_a, visible, loc))
        except Skip as e:
            skipped["decision_loc"][str(e)] += 1
    summary = {
        "rows": len(rows), "locrank_items": len(lr), "decision_loc_items": len(dl),
        "skipped": {k: dict(sorted(v.items())) for k, v in skipped.items()},
    }
    return lr, dl, summary


def read_rows(path):
    rows = []
    with open(path) as fh:
        for i, line in enumerate(fh, 1):
            if not line.strip():
                continue
            try:
                v = json.loads(line)
            except ValueError as e:
                rows.append((i, f"line {i}: {e.msg}"))
                continue
            rows.append(v if isinstance(v, dict) else (i, f"line {i}: not an object"))
    return rows


def write_jsonl(path, items):
    with open(path, "w") as fh:
        for x in items:
            fh.write(json.dumps(x, sort_keys=True) + "\n")


def cmd_convert(a):
    lr, dl, summary = convert(read_rows(a.labels))
    summary["source"] = {"path": a.labels, "sha256": sha256(open(a.labels, "rb").read())}
    os.makedirs(a.out, exist_ok=True)
    write_jsonl(os.path.join(a.out, "locrank_items.jsonl"), lr)
    write_jsonl(os.path.join(a.out, "decision_loc.jsonl"), dl)
    with open(os.path.join(a.out, "summary.json"), "w") as fh:
        json.dump(summary, fh, indent=1, sort_keys=True)
        fh.write("\n")
    print(json.dumps(summary, indent=1, sort_keys=True))
    for k, reasons in summary["skipped"].items():
        for reason, n in reasons.items():
            print(f"{k}: skipped {n} row(s): {reason}", file=sys.stderr)


# --------------------------------------------------------------- selftest

SELFTEST_SRC = """\
fn alpha(n: i64) -> i64 {
    n * 2
}

fn beta(n: i64) -> i64 {
    n + 1
}

@[test]
fn visible() {
    assert_eq(alpha(1) + beta(1), 5)
}

@[test]
fn hidden_completion() {
    assert_eq(beta(5), 7)
    assert_eq(alpha(5), 10)
}
"""


def selftest_row(**over):
    row = {
        "schema": SCHEMA, "ts": "2026-10-12T00:00:00Z",
        "cortex": {"version": "0.1.0", "sha256": None},
        "file": "sub/ambiguous.ax", "file_sha256": sha256(SELFTEST_SRC),
        "check": "hidden_completion", "symbol": None, "generator": "literal", "budget": 8,
        "localization": {"failing": ["visible"], "passing": [],
                         "ranked": [{"symbol": "alpha", "score": 1.0},
                                    {"symbol": "beta", "score": 1.0}],
                         "candidates": 3, "eligible": ["alpha", "beta"]},
        "ranker": {"requested": "spectrum", "used": "spectrum", "reason": None,
                   "order": ["alpha", "beta"], "latency_ms": None, "cost_usd": 0.0},
        "attempted": ["alpha", "beta"], "outcome": "verified_done", "exit_code": 0, "steps": 2,
        "truth": "beta",
        "proposals": [{"symbol": "alpha", "proposals": 1, "steps": 1},
                      {"symbol": "beta", "proposals": 1, "steps": 1}],
        "audit": None, "source": SELFTEST_SRC,
    }
    row.update(over)
    return row


def cmd_selftest(_a):
    sys.path.insert(0, os.path.join(BENCH, "locrank"))
    import locrank  # noqa: E402  (its rank_task/ranker_prompt are the consumers)

    good = selftest_row()
    one = selftest_row(file_sha256=sha256(SELFTEST_SRC + "\n"), source=SELFTEST_SRC + "\n",
                       localization={**good["localization"], "eligible": ["beta"],
                                     "ranked": [{"symbol": "beta", "score": 1.0}]})
    rows = [
        good,
        selftest_row(),                                           # duplicate
        selftest_row(source=None),                                # no --label-source
        selftest_row(outcome="no_progress", exit_code=21, truth=None),
        selftest_row(symbol="beta", localization=None),
        selftest_row(source=SELFTEST_SRC.replace("n + 1", "n + 2")),  # digest mismatch
        selftest_row(schema="cortex-label/0"),
        one,                                                      # locrank only
        (9, "line 9: Expecting value"),
    ]
    lr, dl, s = convert(rows)
    checks = []

    def ok(name, cond):
        checks.append((name, bool(cond)))

    ok("rows counted", s["rows"] == 9)
    ok("two locrank items", s["locrank_items"] == 2)
    ok("one decision item", s["decision_loc_items"] == 1)
    ok("every non-item row has a reason",
       sum(s["skipped"]["locrank"].values()) == 9 - 2 and
       sum(s["skipped"]["decision_loc"].values()) == 9 - 1)
    sk = s["skipped"]["decision_loc"]
    for reason in ("no source: the run did not pass --label-source",
                   "no truth: outcome no_progress",
                   "no localization: the run was given --symbol",
                   "source does not match file_sha256",
                   "duplicate of an earlier row (same file_sha256 and check)",
                   "single candidate: not a decision",
                   "schema is 'cortex-label/0', not cortex-label/1",
                   "not JSON: line 9: Expecting value"):
        ok(f"skip reason {reason!r}", sk.get(reason) == 1)
    it = lr[0]
    ok("hidden check cut from given_a", "hidden_completion" not in it["task"]["given_a"])
    ok("broken is the full source", it["task"]["broken"] == SELFTEST_SRC)
    ok("truth and spectrum rank", it["loc"]["truth"] == "beta" and it["loc"]["spectrum_rank"] == 2)
    ok("tie recorded", it["loc"]["tied_at_top"] == 2 and it["loc"]["tied_with_truth"] == 2)
    prompt = locrank.ranker_prompt(it["task"], it["loc"]["eligible"])
    ok("locrank builds its prompt from the item", "Failing test: visible" in prompt
       and "hidden_completion" not in prompt)
    for kind, want in (("oracle", 1), ("reverser", 1), ("adversary", 2)):
        call, _ = locrank.scripted(kind)
        r = locrank.rank_task(it["task"], it["loc"], call, f"scripted:{kind}")
        ok(f"locrank.rank_task scripted:{kind} final_rank {want}", r["final_rank"] == want)
    d = dl[0]
    ok("decision item shape", set(d) == {"id", "cat", "qid", "qtype", "state", "question",
                                         "label", "incumbent", "provenance"}
       and d["cat"] == "loc" and d["label"] == "beta"
       and set(d["question"]["criteria"]) == {"alpha", "beta"}
       and d["state"]["failing_test"]["name"] == "visible"
       and "hidden_completion" not in json.dumps(d["state"]))
    ok("decision criteria are signatures",
       d["question"]["criteria"]["beta"] == "fn beta(n: i64) -> i64")
    bad = [n for n, c in checks if not c]
    for n, c in checks:
        print(f"{'ok  ' if c else 'FAIL'} {n}")
    print(f"selftest: {len(checks) - len(bad)}/{len(checks)} passed")
    if bad:
        raise SystemExit(1)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sp = ap.add_subparsers(dest="cmd", required=True)
    c = sp.add_parser("convert")
    c.add_argument("labels", help="a cortex-label/1 JSONL file (--label-log output)")
    c.add_argument("--out", default="items", help="output directory (default: ./items)")
    c.set_defaults(fn=cmd_convert)
    sp.add_parser("selftest").set_defaults(fn=cmd_selftest)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
