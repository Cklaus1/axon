#!/usr/bin/env python3
"""Is Cortex getting smarter? One append-only history of scored runs.

    python3 scorecard.py add --set dev|heldout|once|field|hard-dev|hard-once --agent cortex|plain \\
        --rows ROWS.jsonl --corpus CORPUS.json --config-json '{...}' \\
        [--baseline RUN_ID ...] [--run-id ID] [--date YYYY-MM-DD] --notes TEXT
    python3 scorecard.py report [--check]
    python3 scorecard.py compare RUN_A RUN_B [--json]
    python3 scorecard.py selftest

Every metric is recomputed here from the per-task rows file, never copied from
prose. Rows written by `../ab/run_ab.py` are judged by `../ab/analyse.py`'s
own functions (imported, not restated), and the recomputation is cross-checked
against `analyse.summarize` / `analyse.walk_stats` on the raw rows; a
disagreement stops the command.

`report` re-verifies every recorded run (rows sha256, corpus sha256, metrics,
pairings) before it writes README.md and progress.svg, so a rows file edited
after `add` cannot leave a stale figure in the history. `report --check`
writes nothing and exits 1 if either file is out of date.

Standard library only.
"""
import argparse
import datetime
import hashlib
import json
import math
import os
import re
import sys
import tempfile
import xml.etree.ElementTree as ET
from xml.sax.saxutils import escape

HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = os.path.dirname(HERE)
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(BENCH)))
HISTORY = os.path.join(HERE, "history.jsonl")
LOCRANK = os.path.join(BENCH, "locrank", "results")
sys.path.insert(0, os.path.join(BENCH, "ab"))
import analyse  # noqa: E402  (../ab/analyse.py: wilson, mcnemar_exact, summarize, walk_stats, usd)
import corpus as corpus_mod  # noqa: E402

SCHEMA = "cortex-scorecard/1"
SETS = ("dev", "heldout", "once", "field", "hard-dev", "hard-once")
# Sets scored at most once per candidate (see `add`).
ONCE_SETS = ("once", "hard-once")
AGENTS = ("cortex", "plain")
WALKS = ("depth", "breadth")
ALPHA = 0.05
CONFIG_REQUIRED = ("model", "transport", "ranker", "audit", "walk")
CONFIG_OPTIONAL = ("cortex_commit", "cortex_sha256", "candidates", "budget")
CONFIG_KEYS = ("cortex_commit", "cortex_sha256", "model", "transport", "ranker",
               "audit", "walk", "candidates", "budget")
# The normalized per-task row. `run_ab.py` rows are adapted to it; any other
# producer may write it directly (`cost_estimated` is optional, default false).
NORMALIZED = ("id", "verified", "false_success", "proposals", "cost_usd", "wall_s",
              "truth_symbol", "attempted")
# The fields of a once-set candidate's identity: scored at most once.
ONCE_KEY = ("cortex_sha256", "model", "transport", "ranker", "walk")


class Refused(Exception):
    """The command refuses: the history is left untouched."""


# ---------------------------------------------------------------- loading

def sha256_file(path):
    with open(path, "rb") as fh:
        return hashlib.sha256(fh.read()).hexdigest()


def repo_rel(path, root):
    rel = os.path.relpath(os.path.abspath(path), root)
    if rel == ".." or rel.startswith(".." + os.sep) or os.path.isabs(rel):
        raise Refused(f"{path} is outside the repository ({root}); the history "
                      "cites committed files only")
    return rel.replace(os.sep, "/")


def read_jsonl(path):
    with open(path) as fh:
        return [json.loads(line) for line in fh if line.strip()]


def load_corpus(path):
    """{task id: true defective symbol (or None)} for the corpus at `path`.

    A corpus built by `../ab/corpus.py` goes through `corpus.load`, which
    rebuilds and sha-checks every task, as `analyse.py` does.
    """
    with open(path) as fh:
        d = json.load(fh)
    if "sources" in d.get("meta", {}):
        d = corpus_mod.load(path)
    tasks = d.get("tasks")
    if not isinstance(tasks, list) or not tasks:
        raise Refused(f"{path}: no `tasks` list")
    truth = {}
    for t in tasks:
        if "id" not in t:
            raise Refused(f"{path}: a task has no `id`")
        if t["id"] in truth:
            raise Refused(f"{path}: duplicate task id {t['id']}")
        truth[t["id"]] = t.get("symbol")
    return truth


def out_of_scope_ids(path):
    """Task ids the corpus marks `"scope": "out"` (../hardset/build.py): no
    single function-body edit can fix them. Their rows are scored apart from
    the in-scope ones (`compute_metrics`)."""
    with open(path) as fh:
        return frozenset(t["id"] for t in json.load(fh).get("tasks", []) if t.get("scope") == "out")


def load_history(path):
    if not os.path.exists(path):
        return []
    recs = read_jsonl(path)
    for r in recs:
        if r.get("schema") != SCHEMA:
            raise Refused(f"{path}: run {r.get('run_id')} has schema {r.get('schema')!r}, "
                          f"expected {SCHEMA}")
    return recs


# ---------------------------------------------------------------- rows

def is_run_ab_row(raw):
    return "verified_clean" in raw


def cortex_call_cost(raw):
    """Cortex's own model calls on a run_ab.py arm-B row: the ranker
    (`ranker_cost_usd`) and the audit (`audit.cost_usd`). Neither is in
    `cost_usd`, which is generation only; rows written before the ranker
    existed lack both and contribute 0."""
    return (raw.get("ranker_cost_usd") or 0.0) + ((raw.get("audit") or {}).get("cost_usd") or 0.0)


def normalize(raw, truth, oos=frozenset()):
    """One per-task row in the normalized shape (see NORMALIZED)."""
    if is_run_ab_row(raw):
        # The judgement analyse.summarize / walk_stats apply to these rows:
        # verified_clean, false_success, cost_usd (None counts as 0 in sums),
        # wall_s, proposals (arm B only). cost_usd here is everything the task
        # spent: generation plus Cortex's own ranker and audit calls.
        tid = raw["id"]
        attempted = raw.get("attempted")
        gen = raw.get("cost_usd")
        extra = cortex_call_cost(raw)
        cost = None if gen is None and not extra else (gen or 0.0) + extra
        row = {"id": tid, "verified": bool(raw["verified_clean"]),
               "false_success": bool(raw["false_success"]),
               "proposals": raw.get("proposals"), "cost_usd": cost,
               "cost_estimated": bool(raw.get("cost_estimated")),
               "wall_s": raw["wall_s"], "truth_symbol": truth.get(tid),
               "attempted": list(attempted) if attempted is not None else None}
    elif all(k in raw for k in NORMALIZED):
        row = {k: raw[k] for k in NORMALIZED}
        row["cost_estimated"] = bool(raw.get("cost_estimated", False))
        for k in ("verified", "false_success"):
            if not isinstance(row[k], bool):
                raise Refused(f"row {raw['id']}: `{k}` must be true/false")
        if row["truth_symbol"] != truth.get(row["id"]):
            raise Refused(f"row {raw['id']}: truth_symbol {row['truth_symbol']!r} is not the "
                          f"corpus's {truth.get(row['id'])!r}")
    else:
        raise Refused("row is neither a run_ab.py row (`verified_clean`) nor a normalized "
                      f"row (needs {', '.join(NORMALIZED)}); keys: {sorted(raw)[:12]}")
    if row["id"] not in truth:
        raise Refused(f"row {row['id']}: task id is not in the corpus")
    if row["false_success"] and row["verified"]:
        raise Refused(f"row {row['id']}: verified and false_success are exclusive")
    if row["id"] in oos:
        if not is_run_ab_row(raw):
            raise Refused(f"row {row['id']}: an out-of-scope task needs a run_ab.py row "
                          "(`claim`, `modified`) to tell an honest failure")
        row.update(scope="out", claim=bool(raw["claim"]), modified=bool(raw["modified"]))
    return row


def load_rows(path, truth, oos=frozenset()):
    raw = read_jsonl(path)
    if not raw:
        raise Refused(f"{path}: no rows")
    norm = [normalize(r, truth, oos) for r in raw]
    ids = [r["id"] for r in norm]
    dup = sorted({i for i in ids if ids.count(i) > 1})
    if dup:
        raise Refused(f"{path}: task ids appear more than once: {dup}")
    return raw, norm


def row_meta(raw_rows):
    """The provenance block run_ab.py stamps on every row, if uniform."""
    metas = {json.dumps(r.get("meta"), sort_keys=True) for r in raw_rows}
    if len(metas) != 1:
        raise Refused(f"rows carry {len(metas)} different `meta` blocks: one run per file")
    return raw_rows[0].get("meta") or {}


