#!/usr/bin/env bash
# cortex_package_gate_v022.sh — continuous check for the vendored Cortex v0.22
# build package. Runs ALONGSIDE scripts/cortex_package_gate.sh (v0.15), never
# instead of it: G00-r22-package-gate-upgrade requires the older package and its
# gate to be retained.
#
# ORDER IS THE POINT. The package ships Python tools, and its own
# tools/repo_gate_v022.py says "authenticate the received package independently
# before executing any tool included in it". So:
#
#   1. INTEGRITY, by code in THIS file (stdlib only, no package import):
#      SHA256SUMS_v0_22.json (schema axon-package-sha256/1) is checked in BOTH
#      directions, honouring exactly its declared `excluded` list and nothing
#      else. The manifest itself and the excluded receipt are PINNED by digest
#      here, because a forward/reverse check against a manifest that can be
#      rewritten alongside the file it lists proves nothing (that is precisely
#      what `validate_package.py --refresh-hashes` would do — this script never
#      passes it). Re-vendoring means editing the two pins below: a reviewable
#      code change, not a silent regeneration.
#      Any failure here ABORTS before a single line of package code runs.
#
#   2. HONESTY, stdlib only, repo code only: a gate may leave NOT_RUN and a
#      work package leave "Not started" only with a row in
#      governance/cortex_gate_execution_registry.json naming an existing script
#      that an existing invoker is grepped to run — the v0.15 rule, now shared
#      code (scripts/cortex_honesty_invariant.py), with every row re-validated
#      each run. The three runtime-qualification booleans in
#      integration/V022_RUNTIME_QUALIFICATION.json stay false. Repo-side
#      execution belongs in the registry, never in the vendored bytes (which
#      are hash-pinned, and whose own validator rejects any non-NOT_RUN row).
#
#   3. The package's own validator (tools/validate_package.py), READ-ONLY:
#      --output goes to the target dir, never --refresh-hashes, python -B with
#      bytecode writing disabled, and the whole tree (every file, including the
#      excluded ones) is digested before and after — a validator that wrote into
#      the pack fails this gate. Its counts are held to FLOORS measured on the
#      vendored copy: specs 37, tasks 287, proposed_gates 504,
#      hashed_files_checked 337, and orphan_gates must be exactly 0.
#
#   4. The package's offline unit suite, ONLY with its pinned dependency
#      (jsonschema==4.26.0, tools/requirements-review.txt). Nothing is installed
#      globally. The interpreter is, in order: $CORTEX_V022_PYTHON; a venv at
#      ${CARGO_TARGET_DIR:-target}/cortex-v022-venv (created by this script only
#      when CORTEX_V022_BOOTSTRAP_VENV=1, since that needs the network); or the
#      system python3 if it already carries exactly 4.26.0. Otherwise the suite
#      is a legible SKIP — recorded to target/harness-skips.log and FATAL under
#      AXON_HARNESS_STRICT=1, the same convention cli_run.rs uses.
#
# tools/check_v022_runtime_evidence.py is deliberately NOT run: it has no
# success exit code (2 = NOT_QUALIFIED, 3 = REQUIRES_EXTERNAL_VERIFICATION), so
# it cannot be a pass/fail stage. Mapping either code to PASS would be a lie.
#
# Output contract (read by gate.sh): the LAST line is one of
#   cortex_package_gate_v022: PASS — …
#   cortex_package_gate_v022: FAIL — …        (exit 1)
# A skipped offline suite still ends in PASS (integrity/honesty/validator ran)
# but the PASS line itself says "offline suite SKIPPED", and a SKIP line
# precedes it.
#
# Usage: scripts/cortex_package_gate_v022.sh [--pkg DIR]
#   --pkg DIR  check a different copy (used for the deliberate-tamper negative;
#              the pins still apply, so a modified copy must fail).
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT="$(pwd)"

PKG="docs/axon_cortex_v0_22/axon-cortex-build-v0_22"
while [ $# -gt 0 ]; do
  case "$1" in
    --pkg) PKG="$2"; shift 2 ;;
    *) echo "cortex_package_gate_v022: unknown argument $1" >&2; exit 2 ;;
  esac
done
SUMS_NAME="SHA256SUMS_v0_22.json"
REGISTRY="governance/cortex_gate_execution_registry.json"

