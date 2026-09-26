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


def normalize_failure(text):
    """The SEMANTIC part of a failure block: ANSI stripped; line:col, PIDs, thread
    ids, temp paths and timings replaced — so a fingerprint survives line-number
    churn and a different machine, and matches only what the failure says."""
    t = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", text)
    t = re.sub(r"\.rs:\d+:\d+", ".rs:<L>", t)
    t = re.sub(r"\(\d+\)", "(<N>)", t)
    t = re.sub(r"(/tmp|/var/tmp|/root|/home)/[^\s'\"]*", "<PATH>", t)
    t = re.sub(r"\b\d+(\.\d+)?(ms|s)\b", "<T>", t)
    return t


def _failure_block(out, test):
    blk = re.search(r"^---- (\S+::)?" + re.escape(test) + r" stdout ----\n(.*?)(?=^---- |^failures:$)", out, re.M | re.S)
    return normalize_failure(blk.group(2)) if blk else ""


def fingerprint_matches(dd, out):
    """(ok, why) — the declared defect's failure, as it appears in `out`, has the
    declared failure class, panic site and normalized message."""
    fp = dd["fingerprint"]
    text = _failure_block(out, dd["test"])
    if not text:
        return False, "no failure block for the declared test"
    if fp.get("failure_class") == "panic" and "panicked at" not in text:
        return False, "failure class is not a panic"
    site = "panicked at " + fp["test_binary"] + ":<L>"
    if site not in text:
        return False, f"panic site is not {fp['test_binary']}"
    if not re.search(fp["message_regex"], text):
        return False, "normalized failure message does not match the declared signature"
    return True, ""


def classify_suite(out, rc, declared, reproduced=None):
    """The regression suite's status from its cargo output. Exact-match, fail-closed:

      PASS                               no failures (a declared defect that now passes is
                                         reported in `stale_declarations`: remove it)
      PASS_WITH_KNOWN_BASELINE_DEFECTS   failures are EXACTLY declared defects, each with its
                                         declared fingerprint and still reproducing on its pinned
                                         baseline (`reproduced`); never counted as passes
      FAIL                               anything else: another test, a different signature, an
                                         extra failure, or a declaration that no longer reproduces
    """
    passed, failed, ignored = cargo_counts(out)
    fails = sorted(set(re.findall(r"^test (\S+) \.\.\. FAILED", out, re.M)))
    stale = [dd["id"] for dd in declared
             if re.search(r"^test (\S+::)?" + re.escape(dd["test"]) + r" \.\.\. ok$", out, re.M)]
    base = {"counts": {"passed": passed, "known_baseline_defects": 0, "failed": failed,
                       "ignored": ignored}, "stale_declarations": stale}
    if rc == 0 and failed == 0 and passed > 0 and not fails:
        note = f"{passed} passed, 0 failed, {ignored} ignored"
        if stale:
            note += f"; STALE declaration(s) {stale}: the defect no longer fails — remove it"
        return base | dict(status="PASS", detail=note)
    matched, why = [], []
    names = {f.rsplit("::", 1)[-1]: f for f in fails}
    for dd in declared:
        if dd["test"] not in names:
            continue
        ok, reason = fingerprint_matches(dd, out)
        if not ok:
            why.append(f"{dd['id']}: {reason}")
        elif reproduced is not None and not reproduced.get(dd["id"], False):
            why.append(f"{dd['id']}: does not reproduce on its pinned baseline — the exception is void")
        else:
            matched.append(dd)
    matched_tests = {dd["test"] for dd in matched}
    undeclared = [f for f in fails if f.rsplit("::", 1)[-1] not in matched_tests]
    expected = sum(dd["fingerprint"]["suite_failures_exactly"] for dd in matched)
    if matched and not undeclared and not why and failed == expected == len(fails) and passed > 0:
        base["counts"].update(failed=0, known_baseline_defects=failed)
        return base | dict(
            status="PASS_WITH_KNOWN_BASELINE_DEFECTS",
            detail=f"{passed} pass, {failed} known baseline defect(s): " + ", ".join(dd["id"] for dd in matched),
            declared_defects=[dd["id"] for dd in matched])
    problems = why + ([f"undeclared failures: {', '.join(undeclared[:10])}"] if undeclared else []) \
        + ([f"{failed} failures, {expected} declared"] if failed != expected else [])
    return base | dict(status="FAIL", detail=f"exit {rc}; {passed} passed, {failed} failed; "
                       + "; ".join(problems or ["no parsable failure"]))

