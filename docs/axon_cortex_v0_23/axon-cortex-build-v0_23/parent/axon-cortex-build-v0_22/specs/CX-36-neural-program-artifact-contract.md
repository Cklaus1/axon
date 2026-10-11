---
id: CX-36
title: "Neural Program Compiler, Artifact Format, and Runtime Contract"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-10", "CX-11", "CX-13", "CX-15", "CX-19", "CX-23", "CX-32", "CX-34"]
first_stage: M1
implementation_evidence: []
---

# CX-36 — Neural Program Compiler, Artifact Format, and Runtime Contract

**Revision:** 0.2, reviewed draft; integrated into Cortex build package v0.16.  
**Basis:** the supplied standalone CX-36 proposal and Cortex v0.15.  
**Review scope:** design review followed by an adversarial boundary review; no trained model, Axon runtime or production gate was executed.

## 1. Purpose and meaning

A **Neural Program** is a versioned learned-function artifact with a typed input/output contract, reproducible dependency identity, applicability envelope and governed execution path. It can specialize recurring fuzzy transformations, extraction, normalization or decisions. It is not proof that the behavioral specification was implemented correctly for every input.

The architecture searches among deterministic code, retrieval/projection/composition, specialized Reflex, Neural Programs and general reasoning. These are alternative implementations; no task must traverse a fixed ladder. A neural classifier packaged as `.np` may implement the Reflex interface: **Reflex describes a decision operation; Neural Program describes an artifact/execution class.** They are not mutually exclusive categories.

Compilation produces a **candidate**, not an admitted capability. Low latency, deterministic decoding, a valid schema, high confidence and matching examples do not individually establish semantic correctness, safety or authorization.

## 2. Ownership and compatibility

CX-23 remains the owner of Neural Program compilation/import/load/invoke lifecycle. CX-36 owns the canonical naming, `.nps`/`.np` format, identity and portable source/artifact/receipt contracts. CX-22 selects specialization candidates; CX-20 evaluates them; CX-21 measures whole-system impact; CX-11 owns independent admission; CX-34 owns the active capability registry. A "Neural Program Registry" is a view of CX-34, not a second publication authority.

The supplied package has no CX-35 file. That number remains reserved for the separately proposed general Reflex serving/model-state work. This spec neither claims CX-35 exists nor depends on future KV/latent-state injection.

Initial use is through a host/library adapter. `neural fn`, `use neural`, `np://...` and `axon neural ...` in earlier drafts are design sketches, not existing Axon syntax or frozen network protocols. New language syntax follows CX-15's independently reviewed desugaring and engine-parity process.

## 3. Naming and file conventions

Use **Neural Program**, **Neural Program Source**, **Neural Program Artifact**, **Neural Program Compiler**, **Neural Program Runtime**, and **Neural Program Backend** internally. Preserve `ProgramAsWeightsBackend`, `.paw`, external quotations/citations, provider names and importer names when they actually refer to that external implementation. Do not globally search-and-replace the string `PAW` in third-party contracts or attribution.

Axon uses `.nps` for source and `.np` for the compiled package. These are project conventions, not global extension reservations. File extension, media type or filename alone never authorizes loading. A `.paw` file renamed `.np` is invalid unless a validated conversion actually constructed this format.

## 4. Source format: `axon.nps/1`

The first machine-readable source format is **strict UTF-8 JSON**. This resolves the original draft's open YAML-like syntax. Human authoring may still be prose: an authoring adapter produces a draft `.nps`, exposes ambiguities, and renders the resulting contract for review. A future YAML surface must lower to this exact canonical contract under a distinct parser/version; YAML is not silently accepted as JSON.

Source has these required fields:

```text
NeuralProgramSource {
  schema = "axon.nps/1",
  name,
  contract_version,
  input_schema_ref,
  output_schema_ref,
  requirements: [Requirement],
  semantic_validator_refs: [ImmutableRef],
  applicability: ApplicabilitySpec,
  effect_ceiling: EffectCeiling,
  data_policy_ref,
  build: CompileRequest,
  evaluation_policy_ref,
  fallback_policy_ref,
  provenance_refs: [ImmutableRef]
}

Requirement {
  id,
  modality: MUST | SHOULD | NON_GOAL | HYPOTHESIS,
  statement,
  validation_ref?          // mandatory for executable MUST claims in protected release
}

ImmutableRef { id, revision, digest }
```

Reference IDs are resolved in an authorized registry; they are not URLs to fetch automatically. Schema and validator references bind their full dependency closure. A validator supplied by the candidate cannot become a protected verifier merely because it is referenced here.

The machine schema and examples are in `schemas/json/neural-program-source.schema.json` and `fixtures/neural_programs/minimal-source.nps`. The review reference validator checks the source contract, not whether a model satisfies its English behavioral requirements.

### 4.1 Canonical JSON profile

For identity-bearing source/manifests: reject duplicate object keys, BOMs, invalid UTF-8, lone surrogate code points, non-finite numbers, unknown required-schema variants, excessive depth and oversized inputs. Do not silently normalize source text or rewrite a meaningful string. Integers are limited to the exactly interoperable range `[-(2^53-1), 2^53-1]`; decimal quantities use typed decimal **strings** in v1. Binary model tensors are not JSON values.

Canonical bytes use UTF-8, keys sorted by Unicode code-point order, no insignificant whitespace, ordinary JSON escapes and no ASCII-escaping of otherwise valid Unicode. Key comparison and string values are not locale-dependent and are not Unicode-normalized. The algorithm is pinned as `axon.cjson/1`; this is a deliberately restricted Axon profile, not an assertion of general JSON canonicalization compatibility. `.nps` source may be pretty-printed; the packaged manifest must be canonical. The raw source bytes and its canonical semantic digest are separately recorded.

### 4.2 Contract boundaries

Input/output schemas distinguish missing, explicit null, empty, Unknown, refusal and error. Defaults may not invent observations. Free text, examples and retrieved source are untrusted data and cannot alter compiler privileges, provider settings or validators.

Changing MUST meaning, output meaning, authority requirements or required evidence creates a new contract revision and approval. An optimizer may change an implementation or proposal; it may not quietly retag a MUST as a preference. Examples and training references are eligibility-checked; hidden test inputs/answers are not embedded into distributable source packages.

## 5. Package format: `axon.np/1`

V1 is a **ZIP_STORED** archive with a canonical UTF-8 `manifest.json` and an explicit inventory of every other regular file. It is intentionally simple: no compression, encrypted members, symlinks, hardlinks, device files, archive execution hooks or nested executable packages. Large model weights may stay external in a pre-provisioned content store rather than forcing every skill to duplicate a base.

Manifest shape:

```text
NeuralProgramManifest {
  schema = "axon.np/1",
  canonicalization = "axon.cjson/1",
  artifact_kind: "neural_program" | "conformance_fixture",
  name,
  contract_ref,
  source: {path, raw_digest, semantic_digest},
  input_schema_ref,
  output_schema_ref,
  compiler: {id, revision, implementation_digest, recipe_digest},
  target: {
    backend_id, backend_revision, runtime_profile_ref,
    execution_kind, payload_format, precision,
    supported_operations, resource_limits
  },
  dependencies: [{kind, id, revision, digest, bytes}],
  assets: [{path, role, sha256, bytes}],
  applicability_ref,
  effect_ceiling,
  data_policy_ref,
  fallback_policy_ref,
  provenance_refs
}
```

`execution_kind`, payload format, precision and runtime profile are independent: LoRA is not a hardware target, ONNX is not a training method, and WebGPU is not a model architecture. Unsupported combinations refuse. A LoRA target requires exact base/tokenizer/adapter dependencies; a standalone target must bind its full weights and preprocessing dependencies. Opaque provider aliases without immutable weights may be used only under an explicitly weaker, expiring provider-observation profile, never as a reproducible local artifact.

