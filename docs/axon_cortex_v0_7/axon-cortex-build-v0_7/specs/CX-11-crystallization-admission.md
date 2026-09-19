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

A candidate can improve proof search or propose stronger invariants. It cannot edit the trusted proof checker, hidden test corpus, required-gate list, risk classifier, grant issuer or signing identity. Such changes use explicit Axon invariant/TCB governance and rerun incumbent/challenger evidence under both policy versions.

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
