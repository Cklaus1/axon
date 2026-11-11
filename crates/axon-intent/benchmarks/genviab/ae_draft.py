#!/usr/bin/env python3
"""ae_draft: draft the `ae` candidate pool under D12 (tasks/authoring-eval-spec.md AE-3, §6, D12).

    python3 -B ae_draft.py plan    --store DIR
    python3 -B ae_draft.py draft   --store DIR --cap-usd X [--reserve-usd Y] [--limit N] [--only ID,...]
                                   [--parallel N] [--max-drafts N] [--driver PATH]
    python3 -B ae_draft.py earn-ax --store DIR --cap-usd X [--reserve-usd Y] [--limit N] [--only ID,...]
                                   [--parallel N]
    python3 -B ae_draft.py review  --store DIR
    python3 -B ae_draft.py spent   --store DIR

Code only. Every input (`ae-1/families.json`, `drafting/briefs/<tier>.md`) and every output lives in the
private store DIR (contamination rule 2, decision E6); nothing here names a task.

`plan` turns the families into candidate ids `ae-<sha8>` (a secret salt keeps ids opaque) and writes
`drafting/plan.json`, tiers interleaved so a partial run spans every tier. It refuses an existing plan.

`draft` (stage 1, D12): Haiku 5.5, through genviab's micode driver (`drivers/micode-program.sh`, the
Anthropic OAuth route), writes each candidate's `intent.md`, example INPUTS and `reference.py` from its
tier brief and family idea. The expected outputs come from running `reference.py` under gv's pinned bwrap
runner (`gv.PY_RUNNER`, `gv.bwrap_argv`), twice, under PYTHONHASHSEED 0 and 1; they are never typed by a
model. A draft is refused when its reply does not parse, its intent breaks a build.py check (one
paragraph, no code, fs prefixes), its reference raises, times out, leaves the ceiling or differs between
the two runs, gv's reference grader does not pass it on the recorded outputs, no hidden run stages a new
input, or the empty Axon program passes the hidden examples. A refused draft is redrafted with the defect
(up to --max-drafts drafts); then the candidate is dropped and logged. A kept candidate is written to
`ae-1/candidates/<id>/` (intent.md, task.json with family, tier, a fresh uuid4 canary and
`drafted_by: haiku-5-5`, reference.py). Calls are logged per candidate in `drafting/calls/<id>.jsonl`
(the driver's accounting rows), raw replies and defects in `drafting/attempts/<id>/`, the outcome in
`drafting/status/<id>.json` and one event row per candidate (calls, tokens, $) in `drafting/log.jsonl`.
Resumable: a candidate with a status or a candidate directory is not drafted again; an infrastructure
failure (micode error, rate limit, cap stop) leaves it undrafted.

`earn-ax` (stage 2, D12): genviab arm a with Haiku (`--generator micode`, <= 3 repairs) on the candidates
without `reference.ax`, against the examples stage 1 recorded. A verified program (visible and hidden
examples) becomes `reference.ax`; a candidate whose program is not verified is listed in
`ae-1/review/needs-human-ax.txt` for the human reviewer (D12 fallback). Infra rows are retried by a later
call.

Spend: every model call is priced with run_ab's list prices (`genviab.spent`) over every `calls/`
directory under `drafting/` (stage 2's genviab out dirs live there too). A call is made only while
spent + (in flight + 1) x reserve <= --cap-usd, the reserve being genviab's 3x one draft's estimate
(--reserve-usd; default 3x the larger of the spec's per-draft estimate and the dearest draft call
logged so far). --cap-usd is the TOTAL across every call ever logged in the store.

`review` writes `ae-1/review/candidates.tsv` (id, tier, family, refs-agree, reference.ax) and the
`README.md` for the human ambiguity review (HR-1).
"""
import argparse
import concurrent.futures as cf
import datetime
import hashlib
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import threading
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import build  # noqa: E402
import genviab  # noqa: E402
import gv  # noqa: E402

MODEL = "claude-haiku-5-5"
DRAFTED_BY = "haiku-5-5"
TIERS = ("T1", "T2", "T3", "TS")
SET = "ae-1"
# Spec §6: one draft call ~ 12k in + 4k out = $0.0032; stage 2 per candidate 1.7-4 calls x
# (15.8k in + 3k out) = $0.0053-0.0124.
DRAFT_ESTIMATE_USD = 0.0032
AX_ESTIMATE_USD = 0.0124
RESERVE_X = 3
SECTIONS = ("intent.md", "examples.json", "reference.py")
INPUT_CAP = 4096                 # bytes per staged input file
TASK_JSON_CAP = 64 * 1024
BUDGET_SECTION = "## Budget\n- calls: 100\n- tokens: 120000\n- cost_micro: 1000000\n"
SEED_SECTION = "## Seed\n- 42\n"
VARIANT_TEXT = {
    1: "Variant 1 of 2: the most direct reading of the family idea.",
    2: ("Variant 2 of 2: a different setting from the most direct reading of the idea (other domain words, "
        "file names, field names, separators and data), plus one extra rule (an additional error path, "
        "tie rule or edge case)."),
}
PREV_REPLY_CAP = 12000


