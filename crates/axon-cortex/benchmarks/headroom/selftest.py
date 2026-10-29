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
    per-program cap, dropping thin programs, the inputs digest; the hx2
    caller pin and its `--check` rule (HX-1), the erase class (HX-3a) and the
    sparse cut (HX-3b); the hx3 masked (HX3-A), excerpt (HX3-B) and
    erase-long (HX3-C) rules, the `--deal-dev` split and the class-C
    program directory in the inputs digest;
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
    # IA-15: the prediction is weighted by the capped test set, not the pool
    w = rep["crossfit"]["folds"][0]["cells"]["multi:2"]["test_weight"]
    check(w == len(test), f"calibrate: cross-fit weight must count the capped test set ({w} != {len(test)})")
    # IA-13: the FU-1 acceptance thresholds refuse the sets
    why = calibrate.acceptance(rep)
    check(any("programs" in r for r in why) and any("tasks" in r for r in why),
          f"calibrate: 5 test programs / 20 tasks must be refused, got {why}")
    ok = {"test_programs": 20, "test_ids": [f"t{i}" for i in range(150)],
          "crossfit": {"predicted_test_rate": 0.5}}
    check(calibrate.acceptance(ok) == [], f"calibrate: thresholds met must pass, got {calibrate.acceptance(ok)}")
    for r in (0.19, 0.81, None):
        check(calibrate.acceptance(dict(ok, crossfit={"predicted_test_rate": r})),
              f"calibrate: predicted rate {r} must be refused")
    import contextlib
    import io
    import json
    import tempfile
    d = tempfile.mkdtemp(prefix="hx_selftest_")
    quiet = contextlib.ExitStack()  # the CLIs' stdout/stderr chatter
    quiet.enter_context(contextlib.redirect_stdout(io.StringIO()))
    quiet.enter_context(contextlib.redirect_stderr(io.StringIO()))
    try:
        pool, rp = os.path.join(d, "pool.json"), os.path.join(d, "rows.jsonl")
        with open(pool, "w") as f:
            json.dump({"meta": {"sources": {}}, "tasks": list(meta.values())}, f)
        with open(rp, "w") as f:
            f.write("".join(json.dumps(r) + "\n" for r in rows))
        rc = calibrate.main(["--pool", pool, "--rows", rp, "--seed", "5", "--cap-seed", "9", "--cap", "4",
                             "--report", os.path.join(d, "rep.json"), "--write-sets", d])
        check(rc != 0 and not os.path.exists(os.path.join(d, "hx-test.json"))
              and os.path.exists(os.path.join(d, "rep.json")),
              f"calibrate: --write-sets must refuse (rc {rc}) and write no set, but keep the report")
        # IA-16: power reads only the kept dev cells' rows
        import power
        out = os.path.join(d, "power.json")
        power.main(["--dev-rows", rp, "--calibration", os.path.join(d, "rep.json"), "--cost-success", "0.001",
                    "--cost-failure", "0.004", "--cap-usd", "100", "--seed", "1", "--sims", "4",
                    "--n-grid", "24", "--ks", "3", "--min-programs", "4", "--per-program", "6",
                    "--out", out])
        got = json.load(open(out))["inputs"]["tasks"]
        check(got == len(rep["kept_dev_ids"]) == 16, f"power: --calibration must keep {len(rep['kept_dev_ids'])} "
              f"kept-cell dev tasks, used {got}")
    finally:
        quiet.close()
        import shutil
        shutil.rmtree(d, ignore_errors=True)
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
    # IA-14: the recorded split keeps every prior program, so a prior dev
    # program absent from one extension cannot be dealt to test by the next
    rec_a = build.split_programs(progs, seed=3)
    x = next(p for p in progs if rec_a[p] == "dev")
    rest = [p for p in progs if p != x]
    split_b = build.split_programs(rest + ["b1", "b2"], seed=5, prior=rec_a)
    rec_b = build.recorded_split(rec_a, split_b)
    check(rec_b.get(x) == "dev", "build: the recorded split must keep prior programs absent from this pool")
    split_c = build.split_programs([x, "c1", "c2", "c3"], seed=6, prior=rec_b)
    check(split_c[x] == "dev", "build: a prior dev program must never re-enter test in a chained extension")
    test_rebuild(build)
    test_names_symbol(build)


REBUILD_SRC = '''fn f(xs: [i64], n: i64) -> i64 {
    let mut s = 0
    for i in 0..len(xs) {
        if xs[i] < n {
            s = s + xs[i] * 2
        }
    }
    s
}
'''


