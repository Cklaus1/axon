# File Index — v0.3.0

The package contains **127 files** at this stage, including `MANIFEST.json` and `SHA256SUMS`. The final archive is generated from this tree after integrity files are refreshed.

## Reading order

Start with `README.md` → `SPEC.md` → `reports/CORTEX_OVERLAP_REVIEW.md` → `docs/CORTEX_ALIGNMENT.md` → `reports/ADVERSARIAL_REVIEW_V030.md` → `docs/NEXT_INTEGRATIONS.md` → `build/FIRST_TASK.md`.

`reports/VALIDATION.md` records executed checks. `PACKAGE_STATUS.md` separates runnable offline evidence from live integration claims.

## Complete inventory

| File | Purpose |
|---|---|
| `.gitignore` | Excludes local environments, caches and local run artifacts. |
| `AGENTS.md` | Implementation-agent rules, owner boundaries and validation discipline. |
| `CHANGELOG.md` | Version history and architectural fold-ins. |
| `FILE_INDEX.md` | Complete package inventory and reading order. |
| `MANIFEST.json` | Payload inventory with byte counts and SHA-256 hashes. |
| `Makefile` | Convenience commands for offline checks. |
| `PACKAGE_STATUS.md` | Implemented/spec-only boundary, overlap result and evidence limits. |
| `README.md` | Package orientation, architecture and validation commands. |
| `SHA256SUMS` | Checksums for payload plus manifest; excludes itself. |
| `SOURCES.md` | Reviewed source archives, public references and unsupported assumptions. |
| `SPEC.md` | Normative Semantic Compilation v0.3 contract. |
| `build/ACCEPTANCE_GATES.md` | G0/G0A through G5 evidence gates. |
| `build/BACKLOG.json` | Dependency-linked implementation backlog including Cortex/Forge seams. |
| `build/BUILD_LOOP.md` | Implementation iteration and evidence loop. |
| `build/BUILD_PLAN.md` | Ordered Cortex-aligned implementation phases and proof requirements. |
| `build/FIRST_TASK.md` | Smallest recommended live owner-preserving vertical slice. |
| `build/RELEASE_RUNBOOK.md` | Release validation and exact-revision handoff procedure. |
| `build/TEST_MATRIX.md` | Adversarial/native integration test matrix. |
| `build/TRACEABILITY.csv` | Normative requirement-to-document/test-family mapping. |
| `config/defaults.json` | Reference versioned configuration/policy fixture. |
| `config/policy.example.json` | Reference versioned configuration/policy fixture. |
| `config/profiles.json` | Reference versioned configuration/policy fixture. |
| `docs/ADAPTERS.md` | Design/detail document: Adapters. |
| `docs/API_CLI.md` | Design/detail document: Api Cli. |
| `docs/ARCHITECTURE.md` | Design/detail document: Architecture. |
| `docs/COMPILER.md` | Design/detail document: Compiler. |
| `docs/CONFIGURATION.md` | Design/detail document: Configuration. |
| `docs/CORTEX_ALIGNMENT.md` | Design/detail document: Cortex Alignment. |
| `docs/DECISIONS.md` | Design/detail document: Decisions. |
| `docs/EVALUATION_AND_REPAIR.md` | Design/detail document: Evaluation And Repair. |
| `docs/FORGE_SOFTWARE_SEMANTICS.md` | Design/detail document: Forge Software Semantics. |
| `docs/IDENTITY_CACHE.md` | Design/detail document: Identity Cache. |
| `docs/INTEGRATION.md` | Design/detail document: Integration. |
| `docs/IR_SEMANTICS.md` | Design/detail document: Ir Semantics. |
| `docs/MIGRATION.md` | Design/detail document: Migration. |
| `docs/NEXT_INTEGRATIONS.md` | Design/detail document: Next Integrations. |
| `docs/OBSERVABILITY_UX.md` | Design/detail document: Observability Ux. |
| `docs/OPEN_QUESTIONS.md` | Design/detail document: Open Questions. |
| `docs/OPTIMIZATION.md` | Design/detail document: Optimization. |
| `docs/RUNTIME.md` | Design/detail document: Runtime. |
| `docs/SECURITY.md` | Design/detail document: Security. |
| `docs/STRATEGY_EVIDENCE_EXPERIMENTS.md` | Design/detail document: Strategy Evidence Experiments. |
| `examples/README.md` | Guide to synthetic fixtures, limitations and alignment examples. |
| `examples/compilation_record.json` | Synthetic compilation provenance/lowering record. |
| `examples/document_captures.json` | Captured synthetic flawed/repaired document outputs. |
| `examples/document_inputs.json` | Inputs for the synthetic document simulation. |
| `examples/evaluation_result.json` | Typed evaluation-result fixture. |
| `examples/evaluations/coding.json` | Protected evaluation-contract fixture: coding. |
| `examples/evaluations/document.json` | Protected evaluation-contract fixture: document. |
| `examples/evaluations/research.json` | Protected evaluation-contract fixture: research. |
| `examples/event.json` | Execution/provenance event fixture. |
| `examples/evidence_projection.json` | Synthetic CX-32 evidence projection. |
| `examples/experience.json` | Repeated-trial outcome/cost-accounting fixture. |
| `examples/experiment_profile.json` | Synthetic CX-33 experiment-profile projection. |
| `examples/forge_binding.json` | Synthetic IntentIR + AIR → Forge SIR binding. |
| `examples/intents/coding.json` | Synthetic normalized intent fixture: coding. |
| `examples/intents/document.json` | Synthetic normalized intent fixture: document. |
| `examples/intents/research.json` | Synthetic normalized intent fixture: research. |
| `examples/invalid/cycle.json` | Intentionally invalid plan fixture: cycle. |
| `examples/invalid/disconnected_work.json` | Intentionally invalid plan fixture: disconnected work. |
| `examples/invalid/duplicate_node.json` | Intentionally invalid plan fixture: duplicate node. |
| `examples/invalid/missing_port.json` | Intentionally invalid plan fixture: missing port. |
| `examples/invalid/type_mismatch.json` | Intentionally invalid plan fixture: type mismatch. |
| `examples/invalid/unknown_capability.json` | Intentionally invalid plan fixture: unknown capability. |
| `examples/invalid/unknown_dependency.json` | Intentionally invalid plan fixture: unknown dependency. |
| `examples/invalid/unknown_field.json` | Intentionally invalid plan fixture: unknown field. |
| `examples/invalid/unordered_reference.json` | Intentionally invalid plan fixture: unordered reference. |
| `examples/invalid/unsafe_write_retry.json` | Intentionally invalid plan fixture: unsafe write retry. |
| `examples/invalid/unsupported_loop.json` | Intentionally invalid plan fixture: unsupported loop. |
| `examples/invalid/wrong_evaluation_digest.json` | Intentionally invalid plan fixture: wrong evaluation digest. |
| `examples/invalid_cases.json` | Expected diagnostic index for negative fixtures. |
| `examples/owner_profile.json` | Synthetic Cortex owner-profile projection. |
| `examples/plans/coding.json` | Synthetic portable Semantic Plan fixture: coding. |
| `examples/plans/document.json` | Synthetic portable Semantic Plan fixture: document. |
| `examples/plans/research.json` | Synthetic portable Semantic Plan fixture: research. |
| `examples/registry.json` | Synthetic capability-registry projection for examples. |
| `examples/repair_request.json` | Bounded immutable-revision repair request fixture. |
| `examples/resource_projection.json` | Synthetic CX-34 epoch-bound resource projection. |
| `examples/strategy_artifact.json` | Synthetic CX-29-aligned strategy identity/lineage fixture. |
| `harness/__init__.py` | Offline reference harness implementation. |
| `harness/contracts.py` | Offline reference harness implementation. |
| `harness/metrics.py` | Offline reference harness implementation. |
| `harness/simulate.py` | Offline reference harness implementation. |
| `integration/CORTEX_V023_INPUT_LOCK.json` | Pinned Cortex v0.23 source archive identity. |
| `integration/CORTEX_V023_OWNER_PROFILE.json` | Canonical Cortex/Forge owner projection used by v0.3. |
| `integration/CORTEX_V023_REQUIREMENT_CROSSWALK.json` | 73-requirement ownership overlap classification. |
| `integration/FORGE_V03_INPUT_LOCK.json` | Pinned Forge v0.3 source archive identity. |
| `reports/ADVERSARIAL_REVIEW_V030.md` | Adversarial design review and folded mitigations. |
| `reports/CORTEX_OVERLAP_REVIEW.md` | Cortex/Forge overlap analysis and net-new Semantic Compilation value. |
| `reports/DESIGN_REVIEW.md` | Final author consistency review and unresolved blockers. |
| `reports/FOLD_IN_V030.md` | Summary of review changes incorporated into v0.3. |
| `reports/VALIDATION.md` | Executed v0.3 package/upstream validation report and limitations. |
| `reports/contract_checks.json` | Machine-readable output of the offline contract/package checker. |
| `reports/synthetic_demo.json` | Captured output from the synthetic reject/repair/recheck demo. |
| `reports/unit_tests.txt` | Verbose output of the 119-test offline suite. |
| `reports/upstream_cortex_v023_tests.txt` | Captured executed Cortex v0.23 validation evidence. |
| `reports/upstream_cortex_v023_validation.json` | Captured executed Cortex v0.23 validation evidence. |
| `reports/upstream_forge_v03_validation.txt` | Captured executed Forge v0.3 validation evidence. |
| `requirements.lock` | Exact dependency versions used by the offline harness. |
| `requirements.txt` | Reference dependency ranges for the harness. |
| `schemas/capability-registry.schema.json` | JSON Schema 2020-12 contract: capability-registry. |
| `schemas/common.schema.json` | JSON Schema 2020-12 contract: common. |
| `schemas/compilation-record.schema.json` | JSON Schema 2020-12 contract: compilation-record. |
| `schemas/config.schema.json` | JSON Schema 2020-12 contract: config. |
| `schemas/evaluation-contract.schema.json` | JSON Schema 2020-12 contract: evaluation-contract. |
| `schemas/evaluation-result.schema.json` | JSON Schema 2020-12 contract: evaluation-result. |
| `schemas/event.schema.json` | JSON Schema 2020-12 contract: event. |
| `schemas/evidence-projection.schema.json` | JSON Schema 2020-12 contract: evidence-projection. |
| `schemas/experience.schema.json` | JSON Schema 2020-12 contract: experience. |
| `schemas/experiment-profile.schema.json` | JSON Schema 2020-12 contract: experiment-profile. |
| `schemas/forge-binding.schema.json` | JSON Schema 2020-12 contract: forge-binding. |
| `schemas/intent.schema.json` | JSON Schema 2020-12 contract: intent. |
| `schemas/owner-profile.schema.json` | JSON Schema 2020-12 contract: owner-profile. |
| `schemas/plan.schema.json` | JSON Schema 2020-12 contract: plan. |
| `schemas/repair.schema.json` | JSON Schema 2020-12 contract: repair. |
| `schemas/resource-projection.schema.json` | JSON Schema 2020-12 contract: resource-projection. |
| `schemas/strategy-artifact.schema.json` | JSON Schema 2020-12 contract: strategy-artifact. |
| `source_context/BFLOW_AGENTS_COMPLETE_DOCUMENTATION.md` | Unmodified supplied BFlow project documentation for provenance. |
| `source_context/cortex_v023/README.md` | Extracted Cortex v0.23 source context used for ownership review. |
| `source_context/cortex_v023/V023_OWNER_MAP.json` | Extracted Cortex v0.23 source context used for ownership review. |
| `source_context/forge_v03/OWNERSHIP.md` | Extracted Forge v0.3 ownership context used for review. |
| `tests/test_alignment.py` | Offline regression/conformance tests. |
| `tests/test_contracts.py` | Offline regression/conformance tests. |
| `tests/test_metrics.py` | Offline regression/conformance tests. |
| `tests/test_simulator.py` | Offline regression/conformance tests. |
| `tools/check_pack.py` | Package validation/integrity utility. |
| `tools/verify_manifest.py` | Package validation/integrity utility. |
