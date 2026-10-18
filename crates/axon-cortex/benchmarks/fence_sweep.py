#!/usr/bin/env python3
"""R1f — the offline fence sweep: does `cortex repair`'s runtime fence change
any test verdict on the corpora a paid run is scored on?

For every task of the receipt corpus (scorecard/receipt-corpus.json) and of
heldout1 (heldout/heldout.json), in a fresh temp directory:

  1. `cortex grant` on the BROKEN file — the file a repair starts from, so the
     fence is exactly the one `cortex repair` would derive;
  2. `axon test --json` on the broken file AND on the pristine (repaired)
     source, each twice: from an empty environment (how cortex runs every
     check) and from that environment plus the grant's `fence_env`.

A FLIP is any test whose status differs between the two runs, a run that
completes in one and not the other, or a fenced run that prints a sandbox
refusal. The sweep passes only with zero flips. It spends no model call.

Usage:
    AXON_BIN=... CORTEX_BIN=... python3 fence_sweep.py [--corpus FILE ...] [--json]

Exit 0: zero flips over at least one swept task. Exit 1: at least one flip
(each printed). Exit 2: nothing was proven — a corpus that does not load or
holds no task, a task that could not be swept (no grant, a run that never
completed in either arm, a crash), so zero swept tasks — never a pass.
"""
import argparse
import concurrent.futures
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "ab"))
import corpus  # noqa: E402
from common import ROOT  # noqa: E402

AXON = os.environ.get("AXON_BIN", f"{ROOT}/target/debug/axon")
CORTEX = os.environ.get("CORTEX_BIN", f"{ROOT}/target/debug/cortex")
DEFAULT_CORPORA = (os.path.join(HERE, "scorecard", "receipt-corpus.json"),
                   os.path.join(HERE, "heldout", "heldout.json"))
# The interpreter's sandbox refusals (crates/axon-core capabilities): a fenced
# run printing one of these was denied something, whatever its verdict.
REFUSALS = ("the active sandbox restricts it to",
            "not in the active sandbox's allowed set",
            "sandbox violation")
TIMEOUT_S = 120


def axon_test(ws, rel, env):
    """({name: status} or None if the run did not complete, refusal lines)."""
    try:
        p = subprocess.run([AXON, "test", rel, "--json"], cwd=ws, env=env,
                           capture_output=True, text=True, timeout=TIMEOUT_S)
        out = p.stdout + p.stderr
    except subprocess.TimeoutExpired:
        return None, []
    res, saw = {}, False
    for line in p.stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if v.get("type") == "summary":
            saw = True
        elif "name" in v and "status" in v:
            res[v["name"]] = v["status"]
    refused = [ln.strip() for ln in out.splitlines() if any(r in ln for r in REFUSALS)]
    return (res if saw else None), refused


def sweep(set_name, t):
    """One task: its grant, and the flips the fence causes on each source. A
    crash is the task's error, never an uncaught exception (whose exit 1 the
    gate would read as a flip, and which would hide every other row)."""
    try:
        return sweep_one(set_name, t)
    except Exception as e:  # noqa: BLE001 — reported as this task's error
        return {"set": set_name, "id": t.get("id"), "file": t.get("file"),
                "symbol": t.get("symbol"), "flips": [],
                "error": f"sweep crashed: {type(e).__name__}: {e}"}


def sweep_one(set_name, t):
    row = {"set": set_name, "id": t["id"], "file": t["file"], "symbol": t["symbol"],
           "flips": [], "error": None}
    with tempfile.TemporaryDirectory(prefix="fence-sweep-") as ws:
        path = os.path.join(ws, t["file"])
        with open(path, "w") as f:
            f.write(t["broken"])
        g = subprocess.run([CORTEX, "grant", "--workspace", ws, "--file", t["file"],
                            "--symbol", t["symbol"], "--axon", AXON, "--json"],
                           capture_output=True, text=True, timeout=TIMEOUT_S)
        if g.returncode != 0:
            row["error"] = f"cortex grant exit {g.returncode}: {g.stderr.strip()[:300]}"
            return row
        grant = json.loads(g.stdout)["grant"]
        fence_env = grant["fence_env"]
        row.update(basis=grant["basis"], applied=grant["applied"], fence_env=fence_env)
        for label in ("broken", "pristine"):
            with open(path, "w") as f:
                f.write(t[label])
            plain, _ = axon_test(ws, t["file"], {})
            fenced, refused = axon_test(ws, t["file"], dict(fence_env))
            if plain is None and fenced is None:
                row["error"] = f"{label}: `axon test --json` did not complete in either arm"
                return row
            if plain is None or fenced is None:
                row["flips"].append({"source": label, "test": None,
                                     "plain": "completed" if plain is not None else "incomplete",
                                     "fenced": "completed" if fenced is not None else "incomplete"})
                continue
            for name in sorted(set(plain) | set(fenced)):
                if plain.get(name) != fenced.get(name):
                    row["flips"].append({"source": label, "test": name,
                                         "plain": plain.get(name), "fenced": fenced.get(name)})
            for line in refused:
                row["flips"].append({"source": label, "test": None, "refusal": line[:300]})
    return row


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--corpus", action="append",
                    help="a corpus file (repeatable; default: receipt corpus + heldout1)")
    ap.add_argument("--json", action="store_true", help="one JSON summary line on stdout")
    args = ap.parse_args()
    jobs = []
    for path in args.corpus or DEFAULT_CORPORA:
        name = os.path.relpath(path, HERE)
        # corpus.load raises SystemExit (exit 1, read as a flip) on a task
        # whose rebuilt text does not match; any failure to load is exit 2.
        try:
            tasks = corpus.load(path)["tasks"]
        except (Exception, SystemExit) as e:  # noqa: BLE001
            print(f"ERROR {name}: the corpus does not load: {e}", file=sys.stderr)
            sys.exit(2)
        if not tasks:
            print(f"ERROR {name}: the corpus holds no task; nothing would be swept",
                  file=sys.stderr)
            sys.exit(2)
        for t in tasks:
            jobs.append((name, t))
    with concurrent.futures.ThreadPoolExecutor(max_workers=os.cpu_count() or 4) as pool:
        rows = list(pool.map(lambda j: sweep(*j), jobs))

    errors = [r for r in rows if r["error"]]
    flipped = [r for r in rows if r["flips"]]
    for r in errors:
        print(f"ERROR {r['set']} {r['id']}: {r['error']}", file=sys.stderr)
    for r in flipped:
        for f in r["flips"]:
            print(f"FLIP {r['set']} {r['id']} ({r['file']}, {f['source']}): {json.dumps(f)}",
                  file=sys.stderr)
    swept = [r for r in rows if not r["error"]]
    summary = {
        "schema": "cortex-fence-sweep/1",
        "tasks": len(rows),
        "per_set": {s: sum(1 for r in rows if r["set"] == s) for s in sorted({r["set"] for r in rows})},
        "fenced": sum(1 for r in swept if r["fence_env"]),
        "basis_unknown": sum(1 for r in swept if r["basis"] == "unknown"),
        "fence_envs": sorted({json.dumps(r["fence_env"], sort_keys=True) for r in swept}),
        "flips": sum(len(r["flips"]) for r in rows),
        "errors": len(errors),
        "pass": bool(swept) and not errors and not flipped,
    }
    if args.json:
        print(json.dumps(summary))
    else:
        for k, v in summary.items():
            print(f"{k}: {v}")
    # Zero swept tasks proves nothing, whatever else happened.
    sys.exit(2 if errors or not swept else 1 if flipped else 0)


if __name__ == "__main__":
    main()
