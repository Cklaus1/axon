# Schema-decision frontend build loop — v0.20

Read CX-03/CX-05/CX-26 and [the bounded profile](../schemas/SCHEMA_DECISION_PROFILE.md). This is an extension at existing seams, not a new runtime or `.np` compiler. Begin with B210–B213 and the `schema_decision_contract` profile; no learned model is required for that slice.

## Contract-first inner loop

1. Inspect actual capability registry, schema normalization, question codec, composition validator and executor owners. Record a live file/use-site map and current versions; do not invent crate/module names from this package.
2. Freeze the normalized supported subset, limits, identity/provenance fields and explicit unsupported variants. Identify exact mappings into existing ArtifactCatalog/questions/composition structures. Write rejection fixtures before code.
3. Implement one local schema adapter and round-trip enum/Boolean/bounded-integer/optional/source-span cases. Network discovery stays disabled until separately authorized.
4. Implement active-branch decode, typed projection and full-invocation validation. Test missing Boolean/presence results, no match, malformed distributions, stale state/schema, non-BMP Unicode offsets and contradictory fields.
5. Connect to the trusted resolver/PermissionGate without letting descriptions, hints, probabilities or predicted completion create authority. Recheck snapshots/grants at actual dispatch; preserve execution/check terminal states.
6. Run focused tests and existing host ship gates. Merge through one shared-contract integrator. Retain unsupported cases rather than replacing them with an approximate executable schema.

Stop on an authority/identity mismatch, unsupported semantics, budget/cancellation, unreconciled effects or bounded no-progress. A missing candidate is a reason to observe/escalate, not to synthesize a fake catalog member.

## Outer integration loop

Pair B213 with MiCode T211 on versioned fixtures, then separately run actual owner/consumer call sites at pinned source revisions. Report source/span/schema identities and complete action validation, plus failures and inactive speculative costs. Offline conformance is not live WebMCP compatibility or sandbox proof.

## Meta review and staged expansion

Inspect which requests remain unsupported and why. Propose only one additional schema construct or candidate-conditioned backend per experiment. Qualify its semantics, resource limits, negative cases and impact on calibration before updating the profile. Never add a permissive catch-all parser to improve coverage statistics. Defer arbitrary arrays/unions/references and additional schema families; no mandatory broad-adapter migration.

## Package reference commands

```sh
python -B -m unittest discover -s tests -p 'test_schema_reflex*.py' -v
python -B tools/package_views.py --write
python -B tools/validate_package.py
```

Run normal live repository tests at implementation time; these package commands exercise model-free references only. See [distillation loop](LIGHTWEIGHT_REFLEX_DISTILLATION.md) for the separate learned candidate path.
