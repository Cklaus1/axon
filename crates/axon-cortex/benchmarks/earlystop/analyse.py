#!/usr/bin/env python3
"""The verdict of PREREG-earlystop.md, computed from committed rows.

  python3 analyse.py NEW_HARD_DEV.jsonl NEW_TEMPT.jsonl [--control HD.jsonl TEMPT.jsonl]
  python3 analyse.py --selftest

Baseline (always): the committed Cortex rows, `../hardset/results/hard-dev/
b-spectrum.jsonl` and `../tempt/results/paid/b-spectrum.jsonl`. `--control`
adds a second, same-day baseline (`results/measure/`, the same code with the
rule absent) that is REPORTED and never decides.

Every number printed is computed here from the rows; nothing is typed in.
"""
import json
import os
import sys
from math import comb

HERE = os.path.dirname(os.path.abspath(__file__))
BASE_HD = os.path.join(HERE, "..", "hardset", "results", "hard-dev", "b-spectrum.jsonl")
BASE_TEMPT = os.path.join(HERE, "..", "tempt", "results", "paid", "b-spectrum.jsonl")


def load(p):
    return {r["id"]: r for r in (json.loads(l) for l in open(p) if l.strip())}


def out_of_scope(tid):
    """Defects one function body cannot repair: hard-dev's out-of-scope tasks
    and tempt's unsatisfiable ones."""
    return tid.startswith("hd-out") or tid.startswith("tempt-unsat")


def verified(r):
    return bool(r["claim"] and r["verified_clean"])


def mcnemar_two_sided(b, c):
    """Exact two-sided McNemar on the discordant counts b and c."""
    n = b + c
    if n == 0:
        return 1.0
    k = min(b, c)
    p = 2 * sum(comb(n, i) for i in range(k + 1)) / 2**n
    return min(1.0, p)


def ranks(xs):
    """Average ranks of |x| (1-based), ties share the mean rank."""
    order = sorted(range(len(xs)), key=lambda i: abs(xs[i]))
    out = [0.0] * len(xs)
    i = 0
    while i < len(order):
        j = i
        while j + 1 < len(order) and abs(xs[order[j + 1]]) == abs(xs[order[i]]):
            j += 1
        for k in range(i, j + 1):
            out[order[k]] = (i + j) / 2 + 1
        i = j + 1
    return out


def wilcoxon_less(diffs):
    """Exact one-sided Wilcoxon signed-rank p for H1: the paired differences
    (new - committed) are negative. Zero differences are dropped. The null
    distribution is enumerated over sign assignments of the (doubled, so
    integral) average ranks."""
    d = [x for x in diffs if x != 0]
    n = len(d)
    if n == 0:
        return 1.0
    r2 = [round(2 * r) for r in ranks(d)]
    w_neg2 = sum(rr for rr, x in zip(r2, d) if x < 0)  # observed sum of ranks of the negatives
    total = sum(r2)
    counts = {0: 1}
    for rr in r2:
        nxt = dict(counts)
        for s, c in counts.items():
            nxt[s + rr] = nxt.get(s + rr, 0) + c
        counts = nxt
    # P(W_neg >= observed): the negatives carry at least this much rank.
    ge = sum(c for s, c in counts.items() if s >= w_neg2)
    assert sum(counts.values()) == 2**n and total == max(counts)
    return ge / 2**n


def selftest():
    assert abs(wilcoxon_less([-1, -2, -3, -4, -5]) - 1 / 32) < 1e-12
    assert wilcoxon_less([1, 2, 3, 4, 5]) == 1.0
    assert wilcoxon_less([0, 0]) == 1.0
    assert abs(wilcoxon_less([-1, -1, 2]) - 0.625) < 1e-12  # ties share 1.5, 1.5, 3
    assert mcnemar_two_sided(0, 6) == 2 / 64
    assert mcnemar_two_sided(3, 3) == 1.0
    assert mcnemar_two_sided(0, 0) == 1.0
    print("selftest ok")


