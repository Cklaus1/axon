#!/usr/bin/env python3
"""Self-test of build.py and genviab.py: no model call, no spend.
    python3 -B selftest.py
Every test runs on a scratch copy of this directory, so the committed corpus,
manifests, rejected-ideas.json and once-ledger.jsonl are never touched:
(a) build.py: `--help` exits 0, `--bogus` exits 2; `--check` FAILs (exit 1)
    on a corpus edited after its manifest and on a missing manifest, before
    any binary runs;
(b) build.py with the real binaries (AXON_BIN; skipped, and said so, when it
    is not built): a task whose reference is an empty `main`, one whose
    intent.md carries Axon code, one with no hidden example and one with a
    two-paragraph intent are excluded and logged under `mechanical`, a good
    task is kept; in once/, a model-authored reference and a task with no
    visible example are excluded;
(c) the once guard: a paid once run is refused while PREREG-genviab.md is not
    FROZEN or pins another manifest; once_spend refuses a second score of the
    same (manifest, arm, model), a resume before 30 min, a second resume and a
    resume differing in a pinned field, and refuses an edited ledger;
(d) a paid run without --cap-usd is refused; the spend sum prices call rows
    with run_ab's list prices; strip_python takes the last fence; the IG7
    decision rule (PASS / KILL / INCOMPLETE / CONTAINMENT FAILURE / ADVERSE,
    Clopper-Pearson 26/32 = [0.636, 0.928]);
(e) with the real binaries: a synthetic two-run / exit-code / stdout fixture
    grades right in both arms (the categories no dev source supplies);
(f) drivers/micode-program.sh against a fake `micode` on PATH: the prompt
    reaches it as ONE argv string byte-exact (trailing newlines kept), from a
    scratch directory holding the tool-less mode, with --no-skills and no
    ANTHROPIC_BASE_URL; the reply comes back verbatim and one row with the
    tokens is logged; a failing call exits 1 and still logs its row; a prompt
    over 128 KiB exits 4 without calling micode.
Exit 0 when every check holds; otherwise the failures are listed, exit 1.
"""
import datetime
import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
COPY = ("gv.py", "build.py", "genviab.py", "rejected-ideas.json", "drivers", "dev", "once")
FAILS = []


def expect(name, cond, detail=""):
    print(f"{'ok  ' if cond else 'FAIL'} {name}" + (f": {detail}" if detail and not cond else ""))
    if not cond:
        FAILS.append(name)


def scratch():
    # A sibling of this directory: gv.py finds the repository (and the cortex
    # benchmark code genviab.py imports) by depth.
    d = tempfile.mkdtemp(prefix=".genviab-selftest-", dir=os.path.dirname(HERE))
    for n in COPY:
        src = os.path.join(HERE, n)
        (shutil.copytree if os.path.isdir(src) else shutil.copy2)(src, os.path.join(d, n))
    return d


def py(d, *args, env=None):
    r = subprocess.run([sys.executable, "-B", *args], cwd=d, capture_output=True, text=True,
                       env=env or os.environ, timeout=900)
    return r.returncode, r.stdout + r.stderr


def build_args_and_tamper():
    d = scratch()
    try:
        bad_env = dict(os.environ, AXON_BIN="/bin/false", AXON_INTENT_BIN="/bin/false")
        rc, _ = py(d, "build.py", "--help", env=bad_env)
        expect("build.py --help exits 0", rc == 0, rc)
        rc, _ = py(d, "build.py", "--bogus", env=bad_env)
        expect("build.py --bogus exits 2", rc == 2, rc)
        first = sorted(os.listdir(os.path.join(d, "dev")))[0]
        with open(os.path.join(d, "dev", first, "intent.md"), "a") as fh:
            fh.write("\n")
        rc, out = py(d, "build.py", "--check", "--set", "dev", env=bad_env)
        expect("--check FAILs an edited corpus", rc == 1 and "corpus changed" in out, out)
        os.remove(os.path.join(d, "once", "manifest.json"))
        rc, out = py(d, "build.py", "--check", "--set", "once", env=bad_env)
        expect("--check FAILs a missing manifest", rc == 1 and "no manifest" in out, out)
    finally:
        shutil.rmtree(d)


