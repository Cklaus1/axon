#!/usr/bin/env python3
"""The mechanical half of governance/notes/v022-pci-delta.md.

Everything that changed under crates/axon-core/src since the certified interpreter
(31413ca7) is listed from git, not from memory: per-file line counts and every
commit, each tagged with the theme (and amendment) it belongs to. The note embeds
the output between two markers; `--check` regenerates it at the head the note
names and fails on any difference.

A commit under crates/axon-core/src that THEMES (governance/notes/pci-themes/,
one file per commit) does not classify is a FAILURE, so a new interpreter change
cannot reach the tree without being placed in the note; a merge with no file is
classified only when it is a pure join (see PURE_JOIN). `--check` also fails when
a commit after the note's pinned head touches crates/axon-core/src: the note is
then stale and must be regenerated (`python3 tasks/r51-pci-regen.py`).

    python3 scripts/pci_delta.py --emit [REV] [--drop HASH8]  # print the block for REV (default HEAD)
    python3 scripts/pci_delta.py --check        # compare with the note
    python3 scripts/pci_delta.py --merge-note BASE OURS THEIRS  # the note's git merge driver
"""
import json
import os
import re
import shlex
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NOTE = os.path.join(ROOT, "governance/notes/v022-pci-delta.md")
TRIAGE = os.path.join(ROOT, "governance/notes/v022-psv1-loop-triage.md")
BASE = "31413ca7"
PATHSPEC = "crates/axon-core/src"
BEGIN = "<!-- BEGIN MECHANICAL (scripts/pci_delta.py) -->"
END = "<!-- END MECHANICAL -->"

# commit (first 8 hex) -> (theme, what it does to pass/fail, by the commit's own message). One file per
# commit, governance/notes/pci-themes/<hash8>.json = {"theme": ..., "text": ...}, so two lanes that classify
# different commits merge without a conflict. Theme `merged from main (AX findings)`: origin/main merged into
# v022/veto (PR #8); theme `merged from main (R51 S0)`: origin/main 5864c423 merged into r51/unify (R51 S0).
# Those commits were written and tested on main, NOT reviewed against the PCI claims: none is a PCI narrowing,
# and what they do to pass/fail was not analysed there (R51 keeps every sealed run refused until the seal is
# re-homed onto this interpreter, S6). The ones that touch the interpreter are listed as such so a reviewer
# reads them.
THEMES_DIR = os.path.join(ROOT, "governance/notes/pci-themes")


def load_themes():
    out = {}
    for name in sorted(os.listdir(THEMES_DIR)):
        m = re.fullmatch(r"([0-9a-f]{8})\.json", name)
        if not m:
            sys.exit(f"pci_delta: {THEMES_DIR}/{name} is not <hash8>.json")
        with open(os.path.join(THEMES_DIR, name), encoding="utf-8") as f:
            d = json.load(f)
        if not (isinstance(d, dict) and set(d) == {"theme", "text"} and all(isinstance(v, str) for v in d.values())):
            sys.exit(f"pci_delta: {name} must be {{\"theme\": str, \"text\": str}}")
        out[m.group(1)] = (d["theme"], d["text"])
    return out


THEMES = load_themes()

# A merge with no themes file is a PURE JOIN, and classified as one, iff git's merge of its two parents
# (`git merge-tree --write-tree`) leaves no conflicted path under crates/axon-core/src and the merge
# commit's crates/axon-core/src is that merge's crates/axon-core/src: it then changes the interpreter only
# by the commits it joins, each of which is listed and classified on its own. A conflict outside that tree
# (a registry, a status file) does not count: whatever the merge did there is not an interpreter change. A
# merge that conflicted under crates/axon-core/src (however it was resolved, even to one side's text), or
# that changes anything there git's merge does not (an evil merge), stays UNCLASSIFIED until a themes file
# is written for it.
# The replay runs with RENAME DETECTION OFF (merge.renames=false, merge.directoryRenames=false): the listing
# (`git log -- crates/axon-core/src`) does not follow renames, so a commit that edits a file outside that
# tree is never listed even when the other side moves the file into it. With renames on, git's merge would
# carry that unlisted edit into crates/axon-core/src without a conflict and the merge would pass as a pure
# join; with them off, the replayed tree has the moved file without the edit (the edit stays at the old
# path, a conflict outside the tree), so the merge's tree differs and it is UNCLASSIFIED.
# The replay does not depend on repository, user or system configuration for what it judges: rename
# detection and merge.renormalize are pinned on the command line; attributes come from a fixed tree
# (attr_tree) that gives every path the built-in text merge (`* merge`, which git resolves without reading
# merge.default or any merge.<driver> setting, so a `merge.default=union` cannot hide a conflict) and the
# note its driver, given on the command line; no user (core.attributesFile) or system (GIT_ATTR_NOSYSTEM)
# attributes file is read.
PURE_JOIN = "pure join: crates/axon-core/src equals `git merge-tree --write-tree` of its two parents, with no conflict under it; no change beyond the commits it joins"
NOTE_ATTRIBUTE = "governance/notes/v022-pci-delta.md merge=pci-note"
# The note's merge driver (`--merge-note`, .gitattributes `merge=pci-note`): the mechanical block is taken
# from ours and the rest of the note is merged three-way; the block is regenerated after the merge
# (tasks/r51-pci-regen.py, which installs this command as `merge.pci-note.driver`, relative to the worktree
# top so each worktree runs its own copy).
NOTE_DRIVER = "python3 -B scripts/pci_delta.py --merge-note %O %A %B"
_ATTR_TREE = []


