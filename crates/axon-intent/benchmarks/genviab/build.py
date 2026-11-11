#!/usr/bin/env python3
"""Build and check the genviab task sets (IG6): `dev/` and `once/`, and an
authoring-eval (`ae`) set directory (spec AE-3).

  python3 -B build.py [--set dev|once | --set-dir DIR]          build: check every task, write <set>/manifest.json
  python3 -B build.py --check [--set dev|once | --set-dir DIR]  verify; exit 0 ok, 1 FAIL, 2 bad arguments
  python3 -B build.py --check --candidates --set-dir DIR         drafting candidates (no reference.ax yet)

Without --set/--set-dir both genviab sets are processed. The binaries are AXON_BIN (default
target/debug/axon) and AXON_INTENT_BIN (default: next to AXON_BIN).

`ae` set mode (--set-dir; refused inside the public repo): task.json carries
`family`, `tier` (T1/T2/T3/TS), a `canary` GUID and `drafted_by`
(haiku-5-5/human); exactly reference.ax and reference.py; `## Budget` tokens
>= 120000; at least one visible example; no tempting task. `refs-agree`
grades both references on every example (reference.py under the GV-3 runner,
reference.ax through the lift); `once` and `human-value` are skipped; path-lint
and v1-scope run on both references. Defects go to DIR/rejected.json, never
the public rejected-ideas.json. `--candidates` loads with require_ax=False and
runs every check that needs no reference.ax.

Mechanical checks, per task (gv.py has the task format):
  schema       the fields, a known category, exactly one reference (reference.py
               only in once/), example paths relative and under the ceiling's
               fs_read (inputs) / fs_write (outputs) prefixes, every exit in 0..=255
  intent-text  intent.md carries no Axon code: no fence, no `fn`/`let`/builtin call;
               `# Intent` is one paragraph (the loop shows only the first)
  hidden       at least one hidden example; hidden names differ from visible ones;
               no hidden example repeats a visible one; at least one hidden run
               stages inputs no visible run stages; with no fs_read (no run can
               stage an input) a hidden run past every visible example's run
               count checks a field (stdout, exit, an output path) to a value no
               visible run expects for that field
               (both vacuous with no visible example)
  reference    reference.ax passes the visible and hidden examples, lifted through
               `axon-intent compile` (axon check, the gates, the ceiling's sandbox);
               a reference.py passes them under gv.grade_python_reference (bwrap,
               the fault runner: a raised exception fails whatever the exit)
  empty-fails  an empty `main` fails the hidden examples
  human-value  `human_expected` (the source's human-computed answer) is what the
               hidden examples expect at that path, so the reference reproduces it
  path-lint    every path-like string literal in the reference (for Python: every
               `str` constant outside f-strings) is relative, has no `..`, and
               starts with a ceiling prefix spelled exactly
  v1-scope     the reference calls no `ai_*` and no `http_*` builtin (F10); a
               reference.py imports only PY_IMPORTS and calls no exec/eval/
               compile/__import__, no open() in a mode other than r/w/a text
  tempting     a tempting task names a forbidden axis the ceiling withholds and
               hidden examples that reach the tempting branch
  unique       no two tasks share an intent text (a shared reference is allowed)
  once         (once/ only) reference_author is `human:<name>`; at least one
               visible example; the ceiling grants no `exec` (the outcome record
               is forgeable under an exec grant)

A task failing a check is excluded from the manifest and logged under
`mechanical` in rejected-ideas.json (never for a model's failure: no model
runs here). `--check` skips the work when the corpus and both binaries hash as
the manifest records; a corpus that changed since the manifest FAILs (rebuild);
changed binaries re-run every check against the manifest.
"""
import argparse
import ast
import concurrent.futures as cf
import datetime
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gv  # noqa: E402

SETS = ("dev", "once")
SCHEMA = "genviab-manifest/1"
REJECTED = os.path.join(HERE, "rejected-ideas.json")
AXON_CODE = re.compile(r"```|\bfn\s+\w+\s*\(|\blet\s+\w+\s*=|->\s*i64|\b(read_file|write_file|"
                       r"append_file|println|str_split|parse_int)\s*\(")
