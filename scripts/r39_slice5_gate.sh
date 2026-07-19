#!/usr/bin/env bash
# r39_slice5_gate.sh — R39 Slice 5 acceptance gate (governance/specs/R39-typed-execution-graph.md
# §6 slice 5): "a synthetic test fixture with an intentional cycle is rejected; R36's real
# blocked-by: R36 §12 Q1 is correctly reported as still-blocking (Q1 unresolved)."
#
# Checks:
#  1. Real tree is clean (no cycles, no stale blocked-by) via r39_slice5_dag_check.sh.
#  2. Real tree: R36's blocked-by is specifically reported as still-blocking / unresolved (the
#     spec's own named example).
#  3. Synthetic fixture: an intentional 2-spec depends-on cycle (A depends-on B, B depends-on A)
#     is rejected.
#  4. Synthetic fixture: an intentional blocks-edge cycle (A blocks B, B blocks A) is rejected.
#  5. Synthetic fixture: a blocked-by naming an ALREADY-RESOLVED question is flagged stale.
#  6. Synthetic fixture: a blocked-by naming a genuinely unresolved question is NOT flagged (no
#     false positive) — this is also a regression test for a real bug found while building this
#     slice: a naive substring match on "resolved" false-positives on "Unresolved, deliberately"
#     (R40's actual §12 Q1 text).
#  7. Synthetic fixture: a spec's §12 using plain "1./2./3." numbering (no "**Qn**" bold labels,
#     matching R37/R38's real format) still resolves a "Qn"-labeled blocked-by correctly.
#
# Exit codes: 0 = all checks pass, 1 = at least one check failed.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
command -v jq >/dev/null 2>&1 || { echo "r39_slice5_gate: requires jq" >&2; exit 2; }

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
bash "$ROOT/scripts/verify_all_specs.sh" --export-jsonl "$ROOT/governance/state/specs.jsonl" \
    > /dev/null 2>&1
bash "$ROOT/scripts/r39_slice5_dag_check.sh" > /tmp/r39s5_real.log 2>&1
real_exit=$?
check "real tree: clean (no cycles, no stale blocked-by)" '[ "$real_exit" -eq 0 ]'
check "real tree: R36 reported still-blocking on §12 Q1" \
    'grep -q "R36-full-asi-os still-blocking on R36 §12 Q1 (unresolved" /tmp/r39s5_real.log'

FIXTURE=$(mktemp -d)
trap 'rm -rf "$FIXTURE"' EXIT

echo
echo "=== 3. Synthetic depends-on cycle ==="
mkdir -p "$FIXTURE/cycle_depends"
cat > "$FIXTURE/cycle_depends/R910-a.md" <<'EOF'
# Tech Spec — R910: A
**Status:** Draft
```spec-meta
id: R910-a
status-claim: Draft
depends-on: R911-b
blocks: none
blocked-by: none
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```
EOF
cat > "$FIXTURE/cycle_depends/R911-b.md" <<'EOF'
# Tech Spec — R911: B
**Status:** Draft
```spec-meta
id: R911-b
status-claim: Draft
depends-on: R910-a
blocks: none
blocked-by: none
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```
EOF
STORE1="$FIXTURE/cycle_depends.jsonl"
bash "$ROOT/scripts/verify_all_specs.sh" --specs-dir "$FIXTURE/cycle_depends" --export-jsonl "$STORE1" \
    > /dev/null 2>&1 || true
bash "$ROOT/scripts/r39_slice5_dag_check.sh" "$STORE1" --specs-dir "$FIXTURE/cycle_depends" \
    > "$FIXTURE/cycle_depends.log" 2>&1
cyc1_exit=$?
check "depends-on cycle rejected (nonzero exit)" '[ "$cyc1_exit" -ne 0 ]'
check "depends-on cycle: FINDING mentions cycle" 'grep -qi "FINDING:.*cycle" "$FIXTURE/cycle_depends.log"'

echo
echo "=== 4. Synthetic blocks-edge cycle ==="
mkdir -p "$FIXTURE/cycle_blocks"
cat > "$FIXTURE/cycle_blocks/R912-a.md" <<'EOF'
# Tech Spec — R912: A
**Status:** Draft
```spec-meta
id: R912-a
status-claim: Draft
depends-on: none
blocks: R913-b
blocked-by: none
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```
EOF
cat > "$FIXTURE/cycle_blocks/R913-b.md" <<'EOF'
# Tech Spec — R913: B
**Status:** Draft
```spec-meta
id: R913-b
status-claim: Draft
depends-on: none
blocks: R912-a
blocked-by: none
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```
EOF
STORE2="$FIXTURE/cycle_blocks.jsonl"
bash "$ROOT/scripts/verify_all_specs.sh" --specs-dir "$FIXTURE/cycle_blocks" --export-jsonl "$STORE2" \
    > /dev/null 2>&1 || true
bash "$ROOT/scripts/r39_slice5_dag_check.sh" "$STORE2" --specs-dir "$FIXTURE/cycle_blocks" \
    > "$FIXTURE/cycle_blocks.log" 2>&1
