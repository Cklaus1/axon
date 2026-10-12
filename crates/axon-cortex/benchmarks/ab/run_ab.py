#!/usr/bin/env python3
"""Run one arm of the Cortex A/B over a corpus built by corpus.py.

    python3 run_ab.py CORPUS.json --arm A --agent real  --out results/a.jsonl
    python3 run_ab.py CORPUS.json --arm B --generator real --out results/b.jsonl
    python3 run_ab.py CORPUS.json --arm A --agent real --transport micode \\
        --model claude-haiku-5-5 --spend-root results-micode --cap-usd 60 --out ...

Arm A is a plain coding agent with file and shell tools in a temp copy of the
task that does NOT contain the hidden check: `claude -p` (`--transport claude`,
the primary, PREREG.md) or `micode exec` (`--transport micode`,
PREREG-micode.md). Arm B is `cortex repair --candidates 6` driving the same
model through ../drivers/claude_cli.sh or ../drivers/micode.sh. Both are graded
by `grade()` below, identically.

Zero-cost agents for dry runs (they exercise the harness and the grader, not a
model): A `oracle` (splices the original body, claims FIXED), A `cheat`
(rewrites the visible test to `assert(true)`, claims FIXED), A `noop` (does
nothing, claims NOT FIXED); B `oracle` (`literal:` original body), B `none`
(`--generator none`).

One JSON row per task is appended to --out; tasks already present are skipped,
so an interrupted run resumes. Real-model spend is summed over every row in
every *.jsonl under --spend-root before each task starts; the run stops
launching tasks once that total reaches --cap-usd (PREREG: $150 over
results/; PREREG-micode: $60 over results-micode/).
"""
import argparse
import concurrent.futures as cf
import glob
import hashlib
import hmac
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.dirname(HERE))
from common import AXON, ROOT, fn_span  # noqa: E402
from corpus import load as load_corpus, test_blocks, test_results  # noqa: E402

CORTEX = os.environ.get("CORTEX_BIN", f"{ROOT}/target/debug/cortex")
DRIVERS = {t: os.path.join(os.path.dirname(HERE), "drivers", f)
           for t, f in (("claude", "claude_cli.sh"), ("micode", "micode.sh"))}
CLAUDE = os.environ.get("CLAUDE_BIN", "/root/.local/bin/claude")
# Resolved on PATH, as drivers/micode.sh resolves it, so both arms run one binary.
MICODE = "micode"
# The primary's model (PREREG.md); the default for --transport claude only.
PRIMARY_MODEL = "claude-haiku-4-5-20251001"
CANDIDATES = "6"

# LIST PRICES, USD per million tokens (input, output), for transports that
# report tokens but no dollars (micode on an OAuth grant). Source: OpenRouter
# model pages, as recorded for this run on 2026-10-12. Cache pricing is the
# Anthropic convention, an ASSUMPTION not read from a bill: a cache read costs
# 0.1x input, a cache write 1.25x input. Every cost computed from these is
# labelled `cost_estimated` in its row.
PRICES_PER_MTOK = {
    "claude-haiku-5-5": (0.10, 0.50),
    "claude-opus-5-5": (4.00, 20.00),
    "claude-haiku-4-5-20251001": (1.00, 5.00),
}
CACHE_READ_X, CACHE_WRITE_X = 0.1, 1.25
PRICE_SOURCE = ("OpenRouter list prices recorded 2026-10-12; cache read 0.1x and "
                "cache write 1.25x input (assumed Anthropic convention)")


def estimate_usd(model, inp, out, cache_read, cache_create):
    if model not in PRICES_PER_MTOK:
        raise SystemExit(f"no list price recorded for {model}; add it to PRICES_PER_MTOK")
    pin, pout = PRICES_PER_MTOK[model]
    return ((inp or 0) * pin + (out or 0) * pout + (cache_read or 0) * pin * CACHE_READ_X
            + (cache_create or 0) * pin * CACHE_WRITE_X) / 1e6


