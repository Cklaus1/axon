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

## v0.16 review amendment — Event-specific reliability and selective deployment

A correctness estimator names the predicted event, target label semantics, domain, model/runtime/input policy, estimator revision and independent fitting/evaluation lineage. Reference-panel agreement, actual action success, factual accuracy and permission are different events. Calibration is a transformation layered on a score source, not a replacement origin. Calibrator training is separated from model training and final test evaluation or uses declared cross-fitting. The policy evaluates the full specialist-plus-fallback cascade, including the costs of rejected cases.

**G06-reliability-event:** a calibrator for panel-mode agreement, changed option count/input policy or another model cannot be relabeled as task-success confidence; absent/expired evidence blocks reliability-dependent routing and held-out thresholds cannot be tuned on protected tests.

See `build/REVIEW_INVARIANTS.md` for the shared interpretation and `review/BUILD_ADVERSARIAL_REVIEW.md` for attack cases.

## v0.18 ACE amendment — ACE raw provenance and event reliability

P04 separates origin, physical score computation, statistics, empirical calibration, correctness event, applicability and OOD. Preserve original producer tags and explicit aliases. Definition/order/model/deployment changes invalidate uncovered calibration. Missing uncertainty stays missing. Thresholds do not confer authority.

See the normative [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md) and [build guide](../build/ACE_INTEGRATION.md). These requirements extend this owner; no second ACE subsystem is introduced.

**G06-ace-provenance:** Original score vocabulary/tag and canonical mapping remain visible; probability origin cannot be laundered through calibration, generic confidence or an unknown alias; wrong-event/out-of-domain estimates are unusable.

## v0.19 amendment — calibration registry and risk policy

A `CalibrationArtifact` is valid only for a frozen **calibration key**. At minimum the key binds producer/model/checkpoint, decision family/question definition, score provenance, candidate construction policy, candidate order policy, effective-input/working-set projection, task/domain/population, preprocessing/precision class and calibrator/version. A mismatch yields `Unavailable` correctness unless a separately validated transfer rule covers the change.

Evaluation MUST report proper scores (Brier for the applicable event encoding and NLL/log loss), support/sample counts and reliability views; add resolution/refinement, classwise reliability and selective risk/coverage where meaningful. ECE MAY be reported as a diagnostic but MUST NOT be the sole release criterion. Reliability bins carry uncertainty/support so small bins do not masquerade as precise calibration.

Routing policy owns consequence-aware thresholds. High-confidence output can still require verification or abstention under high effect/risk. Low confidence may gather more context, invoke an independent decision, use a generative/world-model path, or ask a human, subject to existing authority and budgets.

Production outcome joins may flag calibration drift and propose a new artifact; they do not mutate the active calibrator in-place during a pinned episode and cannot bypass CX-11 admission.

**G06-calibration-registry:** A calibrated correctness claim resolves to an immutable artifact whose key matches producer/model, score origin, question/candidate policies, effective input and domain; stale or wrong-domain artifacts yield unavailable correctness rather than silent reuse.

**G06-proper-score:** Held-out calibration evaluation records Brier score, NLL, support and reliability evidence with uncertainty, plus applicable classwise/selective-risk views; ECE alone cannot satisfy the gate.

**G06-risk-policy:** Consequence thresholds are external policy inputs: high probability cannot mint authority, certify completion or waive a required verifier, and low-confidence fallback remains semantically and privacy constrained.

**G06-drift:** Joined production outcomes can detect/report calibration drift and create a shadow recalibration candidate, but the active artifact changes only through versioned evaluation/admission and never mid-episode by self-promotion.

## v0.20 amendment — schema decisions and lightweight Reflex specialization

Implement the [lightweight profile](../schemas/LIGHTWEIGHT_REFLEX_PROFILE.md) as an independently evaluated calibration/acceptance procedure: training, selection, calibration, acceptance and frozen protected test have distinct eligible grouped roles. The finite cutoff search uses a declared exact-binomial bound and corrected comparison budget, explicit sparse-class vetoes and an advisory null cutoff when evidence is insufficient. The event must identify teacher/reference agreement versus argument correctness, complete-invocation validity or verified task outcome. Minimum/product of marginals is not joint correctness. Local consequence policy and permissions remain independent.

**G06-independent-threshold:** A preregistered frozen-model finite cutoff search uses independent grouped acceptance evidence, exact lower bounds and corrected comparison/subgroup/submission budgets; final protected-test results do not select the cutoff.

**G06-no-recommendation:** Insufficient independent support, critical-class failure or an unmet lower-bound target produces a null advisory threshold and preserves fallback, even with maximum predicted probability one.

**G06-full-invocation-event:** Reference-label agreement, argument correctness, whole-invocation validity and verified task outcome retain distinct event identities; heuristic marginal aggregation never acquires a joint calibrated correctness claim.

## v0.21 research integration amendment

This additive amendment is normative for the explicitly selected v0.21 profile. Existing authority and wire-format owners remain unchanged. The [research integration contract](../build/RESEARCH_INTEGRATION_V021.md) and [requirements matrix](../integration/RESEARCH_REQUIREMENTS.json) specify the complete profile and source-backed migration. Older versioned profiles remain separately identifiable; none of these declarations establishes runtime implementation.

### Acceptance gates added in v0.21

**G06-r21-permutation-alignment:** Every sampled permutation contains exactly the same candidate IDs, each returned distribution covers those IDs and sums to one, and averaging is label-aligned; missing, duplicate or foreign labels refuse.

**G06-r21-permutation-scope:** Finite permutation sampling is not certified exact invariance or calibration; wrapper identity, seed/schedule and all call costs are recorded, ordinal level meanings are preserved and thresholds bind to the wrapper.

**G06-r21-calibration-binding:** Calibrated correctness and risk thresholds bind to the complete scorer, wrapper, candidate policy, projection and independent calibration partition; a stale or absent binding cannot drive automatic acceptance.

**G06-r21-candidate-stress:** Single-candidate, all-invalid, duplicate/near-duplicate, candidate-addition and out-of-domain tests report selective risk and coverage; candidate-relative softmax is never interpreted as absolute correctness.

**G06-r21-router-bakeoff:** Each routing arm pins its encoder/head/policy and model-role pool and is compared at equal whole-task budgets against applicable deterministic and best-single controls; unsupported domains do not inherit a published win.

**G06-r21-router-recursion:** Route selection cannot recursively invoke an unbounded decision cascade or register/authorize a new model; maximum switches, selector depth, deadlines and safe fallback are explicit.
