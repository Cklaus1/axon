#!/usr/bin/env python3
"""Re-measure the OBSERVED entries of the refusal-site gate (C9 round 11 eqgate7 amendment 107, hardened by
round 12 eqgate8 amendment 110).

The gate's third disposition for a site is neither a row nor a checkable fact but a RECORDED
MEASUREMENT: a survey removed (or changed) the guard / value, and a named test failed. Hundreds of entries
rest on it, and nothing re-ran it: a test that was later deleted, weakened or renamed leaves the entry
standing and the claim wrong. This tool re-measures a deterministic SAMPLE of them and writes a record the
FREEZE refuses to proceed without.

    python3 scripts/v022_resurvey.py --run [--out PATH] [--sample auto|all|PCT] [--families value,py,guard]
    python3 scripts/v022_resurvey.py --check [PATH]      # is there a valid record for HEAD? (exit 1: no)
    python3 scripts/v022_resurvey.py --plan [--sample ..] # what would be re-measured, and the estimate

THE FREEZE PROCEDURE (also in governance/notes/v022-operator-runbook.md):
  1. check out the freeze head with a CLEAN tree;
  2. `python3 scripts/v022_resurvey.py --run` (on gpumaster; hours at the default sample);
  3. commit governance/status/v022-resurvey.json AND governance/status/v022-resurvey-logs/ (a status-only
     commit; the record is for the commit it was made at, and only governance/status/ may differ);
  4. `python3 scripts/v022_freeze_manifest.py` (the freeze). It refuses, naming the first defects, when the
     record is missing, for another commit or tree, made by another gate or tool version, drawn by a salt
     that is not the commit's, below the floor, or backed by a log that is missing or does not name the test.

THE SAMPLE IS NOT THE RECORD'S TO CHOOSE. The salt is sha256(commit | gate digest | tool version), computed
here and RECOMPUTED by `problems()` from the record's commit and this tree's gate: a record cannot name its
own salt (round 12: a ground salt whose 25 % draw held 1 of the 43 flow entries, against an honest ~11, passed
a check that read the salt from the record). The entries are ranked by sha256(salt|family|key) and the first N
are drawn, N = ceil(1.2 x FLOOR) where FLOOR = max(20, 25 % of the population): a count, not a per-entry coin,
so a lucky draw cannot be small.

WHAT IS MEASURED. A VALUE entry: the survey's own mutation of that value (v022_value_survey.mutate), then the
test binaries the entry's reason names (EVERY `<binary> -p P --test T` token, one command each) and then the
owning crate's suite; the entry holds when a test that passed on the unmutated tree now fails. A PYTHON guard:
scripts/v022_py_guard_survey.py on that site's line. A RUST guard: the `.ok_or(..)?` replaced by
`.unwrap_or_default()` and the tests the entry names; an entry whose edit is not that or does not compile is
counted NOT RE-MEASURED, never passed. Any entry that now SURVIVES fails the run (exit 1).

THE RECORD (governance/status/v022-resurvey.json, schema /2) names the commit, the commit's TREE hash, the
gate's digest and the tool version, the counts, every entry drawn with its result, and for each re-measured
entry the commands run, their exit codes, the test binaries, the sha256 of a log kept under
governance/status/v022-resurvey-logs/ and the failing tests the log must name. `problems()` refuses a record
that: is for another commit or tree, was made from a dirty tree, is older than MAX_AGE_DAYS, was made by another
gate or tool version, drew other entries than the rule draws, holds fewer than FLOOR re-measured entries, caps
NOT RE-MEASURED at 20 % of the draw and requires each to carry a reason, lists a survivor or an INCONCLUSIVE
entry, or backs a KILLED entry with a log that is missing, altered, outside the log directory, or does not
mention the failing test, the binaries and the exit codes the entry records.

AMENDMENT 115 (round 13): the log check also requires, for each KILLED cargo entry, that the log lives in
the directory named by the RECORD'S OWN commit (not another commit's), is reached by no symlink (realpath of the
directory and the file stay inside governance/status/v022-resurvey-logs/<commit12>/), is named NNN-<family>.log,
is not shared with another entry (distinct path AND distinct sha256), has a plausible size, names only
non-empty failing tests, carries the runner's own `---- NAME stdout ----` header for each one and a
`test result: FAILED` line with a failure in it, shows each recorded command directly above its recorded
`exit code: N`, and that a command exited non-zero (a log with `exit code: 0` kills nothing).

RESIDUAL, stated as the other validators state theirs: this is SELF-CONSISTENCY and REPRODUCIBILITY, not
authentication. Whoever runs the freeze can still fabricate the logs (they are text the runner writes); what the
checks add is that a forged record must now also forge a consistent log set for a derived sample, which a reviewer
can re-run entry by entry (`--run --only KEY` reproduces one) and which `--check` ties to the commit, tree and
gate that were frozen. A record that nobody re-runs proves only that it is consistent.
"""
import hashlib
import importlib.util
import json
import math
import os
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
STATUS = "governance/status/v022-resurvey.json"
LOG_DIR = "governance/status/v022-resurvey-logs"
SCHEMA = "axon-v022-resurvey/2"
TOOL_VERSION = 2          # bumped when the sampling rule, the floor or the log format changes
MAX_AGE_DAYS = 14
MIN_PCT = 25
FLOOR_ABS = 20            # at least this many entries are re-measured, whatever the population
FLOOR_FRAC = 0.25         # ... and at least this fraction of the population
OVERDRAW = 1.2            # N = ceil(OVERDRAW x FLOOR) entries are drawn, so NOT RE-MEASURED has room
NOT_REMEASURED_MAX = 0.2  # of the entries drawn
BUDGET_S = 300            # `auto` re-measures everything when the estimate is under this
EST_SECONDS = {"value": 40, "py": 45, "guard": 40}
FLAKY = ["--skip", "a_callers_scheduling_state_never_reaches_the_root_launch"]
LOG_CAP = 48_000          # bytes kept per entry log


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, f"scripts/{name}.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT, **kw)


