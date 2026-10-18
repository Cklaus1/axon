#!/usr/bin/env python3
"""Power analysis that sizes hx-test (spec FU-1f).

    python3 power.py (--dev-rows DEV.jsonl [...] | --synthetic A,B,N,K) \\
        --cost-success USD --cost-failure USD [--cost2-success USD --cost2-failure USD --k2 1] \\
        --arms 3 --cap-usd 25 --seed N [--sims 1000] [--n-grid 80:241:16] [--ks 3,5] \\
        [--c-fraction 0.5] [--out power.json]

* **Baseline rates.** Per task, the beta-binomial posterior mean of its dev
  k-trial counts (prior fitted across tasks, calibrate.eb_fit), so no task
  sits at exactly 0 or 1. `--synthetic A,B,N,K` draws N tasks with
  p ~ Beta(A, B) and K trials each instead (seeded).
* **Design.** n test tasks over G = max(20, ceil(n / 8)) programs (<= 8 tasks
  each, dealt round robin). Each simulated replicate draws n baseline rates
  from the pool with replacement.
* **Effect, "logit" model.** Candidate rate = expit(logit(p) + beta + u_g +
  e_i), u_g ~ N(0, sigma_g^2) a program effect, e_i ~ N(0, sigma_e^2) a task
  effect; beta is solved (bisection on a fixed seeded sample) so the mean
  gain is the MDE (+10 points).
* **Effect, "flip" model.** In a random 30% of programs a fraction f of the
  baseline's failing trials become successes; f is set so the mean gain is
  the MDE (capped at 1, reported).
* **Test per replicate** (FU-1h i): k trials per arm, d_i = difference of the
  k-trial rates; reject when the one-sided sign flip on program sums gives
  p < 0.05 AND the mean d >= +5 points. The sign-flip null is approximated by
  its normal limit (mean 0, variance sum S_g^2), which keeps thousands of
  replicates fast; `--exact` uses stats.py's test itself (1 000 Monte Carlo
  flips) to cross-check.
* **Choice.** Designs (n, k), k in {3, 5}, are tried in order of total trials
  n*k (then n); the first with power >= 0.80 at sigma_g = 1.0 whose cost is
  within `--cap-usd` is chosen. Cost = arms * n * k * (r * c_success +
  (1 - r) * c_failure), r the mean baseline rate, plus the second model's
  arms * n * k2 trials at its own per-outcome costs.
  For the chosen design the report gives power at sigma_g in {0, 0.5, 1.0},
  the flip model, the C-only secondary row (`--c-fraction` of tasks and
  programs), and P(delta_2 > 0) for the second model at k2.

Deterministic for a given `--seed`.
"""
import argparse
import json
import math
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from calibrate import eb_fit  # noqa: E402
import stats  # noqa: E402

Z95 = 1.6448536269514722  # one-sided 0.05
SIGMA_GRID = (0.0, 0.5, 1.0)


def logit(p):
    return math.log(p / (1 - p))


def expit(x):
    if x >= 0:
        return 1 / (1 + math.exp(-x))
    e = math.exp(x)
    return e / (1 + e)


def posterior_rates(counts):
    """[(successes, trials)] -> beta-binomial posterior means."""
    a, b = eb_fit([(float(s), float(k)) for s, k in counts])
    return [(a + s) / (a + b + k) for s, k in counts]


def synthetic_counts(a, b, n, k, seed):
    rng = random.Random(f"synthetic:{seed}")
    out = []
    for _ in range(n):
        p = rng.betavariate(a, b)
        out.append((sum(rng.random() < p for _ in range(k)), k))
    return out


_BETA = {}


def calibrate_beta(rates, sigma_g, sigma_e, mde, seed, draws=20000):
    """beta such that mean(expit(logit p + beta + u + e) - p) == mde."""
    key = (tuple(rates), sigma_g, sigma_e, mde, seed)
    if key in _BETA:
        return _BETA[key]
    rng = random.Random(f"beta:{seed}")
    sample = [(p, logit(p) + rng.gauss(0, sigma_g) + rng.gauss(0, sigma_e) if sigma_g or sigma_e else logit(p))
              for p in (rng.choice(rates) for _ in range(draws))]

    def gain(beta):
        return sum(expit(x + beta) - p for p, x in sample) / draws
    lo, hi = -15.0, 15.0
    for _ in range(60):
        mid = (lo + hi) / 2
        if gain(mid) < mde:
            lo = mid
        else:
            hi = mid
    _BETA[key] = (lo + hi) / 2
    return _BETA[key]