class Stop(Exception):
    pass


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def jdump(obj, path):
    tmp = path + ".tmp"
    with open(tmp, "w") as fh:
        fh.write(json.dumps(obj, indent=1, ensure_ascii=False) + "\n")
    os.replace(tmp, path)


class Store:
    def __init__(self, root):
        self.root = os.path.abspath(root)
        self.families = os.path.join(self.root, SET, "families.json")
        self.cands = os.path.join(self.root, SET, "candidates")
        self.review = os.path.join(self.root, SET, "review")
        self.drafting = os.path.join(self.root, "drafting")
        self.plan = os.path.join(self.drafting, "plan.json")
        self.calls = os.path.join(self.drafting, "calls")
        self.attempts = os.path.join(self.drafting, "attempts")
        self.status = os.path.join(self.drafting, "status")
        self.log = os.path.join(self.drafting, "log.jsonl")
        self.ax_runs = os.path.join(self.drafting, "ax-runs")

    def brief(self, tier):
        return open(os.path.join(self.drafting, "briefs", f"{tier}.md")).read()

    def spent(self):
        return genviab.spent(self.drafting) if os.path.isdir(self.drafting) else 0.0

    def status_of(self, cid):
        p = os.path.join(self.status, f"{cid}.json")
        return json.load(open(p)) if os.path.exists(p) else None

    def set_status(self, cid, st):
        os.makedirs(self.status, exist_ok=True)
        jdump(st, os.path.join(self.status, f"{cid}.json"))

    def append_log(self, row):
        os.makedirs(self.drafting, exist_ok=True)
        with open(self.log, "a") as fh:
            fh.write(json.dumps(row, ensure_ascii=False) + "\n")


# ---------------------------------------------------------------- plan

def cmd_plan(a):
    st = Store(a.store)
    if os.path.exists(st.plan):
        raise Stop(f"{st.plan} exists: a plan is written once (ids must stay stable)")
    fams = json.load(open(st.families))["families"]
    salt = secrets.token_hex(16)
    per_tier = {t: [] for t in TIERS}
    for v in (1, 2):
        for f in fams:
            if f["tier"] not in per_tier:
                raise Stop(f"family {f['id']}: unknown tier {f['tier']!r}")
            if v <= f.get("variants", 2):
                cid = "ae-" + hashlib.sha256(f"{salt}\0{f['id']}\0{v}".encode()).hexdigest()[:8]
                per_tier[f["tier"]].append({"id": cid, "family": f["id"], "tier": f["tier"], "variant": v})
    order = []
    for i in range(max(len(x) for x in per_tier.values())):
        order += [per_tier[t][i] for t in TIERS if i < len(per_tier[t])]
    if len({c["id"] for c in order}) != len(order):
        raise Stop("candidate id collision; rerun plan")
    os.makedirs(st.drafting, exist_ok=True)
    jdump({"schema": "axon-ae-plan/1", "set": SET, "written": now(), "salt": salt, "candidates": order},
          st.plan)
    print(f"plan: {len(order)} candidates " + ", ".join(f"{t} {len(per_tier[t])}" for t in TIERS))
    return 0


def load_plan(st):
    if not os.path.exists(st.plan):
        raise Stop(f"no plan at {st.plan}: run `ae_draft.py plan` first")
    plan = json.load(open(st.plan))["candidates"]
    fams = {f["id"]: f for f in json.load(open(st.families))["families"]}
    return plan, fams


def select(plan, a, todo):
    if a.only:
        want = a.only.split(",")
        unknown = sorted(set(want) - {c["id"] for c in plan})
        if unknown:
            raise Stop(f"--only names candidates not in the plan: {unknown}")
        plan = [c for c in plan if c["id"] in want]
    plan = [c for c in plan if todo(c)]
    return plan[:a.limit] if a.limit is not None else plan


# ---------------------------------------------------------------- the draft reply

def strip_fence(text):
    t = text.strip("\n")
    lines = t.split("\n")
    if len(lines) >= 2 and lines[0].lstrip().startswith("```") and lines[-1].strip() == "```":
        t = "\n".join(lines[1:-1])
    return t.strip("\n") + "\n"


def parse_reply(reply):
    """{section: text} or a defect string."""
    marks = [(m.group(1), m.start(), m.end())
             for m in re.finditer(r"^=== *(intent\.md|examples\.json|reference\.py) *$", reply, re.M)]
    names = [m[0] for m in marks]
    if sorted(names) != sorted(SECTIONS) or len(names) != len(set(names)):
        return f"the reply needs exactly the sections {', '.join(SECTIONS)}; it has {names}"
    out = {}
    for i, (name, _, end) in enumerate(marks):
        stop = marks[i + 1][1] if i + 1 < len(marks) else len(reply)
        out[name] = strip_fence(reply[end:stop])
    return out