`conformance_fixture` is restricted to inert test payloads. It cannot be published as an active learned capability. The included example fixture does not contain trained weights.

## 6. Content identity without circular hashes

Do **not** place the artifact's own digest inside the manifest it hashes. Let `M` be canonical manifest bytes and define:

```text
artifact_digest = SHA256( UTF8("AXON-NP") || 0x00 || UTF8("v1") || 0x00 || M )
artifact_ref    = {scheme: "axon.np", digest_algorithm: "sha256", digest: artifact_digest}
```

`M` binds the complete inventory of file sizes/hashes and external dependency hashes. The manifest is excluded from its own asset inventory. The archive byte digest is an additional transport checksum, not the semantic artifact ID. Two archives with the same canonical manifest and identical validated members identify the same artifact even if irrelevant ZIP timestamps differ. Extra or substituted members invalidate the archive.

Base, tokenizer, template, compiler recipe, target runtime and adapters are part of the bound identity. Quantizing, merging, converting, changing prompts or replacing a runtime target creates a **new artifact**, linked to its parent. V1 packages contain one target. A logical capability may reference several independently tested target artifacts; it must not use one artifact ID for untested target substitutions.

## 7. Detached evaluation and release envelope

Freeze/package the executable **before** final evaluation. Validation, calibration and admission receipts reference that exact artifact and target. Release metadata is detached:

```text
NeuralProgramRelease {
  artifact_ref,
  evaluation_receipt_refs,
  reliability_artifact_ref?,
  admission_receipt_ref,
  deployment_profile_ref,
  project_scope_ref,
  revocation_generation,
  lease_expiry?,
  previous_good_release_ref?
}
```

Adding signatures or evaluation results does not change executable bytes. Embedded provenance can describe derivation, but candidate-authored "PASS" records are never admission. Admission authenticates the issuer using CX-11's trusted key/registry path; hashes alone provide no authentication. A calibration change creates a new release/profile binding even when weight bytes do not change.

## 8. Strict reader and package attack surface

Inspect package metadata before allocating tensors or extracting anything. Every member must be declared exactly once, have a canonical relative path and match its recorded size/hash. Reject absolute paths, drive/UNC names, `.` or `..` segments, empty segments, backslashes, NULs, case-fold collisions, Unicode/confusable path variants and local/central ZIP metadata inconsistencies. V1 member names use lowercase ASCII letters, digits, `-`, `_`, `.`, and `/` only. Directory entries are unnecessary and rejected; only regular members are allowed.

Reject undeclared extra files, duplicate manifests/members, unsupported compression, encryption, malformed CRC/lengths and decompression tricks. The initial reference profile limits manifest size to 1 MiB, source to 256 KiB, member count to 256, a member to 1 GiB and total payload to 2 GiB; deployments may lower those limits. Larger targets need an explicit separately tested profile, not disabled checks. Budget parsing, hashing and dependency preparation as well as inference.

Use a fresh staging directory or immutable content store. Validate before atomic publication; reject executable flags and do not honor ownership metadata. Pin verified bytes/handles to avoid a validate-then-replace race. Failed imports leave no active alias or partially trusted cache. Loading must not run installers, arbitrary deserializers, model-repository code, custom graph operators or package-supplied callbacks. Trusted loader/backend code is provisioned and reviewed outside `.np`. Binary weight parsers remain part of the runtime attack surface and require CX-13 isolation.

The included Python format tool validates inert source/container fixtures only. It is not a hardened production tensor loader, sandbox or signature verifier.

## 9. Separate compiler and execution interfaces

Do not require an inference process to carry training or hosted-compilation credentials.

