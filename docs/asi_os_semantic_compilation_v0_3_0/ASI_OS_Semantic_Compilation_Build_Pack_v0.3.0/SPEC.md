# ASI OS Semantic Compilation Specification
**Version:** 0.3.0  
**Contract family:** `asi.semantic-compilation/v0.3`  
**Status:** Proposed; implementation gates are explicit  
**Date:** 6 October 2026  
**Supersedes:** The conversation's v0.1 draft, including its illustrative pseudocode

## 1. Purpose and claim boundary
ASI OS SHALL support translating an authorized task specification into an inspectable executable plan. Intelligence creates, evaluates, repairs and improves plans; deterministic infrastructure schedules operations and enforces limits. Reducing inference cost and latency without degrading task outcomes is a **testable objective**, not an established result.

The source of desired behavior is the approved intent plus explicit constraints and acceptance criteria. The release record also retains the exact generated plan, implementation artifacts and execution evidence. Generated code is not thrown away when needed for reproducibility, audit, debugging or rollback. Natural-language intent alone is insufficient to reconstruct an exact release.

This specification defines a subsystem above ordinary operating systems, runtimes and compilers. It does not replace host process isolation, cryptography, a database, or a traditional compiler. It does not promise that all work can be made deterministic or that a language model can prove arbitrary business intent correct.

`MUST`, `SHALL` and `MUST NOT` define implementation requirements. `SHOULD` requires an explicit justification when not followed. Every release claim must identify which requirements have executable evidence.

## 2. Architecture and ownership
The four adaptive roles are separate:

| Component | Responsibility | Not authorized to do |
|---|---|---|
| Semantic Compiler | Normalize intent, resolve capabilities, emit candidate plans and proposed checks | Grant permissions or certify its own correctness |
| Evaluation Fabric | Run deterministic checks, typed judges, reviews and human evaluation; aggregate evidence | Treat a score as proof or modify protected criteria |
| Repair Worker | Propose bounded artifact or plan revisions from localized diagnostics | Apply changes directly to active execution or weaken tests |
| Axon Optimizer | Select among eligible strategies; record experiments; recommend promotions | Override policy, invent outcomes or self-promote without a gate |

The trusted runtime control plane owns admission, policy, identity, budgets, durable state, dispatch and release. MiCode is a proposed compiler/repair adapter, not a required renamed product. BFlow is a proposed intent/workflow integration. Existing interfaces must be inspected before either is bound.

```text
Approved intent + acceptance contract
             |
       Semantic compiler
             |
  Candidate Semantic Plan / AIR projection
             |
   Structural + semantic validation
             |
   Evaluation / optimization proposals
             |
       Revalidation + admission
             |
      Policy-enforcing runtime
             |
        Artifact + evidence
             |
     Independent acceptance decision
        /           |           \
     accept       repair       abstain/escalate
        |           |                   |
   release      new revision       no unsafe action
        \___________|___________________/
                    |
          experience log -> Axon
```

Every optimizer transformation produces a new candidate. It goes through the same validation and admission boundary as original compiler output. Runtime authorization is checked again at dispatch, not only at compile time.

### 2.1 Cortex v0.23 alignment and owner profile
When integrated with Axon Cortex v0.23, this package is an **integration profile**, not a new owner of concepts Cortex already defines. The canonical mapping is:

| Concern | Canonical owner in Cortex profile | Semantic Compilation role |
|---|---|---|
| Intent meaning and approval | CX-19 IntentIR | normalize/compile from an approved reference |
| Cognitive/execution semantics | CX-04 AIR | emit/project candidate execution structure |
| Program/software semantics | Axon Forge v0.3 SIR | explicit software-semantic lowering and transform binding |
| Capability authority | CX-03 | requester only; current grants rechecked |
| Isolation/effects | CX-13 | request qualified execution; consume receipts |
| Evaluation | CX-21 plus protected evaluator owners | compile evaluation plan/projections; never self-certify |
| Admission/promotion | CX-11 / CX-18 | candidate only |
| Evidence/provenance | CX-32 UEG | emit projection payloads |
| Experiments | CX-33 | attach domain profile to canonical experiment |
| Resources | CX-34 / Compute Fabric | consume epoch-bound projection |
| Coding workflow | CX-16 / MiCode | adapter/consumer |
| Self-application strategy | CX-29 | versioned candidate strategy only |

