#!/usr/bin/env python3
"""Can a cheap model reorder `cortex locate`'s candidates better than the spectrum?

    python3 locrank.py locate                      # spectrum lists for all 60 tasks ($0)
    python3 locrank.py selftest                    # scripted oracle/adversary/reverser ($0)
    python3 locrank.py run --provider spectrum
    python3 locrank.py run --provider claude-cli:claude-haiku-4-5-20251001 --parallel 3
    python3 locrank.py run --provider openrouter:qwen/qwen3-coder-next --parallel 3
    python3 locrank.py snapshot-armb PATH/TO/b.jsonl   # freeze arm-B rows for the projection
    python3 locrank.py summary                     # metrics + projection -> results/summary.{json,md}

The ranker plays the `tiny-router` role of crates/axon-reflex/src/shortlist.rs:
it may only REORDER the eligible set the spectrum produced (its top 6). Its
output is parsed strictly; an unknown id, a duplicate, an empty list or text
that is not the one JSON shape asked for is an ABSTAIN, and the spectrum order
is kept. Ids it omits keep their spectrum order after the ids it named (the
shortlist contract allows an ordered subset; it never allows an addition).

The ranker sees only what cortex has at that point: the mutant file WITHOUT the
hidden check, the visible failing check's name and body, and the candidate
names. Never the hidden check, the mutation metadata (`how`, `class`) or the
true symbol.
"""
import argparse
import concurrent.futures as cf
import glob
import hashlib
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(BENCH, "ab"))
sys.path.insert(0, BENCH)
from common import AXON, ROOT  # noqa: E402
from corpus import load as load_corpus, test_blocks  # noqa: E402

CORTEX = os.environ.get("CORTEX_BIN", f"{ROOT}/target/debug/cortex")
CLAUDE = os.environ.get("CLAUDE_BIN", "/root/.local/bin/claude")
CORPUS = os.path.join(BENCH, "ab", "corpus.json")
RESULTS = os.path.join(HERE, "results")
LOCATE = os.path.join(RESULTS, "locate.jsonl")
ARMB = os.path.join(RESULTS, "armb_snapshot.jsonl")
ELIGIBLE = 6  # arm B runs `cortex repair --candidates 6`
OPENROUTER_URL = "https://openrouter.ai/api/v1/chat/completions"
# What one paid call in flight may still spend, reserved before it starts.
CALL_RESERVE_USD = 0.05
# OpenRouter 429/5xx: at most this many attempts, backoff 5, 10, 20, 40, 60, 60 s.
OR_MAX_ATTEMPTS = 7
OR_BACKOFF_S = 5
OR_BACKOFF_CAP_S = 60


def sha16(s):
    return hashlib.sha256(s.encode() if isinstance(s, str) else s).hexdigest()[:16]


def file_sha16(p):
    return sha16(open(p, "rb").read())


def slug(provider):
    return re.sub(r"[^A-Za-z0-9._-]+", "_", provider)


def read_jsonl(p):
    return [json.loads(l) for l in open(p) if l.strip()] if os.path.exists(p) else []


# ---------------------------------------------------------------- spectrum

def run_locate(task):
    """`cortex locate` exactly as run_ab.py arm B calls it, on the same bytes."""
    ws = tempfile.mkdtemp(prefix="locrank_")
    try:
        open(os.path.join(ws, task["file"]), "w").write(task["broken"])
        began = time.time()
        p = subprocess.run([CORTEX, "locate", "--workspace", ws, "--file", task["file"],
                            "--check", task["hidden"], "--axon", AXON, "--json"],
                           capture_output=True, text=True, timeout=300)
        took = time.time() - began
    finally:
        shutil.rmtree(ws, ignore_errors=True)
    v = None
    for line in reversed(p.stdout.strip().splitlines()):
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if isinstance(v, dict) and v.get("schema") == "cortex-locate/1":
            break
        v = None
    if v is None:
        raise SystemExit(f"{task['id']}: cortex locate gave no cortex-locate/1 line "
                         f"(exit {p.returncode}): {p.stderr[-400:]}")
    ranked = [(r["symbol"], r["score"]) for r in v.get("ranked", [])]
    names = [s for s, _ in ranked]
    truth = task["symbol"]
    rank = names.index(truth) + 1 if truth in names else 0
    top = ranked[0][1] if ranked else None
    tscore = ranked[rank - 1][1] if rank else None
    return {
        "id": task["id"], "cls": task["class"], "truth": truth,
        "ranked": [{"symbol": s, "score": sc} for s, sc in ranked],
        "eligible": names[:ELIGIBLE],
        "spectrum_rank": rank, "in_eligible": 0 < rank <= ELIGIBLE,
        "tied_at_top": sum(1 for _, sc in ranked if sc == top),
        "tied_with_truth": sum(1 for _, sc in ranked if sc == tscore) if rank else 0,
        "failing_visible_to_cortex": v.get("failing", []),
        "locate_s": round(took, 3), "locate_exit": p.returncode,
    }


