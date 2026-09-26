#!/usr/bin/env bash
# cortex_package_gate.sh — continuous check for the vendored Cortex v0.15 build package.
#
# WHY THIS EXISTS.
#
# `docs/axon_cortex_v0_15/axon-cortex-build-v0_15/` is a 114-file documentation
# package: 35 specs, 152 work packages and 247 PROPOSED product gates. Every one
# of those 247 carries `"product_result": "NOT_RUN"` and
# `"implementation_status": "Not implemented in this package"`, and every work
# package is `"status": "Not started"`. The package is scrupulous about saying so.
#
# That honesty is the property most easily destroyed on intake. This repository
# already has the defect at smaller scale: 7 of the 37 `scripts/*.sh` cited as
# evidence in `governance/REQUIREMENTS.md` are invoked by nothing, so a
# requirement reads as verified while its cited gate has never run. 247 more
# gate IDs, each one edit away from reading as executed, would multiply it.
#
# So this gate asserts two things on every run:
#
#   1. INTEGRITY. `SHA256SUMS_v0_15.json` (schema `cortex-package-sha256/1`) is
#      checked in BOTH directions -- every listed file hashes to its recorded
#      digest, AND no unlisted file has appeared under the package root. One
#      direction alone would miss a file being ADDED. The manifest itself is the
#      single legitimate unlisted file (it cannot contain its own hash).
#      The package's own `tools/validate_package.py` is also run -- it is a real
#      check that, before this script, ran nowhere: the same orphaned-verification
#      class described above.
#
#   2. HONESTY. A gate may claim a `product_result` other than NOT_RUN, and a work
#      package a `status` other than "Not started", ONLY when
#      `governance/cortex_gate_execution_registry.json` carries a row naming a
#      file in THIS repository that exists and is invoked by something that runs.
#      Today the registry is empty, so all 247 must remain NOT_RUN and this is a
#      tripwire. It is deliberately NOT a re-assertion of today's constants: when
#      CX-02's gates are implemented for real, the documented path is to add a
#      registry row (see `how_to_add_a_row` in that file) rather than to weaken
#      this script. The row is not self-certifying -- the invoker is grepped for
#      the executed script, so a row pointing at an orphan is rejected, and every
#      row is re-validated on every run so one cannot rot into a rubber stamp.
#
#      Note the vendored manifest is NOT where an implemented gate gets recorded:
#      the package's own validator hard-fails on any product_result other than
#      NOT_RUN, and the sha256 manifest pins the file. NOT_RUN is a true statement
#      about the PACKAGE and stays true however much this repo implements. The
#      registry is this repository's execution record; the manifest clause exists
#      for the day an upstream release re-vendors results in.
#
# NON-VACUITY. Verifying zero files or parsing zero gates is a FAILURE, not a
# pass. This repository has shipped a harness that exited 0 having compared
# nothing; the floors below exist so this cannot be another one.
#
# COST. Pure text and SHA256 over 1.9MB plus one python3 run. No cargo, no
# toolchain, no network. Cheap enough for every gate run.
#
# Exit 0 = package intact and still honest. Exit 1 = one of the two broke.

set -uo pipefail
cd "$(dirname "$0")/.."

PKG="docs/axon_cortex_v0_15/axon-cortex-build-v0_15"
SUMS="$PKG/SHA256SUMS_v0_15.json"
REGISTRY="governance/cortex_gate_execution_registry.json"
REPORT="target/cortex-package-validation.json"
# The manifest is pinned from OUTSIDE the package. A forward/reverse check
# against a manifest that can be regenerated alongside the files it lists
# proves nothing: measured on a scratch copy, a tampered spec plus a
# recomputed SHA256SUMS_v0_15.json read as "both directions clean" (gate PASS).
# Re-vendoring means editing this pin — a reviewable code change.
PIN_SUMS15="99047d324053d7aa09020061de9787372675760c5d67be1e31863ed2f6bddee3"

# Non-vacuity floors. Minimums, not exact values: a legitimately re-vendored
# package may grow. A truncated or empty manifest trips these.
MIN_FILES=100
MIN_GATES=200
MIN_TASKS=100

# Prerequisite problems abort; CHECK failures accumulate. Reporting every broken
# section in one run matters here because a single bad edit trips more than one:
# flipping a gate to PASS breaks BOTH the sha256 manifest and the honesty
# invariant, and seeing only the first would hide which property actually moved.
FAILURES=()
abort()     { echo "❌ cortex package gate ABORTED: $1"; exit 1; }
note_fail() { echo "   ↳ FAILED: $1"; FAILURES+=("$1"); }