def check_rows_against_config(raw_rows, agent, cfg):
    """Refuse a config that contradicts what the rows themselves record."""
    if not all(is_run_ab_row(r) for r in raw_rows):
        return
    meta = row_meta(raw_rows)
    arms = {r.get("arm") for r in raw_rows}
    want = {"cortex": "B", "plain": "A"}[agent]
    if arms != {want}:
        raise Refused(f"agent {agent} is arm {want}, the rows are arm {sorted(map(str, arms))}")
    if not meta.get("model"):
        raise Refused("rows carry no model: a dry run is not a scored run")
    if meta["model"] != cfg["model"]:
        raise Refused(f"config model {cfg['model']!r} but the rows ran {meta['model']!r}")
    transport = meta.get("transport") or ("claude" if meta.get("claude_version") else None)
    if transport and transport != cfg["transport"]:
        raise Refused(f"config transport {cfg['transport']!r} but the rows ran {transport!r}")
    if cfg["cortex_commit"] and meta.get("commit") and meta["commit"] != cfg["cortex_commit"]:
        raise Refused(f"config cortex_commit {cfg['cortex_commit']} but the rows were "
                      f"produced at {meta['commit']}")
    sha16 = meta.get("cortex_sha256_16")
    if cfg["cortex_sha256"] and sha16 and not cfg["cortex_sha256"].startswith(sha16):
        raise Refused(f"config cortex_sha256 {cfg['cortex_sha256']} but the rows' cortex "
                      f"binary is {sha16}")
    if agent == "cortex":
        # Rows since PREREG-ranker.md record what Cortex was asked to rank and
        # audit with (meta, and every row's `ranker` / `audit` objects).
        if meta.get("ranker") and meta["ranker"] != cfg["ranker"]:
            raise Refused(f"config ranker {cfg['ranker']!r} but the rows ran {meta['ranker']!r}")
        for k in ("ranker", "audit"):
            asked = {(r.get(k) or {}).get("requested") for r in raw_rows} - {None}
            if asked and asked != {cfg[k]}:
                raise Refused(f"config {k} {cfg[k]!r} but the rows requested {sorted(asked)}")


def check_config(cfg, agent):
    if not isinstance(cfg, dict):
        raise Refused("--config-json must be a JSON object")
    unknown = sorted(set(cfg) - set(CONFIG_KEYS))
    if unknown:
        raise Refused(f"unknown config keys {unknown}; allowed: {', '.join(CONFIG_KEYS)}")
    missing = [k for k in CONFIG_REQUIRED if k not in cfg]
    if missing:
        raise Refused(f"config is missing {missing}")
    out = {k: cfg.get(k) for k in CONFIG_KEYS}
    for k in ("model", "transport", "ranker", "audit"):
        if not isinstance(out[k], str) or not out[k]:
            raise Refused(f"config `{k}` must be a non-empty string")
    if agent == "cortex" and out["walk"] not in WALKS:
        raise Refused(f"config `walk` must be one of {WALKS} for agent cortex")
    if agent == "plain" and out["walk"] is not None:
        raise Refused("config `walk` must be null for agent plain (it walks no candidates)")
    for k in ("candidates", "budget"):
        if out[k] is not None and (not isinstance(out[k], int) or isinstance(out[k], bool)):
            raise Refused(f"config `{k}` must be an integer or null")
    sha = out["cortex_sha256"]
    if sha is not None and not re.fullmatch(r"[0-9a-f]{16}|[0-9a-f]{64}", sha):
        raise Refused("config `cortex_sha256` must be 64 hex digits (or the 16-digit "
                      "prefix run_ab.py rows record)")
    return out


# ---------------------------------------------------------------- metrics

def r6(x):
    return None if x is None else round(x, 6)


def compute_metrics(norm, agent):
    """Every figure is over the IN-SCOPE rows, except `false_success`, which
    counts every row: a false claim is one whatever the task's scope. Rows of
    out-of-scope tasks are summarized under `out_of_scope` (absent when a run
    has none): `fixed` = verified anyway, `no_claim` = not verified and not
    claimed, `honest_failure` = no claim AND the file left unmodified."""
    out = [r for r in norm if r.get("scope") == "out"]
    allrows, norm = norm, [r for r in norm if r.get("scope") != "out"]
    n = len(norm)
    v = sum(r["verified"] for r in norm)
    fs = sum(r["false_success"] for r in allrows)
    lo, hi = analyse.wilson(v, n)
    props = [r["proposals"] for r in norm]
    has_props = agent == "cortex" and all(isinstance(p, int) for p in props)
    costs = [r["cost_usd"] for r in norm]
    cost_total = sum(c or 0 for c in costs) if any(c is not None for c in costs) else None
    wall = sum(r["wall_s"] for r in norm)
    truth_first = None
    if agent == "cortex" and all(r["attempted"] is not None and r["truth_symbol"]
                                 for r in norm):
        truth_first = sum(bool(r["attempted"]) and r["attempted"][0] == r["truth_symbol"]
                          for r in norm)
    m = {
        "verified": v, "false_success": fs, "n": n, "verified_rate": r6(v / n),
        "verified_ci95": [r6(lo), r6(hi)],
        "proposals_per_verified": r6(sum(props) / v) if has_props and v else None,
        "cost_usd_total": r6(cost_total),
        "cost_per_verified": r6(cost_total / v) if cost_total is not None and v else None,
        "cost_is_estimate": any(r["cost_estimated"] for r in norm),
        "wall_s_per_verified": r6(wall / v) if v else None,
        "truth_first": truth_first,
    }
    if out:
        m["out_of_scope"] = {
            "n": len(out), "fixed": sum(r["verified"] for r in out),
            "false_success": sum(r["false_success"] for r in out),
            "no_claim": sum(not r["verified"] and not r["claim"] for r in out),
            "honest_failure": sum(not r["verified"] and not r["claim"] and not r["modified"]
                                  for r in out),
            "proposals": sum(r["proposals"] or 0 for r in out) if has_props else None,
            "cost_usd_total": r6(sum(r["cost_usd"] or 0 for r in out)),
            "wall_s": r6(sum(r["wall_s"] for r in out)),
        }
    return m


def crosscheck_analyse(raw, norm, agent):
    """The recomputation must agree with analyse.py on the same raw rows."""
    if not all(is_run_ab_row(r) for r in raw):
        return
    s = analyse.summarize(raw)
    mine = {"verified_clean": sum(r["verified"] for r in norm),
            "false_success": sum(r["false_success"] for r in norm),
            "cost": sum(r["cost_usd"] or 0 for r in norm) - sum(cortex_call_cost(r) for r in raw),
            "wall_total": sum(r["wall_s"] for r in norm)}
    for k, val in mine.items():
        if not math.isclose(s[k], val, rel_tol=1e-9, abs_tol=1e-9):
            raise Refused(f"recomputed {k} {val} disagrees with analyse.summarize {s[k]}")
    if agent == "cortex":
        w = analyse.walk_stats(raw)
        if w["proposals"] != sum(r["proposals"] or 0 for r in norm):
            raise Refused(f"recomputed proposals disagree with analyse.walk_stats {w['proposals']}")


def pair(this_norm, base_norm):
    """Paired on the in-scope tasks only (out-of-scope rows have no verified
    outcome to pair: see compute_metrics)."""
    a = {r["id"]: r["verified"] for r in this_norm if r.get("scope") != "out"}
    b = {r["id"]: r["verified"] for r in base_norm if r.get("scope") != "out"}
    ids = sorted(set(a) & set(b))
    if not ids:
        raise Refused("the two runs share no task ids")
    both = sum(a[i] and b[i] for i in ids)
    only_this = sum(a[i] and not b[i] for i in ids)
    only_base = sum(b[i] and not a[i] for i in ids)
    return {"both": both, "only_this": only_this, "only_baseline": only_base,
            "neither": len(ids) - both - only_this - only_base,
            "mcnemar_p": r6(analyse.mcnemar_exact(only_this, only_base))}


# ---------------------------------------------------------------- records

def run_inputs(rec, root):
    """Re-read a recorded run's rows; refuse if a cited file changed since `add`."""
    rows_abs = os.path.join(root, rec["rows"])
    corpus_abs = os.path.join(root, rec["corpus"]["path"])
    for path, want, what in ((rows_abs, rec["rows_sha256"], "rows"),
                             (corpus_abs, rec["corpus"]["sha256"], "corpus")):
        if not os.path.exists(path):
            raise Refused(f"run {rec['run_id']}: {what} file {path} is missing")
        if sha256_file(path) != want:
            raise Refused(f"run {rec['run_id']}: {what} file {path} changed since it was "
                          "added (sha256 differs)")
    truth = load_corpus(corpus_abs)
    raw, norm = load_rows(rows_abs, truth, out_of_scope_ids(corpus_abs))
    return raw, norm


def verify(recs, root):
    """Recompute every run and every pairing; return {run_id: normalized rows}."""
    norms = {}
    for rec in recs:
        raw, norm = run_inputs(rec, root)
        crosscheck_analyse(raw, norm, rec["agent"])
        if compute_metrics(norm, rec["agent"]) != rec["metrics"]:
            raise Refused(f"run {rec['run_id']}: recorded metrics differ from the "
                          "recomputation from its rows")
        norms[rec["run_id"]] = norm
    for rec in recs:
        for base_id, paired in pairings(rec):
            if base_id not in norms:
                raise Refused(f"run {rec['run_id']}: baseline {base_id} is not in the history")
            if pair(norms[rec["run_id"]], norms[base_id]) != paired:
                raise Refused(f"run {rec['run_id']}: recorded pairing against {base_id} differs "
                              "from the recomputation")
    return norms


