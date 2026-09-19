# Build plan: Axon Cortex

## Objective

Deliver an intent-first, interpreter-first, capability-bounded software problem solver and grow it into the cognitive control plane of a self-optimizing Axon stack. Demonstrate that each cognitive addition improves task economics, then federate eligible experience from MiCode/external repositories and crystallize verified discoveries into guarded skills, tools, libraries, compiler transforms or runtime primitives. Do not start with a custom foundation model, a new syntax family, or an OS rewrite.

This is a dependency-ordered engineering plan, not a calendar promise. Repository intake and measurements determine effort. All proposed code paths, commands and work packages remain unimplemented in this deliverable.

## Milestones and evidence

| Stage | Deliverable | Main specs | Required demonstration | What it does not claim |
|---|---|---|---|---|
| M0 — Establish reality | Existing-Axon evidence map, trusted policy, resettable fixtures, baselines and schema contracts | CX-00/01, schema portion of CX-10, audit portion of CX-15 | Required gates/engine limitations are known; task and budget policy is locked; no missing check counts as success. | No Cortex runtime or new model yet. |
| M1 — One safe repair loop | Observer, registry, host isolation, bounded patch/check executor, AIR serial runner, independent verifier, replay | CX-02/03/04/10/13; mock and existing-model adapters | Repair an intentionally broken Axon example in a copy; reject malicious/stale actions; verify final patch independently; replay without real effects. Compare real-model AIR against the simple baseline. | No auto-merge, production action, custom model or learned simulator. |
| M2 — Typed, measured Reflex | Backend-neutral Reflex ABI; executable conformance fixtures; backend feature negotiation/effective-input receipts; dynamic candidate corpus; destructive controls; shared-state/prefix-cache measurements; calibration-aware router; multi-backend comparison | CX-05/06; interface part of CX-15 | Run the same protected decision/task corpus through all feasible conforming backend families; a learned-head pilot is optional at M2 rather than a prerequisite to justify itself; show selective quality/coverage plus cost/latency benefit for any adopted configuration, or retain the baseline. | A normalized score is not automatically calibrated correctness; one request is not guaranteed to be one neural pass; no backend gains execution authority. |
| M3 — Useful software predictions | Concrete-patch/outcome dataset, base-rate and learned short-horizon predictors | CX-07 | Predict held-out outcomes and improve real check/planning choices after charging model cost; expose abstention/model exploitation failures. | No complete world simulation or causal guarantee. |
| M4 — Evidence-seeking planning | Subgoal controller, hypotheses, permitted experiments, reset/control evidence | CX-08 | Choose an informative real check and resolve a debugging ambiguity more efficiently than controls. | No general causal discovery from observational traces alone. |
| M5 — Guarded crystallization | Eligible learning exports, artifact registry, independent admission, one reusable specialization, rollback | CX-10/11; small-model pilot under CX-14 | Promote a cheaper guarded tool/rule/policy or Reflex candidate with protected evidence and demonstrate deoptimization/rollback. | Not every task compiles to a rule; training millions of examples is not a prerequisite. |
| M6 — Representation and transfer | Alternative encodings, typed concept candidates, held-out novel-family evaluation | CX-09 | A new representation/abstraction improves a specified downstream transfer task beyond compute/representation-matched controls. | No universal fluid-intelligence claim. |
| M7 — Controlled learning portfolio | Per-pillar lifecycle, error attribution, curriculum, bounded meta-controller | CX-12 | At least two independently useful component learning pipelines share lifecycle/admission; meta proposals cannot edit their own exam. | No unrestricted self-modification or automatic TCB rewriting. |
| M8 — Experience federation and repository knowledge | MiCode bridge, external repo/history intake, provenance-rich patterns and local reproductions | CX-16/17 | Import a MiCode episode without authority transfer; extract one cross-repo candidate with counterexamples/provenance and reproduce it locally. | External popularity/repetition is not correctness and imported evidence is not automatically training-eligible. |
| M9 — OS-wide crystallization | Staged Pattern→Skill/Tool→Library/Compiler/Runtime promotion with independent admission/deoptimization | CX-18 with CX-11/15 | Promote one guarded capability from eligible experience, use it on held-out tasks, then demonstrate applicability failure/deoptimization; native promotion remains optional. | Not every learned behavior becomes native code; verifier/admission policy remains outside learner control. |

