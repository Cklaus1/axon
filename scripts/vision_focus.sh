#!/usr/bin/env bash
# vision_focus.sh — keep VISION.md a SHORT, FOCUSED, legible north-star doc.
#
# VISION.md is meant to be readable by a non-engineer stakeholder on first pass,
# then deepen into a precise engineer section. Over time such docs bloat and
# re-accumulate jargon. This harness — same idiom as the *_parity.sh checks,
# wired into gate.sh — enforces the focus mechanically and deterministically:
#
#   1. length ceiling            — the doc can't bloat past a word cap
#   2. required sections present — the load-bearing headings all exist
#   3. the three pillars named   — Proof / Containment / Goal-direction
#   4. no undefined jargon in    — every jargon term in the PLAIN half (above the
#      the plain half              "For engineers" divider) must be glossed there
#                                  (followed by a parenthetical) or kept out of it
#   5. internal links valid      — referenced repo docs actually exist
#   6. plain lead is plain       — the opening section has no code fence and no
#                                  raw jargon (a non-expert can read it cold)
#
# Pure text checks over VISION.md — no Axon build, so it's fast and can't flake.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DOC="VISION.md"
fail=0
note() { printf '  %s %s\n' "$1" "$2"; }

if [ ! -f "$DOC" ]; then
  echo "vision_focus: $DOC not found"
  exit 1
fi

# The divider that splits the plain (stakeholder) half from the engineer half.
DIVIDER="## For engineers"

# Jargon that must never appear UN-glossed in the plain half. (Lowercased match.)
JARGON=(
  "refinement type"
  "row-polymorphic"
  "smt"
  "tcb"
  "i-2"
  "anti-laundering"
  "effect row"
  "hindley-milner"
  "monomorphization"
)

# Required sections (substring match, case-insensitive).
REQUIRED=(
  "the problem"
  "what axon is"
  "what makes it an"      # "What makes it an AI language"
  "what success looks like"
  "## for engineers"
)

# ── 1. length ceiling ─────────────────────────────────────────────────────────
# Cap covers the whole doc. The plain (stakeholder) half is ~800 words; the
# remainder is the deliberate "For engineers" appendix + the showcase status
# table. 1300 keeps the doc to roughly two screens and blocks bloat without
# gutting the reference table.
WORDS=$(wc -w < "$DOC")
CAP=1300
if [ "$WORDS" -le "$CAP" ]; then
  note OK "length: $WORDS words (<= $CAP)"
else
  note FAIL "length: $WORDS words exceeds the $CAP-word cap — trim it"
  fail=1
fi

# ── 2. required sections present ───────────────────────────────────────────────
lc_doc="$(tr '[:upper:]' '[:lower:]' < "$DOC")"
for sec in "${REQUIRED[@]}"; do
  if printf '%s' "$lc_doc" | grep -qF "$sec"; then
    note OK "section present: \"$sec\""
  else
    note FAIL "missing required section: \"$sec\""
    fail=1
  fi
done

# ── 3. the three pillars named ────────────────────────────────────────────────
for pillar in "proof" "containment"; do
  if printf '%s' "$lc_doc" | grep -qF "$pillar"; then
    note OK "pillar named: $pillar"
  else
    note FAIL "pillar not named: $pillar"
    fail=1
  fi
done
# Goal-direction may be phrased "goal-directed" / "goal-direction" / "goal-directedness".
if printf '%s' "$lc_doc" | grep -qE "goal-direct"; then
  note OK "pillar named: goal-direction"
else
  note FAIL "pillar not named: goal-direction"
  fail=1
fi

# ── 4 + 6. split on the divider; check the PLAIN half ─────────────────────────
if ! grep -qF "$DIVIDER" "$DOC"; then
  note FAIL "missing the \"$DIVIDER\" divider that separates plain from engineer prose"
  fail=1
  PLAIN="$lc_doc"   # whole doc treated as plain if no divider
else
  # Everything before the divider line is the plain half.
  PLAIN="$(awk -v d="$DIVIDER" 'index($0,d){exit} {print}' "$DOC" | tr '[:upper:]' '[:lower:]')"
fi

# 4. no UN-glossed jargon in the plain half. A term is allowed if every
#    occurrence is immediately followed by a "(" (an inline gloss).
for term in "${JARGON[@]}"; do
  # count raw occurrences and occurrences immediately followed by " ("
  raw=$(printf '%s' "$PLAIN" | grep -oF "$term" | wc -l | tr -d ' ')
  if [ "$raw" -eq 0 ]; then
    continue
  fi
  glossed=$(printf '%s' "$PLAIN" | grep -oF "$term (" | wc -l | tr -d ' ')
  if [ "$raw" -eq "$glossed" ]; then
    note OK "jargon glossed in plain half: \"$term\" ($raw use(s), each followed by a gloss)"
  else
    note FAIL "un-glossed jargon in plain half: \"$term\" ($raw use(s), only $glossed glossed) — gloss it or move it below \"$DIVIDER\""
    fail=1
  fi
done

# 6. plain LEAD readability: the first section (up to the 2nd H2) must have no
#    code fence and no raw jargon at all (the absolute first thing a reader sees).
LEAD="$(awk 'BEGIN{h=0} /^## /{h++; if(h==2) exit} {print}' "$DOC" | tr '[:upper:]' '[:lower:]')"
if printf '%s' "$LEAD" | grep -qF '```'; then
  note FAIL "plain lead contains a code fence — keep the opening prose-only"
  fail=1
else
  note OK "plain lead has no code fence"
fi
lead_jargon=0
for term in "${JARGON[@]}"; do
  if printf '%s' "$LEAD" | grep -qF "$term"; then
    note FAIL "plain lead contains jargon: \"$term\" — the opening must be jargon-free"
    lead_jargon=1
    fail=1
  fi
done
[ "$lead_jargon" -eq 0 ] && note OK "plain lead is jargon-free"

# ── 5. internal links valid ───────────────────────────────────────────────────
for ref in STATUS.md ROADMAP.md CLAUDE.md; do
  if grep -qF "$ref" "$DOC"; then
    if [ -f "$ref" ]; then
      note OK "link valid: $ref"
    else
      note FAIL "VISION.md references $ref but it does not exist"
      fail=1
    fi
  fi
done

echo
if [ "$fail" -ne 0 ]; then
  echo "vision_focus: FAIL — VISION.md drifted from short/focused/legible"
  exit 1
fi
echo "vision_focus: PASS — VISION.md is short, focused, and legible"
