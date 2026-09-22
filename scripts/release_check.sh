#!/usr/bin/env bash
# Decide whether a commit is RELEASE-VERIFIED, from evidence on disk.
#
# WHY THIS EXISTS. The release criteria were being re-derived by hand each
# time, and that is how a release claim gets made from a partial run: the
# person deciding is the same person who wants the answer to be yes. This
# reduces the decision to reading receipts.
#
# It verifies EVIDENCE. It does not run the tests — a checker that can
# produce its own passing evidence is not a checker.
#
# Usage: scripts/release_check.sh [<commit>]     (default: HEAD)
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2
RUNS="$ROOT/.axon-runs"

TARGET="$(git rev-parse "${1:-HEAD}" 2>/dev/null)" || { echo "not a commit: ${1:-HEAD}"; exit 2; }
SHORT="$(git rev-parse --short "$TARGET")"
fails=0
say()  { printf '  %-6s %s\n' "$1" "$2"; }
bad()  { say "FAIL" "$1"; fails=$((fails+1)); }
ok()   { say "ok" "$1"; }

echo "── release check: $SHORT ──────────────────────────────────────────"

# 1. CLEAN TREE. A dirty tree means the commit is not what would be shipped.
if [ -n "$(git status --porcelain)" ]; then
  bad "working tree is dirty — the commit is not what is on disk"
else
  ok "working tree clean"
fi

# 2. HEAD is the target (otherwise the other checks describe something else).
if [ "$(git rev-parse HEAD)" != "$TARGET" ]; then
  bad "HEAD is not $SHORT — checking evidence for a commit that is not checked out"
else
  ok "HEAD is $SHORT"
fi

# 3. A STRICT GATE RECEIPT BOUND TO THIS COMMIT.
#    Stale receipts are the thing this exists to refuse, so the search is by
#    recorded commit, never by "the most recent run".
gate_run=""
for d in "$RUNS"/*/; do
  [ -f "$d/receipt" ] || continue
  grep -q "^head=$TARGET$" "$d/receipt" 2>/dev/null || continue
  grep -qE '^command=.*gate\.sh' "$d/receipt" 2>/dev/null || continue
  if scripts/run_managed.sh verify "${d%/}" --for "$TARGET" >/dev/null 2>&1; then
    gate_run="${d%/}"; break
  fi
done
if [ -n "$gate_run" ]; then
  ok "strict gate receipt: $(basename "$gate_run")"
  # WHICH ENVIRONMENT the gate ran in. Axon's behaviour is steered by AXON_*
  # variables, and a release gate run under, say, AXON_AI_MOCK=1 or a relaxed
  # AXON_ALLOWED_EFFECTS is testing something other than the default product.
  # Motivated by a real bug: two axon-ai tests passed or failed depending on
  # whether AXON_AI_MOCK was set, and the receipt's env field is what
  # localised it.
  gate_env="$(sed -n 's/^env_axon_names=//p' "$gate_run/receipt")"
  if [ -n "$gate_env" ]; then
    bad "the strict gate ran with AXON_* set ($gate_env) — that is not the"
    bad "  default environment, so it does not certify default behaviour"
  else
    ok "strict gate ran with a clean AXON_* environment"
  fi
else
  bad "no CITABLE strict-gate receipt bound to $SHORT"
  bad "  (a receipt for another commit does not certify this one)"
fi

# 4. FALSE GREENS. An OPEN false green blocks any completeness claim.
open_fg="$(python3 -c "
import json
d=json.load(open('AXON-COMPLETENESS.json'))
print(sum(1 for e in d.get('false_greens',[]) if e.get('status')=='open'))" 2>/dev/null)"
if [ "${open_fg:-1}" = "0" ]; then ok "false greens: 0 OPEN"; else bad "false greens: ${open_fg:-unknown} OPEN"; fi

# 5. COMPLETENESS + CLAIMS gates.
if python3 scripts/completeness.py >/dev/null 2>&1; then ok "completeness gate"; else bad "completeness gate"; fi
if ./scripts/claims_gate.sh >/dev/null 2>&1; then ok "claims gate"; else bad "claims gate"; fi

# 6. EVERY security/runtime-critical crate has a required command, and the
#    manifest covers the workspace. The gate RUNS them; this confirms the
#    intent is declared, so a green gate cannot mean "ran nothing".
miss="$(python3 -c "
import json,re
m=json.load(open('governance/release-verification.json'))
members={l.strip().strip('\",').rsplit('/',1)[-1]
         for l in re.search(r'members\s*=\s*\[(.*?)\]',open('Cargo.toml').read(),re.S).group(1).splitlines()
         if l.strip().strip('\",')}
crates=m['crates']
bad=[c for c in members if c and c not in crates]
bad+= [n for n,s in crates.items() if s.get('class') in ('A','B') and not s.get('required')]
print(','.join(sorted(set(bad))))" 2>/dev/null)"
if [ -z "$miss" ]; then ok "crate coverage manifest complete"; else bad "unclassified or unverified crates: $miss"; fi

echo "───────────────────────────────────────────────────────────────────"
if [ "$fails" -eq 0 ]; then
  echo "RELEASE-VERIFIED  $SHORT"
  exit 0
fi
echo "NOT RELEASE-VERIFIED  $SHORT  ($fails check(s) failed)"
echo "A push is still fine — but it must be labelled unverified."
exit 1