def cmd_locate(a):
    corpus = load_corpus(CORPUS)
    os.makedirs(RESULTS, exist_ok=True)
    rows = [run_locate(t) for t in corpus["tasks"]]
    meta = {"corpus_sha16": file_sha16(CORPUS), "cortex_sha16": file_sha16(CORTEX),
            "axon_sha16": file_sha16(AXON), "n": len(rows)}
    with open(LOCATE, "w") as fh:
        for r in rows:
            fh.write(json.dumps({**r, "meta": meta}) + "\n")
    cov = sum(r["in_eligible"] for r in rows)
    print(f"locate: n={len(rows)} truth in top-{ELIGIBLE}: {cov} -> {LOCATE}", file=sys.stderr)


def load_locate():
    rows = read_jsonl(LOCATE)
    if not rows:
        raise SystemExit(f"{LOCATE} missing: run `python3 locrank.py locate` first")
    return {r["id"]: r for r in rows}


# ---------------------------------------------------------------- ranker input

SYSTEM = (
    "You localize a defect. Exactly one function in the Axon source file below "
    "contains an injected bug that makes the failing test fail. Rank the "
    "candidate functions from most to least likely to contain the bug.\n"
    "Reply with ONLY one JSON object and nothing else, of this exact shape:\n"
    '{"ranking": ["<candidate id>", "..."]}\n'
    "Use only the candidate ids given, each at most once. No prose, no explanation."
)


def ranker_prompt(task, eligible):
    """Only what cortex holds: the mutant without the hidden check, the visible
    failing check, the candidate names. The hidden test block is cut out of the
    source here (`given_a`), and no metadata about the mutation is used."""
    src = task["given_a"]
    blocks = {n: src[x:y] for n, x, y in test_blocks(src)}
    vis = task["visible"]
    if vis not in blocks:
        raise SystemExit(f"{task['id']}: visible test {vis} not found in the source")
    if task["hidden"] in blocks:
        raise SystemExit(f"{task['id']}: hidden check leaked into the ranker input")
    return (f"File: {task['file']}\n\n```axon\n{src}\n```\n\n"
            f"Failing test: {vis}\n\n```axon\n{blocks[vis].rstrip()}\n```\n\n"
            f"Candidate ids: {', '.join(eligible)}\n")


# ---------------------------------------------------------------- strict parse

FENCE = re.compile(r"\A```[A-Za-z]*\n(.*)\n```\Z", re.S)


def parse_ranking(text, eligible):
    """(order, None) or (None, abstain_reason). Accepted: one JSON object
    `{"ranking": [ids]}`, bare or as the WHOLE response inside one fence."""
    if text is None:
        return None, "no_output"
    t = text.strip()
    m = FENCE.match(t)
    if m:
        t = m.group(1).strip()
    try:
        v = json.loads(t)
    except ValueError:
        return None, "unparsable"
    if not isinstance(v, dict) or set(v) != {"ranking"} or not isinstance(v["ranking"], list):
        return None, "wrong_shape"
    ids = v["ranking"]
    if not ids:
        return None, "empty"
    seen = set()
    for c in ids:
        if not isinstance(c, str):
            return None, "wrong_shape"
        if c not in eligible:
            return None, "not_eligible"
        if c in seen:
            return None, "duplicate"
        seen.add(c)
    return ids, None


def final_order(order, eligible):
    if order is None:
        return list(eligible)
    return list(order) + [c for c in eligible if c not in order]


# ---------------------------------------------------------------- providers

class Refusal(Exception):
    """A provider that cannot run at all: stated, never silently skipped."""


def scripted(kind):
    def call(task, loc, prompt):
        el = loc["eligible"]
        if kind == "oracle":
            # Reorder only: a truth outside the eligible set cannot be added.
            order = ([loc["truth"]] if loc["truth"] in el else []) + \
                [c for c in el if c != loc["truth"]]
            text = json.dumps({"ranking": order})
        elif kind == "adversary":
            text = json.dumps({"ranking": [c for c in el[:1]] + ["__injected__"] + el[1:]})
        elif kind == "reverser":
            text = json.dumps({"ranking": list(reversed(el))})
        else:
            raise Refusal(f"unknown scripted provider {kind!r} (oracle|adversary|reverser)")
        return {"text": text, "cost_usd": 0.0}
    if kind not in ("oracle", "adversary", "reverser"):
        raise Refusal(f"unknown scripted provider {kind!r} (oracle|adversary|reverser)")
    return call, False