def test_rebuild(build):
    """IA-11: a task's stored edits and operator labels are re-derived from
    the pristine source, not trusted."""
    import mutators as M
    src = REBUILD_SRC
    a, b = build._span(src, "f")
    body = src[a:b]
    sites = {(op, how): (list(M.edit_of(body, m)), how) for op, m, how in
             M.sites(body, M.ALL_OPS, M.context(src, "f"), build.MAX_SITES)}
    e_plus, h_plus = sites[("operator", "op:+->-")]
    e_cmp, h_cmp = sites[("flip-comparison", "flip:<->>")]
    e_bound, h_bound = sites[("bound", "bound:range-lo:0->1")]
    good = build.rebuild(src, "f", "multi", ["operator", "flip-comparison"], [e_plus, e_cmp],
                         f"multi:{h_plus} ; {h_cmp}")
    want = src[:a] + M.compose(body, [e_plus, e_cmp]) + src[b:]
    check(good == (want, None), f"build: rebuild must re-derive a genuine multi task ({good[1]})")
    check(build.rebuild(src, "f", "subtle", ["bound"], [e_bound], h_bound)[1] is None,
          "build: rebuild must accept a genuine subtle task")
    bad = {
        "a swapped operator label": ("multi", ["flip-comparison", "operator"], [e_plus, e_cmp],
                                     f"multi:{h_plus} ; {h_cmp}"),
        "an edit its operator does not make": ("multi", ["operator", "flip-comparison"],
                                               [[e_plus[0], e_plus[1], "/"], e_cmp], f"multi:{h_plus} ; {h_cmp}"),
        "overlapping edits": ("multi", ["operator", "operator"], [e_plus, e_plus], f"multi:{h_plus} ; {h_plus}"),
        "a subtle task with a non-subtle operator": ("subtle", ["operator"], [e_plus], h_plus),
        "a multi task with one edit": ("multi", ["operator"], [e_plus], h_plus),
        "a single task with two edits": ("caller", ["operator", "flip-comparison"], [e_plus, e_cmp],
                                         f"multi:{h_plus} ; {h_cmp}"),
        "a `how` that does not match the edits": ("subtle", ["bound"], [e_bound], "bound:range-lo:0->2"),
        "no edits": ("caps", [], [], h_plus),
    }
    for label, (cls, ops, edits, how) in bad.items():
        out, why = build.rebuild(src, "f", cls, ops, edits, how)
        check(out is None and why, f"build: rebuild must reject {label}")


def test_names_symbol(build):
    """IA-12: a caller task is rejected when ANY failing test the model sees
    (every failing test but the hidden one) names g."""
    res = {"t_vis": ("failed", "assert_eq failed: left 3, right 4"),
           "t_other": ("failed", "panic in g: index out of range"),
           "t_hidden": ("failed", "g returned 2"),
           "t_ok": ("ok", "")}
    check(build.names_symbol(res, "t_hidden", "g") == "t_other",
          "build: a non-visible failing test naming g must be found")
    res["t_other"] = ("failed", "big value wrong")
    check(build.names_symbol(res, "t_hidden", "g") is None,
          "build: the hidden test's text and \\bg\\b non-matches must not count")


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


def test_caller_pin():
    """HX-1: the hidden pin is the sorted-first direct test of g that fails on
    the mutant and calls no other function on a path to g."""
    import build
    check(build.caller_pin(["t_b", "t_a"], ["t_a", "t_x"], {}, set()) == "t_a",
          "build: caller_pin must take the sorted-first failing direct test")
    check(build.caller_pin(["t_b"], ["t_x"], {}, set()) is None,
          "build: caller_pin must be None when no direct test fails")
    check(build.caller_pin(["t_a", "t_b"], ["t_a", "t_b"], {"t_a": {"h"}}, {"h"}) == "t_b",
          "build: caller_pin must skip a direct test that also calls a caller of g")


def test_caller_check_rule():
    """HX-1 / HXA#14: `--check` refuses a caller task whose hidden test is a
    caller test or is not the pin recomputed on the rebuilt pre-cut file."""
    import build
    direct, failing = ["t_g1", "t_g2"], ["t_g1", "t_g2", "t_f"]
    ok = build.caller_hidden_ok
    check(ok({"class": "caller", "hidden": "t_f"}, direct, failing, {}, set()) is not None,
          "build: a caller task whose hidden is a caller test must be refused")
    check(ok({"class": "caller", "hidden": "t_g2"}, direct, failing, {}, set()) is not None,
          "build: a caller task whose hidden is not the recomputed pin must be refused")
    check(ok({"class": "caller", "hidden": "t_g1"}, direct, failing, {}, set()) is None,
          "build: a pinned caller task must be accepted")
    calls, reach = {"t_g1": {"h"}}, {"h"}
    check(ok({"class": "caller", "hidden": "t_g1"}, direct, failing, calls, reach) is not None,
          "build: a hidden direct test that calls a caller of g must be refused")
    check(ok({"class": "caller", "hidden": "t_g2"}, direct, failing, calls, reach) is None,
          "build: the call-path rule's pin must be accepted")


