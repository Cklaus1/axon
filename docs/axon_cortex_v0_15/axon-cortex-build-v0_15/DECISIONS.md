# Proposed architecture decisions

All entries are proposals for owner review, not completed repository governance actions.

| ID | Decision | Why | Revisit trigger |
|---|---|---|---|
| D01 | Name the architecture Axon Cortex; name its decision engine Axon Reflex. | Separates the complete system from one model and from a third-party brand. | Naming/brand review. |
| D02 | AIR means Axon Intelligence Representation in this package. | Distinguishes the execution graph from the runtime that evaluates it. | Existing repository usage conflicts; preserve a compatibility alias rather than silently rename stored schemas. |
| D03 | Use package-local CX-* IDs. | We have not inspected the live R* registry. | Map to actual R* IDs during import. |
| D04 | Begin interpreter-orchestrated and hosted. | Uses the documented reference semantics without inheriting nonexistent native guarantees. | Engine parity and sandbox enforcement demonstrated for a new target. |
| D05 | Rust trusted adapter/executor, Axon goal and scoring programs, external replaceable model workers. | Avoids making early research depend on a language/parser rewrite. | Measured need for compiler-native constructs. |
| D06 | First world is a resettable software workspace, starting with Axon examples and later held-out repositories. | Real observable effects, inexpensive reset, testable outcomes. | Transfer gate justifies another world adapter. |
| D07 | Agent output is an untrusted proposal. | Schema validity, identity, authority, and correctness differ. | Not weakened by a learned component. |
| D08 | Required gates fail closed. | Missing checks cannot establish safety. | Only an explicit non-consequential development profile may tolerate marked skips. |
| D09 | Snapshot-scoped grants and transactional local writes; external effects require specific semantics. | Prevents stale actions and blind retries. | New action type brings a reviewed recovery protocol. |
| D10 | Learning is offline by default; production model/gate artifacts are immutable within an episode. | Reduces feedback instability and makes comparisons interpretable. | Controlled online-learning research approval. |
| D11 | One common admission mechanism, not one mutable mechanism controlled by every learner. | Avoids self-grading and hidden authority expansion. | TCB governance action only. |
| D12 | No automatic THINK→REFLEX→RULE requirement. | Specialization must be empirically useful or formally justified within its scope. | Applicability evidence changes. |
| D13 | Model research starts with a small labeled pilot, not an arbitrary million-record threshold. | Early learning curves are cheaper than unsupported scaling assumptions. | Pilot saturation or performance bottleneck. |
| D14 | No promise of general intelligence, universal equivalence, or automatic causal identification. | Those conclusions need stronger evidence than component implementation. | Publish evidence and assumptions for a specific supported claim. |
| D15 | Axon owns a backend-neutral Reflex ABI; TypeSafe/Jev compatibility is an edge adapter, not the internal architecture. | Lets generative, direct-logit, sequence-scoring and learned decision-head backends compete on one workload and verifier. | Evidence that one backend requires a genuinely incompatible semantic contract. |
| D16 | Dynamic runtime candidate sets are the primary Reflex abstraction, not fixed classifier labels. | Coding/world actions change with current reality; capability compilation needs option-conditioned scoring. | A domain proves a static label surface is sufficient and simpler. |
| D17 | DECIDE, GENERATE and ACT remain distinct trust/semantics stages. | Prevents bounded judgment, open-ended synthesis and authority from collapsing into one model call. | TCB review only; cannot be weakened by model performance. |
| D18 | Probability/uncertainty provenance is typed and preserved. | Native logits, sequence likelihood, generated estimates, entropy and empirical calibration answer different questions. | A new score source adds a new explicit provenance class. |
| D19 | Specialized Reflex architecture is optional and follows a multi-backend bakeoff plus destructive controls. | Avoids prematurely cloning an undisclosed vendor architecture before workload evidence exists. | Bakeoff exposes a quality/serving bottleneck not solved by simpler backends. |

## Owner choices before consequential execution

The owner must approve the first host target, allowed repository/workspace, data classification, resource caps, gate-required policy, evaluation margins, and independent admission signer. An unset consequential policy field is a refusal, not a permissive default. Proposed research can use offline synthetic data without claiming release readiness.

No decision here revokes Axon's separately documented bare-metal ambition. It only prevents that ambition from blocking the first Cortex learning loop.

## v0.3 — Reflex interoperability boundaries

- Preserve Choice, binary-probability/Noul-compatible, and ordinal distribution/Score-compatible semantics at adapters; do not flatten all primitives into `selected + confidence`.
- Backend capability limits are negotiated through manifests, not copied into AIR as vendor constants.
- Record submitted and effective model input; silent truncation is disallowed.
- Only trusted adapter code creates privileged prompt roles.
- Candidate absence and candidate authorization are distinct from ranking.
- Historical success labels realized actions, not unobserved optimality; multiple acceptable actions are first-class.
- Learned-head participation in M2 is optional; the conformance/bakeoff apparatus is mandatory.
- External repos/models/datasets require adoption manifests before protected use.

