# Axon Cortex build package v0.3 — changes

This build-focused package folds the latest System-One/Jev ecosystem review into the v0.2 build contracts.

## New files

- `build/REFLEX_CONFORMANCE.md`
- `build/DEPENDENCY_ADOPTION.md`

## Major changes

- Batch Reflex results keyed by question ID with primitive-specific result types.
- Choice, binary/Noul-compatible and ordinal/Score-compatible semantics remain distinct.
- Added provider-reported distribution provenance.
- Added backend feature negotiation and effective-input/truncation receipts.
- Added trusted prompt-role boundary and question-isolation/model-call-fusion tests.
- Added candidate absence/search semantics and candidate-recall-before-ranking evaluation.
- Added multiple acceptable actions, behavior-policy lineage and unknown counterfactual handling.
- Added exact coverage/selective-error accounting and benchmark comparability review.
- Added transitive dependency/model/tokenizer/encoder/dataset/evaluator adoption manifests.
- Fixed M2 sequencing: learned-head backend is optional research in M2, not a self-justifying prerequisite.
- Added B42 Reflex conformance and B43 dependency adoption work packages plus new acceptance gates.
