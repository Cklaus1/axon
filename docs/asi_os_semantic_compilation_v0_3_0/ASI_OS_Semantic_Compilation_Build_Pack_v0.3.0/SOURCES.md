# Sources, Provenance and Unsupported Assumptions

## Basis and authority
The user's conversation supplies the high-level intent → semantic compilation → execution → evaluation/repair → learning thesis. `SPEC.md` is a proposed integration contract, not evidence that the live ASI OS already implements it.

**S01 · Supplied Axon Cortex v0.23 archive.** `Axon_Cortex_Build_Files_v0_23(1).zip`, SHA-256 `3fd2834a17ad7a7cb339c99d2b79c853d4ecb57a27585d7f7edf5928c62405d4`. This is the primary architecture source for v0.3 alignment. Reviewed owners include CX-19 IntentIR, CX-04 AIR, CX-03 authority, CX-13 isolation/effects, CX-21 evaluation, CX-11/CX-18 admission, CX-29 strategy/self-application, CX-32 evidence, CX-33 experiments, CX-34 resources and CX-16/MiCode workflow. The package's validator and seven unit tests passed when run with `python -B`; captured outputs are in `reports/`.

**S02 · Axon Forge v0.3 embedded dependency.** Nested Forge v0.3 archive, SHA-256 `e126f400a40829303b0007c2a0a6de4da219de7f7df7bf8c06e21f7c4718baa7`. Forge is treated as canonical owner of software/program semantics (SIR, SemanticDiff, Transform/TransformResult, MetaExecution, Migration, BuildProgram/BuildGraph, backend/diagnostics/bootstrap/truth), not of grants, global evidence, experiment identity or physical resource truth. Its included bundle validator passed; captured output is in `reports/upstream_forge_v03_validation.txt`.

**S03 · Supplied BFlow documentation.** `source_context/BFLOW_AGENTS_COMPLETE_DOCUMENTATION.md`, copied without modification from the project source. Its intent-first architecture and evaluator/red-team concepts remain relevant. Its agent-first policy can conflict with deterministic compiled execution and therefore needs an explicit product-policy decision rather than silent reinterpretation.

**S04 · Semantic Compilation v0.2 baseline.** The prior build-pack SHA-256 is `c082f0c81331b4e81acc00b589eb58e4f6222add2285f93b903fd05ece0b6b2b`. v0.3 is a breaking architectural alignment: prior local AI-IR authority becomes a portable Semantic Plan projection, and semantic operation is separated from execution mechanism.

### Source-derived versus proposed
The owner boundaries above are source-derived from the supplied Cortex/Forge packages. The portable Semantic Plan schema, CompilationRecord, provider-neutral evaluation envelopes, immutable targeted-repair protocol, adaptive compiled/interpreted policy, and operator CLI are **Semantic Compilation proposals** designed to bind those owners. The package does not claim live compatibility until real adapters execute against the repository.

## Public primary references checked on 2026-10-05
Links are reference material, not runtime dependencies. Only concise paraphrases are used; no third-party documentation is bundled.

**S05 · JSON Schema, Draft 2020-12 / Validation.** Basis for structural schema vocabulary; structural conformance does not establish authorization or task correctness.
```text
https://json-schema.org/draft/2020-12
https://json-schema.org/draft/2020-12/json-schema-validation
```

**S06 · TypeSafe introduction.** Describes Jev as returning typed structured decisions, with Choice, Score and Noul primitives. It does not describe a prose/code repair generator.
```text
https://docs.typesafe.ai/introduction
```

**S07 · TypeSafe Choice and Score.** Documents option distributions and rubric scores, with provider confidence as a separate field. These meanings are preserved rather than collapsed into generic success probability.
```text
https://docs.typesafe.ai/primitives/choice
https://docs.typesafe.ai/primitives/score
```

**S08 · TypeSafe Noul.** Documents a probability for a yes/no proposition and no separate Noul confidence field.
```text
https://docs.typesafe.ai/primitives/noul
```

**S09 · TypeSafe Confidence and AI primer.** The Confidence page defines provider confidence as a statistic of the returned distribution and recommends domain-specific testing. The AI primer describes calibration across groups of predictions rather than an individual-answer guarantee. Task-specific acceptance/calibration governance in this pack is a proposed engineering control.
```text
https://docs.typesafe.ai/confidence
https://docs.typesafe.ai/introduction/machine-learning-primer
```

**S10 · Python jsonschema validation documentation.** Basis for the local Draft202012Validator and registry-based validation harness. Versions in `requirements.lock` are the versions actually used for package checks, not a claim that they are universally preferred.
```text
https://python-jsonschema.readthedocs.io/en/stable/validate/
```

**S11 · Anthropic, Building effective agents.** Related primary engineering material on workflows, agents and evaluator-optimizer patterns. The package's trust, IR and integration contracts are its own proposals, not an implementation or endorsement by Anthropic.
```text
https://www.anthropic.com/engineering/building-effective-agents
```

## Prior research link: interpretation, not provider specification
The earlier project discussion referenced Di Zhang's RLCD/Jev essay. It was read for context but its reconstruction of the training objective is **not adopted as an established implementation fact**. This build pack specifies a provider-neutral typed-decision adapter, not Plackett–Luce training, model weights or a reproduced RLCD system.
```text
https://di-zhang-llm.github.io/blog/what-is-rlcd-the-secret-behind-jev/
```

No Jev pricing, speedup, model accuracy, live Cortex compatibility or ASI OS performance claim is established by this package. All example decisions, usage and demo outputs are explicitly synthetic.
