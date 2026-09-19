# Terms and semantic boundaries

| Term | Contract in this package |
|---|---|
| Cortex | Overall proposed cognitive execution and controlled-learning architecture. |
| Reflex | Bounded typed judgment engine; not a general text generator or authorization authority. |
| AIR | Proposed Axon Intelligence Representation: typed, versioned execution graph. |
| Observation | An adapter's evidence about the environment, with scope, time and provenance. |
| Snapshot | Immutable identified local workspace/environment manifest; not a claim of complete knowledge. |
| Belief | Inferred uncertain state, distinct from direct observations. |
| World model | A versioned predictor of defined outcomes given state and a fully specified candidate action. |
| Simulator | Executor of hypothetical transitions; may be exact for a restricted model or approximate/learned. |
| Counterfactual | A hypothetical alternative, with its observational or causal assumptions recorded. |
| Capability/grant | Authority held in a trusted registry, attenuated and checked by the executor. |
| Candidate action | Untrusted proposal referring to an available operation and target; never authority by itself. |
| Artifact | Content-addressed patch, model, rule, proof, plan or evaluation result. |
| Calibration artifact | Empirical mapping/evidence tied to model, schema, candidate construction, domain and evaluation split. |
| Confidence | Avoid unqualified use; specify raw category probability, calibrated event probability, interval, or a heuristic. |
| Proof | Mechanically checked derivation of a stated property under explicit assumptions. Tests are not proofs of arbitrary behavior. |
| Verifier | Checks a task's locked completion contract; has separately controlled code/data/credentials. |
| Admission authority | Governs artifact promotion; learner may propose but not rewrite its policy or evidence. |
| Crystallization | A measured, scoped specialization into a cheaper reusable artifact, with fallback. |
| Deoptimization | Withdrawal from a specialization when applicability, calibration or environment no longer holds. |
| Unknown | Explicit lack of evidence, label, attribution, probability, or outcome—not a silent default. |
| Research success | Specific measured improvement on held-out tasks, not a claim that general intelligence has been reached. |

All examples of types and APIs are contracts/pseudocode unless explicitly labeled as an existing command documented in S1.

## v0.4 additions

**Experience plane** — An external system, such as MiCode, that produces structured episodes/experiments for Cortex. It is not part of Axon's authority boundary merely because its data is imported.

**Knowledge candidate** — A provenance-rich proposed pattern/concept extracted from self, external or generated experience. It may be useful evidence without being PromotionEligible.

**Crystallization ladder** — Optional staged progression from episodes/patterns toward reusable Reflex/skills/tools/libraries/compiler/runtime artifacts, with increasing assurance nearer the trusted substrate.

**External experience** — Eligible evidence from MiCode, repositories, histories, benchmarks or research artifacts. External does not mean untrusted by definition, but it never bypasses local evidence/data-use/admission checks.

## Intent vocabulary (v0.5)

**Intent** — a human- or system-authored statement of desired outcome; untrusted until resolved into a typed contract.

**Intent IR / IntentIR** — versioned typed representation of objectives, hard constraints, soft preferences, target scope, requested/prohibited authority, budgets, required evidence, assumptions, ambiguities and provenance.

**ImprovementIntent** — system-generated Intent IR proposing a change to Axon/Cortex; carries no implicit authority and uses the normal admission path.

**Semantic renderer** — deterministic human-readable projection of an exact Intent IR version. It is a review surface, not a second source of truth.

**Intent clause lineage** — mapping from AIR nodes/actions/evidence receipts back to the objective/constraint/authority/evidence clause that justifies them.

## v0.6 terms

**StateHandle** — immutable or emulated Reflex-runtime reference to an exact effective state encoding, bound to state/model/tokenizer/adapter/preprocessing/tenant/expiry metadata. It conveys no execution authority.

**Question dependency** — AIR scheduling relation: `Independent`, `ConditionallyRelevant`, or `AnswerDependent`.

**Listwise decision model** — a bounded-decision model that jointly represents the current candidate set before producing a distribution, rather than scoring every option independently.

**Candidate-order policy** — deterministic or explicitly randomized rule that orders a runtime candidate set. It is part of the inference/calibration domain when the backend is order sensitive.

- **Coding Transfer Frontier** — the furthest registered distribution shift (same repo, unseen repo, unseen family/task family, cross-language where meaningful) at which a Reflex artifact meets its preregistered selective quality, coverage and compute envelope.
- **Canonical Decision Encoding** — the single versioned semantic rendering/packing contract used across Reflex training, evaluation, serving and replay before backend-specific tokenization.

## v0.9 additions

**Cognitive Specialization Compiler (CSC)** — Cortex subsystem that detects recurring typed cognition, compares cheaper representations under protected evaluation, and proposes a guarded specialization without owning activation.

**Specialized Reflex** — learned bounded-decision artifact optimized for a stable decision family/domain; distinct from the general Reflex backend and from a deterministic rule.

**Neural Program** — immutable learned function artifact implementing a typed fuzzy transformation/extraction/normalization contract over a pinned runtime/base model or standalone model. Its output is untrusted data until Axon type/refinement/capability validation succeeds.