# Pins — measured on the vendored copy (docs/axon_cortex_v0_22, 2026-09-24).
PIN_SUMS="b19c0401d1d8d38af74afc71b08b413204bca13c2686cda139199db97e39d891"
PIN_RECEIPT="9473690f4981f425cd68b512b9f742d7fe9a0c226eda4ede05251293f348d7f5"  # package_validation.json
EXPECTED_EXCLUDED="SHA256SUMS_v0_22.json,package_validation.json"
# Floors — minimums, not exact values (a re-vendored release may grow), except
# orphans, whose only honest value is zero.
MIN_SPECS=37; MIN_TASKS=287; MIN_GATES=504; MIN_HASHED=337; MIN_SUITE_TESTS=567
PINNED_JSONSCHEMA="4.26.0"

TGT="${CARGO_TARGET_DIR:-$ROOT/target}"
OUTDIR="$TGT/cortex-v022"
SKIPLOG="$ROOT/target/harness-skips.log"
mkdir -p "$OUTDIR" "$ROOT/target"
# Bytecode must never land in the vendored pack (artifact admission would then
# see __pycache__, and the tree digest below would change).
export PYTHONDONTWRITEBYTECODE=1
export PYTHONPYCACHEPREFIX="$OUTDIR/pycache"

NAME=cortex_package_gate_v022
fail_now() { echo "$NAME: FAIL — $1"; exit 1; }
FAILURES=()
note_fail() { echo "   ↳ FAILED: $1"; FAILURES+=("$1"); }

# Self-check: the package's hash-rewriting tools may not appear as COMMANDS in
# this script, in any spelling argparse accepts. Neither tool sets
# allow_abbrev=False, so an unambiguous PREFIX is the flag (measured on the
# vendored tools): validate_package.py has --root and --refresh-hashes, so
# `--re`, `--ref`, `--refr` … all reseal SHA256SUMS; package_views.py has
# --root and --write, so `--w` rewrites four hash-listed views; and
# tools/run_review_tests.py rewrites review/TEST_LOG.txt + TEST_RESULTS.json on
# every invocation. The sums pin catches the RESULT on the next run; this makes
# the script refuse to be the thing that does it. Patterns are split so this
# line does not match itself; comment lines are exempt.
if grep -nE '^[^#]*(-''-re[a-z-]*|run_review''_tests|package_views\.py[^#]*-''-w[a-z-]*)' "$0" >&2; then
  fail_now "this script invokes a tool that rewrites hash-listed package files (see the line above)"
fi

command -v python3 >/dev/null 2>&1 || fail_now "python3 not found (required to verify the package)"
[ -d "$PKG" ] || fail_now "package root missing: $PKG"
[ -f "$PKG/$SUMS_NAME" ] || fail_now "checksum manifest missing: $PKG/$SUMS_NAME"
PKG="$(cd "$PKG" && pwd)"

tree_digest() {  # every file, excluded ones included, plus the path list
  python3 -B - "$1" <<'PY'
import hashlib, os, sys
root = sys.argv[1]; h = hashlib.sha256()
for d, dirs, files in os.walk(root):
    dirs.sort()
    for f in sorted(files):
        p = os.path.join(d, f)
        h.update(os.path.relpath(p, root).encode() + b"\0")
        with open(p, "rb") as fh: h.update(hashlib.sha256(fh.read()).digest())
print(h.hexdigest())
PY
}

# ── 1. integrity (BEFORE any package code) ───────────────────────────────────
echo "── cortex v0.22: package integrity ($SUMS_NAME, both directions, pinned) ──"
if ! python3 -B - "$PKG" "$SUMS_NAME" "$PIN_SUMS" "$PIN_RECEIPT" "$EXPECTED_EXCLUDED" "$MIN_HASHED" <<'PY'
import hashlib, json, os, sys
pkg, sums_name, pin_sums, pin_receipt, expected_excl, min_hashed = sys.argv[1:7]
min_hashed = int(min_hashed)
def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as fh:
        for c in iter(lambda: fh.read(1 << 20), b""): h.update(c)
    return h.hexdigest()
