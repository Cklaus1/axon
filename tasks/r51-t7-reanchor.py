#!/usr/bin/env python3
"""R51 T7 re-anchor map (Fork 4; EG-2/EG-4/EG-12). Read-only on the tree; writes
tasks/r51-t7-reanchor.json and tasks/r51-t7-reanchor-map.md under ROOT.
Usage: python3 tasks/r51-t7-reanchor.py [ROOT]   (ROOT defaults to this repo)"""
import collections, glob, json, os, re, subprocess, sys
ROOT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), ".."))
OUT_JSON = os.path.join(ROOT, "tasks/r51-t7-reanchor.json")
OUT_MD = os.path.join(ROOT, "tasks/r51-t7-reanchor-map.md")
CORE = "crates/axon-core/src/"
os.environ["V022_KEEP_TMPDIR"] = "1"  # the module would otherwise mkdtemp under /var/tmp (:53)
sys.path.insert(0, os.path.join(ROOT, "scripts"))
import v022_g01_mutations as g  # noqa: E402  main() is __main__-guarded
head = subprocess.run(["git", "-C", ROOT, "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
meta = json.loads(subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"],
                                 cwd=ROOT, capture_output=True, text=True, check=True).stdout)
PKG = {}
for p in meta["packages"]:
    lib, tests, bins = None, {}, {}
    for t in p["targets"]:
        k = t["kind"]
        if "lib" in k or "rlib" in k or "proc-macro" in k:
            lib = os.path.dirname(t["src_path"])
        elif "test" in k:
            tests[t["name"]] = t["src_path"]
        elif "bin" in k:
            bins[t["name"]] = t["src_path"]
    PKG[p["name"]] = (lib, tests, bins, os.path.dirname(p["manifest_path"]))
reg = json.load(open(os.path.join(ROOT, "governance/status/r51-deferred-tests.json")))
DEFERRED = {(t["file"], t["name"]): t["slice"] for t in reg["tests"]}
_cache = {}
def text(path):
    if path not in _cache:
        try:
            with open(path, encoding="utf-8") as f:
                _cache[path] = f.read()
        except FileNotFoundError:
            _cache[path] = None
    return _cache[path]
def rs(d):
    return sorted(glob.glob(os.path.join(d, "**", "*.rs"), recursive=True))
MOD = re.compile(r"^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;", re.M)
def candidate_files(pkg, target, test):
    lib, tests, bins, crate = PKG[pkg]
    toks = target.split()
    if "--test" in toks:
        src = tests.get(toks[toks.index("--test") + 1])
        if src is None:
            return []
        d = os.path.dirname(src)
        out = [src] + rs(os.path.splitext(src)[0])
        for m in MOD.findall(text(src) or ""):
            f = os.path.join(d, m + ".rs")
            out += ([f] if os.path.isfile(f) else []) + rs(os.path.join(d, m))
        return out
    if "--bin" in toks:
        src = bins.get(toks[toks.index("--bin") + 1])
        return [src] if src else []
    # --lib, or a non-flag token cargo reads as a TESTNAME filter (`_CL` rebinding, :6128)
    if lib is None:
        return rs(crate)
    segs = test.split("::")
    if len(segs) == 1:
        return rs(lib)
    top = os.path.join(lib, segs[0])
    return [f for f in rs(lib) if f == top + ".rs" or f.startswith(top + os.sep)]
def find_test(pkg, target, test):
    if pkg not in PKG:
        return None
    pat = re.compile(r"\bfn\s+" + re.escape(test.split("::")[-1]) + r"\s*(?:<[^>{]*>)?\s*\(")
    for f in candidate_files(pkg, target, test):
        t = text(f)
        if t and pat.search(t):
            return os.path.relpath(f, ROOT)
    return None
def core_slice(mid, desc, rel, old):
    f, both = rel[len(CORE):], desc + "\n" + old
    if not f.startswith("interp"):
        return "S0b" if mid == "M1938" else "S0"
    if "PICK" in desc:
        return "S4"
    if f == "interp/pin.rs":
        return "S5"
    if f == "interp/taint.rs":
        return "S2"
    if f == "interp/conform.rs":
        return "S3"
    if re.search(r"\breceiver\b|dispatch rule|selects_between_impls", both):
        return "S4"
    if re.search(r"\b(TAINT|CONTROL|CLOCK)\b", desc) or re.search(r"tl::<|t_branch|eval_t::<|\btaint\b|taint::", old):
        return "S2"
    if re.search(r"\bPIN\b", desc) or re.search(r"self\.pins\b|pin_fn", old):
        return "S5"
    if "seal" not in both.lower() and re.search(
            r"^interpreter:|loop control|TestEnd|test_body_finished|test_frame_depth|contain_(?:frame|loop_control)|source_tag|provenance|complet", both, re.M):
        return "S0b"
    if re.search(r"seal|SEAL|global|cast|mark|snapshot|chan|closure|kernel|\bk\(\)|RNG|rng|no_such_fn|handler|sink|general_calls|PSV-1", both):
        return "S3"
    return "unknown"
ids = [m[0] for m in g.MUTATIONS]
assert len(ids) == len(set(ids)), "duplicate row ids"
active = [m for m in g.MUTATIONS if g.in_scope(m[0], "all")]
rows = []
for (mid, desc, rel, old, new, pkg, target, test) in active:
    t = text(os.path.join(ROOT, rel))
    n = None if t is None else t.count(old)
    cls = "a" if n == 1 else ("c" if n and n > 1 else "b")
    core = rel.startswith(CORE)
    tf = find_test(pkg, target, test)
    dsl = DEFERRED.get((tf, test.split("::")[-1])) if tf else None
    if core:
        sl, disp = core_slice(mid, desc, rel, old), "rerun (Fork 4: core)"
    elif cls != "a":
        sl, disp = "unknown", f"reanchor+rerun ({cls})"
    elif tf is None:
        sl, disp = "S0", "rerun: killing test absent"
    elif tf.startswith(CORE + "interp"):
        sl, disp = "S0", "rerun: killing test in interp*"
    elif dsl:
        sl, disp = dsl, f"rerun at/after {dsl}: killing test #[ignore]"
    else:
        sl, disp = "S0", "carry"
    rows.append({"id": mid, "file": rel, "class": cls, "slice": sl, "test": test, "test_present": tf is not None,
                 "matches": n, "file_absent": t is None, "core": core, "pkg": pkg, "target": target,
                 "test_file": tf, "test_deferred": dsl, "disposition": disp})
os.makedirs(os.path.dirname(OUT_JSON), exist_ok=True)
with open(OUT_JSON, "w") as f:
    json.dump(rows, f, indent=1)
C = collections.Counter
ret = sorted(i for i in ids if i in g.RETIRED)
sib = sorted(i for i in ids if i in g.SIBLING_ONLY and i not in g.RETIRED)
core = [r for r in rows if r["core"]]
out = [r for r in rows if not r["core"]]
L = [f"# R51 T7 re-anchor map @ {head}", "",
     f"ids {len(ids)}; retired {len(ret)}; sibling-only {len(sib)}; active {len(rows)} (EG-2: 2439/149/9/2281)",
     f"core {len(core)} (interp* {sum(r['file'].startswith(CORE + 'interp') for r in core)}; EG-2: 426/360)", "",
     "## class x scope", "| scope | a | b | c |", "|---|---|---|---|"]
for name, rs_ in (("core", core), ("outside", out)):
    c = C(r["class"] for r in rs_)
    L.append(f"| {name} | {c['a']} | {c['b']} | {c['c']} |")
L += ["", "## core: file x class", "| file | a | b | c | absent |", "|---|---|---|---|---|"]
for fname in sorted({r["file"] for r in core}):
    rr = [r for r in core if r["file"] == fname]
    c = C(r["class"] for r in rr)
    L.append(f"| {fname} | {c['a']} | {c['b']} | {c['c']} | {sum(r['file_absent'] for r in rr)} |")
L += ["", "## core: slice that re-creates the anchored code", "| slice | a | b | c | ids |", "|---|---|---|---|---|"]
for s in sorted({r["slice"] for r in core}):
    rr = [r for r in core if r["slice"] == s]
    c = C(r["class"] for r in rr)
    L.append(f"| {s} | {c['a']} | {c['b']} | {c['c']} | {' '.join(r['id'] for r in rr)} |")
L += ["", "## outside core: disposition", "| disposition | n |", "|---|---|"]
for d, n in sorted(C(r["disposition"] for r in out).items()):
    L.append(f"| {d} | {n} |")
for d in sorted({r["disposition"] for r in out if r["disposition"] != "carry"}):
    L += ["", f"### {d}", " ".join(r["id"] for r in out if r["disposition"] == d)]
L += ["", "## killing tests absent (all scopes)", " ".join(r["id"] for r in rows if not r["test_present"])]
with open(OUT_MD, "w") as f:
    f.write("\n".join(L) + "\n")
print(f"wrote {OUT_JSON} ({len(rows)} rows) and {OUT_MD}")