def reproduce_declared(declared, micode, axon_tgt, env, logdir):
    """Re-run each declared defect's test at its pinned revision (a throwaway worktree
    of the peer repository). Reproduced = the test FAILED with the declared fingerprint.
    A TMPDIR on tmpfs cannot reproduce a filesystem-timing defect and counts as NOT
    reproduced — the exception then does not apply."""
    import shutil, tempfile
    got, logrec = {}, []
    tmp = env.get("TMPDIR", "/tmp")
    fs = subprocess.run(["stat", "-f", "-c", "%T", tmp], capture_output=True, text=True).stdout.strip()
    for dd in declared:
        rp = dd.get("reproduction") or {}
        entry = {"id": dd["id"], "revision": rp.get("revision"), "tmpdir_fs": fs}
        if not rp or fs == "tmpfs":
            entry["result"] = "NOT_REPRODUCED: " + ("no reproduction spec" if not rp else "TMPDIR is tmpfs")
            got[dd["id"]] = False
            logrec.append(entry)
            continue
        wt = tempfile.mkdtemp(prefix="s5repro-")
        os.rmdir(wt)
        add = subprocess.run(["git", "-C", micode, "worktree", "add", "-q", "--detach", wt, rp["revision"]],
                             capture_output=True, text=True)
        try:
            if add.returncode != 0:
                entry["result"] = f"NOT_REPRODUCED: cannot check out {rp['revision']}: {add.stderr.strip()[:120]}"
                got[dd["id"]] = False
            else:
                e2 = dict(env, CARGO_TARGET_DIR=os.path.join(axon_tgt, "micode-baseline"))
                e2.pop("RUSTUP_TOOLCHAIN", None)
                rlog = os.path.join(logdir, f"reproduce-{dd['id']}.log")
                rc, out, secs = run(["cargo", "test", "--locked", "-p", rp["package"], "--test", rp["test_target"],
                                     "--", "--exact", dd["test"]], wt, e2, rlog)
                failed_here = re.search(r"^test (\S+::)?" + re.escape(dd["test"]) + r" \.\.\. FAILED$", out, re.M)
                ok, why = fingerprint_matches(dd, out) if failed_here else (False, "the test did not fail")
                got[dd["id"]] = bool(ok)
                entry.update(result="REPRODUCED" if ok else f"NOT_REPRODUCED: {why}", seconds=secs, log=rlog)
        finally:
            subprocess.run(["git", "-C", micode, "worktree", "remove", "--force", wt], capture_output=True)
            shutil.rmtree(wt, ignore_errors=True)
        logrec.append(entry)
    return got, logrec


def _suite_log(results, blocks=()):
    """A minimal cargo-test transcript: `results` = [(name, 'ok'|'FAILED')], one binary."""
    lines = [f"test {n} ... {r}" for n, r in results]
    if blocks:
        lines += ["", "failures:", ""]
        for name, text in blocks:
            lines += [f"---- {name} stdout ----", text, ""]
        lines += ["failures:"] + [f"    {n}" for n, _ in blocks]
    p = sum(r == "ok" for _, r in results); f = sum(r == "FAILED" for _, r in results)
    lines.append(f"test result: {'ok' if not f else 'FAILED'}. {p} passed; {f} failed; 0 ignored; 0 measured; 0 filtered out")
    return "\n".join(lines) + "\n"