```text
NeuralProgramCompiler {
  describe_capabilities(),
  submit(CompileRequest) -> CompileJobRef,
  status(CompileJobRef) -> CompileJobState,
  cancel(CompileJobRef) -> CancellationReceipt,
  collect(CompileJobRef) -> CandidateArtifactRef
}

NeuralProgramRuntime {
  describe_capabilities(),
  prepare(ArtifactRef, PreparationGrant) -> PreparedAssetsReceipt,
  load(ArtifactRef, LoadGrant, RuntimeProfile) -> OpaqueProgramHandle,
  invoke(Handle, TypedInput, InvocationGrant, Budget) -> InvocationResult,
  cancel(InvocationRef) -> CancellationReceipt,
  unload(Handle) -> UnloadReceipt,
  inspect(ArtifactRef) -> ValidatedManifest
}
```

A transport-neutral logical interface is not a frozen C/Rust binary ABI. Host-language traits and optional service adapters may implement it. Compile and runtime deployments negotiate operations; not every backend can train, fork state, serialize caches or run on every device.

## 10. Compile modes and experiment lifecycle

Supported proposal modes are spec-driven synthesis/mapper compilation, training from eligible examples, and import/conversion. Each records what actually occurred. A compiled mapper artifact is not a proof of the natural-language spec, and a teacher-generated dataset is not an observed-outcome corpus.

```text
resolved source + approved acquisition/data-use policy
→ family-aware train/calibration/development split
→ bounded compile/train/import
→ immutable candidate package
→ deterministic format/type conformance
→ development evaluation and separate reliability fitting
→ freeze candidate and release policy
→ protected evaluation under submission ledger
→ independent admission decision
→ separately authorized shadow/canary/active deployment
```

Use CX-11's existing lifecycle rather than creating new state owners. A research load may occur before production admission under a sandboxed research grant. A complete lifecycle rehearsal may end in rejection; the build must not force promotion to claim a successful pilot.

Compile jobs bind idempotency key, source digest, recipe, provider and authority. Timeout or cancellation is not proof the provider stopped work. Reconcile by job ID; never automatically resubmit a potentially billable job solely because a response was lost. Preserve cache/resume identity and aggregate budget; duplicate submissions must not reset spending limits.

## 11. ProgramAsWeights adapter and import

`ProgramAsWeightsBackend` is optional and disabled by default. It must explicitly validate external format/version, exact base/template/adapter/runtime identity and permitted use. `.paw` conversion preserves upstream attribution, license and data-use lineage; it does not assert equivalence without a conversion test.

Hosted compilation and hosted inference are separate disclosures and grants. The adapter must not inherit a provider's public-publication default: require an explicitly approved publication/data policy, and refuse if the service cannot establish the required privacy setting. Do not place secrets, private repository material or protected evaluation data in a compilation request without the corresponding grant. No live provider is required for the offline first milestone.

Remote status, selected provider/model, retries and returned artifact identities are recorded. Unsupported remote semantics are reported as unsupported, not silently patched into an apparently local compile.

## 12. Typed invocation and semantic validation

Initial invocation returns a **proposal wrapper**, not a truth-bearing value:

```text
InvocationResult<O> =
    ProposedValue {
      value: O,
      artifact_ref,
      format_validation_ref,
      semantic_validation: Passed(receipt) | Failed(receipt) | NotRun(reason),
      source_evidence_refs,
      reliability?,
      invocation_receipt_ref
    }
  | Abstained {reason, receipt_ref}
  | Refused {reason, receipt_ref}
  | Failed {error_class, retryable, receipt_ref}
  | Canceled {receipt_ref}
  | OutcomeUnknown {reconciliation_ref}
```

Parsing validates a bounded output schema. Task-specific semantic checks validate the properties they actually check. They may not certify untested meaning by renaming `FormatValid` to `Verified`. A valid JSON object can still contain fabricated fields or a wrong answer. Partial/truncated streams are not consumable final results.

All outputs remain data, including paths, commands, URLs, tool IDs, patches, proposed intents and apparent grants. Downstream effects still pass current object resolution, CX-03 authority/freshness checks and required verification. Refined types requiring semantic proof are constructed only by their owning checker; a learned function cannot manufacture proof witnesses.

