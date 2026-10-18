#!/usr/bin/env python3
"""The hx test statistics (spec FU-1f, FU-1b, FU-10), pure Python.

    python3 stats.py compare --cand C.jsonl --base B.jsonl --corpus hx-test.json [--seed N]
    python3 stats.py strata  --rows B.jsonl --corpus hx-test.json [--cand C.jsonl] [--seed N]
    python3 stats.py route2  --t1 T1.jsonl --t2 T2.jsonl --corpus hx-test.json [--seed N]

Rows are run_ab rows carrying `id`, `trial` and `verified_clean` (k trials per
task); the corpus gives each task's `program` (and `stratum`).

* **Statistic.** Per task, the k-trial success rate of each arm; d_i is the
  candidate's minus the baseline's on the same task; the estimate is the
  task-weighted mean of d_i.
* **p-value.** One-sided sign flip on PROGRAM sums: under H0 each program's
  sum S_g = sum of its d_i is symmetric about 0, so flipping the sign of
  whole programs gives the null distribution, and tasks of one program stay
  together (they share source and author). Exact enumeration of all 2^G sign
  vectors when 2^G <= 65 536, otherwise 10 000 seeded Monte Carlo flips with
  p = (1 + hits) / (1 + draws).
* **90% CI.** Inversion of the same test: the lower bound is where the
  one-sided "greater" test of H0: delta = d0 stops rejecting at 0.05, the
  upper where the "less" test does. Not a bootstrap.
* **Contamination row.** Program-label permutation of the stratum labels
  (D vs C): the D-minus-C difference of task means, one-sided, 10 000 seeded
  permutations.
* **FU-10 route 2.** Per task, mean proposals over verified trials in each
  arm, over tasks with >= 1 verified trial in both; d_i = T2 - T1; one-sided
  sign flip by program ("greater": T1 needs fewer proposals).

Every `p` of a test with G programs is computed as the fraction of flip sets
F with sum_{g in F} a_g <= 0, where a_g = S_g - n_g * d0: flipping F changes
the statistic from sum(a) to sum(a) - 2 sum_F(a), so "at least as large as
observed" is exactly sum_F(a) <= 0.
"""
import argparse
import json
import math
import random
import sys

EXACT_LIMIT = 65536
MC_DRAWS = 10000
ALPHA = 0.05
EPS = 1e-9
INFRA_KEYS = ("rate_limited", "usage_partial", "infra_error")


def read_rows(paths):
    rows = []
    for p in paths:
        with open(p) as f:
            rows += [json.loads(line) for line in f if line.strip()]
    return rows


def task_rates(rows, exclude_infra=True):
    """{id: (successes, trials)}; a repeated (id, trial) is refused, and rows
    flagged as infrastructure failures are left out (FU-11 "Infra failures")."""
    seen, out = set(), {}
    for r in rows:
        key = (r["id"], r.get("trial", 1))
        if key in seen:
            raise ValueError(f"duplicate row for (id, trial) = {key}")
        seen.add(key)
        if exclude_infra and any(r.get(k) for k in INFRA_KEYS):
            continue
        s, n = out.get(r["id"], (0, 0))
        out[r["id"]] = (s + bool(r.get("verified_clean")), n + 1)
    return out


def rate_diffs(cand, base):
    """{id: candidate rate - baseline rate} over the SAME tasks."""
    if set(cand) != set(base):
        only = sorted(set(cand) ^ set(base))[:5]
        raise ValueError(f"arms cover different tasks (e.g. {only})")
    return {i: cand[i][0] / cand[i][1] - base[i][0] / base[i][1] for i in cand}


def delta_hat(diffs):
    return sum(diffs.values()) / len(diffs) if diffs else 0.0


def program_sums(diffs, program_of):
    """[(S_g, n_g)] in program-name order."""
    acc = {}
    for i, d in diffs.items():
        s, n = acc.get(program_of[i], (0.0, 0))
        acc[program_of[i]] = (s + d, n + 1)
    return [acc[g] for g in sorted(acc)]


def _subset_sums(a):
    sums = [0.0]
    for x in a:
        sums = sums + [s + x for s in sums]
    return sums


