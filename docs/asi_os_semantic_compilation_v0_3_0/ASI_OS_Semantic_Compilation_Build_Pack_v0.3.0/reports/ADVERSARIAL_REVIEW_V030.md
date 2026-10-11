# Adversarial Review — Semantic Compilation v0.2 against Cortex v0.23

## Findings folded into v0.3

### A1 — Duplicate IR authority
**Attack:** treat `asi.aiir` as a second canonical AIR/Intent owner, creating divergent semantics and admission paths.
**Fix:** rename the portable wire object to a Semantic Plan projection; add an owner profile; state that CX-19 IntentIR and CX-04 AIR are canonical in Cortex mode.

### A2 — Semantic laundering through mechanism labels
**Attack:** mark a model/tool/code call as harmless merely because its mechanism appears deterministic or read-only.
**Fix:** separate `semantic_op` from `mechanism`; registry declares allowed semantic operations; linter rejects mismatches.

### A3 — Second capability/resource authority
**Attack:** use the local registry or a cached resource view as a bearer grant or reservation.
**Fix:** local registry/resource documents are projections only; CX-03/CX-34 own current authority/truth; dispatch rechecks current state.

### A4 — Experiment/evidence fork
**Attack:** let local experience rows become a second experiment/evidence system, enabling self-selected winners and broken lineage.
**Fix:** add canonical experiment and evidence projection contracts bound to CX-33/CX-32.

### A5 — Untracked prompt/config mutations
**Attack:** change compiler/evaluator prompts between trials without stable strategy identity, then attribute improvement causally.
**Fix:** add `StrategyArtifact` identity/lineage and require it for experimentable behavior.

### A6 — Software semantics collapsed into generic task graph
**Attack:** reason about code only through text patches and raw terminal output, losing semantic invariants and structured diagnostics.
**Fix:** add explicit Forge v0.3 bridge; preserve IntentIR/AIR/SIR separation; prefer Transform/Migration/SemanticDiff when available.

### A7 — Fast invalid candidates win
**Attack:** optimize a weighted utility where latency/cost compensates for failed correctness/security gates.
**Fix:** hard gates precede Pareto/utility ranking; experiment profile binds evaluator and hard-gate references.

### A8 — Stale resource or policy snapshot reused
**Attack:** reuse a plan because its compilation-time resource/policy view looked valid.
**Fix:** resource projections carry observation epoch and never authorize execution; current authority/availability rechecked at dispatch.

### A9 — Model prior overrides executable local truth
**Attack:** compiler follows plausible but stale model knowledge instead of the current toolchain/repository.
**Fix:** adopt Forge/CX-24 coding truth precedence and surface conflicts with provenance.

### A10 — Candidate evaluates or promotes itself
**Attack:** compiler/repair/evaluator strategy edits the yardstick or activates itself after producing a favorable score.
**Fix:** frozen evaluator/admission remains external; strategy/evaluation updates are separate experiments; candidate cannot self-admit.


### A11 — AIR bypass during Forge lowering
**Attack:** bind IntentIR directly to Forge SIR and omit the AIR layer, making software meaning appear equivalent to execution/cognitive meaning.
**Fix:** `forge-binding` now requires both `intent_ir_ref` and `air_ref`; the bridge is AIR→SIR and preserves lineage.

### A12 — Owner profile goes stale after Cortex upgrade
**Attack:** reuse a v0.23 owner map after repository owners or package bytes change.
**Fix:** owner profile dependency locks are exact-version/hash pinned and compilation provenance retains the profile reference. A changed Cortex/Forge input requires a new profile/review, not silent reuse.

### A13 — Projection poisoning
**Attack:** fabricate a syntactically valid CX-32/CX-33/CX-34 reference and treat it as authoritative evidence, experiment state, or resource reservation.
**Fix:** schemas only define projection shape; docs/gates explicitly require live canonical-service resolution. Offline validity never establishes authoritative existence or admission.

### A14 — Forge build success laundering an external effect
**Attack:** treat successful build/backend output as proof deployment/message/payment or other external effect succeeded.
**Fix:** Forge output remains software evidence only; protected effects require CX-13/provider receipts and independent evaluation.

### A15 — Cached plan revives revoked authority
**Attack:** reuse a valid historical plan plus prior approval/resource snapshot after grants were revoked or environment changed.
**Fix:** cache admission is structural only; protected dispatch rechecks current CX-03 authority and CX-34 state. Prior receipts/projections are not bearer grants.

### A16 — Optimizer chooses its own scorecard
**Attack:** a compiler/evaluator/repair strategy mutates hard gates, stopping rules or evaluator identity during an experiment, then wins on the changed metric.
**Fix:** CX-29 strategy identity and CX-33 experiment profile freeze controlled mutation, evaluator, hard gates, falsifier and stopping rule; ranking only occurs after hard-gate eligibility.

## Remaining implementation blockers
The package still does not implement live CX-03/CX-13/CX-32/CX-33/CX-34 adapters, actual AIR or Forge runtime lowering, signed approval verification, durable scheduling, production isolation, provider billing reconciliation, or real-task performance evidence. The v0.3 checks establish contract alignment and offline invariants only.

## Upstream Cortex package note
Running Cortex v0.23 with its documented `python -B` commands passes package validation. Running unit discovery without `-B` creates `tests/__pycache__`, after which the validator correctly flags generated cache files. That is a packaging/test-order footgun, not a product-runtime defect; downstream automation should use the documented `-B` invocation or clean generated caches before validation.
