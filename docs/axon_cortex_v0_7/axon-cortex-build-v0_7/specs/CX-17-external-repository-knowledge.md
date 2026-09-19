---
id: CX-17
title: "External repository knowledge ingestion and evidence ladder"
status: Draft
authority: Proposed
depends_on: ["CX-09", "CX-10", "CX-16"]
first_stage: M8
implementation_evidence: []
---

# CX-17 — External repository knowledge ingestion and evidence ladder

## Intent and source basis

Let Cortex learn software-engineering patterns from external repositories, including episodes produced by MiCode, without confusing popularity, repetition or teacher opinion with correctness. External experience is one source of hypotheses; local reproduction and protected evaluation decide what Axon may claim or promote.

## Decisive fork

Ingest repositories as structured evidence histories, not as undifferentiated training text. Preserve source provenance, license/data-use constraints, before/change/after relationships, tests/benchmarks/reverts and counterexamples. Promote only through an evidence ladder that is stronger than observation frequency.

## Inputs

Permitted inputs may include source trees, dependency/build manifests, commit history, tests, issues/PR metadata where authorized, benchmarks, release/migration notes, security fixes, reverts, and MiCode canonical episodes. Source availability and data-use rights are explicit per artifact.

Static snapshots are useful but weaker than histories that expose change and outcome. A commit message or merged PR is not itself ground truth; evidence strength depends on reproduced behavior and independent checks.

## Knowledge candidate types

- structural architecture patterns;
- implementation/procedural patterns;
- bug/fix and failure signatures;
- test-selection/verification strategies;
- performance transformations;
- concurrency/resource-lifetime patterns;
- security/invariant patterns;
- API/error-handling idioms;
- reusable representations/abstractions;
- negative knowledge: reverted, deprecated, failed or fragile approaches.

Each candidate stores source examples, non-examples/counterexamples, context predicates, hypothesized mechanism, evidence stage, uncertainty, source/license policy and derived-artifact lineage.

## Evidence ladder

A candidate may advance through:

`Observed → Repeated → OutcomeAssociated → LocallyReproduced → Benchmarked → HeldOutVerified → PromotionEligible`

Stages are not automatic. Many repeated patterns should remain descriptive knowledge. `OutcomeAssociated` is not causal proof. Local reproduction must state the environment and intervention. Held-out verification uses task/repository families not used to construct the candidate.

The system records conflicting evidence rather than averaging it away. A pattern useful in one ecosystem may become a guarded domain-specific abstraction rather than a universal rule.

## Knowledge extraction loop

1. canonicalize permitted repository/history inputs;
2. build semantic objects and change/outcome links;
3. propose candidate patterns/abstractions using retrieval, Reflex and/or deliberate reasoning;
4. search for corroborating and contradictory examples;
5. design local reproductions or controlled experiments when feasible;
6. score compression/prediction/transfer benefit;
7. submit only sufficiently evidenced candidates to CX-11/CX-18 admission paths.

The learner that discovers a pattern does not certify its value. Repository rank/stars/downloads are metadata, not assurance.

## Data governance

Respect license, confidentiality, contributor and dataset-use restrictions. Derived summaries/concepts retain required source attribution or use restrictions. If policy later invalidates an input, dependent training/model/knowledge artifacts become quarantined or reevaluated according to CX-10 lineage; provenance is never discarded because the abstraction looks generic.

## Acceptance gates

**G17-provenance:** a knowledge candidate derived from several repositories can enumerate its exact source artifacts, transformations, versions and data-use constraints; missing lineage blocks promotion eligibility.

**G17-counterexample:** the extractor actively finds/reports counterexamples or records that none were found under a bounded search; a contradicted universal claim is narrowed or rejected rather than averaged into confidence.

**G17-reproduce:** at least one nontrivial candidate reaches LocallyReproduced/Benchmarked through an independently resettable Axon experiment; failure to reproduce lowers evidence rather than being omitted.

**G17-license:** a source marked disallowed for training/derivation cannot enter an eligible dataset or promoted artifact; downstream manifests preserve the restriction.

**G17-heldout:** a candidate claiming transfer or generality is evaluated on held-out repositories/task families with matched baselines; inconclusive evidence remains non-promotable.

## Build slices and exclusions

Begin with read-only extraction from a small, owner-approved repository set and MiCode-exported episodes. First deliverable is a provenance-rich candidate notebook/store, not automatic compiler/runtime modification. Issue/PR ingestion and large-scale crawling remain optional until source/legal policies and dedupe/contamination controls are operational.
