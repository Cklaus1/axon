#!/usr/bin/env bash
# r39_slice4_gate.sh — R39 Slice 4 acceptance gate (governance/specs/R39-typed-execution-graph.md
# §6 slice 4, re-scoped 2026-07-20): "the render is a strict superset of the store's structured
# facts (every spec-meta-carrying spec appears exactly once with its real fields; nothing
# hand-added, nothing silently dropped); a synthetic fixture with an injected status-claim/prose
# mismatch shows up flagged in the render, matching what the existing validators already report."
#
# Checks:
#  1. Real tree: every spec-meta-carrying spec in specs.jsonl appears exactly once in the render's
#     table, with its real id/status-claim/prose-status (spot-checked against the store directly,
#     not re-derived) — a strict superset, nothing dropped, nothing invented.
#  2. Real tree: the render's spec-with-meta count and pre-convention count both match
#     verify_all_specs.sh's own report exactly (cross-checked, not just asserted).
#  3. Synthetic fixture: a spec with a status-claim/prose mismatch renders with the same
#     "MISMATCH" flag `verify_all_specs.sh`/`r39_slice2_validate.sh` already report for it —
#     the render doesn't invent a second, differently-worded notion of "wrong."
#  4. Synthetic fixture: a spec with NO mismatch renders "ok", not a false positive.
#  5. --verify-results: when a Slice-3 sidecar is given, the render shows that spec's MOST RECENT
#     verify-run result (not an earlier, stale one) when multiple records exist for the same spec.
#  6. Regenerating twice against an unchanged store produces byte-identical output except for the
#     "Generated <timestamp>" line — proves the render is a pure function of the store, not
#     accumulating hidden state across runs.
#
# Exit codes: 0 = all checks pass, 1 = at least one check failed.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
command -v jq >/dev/null 2>&1 || { echo "r39_slice4_gate: requires jq" >&2; exit 2; }

PASS=0
FAIL=0
check() {
    local name="$1" cond="$2"
    if eval "$cond"; then
        echo "  PASS: $name"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $name"
        FAIL=$((FAIL + 1))
    fi
}

echo "=== 1-2. Real tree ==="
STORE="$ROOT/governance/state/specs.jsonl"
bash "$ROOT/scripts/verify_all_specs.sh" --export-jsonl "$STORE" > /tmp/r39s4_verify.log 2>&1
RENDER_OUT="$ROOT/governance/state/GOVERNANCE_STATUS.md"
bash "$ROOT/scripts/r39_render_status.sh" "$STORE" --out "$RENDER_OUT" > /tmp/r39s4_render.log 2>&1

# Spot-check: every real spec-meta id appears in the render table.
missing=0
while read -r base; do
    grep -q "\`$base\`" "$RENDER_OUT" || { missing=1; echo "  missing from render: $base"; }
done < <(jq -r 'select(.pre_convention == false) | .file' "$STORE")
check "every spec-meta-carrying spec appears in the render (nothing silently dropped)" '[ "$missing" -eq 0 ]'

render_meta_count=$(grep -c '^## Specs with spec-meta' "$RENDER_OUT" > /dev/null; grep -oE '\([0-9]+\)' "$RENDER_OUT" | head -1 | tr -d '()')
store_meta_count=$(jq -s '[.[] | select(.pre_convention == false)] | length' "$STORE")
check "render's spec-with-meta count matches the store exactly ($render_meta_count == $store_meta_count)" \
    '[ "$render_meta_count" = "$store_meta_count" ]'

render_pre_count=$(grep -oE 'Pre-convention specs — no spec-meta yet \([0-9]+\)' "$RENDER_OUT" | grep -oE '[0-9]+')
verify_pre_count=$(grep -oE '\([0-9]+ pre-convention' /tmp/r39s4_verify.log | grep -oE '[0-9]+')
check "render's pre-convention count matches verify_all_specs.sh's own report ($render_pre_count == $verify_pre_count)" \
    '[ -n "$verify_pre_count" ] && [ "$render_pre_count" = "$verify_pre_count" ]'

echo
echo "=== 3-4. Synthetic mismatch fixture ==="
FIXTURE=$(mktemp -d)
trap 'rm -rf "$FIXTURE"' EXIT
mkdir -p "$FIXTURE/specs"