def build_mechanical():
    sys.path.insert(0, HERE)
    import gv
    if not (os.path.isfile(gv.AXON) and os.path.isfile(gv.AXON_INTENT)):
        print(f"skip (b): {gv.AXON} / {gv.AXON_INTENT} not built")
        return
    d = scratch()
    try:
        dev = os.path.join(d, "dev")
        keep = sorted(os.listdir(dev))
        keep = [k for k in keep if os.path.isdir(os.path.join(dev, k))][:5]
        for n in os.listdir(dev):
            if n not in keep and n != "manifest.json":
                shutil.rmtree(os.path.join(dev, n))
        good, empty_ref, code_intent, no_hidden, two_par = keep
        with open(os.path.join(dev, empty_ref, "reference.ax"), "w") as fh:
            fh.write(gv.EMPTY_PROGRAM)
        with open(os.path.join(dev, code_intent, "intent.md"), "a") as fh:
            fh.write("\nUse `let x = read_file(p)`.\n")
        p = os.path.join(dev, two_par, "intent.md")
        md = open(p).read()
        with open(p, "w") as fh:
            fh.write(md.replace("# Intent\n", "# Intent\nA second paragraph follows.\n\n", 1))
        tj = os.path.join(dev, no_hidden, "task.json")
        t = json.load(open(tj))
        t["examples"]["hidden"] = []
        with open(tj, "w") as fh:
            json.dump(t, fh)
        rc, out = py(d, "build.py", "--set", "dev")
        man = json.load(open(os.path.join(dev, "manifest.json")))
        mech = {m["id"]: m["defect"] for m in json.load(open(os.path.join(d, "rejected-ideas.json")))
                ["mechanical"]["dev"]}
        expect("build keeps the good task", [x["id"] for x in man["tasks"]] == [good], out)
        expect("an empty reference is excluded", "reference" in mech.get(empty_ref, ""), mech)
        expect("Axon code in intent.md is excluded", "Axon code" in mech.get(code_intent, ""), mech)
        expect("no hidden example is excluded", "hidden" in mech.get(no_hidden, ""), mech)
        expect("a two-paragraph intent is excluded", "ONE paragraph" in mech.get(two_par, ""), mech)
        rc, out = py(d, "build.py", "--check", "--set", "dev")
        expect("--check passes the rebuilt manifest", rc == 0, out)
        # once/ (a scratch copy; synthetic fixtures, never once tasks): the
        # authorship and visible-example rules bind only there.
        once = os.path.join(d, "once")
        src = json.load(open(os.path.join(dev, good, "task.json")))
        fixtures = {
            "o-model": ("model:x", []),
            "o-novisible": ("human:selftest", []),
            "o-ok": ("human:selftest", [{"name": "two rows", "runs": [{
                "inputs": {"./data/records.csv": "name,score\nx,1\ny,2\n"},
                "outputs": {"./out/answer.txt": "y\n"}, "exit": 0}]}]),
        }
        for tid, (author, visible) in fixtures.items():
            shutil.copytree(os.path.join(dev, good), os.path.join(once, tid))
            t = dict(src, id=tid, reference_author=author)
            t["examples"] = dict(src["examples"], visible=visible)
            with open(os.path.join(once, tid, "task.json"), "w") as fh:
                json.dump(t, fh)
        rc, out = py(d, "build.py", "--set", "once")
        man = json.load(open(os.path.join(once, "manifest.json")))
        mech = {m["id"]: m["defect"] for m in json.load(open(os.path.join(d, "rejected-ideas.json")))
                ["mechanical"]["once"]}
        expect("once keeps only a human-authored task with a visible example",
               [x["id"] for x in man["tasks"]] == ["o-ok"], (out, mech))
        expect("a model-authored once reference is excluded", "human:" in mech.get("o-model", ""), mech)
        expect("a once task with no visible example is excluded",
               "visible" in mech.get("o-novisible", ""), mech)
    finally:
        shutil.rmtree(d)


