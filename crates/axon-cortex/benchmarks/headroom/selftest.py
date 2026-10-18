#!/usr/bin/env python3
"""Self-test of the hx tooling: pure Python, no binaries, a few seconds.

    python3 -B selftest.py

Covers (README.md "Self-test"):
  * stats.py: the sign-flip p-value against a brute-force enumeration of every
    sign vector; the Monte Carlo branch; the 90% CI by test inversion; the
    program-label permutation; FU-10 route 2;
  * calibrate.py on a synthetic fixture: eligible cells, beta-binomial
    shrinkage, the band, a deterministic cross-fit, hx-dev membership, test
    selection by cell membership only, the h45 informativeness check;
  * power.py: a tiny grid runs, is deterministic per seed, and power grows
    with the effect;
  * build.py pure helpers: split determinism and inheritance, the
    per-program cap, dropping thin programs, the inputs digest;
  * mutators.py: each `subtle` operator and multi-mutation composition.

Exit 0 when every check holds; otherwise the failures are listed and the exit
code is 1.
"""
import itertools
import math
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

FAILS = []


def check(cond, what):
    if not cond:
        FAILS.append(what)


# ------------------------------------------------------------------ stats

def brute_p(diffs, program_of, delta0=0.0, alternative="greater"):
    """Every sign vector over programs, the statistic written out longhand."""
    progs = sorted({program_of[i] for i in diffs})
    a = {g: sum(d - delta0 for i, d in diffs.items() if program_of[i] == g) for g in progs}
    obs = sum(a.values())
    hits = 0
    for signs in itertools.product((1, -1), repeat=len(progs)):
        t = sum(s * a[g] for s, g in zip(signs, progs))
        if (t >= obs - 1e-9) if alternative == "greater" else (t <= obs + 1e-9):
            hits += 1
    return hits / 2 ** len(progs)