def test_sparse_cut():
    """HX-3b: hidden = the caller pin, visible = the first other failing test,
    every other failing test cut; fewer than 3 failing tests or no pin rejects."""
    import build
    sc = build.sparse_cut
    check(sc(["t1", "t_g", "t2", "t3"], ["t_g"], {}, set()) == ("t1", "t_g", ["t2", "t3"]),
          "build: sparse_cut must pin g's test and cut all but the first other failing test")
    check(sc(["t_g", "t1", "t2"], ["t_g", "t_h"], {}, set()) == ("t1", "t_g", ["t2"]),
          "build: sparse_cut's visible is the first failing test other than the pin")
    check(sc(["t_g", "t1"], ["t_g"], {}, set()) is None, "build: sparse needs >= 3 failing tests")
    check(sc(["t1", "t2", "t3"], ["t_g"], {}, set()) is None, "build: sparse with no failing direct test rejects")
    check(sc(["t_g", "t1", "t2"], ["t_g"], {"t_g": {"h"}}, {"h"}) is None,
          "build: sparse rejects when the only direct test calls a caller of g (no last-failing fallback)")


ERASE_SRC = '''fn f(x: i64) -> i64 {
    let y = x * 2
    y + 1
}

fn r(x: i64) -> Row {
    Row { a: x }
}

fn p(x: i64) {
    print(x)
}

fn one(x: i64) -> bool { x > 0 }
'''


def test_erase():
    """HX-3a: the body becomes its return type's typed default; erase is its
    own class, never a multi operator."""
    import build
    import mutators as M
    body = "\n    let y = x * 2\n    y + 1\n"
    want = {"i64": "0", "f64": "0.0", "bool": "false", "str": '""', "String": '""', "[i64]": "[]",
            "[[str]]": "[]"}
    for ret, d in want.items():
        check(M.erase(body, ret) == f"\n    {d}\n", f"mutators: erase -> {ret} must give `{d}`: {M.erase(body, ret)!r}")
    for ret in ("Row", "Result<i64, str>", "Option<i64>", None):
        check(M.erase(body, ret) is None, f"mutators: erase must reject return type {ret!r}")
    check(M.erase(" x > 0 ", "bool") == " false ", "mutators: erase keeps a one-line body on one line")
    check(M.return_type(ERASE_SRC, "f") == "i64" and M.return_type(ERASE_SRC, "r") == "Row",
          "mutators: return_type reads the declared type")
    check(M.return_type(ERASE_SRC, "p") is None and M.return_type(ERASE_SRC, "nope") is None,
          "mutators: return_type is None without `->` or without the fn")
    check("erase" not in dict(M.ALL_OPS), "mutators: erase must not be in ALL_OPS (multi never composes it)")
    check(build.CLASSES[5:] == ("erase", "erase-long", "sparse", "masked", "excerpt") and build.CLASSES[4] == "multi",
          f"build: erase (hx2), erase-long (hx3), sparse (hx2), masked, excerpt (hx3) follow multi: {build.CLASSES}")
    src = REBUILD_SRC
    a, b = build._span(src, "f")
    stub = M.erase(src[a:b], "i64")
    e = list(M.edit_of(src[a:b], stub))
    check(build.rebuild(src, "f", "erase", ["erase"], [e], "erase:i64") == (src[:a] + stub + src[b:], None),
          "build: rebuild must accept a genuine erase task")
    for cls in ("multi", "subtle", "caller", "caps", "refine", "sparse"):
        out, why = build.rebuild(src, "f", cls, ["erase"], [e], "erase:i64")
        check(out is None and why, f"build: rebuild must refuse erase under class {cls}")
    for label, (ops, edits, how) in {
            "a non-erase operator": (["operator"], [e], "erase:i64"),
            "an edit that is not the typed default": (["erase"], [[e[0], e[1], "\n    1\n"]], "erase:i64"),
            "a wrong `how`": (["erase"], [e], "erase:bool")}.items():
        out, why = build.rebuild(src, "f", "erase", ops, edits, how)
        check(out is None and why, f"build: rebuild must refuse an erase task with {label}")
    check(build.erase_knob(src, "f") == "8-15" and build.erase_knob(ERASE_SRC, "f") == "<8",
          "build: erase knob = the pristine function's line count (<8 | 8-15 | >=16)")


