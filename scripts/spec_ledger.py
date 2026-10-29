#!/usr/bin/env python3
"""spec_ledger.py - the build specs' status lines agree with the ledger (docs-truth DT-6, O-GAP-11).

Usage: python3 scripts/spec_ledger.py [ROOT] [--list]

Shipped work kept its DRAFT status, and opportunities stayed open after their fix
shipped (L015, O-HI-02, O-GAP-11). This check reads `tasks/*-spec.md`,
`tasks/opportunities.md`, CHANGELOG.md and ROADMAP.md, offline, and enforces:

0. An item id (a `##`/`###` heading such as `SF-3`) is unique across the specs.
1. Every spec has `**Status: <WORD>` in its first 15 lines; WORD is DRAFT, REVIEWED,
   AUTHORIZED, BUILT, PARTIAL, STOPPED or SUPERSEDED.
2. BUILT/PARTIAL names a commit that is an ancestor of HEAD, inside the bold status
   span (later prose does not count).
3. BUILT/PARTIAL: CHANGELOG.md names one of the spec's item ids, or the spec file.
4. DRAFT/REVIEWED/AUTHORIZED: CHANGELOG.md names none of the spec's item ids.
5. BUILT/PARTIAL: every O-* id cited in the spec's first Source paragraph (wherever
   it sits) or an item heading has an opportunities entry carrying a closure stamp
   ([RESOLVED|FIXED|DONE|CLOSED|BUILT|WITHDRAWN|SUPERSEDED ...]), unless a PARTIAL
   status line names it as open: the whole id, then `open` before the next `;`, `.`
   or `)` ("O-S1-02 stays open"). Merely naming the id is not an exemption.
6. An opportunities entry saying `SPECCED ... tasks/X-spec.md`, X BUILT/PARTIAL,
   carries a closure stamp (same PARTIAL exemption).
7. ROADMAP.md points at this script (the index `--list` prints).

Lines inside a fenced block (``` or ~~~, CommonMark closing) are quoted text, not
the spec's own. SPEC_LEDGER_GIT names the repository for rule 2 (default ROOT).

Exit 0: consistent (`spec_ledger: PASS ...`). Exit 1: each problem printed as
`spec_ledger: <problem>`, then `spec_ledger: FAIL ...`.
"""
import glob, os, re, subprocess, sys

STATUS_RE = re.compile(r"^\*\*Status: (DRAFT|REVIEWED|AUTHORIZED|BUILT|PARTIAL|STOPPED|SUPERSEDED)\b(.*)")
CLOSED_RE = re.compile(r"\[(RESOLVED|FIXED|DONE|CLOSED|BUILT|WITHDRAWN|SUPERSEDED)\b")
ID_RE = re.compile(r"\bO-[A-Z][A-Z0-9]*-\d+\b")
ENTRY_RE = re.compile(r"^#{2,3} (O-[A-Z][A-Z0-9]*-\d+)\b")
SPECCED_RE = re.compile(r"(?i)specced\b[^\n]*?tasks/([\w.-]+-spec\.md)")
SHA_RE = re.compile(r"\b[0-9a-f]{7,12}\b")

ITEM_RE = re.compile(r"^#{2,3} ([A-Z][A-Z0-9]*-?\d+[a-z]?)\b")

def entries(path):
    out, cur = {}, None
    for line in open(path, encoding="utf-8"):
        m = ENTRY_RE.match(line)
        if m:
            cur = m.group(1)
            out[cur] = []
            continue
        if line.startswith("#"):
            cur = None
        elif cur:
            out[cur].append(line)
    return {k: "".join(v) for k, v in out.items()}

def is_ancestor(root, sha):
    git = os.environ.get("SPEC_LEDGER_GIT", root)
    r = subprocess.run(["git", "-C", git, "merge-base", "--is-ancestor", sha, "HEAD"],
                       capture_output=True)
    return r.returncode == 0

def named_open(oid, status, rest):
    # The PARTIAL exemption: the status span says this id (whole, not a prefix of a
    # longer one) stays open. "O-GAP-10 built too" is not an exemption.
    return status == "PARTIAL" and re.search(
        r"(?<![\w-])" + re.escape(oid) + r"(?![\w-])[^;.)]*?\bopen\b", rest) is not None


FENCE_RE = re.compile(r"^\s*(`{3,}|~{3,})")

