# Axon Forge API — Normative v0.3

```text
forge.parse(source, frontend) -> ParseResult
forge.lower_to_sir(parse_result) -> SemanticIR
forge.inspect(sir_ref, query) -> SemanticQueryResult
forge.verify(sir_ref, contracts, resource_projection_ref?) -> VerificationResult
forge.transform(sir_ref, transform_ref, context) -> TransformResult
forge.migrate(sir_ref, migration_ref, mode=dry_run|apply) -> MigrationResult
forge.meta_execute(request) -> MetaExecutionResult
forge.build(build_program_ref, context) -> BuildGraph
forge.compile(sir_or_build_ref, backend_ref) -> CompileResult
forge.normalize_diagnostics(raw, backend_ref) -> Diagnostic[]
forge.semantic_diff(before_ref, after_ref) -> SemanticDiff
forge.truth.resolve(query, bootstrap_pack_ref?) -> TruthResolution
forge.experiment_profile(candidate, axon_experiment_ref) -> ForgeExperimentProfile
forge.evidence_payload(observation, axon_evidence_ref) -> ForgeEvidencePayload
```

Forge API calls that cause effects MUST carry external capability/execution references as defined by Cortex/Axon integration.
