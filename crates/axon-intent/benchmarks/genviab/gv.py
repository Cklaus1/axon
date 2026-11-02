#!/usr/bin/env python3
"""genviab's shared code: the task format, the intents built from it, and
grading through the real `axon-intent` lift path. Imported by build.py and
genviab.py; runs nothing on import.

A task is a directory `<set>/<id>/` holding exactly:

    intent.md      the intent WITHOUT `## Examples` (what the operator wrote)
    task.json      metadata and the examples, visible and hidden
    reference.ax   a program that passes every example, or (once/ only)
    reference.py   a hand-written Python 3 reference (PREREG §4, option B),
                   graded by `grade_python_reference`; exactly one of the two

`task.json`:

    {"id": "<dir name>", "category": "transform|multi-step|stateful|errors|tempting",
     "source": {...where the task comes from...},
     "reference_author": "...",
     "human_expected": {"path": "./out/x", "value": "..."}       (optional)
     "tempting": {"forbidden": "net|exec|fs_write|fs_read", "asks": "...",
                  "branch_examples": ["<hidden example name>", ...]}   (tempting only)
     "examples": {"visible": [EXAMPLE...], "hidden": [EXAMPLE...]}}

    EXAMPLE = {"name": "...", "runs": [{"inputs": {"./data/x": "contents"},
                                        "outputs": {"./out/y": "contents"},
                                        "stdout": "..." (optional), "exit": 0}]}

The generation intent is intent.md plus the VISIBLE examples; the grading
intent adds the hidden examples and the candidate as a fenced program, and is
lifted with `axon-intent compile --transcript` (`axon-intent-lift/1`): the
same `axon check`, admission gates and ceiling-scoped sandbox the loop uses.
Expected outputs never reach the program's directory (IG4).
"""
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile

BENCH = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(BENCH, "..", "..", "..", ".."))
CORTEX_BENCH = os.path.join(ROOT, "crates", "axon-cortex", "benchmarks")
AXON = os.environ.get("AXON_BIN", os.path.join(ROOT, "target", "debug", "axon"))
AXON_INTENT = os.environ.get("AXON_INTENT_BIN",
                             os.path.join(os.path.dirname(AXON), "axon-intent"))

CATEGORIES = ("transform", "multi-step", "stateful", "errors", "tempting")
BASE_FILES = ("intent.md", "task.json")
REFERENCE_FILES = {"reference.ax": "axon", "reference.py": "python"}
TASK_FILES = BASE_FILES + tuple(REFERENCE_FILES)   # every file a task may hold
AXES = ("fs_read", "fs_write", "net", "exec")
EMPTY_PROGRAM = "fn main() -> i64 {\n    0\n}\n"
LIFT_SCHEMA = "axon-intent-lift/1"
SANDBOX_REFUSAL = "refused by the sandbox"
# axon-os `read_outcome_record`'s fail-closed reason, behind genloop's prefix (GV-1).
NO_OUTCOME_RECORD = SANDBOX_REFUSAL + ": no outcome record: "
LIFT_TIMEOUT = 300


def sha256_bytes(b):
    return hashlib.sha256(b).hexdigest()


def sha256_file(path):
    with open(path, "rb") as fh:
        return sha256_bytes(fh.read())


def task_dirs(set_dir):
    return sorted(os.path.join(set_dir, d) for d in os.listdir(set_dir)
                  if os.path.isdir(os.path.join(set_dir, d)))


def load_task(d):
    with open(os.path.join(d, "task.json")) as fh:
        t = json.load(fh)
    with open(os.path.join(d, "intent.md")) as fh:
        t["intent_md"] = fh.read()
    ceiling(t["intent_md"])   # a malformed `## Allowed` raises ValueError: the task is refused (B3-4)
    present = [n for n in REFERENCE_FILES if os.path.exists(os.path.join(d, n))]
    t["references"] = present
    t["reference_file"] = present[0] if len(present) == 1 else None
    t["reference_lang"] = REFERENCE_FILES[present[0]] if len(present) == 1 else None
    t["reference"] = (open(os.path.join(d, present[0])).read() if len(present) == 1 else None)
    t["dir"] = d
    return t


