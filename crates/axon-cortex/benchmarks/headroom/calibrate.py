#!/usr/bin/env python3
"""Calibrate the hx recipe on dev and select hx-test by cell membership only
(spec FU-1c steps 3-4).

    python3 calibrate.py --pool hx-pool.json --rows dev-h55.jsonl [...] \\
        [--rows-h45 dev-h45.jsonl] --seed N --cap-seed N [--cap 8] \\
        --report calibration.json [--write-sets DIR]

Input: run_ab k-trial rows on DEV pool tasks (`id`, `trial`, `verified_clean`)
and the pool corpus written by build.py (each task carries `class`, `knob`,
`program`, `split`). A row for a test task is refused: no test task is ever
run before the freeze.

* **Cells** are (class, knob), keyed `class:knob`. A cell is ELIGIBLE with
  >= 12 dev tasks from >= 3 dev programs.
* **Shrinkage.** Each cell's evidence is its task count N_c and the sum S_c of
  its tasks' k-trial rates (a task, not a trial, is the unit: trials of one
  task are not independent). A beta prior is fitted across all cells by
  maximising the beta-binomial marginal likelihood on a fixed (mean,
  precision) grid; a cell's shrunk rate is (a + S_c) / (a + b + N_c).
* **Kept** cells are eligible cells whose shrunk rate is in [0.3, 0.7].
* **Cross-fit.** Dev programs are split 2-fold by a seeded shuffle. On fold A
  the prior is refitted and the eligible cells whose fold-A shrunk rate is in
  the band are selected; their raw task-mean rate on fold B is the
  out-of-fold rate; then the reverse. The predicted test rate is the mean of
  the out-of-fold rates weighted by each cell's TEST pool count.
* **hx-dev** = dev tasks with mixed outcomes (0 < successes < trials).
* **Test selection** = test pool tasks in a kept cell, then at most `--cap`
  per program by build.py's seeded draw. Outcomes play no part.
* **h45 check** (FU-1h iii): the k = 1 rate of the second model on the kept
  dev cells' tasks; informative iff it is in [0.1, 0.9].
"""
import argparse
import json
import math
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import build  # noqa: E402  (pure helpers only: cap_per_program)
from stats import read_rows, task_rates  # noqa: E402

BAND = (0.3, 0.7)
MIN_TASKS, MIN_PROGRAMS = 12, 3
H45_BAND = (0.1, 0.9)
MEANS = [i / 100 for i in range(1, 100)]
PRECISIONS = [0.5 * 1.15 ** i for i in range(60)]  # 0.5 .. ~1900


def cell_of(t):
    return f"{t['class']}:{t.get('knob', '-')}"


def task_counts(rows, exclude_infra=True):
    """{id: [successes, trials]} (stats.task_rates: a repeated (id, trial) is
    refused; rows flagged as infrastructure failures are left out)."""
    return {i: [s, n] for i, (s, n) in task_rates(rows, exclude_infra).items()}


def _lbeta(a, b):
    return math.lgamma(a) + math.lgamma(b) - math.lgamma(a + b)


def eb_fit(evidence):
    """(a, b) maximising sum_c log BB(S_c | N_c, a, b) over the grid; the
    first maximum in grid order wins, so the fit is deterministic."""
    ev = [(s, n) for s, n in evidence if n > 0]
    if not ev:
        return 1.0, 1.0
    best = None
    for m in MEANS:
        for prec in PRECISIONS:
            a, b = m * prec, (1 - m) * prec
            ll = sum(_lbeta(s + a, n - s + b) - _lbeta(a, b) for s, n in ev)
            if best is None or ll > best[0] + 1e-12:
                best = (ll, a, b)
    return best[1], best[2]


def cell_evidence(ids, meta, counts):
    """{cell: {"S", "N", "programs"}} over the given task ids."""
    out = {}
    for i in ids:
        c = out.setdefault(cell_of(meta[i]), {"S": 0.0, "N": 0, "programs": set()})
        s, k = counts[i]
        c["S"] += s / k
        c["N"] += 1
        c["programs"].add(meta[i]["program"])
    return out


def shrink(ev, a, b):
    return {c: (a + e["S"]) / (a + b + e["N"]) for c, e in ev.items()}


def in_band(x, band=BAND):
    return band[0] <= x <= band[1]


def dev_ids(meta, counts):
    bad = sorted(i for i in counts if i not in meta or meta[i].get("split") != "dev")
    if bad:
        raise ValueError(f"rows for tasks that are not dev pool tasks (never run test): {bad[:5]}")
    return sorted(i for i in counts if counts[i][1] > 0)


def crossfit(meta, counts, ids, eligible, test_weight, seed, band=BAND):
    progs = sorted({meta[i]["program"] for i in ids})
    random.Random(seed).shuffle(progs)
    folds = [sorted(progs[0::2]), sorted(progs[1::2])]
    out, num, den = [], 0.0, 0
    for f in (0, 1):
        sel = [i for i in ids if meta[i]["program"] in folds[f]]
        oof = [i for i in ids if meta[i]["program"] in folds[1 - f]]
        ev = cell_evidence(sel, meta, counts)
        a, b = eb_fit([(e["S"], e["N"]) for e in ev.values()])
        shr = shrink(ev, a, b)
        kept = sorted(c for c in eligible if c in shr and in_band(shr[c], band))
        ev_oof = cell_evidence(oof, meta, counts)
        cells = {}
        for c in kept:
            if c in ev_oof:
                r = ev_oof[c]["S"] / ev_oof[c]["N"]
                w = test_weight.get(c, 0)
                cells[c] = {"oof_rate": r, "oof_tasks": ev_oof[c]["N"], "test_weight": w}
                num += w * r
                den += w
            else:
                cells[c] = {"oof_rate": None, "oof_tasks": 0, "test_weight": test_weight.get(c, 0)}
        out.append({"programs": folds[f], "prior": [a, b], "kept": kept, "cells": cells})
    return {"seed": seed, "folds": out, "predicted_test_rate": num / den if den else None}


