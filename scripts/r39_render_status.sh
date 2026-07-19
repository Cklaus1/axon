#!/usr/bin/env bash
# r39_render_status.sh — R39 Slice 4 (governance/specs/R39-typed-execution-graph.md §6 slice 4,
# re-scoped 2026-07-20): renders GOVERNANCE_STATUS.md, a purely-generated structured-facts index,
# from the typed store — id/status-claim/prose-status/match, plus the most recent verify-run
# result if a Slice-3 sidecar is given.
#
# This does NOT replace SESSION_STATUS.md. SESSION_STATUS.md is hand-written narrative (decision
# rationale, investigation findings) that a mechanically-generated file cannot and should not try
# to reproduce; GOVERNANCE_STATUS.md exists so every STRUCTURED status claim is reachable from one
# place, sourced only from the store — never hand-typed, so it cannot silently drift from what the
# store actually says.
#
# Usage: scripts/r39_render_status.sh [STORE_JSONL] [--verify-results PATH] [--out PATH]
#   STORE_JSONL      defaults to governance/state/specs.jsonl
#   --verify-results  optional; a Slice-3 --record-jsonl sidecar. If given, each spec's most
#                     recent record (last occurrence in the file, since it's append-only) is
#                     rendered. Omit if no verify-run history is being tracked yet.
#   --out             defaults to governance/state/GOVERNANCE_STATUS.md
#
# Exit codes: 0 = rendered, 2 = usage/missing-store/missing-jq error.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
command -v jq >/dev/null 2>&1 || { echo "r39_render_status: requires jq" >&2; exit 2; }

STORE=""
VERIFY_RESULTS=""
OUT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --verify-results) VERIFY_RESULTS="${2:?--verify-results requires a PATH}"; shift 2 ;;
        --out) OUT="${2:?--out requires a PATH}"; shift 2 ;;
        *) [ -z "$STORE" ] && { STORE="$1"; shift; } || { echo "r39_render_status: unknown arg '$1'" >&2; exit 2; } ;;
    esac
done
STORE="${STORE:-$ROOT/governance/state/specs.jsonl}"
OUT="${OUT:-$ROOT/governance/state/GOVERNANCE_STATUS.md}"
[ -f "$STORE" ] || { echo "r39_render_status: store not found: $STORE (run verify_all_specs.sh --export-jsonl first)" >&2; exit 2; }

mkdir -p "$(dirname "$OUT")"

# Most-recent verify-run record per spec, if a sidecar was given: jq's `unique_by` keeps the
# FIRST match per key, so reverse the stream first to make that the LAST (most recent) record.
verify_lookup() { # verify_lookup <spec-base>
    [ -n "$VERIFY_RESULTS" ] && [ -f "$VERIFY_RESULTS" ] || { echo ""; return; }
    jq -r --arg spec "$1" '
        select(.spec == $spec) | "\(.result) @ \(.ts) (commit \(.commit))"
    ' "$VERIFY_RESULTS" | tail -1
}

{
    echo "# Governance Status (generated — do not hand-edit)"
    echo ""
    echo "Generated $(date -u +%Y-%m-%dT%H:%M:%SZ) from \`${STORE#$ROOT/}\`."
    echo "Regenerate: \`bash scripts/verify_all_specs.sh --export-jsonl governance/state/specs.jsonl"
    echo "&& bash scripts/r39_render_status.sh\`"
    echo ""
    echo "This is a structured-facts index (id / status-claim / prose-status / evidence match /"
    echo "last verify-run result) sourced ONLY from the typed store — it cannot silently drift"
    echo "from what the store says, because nothing here is hand-typed. It does NOT replace"
    echo "\`SESSION_STATUS.md\`, which keeps recording hand-written narrative (decision rationale,"
    echo "investigation findings) exactly as before — see"
    echo "\`governance/specs/R39-typed-execution-graph.md\` §6 slice 4 for why the two don't merge."
    echo ""

    spec_count=$(jq -s '[.[] | select(.pre_convention == false)] | length' "$STORE")
    echo "## Specs with spec-meta ($spec_count)"
    echo ""
    echo "| Spec | Status claim | Prose status | Match | Last verify-run |"
    echo "|---|---|---|---|---|"
    while IFS=$'\t' read -r base status_claim prose_word; do
        claim_word=$(echo "$status_claim" | awk '{print $1}')
        if [ -n "$prose_word" ] && [ "$claim_word" != "$prose_word" ]; then
            match="⚠ MISMATCH"
        else
            match="ok"
        fi
        last_verify=$(verify_lookup "$base")
        [ -z "$last_verify" ] && last_verify="—"
        echo "| \`$base\` | $status_claim | $prose_word | $match | $last_verify |"
    done < <(jq -r 'select(.pre_convention == false) | [.file, (.status_claim // ""), (.prose_status_word // "")] | @tsv' "$STORE" | sort)

    echo ""
    pre_count=$(jq -s '[.[] | select(.pre_convention == true)] | length' "$STORE")
    echo "## Pre-convention specs — no spec-meta yet ($pre_count)"
    echo ""
    echo "Backfilled on next real edit per \`EXECUTION_MODEL.md\` §3 (not a mass mechanical pass)."
    echo ""
    jq -r 'select(.pre_convention == true) | "- `\(.file)` (\(.prose_status_word // "no prose Status: line"))"' "$STORE" | sort
} > "$OUT"

echo "rendered $(wc -l < "$OUT" | tr -d ' ') lines to ${OUT#$ROOT/}"
