# G01-r22-unknown-outcome — registration record

> Timeout, cancellation, unmatched checks, missing evidence and unverifiable output remain
> distinct non-success states through the bridge and statistical analysis; no default pass is
> supplied.

**REGISTERED** at Axon `a9e4c269c08beb7854397aa2d067a41c7e907e6f` (v022/stage3) + MiCode
`fc6221a5fdc79de946484f893fa2a27ade0a5c24`. Final independent review `wf_dc09551c-ab1`:
REGISTER, zero blockers, six MINORs.

## Candidates

| # | Pair | Review | Result |
|---|---|---|---|
| 1 | axon 6d3f0ff6 + micode e7fdb739 | wf_849bc606-7e8 | 3 blockers: check timeout and missing evidence collapsed into not_run; a run deadline was `failed`; SIGTERM wrote no sidecar |
| 2 | axon 1486ef37 + micode 3192e948 | wf_bac07f9d-087 | 1 blocker: a provider silent past its first-byte deadline ended as stream_death |
| 3 | axon a9e4c269 + micode fc6221a5 | wf_dc09551c-ab1 | **REGISTER** |

All three reviews are in `final-review-a9e4c269.json`.

## What holds

- **The bridge states why there is no verdict.** MiCode's `fabric_check` failure class and the
  run's deadline (wall-clock budget, a provider timeout, and a silent provider past first-byte
  after its re-issues) travel as ONE closed-set content marker in a `not_run`'s
  `verification.evidence_refs`. This keeps the reason inside the pinned episode contract.
- **SIGTERM cancels cooperatively.** A cancelled exec writes a `cancelled` sidecar.
- **Axon keeps the kinds apart.** Intake recognises only that marker set. EVL maps each reason to
  its kind, and how the run ended outranks a stated reason. A D12 verdict is judged but never
  counted. A cancelled or timed-out run counts no verdict. The per-arm `unknown_kinds` statistics
  count each kind apart, and `assigned = pass + fail + Σkinds + missing` holds.
- **Real-binary evidence** (`interop-a9e4c269.txt`, section 8b). Sixteen real MiCode+Fabric trials
  in their own worktrees, judged by the real `evl evaluate`:
  - cancelled: SIGTERM;
  - timed_out, ×6: wall-clock budget, a silent provider past first-byte, the Fabric watchdog, and
    Fabric wall-time;
  - missing_evidence: a crashed Fabric;
  - unverifiable: Fabric with no signer;
  - not_run, ×2: a Fabric refusal and a refused configuration;
  - unmatched, ×2;
  - unbound (D12), ×3.

  The candidate arm counts 0 pass and 0 fail.
- **Discriminators.** The same gate against MiCode e7fdb739 fails 14 of 33 section assertions (the
  candidate-1 blockers). Against MiCode 3192e948 it fails exactly the 3 silent-provider assertions
  (the candidate-2 blocker).
- **Proof run** (`proof-run-a9e4c269.txt`):
  - clippy clean;
  - 543 + 1573 Axon and 5311 MiCode tests;
  - mutations 133/133, including M123–M136 for this gate;
  - PCI 18 rows;
  - MiCode gates 22 rows;
  - trees clean.

## Recorded MINORs (not hidden; none falsifies the claim)

- A process-group SIGINT that lands during the Fabric check (after the agent's run finished) is
  recorded as `missing_evidence`, not `cancelled`.
- The first signal does not interrupt an in-flight provider wait. A supervisor's SIGKILL, or a
  second signal, before the turn ends leaves the trial `missing`.
- The re-issue loop reports only the last attempt's failure: a stall followed by a drop ends as
  stream_death.
- A signed check that ran but could not vouch for its output is MissingEvidence, alongside a
  crashed Fabric.
- Admission ArmFacts keep only the aggregate unknown count. The kinds are in the evaluation it
  references.
- Carried over from candidate 2:
  - the watchdog does not kill a hung Fabric child;
  - a second signal exits without a sidecar;
  - `exhausted_dimension` checks wall-clock last.

This record is immutable. Current state lives in `governance/status/`; readiness is derived by
`scripts/protected_verifier_ready.py`.