def test_stats():
    import stats
    rng = random.Random(7)
    diffs, program_of = {}, {}
    for g in range(6):
        for j in range(rng.randint(1, 4)):
            i = f"t{g}-{j}"
            program_of[i] = f"p{g}"
            diffs[i] = rng.choice([-1, -2 / 3, -1 / 3, 0, 0, 1 / 3, 2 / 3, 1, 1 / 3])
    for alt in ("greater", "less"):
        for d0 in (0.0, 0.1, -0.2):
            p, method = stats.sign_flip_p(diffs, program_of, delta0=d0, alternative=alt)
            check(method == "exact", f"stats: 6 programs must enumerate exactly, got {method}")
            check(abs(p - brute_p(diffs, program_of, d0, alt)) < 1e-12,
                  f"stats: sign_flip_p({alt}, {d0}) = {p} != brute {brute_p(diffs, program_of, d0, alt)}")
    # all-positive program sums: only the identity reaches the observed value
    pos = {f"x{g}": 0.5 for g in range(5)}
    pof = {f"x{g}": f"g{g}" for g in range(5)}
    p, _ = stats.sign_flip_p(pos, pof)
    check(abs(p - 1 / 32) < 1e-12, f"stats: 5 positive programs must give p = 1/32, got {p}")
    # the task-weighted mean
    check(abs(stats.delta_hat({"a": 1.0, "b": 0.0, "c": 0.5}) - 0.5) < 1e-12, "stats: delta_hat")
    # Monte Carlo branch: forced with a tiny exact limit, deterministic per seed, close to exact
    p_mc1, m1 = stats.sign_flip_p(diffs, program_of, exact_limit=4, mc=4000, seed=3)
    p_mc2, _ = stats.sign_flip_p(diffs, program_of, exact_limit=4, mc=4000, seed=3)
    check(m1 == "mc", f"stats: exact_limit=4 must use Monte Carlo, got {m1}")
    check(p_mc1 == p_mc2, "stats: Monte Carlo p must be deterministic per seed")
    check(abs(p_mc1 - brute_p(diffs, program_of)) < 0.03,
          f"stats: Monte Carlo p {p_mc1} far from exact {brute_p(diffs, program_of)}")
    # 2^17 > 65536: Monte Carlo by default
    big = {f"b{g}": 0.1 for g in range(17)}
    _, mb = stats.sign_flip_p(big, {k: k for k in big}, mc=200)
    check(mb == "mc", "stats: 17 programs must use Monte Carlo")
    # CI by inversion: contains the estimate, and the bounds are where the
    # one-sided tests cross alpha = 0.05 (checked against brute force)
    lo, hi = stats.ci90(diffs, program_of)
    dh = stats.delta_hat(diffs)
    check(lo <= dh <= hi, f"stats: CI [{lo}, {hi}] must contain delta_hat {dh}")
    if -1 < lo:
        check(brute_p(diffs, program_of, lo - 1e-6, "greater") <= 0.05 <
              brute_p(diffs, program_of, lo + 1e-6, "greater"),
              f"stats: CI lower bound {lo} is not the crossing of the one-sided test")
    if hi < 1:
        check(brute_p(diffs, program_of, hi + 1e-6, "less") <= 0.05 <
              brute_p(diffs, program_of, hi - 1e-6, "less"),
              f"stats: CI upper bound {hi} is not the crossing of the one-sided test")
    # a clear gain: p < 0.05 and the CI excludes 0
    gain = {f"g{g}-{j}": 0.6 for g in range(8) for j in range(2)}
    gof = {i: i.split("-")[0] for i in gain}
    p, _ = stats.sign_flip_p(gain, gof)
    lo, _hi = stats.ci90(gain, gof)
    check(p < 0.05 and lo > 0, f"stats: a uniform gain must be significant (p={p}, lo={lo})")
    # program-label permutation: D clearly higher than C
    vals, pg, strat = {}, {}, {}
    for g in range(12):
        s = "D" if g < 6 else "C"
        strat[f"q{g}"] = s
        for j in range(3):
            i = f"q{g}-{j}"
            vals[i] = 0.9 if s == "D" else 0.3
            pg[i] = f"q{g}"
    r = stats.stratum_permutation(vals, pg, strat, n_perm=2000, seed=1)
    check(abs(r["observed"] - 0.6) < 1e-9, f"stats: D-C observed {r['observed']}")
    check(r["p"] < 0.01, f"stats: a clean D>C split must permute to p < 0.01, got {r['p']}")
    r2 = stats.stratum_permutation(vals, pg, strat, n_perm=2000, seed=1)
    check(r == r2, "stats: permutation must be deterministic per seed")
    # FU-10 route 2: T1 needs fewer proposals than T2 on every task
    rows1, rows2, rp = [], [], {}
    for g in range(8):
        for j in range(2):
            i = f"r{g}-{j}"
            rp[i] = f"r{g}"
            for t in (1, 2, 3):
                rows1.append({"id": i, "trial": t, "verified_clean": True, "proposals": 1})
                rows2.append({"id": i, "trial": t, "verified_clean": True, "proposals": 3})
    rows2.append({"id": "r0-0", "trial": 4, "verified_clean": False, "proposals": 8})
    r = stats.route2(rows1, rows2, rp)
    check(r["n"] == 16 and abs(r["d_hat"] - 2.0) < 1e-12, f"stats: route2 n/d_hat {r}")
    check(r["p"] < 0.025, f"stats: route2 p must be < 0.025, got {r['p']}")
    # duplicate (id, trial) rows are refused
    try:
        stats.task_rates([{"id": "a", "trial": 1, "verified_clean": True}] * 2)
        check(False, "stats: duplicate (id, trial) must be refused")
    except ValueError:
        pass


# ------------------------------------------------------------------ calibrate

def fixture():
    """Pool meta and k=3 dev rows. Cell multi:2 sits near 0.5, cell subtle:bound
    near 1.0, cell caps:- is too small to be eligible; test tasks exist in
    every cell."""
    meta, rows = {}, []
    rng = random.Random(11)

    def add(i, cls, knob, prog, split, succ=None):
        meta[i] = {"id": i, "class": cls, "knob": knob, "program": prog, "split": split}
        if succ is not None:
            for t in (1, 2, 3):
                rows.append({"id": i, "trial": t, "verified_clean": t <= succ})
    for n in range(16):
        add(f"m{n}", "multi", "2", f"dev{n % 4}", "dev", succ=[0, 1, 2, 3, 1, 2][n % 6])
        add(f"s{n}", "subtle", "bound", f"dev{n % 4}", "dev", succ=3 if n % 8 else 2)
    for n in range(5):
        add(f"c{n}", "caps", "-", f"dev{n % 2}", "dev", succ=rng.randint(0, 3))
    for n in range(30):
        add(f"tm{n}", "multi", "2", f"test{n % 5}", "test")
        add(f"ts{n}", "subtle", "bound", f"test{n % 5}", "test")
        add(f"tc{n}", "caps", "-", f"test{n % 5}", "test")
    return meta, rows