def head():
    return sh(["git", "rev-parse", "HEAD"]).stdout.strip()


def tree_of(commit):
    return sh(["git", "rev-parse", f"{commit}^{{tree}}"]).stdout.strip()


def gate_sha():
    return hashlib.sha256(open(os.path.join(ROOT, "scripts/v022_refusal_coverage.py"), "rb").read()).hexdigest()


def derive_salt(commit, gate=None):
    """The sampling salt: a function of the COMMIT, the gate's digest and the tool version, and nothing else.
    `problems()` recomputes it; a record's own `salt` field is a claim it is checked against."""
    return hashlib.sha256(f"axon-resurvey-salt/{TOOL_VERSION}|{commit}|{gate or gate_sha()}".encode()).hexdigest()


def floor_for(pop):
    return min(pop, max(FLOOR_ABS, math.ceil(FLOOR_FRAC * pop)))


def sample_size(pop, pct):
    if pct >= 100:
        return pop
    return min(pop, math.ceil(max(floor_for(pop), pct / 100 * pop) * OVERDRAW))


def rank(salt, family, key):
    return hashlib.sha256(f"{salt}|{family}|{key}".encode()).hexdigest()


def draw(ents, salt, pct):
    """{family: [key]}: the first N entries (all families together) by sha256(salt|family|key)."""
    allk = [(rank(salt, f, key), f, key) for f, v in ents.items() for key, _ in v]
    n = sample_size(len(allk), pct)
    chosen = {f: [] for f in ents}
    for _, f, key in sorted(allk)[:n]:
        chosen[f].append(key)
    return chosen


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
    keys = draw(ents, salt, pct)
    chosen = {k: [(key, p) for key, p in v if key in set(keys[k])] for k, v in ents.items()}
    return ents, chosen, pct, total, est


# ── logs ──────────────────────────────────────────────────────────────────────

_INTEREST = re.compile(r"^(\s+Running |\s+Doc-tests |test result:|failures:|error: |.*\bFAILED\b|---- |thread '|.*panicked at |"
                       r"\s+`-p )")


