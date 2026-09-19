# Specification index

All specifications are Draft proposals with no product evidence yet. Dependencies name contract dependencies; implementation is sliced in [TASKS](../build/TASKS.md). CX IDs do not reserve live Axon R IDs.

| ID | Specification | Depends on | First stage |
|---|---|---|---|
| CX-00 | [System contract and trust boundaries](CX-00-system-contract.md) | — | M0 |
| CX-01 | [Evaluation, baselines and assurance evidence](CX-01-evaluation.md) | CX-00 | M0 |
| CX-02 | [Software observations, state and working memory](CX-02-world-observation.md) | CX-00 | M1 |
| CX-03 | [Dynamic capabilities and transactional execution](CX-03-capabilities-executor.md) | CX-00, CX-02 | M1 |
| CX-04 | [AIR graph and interpreter-first integration](CX-04-air-runtime.md) | CX-00, CX-03 | M1 |
| CX-05 | [Axon Reflex typed inference and speculative questions](CX-05-reflex-inference.md) | CX-01, CX-04 | M2 |
| CX-06 | [Calibration, risk-aware routing and abstention](CX-06-routing-calibration.md) | CX-01, CX-05 | M2 |
| CX-07 | [Predictive software world models](CX-07-predictive-world.md) | CX-01, CX-02, CX-10 | M3 |
| CX-08 | [Planning, hypotheses and controlled experiments](CX-08-planning-experiments.md) | CX-03, CX-06, CX-07 | M4 |
| CX-09 | [Representation search and transferable abstractions](CX-09-representation-abstraction.md) | CX-07, CX-08 | M6 |
| CX-10 | [Replay, learning data and error attribution](CX-10-replay-learning-data.md) | CX-00, CX-01, CX-02 | M1 |
| CX-11 | [Crystallization and independent artifact promotion](CX-11-crystallization-admission.md) | CX-01, CX-06, CX-10 | M5 |
| CX-12 | [Per-pillar learning, curricula and meta-governance](CX-12-learning-meta.md) | CX-08, CX-09, CX-11 | M7 |
| CX-13 | [Hosted OS integration, resource control and confinement](CX-13-os-runtime.md) | CX-03, CX-04, CX-10 | M1 |
| CX-14 | [Parallel decision-model research](CX-14-parallel-model-research.md) | CX-05, CX-06, CX-10, CX-11 | M5 |
| CX-15 | [Language, compiler and proof integration](CX-15-language-compiler-proof.md) | CX-00, CX-04 | M2 |

| CX-16 | [MiCode experience bridge and cross-system conformance](CX-16-micode-experience-bridge.md) | CX-00, CX-02, CX-10 | M1 |
| CX-17 | [External repository knowledge ingestion and evidence ladder](CX-17-external-repository-knowledge.md) | CX-09, CX-10, CX-16 | M8 |
| CX-18 | [Capability synthesis and staged native promotion](CX-18-capability-synthesis-native-promotion.md) | CX-11, CX-15, CX-16, CX-17 | M9 |

| CX-19 | [Intent compiler, typed Intent IR and semantic approval](CX-19-intent-compiler.md) | CX-00, CX-01, CX-04, CX-11 | M1 |

| CX-20 | [Reflex Research Lab: reproducible decision-model experimentation and admission evidence](CX-20-reflex-research-lab.md) | CX-01, CX-05, CX-06, CX-10, CX-11, CX-14 | M2 |

| CX-21 | [Coding Frontier / Benchmark Lab: protected whole-system capability measurement](CX-21-coding-frontier-benchmark-lab.md) | CX-01, CX-10, CX-11, CX-12, CX-16, CX-19 | M1 |
| CX-22 | [Cognitive Specialization Compiler: compile recurring cognition into cheaper guarded representations](CX-22-cognitive-specialization-compiler.md) | CX-10, CX-11, CX-14, CX-20, CX-21 | M5 |
| CX-23 | [Neural Program Runtime and Skill Compiler: typed learned functions as guarded Axon capabilities](CX-23-neural-program-runtime-skill-compiler.md) | CX-11, CX-13, CX-18, CX-22 | M5 |

| CX-24 | [Semantic Perception and Retrieval](CX-24-semantic-perception-retrieval.md) | CX-02, CX-05, CX-10, CX-20, CX-22 | M2 |
| CX-25 | [Reflex Learning Plane: separated learner, sampler and guarded publication](CX-25-reflex-learning-plane.md) | CX-10, CX-11, CX-20, CX-21, CX-22 | M7 |

## v0.8 research-lab refinement

CX-20 formalizes the Reflex Lab as permanent governed research infrastructure: frozen suites, experiment/run/artifact registries, canonical encoding, mechanism tests, transfer-first evaluation, reproducibility classes and CX-11 evidence handoff.

## v0.6 Reflex refinement

No new CX spec was added. CX-05, CX-14 and CX-15 now carry the shared-state runtime, listwise-model and AIR dependency-scheduling contracts. `build/REFLEX_RUNTIME.md` is the implementation guide.

## v0.7 model-research refinement

