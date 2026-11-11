#!/usr/bin/env python3
"""genviab's OpenAI-compatible program generator behind axon-intent's `cmd:`
seam (authoring-eval AE-1): ONE `/chat/completions` call per invocation.

The prompt (card + task, or a repair prompt) on stdin, the model's reply
VERBATIM on stdout, an `axon-intent-tokens: <n>` line on stderr; exit 0 with a
reply, 1 when the call failed (row logged, `error` set), 4 when it was refused
before any request. One JSON row per invocation is appended to
$GENVIAB_MODEL_LOG with micode_program_log.py's keys plus `provider`,
`base_url_host`, `temperature`, `seed`, `max_tokens`, `finish_reason`,
`cached_tokens`, `reply_sha256`, `cache_hit` (and `truncated`, `shared`,
`replayed`, `replay_mismatch`, `cache_key`, `server`).

Providers (`GENVIAB_PROVIDER`): only `cere` (PROVIDERS). The bearer token is
read from the provider's env file (key CERE_TOKEN_AE) into this process only:
never argv, never a log row, never a file. A missing key is a refusal before
the first request, never a fallback to another key in the file.
GENVIAB_CERE_TOKEN_KEY names a different key explicitly (dev acceptance on a
host where CERE_TOKEN_AE is not issued yet); the row records the key name.
GENVIAB_HTTP_BASE_URL and GENVIAB_HTTP_ENV_FILE replace the provider's base
URL and env file only under GENVIAB_SELFTEST=1 (a loopback fake server).

Limits: `max_tokens` (GENVIAB_MAX_TOKENS, default 8192), `temperature`
(GENVIAB_TEMPERATURE, default 0.7) and `seed` (GENVIAB_SEED, default 0) are
sent explicitly. `finish_reason == "length"` with the declared `max_tokens`
reached is the model's own output: the call succeeds with `truncated: true`
(a model failure the grader sees, spec §2). A `length` stop below the declared
cap is a transport limit: exit 1, an infrastructure failure (D9).

Reply cache (D4): GENVIAB_CACHE_DIR (unset: off). Key = sha256 of (prompt
sha256, GENVIAB_SERIES, temperature, seed, max_tokens, this driver's sha256);
never a binary sha. A hit returns the stored reply (`cache_hit: true`). A
seeded sample of hits is re-queried (GENVIAB_REPLAY_SEED; the first
REPLAY_MIN = 20 hits of a run, then REPLAY_FRACTION = 0.02 of them by a hash
of seed and key; GENVIAB_REPLAY_MIN/_FRACTION override them only under
GENVIAB_SELFTEST=1; the run counts them in GENVIAB_RUN_STATE); a
replay whose reply_sha256 differs writes GENVIAB_RUN_STATE/cache-disabled and
the cache is neither read nor written for the rest of that run.
GENVIAB_SHARE_DIR (ae.py, `regrade`): replies generated in this run, keyed the
same way, reused for an identical prompt under the other binary
(`shared: true`, not a cache hit).
"""
import fcntl
import hashlib
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

E = os.environ
PROVIDERS = {
    "cere": {"base_url": "http://127.0.0.1:18307/v1", "env_file": "/etc/cere/cere-t4.env",
             "token_key": "CERE_TOKEN_AE", "model": "Qwen3.8-Flash-Next"},
}
DEFAULT_MAX_TOKENS = 8192
DEFAULT_TEMPERATURE = 0.7
REPLAY_MIN = 20
REPLAY_FRACTION = 0.02
TIMEOUT_S = 170       # under genviab's AXON_INTENT_TIMEOUT_MS (180 s)


def sha256(b):
    return hashlib.sha256(b).hexdigest()


def driver_sha256():
    with open(os.path.abspath(__file__), "rb") as fh:
        return sha256(fh.read())


def provider_config(name):
    """(base_url, env_file, token_key) or a refusal reason."""
    p = PROVIDERS.get(name)
    if p is None:
        return None, f"provider {name!r} is not one of {sorted(PROVIDERS)}"
    base, env_file = p["base_url"], p["env_file"]
    if E.get("GENVIAB_SELFTEST") == "1":
        base = E.get("GENVIAB_HTTP_BASE_URL") or base
        env_file = E.get("GENVIAB_HTTP_ENV_FILE") or env_file
    return (base.rstrip("/"), env_file, E.get("GENVIAB_CERE_TOKEN_KEY") or p["token_key"]), None


def read_token(env_file, key):
    """The value of `key` in a KEY=VALUE env file, else None. No other key is read."""
    try:
        with open(env_file) as fh:
            for line in fh:
                k, sep, v = line.strip().partition("=")
                if sep and k.strip() == key:
                    v = v.strip().strip('"').strip("'")
                    return v or None
    except OSError:
        return None
    return None