bad = []
sums_path = os.path.join(pkg, sums_name)
if sha(sums_path) != pin_sums:
    bad.append(f"PINNED  {sums_name} digest {sha(sums_path)} != pinned {pin_sums} "
               "(the manifest itself changed — re-vendor deliberately by editing the pin)")
man = json.load(open(sums_path, encoding="utf-8"))
if man.get("schema") != "axon-package-sha256/1":
    bad.append(f"schema is {man.get('schema')!r}, expected axon-package-sha256/1")
excluded = man.get("excluded")
if not isinstance(excluded, list) or set(excluded) != set(expected_excl.split(",")):
    bad.append(f"excluded list is {excluded!r}, expected exactly {expected_excl.split(',')}")
    excluded = []
listed = man.get("files")
if not isinstance(listed, dict):
    print("  integrity FAILED: `files` is not a map"); sys.exit(1)
present = set()
for d, dirs, files in os.walk(pkg):
    # os.walk does not descend into a symlinked directory and lists it under
    # `dirs`, so checking islink on files alone let a linked directory carry
    # unlisted content past this step and into the offline suite (red-team
    # D-01). A symlink anywhere in the pack is refused.
    for sub in dirs:
        if os.path.islink(os.path.join(d, sub)):
            bad.append(f"SYMLINK  {os.path.relpath(os.path.join(d, sub), pkg)}/ (directory)")
    for f in files:
        p = os.path.join(d, f); rel = os.path.relpath(p, pkg)
        if os.path.islink(p): bad.append(f"SYMLINK  {rel}")
        if "__pycache__" in rel.split(os.sep) or rel.endswith((".pyc", ".pyo")):
            bad.append(f"PYCACHE  {rel} (generated output inside the vendored pack)")
        present.add(rel)
for x in excluded:
    if x not in present: bad.append(f"MISSING  excluded file {x}")
rp = os.path.join(pkg, "package_validation.json")
if os.path.isfile(rp) and sha(rp) != pin_receipt:
    bad.append(f"PINNED  package_validation.json digest {sha(rp)} != pinned {pin_receipt}")
verified = 0
for rel, want in sorted(listed.items()):
    if rel in excluded: bad.append(f"LISTED-AND-EXCLUDED {rel}"); continue
    p = os.path.join(pkg, rel)
    if not os.path.isfile(p): bad.append(f"MISSING  {rel}"); continue
    got = sha(p)
    if got == want: verified += 1
    else: bad.append(f"MISMATCH {rel}\n           recorded {want}\n           actual   {got}")
for rel in sorted(present - set(listed) - set(excluded)):
    bad.append(f"UNLISTED {rel}  (present in the package, absent from the manifest)")
if verified < min_hashed:
    bad.append(f"NON-VACUITY: {verified} files verified, floor is {min_hashed}")
if bad:
    print("  integrity FAILED:")
    for b in bad: print("   ", b)
    sys.exit(1)
print(f"  {verified}/{len(listed)} files match; {len(present)} present; excluded {sorted(excluded)} "
      f"honoured (both pinned); both directions clean")
PY
then
  # Abort, do not accumulate: nothing below may execute package code on a pack
  # that failed authentication.
  fail_now "package integrity — no package code was executed"
fi

# ── 2. honesty invariant (stdlib only) ───────────────────────────────────────
echo "── cortex v0.22: honesty invariant (NOT_RUN unless something here runs it; unqualified) ──"
# (a) The runtime-qualification ledger stays unqualified and the spec count
#     holds its floor. Stdlib only.
python3 -B - "$PKG" "$MIN_SPECS" <<'PY' || note_fail "honesty invariant (runtime qualification / spec floor)"
import json, os, sys
pkg, min_specs = sys.argv[1], int(sys.argv[2])
ld = lambda n: json.load(open(os.path.join(pkg, n), encoding="utf-8"))
specs, q = ld("spec_manifest.json")["specs"], ld("integration/V022_RUNTIME_QUALIFICATION.json")
err = []
if len(specs) < min_specs: err.append(f"NON-VACUITY: {len(specs)} specs < {min_specs}")
for k in ("engineering_qualified", "policy_activated", "measured_improvement_supported"):
    if q.get(k) is not False: err.append(f"V022_RUNTIME_QUALIFICATION.{k} = {q.get(k)!r}, must be false")
