# Cross-Project Learning — build guide

CX-31 is not a repository popularity miner and not a generic code-training crawler. It operates on governed project episodes produced under CX-30/CX-16/CX-10.

## Inputs

Use project episodes with immutable contract references, source revision, family labels, outcome evidence, corpus role and data-use policy. Cluster forks/templates/near-duplicates before any transfer claim.

## Pattern discovery

Candidate pattern records should include positive examples, counterexamples, applicability predicates, hypothesized mechanism, source families, transfer tier, evidence stage and proposed destination.

## Evaluation

Evaluate at the project-family level, not only by pooled examples. Hold out complete repository families for transfer claims. Report failures and confidence intervals where statistically meaningful; do not let a large family dominate the aggregate silently.

## Promotion

Shared skill-level reuse may require modest transfer evidence. Native compiler/runtime promotion requires repeated held-out transfer, stable semantics, lower maintenance burden and a clear rollback/de-generalization path.

## De-generalization

If new evidence contradicts a promoted artifact, create an `ApplicabilityRevision` rather than erasing the result. Narrow, split or demote the artifact and re-run affected project evaluations.
