#!/usr/bin/env python3
"""Build and check the genviab task sets (IG6): `dev/` and `once/`.

    python3 -B build.py [--set dev|once]           build: check every task, write <set>/manifest.json
    python3 -B build.py --check [--set dev|once]   verify; exit 0 ok, 1 FAIL, 2 bad arguments

Without --set both sets are processed. The binaries are AXON_BIN (default
target/debug/axon) and AXON_INTENT_BIN (default: next to AXON_BIN).

Mechanical checks, per task (gv.py has the task format):
  schema       the fields, a known category, example paths relative and under
               the ceiling's fs_read (inputs) / fs_write (outputs) prefixes
  intent-text  intent.md carries no Axon code: no fence, no `fn`/`let`/builtin call;
               `# Intent` is one paragraph (the loop shows only the first)
  hidden       at least one hidden example; hidden names differ from visible ones;
               no hidden example repeats a visible one; at least one hidden run
               stages inputs no visible run stages (vacuous with no visible example)
  reference    reference.ax passes the visible and hidden examples, lifted through
               `axon-intent compile` (axon check, the gates, the ceiling's sandbox)
  empty-fails  an empty `main` fails the hidden examples
  human-value  `human_expected` (the source's human-computed answer) is what the
               hidden examples expect at that path, so the reference reproduces it
  path-lint    every path-like string literal in the reference is relative, has
               no `..`, and starts with a ceiling prefix spelled exactly
  v1-scope     the reference calls no `ai_*` and no `http_*` builtin (F10)
  tempting     a tempting task names a forbidden axis the ceiling withholds and
               hidden examples that reach the tempting branch
  unique       no two tasks share an intent text (a shared reference is allowed)

A task failing a check is excluded from the manifest and logged under
`mechanical` in rejected-ideas.json (never for a model's failure: no model
runs here). `--check` skips the work when the corpus and both binaries hash as
the manifest records; a corpus that changed since the manifest FAILs (rebuild);
changed binaries re-run every check against the manifest.
"""
import argparse
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
REQUIRED = ("id", "category", "source", "reference_author", "examples")


def manifest_path(set_):
    return os.path.join(HERE, set_, "manifest.json")


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
    bad = [k for k in REQUIRED if k not in t]
    if bad:
        return f"task.json lacks {bad}"
    if t["id"] != os.path.basename(t["dir"]):
        return f"id {t['id']!r} is not the directory name"
    if t["category"] not in gv.CATEGORIES:
        return f"unknown category {t['category']!r}"
    if t["reference"] is None:
        return "no reference.ax"
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
                if not isinstance(run.get("exit", 0), int):
                    return f"{w} `{ex['name']}`: exit is not an integer"
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
    if vis:
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


def check_path_lint(t):
    ceil = gv.ceiling(t["intent_md"])
    prefixes = ceil["fs_read"] + ceil["fs_write"]
    for m in STRING_LIT.finditer(t["reference"]):
        s = m.group(1)
        if "{" in s or not PATHLIKE.match(s):
            continue
        if s.startswith("/") or ".." in s.split("/"):
            return f"reference literal {s!r} is absolute or climbs out"
        if not any(s.startswith(p) for p in prefixes):
            return f"reference literal {s!r} does not start with a ceiling prefix spelled exactly"
    return None


def check_v1(t):
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


def check_task(t):
    """(checks dict, first defect or None)."""
    checks = {}
    for name, fn in (("schema", check_schema), ("intent-text", check_intent_text),
                     ("hidden", check_hidden), ("human-value", check_human_value),
                     ("path-lint", check_path_lint), ("v1-scope", check_v1),
                     ("tempting", check_tempting)):
        why = fn(t)
        checks[name] = "ok" if why is None else why
        if why is not None:
            return checks, f"{name}: {why}"
    ref = gv.grade_axon(t, t["reference"])
    checks["reference"] = "ok" if ref["passed"] else ref
    if not ref["passed"]:
        return checks, f"reference: fails ({ref['stage']}, {ref['failed_examples']}) {ref['tail'][-300:]}"
    empty = gv.grade_axon(t, gv.EMPTY_PROGRAM, ("hidden",))
    checks["empty-fails"] = "ok" if not empty["passed"] else "an empty main passes the hidden examples"
    if empty["passed"]:
        return checks, "empty-fails: an empty main passes the hidden examples"
    return checks, None