def prose_lines(text):
    # Headings and Source lines inside a fence (quoted CHANGELOG text, code) are not the spec's own.
    # CommonMark: a fence closes only on the same character, at least as long, with nothing after it.
    out, fence = [], None
    for line in text.split("\n"):
        m = FENCE_RE.match(line)
        if fence is None:
            if m:
                fence = m.group(1)
                continue
            out.append(line)
        elif m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence) \
                and line.strip() == m.group(1):
            fence = None
    return out

def main(root, listing=False):
    opp = entries(os.path.join(root, "tasks/opportunities.md"))
    problems, table, status_of = [], [], {}
    changelog = open(os.path.join(root, "CHANGELOG.md"), encoding="utf-8").read()
    specs = sorted(glob.glob(os.path.join(root, "tasks/*-spec.md")))
    owner = {}
    for spec in specs:
        rel = os.path.relpath(spec, root)
        lines = prose_lines(open(spec, encoding="utf-8").read())
        m = next((m for l in lines[:15] if (m := STATUS_RE.match(l))), None)
        if m is None:
            problems.append(f"{rel}: no `**Status: <WORD>` line in its first 15 lines")
            continue
        # Only the bold span is the status claim; later prose may cite unrelated commits.
        status, rest = m.group(1), m.group(2).split("**")[0]
        status_of[os.path.basename(rel)] = (status, rest)
        table.append((rel, status, rest.strip()[:60]))
        items = {mm.group(1) for l in lines if (mm := ITEM_RE.match(l))}
        for i in sorted(items):
            if i in owner:
                problems.append(f"{rel}: item {i} also defined in {owner[i]}")
            else:
                owner[i] = rel
        shipped = sorted(i for i in items if re.search(r"(?<![\w-])" + re.escape(i) + r"(?![\w-])", changelog))
        if status in ("DRAFT", "REVIEWED", "AUTHORIZED") and shipped:
            problems.append(f"{rel}: status {status}, but CHANGELOG.md names its items {shipped}")
        if status in ("BUILT", "PARTIAL") and not shipped and os.path.basename(rel) not in changelog:
            problems.append(f"{rel}: status {status}, but CHANGELOG.md names none of its items")
        if status not in ("BUILT", "PARTIAL"):
            continue
        shas = SHA_RE.findall(rest)
        if not shas:
            problems.append(f"{rel}: status {status} names no commit")
        elif not any(is_ancestor(root, s) for s in shas):
            problems.append(f"{rel}: status {status} names {shas}, none an ancestor of HEAD")
        # The first Source paragraph, wherever it sits, runs from its `Source` line to the
        # next blank line (it may wrap).
        cited, in_source, seen = set(), False, False
        for l in lines:
            if not seen and (l.startswith("Source") or l.startswith("**Source")):
                in_source = seen = True
            elif not l.strip():
                in_source = False
            if in_source:
                cited |= set(ID_RE.findall(l))
        for l in lines:
            if re.match(r"^#{2,4} ", l):
                cited |= set(ID_RE.findall(l))
        for oid in sorted(cited):
            if oid not in opp:
                problems.append(f"{rel}: cites {oid}, which has no entry in tasks/opportunities.md")
            elif not CLOSED_RE.search(opp[oid]) and not named_open(oid, status, rest):
                problems.append(f"{rel}: status {status}, but {oid} carries no closure stamp")
    for oid, body in sorted(opp.items()):
        for name in set(SPECCED_RE.findall(body)):
            st, rest = status_of.get(name, (None, ""))
            if st in ("BUILT", "PARTIAL") and not CLOSED_RE.search(body) \
                    and not named_open(oid, st, rest):
                problems.append(f"{oid}: specced in {name} ({st}) but carries no closure stamp")
    roadmap = open(os.path.join(root, "ROADMAP.md"), encoding="utf-8").read()
    if "scripts/spec_ledger.py" not in roadmap:
        problems.append("ROADMAP.md: does not point at `scripts/spec_ledger.py --list`")
    if listing:
        for r in table:
            print(" | ".join(r))
    for p in problems:
        print("spec_ledger:", p)
    print(f"spec_ledger: {'FAIL' if problems else 'PASS'} ({len(problems)} problem(s), {len(specs)} specs)")
    return 1 if problems else 0


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if a != "--list"]
    sys.exit(main(args[0] if args else ".", "--list" in sys.argv[1:]))
