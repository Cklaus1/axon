# ASI OS · Semantic Compilation Build Pack
**Release 0.3.0 · 6 October 2026 · Cortex v0.23-aligned contracts + offline conformance harness**

> Compile approved intent into inspectable candidate execution, evaluate it independently, repair immutably, and learn without duplicating authority.

This revision aligns Semantic Compilation with **Axon Cortex v0.23** and its pinned **Axon Forge v0.3** substrate. The central change is architectural: Semantic Compilation is now an **integration/compilation profile over canonical Cortex owners**, not a second IntentIR/AIR/evidence/experiment/resource system.

## Start here
Read `SPEC.md`, then `reports/CORTEX_OVERLAP_REVIEW.md`, `docs/CORTEX_ALIGNMENT.md`, `reports/ADVERSARIAL_REVIEW_V030.md`, and `docs/FORGE_SOFTWARE_SEMANTICS.md`. Then run:

```bash
python -m venv .venv
. .venv/bin/activate
python -m pip install -r requirements.lock
python -B tools/verify_manifest.py
python -B tools/check_pack.py
python -B -m unittest discover -s tests -v
python -B -m harness.simulate --case document
```

The checks make no network requests, call no live model, and create no external business effects.

## v0.3 architecture

```text
approved intent
   ↓
CX-19 IntentIR
   ↓ explicit lowering
CX-04 AIR / portable Semantic Plan projection
   ↓ optional software-semantic bridge
Forge v0.3 SIR / Transform / Migration / BuildProgram
   ↓
CX-13 qualified execution
   ↓
CX-32 evidence + CX-21 evaluation
   ↓
CX-33 experiment / CX-29 strategy learning
   ↓
CX-11 / CX-18 admission and promotion
```

No arrow grants authority. CX-03/CX-13/CX-34 recheck current authority/resources at protected dispatch.

## Important v0.3 changes

- **No duplicate IR authority.** CX-19 IntentIR, CX-04 AIR, and Forge SIR remain distinct canonical layers in Cortex mode.
- **Semantic operation ≠ execution mechanism.** A node now declares `semantic_op` separately from `mechanism`; a model call can implement generate/reason/decide/predict, while the same semantic operation may later move to code or a tool.
- **Strategy identity is first-class.** Compiler, repair, evaluator, routing, retrieval, optimization, resource-placement and software-transform strategies have versioned lineage.
- **Evidence/experiments/resources are projections.** Local records attach to CX-32/CX-33/CX-34 owners rather than creating parallel authorities.
- **Forge is the software-semantic substrate.** SIR, Transform, Migration, SemanticDiff, BuildProgram, Backend and structured diagnostics stay Forge-owned.
- **Hard gates precede optimization.** Correctness/security/authority failures cannot be compensated by latency or cost.
- **Coding truth has precedence.** Current compiler/source/conformance evidence outranks stale docs or model priors for the active toolchain revision.

## What is in this package?

| Location | Purpose |
|---|---|
| `SPEC.md` | Normative Semantic Compilation contract and Cortex alignment requirements |
| `docs/CORTEX_ALIGNMENT.md` | Canonical owner map and representation boundaries |
| `docs/FORGE_SOFTWARE_SEMANTICS.md` | IntentIR/AIR/SIR bridge and software-semantic path |
| `docs/STRATEGY_EVIDENCE_EXPERIMENTS.md` | Strategy, UEG/CX-33 projection and resource rules |
| `schemas/` | Portable boundary schemas and offline fixture contracts |
| `integration/` | Cortex/Forge hash locks, owner profile and requirement crosswalk |
| `reports/CORTEX_OVERLAP_REVIEW.md` | Requirement-level overlap analysis |
| `reports/ADVERSARIAL_REVIEW_V030.md` | Attacks found and fixes folded into v0.3 |
| `examples/` | Synthetic plans, evaluation contracts, owner/strategy/evidence/resource/Forge examples |
| `harness/`, `tests/`, `tools/` | Offline validation, negative fixtures and repair demonstration |
| `build/` | Build plan, acceptance gates, test matrix and traceability |
| `docs/NEXT_INTEGRATIONS.md` | Concrete live Cortex/Forge/MiCode conformance work after this pack |

## What is runnable now?
The package validates schemas, selected graph/type/effect invariants, semantic-operation/mechanism separation, owner-profile consistency, dependency locks, negative fixtures, and a synthetic localized-repair loop. It does **not** implement Cortex AIR lowering, Forge execution, CX-03 authority, CX-13 isolation, CX-32/33/34 services, production adapters, durable scheduling, or measured model economics.

## Recommended first vertical slice
Use one repository test-repair task with no push/deploy. Bind exact CX-19 IntentIR, lower to the existing CX-04 AIR seam, use Forge SIR only where available, preserve the CX-21 evaluator, submit evidence to CX-32, and keep CX-29/CX-33 in record-only experimentation. The goal is to prove owner-preserving compilation and evidence flow before adding autonomous optimization.
