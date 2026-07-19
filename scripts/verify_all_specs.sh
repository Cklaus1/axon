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
# R39 Slice 1 (typed export, opt-in, additive — no change to the checks above):
#   verify_all_specs.sh --export-jsonl governance/state/specs.jsonl
#   Writes ONE JSON object per spec file (schema axon-gov-spec/1) to the given path, using
#   the EXACT SAME per-spec parse pass that already computes the checks above — not a second,
#   independently-drifting parser (R39's own motivation: this repo has been bitten before by
#   two mechanisms disagreeing about the same fact). Every governance/specs/*.md file gets
#   exactly one line, whether it carries spec-meta or not (pre-convention specs get a minimal
#   record with "pre_convention":true, matching this script's own pre-convention classification
#   above — not silently dropped). KNOWN LIMITATION carried over unchanged from `get_key`: a
#   spec-meta field is captured from its FIRST LINE ONLY, so a multi-line value (e.g. a
#   `reserves:` field with wrapped continuation lines) is truncated in the export exactly as
#   it already is in this script's own single-line prose comparisons above — not a regression
#   introduced by the export, a pre-existing property of the shared extraction inherited as-is.
#
# Exit codes: 0 = clean (pre-convention specs alone don't fail it), 1 = at least one finding.
#
# --specs-dir DIR overrides governance/specs/ — for R39 Slice 2's gate, which points both this
# script and the ported jq validator at a synthetic fixture dir to prove the port finds the same
# real bugs, not just to confirm both agree on the already-clean tree (a synthetic-fixture
# regression test, not a mass renumbering or new default).
#
# R39 Slice 3: --run TARGET --record-jsonl PATH appends one JSONL record (schema
# axon-gov-verify/1) per evidence command actually re-run, to a SEPARATE sidecar file from
# --export-jsonl's specs.jsonl. Kept separate deliberately: specs.jsonl is a regeneratable index
# derivable purely from markdown (R39 spec §12 Q2); a verify-run record (exit code, timestamp,
# commit) is NOT derivable from markdown alone — it's evidence of an action actually taken, so it
# gets its own append-only log rather than being folded into (and silently mutating the meaning
# of) the "pure function of the tree" export.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SPECS_DIR="$ROOT/governance/specs"
RUN_TARGET=""
EXPORT_JSONL=""
RECORD_JSONL=""
while [ $# -gt 0 ]; do
    case "$1" in
        --run) RUN_TARGET="${2:-all}"; shift 2 ;;
        --export-jsonl) EXPORT_JSONL="${2:?--export-jsonl requires a PATH}"; shift 2 ;;
        --specs-dir) SPECS_DIR="${2:?--specs-dir requires a PATH}"; shift 2 ;;
        --record-jsonl) RECORD_JSONL="${2:?--record-jsonl requires a PATH}"; shift 2 ;;
        *) echo "verify_all_specs: unknown arg '$1'" >&2; exit 2 ;;
    esac
done
if [ -n "$RECORD_JSONL" ]; then
    [ -n "$RUN_TARGET" ] || { echo "verify_all_specs: --record-jsonl requires --run" >&2; exit 2; }
    command -v jq >/dev/null 2>&1 || { echo "verify_all_specs: --record-jsonl requires jq" >&2; exit 2; }
    mkdir -p "$(dirname "$RECORD_JSONL")"
fi
if [ -n "$EXPORT_JSONL" ]; then
    command -v jq >/dev/null 2>&1 || { echo "verify_all_specs: --export-jsonl requires jq" >&2; exit 2; }
    mkdir -p "$(dirname "$EXPORT_JSONL")"
    : > "$EXPORT_JSONL"
fi

findings=0
finding() { echo "FINDING: $*"; findings=$((findings + 1)); }
note()    { echo "  note:  $*"; }

