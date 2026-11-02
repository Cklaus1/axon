#!/usr/bin/env python3
"""Self-test of build.py and genviab.py: no model call, no spend.
    python3 -B selftest.py
    GENVIAB_REQUIRE_BINS=1 python3 -B selftest.py    (the gate: a missing binary FAILs)
Every test runs on a scratch copy of this directory, so the committed corpus,
manifests, rejected-ideas.json and once-ledger.jsonl are never touched:
(a) build.py: `--help` exits 0, `--bogus` exits 2; `--check` FAILs (exit 1)
    on a corpus edited after its manifest and on a missing manifest, before
    any binary runs;
(b) build.py with the real binaries (AXON_BIN; skipped, and said so, when it
    is not built, and a FAIL under GENVIAB_REQUIRE_BINS=1): a task whose
    reference is an empty `main`, one whose intent.md carries Axon code, one
    with no hidden example and one with a two-paragraph intent are excluded
    and logged under `mechanical`, a good task is kept; in once/, a
    model-authored reference and a task with no visible example are excluded;
    reference.py (spec GV-3): a passing one is kept with its file in the
    manifest; one that raises (also KeyboardInterrupt) is a fault whatever its
    exit; it sees a clean environment, a fixed hash seed, no fs_read directory
    with no input staged, and a 1 MiB stdout bound; a stateful task is kept
    with either reference; both references, reference.py in dev/, an absolute
    path, `import socket`, an exit of 300 and an exec grant are excluded; the
    oracle generator refuses a Python-reference task; arm c sees the clean
    environment and per-run fs_read binds;
(c) the once guard: a paid once run is refused while PREREG-genviab.md is not
    FROZEN or pins another manifest, or with the selftest's ledger URL
    override set; once_spend refuses a second score of the same (manifest,
    arm, model), a resume before 30 min, a second resume and a resume
    differing in a pinned field, and refuses an edited ledger; against a bare
    origin standing in for the pinned URL (spec GV-2): a fresh clone sees a
    pushed row, and a reset, an unpushed row, an unreachable origin, another
    origin URL, a rejected push and a checkout ahead of origin/main refuse;
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
REQUIRE_BINS = os.environ.get("GENVIAB_REQUIRE_BINS") == "1"


def expect(name, cond, detail=""):
    print(f"{'ok  ' if cond else 'FAIL'} {name}" + (f": {detail}" if detail and not cond else ""))
    if not cond:
        FAILS.append(name)


def have_binaries(tag, gv):
    """True when both binaries exist. Otherwise a printed skip, or a FAIL
    under GENVIAB_REQUIRE_BINS=1 (the gate stage: a skip is a non-result)."""
    if os.path.isfile(gv.AXON) and os.path.isfile(gv.AXON_INTENT):
        return True
    missing = [p for p in (gv.AXON, gv.AXON_INTENT) if not os.path.isfile(p)]
    if REQUIRE_BINS:
        expect(f"({tag}) the binaries exist (GENVIAB_REQUIRE_BINS=1)", False, f"missing {missing}")
    else:
        print(f"skip ({tag}): {missing} not built")
    return False


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


def gv3_intent(goal, read="./data/", exec_="none"):
    """A fixture intent.md: one paragraph, ./data/ in, ./out/ out."""
    return ("# Intent\n" + goal + "\n\n## Inputs\n- ./data/in.txt\n\n## Outputs\n- ./out/up.txt\n\n"
            "## Allowed\n- fs_read: " + read + "\n- fs_write: ./out/\n- net: none\n- exec: " + exec_
            + "\n- max_label: internal\n\n## Budget\n- calls: 100\n- tokens: 50000\n"
            "- cost_micro: 1000000\n\n## Seed\n- 42\n")


def gv3_ex(name, inp, stdout=None, code=0, outputs=None):
    run = {"inputs": {} if inp is None else {"./data/in.txt": inp}, "outputs": outputs or {},
           "exit": code}
    if stdout is not None:
        run["stdout"] = stdout
    return {"name": name, "runs": [run]}


def gv3_task(set_dir, tid, goal, refs, vis, hid, exec_="none", author="human:selftest"):
    """Write a fixture task: `refs` maps reference file name -> source."""
    p = os.path.join(set_dir, tid)
    os.makedirs(p)
    with open(os.path.join(p, "intent.md"), "w") as fh:
        fh.write(gv3_intent(goal, exec_=exec_))
    with open(os.path.join(p, "task.json"), "w") as fh:
        json.dump({"id": tid, "category": "transform", "source": {"kind": "selftest fixture"},
                   "reference_author": author, "examples": {"visible": vis, "hidden": hid}}, fh)
    for n, src in refs.items():
        with open(os.path.join(p, n), "w") as fh:
            fh.write(src)


PY_UPPER = ('text = open("./data/in.txt").read()\nwith open("./out/up.txt", "w") as fh:\n'
            '    fh.write(text.upper())\nprint("done")\n')
PY_TALLY = '''import os
tally = {}
if os.path.exists("./out/tally.txt"):
    for line in open("./out/tally.txt").read().split("\\n"):
        if line.find("=") > 0:
            k, v = line.split("=", 1)
            tally[k] = tally.get(k, 0) + int(v)
for line in open("./data/words.txt").read().split("\\n"):
    for w in line.split(" "):
        if w:
            tally[w.lower()] = tally.get(w.lower(), 0) + 1
with open("./out/tally.txt", "w") as fh:
    fh.write("".join(f"{k}={tally[k]}\\n" for k in sorted(tally)))
print(f"total: {sum(tally.values())}")
'''


def build_mechanical():
    sys.path.insert(0, HERE)
    import gv
    if not have_binaries("b", gv):
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
        # GV-3: reference.py is once-only.
        shutil.copytree(os.path.join(dev, good), os.path.join(dev, "d-py"))
        os.remove(os.path.join(dev, "d-py", "reference.ax"))
        with open(os.path.join(dev, "d-py", "reference.py"), "w") as fh:
            fh.write('print("x")\n')
        t = json.load(open(os.path.join(dev, good, "task.json")))
        with open(os.path.join(dev, "d-py", "task.json"), "w") as fh:
            json.dump(dict(t, id="d-py"), fh)
        rc, out = py(d, "build.py", "--set", "dev")
        man = json.load(open(os.path.join(dev, "manifest.json")))
        mech = {m["id"]: m["defect"] for m in json.load(open(os.path.join(d, "rejected-ideas.json")))
                ["mechanical"]["dev"]}
        expect("build keeps the good task", [x["id"] for x in man["tasks"]] == [good], out)
        expect("reference.py in dev/ is excluded", "once-only" in mech.get("d-py", ""), mech)
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
        visible = [{"name": "two rows", "runs": [{
            "inputs": {"./data/records.csv": "name,score\nx,1\ny,2\n"},
            "outputs": {"./out/answer.txt": "y\n"}, "exit": 0}]}]
        fixtures = {
            "o-model": ("model:x", []),
            "o-novisible": ("human:selftest", []),
            "o-noname": ("human:", visible),
            "o-blankname": ("human: ", visible),
            "o-ok": ("human:selftest", visible),
        }
        for tid, (author, vis) in fixtures.items():
            shutil.copytree(os.path.join(dev, good), os.path.join(once, tid))
            t = dict(src, id=tid, reference_author=author)
            t["examples"] = dict(src["examples"], visible=vis)
            with open(os.path.join(once, tid, "task.json"), "w") as fh:
                json.dump(t, fh)
        # GV-3 (spec: Tests): reference.py fixtures, graded by the reference runner.
        up = {"./out/up.txt": "AB\n"}
        vis_up = [gv3_ex("ab", "ab\n", "done\n", outputs=up)]
        hid_up = [gv3_ex("xy", "xy z\n", "done\n", outputs={"./out/up.txt": "XY Z\n"})]
        seed0 = subprocess.run([gv.PYTHON, "-c", "print(hash('abc'))"], capture_output=True,
                               text=True, env=dict(os.environ, PYTHONHASHSEED="0")).stdout
        big = "x" * 102400
        h01 = os.path.join(HERE, "dev", "h01-word-tally")
        gv3 = {  # id: (refs, visible, hidden, exec)
            "p-ok": ({"reference.py": PY_UPPER}, vis_up, hid_up, "none"),
            "p-raise": ({"reference.py": 'raise ValueError("bad input")\n'},
                        [gv3_ex("v", "a\n", code=1)], [gv3_ex("h", "b\n", code=1)], "none"),
            "p-kbint": ({"reference.py": "raise KeyboardInterrupt\n"},
                        [gv3_ex("v", "a\n", code=1)], [gv3_ex("h", "b\n", code=1)], "none"),
            "p-env": ({"reference.py": 'import os\nprint(os.environ.get("FAKE_SECRET", "absent"))\n'},
                      [gv3_ex("v", "a\n", "absent\n")], [gv3_ex("h", "b\n", "absent\n")], "none"),
            "p-hash": ({"reference.py": 'print(hash("abc"))\n'},
                       [gv3_ex("v", "a\n", seed0)], [gv3_ex("h", "b\n", seed0)], "none"),
            "p-nodir": ({"reference.py": 'import os\nprint(os.path.isdir("data"))\n'},
                        [gv3_ex("no input", None, "False\n")], [gv3_ex("input", "a\n", "True\n")],
                        "none"),
            "p-big": ({"reference.py": f'import sys\nsys.stdout.write("x" * {len(big)})\n'},
                      [gv3_ex("v", "a\n", big)], [gv3_ex("h", "b\n", big)], "none"),
            "p-both": ({"reference.py": PY_UPPER, "reference.ax": gv.EMPTY_PROGRAM},
                       vis_up, hid_up, "none"),
            "p-abs": ({"reference.py": 'print(open("/etc/x").read())\n'}, vis_up, hid_up, "none"),
            "p-socket": ({"reference.py": "import socket\n" + PY_UPPER}, vis_up, hid_up, "none"),
            "p-exit300": ({"reference.py": PY_UPPER}, vis_up,
                          [gv3_ex("xy", "xy z\n", "done\n", 300)], "none"),
            "p-exec": ({"reference.py": PY_UPPER}, vis_up, hid_up, "any"),
        }
        for tid, (refs, vis, hid, ex_) in gv3.items():
            gv3_task(once, tid, f"Fixture {tid}: upper-case ./data/in.txt into ./out/up.txt and print "
                     "a line.", refs, vis, hid, exec_=ex_)
        # The same stateful two-run task with either reference (parity smoke).
        for tid, ref in (("s-ax", None), ("s-py", PY_TALLY)):
            shutil.copytree(h01, os.path.join(once, tid))
            t = json.load(open(os.path.join(once, tid, "task.json")))
            with open(os.path.join(once, tid, "task.json"), "w") as fh:
                json.dump(dict(t, id=tid, reference_author="human:selftest"), fh)
            if ref is not None:
                os.remove(os.path.join(once, tid, "reference.ax"))
                with open(os.path.join(once, tid, "reference.py"), "w") as fh:
                    fh.write(ref)
                p = os.path.join(once, tid, "intent.md")
                md = open(p).read()
                with open(p, "w") as fh:
                    fh.write(md.replace("Keep a running word tally", "Keep a running word count tally", 1))
        rc, out = py(d, "build.py", "--set", "once", env=dict(os.environ, FAKE_SECRET="leak"))
        man = json.load(open(os.path.join(once, "manifest.json")))
        mech = {m["id"]: m["defect"] for m in json.load(open(os.path.join(d, "rejected-ideas.json")))
                ["mechanical"]["once"]}
        kept = {x["id"]: x for x in man["tasks"]}
        expect("once keeps only a human-authored task with a visible example",
               sorted(k for k in kept if k.startswith("o-")) == ["o-ok"], (out, mech))
        expect("a model-authored once reference is excluded", "human:" in mech.get("o-model", ""), mech)
        for tid in ("o-noname", "o-blankname"):
            expect(f"a once reference_author with no name is excluded ({tid})",
                   "human:<name>" in mech.get(tid, ""), mech)
        expect("a once task with no visible example is excluded",
               "visible" in mech.get("o-novisible", ""), mech)
        expect("a once task with a passing reference.py is kept, reference.py in its manifest files",
               "p-ok" in kept and set(kept["p-ok"]["files"]) == {"intent.md", "task.json", "reference.py"},
               (kept.get("p-ok"), mech.get("p-ok")))
        expect("a reference.py that raises on an example expecting exit 1 is excluded",
               "raised" in mech.get("p-raise", ""), mech.get("p-raise"))
        expect("a reference.py raising KeyboardInterrupt is a fault, not an exit",
               "raised" in mech.get("p-kbint", ""), mech.get("p-kbint"))
        for tid, what in (("p-env", "sees a clean environment (FAKE_SECRET absent)"),
                          ("p-hash", "hashes with the fixed seed"),
                          ("p-nodir", "sees no fs_read directory with no input staged"),
                          ("p-big", "may print 100 KiB of expected stdout (the 1 MiB bound)"),
                          ("s-ax", "stateful task is kept with reference.ax"),
                          ("s-py", "stateful task is kept with reference.py")):
            expect(f"reference.py {what} ({tid})", tid in kept, mech.get(tid))
        for tid, needle in (("p-both", "two references"), ("p-abs", "absolute"),
                            ("p-socket", "imports socket"), ("p-exit300", "0..=255"),
                            ("p-exec", "exec: none")):
            expect(f"excluded: {tid} ({needle})", needle in mech.get(tid, ""), mech.get(tid))
        rc, out = py(d, "genviab.py", "run", "--set", "once", "--arm", "a", "--generator", "oracle",
                     "--only", "p-ok", "--out", os.path.join(d, "oracle-py"))
        expect("the oracle generator refuses a reference.py task",
               rc == 1 and "needs reference.ax" in out, out)
        # Arm c's runner (NH-G3): the clean environment and per-run fs_read binds.
        saved = os.environ.get("FAKE_SECRET")
        os.environ["FAKE_SECRET"] = "leak"
        try:
            # The spec's table; bwrap's --chdir adds PWD.
            keys = repr(["LANG", "LC_ALL", "PATH", "PWD", "PYTHONHASHSEED"]) + "\n"
            task = {"intent_md": gv3_intent("arm c fixture"),
                    "examples": {"visible": [gv3_ex("env", None, keys)], "hidden": []}}
            r = gv.grade_python(task, "import os\nprint(sorted(os.environ))\n", ("visible",))
            expect("arm c sees only the clean environment's keys", r["passed"], r["examples"])
            task["examples"]["visible"] = [gv3_ex("no input", None, "False\n")]
            r = gv.grade_python(task, 'import os\nprint(os.path.isdir("data"))\n', ("visible",))
            expect("arm c sees no fs_read directory with no input staged", r["passed"], r["examples"])
        finally:
            if saved is None:
                os.environ.pop("FAKE_SECRET", None)
            else:
                os.environ["FAKE_SECRET"] = saved
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
        for k, v in (("GENVIAB_LEDGER_URL", "/tmp/elsewhere.git"), ("GENVIAB_SELFTEST", "1")):
            rc, out = py(d, *run, env=dict(env, **{k: v}))
            expect(f"a once run with {k} set is refused", rc == 1 and "override" in out, out)

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

        def attempt(name, fn):
            try:
                expect(name, fn())
            except Exception as e:  # noqa: BLE001 - a crash is this check's FAIL
                expect(name, False, f"{type(e).__name__}: {e}")

        attempt("the ledger of record is main of the pinned GitHub URL",
                lambda: (g.LEDGER_REMOTE, g.LEDGER_URL, g.LEDGER_BRANCH)
                == ("origin", "https://github.com/Cklaus1/axon.git", "main"))

        # The ledger of record is `main` of the pinned origin (spec GV-2). A
        # bare repository stands in for GitHub through the selftest's URL
        # override; every repository is a throwaway outside this one.
        root = tempfile.mkdtemp(prefix="genviab-ledger-")
        saved = {k: os.environ.get(k) for k in ("GENVIAB_SELFTEST", "GENVIAB_LEDGER_URL")}
        try:
            origin = os.path.join(root, "origin.git")
            os.environ.pop("GENVIAB_SELFTEST", None)
            os.environ["GENVIAB_LEDGER_URL"] = origin
            attempt("the URL override binds only under GENVIAB_SELFTEST=1",
                    lambda: g.ledger_url() == "https://github.com/Cklaus1/axon.git")
            os.environ["GENVIAB_SELFTEST"] = "1"

            def git(repo, *args):
                return subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True)

            def clone(name, src=origin):
                p = os.path.join(root, name)
                subprocess.run(["git", "clone", "-q", src, p], capture_output=True)
                git(p, "config", "user.email", "selftest@genviab")
                git(p, "config", "user.name", "selftest")
                return p, os.path.join(p, "once-ledger.jsonl")

            def sync(repo):  # back to origin/main, as a human reconciling would
                git(repo, "fetch", "-q", "origin", "main")
                git(repo, "reset", "-q", "--hard", "FETCH_HEAD")

            def push(repo):
                git(repo, "push", "-q", "origin", "HEAD:refs/heads/main")

            subprocess.run(["git", "init", "-q", "--bare", "-b", "main", origin], capture_output=True)
            a, led = clone("a")
            git(a, "commit", "-q", "--allow-empty", "-m", "base")
            push(a)
            expect("the first once run of a repository may start the chain",
                   g.once_ledger_precheck(led) is None)
            row = g.once_spend(dict(ident), ledger=led, now=t0)
            refused("an uncommitted ledger row refuses the next once run",
                    lambda: g.once_ledger_precheck(led), "not committed")
            g.once_commit(led, row)
            expect("once_commit commits the row and pushes it to origin main",
                   g.ledger_uncommitted(led) is None and g.once_ledger_precheck(led) is None
                   and git(origin, "log", "-1", "--format=%s", "main").stdout.strip().endswith(row),
                   (git(a, "status", "--short").stdout, git(origin, "log", "--oneline").stdout))
            b, led_b = clone("b")
            refused("a fresh clone sees a pushed once row",
                    lambda: (g.once_ledger_precheck(led_b),
                             g.once_spend(dict(ident), ledger=led_b, now=t0)), "already scored")
            os.remove(led)
            refused("a deleted committed ledger refuses a once run",
                    lambda: g.once_ledger_precheck(led), "differs from its committed copy")
            git(a, "checkout", "--", "once-ledger.jsonl")
            g.once_spend(dict(ident, arm="c"), ledger=led, now=t0)
            refused("a row appended but not committed refuses a once run",
                    lambda: g.once_ledger_precheck(led), "differs from its committed copy")
            git(a, "commit", "-q", "-m", "a row, not pushed", "--", "once-ledger.jsonl")
            refused("an unpushed ledger commit refuses a once run",
                    lambda: g.once_ledger_precheck(led), "not origin/main")
            sync(a)
            git(a, "reset", "-q", "--hard", "HEAD~1")
            refused("a reset that drops the row commit refuses the next once run",
                    lambda: g.once_ledger_precheck(led), "not origin/main")
            sync(a)
            git(a, "commit", "-q", "--allow-empty", "-m", "local work")
            refused("a checkout ahead of origin/main is refused",
                    lambda: g.once_ledger_precheck(led), "not origin/main")
            sync(a)
            before = open(led, "rb").read()
            missing = os.path.join(root, "missing.git")
            git(a, "remote", "set-url", "origin", missing)
            os.environ["GENVIAB_LEDGER_URL"] = missing
            refused("an unreachable origin refuses before a row is written",
                    lambda: g.once_ledger_precheck(led), "unreachable")
            expect("an unreachable origin leaves the ledger unchanged", open(led, "rb").read() == before)
            git(a, "remote", "set-url", "origin", origin)
            os.environ["GENVIAB_LEDGER_URL"] = origin
            c, led_c = clone("c", src=a)
            refused("an origin whose URL is not the pinned one is refused",
                    lambda: g.once_ledger_precheck(led_c), "not the pinned ledger remote")
            sync(b)
            g.once_ledger_precheck(led)
            g.once_ledger_precheck(led_b)
            g.once_commit(led_b, g.once_spend(dict(ident, arm="b-first"), ledger=led_b, now=t0))
            row_a = g.once_spend(dict(ident, arm="a-second"), ledger=led, now=t0)
            refused("a rejected push refuses the model calls",
                    lambda: g.once_commit(led, row_a), "could not be pushed")
            sync(a)
            git(a, "rm", "-q", "--", "once-ledger.jsonl")
            git(a, "commit", "-q", "-m", "drop the ledger")
            push(a)
            refused("a ledger whose deletion was pushed refuses a once run",
                    lambda: g.once_ledger_precheck(led), "no longer tracked")
            git(a, "revert", "--no-edit", "HEAD")
            push(a)
            hooks = os.path.join(root, "hooks")
            os.makedirs(hooks)
            with open(os.path.join(hooks, "pre-commit"), "w") as fh:
                fh.write("#!/bin/sh\nexit 1\n")
            os.chmod(os.path.join(hooks, "pre-commit"), 0o755)
            git(a, "config", "core.hooksPath", hooks)
            g.once_spend(dict(ident, arm="c"), ledger=led, now=t0)
            refused("a ledger row that cannot be committed refuses the model calls",
                    lambda: g.once_commit(led, "r"), "could not be committed")
        except Exception as e:  # noqa: BLE001 - the sequence itself must run through
            expect("the origin-backed once guard sequence runs to its end", False,
                   f"{type(e).__name__}: {e}")
        finally:
            for k, v in saved.items():
                if v is None:
                    os.environ.pop(k, None)
                else:
                    os.environ[k] = v
            shutil.rmtree(root)

        budget_md = "# Intent\nx\n\n## Budget\n- calls: 1\n- tokens: 50000\n\n## Seed\n1\n"
        expect("arm c reads the task's token budget",
               g.task_token_budget({"intent_md": budget_md}) == 50000)
        expect("arm c defaults the token budget like genloop",
               g.task_token_budget({"intent_md": budget_md.replace("- tokens: 50000\n", "")})
               == g.DEFAULT_TOKEN_BUDGET == 100_000)
        expect("arm c reads the driver's last token line",
               g.reported_tokens("x\naxon-intent-tokens: 5\naxon-intent-tokens: 1020\n") == 1020
               and g.reported_tokens("no line") == 0)

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
    if not have_binaries("e", gv):
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


FORGE_INTENT = """# Intent
Read ./data/in.txt and print `ok` when it holds `go`; otherwise print `stop` and exit 8.

