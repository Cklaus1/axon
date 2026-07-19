#!/usr/bin/env bash
# r39_slice5_dag_check.sh — R39 Slice 5 (governance/specs/R39-typed-execution-graph.md §6 slice 5):
# "Validate depends-on/blocks/blocked-by edges form a DAG (no cycles) and that every blocked-by
# naming an open question (R36 §12 Q1) is checked against whether that question has actually been
# marked resolved in the spec's own §12 section."
#
# Two checks, both reading the R39 Slice 1 typed store (governance/state/specs.jsonl) — cycle
# detection needs no markdown at all (depends_on/blocks are already typed arrays in the store);
# the open-question staleness check DOES read the target spec's markdown directly, because
# "is Q<n> marked resolved in prose" is not information any landed slice has typed yet (a
# deliberate, narrow exception — this is genuinely new information, not a second parser
# duplicating an existing extraction).
#
# 1. DAG CYCLE DETECTION: builds one directed "must-happen-before" graph from every spec's
#    depends_on (T depends_on-target implies T after target: edge target -> T) and blocks
#    (S blocks-target implies target after S: edge S -> target) arrays, then DFS-detects cycles
#    (3-color: white/gray/black).
# 2. BLOCKED-BY OPEN-QUESTION STALENESS: for every non-empty `blocked_by` string matching
#    `R<id> §<N> Q<k>`, locates spec R<id>'s own `§<N>` section in its markdown, isolates the
#    Q<k> bullet's text (up to the next `**Q` bullet or section end), and checks it for the word
#    "resolved" (case-insensitive). A blocked-by naming an ALREADY-RESOLVED question is a finding
#    (stale — the blocking spec should have been un-blocked); naming a genuinely still-open
#    question is reported, not a finding.
#
# Usage: scripts/r39_slice5_dag_check.sh [STORE_JSONL] [--specs-dir DIR]
#   --specs-dir overrides governance/specs/ (where blocked-by target markdown is read from) — for
#   the gate's synthetic-fixture checks, matching the same override verify_all_specs.sh already
#   supports.
# Exit codes: 0 = clean (no cycles, no stale blocked-by), 1 = at least one finding, 2 = usage error.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SPECS_DIR="$ROOT/governance/specs"
STORE=""
while [ $# -gt 0 ]; do
    case "$1" in
        --specs-dir) SPECS_DIR="${2:?--specs-dir requires a PATH}"; shift 2 ;;
        *) [ -z "$STORE" ] && { STORE="$1"; shift; } || { echo "r39_slice5_dag_check: unknown arg '$1'" >&2; exit 2; } ;;
    esac
done
STORE="${STORE:-$ROOT/governance/state/specs.jsonl}"
command -v jq >/dev/null 2>&1 || { echo "r39_slice5_dag_check: requires jq" >&2; exit 2; }
[ -f "$STORE" ] || { echo "r39_slice5_dag_check: store not found: $STORE (run verify_all_specs.sh --export-jsonl first)" >&2; exit 2; }

findings=0
finding() { echo "FINDING: $*"; findings=$((findings + 1)); }

echo "== [1] DAG cycle detection (depends-on / blocks) =="

declare -A ADJ   # ADJ[node]="succ1 succ2 ..." (edges: node must happen before each successor)
declare -A NODES

add_edge() { # add_edge <before> <after>
    NODES["$1"]=1; NODES["$2"]=1
    ADJ["$1"]="${ADJ[$1]:-} $2"
}

while IFS=$'\t' read -r base key ref; do
    case "$key" in
        depends_on) add_edge "$ref" "$base" ;;   # base depends on ref -> ref before base
        blocks)     add_edge "$base" "$ref" ;;   # base blocks ref -> base before ref
    esac
done < <(jq -r '
  select(.pre_convention == false) | .file as $b |
  (.depends_on[]? | [$b, "depends_on", .]),
  (.blocks[]?     | [$b, "blocks", .])
  | @tsv' "$STORE")

# 3-color DFS cycle detection: 0=white(unvisited) 1=gray(in progress) 2=black(done)
declare -A COLOR
CYCLE_PATH=""
dfs() {
    local node="$1" path="$2"
    COLOR["$node"]=1
    local succs="${ADJ[$node]:-}"
    local s
    for s in $succs; do
        case "${COLOR[$s]:-0}" in
            1) CYCLE_PATH="$path -> $s"; return 1 ;;
            0) dfs "$s" "$path -> $s" || return 1 ;;
        esac
    done
    COLOR["$node"]=2
    return 0
}