def cache_key(prompt_sha, series, temperature, seed, max_tokens, drv):
    return sha256(json.dumps([prompt_sha, series, temperature, seed, max_tokens, drv]).encode())


def entry_path(root, key):
    return os.path.join(root, key[:2], key + ".json")


def load_entry(root, key):
    if not root:
        return None
    try:
        with open(entry_path(root, key)) as fh:
            return json.load(fh)
    except (OSError, ValueError):
        return None


def store_entry(root, key, entry):
    if not root:
        return
    p = entry_path(root, key)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    tmp = f"{p}.{os.getpid()}.tmp"
    with open(tmp, "w") as fh:
        json.dump(entry, fh)
    os.replace(tmp, p)


def take_replay(state, key):
    """Should this cache hit be re-queried? The first REPLAY_MIN hits of the run,
    then a seeded REPLAY_FRACTION of them (counted under a lock in `state`).
    GENVIAB_REPLAY_MIN / GENVIAB_REPLAY_FRACTION are honoured only under
    GENVIAB_SELFTEST=1: an operator's shell never turns the D4 replay off."""
    st = E.get("GENVIAB_SELFTEST") == "1"
    frac = float(E.get("GENVIAB_REPLAY_FRACTION", REPLAY_FRACTION)) if st else REPLAY_FRACTION
    least = int(E.get("GENVIAB_REPLAY_MIN", REPLAY_MIN)) if st else REPLAY_MIN
    seed = E.get("GENVIAB_REPLAY_SEED", "0")
    sampled = int(sha256(f"{seed}:{key}".encode())[:8], 16) / 0xFFFFFFFF < frac
    if not state:
        return sampled
    os.makedirs(state, exist_ok=True)
    with open(os.path.join(state, "replays.lock"), "a+") as fh:
        fcntl.flock(fh, fcntl.LOCK_EX)
        fh.seek(0)
        n = len(fh.read().split())
        take = sampled or n < least
        if take:
            fh.write(key[:16] + "\n")
        return take


