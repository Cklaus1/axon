# v0.22 binding batch, candidate 2b — registration record

Frozen pair: Axon `bd3637ab1b373a74e4d39cd67e07289b2b7af229` (v022/stage3) + MiCode `e7fdb739`.

## History

- Candidate 1 (`governance/proofs/v022-binding/`) registered G32 and G33. The three G11 gates failed it with four blockers.
- Candidate 2 (`6cd4ea5c`) failed its own proof run. Four mutation rows had stopped applying after the refactors. They were retargeted (`bd3637ab`) and the pair refrozen as candidate 2b.

## Proof run on the frozen pair (`proof-run-bd3637ab.txt`)

- Every mutation row applies.
- Clippy is clean.
- Tests: 536 Axon loop/contracts/fabric/cortex/reflex/adapter, 1573 axon-core, 5307 MiCode.
- Runners: PCI 18 rows, Stage-5 binding rows and readiness check all pass.
- Mutations: 112/112 killed.
- Real-binary interop: 250 assertions.
- MiCode gates: 19 rows.
- Trees are clean before and after.

## One independent review, `wf_8aad6d16-ad6`

One reviewer per gate (`final-review-bd3637ab.json`).

| Gate | Verdict | Status |
|---|---|---|
| G11-r22-independent-admission | REGISTER, zero blockers | **REGISTERED** |
| G11-r22-rollback-revalidate | REGISTER, zero blockers | **REGISTERED** |
| G11-r22-admission-disposition | DO_NOT_REGISTER | PARTIAL → candidate 3 |

The disposition blocker, executed by the reviewer: relabelling one trial's usage currency made the arm's economics multi-currency. `admission::facts()` then fell back to an Unresolved total with a liability of **0**, so the arm's unknown Fabric-execution liability vanished, the frozen liability tolerance passed, and the candidate was ACCEPTed and activated.

## Recorded findings (not hidden, not blocking the registered claims)

- **MAJOR-ADJACENT (found by both reviewers):** a baseline's issuer is rechecked for trust, but not for independence, on baseline activation or rollback. Fixed with candidate 3.
- **MAJOR-ADJACENT (disposition):** in a development-class evaluation, a context observer withdrawn after admission does not block activation.
- **MINOR:**
  - The subject set is the one the evaluation declares. This follows from the recorded claims-based identity residual, ADR-001 §3.4.
  - A protected context's launching parent is not in the independence set.
  - A Fabric-listed admitter can revoke; this only pauses, it never promotes.
  - `derive()`'s per-field binding refusals have no named test or mutation row.
  - The population issuer is not rechecked at re-derivation.
  - A replayed rollback transition id returns its historical success.
  - Three rollback guards have no mutation row.
  - In a protected scope, the incumbent-of-record is exempt from the evidence floor.
- **FUTURE:**
  - The plan's approval is self-asserted (ADR-001 §3.4).
  - Rollback does not consult Fabric's own B263 record.
  - A failed rollback refuses rather than pausing atomically.

This record is immutable. Current state lives in `governance/status/`; readiness is derived by `scripts/protected_verifier_ready.py`.
