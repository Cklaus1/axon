#!/usr/bin/env bash
# reference_gate.sh — AXON_REFERENCE.md must match the binary, both directions.
#
# WHY THIS EXISTS.
#
# `claims_gate.sh` checks that every verb CLAUDE.md NAMES exists. It deliberately
# does not check the reverse, and the omission direction is where the drift went:
# the binary had 24 verbs while CLAUDE.md named 13, and `axon session` — six
# slices of work — appeared in no user-facing document at all. Nothing failed,
# because nothing was looking.
#
# A doc that can omit silently is one an agent reads as complete. So this gate
# regenerates the reference from the compiler's own tables and diffs it against
# the checked-in file. A new verb, builtin or attribute fails CI until it is
# regenerated — which is a one-command fix, and the point is that it is not
# optional.
#
# Exit 0 = in sync. Exit 1 = drifted (the diff is printed).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
AXON="${AXON:-$ROOT/target/debug/axon}"
DOC="$ROOT/AXON_REFERENCE.md"

if [ ! -x "$AXON" ]; then
    echo "reference_gate: \$AXON not executable at $AXON" >&2
    echo "hint: cargo build -p axon-core --bin axon" >&2
    exit 1
fi
if [ ! -f "$DOC" ]; then
    echo "reference_gate: $DOC is missing — regenerate with:" >&2
    echo "  $AXON reference > AXON_REFERENCE.md" >&2
    exit 1
fi

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT
"$AXON" reference > "$TMP" || { echo "reference_gate: \`axon reference\` failed" >&2; exit 1; }

# Sanity: the generator must actually have produced a reference. An empty or
# truncated file would otherwise "match" a correspondingly broken checked-in one.
lines=$(wc -l < "$TMP")
if [ "$lines" -lt 50 ]; then
    echo "reference_gate: generated reference is only $lines lines — generator broken" >&2
    exit 1
fi

if diff -u "$DOC" "$TMP" > "$TMP.diff" 2>&1; then
    # Quote the generated file's OWN summary rather than re-deriving counts with
    # a grep. A first draft did re-derive them and reported 334 builtins beside a
    # header saying 338 — the pattern missed some names. Two different numbers in
    # one report is how a gate stops being believed.
    echo "reference_gate: in sync — $(grep -m1 '^The complete surface' "$DOC" | sed 's/^The complete surface of this build — //; s/\.$//')"
    exit 0
fi

echo "reference_gate: AXON_REFERENCE.md has DRIFTED from the binary." >&2
echo >&2
sed -n '1,60p' "$TMP.diff" >&2
echo >&2
echo "Fix: $AXON reference > AXON_REFERENCE.md" >&2
exit 1