STRING_LIT = re.compile(r'"((?:[^"\\]|\\.)*)"')
PATHLIKE = re.compile(r"^(\.{0,2}/|[\w.-]+/)[\w./-]*$")
V1_EXCLUDED = re.compile(r"\b(ai_\w+|http_\w+)\s*\(")
# reference.py's v1-scope (spec GV-3 step 4, NH-G2): a once task must be
# solvable by an Axon v1 program, so its Python reference uses only these.
PY_IMPORTS = ("sys", "os", "os.path", "json", "csv", "re", "math", "collections", "itertools",
              "functools", "string")
PY_CALLS_EXCLUDED = ("exec", "eval", "compile", "__import__")
PY_OPEN_MODES = ("r", "w", "a", "rt", "wt", "at")
REQUIRED = ("id", "category", "source", "reference_author", "examples")


def in_once(t):
    return os.path.basename(os.path.dirname(t["dir"])) == "once"


def manifest_path(set_):
    return os.path.join(root_of(set_), "manifest.json")


def binaries():
    out = {}
    for k, p in (("axon_sha256", gv.AXON), ("axon_intent_sha256", gv.AXON_INTENT)):
        if not os.path.isfile(p):
            raise SystemExit(f"build.py: {p} is missing (set AXON_BIN / AXON_INTENT_BIN)")
        out[k] = gv.sha256_file(p)
    return out


def card_sha256():
    r = subprocess.run([gv.AXON, "card", "--sha256"], capture_output=True, text=True, timeout=60)
    return r.stdout.strip() if r.returncode == 0 else None


# ------------------------------------------------------------------ checks

def check_schema(t):
    ae = t.get("set_mode") == "ae"
    bad = [k for k in (("id", "examples") if ae else REQUIRED) if k not in t]
    if bad:
        return f"task.json lacks {bad}"
    if t["id"] != os.path.basename(t["dir"]):
        return f"id {t['id']!r} is not the directory name"
    if t.get("category", "transform") not in gv.CATEGORIES:
        return f"unknown category {t['category']!r}"
    if not ae:
        # `ae` mode: gv.load_task already required reference.py (+ .ax unless a candidate)
        if len(t["references"]) > 1:
            return f"two references ({', '.join(t['references'])}): a task has exactly one"
        if t["reference"] is None:
            return "no reference.ax (or, in once/, reference.py)"
        if t["reference_lang"] == "python" and not in_once(t):
            return "reference.py is once-only (the dev oracle generator needs reference.ax)"
    extra = sorted(set(os.listdir(t["dir"])) - set(gv.TASK_FILES))
    if extra:
        return f"unexpected files {extra}"
    ceil = gv.ceiling(t["intent_md"])
    for axis in ("fs_read", "fs_write"):
        for p in ceil[axis]:
            if gv.norm_path(p) is None or not p.endswith("/"):
                return f"{axis} prefix {p!r} is not a relative directory prefix"
    for w in ("visible", "hidden"):
        for ex in t["examples"].get(w, []):
            if not ex.get("name") or not ex.get("runs"):
                return f"{w} example without a name or runs"
            for run in ex["runs"]:
                for p in run.get("inputs", {}):
                    if not gv.under(p, ceil["fs_read"]):
                        return f"{w} `{ex['name']}`: input {p} is not under fs_read"
                for p in run.get("outputs", {}):
                    if not gv.under(p, ceil["fs_write"]):
                        return f"{w} `{ex['name']}`: output {p} is not under fs_write"
                code = run.get("exit", 0)
                if not isinstance(code, int) or isinstance(code, bool):
                    return f"{w} `{ex['name']}`: exit is not an integer"
                if not 0 <= code <= 255:
                    return f"{w} `{ex['name']}`: exit {code} is outside 0..=255 (both engines truncate)"
    return None


def check_intent_text(t):
    m = AXON_CODE.search(t["intent_md"])
    if m:
        return f"intent.md contains Axon code ({m.group(0)!r})"
    if re.search(r"^##\s+examples\s*$", t["intent_md"], re.M | re.I):
        return "intent.md carries a `## Examples` section (examples live in task.json)"
    if len(gv.intent_paragraphs(t["intent_md"])) != 1:
        return ("`# Intent` must be ONE paragraph: the loop shows the generator only the "
                "first (Intent::goal), so later prose would be graded but never seen")
    return None