def programs_for(n, per_program, min_programs):
    G = max(min_programs, math.ceil(n / per_program))
    return G, [i % G for i in range(n)]


def flip_fraction(rates, mde, share):
    lose = sum(1 - p for p in rates) / len(rates)
    f = mde / (share * lose) if lose else float("inf")
    return min(f, 1.0), f > 1.0


def simulate_power(rates, n, k, sigma_g, mde=0.10, model="logit", sigma_e=0.5, sims=1000, seed=0,
                   per_program=8, min_programs=20, floor=0.05, flip_share=0.3, exact=False,
                   stat="primary"):
    """Fraction of replicates meeting the criterion. stat "primary": sign-flip
    p < 0.05 and mean d >= floor; stat "positive": mean d > 0 (FU-1h iii)."""
    G, prog = programs_for(n, per_program, min_programs)
    rng = random.Random(f"{seed}:{n}:{k}:{sigma_g}:{model}:{mde}:{sigma_e}:{stat}")
    beta = calibrate_beta(rates, sigma_g, sigma_e, mde, seed) if model == "logit" else None
    f = flip_fraction(rates, mde, flip_share)[0] if model == "flip" else None
    hits = 0
    for _ in range(sims):
        u = [rng.gauss(0, sigma_g) if sigma_g else 0.0 for _ in range(G)]
        flipped = set(rng.sample(range(G), max(1, round(flip_share * G)))) if model == "flip" else ()
        S = [0.0] * G
        diffs = {}
        tot = 0.0
        for i in range(n):
            p = rng.choice(rates)
            if model == "logit":
                q = expit(logit(p) + beta + u[prog[i]] + (rng.gauss(0, sigma_e) if sigma_e else 0.0))
            else:
                q = p + f * (1 - p) if prog[i] in flipped else p
            x = sum(rng.random() < p for _ in range(k))
            y = sum(rng.random() < q for _ in range(k))
            d = (y - x) / k
            S[prog[i]] += d
            tot += d
            diffs[i] = d
        mean = tot / n
        if stat == "positive":
            hits += mean > 0
            continue
        if mean < floor:
            continue
        if exact:
            p_val, _ = stats.sign_flip_p(diffs, {i: prog[i] for i in range(n)}, mc=1000, seed=rng.random())
            hits += p_val < 0.05
        else:
            v = sum(s * s for s in S)
            hits += v > 0 and sum(S) / math.sqrt(v) > Z95
    return hits / sims


def design_cost(n, k, rate, costs, arms, k2=0, costs2=None, rate2=None):
    c = arms * n * k * (rate * costs["success"] + (1 - rate) * costs["failure"])
    if costs2 and k2:
        r2 = rate if rate2 is None else rate2
        c += arms * n * k2 * (r2 * costs2["success"] + (1 - r2) * costs2["failure"])
    return c


def choose_design(rates, n_grid, ks=(3, 5), sims=1000, seed=0, costs=None, arms=3, cap_usd=25.0,
                  min_programs=20, per_program=8, target=0.80, sigma_g=1.0, mde=0.10, sigma_e=0.5,
                  costs2=None, k2=0, rates2=None, c_fraction=0.5, exact=False):
    rate = sum(rates) / len(rates)
    rate2 = sum(rates2) / len(rates2) if rates2 else None
    kw = dict(mde=mde, sigma_e=sigma_e, sims=sims, seed=seed, per_program=per_program,
              min_programs=min_programs, exact=exact)
    evaluated, chosen = [], None
    for n, k in sorted(((n, k) for n in n_grid for k in ks), key=lambda d: (d[0] * d[1], d[0])):
        cost = design_cost(n, k, rate, costs, arms, k2, costs2, rate2)
        row = {"n": n, "k": k, "programs": programs_for(n, per_program, min_programs)[0],
               "cost_usd": round(cost, 4)}
        if cost > cap_usd:
            row["over_cap"] = True
            evaluated.append(row)
            continue
        row["power"] = simulate_power(rates, n, k, sigma_g, **kw)
        evaluated.append(row)
        if row["power"] >= target:
            chosen = row
            break
    out = {"target": target, "sigma_g": sigma_g, "mde": mde, "mean_baseline_rate": rate,
           "evaluated": evaluated, "chosen": None}
    if chosen:
        n, k = chosen["n"], chosen["k"]
        full = dict(chosen)
        full["power_by_sigma_g"] = {str(s): simulate_power(rates, n, k, s, **kw) for s in SIGMA_GRID}
        full["flip"] = {"power": simulate_power(rates, n, k, 0.0, model="flip", **kw),
                        "fraction": flip_fraction(rates, mde, 0.3)[0],
                        "fraction_capped": flip_fraction(rates, mde, 0.3)[1]}
        nc = max(1, round(c_fraction * n))
        gc = max(1, math.ceil(c_fraction * min_programs))
        full["c_only"] = {"n": nc, "programs_min": gc,
                          "power": simulate_power(rates, nc, k, sigma_g,
                                                  **dict(kw, min_programs=gc))}
        if k2:
            full["second_model"] = {"k2": k2, "p_delta2_positive": simulate_power(
                rates2 or rates, n, k2, sigma_g, stat="positive", **kw)}
        out["chosen"] = full
    return out


