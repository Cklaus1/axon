#!/usr/bin/env bash
# r39_slice3_gate.sh — R39 Slice 3 acceptance gate (governance/specs/R39-typed-execution-graph.md
# §6 slice 3): "axon-gov verify <spec>: live re-run. For a spec with a well-formed evidence:
# command, actually execute it, capture exit code + a result summary, and update the store's
# last-verified timestamp + commit hash automatically... Gate: running it against R32/R33/R34
# (already re-verified by hand this session) reproduces the same pass/fail/skip results recorded
# in their evidence tables."
#
# Realized as `verify_all_specs.sh --run TARGET --record-jsonl PATH` (no separate `axon-gov`
# binary yet — §12 Q3 is still open; Slices 1-2 established the pattern of extending the existing
# bash validator rather than standing up a new tool prematurely, and Slice 3 continues it). Each
# re-run evidence command gets ONE JSONL record (schema axon-gov-verify/1: spec, command, result,
# exit_code, ts, commit) appended to a sidecar file, kept separate from specs.jsonl (which stays a
# pure function of the markdown tree — a verify-run record is evidence of an action taken, not
# something re-derivable from the tree alone).
#
# Checks:
#  1. Synthetic PASS fixture (a spec whose evidence script exits 0): recorded result = PASS,
#     exit_code = 0.
#  2. Synthetic FAIL fixture (a spec whose evidence script exits nonzero): recorded result = FAIL,
#     exit_code matches the script's real exit code exactly (not just "nonzero").
#  3. Every record carries a well-formed ISO-8601 UTC timestamp and a valid short git commit hash.
#  4. Real spec R32 (formal-corrigibility-proof): running --run R32 --record-jsonl reproduces a
#     PASS (matches this session's own hand-verified record of r32_acceptance_gate.sh being green).
#  5. Real spec R33 (cross-vm-safety-quorum): same, PASS.
#  6. Real spec R34 (incremental-attestation): same, PASS.
#  7. --record-jsonl without --run is a hard usage error (exit 2), not a silent no-op.
#
# Exit codes: 0 = all checks pass, 1 = at least one check failed.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
command -v jq >/dev/null 2>&1 || { echo "r39_slice3_gate: requires jq" >&2; exit 2; }

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

echo "=== 1-2. Synthetic PASS/FAIL fixtures ==="
FIXTURE=$(mktemp -d)
trap 'rm -rf "$FIXTURE"' EXIT

cat > "$FIXTURE/R901-fixture-pass.md" <<'MDEOF'
# Tech Spec — R901: Fixture Pass

**Status:** Implementing

```spec-meta
id: R901-fixture-pass
status-claim: Implementing
depends-on: none
blocks: none
blocked-by: none
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: scripts/r39_slice3_fixture_pass.sh
```
MDEOF
cat > "$FIXTURE/R902-fixture-fail.md" <<'MDEOF'
# Tech Spec — R902: Fixture Fail

**Status:** Implementing

```spec-meta
id: R902-fixture-fail
status-claim: Implementing
depends-on: none
blocks: none
blocked-by: none
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: scripts/r39_slice3_fixture_fail.sh
```
MDEOF

# These MUST live under $ROOT/scripts/ (verify_all_specs.sh resolves evidence script paths
# against $ROOT, --specs-dir only overrides where the .md files themselves are read from) —
# cleaned up unconditionally by the trap below regardless of how this script exits.
cat > "$ROOT/scripts/r39_slice3_fixture_pass.sh" <<'SHEOF'
#!/usr/bin/env bash
echo "fixture pass gate"
exit 0
SHEOF
cat > "$ROOT/scripts/r39_slice3_fixture_fail.sh" <<'SHEOF'
#!/usr/bin/env bash
echo "fixture fail gate"
exit 7
SHEOF
chmod +x "$ROOT/scripts/r39_slice3_fixture_pass.sh" "$ROOT/scripts/r39_slice3_fixture_fail.sh"
trap 'rm -rf "$FIXTURE" "$ROOT/scripts/r39_slice3_fixture_pass.sh" "$ROOT/scripts/r39_slice3_fixture_fail.sh"' EXIT

RECORD="$FIXTURE/verify-results.jsonl"
bash "$ROOT/scripts/verify_all_specs.sh" --specs-dir "$FIXTURE" --run all --record-jsonl "$RECORD" \
    > "$FIXTURE/run.log" 2>&1 || true

pass_rec=$(jq -c 'select(.spec == "R901-fixture-pass")' "$RECORD" 2>/dev/null || echo "")
fail_rec=$(jq -c 'select(.spec == "R902-fixture-fail")' "$RECORD" 2>/dev/null || echo "")

check "PASS fixture recorded result=PASS" \
    '[ "$(echo "$pass_rec" | jq -r .result)" = "PASS" ]'
check "PASS fixture recorded exit_code=0" \
    '[ "$(echo "$pass_rec" | jq -r .exit_code)" = "0" ]'
check "FAIL fixture recorded result=FAIL" \
    '[ "$(echo "$fail_rec" | jq -r .result)" = "FAIL" ]'
check "FAIL fixture recorded exit_code=7 (the real exit code, not just nonzero)" \
    '[ "$(echo "$fail_rec" | jq -r .exit_code)" = "7" ]'

echo
echo "=== 3. Record shape: timestamp + commit hash ==="
ts=$(echo "$pass_rec" | jq -r .ts)
commit=$(echo "$pass_rec" | jq -r .commit)
check "timestamp is well-formed ISO-8601 UTC ($ts)" \
    'echo "$ts" | grep -qE "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"'
check "commit is a plausible short git hash ($commit)" \
    'echo "$commit" | grep -qE "^[0-9a-f]{6,}$|^unknown$"'

echo
echo "=== 4-6. Real specs: R32, R33, R34 reproduce their known-green evidence gates ==="
REAL_RECORD="$FIXTURE/real-verify-results.jsonl"
for spec in R32 R33 R34; do
    bash "$ROOT/scripts/verify_all_specs.sh" --run "$spec" --record-jsonl "$REAL_RECORD" \
        > "$FIXTURE/real_${spec}.log" 2>&1
    rec=$(jq -c "select(.spec | startswith(\"${spec}-\"))" "$REAL_RECORD" 2>/dev/null | tail -1)
    check "$spec: re-run recorded and result=PASS" \
        '[ -n "$rec" ] && [ "$(echo "$rec" | jq -r .result)" = "PASS" ]'
done

echo
echo "=== 7. --record-jsonl without --run is a hard usage error ==="
bash "$ROOT/scripts/verify_all_specs.sh" --record-jsonl "$FIXTURE/should-not-exist.jsonl" \
    > "$FIXTURE/usage_err.log" 2>&1
usage_exit=$?
check "usage error exits 2" '[ "$usage_exit" -eq 2 ]'
check "no file written on usage error" '[ ! -f "$FIXTURE/should-not-exist.jsonl" ]'

echo
echo "=== R39 Slice 3 gate: $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
