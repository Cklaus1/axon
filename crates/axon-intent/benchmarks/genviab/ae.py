#!/usr/bin/env python3
"""ae: the authoring-eval runner (tasks/authoring-eval-spec.md AE-4, AE-5, AE-6).
python3 -B ae.py run --set-dir DIR --label L --base-bin DIR --cand-bin DIR
    [--seeds 0,1,2] [--confirm RUN_ID] [--expect-card-change] [--advisory] [--pilot]
    [--generator http|oracle|empty] [--provider cere] [--model M] [--cache-dir DIR]
    [--runs-dir DIR] [--history FILE] [--prereg FILE] [--parallel N] [--until HH:MM]
python3 -B ae.py report RUN_ID [--against RUN_ID] [--runs-dir DIR] [--history FILE]
python3 -B ae.py classes RUN_ID [RUN_ID ...] [--runs-dir DIR] [--history FILE]
python3 -B ae.py check-canaries --set-dir DIR

`run` measures one binary pair on a frozen `ae` set (spec AE-4): for every
(task, trial seed) it runs genviab's arm a (`run_arm_a`, the http generator on
CERE) under the base and the candidate binary, alternating which goes first
per task, and arm c (Python, AE-6) once. The grader is genviab's (gv.py); this
file only orchestrates and aggregates. `paired_mode` is decided before the first
call from each binary's locally rendered round-0 prompts: `regrade` when every
task's prompt is byte-equal (round 0 is generated once, through the http
driver's per-run share dir, and graded under both), else `resample`.

Private (spec D8): rows, transcripts, programs and replies live under
--runs-dir (default $AE_PRIVATE/runs, AE_PRIVATE default /root/axon-ae) and
the reply cache under $AE_PRIVATE/cache; both are refused inside the public
repo. Public: one aggregate `axon-ae/1` row per run appended to
ae/history.jsonl, linked to the private rows by `rows_sha256` only.

Refused: a PREREG-ae.md that is not FROZEN (unless --pilot: a pilot/dev run,
no trend row), a manifest sha or model series other than PREREG's pins, a
--confirm of a run on another set version or series, a start inside the
01:30-08:00 CERE blackout, a --set-dir/--runs-dir/--cache-dir in the public
repo, and (--expect-card-change) byte-equal base and candidate cards.
No trend row (the run's row is kept privately in <run>/row.json): infra > 5%,
a run cut short by its window (INCOMPLETE: 01:30 or --until), --pilot, a dry
run (oracle/empty) without an explicit --history, or a set canary found in
the content of `git ls-files`.

`--confirm RUN_ID` reruns that run's binaries on the next k seeds never used
for the set version, with the reply cache off (NH-5): a `design: confirm` row.
`report` resolves the flag (REGRESSION-UNCONFIRMED / REGRESSION / NOT
CONFIRMED / IMPROVED / NO CHANGE) from the run's row and its confirm row;
`--against` compares two runs' FT-pass and names every reply whose sha differs
(D4). `classes` ranks round-0 failure classes over runs of one set version and
prints draft opportunities stanzas; it never writes opportunities.md (AE-5).
"""
import argparse
import concurrent.futures as cf
import copy
import datetime
import fcntl
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gv  # noqa: E402
import genviab as G  # noqa: E402

sys.path.insert(0, os.path.join(gv.CORTEX_BENCH, "headroom"))
import stats  # noqa: E402

Refused = G.Refused
PRIVATE = os.environ.get("AE_PRIVATE", "/root/axon-ae")
HISTORY = os.path.join(HERE, "ae", "history.jsonl")
CLASSES = os.path.join(HERE, "ae", "classes.json")
PREREG_AE = os.path.join(HERE, "PREREG-ae.md")
SCHEMA = "axon-ae/1"
CI_ALPHA = 0.025              # two one-sided tests at 0.025: the 95% CI (spec §3, D6)
INFRA_MAX = 0.05              # D7
DEFAULT_SEEDS = "0,1,2"
BLACKOUT = (datetime.time(1, 30), datetime.time(8, 0))
TIERS = ("T1", "T2", "T3", "TS")
DRAFTERS = {"haiku-5-5": "Haiku 5.5", "human": "human"}
BINS = ("base", "cand")
PUBLIC_ROOT_ENV = "AE_PUBLIC_ROOT"    # honoured only under GENVIAB_SELFTEST=1
CAPTURE_DRIVER = "#!/bin/sh\ncat > /dev/null\nexit 1\n"


# ---------------------------------------------------------------- pins

def private_path(path, what):
    """An absolute path outside the public repo (spec D8, AE-3 contamination rule 2)."""
    p = os.path.abspath(path)
    if (os.path.realpath(p) + os.sep).startswith(os.path.realpath(gv.ROOT) + os.sep):
        raise Refused(f"{what} {p} is inside the public repo: it holds private ae data (spec D8)")
    return p


def prereg_pins(path):
    """PREREG-ae.md's state: FROZEN?, its sha256 and the `- key: \\`value\\`` pins."""
    if not os.path.exists(path):
        return {"frozen": False, "sha256": None, "set_version": None, "manifest_sha256": None,
                "series": None}
    text = open(path).read()

    def pin(key):
        m = re.search(rf"^- {key}: `([^`]+)`", text, re.M)
        return m.group(1) if m else None
    return {"frozen": re.search(r"^\*\*Status: FROZEN", text, re.M) is not None,
            "sha256": gv.sha256_file(path), "set_version": pin("set version"),
            "manifest_sha256": pin("manifest sha256"), "series": pin("model series")}


def bins_of(d):
    b = tuple(os.path.abspath(os.path.join(d, n)) for n in ("axon", "axon-intent"))
    missing = [p for p in b if not os.access(p, os.X_OK)]
    if missing:
        raise Refused(f"binary dir {d}: not executable: {missing}")
    return b


def bin_pins(bins):
    v = G.version([bins[0], "--version"]) or ""
    m = re.search(r"\(([^)]+)\)", v)
    return {"git_head": m.group(1) if m else None, "axon_sha256": gv.sha256_file(bins[0]),
            "axon_intent_sha256": gv.sha256_file(bins[1]), "card_sha256": G.card_sha256(bins)}


