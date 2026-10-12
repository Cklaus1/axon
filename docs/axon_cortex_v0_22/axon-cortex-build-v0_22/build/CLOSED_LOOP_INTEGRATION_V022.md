# Axon 0.22 — closed-loop integration contract

**Proposed normative integration requirements. Runtime implementation/evidence: NOT_RUN.**

## 1. Objective and source basis

Deliver one controlled, preauthorized path in which Axon proposes a bounded policy change, MiCode executes alternatives, Compute Fabric provides qualified execution isolation and lifecycle evidence, independent evaluation and admission determine the result, and a later MiCode task uses the admitted policy or remains on the incumbent. A regression must produce a fenced rollback or pause.

The sources already provide the ownership model: CX-16/MX-12 for the bridge, CX-03/CX-13 for action/authority/runtime boundaries, CX-01/CX-21 for evaluation, CX-11 for admission, CX-29/CX-33 for improvement/experiments, and ACF-01 for compute. See [source evidence](../review/SOURCE_EVIDENCE_V022.md), [owner map](../integration/V022_OWNER_MAP.json) and [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json).

This contract integrates those sources. New choices introduced here are explicitly the **0.22 pilot profile**: exact frozen trial bases; tool/skill shortlisting as the sole initial mutation; host-side inference with offline guest effects; and separate engineering/activation/improvement claims. They are not assertions that these choices already exist in source.

## 2. Ownership and dependency direction

Axon owns hypothesis history, policy/evidence interpretation and independent policy admission through existing owners. MiCode owns local permissions, resolved model/tool execution and session/build-loop integration. Fabric owns authority-checked job lifecycle, reservations, machine/workspace resources, cancellation and observed receipts. A trusted verifier owns conclusions about the exact tested artifact. The candidate proposer/worker must not control the final evaluator, admission rule, held-out data or active-policy pointer.

Reuse existing stores and append sidecar references. Do not add a second generic event database, action registry or approval system merely because the profile has a new name. A module-first implementation is allowed. Pure compute/bridge types cannot depend cyclically on cognitive/runtime owners or force provider SDKs into interpreter/browser builds. Current source uses CX-35 for Reflex serving; retain the alias rather than allocate a conflicting replacement.

## 3. Initial execution topology

A trusted MiCode host receives a task and policy. It resolves current local permissions and observes the actual worktree/model/build context **before the first task model turn**. Approved model calls remain on this host, through current credential and permission mechanisms. Code or tool outputs are untrusted.

Only registered, authorized executable closures and validated arguments reach Fabric. Fabric materializes a durable isolated input workspace and runs native/generated-code checks in the independently qualified offline Linux microVM profile. The guest has no provider secrets, no operator-source write access, no hidden verifier data and no network egress. An independent verifier checks the exact output. General shell execution, optional egress, remote services and hosted WASM are not implicitly added by this bridge.

A worktree is a content/workflow boundary, not a sandbox. A process_scoped adapter cannot meet a microVM requirement. Missing KVM/profile qualification blocks that scope; it never changes the requirement to whatever backend is available.

## 4. Identity and data boundary

Preserve source-system task and episode IDs. Add a versioned envelope linking Task, ExperimentArm, Trial, Attempt, Operation, Execution, Branch, policy version, effective context, workspace input/output and verifier references. A retry of identical transport input may reuse an OperationId; new execution uses a new AttemptId; repeated same-task/arm trials use new TrialIds. A task digest plus seed is not a unique run ID.

Keep separate records for expected context, independently observed preflight, supervisor-observed execution and independently evaluated outcome. Hashes bind canonical bytes but do not authenticate issuers. Production resolves references from an authenticated registry/connection, verifies current issuer authorization and tenant/scope, and then checks all cross-record identities. Document fixtures do none of that authentication.

Existing ACE/Reflex/episode formats and digest rules remain unchanged. New field semantics require the profile handshake or a pinned migration adapter. Unsupported fields/profiles cannot be silently dropped, defaulted or reinterpreted. Closed schemas reject duplicate keys, escaped aliases, unknown fields, non-finite/fractional/unsafe integers, overlarge or deeply nested inputs and malformed references before dispatch.

## 5. The pilot is deliberately narrow

The only variable is a versioned **tool/skill shortlisting policy** over already-eligible candidates. It may reorder or remove eligible candidate IDs; it cannot introduce tools, permissions, model/role/provider changes, verifier changes, runtime flags, new compute profiles, secret routes or larger budgets. Empty/invalid shortlists cause recorded abstention or the independently permitted incumbent shortlist.