def run_key(run):
    return json.dumps(run, sort_keys=True)


def novel(run, vis_runs):
    """IGP-2: does `run` check a field to a value no visible run expects for that
    same field? Fields: stdout (when the run checks it), exit (always checked),
    each output path. A run checking nothing beyond what visible runs expect,
    e.g. `{}` (exit 0, nothing else), is not novel."""
    if run.get("stdout") is not None and run["stdout"] not in {
            r.get("stdout") for r in vis_runs if r.get("stdout") is not None}:
        return True
    if run.get("exit", 0) not in {r.get("exit", 0) for r in vis_runs}:
        return True
    return any(want not in {r.get("outputs", {}).get(p) for r in vis_runs if p in r.get("outputs", {})}
               for p, want in run.get("outputs", {}).items())


def check_hidden(t):
    vis, hid = t["examples"]["visible"], t["examples"]["hidden"]
    if not hid:
        return "no hidden example"
    names = [e["name"] for e in vis + hid]
    if len(set(names)) != len(names):
        return "example names repeat"
    vis_keys = {json.dumps(e["runs"], sort_keys=True) for e in vis}
    for e in hid:
        if json.dumps(e["runs"], sort_keys=True) in vis_keys:
            return f"hidden `{e['name']}` repeats a visible example"
    if not vis:
        return None
    if not gv.ceiling(t["intent_md"])["fs_read"]:
        # B3-2: no run can stage an input, so a hidden example is told apart by
        # its run count: some hidden run past every visible example's run count
        # must check a field (stdout, exit, an output path) to a value no visible
        # run expects for that field (IGP-2), else a hard-coded answer passes.
        m = max(len(e["runs"]) for e in vis)
        vis_runs = [r for e in vis for r in e["runs"]]
        if not any(novel(r, vis_runs) for e in hid for r in e["runs"][m:]):
            return ("the ceiling grants no fs_read, so no run stages inputs: a hidden example needs a run "
                    "beyond every visible example's run count that checks a field (stdout, exit, an output) "
                    "to a value no visible run expects for it")
        return None
    seen = {json.dumps(r.get("inputs", {}), sort_keys=True) for e in vis for r in e["runs"]}
    if all(json.dumps(r.get("inputs", {}), sort_keys=True) in seen
           for e in hid for r in e["runs"]):
        return "no hidden run stages inputs a visible run does not"
    return None


def check_human_value(t):
    h = t.get("human_expected")
    if h is None:
        return None
    got = [r["outputs"][h["path"]] for e in t["examples"]["hidden"] for r in e["runs"]
           if h["path"] in r.get("outputs", {})]
    if not got:
        return f"no hidden example expects {h['path']}"
    if any(g.strip() != h["value"] for g in got):
        return f"a hidden expectation at {h['path']} is not the human value {h['value']!r}"
    return None


def py_tree(t):
    """(ast of reference.py, None) or (None, why)."""
    try:
        return ast.parse(t["reference"], "reference.py"), None
    except (SyntaxError, ValueError) as e:
        return None, f"reference.py does not parse ({type(e).__name__}: {e})"


def reference_strings(t):
    """The reference's string literals: Axon's double-quoted literals; Python's
    `str` constants, f-string literal parts skipped (like Axon's `{` rule)."""
    if t["reference_lang"] != "python":
        return [m.group(1) for m in STRING_LIT.finditer(t["reference"])], None
    tree, why = py_tree(t)
    if tree is None:
        return [], why
    in_fstring = {id(v) for n in ast.walk(tree) if isinstance(n, ast.JoinedStr) for v in n.values}
    return [n.value for n in ast.walk(tree)
            if isinstance(n, ast.Constant) and isinstance(n.value, str) and id(n) not in in_fstring], None


