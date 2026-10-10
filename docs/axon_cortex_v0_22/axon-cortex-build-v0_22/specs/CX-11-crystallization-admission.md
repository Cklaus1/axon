---
id: CX-11
title: "Crystallization and independent artifact promotion"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-06", "CX-10"]
first_stage: M5
implementation_evidence: []
---

# CX-11 — Crystallization and independent artifact promotion

## Intent and source basis

Turn repeated successful cognition into cheaper reusable artifacts while preserving independent admission. S2 pp.43–48 describes THINK→REFLEX→RULE and per-pillar improvement. S1 pp.22 and 83–84 supplies restricted compiler-rewrite gates as prior art. See [SOURCES](../SOURCES.md).

## Decisive fork

Use multiple guarded specialization paths with an independent admission authority. Crystallization is optional and reversible at the artifact level; it is not a claim that every reasoning task becomes a deterministic rule or that previous real-world effects can be undone.

## Candidate paths

Supported classes: prompt/router policy; calibration layer; retrieval policy; distilled decision adapter; tool/macro; guarded deterministic rule; predictor/representation; and restricted compiler rewrite. Each class declares its training/derivation method and assurance level.

A learned student is empirically compared to task outcomes, not declared equivalent to its teacher. A deterministic rule requires a proved property over a restricted domain, exhaustive finite validation, or clearly labeled empirical support plus fallback. Never label corpus agreement as universal equivalence. Macro composition must preserve prerequisites, intermediate effects, failure recovery and total budget.

## Artifact contract

`CandidateManifest` contains type/version/digest, parent/incumbent, applicability predicate, dependencies, authority/effect requirements, training/data lineage, evaluator policy, quality/calibration claims, resource profile, migration rules, fallback target and rollback/deoptimization procedure.

The bundle includes signed or otherwise authenticated gate evidence from the admission domain. The learner cannot write those reports as though they came from the protected runner. Applicability is evaluated against current state/model/schema/environment; failure deoptimizes to the approved baseline or blocks.

## Admission state machine

`Proposed → ValidatedOffline → Shadow → Canary → Active` with terminal Rejected, Suspended or Retired. A purely offline research candidate may never enter Shadow. Every transition is durable and authority-checked; workers cannot self-promote by updating a status field.

Admission gates are separate and conjunctive: schema/provenance; authority non-expansion; safety/adversarial checks; task-quality non-inferiority; calibration/applicability where used; resource benefit; compatibility/migration; and rollback rehearsal. Required missing/expired/inconclusive gates block.

Shadow mode cannot duplicate external actions; it only compares proposals/predictions with recorded or separately authorized observations. Canary is restricted to owner-approved reversible environments and known resource caps. Production irreversible actions need their own approval policy; they are not enabled by this spec.

## Quality and deoptimization

Use CX-01's preregistered statistical contract, not “no significant difference.” Recalibration, new prompt or tokenization, new candidate construction and new environment may invalidate evidence. Watch realized outcomes, drift, disagreement and error rates under the authorized monitoring policy.

On breach, disable new uses, preserve evidence, switch future eligible work to the baseline and reconcile in-flight actions. Artifact rollback may require state-schema migration or compensation; never assume reverting a binary reverses state or external effects. Keep a tested previous-good bundle and dependency closure.

## Self-modification boundary

A candidate can improve proof search or propose stronger invariants. It cannot edit the trusted proof checker, hidden test corpus, required-gate list, hard risk-policy enforcement, grant issuer or signing identity. Such changes use explicit Axon invariant/TCB governance and rerun incumbent/challenger evidence under both policy versions.

## Acceptance gates

**G11-independent:** a learner-written passing report without the expected admission provenance cannot promote; a policy/schema mismatch also refuses.

**G11-scope:** a compiled rule works within its declared domain and reliably falls back outside it; a changed candidate schema revokes stale applicability.

**G11-noninferiority:** evidence below precision/sample requirements yields INCONCLUSIVE; a fast but quality-regressing candidate fails.

**G11-authority:** a tool/macro or compiler pass requesting additional effects fails even when task score improves.

**G11-rollback:** inject a regression after activation in a reversible fixture; stop new use, restore the previous-good bundle and preserve/correct dependent state without repeating effects.

## Build slices and exclusions

First promote one small deterministic helper or prompt policy using the same admission path later used for learned models. Then train a bounded Reflex student on eligible examples. Automatic general program synthesis and arbitrary compiler self-rewrite are not initial deliverables.

## v0.4 OS-wide crystallization

Candidate origins now explicitly include native Cortex experience, MiCode-exported skills/episodes, external-repository knowledge candidates and generated curricula. Origin never weakens admission. Promotion may target a guarded skill/tool, Reflex/retrieval policy, Axon library abstraction, compiler transformation or runtime primitive. Required assurance increases with semantic/authority blast radius; useful userland artifacts need not be driven native.

