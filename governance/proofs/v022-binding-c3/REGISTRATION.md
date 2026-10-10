# v0.22 binding batch, candidate 3 — registration record

Frozen pair: Axon `6d3f0ff6a51bdaebdf3e024772ab47f293d45997` (v022/stage3) + MiCode `e7fdb739`.

## Proof run on the frozen pair (`proof-run-6d3f0ff6.txt`)

- Every mutation row applies. Clippy is clean.
- Tests: 541 Axon loop/contracts/fabric/cortex/reflex/adapter, 1573 axon-core, 5307 MiCode.
- Runners: PCI 18 rows; Stage-5 and MiCode gate runners (20 rows) pass.
- Mutations: 128/128 killed.
- Real-binary interop: 272 assertions, including section 8b.
- Trees are clean before and after.
- Discriminator: interop section 8b run against the M125 mutant fails 4 of its 22 assertions (`discriminator-8b-6d3f0ff6.txt`).

## One independent review, `wf_849bc606-7e8`

| Gate | Verdict | Status |
|---|---|---|
| G11-r22-admission-disposition | REGISTER, zero blockers (third candidate) | **REGISTERED** |
| G01-r22-unknown-outcome | DO_NOT_REGISTER | PARTIAL → candidate 2 |

The three G01-r22-unknown-outcome blockers were all executed through the real `micode exec` binary, on the MiCode side of the bridge:

1. A hung Fabric (the watchdog), a crashed Fabric and an unattested receipt all reach Axon as the same uncited `not_run`. EVL cannot tell a check timeout from missing evidence.
2. The agent run hitting its own deadline (wall-clock budget, provider timeout) is recorded as `failed` + `not_run`, identical to a refusal, so a D12 trial can never be TimedOut.
3. SIGTERM/SIGINT, the real way to cancel a headless exec, writes no sidecar. A cancelled trial arrives as `missing`, not Cancelled.

## Recorded findings (not hidden, not blocking the registered claim)

- **MAJOR-ADJACENT (disposition):** a trusted monitor cannot record a violation against an issued trial that was never intaken. In a development-class evaluation, a withheld unsafe trial becomes a missing unknown that the margin can absorb. A protected evaluation is INCONCLUSIVE on it. This is the same D1/D2 same-uid producer caveat.
- **MINOR (disposition):**
  - the assignment issuer is not rechecked at re-derivation;
  - ACCEPT reasons do not list the tolerated unknowns or liability;
  - the arms' shared currency and the usage price schedule are not pinned until metering (D4/D10);
  - no real-binary admission path exists under D12.
- **MINOR (unknown-outcome):**
  - a degeneration trip or a detach is labelled Cancelled by MiCode;
  - the admission's ArmFacts keep only the unknown count, not the kinds;
  - interop 8b's missing-evidence and unverifiable cases are made by the harness.

This record is immutable. Current state lives in `governance/status/`; readiness is derived by `scripts/protected_verifier_ready.py`.
