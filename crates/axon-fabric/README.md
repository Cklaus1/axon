# axon-fabric

**Status: partial (v0.22 M1 slices).** Only production caller:
`cortex repair --fabric-journal`, which spawns the `axon-fabric` binary
(pinned by path and sha256 in a `cortex-check-registry/1` file) rather than
linking this crate, because this crate depends on `axon-cortex`.

## What it is

* **`journal`** (B260): a durable, append-only operation journal. Intent is
  fsynced before any effect. States run `Intended → Reserved → Launched →
  (Completed | Failed | Cancelled | OutcomeUnknown)`. Reusing an
  `OperationId` with a different input digest is a conflict. On reopen, a
  `Launched` op with no terminal record becomes `OutcomeUnknown`: its
  liability is kept and it is never re-executed. It carves aggregate
  reservations over a combined model/exec/verify/retry budget.
* **`submit`**: `acf-compute-request/1` in, `acf-execution-receipt/1` out.
  Before the journal's launch record it strictly parses the request, rechecks
  the authority **epoch** against an `axon-loop` store, applies isolation and
  registered-executable checks, and runs an `axon-os` admission. The binary
  also has `status` and `cancel`.
* **`backend`**: three backend profiles, chosen by what each one *is*, with no
  fallback between them:
  * `process_scoped/local-interpreter` runs registered checks and has no
    hardware isolation.
  * `axon-metal-fc-nojailer` (the `axon-vm` library profile) is eligible for
    **nothing**.
  * `linux-microvm-protected` (`scripts/fc_linux_profile.sh`) runs
    `interpreter_run` only, and only while its manifest sha256 matches the
    qualification record.

Dependencies: `axon-loop-contracts`, `axon-loop` (epoch reads only),
`axon-cortex`, `axon-os`, `axon-vm`. It is the top of the Cortex family.

## What it does NOT do

* **It does not enforce the request's authority.** `principal_ref` and
  `grant_ref` are opaque strings, used only to format `authority_ref`.
  Admission (`submit.rs` `supervisor_admits`) runs `supervise_requiring` over
  a no-op `AdmissionProbe` that declares an empty effect row. It uses a
  **hard-coded `Profile::Restricted` grant** and `require_approval: false`,
  whatever the request says. At dispatch the check is bounded only by an
  optional `--effect-ceiling`, and with no ceiling it runs unbounded. So the
  grant it admits is not the grant it enforces. Also, `cortex --fabric-journal`
  hard-codes principal `cortex:repair` and `policy_digest acf1:000…`. This is
  open conflict **D-C2**, Stage 2: `governance/cortex-v015/DISCREPANCIES.md`
  D-016.
* **It does not meter cost.** Every receipt says `usage_state: unknown` and
  every settlement is `Billing::Unknown`. Unknown is reported as unknown, never
  as 0.
* **It ignores `required.architecture` and `required.checkpoint_kind`.**
  `backend::select` never reads them (**G6**). A request for an unsupported
  architecture or checkpoint kind is not refused. Open, Stage 2.
* **It cannot treat a CX-11 admission or the active-policy pointer as
  authority.** It reads only the epoch from the loop store (D-018).

## Open defects

| defect | where | evidence |
|---|---|---|
| Admission uses a fixed grant (D-C2) | `src/submit.rs` `supervisor_admits` | D-016 |
| Duplicate `acf1:` canonicaliser across the cortex seam (D-C3) | `src/submit.rs:176-189` vs `axon-cortex/src/runner.rs` `fabric_*_digest` | D-017 |
| Its own reservation algebra instead of `axon_os::ResourceLedger::carve` (D-C5) | `src/journal.rs` `reserve` | D-017 |
| `LinuxProfileConfig::qualification()` accepts a record with BLOCKED > 0: it ignores the missing trusted issuer, host, freshness, engine digests and any signature. An unsigned JSON the operator can write enables protected dispatch | `src/backend.rs` `qualification` | D-020; B263 evidence 32 PASS / 0 FAIL / 4 BLOCKED |
| Linux dispatch is tested only through a **stand-in launcher**, so the tests say nothing about the VM | `tests/submit.rs:550-560` | `F_guest_vm.json` B280 |
| The Linux profile's guest is unpoliced (no in-guest effect ceiling). Fabric refuses requests that need one, so only grant-free `interpreter_run` reaches it | `src/backend.rs` `select` | D-020, operator decision D5 |
| Cost is unmetered | `src/submit.rs` | above |
| G6: architecture and checkpoint kind are ignored | `src/backend.rs` `select` | above |

## Evidence location

The analysis and qualification records cited here come from the operator's
**untracked** `.axon-v022/` directory: `analysis/D_architecture.json`,
`analysis/F_guest_vm.json`, `evidence/b263/*.json`,
`integration/gate_279da77.log`. They are **not in the repository**. Treat them
as operator-side evidence. They cannot be checked from this tree alone.
