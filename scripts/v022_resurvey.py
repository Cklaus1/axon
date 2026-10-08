#!/usr/bin/env python3
"""Re-measure the OBSERVED entries of the refusal-site gate (C9 round 11, eqgate7, amendment 107).

The gate's third disposition for a site is neither a row nor a checkable fact but a RECORDED
MEASUREMENT: a survey removed (or changed) the guard / value, and a named test failed. 167 entries
(96 value sites, 60 Python guards, 11 Rust guards) rest on it, and nothing re-ran it: a test that was
later deleted, weakened or renamed leaves the entry standing and the claim wrong. This tool
re-measures a deterministic SAMPLE of them and writes a record the FREEZE refuses to proceed
without.

    python3 scripts/v022_resurvey.py --run [--out PATH] [--sample auto|all|PCT] [--salt S]
                                     [--families value,py,guard]
    python3 scripts/v022_resurvey.py --check [PATH]      # is there a recent record for HEAD? (exit 1: no)
    python3 scripts/v022_resurvey.py --plan [--sample ..] # what would be re-measured, and the estimate

SAMPLE. `auto` (the default) is every entry when the estimate (EST_SECONDS per entry) is under 5
minutes, else a deterministic 25 %: an entry is drawn when sha256(salt|key) mod 100 < pct, the salt
being the HEAD commit, so the same head draws the same entries and a new head rotates which 25 %.
`--sample all` re-measures everything (hours; the whole run is a freeze-time step on gpumaster).

WHAT IS MEASURED. A VALUE entry: the survey's own mutation of that value (v022_value_survey.mutate),
then the test binaries the entry's reason names (every `<binary> -p P --test T` token) or, with none,
the owning crate's suite; the entry holds when a test that passed on the unmutated tree now fails.
A PYTHON guard: scripts/v022_py_guard_survey.py on that site's line. A RUST guard: the `.ok_or(..)?`
replaced by `.unwrap_or_default()` and the tests the entry names; an entry whose edit is not that
(a bound raised, a table extended) or does not compile is counted NOT RE-MEASURED, never passed.
Any entry that now SURVIVES fails the run (exit 1) and the record says which.

THE RECORD (governance/status/v022-resurvey.json) names the commit, the gate's digest, the counts per
family, every entry drawn with its result, the sample rule and the seconds taken. `--check` (and the
freeze manifest) refuse a record that is for another commit (modulo governance/status/), was made from
a dirty tree, is older than MAX_AGE_DAYS, drew fewer than MIN_PCT of the entries, covers a different
OBSERVED set than the gate now has, lists a survivor, or drew entries the rule does not draw.
"""
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
STATUS = "governance/status/v022-resurvey.json"
SCHEMA = "axon-v022-resurvey/1"
MAX_AGE_DAYS = 14
MIN_PCT = 25
BUDGET_S = 300           # `auto` re-measures everything when the estimate is under this
EST_SECONDS = {"value": 40, "py": 45, "guard": 40}
FLAKY = ["--skip", "a_callers_scheduling_state_never_reaches_the_root_launch"]


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, f"scripts/{name}.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT, **kw)


def head():
    return sh(["git", "rev-parse", "HEAD"]).stdout.strip()


def drawn(key, salt, pct):
    """Whether the entry `key` is in the sample: a fixed function of (salt, key, pct)."""
    return int(hashlib.sha256(f"{salt}|{key}".encode()).hexdigest()[:8], 16) % 100 < pct


def entries(rc):
    """{family: [(key, payload)]} of the gate's OBSERVED entries."""
    out = {"value": [], "py": [], "guard": []}
    for e in rc.VALUE_EXEMPT:
        if e[4] == "OBSERVED":
            out["value"].append((f"{e[0]}|{e[1]}|{e[2]}|{e[3]}", e))
    for e in rc.PY_EXEMPT:
        if e[4] == "OBSERVED":
            out["py"].append((f"{e[0]}|{e[1]}|{e[2]}", e))
    for f, anchor, reason in rc.EXEMPT:
        if reason.startswith("OBSERVED-NOT-ROWED"):
            out["guard"].append((f"{f}|{anchor.strip()[:80]}", (f, anchor, reason)))
    return out


