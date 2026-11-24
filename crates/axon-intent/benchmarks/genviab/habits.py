#!/usr/bin/env python3
"""Habit classes of check-refused round-0 Axon programs (authoring-gap spec AG-2, CF-11, DL-6).

    python3 -B habits.py RUN_DIR [--arm cand] [--bin AXON] [--json]
    python3 -B habits.py --selftest            (AXON_BIN: also the live part)

It reads RUN_DIR/<arm>/s*/transcripts/*.a.json, keeps the scored attempts (rows.jsonl `infra`
false, when the file is there) whose round-0 report stage is `check`, re-checks each round-0
`source` with `axon check --json` (stderr, one JSON line per diagnostic) and prints, per habit
class, n and the n whose first error carries a help line: any help, and this class's own help
(the arm's text). A class is the observed parser message plus a coarse source shape, as in the
spec's AG-2 table; an arm keyed on a narrower shape shows its excluded count as n - own.

No model call. It holds no task text; its selftest runs on synthetic programs only.
"""
import argparse
import ast
import glob
import json
import os
import re
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
DEFAULT_BIN = os.environ.get("AXON_BIN", os.path.join(ROOT, "target", "debug", "axon"))

FN_VALUE = ("= fn(", "(fn(", ", fn(", "return fn(")
SUFFIX = re.compile(r'^unexpected token: Ident\("(?:[iu](?:8|16|32|64|128|size)|f32|f64)"\), '
                    r"expected (?:RParen|RBracket)")


def line_at(src, offset):
    start = src.rfind("\n", 0, offset) + 1
    end = src.find("\n", offset)
    return src[start:len(src) if end < 0 else end].strip()


def token_line(src, offset):
    """The line holding the last non-blank character before `offset` (parse_help::token_line)."""
    before = src[:offset].rstrip()
    return line_at(src, len(before) - 1) if before else line_at(src, offset)


def _is(msg, src, off):
    return msg.startswith('unexpected token: Ident("is"), expected')


def _record(msg, src, off):
    line = line_at(src, off)
    return (msg == "unexpected token: Colon, expected expression"
            and '{"' not in line and ":=" not in line)


def _fn_value(msg, src, off):
    return (msg == "unexpected token: Fn, expected expression"
            and any(k in token_line(src, off) for k in FN_VALUE))


def _fn_nested(msg, src, off):
    return (msg == "unexpected token: Fn, expected expression" and not _fn_value(msg, src, off)
            and re.match(r"fn\s+[A-Za-z_]", src[off:]) is not None)


def _type_brace(msg, src, off):
    words = token_line(src, off).split()
    return msg == "unexpected token: LBrace, expected Eq" and words[:1] == ["type"]


def _star(msg):
    return msg == "unexpected token: Star, expected expression"


def _code(src, off):
    """The offset's line without a trailing `//` comment (the `**` arm reads only code)."""
    return line_at(src, off).split("//")[0]


def _type_in_fn(msg, src, off):
    """`type <Name> =` / `{` / `<` at the offset: a declaration, not a field named `type`."""
    return (msg == "unexpected token: Type, expected expression"
            and re.match(r"type\s+\w+\s*[={<]", src[off:]) is not None)


# (class, the spec's habit, predicate(message, source, byte offset), the arm's help text marker)
CLASSES = (
    ("is", "`x is Err` / `is Some(..)`", _is, "`is` is not an Axon operator"),
    ("record", "`{ k: v }` unnamed record", _record, "an unnamed record needs a named type"),
    ("repeat", "`[x; n]`", lambda m, s, o: m == "unexpected token: Semi, expected RBracket",
     "Axon has no `[x; n]`"),
    ("suffix", "`0i64`", lambda m, s, o: SUFFIX.match(m) is not None, "literals take no type suffix"),
    ("fn_value", "`fn(..)` as a value", _fn_value, "a lambda is written"),
    ("fn_nested", "nested `fn` item", _fn_nested, "fn items live at the top level"),
    ("brace", '`"{"` in a string', lambda m, s, o: m == "unclosed `{` in interpolated string",
     "a literal brace in a string is"),
    ("type_in_fn", "`type` in a fn body", _type_in_fn,
     "`type` declarations live at the top level"),
    ("type_brace", "`type X { .. }`", _type_brace, "a record type is"),
    ("pow", "`a ** 2`", lambda m, s, o: _star(m) and "**" in _code(s, o), "Axon has no `**`"),
    ("deref", "`*x` deref", lambda m, s, o: _star(m) and "**" not in _code(s, o),
     "Axon has no `*` dereference"),
)


