#!/usr/bin/env python3
"""G29-r22-claim-separation: validate governance/v022_release_claims.json.

Three separate release fields — engineering_qualified, policy_activated,
measured_improvement_supported — each {claimed: bool, evidence: {...}|null}.
A claim may be true ONLY with evidence of its own kind, and one claim's
evidence never satisfies another. Absence leaves the claim false. Exit 0 iff
valid. `--self-test` checks the validator against the cases it must refuse.
"""
import json
import os
import sys

FIELDS = {
    # field -> the evidence kind it needs, and what that evidence must carry
    "engineering_qualified": ("release_verified_receipt", ("revision", "receipt")),
    "policy_activated": ("admission_and_activation", ("admission_ref", "activation_transition_ref")),
    "measured_improvement_supported": ("measured_claim_report",
                                       ("report", "population", "uncertainty")),
}


def validate(d):
    errs = []
    if d.get("schema") != "axon-v022-release-claims/1":
        errs.append("schema must be axon-v022-release-claims/1")
    extra = set(d) - set(FIELDS) - {"schema", "purpose"}
    if extra:
        errs.append(f"unknown fields {sorted(extra)}: the three claims are the whole vocabulary")
    for f, (kind, needs) in FIELDS.items():
        c = d.get(f)
        if not isinstance(c, dict) or not isinstance(c.get("claimed"), bool):
            errs.append(f"{f}: missing, or `claimed` is not a boolean")
            continue
        ev = c.get("evidence")
        if not c["claimed"]:
            continue
        if not isinstance(ev, dict) or ev.get("kind") != kind:
            errs.append(f"{f}: claimed true without `{kind}` evidence of its own")
            continue
        missing = [k for k in needs if not ev.get(k)]
        if missing:
            errs.append(f"{f}: evidence lacks {missing}")
    # One claim's evidence never stands for another's.
    evs = [json.dumps(d[f].get("evidence"), sort_keys=True) for f in FIELDS
           if isinstance(d.get(f), dict) and d[f].get("claimed")]
    if len(evs) != len(set(evs)):
        errs.append("two claims cite the same evidence: claims are separate")
    return errs


def self_test():
    base = {"schema": "axon-v022-release-claims/1",
            **{f: {"claimed": False, "evidence": None} for f in FIELDS}}
    good_eng = {"kind": "release_verified_receipt", "revision": "abc", "receipt": "r"}
    cases = [
        ("all false (the default) is valid", base, True),
        ("improvement claimed with no evidence", {**base, "measured_improvement_supported": {"claimed": True, "evidence": None}}, False),
        ("improvement claimed on engineering evidence", {**base, "measured_improvement_supported": {"claimed": True, "evidence": good_eng}}, False),
        ("activation claimed without an activation transition",
         {**base, "policy_activated": {"claimed": True, "evidence": {"kind": "admission_and_activation", "admission_ref": "a"}}}, False),
        ("engineering claimed with its own evidence", {**base, "engineering_qualified": {"claimed": True, "evidence": good_eng}}, True),
        ("a merged 'released' field", {**base, "released": True}, False),
        ("claimed not a boolean", {**base, "policy_activated": {"claimed": "yes", "evidence": None}}, False),
    ]
    bad = []
    for name, d, ok in cases:
        if (not validate(d)) != ok:
            bad.append(f"self-test '{name}': validator said {'valid' if not validate(d) else 'invalid'}")
    return bad


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    bad = self_test()
    if bad:
        print("\n".join("  " + b for b in bad))
        print("v022_claim_separation: FAIL — the validator failed its self-test")
        return 1
    if "--self-test" in sys.argv:
        print("v022_claim_separation: self-test PASS")
        return 0
    p = os.path.join(root, "governance", "v022_release_claims.json")
    d = json.load(open(p))
    errs = validate(d)
    if errs:
        print("\n".join("  " + e for e in errs))
        print("v022_claim_separation: FAIL")
        return 1
    states = ", ".join(f"{f}={d[f]['claimed']}" for f in FIELDS)
    print(f"v022_claim_separation: PASS — {states}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