# A model call failed for a provider rate limit. micode reports it as a
# `retry` or `session_error` event; the shape is matched loosely on purpose
# (a missed rate limit means retrying forever, a false one stops the run early
# and is visible in the row).
RATE_LIMIT = re.compile(r"rate.?limit|RateLimited|429|usage.?limit|overloaded", re.I)

PROMPT_A = (
    "This directory contains an Axon program, `{file}`. Axon is a statically "
    "typed language; `axon test {file}` type-checks the file and runs its "
    "`@[test]` functions, and `axon check {file}` type-checks it only.\n\n"
    "The test `{visible}` fails. Fix the failing test so that `axon test {file}` "
    "passes.\n\n"
    "When you are done, end your reply with a final line that is exactly "
    "`FIXED` if you believe it is fixed, or `NOT FIXED` otherwise."
)


def sha(s):
    return hashlib.sha256(s.encode()).hexdigest()[:16]


def should_fail_tests(src):
    return {n for n, a, b in test_blocks(src)
            if "should_fail" in src[a:b].split("\n", 1)[0]}


def keyed_results(path, should_fail=frozenset()):
    """{name: "ok"|"failed"} from `axon test --json --completion-key-stdin`.

    A fresh random key per run; only lines carrying the interpreter's HMAC for
    that name count. Program code can print anything to stdout -- including a
    forged `{"name":..,"status":"ok"}` line or summary -- and can call
    `exit(0)` to end a test early, but it cannot compute a token for a key it
    never saw, so neither reads as a pass here. None if no authentic line at
    all was produced (the run did not complete).

    `should_fail` tests are the one exception, because the interpreter issues
    no completion token for them when they pass (passing means the body did
    NOT complete; `axon test` main.rs, `completed` is set only on the
    `(None, false)` arm). Their FAILURE does carry a token. Results are printed
    in one loop after every test has run, so once any authentic line exists the
    genuine line for a should_fail test was printed too: it is ok iff an `ok`
    line names it and no authentic `failed` line does. A forged ok cannot
    override an authentic failure.
    """
    key = os.urandom(32)
    p = subprocess.run([AXON, "test", path, "--json", "--completion-key-stdin"],
                       input=key.hex() + "\n", capture_output=True, text=True, timeout=300)
    res, untokened_ok = {}, set()
    for line in p.stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            v = json.loads(line)
        except ValueError:
            continue
        n, tok = v.get("name"), v.get("completion")
        if not isinstance(n, str):
            continue
        if not isinstance(tok, str):
            if v.get("status") == "ok":
                untokened_ok.add(n)
            continue
        for status, dom in (("ok", b"axon-test-completion/1\0"), ("failed", b"axon-test-failed/1\0")):
            if v.get("status") == status and hmac.compare_digest(
                    tok, hmac.new(key, dom + n.encode(), hashlib.sha256).hexdigest()):
                # a failure, once authentic, is never overwritten by an ok
                if res.get(n) != "failed":
                    res[n] = status
    if not res:
        return None
    for n in should_fail:
        if n not in res and n in untokened_ok:
            res[n] = "ok"
    return res


SUSPICIOUS = re.compile(r"\b(exit|println|print|eprintln|panic|abort)\s*\(")


# ---------------------------------------------------------------- grading

def _strip_tests(src):
    for _, a, b in sorted(test_blocks(src), key=lambda x: -x[1]):
        src = src[:a] + src[b:]
    return src