def offset_of(src, line, col):
    """Byte offset of a 1-based (line, byte col), as span::SourceMap counts them."""
    b = src.encode()
    starts = [0] + [i + 1 for i, c in enumerate(b) if c == 0x0A]
    if not 1 <= line <= len(starts):
        return len(src)
    return len(b[:starts[line - 1] + col - 1].decode(errors="ignore"))


def classify(src, diag):
    """The habit classes of a first error diagnostic (a dict from `axon check --json`)."""
    if not diag or diag.get("code") != "E0000":
        return []
    msg = diag.get("message", "")
    off = offset_of(src, int(diag.get("line") or 0), int(diag.get("col") or 1))
    return [name for name, _, pred, _ in CLASSES if pred(msg, src, off)]


def own_help(name, diag):
    marker = {n: m for n, _, _, m in CLASSES}[name]
    return marker in (diag.get("help") or "")


def parse_report(rep):
    if isinstance(rep, str):
        try:
            return ast.literal_eval(rep)
        except (ValueError, SyntaxError):
            return json.loads(rep)
    return rep


def round0_refused(run_dir, arm="cand"):
    """[(attempt key, source)] for scored attempts whose round-0 report stage is `check`."""
    infra = set()
    rows = os.path.join(run_dir, arm, "rows.jsonl")
    if os.path.isfile(rows):
        with open(rows) as fh:
            for line in fh:
                r = json.loads(line)
                if r.get("infra"):
                    infra.add((r["id"], int(r["seed"])))
    out = []
    for path in sorted(glob.glob(os.path.join(run_dir, arm, "s*", "transcripts", "*.a.json"))):
        seed = int(os.path.basename(os.path.dirname(os.path.dirname(path)))[1:])
        tid = os.path.basename(path)[:-len(".a.json")]
        if (tid, seed) in infra:
            continue
        with open(path) as fh:
            t = json.load(fh)
        rounds = t.get("rounds") or []
        rep = parse_report(rounds[0].get("report")) if rounds else None
        if isinstance(rep, dict) and rep.get("stage") == "check" and isinstance(rep.get("source"), str):
            out.append((f"{tid}:s{seed}", rep["source"]))
    return out


def first_error(src, axon):
    with tempfile.TemporaryDirectory(prefix="habits-") as d:
        p = os.path.join(d, "main.ax")
        with open(p, "w") as fh:
            fh.write(src)
        r = subprocess.run([axon, "check", "--json", p], capture_output=True, text=True, timeout=120)
    for line in r.stderr.splitlines():
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if isinstance(d, dict) and d.get("severity") == "error":
            return d
    return None


def tally(sources, axon, workers=8):
    """{class: {n, any, own}} plus the count of sources and of those that now check clean."""
    with ThreadPoolExecutor(workers) as ex:
        diags = list(ex.map(lambda s: first_error(s, axon), sources))
    res = {name: {"n": 0, "any": 0, "own": 0} for name, _, _, _ in CLASSES}
    for src, d in zip(sources, diags):
        for name in classify(src, d):
            res[name]["n"] += 1
            res[name]["any"] += bool(d.get("help"))
            res[name]["own"] += own_help(name, d)
    return res, len(sources), sum(d is None for d in diags)


def render(res, total, clean):
    lines = [f"check-refused round-0 attempts: {total} (now check clean: {clean})", "",
             "| class | habit | n | any help | own help | own % | excluded |", "|---|---|---|---|---|---|---|"]
    for name, habit, _, _ in CLASSES:
        r = res[name]
        pct = f"{100 * r['own'] / r['n']:.0f}%" if r["n"] else "-"
        lines.append(f"| {name} | {habit} | {r['n']} | {r['any']} | {r['own']} | {pct} | {r['n'] - r['own']} |")
    return "\n".join(lines)