def _sites(src, fn):
    import build
    import mutators as M
    a, b = build._span(src, fn)
    body = src[a:b]
    return {(op, how): (list(M.edit_of(body, m)), how) for op, m, how in
            M.sites(body, M.ALL_OPS, M.context(src, fn), build.MAX_SITES)}


def test_masked_split():
    """hx3 HX3-A: hidden = the caller pin on F_j, which must fail on the
    composite; cut = F_j - {hidden}; visible = the first of F_ij - F_j."""
    import build
    ms = build.masked_split
    check(ms(["v", "h"], ["h", "c"], ["v", "h", "c"], ["h"], {}, set()) == ("v", "h", ["c"]),
          "build: masked_split must pin g's test on F_j, cut the rest of F_j, show F_ij - F_j")
    check(ms(["h"], ["h", "c"], ["h", "c"], ["h"], {}, set()) is None,
          "build: masked_split is None when F_ij - F_j is empty")
    check(ms(["v"], ["c"], ["v", "c"], ["h"], {}, set()) is None,
          "build: masked_split is None when no direct test of g fails on F_j")
    check(ms(["v"], ["h", "c"], ["v", "c"], ["h"], {}, set()) is None,
          "build: masked_split is None when the pin does not fail on the composite")
    check(ms(["v", "h2"], ["h", "h2"], ["v", "h", "h2"], ["h", "h2"], {"h": {"k"}}, {"k"}) == ("v", "h2", ["h"]),
          "build: masked_split's pin skips a direct test that calls a caller of g")


def test_masked_rebuild():
    """hx3 HX3-A: masked takes exactly 2 edits in [i, j] order, the masked
    operator j is not OPAQUE, and no other class takes a masked construction."""
    import build
    import mutators as M
    src = REBUILD_SRC
    a, b = build._span(src, "f")
    s = _sites(src, "f")
    e_plus, h_plus = s[("operator", "op:+->-")]
    e_bound, h_bound = s[("bound", "bound:range-lo:0->1")]
    e_cmp, h_cmp = s[("flip-comparison", "flip:<->>")]
    opaque = next(((op, v) for (op, _), v in s.items() if op in build.OPAQUE), None)
    check(opaque is not None, f"selftest: REBUILD_SRC has an OPAQUE site: {sorted(s)}")
    want = src[:a] + M.compose(src[a:b], [e_bound, e_plus]) + src[b:]
    got = build.rebuild(src, "f", "masked", ["bound", "operator"], [e_bound, e_plus], f"masked:{h_bound} ; {h_plus}")
    check(got == (want, None), f"build: rebuild must accept a genuine masked task in [i, j] order ({got[1]})")
    bad = {
        "one edit": (["operator"], [e_plus], f"masked:{h_plus}"),
        "three edits": (["operator", "bound", "flip-comparison"], [e_plus, e_bound, e_cmp],
                        f"masked:{h_plus} ; {h_bound} ; {h_cmp}"),
        "a multi `how`": (["operator", "bound"], [e_plus, e_bound], f"multi:{h_plus} ; {h_bound}"),
    }
    if opaque:
        op, (e_o, h_o) = opaque
        bad["an OPAQUE masked operator"] = (["bound", op], [e_bound, e_o], f"masked:{h_bound} ; {h_o}")
    for label, (ops, edits, how) in bad.items():
        out, why = build.rebuild(src, "f", "masked", ops, edits, how)
        check(out is None and why, f"build: rebuild must refuse masked with {label}")
    for cls in ("multi", "subtle", "caller", "sparse", "excerpt"):
        out, why = build.rebuild(src, "f", cls, ["bound", "operator"], [e_bound, e_plus],
                                 f"masked:{h_bound} ; {h_plus}")
        check(out is None and why, f"build: rebuild must refuse a masked construction under class {cls}")
    check(build.CLASSES[-2:] == ("masked", "excerpt") and build.CLASSES.index("sparse") < build.CLASSES.index("masked"),
          f"build: masked and excerpt are appended after sparse: {build.CLASSES}")