if q.get("live_evidence"): err.append("V022_RUNTIME_QUALIFICATION.live_evidence is non-empty")
if q.get("qualified_profiles"): err.append("V022_RUNTIME_QUALIFICATION.qualified_profiles is non-empty")
if err:
    print("  honesty invariant VIOLATED:"); [print("   ", e) for e in err]; sys.exit(1)
print(f"  {len(specs)} specs; runtime qualification booleans false, no live evidence, no qualified profile")
PY
# (b) The v0.15 registry rule, shared code (scripts/cortex_honesty_invariant.py):
#     a gate off NOT_RUN / a task off "Not started" needs a registry row naming
#     an existing script that an existing invoker greps as invoked, and EVERY
#     row is re-validated each run. Before Stage 6 this script asserted the
#     constants but never read the registry, so a row vouching for an r22 gate
#     through a script nothing runs passed here (measured).
[ -f "$REGISTRY" ] || fail_now "execution registry missing: $REGISTRY"
python3 -B scripts/cortex_honesty_invariant.py --pkg "$PKG" --execution-registry "$REGISTRY" \
  --min-gates "$MIN_GATES" --min-tasks "$MIN_TASKS" \
  --also-known docs/axon_cortex_v0_15/axon-cortex-build-v0_15 \
  || note_fail "honesty invariant (gate_manifest / task_manifest vs $REGISTRY)"

# ── 2b. pack coherence the package validator does not check (stdlib) ─────────
# G00-r22-pack-integrity asks that "every new task/gate has a source-derived or
# explicitly proposed rationale and execution recipe". tools/validate_package.py
# checks the manifests, owner exports, dependency closures, generated views,
# parent bytes and Fabric bytes (stage 3), but NOT this clause: it never reads
# a requirement's `basis` or the acceptance text in build/WORK_PACKAGES_V022.md.
# It trips on such an edit only INCIDENTALLY (stale AXON_CORTEX_MASTER.md view,
# owner-export digest), and both are regenerable. Measured on a scratch copy:
# B284's acceptance line for G00-r22-pack-integrity deleted AND B284's basis
# blanked, then the owner-export lock re-digested, the views regenerated
# (package_views.py --write), the sums resealed and the pins moved — validator
# PASS, 567-test suite PASS, gate PASS. So it is checked here: each of the 32 r22 work
# packages has a requirement row with a non-empty basis and a WORK_PACKAGES
# section carrying a numbered implementation sequence (the recipe) and, for
# every gate target, the gate's manifest text verbatim; every r22 gate is owned
# by exactly such a section.
echo "── cortex v0.22: pack coherence (every r22 task/gate has rationale + recipe) ──"
python3 -B - "$PKG" <<'PY' || note_fail "pack coherence (r22 rationale / execution recipe)"
import json, os, re, sys
pkg = sys.argv[1]
ld = lambda n: json.load(open(os.path.join(pkg, n), encoding="utf-8"))
gd = {g["id"]: g for g in ld("gate_manifest.json")["gates"]}
new = {t["id"]: t for t in ld("integration/V022_WORK_PACKAGES.json")["tasks"]}
reqs = ld("integration/V022_REQUIREMENTS.json")["requirements"]
wp = open(os.path.join(pkg, "build/WORK_PACKAGES_V022.md"), encoding="utf-8").read()
sections = {m.group(1): m for m in re.finditer(r"^## (B\d+) — .*$", wp, re.M)}
starts = sorted(m.start() for m in sections.values()) + [len(wp)]
body = {tid: wp[m.start():starts[starts.index(m.start()) + 1]] for tid, m in sections.items()}
err, covered = [], set()
if len(new) < 32: err.append(f"NON-VACUITY: {len(new)} r22 work packages < 32")
basis = {}
for r in reqs: basis.setdefault(r.get("task"), []).append((r.get("basis") or "").strip())
for tid, t in sorted(new.items()):
    if not basis.get(tid) or not all(basis[tid]): err.append(f"{tid}: requirement row missing or empty `basis` (rationale)")
    sec = body.get(tid)
    if sec is None: err.append(f"{tid}: no '## {tid} — …' section in build/WORK_PACKAGES_V022.md"); continue
    rec = sec.split("### Implementation sequence", 1)
    if len(rec) < 2 or not re.search(r"^1\. \S", rec[1].split("\n### ", 1)[0], re.M):
        err.append(f"{tid}: no numbered '### Implementation sequence' (execution recipe)")
    for gid in t["gate_targets"]:
        g = gd.get(gid)
        if g is None: err.append(f"{tid}: gate target {gid} not in gate_manifest.json"); continue
        if f"**{gid}** — {g['description']}" not in sec:
            err.append(f"{tid}: acceptance line for {gid} missing or differs from the manifest text")
        else: covered.add(gid)
