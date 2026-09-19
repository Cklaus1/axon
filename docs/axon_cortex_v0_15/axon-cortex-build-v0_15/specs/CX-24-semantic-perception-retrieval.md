---
id: CX-24
title: "Semantic Perception and Retrieval: schema-conditioned extraction and proposition scoring"
status: Draft
authority: Proposed
depends_on: ["CX-02", "CX-05", "CX-10", "CX-20", "CX-22"]
first_stage: M2
implementation_evidence: []
---

# CX-24 — Semantic Perception and Retrieval

## 1. Purpose

Cortex needs a learned layer between raw software/environment text and higher-level planning. Not every useful model call is a bounded action choice or open-ended generation. Some calls should **extract structured observations**; others should answer a proposition such as "does this symbol write mutable global state?" for many objects cheaply.

CX-24 defines two learned primitives:

- **Semantic Perception** — schema-conditioned extraction/classification from text, logs, diffs, code fragments or other approved observations into typed facts, spans, relations or records.
- **Semantic Match** — proposition-conditioned scoring of an observed object or chunk, returning an explicitly sourced distribution/score that ordinary code may threshold and compose.

The public GLiNER2 family, Jev-style semantic grep experiments and next-token classifier servers are implementation leads, not normative dependencies.

## 2. Architectural position

```text
raw observations
      ↓
Semantic Perception / Match
      ↓
typed semantic observations + evidence object IDs
      ↓
Observer / Retrieval index / Reflex / Planner
      ↓
capability compiler and ordinary authority checks
```

Perception may enrich the world state; it may not create authority. Semantic matching may rank or filter candidates; deterministic freshness, permission and verifier checks remain outside it.

## 3. Typed contracts

Conceptual contracts:

```text
SemanticExtract<I, O> {
  schema_id,
  model/backend identity,
  applicability,
  provenance,
  calibration domain?
}

extract(input: I, schema) -> Result<Observed<O>, PerceptionError>

SemanticProposition {
  proposition_id,
  text/typed form,
  threshold_policy?,
  polarity
}

match(object_ref, proposition) -> SemanticMatchResult
```

`Observed<O>` records source object/span IDs and confidence/probability provenance. Missing evidence remains Unknown; it is never converted to false merely because an extractor found nothing.

## 4. Retrieval role

Semantic Match complements rather than replaces lexical/AST/embedding retrieval.

Use deterministic indexes for exact identity and cheap high-recall recall. Use semantic proposition scoring when the query depends on relationships, negation, actor/action distinctions, policy conditions or other cross-encoder judgments. The retrieval router must compare cost, recall and downstream utility.

For large corpora, use a staged pipeline:

`cheap candidate recall → semantic matcher/reranker → bounded candidate set → Reflex/Think`.

## 5. Deterministic composition

Independent semantic propositions may be composed by code using registered AND/OR/NOT/threshold expressions. The composition expression and threshold policy are versioned artifacts. A model never receives authority merely because several probabilistic predicates evaluate above a threshold.

A gray zone may be represented explicitly:

```text
positive if p >= high
negative if p < low
otherwise Unknown / ObserveMore
```

This avoids forcing uncertain evidence into booleans.

## 6. Model families

The research lab may compare:

- schema-conditioned bidirectional encoders / extraction models;
- next-token candidate scorers over a shared prefix;
- listwise/pointer decision models;
- general Reflex backends;
- embedding retrieval plus reranking;
- deterministic/static-analysis baselines.

Model family is chosen by measured quality/transfer/cost, not brand.

## 7. Source and probability semantics

The ABI distinguishes at least:

- `GeneratedEstimate` — model-generated probability-like text;
- `TokenLogitDistribution` — normalized selected-token logits;
- `DecisionHeadDistribution` — learned discriminative head/pointer outputs;
- `EmpiricallyCalibrated` — a distribution accompanied by current held-out calibration evidence.

A result may carry more than one provenance layer (for example a token-logit distribution with an empirical calibration transform). Policy must not erase these distinctions.

## 8. Privacy and locality

Semantic retrieval may touch broad repository/log corpora. The system records whether inference is local or remote, which source objects were disclosed, data-use constraints, and redaction/projection transforms. Protected secrets and disallowed corpus roles cannot be exported merely because semantic search would be useful.

## 9. Training and learning data

MiCode/Cortex may emit `SemanticObservationRecord` and `SemanticMatchRecord` examples with exact source identity, proposition/schema version, model/backend version, raw score provenance, verifier/downstream outcome and corpus role. Synthetic labels remain synthetic; deterministic/static-analysis evidence is preferred when available.

Contrastive and deletion tests are especially useful: relevant factual changes should move predictions while irrelevant edits should not; removing required evidence should produce Unknown/ObserveMore rather than a fabricated negative.

## 10. Acceptance gates

**G24-schema:** schema-conditioned extraction preserves declared types, source/span provenance and Unknown; malformed or unsupported outputs fail explicitly rather than being coerced.

**G24-provenance:** generated estimates, selected-token logits, decision-head distributions and empirically calibrated probabilities remain distinguishable through routing, replay and evidence export.

**G24-semantic-match:** on a protected proposition-search suite, semantic matching beats or complements a registered lexical/embedding baseline on preregistered downstream recall/precision/utility without changing the authority surface.

**G24-composition:** registered AND/OR/NOT/threshold expressions are deterministic, replayable and preserve an uncertainty band; reordering equivalent boolean expressions does not change the result outside declared floating tolerance.

**G24-routing:** the retrieval/perception router selects among deterministic, embedding, semantic-match and general-model paths under a registered policy and falls back/abstains outside applicability rather than trusting raw confidence.

**G24-privacy:** a protected/secret fixture is not sent to a remote semantic backend unless the active data-use/authority policy explicitly allows that disclosure; local-only policy fails closed when a remote-only backend is selected.

## 11. First build slice

Implement semantic proposition search over a resettable coding corpus (symbols, diagnostics or log lines). Compare exact/embedding retrieval against one local semantic matcher, preserve object/span IDs, add a gray-zone threshold, and feed only the resulting bounded objects—not raw model strings—into the existing Cortex candidate pipeline.

## v0.12 semantic predicates in query planning

Semantic match/filter/rank should behave like an expensive predicate in a query optimizer: apply exact authorization and deterministic predicates first, use cheap lexical/index narrowing where measured recall permits, batch semantic evaluation, cache only against stable content/definition identity, and preserve explicit no-match/uncertainty outcomes. Learned full scans are not the default merely because the predicate is semantic.