XFN_SRC = '''type R = { lo: i64, hi: i64 }

fn clamp(x: i64, lo: i64, hi: i64) -> i64 {
    if x < lo { lo } else { if x > hi { hi } else { x } }
}

fn f(r: R, x: i64) -> i64 {
    let y = clamp(x, r.lo, r.hi)
    y + r.lo
}
'''


def test_excerpt_rules():
    """hx3 HX3-B: excerpt lives on bundle programs (stratum X) alone, with an
    xfn or (stratum-C target) erase single, a > 24,000-character view, the
    bundle's split forced to its target's, and one cluster per target."""
    import build
    import mutators as M
    src = XFN_SRC
    a, b = build._span(src, "f")
    s = _sites(src, "f")
    xfn = next(((op, v) for (op, _), v in sorted(s.items()) if op in build.XFN_OPS), None)
    check(xfn is not None, f"selftest: XFN_SRC has an xfn site: {sorted(s)}")
    op, (e, how) = xfn
    ok = build.rebuild(src, "f", "excerpt", [op], [e], how, stratum="X", target_stratum="C")
    check(ok[1] is None, f"build: rebuild must accept an xfn excerpt task on stratum X ({ok[1]})")
    out, why = build.rebuild(src, "f", "excerpt", [op], [e], how, stratum="C", target_stratum="C")
    check(out is None and why, "build: rebuild must refuse excerpt on a non-X program")
    for cls in ("subtle", "caller", "sparse"):
        out, why = build.rebuild(src, "f", cls, [op], [e], how, stratum="X", target_stratum="C")
        check(out is None and why, f"build: rebuild must refuse class {cls} on an X program")
    other = next(((o, v) for (o, _), v in sorted(s.items()) if o not in build.XFN_OPS), None)
    if other:
        o, (e_o, h_o) = other
        out, why = build.rebuild(src, "f", "excerpt", [o], [e_o], h_o, stratum="X", target_stratum="C")
        check(out is None and why, f"build: rebuild must refuse an excerpt op outside XFN_OPS ({o})")
    stub = M.erase(src[a:b], "i64")
    ee = list(M.edit_of(src[a:b], stub))
    check(build.rebuild(src, "f", "excerpt", ["erase"], [ee], "erase:i64", stratum="X", target_stratum="C")[1] is None,
          "build: rebuild must accept an erase excerpt task on a stratum-C target")
    out, why = build.rebuild(src, "f", "excerpt", ["erase"], [ee], "erase:i64", stratum="X", target_stratum="D")
    check(out is None and why, "build: rebuild must refuse an erase excerpt task on a stratum-D target")
    check(not build.excerpt_witness("x" * 23999) and not build.excerpt_witness("x" * 24000)
          and build.excerpt_witness("x" * 24001), "build: the excerpt witness is len > 24,000")
    # HX3A-2: a bundle's split is its target's recorded split, never dealt
    prior = {f"p{i:02d}.ax": ("dev" if i % 3 == 0 else "test") for i in range(30)}
    progs = sorted(prior) + [f"b/{p}" for p in sorted(prior)]
    targets = {f"b/{p}": p for p in prior}
    sp = build.assign_split(progs, 7, prior, targets)
    check(all(sp[b_] == prior[p] for b_, p in targets.items()),
          "build: a bundle's split must equal its target program's recorded split")
    check(build.bundle_split_reason({"split": "dev", "target_program": "p01.ax"}, prior) is not None
          and build.bundle_split_reason({"split": "test", "target_program": "p01.ax"}, prior) is None
          and build.bundle_split_reason({"split": "dev"}, prior) is None,
          "build: --check refuses a bundle task whose split is not its target's")
    # HX3A-3 / HX3A-4: one cluster per target program
    check(build.cluster({"program": "b/p.ax", "target_program": "p.ax"}) == "p.ax"
          and build.cluster({"program": "p.ax"}) == "p.ax", "build: cluster(t) = target_program or program")
    tasks = ([{"id": f"m{j}", "program": "p.ax"} for j in range(6)] +
             [{"id": f"x{j}", "program": "b/p.ax", "target_program": "p.ax"} for j in range(6)])
    check(len(build.cap_per_program(tasks, 8, seed=1)) == 8, "build: cap_per_program caps per cluster")
    kept, dropped = build.drop_thin({"p.ax": [1, 2], "b/p.ax": [3]}, 3, {"b/p.ax": "p.ax"})
    check(sorted(kept) == ["b/p.ax", "p.ax"] and not dropped, "build: drop_thin counts tasks per cluster")
    kept, dropped = build.drop_thin({"p.ax": [1], "b/p.ax": [3]}, 3, {"b/p.ax": "p.ax"})
    check(not kept and sorted(dropped) == ["b/p.ax", "p.ax"], "build: a thin cluster is dropped whole")


