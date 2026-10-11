# v0.21 research integration contract

**Normative scope:** additive profiles for existing CX owners. This document supersedes earlier conversational proposals for seven independent planes. It does not replace source authority, declare a new runtime ABI or assert implementation.

## Ownership and version rules

EVO/DEC/EVL/RTR/CVM/SPX/TEL are stable navigation aliases. The authoritative mapping is [SOURCE_OWNER_MAP](../integration/SOURCE_OWNER_MAP.json). CX-11 retains independent admission, CX-03 and source authorization retain effect permission, CX-10 retains canonical evidence, CX-28 retains working-set projections, CX-34 retains capability registry/scheduling, and CX-37 retains ACE provider/runtime ownership. CX-36 bytes and neural wire formats are unchanged.

The live source's CX-35 Reflex work maps to CX-05 and related existing owners without renaming the source ID or consuming the package's reserved ID. New work uses B223–B254 and explicitly prefixed `Gxx-r21-*` gates. Old requirements, source mappings and MiCode v0.14 origin references remain historical contracts, not proof of current peer delivery.

No new `.reflex`, `.clm`, `.dec` or alternate neural container is introduced. Model/head revisions belong in the existing artifact lifecycle. A provisional research record schema is a package reference/negotiation payload, not an automatically accepted `axon-reflex/1` request or new top-level ACE wire tag.

## Shared flow

```text
canonical observations + current grants + registry epoch
  -> authorized candidate view (CX-03/CX-34)
  -> bounded state projection (CX-28)
  -> eligible deterministic / CLM / Jev decision provider (CX-05)
  -> qualified calibration and routing policy (CX-06)
  -> optional isolated draft/verify proposal (CX-26)
  -> existing permission and transactional effect barrier (CX-03)
  -> actual final checks and evaluation evidence (CX-01/CX-21)
  -> existing replay, audit, usage and learning records (CX-10)
  -> independent strategy/head admission, never self-activation (CX-11)
```

Eligibility is checked before sending private context or candidates to a provider. Ranking an action is not authorization to execute it. Evaluation may consume DEC judgments, but deterministic completion and independent admission are not delegated to DEC. EVO can propose policies for these components but cannot change the frozen evaluator, safety floor or approval authority during an experiment.

## Seven profile guides

The [EVO guide](EVO_REGULARIZED_EVOLUTION.md), [DEC guide](DEC_RESEARCH_PROVIDERS.md), [EVL guide](EVL_CHECKLIST_EVIDENCE.md), [RTR guide](RTR_MODEL_ROLE_ROUTING.md), [CVM guide](CVM_CANONICAL_PROJECTIONS.md), [SPX guide](SPX_BOUNDED_CASCADES.md), and [TEL guide](TEL_WHOLE_TASK_ECONOMICS.md) define implementation invariants. Every guide is an Axon adaptation, not a claim that its research source already implements these protections.

## Common record and lifecycle

Use collision-free `task_id`, `episode_id`, `attempt_id`, `candidate_id` and `call_id`, plus principal/tenant/workspace scope. Repeated trials on the same task and arm must not share identity. Bind records to the source snapshot, candidate manifest, effective input, model/provider runtime, tokenizer, pooling, head, wrapper/threshold policy and context projection. Information omitted for privacy must be a permission-qualified reference with explicit availability status, not fabricated content.

A decision result distinguishes raw scores, candidate-conditional probabilities, separately calibrated correctness and a reasoned disposition. `ABSTAIN`, `REFUSED`, `TRANSPORT_ERROR`, `CANCELLED`, `UNSUPPORTED` and a legitimate negative judgment are not interchangeable. Missing evidence/usage is unknown, never automatically zero or false. Existing refusal semantics remain intact.

Record exact nondeterministic results for replay. Replaying must not query live models, recompute a new context policy or reuse a different cache epoch. An episode digest binds content but is not, alone, externally anchored append-only integrity. Integrate the existing audit/recording mechanisms rather than advertising a second hash-based proof layer.

## Research versus release requirements

A P0 research priority means a required contract, migration seam and falsification plan. It does not mean all providers must be installed, all papers reproduced, or all models perform better than the incumbent. The initial release accepts a deterministic provider and explicit unsupported outcomes where learned adapters are not qualified.

The package conformance closure is deliberately small. Runtime build tasks remain explicit and model-free fixture success cannot satisfy them. Real pilots need a reviewed source revision, authenticated endpoints, frozen evaluation policy, budget, independent final outcome and rollback. A rejected/inconclusive research arm is a valid result; forcing a named method into production is not an acceptance criterion.

Optional scopes B251–B254 are outer-loop meta-policy learning, ANN-scale candidate selection, specialization and best-of-N. Native latent cache handoff, new inference engines, unrelated sandboxes, new ABI work and multimodal CLM support are outside this delta.

## Compatibility posture

Keep the existing deterministic Cortex policy adapter and grant boundary. Extend a negotiated Reflex capability rather than stuffing new fields into its strict existing wire. A legacy `/1` request retains existing meaning; unknown versions/profiles refuse. Provider output is bounded and schema-validated before entering the host; score fields cannot modify the grant snapshot. Permission denial must not trigger a retry as another principal.

Use existing feature flags/admission records. Start new research profiles disabled, then offline, then shadow, then one explicitly authorized low-risk scope. The rollback target is the prior source/registry/model/config revision, not merely a different model name. Sidecar/model processes are opt-in and confined; no installer edits shell startup files, starts background daemons or downloads weights during package validation.
