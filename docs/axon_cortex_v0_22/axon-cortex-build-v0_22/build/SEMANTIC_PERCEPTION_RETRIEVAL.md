# Semantic Perception and Retrieval build guide

Implements CX-24.

## Goal

Build a typed semantic layer between raw repository/log observations and bounded Cortex decisions. The first slice should answer propositions over real software objects while preserving object/span provenance and uncertainty.

## Inner loop

1. Select a low-risk corpus with protected gold/derived evidence.
2. Define one typed extraction schema and 3–10 semantic propositions.
3. Establish lexical/AST/embedding baselines and candidate recall.
4. Add one local semantic matcher/extractor backend.
5. Calibrate or register explicit gray-zone thresholds on calibration data only.
6. Compose propositions deterministically with AND/OR/NOT.
7. Feed matching object IDs into the ordinary Reflex/capability pipeline.
8. Run contrastive, negation, actor/action, multilingual/code and evidence-deletion tests.

## Outer loop

Expand from lines/logs to symbols, diffs, commits and repository-history objects only after the previous object family meets protected quality/privacy budgets. Prefer cheap recall + semantic reranking to scanning an entire repository with an expensive cross-encoder.

## Meta loop

Measure whether semantic perception/retrieval actually reduces downstream THINK calls, context volume and tool reads while preserving Verified Coding Frontier quality. If not, keep it as an optional search tool rather than a mandatory observation stage.

## Definition of done

The slice has typed provenance-preserving outputs, a registered probability source, deterministic composition, local/remote disclosure controls, matched baselines and downstream utility evidence. A demo that merely returns plausible semantic hits is insufficient.
