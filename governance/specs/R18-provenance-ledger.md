# R18 — Provenance Ledger: the decision-memory substrate for AI-driven companies

**Spec ID:** `R18-provenance-ledger` (new requirement row; strategic. Productizes the existing provenance/replay/audit/counterfactual substrate. Ties to ROADMAP §2.1 "userland OS = abstractions, not privilege", §6 stdlib `Trace`/`AuditEvent`/`Counterfactual`/`Principal`/`Store`, §9.5 F2/F3)
**Status:** Draft — **strategic proposal; the §12 Q1 wedge decision gates whether any slice past 0 opens**

> **Correction (governance audit, 2026-07-31, REVISED same day after adversarial re-review): the gating
> frame above is STALE — but for narrower reasons than this note first claimed.** A recorded founder decision
> **does** exist: commit `0c3d132` (2026-06-19, "do both") **COMMITTED R17** and **GREENLIT the R18 wedge
> spike** in the same decision, updating the `REQUIREMENTS.md` R18 row status to "🟢 Wedge GREENLIT (founder
> 2026-06-19)" and authorizing the autonomous parts ("importer harness, queries, competitive scan") to
> proceed. `crates/axon-ledger` (landed 2026-06-20, commit `2323632`; now 6,368 lines, 63 passing tests) is
> the descendant of that *authorized* importer-harness-plus-queries work — an earlier version of this note
> wrongly called it a landing "without a recorded wedge decision." What actually survives audit:
> **(1) spike scope was exceeded** — the spike pre-registered a *throwaway* importer ("lives in `/tmp`",
> results doc; "an ingestion-adapter framework … is not [fine]", spike §6), but a permanent workspace crate
> landed with Slice-2–4 surface (as-of query, watch daemon, MCP/webhook/RBAC, multi-repo, GitHub Action);
> **(2) the buyer-jury/WTP gate (H2) never ran**, so the *final* Q1 wedge comparison remains unauthorized;
> **(3) status fields went stale** — REQUIREMENTS.md %complete (0) and its line-46 summary ("0% / spec
> drafted") contradict the row's own GREENLIT status column, and this spec's Status line misdescribed the
> decision state from 2026-06-19 onward. Separately, the **§12 Q1 "one wedge" frame was superseded** by the
> recorded "do both" decision (R17 built to ~90% while R18 opened) — see the Q1 correction note. This is
> recorded as **§12 Q5** (blocking: bless-or-unwind the beyond-spike surface + run-or-waive the jury gate),
> and the built crate does **not** satisfy the §9 Slice-1 acceptance criteria (see the §9 correction note).
> The kill-gate is **not** weakened: the H2/jury gate remains open and still gates *acceptance* of any slice
> past 0.
>
> **Addendum (ASI-trajectory review, 2026-07-31):** the re-baseline Q5 calls for must also fold in **Q6**
> (write-side identity is unauthenticated self-report by the party under audit — BLOCKING alongside Q5),
> **Q7** (the decision unit is degrading under long-horizon autonomy), **Q8** (the value moat against a capable
> model with raw source access), and **Q9** (`crates/axon-audit` already ships the "Slice-2 greenfield"
> hash-chain). The §4 attested/asserted split is the load-bearing change; §8 gains write-side adversarial
> criteria and §9 gains three Slice-1 tests. **No gate is relaxed by any of it** — the jury bar is tightened
> (§9 jury-validity caveat) and the tamper criterion is kept, merely re-baselined as an integration.
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

| Company-ledger need | Shipped? | Reality |
|---|---|---|
| single append-only event timeline | ✅ shipped (log), ❌ causal | provenance log exists (`run_start`/`ai_call`/`agent_action`/`adaptive_return`), but **the core record has NO causal-parent edge** — the timeline's *causal* structure is **derived**, not emitted |
| reconstruct knowledge-state of a past decision | ⚠️ partial | `replay` (run-id + seed) is byte-identical re-execution of an **Axon run** — it is **NOT** `as-of` time-travel over ingested external facts. `as-of` is **greenfield** and shares zero code with replay; do not conflate them |
| typed effect/audit records, attributed | ⚠️ subset only | `principal` + `effect_row` (F3) are on `ai_call`/`agent_action` records **ONLY** — not the core adaptive record. Uniform attribution across all event types is **greenfield** |
| "what if we'd chosen Y" | ⚠️ scalar | `Counterfactual<T>` exists but over a fn input / recorded **scalar**, not over *ledger graph state* — the ledger counterfactual is **derived** |
| who did what, with what authority | ✅ value type | `Principal` + minting (R11) ship; mapping external actors to Principals is adapter work |
| durable typed state | ⚠️ value type only | `Store<T,C,L>` is a *value-type model*, not a scalable backing store — that store is **greenfield** (§5.1) |
| immutable, tamper-evident history | ⚠️ **shipped elsewhere in-tree** (re-baselined 2026-07-31) | `axh1:` SHA-256 hashes **modules / prompts**, NOT a per-event chain — but **`crates/axon-audit` (landed 2026-06-28, after this spec was authored) already implements the missing primitive**: `LedgerEntry { seq, ts_ms, principal, effect, op, prev_hash, entry_hash }`, `compute_entry_hash` folding `prev_hash` in, and a `verify()` reporting `"tamper detected at seq {n}: prev_hash mismatch"`. That field set is essentially §4's target schema **minus `causal-parent` and the richer payload**. `axon-ledger/src/hash.rs::record_id` is a per-record content hash with **zero linkage** (grep for chain/verify/integrity across `axon-ledger/src` returns nothing). So Slice 2 is an **integration**, not greenfield — see §12 Q9 |
| decision → outcome → learning loop | ✅ value types | `Goal`/`Feedback`/`Plan`/`Schedule` ship as the *typed decision objects* — the genuine differentiator vs event-only incumbents |

**Mental model: Datomic + provenance — but mostly the Datomic half is unbuilt.** The honest read: Axon ships
the *decision-typing value types* (the real differentiator) and an *event log*; the **causal-parent graph and
`as-of` time-travel — the things that make it a "ledger" — are derived/greenfield**, sharing little code with
the shipped `replay`. (The **per-event hash-chain is the exception**, re-baselined 2026-07-31: `axon-audit`
ships it; the ledger simply does not use it yet.) `trace` is *proto*-proto-Datomic. The reused asset is the
decision *schema vocabulary* plus `axon-audit`'s chained-entry type, not a working ledger.

> **Trajectory note (ASI-trajectory review, 2026-07-31): the value of this substrate does NOT expire under a
> stronger-model trajectory — its threat model does.** The more code and decisions are machine-authored, the
> more valuable a causal decision record becomes; but this spec's threat model was written for a *careless*
> generator (one that produces noisy, incomplete records) rather than an *optimizing* one (one that reads this
> spec and `crates/axon-ledger`, and produces records that are locally faithful and globally exculpatory).
> Every fact the product sells — rationale, principal, causal edge — is derived on the **write** side from
> artifacts the audited agent itself produces and can rewrite. See the §4 attested/asserted split, the §8
> write-side adversarial criteria, and §12 Q6.

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

> **Schema correction (ASI-trajectory review, 2026-07-31) — split ATTESTED vs ASSERTED provenance.** The flat
> key above conflates two epistemically different classes of fact, and the product currently sells them at the
> same confidence:
> - **ATTESTED** = facts a party *other than the audited agent* signs or that are independently re-derivable:
>   git commit/tree oids, the runtime's own I-13 provenance-log records (which already carry
>   `principal`/`effect_row` per F3), capability grants, `axh1:` module hashes, deploy/metric-source responses.
> - **ASSERTED** = facts produced by the agent under audit: transcript prose, the `goal` string, inferred
>   causal edges, the session `summary`.
>
> **Every query output and MCP tool result must render the distinction** — a `why` answer that does not say
> which half of itself is self-report is exactly the "confidently-wrong `why`" the spike bars. This is the
> single highest-leverage change in the spec and composes with the evidence-digest requirement below and the
> §12 Q6 identity binding.
>
> **`content-hash` must commit to the EVIDENCE, not to the summary.** Shipped today, the session payload is
> `{session_id, file_path, start_ts, end_ts, files_touched, turn_count, goal(≤120 chars), summary}` and
> `hash.rs::record_id` hashes exactly that. `file_path` is a **pointer to a mutable local file**
> (`~/.claude/projects/*.jsonl`); the transcript's *contents* are never hashed and `query.rs` never re-reads it.
> So even once a prev-hash chain lands, the chain attests to a 120-character summary while the underlying
> rationale evidence stays unhashed and rewritable **by the same agent that produced it** — and any human or
> model who follows `file_path` to check the ledger's claim reads whatever is there *now*. Editing the
> transcript is strictly cheaper than attacking the append log. **Required:** hash the source artifact into the
> event payload (transcript-blob digest, git commit/tree oid, raw metric-source response), so the ledger can
> prove *the transcript it cites is the transcript that existed at ingest*. This also **relieves the §5.6
> immutability-vs-erasure tension**: hash the original, then redact or discard the plaintext — the digest
> survives erasure as the attestation.

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

