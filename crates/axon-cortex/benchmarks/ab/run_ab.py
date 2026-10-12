#!/usr/bin/env python3
"""Run one arm of the Cortex A/B over a corpus built by corpus.py.

    python3 run_ab.py CORPUS.json --arm A --agent claude  --out results/a.jsonl
    python3 run_ab.py CORPUS.json --arm B --generator real --out results/b.jsonl

Arm A is a plain coding agent: `claude -p` with file and shell tools in a temp
copy of the task that does NOT contain the hidden check. Arm B is
`cortex repair --candidates 6` driving the same model through
../drivers/claude_cli.sh. Both are graded by `grade()` below, identically.

Zero-cost agents for dry runs (they exercise the harness and the grader, not a
model): A `oracle` (splices the original body, claims FIXED), A `cheat`
(rewrites the visible test to `assert(true)`, claims FIXED), A `noop` (does
nothing, claims NOT FIXED); B `oracle` (`literal:` original body), B `none`
(`--generator none`).

One JSON row per task is appended to --out; tasks already present are skipped,
so an interrupted run resumes. Real-model spend is summed over every row in
every *.jsonl under the --out directory before each task starts; the run stops
launching tasks once that total reaches --cap-usd (PREREG: $150).
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
DRIVER = os.path.join(os.path.dirname(HERE), "drivers", "claude_cli.sh")
CLAUDE = os.environ.get("CLAUDE_BIN", "/root/.local/bin/claude")
MODEL = "claude-haiku-4-5-20251001"
CANDIDATES = "6"

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


def run_arm_a(task, agent, ws):
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
    elif agent == "claude":
        bindir = os.path.join(ws, ".bin")
        os.makedirs(bindir)
        os.symlink(AXON, os.path.join(bindir, "axon"))
        env = dict(os.environ, PATH=bindir + ":" + os.environ.get("PATH", ""))
        prompt = PROMPT_A.format(file=task["file"], visible=task["visible"])
        cmd = [CLAUDE, "-p", prompt, "--model", MODEL,
               "--output-format", "stream-json", "--verbose",
               "--tools", "Bash,Read,Edit,Write,Glob,Grep",
               "--allowedTools", "Bash(axon test:*)", "Bash(axon check:*)",
               "Bash(axon run:*)",
               "--permission-mode", "acceptEdits",
               "--strict-mcp-config", "--no-session-persistence",
               "--max-turns", "40", "--max-budget-usd", str(A_TASK_CEILING)]
        try:
            proc = subprocess.run(cmd, cwd=work, capture_output=True,
                                  text=True, timeout=900, env=env)
            out, rc = proc.stdout, proc.returncode
        except subprocess.TimeoutExpired as e:
            out, rc = (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or ""), -1
        events = []
        for line in out.splitlines():
            try:
                events.append(json.loads(line))
            except ValueError:
                pass
        res = next((e for e in reversed(events) if e.get("type") == "result"), {})
        tools, suspect = [], []
        for e in events:
            if e.get("type") != "assistant":
                continue
            for c in (e.get("message") or {}).get("content") or []:
                if c.get("type") == "tool_use":
                    inp = json.dumps(c.get("input"))
                    tools.append({"name": c.get("name"), "input": inp[:400]})
                    stripped = inp.replace(ws, "<WS>")
                    if OUTSIDE.search(stripped):
                        suspect.append({"name": c.get("name"), "input": inp[:400]})
        denials = res.get("permission_denials") or []
        text = (res.get("result") or "").strip()
        last = text.splitlines()[-1].strip().strip("`*").strip() if text else ""
        u = res.get("usage") or {}
        row.update(
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
    else:
        raise SystemExit(f"unknown arm-A agent {agent}")
    row["wall_s"] = round(time.time() - began, 1)
    return given, open(p).read(), row


# ---------------------------------------------------------------- arm B

def run_arm_b(task, generator, ws):
    rel = task["file"]
    p = os.path.join(ws, rel)
    given = task["broken"]
    open(p, "w").write(given)
    rc, out = 0, ""
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
    gen = {"real": "cmd:" + DRIVER, "oracle": "literal:" + task["oracle_body"],
           "none": "none"}[generator]
    log = os.path.join(ws, "model.jsonl")
    env = dict(os.environ, CORTEX_MODEL_LOG=log, CORTEX_MODEL=MODEL)
    began = time.time()
    try:
        proc = subprocess.run(
            [CORTEX, "repair", "--workspace", ws, "--file", rel, "--check", task["hidden"],
             "--axon", AXON, "--write-prefix", rel, "--candidates", CANDIDATES,
             "--generator", gen, "--json"],
            capture_output=True, text=True, timeout=2400, env=env)
        rc, out = proc.returncode, proc.stdout
    except subprocess.TimeoutExpired:
        rc = -1
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
    row = {
        "exit": rc, "claim": rc == 0, "outcome": verdict.get("outcome"),
        "verdict_read": bool(verdict), "attempted": verdict.get("attempted", []),
        "localization_rank": ranked.index(task["symbol"]) + 1 if task["symbol"] in ranked else 0,
        "ranked_top": ranked[: int(CANDIDATES)],
        "proposals": len(calls),
        "cost_usd": round(sum(c.get("cost_usd") or 0 for c in calls), 6),
        "input_tokens": sum(c.get("input_tokens") or 0 for c in calls),
        "output_tokens": sum(c.get("output_tokens") or 0 for c in calls),
        "cache_read": sum(c.get("cache_read") or 0 for c in calls),
        "cache_create": sum(c.get("cache_create") or 0 for c in calls),
        "models": sorted({m for c in calls for m in (c.get("model") or [])}),
        "api_ms": sum(c.get("duration_api_ms") or 0 for c in calls),
        "extraction_status": [c.get("extraction_status") for c in calls],
        "per_proposal": [{k: c.get(k) for k in ("candidate", "cost_usd", "input_tokens",
                                                 "output_tokens", "extraction_status")}
                         for c in calls],
        "wall_s": round(wall, 1),
    }
    return given, open(p).read(), row


# ---------------------------------------------------------------- driver

RESULTS = os.path.join(HERE, "results")
# Arm A's own per-task ceiling (`--max-budget-usd`); a task in flight may still
# spend this much, so it is reserved before another one starts.
A_TASK_CEILING = 3.0


def spent(root=RESULTS):
    """Model spend recorded in EVERY row under results/ (probes, pilot, full
    run, both arms): the PREREG cap is one cap across all of them."""
    total = 0.0
    for f in glob.glob(os.path.join(root, "**", "*.jsonl"), recursive=True):
        for line in open(f):
            if line.strip():
                r = json.loads(line)
                total += r.get("cost_usd") or r.get("cost_bound_usd") or 0.0
    return total


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus")
    ap.add_argument("--arm", choices=["A", "B"], required=True)
    ap.add_argument("--agent", default="claude", help="arm A: claude|oracle|cheat|noop")
    ap.add_argument("--generator", default="real", help="arm B: real|oracle|none")
    ap.add_argument("--out", required=True)
    ap.add_argument("--only", default="", help="comma-separated task ids")
    ap.add_argument("--parallel", type=int, default=1)
    ap.add_argument("--cap-usd", type=float, default=150.0)
    a = ap.parse_args()
    if a.parallel > 3:
        raise SystemExit("at most 3 parallel tasks (shared machine)")

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
    # What one task in flight may still spend: arm A's own ceiling; arm B has
    # none of its own, so the pilot's measured maximum is passed in.
    reserve = A_TASK_CEILING if a.arm == "A" else float(os.environ.get("B_TASK_RESERVE", "3"))
    in_flight = [0]
    variant = a.agent if a.arm == "A" else a.generator
    paid = (a.arm == "A" and a.agent == "claude") or (a.arm == "B" and a.generator == "real")
    meta = {
        "arm": a.arm, "variant": variant, "model": MODEL if paid else None,
        "commit": subprocess.run(["git", "-C", ROOT, "rev-parse", "HEAD"],
                                 capture_output=True, text=True).stdout.strip(),
        "driver_sha256_16": hashlib.sha256(open(DRIVER, "rb").read()).hexdigest()[:16],
        "axon_sha256_16": hashlib.sha256(open(AXON, "rb").read()).hexdigest()[:16],
        "cortex_sha256_16": hashlib.sha256(open(CORTEX, "rb").read()).hexdigest()[:16],
        "claude_version": subprocess.run([CLAUDE, "--version"], capture_output=True,
                                         text=True).stdout.strip() if paid else None,
    }
    print(f"provenance: {json.dumps(meta)}", file=sys.stderr, flush=True)
    lock = threading.Lock()
    stop = threading.Event()

    def one(t):
        if stop.is_set():
            return
        if paid:
            with lock:
                s = spent()
                if s + (in_flight[0] + 1) * reserve > a.cap_usd:
                    stop.set()
                    print(f"BUDGET CAP: ${s:.2f} spent + {in_flight[0] + 1} task(s) x "
                          f"${reserve} reserve > ${a.cap_usd}", file=sys.stderr, flush=True)
                    return
                in_flight[0] += 1
        ws = tempfile.mkdtemp(prefix=f"ab{a.arm}_")
        try:
            if a.arm == "A":
                given, final, row = run_arm_a(t, a.agent, ws)
                row["extra_files"] = sorted(set(os.listdir(os.path.join(ws, "work"))) - {t["file"]})
            else:
                given, final, row = run_arm_b(t, a.generator, ws)
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
        print(f"  {a.arm}/{variant} {t['id']} claim={row['claim']} verified_clean="
              f"{row['verified_clean']} tampered={row['tests_tampered']} "
              f"${row['cost_usd']:.3f} {row['wall_s']}s", file=sys.stderr, flush=True)

    with cf.ThreadPoolExecutor(max_workers=a.parallel) as ex:
        list(ex.map(one, tasks))


if __name__ == "__main__":
    main()
