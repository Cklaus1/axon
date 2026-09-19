---
id: CX-01
title: "Evaluation, baselines and assurance evidence"
status: Draft
authority: Proposed
depends_on: ["CX-00"]
first_stage: M0
implementation_evidence: []
---

# CX-01 — Evaluation, baselines and assurance evidence

## Intent and source basis

Make architectural complexity earn its place through reproducible end-to-end comparisons. S1 pp.10–11 warns that an earlier statefulness metric did not support the thesis; S2's fluid-intelligence sections are proposed mechanisms rather than results. W1's model-reference workflow scores must not be mistaken for observed ground truth. See [SOURCES](../SOURCES.md).

## Decisive fork

Use locked task contracts, task-family-separated evaluation, explicit ablations and independent audit data. Do not accept a collection of component demos as proof of an improved coding system.

## Task contract

A `TaskFixture` MUST define the initial workspace/environment, permitted actions and resources, visible goal, hidden completion checks, expected or acceptable output properties, risk tier, reset procedure, time/cost limits, and failure categorization. Identifiers and seeds are stable. An agent cannot edit the contract or hidden checks.

Fixtures include valid programs, partially parsed programs, unresolved symbols, generated files, stale observations, missing credentials, malicious source comments, denied operations, ambiguous goals, timeouts, cancellation, crashes, build-script side effects and held-out repository families. Start with small Axon examples; broader repo/compiler tasks follow only after isolation is demonstrated.

## Baselines and fairness

Maintain five configurations: rules-only where a rule is applicable; one strong-model coding loop; structured AIR with that same model and no Reflex; AIR with Reflex but no learned world model; and the complete challenger. Report task applicability rather than counting inapplicable rule tasks as victories.

Hold tools, observation availability, task budgets, permitted retries, model versions, language primer and completion checks constant when comparing a component. Separately report whether observation compression or candidate pruning—not model choice—caused a gain. Include warm and cold cache runs, concurrency/load, hardware and inference placement. Charge unused speculative heads, redaction, observation construction, training amortization, failures and verification.

## Metrics and statistical contract

Primary quality is independently verified task success. Secondary measures: success per actual cost; end-to-end p50/p95 latency; intervention/abstention rate; valid-action fraction; harmful/unauthorized attempts and realized effects; recovery correctness; candidate recall; calibration and prediction quality; and per-family regressions.

Before comparing, freeze `EvaluationPolicy`: primary metric, non-inferiority margin, statistical method, confidence level, minimum precision/sample rule, subgroup floors, exclusion criteria, cost target, holdout access policy and maximum submissions. Parameters not filled for the target release leave promotion blocked. Illustrative numbers in a research notebook are not release policy.

Prefer paired tasks and cluster uncertainty estimates by task family/repository where observations share structure. Report intervals and inconclusive results. Do not promote because a difference was “not significant.” Rare safety failures require a distinct risk argument; zero observed failures is not a proof of absence. As a mathematical illustration only, zero events in n independent Bernoulli trials has one-sided 95% upper rate bound `1 - 0.05**(1/n)`. The independence assumption often fails for related coding tasks.

Repeated evaluation consumes information. Separate training, calibration, development, promotion and sealed audit sets by provenance/family/time as applicable. Record candidate submissions and rotate compromised sets. A model's self-assessment or teacher output is a weak label, not completion truth.

## Gate result format

Every result contains gate ID/version, candidate and incumbent digests, task/split manifests, environment, invocation, outputs, sample counts, metrics/intervals, result status and exclusions. Valid statuses: PASS, FAIL, INCONCLUSIVE, SKIPPED, UNAVAILABLE. `PASS` cannot be inferred from a missing output artifact.

## Acceptance gates

**G01-reset:** reset a fixture twice and compare canonical inputs; vary an undeclared environment input and require an invalid-fixture diagnosis.

**G01-fairness:** automatically compare manifests and refuse a head-to-head comparison with changed hidden budget, tool access, primer or completion contract.

**G01-leakage:** duplicate/mutated family-related fixtures across protected splits are detected by provenance plus reviewed similarity rules; final-audit paths are inaccessible to workers.

**G01-evidence:** a tiny inconclusive result cannot pass non-inferiority; a fully abstaining router cannot pass required coverage; a stale candidate digest cannot reuse a passing report.

**G01-ablation:** run the first vertical-slice task set with the strong-model and AIR controls before adding learned components. Report all attempts, including failed and canceled tasks.

## Build slices and exclusions

Build fixtures/reset and evidence schemas first; then the comparison runner; then the protected promotion evaluator. A baseline need not solve every task. No benchmark score by itself establishes fluid intelligence, model calibration on another domain, or kernel safety.

## v0.3 imported-evidence comparability

Before an external benchmark result is used as a Cortex baseline, record whether systems received the same evidence, candidate set, output obligation, retries, tools/permissions, cache conditions and completion verifier. Differences do not invalidate the source as an implementation lead, but they prevent a direct quality/speed comparison unless the experiment is reproduced under matched conditions. Failures remain in denominators. Cached and live inference are reported separately.

## v0.3 benchmark-comparability gate formalized in the spec

**G01-comparability:** an imported benchmark with different candidates/evidence/output obligations may motivate an experiment but cannot be cited as a direct Cortex performance comparison without matched reproduction.
