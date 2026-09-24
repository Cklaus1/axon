# Source and baseline review — Axon 0.22

**Scope:** targeted static integration review of the supplied Axon (1,213 files) and MiCode (859 files) archives, the full v0.21 package and the Fabric v0.1 pack. Full byte inventories are recorded; this is not a line-by-line audit of all source. Neither Rust compilation nor source/host/model/peer execution was performed by this build.

The actual parent v0.21 validator passed and its 367 tests passed before edits. The earlier assessment is retained in `PRIOR_INTEGRATION_ASSESSMENT_V022.md`; current claims below are anchored to exact [source excerpts](SOURCE_EVIDENCE_V022.md).

## Findings and integration dispositions

| Finding | Source basis | 0.22 disposition |
|---|---|---|
| Supervisor already converges approval/admission/runtime effects. | A01–A02 | Extend existing owner; do not introduce a new policy service with independent grants. B258. |
| Cortex uses Authorized rather than bare effects and has a real registered-check execution seam. | A03–A04 | Preserve type boundary; inject a narrow independent CheckExecutor. B264. |
| axon-vm is the actual Firecracker launcher; generic full VM/WASM/remote claims are not established. | A05, F01/F05 | Extract library preserving CLI; qualify one exact Linux profile. B262–B263. |
| Reflex serving is not proof of a learned CLM decision core. | A06, prior 0.21 review | Keep existing serving seam; no learned-provider success prerequisite. B272. |
| The source package-gate caller retains legacy assumptions. | A07 | Migrate caller schema/path/CLI/count logic, not just its version string. B283. |
| MiCode support docs embed v0.3 and mark important integration roles Intended. | M01–M03 | Real peer delta and source-backed round trips now included. B256/B268/B269. |
| Worktree management is implemented, but Git worktrees are not physical confinement. | M05, F04/F05 | Reuse manager and add enforceable Fabric scope. B261. |
| Delegation source explicitly documents unsupported path/tool subset validation. | M04 | Enforce at actual dispatch or refuse unprovable relationships. B267. |
| Execution-context preflight and stale-on-return behavior are already specified. | M06–M07 | Implement/verify use sites; exact frozen base is a stricter new paired-trial profile, not a claim the source already requires equality globally. B266. |
| The specific MiCode trust-judge graduation verdict is NoGo. | M08 | Preserve its separate evidence rule; Fabric/EVL pass is not graduation. B267. |
| MiCode already has strategy persistence with domain-specific semantics. | M09 | Add linked experiment/episode evidence, not destructive table rename or a duplicate learning authority. B269/B281. |
| Fabric separates original hashes, durable workspace, request identities, lifecycle and financial uncertainty. | F02–F06 | Sidecars retain distinctions; unknown cost is never free and cancellation is not cleanup. B257/B260/B265/B270. |
| The historical 0.21 MiCode 0.15 file was a proposal without supplied peer source. | P02 | Retain as history; current delivered delta is source-backed and does not fabricate a global version. |

## Review limits

Targeted searches for the context-receipt named Rust types did not establish a complete implementation; an equivalent differently named implementation may exist. The implementation agent must trace the actual task/worker/integrator call paths. The source archives lack a live Git object database in this delivery, so archive hashes—not asserted full commit IDs—are the authoritative source pins.

No current public research fetch was needed to produce this source-grounded integration release. Original research names and pins remain provenance, not newly verified performance evidence. Neither independent replication nor production quality is asserted.

## Preservation and changes

Parent task/gate rows and protected neural/ACE bytes are machine-checked. New owner sections add integration obligations only. Fabric's complete reference is copied byte-for-byte and its separate M3/M4 requirements remain deferred. Package-tool version checks change only for the active 0.22 release; historical input locks retain their historical meanings.

Source code in either extracted input tree was not modified. The read-only upgrade planner compares the full reviewed inventories and treats all drift as requiring review, not as instructions to erase local changes.