cyc2_exit=$?
check "blocks-edge cycle rejected (nonzero exit)" '[ "$cyc2_exit" -ne 0 ]'

echo
echo "=== 5-7. Synthetic blocked-by staleness (resolved / unresolved / plain-numbered) ==="
mkdir -p "$FIXTURE/blocked_by"
# R920: cites a question in R921 that IS marked resolved -> should be flagged stale.
cat > "$FIXTURE/blocked_by/R920-waiter.md" <<'EOF'
# Tech Spec — R920: Waiter
**Status:** Draft
```spec-meta
id: R920-waiter
status-claim: Draft
depends-on: none
blocks: none
blocked-by: R921 §12 Q1 (a fixture question)
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```
EOF
cat > "$FIXTURE/blocked_by/R921-target.md" <<'EOF'
# Tech Spec — R921: Target
**Status:** Draft
```spec-meta
id: R921-target
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

### 12. Open questions

- **Q1 (fixture):** Some question. Resolved 2026-07-19: the fixture answer.
- **Q2 (fixture, still unresolved):** Some other question. Unresolved, deliberately.
EOF
STORE3="$FIXTURE/blocked_by.jsonl"
bash "$ROOT/scripts/verify_all_specs.sh" --specs-dir "$FIXTURE/blocked_by" --export-jsonl "$STORE3" \
    > /dev/null 2>&1 || true
bash "$ROOT/scripts/r39_slice5_dag_check.sh" "$STORE3" --specs-dir "$FIXTURE/blocked_by" \
    > "$FIXTURE/blocked_by.log" 2>&1
check "resolved-question blocked-by IS flagged stale" \
    'grep -qi "R920-waiter.*RESOLVED.*stale" "$FIXTURE/blocked_by.log"'

# R922: cites Q2 (genuinely unresolved, "Unresolved, deliberately" text) -> must NOT be flagged.
mkdir -p "$FIXTURE/blocked_by2"
cp "$FIXTURE/blocked_by/R921-target.md" "$FIXTURE/blocked_by2/R921-target.md"
cat > "$FIXTURE/blocked_by2/R922-waiter2.md" <<'EOF'
# Tech Spec — R922: Waiter2
**Status:** Draft
```spec-meta
id: R922-waiter2
status-claim: Draft
depends-on: none
blocks: none
blocked-by: R921 §12 Q2 (a fixture question, genuinely unresolved)
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```
EOF
STORE4="$FIXTURE/blocked_by2.jsonl"
bash "$ROOT/scripts/verify_all_specs.sh" --specs-dir "$FIXTURE/blocked_by2" --export-jsonl "$STORE4" \
    > /dev/null 2>&1 || true
bash "$ROOT/scripts/r39_slice5_dag_check.sh" "$STORE4" --specs-dir "$FIXTURE/blocked_by2" \
    > "$FIXTURE/blocked_by2.log" 2>&1
check "unresolved ('Unresolved, deliberately') blocked-by NOT flagged stale (no false positive)" \
    '! grep -q "FINDING:.*R922-waiter2" "$FIXTURE/blocked_by2.log"'
check "unresolved blocked-by reported as still-blocking" \
    'grep -q "R922-waiter2 still-blocking on R921 §12 Q2" "$FIXTURE/blocked_by2.log"'

# R923: cites Q1 in a target whose §12 uses plain "1./2." numbering, not "**Qn**" bold labels
# (matching R37/R38's real format) -> Q1 must still resolve to the first numbered item.
mkdir -p "$FIXTURE/plain_numbered"
cat > "$FIXTURE/plain_numbered/R924-target.md" <<'EOF'
# Tech Spec — R924: Target
**Status:** Draft
```spec-meta
id: R924-target
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

### 12. Open questions

1. Some question, plain numbered, no Q-label, still open.
2. A second question, still open.
EOF
cat > "$FIXTURE/plain_numbered/R923-waiter3.md" <<'EOF'
# Tech Spec — R923: Waiter3
**Status:** Draft
```spec-meta
id: R923-waiter3
status-claim: Draft
depends-on: none
blocks: none
blocked-by: R924 §12 Q1 (plain-numbered fixture question)
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```
EOF
STORE5="$FIXTURE/plain_numbered.jsonl"
bash "$ROOT/scripts/verify_all_specs.sh" --specs-dir "$FIXTURE/plain_numbered" --export-jsonl "$STORE5" \
    > /dev/null 2>&1 || true
bash "$ROOT/scripts/r39_slice5_dag_check.sh" "$STORE5" --specs-dir "$FIXTURE/plain_numbered" \
    > "$FIXTURE/plain_numbered.log" 2>&1
check "Qn maps to plain-numbered item N when no bold **Qn** labels exist (no false 'doesn't exist')" \
    '! grep -q "FINDING:.*R923-waiter3" "$FIXTURE/plain_numbered.log"'
check "plain-numbered Q1 reported as still-blocking" \
    'grep -q "R923-waiter3 still-blocking on R924 §12 Q1" "$FIXTURE/plain_numbered.log"'

echo
echo "=== R39 Slice 5 gate: $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