No new CX spec was added. CX-05 and CX-14 now include canonical train/serve/replay encoding, typed absence/control candidates, early Kev-style reference-model research, transfer-first evaluation and new G14 gates. Build tasks B62–B68 implement the research/evaluation slices.

## v0.8 coding-frontier refinement

CX-21 creates the protected whole-system Coding Frontier / Benchmark Lab. CX-20 asks whether a Reflex model/runtime is better; CX-21 asks whether the complete coding intelligence actually became better under fixed task, authority, evidence and resource contracts.

## v0.9 cognitive-specialization refinement

CX-22 generalizes specialization beyond one Reflex model: recurring cognition may compile into a rule/template, specialized decision model, neural program, tool/procedure or remain general Reflex/THINK. CX-23 defines typed learned-function artifacts over pinned shared bases/runtimes, with local/offline execution, immutable dependency closure, no silent base-model fallback and outputs that remain data rather than authority.


## v0.10 semantic perception and learning-plane refinement

CX-24 formalizes schema-conditioned perception and proposition-level semantic retrieval as learned layers below Reflex. CX-25 separates learner and sampler planes so continuous/asynchronous training produces candidates without granting training code authority to self-promote. A protected completion critic is added under CX-21 without changing verifier ownership of DONE.

| CX-26 | [Decision Composition Runtime: select, project and compose typed world artifacts before generating new content](CX-26-decision-composition-runtime.md) | CX-02, CX-04, CX-05, CX-11, CX-19, CX-24 | M2 |

## v0.11 decision-composition and semantic-alignment refinement

CX-26 adds SELECT/PROJECT/COPY/COMPOSE as a guarded alternative to open-ended generation when the world already contains the required pieces. CX-20 now also treats question/criteria/decomposition/candidate policy as independently versioned research artifacts and supports uncertainty-plus-random-audit semantic alignment loops without automatic promotion.

| CX-27 | [Semantic Supervisor Plane](CX-27-semantic-supervisor-plane.md) | CX-01, CX-04, CX-10, CX-11, CX-19, CX-21 | M2 |
| CX-28 | [Semantic Working-Set Manager](CX-28-semantic-working-set-manager.md) | CX-02, CX-04, CX-10, CX-13, CX-19, CX-24, CX-27 | M2 |

## v0.12 semantic-control-plane refinement

CX-27 adds an independent semantic supervisor around active workers while preserving deterministic intervention, permission and completion authority. CX-28 treats context as a managed semantic working set with protected pins, semantic GC, recompute contracts, stale-decision rejection, effective-context receipts, cache-aware model routing and an observable cognitive cascade.

| CX-29 | [Reflexive Self-Application Plane](CX-29-reflexive-self-application-plane.md) | CX-01, CX-10, CX-11, CX-12, CX-20, CX-21, CX-22, CX-25, CX-27, CX-28 | M1 |

## v0.13 reflexive self-application refinement

CX-29 makes self-improvement an immediate instrumentation and experimentation concern rather than a late feature. Every non-kernel cognitive component becomes observable/versioned/replayable/replaceable; challengers begin offline and in shadow, while protected authority, admission, locked evaluation, verifier semantics, provenance and rollback remain outside ordinary self-modification.

| CX-30 | [Repository Improvement Plane](CX-30-repository-improvement-plane.md) | CX-01, CX-03, CX-10, CX-11, CX-16, CX-19, CX-21, CX-29 | M1 |
| CX-31 | [Cross-Project Learning Plane](CX-31-cross-project-learning-plane.md) | CX-09, CX-10, CX-11, CX-17, CX-22, CX-30 | M6 |

## v0.14 repository-optimization refinement

CX-30 generalizes reflexive improvement to arbitrary owner-approved repositories through immutable `ProjectImprovementContract`s, project-local baselines, isolated challengers, project-specific evidence and tested rollback. CX-31 mines governed project episodes for reusable patterns but requires family-aware lineage, counterexamples, held-out repository transfer and scope-qualified promotion before knowledge becomes shared MiCode/Axon capability.


| CX-32 | [Universal Evidence Graph](CX-32-universal-evidence-graph.md) | CX-00, CX-01, CX-02, CX-04, CX-10, CX-11, CX-19, CX-29, CX-30 | M1 |
| CX-33 | [Causal and Active Experimentation Plane](CX-33-causal-active-experimentation-plane.md) | CX-01, CX-07, CX-08, CX-09, CX-10, CX-20, CX-21, CX-29, CX-32 | M4 |
| CX-34 | [Cognitive Scheduler and Shared Capability Registry](CX-34-cognitive-scheduler-capability-registry.md) | CX-03, CX-04, CX-06, CX-11, CX-13, CX-18, CX-22, CX-23, CX-24, CX-26, CX-28, CX-31, CX-32 | M2 |

## v0.15 evidence/causal/scheduler refinement

CX-32 unifies intent→observation→decision→action→evidence→claim→verification lineage into one typed evidence graph. CX-33 makes controlled intervention and information-gain-driven experimentation first-class. CX-34 turns the growing set of learned/deterministic cognitive units into an authorized shared registry plus OS-like scheduler that optimizes quality, latency, cost, privacy, risk, hardware and cache state without weakening hard authority constraints.
