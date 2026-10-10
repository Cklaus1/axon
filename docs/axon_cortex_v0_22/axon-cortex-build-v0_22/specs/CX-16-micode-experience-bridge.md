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

## v0.12 MiCode supervisor/context records

The bridge may ingest MiCode `SupervisorDecisionRecord`-like and working-set/context-selection records from the v0.5 support plan for replay and research. Cortex rebinds them to Axon corpus roles, authority contracts and protected outcomes. MiCode relevance, completion or supervisor labels are never treated as Axon admission decisions by themselves.

## v0.14 repository-improvement bridge

CX-16 additionally carries project-local optimization artifacts between MiCode and Axon: `ProjectImprovementContract`, `ProjectBaseline`, `ProjectImprovementCandidate`, `RepositoryExperimentReceipt`, project-family metadata and transfer-evaluation references. MiCode may execute repository-local experiments, but the active contract remains the authority boundary and Axon admission remains separate from builder self-report.

## v0.18 ACE amendment — ACE owner-first MiCode profile

Export P01–P13 and the versioned projection/mapping to the existing MiCode bridge. Keep MX permission/completion authority local, original source lineage intact and T/P transfer namespaces separate. The core host slice and real peer slice have different claims; user views must preserve requested/selected/attempted/observed/accepted producer and next-turn recovery.

See the normative [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md) and [build guide](../build/ACE_INTEGRATION.md). These requirements extend this owner; no second ACE subsystem is introduced.

**G16-ace-core:** One authorized existing real backend runs through a real Axon consumer with exact effective input, typed output, dispatch checks and complete attempt/error/recovery receipts; no trained custom model is required.

**G16-ace-peer:** Pinned Axon and MiCode revisions exercise the profile through the actual client/build-loop with local permission, independent evidence, fallback/cancellation/auth failure and next-turn recovery; mocks cannot close this gate.

## v0.19 amendment — calibrated decision peer semantics

MiCode may consume Axon decision distributions, correctness estimates and calibration provenance only losslessly. It re-applies local PermissionGate, risk thresholds and completion checks. Fan-out/join and high-cardinality lineage must preserve question identity, effective input and candidate-set transitions across the bridge. Production outcome feedback may return evidence/corpus records but cannot remotely activate an Axon calibrator/model.

**G16-v019-peer:** On pinned Axon v0.19/MiCode v0.13 revisions, the peer contract preserves calibration-domain/provenance, fan-out question identity, high-cardinality shortlist lineage and independent completion/authority semantics; live interoperability remains NOT_RUN until exercised on real counterparts.

## v0.20 amendment — schema decisions and lightweight Reflex specialization

MiCode v0.14 consumes the v0.20 schema/fixed-task profiles through the existing bridge. Preserve schema/source/candidate IDs, exact projections, active-result absence, task/label/runtime qualification, label provenance and null threshold semantics. MiCode keeps local credentials, PermissionGate, workspace effects and independent completion checking. Pair B219 with the MiCode integration slice at pinned live revisions; the versioned fixture exchange alone is not that live experiment.

**G16-v020-peer:** Axon v0.20 and MiCode v0.14 retain matching owner snapshots and lossless schema/span/task/runtime/label/threshold provenance; a real low-risk paired slice separately proves PermissionGate, independent checks, refusal, cancellation and next-turn recovery.

## v0.21 research integration amendment

This additive amendment is normative for the explicitly selected v0.21 profile. Existing authority and wire-format owners remain unchanged. The [research integration contract](../build/RESEARCH_INTEGRATION_V021.md) and [requirements matrix](../integration/RESEARCH_REQUIREMENTS.json) specify the complete profile and source-backed migration. Older versioned profiles remain separately identifiable; none of these declarations establishes runtime implementation.

### Acceptance gates added in v0.21

**G16-r21-peer-proposed:** MiCode 0.15 is explicitly a proposed consumer target until its source and pack are reviewed; no nonexistent updated peer archive, test execution or export lock is claimed.

**G16-r21-peer-roundtrip:** An actual pinned Axon/provider/MiCode run must preserve candidate, principal, model, score, context, spend and effect semantics including unsupported-profile refusal, cancellation and next-turn recovery.

## v0.22 amendment — closed-loop Axon / Compute Fabric / MiCode integration