def self_test(declared):
    """The classifier against the cases it exists to tell apart. Run before anything else: a
    checker that cannot tell a declared defect from a new failure must not certify either."""
    # With nothing declared the matcher is unused but must stay PROVEN: test it against a
    # synthetic declaration of the same shape, so a regression cannot hide until the next defect.
    if not declared:
        declared = [{
            "id": "SELF-TEST-SYNTHETIC",
            "test": "the_goodbye_hint_names_an_interactive_resume_that_continues_the_session",
            "fingerprint": {"failure_class": "panic",
                            "test_binary": "crates/micode/tests/tui_terminal_lifecycle.rs",
                            "message_regex": "the goodbye hint names no `/resume <id>` line to type into MiCode",
                            "suite_failures_exactly": 1},
        }]
    d = declared[0]
    fp = d["fingerprint"]
    good = (f"thread '{d['test']}' (4242) panicked at {fp['test_binary']}:1805:13:\n"
            "the goodbye hint names no `/resume <id>` line to type into MiCode: \"/tmp/.tmpX...\"")
    moved = good.replace(":1805:13:", ":1999:7:")
    ok_rep = {d["id"]: True}
    cases = [
        ("expected test + expected fingerprint",
         _suite_log([("a", "ok"), (d["test"], "FAILED")], [(d["test"], good)]), 101, ok_rep,
         "PASS_WITH_KNOWN_BASELINE_DEFECTS", False),
        ("expected test + fingerprint at another line (normalized)",
         _suite_log([("a", "ok"), (d["test"], "FAILED")], [(d["test"], moved)]), 101, ok_rep,
         "PASS_WITH_KNOWN_BASELINE_DEFECTS", False),
        ("expected test + different fingerprint",
         _suite_log([("a", "ok"), (d["test"], "FAILED")],
                    [(d["test"], good.replace("goodbye hint names no", "session 1 never completed"))]),
         101, ok_rep, "FAIL", False),
        ("expected test + different panic site",
         _suite_log([("a", "ok"), (d["test"], "FAILED")], [(d["test"], good.replace("tui_terminal_lifecycle.rs", "other.rs"))]),
         101, ok_rep, "FAIL", False),
        ("different test + same text",
         _suite_log([("other_test", "FAILED"), (d["test"], "ok")], [("other_test", good)]), 101, ok_rep,
         "FAIL", True),
        ("expected test + second new failure",
         _suite_log([("a", "FAILED"), (d["test"], "FAILED")], [("a", "boom"), (d["test"], good)]), 101, ok_rep,
         "FAIL", False),
        ("declared defect no longer reproduces on its pinned baseline",
         _suite_log([("a", "ok"), (d["test"], "FAILED")], [(d["test"], good)]), 101, {d["id"]: False},
         "FAIL", False),
        ("baseline defect unexpectedly passes (stale declaration)",
         _suite_log([("a", "ok"), (d["test"], "ok")]), 0, ok_rep, "PASS", True),
        ("green, declared test absent", _suite_log([("a", "ok")]), 0, ok_rep, "PASS", False),
    ]
    bad = []
    for name, out, rc, rep, want, want_stale in cases:
        got = classify_suite(out, rc, declared, rep)
        if got["status"] != want:
            bad.append(f"classifier self-test '{name}': got {got['status']}, want {want}")
        if want_stale != bool(got["stale_declarations"]):
            bad.append(f"classifier self-test '{name}': stale={got['stale_declarations']}, want stale={want_stale}")
        if got["status"] == "PASS_WITH_KNOWN_BASELINE_DEFECTS" and got["counts"]["known_baseline_defects"] != 1:
            bad.append(f"classifier self-test '{name}': a known defect was not counted separately")
    return bad

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
    broken = self_test(manifest.get("declared_baseline_defects", []))
    if broken:
        for b in broken:
            print("  " + b)
        print("v022_stage5_verify: FAIL — the suite classifier failed its own self-test")
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
        if (h["repo"] == "micode" or h["kind"] == "discriminator") and not (micode and os.path.isdir(micode)):
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
        elif h["kind"] == "discriminator":
            # The same real-system gate against MiCode builds that LACK the property: each must
            # FAIL its section. Throwaway worktrees of the peer's own repository, pinned revisions.
            import shutil, tempfile
            case_out, bad, secs = [], [], 0.0
            disc_tgt = os.path.join(axon_tgt, "micode-discriminator")
            for c in h["cases"]:
                wt = tempfile.mkdtemp(prefix="s5disc-")
                os.rmdir(wt)
                added = subprocess.run(["git", "-C", micode, "worktree", "add", "-q", "--detach", wt, c["micode_revision"]],
                                       capture_output=True, text=True)
                if added.returncode != 0:
                    bad.append(f"{c['section']}: cannot check out {c['micode_revision'][:10]}: {added.stderr.strip()[:120]}")
                    continue
                try:
                    e2 = dict(env, MICODE_DIR=wt, MICODE_TARGET_DIR=disc_tgt)
                    clog = os.path.join(logdir, f"discriminator-{c['section']}.log")
                    rc, out, s2 = run([os.path.join(repo_dir, h["command"][0])], repo_dir, e2, clog)
                    secs += s2
                    pat = re.compile(r"^loop_interop_gate: " + re.escape(c["section"]) + r" section executed (\d+) assertions, (\d+) failed$", re.M)
                    mm = pat.search(out)
                    if rc == 0:
                        bad.append(f"{c['section']}: the gate PASSED a MiCode build without the property ({c['micode_revision'][:10]})")
                    elif not mm:
                        bad.append(f"{c['section']}: section marker absent against {c['micode_revision'][:10]} (the gate did not reach it)")
                    elif int(mm.group(2)) < c["min_failed"]:
                        bad.append(f"{c['section']}: only {mm.group(2)} failed against {c['micode_revision'][:10]}, need >= {c['min_failed']}")
                    case_out.append({"revision": c["micode_revision"], "section": c["section"],
                                     "marker": mm.group(0) if mm else None, "gate_exit": rc})
                finally:
                    subprocess.run(["git", "-C", micode, "worktree", "remove", "--force", wt], capture_output=True)
                    shutil.rmtree(wt, ignore_errors=True)
            rec.update(status="FAIL" if bad or not case_out else "PASS",
                       detail="; ".join(bad) if bad else "; ".join(f"{x['section']} vs {x['revision'][:10]}: {x['marker']}" for x in case_out),
                       cases=case_out, seconds=round(secs, 1))
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
            declared = manifest.get("declared_baseline_defects", [])
            # Each declared defect must STILL reproduce on its pinned baseline, in
            # THIS run, or its exception is void (fail closed).
            reproduced, repro_log = reproduce_declared(declared, micode, axon_tgt, env, logdir)
            rc, out, secs = run(h["command"], repo_dir, env, log)
            rec["seconds"] = secs
            rec["reproductions"] = repro_log
            rec.update(classify_suite(out, rc, declared, reproduced))
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
    hdef = {h["id"]: h for h in manifest["harnesses"]}
    for r in results:
        if r["required"] and r["status"] != "PASS":
            problems.append(f"required harness {r['id']}: {r['status']} — {r.get('detail', '')}")
        elif not r["required"] and hdef[r["id"]].get("gates_profile") and r["status"] not in ("PASS", "PASS_WITH_KNOWN_BASELINE_DEFECTS"):
            problems.append(f"regression harness {r['id']}: {r['status']} — {r.get('detail', '')}")
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
    }
    suite = next((r for r in results if hdef[r["id"]].get("role") == "regression"), None)
    defects = (suite or {}).get("declared_defects", [])
    # Three INDEPENDENT quantities; a known defect is never counted as a pass.
    doc["stage5_required"] = "PASS" if all(
        r["status"] == "PASS" for r in results if r["required"]) and all(
        p["status"] == "PASS" for p in properties if p["required"]) else "FAIL"
    doc["micode_full_suite"] = {
        "status": (suite or {}).get("status", "NOT_RUN"),
        "counts": (suite or {}).get("counts"),
        "known_baseline_defects": defects,
        "stale_declarations": (suite or {}).get("stale_declarations", []),
        "reproductions": (suite or {}).get("reproductions", []),
    }
    doc["stage5_verdict"] = "NOT_VERIFIED" if problems else "VERIFIED"
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
    nreq = sum(1 for r in results if r["required"])
    ms = doc["micode_full_suite"]
    c = ms.get("counts") or {}
    print(f"v022_stage5_verify: {doc['stage5_verdict']} — stage5_required {doc['stage5_required']} "
          f"({nreq} required harnesses, {len(properties)} properties); micode_full_suite {ms['status']} "
          f"({c.get('passed', 0)} pass, {c.get('known_baseline_defects', 0)} known baseline defect(s)"
          + (f": {', '.join(defects)}" if defects else "") + ")"
          + (f"; STALE declaration(s): {', '.join(ms['stale_declarations'])}" if ms["stale_declarations"] else "")
          + f" (axon {ax}, micode {mc}, manifest {doc['manifest_sha256'][:16]})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