# ── 1. Duplicate spec numbers (filename-based, so it works pre-convention too) ──────────
# KNOWN legacy collisions, documented in REQUIREMENTS.md ("dual-numbered") and EXECUTION_MODEL.md:
#   R21/R22/R23/R24/R25 — two independent tracks each claimed the number (vertical targets vs
#   safety stack). R18 — one spec + its companion docs (slice0 spike/results, jury packet).
# These warn (reconcile when convenient); any NEW collision is a hard finding.
# Reconciled 2026-07-18 (SESSION_STATUS.md): each entry below was read in full, not just
# filename-matched. Two distinct shapes both trip the same prefix-collision check:
#   - R21/R22/R23/R24/R25: TRUE collisions — two independent, unrelated specs from two
#     parallel work-tracks each claimed the same next-available number (axon-os-supervisor
#     vs decimal; domain-modules vs intent-approve-gateway; ebpf-target vs proof-certificates;
#     defended-approval-boundary vs tee-target; information-flow-monitor vs zephyr-target).
#     No content overlap or conflict found in either topic of any pair. Mitigation is the
#     full-slug convention (EXECUTION_MODEL.md §3), not renumbering — renumbering an
#     already-shipped/drafted spec would churn every existing cross-reference for no benefit
#     the full-slug rule doesn't already provide.
#   - R18: NOT a true collision — one initiative (R18-provenance-ledger.md is the only file
#     with a Spec ID/spec-meta) with three supporting artifacts (slice0-spike, slice0-results,
#     buyer-jury-packet) that share the R18- filename prefix but claim no independent spec
#     identity. Kept in this list only because the prefix-match check can't tell the
#     difference; do not read its presence here as "R18 has a numbering conflict."
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
# Pure-bash membership set for the dangling-edge check below, instead of forking a `grep -qx`
# per referenced id (hundreds of forks across ~90 specs x 5 edge keys). Under heavy host load
# this host has been observed at load-average ~50 on 32 cores — a subprocess fork can transiently
# fail, which `set -uo pipefail` (no `-e`) turns into a silent false "unknown spec" finding, not a
# script abort. Confirmed by hand: every id a flaky run cited as "unknown" in fact exists. This is
# the same gate-flakes-under-contention class as the wasm_browser/persistent_learner flakes, but
# hitting a governance validator is worse (a phantom FINDING invites a bogus correction commit),
# so it's worth removing the fork pressure rather than just noting the flake.
declare -A ALL_IDS_SET
while IFS= read -r _id; do [ -n "$_id" ] && ALL_IDS_SET["$_id"]=1; done <<< "$all_ids"
pre_convention=()
declare -A META_EVIDENCE

get_key() { # get_key <file> <key>
    awk -v k="$2" '/^```spec-meta/{f=1;next} /^```/{f=0} f && $0 ~ "^"k":"{sub("^"k": *","");print;exit}' "$1"
}

# R39 Slice 1: newline-separated R-id tokens referenced by an edge key (possibly empty).
# Reuses get_key + the exact same extraction regex the dangling-edge check below uses —
# not a second parser.
edge_tokens() { # edge_tokens <file> <key>
    get_key "$1" "$2" | grep -oE 'R[0-9]+[a-z]?-[a-z0-9-]+' || true
}

for f in "$SPECS_DIR"/R*.md; do
    base=$(basename "$f" .md)
    if ! grep -q '^```spec-meta' "$f"; then
        pre_convention+=("$base")
        if [ -n "$EXPORT_JSONL" ]; then
            prose_only=$(grep -m1 -E '^\*\*Status:\*\*' "$f" \
                | sed -E 's/^\*\*Status:\*\* *//; s/^[✅📝📋🟡🔧🚧 ]*//' | awk '{print $1}')
            jq -n -c \
                --arg schema "axon-gov-spec/1" \
                --arg file "$base" \
                --arg prose_status_word "$prose_only" \
                '{schema: $schema, file: $file, pre_convention: true,
                  id: null, status_claim: null, prose_status_word: $prose_status_word,
                  depends_on: [], blocks: [], blocked_by: "", supersedes: [],
                  related: [], conflicts_with: [], reserves: "", evidence: ""}' \
                >> "$EXPORT_JSONL"
        fi
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
            [ -n "${ALL_IDS_SET[$ref]:-}" ] || finding "$base: $key references unknown spec '$ref'"
        done
    done

    if [ -n "$EXPORT_JSONL" ]; then
        jq -n -c \
            --arg schema "axon-gov-spec/1" \
            --arg file "$base" \
            --arg id "$id" \
            --arg status_claim "$claim" \
            --arg prose_status_word "$prose" \
            --arg depends_on_raw "$(edge_tokens "$f" depends-on)" \
            --arg blocks_raw "$(edge_tokens "$f" blocks)" \
            --arg blocked_by "$(get_key "$f" blocked-by)" \
            --arg supersedes_raw "$(edge_tokens "$f" supersedes)" \
            --arg related_raw "$(edge_tokens "$f" related)" \
            --arg conflicts_with_raw "$(edge_tokens "$f" conflicts-with)" \
            --arg reserves "$(get_key "$f" reserves)" \
            --arg evidence "$evidence" \
            '{schema: $schema, file: $file, pre_convention: false,
              id: $id, status_claim: $status_claim, prose_status_word: $prose_status_word,
              depends_on: ($depends_on_raw | split("\n") | map(select(length > 0))),
              blocks: ($blocks_raw | split("\n") | map(select(length > 0))),
              blocked_by: $blocked_by,
              supersedes: ($supersedes_raw | split("\n") | map(select(length > 0))),
              related: ($related_raw | split("\n") | map(select(length > 0))),
              conflicts_with: ($conflicts_with_raw | split("\n") | map(select(length > 0))),
              reserves: $reserves, evidence: $evidence}' \
            >> "$EXPORT_JSONL"
    fi

    echo "  parsed: $base (claim: $claim_word)"
