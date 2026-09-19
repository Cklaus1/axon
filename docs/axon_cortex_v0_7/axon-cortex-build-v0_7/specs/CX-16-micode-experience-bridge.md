---
id: CX-16
title: "MiCode experience bridge and cross-system conformance"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-02", "CX-10"]
first_stage: M1
implementation_evidence: []
---

# CX-16 — MiCode experience bridge and cross-system conformance

## Intent and source basis

Make MiCode a versioned experience/research plane for Cortex without coupling Axon to MiCode internals or importing MiCode authority. MiCode already has the ingredients of a useful coding-world harness—central composition, durable events, tool outcomes, delegation, worktrees and verification—but its permission model, host guarantees and lifecycle remain locally owned. The bridge exchanges evidence and policies, not ambient privilege.

## Decisive fork

Use a schema-versioned bidirectional bridge. Axon consumes canonical episodes, observations, bounded decision records, failure attribution, benchmark bundles and knowledge candidates. MiCode may consume approved Cortex decision policies, Reflex adapters, verifier profiles, capability schemas and ontology/version maps. Neither side trusts the other system's local authority tokens, path grants, risk ceilings or process handles.

Do not link either runtime directly against the other's private structs. The bridge is an artifact/protocol boundary with explicit migrations and conformance fixtures.

## Import contract

Axon-side importable families:

- `CodingEpisodeBundle`: immutable task contract, before/after repository identity, observations, candidate sets, actions, generated artifacts, authorization outcomes, executed effects, verification evidence, budgets/costs and failure attribution;
- `SoftwareObservationBundle`: semantic object catalog plus omissions/truncation/freshness receipts;
- `DecisionDatasetBundle`: question/candidate/result/probability provenance plus behavior-policy lineage;
- `FailureAttributionBundle`: candidate causes, supporting/contradicting evidence and intervention results;
- `BenchmarkBundle`: task/evaluator/environment manifests and all attempts, including failure/cancel/inconclusive;
- `KnowledgeCandidateBundle`: pattern/concept candidate with source provenance, counterexamples, license/data-use constraints and evidence stage;
- `SkillCandidateBundle`: proposed reusable action sequence/tool/library/compiler candidate with applicability and authority requirements.

Every bundle carries producer system/version, schema version, artifact digests, source-system episode IDs, repository/content identities, policy/evaluator versions, sensitivity/use policy, and explicit Unknown/NotObserved states. Absence never becomes a default assertion.

## Export contract

Axon may export only artifacts whose release policy permits external consumption:

- AIR/Cortex decision policies;
- Reflex backend manifests/adapters;
- verifier/acceptance profiles;
- capability/action schemas;
- ontology/concept version maps;
- calibration artifacts valid for the declared domain;
- approved skill/tool specifications.

An exported artifact carries an applicability envelope, version/digest, required evidence, data-classification constraints and fallback. MiCode must still apply its own permission gate and host policy. "Approved by Axon" never means "authorized in MiCode."

## Identity and semantic mapping

MiCode semantic IDs and Axon semantic IDs are local namespaces. Cross-system mapping uses content/structure fingerprints plus source-system IDs and a versioned ontology map. A mapping may be Exact, EquivalentUnderProjection, Approximate, Superseded or Unresolved. Unresolved identity is not silently guessed.

Round-trip conformance preserves distinctions important to learning: denied vs failed vs aborted/not-executed; observed vs predicted; verified vs unverified; selected vs acceptable-but-unchosen; submitted vs effective model input; permission decision vs realized effect; task DONE claim vs independently verified completion.

## Replay and experiment use

Imported MiCode episodes may support:

- exact semantic replay over recorded decisions/results when all necessary artifacts exist;
- counterfactual policy evaluation without repeating external effects;
- resettable re-execution of permitted local fixtures;
- training/evaluation only when CX-10 eligibility permits it.

A recorded MiCode action outcome is historical evidence, not permission to repeat that action in Axon. Missing environment/model/tool inputs downgrade replay support explicitly.

## Security and governance

The bridge strips or transforms local secrets/credentials/authority handles before export. Raw source and logs remain subject to their originating data policy. Repository/license constraints attach transitively to derived knowledge candidates. Import validation runs before any artifact reaches training, inference, admission or runtime registries.

Schema evolution is append/migrate, not silent reinterpretation. Old episodes remain readable through pinned migration adapters. Cross-system schema changes require conformance fixtures on both sides before protected use.

## Acceptance gates

**G16-schema:** a representative coding episode containing denied, failed, aborted, successful and unknown outcomes round-trips MiCode → bridge → Axon reader without semantic field loss or invented defaults.

**G16-authority:** forged MiCode grants/risk ceilings/permission outcomes cannot create an Axon principal grant or bypass an Axon capability check; exported Axon approvals likewise do not authorize MiCode execution.

**G16-lineage:** every imported learning row retains source episode/repository/evaluator/data-use lineage and remains ineligible when required policy fields are absent.

**G16-version:** a schema/ontology/model-version mismatch is migrated by an explicit registered adapter or refused; stale calibration/applicability cannot silently attach to the new version.

**G16-replay:** an imported episode can drive semantic replay/counterfactual comparison without repeating external writes/network/model effects; unsupported replay inputs surface as Unsupported/Incomplete rather than guessed outcomes.

## Build slices and exclusions

Start with JSONL/JSON artifact interchange and a dummy MiCode producer/consumer fixture. Add real MiCode exports only after its canonical episode schema exists. Do not require MiCode availability for Cortex's first local repair loop; the bridge is an additional experience source, not a runtime dependency.
