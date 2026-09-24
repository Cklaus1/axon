# 0.22 source-bound work packages

**Status for every task: Not started in this portable proposal.** Do not reset actual source status. Preserve current implementations by mapping fresh evidence to required behavior. Product commands below are recipes, not executed results.

The master DAG is in `task_manifest.json`; this view adds implementation/read/test detail. Old B00–B254 are retained. New profiles do not depend on the all-provider B250 closure. The original ACF task/gate requirements are referenced, not redefined.

## B255 — Reconcile Axon, MiCode and Fabric source truth

Stage: M0. Owners: CX-00. Dependencies: none. Optional: no. Product status: `NOT_RUN`.

Pin all supplied inputs and actual dirty/untracked target work; map existing CX/MX/ACF owners and legacy release profiles. Preserve B00-B254, old evidence, schemas, source CX-35 and independent Cargo versions. Resolve drift before source edits.

### Read / edit surface

Axon/MiCode AGENTS.md and current build/CI callers; scripts/cortex_package_gate.sh; docs/axon-support (existing).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Fabric obligations

Complete mapped ACF-T00 obligations from [the preserved Fabric plan](../integration/compute-fabric-v0_1-reference/build/IMPLEMENTATION_PLAN.md); its product gates remain independently required.

### Required assertions / acceptance

**G00-r22-source-rebase** — The reviewed Axon/MiCode archives and target working trees are compared without writes; every changed, missing, unsafe and untracked path receives an owner disposition before editing. A matching filename or reported Git label is not a verified revision.

**G00-r22-preservation** — The v0.21 task/gate identities and meanings, CX-36 r0.2, ACE/neural wire formats, historical evidence and source-local identifiers survive migration; no live completion or runtime version is reset by importing this document pack.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B256 — Negotiate the existing CX-16 / MX-12 bridge profile

Stage: M0. Owners: CX-16. Dependencies: B255. Optional: no. Product status: `NOT_RUN`.

Implement a closed, opt-in sidecar profile for existing bridge artifacts. Negotiate exact schemas/features and local compatibility adapters; never infer support from document version 0.22 or rename ACE/Reflex wire versions.

### Read / edit surface

axon: crates/cortex-policy-adapter/src/main.rs and crates/axon-cortex/src/runner.rs (existing); micode: crates/micode/src/{assembly,headless,task_wiring}.rs and delegate/persist use sites (existing; exact insertion to be traced); micode: docs/axon-support/specs/MX-12-axon-bridge.md (existing owner).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G16-r22-closed-wire** — Both real peers refuse duplicate or escaped-alias keys, unknown closed fields, malformed digests, unsafe numeric values and incompatible required capabilities before model calls or dispatch; a syntactic fixture does not prove transport interoperability.

**G16-r22-negotiation** — Old episodes remain readable through pinned adapters; an absent/old peer produces explicit Unsupported or retained-authority incumbent operation, never silent field loss, fabricated peer support or weaker protected execution.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B257 — Bind identities and immutable evidence sidecars

Stage: M0. Owners: CX-10, CX-32. Dependencies: B256. Optional: no. Product status: `NOT_RUN`.

Map Task/ExperimentArm/Trial/Attempt/Operation/Execution/Branch and policy/context/ACF receipt references without altering existing episode hashes. Bind receipts to authenticated producers and immutable artifacts; add same-episode and multi-attempt reconciliation.

### Read / edit surface