The package's JSON schemas are portable boundary contracts and offline fixtures. Under the Cortex profile they MUST reference, project, or lower into the canonical owners above rather than becoming parallel authorities.

## 3. Scope and release slices
### 3.1 Contract v0.3
The portable reference plan is a finite DAG. Version 0.3 separates **semantic operation** from **execution mechanism**. Semantic operations are `observe`, `rule`, `retrieve`, `decide`, `generate`, `reason`, `predict`, `check`, and `act`. Execution mechanisms are `transform`, `model.call`, `tool.call`, `code.execute`, `evaluate`, and `human.request`; `assert` and `return` are structural reference-harness mechanisms. A capability declares which semantic operations it may realize.

This distinction is mandatory because the same semantic operation can move from a frontier model to a small model, deterministic code, a tool, or a native runtime without changing task meaning. In the Cortex profile, these semantic operations lower to or project onto CX-04 AIR; this package MUST NOT create a second VM or a competing AIR owner.

A node has explicit input bindings, output types, dependencies, semantic operation, a pinned capability where applicable, and bounded retry/time/cost requests. Independent nodes may run concurrently under the scheduler. No node, capability projection, resource projection, evaluator result, or cached plan is itself an authorization grant.

Version 0.3 rejects unknown fields and operations. `branch`, `loop`, `map`, `reduce`, `subplan.call`, arbitrary expression strings, shell interpolation and in-place plan patches are not valid wire operations. Later versions may add them after their control, effect and recovery semantics are specified. The supervisor can select a new immutable plan after an unexpected state; this is not a hidden unbounded loop inside the IR.

### 3.2 Implementation phases
P0 establishes contracts and a safe fixture-backed path. P1 adds a real compiler and isolated deterministic execution. P2 adds evidence-based evaluation and bounded repair. P3 adds durable recovery and constrained low-risk effects. P4 adds controlled optimization experiments. P5 adds reusable procedure promotion. `build/BUILD_PLAN.md` defines entry/exit gates; this archive only implements the offline subset described in `PACKAGE_STATUS.md`.

### 3.3 Non-goals
Native binary generation, training RLCD, autonomous policy updates, unrestricted shell execution, arbitrary workflow discovery in production, and a universal UI are outside the initial implementation. Real GPUs, browsers and human systems are adapters, not mandatory v0.3 dependencies.

## 4. Task source and intent admission
**SC-001.** An intent includes an identifier, task class, goal, typed input declarations, output expectations, requested permissions, risk class, hard limits and acceptance-contract reference. The principal and tenant are supplied by authenticated transport; compiler-provided identity is never authoritative.

**SC-002.** Inferred assumptions MUST be labeled and distinguished from user-declared requirements. Material ambiguity about scope, irreversible action, success criteria or authority returns `NEEDS_CLARIFICATION` rather than a guess. The user can approve a normalized specification before compilation.

**SC-003.** Context consists of scoped artifact references plus relevant extracted content. Retrieval MUST respect access control, freshness and data classification. A fetched document, repository comment or tool response cannot modify policy or the evaluation rubric by containing instructions.

The intent schema is `schemas/intent.schema.json`. An artifact hash binds the approved normalized intent. Changes produce a new hash and require fresh admission when they affect authority or acceptance.

## 5. Compilation contract
**SC-004.** `compile` returns `CANDIDATE`, `NEEDS_CLARIFICATION`, `UNSUPPORTED` or `FAILED`, with diagnostics and provenance. It never returns a plan labeled “correct” solely because generation succeeded.

The pipeline is normalization, capability discovery, decomposition, lowering, proposed verification, budget estimation, schema validation, semantic validation, candidate evaluation, optimization, revalidation and emission. Policy snapshots are trusted input; the compiler receives only capability metadata it is permitted to use.

**SC-005.** Candidate count, repair attempts, inference spend, wall time and context size are bounded by one run-level budget. Failed candidates and verifier calls count against it. Compiler estimates are advisory; enforceable reservations come from the control plane.

**SC-006.** Compilation provenance records compiler/model identifiers, provider response model version when available, prompt/template digest, context digests, capability-registry digest, policy snapshot, configuration digest and strategy version. Unknown provider revisions are recorded as unknown, not invented.

**SC-007.** Mandatory evaluation criteria come from an owner-approved contract. The compiler may propose additional criteria, but cannot remove, lower or redefine protected checks. Public tests that a generator sees cannot serve as the only promotion holdout.

