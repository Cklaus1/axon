#!/usr/bin/env python3
"""The mechanical half of governance/notes/v022-pci-delta.md.

Everything that changed under crates/axon-core/src since the certified interpreter
(31413ca7) is listed from git, not from memory: per-file line counts and every
commit, each tagged with the theme (and amendment) it belongs to. The note embeds
the output between two markers; `--check` regenerates it at the head the note
names and fails on any difference.

A commit under crates/axon-core/src that THEMES does not classify is a FAILURE,
so a new interpreter change cannot reach the tree without being placed in the
note. `--check` also fails when a commit after the note's pinned head touches
crates/axon-core/src: the note is then stale and must be regenerated
(`scripts/pci_delta.py --emit HEAD`, paste between the markers, update the pin).

    python3 scripts/pci_delta.py --emit [REV]   # print the block for REV (default HEAD)
    python3 scripts/pci_delta.py --check        # compare with the note
"""
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NOTE = os.path.join(ROOT, "governance/notes/v022-pci-delta.md")
BASE = "31413ca7"
PATHSPEC = "crates/axon-core/src"
BEGIN = "<!-- BEGIN MECHANICAL (scripts/pci_delta.py) -->"
END = "<!-- END MECHANICAL -->"

# commit (first 8 hex) -> (theme, what it does to pass/fail, by the commit's own message)
THEMES = {
    "abf72e0b": ("review fixes B1/B3 (main.rs)", "runner/test-CLI hardening from review wf_d725935a-7ed"),
    "bae904b8": ("keyed failure token (main.rs)", "the interpreter keys every FAILURE it decides, so a printed failure line is not a verdict: narrowing"),
    "a4a05f28": ("key and completion", "K is unreachable from candidate code: narrowing"),
    "e9a13f72": ("key and completion", "the raw key read is unix-only so the wasm32 build compiles: build fix"),
    "91c2f676": ("key and completion", "sealed handlers act only on sealed operations; completion is the body's end: narrowing"),
    "f2a12aab": ("key and completion", "row attack (Option-shaped test) where completion is the only guard: evidence"),
    "8f8e9755": ("sealed modules, module path", "an unreadable suite module never falls through to the candidate: narrowing"),
    "08903fdf": ("sealed modules, module path", "a sealed module's use resolves only in the sealed dirs: narrowing"),
    "38273ef1": ("sealed modules, module path", "one file per module name, suite first: narrowing"),
    "32fdea5c": ("sealed modules, module path", "the sealed-module rule holds whoever imports: narrowing"),
    "8e9858f4": ("RNG isolation", "a sealed frame may not reseed the process RNG: narrowing"),
    "b8bbdb74": ("RNG isolation", "a sealed frame may not DRAW from the process RNG: narrowing"),
    "bce231ad": ("RNG isolation", "RNG hardening completeness, M254 false retirement: narrowing"),
    "8a7962fb": ("RNG isolation", "per-kernel RNG isolation: narrowing"),
    "7da2fe71": ("rows, style, harness", "rustfmt and a row re-anchor: no intended change in semantics"),
    "e1e2a22f": ("rows, style, harness", "row cells and attack messages: no intended change in semantics"),
    "2a397d93": ("rows, style, harness", "row routes: no intended change in semantics"),
    "73834357": ("rows, style, harness", "+23 interp.rs lines with two defect fixes found making rows honest"),
    "48a077ee": ("amendment 74", "predicate primitives are refusal sites; keyed_outcome rowed"),
    "ba1bad03": ("amendment 74", "merge of c9r4c/gate"),
    "e8537726": ("amendment 53", "a value is cast at every declared boundary; an operator method name is the operator's: narrowing (A86)"),
    "3cf23eef": ("amendment 60", "the cast keys on the dispatch type; a local is never called by name: narrowing"),
    "8a5419de": ("amendment 60", "scalar-arm attacks reach their arms; M1155 compiles"),
    "fa0643ac": ("amendment 72", "at a seal crossing every type position is determined from the operator side or refused; E0505 refuses an impl for f32/isize/usize: narrowing"),
    "68176b53": ("amendment 72", "M1148/M1669 attacks: row repair"),
    "6b793da8": ("amendment 72", "the operator's dicts are snapshotted at a seal crossing and verified at every edge back: narrowing"),
    "3b9b9092": ("amendment 78", "a position the operator held is judged by what it held, deeply and strictly: narrowing"),
    "d29d4ef6": ("amendment 78", "row repairs (M1672, M1843, M1845)"),
    "2bf1d9d0": ("amendment 82", "comment-only (TestEnd doc)"),
    "40ca1092": ("amendment 82", "merge of c9r4c/claims; comment-only in this path"),
    "12c6685e": ("amendment 83", "the dispatch rule: operator code never selects an operator impl by a type nothing on the operator side determined (untyped dict/channel/lambda reads must be annotated; pin.rs): narrowing"),
    "554951b6": ("amendment 83", "unit-test case only (an unannotated lambda parameter case in interp.rs tests): no production change"),
    "9e19e961": ("amendment 83", "unit test and comments only (interpolation/comparison of an untyped read select no operator impl): no production change"),
    "b971194c": ("amendment 83", "clippy: the arithmetic arm of the pin walk (interp/pin.rs) collapsed into a guard: no change in what is refused"),
    "70692659": ("amendment 83", "unit-test attack text only (M1996: a shift truncates where + and * panic): no production change"),
    "dab96417": ("amendment 88", "the dispatch analysis trusts only what the operator chose: candidate fns/types/lets, local-name shadowing and trait names no longer determine a receiver; keys carry the owning fn; fail-closed lookup; operator-defined runtime values dispatch; the address cache is removed (pin.rs, interp.rs, eval.rs): narrowing on candidate-influenced receivers, widening only for operator-only polymorphism"),
    "edde3d5d": ("amendment 88", "clippy: an unused `mut` removed in pin.rs; no change in what is refused"),
}