def attr_tree():
    """A tree whose only file is a .gitattributes giving every path the built-in text merge and the note
    its driver: the pure-join replay reads attributes from it, not from the worktree, the merge commit or a
    user file."""
    if not _ATTR_TREE:
        blob = subprocess.run(["git", "hash-object", "-w", "--stdin"], cwd=ROOT, check=True, capture_output=True,
                              text=True, input="* merge\n" + NOTE_ATTRIBUTE + "\n").stdout.strip()
        _ATTR_TREE.append(subprocess.run(["git", "mktree"], cwd=ROOT, check=True, capture_output=True, text=True,
                                         input=f"100644 blob {blob}\t.gitattributes\n").stdout.strip())
    return _ATTR_TREE[0]


def pure_join(h, subj):
    """(theme, PURE_JOIN) when merge commit `h` is a pure join of its two parents, else None."""
    parents = git("show", "-s", "--format=%P", h).split()
    if len(parents) != 2:
        return None
    driver = f"{shlex.quote(sys.executable)} -B {shlex.quote(os.path.abspath(__file__))} --merge-note %O %A %B"
    r = subprocess.run(["git", "-c", "merge.renames=false", "-c", "merge.directoryRenames=false",
                        "-c", "merge.renormalize=false", "-c", f"merge.pci-note.driver={driver}",
                        "-c", f"core.attributesFile={os.devnull}", f"--attr-source={attr_tree()}",
                        "merge-tree", "--write-tree", "-z", "--name-only", "--no-messages", *parents],
                       cwd=ROOT, capture_output=True, env={**os.environ, "GIT_ATTR_NOSYSTEM": "1"})
    if r.returncode not in (0, 1):
        sys.exit(f"pci_delta: git merge-tree {parents[0][:8]} {parents[1][:8]} (for {h[:8]}) exited {r.returncode}: "
                 + r.stderr.decode(errors="replace").strip())
    # `-z --name-only`: the merged tree, then each conflicted path, NUL-terminated.
    tree, *conflicted = r.stdout.decode(errors="surrogateescape").split("\0")
    if any(p.startswith(PATHSPEC + "/") for p in conflicted):
        return None
    joined, merged = git("rev-parse", f"{tree}:{PATHSPEC}", f"{h}:{PATHSPEC}").split()
    if joined != merged:
        return None
    b = re.match(r"Merge (?:remote-tracking )?branch '([^']+)'", subj)
    return (f"{b.group(1)} merge" if b else "merge", PURE_JOIN)


# `--plant KEY:OLD=>NEW` (KEY: note | verdict | gates | spec | matrix | triage) changes the text of one input IN MEMORY before
# `--check` judges it: the control that shows a wrong table row, a wrong result, a wrong package or a wrong
# claim list is REFUSED (crates/axon-core/tests/pci_delta_note.rs). A plant that matches nothing is an error, so
# a control cannot pass because it planted nothing.
PLANTS = {}


def read(key, path):
    text = open(path).read()
    for old, new in PLANTS.get(key, []):
        if old not in text:
            print(f"pci_delta: --plant {key}: {old!r} is not in {os.path.basename(path)}", file=sys.stderr)
            sys.exit(3)
        text = text.replace(old, new, 1)
    return text


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
        t = THEMES.get(h[:8]) or pure_join(h, subj)
        if t is None:
            missing.append(f"{h[:8]} {subj}")
            t = ("UNCLASSIFIED", "")
        out.append(f"| {h[:8]} | {t[0]} | {t[1]} |")
    out.append(f"| {n} commits | | |")
    return "\n".join(out), missing