r22 = {g for g in gd if "-r22-" in g}
for gid in sorted(r22 - covered): err.append(f"{gid}: no work-package section carries its acceptance text")
if len(r22) < 80: err.append(f"NON-VACUITY: {len(r22)} r22 gates < 80")
if err:
    print("  pack coherence FAILED:"); [print("   ", e) for e in err[:20]]; sys.exit(1)
print(f"  {len(new)} r22 work packages: basis + numbered recipe + verbatim acceptance text; {len(covered)}/{len(r22)} r22 gates covered")
PY

# G00-r22-package-gate-upgrade: "an older vendored pack is retained until
# references migrate deliberately". Checked, not assumed: the v0.15 pack and its
# gate must still exist and scripts/gate.sh must still invoke that gate.
OLD_PKG="docs/axon_cortex_v0_15/axon-cortex-build-v0_15"
if [ ! -f "$OLD_PKG/SHA256SUMS_v0_15.json" ] || [ ! -x scripts/cortex_package_gate.sh ] \
   || ! grep -qE '^[^#]*\./scripts/cortex_package_gate\.sh' scripts/gate.sh; then
  note_fail "the older v0.15 pack or its gate is no longer retained and invoked (G00-r22-package-gate-upgrade)"
else
  echo "  older pack retained: $OLD_PKG, gated by scripts/cortex_package_gate.sh (invoked by scripts/gate.sh)"
fi

# Resolve the pinned interpreter once; the validator prefers it too.
has_pin() { "$1" -B -c "import importlib.metadata as m, sys; sys.exit(0 if m.version('jsonschema') == '$PINNED_JSONSCHEMA' else 1)" 2>/dev/null; }
VENV="$TGT/cortex-v022-venv"
PY=""
if [ -n "${CORTEX_V022_PYTHON:-}" ]; then
  has_pin "$CORTEX_V022_PYTHON" && PY="$CORTEX_V022_PYTHON" \
    || note_fail "CORTEX_V022_PYTHON=$CORTEX_V022_PYTHON does not carry jsonschema==$PINNED_JSONSCHEMA"
else
  if [ ! -x "$VENV/bin/python" ] && [ "${CORTEX_V022_BOOTSTRAP_VENV:-}" = 1 ]; then
    python3 -m venv "$VENV" >/dev/null 2>&1 \
      && "$VENV/bin/pip" install -q "jsonschema==$PINNED_JSONSCHEMA" >/dev/null 2>&1 \
      || echo "   (venv bootstrap at $VENV failed)"
  fi
  if [ -x "$VENV/bin/python" ] && has_pin "$VENV/bin/python"; then PY="$VENV/bin/python"
  elif has_pin python3; then PY="python3"
  fi
fi
# ── 3. the package's own validator, read-only ────────────────────────────────
echo "── cortex v0.22: tools/validate_package.py (read-only, floors) ──"
REPORT="$OUTDIR/package-validation.json"
rm -f "$REPORT"
BEFORE="$(tree_digest "$PKG")"
VAL_OUT="$OUTDIR/validator.log"
VPY="${PY:-python3}"
if "$VPY" -B -c 'import jsonschema' 2>/dev/null; then
  echo "  interpreter: $VPY (jsonschema $("$VPY" -B -c 'import importlib.metadata as m;print(m.version("jsonschema"))'))"
  ( cd "$PKG" && "$VPY" -B tools/validate_package.py --root "$PKG" --output "$REPORT" ) >"$VAL_OUT" 2>&1 \
    || note_fail "validator exited non-zero (see $VAL_OUT)"
  AFTER="$(tree_digest "$PKG")"
  [ "$BEFORE" = "$AFTER" ] || note_fail "validator MUTATED the vendored pack (tree digest changed)"
  python3 -B - "$REPORT" "$MIN_SPECS" "$MIN_TASKS" "$MIN_GATES" "$MIN_HASHED" <<'PY' || note_fail "validator report below floors / not PASS"