def round0_prompt_sha(task, bins):
    """The binary's round-0 prompt sha256 for `task`, rendered locally: the
    loop runs once against a driver that reads the prompt and fails (no model
    call); the transcript records the prompt's sha256."""
    ws = tempfile.mkdtemp(prefix="ae-prompt-")
    try:
        cap = os.path.join(ws, "capture.sh")
        with open(cap, "w") as fh:
            fh.write(CAPTURE_DRIVER)
        os.chmod(cap, 0o755)
        intent = os.path.join(ws, f"{task['id']}.intent.md")
        with open(intent, "w") as fh:
            fh.write(gv.generation_intent(task))
        tr = os.path.join(ws, "transcript.json")
        subprocess.run([bins[1], "compile", intent, "--generate", "--generator", f"cmd:{cap}",
                        "--max-rounds", "0", "--out", os.path.join(ws, "o"), "--transcript", tr],
                       cwd=ws, env=gv.intent_env(bins), capture_output=True, text=True, timeout=120)
        rounds = json.load(open(tr)).get("rounds") if os.path.exists(tr) else []
        sha = rounds[0].get("prompt_sha256") if rounds else None
        if not sha:
            raise Refused(f"{task['id']}: {bins[1]} rendered no round-0 prompt")
        return sha
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def prompt_set_sha256(shas):
    return gv.sha256_bytes("\n".join(sorted(shas.values())).encode())


def window_end(now, until, cere):
    """When the run must stop dispatching: the next 01:30 for a CERE run (and
    a refusal inside 01:30-08:00), or --until HH:MM today, whichever is first."""
    end = None
    if cere:
        if BLACKOUT[0] <= now.time() < BLACKOUT[1]:
            raise Refused("CERE runs never start between 01:30 and 08:00 (trainloop, nightly sweep)")
        end = now.replace(hour=1, minute=30, second=0, microsecond=0)
        if end <= now:
            end += datetime.timedelta(days=1)
    if until:
        h, m = (int(x) for x in until.split(":"))
        u = now.replace(hour=h, minute=m, second=0, microsecond=0)
        if u <= now:
            raise Refused(f"--until {until} has passed")
        end = min(end, u) if end else u
    return end


# ---------------------------------------------------------------- files

def run_paths(runs, run_id):
    d = os.path.join(runs, run_id)
    return {"dir": d, "meta": os.path.join(d, "meta.json"), "row": os.path.join(d, "row.json"),
            "rows": {b: os.path.join(d, b, "rows.jsonl") for b in BINS + ("python",)}}


def read_rows(path):
    return [json.loads(l) for l in open(path) if l.strip()] if os.path.exists(path) else []


def rows_digest(paths):
    """sha256 over the base, candidate and python rows files, in that order."""
    parts = []
    for b in BINS + ("python",):
        p = paths["rows"][b]
        parts.append(open(p, "rb").read() if os.path.exists(p) else b"")
    return gv.sha256_bytes(b"\0".join(parts))


def append_jsonl(path, row):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "a") as fh:
        fcntl.flock(fh, fcntl.LOCK_EX)
        fh.write(json.dumps(row, sort_keys=True) + "\n")


def read_history(path):
    return read_rows(path) if path else []


def append_history(path, row):
    bad = validate_row(row)
    if bad:
        raise Refused(f"the trend row does not validate against {SCHEMA}: {bad}")
    append_jsonl(path, row)


# ---------------------------------------------------------------- schema

BIN_KEYS = ("git_head", "axon_sha256", "axon_intent_sha256", "card_sha256", "prompt_set_sha256")
METRIC_KEYS = ("ft_pass", "ft_check", "ccp", "ccp_soundness", "runtime_fail", "ccp_any_round",
               "solve_at", "turns_mean", "false_acceptance", "truncated", "budget_stop", "per_tier")
NESTED = {
    "set": ("version", "manifest_sha256", "n", "drafted_by"),
    "base": BIN_KEYS, "cand": BIN_KEYS,
    "model": ("series", "requested", "served", "provider", "models_entry_sha256",
              "server_build_sha256", "temperature", "seeds", "k"),
    "paired": ("delta_ft_pass", "ci95", "p_two_sided", "n_tasks", "n_families", "delta_solve3"),
    "python": ("ft_pass", "solve3", "gap_ft", "gap_solve3", "drift_vs_last"),
    "classes": ("check", "panic", "ended"),
    "cache": ("hits", "misses", "replayed", "mismatches"),
    "cost": ("calls", "tokens_in", "tokens_out"),
}
NONE = type(None)
TOP = {"schema": str, "run_id": str, "date": str, "label": str, "design": str,
       "confirms": (str, NONE), "paired_mode": (str, NONE), "set": dict, "driver_sha256": str,
       "prereg_sha256": (str, NONE), "base": dict, "cand": dict, "model": dict, "metrics": dict,
       "paired": (dict, NONE), "flag": (str, NONE), "advisory": bool, "t1_regression": bool,
       "python": (dict, NONE), "classes": dict, "classes_delta": dict, "infra": list,
       "cache": dict, "cost": dict, "wall_s": (int, float), "rows_sha256": str, "notes": str}
ENUMS = {"design": ("paired", "confirm", "bridge-model", "bridge-set"),
         "paired_mode": ("regrade", "resample", None),
         "flag": ("REGRESSION", "REGRESSION-UNCONFIRMED", "IMPROVED", "NO CHANGE", None)}


def validate_row(row):
    """Problems of a trend row against `axon-ae/1` (the spec AE-4 schema); [] when valid."""
    bad = []
    if not isinstance(row, dict):
        return ["not an object"]
    bad += [f"unknown key {k}" for k in sorted(set(row) - set(TOP))]
    for k, t in TOP.items():
        if k not in row:
            bad.append(f"missing {k}")
        elif not isinstance(row[k], t) or (t is not bool and isinstance(row[k], bool)
                                           and bool not in (t if isinstance(t, tuple) else (t,))):
            bad.append(f"{k} has type {type(row[k]).__name__}")
    if row.get("schema") != SCHEMA:
        bad.append(f"schema is not {SCHEMA}")
    for k, allowed in ENUMS.items():
        if k in row and row[k] not in allowed:
            bad.append(f"{k} {row[k]!r} is not one of {allowed}")
    for k, keys in NESTED.items():
        if isinstance(row.get(k), dict):
            bad += [f"{k}.{x} missing" for x in keys if x not in row[k]]
    for side in BINS:
        m = (row.get("metrics") or {}).get(side)
        if not isinstance(m, dict):
            bad.append(f"metrics.{side} missing")
        else:
            bad += [f"metrics.{side}.{x} missing" for x in METRIC_KEYS if x not in m]
    if isinstance(row.get("set"), dict) and isinstance(row["set"].get("n"), dict):
        bad += [f"set.n.{t} missing" for t in TIERS if t not in row["set"]["n"]]
    if isinstance(row.get("infra"), list) and len(row["infra"]) != 2:
        bad.append("infra is not [k, n]")
    if row.get("design") == "confirm" and not row.get("confirms"):
        bad.append("a confirm row names no run")
    return bad


