#!/usr/bin/env bash
# R39 Slice 1 acceptance gate — typed export of governance/specs/ (governance/EXECUTION_MODEL.md's
# task-DAG/knowledge-graph conventions), per governance/specs/R39-typed-execution-graph.md §6.
#
# Gate (spec's own words): "parses 100% of specs that already carry spec-meta ... without
# error; specs without it are cleanly recorded as pre-convention ... not a regression [vs
# scripts/verify_all_specs.sh's existing behavior]."
#
# Exit 0 = all checks green. Any failure prints which check failed and exits non-zero.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FAIL=0

pass() { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; FAIL=1; }

echo "=== R39 Slice 1 acceptance gate ==="

JSONL="$REPO_ROOT/governance/state/specs.jsonl"
SPECS_DIR="$REPO_ROOT/governance/specs"

command -v jq >/dev/null 2>&1 || { echo "  ✗ jq is required (not found)"; exit 1; }

# ── 1. verify_all_specs.sh --export-jsonl runs clean and produces the file ──────────────
echo ""
echo "1. Export runs clean (no regression to the existing checks)"

VAS_OUT=$(bash "$SCRIPT_DIR/verify_all_specs.sh" --export-jsonl "$JSONL" 2>&1)
VAS_RC=$?
if [ "$VAS_RC" -eq 0 ]; then
    pass "verify_all_specs.sh --export-jsonl exits 0 (same CLEAN result as the plain run)"
else
    fail "verify_all_specs.sh --export-jsonl exited $VAS_RC — regressed the existing checks:
$VAS_OUT"
fi

[ -f "$JSONL" ] && pass "$JSONL was created" || fail "$JSONL was not created"

# ── 2. Every spec file gets exactly one line — no silent drop ───────────────────────────
echo ""
echo "2. Completeness: one JSONL line per governance/specs/*.md file"

SPEC_COUNT=$(ls "$SPECS_DIR"/R*.md 2>/dev/null | wc -l | tr -d ' ')
LINE_COUNT=$(wc -l < "$JSONL" | tr -d ' ')
if [ "$SPEC_COUNT" = "$LINE_COUNT" ]; then
    pass "$LINE_COUNT JSONL lines == $SPEC_COUNT spec files (no spec silently dropped)"
else
    fail "$LINE_COUNT JSONL lines != $SPEC_COUNT spec files — a spec was dropped or duplicated"
fi

# ── 3. Every line is well-formed JSON (schema axon-gov-spec/1) ──────────────────────────
echo ""
echo "3. Every line parses as valid JSON, schema axon-gov-spec/1"

BAD_LINES=0
BAD_SCHEMA=0
while IFS= read -r line; do
    [ -z "$line" ] && continue
    if ! echo "$line" | jq -e . >/dev/null 2>&1; then
        BAD_LINES=$((BAD_LINES + 1))
        continue
    fi
    schema=$(echo "$line" | jq -r '.schema')
    [ "$schema" = "axon-gov-spec/1" ] || BAD_SCHEMA=$((BAD_SCHEMA + 1))
done < "$JSONL"

if [ "$BAD_LINES" -eq 0 ]; then
    pass "every line is valid JSON"
else
    fail "$BAD_LINES line(s) are not valid JSON"
fi
if [ "$BAD_SCHEMA" -eq 0 ]; then
    pass "every line carries schema axon-gov-spec/1"
else
    fail "$BAD_SCHEMA line(s) have the wrong/missing schema tag"
fi

# ── 4. pre_convention accounting matches verify_all_specs.sh's own count ────────────────
echo ""
echo "4. pre_convention count matches verify_all_specs.sh's own report"

VAS_PRECONV=$(echo "$VAS_OUT" | grep -oE '[0-9]+ pre-convention specs remain' | grep -oE '^[0-9]+')
JSONL_PRECONV=$(jq -s '[.[] | select(.pre_convention == true)] | length' "$JSONL")
if [ -n "$VAS_PRECONV" ] && [ "$VAS_PRECONV" = "$JSONL_PRECONV" ]; then
    pass "pre_convention:true count ($JSONL_PRECONV) matches verify_all_specs.sh's own report ($VAS_PRECONV)"
else
    fail "pre_convention count mismatch: JSONL says $JSONL_PRECONV, verify_all_specs.sh says '$VAS_PRECONV'"
fi

# ── 5. Every non-pre-convention record has a non-null id; every pre-convention one doesn't ─
echo ""
echo "5. id presence matches pre_convention classification (no silent misclassification)"

BAD_META=$(jq -s '[.[] | select(.pre_convention == false and .id == null)] | length' "$JSONL")
BAD_PRE=$(jq -s '[.[] | select(.pre_convention == true and .id != null)] | length' "$JSONL")
if [ "$BAD_META" = "0" ]; then
    pass "every spec-meta record (pre_convention:false) has a non-null id"
else
    fail "$BAD_META spec-meta record(s) have a null id"
fi
if [ "$BAD_PRE" = "0" ]; then
    pass "every pre-convention record correctly has a null id"
else
    fail "$BAD_PRE pre-convention record(s) unexpectedly carry a non-null id"
fi

# ── 6. Spot-check: R33's own record round-trips its known depends-on/related edges ──────
echo ""
echo "6. Spot-check: a known spec's edges round-trip correctly"

R33_DEPENDS=$(jq -s -r '.[] | select(.file == "R33-cross-vm-safety-quorum") | .depends_on[0]' "$JSONL")
if [ "$R33_DEPENDS" = "R31-extended-tcb-attestation" ]; then
    pass "R33's depends_on correctly extracted: R31-extended-tcb-attestation"
else
    fail "R33's depends_on extraction wrong: got '$R33_DEPENDS'"
fi

# ── Final result ──────────────────────────────────────────────────────────────
echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "=== R39 Slice 1 acceptance gate: ALL CHECKS PASSED ==="
    exit 0
else
    echo "=== R39 Slice 1 acceptance gate: FAILED (see above) ==="
    exit 1
fi