def canonical_intent(md):
    """intent.md rebuilt from the reply's `# Intent`, `## Inputs`, `## Outputs`, `## Allowed`, with the
    spec's `## Budget` (tokens 120000, §2) and `## Seed`; or a defect string."""
    m = re.search(r"^#\s+Intent\s*$(.*?)(?=^##\s|\Z)", md, re.M | re.S)
    if not m or not m.group(1).strip():
        return "intent.md has no `# Intent` paragraph"
    parts = ["# Intent\n" + m.group(1).strip("\n") + "\n"]
    for title in ("Inputs", "Outputs", "Allowed"):
        if not re.search(rf"^##\s+{title}\s*$", md, re.M | re.I):
            return f"intent.md has no `## {title}` section"
        body = "\n".join(l for l in gv.section(md, title).split("\n") if l.strip())
        parts.append(f"## {title}\n" + (body + "\n" if body else ""))
    return "\n".join(parts + [BUDGET_SECTION, SEED_SECTION])


def bullets(md, title):
    return [l.strip()[2:].strip() for l in gv.section(md, title).split("\n")
            if l.strip().startswith(("- ", "* "))]


def check_examples(ex, ceil):
    if not isinstance(ex, dict) or set(ex) != {"visible", "hidden"}:
        return "examples.json must be an object with exactly `visible` and `hidden`"
    if not 1 <= len(ex["visible"]) <= 3:
        return f"{len(ex['visible'])} visible examples (1-3 wanted)"
    if len(ex["hidden"]) < 3:
        return f"{len(ex['hidden'])} hidden examples (at least 3, 4-7 wanted)"
    for w in ("visible", "hidden"):
        for e in ex[w]:
            if not isinstance(e, dict) or not isinstance(e.get("name"), str) or not e["name"].strip():
                return f"a {w} example has no name"
            if not isinstance(e.get("runs"), list) or not e["runs"]:
                return f"{w} `{e['name']}` has no runs"
            for run in e["runs"]:
                if not isinstance(run, dict) or set(run) - {"inputs"}:
                    return f"{w} `{e['name']}`: a run holds only `inputs` (outputs come from reference.py)"
                inputs = run.get("inputs", {})
                if not isinstance(inputs, dict):
                    return f"{w} `{e['name']}`: inputs is not an object"
                for p, c in inputs.items():
                    if not isinstance(c, str):
                        return f"{w} `{e['name']}`: input {p} is not a string"
                    if not gv.under(p, ceil["fs_read"]):
                        return f"{w} `{e['name']}`: input {p} is not under fs_read {ceil['fs_read']}"
                    if len(c.encode()) > INPUT_CAP:
                        return f"{w} `{e['name']}`: input {p} is over {INPUT_CAP} bytes"
                run["inputs"] = inputs
    return None


def check_ceiling(md):
    try:
        ceil = gv.ceiling(md)
    except ValueError as e:
        return None, f"## Allowed: {e}"
    if ceil["net"] or ceil["exec"] != "none":
        return None, "## Allowed must say `net: none` and `exec: none`"
    if not ceil["fs_read"] or not ceil["fs_write"]:
        return None, "## Allowed needs an fs_read and an fs_write prefix"
    for axis in ("fs_read", "fs_write"):
        for p in ceil[axis]:
            if gv.norm_path(p) is None or not p.endswith("/") or not p.startswith("./"):
                return None, f"{axis} prefix {p!r} is not a `./<dir>/` prefix"
    return ceil, None


# ---------------------------------------------------------------- outputs from reference.py