# ---------------------------------------------------------------- statistics

def per_task_diffs(base, cand, key):
    """{task: candidate rate - base rate} over (task, trial) pairs where neither
    row is infra (D7); a task with no such pair is left out."""
    b = {(r["id"], r["trial"]): r for r in base}
    c = {(r["id"], r["trial"]): r for r in cand}
    acc = {}
    for k in sorted(set(b) & set(c)):
        if b[k].get("infra") or c[k].get("infra") or b[k].get("tempting"):
            continue
        s = acc.setdefault(k[0], [0, 0, 0])
        s[0] += bool(c[k].get(key))
        s[1] += bool(b[k].get(key))
        s[2] += 1
    return {i: (sc - sb) / n for i, (sc, sb, n) in acc.items()}


def signflip(diffs, family_of):
    """The family-clustered sign-flip of headroom/stats.py (spec §3, D6): Δ̂,
    the 95% CI = SignFlip.ci(alpha=0.025), two-sided p = min(1, 2 min(p+, p-))."""
    if not diffs:
        return None
    sf = stats.SignFlip(stats.program_sums(diffs, family_of), seed=0)
    lo, hi = sf.ci(alpha=CI_ALPHA)
    p = min(1.0, 2 * min(sf.p(0.0, "greater"), sf.p(0.0, "less")))
    return {"delta": round(stats.delta_hat(diffs), 6), "ci95": [round(lo, 6), round(hi, 6)],
            "p_two_sided": round(p, 6), "n_tasks": len(diffs), "n_families": len(sf.S),
            "method": sf.method}


def flag_of(paired, design):
    if paired is None:
        return None
    lo, hi = paired["ci95"]
    if hi < 0:
        return "REGRESSION" if design == "confirm" else "REGRESSION-UNCONFIRMED"
    return "IMPROVED" if lo > 0 else "NO CHANGE"


def resolve_flag(row, history):
    """The report's flag (spec AE-4): rows are never rewritten, so a headline
    regression is resolved against the latest `confirm` row naming the run."""
    if row.get("design") == "confirm" or row.get("flag") != "REGRESSION-UNCONFIRMED":
        return row.get("flag")
    conf = [h for h in history if h.get("design") == "confirm" and h.get("confirms") == row["run_id"]]
    if not conf or not conf[-1].get("paired"):
        return "REGRESSION-UNCONFIRMED"
    return "REGRESSION" if conf[-1]["paired"]["ci95"][1] < 0 else "NOT CONFIRMED"


# ---------------------------------------------------------------- the trend row

def run_metrics(rows):
    m = G.metrics(rows) if rows else {}
    return {k: m.get(k) for k in METRIC_KEYS}


def cache_and_cost(rows):
    calls = sum(r.get("calls", 0) for r in rows)
    hits = sum(r.get("cache_hits", 0) for r in rows)
    return ({"hits": hits, "misses": calls - hits - sum(r.get("shared_replies", 0) for r in rows),
             "replayed": sum(r.get("replayed", 0) for r in rows),
             "mismatches": sum(r.get("replay_mismatches", 0) for r in rows)},
            {"calls": calls, "tokens_in": sum(r.get("input_tokens", 0) for r in rows),
             "tokens_out": sum(r.get("output_tokens", 0) for r in rows)})


def python_block(py, cand, history, set_version, series):
    """M5 (spec §2): arm c's FT-pass and Solve@3, and the gap FT-pass(arm c) -
    FT-pass(arm a, candidate binary); Solve@3's gap the same way round."""
    if not py:
        return None
    s = G.scored_rows(py)
    ft = G.rate(sum(bool(r["b_verified"]) for r in s), len(s))
    solve3 = G.rate(sum(bool(r["verified"]) for r in s), len(s))
    cm = G.metrics(cand) if cand else {}
    sub = lambda x, y: round(x - y, 6) if x is not None and y is not None else None
    last = [h for h in history if (h.get("set") or {}).get("version") == set_version
            and (h.get("model") or {}).get("series") == series and h.get("python")]
    return {"ft_pass": ft, "solve3": solve3, "gap_ft": sub(ft, cm.get("ft_pass")),
            "gap_solve3": sub(solve3, (cm.get("solve_at") or [None] * 4)[3]),
            "drift_vs_last": sub(ft, last[-1]["python"]["ft_pass"]) if last else None}


def build_row(meta, rows, history, wall, notes):
    base, cand, py = rows["base"], rows["cand"], rows["python"]
    family_of = {r["id"]: r.get("family") or r["id"] for r in base + cand}
    ft = signflip(per_task_diffs(base, cand, "b_verified"), family_of)
    s3 = per_task_diffs(base, cand, "verified")
    paired = None if ft is None else {
        "delta_ft_pass": ft["delta"], "ci95": ft["ci95"], "p_two_sided": ft["p_two_sided"],
        "n_tasks": ft["n_tasks"], "n_families": ft["n_families"],
        "delta_solve3": round(stats.delta_hat(s3), 6)}
    t1 = {i for i, t in ((r["id"], r.get("tier")) for r in cand) if t == "T1"}
    d1 = {i: d for i, d in per_task_diffs(base, cand, "b_verified").items() if i in t1}
    cc, bc = G.class_counts(cand), G.class_counts(base)
    allr = base + cand + py
    cache, cost = cache_and_cost(allr)
    served = sorted({m for r in allr for m in r.get("served_models") or []})
    return {
        "schema": SCHEMA, "run_id": meta["run_id"], "date": meta["started"][:10],
        "label": meta["label"], "design": meta["design"], "confirms": meta["confirms"],
        "paired_mode": meta["paired_mode"],
        "set": meta["set"], "driver_sha256": meta["driver_sha256"],
        "prereg_sha256": meta["prereg_sha256"],
        "base": meta["bins"]["base"]["pins"], "cand": meta["bins"]["cand"]["pins"],
        "model": dict(meta["model"], served=served),
        "metrics": {"cand": run_metrics(cand), "base": run_metrics(base)},
        "paired": paired, "flag": flag_of(paired, meta["design"]), "advisory": meta["advisory"],
        "t1_regression": bool(d1) and stats.delta_hat(d1) < 0,
        "python": python_block(py, cand, history, meta["set"]["version"], meta["model"]["series"]),
        "classes": cc,
        "classes_delta": {k: {c: cc[k].get(c, 0) - bc[k].get(c, 0)
                              for c in sorted(set(cc[k]) | set(bc[k]))} for k in cc},
        "infra": [sum(bool(r.get("infra")) for r in allr), len(allr)],
        "cache": cache, "cost": cost, "wall_s": wall,
        "rows_sha256": "", "notes": "; ".join(notes)}