def plan(rc, sample, salt):
    ents = entries(rc)
    total = sum(len(v) for v in ents.values())
    est = sum(len(v) * EST_SECONDS[k] for k, v in ents.items())
    if sample == "auto":
        pct = 100 if est <= BUDGET_S else MIN_PCT
    elif sample == "all":
        pct = 100
    else:
        pct = int(sample)
    chosen = {k: [(key, p) for key, p in v if pct >= 100 or drawn(key, salt, pct)] for k, v in ents.items()}
    return ents, chosen, pct, total, est


# ── the three families ───────────────────────────────────────────────────────

_BASE = {}


def run_tests(cmd):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT, timeout=2400)
    except subprocess.TimeoutExpired:
        return {"<hung>"}, "hung", 124
    out = r.stdout + r.stderr
    failing = set(re.findall(r"^---- (\S+) stdout ----$", out, re.M))
    failing |= {"<binary> " + t for t in re.findall(r"^\s+`(-p [^`]+)`$", out, re.M)}
    if r.returncode not in (0, 101) and not failing:
        failing.add(f"<rc {r.returncode}>")
    if "could not compile" in out:
        return failing, "build", r.returncode
    return failing, "ok", r.returncode


def baseline(cmd):
    k = tuple(cmd)
    if k not in _BASE:
        _BASE[k] = run_tests(cmd)[0]
    return _BASE[k]


def commands_for(crate, reason):
    """The test commands a VALUE entry's reason names, else the crate's suite."""
    cmds = []
    for t in re.findall(r"<binary> (-p \S+ (?:--lib|--bin \S+|--test \S+))", reason):
        cmds.append(["cargo", "test", "--no-fail-fast", *t.split(), "--", *FLAKY])
    if not cmds:
        cmds.append(["cargo", "test", "--no-fail-fast", "-p", crate, "--", *FLAKY])
    return list(dict.fromkeys(map(tuple, cmds)))


def value_family(rc, vs, chosen):
    out = []
    cache = {}
    for key, e in chosen:
        f, fn, n, frag, kind, reason = e
        rec = {"key": key, "family": "value"}
        if f not in cache:
            text = open(os.path.join(ROOT, f)).read()
            cache[f] = (text, rc.value_sites(text, rc.scope_regions(f, rc.code_lines(text), text, []), file=f))
        text, sites = cache[f]
        site = next(((a, b, label) for a, b, label, sfn, sn in sites if (sfn, sn) == (fn, n)), None)
        if site is None:
            rec["result"] = "NOT RE-MEASURED (the entry names no site any more)"
            out.append(rec)
            continue
        a, b, label = site
        new = vs.mutate(label, text[a:b], text[max(0, a - 40):a])
        if new is None or new == text[a:b]:
            rec["result"] = "NOT RE-MEASURED (no mutation rule for this value)"
            out.append(rec)
            continue
        crate = f.split("/")[1]
        killed, broke = [], False
        path = os.path.join(ROOT, f)
        for cmd in commands_for(crate, reason):
            cmd = list(cmd)
            base = baseline(cmd)
            open(path, "w").write(text[:a] + new + text[b:])
            try:
                failing, state, _ = run_tests(cmd)
            finally:
                open(path, "w").write(text)
            if state == "build":
                broke = True
                break
            if state == "hung":
                rec["result"] = "INCONCLUSIVE (hung)"
                break
            killed += sorted(failing - base)
            rec.setdefault("ran", []).append({"cmd": " ".join(cmd[:8]), "failing": sorted(failing)[:3], "base": sorted(base)[:3]})
            if killed:
                break
        else:
            pass
        if "result" not in rec:
            rec["result"] = ("NOT RE-MEASURED (the edit does not build)" if broke
                             else "KILLED" if killed else "SURVIVED")
            rec["failing"] = killed[:4]
        out.append(rec)
        print(rec["result"], key, flush=True)
    return out


