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

**v0.16 ownership resolution:** CX-23 retains implementation lifecycle ownership. CX-36 is the canonical source/package/portable-contract specification; CX-34 owns the registry and CX-11 owns admission. This is one subsystem, not competing Neural Program runtimes.

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
 typed parser + source/semantic checks
              ↓
        provenance-bearing candidate data
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

invoke(program, input: I) -> InvocationResult<O>  // CX-36 ProposedValue / abstention / failure union
```

Initial implementation may use host/runtime adapters rather than new Axon syntax. Parser/compiler-native syntax is deferred until repeated use justifies it.

## 4. Artifact manifest

The canonical `NeuralProgramManifest` is defined by CX-36 and `schemas/json/neural-program-manifest.schema.json`. It binds source, contract, schemas, compiler recipe, one runtime target and all asset/dependency digests. It excludes its own ID and detached evaluation/admission receipts to avoid digest cycles. `NeuralProgramRelease` binds the frozen artifact to independent evaluation, reliability, admission and deployment policy. Mutable aliases never replace immutable receipt subjects. Candidate inspection is allowed only under a separate isolated research grant; it is not active publication.

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

A neural program may generate tokens internally, but its result remains a CX-36 `ProposedValue<O>`. Structural validation against `O` does not establish factual or semantic correctness. Semantic refinements require a receipt from their owning checker, not merely a plausible typed string. Constrained decoding may reduce invalid outputs but does not replace post-generation type/refinement validation.

Examples:

```text
str -> Result<DiagnosticSummary, NeuralProgramError>
str -> Result<IntentFragment, NeuralProgramError>
MalformedJson -> Result<CanonicalJson, NeuralProgramError>
```

If parsing/validation fails, return an explicit error/abstention. Never coerce malformed output into a plausible default.

## 8. Authority and safety

A neural program cannot directly execute its generated text. Outputs that name semantic objects must be resolved against current observations and capability catalogs; outputs that propose actions pass CX-03 authorization/freshness/transaction checks.

The artifact's declared requirements are not a grant. Effective permission is the intersection of caller, project, deployment and admitted artifact ceilings, rechecked by the host at dispatch. Learned artifacts may not modify the trusted verifier, hidden tests, hard risk-policy owner, grant issuer or their own admission policy. Advisory risk estimators may be proposed as separately evaluated candidates, never substituted for hard policy.

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

## v0.16 review amendment — Canonical Neural Program ownership

Implement CX-36's source/package/receipt contracts under this existing lifecycle owner. Neural Program Registry is a CX-34 view. A `.np` candidate may be loaded only in an explicitly authorized research profile until CX-11 issues the corresponding release admission.

**G23-format-owner:** source, artifact, runtime and release identities round-trip between CX-23/CX-36/CX-34 without duplicated manifest or admission semantics; candidate inspection cannot self-publish an active capability.

See `build/REVIEW_INVARIANTS.md` for the shared interpretation and `review/BUILD_ADVERSARIAL_REVIEW.md` for attack cases.

## v0.18 ACE amendment — ACE compiler/runtime lifecycle integration

P09 binds the existing separate compiler/runtime interfaces to reviewed CX-36, with detached release and ProposedValue preserved. Both compilation modes retain eligible data/job/source lineage. Prompt-only recipes are not learned artifacts. Candidate state is not activation; external PAW remains optional. Retain BuildLog → DiagnosticSummary pilot and valid rejection.

See the normative [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md) and [build guide](../build/ACE_INTEGRATION.md). These requirements extend this owner; no second ACE subsystem is introduced.

**G23-ace-lifecycle:** Spec- and eligible-experience-driven candidate lifecycles retain source/recipe/data/job/artifact and detached release; inference lacks compile/publication privilege, unknown jobs reconcile, and prompt-only or invalid artifacts cannot masquerade as Neural Programs.

## v0.19 amendment — decision lowering without Neural Program format churn

The compiler/runtime may lower source-level learned predicates or skill calls into an internal scheduling vocabulary: `DECIDE<T>`, `FANOUT`, `JOIN`, `CALIBRATION_GATE`, `VERIFY`, and `ESCALATE`. These are AIR/skill/runtime operations around typed learned-function calls; they do **not** change CX-36 `.nps`/`.np` revision 0.2 or move execution authority into a Neural Program artifact.

`FANOUT` may fuse only dependency-compatible questions over a pinned observation/effective input. `JOIN` preserves per-question absence/error/cancel states. `CALIBRATION_GATE` consumes a CX-06-scoped correctness estimate plus external policy; it never grants capability. `VERIFY` invokes the owning verifier/evidence path rather than asking the same model to assert completion.

**G23-decision-lowering:** A compiler fixture lowers typed decisions into DECIDE/FANOUT/JOIN/CALIBRATION_GATE/VERIFY/ESCALATE scheduling while preserving CX-36 r0.2 artifacts, dependency semantics, per-question failures and external authority/verifier ownership.

## v0.20 amendment — schema decisions and lightweight Reflex specialization

A frozen-encoder learned classifier may implement Reflex and be packaged through unchanged CX-36 `.nps`/`.np` contracts; no separate `.reflex` extension is needed. TF-IDF/deterministic candidates use their appropriate existing representation lifecycle. [Qualification metadata](../schemas/LIGHTWEIGHT_REFLEX_PROFILE.md) binds task, label map, input/encoder/tokenizer/head, calibration and runtime profile via existing references. Schema-to-question lowering itself neither produces learned weights nor constitutes a CX-36 artifact/admission. Retain compile-candidate/admit/deploy separation.

**G23-lightweight-qualification:** A learned Reflex candidate uses existing source/artifact/admission ownership and detached exact qualification; schema compilation is not a learned artifact, and unchanged CX-36 r0.2 formats do not acquire a new .reflex variant.
