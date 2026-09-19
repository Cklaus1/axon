# MiCode ↔ Axon experience bridge build guide

## Purpose

MiCode is an optional research/experience plane for Axon Cortex. The bridge exists to turn real coding sessions into versioned evidence and to let MiCode exercise approved Cortex policies without sharing authority models.

## Role split

```text
MiCode
  coding world + human-visible harness + episode producer + repo knowledge miner
        │
        │ versioned artifacts only
        ▼
Axon Cortex
  cognitive runtime + world models + Reflex + admission/crystallization
        │
        ▼
Axon language/compiler/OS
  verified execution substrate + destinations for promoted knowledge
```

MiCode remains independently safe/unsafe according to MiCode's own runtime. Axon must not infer that a MiCode action was sandboxed or scope-confined unless the exported evidence proves that specific property. Conversely, an Axon policy exported to MiCode never bypasses MiCode's gate.

## Initial contract

Start with seven export families from MiCode: canonical coding episode, software observation/catalog, decision dataset, failure attribution, benchmark bundle, repository knowledge candidate, and skill/tool/compiler candidate. Start with five Axon export families: AIR policy, Reflex backend manifest, verifier profile, capability/action schema, and ontology/version map.

Implement a dummy producer and consumer first. Freeze schema/version/error semantics before live coupling. Use explicit `Unknown`, `NotObserved`, `Denied`, `Failed`, `Aborted`, `NotExecuted`, `Verified`, `Unverified` states rather than flattening them.

## Build sequence

1. Map MiCode fields to CX-16 without exposing credentials or local authority handles.
2. Build canonical JSON/JSONL fixtures with edge cases.
3. Validate round-trip and schema migration.
4. Import one recorded MiCode repair episode into Cortex replay/evaluation.
5. Export one non-authoritative Cortex decision/verifier profile to a MiCode dummy consumer.
6. Add data-eligibility/lineage checks before any imported episode reaches learning.
7. Add semantic replay/counterfactual comparison on imported episodes.
8. Only then connect real MiCode production traces.

## Non-goals

- no RPC dependency from Axon runtime to MiCode;
- no shared permission token or principal namespace;
- no claim that MiCode episode success implies optimality;
- no automatic promotion of MiCode skills into Axon;
- no raw transcript-as-training shortcut.

## Exit demonstration

One real or fixture MiCode episode round-trips into Axon, supports a recorded-policy counterfactual replay with no external effects, preserves all important outcome/authority distinctions, and remains ineligible for training when its data-use manifest is removed.

## v0.12 MiCode v0.5 bridge

MiCode's Semantic Supervisor Plane and Semantic Working-Set Manager are useful experience producers for CX-27/CX-28. Import supervisor assessments/interventions, working-set receipts, semantic-GC decisions, model routes and subsequent protected outcomes as replayable data. Re-derive eligibility/corpus role on the Axon side and never treat MiCode completion/risk/relevance labels as Axon authority.


## Self-hosting evidence

MiCode v0.5+ internal supervisor, working-set, composition, routing and alignment decisions should be ingested as CX-29 cognitive-operation records where identities can be reconciled. `/build-loop` outcomes provide the first high-volume self-application corpus, but protected benchmark roles remain separated from training/development feedback.