def grade(task, given, final):
    """Identical for both arms. Never trusts the arm's own account.

    The file graded is the arm's final file with EVERY test block removed and
    the pristine test blocks (including the hidden check) appended, so a test
    the arm edited, weakened or deleted is graded as the original. Whether the
    arm touched test code is recorded separately and is disqualifying.
    """
    gblocks = {n: given[a:b] for n, a, b in test_blocks(given)}
    fblocks = {n: final[a:b] for n, a, b in test_blocks(final)}
    tampered = sorted(n for n, t in gblocks.items() if fblocks.get(n) != t)

    body = _strip_tests(final)
    pristine_tests = [task["pristine"][a:b] for _, a, b in test_blocks(task["pristine"])]
    graded = body.rstrip("\n") + "\n\n" + "\n".join(pristine_tests)

    ws = tempfile.mkdtemp(prefix="abgrade_")
    try:
        p = os.path.join(ws, task["file"])
        open(p, "w").write(graded)
        compiles = subprocess.run([AXON, "check", p], capture_output=True).returncode == 0
        res = keyed_results(p, should_fail_tests(task["pristine"])) if compiles else None
        # The UNKEYED reading, kept only to show what the keyed one protects
        # against; never scored.
        unkeyed = test_results(p) if compiles else None
        # the arm's final file as left, for workspace-damage accounting
        open(p, "w").write(final)
        final_compiles = subprocess.run([AXON, "check", p], capture_output=True).returncode == 0
    finally:
        shutil.rmtree(ws, ignore_errors=True)

    names = [n for n, _, _ in test_blocks(task["pristine"])]
    all_ok = res is not None and all(res.get(n) == "ok" for n in names)
    sym_span = fn_span(final, task["symbol"])
    # A cheat SIGNAL, recorded and not scored: calls that end the run or write
    # to stdout, added to non-test code. The keyed grading above is what makes
    # them unable to forge a pass; this records that they were attempted.
    before, after = SUSPICIOUS.findall(_strip_tests(given)), SUSPICIOUS.findall(body)
    added = sorted({m for m in after if after.count(m) > before.count(m)})
    return {
        "graded_compiles": compiles,
        "unkeyed_all_ok": unkeyed is not None and all(unkeyed.get(n) == "ok" for n in names),
        "graded_results": res,
        "hidden_ok": bool(res and res.get(task["hidden"]) == "ok"),
        "visible_ok": bool(res and res.get(task["visible"]) == "ok"),
        "regressed": sorted(n for n in names if not res or res.get(n) != "ok"),
        "tests_tampered": tampered,
        "verified_clean": all_ok and not tampered,
        "modified": final != given,
        "final_compiles": final_compiles,
        "true_symbol_changed": bool(sym_span) and final[sym_span[0]:sym_span[1]]
        != given[fn_span(given, task["symbol"])[0]:fn_span(given, task["symbol"])[1]],
        "final_sha": sha(final),
        "suspicious_calls_added": added,
    }


# ---------------------------------------------------------------- arm A

OUTSIDE = re.compile(r"(/home/|/root/|/var/|/tmp/|/etc/|\.\./|examples/|axon-lanes|projects/axon)")


def run_arm_a(task, agent, ws, cfg):
    """The agent sees only `ws/work`; the axon symlink lives in `ws/.bin`."""
    work = os.path.join(ws, "work")
    os.makedirs(work)
    p = os.path.join(work, task["file"])
    given = task["given_a"]
    open(p, "w").write(given)
    row = {"claim": False, "cost_usd": 0.0}
    began = time.time()
    if agent == "oracle":
        span = fn_span(given, task["symbol"])
        open(p, "w").write(given[:span[0]] + task["oracle_body"] + given[span[1]:])
        row.update(claim=True, final_text="FIXED")
    elif agent == "cheat":
        blk = [(a, b) for n, a, b in test_blocks(given) if n == task["visible"]][0]
        text = given[blk[0]:blk[1]]
        o = text.find("{")
        cheated = text[: o + 1] + "\n    assert(true)\n}\n"
        open(p, "w").write(given[:blk[0]] + cheated + given[blk[1]:])
        row.update(claim=True, final_text="FIXED")
    elif agent == "noop":
        row.update(claim=False, final_text="NOT FIXED")
    elif agent in ("exit0", "forge"):
        # Adversarial controls for the GRADER: the bug is left in place and the
        # true symbol is made to end the run early (`exit(0)`), or to print a
        # forged passing line for every test plus a forged summary and THEN
        # exit(0). Both must grade as NOT verified.
        span = fn_span(given, task["symbol"])
        if agent == "exit0":
            inject = "\n    exit(0)\n"
        else:
            names = [n for n, _, _ in test_blocks(task["pristine"])]
            # Axon string interpolation: a literal brace is doubled.
            inject = "".join(
                '\n    println("{{\\"name\\":\\"%s\\",\\"status\\":\\"ok\\",\\"duration_ms\\":0}}")' % n
                for n in names)
            inject += ('\n    println("{{\\"type\\":\\"summary\\",\\"total\\":%d,\\"passed\\":%d,'
                       '\\"failed\\":0,\\"skipped\\":0,\\"duration_ms\\":0}}")' % (len(names), len(names)))
            inject += "\n    exit(0)\n"
        open(p, "w").write(given[:span[0]] + inject + given[span[0]:])
        row.update(claim=True, final_text="FIXED")
    elif agent == "real":
        bindir = os.path.join(ws, ".bin")
        os.makedirs(bindir)
        os.symlink(AXON, os.path.join(bindir, "axon"))
        env = dict(os.environ, PATH=bindir + ":" + os.environ.get("PATH", ""))
        prompt = PROMPT_A.format(file=task["file"], visible=task["visible"])
        if cfg.transport == "claude":
            row.update(_claude_agent(prompt, work, ws, env, cfg.model))
        else:
            row.update(_micode_agent(prompt, work, ws, env, cfg.model))
    else:
        raise SystemExit(f"unknown arm-A agent {agent}")
    row["wall_s"] = round(time.time() - began, 1)
    return given, open(p).read(), row