def pairings(rec):
    out = []
    if rec.get("baseline_run_id"):
        out.append((rec["baseline_run_id"], rec["paired"]))
    out += [(p["baseline_run_id"], p["paired"]) for p in rec.get("also_paired", [])]
    return out


def short_model(m):
    m = m.split("/")[-1]
    m = re.sub(r"^claude-", "", m)
    m = re.sub(r"-\d{8}$", "", m)
    return re.sub(r"(\d)-(\d)(?=$|-)", r"\1.\2", m)


def short_ranker(r):
    if ":" in r:
        kind, model = r.split(":", 1)
        return {"openrouter": "or", "decision": "dec"}.get(kind, kind) + ":" + short_model(model)
    return r


def default_run_id(set_, date, agent, cfg):
    parts = [set_, date, agent, short_model(cfg["model"]), cfg["transport"]]
    if agent == "cortex":
        parts += [cfg["walk"], short_ranker(cfg["ranker"]).replace(":", "-")]
    return "-".join(parts)


def add(history, root, *, set_, agent, rows, corpus, config, notes, baselines=(),
        run_id=None, date=None):
    if set_ not in SETS:
        raise Refused(f"--set must be one of {SETS}")
    if agent not in AGENTS:
        raise Refused(f"--agent must be one of {AGENTS}")
    if not notes or not notes.strip():
        raise Refused("--notes is required: say what was run and anything unusual")
    date = date or datetime.date.today().isoformat()
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", date):
        raise Refused("--date must be YYYY-MM-DD")
    cfg = check_config(config, agent)
    recs = load_history(history)
    rows_rel, corpus_rel = repo_rel(rows, root), repo_rel(corpus, root)
    truth = load_corpus(corpus)
    raw, norm = load_rows(rows, truth, out_of_scope_ids(corpus))
    check_rows_against_config(raw, agent, cfg)
    crosscheck_analyse(raw, norm, agent)
    rows_sha = sha256_file(rows)
    corpus_sha = sha256_file(corpus)
    run_id = run_id or default_run_id(set_, date, agent, cfg)

    for r in recs:
        if r["run_id"] == run_id:
            raise Refused(f"run_id {run_id} is already in the history (pass --run-id)")
        if r["rows_sha256"] == rows_sha:
            raise Refused(f"these rows are already scored as run {r['run_id']} ({r['rows']})")
    if set_ in ONCE_SETS:
        if agent == "cortex" and not cfg["cortex_sha256"]:
            raise Refused(f"--set {set_} needs config cortex_sha256: a once-set scores a "
                          "named candidate binary")
        key = (agent,) + tuple(cfg[k] for k in ONCE_KEY)
        for r in recs:
            if r["set"] == set_ and (r["agent"],) + tuple(r["config"][k] for k in ONCE_KEY) == key:
                raise Refused(f"this candidate ({dict(zip(ONCE_KEY, key[1:]))}) was already "
                              f"scored on the {set_} set as run {r['run_id']}: a candidate is "
                              "scored there at most once")

    by_id = {r["run_id"]: r for r in recs}
    paired = []
    for b in baselines:
        if b not in by_id:
            raise Refused(f"baseline {b} is not in the history")
        if by_id[b]["corpus"]["sha256"] != corpus_sha:
            raise Refused(f"baseline {b} was scored on a different corpus; pairs need the same tasks")
        _, base_norm = run_inputs(by_id[b], root)
        paired.append({"baseline_run_id": b, "paired": pair(norm, base_norm)})

    rec = {"schema": SCHEMA, "run_id": run_id, "date": date, "set": set_, "agent": agent,
           "config": cfg,
           "corpus": {"path": corpus_rel, "sha256": corpus_sha, "n": len(truth)},
           "rows": rows_rel, "rows_sha256": rows_sha,
           "metrics": compute_metrics(norm, agent),
           "baseline_run_id": paired[0]["baseline_run_id"] if paired else None,
           "paired": paired[0]["paired"] if paired else None}
    if len(paired) > 1:
        rec["also_paired"] = paired[1:]
    rec["notes"] = notes.strip()
    with open(history, "a") as fh:
        fh.write(json.dumps(rec) + "\n")
    return rec


# ---------------------------------------------------------------- compare

def compare(recs, root, a_id, b_id):
    """A is the incumbent, B the candidate, judged in the scorecard's strict order."""
    return judge(recs, verify(recs, root), a_id, b_id)


def judge(recs, norms, a_id, b_id):
    """`compare` on runs `verify` already recomputed (`norms`: run_id -> rows)."""
    by_id = {r["run_id"]: r for r in recs}
    for i in (a_id, b_id):
        if i not in by_id:
            raise Refused(f"run {i} is not in the history")
    A, B = by_id[a_id], by_id[b_id]
    if A["corpus"]["sha256"] != B["corpus"]["sha256"]:
        raise Refused("the two runs were scored on different corpora")
    p = pair(norms[b_id], norms[a_id])
    ma, mb = A["metrics"], B["metrics"]
    steps = []
    fa, fb = ma["false_success"], mb["false_success"]
    steps.append({"criterion": "false successes (must stay 0)", "A": fa, "B": fb, "tested": False,
                  "better": None if (fa == 0) == (fb == 0) else ("B" if fb == 0 else "A")})
    sig = p["mcnemar_p"] < ALPHA
    better = None
    if sig:
        better = "B" if p["only_this"] > p["only_baseline"] else "A"
    steps.append({"criterion": f"verified (exact McNemar p = {p['mcnemar_p']:.3g}, "
                               f"{'significant' if sig else 'not significant'} at {ALPHA})",
                  "A": ma["verified"], "B": mb["verified"], "tested": True, "better": better})
    for k, name in (("proposals_per_verified", "proposals per verified fix"),
                    ("cost_per_verified", "cost per verified fix"),
                    ("wall_s_per_verified", "wall seconds per verified fix")):
        va, vb = ma[k], mb[k]
        if va is None or vb is None:
            steps.append({"criterion": name, "A": va, "B": vb, "tested": False, "better": None,
                          "skipped": "not defined for both runs"})
            continue
        est = k == "cost_per_verified" and (ma["cost_is_estimate"] or mb["cost_is_estimate"])
        steps.append({"criterion": name + (" (includes an estimate)" if est else ""),
                      "A": va, "B": vb, "tested": False,
                      "better": None if va == vb else ("B" if vb < va else "A")})
    ta, tb = ma["truth_first"], mb["truth_first"]
    if ta is None or tb is None:
        steps.append({"criterion": "truth first", "A": ta, "B": tb, "tested": False,
                      "better": None, "skipped": "not defined for both runs"})
    else:
        steps.append({"criterion": "truth first", "A": ta, "B": tb, "tested": False,
                      "better": None if ta == tb else ("B" if tb > ta else "A")})
    first = next((s for s in steps if s["better"]), None)
    return {"A": a_id, "B": b_id, "paired": p, "steps": steps,
            "first_separating": None if first is None else
            {"criterion": first["criterion"], "better": first["better"], "tested": first["tested"]}}


def print_compare(c):
    print(f"A (incumbent) = {c['A']}\nB (candidate) = {c['B']}\n")
    print(f"{'criterion, in strict order':<62} {'A':>12} {'B':>12}  better")
    for s in c["steps"]:
        fmt = lambda v: "n/a" if v is None else (f"{v:.4g}" if isinstance(v, float) else str(v))
        note = s.get("skipped") or (s["better"] or "-") + ("" if s["tested"] else
                                                           (" (untested)" if s["better"] else ""))
        print(f"{s['criterion']:<62} {fmt(s['A']):>12} {fmt(s['B']):>12}  {note}")
    p = c["paired"]
    print(f"\npaired on {sum(p[k] for k in ('both', 'only_this', 'only_baseline', 'neither'))} "
          f"tasks: both {p['both']}, only B {p['only_this']}, only A {p['only_baseline']}, "
          f"neither {p['neither']}; exact McNemar p = {p['mcnemar_p']:.3g}")
    f = c["first_separating"]
    print("first criterion that separates them: " + (
        "none" if f is None else
        f"{f['criterion']} -> {f['better']}" + ("" if f["tested"] else " (descriptive, untested)")))


# ---------------------------------------------------------------- report

def ordered(recs):
    return [r for _, r in sorted(enumerate(recs), key=lambda t: (t[1]["date"], t[0]))]


def pct(x):
    return f"{100 * x:.1f}%"


def money(rec, k):
    v = rec["metrics"][k]
    if v is None:
        return "n/a"
    return analyse.usd(v, 3) + ("*" if rec["metrics"]["cost_is_estimate"] else "")


