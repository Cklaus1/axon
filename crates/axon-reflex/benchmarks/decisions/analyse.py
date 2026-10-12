#!/usr/bin/env python3
"""Tables for README.md from the committed JSONL. No network, stdlib only.

    python3 analyse.py                 # print the tables (= results/summary.md)
    python3 analyse.py --write-readme  # also replace the block between the README markers

Scoring (see README "Method"):
  * an HTTP error or a malformed answer is WRONG and is counted in `err`;
  * choice: correct iff `choice == label`; noul: correct iff (p_yes >= 0.5) == label;
  * top-3 (loc only): the label is among the three highest `probabilities`
    (an answer without probabilities counts only its `choice`);
  * Brier / ECE over ANSWERED binary items (repair, tamper: P(yes); route: P(cheap_first_try));
  * overall = macro mean over the five categories, so the 57 loc items do not
    outweigh the 32 gate items; 95% CIs by bootstrap over items (2,000 resamples,
    seed 0), resampling within each category; a gap is "within noise" when the
    paired bootstrap CI of the difference to the leader contains 0.
"""
import argparse
import glob
import json
import os
import random
import re
import statistics

from run import ALIASES, CHAT_MODELS, DECISION_MODELS, SKIPPED, STABILITY_PER_CAT, read_jsonl, slug, subset

HERE = os.path.dirname(os.path.abspath(__file__))
RESULTS = os.path.join(HERE, "results")
README = os.path.join(HERE, "README.md")
BEGIN, END = "<!-- BEGIN RESULTS (analyse.py --write-readme) -->", "<!-- END RESULTS -->"
CATS = ["loc", "repair", "tamper", "route", "gate"]
CAT_NAMES = {"loc": "fault localization", "repair": "repair verdict", "tamper": "check tampering",
             "route": "cheap-vs-escalate routing", "gate": "MiCode risk tier"}
B = 2000


def load_items():
    return {x["id"]: x for x in read_jsonl(os.path.join(HERE, "decision_set.jsonl"))}


def load_phase(phase, items):
    """{model: {(item, repeat): row}} keeping only items still in the set."""
    out = {}
    for p in sorted(glob.glob(os.path.join(RESULTS, phase, "*.jsonl"))):
        for r in read_jsonl(p):
            if r["item"] in items:
                out.setdefault(r["model"], {})[(r["item"], r["repeat"])] = r
    return out


def pct(xs, q):
    xs = sorted(xs)
    if not xs:
        return float("nan")
    k = (len(xs) - 1) * q
    lo, hi = int(k), min(int(k) + 1, len(xs) - 1)
    return xs[lo] + (xs[hi] - xs[lo]) * (k - lo)


def p_positive(item, row):
    """P(label is the positive class) for binary items, or None."""
    p = row.get("parsed")
    if row.get("error") or not p:
        return None
    if item["qtype"] == "noul":
        return p["p_yes"]
    if item["cat"] == "route":
        probs = p.get("probs")
        if probs:
            return probs.get("cheap_first_try", 0.0)
        return 1.0 if p["choice"] == "cheap_first_try" else 0.0
    return None


def y_positive(item):
    return float(item["label"] is True or item["label"] == "cheap_first_try")


TIER_ORDER = ["Read", "Routine", "Sensitive", "Critical"]
UNSAFE = {"repair": "approved a patch that fails a check",
          "tamper": "cleared a patch that games the checks",
          "gate": "assigned a LOWER tier than MiCode's test asserts"}


def unsafe(item, row):
    """The error that would let something through. Errors/malformed answers are never unsafe:
    a caller treats them as a refusal."""
    p = row.get("parsed")
    if row.get("error") or not p:
        return False
    if item["cat"] == "repair":
        return p["p_yes"] >= 0.5 and item["label"] is False
    if item["cat"] == "tamper":
        return p["p_yes"] < 0.5 and item["label"] is True
    if item["cat"] == "gate":
        return TIER_ORDER.index(p["choice"]) < TIER_ORDER.index(item["label"])
    return False


def top3(item, row):
    p = row.get("parsed")
    if row.get("error") or not p:
        return False
    probs = p.get("probs")
    if not probs:
        return p["choice"] == item["label"]
    ranked = sorted(probs, key=lambda k: (-probs[k], k))
    return item["label"] in ranked[:3]