def check_path_lint(t):
    ceil = gv.ceiling(t["intent_md"])
    prefixes = ceil["fs_read"] + ceil["fs_write"]
    strings, why = reference_strings(t)
    if why:
        return why
    for s in strings:
        if (t["reference_lang"] != "python" and "{" in s) or not PATHLIKE.match(s):
            continue
        if s.startswith("/") or ".." in s.split("/"):
            return f"reference literal {s!r} is absolute or climbs out"
        if not any(s.startswith(p) for p in prefixes):
            return f"reference literal {s!r} does not start with a ceiling prefix spelled exactly"
    return None


def check_v1_python(t):
    tree, why = py_tree(t)
    if tree is None:
        return why
    for n in ast.walk(tree):
        if isinstance(n, ast.Import):
            mods = [a.name for a in n.names]
        elif isinstance(n, ast.ImportFrom):
            mods = ["." * n.level + (n.module or "")]
        else:
            mods = []
        for m in mods:
            if m not in PY_IMPORTS:
                return f"reference.py imports {m} (allowed: {', '.join(PY_IMPORTS)})"
        if isinstance(n, ast.Call) and isinstance(n.func, ast.Name):
            if n.func.id in PY_CALLS_EXCLUDED:
                return f"reference.py calls {n.func.id}()"
            if n.func.id == "open":
                mode = n.args[1] if len(n.args) > 1 else next(
                    (k.value for k in n.keywords if k.arg == "mode"), None)
                if mode is not None and not (isinstance(mode, ast.Constant)
                                             and mode.value in PY_OPEN_MODES):
                    return (f"reference.py calls open() with mode {ast.unparse(mode)} "
                            "(only r/w/a text)")
    return None


def check_v1(t):
    if t["reference_lang"] == "python":
        return check_v1_python(t)
    m = V1_EXCLUDED.search(t["reference"])
    return f"reference calls {m.group(1)} (v1 excludes ai_*/http_*)" if m else None


def check_tempting(t):
    temp = t.get("tempting")
    if t["category"] != "tempting":
        return "a `tempting` block on a non-tempting task" if temp else None
    if not temp or temp.get("forbidden") not in gv.AXES or not temp.get("asks"):
        return "tempting task without forbidden axis/asks"
    ceil = gv.ceiling(t["intent_md"])
    if ceil[temp["forbidden"]] not in ([], "none"):
        return f"the ceiling grants the forbidden axis {temp['forbidden']}"
    hidden = {e["name"] for e in t["examples"]["hidden"]}
    branch = temp.get("branch_examples") or []
    if not branch or not set(branch) <= hidden:
        return "branch_examples must name hidden examples"
    return None


def check_once(t):
    """Once set only (spec IG6, Q7): written by hand, and the loop gets example
    feedback (the dev atlas tasks' zero-visible shape is never copied here)."""
    if not in_once(t):
        return None
    if not re.fullmatch(r"human:\S.*", str(t["reference_author"])):
        return "a once reference_author must be `human:<name>` (written by hand, never by a model)"
    if not t["examples"]["visible"]:
        return "a once task needs at least one visible example"
    if gv.ceiling(t["intent_md"])["exec"] != "none":
        return ("a once task's ceiling must grant `exec: none` (a spawned process could forge "
                "the interpreter's outcome record)")
    return None


AE_REQUIRED = ("id", "family", "tier", "canary", "drafted_by", "examples")
AE_TIERS = ("T1", "T2", "T3", "TS")
AE_DRAFTERS = ("haiku-5-5", "human")
AE_MIN_TOKENS = 120000
CANARY = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")


def py_view(t):
    """An `ae` task seen through its reference.py (the Python-side checks)."""
    return dict(t, reference=t["reference_py"], reference_lang="python")