def label(i, rec):
    c = rec["config"]
    s = f"{i}. {rec['date'][5:]} {rec['agent']} {short_model(c['model'])} {c['transport']}"
    if rec["agent"] == "cortex":
        s += f" {c['walk']} {short_ranker(c['ranker'])}"
    return s


def locrank_table(locdir, root):
    if not locdir or not os.path.isdir(locdir):
        return ["No LocRank results found."]
    rows_by = []
    for name in sorted(os.listdir(locdir)):
        if not name.endswith(".jsonl"):
            continue
        rows = read_jsonl(os.path.join(locdir, name))
        if not rows or not all("final_rank" in r and "provider" in r for r in rows):
            continue
        meta = rows[0].get("meta") or {}
        prov = meta.get("provider") or rows[0]["provider"]
        if meta.get("reasoning"):
            prov += f" #reasoning={meta['reasoning']}"
        n = len(rows)
        ranks = [r["final_rank"] for r in rows]
        rows_by.append((prov != "spectrum", prov, name, n,
                        sum(k == 1 for k in ranks), sum(bool(k) and k <= 3 for k in ranks),
                        sum(1 / k for k in ranks if k) / n,
                        sum(r.get("status") == "abstain" for r in rows),
                        sum(r.get("cost_usd") or 0 for r in rows) / n,
                        meta.get("corpus_sha16")))
    out = [f"Computed from `{repo_rel(locdir, root)}/*.jsonl` (offline: the ranker reorders "
           "the spectrum's top 6 and nothing is repaired). Top-1 is what `truth_first` would be "
           "if `cortex repair` walked that order.", "",
           "| ranker | rows | n | top-1 | top-3 | MRR | abstain | $/task | corpus sha16 |",
           "|---|---|---|---|---|---|---|---|---|"]
    for _, prov, name, n, t1, t3, mrr, ab, cpt, csha in sorted(rows_by):
        out.append(f"| {prov} | `{name}` | {n} | {t1}/{n} | {t3}/{n} | {mrr:.3f} | {ab} | "
                   f"{analyse.usd(cpt, 4)} | {csha} |")
    return out


README_HEAD = """\
# Cortex scorecard

<!-- GENERATED by `python3 scorecard.py report` from history.jsonl and the rows
files it cites. Do not edit by hand: edit scorecard.py and regenerate. -->

Is Cortex (`cortex repair`) getting smarter? Every scored run of the repair
benchmark is one line of `history.jsonl`. Each line names the exact
configuration (Cortex commit and binary hash, model, transport, ranker,
audit, walk), the corpus (path, sha256), the per-task rows file it was
computed from (path, sha256), and the metrics `scorecard.py` recomputed from
those rows. Nothing is copied from prose: rows written by `../ab/run_ab.py`
are judged by `../ab/analyse.py`'s own functions, and `report` re-verifies
every recorded run against its files before it writes this page and
`progress.svg`.

## The strict order

A run is better than another only in this order. A later criterion counts
only when every earlier one is equal (or, for verified, not significantly
different):

1. **False successes must stay 0.** A claimed fix that the hidden check
   rejects is the failure Cortex exists to prevent. A run with any false
   success is not a candidate, whatever else it scores.
2. **Verified rate**: tasks whose final file passes the hidden check with the
   tests untouched (`verified_clean`), with a Wilson 95% interval
   (`analyse.wilson`). Two runs on the same tasks are compared with the
   two-sided exact McNemar test on discordant tasks (`analyse.mcnemar_exact`);
   a difference counts only at p < 0.05.
3. **Proposals, then cost, then wall time per verified fix.** Descriptive,
   untested. Cost is everything a task spent: generation (`cost_usd`) plus
   Cortex's own ranker and audit calls (`ranker_cost_usd`, `audit.cost_usd`;
   absent from rows older than the ranker, so 0 there). Costs marked `*`
   include an ESTIMATE (reported tokens x list price; micode on an OAuth
   grant reports no dollars).
4. **Truth first**: tasks where the first function `cortex repair` attempted
   is the true defective one (`attempted[0]` = the corpus's `symbol`). Not
   defined for the plain agent, which walks no candidates.

`python3 scorecard.py compare RUN_A RUN_B` applies this order (A = incumbent,
B = candidate) and names the first criterion that separates them;
`--json` gives the same for a program (`first_separating`).

## Sets

- **dev**: `../ab/corpus.json`, the 60-task injected-defect corpus every
  development run so far used. Tune against it freely.
- **heldout**: a corpus kept out of development. Every run on it is recorded;
  each use leaks a little of it, so the count below is part of the result.
- **once**: a candidate is scored here at most once. `add --set once` refuses
  a second run with the same (agent, cortex_sha256, model, transport, ranker,
  walk), and requires `cortex_sha256` for agent cortex.
- **field**: rows from real `cortex repair` runs.

## Adding a run

`add` reads the rows, recomputes the metrics, refuses a run_id or a rows file
(by sha256) already in the history, refuses a config the rows contradict
(model, transport, commit, binary hash, arm), pairs the run against each
`--baseline` (the first fills `baseline_run_id` / `paired`, the rest go to
`also_paired`), and appends one line. Then regenerate this page with
`python3 scorecard.py report` and commit `history.jsonl`, `README.md` and
`progress.svg` together with the rows.

A `run_ab.py` run directory with both arms (here the micode Haiku 5.5 run;
substitute your directory, commit, hashes and model):

```sh
cd crates/axon-cortex/benchmarks/scorecard
D=../ab/results-micode/full/claude-haiku-5-5
python3 scorecard.py add --set dev --agent plain --rows $D/a.jsonl \\
  --corpus ../ab/corpus.json --run-id RUN-A --date 2026-10-12 --config-json \\
  '{"model":"claude-haiku-5-5","transport":"micode","ranker":"none","audit":"off","walk":null}' \\
  --notes "what was run, PREREG file, anything unusual"
python3 scorecard.py add --set dev --agent cortex --rows $D/b.jsonl \\
  --corpus ../ab/corpus.json --run-id RUN-B --date 2026-10-12 --baseline RUN-A --config-json \\
  '{"cortex_commit":"<git sha>","cortex_sha256":"<64 or 16 hex>","model":"claude-haiku-5-5","transport":"micode","ranker":"spectrum","audit":"off","walk":"breadth","candidates":6,"budget":8}' \\
  --notes "..."
```

A cortex-only run directory (only `b.jsonl`, e.g. a ranker-on run), paired
against the cortex run it should beat and against the plain agent on the same
model:

```sh
python3 scorecard.py add --set dev --agent cortex --rows DIR/b.jsonl \\
  --corpus ../ab/corpus.json --baseline INCUMBENT-RUN-ID --baseline PLAIN-RUN-ID \\
  --config-json '{"cortex_commit":"<git sha>","cortex_sha256":"<hex>","model":"claude-haiku-5-5","transport":"micode","ranker":"openrouter:anthropic/claude-haiku-5.5","audit":"off","walk":"breadth","candidates":6,"budget":8}' \\
  --notes "..."
```

Config fields: `model`, `transport`, `ranker`, `audit`, `walk` are required;
`cortex_commit`, `cortex_sha256`, `candidates`, `budget` default to null.
For agent plain, `walk` is null and `ranker` is `"none"`. `--run-id` defaults
to `SET-DATE-AGENT-MODEL-TRANSPORT[-WALK-RANKER]`; `--date` to today.

Rows are either `run_ab.py` rows (judged by `analyse.py`) or normalized rows,
one JSON object per task with `id`, `verified`, `false_success`,
`proposals` (int or null), `cost_usd` (float or null), `wall_s`,
`truth_symbol` (must equal the corpus's `symbol`), `attempted` (list or
null), and optionally `cost_estimated`.

`python3 scorecard.py selftest` checks the statistics, the recomputation, the
refusals and the report's determinism on fixtures.
"""


TREND_STEPS = ("false successes", "verified", "proposals / fix", "cost / fix", "wall / fix",
               "truth first")


