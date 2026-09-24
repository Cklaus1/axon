# External repository knowledge ingestion build guide

## Objective

Use MiCode sessions and approved external repositories as evidence for better Axon abstractions without importing code popularity as truth.

## Canonical pipeline

```text
repo/history or MiCode episode
  → normalized semantic evidence
  → candidate pattern/concept
  → counterexample search
  → local reproduction
  → benchmark / held-out transfer
  → CX-11 admission
  → optional CX-18 crystallization
```

## Minimum evidence record

For each candidate store: source repositories/artifacts and revisions; exact extracted examples/non-examples; extraction version; hypothesized scope/mechanism; license/data-use policy; contradicting evidence; reproduction instructions/results; benchmark environment; held-out families; current evidence stage; dependent derived artifacts.

## First experiment

Choose a narrow software-engineering pattern that appears in multiple repositories and is relevant to Axon (for example a bounded retry/backoff, parser recovery pattern, or structured cancellation idiom). Reproduce at least two variants in resettable local fixtures. Measure correctness/resource/complexity effects. The goal is to validate the knowledge pipeline, not to prove a universal abstraction.

## Controls

- shuffled repository labels;
- pattern extracted from only one source;
- popular but locally worse implementation;
- contradictory examples;
- removed/invalidated source permission;
- duplicated/forked repositories counted as one lineage family;
- static-source-only versus history/outcome-aware extraction.

## Stop rules

Stop/pivot when provenance cannot be established, counterexamples invalidate the proposed scope, local reproduction fails, or held-out benefit is inconclusive under the registered evaluation policy. A failed candidate remains useful negative knowledge.