def git(*a):
    return subprocess.run(["git", *a], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def block(rev):
    full = git("rev-parse", rev).strip()
    out = [f"generated-at: {full}", "", f"`git diff --numstat {BASE}..{full[:8]} -- {PATHSPEC}`:", "",
           "| file | added | removed |", "|---|---|---|"]
    ta = tr = 0
    for line in git("diff", "--numstat", f"{BASE}..{full}", "--", PATHSPEC).splitlines():
        a, r, f = line.split("\t")
        ta += int(a); tr += int(r)
        out.append(f"| `{f}` | {a} | {r} |")
    out.append(f"| total | {ta} | {tr} |")
    out += ["", f"`git log --reverse {BASE}..{full[:8]} -- {PATHSPEC}`:", "",
            "| commit | theme | what it does to pass/fail (from its message) |", "|---|---|---|"]
    missing = []
    n = 0
    for line in git("log", "--reverse", "--format=%H %s", f"{BASE}..{full}", "--", PATHSPEC).splitlines():
        h, subj = line.split(" ", 1)
        n += 1
        t = THEMES.get(h[:8])
        if t is None:
            missing.append(f"{h[:8]} {subj}")
            t = ("UNCLASSIFIED", "")
        out.append(f"| {h[:8]} | {t[0]} | {t[1]} |")
    out.append(f"| {n} commits | | |")
    return "\n".join(out), missing


def main():
    if len(sys.argv) >= 2 and sys.argv[1] == "--emit":
        # `--drop HASH8` forgets one classification: the control that shows an
        # unclassified commit is refused (crates/axon-core/tests/pci_delta_note.rs).
        if "--drop" in sys.argv:
            THEMES.pop(sys.argv[sys.argv.index("--drop") + 1], None)
        b, missing = block(sys.argv[2] if len(sys.argv) > 2 else "HEAD")
        print(BEGIN + "\n" + b + "\n" + END)
        if missing:
            print("UNCLASSIFIED: " + "; ".join(missing), file=sys.stderr)
            return 1
        return 0
    if len(sys.argv) == 2 and sys.argv[1] == "--check":
        text = open(NOTE).read()
        m = re.search(re.escape(BEGIN) + r"\n(.*?)\n" + re.escape(END), text, re.S)
        if not m:
            print("pci_delta: the note has no mechanical block")
            return 1
        pin = re.search(r"^generated-at: ([0-9a-f]{40})$", m.group(1), re.M)
        if not pin:
            print("pci_delta: the block names no generated-at head")
            return 1
        want, missing = block(pin.group(1))
        bad = []
        if missing:
            bad.append("unclassified commit(s): " + "; ".join(missing))
        if want != m.group(1):
            bad.append("the note's block is not what git says at its own pinned head")
        later = git("log", "--format=%h %s", f"{pin.group(1)}..HEAD", "--", PATHSPEC).strip()
        if later:
            bad.append("commits after the pinned head touch " + PATHSPEC + " (regenerate the note):\n" + later)
        if bad:
            print("pci_delta: FAIL\n" + "\n".join(bad))
            return 1
        print("pci_delta: PASS")
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
