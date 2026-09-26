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
  reservations over a combined model/exec/verify/retry budget; the
  committed ≤ ceiling check for each dimension is `axon_os::ledger::
  ResourceLedger::carve` (D-C5), with an overflow guard `carve` lacks. The
  journal keeps what the ledger has no notion of: durability, held vs
  liability vs charged, `OutcomeUnknown`, and release on a never-launched
  cancel (`carve`'s `used` never decreases, so it is re-derived per check
  from the journal state rather than stored). An unknown cost keeps its
  WHOLE reservation as liability. `settle` takes an accounting receipt
  `(origin, sequence, actual)`: the identical receipt again is idempotent
  (nothing written); any other second settlement of the op, or reuse of
  `(origin, sequence)` for another op, is refused AND recorded
  (`settle_conflict`), and the op is then DISPUTED — held as liability at the
  componentwise max of its reservation and every claimed amount
  (G13-r22-billing-settlement, `tests/journal.rs` `g13_*`). Resolving a
  dispute is not implemented.
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
  effect UNLESS the signed qualification record shows `x1_guest_policy_channel`
  as `PASS` (a waived BLOCKED x1 does not count; today's record has it
  BLOCKED, so the refusal stands).
* **Guest policy (S3-5).** Every `linux-microvm-protected` launch passes
  `--policy FILE` to the launcher: an `axon-vm-mmds/1` JSON whose
  `allowed_effects` is that same grant-derived ceiling (`""` ⇒ `[]`, deny-all —
  never omitted, never "unrestricted"), plus `run_id` (the operation id) and
  `principal`. The launcher carries it on the kernel cmdline
  (`axon.policy=<base64>`) for `axon-guest-init`. The file is written beside
  the operation's `--out` dir (`<out_root>/<op>.policy.json`, create-new) and
  its sha256 goes into the receipt as `guest-policy-sha256:`. A policy whose
  cmdline word exceeds `GUEST_POLICY_WORD_MAX` (the guest's 2046-byte limit
  minus a 512-byte reserve for the launcher's boot args and Firecracker's
  device words) is `unsupported` before anything is journalled as launched.
* **`backend`**: three backend profiles, chosen by what each one *is*, with no
  fallback between them:
  * `process_scoped/local-interpreter` runs registered checks and has no
    hardware isolation.
  * `axon-metal-fc-nojailer` (the `axon-vm` library profile) is eligible for
    **nothing**.
  * `linux-microvm-protected` (`scripts/fc_linux_profile.sh`) runs
    `interpreter_run` only, and only while `qualification()` accepts its
    evidence (FG-042, fixed in code in Stage 3): a detached Ed25519
    signature over the exact record bytes (`<evidence>.sig`,
    `axon-evidence-signature/1`) that verifies under a public key in
    `profiles/linux-microvm/trusted_issuers/`; `result` `PASS` with no
    BLOCKED assertion, or `PASS_WITH_BLOCKED` with every BLOCKED assertion
    covered by an unexpired, reasoned, issuer-signed waiver
    (`axon-b263-waiver/1`) bound to that record's sha256; `FAIL == 0`,
    `PASS > 0`, `counts.BLOCKED` equal to the BLOCKED assertions; `end` not
    in the future and within `--linux-evidence-max-age-s` (default 30 days);
    firecracker/jailer sha256 recorded (and equal to the manifest's `engine`
    pins when it has them); neither the evidence tree nor the manifest's
    build tree dirty; `host` and `caveat` stated; and the manifest sha256
    equal to the one qualified. The issuer, record sha256, host and caveat
    (D2) go into every receipt's `evidence_refs`. **The committed tree has no
    issuer key and no signed record, and the manifest says
    `axon_tree_dirty_at_build: true`, so Fabric refuses this profile today**
    — until the operator signs a re-qualification (S3-6).