import json, sys
try: r = json.load(open(sys.argv[1], encoding="utf-8"))
except (OSError, ValueError) as e: print(f"  no readable validator report: {e}"); sys.exit(1)
ms, mt, mg, mh = map(int, sys.argv[2:6]); c = r.get("counts", {}); err = []
if r.get("result") != "PASS": err.append(f"result={r.get('result')!r} errors={r.get('errors', [])[:5]}")
if r.get("integrity_checked") is not True: err.append("validator did not check integrity")
if r.get("product_gates_executed") != 0: err.append(f"product_gates_executed={r.get('product_gates_executed')!r}")
for k, floor in (("specs", ms), ("tasks", mt), ("proposed_gates", mg), ("hashed_files_checked", mh)):
    if not isinstance(c.get(k), int) or c[k] < floor: err.append(f"NON-VACUITY: {k}={c.get(k)!r} < {floor}")
if c.get("orphan_gates") != 0: err.append(f"orphan_gates={c.get('orphan_gates')!r}, must be 0")
if err: print("  validator:"); [print("   ", e) for e in err]; sys.exit(1)
print(f"  validator PASS; specs={c['specs']} tasks={c['tasks']} gates={c['proposed_gates']} "
      f"hashed={c['hashed_files_checked']} orphans={c['orphan_gates']}; pack unchanged")
PY
else
  # The validator itself imports jsonschema; without it the pack is still
  # authenticated (step 1) and honest (step 2), but the validator measured
  # nothing and must say so.
  echo "$NAME: validator SKIP — python3 has no jsonschema module"
  echo "$NAME (validator)" >> "$SKIPLOG"
  [ "${AXON_HARNESS_STRICT:-}" = 1 ] && note_fail "validator SKIPPED under AXON_HARNESS_STRICT=1"
  VAL_SKIPPED=1
fi

# ── 4. the offline unit suite, only with its pinned dependency ───────────────
echo "── cortex v0.22: package offline suite (jsonschema==$PINNED_JSONSCHEMA) ──"
SUITE="not run"
if [ -n "$PY" ]; then
  SUITE_OUT="$OUTDIR/suite.log"
  # OFFLINE, two layers. (a) In-process: the suite is started through a guard
  # that makes every socket connect / sendto / name lookup raise, so a test that
  # reaches for the network ERRORS rather than passing on whatever the network
  # happened to return (measured before this: a test connecting to pypi.org:443
  # passed and the gate said PASS). (b) Where the host allows it, a fresh network
  # namespace (`unshare -n`, else `unshare -rn`), which also covers child
  # processes the in-process guard cannot see. (b) is probed by actually running
  # the interpreter against the pack, because `unshare -rn` as root cannot read a
  # home directory owned by another uid; the mode used is printed, never implied.
  # -B -E -s: no bytecode, no PYTHON* env steering (PYTHONPATH/PYTHONSTARTUP),
  # no user site-packages.
  read -r -d '' OFFLINE_GUARD <<'GUARD'
import socket, unittest
_MSG = "network disabled: the v0.22 package suite is run OFFLINE by cortex_package_gate_v022.sh"
def _deny(*a, **k): raise OSError(_MSG)
class _Offline(socket.socket):
    def connect(self, *a): raise OSError(_MSG)
    connect_ex = connect
    def sendto(self, *a): raise OSError(_MSG)