def claude_cli(model):
    if not os.path.exists(CLAUDE):
        raise Refusal(f"claude CLI not found at {CLAUDE} (set CLAUDE_BIN)")

    # Measured on operator-02 before the run (results/probes/): the bare CLI
    # with thinking on wrote 11.8k output tokens of prose with the JSON in a
    # trailing fence (83 s, $0.073, abstain/unparsable); with --json-schema and
    # thinking off it answered in 8 s for $0.013. The schema only shapes the
    # reply (ids are plain strings, NOT an enum of the eligible set), so the
    # strict parser below still decides validity.
    schema = {"type": "object", "additionalProperties": False, "required": ["ranking"],
              "properties": {"ranking": {"type": "array", "items": {"type": "string"}}}}
    env = dict(os.environ, MAX_THINKING_TOKENS="0")

    def call(task, loc, prompt):
        cwd = tempfile.mkdtemp(prefix="locrank_cli_")
        try:
            p = subprocess.run(
                [CLAUDE, "-p", "--output-format", "json", "--model", model,
                 "--tools", "", "--system-prompt", SYSTEM, "--no-session-persistence",
                 "--json-schema", json.dumps(schema),
                 "--settings", json.dumps({"alwaysThinkingEnabled": False})],
                input=prompt, capture_output=True, text=True, timeout=180, cwd=cwd, env=env)
        except subprocess.TimeoutExpired:
            return {"text": None, "error": "timeout", "cost_usd": None}
        finally:
            shutil.rmtree(cwd, ignore_errors=True)
        try:
            v = json.loads(p.stdout)
        except ValueError:
            return {"text": None, "error": f"cli exit {p.returncode}: {p.stderr[-300:]}",
                    "cost_usd": None}
        u = v.get("usage") or {}
        return {
            "text": None if v.get("is_error") else v.get("result"),
            "error": v.get("result") if v.get("is_error") else None,
            "cost_usd": v.get("total_cost_usd"),
            "api_ms": v.get("duration_api_ms"),
            "input_tokens": u.get("input_tokens"), "output_tokens": u.get("output_tokens"),
            "reasoning_tokens": (u.get("output_tokens_details") or {}).get("thinking_tokens"),
            "cache_read": u.get("cache_read_input_tokens"),
            "cache_create": u.get("cache_creation_input_tokens"),
            "models": list((v.get("modelUsage") or {}).keys()),
        }
    return call, True


def openrouter(model, reasoning=None):
    key = os.environ.get("OPENROUTER_API_KEY")
    if not key:
        raise Refusal("OPENROUTER_API_KEY is not set: refusing to run "
                      f"openrouter:{model} (no result is recorded for it)")

    def redact(s):
        return s.replace(key, "<redacted>") if s else s

    def post(body):
        req = urllib.request.Request(
            OPENROUTER_URL, data=json.dumps(body).encode(), method="POST",
            headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json",
                     "X-Title": "axon-cortex locrank"})
        with urllib.request.urlopen(req, timeout=180) as r:
            return json.loads(r.read())

    def call(task, loc, prompt):
        body = {"model": model, "temperature": 0, "max_tokens": 4096,
                "messages": [{"role": "system", "content": SYSTEM},
                             {"role": "user", "content": prompt}],
                "usage": {"include": True}}
        if reasoning == "off":
            body["reasoning"] = {"enabled": False}
        elif reasoning:
            body["reasoning"] = {"effort": reasoning}
        # Free routes are rate-limited: a 429 (or a 5xx from a flaky free
        # upstream) is retried with bounded backoff. `latency_s` is the
        # SUCCESSFUL attempt only; time spent in failed attempts and backoff is
        # reported separately as queueing, not folded into model latency.
        retries, queued, began_all = [], 0.0, time.time()
        for attempt in range(OR_MAX_ATTEMPTS):
            began = time.time()
            try:
                v = post(body)
            except urllib.error.HTTPError as e:
                msg = redact(e.read()[:300].decode(errors="replace"))
                if (e.code == 429 or e.code >= 500) and attempt + 1 < OR_MAX_ATTEMPTS:
                    wait = min(OR_BACKOFF_CAP_S, OR_BACKOFF_S * 2 ** attempt)
                    retries.append({"http": e.code, "wait_s": wait})
                    time.sleep(wait)
                    continue
                return {"text": None, "error": f"http {e.code}: {msg}", "cost_usd": None,
                        "retries": retries, "queue_s": round(time.time() - began_all, 3)}
            except (urllib.error.URLError, TimeoutError, ValueError) as e:
                return {"text": None, "error": redact(f"{type(e).__name__}: {e}"),
                        "cost_usd": None, "retries": retries,
                        "queue_s": round(time.time() - began_all, 3)}
            took = time.time() - began
            queued = (began - began_all)
            break
        else:  # pragma: no cover - the loop always returns or breaks
            raise AssertionError
        if v.get("error"):
            return {"text": None, "error": redact(json.dumps(v["error"])[:300]),
                    "cost_usd": None, "retries": retries, "queue_s": round(queued, 3),
                    "upstream_provider": v.get("provider")}
        u = v.get("usage") or {}
        ch = (v.get("choices") or [{}])[0]
        return {"text": (ch.get("message") or {}).get("content"),
                "finish_reason": ch.get("finish_reason"),
                "cost_usd": u.get("cost"), "input_tokens": u.get("prompt_tokens"),
                "output_tokens": u.get("completion_tokens"),
                "reasoning_tokens": (u.get("completion_tokens_details") or {}).get("reasoning_tokens"),
                "models": [v.get("model")], "upstream_provider": v.get("provider"),
                "retries": retries, "queue_s": round(queued, 3), "call_s": round(took, 3)}
    return call, True