def trend_lines(runs, norms, idx):
    """Per set: the latest cortex run against the earliest cortex run on the
    same corpus and against each of its own baselines, by `judge`."""
    out = ["", "## Trend", "",
           "Per set, the latest cortex run (B) against the earliest cortex run on the same "
           "corpus and against each of its own baselines (A), judged as `compare A B` judges "
           "them, in the strict order. Values are A → B. Only the verified step is a test "
           "(exact McNemar); the later steps are descriptive.", "",
           "| set | latest (B) | against (A) | " + " | ".join(TREND_STEPS)
           + " | first criterion that separates them |",
           "|---" * (len(TREND_STEPS) + 4) + "|"]
    by_id = {r["run_id"]: r for r in runs}

    def val(rec, k, v):
        if v is None:
            return "n/a"
        if k == 1:
            return f"{v}/{rec['metrics']['n']}"
        if k == 2:
            return f"{v:.2f}"
        if k == 3:
            return analyse.usd(v, 3) + ("*" if rec["metrics"]["cost_is_estimate"] else "")
        if k == 4:
            return f"{v:.1f}s"
        if k == 5:
            return f"{v}/{rec['metrics']['n']}"
        return str(v)

    rows = 0
    for s in SETS:
        cortex = [r for r in runs if r["set"] == s and r["agent"] == "cortex"]
        if not cortex:
            continue
        latest = cortex[-1]
        against = []
        first = next(r for r in cortex if r["corpus"]["sha256"] == latest["corpus"]["sha256"])
        if first is not latest:
            against.append((first["run_id"], "earliest cortex"))
        for b, _ in pairings(latest):
            if b in [a for a, _ in against]:
                against = [(a, k + "; baseline" if a == b else k) for a, k in against]
            else:
                against.append((b, "baseline"))
        for a_id, kind in against:
            c = judge(runs, norms, a_id, latest["run_id"])
            A = by_id[a_id]
            cells = []
            for k, st in enumerate(c["steps"]):
                cell = f"{val(A, k, st['A'])} → {val(latest, k, st['B'])}"
                if k == 1:
                    cell += f" (p = {c['paired']['mcnemar_p']:.3g})"
                cells.append(cell)
            k = next((k for k, st in enumerate(c["steps"]) if st["better"]), None)
            sep = "none" if k is None else (
                f"{TREND_STEPS[k]}: {'B (latest)' if c['steps'][k]['better'] == 'B' else 'A'}"
                + ("" if c["steps"][k]["tested"] else ", untested"))
            out.append(f"| {s} | {idx[latest['run_id']]}. `{latest['run_id']}` | "
                       f"{idx[a_id]}. `{a_id}` ({kind}) | " + " | ".join(cells) + f" | {sep} |")
            rows += 1
    if not rows:
        out.append("| (no cortex run has anything to compare against yet) |"
                   + " |" * (len(TREND_STEPS) + 3))
    return out


