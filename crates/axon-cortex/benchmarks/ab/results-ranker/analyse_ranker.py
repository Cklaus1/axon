#!/usr/bin/env python3
"""PREREG-ranker.md analysis: ranker run vs the spectrum-order baseline,
paired on task id. Stdlib only. Usage: analyse_ranker.py [RANKER_ROWS]."""
import json
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
AB = os.path.dirname(HERE)
BASE = os.path.join(AB, "results-micode", "full", "claude-haiku-5-5", "b.jsonl")
RUN = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "full", "b.jsonl")
TRUTH = {t["id"]: t["symbol"] for t in json.load(open(os.path.join(AB, "corpus.json")))["tasks"]}


def rows(p):
    return {r["id"]: r for r in (json.loads(l) for l in open(p) if l.strip())}


def mcnemar(b, c):
    n = b + c
    if n == 0:
        return 1.0
    k = min(b, c)
    return min(1.0, 2 * sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n)


def summary(R):
    v = sum(r["verified_clean"] for r in R)
    prop = sum(r["proposals"] for r in R)
    api = sum(r["api_ms"] for r in R) / 1e3
    gen = sum(r["cost_usd"] or 0 for r in R)
    rk = sum(r.get("ranker_cost_usd") or 0 for r in R)
    per = lambda x: round(x / v, 4) if v else None
    lat = [r["ranker"]["latency_ms"] for r in R if (r.get("ranker") or {}).get("latency_ms")]
    return {
        "n": len(R), "verified": v, "false_success": sum(r["false_success"] for r in R),
        "proposals": prop, "proposals_per_verified": per(prop),
        "model_time_s": round(api, 1), "model_time_per_verified_s": per(api),
        "gen_cost_est_usd": round(gen, 6), "ranker_cost_usd": round(rk, 6),
        "cost_per_verified_usd": per(gen + rk), "wall_s": round(sum(r["wall_s"] for r in R), 1),
        "truth_first": sum(bool(r["attempted"]) and r["attempted"][0] == TRUTH[r["id"]] for r in R),
        "ranker_fallbacks": sum(1 for r in R if r.get("ranker") is not None and r["ranker"].get("used") is None),
        "ranker_latency_ms_mean": round(sum(lat) / len(lat)) if lat else None,
    }


base, run = rows(BASE), rows(RUN)
ids = sorted(set(base) & set(run))
B, Rn = summary([base[i] for i in ids]), summary([run[i] for i in ids])
pair = {"both": 0, "only_ranker": 0, "only_baseline": 0, "neither": 0}
for i in ids:
    x, y = run[i]["verified_clean"], base[i]["verified_clean"]
    pair["both" if x and y else "only_ranker" if x else "only_baseline" if y else "neither"] += 1
pair["mcnemar_p"] = mcnemar(pair["only_ranker"], pair["only_baseline"])
c1 = Rn["false_success"] == 0
c2 = not (Rn["verified"] < B["verified"] and pair["mcnemar_p"] < 0.05)
c3 = Rn["proposals_per_verified"] is not None and Rn["proposals_per_verified"] < B["proposals_per_verified"]
out = {"paired_n": len(ids), "missing": sorted(set(base) - set(run)), "baseline": B, "ranker": Rn,
       "paired": pair, "criteria": {"no_false_success": c1, "verified_not_sig_lower": c2,
                                    "fewer_proposals_per_verified": c3},
       "verdict": "PASS" if (c1 and c2 and c3 and len(ids) == 60) else "FAIL" if len(ids) == 60 else "INCOMPLETE",
       "discordant": {"only_ranker": [i for i in ids if run[i]["verified_clean"] and not base[i]["verified_clean"]],
                      "only_baseline": [i for i in ids if base[i]["verified_clean"] and not run[i]["verified_clean"]]}}
print(json.dumps(out, indent=1))