def produce(src, examples, ceil, declared, hashseed):
    """Run reference.py on every example (runs of one example share a workspace, as in gv) and record
    each run's declared outputs that exist, its stdout when non-empty, and its exit status. Returns
    (filled examples, None) or (None, defect)."""
    filled = {"visible": [], "hidden": []}
    env = dict(gv.PY_ENV, PYTHONHASHSEED=hashseed)
    for w in ("visible", "hidden"):
        for ex in examples[w]:
            ws = tempfile.mkdtemp(prefix="ae-draft-py-")
            host_w, gv_dir = os.path.join(ws, "w"), os.path.join(ws, "gv")
            fault = os.path.join(gv_dir, "fault", "raised")
            os.makedirs(host_w)
            os.makedirs(os.path.dirname(fault))
            try:
                with open(os.path.join(host_w, "main.py"), "w") as fh:
                    fh.write(src)
                with open(os.path.join(gv_dir, "run.py"), "w") as fh:
                    fh.write(gv.PY_RUNNER)
                runs = []
                for n, run in enumerate(ex["runs"], 1):
                    where = f"{w} `{ex['name']}` run {n}"
                    for p, c in run["inputs"].items():
                        hp = os.path.join(host_w, gv.norm_path(p))
                        os.makedirs(os.path.dirname(hp), exist_ok=True)
                        with open(hp, "w") as fh:
                            fh.write(c)
                    if os.path.exists(fault):
                        os.remove(fault)
                    try:
                        r = subprocess.run(gv.bwrap_argv(host_w, ceil, gv_dir), capture_output=True,
                                           timeout=gv.PY_TIMEOUT, env=env)
                    except subprocess.TimeoutExpired:
                        return None, f"reference.py timed out after {gv.PY_TIMEOUT} s on {where}"
                    err = r.stderr.decode("utf-8", "replace")
                    tail = " | ".join([l for l in err.splitlines() if l.strip()][-4:])[:600]
                    if os.path.exists(fault):
                        return None, f"reference.py raised an exception on {where}: {tail}"
                    if gv.PY_DENIAL.search(err):
                        return None, f"reference.py touched something outside its ceiling on {where}: {tail}"
                    if not 0 <= r.returncode <= 255:
                        return None, f"reference.py ended with status {r.returncode} on {where}"
                    if len(r.stdout) > gv.REFERENCE_STDOUT_CAP:
                        return None, f"reference.py printed over {gv.REFERENCE_STDOUT_CAP} bytes on {where}"
                    rec = {"inputs": run["inputs"], "outputs": {}}
                    try:
                        for p in declared:
                            hp = os.path.join(host_w, gv.norm_path(p))
                            if os.path.isfile(hp):
                                rec["outputs"][p] = open(hp, "rb").read().decode("utf-8")
                        out = r.stdout.decode("utf-8")
                    except UnicodeDecodeError:
                        return None, f"reference.py wrote bytes that are not UTF-8 on {where}"
                    if out:
                        rec["stdout"] = out
                    rec["exit"] = r.returncode
                    runs.append(rec)
                filled[w].append({"name": ex["name"], "runs": runs})
            finally:
                shutil.rmtree(ws, ignore_errors=True)
    return filled, None


def category(tier, examples):
    runs = [r for w in ("visible", "hidden") for e in examples[w] for r in e["runs"]]
    if any(len(e["runs"]) > 1 for w in ("visible", "hidden") for e in examples[w]):
        return "stateful"
    if any(r["exit"] != 0 for r in runs):
        return "errors"
    return "transform" if tier == "T1" else "multi-step"


def build_candidate(c, fam, reply):
    """(files {name: text}, None) or (None, defect). Everything after the reply is local and free."""
    sec = parse_reply(reply)
    if isinstance(sec, str):
        return None, sec
    md = canonical_intent(sec["intent.md"])
    if not md.startswith("# Intent"):
        return None, md
    ceil, why = check_ceiling(md)
    if why:
        return None, why
    try:
        examples = json.loads(sec["examples.json"])
    except ValueError as e:
        return None, f"examples.json does not parse: {e}"
    why = check_examples(examples, ceil)
    if why:
        return None, why
    declared = bullets(md, "Outputs")
    if not declared:
        return None, "## Outputs lists no file"
    for p in declared:
        if not gv.under(p, ceil["fs_write"]):
            return None, f"output {p} is not under fs_write {ceil['fs_write']}"
    src = sec["reference.py"]
    t = {"id": c["id"], "intent_md": md, "examples": examples, "reference": src,
         "reference_lang": "python", "reference_file": "reference.py", "references": ["reference.py"]}
    for name, fn in (("intent-text", build.check_intent_text), ("v1-scope", build.check_v1_python),
                     ("path-lint", build.check_path_lint), ("hidden", build.check_hidden)):
        why = fn(t)
        if why:
            return None, f"{name}: {why}"
    first, why = produce(src, examples, ceil, declared, "0")
    if why:
        return None, why
    second, why = produce(src, examples, ceil, declared, "1")
    if why:
        return None, f"second run: {why}"
    if first != second:
        return None, "reference.py is nondeterministic: two runs (hash seeds 0 and 1) differ"
    t["examples"] = first
    seen = {p for w in ("visible", "hidden") for e in first[w] for r in e["runs"] for p in r["outputs"]}
    never = [p for p in declared if p not in seen]
    if never:
        return None, f"declared outputs {never} are never written on any example"
    g = gv.grade_python_reference(t, src)
    if not g["passed"]:
        return None, f"reference.py fails gv's grader on its own outputs: {g['tail'][:400]}"
    empty = gv.grade_axon(t, gv.EMPTY_PROGRAM, ("hidden",))
    if empty["stage"] == "infrastructure":
        raise Stop(f"the empty-program lift failed (binaries? AXON_BIN={gv.AXON}): {empty['tail'][-300:]}")
    if empty["passed"]:
        return None, "an empty main passes the hidden examples"
    task = {
        "id": c["id"], "category": category(c["tier"], first),
        "source": {"kind": "ae candidate drafted under D12",
                   "note": f"family idea from {fam['source']}; drafted by Haiku 5.5, outputs from reference.py"},
        "reference_author": f"model:{MODEL} (D12 draft; outputs computed by running reference.py)",
        "family": c["family"], "tier": c["tier"], "variant": c["variant"],
        "canary": str(uuid.uuid4()), "drafted_by": DRAFTED_BY, "examples": first,
    }
    tj = json.dumps(task, indent=1, ensure_ascii=False) + "\n"
    if len(tj.encode()) > TASK_JSON_CAP:
        return None, f"task.json is over {TASK_JSON_CAP} bytes"
    return {"intent.md": md, "task.json": tj, "reference.py": src}, None


