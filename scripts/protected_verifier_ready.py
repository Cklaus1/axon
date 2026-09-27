#!/usr/bin/env python3
"""protected_verifier_ready.py — the ONE authoritative readiness check.

PROTECTED_VERIFIER_READY is never a flag someone flips: it is DERIVED from
immutable evidence, and so are the two readiness consumers built on it.

    PROTECTED_VERIFIER_READY = G01 registered AND PCI certified
        AND protected backend qualified AND G01 proven on it AND PCI proven on it
        AND verification binding ready AND no open blocking false green
    Stage7Ready = PROTECTED_VERIFIER_READY AND pilot manifest frozen
        AND thresholds frozen AND B263 qualification
    CX21Ready   = PROTECTED_VERIFIER_READY AND experiment spec frozen
        AND task suite frozen AND contamination controls AND analysis plan frozen

Every component carries a status — PASS, PARTIAL, NOT_RUN or FAIL — and its
evidence. A readiness is READY only when EVERY component is PASS: no SKIP,
UNKNOWN, missing row or "works locally" ever counts as PASS.

Layout (user direction, 2026-09-27):
    governance/specs/      frozen claims/specs (immutable once certified)
    governance/proofs/     immutable registration/certification records
    governance/status/     current MUTABLE state (pins, follow-ups, experiment states)
    governance/readiness/  DERIVED milestones — written only by this script

Usage:
    protected_verifier_ready.py                  print the derived readiness (JSON)
    protected_verifier_ready.py --write          regenerate governance/readiness/*.json
    protected_verifier_ready.py --check          fail unless: certified artifacts are
        byte-identical to their pins, the committed readiness files equal the derived
        ones, and no gated experiment has left PREREGISTERED while its readiness is
        not READY (run by scripts/gate.sh)
    protected_verifier_ready.py --require {verifier,stage7,cx21}
        exit 0 only if that readiness is READY; otherwise print
        {"status":"REFUSED","reason":…,"missing":[…]} and exit 3. Every protected
        entry point (scripts/v022_stage7_pilot.sh, scripts/v022_cx21_experiment.sh)
        calls this FIRST.
"""
import hashlib
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REGISTRY = "governance/cortex_gate_execution_registry.json"
COMPLETENESS = "AXON-COMPLETENESS.json"
STATUS = {
    "g01": "governance/status/v022-g01.json",
    "pci": "governance/status/v022-pci.json",
    "experiments": "governance/status/v022-experiments.json",
}
OUT = {
    "verifier": "governance/readiness/protected-verifier-readiness.json",
    "stage7": "governance/readiness/stage7-readiness.json",
    "cx21": "governance/readiness/cx21-readiness.json",
}
PRE_READY = {"PREREGISTERED", "NOT_RUN"}


def path(p):
    return os.path.join(ROOT, p)