done

if [ -n "$EXPORT_JSONL" ]; then
    echo "  exported $(wc -l < "$EXPORT_JSONL" | tr -d ' ') spec records to ${EXPORT_JSONL#$ROOT/}"
fi

if [ ${#pre_convention[@]} -gt 0 ]; then
    echo "  pre-convention (no spec-meta yet; add the block on next edit): ${#pre_convention[@]} specs"
fi

# ── 3. Evidence re-run (opt-in) ────────────────────────────────────────────────────────
if [ -n "$RUN_TARGET" ]; then
    echo "== [3] evidence re-run (target: $RUN_TARGET) =="
    RUN_COMMIT=$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)
    record_verify() { # record_verify <base> <script> <result> <exit_code>
        [ -n "$RECORD_JSONL" ] || return 0
        local ts; ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        jq -n -c \
            --arg schema "axon-gov-verify/1" \
            --arg spec "$1" --arg command "$2" --arg result "$3" \
            --argjson exit_code "$4" --arg ts "$ts" --arg commit "$RUN_COMMIT" \
            '{schema: $schema, spec: $spec, command: $command, result: $result,
              exit_code: $exit_code, ts: $ts, commit: $commit}' \
            >> "$RECORD_JSONL"
    }
    for base in "${!META_EVIDENCE[@]}"; do
        case "$RUN_TARGET" in
            all) ;;
            *) echo "$base" | grep -qE "^${RUN_TARGET}(-|$)" || continue ;;
        esac
        ev="${META_EVIDENCE[$base]}"
        if [ "${ev%% *}" = "none" ]; then
            note "$base: no evidence to run"
            record_verify "$base" "none" "SKIP_NO_EVIDENCE" -1
            continue
        fi
        # only auto-run a single plain scripts/*.sh pointer; anything richer is manual
        script=$(echo "$ev" | grep -oE '^scripts/[A-Za-z0-9_.-]+\.sh' || true)
        if [ -n "$script" ] && [ -f "$ROOT/$script" ]; then
            echo "  running $script for $base ..."
            if bash "$ROOT/$script"; then
                echo "  PASS: $base ($script)"
                record_verify "$base" "$script" "PASS" 0
            else
                rc=$?
                finding "$base: evidence command $script FAILED — status-claim may be stale-optimistic"
                record_verify "$base" "$script" "FAIL" "$rc"
            fi
        else
            note "$base: evidence not a single script pointer — re-run manually: $ev"
            record_verify "$base" "$ev" "SKIP_NOT_A_SCRIPT_POINTER" -1
        fi
    done
    if [ -n "$RECORD_JSONL" ]; then
        echo "  recorded $(wc -l < "$RECORD_JSONL" | tr -d ' ') verify-run record(s) to ${RECORD_JSONL#$ROOT/}"
    fi
fi

echo "=="
if [ "$findings" -eq 0 ]; then
    echo "verify_all_specs: CLEAN (${#pre_convention[@]} pre-convention specs remain)"
    exit 0
else
    echo "verify_all_specs: $findings finding(s) — each requires a 'truth:' correction commit (BUILD_PROTOCOL.md, outer loop)"
    exit 1
fi