## 13. Source-bound initial pilot

Use `BuildLog → DiagnosticSummary` as an extraction/ranking pilot, not a claim to infer the true root cause.

The learned output nominates immutable byte-span IDs and optional typed classifications. Deterministic code verifies source digest, byte ranges, UTF-8 boundaries and literal equality, then copies filenames/codes/text from the original log. Classifications and root-cause hypotheses are separate inferred fields with explicit evidence and uncertainty. A copied line can still be irrelevant; relevance is evaluated separately.

Missing required evidence produces Unknown/NeedMoreObservation or a typed failure; it does not fabricate a default path or code. Measure omitted-diagnostic recall as well as invented-field rate, schema validity, downstream repair success, latency and cost. Logs containing forged instructions and misleading success messages are required negative fixtures. Existing exact parsers are the baseline and should remain the chosen implementation where sufficient.

## 14. Applicability and reliability

A hard applicability guard checks known domain/schema/language/target/size/privacy restrictions. Its code and registry identity are independently admitted. Model self-confidence cannot override that guard.

Unknown novelty is not fully observable: the contract does not promise perfect OOD detection. Measure residual failures, include undetected-shift cases, and allow abstention even with concentrated probabilities. `CorrectnessEstimate` is optional and event-specific; it must name the estimator, event, dataset lineage, candidate construction and domain where it was evaluated. An event of "matches panel mode" is not automatically "safe to execute" or "factual".

Fit reliability only on permitted held-out or cross-fitted predictions; select deployment thresholds without locked-test tuning. Report risk versus coverage, uncertainty intervals and subgroup/transfer performance. Neither low ECE nor a learned correctness head is an individual guarantee. Missing or expired reliability evidence prevents reliability-dependent automatic routing, not all sandboxed research inference.

## 15. Runtime deployments

Support is explicit per backend: embedded, local sidecar or remote service. Equivalent logical results do not imply identical isolation or reproducibility guarantees.

Embedded native loaders require explicit trust in the backend binary and payload parser; arbitrary candidates do not run inside the admission/authority process. Prefer a restricted sidecar for untrusted-model evaluation. Local IPC authenticates the caller and scopes opaque handles; a localhost address alone is not identity. Remote service uses authenticated transport, authorization, quotas and data-egress policy. Readiness indicates the requested immutable artifact is loaded and compatible, not that outputs are correct.

Health/discovery endpoints reveal only authorized metadata and distinguish process liveness, dependency readiness, artifact availability and inference eligibility. Backend fallback is visible and never silently changes a required assurance class.

## 16. State, adapter and cache isolation

Read-only base weights may be shared across authorized jobs. Adapter binding, KV/recurrent state, working inputs, output buffers and temporary files are scoped to the exact artifact, target, caller/project and epoch. Never reuse a prefix computed under adapter A as adapter B's prefix without a separately validated compatibility contract. Cache hit paths recheck current policy/revocation and do not grant access through possession of a digest.

Freeze artifact/adapter selection for an invocation. Hot-swap means a new deployment revision for future invocations; in-flight work drains, cancels or reconciles under its original revision. Training workers cannot mutate resident serving tensors. Fork/serialize/restore is optional and declared by the backend; unsupported operations refuse.

KV tensors, recurrent latents and embeddings are sensitive derived data, not portable semantics or authority. V1 `.np` prohibits serialized live model state. Cross-model cache transfer, learned KV generation and latent-state synthesis are research outside this format. The authoritative portable record remains source/Intent/AIR/evidence plus the relevant immutable dependencies.

## 17. Offline and resource guarantees

`offline_required` means zero network during preparation/load/invoke, including model downloads, tokenizer/config lookup, telemetry, remote fallback and remote license checks. Missing dependencies refuse. An explicit provisioning step may acquire assets earlier under separate grants. Test denial at the host boundary, not just by omitting API calls in one code path.

