#!/usr/bin/env python3
"""Run decision_set.jsonl through OpenRouter decision models and chat baselines.

    . /root/.openrouter.env              # OPENROUTER_API_KEY; never logged
    python3 run.py --probe               # which listed models answer at all -> results/probe.jsonl
    python3 run.py --pilot               # 10 items x every working model    -> results/pilot/
    python3 run.py --full                # every item x every working model  -> results/full/
    python3 run.py --stability           # 30-item subset x 3 more repeats   -> results/stability/
    python3 run.py --drift --models M --repeat N  # whole set again for M -> results/drift/
    python3 run.py --full --only-decision --outdir full-fast   # decision models only, own dir
    python3 run.py --full --only-decision --batch              # batched variant -> results/full-batched/
    python3 analyse.py                   # tables (README) from the JSONL above
    python3 analyse.py --phase full-fast --only-decision       # rank whatever has finished

Scheduling: every model gets its own worker pool (8 workers for a decision
model, 4 for a chat model) and all pools run at once, decision models
submitted first; a global semaphore keeps at most --concurrency (default 32)
HTTP requests in flight, held only for the duration of one request. A 429
pauses only that model (Retry-After when the response sends one, else
exponential), so a slow chat model or a throttled provider never holds up
the others. Each run appends wall time, calls and per-model 429 counts to
results/<outdir>/RUNLOG.json.

--batch sends one state with several questions: items whose states are equal
apart from their own `proposed_patch` / `ranked_candidates` (the same PoC
task; the six sealed candidates against one suite) go in one call; those
per-item fields move under `per_question.<qid>` and that question's
instructions say so. Each answer is
mapped back to its item; a missing or malformed answer is wrong for that
item only. Batched results are a separate variant, never ranked with the
unbatched runs.

Every run RESUMES: a (model, item, repeat) already recorded is not called again,
so re-running any phase over the committed JSONL makes no requests. Python 3
standard library only.

Decision models get the item as-is: POST /api/alpha/decisions {model, state,
questions: {qid: question}}. Chat baselines get the SAME state, instructions
and criteria rendered as one prompt that asks for a JSON answer of the same
shape, constrained by a strict JSON-schema `response_format` (structured
outputs, `provider.require_parameters`), temperature 0 where the model accepts
it. An answer that fails validation (missing question id,
wrong type, unknown option, probability outside [0,1], choice probabilities
not summing to 1 +- 0.05, unparseable chat text) is recorded as `malformed`
and scored WRONG, as is every HTTP error; neither is ever an approval.

Spend: before each call the harness sums `usage.cost` over every row under
results/ plus a reserve for calls in flight, and stops launching calls once
that reaches --cap-usd (default 10.0, this lane's hard cap).
"""
import argparse
import concurrent.futures as cf
import glob
import hashlib
import json
import math
import os
import re
import sys
import threading
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
SET = os.path.join(HERE, "decision_set.jsonl")
RESULTS = os.path.join(HERE, "results")
DECISIONS_URL = "https://openrouter.ai/api/alpha/decisions"
CHAT_URL = "https://openrouter.ai/api/v1/chat/completions"
LIST_URL = "https://openrouter.ai/api/v1/models?output_modalities=decisions"

# Every decision model the listing showed on 2026-10-12, except respan/span-01*:
# those score the BEHAVIOUR of a conversation (a different task, no
# state/questions contract) and are out of scope.
DECISION_MODELS = [
    "microsoft/microsoft-decision-1", "perplexity/pplx-decider-v1.1-27b", "openai/gpt-6-luna-decisions",
    "cloudflare/clef", "cloudflare/clef-flash", "cloudflare/clef-omni", "liquid/d1",
    "togethercomputer/tev1-4b-experimental", "inception/mercury-decide",
    "typesafe/jev-1.13", "~typesafe/jev-latest", "jaredpalmer/kev-4b", "nace-ai/drex-v1.5",
    "upstage/solar-decide", "upstage/solar-decide-flash",
]
SKIPPED = {"respan/span-01": "conversation-behaviour scorer, not a state/questions decision model",
           "respan/span-01-lite": "same family as respan/span-01",
           "respan/span-01-lite:free": "same family as respan/span-01",
           "inception/mercury-decide:free": "free alias of inception/mercury-decide (rate-limited); paid id used"}