def render_readme(recs, root, locdir, norms):
    runs = ordered(recs)
    idx = {r["run_id"]: i + 1 for i, r in enumerate(runs)}
    out = [README_HEAD, "## Runs", "",
           "`#` is the run's position on the x axis of `progress.svg`. `*` = cost estimated.", "",
           "| # | run | date | set | agent | model / transport | ranker / audit / walk | "
           "verified (95% CI) | false successes | proposals / fix | cost / fix | wall / fix "
           "| truth first |",
           "|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for r in runs:
        m, c = r["metrics"], r["config"]
        lo, hi = m["verified_ci95"]
        tf = "n/a" if m["truth_first"] is None else f"{m['truth_first']}/{m['n']}"
        ppv = "n/a" if m["proposals_per_verified"] is None else f"{m['proposals_per_verified']:.2f}"
        wpv = "n/a" if m["wall_s_per_verified"] is None else f"{m['wall_s_per_verified']:.1f}s"
        out.append(f"| {idx[r['run_id']]} | `{r['run_id']}` | {r['date']} | {r['set']} | "
                   f"{r['agent']} | {c['model']} / {c['transport']} | "
                   f"{c['ranker']} / {c['audit']} / {c['walk'] or '-'} | "
                   f"{m['verified']}/{m['n']} ({pct(m['verified_rate'])}, {pct(lo)}-{pct(hi)}) | "
                   f"{m['false_success']} | {ppv} | {money(r, 'cost_per_verified')} | {wpv} | {tf} |")
    out += ["", "Per run: configuration hashes, corpus and notes.", ""]
    for r in runs:
        c = r["config"]
        bits = [f"rows `{r['rows']}`", f"corpus `{r['corpus']['path']}` "
                f"(sha256 {r['corpus']['sha256'][:16]}, n={r['corpus']['n']})"]
        if c["cortex_commit"]:
            bits.append(f"cortex commit {c['cortex_commit'][:12]}")
        if c["cortex_sha256"]:
            bits.append(f"cortex sha256 {c['cortex_sha256'][:16]}")
        if c["candidates"] is not None:
            bits.append(f"candidates {c['candidates']}")
        if c["budget"] is not None:
            bits.append(f"budget {c['budget']}")
        if r["metrics"]["cost_usd_total"] is not None:
            bits.append(f"total cost {money(r, 'cost_usd_total')}")
        oos = r["metrics"].get("out_of_scope")
        if oos:
            bits.append(f"out of scope (not in verified): {oos['n']} tasks, {oos['fixed']} fixed, "
                        f"{oos['honest_failure']} honest failures (no claim, file unchanged), "
                        f"{oos['no_claim']} not claimed, {oos['false_success']} false successes")
        out.append(f"- **{idx[r['run_id']]}. `{r['run_id']}`**: {'; '.join(bits)}. {r['notes']}")

    out += ["", "## Which differences are significant", "",
            f"Paired on the tasks both runs scored. Significant = two-sided exact McNemar "
            f"p < {ALPHA} on the discordant tasks. Each line is its own test; there is no "
            "correction for the number of comparisons, so treat a lone p near 0.05 as a "
            "lead, not a finding. Only the verified outcome is tested.", "",
            "| run | baseline | paired n | both | only run | only baseline | neither | "
            "exact McNemar p | significant |",
            "|---|---|---|---|---|---|---|---|---|"]
    any_pair = False
    for r in runs:
        for b, p in pairings(r):
            any_pair = True
            n = p["both"] + p["only_this"] + p["only_baseline"] + p["neither"]
            sig = p["mcnemar_p"] < ALPHA
            out.append(f"| {idx[r['run_id']]}. `{r['run_id']}` | {idx[b]}. `{b}` | {n} | "
                       f"{p['both']} | {p['only_this']} | {p['only_baseline']} | {p['neither']} | "
                       f"{p['mcnemar_p']:.3g} | {'**yes**' if sig else 'no'} |")
    if not any_pair:
        out.append("| (no paired runs yet) | | | | | | | | |")

    out += trend_lines(runs, norms, idx)

    out += ["", "## Held-out and once-set use", ""]
    held = [r for r in runs if r["set"] == "heldout"]
    if not held:
        out.append("Held-out set: used 0 times.")
    for sha in sorted({r["corpus"]["sha256"] for r in held}):
        rs = [r for r in held if r["corpus"]["sha256"] == sha]
        out.append(f"Held-out corpus `{rs[0]['corpus']['path']}` (sha256 {sha[:16]}): used "
                   f"{len(rs)} time{'s' if len(rs) != 1 else ''}: "
                   + ", ".join(f"`{r['run_id']}`" for r in rs) + ".")
    out.append("")
    for s, name in (("once", "Once-set"), ("hard-once", "Hard once-set")):
        once = [r for r in runs if r["set"] == s]
        out.append(f"{name}: {len(once)} candidate{'s' if len(once) != 1 else ''} scored"
                   + (": " + ", ".join(f"`{r['run_id']}`" for r in once) if once else "") + ".")

    out += ["", "## Localization only (LocRank, not runs)", "",
            "LocRank replays localization without repairing, so it has no verified or "
            "false-success outcome and is not a scorecard run.", ""]
    out += locrank_table(locdir, root)
    out += ["", "## Progress plot", "",
            "![progress](progress.svg)", "",
            "x = runs in date order (the `#` above). Panels: verified rate with 95% CI, "
            "false successes, proposals per verified fix (cortex only), cost per verified fix "
            "(log scale; hollow = estimate), truth-first rate. Colour = agent (blue cortex, "
            "orange plain); marker = set "
            "(circle dev, square heldout, diamond once, triangle field, inverted triangle "
            "hard-dev, hexagon hard-once).", ""]
    return "\n".join(out)


COLORS = {"cortex": "#1f6fb4", "plain": "#d9822b"}


def marker(set_, x, y, color, hollow=False, title=None):
    fill = "#ffffff" if hollow else color
    st = f'fill="{fill}" stroke="{color}" stroke-width="1.5"'
    t = f"<title>{escape(title)}</title>" if title else ""
    if set_ == "dev":
        return f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4.5" {st}>{t}</circle>'
    if set_ == "heldout":
        return f'<rect x="{x - 4.5:.1f}" y="{y - 4.5:.1f}" width="9" height="9" {st}>{t}</rect>'
    if set_ == "once":
        pts = f"{x:.1f},{y - 6:.1f} {x + 6:.1f},{y:.1f} {x:.1f},{y + 6:.1f} {x - 6:.1f},{y:.1f}"
    elif set_ == "hard-dev":
        pts = f"{x:.1f},{y + 5.5:.1f} {x + 5.5:.1f},{y - 4.5:.1f} {x - 5.5:.1f},{y - 4.5:.1f}"
    elif set_ == "hard-once":
        pts = " ".join(f"{x + 5.5 * math.cos(math.radians(a)):.1f},"
                       f"{y + 5.5 * math.sin(math.radians(a)):.1f}" for a in range(0, 360, 60))
    else:
        pts = f"{x:.1f},{y - 5.5:.1f} {x + 5.5:.1f},{y + 4.5:.1f} {x - 5.5:.1f},{y + 4.5:.1f}"
    return f'<polygon points="{pts}" {st}>{t}</polygon>'


def render_svg(recs):
    runs = ordered(recs)
    n = max(len(runs), 1)
    labels = [label(i, r) for i, r in enumerate(runs, 1)]
    NP, PW, R, TOP, PH, GAP = 5, 860, 20, 70, 150, 46
    step = PW / n
    # The x labels hang down-left from their tick at -35 degrees; size the left
    # and bottom margins so the longest one is never cut off (6.4 px per
    # character over-estimates 11 px sans-serif).
    widths = [6.4 * len(s) for s in labels]
    cos35, sin35 = math.cos(math.radians(35)), math.sin(math.radians(35))
    L = max([80] + [math.ceil(8 + w * cos35 - (i + 0.5) * step) for i, w in enumerate(widths)])
    BOTTOM = math.ceil(max([0.0] + [w * sin35 for w in widths]) + 40)
    W = L + PW + R
    H = TOP + NP * PH + (NP - 1) * GAP + BOTTOM
    pw = PW
    xs = [L + (i + 0.5) * step for i in range(len(runs))]
    o = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
         f'viewBox="0 0 {W} {H}" font-family="sans-serif" font-size="11">',
         f'<rect x="0" y="0" width="{W}" height="{H}" fill="#ffffff"/>',
         f'<text x="{L}" y="22" font-size="15" font-weight="bold">Cortex scorecard: '
         f'{len(runs)} run{"s" if len(runs) != 1 else ""}</text>']
    # Legend.
    lx = L
    for agent in AGENTS:
        o.append(f'<rect x="{lx}" y="36" width="10" height="10" fill="{COLORS[agent]}"/>')
        o.append(f'<text x="{lx + 14}" y="45">{"cortex" if agent == "cortex" else "plain agent"}</text>')
        lx += 100
    for s in SETS:
        o.append(marker(s, lx + 5, 41, "#555555"))
        o.append(f'<text x="{lx + 14}" y="45">{s}</text>')
        lx += 80
    o.append(marker("dev", lx + 5, 41, "#555555", hollow=True))
    o.append(f'<text x="{lx + 14}" y="45">cost estimated</text>')

    def panel(k, title, ticks, ymap, tickfmt):
        y0 = TOP + k * (PH + GAP)
        o.append(f'<text x="{L}" y="{y0 - 8}" font-weight="bold">{escape(title)}</text>')
        o.append(f'<rect x="{L}" y="{y0}" width="{pw}" height="{PH}" fill="none" stroke="#999999"/>')
        for t in ticks:
            y = y0 + ymap(t)
            o.append(f'<line x1="{L}" y1="{y:.1f}" x2="{L + pw}" y2="{y:.1f}" stroke="#e3e3e3"/>')
            o.append(f'<text x="{L - 6}" y="{y + 4:.1f}" text-anchor="end">{escape(tickfmt(t))}</text>')
        for x in xs:
            o.append(f'<line x1="{x:.1f}" y1="{y0 + PH}" x2="{x:.1f}" y2="{y0 + PH + 4}" stroke="#999999"/>')
        return y0

    def na(x, y0):
        o.append(f'<text x="{x:.1f}" y="{y0 + PH - 6}" text-anchor="middle" fill="#888888">n/a</text>')

    # 1. Verified rate with 95% CI.
    los = [r["metrics"]["verified_ci95"][0] for r in runs] or [0.5]
    lo_axis = max(0.0, math.floor(min(los) * 10) / 10)
    span = 1.0 - lo_axis or 1.0
    ymap = lambda v: PH - (v - lo_axis) / span * PH
    tstep = 0.05 if span <= 0.3 else 0.1
    ticks = [round(lo_axis + i * tstep, 2) for i in range(int(round(span / tstep)) + 1)]
    y0 = panel(0, "Verified rate (95% Wilson CI)", ticks, ymap, lambda t: f"{100 * t:.0f}%")
    for x, r in zip(xs, runs):
        m, col = r["metrics"], COLORS[r["agent"]]
        lo, hi = m["verified_ci95"]
        ya, yb = y0 + ymap(lo), y0 + ymap(hi)
        o.append(f'<line x1="{x:.1f}" y1="{ya:.1f}" x2="{x:.1f}" y2="{yb:.1f}" stroke="{col}" stroke-width="1.5"/>')
        for yy in (ya, yb):
            o.append(f'<line x1="{x - 5:.1f}" y1="{yy:.1f}" x2="{x + 5:.1f}" y2="{yy:.1f}" stroke="{col}" stroke-width="1.5"/>')
        o.append(marker(r["set"], x, y0 + ymap(m["verified_rate"]), col,
                        title=f"{r['run_id']}: {m['verified']}/{m['n']} verified, "
                              f"CI {100 * lo:.1f}-{100 * hi:.1f}%"))

    # 2. False successes.
    top = max([r["metrics"]["false_success"] for r in runs] + [1])
    ymap = lambda v: PH - v / top * (PH - 12) - 6
    fticks = sorted({0, top} | set(range(0, top + 1, max(1, top // 4))))
    y0 = panel(1, "False successes (must stay 0)", fticks, ymap, str)
    for x, r in zip(xs, runs):
        fs = r["metrics"]["false_success"]
        col = COLORS[r["agent"]]
        y = y0 + ymap(fs)
        o.append(marker(r["set"], x, y, col, title=f"{r['run_id']}: {fs} false successes"))
        o.append(f'<text x="{x + 8:.1f}" y="{y + 4:.1f}" fill="{"#c0392b" if fs else "#333333"}">{fs}</text>')

    # 3. Proposals per verified fix: cortex only, the plain agent proposes no
    # candidate patches.
    ppv = [r["metrics"]["proposals_per_verified"] for r in runs
           if r["metrics"]["proposals_per_verified"] is not None]
    pmax = max(ppv + [1.0])
    pstep = next(s for s in (m * 10 ** e for e in range(12) for m in (1, 2, 5)) if pmax / s <= 5)
    ptop = pstep * math.ceil(pmax / pstep)
    ymap = lambda v: PH - v / ptop * PH
    y0 = panel(2, "Proposals per verified fix (cortex only)",
               [pstep * i for i in range(int(ptop / pstep) + 1)], ymap, str)
    for x, r in zip(xs, runs):
        v = r["metrics"]["proposals_per_verified"]
        if v is None:
            na(x, y0)
            continue
        col = COLORS[r["agent"]]
        y = y0 + ymap(v)
        o.append(marker(r["set"], x, y, col, title=f"{r['run_id']}: {v:.2f} proposals per verified fix"))
        o.append(f'<text x="{x + 8:.1f}" y="{y + 4:.1f}" fill="#333333">{v:.2f}</text>')

    # 4. Cost per verified fix, log scale.
    cpv = [r["metrics"]["cost_per_verified"] for r in runs if r["metrics"]["cost_per_verified"]]
    lo_d = math.floor(math.log10(min(cpv))) if cpv else -3
    hi_d = math.ceil(math.log10(max(cpv))) if cpv else 0
    if hi_d == lo_d:
        hi_d += 1
    ymap = lambda v: PH - (math.log10(v) - lo_d) / (hi_d - lo_d) * PH
    y0 = panel(3, "Cost per verified fix, USD (log scale; hollow = estimated)",
               [10.0 ** d for d in range(lo_d, hi_d + 1)], ymap,
               lambda t: f"${t:.{max(0, -int(round(math.log10(t))))}f}")
    for x, r in zip(xs, runs):
        m, col = r["metrics"], COLORS[r["agent"]]
        v = m["cost_per_verified"]
        if not v:
            na(x, y0)
            continue
        o.append(marker(r["set"], x, y0 + ymap(v), col, hollow=m["cost_is_estimate"],
                        title=f"{r['run_id']}: {analyse.usd(v, 3)} per verified fix"
                              + (" (estimated)" if m["cost_is_estimate"] else "")))

    # 5. Truth-first rate.
    ymap = lambda v: PH - v * PH
    y0 = panel(4, "Truth first (first attempted function is the defective one)",
               [0.0, 0.25, 0.5, 0.75, 1.0], ymap, lambda t: f"{100 * t:.0f}%")
    for x, r in zip(xs, runs):
        m, col = r["metrics"], COLORS[r["agent"]]
        if m["truth_first"] is None:
            na(x, y0)
            continue
        o.append(marker(r["set"], x, y0 + ymap(m["truth_first"] / m["n"]), col,
                        title=f"{r['run_id']}: truth first {m['truth_first']}/{m['n']}"))

    # X labels under the last panel.
    ybase = TOP + NP * PH + (NP - 1) * GAP + 14
    for x, r, text in zip(xs, runs, labels):
        o.append(f'<text x="{x:.1f}" y="{ybase}" text-anchor="end" '
                 f'transform="rotate(-35 {x:.1f} {ybase})">{escape(text)}</text>')
    o.append("</svg>")
    return "\n".join(o) + "\n"


def render(history, root, locdir):
    recs = load_history(history)
    norms = verify(recs, root)
    readme = render_readme(recs, root, locdir, norms)
    svg = render_svg(recs)
    ET.fromstring(svg)  # must be well-formed XML
    return readme, svg


def report(history, root, locdir, check=False):
    readme, svg = render(history, root, locdir)
    d = os.path.dirname(os.path.abspath(history))
    stale = []
    for name, text in (("README.md", readme), ("progress.svg", svg)):
        path = os.path.join(d, name)
        old = open(path).read() if os.path.exists(path) else None
        if old != text:
            stale.append(name)
            if not check:
                with open(path, "w") as fh:
                    fh.write(text)
    return stale


# ---------------------------------------------------------------- selftest

def selftest():
    fails = []

    def expect(cond, what):
        if not cond:
            fails.append(what)

    def refused(fn, needle):
        try:
            fn()
        except Refused as e:
            return needle in str(e)
        return False

    # Statistics: the Wilson interval the A/B README prints for 54/60, and
    # exact McNemar against brute-force enumeration of the binomial.
    lo, hi = analyse.wilson(54, 60)
    expect((round(100 * lo, 1), round(100 * hi, 1)) == (79.9, 95.3), f"wilson 54/60 = {lo}, {hi}")
    for b, c in ((5, 4), (0, 5), (1, 4), (0, 6), (3, 12), (0, 0)):
        n = b + c
        tail = sum(math.comb(n, i) for i in range(n + 1) if math.comb(n, i) <= math.comb(n, min(b, c)))
        brute = 1.0 if n == 0 else min(1.0, tail / 2 ** n)
        expect(math.isclose(analyse.mcnemar_exact(b, c), brute), f"mcnemar({b},{c})")
    expect(analyse.mcnemar_exact(0, 5) == 0.0625, "mcnemar(0,5) = 0.0625")
    expect(analyse.mcnemar_exact(0, 6) < ALPHA < analyse.mcnemar_exact(0, 5),
           "6 vs 0 discordant is the smallest significant split")

    with tempfile.TemporaryDirectory() as root:
        corpus = os.path.join(root, "corpus.json")
        ids = [f"t{i}" for i in range(6)]
        with open(corpus, "w") as fh:
            json.dump({"meta": {}, "tasks": [{"id": i, "symbol": "f_" + i} for i in ids]}, fh)
        hist = os.path.join(root, "history.jsonl")

        def ab_row(i, arm, verified, fs=False, attempted=None, props=None, cost=0.5,
                   est=False, model="m1", transport="micode", wall=10.0):
            r = {"id": i, "arm": arm, "verified_clean": verified, "false_success": fs,
                 "claim": verified or fs, "tests_tampered": [], "modified": True,
                 "final_compiles": True, "unkeyed_all_ok": verified, "wall_s": wall,
                 "cost_usd": cost,
                 "meta": {"arm": arm, "model": model, "transport": transport, "commit": "c" * 40,
                          "cortex_sha256_16": "ab" * 8}}
            if est:
                r["cost_estimated"] = True
            if arm == "B":
                # Cortex's own ranker call: billed apart from generation.
                r["attempted"], r["proposals"] = attempted, props
                r["ranker"] = {"requested": "openrouter:r/k", "cost_usd": 0.01}
                r["ranker_cost_usd"] = 0.01
                r["audit"] = {"requested": "off", "cost_usd": 0.0}
            return r

        def write_rows(name, rows):
            p = os.path.join(root, name)
            with open(p, "w") as fh:
                fh.writelines(json.dumps(r) + "\n" for r in rows)
            return p

        # Plain agent: 4/6 verified, 1 false success, one row with no cost.
        a_rows = [ab_row("t0", "A", True), ab_row("t1", "A", True), ab_row("t2", "A", True),
                  ab_row("t3", "A", True, cost=None), ab_row("t4", "A", False, fs=True),
                  ab_row("t5", "A", False)]
        # Cortex: 5/6 verified, truth first on t0, t1, t4 only.
        b_rows = [ab_row(i, "B", v, attempted=att, props=p, cost=0.1, est=True, wall=20.0)
                  for i, v, att, p in (("t0", True, ["f_t0"], 1), ("t1", True, ["f_t1", "x"], 2),
                                       ("t2", True, ["x", "f_t2"], 3), ("t3", False, ["x"], 8),
                                       ("t4", True, ["f_t4"], 1), ("t5", True, [], 0))]
        pa, pb = write_rows("a.jsonl", a_rows), write_rows("b.jsonl", b_rows)
        cfg_a = {"model": "m1", "transport": "micode", "ranker": "none", "audit": "off", "walk": None}
        cfg_b = {"model": "m1", "transport": "micode", "ranker": "openrouter:r/k", "audit": "off",
                 "walk": "breadth", "cortex_sha256": "ab" * 8, "cortex_commit": "c" * 40}
        ra = add(hist, root, set_="dev", agent="plain", rows=pa, corpus=corpus, config=cfg_a,
                 notes="fixture A", run_id="A", date="2026-01-01")
        ma = ra["metrics"]
        expect((ma["verified"], ma["false_success"], ma["n"]) == (4, 1, 6), f"A counts {ma}")
        expect(ma["cost_usd_total"] == 2.5 and ma["cost_per_verified"] == 0.625, f"A cost {ma}")
        expect(ma["proposals_per_verified"] is None and ma["truth_first"] is None,
               "plain agent has no proposals / truth_first")
        expect(ma["cost_is_estimate"] is False and ma["wall_s_per_verified"] == 15.0, f"A {ma}")
        rb = add(hist, root, set_="dev", agent="cortex", rows=pb, corpus=corpus, config=cfg_b,
                 notes="fixture B", baselines=["A"], run_id="B", date="2026-01-02")
        mb = rb["metrics"]
        expect((mb["verified"], mb["false_success"]) == (5, 0), f"B counts {mb}")
        expect(mb["proposals_per_verified"] == 3.0, f"B proposals/fix {mb}")
        expect(mb["truth_first"] == 3, f"B truth_first {mb['truth_first']}")
        expect(mb["cost_is_estimate"] is True and mb["cost_per_verified"] == 0.132,
               f"B cost (generation + ranker) {mb}")
        expect(rb["paired"] == {"both": 3, "only_this": 2, "only_baseline": 1, "neither": 0,
                                "mcnemar_p": 1.0}, f"B paired {rb['paired']}")

        # Duplicates: same run_id; same rows under another id.
        expect(refused(lambda: add(hist, root, set_="dev", agent="plain", rows=pa, corpus=corpus,
                                   config=cfg_a, notes="x", run_id="A"), "already in the history"),
               "duplicate run_id not refused")
        expect(refused(lambda: add(hist, root, set_="dev", agent="plain", rows=pa, corpus=corpus,
                                   config=cfg_a, notes="x", run_id="A2"), "already scored"),
               "duplicate rows not refused")
        # The rows contradict the config: model, arm.
        expect(refused(lambda: add(hist, root, set_="dev", agent="plain",
                                   rows=write_rows("a2.jsonl", a_rows[:3]), corpus=corpus,
                                   config=dict(cfg_a, model="m2"), notes="x"), "rows ran"),
               "model mismatch not refused")
        expect(refused(lambda: add(hist, root, set_="dev", agent="cortex",
                                   rows=write_rows("a3.jsonl", a_rows[:2]), corpus=corpus,
                                   config=cfg_b, notes="x"), "arm"), "arm mismatch not refused")
        expect(refused(lambda: add(hist, root, set_="dev", agent="cortex", rows=pb, corpus=corpus,
                                   config=dict(cfg_b, ranker="spectrum"), notes="x"),
                       "rows requested"), "ranker mismatch not refused")
        expect(refused(lambda: add(hist, root, set_="dev", agent="plain",
                                   rows=write_rows("bad.jsonl", [{"id": "t0", "ok": True}]),
                                   corpus=corpus, config=cfg_a, notes="x"), "neither"),
               "unknown row format not refused")
        expect(refused(lambda: add(hist, root, set_="dev", agent="plain",
                                   rows=write_rows("alien.jsonl", [ab_row("zz", "A", True)]),
                                   corpus=corpus, config=cfg_a, notes="x"), "not in the corpus"),
               "task id outside the corpus not refused")
        expect(refused(lambda: add(hist, root, set_="dev", agent="plain", rows=pa, corpus=corpus,
                                   config=dict(cfg_a, rnaker="x"), notes="x"), "unknown config"),
               "config typo not refused")

        # Once-set: the same candidate twice is refused; another ranker is not.
        def norm_rows(name, ranker_rows):
            return write_rows(name, [{"id": r["id"], "verified": r["verified_clean"],
                                      "false_success": False, "proposals": r["proposals"],
                                      "cost_usd": None, "wall_s": 1.0,
                                      "truth_symbol": "f_" + r["id"], "attempted": r["attempted"]}
                                     for r in ranker_rows])
        once_cfg = dict(cfg_b, ranker="openrouter:x/y")
        add(hist, root, set_="once", agent="cortex", rows=norm_rows("o1.jsonl", b_rows),
            corpus=corpus, config=once_cfg, notes="once 1", run_id="O1", date="2026-01-03")
        expect(refused(lambda: add(hist, root, set_="once", agent="cortex",
                                   rows=norm_rows("o2.jsonl", b_rows[:5]), corpus=corpus,
                                   config=once_cfg, notes="once 2", run_id="O2"), "at most once"),
               "second once-set run of one candidate not refused")
        add(hist, root, set_="once", agent="cortex", rows=norm_rows("o3.jsonl", b_rows[:4]),
            corpus=corpus, config=dict(once_cfg, ranker="spectrum"), notes="once 3",
            run_id="O3", date="2026-01-03")
        expect(refused(lambda: add(hist, root, set_="once", agent="cortex",
                                   rows=norm_rows("o4.jsonl", b_rows[:3]), corpus=corpus,
                                   config=dict(once_cfg, cortex_sha256=None), notes="x"),
                       "needs config cortex_sha256"), "once-set without cortex_sha256 not refused")
        # Heldout uses are counted.
        for k in (1, 2):
            add(hist, root, set_="heldout", agent="cortex", rows=norm_rows(f"h{k}.jsonl", b_rows[1:k + 2]),
                corpus=corpus, config=dict(cfg_b, ranker=f"r{k}"), notes=f"heldout {k}",
                run_id=f"H{k}", date="2026-01-04")
        # Out-of-scope tasks (../hardset): never in verified/n, summarized apart;
        # a false claim on one still counts as a false success.
        corpus_h = os.path.join(root, "hard.json")
        with open(corpus_h, "w") as fh:
            json.dump({"meta": {}, "tasks": [{"id": "i0", "symbol": "f"}, {"id": "i1", "symbol": "g"},
                                             {"id": "x0", "symbol": "h", "scope": "out"},
                                             {"id": "x1", "symbol": "k", "scope": "out"}]}, fh)
        h_rows = [ab_row("i0", "B", True, attempted=["f"], props=1),
                  ab_row("i1", "B", False, attempted=["g"], props=2),
                  dict(ab_row("x0", "B", False, attempted=["h"], props=3), claim=False,
                       modified=False),
                  ab_row("x1", "B", False, fs=True, attempted=["h"], props=1)]
        rh = add(hist, root, set_="hard-dev", agent="cortex", rows=write_rows("hd.jsonl", h_rows),
                 corpus=corpus_h, config=cfg_b, notes="hard-dev", run_id="HD", date="2026-01-05")
        mh = rh["metrics"]
        expect((mh["verified"], mh["n"], mh["false_success"], mh["proposals_per_verified"])
               == (1, 2, 1, 3.0), f"hard-dev in-scope counts {mh}")
        expect(mh["out_of_scope"] == {"n": 2, "fixed": 0, "false_success": 1, "no_claim": 1,
                                      "honest_failure": 1, "proposals": 4,
                                      "cost_usd_total": 1.02, "wall_s": 20.0},
               f"hard-dev out-of-scope summary {mh.get('out_of_scope')}")
        x0_norm = {"id": "x0", "verified": False, "false_success": False, "proposals": 1,
                   "cost_usd": None, "wall_s": 1.0, "truth_symbol": "h", "attempted": ["h"]}
        expect(refused(lambda: add(hist, root, set_="hard-dev", agent="cortex",
                                   rows=write_rows("hn.jsonl", [x0_norm]), corpus=corpus_h,
                                   config=cfg_b, notes="x"), "out-of-scope"),
               "normalized row for an out-of-scope task not refused")

        stale1 = report(hist, root, None)
        texts = [open(os.path.join(root, f)).read() for f in ("README.md", "progress.svg")]
        stale2 = report(hist, root, None)
        texts2 = [open(os.path.join(root, f)).read() for f in ("README.md", "progress.svg")]
        expect(stale1 == ["README.md", "progress.svg"] and stale2 == [] and texts == texts2,
               "report is not byte-identical on a second run")
        expect("used 2 times" in texts[0], "heldout use count missing from README")
        expect("Once-set: 2 candidates scored" in texts[0], "once-set count missing")
        expect("| dev | 2. `B` | 1. `A` (baseline) | 1 → 0 |" in texts[0]
               and "false successes: B (latest), untested |" in texts[0],
               "trend: dev latest cortex vs its baseline missing or misjudged")
        expect("| heldout | 6. `H2` | 5. `H1` (earliest cortex) |" in texts[0]
               and "| once | 4. `O3` | 3. `O1` (earliest cortex) |" in texts[0],
               "trend: latest vs earliest cortex run missing")
        expect("Proposals per verified fix (cortex only)" in texts[1], "SVG proposals panel missing")
        cos35 = math.cos(math.radians(35))
        for m_ in re.finditer(r'<text x="([\d.]+)" y="[\d.]+" text-anchor="end" '
                              r'transform="rotate\(-35[^"]*">([^<]*)</text>', texts[1]):
            expect(float(m_.group(1)) - 6.4 * len(m_.group(2)) * cos35 >= 0,
                   f"x label clipped on the left: {m_.group(2)}")
        try:
            ET.fromstring(texts[1])
        except ET.ParseError as e:
            fails.append(f"progress.svg is not well-formed: {e}")

        c = compare(load_history(hist), root, "A", "B")
        expect(c["first_separating"] == {"criterion": "false successes (must stay 0)",
                                         "better": "B", "tested": False},
               f"compare strict order {c['first_separating']}")

        # A rows file edited after `add` is caught by report.
        with open(pa, "a") as fh:
            fh.write(json.dumps(ab_row("t0", "A", True)) + "\n")
        expect(refused(lambda: report(hist, root, None), "changed since it was added"),
               "edited rows file not caught by report")

    # Real data: the primary's rows give the README's 54/60, 0 false successes.
    real = os.path.join(BENCH, "ab", "results", "full", "b.jsonl")
    if os.path.exists(real):
        truth = load_corpus(os.path.join(BENCH, "ab", "corpus.json"))
        raw, norm = load_rows(real, truth)
        crosscheck_analyse(raw, norm, "cortex")
        m = compute_metrics(norm, "cortex")
        expect((m["verified"], m["false_success"], m["n"]) == (54, 0, 60), f"primary B {m}")

    if fails:
        for f in fails:
            print("FAIL " + f, file=sys.stderr)
        return 1
    print("scorecard self-test: pass (wilson, exact McNemar, recomputation, analyse cross-check, "
          "refusals: duplicate run_id / rows / config / arm / format / corpus / once-set, "
          "heldout count, report determinism, SVG well-formed, compare order, stale rows)")
    return 0


# ---------------------------------------------------------------- CLI

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--history", default=HISTORY, help=argparse.SUPPRESS)
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("add", help="score a rows file and append it to the history")
    a.add_argument("--set", required=True, choices=SETS)
    a.add_argument("--agent", required=True, choices=AGENTS)
    a.add_argument("--rows", required=True)
    a.add_argument("--corpus", required=True)
    a.add_argument("--config-json", required=True)
    a.add_argument("--baseline", action="append", default=[])
    a.add_argument("--run-id")
    a.add_argument("--date")
    a.add_argument("--notes", required=True)
    r = sub.add_parser("report", help="regenerate README.md and progress.svg")
    r.add_argument("--check", action="store_true", help="write nothing; exit 1 if stale")
    c = sub.add_parser("compare", help="two runs in the strict order (A incumbent, B candidate)")
    c.add_argument("run_a")
    c.add_argument("run_b")
    c.add_argument("--json", action="store_true")
    sub.add_parser("selftest", help="fixtures for the statistics, refusals and report")
    args = ap.parse_args(argv)
    try:
        if args.cmd == "add":
            try:
                cfg = json.loads(args.config_json)
            except json.JSONDecodeError as e:
                raise Refused(f"--config-json is not JSON: {e}")
            rec = add(args.history, ROOT, set_=args.set, agent=args.agent, rows=args.rows,
                      corpus=args.corpus, config=cfg, notes=args.notes,
                      baselines=args.baseline, run_id=args.run_id, date=args.date)
            m = rec["metrics"]
            print(f"added {rec['run_id']}: {m['verified']}/{m['n']} verified, "
                  f"{m['false_success']} false successes"
                  + "".join(f"; vs {b}: McNemar p = {p['mcnemar_p']:.3g}" for b, p in pairings(rec)))
            print("now run: python3 scorecard.py report")
        elif args.cmd == "report":
            stale = report(args.history, ROOT, LOCRANK, check=args.check)
            if args.check:
                if stale:
                    print(f"stale: {', '.join(stale)}; run python3 scorecard.py report",
                          file=sys.stderr)
                    return 1
                print("README.md and progress.svg are up to date")
            else:
                print("wrote " + (", ".join(stale) if stale else "nothing (already up to date)"))
        elif args.cmd == "compare":
            cmp = compare(load_history(args.history), ROOT, args.run_a, args.run_b)
            if args.json:
                print(json.dumps(cmp, indent=1))
            else:
                print_compare(cmp)
        else:
            return selftest()
    except Refused as e:
        print(f"refused: {e}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
