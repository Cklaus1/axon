---
id: CX-28
title: "Semantic Working-Set Manager: context selection, semantic GC and recomputable-memory control"
status: Draft
authority: Proposed
depends_on: ["CX-02", "CX-04", "CX-10", "CX-13", "CX-19", "CX-24", "CX-27"]
first_stage: M2
implementation_evidence: []
---

# CX-28 — Semantic Working-Set Manager

## 1. Purpose

Long-horizon agents fail when every rule, skill, repository note, tool result and historical observation is kept in the active context forever. Ordinary summarization is also dangerous because it can erase exact paths, errors, constraints or evidence.

The Semantic Working-Set Manager (SWM) treats active context like managed memory. It decides what must remain verbatim, what can be represented structurally, what can be recomputed later, and what may be omitted from the active cognitive window while preserving durable provenance.

The SWM manages **context presence**, not truth or authority.

## 2. Working-set classes

Every context artifact receives a runtime disposition:

```text
PIN
KEEP_VERBATIM
KEEP_STRUCTURE
COMPRESS
DROP_RECOMPUTABLE
```

Typical examples:

- `PIN`: approved Intent IR, authority ceilings, unresolved acceptance clauses, active safety rules, protected verifier receipts;
- `KEEP_VERBATIM`: exact compiler error, relevant user instruction, unreproducible external result;
- `KEEP_STRUCTURE`: tool call identity/input plus a short typed outcome while bulk output lives in durable storage;
- `COMPRESS`: non-authoritative explanatory material where loss is measured and acceptable;
- `DROP_RECOMPUTABLE`: cheap observations that can be re-read from an immutable snapshot or rerun safely.

Dropping from active context does not delete durable episode data.

## 3. Context catalog

Selection operates over authorized/versioned context candidates:

```text
ContextArtifact {
  artifact_id,
  kind,
  semantic_description,
  source_ref,
  snapshot_digest?,
  revision?,
  authority_class,
  recompute_contract?,
  cost_to_reload,
  privacy_class,
  token_or_byte_size,
  dependencies[],
  last_use,
  provenance_refs[]
}
```

Candidate inventory is produced by the runtime. A semantic model may rank relevance, but it cannot invent hidden rules/skills/files and cause them to be treated as authorized context.

## 4. Dynamic rules, skills and maps

The SWM can select which already-authorized rules, skills, repository maps or reference documents are relevant to the current Intent/AIR node/files being edited.

Selection is re-evaluated when the operational context changes materially—for example when a worker begins editing a new subsystem—even if the original natural-language request was vague.

Safety-critical `always` rules and other pinned material bypass semantic filtering.

## 5. Semantic context GC

Tool history receives special handling. For each tool use/result pair, the manager may ask separately:

- does knowing that this call occurred still matter?
- does the exact result still matter?
- is the result safely recomputable from the bound snapshot?

The rebuild invariant is strict: a retained result cannot outlive the identity of the call that produced it, and a truncated/omitted result must retain enough provenance to retrieve or recompute it when allowed.

## 6. Recompute contracts

`DROP_RECOMPUTABLE` is allowed only when a concrete recompute contract exists, including:

```text
RecomputeContract {
  operation_ref,
  snapshot_or_input_digest,
  authority_requirements,
  side_effect_class,
  expected_cost,
  deterministic_or_variance_notes,
  expiry
}
```

Effectful, externally mutable, rate-limited, or expensive observations may not be treated as cheaply recomputable merely because a similar tool exists.

## 7. Staleness and async decisions

Context selection frequently happens asynchronously. A decision is valid only for the state/catalog digest it evaluated.

If the prompt, edited files, intent revision, candidate catalog, rule set or relevant snapshot changes before application, the selection is stale and must be discarded or re-evaluated.

## 8. Context receipt

Every model/decision call receives an effective-context receipt:

```text
WorkingSetReceipt {
  intent_digest,
  state_digest,
  catalog_digest,
  selected_artifact_ids[],
  pinned_artifact_ids[],
  omitted_artifact_ids[],
  compressed_artifact_ids[],
  recomputable_artifact_ids[],
  total_size,
  selection_policy_revision,
  model_route?,
  cache_economics?
}
```