> **Expiring assumption #1 (ASI-trajectory review, 2026-07-31): "a decision ≡ an agent-session" is being
> invalidated by long-horizon autonomy, and the recorded rationale is a 120-char first prompt.** This spec
> honestly pre-registers that the v1-cheap option *narrows* the differentiator vs git. The trajectory makes it
> worse than narrow. In `ingest/session.rs` a session record carries **one** `goal` = the first user message,
> run through `clean_goal_text` and truncated with `.chars().take(120)`, plus a `turn_count`. A modern
> autonomous session — **this repo's own `/loop` and `build-loop` skills, which the code already special-cases
> (`clean_goal_text` handles "the common pattern of `/loop` goals")** — runs hundreds of turns and makes many
> independent consequential decisions from one prompt, so goal→decision goes from ~1:1 to **1:N, with N growing
> with model capability**. And that first message is increasingly **machine-authored by an orchestrator** rather
> than typed by a person. So the product's headline unit of value is defined by the artifact shrinking fastest
> in information content relative to what actually happened.
> **Stated limit:** as written, v1 assumes a human authored the session goal and a human reviews the generated
> `why`. Both are load-bearing and both are eroding; the spec depends on them and must say so rather than imply
> them. **Required before H2 is graded:** (a) pre-register and measure a *consequential-decisions-per-session*
> statistic on a recent long-horizon autonomous transcript — if it is ≫ 1, the v1-cheap option does not merely
> narrow the differentiator, it **drops most of the signal**; (b) fix the human-attribution language in shipped
> tool descriptions — `mcp.rs` currently advertises `ledger_why` as showing *"the original goal the engineer
> typed"*, which the ledger cannot know; it must read *"the goal the session was started with"*. Direction of
> travel (§12 Q7): detect decision boundaries **intra-session** — on plan/goal changes, approvals, tool-policy
> changes, or on the `Plan`/`Feedback`/`Goal` typed objects §3 already names as the genuine differentiator.