def task_files(t):
    """The files a task's manifest row hashes: intent.md, task.json, its reference."""
    return BASE_FILES + ((t["reference_file"],) if t.get("reference_file") else ())


def corpus_digest(set_dir):
    """sha256 over every file of every task (path and bytes), manifest excluded."""
    lines = []
    for d in task_dirs(set_dir):
        for name in sorted(os.listdir(d)):
            p = os.path.join(d, name)
            if os.path.isfile(p):
                lines.append(f"{os.path.relpath(p, set_dir)} {sha256_file(p)}")
    return sha256_bytes("\n".join(lines).encode())


def ceiling(intent_md):
    """The `## Allowed` section: axis -> list (net/fs) or str (exec), parsed as
    `axon-intent`'s `parse_allowed` (intent.rs) does: `- ` and `* ` bullets,
    `key: value` split on the first `:`, `none` (any case) or empty -> [],
    repeated keys extend, the last `exec` wins. Stricter than intent.rs, so it
    fails closed (B3-4): a non-blank line that is not a bullet, a bullet without
    `:`, an unknown key or a bad `exec`/`max_label` value raises ValueError."""
    out = {"fs_read": [], "fs_write": [], "net": [], "exec": "none"}
    for line in section(intent_md, "allowed").split("\n"):
        t = line.strip()
        if not t:
            continue
        if not t.startswith(("- ", "* ")):
            raise ValueError(f"## Allowed: not a bullet: {line!r}")
        b = t[2:].strip()
        if ":" not in b:
            raise ValueError(f"## Allowed: expected `key: value`, got {b!r}")
        k, v = (s.strip() for s in b.split(":", 1))
        if k in ("fs_read", "fs_write", "net"):
            if v and v.lower() != "none":
                out[k] += [x.strip() for x in v.split(",") if x.strip()]
        elif k == "exec":
            if v not in ("none", "any"):
                raise ValueError(f'## Allowed: exec must be "none" or "any", got {v!r}')
            out[k] = v
        elif k == "max_label":
            if v not in ("public", "internal", "secret"):
                raise ValueError(f"## Allowed: max_label must be public|internal|secret, got {v!r}")
        else:
            raise ValueError(f"## Allowed: unknown key {k!r}")
    return out


def section(md, title):
    """Body of the first `## <title>` section (title case-insensitive), '' if
    absent. Mirrors `axon-intent`'s `split_sections` (intent.rs; B3-4): outside a
    ``` fence, a line (trailing space stripped) starting with `## ` opens a
    section titled by the rest, trimmed and lower-cased, and one starting with
    `# ` ends it; fence lines and fenced lines belong to no section."""
    want, in_fence, found = title.strip().lower(), False, None
    for raw in md.split("\n"):   # str.lines(): only \n splits (splitlines() also splits on \r, \f, ...)
        line = raw.rstrip()
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        if line.startswith(("## ", "# ")):
            if found is not None:
                break
            if line.startswith("## ") and line[3:].strip().lower() == want:
                found = []
            continue
        if found is not None:
            found.append(line)
    return "\n".join(found) if found is not None else ""


def intent_paragraphs(md):
    """The prose paragraphs under `# Intent` (before the first `##`)."""
    m = re.search(r"^#\s+Intent\s*$(.*?)(?=^##\s|\Z)", md, re.M | re.S)
    body = m.group(1) if m else ""
    return [" ".join(l.strip() for l in p.splitlines() if l.strip())
            for p in re.split(r"\n\s*\n", body) if p.strip()]


def goal(md):
    """What the loop shows the generator of `# Intent`: its FIRST paragraph,
    lines joined by a space (axon-intent's `Intent::goal`). Later paragraphs
    reach no generator, so build.py rejects a task that has any."""
    ps = intent_paragraphs(md)
    return ps[0] if ps else ""



def norm_path(p):
    """`./out//x` -> `out/x`; None for an absolute path or a `..` component."""
    if p.startswith("/") or "\\" in p:
        return None
    parts = [c for c in p.split("/") if c not in ("", ".")]
    if not parts or ".." in parts:
        return None
    return "/".join(parts)


def under(path, prefixes):
    n = norm_path(path)
    for pre in prefixes:
        q = norm_path(pre)
        if n is not None and q is not None and (n == q or n.startswith(q + "/")):
            return True
    return False