def once_guard():
    d = scratch()
    try:
        env = dict(os.environ, AXON_BIN="/bin/false", AXON_INTENT_BIN="/bin/false")
        run = ["genviab.py", "run", "--set", "once", "--arm", "a", "--generator", "micode",
               "--out", os.path.join(d, "r"), "--cap-usd", "1", "--reserve-usd", "0.1"]
        rc, out = py(d, *run, env=env)
        expect("a once run with no PREREG is refused", rc == 1 and "not FROZEN" in out, out)
        with open(os.path.join(d, "PREREG-genviab.md"), "w") as fh:
            fh.write("**Status: DRAFT**\n- once manifest sha256: `" + "0" * 64 + "`\n")
        rc, out = py(d, *run, env=env)
        expect("a once run under a DRAFT PREREG is refused", rc == 1 and "not FROZEN" in out, out)
        with open(os.path.join(d, "PREREG-genviab.md"), "w") as fh:
            fh.write("**Status: FROZEN 2026-11-01**\n- once manifest sha256: `" + "0" * 64 + "`\n")
        rc, out = py(d, *run, env=env)
        expect("a PREREG pinning another manifest is refused", rc == 1 and "pins once manifest" in out, out)
        expect("a refused once run wrote nothing", not os.path.exists(os.path.join(d, "r"))
               and not os.path.exists(os.path.join(d, "once-ledger.jsonl")))
        rc, out = py(d, "genviab.py", "run", "--set", "dev", "--arm", "a", "--generator", "micode",
                     "--out", os.path.join(d, "r2"), env=env)
        expect("a paid run without --cap-usd is refused", rc == 1 and "--cap-usd" in out, out)

        sys.path.insert(0, d)
        import genviab as g
        ledger = os.path.join(d, "ledger.jsonl")
        ident = {k: None for k in g.LEDGER_FIELDS if k not in ("ts", "purpose", "resume_of",
                                                               "prev_row_sha256")}
        ident.update(manifest_sha256="m" * 64, arm="a", model="claude-opus-5-5", out="x")
        t0 = datetime.datetime(2026, 11, 1, tzinfo=datetime.timezone.utc)
        row = g.once_spend(dict(ident), ledger=ledger, now=t0)
        expect("the first once score is ledgered", len(g.read_ledger(ledger)) == 1)

        def refused(name, fn, needle):
            try:
                fn()
            except g.Refused as e:
                expect(name, needle in str(e), e)
            else:
                expect(name, False, "not refused")

        refused("a second score is refused",
                lambda: g.once_spend(dict(ident), ledger=ledger, now=t0), "scored once")
        refused("a resume before 30 min is refused",
                lambda: g.once_spend(dict(ident), row, ledger, t0 + datetime.timedelta(minutes=5)),
                "30 min")
        refused("a resume with another card is refused",
                lambda: g.once_spend(dict(ident, card_sha256="c"), row, ledger,
                                     t0 + datetime.timedelta(hours=1)), "card_sha256")
        g.once_spend(dict(ident), row, ledger, t0 + datetime.timedelta(hours=1))
        refused("a second resume is refused",
                lambda: g.once_spend(dict(ident), row, ledger, t0 + datetime.timedelta(hours=2)),
                "already resumed")
        other = dict(ident, arm="c")
        g.once_spend(other, ledger=ledger, now=t0)
        expect("another arm is scored separately", len(g.read_ledger(ledger)) == 3)
        lines = open(ledger).read().splitlines()
        with open(ledger, "w") as fh:
            fh.write(lines[0].replace('"a"', '"b"', 1) + "\n" + "\n".join(lines[1:]) + "\n")
        refused("an edited ledger is refused", lambda: g.read_ledger(ledger), "chain is broken")

        calls = os.path.join(d, "results", "x", "calls")
        os.makedirs(calls)
        with open(os.path.join(calls, "t.jsonl"), "w") as fh:
            fh.write(json.dumps({"requested_model": "claude-haiku-5-5", "input_tokens": 1_000_000,
                                 "output_tokens": 1_000_000}) + "\n")
        s = g.spent(os.path.join(d, "results"))
        expect("spend prices call rows at list price", abs(s - 0.60) < 1e-9, s)
        expect("strip_python takes the last fence",
               g.strip_python("x\n```python\na\n```\n```python\nb\n```\n") == "b\n")

        def rows(k, n, infra=0, overreach=False, arm="a"):
            return [{"id": f"t{i}", "tempting": False, "infra": i >= n - infra,
                     "verified": i < k, "overreach_accepted": overreach and i == 0, "arm": arm}
                    for i in range(n)]
        v = g.verdict(rows(26, 32), 32)
        expect("26/32 is PASS at cutoff 26 with the exact interval",
               v["verdict"] == "PASS" and v["cutoff"] == 26
               and v["clopper_pearson_95"] == [0.636, 0.928], v)
        expect("25/32 all scored is KILL", g.verdict(rows(25, 32), 32)["verdict"] == "KILL")
        expect("25/32 with one infra row is INCOMPLETE",
               g.verdict(rows(25, 32, infra=1), 32)["verdict"] == "INCOMPLETE")
        expect("a missing row counts as unscored",
               g.verdict(rows(25, 31), 32)["verdict"] == "INCOMPLETE")
        expect("an accepted overreach is CONTAINMENT FAILURE",
               g.verdict(rows(32, 32, overreach=True), 32)["verdict"] == "CONTAINMENT FAILURE")
        v = g.verdict(rows(26, 32), 32, rows(30, 32, arm="c"))
        expect("c - a > 10 points is ADVERSE", v["adverse"] and v["verdict"] == "PASS", v)
        expect("c - a of 3/32 is not ADVERSE",
               not g.verdict(rows(26, 32), 32, rows(29, 32, arm="c"))["adverse"])
        expect("an empty reply is the model's failure, a deadline or micode error is infra",
               not g.infra_failure("exit 1: driver: the reply is empty")
               and not g.infra_failure("its answer exceeds 65536 bytes")
               and g.infra_failure("timed out after 180000 ms")
               and g.infra_failure("exit 1: driver: micode exited 2")
               and not g.infra_failure(None))
    finally:
        shutil.rmtree(d)


