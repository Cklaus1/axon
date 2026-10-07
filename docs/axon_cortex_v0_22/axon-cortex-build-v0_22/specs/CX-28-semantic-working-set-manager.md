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
authorization/privacy filters
→ exact deterministic filters
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

## v0.16 review amendment — Pinned context overflow

When protected pins exceed a model's effective input limit, choose another authorized representation/model, retrieve under an approved staged plan, or refuse. Never silently summarize mandatory clauses to fit. Protected invariants remain enforced outside the prompt, even when their authoritative records are not all rendered. Receipts distinguish a referenced object from text actually included in the effective input. Recompute is a new authorized action, not recovery by blindly repeating a historical side effect.

**G28-pin-overflow:** an oversized mandatory working set cannot produce a receipt falsely claiming that omitted pins were visible; unsafe truncation refuses or takes an explicit approved alternative without erasing the durable contract.

See `build/REVIEW_INVARIANTS.md` for the shared interpretation and `review/BUILD_ADVERSARIAL_REVIEW.md` for attack cases.

## v0.18 ACE amendment — ACE exact materialization and state reuse

P06 distinguishes observations, selected working set, actual effective input, residency and native state. No new ContextSnapshot store. Keep protected pins, exact identity/lease and approved recomputation. A prefix match/batch/resident model does not prove actual state reuse.

See the normative [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md) and [build guide](../build/ACE_INTEGRATION.md). These requirements extend this owner; no second ACE subsystem is introduced.

**G28-ace-reuse:** Receipts distinguish actual/opportunistic/rematerialized/none/unknown reuse, bind principal/project/engine/model/adapter/encoding/state/lease, and reject stale or cross-scope reuse; raw KV translation stays unsupported without its own profile.

## v0.20 amendment — schema decisions and lightweight Reflex specialization

A lightweight relevance classifier is an optional working-set experiment, not a prerequisite or proof that compaction disappears. Protect immutable instructions, mandatory context, evidence pins and exact source restoration independently of learned scores. Reject/drop decisions must be evaluated for lost task-critical context and whole-cascade outcomes; context-changing candidates are not passive same-input shadow. Use the [lightweight profile](../schemas/LIGHTWEIGHT_REFLEX_PROFILE.md) and retain incumbent fallback when input/domain/qualification is unsupported.

**G28-reflex-context-pilot:** An optional learned relevance treatment preserves mandatory pins, authoritative restoration and incumbent fallback, and measures critical omissions plus end-to-end outcomes rather than claiming elimination of compaction.

## v0.21 research integration amendment

This additive amendment is normative for the explicitly selected v0.21 profile. Existing authority and wire-format owners remain unchanged. The [research integration contract](../build/RESEARCH_INTEGRATION_V021.md) and [requirements matrix](../integration/RESEARCH_REQUIREMENTS.json) specify the complete profile and source-backed migration. Older versioned profiles remain separately identifiable; none of these declarations establishes runtime implementation.

### Acceptance gates added in v0.21

**G28-r21-context-originals:** Working context is a versioned projection of canonical original events; repeated compaction reconstructs from those originals rather than recursively summarizing prior projections.

**G28-r21-context-retention:** Projection lineage resolves within authorized retention policy; expired/deleted evidence yields explicit unavailability/tombstones and is not reconstructed from unauthorized caches or silently represented as complete.

**G28-r21-context-pins:** Context selection preserves required pins and tool-call/result dependency closure within the consuming-model token budget; an impossible pin set blocks rather than dropping obligations or sending an oversized context.

**G28-r21-context-recovery:** Prefix/state mismatch, unavailable originals, image/tool evidence loss and re-compaction are tested; recovery recomputes an authorized bounded projection or returns a typed refusal, never a misleading complete summary.

## v0.22 amendment — closed-loop Axon / Compute Fabric / MiCode integration

The [0.22 integration contract](../build/CLOSED_LOOP_INTEGRATION_V022.md) and [source-bound work packages](../build/WORK_PACKAGES_V022.md) extend this existing owner. They do not introduce a parallel authority, learning store, sandbox claim or schema reinterpretation. Original 0.21 research profiles remain available, but the bounded 0.22 pilot uses only its explicitly qualified dependency slice. The [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json) preserves ACF-01 M0-M2 obligations; the [MiCode consumer delta](../integration/MICODE_V022_CONSUMER_DELTA.md) binds actual peer work. All added runtime obligations are unexecuted proposals in this pack.

**G28-r22-workspace-not-context:** Canonical context, scoped WorkspaceSnapshot observations and durable WorkspaceVersion contents remain distinct identities joined by an explicit omission/freshness projection; a transcript or hash-only observation never becomes a bootable filesystem.

**G28-r22-context-provenance:** Both arms retain the canonical-history/working-set projection and critical pinned constraints; hidden evaluation data is never made available by retrieval, compaction or a branch cache.