def below(res, floor=0.9):
    """Classes with instances whose own-help share is under `floor` (AG-2 acceptance)."""
    return [n for n, r in res.items() if r["n"] and r["own"] < floor * r["n"]]


# ---- selftest: synthetic programs only ---------------------------------------------------------

SYN = (
    ("is", 'fn main() -> i64 {\n    let r = parse_int("1")\n    if r is Err(_) { return 1 }\n    0\n}\n'),
    ("is", 'fn main() -> i64 {\n    let o = Some(1)\n    while o is None { o = None }\n    0\n}\n'),
    ("record", "fn main() -> i64 {\n    let n = 1\n    let it = { name: n }\n    0\n}\n"),
    ("record", "type P = { x: i64 }\nfn f() -> P {\n    {\n        x: 1\n    }\n}\nfn main() -> i64 { 0 }\n"),
    ("repeat", "fn main() -> i64 {\n    let n = 3\n    let xs = [0; n]\n    0\n}\n"),
    ("suffix", "fn main() -> i64 {\n    println(to_str(5i64))\n    0\n}\n"),
    ("suffix", "fn main() -> i64 {\n    let xs = [0i64, 1]\n    0\n}\n"),
    ("fn_value", "fn main() -> i64 {\n    let f = fn(a: i64) -> i64 { a + 1 }\n    f(1)\n}\n"),
    ("fn_value", "fn main() -> i64 {\n    let ys = arr_map([1], fn(a: i64) -> i64 { a })\n    0\n}\n"),
    ("fn_value", "fn mk() -> i64 {\n    return fn(a: i64) -> i64 { a }\n}\nfn main() -> i64 { 0 }\n"),
    ("fn_nested", "fn main() -> i64 {\n    fn helper(a: i64) -> i64 { a }\n    helper(1)\n}\n"),
    ("brace", 'fn main() -> i64 {\n    println("{")\n    0\n}\n'),
    ("type_in_fn", "fn main() -> i64 {\n    type T = { a: i64 }\n    0\n}\n"),
    ("type_brace", "type X {\n    a: i64\n}\nfn main() -> i64 { 0 }\n"),
    ("pow", "fn main() -> i64 {\n    let a = 2\n    let b = a ** 2\n    b\n}\n"),
    ("deref", "fn main() -> i64 {\n    let a = 2\n    let b = *a\n    b\n}\n"),
    ("deref", "fn main() -> i64 {\n    let a = 2\n    let b = *a // **note**\n    b\n}\n"),
)
# Colliding shapes (spec AG-2): same parser message, not this habit; none may get a habit's help.
SYN_NONE = (
    ("record", 'fn main() -> i64 {\n    let d = {"k": 1}\n    0\n}\n'),
    ("record", "fn main() -> i64 {\n    c := 0\n    0\n}\n"),
    ("type_brace", "fn main() -> i64 {\n    let x { 1 }\n    0\n}\n"),
    ("type_in_fn", "type T = { kind: i64 }\nfn main() -> i64 {\n    let t = T { type: 1 }\n    0\n}\n"),
    ("type_in_fn", "fn main() -> i64 {\n    let s = {\n        type = 1\n    }\n    0\n}\n"),
)