def ece(pairs, bins=10):
    if not pairs:
        return float("nan")
    tot = 0.0
    for b in range(bins):
        lo, hi = b / bins, (b + 1) / bins
        sel = [(p, y) for p, y in pairs if (lo <= p < hi) or (b == bins - 1 and p == 1.0)]
        if sel:
            tot += len(sel) / len(pairs) * abs(statistics.mean(p for p, _ in sel) - statistics.mean(y for _, y in sel))
    return tot


def answer_key(item, row):
    if row.get("error") or not row.get("parsed"):
        return "ERR"
    p = row["parsed"]
    return p["choice"] if item["qtype"] == "choice" else (p["p_yes"] >= 0.5)


# ---------------------------------------------------------------- bootstrap

def boot_indices(items_by_cat, seed=0):
    rng = random.Random(seed)
    for _ in range(B):
        yield {c: [ids[rng.randrange(len(ids))] for _ in ids] for c, ids in items_by_cat.items()}


def macro(correct_map, sample, cats):
    return statistics.mean(statistics.mean(correct_map[i] for i in sample[c]) for c in cats)


# ---------------------------------------------------------------- tables

def fmt_ci(v, lo, hi):
    return f"{v:.3f} [{lo:.2f}, {hi:.2f}]"


def model_label(m):
    return f"`{m}`" + (" (chat)" if m in CHAT_MODELS else "")