def check_ae_schema(t):
    """`ae` set mode (spec AE-3): genviab's format plus family/tier/canary/
    drafted_by, both references, no tempting task, `tokens` >= 120000."""
    bad = [k for k in AE_REQUIRED if k not in t]
    if bad:
        return f"task.json lacks {bad}"
    if t["id"] != os.path.basename(t["dir"]):
        return f"id {t['id']!r} is not the directory name"
    if t["tier"] not in AE_TIERS:
        return f"tier {t['tier']!r} is not one of {AE_TIERS}"
    if not isinstance(t["family"], str) or not t["family"]:
        return "family must be a non-empty string"
    if not isinstance(t["canary"], str) or not CANARY.fullmatch(t["canary"]):
        return "canary must be a lowercase GUID"
    if t["drafted_by"] not in AE_DRAFTERS:
        return f"drafted_by {t['drafted_by']!r} is not one of {AE_DRAFTERS}"
    if t.get("category", "transform") not in gv.CATEGORIES or t.get("category") == "tempting":
        return f"category {t.get('category')!r}: the ae set has no tempting task"
    if t["canary"] in t["intent_md"]:
        return "the canary is in intent.md (it is never rendered into a prompt)"
    tokens = gv.budget_tokens(t["intent_md"])
    if tokens is None or tokens < AE_MIN_TOKENS:
        return f"`## Budget` tokens is {tokens}, below {AE_MIN_TOKENS} (spec §2: the round cap binds)"
    if not t["examples"].get("visible"):
        return "an ae task needs at least one visible example"
    return check_schema(t)


def check_refs_agree(t):
    """Both references pass every visible and hidden example. The expected
    outputs come from reference.py (D12), so reference.ax must reproduce them."""
    py = gv.grade_python_reference(t, t["reference_py"])
    if not py["passed"]:
        return f"reference.py fails ({py['stage']}, {py['failed_examples']}) {py['tail'][:300]}"
    if t["reference"] is None:
        return None   # a drafting candidate (require_ax=False): reference.ax not earned yet
    ax = gv.grade_axon(t, t["reference"])
    if not ax["passed"]:
        return (f"reference.ax disagrees with reference.py ({ax['stage']}, "
                f"{ax['failed_examples']}) {ax['tail'][-300:]}")
    return None


def check_ae_task(t):
    """(checks dict, first defect or None) in `ae` set mode."""
    checks = {}
    pv = py_view(t)
    steps = [("schema", check_ae_schema), ("intent-text", check_intent_text),
             ("hidden", check_hidden), ("path-lint-py", lambda _: check_path_lint(pv)),
             ("v1-scope-py", lambda _: check_v1_python(pv))]
    if t["reference"] is not None:
        steps += [("path-lint", check_path_lint), ("v1-scope", check_v1)]
    steps.append(("refs-agree", check_refs_agree))
    for name, fn in steps:
        why = fn(t)
        checks[name] = "ok" if why is None else why
        if why is not None:
            return checks, f"{name}: {why}"
    empty = gv.grade_axon(t, gv.EMPTY_PROGRAM, ("hidden",))
    checks["empty-fails"] = "ok" if not empty["passed"] else "an empty main passes the hidden examples"
    if empty["passed"]:
        return checks, "empty-fails: an empty main passes the hidden examples"
    return checks, None


def check_task(t):
    """(checks dict, first defect or None)."""
    if t.get("set_mode") == "ae":
        return check_ae_task(t)
    checks = {}
    for name, fn in (("schema", check_schema), ("once", check_once),
                     ("intent-text", check_intent_text),
                     ("hidden", check_hidden), ("human-value", check_human_value),
                     ("path-lint", check_path_lint), ("v1-scope", check_v1),
                     ("tempting", check_tempting)):
        why = fn(t)
        checks[name] = "ok" if why is None else why
        if why is not None:
            return checks, f"{name}: {why}"
    if t["reference_lang"] == "python":
        ref = gv.grade_python_reference(t, t["reference"])
        tail = ref["tail"][:300]
    else:
        ref = gv.grade_axon(t, t["reference"])
        tail = ref["tail"][-300:]
    checks["reference"] = "ok" if ref["passed"] else {k: v for k, v in ref.items() if k != "examples"}
    if not ref["passed"]:
        return checks, f"reference: fails ({ref['stage']}, {ref['failed_examples']}) {tail}"
    empty = gv.grade_axon(t, gv.EMPTY_PROGRAM, ("hidden",))
    checks["empty-fails"] = "ok" if not empty["passed"] else "an empty main passes the hidden examples"
    if empty["passed"]:
        return checks, "empty-fails: an empty main passes the hidden examples"
    return checks, None


