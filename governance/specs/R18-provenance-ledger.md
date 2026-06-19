# R18 — Provenance Ledger: the decision-memory substrate for AI-driven companies

**Spec ID:** `R18-provenance-ledger` (new requirement row; strategic. Productizes the existing provenance/replay/audit/counterfactual substrate. Ties to ROADMAP §2.1 "userland OS = abstractions, not privilege", §6 stdlib `Trace`/`AuditEvent`/`Counterfactual`/`Principal`/`Store`, §9.5 F2/F3)
**Status:** Draft — **strategic proposal; the §12 Q1 wedge decision gates whether any slice past 0 opens**
**Risk class:** Structural (a product direction, not just a feature; reframes what "Axon OS" means)
**Author / date:** cklaus, 2026-06-19

> **One-line thesis:** Axon already built, as compiler internals, a single-program version of the
> "event-sourced company" data foundation — provenance log + deterministic replay + typed audit +
> counterfactual. R18 asks whether to **productize that substrate**: a typed, causal, replayable *decision
> ledger* that sits **across** existing systems (Datadog / PostHog / Drive / Slack / git / AI-agent chats),
> not as a replacement, answering the longitudinal questions none of them can — *why did we decide X, what did
> we know then, what changed, what was rolled back, what if we'd chosen Y.* **Beachhead: the AI-coding-agent
> timeline.** This doc is the strategic frame + the mapping onto shipped Axon primitives, not a build order.

---

### 1. Motivation

Companies run on siloed systems — observability (Datadog), product metrics (PostHog), files (Drive),
comms (Slack), code (git), and now AI-agent transcripts (Claude Code / Codex). Each has its own schema,
retention, and no causal links across them, so the **longitudinal** questions are unanswerable: you cannot
reconstruct the *knowledge-state that produced a decision*, diff the *business state* across time, or run a
counterfactual on a past choice. Diffs are local to each system; there is no global rollback. AI-coding-agent
storage companies are hitting this at the code layer (agent memory: what changed and *why*); it generalizes to
the whole business. The incumbents log **events**; nobody models **decisions-with-provenance** — the rationale,
the knowledge at decision-time, the outcome, the counterfactual.

**The Axon-specific motivation:** Axon's ASI thesis *already is* this primitive set, at the scale of one
program (§3). The provenance log (I-13), deterministic replay (F2), typed `AuditEvent` (F3),
`Counterfactual<T>`, `Principal`, and `Store<T,C,L>` are a working prototype of the company data foundation.
R18 is the question of whether the **substrate is Axon's product**, with the language as one privileged, *safe*
way to author the agents that run on it.

### 2. Requirement link