def py_family(rc, chosen):
    """The Python guards: scripts/v022_py_guard_survey.py on the drawn sites' lines."""
    if not chosen:
        return []
    want = {(e[1], e[2]) for _, e in chosen}
    text = open(os.path.join(ROOT, "scripts/guest_build_env.py")).read()
    lines = [str(a + 1) for g, a, b, fn, n in rc.py_sites(text) if (fn, n) in want]
    out_json = os.path.join(os.environ.get("TMPDIR", "/tmp"), f"resurvey-py-{os.getpid()}.json")
    r = sh([sys.executable, "scripts/v022_py_guard_survey.py", "--json", out_json, *lines], timeout=7200)
    res = []
    by = {}
    try:
        for row in json.load(open(out_json)):
            by[(row["function"], row["n"])] = row
    except (OSError, ValueError):
        pass
    for key, e in chosen:
        row = by.get((e[1], e[2]))
        rec = {"key": key, "family": "py"}
        if row is None:
            rec["result"] = "NOT RE-MEASURED (the survey produced no row for it)"
        else:
            rec["result"] = "KILLED" if row["verdict"] == "KILLED" else "SURVIVED"
            rec["failing"] = row.get("cases", [])[:4]
        res.append(rec)
        print(rec["result"], key, flush=True)
    return res


def guard_family(rc, chosen):
    """A Rust guard exemption: an `.ok_or(..)?` replaced by `.unwrap_or_default()`, and the tests it names."""
    out = []
    for key, (f, anchor, reason) in chosen:
        rec = {"key": key, "family": "guard"}
        path = os.path.join(ROOT, f)
        text = open(path).read()
        i = text.find(anchor) if text.count(anchor) == 1 else -1
        m = re.search(r"\.ok_or(?:_else)?\(", anchor)
        names = re.search(r"test\(s\) (.*?)(?: \(|;)", reason)
        if i < 0 or not m or not names:
            rec["result"] = "NOT RE-MEASURED (the edit is not an ok_or replacement)"
            out.append(rec)
            print(rec["result"], key, flush=True)
            continue
        s = i + m.start()
        clean = text
        op = i + m.end() - 1
        close = rc._match_close(text, op)
        tail = text[close:close + 2]
        if not tail.startswith("?"):
            rec["result"] = "NOT RE-MEASURED (no `?` after the ok_or call)"
            out.append(rec)
            continue
        new = text[:s] + ".unwrap_or_default()" + text[close + 1:]
        test_names = [t.strip() for t in names.group(1).split(",") if t.strip()]
        crate = f.split("/")[1]
        cmd = ["cargo", "test", "--no-fail-fast", "-p", crate, "--", *FLAKY]
        base = baseline(cmd)
        open(path, "w").write(new)
        try:
            failing, state, _ = run_tests(cmd)
        finally:
            open(path, "w").write(text)
        if state == "build":
            rec["result"] = "NOT RE-MEASURED (the edit does not build)"
        elif state == "hung":
            rec["result"] = "INCONCLUSIVE (hung)"
        else:
            hit = sorted(t for t in failing - base if any(t.endswith(n.split("::")[-1]) for n in test_names))
            rec["result"] = "KILLED" if hit else "SURVIVED"
            rec["failing"] = hit[:4]
        out.append(rec)
        print(rec["result"], key, flush=True)
    return out


# ── the record ────────────────────────────────────────────────────────────────