def compare(label, new, old, report):
    ids = sorted(set(new) & set(old))
    missing = sorted(set(old) - set(new))
    ins = [i for i in ids if not out_of_scope(i)]
    oos = [i for i in ids if out_of_scope(i)]
    print(f"== {label}: new {len(new)} rows, baseline {len(old)} rows, paired {len(ids)}"
          f" (in-scope {len(ins)}, out-of-scope/unsat {len(oos)}); missing from new: {missing}")
    fs = [i for i in new if new[i]["false_success"]]
    print(f"false successes in new rows: {len(fs)} {fs}")
    nv = sum(verified(new[i]) for i in ins)
    ov = sum(verified(old[i]) for i in ins)
    b = [i for i in ins if verified(old[i]) and not verified(new[i])]  # lost
    c = [i for i in ins if verified(new[i]) and not verified(old[i])]  # gained
    p = mcnemar_two_sided(len(b), len(c))
    print(f"in-scope verified: new {nv}/{len(ins)} vs baseline {ov}/{len(ins)}; lost {b}, gained {c}; "
          f"exact two-sided McNemar p = {p:.4f}")
    wrongly_stopped = [i for i in ins if verified(old[i]) and new[i]["outcome"] == "out_of_scope_suspected"]
    print(f"in-scope tasks the baseline verified that the rule stopped: {wrongly_stopped}")
    stopped_in = [i for i in ins if new[i]["outcome"] == "out_of_scope_suspected"]
    print(f"in-scope tasks that ended out_of_scope_suspected (any baseline verdict): {stopped_in}")
    sn = sum(new[i]["wall_s"] for i in oos)
    so = sum(old[i]["wall_s"] for i in oos)
    pw = wilcoxon_less([new[i]["wall_s"] - old[i]["wall_s"] for i in oos])
    print(f"out-of-scope + unsat wall: new {sn:.1f}s vs baseline {so:.1f}s over {len(oos)} tasks "
          f"({100 * (sn - so) / so:+.1f}%); one-sided exact Wilcoxon signed-rank p = {pw:.4f}")
    wn = sum(new[i]["wall_s"] for i in ins)
    wo = sum(old[i]["wall_s"] for i in ins)
    print(f"in-scope wall (reported): new {wn:.1f}s vs baseline {wo:.1f}s ({100 * (wn - wo) / wo:+.1f}%)")
    pn = sum(new[i]["proposals"] for i in oos)
    po = sum(old[i]["proposals"] for i in oos)
    print(f"out-of-scope + unsat proposals: new {pn} vs baseline {po}")
    outs = {}
    for i in oos:
        outs[new[i]["outcome"]] = outs.get(new[i]["outcome"], 0) + 1
    print(f"out-of-scope + unsat outcomes (new): {outs}")
    claims_oos = [i for i in oos if new[i]["claim"]]
    print(f"claims on out-of-scope + unsat tasks: {claims_oos}")
    report.update(
        fs=len(fs), lost=b, p_mcnemar=p, wrongly_stopped=wrongly_stopped,
        wall_new=sn, wall_old=so, p_wilcoxon=pw, in_new=nv, in_old=ov,
    )
    return report


def verdict(r):
    c0 = r["fs"] == 0
    c1 = not r["wrongly_stopped"] and not (r["in_new"] < r["in_old"] and r["p_mcnemar"] < 0.05)
    c2 = r["wall_new"] < r["wall_old"] and r["p_wilcoxon"] < 0.05
    print(f"C0 zero false successes: {'PASS' if c0 else 'FAIL'}")
    print(f"C1 in-scope verified not lower (no verified fix stopped, no significant loss): "
          f"{'PASS' if c1 else 'FAIL'}")
    print(f"C2 out-of-scope + unsat wall time lower (sum and one-sided Wilcoxon p < 0.05): "
          f"{'PASS' if c2 else 'FAIL'}")
    print("VERDICT:", "PASS" if (c0 and c1 and c2) else "FAIL")


def main():
    a = sys.argv[1:]
    if a[:1] == ["--selftest"]:
        return selftest()
    control = None
    if "--control" in a:
        k = a.index("--control")
        control = a[k + 1:k + 3]
        a = a[:k]
    new = {**load(a[0]), **load(a[1])}
    old = {**load(BASE_HD), **load(BASE_TEMPT)}
    r = compare("committed Cortex rows (DECIDES)", new, old, {})
    verdict(r)
    if control:
        print()
        compare("same-day control, rule absent (REPORTED, never decides)", new,
                {**load(control[0]), **load(control[1])}, {})


if __name__ == "__main__":
    main()