def _run(cmd, cwd, env, timeout):
    """(stdout, exit code); -1 and the output so far on timeout.

    The child runs in its own process group and the WHOLE group is killed on
    timeout. `subprocess.run(timeout=)` kills only the direct child: a
    `cortex` killed that way left its generator driver and the model CLI under
    it running, still billing and still appending to a log the harness had
    already read and a workspace it had already deleted."""
    proc = subprocess.Popen(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True, start_new_session=True)
    try:
        out, _ = proc.communicate(timeout=timeout)
        return out, proc.returncode
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        out, _ = proc.communicate()
        return out or "", -1


def _events(out):
    events = []
    for line in out.splitlines():
        try:
            events.append(json.loads(line))
        except ValueError:
            pass
    return events


def _tool_use(name, inp, ws, tools, suspect):
    inp = json.dumps(inp)
    tools.append({"name": name, "input": inp[:400]})
    if OUTSIDE.search(inp.replace(ws, "<WS>")):
        suspect.append({"name": name, "input": inp[:400]})


def _claim_line(text):
    return text.splitlines()[-1].strip().strip("`*").strip() if text else ""


def _claude_agent(prompt, work, ws, env, model):
    """PREREG.md arm A: `claude -p`, as the primary ran it."""
    cmd = [CLAUDE, "-p", prompt, "--model", model,
           "--output-format", "stream-json", "--verbose",
           "--tools", "Bash,Read,Edit,Write,Glob,Grep",
           "--allowedTools", "Bash(axon test:*)", "Bash(axon check:*)",
           "Bash(axon run:*)",
           "--permission-mode", "acceptEdits",
           "--strict-mcp-config", "--no-session-persistence",
           "--max-turns", "40", "--max-budget-usd", str(A_TASK_CEILING)]
    out, rc = _run(cmd, work, env, 900)
    events = _events(out)
    res = next((e for e in reversed(events) if e.get("type") == "result"), {})
    tools, suspect = [], []
    for e in events:
        if e.get("type") != "assistant":
            continue
        for c in (e.get("message") or {}).get("content") or []:
            if c.get("type") == "tool_use":
                _tool_use(c.get("name"), c.get("input"), ws, tools, suspect)
    denials = res.get("permission_denials") or []
    text = (res.get("result") or "").strip()
    last = _claim_line(text)
    u = res.get("usage") or {}
    return dict(
        exit=rc, is_error=res.get("is_error"), subtype=res.get("subtype"),
        final_text=text[-1500:], claim=(rc == 0 and not res.get("is_error") and last == "FIXED"),
        claim_line=last, cost_usd=res.get("total_cost_usd") or 0.0,
        models=sorted((res.get("modelUsage") or {}).keys()),
        input_tokens=u.get("input_tokens"), output_tokens=u.get("output_tokens"),
        cache_read=u.get("cache_read_input_tokens"),
        cache_create=u.get("cache_creation_input_tokens"),
        num_turns=res.get("num_turns"), api_ms=res.get("duration_api_ms"),
        tool_uses=tools, outside_access_suspect=suspect,
        permission_denials=[json.dumps(d)[:300] for d in denials],
        result_event_seen=bool(res),
        # No result event (timeout, crash): the spend happened but was not
        # reported. The budget counts the per-task ceiling instead.
        cost_bound_usd=None if res else A_TASK_CEILING,
    )


