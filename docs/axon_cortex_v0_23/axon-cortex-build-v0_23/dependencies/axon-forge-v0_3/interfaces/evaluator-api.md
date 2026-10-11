# Evaluator API

```text
Evaluator.evaluate(candidate, context) -> EvaluationEvidence
```

Evidence MUST identify evaluator id/version, subject artifact, hard-constraint results, metrics, observations, confidence when applicable, and provenance.

A candidate MUST NOT be able to mutate the evaluator or hidden inputs judging that same candidate.