# ---------------------------------------------------------------- canaries

def canary_leaks(root, canaries):
    """Tracked files under `root` whose content holds one of `canaries`."""
    canaries = sorted({c for c in canaries if c})
    if not canaries:
        return []
    with tempfile.NamedTemporaryFile("w", suffix=".pat", delete=False) as fh:
        fh.write("\n".join(canaries) + "\n")
    try:
        r = subprocess.run(["git", "-C", root, "grep", "-l", "-F", "-f", fh.name],
                           capture_output=True, text=True)
    finally:
        os.unlink(fh.name)
    if r.returncode not in (0, 1):
        raise Refused(f"git grep under {root} failed: {r.stderr.strip()[:300]}")
    return r.stdout.split()


def public_root():
    if os.environ.get(G.SELFTEST_ENV) == "1" and os.environ.get(PUBLIC_ROOT_ENV):
        return os.environ[PUBLIC_ROOT_ENV]
    return gv.ROOT


def set_canaries(tasks):
    return [json.load(open(os.path.join(t["dir"], "task.json"))).get("canary") for t in tasks]


def reply_canary_hits(run_dir, canary_of):
    """Spec AE-3 contamination rule 3: every reply of the run (the transcripts
    under <run>/{base,cand,python}/s<seed>/transcripts/<id>.<arm>.json carry
    each round's answer verbatim) grepped for every set canary. A hit is a
    canary of a task other than the transcript's own: [(task, canary's task, file)]."""
    hits = []
    for b in BINS + ("python",):
        for dp, _, files in os.walk(os.path.join(run_dir, b)):
            if os.path.basename(dp) != "transcripts":
                continue
            for f in sorted(files):
                tid = f.rsplit(".", 2)[0]
                text = open(os.path.join(dp, f), errors="replace").read()
                hits += [(tid, other, os.path.join(dp, f)) for other, c in sorted(canary_of.items())
                         if c and other != tid and c in text]
    return hits


# ---------------------------------------------------------------- run

def sub_out(run_dir, name, seed):
    out = os.path.join(run_dir, name, f"s{seed}")
    for sub in ("calls", "transcripts"):
        os.makedirs(os.path.join(out, sub), exist_ok=True)
    return out


def load_run(runs, run_id, history):
    """(row, meta, paths, in_history) of a finished run."""
    paths = run_paths(runs, run_id)
    if not os.path.exists(paths["meta"]):
        raise Refused(f"no run {run_id} under {runs}")
    meta = json.load(open(paths["meta"]))
    hist = [h for h in history if h.get("run_id") == run_id]
    row = hist[-1] if hist else (json.load(open(paths["row"])) if os.path.exists(paths["row"]) else None)
    if row is None:
        raise Refused(f"run {run_id} has no row (it did not finish)")
    return row, meta, paths, bool(hist)


def used_seeds(history, runs, set_version):
    """Every seed any run on this set version used (history rows and private metas)."""
    used = set()
    for h in history:
        if (h.get("set") or {}).get("version") == set_version:
            used.update((h.get("model") or {}).get("seeds") or [])
    if os.path.isdir(runs):
        for d in os.listdir(runs):
            p = os.path.join(runs, d, "meta.json")
            if os.path.exists(p):
                m = json.load(open(p))
                if m["set"]["version"] == set_version:
                    used.update(m["model"]["seeds"])
    return used


def drafted_text(drafted_by):
    return " + ".join(DRAFTERS.get(x, x) for x in drafted_by.split("+"))