def log_block(cmd, rc, out):
    """The part of one command's output a reviewer needs: what ran, every failure and its panic, the exit code."""
    keep = [l for l in out.split("\n") if _INTEREST.match(l)]
    # panic bodies: the two lines after each `---- name stdout ----`
    lines = out.split("\n")
    for i, l in enumerate(lines):
        if l.startswith("---- ") and l.endswith(" stdout ----"):
            keep.extend(lines[i + 1:i + 5])
    body = "\n".join(dict.fromkeys(keep))
    return f"$ {' '.join(cmd)}\nexit code: {rc}\n{body}\n"


def write_log(commit, idx, family, blocks):
    """Write one entry's log; returns (relative path, sha256)."""
    text = "".join(blocks)[:LOG_CAP]
    d = os.path.join(ROOT, LOG_DIR, commit[:12])
    os.makedirs(d, exist_ok=True)
    rel = f"{LOG_DIR}/{commit[:12]}/{idx:03d}-{family}.log"
    with open(os.path.join(ROOT, rel), "w") as fh:
        fh.write(text)
    return rel, hashlib.sha256(text.encode()).hexdigest()


# ── the three families ───────────────────────────────────────────────────────

_BASE = {}
_LAST = {}


def run_tests(cmd):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT, timeout=2400)
    except subprocess.TimeoutExpired:
        _LAST.update(tail="", rc=124, out="hung\n")
        return {"<hung>"}, "hung", 124
    out = r.stdout + r.stderr
    _LAST["tail"] = out[-600:]
    _LAST["rc"] = r.returncode
    _LAST["out"] = out
    failing = set(re.findall(r"^---- (\S+) stdout ----$", out, re.M))
    failing |= {"<binary> " + t for t in re.findall(r"^\s+`(-p [^`]+)`$", out, re.M)}
    if r.returncode not in (0, 101) and not failing:
        failing.add(f"<rc {r.returncode}>")
    if "could not compile" in out:
        return failing, "build", r.returncode
    if re.search(r"^error: no (?:test|bin) target named", out, re.M):
        return failing, "bad", r.returncode
    return failing, "ok", r.returncode


def baseline(cmd):
    k = tuple(cmd)
    if k not in _BASE:
        _BASE[k] = run_tests(cmd)[0]
    return _BASE[k]


def commands_for(crate, reason):
    """The test commands a VALUE entry's reason names (ONE per named binary: round 12 found a survey that ran
    only the last of two), then the crate's suite (a reason lists at most six tests, so the binaries it names
    may be a subset of the ones that kill)."""
    cmds = []
    for t in re.findall(r"<binary> (-p [\w-]+ (?:--lib|--bin [\w-]+|--test [\w-]+))", reason):
        cmds.append(["cargo", "test", "--no-fail-fast", *t.split(), "--", *FLAKY])
    cmds.append(["cargo", "test", "--no-fail-fast", "-p", crate, "--", *FLAKY])
    return list(dict.fromkeys(map(tuple, cmds)))


def _binaries(out):
    return sorted(set(re.findall(r"^\s+Running (\S+)", out, re.M))
                  | set(re.findall(r"^\s+`(-p [^`]+)`$", out, re.M)))


def value_family(rc, vs, chosen, logger):
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
        blocks, bins = [], set()
        path = os.path.join(ROOT, f)
        for cmd in commands_for(crate, reason):
            cmd = list(cmd)
            base = baseline(cmd)
            open(path, "w").write(text[:a] + new + text[b:])
            try:
                failing, state, _ = run_tests(cmd)
            finally:
                open(path, "w").write(text)
            if state in ("build", "bad"):
                broke = True
                break
            if state == "hung":
                rec["result"] = "INCONCLUSIVE (hung)"
                break
            killed += sorted(failing - base)
            blocks.append(log_block(cmd, _LAST.get("rc"), _LAST.get("out", "")))
            bins |= set(_binaries(_LAST.get("out", "")))
            rec.setdefault("ran", []).append({"cmd": " ".join(cmd[:8]), "failing": sorted(failing)[:3], "base": sorted(base)[:3], "rc": _LAST.get("rc")})
            if killed:
                break
        if "result" not in rec:
            rec["result"] = ("NOT RE-MEASURED (the edit does not build)" if broke
                             else "KILLED" if killed else "SURVIVED")
            rec["failing"] = killed[:4]
        if blocks:
            rec["binaries"] = sorted(bins)
            rec["log"], rec["log_sha256"] = logger(blocks)
        out.append(rec)
        print(rec["result"], key, flush=True)
    return out


