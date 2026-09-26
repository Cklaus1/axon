#!/usr/bin/env python3
"""Execute the v0.22 Stage-5 verification profile on an exact Axon + MiCode pair.

    MICODE_DIR=<micode checkout> scripts/v022_stage5_verify.py \
        [--manifest governance/v022_stage5_verification.json] [--results FILE]

The manifest (axon-v022-stage-verification/1) lists every harness Stage 5
REQUIRES. Each is executed here and classified:

  PASS     executed and established its property
  FAIL     executed and the property failed (or a required marker is absent)
  SKIP     the harness reported a skip          -> a Stage-5 FAILURE
  NOT_RUN  a prerequisite or the peer is absent -> a Stage-5 FAILURE

A missing prerequisite never turns a property into PASS. The results document
(axon-v022-stage-results/1) binds the verdict to BOTH revisions, both trees'
cleanliness at start AND end, and the manifest's digest; if either revision
moves during the run the verdict is FAIL (the pair is stale). Exit 0 only when
every required harness PASSed.

This is a STAGE profile. It does not change what AXON_HARNESS_STRICT means for
repository-wide qualification, and skips outside the Stage-5 set (listed in the
manifest's `outside_stage5_skips`) are recorded there as non-results, never as
passes.
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def git(repo, *args):
    r = subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else None


def tree_state(repo):
    if not repo or not os.path.isdir(repo):
        return {"head": None, "clean": False}
    head = git(repo, "rev-parse", "HEAD")
    porcelain = git(repo, "status", "--porcelain")
    return {"head": head, "clean": head is not None and porcelain == ""}


def run(cmd, cwd, env, log):
    t0 = time.time()
    with open(log, "w") as f:
        p = subprocess.run(cmd, cwd=cwd, env=env, stdout=f, stderr=subprocess.STDOUT)
    with open(log, errors="replace") as f:
        out = f.read()
    return p.returncode, out, round(time.time() - t0, 1)


def cargo_counts(out):
    passed = failed = ignored = 0
    for m in re.finditer(r"^test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored", out, re.M):
        passed += int(m.group(1))
        failed += int(m.group(2))
        ignored += int(m.group(3))
    return passed, failed, ignored


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", default=os.path.join(ROOT, "governance/v022_stage5_verification.json"))
    ap.add_argument("--results", default=os.path.join(ROOT, "target/v022-stage5-results.json"))
    a = ap.parse_args()

    raw = open(a.manifest, "rb").read()
    manifest = json.loads(raw)
    if manifest.get("schema") != "axon-v022-stage-verification/1":
        print("v022_stage5_verify: FAIL — manifest schema is not axon-v022-stage-verification/1")
        return 1
    micode = os.environ.get(manifest["peer"]["env"], "")
    micode = os.path.abspath(micode) if micode else ""
    axon_tgt = os.environ.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))
    micode_tgt = os.environ.get("MICODE_TARGET_DIR", os.path.join(axon_tgt, "micode"))
    logdir = os.path.join(os.path.dirname(os.path.abspath(a.results)), "v022-stage5-logs")
    os.makedirs(logdir, exist_ok=True)

    start = {"axon": tree_state(ROOT), "micode": tree_state(micode)}
    results = []

    for h in manifest["harnesses"]:
        rec = {"id": h["id"], "required": bool(h.get("required")), "kind": h["kind"], "repo": h["repo"]}
        repo_dir = ROOT if h["repo"] == "axon" else micode
        if h["repo"] == "micode" and not (micode and os.path.isdir(micode)):
            rec.update(status="NOT_RUN", detail=f"peer absent: {manifest['peer']['env']} is not a MiCode checkout")
            results.append(rec)
            continue
        env = dict(os.environ)
        if h["repo"] == "micode":
            env["CARGO_TARGET_DIR"] = micode_tgt
            # MiCode pins its own toolchain; do not leak Axon's override into it.
            env.pop("RUSTUP_TOOLCHAIN", None)
        missing = []
        for pre in h.get("prerequisites", []):
            if subprocess.run(pre, shell=True, cwd=repo_dir, env=env,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode != 0:
                missing.append(pre)
        if missing:
            rec.update(status="NOT_RUN", detail="prerequisite absent (a Stage-5 failure, not a skip): " + "; ".join(missing))
            results.append(rec)
            continue
        log = os.path.join(logdir, f"{h['id']}.log")
        if h["kind"] == "script":
            cmd = [os.path.join(repo_dir, h["command"][0]), *h["command"][1:]]
            if h["repo"] == "axon" and h["id"] == "loop-interop":
                env.setdefault("MICODE_TARGET_DIR", micode_tgt)
            rc, out, secs = run(cmd, repo_dir, env, log)
            lines = out.splitlines()
            if h.get("skip_marker") and any(l.startswith(h["skip_marker"]) for l in lines):
                rec.update(status="SKIP", detail="harness reported a skip — a required Stage-5 harness may not skip")
            elif rc != 0 or not any(l.startswith(h["pass_marker"]) for l in lines):
                rec.update(status="FAIL", detail=f"exit {rc}; pass marker {'present' if any(l.startswith(h['pass_marker']) for l in lines) else 'absent'}")
            else:
                bad = []
                seen = []
                for m in h.get("require_markers", []):
                    hit = next((re.match(m["pattern"], l) for l in lines if re.match(m["pattern"], l)), None)
                    if not hit:
                        bad.append(f"marker absent: {m['pattern']}")
                        continue
                    first, second = int(hit.group(1)), int(hit.group(2))
                    seen.append(hit.group(0))
                    if first < m.get("min_first", 0):
                        bad.append(f"{hit.group(0)}: fewer than {m['min_first']}")
                    if "second_equals" in m and second != m["second_equals"]:
                        bad.append(f"{hit.group(0)}: expected {m['second_equals']} failed")
                pass_line = next(l for l in lines if l.startswith(h["pass_marker"]))
                rec.update(status="FAIL" if bad else "PASS",
                           detail="; ".join(bad) if bad else pass_line, markers=seen)
            rec["seconds"] = secs
        elif h["kind"] == "cargo-exact":
            cmd = ["cargo", "test", "--locked", "-p", h["package"], *h["target"].split(), "--", "--exact", *h["tests"]]
            rc, out, secs = run(cmd, repo_dir, env, log)
            passed, failed, ignored = cargo_counts(out)
            want = len(h["tests"])
            if rc == 0 and passed == want and failed == 0 and ignored == 0:
                rec.update(status="PASS", detail=f"{passed}/{want} named tests passed")
            else:
                rec.update(status="FAIL", detail=f"exit {rc}; {passed} passed, {failed} failed, {ignored} ignored of {want} named (a renamed or filtered test is a failure)")
            rec["seconds"] = secs
        elif h["kind"] == "cargo-suite":
            rc, out, secs = run(h["command"], repo_dir, env, log)
            passed, failed, ignored = cargo_counts(out)
            if rc == 0 and failed == 0 and passed > 0:
                rec.update(status="PASS", detail=f"{passed} passed, 0 failed, {ignored} ignored")
            else:
                fails = sorted(set(re.findall(r"^test (\S+) \.\.\. FAILED", out, re.M)))
                rec.update(status="FAIL", detail=f"exit {rc}; {passed} passed, {failed} failed: {', '.join(fails[:10])}")
            rec["counts"] = {"passed": passed, "failed": failed, "ignored": ignored}
            rec["seconds"] = secs
        else:
            rec.update(status="FAIL", detail=f"unknown harness kind {h['kind']!r}")
        rec["log"] = log
        results.append(rec)

    end = {"axon": tree_state(ROOT), "micode": tree_state(micode)}
    by_id = {r["id"]: r for r in results}
    problems = []
    for side in ("axon", "micode"):
        if not start[side]["head"]:
            problems.append(f"{side}: no revision")
        elif start[side]["head"] != end[side]["head"]:
            problems.append(f"{side}: revision moved during the run ({start[side]['head']} -> {end[side]['head']}); the pair is stale")
        if not (start[side]["clean"] and end[side]["clean"]):
            problems.append(f"{side}: tree not clean at start and end")
    for r in results:
        if r["required"] and r["status"] != "PASS":
            problems.append(f"required harness {r['id']}: {r['status']} — {r.get('detail', '')}")
    properties = []
    for p in manifest["properties"]:
        st = "PASS" if all(by_id.get(x, {}).get("status") == "PASS" for x in p["harnesses"]) else "FAIL"
        properties.append({"task": p["task"], "gate": p["gate"], "harnesses": p["harnesses"],
                           "required": p["required"], "status": st})
        if p["required"] and st != "PASS":
            problems.append(f"property {p['task']}/{p['gate']}: not established")

    doc = {
        "schema": "axon-v022-stage-results/1",
        "profile": manifest["profile"],
        "manifest": os.path.relpath(a.manifest, ROOT),
        "manifest_sha256": hashlib.sha256(raw).hexdigest(),
        "pair": {"axon": {"start": start["axon"], "end": end["axon"]},
                 "micode": {"dir": micode, "start": start["micode"], "end": end["micode"]}},
        "harnesses": results,
        "properties": properties,
        "outside_stage5_skips": manifest.get("outside_stage5_skips", []),
        "not_established_here": manifest.get("not_established_here", []),
        "problems": problems,
        "verdict": "PASS" if not problems else "FAIL",
    }
    os.makedirs(os.path.dirname(os.path.abspath(a.results)), exist_ok=True)
    with open(a.results, "w") as f:
        json.dump(doc, f, indent=1, sort_keys=True)
        f.write("\n")

    for r in results:
        print(f"  {r['status']:<8}{r['id']:<26}{r.get('detail', '')}")
    for p in problems:
        print(f"  problem: {p}")
    ax, mc = start["axon"]["head"] or "?", start["micode"]["head"] or "?"
    if problems:
        print(f"v022_stage5_verify: FAIL — {len(problems)} problem(s) (axon {ax[:10]}, micode {mc[:10]})")
        return 1
    print(f"v022_stage5_verify: PASS — {len(results)} required harnesses, {len(properties)} properties "
          f"(axon {ax}, micode {mc}, manifest {doc['manifest_sha256'][:16]})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
