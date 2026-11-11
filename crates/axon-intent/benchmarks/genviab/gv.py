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
import unicodedata

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


# The genviab sets (IG7). A task directory under any other set root is an
# authoring-eval (`ae`) task (spec AE-3): it holds BOTH references.
GENVIAB_SETS = ("dev", "once")


def set_mode(task_dir):
    """`genviab` for a task under dev/ or once/, else `ae`."""
    root = os.path.basename(os.path.dirname(os.path.abspath(task_dir)))
    return "genviab" if root in GENVIAB_SETS else "ae"


def load_task(d, require_ax=True):
    """A task directory as a dict. genviab sets: one reference, `reference` its
    text (None with zero or two). `ae` mode (spec AE-3): reference.py is
    required (`reference_py`), and `reference` is reference.ax's text, required
    unless `require_ax=False` (a drafting candidate, D12: None until earned);
    anything else raises ValueError."""
    with open(os.path.join(d, "task.json")) as fh:
        t = json.load(fh)
    with open(os.path.join(d, "intent.md")) as fh:
        t["intent_md"] = fh.read()
    ceiling(t["intent_md"])   # a malformed `## Allowed` raises ValueError: the task is refused (B3-4)
    present = [n for n in REFERENCE_FILES if os.path.exists(os.path.join(d, n))]
    t["references"] = present
    t["set_mode"] = set_mode(d)
    t["dir"] = d
    if t["set_mode"] == "ae":
        if "reference.py" not in present:
            raise ValueError("an ae task needs reference.py (the expected outputs come from it, D12)")
        if "reference.ax" not in present and require_ax:
            raise ValueError("an ae task needs reference.ax (only a drafting candidate, "
                             "require_ax=False, may lack it)")
        with open(os.path.join(d, "reference.py")) as fh:
            t["reference_py"] = fh.read()
        ax = "reference.ax" in present
        t["reference_file"] = "reference.ax" if ax else None
        t["reference_lang"] = "axon" if ax else None
        t["reference"] = open(os.path.join(d, "reference.ax")).read() if ax else None
        return t
    t["reference_file"] = present[0] if len(present) == 1 else None
    t["reference_lang"] = REFERENCE_FILES[present[0]] if len(present) == 1 else None
    t["reference"] = (open(os.path.join(d, present[0])).read() if len(present) == 1 else None)
    return t


def task_files(t):
    """The files a task's manifest row hashes: intent.md, task.json, its
    reference (both references in `ae` mode)."""
    if t.get("set_mode") == "ae":
        return BASE_FILES + tuple(t["references"])
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


# Rust's `char::is_whitespace` (Unicode White_Space), which `str::trim` and
# `trim_end` strip. Python's `str.strip()` also strips \x1c-\x1f, so gv strips
# only these (IGP-1); selftest checks the set against `str.isspace`.
RUST_WS = ("\t\n\x0b\x0c\r \x85\xa0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009"
           "\u200a\u2028\u2029\u202f\u205f\u3000")


def ceiling(intent_md):
    """The `## Allowed` section: axis -> list (net/fs) or str (exec), parsed as
    `axon-intent`'s `parse_allowed` (intent.rs) does: `- ` and `* ` bullets,
    `key: value` split on the first `:`, `none` (any case) or empty -> [],
    repeated keys extend, the last `exec` wins. Stricter than intent.rs, so it
    fails closed (B3-4): a line holding a control character (a trailing `\\r`
    aside, IGP-1), a non-blank line that is not a bullet, a bullet without `:`,
    an unknown key, a bad `exec`/`max_label` value or an fs prefix with a `..`
    component (split on `/` and `\\`, as `validate_prefixes`; IGP-4) raises
    ValueError."""
    out = {"fs_read": [], "fs_write": [], "net": [], "exec": "none"}
    for line in section(intent_md, "allowed").split("\n"):
        if any(unicodedata.category(c) == "Cc" for c in line):
            raise ValueError(f"## Allowed: a control character in {line!r}")
        t = line.strip(RUST_WS)
        if not t:
            continue
        if not t.startswith(("- ", "* ")):
            raise ValueError(f"## Allowed: not a bullet: {line!r}")
        b = t[2:].strip(RUST_WS)
        if ":" not in b:
            raise ValueError(f"## Allowed: expected `key: value`, got {b!r}")
        k, v = (s.strip(RUST_WS) for s in b.split(":", 1))
        if k in ("fs_read", "fs_write", "net"):
            if v and v.lower() != "none":
                items = [x.strip(RUST_WS) for x in v.split(",") if x.strip(RUST_WS)]
                for x in items:
                    if k != "net" and ".." in re.split(r"[/\\]", x):
                        raise ValueError(f"## Allowed: {k} path {x!r} contains a `..` component")
                out[k] += items
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
    ``` fence, a line (trailing whitespace stripped, Rust's set: `RUST_WS`) starting
    with `## ` opens a section titled by the rest, trimmed and lower-cased, and one
    starting with `# ` ends it; fence lines and fenced lines belong to no section."""
    want, in_fence, found = title.strip(RUST_WS).lower(), False, None
    for raw in md.split("\n"):   # str.lines(): only \n splits (splitlines() also splits on \r, \f, ...)
        line = raw.rstrip(RUST_WS)
        if line.lstrip(RUST_WS).startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        if line.startswith(("## ", "# ")):
            if found is not None:
                break
            if line.startswith("## ") and line[3:].strip(RUST_WS).lower() == want:
                found = []
            continue
        if found is not None:
            found.append(line)
    return "\n".join(found) if found is not None else ""


def budget_tokens(md):
    """The intent's `## Budget` `tokens` (a positive int), else None."""
    m = re.search(r"^\s*-\s*tokens\s*:\s*(\d+)\s*$", section(md, "budget"), re.M)
    return int(m.group(1)) if m and int(m.group(1)) > 0 else None


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


