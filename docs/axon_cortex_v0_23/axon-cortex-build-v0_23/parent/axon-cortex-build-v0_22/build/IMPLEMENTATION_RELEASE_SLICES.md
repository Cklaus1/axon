> **Current release: 0.22.** The [closed-loop integration contract](CLOSED_LOOP_INTEGRATION_V022.md) adds the Axon/Fabric/MiCode slice. Historical sections below retain their original version/scope; they do not establish current peer implementation or runtime qualification. Use `../integration/V022_INPUT_LOCK.json` for current input identity.

> **v0.21 Release-slice amendment.** Use [research integration](RESEARCH_INTEGRATION_V021.md), [source upgrade](UPGRADE_V021.md) and [v0.21 bootstrap](BOOTSTRAP_PROMPT_V021.md) for the current additive delta. Historical profiles below retain their original version scope. Package statuses do not reset live source evidence.

# Dependency and release slices — v0.16

The package is a backlog, not one atomic release. Build only the owner-approved profile. `release_profiles.json` gives entry tasks; derive their full dependency closure from `task_manifest.json`. A profile reaching its exit tasks means implementation/evidence for that profile, not completion of every Cortex feature.

| Profile | Exit tasks | Meaning |
|---|---|---|
| `intake` | B02, B165 | Live-code ownership/gate audit and document checks; no model or effectful runtime. |
| `safe_repair` | B14 | Existing M1 deterministic repair, confinement, verifier and recorded replay. |
| `first_model` | B16 | One authorized existing-model vertical slice against controls. |
| `neural_format` | B155 | `.nps`/`.np` format/identity tooling; inert examples only, no trained capability. |
| `early_self_observation` | B118, B140 | Instrument incumbent route/context decisions and evidence shapes without requiring learned successors. |
| `reference_scheduler` | B149 | Minimal registry and hard-filtered reference dispatch; advanced specialists/transfer are not prerequisites. |
| `neural_candidate` | B164, B166 | One local source-bound candidate, protected evaluation, accept/reject and any required canary/rollback. |
| `review_integration` | B173 | Later cross-component negative tests, bridge migration, transfer and recursive-governance checks. |

## Corrected dependency hazards

The supplied DAG was acyclic but some early tasks waited on late products: B148's M2 registry required B137 cross-project promotion; B141's M1 evidence integration required supervisor and working-set modules; B116/B118 instrumentation assumed the new policies already existed. v0.16 separates **instrument existing behavior / define shared contracts** from **deploy learned replacements / demonstrate cross-project promotion**.

B116 now depends on B04; B117 instruments the incumbent model seam; B118 instruments incumbent observations/context without forcing a learned working-set manager. B141 binds base receipts before supervisor-specific integration. B148 defines the registry over canonical contracts and grants before specialist/cross-project plugins. B143 provides an early evidence-closure adapter; B124/B173 exercise the full admitted self-application lifecycle later.

B77 remains the existing Neural Program implementation entry, but adopts CX-36 rather than defining a second manifest. B158/B159 harden/integrate B79/B80 instead of building duplicate runtimes/caches. B157 is optional external import and is not a required dependency of local/offline use.

## Release preconditions

A production release enumerates its mandatory gates, supported capabilities, deployment profile, evaluator, statistical policy and exact artifacts before execution. Missing/inconclusive/expired gates block. Optional unsupported features remain disabled; they are not silently considered passing. Primitive format checks may run before inference; product isolation/quality gates cannot be claimed from these checks.

Keep B identifiers stable for reconciliation with existing work. All task statuses remain `Not started` in this proposal package even when document/format-reference tests pass here. The live repository owns actual implementation evidence and may map these IDs into its governance ledger.

## ACE profiles added in v0.18

[AN0–AN5](ACE_INTEGRATION.md) layer onto, rather than replace, these existing profiles. The core closure ends at B182. B183 needs the live MiCode counterpart. B181/B184/B185/B186 are mechanism-specific or research qualifications and must not become prerequisites of B182. B157 remains optional.

## v0.20 schema / lightweight Reflex supplement

Apply [schema frontend](SCHEMA_DECISION_FRONTEND.md) and [lightweight distillation](LIGHTWEIGHT_REFLEX_DISTILLATION.md) under existing owner contracts. B210–B213 are model-free schema/active-output/source/authority seams; B214–B216 are governed fixed-task data/training/independent acceptance; B217–B219 qualify runtime/disposition and real peer use; B220 maintains offline package conformance. B221/B222 are optional context/richer-learner hypotheses, never success prerequisites for the new bounded profiles.

Preserve exact task/labels/source/runtime identity, partition/evaluator custody, null-threshold/OOD fallback, local permissions and independent completion. No new .reflex/ABI owner, schema-plan-to-trained-artifact shortcut, in-place self-training or head-only speed claim. [Coordinated apply order](APPLY_V020_WITH_MICODE_V014.md) preserves current live evidence.
