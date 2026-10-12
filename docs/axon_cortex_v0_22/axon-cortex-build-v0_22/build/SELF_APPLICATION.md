# Reflexive self-application build guide

This guide operationalizes [CX-29](../specs/CX-29-reflexive-self-application-plane.md). The principle is simple: Axon and MiCode must use the same observation, replay, challenger, shadow, admission and crystallization machinery on their own cognitive components that they use on application workloads.

## Start now: observation before mutation

The first milestone is not autonomous code rewriting. It is complete instrumentation of internal cognitive operations. Every model route, context/working-set selection, rule or skill selection, semantic retrieval decision, composition decision, supervisor assessment and completion judgment should expose a versioned record linked to downstream evidence.

```text
internal cognitive operation
→ CognitiveOperationRecord
→ episode/outcome join
→ replay corpus
→ challenger experiment
→ shadow
→ protected admission
```

If a component cannot be replayed or compared, it is not yet eligible for self-improvement.

## Protected kernel

Keep these outside the ordinary self-application envelope:

```text
capability/effect enforcement
interpreter reference semantics
artifact/hash validation
locked evaluation ownership
admission policy
verifier completion authority
provenance/audit durability
rollback
corpus-role separation
```

Candidates may improve the cognitive plane but cannot modify the ruler used to measure themselves.

## Initial self-improvement targets

Begin with high-volume, low-blast-radius choices:

1. model routing;
2. working-set/context selection;
3. rule/map/skill selection;
4. semantic query/retrieval thresholds;
5. semantic definition/question wording;
6. context GC / result-retention policy.

Then expand to supervisor intervention, decision composition, specialized Reflexes, neural skills and planning only after the first lifecycle is proven.

## Self-hosting sequence

MiCode should dogfood each capability first because `/build-loop` produces abundant verified software episodes. Axon then adopts the same component interface internally.

A useful first loop is:

```text
WorkingSetPolicy v1 active
→ collect build-loop outcomes
→ identify excess-context family
→ ImprovementIntent
→ candidate policy v2
→ frozen replay
→ live shadow
→ protected benchmark
→ canary
→ promote/rollback
```

Repeat for model routing and semantic rule/skill selection.

## Challenger classes

A challenger does not need to be another model. Candidate representations include:

```text
revised deterministic policy
revised question/criteria definition
alternative candidate-construction policy
general Reflex backend
specialized classifier
neural program
composition policy
new routing cascade
```

This keeps self-improvement focused on architectural simplification rather than only weight training.

## Experiment requirements

Every comparison binds:

- incumbent and candidate revisions;
- state/effective-input receipts;
- candidate catalog where applicable;
- authority context;
- dataset/corpus role;
- cost and latency;
- downstream verifier/outcome evidence;
- fallback/applicability envelope.

Use CX-20 for mechanism/model experiments and CX-21 for whole-system coding capability. Promotion still occurs through CX-11.

## Automatic promotion policy

Do not enable automatic promotion in the first implementation. After the pipeline is proven, an allowlisted low-risk policy class may receive bounded automatic canary promotion only if CX-11/CX-29 gates explicitly permit it. Automatic promotion never applies to authority enforcement, protected verifier behavior, locked evaluation or the admission kernel itself.

## New primitive discovery

When repeated successful challengers reveal a strategy that does not fit existing AIR primitives, emit a `PrimitiveProposal` instead of silently adding a special case. The proposal must define semantics, lowering/interpreter behavior, authority, replay encoding, cross-family evidence and rollback.

## Definition of success

The self-application plane is real when Axon can demonstrate a complete internal lifecycle:

```text
observe itself
→ identify a measurable inefficiency
→ propose a challenger
→ replay it
→ shadow it
→ independently admit it
→ activate it
→ detect regression or validate benefit
→ roll it back or retain it
```

without the candidate being able to weaken the gates that judge it.

## v0.14 project scopes

Self-application now has three nested but separately governed scopes:

```text
project-local optimization (CX-30)
→ cross-project transfer/pattern learning (CX-31)
→ platform self-optimization (CX-29)
```

Evidence does not automatically move upward. A project-local win must earn transfer evidence before becoming shared capability; a shared capability must still pass each receiving project's contract; and a shared repository pattern does not modify protected Cortex kernel semantics without the existing platform-level admission path.