# The chat baselines of the committed 2026-10-12 runs (analyse.py ranks these).
CHAT_MODELS_2026_10_12 = ["anthropic/claude-haiku-4.5", "anthropic/claude-haiku-5.5", "anthropic/claude-opus-5.5",
                          "typesafe/jev-router"]
# Later runs (user decision 2026-10-12: cost/latency; haiku-5.5 tied opus-5.5 and
# jev-router within noise) call only this one. The others still run with --models.
CHAT_MODELS = ["anthropic/claude-haiku-5.5"]
# The listing (supported_parameters, 2026-10-12) has no `temperature` for these;
# with require_parameters, sending it leaves no endpoint (HTTP 404).
NO_TEMPERATURE = {"anthropic/claude-haiku-5.5", "anthropic/claude-opus-5.5"}
# A router, not one model: with a json_schema response_format it routed 6/10
# pilot calls to an endpoint behind an 18+ attestation (HTTP 403). It gets the
# plain prompt (it returned valid JSON 10/10 that way); results/pilot-schema-403/.
NO_SCHEMA = {"typesafe/jev-router"}
# Reserved per call in flight, before its real cost is known.
RESERVE = {"decision": 0.002, "chat": 0.04}
# HTTP requests in flight, all models together (--concurrency). The pilot, the
# first full run, the stability and drift phases ran at 4 (a shared machine).
CONCURRENCY = 32
SLOTS = threading.BoundedSemaphore(CONCURRENCY)
# Requests in flight per model: WORKERS[kind] while many models run, rising to an
# equal share of CONCURRENCY as models finish (the slowest model is not left at 8
# with the other slots idle).
WORKERS = {"decision": 8, "chat": 4}
# Per-attempt timeout. The slowest successful decision call in any run took 49 s;
# in results/full-fast three calls hung until the earlier 180 s timeout, which set
# that run's wall time.
TIMEOUT = {"decision": 60, "chat": 180}
# An alias that resolves to a model already in the list: run in the probe and the
# pilot only, to check it returns the same answers, and never ranked twice.
ALIASES = {"~typesafe/jev-latest": "typesafe/jev-1.13"}
RETRY_STATUS = {408, 500, 502, 503, 504}
MAX_ATTEMPTS = 3      # attempts that end in a transient non-429 failure
MAX_429 = 8           # 429s tolerated per call before it is recorded as an HTTP error
MAX_BACKOFF_S = 60
# State fields that belong to one item, not to the evidence it shares with others
# (--batch groups items whose states are equal without them).
PER_ITEM = ("proposed_patch", "ranked_candidates")
PILOT_PER_CAT = 2
STABILITY_PER_CAT = 6
STABILITY_REPEATS = 3

CHAT_SYSTEM = (
    "You are a decision function. You receive a STATE and ONE question about it. "
    "Answer only from the state. Reply with ONLY one JSON object, no prose, no code fence.\n"
    "For a `choice` question: {\"choice\": \"<one option id>\", \"probabilities\": {\"<option id>\": <p>, ...}} "
    "with a probability for EVERY option id, summing to 1.\n"
    "For a `noul` (yes/no) question: {\"p_yes\": <probability from 0 to 1 that the answer is yes/true>}."
)


def sha16(s):
    return hashlib.sha256(s.encode()).hexdigest()[:16]


def slug(model):
    return re.sub(r"[^A-Za-z0-9._-]+", "_", model)


def read_jsonl(p):
    return [json.loads(l) for l in open(p) if l.strip()] if os.path.exists(p) else []


def load_set():
    return read_jsonl(SET)


def kind_of(model):
    return "decision" if model in DECISION_MODELS else "chat"


def subset(items, per_cat, salt):
    cats = sorted({x["cat"] for x in items})
    out = []
    for c in cats:
        xs = sorted((x for x in items if x["cat"] == c), key=lambda x: sha16(salt + x["id"]))
        out += xs[:per_cat]
    return out


# ---------------------------------------------------------------- spend

class Budget:
    def __init__(self, cap):
        self.cap, self.lock, self.in_flight = cap, threading.Lock(), 0.0
        self.base = spent()

    def reserve(self, amount):
        with self.lock:
            if self.base + self.in_flight + amount > self.cap:
                return False
            self.in_flight += amount
            return True

    def settle(self, reserved, actual):
        with self.lock:
            self.in_flight -= reserved
            self.base += actual or 0.0


