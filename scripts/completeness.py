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

    # ENGINE DIVERGENCE. A security-sensitive semantic enforced by one engine
    # and not another is the D-002 shape: the interpreter enforced the ambient
    # effect ceiling and a natively built binary ignored it. A row may record
    # that state, but it may not also call itself complete.
    eng = r.get("engines")
    if isinstance(eng, dict):
        legal = {"enforced", "not-enforced", "refused", "n/a", "unknown"}
        bad = {k: v for k, v in eng.items() if v not in legal}
        if bad:
            fails.append(f"{name}: engines {bad} not in {sorted(legal)}")
        if "enforced" in eng.values() and "not-enforced" in eng.values() \
                and r.get("implementation") == "complete":
            fails.append(
                f"{name}: engines DISAGREE ({eng}) while implementation=complete "
                f"— one engine enforcing is not system-wide support")

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

    # Existence is not integration — but they are SEPARATE AXES, and the
    # point of having two columns is to be able to say "fully built, wired to
    # nothing". This rule originally forbade complete+no, which blocked the
    # most informative honest row in the file: axon-signal is 5,389 lines, 59
    # tests, and reached by zero execution roots. Forcing those two columns to
    # agree conflates the thing the manifest exists to separate.
    #
    # What is NOT allowed is complete + UNKNOWN: claiming a subsystem is
    # finished while nobody has checked whether anything calls it.
    if r.get("implementation") == "complete" and r.get("production_proof") == "unknown":
        fails.append(f"{name}: implementation=complete with an UNKNOWN "
                     f"production proof — nobody established whether the "
                     f"production path uses it")
    if (r.get("implementation") == "complete" and r.get("production_proof") == "no"
            and not (r.get("gap") or "").strip()):
        fails.append(f"{name}: built but not wired, with no gap explaining it")

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
# Derive coverage from the row's NAME, not from where its evidence happens to
# live. Inferring it from evidence paths was wrong for exactly the rows that
# matter most: a crate's strongest evidence is its PRODUCTION CALL SITE, which
# by definition lives in another crate — so assessing axon-domain properly
# (citing its call site in axon-core) made it read as UNCOVERED.
#
# Join on identity, not on a path that correlates with it.
covered = set()
for r in rows:
    sub = r.get("subsystem", "")
    if sub.startswith("crate "):
        covered.add(sub[len("crate "):].strip())
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

# ── CONTROL MATRIX ──────────────────────────────────────────────────────────
# A runtime/security control must say what EVERY engine does with it. The
# defect state is nameable (`silently-ignored`) because every divergence found
# so far had that shape: an engine that neither honours a control nor refuses
# it, so the control reads as system-wide when it is not.
CONTROL_STATES = {"enforced", "explicitly-refused", "not-applicable",
                  "unknown", "silently-ignored"}
ENGINES = {"interpreter", "native", "wasm", "guest"}
# A control row may be keyed by EXECUTION ENGINE or by SERVING MODE, declared
# per row. The two are not the same question and must not be conflated:
# Embedded / LocalSidecar / RemoteService are deployment topologies for a
# Reflex backend (CX-35), not additional compiler engines. Keeping one closed
# state set but two key sets lets the serving matrix live here — the only
# acceptable home, since D-007 records that this repository already carries
# four governance registries — without implying that a mode row says anything
# about whether the interpreter, native, wasm or guest engine supports a
# control. A passing mode row is NOT engine support; engine rows keep their own
# keys, their own validation, and their own evidence.
MODES = {"embedded", "local-sidecar", "remote-service"}
AXES = {"engine": ENGINES, "mode": MODES}
for c in art.get("controls") or []:
    cn = c.get("name", "?")
    axis = c.get("axis", "engine")
    if axis not in AXES:
        fails.append(f"control `{cn}`: axis {axis!r} is not one of "
                     f"{sorted(AXES)}")
        continue
    keys = AXES[axis]
    eng = c.get("engines") or {}
    missing = keys - set(eng)
    if missing:
        fails.append(f"control `{cn}`: no state for {sorted(missing)} — a "
                     f"control must say what EVERY {axis} does with it")
    extra = set(eng) - keys
    if extra:
        fails.append(f"control `{cn}`: axis={axis} but carries "
                     f"{sorted(extra)}, which belong to a different axis — "
                     f"a mode row must not assert engine support, or vice versa")
    for k, v in eng.items():
        if v not in CONTROL_STATES:
            fails.append(f"control `{cn}`: {axis} `{k}` = {v!r}, not in "
                         f"{sorted(CONTROL_STATES)}")
    if c.get("status") == "resolved":
        bad = {k: v for k, v in eng.items()
               if v in ("unknown", "silently-ignored")}
        if bad:
            fails.append(f"control `{cn}`: status=resolved while {bad} — a "
                         f"control is not resolved while a {axis} is unknown "
                         f"or silently ignoring it")
    ev = c.get("evidence")
    if ev and not os.path.exists(os.path.join(ROOT, ev.split("::")[0])):
        fails.append(f"control `{cn}`: cites `{ev}`, which does not exist")