Charge preparation, load, validation, fallback, shadow evaluation, cache memory, queuing and inference to bounded resources. Unknown provider cost is Unknown, not zero. Cancellation prevents new downstream dispatch even if a remote inference continues; receipt reports that possibility. Every retry/fallback inherits the parent budget and deadline. Refusal, invalid artifact and unsupported target are not evidence that a more permissive remote provider should be tried.

## 18. Registry, admission, rollback and invalidation

Use CX-34 for capabilities and CX-11 for lifecycle transitions. An active release binds artifact, target runtime, validator bundle, reliability artifact, project policy and admission receipt. Permission remains caller-owned and intersection-bounded by artifact, deployment and project ceilings.

Recheck active status at dispatch. A source/model revocation or data-use invalidation suspends affected release eligibility through CX-32's typed dependency graph. Do not quarantine unrelated artifacts merely because they appear in the same graph component. Retired or invalid previous-good releases cannot become automatic rollback destinations; block or escalate when no eligible destination exists.

Activation, revocation and rollback update durable versioned pointers atomically. Restoring old code does not undo external effects, delete disclosures or reverse schema migrations. In-flight work is canceled or reconciled according to its own action semantics.

## 19. Evidence, privacy and replay

Every invocation binds the immutable artifact/target/runtime, input and effective-input digests, actual projection, caller/project/epoch, budget/egress decisions, output digest, format and semantic validation separately, reliability provenance, fallback child references, timing, cancellation and downstream evidence.

Record enough permitted material for the declared replay mode; do not mandate storing raw private input forever. Hashes can themselves be sensitive. Exact recorded replay returns recorded outputs without effects; a fresh run is separately labeled deterministic, tolerance-equivalent, statistical or observed-once. Greedy/temperature-zero decoding is not a cross-hardware bitwise guarantee.

Evidence retrieval is access-controlled. Missing/expired/withheld evidence makes the relevant claim incomplete, not false and not passing. Counterfactual outputs stay predictions. Deletion policy may preserve permitted tombstones while reducing replay capability.

## 20. Self-improvement and receiving-project use

A repeated expensive function may generate an `ImprovementIntent`, source proposal and candidate. The candidate cannot change its admission thresholds, evaluator, required checks, authority or pinned validation bundle. Better data/wording/schema/architecture proposals remain separate, versioned interventions; semantic contract changes require explicit new approval.

Cross-project use retains derivation restrictions and negative-transfer evidence. A receiving project re-admits the exact release for its own scope. Transfer is a set of demonstrated domains, not an ordinal badge authorizing every lower-level task. A Neural Program may remain userland or be replaced by a deterministic tool; native/compiler promotion remains CX-15/CX-18 work with stronger claims and evidence.

## 21. Implementation order

1. Reconcile live ownership; freeze naming and source schema. Preserve external PAW references.
2. Implement strict source parser and inert package/identity checker. No model/provider required.
3. Add detached release/receipt types and candidate-versus-active handling to existing registries.
4. Implement one isolated local runtime adapter and source-bound output validation.
5. Run a reproducible pilot against exact-parser/current-incumbent baselines; legitimate rejection is acceptable.
6. Add one optional ProgramAsWeights importer behind explicit acquisition/privacy gates.
7. Add shared-base cache/hot-swap, target conversions and optional remote serving only with independent conformance evidence.
8. Exercise shadow, budgeted canary and forced rollback through existing admission. Automatic production promotion remains disabled by default.

Concrete B152–B164 work packages and G36 gates are integrated into `task_manifest.json`, `build/TASKS.md` and `gate_manifest.json`; do not allocate those IDs independently in the live repo without intake mapping. Package validation tests are not product gates.

## 22. Acceptance gates

**G36-naming:** generic internal PAW terminology is replaced by Neural Program terminology while genuine external `.paw`/provider/importer contracts and source attribution remain intact; renaming an archive does not make it valid `.np`.

