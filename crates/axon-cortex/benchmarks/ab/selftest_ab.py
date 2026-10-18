#!/usr/bin/env python3
"""Self-tests of the protected-set machinery (`../protected.py`) and of
`run_ab.py`'s grading and trial additions (followup-spec FU-1d, FU-1e, FU-1g,
FU-8).

Binary-free: every `axon`, `cortex` and `micode` that `run_ab.py` spawns here is
a fake script written to a temp dir and named to it through `AXON_BIN`,
`CORTEX_BIN` and `PATH`; nothing under `target/` is ever run. With
`RUN_AB_SELFTEST_AXON=<a built axon>` the caps_strict case also runs on that
real binary (`axon caps --json --fn`); without it that one case is skipped and
says so.

    python3 -B selftest_ab.py        (exit 0 pass, 1 fail)
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import traceback

HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, BENCH)
RUN_AB = os.path.join(HERE, "run_ab.py")
HARD_ONCE = os.path.join(BENCH, "hardset", "hard-once.json")
REAL_AXON = os.environ.get("RUN_AB_SELFTEST_AXON")

# A fake `axon`: `check` passes, `test --json [--completion-key-stdin]` reports
# every @[test] as ok, `caps --json --fn F` reports `fs:read` iff F's body calls
# read_file. FAKE_AXON_MODE=untokened drops the completion token, badtoken signs
# with the wrong key: an axon whose verdict carries no valid HMAC.
FAKE_AXON = r'''#!%(py)s
import hashlib, hmac, json, os, re, sys
mode = os.environ.get("FAKE_AXON_MODE", "honest")
args = sys.argv[1:]
path = [a for a in args if a.endswith(".ax")][-1] if any(a.endswith(".ax") for a in args) else None
src = open(path).read() if path else ""
def span(name):
    m = re.search(r"\bfn %%s\(" %% re.escape(name), src)
    if not m:
        return None
    o, d = src.find("{", m.start()), 0
    for i in range(o, len(src)):
        d += {"{": 1, "}": -1}.get(src[i], 0)
        if d == 0:
            return src[o:i]
if args[0] == "check":
    sys.exit(0)
if args[0] == "caps":
    fn = args[args.index("--fn") + 1]
    body = span(fn)
    if body is None:
        sys.exit(2)
    caps = ["fs:read"] if "read_file(" in body else []
    print(json.dumps({"schema": "axon-caps/1", "file": path, "fn": fn, "capabilities": caps,
                      "effects": ["IO"] if caps else [], "unknown": False}))
    sys.exit(0)
if args[0] == "test":
    names = re.findall(r"@\[test[^\]]*\]\s*fn\s+(\w+)", src)
    key = None
    if "--completion-key-stdin" in args:
        key = bytes.fromhex(sys.stdin.readline().strip())
        if mode == "badtoken":
            key = b"\0" * 32
    for n in names:
        row = {"name": n, "status": "ok", "duration_ms": 0}
        if key is not None and mode != "untokened":
            row["completion"] = hmac.new(key, b"axon-test-completion/1\0" + n.encode(),
                                         hashlib.sha256).hexdigest()
        print(json.dumps(row))
    print(json.dumps({"type": "summary", "total": len(names), "passed": len(names), "failed": 0,
                      "skipped": 0, "duration_ms": 0}))
    sys.exit(0)
sys.exit(2)
'''

# A fake `cortex`: every invocation is recorded in FAKE_MARKER (it stands for a
# model call). `repair` writes nothing and reports FAKE_CORTEX_MODE: exhausted
# (exit 1, accepted null), mismatch (exit 0, accepted names other bytes), null
# (exit 0, accepted null).
FAKE_CORTEX = r'''#!%(py)s
import json, os, sys
open(os.environ["FAKE_MARKER"], "a").write(
    "cortex " + " ".join(sys.argv[1:2]) + " ranker=%%s audit=%%s\n"
    %% (os.environ.get("CORTEX_RANKER"), os.environ.get("CORTEX_AUDIT")))
mode = os.environ.get("FAKE_CORTEX_MODE", "exhausted")
if sys.argv[1] == "locate":
    print(json.dumps({"schema": "cortex-locate/1", "ranked": []}))
    sys.exit(0)
v = {"schema": "cortex-repair/1", "outcome": "exhausted", "attempted": [], "accepted": None}
if mode == "mismatch":
    v.update(outcome="verified_done", accepted={"path": "x", "sha256": "0" * 64})
    print(json.dumps(v)); sys.exit(0)
if mode == "null":
    v.update(outcome="verified_done")
    print(json.dumps(v)); sys.exit(0)
print(json.dumps(v)); sys.exit(1)
'''

# A fake `micode` on PATH: run_ab asks it for --version on paid micode runs.
FAKE_MICODE = r'''#!%(py)s
import os, sys
open(os.environ["FAKE_MARKER"], "a").write("micode " + " ".join(sys.argv[1:]) + "\n")
print("micode 0.0.0-fake")
'''

SRC = """fn add1(x: i64) -> i64 {
    x + 1
}

