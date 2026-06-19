# R18 Slice-0 Spike — does the provenance substrate generalize, and is the answer worth paying for?

**Spec ID:** `R18-slice0-spike` (the evidence experiment that gates `R18-provenance-ledger.md` §12 Q1 — the wedge decision)
**Status:** Draft — ready to run
**Risk class:** Trivial (a throwaway experiment; nothing ships)
**Author / date:** cklaus, 2026-06-19

> **This is a spike, not a spec.** Its only job is to produce *evidence* that resolves "is R18 the wedge?"
> before any PRD / tech spec / build is written. It is deliberately throwaway: a one-shot import script + two
> hardcoded queries + one human judgment session. **Nothing here is production code.** If it tempts you to
> build a store, a connector framework, or a UI — stop; that's Slice 1+, and it only opens if this passes.

---

### 1. The two hypotheses under test (pre-registered)

A spike that can't fail teaches nothing. These are stated **before** building, with sharp bars, so the
go/no-go is honest, not post-hoc.

- **H1 — Technical:** *the existing Axon provenance schema survives contact with a real external source.* A
  coding-agent transcript + git history map cleanly into `(principal, effect, causal-parent, content-hash, ts,
  payload)` — the same shape the runtime already emits — **without schema violence** (no field that has no
  home; no event that can't be causally linked).
- **H2 — Value:** *the longitudinal answer is compelling.* A `why <change>` / `asof <t>` query reconstructs a
  decision → rationale → outcome chain that (a) a human could otherwise assemble only by manual archaeology
  across ≥3 systems, and (b) a target user would recognize as worth paying for.

H1 is necessary; **H2 is the one that matters.** A clean mapping with a boring answer is a fail.

### 2. Pass/fail bar (pre-registered — fill the question in BEFORE building)

**Pick one real, already-happened question now, and write it here before any code:**

> _Spike question (fill in):_ "Why did `<commit-sha / change>` happen — what did the agent decide, on what
> rationale, what was true at the time, and what was the outcome?"

**H1 passes iff:** every event from the source (each agent edit/tool-use, each commit, each outcome) lands in
the schema with a real `principal`, a real `effect`, and at least one valid `causal-parent` edge — and the
reviewer cannot name a source fact that the schema had nowhere to put.

**H2 passes iff ALL three hold**, judged in a single forced-binary session (§5):
1. **Materiality:** the ledger's `why` answer assembles, in one query, a causal chain that manual
   reconstruction needed **≥3 separate sources + correlation by hand** to match (e.g. transcript + git + CI).
2. **Non-triviality:** the answer is **not** something `git log` + reading the PR already gives you. At least
   one element must be genuinely hard-to-recover today — the *rationale at decision time*, the *knowledge-state*
   via `asof`, or a *counterfactual*.
3. **Willingness-to-pay:** the reviewer answers **yes** to "would a target user (a coding-agent platform / an
   eng org) pay for this answer?" — a real yes, not a polite one.

**Hard negative bar (anti-self-deception):** if the most impressive part of the demo is the *timeline view*
rather than an *answer you couldn't get before*, H2 **fails**. Pretty ≠ valuable.

### 3. The data source — dogfood on this repo (cheapest + most honest)

The beachhead *is this repository*: an AI-coding-agent-built project with the raw material already on disk.

- **Git history** — `git log` with diffs, SHAs, timestamps, authors. Note: recent commits already carry
  `Co-Authored-By: Claude …` — a **literal, pre-existing causal link** between an agent and a diff. Free signal.
- **Coding-agent transcripts** — Claude Code session logs under `~/.claude/projects/-home-cklaus-projects-axon/`
  (and the auto-memory dir). One or two sessions is enough.
- **Outcome signal** — the cheapest available: `cargo test` result / CI status for the relevant SHA, or the
  existing Axon provenance log (`axon trace --json`) if the change touched an `@[adaptive]`/`@[goal]` run.

No external SaaS (Datadog/PostHog/Slack) in the spike — that's Slice 3. One agent-source + git + one outcome
is the whole input.

### 4. Method (the throwaway build — target ≤ 3 days)

1. **Pre-register** the spike question (§2) and the H2 criteria. Commit that *before* writing the importer, so
   the bar can't drift to fit the result.
2. **Importer** (a script, ~a few hundred lines — *not* wired into the compiler): read git log + one transcript
   + one outcome; emit events into the existing provenance schema. Mapping:
   - agent session → a `Principal`; each agent edit/tool-use → an event (`effect`: fs/edit/ai_call).
   - each commit → an event; `causal-parent` = the session/turns that produced it (link by the
     `Co-Authored-By` marker + timestamp + touched-file overlap).
   - each outcome (test/CI/trace) → an event; `causal-parent` = the commit SHA.
   - `content-hash` per event reuses the lockfile `axh1:` SHA-256 discipline (append-only, tamper-evident).
3. **Two queries** over the merged append-only log (hardcoded, no query language):
   - `why <commit>` — walk `causal-parent` edges **back** to the agent rationale and **forward** to the outcome.
   - `asof <ts>` — filter events ≤ ts; render "what was known/true" at that moment.
   - *(optional, if cheap)* one `whatif` using the shipped `Counterfactual<T>` over a recorded score.
4. **Run** on the pre-registered question against real data.
5. **Judge** (§5).

### 5. The judgment session (forced, binary, one sitting)

A real person (you) does two things back-to-back:
- **(a)** answer the pre-registered question the **manual** way — open git, the transcript, memory, CI — and
  time it / note how many sources it took.
- **(b)** run `why`/`asof` and read the ledger's answer.

Then score, out loud, binary: **H1 (clean mapping y/n)**, and **H2's three criteria (materiality /
non-triviality / willingness-to-pay, each y/n)**. No half-marks, no "it has potential." Write the verdict in
§7 of this file.

### 6. Scope — explicitly OUT (keeps it days, not quarters)

Building any of these in the spike is a process failure:
- ❌ a scalable / real event store (reuse NDJSON append)
- ❌ an ingestion-adapter framework (one one-shot import for one source)
- ❌ a query language (two hardcoded queries)
- ❌ any UI (CLI text output)
- ❌ multi-tenancy / access control / compliance
- ❌ Datadog / PostHog / Slack / Drive connectors
- ❌ any new Axon language feature or compiler change

### 7. Decision tree (what each outcome tells you — fill the verdict in after the run)

| H1 | H2 | What it means | Next move |
|---|---|---|---|
| pass | pass | the substrate generalizes **and** the answer is valuable — **the wedge is real** | write the R18 **PRD** (customer/market/pricing), open Slice 1 (the coding-agent beachhead) |
| pass | fail | timeline generalizes but value isn't there *as built* — likely the *decision/counterfactual* layer, not the raw timeline, is the differentiator | one iteration on the query (add `whatif`/rationale depth); if still fail, **park R18**, refocus on the sandbox wedge |
| fail | — | the existing schema needs real rework — the "conceptual head start" is **weaker than claimed**; R18 is more expensive than R18-provenance-ledger.md assumes | **reconsider R18**; do not write the PRD on a false premise |

> _Verdict (fill after running):_ H1 = ? · H2 = (materiality ?/ non-triviality ?/ WTP ?) · Decision = ?

### 8. Effort & time-box

- **Time-box: 3 days, hard stop.** If the importer isn't producing answerable events by day 2, that *is* a
  signal about H1 (the mapping is harder than claimed) — record it and stop; don't grind.
- **Effort:** one throwaway script + two query fns + one judgment session. No tests-to-keep, no review gate —
  it's a spike, it gets deleted (its *learnings* land in the R18 PRD or the decision to park).