def norm_text(s):
    return re.sub(r"\s+", " ", s).strip()


def root_of(set_):
    """A genviab set name (dev/once) or an `ae` set directory (--set-dir)."""
    return os.path.join(HERE, set_) if set_ in SETS else os.path.abspath(set_)


def name_of(set_):
    return set_ if set_ in SETS else os.path.basename(root_of(set_))


def hint(set_):
    return f"--set {set_}" if set_ in SETS else f"--set-dir {set_}"


def check_set(set_, candidates=False):
    """(passing tasks with digests, rejected [(id, defect)]). `candidates`: an
    `ae` drafting set, loaded with require_ax=False (spec E5)."""
    set_dir = root_of(set_)
    tasks = []
    for d in gv.task_dirs(set_dir):
        try:
            tasks.append(gv.load_task(d, require_ax=not candidates))
        except (OSError, ValueError) as e:
            tasks.append({"id": os.path.basename(d), "dir": d, "load_error": str(e)})
    ok, bad = [], []
    loaded = [t for t in tasks if "load_error" not in t]
    bad += [(t["id"], f"load: {t['load_error']}") for t in tasks if "load_error" in t]
    with cf.ThreadPoolExecutor(max_workers=6) as ex:
        results = list(ex.map(check_task, loaded))
    # A duplicate is a repeated task statement. Two tasks may share a
    # reference (the same program under a different ceiling is a different
    # task: i-summarize / i-overbroad).
    seen = {}
    for t, (checks, why) in zip(loaded, results):
        if why is None:
            key = norm_text(t["intent_md"])
            if key in seen:
                why = f"unique: repeats {seen[key]}"
                checks["unique"] = why
            else:
                seen[key] = t["id"]
                checks["unique"] = "ok"
        if why is not None:
            bad.append((t["id"], why))
            continue
        files = {n: gv.sha256_file(os.path.join(t["dir"], n)) for n in gv.task_files(t)}
        row = {"id": t["id"], "category": t.get("category", "transform"),
               "tempting": t.get("category") == "tempting",
               "visible_examples": len(t["examples"]["visible"]),
               "hidden_examples": len(t["examples"]["hidden"]),
               "files": files,
               "task_sha256": gv.sha256_bytes(json.dumps(files, sort_keys=True).encode()),
               "checks": checks}
        if t.get("set_mode") == "ae":
            row.update(family=t["family"], tier=t["tier"], drafted_by=t["drafted_by"])
        else:
            row["reference_author"] = t["reference_author"]
        ok.append(row)
    return ok, bad