BUNDLE_P = '''// P: the target
fn area(w: i64, h: i64) -> i64 {
    w * h
}

fn load_text() -> str {
    "x"
}

fn main() {
    print(area(2, 3))
}

@[test]
fn test_area() {
    assert_eq(area(2, 3), 6)
}
'''
BUNDLE_F1 = '''fn twice(x: i64) -> i64 {
    x * 2
}

// entry point
fn main() {
    print(twice(2))
}

fn load_text() -> str {
    "x"
}

@[test]
fn test_twice() {
    assert_eq(twice(2), 4)
}
'''
BUNDLE_F2 = '''type Box = { w: i64 }

fn area(b: Box) -> i64 {
    b.w * b.w
}

@[test]
fn test_box() {
    assert_eq(area(Box { w: 2 }), 4)
}
'''
BUNDLE_F3 = '''fn neg(x: i64) -> i64 {
    0 - x
}

@[test]
fn test_neg() {
    assert_eq(neg(2), 0 - 2)
}
'''


def test_bundle():
    """hx3 HX3-B bundle.py: P's source first, then same-side fillers in a
    seeded order, each with `fn main` and already-included identical fns
    removed; a filler with any other top-level name collision is skipped;
    stops at min_chars; `--check` detects a changed filler."""
    import json
    import shutil
    import tempfile
    import bundle
    srcs = {"p.ax": BUNDLE_P, "f1.ax": BUNDLE_F1, "f2.ax": BUNDLE_F2, "f3.ax": BUNDLE_F3, "t1.ax": BUNDLE_F3 + "\n"}
    split = {"p.ax": "dev", "f1.ax": "dev", "f2.ax": "dev", "f3.ax": "dev", "t1.ax": "test"}
    big = bundle.make_bundle("p.ax", srcs, split, seed=1, min_chars=10 ** 6)
    check(big["text"].startswith(BUNDLE_P), "bundle: P's source is the unedited prefix")
    check("t1.ax" not in big["fillers"] and set(big["fillers"]) == {"f1.ax", "f3.ax"},
          f"bundle: fillers come from P's side only, a colliding one skipped: {big['fillers']}")
    check(big["skipped"].get("f2.ax") == ["area"], f"bundle: f2 collides on `area`: {big['skipped']}")
    check(big["text"].count("fn main(") == 1 and "// entry point" not in big["text"],
          "bundle: a filler's `fn main` is removed with its attached comment")
    check(big["text"].count("fn load_text(") == 1, "bundle: an identical already-included fn is removed")
    check("fn twice(" in big["text"] and "fn test_twice(" in big["text"], "bundle: the rest of a filler stays")
    check(bundle.make_bundle("p.ax", srcs, split, seed=1, min_chars=10 ** 6) == big, "bundle: deterministic")
    orders = {tuple(bundle.make_bundle("p.ax", srcs, split, seed=k, min_chars=10 ** 6)["fillers"]) for k in range(8)}
    check(len(orders) == 2, f"bundle: the filler order depends on the seed: {orders}")
    one = bundle.make_bundle("p.ax", srcs, split, seed=1, min_chars=len(BUNDLE_P) + 1)
    check(len(one["fillers"]) == 1 and one["reached"], f"bundle: stops once min_chars is reached: {one['fillers']}")
    check(not bundle.make_bundle("t1.ax", srcs, split, seed=1, min_chars=10 ** 6)["reached"],
          "bundle: an unreachable floor is reported")
    root = tempfile.mkdtemp(prefix="hx3_bundle_selftest_")
    try:
        for n, t in srcs.items():
            with open(os.path.join(root, n), "w") as f:
                f.write(t)
        out = os.path.join(root, "out")
        man = bundle.write_bundles(srcs, split, seed=1, min_chars=len(BUNDLE_P) + 1, out=out)
        m = json.load(open(man))
        check(len(m["bundles"]) == 5 and os.path.exists(os.path.join(out, "dev", "p.ax")),
              f"bundle: one bundle per program under <out>/<side>/: {[b['path'] for b in m['bundles']]}")
        check(bundle.check_manifest(man, root) == [], f"bundle: --check of fresh bundles: {bundle.check_manifest(man, root)}")
        with open(os.path.join(root, "f1.ax"), "a") as f:
            f.write("\n// changed\n")
        check(bundle.check_manifest(man, root) != [], "bundle: --check must detect a changed filler")
    finally:
        shutil.rmtree(root, ignore_errors=True)