command -v python3 >/dev/null 2>&1 || abort "python3 not found (required to parse the package manifests)"
[ -d "$PKG" ] || abort "package root missing: $PKG"
[ -f "$SUMS" ] || abort "sha256 manifest missing: $SUMS"
[ -f "$REGISTRY" ] || abort "execution registry missing: $REGISTRY"
mkdir -p target

echo "── cortex: package integrity (SHA256SUMS_v0_15, both directions) ──"
python3 -B - "$PKG" "$SUMS" "$MIN_FILES" "$PIN_SUMS15" <<'PY'
import hashlib, json, os, sys
pkg, sums, min_files, pin_sums = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
got_sums = hashlib.sha256(open(sums, "rb").read()).hexdigest()
if got_sums != pin_sums:
    print(f"  integrity FAILED:\n    PINNED   {os.path.basename(sums)} digest {got_sums} != pinned {pin_sums} "
          "(the manifest itself changed — re-vendor deliberately by editing PIN_SUMS15)"); sys.exit(1)
man = json.load(open(sums, encoding="utf-8"))
if man.get("schema") != "cortex-package-sha256/1":
    print(f"  manifest schema is {man.get('schema')!r}, expected cortex-package-sha256/1"); sys.exit(1)
listed = man["files"]
if not isinstance(listed, dict) or len(listed) < min_files:
    print(f"  NON-VACUITY: manifest lists {len(listed) if hasattr(listed,'__len__') else '?'} files, floor is {min_files}"); sys.exit(1)

present = set()
links = []
if os.path.islink(pkg.rstrip("/")):
    links.append("SYMLINK  <package root>")
for root, dirs, files in os.walk(pkg):
    # os.walk lists a symlinked directory under `dirs` but does not descend into
    # it, so a link pointing OUTSIDE the package was invisible to this walk
    # (measured: "both directions clean" with specs/extra -> an outside dir).
    # Any link is refused here, by the repo's own walk.
    for n in dirs + files:
        if os.path.islink(os.path.join(root, n)):
            links.append(f"SYMLINK  {os.path.relpath(os.path.join(root, n), pkg)}")
    for f in files:
        present.add(os.path.relpath(os.path.join(root, f), pkg))

# KNOWN DEVIATION, pinned. Exactly one file in this repository's copy differs
# from the upstream manifest, and the cause is the footgun documented below:
# `tools/validate_package.py` writes its report to <root>/package_validation.json
# BY DEFAULT, and that file is itself hash-listed. The intake session ran the
# validator to verify the package (reporting "113/113 files match" -- true when
# measured, false immediately afterwards) and the rewritten file, carrying a
# fresh `validated_at` timestamp, is what got committed in 32937d3.
#
# The file is NOT un-checked here: it is pinned to the digest actually committed,
# so any FURTHER change to it still fails. The deviation lives in this script
# rather than a data file so that adding one requires a reviewable code edit.
# Removing it means re-vendoring the file with its upstream bytes.
# Deliberately EMPTY. A deviation was briefly needed here: the intake run of
# `tools/validate_package.py` defaults `--report` to <root>/package_validation.json,
# which is itself hash-listed, so verifying the package MUTATED it and the
# mutated copy was committed. The cause is fixed in two places instead — the
# pristine bytes are restored, and this gate passes `--report` to target/ so the
# footgun cannot re-fire — which leaves this map with nothing legitimate to
# hold. It stays as a named, reviewable mechanism rather than an implicit one:
# a future deviation must be added here with a reason, not tolerated silently.
DEVIATIONS: dict[str, tuple[str, str]] = {}