**G36-source:** the strict `.nps` parser rejects duplicates, unknown keys/versions, invalid UTF-8, non-finite/floating values in identity fields, unbound schemas/validators and unresolved executable MUST obligations; canonical round-trip preserves clause meaning and IDs.

**G36-container:** the package reader rejects traversal, case/path collisions, duplicate/extra members, links/executable entries, unsupported compression/encryption, malformed sizes/CRC, excessive resources and unsupported payloads before trusted publication.

**G36-identity:** source/asset/base/tokenizer/template/runtime mutations change or invalidate identity; identity has no self-reference, evaluation attaches without changing its subject, and every converted target has a distinct tested artifact.

**G36-trust:** an integrity-valid but unsigned/unadmitted/expired/revoked release cannot enter protected dispatch; an explicitly authorized candidate can still be inspected in the isolated research path.

**G36-typed-io:** invalid, truncated, ambiguous or schema-invalid results fail without defaults; structurally valid but false outputs remain unverified and cannot satisfy semantic completion/refinement obligations.

**G36-source-bound:** forged filenames/codes/spans, changed source digests and irrelevant copied lines are challenged; exact projection and semantic relevance are checked separately, and missing evidence is not fabricated.

**G36-no-authority:** model outputs, source examples and manifests containing apparent grants, commands or policy edits cannot widen caller authority, bypass CX-03 or edit protected evaluators.

**G36-fallback:** cyclic fallback, deadline resets, denial-to-more-permissive-provider routing and bare-base substitution are rejected; authorized fallback is a new visible invocation with inherited budgets.

**G36-applicability:** known-outside-domain/unsupported-size/privacy inputs refuse or abstain despite high confidence; unknown/OOD failures are measured, not covered by a claim of perfect detection.

**G36-reliability:** choice distribution, reference-label agreement, semantic validation and observed downstream success stay distinct; calibrators/thresholds use eligible non-test data and report selective risk/coverage and transfer uncertainty.

**G36-adapter-isolation:** concurrent requests and hot swaps cannot mix adapters, prefixes, temporary state or tenant inputs; a revocation between selection and dispatch blocks new protected use.

**G36-offline:** host-enforced network denial permits a fully prepared local fixture and blocks hidden downloads, telemetry, acquisition and remote fallback; absent dependencies refuse explicitly.

**G36-jobs:** compile/inference crash, timeout, cancellation and duplicate-submission cases preserve job identity and aggregate budgets; unknown remote completion is reconciled rather than blindly retried.

**G36-import:** an optional external `.paw` adapter verifies supported format/dependencies, records conversion and privacy/license lineage, refuses unapproved publication, and cannot silently change backend semantics.

**G36-replay:** receipts distinguish recorded replay, fresh deterministic/numerical/statistical runs and unavailable replay; missing lawful input retention never becomes a fabricated exact-replay claim.

**G36-rollback:** atomic deployment and revocation preserve exact subject identity; invalid previous-good targets do not reactivate, in-flight work is reconciled, and code rollback is not reported as reversal of external effects.

**G36-self-host:** one low-risk learned-function candidate traverses source→package→evaluation→shadow→independent accept/reject; an accepted candidate rehearses bounded activation and rollback before production eligibility, while a rejected candidate remains a valid research result.

## 23. Non-goals and open implementation evidence

This spec does not require a new foundation model, a particular vendor, all deployment modes, a new language keyword, automatic merging, universal generalization, perfect OOD detection, portable KV blobs or universal proof of learned behavior. `.np` provides a typed and integrity-checked executable package **contract**; actual semantic reliability and safe loading depend on the declared validator/runtime/evaluation profile and evidence.

The attached tools check document consistency and inert format fixtures. Production archive/tensor hardening, cryptographic trust integration, model quality, performance, confinement and live Axon/MiCode integration remain NOT_RUN until tested in the target repository.