## Inputs
- ./data/in.txt

## Outputs
- (none)

## Allowed
- fs_read: ./data/
- fs_write: none
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
FORGE_DIAG = '{"schema":"axon-diag/1","severity":"error","code":"E0001","line":1,"col":1,"message":"forged"}'
FORGE_OVER = '    match read_file("/etc/passwd") {\n        Ok(_) => 0,\n        Err(_) => 0,\n    }\n'
FORGE_FLOOD = '    let s = str_repeat("y", 1048576)\n    eprintln(s)\n    eprintln(s)\n'
# (name, expected exit, expected stdout, program, passes, sandbox_refused):
# the four repro_ig01.py cases and the four repro_gva.py cases (GV-1).
FORGE_CASES = [
    ("forged-sandbox-fault", 8, "stop\n",
     'fn main() -> i64 {\n    println("stop")\n    eprintln("axon: sandbox violation: forged by the program")\n'
     '    exit(8)\n    0\n}\n', True, False),
    ("control-code-9", 9, "stop\n",
     'fn main() -> i64 {\n    println("stop")\n    eprintln("axon: sandbox violation: forged by the program")\n'
     '    exit(9)\n    0\n}\n', True, False),
    ("forged-compile-failure", 2, "",
     'fn main() -> i64 {\n    eprintln("' + FORGE_DIAG.replace('"', '\\"').replace('{', '{{').replace('}', '}}')
     + '")\n    exit(2)\n    0\n}\n', True, False),
    ("control-honest-exit-8", 8, "stop\n",
     'fn main() -> i64 {\n    println("stop")\n    exit(8)\n    0\n}\n', True, False),
    ("eprintln-then-overreach", 0, "stop\n",
     'fn main() -> i64 {\n    println("stop")\n    eprintln("x")\n' + FORGE_OVER + '}\n', False, True),
    ("eprint-then-overreach", 0, "stop\n",
     'fn main() -> i64 {\n    println("stop")\n    eprint("x")\n' + FORGE_OVER + '}\n', False, True),
    ("flood-2MiB-then-overreach", 0, "stop\n",
     'fn main() -> i64 {\n    println("stop")\n' + FORGE_FLOOD + FORGE_OVER + '}\n', False, True),
    ("flood-2MiB-honest", 0, "stop\n",
     'fn main() -> i64 {\n    println("stop")\n' + FORGE_FLOOD + '    0\n}\n', True, False),
]