def write_candidate(st, cid, files):
    os.makedirs(st.cands, exist_ok=True)
    tmp = os.path.join(st.cands, f".tmp-{cid}")
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    for name, text in files.items():
        with open(os.path.join(tmp, name), "w") as fh:
            fh.write(text)
    os.rename(tmp, os.path.join(st.cands, cid))


# ---------------------------------------------------------------- stage 1: draft

def draft_prompt(brief, fam, c):
    return (brief.rstrip("\n") + "\n\n## Your candidate\n\n"
            f"- Family idea: {fam['idea']}\n- Where the idea comes from: {fam['source']}\n"
            f"- {VARIANT_TEXT[c['variant']]}\n\nReply in the three-section format now.\n")


def retry_prompt(base, prev, defect):
    return (base + "\n## Your previous draft was refused\n\n"
            f"Defect: {defect}\n\nYour previous reply:\n\n{prev[:PREV_REPLY_CAP]}\n\n"
            "Write a complete corrected draft in the same three-section format.\n")


def call_rows(st, cid):
    return genviab.call_rows(os.path.join(st.calls, f"{cid}.jsonl"))


def call_driver(a, st, cid, prompt):
    """(reply or None, failure or None, the call's accounting row or None)."""
    env = dict(os.environ, GENVIAB_MODEL=MODEL, GENVIAB_MODEL_LOG=os.path.join(st.calls, f"{cid}.jsonl"),
               GENVIAB_TASK=cid, GENVIAB_ARM="draft", TMPDIR=os.environ.get("TMPDIR", "/tmp"))
    env.pop("GENVIAB_EFFORT", None)
    n0 = len(call_rows(st, cid))
    try:
        r = subprocess.run([a.driver], input=prompt, env=env, capture_output=True, text=True,
                           timeout=genviab.CALL_TIMEOUT_MS / 1000 + 60)
        rc, out, err = r.returncode, r.stdout, r.stderr
    except subprocess.TimeoutExpired:
        rc, out, err = None, "", "driver timed out"
    rows = call_rows(st, cid)
    row = rows[-1] if len(rows) > n0 else None
    if rc != 0:
        return None, (err.strip()[-300:] or f"driver exited {rc}"), row
    if row and row.get("model") and any(m != MODEL for m in row["model"]):
        return None, f"model substituted: {row['model']}", row
    return out, None, row


def rate_limited(row):
    return bool(row) and any(genviab.RATE_LIMIT.search(s)
                             for s in row.get("session_errors", []) + row.get("retries", []))


class Spend:
    """The cap rule shared by every worker: a call starts only while
    spent + (in flight + 1) x reserve <= cap."""

    def __init__(self, st, cap, reserve, estimate):
        self.st, self.cap, self.fixed, self.estimate = st, cap, reserve, estimate
        self.lock, self.in_flight, self.stopped, self.rl_run = threading.Lock(), 0, None, 0

    def reserve(self):
        if self.fixed is not None:
            return self.fixed
        dearest = max([genviab.call_cost(r) for d, _, fs in os.walk(self.st.drafting)
                       if os.path.basename(d) == "calls" for f in fs if f.endswith(".jsonl")
                       for r in genviab.call_rows(os.path.join(d, f))] or [0.0])
        return RESERVE_X * max(self.estimate, dearest)

    def acquire(self):
        with self.lock:
            if self.stopped:
                return False
            s, res = self.st.spent(), self.reserve()
            if s + (self.in_flight + 1) * res > self.cap:
                self.stopped = (f"BUDGET CAP: ${s:.4f} spent + {self.in_flight + 1} x ${res:.4f} reserve "
                                f"> ${self.cap}")
                print(self.stopped, file=sys.stderr, flush=True)
                return False
            self.in_flight += 1
            return True

    def release(self, row):
        with self.lock:
            self.in_flight -= 1
            self.rl_run = self.rl_run + 1 if rate_limited(row) else 0
            if self.rl_run >= genviab.RATE_LIMIT_STOP and not self.stopped:
                self.stopped = f"RATE LIMIT STOP: {self.rl_run} consecutive rate-limited calls"
                print(self.stopped, file=sys.stderr, flush=True)


def summarize(st, cid, arm=None):
    rows = [r for r in call_rows(st, cid) if arm is None or r.get("arm") == arm]
    return {"calls": len(rows), "input_tokens": sum(r.get("input_tokens") or 0 for r in rows),
            "output_tokens": sum(r.get("output_tokens") or 0 for r in rows),
            "cache_read": sum(r.get("cache_read") or 0 for r in rows),
            "cache_create": sum(r.get("cache_create") or 0 for r in rows),
            "cost_usd": round(sum(genviab.call_cost(r) for r in rows), 6)}


