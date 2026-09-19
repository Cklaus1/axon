# Neural programs: learned-function build guide

This guide implements CX-23 and the neural-program target of CX-22/CX-18.

## Why this exists

Reflex answers bounded questions. Some recurring fuzzy functions instead transform or extract data. A neural program compiles such a stable semantic contract into a small learned artifact that can execute locally over a shared base or standalone model.

## First vertical slice

Use a low-risk function such as:

`raw build log -> DiagnosticSummary`

Requirements:

1. define exact typed input/output schemas;
2. collect eligible verified examples and counterexamples;
3. define a simple non-neural/general-model baseline;
4. compile/train one small learned artifact;
5. generate an immutable NeuralProgramManifest;
6. prepare all required assets and demonstrate offline execution;
7. validate outputs through the same typed parser/refinements used in live execution;
8. inject artifact/template/base mismatches and prove fail-closed loading;
9. compare quality/coverage/latency/cost/memory to the incumbent;
10. exercise fallback and de-specialization;
11. submit evidence to CX-22/CX-11, not directly to activation.

## Artifact/runtime rules

- Immutable ID/digest is the execution identity; aliases are discovery only.
- Base model, adapter/program, tokenizer, renderer/template and runtime manifest are pinned together.
- Missing specialization never silently becomes base-model execution.
- Offline-ready means every required asset has been validated locally; partial readiness is explicit.
- Learned output is data, never authority.
- Constrained decoding is an optimization; type/refinement validation remains mandatory.
- Compile jobs have durable IDs. Timeout/transport failure is Unknown if the remote service may still have accepted work.

## Shared-base cache

Measure the economics of many skills sharing one base:

- resident base memory;
- adapter/program memory;
- cold-load latency;
- hot-switch latency;
- maximum useful resident skill set;
- principal/data-scope isolation;
- eviction behavior;
- whether adapter composition is allowed or explicitly unsupported.

Do not assume thousands of artifacts are cheap until these measurements exist.

## Build loops

### Compile loop

`spec/examples -> candidate artifact -> local conformance -> dev/transfer eval -> manifest -> evidence`.

### Runtime loop

`resolve immutable artifact -> verify dependency closure -> load/cache -> invoke -> typed validate -> observe outcome -> trace`.

### Crystallization loop

Repeated neural-program behavior may later become a tool/rule/library/compiler/runtime candidate under CX-18. Frequency alone never justifies native promotion.