def bins_or_default(bins=None):
    """(axon, axon-intent): `bins` when given, else the env-selected globals
    read at call time. AE-4 passes `bins` so one process alternates binaries
    without mutating these globals under --parallel."""
    return tuple(bins) if bins else (AXON, AXON_INTENT)


def intent_env(bins=None):
    env = dict(os.environ, AXON_BIN=bins_or_default(bins)[0])
    env.pop("AXON_AI_MOCK", None)
    env.pop("AXON_APPROVAL_KEY", None)
    return env


def lift(intent_text, name="task", bins=None):
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
        axon_intent = bins_or_default(bins)[1]
        r = subprocess.run([axon_intent, "compile", p, "--out", os.path.join(ws, "o"),
                            "--transcript", rep], cwd=ws, env=intent_env(bins),
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


# Authoring-eval AE-2: one class per graded run, parsed from genloop's `ended`
# (`exit N` / `did not compile: ..` / `faulted (status ..): <Verdict::legible()>`
# / `refused by the sandbox: ..` / `timed out after N ms`). Only `panic` (a
# fault on axis `runtime`: fault_verdict's `_` arm, the interpreter's panics)
# counts in M2; every class is its own column (spec §2).
ENDED_CLASSES = ("ok", "wrong-output", "exit-mismatch", "panic", "resource", "refine", "halted",
                 "other-fault", "timeout", "sandbox-refused", "no-outcome-record",
                 "compile-failed")
FAULTED = re.compile(r"^faulted \(status .*?\): (.*)$", re.S)  # status `Some(101)` nests parens
AXIS = re.compile(r"\(axis: ([\w-]+)\)\s*$")
RESOURCE_AXES = ("steps", "wall", "budget")
# The panic reason's kind (spec AE-2), from the interpreter's panic strings
# (axon-core interp/builtins at this commit). `type-class`: the checker should
# have refused it (M2s). `partial-op`: a partial operation on a value the
# checker cannot see (index, division, overflow, assert, a missing key).
# Anything else is `other`, reported, never dropped.
PANIC_KINDS = (
    ("type-class", re.compile(
        r"^cannot apply \S+ to |^(\S+: )?expected (bool|i64|f64|str|Decimal|array|a \w+), got |"
        r"expected .+ found |no match arm|is not a runtime builtin|requires numeric index, got|"
        r"cannot index into scalar|key fn must return numeric|expected \d+ args, got")),
    ("partial-op", re.compile(
        r"index -?\d+ out of bounds|negative index|division by zero|remainder by zero|"
        r"integer overflow|^assertion failed|array is empty|key .+ not found|"
        r"is not a valid Unicode code point|is not a multiple of 4|^panic\b")),
)


def panic_reason(ended):
    """The interpreter's panic text inside a `runtime`-axis fault, without
    `axon: panic: ` and the `(axis: runtime)` suffix."""
    m = FAULTED.match(ended or "")
    s = AXIS.sub("", m.group(1) if m else (ended or "")).strip()
    s = re.sub(r"^\S*\s*DENIED: ", "", s)
    return re.sub(r"^axon: panic: ", "", s).strip()


def panic_kind(reason):
    for kind, rx in PANIC_KINDS:
        if rx.search(reason):
            return kind
    return "other"


def ended_class(run):
    """One graded run (a lift report `examples` entry) -> its §2 ended_class."""
    if run.get("passed"):
        return "ok"
    ended = run.get("ended") or ""
    if ended.startswith(NO_OUTCOME_RECORD):
        return "no-outcome-record"
    if ended.startswith(SANDBOX_REFUSAL):
        return "sandbox-refused"
    if ended.startswith("timed out"):
        return "timeout"
    if ended.startswith("did not compile"):
        return "compile-failed"
    if ended.startswith("exit "):
        diffs = run.get("diffs") or []
        return ("exit-mismatch" if diffs and all(d.startswith("exit: expected") for d in diffs)
                else "wrong-output")
    m = FAULTED.match(ended)
    if not m:
        return "other-fault"
    v = m.group(1)
    axis = AXIS.search(v)
    if axis:
        a = axis.group(1)
        if a == "runtime":
            return "panic"
        return "resource" if a in RESOURCE_AXES else "other-fault"
    if "HALTED" in v:
        return "halted"
    if v.startswith("refinement violated"):
        return "refine"
    if v.startswith("budget exhausted") or "RESOURCE BOUND" in v or "COALITION BOUND" in v:
        return "resource"
    return "other-fault"


def graded_runs(task, report):
    """Every graded run of a lift report: {example, run, which, ended_class,
    panic_kind, passed}. The loop runs every example and stops an example at
    its first failing run, so a failing example contributes that run."""
    which_of = {e["name"]: w for w in ("visible", "hidden") for e in task["examples"][w]}
    out = []
    for e in (report or {}).get("examples", []):
        cls = ended_class(e)
        out.append({"example": e.get("example"), "run": e.get("run"),
                    "which": which_of.get(e.get("example")), "ended_class": cls,
                    "panic_kind": panic_kind(panic_reason(e.get("ended"))) if cls == "panic"
                    else None,
                    "panic_reason": panic_reason(e.get("ended"))[:200] if cls == "panic" else None,
                    "passed": bool(e.get("passed"))})
    return out


def grade_axon(task, program, which=("visible", "hidden"), bins=None):
    """Grade an Axon program on the task's examples through the lift path."""
    rc, rep, tail = lift(grading_intent(task, program, which), task["id"], bins)
    return {
        "rc": rc,
        "passed": rc == 0 and (rep or {}).get("stage") == "accepted",
        "stage": (rep or {}).get("stage"),
        "check_ok": (rep or {}).get("check_ok"),
        "diag_codes": diag_codes((rep or {}).get("diagnostics")),
        "failed_examples": failed_examples(rep),
        "sandbox_refused": sandbox_refused(rep),
        "no_outcome_record_runs": no_outcome_record_runs(rep),
        "gate_refusal": (rep or {}).get("gate_refusal"),
        "runs": graded_runs(task, rep),
        "program_sha256": sha256_bytes(program.encode()),
        "tail": "" if rc == 0 else tail[-600:],
    }


def diag_codes(diagnostics):
    """`axon-diag/1` error codes of a report's diagnostics, in order."""
    out = []
    for d in diagnostics or []:
        try:
            j = json.loads(d) if isinstance(d, str) else d
        except ValueError:
            continue
        if isinstance(j, dict) and j.get("severity") == "error" and j.get("code"):
            out.append(j["code"])
    return out


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
# program's own exit. IGP-3: every name the handler uses (the marker writer,
# the exception classes, print_exc) is bound before the program runs, so a
# program that rebinds `builtins.open`, `sys.stderr`, `traceback` or a builtin
# exception class and then raises is still a fault. (Not fault-by-default: a
# marker made before the run would turn `os._exit(n)` into a fault.)
PY_RUNNER = '''import os, runpy, sys, traceback
_open, _close, _flags = os.open, os.close, os.O_WRONLY | os.O_CREAT
_print_exc, _SystemExit, _BaseException = traceback.print_exc, SystemExit, BaseException
sys.argv = ["/w/main.py"]
sys.path.insert(0, "/w")
try:
    runpy.run_path("/w/main.py", run_name="__main__")
except _SystemExit:
    raise
except _BaseException:
    try:
        _close(_open("/gv/fault/raised", _flags, 0o644))
    finally:
        try:
            _print_exc()
        except _BaseException:
            pass
    raise _SystemExit(1)
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