def h45_check(rows, kept_dev_ids, band=H45_BAND):
    counts = task_counts(rows)
    ids = [i for i in kept_dev_ids if i in counts and counts[i][1]]
    if not ids:
        return {"n": 0, "rate": None, "informative": False}
    rate = sum(counts[i][0] / counts[i][1] for i in ids) / len(ids)
    return {"n": len(ids), "rate": rate, "informative": in_band(rate, band)}


def calibrate(meta, rows, seed, cap=8, cap_seed=0, band=BAND, min_tasks=MIN_TASKS,
              min_programs=MIN_PROGRAMS):
    """The calibration report (module docstring). `meta` = {id: pool task}."""
    counts = task_counts(rows)
    ids = dev_ids(meta, counts)
    ev = cell_evidence(ids, meta, counts)
    a, b = eb_fit([(e["S"], e["N"]) for e in ev.values()])
    shr = shrink(ev, a, b)
    eligible = sorted(c for c, e in ev.items() if e["N"] >= min_tasks and len(e["programs"]) >= min_programs)
    kept = sorted(c for c in eligible if in_band(shr[c], band))
    test_pool = [t for t in meta.values() if t.get("split") == "test"]
    test_weight = {}
    for t in test_pool:
        test_weight[cell_of(t)] = test_weight.get(cell_of(t), 0) + 1
    chosen = build.cap_per_program([t for t in test_pool if cell_of(t) in kept], cap, cap_seed)
    return {
        "seed": seed, "cap": cap, "cap_seed": cap_seed, "band": list(band),
        "min_tasks": min_tasks, "min_programs": min_programs,
        "prior": {"a": a, "b": b, "mean": a / (a + b), "precision": a + b},
        "cells": {c: {"tasks": e["N"], "programs": len(e["programs"]), "raw_rate": e["S"] / e["N"],
                      "shrunk_rate": shr[c], "eligible": c in eligible, "kept": c in kept,
                      "test_pool": test_weight.get(c, 0)} for c, e in sorted(ev.items())},
        "eligible": eligible, "kept": kept,
        "crossfit": crossfit(meta, counts, ids, eligible, test_weight, seed, band),
        "hx_dev": sorted(i for i in ids if 0 < counts[i][0] < counts[i][1]),
        "kept_dev_ids": sorted(i for i in ids if cell_of(meta[i]) in kept),
        "test_ids": sorted(t["id"] for t in chosen),
        "test_programs": len({t["program"] for t in chosen}),
    }


def write_subset(pool, ids, name, path, extra):
    keep = set(ids)
    tasks = [t for t in pool["tasks"] if t["id"] in keep]
    meta = dict(pool["meta"])
    meta["sources"] = {k: v for k, v in pool["meta"]["sources"].items()
                       if any(t["source_file"] == k for t in tasks)}
    meta["set"] = name
    meta["cheat_subset"] = [i for i in pool["meta"].get("cheat_subset", []) if i in keep]
    meta["achieved"] = {c: sum(t["class"] == c for t in tasks) for c in sorted({t["class"] for t in tasks})}
    meta["calibration"] = extra
    with open(path, "w") as f:
        json.dump({"meta": meta, "tasks": tasks}, f, indent=1)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--pool", required=True)
    ap.add_argument("--rows", nargs="+", required=True)
    ap.add_argument("--rows-h45", nargs="*", default=[])
    ap.add_argument("--seed", type=int, required=True, help="cross-fit fold seed")
    ap.add_argument("--cap-seed", type=int, required=True, help="per-program cap draw seed")
    ap.add_argument("--cap", type=int, default=8)
    ap.add_argument("--report", required=True)
    ap.add_argument("--write-sets", metavar="DIR", help="write DIR/hx-dev.json and DIR/hx-test.json")
    a = ap.parse_args(argv)
    pool = json.load(open(a.pool))
    meta = {t["id"]: t for t in pool["tasks"]}
    rep = calibrate(meta, read_rows(a.rows), a.seed, a.cap, a.cap_seed)
    if a.rows_h45:
        rep["h45"] = h45_check(read_rows(a.rows_h45), rep["kept_dev_ids"])
    rep["pool_sha256"] = build.sha256_file(a.pool)
    with open(a.report, "w") as f:
        json.dump(rep, f, indent=1)
    if a.write_sets:
        extra = {"report_sha256": build.sha256_file(a.report), "kept": rep["kept"]}
        write_subset(pool, rep["hx_dev"], "hx-dev", os.path.join(a.write_sets, "hx-dev.json"), extra)
        write_subset(pool, rep["test_ids"], "hx-test", os.path.join(a.write_sets, "hx-test.json"), extra)
        sys.path.insert(0, os.path.join(os.path.dirname(HERE), "ab"))
        import corpus  # rebuild every derived text to its recorded sha256
        for n in ("hx-dev.json", "hx-test.json"):
            corpus.load(os.path.join(a.write_sets, n))
    print(json.dumps({k: rep[k] for k in ("eligible", "kept")} |
                     {"predicted_test_rate": rep["crossfit"]["predicted_test_rate"],
                      "hx_dev": len(rep["hx_dev"]), "test": len(rep["test_ids"]),
                      "test_programs": rep["test_programs"], "h45": rep.get("h45")}, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
