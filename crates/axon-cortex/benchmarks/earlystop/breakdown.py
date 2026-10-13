#!/usr/bin/env python3
"""Where Cortex's wall time goes on the tasks it cannot fix, and what each
early-stop signal would have saved on those rows.

Reads rows written by `run_ab.py` (arm B). Rows from before this work have no
`early_stop` block; the signal columns are then left out and only the time
breakdown is printed.

  python3 breakdown.py HARD_DEV.jsonl TEMPT.jsonl [--simulate MEASURE_HD.jsonl MEASURE_TEMPT.jsonl]

The simulation is IN-SAMPLE, on the rows the rule was designed from, and is a
design aid, not a result: the result is the paid run in PREREG-earlystop.md.
It replays one row: a candidate stops being asked after its Nth consecutive
inert proposal (a compiled, refused proposal that left the visible failing
set exactly as it was before any proposal) and a candidate that is not in
`reached_by_all` is never asked. The time saved is the sum of the recorded
per-proposal API durations of the proposals that would not have been made.
"""
import json, sys


def load(p):
    return [json.loads(l) for l in open(p) if l.strip()]


def is_oos(r):
    return r["id"].startswith("hd-out") or r["id"].startswith("tempt-unsat")


def breakdown(rows, label):
    n = len(rows)
    wall = sum(r["wall_s"] for r in rows)
    api = sum(r["api_ms"] for r in rows) / 1000
    props = sum(r["proposals"] for r in rows)
    first = first_t = 0
    for r in rows:
        seen = set()
        for q in r["per_proposal"]:
            if q["candidate"] not in seen:
                seen.add(q["candidate"])
                first += 1
                first_t += q["duration_api_ms"] / 1000
    cands = sum(len(set(r["attempted"])) for r in rows)
    print(
        f"{label}: n={n} wall={wall:.1f}s api={api:.1f}s other={wall - api:.1f}s "
        f"proposals={props} candidates={cands} | first proposal of a candidate: "
        f"{first} ({first_t:.1f}s) | later proposals: {props - first} ({api - first_t:.1f}s)"
    )


def simulate(r, n_inert, prune):
    """Seconds of API time the rule would have saved on one row."""
    es = r.get("early_stop")
    if not es:
        return 0.0, 0
    base = set(es["visible_failing"] or [])
    reach = es["reached_by_all"] or []
    per = {}
    for q in r["per_proposal"]:
        per.setdefault(q["candidate"], []).append(q["duration_api_ms"] / 1000)
    att = {}
    for a in es["attempts"]:
        att.setdefault(a["candidate"], []).append(a)
    saved = 0.0
    skipped = 0
    for c, times in per.items():
        if prune and c not in reach:
            saved += sum(times)
            skipped += len(times)
            continue
        if not n_inert:
            continue
        streak = 0
        for j, t in enumerate(times):
            if streak >= n_inert:
                saved += t
                skipped += 1
                continue
            a = att.get(c, [])
            a = a[j] if j < len(a) else None
            if a and a["rejection"] == "check_refused" and a["visible_failing"] is not None:
                if base and set(a["visible_failing"]) == base:
                    streak += 1
                else:
                    streak = 0
    if prune and not reach and not per:
        pass
    return saved, skipped


def main():
    args = sys.argv[1:]
    sim = []
    if "--simulate" in args:
        i = args.index("--simulate")
        sim = args[i + 1:]
        args = args[:i]
    hd, tp = (load(p) for p in args[:2])
    for label, rows in (("hard-dev", hd), ("tempt", tp)):
        oos = [r for r in rows if is_oos(r)]
        ins = [r for r in rows if not is_oos(r)]
        breakdown(oos, f"{label} out-of-scope/unsat")
        breakdown(ins, f"{label} in-scope/satisfiable")
    for p in sim:
        rows = load(p)
        oos = [r for r in rows if is_oos(r)]
        ins = [r for r in rows if not is_oos(r)]
        tot = sum(r["wall_s"] for r in oos)
        for n_inert, prune in ((0, True), (1, False), (2, False), (3, False), (1, True), (2, True), (3, True)):
            s = sum(simulate(r, n_inert, prune)[0] for r in oos)
            k = sum(simulate(r, n_inert, prune)[1] for r in oos)
            si = sum(simulate(r, n_inert, prune)[0] for r in ins)
            ki = sum(simulate(r, n_inert, prune)[1] for r in ins)
            print(
                f"{p.split('/')[-1]:18} inert={n_inert} prune_to_reach={prune!s:5} "
                f"saves {s:6.1f}s of {tot:6.1f}s ({100 * s / tot:4.1f}%), {k:3d} proposals | "
                f"in-scope/sat: would skip {ki} proposals ({si:.1f}s)"
            )


if __name__ == "__main__":
    main()
