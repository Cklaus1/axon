#!/usr/bin/env bash
# The root Cargo.lock must be TRACKED and CURRENT for the committed manifests.
#
# This workspace ships executable artifacts and uses immutable snapshot
# worktrees as release evidence, so the commit has to determine the dependency
# graph the evidence came from. Two things can break that, and this gate checks
# both:
#
#   1. the lockfile stops being tracked (it was gitignored until 473480f, which
#      cost two 30-minute strict-gate cycles to registry fetch timeouts and left
#      receipts unable to say what was actually built);
#   2. a Cargo.toml changes and the lockfile is not updated with it, so the
#      committed pair disagrees and the next resolve silently moves versions.
#
# Check 2 is NOT delegated to `--locked` alone. `--locked` reports a stale lock
# and an unresolvable one with the same failure, so on a machine that cannot
# reach the registry its message does not distinguish "you forgot to commit the
# lock" from "you are offline". Asserting the lockfile is UNCHANGED by a
# resolve answers the question directly, and a resolve that cannot run at all
# is failed rather than skipped.
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
ok()  { echo "  ok    $*"; }
bad() { echo "  FAIL  $*"; fail=$((fail+1)); }

echo "── lockfile gate ──────────────────────────────────────────────────"

if git ls-files --error-unmatch Cargo.lock >/dev/null 2>&1; then
  ok "Cargo.lock is tracked"
else
  bad "Cargo.lock is NOT tracked — a commit cannot determine what gets built"
fi

if git diff --quiet -- Cargo.lock 2>/dev/null && git diff --cached --quiet -- Cargo.lock 2>/dev/null; then
  ok "Cargo.lock has no uncommitted changes"
else
  bad "Cargo.lock has uncommitted changes — commit it with the manifest change that caused it"
fi

# Does resolving the CURRENT manifests want to rewrite the lockfile? Run
# offline first: it needs no network when the cache is warm, and a warm cache is
# the normal state for this repo's gate. Fall back to an online resolve only if
# the offline one cannot proceed, so a cold cache is not reported as drift.
before="$(sha256sum Cargo.lock 2>/dev/null | cut -d' ' -f1)"
resolved=0
if CARGO_NET_OFFLINE=1 cargo metadata --format-version 1 >/dev/null 2>&1; then
  resolved=1
elif cargo metadata --format-version 1 >/dev/null 2>&1; then
  resolved=1
  echo "  note  offline resolve unavailable; used the registry"
fi
after="$(sha256sum Cargo.lock 2>/dev/null | cut -d' ' -f1)"

if [ "$resolved" -ne 1 ]; then
  # An absence is not a result. If the resolve could not run, this gate has
  # verified nothing about drift and must say so rather than pass.
  bad "could not resolve the workspace either offline or online — drift is UNVERIFIED"
elif [ "$before" != "$after" ]; then
  bad "resolving rewrote Cargo.lock — the committed lockfile is stale for these manifests"
  git diff --stat -- Cargo.lock 2>/dev/null | sed 's/^/        /'
else
  ok "resolving leaves Cargo.lock byte-identical (lock matches the manifests)"
fi

echo "───────────────────────────────────────────────────────────────────"
if [ "$fail" -eq 0 ]; then echo "lockfile_gate: PASS"; exit 0; fi
echo "lockfile_gate: FAIL ($fail check(s))"
exit 1
