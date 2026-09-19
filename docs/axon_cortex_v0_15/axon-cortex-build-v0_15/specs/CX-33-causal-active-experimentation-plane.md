---
id: CX-33
title: "Causal and Active Experimentation Plane: intervention, information gain and mechanism discovery"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-07", "CX-08", "CX-09", "CX-10", "CX-20", "CX-21", "CX-29", "CX-32"]
first_stage: M4
implementation_evidence: []
---

# CX-33 — Causal and Active Experimentation Plane

## 1. Purpose

Move Cortex from correlation-only optimization toward controlled causal learning. The plane chooses interventions that reduce uncertainty, compares matched alternatives, records causal assumptions and learns mechanism hypotheses without confusing association with causation.

The control loop is:

```text
uncertainty / failure
→ candidate hypotheses
→ candidate interventions
→ expected information gain + cost + risk
→ controlled experiment
→ observed outcome
→ causal update
→ representation/policy/model change proposal
```

## 2. Experiment objects

Every experiment declares:

- target question/hypothesis;
- treatment variable(s);
- controlled/fixed variables;
- assignment strategy;
- expected outcome metrics;
- project/authority scope;
- intervention side-effect class and reversibility;
- contamination constraints;
- stopping rule and budget;
- causal assumptions and known confounders.

An experiment result records realized treatment, actual environment fingerprint, deviations from protocol and whether inference is causal, quasi-experimental or merely observational.

## 3. Matched cognitive experiments

For cognitive components, preferred experiments hold constant the task/world state/candidate set/effective input while changing one dimension where practical:

```text
same episode
same state digest
same candidate catalog/order
same verifier
swap model OR question definition OR context policy OR composition policy
```

If exact matching is impossible, the comparison is labeled accordingly and may not be reported as a direct causal effect.

## 4. Active information gathering

Cortex may select an observation/experiment because it is expected to reduce uncertainty, not merely advance the current task. Candidate experiments are ranked by a decision-theoretic objective that includes:

```text
expected information gain
× relevance to protected decision
× transfer value
− compute/latency cost
− side-effect/risk cost
```

Examples include running a targeted test, inspecting one symbol, reproducing an error under one configuration, deleting one evidence sentence in a contrastive example, or trying one candidate action in a resettable fixture.

## 5. Counterfactual discipline

Simulation/world-model counterfactuals are hypotheses unless reproduced or otherwise validated. Historical off-policy evaluation must retain behavior-policy lineage and support assumptions. Resettable environments should execute alternatives directly when feasible.

## 6. Mechanism discovery

The plane can propose causal abstractions such as:

- a context artifact changes success only for one task family;
- a candidate-order effect causes instability;
- a repository pattern fails because of an architecture boundary;
- a supervisor intervention improves recovery only after repeated identical failures;
- a specialized model fails when schema cardinality shifts.

Mechanism proposals become typed hypotheses in CX-09/CX-32 and require further tests before native crystallization.

## 7. Reversibility and risk

Interventions are classified before execution:

```text
READ_ONLY
REVERSIBLE
COMPENSATABLE
IRREVERSIBLE
UNKNOWN
```

Experiment automation is most permissive for resettable/read-only work. Irreversible or unknown interventions require stronger authority and cannot be justified solely by high expected information gain.

## 8. Integration

- CX-08 owns planning/hypothesis generation; CX-33 adds causal protocol and active experiment selection.
- CX-09 consumes mechanism/representation discoveries.
- CX-20/CX-21 use matched experiments for model/whole-system claims.
- CX-29 self-application uses this plane to determine whether a challenger caused an improvement.
- CX-30/CX-31 use it to distinguish project-local association from transferable mechanism.
- CX-32 stores experiment and causal-evidence lineage.

## 9. Acceptance gates

**G33-preregister:** protected causal experiments freeze hypothesis, treatment, controls, metrics, assignment/stopping rules and analysis plan before protected outcomes are inspected.

**G33-single-delta:** direct component-effect claims require matched runs differing only in the declared treatment or explicitly record uncontrolled differences/confounders.

**G33-causal-label:** observational, counterfactual, quasi-experimental and randomized/resettable evidence remain distinct; reports cannot imply causal identification unsupported by the design.

**G33-info-gain:** active experiment selection records expected information gain, cost, risk and alternatives considered; a model's bare preference is not sufficient experiment authority.

**G33-reversibility:** intervention reversibility class is known before execution and determines required authority/rollback; unknown/irreversible experiments cannot auto-run through low-risk paths.

**G33-counterfactual:** simulated/off-policy alternatives retain model/behavior-policy assumptions and cannot count as realized outcomes without reproduction or a declared validated estimator.

**G33-contamination:** experimentation preserves train/dev/calibration/locked-test/transfer roles and cannot adapt to protected labels through repeated probing.

**G33-mechanism-transfer:** a claimed mechanism promoted across project families is challenged on held-out families and negative cases before becoming a shared/native capability.

## 10. First deliverable

Run a three-arm experiment on one low-risk cognitive policy (for example working-set selection or model routing): incumbent, challenger A and challenger B over identical replay episodes, then shadow the strongest candidate on live no-effect traffic. Report causal limits, expected information gain, cost and any interaction with task family.