def render_examples(examples):
    """The IG1 `## Examples` grammar; contents as exact JSON strings."""
    out = ["## Examples", ""]
    for ex in examples:
        out.append(f"### {ex['name']}")
        for n, run in enumerate(ex["runs"], 1):
            out.append(f"#### run {n}")
            for p, c in run.get("inputs", {}).items():
                out.append(f"- input {p}: {json.dumps(c)}")
            for p, c in run.get("outputs", {}).items():
                out.append(f"- output {p}: {json.dumps(c)}")
            if run.get("stdout") is not None:
                out.append(f"- stdout: {json.dumps(run['stdout'])}")
            out.append(f"- exit: {run.get('exit', 0)}")
        out.append("")
    return "\n".join(out)


def generation_intent(task):
    """What the generator's intent file holds: intent.md + visible examples."""
    md = task["intent_md"].rstrip("\n") + "\n"
    vis = task["examples"]["visible"]
    return md + ("\n" + render_examples(vis) if vis else "")


def grading_intent(task, program, which=("visible", "hidden")):
    """intent.md + the chosen examples + `program` as the fenced program.
    None when the program cannot be fenced (it contains ```), which grades
    as not verified."""
    if "```" in program:
        return None
    exs = [e for w in which for e in task["examples"][w]]
    body = program if program.endswith("\n") else program + "\n"
    return (task["intent_md"].rstrip("\n") + "\n\n" + render_examples(exs)
            + "\n## Program\n```axon\n" + body + "```\n")


def intent_env():
    env = dict(os.environ, AXON_BIN=AXON)
    env.pop("AXON_AI_MOCK", None)
    env.pop("AXON_APPROVAL_KEY", None)
    return env


def lift(intent_text, name="task"):
    """Lift a grading intent: (rc, lift report dict or None, stdout+stderr tail).
    rc 0 = `axon check`, the gates and every example passed."""
    if intent_text is None:
        return 2, None, "program contains ``` and cannot be fenced for grading"
    ws = tempfile.mkdtemp(prefix="genviab-lift-")
    try:
        p = os.path.join(ws, f"{name}.intent.md")
        with open(p, "w") as fh:
            fh.write(intent_text)
        rep = os.path.join(ws, "lift.json")
        r = subprocess.run([AXON_INTENT, "compile", p, "--out", os.path.join(ws, "o"),
                            "--transcript", rep], cwd=ws, env=intent_env(),
                           capture_output=True, text=True, timeout=LIFT_TIMEOUT)
        doc = None
        if os.path.exists(rep):
            with open(rep) as fh:
                doc = json.load(fh)
            if doc.get("schema") != LIFT_SCHEMA:
                raise SystemExit(f"lift report schema {doc.get('schema')!r} is not {LIFT_SCHEMA}")
        tail = (r.stdout + r.stderr)[-1500:]
        return r.returncode, (doc or {}).get("report"), tail
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def sandbox_refused(report):
    """Did any example run end in a sandbox refusal? `ended` is the supervisor's
    classification (axon-os `run_sandboxed_capture`), decided from the
    interpreter's outcome record on its own pipe, never from the program's
    stderr (GV-1). A run that ended without one valid record is a sandbox
    refusal too (fail closed), counted apart by `no_outcome_record_runs`."""
    return any(e.get("ended", "").startswith(SANDBOX_REFUSAL)
               for e in (report or {}).get("examples", []))


def no_outcome_record_runs(report):
    """Example runs that ended without one valid outcome record (GV-1 step 6):
    a sandbox refusal by rule, split out so a report tells them from real scope
    violations."""
    return sum(1 for e in (report or {}).get("examples", [])
               if e.get("ended", "").startswith(NO_OUTCOME_RECORD))


def failed_examples(report):
    return sorted({f"{e['example']}#{e['run']}" for e in (report or {}).get("examples", [])
                   if not e.get("passed")})


