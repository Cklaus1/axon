#!/usr/bin/env python3
"""v022_protected_readiness.py — the PROTECTED_VERIFIER_READY milestone, derived.

Reads governance/status/v022-verifier-status.json (the MUTABLE status record)
and enforces three things; exits non-zero if any fails:

1. Certified artifacts are IMMUTABLE. Every file listed in
   `certified_artifacts` must still hash to its pinned sha256: a certification
   must keep pointing at exactly what the reviewers saw. Status changes go in
   the status record, never into a certified file.
2. The milestone is DERIVED, never asserted. A component is true only when each
   gate it names has a row in governance/cortex_gate_execution_registry.json
   and each file it names exists. The recorded `value` must equal the derived
   one, so the record cannot claim readiness the evidence does not support.
3. Gated experiments (Stage 7 pilot, CX-21) stay PREREGISTERED while the
   milestone is false: no RUNNABLE / RUNNING / RUN / COMPLETE state is accepted.

Prints one line per component and `v022_protected_readiness: PASS — …`.
"""
import hashlib
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
STATUS = os.path.join(ROOT, "governance/status/v022-verifier-status.json")
REGISTRY = os.path.join(ROOT, "governance/cortex_gate_execution_registry.json")
PRE_READY_STATES = {"PREREGISTERED", "NOT_RUN"}


def sha(p):
    with open(p, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def main():
    status = json.load(open(STATUS))
    registered = {r["gate_id"] for r in json.load(open(REGISTRY))["gates"]}
    fail = []

    for a in status["certified_artifacts"]:
        p = os.path.join(ROOT, a["path"])
        if not os.path.exists(p):
            fail.append(f"certified artifact missing: {a['path']}")
        elif sha(p) != a["sha256"]:
            fail.append(
                f"certified artifact CHANGED: {a['path']} ({a['what']}) — restore the certified "
                f"blob and record status in {os.path.relpath(STATUS, ROOT)} instead"
            )

    m = status["milestone"]
    derived = True
    for name, c in m["components"].items():
        missing_g = [g for g in c.get("requires_gates", []) if g not in registered]
        missing_f = [f for f in c.get("requires_files", []) if not os.path.exists(os.path.join(ROOT, f))]
        ok = not missing_g and not missing_f
        derived &= ok
        why = "" if ok else f"  (unregistered: {missing_g}; missing: {missing_f})"
        print(f"  {'OK ' if ok else 'NO '} {name}{why}")
    if m["value"] != derived:
        fail.append(f"{m['name']} recorded as {m['value']} but the evidence derives {derived}")

    for exp, e in status["gated_experiments"].items():
        if not derived and e["state"] not in PRE_READY_STATES:
            fail.append(f"{exp} is {e['state']} while {m['name']} is false")

    if fail:
        for f in fail:
            print(f"  FAIL {f}")
        print("v022_protected_readiness: FAIL")
        return 1
    print(f"v022_protected_readiness: PASS — {m['name']}={str(derived).lower()}; "
          f"{len(status['certified_artifacts'])} certified artifacts unchanged")
    return 0


if __name__ == "__main__":
    sys.exit(main())