class SignFlip:
    """The sign-flip null for one set of program sums. The flip sets are
    fixed at construction (all of them, or one seeded Monte Carlo draw), so p
    is monotone in d0 and the CI inversion is well defined."""

    def __init__(self, sums, seed=0, mc=MC_DRAWS, exact_limit=EXACT_LIMIT):
        self.S = [s for s, _ in sums]
        self.N = [n for _, n in sums]
        G = len(sums)
        self.exact = 2 ** G <= exact_limit
        if self.exact:
            self.masks = None
            self.method = "exact"
        else:
            rng = random.Random(seed)
            self.masks = [rng.getrandbits(G) for _ in range(mc)]
            self.method = "mc"

    def _a(self, d0):
        return [s - n * d0 for s, n in zip(self.S, self.N)]

    def p(self, d0=0.0, alternative="greater"):
        a = self._a(d0)
        hit = (lambda t: t <= EPS) if alternative == "greater" else (lambda t: t >= -EPS)
        if self.exact:
            sums = _subset_sums(a)
            return sum(1 for t in sums if hit(t)) / len(sums)
        hits = 0
        for m in self.masks:
            t, g = 0.0, 0
            while m:
                if m & 1:
                    t += a[g]
                m >>= 1
                g += 1
            hits += hit(t)
        return (1 + hits) / (1 + len(self.masks))

    def ci(self, alpha=ALPHA, lo=-1.0, hi=1.0, iters=60):
        """(lower, upper) by inverting the two one-sided tests at `alpha`."""
        def bisect(pred, a, b):  # pred false at a, true at b
            for _ in range(iters):
                m = (a + b) / 2
                if pred(m):
                    b = m
                else:
                    a = m
            return b
        if self.p(lo, "greater") > alpha:
            lower = lo
        else:
            lower = bisect(lambda d: self.p(d, "greater") > alpha, lo, hi)
        if self.p(hi, "less") > alpha:
            upper = hi
        else:
            upper = -bisect(lambda d: self.p(-d, "less") > alpha, -hi, -lo)
        return lower, upper


def sign_flip_p(diffs, program_of, delta0=0.0, alternative="greater", seed=0, mc=MC_DRAWS,
                exact_limit=EXACT_LIMIT):
    """(p, "exact"|"mc") for H0: delta <= delta0 ("greater") or >= ("less")."""
    sf = SignFlip(program_sums(diffs, program_of), seed, mc, exact_limit)
    return sf.p(delta0, alternative), sf.method


def ci90(diffs, program_of, seed=0, mc=MC_DRAWS, exact_limit=EXACT_LIMIT):
    return SignFlip(program_sums(diffs, program_of), seed, mc, exact_limit).ci(ALPHA)


def compare(diffs, program_of, seed=0):
    sf = SignFlip(program_sums(diffs, program_of), seed)
    lo, hi = sf.ci(ALPHA)
    return {"n": len(diffs), "programs": len(sf.S), "delta_hat": delta_hat(diffs),
            "p_one_sided": sf.p(0.0, "greater"), "method": sf.method, "ci90": [lo, hi]}


def stratum_permutation(values, program_of, stratum_of, a="D", b="C", n_perm=MC_DRAWS, seed=0):
    """One-sided program-label permutation of mean(a) - mean(b) over tasks.
    The stratum labels are shuffled across programs (the count of `a`
    programs is kept); p = (1 + #perm >= observed) / (1 + n_perm)."""
    progs = sorted({program_of[i] for i in values})
    tot = {g: [0.0, 0] for g in progs}
    for i, v in values.items():
        tot[program_of[i]][0] += v
        tot[program_of[i]][1] += 1
    labels = [stratum_of[g] for g in progs]
    if a not in labels or b not in labels:
        return {"observed": None, "p": None, "n_perm": 0, "reason": f"needs both {a} and {b} programs"}

    def stat(lab):
        sa = na = sb = nb = 0
        for g, s in zip(progs, lab):
            if s == a:
                sa += tot[g][0]
                na += tot[g][1]
            elif s == b:
                sb += tot[g][0]
                nb += tot[g][1]
        return sa / na - sb / nb
    obs = stat(labels)
    rng = random.Random(seed)
    lab = list(labels)
    hits = 0
    for _ in range(n_perm):
        rng.shuffle(lab)
        hits += stat(lab) >= obs - EPS
    return {"observed": obs, "p": (1 + hits) / (1 + n_perm), "n_perm": n_perm}