def py_family(rc, chosen, logger):
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
            rec["binaries"] = ["scripts/v022_py_guard_survey.py"]
            rec["ran"] = [{"cmd": "scripts/v022_py_guard_survey.py --json", "failing": rec["failing"], "base": [], "rc": r.returncode}]
            rec["log"], rec["log_sha256"] = logger([
                f"$ python3 scripts/v022_py_guard_survey.py --json OUT {' '.join(lines)}\nexit code: {r.returncode}\n"
                + json.dumps(row, sort_keys=True)[:6000] + "\n"])
        res.append(rec)
        print(rec["result"], key, flush=True)
    return res


def guard_family(rc, chosen, logger):
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
            rec["binaries"] = _binaries(_LAST.get("out", ""))
            rec["ran"] = [{"cmd": " ".join(cmd[:8]), "failing": sorted(failing)[:3], "base": sorted(base)[:3], "rc": _LAST.get("rc")}]
            rec["log"], rec["log_sha256"] = logger([log_block(cmd, _LAST.get("rc"), _LAST.get("out", ""))])
        out.append(rec)
        print(rec["result"], key, flush=True)
    return out


# ── the record ────────────────────────────────────────────────────────────────

def run(args):
    sample, out, only = "auto", STATUS, None
    fams = ["value", "py", "guard"]
    it = iter(args)
    for a in it:
        if a == "--sample":
            sample = next(it)
        elif a == "--out":
            out = next(it)
        elif a == "--families":
            fams = next(it).split(",")
        elif a == "--only":      # debugging: keys containing this text (the record is then NOT a freeze record)
            only = next(it)
        else:
            sys.exit(__doc__)    # no --salt: the salt is derived, never chosen
    if sh(["git", "status", "--porcelain", "--untracked-files=no"]).stdout.strip():
        sys.exit("refused: the tree is not clean; the survey edits sources in place and a record is evidence about a commit")
    rc, vs = load("v022_refusal_coverage"), load("v022_value_survey")
    commit = head()
    salt = derive_salt(commit)
    ents, chosen, pct, total, est = plan(rc, sample, salt)
    if only:
        chosen = {k: [(key, p) for key, p in v if only in key] for k, v in ents.items()}
    ctr = [0]
    d = os.path.join(ROOT, LOG_DIR, commit[:12])
    if os.path.isdir(d):
        for fn_ in os.listdir(d):
            os.unlink(os.path.join(d, fn_))

    def logger(family):
        def f(blocks):
            ctr[0] += 1
            return write_log(commit, ctr[0], family, blocks)
        return f

    t0 = time.time()
    results = []
    if "value" in fams:
        results += value_family(rc, vs, chosen["value"], logger("value"))
    if "py" in fams:
        results += py_family(rc, chosen["py"], logger("py"))
    if "guard" in fams:
        results += guard_family(rc, chosen["guard"], logger("guard"))
    secs = round(time.time() - t0, 1)
    fam = {}
    for k in ("value", "py", "guard"):
        rs = [r for r in results if r["family"] == k]
        fam[k] = {"observed": len(ents[k]), "drawn": len(chosen[k]) if k in fams else 0,
                  "killed": sum(r["result"] == "KILLED" for r in rs),
                  "survived": [r["key"] for r in rs if r["result"] == "SURVIVED"],
                  "not_remeasured": sum(r["result"].startswith(("NOT RE-MEASURED", "INCONCLUSIVE")) for r in rs)}
    doc = {"schema": SCHEMA, "tool_version": TOOL_VERSION, "commit": commit, "tree_sha": tree_of(commit), "tree_clean": True,
           "gate_sha256": gate_sha(), "partial": bool(only) or sorted(fams) != ["guard", "py", "value"],
           "sample": {"pct": pct, "salt": salt, "mode": sample, "population": total, "floor": floor_for(total),
                      "n": sample_size(total, pct),
                      "rule": "salt = sha256(axon-resurvey-salt/V|commit|gate sha256); entries ranked by "
                              "sha256(salt|family|key); the first N are drawn"},
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


MIN_LOG_BYTES = {"value": 300, "guard": 300, "py": 120}   # a log of a real run is longer than three lines
_LOG_NAME = re.compile(r"\d{3,}-(value|py|guard)\.log")


def _log_path_problems(r, logs_root, commit):
    """(problems, text): is the log where the record says, inside THIS commit's log directory and not reached
    through a symlink (amendment 115: a log filed under another commit's directory, a symlinked log
    directory and `..` all passed the old prefix test)."""
    rel = r.get("log")
    c12 = commit[:12] if isinstance(commit, str) else ""
    want_dir = f"{LOG_DIR}/{c12}/"
    if not isinstance(rel, str) or not c12 or not rel.startswith(want_dir) or ".." in rel.split("/"):
        return [f"has no log under {want_dir} (the directory is named by the record's own commit)"], None
    name = rel[len(want_dir):]
    m = _LOG_NAME.fullmatch(name)
    if not m or m.group(1) != r.get("family"):
        return [f"its log {rel} is not named NNN-{r.get('family')}.log"], None
    base = os.path.realpath(logs_root)
    expect_dir = os.path.join(base, LOG_DIR, c12)
    path = os.path.join(logs_root, rel)
    if os.path.realpath(os.path.dirname(path)) != expect_dir or os.path.islink(os.path.join(logs_root, LOG_DIR, c12)) \
            or os.path.islink(path) or os.path.realpath(path) != os.path.join(expect_dir, name):
        return [f"its log {rel} is reached through a symlink or leaves {want_dir} (realpath check)"], None
    try:
        return [], open(path).read()
    except OSError:
        return [f"its log {rel} is missing"], None


def _log_problems(r, logs_root, commit=None):
    """Why a KILLED entry's log does not back it (empty: it does). Beyond the old text-contains checks
    (amendment 115): the log is where the record's commit puts it and not behind a symlink; it has a plausible
    size; a cargo log carries the runner's own lines (`test result: FAILED`, the `---- NAME stdout ----` header
    of EVERY named failing test, the `$ command` line directly above its `exit code: N`), and a KILLED entry has
    a command that exited non-zero (a passing command cannot have killed anything). What this CANNOT do: tell a
    log the runner wrote from one a person typed in the same shape; see the module docstring's RESIDUAL."""
    bad, text = _log_path_problems(r, logs_root, commit)
    if text is None:
        return bad
    rel = r.get("log")
    out = list(bad)
    fam = r.get("family")
    if hashlib.sha256(text.encode()).hexdigest() != r.get("log_sha256"):
        out.append(f"its log {rel} does not match the sha256 the record holds (altered)")
    if len(text) < MIN_LOG_BYTES.get(fam, 300):
        out.append(f"its log {rel} is {len(text)} bytes, too short to be the output of a run")
    failing = r.get("failing") or []
    if not failing:
        out.append("is KILLED but names no failing test")
    if not all(isinstance(t, str) and t.strip() for t in failing):
        out.append("names a failing test that is empty or not a string")
    ran = r.get("ran") or []
    if not ran:
        out.append("records no command it ran")
    cargo = fam in ("value", "guard")
    for t in failing:
        if not isinstance(t, str) or not t.strip():
            continue
        if t not in text:
            out.append(f"its log {rel} does not mention the failing test {t!r}")
        elif cargo and not t.startswith("<") and f"---- {t} stdout ----" not in text:
            out.append(f"its log {rel} has no `---- {t} stdout ----` header: the runner did not report that test failing")
        elif cargo and t.startswith("<binary> ") and f"`{t[len('<binary> '):]}`" not in text:
            out.append(f"its log {rel} does not show the runner's failed-binary line for {t!r}")
    if fam == "py" and '"verdict": "KILLED"' not in text:
        out.append(f"its log {rel} does not carry the survey's own KILLED verdict")
    if cargo:
        if not re.search(r"^test result: FAILED\. \d+ passed; [1-9]\d* failed;", text, re.M) \
                and not all(isinstance(t, str) and t.startswith("<") for t in failing):
            out.append(f"its log {rel} has no `test result: FAILED` line with a failure in it")
        if not any(isinstance(c.get("rc"), int) and c["rc"] != 0 for c in ran):
            out.append("is KILLED but no command it ran exited non-zero (a passing run kills nothing)")
        if not re.search(r"^\s+Running \S+", text, re.M) and not re.search(r"^\s+`-p [^`]+`$", text, re.M):
            out.append(f"its log {rel} has no cargo `Running <binary>` line")
    for c in ran:
        rc_ = c.get("rc")
        cmd = str(c.get("cmd", ""))
        if not isinstance(rc_, int) or f"exit code: {rc_}" not in text:
            out.append(f"its log {rel} does not show the exit code {rc_!r} of `{cmd[:60]}`")
        elif cmd and not re.search(r"^\$ (?:python3 )?" + re.escape(cmd[:60]) + r".*\nexit code: " + str(rc_) + r"$", text, re.M):
            out.append(f"its log {rel} does not show `{cmd[:60]}` followed directly by `exit code: {rc_}`")
        if cargo and rc_ == 0 and c.get("failing"):
            out.append(f"records failing tests for `{cmd[:60]}` that exited 0")
        if cmd[:60] not in text:
            out.append(f"its log {rel} does not show the command `{cmd[:60]}`")
    for b in r.get("binaries") or []:
        if b not in text:
            out.append(f"its log {rel} does not name the test binary {b!r}")
    return out


def problems(doc, head_sha, rc, mut, now=None, logs_root=ROOT, tree_fn=tree_of):
    """Every defect that stops `doc` from being the re-survey record for the commit `head_sha`."""
    mstat = load("v022_mutation_status")
    out = []
    if not isinstance(doc, dict) or doc.get("schema") != SCHEMA:
        return [f"not a {SCHEMA} record"]
    out.extend(mstat.evidence_commit_problems(doc.get("commit"), head_sha, mut))
    if doc.get("tree_clean") is not True:
        out.append("it does not record a clean tree")
    if doc.get("partial"):
        out.append("it is a PARTIAL run (--only or --families), not a freeze record")
    if doc.get("tool_version") != TOOL_VERSION:
        out.append(f"it was made by tool version {doc.get('tool_version')!r}, this tree's is {TOOL_VERSION}")
    if not isinstance(doc.get("commit"), str) or not doc.get("tree_sha") or doc.get("tree_sha") != tree_fn(doc["commit"]):
        out.append("its tree hash is not the tree of its commit (a record is bound to the tree it was made on)")
    now = now if now is not None else time.time()
    age = (now - doc.get("finished_unix", 0)) / 86400
    if not (0 <= age <= MAX_AGE_DAYS):
        out.append(f"it is {age:.1f} days old (the bound is {MAX_AGE_DAYS})")
    gate = gate_sha()
    if doc.get("gate_sha256") != gate:
        out.append("it was made by another version of the gate than this tree's")
    s = doc.get("sample") or {}
    if not isinstance(s.get("pct"), int) or s["pct"] < MIN_PCT:
        out.append(f"it drew {s.get('pct')!r}% of the entries; the freeze needs at least {MIN_PCT}%")
    salt = derive_salt(doc.get("commit"), doc.get("gate_sha256"))
    if s.get("salt") != salt:
        out.append("its sampling salt is not the one derived from its commit and its gate (a record cannot choose its own sample)")
    ents = entries(rc)
    pop = sum(len(v) for v in ents.values())
    pct = s["pct"] if isinstance(s.get("pct"), int) else MIN_PCT
    want_by = draw(ents, salt, max(pct, MIN_PCT)) if pop else {k: [] for k in ents}
    fam = doc.get("families") or {}
    killed_total = nrm_total = drawn_total = 0
    for k, v in ents.items():
        f = fam.get(k) or {}
        if f.get("observed") != len(v):
            out.append(f"{k}: it covers {f.get('observed')} OBSERVED entries, the gate now has {len(v)}")
        want = want_by[k]
        recs = [r for r in doc.get("entries", []) if r.get("family") == k]
        got = sorted(r.get("key") for r in recs)
        if got != sorted(want):
            out.append(f"{k}: the entries it re-measured are not the ones the rule draws "
                       f"({len(got)} recorded, {len(want)} drawn)")
        if f.get("survived"):
            out.append(f"{k}: {len(f['survived'])} OBSERVED entr(ies) now SURVIVE (first: {f['survived'][0]})")
        r_ok = sum(1 for r in recs if r.get("result") == "KILLED")
        if f.get("killed") != r_ok:
            out.append(f"{k}: the family counts {f.get('killed')} killed, the entries show {r_ok}")
        nrm = [r for r in recs if str(r.get("result", "")).startswith("NOT RE-MEASURED")]
        if f.get("not_remeasured") != len(nrm):
            out.append(f"{k}: the family counts {f.get('not_remeasured')} not re-measured, the entries show {len(nrm)}")
        for r in recs:
            res = str(r.get("result"))
            if res == "KILLED":
                for why in _log_problems(r, logs_root, doc.get("commit")):
                    out.append(f"{k}: entry {str(r.get('key'))[:70]!r} {why}")
            elif res.startswith("NOT RE-MEASURED"):
                if not re.fullmatch(r"NOT RE-MEASURED \([^()]{8,}\)", res):
                    out.append(f"{k}: entry {str(r.get('key'))[:70]!r} is NOT RE-MEASURED with no reason a reviewer can read")
            else:
                out.append(f"{k}: entry {str(r.get('key'))[:70]!r} has the result {res[:40]!r}, which is neither KILLED nor "
                           "NOT RE-MEASURED (a survivor, or a run that did not conclude)")
        killed_total += r_ok
        nrm_total += len(nrm)
        drawn_total += len(recs)
    # every KILLED entry has a log of its OWN: one shared log, or one log filed twice, backs nothing
    logs = [(r.get("log"), r.get("log_sha256")) for r in doc.get("entries", []) if r.get("result") == "KILLED"]
    if len({p for p, _ in logs}) != len(logs) or len({h for _, h in logs}) != len(logs):
        out.append("two KILLED entries share a log (or a log's bytes): each entry needs its own run's log")
    floor = floor_for(pop)
    if killed_total < floor:
        out.append(f"only {killed_total} entries were re-measured and held; the floor is {floor} "
                   f"(max({FLOOR_ABS}, {int(FLOOR_FRAC * 100)}% of {pop}))")
    if nrm_total > math.floor(NOT_REMEASURED_MAX * max(drawn_total, 1)):
        out.append(f"{nrm_total} of {drawn_total} drawn entries are NOT RE-MEASURED; the cap is {int(NOT_REMEASURED_MAX * 100)}%")
    return out


def freeze_refusal(rc, mut, head_sha, path=STATUS, logs_root=ROOT):
    """(reason, doc): why a FREEZE at `head_sha` must refuse (None: it may proceed), and the record read.
    Used by scripts/v022_freeze_manifest.py, so the refusal is this function's and a test can call it."""
    try:
        doc = json.load(open(os.path.join(ROOT, path)))
    except (OSError, ValueError) as e:
        return (f"{path} cannot be read ({e}): the OBSERVED entries of the refusal-site gate were not "
                "re-measured for this head; run scripts/v022_resurvey.py --run and commit "
                f"{STATUS} with {LOG_DIR}/ (amendment 110)"), None
    bad = problems(doc, head_sha, rc, mut, logs_root=logs_root)
    if bad:
        return ("the re-survey record is not for this head or does not hold: " + "; ".join(bad[:6])
                + " (scripts/v022_resurvey.py --run, amendment 110)"), doc
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
        print(f"resurvey: a record for this head: {sum(v['drawn'] for v in f.values())} OBSERVED entries drawn at "
              f"{doc['sample']['pct']}% (floor {doc['sample']['floor']}), "
              f"{sum(v['killed'] for v in f.values())} re-measured and held, in {doc['seconds']}s, none stale")
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
        ents, chosen, pct, total, est = plan(rc, sample, derive_salt(head()))
        print(f"{total} OBSERVED entries ({', '.join(f'{k} {len(v)}' for k, v in ents.items())}); estimate {est}s; "
              f"sample {pct}%: {sum(len(v) for v in chosen.values())} drawn (floor {floor_for(total)})")
        return
    sys.exit(__doc__)


if __name__ == "__main__":
    main()