## 6. Semantic Plan and type semantics
**IR-001.** Portable Semantic Plans are immutable JSON objects conforming to `schemas/plan.schema.json`. They are a boundary/reference projection, not a claim to own Cortex AIR semantics. JSON Schema checks structural shape [S02]; semantic lint adds graph, registry, reference, effect and acceptance checks. Schema-valid does not mean authorized, correct or safe.

**IR-002.** Each node ID is unique within a plan. All dependencies exist, no node depends on itself, and the graph is acyclic. Each data reference names a declared input or an output port of a transitive dependency. A node cannot read an unordered peer's output.

**IR-003.** Binding syntax is exactly one of `{"input":"name"}`, `{"node":"id","port":"name"}` or `{"literal":value}`. Strings are data, not executable references. There is no `eval`, dynamic import, shell expansion or arbitrary expression evaluator in v0.3.

**IR-004.** Wire types are `string`, `integer`, `number`, `boolean`, `object`, `array`, `artifact`. Only integer-to-number widening is implicit. An `artifact` is a scoped immutable descriptor, not an arbitrary filesystem path. Detailed payload schemas and semantic domain identities belong to registered capabilities; shallow type matching does not prove domain correctness. Production admission checks literal payloads and runtime outputs against pinned detailed schemas.

**IR-005.** A plan has exactly one terminal `return`. All nodes must be its ancestors or that return node. Mandatory work-plan assertions must dominate successful return through dependencies. The separate evaluation contract is applied before release even when a work graph completed successfully.

**IR-006.** The capability registry projection, not generated node annotations, determines declared effects, replay class, argument/return contracts, implementation identity, mechanism and allowed semantic operations. The authoritative capability/resource owners remain external under an integrated owner profile. An implementation upgrade changes the pinned registry or capability revision.

**IR-007.** Sequence is a dependency chain. Parallel execution is permitted only for unordered nodes with compatible effects and available resources. A scheduling optimization cannot reorder observable writes, merge stochastic samples intended to be independent, or cache mutable reads without a declared freshness contract.

## 6.1 Ownership, lowering and representation boundaries
**OWN-001.** Every deployment selects a versioned owner profile. In `cortex_v0.23` mode, intent, execution semantics, authority, isolation, evaluation, admission, evidence, experiments and resource truth resolve to their Cortex owners. Local caches and schemas are projections only.

**OWN-002.** IntentIR, AIR and Forge SIR are distinct representations with distinct identities. Any IntentIR→AIR, AIR→SIR, source→SIR or SIR→backend lowering records source/target references and evidence. No representation silently overwrites another.

**OWN-003.** A plan node's `semantic_op` is distinct from its `mechanism`. Capability registration declares allowed semantic operations. A model call cannot become a `rule` merely by labeling it deterministic, and a tool invocation cannot hide an `act` effect behind a retrieval label.

**OWN-004.** Experimentable compiler, repair, evaluation, routing, retrieval, plan-optimization, resource-placement and software-transform behavior has a stable `StrategyArtifact` identity and lineage. Prompt/config mutation without strategy identity is not promotion evidence.

**OWN-005.** A semantic compilation record binds approved intent, candidate plan, strategy, evaluation contract, registry/config snapshots and explicit lowering records. In the Cortex profile it also binds canonical owner references. This record is provenance, not authority.

## 6.2 Forge software-semantic bridge
**FORGE-001.** Software/compiler tasks MAY lower AIR-relevant work into Forge SIR, but the bridge is explicit and version/hash pinned. Forge owns SIR/Transform/Migration/BuildProgram/Backend contracts; Semantic Compilation does not copy those schemas into a competing namespace.

**FORGE-002.** Supported software changes SHOULD use Forge Transform or Migration artifacts with preconditions, preserved invariants, semantic diff and required verification. Text patching remains a fallback when no semantic representation is available.

**FORGE-003.** Compiler/build/backend success is insufficient for acceptance. Protected execution must bind to external runtime/environment receipts and independent semantic/correctness/security evidence.

**FORGE-004.** Coding truth resolution prefers current compiler probes/source/installed libraries/conformance evidence/versioned local specs over stale external docs or model priors. Conflicts are surfaced with provenance rather than collapsed by model confidence.

## 6.3 Canonical evidence, experiments and resources
**XPL-001.** Optimization trials attach to a canonical experiment identity with hypothesis, falsifier, controlled mutation, evaluator identity, metrics, corpus role and stopping rule frozen before protected outcomes. Domain profiles cannot self-admit results.

