# Intent compiler build guide

## Objective

Build the missing layer above Cortex: **natural/structured intent → typed Intent IR → semantic review/approval → constrained AIR → Axon execution/evidence**. The first vertical slice should prove contract preservation, not language-generation sophistication.

## Architectural rule

Do not compile free-form English directly into authority-bearing `.ax` or executable AIR. Natural language is untrusted input to an intent compiler. The first authoritative object is an approved typed `IntentIR`.

```text
human/system intent
      ↓
intent parser / resolver
      ↓
Intent IR
      ↓
ambiguity + authority + acceptance review
      ↓
approved semantic contract
      ↓
AIR lowering / planning
      ↓
Axon program/actions
      ↓
independent evidence
      ↓
result explained against original intent
```

## First vertical scenario

Input:

> Make this parser path faster without changing observable behavior. Do not add dependencies or network access. Keep peak memory within 10%. Show benchmark and regression evidence.

Expected typed contract:

```text
objective: minimize latency(parser_path)
hard constraints:
  - behavior_equivalent(baseline)
  - peak_memory_delta <= 10%
  - no_new_dependencies
  - no_network
requested authority:
  - inspect/edit(parser scope)
  - run(registered tests/benchmarks)
required evidence:
  - regression suite
  - parity/reference check
  - benchmark
```

Cortex may choose many different plans. All must preserve this contract.

## Slices

### I0 — Source-truth mapping

Map the existing `axon intent compile`, `axon ast review`, approval artifact, deploy gate and structured-prose implementation. Record actual schemas, trust assumptions and drift before adding another layer. Reuse existing components where possible.

### I1 — Intent IR schema

Implement/version the CX-19 schema and round-trip fixtures. Preserve unknown/absent, hard/soft, requested/forbidden authority, evidence requirements and provenance separately.

### I2 — Resolver and ambiguity protocol

Convert simple structured prose to Intent IR. Emit explicit unresolved alternatives/questions. No consequential default on materially ambiguous input.

### I3 — Semantic renderer + approval

Render intent into stable human-readable contract text plus machine-readable diff. Approval binds the exact Intent IR digest/version. Modified intents require renewed approval.

### I4 — Intent→AIR lowering

Lower one simple repair/optimization intent to an AIR graph. Every AIR node/action carries clause lineage to objectives/constraints/evidence requirements. Replanning cannot relax the parent contract.

### I5 — Completion and explanation

Map independent verifier receipts back to required evidence clauses. Produce an explanation: achieved objective, preserved/violated constraints, authority actually exercised, evidence obtained, unresolved items.

### I6 — ImprovementIntent

Create one **system-generated** improvement proposal from a measured Cortex/Axon bottleneck. It must enter through the same intent/authority/admission flow and cannot activate itself.

## Inner / outer / meta loops

**Intent inner loop:** parse → type/normalize → detect ambiguity/conflict → resolve/refuse → render. It terminates on a valid unresolved/refused/approved contract; it does not silently retry until an LLM returns a convenient interpretation.

**Task outer loop:** approved Intent IR → plan → act → observe → replan while preserving clauses → independent completion. Replanning changes method, not success semantics.

**Self-optimization loop:** measurement/knowledge → ImprovementIntent candidate → normal intent/admission path → candidate implementation → evidence → promote/reject. The learner cannot edit the contract or evaluator after seeing results.

**Intent meta loop:** analyze recurring ambiguities, clause types and renderer failures; propose parser/schema/UX improvements through normal governance. Do not adapt the active acceptance contract during the episode being evaluated.

## Negative cases

- English asks for faster performance but omits what behavior must remain stable: require ambiguity/acceptance resolution.
- Model lowers `no network` into an AIR HTTP call: authority gate refuses.
- Planner drops the benchmark because tests passed: completion remains blocked.
- System-generated compiler optimization asks to edit verifier policy: separate authority/admission refuses.
- Intent edited after approval: old approval digest is invalid.
- Semantic renderer hides an assumption or requested capability: conformance gate fails.

## Definition of done

The slice is complete only when one prose intent produces an approved typed contract, a simple Cortex/AIR execution, independent evidence, and a final result traceable to the original clauses; all negative cases above are runnable. Do not call prose→code generation alone an intent compiler.
