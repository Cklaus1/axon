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