def spent():
    total = 0.0
    for p in glob.glob(os.path.join(RESULTS, "**", "*.jsonl"), recursive=True):
        for r in read_jsonl(p):
            total += float(r.get("cost") or 0.0)
    return total


# ---------------------------------------------------------------- HTTP

def retry_after_s(headers):
    """Retry-After in seconds (delta form only), or None."""
    try:
        v = float((headers or {}).get("Retry-After"))
    except (TypeError, ValueError):
        return None
    return v if math.isfinite(v) and v >= 0 else None


def post(url, body, timeout=180):
    """(status, text, latency_s, retry_after_s). Holds one global slot while in flight."""
    key = os.environ.get("OPENROUTER_API_KEY")
    if not key:
        raise SystemExit("OPENROUTER_API_KEY is not set (. /root/.openrouter.env)")
    data = json.dumps(body).encode()
    req = urllib.request.Request(url, data, {"Authorization": "Bearer " + key,
                                             "Content-Type": "application/json"})
    with SLOTS:
        began = time.time()
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                raw = r.read()
                return r.status, raw.decode("utf-8", "replace"), time.time() - began, None
        except urllib.error.HTTPError as e:
            return e.code, e.read().decode("utf-8", "replace"), time.time() - began, retry_after_s(e.headers)
        except Exception as e:  # timeout, connection reset
            return None, f"{type(e).__name__}: {e}", time.time() - began, None


class Throttle:
    """One model's 429 back-off. A 429 pauses only this model's calls; the
    sleeping worker holds no global slot, so other models keep running."""

    def __init__(self):
        self.lock, self.until, self.streak, self.n429 = threading.Lock(), 0.0, 0, 0

    def wait(self):
        while True:
            with self.lock:
                d = self.until - time.time()
            if d <= 0:
                return
            time.sleep(min(d, 5.0))

    def hit(self, retry_after):
        with self.lock:
            self.n429 += 1
            self.streak += 1
            delay = retry_after if retry_after is not None else min(MAX_BACKOFF_S, 2.0 ** self.streak)
            self.until = max(self.until, time.time() + min(delay, MAX_BACKOFF_S))

    def ok(self):
        with self.lock:
            self.streak = 0


def call_with_retry(url, body, throttle=None, timeout=180):
    """(status, text, latency_of_last_attempt, attempts, n429). Bounded: up to
    MAX_ATTEMPTS transient failures (sleeping 5 s, 10 s between them) and
    MAX_429 rate limits (backing off through `throttle`, one per model)."""
    throttle = throttle or Throttle()
    attempts = n429 = 0
    while True:
        throttle.wait()
        status, text, took, retry_after = post(url, body, timeout)
        attempts += 1
        if status == 429:
            n429 += 1
            throttle.hit(retry_after)
            if n429 < MAX_429:
                continue
            return status, text, took, attempts, n429
        if status == 200:
            throttle.ok()
        failures = attempts - n429
        if status == 200 or (status is not None and status not in RETRY_STATUS) or failures >= MAX_ATTEMPTS:
            return status, text, took, attempts, n429
        time.sleep(5 * failures)


# ---------------------------------------------------------------- validation

def finite01(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v) and 0.0 <= v <= 1.0


def validate(item, ans):
    """(parsed, None) or (None, reason). parsed: {choice, probs} or {p_yes}."""
    q = item["question"]
    if not isinstance(ans, dict):
        return None, "answer is not an object"
    if q["type"] == "choice":
        opts = list(q["criteria"])
        c = ans.get("choice")
        if c not in opts:
            return None, f"choice {c!r} is not an option"
        probs = ans.get("probabilities")
        if probs is not None:
            if not isinstance(probs, dict) or any(k not in opts or not finite01(v) for k, v in probs.items()):
                return None, "probabilities malformed"
            if abs(sum(probs.values()) - 1.0) > 0.05:
                return None, f"probabilities sum to {sum(probs.values()):.3f}"
        return {"choice": c, "probs": probs}, None
    p = ans.get("noul", ans.get("p_yes"))
    if not finite01(p):
        return None, f"noul {p!r} is not a probability"
    return {"p_yes": float(p)}, None


def correct(item, parsed):
    if parsed is None:
        return False
    if item["qtype"] == "choice":
        return parsed["choice"] == item["label"]
    return (parsed["p_yes"] >= 0.5) == bool(item["label"])


# ---------------------------------------------------------------- one call