## v0.9 compiled-cognition refinement

Crystallization targets now explicitly include **specialized Reflex models and typed neural programs** in addition to prompts, rules, tools and compiler rewrites. CX-22 chooses the candidate representation; CX-23 owns Neural Program compiler/runtime lifecycle; CX-36 owns the canonical `.nps`/`.np` artifact and portable contracts. A successful learned function is not presumed to deserve tool/library/compiler/runtime promotion. Each downward move creates a new immutable artifact and requires evidence appropriate to the larger semantic/authority blast radius.


## v0.13 self-application admission
CX-11 is the protected promotion authority for CX-29. Candidate components may produce artifacts and evidence but cannot alter their locked suites, evidence threshold, admission logic, previous-good target or rollback mechanism within the candidate change.

## v0.16 review amendment — Detached release identity and eligible rollback

Freeze candidate executable identity before evaluation. Evaluation, reliability and admission receipts bind the exact artifact/target/configuration but are detached from its bytes. Candidate registration is not activation. Updates to aliases/active pointers are atomic and current revocation is rechecked. Previous-good means currently eligible as well as historically good; a revoked previous-good artifact cannot be automatically restored. Candidate statuses stay governed by this spec; other registries expose views rather than competing state machines.

**G11-release-binding:** attaching an evaluation/admission receipt cannot change the evaluated subject; mismatched target conversion or stale/revoked rollback destinations refuse, including activation/revocation races.

See `build/REVIEW_INVARIANTS.md` for the shared interpretation and `review/BUILD_ADVERSARIAL_REVIEW.md` for attack cases.

## v0.18 ACE integration

ACE cannot publish/activate its own candidate. Current detached admission, revocation, receiving-project policy and rollback eligibility remain authoritative for every deployment mode and fallback. See [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md).

## v0.21 research integration amendment

This additive amendment is normative for the explicitly selected v0.21 profile. Existing authority and wire-format owners remain unchanged. The [research integration contract](../build/RESEARCH_INTEGRATION_V021.md) and [requirements matrix](../integration/RESEARCH_REQUIREMENTS.json) specify the complete profile and source-backed migration. Older versioned profiles remain separately identifiable; none of these declarations establishes runtime implementation.

### Acceptance gates added in v0.21

**G11-r21-prune-protections:** Pruning tests include regression, observability and required safety behavior; protected components cannot be removed because a narrow benchmark gives them zero apparent utility.

**G11-r21-evolution-independent-admit:** Winning or pruned strategies require existing independent admission and explicit activation scope; proposer/critic/judge identities cannot self-authorize publication or mutate the safety floor.

## v0.22 amendment — closed-loop Axon / Compute Fabric / MiCode integration

The [0.22 integration contract](../build/CLOSED_LOOP_INTEGRATION_V022.md) and [source-bound work packages](../build/WORK_PACKAGES_V022.md) extend this existing owner. They do not introduce a parallel authority, learning store, sandbox claim or schema reinterpretation. Original 0.21 research profiles remain available, but the bounded 0.22 pilot uses only its explicitly qualified dependency slice. The [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json) preserves ACF-01 M0-M2 obligations; the [MiCode consumer delta](../integration/MICODE_V022_CONSUMER_DELTA.md) binds actual peer work. All added runtime obligations are unexecuted proposals in this pack.

**G11-r22-workspace-cas:** Workspace publication requires expected base, current fencing epoch, authorized writer, exact verified output and independent approval; a concurrent update forces conflict/rebase and reverification, never overwrite.

**G11-r22-independent-admission:** Policy admission requires a non-subject authority, complete artifact/experiment/verifier bindings and the prespecified evidence rule; proposer, learned ranker and Compute Fabric have no self-promotion right.

**G11-r22-admission-disposition:** Accepted, rejected and inconclusive have explicit immutable reasons; only accepted and currently authorized evidence permits activation, and stale/unknown costs or safety failures cannot be hidden by aggregate utility.

**G11-r22-policy-cas:** Activation checks expected active policy, monotonic fence, exact admitted candidate, scope and revocation immediately before publication; competing/stale activations fail rather than overwrite one another.

**G11-r22-rollback-revalidate:** A rollback rechecks predecessor artifact, current applicability, permissions, profile qualification and revocation; a revoked or weaker predecessor causes a paused/refused state, not unsafe fallback.

**G11-r22-regression-observed:** An intentionally injected regression triggers the configured independent monitor, a recorded rollback/pause and a later task using the expected safe version; the injection is labeled a mechanism test, not measured improvement evidence.
