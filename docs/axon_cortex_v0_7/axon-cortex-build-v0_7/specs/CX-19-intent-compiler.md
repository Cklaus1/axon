---
id: CX-19
title: "Intent compiler, typed Intent IR and semantic approval"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-01", "CX-04", "CX-11"]
first_stage: M1
implementation_evidence: []
---

# CX-19 — Intent compiler, typed Intent IR and semantic approval

## Intent and source basis

Restore and strengthen Axon's original product thesis: humans and higher-level system components express **intent**, while Axon resolves that intent into a typed, reviewable contract before Cortex/AIR chooses an implementation. The original roadmap already described structured prose → typed `.ax` plus explicit AST review/approval; Cortex needs a distinct Intent IR so ambiguous natural language does not compile directly into executable code or unrestricted self-modification.

## Decisive fork

Natural language does **not** lower directly to `.ax` or to executable AIR. It first lowers into a versioned typed `IntentIR` that separates objective, constraints, preferences, authority requests, budgets, ambiguity, and required evidence. `IntentIR` then lowers to one or more AIR plans and eventually to `.ax`/runtime actions. The approved semantic contract is immutable for an episode; implementation plans may change underneath it only while preserving that contract.

## Representation layers

1. **Natural / structured intent** — human or system language, possibly incomplete or ambiguous.
2. **Intent IR** — typed desired outcome, constraints, preferences, requested authority, budgets, required evidence, unresolved ambiguities and provenance.
3. **AIR** — cognitive/execution plan: Observe/Retrieve/Reflex/Generate/Think/Simulate/Prove/Act/Verify.
4. **Axon program / capability artifacts** — deterministic executable implementation and requested effects.
5. **Execution/evidence graph** — what actually happened, which evidence passed, and how it relates back to the approved intent.

The layers are linked by digests and explicit lowering records. No layer may silently widen the authority or relax the acceptance contract of the layer above it.

## Intent schema

A minimal `IntentIR` contains:

- stable intent ID/version and source/provenance;
- objective(s) and optimization direction(s);
- hard constraints and invariants;
- soft preferences and tie-breakers;
- target scope / semantic objects;
- requested authority/effects and prohibited effects;
- resource and time budgets;
- acceptance/evidence requirements;
- risk / reversibility requirements where applicable;
- ambiguity set, confidence/provenance for each interpretation, and questions requiring resolution;
- assumptions and environmental dependencies;
- parent intent for system-generated/sub-intents;
- approval state and approval artifact digest.

Unknown is distinct from absent, and preference is distinct from constraint. A model-generated confidence does not transform an ambiguity into a resolved fact.

## Human and system-generated intents

The same typed representation accepts:

- human-authored product/task intent;
- Cortex-generated improvement intent;
- verifier-generated investigation intent;
- knowledge-derived optimization hypotheses;
- curriculum-generated training/evaluation intent.

System-generated intents have **no implicit self-authority**. A proposal to improve the compiler, verifier, world model or runtime requests authority and evidence exactly as a human-authored intent does. It cannot alter its own admission policy or acceptance evidence.

## Ambiguity resolution

The compiler must surface materially different interpretations before consequential execution. Resolution may use deterministic parsing, Reflex, Think, retrieval or a human question, but the resulting choice and source are recorded. If multiple interpretations are intentionally allowed, the Intent IR represents the disjunction explicitly and the acceptance contract states what evidence distinguishes/accepts them.

A low-confidence interpretation cannot silently become an executable default. Non-consequential exploration may run under an explicitly bounded development profile, but its outputs remain proposals until the intent is resolved/admitted.

## Semantic approval and rendering

Humans should not need to approve raw AST syntax alone. Axon provides a deterministic semantic renderer that summarizes:

- what outcome is being optimized;
- what may and may not be changed/touched;
- what authority/effects are requested;
- what budgets apply;
- what evidence is required for completion/promotion;
- which ambiguities/assumptions remain;
- how the current AIR/program differs from the previously approved intent.

The typed Intent IR remains the machine contract. Rendered prose is a review surface whose digest references the exact IR version. Editing the IR invalidates prior approval.

## Intent → AIR lowering

Lowering produces one or more candidate AIR graphs constrained by the approved intent. Planning may replan dynamically, but every action must trace to an active goal/constraint/evidence clause. A planner may narrow authority or add stronger checks; it may not remove a required check or widen effects without an explicit new intent/approval event.

`DoneClaim` is evaluated against the Intent IR acceptance contract plus independent verifier evidence. The reasoner cannot redefine success after seeing the outcome.

## Self-optimization through intent

Self-improvement is expressed as typed `ImprovementIntent`, not unrestricted mutation. Example:

```text
ImprovementIntent {
  objective: minimize(compiler.build_time),
  target: infer/type-map subsystem,
  constraints: [preserve(reference_semantics), preserve(capability_monotonicity)],
  requested_authority: [edit(scope), run(registered_checks)],
  required_evidence: [full_suite, parity, benchmark, invariant_review]
}
```

Repository-knowledge or prediction discoveries may propose such intents, but CX-11/CX-18 admission remains independent.

## Acceptance gates

**G19-parse:** representative human/system intents round-trip through `IntentIR`; hard constraints, preferences, unknowns, authority requests and evidence requirements remain distinguishable and provenance-preserving.

**G19-ambiguity:** a prompt with two materially different valid interpretations cannot enter consequential execution until the ambiguity is explicitly resolved or represented as an approved disjunction; low confidence alone never authorizes a default.

**G19-authority:** lowering/replanning that attempts to widen requested effects, target scope or principal authority beyond the approved Intent IR is refused; narrowing remains allowed.

**G19-evidence:** a planner/reasoner attempts to drop a required test/proof/benchmark or declare DONE under a weaker success criterion. Independent completion remains blocked by the original approved contract.

**G19-render:** semantic rendering of an approved intent is deterministic for the same IR/version, exposes requested authority/evidence/assumptions, and changes its digest when the IR changes; stale approval cannot attach to the modified intent.

**G19-trace:** every consequential action/evidence receipt in one vertical episode can be traced back to the active Intent IR clause(s), AIR lowering record and exact approval artifact; orphan consequential actions fail the trace/admission check.

## Build slices and exclusions

First implement the typed schema, parser/lowering adapter and deterministic semantic renderer around the existing `axon intent compile` / review concepts; do not require new language syntax. Demonstrate prose → Intent IR → reviewed contract → simple AIR repair → evidence → explanation. Only later consider surface-language syntax if repeated workload evidence shows it is useful.
