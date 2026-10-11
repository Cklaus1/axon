# Changelog v0.6 — Reflex runtime and listwise decision refinement

Prepared 2026-09-18. Documentation/build-plan changes only.

## Added

- `build/REFLEX_RUNTIME.md`.
- `StateHandle` / shared-state runtime contract and backend-emulation semantics.
- Question scheduling classes: `Independent`, `ConditionallyRelevant`, `AnswerDependent`.
- Candidate-order policy/digest as part of inference and calibration scope.
- Question-isolation conformance and shared-state identity/isolation gates.
- Listwise/pointer/independent model research matrix.
- Outcome-Calibrated Decision Training terminology and proper-scoring requirements.
- Work packages B58–B61 and gates G05-state-handle, G05-question-isolation, G05-order-domain, G14-listwise, G14-training-score, G15-dependency-types, G15-batch-semantics.

## Preserved

- Backend-neutral Reflex ABI.
- Dynamic action spaces and DECIDE ≠ GENERATE ≠ ACT.
- Independent verification/admission.
- Intent IR, world-model, MiCode bridge, knowledge ingestion and self-optimizing OS roadmap.
- Custom model architecture remains optional and evidence-gated.

## Explicit non-claims

The Archer Hume article is behavioral analysis and architectural inference. v0.6 does not claim TypeSafe uses a causal decoder, sparse MoE, listwise transformer, or any specific RLCD implementation.