def parse_grid(s):
    if ":" in s:
        a, b, c = (int(x) for x in s.split(":"))
        return list(range(a, b, c))
    return [int(x) for x in s.split(",")]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    src = ap.add_mutually_exclusive_group(required=True)
    src.add_argument("--dev-rows", nargs="+", help="run_ab k-trial rows on dev (id, trial, verified_clean)")
    src.add_argument("--synthetic", help="A,B,N,K: N tasks, p ~ Beta(A,B), K trials")
    ap.add_argument("--dev-rows2", nargs="*", default=[], help="second model's dev rows")
    ap.add_argument("--cost-success", type=float, required=True)
    ap.add_argument("--cost-failure", type=float, required=True)
    ap.add_argument("--cost2-success", type=float)
    ap.add_argument("--cost2-failure", type=float)
    ap.add_argument("--k2", type=int, default=1)
    ap.add_argument("--arms", type=int, default=3)
    ap.add_argument("--cap-usd", type=float, required=True)
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--sims", type=int, default=1000)
    ap.add_argument("--n-grid", default="80:241:16")
    ap.add_argument("--ks", default="3,5")
    ap.add_argument("--mde", type=float, default=0.10)
    ap.add_argument("--sigma-e", type=float, default=0.5)
    ap.add_argument("--c-fraction", type=float, default=0.5)
    ap.add_argument("--min-programs", type=int, default=20)
    ap.add_argument("--per-program", type=int, default=8)
    ap.add_argument("--target", type=float, default=0.80)
    ap.add_argument("--exact", action="store_true")
    ap.add_argument("--out")
    a = ap.parse_args(argv)
    if a.dev_rows:
        counts = list(stats.task_rates(stats.read_rows(a.dev_rows)).values())
    else:
        A, B, N, K = a.synthetic.split(",")
        counts = synthetic_counts(float(A), float(B), int(N), int(K), a.seed)
    rates = posterior_rates(counts)
    rates2 = posterior_rates(list(stats.task_rates(stats.read_rows(a.dev_rows2)).values())) \
        if a.dev_rows2 else None
    costs2 = {"success": a.cost2_success, "failure": a.cost2_failure} \
        if a.cost2_success is not None and a.cost2_failure is not None else None
    out = choose_design(rates, parse_grid(a.n_grid), tuple(int(k) for k in a.ks.split(",")), a.sims,
                        a.seed, {"success": a.cost_success, "failure": a.cost_failure}, a.arms,
                        a.cap_usd, a.min_programs, a.per_program, a.target, 1.0, a.mde, a.sigma_e,
                        costs2, a.k2 if costs2 else 0, rates2, a.c_fraction, a.exact)
    out["inputs"] = {"tasks": len(counts), "seed": a.seed, "sims": a.sims, "sigma_e": a.sigma_e,
                     "arms": a.arms, "cap_usd": a.cap_usd, "source": "dev-rows" if a.dev_rows else
                     f"synthetic:{a.synthetic}", "approximation": "exact-mc1000" if a.exact else "normal"}
    text = json.dumps(out, indent=1)
    if a.out:
        with open(a.out, "w") as f:
            f.write(text + "\n")
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