def write_rejected(set_, bad):
    """genviab sets: the public rejected-ideas.json. An `ae` set keeps its
    defects in its own (private) directory: they quote task text (D8)."""
    if set_ not in SETS:
        with open(os.path.join(root_of(set_), "rejected.json"), "w") as fh:
            fh.write(json.dumps({"mechanical": [{"id": i, "defect": w} for i, w in bad]},
                                indent=1, ensure_ascii=False) + "\n")
        return
    doc = json.load(open(REJECTED)) if os.path.exists(REJECTED) else {}
    doc.setdefault("mechanical", {})[set_] = [{"id": i, "defect": w} for i, w in bad]
    with open(REJECTED, "w") as fh:
        fh.write(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")


def build(set_):
    bins = binaries()
    ok, bad = check_set(set_)
    counts = {c: sum(1 for t in ok if t["category"] == c) for c in gv.CATEGORIES}
    doc = {"schema": SCHEMA, "set": name_of(set_),
           "built": datetime.date.today().isoformat(),
           "corpus_sha256": gv.corpus_digest(root_of(set_)),
           **bins, "card_sha256": card_sha256(),
           "counts": counts, "n": len(ok), "excluded": [i for i, _ in bad], "tasks": ok}
    if set_ not in SETS:
        doc["mode"] = "ae"
        doc["tiers"] = {k: sum(1 for t in ok if t["tier"] == k) for k in AE_TIERS}
    with open(manifest_path(set_), "w") as fh:
        fh.write(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
    write_rejected(set_, bad)
    for i, w in bad:
        print(f"EXCLUDED {name_of(set_)}/{i}: {w}", file=sys.stderr)
    print(f"{name_of(set_)}: {len(ok)} task(s) {doc.get('tiers', counts)}, {len(bad)} excluded; "
          f"corpus {doc['corpus_sha256'][:16]}")
    return 0


def check_candidates(set_):
    """`--check --candidates`: every check a drafting candidate can pass before
    its reference.ax is earned (D12); no manifest. Exit 0 all pass, else 1."""
    ok, bad = check_set(set_, candidates=True)
    for t in ok:
        print(f"PASS {name_of(set_)}/{t['id']}")
    for i, w in bad:
        print(f"FAIL {name_of(set_)}/{i}: {w}")
    return 1 if bad else 0


def check(set_):
    name = name_of(set_)
    mp = manifest_path(set_)
    if not os.path.exists(mp):
        print(f"FAIL {name}: no manifest.json (run build.py {hint(set_)})")
        return 1
    man = json.load(open(mp))
    corpus = gv.corpus_digest(root_of(set_))
    if man.get("schema") != SCHEMA or man.get("set") != name:
        print(f"FAIL {name}: manifest schema/set mismatch")
        return 1
    if corpus != man["corpus_sha256"]:
        print(f"FAIL {name}: the corpus changed since the manifest ({corpus[:16]} != "
              f"{man['corpus_sha256'][:16]}); rebuild with build.py {hint(set_)}")
        return 1
    for t in man["tasks"]:
        for n, sha in t["files"].items():
            if gv.sha256_file(os.path.join(root_of(set_), t["id"], n)) != sha:
                print(f"FAIL {name}/{t['id']}: {n} differs from the manifest")
                return 1
    bins = binaries()
    if all(man.get(k) == v for k, v in bins.items()):
        print(f"{name}: corpus and binaries unchanged since the manifest; checks skipped "
              f"({man['n']} task(s))")
        return 0
    ok, bad = check_set(set_)
    ids = {t["id"] for t in ok}
    lost = [t["id"] for t in man["tasks"] if t["id"] not in ids]
    gained = sorted(ids - {t["id"] for t in man["tasks"]})
    for i, w in bad:
        if i in lost:
            print(f"FAIL {name}/{i}: {w}")
    if gained:
        print(f"FAIL {name}: {gained} pass now but the manifest excludes them; rebuild")
    if lost or gained:
        return 1
    print(f"{name}: binaries changed since the manifest; all {len(ok)} task(s) re-checked PASS "
          "(the manifest keeps the build's binary pins)")
    return 0


def inside_repo(path):
    root = os.path.realpath(gv.ROOT) + os.sep
    return (os.path.realpath(path) + os.sep).startswith(root)


def main():
    ap = argparse.ArgumentParser(usage=__doc__.split("\n\n")[1])
    ap.add_argument("--check", action="store_true")
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--set", choices=SETS)
    g.add_argument("--set-dir")
    ap.add_argument("--candidates", action="store_true")
    try:
        a = ap.parse_args()
    except SystemExit as e:
        return 0 if e.code == 0 else 2
    if a.set_dir:
        if not os.path.isdir(a.set_dir):
            print(f"build.py: --set-dir {a.set_dir} is not a directory", file=sys.stderr)
            return 2
        if os.path.basename(os.path.abspath(a.set_dir)) in SETS:
            print("build.py: --set-dir names a genviab set; use --set", file=sys.stderr)
            return 2
        if inside_repo(a.set_dir):
            # spec AE-3 contamination rule 1: task text never enters the public repo
            print(f"build.py: --set-dir {a.set_dir} is inside the public repo {gv.ROOT}; an ae "
                  "set lives in the private store", file=sys.stderr)
            return 2
    if a.candidates and not (a.set_dir and a.check):
        print("build.py: --candidates needs --check --set-dir", file=sys.stderr)
        return 2
    if a.candidates:
        return check_candidates(a.set_dir)
    sets = [a.set_dir] if a.set_dir else [a.set] if a.set else list(SETS)
    rc = 0
    for s in sets:
        rc = max(rc, check(s) if a.check else build(s))
    return rc


if __name__ == "__main__":
    sys.exit(main())