def make_provider(spec, reasoning=None):
    """(callable | None, paid). `spectrum` has no callable: it IS the incumbent."""
    kind, _, arg = spec.partition(":")
    if kind == "spectrum" and not arg:
        return None, False
    if kind == "scripted":
        return scripted(arg)
    if kind == "claude-cli" and arg:
        return claude_cli(arg)
    if kind == "openrouter" and arg:
        return openrouter(arg, reasoning)
    raise Refusal(f"unknown provider {spec!r}: spectrum | scripted:{{oracle,adversary,"
                  "reverser}} | claude-cli:MODEL | openrouter:MODEL_ID")


# ---------------------------------------------------------------- run

def spent(root=RESULTS, prefix=""):
    """This lane's own model spend: every ranker row and probe under results/
    (only rows whose `provider` starts with `prefix`, when one is given). The
    arm-B snapshot is ANOTHER experiment's spend and is not counted."""
    total = 0.0
    for f in glob.glob(os.path.join(root, "**", "*.jsonl"), recursive=True):
        if os.path.abspath(f) == os.path.abspath(ARMB):
            continue
        for r in read_jsonl(f):
            if not prefix or (r.get("provider") or "").startswith(prefix):
                total += r.get("cost_usd") or 0.0
    return total


def rank_task(task, loc, call, provider):
    el = loc["eligible"]
    row = {"id": task["id"], "cls": task["class"], "provider": provider,
           "eligible": el, "truth": loc["truth"], "spectrum_rank": loc["spectrum_rank"],
           "in_eligible": loc["in_eligible"]}
    if call is None:
        order, reason, res, took, psha = list(el), None, {"cost_usd": 0.0}, 0.0, None
        status = "incumbent"
    else:
        prompt = ranker_prompt(task, el)
        psha = sha16(SYSTEM + "\n" + prompt)
        began = time.time()
        res = call(task, loc, prompt)
        took = time.time() - began
        order, reason = parse_ranking(res.get("text"), el)
        if res.get("error"):
            reason = "provider_error"
        status = "ok" if reason is None else "abstain"
        if reason:
            order = None
    fo = final_order(order, el)
    row.update({
        "status": status, "abstain_reason": reason,
        "complete": order is not None and len(order) == len(el),
        "final_order": fo,
        "final_rank": fo.index(loc["truth"]) + 1 if loc["truth"] in fo else 0,
        # Model latency: the successful call alone when the provider reports
        # it (OpenRouter, after any 429 backoff); `wall_s` includes queueing.
        "latency_s": None if res.get("error") else round(res.get("call_s", took), 3),
        "wall_s": round(took, 3),
        "prompt_sha16": psha,
        "raw": (res.get("text") or "")[:2000], "error": res.get("error"),
        **{k: res.get(k) for k in ("cost_usd", "api_ms", "input_tokens", "output_tokens",
                                   "reasoning_tokens", "cache_read", "cache_create", "models",
                                   "finish_reason", "upstream_provider", "retries", "queue_s")
           if k in res},
    })
    return row


def label(provider, reasoning):
    """A run's name in rows and the summary: the provider, plus the reasoning
    override when one was sent (default = the model's own default)."""
    return f"{provider}#reasoning={reasoning}" if reasoning else provider


def run_provider(provider, out, only=None, parallel=1, cap=2.0, reasoning=None, cap_prefix="",
                 max_call_usd=None):
    call, paid = make_provider(provider, reasoning)
    if parallel > 3:
        raise SystemExit("at most 3 parallel calls (shared machine)")
    corpus = load_corpus(CORPUS)
    locs = load_locate()
    tasks = [t for t in corpus["tasks"] if not only or t["id"] in only]
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    done = {r["id"] for r in read_jsonl(out)}
    tasks = [t for t in tasks if t["id"] not in done]
    meta = {"provider": provider, "reasoning": reasoning, "max_call_usd": max_call_usd,
            "corpus_sha16": file_sha16(CORPUS), "system_sha16": sha16(SYSTEM),
            "commit": subprocess.run(["git", "-C", ROOT, "rev-parse", "HEAD"],
                                     capture_output=True, text=True).stdout.strip()}
    lock, stop, in_flight = threading.Lock(), threading.Event(), [0]

    def one(t):
        if stop.is_set():
            return
        if paid:
            with lock:
                s = spent(prefix=cap_prefix)
                if s + (in_flight[0] + 1) * CALL_RESERVE_USD > cap:
                    stop.set()
                    print(f"BUDGET CAP: ${s:.4f} spent + reserve > ${cap}", file=sys.stderr)
                    return
                in_flight[0] += 1
        row = rank_task(t, locs[t["id"]], call, label(provider, reasoning))
        row["meta"] = meta
        with lock:
            with open(out, "a") as fh:
                fh.write(json.dumps(row) + "\n")
            if paid:
                in_flight[0] -= 1
            if max_call_usd is not None and (row.get("cost_usd") or 0.0) > max_call_usd:
                stop.set()
                print(f"PER-CALL CAP: {t['id']} cost ${row['cost_usd']:.4f} > ${max_call_usd}; "
                      "no further calls", file=sys.stderr, flush=True)
        print(f"  {label(provider, reasoning)} {t['id']} {row['status']}{'/' + row['abstain_reason'] if row['abstain_reason'] else ''} "
              f"rank {row['spectrum_rank']}->{row['final_rank']} {row['latency_s']}s "
              f"${row.get('cost_usd') or 0:.4f}", file=sys.stderr, flush=True)

    with cf.ThreadPoolExecutor(max_workers=parallel) as ex:
        list(ex.map(one, tasks))
    if stop.is_set():
        raise SystemExit(f"stopped early (cap ${cap}, per-call cap {max_call_usd}); "
                         "see the message above")