def run(args):
    sample, salt, out, only = "auto", None, STATUS, None
    fams = ["value", "py", "guard"]
    it = iter(args)
    for a in it:
        if a == "--sample":
            sample = next(it)
        elif a == "--salt":
            salt = next(it)
        elif a == "--out":
            out = next(it)
        elif a == "--families":
            fams = next(it).split(",")
        elif a == "--only":      # debugging: keys containing this text (the record is then NOT a freeze record)
            only = next(it)
    if sh(["git", "status", "--porcelain", "--untracked-files=no"]).stdout.strip():
        sys.exit("refused: the tree is not clean; the survey edits sources in place and a record is evidence about a commit")
    rc, vs = load("v022_refusal_coverage"), load("v022_value_survey")
    salt = salt or head()
    ents, chosen, pct, total, est = plan(rc, sample, salt)
    if only:
        chosen = {k: [(key, p) for key, p in v if only in key] for k, v in ents.items()}
    t0 = time.time()
    results = []
    if "value" in fams:
        results += value_family(rc, vs, chosen["value"])
    if "py" in fams:
        results += py_family(rc, chosen["py"])
    if "guard" in fams:
        results += guard_family(rc, chosen["guard"])
    secs = round(time.time() - t0, 1)
    fam = {}
    for k in ("value", "py", "guard"):
        rs = [r for r in results if r["family"] == k]
        fam[k] = {"observed": len(ents[k]), "drawn": len(chosen[k]) if k in fams else 0,
                  "killed": sum(r["result"] == "KILLED" for r in rs),
                  "survived": [r["key"] for r in rs if r["result"] == "SURVIVED"],
                  "not_remeasured": sum(r["result"].startswith(("NOT RE-MEASURED", "INCONCLUSIVE")) for r in rs)}
    doc = {"schema": SCHEMA, "commit": head(), "tree_clean": True,
           "gate_sha256": hashlib.sha256(open(os.path.join(ROOT, "scripts/v022_refusal_coverage.py"), "rb").read()).hexdigest(),
           "sample": {"pct": pct, "salt": salt, "mode": sample,
                      "rule": "an entry is drawn when sha256(salt|key)[:8] mod 100 < pct"},
           "estimate_seconds": est, "seconds": secs, "finished_unix": int(time.time()),
           "host": os.uname().nodename, "families": fam, "entries": results}
    with open(os.path.join(ROOT, out), "w") as fh:
        json.dump(doc, fh, indent=1)
        fh.write("\n")
    surv = [k for f in fam.values() for k in f["survived"]]
    print(f"resurvey: {sum(f['drawn'] for f in fam.values())} of {total} OBSERVED entries re-measured at {pct}% in {secs}s "
          f"(estimate {est}s): " + ", ".join(f"{k} {v['killed']}/{v['drawn']} killed, {v['not_remeasured']} not re-measured"
                                              for k, v in fam.items()) + f" -> {out}")
    for k in surv:
        print("SURVIVED (the OBSERVED entry is stale):", k)
    return 1 if surv else 0