def draft_one(a, st, spend, c, fam):
    adir = os.path.join(st.attempts, c["id"])
    os.makedirs(adir, exist_ok=True)
    base = draft_prompt(st.brief(c["tier"]), fam, c)
    prompt, defect, n = base, None, len(os.listdir(adir)) // 2
    for _ in range(a.max_drafts):
        if not spend.acquire():
            return "stopped", spend.stopped
        row = None
        try:
            reply, fail, row = call_driver(a, st, c["id"], prompt)
        finally:
            spend.release(row)
        if fail:
            st.append_log({"ts": now(), "id": c["id"], "event": "infra", "why": fail[:300],
                           **summarize(st, c["id"])})
            return "infra", fail
        n += 1
        with open(os.path.join(adir, f"draft-{n}.txt"), "w") as fh:
            fh.write(reply)
        files, defect = build_candidate(c, fam, reply)
        jdump({"draft": n, "defect": defect, "ts": now()}, os.path.join(adir, f"draft-{n}.json"))
        if files:
            write_candidate(st, c["id"], files)
            summ = summarize(st, c["id"])
            st.set_status(c["id"], {"id": c["id"], "tier": c["tier"], "family": c["family"],
                                    "variant": c["variant"], "state": "drafted", "drafts": n, **summ})
            st.append_log({"ts": now(), "id": c["id"], "tier": c["tier"], "family": c["family"],
                           "event": "drafted", "drafts": n, **summ})
            return "drafted", None
        prompt = retry_prompt(base, reply, defect)
    summ = summarize(st, c["id"])
    st.set_status(c["id"], {"id": c["id"], "tier": c["tier"], "family": c["family"],
                            "variant": c["variant"], "state": "dropped", "drafts": n, "defect": defect, **summ})
    st.append_log({"ts": now(), "id": c["id"], "tier": c["tier"], "family": c["family"],
                   "event": "dropped", "drafts": n, "defect": defect, **summ})
    return "dropped", defect


def cmd_draft(a):
    st = Store(a.store)
    plan, fams = load_plan(st)
    todo = select(plan, a, lambda c: st.status_of(c["id"]) is None
                  and not os.path.exists(os.path.join(st.cands, c["id"])))
    os.makedirs(st.calls, exist_ok=True)
    spend = Spend(st, a.cap_usd, a.reserve_usd, DRAFT_ESTIMATE_USD)
    s0 = st.spent()
    print(f"draft: {len(todo)} candidates; ${s0:.4f} spent of ${a.cap_usd} before this call", flush=True)
    counts = {}
    with cf.ThreadPoolExecutor(max_workers=a.parallel) as ex:
        futs = {ex.submit(draft_one, a, st, spend, c, fams[c["family"]]): c for c in todo}
        for f in cf.as_completed(futs):
            c = futs[f]
            state, why = f.result()
            counts[state] = counts.get(state, 0) + 1
            print(f"{c['id']} {c['tier']} {c['family']}: {state}" + (f" ({why[:160]})" if why else ""),
                  flush=True)
    s1 = st.spent()
    print(f"draft: {counts}; this call ${s1 - s0:.4f}; total ${s1:.4f} of ${a.cap_usd}"
          + (f"; {spend.stopped}" if spend.stopped else ""), flush=True)
    return 3 if spend.stopped else 0


# ---------------------------------------------------------------- stage 2: earn reference.ax

def accepted_source(out, cid):
    """The accepted program of a set-dir arm-a run: `<out>/programs/<id>.ax`, else the
    transcript's last round when it was accepted; None when there is none."""
    prog = os.path.join(out, "programs", f"{cid}.ax")
    if os.path.exists(prog):
        return open(prog).read()
    transcript = os.path.join(out, "transcripts", f"{cid}.a.json")
    if not os.path.exists(transcript):
        return None
    rounds = json.load(open(transcript)).get("rounds") or []
    rep = (rounds[-1].get("report") or {}) if rounds else {}
    return rep.get("source") if rep.get("stage") == "accepted" else None


def needs_human(st):
    p = os.path.join(st.review, "needs-human-ax.txt")
    if not os.path.exists(p):
        return {}
    return {l.split("\t", 1)[0]: l.rstrip("\n") for l in open(p) if l.strip() and not l.startswith("#")}


def write_needs_human(st, rows):
    os.makedirs(st.review, exist_ok=True)
    with open(os.path.join(st.review, "needs-human-ax.txt"), "w") as fh:
        fh.write("# candidates whose reference.ax Haiku did not earn through arm a (<= 3 repairs); "
                 "the human writes it or drops the candidate (D12)\n# id\ttier\tfamily\twhy\n")
        for cid in sorted(rows):
            fh.write(rows[cid] + "\n")


