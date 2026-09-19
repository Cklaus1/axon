# Cognitive specialization build guide

This guide implements CX-22. It turns recurring expensive cognition into candidate cheaper representations without assuming that one universal Reflex model is optimal.

## Build objective

Demonstrate one end-to-end specialization:

`verified recurring operation → eligibility profile → candidate representations → matched evaluation → applicability/fallback → CX-11 evidence bundle`.

## Inner loop

1. Select one low-risk cognitive function with stable typed inputs/outputs and verified outcomes.
2. Reconstruct its eligible history from CX-10/MiCode without leaking protected test partitions.
3. Measure schema/candidate/domain stability and learning curves.
4. Implement the simplest candidate first: deterministic rule/template if plausible, then small specialized model/neural program.
5. Run matched dev/transfer tests against the incumbent/general Reflex.
6. Inspect errors, OOD and abstentions; narrow the applicability guard before adding model complexity.
7. Stop when the preregistered quality/resource claim is established, rejected or inconclusive.

## Outer loop

Across specialization families, maintain a registry of `CognitiveFunctionProfile` records. Rank experiments by expected savings × frequency × evidence quality, never by raw call count alone. High-risk authority/proof/verifier functions are excluded unless deterministic equivalence is available.

## Meta loop

Ask whether the specialization machinery itself is producing durable savings:

- Are specialized artifacts actually used inside their applicability envelopes?
- Do fallback rates or OOD events erase latency gains?
- Are maintenance/retraining costs dominating?
- Are too many one-off models accumulating?
- Should a family move down to a rule/tool or back up to general Reflex?

The meta loop may propose thresholds/representation families; CX-11/governance approves changes to admission policy.

## Required comparison matrix

At minimum record incumbent/general Reflex plus one candidate. Where useful include:

- deterministic rule/template;
- schema-conditioned encoder classifier;
- candidate-token scorer;
- pointer/listwise scorer;
- neural program;
- THINK/GENERATE fallback.

## Definition of done

A specialization is not "done" because training succeeded. The slice completes only when artifact identity, applicability, fallback, drift response, matched evaluation and resource accounting are all present, or the experiment is closed as rejected/inconclusive.
