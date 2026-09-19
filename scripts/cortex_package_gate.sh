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
python3 - "$PKG" "$SUMS" "$MIN_FILES" <<'PY'
import hashlib, json, os, sys
pkg, sums, min_files = sys.argv[1], sys.argv[2], int(sys.argv[3])
man = json.load(open(sums, encoding="utf-8"))
if man.get("schema") != "cortex-package-sha256/1":
    print(f"  manifest schema is {man.get('schema')!r}, expected cortex-package-sha256/1"); sys.exit(1)
listed = man["files"]
if not isinstance(listed, dict) or len(listed) < min_files:
    print(f"  NON-VACUITY: manifest lists {len(listed) if hasattr(listed,'__len__') else '?'} files, floor is {min_files}"); sys.exit(1)

present = set()
for root, _dirs, files in os.walk(pkg):
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
DEVIATIONS = {
    "package_validation.json": (
        "cb2deaf7a7fb64da94d3fca150690715aa6e3b5325b649857ffb03297c01e421",
        "upstream digest is adb80ef7...; the intake run of tools/validate_package.py "
        "overwrote this file with a fresh validated_at timestamp before it was committed",
    ),
}

bad = []
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
[ $? -eq 0 ] || note_fail "package integrity (SHA256SUMS_v0_15.json)"

echo "── cortex: the package's own validator (tools/validate_package.py) ─"
# --report is NOT optional. The validator's default report path is
# <root>/package_validation.json, which is a HASH-LISTED file, and it stamps a
# fresh `validated_at` every run -- running it with defaults would break the
# integrity check above. Write the report to target/ instead.
if ! python3 "$PKG/tools/validate_package.py" --root "$PKG" --report "$REPORT" >/dev/null; then
  note_fail "package validator reported errors (see $REPORT)"
fi
python3 - "$REPORT" <<'PY'
import json, sys
r = json.load(open(sys.argv[1], encoding="utf-8"))
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
python3 - "$PKG" "$REGISTRY" "$MIN_GATES" "$MIN_TASKS" <<'PY'
import json, os, sys
pkg, registry_path, min_gates, min_tasks = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])

DEFAULT_RESULT = "NOT_RUN"
DEFAULT_STATUS = "Not started"
DONE_STATUSES = {"Done", "Complete", "Completed", "Landed"}

gates = json.load(open(os.path.join(pkg, "gate_manifest.json"), encoding="utf-8"))["gates"]
tasks = json.load(open(os.path.join(pkg, "task_manifest.json"), encoding="utf-8"))["tasks"]
reg = json.load(open(registry_path, encoding="utf-8"))

errors, notices = [], []

if reg.get("schema") != "cortex-gate-execution/1":
    errors.append(f"{registry_path}: schema is {reg.get('schema')!r}, expected cortex-gate-execution/1")
reg_gates = {r.get("gate_id"): r for r in reg.get("gates", [])}
reg_tasks = {r.get("task_id"): r for r in reg.get("tasks", [])}

if len(gates) < min_gates:
    errors.append(f"NON-VACUITY: parsed {len(gates)} gates, floor is {min_gates}")
if len(tasks) < min_tasks:
    errors.append(f"NON-VACUITY: parsed {len(tasks)} work packages, floor is {min_tasks}")

def registry_row_is_real(row, what):
    """A row may only vouch for a gate if it names a file that EXISTS and is
    INVOKED. Grepping the invoker for the executed path is what stops this
    registry from being a place to declare things green."""
    bad = []
    ex = row.get("executed_by")
    inv = row.get("invoked_by")
    if not ex or not os.path.isfile(ex):
        bad.append(f"{what}: executed_by {ex!r} does not exist in this repo")
        return bad
    if not os.access(ex, os.X_OK) and not ex.endswith((".rs", ".py")):
        bad.append(f"{what}: executed_by {ex!r} is not executable")
    if not inv or not os.path.isfile(inv):
        bad.append(f"{what}: invoked_by {inv!r} does not exist in this repo")
        return bad
    try:
        text = open(inv, encoding="utf-8", errors="replace").read()
    except OSError as e:
        bad.append(f"{what}: cannot read invoker {inv!r}: {e}")
        return bad
    if ex not in text and os.path.basename(ex) not in text:
        bad.append(f"{what}: {inv} does not invoke {ex} — a registry row may not "
                   f"vouch for a script nothing runs (orphaned-gate class)")
    return bad