def cmd_earn_ax(a):
    st = Store(a.store)
    plan, _ = load_plan(st)
    human = needs_human(st)
    todo = select(plan, a, lambda c: os.path.isdir(os.path.join(st.cands, c["id"]))
                  and not os.path.exists(os.path.join(st.cands, c["id"], "reference.ax"))
                  and c["id"] not in human)
    if not todo:
        print("earn-ax: nothing to do")
        return 0
    s = st.spent()
    reserve = a.reserve_usd if a.reserve_usd is not None else RESERVE_X * AX_ESTIMATE_USD
    if s + reserve > a.cap_usd:
        print(f"earn-ax: BUDGET CAP: ${s:.4f} spent + ${reserve} reserve > ${a.cap_usd}", file=sys.stderr)
        return 3
    # genviab's cap check is spent(results/) + spent(--out): give it results/' spend plus this
    # store's remaining cap (the fresh --out starts at $0).
    root_spent = genviab.spent()
    out = os.path.join(st.ax_runs, datetime.datetime.now().strftime("%Y%m%dT%H%M%S"))
    cmd = [sys.executable, "-B", os.path.join(HERE, "genviab.py"), "run", "--set-dir", st.cands,
           "--candidates", "--arm", "a", "--generator", "micode", "--model", MODEL, "--out", out,
           "--only", ",".join(c["id"] for c in todo), "--parallel", str(a.parallel),
           "--cap-usd", f"{root_spent + a.cap_usd - s:.6f}", "--reserve-usd", f"{reserve:.6f}"]
    print(f"earn-ax: {len(todo)} candidates; ${s:.4f} spent of ${a.cap_usd}; out {out}", flush=True)
    r = subprocess.run(cmd)
    rows = {row["id"]: row for row in genviab.call_rows(os.path.join(out, "rows.jsonl"))
            if row.get("id")}
    counts = {}
    for c in todo:
        cid = c["id"]
        row = rows.get(cid)
        d = os.path.join(st.cands, cid)
        if row is None or row.get("infra"):
            state, why = "infra", "no row" if row is None else "infra row"
        else:
            src = accepted_source(out, cid)
            t = gv.load_task(d, require_ax=False)
            g = gv.grade_axon(t, src) if (row.get("verified") and src) else None
            if g and g["passed"]:
                with open(os.path.join(d, "reference.ax"), "w") as fh:
                    fh.write(src if src.endswith("\n") else src + "\n")
                state, why = "earned", None
            else:
                why = ("accepted but not verified" if row.get("accepted")
                       else f"not accepted after {row.get('rounds')} rounds ({(row.get('round_stages') or [None])[-1]})")
                human[cid] = f"{cid}\t{c['tier']}\t{c['family']}\t{why}"
                state = "needs-human"
        prev = st.status_of(cid) or {"id": cid}
        prev.update(ax=state, ax_why=why, ax_run=os.path.relpath(out, st.root))
        prev["ax_calls"] = summarize_dir(os.path.join(out, "calls", f"{cid}.jsonl"))
        st.set_status(cid, prev)
        st.append_log({"ts": now(), "id": cid, "tier": c["tier"], "family": c["family"],
                       "event": f"ax-{state}", "why": why, **prev["ax_calls"]})
        counts[state] = counts.get(state, 0) + 1
    write_needs_human(st, human)
    print(f"earn-ax: {counts}; genviab rc {r.returncode}; total ${st.spent():.4f} of ${a.cap_usd}")
    return 0 if r.returncode == 0 else 3


def summarize_dir(path):
    rows = genviab.call_rows(path)
    return {"calls": len(rows), "input_tokens": sum(r.get("input_tokens") or 0 for r in rows),
            "output_tokens": sum(r.get("output_tokens") or 0 for r in rows),
            "cost_usd": round(sum(genviab.call_cost(r) for r in rows), 6)}


# ---------------------------------------------------------------- review packet

REVIEW_README = """# ae-1 candidate review (HR-1)

Human ambiguity review of every drafted candidate (spec AE-3 "Authorship and review"; build-loop-ae.md HR-1).
Generated by `ae_draft.py review`; `candidates.tsv` has one line per kept candidate.

Per candidate directory `../candidates/<id>/`: `intent.md` (the solver sees its `# Intent` paragraph and the
sections below it; the loop renders the visible examples from `task.json`), `task.json` (family, tier, canary,
every example with the outputs computed by running `reference.py`), `reference.py` (Haiku 5.5's draft), and
`reference.ax` when Haiku earned it through genviab arm a (<= 3 repairs, then graded on every example).

Columns: `id`, `tier`, `family`, `refs_agree` (`agree`: reference.py and reference.ax both pass every
visible and hidden example; `py-only`: no reference.ax yet; `DISAGREE`/`py-fails`: a defect), `reference_ax`
(`earned` or `needs-human`, see `needs-human-ax.txt`; `pending` when stage 2 has not run on it).

For each candidate, check: the `# Intent` paragraph alone determines every expected output (format, ties,
errors); no hidden example depends on a rule the paragraph does not state; the task is not a copy of a public
program. Fix or drop for ambiguity only; difficulty is tuned at tier level (AE-3 "Pilot, then freeze"). A
`needs-human` candidate needs a human-written `reference.ax` that reproduces the outputs in `task.json`, or is
dropped. After any edit, rerun the check below.

Counts at generation:

{counts}

`{check_cmd}` at generation: exit {check_rc}, {check_pass} PASS, {check_fail} FAIL.
{check_fails}
"""