cycle_found=0
for n in "${!NODES[@]}"; do
    [ "${COLOR[$n]:-0}" = 0 ] || continue
    if ! dfs "$n" "$n"; then
        finding "dependency cycle detected: $CYCLE_PATH"
        cycle_found=1
        break
    fi
done
[ "$cycle_found" -eq 0 ] && echo "  ok: no cycles across $(echo "${!NODES[@]}" | tr ' ' '\n' | grep -c .) nodes"

echo
echo "== [2] blocked-by open-question staleness =="
section_body() { # section_body <file> <section_num>
    awk -v n="$2" '
        BEGIN{insec=0}
        /^#+[ \t]+[0-9]+\./{
            match($0, /^#+[ \t]+([0-9]+)\./, m)
            if (insec) exit
            if (m[1] == n) insec=1
        }
        insec{print}
    ' "$1"
}
question_bullet() { # question_bullet <section_text> <qlabel, e.g. Q1>
    local section="$1" qlabel="$2" qnum="${2#Q}" result
    result=$(awk -v q="$qlabel" '
        BEGIN{inq=0}
        /\*\*Q[0-9]+/{
            match($0, /\*\*(Q[0-9]+)/, m)
            if (inq) exit
            if (m[1] == q) inq=1
        }
        inq{print}
    ' <<< "$section")
    if [ -z "$result" ]; then
        # Fallback: this spec's §N doesn't follow the "**Q<n>**" bold-label convention (e.g.
        # R37/R38 use plain "1. / 2. / 3." numbered items) — map Q<n> to the n'th numbered item.
        result=$(awk -v n="$qnum" '
            BEGIN{inq=0}
            /^[0-9]+\.[ \t]/{
                match($0, /^([0-9]+)\./, m)
                if (inq) exit
                if (m[1] == n) inq=1
            }
            inq{print}
        ' <<< "$section")
    fi
    echo "$result"
}

any_blocked_by=0
while IFS=$'\t' read -r base blocked_by; do
    [ -n "$blocked_by" ] || continue
    any_blocked_by=1
    if [[ "$blocked_by" =~ (R[0-9]+[a-z]?)[[:space:]]*§([0-9]+)[[:space:]]*(Q[0-9]+) ]]; then
        target_id="${BASH_REMATCH[1]}"
        sec_num="${BASH_REMATCH[2]}"
        qlabel="${BASH_REMATCH[3]}"
        target_file=$(ls "$SPECS_DIR" | grep -E "^${target_id}-" | head -1)
        if [ -z "$target_file" ]; then
            finding "$base: blocked-by references '$target_id §$sec_num $qlabel' but no spec file matches '$target_id-*'"
            continue
        fi
        body=$(section_body "$SPECS_DIR/$target_file" "$sec_num")
        if [ -z "$body" ]; then
            finding "$base: blocked-by references '$target_id §$sec_num' but $target_file has no §$sec_num section"
            continue
        fi
        qtext=$(question_bullet "$body" "$qlabel")
        if [ -z "$qtext" ]; then
            finding "$base: blocked-by references '$target_id §$sec_num $qlabel' but that question doesn't exist in $target_file"
            continue
        fi
        # word-boundary match on "resolved" that excludes "unresolved" (a plain substring grep
        # would false-positive on "Unresolved, deliberately" — found via R40's real Q1 text).
        if echo "$qtext" | grep -qiE '(^|[^a-zA-Z])resolved\b'; then
            finding "$base: blocked-by names $target_id §$sec_num $qlabel, but that question is marked RESOLVED — stale blocked-by, should be lifted"
        else
            echo "  ok: $base still-blocking on $target_id §$sec_num $qlabel (unresolved, as expected)"
        fi
    else
        echo "  note: $base: blocked-by '$blocked_by' doesn't reference a spec §N Q<k> — not this check's shape, skipped"
    fi
done < <(jq -r 'select(.pre_convention == false and .blocked_by != "" and .blocked_by != "none") | [.file, .blocked_by] | @tsv' "$STORE")
[ "$any_blocked_by" -eq 1 ] || echo "  (no non-empty blocked-by fields in the store)"

echo
echo "=="
if [ "$findings" -eq 0 ]; then
    echo "r39_slice5_dag_check: CLEAN"
    exit 0
else
    echo "r39_slice5_dag_check: $findings finding(s)"
    exit 1
fi
