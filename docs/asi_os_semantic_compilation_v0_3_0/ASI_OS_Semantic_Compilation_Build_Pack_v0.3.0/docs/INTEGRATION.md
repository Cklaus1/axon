# Axon Cortex v0.23, Forge v0.3, MiCode and BFlow Integration Plan

## Reviewed inputs
This revision was reviewed against the supplied **Axon Cortex v0.23** package and the **Axon Forge v0.3** dependency embedded in that package. Exact archive hashes are pinned in `integration/CORTEX_V023_INPUT_LOCK.json` and `integration/FORGE_V03_INPUT_LOCK.json`.

The Cortex package already defines canonical owners for intent semantics, AIR execution semantics, authority, isolation, evaluation, admission/promotion, evidence, causal experiments, resources, coding workflow and self-application. The integration rule is therefore **extend/project, do not duplicate**.

The supplied BFlow documentation remains useful as an intent/orchestration source context. Its agent-first policy still requires an explicit product decision if deterministic execution is introduced as a normal path.

## Binding map
| Semantic Compilation function | Canonical integration target |
|---|---|
| Normalize/approve intent | CX-19 IntentIR |
| Candidate execution semantics | CX-04 AIR |
| Software/program semantics | Forge v0.3 SIR; CX-15/CX-30 consumers |
| Coding workflow | CX-16 / MiCode |
| Capability request/current grant | CX-03 |
| Isolation/effect execution | CX-13 |
| Protected evaluation | CX-21 plus existing verifier owners |
| Admission/promotion | CX-11 / CX-18 |
| Strategy/self-application | CX-29 |
| Evidence/provenance | CX-32 UEG |
| Causal experiments | CX-33 |
| Physical resources | CX-34 / Compute Fabric |

## Required implementation seams
1. Bind approved intent by canonical CX-19 reference and preserve clause lineage through compilation.
2. Lower the portable Semantic Plan to the repository's CX-04 AIR representation rather than adding a second executor/VM.
3. For software tasks, call Forge through versioned references; do not clone SIR/Transform/Migration/BuildProgram schemas.
4. Request capabilities through CX-03 and execute protected work through CX-13, retaining external execution receipts.
5. Submit observations as CX-32 evidence projections and optimization trials as CX-33 experiment profiles.
6. Use CX-34 resource projections only for planning; recheck availability/authority at dispatch.
7. Keep CX-21 evaluator/locked criteria outside compiler/repair/optimizer control.
8. Keep CX-29 optimization record-only until a separately admitted experiment allows exploration or promotion.

## MiCode
MiCode remains coding-workflow owner. The preferred coding loop is:

`task/IntentIR → AIR → source/SIR → diagnostics/Transform/Migration → compile/test → CX-13 receipt → CX-32 evidence → CX-21/CX-33 comparison`.

When a backend supplies structured diagnostics/SIR references, MiCode should consume them instead of relying only on raw terminal text.

## BFlow
BFlow may supply user/business intent, orchestration UX and evaluation/red-team adapters. It should not become a second policy kernel, experiment store, evidence graph or resource registry. If BFlow continues to require conversational agents for all business logic, deterministic compiled execution must be approved as an explicit policy amendment rather than silently reinterpreted.