## Dependency structure

The core chain is M0 → M1 → M2 → M3 → M4 → M6. M5 begins after M2 plus eligible data and independent admission; it need not wait for advanced causal or representation research. M7 requires M5, M6 and at least two demonstrated learning pipelines. The MiCode bridge contract (CX-16/B44) may start after M1 evidence exists, but M8 knowledge claims require M6 representation/abstraction plus eligible imported data. M9 crystallization builds on CX-11 admission and M8 evidence; userland skill/tool promotion does not require compiler-native work. M2 is explicitly an **interface-and-bakeoff stage**, not a commitment to a particular Jev-like model architecture.

OS H0 is part of M1, not a late feature. Hosted-service H1 can follow M1 in parallel with learning. Language/type-map work starts as an audit early and becomes a prerequisite only when new cross-engine semantics are proposed. Specialized parallel-model research branches from actual pilot evidence. Bare-metal work is a separate owner-approved branch, never a blocker for M1.

## Reflex build principles added from the System-One ecosystem review

1. **Freeze the Axon Reflex ABI before picking a model.** TypeSafe/Jev compatibility is an adapter at the edge; internal contracts remain Axon-owned.
2. **Dynamic finite option sets are first-class.** Reflex scores runtime-generated symbols/tests/scopes/options, not only fixed classifier labels.
3. **DECIDE ≠ GENERATE ≠ ACT.** Reflex selects bounded semantics; Generate creates open-ended content; only the capability executor performs effects after revalidation.
4. **Probability provenance is typed.** Preserve native logits, sequence likelihoods, generated estimates, entropy-derived uncertainty and empirically calibrated probabilities as different evidence classes.
5. **Four backend families share one corpus and verifier.** Generative adapter, direct option logits, sequence scorer and learned decision head are compared under the same task/authority contract.
6. **Shared-state inference is measured, not assumed.** Record prefill/state-encoding, incremental question and option costs separately; benchmark serial, ordinary parallel and shared-prefix implementations.
7. **Destructive controls are mandatory.** Shuffled/empty/wrong/stale state, reordered/renamed candidates, irrelevant context and adversarial option sets test whether the backend truly uses the world state.
8. **Calibration is a routing property, not a JSON field.** Use grouped held-out outcomes, proper scoring, risk/coverage and Verified Utility at Coverage; a high score never creates authority.
9. **Question decomposition is learnable.** Store monolithic versus decomposed judgment formulations and deterministic composition rules; promote decompositions only when protected outcomes improve.
10. **Custom model research remains optional.** Small-model experiments start once useful verified data exists, but a new architecture is justified only by a demonstrated workload/serving bottleneck.

See [REFLEX_BACKEND_BAKEOFF](REFLEX_BACKEND_BAKEOFF.md), [REFLEX_DECISION_CORPUS](REFLEX_DECISION_CORPUS.md), and [REFLEX_CALIBRATION_LAB](REFLEX_CALIBRATION_LAB.md).

## First executable vertical slice

Use a resettable copy of a tiny Axon example containing a localized semantic bug. Visible reproduction checks and hidden completion tests are pinned outside the patchable region. Permit only inspect/search, one approved symbol-body patch, registered isolated checks and a completion claim.

Sequence: snapshot → partial semantic observation → legal action catalog → select action → generate a patch artifact → revalidate grant/payload/snapshot → apply in copy → run checks → independently verify patch and output → save evidence → replay. Record prediction attempts only once M3 exists; do not block this first loop on fabricated forecasts.

Start with deterministic fixture decisions for conformance, then the same loop with an existing model. A scripted golden test proves the plumbing; varied model-driven tasks establish solver behavior. Keep these results separate.

## Proposed implementation organization

Logical modules: contracts; observer; grants; executor; AIR scheduler; model adapters; evidence/replay; evaluation; registry/admission; and hosted supervisor integration. Start in a small number of modules/crates appropriate to existing Axon seams. Do not create sixteen crates because there are sixteen specs.

Prefer Rust for authority-sensitive host adapters and registry integration; Axon for task logic, typed scoring and user-level programs; and isolated replaceable workers for training/inference. This is a proposal, not a claim that a new runtime crate exists.