def cmd_run(a):
    out = a.out or os.path.join(RESULTS, slug(label(a.provider, a.reasoning)) + ".jsonl")
    try:
        run_provider(a.provider, out, set(a.only.split(",")) if a.only else None,
                     a.parallel, a.cap_usd, a.reasoning, a.cap_provider_prefix, a.max_call_usd)
    except Refusal as e:
        raise SystemExit(f"REFUSED: {e}")
    print(json.dumps(metrics(read_jsonl(out))), file=sys.stderr)


# ---------------------------------------------------------------- metrics

def pct(xs, q):
    xs = sorted(xs)
    if not xs:
        return None
    k = (len(xs) - 1) * q
    lo, hi = int(k), min(int(k) + 1, len(xs) - 1)
    return xs[lo] + (xs[hi] - xs[lo]) * (k - lo)


def metrics(rows):
    n = len(rows)
    if not n:
        return {"n": 0}
    ranks = [r["final_rank"] if r["in_eligible"] else 0 for r in rows]
    cov = sum(r["in_eligible"] for r in rows)
    # Latency over calls that RETURNED (a provider error has no model latency).
    lat = [r["latency_s"] for r in rows
           if r["status"] != "incumbent" and r["abstain_reason"] != "provider_error"]
    queue = [r.get("queue_s") or 0.0 for r in rows if "queue_s" in r]
    costs = [r.get("cost_usd") or 0.0 for r in rows]
    reasons = {}
    ups, routed = {}, {}
    for r in rows:
        if r["abstain_reason"]:
            reasons[r["abstain_reason"]] = reasons.get(r["abstain_reason"], 0) + 1
        if r.get("upstream_provider"):
            ups[r["upstream_provider"]] = ups.get(r["upstream_provider"], 0) + 1
        for mdl in r.get("models") or []:
            if mdl:
                routed[mdl] = routed.get(mdl, 0) + 1
    errors = reasons.get("provider_error", 0)
    return {
        "n": n, "coverage_top6": cov,
        "top1": sum(k == 1 for k in ranks), "top3": sum(0 < k <= 3 for k in ranks),
        "mrr": round(sum(1 / k for k in ranks if k) / n, 4),
        "abstain": sum(r["status"] == "abstain" for r in rows), "abstain_reasons": reasons,
        "incomplete_orders": sum(r["status"] == "ok" and not r["complete"] for r in rows),
        "provider_errors": errors, "errors_over_10pct": errors > 0.1 * n,
        "models_answering": routed,
        "usd_max_call": round(max(costs), 6),
        "latency_n": len(lat),
        "latency_p50_s": round(pct(lat, 0.5), 2) if lat else 0.0,
        "latency_p90_s": round(pct(lat, 0.9), 2) if lat else 0.0,
        "retries_total": sum(len(r.get("retries") or []) for r in rows),
        "queue_p50_s": round(pct(queue, 0.5), 2) if queue else None,
        "queue_p90_s": round(pct(queue, 0.9), 2) if queue else None,
        "upstream_providers": ups,
        "usd_total": round(sum(costs), 6), "usd_per_task": round(sum(costs) / n, 6),
    }


# ---------------------------------------------------------------- self-test