**XPL-002.** Hard correctness, security, authority and acceptance gates are applied before Pareto or utility ranking. No weighted score can rescue an invalid candidate.

**XPL-003.** Evidence emitted by compilation, evaluation, repair, build or benchmark activity attaches to a canonical evidence identity and records material effective inputs, environment and resource epoch. Local experience rows do not substitute for required evidence lineage.

**XPL-004.** Resource observations are epoch-bound projections from the authoritative scheduler/fabric. They may inform planning, but current availability and grants are rechecked before protected dispatch. A projection never reserves or authorizes resources.

## 7. Evaluation Fabric and Jev-like decisions
**EV-001.** Compilation produces a work plan and references an independently approved evaluation contract. The contract specifies criterion IDs, target artifacts or ports, evaluator IDs, evidence requirements, required versus advisory status, escalation policy, repair budget and release policy. Contract identity is part of plan identity.

**EV-002.** The Fabric can combine deterministic tests, schema/type checks, static analysis, provenance checks, typed decision models, generative reviewers and human reviews. Typed decisions and generative critique are separate interfaces. Jev's documented primitives are Choice, Score and Noul; they do not generate repair text [S03–S05]. A Jev-like adapter is optional; a provider-neutral envelope is the core contract.

**EV-003.** A result records evaluator version, target digest, criterion ID, evidence references, status, raw decision values when available, calibration reference and usage. Allowed statuses are `pass`, `fail`, `abstain`, `error`. Missing, timed-out, stale or malformed evaluations cannot count as passes.

**EV-004.** A selected option, probability of a proposition, rubric score and provider confidence are different quantities. The system MUST retain their distinct meanings. A 0.95 quality score is not a 95% probability of task success. Calibration is measured on labeled task distributions, not asserted for an individual result [S06].

**EV-005.** Learned acceptance requires a separately versioned decision policy and held-out validation for the task family and operating conditions. Confidence and probability are not multiplied across correlated criteria to manufacture a “plan reliability.” Distribution shift, judge disagreement and insufficient evidence cause abstention or escalation.

**EV-006.** Deterministic failures cannot be averaged away by high subjective scores. For high-impact effects, a learned judge is supplementary evidence; authorization and mandatory deterministic preconditions remain hard gates. The compiler, repair worker and optimizer cannot edit those gates.

**EV-007.** Evaluators are sandboxed, budgeted, traceable and exposed to minimal authorized context. Candidate text and retrieved state are untrusted content. Evaluation prompts must identify the actual criterion in instructions; adapter field names must not be assumed to be visible to a model.

**EV-008.** Pre-execution ranking is a forecast, not an observed outcome. A cheap plan score may decide which safe candidate to test but cannot establish execution correctness. Candidate ordering, self-preference, answer length, injected persuasion and incomplete candidate sets are evaluated for bias.

## 8. Repair and adaptive execution
**RP-001.** Failed evaluation may issue a `RepairRequest` with the original goal, protected criteria, failed targets, permitted edit surface, input/evidence hashes, expected parent revision and remaining budget. The request is diagnostic data, not authority.

**RP-002.** A separate generator proposes a new artifact or plan. All changes are recorded as a new immutable revision. The supervisor compares the expected parent hash, revalidates, reruns affected checks and obtains new authorization when action arguments or scope changed. No blind application of arbitrary JSON Patch to a running plan is allowed.

**RP-003.** Repair invalidates the changed node and its transitive consumers. Reuse is allowed only when inputs, dependencies, implementation, tenant, policy, data freshness and nondeterministic capture contract still match. Final acceptance is rerun on the exact final artifact, including relevant regression criteria.

**RP-004.** Completed external effects are not undone by resetting graph state. Unknown outcomes require reconciliation. A repaired plan cannot resend a message or repeat a payment merely because the original output is missing. Compensation is a separately authorized business action, not proof of rollback.

**RP-005.** Exhausted repair attempts, unresolved authority, repeated oscillation, unavailable evidence or exhausted budget ends in `NEEDS_REVIEW` or failure. Adaptive mode is bounded supervision, not an unmetered reasoning escape hatch.

## 9. Security and authority
**SEC-001.** Compile, evaluate, optimize and repair components are untrusted with respect to execution authority. Effective permissions are the intersection of principal grants, tenant policy, task scope, current environment and resource limits.