def cmd_review(a):
    st = Store(a.store)
    plan, _ = load_plan(st)
    human = needs_human(st)
    lines, counts = [], {t: {"planned": 0, "drafted": 0, "dropped": 0, "undrafted": 0, "ax_earned": 0,
                             "needs_human": 0, "agree": 0} for t in TIERS}
    for c in plan:
        k = counts[c["tier"]]
        k["planned"] += 1
        d = os.path.join(st.cands, c["id"])
        stt = st.status_of(c["id"]) or {}
        if not os.path.isdir(d):
            k["dropped" if stt.get("state") == "dropped" else "undrafted"] += 1
            continue
        k["drafted"] += 1
        t = gv.load_task(d, require_ax=False)
        py = open(os.path.join(d, "reference.py")).read()
        ax_path = os.path.join(d, "reference.ax")
        py_ok = gv.grade_python_reference(t, py)["passed"]
        if os.path.exists(ax_path):
            k["ax_earned"] += 1
            ax_ok = gv.grade_axon(t, open(ax_path).read())["passed"]
            agree = "agree" if py_ok and ax_ok else ("py-fails" if not py_ok else "DISAGREE")
            ax = "earned"
        else:
            agree = "py-only" if py_ok else "py-fails"
            ax = "needs-human" if c["id"] in human else "pending"
        k["agree"] += agree == "agree"
        k["needs_human"] += ax == "needs-human"
        lines.append(f"{c['id']}\t{c['tier']}\t{c['family']}\t{agree}\t{ax}")
    chk = subprocess.run([sys.executable, "-B", os.path.join(HERE, "build.py"), "--check", "--candidates",
                          "--set-dir", st.cands], capture_output=True, text=True)
    chk_lines = chk.stdout.splitlines()
    fails = [l for l in chk_lines if l.startswith("FAIL")]
    os.makedirs(st.review, exist_ok=True)
    with open(os.path.join(st.review, "candidates.tsv"), "w") as fh:
        fh.write("id\ttier\tfamily\trefs_agree\treference_ax\n" + "".join(l + "\n" for l in sorted(lines)))
    table = ["| tier | planned | drafted | dropped | undrafted | ax earned | needs-human | refs agree |",
             "|---|---|---|---|---|---|---|---|"]
    for t in TIERS:
        k = counts[t]
        table.append(f"| {t} | {k['planned']} | {k['drafted']} | {k['dropped']} | {k['undrafted']} | "
                     f"{k['ax_earned']} | {k['needs_human']} | {k['agree']} |")
    table.append(f"\nDrafting spend: ${st.spent():.4f} nominal ({genviab.PRICE_SOURCE}; OAuth grant).")
    with open(os.path.join(st.review, "README.md"), "w") as fh:
        fh.write(REVIEW_README.format(
            counts="\n".join(table), check_cmd="build.py --check --candidates --set-dir ae-1/candidates",
            check_rc=chk.returncode, check_pass=sum(l.startswith("PASS") for l in chk_lines),
            check_fail=len(fails), check_fails="".join(f"\n    {l}" for l in fails)))
    print(f"build.py --check --candidates: exit {chk.returncode}, {len(fails)} FAIL")
    print("\n".join(table))
    return 0


def cmd_spent(a):
    st = Store(a.store)
    print(f"${st.spent():.4f} nominal across {st.drafting} ({genviab.PRICE_SOURCE})")
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(usage=__doc__.split("\n\n")[1])
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name in ("plan", "draft", "earn-ax", "review", "spent"):
        p = sub.add_parser(name)
        p.add_argument("--store", required=True)
        if name in ("draft", "earn-ax"):
            p.add_argument("--cap-usd", type=float, required=True)
            p.add_argument("--reserve-usd", type=float)
            p.add_argument("--limit", type=int)
            p.add_argument("--only")
            p.add_argument("--parallel", type=int, default=4)
        if name == "draft":
            p.add_argument("--max-drafts", type=int, default=2)
            p.add_argument("--driver", default=genviab.MICODE_DRIVER)
    a = ap.parse_args(argv)
    try:
        return {"plan": cmd_plan, "draft": cmd_draft, "earn-ax": cmd_earn_ax, "review": cmd_review,
                "spent": cmd_spent}[a.cmd](a)
    except Stop as e:
        print(f"ae_draft: REFUSED: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
