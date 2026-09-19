# Axon self-optimizing OS loop

## Headline objective

Axon Cortex is the cognitive control plane for a self-optimizing language/compiler/runtime/OS. The system learns from three evidence sources:

1. **self experience** — Axon's own executions, predictions, failures and experiments;
2. **external experience** — MiCode coding episodes, repositories, histories, benchmarks and approved research artifacts;
3. **generated experience** — curricula, synthetic bugs, adversarial tasks, simulations and controlled counterfactuals.

All three enter one evidence → abstraction → validation → admission → crystallization pipeline. They do not receive separate truth standards.

## Knowledge ladder

```text
Episode
  → Memory / Evidence
  → Pattern
  → Concept / Representation
  → Reflex / Retrieval Policy
  → Skill / Tool
  → Library Abstraction
  → Compiler Transformation
  → Runtime Primitive
```

Progress down the ladder means more reuse and typically a larger blast radius, so required assurance increases. Many useful concepts should never become native primitives.

## Coupled optimization loops

- better Observer → better state → better decisions and training labels;
- better Reflex → cheaper high-frequency decisions → more experiments;
- better World Model → better experiment selection → better causal/abstraction evidence;
- better Verifier → safer aggressive exploration → stronger promotion evidence;
- better Compiler/Runtime → cheaper experiments → larger searchable improvement space;
- better repository knowledge extraction → better candidate abstractions → better OS/library/compiler choices.

## Timescales

| Timescale | Allowed adaptation |
|---|---|
| milliseconds–seconds | working state, cached observations, Reflex/routing decisions |
| seconds–minutes | subgoals, hypotheses, representation/experiment choice |
| hours | skill/tool candidates, calibration/retrieval updates in isolated evaluation |
| days | world-model/representation/knowledge updates through admission |
| weeks+ | model training, compiler/library/runtime promotion, governance changes |

Trusted admission/proof/capability enforcement never mutates online because a learner wants an easier pass.

## System-level scorecard

Track verified coding frontier, success per cost, time-to-correct-patch, frontier-model usage, tool/build/test calls, human intervention, calibration/coverage, prediction error, regression/rollback rate, compiler build time, generated-program performance, memory/binary size, proof burden and capability footprint. Do not collapse these into one unreviewed scalar objective.

## First closed-loop demonstration

A MiCode-exported or native Cortex coding episode reveals a recurring bounded workflow. Cortex turns it into a guarded skill/tool candidate, independently evaluates it on held-out fixtures, promotes it through CX-11, uses it in later tasks, detects an injected applicability mismatch and deoptimizes to the previous path. This proves the learning/crystallization plumbing without requiring compiler self-modification.

## Intent as the control envelope (v0.5)

Self-optimization does not mean free mutation. Every consequential change begins as an approved typed intent. Self experience, external repositories and generated curricula may produce **ImprovementIntent candidates**; those candidates specify objective, scope, constraints, requested authority and required evidence, then flow through normal AIR/executor/verifier/admission machinery. This makes "why is the OS changing itself?" a query over explicit intent/evidence lineage rather than an inference from a patch after the fact.

Canonical loop:

`measurement/knowledge → ImprovementIntent → Intent IR → AIR candidate search → artifact → independent evidence → CX-11/CX-18 admission → active/deoptimized artifact`.

A learner may improve how it proposes future intents, but it may not mutate the active intent, admission policy or verifier to make the current candidate pass.

## v0.9 compiled cognition

The crystallization ladder now has an explicit middle tier:

```text
THINK / GENERATE
      ↓
GENERAL REFLEX
      ↓
SPECIALIZED COGNITION
  ├─ Specialized Reflex
  ├─ Neural Program
  └─ Tool / Procedure
      ↓
RULE / LIBRARY / COMPILER / RUNTIME
```

The OS continuously asks whether recurring cognition can move downward **without losing verified utility or widening authority**. Movement is reversible at the artifact-routing level: drift or applicability mismatch returns future work to the previous-good/general executor. Real-world effects already taken are never undone merely by changing the chosen cognitive representation.

## v0.12 control-plane learning

Self-optimization may simplify not only task decisions but also supervision and memory management. Repeated reliable supervisor/context behavior can crystallize into deterministic guard rules, reusable working-set policies, semantic query plans or cascade policies. The optimization objective includes protected-clause retention, false-intervention cost, recompute correctness and cache/context economics—not merely model accuracy.


## Reflexive self-application (v0.13)

Self-optimization begins with internal observability now, not with future autonomous mutation. Every non-kernel cognitive component should emit CX-29 operation records and become eligible for replay/shadow challengers. The OS optimizes not only application cognition but its own routing, working-set, retrieval, composition, supervision and specialization policies while the protected admission/authority kernel remains outside the candidate path.