def cmd_run(a):
    t_start = time.time()
    set_dir = private_path(a.set_dir, "--set-dir")
    runs = private_path(a.runs_dir or os.path.join(PRIVATE, "runs"), "--runs-dir")
    http = a.generator == "http"
    man, man_sha, tasks = G.load_set(None, set_dir)
    if not tasks:
        raise Refused(f"{set_dir} has no task")
    if any(t.get("set_mode") != "ae" for t in tasks):
        raise Refused(f"{set_dir} is not an ae set")
    pins = prereg_pins(a.prereg)
    if pins["frozen"]:
        if pins["manifest_sha256"] != man_sha:
            raise Refused(f"the manifest sha {man_sha[:12]} is not PREREG's "
                          f"{(pins['manifest_sha256'] or 'none')[:12]} ({a.prereg})")
    elif not a.pilot:
        raise Refused(f"{a.prereg} is not FROZEN: pass --pilot for a pilot/dev run (no trend row)")
    set_version = pins["set_version"] if pins["frozen"] else "unfrozen"
    hist_path = a.history or (HISTORY if http else None)
    history = read_history(hist_path or HISTORY)

    design, confirms, prior = "paired", None, None
    if a.confirm:
        prow, pmeta, _, _ = load_run(runs, a.confirm, history)
        if pmeta["set"]["version"] != set_version or pmeta["set"]["manifest_sha256"] != man_sha:
            raise Refused(f"--confirm {a.confirm} ran on set {pmeta['set']['version']} "
                          f"({pmeta['set']['manifest_sha256'][:12]}), not this one")
        prior, design, confirms = pmeta, "confirm", a.confirm
        bins = {b: bins_of(pmeta["bins"][b]["dir"]) for b in BINS}
        k = len(pmeta["model"]["seeds"])
        used, seeds, s = used_seeds(history, runs, set_version), [], 0
        while len(seeds) < k:
            if s not in used:
                seeds.append(s)
            s += 1
    else:
        if not (a.base_bin and a.cand_bin):
            raise Refused("--base-bin and --cand-bin are required (or --confirm RUN_ID)")
        bins = {"base": bins_of(a.base_bin), "cand": bins_of(a.cand_bin)}
        seeds = [int(x) for x in a.seeds.split(",")]
        if len(set(seeds)) != len(seeds):
            raise Refused(f"--seeds repeats a seed: {seeds}")
    # The blackout guards the real CERE route; the selftest's fake server is loopback.
    end = window_end(datetime.datetime.now(), a.until,
                     http and os.environ.get(G.SELFTEST_ENV) != "1")

    run_id = time.strftime("%Y%m%d-%H%M%S") + "-" + re.sub(r"[^A-Za-z0-9._-]+", "-", a.label)
    paths = run_paths(runs, run_id)
    os.makedirs(paths["dir"], exist_ok=False)
    ns = argparse.Namespace(generator=a.generator, model=a.model, provider=a.provider, effort=None,
                            max_rounds=None, temperature=a.temperature, seed=None,
                            max_tokens=a.max_tokens, replay_seed=None, share_dir=None, arm="a",
                            cache_dir=None if a.confirm or not http else a.cache_dir,
                            deadline=end.timestamp() if end else None)
    if http:
        if ns.cache_dir:
            ns.cache_dir = private_path(ns.cache_dir, "--cache-dir")
        G.resolve_model(ns)
        series = G.prepare_http(ns, paths["dir"])
    else:
        series = {"series": f"local:{a.generator}", "provider": None, "requested": None,
                  "models_entry_sha256": None}
        ns.model = None
    if pins["frozen"] and pins["series"] and series["series"] != pins["series"]:
        raise Refused(f"the served model series {series['series']} is not PREREG's {pins['series']}: "
                      "a new series needs a bridge-model run first (spec §3)")
    if prior and prior["model"]["series"] != series["series"]:
        raise Refused(f"--confirm {a.confirm} ran on series {prior['model']['series']}, "
                      f"not {series['series']}")

    bp = {b: bin_pins(bins[b]) for b in BINS}
    if prior:
        for b in BINS:
            was = prior["bins"][b]["pins"]
            if (bp[b]["axon_sha256"], bp[b]["axon_intent_sha256"]) != (
                    was["axon_sha256"], was["axon_intent_sha256"]):
                raise Refused(f"--confirm {a.confirm}: the {b} binaries changed since that run")
    if a.expect_card_change and bp["base"]["card_sha256"] == bp["cand"]["card_sha256"]:
        raise Refused("--expect-card-change: the base and candidate cards are byte-equal "
                      "(no trend row; nothing was called)")
    with cf.ThreadPoolExecutor(max_workers=a.parallel) as ex:
        prompts = {b: dict(zip([t["id"] for t in tasks],
                               ex.map(lambda t, b=b: round0_prompt_sha(t, bins[b]), tasks)))
                   for b in BINS}
    paired_mode = "regrade" if prompts["base"] == prompts["cand"] else "resample"
    for b in BINS:
        bp[b]["prompt_set_sha256"] = prompt_set_sha256(prompts[b])
    labels = [h["label"] for h in history if (h.get("set") or {}).get("version") == set_version
              and h.get("design") == "paired" and not h["label"].startswith("weekly-")]
    first3 = list(dict.fromkeys(labels))[:3]
    advisory = bool(a.advisory or a.label.startswith("weekly-")
                    or len(first3) < 3 or a.label in first3)
    drafted = sorted({json.load(open(os.path.join(t["dir"], "task.json")))["drafted_by"]
                      for t in tasks})
    meta = {
        "run_id": run_id, "label": a.label, "design": design, "confirms": confirms,
        "started": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "window_end": end.isoformat(timespec="minutes") if end else None,
        "set_dir": set_dir, "pilot": bool(a.pilot), "advisory": advisory,
        "set": {"version": set_version, "manifest_sha256": man_sha,
                "n": {t: sum(1 for x in tasks if x.get("tier") == t) for t in TIERS},
                "drafted_by": "+".join(drafted)},
        "prereg_sha256": pins["sha256"], "driver_sha256": gv.sha256_file(G.driver_for(ns)),
        "generator": a.generator, "paired_mode": paired_mode,
        "model": {"series": series["series"], "requested": ns.model, "served": [],
                  "provider": series.get("provider"),
                  "models_entry_sha256": series.get("models_entry_sha256"),
                  "server_build_sha256": series.get("server_build_sha256"),
                  "temperature": ns.temperature if http else None, "seeds": seeds, "k": len(seeds)},
        "max_tokens": ns.max_tokens if http else None, "cache": bool(ns.cache_dir),
        "replay_seed": ns.replay_seed,
        "bins": {b: {"dir": os.path.dirname(bins[b][0]), "pins": bp[b]} for b in BINS},
        "round0_prompts": prompts,
    }
    with open(paths["meta"], "w") as fh:
        json.dump(meta, fh, indent=1)
    print(f"ae: run {run_id}: {len(tasks)} task(s) x seeds {seeds}, {paired_mode}, "
          f"{a.generator} {series['series']}", file=sys.stderr, flush=True)

    incomplete = threading.Event()
    share = os.path.join(paths["dir"], "share") if http and paired_mode == "regrade" else None

    def unit(u):
        i, t, s = u
        if incomplete.is_set() or (end and datetime.datetime.now() >= end):
            incomplete.set()
            return
        order = BINS if i % 2 == 0 else BINS[::-1]     # alternate which binary goes first
        for b in order:
            x = copy.copy(ns)
            x.seed, x.arm, x.share_dir = s, "a", share
            row = G.task_row(x, t, sub_out(paths["dir"], b, s), meta["set"]["version"], bins[b])
            row.update(trial=s, seed=s, bin=b)
            append_jsonl(paths["rows"][b], row)
        x = copy.copy(ns)
        x.seed, x.arm = s, "c"
        row = G.task_row(x, t, sub_out(paths["dir"], "python", s), meta["set"]["version"])
        row.update(trial=s, seed=s, bin="python")
        append_jsonl(paths["rows"]["python"], row)
        print(f"  {t['id']} s{s} done", file=sys.stderr, flush=True)

    units = [(i, t, s) for s in seeds for i, t in enumerate(tasks)]
    with cf.ThreadPoolExecutor(max_workers=a.parallel) as ex:
        list(ex.map(unit, units))
    # The driver makes no request past `end` (GENVIAB_DEADLINE); a unit still
    # grading then is late too: a run not done by the window's end is INCOMPLETE.
    if end and datetime.datetime.now() >= end:
        incomplete.set()
    rows = {b: read_rows(p) for b, p in paths["rows"].items()}
    notes = []
    if a.pilot:
        notes.append("pilot: PREREG-ae.md not frozen")
    if os.path.exists(os.path.join(paths["dir"], "run-state", "cache-disabled")):
        notes.append("D4 violated: a cache replay mismatched; the cache was off for the rest of the run")
    canary_of = {t["id"]: c for t, c in zip(tasks, set_canaries(tasks))}
    hits = reply_canary_hits(paths["dir"], canary_of)
    with open(os.path.join(paths["dir"], "canary-hits.json"), "w") as fh:   # private: names tasks
        json.dump([{"task": h[0], "canary_of": h[1], "file": h[2]} for h in hits], fh, indent=1)
    if hits:
        notes.append(f"CANARY in replies: {len(hits)} reply transcript(s) hold another task's "
                     "canary (memorisation signal, spec AE-3 rule 3; canary-hits.json in the run dir)")
        print(f"ae: CANARY: {len(hits)} reply transcript(s) hold another task's canary",
              file=sys.stderr)
    row = build_row(meta, rows, history, round(time.time() - t_start, 1), notes)
    row["rows_sha256"] = rows_digest(paths)
    why_not = []
    if incomplete.is_set():
        why_not.append(f"INCOMPLETE: the window ended at {meta['window_end']}")
    k, n = row["infra"]
    if n and k / n > INFRA_MAX:
        why_not.append(f"INVALID: infra {k}/{n} > 5%")
    if a.pilot:
        why_not.append("pilot run")
    if not hist_path:
        why_not.append(f"dry run ({a.generator}) without --history")
    leaks = canary_leaks(public_root(), set_canaries(tasks))
    if leaks:
        why_not.append(f"CANARY under git ls-files: {leaks[:5]}")
    if why_not:
        row["notes"] = "; ".join(notes + ["no trend row: " + "; ".join(why_not)])
    with open(paths["row"], "w") as fh:
        json.dump(row, fh, indent=1)
    if not why_not:
        append_history(hist_path, row)
    print(report(run_id, runs, hist_path or HISTORY))
    if why_not:
        print("ae: no trend row: " + "; ".join(why_not), file=sys.stderr)
        return 1 if leaks or incomplete.is_set() or (n and k / n > INFRA_MAX) else 0
    return 0