socket.socket = _Offline
socket.create_connection = socket.getaddrinfo = socket.gethostbyname = socket.gethostbyname_ex = _deny
unittest.main(module=None, argv=["unittest", "discover", "-s", "tests"])
GUARD
  NETNS=()
  for cand in "unshare -n" "unshare -rn"; do
    # shellcheck disable=SC2086
    if command -v unshare >/dev/null 2>&1 && $cand "$PY" -B -c 'import os, sys; os.listdir(sys.argv[1])' "$PKG" >/dev/null 2>&1; then
      read -r -a NETNS <<<"$cand"; break
    fi
  done
  OFFLINE_MODE="in-process socket guard"; [ ${#NETNS[@]} -gt 0 ] && OFFLINE_MODE="$OFFLINE_MODE + ${NETNS[*]}"
  echo "  offline: $OFFLINE_MODE"
  BEFORE="$(tree_digest "$PKG")"
  ( cd "$PKG" && "${NETNS[@]}" "$PY" -B -E -s -c "$OFFLINE_GUARD" ) >"$SUITE_OUT" 2>&1; SRC=$?
  AFTER="$(tree_digest "$PKG")"
  [ "$BEFORE" = "$AFTER" ] || note_fail "offline suite MUTATED the vendored pack (tree digest changed)"
  if find "$PKG" \( -name __pycache__ -o -name '*.pyc' -o -name '*.pyo' \) | grep -q .; then
    note_fail "Python bytecode present inside the vendored pack after the suite"
  fi
  # A skip is not a pass, and neither is an expected failure or an unexpected
  # success: the summary line must be exactly "OK". Measured before this: a test
  # marked @unittest.skip ("needs live KVM") ran as "568 tests OK (1 skipped)"
  # and the gate said PASS — the NOT_RUN-dressed-as-green shape G00-r22-honest-
  # status exists to forbid.
  if ! SUITE="$(python3 -B - "$SUITE_OUT" "$SRC" "$MIN_SUITE_TESTS" <<'PY'
import re, sys
log, rc, floor = open(sys.argv[1], encoding="utf-8", errors="replace").read(), int(sys.argv[2]), int(sys.argv[3])
m = re.findall(r"^Ran (\d+) tests? in", log, re.M)
ran = int(m[-1]) if m else 0
lines = [l for l in log.strip().splitlines() if l.strip()]
tail = lines[-1].strip() if lines else ""
err = []
if rc != 0: err.append(f"unittest exited {rc}")
if ran < floor: err.append(f"NON-VACUITY: ran {ran} tests, floor is {floor}")
if tail != "OK": err.append(f"summary line is {tail!r}, expected exactly 'OK' (a skip, expected failure or unexpected success is not a pass)")
if err:
    for e in err: print(e)
    sys.exit(1)
print(f"{ran} tests OK, 0 skipped")
PY
)"; then
    tail -n 12 "$SUITE_OUT" | sed 's/^/     /'
    printf '%s\n' "$SUITE" | sed 's/^/  /'
    note_fail "offline suite not clean (see $SUITE_OUT)"
    SUITE="FAILED"
  else
    SUITE="$SUITE, offline ($OFFLINE_MODE), under $("$PY" -B -c 'import sys;print(sys.executable)')"
    echo "  $SUITE"
  fi
else
  echo "  offline suite needs jsonschema==$PINNED_JSONSCHEMA (have: $(python3 -B -c 'import importlib.metadata as m;print(m.version("jsonschema"))' 2>/dev/null || echo none))."
  echo "  Provide it without a global install: CORTEX_V022_BOOTSTRAP_VENV=1 (venv at $VENV) or CORTEX_V022_PYTHON=<python>."
  echo "$NAME: offline suite SKIP — pinned jsonschema==$PINNED_JSONSCHEMA not available"
  echo "$NAME (offline suite)" >> "$SKIPLOG"
  [ "${AXON_HARNESS_STRICT:-}" = 1 ] && note_fail "offline suite SKIPPED under AXON_HARNESS_STRICT=1"
  SUITE="SKIPPED"
fi

if [ ${#FAILURES[@]} -gt 0 ]; then
  echo "$NAME: FAIL — ${#FAILURES[@]} check(s): $(IFS='; '; echo "${FAILURES[*]}")"
  exit 1
fi
VAL="validator PASS"; [ "${VAL_SKIPPED:-0}" = 1 ] && VAL="validator SKIPPED"
# The scope is part of the verdict (G00-r22-honest-status): an offline pass here
# is documentation/reference conformance, not a product gate.
echo "  scope: package conformance only — 0 product gates executed; NOT Rust implementation, physical backend evidence, real MiCode interoperability or measured self-improvement"
echo "$NAME: PASS — integrity pinned + both directions, honesty holds, coherence holds, $VAL; offline suite: $SUITE"
