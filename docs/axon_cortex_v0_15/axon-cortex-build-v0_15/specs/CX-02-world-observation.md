---
id: CX-02
title: "Software observations, state and working memory"
status: Draft
authority: Proposed
depends_on: ["CX-00"]
first_stage: M1
implementation_evidence: []
---

# CX-02 — Software observations, state and working memory

## Intent and source basis

Produce compact, provenance-bearing state without losing necessary information. S2 pp.26–30 motivates structured observations and refreshed action spaces. S1 pp.6–7 documents R44 value materialization and its limits. See [SOURCES](../SOURCES.md).

## Decisive fork

Use an incremental semantic observer with explicit incomplete state, not full-repository prompting and not a claim that a code graph is already a learned world model.

## State contracts

`WorkspaceSnapshot` identifies tracked and untracked permitted files, content digests, base revision, dependency lockfiles, generated artifact manifests, configured toolchain/environment, and observer version. A Git commit alone is insufficient. Hashing the entire machine is neither required nor permitted; the manifest states its observation scope and unknown inputs.

`Observation` contains snapshot ID, goal digest, diagnostics, relevant symbols/types/callers, changed files, available test descriptors, recent action outcomes, retrieval references and a partial-observation report. Each fact includes origin, artifact digest, extraction method, observed time and trust class. Distinguish stale, absent, parse-failed and explicitly unknown.

`WorkingState` holds subgoals, hypotheses, assumptions, contradictions, experiment results, remaining budgets, attempted strategies and evidence references. Store concise, externally inspectable decision records; no requirement to preserve private model reasoning. A belief does not overwrite an observation. Contradicting facts stay visible with provenance until resolved.

## IDs, compression and retrieval

Within one snapshot, object IDs are stable and collision-checked. Across revisions, use an explicit symbol mapping with confidence and failure cases; never assume a string ID still refers to the same node. Serialization is canonical for identical input content and configured nondeterministic fields. Real clocks, network state and races are recorded rather than wished away by sorting JSON.

Observation compression has a hard token/size budget, retains permissions and critical diagnostics, and exposes omitted regions with reasons. A retrieval result carries context-source hashes and a freshness deadline. Caches are namespaced by repository/tenant and invalidated by content, parser, model, schema or policy changes where relevant.

Support `INSPECT`, `SEARCH` and `EXPAND_SCOPE` within existing authority. A task can request more context without receiving more permissions. Large action spaces use hierarchical retrieval; measure whether the oracle-relevant target remained available. If no relevant candidate is present, return insufficient-context or abstain rather than force a bad choice.

## Broken-code behavior

Use recoverable parse trees where available; otherwise fall back to explicitly marked lexical/file-level observations. A missing type must be represented as Unknown, not inferred by another unsound heuristic. New-file proposals refer to a permitted creation namespace and validated artifact manifest; they are not blocked merely because the file was not in the initial snapshot.

Do not persist live interpreter closures, channels, aliased mutable objects or native handles across R44 cells. Persist typed serializable values and artifact references, then reacquire authority-bound runtime handles each epoch.

## Interfaces

Proposed protocol functions: `observe(snapshot, goal, scope_budget)`, `expand(observation_id, requested_scope)`, `diff(old_snapshot, new_snapshot)`, `resolve(object_id, snapshot_id)` and `retrieve(query, snapshot_id, trust_filter)`. These are contract names, not existing Axon builtins.

## Acceptance gates

**G02-canonical:** identical snapshot and observer inputs produce equal canonical observation bytes excluding declared volatile envelope fields; round-trip loses no fact provenance.

**G02-partial:** deliberately break syntax/type resolution. Observer still returns a useful partial state and never invents a successfully inferred type.

**G02-stale:** mutate tracked, untracked and dependency inputs separately. Relevant cached observations invalidate and old object references cannot silently resolve to new targets.

**G02-recall:** a fixture whose fix is outside the initial neighborhood can request bounded expansion and expose the correct target; report candidate recall separately from solver success.

**G02-injection:** source comments and retrieved text containing authority instructions are stored as untrusted content and cannot change permissions or task contracts.

## Build slices and exclusions

Implement file/diagnostic snapshots first; add semantic neighborhoods second; add working-memory and retrieval policies after the single-step loop works. Full ontology induction, multi-domain perception and a predictive world model are separate specs. Reuse existing parser/type information rather than duplicating it.

## v0.3 candidate search and absence semantics

The observer/capability compiler measures candidate recall independently from Reflex selection. It must distinguish `NoneSuitable`, `NeedMoreObservation`, `SuitableButUnauthorized`, and `InferenceUnavailable`. Candidate expansion may use flat scoring, retrieval+rereanking, hierarchical selection or bounded beam search, but every strategy records search cost, scope lineage and final recall. A top normalized Choice value is never evidence that a suitable option exists.

## v0.4 imported software observations

CX-16 may import MiCode/external semantic observations, but imported facts retain source-system identity, freshness/omission receipts and trust class. Axon may map semantic objects into its ontology only through explicit Exact/Projected/Approximate/Unresolved mappings. Imported observations never override a fresher authoritative local observation merely because they are more complete.