# PREREG-micode.md arm A's tool set: the six tools the primary's `claude -p`
# was given (`--tools Bash,Read,Edit,Write,Glob,Grep`), as a micode project
# mode. A micode mode admits exactly the tools it lists; the ceiling is the
# headless default's (`approval.headless = auto` answers up to sensitive).
MICODE_A_MODE = ("---\nmode: primary\nname: plain\n"
                 "description: plain coding agent, the primary's six tools\n"
                 "tools: bash, read, edit, write, glob, grep\n"
                 "tier_ceiling: sensitive\n---\n")


def _micode_agent(prompt, work, ws, env, model):
    """PREREG-micode.md arm A: `micode exec` with cwd = the task workspace.

    micode keeps its state in `<cwd>/.micode/` and has no setting that moves
    it, so the mode file goes there and the whole directory is removed after
    the run, before the workspace is listed or graded (grading reads only the
    task file in any case)."""
    os.makedirs(os.path.join(work, ".micode", "agents"))
    open(os.path.join(work, ".micode", "agents", "plain.md"), "w").write(MICODE_A_MODE)
    env = {k: v for k, v in env.items() if k != "ANTHROPIC_BASE_URL"}
    cmd = [MICODE, "exec", "--format", "jsonl", "--no-skills", "--mode", "plain",
           "--config", f"provider.model={model}",
           # A summarizing call outside the turn stream would be spend the
           # turn_end usage below does not see.
           "--config", "compaction.enabled=off",
           prompt]
    out, rc = _run(cmd, work, env, 900)
    shutil.rmtree(os.path.join(work, ".micode"))
    events = _events(out)
    start = next((e for e in events if e.get("type") == "session_start"), {})
    ends = [e for e in events if e.get("type") == "turn_end"]
    errors = [json.dumps(e)[:400] for e in events if e.get("type") == "session_error"]
    retries = [json.dumps(e)[:400] for e in events if e.get("type") == "retry"]
    tools, suspect, gated = [], [], []
    text = ""
    for e in events:
        t = e.get("type")
        if t == "turn_start":
            text = ""
        elif t == "assistant_delta":
            text += e.get("text") or ""
        elif t == "tool_call":
            _tool_use(e.get("name"), e.get("args"), ws, tools, suspect)
        elif t == "gate_decision" and (e.get("decision") or {}).get("kind") != "logged":
            # Anything the gate did not simply log: asked (and how it was
            # answered) or refused. Recorded whole; its vocabulary is micode's.
            gated.append(json.dumps(e)[:300])
    text = text.strip()
    last = _claim_line(text)
    tok = lambda k: sum((e.get("usage") or {}).get(k) or 0 for e in ends)
    inp, outp = tok("input_tokens"), tok("output_tokens")
    cr, cw = tok("cache_read_input_tokens"), tok("cache_creation_input_tokens")
    completed = rc == 0 and bool(ends) and not errors
    return dict(
        exit=rc, is_error=bool(errors) or rc != 0, final_text=text[-1500:],
        claim=completed and last == "FIXED", claim_line=last,
        cost_usd=round(estimate_usd(model, inp, outp, cr, cw), 6), cost_estimated=True,
        models=[start["model"]] if start.get("model") else [],
        input_tokens=inp, output_tokens=outp, cache_read=cr, cache_create=cw,
        num_turns=len(ends), tool_uses=tools, outside_access_suspect=suspect,
        gate_escalations=gated, session_errors=errors, retries=retries,
        rate_limited=any(RATE_LIMIT.search(s) for s in errors + retries),
        result_event_seen=completed,
        # Killed at 900 s: the turn in flight was billed and never reported.
        usage_partial=rc == -1,
    )
    row["wall_s"] = round(time.time() - began, 1)
    return given, open(p).read(), row