def mean_proposals_verified(rows):
    """{id: mean proposals over verified trials}, tasks with >= 1 verified trial."""
    acc = {}
    seen = set()
    for r in rows:
        key = (r["id"], r.get("trial", 1))
        if key in seen:
            raise ValueError(f"duplicate row for (id, trial) = {key}")
        seen.add(key)
        if r.get("verified_clean") and not any(r.get(k) for k in INFRA_KEYS):
            acc.setdefault(r["id"], []).append(float(r.get("proposals") or 0))
    return {i: sum(v) / len(v) for i, v in acc.items()}


def route2(rows_t1, rows_t2, program_of, seed=0):
    """FU-10 route 2: d_i = T2 - T1 mean proposals over verified trials, on
    tasks verified at least once in both arms; one-sided sign flip by program."""
    m1, m2 = mean_proposals_verified(rows_t1), mean_proposals_verified(rows_t2)
    d = {i: m2[i] - m1[i] for i in sorted(set(m1) & set(m2))}
    if not d:
        return {"n": 0, "d_hat": None, "p": None}
    sf = SignFlip(program_sums(d, program_of), seed)
    return {"n": len(d), "d_hat": delta_hat(d), "p": sf.p(0.0, "greater"), "method": sf.method}


def fu10_verdict(route1_p, delta2_hat, route2_p, solve_delta_hat):
    """KEEP iff route 1 (p < 0.025 on h55 and h45's delta >= -5 points) or
    route 2 (p < 0.025 and the solve-rate delta >= 0); REMOVE otherwise."""
    r1 = route1_p is not None and route1_p < 0.025 and delta2_hat is not None and delta2_hat >= -0.05
    r2 = route2_p is not None and route2_p < 0.025 and solve_delta_hat is not None and solve_delta_hat >= 0
    return {"route1": r1, "route2": r2, "verdict": "KEEP" if r1 or r2 else "REMOVE"}


def corpus_index(path):
    d = json.load(open(path))
    return ({t["id"]: t["program"] for t in d["tasks"]},
            {t["program"]: t.get("stratum") for t in d["tasks"]})


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("compare")
    c.add_argument("--cand", nargs="+", required=True)
    c.add_argument("--base", nargs="+", required=True)
    s = sub.add_parser("strata")
    s.add_argument("--rows", nargs="+", required=True)
    s.add_argument("--cand", nargs="+")
    r = sub.add_parser("route2")
    r.add_argument("--t1", nargs="+", required=True)
    r.add_argument("--t2", nargs="+", required=True)
    for p in (c, s, r):
        p.add_argument("--corpus", required=True)
        p.add_argument("--seed", type=int, default=0)
    a = ap.parse_args(argv)
    program_of, stratum_of = corpus_index(a.corpus)
    if a.cmd == "compare":
        d = rate_diffs(task_rates(read_rows(a.cand)), task_rates(read_rows(a.base)))
        out = compare(d, program_of, a.seed)
    elif a.cmd == "strata":
        base = task_rates(read_rows(a.rows))
        vals = {i: s / n for i, (s, n) in base.items()}
        out = {"rate": {}, "incumbent_D_minus_C": stratum_permutation(vals, program_of, stratum_of,
                                                                        seed=a.seed)}
        for st in ("D", "C"):
            ids = [i for i in vals if stratum_of[program_of[i]] == st]
            out["rate"][st] = sum(vals[i] for i in ids) / len(ids) if ids else None
        if a.cand:
            d = rate_diffs(task_rates(read_rows(a.cand)), base)
            out["delta"] = {}
            for st in ("D", "C"):
                ds = {i: v for i, v in d.items() if stratum_of[program_of[i]] == st}
                out["delta"][st] = compare(ds, program_of, a.seed) if ds else None
        p = out["incumbent_D_minus_C"]["p"]
        out["contamination_suspect"] = p is not None and p < ALPHA
    else:
        out = route2(read_rows(a.t1), read_rows(a.t2), program_of, a.seed)
    print(json.dumps(out, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