def chat_prompt(item):
    q = item["question"]
    return ("STATE:\n" + json.dumps(item["state"], indent=1) + "\n\nQUESTION (id `" + item["qid"] + "`, type `"
            + q["type"] + "`):\n" + json.dumps({"instructions": q["instructions"], "criteria": q["criteria"]},
                                               indent=1) + "\n")


def answer_schema(item):
    """Structured-output schema of the same answer shape (OpenRouter `response_format`).
    Without it claude-haiku-4.5 reasoned in prose before its JSON on 8/10 pilot items."""
    q = item["question"]
    if q["type"] == "choice":
        opts = list(q["criteria"])
        props = {"choice": {"type": "string", "enum": opts},
                 "probabilities": {"type": "object", "properties": {o: {"type": "number"} for o in opts},
                                   "required": opts, "additionalProperties": False}}
    else:
        props = {"p_yes": {"type": "number"}}
    return {"type": "json_schema", "json_schema": {
        "name": "decision", "strict": True,
        "schema": {"type": "object", "properties": props, "required": list(props), "additionalProperties": False}}}


def parse_chat(text):
    t = (text or "").strip()
    m = re.fullmatch(r"```(?:json)?\s*(.*?)\s*```", t, re.S)
    if m:
        t = m.group(1)
    try:
        return json.loads(t), None
    except ValueError:
        return None, "chat reply is not one JSON object"


def batch_qids(group):
    """Question ids for one call: the item's own qid, suffixed with the item's
    kind when two items in the group share it (two patches for one task)."""
    qids = [x["qid"] for x in group]
    out = [q if qids.count(q) == 1 else q + "__" + re.sub(r"[^A-Za-z0-9]+", "_", x["id"].split("/", 2)[-1]).strip("_")
           for q, x in zip(qids, group)]
    assert len(set(out)) == len(out), out
    return out


def decision_body(model, group):
    """One item: exactly the unbatched request. Several (--batch): the shared
    evidence once, each question's own fields under `per_question.<qid>`."""
    qids = batch_qids(group)
    if len(group) == 1:
        return {"model": model, "state": group[0]["state"], "questions": {qids[0]: group[0]["question"]}}, qids
    state = {k: v for k, v in group[0]["state"].items() if k not in PER_ITEM}
    per, questions = {}, {}
    for q, x in zip(qids, group):
        extra = {k: v for k, v in x["state"].items() if k in PER_ITEM}
        question = dict(x["question"])
        if extra:
            per[q] = extra
            names = ", ".join(f"`{k}`" for k in extra)
            question["instructions"] = (f"For this question, {names} {'is' if len(extra) == 1 else 'are'} "
                                        f"under `per_question.{q}` in the state. " + question["instructions"])
        questions[q] = question
    if per:
        state["per_question"] = per
    return {"model": model, "state": state, "questions": questions}, qids


def chat_body(model, item):
    body = {"model": model, "max_tokens": 1024, "usage": {"include": True},
            "messages": [{"role": "system", "content": CHAT_SYSTEM},
                         {"role": "user", "content": chat_prompt(item)}]}
    if model not in NO_SCHEMA:
        body.update(response_format=answer_schema(item), provider={"require_parameters": True})
    if model not in NO_TEMPERATURE:
        body["temperature"] = 0
    return body


