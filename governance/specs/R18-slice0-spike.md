# R18 Slice-0 Spike — does the provenance substrate generalize, and is the answer worth paying for?

**Spec ID:** `R18-slice0-spike` (the evidence experiment that gates `R18-provenance-ledger.md` §12 Q1 — the wedge decision)
**Status:** **GREENLIT (founder decision 2026-06-19) — blocked on two external inputs only.** Reshaped per the panel review. The autonomous parts (the importer harness, the queries, the competitive scan) can proceed now; the spike is **runnable end-to-end once the founder supplies: (1) an external, not-pre-linked Source A repo, and (2) ≥3 coding-agent-platform buyer-jury contacts.** Those two cannot be procured autonomously — they are the hand-off.
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

- **H1 — Technical (generalization, not self-fit):** *the target schema `(principal, effect, causal-parent,
  content-hash, ts, payload)` can be DERIVED from a real external source where the causal edges are not handed
  to it for free.* **Note (code-verified):** the runtime does **not** emit this shape — there is no
  causal-parent edge or hash-chain in the shipped records (see R18 §3), so "the schema already exists" is
  false; H1 tests whether it can be *built*, not *reused*. H1 must be measured on a **genuinely external,
  not-pre-linked** repo (no `Co-Authored-By` trailer) with a **causal-edge precision/recall** bar against a
  hand-labeled ground truth — *not* on this Axon repo, whose `Co-Authored-By` markers hand the schema its
  hardest field for free (that's the cheapest case, and therefore the *least* honest test of generalization).
- **H2 — Value:** *the longitudinal answer is compelling.* A `why`/`asof`/`whatif` query reconstructs a
  decision → rationale → **consequential outcome** chain that (a) a human could otherwise assemble only by
  manual archaeology across ≥3 systems, and (b) **≥3 external target-population buyers** (not the author) would
  pay for.

H1 is necessary; **H2 is the one that matters.** A clean mapping with a boring answer is a fail — and a clean
mapping *on data rigged to map cleanly* teaches nothing.

### 2. Pass/fail bar (pre-registered — fill the question in BEFORE building)

**Choose the question MECHANICALLY, not by taste (anti-cherry-pick).** Pre-register a selection rule, then
take what it returns: e.g. *"the 3 most recent commits touching ≥2 source files AND having a non-CI
consequential outcome; run `why` on all 3 and report the WORST."* Write the rule + the resulting SHAs here
**before any code**:

> _Selection rule (fill in):_ …
> _Resulting questions/SHAs (fill in):_ …

**Mandatory outcome constraint (pre-registered):** the chosen change MUST have a **non-CI consequential
outcome** — an `@[adaptive]`/`@[goal]` score trajectory, a metric move, or a later **revert/rollback**. A
`cargo test`/CI-green outcome is **disqualified**: a "decision → edit → CI passed" chain is exactly what
`git`+PR already give you, so it can pass H2 on a mechanism that fails the non-triviality bar below. With a
CI-only outcome the spike tests **H1 only**; H2 stays unproven.

**H1 passes iff:** events from the external source land in the schema, and the derived `causal-parent` edges
hit a pre-registered **precision/recall bar** against a hand-labeled ground truth (with any `Co-Authored-By`
trailer deliberately ignored). A confidently-*wrong* `why` is worse than no answer — so edge accuracy is
measured, not eyeballed. If a precision/recall bar can't fit the time-box (§8), H1 is honestly renamed
"schema fits Axon's own near-native data" and a pass is **not** read as generalization.

**H2 passes iff ALL FOUR hold** (§5 judgment session):
1. **Materiality:** the answer assembles, in one query, a causal chain that manual reconstruction needed **≥3
   separate sources + correlation by hand** to match.
2. **Non-triviality (raised floor):** the answer beats **not just `git log`+PR**, but `git`+PR + the agent's
   own raw session log + existing LLM-trace tooling (Langfuse/Helicone-class) + the 2–3 named incumbents from
   the competitive scan (§3). At least one element must be genuinely hard-to-recover across *all* of those.
3. **Willingness-to-pay (external jury, not the author):** **≥3 target-population judges** (a coding-agent
   platform PM / eng lead), shown the `why`/`asof` output **blind and unprimed**, give an **unprompted concrete
   pay answer** attached to an order-of-magnitude **price band** (pre-register the hypothesis: per-seat vs
   per-ingested-event vs platform-OEM). The author may NOT grade this. *(If ≥3 external judges aren't reachable
   in the time-box, see §8 — H2-WTP is demoted to a separate later test, not self-graded.)*
4. **Compliance-deployable (qualitative gate):** the judges (or a security-literate proxy) answer **yes** to
   "would your security/legal review permit ingesting these sources into one immutable store?" A `no` here is a
   real-world `fail` even if 1–3 pass (see R18 §5.6).

**Hard negative bar (anti-self-deception):** if the most impressive part of the demo is the *timeline view*
rather than an *answer you couldn't get before*, H2 **fails**. Pretty ≠ valuable. And the author's own "yes"
is **not** evidence — it is the exact self-deception this bar exists to catch, one level up.

### 3. The data source — TWO sources, because "cheapest" and "most honest" are in tension

The review's sharpest finding: dogfooding on *this* repo is the **cheapest but least honest** test of
generalization, because its `Co-Authored-By: Claude` commits hand the schema the exact field (the agent→diff
causal edge) that the runtime doesn't actually emit. So the spike uses **both**:

- **Source A — external, not-pre-linked (the real H1 test):** a non-Axon, **multi-contributor** repo with a
  coding-agent transcript but **no `Co-Authored-By` trailer**, where the agent→commit→outcome edge must be
  **inferred** (timestamp + touched-file overlap + content), then scored against a **hand-labeled ground
  truth** (precision/recall). This is what makes an H1 pass mean "generalizes."
- **Source B — this repo (convenience / near-native):** fine for building the importer and for an H2 demo, but
  an H1 pass here is labeled **"near-native fit," not generalization** (§1).
- **Outcome signal** — must be **non-CI/consequential** (§2): an `@[adaptive]`/`@[goal]` score trajectory
  (`axon trace --json`), a metric move, or a later revert. CI-green is disqualified.
- **Competitive scan (½ day, no code, a Slice-0 deliverable):** name **2–3 actual funded** agent-memory /
  coding-history products + LLM-trace tools (Langfuse/Helicone-class) and what each already answers for "why
  did this change happen + outcome." This sets the non-triviality floor (§2.2) and either substantiates or
  forces a downgrade of R18's unsourced timing claim ("AI-coding-agent storage companies are hitting this").

No external SaaS *connectors* (Datadog/PostHog/Slack ingestion) in the spike — that's Slice 3. The competitive
scan and buyer interviews are **research, not code** — explicitly in scope (§6).

### 4. Method (the throwaway build — target ≤ 3 days)

1. **Pre-register** (and commit *before* any code): the mechanical selection rule + resulting SHAs (§2), the
   non-CI outcome constraint, the causal-edge precision/recall bar + how ground truth is labeled, and the H2
   criteria. This is what stops the bar drifting to fit the result.
2. **Competitive scan** (½ day, no code) — produce the §3 2–3 named incumbents + what each answers; set the
   non-triviality floor; substantiate-or-downgrade the timing claim.
3. **Importer** (a script, ~a few hundred lines — *not* wired into the compiler): build the target schema
   **derivation** over **Source A (external, not-pre-linked)** primarily, Source B for the demo. Mapping:
   - agent session → a `Principal`; each agent edit/tool-use → an event (`effect`: fs/edit/ai_call).
   - each commit → an event; `causal-parent` = **inferred** from timestamp + touched-file overlap + content
     (on Source A the `Co-Authored-By` shortcut is **deliberately ignored**).
   - each outcome (score-trajectory / metric / revert) → an event; `causal-parent` = the commit.
   - `content-hash` per event — **new derivation** (the runtime emits none; not a reuse of `axh1:`).
4. **Measure H1** — compute causal-edge **precision/recall** of the inferred edges vs the hand-labeled ground
   truth; record it. (A confidently-wrong edge counts against, not toward.)
5. **Two/three queries** (hardcoded, no query language): `why <commit>`, `asof <ts>`, and — required, since
   §2.2 leans on it — one `whatif` via the shipped `Counterfactual<T>`.
6. **Run** on the pre-registered questions; report the WORST per the selection rule.
7. **Judge** (§5) — including the **external buyer jury**, scheduled in parallel (recruiting ≥3 judges is the
   long-pole; start it on day 0).

### 5. The judgment session (forced, binary, one sitting)

Two graders, deliberately split so the author can't score the market:

- **Author grades the OBJECTIVE legs only:** H1 (precision/recall vs ground truth, y/n at the bar), H2.1
  materiality (time/sources the manual reconstruction took), H2.2 non-triviality (vs the §3 incumbents). For
  these the author answers the pre-registered question the **manual** way first (open git, the transcript, the
  incumbents' tools — time it, count sources), then runs `why`/`asof`/`whatif`.
- **External jury grades H2.3 (willingness-to-pay):** **≥3 target-population judges** see the output **blind
  and unprimed** and give an unprompted concrete pay answer + price band. **The author may not grade WTP.**
- **Compliance gate (H2.4):** the jury (or a security-literate proxy) answers the §2.4 ingest-permission
  question.

Score binary, no half-marks, no "it has potential." Write the verdict in §7. **If the external jury can't be
assembled in the time-box, the spike does NOT self-grade WTP — it reports "H1 + H2.1/H2.2 only" and WTP moves
to a separate gate (§8).**

### 6. Scope — explicitly OUT (keeps it days, not quarters)

Building any of these in the spike is a process failure:
- ❌ a scalable / real event store (reuse NDJSON append)
- ❌ an ingestion-adapter framework (two one-shot imports — Source A + B — is fine; a *framework* is not)
- ❌ a query language (two/three hardcoded queries)
- ❌ any UI (CLI text output)
- ❌ multi-tenancy / access control / a compliance *implementation*
- ❌ Datadog / PostHog / Slack / Drive connectors
- ❌ any new Axon language feature or compiler change

**Explicitly IN scope (research, not code):** the ½-day competitive scan (§3), the buyer-jury interviews (§5),
and the hand-labeling of causal-edge ground truth (§4). These are the spike's *evidence* — not the trap §6
guards against.

### 7. Decision tree (what each outcome tells you — fill the verdict in after the run)

| H1 | H2 | What it means | Next move |
|---|---|---|---|
| pass | pass | the schema **generalizes** (measured) **and** an external jury would pay | **premise survived first contact — but this does NOT authorize the build.** Run the R18 §12 Q1 **wedge comparison** (defensibility vs a fast-follower, compliance-deployability §5.6, opportunity cost vs the sandbox wedge) *before* any PRD. A green spike buys the right to *compare*, not to build. |
| pass | fail (after one iteration) | schema generalizes but no external buyer pays for the answer | **KILL R18: close it, remove the `REQUIREMENTS.md` R18 row, consolidate focus on the sandbox wedge.** (Not "park" — a parked third front keeps eating attention; §12 Q1.) |
| fail | — | the causal edges can't be derived at the precision bar — the head-start is **weaker than R18 §3 (already corrected) claims**; R18 is materially more expensive | **reconsider R18**; do not write the PRD on a false premise |
| H1 only (no external jury reachable) | — | WTP could not be tested | spike certifies **H1 + author-non-triviality only**; **do not** self-grade WTP — gate it to a separate buyer test before the Q1 comparison |

> _Verdict (fill after running):_ H1 = (precision/recall ?) · H2 = (materiality ?/ non-triviality ?/ external-WTP ?/ compliance ?) · Decision = ?

### 8. Effort & time-box

- **Time-box: 3 days of *build*, hard stop** — but two added rigor items (the external buyer jury and the
  hand-labeled precision/recall ground truth) **may not fit 3 days**, and that tension is real, not ignorable:
  - **Buyer jury:** recruiting ≥3 target-population judges is the long pole — **start day 0, in parallel**. If
    they're not in hand by judgment day, do **not** self-grade WTP: report "H1 + non-triviality only" and gate
    WTP to a separate, slightly-later buyer test (per §5 / §7 row 4). Pick this path explicitly; don't assume
    judges materialize.
  - **Precision/recall ground truth:** hand-labeling true agent→commit→outcome edges on Source A is itself
    work. If it blows the box, either narrow Source A to a smaller window, or honestly rename H1 to
    "near-native fit" (§1) and record that generalization was **not** measured — never silently drop the bar.
- If the importer isn't producing answerable events by day 2, that *is* a signal about H1 — record it and stop.
- **Effort:** throwaway script + 2–3 query fns + a competitive scan + a buyer jury. Nothing ships; the
  *learnings* land in the R18 §12 Q1 wedge comparison (on pass) or the decision to KILL (on fail).