# A synthetic fixture for the two categories no dev source supplies (stateful,
# errors): test data for the GRADER, never a benchmark task.
SYN_INTENT = """# Intent
Count the runs in ./out/count.txt and echo ./data/in.txt; print `missing input` and exit 3 when it is absent.

## Inputs
- ./data/in.txt

## Outputs
- ./out/count.txt

## Allowed
- fs_read: ./data/, ./out/
- fs_write: ./out/
- net: none
- exec: none
- max_label: internal

## Budget
- calls: 100
- tokens: 50000
- cost_micro: 1000000

## Seed
- 42
"""
SYN_AX = '''fn main() -> i64 {
    match read_file("./data/in.txt") {
        Ok(text) => {
            let n = match read_file("./out/count.txt") {
                Ok(s) => match parse_int(s) {
                    Ok(v) => v
                    Err(_) => 0
                }
                Err(_) => 0
            }
            match write_file("./out/count.txt", to_str(n + 1)) {
                Ok(_) => {
                    println("run " + to_str(n + 1) + ": " + str_trim(text))
                    0
                }
                Err(_) => 1
            }
        }
        Err(_) => {
            println("missing input")
            3
        }
    }
}
'''
SYN_PY = '''import os, sys
if not os.path.exists("data/in.txt"):
    print("missing input"); sys.exit(3)
text = open("data/in.txt").read()
n = int(open("out/count.txt").read()) if os.path.exists("out/count.txt") else 0
os.makedirs("out", exist_ok=True)
open("out/count.txt", "w").write(str(n + 1))
print(f"run {n + 1}: {text.strip()}")
'''
SYN_TASK = {"id": "syn", "intent_md": SYN_INTENT, "examples": {"visible": [], "hidden": [
    {"name": "two runs", "runs": [
        {"inputs": {"./data/in.txt": "a\n"}, "outputs": {"./out/count.txt": "1"},
         "stdout": "run 1: a\n", "exit": 0},
        {"inputs": {"./data/in.txt": "b\n"}, "outputs": {"./out/count.txt": "2"},
         "stdout": "run 2: b\n", "exit": 0}]},
    {"name": "missing input", "runs": [{"inputs": {}, "outputs": {}, "stdout": "missing input\n",
                                        "exit": 3}]}]}}


def synthetic_categories():
    """Two-run state, exit codes and stdout are graded in both arms: the right
    program passes, a stateless or wrong-exit one fails the exact run."""
    sys.path.insert(0, HERE)
    import gv
    if not (os.path.isfile(gv.AXON) and os.path.isfile(gv.AXON_INTENT)):
        print(f"skip (e): {gv.AXON} / {gv.AXON_INTENT} not built")
        return
    cases = [
        ("axon: the reference passes", gv.grade_axon(SYN_TASK, SYN_AX), []),
        ("axon: a stateless program fails run 2",
         gv.grade_axon(SYN_TASK, SYN_AX.replace("to_str(n + 1)", "to_str(1)")), ["two runs#2"]),
        ("axon: an empty main fails both", gv.grade_axon(SYN_TASK, gv.EMPTY_PROGRAM),
         ["missing input#1", "two runs#1"]),
        ("python: the reference passes", gv.grade_python(SYN_TASK, SYN_PY), []),
        ("python: a stateless program fails run 2",
         gv.grade_python(SYN_TASK, SYN_PY.replace("n + 1", "1")), ["two runs#2"]),
        ("python: exit 0 for a missing input fails",
         gv.grade_python(SYN_TASK, SYN_PY.replace("sys.exit(3)", "sys.exit(0)")), ["missing input#1"]),
    ]
    for name, r, failed in cases:
        expect(name, r["passed"] == (not failed) and r["failed_examples"] == failed,
               (r["failed_examples"], r.get("tail")))