def call(base, token, body, timeout=TIMEOUT_S):
    """POST /chat/completions: (status, parsed json or None, error text or None)."""
    req = urllib.request.Request(base + "/chat/completions", data=json.dumps(body).encode(),
                                 headers={"Authorization": "Bearer " + token,
                                          "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read()), None
    except urllib.error.HTTPError as e:
        return e.code, None, f"HTTP {e.code}: {e.read()[:300].decode('utf-8', 'replace')}"
    except (urllib.error.URLError, OSError, ValueError) as e:
        return None, None, f"{type(e).__name__}: {e}"[:300]


def outcome(doc, max_tokens):
    """The parts of a completion this driver keeps."""
    ch = (doc.get("choices") or [{}])[0]
    usage = doc.get("usage") or {}
    text = (ch.get("message") or {}).get("content") or ""
    out_tok = usage.get("completion_tokens") or 0
    cere = doc.get("cere") or {}
    return {"reply": text, "reply_sha256": sha256(text.encode()),
            "finish_reason": ch.get("finish_reason"), "served_model": doc.get("model"),
            "input_tokens": usage.get("prompt_tokens") or 0, "output_tokens": out_tok,
            "cached_tokens": (usage.get("prompt_tokens_details") or {}).get("cached_tokens") or 0,
            "server": {k: cere.get(k) for k in ("recipe_id", "profile_id", "lock_sha256",
                                                "config_hash") if k in cere} or None,
            "max_tokens": max_tokens}


def main():
    raw = sys.stdin.buffer.read()
    prompt = raw.decode("utf-8")
    provider = E.get("GENVIAB_PROVIDER", "cere")
    model = E.get("GENVIAB_MODEL") or ""
    max_tokens = int(E.get("GENVIAB_MAX_TOKENS", DEFAULT_MAX_TOKENS))
    temperature = float(E.get("GENVIAB_TEMPERATURE", DEFAULT_TEMPERATURE))
    seed = int(E.get("GENVIAB_SEED", "0"))
    series = E.get("GENVIAB_SERIES", "")
    state = E.get("GENVIAB_RUN_STATE") or None
    prompt_sha = sha256(raw)
    drv = driver_sha256()
    key = cache_key(prompt_sha, series, temperature, seed, max_tokens, drv)
    cfg, why = provider_config(provider)
    row = {
        "transport": "http", "provider": provider, "model": [], "requested_model": model,
        "effort": None, "micode_version": None, "task": E.get("GENVIAB_TASK"),
        "arm": E.get("GENVIAB_ARM"), "prompt_bytes": len(raw), "prompt_sha256": prompt_sha,
        "cost_usd": None, "duration_api_ms": 0, "input_tokens": 0, "output_tokens": 0,
        "cache_read": 0, "cache_create": 0, "turns": 0, "exit": 0, "session_errors": [],
        "retries": [], "reply_bytes": 0,
        "base_url_host": urllib.parse.urlsplit(cfg[0]).netloc if cfg else None,
        "token_key": cfg[2] if cfg else None, "temperature": temperature, "seed": seed,
        "max_tokens": max_tokens, "series": series, "finish_reason": None, "cached_tokens": 0,
        "reply_sha256": None, "cache_hit": False, "shared": False, "replayed": False,
        "replay_mismatch": None, "truncated": False, "cache_key": key, "server": None,
    }
    disabled = bool(state) and os.path.exists(os.path.join(state, "cache-disabled"))
    cache_dir = None if disabled else (E.get("GENVIAB_CACHE_DIR") or None)
    share_dir = E.get("GENVIAB_SHARE_DIR") or None
    code, got, live = 0, None, False
    token = None
    if why is None:
        token = read_token(cfg[1], cfg[2])
        if token is None:
            why = f"no {cfg[2]} in the provider's env file (no fallback to another key)"
    if why is not None:
        code = 4
    else:
        shared = load_entry(share_dir, key)
        hit = None if shared else load_entry(cache_dir, key)
        if shared:
            got, row["shared"] = shared, True
        elif hit:
            got, row["cache_hit"] = hit, True
            if take_replay(state, key):
                row["replayed"] = True
                live = True
        else:
            live = True
        # GENVIAB_DEADLINE (epoch s, ae.py's window end): no request starts at
        # or after it, and none outlives it. A replay past it is not made (the
        # hit stands); a miss past it is infra, and ae.py marks the run INCOMPLETE.
        deadline = float(E["GENVIAB_DEADLINE"]) if E.get("GENVIAB_DEADLINE") else None
        left = None if deadline is None else deadline - time.time()
        if live and left is not None and left < 1:
            live = False
            if row["replayed"]:
                row["replayed"] = False
            else:
                why, code = "the run's window ended (GENVIAB_DEADLINE): no request made", 1
        if live:
            body = {"model": model, "messages": [{"role": "user", "content": prompt}],
                    "max_tokens": max_tokens, "temperature": temperature, "seed": seed}
            began = time.time()
            status, doc, err = call(cfg[0], token, body,
                                    TIMEOUT_S if left is None else min(TIMEOUT_S, left))
            row["duration_api_ms"] = int((time.time() - began) * 1000)
            row["turns"] = 1
            if err or doc is None:
                if row["replayed"]:
                    # the replay could not be made: the hit stands, nothing compared
                    row["replayed"] = False
                    row["retries"] = [(err or f"HTTP {status}: not JSON")[:400]]
                else:
                    row["session_errors"] = [(err or f"HTTP {status}: not JSON")[:400]]
                    why, code = f"request failed: {row['session_errors'][0][:200]}", 1
            else:
                fresh = outcome(doc, max_tokens)
                if row["replayed"]:
                    row["replay_mismatch"] = fresh["reply_sha256"] != got["reply_sha256"]
                    if row["replay_mismatch"]:
                        os.makedirs(state or ".", exist_ok=True)
                        with open(os.path.join(state or ".", "cache-disabled"), "a") as fh:
                            fh.write(json.dumps({"key": key, "cached": got["reply_sha256"],
                                                 "replayed": fresh["reply_sha256"]}) + "\n")
                        got = fresh   # the live reply; the hit stays counted
                else:
                    got = fresh
                    if fresh["finish_reason"] == "stop" or (
                            fresh["finish_reason"] == "length"
                            and fresh["output_tokens"] >= max_tokens):
                        store_entry(cache_dir, key, fresh)
                        store_entry(share_dir, key, fresh)
    if got is not None:
        row.update({k: got[k] for k in ("reply_sha256", "finish_reason", "input_tokens",
                                        "output_tokens", "cached_tokens", "server")})
        row["model"] = [got["served_model"]] if got.get("served_model") else []
        row["reply_bytes"] = len(got["reply"].encode())
        if got["finish_reason"] == "length":
            if got["output_tokens"] >= max_tokens:
                row["truncated"] = True
            else:
                why, code = (f"transport cut: finish_reason length at {got['output_tokens']} "
                             f"output tokens, below the declared max_tokens {max_tokens}"), 1
        elif got["finish_reason"] != "stop":
            why, code = f"finish_reason {got['finish_reason']!r}", 1
        if why is None and not got["reply"].strip():
            why, code = "the reply is empty", 1
    row["exit"] = code
    row["error"] = why
    with open(E.get("GENVIAB_MODEL_LOG", "/dev/null"), "a") as fh:
        fh.write(json.dumps(row) + "\n")
    if why:
        sys.stderr.write("driver: " + why + "\n")
        return code
    sys.stdout.write(got["reply"])
    sys.stderr.write(f"axon-intent-tokens: {got['input_tokens'] + got['output_tokens']}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
