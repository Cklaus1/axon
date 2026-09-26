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
#        scripts/release_check.sh --stage v022-stage5 [<axon commit>] [--micode-dir DIR]
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2
RUNS="$ROOT/.axon-runs"

# ── STAGE mode ──────────────────────────────────────────────────────────────
#   scripts/release_check.sh --stage v022-stage5 [<axon commit>] [--micode-dir DIR]
# Certifies a STAGE on an exact Axon + MiCode PAIR from a `gate.sh --strict
# --profile=v022-stage5` receipt. It is NOT a release verdict: it prints
# "STAGE-5 VERIFIED", never "RELEASE-VERIFIED", and repository-wide blockers
# (open false greens, unsigned B263, NOT_RUN package gates) are not its subject.
if [ "${1:-}" = "--stage" ]; then
  STAGE="${2:-}"; shift 2 || true
  [ "$STAGE" = v022-stage5 ] || { echo "unknown stage: ${STAGE:-<none>} (known: v022-stage5)"; exit 2; }
  AX_REF="HEAD"; MC_DIR="${MICODE_DIR:-}"
  while [ $# -gt 0 ]; do
    case "$1" in
      --micode-dir) MC_DIR="${2:?--micode-dir needs a path}"; shift 2 ;;
      *) AX_REF="$1"; shift ;;
    esac
  done
  TARGET="$(git rev-parse "$AX_REF" 2>/dev/null)" || { echo "not a commit: $AX_REF"; exit 2; }
  fails=0
  sbad() { printf '  %-6s %s\n' FAIL "$1"; fails=$((fails+1)); }
  sok()  { printf '  %-6s %s\n' ok "$1"; }
  echo "── stage check: $STAGE, axon $(git rev-parse --short "$TARGET") ─────────────────────"
  [ -z "$(git status --porcelain)" ] && sok "axon working tree clean" || sbad "axon working tree dirty"
  [ "$(git rev-parse HEAD)" = "$TARGET" ] && sok "axon HEAD is the target" || sbad "axon HEAD is not the target"
  run=""
  for d in "$RUNS"/*/; do
    r="$d/receipt"; [ -f "$r" ] || continue
    grep -q "^head=$TARGET$" "$r" && grep -q "^gate_run=yes$" "$r" && grep -q "^gate_strict=yes$" "$r" \
      && grep -q "^gate_profile=$STAGE$" "$r" || continue
    scripts/run_managed.sh verify "${d%/}" --for "$TARGET" >/dev/null 2>&1 && { run="${d%/}"; break; }
  done
  if [ -z "$run" ]; then
    sbad "no citable strict --profile=$STAGE gate receipt for this axon commit"
  else
    sok "stage receipt: $(basename "$run")"
    [ -z "$(sed -n 's/^env_axon_names=//p' "$run/receipt")" ] && sok "clean AXON_* environment" \
      || sbad "the gate ran with AXON_* set: $(sed -n 's/^env_axon_names=//p' "$run/receipt")"
    out="$(python3 -B - "$run" "$TARGET" "$MC_DIR" <<'PY'
import hashlib, json, os, subprocess, sys
run, target, mc_dir = sys.argv[1], sys.argv[2], sys.argv[3]
rc = dict(l.rstrip("\n").split("=", 1) for l in open(os.path.join(run, "receipt")) if "=" in l)
out = []
def bad(m): out.append("FAIL " + m)
def ok(m): out.append("ok " + m)
p = os.path.join(run, "stage-results.json")
if not os.path.isfile(p):
    bad("stage results document absent from the run"); print("\n".join(out)); sys.exit()
raw = open(p, "rb").read(); d = json.loads(raw)
(ok if hashlib.sha256(raw).hexdigest() == rc.get("stage_results_sha256") else bad)("results document is the one the receipt binds")
m = open("governance/v022_stage5_verification.json", "rb").read()
(ok if hashlib.sha256(m).hexdigest() == d.get("manifest_sha256") else bad)("results were produced against the COMMITTED Stage-5 manifest")
pair = d.get("pair", {})
ax, mc = pair.get("axon", {}), pair.get("micode", {})
(ok if ax.get("start", {}).get("head") == target == ax.get("end", {}).get("head") else bad)("axon revision recorded and unmoved: " + target)
mch = mc.get("start", {}).get("head")
(ok if mch and mch == mc.get("end", {}).get("head") else bad)(f"micode revision recorded and unmoved: {mch}")
(ok if all(s.get("clean") for s in (ax.get("start", {}), ax.get("end", {}), mc.get("start", {}), mc.get("end", {}))) else bad)("both trees clean at start and end")
if mc_dir:
    cur = subprocess.run(["git", "-C", mc_dir, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    dirty = subprocess.run(["git", "-C", mc_dir, "status", "--porcelain"], capture_output=True, text=True).stdout.strip()
    (ok if cur == mch and not dirty else bad)(f"micode checkout is still the certified revision ({cur[:10]}{', dirty' if dirty else ''}) — otherwise the pair is stale")
hs = {h["id"]: h for h in d.get("harnesses", [])}
man = json.loads(m)
declared = {x["id"] for x in man.get("declared_baseline_defects", [])}
for h in man["harnesses"]:
    got = hs.get(h["id"])
    st = got.get("status") if got else None
    if not got:
        bad(f"harness {h['id']} NOT RUN")
    elif h.get("required"):
        (ok if st == "PASS" else bad)(f"required harness {h['id']}: {st}" + ("" if st == "PASS" else f" — {got.get('detail')}"))
    elif h.get("gates_profile"):
        present = set(got.get("declared_defects", []))
        repro = {x["id"]: x.get("result", "") for x in got.get("reproductions", [])}
        if st == "PASS":
            ok(f"regression harness {h['id']}: PASS (fully green)")
        elif st == "PASS_WITH_KNOWN_BASELINE_DEFECTS" and present and present <= declared \
                and all(repro.get(x) == "REPRODUCED" for x in present):
            c = got.get("counts", {})
            ok(f"regression harness {h['id']}: PASS_WITH_KNOWN_BASELINE_DEFECTS — {c.get('passed')} pass, "
               f"{c.get('known_baseline_defects')} known baseline defect(s) {sorted(present)}, each re-reproduced "
               "on its pinned baseline this run; NOT counted as passes")
        else:
            bad(f"regression harness {h['id']}: {st} — {got.get('detail')} (reproductions: {repro})")
        for x in got.get("stale_declarations", []):
            out.append("stale " + x)
li = hs.get("loop-interop", {})
for sec in ("G3", "B256"):
    (ok if any(x.startswith(f"loop_interop_gate: {sec} section executed") for x in li.get("markers", [])) else bad)(f"loop_interop_gate {sec} section executed")
(ok if d.get("stage5_required") == "PASS" else bad)("stage5_required: " + str(d.get("stage5_required")))
(ok if not d.get("problems") and d.get("stage5_verdict") == "VERIFIED" else bad)("stage5_verdict: " + str(d.get("stage5_verdict")))
ms = d.get("micode_full_suite", {})
out.append("defects " + ",".join(ms.get("known_baseline_defects", [])))
c = ms.get("counts") or {}
out.append(f"suite {ms.get('status')} ({c.get('passed')} pass, {c.get('known_baseline_defects')} known baseline defect(s), {c.get('failed')} failed)")
print("\n".join(out))
PY
)"
    DEFECTS=""; SUITE=""; STALE=""
    while IFS= read -r l; do
      case "$l" in "ok "*) sok "${l#ok }" ;; "FAIL "*) sbad "${l#FAIL }" ;;
        "defects "*) DEFECTS="${l#defects }" ;; "suite "*) SUITE="${l#suite }" ;;
        "stale "*) STALE="$STALE ${l#stale }" ;; esac
    done <<< "$out"
    MCH="$(sed -n 's/^pair_micode_head=//p' "$run/receipt")"
  fi
  echo "───────────────────────────────────────────────────────────────────"
  if [ "$fails" -eq 0 ]; then
    if [ -n "$DEFECTS" ]; then
      n=$(tr ',' '\n' <<< "$DEFECTS" | grep -c .)
      echo "STAGE-5 VERIFIED — $n declared pre-existing MiCode defect(s) remain: $DEFECTS"
    else
      echo "STAGE-5 VERIFIED"
    fi
    echo "  axon $(git rev-parse --short "$TARGET") + micode ${MCH:0:8}; micode_full_suite: $SUITE"
    [ -n "$STALE" ] && echo "  STALE declaration(s):$STALE — the defect no longer fails; remove the declaration"
    echo "  a STAGE verdict only — no release qualification is implied"
    exit 0
  fi
  echo "NOT STAGE-5 VERIFIED  ($fails check(s) failed)"
  exit 1
fi

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
  # `gate_run=yes` / `gate_strict=yes`, not a substring match on the command
  # string. A plain (non-strict) `scripts/gate.sh` run satisfied a substring
  # check for "gate.sh" and was accepted here as a "strict gate receipt" —
  # found by an adversarial review, reproduced with a one-line grep, and
  # serious: --strict is what runs parity_all.sh (~22 interp/codegen/AOT-wasm
  # harnesses) and --all-targets clippy; a non-strict pass proves nothing
  # about that and still prints "gate PASSED". The fields are now written
  # by write_receipt itself, parsed the same way gate.sh parses its own
  # argv (an exact --strict token), so this script only ever reads them.
  grep -q "^gate_run=yes$" "$d/receipt" 2>/dev/null || continue
  grep -q "^gate_strict=yes$" "$d/receipt" 2>/dev/null || continue
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
