# Decision Composition Runtime build guide

Implements CX-26. The objective is to prove that Cortex can solve useful work by selecting and composing existing typed artifacts before asking a generator to synthesize new content.

## Build-inner loop

1. Freeze one approved Intent IR and world snapshot.
2. Compile an `ArtifactCatalog` from authoritative runtime objects.
3. Add deterministic projections for values that already exist.
4. Run a bounded selection policy over the catalog.
5. Build a small typed composition graph.
6. Validate schema, dependency, freshness and authority.
7. Compare the result with a generative control under the same verifier.
8. Record exact candidate/catalog/order/projection/composition lineage.

Stop on a gate failure, missing authoritative object, authority conflict or exhausted experiment budget. Do not invent a candidate to keep the loop moving.

## Build-outer loop

Grow from one selection to three composition families:

- **select/copy**: authoritative values already present in the world;
- **plan composition**: choose and order registered verification/build steps;
- **artifact composition**: assemble a typed spec/DAG/pipeline from known pieces.

After each family, test held-out examples and measure generation avoided, latency, model calls, verifier success and fallback rate.

## Meta loop

Ask whether an observed repeated generative pattern should become:

- a new catalog entry;
- a projection rule;
- a composition template;
- a specialized composition policy;
- a deterministic tool/compiler primitive.

Meta proposals cannot edit the protected verifier or widen the active capability catalog.

## First pilot

Use a small Axon coding task where a requested fact and next verification actions already exist:

```text
Intent: identify the authoritative type of symbol X and run the narrowest registered check proving the related edit.
```

The pilot must SELECT the symbol/type object, PROJECT the authoritative type, COMPOSE a verification plan from registered checks, and refuse stale/unauthorized alternatives. Only an unresolved novel patch body may invoke GENERATE.

## Metrics

- verified task completion;
- exact-source projection rate;
- generation calls avoided;
- composition validation failure rate;
- stale/authority refusal correctness;
- latency/cost/tool-call reduction;
- fallback rate;
- transfer to unseen catalogs/repositories.