Opens a **new `REQUIREMENTS.md` row R18** (platform-vision / product bucket). It does **not** add a language
feature; it proposes elevating the runtime provenance substrate to a product. It advances ROADMAP §2.1
(the "userland OS" whose OS-ness is its abstractions — agent/goal/principal/effect/store/**proof** — applied
here to a *company*) and passes the §0 discipline test: it is the **provenance + audit substrate that
Containment and Goal-directedness require to be trustworthy.** Acceptance (full vision): *a company's code
changes, deploys, metrics, observability, docs, comms, and agent reasoning live in one retrievable, causal,
replayable timeline, and a user can ask "why / what-did-we-know / what-changed / what-was-rolled-back / what-if"
and get a typed, sourced answer.* v1 acceptance is the **coding-agent beachhead** (§9).

> **Hard truth up front: this is a category-defining product, and the "rebuild every company" framing is a
> graveyard thesis.** The generic "single source of truth / data lake / company knowledge graph" ambition has
> killed many companies; adoption friction is brutal (nobody rips out Slack/Datadog). **Axon's head start is
> conceptual, not product** — the 99% that's hard (storage at scale, ingestion connectors, retrieval, UI,
> compliance, multi-tenancy) is *not* compiler work. And this is now a **third/fourth platform front**
> alongside the language, R16 (UI), and R17 (bare-metal). The scarce resource is **focus**, not vision. R18 is
> only worth opening past Slice 0 if it is chosen as *the* wedge (§12 Q1).

### 3. The substrate already exists (the mapping onto shipped Axon primitives)

The differentiator is that **Axon models decisions, not just events.** What's already built, and what each
becomes in the company-ledger framing:

| Company-ledger need | Shipped Axon primitive | Gap to productize |
|---|---|---|
| single append-only decision timeline | provenance log (I-13): `run_start`, `ai_call` (prompt-hash/tier/model/cost/mode), `agent_action` (who/effect/caps/**principal**), `adaptive_return` (score/input/src) | NDJSON file → a real append-only, content-addressed, time-indexed store |
| reconstruct the exact knowledge-state of a past decision | deterministic **replay** (`axon trace --replay`, run-id + seed → byte-identical re-execution; F2) | replay is per-Axon-run; generalize the "as-of" reconstruction to ingested external events |
| typed effect/audit records, attributed | **`AuditEvent`** + `Trace` stdlib (§6); F3 principal + effect_row on every record | schema unification across external sources |
| "what if we'd chosen Y" | **`Counterfactual<T>`** + `CfSet` value types (Phase 14+) | counterfactual over *ledger* state, not just a fn input |
| who did what, with what authority | **`Principal`** + capability minting (R11) | map external actors (humans, agents, services) to Principals |
| durable typed state with consistency + lifetime | **`Store<T, Consistency, Lifetime>`** (§6) | the ledger *is* a Store; needs a scalable backing |
| immutable, tamper-evident, diffable history | content-addressing (`axh1:` SHA-256 lockfile + content-addressed TCB manifest) | extend from modules to arbitrary events ("git for company state") |
| the decision → outcome → learning loop | **`Goal` / `Feedback` / `Plan` / `Schedule`** stdlib | these are the *typed decision objects* incumbents lack |

**Mental model: Datomic + provenance.** Immutable time-indexed facts, "as-of" queries, retrievable history —
plus Axon's causal/decision typing and replay. Axon's `trace` + content-addressing is **proto-Datomic**; the
missing pieces are the store, ingestion, and query layers (§5), none of which are compiler work.

### 4. Surface (what the user/operator interacts with)

Not `.ax` syntax — this is a *substrate + query surface*. Sketch (subject to §12):

```
# Ingest from existing systems into the typed ledger (adapters; don't replace the source)
axon ledger ingest --source datadog|posthog|drive|slack|git|claude-code|codex --since <t>

# The longitudinal queries the incumbents cannot answer:
axon ledger why    <decision-id>        # rationale + knowledge-state at decision time
axon ledger asof   <timestamp> <entity> # reconstruct what was true/known then
axon ledger diff   <t1> <t2>            # what changed in business state across the window
axon ledger rollback-trace <change-id>  # the causal chain a rollback touched
axon ledger whatif <decision-id> --alt <counterfactual>   # Counterfactual<T> over ledger state
```

Every record is a typed event keyed by `(principal, effect, causal-parent, content-hash, ts)` — the same
schema the Axon runtime already emits, with ingestion adapters mapping external events into it.

### 5. The real gap (what is NOT compiler work)

Honest enumeration so the "conceptual vs product" distinction stays visible:

1. **Event store** — NDJSON → an append-only, content-addressed, time-indexed store (event-sourcing / a log; Datomic/Kafka/embedded-store class). Scale, retention, indexing.
2. **Ingestion adapters** — typed connectors for Datadog/PostHog/Drive/Slack/git/Claude-Code/Codex, each mapping a source event into the `(principal, effect, causal-parent, hash, ts)` schema.
3. **Query / as-of / causal layer** — today there's replay + counterfactual, not general query. Needs "as-of" time-travel + causal-graph traversal.
4. **UI** — the timeline / decision-inspector (overlaps R16, but webview is fine here; this is not the GPU-UI use case).
5. **The unglamorous 99%** — multi-tenancy, access control (maps to Principal/capability — an *asset*), compliance/retention, encryption, dedup, cost.

**Only items 3 and the schema are close to Axon's existing strengths. 1, 2, 4, 5 are greenfield product.**

### 6. Error codes

N/A at spec stage (no language change). Ingestion/query error taxonomy is a Slice-1 design artifact.

### 7. Invariants touched

- **None of the language invariants change.** R18 productizes the runtime substrate; it does not alter the
  pipeline, type system, or safety model.
- **I-10 (determinism is available)** and **I-13 (provenance not opt-out-able)** are the *load-bearing assets*
  this product is built on — preserved and elevated, not changed.
- **I-11 (capability boundary)** becomes the access-control model for the ledger (Principal-scoped reads) — a
  reuse, not a change.

### 8. Test plan

Beachhead-scoped (the coding-agent timeline):

- [ ] **Integration:** ingest a real Claude Code / Codex session + the git diffs + a deploy + observability
      outcome; assert they land as causally-linked typed events in one timeline.
- [ ] **Journey:** `axon ledger why <change-id>` returns the agent-session rationale + the metric outcome it
      produced; `asof` reconstructs the repo+knowledge state at that point; `whatif` runs a counterfactual.
- [ ] **Property:** the ledger is append-only + content-addressed — a tampered historical event is detectable
      (reuse the lockfile `axh1:` hashing discipline).
- [ ] **Adversarial:** a Principal without read capability cannot query another Principal's events (I-11).

### 9. Acceptance criteria (per slice)

**Slice 0 (spike — prove the substrate generalizes):**
- [ ] `ledger_ingests_external_events_into_typed_timeline` — a non-Axon event source (a coding-agent
      transcript + git log) maps into the existing provenance schema and is queryable by `asof`.

**Slice 1 (the coding-agent beachhead — the real v1 gate):**
- [ ] `ledger_why_links_agent_session_to_outcome` — `why <change-id>` returns rationale + causal chain +
      metric outcome across agent-session ↔ diff ↔ deploy ↔ observability.
- [ ] `ledger_whatif_counterfactual_over_ledger_state` passes.
- [ ] `ledger_append_only_tamper_detectable` passes.

### 10. Performance budget

N/A at spec stage. (At product scale: ingestion throughput + query latency budgets are Slice-1 artifacts.)

### 11. Rollout & rollback

This is a **doc-only strategic commit** — zero code. If pursued, it ships as a *separate product surface*
(`axon ledger ...`), not wired into the language; the hosted/compiler build is untouched. Sliced so a spike is
cheap and reversible:

| Slice | Deliverable | Note |
|---|---|---|
| **0 — spike** | one external source (coding-agent transcript + git) → typed timeline → `asof` query | proves the substrate generalizes beyond Axon runs; cheap; high signal |
| **1 — coding-agent beachhead** | agent-session ↔ diff ↔ deploy ↔ metrics unified; `why`/`whatif`/`diff`/`rollback-trace` | the wedge: win "agent decision memory" first |
| **2 — store graduation** | NDJSON → append-only content-addressed time-indexed store; scale | the real infra |
| **3 — broaden ingestion** | Datadog/PostHog/Drive/Slack adapters | generalize to the business |
| **4 — UI + multi-tenant** | timeline/decision-inspector UI (webview is fine); Principal-scoped access | product hardening |

### 12. Open questions

1. **(strategic — BLOCKS opening past Slice 0; THE decision)** Is R18 *the* Axon product wedge? It now
   competes for focus with: (a) the language + ASI-safety core, (b) **"capability-sandbox for AI code"** (the
   prior recommended value wedge — `examples/flagship/`), (c) R16 GPU UI / "Axon platform", (d) R17 bare-metal
   "Axon OS". R18 and (b) are *the same bet from two faces* — "Axon as the trustworthy substrate **under** AI
   agents" (b = sandbox the agent's *actions*; R18 = remember the agent's *decisions*). They may compose into
   one wedge. *Recommendation: do NOT open Slices 1–4 until one wedge is chosen; land Slice 0 as a cheap spike
   to test whether the substrate generalizes, and decide with that data.*
2. **(§5)** Build the store, or sit on an existing event-sourcing engine (Datomic/XTDB/Kafka/embedded)?
   *Default: sit on an existing engine for the beachhead; the differentiator is the decision-typing + replay +
   counterfactual layer on top, not the storage.*
3. **(§4)** Replace vs augment incumbents. *Default: augment — ingest, never replace; "rebuild" is the
   graveyard framing. The product is the causal decision LAYER across existing systems.*
4. **(relationship to R16/R17)** If R18 is the wedge, R16 (UI) downgrades to "a webview inspector is fine"
   and R17 (bare-metal OS) is clearly *not* the near-term "Axon OS" — the OS is the **decision ledger**, per
   §2.1. *Confirm this re-prioritization explicitly if Q1 resolves toward R18.*