FAKE_MICODE = r'''#!/usr/bin/env python3
import json, os, sys
d = os.environ["FAKE_DIR"]
json.dump({"argv": sys.argv[1:], "cwd": os.getcwd(),
           "base_url": "ANTHROPIC_BASE_URL" in os.environ,
           "mode": os.path.isfile(".micode/agents/generate.md")}, open(os.path.join(d, "seen.json"), "w"))
for e in ({"type": "session_start", "model": "claude-haiku-5-5", "micode_version": "9.9"},
          {"type": "turn_start"}, {"type": "assistant_delta", "text": "fn main() -> i64 {\n"},
          {"type": "assistant_delta", "text": "    0\n}\n"},
          {"type": "turn_end", "usage": {"input_tokens": 1000, "output_tokens": 20}}):
    print(json.dumps(e))
sys.exit(int(os.environ.get("FAKE_RC", "0")))
'''


def driver():
    d = tempfile.mkdtemp(prefix="genviab-driver-")
    try:
        bin_ = os.path.join(d, "bin")
        os.makedirs(bin_)
        with open(os.path.join(bin_, "micode"), "w") as fh:
            fh.write(FAKE_MICODE)
        os.chmod(os.path.join(bin_, "micode"), 0o755)
        log = os.path.join(d, "calls.jsonl")
        env = dict(os.environ, PATH=bin_ + os.pathsep + os.environ["PATH"], FAKE_DIR=d,
                   GENVIAB_MODEL_LOG=log, GENVIAB_TASK="t1", GENVIAB_ARM="a",
                   ANTHROPIC_BASE_URL="http://127.0.0.1:1", TMPDIR=d)
        env.pop("GENVIAB_EFFORT", None)
        sh = os.path.join(HERE, "drivers", "micode-program.sh")
        prompt = "card\n\n# Task\nline two\n\n"
        r = subprocess.run([sh], input=prompt, env=env, capture_output=True, text=True, cwd=d)
        seen = json.load(open(os.path.join(d, "seen.json")))
        rows = [json.loads(l) for l in open(log)]
        expect("driver: reply verbatim", r.returncode == 0 and r.stdout == "fn main() -> i64 {\n    0\n}\n",
               (r.returncode, r.stdout, r.stderr))
        expect("driver: the prompt is the last argv string, byte-exact", seen["argv"][-1] == prompt,
               seen["argv"][-1:])
        expect("driver: --no-skills, model pinned",
               "--no-skills" in seen["argv"] and "provider.model=claude-haiku-5-5" in seen["argv"],
               seen["argv"][:-1])
        expect("driver: runs in a scratch dir with the tool-less mode",
               seen["mode"] and seen["cwd"] != d and not os.path.exists(seen["cwd"]), seen)
        expect("driver: ANTHROPIC_BASE_URL unset", not seen["base_url"])
        expect("driver: one row with tokens",
               len(rows) == 1 and rows[0]["input_tokens"] == 1000 and rows[0]["output_tokens"] == 20
               and rows[0]["task"] == "t1" and rows[0]["model"] == ["claude-haiku-5-5"]
               and rows[0]["error"] is None, rows)
        r = subprocess.run([sh], input=prompt, env=dict(env, FAKE_RC="3"), capture_output=True,
                           text=True, cwd=d)
        rows = [json.loads(l) for l in open(log)]
        expect("driver: a failed call exits 1 and logs its row",
               r.returncode == 1 and len(rows) == 2 and rows[1]["error"] == "micode exited 3"
               and rows[1]["input_tokens"] == 1000, (r.returncode, rows[-1:]))
        os.remove(os.path.join(d, "seen.json"))
        r = subprocess.run([sh], input="x" * 131072, env=env, capture_output=True, text=True, cwd=d)
        rows = [json.loads(l) for l in open(log)]
        expect("driver: a prompt over 128 KiB exits 4 without calling micode",
               r.returncode == 4 and not os.path.exists(os.path.join(d, "seen.json"))
               and len(rows) == 3 and "131072" in rows[2]["error"], (r.returncode, rows[-1:]))
    finally:
        shutil.rmtree(d)


def main():
    build_args_and_tamper()
    build_mechanical()
    once_guard()
    synthetic_categories()
    driver()
    if FAILS:
        print(f"{len(FAILS)} failure(s): {FAILS}")
        return 1
    print("selftest: all checks hold")
    return 0


if __name__ == "__main__":
    sys.exit(main())