n_offdefault_gates = 0
for g in gates:
    gid, res = g.get("id"), g.get("product_result")
    if res is None:
        errors.append(f"gate {gid}: no product_result field")
        continue
    if res == DEFAULT_RESULT:
        continue
    n_offdefault_gates += 1
    row = reg_gates.get(gid)
    if row is None:
        errors.append(f"gate {gid}: product_result={res!r} but no row in {registry_path}. "
                      f"The package may only claim a gate ran if something in this repo runs it.")
        continue
    # (structural validity of the row is checked once, over all rows, below)
    allowed = row.get("allowed_product_result") or []
    if res not in allowed:
        errors.append(f"gate {gid}: product_result={res!r} not in registry allowed_product_result {allowed}")

n_offdefault_tasks = 0
for t in tasks:
    tid, st = t.get("id"), t.get("status")
    if st is None:
        errors.append(f"task {tid}: no status field")
        continue
    if st == DEFAULT_STATUS:
        continue
    n_offdefault_tasks += 1
    row = reg_tasks.get(tid)
    if row is None:
        errors.append(f"task {tid}: status={st!r} but no row in {registry_path}.")
        continue
    allowed = row.get("allowed_status") or []
    if st not in allowed:
        errors.append(f"task {tid}: status={st!r} not in registry allowed_status {allowed}")
    # A work package declared DONE implies its gate targets ran. Require each to
    # be registry-backed too, so "Done" cannot launder an unexecuted gate.
    if st in DONE_STATUSES:
        for gt in t.get("gate_targets", []):
            if gt not in reg_gates:
                errors.append(f"task {tid}: status={st!r} but gate target {gt} has no registry row")

# Stale registry rows are a notice, not a failure: wiring a real gate BEFORE the
# vendored manifest is re-vendored is legitimate and harmless.
gate_ids = {g.get("id") for g in gates}
task_ids = {t.get("id") for t in tasks}
for gid in sorted(set(reg_gates) - gate_ids):
    notices.append(f"registry row for unknown gate {gid} (not in gate_manifest.json)")
for tid in sorted(set(reg_tasks) - task_ids):
    notices.append(f"registry row for unknown task {tid} (not in task_manifest.json)")
# EVERY registry row is validated, not just the ones currently vouching for an
# off-default result. A row whose script was deleted or unwired must break the
# day it rots, not the day someone flips the manifest -- otherwise the registry
# silently becomes the rubber stamp it exists to prevent.
for gid, row in sorted(reg_gates.items()):
    errors.extend(registry_row_is_real(row, f"registry row {gid}"))
    if gid in gate_ids and next(g for g in gates if g.get("id") == gid).get("product_result") == DEFAULT_RESULT:
        notices.append(f"{gid} is executed by {row.get('executed_by')} in this repo while the vendored "
                       f"manifest still says NOT_RUN — correct and expected: NOT_RUN is a true statement "
                       f"about the PACKAGE, and only a re-vendored upstream release may change it")

for n in notices:
    print(f"  notice: {n}")
if errors:
    print("  honesty invariant VIOLATED:")
    for e in errors:
        print("   ", e)
    sys.exit(1)
print(f"  {len(gates)} gates, {len(tasks)} work packages parsed; "
      f"{n_offdefault_gates} gates off {DEFAULT_RESULT} and {n_offdefault_tasks} tasks off '{DEFAULT_STATUS}', "
      f"all registry-backed; registry has {len(reg_gates)} gate row(s), {len(reg_tasks)} task row(s)")
PY
[ $? -eq 0 ] || note_fail "honesty invariant (gate_manifest / task_manifest vs $REGISTRY)"

if [ ${#FAILURES[@]} -gt 0 ]; then
  echo ""
  echo "❌ cortex package gate FAILED (${#FAILURES[@]} check(s)):"
  for f in "${FAILURES[@]}"; do echo "   • $f"; done
  exit 1
fi
echo "✅ cortex package gate: integrity verified in both directions, validator PASS, honesty invariant holds"