> **Expiring assumption #2 (ASI-trajectory review, 2026-07-31): causal-edge inference assumes ~one agent
> session at a time, and its precision was measured where concurrency was ≈1.** `ingest/edge.rs` links
> session→commit by a `[session_start − 1h, session_end + 6h]` window (`saturating_sub(3_600_000)` /
> `+ 21_600_000`) plus file-tail overlap, and emits an edge for **every** qualifying (session, commit) pair.
> The `confidence` score (file-coverage ratio × time decay) is **never used as a filter** — it is read only for
> display (`query.rs:546`). The jury packet reports **precision 1.00 / recall 0.52** on 200-commit ground
> truth, and §9's own caveat records that this was measured on **trainloop, a founder-owned near-solo repo**.
> Precision 1.00 is substantially an artifact of near-zero session concurrency: with N agents / subagents /
> worktrees open on overlapping files inside the same 7-hour window — **already true in this repo** (parallel
> subagent workflows, background tasks, the recorded parallel-dev-on-main situation) — the heuristic degrades
> toward *"link every session that was open."* The spike's own bar says **"a confidently-wrong `why` is worse
> than no answer,"** yet nothing in the pipeline suppresses or even flags a low-confidence edge.
> **Required:** record edge provenance (`emitted` vs `inferred`) on every `AgentEdge` — the ASSERTED/ATTESTED
> split above applied to edges — and **stop inferring what can be emitted**: have the agent harness stamp its
> session id at write time (commit trailer / git note, or an edge event emitted by the harness), keeping
> inference as the fallback for un-instrumented sources. See the §9 Slice-1 concurrency criterion.

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
  > **Correction (ASI-trajectory review, 2026-07-31): "a reuse, not a change" overstates it in one important
  > direction.** The language's I-11 Principal is **compiler-enforced and unforgeable**; the ledger's is an
  > **unauthenticated self-asserted CLI string**. `rbac.rs`'s own docstring: caller identity comes from
  > `--as <email>` or the `AXON_PRINCIPAL` env var, defaulting to `"unknown"`, and *"when RBAC is disabled (no
  > rbac.json), all records are visible to everyone."* Worse on the write side, `ingest/session.rs` sets
  > `principal = engineer` from the `--engineer` flag, which defaults to `git config user.email` — so **an
  > autonomous agent's session is stamped with a human's identity**, and the ledger manufactures human
  > accountability for a machine decision. The invariant is *borrowed in name*; the enforcement is not
  > inherited. Do not cite I-11 as if the ledger's principal carried the language's guarantees. Binding this is
  > §12 Q6.

