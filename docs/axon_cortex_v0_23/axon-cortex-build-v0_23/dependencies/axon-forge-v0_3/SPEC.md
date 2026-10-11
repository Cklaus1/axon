# Axon Forge Normative Specification v0.3

## 1. Scope

Forge is a language-agnostic semantic/compiler substrate. It lets an AI inspect a program semantically, execute bounded programs during compilation, create transformations and migrations, compile through interchangeable backends, consume structured diagnostics, and expose resulting artifacts to Axon's experiment/evidence machinery.

Forge is not an autonomous optimization authority. It is the **domain substrate** that makes software/compiler choices measurable by Axon.

## 2. Stable ownership boundary

### Forge MUST own

`SemanticIR`, `SemanticDiff`, `Transform`, `TransformResult`, `MetaExecutionRequest/Result`, `Migration`, `BuildProgram`, `BuildGraph`, compiler/backend adapters, diagnostic normalization, bootstrap packs, truth-resolution evidence, and Forge conformance.

### Forge MUST NOT own

Generic experiment IDs/lifecycles, global evidence lineage, policy admission, capability grant minting, host isolation authority, production activation, or physical resource inventory.

Forge uses typed references/projections into those owners.

## 3. Semantic classes

Each mutable concept is classified as:

- `semantic_invariant`: required behavior/safety; never optimized away;
- `implementation_strategy`: interchangeable implementation preserving invariants;
- `optimization_parameter`: tunable value inside a strategy.

## 4. End-to-end semantic loop

`source -> frontend -> SIR -> verify -> meta/transform/migrate -> build/backend -> execute -> Forge evidence payload -> Axon evidence/experiment -> admission decision`

The Axon experiment and admission objects are outside Forge authority.

## 5. Programs may participate in compilation

A Forge meta-program may inspect types/SIR, constant-evaluate, generate data/code/SIR, specialize code, register transforms, generate bindings/assets, construct build graphs, emit diagnostics, run declared tests, and request Axon experiments.

Meta execution MUST be capability-requesting, never capability-minting. Actual grant validation and isolation are external authority seams.

## 6. Local executable truth

When language/API semantics are uncertain, default resolution priority is:

1. current compiler behavior/probes;
2. repository source;
3. installed libraries/modules;
4. conformance tests;
5. canonical examples;
6. local versioned specification;
7. version-matched docs;
8. external docs;
9. model priors.

Conflicts are surfaced with evidence; they are not silently resolved by model intuition.

## 7. Transformations

Every transform declares applicability, pre/postconditions, preserved invariants, permitted changes, tunable parameters, verification requirements, and stable identity. `apply(input_ir, context)` returns `TransformResult` with output SIR when applied, semantic diff, structured diagnostics, provenance projection, observed inputs, and verification requirements.

## 8. Semantic migrations

Large repetitive changes SHOULD be represented as `Migration` artifacts with match/rewrite semantics, dry-run semantic diff, idempotency expectation, validation, progress identity, and rollback metadata.

## 9. Build programs

Build logic MAY be ordinary semantic programs. A `BuildProgram` executes under explicit capabilities and produces a deterministic `BuildGraph` containing declared inputs, outputs, tools, target constraints and artifact identities.

## 10. Diagnostics and repair

Backends SHOULD normalize diagnostics into typed records. MiCode SHOULD prefer structured diagnostics and semantic references over raw terminal scraping when available.

## 11. Resources

Forge consumes an externally observed resource projection. SIR may express placement/capability requirements, but Forge MUST NOT claim that its projection authorizes or proves resource availability.

## 12. Axon integration

Forge emits `ForgeExperimentProfile` and `ForgeEvidencePayload` records that attach domain-specific SIR/transform/compiler/build facts to canonical Axon experiment/evidence identities. Hard invariant failures always invalidate a Forge candidate independent of performance.

## 13. AI reproducibility

A capable coding agent given this package, schemas, examples and conformance vectors SHOULD be able to construct an interoperable minimal Forge without guessing foundational contracts.
