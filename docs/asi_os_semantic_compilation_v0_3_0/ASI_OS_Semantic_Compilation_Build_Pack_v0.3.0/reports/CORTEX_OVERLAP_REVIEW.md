# Axon Cortex v0.23 vs ASI Semantic Compilation v0.2 Review

## Bottom line
There is **substantial conceptual overlap**, and that is good news: Cortex v0.23 already contains most of the governance, intent, execution, evidence, experiment, resource, and self-optimization machinery Semantic Compilation needs.

Using the 73 v0.3 normative requirements after alignment as a requirement-level crosswalk (not code similarity):

- **56 / 73 (76.7%)** map primarily to existing Cortex/Forge owners.
- **13 / 73 (17.8%)** are Semantic Compilation integration/lifecycle glue.
- **4 / 73 (5.5%)** are package/release-conformance requirements.

So the right move is **not** to build Semantic Compilation as another competing control plane. The value is to make it the compilation profile that composes Cortex owners into a coherent `intent → plan → evaluation/repair → execution → evidence → experiment → learning` loop.

See `integration/CORTEX_V023_REQUIREMENT_CROSSWALK.json` for the explicit classification.

## Highest-overlap areas
1. Intent compilation: Semantic Compilation SC-* strongly overlaps CX-19 IntentIR and `INTENT_COMPILER.md`.
2. Execution graph: prior AI-IR overlaps CX-04 AIR.
3. Evaluation/admission: EV-* overlaps CX-21/CX-11 and frozen-evaluator requirements.
4. Security/runtime: SEC-*/RT-* overlap CX-03/CX-13/CX-34.
5. Optimization/self-improvement: AX-* overlaps CX-29/CX-33 and the self-optimizing OS loop.
6. Evidence/provenance: OBS/ID areas overlap CX-32 UEG.
7. Software semantic compilation: Cortex v0.23 + Forge v0.3 adds SIR/Transform/Migration/BuildProgram capabilities not present in Semantic Compilation v0.2.

## Net-new value retained
The Semantic Compilation package still contributes a useful **product/runtime composition**: compilation records, work/evaluation separation, immutable targeted repair, plan reuse/cache semantics, adaptive interpreted-vs-compiled execution, provider-neutral evaluation envelopes, and concrete offline conformance fixtures. Those should stay, but bind to Cortex owners rather than duplicate them.
