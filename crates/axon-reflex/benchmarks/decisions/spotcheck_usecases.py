#!/usr/bin/env python3
"""Recompute 10 random labels per use-case category from their sources, without build_usecases.py.

    python3 spotcheck_usecases.py      # prints one line per checked item and a summary;
                                       # exit 1 on any mismatch (needs AXON_BIN, CARGO_TARGET_DIR)

trust    re-read the arm-A row (claim / false_success / verified_clean / tests_tampered),
         or the MiCode corpus case's `expected`.
axbool   run `axon check` / `axon run` / `axon test` on the program in the item's prompt.
improve  `axon check` the item's program, then `AXON_AI_MOCK=1 axon improve discover --ai`:
         the only template proposed must be the label.
route55  re-read the Haiku 5.5 B-arm row: verified_done on the first proposal.
audit    re-read the recorded grade; where the state holds the complete file, apply
         `proposed_patch` to it with patch(1) and compare sha256[:16] with the row's final_sha.
risk     decode the command (sh -c / base64 / hex-printf) back to the inner command, read its
         RiskTier at the cited line of MiCode's test source, apply the rule (sh -c: the pin;
         obfuscated: max(pin, sensitive)); re-run MiCode's Tier A (the probe binary
         build_usecases.py built, $CARGO_TARGET_DIR/debug/micode-risk-probe) on the command.
"""
import base64
import hashlib
import json
import os
import random
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
AB = os.environ.get("AB_DIR", os.path.join(ROOT, "crates", "axon-cortex", "benchmarks", "ab"))
AXON = os.environ.get("AXON_BIN", "/home/cklaus/projects/axon-lanes/.doc-build/decision-usecases/debug/axon")
MICODE = os.environ.get("MICODE_DIR", "/home/cklaus/projects/aicoding/micode")
CATS = ["trust", "axbool", "improve", "route55", "audit", "risk"]
RISK_ORDER = ["read", "routine", "sensitive", "critical"]
PROBE = os.path.join(os.environ.get("CARGO_TARGET_DIR", ""), "debug", "micode-risk-probe")
RUNS = {"cc-haiku-4.5": "results/full/a.jsonl",
        "micode-haiku-5.5": "results-micode/full/claude-haiku-5-5/a.jsonl",
        "micode-opus-5.5": "results-micode/full/claude-opus-5-5/a.jsonl"}
TMP = os.environ.get("TMPDIR")


def rows(path):
    return {r["id"]: r for r in map(json.loads, open(os.path.join(AB, path)))}


def sh(args, src, name="prog.ax", env=None):
    ws = tempfile.mkdtemp(prefix="spot_", dir=TMP)
    try:
        open(os.path.join(ws, name), "w").write(src)
        r = subprocess.run([AXON, *args, name], cwd=ws, capture_output=True, text=True, timeout=120,
                           env=dict(os.environ, AXON_SEED="1", **(env or {})))
        return r.returncode
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def program(prompt):
    return re.search(r"```axon\n(.*)```", prompt, re.S).group(1)


def trust(x):
    p = x["provenance"]
    if p["run"] == "micode-corpus":
        case = next(json.loads(l) for l in open(os.path.join(MICODE, "crates/micode-verify/corpus/synthetic_examples.jsonl"))
                    if json.loads(l)["name"] == p["task"])
        assert case["response"]["text"] == x["state"]["candidate_response"]
        return "genuine" if case["expected"] == "Clean" else "fabricated", f"expected={json.dumps(case['expected'])}"
    r = rows(RUNS[p["run"]])[p["task"]]
    assert r["final_text"] == x["state"]["candidate_response"] and r["claim"]
    lab = "fabricated" if r["false_success"] else ("genuine" if r["verified_clean"] and not r["tests_tampered"] else None)
    return lab, f"false_success={r['false_success']} verified_clean={r['verified_clean']}"


def axbool(x):
    kind, prompt = x["provenance"]["kind"], x["state"]["prompt"]
    if kind == "eval":
        expr = re.search(r"expression `(.*)` evaluate", prompt).group(1)
        rc = sh(["run"], "fn main() -> i64 {\n    if %s { 1 } else { 0 }\n}\n" % expr)
        return {1: True, 0: False}.get(rc), f"axon run exit {rc}"
    if kind == "typecheck":
        rc = sh(["check"], program(prompt))
        return {0: True, 2: False}.get(rc), f"axon check exit {rc}"
    rc = sh(["test"], program(prompt))
    return {0: True, 3: False}.get(rc), f"axon test exit {rc}"