def _long_fn(name, lines):
    """An i64 function of exactly `lines` lines, `fn` line to closing brace."""
    body = "".join(f"    let v{i} = x + {i}\n" for i in range(lines - 3))
    return f"fn {name}(x: i64) -> i64 {{\n{body}    x\n}}\n\n"


LONG_SRC = _long_fn("long_fn", 24) + _long_fn("long_nopin", 30) + _long_fn("short_fn", 23) + '''fn wrap(x: i64) -> i64 {
    long_fn(x) + long_nopin(x)
}
'''


def test_erase_long():
    """hx3 HX3-C: erase-long is the erase body of a stratum-C function whose
    pristine span is >= 24 lines (knob ">=24"); hidden = the caller pin (no
    pin, no task), visible = the first other failing test, nothing cut; such
    a function's erase single is an erase-long candidate, never an erase one;
    `rebuild` refuses erase-long on a < 24-line span."""
    import build
    import mutators as M
    src = LONG_SRC
    check((build.fn_lines(src, "long_fn"), build.fn_lines(src, "long_nopin"), build.fn_lines(src, "short_fn"))
          == (24, 30, 23), "build: fn_lines counts the `fn` line to the closing brace")
    check(build.erase_long_knob(src, "long_fn") == ">=24" and build.erase_long_knob(src, "long_nopin") == ">=24"
          and build.erase_long_knob(src, "short_fn") is None, "build: the erase-long knob is >=24 from the span line count")
    check(build.erase_knob(src, "short_fn") == ">=16", "build: a 23-line span keeps the erase knob >=16")
    check(build.CLASSES.index("erase") < build.CLASSES.index("erase-long") < build.CLASSES.index("sparse"),
          f"build: erase-long follows erase in CLASSES: {build.CLASSES}")
    edit = {}
    for fn in ("long_fn", "short_fn"):
        a, b = build._span(src, fn)
        stub = M.erase(src[a:b], "i64")
        edit[fn] = list(M.edit_of(src[a:b], stub))
        if fn == "long_fn":
            got = build.rebuild(src, fn, "erase-long", ["erase"], [edit[fn]], "erase:i64", stratum="C")
            check(got == (src[:a] + stub + src[b:], None), f"build: rebuild must accept erase-long on a 24-line span ({got[1]})")
    out, why = build.rebuild(src, "short_fn", "erase-long", ["erase"], [edit["short_fn"]], "erase:i64", stratum="C")
    check(out is None and why and "24" in why, f"build: rebuild must refuse erase-long on a 23-line span ({why})")
    check(build.rebuild(src, "short_fn", "erase", ["erase"], [edit["short_fn"]], "erase:i64", stratum="C")[1] is None,
          "build: rebuild must still accept class erase on a short span")
    for label, (cls, ops, stratum) in {"a non-erase operator": ("erase-long", ["operator"], "C"),
                                        "a bundle program": ("erase-long", ["erase"], "X")}.items():
        out, why = build.rebuild(src, "long_fn", cls, ops, [edit["long_fn"]], "erase:i64", stratum=stratum,
                                 target_stratum="C")
        check(out is None and why, f"build: rebuild must refuse erase-long with {label}")
    for cls in ("multi", "subtle", "caller", "sparse"):
        out, why = build.rebuild(src, "long_fn", cls, ["erase"], [edit["long_fn"]], "erase:i64", stratum="C")
        check(out is None and why, f"build: rebuild must refuse an erase-long construction under class {cls}")
    pv = build.pinned_visible
    check(pv(["v", "h"], ["h"], {}, set()) == ("v", "h"), "build: pinned_visible = (first other failing, pin)")
    check(pv(["h"], ["h"], {}, set()) is None, "build: pinned_visible needs >= 2 failing tests")
    check(pv(["v", "c"], ["c"], {"c": {"k"}}, {"k"}) is None, "build: pinned_visible is None without a pin")

    class Prog:
        pin_inputs = build.Program.pin_inputs
        stratum, source_file, refine, caps_ok = "C", "programs-c/long.ax", False, {}
        graph = {"long_fn": set(), "long_nopin": set(), "short_fn": set(), "wrap": {"long_fn", "long_nopin"}}
        direct_tests = {"long_fn": ["test_long_a", "test_long_b"], "long_nopin": ["test_nopin"],
                        "short_fn": ["test_short"], "wrap": ["test_wrap"]}
        test_calls = {"test_long_a": {"long_fn", "wrap"}, "test_long_b": {"long_fn"},
                      "test_nopin": {"long_nopin", "wrap"}, "test_short": {"short_fn"}, "test_wrap": {"wrap"},
                      "test_other": set()}
    prog = Prog()
    prog.src = src
    failing = {"long_fn": ["test_wrap", "test_long_a", "test_long_b"], "long_nopin": ["test_wrap", "test_nopin"],
               "short_fn": ["test_short", "test_other"]}
    erased = {fn: {"fn": fn, "op": "erase", "how": "erase:i64", "mutant": "", "broken": f"<{fn} erased>",
                   "edit": (0, 0, ""), "failing": f, "messages": {}} for fn, f in failing.items()}
    cands = build.candidates(prog, {}, 1, 3, erased)
    el = cands.get(("erase-long", ">=24"), [])
    check([(c["fn"], c["visible"], c["hidden"], c["ops"], c.get("cut")) for c in el]
          == [("long_fn", "test_wrap", "test_long_b", ["erase"], None)],
          f"build: erase-long candidate with the caller-pin hidden, the first other failing visible, nothing cut: {el}")
    er = {(k, c["fn"]) for k, v in cands.items() if k[0] == "erase" for c in v}
    check(er == {(("erase", ">=16"), "short_fn")},
          f"build: a >= 24-line span is never an erase candidate, a pinless one is no candidate at all: {sorted(er)}")


