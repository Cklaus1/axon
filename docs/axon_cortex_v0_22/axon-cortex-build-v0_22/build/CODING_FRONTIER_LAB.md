# Coding Frontier / Benchmark Lab build guide

This document turns [CX-21](../specs/CX-21-coding-frontier-benchmark-lab.md) into build work. It evaluates the **whole Cortex coding system**, not only Reflex.

## Objective

Maintain a protected, reproducible scoreboard answering whether the current Axon/Cortex stack moves the Verified Coding Frontier under declared quality, authority, cost, latency and compute envelopes.

## Build order

1. **Task registry + reset harness** — immutable task/intent, authority, reset, resource and acceptance manifests.
2. **Protected verifier** — completion determined outside the candidate's editable surface.
3. **Baseline portfolio** — repair, debugging, feature, refactor and optimization fixtures plus simple controls.
4. **Transfer suites** — repository/family/task-family splits and contamination metadata.
5. **Frontier reporter** — quality/coverage/cost/time curves, Pareto views and regression matrix.
6. **Curriculum/experience intake** — independently admit MiCode/generated/external tasks without making training data into hidden tests.
7. **Admission handoff** — immutable benchmark evidence bundle for CX-11.

## Inner benchmark loop

```text
reset task
→ validate task/system manifests
→ run one bounded coding episode
→ protected verify
→ collect complete resource/outcome accounting
→ persist immutable episode/evidence
```

No model may convert an internal score, prediction or `DONE` token into completion.

## Outer benchmark loop

```text
select frozen suite
→ run candidate + controls under matched contracts
→ aggregate by task family / transfer level
→ compute quality-cost-time frontier
→ inspect regressions / failures / abstentions
→ issue scoped benchmark claim
```

The outer loop may compare systems. It may not tune the candidate against locked outcomes.

## Meta-evaluation loop

```text
observe benchmark blind spots / saturation / contamination
→ propose new task family or retirement
→ independent benchmark review
→ version next suite
→ preserve old-suite lineage
```

The candidate optimizer can propose benchmark additions but cannot silently edit its own active exam.

## Required baseline systems

At minimum preserve:

- simple deterministic/rule control where applicable;
- strong-model coding baseline under the same authority/tools;
- previous admitted Cortex release;
- component ablations when attribution matters.

## Required artifacts

- task manifest;
- suite manifest + hashes;
- system-under-test manifest;
- evaluator/verifier identity;
- full episode records;
- resource accounting;
- contamination report;
- aggregate frontier report;
- regression matrix;
- claim-scope statement;
- CX-11 evidence bundle when used for promotion.

## Stop rules

Stop or downgrade the claim when:

- resetability/protected verification fails;
- contamination invalidates the claimed transfer tier;
- task eligibility differs silently;
- failed attempts are missing from accounting;
- confidence intervals/evidence are too weak for the registered claim;
- benchmark changes after candidate outcomes are observed.

## First useful demonstration

Run one current Cortex vertical slice and one matched strong-model baseline across a small frozen suite containing:

- localized repair;
- wrong-file distractor repair;
- multi-file dependency bug;
- small feature addition;
- behavior-preserving refactor;
- micro-optimization with benchmark acceptance.

Report verified completion, wall time, model cost, tool/build calls, human intervention, failure categories and regressions. Do not collapse the result into one score.
