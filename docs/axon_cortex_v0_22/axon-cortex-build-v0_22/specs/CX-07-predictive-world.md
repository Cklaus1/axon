---
id: CX-07
title: "Predictive software world models"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-02", "CX-10"]
first_stage: M3
implementation_evidence: []
---

# CX-07 — Predictive software world models

## Intent and source basis

Predict defined consequences of candidate software actions and improve real planning decisions. S1 pp.23 and 83–84 supplies narrow numerical World/MDL prior art, not a general learned transition model. S2 pp.33–43 proposes prediction error and new representations. W4 studies model-bias concerns in model-based learning. See [SOURCES](../SOURCES.md).

## Decisive fork

Start with small, observable, short-horizon predictions grounded in actual executions. Do not start with a neural latent simulator of the entire OS, and do not treat successful simulation as evidence that a real task is complete.

## Layered world contract

Keep five layers distinct: raw evidence; structured observation; inferred belief/hidden-state hypotheses; a predictive model; and a planner using those predictions. An observer can be useful without any learned model. A predictive model is explicitly partial and is not assumed to expose a sufficient/Markov state.

`WorldModelManifest` records version/artifact, outcome schema, required observation features, permitted action classes, training lineage, calibration/evaluation reports, supported domains/horizons, uncertainty mechanism, resource limits and revocation conditions.

`PredictionRequest` names observation/snapshot, the fully specified candidate action, action payload digest, environment/toolchain, horizon and requested outcome fields. For edits, predict after a concrete patch exists. A prediction about “edit symbol X somehow” must be labeled a policy-level forecast over unspecified edits, not a prediction for the final patch.

`PredictionResult` contains distributions or intervals over the requested outcomes, model manifest, applicability, support/evidence references, omitted targets, and abstention where necessary. Use a separate `PredictionError` record only after matched real outcomes arrive; censored outcomes stay unknown.

## First prediction targets

Start with affected file/test neighborhoods; probability of selected check pass; likely diagnostic family; check runtime interval; and chance an action leaves its workspace scope. Some targets are deterministic analyses and should remain Rules. Compare a learned model against dependency heuristics, empirical base rates and the no-predictor agent.

A security-violation prediction is advisory. The executor must prevent prohibited behavior independently of any predicted probability. Predictions are not substitutes for tests, compiler checking or proof obligations.

## Training and fit

Use eligible `(snapshot, observation, concrete action, outcome, environment)` episodes with explicit label quality from CX-10. Split by repository/task family/time where relevant. Preserve rare failures and report distribution shift. Distinguish data selected by the incumbent policy from representative coverage; unchosen actions have no observed outcome by default.

Fit tolerances depend on the domain. Deterministic toy tasks may require exact fit; noisy timing and external outcomes require declared noise models or intervals. Keep quality constraints and model-size/complexity preference separate. AST MDL is a labeled proxy; a genuine two-part description objective must count model and residual/exception costs with a specified code scheme.

## Planning limits

Bound simulated horizon, candidate count and compute. Stop or shorten rollouts when model applicability/support becomes poor. Root simulations in real recent observations and reobserve after effects. Model-generated synthetic labels are never mixed with real outcomes without provenance.

Optimize for downstream benefit: fewer check runs at the same completion quality, better experiment choice, or lower task cost. Forecast scores alone cannot justify removing real verification. A model that improves calibration but worsens planner outcomes is not automatically promoted.

## Acceptance gates

**G07-target:** every prediction binds to its exact patch/action and environment. Mutating the patch or toolchain invalidates the forecast.

**G07-holdout:** beat or match preregistered simple prediction baselines on protected outcomes; report calibration, interval coverage, error and per-domain failure.

**G07-planning:** compare the same planner with no model, simple model and candidate model on held-out tasks; charge prediction cost.

**G07-exploitation:** adversarial candidate search attempts to find actions the simulator likes but real checks reject. Report gaps; no such simulated success may certify completion.

**G07-unknown:** missing features, unsupported action/horizon and canceled experiments return explicit inapplicability or censored labels, not invented confident forecasts.

## Build slices and exclusions

Implement prediction/outcome matching and base-rate models first; then learned diagnostic/test forecasts; then short-horizon planning. General causal discovery, abstract latent worlds, physics and kernel models are later research, not entry requirements.

## v0.3 counterfactual label discipline

Predictive training/evaluation distinguishes realized outcomes from unobserved alternatives. An executed action supplies an observed target only for that action under the recorded state/environment. Alternative actions remain unknown unless replay/reset experiments, randomized exploration or a declared off-policy estimator provides evidence. Selection probabilities refer to behavior policy, not model confidence.
