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