def grade_axon(task, program, which=("visible", "hidden")):
    """Grade an Axon program on the task's examples through the lift path."""
    rc, rep, tail = lift(grading_intent(task, program, which), task["id"])
    return {
        "rc": rc,
        "passed": rc == 0 and (rep or {}).get("stage") == "accepted",
        "stage": (rep or {}).get("stage"),
        "failed_examples": failed_examples(rep),
        "sandbox_refused": sandbox_refused(rep),
        "no_outcome_record_runs": no_outcome_record_runs(rep),
        "gate_refusal": (rep or {}).get("gate_refusal"),
        "tail": "" if rc == 0 else tail[-600:],
    }


# ---------------------------------------------------------------- python (arm c, reference.py)

# The pinned bwrap invocation (PREREG-genviab.md). `--remount-ro /` makes the
# whole root read-only, so the program's directory `/w` (a directory of that
# root) refuses writes; only the fs_write prefixes are bound writable and the
# fs_read prefixes read-only. `--unshare-all` drops the network.
BWRAP = "/usr/bin/bwrap"
BWRAP_BASE = ["--ro-bind", "/usr", "/usr", "--symlink", "usr/lib", "/lib",
              "--symlink", "usr/lib64", "/lib64", "--symlink", "usr/bin", "/bin",
              "--proc", "/proc", "--dev", "/dev", "--dir", "/w"]
BWRAP_TAIL = ["--chdir", "/w", "--unshare-all", "--die-with-parent", "--remount-ro", "/"]
PYTHON = "/usr/bin/python3"
PY_TIMEOUT = 20
STDOUT_CAP = 64 * 1024              # arm c's stdout bound
REFERENCE_STDOUT_CAP = 1 << 20      # runtime.rs DEFAULT_MAX_CAPTURE_BYTES: the Axon reference's bound
# The environment of every Python run, reference.py and arm c alike (spec GV-3
# table; O-GV-02; B3-1): nothing of the harness's own environment reaches the
# program, and PYTHONHASHSEED binds.
PY_ENV = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "PYTHONHASHSEED": "0"}
# The runner of every Python run (spec GV-3 step 2; B3-1 for arm c): a raised
# exception is a FAULT that fails the run whatever the exit status, reported
# through a file the harness reads, never through stderr. SystemExit is the
# program's own exit.
PY_RUNNER = '''import runpy, sys, traceback
sys.argv = ["/w/main.py"]
sys.path.insert(0, "/w")
try:
    runpy.run_path("/w/main.py", run_name="__main__")
except SystemExit:
    raise
except BaseException:
    traceback.print_exc()
    open("/gv/fault/raised", "w").close()
    sys.exit(1)
'''
PY_DENIAL = re.compile(r"PermissionError|Read-only file system|Errno (13|30|101)|"
                       r"Network is unreachable|Temporary failure in name resolution|"
                       r"Name or service not known")


def bwrap_argv(host_w, ceil, gv_dir):
    """bwrap argv for a program at host_w/main.py under ceiling `ceil`, for one
    run: fs_write prefixes are always bound (created empty, as Axon creates
    them); an fs_read prefix only when it exists on the host after this run's
    staging (Axon's `file_exists("./data/")` is false with no input; O-GV-03).
    The fault runner in `gv_dir` (run.py read-only, fault/ writable) runs it."""
    argv = [BWRAP] + BWRAP_BASE
    binds = {}
    for p in ceil["fs_read"]:
        if norm_path(p) and os.path.isdir(os.path.join(host_w, norm_path(p))):
            binds[norm_path(p)] = binds.get(norm_path(p), "--ro-bind")
    for p in ceil["fs_write"]:
        if norm_path(p):
            binds[norm_path(p)] = "--bind"
    for rel in sorted(binds):
        host = os.path.join(host_w, rel)
        os.makedirs(host, exist_ok=True)
        argv += [binds[rel], host, "/w/" + rel]
    argv += ["--ro-bind", os.path.join(host_w, "main.py"), "/w/main.py"]
    argv += ["--ro-bind", os.path.join(gv_dir, "run.py"), "/gv/run.py",
             "--bind", os.path.join(gv_dir, "fault"), "/gv/fault"]
    return argv + BWRAP_TAIL + [PYTHON, "-B", "-s", "-P", "/gv/run.py"]