def sha(p):
    with open(path(p), "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def load(p):
    with open(path(p)) as f:
        return json.load(f)


def gates_component(registered, gates, files=()):
    """PASS iff every gate has a registry row and every proof file exists;
    PARTIAL if some do; NOT_RUN if none do."""
    have_g = [g for g in gates if g in registered]
    have_f = [f for f in files if os.path.exists(path(f))]
    need = len(gates) + len(files)
    got = len(have_g) + len(have_f)
    status = "PASS" if got == need else ("PARTIAL" if got else "NOT_RUN")
    c = {"status": status, "requires_gates": list(gates)}
    if files:
        c["requires_proofs"] = list(files)
    missing = [g for g in gates if g not in registered] + [f for f in files if f not in have_f]
    if missing:
        c["missing"] = missing
    return c


def certified_component(st):
    """A certified/registered artifact: PASS iff every pinned file still hashes
    to its pin (immutability) and its registry/proof evidence exists."""
    bad = [a["path"] for a in st["certified_artifacts"]
           if not os.path.exists(path(a["path"])) or sha(a["path"]) != a["sha256"]]
    c = {
        "status": "FAIL" if bad else "PASS",
        "evidence": st["record"],
        "axon_revision": st["axon_revision"],
        "micode_revision": st["micode_revision"],
        "pinned": {a["path"]: a["sha256"] for a in st["certified_artifacts"]},
    }
    if bad:
        c["changed_since_certification"] = bad
    return c


def frozen_doc_component(pins, key):
    """An experiment prerequisite frozen as a document: PASS iff pinned in
    status/v022-experiments.json and the file still matches its pin."""
    p = pins.get(key)
    if not p:
        return {"status": "NOT_RUN", "missing": [f"{key}: no frozen document pinned"]}
    if not os.path.exists(path(p["path"])):
        return {"status": "FAIL", "missing": [f"{key}: pinned file {p['path']} is gone"]}
    if sha(p["path"]) != p["sha256"]:
        return {"status": "FAIL", "missing": [f"{key}: {p['path']} changed since it was frozen"]}
    return {"status": "PASS", "evidence": p["path"], "sha256": p["sha256"]}


def readiness(schema, components):
    missing = [k for k, c in components.items() if c["status"] != "PASS"]
    return {
        "schema": schema,
        "status": "READY" if not missing else "NOT_READY",
        "derived_by": "scripts/protected_verifier_ready.py",
        "components": components,
        "missing": missing,
    }


def derive():
    registered = {r["gate_id"] for r in load(REGISTRY)["gates"]}
    g01, pci, exp = (load(STATUS[k]) for k in ("g01", "pci", "experiments"))
    open_blocking = [
        {"id": f["id"], "where": f["where"]}
        for f in load(COMPLETENESS)["false_greens"]
        if f["status"] == "open" and f.get("severity") == "security"
    ]
    v = {
        "g01_authenticity": certified_component(g01),
        "pci_isolation": certified_component(pci),
        "protected_backend": gates_component(
            registered,
            ["G13-r22-profile-qualification", "G13-r22-profile-eligibility", "G13-r22-guest-truth"],
        ),
        "g01_on_protected_backend": gates_component(
            registered,
            ["G01-r22-registered-check", "G01-r22-verifier-separation"],
            ["governance/proofs/v022-g01-microvm/REGISTRATION.md"],
        ),
        "pci_on_protected_backend": gates_component(
            registered,
            ["G03-r22-trial-isolation", "G03-r22-physical-isolation"],
            ["governance/proofs/v022-pci-microvm/CERTIFICATION.md"],
        ),
        "verification_binding": gates_component(
            registered,
            [
                "G01-r22-nonvacuous-outcome", "G01-r22-unknown-outcome",
                "G32-r22-receipt-roles", "G32-r22-artifact-recheck",
                "G32-r22-evidence-laundering", "G32-r22-sidecar-bindings",
                "G11-r22-independent-admission", "G11-r22-admission-disposition",
                "G11-r22-rollback-revalidate", "G33-r22-decision-rule-freeze",
            ],
        ),
        "no_open_blocking_false_greens": (
            {"status": "FAIL", "open": open_blocking}
            if open_blocking else {"status": "PASS", "evidence": COMPLETENESS}
        ),
    }
    verifier = readiness("protected-verifier-readiness/1", v)
    gate = {"status": "PASS" if verifier["status"] == "READY" else "NOT_RUN",
            "evidence": OUT["verifier"]}
    pins = exp.get("frozen_documents", {})
    stage7 = readiness("stage7-readiness/1", {
        "protected_verifier_ready": gate,
        "pilot_manifest_frozen": frozen_doc_component(pins, "stage7_pilot_manifest"),
        "thresholds_frozen": frozen_doc_component(pins, "stage7_thresholds"),
        "b263_qualification": frozen_doc_component(pins, "b263_qualification"),
    })
    cx21 = readiness("cx21-readiness/1", {
        "protected_verifier_ready": dict(gate),
        "experiment_spec_frozen": frozen_doc_component(pins, "cx21_experiment_spec"),
        "task_suite_frozen": frozen_doc_component(pins, "cx21_task_suite"),
        "contamination_controls": frozen_doc_component(pins, "cx21_contamination_controls"),
        "analysis_plan_frozen": frozen_doc_component(pins, "cx21_analysis_plan"),
    })
    return {"verifier": verifier, "stage7": stage7, "cx21": cx21}, exp


def dump(d):
    return json.dumps(d, indent=2) + "\n"


def main(argv):
    derived, exp = derive()
    if "--write" in argv:
        for k, p in OUT.items():
            os.makedirs(os.path.dirname(path(p)), exist_ok=True)
            with open(path(p), "w") as f:
                f.write(dump(derived[k]))
        print("wrote " + ", ".join(OUT.values()))
        return 0
    if "--require" in argv:
        which = argv[argv.index("--require") + 1]
        r = derived[which]
        if r["status"] == "READY":
            print(json.dumps({"status": "READY", "readiness": which}))
            return 0
        print(json.dumps({
            "status": "REFUSED",
            "reason": {"verifier": "protected_verifier_not_ready",
                       "stage7": "stage7_not_ready", "cx21": "cx21_not_ready"}[which],
            "missing": r["missing"],
        }))
        return 3
    if "--check" in argv:
        fail = []
        for key in ("g01", "pci"):
            c = derived["verifier"]["components"][f"{key}_authenticity" if key == "g01" else "pci_isolation"]
            for p in c.get("changed_since_certification", []):
                fail.append(f"certified artifact CHANGED: {p} — restore the certified blob; "
                            f"current state belongs in {STATUS[key]}")
        for k, p in OUT.items():
            if not os.path.exists(path(p)):
                fail.append(f"{p} missing — run with --write")
            elif open(path(p)).read() != dump(derived[k]):
                fail.append(f"{p} differs from the derived readiness — it is generated, never "
                            f"edited: run with --write and review the diff")
        for name, e in exp["gated_experiments"].items():
            if derived[e["readiness"]]["status"] != "READY" and e["state"] not in PRE_READY:
                fail.append(f"{name} is {e['state']} while {OUT[e['readiness']]} is not READY")
        for k in ("verifier", "stage7", "cx21"):
            r = derived[k]
            print(f"  {k}: {r['status']}" + (f"  (missing: {', '.join(r['missing'])})" if r["missing"] else ""))
        if fail:
            for f in fail:
                print(f"  FAIL {f}")
            print("protected_verifier_ready: FAIL")
            return 1
        print(f"protected_verifier_ready: PASS — verifier={derived['verifier']['status']}, "
              f"stage7={derived['stage7']['status']}, cx21={derived['cx21']['status']}")
        return 0
    print(dump(derived["verifier"]), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
