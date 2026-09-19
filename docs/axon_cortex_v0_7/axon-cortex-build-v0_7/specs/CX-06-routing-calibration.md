---
id: CX-06
title: "Calibration, risk-aware routing and abstention"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-05"]
first_stage: M2
implementation_evidence: []
---

# CX-06 — Calibration, risk-aware routing and abstention

## Intent and source basis

Choose inexpensive cognition only inside an empirically supported reliability region. S2 pp.10–15 proposes confidence-gated escalation; W3 motivates validating probability calibration separately from classification. See [SOURCES](../SOURCES.md).

## Decisive fork

Hard authorization and required evidence precede statistical routing. Within that permitted set, optimize task loss, cost and latency using measured policy performance. Do not adopt a universal probability threshold or assume that every task should avoid deliberate reasoning.

## Calibration contract

A `CalibrationArtifact` binds model weights/version, tokenizer, prompt and question schema, candidate-construction policy, domain/task family, training/calibration/evaluation split manifests, fitting method, metrics with uncertainty, coverage, subgroup exclusions, validity period and revocation triggers. A content hash pins the artifact. Quantization or a model/provider change invalidates calibration unless revalidated.

Measure proper scoring metrics appropriate to the problem, such as multiclass Brier score and negative log likelihood, plus reliability plots, selective risk/coverage, subgroup performance, state/candidate perturbation sensitivity and a preregistered routed-system measure such as Verified Utility at Coverage (VUC). ECE is a diagnostic, not a sole promotion gate; binning and limited samples can obscure errors. A probability of a choice and the probability that a full action succeeds require different labels.

For an ordinal score, specify the event being calibrated: score category, threshold crossing, or observed downstream outcome. For continuous outcomes, define intervals and coverage separately. No estimate is described as an individual guarantee just because aggregate calibration passed.

## Router policy

Input: task/authority profile; observation and omission report; available candidates; raw decision packet; calibration applicability; novelty/disagreement signals; prior progress; budget remaining; and action reversibility.

Allowed routes: deterministic Rule; Reflex; Retrieve/Observe; Generate; Reason; Predict/Experiment; independently required Check; authorized human review; or Blocked. Routing is a graph, not a compulsory linear RULE→REFLEX→THINK chain. A simple deterministic check may settle a question more cheaply than invoking a bigger model. A known difficult task may go directly to Reason.

The policy can minimize estimated expected loss plus measured resource cost subject to authority, task quality and coverage constraints. Cost weights and risk limits are owner-approved versioned inputs. It cannot waive a proof/check or invent missing permission because expected utility looks positive.

Out-of-domain signals include calibration inapplicability, candidate omission, ensemble disagreement, stale observations, prediction failures and repeated no-progress actions. These are imperfect signals. The controller must support abstention even with a high predicted probability, and report undetected shift on evaluation fixtures.

## Operational semantics

Apply hysteresis or a bounded switching policy to avoid repeated small-model/large-model oscillation. Cache only exact pinned decisions or scope-validated reusable facts. All escalations consume the same parent budget; a retry cannot reset the budget. A exhausted reasoning policy stops or asks the authorized operator rather than quietly looping.

Fallback and deoptimization are first-class: a rule or Reflex specialization that leaves its applicability domain returns control to the baseline policy. Log the reason, versions and cost. Model access failure is not evidence of low semantic confidence.

## Acceptance gates

**G06-calibration:** fitting uses only grouped permitted calibration data; raw score origin and calibrated result remain separate; proper scoring/reliability/risk-coverage/VUC on held-out task families are reported separately. Shuffling labels or changing model/schema/candidate policy invalidates the artifact.

**G06-risk:** an unauthorized/irreversible action never becomes allowed solely because the selected answer has probability 1.0.

**G06-coverage:** an all-abstain router fails the coverage target; a high-coverage router with unacceptable selective risk also fails.

**G06-shift:** shift/OOD fixtures cause the specified escalation or inapplicability behavior; report remaining failures instead of claiming perfect detection.

**G06-budget:** repeated escalation, retry and fallback cannot exceed parent limits; the termination reason is observable.

## Build slices and exclusions

Start with a deterministic router that is safe without probabilities. Add empirical calibration and learned routing only after labels and baseline evaluations exist. No RL-based router is required for v0. Safety limits are governed by CX-00/CX-11, not learned online.

## v0.3 selective-routing definitions

For protected evaluation, `Coverage = handled_without_escalation / eligible_decisions`. `SelectiveError = incorrect_handled / handled`; when no decisions are handled it is undefined, not zero. Report task success, latency and cost beside coverage/selective error rather than hiding them inside a single score. Any aggregate VUC-style metric must publish its exact formula, weights and eligibility rules before the experiment.

Provider-reported distributions, entropy measures and generated estimates can inform routing but do not become calibrated correctness probabilities without an applicable empirical artifact. Calibration binds the effective-input/preprocessing contract and is invalidated by material truncation/projection changes.

A backend cannot improve reported quality by dropping failed requests, silently narrowing the eligible set, or escalating difficult examples outside the denominator.
