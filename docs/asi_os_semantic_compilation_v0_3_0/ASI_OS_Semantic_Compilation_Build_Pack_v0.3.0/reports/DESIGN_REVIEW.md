# Author Consistency Review — v0.3

**Scope:** design/contract review of this build pack against the supplied Axon Cortex v0.23 package and its pinned Forge v0.3 dependency. This is not an independent security audit and does not establish live repository compatibility.

## Principal conclusion
The earlier Semantic Compilation design had too much ownership overlap with Cortex. The corrected architecture treats Semantic Compilation as a **compilation/integration profile over canonical Cortex/Forge owners**, not as a parallel control plane. The requirement crosswalk maps 56/73 requirements (76.7%) primarily to existing Cortex/Forge owners, 13/73 (17.8%) to integration/lifecycle glue, and 4/73 (5.5%) to package/release conformance. This is requirement ownership, not source-code similarity.

## Corrected or made explicit
- CX-19 IntentIR, CX-04 AIR, and Forge SIR are separate representations with explicit lowering/provenance.
- Portable Semantic Plans are boundary projections; they do not create a second AIR VM.
- `semantic_op` is separate from `mechanism`, preventing a model/tool/code implementation label from redefining semantic meaning.
- Forge owns software/program semantics; Semantic Compilation records an explicit IntentIR + AIR → SIR binding rather than cloning SIR/Transform/Migration/BuildProgram.
- CX-29 StrategyArtifact identity covers experimentable compiler/evaluator/repair/routing/retrieval/resource/transform behavior.
- CX-32 evidence, CX-33 experiments, and CX-34 resources remain canonical; local documents are projections only.
- Hard correctness/security/authority gates precede Pareto/utility ranking.
- Compile/build success never proves external effect success; CX-13/provider receipts and independent evaluation are required.
- Cached plans and old projections cannot revive revoked authority; protected dispatch rechecks current state.
- Jev-like typed judgment remains separate from generative repair, and a candidate cannot change its own protected evaluator or promotion rule.

## Adversarial review
`reports/ADVERSARIAL_REVIEW_V030.md` records sixteen attack/failure patterns and the corresponding fold-ins. Offline tests cover ownership locks, semantic laundering, projection shapes, strategy identity, hard-gate documentation, Forge package pinning and mandatory AIR lineage. Several attacks require live integration tests and therefore remain release blockers rather than being marked solved by schemas.

## Remaining release blockers
Live CX-19→CX-04 lowering/round-trip; live Forge SIR/build integration; CX-03/CX-13 authority/isolation/effect receipts; CX-32/33/34 canonical-service adapters; signed approval verification; durable scheduling/ledger; real sandbox fault injection; model/evaluator calibration; independent held-out task outcomes; MiCode workflow integration; measured end-to-end economics.

A green G0/G0A result proves package coherence and owner-alignment invariants only. It does not authorize production effects or establish quality/cost improvements.
