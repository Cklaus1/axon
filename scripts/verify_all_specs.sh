#!/usr/bin/env bash
# verify_all_specs.sh — the OUTER LOOP's mechanized half (governance/EXECUTION_MODEL.md §4).
#
# Static lint (default, cheap, no builds):
#   * duplicate spec numbers across governance/specs/ (the R21/R22/R23 dual-numbering class)
#   * spec-meta parsing: status-claim vs prose **Status:** mismatch
#   * non-Draft specs with `evidence: none` (a claim without a re-runnable evidence
#     pointer is not a valid status)
#   * evidence pointers naming scripts that do not exist
#   * dangling depends-on/blocks/supersedes/conflicts-with edges (unknown spec IDs)
#   * specs with no spec-meta block at all -> reported as pre-convention (the backfill worklist)
#
# Evidence re-run (expensive, opt-in):
#   verify_all_specs.sh --run R28          # re-run one spec's evidence command
#   verify_all_specs.sh --run all          # re-run every runnable evidence command
#   (only commands that are a single existing scripts/*.sh are auto-run; anything else is
#    listed for manual re-run — this prototype does not eval arbitrary prose)
#
# Exit codes: 0 = clean (pre-convention specs alone don't fail it), 1 = at least one finding.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SPECS_DIR="$ROOT/governance/specs"
RUN_TARGET=""
[ "${1:-}" = "--run" ] && RUN_TARGET="${2:-all}"

findings=0
finding() { echo "FINDING: $*"; findings=$((findings + 1)); }
note()    { echo "  note:  $*"; }

# ── 1. Duplicate spec numbers (filename-based, so it works pre-convention too) ──────────
# KNOWN legacy collisions, documented in REQUIREMENTS.md ("dual-numbered") and EXECUTION_MODEL.md:
#   R21/R22/R23/R24/R25 — two independent tracks each claimed the number (vertical targets vs
#   safety stack). R18 — one spec + its companion docs (slice0 spike/results, jury packet).
# These warn (reconcile when convenient); any NEW collision is a hard finding.
KNOWN_DUAL="R18 R21 R22 R23 R24 R25"
echo "== [1] duplicate spec numbers =="
dups=$(ls "$SPECS_DIR" | grep -E '^R[0-9]+[a-z]?-' | sed -E 's/^(R[0-9]+[a-z]?)-.*/\1/' \
       | sort | uniq -d)
new_dup=0
if [ -n "$dups" ]; then
    for n in $dups; do
        files=$(ls "$SPECS_DIR" | grep -E "^${n}-" | tr '\n' ' ')
        if echo " $KNOWN_DUAL " | grep -q " $n "; then
            echo "  warn: known legacy dual-numbering $n: $files"
        else
            finding "NEW spec-number collision $n: $files"
            new_dup=1
        fi
    done
fi
[ "$new_dup" -eq 0 ] && echo "  ok: no NEW duplicate numbers"

# ── 2. Per-spec spec-meta lint ─────────────────────────────────────────────────────────
echo "== [2] spec-meta lint =="
all_ids=$(ls "$SPECS_DIR" | grep -E '\.md$' | sed 's/\.md$//')
pre_convention=()
declare -A META_EVIDENCE

get_key() { # get_key <file> <key>
    awk -v k="$2" '/^```spec-meta/{f=1;next} /^```/{f=0} f && $0 ~ "^"k":"{sub("^"k": *","");print;exit}' "$1"
}

for f in "$SPECS_DIR"/R*.md; do
    base=$(basename "$f" .md)
    if ! grep -q '^```spec-meta' "$f"; then
        pre_convention+=("$base")
        continue
    fi
    id=$(get_key "$f" id)
    claim=$(get_key "$f" status-claim)
    evidence=$(get_key "$f" evidence)

    # id matches filename
    [ "$id" = "$base" ] || finding "$base: spec-meta id '$id' != filename '$base'"

    # status-claim vs prose **Status:** first word
    prose=$(grep -m1 -E '^\*\*Status:\*\*' "$f" | sed -E 's/^\*\*Status:\*\* *//; s/^[✅📝📋🟡🔧🚧 ]*//' | awk '{print $1}')
    claim_word=$(echo "$claim" | awk '{print $1}')
    if [ -n "$prose" ] && [ "$claim_word" != "$prose" ]; then
        finding "$base: status-claim '$claim_word' != prose Status '$prose'"
    fi

    # non-Draft requires evidence
    if [ "$claim_word" != "Draft" ] && { [ -z "$evidence" ] || [ "${evidence%% *}" = "none" ]; }; then
        finding "$base: status-claim '$claim_word' but evidence is 'none' — not a valid status"
    fi

    # evidence scripts must exist (check every scripts/*.sh token; brace-globs expanded).
    # A line containing "planned" is exempt from existence checks — a Draft spec may
    # pre-name its future gate (encouraged: gates are invented at spec time).
    if ! echo "$evidence" | grep -qi 'planned'; then
        for tok in $(echo "$evidence" | grep -oE 'scripts/[A-Za-z0-9_{}.,…-]+\.sh' | grep -v '…'); do
            expanded=$(eval echo "$ROOT/$tok" 2>/dev/null || echo "$ROOT/$tok")
            for path in $expanded; do
                [ -f "$path" ] || finding "$base: evidence names missing script ${path#$ROOT/}"
            done
        done
    fi
    META_EVIDENCE[$base]="$evidence"

    # dangling edges: every R<n>-slug token in edge keys must be a real spec file
    for key in depends-on blocks supersedes conflicts-with related; do
        val=$(get_key "$f" "$key")
        for ref in $(echo "$val" | grep -oE 'R[0-9]+[a-z]?-[a-z0-9-]+'); do
            echo "$all_ids" | grep -qx "$ref" || finding "$base: $key references unknown spec '$ref'"
        done
    done
    echo "  parsed: $base (claim: $claim_word)"
done

if [ ${#pre_convention[@]} -gt 0 ]; then
    echo "  pre-convention (no spec-meta yet; add the block on next edit): ${#pre_convention[@]} specs"
fi

# ── 3. Evidence re-run (opt-in) ────────────────────────────────────────────────────────
if [ -n "$RUN_TARGET" ]; then
    echo "== [3] evidence re-run (target: $RUN_TARGET) =="
    for base in "${!META_EVIDENCE[@]}"; do
        case "$RUN_TARGET" in
            all) ;;
            *) echo "$base" | grep -qE "^${RUN_TARGET}(-|$)" || continue ;;
        esac
        ev="${META_EVIDENCE[$base]}"
        [ "${ev%% *}" = "none" ] && { note "$base: no evidence to run"; continue; }
        # only auto-run a single plain scripts/*.sh pointer; anything richer is manual
        script=$(echo "$ev" | grep -oE '^scripts/[A-Za-z0-9_.-]+\.sh' || true)
        if [ -n "$script" ] && [ -f "$ROOT/$script" ]; then
            echo "  running $script for $base ..."
            if bash "$ROOT/$script"; then
                echo "  PASS: $base ($script)"
            else
                finding "$base: evidence command $script FAILED — status-claim may be stale-optimistic"
            fi
        else
            note "$base: evidence not a single script pointer — re-run manually: $ev"
        fi
    done
fi

echo "=="
if [ "$findings" -eq 0 ]; then
    echo "verify_all_specs: CLEAN (${#pre_convention[@]} pre-convention specs remain)"
    exit 0
else
    echo "verify_all_specs: $findings finding(s) — each requires a 'truth:' correction commit (BUILD_PROTOCOL.md, outer loop)"
    exit 1
fi