**SEC-002.** Every effectful dispatch requires a fresh policy decision on exact arguments. Approval receipts bind principal, tenant, plan revision, node, action digest, target scope, expiry and use limits. A boolean returned by `human.request` is not an approval receipt.

**SEC-003.** Tool credentials are resolved by a broker after admission. Secrets do not appear in IR, prompts, model-visible registry metadata, default traces or repair requests. Egress restrictions apply to model calls as well as tools and code.

**SEC-004.** An evaluator cannot authorize spending, deployment or messaging. Safety checks precede effects; post-hoc evaluation alone is insufficient. Policy failures fail closed. The policy kernel may revoke previously admitted runs.

**SEC-005.** Production code execution requires isolated workspaces, restricted egress, bounded resources, immutable image/program identities and descendant-process cleanup. The host is never the fallback execution environment. Shell command strings from a model are not passed to a host shell.

**SEC-006.** Replay and experiments do not repeat irreversible effects. Effectful capabilities are disabled by default in shadow runs. Synthetic captures and production evidence are separate namespaces.

## 10. Runtime, scheduler and durability
**RT-001.** Run state and plan state are separate. A successful work graph can still fail evaluation; it is not released until acceptance succeeds. Required run states include `CREATED`, `ADMITTED`, `RUNNING`, `WAITING_APPROVAL`, `EVALUATING`, `REPAIR_PENDING`, `SUCCEEDED`, `FAILED`, `CANCELLED`, `NEEDS_REVIEW`.

**RT-002.** Node states include `PENDING`, `READY`, `RUNNING`, `SUCCEEDED`, `FAILED`, `CANCELLED`, `UNKNOWN_EFFECT`. Scheduling persists transition intent before dispatch and commits results with an idempotent event identity. No double-dispatch is inferred safe merely because a lease expired.

**RT-003.** Each invocation has a logical action identity distinct from attempt identity. Retries of one intended external action reuse its idempotency identity when the provider supports it. A new attempt still receives a new trace identity. For unsupported idempotency, automatic write retries are forbidden.

**RT-004.** The runtime does not claim universal exactly-once effects. After an ambiguous timeout, use the provider's reconciliation contract or require operator review. Crash tests must cover the gap between external success and local persistence.

**RT-005.** The scheduler reserves worst-case authorized resources before launching concurrent calls and reconciles actual usage afterward. Cost units are integer micro-USD; business transfer amounts are separate typed quantities. Unbounded-price operations cannot enter a hard-budget execution mode without an enforceable cap.

**RT-006.** Cancellation prevents new dispatch, propagates to workers and records residual costs or effects that cannot be canceled. Timeouts bound orchestration behavior; they do not retroactively cancel a provider's already accepted action.

## 11. Replay, identity and reuse
**ID-001.** The plan digest covers canonical plan content, intent reference, evaluation reference, dependency locks and configuration reference. Run IDs, task IDs, artifact IDs, candidate IDs, experiment IDs and provider request IDs are distinct.

**ID-002.** Three modes are explicit: `inspect` reconstructs a recorded trace; `captured` feeds recorded nondeterministic outputs to deterministic logic without new effects; `reexecute` performs a new authorized execution. Only the first two are replay. Live model calls are not presumed bitwise reproducible even with a seed or temperature setting.

**ID-003.** Semantic retrieval may suggest a reusable plan, but cannot admit it. Reuse checks the exact task contract, schemas, environment, input constraints, dependency versions, freshness, tenant boundaries and current policy. Result caching and plan caching are separate stores.

**ID-004.** Revoked authority, broken compatibility or stale safety contracts block execution. A cheaper model or a better strategy merely triggers reconsideration; it does not silently invalidate a working release. Model aliases may resolve to new versions, so resolved versions and unknown-version states are recorded.

**ID-005.** Procedural memory promotion, previously called “intelligence crystallization,” requires cross-instance validation, leakage checks, an owner, a version, a scope and rollback. A few successful runs do not establish a general reusable procedure.

## 12. Axon optimization and learning
**AX-001.** Axon receives context features, eligible candidates, uncertainty, cost/latency estimates and constraints. It returns a selection with policy version, exploration metadata and a reason record. The runtime can reject the selection.

**AX-002.** Experience records include `(experiment_id, task_id, trial_id, arm_id, run_id)`. This prevents collisions when the same task/arm is tested repeatedly. A task ID plus arm label alone is insufficient. Retries are nested attempts, not independent successes.

