# Axon Cortex — complete review, specifications and build plan

Consolidated reader • 2026-09-18 • v0.7 • All specifications are Draft proposals. No Axon product/model tests were executed. For code-agent editing, use the individual files in the companion package.

## Contents

- [README.md](#doc-readme-md)
- [REVIEW.md](#doc-review-md)
- [DECISIONS.md](#doc-decisions-md)
- [EXISTING_AXON_MAP.md](#doc-existing-axon-map-md)
- [build/BUILD_PLAN.md](#doc-build-build-plan-md)
- [build/BUILD_PROTOCOL.md](#doc-build-build-protocol-md)
- [build/LOOPS.md](#doc-build-loops-md)
- [build/SELF_OPTIMIZING_OS.md](#doc-build-self-optimizing-os-md)
- [build/INTENT_COMPILER.md](#doc-build-intent-compiler-md)
- [build/MICODE_EXPERIENCE_BRIDGE.md](#doc-build-micode-experience-bridge-md)
- [build/KNOWLEDGE_INGESTION.md](#doc-build-knowledge-ingestion-md)
- [build/TASKS.md](#doc-build-tasks-md)
- [build/REFLEX_BACKEND_BAKEOFF.md](#doc-build-reflex-backend-bakeoff-md)
- [build/REFLEX_KEV_PILOT.md](#doc-build-reflex-kev-pilot-md)
- [build/REFLEX_DECISION_CORPUS.md](#doc-build-reflex-decision-corpus-md)
- [build/REFLEX_CALIBRATION_LAB.md](#doc-build-reflex-calibration-lab-md)
- [build/REFLEX_CONFORMANCE.md](#doc-build-reflex-conformance-md)
- [build/REFLEX_RUNTIME.md](#doc-build-reflex-runtime-md)
- [build/DEPENDENCY_ADOPTION.md](#doc-build-dependency-adoption-md)
- [build/ACCEPTANCE_GATES.md](#doc-build-acceptance-gates-md)
- [build/EXPERIMENT_REGISTER.md](#doc-build-experiment-register-md)
- [build/BOOTSTRAP_PROMPT.md](#doc-build-bootstrap-prompt-md)
- [specs/INDEX.md](#doc-specs-index-md)
- [specs/CX-00-system-contract.md](#doc-specs-cx-00-system-contract-md)
- [specs/CX-01-evaluation.md](#doc-specs-cx-01-evaluation-md)
- [specs/CX-02-world-observation.md](#doc-specs-cx-02-world-observation-md)
- [specs/CX-03-capabilities-executor.md](#doc-specs-cx-03-capabilities-executor-md)
- [specs/CX-04-air-runtime.md](#doc-specs-cx-04-air-runtime-md)
- [specs/CX-05-reflex-inference.md](#doc-specs-cx-05-reflex-inference-md)
- [specs/CX-06-routing-calibration.md](#doc-specs-cx-06-routing-calibration-md)
- [specs/CX-07-predictive-world.md](#doc-specs-cx-07-predictive-world-md)
- [specs/CX-08-planning-experiments.md](#doc-specs-cx-08-planning-experiments-md)
- [specs/CX-09-representation-abstraction.md](#doc-specs-cx-09-representation-abstraction-md)
- [specs/CX-10-replay-learning-data.md](#doc-specs-cx-10-replay-learning-data-md)
- [specs/CX-11-crystallization-admission.md](#doc-specs-cx-11-crystallization-admission-md)
- [specs/CX-12-learning-meta.md](#doc-specs-cx-12-learning-meta-md)
- [specs/CX-13-os-runtime.md](#doc-specs-cx-13-os-runtime-md)
- [specs/CX-14-parallel-model-research.md](#doc-specs-cx-14-parallel-model-research-md)
- [specs/CX-15-language-compiler-proof.md](#doc-specs-cx-15-language-compiler-proof-md)
- [specs/CX-16-micode-experience-bridge.md](#doc-specs-cx-16-micode-experience-bridge-md)
- [specs/CX-17-external-repository-knowledge.md](#doc-specs-cx-17-external-repository-knowledge-md)
- [specs/CX-18-capability-synthesis-native-promotion.md](#doc-specs-cx-18-capability-synthesis-native-promotion-md)
- [specs/CX-19-intent-compiler.md](#doc-specs-cx-19-intent-compiler-md)
- [schemas/PROTOCOLS.md](#doc-schemas-protocols-md)
- [examples/SAFE_REPAIR_EPISODE.md](#doc-examples-safe-repair-episode-md)
- [templates/WORK_PACKAGE.md](#doc-templates-work-package-md)
- [templates/SPEC_TEMPLATE.md](#doc-templates-spec-template-md)
- [GLOSSARY.md](#doc-glossary-md)
- [SOURCES.md](#doc-sources-md)
- [build/STATUS.md](#doc-build-status-md)
- [CHANGELOG_v0_3.md](#doc-changelog-v0-3-md)
- [CHANGELOG_v0_4.md](#doc-changelog-v0-4-md)
- [CHANGELOG_v0_5.md](#doc-changelog-v0-5-md)
- [CHANGELOG_v0_6.md](#doc-changelog-v0-6-md)
- [CHANGELOG_v0_7.md](#doc-changelog-v0-7-md)

---

<a id="doc-readme-md"></a>

## README.md

# Axon Cortex — specification and build package

**Prepared:** 2026-09-18. **Revision:** v0.7 Kev/transfer-first Reflex refinement. **Status:** Draft architecture proposals. **Product implementation/tests:** not performed in this package.

Axon Cortex is the proposed cognitive control plane for a **self-optimizing, intent-first Axon language/compiler/runtime/OS**: natural/system intent → typed Intent IR → observed state → bounded typed decisions → authorized execution → independent verification → eligible experience → controlled specialization/crystallization. It learns from Axon's own executions, MiCode's coding-world episodes, approved external repositories/histories, and generated curricula under one evidence and admission discipline.

**Axon Reflex** is the bounded typed-decision engine. **AIR** is defined here as **Axon Intelligence Representation**, the typed execution graph. These names distinguish the full architecture from a model, and distinguish Axon's work from the external Jev reference. This is a working-name decision, not trademark clearance.

## Start here

For a single document, use [AXON_CORTEX_MASTER.md](AXON_CORTEX_MASTER.md). The modular files remain authoritative for editing.

Read [REVIEW.md](REVIEW.md) for the substantive gaps/corrections, then [BUILD_PLAN.md](build/BUILD_PLAN.md) for milestone sequencing. Use [BOOTSTRAP_PROMPT.md](build/BOOTSTRAP_PROMPT.md) to start a coding agent on repository intake, not on a giant implementation sprint.

| Document | Purpose |
|---|---|
| [Review](REVIEW.md) | Nineteen issues and concrete changes to the original roadmap, including the System-One backend bakeoff revision. |
| [Architecture decisions](DECISIONS.md) | Proposed forks, tradeoffs and owner choices. |
| [Existing Axon map](EXISTING_AXON_MAP.md) | Reuse/verify/extend matrix and unresolved source discrepancies. |
| [Specification index](specs/INDEX.md) | 20 detailed Draft specs with dependencies, contracts and negative acceptance cases. |
| [Build plan](build/BUILD_PLAN.md) | Milestones M0–M9, intent-first entry path, parallel tracks and the first vertical slice. |
| [Build tasks](build/TASKS.md) | 69 dependency-tracked work packages; optional research/native/kernel branches are explicit. |
| [Builder protocol](build/BUILD_PROTOCOL.md) | Intake, implementation loops, integration, evidence and definition of done. |
| [Loop protocol](build/LOOPS.md) | Runtime inner/outer loops, learning/meta loops and separate builder loops. |
| [Acceptance gates](build/ACCEPTANCE_GATES.md) | 122 proposed product gates; none is presented as already executed. |
| [Experiment register](build/EXPERIMENT_REGISTER.md) | Baselines, ablations and research stop/pivot criteria. |
| [Reflex conformance](build/REFLEX_CONFORMANCE.md) | Primitive/batch semantics, backend feature negotiation, effective-input receipts, prompt-role isolation and adapter fixtures. |
| [Reflex runtime](build/REFLEX_RUNTIME.md) | Shared-state handles, dependency-aware branch scheduling, candidate-order manifests, cache/accounting and isolation semantics. |
| [Dependency adoption](build/DEPENDENCY_ADOPTION.md) | External repo/model/dataset intake, transitive pinning, licenses/security and benchmark comparability. |
| [Reflex backend bakeoff](build/REFLEX_BACKEND_BAKEOFF.md) | Backend-neutral ABI; generative/logit/sequence/learned-head comparison; shared-state measurements and destructive controls. |
| [Kev-style Reflex pilot](build/REFLEX_KEV_PILOT.md) | Early small learned coding-decision reference arm; transfer-first training/evaluation and stop/pivot rules. |
| [Reflex decision corpus](build/REFLEX_DECISION_CORPUS.md) | Dynamic candidate examples, grouped splits, label lineage, error attribution and question decomposition. |
| [Reflex calibration lab](build/REFLEX_CALIBRATION_LAB.md) | Probability provenance, calibration artifacts, selective risk/coverage and VUC. |
| [MiCode experience bridge](build/MICODE_EXPERIENCE_BRIDGE.md) | Cross-system artifact contracts, authority separation and semantic replay. |
| [Knowledge ingestion](build/KNOWLEDGE_INGESTION.md) | Repository/history evidence pipeline and counterexample/reproduction discipline. |
| [Self-optimizing OS loop](build/SELF_OPTIMIZING_OS.md) | Experience sources, crystallization ladder, coupled learning loops and timescales. |
| [Intent compiler](build/INTENT_COMPILER.md) | Natural/system intent → typed Intent IR → semantic approval → constrained AIR → evidence. |
| [Protocol schemas](schemas/PROTOCOLS.md) | Versioned data contracts and action/admission state machines. |
| [Worked repair episode](examples/SAFE_REPAIR_EPISODE.md) | Concrete safe coding-world scenario and negative variants. |
| [Sources and limits](SOURCES.md) | Attachment page references, narrow external checks and claim boundaries. |
| [Status](build/STATUS.md) | What was prepared versus what still requires repository evidence. |
| [Glossary](GLOSSARY.md) | Meaning of observation, belief, world model, calibration, proof and authority. |

## Major changes from the previous roadmap

The first milestone now reconciles actual Axon support rather than inheriting blanket safety claims. A working hosted repair loop comes before custom neural architecture. Capability IDs receive real authority/freshness/payload/recovery semantics. Evaluation/data/admission move early, and missing required gates fail closed. Statistical prediction never substitutes for proof or realized outcomes. Meta-learning proposes changes but cannot redefine its own exam.

The OS goal remains staged: H0 confinement in the first runtime, H1 hosted-service operation, H2 portability/optional attestation, then a separately approved bare-metal branch. Language syntax/native lowering follow measured runtime usage and type-map/parity evidence.

## Usage and import

Copy this directory into a staging location such as `docs/cortex-proposal/` in the actual repository. Inspect existing templates, R-ID allocation, invariants, exit codes and R39 governance before import. Do not overwrite those systems. All `CX-*` IDs are package-local. All API/type names and code locations not explicitly described as existing in SOURCES are proposals/pseudocode.

The four JSON manifests make specs, tasks, gates and source provenance easy to ingest. `tools/validate_package.py` is an actual utility supplied here; it validates this package's references and DAGs only. It does not implement Cortex or run Axon tests.

```bash
python3 tools/validate_package.py
```

See `package_validation.json` for the latest actual document-only validation report. A planned product gate is not a runnable command until implemented in the repository.

## First deliverable to build

A resettable local Axon example; compact partial observations; session/snapshot-bound grants; one bounded generated patch; isolated registered checks; independent completion verification; and replay without real side effects. Produce a patch/evidence artifact for review, not an automatic merge or deployment.

No proprietary training recipe, vendor speedup, arbitrary confidence threshold, universal program equivalence, or general-intelligence result is assumed.

## v0.3 focus

This revision folds the second System-One/Jev ecosystem review into executable build contracts. The architecture is unchanged; the experimental boundary is tighter. A backend cannot silently reinterpret primitive semantics, match batch results by position, hide truncation/preprocessing, turn repository text into privileged prompt roles, conceal retries/normalization, or claim calibration outside the input contract it actually evaluated. Candidate recall/no-suitable-option handling and learning-label lineage are now explicit.

## v0.5 focus

This revision reconnects Cortex to Axon's original intent-first product thesis. Natural language or system-generated optimization proposals now lower to a typed `IntentIR` before AIR or `.ax`; the intent contract owns objective, hard constraints, preferences, authority requests, budgets, ambiguity and required evidence. Human approval is bound to the exact Intent IR version, and self-improvement is expressed as `ImprovementIntent` through the same authority/admission path rather than unrestricted mutation.

## v0.6 focus

This revision kept the Cortex architecture and milestone ordering intact while sharpening the Reflex implementation. Reflex now has an explicit runtime/model split; shared-state reuse is a measurable backend capability; AIR questions declare independent/conditional/answer-dependent scheduling; candidate order is part of inference/calibration semantics; distributions are primary and confidence summaries derived; and listwise candidate interaction becomes a first-class research arm rather than an assumption. No claim is made that external behavioral probing reveals Jev's exact hidden architecture.

## v0.7 Kev update

The working `jaredpalmer/kev` reconstruction makes the Jev-style model mechanism an executable reference rather than only a hypothesis. v0.7 therefore adds an early small Axon Reflex model pilot once eligible coding data exists, while shifting the main research emphasis to **cross-repository/task transfer, candidate-absence handling, permutation robustness, canonical decision encoding and frozen locked-test research suites**. The broader Intent IR → AIR → Cortex → self-optimizing OS architecture is unchanged.

---

<a id="doc-review-md"></a>

## REVIEW.md

# Architecture review: from an ambitious roadmap to a buildable program

## Verdict

Keep the central direction: observed world state → bounded actions → typed decisions → capability-gated effects → independent evidence → controlled learning. Replace the long feature staircase with a small executable vertical slice, a proof-of-value program, and separately gated research tracks.

The original roadmap mixes four kinds of work: safety infrastructure, cognitive execution, empirical learning, and open-ended research. They need different acceptance standards. A passing type checker cannot establish calibration; a calibrated choice cannot establish permission; a prediction cannot establish a real-world outcome; a test corpus cannot prove arbitrary program equivalence.

This review is grounded in the attached documents, not a source-code audit. The changes below are proposals. Evidence references resolve in [SOURCES.md](SOURCES.md).

## 1. Existing Axon guarantees were overstated

S1 p.23 calls the simulation world a parametric one-dimensional linear model and describes several abstractions as userland types. S1 pp.83–84 describes prototype compression work and future work for whole-program candidate complexity. That is valuable prior art, not a general predictive software model.

More urgently, p.16 explicitly says the ambient effect ceiling is interpreter-only and ignored by a native binary. Page 23 describes absent deployment gate functions as open. Page 12 narrows the FFI trust boundary, while p.90 still uses stronger invariant wording. Page 86 labels cross-VM features as implementing with remaining integration gaps. These are not reasons to discard Axon; they are reasons to establish an engine-by-engine evidence map before composing new guarantees.

**Change:** M0 records commit-pinned evidence for every relied-on property. Cortex's consequential path fails closed on a missing required gate, unsupported engine, or unavailable enforcement. Initially support interpreter orchestration plus an externally isolated tool runner; no inferred native parity. See CX-00, CX-13, CX-15.

## 2. Too many new primitives and premature language work

The original list promotes “learn,” “discover,” “abstract,” and other research concepts into prospective language primitives before their contracts are stable. That risks introducing syntax whose semantics are just a provider call.

**Change:** Start AIR as a versioned typed execution representation with a small set of operations. Keep representation search and discovery as library-level workflows. Add language syntax only after repeated, measured usage demonstrates the need. Do not make R2a a blocker for an isolated runtime experiment, but do make authoritative type-map integration a gate before new cross-engine compiler semantics. See CX-04 and CX-15.

## 3. A list of objects is not an authorization system

A model selecting a valid symbol can still choose the wrong symbol. A valid edit can introduce a malicious build script. A registered test may run arbitrary subprocesses or dependencies. An opaque ID is not intrinsically unforgeable, and a model can repeat another session's ID.

**Change:** Bind every grant to a runtime principal, session, snapshot, action kind, resource scope, expiry, budget, and server-side record. Validate generated payloads independently. Treat builds/tests as untrusted executable workloads. Include symlinks, redirects, untracked files, dependencies, and executable arguments in the threat model. Never pass model text to a shell. See CX-03/CX-13.

## 4. The action-space compiler can prune away the solution

The prior design focuses on preventing impossible actions, but can make a task unsolvable by exposing only a small, wrong neighborhood. Observation compression can omit precisely the clue that matters. Starting from a broken repo may prevent a full AST from being available.

**Change:** Preserve partial observations and explicit unknowns; include permission-bounded INSPECT, SEARCH, and EXPAND_SCOPE. Measure candidate recall separately from decision accuracy. Hierarchical action retrieval handles large repositories; retrieval must not grant new authority. New files and new tools are proposals accepted through a manifest, not impossible forever merely because they did not exist at observation time. See CX-02/CX-03.

## 5. The execution protocol is missing crash and race semantics

“Check freshness, then act” leaves a race between checking and writing. Git HEAD alone misses dirty files, toolchain changes, generated data, and other agents. Retrying after a crash can duplicate an external effect.

**Change:** Use immutable workspace snapshots, per-action read/write sets, compare-and-swap or locked commit, content hashes, idempotency keys, and durable action states. An interrupted non-idempotent operation becomes OutcomeUnknown until reconciliation, not automatically retryable. Rollback restores code/model artifacts; it does not undo an email, network request, or other external effect. See CX-03/CX-10/CX-13.

## 6. Parallel fields are not a joint probability model

S2 pp.21–22 motivates speculative branch-specific questions. W2 describes independent evaluation, meaning execution isolation. It does not establish independent errors. Shared state/model/training can produce correlated mistakes. A parallel batch can also be slower and more expensive for high cardinality or multi-token candidates.

**Change:** Define a tagged-union Action and field-dependency graph. Speculate only effect-free decision outputs under explicit resource budgets; never speculate execution. Ask conditional target questions with the operation fixed in the question. Validate the selected branch and cross-field constraints. Model/inference network calls still consume authorized effects and cost. Benchmark sequential, parallel, and shared-prefix modes rather than requiring a single forward pass. See CX-05.

## 7. A probability is not a permission, a confidence certificate, or a proof

The illustrative 0.995 threshold was arbitrary. Confidence can denote category probability, uncertainty about the category distribution, or empirical reliability; those are different quantities. Valid JSON and normalized softmax cannot settle semantic correctness. W3 supports treating calibration as a measured property rather than a formatting property.

**Change:** Separate raw scores, supplied probabilities, calibration artifact, observed validity domain, abstention, and task-risk policy. An adapter without real probabilities reports them as unavailable. Apply hard authority/risk gates first, then a workload-specific empirical routing policy. Out-of-distribution cases and expired calibration cannot gain permission from a high score. See CX-05/CX-06.

## 8. Missing baselines make expensive architecture easy to rationalize

The previous roadmap would build many components before showing that the extra components help. More capabilities do not necessarily improve end-to-end performance. S1 pp.10–11 itself reports a small experiment in which a stateless control beat a stateful system on the studied metric.

**Change:** Preserve rules-only, one strong-model agent, structured AIR without Reflex, Reflex without a learned world model, and the candidate system as ablations. Match tools, budgets, observation access, primers, and task splits. Charge retrieval, preprocessing, failed calls, speculative branches, training amortization, and verification. Measure success per cost and latency, not model latency alone. See CX-01.

## 9. “No statistically significant loss” is the wrong promotion criterion

A tiny test can fail to detect even a substantial regression. Zero observed failures is not zero failure probability. Reusing the same holdout in an indefinite optimization loop turns it into training feedback.

**Change:** Pre-register non-inferiority margins, confidence intervals, minimum sample sizes/precision, subgroup treatment, cost targets, and test families. Reserve an independently held final audit set, rotate exhausted sets, limit submissions, and record every attempted candidate. Report inconclusive when evidence is insufficient. Safety gates remain separate from quality tradeoffs. See CX-01/CX-11.

## 10. Learned world models need a precise target and limits

A graph of symbols is observed state, not learned transition dynamics. A repository digest is not a Markov state. Predicting the effect of EDIT_SYMBOL before the actual patch exists leaves the action underspecified. A learned simulator can be wrong exactly where an optimizer finds its apparent best action; W4 studies related model-bias issues in model-based learning.

**Change:** Distinguish observations, beliefs, transition predictions, simulators, and real outcomes. Start with short-horizon measurable targets such as test outcomes, diagnostic families, and affected files. Condition edit predictions on the full candidate patch digest and environment. Allow uncertainty/abstention and forbid simulations from certifying completion. Compare useful test/planning decisions against no-model controls. See CX-07.

## 11. A conditional predictor is not automatically causal

Learning P(next | state, action) does not by itself identify intervention effects when actions were selectively chosen. Counterfactual claims need stronger assumptions than forecasting.

**Change:** Explicitly label observational, intervention-backed, and assumed structural claims. Start causal work with resettable sandbox interventions and matched controls; record which variables were held fixed. Information-gain scores use the current hypothesis model and can themselves be wrong. Evaluate downstream diagnosis, not just calculated entropy reductions. See CX-08.

## 12. MDL should not mean “simplest answer that fits yesterday”

A zero-residual constraint makes sense for some deterministic examples, not all noisy observations. Small training descriptions can overfit, erase rare cases, or omit causal state. The source already identifies the current complexity measure as a prototype requiring further calibration (S1 pp.83–84).

**Change:** Use preregistered fit tolerances appropriate to the domain, locked holdouts, and explicit residual/exception cost. Keep AST complexity labeled as a proxy, not a universal measure of understanding. Require improved transfer or downstream decisions for a new abstraction. See CX-07/CX-09.

## 13. Learning needs data eligibility, not automatic ingestion of every trace

The source warns that host journals can contain file contents, HTTP bodies, input, and environment values (S1 p.15). A detailed replay log is useful operational evidence and potentially dangerous training material. Teacher answers and agent self-assessments are not ground truth. Outcomes may arrive late or never.

**Change:** Separate access-controlled raw journals, redacted review traces, eligible learning records, and sealed evaluations. Record retention, provenance, consent/authority, label quality, delayed/censored outcomes, and contamination lineage. Permit audit evidence without permission to train on it. See CX-10.

## 14. Error attribution is uncertain and can be multi-causal

The proposed failure enum is useful, but demanding a single correct diagnosis before learning can freeze progress or produce fabricated explanations.

**Change:** Store a distribution or set of candidate causes with evidence and Unknown as a legitimate result. Confirm attribution through replay, component substitution, and controlled experiments. Update one component at a time by default; coordinated changes need interaction tests. See CX-10/CX-12.

## 15. The meta-loop must not grade its own exam

The preceding answer allowed changing evaluation suites and promotion thresholds while also prohibiting weakening the gate. Without a trust/approval split, those statements conflict. A learner could make every candidate pass by changing the metric or holdout.

**Change:** Separate learning workers from an independently controlled admission authority. Workers may propose new curricula, metrics, gates, proof tactics, or compiler changes. The authority versions and approves policy; incumbents and challengers are rerun under both old and new policies before a switch. Learning a prover strategy is permitted; rewriting the trusted proof checker is a TCB change. See CX-11/CX-12/CX-15.

## 16. Crystallization is optional specialization, not a law of intelligence

Not every useful reasoning task has a stable deterministic rule. An empirical student is not behaviorally equivalent to its teacher. Repeated task success may be highly context-dependent. Waiting for “millions of decisions” is also an arbitrary blocker for useful small-model experiments.

**Change:** Support several paths: prompt optimization, calibrated routing, retrieval, distilled adapters, verified tools, guarded rules, and restricted compiler rewrites. Every specialization records an applicability predicate and fallback/deoptimization path. Train small prototypes once a useful labeled pilot exists; expand only when learning curves justify it. See CX-11/CX-14.

## 17. OS scope must be staged, not forgotten or allowed to dominate

S1 contains both the early userland-only direction and an explicit bare-metal reversal. The later roadmap notes unresolved confinement and kernel/VM integration (pp.62–63, 86). We cannot silently declare the OS done or cancel the user's kernel goal.

**Change:** Deliver a hosted Cortex runtime first; then confined worker execution and operational controls; then portability; finally a separately approved bare-metal research branch. For Cortex, hard process authority, cancellation, recovery, and resource bounds precede desktop polish or kernel replacement. See CX-13.

## 18. Research milestones need evidence-based names

“Fluid intelligence starts here” or “fully self-improving” is not a measurable completion gate. A system can invent plausible abstraction names without improving any task.

**Change:** Require held-out novel problem families, few-shot learning curves, representation ablations, transfer tests, and bounded-compute comparisons. Report demonstrated capabilities and limits, not a universal intelligence claim. See CX-09/CX-12.

## What remains unchanged

The learning hierarchy, distinct decision/generation roles, dynamic observed targets, interpreter-first semantics, independent verification, and lowering expensive cognition into reusable machinery remain central. The package changes sequencing, precision, and evidence—not the long-term aim.

## Recommended first deliverable

A sandboxed repair of one small, intentionally broken Axon example: observe a partial program; choose an inspect or edit target; generate one bounded patch; run approved checks/parity when supported; produce an independently validated patch artifact; replay without live effects. No auto-merge, production deployment, kernel modification, or model self-training is required to demonstrate this first loop.

## 19. The System-One ecosystem changes the experimental order

The user-supplied Sept-2026 ecosystem map adds a useful implementation lesson: the Jev/System-One **programming shape** can be separated from the underlying model. Official adapter patterns, OpenJev-style logit/sequence scorers, MLX/shared-prefix implementations, option-conditioned learned heads, browser agents and calibration projects suggest a better research sequence than “build a custom Reflex model later.”

**Change:** freeze one Axon-owned backend-neutral Reflex ABI, then compare generative constrained output, direct option logits, sequence probability and learned option-conditioned heads on the same dynamic-candidate corpus. Add destructive state/candidate controls, grouped splits, shared-prefix/prefill measurements and typed probability provenance. Calibration and routing use protected real outcomes, not merely normalized model scores. Custom architecture work becomes a response to measured quality/serving bottlenecks rather than a roadmap assumption. See the three `build/REFLEX_*` documents and updated CX-05/CX-06.

This does not validate launch-week repo benchmarks or reproduce TypeSafe's private training stack. The ecosystem is used to improve experiment design, not as proof of performance.

## v0.4 review — MiCode changes the data/experimentation architecture, not the core execution path

The MiCode support plan does **not** justify replacing Cortex's M0–M7 core. It adds a valuable external experience plane and makes the self-optimizing-OS objective more explicit. Three additions matter: a strict MiCode↔Axon artifact bridge (CX-16), provenance-rich repository/history knowledge ingestion (CX-17), and a staged capability/native crystallization path (CX-18).

The key boundary is authority. MiCode can produce rich episodes and run challenger experiments, but its local grants, risk tiers and host properties cannot become Axon authority. Likewise, Axon can export policies/verifier profiles to MiCode without bypassing MiCode's gate. This keeps co-development scientifically useful without creating a distributed TCB by accident.

The build sequence therefore changes modestly: freeze the bridge schema early; ingest one episode once available; defer external-knowledge claims until representation/abstraction and data-governance machinery exist; demonstrate Pattern→Skill/Tool before optional compiler/runtime promotion. Approximately the existing M0–M7 execution/research program remains intact.

## v0.5 finding — the intent layer was under-specified

The original Axon roadmap had the correct product direction—structured prose compiled to typed Axon artifacts and explicitly reviewed—but the Cortex package mostly began from an already supplied `goal/task`. That skips a load-bearing boundary: natural language mixes objectives, constraints, preferences, authority requests and success evidence, and those cannot safely become implementation instructions in one opaque model translation. CX-19 inserts a typed Intent IR and semantic approval layer. This is not a rewrite of M0–M9; it makes the contract above AIR explicit and makes self-optimization auditable as `ImprovementIntent` rather than unrestricted mutation.

## v0.6 review conclusion — Reflex implementation, not Cortex redesign

The new Jev behavioral analysis does not overturn the Cortex architecture. It improves the implementation hypothesis for Reflex: shared state becomes a first-class measurable runtime contract; candidate order/composition become formal inference context; question dependency scheduling becomes explicit; listwise candidate interaction is prioritized in research; and distributions/proper scoring are treated as primary. Exact Jev backbone/training reconstruction remains unsupported and is excluded from the critical path.

## v0.7 review — Kev changes the bottleneck, not Cortex

Kev demonstrates that a Jev-style shared-state/isolated-question/pointer-readout model can be implemented and trained cheaply with a small pretrained backbone. The important negative result is transfer: familiar-source quality can approach the hosted reference while out-of-source quality remains far lower. Therefore the build should spend less effort speculating about an exotic undisclosed architecture and more effort on coding-world representation, verified MiCode/Axon experience, repository-level split discipline, transfer evaluation, OOD routing and curriculum.

The package is updated accordingly: a Kev-style model is an early reference arm, not a promoted default; locked tests and transfer partitions become explicit; permutation/absence augmentation are first-class experiments; and one canonical decision encoding owns train/eval/serve/replay semantics.

---

<a id="doc-decisions-md"></a>

## DECISIONS.md

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

---

<a id="doc-existing-axon-map-md"></a>

## EXISTING_AXON_MAP.md

# Reuse, verify, extend: map to existing Axon

Basis: S1 in [SOURCES.md](SOURCES.md). Status is **documented, not rerun** throughout. Repository paths are taken from the attachment and must be confirmed before editing.

| Existing surface | Source | Reuse | Required check / new work |
|---|---|---|---|
| `interp.rs`, LLVM codegen, parity scripts | pp.2–4, 89 | Keep reference semantics and sound refusal. | Produce feature × engine × enforcement matrix; no native ambient-ceiling assumption. |
| `infer.rs`, checker, R2a type map | p.4 | Use authoritative inferred types. | Thread stable IDs/type information before new cross-engine semantics; no additional ad hoc type heuristic. |
| `capabilities.rs`, effects, contained annotations | pp.5–6, 19–21 | Existing static rules and effect vocabulary. | Cortex dynamic handles need runtime authority, snapshot binding, and per-argument validation. |
| Principal/kernel registry | pp.44–47 | Attenuated grants and budget authority. | Audit identity is not authorization; check each executor use, handle ownership, resource budget semantics. |
| Sandbox APIs | pp.16, 48 | Reference for scoped interpreter effects. | Empty scoped list is not deny-all; untrusted native tools need process isolation outside interpreter checks. |
| AxonHost, host journals and replay | pp.15, 94 | Reuse seam, divergence semantics, recording mechanism. | Record model boundaries, cancellation and ordering; isolate secret-bearing journals from training export. |
| R44 accumulating sessions | pp.6–7, 14 | Interactive typed execution and explicit cell boundaries. | Closures/channels/native handles cannot be assumed persistent across cells; Cortex stores stable artifact refs, not live heap values. |
| Goal search, Plan, Schedule, Feedback | pp.23, 40–41 | Reuse bounded optimization and value-level types. | Real task planner needs explicit state, budgets, completion evidence, and scheduling semantics. |
| World/Counterfactual and MDL | pp.23, 83–84 | Existing small examples and complexity measurement. | Distinguish a numerical demo from a learned software model; add train/eval splits and bounded prediction targets. |
| Distributions and belief helpers | pp.23, 28, 34–35 | Numerical distributions and runtime refinements. | Do not reuse moment checks as a calibration certificate or causal proof. |
| Self-improving rewrite DSL | pp.22, 83–84 | Restricted rewrite candidates and multi-gate admission. | Corpus equality is empirical; formal proof applies only where its scope is explicit. |
| CLI/web approval and risk gate | pp.14, 22–23 | Typed artifact review and explicit human approval. | Bind approval to final content digest; missing required gate becomes rejection in Cortex profile. |
| `axon-os`, audit ledger, compliance monitor | pp.84–85 | Hosted supervisor, kill, resources and evidence. | Confinement profile tests, durable job recovery, privileged parent isolation, cancellation propagation. |
| R26–R34 attestation/quorum | pp.84–86 | Integrate only portions confirmed by required evidence. | Hardware-backed vs simulated/HMAC reports must be distinguishable; cross-VM quorum is not an initial dependency. |
| R36 kernel/platform proposals | pp.86–88 | Preserve proposal and reuse exact unresolved decisions. | Separate confinement and syscall-gate proof obligations before claiming a full OS. |
| R38 embedded runtime proposal | p.86 | Avoid re-deriving SDK packaging. | Check overlap with Cortex adapters and runtime before creating a new product. |
| R39 typed governance/evidence graph | pp.87–88 | Store spec dependencies and rerunnable evidence in existing graph. | Import CX specs after namespace reconciliation; do not build a second governance registry. |
| R40 research compiler proposal | p.88 | Prior art for evidence/experiment/knowledge graphs. | Scope Cortex to runtime and learning experiments, not a replacement project-management platform. |
| R41 polyglot candidate | p.7 | Potential future adapter seam. | Do not make polyglot semantics mandatory for v0. |

## Reconciliation work product

For each dependency, record: path; function or interface; repository commit; source spec; target/engine; gate command; environment; stdout/stderr artifact; actual return status; PASS/FAIL/SKIPPED/UNAVAILABLE; and the precise claim established. A missing repository or a skipped gate is not PASS.

For conflicting documentation, add a discrepancy record with both passages, observed implementation, proposed resolution, owner, and linked invariant change when necessary. Do not edit the old document to claim resolution before running evidence.

## v0.5 intent-layer follow-up

The supplied Axon architecture already documents `axon intent compile`, `axon ast review`, `axon ast approve`, structured prose and the principle that the typed AST is the executable/audit artifact. CX-19 does not assume their current implementation exactly matches the new Intent IR contract. B52 must inspect the live repository first, distinguish what is current versus aspirational, and decide which existing parser/approval artifacts can be reused or migrated. The proposal strengthens the earlier prose→AST path into prose/system intent→typed desired-outcome contract→AIR→`.ax`/actions→evidence.

---

<a id="doc-build-build-plan-md"></a>

## build/BUILD_PLAN.md

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

See [REFLEX_BACKEND_BAKEOFF](build/REFLEX_BACKEND_BAKEOFF.md), [REFLEX_DECISION_CORPUS](build/REFLEX_DECISION_CORPUS.md), and [REFLEX_CALIBRATION_LAB](build/REFLEX_CALIBRATION_LAB.md).

## First executable vertical slice

Use a resettable copy of a tiny Axon example containing a localized semantic bug. Visible reproduction checks and hidden completion tests are pinned outside the patchable region. Permit only inspect/search, one approved symbol-body patch, registered isolated checks and a completion claim.

Sequence: snapshot → partial semantic observation → legal action catalog → select action → generate a patch artifact → revalidate grant/payload/snapshot → apply in copy → run checks → independently verify patch and output → save evidence → replay. Record prediction attempts only once M3 exists; do not block this first loop on fabricated forecasts.

Start with deterministic fixture decisions for conformance, then the same loop with an existing model. A scripted golden test proves the plumbing; varied model-driven tasks establish solver behavior. Keep these results separate.

## Proposed implementation organization

Logical modules: contracts; observer; grants; executor; AIR scheduler; model adapters; evidence/replay; evaluation; registry/admission; and hosted supervisor integration. Start in a small number of modules/crates appropriate to existing Axon seams. Do not create sixteen crates because there are sixteen specs.

Prefer Rust for authority-sensitive host adapters and registry integration; Axon for task logic, typed scoring and user-level programs; and isolated replaceable workers for training/inference. This is a proposal, not a claim that a new runtime crate exists.

Reuse `AxonHost`, the interpreter, capability/effect catalog, principal services, current CLI/web approval machinery, audit/replay, R39 governance and restricted rewrite DSL where their gates substantiate the needed contract. Inspect existing R38/R40/R41 work before duplicating SDK/research/polyglot infrastructure. See [EXISTING_AXON_MAP](EXISTING_AXON_MAP.md).

## Parallel builder lanes

A contracts owner serializes shared schema and public-type changes. An observer lane owns snapshots and projections. An executor/host lane owns grants, transactions and sandboxing. An evidence/eval lane owns protected checks and baselines. A model lane owns adapters and empirical calibration. Research branches consume stable interfaces and do not block the core path.

No two agents should concurrently rewrite the same compiler type graph or inference/codegen files without a coordinated work package. R2a-like changes require a dedicated branch and sequenced merges; observer/runtime work can continue against stable external contracts.

## Operating rules

Each slice has a red test or concrete counterexample, an exact scope, acceptance gates, non-goals and evidence output. Execute one bounded slice, verify it, then refresh the dependency graph. Do not batch unrelated optimizations into a safety change. A model's explanation never changes a gate result.

The builder may stop with a verified failure, missing prerequisite or inconclusive experiment. It must not “keep looping until confident” without budgets and a stop rule. Use the protocols in [BUILD_PROTOCOL](build/BUILD_PROTOCOL.md) and [LOOPS](build/LOOPS.md).

## Start order

Begin B00–B04: import/reconcile, reproduce existing evidence, freeze policy/tasks, create resettable fixtures, and define schema/event contracts. Then B05–B14 build the deterministic safe loop; B15–B16 add the existing-model comparison. Next execute the Reflex sequence: **B17 freeze the backend-neutral ABI and scoring provenance → B42 implement Reflex conformance/feature/effective-input fixtures → B21 export the eligible grouped decision corpus/label lineage → B18 build destructive controls and speculative/shared-state path → B19 calibrate/selectively route → B20 run the multi-backend bakeoff and decide adoption. B43 dependency adoption begins before external backends and remains required for protected use.** Only after those results should specialized Reflex training/architecture research become a priority.

No release date, headcount, training size or hardware order is implied. Estimate after the intake and first measured baseline, not from the number of conceptual milestones.

## v0.3 contract-hardening priorities

Before expanding model research, complete two cross-cutting build contracts:

- [REFLEX_CONFORMANCE](build/REFLEX_CONFORMANCE.md): batch-by-ID semantics, primitive-specific results, backend feature negotiation, effective-input/truncation receipts, trusted prompt-role construction, question-isolation/fusion tests, retry and score provenance.
- [DEPENDENCY_ADOPTION](build/DEPENDENCY_ADOPTION.md): pinned code/model/tokenizer/encoder/dataset/evaluator lineage, license/security review, SDK transformation accounting, benchmark comparability and protected deployment profiles.

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

See [MICODE_EXPERIENCE_BRIDGE](build/MICODE_EXPERIENCE_BRIDGE.md), [KNOWLEDGE_INGESTION](build/KNOWLEDGE_INGESTION.md), and [SELF_OPTIMIZING_OS](build/SELF_OPTIMIZING_OS.md).

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

---

<a id="doc-build-build-protocol-md"></a>

## build/BUILD_PROTOCOL.md

# Builder protocol: specification to evidenced implementation

## Authority and status

Every package specification starts Draft. Import does not make it Reviewed or Approved. First inspect Axon's existing spec template, `spec-meta` requirements, R-ID registry, invariant ledger, exit ledger and R39 evidence graph. Map CX identifiers into that system deliberately; preserve the source package IDs as aliases in migration notes.

Do not overwrite the repository's build/governance protocol with this file. This is a proposed Cortex-specific supplement, subject to its existing gates.

## Repository intake

Record repository root, branch/commit, dirty files, environment/toolchain and local changes. Do not reset, clean or overwrite user work. Read prior specs/tasks/prototypes before designing replacements. Reproduce source-referenced gates only after checking what they execute and ensuring a safe environment.

The Axon attachment documents commands including `cargo build -p axon-core --no-default-features --bin axon`, `axon check`, `axon test` and parity scripts; those are prior-art leads, not commands executed by this package. The real repo may have changed. A failing or unavailable command is evidence to record, not something to hide with `|| true`.

Create an intake map with exact implemented interfaces, engine support, required dependencies and PASS/FAIL/SKIPPED/UNAVAILABLE evidence. Resolve document contradictions through code and tests; propose an invariant change when needed.

## Work-package contract

A work package identifies spec requirements, upstream slices, permitted paths, protected files, expected interfaces, tests/fixtures, acceptance gates, migration/rollback plan, resource budget and stop rules. Before code, state the decisive fork and why the alternative was rejected. Limit the package to one reviewable behavior change or tightly coupled contract migration.

Mark proposed paths as proposed until confirmed. Never declare an unimplemented acceptance script passing merely because its filename is in a spec.

## Inner build loop

1. Reproduce the defect or define a negative fixture that would fail without the feature.
2. Inspect the precise code/data seam and tests; gather only the needed context.
3. Implement the smallest change inside the allowed scope.
4. Run formatting/type/unit checks relevant to the seam, then the negative/positive contract tests.
5. If behavior spans interpreter/native, add parity or explicit-refusal cases. For host effects, test actual isolation rather than mocks alone.
6. Inspect the diff and evidence for accidental scope, budget, authority, ABI or documentation changes.
7. Record a result artifact tied to commit/diff/environment. Retry within budget or stop with a localized blocker.

A failing acceptance criterion is not repaired by weakening its assertion. A wrong test can be changed only through explicit review with a reproducible justification and impact on prior results.

## Outer build loop

At slice completion, the integrator runs seam/integration gates from a clean controlled workspace and compares the simple incumbent. Merge only after shared contract compatibility, adversarial cases, generated reference/docs, and rollback checks pass. Update source specs, requirement status and evidence together.

At milestone completion, demonstrate the end-to-end workflow with the actual interfaces—not a collection of mocks. A mock conformance gate and a real enforcement gate are separate records. Re-run the baseline when environment/model/toolchain changed.

## Meta build loop

After several slices or an unexpected regression, review whether the bottleneck is observation, action availability, data, inference, orchestration, tests, or poor task framing. Run a bounded ablation, not a global rewrite. Reprioritize future work based on evidence; do not let a meta agent rewrite admission policy or mark its own research successful.

Architecture changes require an ADR/spec amendment, dependency impact, invariant/TCB analysis, migration and fresh gates. “A better design” is a proposal until it survives the same independent process.

## Reflex backend and corpus protocol

When implementing B17–B20, treat the Axon Reflex ABI as the controlled variable and the backend as the experimental variable. Do not change the question schema, candidate-construction policy, hidden verifier, or authority envelope between backend comparisons unless the experiment is explicitly about that change.

Required sequence:

1. Freeze the backend-neutral question/response schema and probability-provenance enum.
2. Add conformance fixtures for dynamic candidate sets, multi-token options, abstention, invalid scores, stale state and branch conditions.
3. Add at least one adapter from each feasible family: generative constrained output, direct option logits, sequence scoring, and learned/option-conditioned head. Missing families are reported as unavailable, not silently substituted.
4. Build the grouped decision corpus from eligible episodes; keep teacher labels distinct from independently verified outcomes.
5. Run destructive controls: shuffled/empty/wrong/stale state, candidate reorder/rename, irrelevant context, adversarial candidates and candidate-omission/scope-expansion.
6. Record state-prefill, incremental question/option time, memory/cache mode, total speculative work, raw scores and calibrated values with exact model/tokenizer/runtime revisions.
7. Fit calibration only on permitted calibration splits and evaluate on grouped held-out task families.
8. Compare selective task utility, coverage, quality, latency and cost against the locked simple baseline before adopting a backend or router.

Do not infer calibration from normalized logits or entropy. Do not fabricate probabilities for label-only adapters. Do not compare backend A on one candidate catalog to backend B on a different catalog and call the result an inference benchmark.

Question decomposition is itself an experiment. When splitting a broad judgment into narrower subquestions, register the deterministic composition rule, compare against the original formulation, and preserve both versions in the corpus. A better decomposition may become a reusable Reflex template; it does not automatically become a safety gate.

See [REFLEX_BACKEND_BAKEOFF](build/REFLEX_BACKEND_BAKEOFF.md), [REFLEX_DECISION_CORPUS](build/REFLEX_DECISION_CORPUS.md), and [REFLEX_CALIBRATION_LAB](build/REFLEX_CALIBRATION_LAB.md).

## Resource and stop policy

Before running a work package, set wall time, maximum model/tool calls, retries, compute/storage and maximum candidate submissions. Suggested development defaults are finite and local: at most two automatic retries of a failed action, three consecutive no-progress iterations, and a 30-step task ceiling. These are proposed starting limits, not measured optimal settings or production guarantees.

Stop immediately on attempted authority expansion, suspected secret exposure, unbounded subprocesses, unknown non-idempotent outcomes, policy/signer mismatch or corruption of protected evaluation artifacts. Ordinary implementation failures can be retried inside budget; safety uncertainty is not a reason to disable the control.

## Definition of done for a slice

The behavior is implemented and documented; positive/negative/adversarial cases pass; engine/host support is explicit; evidence names exact commands and artifacts; no protected gate was weakened; source reference and generated docs are updated where required; rollback is tested; remaining limitations are recorded. Owner review occurs under the actual repository workflow.

Required gates report five states. Only PASS satisfies a required gate. SKIPPED, UNAVAILABLE and INCONCLUSIVE prevent a completion claim for that guarantee. Product results cannot be inferred from the package validator.

## Builder handoff

Leave a concise report: implemented change, files/digests, commands/results, observed limitations, failed hypotheses, next unblocked slice and stop reason. Keep secrets and private model reasoning out of handoff text. Link auditable artifacts rather than copying enormous logs into the next prompt.

## External backend and model intake (v0.3)

Before an external model/SDK participates in protected evaluation, complete [DEPENDENCY_ADOPTION](build/DEPENDENCY_ADOPTION.md) and [REFLEX_CONFORMANCE](build/REFLEX_CONFORMANCE.md). Do not begin by copying benchmark claims into acceptance criteria. Pin effective dependencies, reproduce a minimal example, run Cortex conformance fixtures, then register matched experiments.

A question-design review precedes every new Reflex family. Mechanically decidable freshness, authorization, arithmetic, authenticated-check and structural invariants stay deterministic. For semantic questions, record scope, candidates, evidence required, abstention/absence behavior and exactly how the answer is consumed.

Candidate recall is evaluated before decision quality. If the correct/suitable action is missing from the catalog, the failure belongs to observation/candidate construction rather than the Reflex backend.

## External experience / MiCode protocol (v0.4)

For slices using MiCode or external repositories, record the source-system/repository revisions, schema/migration versions, data-use/license policy, omissions, semantic-object mapping quality, evaluator version and whether the evidence is observed, teacher-labeled, simulated or inferred. Reproduce consequential knowledge claims locally before promotion. Never let a successful external episode substitute for an Axon capability grant, verifier result or invariant check.

## Intent-first build discipline (v0.5)

For user/system goals that originate as natural or structured prose, freeze the typed Intent IR and its approval/evidence requirements before executing consequential work. Builder changes that alter Intent IR semantics, semantic rendering, ambiguity handling or lowering require explicit fixtures showing old/new behavior; they cannot be justified solely by the downstream model producing nicer code. A self-improvement builder may propose `ImprovementIntent` but cannot edit the gate/evaluator that admits its own candidate.

---

<a id="doc-build-loops-md"></a>

## build/LOOPS.md

# Loops: running Cortex, learning, and building Cortex

The previous roadmap blurred three activities: executing a task, improving a component, and changing the architecture. This protocol separates their state, budgets, evidence and write permissions. “Inner/outer/meta” describes containment and responsibility, not a requirement for infinite recursion.

## A. Runtime loop hierarchy

| Loop | Inputs | Work | Writes | Stop condition / gate |
|---|---|---|---|---|
| R1 — Inner action loop | Pinned observation, action catalog, remaining budget | Select or request context; generate concrete payload if needed; predict when supported; revalidate; execute; observe; check | Owned ephemeral workspace, action/evidence events | One action reaches Verified/Refused/Failed/Canceled/OutcomeUnknown; no hidden retries |
| R2 — Outer task loop | Goal contract, subgoals, working beliefs, R1 evidence | Plan, choose next action/experiment, evaluate progress, replan | Task working state and proposed plan | Independent goal completion; authority/evidence/budget limit; cancellation; bounded no-progress |
| R3 — Component learning loop | Eligible labeled episodes, candidate failure attribution, fixed baseline | Propose one update; train/derive; run held-out and adversarial eval | Candidate artifact and unsigned proposal; no release authority | Candidate rejected/inconclusive or submitted to independent admission |
| R4 — Portfolio/meta-learning loop | Results across components and tasks, fixed audit policy | Choose next component/experiment; test bottleneck hypotheses; propose curriculum/routing changes | Experiment plans, training curricula, candidate policy revisions | Budget exhausted, evidence insufficient, no expected useful experiment, or bounded review complete |
| R5 — Architecture governance loop | Repeated interface failures, ADRs, invariant/TCB changes | Compare architecture alternatives, migration cost, effects and evidence | Reviewed specifications and explicitly approved policy changes | Independent owner/governance decision; never self-activation |

The admission authority is outside R3/R4. Runtime capability enforcement and protected final evaluation are outside learner write permissions at every loop level.

## R1 exact protocol

Acquire current snapshot and grant references; verify parent budget; resolve schema dependencies; run Rule/Reflex/Generate/Reason as permitted; construct a concrete action artifact; obtain a scoped prediction or explicit “not supported” record once that feature is enabled; prepare; atomically revalidate; execute; record observed delta; run the registered action check; release reservations and update actual usage.

The runtime preserves a hard semantic split:

```text
DECIDE   = choose bounded semantic intent/target; no side effect
GENERATE = synthesize open-ended payload/artifact; no authority
ACT      = trusted executor validates grant + payload + freshness, then performs effect
```

A Reflex answer can never directly become a shell command, path, selector, network destination, or write. Open-ended strings from a generative backend remain untrusted payloads until the executor's typed action validator accepts them.

Invalid or stale inputs go back to observation, not to repeated execution. No side effect is performed while merely choosing a branch. An ambiguous external outcome enters reconciliation. Exact host replay serves recorded outcomes and performs no real side effect.

## R2 exact protocol

Maintain current subgoal, hypotheses, contradictions, expected next observations, progress signature and budgets. Choose between action, evidence gathering, deliberate reasoning, authorized human review or stop. Replan after meaningful new evidence, not automatically after every unchanged observation. A DONE proposal triggers protected goal checks; failure returns the evidence and consumes the same task budget.

Learning inside an episode is limited to working-state updates. Changing model weights, calibrated thresholds or gate versions creates an explicit new epoch with fresh applicability checks; it does not silently alter the running task.

## Error attribution as an early cross-loop service

Do not wait for late-stage meta-learning to begin failure localization. Every failed/blocked episode should emit candidate responsibility across Observer, Retrieval, Reflex, WorldModel, Planner, Generator, Executor, Verifier, MultiCausal, or Unknown. Attribution is a hypothesis unless supported by substitution/intervention evidence; it is used to choose the next diagnostic experiment, not to trigger broad retraining automatically.

Track whether the failure came from missing candidates, wrong state, state-insensitive decision behavior, generation, stale execution, protected-check failure, or verifier insufficiency. The Reflex corpus should retain these cases for targeted classifiers and question-decomposition experiments.

## R3 exact protocol

Select a narrow hypothesis about a component failure. Validate data eligibility and label quality. Freeze the baseline, split and evaluation policy. Propose an update in an isolated workspace. Evaluate with ablations and actual resource accounting. Submit a complete manifest to CX-11 admission. Rejected candidates and failed experiments remain in provenance; they are not omitted from progress reports.

Do not require a single certain causal attribution before experimenting. Unknown/multi-causal records can motivate targeted probes. They cannot justify changing every component at once.

## R4/R5 exact protocol

R4 allocates research attention and compute; it does not change what counts as safe or successful. It may propose a revised metric when existing evidence shows a mismatch. R5 independently reviews the proposal, reruns incumbents/challengers under both policies and records the change. Old results retain their original policy version.

This distinction prevents a system from reporting progress by lowering thresholds, selecting easier tasks, deleting failures or removing required checks.

## B. Builder loop hierarchy

| Loop | Purpose | Typical output |
|---|---|---|
| B-inner | Implement one falsifiable behavior change with red/green, negative and seam tests | Reviewable patch plus command/evidence record |
| B-outer | Integrate a vertical slice across observer, executor, model seam and verifier | Demonstrated milestone with baseline comparison and rollback |
| B-meta | Find which development assumption, interface or test is causing churn | Bounded experiment or ADR, not an unreviewed rewrite |

A builder working on the verifier cannot simultaneously serve as its release approver. Generated tests are useful development artifacts but must be reviewed against the locked requirement; tests manufactured solely to pass the new implementation do not establish correctness.

## Reflex loop instrumentation

When R1 invokes Reflex, record exact model/backend revision, state digest, question digest, ordered dynamic option set, raw score source, any derived probabilities, calibration artifact, timing/cost and eventual verified outcome. If the backend uses shared-state/prefix caching, split timing into state prefill/encoding, incremental question work and incremental option work.

The router treats `NativeOptionLogit`, `SequenceLikelihood`, `GeneratedEstimate`, `EntropyDerived`, `EnsembleEstimate`, `CalibratedEmpirical`, and `Unavailable` as different evidence origins. Calibration/authority is never inferred merely from a numeric score.

## Common loop envelope

Every loop invocation has loop ID/kind, parent ID, objective, versioned inputs, read/write scope, authority, budgets, attempt count, evidence access, progress measure, allowed updates, termination criteria and output schema. Parent budgets are reserved/charged atomically across descendants. Parent cancellation stops new child work and triggers the host's actual termination protocol.

## Example: type-lowering failure

R1 runs the authorized parity check and observes a divergence. R2 compares two hypotheses—type-map mismatch versus incorrect native layout—and chooses a targeted inspection/check. R3 later tests a specialized error-triage adapter trained on eligible verified cases. R4 may prioritize type-map work over more model inference if ablations support that bottleneck. R5 reviews an actual compiler/invariant change; the learner cannot alter reference semantics to make the divergence disappear.

## Example: misleading improvement

A Reflex candidate is cheaper but abstains on every hard task. Component latency improves, but task coverage violates the fixed policy. R3 rejects it. R4 cannot fix this by deleting those tasks. It can propose a new domain-limited deployment envelope, which is then explicitly evaluated and approved as a narrower capability.

## C. Experience federation and knowledge loops (v0.4)

Cortex now distinguishes where experience came from without giving any source a different truth standard.

| Loop | Inputs | Work | Writes | Stop condition / gate |
|---|---|---|---|---|
| E1 — Experience federation | Native Axon episodes, MiCode bundles, generated curricula | schema/identity migration, data-use eligibility, dedupe/lineage, semantic normalization | eligible evidence store; never authority registries | invalid schema/lineage/policy, unresolved required identity, or accepted evidence bundle |
| E2 — Repository knowledge loop | eligible repositories/histories/MiCode episodes | extract patterns, search counterexamples, reproduce locally, benchmark/holdout | knowledge candidates + negative knowledge | contradicted scope, unreproducible claim, inconclusive evidence, or PromotionEligible candidate |
| E3 — Crystallization loop | admitted pattern/concept/skill candidates | synthesize guarded skill/tool/library/compiler/runtime candidate, run CX-11/CX-15 evidence | candidate artifact registry only | rejected/inconclusive, active guarded artifact, or deoptimized previous-good |

The canonical timescales are: ms–seconds working state/routing; seconds–minutes planning/experiments; hours skill/tool/calibration candidates; days world-model/representation/knowledge updates; weeks+ model/compiler/library/runtime promotion and governance. Trusted verifier/admission/capability policy never silently adapts inside R1/R2.

MiCode is an E1/E2 producer and experimental consumer. A MiCode episode may be rich evidence, but its local permission outcome cannot authorize Axon effects. Imported historical actions are never replayed as real effects merely because they occurred before.

## D. Intent loops (v0.5)

| Loop | Inputs | Work | Writes | Stop condition / gate |
|---|---|---|---|---|
| I1 — Intent inner loop | natural/structured intent | parse, type/normalize, detect conflict/ambiguity, resolve/refuse, semantic render | versioned Intent IR proposal only | valid unresolved/refused/approved contract; never "retry until convenient" |
| I2 — Intent/task outer loop | approved Intent IR | lower to AIR, plan/act/observe/replan while preserving clauses | AIR/execution/evidence artifacts | independently verified completion, explicit blocked state, or budget stop |
| I3 — Improvement-intent loop | measured bottleneck/pattern/knowledge candidate | construct `ImprovementIntent`, request authority/evidence, evaluate candidate | candidate/admission artifacts | reject/inconclusive/promote through CX-11; learner cannot change its exam |
| I4 — Intent meta loop | ambiguity/rendering/schema outcomes | propose parser/schema/UX changes, benchmark on protected intent corpus | future candidate versions only | independent evaluation/admission; active episode contract is immutable |

Intent clauses are parent constraints for R1/R2. Runtime replanning can choose a different strategy or stronger verification, but cannot silently relax the approved hard constraints, authority ceiling or required evidence.

---

<a id="doc-build-self-optimizing-os-md"></a>

## build/SELF_OPTIMIZING_OS.md

# Axon self-optimizing OS loop

## Headline objective

Axon Cortex is the cognitive control plane for a self-optimizing language/compiler/runtime/OS. The system learns from three evidence sources:

1. **self experience** — Axon's own executions, predictions, failures and experiments;
2. **external experience** — MiCode coding episodes, repositories, histories, benchmarks and approved research artifacts;
3. **generated experience** — curricula, synthetic bugs, adversarial tasks, simulations and controlled counterfactuals.

All three enter one evidence → abstraction → validation → admission → crystallization pipeline. They do not receive separate truth standards.

## Knowledge ladder

```text
Episode
  → Memory / Evidence
  → Pattern
  → Concept / Representation
  → Reflex / Retrieval Policy
  → Skill / Tool
  → Library Abstraction
  → Compiler Transformation
  → Runtime Primitive
```

Progress down the ladder means more reuse and typically a larger blast radius, so required assurance increases. Many useful concepts should never become native primitives.

## Coupled optimization loops

- better Observer → better state → better decisions and training labels;
- better Reflex → cheaper high-frequency decisions → more experiments;
- better World Model → better experiment selection → better causal/abstraction evidence;
- better Verifier → safer aggressive exploration → stronger promotion evidence;
- better Compiler/Runtime → cheaper experiments → larger searchable improvement space;
- better repository knowledge extraction → better candidate abstractions → better OS/library/compiler choices.

## Timescales

| Timescale | Allowed adaptation |
|---|---|
| milliseconds–seconds | working state, cached observations, Reflex/routing decisions |
| seconds–minutes | subgoals, hypotheses, representation/experiment choice |
| hours | skill/tool candidates, calibration/retrieval updates in isolated evaluation |
| days | world-model/representation/knowledge updates through admission |
| weeks+ | model training, compiler/library/runtime promotion, governance changes |

Trusted admission/proof/capability enforcement never mutates online because a learner wants an easier pass.

## System-level scorecard

Track verified coding frontier, success per cost, time-to-correct-patch, frontier-model usage, tool/build/test calls, human intervention, calibration/coverage, prediction error, regression/rollback rate, compiler build time, generated-program performance, memory/binary size, proof burden and capability footprint. Do not collapse these into one unreviewed scalar objective.

## First closed-loop demonstration

A MiCode-exported or native Cortex coding episode reveals a recurring bounded workflow. Cortex turns it into a guarded skill/tool candidate, independently evaluates it on held-out fixtures, promotes it through CX-11, uses it in later tasks, detects an injected applicability mismatch and deoptimizes to the previous path. This proves the learning/crystallization plumbing without requiring compiler self-modification.

## Intent as the control envelope (v0.5)

Self-optimization does not mean free mutation. Every consequential change begins as an approved typed intent. Self experience, external repositories and generated curricula may produce **ImprovementIntent candidates**; those candidates specify objective, scope, constraints, requested authority and required evidence, then flow through normal AIR/executor/verifier/admission machinery. This makes "why is the OS changing itself?" a query over explicit intent/evidence lineage rather than an inference from a patch after the fact.

Canonical loop:

`measurement/knowledge → ImprovementIntent → Intent IR → AIR candidate search → artifact → independent evidence → CX-11/CX-18 admission → active/deoptimized artifact`.

A learner may improve how it proposes future intents, but it may not mutate the active intent, admission policy or verifier to make the current candidate pass.

---

<a id="doc-build-intent-compiler-md"></a>

## build/INTENT_COMPILER.md

# Intent compiler build guide

## Objective

Build the missing layer above Cortex: **natural/structured intent → typed Intent IR → semantic review/approval → constrained AIR → Axon execution/evidence**. The first vertical slice should prove contract preservation, not language-generation sophistication.

## Architectural rule

Do not compile free-form English directly into authority-bearing `.ax` or executable AIR. Natural language is untrusted input to an intent compiler. The first authoritative object is an approved typed `IntentIR`.

```text
human/system intent
      ↓
intent parser / resolver
      ↓
Intent IR
      ↓
ambiguity + authority + acceptance review
      ↓
approved semantic contract
      ↓
AIR lowering / planning
      ↓
Axon program/actions
      ↓
independent evidence
      ↓
result explained against original intent
```

## First vertical scenario

Input:

> Make this parser path faster without changing observable behavior. Do not add dependencies or network access. Keep peak memory within 10%. Show benchmark and regression evidence.

Expected typed contract:

```text
objective: minimize latency(parser_path)
hard constraints:
  - behavior_equivalent(baseline)
  - peak_memory_delta <= 10%
  - no_new_dependencies
  - no_network
requested authority:
  - inspect/edit(parser scope)
  - run(registered tests/benchmarks)
required evidence:
  - regression suite
  - parity/reference check
  - benchmark
```

Cortex may choose many different plans. All must preserve this contract.

## Slices

### I0 — Source-truth mapping

Map the existing `axon intent compile`, `axon ast review`, approval artifact, deploy gate and structured-prose implementation. Record actual schemas, trust assumptions and drift before adding another layer. Reuse existing components where possible.

### I1 — Intent IR schema

Implement/version the CX-19 schema and round-trip fixtures. Preserve unknown/absent, hard/soft, requested/forbidden authority, evidence requirements and provenance separately.

### I2 — Resolver and ambiguity protocol

Convert simple structured prose to Intent IR. Emit explicit unresolved alternatives/questions. No consequential default on materially ambiguous input.

### I3 — Semantic renderer + approval

Render intent into stable human-readable contract text plus machine-readable diff. Approval binds the exact Intent IR digest/version. Modified intents require renewed approval.

### I4 — Intent→AIR lowering

Lower one simple repair/optimization intent to an AIR graph. Every AIR node/action carries clause lineage to objectives/constraints/evidence requirements. Replanning cannot relax the parent contract.

### I5 — Completion and explanation

Map independent verifier receipts back to required evidence clauses. Produce an explanation: achieved objective, preserved/violated constraints, authority actually exercised, evidence obtained, unresolved items.

### I6 — ImprovementIntent

Create one **system-generated** improvement proposal from a measured Cortex/Axon bottleneck. It must enter through the same intent/authority/admission flow and cannot activate itself.

## Inner / outer / meta loops

**Intent inner loop:** parse → type/normalize → detect ambiguity/conflict → resolve/refuse → render. It terminates on a valid unresolved/refused/approved contract; it does not silently retry until an LLM returns a convenient interpretation.

**Task outer loop:** approved Intent IR → plan → act → observe → replan while preserving clauses → independent completion. Replanning changes method, not success semantics.

**Self-optimization loop:** measurement/knowledge → ImprovementIntent candidate → normal intent/admission path → candidate implementation → evidence → promote/reject. The learner cannot edit the contract or evaluator after seeing results.

**Intent meta loop:** analyze recurring ambiguities, clause types and renderer failures; propose parser/schema/UX improvements through normal governance. Do not adapt the active acceptance contract during the episode being evaluated.

## Negative cases

- English asks for faster performance but omits what behavior must remain stable: require ambiguity/acceptance resolution.
- Model lowers `no network` into an AIR HTTP call: authority gate refuses.
- Planner drops the benchmark because tests passed: completion remains blocked.
- System-generated compiler optimization asks to edit verifier policy: separate authority/admission refuses.
- Intent edited after approval: old approval digest is invalid.
- Semantic renderer hides an assumption or requested capability: conformance gate fails.

## Definition of done

The slice is complete only when one prose intent produces an approved typed contract, a simple Cortex/AIR execution, independent evidence, and a final result traceable to the original clauses; all negative cases above are runnable. Do not call prose→code generation alone an intent compiler.

---

<a id="doc-build-micode-experience-bridge-md"></a>

## build/MICODE_EXPERIENCE_BRIDGE.md

# MiCode ↔ Axon experience bridge build guide

## Purpose

MiCode is an optional research/experience plane for Axon Cortex. The bridge exists to turn real coding sessions into versioned evidence and to let MiCode exercise approved Cortex policies without sharing authority models.

## Role split

```text
MiCode
  coding world + human-visible harness + episode producer + repo knowledge miner
        │
        │ versioned artifacts only
        ▼
Axon Cortex
  cognitive runtime + world models + Reflex + admission/crystallization
        │
        ▼
Axon language/compiler/OS
  verified execution substrate + destinations for promoted knowledge
```

MiCode remains independently safe/unsafe according to MiCode's own runtime. Axon must not infer that a MiCode action was sandboxed or scope-confined unless the exported evidence proves that specific property. Conversely, an Axon policy exported to MiCode never bypasses MiCode's gate.

## Initial contract

Start with seven export families from MiCode: canonical coding episode, software observation/catalog, decision dataset, failure attribution, benchmark bundle, repository knowledge candidate, and skill/tool/compiler candidate. Start with five Axon export families: AIR policy, Reflex backend manifest, verifier profile, capability/action schema, and ontology/version map.

Implement a dummy producer and consumer first. Freeze schema/version/error semantics before live coupling. Use explicit `Unknown`, `NotObserved`, `Denied`, `Failed`, `Aborted`, `NotExecuted`, `Verified`, `Unverified` states rather than flattening them.

## Build sequence

1. Map MiCode fields to CX-16 without exposing credentials or local authority handles.
2. Build canonical JSON/JSONL fixtures with edge cases.
3. Validate round-trip and schema migration.
4. Import one recorded MiCode repair episode into Cortex replay/evaluation.
5. Export one non-authoritative Cortex decision/verifier profile to a MiCode dummy consumer.
6. Add data-eligibility/lineage checks before any imported episode reaches learning.
7. Add semantic replay/counterfactual comparison on imported episodes.
8. Only then connect real MiCode production traces.

## Non-goals

- no RPC dependency from Axon runtime to MiCode;
- no shared permission token or principal namespace;
- no claim that MiCode episode success implies optimality;
- no automatic promotion of MiCode skills into Axon;
- no raw transcript-as-training shortcut.

## Exit demonstration

One real or fixture MiCode episode round-trips into Axon, supports a recorded-policy counterfactual replay with no external effects, preserves all important outcome/authority distinctions, and remains ineligible for training when its data-use manifest is removed.

---

<a id="doc-build-knowledge-ingestion-md"></a>

## build/KNOWLEDGE_INGESTION.md

# External repository knowledge ingestion build guide

## Objective

Use MiCode sessions and approved external repositories as evidence for better Axon abstractions without importing code popularity as truth.

## Canonical pipeline

```text
repo/history or MiCode episode
  → normalized semantic evidence
  → candidate pattern/concept
  → counterexample search
  → local reproduction
  → benchmark / held-out transfer
  → CX-11 admission
  → optional CX-18 crystallization
```

## Minimum evidence record

For each candidate store: source repositories/artifacts and revisions; exact extracted examples/non-examples; extraction version; hypothesized scope/mechanism; license/data-use policy; contradicting evidence; reproduction instructions/results; benchmark environment; held-out families; current evidence stage; dependent derived artifacts.

## First experiment

Choose a narrow software-engineering pattern that appears in multiple repositories and is relevant to Axon (for example a bounded retry/backoff, parser recovery pattern, or structured cancellation idiom). Reproduce at least two variants in resettable local fixtures. Measure correctness/resource/complexity effects. The goal is to validate the knowledge pipeline, not to prove a universal abstraction.

## Controls

- shuffled repository labels;
- pattern extracted from only one source;
- popular but locally worse implementation;
- contradictory examples;
- removed/invalidated source permission;
- duplicated/forked repositories counted as one lineage family;
- static-source-only versus history/outcome-aware extraction.

## Stop rules

Stop/pivot when provenance cannot be established, counterexamples invalidate the proposed scope, local reproduction fails, or held-out benefit is inconclusive under the registered evaluation policy. A failed candidate remains useful negative knowledge.

---

<a id="doc-build-tasks-md"></a>

## build/TASKS.md

# Dependency-ordered build work packages

All entries are **Not started**. Gate targets name the eventual acceptance checks touched by a slice, not scripts already implemented. Documentation/intake slices complete with reviewed artifacts; full runtime gate claims wait for executable prerequisites. Optional research/host/compiler branches are not mandatory dependencies of the M1–M7 core.

Use [BUILD_PROTOCOL](build/BUILD_PROTOCOL.md) for every slice and [WORK_PACKAGE template](templates/WORK_PACKAGE.md) to expand the selected row before editing code.

| Slice | Stage / lane | Work | Depends on | Concrete output | Gate targets |
|---|---|---|---|---|---|
| B00 | M0 / contracts | Import and reconcile the package | — | Owner-reviewed naming, scope and CX-to-repository ID mapping; preserve existing governance. | G00-contract |
| B01 | M0 / contracts | Audit Axon seams and engine guarantees | B00 | Commit-pinned evidence matrix; record native ceiling, missing-gate, FFI, R44 and OS discrepancies. | G00-missing-gate, G15-parity |
| B02 | M0 / evaluation | Freeze task, safety and evaluation policy | B01 | Reviewed initial workload, required gates, host profile, budgets, split rules and evaluation margins. | G00-contract, G01-evidence |
| B03 | M0 / evaluation | Build resettable task fixtures and simple controls | B02 | Tiny repair fixtures plus stale/malicious/crash cases; strong-model and rules control definitions. | G01-reset, G01-fairness |
| B04 | M0 / contracts | Implement common manifests and event schemas | B01, B02 | Validated Run/Action/Evidence contracts, versions, canonical digests and negative schema tests. | G00-contract, G04-schema, G10-lineage |
| B05 | M1 / observer | Implement partial software observations | B03, B04 | Content-based snapshots, incomplete AST/diagnostics and scope-expansion path. | G02-canonical, G02-partial, G02-recall |
| B06 | M1 / executor | Implement grants and denial-first catalog | B04, B05 | Principal/session/snapshot-bound grant registry; forged and stale references denied. | G03-forgery, G03-budget |
| B07 | M1 / executor | Implement the H0 isolated tool host | B01, B04, B06 | Tested local sandbox, environment projection, process-tree cancellation and quotas. | G13-escape, G13-kill, G13-quota |
| B08 | M1 / executor | Implement local patch transactions | B05, B06, B07 | Immutable patch artifacts, constrained apply, locked/CAS commit and rollback. | G03-payload, G03-race |
| B09 | M1 / executor | Execute registered checks with durable action states | B03, B07, B08 | Approved test/build adapter, write-ahead action state and crash reconciliation. | G03-crash, G13-recovery |
| B10 | M1 / runtime | Implement AIR validation and serial scheduler | B04, B06 | Typed graph execution with explicit dependencies, bounded iteration and effect checks. | G04-schema, G04-effect, G04-scheduler |
| B11 | M1 / models | Add mock decision and generation adapters | B10 | Deterministic fixture adapters with clear mock provenance, invalid-result tests and no authority. | G05-schema, G05-branch |
| B12 | M1 / evaluation | Implement protected final verification | B03, B09 | Agent cannot alter hidden checks; final artifact digest and task contract determine completion. | G03-done, G01-leakage |
| B13 | M1 / evidence | Integrate raw/redacted events and exact replay | B04, B09, B10, B11 | Record model/tool seam; replay without effects; secret-safe review view. | G10-replay, G10-secrets, G10-crash |
| B14 | M1 / integrator | Run deterministic end-to-end conformance | B11, B12, B13 | Safe repair golden path plus adversarial, cancellation and recovery demonstrations. | G00-mutation, G03-done, G10-replay |
| B15 | M1 / models | Add an authorized existing-model backend | B02, B07, B10, B13, B43 | Pinned model adapter for bounded decisions and untrusted patch generation; explicit fallback and accounting. | G04-fallback, G05-schema |
| B16 | M1 / integrator | Compare the real-model vertical slice to controls | B14, B15 | Protected task-family results, full costs, quality intervals and independent milestone evidence. | G01-ablation, G01-evidence |
| B17 | M2 / models | Freeze backend-neutral Reflex ABI and score provenance | B15, B16 | Choice/Binary/Ordinal contracts, dynamic candidate manifests, typed probability origins, and generative/direct-logit/sequence/learned adapter conformance scaffold. | G05-schema, G05-tokenization |
| B21 | M2/M3/M5 / evidence | Implement eligible learning/decision-corpus export and label lineage | B13, B16 | Purpose-scoped dynamic-choice datasets, grouped protected splits, verified/human/teacher/weak/delayed/unknown labels, attribution records and revocation lineage. | G10-secrets, G10-lineage, G10-attribution |
| B18 | M2 / models | Build Reflex decision corpus, destructive controls and shared-state/speculative path | B17, B21 | Grouped verified decision corpus; shuffled/empty/wrong/stale-state and option-order/ID controls; branch-specific targets; serial/parallel/shared-prefix timing. | G05-branch, G05-isolation, G05-performance |
| B19 | M2 / models | Run calibration lab and implement selective routing | B02, B16, B17, B18 | Pinned calibration artifacts by probability origin; proper scoring, risk/coverage/VUC, deterministic fallback, explicit OOD/abstention and budget control. | G06-calibration, G06-risk, G06-coverage, G06-budget |
| B20 | M2 / integrator | Run multi-backend Reflex bakeoff and decide adoption | B18, B19, B42, B43 | Compare generative, direct-logit, sequence and learned-head families on one protected corpus; retain only measured useful configurations; baseline remains active when evidence is inconclusive. | G05-performance, G06-shift, G01-evidence |
| B22 | M3 / research | Match concrete action predictions to outcomes | B05, B13, B16, B21 | Patch/environment-bound prediction schema and simple dependency/base-rate baselines. | G07-target, G07-unknown |
| B23 | M3 / research | Train short-horizon software predictors | B20, B22 | Diagnostic/test outcome models with held-out calibration and declared applicability. | G07-holdout, G07-unknown |
| B24 | M3 / evaluation | Evaluate real planning value and model exploitation | B23 | No-model/simple-model/challenger ablations and adversarial candidate-search evidence. | G07-planning, G07-exploitation |
| B25 | M4 / research | Implement hypotheses and permitted probe selection | B19, B24 | Bounded outer planner and concrete experiment artifacts over approved checks. | G08-hypothesis, G08-authority, G08-progress |
| B26 | M4 / evaluation | Evaluate controlled interventions and diagnosis | B25 | Matched reset/control experiments; scoped causal claims and baseline comparison. | G08-controls, G08-value |
| B27 | M5 / admission | Implement independent artifact admission | B12, B19, B21 | Protected candidate registry, digest-bound gate receipts and shadow/canary states. | G11-independent, G11-authority |
| B28 | M5 / learning | Derive one guarded reusable tool/rule/policy | B20, B27 | Candidate with applicability predicate, empirical/proof scope and approved fallback. | G11-scope, G11-noninferiority |
| B29 (optional) | Research / research | Run learned option-conditioned Reflex learning curves | B20, B21, B27 | Small-model/decision-head experiment over runtime candidate sets with grouped splits, destructive state controls and calibration; pursue or stop based on measured value. | G14-pilot, G14-quality |
| B30 | M5 / integrator | Demonstrate promotion and rollback | B27, B28 | Independently admitted specialization, non-effecting shadow comparison and regression recovery. | G11-rollback, G11-independent |
| B31 | M6 / research | Compare fixed alternative problem encodings | B24, B26 | Two traceable representations evaluated against compute-matched controls. | G09-lineage, G09-ablation |
| B32 | M6 / research | Test learned concepts and held-out transfer | B31 | Typed concept/representation proposals with counterexamples, fit and novel-family evidence. | G09-counterexample, G09-transfer, G09-compression |
| B33 | M7 / learning | Unify independently useful pillar lifecycles | B26, B30, B32 | At least two evaluated update pipelines share manifests, admission and rollback without shared authority. | G12-contract, G12-cause |
| B34 | M7 / research | Add bounded curriculum and meta experiment allocation | B33 | Meta proposals with budgets; protected policy/audit boundary; task-generator lineage. | G12-meta, G12-curriculum |
| B35 | M7 / evaluation | Run longitudinal stability and transfer checks | B34 | Pinned-version multi-episode improvement report, regression audit and no self-grading. | G12-stability, G12-meta |
| B36 | OS-H1 / os | Harden hosted service operation | B16 | Job queues, durable restart, observability, revocation and configured host compatibility matrix. | G13-recovery, G13-quota, G13-tier |
| B37 | Compiler / compiler | Audit stable runtime-to-language seams | B01, B16 | Actual R2a status, wrapper APIs, type ownership map, language-surface friction evidence. | G15-types, G15-docs |
| B38 (optional) | Compiler / compiler | Integrate authoritative types for new semantics | B37 | Sequenced type-map change only when required; preserved import/AST identity and regression evidence. | G15-types, G15-parity |
| B39 (optional) | Compiler / compiler | Add justified native/language/proof support | B20, B38 | Evidence-backed desugaring/native subset/proof receipts with explicit unsupported cases. | G15-proof, G15-optimization, G15-parity |
| B40 (optional) | Research / research | Investigate specialized shared-state Reflex architecture | B29, B30 | Shared encoder/prefill, parallel typed heads, quantization/cache or RLCD-like training experiment only for a demonstrated serving/quality bottleneck; compare against B20 winners. | G14-parallel, G14-version, G14-release |
| B41 (optional) | OS-K / os | Evaluate the separate bare-metal branch | B01, B36, B39 | Owner-approved kernel plan with actual confinement, syscall, driver and TCB obligations. | G13-tier, G15-proof |

| B42 | M2 / contracts | Implement Reflex conformance fixtures and backend feature/effective-input receipts | B17 | Primitive-specific batch results keyed by QuestionId; provider/logit/sequence/generated score provenance; backend capability manifests; prompt-role/isolation/truncation/retry fixtures. | G05-schema, G05-isolation, G05-tokenization |
| B43 | M0/M2 / supply-chain | Implement dependency/model/dataset adoption manifests | B01, B02 | Commit/license/security/transitive-model/tokenizer/encoder/dataset/evaluator lineage; benchmark comparability review; SDK transformation and hosted-profile checks. | G00-contract, G01-evidence, G13-tier |

| B44 | M1/M2 / contracts | Freeze MiCode↔Axon experience bridge and conformance fixtures | B04, B13 | Versioned episode/observation/decision/knowledge schemas; authority separation; dummy producer/consumer; migration tests. | G16-schema, G16-authority, G16-version |
| B45 | M2/M5 / evidence | Import one canonical MiCode coding episode into Cortex | B21, B44 | Provenance-preserving episode import, semantic replay, eligibility/lineage and source-system attribution. | G16-lineage, G16-replay, G10-lineage |
| B46 | M8 / knowledge | Build external repository/history intake and knowledge-candidate extractor | B32, B45 | Read-only normalized repo/history evidence, pattern/counterexample store, license/data-use manifests and dedupe lineage. | G17-provenance, G17-license, G17-counterexample |
| B47 | M8 / evaluation | Reproduce and evaluate one cross-repository knowledge candidate | B46 | Resettable local reproduction, benchmark/control evidence and held-out repository-family evaluation. | G17-reproduce, G17-heldout, G01-evidence |
| B48 | M9 / learning | Synthesize one guarded skill/tool from admitted knowledge/episodes | B30, B47 | Pattern→Skill/Tool candidate with applicability, effects/capabilities, intermediate checks, fallback and full lineage. | G18-ladder, G18-authority, G11-scope |
| B49 | M9 / admission | Demonstrate skill/tool promotion, use and deoptimization | B48 | Independently admitted artifact used on held-out tasks; injected applicability/regression case returns to previous-good path. | G18-deopt, G11-independent, G11-rollback |
| B50 | M9 / integrator | Demonstrate closed self/external/generated experience loop | B35, B45, B49 | One report showing common evidence pipeline, source-separated ablations, failure attribution and OS-wide scorecard. | G12-stability, G16-lineage, G18-ladder |
| B51 (optional) | Compiler/OS / compiler | Promote a proven capability into Axon library/compiler/runtime | B37, B49 | Measured native/library candidate with CX-15 parity/proof/invariant evidence; refusal/fallback if unjustified. | G18-native, G18-equivalence, G15-parity, G15-proof |

| B52 | M0/M1 / contracts | Map existing Axon intent/surface/approval implementation | B01 | Commit-pinned map of `intent compile`, AST review/approve, structured-prose surface, authority/evidence semantics and drift. | G19-parse, G19-render |
| B53 | M1 / contracts | Implement versioned Intent IR and round-trip fixtures | B04, B52 | Typed goal/constraints/preferences/authority/budget/evidence/ambiguity schema with provenance and migrations. | G19-parse, G19-ambiguity |
| B54 | M1 / surface | Implement ambiguity resolution and deterministic semantic renderer | B53 | Explicit interpretation alternatives/questions, stable contract renderer/diff and digest-bound approval artifact. | G19-ambiguity, G19-render |
| B55 | M1 / integrator | Lower approved Intent IR to constrained AIR | B14, B54 | Clause-traceable AIR graph; replanning can narrow/add checks but cannot widen authority/drop required evidence. | G19-authority, G19-trace |
| B56 | M1 / evaluation | Demonstrate intent-to-evidence vertical slice | B16, B55 | Prose intent → approved contract → bounded repair/optimization → independent evidence → explanation against original clauses. | G19-evidence, G19-trace |
| B57 | M9 / learning | Demonstrate self-optimization through ImprovementIntent | B30, B50, B56 | Measured bottleneck generates typed improvement proposal that requests authority/evidence and passes normal admission; it cannot activate/edit its own gate. | G19-authority, G19-evidence, G11-independent |

## Scheduling notes

B00–B04 establish contracts and evidence. B05/B10 can develop in parallel against frozen contracts; B07 must establish actual host confinement before B09 runs untrusted checks. B14 proves the deterministic plumbing; B16 proves the model-driven workflow. Neither should be presented as the other.

After B20, prediction work (B22–B26) and admission/crystallization (B27–B30) can proceed separately using B21 eligible data. The mandatory M2 sequence is B17 → B42 → B21 → B18 → B19 → B20; B43 starts after B01/B02 and is required before external backends enter protected comparisons (ABI → eligible grouped corpus → destructive/speculative/shared-state evaluation → calibration/router → bakeoff); B29/B40 remain optional specialized-model research. A failed optional small-model pilot B29 does not block a useful guarded tool/policy promotion. B38/B39 require a demonstrated language/native need; no imperative to churn the compiler just to complete the table.

B41 preserves the OS ambition but cannot claim a kernel until its separate proposal, resources and real isolation evidence are approved. Shared-memory/budget interfaces and R2a-like compiler changes have one integration owner.

B44 freezes the bridge contract without making MiCode a Cortex dependency. B45 can begin as soon as a conforming MiCode episode exists. M8 requires B32 so repository patterns are evaluated through the representation/abstraction machinery rather than dumped straight into training. B48/B49 demonstrate userland crystallization first; B51 is optional and cannot become a required dependency of the self-optimization core.

### v0.5 intent sequencing

B52 maps existing intent functionality before adding schemas. B53/B54 establish the approved semantic contract. B55 depends on the existing AIR vertical plumbing rather than inventing a parallel executor. B56 is the first end-to-end user-intent demonstration. B57 is intentionally late: system-generated self-improvement must use the same contract only after independent admission exists.

| B58 | M2 / models-runtime | Implement Reflex Runtime state handles and branch scheduler | B17, B18, B42 | Immutable/emulated StateHandle contract, shared-state scheduler, candidate packing/order manifests, per-branch accounting and cancellation. | G05-state-handle, G05-branch |
| B59 | M2 / evaluation | Run question-isolation and candidate-order conformance | B58, B19, B21 | Q1-alone/sibling/adversarial/100-question and permutation suite; calibration-domain drift report and stop/pivot decision per backend. | G05-question-isolation, G05-order-domain, G06-shift |
| B60 (optional) | M5 / research | Compare listwise and option-conditioned Reflex architectures | B20, B21, B27, B29 | Independent/pointer/listwise compute-matched pilot with dynamic candidate compositions, proper-scoring learning curves and task-level evaluation. | G14-listwise, G14-training-score, G14-quality |
| B61 (optional) | Compiler / compiler | Implement AIR question-dependency scheduling optimization | B10, B37, B58 | `Independent`/`ConditionallyRelevant`/`AnswerDependent` validation, legal batching/speculation, and changed-policy detection for semantic fusion. | G15-dependency-types, G15-batch-semantics, G15-optimization |

| B62 (optional) | M2 / research | Stand up Kev-style Axon Reflex reference pilot | B21, B58, B59 | Small backbone + adapter + isolated branch mask + pointer/listwise head trained with canonical encoding; reproducible learning curve. | G14-pilot, G14-canonical-encoding |
| B63 | M2 / evaluation | Freeze Reflex train/calibration/dev/locked-test/transfer suites | B21, B02 | Repo-aware split manifest, revision hashes, inaccessible locked test and explicit transfer ladder. | G14-locked-test, G01-leakage |
| B64 (optional) | M5 / research | Run permutation-robustness training study | B62, B63 | Canonical-order vs shuffle augmentation vs permutation-consistency objective under matched compute. | G14-permutation-training, G05-order-domain |
| B65 | M2 / data | Add typed candidate-absence/control examples | B21, B42 | Missing-target/distractor fixtures with registered NONE/OBSERVE_MORE/ESCALATE outcomes and verifier labels. | G14-absent-candidate |
| B66 | M2 / runtime | Unify canonical decision encoding across train/eval/serve/replay | B58, B63 | Single versioned renderer/encoder API plus effective-input receipts and replay fixtures. | G14-canonical-encoding, G05-conformance |
| B67 | M5 / evaluation | Measure Coding Transfer Frontier | B62, B63, B65, B66 | Same-repo→unseen-repo→unseen-family/task-family→cross-language transfer curves with selective quality/coverage/compute. | G14-transfer-frontier, G14-quality |
| B68 (optional) | M5 / research | Compare Kev-style transfer improvements | B64, B67 | Representation/data/backbone/augmentation ablations focused on held-out coding transfer; no locked-test tuning. | G14-transfer-frontier, G14-training-score |

---

<a id="doc-build-reflex-backend-bakeoff-md"></a>

## build/REFLEX_BACKEND_BAKEOFF.md

# Axon Reflex backend bakeoff

## Purpose

This build packet turns the System-One/Jev ecosystem review into a controlled Axon experiment. The goal is **not** to clone a vendor implementation or assume one neural architecture is correct. The goal is to freeze one Axon Reflex decision ABI, plug multiple backend families into it, and let protected task/evaluation evidence determine which backend deserves continued investment.

The ecosystem examples supplied by the user are treated as implementation leads, not reproduced evidence. Their benchmark, calibration, latency, and training claims must be rerun on Axon workloads before adoption.

## Stable Reflex ABI first

The runtime contract is backend-neutral. A backend receives one immutable state reference plus one or more typed finite questions and returns typed answers. It does not receive execution authority.

Required question families for the initial bakeoff:

- `Choice<T>` — one value from a runtime-supplied finite candidate set.
- `BinaryDecision` — internal Axon form for yes/no; a TypeSafe compatibility adapter may map this to/from `Noul` without making vendor terminology part of the core ABI.
- `OrdinalScore` — one bounded rubric category with explicit level meanings.
- `Rank<T>` — optional experimental family after Choice conformance; not required for the first safe repair loop.

Open-ended text/code is **not** a Reflex answer. It belongs to `Generate`. Tool execution is **not** a Reflex answer. It belongs to the executor after capability/freshness validation.

## Backend families

Run the same question corpus through at least these four families:

| Family | Definition | Implementation pattern | Probability semantics |
|---|---|---|---|
| A — Generative adapter | Ordinary LLM produces a constrained typed answer, optionally generated numeric confidence/probabilities | Official System-One-adapter-style pattern | Generated estimate or unavailable; never relabel as native logits |
| B — Direct option logits | Open model scores bounded labels/options directly | OpenJev-style label/logit implementation | Native option logits transformed under a declared normalization; not automatically calibrated correctness probability |
| C — Sequence scorer | Compute `P(option_text | state, question)` or a declared sequence score for every candidate | open-jev sequence-probability pattern | Sequence likelihood / normalized sequence score with explicit length/tokenization treatment |
| D — Learned decision head | Shared encoder or option-conditioned scorer trained for variable runtime candidate sets | Jevlike/ModernBERT-style experimental scorer | Model score/probability with separate empirical calibration artifact |

A fifth provider-native TypeSafe/Jev backend can be added as a comparison when available, but the core ABI and evaluation do not depend on it.

## Dynamic candidate-set requirement

The fundamental unit is not a fixed classification label. It is:

> score an arbitrary, runtime-generated, finite set of typed semantic possibilities conditioned on the current state.

A coding task may expose symbols `[sym:a3, sym:f9, sym:44]` today and a different set tomorrow. Backends must consume candidate IDs/descriptions at inference time, preserve opaque IDs, and avoid learning task authority from human-facing names alone.

Candidate-set metadata recorded for every request:

- ordered candidate IDs and descriptions;
- candidate-manifest digest;
- cardinality and token lengths;
- construction/retrieval policy version;
- branch condition, if conditional;
- candidate-order seed/permutation when under sensitivity testing.

## Required destructive controls

Every backend evaluation includes controls that test whether the model actually uses the state and semantics rather than priors or labels:

1. correct state;
2. shuffled state from another task in the same domain;
3. empty/minimal state;
4. wrong repository/project state;
5. randomized candidate order;
6. stable candidates with renamed opaque IDs;
7. irrelevant-state injection;
8. stale-state snapshot;
9. adversarial/distractor candidate set;
10. candidate omission followed by the permitted scope-expansion path.

A backend that retains nearly unchanged performance under destructive state shuffling is flagged as state-insensitive and cannot be promoted merely on ordinary accuracy.

## Shared-state / prefix-cache benchmark

Speculative questions benchmark both ordinary parallel calls and shared-state inference:

```text
parallel API calls:
  model(state, q1)
  model(state, q2)
  model(state, q3)

shared-state inference:
  encode/prefill(state) once
      ├─ q1 + options
      ├─ q2 + options
      └─ q3 + options
```

Record separately state encoding/prefill time, incremental question time, incremental option time, p50/p95 end-to-end latency, backend-only latency, KV/cache memory, token/compute cost, total speculative work/discarded work, and quality for the complete selected action packet.

“One request” is not treated as evidence of “one neural forward pass.” Shared-prefix caching is an optimization claim that requires measurement under the exact model/tokenizer/prompt/candidate schema.

## Decision provenance

Every answer record carries backend family/adapter version, exact model/weights/provider revision, tokenizer/quantization/runtime revision, state digest, question digest, candidate-manifest digest and ordered options, genuine raw backend scores where exposed, derived probabilities where computed, probability/uncertainty provenance class, calibration artifact ID if applicable, selected answer/abstention, latency/usage/cost/cache mode, and eventual independently verified outcome reference.

Do not synthesize raw logits for a backend that does not expose them.

## Acceptance decision

The bakeoff does not choose a universal winner. It can select different approved backends by question family/domain if the routing policy and calibration artifacts support that scope.

Continue investment only when the backend shows useful task-level selective performance after charging all state construction, inference, calibration, and fallback cost. Failure to beat the simple baseline is a valid result. Before escalating to a new architecture, inspect observation completeness, candidate recall, destructive controls, grouped-split leakage, label quality, tokenization/option semantics, calibration, and baseline strength.

## v0.3 conformance prerequisite and feasible-family rule

A backend enters scored comparison only after passing the feature subset it claims in [REFLEX_CONFORMANCE](build/REFLEX_CONFORMANCE.md) and completing [DEPENDENCY_ADOPTION](build/DEPENDENCY_ADOPTION.md) for the intended use. Backend-specific context/candidate limits are recorded rather than normalized away. Effective input, retries and transformations are charged and traced.

The M2 bakeoff compares all **feasible conforming** families. A learned head is desirable as an experiment when data/resources permit but is not mandatory to decide whether learned-head research should continue. `NotEvaluated` is distinct from `Failed` and does not block the safe coding loop.

Add candidate-search controls: flat scoring, retrieval+rereanking, and—where cardinality requires it—hierarchical or bounded beam selection. Report catalog recall and no-suitable-option detection before top-1 decision quality.

## v0.6 additional architecture arms

The model-family research matrix now explicitly includes independent option scoring, pointer/option-conditioned scoring and listwise candidate interaction. Causal/bidirectional and dense/sparse-MoE are optional follow-on hypotheses only. Report state-encode/prefill, per-question and per-candidate incremental costs separately. Candidate ordering and candidate-set composition are experimental factors, not nuisance variables to discard.

## v0.7 Kev-style executable reference arm

Add a concrete **Kev-style reference arm** to the bakeoff after the first eligible coding corpus exists: small pretrained causal backbone, lightweight adapter, exact sibling-question isolation through a branch mask, listwise/pointer readout over runtime candidates, no autoregressive answer decoding. This arm is intentionally small enough to train repeatedly while we study data/representation/transfer.

Compare it against the existing generative, direct-logit, sequence-scoring, independent option-scoring and other listwise arms under the same canonical decision encoding and candidate compiler.

Required ablations include: backbone size; frozen versus adapted backbone; pointer/listwise head versus independent scoring; question packing versus separate inference; canonical-order only versus shuffled-order training; permutation-consistency loss; distractor/absence augmentation; and state-observation variants. The purpose is to identify the bottleneck, not crown a preselected architecture.

A model that matches a reference backend on familiar tasks but collapses on repository/task-family transfer does not pass the Cortex model gate, even if latency is excellent.

---

<a id="doc-build-reflex-kev-pilot-md"></a>

## build/REFLEX_KEV_PILOT.md

# Axon Reflex — Kev-style reference pilot

## Purpose

Build the first small learned Axon Reflex backend as a **reference experiment**, not as the production default. The target is to reproduce the useful mechanism demonstrated by `jaredpalmer/kev` on Axon/MiCode coding decisions, then determine whether the bottleneck is model architecture, software-state representation, training data, candidate construction or transfer.

## Entry conditions

- CX-05 Reflex ABI and conformance fixtures exist.
- B21 decision corpus has independently eligible labels and repository-aware lineage.
- B58 Reflex Runtime can supply immutable state/candidate manifests.
- B63 has frozen `train`, `calibration`, `development`, `locked_test` and `transfer` partitions.
- One canonical decision encoder is pinned for training/evaluation/serving/replay.

## Reference architecture

Use a small pretrained causal backbone as the initial arm, with:

1. shared state prefix;
2. isolated question branches under a block-causal mask;
3. branch-local position policy;
4. runtime candidate list visible to the decision token;
5. pointer/listwise readout from the decision representation to candidate representations;
6. softmax distribution per typed question;
7. no autoregressive answer generation.

Start with lightweight adaptation (for example LoRA or an equivalent bounded adapter) plus a small readout head. Exact backbone, size and adaptation method are experiment parameters, not architecture invariants.

## Coding decision families

Initial families should be independently checkable and naturally finite:

- operation selection (`INSPECT`, `SEARCH`, `EDIT`, `CHECK`, `ESCALATE`, `DONE`, `BLOCKED`);
- symbol/test/scope target selection;
- check-selection / information-gain proxy;
- failure localization;
- `OBSERVE_MORE` / candidate-absence detection;
- escalation to THINK.

Do not begin with open-ended patch generation; that remains `GENERATE`.

## Training controls

Compare at least:

- canonical order only;
- option-order shuffle augmentation;
- permutation-consistency objective;
- distractor augmentation;
- typed absence/control augmentation;
- pointer/listwise head versus a simpler independent scorer;
- one or more backbone sizes if the first learning curve justifies it.

For ordered `Score`-like tasks, compare ordinary cross-entropy with an order-aware proper-scoring term. Keep post-hoc calibration separate from model training.

## Evaluation hierarchy

Report separately:

1. in-repository development;
2. unseen repositories in the same language/ecosystem;
3. unseen repository families and bug/task families;
4. cross-language transfer where the observation/candidate schema is still meaningful;
5. selective quality/coverage after routing/abstention;
6. task-level verified utility and total compute/cost.

Do not use the locked test for model selection. Calibration is fit on the calibration split only. Transfer data does not silently become training data without a new suite version.

## Mechanism/conformance tests

- packed versus separate questions;
- sibling-question isolation;
- boundary/delimiter forgery;
- candidate-order permutations;
- irrelevant-option insertion;
- missing-candidate / `NONE` / `OBSERVE_MORE` behavior;
- effective-input truncation/omission receipt;
- state-shuffle and wrong-state controls.

## Stop / pivot rules

Stop or pivot the architecture when one of these holds:

- simple baselines match the learned model under the same transfer/task budget;
- familiar-split gains do not transfer after representation/data improvements;
- order sensitivity remains above the registered action-risk envelope;
- the observer/candidate compiler, rather than model capacity, dominates errors;
- inference economics do not beat the incumbent after fallback cost.

A negative result is useful: it localizes the next research target.

## Promotion boundary

A successful pilot remains a research artifact until CX-11 independent admission passes. It cannot change authority, waive verification, access the locked test during tuning or activate itself.

---

<a id="doc-build-reflex-decision-corpus-md"></a>

## build/REFLEX_DECISION_CORPUS.md

# Axon Reflex decision corpus and evaluation construction

## Purpose

Build the empirical substrate for Axon Reflex before custom architecture work. The corpus contains **typed bounded judgments with independently checkable outcomes**, not generic chat transcripts and not raw replay logs copied wholesale into training.

## Unit of data

```text
DecisionExample {
  task_family
  episode_ref
  observation/state digest + eligible state projection
  typed question schema
  runtime-generated candidate set
  selected/teacher answer, if any
  verified outcome label or label status
  backend/model provenance
  split/group lineage
}
```

Keep `VerifiedOutcome`, `HumanReviewed`, `TeacherOnly`, `Weak`, `Delayed`, and `Unknown` labels distinct. Teacher agreement is reported separately from real task outcomes.

## Grouped split policy

Prevent leakage by grouping related examples before splitting. At minimum group by repository/task family, bug seed/template, code lineage, mutation family, and near-duplicate semantic structure when known. Derived variants from one underlying task stay in the same split.

Keep protected final-audit families inaccessible to the builder/model-training process. Similarity checks supplement provenance grouping; they do not replace it.

## Decision families to collect first

Prioritize judgments already present in the safe coding loop:

- operation selection: inspect/search/edit/check/escalate/done/blocked;
- target selection from dynamic symbols/tests/scopes;
- whether more observation is required;
- whether a candidate is stale/inapplicable;
- which check is likely to be informative;
- failure localization across observer/reflex/world-model/planner/generator/executor/verifier;
- decomposition signals such as capability expansion, trusted-code touch, external-effect change, test weakening, unchecked input, and invariant impact.

Each family needs a downstream use, verifier/reviewed label source, and baseline.

## Error-attribution dataset

Move attribution earlier because controlled self-improvement depends on localizing errors. For failed episodes capture candidate responsibility across `OBSERVER`, `RETRIEVAL`, `REFLEX`, `WORLD_MODEL`, `PLANNER`, `GENERATOR`, `EXECUTOR`, `VERIFIER`, `MULTI_CAUSAL`, or `UNKNOWN`.

Attribution remains a hypothesis unless substitution/intervention evidence supports it. Do not globally retrain all pillars because one classifier emits a confident cause.

## Question-decomposition dataset

Store both monolithic and decomposed formulations when System Two or humans discover a useful decomposition.

```text
monolithic: "is this patch safe?"

decomposed signals:
  expands capabilities?
  touches trusted code?
  changes external effects?
  weakens a test?
  introduces unchecked input?
  alters an invariant?
```

The deterministic composition rule is a versioned artifact. Evaluate whether decomposition improves verified downstream decisions, calibration, latency, cost, or transfer. Successful decompositions become candidates for crystallization into Reflex templates/rules.

## Corpus quality controls

For every corpus version record source/episode lineage and redaction version, label provenance/revision, candidate-construction policy, task-family/group split manifest, class/candidate-cardinality distribution, duplicate/near-duplicate audit, state/option length distributions, OOD/protected families, revoked examples/reasons, and eligible uses: training, calibration, development evaluation, or protected evaluation.

Raw Axon replay journals may contain secrets and are **not** automatically eligible training data. Export only purpose-scoped redacted projections through CX-10 lineage rules.

## Learning-curve protocol

Train/evaluate at multiple verified-data sizes. Compare simple priors/rules, lightweight classifier/ranker, direct-logit open model, sequence scorer, learned option-conditioned head, and generative strong-model adapter.

Report grouped held-out curves, calibration, coverage, latency/cost, destructive state controls, and real downstream task impact. The purpose is to discover the data/architecture regime, not to prove that a custom model must exist.

## v0.3 labels, candidate absence and behavior policy

Corpus rows distinguish candidate recall from option selection. Include examples where no supplied candidate is suitable, where further observation would reveal a candidate, and where a suitable target exists but is not authorized.

Label targets are explicit: semantic class, acceptable action set, observed action outcome, comparative action value, or task completion. Multiple acceptable next actions are representable. An unchosen candidate is not automatically a negative. Alternative action outcomes are `Unknown` unless independently evaluated or supported by a declared estimator.

Record the behavior-policy version and, when known, action-selection probability separately from model correctness/confidence. Group/split by repository/task/bug lineage to prevent near-duplicate leakage.

## v0.7 coding transfer and Kev-style training records

The corpus now maintains five explicit partitions for Reflex model research: `train`, `calibration`, `development`, `locked_test` and `transfer`. Split construction is repository-aware before example-level balancing. Derived commits, forks, generated siblings and near-duplicate bug families remain in one group unless a reviewed transfer experiment explicitly says otherwise.

Minimum transfer ladder for coding research:

1. unseen episodes in a known repository;
2. unseen repositories in a known ecosystem/language;
3. unseen repository families/frameworks;
4. unseen task families;
5. cross-language transfer where the candidate/observation schema remains meaningful.

Every record stores the canonical decision-encoding version and exact candidate order. Training/evaluation/serving/replay all invoke the same canonical encoder implementation where possible.

Add explicit negative candidate-set records: correct candidate absent; more observation required; escalation required; all ordinary candidates distractors. These examples use registered typed control candidates rather than marking the least-bad ordinary candidate as correct.

For permutation research, preserve paired examples with stable semantic candidate IDs and multiple randomized orders. The paired lineage allows augmentation and permutation-consistency objectives without leaking variants across train/calibration/dev/test/transfer partitions.

The first learned Axon Reflex pilot may start when a clean coding corpus is large enough to produce a measurable learning curve; data quality and transfer coverage, not a fixed example count, determine expansion.

---

<a id="doc-build-reflex-calibration-lab-md"></a>

## build/REFLEX_CALIBRATION_LAB.md

# Axon Reflex calibration and selective-routing lab

## Goal

Determine when a Reflex answer is reliable enough to replace or avoid more expensive cognition **without confusing model scores, uncertainty summaries, empirical correctness, authority, or proof**.

## Uncertainty provenance classes

Every probability-like value declares its origin. Suggested wire-level classes:

```text
NativeOptionLogit
SequenceLikelihood
GeneratedEstimate
EntropyDerived
EnsembleEstimate
CalibratedEmpirical
Unavailable
```

A value may carry more than one stage, for example native logits plus an empirical calibrator. Preserve the raw source and calibrated result separately.

`EntropyDerived` is uncertainty about a distribution, not automatically a probability that the selected answer is correct. `GeneratedEstimate` is model-authored content, not native scoring evidence. `CalibratedEmpirical` is valid only inside the artifact's declared model/schema/domain/candidate-construction envelope.

## Required metrics

At the question-family level report top-1/top-k accuracy where meaningful, NLL, Brier, ECE with binning details, reliability diagrams, selective risk vs coverage, error under abstention, subgroup/task-family/OOD performance, candidate-order sensitivity, candidate-count sensitivity, state-shuffle/wrong-state sensitivity, and latency/cost/memory including prefill/incremental timing.

At the routed system level report **Verified Utility at Coverage (VUC)** or an equivalent preregistered measure: how much workload Reflex handles at a specified independently verified error/quality envelope after fallback cost is charged.

Example question:

```text
Can Reflex handle >= X% of eligible bounded decisions while the independently
verified error rate remains <= Y and the end-to-end cost/latency improves versus
the locked baseline?
```

X and Y are task/policy-specific preregistered values, not universal constants.

## Calibration artifacts

Bind each artifact to model/weights/provider revision, tokenizer/quantization/runtime, prompt/question schema, candidate representation/construction policy, task/domain families, calibration dataset/grouping/split manifest, fitting method, validity/revocation triggers, metrics/intervals, and coverage/applicability envelope.

Any relevant model/schema/candidate-policy change invalidates or narrows the artifact until rechecked.

## Routing hierarchy

Authorization and mandatory verification come first. Within eligible actions, routing may use empirical selective risk, cost, latency, novelty, disagreement and progress. The router may choose Rule, Reflex, more Observe/Retrieve, Generate, Reason, Experiment/Predict, human review or Blocked.

No confidence value can create permission, waive a mandatory check, or certify DONE.

## Acceptance / pivot

Adopt a backend/router configuration only if it improves a preregistered task-level objective under minimum quality and coverage constraints. An all-abstain system fails coverage. A high-coverage but high-risk system fails quality. If no configuration improves economics, retain the incumbent and record the negative result.

## v0.3 coverage accounting

For every registered experiment:

- `Coverage = handled_without_escalation / eligible_decisions`.
- `SelectiveError = incorrect_handled / handled`; undefined when `handled = 0`.
- Eligibility rules and failure handling are frozen before scoring.
- Report protected task success, latency and total cost beside calibration/selective metrics.
- Any VUC-style aggregate publishes its formula/weights and cannot hide an all-abstain policy or dropped failures.

Calibration applicability binds the effective input/preprocessing receipt. Material truncation, projection, tokenizer change, candidate-policy change or question-fusion change requires revalidation.

## v0.6 calibration domain refinement

A calibration artifact is keyed by model/backend revision, effective-input policy, question family, candidate-generation policy **and candidate-order policy**. The primary learned object is a distribution; display confidence is a derived summary with formula provenance. Report NLL and Brier score alongside ECE, selective error/coverage, VUC and task-level outcomes. Reordering or materially changing candidate composition is a distribution shift unless revalidated.

## v0.7 transfer-aware calibration

Calibration and intelligence are distinct. Report calibration separately on in-distribution, repository-held-out and transfer partitions. A backend can be well calibrated while too inaccurate to be useful; low ECE never substitutes for task quality.

Define a **Coding Transfer Frontier** for each registered decision family: the furthest registered distribution shift at which the backend meets the preregistered selective-quality, coverage and compute/cost envelope. Report the whole frontier curve rather than one scalar whenever possible.

Temperature scaling or another post-hoc calibrator is fit only on the calibration partition. Development data selects configurations. The locked test remains untouched until promotion evaluation. Transfer partitions are not repurposed into training without minting a new corpus/suite version.

---

<a id="doc-build-reflex-conformance-md"></a>

## build/REFLEX_CONFORMANCE.md

# Axon Reflex conformance contract

**Status:** Draft normative build contract for v0.3.

## Purpose

A Reflex backend is conforming only when it preserves Axon question semantics, reports what input it actually evaluated, exposes its backend limits, and fails visibly when it cannot provide required evidence. Matching the shape of a TypeSafe/Jev response is not sufficient.

This contract covers provider-native System-One services, ordinary LLM adapters, direct-logit scorers, sequence scorers, and learned decision heads. It does **not** grant execution authority.

## Primitive semantics

V0 distinguishes the result families rather than coercing them to a universal `selected + confidence` record.

- **Choice<T>** returns a selected candidate or abstention plus, when available, a complete distribution over the supplied candidates. Candidate order is part of the request manifest even when a backend claims order invariance.
- **BinaryProbability** returns an explicit probability for the positive event when the backend provides one. It is not silently rounded into a Boolean and no separate confidence field is invented.
- **OrdinalDistribution** returns the full distribution over ordered rubric levels plus a derived expectation/mean only when that derivation is declared. A fractional mean must never be silently rounded to a category.
- **Rank<T>** is experimental and requires an explicit ranking/scoring contract before admission.

Open-ended code or text is `Generate`, not Reflex. Execution is `Act`, not Reflex.

## Batch contract

Requests may contain several typed questions. Results are keyed by stable `QuestionId`, never matched by array position.

```text
DecisionBatchResult {
  request_id,
  results: Map<QuestionId, QuestionResult>,
  attempt_records[],
  aggregate_usage,
  backend_manifest_ref,
  effective_input_receipt_ref
}

QuestionResult =
    ChoiceResult
  | BinaryProbabilityResult
  | OrdinalDistributionResult
  | Abstained
  | Failed
```

Duplicate question IDs refuse the request. Unknown returned IDs are errors. Missing required active-branch results block action construction. A failed unused speculative branch may be tolerated only when the versioned batch policy explicitly says so. Out-of-order backend completion is permitted because identity is explicit.

## Probability and score provenance

A backend reports only evidence it actually exposes. Allowed origins include:

- `ProviderReportedDistribution`
- `NativeOptionLogit`
- `SequenceLikelihood`
- `GeneratedEstimate`
- `EntropyDerived`
- `EnsembleEstimate`
- `Unavailable`

Empirical calibration is a separate artifact and never replaces raw origin. Entropy-derived confidence is not relabeled as correctness probability. Generated numeric confidence remains generated evidence until calibrated against protected outcomes.

## Backend feature negotiation

Every adapter supplies a versioned `BackendFeatureManifest` before dispatch:

```text
BackendFeatureManifest {
  adapter_version, backend_family, model_identity,
  supported_question_types[],
  supported_input_modalities[],
  maximum_questions?, maximum_candidates?,
  context_limits?, candidate_text_limits?,
  available_score_forms[],
  question_isolation_mode,
  preprocessing_policy_ref,
  cancellation_semantics, usage_reporting,
  immutable_model_revision_available
}
```

AIR validates a request against the effective backend manifest before inference. A fallback is a new dispatch and must independently pass feature validation. Provider limits are not copied into AIR as universal constants.

## Effective-input receipt

The trace records both what Cortex intended to provide and what the backend actually evaluated.

```text
EffectiveInputReceipt {
  submitted_observation_digest,
  effective_input_digest,
  preprocessing_manifest,
  tokenizer_revision?,
  omission_or_truncation_report[],
  effective_question_digests,
  effective_candidate_digests
}
```

Silent truncation is forbidden. When evidence is omitted because of context or candidate limits, the adapter must refuse or produce an explicit authorized projection receipt. Calibration established on complete inputs is inapplicable to silently altered inputs.

## Trusted prompt-role boundary

Only the trusted adapter constructs privileged model instructions. Repository text, logs, retrieved documents, recorded conversations, candidate descriptions, and source comments remain untrusted data even if they contain strings such as `system`, `assistant`, tool calls, or prompt delimiters.

An adapter that accepts chat-shaped state must preserve this boundary rather than promoting untrusted role labels to privileged messages.

## Question isolation and fusion

Conformance includes tests for:

1. `Q` alone;
2. `Q` plus an unrelated question;
3. `Q` plus a contradictory/adversarial unused question;
4. reordered questions;
5. different speculative branches;
6. different cache/prefix modes.

A backend may claim exact question isolation only for a pinned mode that demonstrates it. Otherwise the property is measured empirically. Combining two model calls into one joint prompt is a model/policy change unless the backend can demonstrate that the declared computation is preserved.

## Dynamic candidate conformance

Tests cover empty sets, singleton sets, many candidates, multi-token candidates, Unicode, prefix-sharing options, renamed opaque IDs, reordered candidates, duplicated semantic descriptions, and a correct option near the backend limit.

The adapter must distinguish:

- no suitable supplied option;
- more observation/candidate expansion may reveal an option;
- a suitable option exists but is unauthorized;
- inference failed or was unsupported.

## Retry, normalization and usage accounting

Every backend attempt is represented in `attempt_records`, including corrective retries and transient retries. Probability repair/normalization, candidate remapping, truncation, and fallback transformations are explicit transformations with versions. They consume the same parent budget.

Failures stay in evaluation denominators. An adapter cannot improve apparent reliability by dropping malformed/failed calls from results.

## Required conformance fixtures

The implementation must ship recorded fixtures for at least:

- TypeSafe-compatible Choice/Noul/Score mappings;
- fractional ordinal expectation with distinct underlying distributions;
- provider result without logits;
- missing active-branch result;
- failed unused speculative result;
- duplicate and unknown question IDs;
- candidate limit exceeded;
- decisive evidence beyond context limit;
- fake `system` messages inside repository text;
- question-interference/adversarial batch;
- multi-token candidate scoring;
- normalization repair plus retry budget;
- immutable and non-immutable model identity cases.

## Admission rule

A backend can join the bakeoff only after these fixtures pass for the capabilities it claims. Unsupported features are recorded as unsupported, not simulated. Conformance does not imply calibration, task quality, safety, or authority.

## v0.6 shared-state, isolation and ordering fixtures

Add fixtures for `StateHandle` identity/expiry/tenant isolation; Q1-alone versus unrelated/contradictory/adversarial/100-sibling batches; candidate order permutations; derived confidence summaries versus primary distributions; and backend claims about actual shared-state reuse. A backend may conform without reusable KV/state, but it must not claim the optimization when it only repeats full inference.

---

<a id="doc-build-reflex-runtime-md"></a>

## build/REFLEX_RUNTIME.md

# Reflex Runtime — shared-state execution and branch scheduler

**Status:** Draft build guide. This document does not claim a Reflex runtime is implemented.

## Purpose

Separate the long-lived Axon Reflex **runtime** from any particular Reflex **model**. The runtime evaluates many typed decision branches over one immutable software/world observation while preserving AIR dependency semantics, provenance, calibration domain, budgets and authority separation.

## Target architecture

```text
Software/World Observation
        |
        v
EffectiveInputReceipt
        |
        v
State Encoder / Prefix Cache
        |
        v
StateHandle
   |      |      |
   v      v      v
 Q1     Q2      Q3
indep   cond.   answer-dependent -> next stage
   |      |
   +------+
      |
DecisionBatchResult
      |
branch/joint validation
      |
Action proposal (still untrusted)
```

## Runtime responsibilities

- Canonicalize and authorize the observation projection before model dispatch.
- Bind `StateHandle` to state/model/tokenizer/adapter/preprocessing/tenant/expiry.
- Schedule `Independent`, `ConditionallyRelevant`, and `AnswerDependent` questions correctly.
- Pack dynamic candidates with deterministic construction and order manifests.
- Preserve opaque candidate IDs independently of human labels.
- Track prefill/state-encode, incremental-question, candidate and total latency/cost/memory separately.
- Enforce cancellation and parent budgets across all speculative branches and retries.
- Map provider responses by `QuestionId`; partial failures follow explicit batch policy.
- Never let speculative results directly invoke tools.
- Preserve probability origin and derived-summary formulas.
- Isolate mutable KV/state caches across tenants/principals unless an explicit shared-cache policy is reviewed.

## Model/runtime interface

Conceptual API; exact repository API is chosen after B00/B01 intake:

```text
encode_state(StateInput, BackendFeatureManifest) -> StateHandle | EmulatedStateHandle
decide_batch(StateHandle, QuestionBatch) -> DecisionBatchResult
release_state(StateHandle)
```

An adapter may implement `encode_state + decide_batch` in one remote request. The semantic receipt remains the same so benchmarks can distinguish actual state reuse from API-shaped batching.

## Candidate-order contract

`CandidateManifest` records:

- candidate IDs and descriptions;
- generation/retrieval policy and version;
- canonical order rule and `order_digest`;
- any randomized/permutation seed;
- truncation/filtering/authorization outcomes.

Calibration is scoped to this construction policy. A changed ordering policy requires revalidation; it cannot silently reuse the previous calibration artifact.

## Question isolation conformance

For backends that claim isolated branches, run Q1 under:

- Q1 alone;
- Q1 + unrelated Q2;
- Q1 + contradictory Q2;
- Q1 + adversarial unused branch;
- Q1 + 100 irrelevant questions;
- reordered batch and equivalent cache state.

Use exact equality only for deterministic backends where that is promised. Otherwise preregister a statistical/tolerance contract and report drift rather than hiding it.

## Performance matrix

Measure separately:

1. state encoding/prefill;
2. incremental question cost;
3. incremental candidate/option cost;
4. branch scheduler overhead;
5. memory/KV residency;
6. cancellation waste;
7. end-to-end verified task cost/latency.

Compare serial calls, ordinary concurrent calls, shared-prefix/state reuse and specialized listwise models. 'One API request' is not reported as 'one forward pass' without backend evidence.

## Build sequence

1. Mock state handle and deterministic branch scheduler.
2. Emulated handle over existing generative adapter.
3. Direct-logit/sequence adapters with exact receipts.
4. Shared-prefix backend where supported.
5. Isolation/order/performance conformance suite.
6. AIR compiler scheduling integration.
7. Optional listwise learned model research only after the workload/bakeoff identifies value.

## Stop conditions

Stop/pivot if state reuse creates negligible end-to-end benefit, if question isolation cannot be preserved for a backend, or if memory/cancellation cost outweighs reduced prefill. Retain the stable ABI and fall back to serial/concurrent execution.

## v0.7 canonical decision encoding boundary

The Reflex Runtime owns one versioned canonical decision encoder used by model training exports, evaluation, live serving and semantic replay. Backend-specific tokenization may occur after this boundary, but any semantic transformation—state flattening, option description rewrite, omission/truncation, candidate reordering, delimiter policy—must be recorded in the effective-input receipt and calibration domain.

For packed isolated-question backends, branch masks and position policies are backend implementation details but are conformance-tested against separate inference. For cross-request reuse, `StateHandle` remains the public abstraction even when a backend implements only per-request packing and cannot persist KV/state across requests.

---

<a id="doc-build-dependency-adoption-md"></a>

## build/DEPENDENCY_ADOPTION.md

# Cortex dependency, model and dataset adoption

**Status:** Draft normative build contract for v0.3.

## Purpose

System-One/Jev ecosystem repositories are implementation leads, not pre-approved production dependencies. Cortex must be able to reproduce what it adopted, identify transitive artifacts, understand license/security consequences, and distinguish upstream benchmark claims from Axon evidence.

## Adoption manifest

Every external repository, package, model, tokenizer, dataset, evaluator or hosted endpoint used beyond disposable exploration gets an `AdoptionManifest`.

```text
AdoptionManifest {
  artifact_class,
  source_url, source_commit_or_version,
  integrity_digest,
  license_and_notice_refs[],
  dependency_lock_digest?,
  transitive_model_refs[], tokenizer_refs[], encoder_refs[],
  dataset_refs[], preprocessing_refs[], evaluator_refs[],
  security_profile, network_endpoints[], credential_requirements[],
  local_reproduction_commands[], local_reproduction_receipts[],
  approved_use: research | benchmark | dev | protected_runtime,
  owner, review_date, supersedes?
}
```

Pin the artifacts that determine behavior, not only the top-level repo. If a scorer loads an external encoder by name, that encoder revision is part of the effective model identity. If a dataset or model has separate terms, record them separately.

## Intake sequence

1. Pin source commit/version and archive integrity.
2. Read license/notice and terms for code, model weights, datasets and hosted services separately.
3. Enumerate transitive executable/model artifacts and network dependencies.
4. Reproduce minimal upstream example in an isolated environment.
5. Re-run a Cortex-owned conformance fixture rather than accepting README output.
6. Record security defaults; insecure example defaults do not become protected deployment defaults.
7. Add the artifact to SOURCES with claim boundaries.
8. Permit wider use only through owner review and, where appropriate, admission gates.

## Benchmark comparability review

Imported results must state whether systems received the same:

- state/evidence;
- candidate set and candidate ordering;
- output obligation;
- reasoning/generation budget;
- retries/fallbacks;
- tools and permissions;
- cache/warmup conditions;
- completion verifier;
- failure accounting.

If these differ, the result may still motivate an experiment but is not presented as a direct performance comparison. Cached responses are labeled separately from live inference. Injected-failure benchmarks are labeled separately from naturally occurring failures.

## Hosted service profile

A sample endpoint being unauthenticated or broadly network-accessible is not an Axon deployment recommendation. Protected-runtime adoption requires authenticated principals, transport security where applicable, tenant/cache isolation, budgets, observability, revocation, and the host policies required by CX-13.

## SDK transformations

Official/community SDKs may retry, normalize probabilities, coerce outputs, truncate prompts, remap candidates, or select fallback models. Cortex wraps these transformations in explicit adapter events and budget accounting. Convenience behavior that cannot be observed or disabled is evaluated as part of that backend, not treated as invisible plumbing.

## Update policy

Dependency/model updates create a new effective identity and trigger the relevant conformance, calibration and protected-task checks. Floating aliases are allowed only for explicitly non-reproducible research profiles; protected evidence records the effective resolved model identity returned by the provider when available.

## Initial ecosystem adoption categories

The following categories are suitable for research intake, subject to repository-level verification at implementation time:

- official TypeSafe SDKs and System-One adapter for interface semantics;
- OpenJev/openjev-sglang/MLX/sequence-scoring implementations for bounded-option inference techniques;
- learned option scorers for variable candidate-set research;
- Jev benchmarks/calibration projects for fixture ideas;
- browser/computer-use agents for dynamic action-space patterns;
- guard/triage/search/tree projects for decomposition and hierarchical candidate search ideas.

This list conveys research relevance, not license clearance, security approval or benchmark reproduction.

## Acceptance cases

- Changing a transitive encoder revision changes effective identity.
- An unpinned/floating model used in protected evidence is refused or explicitly marked non-reproducible.
- A probability-normalizing SDK records the pre/post transformation.
- Corrective retries consume the same parent budget and remain visible.
- An unauthenticated demo endpoint fails the protected-runtime host profile.
- A benchmark with easier candidate information is accepted as an implementation lead but rejected as a direct quality comparison.

---

<a id="doc-build-acceptance-gates-md"></a>

## build/ACCEPTANCE_GATES.md

# Acceptance-gate registry and evidence protocol

This registry lists proposed product tests. None was implemented or executed by preparing this package. The package validator checks only document consistency. A gate filename, test skeleton, mock response or favorable assistant review is not product evidence.

## Required execution record

Record gate ID/version; requirement; candidate/incumbent commit and artifact digests; task/data/split manifests; runtime/model/schema/policy/calibration versions; hardware/host/sandbox; exact invocation; start/end; actual return status; stdout/stderr/report artifact digests; measured metrics and intervals; exclusions; result status; and verifying identity. Sensitive logs use protected references and redacted display.

Status is PASS, FAIL, INCONCLUSIVE, SKIPPED or UNAVAILABLE. NOT_RUN is a planning state only. Required gates need real PASS evidence for the exact artifact and environment. Safety/effect gates test actual permitted and denied behavior; mocks alone cannot establish host confinement.

## Implementing a gate

First place a negative fixture in the real repository and prove it catches the pre-change failure, or demonstrate the expected refusal in a newly isolated component. Add the corresponding positive fixture. Register an executable gate through Axon's existing test/evidence system after inspecting it. Make it fail nonzero on a violated assertion, unavailable required tool or incomplete required check. Preserve explicit skip states only for non-required development coverage.

Name commands in spec evidence only after those commands exist. A proposed `cortex` CLI or gate runner in these documents is not yet an executable interface. Do not generate dummy scripts that print PASS.

## Milestone aggregation

M0 requires the reconciled capability/engine map and reviewed policies plus runnable reset/baseline infrastructure. M1 additionally requires actual registry, transaction, host, verifier and replay evidence. M2 adds decision/routing measurements. Later milestones add their spec gates without weakening earlier safety gates. A failed optional research experiment is reported as a result, not concealed, and does not force adoption.

## Gate inventory

See linked specs for exact inputs, negative cases and scope. IDs below are package-local; import them into the real evidence registry deliberately.

| Gate | Spec | Required behavior |
|---|---|---|
| G00-contract | [CX-00](specs/CX-00-system-contract.md) | a fixture with a complete profile parses; each missing mandatory field refuses. A worker claiming another principal or fabricating a completion signature cannot execute/close. |
| G00-mutation | [CX-00](specs/CX-00-system-contract.md) | approve artifact A, replace one byte to produce B, and attempt execution. B is denied without a new approval. Policy-version mismatch also refuses. |
| G00-missing-gate | [CX-00](specs/CX-00-system-contract.md) | simulate PASS, FAIL, SKIPPED, UNKNOWN and unavailable mandatory verifier. Only actual passing evidence for all requirements permits admission. |
| G00-authority | [CX-00](specs/CX-00-system-contract.md) | a learner attempts to edit gate code, evaluation data, policy or signer credentials. The host denies access and records the denied attempt. |
| G01-reset | [CX-01](specs/CX-01-evaluation.md) | reset a fixture twice and compare canonical inputs; vary an undeclared environment input and require an invalid-fixture diagnosis. |
| G01-fairness | [CX-01](specs/CX-01-evaluation.md) | automatically compare manifests and refuse a head-to-head comparison with changed hidden budget, tool access, primer or completion contract. |
| G01-leakage | [CX-01](specs/CX-01-evaluation.md) | duplicate/mutated family-related fixtures across protected splits are detected by provenance plus reviewed similarity rules; final-audit paths are inaccessible to workers. |
| G01-evidence | [CX-01](specs/CX-01-evaluation.md) | a tiny inconclusive result cannot pass non-inferiority; a fully abstaining router cannot pass required coverage; a stale candidate digest cannot reuse a passing report. |
| G01-ablation | [CX-01](specs/CX-01-evaluation.md) | run the first vertical-slice task set with the strong-model and AIR controls before adding learned components. Report all attempts, including failed and canceled tasks. |
| G02-canonical | [CX-02](specs/CX-02-world-observation.md) | identical snapshot and observer inputs produce equal canonical observation bytes excluding declared volatile envelope fields; round-trip loses no fact provenance. |
| G02-partial | [CX-02](specs/CX-02-world-observation.md) | deliberately break syntax/type resolution. Observer still returns a useful partial state and never invents a successfully inferred type. |
| G02-stale | [CX-02](specs/CX-02-world-observation.md) | mutate tracked, untracked and dependency inputs separately. Relevant cached observations invalidate and old object references cannot silently resolve to new targets. |
| G02-recall | [CX-02](specs/CX-02-world-observation.md) | a fixture whose fix is outside the initial neighborhood can request bounded expansion and expose the correct target; report candidate recall separately from solver success. |
| G02-injection | [CX-02](specs/CX-02-world-observation.md) | source comments and retrieved text containing authority instructions are stored as untrusted content and cannot change permissions or task contracts. |
| G03-forgery | [CX-03](specs/CX-03-capabilities-executor.md) | unknown, expired, wrong-principal and previous-snapshot grants all refuse without a side effect. |
| G03-payload | [CX-03](specs/CX-03-capabilities-executor.md) | a valid Edit grant paired with a traversal, symlink or policy-file patch refuses; a permitted ordinary patch succeeds. |
| G03-race | [CX-03](specs/CX-03-capabilities-executor.md) | mutate input between prepare and commit; exactly one compatible local transaction can commit and the stale one must reobserve. |
| G03-crash | [CX-03](specs/CX-03-capabilities-executor.md) | inject crashes at every durable boundary. Recovery never blindly duplicates an external effect and identifies an unknown outcome. |
| G03-budget | [CX-03](specs/CX-03-capabilities-executor.md) | nested child actions and speculative requests cannot exceed the parent's reserved aggregate budget; concurrent debits are atomic. |
| G03-done | [CX-03](specs/CX-03-capabilities-executor.md) | a DONE claim with failing hidden checks cannot close the task, regardless of confidence or agent explanation. |
| G04-schema | [CX-04](specs/CX-04-air-runtime.md) | unknown nodes, bad types, missing branch outputs, cycles and unbounded loops fail validation with stable symbolic categories. |
| G04-effect | [CX-04](specs/CX-04-air-runtime.md) | a Rule node attempting an AI or filesystem effect and a child node requesting widened authority both refuse. |
| G04-replay | [CX-04](specs/CX-04-air-runtime.md) | a pinned simple graph executed through recorded host/model replies reproduces output and action selection without new effects. |
| G04-scheduler | [CX-04](specs/CX-04-air-runtime.md) | deterministic serial and permitted parallel modes agree on the semantic result; reordered effectful nodes are rejected. |
| G04-fallback | [CX-04](specs/CX-04-air-runtime.md) | a provider outage produces an explicit failure/authorized fallback event, never an unnoticed model switch or a fabricated answer. |
| G05-schema | [CX-05](specs/CX-05-reflex-inference.md) | all backend families conform to one dynamic-candidate ABI; malformed/non-finite/outside-candidate/wrong-snapshot results fail; label-only responses stay label-only; probability provenance cannot be forged. |
| G05-branch | [CX-05](specs/CX-05-reflex-inference.md) | a selected Check operation can only use CheckTarget; speculative EditTarget output never causes a write. Contradictory active fields cannot produce an action. |
| G05-isolation | [CX-05](specs/CX-05-reflex-inference.md) | correct state is compared with shuffled/empty/wrong/stale state, candidate reorder/rename, irrelevant context and adversarial options; state-insensitive shortcuts and sensitivity are reported; no false independence claim. |
| G05-tokenization | [CX-05](specs/CX-05-reflex-inference.md) | multi-token candidates are scored by a declared complete method; exact opaque candidate IDs round-trip independently of labels; candidate count/order/length sensitivity is measured. |
| G05-performance | [CX-05](specs/CX-05-reflex-inference.md) | compare generative/direct-logit/sequence/learned-head families plus serial/parallel/shared-state modes on the same protected corpus; publish prefill, incremental question/option, memory, cost, latency, selective quality and downstream task impact. |
| G06-calibration | [CX-06](specs/CX-06-routing-calibration.md) | fitting uses only grouped permitted calibration data; raw score origin and calibrated result remain separate; NLL/Brier/reliability/risk-coverage/VUC are reported on held-out task families; schema/model/candidate-policy change invalidates applicability. |
| G06-risk | [CX-06](specs/CX-06-routing-calibration.md) | an unauthorized/irreversible action never becomes allowed solely because the selected answer has probability 1.0. |
| G06-coverage | [CX-06](specs/CX-06-routing-calibration.md) | an all-abstain router fails the coverage target; a high-coverage router with unacceptable selective risk also fails. |
| G06-shift | [CX-06](specs/CX-06-routing-calibration.md) | shift/OOD fixtures cause the specified escalation or inapplicability behavior; report remaining failures instead of claiming perfect detection. |
| G06-budget | [CX-06](specs/CX-06-routing-calibration.md) | repeated escalation, retry and fallback cannot exceed parent limits; the termination reason is observable. |
| G07-target | [CX-07](specs/CX-07-predictive-world.md) | every prediction binds to its exact patch/action and environment. Mutating the patch or toolchain invalidates the forecast. |
| G07-holdout | [CX-07](specs/CX-07-predictive-world.md) | beat or match preregistered simple prediction baselines on protected outcomes; report calibration, interval coverage, error and per-domain failure. |
| G07-planning | [CX-07](specs/CX-07-predictive-world.md) | compare the same planner with no model, simple model and candidate model on held-out tasks; charge prediction cost. |
| G07-exploitation | [CX-07](specs/CX-07-predictive-world.md) | adversarial candidate search attempts to find actions the simulator likes but real checks reject. Report gaps; no such simulated success may certify completion. |
| G07-unknown | [CX-07](specs/CX-07-predictive-world.md) | missing features, unsupported action/horizon and canceled experiments return explicit inapplicability or censored labels, not invented confident forecasts. |
| G08-hypothesis | [CX-08](specs/CX-08-planning-experiments.md) | two hypotheses with distinct registered predictions cause selection of a separating permitted check; unsupported labels remain uncertain. |
| G08-controls | [CX-08](specs/CX-08-planning-experiments.md) | replay/reset and matched control execution preserve declared controlled variables; a confounded fixture does not produce an unqualified causal conclusion. |
| G08-authority | [CX-08](specs/CX-08-planning-experiments.md) | a high-information experiment requiring denied execution/network access is blocked rather than run. |
| G08-progress | [CX-08](specs/CX-08-planning-experiments.md) | repeated identical unsuccessful actions trigger bounded replan/escalation/stop; no infinite reasoning loop or budget reset occurs. |
| G08-value | [CX-08](specs/CX-08-planning-experiments.md) | compare against fixed check order, random permitted probing and strong-model-only reasoning on the same held-out tasks and budgets. |
| G09-lineage | [CX-09](specs/CX-09-representation-abstraction.md) | every derived node/claim maps to source evidence or an explicit hypothesis; unsupported invented facts cannot masquerade as observations. |
| G09-counterexample | [CX-09](specs/CX-09-representation-abstraction.md) | a discovered concept is tested on known counterexamples and out-of-domain cases, with appropriate abstention or failure. |
| G09-transfer | [CX-09](specs/CX-09-representation-abstraction.md) | preregistered novel-family tasks show the claimed gain against representation and compute-matched baselines; inconclusive results stay research artifacts. |
| G09-compression | [CX-09](specs/CX-09-representation-abstraction.md) | a shorter model that violates fixed held-out fit/safety constraints fails, even if its training fit or description length improves. |
| G09-ablation | [CX-09](specs/CX-09-representation-abstraction.md) | removing the concept or scrambling its assignments measurably tests whether it—not additional context or compute—caused the gain. |
| G10-replay | [CX-10](specs/CX-10-replay-learning-data.md) | recorded task execution repeats without filesystem/network/model side effects; changed arguments cause visible divergence. |
| G10-crash | [CX-10](specs/CX-10-replay-learning-data.md) | an episode interrupted between action and receipt remains OutcomeUnknown until reconciled; no fictitious success label enters training. |
| G10-secrets | [CX-10](specs/CX-10-replay-learning-data.md) | seeded secrets in source, environment, prompts and errors remain unavailable in redacted review and learning export; raw-journal access is separately controlled. |
| G10-lineage | [CX-10](specs/CX-10-replay-learning-data.md) | every training row points to permitted evidence and split lineage; weak/simulated labels cannot be silently upgraded to real outcomes. |
| G10-attribution | [CX-10](specs/CX-10-replay-learning-data.md) | a deliberately multi-causal failure can retain multiple candidate causes/Unknown; a confirmed substitution updates the attribution with supporting evidence. |
| G11-independent | [CX-11](specs/CX-11-crystallization-admission.md) | a learner-written passing report without the expected admission provenance cannot promote; a policy/schema mismatch also refuses. |
| G11-scope | [CX-11](specs/CX-11-crystallization-admission.md) | a compiled rule works within its declared domain and reliably falls back outside it; a changed candidate schema revokes stale applicability. |
| G11-noninferiority | [CX-11](specs/CX-11-crystallization-admission.md) | evidence below precision/sample requirements yields INCONCLUSIVE; a fast but quality-regressing candidate fails. |
| G11-authority | [CX-11](specs/CX-11-crystallization-admission.md) | a tool/macro or compiler pass requesting additional effects fails even when task score improves. |
| G11-rollback | [CX-11](specs/CX-11-crystallization-admission.md) | inject a regression after activation in a reversible fixture; stop new use, restore the previous-good bundle and preserve/correct dependent state without repeating effects. |
| G12-contract | [CX-12](specs/CX-12-learning-meta.md) | every participating pillar has a complete lifecycle/metric/authority manifest; unsupported lifecycle operations are explicit. |
| G12-cause | [CX-12](specs/CX-12-learning-meta.md) | controlled component substitution attributes a known fault without falsely updating unrelated pillars; Unknown remains valid when ambiguous. |
| G12-meta | [CX-12](specs/CX-12-learning-meta.md) | attempts to lower admission thresholds, edit hidden data or self-sign a model through a meta job are denied. |
| G12-curriculum | [CX-12](specs/CX-12-learning-meta.md) | generator siblings cannot leak into final audit unnoticed; transfers are tested against templates and compute-matched controls. |
| G12-stability | [CX-12](specs/CX-12-learning-meta.md) | simultaneous conflicting updates are serialized or evaluated together; an episode never silently mixes component versions. |
| G13-escape | [CX-13](specs/CX-13-os-runtime.md) | adversarial build/test fixtures try host reads/writes, unapproved subprocesses, network access, environment secrets and protected evaluator paths. Allowed operations succeed; denied operations produce no unauthorized realized effect. |
| G13-kill | [CX-13](specs/CX-13-os-runtime.md) | stuck process trees and slow inference are canceled under the declared bound; no further dispatch or resumable self-clear occurs. |
| G13-quota | [CX-13](specs/CX-13-os-runtime.md) | concurrent child jobs cannot oversubscribe carved quotas or evade them through restart. |
| G13-recovery | [CX-13](specs/CX-13-os-runtime.md) | parent/worker crashes at action boundaries produce safe reconciliation and no blind external retry. |
| G13-tier | [CX-13](specs/CX-13-os-runtime.md) | unsupported/native profiles cannot inherit interpreter-only guarantees; simulated attestation cannot satisfy a hardware-required profile. |
| G14-pilot | [CX-14](specs/CX-14-parallel-model-research.md) | a small approved dataset produces a reproducible learning curve against the strongest relevant simple baseline; invalid labels or contaminated splits block the experiment. |
| G14-quality | [CX-14](specs/CX-14-parallel-model-research.md) | task and calibration non-inferiority under CX-01 policy, with per-family/OOD results and abstention coverage, passes before cost savings justify release. |
| G14-parallel | [CX-14](specs/CX-14-parallel-model-research.md) | real context/cardinality/load benchmarks show whether specialized parallel inference helps; semantic and active-branch conformance match CX-05. |
| G14-version | [CX-14](specs/CX-14-parallel-model-research.md) | quantized/retrained/tokenizer-changed artifacts cannot reuse stale calibration or approvals. |
| G14-release | [CX-14](specs/CX-14-parallel-model-research.md) | a successful research artifact still passes independent CX-11 admission and rollback rehearsal; a notebook metric cannot self-activate a model. |
| G15-types | [CX-15](specs/CX-15-language-compiler-proof.md) | a corpus exercising new type/ownership/refinement paths uses the authoritative type map; deliberately inconsistent lowering refuses rather than guessing. |
| G15-parity | [CX-15](specs/CX-15-language-compiler-proof.md) | supported interpreter/native cases agree on semantic output, effects, failures and recorded model behavior; unsupported cases refuse explicitly. |
| G15-docs | [CX-15](specs/CX-15-language-compiler-proof.md) | generated reference and documented surface update with the change; language primer tests measure model authoring/repair fluency without cherry-picking examples. |
| G15-proof | [CX-15](specs/CX-15-language-compiler-proof.md) | stale subject hashes, changed assumptions and invalid proofs cannot produce a valid receipt or bypass runtime fallback. |
| G15-optimization | [CX-15](specs/CX-15-language-compiler-proof.md) | a proposed fusion/specialization preserves budgets, authority and control dependencies, including adversarial and failure paths. |

## v0.3 Reflex conformance and adoption gates

**G05-batch-identity:** multi-question results are keyed by QuestionId; duplicate/unknown IDs fail; missing active-branch output blocks the action; unused speculative failure follows an explicit batch policy.

**G05-effective-input:** decisive evidence beyond a backend limit produces refusal or a visible authorized projection/truncation receipt; silent truncation fails.

**G05-prompt-boundary:** repository/log/replay text containing fake system/tool messages cannot become privileged adapter instructions or execution authority.

**G05-primitive-semantics:** Choice distributions, binary positive-event probabilities and ordinal distributions/expectations round-trip without silent rounding or invented confidence/logits.

**G05-candidate-absence:** no-suitable-option, need-more-observation, unauthorized-candidate and inference-unavailable paths remain distinct and produce the specified controller behavior.

**G10-multi-valid:** two independently validated useful next actions can coexist in learning data; the unchosen valid action is not auto-labeled negative.

**G10-behavior-policy:** behavior-policy version and selection probability, when known, are separate from Reflex correctness probability; unobserved alternatives remain unknown absent explicit evidence.

**G13-adoption:** a protected external backend has pinned source/transitive model/tokenizer/encoder identity, reviewed license/security profile and visible SDK transformations/retries; unauthenticated demo deployment fails protected admission.

**G01-comparability:** an imported benchmark with different candidates/evidence/output obligations may motivate an experiment but cannot be cited as a direct Cortex performance comparison without matched reproduction.

| G16-schema | [CX-16](specs/CX-16-micode-experience-bridge.md) | representative MiCode episode round-trips into Axon without semantic loss/invented defaults across denied/failed/aborted/success/unknown outcomes. |
| G16-authority | [CX-16](specs/CX-16-micode-experience-bridge.md) | foreign grants/risk ceilings/approvals cannot create local Axon authority or bypass a local capability check. |
| G16-lineage | [CX-16](specs/CX-16-micode-experience-bridge.md) | imported learning/evaluation rows retain source-system/repository/evaluator/data-use lineage and remain ineligible when required policy is absent. |
| G16-version | [CX-16](specs/CX-16-micode-experience-bridge.md) | schema/ontology/version mismatch requires registered migration or refusal; stale calibration/applicability is not reused. |
| G16-replay | [CX-16](specs/CX-16-micode-experience-bridge.md) | imported episode supports semantic replay/counterfactual comparison without repeating external effects; missing inputs are surfaced. |
| G17-provenance | [CX-17](specs/CX-17-external-repository-knowledge.md) | knowledge candidate enumerates exact sources/transforms/data-use constraints; missing lineage blocks promotion eligibility. |
| G17-counterexample | [CX-17](specs/CX-17-external-repository-knowledge.md) | contradicted/generalized candidate is narrowed/rejected; counterexample search result is recorded rather than omitted. |
| G17-reproduce | [CX-17](specs/CX-17-external-repository-knowledge.md) | one candidate is locally reproduced/benchmarked in a resettable Axon experiment, including failed reproduction when applicable. |
| G17-license | [CX-17](specs/CX-17-external-repository-knowledge.md) | disallowed source cannot enter eligible learning/derived promotion; restrictions propagate to dependents. |
| G17-heldout | [CX-17](specs/CX-17-external-repository-knowledge.md) | transfer/generality claim is evaluated on held-out repository/task families; inconclusive evidence stays non-promotable. |
| G18-ladder | [CX-18](specs/CX-18-capability-synthesis-native-promotion.md) | repeated evidence becomes a guarded Skill/Tool candidate with full lineage; frequency alone cannot advance a stage. |
| G18-authority | [CX-18](specs/CX-18-capability-synthesis-native-promotion.md) | synthesized capability that widens effects/scope/authority is rejected regardless of task score. |
| G18-equivalence | [CX-18](specs/CX-18-capability-synthesis-native-promotion.md) | deterministic/compiler specialization is validated over its declared domain and falls back outside it. |
| G18-deopt | [CX-18](specs/CX-18-capability-synthesis-native-promotion.md) | regression/applicability mismatch suspends new use and restores previous-good artifact while preserving lineage. |
| G18-native | [CX-18](specs/CX-18-capability-synthesis-native-promotion.md) | compiler/runtime promotion cannot activate without CX-15 parity/proof/invariant evidence; useful userland artifact may remain userland. |

## v0.5 intent-compiler gates

| Gate | Spec | Required behavior |
|---|---|---|
| G19-parse | [CX-19](specs/CX-19-intent-compiler.md) | representative human/system intents round-trip through Intent IR without collapsing constraints/preferences/unknowns/authority/evidence or losing provenance. |
| G19-ambiguity | [CX-19](specs/CX-19-intent-compiler.md) | materially ambiguous intent cannot enter consequential execution until explicitly resolved or approved as a disjunction; low model confidence never silently chooses a default. |
| G19-authority | [CX-19](specs/CX-19-intent-compiler.md) | lowering/replanning that widens scope/effects/authority beyond approved Intent IR is refused; narrowing remains valid. |
| G19-evidence | [CX-19](specs/CX-19-intent-compiler.md) | planner/reasoner cannot drop required acceptance evidence or redefine DONE after seeing outcomes. |
| G19-render | [CX-19](specs/CX-19-intent-compiler.md) | deterministic semantic rendering exposes objectives/constraints/authority/evidence/assumptions and stale approval cannot bind a modified IR. |
| G19-trace | [CX-19](specs/CX-19-intent-compiler.md) | consequential actions/evidence in a vertical episode trace to active Intent IR clauses, lowering record and exact approval; orphan actions fail. |

## v0.6 Reflex runtime and architecture gates

- `G05-state-handle` — state cache identity, expiry and isolation.
- `G05-question-isolation` — sibling-question noninterference claim under registered tolerance.
- `G05-order-domain` — candidate ordering is measured and bound to calibration.
- `G14-listwise` — listwise/option-conditioned architecture comparison under matched compute.
- `G14-training-score` — proper-scoring and task-level training evidence.
- `G15-dependency-types` — AIR scheduling classes are enforced.
- `G15-batch-semantics` — batching is distinguished from semantic prompt fusion.

| G14-transfer-frontier | [CX-14](specs/CX-14-parallel-model-research.md) | coding-generality claims require repository/task-family held-out selective quality/coverage/compute evidence; same-repo/random splits cannot pass. |
| G14-permutation-training | [CX-14](specs/CX-14-parallel-model-research.md) | invariance training reduces permutation sensitivity without hiding task/calibration regressions; order remains recorded in provenance. |
| G14-absent-candidate | [CX-14](specs/CX-14-parallel-model-research.md) | missing-target fixtures select only registered absence/control outcomes rather than a forced ordinary candidate. |
| G14-canonical-encoding | [CX-14](specs/CX-14-parallel-model-research.md) | training/evaluation/live inference/replay share one versioned semantic encoding or record an explicit separately calibrated transformation. |
| G14-locked-test | [CX-14](specs/CX-14-parallel-model-research.md) | normal training/model-selection workers cannot read the locked test; promoted-candidate evaluation records suite/model/code hashes. |

---

<a id="doc-build-experiment-register-md"></a>

## build/EXPERIMENT_REGISTER.md

# Initial experiment register

These are preregistration templates, not findings. Fill the task manifests, allowed data, hardware, budget, primary metric, non-inferiority margin and stopping rule before results are examined. All statuses: Not run.

| Experiment | Question and challenger | Control / key confound | Evidence required to continue |
|---|---|---|---|
| E01 — Structured loop | Does AIR/structured observation improve task completion economics? | Same strong model, tools, primer, task budget and hidden verifier without AIR. | Task-quality and end-to-end cost/latency results, not tool-call count alone. |
| E02 — Candidate catalog | Does bounded target selection reduce invalid actions without hiding necessary targets? | Wider authorized catalog plus retrieval-only baseline. | Candidate recall, invalid-action rate, scope-expansion success and full task outcomes. |
| E03 — Reflex backend bakeoff | Which backend family best serves each bounded decision workload under one frozen Reflex ABI? Generative adapter vs direct option logits vs sequence scorer vs learned decision head. | Same state projection, question schema, ordered candidate manifests, authority, task family, verifier and budget. | Verified task outcomes; grouped accuracy/NLL/Brier/risk-coverage; latency/cost/memory; raw-score provenance; destructive state/candidate controls. |
| E03A — Shared-state speculation | Do conditional parallel target questions and shared-state/prefix reuse help? | Sequential operation→target and ordinary parallel calls on the same backend. | State-prefill, incremental question/option timing; context/cardinality sweeps; discarded speculative compute; cross-field validity; task quality. |
| E03B — State dependence | Does the backend actually use the supplied state and dynamic candidate semantics? | Correct-state result versus shuffled/empty/wrong/stale state, candidate reorder/rename, irrelevant-state injection and adversarial option sets. | State-sensitivity report and candidate-recall/task outcome deltas; reject state-insensitive shortcuts. |
| E04 — Calibration/router | Does empirically calibrated selective execution beat always-Reason or raw-threshold routing? | Same observation/model budget; include all-abstain and always-act sanity checks; preserve raw score provenance. | Proper scoring, selective risk/coverage, subgroup/OOD behavior, Verified Utility at Coverage, and task economics. |
| E05 — Small Reflex pilot | Can grouped eligible labels support a cheaper option-conditioned specialist over runtime-generated candidate sets? | Teacher/strong model, direct-logit/sequence baselines, small untrained model and simple classifier/ranker. | Learning curves, destructive state controls, calibration/coverage and data costs; teacher agreement separated from real task outcomes. |
| E05A — Question decomposition | Does decomposing one broad judgment into narrower typed signals plus deterministic composition improve verified decisions? | Original monolithic question using the same state/model/backend and a compute-matched control. | Protected task outcomes, calibration, latency/cost, failure modes, transfer; preserve both formulations and the composition-rule artifact. |
| E06 — Prediction value | Do outcome predictions improve check selection? | Dependency heuristic, base rates and no model. | Held-out forecasts plus real planner outcomes after charging prediction cost. |
| E07 — Active probes | Does experiment selection resolve ambiguity efficiently? | Fixed checks, random permitted probe and deliberate reasoning only. | Matched reset/control, total checks/cost, diagnosis and completed tasks. |
| E08 — Guarded specialization | Can one recurring reasoning pattern become a tool/rule/policy? | Approved incumbent with the same authority and task set. | Applicability envelope, independent quality gate, authority non-expansion and fallback behavior. |
| E09 — Representation | Does a new representation help on a novel family? | Existing model, standard representation and scrambled/removed concept. | Transfer and few-shot curves, total compute, source lineage and counterexamples. |
| E10 — Learning portfolio | Does meta-allocation improve learning efficiency? | Fixed round-robin or manually preregistered experiment order. | Progress per real budget on a fixed protected audit suite; no metric/task rewriting. |
| E11 — Host portability | Does a new host preserve the supported contract? | Original supported host/profile. | Actual effect, recovery, kill, quota and replay gates—not only compilation. |
| E12 — Compiler lowering | Does native/syntax integration justify maintenance cost? | Existing library/interpreter path. | Authoring fluency, semantic/effect parity, refusal cases, build/runtime costs and docs drift gates. |

## Stop or pivot conditions

Do not build a custom architecture merely because an adapter experiment underperforms. Check labels, grouped-split leakage, observation completeness, state sensitivity, candidate recall, option/tokenization semantics, calibration and baseline strength first. Stop a branch when its preregistered information budget is consumed or measured benefit is absent; retain the simple baseline and document the negative result.

A representation or world-model experiment with inconclusive transfer remains research. A failed safety gate cannot be compensated by task-score or latency improvements. Optional model research does not prevent shipping a useful hosted runtime.

## v0.4 required external-experience experiments

- **Bridge conformance:** MiCode episode round-trip, migration, Unknown/Denied/Failed preservation, authority non-transfer.
- **Source-value ablation:** self-only vs MiCode-external vs generated experience under matched task/evaluator budgets.
- **Repository-history value:** static snapshot extraction vs history/outcome-aware extraction.
- **Knowledge transfer:** extracted pattern active vs ablated/scrambled on held-out repository families.
- **Crystallization economics:** baseline multi-step workflow vs promoted guarded skill/tool, including verification and deoptimization cost.

- **E14K — Kev-style coding Reflex:** small pretrained backbone + adapter + block-causal isolated branches + pointer/listwise head versus strongest simple Reflex baselines; report learning curves, packing speedup and transfer.
- **E14P — Permutation invariance:** canonical-order/shuffle augmentation/permutation-consistency objectives under matched compute; report flips, probability spread, NLL/Brier, selective task quality and downstream outcomes.
- **E14N — Candidate absence:** ordinary-only candidate sets versus typed `NONE`/`OBSERVE_MORE`/`ESCALATE` augmentation on missing-target and distractor fixtures.
- **E14T — Coding Transfer Frontier:** same-repo → unseen-repo → unseen-family/task-family → cross-language ladder with fixed quality/coverage/compute criteria.

---

<a id="doc-build-bootstrap-prompt-md"></a>

## build/BOOTSTRAP_PROMPT.md

# Bootstrap prompt for an AI builder

Paste this into the coding harness after mounting the actual Axon repository and this package. It authorizes no production deployment or irreversible external action.

---

You are implementing **Axon Cortex**, with **Axon Reflex** as the bounded typed-decision engine and **AIR** as the proposed Axon Intelligence Representation. Treat every spec in this package as Draft until imported and reviewed under the repository's existing governance. Do not assume proposed symbols or commands already exist.

Read `README.md`, `REVIEW.md`, `EXISTING_AXON_MAP.md`, `build/BUILD_PLAN.md`, `build/BUILD_PROTOCOL.md`, `build/LOOPS.md`, `build/TASKS.md`, and the specs for the first unblocked slice. Read the repository's actual build protocol, spec template/registry, invariants, exit ledger, generated reference, task notes and prior prototypes before modifying anything.

First perform B00/B01, not a broad implementation sprint. Identify the repository commit and dirty work. Preserve user changes. Map CX identifiers to existing governance without reserving arbitrary R numbers. Find actual evidence for interpreter/native parity, ambient effects, missing deployment gate behavior, FFI scope, R44 materialization, R2a status and host confinement. Record PASS/FAIL/SKIPPED/UNAVAILABLE with the precise claim. A source paragraph saying complete is not evidence you ran a gate.

Produce an intake/evidence map and one expanded work package for the next slice. Do not rewrite the architecture if prior art solves the problem. Mark blockers that a read or controlled test cannot resolve. Do not silently relax safety policy to make progress.

Build the smallest vertical slice: a resettable local Axon example, structured observation, snapshot-bound grant catalog, one bounded patch, isolated registered checks, protected completion verification and exact recorded replay. Start with a deterministic/mock conformance path, then compare an existing-model path against the simple strong-model baseline. No auto-merge, production deploy, arbitrary shell command tool, self-modifying verifier, custom model training or kernel rewrite in the first slice.

For each patch: write the failing/negative fixture first; implement one contract; run relevant tests; run parity or explicit-refusal cases where applicable; test real host enforcement before untrusted tools; inspect the diff; capture command/evidence artifacts; update matching specs/reference. Do not change the acceptance criterion merely because your patch fails it. Stop after bounded retries/no-progress or any authority/secret/recovery uncertainty.

Use one contract owner for shared schemas and sequenced integration for compiler type-map changes. Proposal workers cannot edit hidden evaluator data, signer credentials, required-gate policy or runtime authority. All model outputs are untrusted proposals. Report unavailable probabilities, stale state and OutcomeUnknown explicitly.

When B17–B20 become unblocked, do not “implement Jev.” Freeze the Axon Reflex ABI first. Compare the same dynamic-candidate corpus across generative structured-output, direct-logit, sequence-scoring and learned option-conditioned backends where feasible. Preserve score/probability provenance, run shuffled/empty/wrong/stale-state and option-order/ID controls, benchmark serial/parallel/shared-prefix inference, and fit calibration only from eligible grouped splits. Keep DECIDE, GENERATE and ACT separate. A backend or custom model is adopted only after protected task outcomes show value; negative bakeoff results are acceptable.

Read `build/REFLEX_BACKEND_BAKEOFF.md`, `build/REFLEX_DECISION_CORPUS.md`, and `build/REFLEX_CALIBRATION_LAB.md` before model-specialization work.

At handoff, report implemented behavior, files/digests, exact tests/results, unproven claims, unresolved dependencies, resource spend, and the next unblocked slice. Do not say a milestone is complete unless its real end-to-end gates passed for the recorded artifact/environment. Do not report package-document checks as runtime tests.

Begin with repository intake and B00/B01.

## v0.4 co-development note

Do not make the first Cortex milestone depend on MiCode. After B13 establishes canonical local evidence, B44 may freeze the MiCode↔Axon bridge in parallel. Treat MiCode exports as untrusted versioned artifacts; never deserialize foreign authority into local grants. Do not start repository mining/native promotion until CX-10/CX-11/CX-17 prerequisites and their evaluation gates exist.

## v0.5 intent-first requirement

Before adding a new direct prose→AIR/`.ax` path, inspect CX-19 and the live existing `intent compile`/review/approval implementation. Treat prose as untrusted input to a typed intent contract. Do not let a model-generated plan silently define its own authority, acceptance criteria or DONE condition.

---

<a id="doc-specs-index-md"></a>

## specs/INDEX.md

# Specification index

All specifications are Draft proposals with no product evidence yet. Dependencies name contract dependencies; implementation is sliced in [TASKS](build/TASKS.md). CX IDs do not reserve live Axon R IDs.

| ID | Specification | Depends on | First stage |
|---|---|---|---|
| CX-00 | [System contract and trust boundaries](specs/CX-00-system-contract.md) | — | M0 |
| CX-01 | [Evaluation, baselines and assurance evidence](specs/CX-01-evaluation.md) | CX-00 | M0 |
| CX-02 | [Software observations, state and working memory](specs/CX-02-world-observation.md) | CX-00 | M1 |
| CX-03 | [Dynamic capabilities and transactional execution](specs/CX-03-capabilities-executor.md) | CX-00, CX-02 | M1 |
| CX-04 | [AIR graph and interpreter-first integration](specs/CX-04-air-runtime.md) | CX-00, CX-03 | M1 |
| CX-05 | [Axon Reflex typed inference and speculative questions](specs/CX-05-reflex-inference.md) | CX-01, CX-04 | M2 |
| CX-06 | [Calibration, risk-aware routing and abstention](specs/CX-06-routing-calibration.md) | CX-01, CX-05 | M2 |
| CX-07 | [Predictive software world models](specs/CX-07-predictive-world.md) | CX-01, CX-02, CX-10 | M3 |
| CX-08 | [Planning, hypotheses and controlled experiments](specs/CX-08-planning-experiments.md) | CX-03, CX-06, CX-07 | M4 |
| CX-09 | [Representation search and transferable abstractions](specs/CX-09-representation-abstraction.md) | CX-07, CX-08 | M6 |
| CX-10 | [Replay, learning data and error attribution](specs/CX-10-replay-learning-data.md) | CX-00, CX-01, CX-02 | M1 |
| CX-11 | [Crystallization and independent artifact promotion](specs/CX-11-crystallization-admission.md) | CX-01, CX-06, CX-10 | M5 |
| CX-12 | [Per-pillar learning, curricula and meta-governance](specs/CX-12-learning-meta.md) | CX-08, CX-09, CX-11 | M7 |
| CX-13 | [Hosted OS integration, resource control and confinement](specs/CX-13-os-runtime.md) | CX-03, CX-04, CX-10 | M1 |
| CX-14 | [Parallel decision-model research](specs/CX-14-parallel-model-research.md) | CX-05, CX-06, CX-10, CX-11 | M5 |
| CX-15 | [Language, compiler and proof integration](specs/CX-15-language-compiler-proof.md) | CX-00, CX-04 | M2 |

| CX-16 | [MiCode experience bridge and cross-system conformance](specs/CX-16-micode-experience-bridge.md) | CX-00, CX-02, CX-10 | M1 |
| CX-17 | [External repository knowledge ingestion and evidence ladder](specs/CX-17-external-repository-knowledge.md) | CX-09, CX-10, CX-16 | M8 |
| CX-18 | [Capability synthesis and staged native promotion](specs/CX-18-capability-synthesis-native-promotion.md) | CX-11, CX-15, CX-16, CX-17 | M9 |

| CX-19 | [Intent compiler, typed Intent IR and semantic approval](specs/CX-19-intent-compiler.md) | CX-00, CX-01, CX-04, CX-11 | M1 |

## v0.6 Reflex refinement

No new CX spec was added. CX-05, CX-14 and CX-15 now carry the shared-state runtime, listwise-model and AIR dependency-scheduling contracts. `build/REFLEX_RUNTIME.md` is the implementation guide.

## v0.7 model-research refinement

No new CX spec was added. CX-05 and CX-14 now include canonical train/serve/replay encoding, typed absence/control candidates, early Kev-style reference-model research, transfer-first evaluation and new G14 gates. Build tasks B62–B68 implement the research/evaluation slices.

---

<a id="doc-specs-cx-00-system-contract-md"></a>

## specs/CX-00-system-contract.md

---
id: CX-00
title: "System contract and trust boundaries"
status: Draft
authority: Proposed
depends_on: []
first_stage: M0
implementation_evidence: []
---

# CX-00 — System contract and trust boundaries

## Intent and source basis

Specify what Cortex is allowed to promise and who may change those promises. Preserve Axon's interpreter-first and authority constraints, while refusing to infer stronger guarantees from phase-complete labels. Source basis: S1 pp.2–8, 12, 16, 23, 89–94; S2 pp.28–33 and 44–48. Sources resolve in [SOURCES](SOURCES.md).

## Decisive fork

Choose a **hosted, interpreter-orchestrated cognitive runtime with a separately enforced action boundary**, not a new neural architecture or replacement kernel as the first deliverable. A hosted runtime is a first deployment tier, not abandonment of the separately documented bare-metal direction.

## Required contracts

Cortex comprises four logically separated domains: observations and artifacts; untrusted proposal/learning workers; trusted execution authority; and independently controlled verification/admission. A process boundary alone is insufficient if the worker can edit the other domain's policy or files.

| Requirement | Normative behavior |
|---|---|
| CX00-R1 | Every action MUST be authorized by the executor using a trusted grant, not a model answer, confidence, pathname, or claimed principal. |
| CX00-R2 | Task completion MUST be decided against a locked completion contract by a verifier the candidate cannot modify. |
| CX00-R3 | Every approval MUST name the exact artifact digest, policy version, environment constraints and authority scope. A subsequent mutation invalidates the approval. |
| CX00-R4 | A required gate that is absent, skipped, unsupported, expired or inconclusive MUST NOT be treated as passing. |
| CX00-R5 | Evidence MUST distinguish test observation, statistical estimate, formal proof, simulation and unknown. |
| CX00-R6 | Learned components MAY propose authority, gate or checker changes but MUST NOT activate them. Such changes require the existing Axon TCB/invariant governance path. |
| CX00-R7 | Runtime/model/schema/policy versions MUST stay pinned for an episode; a requested change starts an explicit new epoch. |
| CX00-R8 | Non-consequential development profiles MAY mark optional checks skipped; they MUST be visibly different and ineligible for consequential promotion. |

## Data and interfaces

Define a versioned `RunManifest` containing run ID, principal handle, goal/contract digests, initial snapshot, runtime/toolchain digests, model and tokenizer/adapter versions, policy/calibration IDs, sandbox profile, budgets, approved effects, evaluation cohort, and trace destination. A hash of secret material does not grant disclosure rights; secret references are handled by CX-10.

Define a `SafetyProfile` with mandatory gate IDs, permitted targets, effect budgets, grant issuer, verifier identity, admission identity, retention policy and whether external effects are permitted. Unconfigured consequential fields cause a refusal. Initial profile permits resettable local workspace edits and approved isolated tests only; no production deploy, network mutation, self-merge or TCB edits.

The candidate cannot obtain the verifier's signing credential or final evaluation corpus. A local development signer is explicitly marked non-production. The same model may supply suggestions to both domains, but a model opinion cannot replace the verifier's mechanical evidence.

## Failure and assurance semantics

Represent errors using symbolic Cortex categories plus phase, retryability, side-effect status and evidence references. Do not allocate new Axon numeric diagnostic/exit codes in this package. Map categories into existing namespaces only after the ledger is inspected. A process exit code alone is not authenticated evidence that a guard fired.

Guarantees are scoped: a capability checker constrains covered operations; native FFI and the host kernel remain trusted where documented. An independent checker can still be wrong; assurance requires explicit assumptions, adversarial tests and scoped proof, not the word independent alone.

## Acceptance gates

**G00-contract:** a fixture with a complete profile parses; each missing mandatory field refuses. A worker claiming another principal or fabricating a completion signature cannot execute/close.

**G00-mutation:** approve artifact A, replace one byte to produce B, and attempt execution. B is denied without a new approval. Policy-version mismatch also refuses.

**G00-missing-gate:** simulate PASS, FAIL, SKIPPED, UNKNOWN and unavailable mandatory verifier. Only actual passing evidence for all requirements permits admission.

**G00-authority:** a learner attempts to edit gate code, evaluation data, policy or signer credentials. The host denies access and records the denied attempt.

## Build slices and exclusions

First implement manifest/profile validation and a denial-only executor; then integrate approved local execution through CX-03/CX-13. Keep snapshots and evidence append-only by default. Do not add custom AI syntax, native cognition, a neural model, distributed quorum, or a kernel in this slice.

## Open decisions and evidence

Owner must approve the first host target, workload, budgets and admission authority. Evidence is currently empty; source claims are documented only. Completion requires recorded commands and artifacts for G00 plus a reviewed threat-boundary diagram in the real repository.

## v0.4 self-optimizing OS / external experience boundary

Cortex is the cognitive control plane of a self-optimizing Axon stack. Eligible experience may come from Axon's own execution, MiCode coding episodes, approved external repositories/histories, or generated curricula. Source does not imply trust. Every external artifact enters through schema validation, data-use policy and local Axon authority; no bridge can mint a principal grant or redefine required gates.

---

<a id="doc-specs-cx-01-evaluation-md"></a>

## specs/CX-01-evaluation.md

---
id: CX-01
title: "Evaluation, baselines and assurance evidence"
status: Draft
authority: Proposed
depends_on: ["CX-00"]
first_stage: M0
implementation_evidence: []
---

# CX-01 — Evaluation, baselines and assurance evidence

## Intent and source basis

Make architectural complexity earn its place through reproducible end-to-end comparisons. S1 pp.10–11 warns that an earlier statefulness metric did not support the thesis; S2's fluid-intelligence sections are proposed mechanisms rather than results. W1's model-reference workflow scores must not be mistaken for observed ground truth. See [SOURCES](SOURCES.md).

## Decisive fork

Use locked task contracts, task-family-separated evaluation, explicit ablations and independent audit data. Do not accept a collection of component demos as proof of an improved coding system.

## Task contract

A `TaskFixture` MUST define the initial workspace/environment, permitted actions and resources, visible goal, hidden completion checks, expected or acceptable output properties, risk tier, reset procedure, time/cost limits, and failure categorization. Identifiers and seeds are stable. An agent cannot edit the contract or hidden checks.

Fixtures include valid programs, partially parsed programs, unresolved symbols, generated files, stale observations, missing credentials, malicious source comments, denied operations, ambiguous goals, timeouts, cancellation, crashes, build-script side effects and held-out repository families. Start with small Axon examples; broader repo/compiler tasks follow only after isolation is demonstrated.

## Baselines and fairness

Maintain five configurations: rules-only where a rule is applicable; one strong-model coding loop; structured AIR with that same model and no Reflex; AIR with Reflex but no learned world model; and the complete challenger. Report task applicability rather than counting inapplicable rule tasks as victories.

Hold tools, observation availability, task budgets, permitted retries, model versions, language primer and completion checks constant when comparing a component. Separately report whether observation compression or candidate pruning—not model choice—caused a gain. Include warm and cold cache runs, concurrency/load, hardware and inference placement. Charge unused speculative heads, redaction, observation construction, training amortization, failures and verification.

## Metrics and statistical contract

Primary quality is independently verified task success. Secondary measures: success per actual cost; end-to-end p50/p95 latency; intervention/abstention rate; valid-action fraction; harmful/unauthorized attempts and realized effects; recovery correctness; candidate recall; calibration and prediction quality; and per-family regressions.

Before comparing, freeze `EvaluationPolicy`: primary metric, non-inferiority margin, statistical method, confidence level, minimum precision/sample rule, subgroup floors, exclusion criteria, cost target, holdout access policy and maximum submissions. Parameters not filled for the target release leave promotion blocked. Illustrative numbers in a research notebook are not release policy.

Prefer paired tasks and cluster uncertainty estimates by task family/repository where observations share structure. Report intervals and inconclusive results. Do not promote because a difference was “not significant.” Rare safety failures require a distinct risk argument; zero observed failures is not a proof of absence. As a mathematical illustration only, zero events in n independent Bernoulli trials has one-sided 95% upper rate bound `1 - 0.05**(1/n)`. The independence assumption often fails for related coding tasks.

Repeated evaluation consumes information. Separate training, calibration, development, promotion and sealed audit sets by provenance/family/time as applicable. Record candidate submissions and rotate compromised sets. A model's self-assessment or teacher output is a weak label, not completion truth.

## Gate result format

Every result contains gate ID/version, candidate and incumbent digests, task/split manifests, environment, invocation, outputs, sample counts, metrics/intervals, result status and exclusions. Valid statuses: PASS, FAIL, INCONCLUSIVE, SKIPPED, UNAVAILABLE. `PASS` cannot be inferred from a missing output artifact.

## Acceptance gates

**G01-reset:** reset a fixture twice and compare canonical inputs; vary an undeclared environment input and require an invalid-fixture diagnosis.

**G01-fairness:** automatically compare manifests and refuse a head-to-head comparison with changed hidden budget, tool access, primer or completion contract.

**G01-leakage:** duplicate/mutated family-related fixtures across protected splits are detected by provenance plus reviewed similarity rules; final-audit paths are inaccessible to workers.

**G01-evidence:** a tiny inconclusive result cannot pass non-inferiority; a fully abstaining router cannot pass required coverage; a stale candidate digest cannot reuse a passing report.

**G01-ablation:** run the first vertical-slice task set with the strong-model and AIR controls before adding learned components. Report all attempts, including failed and canceled tasks.

## Build slices and exclusions

Build fixtures/reset and evidence schemas first; then the comparison runner; then the protected promotion evaluator. A baseline need not solve every task. No benchmark score by itself establishes fluid intelligence, model calibration on another domain, or kernel safety.

## v0.3 imported-evidence comparability

Before an external benchmark result is used as a Cortex baseline, record whether systems received the same evidence, candidate set, output obligation, retries, tools/permissions, cache conditions and completion verifier. Differences do not invalidate the source as an implementation lead, but they prevent a direct quality/speed comparison unless the experiment is reproduced under matched conditions. Failures remain in denominators. Cached and live inference are reported separately.

## v0.3 benchmark-comparability gate formalized in the spec

**G01-comparability:** an imported benchmark with different candidates/evidence/output obligations may motivate an experiment but cannot be cited as a direct Cortex performance comparison without matched reproduction.

---

<a id="doc-specs-cx-02-world-observation-md"></a>

## specs/CX-02-world-observation.md

---
id: CX-02
title: "Software observations, state and working memory"
status: Draft
authority: Proposed
depends_on: ["CX-00"]
first_stage: M1
implementation_evidence: []
---

# CX-02 — Software observations, state and working memory

## Intent and source basis

Produce compact, provenance-bearing state without losing necessary information. S2 pp.26–30 motivates structured observations and refreshed action spaces. S1 pp.6–7 documents R44 value materialization and its limits. See [SOURCES](SOURCES.md).

## Decisive fork

Use an incremental semantic observer with explicit incomplete state, not full-repository prompting and not a claim that a code graph is already a learned world model.

## State contracts

`WorkspaceSnapshot` identifies tracked and untracked permitted files, content digests, base revision, dependency lockfiles, generated artifact manifests, configured toolchain/environment, and observer version. A Git commit alone is insufficient. Hashing the entire machine is neither required nor permitted; the manifest states its observation scope and unknown inputs.

`Observation` contains snapshot ID, goal digest, diagnostics, relevant symbols/types/callers, changed files, available test descriptors, recent action outcomes, retrieval references and a partial-observation report. Each fact includes origin, artifact digest, extraction method, observed time and trust class. Distinguish stale, absent, parse-failed and explicitly unknown.

`WorkingState` holds subgoals, hypotheses, assumptions, contradictions, experiment results, remaining budgets, attempted strategies and evidence references. Store concise, externally inspectable decision records; no requirement to preserve private model reasoning. A belief does not overwrite an observation. Contradicting facts stay visible with provenance until resolved.

## IDs, compression and retrieval

Within one snapshot, object IDs are stable and collision-checked. Across revisions, use an explicit symbol mapping with confidence and failure cases; never assume a string ID still refers to the same node. Serialization is canonical for identical input content and configured nondeterministic fields. Real clocks, network state and races are recorded rather than wished away by sorting JSON.

Observation compression has a hard token/size budget, retains permissions and critical diagnostics, and exposes omitted regions with reasons. A retrieval result carries context-source hashes and a freshness deadline. Caches are namespaced by repository/tenant and invalidated by content, parser, model, schema or policy changes where relevant.

Support `INSPECT`, `SEARCH` and `EXPAND_SCOPE` within existing authority. A task can request more context without receiving more permissions. Large action spaces use hierarchical retrieval; measure whether the oracle-relevant target remained available. If no relevant candidate is present, return insufficient-context or abstain rather than force a bad choice.

## Broken-code behavior

Use recoverable parse trees where available; otherwise fall back to explicitly marked lexical/file-level observations. A missing type must be represented as Unknown, not inferred by another unsound heuristic. New-file proposals refer to a permitted creation namespace and validated artifact manifest; they are not blocked merely because the file was not in the initial snapshot.

Do not persist live interpreter closures, channels, aliased mutable objects or native handles across R44 cells. Persist typed serializable values and artifact references, then reacquire authority-bound runtime handles each epoch.

## Interfaces

Proposed protocol functions: `observe(snapshot, goal, scope_budget)`, `expand(observation_id, requested_scope)`, `diff(old_snapshot, new_snapshot)`, `resolve(object_id, snapshot_id)` and `retrieve(query, snapshot_id, trust_filter)`. These are contract names, not existing Axon builtins.

## Acceptance gates

**G02-canonical:** identical snapshot and observer inputs produce equal canonical observation bytes excluding declared volatile envelope fields; round-trip loses no fact provenance.

**G02-partial:** deliberately break syntax/type resolution. Observer still returns a useful partial state and never invents a successfully inferred type.

**G02-stale:** mutate tracked, untracked and dependency inputs separately. Relevant cached observations invalidate and old object references cannot silently resolve to new targets.

**G02-recall:** a fixture whose fix is outside the initial neighborhood can request bounded expansion and expose the correct target; report candidate recall separately from solver success.

**G02-injection:** source comments and retrieved text containing authority instructions are stored as untrusted content and cannot change permissions or task contracts.

## Build slices and exclusions

Implement file/diagnostic snapshots first; add semantic neighborhoods second; add working-memory and retrieval policies after the single-step loop works. Full ontology induction, multi-domain perception and a predictive world model are separate specs. Reuse existing parser/type information rather than duplicating it.

## v0.3 candidate search and absence semantics

The observer/capability compiler measures candidate recall independently from Reflex selection. It must distinguish `NoneSuitable`, `NeedMoreObservation`, `SuitableButUnauthorized`, and `InferenceUnavailable`. Candidate expansion may use flat scoring, retrieval+rereanking, hierarchical selection or bounded beam search, but every strategy records search cost, scope lineage and final recall. A top normalized Choice value is never evidence that a suitable option exists.

## v0.4 imported software observations

CX-16 may import MiCode/external semantic observations, but imported facts retain source-system identity, freshness/omission receipts and trust class. Axon may map semantic objects into its ontology only through explicit Exact/Projected/Approximate/Unresolved mappings. Imported observations never override a fresher authoritative local observation merely because they are more complete.

---

<a id="doc-specs-cx-03-capabilities-executor-md"></a>

## specs/CX-03-capabilities-executor.md

---
id: CX-03
title: "Dynamic capabilities and transactional execution"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-02"]
first_stage: M1
implementation_evidence: []
---

# CX-03 — Dynamic capabilities and transactional execution

## Intent and source basis

Turn selected semantic actions into bounded effects without allowing model output to create authority. S2 pp.18–33 motivates opaque observed targets and freshness checks; S1 pp.12, 16 and 48 limits the guarantees available from the existing interpreter/FFI stack. See [SOURCES](SOURCES.md).

## Decisive fork

Choose a trusted server-side grant registry and immutable workspace transactions. Do not treat enum membership, a signed-looking string, or a path prefix as authorization.

## Capability contract

A grant stores grant ID, issuer, principal/session, action kind, snapshot/epoch, allowed target set or namespace, read/write scopes, payload constraints, preconditions, expiry, invocation and resource bounds, and revocation state. The model sees a reference and readable description. The reference is meaningful only after trusted registry lookup. Child grants can attenuate parent authority and carved budgets, never enlarge them.

The selected action is a tagged union: Inspect(target), Search(scope), ProposePatch(targets, artifact), RunCheck(check_id), Revert(local_checkpoint), RequestContext(scope), Escalate(reason), DoneClaim(evidence_refs), or Blocked(reason). Additional action types require a manifest and recovery semantics. DONE has no execution authority.

A registered check maps to trusted executor configuration: program/entrypoint, argument schema, working directory, sandbox profile, effect permissions, environment/secret projection, and budgets. Arguments are arrays/typed values, never a shell command string. A generated test/build script is untrusted executable content even when invoked through a registered check.

## Payload and freshness validation

Generated patches are data. Parse/apply them only to the approved workspace; reject out-of-scope paths, traversal, symlink escapes, secret destinations and forbidden manifest changes. Validate new files through an explicitly permitted namespace. An inspected target can be valid yet semantically wrong: task correctness remains the verifier's job.

Prepare on an immutable snapshot. Before commit, validate expected content hashes and authority under a workspace lock or equivalent atomic compare-and-swap. Include dirty/untracked inputs and dependency/environment fingerprints. Avoid a check-then-write gap. Concurrent actions need compatible read/write sets and an atomic commit order; v0 is single-writer.

## Durable action state machine

`Proposed → Prepared → Validated → Running → Observed → Verified` is the normal path. Terminal alternatives are Refused, Failed, Canceled, or OutcomeUnknown. “Observed” means effect outcome recorded; “Verified” means the action/task contract was checked, not that every goal is complete.

Persist action ID and preparation evidence before effects. The execution attempt has an idempotency key and the host adapter declares at-most-once, deduplicated retry, or reconciliation-required semantics. A crash between an external effect and receipt is OutcomeUnknown. Reconcile against the external system or require operator review; never blindly rerun a non-idempotent effect.

Local patch rollback restores an immutable checkpoint and invalidates descendant artifacts/observations. It does not undo external calls or previously disclosed secrets. A test may generate local artifacts; include them in observed deltas and delete only inside its owned ephemeral workspace.

## Threat model and host dependency

Cover cross-session handle replay, principal impersonation, grant expiry, budget reuse, TOCTOU, symlinks/hardlinks, executable/tool replacement, compiler plugin/build-script execution and a worker attempting to mutate verifier assets. Where network is allowed later, destinations and redirects are executor policy, not model-provided permission. Strong host confinement is CX-13; unsafe host profiles remain disabled for real execution.

## Acceptance gates

**G03-forgery:** unknown, expired, wrong-principal and previous-snapshot grants all refuse without a side effect.

**G03-payload:** a valid Edit grant paired with a traversal, symlink or policy-file patch refuses; a permitted ordinary patch succeeds.

**G03-race:** mutate input between prepare and commit; exactly one compatible local transaction can commit and the stale one must reobserve.

**G03-crash:** inject crashes at every durable boundary. Recovery never blindly duplicates an external effect and identifies an unknown outcome.

**G03-budget:** nested child actions and speculative requests cannot exceed the parent's reserved aggregate budget; concurrent debits are atomic.

**G03-done:** a DONE claim with failing hidden checks cannot close the task, regardless of confidence or agent explanation.

## Build slices and exclusions

Build denial cases first, local snapshot patching second, isolated checks third. No generic shell tool, automatic production deployment, irreversible external effect, multi-agent concurrent writer, or capability self-minting in v0.

---

<a id="doc-specs-cx-04-air-runtime-md"></a>

## specs/CX-04-air-runtime.md

---
id: CX-04
title: "AIR graph and interpreter-first integration"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-03"]
first_stage: M1
implementation_evidence: []
---

# CX-04 — AIR graph and interpreter-first integration

## Intent and source basis

Give cognition explicit types, dependencies and budgets without multiplying language keywords. S2 pp.25–26 separates decision, generation and reasoning; S1 pp.2–5 establishes interpreter reference semantics and extension discipline. See [SOURCES](SOURCES.md).

## Decisive fork

AIR is initially a versioned host-validated execution graph and library interface. It is not immediately an Axon parser extension or an assertion that the current compiler supports cognitive syntax.

## Minimal operation set

| Operation | Semantics |
|---|---|
| Observe | Read a scoped environment observation through its authorized adapter. |
| Rule | Invoke a pinned deterministic function over typed inputs. |
| Retrieve | Fetch scoped evidence/artifacts; results carry origin and freshness. |
| Decide | Request bounded typed probabilities/choices through Reflex or a compatible adapter. |
| Generate | Produce an untrusted artifact such as a patch or text. |
| Reason | Produce a structured plan/hypothesis proposal using deliberate computation. |
| Predict | Produce a distribution over defined outcomes, possibly with abstention. |
| Check | Run a registered test/proof/check and return evidence, not model opinion. |
| Act | Request an executor-authorized effect. |

REFRAME, ABSTRACT, HYPOTHESIZE, EXPERIMENT and DISCOVER start as composed library workflows. Learning runs outside live task graphs, using explicit offline jobs and admission. A node label never makes an external model call pure: inference still has configured AI/network/cost effects.

## Graph contract

An AIR artifact contains schema version, content digest, node IDs/kinds, typed inputs/outputs, data dependencies, control predicates, capability requirements, resource limits, model/provider selectors, timeout/retry policies, and observation/artifact lineage. Acyclic per-iteration graphs are the initial representation. Repetition belongs to a bounded task controller with explicit stop conditions, not hidden graph recursion.

Static validation checks unknown references, cycles, type mismatches, missing branch fields, authorization requirements, effect escalation, unbounded iteration, and invalid speculative regions. Runtime checks bind dynamic targets and payloads to actual grants/snapshots. A field needed after another field's resolution has a real dependency edge; it is not independent merely because a model can answer both.

Rule functions claiming determinism cannot call an effectful provider. Returned uncertainty and unavailable evidence must be represented in the output type. No implicit empty artifact, default success, guessed distribution or hidden provider fallback is permitted.

## Scheduling and execution

Start with a deterministic serial scheduler. Parallelization is an optimization over nodes whose effects and dependencies permit it. Record order of observed completions and cancellation events. Speculative Decide outputs may be computed; Act and untrusted tool effects are never speculative in v0.

Budget reservations happen before dispatch and reconcile actual usage when known. Queue deadlines, timeouts, bounded retries and backpressure propagate to children. Refusal, failure, abstention and cancellation are distinct outcomes. Provider fallback is a new explicit dispatch recorded with the effective model/schema/calibration versions.

## Axon integration

Reuse Axon typed programs for deterministic transformations, metrics and scoped checks. Add a narrow host seam rather than routing arbitrary AIR operations through `exec`. Logical modules may initially live together behind interfaces; physical crate splits should follow measured build/test boundaries.

R44 sessions cannot carry arbitrary live handles across cells. Store stable artifact references in the Cortex controller; reacquire validated handles per call. Do not rebuild a parallel type inference engine. Native lowering is deferred; unsupported cognitive operations refuse explicitly rather than silently run without interpreter safety.

## Acceptance gates

**G04-schema:** unknown nodes, bad types, missing branch outputs, cycles and unbounded loops fail validation with stable symbolic categories.

**G04-effect:** a Rule node attempting an AI or filesystem effect and a child node requesting widened authority both refuse.

**G04-replay:** a pinned simple graph executed through recorded host/model replies reproduces output and action selection without new effects.

**G04-scheduler:** deterministic serial and permitted parallel modes agree on the semantic result; reordered effectful nodes are rejected.

**G04-fallback:** a provider outage produces an explicit failure/authorized fallback event, never an unnoticed model switch or a fabricated answer.

## Build slices and exclusions

Implement schema validation, serial scheduler and a mock Decide adapter before model integrations. Add actual model dispatch after the safe single-step task works. Language sugar, optimizer fusion and native execution each require separate CX-15 evidence.

## v0.3 model-call scheduling rule

AIR may share verified common state/prefix work across questions, but it may not silently fuse separately specified question computations into a joint generative prompt and call the result semantics-preserving. Such fusion is a backend/policy variant and requires conformance, calibration and protected-task evaluation. Batch dependencies and active-branch result requirements are explicit graph edges.

---

<a id="doc-specs-cx-05-reflex-inference-md"></a>

## specs/CX-05-reflex-inference.md

---
id: CX-05
title: "Axon Reflex typed inference and speculative questions"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-04"]
first_stage: M2
implementation_evidence: []
---

# CX-05 — Axon Reflex typed inference and speculative questions

## Intent and source basis

Implement the fast bounded-judgment interface independently of any vendor or custom neural architecture. S2 pp.18–26 motivates operation selection plus branch-specific targets. W2 documents isolated questions against shared state; that is not a statement of statistical independence. See [SOURCES](SOURCES.md).

## Decisive fork

Support several conforming backends and measure them on the same tasks and dynamic candidate manifests: (A) constrained-output generative adapter, (B) direct option-logit scorer, (C) sequence-probability scorer, and (D) learned option-conditioned/shared-encoder decision head. Keep generation and reasoning separate from Decide, and keep Decide separate from Act. Do not require a particular decoder or claim it reproduces an undisclosed vendor training method.

## Supported schemas and results

V0 offers BinaryDecision (with compatibility adapters free to map external Noul naming), Choice over a finite ordered runtime candidate set, and bounded ordinal Score with an explicit rubric. Rank, continuous prediction and open-ended extraction are extensions; arbitrary text/code belongs to Generate. Every question includes ID, definition, candidate IDs/descriptions, required dependencies, state digest, schema version and permitted abstention reasons.

A response reports selected candidate or abstention; optional genuine raw scores; optional complete derived probabilities; typed probability/uncertainty origin (`NativeOptionLogit`, `SequenceLikelihood`, `GeneratedEstimate`, `EntropyDerived`, `EnsembleEstimate`, `CalibratedEmpirical`, or `Unavailable`); inference/model/tokenizer/adapter versions; calibration reference or Uncalibrated; state/question/candidate-set digests; ordered candidate manifest; prefill/incremental timing where available; latency/usage/cost; and any refusal. A backend that only produces a label MUST NOT invent a probability. Model-reported confidence is marked self-report and ineligible for probability-based release routing without empirical calibration evidence.

Validate exact candidate membership; finite numeric values; range; normalization tolerance; field presence; branch tag; and snapshot identity. Invalid results remain failed observations, never silently coerced into successful decisions. The candidate ordering is part of the schema; test permutation sensitivity explicitly.

## Speculative conditional questions

For ActionKind, EditTarget, CheckTarget and InspectTarget, frame each target question conditionally: choose an edit target assuming the operation is Edit, for example. All questions refer to the same observation/candidate-manifest digest. After ActionKind resolves, validate and retain only its corresponding target output and any cross-field constraints.

Unused results do not become tool calls. They are still paid model work, and may still involve authorized network disclosure. Bound question count, total candidates, context bytes, KV-memory allocation and speculative spend. A dependent question that needs generated patch content or another answer's actual value must wait, or explicitly branch over a finite validated hypothesis set.

Parallel evaluation does not justify multiplying marginal probabilities to obtain joint correctness. Joint risk is calibrated for the complete selected action packet or treated conservatively. Contradictory independently produced outputs are rejected or escalated.

## Candidate-scoring details

Multi-token labels must be scored by a declared method, including complete sequences or a prefix-trie traversal; comparing only first-token logits is not a general solution. Register the normalization/length-bias treatment and any fixed opaque label encoding. Refit/recheck calibration when label construction changes.

Shared-prefix caching is an optimization with strict model/tokenizer/prompt/schema/tenant keys. Do not share mutable caches between tenants without isolation. Include long contexts, many candidates, shared token prefixes, Unicode, empty candidates and adversarial descriptions in conformance tests. No backend receives a blanket “one forward pass” or constant-time guarantee.

## Acceptance gates

**G05-schema:** all backend families conform to one dynamic-candidate ABI; malformed, missing, non-finite, outside-candidate and wrong-snapshot results fail predictably; label-only responses stay label-only; probability provenance cannot be forged.

**G05-branch:** a selected Check operation can only use CheckTarget; speculative EditTarget output never causes a write. Contradictory active fields cannot produce an action.

**G05-isolation:** correct state is compared with shuffled/empty/wrong/stale state, candidate reorder/rename, irrelevant context and adversarial options; state-insensitive shortcuts and sensitivity are reported; no false guarantee of independent errors is made.

**G05-tokenization:** multi-token candidates use a declared complete scoring method; exact opaque candidate IDs round-trip independently of human labels; candidate count/order/length sensitivity is measured.

**G05-performance:** compare generative/direct-logit/sequence/learned-head backends and serial/parallel/shared-state modes across context/cardinality/load sweeps. Publish prefill, incremental question/option, memory, end-to-end/backend cost/latency, selective decision quality and downstream task impact.

## Build slices and exclusions

Begin with mock and constrained generative adapters, then direct-logit and sequence scorers against one stable ABI. Build the grouped decision corpus and destructive controls before interpreting quality. A learned option-conditioned/shared-state neural head remains CX-14 research unless the B20 bakeoff justifies it. Provider success does not certify task completion, safety or calibration.

## v0.3 contract hardening

CX-05 additionally requires the executable conformance contract in [REFLEX_CONFORMANCE](build/REFLEX_CONFORMANCE.md). Multi-question responses are keyed by `QuestionId` and may partially fail; the active branch defines which missing results block action construction. Choice, binary probability and ordinal score/distribution are distinct result families rather than a universal selected-label envelope.

Every backend publishes a `BackendFeatureManifest` and an `EffectiveInputReceipt`. Backend limits, silent preprocessing and truncation cannot be hidden behind a conforming response type. `ProviderReportedDistribution` is a valid raw evidence class distinct from native logits. Only the trusted adapter constructs privileged model instructions.

Candidate search must explicitly support `NoneSuitable`, `NeedMoreObservation`, `UnauthorizedCandidate`, and `InferenceUnavailable` states. Large candidate spaces are evaluated with registered retrieval/reranking or hierarchical/beam strategies; ranking heuristics such as path-score composition are not called calibrated correctness probabilities without separate evidence.

Question fusion/shared-prefix execution is an optimization only when the declared computation is preserved. Joining previously isolated prompts into one generative prompt is treated as a model/policy change and re-enters calibration/evaluation.

## v0.3 conformance acceptance gates formalized in the spec

**G05-batch-identity:** multi-question results are keyed by QuestionId; duplicate/unknown IDs fail; missing active-branch output blocks the action; unused speculative failure follows an explicit batch policy.

**G05-effective-input:** decisive evidence beyond a backend limit produces refusal or a visible authorized projection/truncation receipt; silent truncation fails.

**G05-prompt-boundary:** repository/log/replay text containing fake system/tool messages cannot become privileged adapter instructions or execution authority.

**G05-primitive-semantics:** Choice distributions, binary positive-event probabilities and ordinal distributions/expectations round-trip without silent rounding or invented confidence/logits.

**G05-candidate-absence:** no-suitable-option, need-more-observation, unauthorized-candidate and inference-unavailable paths remain distinct and produce the specified controller behavior.

## v0.6 shared-state Reflex runtime contract

Reflex is split conceptually into **Reflex Model** and **Reflex Runtime**. A conforming runtime MAY expose a two-stage optimization interface:

```text
encode_state(state, model_identity, preprocessing_policy) -> StateHandle
decide_batch(StateHandle, QuestionBatch) -> DecisionBatchResult
```

`StateHandle` is an optimization capability, not executable authority. It is immutable, bound to the exact state digest, model/tokenizer/adapter revision, preprocessing manifest, tenant/principal scope and expiry, and cannot be reused after any binding changes. Backends that cannot expose reusable state may emulate the interface in one call while still reporting equivalent receipts.

The runtime owns branch scheduling, candidate packing, response mapping, cancellation, budgets, cache lifetime/isolation and accounting. The model owns score/distribution production. This separation lets TypeSafe-compatible, direct-logit, sequence, learned-head and future Axon-native models compete without rewriting AIR.

Candidate-set inference is defined over the tuple `(state, question, candidate set, candidate order policy)`. Ordering is therefore part of the calibration/replay domain. Every dynamic candidate manifest records a deterministic construction policy and `order_digest`. High-assurance deployments MAY use registered permutation ensembles; the aggregation rule and extra compute must be explicit.

A decision distribution is primary. `top_choice`, entropy, margin, expected utility and any display `confidence_summary` are derived fields with formula/provenance. A derived confidence value is never silently promoted into empirical correctness probability.

### Question dependency classes

AIR/Reflex questions declare exactly one scheduling class:

- `Independent`: answer does not depend on sibling answers; eligible for the same shared-state batch.
- `ConditionallyRelevant(branch)`: framed under an explicit branch assumption and eligible for speculative evaluation; only the selected branch may be consumed.
- `AnswerDependent(question_id)`: requires an earlier realized answer and must execute in a later stage unless the graph explicitly expands a finite validated hypothesis set.

Batching/fusion is semantics-preserving only when these dependencies and prompt-role boundaries remain unchanged. Joining previously isolated questions into one conversational/generative prompt is a model-policy change, not a compiler optimization.

### Additional acceptance gates

**G05-state-handle:** state encoding is computed/reused only under matching state/model/tokenizer/preprocessing/tenant bindings; a stale or cross-tenant `StateHandle` refuses. Backends without reusable state still emit equivalent effective-input and timing receipts.

**G05-question-isolation:** for backends claiming isolated question semantics, Q1 alone and Q1 batched with unrelated, contradictory, adversarial and 100 irrelevant sibling questions stay within a preregistered tolerance under the backend's pinned deterministic/stochastic evaluation protocol. Failure is reported as loss of the isolation claim, not normalized away.

**G05-order-domain:** candidate permutation tests are part of conformance and calibration. Order-sensitive backends record the exact order policy/digest; a calibration artifact cannot be reused after candidate construction/order policy changes without revalidation.

## v0.7 control candidates and canonical encoding

Finite-option inference must represent the possibility that the current candidate compiler is incomplete or that escalation is required. Where a decision family permits it, the candidate compiler may append typed control candidates such as `NONE`, `OBSERVE_MORE`, `ESCALATE` or `BLOCKED`. These are ordinary typed candidates with policy-defined semantics; they do not grant authority. A backend may never synthesize an unregistered control candidate.

The canonical decision encoder/renderer is a versioned contract shared by training, evaluation, serving and replay. It owns reserved-token escaping, state/question/candidate serialization, branch boundaries, position policy and candidate-ID mapping. Training against one encoding while serving another is distribution shift and invalidates calibration until revalidated.

Candidate-order robustness is desirable but does not erase order from semantics or provenance. Every answer remains bound to the exact candidate manifest and `order_digest` that was evaluated.

---

<a id="doc-specs-cx-06-routing-calibration-md"></a>

## specs/CX-06-routing-calibration.md

---
id: CX-06
title: "Calibration, risk-aware routing and abstention"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-05"]
first_stage: M2
implementation_evidence: []
---

# CX-06 — Calibration, risk-aware routing and abstention

## Intent and source basis

Choose inexpensive cognition only inside an empirically supported reliability region. S2 pp.10–15 proposes confidence-gated escalation; W3 motivates validating probability calibration separately from classification. See [SOURCES](SOURCES.md).

## Decisive fork

Hard authorization and required evidence precede statistical routing. Within that permitted set, optimize task loss, cost and latency using measured policy performance. Do not adopt a universal probability threshold or assume that every task should avoid deliberate reasoning.

## Calibration contract

A `CalibrationArtifact` binds model weights/version, tokenizer, prompt and question schema, candidate-construction policy, domain/task family, training/calibration/evaluation split manifests, fitting method, metrics with uncertainty, coverage, subgroup exclusions, validity period and revocation triggers. A content hash pins the artifact. Quantization or a model/provider change invalidates calibration unless revalidated.

Measure proper scoring metrics appropriate to the problem, such as multiclass Brier score and negative log likelihood, plus reliability plots, selective risk/coverage, subgroup performance, state/candidate perturbation sensitivity and a preregistered routed-system measure such as Verified Utility at Coverage (VUC). ECE is a diagnostic, not a sole promotion gate; binning and limited samples can obscure errors. A probability of a choice and the probability that a full action succeeds require different labels.

For an ordinal score, specify the event being calibrated: score category, threshold crossing, or observed downstream outcome. For continuous outcomes, define intervals and coverage separately. No estimate is described as an individual guarantee just because aggregate calibration passed.

## Router policy

Input: task/authority profile; observation and omission report; available candidates; raw decision packet; calibration applicability; novelty/disagreement signals; prior progress; budget remaining; and action reversibility.

Allowed routes: deterministic Rule; Reflex; Retrieve/Observe; Generate; Reason; Predict/Experiment; independently required Check; authorized human review; or Blocked. Routing is a graph, not a compulsory linear RULE→REFLEX→THINK chain. A simple deterministic check may settle a question more cheaply than invoking a bigger model. A known difficult task may go directly to Reason.

The policy can minimize estimated expected loss plus measured resource cost subject to authority, task quality and coverage constraints. Cost weights and risk limits are owner-approved versioned inputs. It cannot waive a proof/check or invent missing permission because expected utility looks positive.

Out-of-domain signals include calibration inapplicability, candidate omission, ensemble disagreement, stale observations, prediction failures and repeated no-progress actions. These are imperfect signals. The controller must support abstention even with a high predicted probability, and report undetected shift on evaluation fixtures.

## Operational semantics

Apply hysteresis or a bounded switching policy to avoid repeated small-model/large-model oscillation. Cache only exact pinned decisions or scope-validated reusable facts. All escalations consume the same parent budget; a retry cannot reset the budget. A exhausted reasoning policy stops or asks the authorized operator rather than quietly looping.

Fallback and deoptimization are first-class: a rule or Reflex specialization that leaves its applicability domain returns control to the baseline policy. Log the reason, versions and cost. Model access failure is not evidence of low semantic confidence.

## Acceptance gates

**G06-calibration:** fitting uses only grouped permitted calibration data; raw score origin and calibrated result remain separate; proper scoring/reliability/risk-coverage/VUC on held-out task families are reported separately. Shuffling labels or changing model/schema/candidate policy invalidates the artifact.

**G06-risk:** an unauthorized/irreversible action never becomes allowed solely because the selected answer has probability 1.0.

**G06-coverage:** an all-abstain router fails the coverage target; a high-coverage router with unacceptable selective risk also fails.

**G06-shift:** shift/OOD fixtures cause the specified escalation or inapplicability behavior; report remaining failures instead of claiming perfect detection.

**G06-budget:** repeated escalation, retry and fallback cannot exceed parent limits; the termination reason is observable.

## Build slices and exclusions

Start with a deterministic router that is safe without probabilities. Add empirical calibration and learned routing only after labels and baseline evaluations exist. No RL-based router is required for v0. Safety limits are governed by CX-00/CX-11, not learned online.

## v0.3 selective-routing definitions

For protected evaluation, `Coverage = handled_without_escalation / eligible_decisions`. `SelectiveError = incorrect_handled / handled`; when no decisions are handled it is undefined, not zero. Report task success, latency and cost beside coverage/selective error rather than hiding them inside a single score. Any aggregate VUC-style metric must publish its exact formula, weights and eligibility rules before the experiment.

Provider-reported distributions, entropy measures and generated estimates can inform routing but do not become calibrated correctness probabilities without an applicable empirical artifact. Calibration binds the effective-input/preprocessing contract and is invalidated by material truncation/projection changes.

A backend cannot improve reported quality by dropping failed requests, silently narrowing the eligible set, or escalating difficult examples outside the denominator.

---

<a id="doc-specs-cx-07-predictive-world-md"></a>

## specs/CX-07-predictive-world.md

---
id: CX-07
title: "Predictive software world models"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-02", "CX-10"]
first_stage: M3
implementation_evidence: []
---

# CX-07 — Predictive software world models

## Intent and source basis

Predict defined consequences of candidate software actions and improve real planning decisions. S1 pp.23 and 83–84 supplies narrow numerical World/MDL prior art, not a general learned transition model. S2 pp.33–43 proposes prediction error and new representations. W4 studies model-bias concerns in model-based learning. See [SOURCES](SOURCES.md).

## Decisive fork

Start with small, observable, short-horizon predictions grounded in actual executions. Do not start with a neural latent simulator of the entire OS, and do not treat successful simulation as evidence that a real task is complete.

## Layered world contract

Keep five layers distinct: raw evidence; structured observation; inferred belief/hidden-state hypotheses; a predictive model; and a planner using those predictions. An observer can be useful without any learned model. A predictive model is explicitly partial and is not assumed to expose a sufficient/Markov state.

`WorldModelManifest` records version/artifact, outcome schema, required observation features, permitted action classes, training lineage, calibration/evaluation reports, supported domains/horizons, uncertainty mechanism, resource limits and revocation conditions.

`PredictionRequest` names observation/snapshot, the fully specified candidate action, action payload digest, environment/toolchain, horizon and requested outcome fields. For edits, predict after a concrete patch exists. A prediction about “edit symbol X somehow” must be labeled a policy-level forecast over unspecified edits, not a prediction for the final patch.

`PredictionResult` contains distributions or intervals over the requested outcomes, model manifest, applicability, support/evidence references, omitted targets, and abstention where necessary. Use a separate `PredictionError` record only after matched real outcomes arrive; censored outcomes stay unknown.

## First prediction targets

Start with affected file/test neighborhoods; probability of selected check pass; likely diagnostic family; check runtime interval; and chance an action leaves its workspace scope. Some targets are deterministic analyses and should remain Rules. Compare a learned model against dependency heuristics, empirical base rates and the no-predictor agent.

A security-violation prediction is advisory. The executor must prevent prohibited behavior independently of any predicted probability. Predictions are not substitutes for tests, compiler checking or proof obligations.

## Training and fit

Use eligible `(snapshot, observation, concrete action, outcome, environment)` episodes with explicit label quality from CX-10. Split by repository/task family/time where relevant. Preserve rare failures and report distribution shift. Distinguish data selected by the incumbent policy from representative coverage; unchosen actions have no observed outcome by default.

Fit tolerances depend on the domain. Deterministic toy tasks may require exact fit; noisy timing and external outcomes require declared noise models or intervals. Keep quality constraints and model-size/complexity preference separate. AST MDL is a labeled proxy; a genuine two-part description objective must count model and residual/exception costs with a specified code scheme.

## Planning limits

Bound simulated horizon, candidate count and compute. Stop or shorten rollouts when model applicability/support becomes poor. Root simulations in real recent observations and reobserve after effects. Model-generated synthetic labels are never mixed with real outcomes without provenance.

Optimize for downstream benefit: fewer check runs at the same completion quality, better experiment choice, or lower task cost. Forecast scores alone cannot justify removing real verification. A model that improves calibration but worsens planner outcomes is not automatically promoted.

## Acceptance gates

**G07-target:** every prediction binds to its exact patch/action and environment. Mutating the patch or toolchain invalidates the forecast.

**G07-holdout:** beat or match preregistered simple prediction baselines on protected outcomes; report calibration, interval coverage, error and per-domain failure.

**G07-planning:** compare the same planner with no model, simple model and candidate model on held-out tasks; charge prediction cost.

**G07-exploitation:** adversarial candidate search attempts to find actions the simulator likes but real checks reject. Report gaps; no such simulated success may certify completion.

**G07-unknown:** missing features, unsupported action/horizon and canceled experiments return explicit inapplicability or censored labels, not invented confident forecasts.

## Build slices and exclusions

Implement prediction/outcome matching and base-rate models first; then learned diagnostic/test forecasts; then short-horizon planning. General causal discovery, abstract latent worlds, physics and kernel models are later research, not entry requirements.

## v0.3 counterfactual label discipline

Predictive training/evaluation distinguishes realized outcomes from unobserved alternatives. An executed action supplies an observed target only for that action under the recorded state/environment. Alternative actions remain unknown unless replay/reset experiments, randomized exploration or a declared off-policy estimator provides evidence. Selection probabilities refer to behavior policy, not model confidence.

---

<a id="doc-specs-cx-08-planning-experiments-md"></a>

## specs/CX-08-planning-experiments.md

---
id: CX-08
title: "Planning, hypotheses and controlled experiments"
status: Draft
authority: Proposed
depends_on: ["CX-03", "CX-06", "CX-07"]
first_stage: M4
implementation_evidence: []
---

# CX-08 — Planning, hypotheses and controlled experiments

## Intent and source basis

Let Cortex gather discriminating evidence instead of repeatedly generating patches or escalating blindly. S2 pp.34–39 describes hypothesis/experiment loops; S1 p.23 documents value-level Plan/Schedule/Feedback prior art. See [SOURCES](SOURCES.md).

## Decisive fork

Build bounded task planning over real executor capabilities, with explicit hypothesis uncertainty and resettable experiments. Do not assume that a conditional predictor identifies causality or that calculated information gain is ground truth.

## Plan and hypothesis contracts

`Plan` contains goal/contract ID, current snapshot/belief, subgoals, candidate action graph, expected outcomes, evidence needs, dependencies, budgets, replanning triggers, stop conditions and fallback. Proposal is untrusted. The plan never widens the union of currently granted effects.

`Hypothesis` contains a falsifiable statement, variables/representation, support and contradicting evidence, prior/posterior or qualitative uncertainty, predicted observations, assumptions and applicability. Contradictory hypotheses may coexist. Never normalize made-up probabilities just to satisfy an interface; an unweighted set is valid until a weighting model is justified.

An `Experiment` identifies the intervention, control/reset strategy, concrete capability/action, measured variables, predicted outcomes by hypothesis, information-cost estimate, risk, resource budget, stopping rule and resulting evidence. Experiment descriptions must compile into granted actions; prose does not authorize execution.

## Selection and control

Use a transparent baseline first: choose the cheapest permitted check that distinguishes the most remaining hypotheses according to the declared model. A later policy may maximize estimated expected information gain or value of information subject to resource/risk constraints. Both the estimate and its assumptions are logged.

Do not rank experiments only by entropy reduction: a low-entropy wrong hypothesis can be worse than an uncertain correct set. Evaluate diagnosis accuracy, task completion and information gained per actual cost. A useful experiment may reduce uncertainty without changing the code.

Replan on stale snapshots, failed preconditions, unexpected outcomes, repeated no-progress actions or budget changes. Keep an action-history signature and maximum retry/no-progress budget. A new strategy proposal does not reset the parent budget. Termination modes include VerifiedComplete, BudgetExhausted, NeedsAuthority, InsufficientEvidence, Canceled and Failed.

## Causal evidence levels

Every causal edge/claim carries one of ObservationalAssociation, InterventionSupported, or AssumedMechanism. An observational predictor does not automatically upgrade its edges after a confident answer.

Initial interventions are restricted to resettable copies of software environments. Record exact before/after state, randomized or matched assignment where feasible, controlled variables, repetitions and residual confounders. A concurrency test needs enough repeats and explicit scheduling assumptions; a single disappearing failure does not prove a race mechanism.

Counterfactual queries name the structural assumptions and whether their output came from an exact executable model or a learned approximation. A causal hypothesis can guide experiments without being treated as established fact.

## Acceptance gates

**G08-hypothesis:** two hypotheses with distinct registered predictions cause selection of a separating permitted check; unsupported labels remain uncertain.

**G08-controls:** replay/reset and matched control execution preserve declared controlled variables; a confounded fixture does not produce an unqualified causal conclusion.

**G08-authority:** a high-information experiment requiring denied execution/network access is blocked rather than run.

**G08-progress:** repeated identical unsuccessful actions trigger bounded replan/escalation/stop; no infinite reasoning loop or budget reset occurs.

**G08-value:** compare against fixed check order, random permitted probing and strong-model-only reasoning on the same held-out tasks and budgets.

## Build slices and exclusions

Start with two or three competing debugging hypotheses and approved checks, then extend plan depth and experiment policies. No open-ended internet experimentation, arbitrary environment mutation, or production exploration in v0. General causal discovery remains a research result to demonstrate, not a consequence of adding a causal graph type.

---

<a id="doc-specs-cx-09-representation-abstraction-md"></a>

## specs/CX-09-representation-abstraction.md

---
id: CX-09
title: "Representation search and transferable abstractions"
status: Draft
authority: Proposed
depends_on: ["CX-07", "CX-08"]
first_stage: M6
implementation_evidence: []
---

# CX-09 — Representation search and transferable abstractions

## Intent and source basis

Test whether new representations and reusable concepts improve novel problem solving. S2 pp.33–44 motivates reframing, hypothesis invention, analogy and crystallization. S1 pp.83–84 supplies a constrained compression prototype. See [SOURCES](SOURCES.md).

## Decisive fork

Treat representation/abstraction discovery as a bounded empirical research program over typed artifacts. Do not equate a new concept name, attractive explanation or small AST with demonstrated fluid intelligence.

## Representation contract

A `Representation` records source observations, adapter/generator artifact, representation schema, entities/relations, assumptions, lossy transformations, origin mapping, allowed queries, measured predictive/solver utility, complexity proxy and validation results. Original evidence remains recoverable through references; no lossy transform may silently drop an authority constraint or contradicting observation.

Initial representation candidates are AST/type relationships, dependency/dataflow graphs, state machines and constraint encodings. They are generated and compared under a fixed resource budget. Reuse native parser/type data where possible rather than asking models to infer it from text. Generated encoders run under the same sandbox as other untrusted tools.

A `ConceptCandidate` contains a typed definition or executable recognizer, supporting examples, counterexamples, relations to existing concepts, applicability predicate, claimed uses and testable transfer predictions. New vocabulary alone is insufficient. Refinement, split and merge operations preserve provenance and invalidate dependent caches/artifacts when semantics change.

## Search and admission

Search over representation transformations and concept definitions. Evaluate on held-out examples and downstream decisions. Complexity can regularize search, but acceptance requires a preregistered quality/transfer benefit or a proved scope-specific simplification. Penalize residual exceptions explicitly; do not compress away rare safety-relevant cases.

Representation-dependent claims state their assumptions. If a dataflow abstraction ignores concurrency, it must not be used to claim concurrency safety. A synthesized invariant is an untrusted proposal until checked. A candidate proof's validity is limited to its theorem and assumptions.

Admission produces an immutable representation/concept artifact with a domain envelope, version, metrics, evidence and deoptimization path. An accepted concept may aid retrieval or planning without becoming a language primitive. Compiler integration belongs to CX-15.

## Research evaluation

Separate discovery tasks from transfer tasks by family and construction mechanism, not just by different variable names. Compare against the same strong model without the new representation, with a standard human-provided representation, and with shuffled/ablated concepts. Report examples needed to learn, compute spent and regression on existing tasks.

Use curriculum tasks only for training/development. A generator cannot choose the final audit problems or reward itself for recognizing its own templates. Keep public/in-scope prior exposure recorded rather than claiming impossible-to-verify total novelty.

For analogy, test relation-preserving transfer and the conditions under which it fails. Superficial similarity scores are not evidence of a shared mechanism.

## Acceptance gates

**G09-lineage:** every derived node/claim maps to source evidence or an explicit hypothesis; unsupported invented facts cannot masquerade as observations.

**G09-counterexample:** a discovered concept is tested on known counterexamples and out-of-domain cases, with appropriate abstention or failure.

**G09-transfer:** preregistered novel-family tasks show the claimed gain against representation and compute-matched baselines; inconclusive results stay research artifacts.

**G09-compression:** a shorter model that violates fixed held-out fit/safety constraints fails, even if its training fit or description length improves.

**G09-ablation:** removing the concept or scrambling its assignments measurably tests whether it—not additional context or compute—caused the gain.

## Build slices and exclusions

First compare two fixed encodings on one task family. Next allow a bounded generator to propose transformations. Only then attempt reusable new concepts and cross-domain transfer. No promised date for general intelligence, no automatic promotion into compiler syntax and no mutable shared ontology without migration records.

---

<a id="doc-specs-cx-10-replay-learning-data-md"></a>

## specs/CX-10-replay-learning-data.md

---
id: CX-10
title: "Replay, learning data and error attribution"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-01", "CX-02"]
first_stage: M1
implementation_evidence: []
---

# CX-10 — Replay, learning data and error attribution

## Intent and source basis

Make every executed step auditable and every eligible outcome learnable without treating every trace as training material. S1 pp.15–16 and 94 describes host replay and secret-bearing journals; S2 pp.44–48 describes per-pillar learning and attribution. See [SOURCES](SOURCES.md).

## Decisive fork

Use one linked event lineage with separate storage/access projections: restricted raw replay, redacted review, policy-approved learning export and sealed evaluation. Do not create four unrelated truth stores or train indiscriminately on raw host journals.

## Event contract

Events include run/episode/action IDs, parent causal IDs, sequence/epoch, manifest/version references, principal handle, observation/state/question/candidate-set digests, ordered dynamic candidate manifest, requested/effective backend family and exact model/tokenizer/runtime revisions, raw-score/probability provenance, calibration artifact, prediction, chosen action, grant reference, budget reservation/usage, prefill/incremental/total timing where available, executor state, observed outcome, verifier evidence and later label/attribution revisions.

Write-ahead events precede non-idempotent effects. Completion/receipt and verification are separate events. Outcome labels distinguish ObservedVerified, ObservedUnverified, WeakTeacherLabel, Simulated, Delayed, Censored, and Unknown. A verifier label identifies its artifact and contract, not just a boolean column. Correction creates a linked revision rather than overwriting prior evidence.

## Replay semantics

Reuse the documented AxonHost seam where possible. Record model/tool replies, nondeterministic clocks/RNG, selected actions and completion order sufficient for the supported replay tier. Exact replay means serving recorded responses, not making a fresh remote model call with the same seed.

Declared tiers: exact serialized host/model replay; environment reset plus rerun with tolerance-defined comparisons; and statistical reproducibility across fresh runs. Concurrency and GPU nondeterminism must not be mislabeled byte-identical. When a needed input was never recorded, return unsupported/incomplete replay.

Replaying an external write returns its recorded receipt and does not repeat the effect. Divergence includes changed inputs, missing/excess events or unconsumed required events. Diagnostic metadata identifies the first supported divergence point.

## Data eligibility and privacy

A `DatasetManifest` records source episodes and revisions, legal/owner authority for use, intended purpose, retention, sensitivity, permitted recipients/regions where configured, redaction transform version, task/repository/bug-lineage grouping, split membership, contamination/near-duplicate checks, candidate-construction policy and deletion/deactivation lineage. No eligibility metadata means not eligible for training.

Raw journals require restricted access and protected storage; hexadecimal encoding or hashing is not encryption or anonymity. Redacted review artifacts must not expose secrets through summaries, small-domain hashes or error messages. Remote inference receives only an authorized projection of observations; local audit permission does not imply remote disclosure permission.

Separate tenant datasets and inference caches. Honor retention/deletion policy while preserving a minimal permitted audit tombstone; do not promise deletion from already trained weights without a defined retraining/unlearning procedure. Quarantine model artifacts trained on later-invalidated data until policy resolves their use.

## Error attribution

An `Attribution` contains candidate causes, evidence, confidence/probability origin, tested interventions, implicated component versions and Unknown. Multiple causes may coexist: truncated observation and a miscalibrated router, for example. Do not force a single label.

Use component substitution, replay and controlled ablations to confirm causes. Preserve the distinction between “the generator changed” and “the generator caused the failure.” Update one component per experiment by default; bundled updates need factorial/interaction tests or a justified coordinated protocol.

## Acceptance gates

**G10-replay:** recorded task execution repeats without filesystem/network/model side effects; changed arguments cause visible divergence.

**G10-crash:** an episode interrupted between action and receipt remains OutcomeUnknown until reconciled; no fictitious success label enters training.

**G10-secrets:** seeded secrets in source, environment, prompts and errors remain unavailable in redacted review and learning export; raw-journal access is separately controlled.

**G10-lineage:** every training row points to permitted evidence and split lineage; weak/simulated labels cannot be silently upgraded to real outcomes.

**G10-attribution:** a deliberately multi-causal failure can retain multiple candidate causes/Unknown; a confirmed substitution updates the attribution with supporting evidence.

## Build slices and exclusions

Implement the common event schema and raw/redacted split during the first vertical slice. Add learning export before any training. Sophisticated attribution models follow basic replay; no global automatic retraining in v0.

## v0.3 decision-label and behavior-policy lineage

A successful executed action is not automatically the uniquely correct decision. Learning exports distinguish `semantic_class`, `acceptable_action_set`, `observed_action_outcome`, `comparative_action_value`, and `task_completion`. Unchosen alternatives remain unknown unless independently evaluated or supported by a declared estimator.

Each decision record stores the behavior-policy version and, when known, the probability with which the controller selected the executed action. This selection probability is not the Reflex probability that the action is correct. Historical-trace evaluation of a new policy must state its off-policy assumptions and action coverage; resettable software fixtures should prefer controlled alternative execution when practical.

Multiple acceptable next actions are supported. The corpus must not turn an unchosen but valid inspection/test into a negative example merely because a prior controller chose something else.

Replay also records submitted-versus-effective model input, backend feature manifest, adapter transformations/retries and resolved transitive model/tokenizer/encoder identities where available.

## v0.4 experience federation

Learning/replay manifests add `source_system`, source schema/version, bridge-migration version and source data-use lineage. MiCode/external repository records remain ineligible by default until CX-16/CX-17 policy checks pass. Historical action outcomes can support semantic replay and world-model learning but never authorize repeating the action. Self, external and generated experience retain separate source labels for ablations and contamination analysis even after normalization.

## v0.3 decision-label gates formalized in the spec

**G10-multi-valid:** two independently validated useful next actions can coexist in learning data; the unchosen valid action is not auto-labeled negative.

**G10-behavior-policy:** behavior-policy version and selection probability, when known, are separate from Reflex correctness probability; unobserved alternatives remain unknown absent explicit evidence.

---

<a id="doc-specs-cx-11-crystallization-admission-md"></a>

## specs/CX-11-crystallization-admission.md

---
id: CX-11
title: "Crystallization and independent artifact promotion"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-06", "CX-10"]
first_stage: M5
implementation_evidence: []
---

# CX-11 — Crystallization and independent artifact promotion

## Intent and source basis

Turn repeated successful cognition into cheaper reusable artifacts while preserving independent admission. S2 pp.43–48 describes THINK→REFLEX→RULE and per-pillar improvement. S1 pp.22 and 83–84 supplies restricted compiler-rewrite gates as prior art. See [SOURCES](SOURCES.md).

## Decisive fork

Use multiple guarded specialization paths with an independent admission authority. Crystallization is optional and reversible at the artifact level; it is not a claim that every reasoning task becomes a deterministic rule or that previous real-world effects can be undone.

## Candidate paths

Supported classes: prompt/router policy; calibration layer; retrieval policy; distilled decision adapter; tool/macro; guarded deterministic rule; predictor/representation; and restricted compiler rewrite. Each class declares its training/derivation method and assurance level.

A learned student is empirically compared to task outcomes, not declared equivalent to its teacher. A deterministic rule requires a proved property over a restricted domain, exhaustive finite validation, or clearly labeled empirical support plus fallback. Never label corpus agreement as universal equivalence. Macro composition must preserve prerequisites, intermediate effects, failure recovery and total budget.

## Artifact contract

`CandidateManifest` contains type/version/digest, parent/incumbent, applicability predicate, dependencies, authority/effect requirements, training/data lineage, evaluator policy, quality/calibration claims, resource profile, migration rules, fallback target and rollback/deoptimization procedure.

The bundle includes signed or otherwise authenticated gate evidence from the admission domain. The learner cannot write those reports as though they came from the protected runner. Applicability is evaluated against current state/model/schema/environment; failure deoptimizes to the approved baseline or blocks.

## Admission state machine

`Proposed → ValidatedOffline → Shadow → Canary → Active` with terminal Rejected, Suspended or Retired. A purely offline research candidate may never enter Shadow. Every transition is durable and authority-checked; workers cannot self-promote by updating a status field.

Admission gates are separate and conjunctive: schema/provenance; authority non-expansion; safety/adversarial checks; task-quality non-inferiority; calibration/applicability where used; resource benefit; compatibility/migration; and rollback rehearsal. Required missing/expired/inconclusive gates block.

Shadow mode cannot duplicate external actions; it only compares proposals/predictions with recorded or separately authorized observations. Canary is restricted to owner-approved reversible environments and known resource caps. Production irreversible actions need their own approval policy; they are not enabled by this spec.

## Quality and deoptimization

Use CX-01's preregistered statistical contract, not “no significant difference.” Recalibration, new prompt or tokenization, new candidate construction and new environment may invalidate evidence. Watch realized outcomes, drift, disagreement and error rates under the authorized monitoring policy.

On breach, disable new uses, preserve evidence, switch future eligible work to the baseline and reconcile in-flight actions. Artifact rollback may require state-schema migration or compensation; never assume reverting a binary reverses state or external effects. Keep a tested previous-good bundle and dependency closure.

## Self-modification boundary

A candidate can improve proof search or propose stronger invariants. It cannot edit the trusted proof checker, hidden test corpus, required-gate list, risk classifier, grant issuer or signing identity. Such changes use explicit Axon invariant/TCB governance and rerun incumbent/challenger evidence under both policy versions.

## Acceptance gates

**G11-independent:** a learner-written passing report without the expected admission provenance cannot promote; a policy/schema mismatch also refuses.

**G11-scope:** a compiled rule works within its declared domain and reliably falls back outside it; a changed candidate schema revokes stale applicability.

**G11-noninferiority:** evidence below precision/sample requirements yields INCONCLUSIVE; a fast but quality-regressing candidate fails.

**G11-authority:** a tool/macro or compiler pass requesting additional effects fails even when task score improves.

**G11-rollback:** inject a regression after activation in a reversible fixture; stop new use, restore the previous-good bundle and preserve/correct dependent state without repeating effects.

## Build slices and exclusions

First promote one small deterministic helper or prompt policy using the same admission path later used for learned models. Then train a bounded Reflex student on eligible examples. Automatic general program synthesis and arbitrary compiler self-rewrite are not initial deliverables.

## v0.4 OS-wide crystallization

Candidate origins now explicitly include native Cortex experience, MiCode-exported skills/episodes, external-repository knowledge candidates and generated curricula. Origin never weakens admission. Promotion may target a guarded skill/tool, Reflex/retrieval policy, Axon library abstraction, compiler transformation or runtime primitive. Required assurance increases with semantic/authority blast radius; useful userland artifacts need not be driven native.

---

<a id="doc-specs-cx-12-learning-meta-md"></a>

## specs/CX-12-learning-meta.md

---
id: CX-12
title: "Per-pillar learning, curricula and meta-governance"
status: Draft
authority: Proposed
depends_on: ["CX-08", "CX-09", "CX-11"]
first_stage: M7
implementation_evidence: []
---

# CX-12 — Per-pillar learning, curricula and meta-governance

## Intent and source basis

Realize the source's per-pillar learning idea without allowing learners to rewrite their own acceptance criteria. S2 pp.44–48 provides the lifecycle and attribution ambition; S1 pp.87–91 supplies evidence/governance and invariant-change discipline. See [SOURCES](SOURCES.md).

## Decisive fork

Unify lifecycle interfaces but separate execution, learning and admission authority. “Every pillar can improve” does not mean “every pillar may update itself online” or “the safety boundary is optimized as a task score.”

## Common lifecycle

A component adapter exposes `describe`, `infer_or_execute`, `record_outcome`, `propose_update`, `evaluate`, `request_promotion`, `activate_approved`, and `deoptimize`. Functions may be inapplicable: a parser need not predict a distribution to fit the interface. Activation requires an independent approved artifact receipt.

Every component declares inputs, outputs, trainable parameters, protected code, eligible data, observed failures, quality/cost metrics, update frequency/budget, baseline, holdouts and rollback. Learned changes are pinned during a task epoch. Correlated multi-component updates are staged and explicitly evaluated.

## Pillar register

| Pillar | Permitted learned proposal | Must remain independent |
|---|---|---|
| Observer/retrieval/memory | Projection, ranking, retention recommendation, typed summary | Access control, source provenance, secret export policy |
| Reflex/router | Adapter, calibrator, routing policy | Action authority, mandatory evidence, approved risk envelope |
| Generator/reasoner | Prompt, tool selection, code proposal, strategy | Executable checks and protected task contract |
| World model/planner | Features, predictor, rollout policy, experiment ranking | Real outcome labels and capability limits |
| Critic/evaluator assistant | Heuristic scoring and failure triage | Release evaluator, hidden audit set, promotion thresholds |
| Representation/abstraction | New encodings/concepts | Traceability to evidence and unchanged evaluation policy |
| Capability proposer | Useful bounded macros or candidate pruning | Grant issuance/attenuation and executor checks |
| Proof search/compiler optimizer | Tactics, lemmas, restricted rewrite candidates | Proof kernel, reference semantics, TCB modification process |
| Meta-controller | Experiment allocation, curriculum, proposed policy revisions | Approved policy activation and final audit authority |

## Loop hierarchy

Task execution manages actions; task planning manages subgoals; component learning manages one candidate; portfolio learning chooses experiments/components; architecture governance decides interface/invariant changes. These loops share event lineage but have different budgets and write permissions. Detailed builder and runtime protocols are in [LOOPS](build/LOOPS.md).

A meta-controller uses evidence to choose the next improvement experiment: estimated task bottleneck, expected information value, uncertainty and actual resource spend. It can propose stopping a research track whose marginal value is poor. No mandatory infinite improvement loop exists.

## Curriculum contract

Generated tasks carry generator/version, family/mechanism lineage, difficulty evidence, solvability checks where available, training/audit designation and contamination analysis. Curricula target observed gaps and include adversarial/rare cases, but cannot replace protected externally controlled audit tasks.

Measure transfer beyond generator templates. Evaluation changes are versioned policy proposals; rerun the incumbent under the old and new suites and document why the new suite better measures the intended outcome. Easier tasks cannot silently redefine “improvement.”

## Stop conditions and stability

Each learning/meta job has wall time, token/compute/cost, candidate-count and evidence-access budgets. Stop on insufficient data, unstable labels, budget exhaustion, absent authority, repeated inconclusive trials or increased regression risk. Report the failure and best supported hypothesis without manufacturing a completed milestone.

No component retrains on its own unverified predictions as though they were real outcomes. Synthetic training remains labeled, and high-feedback loops need explicit contamination and stability evaluation.

## Acceptance gates

**G12-contract:** every participating pillar has a complete lifecycle/metric/authority manifest; unsupported lifecycle operations are explicit.

**G12-cause:** controlled component substitution attributes a known fault without falsely updating unrelated pillars; Unknown remains valid when ambiguous.

**G12-meta:** attempts to lower admission thresholds, edit hidden data or self-sign a model through a meta job are denied.

**G12-curriculum:** generator siblings cannot leak into final audit unnoticed; transfers are tested against templates and compute-matched controls.

**G12-stability:** simultaneous conflicting updates are serialized or evaluated together; an episode never silently mixes component versions.

## Build slices and exclusions

After two individually successful learning pipelines, add the common registry and experiment-allocation controller. Architecture-level proposals remain human/governance reviewed. Broad self-improvement claims require longitudinal evidence; this spec delivers mechanisms and tests, not an intelligence verdict.

## v0.4 three-source learning portfolio

Portfolio/meta learning allocates experiments across **self experience**, **external experience** (MiCode/repositories/research) and **generated experience** (curricula/simulations). It may learn which source is useful for which pillar, but cannot lower evidence/admission standards for one source to make progress appear faster. Source mix, contamination risk and transfer are reported explicitly.

---

<a id="doc-specs-cx-13-os-runtime-md"></a>

## specs/CX-13-os-runtime.md

---
id: CX-13
title: "Hosted OS integration, resource control and confinement"
status: Draft
authority: Proposed
depends_on: ["CX-03", "CX-04", "CX-10"]
first_stage: M1
implementation_evidence: []
---

# CX-13 — Hosted OS integration, resource control and confinement

## Intent and source basis

Provide the operating-system responsibilities needed for safe long-running cognition: process authority, isolation, scheduling, persistence, cancellation and recovery. S1 pp.16, 62–63 and 84–88 documents hosted services and unresolved kernel/VM scope. See [SOURCES](SOURCES.md).

## Decisive fork

Adopt staged host tiers. Cortex v0 depends on a tested hosted executor, not an Axon-authored kernel. Native test/build artifacts still run in external confinement; interpreter capability checks do not automatically restrict arbitrary native subprocesses.

## Tiered roadmap

Tier H0: one supported host, single-writer isolated workspaces, no production effects, supervisor-managed model/tool jobs. Tier H1: durable multi-job service, quotas, revocation, observability, recovery and separately approved remote adapters. Tier H2: parity/equivalent-contract host portability, optional microVM and hardware-attestation integration. Tier K: separately approved bare-metal branch with syscall/confinement evidence. Tier K is not an implied deliverable of H0–H2.

Support status is per host/profile and version. A missing sandbox feature refuses real tool execution; a documented mock can run non-consequential fixtures but never pass a production-confinement gate.

## Supervisor and job contract

The trusted parent owns grants, credentials, policy, immutable artifact store, required-gate execution and job state. Untrusted model/generated-code workers receive only a projected input, scoped handles and an ephemeral filesystem view. The parent must not load arbitrary worker code into its own address space.

`JobManifest` binds executable/artifact, principal, input snapshot, working directory, resource reservations, permitted mounts, secret/environment projection, egress policy, deadline, cancellation channel and expected output schema. Restrict descendants as well as the initial process. A sandbox that limits the command string but lets descendants access the host is insufficient.

Reserve aggregate CPU/memory/GPU/time/token/cost/storage budgets where supported. Report estimated versus actual model usage and reconcile with available provider receipts; an estimate alone cannot guarantee exact monetary spending. Hard local deadlines and conservative call/output limits remain enforcement tools. Backpressure bounds queued work and abandoned speculative inference.

## Confinement and secrets

No worker sees the parent's verifier/admission credentials. The environment is an allowlist projection, not inherited ambient state. Mount only permitted content, make protected artifacts read-only, and prevent escape through symlinks, replacement executables, inherited descriptors and shared caches.

Network-denied workloads must be denied at the actual host boundary. For later scoped egress, validate the chosen enforcement mechanism against names, resolved destinations, redirects, proxy behavior and credential forwarding. Do not infer effective network controls from an annotation or an untested container flag.

Source/build dependencies are code. Dependency fetching and builds need separate policies, pinning and isolated artifacts. The initial release uses approved offline fixtures; live package installation is not silently authorized by RunBuild.

## Cancellation and recovery

Cancel propagates from trusted supervisor to all child processes/model requests where possible, prevents new dispatch, records possibly ongoing remote work, and never clears a latched stop without the approved lifecycle. Worker acknowledgement alone is not evidence that it stopped. Test actual termination and resource release under the configured bound.

Restart reconstructs durable jobs/actions and reconciles OutcomeUnknown; it does not replay effects as a recovery strategy. A worker cannot modify the kill latch, supervisor binary or root policy. S1 distinguishes supervisor kill behavior from interpreter flags; preserve that distinction.

## Attestation and portability

Claims identify whether evidence is simulated, local hash/HMAC, or hardware-rooted, and exactly which loaded components/artifacts are covered. Do not call a digest proof that the runtime remained uncompromised or that a decision was correct. Reuse R26–R34 only after their target-specific gates are reproduced.

For each added engine/host, run a contract matrix: isolation, effects, cancellation, replay, resource limits and evidence. Equivalent user-visible guarantees matter more than identical host implementation. Unsupported cases refuse explicitly.

## Acceptance gates

**G13-escape:** adversarial build/test fixtures try host reads/writes, unapproved subprocesses, network access, environment secrets and protected evaluator paths. Allowed operations succeed; denied operations produce no unauthorized realized effect.

**G13-kill:** stuck process trees and slow inference are canceled under the declared bound; no further dispatch or resumable self-clear occurs.

**G13-quota:** concurrent child jobs cannot oversubscribe carved quotas or evade them through restart.

**G13-recovery:** parent/worker crashes at action boundaries produce safe reconciliation and no blind external retry.

**G13-tier:** unsupported/native profiles cannot inherit interpreter-only guarantees; simulated attestation cannot satisfy a hardware-required profile.

## Build slices and exclusions

Implement H0 before untrusted test execution in M1, then operational H1. Desktop UI, drivers, general kernel replacement, multi-VM consensus and hosted commercial service packaging are separate decisions.

## v0.3 external-service adoption

Protected hosted backends must satisfy the dependency/deployment contract in [DEPENDENCY_ADOPTION](build/DEPENDENCY_ADOPTION.md). Example unauthenticated endpoints or permissive demo configurations are research-only until principal authentication, transport/network policy, tenant/cache isolation, budgets, observability and revocation are established.

## v0.3 external-adoption gate formalized in the spec

**G13-adoption:** a protected external backend has pinned source/transitive model/tokenizer/encoder identity, reviewed license/security profile and visible SDK transformations/retries; unauthenticated demo deployment fails protected admission.

---

<a id="doc-specs-cx-14-parallel-model-research-md"></a>

## specs/CX-14-parallel-model-research.md

---
id: CX-14
title: "Parallel decision-model research"
status: Draft
authority: Proposed
depends_on: ["CX-05", "CX-06", "CX-10", "CX-11"]
first_stage: M5
implementation_evidence: []
---

# CX-14 — Parallel decision-model research

## Intent and source basis

Investigate a learned Axon Reflex model only after the workload/interface and admission path exist. S2 pp.7–15 distinguishes runtime engineering from a custom foundation model. W1/W2 are vendor interface descriptions, and W5 is a lead for bounded parallel decoding—not proof of equivalent training or calibration. See [SOURCES](SOURCES.md).

## Decisive fork

Run a learning-curve and inference-bottleneck program, not a mandatory progression toward a particular neural design. Begin a pilot when eligible labeled data exists; there is no arbitrary million-example prerequisite. A custom architecture is pursued only when it addresses a measured quality/cost/latency limit.

## Candidate model stages

R0: existing model with constrained schemas or candidate scoring; establish exact probabilities available versus missing. R1: small standard model/classifier or adapter trained for selected tasks. R2: shared state representation with question- and candidate-conditioned scoring. R3: specialized parallel inference/training only if R2 and simpler baselines leave a demonstrated bottleneck.

These are experiment options, not successively guaranteed releases. One model does not need to handle every task family. A mixture of simple calibrated specialists may be the better measured result.

## Model interface

Input includes structured/unstructured authorized state, typed questions, candidate IDs plus semantic descriptions, field dependencies and schema hash. Outputs conform to CX-05. Dynamic target cardinality requires candidate-conditioned scoring or another explicitly supported mechanism; fixed class heads alone cannot represent arbitrary runtime symbol sets.

A shared encoder/decoder must preserve question isolation where promised and declare dependencies where needed. Memory and compute scale with context, questions and candidates; avoid constant-time claims. Quantization, prefix caching, batching and candidate-token encoding are independently benchmarked and calibration-tested.

## Training data and objectives

Training examples carry approved outcome labels or clearly marked weak teacher labels, task family, candidate-generation policy, model/prompt versions, support weighting and contamination lineage. Teacher disagreement can identify useful examples, not establish the true answer. Preserve abstentions and failed episodes, not only successes.

Begin with appropriate supervised objectives and empirical calibration. A calibration-focused training objective must be stated precisely and evaluated with held-out proper scoring and selective task risk. The term RLCD is not an implementation specification. No proprietary method is assumed reproducible from its name.

Maintain held-out schemas, candidate phrasings/orderings, task families and repositories to test generalization beyond training labels. Control for label leakage through descriptive IDs. Data quantity decisions follow learning curves with uncertainty; include training and serving total cost.

## Inference experiment matrix

Sweep context length, question count, candidate cardinality, token-prefix ambiguity, cold/warm cache, batch size, concurrent load, quantization and device type. Record prefill, decision scoring, queue, serialization and host overhead separately. Compare with a no-probability label-only backend only as a separately identified interface, not an apples-to-apples speed victory.

Initial hardware is the configured available host; no required GPU purchase is encoded here. Report exact hardware/software and measured memory. Remote inference policy and data projection remain CX-10/CX-13 requirements.

## Acceptance gates

**G14-pilot:** a small approved dataset produces a reproducible learning curve against the strongest relevant simple baseline; invalid labels or contaminated splits block the experiment.

**G14-quality:** task and calibration non-inferiority under CX-01 policy, with per-family/OOD results and abstention coverage, passes before cost savings justify release.

**G14-parallel:** real context/cardinality/load benchmarks show whether specialized parallel inference helps; semantic and active-branch conformance match CX-05.

**G14-version:** quantized/retrained/tokenizer-changed artifacts cannot reuse stale calibration or approvals.

**G14-release:** a successful research artifact still passes independent CX-11 admission and rollback rehearsal; a notebook metric cannot self-activate a model.

## Build slices and exclusions

Start with one routing/check-selection task and actual labels. Benchmark first; then train a small adapter; then inspect whether shared-state computation is useful. Frontier-equivalent capability, generalized calibration and a novel foundation model are research questions, not promised milestones.

## v0.3 research sequencing

The M2 bakeoff requires conformance machinery and comparison among feasible existing backend families; it does not require a learned-head implementation in order to justify learned-head research. A lightweight learned option scorer is optional in M2 when the corpus/resources exist. Expanded learned-head/shared-encoder/RLCD-like work remains a later research branch justified by measured workload/serving gaps. `NotEvaluated` is distinct from `Failed`.

## v0.6 model hypotheses from Jev architecture probing

Treat the Jev architecture analysis as hypothesis generation, not source code or a disclosed training recipe. The research matrix now compares at least these bounded-decision families when justified by workload evidence:

1. **Independent option scorer** — score each option conditioned on state/question, then normalize.
2. **Slot-head classifier** — shared representation plus fixed/variable decision heads where appropriate.
3. **Pointer/option-conditioned scorer** — encode candidate objects and select among runtime options.
4. **Listwise option model** — jointly represent the complete candidate list before producing a distribution.

Listwise interaction is a first-class hypothesis because candidate composition/order may change relative odds. It is not assumed correct a priori; compare it against simpler scorers under matched candidate sets and compute budgets.

Causal-decoder versus bidirectional encoder and dense versus sparse-MoE are later research branches only. No build slice may claim the external article identified Jev's exact backbone. Sparse MoE is considered only after a measured dense-model serving bottleneck.

Training pilots optimize/report proper scoring objectives such as negative log likelihood and Brier score in addition to accuracy. Post-hoc calibration remains separate. 'RLCD reproduction' is not a milestone name unless a reproducible public algorithm is actually pinned; this package uses **Outcome-Calibrated Decision Training** as the generic research program.

**G14-listwise:** compare independent, pointer/option-conditioned and listwise candidates on dynamic option sets, option-order perturbations and held-out candidate compositions. Any listwise advantage must survive compute-matched and task-level evaluation.

**G14-training-score:** a training pilot publishes learning curves for accuracy, NLL, Brier, calibration/selective risk and downstream verified task impact; no single metric is sufficient for promotion.

## v0.7 Kev reference implementation findings

`jaredpalmer/kev` is now an executable reference for the Jev-style mechanism rather than only a behavioral reconstruction. Its released research prototype uses a frozen small causal LM, LoRA, a block-causal shared-state/question-branch mask, and a pointer/listwise readout over runtime options. The important update for Cortex is **not** that this exact model should be copied. It is that the mechanism is cheap enough to prototype early, while the harder research problem is transfer beyond the training distribution.

Kev's public results show a large gap between in-source performance and out-of-source transfer. Cortex therefore treats cross-repository/task-family transfer as a primary model gate rather than an optional appendix. A small Axon/MiCode coding model pilot may begin once a clean eligible corpus exists; no million-example threshold is required.

Required v0.7 model experiments:

1. **Kev-style reference arm** — small pretrained backbone + adapter + block-causal isolated branches + pointer/listwise readout.
2. **Transfer-first evaluation** — repository-, repository-family-, task-family- and eventually language-held-out suites; in-repository random splits are insufficient evidence for broad coding use.
3. **Permutation robustness training** — compare augmentation-only, permutation-consistency KL and canonical-order-only controls. Candidate order remains recorded even if robustness improves.
4. **Absent-candidate training** — include explicit `NONE`, `OBSERVE_MORE` and `ESCALATE` control candidates where semantically valid, plus distractor candidates. Softmax over an inadequate set must not force a false semantic action.
5. **Canonical train/serve/replay encoding** — one versioned renderer/encoder owns question/candidate formatting. Any training-specific rendering path is a conformance failure unless explicitly modeled as a distribution shift.
6. **Frozen research suites** — separate training, calibration, development, locked test and transfer partitions with revision/hash provenance. Locked test access is reserved for promoted candidates.
7. **Coding Transfer Frontier** — report quality as distance from the training distribution under fixed quality/coverage/compute constraints, not only aggregate held-out accuracy.

Kev's implementation is an experimental baseline, not proof of TypeSafe's proprietary training method or general coding intelligence. The initial public Kev checkpoint is narrow and explicitly not trained on coding. Cortex must reproduce all mechanism claims on the exact adopted commit/model and evaluate on Axon workloads.

**G14-transfer-frontier:** a candidate that claims general coding utility reports repository-family and task-family held-out results, selective coverage and uncertainty under a frozen transfer suite. Same-repository/random-example performance cannot satisfy this gate.

**G14-permutation-training:** permutation robustness is measured before and after any invariance objective. Improvements must not hide accuracy/calibration regressions, and exact candidate order remains in replay/provenance.

**G14-absent-candidate:** on fixtures where the correct semantic action is absent, the model may select only a registered absence/control outcome (`NONE`, `OBSERVE_MORE`, `ESCALATE` as policy permits), never an arbitrary ordinary candidate forced by normalization.

**G14-canonical-encoding:** training, evaluation, live inference and semantic replay use the same versioned canonical decision encoding or record an explicit transformation with separate calibration. A divergent hidden renderer blocks promotion.

**G14-locked-test:** ordinary training/model-selection workers cannot read the locked-test partition; promoted candidates are evaluated once under the registered release process, with suite/model/code hashes recorded.

---

<a id="doc-specs-cx-15-language-compiler-proof-md"></a>

## specs/CX-15-language-compiler-proof.md

---
id: CX-15
title: "Language, compiler and proof integration"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-04"]
first_stage: M2
implementation_evidence: []
---

# CX-15 — Language, compiler and proof integration

## Intent and source basis

Promote proven runtime concepts into Axon language/compiler support without multiplying inference paths or weakening the interpreter oracle. S1 pp.2–5, 20–21 and 89–91 supplies the extension/invariant discipline and R2a debt; pp.83–85 distinguishes rewrite experiments and specific formal-proof work. See [SOURCES](SOURCES.md).

## Decisive fork

Library/host contracts first; syntax and native lowering only when stable usage and evidence justify them. Reuse the current compiler, runtime and proof infrastructure. Do not require adding a new prover solely because the architecture discussion mentions one.

## Phase L0 — Adapter integration

Introduce typed serializable contracts and Axon library wrappers over a narrow registered host seam. Avoid exposing raw authority pointers or arbitrary native FFI to generated code. Document which engine supports each operation and why. Unsupported operations refuse with a properly allocated stable diagnostic; allocation occurs in the actual repository governance process, not this package.

Conformance fixtures cover input/output types, effects, budget accounting, branching, errors and replay. Existing R44 materialization restrictions remain explicit; persistent action handles are reacquired through the host registry. New raw type guesses in codegen are prohibited.

## Phase L1 — Authoritative types and language surface

Before new cross-engine cognitive semantics, persist/reuse an authoritative expression-ID-to-type map as appropriate to R2a's actual status. Define stable expression identities, invalidation under AST rewrites/imports, ownership lifetimes and propagation through mono/lowering. Audit existing implementation instead of assuming the PDF's reported debt is still unchanged.

Only add syntax when at least two concrete workflows demonstrate that the library form causes repeated measurable errors/overhead or cannot express the required semantics. The syntax must have a formal desugaring into AIR/library operations, diagnostics, effect rules, formatter behavior, reference generation and migration guidance.

Probability/refinement syntax does not create empirical calibration. Typed action references cannot be deserialized into authority without registry validation. New distribution or graph types state their representation and ownership cost rather than silently copying large values.

## Phase L2 — Native and optimizer support

Every implemented native feature must preserve the reference semantics for its supported input domain. For model calls, conformance uses recorded replies; fresh stochastic inference is not expected to be bit-identical. Numeric comparisons must preserve the specified exact or tolerance semantics; never relax a safety predicate merely to make parity pass.

Ambient interpreter constraints require explicit native runtime/launcher support before Cortex enables that target. The lack of a call site is not an excuse for quietly ignoring policy. Capability flags are not an optimization hint and cannot be eliminated by an unproved rewrite.

Compiler optimizations include safe graph scheduling, redundant observation reuse under exact invalidation rules, conditional question fusion and restricted deterministic specialization. Compare output, effects, failure behavior, budgets and trace/provenance requirements—not just stdout or speed.

## Proof integration

A `ProofReceipt` names theorem/property, assumptions, subject artifact digest, checker/version, proof artifact, result and evidence. A proof about an old patch or weaker precondition cannot approve a new patch. Report what is proved and what remains tested or trusted.

Use existing SMT/refinement infrastructure where it fits. Lean integration is optional future adapter work, not documented as shipped. The supplied TLA+/Coq work concerns a specific corrigibility mechanism, not the full compiler/OS. Learned proof search can propose tactics/lemmas; the trusted checker and its assumptions stay protected.

## Acceptance gates

**G15-types:** a corpus exercising new type/ownership/refinement paths uses the authoritative type map; deliberately inconsistent lowering refuses rather than guessing.

**G15-parity:** supported interpreter/native cases agree on semantic output, effects, failures and recorded model behavior; unsupported cases refuse explicitly.

**G15-docs:** generated reference and documented surface update with the change; language primer tests measure model authoring/repair fluency without cherry-picking examples.

**G15-proof:** stale subject hashes, changed assumptions and invalid proofs cannot produce a valid receipt or bypass runtime fallback.

**G15-optimization:** a proposed fusion/specialization preserves budgets, authority and control dependencies, including adversarial and failure paths.

## Build slices and exclusions

The audit/interface phase starts early; deep frontend changes are not a prerequisite to the hosted vertical slice. No formal proof of all Axon semantics, unrestricted self-modifying compiler, mandatory bare-metal kernel, or automatic equivalence of learned policies is claimed.

## v0.3 question-design and fusion checks

Compiler/AIR reviews classify deterministic predicates before creating a Reflex question. Snapshot equality, grant expiry, authenticated test outcomes, and other mechanically decidable invariants stay in deterministic code. Semantic judgments may use Reflex only after the question declares scope, evidence needs, answer-consumption policy and deterministic composition.

A compiler transformation that combines or rephrases model questions is semantics-preserving only under a declared backend contract that substantiates that property; otherwise it is an empirical model/policy transformation and cannot rely solely on replay of old answers as proof of fresh-inference equivalence.

## v0.4 native-promotion boundary

CX-18 may nominate repeated tools/rules/library abstractions for compiler/runtime promotion. Such nomination is not evidence. Any compiler/native candidate still requires authoritative type information, interpreter/reference semantics, parity/refusal behavior, capability/effect monotonicity and applicable proof/invariant gates. The default successful outcome for a learned capability is a guarded userland artifact; native integration is a separately justified optimization.

## v0.6 AIR dependency typing and batching legality

AIR makes cognitive data dependencies explicit so the compiler/runtime can optimize execution without changing semantics:

```text
QuestionDependency =
    Independent
  | ConditionallyRelevant { branch }
  | AnswerDependent { source_question }
```

The compiler may co-schedule `Independent` questions and may speculatively execute `ConditionallyRelevant` branches against one immutable state handle. `AnswerDependent` questions form a stage boundary unless a finite branch expansion is explicitly represented in AIR and remains within registered budgets.

A shared-state/prefix-cache optimization is legal only when it preserves question isolation, candidate manifests, trusted prompt roles, model/tokenizer/preprocessing identity, and answer dependency semantics. Prompt concatenation or question fusion that changes which text can attend to which text is a policy/model transformation and must pass fresh conformance/calibration/evaluation rather than compiler parity alone.

**G15-dependency-types:** the AIR validator rejects cycles/missing dependencies and prevents an `AnswerDependent` question from being scheduled in the same stage as its unresolved source. Conditional speculative outputs cannot be consumed outside their branch.

**G15-batch-semantics:** compiler/runtime batching of isolated/shared-state questions reproduces the registered semantic decision contract within the backend's tolerance; a deliberately fused conversational prompt is identified as a changed policy and cannot pass as a transparent optimization.

---

<a id="doc-specs-cx-16-micode-experience-bridge-md"></a>

## specs/CX-16-micode-experience-bridge.md

---
id: CX-16
title: "MiCode experience bridge and cross-system conformance"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-02", "CX-10"]
first_stage: M1
implementation_evidence: []
---

# CX-16 — MiCode experience bridge and cross-system conformance

## Intent and source basis

Make MiCode a versioned experience/research plane for Cortex without coupling Axon to MiCode internals or importing MiCode authority. MiCode already has the ingredients of a useful coding-world harness—central composition, durable events, tool outcomes, delegation, worktrees and verification—but its permission model, host guarantees and lifecycle remain locally owned. The bridge exchanges evidence and policies, not ambient privilege.

## Decisive fork

Use a schema-versioned bidirectional bridge. Axon consumes canonical episodes, observations, bounded decision records, failure attribution, benchmark bundles and knowledge candidates. MiCode may consume approved Cortex decision policies, Reflex adapters, verifier profiles, capability schemas and ontology/version maps. Neither side trusts the other system's local authority tokens, path grants, risk ceilings or process handles.

Do not link either runtime directly against the other's private structs. The bridge is an artifact/protocol boundary with explicit migrations and conformance fixtures.

## Import contract

Axon-side importable families:

- `CodingEpisodeBundle`: immutable task contract, before/after repository identity, observations, candidate sets, actions, generated artifacts, authorization outcomes, executed effects, verification evidence, budgets/costs and failure attribution;
- `SoftwareObservationBundle`: semantic object catalog plus omissions/truncation/freshness receipts;
- `DecisionDatasetBundle`: question/candidate/result/probability provenance plus behavior-policy lineage;
- `FailureAttributionBundle`: candidate causes, supporting/contradicting evidence and intervention results;
- `BenchmarkBundle`: task/evaluator/environment manifests and all attempts, including failure/cancel/inconclusive;
- `KnowledgeCandidateBundle`: pattern/concept candidate with source provenance, counterexamples, license/data-use constraints and evidence stage;
- `SkillCandidateBundle`: proposed reusable action sequence/tool/library/compiler candidate with applicability and authority requirements.

Every bundle carries producer system/version, schema version, artifact digests, source-system episode IDs, repository/content identities, policy/evaluator versions, sensitivity/use policy, and explicit Unknown/NotObserved states. Absence never becomes a default assertion.

## Export contract

Axon may export only artifacts whose release policy permits external consumption:

- AIR/Cortex decision policies;
- Reflex backend manifests/adapters;
- verifier/acceptance profiles;
- capability/action schemas;
- ontology/concept version maps;
- calibration artifacts valid for the declared domain;
- approved skill/tool specifications.

An exported artifact carries an applicability envelope, version/digest, required evidence, data-classification constraints and fallback. MiCode must still apply its own permission gate and host policy. "Approved by Axon" never means "authorized in MiCode."

## Identity and semantic mapping

MiCode semantic IDs and Axon semantic IDs are local namespaces. Cross-system mapping uses content/structure fingerprints plus source-system IDs and a versioned ontology map. A mapping may be Exact, EquivalentUnderProjection, Approximate, Superseded or Unresolved. Unresolved identity is not silently guessed.

Round-trip conformance preserves distinctions important to learning: denied vs failed vs aborted/not-executed; observed vs predicted; verified vs unverified; selected vs acceptable-but-unchosen; submitted vs effective model input; permission decision vs realized effect; task DONE claim vs independently verified completion.

## Replay and experiment use

Imported MiCode episodes may support:

- exact semantic replay over recorded decisions/results when all necessary artifacts exist;
- counterfactual policy evaluation without repeating external effects;
- resettable re-execution of permitted local fixtures;
- training/evaluation only when CX-10 eligibility permits it.

A recorded MiCode action outcome is historical evidence, not permission to repeat that action in Axon. Missing environment/model/tool inputs downgrade replay support explicitly.

## Security and governance

The bridge strips or transforms local secrets/credentials/authority handles before export. Raw source and logs remain subject to their originating data policy. Repository/license constraints attach transitively to derived knowledge candidates. Import validation runs before any artifact reaches training, inference, admission or runtime registries.

Schema evolution is append/migrate, not silent reinterpretation. Old episodes remain readable through pinned migration adapters. Cross-system schema changes require conformance fixtures on both sides before protected use.

## Acceptance gates

**G16-schema:** a representative coding episode containing denied, failed, aborted, successful and unknown outcomes round-trips MiCode → bridge → Axon reader without semantic field loss or invented defaults.

**G16-authority:** forged MiCode grants/risk ceilings/permission outcomes cannot create an Axon principal grant or bypass an Axon capability check; exported Axon approvals likewise do not authorize MiCode execution.

**G16-lineage:** every imported learning row retains source episode/repository/evaluator/data-use lineage and remains ineligible when required policy fields are absent.

**G16-version:** a schema/ontology/model-version mismatch is migrated by an explicit registered adapter or refused; stale calibration/applicability cannot silently attach to the new version.

**G16-replay:** an imported episode can drive semantic replay/counterfactual comparison without repeating external writes/network/model effects; unsupported replay inputs surface as Unsupported/Incomplete rather than guessed outcomes.

## Build slices and exclusions

Start with JSONL/JSON artifact interchange and a dummy MiCode producer/consumer fixture. Add real MiCode exports only after its canonical episode schema exists. Do not require MiCode availability for Cortex's first local repair loop; the bridge is an additional experience source, not a runtime dependency.

---

<a id="doc-specs-cx-17-external-repository-knowledge-md"></a>

## specs/CX-17-external-repository-knowledge.md

---
id: CX-17
title: "External repository knowledge ingestion and evidence ladder"
status: Draft
authority: Proposed
depends_on: ["CX-09", "CX-10", "CX-16"]
first_stage: M8
implementation_evidence: []
---

# CX-17 — External repository knowledge ingestion and evidence ladder

## Intent and source basis

Let Cortex learn software-engineering patterns from external repositories, including episodes produced by MiCode, without confusing popularity, repetition or teacher opinion with correctness. External experience is one source of hypotheses; local reproduction and protected evaluation decide what Axon may claim or promote.

## Decisive fork

Ingest repositories as structured evidence histories, not as undifferentiated training text. Preserve source provenance, license/data-use constraints, before/change/after relationships, tests/benchmarks/reverts and counterexamples. Promote only through an evidence ladder that is stronger than observation frequency.

## Inputs

Permitted inputs may include source trees, dependency/build manifests, commit history, tests, issues/PR metadata where authorized, benchmarks, release/migration notes, security fixes, reverts, and MiCode canonical episodes. Source availability and data-use rights are explicit per artifact.

Static snapshots are useful but weaker than histories that expose change and outcome. A commit message or merged PR is not itself ground truth; evidence strength depends on reproduced behavior and independent checks.

## Knowledge candidate types

- structural architecture patterns;
- implementation/procedural patterns;
- bug/fix and failure signatures;
- test-selection/verification strategies;
- performance transformations;
- concurrency/resource-lifetime patterns;
- security/invariant patterns;
- API/error-handling idioms;
- reusable representations/abstractions;
- negative knowledge: reverted, deprecated, failed or fragile approaches.

Each candidate stores source examples, non-examples/counterexamples, context predicates, hypothesized mechanism, evidence stage, uncertainty, source/license policy and derived-artifact lineage.

## Evidence ladder

A candidate may advance through:

`Observed → Repeated → OutcomeAssociated → LocallyReproduced → Benchmarked → HeldOutVerified → PromotionEligible`

Stages are not automatic. Many repeated patterns should remain descriptive knowledge. `OutcomeAssociated` is not causal proof. Local reproduction must state the environment and intervention. Held-out verification uses task/repository families not used to construct the candidate.

The system records conflicting evidence rather than averaging it away. A pattern useful in one ecosystem may become a guarded domain-specific abstraction rather than a universal rule.

## Knowledge extraction loop

1. canonicalize permitted repository/history inputs;
2. build semantic objects and change/outcome links;
3. propose candidate patterns/abstractions using retrieval, Reflex and/or deliberate reasoning;
4. search for corroborating and contradictory examples;
5. design local reproductions or controlled experiments when feasible;
6. score compression/prediction/transfer benefit;
7. submit only sufficiently evidenced candidates to CX-11/CX-18 admission paths.

The learner that discovers a pattern does not certify its value. Repository rank/stars/downloads are metadata, not assurance.

## Data governance

Respect license, confidentiality, contributor and dataset-use restrictions. Derived summaries/concepts retain required source attribution or use restrictions. If policy later invalidates an input, dependent training/model/knowledge artifacts become quarantined or reevaluated according to CX-10 lineage; provenance is never discarded because the abstraction looks generic.

## Acceptance gates

**G17-provenance:** a knowledge candidate derived from several repositories can enumerate its exact source artifacts, transformations, versions and data-use constraints; missing lineage blocks promotion eligibility.

**G17-counterexample:** the extractor actively finds/reports counterexamples or records that none were found under a bounded search; a contradicted universal claim is narrowed or rejected rather than averaged into confidence.

**G17-reproduce:** at least one nontrivial candidate reaches LocallyReproduced/Benchmarked through an independently resettable Axon experiment; failure to reproduce lowers evidence rather than being omitted.

**G17-license:** a source marked disallowed for training/derivation cannot enter an eligible dataset or promoted artifact; downstream manifests preserve the restriction.

**G17-heldout:** a candidate claiming transfer or generality is evaluated on held-out repositories/task families with matched baselines; inconclusive evidence remains non-promotable.

## Build slices and exclusions

Begin with read-only extraction from a small, owner-approved repository set and MiCode-exported episodes. First deliverable is a provenance-rich candidate notebook/store, not automatic compiler/runtime modification. Issue/PR ingestion and large-scale crawling remain optional until source/legal policies and dedupe/contamination controls are operational.

---

<a id="doc-specs-cx-18-capability-synthesis-native-promotion-md"></a>

## specs/CX-18-capability-synthesis-native-promotion.md

---
id: CX-18
title: "Capability synthesis and staged native promotion"
status: Draft
authority: Proposed
depends_on: ["CX-11", "CX-15", "CX-16", "CX-17"]
first_stage: M9
implementation_evidence: []
---

# CX-18 — Capability synthesis and staged native promotion

## Intent and source basis

Provide a controlled path by which repeated verified behavior discovered in Cortex/MiCode/external repositories can become a reusable skill, tool, library abstraction, Reflex policy, compiler transformation or runtime primitive. This is the crystallization path that lets the whole Axon stack become more capable over time without turning every learned behavior into trusted native code.

## Decisive fork

Use a staged knowledge/capability ladder with increasing assurance requirements as an artifact moves closer to the trusted/runtime substrate. Promotion is never automatic from frequency or model confidence. Every stage has an applicability guard, evidence manifest, authority/effect declaration and fallback/deoptimization path.

## Promotion ladder

Typical path:

`Episode → Pattern → Concept → Skill → Tool/Procedure → Library Abstraction → Compiler Transform → Runtime Primitive`

Alternative paths are allowed: a concept may directly improve retrieval/Reflex without becoming a tool; a deterministic transformation may become a compiler pass without a learned intermediary. The ladder represents stronger crystallization, not mandatory progression.

## Candidate classes

- declarative skill/workflow;
- bounded semantic tool or macro;
- retrieval/decision/verification policy;
- guarded deterministic rule;
- Axon stdlib abstraction/type;
- restricted compiler rewrite/pass;
- runtime/host primitive;
- Reflex specialization or calibration artifact.

Each class declares what can be learned, what must be verified, what authority it needs and which rollback/deoptimization mechanism is available.

## Synthesis rules

Repeated action sequences may propose a skill/tool only when intermediate preconditions/effects/failures are modeled. Compressing several actions into one capability must not hide authority expansion, budget use or lost observability. A synthesized tool receives no self-declared risk tier; capability/effect classification comes from the trusted catalog/compiler/runtime.

Library/compiler/runtime promotion requires evidence that the abstraction is sufficiently stable, reusable and beneficial under held-out workloads. "Frequently used" is not enough. Compiler/runtime candidates preserve interpreter/reference semantics, effect/capability monotonicity and existing invariants or explicitly propose a governed invariant change.

## Admission and deoptimization

All candidates pass CX-11 independent admission. Lower layers require stronger evidence: an empirical Skill may be allowed with bounded reversible effects; a compiler/runtime primitive requires parity/proof/negative tests appropriate to its semantic blast radius. Applicability mismatch or regression deoptimizes to the previous-good artifact. Real historical effects are never "rolled back" by merely changing the artifact version.

Artifacts originating from MiCode or external repositories retain source lineage through promotion. Axon may reimplement a pattern independently, but the derivation record remains attached if it influenced the design.

## Acceptance gates

**G18-ladder:** one repeated MiCode/Axon action pattern is represented successively as Pattern → guarded Skill/Tool candidate with complete lineage and applicability; no stage transition occurs solely because of frequency.

**G18-authority:** a synthesized skill/tool that would widen effects, path/network scope or principal authority is rejected even if it improves task success/cost.

**G18-equivalence:** a deterministic rule/compiler transform promoted from repeated reasoning is validated over its declared domain using proof/exhaustive/empirical evidence appropriate to the claim; outside the domain it falls back rather than pretending equivalence.

**G18-deopt:** an active promoted artifact with an injected regression or applicability mismatch stops new use and cleanly returns to the previous-good implementation while preserving evidence and dependent state lineage.

**G18-native:** a candidate runtime/compiler promotion cannot enter the trusted/native path until CX-15 parity/proof requirements and relevant architecture invariants pass; a useful userland tool may remain userland indefinitely.

## Build slices and exclusions

First demonstrate Pattern → Skill/Tool with a reversible coding workflow exported from MiCode or generated by Cortex. Native/compiler promotion is optional and only follows measured need plus CX-15 readiness. Do not add parser keywords or runtime intrinsics merely to represent an experiment.

---

<a id="doc-specs-cx-19-intent-compiler-md"></a>

## specs/CX-19-intent-compiler.md

---
id: CX-19
title: "Intent compiler, typed Intent IR and semantic approval"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-01", "CX-04", "CX-11"]
first_stage: M1
implementation_evidence: []
---

# CX-19 — Intent compiler, typed Intent IR and semantic approval

## Intent and source basis

Restore and strengthen Axon's original product thesis: humans and higher-level system components express **intent**, while Axon resolves that intent into a typed, reviewable contract before Cortex/AIR chooses an implementation. The original roadmap already described structured prose → typed `.ax` plus explicit AST review/approval; Cortex needs a distinct Intent IR so ambiguous natural language does not compile directly into executable code or unrestricted self-modification.

## Decisive fork

Natural language does **not** lower directly to `.ax` or to executable AIR. It first lowers into a versioned typed `IntentIR` that separates objective, constraints, preferences, authority requests, budgets, ambiguity, and required evidence. `IntentIR` then lowers to one or more AIR plans and eventually to `.ax`/runtime actions. The approved semantic contract is immutable for an episode; implementation plans may change underneath it only while preserving that contract.

## Representation layers

1. **Natural / structured intent** — human or system language, possibly incomplete or ambiguous.
2. **Intent IR** — typed desired outcome, constraints, preferences, requested authority, budgets, required evidence, unresolved ambiguities and provenance.
3. **AIR** — cognitive/execution plan: Observe/Retrieve/Reflex/Generate/Think/Simulate/Prove/Act/Verify.
4. **Axon program / capability artifacts** — deterministic executable implementation and requested effects.
5. **Execution/evidence graph** — what actually happened, which evidence passed, and how it relates back to the approved intent.

The layers are linked by digests and explicit lowering records. No layer may silently widen the authority or relax the acceptance contract of the layer above it.

## Intent schema

A minimal `IntentIR` contains:

- stable intent ID/version and source/provenance;
- objective(s) and optimization direction(s);
- hard constraints and invariants;
- soft preferences and tie-breakers;
- target scope / semantic objects;
- requested authority/effects and prohibited effects;
- resource and time budgets;
- acceptance/evidence requirements;
- risk / reversibility requirements where applicable;
- ambiguity set, confidence/provenance for each interpretation, and questions requiring resolution;
- assumptions and environmental dependencies;
- parent intent for system-generated/sub-intents;
- approval state and approval artifact digest.

Unknown is distinct from absent, and preference is distinct from constraint. A model-generated confidence does not transform an ambiguity into a resolved fact.

## Human and system-generated intents

The same typed representation accepts:

- human-authored product/task intent;
- Cortex-generated improvement intent;
- verifier-generated investigation intent;
- knowledge-derived optimization hypotheses;
- curriculum-generated training/evaluation intent.

System-generated intents have **no implicit self-authority**. A proposal to improve the compiler, verifier, world model or runtime requests authority and evidence exactly as a human-authored intent does. It cannot alter its own admission policy or acceptance evidence.

## Ambiguity resolution

The compiler must surface materially different interpretations before consequential execution. Resolution may use deterministic parsing, Reflex, Think, retrieval or a human question, but the resulting choice and source are recorded. If multiple interpretations are intentionally allowed, the Intent IR represents the disjunction explicitly and the acceptance contract states what evidence distinguishes/accepts them.

A low-confidence interpretation cannot silently become an executable default. Non-consequential exploration may run under an explicitly bounded development profile, but its outputs remain proposals until the intent is resolved/admitted.

## Semantic approval and rendering

Humans should not need to approve raw AST syntax alone. Axon provides a deterministic semantic renderer that summarizes:

- what outcome is being optimized;
- what may and may not be changed/touched;
- what authority/effects are requested;
- what budgets apply;
- what evidence is required for completion/promotion;
- which ambiguities/assumptions remain;
- how the current AIR/program differs from the previously approved intent.

The typed Intent IR remains the machine contract. Rendered prose is a review surface whose digest references the exact IR version. Editing the IR invalidates prior approval.

## Intent → AIR lowering

Lowering produces one or more candidate AIR graphs constrained by the approved intent. Planning may replan dynamically, but every action must trace to an active goal/constraint/evidence clause. A planner may narrow authority or add stronger checks; it may not remove a required check or widen effects without an explicit new intent/approval event.

`DoneClaim` is evaluated against the Intent IR acceptance contract plus independent verifier evidence. The reasoner cannot redefine success after seeing the outcome.

## Self-optimization through intent

Self-improvement is expressed as typed `ImprovementIntent`, not unrestricted mutation. Example:

```text
ImprovementIntent {
  objective: minimize(compiler.build_time),
  target: infer/type-map subsystem,
  constraints: [preserve(reference_semantics), preserve(capability_monotonicity)],
  requested_authority: [edit(scope), run(registered_checks)],
  required_evidence: [full_suite, parity, benchmark, invariant_review]
}
```

Repository-knowledge or prediction discoveries may propose such intents, but CX-11/CX-18 admission remains independent.

## Acceptance gates

**G19-parse:** representative human/system intents round-trip through `IntentIR`; hard constraints, preferences, unknowns, authority requests and evidence requirements remain distinguishable and provenance-preserving.

**G19-ambiguity:** a prompt with two materially different valid interpretations cannot enter consequential execution until the ambiguity is explicitly resolved or represented as an approved disjunction; low confidence alone never authorizes a default.

**G19-authority:** lowering/replanning that attempts to widen requested effects, target scope or principal authority beyond the approved Intent IR is refused; narrowing remains allowed.

**G19-evidence:** a planner/reasoner attempts to drop a required test/proof/benchmark or declare DONE under a weaker success criterion. Independent completion remains blocked by the original approved contract.

**G19-render:** semantic rendering of an approved intent is deterministic for the same IR/version, exposes requested authority/evidence/assumptions, and changes its digest when the IR changes; stale approval cannot attach to the modified intent.

**G19-trace:** every consequential action/evidence receipt in one vertical episode can be traced back to the active Intent IR clause(s), AIR lowering record and exact approval artifact; orphan consequential actions fail the trace/admission check.

## Build slices and exclusions

First implement the typed schema, parser/lowering adapter and deterministic semantic renderer around the existing `axon intent compile` / review concepts; do not require new language syntax. Demonstrate prose → Intent IR → reviewed contract → simple AIR repair → evidence → explanation. Only later consider surface-language syntax if repeated workload evidence shows it is useful.

---

<a id="doc-schemas-protocols-md"></a>

## schemas/PROTOCOLS.md

# Protocol contracts and state machines

This is implementation-oriented schema notation, not valid Axon source and not a claim that these types are already in the compiler. Stabilize these contracts before writing adapters. JSON may be the initial wire format, with generated schemas added in the actual implementation.

## Shared rules

All externally received values are untrusted and schema-validated. Every envelope has schema/version, message ID, causal parent, run/epoch, producer version and relevant artifact digests. Enums are closed within a schema version; unknown variants refuse rather than default. Absent, null/None, empty and Unknown are not interchangeable. Preserve Option/Result semantics at Axon boundaries.

Canonical serialization defines map ordering, encoding, number representation and which volatile fields are excluded from content digests. Do not accept NaN/infinity or ambiguous duplicate keys in protocol JSON. Content hashes identify artifacts; they do not authenticate issuers. Signatures/registry lookups provide the separate authority check.

## Core type shapes

```text
RunManifest {
  run_id, epoch, principal_ref,
  goal_digest, completion_contract_digest, initial_snapshot_id,
  runtime_digest, toolchain_manifest,
  model_manifests[], schema_versions[], policy_id,
  sandbox_profile_id, budget, effect_grants[], trace_ref
}

Budget {
  max_steps, max_node_attempts, max_no_progress,
  deadline, token_cap, reserved_cost_cap,
  cpu_limit, memory_limit, storage_limit, max_concurrency,
  actual_usage, estimate_provenance
}

WorkspaceSnapshot {
  snapshot_id, parent_snapshot_id?, base_revision?,
  tracked_and_untracked_manifest, dependency_manifest,
  toolchain_environment_manifest, observation_scope, unknown_inputs[]
}

ObservedFact<T> {
  value: T | Unknown(reason), source_ref, source_digest,
  observed_at, extraction_version, trust_class, freshness_policy
}

Observation {
  observation_id, snapshot_id, goal_digest,
  diagnostics[], object_catalog[], relevant_facts[],
  omission_report[], retrieval_refs[], recent_action_refs[], working_state_ref
}

WorkingState {
  subgoals[], hypotheses[], assumptions[], contradictions[],
  experiment_refs[], progress_signature, budgets_remaining
}
```

A partial observer supplies unknown type/parse facts rather than fabricated defaults. An observation may contain untrusted source text; text inside it cannot modify the RunManifest.

## Decision and action types

```text
DecisionRequest {
  request_id, observation_digest, candidate_manifest_digest,
  questions: [Question], dependencies: [FieldDependency],
  max_speculative_cost, schema_version, model_selector
}

Question {
  id, kind: BinaryDecision | Choice | OrdinalScore | Rank,
  definition, candidates: [{id, description}],
  condition?, abstention_allowed, result_required_for_branch?
}

ProbabilityEvidence {
  raw_origin: ProviderReportedDistribution | NativeOptionLogit | SequenceLikelihood | GeneratedEstimate |
              EntropyDerived | EnsembleEstimate | Unavailable,
  raw_scores?: Map<CandidateId, FiniteNumber>,
  derived_probabilities?: Map<CandidateId, NumberInZeroToOne>,
  calibrated_probabilities?: Map<CandidateId, NumberInZeroToOne>,
  calibration_id?: ArtifactId,
  calibration_status: CalibratedEmpirical | Uncalibrated | Inapplicable
}

BackendFeatureManifest {
  adapter_version, backend_family, effective_model_identity,
  supported_question_types[], supported_input_modalities[],
  maximum_questions?, maximum_candidates?, context_limits?,
  available_score_forms[], question_isolation_mode,
  preprocessing_policy_ref, cancellation_semantics, usage_reporting,
  immutable_model_revision_available
}

DecisionBatchResult {
  request_id, results: Map<QuestionId, QuestionResult>,
  attempt_records[], aggregate_usage,
  effective_backend_manifest_ref, effective_input_receipt_ref,
  validation_result
}

QuestionResult =
    ChoiceResult {selected, distribution?, probability_evidence}
  | BinaryProbabilityResult {p_yes?, probability_evidence}
  | OrdinalDistributionResult {distribution, expectation?, probability_evidence}
  | RankResult {ranking, score_evidence}
  | Abstained {reason}
  | Failed {error_class, retryable}

EffectiveInputReceipt {
  submitted_observation_digest, effective_input_digest,
  preprocessing_manifest, tokenizer_revision?,
  omission_or_truncation_report[],
  effective_question_digests[], effective_candidate_digests[]
}

Action =
  Inspect(object_ref)
  | Search(scope_ref, validated_query)
  | ProposePatch(target_refs[], patch_artifact_ref)
  | RunCheck(registered_check_ref)
  | Revert(local_checkpoint_ref)
  | RequestContext(scope_ref)
  | Escalate(reason)
  | DoneClaim(evidence_refs[])
  | Blocked(reason)

GrantRecord {
  grant_ref, issuer, principal_ref, session_id, epoch,
  action_kind, snapshot_id, target_scope, payload_constraints,
  read_scope, write_scope, allowed_effects, budget_reservation,
  expires_at, revoked, parent_grant_ref?
}

ActionRequest {
  action_id, idempotency_key, run_id, epoch,
  snapshot_id, action: Action, grant_ref,
  payload_digest?, expected_read_hashes[], expected_write_hashes[]
}
```

`grant_ref` is resolved in the executor's trusted registry. Possession of a matching string or a candidate ID alone does not authorize use. A selected ProposePatch still needs payload, scope, freshness and actual execution validation. Candidate probability is not an Action field that can bypass those checks.

## Evidence, model and learning contracts

```text
Prediction {
  model_manifest_ref, observation_digest, concrete_action_digest,
  outcome_schema, distributions_or_intervals,
  supported_horizon, applicability, abstention?, assumptions[]
}

ActionOutcome {
  action_id, state, effect_status,
  observed_delta_ref?, receipt_ref?, error?, finished_at?
}

EvidenceReceipt {
  gate_id, gate_version, subject_artifact_digest,
  contract_digest, policy_id, environment_manifest,
  invocation_ref, output_refs[], metrics, intervals,
  result: Pass | Fail | Inconclusive | Skipped | Unavailable,
  verifier_identity, authenticated_receipt_ref
}

LearningRecord {
  episode_ref, action_ref, observation_ref, prediction_ref?, outcome_ref,
  label_kind, label_revision, eligibility_manifest,
  attribution_ref, task_family, split_manifest, redaction_version
}

Attribution {
  candidate_causes[], probability_origin?, confidence_evidence?,
  tested_substitutions[], supporting_refs[], contradicting_refs[],
  status: Unknown | Hypothesized | ExperimentSupported
}

CandidateArtifact {
  digest, class, incumbent_ref, applicability_predicate_ref,
  required_effects, dependency_manifest, dataset_lineage,
  evaluation_policy, evidence_refs[], fallback_ref, rollback_manifest
}
```

## Reflex batch, preprocessing and prompt-role rules (v0.3)

Question results are matched by `QuestionId`, never by array position. Duplicate IDs refuse dispatch; unknown returned IDs fail validation. A missing result required by the selected branch blocks action construction. Failure of an unused speculative result may be tolerated only by a versioned batch policy.

Before dispatch, AIR validates the request against the effective `BackendFeatureManifest`. Fallback is a fresh dispatch that must validate against the fallback manifest. Backend-specific candidate/context limits are not AIR-wide constants.

The `EffectiveInputReceipt` records what the backend actually evaluated. Silent truncation is forbidden; omission requires an explicit projection/omission report or refusal. Calibration applicability is checked against the effective, not merely submitted, input contract.

Only trusted adapter code constructs privileged model instructions. Repository text, logs, candidate descriptions and replayed conversations remain data even if they contain role labels or prompt delimiters. Model-call fusion is considered a policy/model transformation unless the pinned backend demonstrates preservation of the declared question-isolation semantics.

`Choice`, binary probability and ordinal distributions retain distinct semantics. A provider distribution is not mislabeled as native logits; a binary probability is not rounded into a Boolean merely for wire convenience; an ordinal expectation does not replace the underlying level distribution.

## Action state machine

Only the trusted executor advances effect states. `Prepared` precedes effects; `Validated` requires live grant/snapshot checks; `Running` may have effects; `Observed` records actual outcome; `Verified` attaches the appropriate action evidence. Rejected preconditions become Refused. Failed/canceled execution records whether effects occurred. Unknown receipt after possible effect becomes OutcomeUnknown and blocks blind retry.

Task completion is a separate state: Active → CompletionClaimed → VerifiedComplete, or continued Active/Blocked/Failed after evidence. An action Verified state cannot imply the whole goal is done.

## Promotion state machine

Proposed → ValidatedOffline → Shadow → Canary → Active, with Rejected/Suspended/Retired alternatives. Each transition checks authenticated admission evidence for the candidate and policy version. Shadow cannot perform duplicate real actions. Active artifact changes create a new digest/version, not an in-place mutable model.

## Compatibility rules

Breaking schema changes require a new version and explicit migration. Old traces retain their schemas and replay adapters. Recalibration/applicability evidence is invalidated when relevant prompts, tokenization, candidates or model versions change. Missing required fields or unknown enum variants never silently select an older permissive mode.

## Contract tests to generate in the implementation

Generate success/failure cases for round-trip serialization; unknown/duplicate keys; non-finite probabilities; empty candidate sets; stale digests; mismatched active branches; wrong principal/session; expired grants; duplicate external requests; missing receipts; revoked calibration; protected-data export; altered proof assumptions; and schema migration/replay. Link each generated fixture to CX-00 through CX-15 rather than treating schema validation alone as full product assurance.

## Cross-system experience and knowledge contracts (v0.4)

```text
ExperienceEnvelope {
  schema_version, producer_system, producer_version,
  artifact_id, artifact_digest, created_at,
  source_repository_refs[], source_episode_refs[],
  data_use_manifest_ref, sensitivity_class,
  ontology_version, migration_history[], payload_kind, payload
}

CodingEpisodeBundle {
  task_contract_ref, before_snapshot_ref, after_snapshot_ref?,
  observation_refs[], candidate_manifest_refs[], decision_refs[],
  action_refs[], authorization_outcomes[], generated_artifact_refs[],
  evidence_refs[], completion_status, budgets_and_usage,
  attribution_ref?, omitted_or_unknown_fields[]
}

SemanticObjectMap {
  source_system, source_object_id, source_digest,
  axon_object_ref?, relation: Exact | EquivalentUnderProjection | Approximate | Superseded | Unresolved,
  mapping_version, evidence_refs[]
}

KnowledgeCandidate {
  candidate_id, kind, statement_or_structure,
  scope_predicate, source_examples[], counterexamples[],
  hypothesis_or_mechanism?, evidence_stage:
    Observed | Repeated | OutcomeAssociated | LocallyReproduced | Benchmarked | HeldOutVerified | PromotionEligible,
  source_policy_refs[], confidence_evidence?, reproduction_refs[], derived_artifact_refs[]
}

SkillCandidate {
  candidate_id, action_graph_ref, applicability_predicate,
  required_effects, required_capabilities, intermediate_checks[],
  failure_recovery, source_knowledge_refs[], evaluation_refs[], fallback_ref
}
```

Bridge imports never deserialize foreign authority handles into local grants/principals. Exported Axon artifacts carry applicability and data-disclosure policy, not executable authority. Unknown semantic-object mappings remain unresolved.

The knowledge/capability ladder is represented as lineage, not mutable in-place upgrades. Pattern→Skill→Tool→Library→Compiler/Runtime creates new artifacts with new IDs/digests and stronger gate receipts; prior evidence remains queryable.

## Intent contracts (v0.5)

```text
IntentIR {
  schema_version, intent_id, intent_version, source_kind,
  source_artifact_refs[], parent_intent_ref?, provenance_refs[],
  objectives: [Objective], hard_constraints: [Constraint],
  preferences: [Preference], target_scope: [SemanticObjectRef],
  requested_authority: [AuthorityRequest], prohibited_effects: [Effect],
  budgets: BudgetSet, required_evidence: [EvidenceRequirement],
  assumptions: [Assumption], ambiguities: [Ambiguity],
  risk_reversibility_policy?, approval_state, approval_artifact_ref?
}

Ambiguity {
  ambiguity_id, statement_ref, interpretations[],
  materiality, evidence_refs[], resolution_status,
  resolved_interpretation_ref?, resolved_by?, resolution_artifact_ref?
}

IntentApproval {
  intent_digest, renderer_version, rendered_contract_digest,
  approver_identity, approved_at, policy_version,
  allowed_profile, supersedes_approval_ref?
}

IntentLoweringRecord {
  intent_digest, air_graph_digest, lowering_version,
  clause_to_nodes: Map<IntentClauseId, [AirNodeId]>,
  added_stronger_checks[], narrowed_authority[],
  unresolved_clause_refs[], planner_manifest_ref
}

IntentCompletionReport {
  intent_digest, air_graph_digest, execution_episode_ref,
  objective_results[], constraint_results[],
  authority_exercised[], evidence_clause_results[],
  unresolved_items[], verifier_receipts[], final_status
}
```

Natural/system prose is not executable authority. `IntentApproval` binds an exact `IntentIR` digest/version. Any modified clause invalidates the prior approval unless an explicitly reviewed migration says otherwise. AIR lowering may add stronger checks or narrow authority but cannot delete required evidence or widen requested authority without a new intent/approval event. `DoneClaim` becomes `VerifiedComplete` only when the independent completion report satisfies the approved Intent IR.

## v0.6 Reflex runtime protocol additions

```text
StateHandleReceipt {
  handle_id,
  state_digest,
  effective_input_digest,
  backend_id,
  model_revision?, tokenizer_revision?, adapter_revision,
  preprocessing_manifest_digest,
  tenant_or_principal_scope,
  created_at, expires_at?,
  reuse_mode: Native | RemoteOpaque | EmulatedNoReuse,
  prefill_or_encode_usage?
}

QuestionDependency =
    Independent
  | ConditionallyRelevant { branch_id }
  | AnswerDependent { question_id }

CandidateManifest {
  candidate_set_digest,
  generation_policy_id,
  generation_policy_version,
  ordered_candidate_ids[],
  order_policy,
  order_digest,
  randomization_seed?,
  omitted_or_unauthorized_candidates[]
}
```

A `DecisionBatchResult` references the exact StateHandleReceipt (or equivalent single-call receipt), question dependency declarations and CandidateManifest digests. Derived confidence summaries include a formula identifier and never replace the primary distribution/provenance record.

## v0.7 Reflex canonical encoding and control-candidate additions

```text
CanonicalDecisionEncoding {
  encoding_version,
  state_projection_digest,
  question_schema_digest,
  candidate_manifest_digest,
  candidate_order_digest,
  reserved_token_policy,
  backend_preprocessing_receipt?
}

ControlCandidate = NONE | OBSERVE_MORE | ESCALATE | BLOCKED
```

Control candidates are registered by the candidate compiler for decision families that permit them; the model cannot mint one. Their policy semantics are deterministic and external to the model. Training, evaluation, live inference and semantic replay reference the same `encoding_version`; a backend-specific semantic rewrite must mint a distinct effective-input/calibration record.

---

<a id="doc-examples-safe-repair-episode-md"></a>

## examples/SAFE_REPAIR_EPISODE.md

# Worked vertical slice: repair one Axon function

This is a proposed test fixture and protocol walkthrough. It was not executed. It uses familiar Axon syntax from the supplied language examples, but no new Cortex API is claimed to compile today.

## Fixture

A permitted local copy contains a semantic bug:

```text
fn add(a: i64, b: i64) -> i64 { a - b }
```

The visible task is to make `add` satisfy its addition contract. A visible test exposes one failing case. Protected hidden checks include multiple positive, negative and zero inputs within a defined non-overflow range; they also verify that only the approved function body changed. Hidden tests and verifier code are outside the worker's readable/writeable workspace. Do not claim those finite cases prove correctness for every integer input.

The baseline uses the same model, source, allowed tools, primer, budget and hidden checks without the new cognitive scheduler. The deterministic golden path uses a fixed proposed patch; its result is conformance evidence only.

## Episode

1. Supervisor pins the run manifest, task contract, workspace snapshot, toolchain, policy, resource limits and model versions. The allowed profile has no network mutation, production deploy or self-merge.
2. Observer identifies `add` and the failing diagnostic/test output, preserving the source digest and any unknown facts. Candidate catalog includes Inspect(add), ProposePatch(add), RunCheck(approved_check), DoneClaim and Blocked.
3. Registry issues session/principal/snapshot-bound grants. These references are not executable commands. A grant to edit `add` cannot change tests or a sibling file.
4. Reflex or the baseline model chooses Inspect or ProposePatch. Any probability is recorded with its origin and calibration status; authorization does not depend on that probability.
5. Generate proposes `a + b` as a patch artifact. Executor parses and validates its exact scope. If the payload instead changes tests or emits a shell script, it refuses.
6. In the M3 version only, the world model predicts the approved check's outcome from this exact patch and toolchain digest. A prediction that lacks support abstains; it cannot certify success.
7. Executor prepares and atomically checks expected hashes. A concurrent modification invalidates the transaction and sends the task back to observation.
8. Host applies the patch to the owned copy and executes the registered check under actual process confinement. Build scripts/subprocesses remain untrusted.
9. The action result and observed delta enter the event log. The agent may claim Done, but protected hidden checks and the completion contract decide whether the task is complete.
10. A passing evidence receipt names the final patch and environment. The output is an approved-to-review patch artifact, not an automatic merge or production deployment.
11. Exact replay serves recorded model and tool responses and performs no real check or file/network effect. Mutating the recorded action arguments causes divergence.
12. A separately authorized redacted export may supply a verified training record. Raw source/secret-bearing journal bytes are not automatically eligible.

## Mandatory negative variants

The same fixture must cover stale snapshot; forged cross-session grant; patch outside the symbol body; changed completion contract; hidden verifier modification; hostile source comment; native worker trying to read a host secret; missing required check; process timeout; crash after effect before receipt; and an agent claiming Done after failing checks.

Each negative variant asserts the exact refusal/reconciliation state and absence of unauthorized realized effects. A refusal counts as working enforcement, not a solver crash. It also counts toward coverage/task economics when the system cannot finish the task.

## Crystallization extension

Later, repeated verified fixes of a genuinely equivalent restricted transformation may motivate a guarded tool. Its applicability predicate must exclude ambiguous arithmetic, overflow assumptions and cases requiring broader semantic reasoning. Finite training agreement alone does not justify a global compiler rewrite. Admission and fallback remain CX-11 responsibilities.

---

<a id="doc-templates-work-package-md"></a>

## templates/WORK_PACKAGE.md

# Work package template

Status: Draft / Not started. Replace placeholders before execution; do not interpret them as defaults.

## Identity and scope

Record work-package ID; linked CX/R requirement IDs; repository commit; upstream slices; owner/integrator; permitted paths; protected paths; and one-sentence behavior change.

## Evidence and decisive fork

Record prior art inspected, exact current failure/limitation, the competing designs, chosen design, rejected alternative and why. Identify what is documented versus reproduced. Attach the red/negative fixture or specify how to create it safely.

## Contract

Define inputs/outputs, types, versions, authority, effects, resource costs, error/refusal states, concurrency/recovery, engine/host support, migration and rollback. List non-goals explicitly.

## Implementation steps

Define the smallest sequenced edits, shared-interface coordination and checks after each edit. No broad opportunistic refactor. New parser/type/codegen work requires the compiler gate plan; new host effects require actual confinement tests.

## Acceptance

List required gate IDs, concrete test fixtures, exact command locations after implemented, environment and expected outputs/statuses. Distinguish mocked conformance from real enforcement. Define quality/calibration/coverage/resource margins where relevant before seeing results.

## Budget and stop rules

Set maximum wall time, steps, tool/model calls, retries, compute/cost/storage, candidate submissions and no-progress threshold. State immediate safety/secret/recovery stop conditions and escalation owner.

## Evidence after execution

Record actual commands, statuses, logs/reports, candidate and policy digests, baseline comparisons, failed attempts and unproven assumptions. No gate can be marked PASS from its planned name. Link the review and admission decision separately from the author's implementation claim.

---

<a id="doc-templates-spec-template-md"></a>

## templates/SPEC_TEMPLATE.md

# Cortex specification template

Use the repository's actual governance template on import. Package metadata is only a staging format; map it to the real spec-meta schema and reserve official IDs through the existing registry.

```yaml
id: CX-NEW
status: Draft
authority: Proposed
depends_on: []
implementation_evidence: []
```

## Intent and source basis

State the requirement and exact source/measurement. Separate attachment claims, reproduced evidence and new proposals.

## Decisive fork

Name the alternatives, chosen path, rationale and revisit condition. Do not hide a product/TCB decision inside implementation detail.

## Required contracts

Define types, errors, effects, authority, nondeterminism, state transitions, budgets, concurrency, privacy, compatibility and rollback. List what the subsystem cannot guarantee.

## Integration

Identify existing Axon seams, exact supported engines/hosts, type/reference changes, and dependencies. Proposed paths remain labeled until confirmed.

## Acceptance gates

For each gate, include a positive fixture, adversarial/negative case, observable result, allowed uncertainty, executable command after implementation, and artifact/version binding. Missing required evidence blocks completion.

## Build slices and exclusions

Break work into reviewable units with exact outputs and prerequisites. Keep research experiments and optional platform work off the core dependency path unless evidence makes them necessary.

## Open decisions and evidence

List genuine unresolved choices, safe blocked behavior, owner and evidence needed. Do not use “complete” until actual executable evidence exists under the repository's governance.

---

<a id="doc-glossary-md"></a>

## GLOSSARY.md

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

---

<a id="doc-sources-md"></a>

## SOURCES.md

# Sources, scope, and evidence boundaries

Prepared: 2026-09-18. This package reviews the preceding Axon roadmap and proposes an implementable replacement. It does not audit a checked-out Axon repository, execute Axon, reproduce model benchmarks, or approve any architecture change.

## Evidence vocabulary

**Documented** means an uploaded source states something, not that this review reproduced it. **Observed conflict** means two passages in the supplied material disagree or have different scopes. **Proposed** means a new recommendation in this package. **Verified-package** is reserved for checks on these Markdown files. **Verified-product** requires a future executable acceptance run against an identified repository commit and environment.

Every specification is Draft. Proposed API names, paths, type signatures, operation tags, and diagnostic identifiers are design contracts, not existing Axon syntax. The package's `CX-*` identifiers are provisional and do not reserve Axon's `R*`, diagnostic, or exit-code namespaces.

## S1 — Axon architecture and reference

User attachment: `axon-docs.pdf`, 94 physical pages. The architecture section reports measurements dated 2026-09-17; other bundled sections have different dates and contain historical text. Page references below use physical PDF pages, which match the printed numbers.

| Pages | Relevant evidence |
|---|---|
| 2–8 | Interpreter reference semantics; parity/refusal; repeated type inference; R2a type-map debt; capability/effect/sandbox/replay layers; R44 materialized sessions; status-vs-evidence warnings. |
| 10–12 | Small fluency experiments and withdrawn broader benchmark interpretation; limited ecosystem; native FFI boundary and trusted code. |
| 15–16 | Journals can contain secrets; ambient effects are interpreter-only; `AXON_REQUIRE_CERTS` behavior; audit identity differs from authority; supervisor kill variable is not an interpreter kill mechanism. |
| 20–24 | Refinement and effect implementation descriptions; userland versus kernel primitives; risk pipeline; absence of a gate function described as open; narrow world/counterfactual examples. |
| 26–54 | Generated command and builtin surface; reference signatures are useful but do not independently prove end-to-end guarantees. |
| 48 | Scoped sandbox semantics, including empty scope meaning unscoped, not deny-all. |
| 60 | Reported tests and a known message-parity failure; not rerun here. |
| 62–64 | Product split, AST approval, userland runtime, and explicit reversal of the earlier no-kernel direction. |
| 77 | Token-cost accounting still described as using an estimate rather than response-derived usage. |
| 83–88 | MDL/world prototype; attestation scope; R33/R34 partial work; R36–R40 proposed or specialized directions; R39 evidence/governance graph. |
| 89–94 | Invariants and exit ledger; I-11 wording conflicts with the newer FFI discussion; program exit status is not an authenticated attestation of enforcement. |

Important unresolved contrasts: phase-complete labels do not imply a general learned world model; risk-gate implementation is less strict than the broad pipeline slogan; the OS ambition was reversed in one section but remains described as out of scope elsewhere. This package records these contrasts rather than silently choosing whichever passage is most convenient.

## S2 — Architecture discussion supplied by the user

User attachment: `Jev Arch(1).pdf`, 48 physical pages. This is a prior conversation/design discussion, not the vendor's implementation specification. BFlow-specific portions are excluded from this package's scope.

| Pages | Relevant evidence |
|---|---|
| 1–15 | Bounded semantic judgments; deterministic composition; distinction between adopting a programming pattern and inventing a foundation model. |
| 18–33 | Summarized browser pattern: observed objects, indexed action choices, speculative branch-specific questions, separate generation, stale-state validation, independent completion verification. |
| 33–44 | Representation search, hypotheses, active experiments, abstraction, and crystallization as proposed routes toward transferable problem solving. |
| 44–48 | Per-pillar learning lifecycles, attribution, and repeated discoveries becoming reusable capabilities. |

Several pages contain unavailable-image placeholders. No hidden content in those placeholders was inferred. Browser implementation and performance claims in this discussion are not independently reproduced here.

## S3 — The roadmap being reviewed

The preceding assistant response in this conversation: milestones M0–M10; suggested R45–R61 documents; nested execution/learning/meta loops. It is a proposal, not evidence that features exist. `REVIEW.md` identifies amendments to that proposal.

## External primary-source checks

These checks clarify narrow technical points; they do not replace S1/S2 as the basis for the package. Accessed 2026-09-17.

### W1 — TypeSafe announcement

TypeSafe, its own System One/Jev announcement, dated 2026-09-15. URL: `https://typesafe.ai/blog/introducing-system-one-models-and-jev`.

The vendor describes a custom model/sampler/training stack. Its workflow reference targets include other models' answers; its schema guarantee is not an independent demonstration of correct decisions. This package imports no vendor latency, cost, or accuracy target.

### W2 — TypeSafe public API introduction

URL: `https://docs.typesafe.ai/introduction`.

Documents Choice, Score, Noul, and isolated parallel questions against shared state. This supports the bounded-question interface pattern. It does not establish that errors across questions are statistically independent or disclose a reproducible training recipe.

### W3 — Calibration research

Guo, Pleiss, Sun, Weinberger; ICML 2017, PMLR 70. URL: `https://proceedings.mlr.press/v70/guo17a.html`.

Provides empirical evidence that neural prediction probabilities can be miscalibrated, and studies post-processing calibration. Our workload-specific calibration contracts and promotion rules are proposed engineering policy, not guarantees furnished by that paper.

### W4 — Model-based planning research

Janner, Fu, Zhang, Levine; NeurIPS 2019, arXiv version 3 revised 2021. URL: `https://arxiv.org/abs/1906.08253v3`.

Studies model bias and short simulated rollouts rooted in real data. Our software-world evaluation and planning rules are proposals; the paper does not establish those rules for compilers or operating systems.

### W5 — Small-model parallel decoding reference

Author-published model card. URL: `https://huggingface.co/harshatheg/Qwen-2.5-1B-RLCD`.

Describes shared-prefix/KV-cache parallel bounded-field inference. The card's title does not establish reproduction of TypeSafe's RLCD, and normalizing logits does not by itself establish calibration. Treat it as an experimental implementation lead, not a verified baseline or an endorsed dependency. No code or weights from it are bundled.

## S4 — User-provided Jev/System-One ecosystem survey (2026-09-18)

The user supplied a launch-week ecosystem map covering official TypeSafe SDKs/adapters, community Jev/OpenJev implementations, calibration/benchmark projects, and bounded-action agent examples. It is treated as **design input**, not as independently reproduced evidence in this package.

Patterns incorporated into the build process:

- stable System-One-like API with interchangeable backends;
- official-adapter pattern for comparing ordinary LLMs against the same typed decision shape;
- direct option-logit and sequence-probability scoring;
- variable runtime candidate sets and option-conditioned learned scorers;
- shared-state/prefix-cache parallel decision experiments;
- dynamic indexed action spaces and separate decision versus generation;
- calibration/error benchmarks before consequential automation;
- grouped split/leakage controls and shuffled-context tests;
- ecosystem claims must be rerun on Axon workloads before adoption.

Representative links supplied by the user include `typesafe-ai/typesafe-sdk-js`, `typesafe-ai/typesafe-sdk-python`, `typesafe-ai/system-one-adapter-python`, `TheoLeeCJ/openjev`, `ekzhang/openjev-sglang`, `bnsd55/jevmlx`, `vinnylarouge/jevlike`, `daseinlabs/open-jev`, `Heman10x-NGU/Verdict-open-jev`, `browser-use/jev-ultrafast`, `shiftynick/jev-axi`, `AbdelStark/jev-benchmarks`, and `abhixhek/jevcal`. This package does not assert their current behavior or benchmark values without a future source/reproduction pass.

## Naming

**Axon Cortex** is a proposed working name for the architecture. **Axon Reflex** is its bounded typed-decision component. **AIR**, in this package, is explicitly defined as **Axon Intelligence Representation**, the typed cognitive execution graph. This expansion is a new naming decision, not attributed to the attachments. The name is not a trademark or domain-availability clearance. “Jev” remains only a source/vendor attribution, not the name of an Axon component.

## v0.3 ecosystem-review claim boundaries

The System-One/Jev ecosystem is used as a source of interface, serving, calibration and agent-loop implementation ideas. Community README benchmark numbers are not Axon evidence unless reproduced under matched Cortex task/evaluation contracts. Official TypeSafe primitive semantics should be checked again at implementation time against the pinned SDK/docs version. Open implementations may have backend-specific candidate/context limits, preprocessing/truncation and deployment defaults; those become adapter/adoption metadata rather than AIR-wide assumptions.

Relevant repository classes for intake include official TypeSafe SDK/adapter; OpenJev-style direct logit/prefill implementations; sequence scorers; MLX/local variants; learned option-conditioned scorers; calibration/agent-failure benchmarks; dynamic browser/computer-use loops; guard/triage/search/tree patterns. Each individual dependency still requires commit/license/security/transitive-artifact review under `build/DEPENDENCY_ADOPTION.md`.

## S5 — MiCode platform and Axon-support build plan (2026-09-18)

User-supplied `MICODE_PLATFORM_MANUAL.pdf` describes MiCode's current architecture and explicitly distinguishes current/intended/partial/deferred behavior. Relevant design facts for the bridge include a central composition root, one resolved-call permission choke point, durable persistence, delegation/worktrees/verification, and the explicit absence of an execution sandbox in the current snapshot. These are MiCode-side facts and must not be projected onto Axon guarantees.

The companion user-directed `MiCode_Axon_Support_Build_Plan_v0_1.zip` is a proposal package defining MiCode as a coding-world/experience plane for Axon. Its MX-01 canonical episode, MX-10 repository knowledge, MX-11 crystallization and MX-12 bridge concepts motivate CX-16–CX-18. They are design inputs, not evidence that MiCode or Axon has implemented the bridge.

## v0.5 source note — intent architecture

The supplied Axon architecture/reference is the basis for the claim that the project already intended a structured-prose → typed Axon → review/approve/deploy product path and treated the typed AST as the audit artifact. CX-19 is a proposed refinement of that direction, not a claim that a distinct Intent IR is already implemented.

### W6 — Jev architecture behavioral probing (secondary analysis)

Archer Hume, `Jev's Architecture Unmasked?`, user-supplied URL accessed in this conversation: `https://archerhume.com/posts/jevs-architecture-unmasked/?v=3`.

The article reports behavioral probes consistent with shared-state processing, sibling-question isolation, strong batched-question scaling, candidate-list/order interactions, and direct distribution readout; it proposes possible causal-transformer/MoE/training explanations. This package treats the measured behaviors as hypotheses worth reproducing and the architecture reconstruction as **non-authoritative**. It does not claim the article reveals TypeSafe's exact model architecture or RLCD recipe.

v0.6 incorporates only the robust engineering consequences: split Reflex Runtime from Reflex Model, formalize state handles and question dependencies, make candidate ordering part of the calibration domain, prioritize listwise candidate models in the bakeoff, and treat distributions/proper scoring as first-class. Causal-decoder and MoE details remain optional research arms.

### W7 — Kev: executable Jev-style decision-model reconstruction

Jared Palmer, `jaredpalmer/kev`, model card and implementation inspected 2026-09-18. URLs: `https://github.com/jaredpalmer/kev/blob/main/MODEL_CARD.md`, `https://github.com/jaredpalmer/kev/blob/main/README.md`, and the pinned repository files used during this review.

Kev provides a working research implementation of the architecture hypothesized in W6: causal pretrained backbone, lightweight adaptation, block-causal state/question isolation, branch-local positions and a pointer/listwise softmax readout with no autoregressive answer generation. The released model card reports a laptop-scale 0.5B prototype, exact/near-exact packed-vs-separate isolation tests, option-order sensitivity, and a large degradation on sources outside its training distribution. The repository's current training code includes optional permutation-consistency and ordinal proper-scoring objectives plus frozen-suite/locked-test research infrastructure.

This package treats Kev as an **executable reference and experimental lead**, not proof of TypeSafe's proprietary architecture/training or a production coding model. Its published model is narrow, English-only and not trained on coding; all quality, latency and transfer claims must be reproduced on Axon/MiCode workloads before adoption. v0.7 uses only the engineering implications: move a small learned reference pilot earlier, prioritize transfer, make candidate-absence/permutation training explicit, unify train/serve/replay encoding, and adopt frozen train/calibration/dev/locked-test/transfer suite discipline.

---

<a id="doc-build-status-md"></a>

## build/STATUS.md

# Package and product status

Prepared on 2026-09-18.

| Item | Status |
|---|---|
| Review of supplied roadmap and documents | Completed as a document review; not a source-code audit. |
| Proposed Cortex/Reflex naming | Used throughout the package; not trademark-cleared. |
| Specifications | Draft; implementation evidence empty. |
| Work-package DAG | Proposed; all tasks Not started. |
| Product acceptance gates | Defined, not implemented or run here. |
| Existing Axon tests/benchmarks | Not executed by this package. |
| Vendor/model claims | Narrow primary-source checks recorded in SOURCES; no performance reproduction. |
| Package validator | See the generated `package_validation.json` for the actual most recent document-only check. |
| Actual repository changes | None. |

## 2026-09-18 build-plan revision

The user supplied a broad Sept-2026 Jev/System-One ecosystem survey. The build package now incorporates its **architectural patterns** without accepting repository performance/training claims as reproduced evidence. Changes include a stable backend-neutral Reflex ABI; four backend families; dynamic option-set corpus construction; shared-prefix/prefill benchmarking; destructive state/candidate controls; typed uncertainty provenance; grouped split/leakage rules; Verified Utility at Coverage; earlier error attribution; question-decomposition experiments; and optional custom-model research only after the bakeoff exposes a real need.

New build documents: `REFLEX_BACKEND_BAKEOFF.md`, `REFLEX_DECISION_CORPUS.md`, and `REFLEX_CALIBRATION_LAB.md`.

## Next action

Import the package into the real Axon repository through its existing governance; begin B00/B01. Status can advance only with evidence from that repository/environment. Preserve this initial status as historical provenance rather than retroactively implying work had already shipped.

## 2026-09-18 v0.4 design revision

Added MiCode experience federation and OS-wide knowledge crystallization as Draft-only proposals: CX-16 bridge, CX-17 repository knowledge, CX-18 staged capability/native promotion; build guides for the bridge, knowledge ingestion and self-optimizing OS loop; tasks B44–B51. No MiCode/Axon bridge code, repository mining, skill promotion or native self-optimization was executed by this package.

## 2026-09-18 v0.5 intent-first revision

Added CX-19 and `build/INTENT_COMPILER.md`. The package now distinguishes natural/system intent, typed Intent IR, AIR, executable Axon artifacts and realized evidence as separate representations. It defines semantic approval, ambiguity handling, intent-clause lineage and self-improvement through `ImprovementIntent`. These are proposals only; no intent compiler/runtime gates were executed.

## 2026-09-18 v0.6 Reflex-runtime revision

Added `build/REFLEX_RUNTIME.md`; formalized shared-state handles, question-isolation and candidate-order conformance; added AIR dependency classes; expanded model research to independent/pointer/listwise candidates with proper scoring; added B58–B61 and seven proposed gates. These remain Draft build contracts; no model/runtime product benchmark was executed by this package.

## v0.7 documentation update

Added Kev-derived model-research contracts: early Kev-style coding Reflex pilot, repository/task-family transfer gates, candidate-absence augmentation, permutation-consistency experiments, canonical train/eval/serve/replay encoding, frozen train/calibration/dev/locked-test/transfer suites, and Coding Transfer Frontier reporting. These remain Draft build contracts; no Axon model was trained or benchmarked by this package.

---

<a id="doc-changelog-v0-3-md"></a>

## CHANGELOG_v0_3.md

# Axon Cortex build package v0.3 — changes

This build-focused package folds the latest System-One/Jev ecosystem review into the v0.2 build contracts.

## New files

- `build/REFLEX_CONFORMANCE.md`
- `build/DEPENDENCY_ADOPTION.md`

## Major changes

- Batch Reflex results keyed by question ID with primitive-specific result types.
- Choice, binary/Noul-compatible and ordinal/Score-compatible semantics remain distinct.
- Added provider-reported distribution provenance.
- Added backend feature negotiation and effective-input/truncation receipts.
- Added trusted prompt-role boundary and question-isolation/model-call-fusion tests.
- Added candidate absence/search semantics and candidate-recall-before-ranking evaluation.
- Added multiple acceptable actions, behavior-policy lineage and unknown counterfactual handling.
- Added exact coverage/selective-error accounting and benchmark comparability review.
- Added transitive dependency/model/tokenizer/encoder/dataset/evaluator adoption manifests.
- Fixed M2 sequencing: learned-head backend is optional research in M2, not a self-justifying prerequisite.
- Added B42 Reflex conformance and B43 dependency adoption work packages plus new acceptance gates.

---

<a id="doc-changelog-v0-4-md"></a>

## CHANGELOG_v0_4.md

# Changelog — v0.4

## Why this revision exists

The MiCode→Axon support plan clarified that MiCode should serve as a real-world coding experience/experiment plane for Axon, while Cortex remains the cognitive runtime and Axon remains the verified execution substrate. It also clarified the larger product thesis: the whole Axon stack should learn from self experience, external repositories/MiCode, and generated curricula, then crystallize verified patterns into cheaper/more reliable artifacts.

## Added

- CX-16 — MiCode experience bridge and cross-system conformance.
- CX-17 — external repository knowledge ingestion and evidence ladder.
- CX-18 — capability synthesis and staged native promotion.
- `build/MICODE_EXPERIENCE_BRIDGE.md`.
- `build/KNOWLEDGE_INGESTION.md`.
- `build/SELF_OPTIMIZING_OS.md`.
- build slices B44–B51.
- cross-system/knowledge/capability protocol types and gates.

## Changed

- top-level objective now explicitly describes Cortex as the cognitive control plane for a self-optimizing Axon language/compiler/runtime/OS;
- roadmap extends through M8 experience federation/repository knowledge and M9 OS-wide crystallization;
- loops now include experience federation, repository knowledge and crystallization loops plus adaptation timescales;
- learning data explicitly preserves source-system labels and does not inherit foreign authority;
- crystallization may target skills/tools/libraries/compiler/runtime with increasing assurance requirements;
- task manifest now includes the previously documented B42/B43 slices as well as the new B44–B51 slices.

## Still intentionally unchanged

- M0–M1 safe local repair path does not depend on MiCode;
- Reflex/custom-model research remains optional;
- external repo popularity is not evidence of correctness;
- a learner still cannot redefine its own verifier/admission policy;
- native/compiler promotion remains optional and separately gated.

---

<a id="doc-changelog-v0-5-md"></a>

## CHANGELOG_v0_5.md

# Changelog v0.5 — intent-first Cortex integration

Prepared 2026-09-18. Documentation/build-plan update only; no Axon product gates executed.

## Added

- **CX-19 — Intent compiler, typed Intent IR and semantic approval.** Natural language/system proposals now lower first to a typed desired-outcome contract, not directly to executable AIR/`.ax`.
- **build/INTENT_COMPILER.md** with vertical slices, negative cases and inner/outer/meta loops.
- Intent/evidence lineage protocols and gates: `G19-parse`, `G19-ambiguity`, `G19-authority`, `G19-evidence`, `G19-render`, `G19-trace`.
- Work packages **B52–B57** for source mapping, Intent IR, ambiguity resolution, semantic approval, AIR lowering, and self-improvement-through-intent.

## Changed

- Cortex is explicitly **intent-first**: `Natural intent → Intent IR → AIR → Axon program/actions → evidence graph`.
- Human and system-generated self-improvement requests share one Intent IR and authority/admission path.
- `Done` semantics bind to the approved Intent IR acceptance/evidence clauses; replanning may change method but cannot weaken success criteria.
- Semantic review becomes the primary human UX over the typed contract; raw AST remains inspectable but is no longer the only meaningful approval representation.
- M1 now includes the basic intent contract/lowering slice; advanced self-generated improvement intent is staged after admission/crystallization is available.

## Not claimed

No natural-language parser quality, intent compilation success rate, UI, new syntax, or self-improvement behavior was implemented or measured by producing this package.

---

<a id="doc-changelog-v0-6-md"></a>

## CHANGELOG_v0_6.md

# Changelog v0.6 — Reflex runtime and listwise decision refinement

Prepared 2026-09-18. Documentation/build-plan changes only.

## Added

- `build/REFLEX_RUNTIME.md`.
- `StateHandle` / shared-state runtime contract and backend-emulation semantics.
- Question scheduling classes: `Independent`, `ConditionallyRelevant`, `AnswerDependent`.
- Candidate-order policy/digest as part of inference and calibration scope.
- Question-isolation conformance and shared-state identity/isolation gates.
- Listwise/pointer/independent model research matrix.
- Outcome-Calibrated Decision Training terminology and proper-scoring requirements.
- Work packages B58–B61 and gates G05-state-handle, G05-question-isolation, G05-order-domain, G14-listwise, G14-training-score, G15-dependency-types, G15-batch-semantics.

## Preserved

- Backend-neutral Reflex ABI.
- Dynamic action spaces and DECIDE ≠ GENERATE ≠ ACT.
- Independent verification/admission.
- Intent IR, world-model, MiCode bridge, knowledge ingestion and self-optimizing OS roadmap.
- Custom model architecture remains optional and evidence-gated.

## Explicit non-claims

The Archer Hume article is behavioral analysis and architectural inference. v0.6 does not claim TypeSafe uses a causal decoder, sparse MoE, listwise transformer, or any specific RLCD implementation.

---

<a id="doc-changelog-v0-7-md"></a>

## CHANGELOG_v0_7.md

# Changelog v0.7

v0.7 incorporates the executable `jaredpalmer/kev` reference implementation and model card.

## What changed

- moved a small Kev-style Axon Reflex pilot earlier, once an eligible coding corpus exists;
- shifted model-research emphasis from reconstructing the Jev mechanism to cross-repository/task transfer;
- added repository-aware train/calibration/development/locked-test/transfer suite discipline;
- added typed candidate-absence/control training (`NONE`, `OBSERVE_MORE`, `ESCALATE`, `BLOCKED` where policy permits);
- added permutation-consistency training experiments while preserving order provenance;
- established one canonical decision encoding for train/eval/serve/replay;
- added Coding Transfer Frontier reporting;
- added B62–B68 and G14 transfer/permutation/absence/encoding/locked-test gates;
- documented Kev as an executable reference, not proof of TypeSafe's proprietary architecture or general coding intelligence.

No Axon runtime/model gate was executed by this documentation update.
