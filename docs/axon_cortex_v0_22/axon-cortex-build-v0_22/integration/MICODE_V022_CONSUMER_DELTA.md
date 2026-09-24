# MiCode consumer delta for Axon 0.22

**Delivered integration specification; all new behavior Intended / runtime NOT_RUN.** This is not a global MiCode v0.22 product or a claimed v0.15 support release. It reconciles the supplied `micode-src-8ffc2504(1).zip` with Axon v0.21 and ACF v0.1.0. The archived filename is a source label, not a Git-verified commit.

## Existing owners and exact changes

| Owner / actual source | Required delta | Master tasks |
|---|---|---|
| `docs/axon-support/specs/MX-12-axon-bridge.md` | Negotiate versioned sidecars; real policy intake and complete episode export, not dummy-reader-only evidence. | B256–B257, B268–B269 |
| `docs/axon-support/specs/MX-08-challenger-lab.md` | Controlled immutable A/B trials and frozen hidden evaluation; bounded live pilot. | B271, B274–B276 |
| `EXECUTION_CONTEXT_RECEIPT_SPEC.md` | Implement task/worker/integrator receipt consumption before first model turn and at return. | B266 |
| `crates/micode-git/src/worktree.rs` | Reuse provisioning/cleanup, add durable input/version/fencing projection and per-trial caches; no sandbox claim. | B261, B266, B271 |
| `crates/micode-delegate/src/spawner.rs` and actual permission/tool dispatch | Enforce inherited concrete tool/file limits; refuse unsupported pattern subset relations. Preserve existing local decision owners. | B267 |
| `crates/micode/src/assembly.rs`, `headless.rs`, `task_wiring.rs`, core loop use sites | Choose the real composition path during intake; add opt-in policy consumption, context gating, effect dispatch and exact effective-policy receipt. Proposed edits are not assumed existing APIs. | B268, B275, B278 |
| `crates/micode-persist/src/strategy_memory.rs`, existing persistence owners | Reference policy/experiment history without renaming old strategy-memory tables or pretending similarity retrieval is admission. | B269, B281 |
| `crates/micode-verify/src/graduation.rs` | Preserve the specific trust-judge NoGo/graduation mechanism; no forced Go or coupling to Fabric pass. | B267 |

## Preflight contract

The source requires independently observed context and base ancestry for general implementation workers. The new **paired-trial profile is stricter**: its declared base/workspace version must match exactly before trial work; ancestry alone could hide extra changes in one arm. Outside that profile the original role-specific source requirements remain unchanged.

Observe repository, actual head/branch/worktree/cwd, role, namespace existence/absence, resolved provider/model, dedicated build target and effective scope. The worker observes facts rather than echoing parent expectations; the trusted host binds and stores the observation. Check before spending task-model tokens or enabling task effects. Result intake rechecks current integration base and classifies stale results; do not auto-merge just because worker tests passed.

Parent path globs/tool allowlists are not accepted as enforcement merely because they are populated. At the actual permission/tool/file boundary, intersect local inherited authority with the permitted candidate view. If a pattern relation is not supported, refuse or require explicit reviewed narrowing; never assume child patterns are narrower.

## Runtime topology

Keep inference calls and credentials in the existing trusted host. Submit registered native checks/build effects through Fabric after local and Axon authority convergence. The first guest is offline and cannot directly call model providers. An imported policy cannot supply arbitrary executables, verifier binaries, resource profiles or API keys.

Existing permission prompts and policy are not bypassed. For unattended pilot execution, preauthorize the bounded task/candidate/effect scope through existing mechanisms. Uncovered actions stop for review rather than silently enabling an autonomous superuser.

## Current-versus-intended evidence

The supplied support roadmap is labeled v0.3 and Intended. `ExecutionContextReceipt` named types were not located in Rust in the targeted search; that is a limitation of symbol-based inspection, not proof that no equivalent functionality exists. Re-inventory actual use sites before implementation. The 0.21 `MICODE_V015_PROPOSED_DELTA.md` remains historical; this reviewed delta supersedes its assumption that no MiCode source was available.

## Peer acceptance

Both peers must reject bad versions, receipt/candidate substitution, authority injection, stale activation and unqualified profile selection. Show actual policy ingestion and actual episode output on a real coding task, subsequent task uptake, and rollback. Negative tests include disconnected peer, revoked credentials, lost acknowledgement, unknown billing, empty/invalid shortlist, changed task model and stale integration head. Mocks demonstrate wire contract only.

Candidate source test commands are listed in [runtime evidence recipes](../build/RUNTIME_EVIDENCE_V022.md). They are not execution claims. Preserve MiCode's own AGENTS/build protocol and local test requirements.