## v0.4 — MiCode experience plane and OS-wide crystallization

- **D20 — MiCode is an optional experience/research plane, not an Axon runtime dependency.** Cortex accepts versioned artifacts from MiCode but remains able to execute its own first repair loop without MiCode availability.
- **D21 — Cross-system evidence never transfers authority.** MiCode grants/risk ceilings and Axon principals/capabilities are distinct namespaces; each side re-applies local authority policy.
- **D22 — Self, external and generated experience share one evidence/admission discipline.** External repositories or MiCode successes do not receive weaker truth standards than Axon's own traces.
- **D23 — Repository ingestion extracts provenance-rich knowledge candidates, not unqualified training truth.** Observation frequency/popularity cannot promote an abstraction without reproduction/counterexample/held-out evidence appropriate to the claim.
- **D24 — Crystallization uses a staged ladder.** Episode → pattern → concept → Reflex/skill/tool → library → compiler/runtime is optional progression with stronger evidence nearer the trusted substrate.
- **D25 — Repeated action sequences may propose capabilities but cannot self-classify authority.** Trusted capability/effect analysis owns the executable scope; native/compiler promotion requires CX-15 and invariant evidence.

## v0.5 — intent-first architecture

- **D26 — Natural language never directly grants executable authority.** Human/system prose first lowers to typed Intent IR; AIR/`.ax` execution requires an approved contract plus normal grants.
- **D27 — Intent IR owns success semantics.** Objective, hard constraints, preferences, authority requests, budgets and required evidence remain distinct; replanning may change implementation but not silently weaken the contract.
- **D28 — Semantic review is a first-class approval surface.** Humans may inspect raw AST/AIR, but approval binds the exact typed Intent IR and deterministic semantic rendering/diff rather than requiring source-language fluency.
- **D29 — Self-improvement is expressed as ImprovementIntent.** Cortex/world-model/knowledge discoveries propose typed improvements through the same authority/admission path; no subsystem gets implicit permission to rewrite itself or its verifier.
- **D30 — Intent is bidirectional and traceable.** Executed actions/evidence map back to intent clauses, and final reports explain outcomes against the approved contract.

## v0.6 — Reflex runtime architecture refinements

- **D31 — Reflex Model and Reflex Runtime are separate abstractions.** The runtime owns immutable/equivalent state handles, dependency-aware branch scheduling, candidate manifests, cache/accounting/cancellation and response mapping; model backends own distributions/scores.
- **D32 — Shared state is a first-class optimization contract, not a universal backend guarantee.** Backends may emulate it; only measured/pinned backends may claim actual prefix/KV/state reuse.
- **D33 — Candidate set construction and ordering are part of inference semantics and calibration scope.** Ordering is deterministic/recorded or explicitly randomized; calibration is invalidated/revalidated when the policy changes.
- **D34 — Decision distributions are primary; confidence summaries are derived.** Empirical correctness probability requires calibration evidence, not a presentation scalar.
- **D35 — AIR questions declare `Independent`, `ConditionallyRelevant`, or `AnswerDependent` scheduling semantics.** The compiler may batch/speculate only when those semantics are preserved.
- **D36 — Listwise candidate interaction is a prioritized research hypothesis, not an assumed Jev implementation.** Causal-decoder and sparse-MoE choices remain optional later experiments.
- **D37 — Use Outcome-Calibrated Decision Training as the generic program name.** Do not claim RLCD reproduction without a pinned reproducible public method.

- **D28 — Kev-style architecture becomes the first learned Reflex reference arm, not the final architecture.** A small pretrained backbone + adapter + isolated branch mask + pointer/listwise readout is cheap enough to train early once eligible coding data exists. Continued investment depends on transfer evidence.
- **D29 — Transfer outranks familiar-split parity.** Reflex promotion claims about coding generality require repository/task-family held-out evidence; random/example-level held-out accuracy is insufficient.
- **D30 — One canonical decision encoding owns train/eval/serve/replay semantics.** Backend-specific tokenization is downstream and observable; silent renderer divergence invalidates calibration.
- **D31 — Candidate absence is representable.** Registered typed control candidates (`NONE`, `OBSERVE_MORE`, `ESCALATE`, `BLOCKED` as policy permits) prevent normalized distributions from forcing an ordinary action when the candidate compiler is incomplete.
- **D32 — Permutation robustness is a learned property, not an excuse to discard order provenance.** Candidate order remains recorded and calibration-bound even when training objectives reduce sensitivity.

## v0.9 — cognitive specialization and neural programs