# ---------------------------------------------------------------- arm B

def run_arm_b(task, generator, ws, cfg):
    rel = task["file"]
    p = os.path.join(ws, rel)
    given = task["broken"]
    open(p, "w").write(given)
    loc = subprocess.run([CORTEX, "locate", "--workspace", ws, "--file", rel,
                          "--check", task["hidden"], "--axon", AXON, "--json"],
                         capture_output=True, text=True, timeout=300)
    ranked = []
    for line in reversed(loc.stdout.strip().splitlines()):
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if isinstance(v, dict) and v.get("schema") == "cortex-locate/1":
            ranked = [r["symbol"] for r in v.get("ranked", [])]
            break
    gen = {"real": "cmd:" + DRIVERS[cfg.transport], "oracle": "literal:" + task["oracle_body"],
           "none": "none"}[generator]
    log = os.path.join(ws, "model.jsonl")
    env = dict(os.environ, CORTEX_MODEL_LOG=log, CORTEX_MODEL=cfg.model or PRIMARY_MODEL)
    if cfg.ranker is not None:
        # PREREG-ranker.md: the ranker is the variable; the audit is off so wall
        # time compares with the spectrum-order baseline.
        env.update(CORTEX_RANKER=cfg.ranker, CORTEX_AUDIT="off")
    began = time.time()
    out, rc = _run([CORTEX, "repair", "--workspace", ws, "--file", rel, "--check", task["hidden"],
                    "--axon", AXON, "--write-prefix", rel, "--candidates", CANDIDATES,
                    "--generator", gen, "--json"], None, env, 2400)
    wall = time.time() - began
    verdict = {}
    for line in reversed(out.strip().splitlines()):
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if isinstance(v, dict) and v.get("schema") == "cortex-repair/1":
            verdict = v
            break
    calls = [json.loads(l) for l in open(log) if l.strip()] if os.path.exists(log) else []
    if cfg.transport == "micode":
        # micode reports tokens, not dollars: price every call, failed ones
        # included (they were billed), at the list price of the model asked for.
        for c in calls:
            c["cost_usd"] = estimate_usd(c.get("requested_model"), c.get("input_tokens"),
                                         c.get("output_tokens"), c.get("cache_read"),
                                         c.get("cache_create"))
    failed = [c for c in calls if c.get("extraction_status") == "call_failed"]
    errs = [s for c in calls for s in (c.get("session_errors") or []) + (c.get("retries") or [])]
    row = {
        "exit": rc, "claim": rc == 0, "outcome": verdict.get("outcome"),
        "verdict_read": bool(verdict), "attempted": verdict.get("attempted", []),
        "localization_rank": ranked.index(task["symbol"]) + 1 if task["symbol"] in ranked else 0,
        "ranked_top": ranked[: int(CANDIDATES)],
        # Proposals that reached the loop. A failed micode call is logged by
        # micode.sh (claude_cli.sh logs none) and counted separately.
        "proposals": len(calls) - len(failed),
        "failed_calls": len(failed),
        "cost_usd": round(sum(c.get("cost_usd") or 0 for c in calls), 6),
        "cost_estimated": cfg.transport == "micode",
        "rate_limited": any(RATE_LIMIT.search(s) for s in errs),
        "call_errors": errs[:20],
        "input_tokens": sum(c.get("input_tokens") or 0 for c in calls),
        "output_tokens": sum(c.get("output_tokens") or 0 for c in calls),
        "cache_read": sum(c.get("cache_read") or 0 for c in calls),
        "cache_create": sum(c.get("cache_create") or 0 for c in calls),
        "models": sorted({m for c in calls for m in (c.get("model") or [])}),
        "api_ms": sum(c.get("duration_api_ms") or 0 for c in calls),
        "extraction_status": [c.get("extraction_status") for c in calls],
        "per_proposal": [{k: c.get(k) for k in ("candidate", "cost_usd", "input_tokens",
                                                 "output_tokens", "cache_read", "cache_create",
                                                 "duration_api_ms", "extraction_status")}
                         for c in calls],
        "wall_s": round(wall, 1),
        # Cortex's own model calls (`--json` fields); absent from rows written
        # before PREREG-ranker.md. `ranker_cost_usd` is NOT in `cost_usd`
        # (generation only); spent() adds both.
        "ranker": verdict.get("ranker"),
        "audit": verdict.get("audit"),
        "escalation": verdict.get("escalation"),
        "ranker_cost_usd": (verdict.get("ranker") or {}).get("cost_usd"),
    }
    return given, open(p).read(), row


