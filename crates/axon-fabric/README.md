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
  registered-executable checks, resolves the request's grant and runs an
  `axon-os` admission under it. The binary also has `status` and `cancel`.
* **`grants`** (D-016): the operator's `axon-fabric-grant-registry/1` file
  maps `grant_ref` → a grant file pinned by sha256 and bound to one
  `principal_ref`. The grant file is an axon-os `.axjob` without `program`
  and is parsed by `axon_os::parse_manifest`, so profile semantics are
  axon-os's own (misspelling refused, omitted dimensions materialised,
  `reproducible` ORed on intersection, `require_approval` per the axon-os
  table with the token at the grant file's `.approval` sibling). An unknown
  ref, a principal it is not bound to, or an edited grant file is refused
  (exit 7, `unauthorized`) before the journal is opened. The all-zero
  `policy_digest` placeholder is refused (exit 3).
* **Authority at execution.** Admission is `supervise_requiring` under the
  resolved grant, over a probe that declares the program's SCANNED effect row
  (`axon_os::runtime::scan_effects`). The check then runs under
  `AXON_ALLOWED_EFFECTS` derived from that same grant (the mapping
  `axon-os`'s sandbox wrapper uses; a grant withholding everything yields `""`
  = deny every effect). There is no `--effect-ceiling` flag any more. Grants
  the backends cannot enforce are `unsupported`, never weakened: a
  path/host-scoped grant (the interpreter's ceiling is coarse effect names),
  a reproducible (hermetic) grant (the executor inherits the Fabric's
  environment), and on `linux-microvm-protected` any grant that withholds an
  effect (no guest policy channel — in-guest enforcement is Stage 3, B263 x1).
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

* **It does not authenticate the principal.** The grant registry BINDS a
  `principal_ref` to a `grant_ref`; it does not prove the caller is that
  principal (a claim, like `AXON_PRINCIPAL`). `cortex --fabric-journal` now
  takes `--fabric-principal`, `--fabric-grant-ref`, `--fabric-grant-registry`
  and `--fabric-policy-digest` explicitly and refuses to start without them
  (D-016 closed for the Fabric; `axon-loop` plan approval is not covered).
* **The admission probe does not run the program.** Admission decides from
  the scanned effect row; the executed check is bounded by the grant's
  interpreter ceiling, which is process-scoped enforcement, not an OS
  boundary. `grant.budget` bounds only `limits.max_cost_micro`.
* **It does not meter cost.** Every receipt says `usage_state: unknown` and
  every settlement is `Billing::Unknown`. Unknown is reported as unknown, never
  as 0.
* **It does not checkpoint.** Every profile states the architectures it runs
  (`Profile::architectures`: the host's for the local interpreter, x86_64 for
  both VM profiles) and offers `checkpoint_kind = none` only. A request for
  another architecture or checkpoint kind — or an `axon_wasm` /
  `native_process` engine — is refused as `unsupported` (journalled, never
  launched) before any effect (G6, fixed in Stage 2).
* **It cannot treat a CX-11 admission or the active-policy pointer as
  authority.** It reads only the epoch from the loop store (D-018).

## Open defects

| defect | where | evidence |
|---|---|---|
| Duplicate `acf1:` canonicaliser across the cortex seam (D-C3) | `src/submit.rs:176-189` vs `axon-cortex/src/runner.rs` `fabric_*_digest` | D-017 |
| Its own reservation algebra instead of `axon_os::ResourceLedger::carve` (D-C5) | `src/journal.rs` `reserve` | D-017 |
| `LinuxProfileConfig::qualification()` accepts a record with BLOCKED > 0: it ignores the missing trusted issuer, host, freshness, engine digests and any signature. An unsigned JSON the operator can write enables protected dispatch | `src/backend.rs` `qualification` | D-020; B263 evidence 32 PASS / 0 FAIL / 4 BLOCKED |
| Linux dispatch is tested only through a **stand-in launcher**, so the tests say nothing about the VM | `tests/submit.rs:550-560` | `F_guest_vm.json` B280 |
| The Linux profile's guest is unpoliced (no in-guest effect ceiling). Fabric refuses requests that need one, so only grant-free `interpreter_run` reaches it | `src/backend.rs` `select` | D-020, operator decision D5 |
| Cost is unmetered | `src/submit.rs` | above |

## Evidence location

The analysis and qualification records cited here come from the operator's
**untracked** `.axon-v022/` directory: `analysis/D_architecture.json`,
`analysis/F_guest_vm.json`, `evidence/b263/*.json`,
`integration/gate_279da77.log`. They are **not in the repository**. Treat them
as operator-side evidence. They cannot be checked from this tree alone.
