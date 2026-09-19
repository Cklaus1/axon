---
id: CX-23
title: "Neural Program Runtime and Skill Compiler: typed learned functions as guarded Axon capabilities"
status: Draft
authority: Proposed
depends_on: ["CX-11", "CX-13", "CX-18", "CX-22"]
first_stage: M5
implementation_evidence: []
---

# CX-23 — Neural Program Runtime and Skill Compiler

## 1. Purpose

Some recurring cognition is not naturally a bounded Choice/Noul/Score decision and is not yet stable enough for deterministic code. Examples include fuzzy normalization, extraction, compact summarization, format repair, intent-fragment canonicalization, log triage and other small learned transformations.

CX-23 defines a **Neural Program** as a typed learned function artifact with a semantic contract, pinned runtime/base-model dependencies, explicit applicability/authority bounds, local/offline execution where supported, and independent validation of outputs.

The design is inspired by public "programs as weights" systems that compile a natural-language behavior specification into small adapters/program artifacts over a shared base model. Axon does not assume or depend on any external service or proprietary compiler; it treats that pattern as an implementation lead.

## 2. Decisive fork

Neural programs are first-class **learned artifacts**, not trusted code and not Reflex authority.

```text
Skill specification + eligible examples/evidence
              ↓
      Neural Skill Compiler
              ↓
   immutable learned artifact
              ↓
 shared-base/local runtime
              ↓
 typed parser + policy checks
              ↓
        ordinary Axon data
```

A neural program's output is data. It cannot mint file paths, shell authority, network permission, grants or completion evidence merely by emitting strings that look like those objects.

## 3. ABI

Conceptual typed surface:

```text
NeuralProgram<I, O> {
  artifact_id,
  semantic_contract_ref,
  input_schema_ref,
  output_schema_ref,
  applicability_guard_ref,
  runtime_manifest_ref,
  authority/effect_ceiling,
  fallback_ref?
}

invoke(program, input: I) -> Result<O, NeuralProgramError>
```

Initial implementation may use host/runtime adapters rather than new Axon syntax. Parser/compiler-native syntax is deferred until repeated use justifies it.

## 4. Artifact manifest

Every artifact has immutable identity and includes or references:

```text
NeuralProgramManifest {
  artifact_id, digest,
  semantic_contract_digest,
  skill_compiler_id, compiler_revision,
  base_model_id, base_model_revision,
  adapter_or_program_digest,
  tokenizer_revision?,
  prompt/renderer/template_digest,
  runtime_manifest_version,
  input/output schema versions,
  training/evaluation suite refs,
  data lineage and usage policy,
  applicability_guard_ref,
  calibration/quality claims?,
  authority/effect ceiling,
  resource profile,
  fallback/deoptimization target
}
```

Mutable slugs/aliases may point to artifacts; execution/replay/evidence always records immutable IDs/digests.

## 5. Compiler modes

The Skill Compiler may use multiple implementation strategies:

- adapter/LoRA training over a shared pretrained base;
- small standalone model;
- prompt/prefix compilation when evidence supports it;
- constrained-decoding specialization;
- future Axon-native learned representation.

A remote compile service is optional research infrastructure, never an architectural requirement. Long-running compilation jobs use durable job identity/status; a timeout is Unknown, not proof that compilation did not occur. Compiler provenance and data disclosure are recorded.

## 6. Runtime and shared-base registry

The runtime supports a registry of pinned base interpreters and small learned artifacts. It may cache hot base models/adapters, but cache identity must include all semantics-affecting revisions.

Desired properties:

- local/offline execution after validated assets are present;
- fail closed when required artifact/base/template/runtime assets are missing or incompatible;
- no silent fallback to the unadapted base;
- bounded context/output/resource usage;
- tenant/principal-aware cache isolation where data sensitivity requires it;
- deterministic artifact resolution by immutable ID;
- optional hot/cold skill cache with observable eviction/loading costs.

Offline availability is an explicit state. "Program file present" does not imply the shared base/runtime is present.

## 7. Typed output and constrained generation

A neural program may generate tokens internally, but Axon accepts only a value that validates against `O` and any refinements/effect-free parsing rules. Constrained decoding may reduce invalid outputs but does not replace post-generation type/refinement validation.

Examples:

```text
str -> Result<DiagnosticSummary, NeuralProgramError>
str -> Result<IntentFragment, NeuralProgramError>
MalformedJson -> Result<CanonicalJson, NeuralProgramError>
```

If parsing/validation fails, return an explicit error/abstention. Never coerce malformed output into a plausible default.

## 8. Authority and safety

A neural program cannot directly execute its generated text. Outputs that name semantic objects must be resolved against current observations and capability catalogs; outputs that propose actions pass CX-03 authorization/freshness/transaction checks.

The artifact's authority ceiling can only narrow the caller. Learned artifacts may not modify the trusted verifier, hidden tests, risk classifier, grant issuer or their own admission policy.

## 9. Training, evaluation and promotion

CX-22 decides whether a neural program is an appropriate representation. CX-20 owns controlled training/evaluation. CX-21 measures whole-system utility. CX-11 owns admission.

Compare against at least the incumbent executor and a simpler alternative where feasible. Evaluate transfer/applicability, typed-output validity, latency, memory/resident artifact cost, compile/training cost, fallback/OOD behavior and data-policy compatibility.

## 10. Acceptance gates

**G23-artifact:** mutate the adapter/program bytes, prompt template, base revision or runtime manifest; loading refuses with the mismatched component named and does not resolve through a mutable alias to another artifact.

**G23-typed-output:** force syntactically plausible but schema/refinement-invalid model output; invocation returns an explicit validation error and no downstream action is created.

**G23-no-fallback:** remove/corrupt the learned artifact while leaving the shared base available; runtime refuses or follows the registered semantic fallback and never runs the bare base while reporting the specialized artifact ID.

**G23-authority:** a neural program emits a shell command/path/tool-like string outside the current legal action catalog; the string remains untrusted data and cannot bypass CX-03.

**G23-offline:** after all declared assets are prepared, run an eligible fixture with networking disabled; missing assets fail explicitly rather than reaching the network. The evidence states exactly which base/runtime assets were cached.

**G23-cache:** load two programs sharing a base, exercise hot/cold cache transitions and principal/data-scope changes, and verify correct artifact/base identity, isolation and accounted memory/loading costs.

## 11. First build slice

Implement one low-risk typed fuzzy function such as build-log normalization or diagnostic extraction. Use an open/reference compile path or locally trained adapter, pin every artifact dependency, run locally/offline, validate typed output, compare against the incumbent/general model, and submit the candidate to CX-22/CX-11. Do not use a neural program for permission, proof, final verification or irreversible execution in the first slice.