def selftest():
    fails = []

    def expect(name, cond, detail=""):
        print(f"{'ok  ' if cond else 'FAIL'} {name}" + (f": {detail}" if detail and not cond else ""))
        if not cond:
            fails.append(name)

    # Pure part: classification of hand-written diagnostics.
    d = lambda msg, line, col, help=None: {"code": "E0000", "severity": "error", "message": msg,
                                           "line": line, "col": col, "help": help}
    src = "fn main() -> i64 {\n    let it = { name: 1 }\n    0\n}\n"
    expect("classify: an unnamed record colon is `record`",
           classify(src, d("unexpected token: Colon, expected expression", 2, 20)) == ["record"])
    src = 'fn main() -> i64 {\n    let d = {"k": 1}\n    0\n}\n'
    expect("classify: a dict-literal colon is not `record`",
           classify(src, d("unexpected token: Colon, expected expression", 2, 17)) == [])
    src = "fn main() -> i64 {\n    let b = a ** 2\n    b\n}\n"
    expect("classify: `**` is `pow`, not `deref`",
           classify(src, d("unexpected token: Star, expected expression", 2, 16)) == ["pow"])
    src = "type X {\n    a: i64\n}\n"
    expect("classify: `type X {` reported on the next line is `type_brace` (token line)",
           classify(src, d("unexpected token: LBrace, expected Eq", 2, 5)) == ["type_brace"])
    expect("classify: a non-E0000 diagnostic has no class",
           classify(src, {"code": "E0001", "message": "unexpected token: Type, expected expression"}) == [])
    expect("classify: a clean program has no class", classify(src, None) == [])
    expect("offset_of counts bytes, as span::SourceMap does",
           offset_of("é\nab", 2, 2) == 3 and offset_of("é\nab", 1, 3) == 1)
    r = {"is": {"n": 10, "any": 9, "own": 9}, "pow": {"n": 10, "any": 8, "own": 8}, "x": {"n": 0, "any": 0, "own": 0}}
    expect("below: 90% passes, 80% fails, n = 0 is not judged", below(r) == ["pow"])

    # Loader: a synthetic run directory (infra row skipped, Python-literal report, other stages ignored).
    with tempfile.TemporaryDirectory(prefix="habits-selftest-") as root:
        tdir = os.path.join(root, "cand", "s1", "transcripts")
        os.makedirs(tdir)
        reps = {"t-a": {"stage": "check", "source": "fn main() -> i64 { 0 }\n"},
                "t-b": repr({"stage": "check", "source": "fn f() {}\n", "check_ok": "False"}),
                "t-c": {"stage": "accepted", "source": "x"},
                "t-d": {"stage": "check", "source": "infra"}}
        for tid, rep in reps.items():
            with open(os.path.join(tdir, f"{tid}.a.json"), "w") as fh:
                json.dump({"rounds": [{"round": 0, "report": rep}]}, fh)
        with open(os.path.join(root, "cand", "rows.jsonl"), "w") as fh:
            fh.write(json.dumps({"id": "t-d", "seed": 1, "infra": True}) + "\n")
        got = round0_refused(root)
        expect("loader: check-stage round 0 only, infra skipped, literal reports parsed",
               got == [("t-a:s1", "fn main() -> i64 { 0 }\n"), ("t-b:s1", "fn f() {}\n")], str(got))

    # Live part: the real compiler classifies each synthetic program and the arm's help fires.
    axon = DEFAULT_BIN
    if not os.path.isfile(axon):
        if os.environ.get("GENVIAB_REQUIRE_BINS") == "1":
            expect("habits: the axon binary exists (GENVIAB_REQUIRE_BINS=1)", False, axon)
        else:
            print(f"skip (habits live): {axon} not built")
    else:
        for want, src in SYN:
            diag = first_error(src, axon)
            got = classify(src, diag)
            expect(f"live: a synthetic `{want}` program is classed `{want}`", got == [want], f"{got} {diag}")
            expect(f"live: a synthetic `{want}` program gets the `{want}` help", bool(diag) and own_help(want, diag),
                   str(diag))
        for name, src in SYN_NONE:
            diag = first_error(src, axon) or {}
            hit = [n for n, _, _, _ in CLASSES if own_help(n, diag)]
            expect(f"live: a colliding shape for `{name}` gets no habit help", not hit, f"{hit} {diag}")
        res, total, clean = tally([s for _, s in SYN], axon)
        expect("live: tally counts every synthetic program in its class",
               total == len(SYN) and clean == 0 and sum(r["n"] for r in res.values()) == len(SYN)
               and not below(res, 1.0), render(res, total, clean))
    if fails:
        print(f"habits selftest: {len(fails)} failure(s): {fails}")
        return 1
    print("habits selftest: all checks hold")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("run_dir", nargs="?")
    ap.add_argument("--arm", default="cand")
    ap.add_argument("--bin", default=DEFAULT_BIN)
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    if not a.run_dir:
        ap.error("RUN_DIR is required")
    sources = [s for _, s in round0_refused(a.run_dir, a.arm)]
    res, total, clean = tally(sources, a.bin)
    if a.json:
        print(json.dumps({"total": total, "clean": clean, "classes": res}, sort_keys=True))
    else:
        print(render(res, total, clean))
    return 0


if __name__ == "__main__":
    sys.exit(main())