def cmd_selftest(a):
    # Parser contract, independent of any provider.
    el = ["a", "b", "c"]
    cases = [('{"ranking": ["b", "a", "c"]}', ["b", "a", "c"], None),
             ('```json\n{"ranking": ["c"]}\n```', ["c"], None),
             ('{"ranking": ["b", "zz"]}', None, "not_eligible"),
             ('{"ranking": ["b", "b"]}', None, "duplicate"),
             ('{"ranking": []}', None, "empty"),
             ('Sure! {"ranking": ["a"]}', None, "unparsable"),
             ('{"ranking": ["a"], "why": "x"}', None, "wrong_shape"),
             ('["a", "b"]', None, "wrong_shape"),
             ('', None, "unparsable"), (None, None, "no_output")]
    for text, want, why in cases:
        got = parse_ranking(text, el)
        assert got == (want, why), f"parse {text!r}: {got} != {(want, why)}"
    assert final_order(["c"], el) == ["c", "a", "b"]
    assert final_order(None, el) == el
    out = os.path.join(RESULTS, "selftest")
    os.makedirs(out, exist_ok=True)
    m = {}
    for kind in ("oracle", "adversary", "reverser"):
        p = os.path.join(out, f"scripted_{kind}.jsonl")
        if os.path.exists(p):
            os.remove(p)
        run_provider(f"scripted:{kind}", p)
        rows = read_jsonl(p)
        m[kind] = metrics(rows)
        if kind == "oracle":
            assert all(r["final_rank"] == 1 for r in rows if r["in_eligible"]), "oracle missed"
            assert m[kind]["top1"] == m[kind]["coverage_top6"]
            assert m[kind]["abstain"] == 0
        if kind == "adversary":
            assert m[kind]["abstain"] == len(rows), "adversary was not refused"
            assert all(r["abstain_reason"] == "not_eligible" and r["final_order"] == r["eligible"]
                       for r in rows), "abstain did not keep the spectrum order"
        if kind == "reverser":
            assert m[kind]["abstain"] == 0
            for r in rows:
                if r["in_eligible"]:
                    assert r["final_rank"] == len(r["eligible"]) + 1 - r["spectrum_rank"]
    json.dump(m, open(os.path.join(out, "selftest_summary.json"), "w"), indent=1)
    o = m["oracle"]
    print(f"selftest OK: parser {len(cases)} cases; oracle top1 {o['top1']}/{o['coverage_top6']} "
          f"covered (n={o['n']}); adversary abstain {m['adversary']['abstain']}/"
          f"{m['adversary']['n']}; reverser abstain {m['reverser']['abstain']}", file=sys.stderr)


# ---------------------------------------------------------------- arm-B projection

ARMB_KEYS = ("id", "localization_rank", "ranked_top", "attempted", "outcome",
             "verified_clean", "proposals", "cost_usd", "output_tokens", "api_ms",
             "wall_s", "per_proposal")