def test_calibrate():
    import calibrate
    meta, rows = fixture()
    rep = calibrate.calibrate(meta, rows, seed=5, cap=4, cap_seed=9)
    check(sorted(rep["eligible"]) == ["multi:2", "subtle:bound"],
          f"calibrate: eligible cells {rep['eligible']}")
    check(rep["kept"] == ["multi:2"], f"calibrate: kept cells {rep['kept']}")
    raw = rep["cells"]["multi:2"]["raw_rate"]
    shr = rep["cells"]["multi:2"]["shrunk_rate"]
    grand = rep["prior"]["mean"]
    check(min(raw, grand) - 1e-9 <= shr <= max(raw, grand) + 1e-9,
          f"calibrate: shrunk rate {shr} must lie between raw {raw} and prior mean {grand}")
    rep2 = calibrate.calibrate(meta, rows, seed=5, cap=4, cap_seed=9)
    check(rep["crossfit"] == rep2["crossfit"], "calibrate: cross-fit must be deterministic per seed")
    pr = rep["crossfit"]["predicted_test_rate"]
    check(pr is None or 0 <= pr <= 1, f"calibrate: predicted rate {pr}")
    folds = rep["crossfit"]["folds"]
    check(len(folds) == 2 and not set(folds[0]["programs"]) & set(folds[1]["programs"]),
          "calibrate: the two folds must partition the dev programs")
    succ = {}
    for r in rows:
        succ[r["id"]] = succ.get(r["id"], 0) + bool(r["verified_clean"])
    mixed = {i for i, s in succ.items() if 0 < s < 3}
    check(set(rep["hx_dev"]) == mixed, f"calibrate: hx-dev = {sorted(set(rep['hx_dev']) ^ mixed)} off")
    test = rep["test_ids"]
    check(test and all(meta[i]["class"] == "multi" and meta[i]["split"] == "test" for i in test),
          "calibrate: test selection must take only kept-cell test tasks")
    per = {}
    for i in test:
        per[meta[i]["program"]] = per.get(meta[i]["program"], 0) + 1
    check(max(per.values()) <= 4 and len(test) == 20, f"calibrate: per-program cap 4, got {per}")
    # h45: k=1 rows on kept dev cells
    h45 = [{"id": f"m{n}", "trial": 1, "verified_clean": n < 2} for n in range(16)]
    hc = calibrate.h45_check(h45, rep["kept_dev_ids"])
    check(abs(hc["rate"] - 2 / 16) < 1e-12 and hc["informative"], f"calibrate: h45 {hc}")
    hc = calibrate.h45_check([dict(r, verified_clean=True) for r in h45], rep["kept_dev_ids"])
    check(not hc["informative"], "calibrate: h45 at 1.0 must be uninformative")
    # an infrastructure row is excluded, a duplicate refused
    counts = calibrate.task_counts(rows + [{"id": "m0", "trial": 4, "verified_clean": True,
                                             "rate_limited": True}])
    check(counts["m0"] == [0, 3], f"calibrate: rate-limited row must be excluded, got {counts['m0']}")
    try:
        calibrate.task_counts(rows + rows[:1])
        check(False, "calibrate: duplicate (id, trial) must be refused")
    except ValueError:
        pass


# ------------------------------------------------------------------ power

def test_power():
    import power
    rates = power.posterior_rates([(1, 3), (2, 3), (0, 3), (3, 3), (1, 3), (2, 3)])
    check(all(0 < p < 1 for p in rates), f"power: posterior rates must avoid 0 and 1: {rates}")
    kw = dict(n=48, k=3, sims=60, seed=4, min_programs=8, per_program=6)
    p1 = power.simulate_power(rates, sigma_g=1.0, mde=0.10, **kw)
    p2 = power.simulate_power(rates, sigma_g=1.0, mde=0.10, **kw)
    check(p1 == p2, "power: must be deterministic per seed")
    p0 = power.simulate_power(rates, sigma_g=1.0, mde=0.0, **kw)
    pb = power.simulate_power(rates, sigma_g=0.0, mde=0.30, **kw)
    check(p0 < pb, f"power: power must grow with the effect ({p0} vs {pb})")
    pf = power.simulate_power(rates, sigma_g=0.0, mde=0.10, model="flip", **kw)
    check(0 <= pf <= 1, f"power: flip model {pf}")
    beta = power.calibrate_beta(rates, sigma_g=0.5, sigma_e=0.5, mde=0.10, seed=4)
    check(beta > 0, f"power: beta for a +10 point gain must be positive, got {beta}")
    d = power.choose_design(rates, n_grid=[24, 48], ks=(3, 5), sims=40, seed=4,
                            costs={"success": 0.001, "failure": 0.004}, arms=3, cap_usd=100.0,
                            min_programs=4, per_program=6, target=0.0)
    check(d["chosen"] is not None and d["chosen"]["n"] == 24 and d["chosen"]["k"] == 3,
          f"power: target 0 must pick the cheapest design, got {d['chosen']}")
    d = power.choose_design(rates, n_grid=[24], ks=(3,), sims=10, seed=4,
                            costs={"success": 0.001, "failure": 0.004}, arms=3, cap_usd=0.0001,
                            min_programs=4, per_program=6, target=0.0)
    check(d["chosen"] is None, "power: a design over the cost cap must not be chosen")


