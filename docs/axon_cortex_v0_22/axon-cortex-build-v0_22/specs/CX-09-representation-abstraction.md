---
id: CX-09
title: "Representation search and transferable abstractions"
status: Draft
authority: Proposed
depends_on: ["CX-07", "CX-08"]
first_stage: M6
implementation_evidence: []
---

# CX-09 — Representation search and transferable abstractions

## Intent and source basis

Test whether new representations and reusable concepts improve novel problem solving. S2 pp.33–44 motivates reframing, hypothesis invention, analogy and crystallization. S1 pp.83–84 supplies a constrained compression prototype. See [SOURCES](../SOURCES.md).

## Decisive fork

Treat representation/abstraction discovery as a bounded empirical research program over typed artifacts. Do not equate a new concept name, attractive explanation or small AST with demonstrated fluid intelligence.

## Representation contract

A `Representation` records source observations, adapter/generator artifact, representation schema, entities/relations, assumptions, lossy transformations, origin mapping, allowed queries, measured predictive/solver utility, complexity proxy and validation results. Original evidence remains recoverable through references; no lossy transform may silently drop an authority constraint or contradicting observation.

Initial representation candidates are AST/type relationships, dependency/dataflow graphs, state machines and constraint encodings. They are generated and compared under a fixed resource budget. Reuse native parser/type data where possible rather than asking models to infer it from text. Generated encoders run under the same sandbox as other untrusted tools.

A `ConceptCandidate` contains a typed definition or executable recognizer, supporting examples, counterexamples, relations to existing concepts, applicability predicate, claimed uses and testable transfer predictions. New vocabulary alone is insufficient. Refinement, split and merge operations preserve provenance and invalidate dependent caches/artifacts when semantics change.

## Search and admission

Search over representation transformations and concept definitions. Evaluate on held-out examples and downstream decisions. Complexity can regularize search, but acceptance requires a preregistered quality/transfer benefit or a proved scope-specific simplification. Penalize residual exceptions explicitly; do not compress away rare safety-relevant cases.

Representation-dependent claims state their assumptions. If a dataflow abstraction ignores concurrency, it must not be used to claim concurrency safety. A synthesized invariant is an untrusted proposal until checked. A candidate proof's validity is limited to its theorem and assumptions.

Admission produces an immutable representation/concept artifact with a domain envelope, version, metrics, evidence and deoptimization path. An accepted concept may aid retrieval or planning without becoming a language primitive. Compiler integration belongs to CX-15.

## Research evaluation

Separate discovery tasks from transfer tasks by family and construction mechanism, not just by different variable names. Compare against the same strong model without the new representation, with a standard human-provided representation, and with shuffled/ablated concepts. Report examples needed to learn, compute spent and regression on existing tasks.

Use curriculum tasks only for training/development. A generator cannot choose the final audit problems or reward itself for recognizing its own templates. Keep public/in-scope prior exposure recorded rather than claiming impossible-to-verify total novelty.

For analogy, test relation-preserving transfer and the conditions under which it fails. Superficial similarity scores are not evidence of a shared mechanism.

## Acceptance gates

**G09-lineage:** every derived node/claim maps to source evidence or an explicit hypothesis; unsupported invented facts cannot masquerade as observations.

**G09-counterexample:** a discovered concept is tested on known counterexamples and out-of-domain cases, with appropriate abstention or failure.

**G09-transfer:** preregistered novel-family tasks show the claimed gain against representation and compute-matched baselines; inconclusive results stay research artifacts.

**G09-compression:** a shorter model that violates fixed held-out fit/safety constraints fails, even if its training fit or description length improves.

**G09-ablation:** removing the concept or scrambling its assignments measurably tests whether it—not additional context or compute—caused the gain.

## Build slices and exclusions

First compare two fixed encodings on one task family. Next allow a bounded generator to propose transformations. Only then attempt reusable new concepts and cross-domain transfer. No promised date for general intelligence, no automatic promotion into compiler syntax and no mutable shared ontology without migration records.
