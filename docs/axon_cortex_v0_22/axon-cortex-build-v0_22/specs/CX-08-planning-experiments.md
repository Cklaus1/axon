---
id: CX-08
title: "Planning, hypotheses and controlled experiments"
status: Draft
authority: Proposed
depends_on: ["CX-03", "CX-06", "CX-07"]
first_stage: M4
implementation_evidence: []
---

# CX-08 — Planning, hypotheses and controlled experiments

## Intent and source basis

Let Cortex gather discriminating evidence instead of repeatedly generating patches or escalating blindly. S2 pp.34–39 describes hypothesis/experiment loops; S1 p.23 documents value-level Plan/Schedule/Feedback prior art. See [SOURCES](../SOURCES.md).

## Decisive fork

Build bounded task planning over real executor capabilities, with explicit hypothesis uncertainty and resettable experiments. Do not assume that a conditional predictor identifies causality or that calculated information gain is ground truth.

## Plan and hypothesis contracts

`Plan` contains goal/contract ID, current snapshot/belief, subgoals, candidate action graph, expected outcomes, evidence needs, dependencies, budgets, replanning triggers, stop conditions and fallback. Proposal is untrusted. The plan never widens the union of currently granted effects.

`Hypothesis` contains a falsifiable statement, variables/representation, support and contradicting evidence, prior/posterior or qualitative uncertainty, predicted observations, assumptions and applicability. Contradictory hypotheses may coexist. Never normalize made-up probabilities just to satisfy an interface; an unweighted set is valid until a weighting model is justified.

An `Experiment` identifies the intervention, control/reset strategy, concrete capability/action, measured variables, predicted outcomes by hypothesis, information-cost estimate, risk, resource budget, stopping rule and resulting evidence. Experiment descriptions must compile into granted actions; prose does not authorize execution.

## Selection and control

Use a transparent baseline first: choose the cheapest permitted check that distinguishes the most remaining hypotheses according to the declared model. A later policy may maximize estimated expected information gain or value of information subject to resource/risk constraints. Both the estimate and its assumptions are logged.

Do not rank experiments only by entropy reduction: a low-entropy wrong hypothesis can be worse than an uncertain correct set. Evaluate diagnosis accuracy, task completion and information gained per actual cost. A useful experiment may reduce uncertainty without changing the code.

Replan on stale snapshots, failed preconditions, unexpected outcomes, repeated no-progress actions or budget changes. Keep an action-history signature and maximum retry/no-progress budget. A new strategy proposal does not reset the parent budget. Termination modes include VerifiedComplete, BudgetExhausted, NeedsAuthority, InsufficientEvidence, Canceled and Failed.

## Causal evidence levels

Every causal edge/claim carries one of ObservationalAssociation, InterventionSupported, or AssumedMechanism. An observational predictor does not automatically upgrade its edges after a confident answer.

Initial interventions are restricted to resettable copies of software environments. Record exact before/after state, randomized or matched assignment where feasible, controlled variables, repetitions and residual confounders. A concurrency test needs enough repeats and explicit scheduling assumptions; a single disappearing failure does not prove a race mechanism.

Counterfactual queries name the structural assumptions and whether their output came from an exact executable model or a learned approximation. A causal hypothesis can guide experiments without being treated as established fact.

## Acceptance gates

**G08-hypothesis:** two hypotheses with distinct registered predictions cause selection of a separating permitted check; unsupported labels remain uncertain.

**G08-controls:** replay/reset and matched control execution preserve declared controlled variables; a confounded fixture does not produce an unqualified causal conclusion.

**G08-authority:** a high-information experiment requiring denied execution/network access is blocked rather than run.

**G08-progress:** repeated identical unsuccessful actions trigger bounded replan/escalation/stop; no infinite reasoning loop or budget reset occurs.

**G08-value:** compare against fixed check order, random permitted probing and strong-model-only reasoning on the same held-out tasks and budgets.

## Build slices and exclusions

Start with two or three competing debugging hypotheses and approved checks, then extend plan depth and experiment policies. No open-ended internet experimentation, arbitrary environment mutation, or production exploration in v0. General causal discovery remains a research result to demonstrate, not a consequence of adding a causal graph type.

## v0.21 research integration amendment

This additive amendment is normative for the explicitly selected v0.21 profile. Existing authority and wire-format owners remain unchanged. The [research integration contract](../build/RESEARCH_INTEGRATION_V021.md) and [requirements matrix](../integration/RESEARCH_REQUIREMENTS.json) specify the complete profile and source-backed migration. Older versioned profiles remain separately identifiable; none of these declarations establishes runtime implementation.

### Acceptance gates added in v0.21

**G08-r21-evolution-preregister:** Selection policy, noise floor, task partitions, repeated-trial identity, stopping rule and multiplicity control are frozen before outcomes are visible; an evaluator cannot tune its own acceptance threshold on the final holdout.

**G08-r21-evolution-generalization:** Leakage criticism and disjoint generalization tests accompany incumbent comparisons under equal total budgets; inconclusive, rejected and failed treatments retain costs and cannot be relabeled as improvements.

## v0.22 amendment — closed-loop Axon / Compute Fabric / MiCode integration

The [0.22 integration contract](../build/CLOSED_LOOP_INTEGRATION_V022.md) and [source-bound work packages](../build/WORK_PACKAGES_V022.md) extend this existing owner. They do not introduce a parallel authority, learning store, sandbox claim or schema reinterpretation. Original 0.21 research profiles remain available, but the bounded 0.22 pilot uses only its explicitly qualified dependency slice. The [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json) preserves ACF-01 M0-M2 obligations; the [MiCode consumer delta](../integration/MICODE_V022_CONSUMER_DELTA.md) binds actual peer work. All added runtime obligations are unexecuted proposals in this pack.

**G08-r22-logical-branches:** Incumbent and challenger start from the same frozen durable base and declared resource/cache regime with independent run identities; this is logical branching, not a claim of RAM forks.

**G08-r22-branch-cancellation:** Stopping a losing branch preserves its events, usage and pending liabilities, reconciles descendants and leaves surviving branches isolated; cancellation does not erase an unfavorable outcome.

**G08-r22-real-paired-execution:** Real paired MiCode trials use the declared immutable inputs, fixed controls and unique identifiers, with preflight and Fabric receipts on both arms; a prerecorded fixture or a reused episode cannot count as a live execution.
