#!/usr/bin/env python3
"""Run decision_set.jsonl through OpenRouter decision models and chat baselines.

    . /root/.openrouter.env              # OPENROUTER_API_KEY; never logged
    python3 run.py --probe               # which listed models answer at all -> results/probe.jsonl
    python3 run.py --pilot               # 10 items x every working model    -> results/pilot/
    python3 run.py --full                # every item x every working model  -> results/full/
    python3 run.py --stability           # 30-item subset x 3 more repeats   -> results/stability/
    python3 run.py --drift --models M --repeat N  # whole set again for M -> results/drift/
    python3 analyse.py                   # tables (README) from the JSONL above

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
CHAT_MODELS = ["anthropic/claude-haiku-4.5", "anthropic/claude-haiku-5.5", "anthropic/claude-opus-5.5",
               "typesafe/jev-router"]
# The listing (supported_parameters, 2026-10-12) has no `temperature` for these;
# with require_parameters, sending it leaves no endpoint (HTTP 404).
NO_TEMPERATURE = {"anthropic/claude-haiku-5.5", "anthropic/claude-opus-5.5"}
# A router, not one model: with a json_schema response_format it routed 6/10
# pilot calls to an endpoint behind an 18+ attestation (HTTP 403). It gets the
# plain prompt (it returned valid JSON 10/10 that way); results/pilot-schema-403/.
NO_SCHEMA = {"typesafe/jev-router"}
# Reserved per call in flight, before its real cost is known.
RESERVE = {"decision": 0.002, "chat": 0.04}
MAX_CONCURRENT = 4
# An alias that resolves to a model already in the list: run in the probe and the
# pilot only, to check it returns the same answers, and never ranked twice.
ALIASES = {"~typesafe/jev-latest": "typesafe/jev-1.13"}
RETRY_STATUS = {408, 429, 500, 502, 503, 504}
MAX_ATTEMPTS = 3
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

def post(url, body, timeout=180):
    key = os.environ.get("OPENROUTER_API_KEY")
    if not key:
        raise SystemExit("OPENROUTER_API_KEY is not set (. /root/.openrouter.env)")
    data = json.dumps(body).encode()
    req = urllib.request.Request(url, data, {"Authorization": "Bearer " + key,
                                             "Content-Type": "application/json"})
    began = time.time()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
            return r.status, raw.decode("utf-8", "replace"), time.time() - began
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", "replace"), time.time() - began
    except Exception as e:  # timeout, connection reset
        return None, f"{type(e).__name__}: {e}", time.time() - began


def call_with_retry(url, body):
    """(status, text, latency_of_last_attempt, attempts). Bounded; only transient failures retried."""
    for attempt in range(1, MAX_ATTEMPTS + 1):
        status, text, took = post(url, body)
        if status == 200 or (status is not None and status not in RETRY_STATUS) or attempt == MAX_ATTEMPTS:
            return status, text, took, attempt
        time.sleep(5 * attempt)


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


def run_one(model, item, phase, repeat):
    kind = kind_of(model)
    row = {"model": model, "kind": kind, "phase": phase, "repeat": repeat, "item": item["id"],
           "cat": item["cat"], "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    if kind == "decision":
        body = {"model": model, "state": item["state"], "questions": {item["qid"]: item["question"]}}
        url = DECISIONS_URL
    else:
        body = {"model": model, "max_tokens": 1024, "usage": {"include": True},
                "messages": [{"role": "system", "content": CHAT_SYSTEM},
                             {"role": "user", "content": chat_prompt(item)}]}
        if model not in NO_SCHEMA:
            body.update(response_format=answer_schema(item), provider={"require_parameters": True})
        if model not in NO_TEMPERATURE:
            body["temperature"] = 0
        url = CHAT_URL
    row["request_sha16"] = sha16(json.dumps(body, sort_keys=True))
    status, text, took, attempts = call_with_retry(url, body)
    row.update(http_status=status, latency_s=round(took, 3), attempts=attempts)
    parsed, err, resp = None, None, None
    if status != 200:
        err = f"http {status}: {text[:300]}"
    else:
        try:
            resp = json.loads(text)
        except ValueError:
            err = "response is not JSON"
    if resp is not None:
        if "error" in resp and not resp.get("answers") and not resp.get("choices"):
            err = f"error body: {json.dumps(resp['error'])[:300]}"
        usage = resp.get("usage") or {}
        row["model_resolved"] = resp.get("model")
        row["provider"] = resp.get("provider")
        row["input_tokens"] = usage.get("input_tokens", usage.get("prompt_tokens"))
        row["output_tokens"] = usage.get("output_tokens", usage.get("completion_tokens"))
        row["cost"] = usage.get("cost")
        if err is None and kind == "decision":
            ans = (resp.get("answers") or {}).get(item["qid"])
            row["answer"] = ans
            if isinstance(ans, dict) and ans.get("type") not in (None, item["qtype"]):
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
    return row


# ---------------------------------------------------------------- phases

def working_models():
    """Models whose most recent probe returned a valid answer."""
    last = {}
    for r in read_jsonl(os.path.join(RESULTS, "probe.jsonl")):
        last[r["model"]] = r
    return [m for m in DECISION_MODELS + CHAT_MODELS if m in last and last[m].get("error") is None]


def run_phase(phase, items, models, repeats, cap):
    os.makedirs(os.path.join(RESULTS, phase), exist_ok=True)
    budget = Budget(cap)
    jobs = []
    for m in models:
        for rep in repeats:
            out = os.path.join(RESULTS, phase, f"{slug(m)}.jsonl" if len(repeats) == 1 and rep == 0
                               else f"{slug(m)}.r{rep}.jsonl")
            done = {r["item"] for r in read_jsonl(out)}
            jobs += [(m, x, rep, out) for x in items if x["id"] not in done]
    print(f"{phase}: {len(jobs)} calls to make; spent so far ${budget.base:.4f} of ${cap:.2f}", file=sys.stderr)
    lock, stop = threading.Lock(), threading.Event()

    def work(job):
        m, x, rep, out = job
        if stop.is_set():
            return
        reserve = RESERVE[kind_of(m)]
        if not budget.reserve(reserve):
            stop.set()
            print(f"STOP: spend cap ${cap:.2f} reached", file=sys.stderr)
            return
        row = None
        try:
            row = run_one(m, x, phase, rep)
        finally:
            budget.settle(reserve, float((row or {}).get("cost") or 0.0))
        with lock:
            with open(out, "a") as f:
                f.write(json.dumps(row, sort_keys=True) + "\n")
        if row["error"]:
            print(f"  {m} {x['id']}: {row['error'][:160]}", file=sys.stderr)

    # Interleave models so one slow model does not serialise the run.
    jobs.sort(key=lambda j: (sha16(j[1]["id"] + str(j[2])), j[0]))
    with cf.ThreadPoolExecutor(MAX_CONCURRENT) as ex:
        list(ex.map(work, jobs))
    print(f"{phase}: done; spent ${spent():.4f}", file=sys.stderr)
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
    with cf.ThreadPoolExecutor(MAX_CONCURRENT) as ex:
        rows = list(ex.map(lambda m: run_one(m, PROBE_ITEM, "probe", 0), models))
    with open(out, "a") as f:
        for r in rows:
            f.write(json.dumps(r, sort_keys=True) + "\n")
            print(f"{r['model']:42s} {'OK  ' if r['error'] is None else 'FAIL'} {r['latency_s']:6.2f}s "
                  f"resolved={r.get('model_resolved')} provider={r.get('provider')} "
                  f"{'' if r['error'] is None else r['error'][:140]}")
    del budget


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    g = ap.add_mutually_exclusive_group(required=True)
    for p in ("probe", "pilot", "full", "stability", "drift", "list"):
        g.add_argument("--" + p, action="store_true")
    ap.add_argument("--models", help="comma-separated subset (default: every model the last probe found working)")
    ap.add_argument("--cap-usd", type=float, default=10.0)
    ap.add_argument("--repeat", type=int, default=1, help="repeat index for --drift")
    a = ap.parse_args()
    items = load_set()
    if a.list:
        print(json.dumps({"decision": DECISION_MODELS, "chat": CHAT_MODELS, "skipped": SKIPPED}, indent=1))
        return
    if a.probe:
        probe(a.models.split(",") if a.models else DECISION_MODELS + CHAT_MODELS)
        return
    models = a.models.split(",") if a.models else working_models()
    if not models:
        raise SystemExit("no working models: run --probe first")
    if a.pilot:
        ok = run_phase("pilot", subset(items, PILOT_PER_CAT, "pilot"), models, [0], a.cap_usd)
    elif a.full:
        ok = run_phase("full", items, [m for m in models if m not in ALIASES], [0], a.cap_usd)
    elif a.drift:
        # Whole-set extra samples (--repeat N, 1-based), for a model whose stability repeats
        # disagree with its full run (used for perplexity/pplx-decider-v1.1-27b; pass --models).
        ok = run_phase("drift", items, [m for m in models if m not in ALIASES], [a.repeat], a.cap_usd)
    else:
        ok = run_phase("stability", subset(items, STABILITY_PER_CAT, "stability"),
                       [m for m in models if m not in ALIASES],
                       list(range(1, STABILITY_REPEATS + 1)), a.cap_usd)
    sys.exit(0 if ok else 3)


if __name__ == "__main__":
    main()