# Every registry variable must have a control row. Without this the matrix
# could look complete by simply omitting the awkward vars — the same omission
# direction the env registry itself gates in both directions, and for the same
# reason. NOTE the known limit, recorded as D-013: the registry is built from a
# literal scan that cannot see a var read through the host seam, so full
# coverage OF THE REGISTRY is not full coverage of the vars the code reads.
import re as _re
_regsrc = os.path.join(ROOT, "crates/axon-core/src/env_registry.rs")
if os.path.exists(_regsrc):
    _reg = set(_re.findall(r'"(AXON_[A-Z0-9_]+)"', open(_regsrc).read()))
    _named = {c.get("name") for c in (art.get("controls") or [])}
    for _v in sorted(_reg - _named):
        fails.append(f"env registry declares `{_v}` but the control matrix has "
                     f"no row for it — a control matrix that may omit rows "
                     f"cannot be read as coverage")

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
# ── Render the control matrix ───────────────────────────────────────────────
# A matrix only the gate can read is half-built. The interesting rows lead:
# anything an engine ignores silently or has never been assessed on.
_c = art.get("controls") or []
if _c:
    _bad = lambda c: any(v in ("unknown", "silently-ignored")
                         for v in (c.get("engines") or {}).values())
    lines += ["", "## Cross-engine control matrix", "",
              "Per-engine support for each runtime/security control. States are a "
              "closed set: `enforced` / `explicitly-refused` / `not-applicable` / "
              "`unknown` / `silently-ignored`. The defect state is named on purpose "
              "— a control an engine neither honours nor refuses reads as "
              "system-wide when it is not, and that shape produced every divergence "
              "found so far.", "",
              f"**{len(_c)} controls tracked; "
              f"{sum(1 for c in _c for v in (c.get('engines') or {}).values() if v in ('unknown','silently-ignored'))} "
              f"engine states unknown or silently-ignored.**", "",
              "| control | category | interp | native | wasm | guest | status |",
              "|---|---|---|---|---|---|---|"]
    _sym = {"enforced": "✓", "explicitly-refused": "refused",
            "not-applicable": "n/a", "unknown": "**?**",
            "silently-ignored": "**IGNORED**"}
    for c in sorted(_c, key=lambda c: (not _bad(c), c["name"])):
        e = c.get("engines") or {}
        cells = " | ".join(_sym.get(e.get(k, "unknown"), "?")
                           for k in ("interpreter", "native", "wasm", "guest"))
        lines.append(f"| `{c['name']}` | {c.get('category','')} | {cells} | "
                     f"{c.get('status','')} |")
    _open = [c for c in _c if c.get("open")]
    if _open:
        lines += ["", "### Open control divergences", ""]
        for c in _open:
            lines += [f"- **`{c['name']}`** — {c['open']}"]

open(OUT, "w").write("\n".join(lines) + "\n")
# The control tally shares this headline on purpose. Reporting "0 unknown"
# for rows while six controls carry an unknown engine is the same
# absent-vs-passed collapse the controls section exists to catch, committed
# by the tool that catches it.
_ctl = art.get("controls") or []
_ctl_unknown = sum(1 for c in _ctl
                   for v in (c.get("engines") or {}).values()
                   if v in ("unknown", "silently-ignored"))
print(f"AXON-COMPLETENESS.md generated: {tot} rows, {proven} with a production "
      f"proof, {unknown} unknown")
print(f"controls: {len(_ctl)} tracked, {_ctl_unknown} engine states unknown or "
      f"silently-ignored"
      + ("" if not _ctl_unknown else " — NOT a clean bill"))
