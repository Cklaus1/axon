# Changelog v0.9 — cognitive specialization and neural programs

This revision incorporates the ProgramAsWeights design review into the Cortex build plan without adopting PAW as a dependency or claiming its hosted compiler is reproduced.

## Added

- **CX-22 — Cognitive Specialization Compiler**: recurring cognition can specialize into a rule/template, specialized Reflex model, typed Neural Program, tool/procedure, general Reflex, or remain THINK/GENERATE.
- **CX-23 — Neural Program Runtime and Skill Compiler**: typed learned-function artifacts, immutable compiler/base/adapter/template/runtime identity, local/offline target semantics, typed output validation, shared-base cache and no-silent-fallback rules.
- `build/COGNITIVE_SPECIALIZATION.md`.
- `build/NEURAL_PROGRAMS.md`.
- work packages **B74–B83**.
- acceptance gates **G22-* and G23-***.
- protocol records for `CognitiveFunctionProfile`, `SpecializationCandidate`, `NeuralProgramManifest` and invocation receipts.

## Changed

- CX-11/CX-18 crystallization now includes specialized Reflex and Neural Program as userland learned-artifact destinations before native/compiler promotion.
- CX-20 can compare general Reflex, specialized models, neural programs and deterministic alternatives under one governed lab.
- CX-14 explicitly allows specialized encoder/token-scorer research without treating a task-specific winner as the universal Reflex model.
- the self-optimizing OS ladder is now `THINK/GENERATE → GENERAL REFLEX → SPECIALIZED COGNITION → RULE/LIBRARY/COMPILER/RUNTIME`.
- artifact identity and fallback semantics are stronger: a missing adapter/program cannot silently become unadapted-base inference while retaining specialized claims.

## Evidence boundary

The PAW public SDK/docs are used as implementation inspiration for compiled learned functions and artifact discipline. This package does not claim Axon can reproduce PAW's hosted compilation process, quality, program sizes or inference performance. All Cortex product gates remain NOT_RUN.