# ---------------------------------------------------------------- failure classes (AE-5)

QUOTED = re.compile(r"(['`])([^'`\s]{1,60})\1")
# A double-quoted span is a value (a string literal, a map key `{key:?}`), never
# a name: it can come from a hidden example, so every key drops it (spec AE-5).
DQUOTED = re.compile(r'"(?:[^"\\]|\\.)*"')


def own_name(name, src):
    """Is `name` defined by the candidate program itself (fn, let, param, type)?"""
    e = re.escape(name)
    return bool(re.search(rf"\b(?:fn|let|struct|enum|type|for)\s+{e}\b", src or "")
                or re.search(rf"[(,]\s*{e}\s*:", src or ""))


def normalise(msg, src):
    """The first 80 chars of a diagnostic message, the program's own names
    stripped to `_`, numbers to N: a class key, not a task's text."""
    s = QUOTED.sub(lambda m: f"{m.group(1)}_{m.group(1)}" if own_name(m.group(2), src)
                   else m.group(0), DQUOTED.sub('"_"', msg)[:80])
    return re.sub(r"\d+", "N", s)


def card_text(bins_dir):
    try:
        r = subprocess.run([os.path.join(bins_dir, "axon"), "card"], capture_output=True,
                           text=True, timeout=60)
        return r.stdout if r.returncode == 0 else None
    except OSError:
        return None


def class_rules():
    return json.load(open(CLASSES))["rules"]


def area_of(rules, **facts):
    for r in rules:
        if all(facts.get(k) == v or (k == "code_prefix" and str(facts.get("code", "")).startswith(v))
               for k, v in r.items() if k not in ("class", "area")):
            return r["class"], r["area"]
    return "unassigned", "unassigned"


def construct_of(reason):
    """A panic reason as a class key: only the construct, never data. Every
    quoted span (', `, ") becomes `_` (a key or a value can come from a hidden
    example), numbers N; a reason that matches no PANIC_KINDS pattern, or an
    explicit panic's free text, is reduced to its kind word."""
    s = (reason or "").strip()
    kind = gv.panic_kind(s)
    if kind == "other":
        return "other"
    if re.match(r"panic\b", s):
        return "panic(_)"
    s = re.sub(r"(['`])[^'`]*\1", r"\1_\1", DQUOTED.sub('"_"', s))
    return re.sub(r"\d+", "N", s)[:80]


def tally_run(run_dir, name, rows, card, rules):
    """Round-0 failure classes of one binary's rows: {key: {...}} plus the
    stall counts per check code and the scored / failing round-0 totals."""
    out, stall, scored, fail0 = {}, {}, 0, 0
    for r in G.scored_rows(rows):
        scored += 1
        fail0 += not r.get("b_verified")
        tr_path = os.path.join(run_dir, name, f"s{r['trial']}", "transcripts", f"{r['id']}.a.json")
        tr = json.load(open(tr_path)) if os.path.exists(tr_path) else {}
        rep = ((tr.get("rounds") or [{}])[0].get("report") or {})
        keys = {}
        for d in rep.get("diagnostics") or []:
            try:
                j = json.loads(d) if isinstance(d, str) else d
            except ValueError:
                continue
            if not isinstance(j, dict) or j.get("severity") != "error" or not j.get("code"):
                continue
            msg = j.get("message") or ""
            q = QUOTED.search(msg)
            in_card = None if card is None or not q else bool(
                re.search(rf"\b{re.escape(q.group(2))}\b", card))
            cls, area = area_of(rules, code=j["code"], name_in_card=in_card)
            keys[f"{j['code']} {normalise(msg, rep.get('source'))}"] = (cls, area)
        if r.get("b_ccp"):
            for run in (r.get("b_grade") or {}).get("runs") or []:
                if run.get("ended_class") == "panic":
                    m2 = "M2s" if run.get("panic_kind") == "type-class" else "M2"
                    cls, area = area_of(rules, kind=m2.lower(), panic_kind=run.get("panic_kind"))
                    keys[f"{m2} {run.get('panic_kind')}: {construct_of(run.get('panic_reason'))}"] = (cls, area)
        if r.get("b_runtime_fail"):
            for run in (r.get("b_grade") or {}).get("runs") or []:
                if run.get("ended_class") in G.RUNTIME_FAIL:
                    keys[f"M2f {run['ended_class']}"] = area_of(rules, kind="m2f",
                                                               ended=run["ended_class"])
        if not r.get("b_verified") and r.get("b_check_ok") and not keys:
            ended = {run.get("ended_class") for run in (r.get("b_grade") or {}).get("runs") or []}
            if "wrong-output" in ended or "exit-mismatch" in ended:
                keys["wrong output"] = area_of(rules, ended="wrong-output")
        for k, (cls, area) in keys.items():
            e = out.setdefault(k, {"class": cls, "area": area, "n": 0, "families": set(),
                                   "seeds": set(), "instances": []})
            e["n"] += 1
            e["families"].add(r.get("family") or r["id"])
            e["seeds"].add(r["trial"])
            if k.startswith("M2s "):
                e["instances"].append((r.get("family") or r["id"], r["trial"],
                                       ((r.get("b_grade") or {}).get("program_sha256") or "?")[:12]))
        codes = r.get("diag_codes_per_round") or []
        for i in range(len(codes) - 1):
            for c in set(codes[i] or []):
                s = stall.setdefault(c, [0, 0])
                s[1] += 1
                s[0] += c in (codes[i + 1] or [])
    return out, stall, scored, fail0


def opaque(family):
    return "fam-" + gv.sha256_bytes(family.encode())[:8]


