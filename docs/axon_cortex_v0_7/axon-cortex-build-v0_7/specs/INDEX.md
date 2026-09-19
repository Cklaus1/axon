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

## v0.6 Reflex refinement

No new CX spec was added. CX-05, CX-14 and CX-15 now carry the shared-state runtime, listwise-model and AIR dependency-scheduling contracts. `build/REFLEX_RUNTIME.md` is the implementation guide.

## v0.7 model-research refinement

No new CX spec was added. CX-05 and CX-14 now include canonical train/serve/replay encoding, typed absence/control candidates, early Kev-style reference-model research, transfer-first evaluation and new G14 gates. Build tasks B62–B68 implement the research/evaluation slices.