def norm_text(s):
    return re.sub(r"\s+", " ", s).strip()


def check_set(set_):
    """(passing tasks with digests, rejected [(id, defect)])."""
    set_dir = os.path.join(HERE, set_)
    tasks = []
    for d in gv.task_dirs(set_dir):
        try:
            tasks.append(gv.load_task(d))
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
        files = {n: gv.sha256_file(os.path.join(t["dir"], n)) for n in gv.TASK_FILES}
        ok.append({"id": t["id"], "category": t["category"],
                   "tempting": t["category"] == "tempting",
                   "reference_author": t["reference_author"],
                   "visible_examples": len(t["examples"]["visible"]),
                   "hidden_examples": len(t["examples"]["hidden"]),
                   "files": files,
                   "task_sha256": gv.sha256_bytes(json.dumps(files, sort_keys=True).encode()),
                   "checks": checks})
    return ok, bad


def write_rejected(set_, bad):
    doc = json.load(open(REJECTED)) if os.path.exists(REJECTED) else {}
    doc.setdefault("mechanical", {})[set_] = [{"id": i, "defect": w} for i, w in bad]
    with open(REJECTED, "w") as fh:
        fh.write(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")


def build(set_):
    bins = binaries()
    ok, bad = check_set(set_)
    counts = {c: sum(1 for t in ok if t["category"] == c) for c in gv.CATEGORIES}
    doc = {"schema": SCHEMA, "set": set_,
           "built": datetime.date.today().isoformat(),
           "corpus_sha256": gv.corpus_digest(os.path.join(HERE, set_)),
           **bins, "card_sha256": card_sha256(),
           "counts": counts, "n": len(ok), "excluded": [i for i, _ in bad], "tasks": ok}
    with open(manifest_path(set_), "w") as fh:
        fh.write(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
    write_rejected(set_, bad)
    for i, w in bad:
        print(f"EXCLUDED {set_}/{i}: {w}", file=sys.stderr)
    print(f"{set_}: {len(ok)} task(s) {counts}, {len(bad)} excluded; corpus "
          f"{doc['corpus_sha256'][:16]}")
    return 0


def check(set_):
    mp = manifest_path(set_)
    if not os.path.exists(mp):
        print(f"FAIL {set_}: no manifest.json (run build.py --set {set_})")
        return 1
    man = json.load(open(mp))
    corpus = gv.corpus_digest(os.path.join(HERE, set_))
    if man.get("schema") != SCHEMA or man.get("set") != set_:
        print(f"FAIL {set_}: manifest schema/set mismatch")
        return 1
    if corpus != man["corpus_sha256"]:
        print(f"FAIL {set_}: the corpus changed since the manifest ({corpus[:16]} != "
              f"{man['corpus_sha256'][:16]}); rebuild with build.py --set {set_}")
        return 1
    for t in man["tasks"]:
        for n, sha in t["files"].items():
            if gv.sha256_file(os.path.join(HERE, set_, t["id"], n)) != sha:
                print(f"FAIL {set_}/{t['id']}: {n} differs from the manifest")
                return 1
    bins = binaries()
    if all(man.get(k) == v for k, v in bins.items()):
        print(f"{set_}: corpus and binaries unchanged since the manifest; checks skipped "
              f"({man['n']} task(s))")
        return 0
    ok, bad = check_set(set_)
    ids = {t["id"] for t in ok}
    lost = [t["id"] for t in man["tasks"] if t["id"] not in ids]
    gained = sorted(ids - {t["id"] for t in man["tasks"]})
    for i, w in bad:
        if i in lost:
            print(f"FAIL {set_}/{i}: {w}")
    if gained:
        print(f"FAIL {set_}: {gained} pass now but the manifest excludes them; rebuild")
    if lost or gained:
        return 1
    print(f"{set_}: binaries changed since the manifest; all {len(ok)} task(s) re-checked PASS "
          "(the manifest keeps the build's binary pins)")
    return 0


def main():
    ap = argparse.ArgumentParser(usage=__doc__.split("\n\n")[1])
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--set", choices=SETS)
    try:
        a = ap.parse_args()
    except SystemExit as e:
        return 0 if e.code == 0 else 2
    sets = [a.set] if a.set else list(SETS)
    rc = 0
    for s in sets:
        rc = max(rc, check(s) if a.check else build(s))
    return rc


if __name__ == "__main__":
    sys.exit(main())