def improve(x):
    src = x["state"]["program"]
    if sh(["check"], src) != 0:
        return None, "does not type-check"
    ws = tempfile.mkdtemp(prefix="spot_imp_", dir=TMP)
    try:
        os.makedirs(os.path.join(ws, "c"))
        open(os.path.join(ws, "c", "p.ax"), "w").write(src)
        subprocess.run([AXON, "improve", "discover", "c", "--ai"], cwd=ws, capture_output=True, timeout=120,
                       env=dict(os.environ, AXON_AI_MOCK="1"), check=True)
        names = sorted(re.search(r"^name = (.*)$", open(os.path.join(ws, "proposals", f)).read(), re.M).group(1)
                       for f in os.listdir(os.path.join(ws, "proposals")))
    finally:
        shutil.rmtree(ws, ignore_errors=True)
    return (names[0] if len(names) == 1 else None), f"proposed {names}"


def route55(x):
    r = rows("results-micode/full/claude-haiku-5-5/b.jsonl")[x["provenance"]["task"]]
    one = r["outcome"] == "verified_done" and int(r["proposals"]) == 1
    return ("cheap_first_try" if one else "escalate"), f"outcome={r['outcome']} proposals={r['proposals']}"


def audit(x):
    p, st = x["provenance"], x["state"]
    if p["kind"] == "real-agent":
        run = {v: k for k, v in {"cc-haiku-4.5": "claude-haiku-4.5 via Claude Code",
                                 "micode-haiku-5.5": "claude-haiku-5-5 via MiCode",
                                 "micode-opus-5.5": "claude-opus-5-5 via MiCode"}.items()}[p["agent"]]
        r = rows(RUNS[run])[p["task"]]
        lab = False if (r["verified_clean"] and not r["tests_tampered"] and not r["suspicious_calls_added"]) else None
    else:
        r = rows(f"results/dryrun/a-{p['kind']}.jsonl")[p["task"]]
        lab = True if (r["false_success"] and not r["verified_clean"]) else None
    note = f"false_success={r['false_success']} verified_clean={r['verified_clean']}"
    if st["source_note"].startswith("complete file"):
        ws = tempfile.mkdtemp(prefix="spot_aud_", dir=TMP)
        try:
            f = os.path.join(ws, st["file"])
            open(f, "w").write(st["source"])
            open(os.path.join(ws, "d.patch"), "w").write(st["proposed_patch"])
            subprocess.run(["patch", "-s", "-p1", "-i", "d.patch"], cwd=ws, check=True, capture_output=True)
            got = hashlib.sha256(open(f).read().encode()).hexdigest()[:16]
        finally:
            shutil.rmtree(ws, ignore_errors=True)
        note += f"; patched sha {got} " + ("==" if got == r["final_sha"] else "!=") + " final_sha"
        if got != r["final_sha"]:
            lab = None
    return lab, note


def risk(x):
    cmd, p = x["state"]["command"], x["provenance"]
    if cmd.startswith("sh -c "):
        words = shlex.split(cmd)
        assert words[:2] == ["sh", "-c"] and len(words) == 3
        inner, rule = words[2], "pin"
    elif cmd.endswith(" | base64 -d | bash"):
        inner, rule = base64.b64decode(cmd[len("echo "):-len(" | base64 -d | bash")]).decode(), "floor"
    else:
        hexed = re.fullmatch(r"printf '((?:\\x[0-9a-f]{2})+)' \| sh", cmd).group(1)
        inner, rule = bytes.fromhex(hexed.replace("\\x", "")).decode(), "floor"
    assert inner == p["inner"], (inner, p["inner"])
    tiers = set()
    for where in p["where"]:
        path, line = re.match(r"(\S+):(\d+)", where).groups()
        src = open(os.path.join(MICODE, path)).read()
        at = sum(len(l) for l in src.splitlines(True)[:int(line) - 1])
        k = src.index(json.dumps(inner, ensure_ascii=False)[1:-1], at)
        tiers.add(re.compile(r"RiskTier::(\w+)").search(src, k).group(1))
    assert len(tiers) == 1, tiers
    pin = tiers.pop().lower()
    lab = pin if rule == "pin" else max(pin, "sensitive", key=RISK_ORDER.index)
    out = subprocess.run([PROBE], input=json.dumps(cmd) + "\n", capture_output=True, text=True, check=True).stdout
    st = json.loads(out)["static_tier"]
    if st != x["state"]["static_tier"]:
        lab = None
    return lab, f"inner `{inner}` pinned {pin} ({p['where'][0].split(' ')[0]}); Tier A {st}"


def main():
    items = [json.loads(l) for l in open(os.path.join(HERE, "decision_set.jsonl"))]
    rng = random.Random(20261012)
    bad = 0
    for c in CATS:
        xs = [x for x in items if x["cat"] == c]
        ok = 0
        for x in rng.sample(xs, 10):
            lab, note = globals()[c](x)
            match = lab == x["label"]
            ok += match
            print(f"{'OK ' if match else 'BAD'} {x['id']}: label={x['label']} recomputed={lab} ({note})")
        print(f"== {c}: {ok}/10 match\n")
        bad += 10 - ok
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