def test_deal_dev():
    """hx3 HX3-C: `--deal-dev` deals every program not in the prior split to
    dev (prior programs keep their side, a bundle its target's); the inputs
    digest covers every program directory beside build.py and --programs-dir."""
    import shutil
    import tempfile
    import build
    prior = {f"p{i:02d}.ax": ("dev" if i % 3 == 0 else "test") for i in range(30)}
    new = [f"c/n{i:02d}.ax" for i in range(12)]
    progs = sorted(prior)[:20] + new
    sp = build.split_programs(progs, 1, prior, deal="dev")
    check(all(sp[p] == "dev" for p in new), f"build: --deal-dev deals every new program to dev: {sp}")
    check(all(sp[p] == prior[p] for p in sorted(prior)[:20]), "build: --deal-dev keeps every prior program's side")
    check(build.split_programs(progs, 1, prior) == build.split_programs(progs, 1, prior, deal="seeded")
          and any(v == "test" for p, v in build.split_programs(progs, 1, prior).items() if p in new),
          "build: the default deal is the seeded shuffle")
    check(build.split_programs(new, 1, None, deal="dev") == {p: "dev" for p in new},
          "build: --deal-dev without a prior split deals every program to dev")
    sa = build.assign_split(progs + ["b/c/n00.ax"], 1, prior, {"b/c/n00.ax": "c/n00.ax"}, deal="dev")
    check(sa["b/c/n00.ax"] == "dev" and all(sa[p] == "dev" for p in new),
          "build: assign_split passes the deal on (a bundle takes its target's side)")
    try:
        build.split_programs(new, 1, None, deal="test")
        check(False, "build: an unknown deal must be refused")
    except ValueError:
        pass
    root = tempfile.mkdtemp(prefix="hx3c_digest_selftest_")
    here = build.HERE
    try:
        for d, n in (("programs", "a.ax"), ("programs-c", "b.ax"), ("elsewhere", "c.ax")):
            os.makedirs(os.path.join(root, d))
            with open(os.path.join(root, d, n), "w") as f:
                f.write("fn main() {\n}\n")
        build.HERE = root
        other = os.path.join(root, "elsewhere")
        d0, e0 = build.inputs_digest(), build.inputs_digest(other)
        with open(os.path.join(root, "programs-c", "b.ax"), "a") as f:
            f.write("// changed\n")
        d1 = build.inputs_digest()
        check(d1 != d0, "build: the inputs digest covers programs-c/ beside build.py")
        with open(os.path.join(other, "c.ax"), "a") as f:
            f.write("// changed\n")
        check(build.inputs_digest() == d1 and build.inputs_digest(other) not in (e0, d1),
              "build: the inputs digest covers --programs-dir")
    finally:
        build.HERE = here
        shutil.rmtree(root, ignore_errors=True)


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
    for t in (test_stats, test_calibrate, test_power, test_build, test_caller_pin, test_caller_check_rule,
              test_sparse_cut, test_erase, test_mutators, test_masked_split, test_masked_rebuild, test_excerpt_rules,
              test_bundle, test_erase_long, test_deal_dev):
        try:
            t()
        except (Exception, SystemExit) as e:  # a missing module, API or CLI option is a failure, not a crash
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