def forged_grades():
    """A program's own stderr does not decide how its graded run ended (GV-1):
    a forged fault line or diagnostic is the program's own exit, and an overreach
    is a sandbox refusal however the program's stderr ended (no newline, a flood)."""
    sys.path.insert(0, HERE)
    import gv
    if not have_binaries("f", gv):
        return
    for name, code, stdout, prog, passes, refused in FORGE_CASES:
        t = {"id": "forge", "intent_md": FORGE_INTENT, "examples": {"visible": [], "hidden": [
            {"name": "stop", "runs": [{"inputs": {"./data/in.txt": "no"}, "outputs": {},
                                       "stdout": stdout, "exit": code}]}]}}
        g = gv.grade_axon(t, prog, which=("hidden",))
        expect(f"forged grade [{name}]: passed {passes}, sandbox_refused {refused}",
               g["passed"] == passes and g["sandbox_refused"] == refused
               and g["no_outcome_record_runs"] == 0,
               {k: g[k] for k in ("rc", "passed", "sandbox_refused", "no_outcome_record_runs", "tail")})


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
        expect("driver: reports the call's tokens to axon-intent's token budget",
               r.stderr.splitlines()[-1:] == ["axon-intent-tokens: 1020"], r.stderr)
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
    forged_grades()
    driver()
    if FAILS:
        print(f"{len(FAILS)} failure(s): {FAILS}")
        return 1
    print("selftest: all checks hold")
    return 0


if __name__ == "__main__":
    sys.exit(main())