def classes(run_ids, runs, history, min_share=0.03, min_families=5):
    """AE-5: class table, stall rates and draft opportunities stanzas (text)."""
    rules = class_rules()
    agg, stall, scored, fail0, versions, texts = {}, {}, 0, 0, set(), []
    for rid in run_ids:
        row, meta, paths, _ = load_run(runs, rid, history)
        versions.add((meta["set"]["version"], meta["set"]["manifest_sha256"]))
        texts += [open(os.path.join(meta["set_dir"], t, "intent.md")).read()
                  for t in sorted({r["id"] for r in read_rows(paths["rows"]["cand"])})
                  if os.path.exists(os.path.join(meta["set_dir"], t, "intent.md"))]
        card = card_text(meta["bins"]["cand"]["dir"])
        t, st, sc, f0 = tally_run(paths["dir"], "cand", read_rows(paths["rows"]["cand"]), card, rules)
        scored += sc
        fail0 += f0
        for k, e in t.items():
            a = agg.setdefault(k, {"class": e["class"], "area": e["area"], "n": 0,
                                   "families": set(), "seeds": set(), "runs": set(), "instances": []})
            a["n"] += e["n"]
            a["families"] |= e["families"]
            a["seeds"] |= e["seeds"]
            a["runs"].add(rid)
            a["instances"] += [(rid,) + i for i in e["instances"]]
        for c, (k, n) in st.items():
            s = stall.setdefault(c, [0, 0])
            s[0] += k
            s[1] += n
    if len(versions) > 1:
        raise Refused(f"the runs span set versions {sorted(versions)}: classes aggregate one version")
    out = [f"## failure classes (round 0, candidate binary): {len(run_ids)} run(s), "
           f"{scored} scored task-trials, {fail0} failing round 0",
           "| class key | class | area | task-trials | share | families | runs | seeds |",
           "|---|---|---|---|---|---|---|---|"]
    ranked = sorted(agg.items(), key=lambda kv: (-kv[1]["n"], kv[0]))
    for k, e in ranked:
        out.append(f"| {k} | {e['class']} | {e['area']} | {e['n']} | "
                   f"{e['n'] / scored if scored else 0:.3f} | {len(e['families'])} | "
                   f"{len(e['runs'])} | {sorted(e['seeds'])} |")
    out.append("## stall rate per check code (the code reappears in the next round)")
    out.append("| code | stalled | rounds | rate |")
    out.append("|---|---|---|---|")
    for c, (k, n) in sorted(stall.items(), key=lambda kv: (-kv[1][0] / kv[1][1], kv[0])):
        out.append(f"| {c} | {k} | {n} | {k / n:.3f} |")
    out.append("## M2s instances (every one, regardless of rate; a checker-soundness bug report each)")
    m2s = [(k, i) for k, e in ranked for i in e["instances"]]
    out += [f"- {k}: run {i[0]} family {opaque(i[1])} seed {i[2]} program {i[3]}" for k, i in m2s]
    if not m2s:
        out.append("- none")
    out.append("## draft opportunities stanzas (printed only; never written to opportunities.md)")
    promoted = 0
    for k, e in ranked:
        share = e["n"] / scored if scored else 0
        if not ((share >= min_share or len(e["families"]) >= min_families)
                and len(e["runs"]) >= 2 and len(e["seeds"]) >= 2):
            continue
        promoted += 1
        key = QUOTED.sub(lambda m: m.group(0) if not any(m.group(2) in x for x in texts)
                         else f"{m.group(1)}_{m.group(1)}", k)
        fams = ", ".join(sorted(opaque(f) for f in e["families"])[:12])
        out += [f"### (draft) authoring-eval class `` {key} ``",
                f"- Class: {e['class']} (owner area: {e['area']}); round-0 task-trials {e['n']}/{scored} "
                f"({share:.1%}); {len(e['families'])} families ({fams}); "
                f"{len(e['runs'])} runs, seeds {sorted(e['seeds'])}.",
                f"- Upper bound on FT-pass gain: {e['n'] / fail0 if fail0 else 0:.1%} "
                "(the class's share of round-0 failures). Measured on CERE Qwen only.",
                "- Measured effect: none yet; after the fix, this class's task subset (fixed now) is "
                "re-run as a fresh paired pair (spec AE-5)."]
    if not promoted:
        out.append("(no class meets the promotion rule: >= 3% of scored round-0 task-trials or "
                   ">= 5 families, in >= 2 runs with different seeds)")
    return "\n".join(out)


# ---------------------------------------------------------------- report

def fmt(x):
    return "-" if x is None else f"{x:.3f}"


def d4_diff(paths_a, paths_b):
    """Replies whose sha differs between two runs, call by call (D4)."""
    diffs = []
    for b in BINS + ("python",):
        root_a, root_b = os.path.dirname(paths_a["rows"][b]), os.path.dirname(paths_b["rows"][b])
        for dirpath, _, files in os.walk(root_a):
            if os.path.basename(dirpath) != "calls":
                continue
            for f in sorted(files):
                rel = os.path.relpath(os.path.join(dirpath, f), root_a)
                ra = G.call_rows(os.path.join(root_a, rel))
                rb = G.call_rows(os.path.join(root_b, rel))
                for i in range(max(len(ra), len(rb))):
                    sa = ra[i].get("reply_sha256") if i < len(ra) else None
                    sb = rb[i].get("reply_sha256") if i < len(rb) else None
                    if sa != sb:
                        diffs.append(f"{b}/{rel}#{i}: {(sa or '-')[:12]} vs {(sb or '-')[:12]}")
    return diffs


