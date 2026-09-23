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
import json, os, subprocess, sys

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
#
# `not-applicable` IS NOT `unreachable`. That conflation was made in this file's
# own data and is the reason the set grew: AXON_NATIVE_TRACE was marked N/A on
# wasm because a `native::gfx` program does not LINK there — but it does not
# link because of the open i64->i32 ABI retarget gap (R7 §12), so fixing an
# unrelated defect would silently turn a "permanent" N/A into a live, untested
# control. A classification a neighbouring bugfix can invalidate was never a
# classification about design.
#
#   not-applicable          the control CANNOT semantically apply to this
#                           engine, by architecture. Browser wasm has no
#                           process environment, so an env-var runtime control
#                           can never reach it — no future fix changes that.
#   unsupported             the engine deliberately does not implement the
#                           underlying capability. A decision, not a defect.
#   blocked-by-open-defect  the control would become meaningful if a KNOWN
#                           open defect were fixed; the program that would
#                           exercise it currently cannot run. Names the defect.
#
# The last two exist so the unknown/N-A counts cannot fall because an upstream
# defect made a feature unreachable. Ten honest unknowns beat zero containing
# one false N/A.
CONTROL_STATES = {"enforced", "explicitly-refused", "not-applicable",
                  "unsupported", "blocked-by-open-defect",
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

# ── FALSE GREENS ────────────────────────────────────────────────────────────
# A false green is a check that REPORTED SUCCESS while not verifying what it
# claims. It is strictly worse than an `unknown`: an unknown advertises itself,
# a false green is believed. Every entry must be REPRODUCED — a suspicion does
# not belong here, because a ledger padded with speculation stops being read.
FG_STATUS = {"open", "fixed"}
FG_SEVERITY = {"security", "correctness", "reporting"}
_fg = art.get("false_greens") or []
for f in _fg:
    fid = f.get("id", "?")
    for field in ("where", "claimed", "reality", "reproduced", "fix"):
        if not f.get(field):
            fails.append(f"false green `{fid}`: missing `{field}` — an entry "
                         f"that cannot say what was claimed, what was true, and "
                         f"how it was reproduced is itself an unsupported claim")
    if f.get("status") not in FG_STATUS:
        fails.append(f"false green `{fid}`: status {f.get('status')!r} not in "
                     f"{sorted(FG_STATUS)}")
    if f.get("severity") not in FG_SEVERITY:
        fails.append(f"false green `{fid}`: severity {f.get('severity')!r} not "
                     f"in {sorted(FG_SEVERITY)}")
    _c = f.get("commit", "")
    # A placeholder is not a citation. "pending" is truthy, so a bare
    # presence check accepted it — the same absent-vs-verified shape this
    # ledger exists to record, in the ledger's own gate.
    if f.get("status") == "fixed" and (
        not _c or not all(ch in "0123456789abcdef" for ch in _c) or len(_c) < 7
    ):
        fails.append(f"false green `{fid}`: marked fixed with no commit — "
                     f"'fixed' without a citation is the same unsupported claim "
                     f"this ledger exists to record")
    w = (f.get("where") or "").split(" :: ")[0]
    if w and not os.path.exists(os.path.join(ROOT, w)):
        fails.append(f"false green `{fid}`: cites `{w}`, which does not exist")
    # A `fixed` entry's citation must lead to the fix: the commit's own diff
    # must touch the file the entry claims to fix. FG-021 shipped citing a
    # commit whose diff was two unrelated files, while the fix it described
    # (a 15-call-site change) had landed in a DIFFERENT commit whose message
    # happened to describe the same work — found only by checking every
    # citation by hand, which is what this codifies so it need not be redone
    # by hand again.
    if w and f.get("status") == "fixed" and _c and len(_c) >= 7 and all(
        ch in "0123456789abcdef" for ch in _c
    ):
        _r = subprocess.run(
            ["git", "show", "--name-only", "--format=", _c],
            cwd=ROOT, capture_output=True, text=True,
        )
        if _r.returncode != 0:
            fails.append(f"false green `{fid}`: commit `{_c}` does not exist "
                         f"in this repo")
        else:
            _touched = set(_r.stdout.split())
            if not any(w == t or t.startswith(w + "/") for t in _touched):
                fails.append(f"false green `{fid}`: cites commit `{_c}`, whose "
                             f"diff does not touch `{w}` — the citation does "
                             f"not lead to the fix")

# THE RELEASE CRITERION. An OPEN false green blocks any claim of completeness —
# harder than the unknown count, and deliberately so: unknowns shrink by doing
# work, false greens shrink only by admitting the check was lying.
_fg_open = [f for f in _fg if f.get("status") == "open"]

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

# ── `not-applicable` must mean INAPPLICABLE, not merely unreachable ─────────
#
# The distinction is enforced mechanically because it was got wrong here by
# hand: AXON_NATIVE_TRACE was marked N/A on wasm because a `native::gfx`
# program fails to LINK there — which is a consequence of the open i64->i32 ABI
# retarget gap, not a statement about design. Fixing that gap would have turned
# a settled row into a live, untested control with nothing to notice.
#
# Two rules:
#   1. a `blocked-by-open-defect` state must NAME the blocking defect, or it is
#      just `unknown` wearing a more confident label;
#   2. a `not-applicable` state must not justify itself with the VOCABULARY OF
#      FAILURE. "does not link", "undefined symbol", "signature mismatch",
#      "not implemented" describe something broken or unbuilt, and a row that
#      reaches for those words is describing unreachability. Architecture reads
#      differently: "has no process environment", "there is no such layer".
_UNREACHABLE_WORDS = ("does not link", "fails to link", "undefined symbol",
                      "signature mismatch", "not implemented", "unimplemented",
                      "cannot link", "link fails")
for _c in (art.get("controls") or []):
    _eng = _c.get("engines") or {}
    _why = (_c.get("open") or "") + " " + (_c.get("why") or "")
    _low = _why.lower()
    for _e, _st in _eng.items():
        if _st == "blocked-by-open-defect" and "blocking defect" not in _low:
            fails.append(
                f"control `{_c.get('name')}` marks `{_e}` blocked-by-open-defect "
                f"without naming the blocking defect — say WHICH defect, or the "
                f"state is `unknown` with a confident label")
        if _st == "not-applicable":
            _hit = [w for w in _UNREACHABLE_WORDS if w in _low]
            if _hit:
                fails.append(
                    f"control `{_c.get('name')}` marks `{_e}` not-applicable but "
                    f"justifies it with {_hit!r} — that describes something "
                    f"UNREACHABLE, not inapplicable. If a fix elsewhere would "
                    f"make this control meaningful, it is blocked-by-open-defect")

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
    # `blocked-by-open-defect` counts as NOT SETTLED, deliberately. It means
    # nobody knows what that engine does with the control and there is a
    # standing reason nobody can find out — which is nearer to `unknown` than
    # to a decision. Excluding it would let the headline number improve by
    # reclassifying a row away from `unknown`, which is the exact optimism this
    # ledger exists to resist.
    _bad = lambda c: any(v in ("unknown", "silently-ignored",
                               "blocked-by-open-defect")
                         for v in (c.get("engines") or {}).values())
    lines += ["", "## Cross-engine control matrix", "",
              "Per-engine support for each runtime/security control. States are a "
              "closed set: `enforced` / `explicitly-refused` / `not-applicable` / "
              "`unsupported` / `blocked-by-open-defect` / "
              "`unknown` / `silently-ignored`. `not-applicable` means the control "
              "cannot apply BY DESIGN; a control that is merely unreachable "
              "because some other defect is open is `blocked-by-open-defect`, so "
              "that fixing that defect cannot quietly convert a settled row into "
              "an untested one. The defect state is named on purpose "
              "— a control an engine neither honours nor refuses reads as "
              "system-wide when it is not, and that shape produced every divergence "
              "found so far.", "",
              f"**{len(_c)} controls tracked; "
              f"{sum(1 for c in _c for v in (c.get('engines') or {}).values() if v in ('unknown','silently-ignored'))} "
              f"engine states unsettled (unknown / silently-ignored / blocked-by-open-defect).**", "",
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

# ── Render the false-green ledger ───────────────────────────────────────────
_fgl = art.get("false_greens") or []
if _fgl:
    _open = [f for f in _fgl if f.get("status") == "open"]
    lines += ["", "## False greens", "",
              "A false green is a check, test, or matrix cell that REPORTED "
              "SUCCESS while the thing it claims to verify was not verified. It "
              "is strictly worse than an `unknown`: an unknown advertises itself "
              "and invites work; a false green discourages the work and is "
              "believed. Every entry below was REPRODUCED.", "",
              "The doctrine they all violate: **success must carry evidence; "
              "failure may never synthesize success.**", "",
              f"**{len(_open)} OPEN, {len(_fgl) - len(_open)} fixed.** An open "
              "false green blocks any completeness claim — a harder criterion "
              "than the unknown count, and deliberately so: unknowns shrink by "
              "doing work, false greens shrink only by admitting a check was "
              "lying. The two must never be traded against each other, because "
              "relabelling an unknown to improve its count manufactures a false "
              "green.", ""]
    for f in sorted(_fgl, key=lambda f: (f.get("status") != "open", f.get("id", ""))):
        mark = "**OPEN**" if f.get("status") == "open" else "fixed"
        lines += [f"### {f.get('id')} — {f.get('where')} ({f.get('severity')}, {mark})", "",
                  f"- **Claimed:** {f.get('claimed')}",
                  f"- **Reality:** {f.get('reality')}",
                  f"- **Reproduced:** {f.get('reproduced')}",
                  f"- **Fix:** {f.get('fix')}"
                  + (f" (`{f.get('commit')}`)" if f.get("commit") else ""), ""]

open(OUT, "w").write("\n".join(lines) + "\n")
# The control tally shares this headline on purpose. Reporting "0 unknown"
# for rows while six controls carry an unknown engine is the same
# absent-vs-passed collapse the controls section exists to catch, committed
# by the tool that catches it.
_ctl = art.get("controls") or []
_ctl_unknown = sum(1 for c in _ctl
                   for v in (c.get("engines") or {}).values()
                   if v in ("unknown", "silently-ignored",
                            "blocked-by-open-defect"))
print(f"AXON-COMPLETENESS.md generated: {tot} rows, {proven} with a production "
      f"proof, {unknown} unknown")
print(f"controls: {len(_ctl)} tracked, {_ctl_unknown} engine states unsettled"
      f" (unknown / silently-ignored / blocked-by-open-defect)"
      + ("" if not _ctl_unknown else " — NOT a clean bill"))
# Reported BESIDE the unknown count, never traded against it: relabelling an
# unknown as enforced to improve one number manufactures the other.
_fgo = len([f for f in (art.get("false_greens") or []) if f.get("status") == "open"])
_fgf = len(art.get("false_greens") or []) - _fgo
print(f"false greens: {_fgo} OPEN, {_fgf} fixed"
      + ("" if not _fgo else " — an open false green blocks any completeness claim"))