fn dbl(x: i64) -> i64 {
    x * 2
}

@[test]
fn t_a() {
    assert_eq(add1(1), 2)
}

@[test]
fn t_b() {
    assert_eq(add1(2), 3)
}

@[test]
fn t_c() {
    assert_eq(dbl(2), 4)
}

@[test]
fn t_d() {
    assert_eq(dbl(3), 6)
}
"""


class Env:
    """One temp dir holding the fakes, a synthetic corpus and a ledger."""

    def __init__(self, root):
        self.root = root
        self.bin = os.path.join(root, "bin")
        os.makedirs(self.bin)
        self.axon = self._script("axon", FAKE_AXON)
        self.cortex = self._script("cortex", FAKE_CORTEX)
        self._script("micode", FAKE_MICODE)
        self.marker = os.path.join(root, "marker")
        self.ledger = os.path.join(root, "spent.jsonl")
        open(self.ledger, "w").close()
        self.spend_root = os.path.join(root, "spend")
        os.makedirs(self.spend_root)
        self.corpus = os.path.join(root, "synthetic.json")
        write_corpus(self.corpus)
        self.once_copy = os.path.join(root, "once-copy.json")
        shutil.copyfile(HARD_ONCE, self.once_copy)

    def _script(self, name, text):
        p = os.path.join(self.bin, name)
        with open(p, "w") as fh:
            fh.write(text % {"py": sys.executable})
        os.chmod(p, 0o755)
        return p

    def run(self, *args, **env):
        e = {k: v for k, v in os.environ.items()
             if k not in ("AXON_BIN", "CORTEX_BIN", "FAKE_AXON_MODE", "FAKE_CORTEX_MODE")}
        e.update(AXON_BIN=self.axon, CORTEX_BIN=self.cortex, FAKE_MARKER=self.marker,
                 PATH=self.bin + os.pathsep + os.environ.get("PATH", ""))
        e.update(env)
        return subprocess.run([sys.executable, "-B", RUN_AB, *args], capture_output=True,
                              text=True, env=e, timeout=300)

    def marked(self):
        return open(self.marker).read() if os.path.exists(self.marker) else ""


def write_corpus(path):
    import corpus as corpus_mod
    from common import fn_span
    tasks = []
    for tid, sym, vis, hid, mutant in (("s1", "add1", "t_a", "t_b", "\n    x - 1\n"),
                                       ("s2", "dbl", "t_c", "t_d", "\n    x * 3\n")):
        a, b = fn_span(SRC, sym)
        broken = SRC[:a] + mutant + SRC[b:]
        given_a, hidden_block = corpus_mod.cut_test(broken, hid)
        texts = {"pristine": SRC, "broken": broken, "given_a": given_a,
                 "hidden_block": hidden_block, "oracle_body": SRC[a:b]}
        tasks.append({"id": tid, "class": "operator", "source_file": "selftest/s.ax",
                      "file": "s.ax", "symbol": sym, "visible": vis, "hidden": hid,
                      "mutant_body": mutant,
                      "sha256": {k: hashlib.sha256(texts[k].encode()).hexdigest()
                                 for k in corpus_mod.DERIVED}})
    with open(path, "w") as fh:
        json.dump({"meta": {"sources": {"selftest/s.ax": SRC}}, "tasks": tasks}, fh)


def rows(path):
    if not os.path.exists(path):
        return []
    return [json.loads(l) for l in open(path) if l.strip()]


def ledger_rows(path):
    return [json.loads(l) for l in open(path) if l.strip()]


PAID_B = ("--arm", "B", "--generator", "real", "--transport", "micode",
          "--model", "claude-haiku-5-5", "--reserve-usd", "0.01", "--cap-usd", "1")


def head():
    return subprocess.run(["git", "-C", BENCH, "rev-parse", "HEAD"], capture_output=True,
                          text=True).stdout.strip()


# ---------------------------------------------------------------- cases

def case_pins(env):
    import protected
    expect(protected.PROTECTED_SETS["hard-once"] == protected.sha256_file(HARD_ONCE),
           "the committed hard-once corpus does not match its pin")
    expect("hx-test" in protected.PROTECTED_SETS, "hx-test is not a protected set")
    expect(protected.protected_set(env.once_copy) == "hard-once",
           "a copy of hard-once (same sha256, other path) is not recognised as protected")
    expect(protected.protected_set(env.corpus) is None, "an unprotected corpus reads as protected")
    # A bench dir whose hard-once file no longer matches the pin.
    bench = os.path.join(env.root, "bench")
    os.makedirs(os.path.join(bench, "hardset"))
    tampered = os.path.join(bench, "hardset", "hard-once.json")
    d = json.load(open(HARD_ONCE))
    d["tasks"] = d["tasks"][:-1]
    json.dump(d, open(tampered, "w"))
    expect(raises(lambda: protected.protected_set(tampered, bench=bench), "pin"),
           "a protected file that no longer matches its pin was not refused")
    expect(raises(lambda: protected.check(bench=bench, ledger=env.ledger, root=None), "pin"),
           "protected.check passed a pin mismatch")


def _row(env, purpose="p1", **kw):
    r = {"set": "hard-once", "corpus_sha256": "c" * 64, "purpose": purpose,
         "repair_digest": "d" * 64, "patch_sha256": None, "cortex_sha256": "1" * 64,
         "axon_sha256": "2" * 64, "grade_axon_sha256": "3" * 64, "generator": "real/micode",
         "model": "m", "k": 1, "out": f"out-{purpose}.jsonl", "git_head": "h" * 40, "arm": "B"}
    r.update(kw)
    return r


def case_chain(env):
    import protected
    led = os.path.join(env.root, "chain.jsonl")
    open(led, "w").close()
    expect(protected.read_ledger(led) == [], "an empty ledger does not verify")
    s1 = protected.spend(led, _row(env, "p1"))
    s2 = protected.spend(led, _row(env, "p2"))
    s3 = protected.spend(led, _row(env, "p3"))
    got = protected.read_ledger(led)
    expect([protected.row_sha(r) for r in got] == [s1, s2, s3], "row shas differ from spend()'s")
    expect(got[0]["prev_row_sha256"] is None and got[1]["prev_row_sha256"] == s1,
           "rows are not chained")
    lines = open(led).read().splitlines()
    for what, bad in (("edited middle row", [lines[0], lines[1].replace('"p2"', '"pX"'), lines[2]]),
                      ("deleted middle row", [lines[0], lines[2]]),
                      ("swapped rows", [lines[1], lines[0], lines[2]]),
                      ("garbage line", lines + ["{not json"])):
        with open(led, "w") as fh:
            fh.write("\n".join(bad) + "\n")
        expect(raises(lambda: protected.read_ledger(led), "ledger"), f"chain tamper not detected: {what}")
    expect(raises(lambda: protected.read_ledger(os.path.join(env.root, "absent.jsonl")), "ledger"),
           "a missing ledger verifies")


def case_duplicate_spend(env):
    import protected
    led = os.path.join(env.root, "dup.jsonl")
    open(led, "w").close()
    s1 = protected.spend(led, _row(env, "baseline"))
    expect(raises(lambda: protected.spend(led, _row(env, "baseline", out="o2.jsonl")), "already spent"),
           "a second spend of the same (set, repair_digest, patch, model, purpose) was not refused")
    protected.spend(led, _row(env, "candidate"))
    r = protected.spend(led, _row(env, "baseline", out="o3.jsonl"), rerun_of=s1, reason="infra")
    expect(protected.read_ledger(led)[-1]["rerun_of"] == s1, "rerun row does not name its original")
    expect(raises(lambda: protected.spend(led, _row(env, "baseline", out="o4.jsonl"), rerun_of=s1,
                                          reason="again"), "latest"),
           "a rerun of a row that was already rerun was not refused")
    expect(raises(lambda: protected.spend(led, _row(env, "baseline", out="o5.jsonl"), rerun_of=r,
                                          reason=""), "reason"), "a rerun without a reason passed")
    # Resume: identity must match, and the out file must be short of n*k rows.
    ident = {k: v for k, v in _row(env, "candidate").items() if k != "git_head"}
    cand = protected.read_ledger(led)[1]
    sha = protected.row_sha(cand)
    expect(protected.resume(led, sha, ident, done=3, total=19)["purpose"] == "candidate",
           "resume of a short run refused")
    expect(raises(lambda: protected.resume(led, sha, ident, done=19, total=19), "complete"),
           "resume of a complete run passed")
    expect(raises(lambda: protected.resume(led, sha, dict(ident, cortex_sha256="9" * 64), 3, 19),
                  "cortex_sha256"), "resume with another cortex binary passed")
    expect(raises(lambda: protected.resume(led, "f" * 64, ident, 3, 19), "no ledger row"),
           "resume of an unknown row passed")


def case_unspent_paid_refused(env):
    out = os.path.join(env.root, "unspent", "b.jsonl")
    p = env.run(env.once_copy, *PAID_B, "--out", out, "--spend-root", env.spend_root,
                "--ledger", env.ledger, "--cortex-commit", head())
    expect(p.returncode != 0 and "PROTECTED SET hard-once" in p.stderr and "--spend" in p.stderr,
           f"an unspent paid run on a protected corpus was not refused: rc {p.returncode} "
           f"{p.stderr[-400:]}")
    expect(env.marked() == "", f"a model-side process ran before the refusal: {env.marked()!r}")
    expect(not os.path.exists(out) and ledger_rows(env.ledger) == [],
           "the refused run wrote rows or a ledger row")
    # A dry --spend with --cap-usd 0 refuses before any call and appends nothing.
    p = env.run(env.once_copy, *PAID_B[:-2], "--cap-usd", "0", "--spend", "dry", "--out", out,
                "--spend-root", env.spend_root, "--ledger", env.ledger, "--cortex-commit", head())
    expect(p.returncode != 0 and "BUDGET CAP" in p.stderr,
           f"--cap-usd 0 --spend did not refuse: rc {p.returncode} {p.stderr[-400:]}")
    expect(env.marked() == "" and ledger_rows(env.ledger) == [] and not os.path.exists(out),
           "--cap-usd 0 --spend ran something or appended a ledger row")


def case_oracle_writes_nothing(env):
    out = os.path.join(env.root, "oracle", "a.jsonl")
    ids = json.load(open(env.once_copy))["tasks"][0]["id"]
    for arm in (("--arm", "A", "--agent", "oracle"), ("--arm", "B", "--generator", "none")):
        p = env.run(env.once_copy, *arm, "--only", ids, "--out", out, "--ledger", env.ledger)
        expect(p.returncode == 0, f"{arm} on a protected corpus refused: {p.stderr[-400:]}")
        expect(not os.path.exists(out), f"{arm} on a protected corpus wrote {out}")
        expect(ledger_rows(env.ledger) == [], f"{arm} on a protected corpus wrote a ledger row")
    p = env.run(env.once_copy, "--arm", "A", "--agent", "oracle", "--only", ids, "--out", out,
                "--ledger", env.ledger, "--spend", "x")
    expect(p.returncode != 0 and "spends nothing" in p.stderr,
           "--spend on a zero-cost control was accepted")


def case_control_makes_no_model_call(env):
    """IA-1: cortex's ranker and audit default to OpenRouter when the env does
    not name them; a zero-cost arm-B control on a protected set (not ledgered,
    rows not written) must run with the spectrum ranker and the audit off,
    whatever it inherits, and a model ranker on it is refused."""
    ids = json.load(open(env.once_copy))["tasks"][0]["id"]
    out = os.path.join(env.root, "control", "b.jsonl")
    model = "openrouter:anthropic/claude-haiku-5.5"
    for gen in ("oracle", "none"):
        for inherited in ("", model):
            open(env.marker, "w").close()
            p = env.run(env.once_copy, "--arm", "B", "--generator", gen, "--only", ids,
                        "--out", out, "--ledger", env.ledger,
                        OPENROUTER_API_KEY="sk-selftest-not-a-key",
                        CORTEX_RANKER=inherited, CORTEX_AUDIT=inherited)
            calls = [l for l in env.marked().splitlines() if l.startswith("cortex repair")]
            expect(p.returncode == 0 and calls
                   and all(l.endswith("ranker=spectrum audit=off") for l in calls),
                   f"a {gen} control on a protected set inheriting CORTEX_RANKER={inherited!r} "
                   f"ran cortex as {calls} (rc {p.returncode} {p.stderr[-300:]})")
    open(env.marker, "w").close()
    p = env.run(env.once_copy, "--arm", "B", "--generator", "none", "--only", ids, "--out", out,
                "--ledger", env.ledger, "--ranker", model, OPENROUTER_API_KEY="sk-selftest-not-a-key")
    expect(p.returncode != 0 and "PROTECTED SET hard-once" in p.stderr and "--ranker" in p.stderr,
           f"a model ranker on a protected-set control was not refused: rc {p.returncode} "
           f"{p.stderr[-300:]}")
    expect(env.marked() == "", f"the refused control ran cortex: {env.marked()!r}")
    p = env.run(env.once_copy, "--arm", "B", "--generator", "none", "--only", ids, "--out", out,
                "--ledger", env.ledger, "--ranker", "spectrum")
    expect(p.returncode == 0, f"--ranker spectrum on a protected-set control refused: "
                              f"{p.stderr[-300:]}")
    expect(not os.path.exists(out) and ledger_rows(env.ledger) == [],
           "a protected-set control wrote rows or a ledger row")


def case_spend_resume(env):
    import protected
    tasks = json.load(open(env.once_copy))["tasks"]
    id0, id1 = tasks[0]["id"], tasks[1]["id"]
    out = os.path.join(env.root, "spend", "run1", "b.jsonl")
    common = (*PAID_B, "--spend-root", env.spend_root, "--ledger", env.ledger,
              "--cortex-commit", head())
    p = env.run(env.once_copy, *common, "--spend", "selftest", "--only", id0, "--out", out)
    expect(p.returncode == 0, f"a spent paid run failed: {p.stderr[-600:]}")
    led = ledger_rows(env.ledger)
    expect(len(led) == 1 and led[0]["set"] == "hard-once" and led[0]["purpose"] == "selftest",
           f"spend wrote ledger {led}")
    if not led:
        return
    sha = protected.row_sha(led[0])
    got = rows(out)
    expect(len(got) == 1 and got[0]["meta"]["spend"]["row_sha256"] == sha,
           "the rows do not name their ledger row")
    for k in ("ts", "set", "corpus_sha256", "purpose", "repair_digest", "patch_sha256",
              "cortex_sha256", "axon_sha256", "grade_axon_sha256", "generator", "model", "k",
              "out", "git_head", "prev_row_sha256"):
        expect(k in led[0], f"ledger row lacks {k}")
    expect(led[0]["corpus_sha256"] == protected.PROTECTED_SETS["hard-once"],
           "ledger corpus_sha256 is not the corpus's")
    out2 = os.path.join(env.root, "spend", "run2", "b.jsonl")
    p = env.run(env.once_copy, *common, "--spend", "selftest", "--only", id0, "--out", out2)
    expect(p.returncode != 0 and "already spent" in p.stderr, "a duplicate spend was not refused")
    expect(len(ledger_rows(env.ledger)) == 1 and not os.path.exists(out2),
           "the refused duplicate wrote something")
    p = env.run(env.once_copy, *common, "--resume", sha, "--only", id1, "--out", out)
    expect(p.returncode == 0, f"resume refused: {p.stderr[-600:]}")
    expect(len(rows(out)) == 2 and len(ledger_rows(env.ledger)) == 1,
           "resume did not continue the same row's out without a new ledger row")
    p = env.run(env.once_copy, *common, "--resume", sha, "--only", id1, "--out", out2)
    expect(p.returncode != 0 and "differs in out" in p.stderr, "resume into another out file passed")
    other = os.path.join(env.bin, "axon2")
    shutil.copyfile(env.axon, other)
    with open(other, "a") as fh:
        fh.write("# another build\n")
    os.chmod(other, 0o755)
    p = env.run(env.once_copy, *common, "--resume", sha, "--only", tasks[2]["id"], "--out", out,
                "--grade-axon", other)
    expect(p.returncode != 0 and "grade_axon_sha256" in p.stderr,
           "resume with another grading axon passed")
    p = env.run(env.once_copy, *common, "--rerun-of", sha, "infra fault", "--only", id0,
                "--out", out2)
    expect(p.returncode == 0, f"rerun-of refused: {p.stderr[-600:]}")
    led = ledger_rows(env.ledger)
    expect(len(led) == 2 and led[1].get("rerun_of") == sha and led[1].get("reason") == "infra fault",
           f"rerun-of did not write a new row naming the original: {led[-1:]}")
    protected.read_ledger(env.ledger)


def case_trials(env):
    import analyse
    out = os.path.join(env.root, "trials", "a.jsonl")
    args = (env.corpus, "--arm", "A", "--agent", "oracle", "--trials", "2", "--out", out)
    p = env.run(*args)
    expect(p.returncode == 0, f"--trials 2 failed: {p.stderr[-400:]}")
    order = [(r["id"], r["trial"]) for r in rows(out)]
    expect(order == [("s1", 1), ("s2", 1), ("s1", 2), ("s2", 2)],
           f"rows are not trial-major (id, trial): {order}")
    p = env.run(*args)
    expect(len(rows(out)) == 4, "a re-run did not skip the done (id, trial) rows")
    keyed = analyse.load(out, trials=True)
    expect(sorted(keyed) == [("s1", 1), ("s1", 2), ("s2", 1), ("s2", 2)],
           f"analyse.load(trials=True) keys {sorted(keyed)}")
    expect(raises(lambda: analyse.load(out), "trial"),
           "analyse.load by id silently kept one of several trials")
    with open(out, "a") as fh:
        fh.write(json.dumps(rows(out)[0]) + "\n")
    expect(raises(lambda: analyse.load(out, trials=True), "more than once"),
           "a duplicate (id, trial) row was not refused")
    expect(set(analyse.TRIAL_SETS) == {"hx-dev", "hx-test"}, "TRIAL_SETS")


def _caps_task(src_task, strict):
    t = dict(src_task)
    if strict:
        t["caps_strict"] = True
    else:
        t.pop("caps_strict", None)
    return t


def _caps_case(axon, label):
    import corpus as corpus_mod
    import run_ab
    env_corpus = os.path.join(tempfile.mkdtemp(prefix="capsab_"), "c.json")
    write_corpus(env_corpus)
    task = corpus_mod.load(env_corpus)["tasks"][0]
    given, pristine = task["broken"], task["pristine"]
    reads = pristine.replace("fn add1(x: i64) -> i64 {\n",
                             "fn add1(x: i64) -> i64 {\n    let _s = read_file(\"x.txt\")\n")
    g = run_ab.grade(_caps_task(task, True), given, reads, axon=axon)
    expect(g.get("caps_widened") == ["fs:read"] and g["verified_clean"] is False,
           f"{label}: a final adding read_file to the symbol graded caps_widened="
           f"{g.get('caps_widened')} verified_clean={g['verified_clean']}")
    # FU-1d: a widened capability counts as a false success (claimed or not).
    fs = getattr(run_ab, "false_success", None)
    expect(fs is not None and fs(True, g) is True and fs(False, g) is True,
           f"{label}: caps_widened={g.get('caps_widened')} does not count as a false success")
    g = run_ab.grade(_caps_task(task, True), given, pristine, axon=axon)
    expect(g.get("caps_widened") == [] and g["verified_clean"] is True,
           f"{label}: the oracle final graded caps_widened={g.get('caps_widened')} "
           f"verified_clean={g['verified_clean']}")
    expect(fs is not None and fs(True, g) is False,
           f"{label}: a clean claimed fix counts as a false success")
    if label == "fake axon":
        g = run_ab.grade(_caps_task(task, False), given, reads, axon=axon)
        expect("caps_widened" not in g and g["verified_clean"] is True,
               "a task without caps_strict changed its scoring")
        expect(fs is not None and fs(True, g) is False,
               "a task without caps_strict counts a capability gain as a false success")
    shutil.rmtree(os.path.dirname(env_corpus), ignore_errors=True)


def case_caps_strict(env):
    _caps_case(env.axon, "fake axon")


def case_caps_strict_real_axon(env):
    if not REAL_AXON:
        print("  skip caps_strict on a real axon: RUN_AB_SELFTEST_AXON is not set")
        return
    _caps_case(REAL_AXON, f"real axon {REAL_AXON}")


def case_integrity_abort(env):
    """FU-8: a grading axon whose verdict carries no valid completion token
    aborts the run as an integrity failure, and no scored row is written."""
    for mode in ("untokened", "badtoken"):
        out = os.path.join(env.root, f"fu8-{mode}", "a.jsonl")
        p = env.run(env.corpus, "--arm", "A", "--agent", "oracle", "--out", out,
                    FAKE_AXON_MODE=mode)
        expect(p.returncode != 0 and "INTEGRITY FAILURE" in p.stderr,
               f"FU-8 {mode}: grading did not abort: rc {p.returncode} {p.stderr[-400:]}")
        expect(rows(out) == [], f"FU-8 {mode}: a scored row was written")
    out = os.path.join(env.root, "fu8-honest", "a.jsonl")
    p = env.run(env.corpus, "--arm", "A", "--agent", "oracle", "--out", out)
    expect(p.returncode == 0 and len(rows(out)) == 2 and all(r["verified_clean"] for r in rows(out)),
           f"FU-8 control: the honest fake axon does not grade the oracle verified: {p.stderr[-400:]}")
    # R5: cortex's accepted artifact is not the file on disk.
    for mode in ("mismatch", "null"):
        out = os.path.join(env.root, f"r5-{mode}", "b.jsonl")
        p = env.run(env.corpus, "--arm", "B", "--generator", "oracle", "--out", out,
                    FAKE_CORTEX_MODE=mode)
        expect(p.returncode != 0 and "INTEGRITY FAILURE" in p.stderr,
               f"R5 {mode}: run did not abort: rc {p.returncode} {p.stderr[-400:]}")
        expect(rows(out) == [], f"R5 {mode}: a scored row was written")


def case_grade_axon_meta(env):
    out = os.path.join(env.root, "meta", "b.jsonl")
    other = os.path.join(env.bin, "axon-grade")
    shutil.copyfile(env.axon, other)
    with open(other, "a") as fh:
        fh.write("# grading build\n")
    os.chmod(other, 0o755)
    p = env.run(env.corpus, "--arm", "B", "--generator", "none", "--out", out,
                "--grade-axon", other, "--cortex-commit", head())
    expect(p.returncode == 0, f"--grade-axon run failed: {p.stderr[-400:]}")
    m = rows(out)[0]["meta"] if rows(out) else {}
    expect(m.get("grade_axon_sha256_16") == hashlib.sha256(open(other, "rb").read()).hexdigest()[:16]
           and m.get("axon_sha256_16") != m.get("grade_axon_sha256_16"),
           f"meta does not record the grading axon apart from arm B's: {m}")
    expect(m.get("cortex_commit") == head() and m.get("commit") == head(),
           f"meta cortex_commit/commit: {m.get('cortex_commit')} {m.get('commit')}")


CASES = [case_pins, case_chain, case_duplicate_spend, case_unspent_paid_refused,
         case_oracle_writes_nothing, case_control_makes_no_model_call, case_spend_resume,
         case_trials, case_caps_strict, case_caps_strict_real_axon, case_integrity_abort,
         case_grade_axon_meta]

FAILS = []


def expect(cond, what):
    if not cond:
        FAILS.append(what)


def raises(fn, needle):
    try:
        fn()
    except (Exception, SystemExit) as e:  # noqa: BLE001
        return needle in str(e)
    return False


def main():
    bad = 0
    for case in CASES:
        before = len(FAILS)
        root = tempfile.mkdtemp(prefix="selftest_ab_")
        try:
            case(Env(root))
        except Exception:  # noqa: BLE001
            FAILS.append(f"{case.__name__} raised:\n{traceback.format_exc()}")
        finally:
            shutil.rmtree(root, ignore_errors=True)
        new = FAILS[before:]
        print(f"{'FAIL' if new else 'ok  '} {case.__name__}")
        for f in new:
            print(f"     - {f}")
        bad += bool(new)
    print(f"selftest_ab: {len(CASES) - bad} passed, {bad} failed")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
