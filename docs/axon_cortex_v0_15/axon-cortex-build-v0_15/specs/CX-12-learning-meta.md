---
id: CX-12
title: "Per-pillar learning, curricula and meta-governance"
status: Draft
authority: Proposed
depends_on: ["CX-08", "CX-09", "CX-11"]
first_stage: M7
implementation_evidence: []
---

# CX-12 — Per-pillar learning, curricula and meta-governance

## Intent and source basis

Realize the source's per-pillar learning idea without allowing learners to rewrite their own acceptance criteria. S2 pp.44–48 provides the lifecycle and attribution ambition; S1 pp.87–91 supplies evidence/governance and invariant-change discipline. See [SOURCES](../SOURCES.md).

## Decisive fork

Unify lifecycle interfaces but separate execution, learning and admission authority. “Every pillar can improve” does not mean “every pillar may update itself online” or “the safety boundary is optimized as a task score.”

## Common lifecycle

A component adapter exposes `describe`, `infer_or_execute`, `record_outcome`, `propose_update`, `evaluate`, `request_promotion`, `activate_approved`, and `deoptimize`. Functions may be inapplicable: a parser need not predict a distribution to fit the interface. Activation requires an independent approved artifact receipt.

Every component declares inputs, outputs, trainable parameters, protected code, eligible data, observed failures, quality/cost metrics, update frequency/budget, baseline, holdouts and rollback. Learned changes are pinned during a task epoch. Correlated multi-component updates are staged and explicitly evaluated.

## Pillar register

| Pillar | Permitted learned proposal | Must remain independent |
|---|---|---|
| Observer/retrieval/memory | Projection, ranking, retention recommendation, typed summary | Access control, source provenance, secret export policy |
| Reflex/router | Adapter, calibrator, routing policy | Action authority, mandatory evidence, approved risk envelope |
| Generator/reasoner | Prompt, tool selection, code proposal, strategy | Executable checks and protected task contract |
| World model/planner | Features, predictor, rollout policy, experiment ranking | Real outcome labels and capability limits |
| Critic/evaluator assistant | Heuristic scoring and failure triage | Release evaluator, hidden audit set, promotion thresholds |
| Representation/abstraction | New encodings/concepts | Traceability to evidence and unchanged evaluation policy |
| Capability proposer | Useful bounded macros or candidate pruning | Grant issuance/attenuation and executor checks |
| Proof search/compiler optimizer | Tactics, lemmas, restricted rewrite candidates | Proof kernel, reference semantics, TCB modification process |
| Meta-controller | Experiment allocation, curriculum, proposed policy revisions | Approved policy activation and final audit authority |

## Loop hierarchy

Task execution manages actions; task planning manages subgoals; component learning manages one candidate; portfolio learning chooses experiments/components; architecture governance decides interface/invariant changes. These loops share event lineage but have different budgets and write permissions. Detailed builder and runtime protocols are in [LOOPS](../build/LOOPS.md).

A meta-controller uses evidence to choose the next improvement experiment: estimated task bottleneck, expected information value, uncertainty and actual resource spend. It can propose stopping a research track whose marginal value is poor. No mandatory infinite improvement loop exists.

## Curriculum contract

Generated tasks carry generator/version, family/mechanism lineage, difficulty evidence, solvability checks where available, training/audit designation and contamination analysis. Curricula target observed gaps and include adversarial/rare cases, but cannot replace protected externally controlled audit tasks.

Measure transfer beyond generator templates. Evaluation changes are versioned policy proposals; rerun the incumbent under the old and new suites and document why the new suite better measures the intended outcome. Easier tasks cannot silently redefine “improvement.”

## Stop conditions and stability

Each learning/meta job has wall time, token/compute/cost, candidate-count and evidence-access budgets. Stop on insufficient data, unstable labels, budget exhaustion, absent authority, repeated inconclusive trials or increased regression risk. Report the failure and best supported hypothesis without manufacturing a completed milestone.

No component retrains on its own unverified predictions as though they were real outcomes. Synthetic training remains labeled, and high-feedback loops need explicit contamination and stability evaluation.

## Acceptance gates

**G12-contract:** every participating pillar has a complete lifecycle/metric/authority manifest; unsupported lifecycle operations are explicit.

**G12-cause:** controlled component substitution attributes a known fault without falsely updating unrelated pillars; Unknown remains valid when ambiguous.

**G12-meta:** attempts to lower admission thresholds, edit hidden data or self-sign a model through a meta job are denied.

**G12-curriculum:** generator siblings cannot leak into final audit unnoticed; transfers are tested against templates and compute-matched controls.

**G12-stability:** simultaneous conflicting updates are serialized or evaluated together; an episode never silently mixes component versions.

## Build slices and exclusions

After two individually successful learning pipelines, add the common registry and experiment-allocation controller. Architecture-level proposals remain human/governance reviewed. Broad self-improvement claims require longitudinal evidence; this spec delivers mechanisms and tests, not an intelligence verdict.

## v0.4 three-source learning portfolio

Portfolio/meta learning allocates experiments across **self experience**, **external experience** (MiCode/repositories/research) and **generated experience** (curricula/simulations). It may learn which source is useful for which pillar, but cannot lower evidence/admission standards for one source to make progress appear faster. Source mix, contamination risk and transfer are reported explicitly.


## v0.13 recursive learning boundary
Meta-learning may optimize cognitive components and propose new primitives through CX-29, but recursive experiments remain subject to independent evidence and cannot capture the evaluator/admission roots.
