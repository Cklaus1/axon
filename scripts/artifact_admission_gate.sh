#!/usr/bin/env bash
# Artifact admission gate.
#
# The invariant: generated build output never becomes repository source by
# accident. Two real violations motivated this, both from a broad `git add`
# folding unrelated untracked artifacts into an otherwise coherent commit:
#
#   * cac9cdf  crates/axon-core/{cg,vapp} — two 44MB native binaries left by
#              `axon build` probes, which writes output next to the source
#   * the Cortex experiment commits — 8 __pycache__/*.pyc files
#
# Neither was caught by anything, because nothing asserted that a state
# transition (untracked -> tracked) was INTENDED. This asserts it.
#
# The check runs over TRACKED files, not the working tree: an artifact that is
# merely present on disk is harmless, one that is tracked is the defect.
#
# Exit 0 clean, 1 on any violation.
set -uo pipefail
cd "$(dirname "$0")/.."

ALLOW=scripts/artifact_admission_allowlist.txt
MAX_BYTES=$((2 * 1024 * 1024))   # derived, not guessed: the largest legitimate
                                 # tracked file today is cli_run.rs at ~1.17MB.
fail=0
note() { printf '  %s\n' "$*"; }

# --- the allowlist, read first so both directions can be checked -------------
declare -A allowed=()
if [[ -f $ALLOW ]]; then
  while IFS= read -r line; do
    line="${line%%#*}"; line="$(echo "$line" | xargs)"
    [[ -n $line ]] && allowed["$line"]=1
  done < "$ALLOW"
fi

mapfile -t tracked < <(git ls-files)

# --- 1. tracked files git considers binary ----------------------------------
# `git grep -I` lists files with TEXT content; the complement is git's own
# binary determination, which is what `git diff` uses, so this agrees with the
# tool rather than reimplementing a heuristic.
mapfile -t textfiles < <(git grep -I --name-only -e '' 2>/dev/null | sort -u)
printf '%s\n' "${tracked[@]}" | sort -u > /tmp/.aag_all.$$
printf '%s\n' "${textfiles[@]}" > /tmp/.aag_txt.$$
mapfile -t binaries < <(comm -23 /tmp/.aag_all.$$ /tmp/.aag_txt.$$)
rm -f /tmp/.aag_all.$$ /tmp/.aag_txt.$$

for f in "${binaries[@]}"; do
  [[ -z $f ]] && continue
  [[ -n ${allowed[$f]:-} ]] && continue
  # An EMPTY file has no text content, so `git grep -I` does not list it and it
  # lands in the complement looking exactly like a binary. It is not one, and
  # calling it one would make the gate cry wolf on every touch-created file.
  [[ -s $f ]] || continue
  note "BINARY not allowlisted: $f ($(du -h "$f" 2>/dev/null | cut -f1))"
  fail=1
done

# --- 2. oversized tracked files ---------------------------------------------
for f in "${tracked[@]}"; do
  [[ -f $f ]] || continue
  [[ -n ${allowed[$f]:-} ]] && continue
  sz=$(stat -c %s "$f" 2>/dev/null || echo 0)
  if (( sz > MAX_BYTES )); then
    note "OVERSIZED ($((sz/1024/1024))MB > $((MAX_BYTES/1024/1024))MB): $f"
    fail=1
  fi
done

# --- 3. known build-output shapes -------------------------------------------
# Name/path patterns that are build output by construction. These are rejected
# even when small and even when they happen to be text (a .pyc is binary, but
# a generated .d or an empty .log is not).
for f in "${tracked[@]}"; do
  [[ -n ${allowed[$f]:-} ]] && continue
  case "$f" in
    *__pycache__/*|*.pyc|*.pyo|*.o|*.a|*.so|*.dylib|*.dll|*.rlib|*.rmeta|\
    target/*|*/target/*|*.bin|crates/*/cg|crates/*/vapp)
      note "BUILD OUTPUT: $f"; fail=1 ;;
  esac
done

# --- 4. allowlist drift (the other direction) --------------------------------
# An allowlist entry for a file that is no longer tracked is a stale exemption,
# and a stale exemption is how an allowlist quietly becomes a blanket one.
for f in "${!allowed[@]}"; do
  git ls-files --error-unmatch "$f" >/dev/null 2>&1 || {
    note "STALE ALLOWLIST entry (file not tracked): $f"; fail=1; }
done

if (( fail )); then
  echo "artifact_admission_gate: FAILED"
  echo "  A tracked file looks like generated output. Either it is — untrack it"
  echo "  (git rm --cached) and add an ignore rule — or it is a legitimate"
  echo "  checked-in fixture, in which case add it to $ALLOW with a one-line"
  echo "  reason so the exemption is reviewable."
  exit 1
fi
echo "artifact_admission_gate: OK (${#tracked[@]} tracked files, 0 violations)"