def merge_note(base, ours, theirs):
    """The note's merge driver: base and theirs get ours' mechanical block, then `git merge-file` merges
    the rest into `ours` (git's %A, the result). Exit 0 iff no conflict is left outside the block."""
    blk = re.compile(re.escape(BEGIN) + r"\n.*?\n" + re.escape(END), re.S)
    with open(ours, encoding="utf-8") as f:
        m = blk.search(f.read())
    with tempfile.TemporaryDirectory() as d:
        sides = []
        for name, path in (("base", base), ("theirs", theirs)):
            with open(path, encoding="utf-8") as f:
                text = f.read()
            if m:
                text = blk.sub(lambda _: m.group(0), text, count=1)
            sides.append(os.path.join(d, name))
            with open(sides[-1], "w", encoding="utf-8") as f:
                f.write(text)
        r = subprocess.run(["git", "merge-file", "-L", "ours", "-L", "base", "-L", "theirs", ours, *sides])
    return 0 if r.returncode == 0 else 1


def registry_ranges():
    """{amendment: (lo, hi, ids)} for the mutation rows tagged `(amN` in their description
    (amendments 102 onward tag their rows): the ranges the note and the claim quote are
    DERIVED from the registry here, never retyped."""
    sys.path.insert(0, os.path.join(ROOT, "scripts"))
    import v022_g01_mutations as reg  # noqa: E402

    out = {}
    for r in reg.MUTATIONS:
        mm = re.search(r"\(am(\d+)\b", r[1])
        if mm:
            out.setdefault(int(mm.group(1)), []).append((int(r[0][1:]), r[7].split("::")[-1], r[5], r[6], r[7]))
    return {n: (min(i for i, *_ in v), max(i for i, *_ in v), v) for n, v in out.items()}


GATES = os.path.join(ROOT, "scripts/v022_pci_gates.sh")
# The cargo invocation the gate script builds for each row, as the lines that build it say.
INVOCATION = (
    'cargo test --locked -q -p "$pkg" "${feat[@]}" "${sel[@]}" -- --exact $names',
    'if [ "$target" = lib ]; then sel=(--lib); else sel=(--test "$target"); fi',
)
# Amendments in the note's table that are not interpreter-EVALUATION deltas the claim lists:
# 74 made predicate primitives refusal sites and rowed `keyed_outcome` (test CLI, not evaluation).
NOT_IN_CLAIM_LIST = {74}


def gate_rows():
    """[(label, package, target, [exact test names])] parsed from the script's ROWS array, and the
    script text. The invocation is the one the script builds from those fields, checked below."""
    text = read("gates", GATES)
    rows = [
        (m.group(1).strip(), m.group(2), m.group(3), m.group(4).split())
        for m in re.finditer(r'^  "([^"|]*)\|([^"|]*)\|([^"|]*)\|([^"|]*)"$', text, re.M)
    ]
    return rows, text


def gate_runs(rows):
    """{(package, cargo target selector, exact test name)} the gate runs: `--lib` for `lib`, else
    `--test <target>`, which is how registry rows spell their target."""
    out = set()
    for _, pkg, target, names in rows:
        sel = "--lib" if target == "lib" else f"--test {target}"
        out.update((pkg, sel, n) for n in names)
    return out