axon: crates/axon-cortex/src/episode.rs (existing); existing episode/evidence owners; authenticated sidecar resolver (proposed); micode: crates/micode-persist/ (existing owner); sidecar/usage projection module (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G10-r22-trial-identity** — Repeated runs of one task and arm use distinct TrialIds; transport retry reuses only the identical OperationId/input binding, while an authorized new execution uses a new AttemptId. Same semantic task ID never deduplicates a fresh trial.

**G32-r22-sidecar-bindings** — Import validates task/arm/trial/attempt/operation, principal, policy, effective context, input/output workspace and verifier bindings against authenticated stored records; matching hash-shaped strings or a worker issuer claim cannot authenticate evidence.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B258 — Converge compute and peer requests at existing authority

Stage: M0. Owners: CX-03, CX-13. Dependencies: B256, B257. Optional: no. Product status: `NOT_RUN`.

Reuse Axon supervisor/grant admission and MiCode local permission owners. Resolve executable closure/profile/approval/resource references from trusted registries, not caller text. Recheck current epochs at all effects and retirement routes.

### Read / edit surface

axon: crates/axon-cortex/src/runner.rs (existing); axon: crates/axon-os/src/supervisor.rs (existing); pure compute admission module (proposed); axon: crates/axon-os/src/{runtime,supervisor,grant}.rs (existing); axon: crates/axon-vm/src/main.rs (existing); journal/profile/library adapter modules (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Fabric obligations

Complete mapped ACF-T01 obligations from [the preserved Fabric plan](../integration/compute-fabric-v0_1-reference/build/IMPLEMENTATION_PLAN.md); its product gates remain independently required.

### Required assertions / acceptance

**G03-r22-authority-intersection** — Every new submit, inspect, cancel, reconcile, promotion and peer-import route enforces current local authority and resource scope; possession of Axon policy, Fabric handle or MiCode receipt grants no additional capability.

**G13-r22-profile-eligibility** — Only the exact independently qualified profile/configuration/host within its freshness policy can satisfy requested isolation; source_inspected, experimental, withdrawn or unresolved capability combinations are refused before effects.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B259 — Preserve the truthful legacy process adapter

Stage: M1. Owners: CX-13. Dependencies: B258. Optional: no. Product status: `NOT_RUN`.

Retain scoped interpreter semantics and CLI parity; introduce private staging, bounded process/output handling and truthful process_scoped labels. Never translate unknown native effects into an empty effect set.

### Read / edit surface

axon: crates/axon-os/src/{runtime,supervisor,grant}.rs (existing); axon: crates/axon-vm/src/main.rs (existing); journal/profile/library adapter modules (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Fabric obligations

Complete mapped ACF-T02 obligations from [the preserved Fabric plan](../integration/compute-fabric-v0_1-reference/build/IMPLEMENTATION_PLAN.md); its product gates remain independently required.

### Required assertions / acceptance

**G13-r22-legacy-scope** — Legacy interpreter/program runs retain declared-effect and authority semantics and existing parity tests; a fake Axon filename cannot dispatch arbitrary native shell work.

**G13-r22-no-weak-fallback** — A request requiring qualified microVM protection is refused or paused when unavailable; rollback, timeout, feature disable and provider selection never route it to an ordinary subprocess or Git worktree.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B260 — Implement durable operation journal and aggregate reservations

Stage: M1. Owners: CX-13. Dependencies: B258, B259. Optional: no. Product status: `NOT_RUN`.

Implement transactional intent-before-effect, uniqueness, fencing, outbox/reconciliation and shared budget reservations. Keep action, resource, billing and cleanup states separate. Fault-test actual persistent storage.

### Read / edit surface

axon: crates/axon-os/src/{runtime,supervisor,grant}.rs (existing); axon: crates/axon-vm/src/main.rs (existing); journal/profile/library adapter modules (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Fabric obligations

Complete mapped ACF-T03 obligations from [the preserved Fabric plan](../integration/compute-fabric-v0_1-reference/build/IMPLEMENTATION_PLAN.md); its product gates remain independently required.

### Required assertions / acceptance

**G13-r22-journal-before-effect** — A crash at each reservation/journal/dispatch boundary recovers one recorded operation without blind re-execution; same OperationId with changed immutable request is refused, and the tested persistent store survives process restart.

**G13-r22-aggregate-reservation** — Sibling jobs, model calls, verifier work and retries share an atomic task/experiment ceiling; concurrent reservations cannot overspend and failed or cancelled work is not dropped.

**G13-r22-unknown-reconcile** — A timeout after a possible effect yields OutcomeUnknown with outstanding cleanup/billing liability; no exactly-once claim, immediate free-budget refund or automatic duplicate-effect retry is inferred from an idempotency key.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B261 — Materialize immutable workspaces with per-trial isolation

Stage: M1. Owners: CX-03, CX-28. Dependencies: B260. Optional: no. Product status: `NOT_RUN`.

Reuse MiCode WorktreeManager and Cortex observation digests through explicit projections. Publish durable content versions, fresh per-trial build/dependency caches and fenced leases; harden import and resource quotas.

### Read / edit surface

axon: crates/axon-cortex/src/runner.rs (existing); axon: crates/axon-os/src/supervisor.rs (existing); pure compute admission module (proposed); existing context/working-set owners; durable workspace projection (proposed); micode: crates/micode-git/src/worktree.rs and EXECUTION_CONTEXT_RECEIPT_SPEC.md (existing).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Fabric obligations

Complete mapped ACF-T04 obligations from [the preserved Fabric plan](../integration/compute-fabric-v0_1-reference/build/IMPLEMENTATION_PLAN.md); its product gates remain independently required.

### Required assertions / acceptance

**G03-r22-workspace-import** — Materialization rejects traversal, absolute paths, unsafe links/devices, namespace collisions and quota expansion; immutable inputs and outputs have retrievable byte-complete manifests with explicit omissions.

**G28-r22-workspace-not-context** — Canonical context, scoped WorkspaceSnapshot observations and durable WorkspaceVersion contents remain distinct identities joined by an explicit omission/freshness projection; a transcript or hash-only observation never becomes a bootable filesystem.

**G03-r22-trial-isolation** — Incumbent/challenger workspaces and mutable build caches cannot contaminate one another or the integration checkout; an unauthorized path write is blocked at the actual enforcement boundary, not merely detected in a later diff.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B262 — Extract axon-vm as a profiled backend without CLI drift

Stage: M1. Owners: CX-13. Dependencies: B255. Optional: no. Product status: `NOT_RUN`.

Refactor the existing Firecracker launcher only as required for a narrow library adapter. Preserve CLI, interpreter/browser dependency direction and attestation meaning; pin actual backend versions during implementation.

### Read / edit surface

axon: crates/axon-os/src/{runtime,supervisor,grant}.rs (existing); axon: crates/axon-vm/src/main.rs (existing); journal/profile/library adapter modules (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Fabric obligations

Complete mapped ACF-T05 obligations from [the preserved Fabric plan](../integration/compute-fabric-v0_1-reference/build/IMPLEMENTATION_PLAN.md); its product gates remain independently required.

### Required assertions / acceptance

**G13-r22-vm-cli-parity** — The existing axon-vm CLI and applicable legacy tests retain behavior after extraction; provider SDKs and native codegen are not introduced into core interpreter/browser builds.

**G13-r22-guest-truth** — The custom Axon guest demo is never advertised as Linux or a complete native execution environment; backend labels enumerate tested engine/enclosure/guest/OS/architecture combinations rather than a tier hierarchy.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B263 — Physically qualify the protected Linux microVM profile

Stage: M1. Owners: CX-13, CX-03. Dependencies: B258, B260, B261, B262. Optional: no. Product status: `NOT_RUN`.

On a suitable Linux/KVM host, test host/guest boundaries, dedicated UID/cgroup/process ownership, networking denial, rootfs/image pinning, crash cleanup and quotas. Keep production qualification empty until these tests run.

### Read / edit surface

axon: crates/axon-cortex/src/runner.rs (existing); axon: crates/axon-os/src/supervisor.rs (existing); pure compute admission module (proposed); axon: crates/axon-os/src/{runtime,supervisor,grant}.rs (existing); axon: crates/axon-vm/src/main.rs (existing); journal/profile/library adapter modules (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Fabric obligations

Complete mapped ACF-T06 obligations from [the preserved Fabric plan](../integration/compute-fabric-v0_1-reference/build/IMPLEMENTATION_PLAN.md); its product gates remain independently required.

### Required assertions / acceptance

**G03-r22-physical-isolation** — A real Linux microVM blocks host source/credential access and egress, enforces mount/child-process/resource/output limits, and survives adversarial guest behavior with independently observed host checks.

**G13-r22-profile-qualification** — Profile qualification binds source/build, engine/image/configuration, tested host and nonzero product assertions to a trusted evidence issuer; fixture data or a verified flag without those receipts cannot enable protected dispatch.

**G13-r22-launch-cleanup** — Injected failures after process/cgroup/socket/disk acquisition leave no unowned resources; cancel acknowledgement is not cleanup completion and stopped is not destroyed.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B264 — Wire one real registered Cortex CheckExecutor

Stage: M1. Owners: CX-01, CX-03. Dependencies: B260, B261, B263. Optional: no. Product status: `NOT_RUN`.

Introduce the narrow Runner registered-check seam with a separate fixture implementation and Fabric-backed implementation. Execute frozen checks outside subject authority against exact output bytes, keeping hidden checks outside worker-visible context.

### Read / edit surface

axon: crates/axon-cortex/src/runner.rs (existing); axon: crates/axon-cortex/src/runner.rs::run_tests_json (existing seam); axon: crates/axon-os/src/supervisor.rs (existing); pure compute admission module (proposed); micode: crates/micode-verify/ (existing); independent CheckExecutor adapter (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Fabric obligations

Complete mapped ACF-T07 obligations from [the preserved Fabric plan](../integration/compute-fabric-v0_1-reference/build/IMPLEMENTATION_PLAN.md); its product gates remain independently required.

### Required assertions / acceptance

**G01-r22-registered-check** — One registered check resolves its pinned executable/dependency closure and executes through the real protected host path; process exit zero, zero matched checks or a fabricated worker result cannot close verification.

**G01-r22-verifier-separation** — The subject cannot edit the verifier binary, fixture, rubric, hidden expected outputs or issued receipts; trusted independent evaluation binds the exact candidate artifact and profile, with denied/failed/unknown distinctions preserved.

**G03-r22-check-effects** — The actual check dispatch is within current approved authority and profile requirements; arbitrary executable/argv substitution and stale authority are refused with zero backend effects.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B265 — Join preflight, execution and independent outcome evidence

Stage: M1. Owners: CX-32. Dependencies: B257, B264. Optional: no. Product status: `NOT_RUN`.

Keep ExecutionContextReceipt, Fabric ExecutionReceipt, Cortex/MiCode episode and independent verifier result as separate referenced records. Reconcile all IDs and output digests; reject altered, missing or cross-tenant evidence.

### Read / edit surface

existing episode/evidence owners; authenticated sidecar resolver (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G32-r22-receipt-roles** — Context preflight proves an observed launch context only; supervisor-observed execution proves process facts only; an independent verifier proves its specific check outcome. No receipt role is silently upgraded into another.

**G32-r22-artifact-recheck** — Repairing, rebasing, renaming or replacing the selected output invalidates verification for old bytes; all admitted and promoted artifacts are the exact independently checked versions.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B266 — Gate MiCode workers on observed execution context

Stage: M1. Owners: CX-16. Dependencies: B256, B261. Optional: no. Product status: `NOT_RUN`.

Implement the documented ExecutionContextReceipt lifecycle and three use sites: task engine, worker-before-first-model-turn and result integrator. Observe context independently; establish exact trial base for the pilot, namespaces, role, model route and build cache.

### Read / edit surface

axon: crates/cortex-policy-adapter/src/main.rs and crates/axon-cortex/src/runner.rs (existing); micode: crates/micode/src/{assembly,headless,task_wiring}.rs and delegate/persist use sites (existing; exact insertion to be traced); micode: docs/axon-support/specs/MX-12-axon-bridge.md (existing owner).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G16-r22-preflight-start** — A mismatched repository, exact trial base, observed branch/worktree, role, namespace, provider/model or dedicated build namespace yields TASK_NOT_STARTED before the first task model turn and before effects; a parent-echo receipt is rejected.

**G16-r22-preflight-return** — Result intake rechecks current integration head, context receipt and declared write scope; stale worker success is classified for rebase/conflict/obsolescence, never silently admitted against a different base.

**G16-r22-role-scope** — Read-only critics/verifiers/documentation/implementation roles have distinct enforceable context and write/build contracts; experiment subjects cannot build in shared mutable namespaces or widen their own declared write set.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B267 — Enforce MiCode scope intersection and preserve trust graduation

Stage: M1. Owners: CX-16, CX-03. Dependencies: B258, B266. Optional: no. Product status: `NOT_RUN`.

Close or explicitly refuse unprovable delegation subset semantics at actual tool/file dispatch. Preserve PermissionGate and the independent trust-judge NoGo/graduation mechanism; no imported evaluator changes that verdict.

### Read / edit surface

axon: crates/axon-cortex/src/runner.rs (existing); axon: crates/axon-os/src/supervisor.rs (existing); pure compute admission module (proposed); axon: crates/cortex-policy-adapter/src/main.rs and crates/axon-cortex/src/runner.rs (existing); micode: crates/micode/src/{assembly,headless,task_wiring}.rs and delegate/persist use sites (existing; exact insertion to be traced); micode: docs/axon-support/specs/MX-12-axon-bridge.md (existing owner).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G16-r22-local-authority** — An Axon policy may only select from already-permitted candidates; delegated tools and concrete file effects are checked against inherited local limits. Unknown glob/subset relations are denied or escalated for explicit review.

**G16-r22-trust-graduation** — MiCode trust-judge enforcement stays behind its existing graduation evidence and per-mode criteria; a new EVL or passing Fabric check cannot rewrite PUBLISHED_VERDICT or imply trust-judge graduation.

**G03-r22-dispatch-recheck** — Permissions, scope, revocation epoch and candidate identity are rechecked at actual tool execution after shortlisting and immediately before effects, including nested delegation and retries.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B268 — Consume policies through the real MiCode bridge

Stage: M1. Owners: CX-16. Dependencies: B256, B266, B267. Optional: no. Product status: `NOT_RUN`.

Wire versioned policy import into the actual session/headless/build-loop composition, behind explicit opt-in. Enforce applicability, scope, active policy version and evidence requirements. Keep only one adoption owner.

### Read / edit surface

axon: crates/cortex-policy-adapter/src/main.rs and crates/axon-cortex/src/runner.rs (existing); micode: crates/micode/src/{assembly,headless,task_wiring}.rs and delegate/persist use sites (existing; exact insertion to be traced); micode: docs/axon-support/specs/MX-12-axon-bridge.md (existing owner).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G16-r22-real-consumer** — A pinned real Axon producer and real MiCode consumer exchange a policy through an actual coding/build-loop use site, record the effective policy digest and preserve subsequent-turn recovery; a dummy reader cannot close this gate.

**G16-r22-candidate-shortlist** — The pilot only reorders or narrows a known eligible tool/skill set; no new tool, permission, verifier, model, compute profile, credential route or budget is introduced by a policy candidate.

**G16-r22-provider-host-boundary** — Approved inference runs in the trusted permission-enforced host path with secrets excluded from guest/episode exports; the initial guest remains offline, and denied broker/egress requirements never silently enable guest network access.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B269 — Export complete MiCode episodes to actual Axon intake

Stage: M1. Owners: CX-16, CX-10. Dependencies: B265, B268. Optional: no. Product status: `NOT_RUN`.

Produce canonical versioned episode sidecars from observed MiCode execution. Join before/after artifacts, denied/failed/cancelled/unknown outcomes, effective input, policy context and authoritative cost; preserve sensitivity and corpus roles.

### Read / edit surface

axon: crates/axon-cortex/src/episode.rs (existing); axon: crates/cortex-policy-adapter/src/main.rs and crates/axon-cortex/src/runner.rs (existing); micode: crates/micode-persist/ (existing owner); sidecar/usage projection module (proposed); micode: crates/micode/src/{assembly,headless,task_wiring}.rs and delegate/persist use sites (existing; exact insertion to be traced); micode: docs/axon-support/specs/MX-12-axon-bridge.md (existing owner).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G16-r22-real-producer** — The actual MiCode episode exporter reaches the actual Axon intake and round-trips known/unknown, requested/effective and predicted/observed facts without omission or manufactured defaults across normal and failed tasks.

**G10-r22-all-attempts** — Every model/tool/check attempt including retries, abandoned speculation, refusals, failures and unresolved outcomes remains attributable to a unique trial and included in denominator and cost policy.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B270 — Reconcile whole-task inference and execution economics

Stage: M1. Owners: CX-10, CX-13. Dependencies: B260, B269. Optional: no. Product status: `NOT_RUN`.

Join price-weighted model usage and Fabric CPU/memory/disk/check costs through shared budget owners. Pin currency, price schedule, usage source, experiment cohort and pending liabilities. Separate estimates from measured costs.

### Read / edit surface

axon: crates/axon-cortex/src/episode.rs (existing); axon: crates/axon-os/src/{runtime,supervisor,grant}.rs (existing); axon: crates/axon-vm/src/main.rs (existing); journal/profile/library adapter modules (proposed); micode: crates/micode-persist/ (existing owner); sidecar/usage projection module (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G10-r22-full-task-cost** — Cost comparisons include uncached input, cached input, output, inference/encoder, checks, failed/retried/cancelled work and execution charges under pinned price schedules; cache hits and GPU time are not assumed free.

**G13-r22-billing-settlement** — Duplicate accounting receipts are idempotent only under identical origin/sequence/content; unresolved usage remains unknown with conservatively reserved liability and cannot contribute a spurious zero-cost winner.

**G10-r22-cohort-denominator** — All assigned tasks are retained in paired outcomes; failures are not omitted from average-cost reporting and successes alone cannot redefine the comparison denominator or experiment population.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B271 — Run logical A/B branches with fenced promotion

Stage: M2. Owners: CX-08, CX-11. Dependencies: B261, B264, B270. Optional: no. Product status: `NOT_RUN`.

Implement Fabric logical branches from one immutable base with independent TrialIds, AttemptIds, worktrees, mutable caches and carved shared budgets. Use CAS for workspace publication, separate from policy admission.

### Read / edit surface

existing Cortex admission owner and MiCode active policy consumption; scoped CAS/ack use sites (proposed); existing Cortex experiment owner; Fabric logical branch adapter (proposed); micode: docs/axon-support/specs/MX-08-challenger-lab.md (Intended).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Fabric obligations

Complete mapped ACF-T08 obligations from [the preserved Fabric plan](../integration/compute-fabric-v0_1-reference/build/IMPLEMENTATION_PLAN.md); its product gates remain independently required.

### Required assertions / acceptance

**G08-r22-logical-branches** — Incumbent and challenger start from the same frozen durable base and declared resource/cache regime with independent run identities; this is logical branching, not a claim of RAM forks.

**G11-r22-workspace-cas** — Workspace publication requires expected base, current fencing epoch, authorized writer, exact verified output and independent approval; a concurrent update forces conflict/rebase and reverification, never overwrite.

**G08-r22-branch-cancellation** — Stopping a losing branch preserves its events, usage and pending liabilities, reconciles descendants and leaves surviving branches isolated; cancellation does not erase an unfavorable outcome.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B272 — Bind the 0.21 profiles to a deterministic pilot baseline

Stage: M2. Owners: CX-05, CX-06, CX-26, CX-28, CX-34. Dependencies: B268, B270. Optional: no. Product status: `NOT_RUN`.

Use existing owned DEC/RTR/CVM/SPX/TEL contracts with explicit deterministic behavior: fixed model/route/context, no active speculation, known eligible candidates and full receipts. Learned providers remain separately qualified and optional.

### Read / edit surface

axon: crates/axon-reflex/src/lib.rs (existing seam); deterministic shortlist adapter (proposed); existing composition owner; disabled/qualified speculation policy gate (proposed integration); existing context/working-set owners; durable workspace projection (proposed); existing registry/eligibility owner; immutable eligible-candidate projection (proposed); existing routing owner; fixed provider/model/session control (proposed pilot profile); micode: crates/micode-git/src/worktree.rs and EXECUTION_CONTEXT_RECEIPT_SPEC.md (existing).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G34-r22-pilot-controls** — The pilot fixes model/role/provider version, eligible profile, verifier, authority, budgets and context settings; unknown or implicit default changes invalidate comparability rather than being attributed to the shortlist.

**G05-r22-decision-semantics** — Candidate rankings, relative probabilities, calibrated correctness and abstention remain distinct; CLM/Jev/pijev adapters are not required to win or even be active to establish the closed-loop mechanism.

**G26-r22-speculation-disabled** — Unqualified speculative effects remain disabled; any later approved branch retains exact selected bytes, shared cost and cancellation semantics instead of becoming an alternate dispatch authority.

**G28-r22-context-provenance** — Both arms retain the canonical-history/working-set projection and critical pinned constraints; hidden evaluation data is never made available by retrieval, compaction or a branch cache.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B273 — Generate one bounded EVO policy candidate

Stage: M2. Owners: CX-29. Dependencies: B272. Optional: no. Product status: `NOT_RUN`.

Use the existing hypothesis/evolution ledger to propose a tool/skill shortlist variant from allowed discovery evidence. Record mutation identity, rationale, parent, edit ceiling and all rejected hypotheses. Do not let proposer edit evaluation/admission.

### Read / edit surface

existing self-application/hypothesis owner; bounded shortlist mutation artifact (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G29-r22-bounded-mutation** — The proposer modifies only the allowlisted shortlist policy artifact and records its parent/version/evidence; changes to permissions, evaluator, promotion rule, experiment corpus or core code are rejected before execution.

**G29-r22-hypothesis-memory** — Rejected, inconclusive and failed candidates remain in immutable hypothesis history with tested task scope and verdict; they are not silently relabeled successful or retried under a new ID to erase prior evidence.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B274 — Freeze the independent experiment and admission plan

Stage: M2. Owners: CX-21, CX-33. Dependencies: B269, B273. Optional: no. Product status: `NOT_RUN`.

Register the task/repository split, candidate generation budget, repetitions, independent unit, fixed-horizon or valid sequential method, noninferiority/economic criteria and contamination controls before confirmation data is visible. Require an approved completed plan for live testing.

### Read / edit surface

existing benchmark/evaluation owners; paired held-out pilot driver (proposed); existing experiment register; immutable preregistration/analysis artifact (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G21-r22-protected-splits** — Discovery, tuning/calibration, promotion confirmation and final reporting roles are separated by task/repository lineage; hidden checks and protected outcomes never feed proposer prompts, CLM training or router fitting.

**G33-r22-decision-rule-freeze** — The exact statistical method, sample/horizon, multiple-comparison handling, minimum worthwhile improvement, quality margin, missing-data rule and cluster unit are frozen before protected results; optional stopping or repeated tasks do not inflate independent sample size.

**G21-r22-inconclusive-valid** — Too little evidence, regression, unknown outcomes or an unsupported superiority claim produces reject/inconclusive/no-claim, not a forced winner or a changed threshold after seeing outcomes.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B275 — Execute controlled real incumbent/challenger trials

Stage: M2. Owners: CX-08, CX-21. Dependencies: B271, B272, B274. Optional: no. Product status: `NOT_RUN`.

Run the real MiCode path on prespecified tasks in authorized isolated Fabric jobs. Block/randomize order, bound repeats, enforce model/cache/resource parity and capture actual effective policies and workspaces for both arms.

### Read / edit surface

existing Cortex experiment owner; Fabric logical branch adapter (proposed); existing benchmark/evaluation owners; paired held-out pilot driver (proposed); micode: docs/axon-support/specs/MX-08-challenger-lab.md (Intended).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G08-r22-real-paired-execution** — Real paired MiCode trials use the declared immutable inputs, fixed controls and unique identifiers, with preflight and Fabric receipts on both arms; a prerecorded fixture or a reused episode cannot count as a live execution.

**G21-r22-order-cache-controls** — The declared blocking/randomization, warm/cold cache policy, concurrent resource regime and provider revisions are recorded; drift is stratified or invalidates comparison rather than credited as policy improvement.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B276 — Evaluate exact candidates outside subject authority

Stage: M2. Owners: CX-01. Dependencies: B264, B265, B275. Optional: no. Product status: `NOT_RUN`.

Execute frozen deterministic checks and applicable evidence-linked checklists independently. Preserve whole-task conjunctive gates; CLM/Jev or worker judgments are auxiliary and cannot compensate for failed mandatory tests.

### Read / edit surface

axon: crates/axon-cortex/src/runner.rs::run_tests_json (existing seam); micode: crates/micode-verify/ (existing); independent CheckExecutor adapter (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G01-r22-nonvacuous-outcome** — Whole-task acceptance requires nonzero applicable mandatory checks over exact candidate bytes; a high checklist score, exit code zero or low cost cannot offset a failed correctness/security requirement.

**G01-r22-independent-issuer** — Verifier receipts are authenticated through trusted issuer lookup and bind profile/configuration/artifact/verifier revision; subject-generated, stale or cross-task evidence is rejected even if its JSON and hashes validate.

**G01-r22-unknown-outcome** — Timeout, cancellation, unmatched checks, missing evidence and unverifiable output remain distinct non-success states through the bridge and statistical analysis; no default pass is supplied.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B277 — Apply the frozen independent policy-admission decision

Stage: M2. Owners: CX-11. Dependencies: B274, B276. Optional: no. Product status: `NOT_RUN`.

Use existing admission ownership to evaluate held-out confirmation evidence under frozen constraints. Record accepted/rejected/inconclusive separately from candidate ranking and require current scope/revocation checks; never autoactivate the top-ranked candidate.

### Read / edit surface

existing Cortex admission owner and MiCode active policy consumption; scoped CAS/ack use sites (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G11-r22-independent-admission** — Policy admission requires a non-subject authority, complete artifact/experiment/verifier bindings and the prespecified evidence rule; proposer, learned ranker and Compute Fabric have no self-promotion right.

**G11-r22-admission-disposition** — Accepted, rejected and inconclusive have explicit immutable reasons; only accepted and currently authorized evidence permits activation, and stale/unknown costs or safety failures cannot be hidden by aggregate utility.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B278 — Prove fenced policy activation and future-task uptake

Stage: M2. Owners: CX-11, CX-16. Dependencies: B268, B277. Optional: no. Product status: `NOT_RUN`.

Atomically switch a scoped active-policy pointer using expected incumbent digest and monotonic epoch. Pin it at task start and record acknowledgements/effective digest in subsequent real MiCode tasks; do not live-edit active trials.

### Read / edit surface

axon: crates/cortex-policy-adapter/src/main.rs and crates/axon-cortex/src/runner.rs (existing); existing Cortex admission owner and MiCode active policy consumption; scoped CAS/ack use sites (proposed); micode: crates/micode/src/{assembly,headless,task_wiring}.rs and delegate/persist use sites (existing; exact insertion to be traced); micode: docs/axon-support/specs/MX-12-axon-bridge.md (existing owner).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G11-r22-policy-cas** — Activation checks expected active policy, monotonic fence, exact admitted candidate, scope and revocation immediately before publication; competing/stale activations fail rather than overwrite one another.

**G16-r22-future-task-uptake** — After real admission, a later independent MiCode task consumes and reports the exact activated policy digest, not just a stored benchmark winner; failed acknowledgement yields quarantine/paused routing rather than claimed deployment.

**G16-r22-inflight-policy-pin** — In-flight tasks retain their original pinned policy/version and receipts; a subsequent activation cannot relabel their earlier actions or mix two policies within an unrecorded trial.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B279 — Exercise rollback, revocation and safe paused states

Stage: M2. Owners: CX-11, CX-13. Dependencies: B260, B278. Optional: no. Product status: `NOT_RUN`.

Run real regression/failure injection against a scoped activation. Roll back by a new fenced decision only to a still-valid admitted predecessor, or pause. Drain/reconcile protected jobs and keep accounting/readers.

### Read / edit surface

axon: crates/axon-os/src/{runtime,supervisor,grant}.rs (existing); axon: crates/axon-vm/src/main.rs (existing); journal/profile/library adapter modules (proposed); existing Cortex admission owner and MiCode active policy consumption; scoped CAS/ack use sites (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G11-r22-rollback-revalidate** — A rollback rechecks predecessor artifact, current applicability, permissions, profile qualification and revocation; a revoked or weaker predecessor causes a paused/refused state, not unsafe fallback.

**G13-r22-rollback-lifecycle** — Crash during activation or rollback reconciles the durable journal and active pointer without split-brain authority; active jobs are cancelled/drained according to policy and liabilities remain accounted.

**G11-r22-regression-observed** — An intentionally injected regression triggers the configured independent monitor, a recorded rollback/pause and a later task using the expected safe version; the injection is labeled a mechanism test, not measured improvement evidence.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B280 — Fault-test cancellation, budgets and restart across peers

Stage: M2. Owners: CX-13, CX-16. Dependencies: B260, B263, B264, B271. Optional: no. Product status: `NOT_RUN`.

Inject duplicate/out-of-order messages, process death, lost acknowledgements, worker timeout and unknown billing. Verify journals, inherited cancellation, operation reconciliation, authenticated receipt ingestion and non-duplicated resource charges.

### Read / edit surface

axon: crates/axon-os/src/{runtime,supervisor,grant}.rs (existing); axon: crates/axon-vm/src/main.rs (existing); journal/profile/library adapter modules (proposed); axon: crates/cortex-policy-adapter/src/main.rs and crates/axon-cortex/src/runner.rs (existing); micode: crates/micode/src/{assembly,headless,task_wiring}.rs and delegate/persist use sites (existing; exact insertion to be traced); micode: docs/axon-support/specs/MX-12-axon-bridge.md (existing owner).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G13-r22-restart-matrix** — Actual process/store restarts at every important effect boundary leave no unowned worker or silent repeated consequential operation; cleanup and financial obligations reconcile or remain explicitly pending.

**G16-r22-peer-failure-matrix** — Peer outage, replayed messages, stale epochs, partial episode export and schema mismatch preserve local authority and unknown status; reconnecting does not duplicate activation, billing or task effects.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B281 — Enforce learning eligibility and data-retention boundaries

Stage: M2. Owners: CX-10, CX-25. Dependencies: B269, B274. Optional: no. Product status: `NOT_RUN`.

Route only eligible observed histories to training/retrieval with license, tenant, retention and corpus-role labels. Candidate-selected data is not unbiased counterfactual evidence; protected confirmation data stays excluded.

### Read / edit surface

axon: crates/axon-cortex/src/episode.rs (existing); existing learning-plane owner; eligible episode projection (proposed); micode: crates/micode-persist/ (existing owner); sidecar/usage projection module (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G10-r22-eligibility-projection** — Missing data-use, tenant, provenance, model/evaluator revision or corpus-role facts block learning export; redaction retains a hash-bound omission/projection record rather than claiming unchanged canonical content.

**G25-r22-no-self-label-loop** — CLM/router labels come from independently eligible outcomes, not the model voting itself correct; unobserved alternatives remain unknown, and protected/final evaluation evidence cannot train the next proposer.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B282 — Close joint adversarial and authority-bypass tests

Stage: M2. Owners: CX-03, CX-32. Dependencies: B267, B276, B279, B280, B281. Optional: no. Product status: `NOT_RUN`.

Exercise malicious repository/tool output, receipt forgery, context echo, scope widening, candidate substitution, hidden-test exfiltration, cache cross-contamination and re-entry through fallback/replay/attach. Track every finding and limitation.

### Read / edit surface

axon: crates/axon-cortex/src/runner.rs (existing); axon: crates/axon-os/src/supervisor.rs (existing); pure compute admission module (proposed); existing episode/evidence owners; authenticated sidecar resolver (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G03-r22-joint-bypass** — All public/child/fallback/retry routes reject attempts to bypass authority, host/guest confinement, verifier separation or forbidden write sets, with nonzero actual source/host tests on the selected profile.

**G32-r22-evidence-laundering** — Mutated receipts, forged issuer identities, hash-only success, missing attempts and cross-tenant references cannot cross independent admission even when individually schema-valid.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B283 — Integrate compatible package and cross-project source gates

Stage: M0. Owners: CX-00, CX-16. Dependencies: B255, B256. Optional: no. Product status: `NOT_RUN`.

Update package-gate invocation only after source review of schema/CLI/count conventions; add scoped source tests and interoperability checks without hiding missing Rust/KVM prerequisites or altering historical evidence.

### Read / edit surface

Axon/MiCode AGENTS.md and current build/CI callers; axon: crates/cortex-policy-adapter/src/main.rs and crates/axon-cortex/src/runner.rs (existing); micode: crates/micode/src/{assembly,headless,task_wiring}.rs and delegate/persist use sites (existing; exact insertion to be traced); micode: docs/axon-support/specs/MX-12-axon-bridge.md (existing owner); scripts/cortex_package_gate.sh; docs/axon-support (existing).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G00-r22-package-gate-upgrade** — The actual repository gate validates the 0.22 inventory/CLI/report semantics without zero-count false failures or self-hash cycles; an older vendored pack is retained until references migrate deliberately.

**G16-r22-source-ci-scope** — Paired Axon/MiCode revision, build features, host/profile, executed test names and nonzero assertions are captured for source/interop jobs; skipped or unavailable compiler/KVM/provider tests remain NOT_RUN or BLOCKED.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B284 — Validate the complete 0.22 specification and reference pack

Stage: M0. Owners: CX-00. Dependencies: B255, B256, B257. Optional: no. Product status: `NOT_RUN`.

Maintain generated master/views, schema fixtures, parent/Fabric checksums, source pins, requirement/task/gate mappings and distinct runtime qualification ledger. Run documentation/reference tests separately from product gates.

### Read / edit surface

Axon/MiCode AGENTS.md and current build/CI callers; scripts/cortex_package_gate.sh; docs/axon-support (existing).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G00-r22-pack-integrity** — Manifests, owner amendments, dependency closures, generated views, protected parent bytes and vendored Fabric bytes agree; every new task/gate has a source-derived or explicitly proposed rationale and execution recipe.

**G00-r22-honest-status** — All unexecuted product obligations remain NOT_RUN; offline reference/demo success is explicitly neither Rust implementation, physical backend evidence, real MiCode interoperability nor measured self-improvement.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B285 — Qualify the bounded operational closed loop

Stage: M2. Owners: CX-16, CX-29. Dependencies: B279, B282, B283, B284. Optional: no. Product status: `NOT_RUN`.

Assemble independent nonvacuous runtime evidence for the selected dependency closure, including ACF-G00-G37. Demonstrate real proposal, trials, legitimate rejection and controlled activation/uptake/rollback within preauthorized scope. Activation mechanics may use a clearly labeled approved fixture policy, never a fabricated improvement claim.

### Read / edit surface

axon: crates/cortex-policy-adapter/src/main.rs and crates/axon-cortex/src/runner.rs (existing); existing self-application/hypothesis owner; bounded shortlist mutation artifact (proposed); micode: crates/micode/src/{assembly,headless,task_wiring}.rs and delegate/persist use sites (existing; exact insertion to be traced); micode: docs/axon-support/specs/MX-12-axon-bridge.md (existing owner).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G16-r22-operational-loop** — One authorized real Axon-MiCode-Fabric workflow completes identity-bound dispatch, evaluation, admission/refusal, future-task policy use and rollback/pause, with qualified physical backend and real peer evidence; fixtures alone cannot close the release.

**G29-r22-no-forced-winner** — Mechanism qualification may finish with a rejected or inconclusive challenger. A separately labeled operator-approved mechanism-test policy may exercise activation, but cannot be recorded as a learned or measured winner.

**G16-r22-bounded-activation** — The release report enumerates exactly qualified scopes/features and explicitly disabled/deferred ones; autonomous operation stays within preauthorized pilot limits and cannot expand its own admission or evaluation rules.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).

## B286 — Report evidence for or against a real improvement claim

Stage: M3. Owners: CX-21, CX-29. Dependencies: B285. Optional: yes. Product status: `NOT_RUN`.

Execute the separately approved held-out reporting study for the selected task distribution; report uncertainty, paired outcomes, full task costs and failure classes. Evidence may support benefit, harm or inconclusive; assert improvement only if the prespecified rule and independent evidence support it.

### Read / edit surface

existing benchmark/evaluation owners; paired held-out pilot driver (proposed); existing self-application/hypothesis owner; bounded shortlist mutation artifact (proposed).

Existing paths are confirmed source seams or owner directories; proposed modules/functions require use-site tracing and must not be assumed implemented. See [source review](../review/SOURCE_REVIEW_V022.md) and [evidence](../review/SOURCE_EVIDENCE_V022.md).

### Implementation sequence

1. Capture baseline behavior and reproduce the relevant negative case under existing authority; record unavailable prerequisites instead of weakening the test.
2. Implement the smallest additive change through the owners above; preserve historical encodings, live evidence and current permission/profile requirements.
3. Add actual source/use-site tests plus failure/recovery cases and a receipt mapping each obligation below to executed evidence. Keep reference fixtures separately labeled.

### Required assertions / acceptance

**G21-r22-measured-claim** — Any improvement statement names actual task/repository population, independent sample/cluster unit, paired outcome and cost evidence, frozen statistical method and uncertainty. Simulated/injected fixtures or training loss cannot establish the claim.

**G29-r22-claim-separation** — Engineering-qualified, policy-activated and measured-improvement-supported are separate release fields; absence of a justified benefit leaves measured_improvement_supported false without manufacturing a winner.

### Evidence and stop condition

Record source/build/peer/profile pins, authenticated invoker, actual command and nonzero test cases/assertions, independent artifact/receipt paths, and observed result. Stop this scope on authority/profile drift, unexplained source changes, wrong context, missing prerequisite or absent independent evidence. Do not turn an offline model into a runtime claim. Use [runtime evidence recipes](RUNTIME_EVIDENCE_V022.md).