Reuse `AxonHost`, the interpreter, capability/effect catalog, principal services, current CLI/web approval machinery, audit/replay, R39 governance and restricted rewrite DSL where their gates substantiate the needed contract. Inspect existing R38/R40/R41 work before duplicating SDK/research/polyglot infrastructure. See [EXISTING_AXON_MAP](../EXISTING_AXON_MAP.md).

## Parallel builder lanes

A contracts owner serializes shared schema and public-type changes. An observer lane owns snapshots and projections. An executor/host lane owns grants, transactions and sandboxing. An evidence/eval lane owns protected checks and baselines. A model lane owns adapters and empirical calibration. Research branches consume stable interfaces and do not block the core path.

No two agents should concurrently rewrite the same compiler type graph or inference/codegen files without a coordinated work package. R2a-like changes require a dedicated branch and sequenced merges; observer/runtime work can continue against stable external contracts.

## Operating rules

Each slice has a red test or concrete counterexample, an exact scope, acceptance gates, non-goals and evidence output. Execute one bounded slice, verify it, then refresh the dependency graph. Do not batch unrelated optimizations into a safety change. A model's explanation never changes a gate result.

The builder may stop with a verified failure, missing prerequisite or inconclusive experiment. It must not “keep looping until confident” without budgets and a stop rule. Use the protocols in [BUILD_PROTOCOL](BUILD_PROTOCOL.md) and [LOOPS](LOOPS.md).

## Start order

Begin B00–B04: import/reconcile, reproduce existing evidence, freeze policy/tasks, create resettable fixtures, and define schema/event contracts. Then B05–B14 build the deterministic safe loop; B15–B16 add the existing-model comparison. Next execute the Reflex sequence: **B17 freeze the backend-neutral ABI and scoring provenance → B42 implement Reflex conformance/feature/effective-input fixtures → B21 export the eligible grouped decision corpus/label lineage → B18 build destructive controls and speculative/shared-state path → B19 calibrate/selectively route → B20 run the multi-backend bakeoff and decide adoption. B43 dependency adoption begins before external backends and remains required for protected use.** Only after those results should specialized Reflex training/architecture research become a priority.

No release date, headcount, training size or hardware order is implied. Estimate after the intake and first measured baseline, not from the number of conceptual milestones.

## v0.3 contract-hardening priorities

Before expanding model research, complete two cross-cutting build contracts:

- [REFLEX_CONFORMANCE](REFLEX_CONFORMANCE.md): batch-by-ID semantics, primitive-specific results, backend feature negotiation, effective-input/truncation receipts, trusted prompt-role construction, question-isolation/fusion tests, retry and score provenance.
- [DEPENDENCY_ADOPTION](DEPENDENCY_ADOPTION.md): pinned code/model/tokenizer/encoder/dataset/evaluator lineage, license/security review, SDK transformation accounting, benchmark comparability and protected deployment profiles.

Candidate search is now an explicit algorithmic subsystem. The build must measure candidate recall before decision accuracy, support a no-suitable-candidate outcome, and compare flat/retrieval/hierarchical/beam strategies where repository scale requires them. Known freshness/authorization predicates stay deterministic rather than becoming Reflex questions.

Learning exports distinguish observed action outcomes from counterfactual optimality and permit multiple acceptable actions. Off-policy evaluation from historical traces requires behavior-policy lineage and explicit assumptions; resettable Axon fixtures should run alternatives directly when practical.

## MiCode/Axon co-development track

MiCode is used as an **experience plane**, not as a source of authority or a required runtime service. Its planned canonical episodes, semantic observations, challenger experiments, failure attribution and repository knowledge candidates can feed Cortex through CX-16 once schemas are stable. Cortex can export decision/verifier/capability/ontology artifacts back to MiCode for experiments, but MiCode re-applies its own gate and host policy.

This creates a deliberate co-development loop:

```text
MiCode real coding tasks → canonical episodes → Cortex replay/learning/world-model research
Cortex policies/Reflex/verifier profiles → MiCode shadow/challenger experiments
verified patterns → CX-11/CX-18 admission → Axon skill/tool/library/compiler/runtime candidates
```

See [MICODE_EXPERIENCE_BRIDGE](MICODE_EXPERIENCE_BRIDGE.md), [KNOWLEDGE_INGESTION](KNOWLEDGE_INGESTION.md), and [SELF_OPTIMIZING_OS](SELF_OPTIMIZING_OS.md).

