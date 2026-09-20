#!/usr/bin/env python3
"""Generate AXON-COMPLETENESS.md from AXON-COMPLETENESS.json, and gate it.

A hand-maintained completeness table is intuition in a grid: every row is a
claim its author made about their own work, and nothing stops a row saying
"complete / proven" because it felt that way on the day. This exists to make
the claims falsifiable, so it does two things and the SECOND is the point:

  1. render the markdown, so the table cannot drift from its source;
  2. REFUSE claims that are not backed:
       * a row claiming production_proof=yes or mutation=yes must cite
         evidence, and every cited path must exist on disk;
       * a row cannot be implementation=complete while its production proof
         is missing or unknown — "it exists" is not "the production path uses
         it", which is the distinction this whole file is for;
       * enum values must be legal, so a typo cannot read as a pass;
       * the row count cannot silently shrink.

Exit 0 generate+pass, 1 a claim failed, 2 the checker could not run (a broken
checker must not be indistinguishable from a clean tree).
"""
import json, os, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "AXON-COMPLETENESS.json")
OUT = os.path.join(ROOT, "AXON-COMPLETENESS.md")
MIN_ROWS = 25

# `unknown` is legal on EVERY axis, deliberately. Forcing a value where none
# has been established is how a placeholder becomes a claim: the first version
# of this file omitted `unknown` from `docs`, which would have made every
# unassessed crate assert something about its documentation.
IMPL = {"complete", "partial", "staged", "absent", "unknown"}
TRI = {"yes", "no", "unknown"}
QUAD = {"yes", "partial", "no", "unknown"}
DOCS = {"yes", "partial", "no", "unknown"}


def die(why, code=2):
    print(f"CHECKER ERROR: {why}", file=sys.stderr)
    sys.exit(code)


try:
    art = json.load(open(SRC))
except Exception as e:
    die(f"cannot read {os.path.basename(SRC)}: {e}")

rows = art.get("rows")
if not isinstance(rows, list):
    die("the manifest has no `rows` list")
if len(rows) < MIN_ROWS:
    die(f"only {len(rows)} rows (minimum {MIN_ROWS}) — the manifest has been "
        f"gutted, and a short table reads as a small project rather than an "
        f"unmeasured one")

fails = []
for r in rows:
    name = f"{r.get('area','?')}/{r.get('subsystem','?')}"
    for field, legal in (("implementation", IMPL), ("production_proof", TRI),
                         ("mutation", QUAD), ("docs", DOCS)):
        if r.get(field) not in legal:
            fails.append(f"{name}: {field}={r.get(field)!r} is not one of "
                         f"{sorted(legal)}")

    ev = r.get("evidence") or []
    claims = [f for f in ("production_proof", "mutation") if r.get(f) == "yes"]
    if claims and not ev:
        fails.append(f"{name}: claims {'+'.join(claims)}=yes but cites no "
                     f"evidence — an unbacked claim is the thing this file "
                     f"exists to prevent")
    for e in ev:
        path = os.path.join(ROOT, e.split("::")[0])
        if not os.path.exists(path):
            fails.append(f"{name}: cites `{e}`, which does not exist")

    # Existence is not integration. A subsystem is not complete until the
    # production path is PROVEN to use it.
    if r.get("implementation") == "complete" and r.get("production_proof") != "yes":
        fails.append(f"{name}: implementation=complete with "
                     f"production_proof={r.get('production_proof')!r} — "
                     f"existence is not integration")

# COVERAGE. The rows were authored by hand from what someone happened to be
# working on, so the manifest's own blind spots are invisible in it — an
# absent row reads exactly like an absent problem. Derive which crates are
# represented (from the evidence paths, not from a restated list) and require
# every crate to be either covered or EXPLICITLY excused with a reason.
#
# This is the manifest applying its own rule to itself: existence of a table is
# not coverage by the table.
crates = sorted(d.name for d in os.scandir(os.path.join(ROOT, "crates"))
                if d.is_dir())
covered = set()
for r in rows:
    for e in r.get("evidence") or []:
        parts = e.split("/")
        if len(parts) > 1 and parts[0] == "crates":
            covered.add(parts[1])
excused = art.get("crates_excused") or {}
uncovered = [c for c in crates if c not in covered and c not in excused]
for c in uncovered:
    fails.append(f"crate `{c}` has no row and no entry in `crates_excused` — "
                 f"its state is unrecorded, which in a completeness manifest "
                 f"reads as 'nothing to report'")
for c in excused:
    if c not in crates:
        fails.append(f"`{c}` is excused but is not a crate — the excuse list "
                     f"has drifted")
    elif not str(excused[c]).strip():
        fails.append(f"crate `{c}` is excused with an empty reason")

if fails:
    for f in fails:
        print(f"  UNBACKED: {f}")
    print(f"\n{len(fails)} completeness claim(s) are not backed by evidence")
    sys.exit(1)

# ── render ──────────────────────────────────────────────────────────────────
MARK = {"complete": "✓", "yes": "✓", "partial": "~", "staged": "staged",
        "absent": "✗", "no": "✗", "unknown": "?"}
lines = [
    "# Axon completeness",
    "",
    "**GENERATED from `AXON-COMPLETENESS.json` by `scripts/completeness.py`. "
    "Do not edit this file — edit the JSON.**",
    "",
    "`production proof` asks whether the PRODUCTION path is proven to use a "
    "thing, not whether it exists and has tests. Those come apart constantly, "
    "and every row claiming a proof cites evidence the generator checks for.",
    "",
    "A `?` is an honest answer and is more useful than a guess: it marks work "
    "whose state nobody has established.",
    "",
]
for area in dict.fromkeys(r["area"] for r in rows):
    lines += [f"## {area}", "",
              "| subsystem | impl | production proof | mutation | docs | gap |",
              "|---|---|---|---|---|---|"]
    for r in (x for x in rows if x["area"] == area):
        lines.append(
            f"| {r['subsystem']} | {MARK[r['implementation']]} | "
            f"{MARK[r['production_proof']]} | {MARK[r['mutation']]} | "
            f"{MARK[r['docs']]} | {r.get('gap','')} |")
    lines.append("")

tot = len(rows)
proven = sum(1 for r in rows if r["production_proof"] == "yes")
unknown = sum(1 for r in rows if r["production_proof"] == "unknown")
lines += [
    "## Where the gaps are",
    "",
    f"{len(covered)} of {len(crates)} crates are represented "
    f"({len(excused)} explicitly excused). "
    f"{proven} of {tot} subsystems have a production proof; {unknown} are "
    f"UNKNOWN — not failing, unestablished, which is the state most worth "
    f"acting on.",
    "",
    "Deliberately NOT summarised as a single percentage. One number averages "
    "over the axis that matters: a parser at 100% and import-graph approval at "
    "0% do not combine into anything a reader can act on.",
    "",
]
open(OUT, "w").write("\n".join(lines) + "\n")
print(f"AXON-COMPLETENESS.md generated: {tot} rows, {proven} with a production "
      f"proof, {unknown} unknown")
