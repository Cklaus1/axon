---
id: CX-04
title: "AIR graph and interpreter-first integration"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-03"]
first_stage: M1
implementation_evidence: []
---

# CX-04 — AIR graph and interpreter-first integration

## Intent and source basis

Give cognition explicit types, dependencies and budgets without multiplying language keywords. S2 pp.25–26 separates decision, generation and reasoning; S1 pp.2–5 establishes interpreter reference semantics and extension discipline. See [SOURCES](../SOURCES.md).

## Decisive fork

AIR is initially a versioned host-validated execution graph and library interface. It is not immediately an Axon parser extension or an assertion that the current compiler supports cognitive syntax.

## Minimal operation set

| Operation | Semantics |
|---|---|
| Observe | Read a scoped environment observation through its authorized adapter. |
| Rule | Invoke a pinned deterministic function over typed inputs. |
| Retrieve | Fetch scoped evidence/artifacts; results carry origin and freshness. |
| Decide | Request bounded typed probabilities/choices through Reflex or a compatible adapter. |
| Generate | Produce an untrusted artifact such as a patch or text. |
| Reason | Produce a structured plan/hypothesis proposal using deliberate computation. |
| Predict | Produce a distribution over defined outcomes, possibly with abstention. |
| Check | Run a registered test/proof/check and return evidence, not model opinion. |
| Act | Request an executor-authorized effect. |

REFRAME, ABSTRACT, HYPOTHESIZE, EXPERIMENT and DISCOVER start as composed library workflows. Learning runs outside live task graphs, using explicit offline jobs and admission. A node label never makes an external model call pure: inference still has configured AI/network/cost effects.

## Graph contract

An AIR artifact contains schema version, content digest, node IDs/kinds, typed inputs/outputs, data dependencies, control predicates, capability requirements, resource limits, model/provider selectors, timeout/retry policies, and observation/artifact lineage. Acyclic per-iteration graphs are the initial representation. Repetition belongs to a bounded task controller with explicit stop conditions, not hidden graph recursion.

Static validation checks unknown references, cycles, type mismatches, missing branch fields, authorization requirements, effect escalation, unbounded iteration, and invalid speculative regions. Runtime checks bind dynamic targets and payloads to actual grants/snapshots. A field needed after another field's resolution has a real dependency edge; it is not independent merely because a model can answer both.

Rule functions claiming determinism cannot call an effectful provider. Returned uncertainty and unavailable evidence must be represented in the output type. No implicit empty artifact, default success, guessed distribution or hidden provider fallback is permitted.

## Scheduling and execution

Start with a deterministic serial scheduler. Parallelization is an optimization over nodes whose effects and dependencies permit it. Record order of observed completions and cancellation events. Speculative Decide outputs may be computed; Act and untrusted tool effects are never speculative in v0.

Budget reservations happen before dispatch and reconcile actual usage when known. Queue deadlines, timeouts, bounded retries and backpressure propagate to children. Refusal, failure, abstention and cancellation are distinct outcomes. Provider fallback is a new explicit dispatch recorded with the effective model/schema/calibration versions.

## Axon integration

Reuse Axon typed programs for deterministic transformations, metrics and scoped checks. Add a narrow host seam rather than routing arbitrary AIR operations through `exec`. Logical modules may initially live together behind interfaces; physical crate splits should follow measured build/test boundaries.

R44 sessions cannot carry arbitrary live handles across cells. Store stable artifact references in the Cortex controller; reacquire validated handles per call. Do not rebuild a parallel type inference engine. Native lowering is deferred; unsupported cognitive operations refuse explicitly rather than silently run without interpreter safety.

## Acceptance gates

**G04-schema:** unknown nodes, bad types, missing branch outputs, cycles and unbounded loops fail validation with stable symbolic categories.

**G04-effect:** a Rule node attempting an AI or filesystem effect and a child node requesting widened authority both refuse.

**G04-replay:** a pinned simple graph executed through recorded host/model replies reproduces output and action selection without new effects.

**G04-scheduler:** deterministic serial and permitted parallel modes agree on the semantic result; reordered effectful nodes are rejected.

**G04-fallback:** a provider outage produces an explicit failure/authorized fallback event, never an unnoticed model switch or a fabricated answer.

## Build slices and exclusions

Implement schema validation, serial scheduler and a mock Decide adapter before model integrations. Add actual model dispatch after the safe single-step task works. Language sugar, optimizer fusion and native execution each require separate CX-15 evidence.

## v0.3 model-call scheduling rule

AIR may share verified common state/prefix work across questions, but it may not silently fuse separately specified question computations into a joint generative prompt and call the result semantics-preserving. Such fusion is a backend/policy variant and requires conformance, calibration and protected-task evaluation. Batch dependencies and active-branch result requirements are explicit graph edges.

## v0.12 integration — cognitive cascades and supervisor events

AIR execution may record an explicit cognitive cascade (`RULE → RETRIEVE → SELECT/PROJECT → COMPOSE → REFLEX/SPECIALIZED → GENERATE → THINK/PROVE`) where each escalation has a typed reason. Supervisor observations/interventions are side-channel control events, not worker-authored AIR nodes with extra authority. The interpreter/replay path must preserve cascade and intervention lineage.
