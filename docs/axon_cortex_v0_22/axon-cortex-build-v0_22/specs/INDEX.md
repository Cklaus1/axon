# Axon Cortex spec index — v0.22

Generated from `spec_manifest.json`. All specifications are Draft proposals. CX-35 remains reserved in this package namespace (the source uses that ID for Reflex serving; see integration/SOURCE_OWNER_MAP.json); CX-36 does not create a second CX-23 runtime; CX-37 owns the ACE provider/runtime contract.

| ID | Specification | Contract dependencies | First contract stage |
|---|---|---|---|
| CX-00 | [System contract and trust boundaries](CX-00-system-contract.md) | none | M0 |
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
| CX-24 | [Semantic Perception and Retrieval: schema-conditioned extraction and proposition scoring](CX-24-semantic-perception-retrieval.md) | CX-02, CX-05, CX-10, CX-20, CX-22 | M2 |
| CX-25 | [Reflex Learning Plane: separated learner, sampler and guarded model publication](CX-25-reflex-learning-plane.md) | CX-10, CX-11, CX-20, CX-21, CX-22 | M7 |
| CX-26 | [Decision Composition Runtime: select, project and compose typed world artifacts before generating new content](CX-26-decision-composition-runtime.md) | CX-02, CX-04, CX-05, CX-11, CX-19, CX-24 | M2 |
| CX-27 | [Semantic Supervisor Plane: independent semantic oversight around active cognitive workers](CX-27-semantic-supervisor-plane.md) | CX-01, CX-04, CX-10, CX-11, CX-19, CX-21 | M2 |
| CX-28 | [Semantic Working-Set Manager: context selection, semantic GC and recomputable-memory control](CX-28-semantic-working-set-manager.md) | CX-02, CX-04, CX-10, CX-13, CX-19, CX-24, CX-27 | M2 |
| CX-29 | [Reflexive Self-Application Plane: self-observation, challenger generation and guarded self-improvement](CX-29-reflexive-self-application-plane.md) | CX-01, CX-10, CX-11, CX-12, CX-20, CX-21, CX-22, CX-25, CX-27, CX-28 | M1 |
| CX-30 | [Repository Improvement Plane: governed self-improvement for arbitrary projects](CX-30-repository-improvement-plane.md) | CX-01, CX-03, CX-10, CX-11, CX-16, CX-19, CX-21, CX-29 | M1 |
| CX-31 | [Cross-Project Learning Plane: transfer, pattern mining and promotion across repository families](CX-31-cross-project-learning-plane.md) | CX-09, CX-10, CX-11, CX-17, CX-22, CX-30 | M6 |
| CX-32 | [Universal Evidence Graph: typed provenance from intent to verified claims](CX-32-universal-evidence-graph.md) | CX-00, CX-01, CX-02, CX-04, CX-10, CX-11, CX-19, CX-29, CX-30 | M1 |
| CX-33 | [Causal and Active Experimentation Plane: intervention, information gain and mechanism discovery](CX-33-causal-active-experimentation-plane.md) | CX-01, CX-07, CX-08, CX-09, CX-10, CX-20, CX-21, CX-29, CX-32 | M4 |
| CX-34 | [Cognitive Scheduler and Shared Capability Registry: route work across heterogeneous intelligence units](CX-34-cognitive-scheduler-capability-registry.md) | CX-03, CX-04, CX-06, CX-11, CX-13, CX-18, CX-22, CX-23, CX-24, CX-26, CX-28, CX-31, CX-32 | M2 |
| CX-36 | [Neural Program Compiler, Artifact Format, and Runtime Contract](CX-36-neural-program-artifact-contract.md) | CX-00, CX-10, CX-11, CX-13, CX-15, CX-19, CX-23, CX-32, CX-34 | M1 |
| CX-37 | [ACE Provider Runtime, Execution Plan, and Cognitive Result ABI](CX-37-ace-provider-runtime.md) | CX-00, CX-03, CX-04, CX-05, CX-06, CX-10, CX-11, CX-13, CX-23, CX-28, CX-32, CX-34, CX-36 | M2 |

A spec dependency requires its relevant contract, not completion of every experiment. The [task DAG](../build/TASKS.md) and [release slices](../build/IMPLEMENTATION_RELEASE_SLICES.md) define implementation ordering.
