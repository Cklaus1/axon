#!/usr/bin/env bash
# r39_slice2_validate.sh — R39 Slice 2: the SAME checks verify_all_specs.sh's markdown/awk pass
# performs, ported to run against the typed store (governance/state/specs.jsonl, R39 Slice 1's
# `--export-jsonl` output) instead of re-parsing governance/specs/*.md directly.
# (governance/specs/R39-typed-execution-graph.md §6 slice 2)
#
# This is deliberately a PORT, not a rewrite: every field it reads (id, status_claim,
# prose_status_word, evidence, depends_on/blocks/supersedes/conflicts_with/related) is exactly
# what verify_all_specs.sh's own extraction pass already wrote into the store — this script does
# not re-derive anything from the .md files. The gate (scripts/r39_slice2_gate.sh) proves this
# port finds the same real bugs the bash/awk version finds, on both the real tree and a synthetic
# fixture with deliberately injected findings.
#
# Checks ported (same as verify_all_specs.sh's [1] and [2] sections):
#   * duplicate spec numbers (KNOWN_DUAL legacy collisions warn, any NEW collision is a finding)
#   * spec-meta id must match filename
#   * status-claim vs prose **Status:** first-word mismatch
#   * non-Draft claim requires a real evidence pointer (not empty / "none")
#   * evidence pointers naming scripts/*.sh must exist on disk (unless the line says "planned")
#   * dangling depends-on/blocks/supersedes/conflicts-with/related edges (unknown spec ids)
#     — blocked-by is intentionally excluded from this check, matching verify_all_specs.sh (it
#     may legitimately name an open question like "R36 §12 Q1", not always a spec id)
#
# Usage: scripts/r39_slice2_validate.sh [STORE_JSONL]
#   STORE_JSONL defaults to governance/state/specs.jsonl. Regenerate it first via
#   `scripts/verify_all_specs.sh --export-jsonl governance/state/specs.jsonl` if stale/missing —
#   this script never writes to it, only reads.
#
# Exit codes: 0 = clean, 1 = at least one finding, 2 = usage/missing-store/missing-jq error.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STORE="${1:-$ROOT/governance/state/specs.jsonl}"
command -v jq >/dev/null 2>&1 || { echo "r39_slice2_validate: requires jq" >&2; exit 2; }
[ -f "$STORE" ] || { echo "r39_slice2_validate: store not found: $STORE (run verify_all_specs.sh --export-jsonl first)" >&2; exit 2; }

findings=0
finding() { echo "FINDING: $*"; findings=$((findings + 1)); }

# Same known legacy collisions verify_all_specs.sh carries — see that script's own comment for
# the full reconciliation record (SESSION_STATUS.md, EXECUTION_MODEL.md §3).
KNOWN_DUAL="R18 R21 R22 R23 R24 R25"

all_files=$(jq -r '.file' "$STORE" | sort -u)
declare -A ALL_FILES_SET
while IFS= read -r _f; do [ -n "$_f" ] && ALL_FILES_SET["$_f"]=1; done <<< "$all_files"

echo "== [1] duplicate spec numbers (from typed store) =="
dups=$(echo "$all_files" | grep -E '^R[0-9]+[a-z]?-' | sed -E 's/^(R[0-9]+[a-z]?)-.*/\1/' | sort | uniq -d)
new_dup=0
if [ -n "$dups" ]; then
    for n in $dups; do
        files=$(echo "$all_files" | grep -E "^${n}-" | tr '\n' ' ')
        if echo " $KNOWN_DUAL " | grep -q " $n "; then
            echo "  warn: known legacy dual-numbering $n: $files"
        else
            finding "NEW spec-number collision $n: $files"
            new_dup=1
        fi
    done
fi
[ "$new_dup" -eq 0 ] && echo "  ok: no NEW duplicate numbers"

echo "== [2] spec-meta lint (from typed store) =="
pre_conv_count=0
while IFS=$'\t' read -r base pre_conv id status_claim prose_word evidence; do
    if [ "$pre_conv" = "true" ]; then
        pre_conv_count=$((pre_conv_count + 1))
        continue
    fi

    [ "$id" = "$base" ] || finding "$base: spec-meta id '$id' != filename '$base'"

    claim_word=$(echo "$status_claim" | awk '{print $1}')
    if [ -n "$prose_word" ] && [ "$claim_word" != "$prose_word" ]; then
        finding "$base: status-claim '$claim_word' != prose Status '$prose_word'"
    fi

    if [ "$claim_word" != "Draft" ] && { [ -z "$evidence" ] || [ "${evidence%% *}" = "none" ]; }; then
        finding "$base: status-claim '$claim_word' but evidence is 'none' — not a valid status"
    fi

    if ! echo "$evidence" | grep -qi 'planned'; then
        for tok in $(echo "$evidence" | grep -oE 'scripts/[A-Za-z0-9_{}.,…-]+\.sh' | grep -v '…'); do
            expanded=$(eval echo "$ROOT/$tok" 2>/dev/null || echo "$ROOT/$tok")
            for path in $expanded; do
                [ -f "$path" ] || finding "$base: evidence names missing script ${path#$ROOT/}"
            done
        done
    fi

    echo "  parsed: $base (claim: $claim_word)"
done < <(jq -r 'select(.pre_convention == false) | [.file, (.pre_convention|tostring), (.id // ""), (.status_claim // ""), (.prose_status_word // ""), (.evidence // "")] | @tsv' "$STORE")

# pre-convention count via a separate pass (kept out of the read-loop above so a `continue`
# there doesn't need to also fall through into edge-checking further down)
pre_conv_count=$(jq -r 'select(.pre_convention == true) | .file' "$STORE" | wc -l | tr -d ' ')

echo "== [2b] dangling edges (depends-on/blocks/supersedes/conflicts-with/related) =="
edge_findings_before=$findings
while IFS=$'\t' read -r base key ref; do
    [ -n "${ALL_FILES_SET[$ref]:-}" ] || finding "$base: $key references unknown spec '$ref'"
done < <(jq -r '
  .file as $b |
  (.depends_on[]? | [$b, "depends-on", .]),
  (.blocks[]?     | [$b, "blocks", .]),
  (.supersedes[]? | [$b, "supersedes", .]),
  (.conflicts_with[]? | [$b, "conflicts-with", .]),
  (.related[]?    | [$b, "related", .])
  | @tsv' "$STORE")
[ "$findings" -eq "$edge_findings_before" ] && echo "  ok: no dangling edges"

if [ "$pre_conv_count" -gt 0 ]; then
    echo "  pre-convention (no spec-meta yet; add the block on next edit): $pre_conv_count specs"
fi

echo "=="
if [ "$findings" -eq 0 ]; then
    echo "r39_slice2_validate: CLEAN ($pre_conv_count pre-convention specs remain)"
    exit 0
else
    echo "r39_slice2_validate: $findings finding(s) — each requires a 'truth:' correction commit (BUILD_PROTOCOL.md, outer loop)"
    exit 1
fi