def run_group(model, group, phase, repeat, throttle=None):
    """One HTTP call answering every item in `group`; one row per item. A batch
    row carries its share of the call's cost (`cost`) and the whole call's
    (`batch_cost`, `batch_*_tokens`); latency is the call's."""
    kind = kind_of(model)
    if kind == "decision":
        body, qids = decision_body(model, group)
        url = DECISIONS_URL
    else:
        if len(group) != 1:
            raise SystemExit("--batch is for decision models only")
        body, qids, url = chat_body(model, group[0]), [group[0]["qid"]], CHAT_URL
    common = {"model": model, "kind": kind, "phase": phase, "repeat": repeat,
              "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "request_sha16": sha16(json.dumps(body, sort_keys=True))}
    began = time.time()
    status, text, took, attempts, n429 = call_with_retry(url, body, throttle, TIMEOUT[kind])
    common.update(http_status=status, latency_s=round(took, 3), elapsed_s=round(time.time() - began, 3),
                  attempts=attempts, n429=n429)
    batched = len(group) > 1
    if batched:
        common.update(batch_id=common["request_sha16"], batch_size=len(group))
    call_err, resp = None, None
    if status != 200:
        call_err = f"http {status}: {text[:300]}"
    else:
        try:
            resp = json.loads(text)
        except ValueError:
            call_err = "response is not JSON"
    if resp is not None:
        if "error" in resp and not resp.get("answers") and not resp.get("choices"):
            call_err = f"error body: {json.dumps(resp['error'])[:300]}"
        usage = resp.get("usage") or {}
        tin = usage.get("input_tokens", usage.get("prompt_tokens"))
        tout = usage.get("output_tokens", usage.get("completion_tokens"))
        cost = usage.get("cost")
        common.update(model_resolved=resp.get("model"), provider=resp.get("provider"))
        if batched:
            common.update(batch_input_tokens=tin, batch_output_tokens=tout, batch_cost=cost,
                          cost=None if cost is None else cost / len(group))
        else:
            common.update(input_tokens=tin, output_tokens=tout, cost=cost)
    rows = []
    for item, qid in zip(group, qids):
        row = dict(common, item=item["id"], cat=item["cat"])
        if batched:
            row["batch_qid"] = qid
        parsed, err = None, call_err
        if err is None and kind == "decision":
            ans = (resp.get("answers") or {}).get(qid)
            row["answer"] = ans
            if ans is None:
                err = f"no answer for question {qid!r}"
            elif isinstance(ans, dict) and ans.get("type") not in (None, item["qtype"]):
                err = f"answer type {ans.get('type')!r} != {item['qtype']}"
            else:
                parsed, err = validate(item, ans)
        elif err is None:
            try:
                content = resp["choices"][0]["message"]["content"]
            except (KeyError, IndexError, TypeError):
                content = None
            row["raw_text"] = content
            row["finish_reason"] = (resp.get("choices") or [{}])[0].get("finish_reason")
            ans, err = parse_chat(content)
            if err is None:
                row["answer"] = ans
                parsed, err = validate(item, ans)
        row["parsed"] = parsed
        row["error"] = err
        row["error_class"] = (None if err is None else "http" if status != 200 else "malformed")
        row["correct"] = correct(item, parsed) if err is None else False
        rows.append(row)
    return rows


def run_one(model, item, phase, repeat, throttle=None):
    return run_group(model, [item], phase, repeat, throttle)[0]


# ---------------------------------------------------------------- phases

def working_models():
    """Models whose most recent probe returned a valid answer."""
    last = {}
    for r in read_jsonl(os.path.join(RESULTS, "probe.jsonl")):
        last[r["model"]] = r
    return [m for m in DECISION_MODELS + CHAT_MODELS if m in last and last[m].get("error") is None]


def groups_of(items, batch):
    """Unbatched: one item per call. --batch: items whose state is identical
    apart from PER_ITEM fields (the same PoC task, or the six sealed candidates
    against one suite) share a call. A MiCode gate item's tool call is its whole
    evidence, so gate items never group."""
    if not batch:
        return [[x] for x in items]
    groups = {}
    for x in items:
        s = x["state"]
        key = (json.dumps({k: v for k, v in s.items() if k not in PER_ITEM}, sort_keys=True)
               if isinstance(s, dict) else "solo:" + x["id"])
        groups.setdefault(key, []).append(x)
    return [sorted(g, key=lambda x: x["id"]) for g in groups.values()]


def run_phase(phase, items, models, repeats, cap, outdir=None, batch=False):
    """Every model gets its own pool; all pools run at once (decision models
    submitted first); SLOTS bounds the HTTP requests in flight overall, and a
    model has at most max(WORKERS[kind], CONCURRENCY // models still running)."""
    outdir = outdir or phase
    d = os.path.join(RESULTS, outdir)
    os.makedirs(d, exist_ok=True)
    budget = Budget(cap)
    groups = groups_of(items, batch)
    jobs = {}
    for m in models:
        for rep in repeats:
            out = os.path.join(d, f"{slug(m)}.jsonl" if len(repeats) == 1 and rep == 0 else f"{slug(m)}.r{rep}.jsonl")
            done = {r["item"] for r in read_jsonl(out)}
            jobs.setdefault(m, []).extend((g, rep, out) for g in groups if not all(x["id"] in done for x in g))
        # Within a model, a fixed pseudo-random order mixes the categories.
        jobs[m].sort(key=lambda j: sha16(j[0][0]["id"] + str(j[1])))
    n_calls = sum(len(v) for v in jobs.values())
    print(f"{outdir}: {n_calls} calls to make ({sum(len(g) for v in jobs.values() for g, _, _ in v)} decisions); "
          f"spent so far ${budget.base:.4f} of ${cap:.2f}; {CONCURRENCY} in flight max", file=sys.stderr)
    lock, stop = threading.Lock(), threading.Event()
    throttles = {m: Throttle() for m in models}
    calls = {m: 0 for m in models}
    finished = {}
    cv = threading.Condition()
    inflight = {m: 0 for m in models}
    active = {m for m in models if jobs[m]}
    began, started = time.time(), time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())

    def limit(m):
        return max(WORKERS[kind_of(m)], CONCURRENCY // max(1, len(active)))

    def work(m, job):
        g, rep, out = job
        if stop.is_set():
            return
        reserve = RESERVE[kind_of(m)] * len(g)
        with cv:
            while inflight[m] >= limit(m):
                cv.wait()
            inflight[m] += 1
        if not budget.reserve(reserve):
            stop.set()
            print(f"STOP: spend cap ${cap:.2f} reached", file=sys.stderr)
            with cv:
                inflight[m] -= 1
                cv.notify_all()
            return
        rows = None
        try:
            rows = run_group(m, g, outdir, rep, throttles[m])
        finally:
            with cv:
                inflight[m] -= 1
                cv.notify_all()
            budget.settle(reserve, sum(float(r.get("cost") or 0.0) for r in rows or []))
        with lock:
            with open(out, "a") as f:
                f.write("".join(json.dumps(r, sort_keys=True) + "\n" for r in rows))
            calls[m] += 1
        errs = [r for r in rows if r["error"]]
        if errs:
            more = f" (+{len(errs) - 1} more in this call)" if len(errs) > 1 else ""
            print(f"  {m} {errs[0]['item']}{more}: {errs[0]['error'][:160]}", file=sys.stderr)

    def on_done(m):
        def cb(_):
            with lock:
                remaining[m] -= 1
                last = remaining[m] == 0
                if last:
                    finished[m] = round(time.time() - began, 1)
                    print(f"  {m}: done at +{finished[m]:.0f}s", file=sys.stderr)
            if last:
                with cv:
                    active.discard(m)
                    cv.notify_all()
        return cb

    ordered = [m for m in models if kind_of(m) == "decision"] + [m for m in models if kind_of(m) == "chat"]
    pools = {m: cf.ThreadPoolExecutor(CONCURRENCY, thread_name_prefix=slug(m)) for m in ordered if jobs[m]}
    remaining = {m: len(jobs[m]) for m in pools}
    futs = []
    for m, pool in pools.items():
        for j in jobs[m]:
            f = pool.submit(work, m, j)
            f.add_done_callback(on_done(m))
            futs.append(f)
    for f in futs:
        f.result()
    for pool in pools.values():
        pool.shutdown()
    wall = round(time.time() - began, 1)

    def done_at(kind):
        ts = [finished[m] for m in pools if kind_of(m) == kind and m in finished]
        return max(ts) if ts else None
    if n_calls:
        log_p = os.path.join(d, "RUNLOG.json")
        log = json.load(open(log_p)) if os.path.exists(log_p) else []
        log.append({"outdir": outdir, "batch": batch, "started": started, "wall_s": wall,
                    "concurrency": CONCURRENCY, "workers_per_model": WORKERS, "timeout_s": TIMEOUT,
                    "per_model_limit": "max(workers, concurrency // models running)", "calls": sum(calls.values()),
                    "decision_models_done_s": done_at("decision"), "chat_models_done_s": done_at("chat"),
                    "stopped_by_cap": stop.is_set(),
                    "models": {m: {"calls": calls[m], "done_s": finished.get(m), "n429": throttles[m].n429}
                               for m in pools}})
        json.dump(log, open(log_p, "w"), indent=1, sort_keys=True)
    n429 = {m: t.n429 for m, t in throttles.items() if t.n429}
    print(f"{outdir}: done in {wall:.0f}s; {sum(calls.values())} calls; 429s per model: {n429 or 'none'}; "
          f"spent ${spent():.4f}", file=sys.stderr)
    return not stop.is_set()


PROBE_ITEM = {"id": "probe", "cat": "probe", "qid": "buggy_fn", "qtype": "choice", "label": "scale",
              "state": {"program": "fn add(a: i64, b: i64) -> i64 { a + b }\nfn scale(x: i64) -> i64 { x * 3 }"
                                   "   // intended: double the input\nfn clamp(x: i64) -> i64 { if x > 100 { 100 } "
                                   "else { x } }\n@[test] fn t() { assert_eq(clamp(add(scale(5), 1)), 11) }",
                        "failing_test": "t: assertion failed: 16 != 11"},
              "question": {"type": "choice", "instructions": "Which function contains the bug that makes the test fail?",
                           "criteria": {"add": "add two numbers", "scale": "double the input", "clamp": "cap at 100"}}}


def probe(models):
    out = os.path.join(RESULTS, "probe.jsonl")
    os.makedirs(RESULTS, exist_ok=True)
    budget = Budget(10 ** 9)
    with cf.ThreadPoolExecutor(CONCURRENCY) as ex:
        rows = list(ex.map(lambda m: run_one(m, PROBE_ITEM, "probe", 0), models))
    with open(out, "a") as f:
        for r in rows:
            f.write(json.dumps(r, sort_keys=True) + "\n")
            print(f"{r['model']:42s} {'OK  ' if r['error'] is None else 'FAIL'} {r['latency_s']:6.2f}s "
                  f"resolved={r.get('model_resolved')} provider={r.get('provider')} "
                  f"{'' if r['error'] is None else r['error'][:140]}")
    del budget


def main():
    global CONCURRENCY, SLOTS
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    g = ap.add_mutually_exclusive_group(required=True)
    for p in ("probe", "pilot", "full", "stability", "drift", "list"):
        g.add_argument("--" + p, action="store_true")
    ap.add_argument("--models", help="comma-separated subset (default: every model the last probe found working)")
    k = ap.add_mutually_exclusive_group()
    k.add_argument("--only-decision", action="store_true", help="decision models only (/api/alpha/decisions)")
    k.add_argument("--only-chat", action="store_true", help="chat baselines only")
    ap.add_argument("--batch", action="store_true",
                    help="--full only: one call per group of items sharing evidence -> results/full-batched/")
    ap.add_argument("--outdir", help="results subdirectory (default: the phase name; full-batched with --batch)")
    ap.add_argument("--concurrency", type=int, default=CONCURRENCY, help="HTTP requests in flight, all models")
    ap.add_argument("--cap-usd", type=float, default=10.0)
    ap.add_argument("--repeat", type=int, default=1, help="repeat index for --drift")
    a = ap.parse_args()
    CONCURRENCY, SLOTS = a.concurrency, threading.BoundedSemaphore(a.concurrency)
    items = load_set()
    if a.list:
        print(json.dumps({"decision": DECISION_MODELS, "chat": CHAT_MODELS, "skipped": SKIPPED}, indent=1))
        return
    if a.probe:
        probe(a.models.split(",") if a.models else DECISION_MODELS + CHAT_MODELS)
        return
    models = a.models.split(",") if a.models else working_models()
    if a.only_decision or a.only_chat:
        models = [m for m in models if (kind_of(m) == "decision") == a.only_decision]
    if not models:
        raise SystemExit("no working models: run --probe first")
    if a.batch and not a.full:
        raise SystemExit("--batch goes with --full")
    if a.batch and any(kind_of(m) == "chat" for m in models):
        raise SystemExit("--batch is for decision models only: add --only-decision")
    ranked = [m for m in models if m not in ALIASES]
    if a.pilot:
        ok = run_phase("pilot", subset(items, PILOT_PER_CAT, "pilot"), models, [0], a.cap_usd, a.outdir)
    elif a.full:
        ok = run_phase("full", items, ranked, [0], a.cap_usd,
                       a.outdir or ("full-batched" if a.batch else None), a.batch)
    elif a.drift:
        # Whole-set extra samples (--repeat N, 1-based), for a model whose stability repeats
        # disagree with its full run (used for perplexity/pplx-decider-v1.1-27b; pass --models).
        ok = run_phase("drift", items, ranked, [a.repeat], a.cap_usd, a.outdir)
    else:
        ok = run_phase("stability", subset(items, STABILITY_PER_CAT, "stability"), ranked,
                       list(range(1, STABILITY_REPEATS + 1)), a.cap_usd, a.outdir)
    sys.exit(0 if ok else 3)


if __name__ == "__main__":
    main()
