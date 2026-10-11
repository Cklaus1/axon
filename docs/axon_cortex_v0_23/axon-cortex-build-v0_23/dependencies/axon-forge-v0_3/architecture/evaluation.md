# Evaluation

Evaluation order:

1. semantic verification;
2. build validity;
3. correctness;
4. security/policy;
5. required acceptance tests;
6. optimization objectives.

A faster candidate that violates a hard constraint is invalid.

Evaluators and judge prompts are versioned artifacts. Subjective domains MAY use blind pairwise model judgment, calibrated judge panels, human preference, or rubrics; agreement/confidence SHOULD be recorded.

Anti-Goodhart controls SHOULD include hidden/held-out cases, multiple metrics, rotating tests, adversarial tests, cross-evaluator checks, and periodic human review.