**AX-003.** Optimize total cost per successful task, including compilation, evaluation, repair, failed attempts, cached-input billing and amortized offline work where relevant. Record failure and abstention rates alongside cost so refusing difficult tasks cannot masquerade as efficiency.

**AX-004.** Safety, authority and mandatory quality checks are feasibility constraints. Preference, latency and cost rank eligible choices; they do not compensate for violated constraints. Claims of equivalence require empirical or formal evidence appropriate to the transformation.

**AX-005.** Initial operation is record-only. Exploration and promotion are disabled by default. A separately approved experiment policy defines task slices, randomization, sample size/power analysis, quality noninferiority margin, stopping rule, exploration budget and rollback. Numeric defaults in this package are examples, not validated deployment thresholds.

**AX-006.** Baseline interpreter, compiled-only, compiled-plus-evaluation and compiled-plus-repair arms are compared on frozen held-out tasks with paired trials when suitable. Holdouts are unavailable to generation and repair. Evaluate both fixed-model comparisons and current-best-strategy comparisons.

**AX-007.** Judges and repair policies are themselves evaluated, but they cannot co-optimize away the independent release yardstick. Evaluator updates and generator updates are tested separately before joint deployment. Avoid selection bias, repeated-test leakage and unplanned peeking.

## 13. Configuration and APIs
**CFG-001.** Runtime tunables, prompts, labels, rubrics, budget limits and routing profiles are versioned configuration, not duplicated magic literals. Schema constants, protocol enums and unit conversions remain explicit contract constants. Effective configuration is validated, hashed and attached to each run.

**CFG-002.** Authorized policy is separate from user preferences. Per-task requests can tighten limits, not relax authority. Running executions retain their admitted configuration snapshot except explicit emergency revocation or budget reduction. Hot changes do not silently alter an experiment arm.

The proposed CLI includes compile, inspect, verify, run, evaluate, explain, trace, replay, bench and promote. Only the `harness` and `tools` commands in this archive are implemented. The proposed service API and error classes are in `docs/API_CLI.md`.

## 14. Telemetry and operator experience
**OBS-001.** Every attempted node records plan/config/registry versions, start/end events, invocation identity, resource identity, usage, cache disposition, output references, errors and policy evidence. Raw sensitive inputs are not indiscriminately logged. Artifact access is audited and retention is policy-controlled.

**OBS-002.** The operator can inspect proposed work, required permissions, evidence behind acceptance, estimated versus actual spend, repair diffs, dependency invalidations and experimental versus active status. Explanations cite structured facts rather than inventing rationales from a model's private reasoning.

**OBS-003.** Quality is multi-dimensional. Code uses independent tests and behavioral checks; research uses claim/evidence checks; UI uses interaction/accessibility checks plus task-specific human preference. A citation URL's existence does not prove support, and aesthetic preference is not a universal scalar correctness measure.

## 15. Acceptance and release
**REL-001.** The package gate checks schemas, fixtures, selected semantic invariants and offline tests. It establishes package coherence only.

**REL-002.** Production gates additionally require real adapter contracts, denied-action tests, crash/reconciliation tests, exact-artifact evaluation, data-isolation checks, measured budget behavior, holdout outcomes, dependency review and operator approval. Missing evidence blocks the corresponding capability.

**REL-003.** Release evidence is tied to the exact source revision, dependency lock, config hash and artifact digest. Any post-gate change invalidates that gate. A report must list unexecuted tests rather than interpreting their absence as success.

**REL-004.** The initial thesis is supported only if, on predeclared task slices, compiled execution meets the approved quality criterion and improves the selected cost/latency metric after all overhead. A negative result narrows the task class or keeps interpreted execution; it is not hidden by changing the rubric.

## 16. Supporting contracts
`docs/IR_SEMANTICS.md` defines wire-level rules; `docs/EVALUATION_AND_REPAIR.md` defines judge and repair boundaries; `docs/SECURITY.md` defines threats and tests; `docs/RUNTIME.md` defines recovery. Machine-readable schemas define structural wire constraints. Where prose and schemas disagree, fail the build and resolve the conflict—do not choose whichever permits execution.

Sources and provenance are in `SOURCES.md`. Architectural choices beyond source documents are proposals made in this package, not claims about existing Axon, MiCode, BFlow or Jev implementations.