Hold model/provider revision, task family, task budgets, context policy, eligible capability registry, environment, verifier and data/authority scope fixed. CLM, Jev/RLCD, pijev, routing research and RLM-Cascade remain independently qualified alternatives. Their claims, loss curves or synthetic demos are not dependencies of this initial mechanism. The 0.21 research requirements and optional paths remain intact for later controlled arms.

## 6. Frozen experimental controls

Before confirmation outcomes are accessible, register discovery/tuning/confirmation/final-reporting roles, immutable task/repository sets, allowed mutation and search budget, repetitions, order/randomization, cache regime, concurrency regime, independent sampling unit, sample/horizon and exact analysis method. Define quality noninferiority, safety constraints and the minimum meaningful economic benefit; require explicit missing/unknown outcome and cost handling. Record the number of attempted candidates and adjustment or independent confirmation for multiple comparisons.

Repeated trials of one task are not automatically independent samples. Candidate generation may use allowed discovery data, not held-out confirmation results. Do not keep checking the final set and treating it as unseen; retire contaminated holdouts or apply the preregistered sequential design. The included pilot template has required operator fields unset and activation false. No real experiment starts until they are resolved and independently approved.

## 7. Lifecycle and economic invariants

Follow ACF's action and machine state machines separately. A durable transactional journal records operation/input/configuration/authority/reservation/expected version before effects; recovery reconciles instead of blindly executing replay. Same operation with changed input is a conflict. An idempotency key does not guarantee exactly-once external side effects.

Reserve combined model, execution, verification, retry and speculation budgets. Failed/cancelled/lost work remains charged or an explicitly outstanding liability. Cost unknown is null/unknown, not zero. Cancellation acknowledgement, process stop, resource cleanup, receipt finalization and billing settlement are different facts. Aggregate utilization and experiment spending cannot be bypassed through child jobs.

Every trial starts from the declared durable base and isolated mutable build caches. Deduplicated immutable artifacts are allowed only under the declared cache policy and tenant scope. Fairness is measured and recorded, not assumed from equal wall-time limits.

## 8. Verification, admission and activation are separate

Verification evaluates exact bytes using frozen nonzero checks. Policy admission is a separate owner decision using eligible evidence, frozen thresholds and current authorization. A ranking probability, worker DONE message, hash chain, process exit or Fabric profile flag cannot substitute for either.

After independent admission, activation atomically advances a scoped policy pointer with expected prior digest and monotonic fence. New tasks record the effective adopted version. In-flight tasks keep their original pin. No adoption acknowledgement means no deployment claim. Workspace CAS publication is distinct from policy-pointer CAS; success in one does not imply authorization in the other.

Rollback is a **new current-authority decision** to a still-valid admitted predecessor. If it is revoked, incompatible or weaker than the task protection, pause/refuse. Stop/drain/reconcile outstanding jobs and liabilities before dismantling their route. An ordinary subprocess is not a protected rollback target.

## 9. Claims and release evidence

Keep three fields separate: `engineering_qualified`, `policy_activated`, `measured_improvement_supported`. Package conformance establishes none of them. Actual source/host/peer evidence is required for engineering qualification. An exact adoption record plus subsequent-task observation is required for activation. Independent held-out statistical evidence is required for an improvement claim.

No candidate is required to win. An operator-approved mechanism-test policy may exercise activation and rollback when no genuine candidate improves, but its receipts must label it as a mechanism fixture and exclude it from claims/training. Rejection, harm and inconclusive results remain evidence. Autonomous operation stays within prior human/owner-approved scope; it cannot expand its own permission or modify the admission/evaluation constraints.

## 10. Required and deferred scope

The bounded release maps ACF-T00–T08 and ACF-G00–G37. M3/M4 memory checkpoint/fork, egress/secret broker, generic WASM, remote provider and learned placement work is retained as deferred reference, not silently discarded. Do not use ACF-G54's whole-Fabric requirement as the bounded M0–M2 exit gate; B285 explicitly scopes its joint release evidence. The independently gated optional B286 study is not a forced release winner.

Implementation details and test recipes are in [the work packages](WORK_PACKAGES_V022.md). These task edges select required contract/use-site slices; they do not claim the entire old backlog is complete. Existing source implementations can satisfy a slice only with current attributable evidence.