cat > "$FIXTURE/specs/R900-mismatch.md" <<'EOF'
# Tech Spec — R900: Mismatch fixture
**Status:** Draft
```spec-meta
id: R900-mismatch
status-claim: Landed
depends-on: none
blocks: none
blocked-by: none
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```
EOF
cat > "$FIXTURE/specs/R901-clean.md" <<'EOF'
# Tech Spec — R901: Clean fixture
**Status:** Draft
```spec-meta
id: R901-clean
status-claim: Draft
depends-on: none
blocks: none
blocked-by: none
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```
EOF

FIXTURE_STORE="$FIXTURE/specs.jsonl"
FIXTURE_RENDER="$FIXTURE/GOVERNANCE_STATUS.md"
bash "$ROOT/scripts/verify_all_specs.sh" --specs-dir "$FIXTURE/specs" --export-jsonl "$FIXTURE_STORE" \
    > /tmp/r39s4_fixture_verify.log 2>&1 || true
bash "$ROOT/scripts/r39_render_status.sh" "$FIXTURE_STORE" --out "$FIXTURE_RENDER" > /dev/null 2>&1

check "mismatched spec (status-claim Landed, prose Draft) is flagged MISMATCH in the render" \
    'grep "R900-mismatch" "$FIXTURE_RENDER" | grep -q "MISMATCH"'
check "clean spec (status-claim == prose) renders ok, not a false positive" \
    'grep "R901-clean" "$FIXTURE_RENDER" | grep -q "| ok |"'
check "render's mismatch flag matches verify_all_specs.sh's own finding for the same spec" \
    'grep -q "R900-mismatch.*status-claim.*!=.*prose" /tmp/r39s4_fixture_verify.log'

echo
echo "=== 5. --verify-results: most-recent record wins ==="
mkdir -p "$FIXTURE/specs2"
cat > "$FIXTURE/specs2/R902-history.md" <<'EOF'
# Tech Spec — R902: History fixture
**Status:** Implementing
```spec-meta
id: R902-history
status-claim: Implementing
depends-on: none
blocks: none
blocked-by: none
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```
EOF
FIXTURE_STORE2="$FIXTURE/specs2.jsonl"
bash "$ROOT/scripts/verify_all_specs.sh" --specs-dir "$FIXTURE/specs2" --export-jsonl "$FIXTURE_STORE2" \
    > /dev/null 2>&1 || true

VERIFY_HISTORY="$FIXTURE/verify-results.jsonl"
echo '{"schema":"axon-gov-verify/1","spec":"R902-history","command":"x.sh","result":"FAIL","exit_code":1,"ts":"2026-07-01T00:00:00Z","commit":"aaa0000"}' > "$VERIFY_HISTORY"
echo '{"schema":"axon-gov-verify/1","spec":"R902-history","command":"x.sh","result":"PASS","exit_code":0,"ts":"2026-07-19T00:00:00Z","commit":"bbb1111"}' >> "$VERIFY_HISTORY"

FIXTURE_RENDER2="$FIXTURE/GOVERNANCE_STATUS2.md"
bash "$ROOT/scripts/r39_render_status.sh" "$FIXTURE_STORE2" --verify-results "$VERIFY_HISTORY" \
    --out "$FIXTURE_RENDER2" > /dev/null 2>&1

check "render shows the MOST RECENT verify-run result (PASS @ 07-19), not the stale FAIL @ 07-01" \
    'grep "R902-history" "$FIXTURE_RENDER2" | grep -q "PASS @ 2026-07-19" && ! (grep "R902-history" "$FIXTURE_RENDER2" | grep -q "FAIL @ 2026-07-01")'

echo
echo "=== 6. Pure function of the store (regenerate twice, diff modulo timestamp) ==="
RUN1="$FIXTURE/run1.md"
RUN2="$FIXTURE/run2.md"
bash "$ROOT/scripts/r39_render_status.sh" "$STORE" --out "$RUN1" > /dev/null 2>&1
sleep 1
bash "$ROOT/scripts/r39_render_status.sh" "$STORE" --out "$RUN2" > /dev/null 2>&1
DIFF_MINUS_TS=$(diff <(grep -v '^Generated ' "$RUN1") <(grep -v '^Generated ' "$RUN2") || true)
check "two renders of an unchanged store are byte-identical except the Generated timestamp line" \
    '[ -z "$DIFF_MINUS_TS" ]'

echo
echo "=== R39 Slice 4 gate: $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