def cmd_snapshot(a):
    raw = open(a.src, "rb").read()
    rows = [json.loads(l) for l in raw.decode().splitlines() if l.strip()]
    os.makedirs(RESULTS, exist_ok=True)
    with open(ARMB, "w") as fh:
        for r in rows:
            fh.write(json.dumps({k: r.get(k) for k in ARMB_KEYS}) + "\n")
    meta = {"source": os.path.abspath(a.src), "rows": len(rows), "source_sha16": sha16(raw),
            "taken_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "model": (rows[0].get("meta") or {}).get("model") if rows else None}
    json.dump(meta, open(ARMB + ".meta.json", "w"), indent=1)
    print(f"snapshot: {len(rows)} arm-B rows -> {ARMB}", file=sys.stderr)


def lstsq2(X, y):
    """y ~ a*x0 + b*x1, no intercept, closed form."""
    s00 = sum(x[0] * x[0] for x in X)
    s01 = sum(x[0] * x[1] for x in X)
    s11 = sum(x[1] * x[1] for x in X)
    t0 = sum(x[0] * v for x, v in zip(X, y))
    t1 = sum(x[1] * v for x, v in zip(X, y))
    det = s00 * s11 - s01 * s01
    a = (t0 * s11 - t1 * s01) / det
    b = (s00 * t1 - s01 * t0) / det
    ss_res = sum((v - a * x[0] - b * x[1]) ** 2 for x, v in zip(X, y))
    my = sum(y) / len(y)
    ss_tot = sum((v - my) ** 2 for v in y)
    return a, b, 1 - ss_res / ss_tot


def projection(armb, provider_rows):
    """PROJECTION, not a measurement. Model, per arm-B row that ended
    verified_done with the truth among the candidates it walked:

      * cortex walks the candidate list depth-first and stops at the first
        verified repair; under a new order it walks the same way;
      * a candidate's cost (proposals, output tokens, $) does not depend on its
        position: an observed candidate costs what it cost in arm B; a wrong
        candidate arm B never reached costs the pooled mean of every observed
        wrong candidate; the truth costs what it cost in arm B;
      * no wrong candidate yields a verified repair (none did in arm B);
      * wall seconds = a*proposals + b*output_tokens, fitted by least squares
        (no intercept) over every snapshot row;
      * the ranker's own measured latency and $ are added per task.
    Rows not verified_done, or whose truth is outside the eligible set, are
    carried unchanged (a reorder cannot reach them).
    """
    fa, fb, r2 = lstsq2([(r["proposals"], r["output_tokens"]) for r in armb],
                        [r["wall_s"] for r in armb])

    def groups(r):
        g = {}
        for p in r["per_proposal"]:
            c = g.setdefault(p["candidate"], {"n": 0, "tok": 0, "usd": 0.0})
            c["n"] += 1
            c["tok"] += p.get("output_tokens") or 0
            c["usd"] += p.get("cost_usd") or 0.0
        return g

    def secs(c):
        return fa * c["n"] + fb * c["tok"]

    wrong = []
    for r in armb:
        truth = r["attempted"][-1] if r["outcome"] == "verified_done" else None
        wrong += [c for k, c in groups(r).items() if k != truth]
    pooled = {k: statistics.mean(c[k] for c in wrong) for k in ("n", "tok", "usd")} \
        if wrong else None
    by_id = {r["id"]: r for r in provider_rows}
    tot = {"rows": 0, "projected_rows": 0, "carried_rows": 0, "missing_provider_rows": 0,
           "observed_proposals": 0, "observed_wall_s": 0.0, "observed_usd": 0.0,
           "fitted_spectrum_wall_s": 0.0, "proposals": 0.0, "wall_s": 0.0, "usd": 0.0,
           "ranker_s": 0.0, "ranker_usd": 0.0, "eligible_mismatch": 0, "per_task": []}
    for r in armb:
        tot["rows"] += 1
        tot["observed_proposals"] += r["proposals"]
        tot["observed_wall_s"] += r["wall_s"]
        tot["observed_usd"] += r["cost_usd"] or 0.0
        g = groups(r)
        fitted = sum(secs(c) for c in g.values())
        pr = by_id.get(r["id"])
        ok = (r["outcome"] == "verified_done" and pr is not None and pr["in_eligible"]
              and r["attempted"] and r["attempted"][-1] == pr["truth"])
        if pr is None:
            tot["missing_provider_rows"] += 1
        if pr is not None and pr["eligible"] != r["ranked_top"]:
            tot["eligible_mismatch"] += 1
        if not ok:
            tot["carried_rows"] += 1
            n, w, usd = r["proposals"], fitted, r["cost_usd"] or 0.0
            order = None
        else:
            tot["projected_rows"] += 1
            n = w = usd = 0.0
            order = pr["final_order"]
            for c in order:
                cc = g.get(c, pooled)
                n += cc["n"]
                w += secs(cc)
                usd += cc["usd"]
                if c == pr["truth"]:
                    break
        rs, ru = (pr["latency_s"], pr.get("cost_usd") or 0.0) if pr else (0.0, 0.0)
        tot["fitted_spectrum_wall_s"] += fitted
        tot["proposals"] += n
        tot["wall_s"] += w + rs
        tot["usd"] += usd + ru
        tot["ranker_s"] += rs
        tot["ranker_usd"] += ru
        tot["per_task"].append({"id": r["id"], "observed_proposals": r["proposals"],
                                "observed_wall_s": r["wall_s"], "fitted_wall_s": round(fitted, 1),
                                "projected_proposals": round(n, 2),
                                "projected_wall_s": round(w + rs, 1),
                                "spectrum_rank": r["localization_rank"],
                                "provider_rank": pr["final_rank"] if pr else None,
                                "projected": ok})
    for k in ("observed_wall_s", "observed_usd", "fitted_spectrum_wall_s", "proposals",
              "wall_s", "usd", "ranker_s", "ranker_usd"):
        tot[k] = round(tot[k], 4 if "usd" in k else 1)
    return {"fit": {"sec_per_proposal": round(fa, 3), "sec_per_output_token": round(fb, 5),
                    "r2": round(r2, 3), "n_rows": len(armb)},
            "pooled_wrong_candidate": {k: round(v, 3) for k, v in pooled.items()} if pooled else None,
            "n_wrong_candidates_observed": len(wrong), **tot}


# ---------------------------------------------------------------- summary

def cmd_summary(a):
    files = sorted(f for f in glob.glob(os.path.join(RESULTS, "*.jsonl"))
                   if os.path.basename(f) not in ("locate.jsonl", "armb_snapshot.jsonl"))
    locs = load_locate()
    armb = read_jsonl(ARMB)
    armb_meta = json.load(open(ARMB + ".meta.json")) if armb else None
    out = {"n_tasks": len(locs), "eligible": ELIGIBLE,
           "spectrum_ties": {
               "truth_tied_with_rank1": sum(1 for r in locs.values()
                                            if r["spectrum_rank"] and r["tied_with_truth"] > 1
                                            and r["ranked"][r["spectrum_rank"] - 1]["score"]
                                            == r["ranked"][0]["score"]),
               "median_tied_at_top": statistics.median(r["tied_at_top"] for r in locs.values())},
           "armb_snapshot": armb_meta, "providers": {}}
    for f in files:
        rows = read_jsonl(f)
        prov = rows[0]["provider"]
        m = metrics(rows)
        by_cls = {}
        for c in sorted({r["cls"] for r in rows}):
            cm = metrics([r for r in rows if r["cls"] == c])
            by_cls[c] = {k: cm[k] for k in ("n", "coverage_top6", "top1", "top3", "mrr")}
        # Split by whether the spectrum's top score is unique: where it is
        # tied, the spectrum's rank-1 is a tie-break, not evidence.
        by_tie = {}
        for label, want in (("sole_top", True), ("tied_top", False)):
            sub = [r for r in rows if (locs[r["id"]]["tied_at_top"] == 1) == want]
            cm = metrics(sub)
            by_tie[label] = {k: cm.get(k) for k in ("n", "top1", "top3", "mrr")}
        out["providers"][prov] = {"file": os.path.relpath(f, HERE), **m, "by_class": by_cls,
                                  "by_spectrum_tie": by_tie,
                                  "projection": projection(armb, rows) if armb else None}
    json.dump(out, open(os.path.join(RESULTS, "summary.json"), "w"), indent=1)
    lines = [f"n = {out['n_tasks']} tasks; eligible = spectrum top-{ELIGIBLE}", "",
             "| provider | n | truth in top-6 | top-1 | top-3 | MRR | abstain | p50 s | p90 s | $/task | $ total |",
             "|---|---|---|---|---|---|---|---|---|---|---|"]
    for p, m in out["providers"].items():
        lines.append(f"| `{p}` | {m['n']} | {m['coverage_top6']} | {m['top1']} | {m['top3']} | "
                     f"{m['mrr']} | {m['abstain']} | {m['latency_p50_s']} | {m['latency_p90_s']} | "
                     f"{m['usd_per_task']:.5f} | {m['usd_total']:.4f} |")
    lines += ["", "OpenRouter call health (errors after bounded 429/5xx backoff are abstains):", "",
              "| provider | provider errors | >10% | retries | queue p50/p90 s | upstream |",
              "|---|---|---|---|---|---|"]
    for p, m in out["providers"].items():
        if p.startswith("openrouter:"):
            lines.append(f"| `{p}` | {m['provider_errors']} | {m['errors_over_10pct']} | "
                         f"{m['retries_total']} | {m['queue_p50_s']}/{m['queue_p90_s']} | "
                         f"{m['upstream_providers']} |")
    lines += ["", "Top-1 split by whether the spectrum's top score is unique "
              "(`sole_top`) or shared (`tied_top`):", "",
              "| provider | sole_top n | top-1 | tied_top n | top-1 |", "|---|---|---|---|---|"]
    for p, m in out["providers"].items():
        s, t = m["by_spectrum_tie"]["sole_top"], m["by_spectrum_tie"]["tied_top"]
        lines.append(f"| `{p}` | {s['n']} | {s['top1']} | {t['n']} | {t['top1']} |")
    if armb:
        lines += ["", f"PROJECTION over the arm-B snapshot ({armb_meta['rows']} rows, "
                  f"taken {armb_meta['taken_utc']}):", "",
                  "| provider | projected rows | proposals (observed -> projected) | "
                  "wall s (observed / fitted -> projected) | model $ (observed -> projected incl. ranker) |",
                  "|---|---|---|---|---|"]
        for p, m in out["providers"].items():
            j = m["projection"]
            lines.append(f"| `{p}` | {j['projected_rows']} | {j['observed_proposals']} -> "
                         f"{j['proposals']:.1f} | {j['observed_wall_s']} / {j['fitted_spectrum_wall_s']}"
                         f" -> {j['wall_s']} | {j['observed_usd']:.3f} -> {j['usd']:.3f} |")
        lines += ["", "Per task, the five slowest arm-B tasks (projected wall s incl. ranker; "
                  "spectrum rank -> provider rank):", ""]
        slow = ("operator-02", "operator-04", "operator-05", "operator-07", "operator-11")
        hdr = "| task | observed s | " + " | ".join(f"`{p}`" for p in out["providers"]) + " |"
        lines += [hdr, "|---|---|" + "---|" * len(out["providers"])]
        for tid in slow:
            cells = []
            obs = None
            for p, m in out["providers"].items():
                pt = next((x for x in m["projection"]["per_task"] if x["id"] == tid), None)
                if pt is None:
                    cells.append("n/a")
                    continue
                obs = pt["observed_wall_s"]
                cells.append(f"{pt['projected_wall_s']} ({pt['spectrum_rank']}->{pt['provider_rank']})")
            lines.append(f"| {tid} | {obs if obs is not None else 'n/a'} | " + " | ".join(cells) + " |")
    open(os.path.join(RESULTS, "summary.md"), "w").write("\n".join(lines) + "\n")
    print("\n".join(lines))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sp = ap.add_subparsers(dest="cmd", required=True)
    sp.add_parser("locate").set_defaults(fn=cmd_locate)
    sp.add_parser("selftest").set_defaults(fn=cmd_selftest)
    r = sp.add_parser("run")
    r.add_argument("--provider", required=True)
    r.add_argument("--out")
    r.add_argument("--only", default="")
    r.add_argument("--parallel", type=int, default=1)
    r.add_argument("--cap-usd", type=float, default=2.0,
                   help="stop launching calls once the $ sum of results/**/*.jsonl rows reaches this")
    r.add_argument("--cap-provider-prefix", default="",
                   help="count only rows whose provider starts with this (e.g. `openrouter:`)")
    r.add_argument("--max-call-usd", type=float,
                   help="stop launching calls once any single call has cost more than this")
    r.add_argument("--reasoning", help="openrouter only: `off` sends reasoning.enabled=false; "
                   "anything else is sent as reasoning.effort")
    r.set_defaults(fn=cmd_run)
    s = sp.add_parser("snapshot-armb")
    s.add_argument("src")
    s.set_defaults(fn=cmd_snapshot)
    sp.add_parser("summary").set_defaults(fn=cmd_summary)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