# ------------------------------------------------------------------ build helpers

def test_build():
    import build
    progs = [f"p{i:02d}" for i in range(36)]
    s1 = build.split_programs(progs, seed=3)
    s2 = build.split_programs(list(reversed(progs)), seed=3)
    check(s1 == s2, "build: split must not depend on input order")
    check(s1 != build.split_programs(progs, seed=4), "build: split must depend on the seed")
    nd = sum(v == "dev" for v in s1.values())
    check(nd >= 10 and len(progs) - nd >= 20, f"build: 36 programs must give >=10 dev, >=20 test ({nd} dev)")
    prior = {p: s1[p] for p in progs[:30]}
    s3 = build.split_programs(progs + ["new1", "new2", "new3"], seed=99, prior=prior)
    check(all(s3[p] == s1[p] for p in progs[:30]), "build: programs in a prior split must inherit it")
    check(all(p in s3 for p in ("new1", "new2", "new3")), "build: new programs must be assigned")
    tasks = [{"id": f"t{p}-{j}", "program": p} for p in ("a", "b") for j in range(12)]
    c1 = build.cap_per_program(tasks, 8, seed=2)
    c2 = build.cap_per_program(list(reversed(tasks)), 8, seed=2)
    check(sorted(t["id"] for t in c1) == sorted(t["id"] for t in c2), "build: cap must be order-free")
    check(len(c1) == 16 and all(sum(t["program"] == p for t in c1) == 8 for p in "ab"),
          "build: cap 8 per program")
    check(sorted(t["id"] for t in c1) != sorted(t["id"] for t in build.cap_per_program(tasks, 8, seed=3)),
          "build: the cap's draw must depend on the seed")
    kept, dropped = build.drop_thin({"a": [1, 2, 3], "b": [1, 2]}, 3)
    check(list(kept) == ["a"] and list(dropped) == ["b"], "build: programs with < 3 tasks are dropped")
    d1 = build.inputs_digest()
    check(d1 == build.inputs_digest() and len(d1) == 64, "build: inputs digest deterministic")
    check(build.STRATUM_D and len(build.STRATUM_D) == 18, "build: stratum D lists 18 programs")
    test_cut_tests(build)


CUT_SRC = '''fn g(x: i64) -> i64 {
    x + 1
}

fn f(x: i64) -> i64 {
    g(x) * 2
}

@[test]
fn test_g() {
    assert_eq(g(1), 2)
}

@[test]
fn test_f() {
    assert_eq(f(1), 4)
}
'''


def test_cut_tests(build):
    """A test cut that ../ab/corpus.py's brace counting (not string-aware)
    cannot locate, or locates wrongly, is a rejection with a reason, never
    an exception and never an accepted cut."""
    sys.path.insert(0, os.path.dirname(HERE))
    sys.path.insert(0, os.path.join(os.path.dirname(HERE), "ab"))
    import corpus
    out, why = build.cut_tests(corpus, CUT_SRC, ["test_g"])
    check(why is None and "test_g" not in out and "fn test_f" in out and "fn g(" in out,
          f"build: a clean cut must succeed ({why})")
    cases = {
        # "{" in a string: the block never closes -> corpus.cut_test raises KeyError
        "open brace in the last test": (CUT_SRC.replace("assert_eq(f(1), 4)", 'assert_eq(len("{{"), 1)'),
                                        ["test_f"]),
        # "}" in a string: the block ends inside the string, leaving junk behind
        "close brace in a string": (CUT_SRC.replace("assert_eq(g(1), 2)", 'assert_eq(len("}}"), 1)'),
                                    ["test_g"]),
        # "{" in test_g's string and "}" in test_f's: one cut swallows test_f too
        "cut swallows the next test": (CUT_SRC.replace("assert_eq(g(1), 2)", 'assert_eq(len("{{"), 1)')
                                       .replace("assert_eq(f(1), 4)", 'assert_eq(len("}}"), 1)'),
                                       ["test_g"]),
    }
    for label, (src, names) in cases.items():
        try:
            out, why = build.cut_tests(corpus, src, names)
        except Exception as e:  # noqa: BLE001
            check(False, f"build: cut_tests must not raise ({label}): {type(e).__name__}: {e}")
            continue
        check(out is None and why, f"build: a mis-delimited cut must be rejected ({label})")

    # run_ab grading strips and re-appends test blocks with corpus.test_blocks:
    # a program with any block it mis-delimits is ungradeable and is rejected.
    check(build.blocks_reason(corpus, CUT_SRC) is None, "build: a cleanly delimited program is accepted")
    for label, (src, _) in cases.items():
        check(build.blocks_reason(corpus, src), f"build: a program with a mis-delimited block is rejected ({label})")