### 8. Test plan

Beachhead-scoped (the coding-agent timeline):

- [ ] **Integration:** ingest a real Claude Code / Codex session + the git diffs + a deploy + observability
      outcome; assert they land as causally-linked typed events in one timeline.
- [ ] **Journey:** `axon ledger why <change-id>` returns the agent-session rationale + the metric outcome it
      produced; `asof` reconstructs the repo+knowledge state at that point; `whatif` runs a counterfactual.
- [ ] **Property (SLICE 2, not beachhead — re-sliced 2026-07-31 to match §9/§11):** the ledger is
      append-only + content-addressed — a tampered historical event is detectable (per-event hash-chain;
      **adopt `crates/axon-audit`'s chained-entry type — that, not the lockfile `axh1:` module hashing, is the
      actual in-tree chain precedent** (corrected 2026-07-31)). *Consequence to keep visible: until Slice 2
      lands, the §1/§3 headline claim "immutable, tamper-evident history" is NOT met by the shipped store
      (`OpenOptions::append(true)`, no integrity verifier).*
- [ ] **Adversarial (READ side):** a Principal without read capability cannot query another Principal's events
      (I-11).

> **Test-plan gap (ASI-trajectory review, 2026-07-31): the only adversarial criterion in this spec is on the
> READ side, while every fact the product sells is produced on the WRITE side by the party under audit.**
> `store.rs` is a plain `OpenOptions::append(true)` NDJSON file under `~/.axon/ledger/`, writable by the same
> process; the `goal` is scraped from the agent's own transcript; the principal is a CLI string (§7). Against
> an optimizing generator that has read this spec and `axon-ledger`, what §2's "typed, sourced answer" actually
> delivers is **faithful transcription of the agent's own account of itself** — and `ledger_pre_deploy` ("flag
> unexplained commits before a deploy") becomes a gate an agent can satisfy **by writing a session file, not by
> behaving.** A careless generator yields noisy records; a capable one yields *exculpatory* ones. The following
> are required alongside the read-side bullet (graded in §9):

- [ ] **Adversarial (WRITE side, a):** an agent with fs + git write access **cannot forge or alter an ATTESTED
      event** (§4) undetected — the chain plus the evidence-digest must catch it.
- [ ] **Adversarial (WRITE side, b):** an agent **cannot elevate its own principal**. `--as` / `AXON_PRINCIPAL`
      / `--engineer` must be bound to something the agent cannot mint. Minimum bar: **distinct principal
      namespaces for human-attested vs process-asserted identity, and never defaulting an agent session to a
      human's email.**
- [ ] **Adversarial (WRITE side, c):** editing the cited transcript after ingest is **detectable** — i.e. the
      §4 evidence-digest requirement, tested, not just specified.

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