def doc_drift():
    """The amendment -> matrix-rows and amendment -> mutation-ids statements of the note and of
    the claim, each checked against the file that owns it."""
    bad = []
    note = read("note", NOTE)
    spec = read("spec", os.path.join(ROOT, "governance/specs/v022-psv-protocol.md"))
    verdict = read("verdict", os.path.join(ROOT, "governance/specs/v022-protected-suite-verdict.md"))
    matrix = read("matrix", os.path.join(ROOT, "governance/specs/v022-psv-negative-matrix.md"))
    have = {int(m.group(1)) for m in re.finditer(r"^\|\s*A(\d+)\s*\|", matrix, re.M)}
    reg = registry_ranges()
    grows, gates = gate_rows()
    runs = gate_runs(grows)
    for shape in INVOCATION:
        if shape not in gates:
            bad.append(f"the gate script no longer builds its cargo invocation as pci_delta.py parses it: missing `{shape}`")
    for line in note.splitlines():
        m = re.match(r"\| amendment (\d+) \|(.*)", line)
        if not m:
            continue
        n = int(m.group(1))
        mr = re.search(r"matrix (A\d+)(?:-(A\d+))?", m.group(2))
        if mr:
            lo = int(mr.group(1)[1:])
            hi = int(mr.group(2)[1:]) if mr.group(2) else lo
            # The amendment's own text states its rows (and the integrator rewrites that text
            # when it renumbers), so the note must say what the amendment says.
            i = re.search(r"^%d\. \*\*" % n, spec, re.M)
            st = re.search(r"matrix rows? A(\d+)(?:-A(\d+))?", spec[i.start() : i.start() + 3500]) if i else None
            if st:
                slo = int(st.group(1))
                shi = int(st.group(2)) if st.group(2) else slo
                if (slo, shi) != (lo, hi):
                    bad.append(f"the note gives amendment {n} matrix rows A{lo}-A{hi}; the amendment says A{slo}-A{shi}")
            miss = [k for k in range(lo, hi + 1) if k not in have]
            if miss:
                bad.append(f"the note gives amendment {n} matrix rows A{lo}-A{hi}; the matrix has no A{miss[0]}")
        if n in reg:
            want = f"M{reg[n][0]}-M{reg[n][1]}"
            if want not in m.group(2):
                bad.append(f"the note gives amendment {n} mutation ids other than the registry's {want}")
    for n, (lo, hi, ids) in sorted(reg.items()):
        if f"M{lo}-M{hi}" not in note:
            bad.append(f"the note does not give amendment {n}'s registry range M{lo}-M{hi}")
        # Every row of the amendment fails a test some gate row RUNS: the package, the cargo
        # target and the exact name of the invocation the script builds (not a substring of it).
        for _, t, pkg, tgt, full in ids:
            if (pkg, tgt, full) not in runs:
                bad.append(f"amendment {n}: the test {full} ({pkg} {tgt}) of a mutation row is run by no gate row")
    for m in re.finditer(r"am(\d+) \(M(\d+)-M(\d+)\)", verdict):
        n = int(m.group(1))
        if n in reg and (int(m.group(2)), int(m.group(3))) != reg[n][:2]:
            bad.append(f"the claim quotes am{n} as M{m.group(2)}-M{m.group(3)}; the registry says M{reg[n][0]}-M{reg[n][1]}")
    # The coverage table of the note is the gate script's rows plus the am102 sweep: label, package/target
    # AND the result each row prints (`PASS n/n`, n the number of named tests), not the labels alone.
    t = re.search(r"\| row \| package/target \| result \|\n\|[-|]*\|\n((?:\|.*\n)+)", note)
    if not t:
        bad.append("the note has no gate-row coverage table")
    else:
        lab = lambda x: re.sub(r"\s+", " ", x).strip()
        tab = sorted((lab(c[1]), lab(c[2]), lab(c[3])) for c in (l.split("|") for l in t.group(1).splitlines()))
        want = sorted(
            [(lab(l), f"{pkg}/{tg}", f"PASS {len(ns)}/{len(ns)}") for l, pkg, tg, ns in grows]
            + [(lab("am102 sweep (only the taint on: exactly the two static-only programs differ)"), "axon-core/lib (PSV1T_TAINT_ONLY)", "PASS")]
        )
        if tab != want:
            only_t = [x for x in tab if x not in want][:3]
            only_s = [x for x in want if x not in tab][:3]
            bad.append(f"the note's coverage table ({len(tab)} rows) is not the gate script's rows plus the sweep ({len(want)}): table-only {only_t}, script-only {only_s}")
    # The claim's two amendment lists are derived, not retyped: the delta amendments are the note's table
    # rows (bar those that are not evaluation deltas), and the amendments whose ARMS are verified to fail a
    # gate row are am100 and every amendment the registry tags.
    delta = sorted({int(x) for x in re.findall(r"^\| amendment (\d+) \|", note, re.M)} - NOT_IN_CLAIM_LIST)
    m = re.search(r"plus amendments ([0-9/]+) \(the delta", verdict)
    if not m or [int(x) for x in m.group(1).split("/")] != delta:
        bad.append("the claim's delta amendment list is not the note's table: expected " + "/".join(map(str, delta)) + ", the claim says " + (m.group(1) if m else "nothing"))
    arms = sorted({100} | set(reg))
    m = re.search(r"and the arms of\s+((?:am\d+, )*am\d+(?: and am\d+)?) \(", verdict)
    got = sorted(int(x) for x in re.findall(r"am(\d+)", m.group(1))) if m else None
    if got != arms:
        bad.append("the claim's list of amendments whose arms are verified is not am100 plus the registry's: expected " + ", ".join(f"am{x}" for x in arms) + ", the claim says " + (m.group(1) if m else "nothing"))
    # The PSV-1 claim's list of what is NOT claimed names every finding the loop's triage table decided
    # NARROW-CLAIM (amendment 117), and nothing else: a finding cannot be narrowed in the table and
    # silently dropped from the claim, and the claim cannot keep an entry the table no longer narrows.
    tri = read("triage", TRIAGE)
    tm = re.search(r"<!-- BEGIN TRIAGE.*?-->\n(.*?)\n<!-- END TRIAGE -->", tri, re.S)
    if not tm:
        bad.append("the triage table has no BEGIN TRIAGE / END TRIAGE markers")
        narrowed, decided = set(), {}
    else:
        decided = {}
        for line in tm.group(1).splitlines():
            c = [x.strip() for x in line.strip().strip("|").split("|")]
            if len(c) >= 4 and c[0].isdigit():
                decided[c[1].strip("`")] = c[3]
        narrowed = {sig for sig, dec in decided.items() if dec == "NARROW-CLAIM"}
        for sig, dec in decided.items():
            if dec not in ("FIX", "NARROW-CLAIM", "NOT-A-RUBRIC-ISSUE"):
                bad.append(f"the triage table gives finding `{sig}` the decision {dec!r}")
        if len(decided) < 53:
            bad.append(f"the triage table has {len(decided)} findings, the loop confirmed 53")
    nm = re.search(r"<!-- BEGIN PSV-1 NON-CLAIMS.*?-->\n(.*?)\n<!-- END PSV-1 NON-CLAIMS -->", verdict, re.S)
    if not nm:
        bad.append("the claim has no BEGIN PSV-1 NON-CLAIMS / END PSV-1 NON-CLAIMS list")
        listed = set()
    else:
        listed = set(re.findall(r"^\s+- `([a-z0-9_-]+)`\s*$", nm.group(1), re.M))
    for sig in sorted(narrowed - listed):
        bad.append(f"the finding `{sig}` is NARROW-CLAIM in the triage table and is not in the claim's non-claim list")
    for sig in sorted(listed - narrowed):
        bad.append(f"the claim's non-claim list names `{sig}`, which the triage table does not mark NARROW-CLAIM")
    # The list of non-retired rows whose target is the interpreter, with its count.
    sys.path.insert(0, os.path.join(ROOT, "scripts"))
    import v022_g01_mutations as regm  # noqa: E402

    ids = sorted(int(r[0][1:]) for r in regm.MUTATIONS if r[2].startswith(PATHSPEC) and r[0] not in regm.RETIRED)
    runs, a = [], ids[0]
    for x, y in zip(ids, ids[1:] + [None]):
        if y != x + 1:
            runs.append(f"M{a}" if a == x else f"M{a}-M{x}")
            a = y
    want_list = f"{len(ids)} rows at this head, recomputed from the script): " + ", ".join(runs)
    if want_list not in note:
        bad.append("the note's list of interpreter mutation rows is not the registry's: expected " + want_list[:120] + " ... " + want_list[-80:])
    return bad