def run_python_example(src, ex, ceil, cap):
    """One example, every run, under bwrap and the fault runner: list of run
    results. `cap`: the stdout bound in bytes (STDOUT_CAP for arm c,
    REFERENCE_STDOUT_CAP for reference.py)."""
    ws = tempfile.mkdtemp(prefix="genviab-py-")
    host_w = os.path.join(ws, "w")
    gv_dir = os.path.join(ws, "gv")
    fault = os.path.join(gv_dir, "fault", "raised")
    os.makedirs(host_w)
    os.makedirs(os.path.dirname(fault))
    try:
        with open(os.path.join(host_w, "main.py"), "w") as fh:
            fh.write(src)
        with open(os.path.join(gv_dir, "run.py"), "w") as fh:
            fh.write(PY_RUNNER)
        results = []
        for n, run in enumerate(ex["runs"], 1):
            for p, c in run.get("inputs", {}).items():
                hp = os.path.join(host_w, norm_path(p))
                os.makedirs(os.path.dirname(hp), exist_ok=True)
                with open(hp, "w") as fh:
                    fh.write(c)
            if os.path.exists(fault):
                os.remove(fault)
            argv = bwrap_argv(host_w, ceil, gv_dir)
            diffs = []
            try:
                r = subprocess.run(argv, capture_output=True, timeout=PY_TIMEOUT, env=PY_ENV)
                code, out, err = r.returncode, r.stdout, r.stderr.decode("utf-8", "replace")
                ended = f"exit {code}"
            except subprocess.TimeoutExpired:
                code, out, err, ended = None, b"", "", f"timed out after {PY_TIMEOUT} s"
            if os.path.exists(fault):
                ended = "raised an exception (a fault: fails whatever the exit status)"
                diffs.append("the program raised an exception")
            if code != run.get("exit", 0):
                diffs.append(f"exit: expected {run.get('exit', 0)}, got {code}")
            for p, want in run.get("outputs", {}).items():
                hp = os.path.join(host_w, norm_path(p))
                got = open(hp, "rb").read() if os.path.isfile(hp) else None
                if got is None:
                    diffs.append(f"output {p}: expected {json.dumps(want)}, the file does not exist")
                elif got != want.encode():
                    diffs.append(f"output {p}: expected {json.dumps(want)}, got "
                                 f"{json.dumps(got.decode('utf-8', 'replace'))[:400]}")
            if run.get("stdout") is not None:
                got = out[:cap].decode("utf-8", "replace")
                if got != run["stdout"] or len(out) > cap:
                    diffs.append(f"stdout: expected {json.dumps(run['stdout'])}, got "
                                 f"{json.dumps(got)[:400]}"
                                 + (f" ({len(out)} bytes, over the {cap}-byte bound)"
                                    if len(out) > cap else ""))
            tail = " | ".join([l for l in err.splitlines() if l.strip()][-5:])
            results.append({"example": ex["name"], "run": n, "passed": not diffs,
                            "ended": ended, "diffs": diffs, "stderr_tail": tail,
                            "scope_denied": bool(PY_DENIAL.search(err))})
            if diffs:
                break
        return results
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def grade_python(task, src, which=("visible", "hidden"), cap=STDOUT_CAP):
    """Grade a Python program (arm c by default) on the task's examples."""
    ceil = ceiling(task["intent_md"])
    try:
        compile(src, "main.py", "exec")
    except (SyntaxError, ValueError) as e:
        return {"passed": False, "stage": "check", "failed_examples": [],
                "scope_denied": False, "tail": f"{type(e).__name__}: {e}", "examples": []}
    res = [r for w in which for ex in task["examples"][w]
           for r in run_python_example(src, ex, ceil, cap)]
    bad = [r for r in res if not r["passed"]]
    return {"passed": not bad, "stage": "examples",
            "failed_examples": sorted(f"{r['example']}#{r['run']}" for r in bad),
            "scope_denied": any(r["scope_denied"] for r in res), "examples": res,
            "tail": "; ".join(f"`{r['example']}` run {r['run']}: {r['ended']}: {', '.join(r['diffs'])}"
                              for r in bad)[:600]}


def grade_python_reference(task, src):
    """A once task's reference.py on every visible and hidden example (spec GV-3),
    under the Axon reference's 1 MiB stdout bound."""
    return grade_python(task, src, ("visible", "hidden"), REFERENCE_STDOUT_CAP)