* **`workspace`** (B261, v0.22 Stage 5): WorkspaceVersion — one identity for
  a workspace TREE, byte-identical to MiCode's `axon.workspace-version/1`
  recipe (`tests/fixtures/workspace_version_vector.json`, copied from MiCode
  `docs/axon-support/fixtures/` at 09029360; the byte recipe itself is
  `axon_cortex::runner::{workspace_manifest_bytes, workspace_version_ref}`).
  Import refuses the whole tree on traversal, absolute paths, escaping
  symlinks, devices/FIFOs/sockets, non-UTF-8 names, control characters,
  duplicates, quota overflow (D11: 20 000 entries / 256 MiB / depth 32) and —
  beyond the recipe — **namespace collisions** (equal after NFC + lowercase,
  or a path that is both file and directory). Skipped top-level `.git` /
  `.micode` are recorded as explicit omissions. The store
  (`<state>/tenants/<key>/workspaces`, `--state`, default `<journal>.state`) is
  content-addressed and write-once (fsynced temp + no-clobber rename); a
  materialization re-verifies every blob and refuses an existing
  destination. A `WorkspaceProjection` with no `version_ref` (a hash-only
  observation) or an unpublished ref cannot be materialized (G28). Each trial
  runs with its own `HOME` / `XDG_CACHE_HOME` / `CARGO_TARGET_DIR` under
  `<state>/tenants/<key>/trial-caches/<sha256(trial_id)>`. Store and caches
  are per TENANT (`<state>/tenants/<sha256(tenant)[..32]>`): a version
  another tenant published does not resolve (content addressing is not a
  capability), and equal TrialIds in two tenants never share a cache. `workspace_version_ref` may name
  a published version (materialized privately per operation), the one-file
  version of `argv[0]` (copied into the store first; the COPY is judged), or
  the historical single-file digest (read in place). No GC (D11).
  `axon-fabric workspace-import --state DIR --tenant T --root DIR` publishes a tree.
  Mode bits are no boundary for root: the store's integrity is re-hashing,
  not permissions.
* **Check suites (B264).** A `cortex-check-registry/1` file may carry
  `checks: [{id, visibility: visible|hidden, root, entry,
  workspace_version_ref}]`. A request with `argv = ["check:<id>", filter]`
  judges a PUBLISHED candidate version: the suite root is imported at
  dispatch and must still be the pinned version (else `unregistered`, zero
  launches), then the candidate is materialized into `<run>/candidate` and
  the suite read-only into `<run>/check` — never inside the candidate — and
  the suite reaches the candidate only through `AXON_PATH` (the operator's
  ambient `AXON_PATH` is never inherited). Admission scans the suite entry
  with each `mod NAME` replaced by the candidate's `NAME.ax`; an unresolved
  module still scans as every effect. `CheckRegistry::subject_visible_checks`
  never lists a hidden suite. The journal records a suite's identity, never
  its bytes.
* **Output binding (B264/B265).** After a local run the Fabric re-imports
  what the run left: the receipt's `output_workspace_ref` is that version
  (published, so retrievable), or the historical digest of the file for a
  historical ref. If it differs from the input, or a suite's own bytes moved,
  a pass/fail verdict becomes `unknown` with the reason. The cortex
  `FabricSubmitExecutor` sends the one-file WorkspaceVersion of the checked
  file and accepts a report only if the receipt's input AND output refs are
  that version. Candidate code runs inside the verifier process: a grant
  that allows fs reads lets it read `<run>/check` — the effect ceiling, not
  the directory layout, is what bounds that.

`acf1:` identities (`executable_digest`, `workspace_digest`) delegate to the
single canonicaliser `axon_cortex::runner::acf1_canonical_bytes`, which the
cortex side builds its requests with (D-C3, fixed in Stage 2).

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
| Linux dispatch is tested only through a **stand-in launcher**, so the tests say nothing about the VM | `tests/submit.rs:550-560` | `F_guest_vm.json` B280 |
| The Linux profile's guest is unpoliced (no in-guest effect ceiling). Fabric refuses requests that need one, so only grant-free `interpreter_run` reaches it | `src/backend.rs` `select` | D-020, operator decision D5 |
| Cost is unmetered | `src/submit.rs` | above |

## Evidence location

The analysis and qualification records cited here come from the operator's
**untracked** `.axon-v022/` directory: `analysis/D_architecture.json`,
`analysis/F_guest_vm.json`, `evidence/b263/*.json`,
`integration/gate_279da77.log`. They are **not in the repository**. Treat them
as operator-side evidence. They cannot be checked from this tree alone.