The [0.22 integration contract](../build/CLOSED_LOOP_INTEGRATION_V022.md) and [source-bound work packages](../build/WORK_PACKAGES_V022.md) extend this existing owner. They do not introduce a parallel authority, learning store, sandbox claim or schema reinterpretation. Original 0.21 research profiles remain available, but the bounded 0.22 pilot uses only its explicitly qualified dependency slice. The [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json) preserves ACF-01 M0-M2 obligations; the [MiCode consumer delta](../integration/MICODE_V022_CONSUMER_DELTA.md) binds actual peer work. All added runtime obligations are unexecuted proposals in this pack.

**G16-r22-closed-wire:** Both real peers refuse duplicate or escaped-alias keys, unknown closed fields, malformed digests, unsafe numeric values and incompatible required capabilities before model calls or dispatch; a syntactic fixture does not prove transport interoperability.

**G16-r22-negotiation:** Old episodes remain readable through pinned adapters; an absent/old peer produces explicit Unsupported or retained-authority incumbent operation, never silent field loss, fabricated peer support or weaker protected execution.

**G16-r22-preflight-start:** A mismatched repository, exact trial base, observed branch/worktree, role, namespace, provider/model or dedicated build namespace yields TASK_NOT_STARTED before the first task model turn and before effects; a parent-echo receipt is rejected.

**G16-r22-preflight-return:** Result intake rechecks current integration head, context receipt and declared write scope; stale worker success is classified for rebase/conflict/obsolescence, never silently admitted against a different base.

**G16-r22-role-scope:** Read-only critics/verifiers/documentation/implementation roles have distinct enforceable context and write/build contracts; experiment subjects cannot build in shared mutable namespaces or widen their own declared write set.

**G16-r22-local-authority:** An Axon policy may only select from already-permitted candidates; delegated tools and concrete file effects are checked against inherited local limits. Unknown glob/subset relations are denied or escalated for explicit review.

**G16-r22-trust-graduation:** MiCode trust-judge enforcement stays behind its existing graduation evidence and per-mode criteria; a new EVL or passing Fabric check cannot rewrite PUBLISHED_VERDICT or imply trust-judge graduation.

**G16-r22-real-consumer:** A pinned real Axon producer and real MiCode consumer exchange a policy through an actual coding/build-loop use site, record the effective policy digest and preserve subsequent-turn recovery; a dummy reader cannot close this gate.

**G16-r22-candidate-shortlist:** The pilot only reorders or narrows a known eligible tool/skill set; no new tool, permission, verifier, model, compute profile, credential route or budget is introduced by a policy candidate.

**G16-r22-provider-host-boundary:** Approved inference runs in the trusted permission-enforced host path with secrets excluded from guest/episode exports; the initial guest remains offline, and denied broker/egress requirements never silently enable guest network access.

**G16-r22-real-producer:** The actual MiCode episode exporter reaches the actual Axon intake and round-trips known/unknown, requested/effective and predicted/observed facts without omission or manufactured defaults across normal and failed tasks.

**G16-r22-future-task-uptake:** After real admission, a later independent MiCode task consumes and reports the exact activated policy digest, not just a stored benchmark winner; failed acknowledgement yields quarantine/paused routing rather than claimed deployment.

**G16-r22-inflight-policy-pin:** In-flight tasks retain their original pinned policy/version and receipts; a subsequent activation cannot relabel their earlier actions or mix two policies within an unrecorded trial.

**G16-r22-peer-failure-matrix:** Peer outage, replayed messages, stale epochs, partial episode export and schema mismatch preserve local authority and unknown status; reconnecting does not duplicate activation, billing or task effects.

**G16-r22-source-ci-scope:** Paired Axon/MiCode revision, build features, host/profile, executed test names and nonzero assertions are captured for source/interop jobs; skipped or unavailable compiler/KVM/provider tests remain NOT_RUN or BLOCKED.

**G16-r22-operational-loop:** One authorized real Axon-MiCode-Fabric workflow completes identity-bound dispatch, evaluation, admission/refusal, future-task policy use and rollback/pause, with qualified physical backend and real peer evidence; fixtures alone cannot close the release.

**G16-r22-bounded-activation:** The release report enumerates exactly qualified scopes/features and explicitly disabled/deferred ones; autonomous operation stays within preauthorized pilot limits and cannot expand its own admission or evaluation rules.