This makes replay and calibration meaningful: a result is evaluated against what the model actually saw, not what the system theoretically possessed.

## 9. Model routing and cache economics

Working-set decisions interact with model routing. A cheaper model is not cheaper if switching invalidates a large reusable prefix/cache or lacks the required capability/context window.

Routing may account for:

- required modality/capability;
- context length;
- warm/prefix cache reuse;
- latency/cost budgets;
- privacy/locality requirements;
- expected quality and calibrated abstention;
- current working-set size.

The routing decision remains advisory to the deterministic runtime policy.

## 10. Semantic query planning

CX-24 semantic predicates should participate in normal query planning rather than forcing a learned full scan first. The SWM/query planner should prefer:

```text
exact deterministic filters
→ authorization/privacy filters
→ cheap lexical/index narrowing
→ semantic predicate/ranking
→ expensive generation/reasoning only when needed
```

Batching, bounded read-ahead and cache reuse are implementation strategies whose quality/recall effects must be measured. `LIMIT`/early-stop semantics may reduce semantic work only when doing so preserves the query contract.

## 11. Cognitive cascade

The working set also feeds a first-class fallback cascade:

```text
RULE
→ RETRIEVE
→ SELECT / PROJECT
→ COMPOSE
→ SEMANTIC MATCH / REFLEX
→ SPECIALIZED COGNITION
→ GENERATE
→ THINK / SIMULATE / PROVE
→ HUMAN when required
```

Each transition has an explicit reason such as no suitable candidate, low calibrated coverage, unresolved novelty, missing evidence or authority/human requirement. Cascades are observable/replayable and may later be specialized under CX-22.

## 12. Relationship to other specs

- CX-02 provides durable observations and snapshot identity.
- CX-24 supplies semantic selection/retrieval primitives.
- CX-26 supplies select/project/compose artifact paths.
- CX-27 consumes bounded supervisor working sets.
- CX-13 owns runtime resource accounting.
- CX-19 defines protected intent/acceptance clauses that must remain pinned.
- CX-10 owns durable episode/history storage independent of active context.
- CX-22 may learn cheaper working-set or cascade policies after evidence.

## 13. Acceptance gates

**G28-protected-pin:** Intent authority ceilings, unresolved MUST/acceptance clauses, active protected rules and verifier receipts cannot be semantically dropped or compressed below their registered representation.

**G28-context-gc:** tool-call/result pruning preserves pair identity, durable provenance and retrieval/recompute references; no orphan result or fabricated summary may replace missing exact evidence.

**G28-recompute:** `DROP_RECOMPUTABLE` requires a valid recompute contract bound to snapshot/input identity, authority and side-effect class; mutable/effectful data is not assumed reproducible.

**G28-stale-reject:** async working-set/rule/skill decisions are applied only if the relevant intent/state/catalog/revision digests still match.

**G28-receipt:** every protected learned decision can produce the exact effective working-set receipt used for inference, including omissions/compressions and model/cache routing metadata.

**G28-auth-before-rank:** unauthorized, revoked or incompatible skills/rules/artifacts are excluded before semantic ranking and rechecked before use; recommendation never creates authorization.

**G28-fail-safe-rules:** semantic selection failure cannot remove mandatory rules or protected context; fallback behavior is explicit, deterministic and bounded rather than silently empty.

**G28-cascade-trace:** every escalation in a cognitive cascade records the failed/abstained prior strategy and cannot skip required authority/verification stages merely to reduce latency.

## 14. First build slice

Use a long coding episode with known relevant/irrelevant rules, several large tool results and a stable repository snapshot. Compare full-context, generic-summary and semantic-working-set paths on task quality, protected-clause retention, replayability, context size and latency/cost. Inject stale async decisions and non-recomputable external results. No context reduction is promoted until the protected invariants pass.


## v0.13 working-set self-application
Every working-set selection emits CX-29-compatible policy/revision/effective-input/outcome lineage. The working-set policy can be replayed and shadowed as a challenger target, while protected pins and no-effect receipts are emitted/enforced outside the candidate policy.
