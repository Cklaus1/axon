#!/usr/bin/env python3
"""release_ledger.py - every release bumps the version its changes require (release-contract RC-3).

Usage: python3 scripts/release_ledger.py [--root PATH] [--tag vX.Y.Z]
       python3 scripts/release_ledger.py [--root PATH] --section-has-behaviour-change vX.Y.Z

Reads, offline, under ROOT (default: this repository): CHANGELOG.md, Cargo.toml, Cargo.lock,
each `[workspace] members` manifest and crates/axon-core/card-versions.txt. A release heading
in CHANGELOG.md is

    ## vX.Y.Z - YYYY-MM-DD (card N, <sha8>)

(spec/stability.md rule 6); a release's section runs from its heading to the next release
heading. Refused, one stderr line per problem, exit 1:

0. a line starting `## v<digit>` that is not a full release heading, and an emphasised
   "behaviour change" (`**`, `__` or `*`, any case, either spelling, `changes`) that is not
   exactly the marker `**Behaviour change` (a near-miss marker), each naming its line;
1. release versions that do not strictly decrease top to bottom;
2. a section with a line containing the bold marker `**Behaviour change` (any suffix), or a
   card number other than the next-older release's, whose version does not bump MINOR (or
   MAJOR) over the next-older release; a MINOR bump whose PATCH is not 0, a MAJOR bump whose
   MINOR or PATCH is not 0;
3. a heading card number that card-versions.txt does not list, or a heading <sha8> that is
   not the prefix of line N's sha (a released card line is frozen); with --tag, a newest
   heading whose card number is not the file's last version; a card change not named in the
   CHANGELOG (spec/stability.md rule 4): a release whose card is newer than the next-older
   release's must say `card K` in its section for every K it moves past, and with any release
   heading, every card-versions.txt line newer than the newest heading's card must be named
   (`card K`) above that heading (the unreleased text);
4. with any release heading: the newest heading's version is not Cargo.toml's
   `[workspace.package] version`, or a member whose manifest says `version.workspace = true`
   has another version in Cargo.lock (self-versioned members are ignored);
5. with --tag: the tag is not the newest heading's version or the Cargo version, or text
   sits above the newest heading (the release must cover every batch). With no release
   heading, --tag refuses and the default mode passes (nothing released; rule 0 still runs).

Exit 0 prints `release_ledger: PASS (<n> releases)`.

--section-has-behaviour-change vX.Y.Z prints `1` when that release's section holds a marker
line (rule 2's test) and `0` otherwise, rc 0; rc 1 when no such heading exists. The release
workflow feeds it to scripts/compat_sweep.py --diff.
"""

import argparse
import re
import sys
import tomllib
from pathlib import Path

HEADING = re.compile(r"^## v(\d+)\.(\d+)\.(\d+) - \d{4}-\d{2}-\d{2} \(card (\d+), ([0-9a-f]{8})\)$")
# A line starting `## v<digit>` is meant as a release heading; one HEADING does not match is refused
# (rule 0), never folded into the section above it.
HEADING_LIKE = re.compile(r"^## v\d")
MARKER = "**Behaviour change"
# Emphasis plus "behaviour change" in any case or spelling. Every such span must be the exact
# MARKER (rule 0): a typo'd marker would otherwise ship a behaviour change as a PATCH.
NEAR_MARKER = re.compile(r"(?i)(\*\*|__|\*)\s*behaviou?r changes?")
TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
CARD_LINE = re.compile(r"^(\d+) ([0-9a-f]{64})$")


def names_card(lines, n):
    """True when a line says `card N` (word-bounded: `card 30` is not card 3)."""
    pat = re.compile(r"\bcard %d\b" % n)
    return any(pat.search(line) for line in lines)


def malformed(changelog):
    """Rule 0: near-miss marker spans and heading-like lines that are not a HEADING."""
    problems = []
    for i, line in enumerate(changelog.splitlines(), 1):
        if HEADING_LIKE.match(line) and not HEADING.match(line):
            problems.append(
                "CHANGELOG.md:%d: a `## v` line that is not a release heading "
                "`## vX.Y.Z - YYYY-MM-DD (card N, <sha8>)`: %r" % (i, line)
            )
        for m in NEAR_MARKER.finditer(line):
            if not line.startswith(MARKER, m.start()):
                problems.append(
                    "CHANGELOG.md:%d: a near-miss behaviour-change marker %r (the marker is exactly `%s`)"
                    % (i, m.group(0), MARKER)
                )
    return problems


def vstr(v):
    return "v%d.%d.%d" % v


def releases(changelog):
    """[(version, card, sha8, heading line no, section lines)], newest first, and the
    lines above the newest heading."""
    lines = changelog.splitlines()
    out = []
    first = None
    for i, line in enumerate(lines):
        m = HEADING.match(line)
        if not m:
            if out:
                out[-1][4].append(line)
            continue
        if first is None:
            first = i
        v = (int(m.group(1)), int(m.group(2)), int(m.group(3)))
        out.append((v, int(m.group(4)), m.group(5), i + 1, []))
    above = lines[:first] if first is not None else lines
    return out, above


def has_marker(section):
    return any(MARKER in line for line in section)


def card_lines(text, problems):
    shas = {}
    last = 0
    for i, line in enumerate(text.splitlines(), 1):
        m = CARD_LINE.match(line)
        if not m or int(m.group(1)) != i:
            problems.append("card-versions.txt:%d: not `%d <sha256>`: %r" % (i, i, line))
            continue
        shas[i] = m.group(2)
        last = i
    return shas, last


