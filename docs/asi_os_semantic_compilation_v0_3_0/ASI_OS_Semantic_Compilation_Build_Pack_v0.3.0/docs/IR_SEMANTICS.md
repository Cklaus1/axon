# Semantic Plan v0.3 Wire and Execution Semantics

The portable plan schema is a **boundary/reference projection**, not a second canonical AIR implementation. In the Cortex v0.23 profile, semantic execution meaning belongs to CX-04 AIR.

## Two axes: semantic operation and mechanism
Every non-return node declares a `semantic_op` describing **what the computation means**:

`observe`, `rule`, `retrieve`, `decide`, `generate`, `reason`, `predict`, `check`, `act`.

A capability-backed node also declares a `mechanism` describing **how it is currently implemented**:

`transform`, `model.call`, `tool.call`, `code.execute`, `evaluate`, `human.request`.

`assert` and `return` are structural fixture-harness mechanisms. A capability declares `semantic_ops[]`; the linter rejects a plan that assigns a semantic role the capability does not support.

This prevents semantic laundering and permits optimization such as `generate: model.call → generate: deterministic/tool/native implementation` only when evidence supports equivalence for the task contract.

## Graph and bindings
The reference graph remains acyclic. Bindings are exactly one of input, node/port, or literal. Strings are data; no expression evaluator, shell interpolation, dynamic import, hidden recursion, or implicit provider fallback exists in this contract.

All node dependencies must exist. A node may read another node only when that producer is in its transitive dependency closure. Every node must contribute to the terminal return. Parallel execution is permitted only where dependencies/effects/resources allow it.

## Types
Wire types remain `string`, `integer`, `number`, `boolean`, `object`, `array`, `artifact`. Integer-to-number is the only implicit widening. Artifact descriptors are immutable scoped references, not arbitrary host paths.

## Capability projection
The registry is descriptive and version-pinned. It provides mechanism, allowed semantic operations, typed inputs/outputs, declared effects, replay class, implementation identity, and idempotency support. In Cortex mode it is a projection of canonical capability/resource owners and is never an authority grant.

## Evaluation binding
Each evaluation criterion fills all but exactly one registered evaluator input; the criterion target fills the remaining input. Missing/stale/malformed evaluation blocks acceptance. The evaluator cannot widen authority or edit the protected contract.

## Cortex and Forge lowering
IntentIR→AIR and AIR/source→Forge SIR are explicit lowering boundaries. They retain source/target identity and evidence. A Forge SIR artifact never substitutes for IntentIR, and a portable Semantic Plan never silently becomes a second AIR authority.
