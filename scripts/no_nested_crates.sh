#!/usr/bin/env bash
# No Rust package root may be nested inside another package's directory.
#
# `crates/axon-ledger/axon-ledger/` was a git-tracked duplicate of the ledger
# crate inside itself: 11 files, absent from the workspace members, referenced
# by nothing, and a PRE-RBAC snapshot — it had no rbac.rs at all. Dead code that
# still shipped beside the authoritative implementation.
#
# That is worse than ordinary dead code. A stale copy of a security-relevant
# crate is exactly what a future contributor or agent revives, carrying the bug
# that was just fixed in the real one.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

fail=0
while IFS= read -r manifest; do
  dir="$(dirname "$manifest")"
  # Is any ANCESTOR directory (below crates/) also a package root?
  parent="$(dirname "$dir")"
  while [ "$parent" != "." ] && [ "$parent" != "/" ]; do
    if [ -f "$parent/Cargo.toml" ]; then
      echo "  ✗ nested package root: $manifest is inside $parent/Cargo.toml"
      fail=1
    fi
    parent="$(dirname "$parent")"
  done
done < <(git ls-files '*/Cargo.toml' | grep -v '^Cargo.toml$')

# Guard the guard: if the scan found no manifests at all it would pass while
# examining nothing.
n=$(git ls-files '*/Cargo.toml' | grep -vc '^Cargo.toml$')
if [ "$n" -lt 5 ]; then
  echo "  ✗ only $n package manifests found — this check's premise is gone, not satisfied"
  exit 1
fi

if [ "$fail" -ne 0 ]; then
  echo "no_nested_crates: FAIL — a package nested inside another is dead code that"
  echo "  still ships. Delete it, or make it a real workspace member with a reason."
  exit 1
fi
echo "no_nested_crates: OK — $n package roots, none nested"