## Intent-first entry path (v0.5)

Cortex does not begin from an opaque `goal/task` string. The contract path is:

`Natural/system intent → IntentIR → semantic review/approval → AIR → Axon executable artifacts/actions → evidence graph`.

Basic Intent IR/schema/approval/lowering work is part of M1 because authority and completion semantics depend on it. Advanced system-generated `ImprovementIntent` experiments wait until CX-11 admission exists. Replanning may change tactics, observations and candidate implementations; it may not widen authority or weaken the approved evidence/success contract.

The new task sequence is B52 → B53 → B54 → B55 → B56 for the first human-intent vertical slice. B57 then demonstrates a system-generated optimization proposal through the same path after B30/B50 provide independent admission/self-optimization infrastructure.

## v0.6 Reflex runtime refinement

The milestone order is unchanged. M2 now explicitly builds a **Reflex Runtime** separate from any proprietary Reflex Model: immutable shared-state handles where supported, typed question dependencies, candidate-order manifests, branch scheduling, isolation conformance and performance accounting. Specialized listwise/causal/MoE model research remains optional and follows workload evidence. This is an implementation refinement, not a new Cortex milestone.

## v0.7 model-research refinement from Kev

The working Jev-style `kev` prototype changes the sequencing of model research without changing the Cortex architecture. The mechanism—shared state, isolated branches, listwise/pointer readout, no decoding—is now cheap enough to reproduce as an **early reference arm once the first eligible coding corpus exists**. The strategic bottleneck becomes transfer/generalization and software representation, not merely reconstructing the decision head.

M2 still ships without requiring a proprietary learned model. In parallel, a small Kev-style pilot may start as soon as B21 and the frozen research-suite machinery are ready. M5 promotion still requires CX-11 admission. Model research prioritizes cross-repository/task-family transfer, absence-control training, permutation robustness, canonical train/serve encoding and locked-test discipline.


## v0.8 Coding Frontier / Benchmark Lab

CX-21 adds a protected whole-system benchmark substrate. It starts in M1 with task contracts/reset/protected verification, then matures through M5 into repository/task-family transfer, longitudinal regression and Verified Coding Frontier reporting. CX-20 asks whether a Reflex model/runtime is better; CX-21 asks whether the entire Cortex coding system became better under matched authority, evidence and resource constraints. See [CODING_FRONTIER_LAB](CODING_FRONTIER_LAB.md).

B69–B70 establish the benchmark harness and initial portfolio. B71 adds contamination-aware transfer partitions. B72 implements frontier/regression reporting. B73 supplies immutable evidence to CX-11 admission.


## v0.9 Cognitive Specialization Compiler and Neural Programs

Cortex no longer assumes all recurring cognition should become one specialized Reflex model. M5 gains a specialization lane that searches over `Rule | Template | SpecializedReflex | NeuralProgram | Tool | GeneralReflex | Think` under the same protected evaluator. This is a refinement of CX-11/CX-18, not a new authority path.

The build sequence is:

`B74 recurring-function profiles → B75 specialized decision baselines → B76 applicability/ROI/drift evaluator`, while `B77 neural-program ABI/manifest → B78 reference learned-function arm → B79 typed runtime → B80 shared-base cache`. B81 compares learned representations, B82 exercises de-specialization/fallback, and B83 may feed one admitted artifact into the existing M9 staged crystallization path.

Neural Programs are intended for fuzzy transformations/extractions/normalization that do not fit bounded Reflex decisions. Their output is always data: typed validation and the existing capability/executor path remain authoritative. Remote compilation/inference is optional research infrastructure; the target runtime must be able to prove exactly which assets are required for local/offline execution.

See [COGNITIVE_SPECIALIZATION](COGNITIVE_SPECIALIZATION.md), [NEURAL_PROGRAMS](NEURAL_PROGRAMS.md), CX-22 and CX-23.


## v0.10 Semantic Perception / Retrieval and Learning Plane

CX-24 inserts a typed learned layer between raw observations and Reflex: schema-conditioned extractors and proposition-level semantic matchers preserve object/span provenance, explicit uncertainty and probability-source semantics. The first implementation is retrieval/perception support, not a new authority path. B84–B86 build and compare this layer.