- **D40 — Specialize per cognitive function, not globally.** Cortex chooses among rule/template, specialized Reflex, neural program, explicit tool, general Reflex and THINK based on matched evidence and applicability rather than one universal System-One model.
- **D41 — Learned fuzzy functions are a distinct artifact class.** A recurring transformation/extraction/repair task may compile into a typed neural program instead of being forced into a classifier or deterministic tool.
- **D42 — Shared-base learned artifacts require immutable dependency closure.** Base model, adapter/program, tokenizer, renderer/template, runtime manifest, data/evaluation suite and applicability guard are version-bound; aliases never define execution identity.
- **D43 — Missing specialization never silently becomes base-model execution.** The runtime follows the registered semantic fallback or refuses, preserving omission/failure distinctions.
- **D44 — Neural outputs are data, never authority.** Generated strings or structured values still pass ordinary Axon parsing, semantic-object resolution, capability checks and verification.
- **D45 — Local/offline execution is a target property, not an assumption.** An artifact is offline-ready only when every required base/runtime/program asset is present and validated; remote compile/inference remains optional research infrastructure.


## v0.10 decisions

- **D-v0.10-1 — Semantic perception is distinct from Reflex.** Schema-conditioned extraction and proposition matching may enrich/rank observations, but cannot authorize effects.
- **D-v0.10-2 — Probability source is semantic.** Generated probability text, selected-token logits, decision-head outputs and empirically calibrated values are not interchangeable.
- **D-v0.10-3 — DONE remains verifier-owned.** A completion critic may request continued work but cannot create completion evidence or weaken the approved contract.
- **D-v0.10-4 — Learners publish candidates, not active weights.** Continuous/asynchronous training is allowed only behind candidate registry, shadow evaluation and CX-11 admission.
- **D-v0.10-5 — Weight transport is an optimization.** Direct streaming/NCCL is optional; immutable artifact identity and conversion conformance are normative.

## v0.11 decisions

### D26 — Prefer select/project/compose before generation
When the world already contains the requested value or reusable typed pieces, Cortex should select/project/copy or compose them under CX-26 instead of asking a model to regenerate equivalent content. Generation remains the explicit fallback for unresolved novel content.

### D27 — Candidate catalogs are authority-owned
Artifact/capability catalogs are compiled by the runtime from observed state, provenance, policy and authority. A model may choose among catalog entries but cannot mint executable entries in the same trust step.

### D28 — Semantic decision definitions are versioned research artifacts
Question wording, criteria, decomposition, candidate policy, order policy and state projection are independently versioned and evaluated. Improvements in these artifacts are not reported as model-weight improvements and require independent admission before activation.

### D29 — Completion remains verifier-owned
Selection/composition may reduce generation and a completion critic may detect likely unfinished work, but neither a `finish` choice nor a syntactically complete composition creates VerifiedComplete.

## D-v0.12 — Treat semantic supervision and working-set management as first-class control planes

**Decision:** add an independent Semantic Supervisor Plane and Semantic Working-Set Manager rather than embedding those concerns inside the main generative worker. Supervisor models emit assessments only; deterministic policy and existing capability/verifier systems own interventions. Context selection operates over authorized versioned artifacts with hard-pinned protected state and explicit recompute contracts.

**Reason:** Foreman/pi-warden-style supervision shows that progress/stuck/drift/verification questions are narrower than software generation, while fast-compaction/jev-rules/skill-selection patterns show that long-horizon quality depends on maintaining the right active working set. Both functions create useful Axon learning data without requiring a monolithic agent model.

**Consequence:** AIR/replay gains supervisor and working-set receipts; CX-24 semantic predicates participate in query planning; CX-22 may later specialize supervisor/context/cascade policies, but no learned component gains authority by specialization.


## D-v0.13 — self-improvement starts with self-observation, not self-modification

**Decision:** treat every non-kernel cognitive subsystem as a versioned optimization target now. Collect outcome-linked records, support replay and live shadow challengers, and only later permit bounded automatic promotion for explicitly allowlisted low-risk policy classes. Protected authority, admission, locked evaluation, verifier semantics, provenance and rollback are not ordinary self-improvement targets.

## v0.14 — repository-wide self-improvement

- Adopt a three-level optimization scope: project-local optimizer (CX-30), cross-project learner (CX-31), and platform self-optimizer (CX-29).
- Require an immutable `ProjectImprovementContract` before any repository becomes an optimization target.
- Treat project-specific acceptance semantics as authoritative for that project; cross-project evidence cannot override them.
- Promote shared patterns only to the scope supported by held-out repository-family transfer evidence.
- Preserve negative transfer and support de-generalization/scope narrowing rather than forcing one universal abstraction.
- Reuse MiCode as the primary repository execution/episode plane while keeping Axon responsible for generic learning, transfer and admission contracts.


## v0.15 — evidence, causality and cognitive scheduling

- Adopt one typed Universal Evidence Graph as the provenance index across intent, cognition, action, verification, learning and admission rather than allowing each subsystem to invent incompatible evidence semantics.
- Require causal labels and controlled intervention discipline before self-improvement reports component changes as causal improvements.
- Make active information-gain experiments first-class, but bind experiment authority to reversibility/risk before execution.
- Introduce a Shared Capability Registry and OS-like Cognitive Scheduler; hard authority/type/privacy/evidence constraints always precede utility/cost routing.
- Treat confidence as one input to decision-theoretic routing, never as a substitute for consequence/reversibility/verification requirements.