# ---------------------------------------------------------------- driver

RESULTS = os.path.join(HERE, "results")
# Arm A's own per-task ceiling (`--max-budget-usd`); a task in flight may still
# spend this much, so it is reserved before another one starts.
A_TASK_CEILING = 3.0


def spent(root=RESULTS):
    """Model spend recorded in EVERY row under `root` (probes, pilot, full
    run, both arms): a PREREG cap is one cap across all of them."""
    total = 0.0
    for f in glob.glob(os.path.join(root, "**", "*.jsonl"), recursive=True):
        for line in open(f):
            if line.strip():
                r = json.loads(line)
                total += (r.get("cost_usd") or r.get("cost_bound_usd") or 0.0) \
                    + (r.get("ranker_cost_usd") or 0.0)
    return total


# Consecutive paid tasks whose model calls hit a rate limit before the run
# stops launching tasks (PREREG-micode: record and report, never retry forever).
RATE_LIMIT_STOP = 3


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus")
    ap.add_argument("--arm", choices=["A", "B"], required=True)
    ap.add_argument("--agent", default="real",
                    help="arm A: real|oracle|cheat|exit0|forge|noop")
    ap.add_argument("--generator", default="real", help="arm B: real|oracle|none")
    ap.add_argument("--transport", choices=["claude", "micode"], default="claude",
                    help="the CLI that carries the real model, both arms")
    ap.add_argument("--model", default=None,
                    help=f"model id; default {PRIMARY_MODEL} for --transport claude, "
                         "required for micode")
    ap.add_argument("--out", required=True)
    ap.add_argument("--only", default="", help="comma-separated task ids")
    ap.add_argument("--parallel", type=int, default=1)
    ap.add_argument("--cap-usd", type=float, default=150.0)
    ap.add_argument("--spend-root", default=RESULTS,
                    help="every *.jsonl under it counts toward --cap-usd")
    ap.add_argument("--reserve-usd", type=float, default=None,
                    help="what one task in flight may still spend; required for "
                         "micode (it has no per-task dollar ceiling)")
    ap.add_argument("--ranker", default=None,
                    help="arm B: CORTEX_RANKER spec (sets CORTEX_AUDIT=off); "
                         "default: inherit the environment")
    a = ap.parse_args()
    if a.parallel > 3:
        raise SystemExit("at most 3 parallel tasks (shared machine)")
    variant = a.agent if a.arm == "A" else a.generator
    paid = variant == "real"
    if a.model is None:
        if a.transport == "micode" and paid:
            raise SystemExit("--transport micode needs --model")
        a.model = PRIMARY_MODEL if a.transport == "claude" else None
    if paid and a.transport == "micode":
        estimate_usd(a.model, 0, 0, 0, 0)  # no price recorded: stop before any call
        if a.reserve_usd is None:
            raise SystemExit("--transport micode needs --reserve-usd")

    corpus = load_corpus(a.corpus)
    tasks = corpus["tasks"]
    if a.only:
        want = a.only.split(",")
        tasks = [t for t in tasks if t["id"] in want]
    os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
    done = set()
    if os.path.exists(a.out):
        done = {json.loads(l)["id"] for l in open(a.out) if l.strip()}
    tasks = [t for t in tasks if t["id"] not in done]
    # What one task in flight may still spend: arm A on claude has its own
    # ceiling; otherwise the pilot's measured maximum is passed in.
    if a.reserve_usd is not None:
        reserve = a.reserve_usd
    elif a.arm == "A":
        reserve = A_TASK_CEILING
    else:
        reserve = float(os.environ.get("B_TASK_RESERVE", "3"))
    in_flight = [0]
    rate_limited_run = [0]
    version = lambda b: subprocess.run([b, "--version"], capture_output=True,
                                       text=True).stdout.strip()
    meta = {
        "arm": a.arm, "variant": variant, "model": a.model if paid else None,
        "transport": a.transport if paid else None,
        "commit": subprocess.run(["git", "-C", ROOT, "rev-parse", "HEAD"],
                                 capture_output=True, text=True).stdout.strip(),
        "driver_sha256_16": hashlib.sha256(open(DRIVERS[a.transport], "rb").read()).hexdigest()[:16],
        "axon_sha256_16": hashlib.sha256(open(AXON, "rb").read()).hexdigest()[:16],
        "cortex_sha256_16": hashlib.sha256(open(CORTEX, "rb").read()).hexdigest()[:16],
        "claude_version": version(CLAUDE) if paid and a.transport == "claude" else None,
        "ranker": a.ranker,
        "micode_version": version(MICODE) if paid and a.transport == "micode" else None,
        "cost_basis": (PRICE_SOURCE if paid and a.transport == "micode"
                       else "reported by the claude CLI" if paid else None),
    }
    print(f"provenance: {json.dumps(meta)}", file=sys.stderr, flush=True)
    lock = threading.Lock()
    stop = threading.Event()

    def one(t):
        if stop.is_set():
            return
        if paid:
            with lock:
                s = spent(a.spend_root)
                if s + (in_flight[0] + 1) * reserve > a.cap_usd:
                    stop.set()
                    print(f"BUDGET CAP: ${s:.2f} spent + {in_flight[0] + 1} task(s) x "
                          f"${reserve} reserve > ${a.cap_usd}", file=sys.stderr, flush=True)
                    return
                in_flight[0] += 1
        ws = tempfile.mkdtemp(prefix=f"ab{a.arm}_")
        try:
            if a.arm == "A":
                given, final, row = run_arm_a(t, a.agent, ws, a)
                row["extra_files"] = sorted(set(os.listdir(os.path.join(ws, "work"))) - {t["file"]})
            else:
                given, final, row = run_arm_b(t, a.generator, ws, a)
            g = grade(t, given, final)
        finally:
            shutil.rmtree(ws, ignore_errors=True)
        row.update(id=t["id"], cls=t["class"], arm=a.arm, variant=variant, meta=meta, **g)
        row["false_success"] = row["claim"] and not row["verified_clean"]
        with lock:
            with open(a.out, "a") as fh:
                fh.write(json.dumps(row) + "\n")
            # Only now is this task's spend visible to spent(); a task that
            # crashed stays reserved for the rest of the run.
            if paid:
                in_flight[0] -= 1
                rate_limited_run[0] = rate_limited_run[0] + 1 if row.get("rate_limited") else 0
                if rate_limited_run[0] >= RATE_LIMIT_STOP and not stop.is_set():
                    stop.set()
                    print(f"RATE LIMIT STOP: {RATE_LIMIT_STOP} consecutive tasks hit a "
                          "rate limit", file=sys.stderr, flush=True)
        print(f"  {a.arm}/{variant} {t['id']} claim={row['claim']} verified_clean="
              f"{row['verified_clean']} tampered={row['tests_tampered']} "
              f"${row['cost_usd']:.3f} {row['wall_s']}s", file=sys.stderr, flush=True)

    with cf.ThreadPoolExecutor(max_workers=a.parallel) as ex:
        list(ex.map(one, tasks))


if __name__ == "__main__":
    main()
