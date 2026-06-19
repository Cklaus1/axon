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

### 3. What the substrate actually provides (and what's derived/greenfield)

> **Correction (post-review, code-verified 2026-06-19):** an earlier draft claimed the ledger "builds on the
> same schema the runtime already emits." That is **overstated**. The shipped provenance record
> (`axon-rt::Record`: `fn_name`/`score`/`ts_ms`/`input`) carries **no causal-parent edge and no per-event
> hash-chain**; `principal`/`effect_row` exist on `ai_call`/`agent_action` records **only**, not on the core
> adaptive record. So the *event log* is shipped, but the **causal graph + tamper-evident chain the ledger
> actually sells are DERIVED / greenfield.** The conceptual head start is real but thinner than first
> stated — which materially changes the cost-to-signal math used to rank R18 ahead of R16/R17 (§12 Q1).

The differentiator is that **Axon models decisions, not just events.** What is *shipped* vs what must be
*derived/built*:

| Company-ledger need | Shipped Axon primitive | Gap to productize |
|---|---|---|
| Company-ledger need | Shipped? | Reality |
|---|---|---|
| single append-only event timeline | ✅ shipped (log), ❌ causal | provenance log exists (`run_start`/`ai_call`/`agent_action`/`adaptive_return`), but **the core record has NO causal-parent edge** — the timeline's *causal* structure is **derived**, not emitted |
| reconstruct knowledge-state of a past decision | ⚠️ partial | `replay` (run-id + seed) is byte-identical re-execution of an **Axon run** — it is **NOT** `as-of` time-travel over ingested external facts. `as-of` is **greenfield** and shares zero code with replay; do not conflate them |
| typed effect/audit records, attributed | ⚠️ subset only | `principal` + `effect_row` (F3) are on `ai_call`/`agent_action` records **ONLY** — not the core adaptive record. Uniform attribution across all event types is **greenfield** |
| "what if we'd chosen Y" | ⚠️ scalar | `Counterfactual<T>` exists but over a fn input / recorded **scalar**, not over *ledger graph state* — the ledger counterfactual is **derived** |
| who did what, with what authority | ✅ value type | `Principal` + minting (R11) ship; mapping external actors to Principals is adapter work |
| durable typed state | ⚠️ value type only | `Store<T,C,L>` is a *value-type model*, not a scalable backing store — that store is **greenfield** (§5.1) |
| immutable, tamper-evident history | ❌ greenfield | `axh1:` SHA-256 hashes **modules / prompts**, NOT a **per-event hash-chain** — tamper-evidence over the event log is **Slice-2 greenfield**, not reuse |
| decision → outcome → learning loop | ✅ value types | `Goal`/`Feedback`/`Plan`/`Schedule` ship as the *typed decision objects* — the genuine differentiator vs event-only incumbents |

**Mental model: Datomic + provenance — but mostly the Datomic half is unbuilt.** The honest read: Axon ships
the *decision-typing value types* (the real differentiator) and an *event log*; the **causal-parent graph, the
per-event hash-chain, and `as-of` time-travel — the things that make it a "ledger" — are derived/greenfield**,
sharing little code with the shipped `replay`. `trace` is *proto*-proto-Datomic. The reused asset is the
decision *schema vocabulary*, not a working ledger.

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

Every record is a typed event keyed by `(principal, effect, causal-parent, content-hash, ts)`. **NB (per §3):
the runtime does *not* emit this shape today** — `causal-parent` and `content-hash` are added by the ingestion
layer; this is the target schema, not a reused one.

**The `<decision-id>` identity scheme (must be pinned before queries mean anything).** The headline queries key
on a "decision," but the product's whole claim is "model *decisions*, not events." Two options, decide
explicitly:
- **v1 cheap:** a *decision* ≡ a commit or an agent-session. Honest consequence: the differentiator vs `git`
  narrows sharply — `why <commit>` is close to `git show` + the transcript. The spike's H2 non-triviality bar
  (§ spike) must then be carried by `as-of`/counterfactual, not `why`.
- **v1 real:** a *decision* is a first-class object (an intent + the alternatives considered + the chosen
  option + its rationale + the principal), distinct from the commit that implements it — this is the actual
  "decisions not events" differentiator, and it is **greenfield** (the runtime has no such object). The spike
  should test whether such an object is even *recoverable* from a real source, not assume it.
*Default: state v1 = commit/session for the spike, but pre-register that a pass on `why` alone does NOT
validate the "decisions not events" thesis — only `as-of`/counterfactual or a recovered decision-object does.*

### 5. The real gap (what is NOT compiler work)

Honest enumeration so the "conceptual vs product" distinction stays visible:

1. **Event store** — NDJSON → an append-only, content-addressed, time-indexed store (event-sourcing / a log; Datomic/Kafka/embedded-store class). Scale, retention, indexing.
2. **Ingestion adapters** — typed connectors for Datadog/PostHog/Drive/Slack/git/Claude-Code/Codex, each mapping a source event into the `(principal, effect, causal-parent, hash, ts)` schema.
3. **Query / as-of / causal layer** — today there's replay + counterfactual, not general query. Needs "as-of" time-travel + causal-graph traversal.
4. **UI** — the timeline / decision-inspector (overlaps R16, but webview is fine here; this is not the GPU-UI use case).
5. **The unglamorous 99%** — multi-tenancy, access control (maps to Principal/capability — an *asset*), encryption, dedup, cost.
6. **Compliance & data-governance (a potential *fatal* blocker, not just work).** An **append-only, tamper-evident, "not opt-out-able" (I-13)** ledger collides head-on with **GDPR/CCPA right-to-erasure, retention limits, and legal-hold purges** — immutability and subject-deletion are in direct tension and need a designed mechanism (crypto-shredding / tombstoning / a PII side-store) that erases without breaking the hash-chain. Ingesting Slack/Drive/transcripts also **centralizes PII, secrets, and source code** in one high-value place → demands **classify-and-redact-on-ingest** (a Slice-1 acceptance criterion, not an afterthought) and a DPA/SOC2 story. **This can make a *technically validated* wedge unsellable to the exact enterprise beachhead** — so it is a gating input to the §12 Q1 wedge decision, not a later detail.

**Honest reuse accounting:** only the **decision-typing value-type vocabulary** is genuinely close to Axon's
existing strengths. The causal graph, hash-chain, `as-of` query, store, ingestion, UI, and compliance (items
1–6) are **greenfield product** — and §12 Q2 (buy storage) would *discard* the shipped NDLog, leaving the
reused asset = the schema vocabulary alone.

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

**Slice 0 (spike — prove the substrate generalizes AND the answer is worth paying for):**
> **The Slice-0 bar is defined by `R18-slice0-spike.md` (H1 + H2), NOT by this checklist alone.** An earlier
> version of this criterion said only "queryable by `asof`," which is *looser* than the spike's value bar (H2,
> the why/as-of/counterfactual answer + external willingness-to-pay) — and the looser bar must not be cited as
> a pass. Both documents name the **same** Slice-0 bar: the spike's H1 *and* H2 both hold.
- [ ] `ledger_ingests_external_events_into_typed_timeline` — a **genuinely external, not-pre-linked** source
      (see spike §3) maps into the target schema with a *measured* causal-edge precision/recall, not just "no
      schema violence." (H1)
- [ ] The spike's **H2** passes its external-jury willingness-to-pay + non-CI-outcome bars (spike §2/§5), or
      the spike's **KILL leaf** (spike §7) fires. A green H1 with an unproven/failed H2 is **not** a Slice-0 pass.

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
   one wedge. *Recommendation: do NOT open Slices 1–4 until one wedge is chosen.* The wedge decision must
   weigh **three** inputs, not just the spike's technical pass: (i) the spike's H1+H2 evidence; (ii)
   **compliance-deployability** (§5.6 — the immutability-vs-erasure + PII-aggregation problem can veto the
   product in enterprise procurement regardless of technical merit); (iii) **opportunity cost vs the
   sandbox-wedge (b)**, which is *prevention* (a painkiller bought *before* incidents) where R18 is *recall* (a
   tool wanted *after*, often unbudgeted). *A green spike authorizes running this Q1 comparison — not writing
   the PRD.* The "R18 and (b) are two faces of one bet" claim is itself an **untested hypothesis**: the spike
   (or buyer interviews) must test it, not assume it as decision rationale.
2. **(§5)** Build the store, or sit on an existing event-sourcing engine (Datomic/XTDB/Kafka/embedded)?
   *Default: sit on an existing engine.* **Honest consequence (do not skip):** buying storage means the
   shipped NDJSON provenance log is **discarded** and the decision-typing + counterfactual layer is
   re-implemented on a foreign engine — so the "builds on shipped substrate, cheaper than R16/R17" ranking
   premise (§12 Q1) **largely evaporates**; the only surviving reused asset is the schema *vocabulary*.
   Separately stress-test **defensibility**: a thin decision-typing + counterfactual layer over someone else's
   storage may be **"a feature, not a company"** — what stops a fast-follower with distribution from copying it
   once the category is proven? This is unresolved and load-bearing for the wedge decision.
3. **(§4)** Replace vs augment incumbents. *Default: augment — ingest, never replace; "rebuild" is the
   graveyard framing. The product is the causal decision LAYER across existing systems.*
4. **(relationship to R16/R17)** If R18 is the wedge, R16 (UI) downgrades to "a webview inspector is fine"
   and R17 (bare-metal OS) is clearly *not* the near-term "Axon OS" — the OS is the **decision ledger**, per
   §2.1. *Confirm this re-prioritization explicitly if Q1 resolves toward R18.*