bad = list(links)
verified = 0
deviated = []
for rel, want in sorted(listed.items()):
    p = os.path.join(pkg, rel)
    if not os.path.isfile(p):
        bad.append(f"MISSING  {rel}"); continue
    h = hashlib.sha256()
    with open(p, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    got = h.hexdigest()
    if got == want:
        verified += 1
    elif rel in DEVIATIONS and got == DEVIATIONS[rel][0]:
        verified += 1
        deviated.append(f"{rel} — pinned deviation: {DEVIATIONS[rel][1]}")
    else:
        bad.append(f"MISMATCH {rel}\n           recorded {want}\n           actual   {got}")

# Reverse direction: a file ADDED to the package is invisible to a forward-only
# check. The sha256 manifest is the one legitimate exception -- it cannot list
# its own hash.
allowed_unlisted = {os.path.basename(sums)}
unlisted = sorted(present - set(listed) - allowed_unlisted)
for rel in unlisted:
    bad.append(f"UNLISTED {rel}  (present in the package, absent from the manifest)")

if verified < min_files:
    bad.append(f"NON-VACUITY: only {verified} files verified, floor is {min_files}")
if bad:
    print("  integrity FAILED:")
    for b in bad: print("   ", b)
    sys.exit(1)
for d in deviated:
    print(f"  notice: {d}")
print(f"  {verified}/{len(listed)} files match ({len(deviated)} via a pinned deviation); {len(present)} present; "
      f"unlisted: {sorted(allowed_unlisted)} (the manifest itself) — both directions clean")
PY
INTEGRITY15=$?
[ $INTEGRITY15 -eq 0 ] || note_fail "package integrity (SHA256SUMS_v0_15.json)"
# The package's Python runs only on a tree whose bytes were just verified.
# Measured (v0.22 Stage 6, donor lesson 90c4c616): with integrity merely
# RECORDED as failed, a line inserted into tools/validate_package.py still
# executed with the gate's privileges before the verdict. Abort instead: an
# integrity failure means no package code runs at all.
[ $INTEGRITY15 -eq 0 ] || abort "v0.15 integrity failed — refusing to execute code from an unverified package (no package code was executed)"

echo "── cortex: the package's own validator (tools/validate_package.py) ─"
# --report is NOT optional. The validator's default report path is
# <root>/package_validation.json, which is a HASH-LISTED file, and it stamps a
# fresh `validated_at` every run -- running it with defaults would break the
# integrity check above. Write the report to target/ instead.
# A stale PASS report must not survive into this run. Measured: with the
# validator replaced by a no-op that exits 0 without writing, the gate read the
# PREVIOUS run's PASS report from target/ and passed.
rm -f "$REPORT"
if ! python3 -B "$PKG/tools/validate_package.py" --root "$PKG" --report "$REPORT" >/dev/null; then
  note_fail "package validator reported errors (see $REPORT)"
fi
python3 - "$REPORT" <<'PY'
import json, sys
try: r = json.load(open(sys.argv[1], encoding="utf-8"))
except (OSError, ValueError) as e: print(f"  no readable validator report (it must be written by THIS run): {e}"); sys.exit(1)
if r.get("result") != "PASS":
    print("  validator result:", r.get("result"), r.get("errors")); sys.exit(1)
c = r.get("counts", {})
if not c or min(c.values(), default=0) <= 0:
    print(f"  NON-VACUITY: validator counted nothing: {c}"); sys.exit(1)
print(f"  validator PASS; counts={c}; product_gates_executed={r.get('product_gates_executed')}")
PY
[ $? -eq 0 ] || note_fail "package validator self-report"

# The validator must not have written into the package. Re-hash the one file it
# would have clobbered; this is the cheap direct guard for that footgun.
python3 - "$PKG" "$SUMS" <<'PY'
import hashlib, json, os, sys
pkg, sums = sys.argv[1], sys.argv[2]
rel = "package_validation.json"
try:
    want = json.load(open(sums, encoding="utf-8"))["files"][rel]
except (OSError, ValueError, KeyError) as e:
    print(f"  cannot read the recorded digest for {rel}: {e}"); sys.exit(1)
# Same pinned deviation as the integrity pass: what matters here is that the file
# did not change ACROSS the validator run, so compare against whichever digest the
# integrity pass already accepted.
PINNED = "cb2deaf7a7fb64da94d3fca150690715aa6e3b5325b649857ffb03297c01e421"
got = hashlib.sha256(open(os.path.join(pkg, rel), "rb").read()).hexdigest()
if got not in (want, PINNED):
    print(f"  {rel} changed while the validator ran (it wrote into the vendored package)"); sys.exit(1)
PY
[ $? -eq 0 ] || note_fail "validator mutated the vendored package ($PKG/package_validation.json)"

echo "── cortex: honesty invariant (NOT_RUN unless something here runs it) ─"
# One implementation for every vendored package (moved out of this script in
# v0.22 Stage 6 so v0.22 is held to exactly the same registry rule).
python3 -B scripts/cortex_honesty_invariant.py --pkg "$PKG" --execution-registry "$REGISTRY" \
  --min-gates "$MIN_GATES" --min-tasks "$MIN_TASKS" \
  --also-known docs/axon_cortex_v0_22/axon-cortex-build-v0_22
[ $? -eq 0 ] || note_fail "honesty invariant (gate_manifest / task_manifest vs $REGISTRY)"

if [ ${#FAILURES[@]} -gt 0 ]; then
  echo ""
  echo "❌ cortex package gate FAILED (${#FAILURES[@]} check(s)):"
  for f in "${FAILURES[@]}"; do echo "   • $f"; done
  exit 1
fi
echo "✅ cortex package gate: integrity verified in both directions, validator PASS, honesty invariant holds"