# ------------------------------------------------------------------ mutators

def test_mutators():
    import mutators as M
    ctx = {"params": ["xs", "n"], "fields": {"lo": "i64", "hi": "i64", "name": "str"},
           "sigs": {"clamp": ["i64", "i64", "i64"], "label": ["str", "i64"]}}
    body = ("\n    let mut total = 0\n    let mut i = 0\n    while i < n {\n"
            "        total = total + clamp(xs[i], r.lo, r.hi)\n        i = i + 1\n    }\n"
            "    for j in 0..n {\n        if j > 2 {\n            total = total + 1\n"
            "        } else {\n            total = total - 1\n        }\n    }\n    total\n")

    def all_sites(op):
        out, k = [], 0
        while True:
            r = op(body, k, ctx)
            if r is None:
                return out
            out.append(r)
            k += 1
    b = all_sites(M.bound)
    hows = [h for _, h in b]
    check(any("n + 1" in m for m, _ in b) and any("n - 1" in m for m, _ in b),
          f"mutators: bound must shift `while i < n` by +-1: {hows}")
    check(any("1..n" in m for m, _ in b), f"mutators: bound must shift a range's lower bound: {hows}")
    a = all_sites(M.typed_argswap)
    check(any("clamp(r.lo, xs[i], r.hi)" in m for m, _ in a) or any("clamp(xs[i], r.hi, r.lo)" in m for m, _ in a),
          f"mutators: typed argswap must swap same-typed args of clamp: {[h for _, h in a]}")
    check(M.typed_argswap("\n    label(s, n)\n", 0, ctx) is None,
          "mutators: typed argswap must not swap differently-typed args")
    w = all_sites(M.wrong_field)
    check(any("r.hi, r.hi" in m or "r.lo, r.lo" in m for m, _ in w), "mutators: wrong_field same-typed")
    check(not any(".name" in m for m, _ in w), "mutators: wrong_field must keep the type")
    acc = all_sites(M.acc_init)
    check(any("let mut total = 1" in m for m, _ in acc), f"mutators: acc_init 0->1: {[h for _, h in acc]}")
    de = all_sites(M.drop_else)
    check(len(de) == 1 and "else" not in de[0][0] and "total - 1" not in de[0][0],
          f"mutators: drop_else must remove the else arm: {de}")
    check(all(M.SUBTLE_OPS[i][0] for i in range(5)) and len(M.SUBTLE_OPS) == 5,
          "mutators: five subtle operators")
    e1 = M.edit_of(body, all_sites(M.acc_init)[0][0])
    e2 = M.edit_of(body, de[0][0])
    check(M.disjoint([e1, e2]), "mutators: far-apart edits are disjoint")
    comp = M.compose(body, [e1, e2])
    check("let mut total = 1" in comp and "total - 1" not in comp, "mutators: compose applies both edits")
    check(M.compose(body, [e2, e1]) == comp, "mutators: compose is order-free")
    check(not M.disjoint([e1, e1]), "mutators: an edit overlaps itself")
    # string literals and comments are never sites
    q = '\n    let s = "while i < n {"\n    // for j in 0..n {\n    s\n'
    check(M.bound(q, 0, ctx) is None, "mutators: bound must skip strings and comments")


def main():
    for t in (test_stats, test_calibrate, test_power, test_build, test_mutators):
        try:
            t()
        except Exception as e:  # a missing module or API is a failure, not a crash
            import traceback
            FAILS.append(f"{t.__name__}: {type(e).__name__}: {e}\n{traceback.format_exc(limit=3)}")
    if FAILS:
        print(f"headroom selftest: {len(FAILS)} FAILED", file=sys.stderr)
        for f in FAILS:
            print("  FAIL " + f, file=sys.stderr)
        return 1
    print("headroom selftest: ok", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