> **Slice-0 evidence caveats (audit, 2026-07-31 — carry these into any §12 Q1 citation of the results):**
> `R18-slice0-results.md` grades **H1 "PASS (measured)"**, but the recorded run **deviates from the
> pre-registered spike design**: (a) Source A was **trainloop** — a founder-owned, near-solo agent repo
> **with** `Co-Authored-By` trailers — not the pre-registered "non-Axon, multi-contributor repo with NO
> trailer"; (b) ground truth was the **trailer itself**, not hand labels (ignoring the trailer at inference
> time mitigates but the reported recall 0.52 inherits any trailer omissions); (c) the spike-§2
> pre-registration blanks (selection rule / SHAs, numeric precision-recall bar) were **never filled in**, so
> "PASS" was declared against no stated bar — per the spike's own fallback, this reads as
> "near-native-fit-plus," not "generalizes." Additionally, **spike §4 steps 5–6 were skipped**: no `whatif`
> query was ever run (though §2.2's H2 non-triviality leans on it) and the demo commit was not selected by
> the pre-registered worst-of-N mechanical rule (a single apparently hand-picked commit is presented as "The
> Demo" in the jury packet). **Before the buyer jury is graded:** run the missing `whatif`, re-run `why` on a
> mechanically-selected set reporting the WORST, and amend the results doc — or downgrade the H2.2 claim in
> both the results doc and the jury packet.

> **Jury-validity caveat (ASI-trajectory review, 2026-07-31 — also a prerequisite to grading H2): the jury is
> being asked to grade fluent narrative assembled from text the audited agent wrote.** The spike anticipates
> the "pretty timeline" hard-negative bar, but not the failure mode that matters on this trajectory:
> distinguishing a **correct** causal chain from a merely **plausible** one is precisely the judgment humans do
> worst, and *the more fluent the underlying generator, the wider that gap.* Concretely: **recall is 0.52**
> (jury packet line 19), so roughly **half** of the real session→commit edges are absent from anything the jury
> sees, and `confidence` exists on every edge but is never surfaced as a filter or caveat.
> **Required before the jury is graded (which §12 Q5 still gates):**
> (a) show each judge **at least one known-wrong or known-incomplete `why`** alongside the good one and record
> whether they can tell them apart — that single measurement tells you whether human sign-off on ledger answers
> means anything at agent scale, and it is the empirical test of the stated limit recorded in §4;
> (b) surface **edge confidence and edge coverage** ("N of M commits in this window have linked sessions") in
> `why` / `ledger_why` output;
> (c) run the pre-registered worst-of-N selection, as the caveat above already requires.
> *This tightens the gate; it does not weaken it. A jury that cannot separate correct from plausible is not a
> pass.*

**Slice 1 (the coding-agent beachhead — the real v1 gate):**

> **Correction (audit, 2026-07-31): the shipped `crates/axon-ledger` does NOT meet this bar.** None of the
> three named acceptance tests exist anywhere in the tree; the crate has **no** whatif/counterfactual code at
> all, and its "append-only" is `OpenOptions::append(true)` with a per-record SHA-256 content id — **no
> prev-hash chaining, so tampering detects nothing.** These criteria remain OPEN; the crate's "Slice 1"
> commit title does not close them. Note also the tamper test was **mis-sliced here**: §3 (line "immutable,
> tamper-evident history") and the §11 slice table both place the hash-chain in **Slice 2** — it is
> re-sliced below accordingly (moved, not dropped).
- [ ] `ledger_why_links_agent_session_to_outcome` — `why <change-id>` returns rationale + causal chain +
      metric outcome across agent-session ↔ diff ↔ deploy ↔ observability.
- [ ] `ledger_whatif_counterfactual_over_ledger_state` passes. *(No counterfactual code exists in the shipped
      crate as of 2026-07-31 despite §4 listing `axon ledger whatif` as a headline query.)*
- [ ] All Slice-1 acceptance tests run under a repo gate. **Honest current state (2026-07-31): `gate.sh`
      only clippy-checks the crate (allowlist line, joined 2026-07-31); NO gate stage runs its tests** — every
      `gate.sh` test stage is `-p axon-core`, so the crate's 63 unit tests run only under a bare `cargo test`
      no gate script executes. The criterion: add a `cargo test -p axon-ledger` stage to `gate.sh`, and land
      the Slice-1 acceptance tests under that stage.
- [ ] `ledger_ingest_classifies_and_redacts` — transcript/PII/secret content is classified and redacted at
      ingest, per §5 item 6 ("a Slice-1 acceptance criterion, not an afterthought"). **The shipped crate
      currently FAILS this: it ingests full Claude Code session transcripts into `~/.axon/ledger/events.ndjson`
      with zero classify/redact code (the GDPR `prune` command addresses retention, not ingest-time
      classification) — the §5.6 PII/secrets-centralization risk is live in shipped code.**

*Added 2026-07-31 (ASI-trajectory review) — these three are Slice-1, not deferrable to Slice 2, because each
guards a claim the Slice-1 queries already make:*
- [ ] `ledger_edge_precision_under_concurrent_sessions` — causal-edge precision measured against a
      **CONCURRENT-sessions fixture** (k overlapping sessions touching shared files inside one 7-hour window),
      with a **pre-registered numeric precision floor** — not the near-solo trainloop corpus where precision
      1.00 was observed. Additionally: `why` must **suppress or explicitly label** edges below a confidence
      threshold rather than silently rendering them (`confidence` is currently display-only, `query.rs:546`),
      and every edge must carry `emitted` vs `inferred` provenance (§4).
- [ ] `ledger_renders_attested_vs_asserted` — every `why`/`asof`/MCP result distinguishes the two classes of
      fact (§4). A `why` answer that presents self-reported rationale at the same confidence as a git oid fails
      this criterion.
- [ ] `ledger_cites_hashed_evidence` — the event payload commits to a **digest of the source artifact**
      (transcript blob, git commit/tree oid, raw metric-source response), not merely to a `file_path` pointer
      and a 120-char summary; altering the cited transcript after ingest is detectable (§4, §8 write-side c).
      Composes with `ledger_ingest_classifies_and_redacts`: hash first, then redact.

**Slice 2 (store graduation) — re-sliced from Slice 1 per §3/§11:**
- [ ] `ledger_append_only_tamper_detectable` passes — a **per-event hash-chain** (each record commits to its
      predecessor), not just a per-record content hash; a tampered historical event is detectable.
      > **Re-baselined 2026-07-31 (ASI-trajectory review): this is an INTEGRATION, not "the real infra."**
      > `crates/axon-audit` (2026-06-28) already ships the chained-entry type, `compute_entry_hash` folding
      > `prev_hash` in, and a `verify()` that reports the tampered `seq`. Its field set is §4's target schema
      > minus `causal-parent` and the richer payload. **The criterion is therefore: either adopt that type
      > inside `axon-ledger/src/store.rs` — a small enough change to land in Slice 1 — or state explicitly in
      > this spec why the ledger's payload/causal-parent shape does not fit and what is genuinely new.** See
      > §12 Q9. Tamper-evidence matters *more* under a capable generator, not less: it is the one property
      > that distinguishes a record **of** an optimizing agent's work from that agent's **account of** it.
- [ ] **Scale floor (added 2026-07-31, per §10):** ≥ 10⁶ events with p95 query < 1s, and **incremental** edge
      inference (over records newer than the last edge) rather than a full re-scan.

### 10. Performance budget

N/A at spec stage. (At product scale: ingestion throughput + query latency budgets are Slice-1 artifacts.)

> **Expiring assumption #3 (ASI-trajectory review, 2026-07-31): the scaling variable is agent-events per repo
> per day — which tracks model throughput, not headcount.** Deferring scale to "a Slice-1 artifact" prices it
> against a human-paced volume curve. The shipped store is that assumption made concrete:
> `store.rs::all()` reads and JSON-parses the **entire** `events.ndjson` on every access;
> `find_by_id` / `find_by_effect` / `find_by_repo` / `find_by_payload_field` each call it; `query.rs` composes
> several such scans per query; `prune` rewrites the whole file; and `infer_edges` is an **O(sessions ×
> commits)** pass over full in-memory vectors, **re-run on every `ledger_refresh`** — which is exposed as an
> MCP tool an agent calls *at the start of a session*. A beachhead validated on a 200-commit single-owner
> corpus will never surface this. **State the variable as agent-events/repo/day (not seats)** and carry the
> concrete floor now recorded in the §9 Slice-2 criteria, so the wedge is not validated at toy volume.
> **Cheap interim (no architecture change):** index by `effect`/`sha`/`session` on load, and make `infer_edges`
> incremental over records newer than the last edge.

### 11. Rollout & rollback

~~This is a **doc-only strategic commit** — zero code.~~ **Corrected 2026-07-31: no longer true** —
`crates/axon-ledger` (Slice-1-class code plus parts of Slices 2–4's surface: as-of query, watch daemon,
MCP/webhook/RBAC) landed 2026-06-20 onward without this spec being updated; see the Status-line correction
and §12 Q5. The intended shape held in one respect: it ships as a *separate product surface*
(`axon ledger ...`), not wired into the language; the hosted/compiler build is untouched. The slice table
below remains the *acceptance* structure, but "cheap and reversible" must now be read against
already-integrated code:

| Slice | Deliverable | Note |
|---|---|---|
| **0 — spike** | one external source (coding-agent transcript + git) → typed timeline → `asof` query | proves the substrate generalizes beyond Axon runs; cheap; high signal |
| **1 — coding-agent beachhead** | agent-session ↔ diff ↔ deploy ↔ metrics unified; `why`/`whatif`/`diff`/`rollback-trace` | the wedge: win "agent decision memory" first |
| **2 — store graduation** | NDJSON → append-only content-addressed time-indexed store; scale | the real infra — **but the hash-chain half is an integration of `crates/axon-audit`, not greenfield (re-baselined 2026-07-31, §12 Q9); scale floor now pinned in §9/§10** |
| **3 — broaden ingestion** | Datadog/PostHog/Drive/Slack adapters | generalize to the business |
| **4 — UI + multi-tenant** | timeline/decision-inspector UI (webview is fine); Principal-scoped access | product hardening |

### 12. Open questions

1. **(strategic — BLOCKS opening past Slice 0; THE decision)** Is R18 *the* Axon product wedge? It now
   competes for focus with: (a) the language + ASI-safety core, (b) **"capability-sandbox for AI code"** (the
   prior recommended value wedge — `examples/flagship/`), (c) R16 GPU UI / "Axon platform", (d) R17 bare-metal
   "Axon OS". R18 and (b) are *the same bet from two faces* — "Axon as the trustworthy substrate **under** AI
   agents" (b = sandbox the agent's *actions*; R18 = remember the agent's *decisions*). They may compose into
   one wedge. *Recommendation: do NOT open Slices 1–4 until one wedge is chosen.* The wedge decision must
   weigh **three** inputs, not just the spike's technical pass: (i) the spike's H1+H2 evidence — **carrying
   the Slice-0 evidence caveats recorded in §9 (H1 measured off-design on trainloop with trailer-as-ground-
   truth and no pre-registered bar; whatif never run)**; (ii)
   **compliance-deployability** (§5 item 6 — the immutability-vs-erasure + PII-aggregation problem can veto the
   product in enterprise procurement regardless of technical merit); (iii) **opportunity cost vs the
   sandbox-wedge (b)**, which is *prevention* (a painkiller bought *before* incidents) where R18 is *recall* (a
   tool wanted *after*, often unbudgeted). *A green spike authorizes running this Q1 comparison — not writing
   the PRD.* The "R18 and (b) are two faces of one bet" claim is itself an **untested hypothesis**: the spike
   (or buyer interviews) must test it, not assume it as decision rationale.
   > **Correction (audit, 2026-07-31): this single-wedge frame was superseded in practice** by the recorded
   > 2026-06-19 founder "do both" decision (`0c3d132`): R17 was COMMITTED (and built to ~90%) *in the same
   > commit* that greenlit the R18 wedge spike — i.e. multiple fronts were opened simultaneously, contradicting
   > this question's "do NOT open Slices 1–4 until one wedge is chosen" premise and Q4's conditional. The
   > founder must either **re-affirm single-wedge discipline** (and reconcile the R17 commitment against it) or
   > **rewrite Q1/Q4 to reflect the actual multi-front posture**. The final wedge *comparison* (jury-gated)
   > remains open either way.
   > **Second input added 2026-07-31 (ASI-trajectory review): Q8 below is a fourth gating input to Q1.** If the
   > only part of the answer that survives a capable model with raw source access is the **attested** half
   > (§4), then Q1 moves measurably toward wedge **(b)** — and the "two faces of one bet" hypothesis gets its
   > first real test rather than staying decision rationale.
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
5. **(BLOCKING — opened 2026-07-31, governance audit; REVISED same day) Authorized spike tooling exceeded
   spike scope, the jury gate never ran, and status docs went stale.** An earlier version of this question
   claimed the crate landed "with no wedge-comparison decision … and no governance-doc update anywhere in
   `governance/`" — **false**: the founder's `0c3d132` (2026-06-19) greenlit the wedge spike, updated the
   `REQUIREMENTS.md` R18 row status, and authorized the importer harness + queries; `R18-slice0-spike.md`
   records the same, and `R18-slice0-results.md` (fbb9f3f, 2026-06-19) records built importer code and query
   runs. The overbroad "every R18 status document claimed zero code / 0%" is narrowed to: **the %complete
   fields and prose summaries** (REQUIREMENTS.md %complete = 0 and its "0% / spec drafted" summary line —
   which contradicts the same file's GREENLIT row status — plus this spec's Status line and
   `GOVERNANCE_STATUS.md` "Draft") stayed stale for ~6 weeks. What the founder must actually decide is
   **narrower**: (a) **bless or unwind the beyond-spike surface** — the crate is a *permanent workspace
   member* carrying Slice-2–4 surface (as-of, watch daemon, MCP/webhook/RBAC, multi-repo, GitHub Action),
   where the spike pre-registered a throwaway `/tmp` importer and its §6 explicitly excluded an
   ingestion-adapter framework (the results doc's "/tmp throwaway" description vs. the workspace crate is an
   unreconciled discrepancy to name in the re-baseline); and (b) **run or explicitly waive the buyer-jury/WTP
   gate (H2)** — the final Q1 wedge comparison is still unauthorized without it. Either way, re-baseline this
   spec + the `REQUIREMENTS.md` %complete/summary + `GOVERNANCE_STATUS.md` against the built crate. **Until
   then: the crate's existence is NOT evidence for Q1** (shipped code must not launder the wedge decision),
   the crate does not meet the §9 Slice-1 acceptance bar (see the §9 correction), and the spike's KILL leaf
   must be re-read honestly — "KILL R18" now means removing integrated code, not deleting a spec row.
   > **Scope note (2026-07-31): Q6–Q9 below are all fixable inside this still-open re-baseline** — they are
   > amendments to a spec that has not yet passed its gate, not new fronts. Q6 + Q9 + the §4 evidence-digest
   > requirement **compose into one coherent change** (split attested vs asserted, chain the attested half,
   > hash the evidence rather than the summary), which is also the strongest available answer to Q8.

6. **(BLOCKING for any slice past 0 — opened 2026-07-31, ASI-trajectory review) How is write-side identity
   bound to something the audited agent cannot mint?** Today it is not bound at all: `--engineer` defaults to
   `git config user.email` (stamping an agent session with a human's identity), and `--as` / `AXON_PRINCIPAL`
   are unauthenticated self-asserted strings that default to `"unknown"` — while `store.rs` is an append-mode
   NDJSON file writable by the same process (§7, §8). Until this is answered, the ledger **manufactures human
   accountability for machine decisions**, and `ledger_pre_deploy` is a gate satisfiable by writing a session
   file. *Minimum answer (already pinned as §8 write-side criteria): distinct principal namespaces for
   human-attested vs process-asserted identity; never default an agent session to a human's email.* The
   stronger answer — signing attested events with a key the agent's own capability set cannot reach — is the
   natural composition point with wedge (b). **Do not close Q1 while Q6 is open**: a ledger whose attested
   half is real is a different, and better, wedge than one whose value is narrative recall.
7. **(§4 — opened 2026-07-31, ASI-trajectory review) Does the decision boundary move intra-session, and what
   emits it?** The v1 "decision ≡ commit or agent-session" identity gives one 120-char `goal` per hundreds of
   turns and is degrading with model capability (§4, expiring assumption #1). Candidate answer: emit a decision
   event on plan/goal changes, approvals, and tool-policy changes, or key off the `Plan` / `Feedback` / `Goal`
   typed objects §3 already names as the genuine differentiator — which would make the ledger's decision unit
   *Axon-native* rather than git-derived. **Measure before deciding:** the consequential-decisions-per-session
   statistic pre-registered in §4.
8. **(strategic, feeds Q1 — opened 2026-07-31, ASI-trajectory review) Does the value survive a capable model
   with raw access to the same sources?** The spike's non-triviality floor (§2.2) compares against
   git + PR + the agent's own raw session log + Langfuse/Helicone-class tooling + 2–3 named incumbents, and its
   materiality bar prices manual reconstruction across "≥3 separate sources correlated **BY HAND**" as
   expensive. Both are stated relative to *human archaeology and today's tools*. The cheapest substitute over
   the next 1–3 years is **not a product**: an agent given read access to the repo, the transcripts, and the
   metrics — **the exact inputs `axon ledger ingest` consumes** — answering `why` on demand and doing that
   correlation. The competitive scan does not list this arm, so H2 could pass against a comparison set that is
   already the wrong one. **Required: add "a capable agent with raw access to the same sources" as an explicit
   arm of the §2.2 comparison, and pre-register which part of the answer survives it.** The plausible survivor
   is *not* narrative recall (a model reconstructs that) but **attested, non-reconstructible facts**: what was
   signed, what capabilities were granted, what was actually known at time T, what cannot be re-derived after
   the fact. That is the same attested/asserted split as Q6 — and it is why Q6 is the value moat, not merely a
   security fix.
9. **(§3/§9/§11 — opened 2026-07-31, ASI-trajectory review) Adopt `crates/axon-audit`'s chained entry inside
   `axon-ledger/src/store.rs`, or state what is genuinely new?** The spec calls the per-event hash-chain
   Slice-2 greenfield; it landed in-tree on 2026-06-28, nine days after this spec was authored, as
   `LedgerEntry { seq, ts_ms, principal, effect, op, prev_hash, entry_hash }` plus a tamper-reporting
   `verify()` — essentially §4's schema minus `causal-parent` and the richer payload. *Default: adopt it,
   converting "the real infra" into a small integration that can land in Slice 1.* If the richer
   payload/causal-parent shape genuinely does not fit, **say so in §3 and enumerate what is new** rather than
   leaving the row marked greenfield. Answering this is the cheapest available step toward Q6 and Q8.