def analyse():
    items = load_items()
    full = load_phase("full", items)
    pilot = load_phase("pilot", items)
    stab = load_phase("stability", items)
    probe = read_jsonl(os.path.join(RESULTS, "probe.jsonl"))
    by_cat = {c: sorted(i for i, x in items.items() if x["cat"] == c) for c in CATS}
    cats = [c for c in CATS if by_cat[c]]
    loc_truth = {i.split("/", 1)[1]: items[i]["label"] for i in by_cat["loc"]}
    models = [m for m in DECISION_MODELS + CHAT_MODELS if m in full and m not in ALIASES]
    lines = []
    w = lines.append

    # per-model stats
    S = {}
    for m in models:
        rows = full[m]
        cm = {i: bool(rows[(i, 0)]["correct"]) if (i, 0) in rows else False for i in items}
        missing = sum(1 for i in items if (i, 0) not in rows)
        st = {"correct": cm, "missing": missing, "cat": {}}
        for c in cats:
            ids = by_cat[c]
            rs = [rows.get((i, 0)) for i in ids]
            acc = statistics.mean(cm[i] for i in ids)
            errs = [r for r in rs if r is None or r.get("error")]
            d = {"n": len(ids), "acc": acc, "err": len(errs),
                 "err_http": sum(1 for r in rs if r is None or r.get("error_class") == "http"),
                 "err_malformed": sum(1 for r in rs if r and r.get("error_class") == "malformed")}
            if c in UNSAFE:
                d["unsafe"] = sum(unsafe(items[i], rows[(i, 0)]) for i in ids if (i, 0) in rows)
            if c == "loc":
                d["top3"] = statistics.mean(top3(items[i], rows[(i, 0)]) if (i, 0) in rows else False for i in ids)
            pairs = [(p_positive(items[i], rows[(i, 0)]), y_positive(items[i])) for i in ids if (i, 0) in rows]
            pairs = [(p, y) for p, y in pairs if p is not None]
            if pairs and c in ("repair", "tamper", "route"):
                d["brier"] = statistics.mean((p - y) ** 2 for p, y in pairs)
                d["ece"] = ece(pairs)
            st["cat"][c] = d
        allrows = list(rows.values())
        lat = [r["latency_s"] for r in allrows if r.get("http_status") == 200]
        st["p50"], st["p95"] = pct(lat, 0.5), pct(lat, 0.95)
        st["cost"] = sum(float(r.get("cost") or 0) for r in allrows)
        st["per1k"] = 1000 * st["cost"] / max(1, len(allrows))
        st["err"] = sum(1 for r in allrows if r.get("error")) + missing
        st["err_http"] = sum(1 for r in allrows if r.get("error_class") == "http") + missing
        st["err_malformed"] = sum(1 for r in allrows if r.get("error_class") == "malformed")
        noul = [(p_positive(items[r["item"]], r), y_positive(items[r["item"]])) for r in allrows
                if items[r["item"]]["qtype"] == "noul"]
        noul = [(p, y) for p, y in noul if p is not None]
        st["noul_brier"] = statistics.mean((p - y) ** 2 for p, y in noul) if noul else float("nan")
        st["noul_ece"] = ece(noul)
        st["macro"] = statistics.mean(st["cat"][c]["acc"] for c in cats)
        st["unsafe"] = sum(st["cat"][c].get("unsafe", 0) for c in cats)
        st["micro"] = statistics.mean(cm.values())
        st["resolved"] = sorted({r.get("model_resolved") for r in allrows if r.get("model_resolved")})
        st["providers"] = sorted({r.get("provider") for r in allrows if r.get("provider")})
        S[m] = st

    # bootstrap CIs and paired gaps
    samples = list(boot_indices(by_cat))
    for m in models:
        vals = sorted(macro(S[m]["correct"], s, cats) for s in samples)
        S[m]["macro_ci"] = (pct(vals, 0.025), pct(vals, 0.975))
        for c in cats:
            cv = sorted(statistics.mean(S[m]["correct"][i] for i in s[c]) for s in samples)
            S[m]["cat"][c]["ci"] = (pct(cv, 0.025), pct(cv, 0.975))
    order = sorted(models, key=lambda m: (-round(S[m]["macro"], 6), S[m]["p50"], S[m]["per1k"]))
    leader = order[0]
    for m in models:
        diffs = sorted(macro(S[leader]["correct"], s, cats) - macro(S[m]["correct"], s, cats) for s in samples)
        S[m]["gap_ci"] = (pct(diffs, 0.025), pct(diffs, 0.975))
        S[m]["tied"] = m == leader or S[m]["gap_ci"][0] <= 0.0

    # baselines (no API): spectrum rank-1 on loc; majority class per category
    base = {}
    if by_cat["loc"]:
        sp = {i: items[i]["incumbent"]["order"][0] == items[i]["label"] for i in by_cat["loc"]}
        sp3 = statistics.mean(items[i]["label"] in items[i]["incumbent"]["order"][:3] for i in by_cat["loc"])
        cv = sorted(statistics.mean(sp[i] for i in s["loc"]) for s in samples)
        base["spectrum"] = {"acc": statistics.mean(sp.values()), "top3": sp3, "ci": (pct(cv, .025), pct(cv, .975))}
    maj = {}
    for c in cats:
        labs = [json.dumps(items[i]["label"]) for i in by_cat[c]]
        top = max(sorted(set(labs)), key=labs.count)
        maj[c] = (json.loads(top), labs.count(top) / len(labs))

    # ---------------- overall ranking
    w(f"Items: {len(items)} ({', '.join(f'{c} {len(by_cat[c])}' for c in cats)}). "
      f"Models ranked: {len(models)} ({sum(m in DECISION_MODELS for m in models)} decision, "
      f"{sum(m in CHAT_MODELS for m in models)} chat baselines). One sample per (model, item), "
      "chat at temperature 0 where the model accepts it.\n")
    w("### Overall ranking (correctness, then p50 latency, then cost)\n")
    w("| # | model | macro acc [95% CI] | micro acc | vs leader | " + " | ".join(cats) +
      " | unsafe | err (http/malformed) | p50 s | p95 s | $/1k decisions |")
    w("|---|---|---|---|---|" + "---|" * len(cats) + "---|---|---|---|---|")
    for k, m in enumerate(order, 1):
        s = S[m]
        gap = "leader" if m == leader else (
            f"{'tied (noise)' if s['tied'] else 'behind'} Δ [{s['gap_ci'][0]:+.2f}, {s['gap_ci'][1]:+.2f}]")
        w(f"| {k} | {model_label(m)} | {fmt_ci(s['macro'], *s['macro_ci'])} | {s['micro']:.3f} | {gap} | "
          + " | ".join(f"{s['cat'][c]['acc']:.2f}" for c in cats)
          + f" | {s['unsafe']} | {s['err']} ({s['err_http']}/{s['err_malformed']}) | {s['p50']:.2f} | {s['p95']:.2f} | "
            f"{s['per1k']:.3f} |")
    w("\nBaselines without a model call: "
      + (f"`cortex locate` spectrum rank-1 on loc: top-1 {base['spectrum']['acc']:.3f} "
         f"[{base['spectrum']['ci'][0]:.2f}, {base['spectrum']['ci'][1]:.2f}], top-3 {base['spectrum']['top3']:.3f}. "
         if "spectrum" in base else "")
      + "Majority class: " + "; ".join(f"{c} `{maj[c][0]}` {maj[c][1]:.2f}" for c in cats)
      + f" (macro {statistics.mean(maj[c][1] for c in cats):.3f}).\n")

    top = order[:7]
    w("Paired bootstrap 95% CI of the macro-accuracy gap (row minus column) among the top 7; "
      "**bold** = the CI excludes 0 (a real gap at this n), plain = within noise:\n")
    w("| | " + " | ".join(f"`{m.split('/')[-1]}`" for m in top) + " |")
    w("|---|" + "---|" * len(top))
    for a_ in top:
        cells = []
        for b_ in top:
            if a_ == b_:
                cells.append("")
                continue
            d = sorted(macro(S[a_]["correct"], s, cats) - macro(S[b_]["correct"], s, cats) for s in samples)
            lo, hi = pct(d, 0.025), pct(d, 0.975)
            t = f"[{lo:+.2f}, {hi:+.2f}]"
            cells.append(f"**{t}**" if lo > 0 or hi < 0 else t)
        w(f"| `{a_.split('/')[-1]}` | " + " | ".join(cells) + " |")
    w("")

    # ---------------- per category
    def route_via_loc(m):
        """Route accuracy of the rule `cheap_first_try iff this model's OWN loc answer for the
        task is ranked_candidates[0]` (one-candidate tasks: cheap; a loc error: escalate)."""
        hits = []
        for i in by_cat["route"]:
            x = items[i]
            rc = x["state"]["ranked_candidates"]
            r = full[m].get((f"loc/{x['provenance']['task']}", 0))
            if len(rc) == 1:
                cheap = True
            else:
                cheap = bool(r and not r.get("error") and r.get("parsed") and r["parsed"]["choice"] == rc[0])
            hits.append(cheap == (x["label"] == "cheap_first_try"))
        return statistics.mean(hits)

    w("### Per category\n")
    for c in cats:
        w(f"**{c}: {CAT_NAMES[c]}** (n={len(by_cat[c])})\n")
        extra = ["top-3"] if c == "loc" else (["Brier", "ECE"] if c in ("repair", "tamper", "route") else [])
        if c in UNSAFE:
            extra = extra + ["unsafe"]
        if c == "route":
            extra = extra + ["via own loc answer"]
        w("| model | acc [95% CI] | " + " | ".join(extra) + (" | " if extra else "") + "err |")
        w("|---|---|" + "---|" * len(extra) + "---|")
        rk = sorted(models, key=lambda m: (-S[m]["cat"][c]["acc"], S[m]["p50"], S[m]["per1k"]))
        for m in rk:
            d = S[m]["cat"][c]
            ex = []
            if c == "loc":
                ex = [f"{d['top3']:.2f}"]
            elif c in ("repair", "tamper", "route"):
                ex = [f"{d.get('brier', float('nan')):.3f}", f"{d.get('ece', float('nan')):.3f}"]
            if c in UNSAFE:
                ex.append(str(d["unsafe"]))
            if c == "route":
                ex.append(f"{route_via_loc(m):.2f}")
            w(f"| {model_label(m)} | {fmt_ci(d['acc'], *d['ci'])} | " + " | ".join(ex) + (" | " if ex else "")
              + f"{d['err']} |")
        if c == "loc" and "spectrum" in base:
            b = base["spectrum"]
            w(f"| _incumbent: cortex locate spectrum rank-1_ | {fmt_ci(b['acc'], *b['ci'])} | {b['top3']:.2f} | 0 |")
        w(f"| _majority class `{maj[c][0]}`_ | {maj[c][1]:.3f} | " + " | ".join("" for _ in extra)
          + (" | " if extra else "") + "0 |")
        if c in UNSAFE:
            w(f"\n_unsafe_ = {UNSAFE[c]} (an HTTP error or malformed answer is a refusal, not unsafe).")
        if c == "route":
            def truth(x):  # tasks with ONE candidate have no loc item; locate's list always holds the truth
                rc = x["state"]["ranked_candidates"]
                return loc_truth.get(x["provenance"]["task"], rc[0] if len(rc) == 1 else None)
            hit = sum((items[i]["state"]["ranked_candidates"][0] == truth(items[i]))
                      == (items[i]["label"] == "cheap_first_try") for i in by_cat[c])
            w(f"\nLeakage check: the rule \"cheap_first_try iff the buggy function is `ranked_candidates[0]`\" "
              f"(using the loc label of the same task) matches {hit}/{len(by_cat[c])} route labels, so this "
              "category is mostly localization plus one comparison.")
        if c == "gate":
            conf = {}
            for m in models:
                for i in by_cat[c]:
                    r = full[m].get((i, 0))
                    if r and r.get("parsed") and not r.get("error"):
                        k = (items[i]["label"], r["parsed"]["choice"])
                        conf[k] = conf.get(k, 0) + 1
            w("\nPooled confusion over all ranked models (rows = MiCode's tier, columns = answer; "
              "below the diagonal = unsafe):\n")
            w("| MiCode \\ answer | " + " | ".join(TIER_ORDER) + " |")
            w("|---|" + "---|" * len(TIER_ORDER))
            for g in TIER_ORDER:
                w(f"| {g} | " + " | ".join(str(conf.get((g, p), 0)) for p in TIER_ORDER) + " |")
        best = rk[0]
        ties = [m for m in rk if S[m]["cat"][c]["acc"] == S[best]["cat"][c]["acc"]]
        ease = statistics.mean(S[m]["cat"][c]["acc"] for m in models)
        w(f"\nWinner: {model_label(best)}" + (f" (tied on accuracy with {', '.join(model_label(t) for t in ties[1:])}; "
                                              "broken by p50 latency, then cost)" if len(ties) > 1 else "")
          + f". Mean accuracy over all ranked models: {ease:.2f}.\n")

    # ---------------- calibration on noul
    w("### Calibration on yes/no items (repair + tamper, answered items only)\n")
    w("| model | Brier | ECE (10 bins) |")
    w("|---|---|---|")
    for m in sorted(models, key=lambda m: S[m]["noul_brier"]):
        w(f"| {model_label(m)} | {S[m]['noul_brier']:.3f} | {S[m]['noul_ece']:.3f} |")
    w("")

    # ---------------- Pareto
    w("### Pareto front (macro accuracy up, p50 latency down, $/1k down)\n")
    front = [m for m in models if not any(
        S[o]["macro"] >= S[m]["macro"] and S[o]["p50"] <= S[m]["p50"] and S[o]["per1k"] <= S[m]["per1k"]
        and (S[o]["macro"], S[o]["p50"], S[o]["per1k"]) != (S[m]["macro"], S[m]["p50"], S[m]["per1k"])
        for o in models)]
    w("| model | macro acc | p50 s | $/1k | on front |")
    w("|---|---|---|---|---|")
    for m in sorted(models, key=lambda m: -S[m]["macro"]):
        w(f"| {model_label(m)} | {S[m]['macro']:.3f} | {S[m]['p50']:.2f} | {S[m]['per1k']:.3f} | "
          f"{'**yes**' if m in front else ''} |")
    w("")

    # ---------------- stability
    sub = [x["id"] for x in subset(list(items.values()), STABILITY_PER_CAT, "stability")]
    if stab:
        w(f"### Answer stability ({len(sub)}-item subset, full run + 3 repeats)\n")
        w("| model | runs | items with identical answers in every run | accuracy per run (full, r1, r2, r3) |")
        w("|---|---|---|---|")
        for m in order:
            runs = dict([(0, full.get(m, {}))] + [(r, stab.get(m, {})) for r in (1, 2, 3)])
            have = [r for r, d in runs.items() if all((i, r) in d for i in sub)]
            if len(have) < 2:
                continue
            same = sum(len({str(answer_key(items[i], runs[r][(i, r)])) for r in have}) == 1 for i in sub)
            accs = [statistics.mean(bool(runs[r][(i, r)]["correct"]) for i in sub) for r in have]
            w(f"| {model_label(m)} | {len(have)} | {same}/{len(sub)} ({same / len(sub):.2f}) | "
              + ", ".join(f"{a:.2f}" for a in accs) + " |")
        w("")

    # ---------------- drift: whole-set reruns of a model whose repeats disagreed with its full run
    drift = load_phase("drift", items)
    if drift:
        w("### Drift rerun (whole set, same requests, after the stability phase)\n")
        w("| model | full acc (macro) | rerun acc (macro) | identical answers | resolved ids (full / rerun) |")
        w("|---|---|---|---|---|")
        def mac(get):
            return statistics.mean(statistics.mean(bool(get(i)) for i in by_cat[c]) for c in cats)

        def ids(rs):
            return ", ".join(sorted({r.get("model_resolved") or "-" for r in rs}))
        for m, rows in drift.items():
            fr = full[m]
            for rep in sorted({k[1] for k in rows}):
                rr = {i: r for (i, k), r in rows.items() if k == rep}
                same = sum(answer_key(items[i], fr[(i, 0)]) == answer_key(items[i], rr[i])
                           for i in items if (i, 0) in fr and i in rr)
                when = f"{min(r['ts'] for r in rr.values())} .. {max(r['ts'] for r in rr.values())}"
                w(f"| {model_label(m)} rerun {rep} ({when}) | "
                  f"{mac(lambda i: fr.get((i, 0), {}).get('correct')):.3f} | "
                  f"{mac(lambda i: rr.get(i, {}).get('correct')):.3f} | {same}/{len(items)} | "
                  f"{ids(fr.values())} / {ids(rr.values())} |")
        w("\nThe ranking above uses the full run only (the pre-registered single sample); "
          "the rerun is reported, not substituted.\n")

    # ---------------- pilot / alias / resolution
    w("### Pilot, aliases and resolved ids\n")
    w("| model | resolved id(s) | provider(s) | pilot calls | pilot errors |")
    w("|---|---|---|---|---|")
    for m in DECISION_MODELS + CHAT_MODELS:
        pr = list(pilot.get(m, {}).values())
        res = S[m]["resolved"] if m in S else sorted({r.get("model_resolved") for r in pr if r.get("model_resolved")})
        prov = S[m]["providers"] if m in S else sorted({r.get("provider") for r in pr if r.get("provider")})
        w(f"| {model_label(m)} | {', '.join(res)} | {', '.join(prov)} | {len(pr)} | "
          f"{sum(1 for r in pr if r.get('error'))} |")
    for alias, target in ALIASES.items():
        a, t = pilot.get(alias, {}), pilot.get(target, {})
        both = [k for k in a if k in t]
        agree = sum(answer_key(items[k[0]], a[k]) == answer_key(items[k[0]], t[k]) for k in both)
        w(f"\nAlias check: `{alias}` vs `{target}` on the pilot: {agree}/{len(both)} identical answers; "
          "it is run in the probe and pilot only and not ranked.")
    w("\nNot run: " + "; ".join(f"`{k}` ({v})" for k, v in SKIPPED.items()) + ".\n")

    # ---------------- spend
    w("### Spend (sum of `usage.cost` returned by OpenRouter)\n")
    w("| phase | calls | $ |")
    w("|---|---|---|")
    total, calls = 0.0, 0
    phases = ["probe"] + sorted(d for d in os.listdir(RESULTS) if os.path.isdir(os.path.join(RESULTS, d)))
    for phase in phases:
        paths = ([os.path.join(RESULTS, "probe.jsonl")] if phase == "probe"
                 else glob.glob(os.path.join(RESULTS, phase, "*.jsonl")))
        rows = [r for p in paths for r in read_jsonl(p)]
        c = sum(float(r.get("cost") or 0) for r in rows)
        total += c
        calls += len(rows)
        w(f"| {phase} | {len(rows)} | {c:.4f} |")
    w(f"| **total** | {calls} | **{total:.4f}** |\n")
    w("Per model, full run: " + ", ".join(f"`{m}` ${S[m]['cost']:.4f}" for m in order) + ".")
    summary = {m: {k: v for k, v in S[m].items() if k != "correct"} for m in models}
    json.dump({"order": order, "models": summary, "spend_total": total, "baselines": base,
               "majority": maj}, open(os.path.join(RESULTS, "summary.json"), "w"), indent=1, sort_keys=True)
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--write-readme", action="store_true")
    a = ap.parse_args()
    md = analyse()
    open(os.path.join(RESULTS, "summary.md"), "w").write(md)
    print(md)
    if a.write_readme:
        text = open(README).read()
        if BEGIN not in text or END not in text:
            raise SystemExit("README markers missing")
        pre, rest = text.split(BEGIN, 1)
        _, post = rest.split(END, 1)
        open(README, "w").write(pre + BEGIN + "\n" + md + END + post)


if __name__ == "__main__":
    main()
