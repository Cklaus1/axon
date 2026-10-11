# Cortex v0.23 Alignment Profile

## Decision
ASI Semantic Compilation is **not** a new owner of IntentIR, AIR, global capability authority, evaluation authority, evidence, experiments, resource truth, admission, or promotion when Axon Cortex v0.23 is present. It is the orchestration/compilation profile that composes those owners into an intent → candidate execution → evaluation/repair → experiment → learning lifecycle.

The rule is **projection and lowering, not duplication**.

## Canonical owner map

| Semantic Compilation concern | Cortex/Forge owner | Binding rule |
|---|---|---|
| Approved task meaning | CX-19 IntentIR | compile from exact approved intent reference; never replace with a local intent authority |
| Execution/cognitive semantics | CX-04 AIR | portable semantic plan maps to AIR; no second VM |
| Program meaning | Forge v0.3 SIR | use only for software/compiler semantics; keep separate from IntentIR/AIR |
| Grants/current authority | CX-03 | request/recheck; no grant minting |
| Isolated execution/effect receipts | CX-13 | execute through qualified seam; never trust candidate success flags |
| Evaluation | CX-21 / protected evaluator owners | frozen acceptance contract remains outside subject control |
| Admission and activation | CX-11 / CX-18 | candidate cannot self-admit or self-promote |
| Strategy/self-application | CX-29 | strategies are versioned candidates; previous-good rollback retained |
| Evidence/provenance | CX-32 UEG | local records are projection payloads, not a second graph |
| Causal experiments | CX-33 | attach profile to canonical experiment identity |
| Physical resource truth | CX-34 / Compute Fabric | consume epoch-bound projection; dispatch rechecks current state |
| Coding workflow | CX-16 / MiCode | MiCode remains workflow owner |
| Compiler/program semantics | Forge / CX-15 / CX-30 | Transform/Migration/BuildProgram/Backend contracts remain Forge-owned |

## What Semantic Compilation still owns

It owns the **integration semantics** of compiling an approved task into candidate execution structures, binding work and evaluation plans, targeted immutable repair, compilation provenance, reusable-plan packaging, compiled/interpreted/adaptive execution policy, and the operator-facing compile/inspect/verify/replay workflow. These are valuable even though most underlying authorities already exist.

## Representation stack

```text
natural/system intent
  ↓
CX-19 IntentIR                 desired outcome / constraints / authority request
  ↓ explicit lowering record
CX-04 AIR                      cognitive/execution semantics
  ↓ optional software bridge
Forge SIR                      program meaning for software/compiler tasks
  ↓ backend/build lowering
host/backend artifacts         executable implementation
  ↓
CX-13 execution receipts + CX-32 evidence
```

No arrow grants authority. Each lowering keeps source/target identity and evidence.

## Semantic operation vs mechanism

A major v0.3 correction is that a plan no longer treats `model.call`, `tool.call`, or `code.execute` as semantic meaning. Nodes declare both:

- `semantic_op`: observe, rule, retrieve, decide, generate, reason, predict, check, act;
- `mechanism`: transform, model.call, tool.call, code.execute, evaluate, human.request.

The capability projection declares which semantic operations a mechanism implementation may realize. This preserves Cortex's separation of cognitive operation, capability class, and physical mechanism and enables safe optimization without semantic laundering.
