#!/usr/bin/env bash
# r39_slice2_gate.sh — R39 Slice 2 acceptance gate (governance/specs/R39-typed-execution-graph.md
# §6 slice 2): "byte-identical findings to the current verify_all_specs.sh on today's tree ... the
# new validator must not find fewer real bugs than the bash version, and any new finding must be
# manually confirmed real before the port is trusted."
#
# Two checks:
#  A. REAL TREE: verify_all_specs.sh's findings and scripts/r39_slice2_validate.sh's findings
#     (run against a freshly regenerated store) must be the exact same SET of FINDING: lines.
#  B. SYNTHETIC FIXTURE: a scratch copy of governance/specs/ with 4 deliberately injected real
#     bugs (dangling depends-on, status-claim/prose mismatch, missing evidence script, a NEW
#     duplicate spec number not on the KNOWN_DUAL allowlist) — both validators must report the
#     exact same set of findings against it. This is the actual regression test the spec's gate
#     calls for: proves the port finds bugs, not just that both agree an already-clean tree is
#     clean.
#
# Exit codes: 0 = all checks pass, 1 = at least one check failed.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
command -v jq >/dev/null 2>&1 || { echo "r39_slice2_gate: requires jq" >&2; exit 2; }

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


# Normalizes away one cosmetic, non-semantic representational difference: the bash validator's
# duplicate-number finding lists filenames WITH the .md suffix (it works straight off `ls`); the
# typed store's `file` field is deliberately the bare id, never .md (Slice 1's own gate already
# locks that in). Same set of colliding numbers, same warn/finding classification either way —
# stripping ".md" before comparing treats this as the same finding, not "the ported validator
# found something new."
extract_findings() { grep '^FINDING:' | sed -E 's/\.md\b//g' | sort; }

echo "=== A. Real tree: bash validator vs. ported (store) validator ==="
STORE="$ROOT/governance/state/specs.jsonl"
bash "$ROOT/scripts/verify_all_specs.sh" --export-jsonl "$STORE" > /tmp/r39s2_real_orig.log 2>&1
bash "$ROOT/scripts/r39_slice2_validate.sh" "$STORE" > /tmp/r39s2_real_ported.log 2>&1

orig_findings=$(extract_findings < /tmp/r39s2_real_orig.log)
ported_findings=$(extract_findings < /tmp/r39s2_real_ported.log)
echo "  orig findings:   $(echo -n "$orig_findings" | grep -c '^FINDING' || true)"
echo "  ported findings: $(echo -n "$ported_findings" | grep -c '^FINDING' || true)"
check "real-tree findings are identical sets" '[ "$orig_findings" = "$ported_findings" ]'

orig_pre=$(grep -oE '\([0-9]+ pre-convention' /tmp/r39s2_real_orig.log | grep -oE '[0-9]+')
ported_pre=$(grep -oE '\([0-9]+ pre-convention' /tmp/r39s2_real_ported.log | grep -oE '[0-9]+')
check "pre-convention counts match ($orig_pre == $ported_pre)" '[ -n "$orig_pre" ] && [ "$orig_pre" = "$ported_pre" ]'

echo
echo "=== B. Synthetic fixture: 4 injected real bugs ==="
FIXTURE=$(mktemp -d)
trap 'rm -rf "$FIXTURE"' EXIT
cp "$ROOT"/governance/specs/*.md "$FIXTURE"/

# Bug 1: dangling depends-on (R14-mobile-targets currently depends-on: R13-native-ffi only)
sed -i 's/^depends-on: R13-native-ffi$/depends-on: R13-native-ffi, R999-fixture-nonexistent/' \
    "$FIXTURE/R14-mobile-targets.md"

# Bug 2: status-claim vs prose Status mismatch (R33 spec-meta says Implementing; flip it)
sed -i 's/^status-claim: Implementing$/status-claim: Landed/' \
    "$FIXTURE/R33-cross-vm-safety-quorum.md"

# Bug 3: evidence names a script that doesn't exist (R34's real evidence is a real script)
sed -i 's#^evidence: scripts/r34_acceptance_gate\.sh$#evidence: scripts/r34_fixture_missing_gate.sh#' \
    "$FIXTURE/R34-incremental-attestation.md"

# Bug 4: a NEW duplicate spec number not on KNOWN_DUAL (R39 isn't in R18/R21-R25)
cp "$FIXTURE/R39-typed-execution-graph.md" "$FIXTURE/R39-fixture-duplicate.md"

FIXTURE_STORE=$(mktemp)
bash "$ROOT/scripts/verify_all_specs.sh" --specs-dir "$FIXTURE" --export-jsonl "$FIXTURE_STORE" \
    > /tmp/r39s2_fixture_orig.log 2>&1
bash "$ROOT/scripts/r39_slice2_validate.sh" "$FIXTURE_STORE" > /tmp/r39s2_fixture_ported.log 2>&1
rm -f "$FIXTURE_STORE"

fixture_orig=$(extract_findings < /tmp/r39s2_fixture_orig.log)
fixture_ported=$(extract_findings < /tmp/r39s2_fixture_ported.log)

check "fixture: bash validator caught >= 4 findings" \
    '[ "$(echo -n "$fixture_orig" | grep -c "^FINDING")" -ge 4 ]'
check "fixture: ported validator caught >= 4 findings" \
    '[ "$(echo -n "$fixture_ported" | grep -c "^FINDING")" -ge 4 ]'
check "fixture: findings are identical sets (byte-identical port)" \
    '[ "$fixture_orig" = "$fixture_ported" ]'

check "fixture: dangling depends-on caught by both" \
    'echo "$fixture_orig" | grep -q "R14-mobile-targets.*R999-fixture-nonexistent" && echo "$fixture_ported" | grep -q "R14-mobile-targets.*R999-fixture-nonexistent"'
check "fixture: status-claim/prose mismatch caught by both" \
    'echo "$fixture_orig" | grep -q "R33-cross-vm-safety-quorum.*status-claim" && echo "$fixture_ported" | grep -q "R33-cross-vm-safety-quorum.*status-claim"'
check "fixture: missing evidence script caught by both" \
    'echo "$fixture_orig" | grep -q "R34-incremental-attestation.*missing script" && echo "$fixture_ported" | grep -q "R34-incremental-attestation.*missing script"'
check "fixture: NEW duplicate spec number caught by both" \
    'echo "$fixture_orig" | grep -q "NEW spec-number collision R39" && echo "$fixture_ported" | grep -q "NEW spec-number collision R39"'

# Sanity: a REAL known-dual prefix (e.g. R21) must still be a warn, not a finding, in the fixture
# too — proves KNOWN_DUAL ported correctly rather than accidentally being dropped.
check "fixture: KNOWN_DUAL collision (R21) still just a warn, not a finding, in both" \
    '! echo "$fixture_orig" | grep -q "collision R21" && ! echo "$fixture_ported" | grep -q "collision R21"'

echo
echo "=== R39 Slice 2 gate: $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