A completion critic is added as B87–B88: it runs before authoritative task closure to catch likely premature stops and missing acceptance clauses, but CX-21/protected verification remains the only source of `VerifiedComplete`.

CX-25 adds a separated learner/sampler plane for later self-improvement. B89–B93 prove candidate-only publication, shadow serving, versioned transport, mixed-policy accounting and admission-only activation/rollback. Asynchronous RL/weight streaming is an optional optimization after authority separation and artifact identity are proven.

## v0.11 decision-composition and semantic-alignment refinement

The System-One/browser/structured-artifact research adds a new rule to the M2/M5 program: **prefer selection and composition over generation when the required information or parts already exist in the typed world**.

After M1 establishes Intent IR, observations, authority and verification, M2 now develops two complementary paths:

```text
bounded semantic judgment → Reflex
existing typed artifacts → SELECT / PROJECT / COMPOSE
```

Generation is used only for unresolved novel content. CX-26 owns typed artifact catalogs, source-bound projections, partial/fallback semantics and composition validation. B94–B97 implement the first vertical path and compare it against a generative control.

The Reflex Lab also treats semantic definitions as research artifacts. B98–B101 version questions/criteria/decompositions/candidate policies, run uncertainty-plus-random-audit labeling, compare monolithic versus decomposed judgments, and submit successful semantic/composition policies through normal CX-11 admission. A higher development score never self-promotes a semantic change.

This does not change the core dependency chain or authority model. It reduces unnecessary synthesis and creates another crystallization destination for recurring THINK/GENERATE behavior: a reusable composition policy or deterministic projection rule.

## v0.12 Semantic Supervisor Plane and Semantic Working-Set Manager

The latest Jev ecosystem work reveals two control-plane functions that are broader than a single Reflex call. CX-27 adds an independent semantic supervisor around active workers; CX-28 manages the semantic working set feeding workers, supervisors and model routes.

The M2 sequence after the existing decision-composition slice is:

`B102 supervisor ABI → B103 shadow supervision → B104 deterministic intervention policy → B105 feature-gated steering`, in parallel with `B107 working-set schemas → B108 rule/skill/map selection → B109 semantic context GC → B110 stale-result rejection`. B111–B113 then exercise cache-aware model routing, semantic-predicate query planning and explicit cognitive cascades. B106/B114 measure whole-system value; B115 submits only evidence-backed policies through CX-11 admission.

This revision does not make the supervisor an authority layer and does not make context reduction a summarization shortcut. Protected Intent/authority/acceptance/verifier artifacts stay pinned, and completion remains owned by protected verification.


## v0.13 self-application milestone

Start self-improvement during M1 by implementing B116–B122 before higher-impact automatic adaptation. The first release only instruments, replays and shadows internal policies. B123–B124 prove a bounded low-risk promotion/rollback lifecycle; deeper recursive experiments remain later and research-only.

## v0.14 Repository Improvement Plane and Cross-Project Learning

The reflexive optimization machinery is now generalized beyond MiCode/Axon internals. CX-30 introduces a project contract and repository adapter layer so arbitrary owner-approved repositories can become governed optimization environments without losing their local acceptance semantics. CX-31 then learns across those project-local episodes, but only through family-aware lineage, held-out repository transfer and scope-qualified promotion.

The sequence is:

`B128 project contracts → B129 repository adapters → B130 baselines/queues → B131 isolated challengers → B132 replay/shadow/canary → B133 heterogeneous-project pilot`.

Only after project-local evidence is reliable does cross-project learning begin:

`B134 family/lineage metadata → B135 pattern mining → B136 transfer tiers → B137 shared promotion → B138 de-generalization → B139 project-to-platform pilot`.

This creates three explicit optimization scopes:

```text
Level 1 — project-local optimizer
Level 2 — cross-project learner
Level 3 — platform self-optimizer (CX-29)
```

Evidence may move upward only after new transfer/admission checks. A shared pattern still re-enters each target repository through its own `ProjectImprovementContract`; shared knowledge never becomes implicit authority.


## v0.15 sequencing

B140–B143 (evidence graph) should start before higher-risk recursive optimization because later causal/scheduler evidence depends on shared provenance. B144–B147 add causal/active experimentation after matched replay exists. B148–B151 introduce the registry and deterministic reference scheduler before any learned scheduler policy is eligible for self-application. These slices do not move or weaken existing protected-kernel gates.