def check_versions(root, newest, problems):
    """Rule 4: Cargo.toml's version and the inheriting members' Cargo.lock versions."""
    ws = tomllib.loads((root / "Cargo.toml").read_text())
    cargo_version = ws.get("workspace", {}).get("package", {}).get("version")
    if cargo_version != "%d.%d.%d" % newest:
        problems.append(
            "the newest release %s is not Cargo.toml's [workspace.package] version %s" % (vstr(newest), cargo_version)
        )
    inheriting = []
    for member in ws.get("workspace", {}).get("members", []):
        pkg = tomllib.loads((root / member / "Cargo.toml").read_text()).get("package", {})
        if pkg.get("version") == {"workspace": True}:
            inheriting.append(pkg["name"])
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    locked = {}
    for p in lock.get("package", []):
        if "source" not in p:
            locked[p["name"]] = p["version"]
    for name in inheriting:
        if locked.get(name) != cargo_version:
            problems.append(
                "Cargo.lock holds %s %s, not the workspace version %s (run `cargo update -w --offline`)"
                % (name, locked.get(name), cargo_version)
            )
    return cargo_version


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", default=str(Path(__file__).resolve().parent.parent))
    ap.add_argument("--tag")
    ap.add_argument("--section-has-behaviour-change", dest="query", metavar="VERSION")
    a = ap.parse_args()
    root = Path(a.root)
    changelog = (root / "CHANGELOG.md").read_text()
    rels, above = releases(changelog)

    if a.query is not None:
        for v, _card, _sha8, _line, section in rels:
            if vstr(v) == a.query:
                print("1" if has_marker(section) else "0")
                return 0
        print("release_ledger: no release heading %s in CHANGELOG.md" % a.query, file=sys.stderr)
        return 1

    problems = malformed(changelog)
    shas, last_card = card_lines((root / "crates/axon-core/card-versions.txt").read_text(), problems)

    for k, (v, card, sha8, line, section) in enumerate(rels):
        where = "CHANGELOG.md:%d %s" % (line, vstr(v))
        if card not in shas:
            problems.append("%s: card %d is not listed in card-versions.txt" % (where, card))
        elif not shas[card].startswith(sha8):
            problems.append(
                "%s: card %d is %s in card-versions.txt, the heading says %s (a released card line was rewritten)"
                % (where, card, shas[card][:8], sha8)
            )
        if k + 1 == len(rels):
            continue
        older, older_card = rels[k + 1][0], rels[k + 1][1]
        if not v > older:
            problems.append("%s: not newer than the next-older release %s" % (where, vstr(older)))
            continue
        major = v[0] != older[0]
        minor = not major and v[1] != older[1]
        if major and (v[1], v[2]) != (0, 0):
            problems.append("%s: a MAJOR bump over %s must reset MINOR and PATCH to 0" % (where, vstr(older)))
        if minor and v[2] != 0:
            problems.append("%s: a MINOR bump over %s must reset PATCH to 0" % (where, vstr(older)))
        behaviour_changed = has_marker(section)
        card_changed = card != older_card
        if behaviour_changed or card_changed:
            if not (major or minor):
                why = []
                if behaviour_changed:
                    why.append("its section records a `**Behaviour change`")
                if card_changed:
                    why.append("its card %d differs from %s's card %d" % (card, vstr(older), older_card))
                problems.append(
                    "%s: %s, so it must bump MINOR over %s" % (where, " and ".join(why), vstr(older))
                )
        for n in range(older_card + 1, card + 1):
            if not names_card(section, n):
                problems.append(
                    "%s: moves to card %d over %s's card %d, and its section never says `card %d` "
                    "(spec/stability.md rule 4)" % (where, card, vstr(older), older_card, n)
                )

    if rels:
        newest, newest_card = rels[0][0], rels[0][1]
        cargo_version = check_versions(root, newest, problems)
        for n in range(newest_card + 1, last_card + 1):
            if not names_card(above, n):
                problems.append(
                    "card-versions.txt lists card %d, newer than the newest release's card %d, and no unreleased "
                    "CHANGELOG text above %s says `card %d` (spec/stability.md rule 4)" % (n, newest_card, vstr(newest), n)
                )
    if a.tag is not None:
        if not TAG.match(a.tag):
            problems.append("--tag %s is not vX.Y.Z" % a.tag)
        elif not rels:
            problems.append("--tag %s: CHANGELOG.md has no release heading" % a.tag)
        else:
            if a.tag != vstr(newest) or a.tag != "v%s" % cargo_version:
                problems.append(
                    "--tag %s is not the released version (newest heading %s, Cargo.toml %s)"
                    % (a.tag, vstr(newest), cargo_version)
                )
            if newest_card != last_card:
                problems.append(
                    "--tag %s: the release names card %d, card-versions.txt ends at card %d"
                    % (a.tag, newest_card, last_card)
                )
            unreleased = [l for l in above if l.strip() and not l.startswith("# ")]
            if unreleased:
                problems.append(
                    "--tag %s: unreleased text above the newest release heading: %r" % (a.tag, unreleased[0])
                )

    if problems:
        for p in problems:
            print("release_ledger: %s" % p, file=sys.stderr)
        print("release_ledger: FAIL (%d problems)" % len(problems), file=sys.stderr)
        return 1
    print("release_ledger: PASS (%d releases)" % len(rels))
    return 0


if __name__ == "__main__":
    sys.exit(main())
