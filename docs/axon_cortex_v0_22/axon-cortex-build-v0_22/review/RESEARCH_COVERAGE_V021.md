# Research coverage — v0.21

This matrix is based on the uploaded v0.20 design and the reviewed source snapshot. Existing owners are retained; the added contracts do not imply production implementation. Every family has source references, build tasks and concrete product gates in the machine-readable matrix.

| Research | Existing coverage / actual source observation | Added tasks | Profile |
|---|---|---|---|
| RRSI | Substantial owners already exist in CX-08/11/12/29/33; named regularizers and complete hypothesis-history/anti-overfit qualification need explicit integration. | B234, B235, B236, B251 | EVO |
| CLM | CX-05/20/24/25/34 already cover Reflex learners and registry; current axon-reflex core is a deterministic protocol placeholder, not CLM. | B225, B226, B227, B230, B231, B252, B253 | DEC |
| Jev / RLCD | Already materially specified in v0.19/v0.20; extend and qualify providers rather than invent a new decision plane. | B225, B228, B230 | DEC |
| CheckEval | Existing protected evaluation owners and source check events; add frozen evidence-linked checklist semantics without replacing deterministic checks. | B232, B233 | EVL |
| TinyRouter / TRINITY / Semantic Router | Existing CX-06/34 routing and source AI gateway; named learned routing adapters and measured model-role switching remain unqualified. | B237, B238, B248, B253 | RTR |
| CliffCompaction | Existing CX-28 working-set contract; add original-event projections, pin closure, explicit mismatch behavior and consuming-model budgets. | B239, B240, B245 | CVM |
| TypeLLM/pijev | Existing order/calibration work partially covers this; add finite-sampling limits, label-aligned aggregation and wrapper-bound qualification. | B229, B230 | DEC |
| RLM-Cascade | Existing CX-26 composition and source patch proposal/verification; add bounded response cascade and effects barrier, initially without speculative tools. | B241, B242, B244, B254 | SPX |

## Critical distinction

The largest source gap is not missing names: it is qualified learned behavior behind the existing Reflex seam, plus complete runtime integration of the new evidence/context/cascade contracts. Jev/RLCD and calibration were already materially specified before this release. RRSI, CLM, CheckEval and the remaining references sharpen existing owners rather than replacing them.

Initial contract and deterministic guardrail work is required. Real model execution is independently authorized and evaluated. Outer-loop meta-policy search, ANN-scale routing, specialization and best-of-N are optional B251–B254. Rejection of a research arm is a valid qualification outcome.

See [source review](SOURCE_REVIEW_V021.md), [runtime seam map](../build/IMPLEMENTATION_SEAMS_V021.md) and [provider plans](../build/PROVIDER_QUALIFICATION_V021.md).
