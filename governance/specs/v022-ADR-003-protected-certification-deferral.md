# ADR-003: protected certification is deferred past v0.22

Status: **ACCEPTED 2026-10-09 by the operator** (answer to the question "How should we handle protected certification and
its signatures for v0.22?": "Defer to a post-0.22 milestone"). Amends the *schedule* of ADR-001 (experiment integrity), not its
decisions D1-D6 and not any certified artifact.

## 1. Decision

v0.22 ships as a **DEV-profile candidate**. The protected path (Firecracker microVM backend, custodian, observer service,
privileged launcher, PSV protocol, controlled guest build, readiness derivation) is **built, tested and reviewed to the state
below, but not certified**. No protected or promotion claim is made by v0.22 (ADR-001 D1, DEV profile: "no protected or promotion
claims").

Protected certification (signatures, the frozen head, the joined paired-disable and mutation runs, the guest image rebuild and
boot test, the certifying review, and the operator's key/waiver/qualification steps) is a **post-0.22 milestone**, not a
v0.22 exit condition.

## 2. What this does NOT change

- No check, gate or refusal is removed or weakened. The signature checks, the freeze manifest's refusals (joined paired-disable
  status, merged mutation-run status, re-survey record), `protected_verifier_ready.py` and the readiness derivation stay as they
  are. They are not "turned off": they are simply not exercised, because no protected claim is made.
- **Readiness stays NOT_READY** (verifier, Stage 7, CX-21). Stage 7 and CX-21 work remains gated on READY exactly as before
  (ADR-001 §7, "Stage 7/CX-21 refuse unless READY"). Skipping signatures does not lift that gate; lifting it is a separate
  operator decision.
- Certified specs, proofs and the immutable governance records are untouched. No record is signed, fabricated or backdated.
- ADR-001 D1-D6 stand: the operator retains every private key; agents receive public keys and signatures only.
- The narrowed PSV-1 claim and its NON-CLAIMS block (governance/specs/v022-protected-suite-verdict.md) and the non-normative
  backlog (governance/specs/post-c9-hardening.md) stay as written. The deferral is not a waiver of any non-claim.

## 3. State at the time of the decision (2026-10-09)

- Review rounds 8-15 and a six-pass find-until-dry loop on PSV-1/SENTINEL (154 agents; 53 confirmed findings; 44 fixed in
  amendment 117, 9 narrowed) did not reach two clean passes. Round 15 (at b974655d) registered PSV-1, PSV-2, PSV-3 and EQUIVALENCE
  and did NOT register FIELD-ORIGIN (shell xtrace in `ns_run`) or SENTINEL (existence-oracle text for a first-in-file sealed
  item). Both blockers have fix branches in flight at the time of writing (c9r15/buildenv12, c9r15/psv1y). Whether they land
  before v0.22 is cut is recorded in the amendment that integrates them.
- `governance/status/v022-psv-paired-disable.json` and `v022-resurvey.json` are stale by design (made at a freeze head that
  does not exist).
- M3038 is withdrawn, not proven; M2603/M2604/M2605/M2607 are withdrawn; M186 is an active row.

## 4. Consequences

- v0.22 may be used for development and for the non-protected workflows. It must not be described as certified,
  protected-ready or `PSV_PROTOCOL_PROVEN` in release notes, the changelog, the README or CLAUDE.md.
- Any user-visible claim about the protected profile must say "built, uncertified; see ADR-003".
- The cost of the deferral is the later, larger cycle: when certification is wanted, the head will have moved, so the freeze,
  the joined runs, the boot test and a certifying review all start from a new head, and the review loop will have to be
  re-run on whatever changed.

## 5. Conditions to resume certification (a checklist, in order)

1. Operator decision to resume, recorded as ADR-004 or an amendment here.
2. A zero-blocker confirmation round on the then-current head (use the find-until-dry design in post-c9-hardening.md Part E).
3. Freeze from a standalone clone: joined paired-disable status, merged mutation-run status, re-survey record, freeze manifest.
4. Guest image rebuild and `psv_guest_boot_test.sh` with root and KVM (the helper leg is unproven).
5. Final evidence run on both hosts at the frozen head.
6. One certifying review.
7. Operator steps (only the operator performs them; nothing here authorises an agent to): back up the qualification key; install
   the builder pin and re-pin `verifier.json`; run the kit; generate the observer key; B263 re-qualification and signature;
   waivers; certification signatures, each only when the operator names the record that turn.
8. Readiness derived READY by `scripts/protected_verifier_ready.py`; only then Stage 7 / CX-21.

## 6. Recording

This ADR is the single place that states the status. CLAUDE.md, the changelog and release notes link here rather than restating it.