**Skill Compiler** — research/build pipeline that derives a Neural Program candidate from an approved semantic contract plus eligible examples/evidence. It does not itself grant deployment authority.

**Shared-base skill runtime** — execution substrate in which multiple small learned program/adapter artifacts reuse a pinned base model while maintaining exact artifact identity, isolation and resource accounting.

**De-specialization** — disabling/narrowing a specialized artifact after drift, applicability mismatch or regression and routing future work to a previous-good/general executor.


**Semantic Perception** — Schema-conditioned learned extraction/classification that turns approved raw observations into typed facts/spans/relations with provenance.

**Semantic Match** — Proposition-conditioned scoring of an observed object/chunk; useful for semantic grep/reranking and deterministic boolean composition.

**Completion Critic** — Cheap advisory model/policy invoked when an agent proposes stopping; may identify unresolved clauses but never owns `VerifiedComplete`.

**Learning Plane** — Isolated training/candidate-publication subsystem separated from the active serving/sampler plane and independent admission authority.

**Probability source** — The mechanism that produced a probability-like value, such as generated estimate, selected-token logits, decision head, or empirical calibration transform.

## v0.11 additions

**Artifact Catalog** — Runtime-generated immutable set of typed world artifacts eligible for a particular selection/composition decision, with provenance, freshness and authority metadata.

**SELECT / PROJECT / COPY** — Bounded path that chooses an existing typed artifact and returns an authoritative value or deterministic projection of it rather than regenerating equivalent content.

**Decision Composition Runtime (DCR)** — CX-26 subsystem that selects, orders and composes typed artifacts into validated plans/specs/pipelines before using GENERATE for unresolved novel content.

**Semantic Definition Revision** — Immutable version of a decision's state projection, question, criteria, decomposition, composition rule and candidate policy, evaluated separately from model weights.

**Semantic Alignment Loop** — Reflex Lab loop that samples uncertain/error plus random-audit examples, labels them, proposes semantic-definition changes, and evaluates those changes without automatic promotion.

## v0.12 terms

**Semantic Supervisor Plane** — independent semantic assessment loop around a worker/planner that emits typed progress/drift/stuck/verification/completion-related judgments; it does not own tool authority or VerifiedComplete.

**Semantic Working Set** — the subset of durable rules, skills, maps, observations, history and evidence currently loaded into a cognitive operation, with protected pinning and an effective-context receipt.

**Semantic GC** — relevance/recomputability-driven removal or structural retention of active context without deleting durable provenance.

**Recompute contract** — explicit declaration that omitted context can be recreated from bound inputs/snapshots under stated authority, side-effect and cost assumptions.

**Cognitive cascade** — observable ordered attempt to solve an operation with progressively more expensive/general strategies, recording each fallback/abstention reason.


**Reflexive Self-Application Plane** — CX-29 infrastructure that makes Axon/MiCode internal cognitive components observable, replayable, shadowable, replaceable and eligible for guarded specialization/admission.

**CognitiveOperationRecord** — versioned record of an internal cognitive operation, its effective input, component revision, output, cost, downstream outcome and verifier evidence.

**ImprovementIntent** — immutable proposal to improve one cognitive component; names incumbent, observed problem, hypothesis, protected invariants, required suites and rollback target.

**Protected kernel** — authority/admission/evaluation/provenance/rollback roots that cannot be weakened by the ordinary candidate self-improvement path.

## v0.14 additions

**ProjectImprovementContract** — immutable, owner-approved contract that makes one repository/project a governed optimization environment by defining identity, authority, protected areas, evidence, budgets, allowed improvement classes and rollback semantics.

**Repository Improvement Plane** — CX-30 project-local loop that baselines a repository, proposes challengers, evaluates them in isolation and promotes only under project-specific acceptance evidence.

**Cross-Project Learning Plane** — CX-31 layer that mines governed project episodes for reusable patterns, validates transfer across repository families and submits scope-qualified shared capabilities.

**Project family** — cluster of repositories that share meaningful lineage, template, ecosystem or architecture; used to prevent correlated repositories from masquerading as independent transfer evidence.

**De-generalization** — narrowing, splitting or demoting a shared artifact when later evidence shows its applicability is weaker than previously believed.


## v0.15 additions

**Universal Evidence Graph** — typed immutable provenance graph connecting intent, observations, cognitive operations, actions, world changes, claims, verifier evidence, admission and rollback.

**Claim strength** — explicit evidence class such as observed, derived, statistical, counterfactual, verified or proof-backed; downstream systems may not silently strengthen it.

**Active experiment** — bounded observation/intervention chosen partly for expected information gain under explicit cost, risk and reversibility constraints.

**Reversibility class** — pre-execution classification such as read-only, reversible, compensatable, irreversible or unknown, used by experiment/scheduler authority policy.

**Shared Capability Registry** — admitted versioned catalog of rules, semantic matchers, Reflexes, Neural Programs, tools, generators and other typed cognitive capabilities with applicability/evidence/runtime metadata.

**Cognitive Scheduler** — runtime dispatcher that selects among authorized compatible capabilities using hard constraints plus a versioned utility policy over expected quality, latency, cost, risk, privacy, hardware and cache state.