def problems(doc, head_sha, rc, mut, now=None):
    """Every defect that stops `doc` from being the re-survey record for the commit `head_sha`."""
    mstat = load("v022_mutation_status")
    out = []
    if not isinstance(doc, dict) or doc.get("schema") != SCHEMA:
        return [f"not a {SCHEMA} record"]
    out.extend(mstat.evidence_commit_problems(doc.get("commit"), head_sha, mut))
    if doc.get("tree_clean") is not True:
        out.append("it does not record a clean tree")
    now = now if now is not None else time.time()
    age = (now - doc.get("finished_unix", 0)) / 86400
    if not (0 <= age <= MAX_AGE_DAYS):
        out.append(f"it is {age:.1f} days old (the bound is {MAX_AGE_DAYS})")
    gate = hashlib.sha256(open(os.path.join(ROOT, "scripts/v022_refusal_coverage.py"), "rb").read()).hexdigest()
    if doc.get("gate_sha256") != gate:
        out.append("it was made by another version of the gate than this tree's")
    s = doc.get("sample") or {}
    if not isinstance(s.get("pct"), int) or s["pct"] < MIN_PCT:
        out.append(f"it drew {s.get('pct')!r}% of the entries; the freeze needs at least {MIN_PCT}%")
    ents = entries(rc)
    fam = doc.get("families") or {}
    for k, v in ents.items():
        f = fam.get(k) or {}
        if f.get("observed") != len(v):
            out.append(f"{k}: it covers {f.get('observed')} OBSERVED entries, the gate now has {len(v)}")
        want = [key for key, _ in v if s.get("pct", 0) >= 100 or drawn(key, s.get("salt", ""), s.get("pct", 0))]
        got = sorted(r["key"] for r in doc.get("entries", []) if r.get("family") == k)
        if got != sorted(want):
            out.append(f"{k}: the entries it re-measured are not the ones its sample rule draws "
                       f"({len(got)} recorded, {len(want)} drawn)")
        if f.get("survived"):
            out.append(f"{k}: {len(f['survived'])} OBSERVED entr(ies) now SURVIVE (first: {f['survived'][0]})")
        r_ok = sum(1 for r in doc.get("entries", []) if r.get("family") == k and r.get("result") == "KILLED")
        if f.get("killed") != r_ok:
            out.append(f"{k}: the family counts {f.get('killed')} killed, the entries show {r_ok}")
        if want and r_ok + int(f.get("not_remeasured", 0)) < len(want):
            out.append(f"{k}: {len(want) - r_ok - int(f.get('not_remeasured', 0))} drawn entr(ies) are neither killed nor listed as not re-measured")
    return out


def freeze_refusal(rc, mut, head_sha, path=STATUS):
    """(reason, doc): why a FREEZE at `head_sha` must refuse (None: it may proceed), and the record read.
    Used by scripts/v022_freeze_manifest.py, so the refusal is this function's and a test can call it."""
    try:
        doc = json.load(open(os.path.join(ROOT, path)))
    except (OSError, ValueError) as e:
        return (f"{path} cannot be read ({e}): the OBSERVED entries of the refusal-site gate were not "
                "re-measured for this head; run scripts/v022_resurvey.py --run (amendment 107)"), None
    bad = problems(doc, head_sha, rc, mut)
    if bad:
        return ("the re-survey record is not for this head or does not hold: " + "; ".join(bad[:6])
                + " (scripts/v022_resurvey.py --run, amendment 107)"), doc
    return None, doc


def check(args):
    path = args[0] if args else STATUS
    rc, mut = load("v022_refusal_coverage"), load("v022_g01_mutations")
    try:
        doc = json.load(open(os.path.join(ROOT, path)))
    except (OSError, ValueError) as e:
        print(f"resurvey: no usable record at {path}: {e}")
        return 1
    bad = problems(doc, head(), rc, mut)
    for b in bad:
        print("resurvey: " + b)
    if not bad:
        f = doc["families"]
        print(f"resurvey: a record for this head: {sum(v['drawn'] for v in f.values())} OBSERVED entries re-measured at "
              f"{doc['sample']['pct']}% in {doc['seconds']}s, none stale")
    return 1 if bad else 0


def main():
    a = sys.argv[1:]
    if a[:1] == ["--run"]:
        sys.exit(run(a[1:]))
    if a[:1] == ["--check"]:
        sys.exit(check(a[1:]))
    if a[:1] == ["--plan"]:
        rc = load("v022_refusal_coverage")
        sample = a[a.index("--sample") + 1] if "--sample" in a else "auto"
        ents, chosen, pct, total, est = plan(rc, sample, head())
        print(f"{total} OBSERVED entries ({', '.join(f'{k} {len(v)}' for k, v in ents.items())}); estimate {est}s; "
              f"sample {pct}%: {sum(len(v) for v in chosen.values())} drawn")
        return
    sys.exit(__doc__)


if __name__ == "__main__":
    main()
