# Semantic Alignment Loop

Extends CX-20. Decision quality can improve by changing the semantic definition of a decision, not only by changing model weights.

## Objects under optimization

Treat the following as independently versioned experimental artifacts:

- question/instruction wording;
- criteria/option descriptions;
- question decomposition;
- deterministic composition rule;
- candidate-generation policy;
- candidate ordering policy;
- state projection;
- model/runtime configuration.

A change to one dimension must not be reported as a model improvement in another.

## Inner loop

1. Evaluate a frozen development pool.
2. Select uncertain/error examples plus a random audit sample.
3. Obtain protected verifier/human labels where appropriate.
4. Propose one semantic-definition change or decomposition.
5. Re-evaluate on development data and destructive controls.
6. Produce a semantic diff and evidence bundle.
7. Accept/reject/rewind through the ordinary research/admission workflow.

The loop never auto-promotes because a development score increased.

## Outer loop

Accumulate production/MiCode examples under purpose and contamination labels. Periodically revisit recurring decision families, preserving full version lineage from examples to semantic-definition revisions.

## Meta loop

Measure which source of improvement is paying off:

```text
better state representation?
better candidate catalog?
better question/criteria?
better decomposition/composition?
better model?
better calibration?
```

Allocate experiments accordingly. Do not default to fine-tuning when a semantic-definition fix is cheaper and more transferable.

## Required evaluation

- held-out and transfer quality;
- calibration/selective risk;
- question isolation;
- candidate-order sensitivity;
- decomposition ablation;
- definition stability across task/repository families;
- human/verifier disagreement and random-audit error rate.