def report(run_id, runs, history_path, against=None):
    history = read_history(history_path)
    row, meta, paths, in_hist = load_run(runs, run_id, history)
    if rows_digest(paths) != row["rows_sha256"]:
        raise Refused(f"run {run_id}: rows_sha256 does not match its private rows "
                      "(edited or partial rows; the row is refused)")
    rows = {b: read_rows(p) for b, p in paths["rows"].items()}
    s = row["set"]
    out = [f"{s['version']}, drafted by {drafted_text(s['drafted_by'])}",
           f"run {run_id}: label {row['label']}, design {row['design']}"
           + (f" (confirms {row['confirms']})" if row["confirms"] else "")
           + f", paired_mode {row['paired_mode']}, model {row['model']['series']}, "
           f"seeds {row['model']['seeds']} (k {row['model']['k']}), "
           + ("in history.jsonl" if in_hist else "NOT in history.jsonl (private row only)")]
    mb, mc, p = row["metrics"]["base"], row["metrics"]["cand"], row["paired"]
    flag = resolve_flag(row, history)
    if p:
        out.append(f"FT-pass {fmt(mb['ft_pass'])} -> {fmt(mc['ft_pass'])}: "
                   f"Δ {p['delta_ft_pass']:+.3f} [95% CI {p['ci95'][0]:+.3f}, {p['ci95'][1]:+.3f}], "
                   f"p (two-sided) {p['p_two_sided']:.3f}, {p['n_tasks']} tasks / "
                   f"{p['n_families']} families -> {flag}"
                   + (" T1-REGRESSION" if row["t1_regression"] else "")
                   + (" (advisory: never blocks)" if row["advisory"] else ""))
        out.append(f"Solve@3 Δ {p['delta_solve3']:+.3f} (secondary, D2)")
    else:
        out.append("FT-pass: no paired task (every pair infra)")
    for b in ("cand", "base"):
        out.append(f"## {b} ({row[b]['git_head']}, card {row[b]['card_sha256'][:12]})")
        out.extend(G.metric_lines(rows[b]) if rows[b] else ["- no rows"])
    out.append("- per tier (descriptive, never flags): " + ("; ".join(
        f"{t} FT-pass {fmt((mb['per_tier'].get(t) or {}).get('ft_pass'))} -> "
        f"{fmt((mc['per_tier'].get(t) or {}).get('ft_pass'))} (n {(mc['per_tier'].get(t) or {}).get('n', 0)})"
        for t in TIERS if t in (mc["per_tier"] or {})) or "none"))
    py = row["python"]
    if py:
        family_of = {r["id"]: r.get("family") or r["id"] for r in rows["cand"]}
        # §2 M5: arm c - arm a (per_task_diffs(x, y) is y - x).
        g = signflip(per_task_diffs(rows["cand"], rows["python"], "b_verified"), family_of)
        out.append(f"- Python (arm c, M5): FT-pass {fmt(py['ft_pass'])}, Solve@3 {fmt(py['solve3'])}; "
                   f"gap FT (Python - Axon cand) {fmt(py['gap_ft'])}"
                   + (f" [95% CI {g['ci95'][0]:+.3f}, {g['ci95'][1]:+.3f}] over {g['n_tasks']} tasks"
                      if g else "")
                   + f"; gap Solve@3 {fmt(py['gap_solve3'])}; drift_vs_last {fmt(py['drift_vs_last'])}")
    else:
        out.append("- Python (arm c): no rows")
    c, k = row["cache"], row["cost"]
    out.append(f"- infra: {row['infra'][0]}/{row['infra'][1]}; cache: {c['hits']} hits, {c['misses']} "
               f"misses, {c['replayed']} replayed, {c['mismatches']} mismatches; cost: {k['calls']} calls, "
               f"tokens {k['tokens_in']} in / {k['tokens_out']} out ($0, CERE); wall {row['wall_s']} s")
    if row["notes"]:
        out.append(f"- notes: {row['notes']}")
    rules = class_rules()
    card = card_text(meta["bins"]["cand"]["dir"])
    tb = tally_run(paths["dir"], "base", rows["base"], card, rules)[0]
    tc = tally_run(paths["dir"], "cand", rows["cand"], card, rules)[0]
    top = sorted(set(tb) | set(tc), key=lambda x: (-max((tb.get(x) or {}).get("n", 0),
                                                        (tc.get(x) or {}).get("n", 0)), x))[:10]
    out.append("- top failure classes (round 0, task-trials; AE-5):")
    out.append("  | class key | area | base | cand |")
    out.append("  |---|---|---|---|")
    for x in top:
        e = tc.get(x) or tb.get(x)
        out.append(f"  | {x} | {e['area']} | {(tb.get(x) or {}).get('n', 0)} | "
                   f"{(tc.get(x) or {}).get('n', 0)} |")
    if against:
        orow, _, opaths, _ = load_run(runs, against, history)
        om = orow["metrics"]["cand"]["ft_pass"], orow["metrics"]["base"]["ft_pass"]
        diffs = d4_diff(paths, opaths)
        same = om == (mc["ft_pass"], mb["ft_pass"])
        out.append(f"- D4 against {against}: FT-pass cand {fmt(om[0])} / base {fmt(om[1])} "
                   f"({'reproduced exactly' if same else 'DIFFERS'}); replies whose sha differs: "
                   f"{len(diffs)}")
        out += [f"  - {d}" for d in diffs]
    return "\n".join(out)


# ---------------------------------------------------------------- main

def main(argv=None):
    ap = argparse.ArgumentParser(usage=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--set-dir", required=True)
    r.add_argument("--label", required=True)
    r.add_argument("--base-bin")
    r.add_argument("--cand-bin")
    r.add_argument("--seeds", default=DEFAULT_SEEDS)
    r.add_argument("--confirm")
    r.add_argument("--expect-card-change", action="store_true")
    r.add_argument("--advisory", action="store_true")
    r.add_argument("--pilot", action="store_true")
    r.add_argument("--generator", choices=("http", "oracle", "empty"), default="http")
    r.add_argument("--provider", default=None)
    r.add_argument("--model", default=None)
    r.add_argument("--temperature", type=float, default=0.7)
    r.add_argument("--max-tokens", type=int, default=8192)
    r.add_argument("--cache-dir", default=os.path.join(PRIVATE, "cache"))
    r.add_argument("--runs-dir")
    r.add_argument("--history")
    r.add_argument("--prereg", default=PREREG_AE)
    r.add_argument("--parallel", type=int, default=2)
    r.add_argument("--until")
    for name in ("report", "classes"):
        p = sub.add_parser(name)
        p.add_argument("run_ids", nargs="+" if name == "classes" else 1)
        p.add_argument("--runs-dir")
        p.add_argument("--history", default=HISTORY)
        if name == "report":
            p.add_argument("--against")
    c = sub.add_parser("check-canaries")
    c.add_argument("--set-dir", required=True)
    try:
        a = ap.parse_args(argv)
    except SystemExit as e:
        return 0 if e.code == 0 else 2
    try:
        runs = getattr(a, "runs_dir", None) or os.path.join(PRIVATE, "runs")
        if a.cmd == "run":
            return cmd_run(a)
        if a.cmd == "report":
            print(report(a.run_ids[0], runs, a.history, a.against))
            return 0
        if a.cmd == "classes":
            print(classes(a.run_ids, runs, read_history(a.history)))
            return 0
        tasks = [gv.load_task(d, require_ax=False) for d in gv.task_dirs(a.set_dir)]
        leaks = canary_leaks(public_root(), set_canaries(tasks))
        print(f"ae: {len(tasks)} canaries; tracked files holding one: {leaks or 'none'}")
        return 1 if leaks else 0
    except Refused as e:
        print(f"ae: REFUSED: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