def main():
    args = sys.argv[1:]
    while "--plant" in args:
        i = args.index("--plant")
        spec = args[i + 1]
        key, _, rest = spec.partition(":")
        old, sep, new = rest.partition("=>")
        if not sep or key not in ("note", "verdict", "gates", "spec", "matrix", "triage"):
            print("pci_delta: --plant KEY:OLD=>NEW with KEY in note|verdict|gates|spec|matrix|triage", file=sys.stderr)
            return 2
        PLANTS.setdefault(key, []).append((old, new))
        del args[i : i + 2]
    sys.argv[1:] = args
    if len(sys.argv) == 5 and sys.argv[1] == "--merge-note":
        return merge_note(*sys.argv[2:])
    if len(sys.argv) >= 2 and sys.argv[1] == "--emit":
        # `--drop HASH8` forgets one classification: the control that shows an unclassified commit is
        # refused, and that a merge whose file is dropped is classified only if it is a pure join
        # (crates/axon-core/tests/pci_delta_note.rs).
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
        rows = len(re.findall(r'^  "[^"]*\|[^"]*\|[^"]*\|[^"]*"$', open(os.path.join(ROOT, "scripts/v022_pci_gates.sh")).read(), re.M))
        for q in re.findall(r"v022_pci_gates[^\n]{0,40}?(\d+) rows", open(NOTE).read()):
            if int(q) != rows:
                bad.append(f"the note quotes {q} gate rows, the script has {rows}")
        if re.search(r"v022_pci_gates\.sh`?,? \d+ rows", open(os.path.join(ROOT, "governance/specs/v022-protected-suite-verdict.md")).read()):
            bad.append("the verdict spec quotes a hand-typed gate row count")
        bad += doc_drift()
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
