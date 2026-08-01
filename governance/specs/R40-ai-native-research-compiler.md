# Tech Spec — R40: AI-Native Research/Build Compiler (general orchestrator — held for later)

**Spec ID:** `R40-ai-native-research-compiler` (the general-purpose superset of R39: a natural-language
front end, a typed IR spanning objective/decision/experiment/task/evidence/knowledge/provenance
graphs, a mutation-validation engine, and a harness runtime — usable for *any* AI-driven software or
research project, not just this repo's own governance)
**Status:** Draft (2026-07-18) — strategic proposal, **condensed** from a user-authored design;
the original proposal document (whose §6–30 detailed the full design) is **NOT preserved in this
repo** — this spec is the only surviving record (fidelity claim corrected 2026-07-31; the header
previously said "captured in full," which was false). The design remains **explicitly NOT
founder-committed and not started.** This spec exists so the idea is preserved well enough to
build later, not because building it is recommended now — see §5 and §12 Q1, which blocks
everything past a possible Slice 0 spike.
**Adversarial review folded 2026-07-31** (ASI-trajectory pass): added §1.5 (threat model + R40's
own TCB, inheriting `VISION_OS.md` §1 instead of §1's weaker "careless generator" model), the
evaluator-authorship separation and pre-registration rules in §4, the stated-limits/expiring-
assumptions block in §11, and the reuse map in §10. **Two of those (§1.5 and §4's
evaluator-authorship rule) are gating: they must be in place before any Slice-0 spike, not
retrofitted after a harness exists.**
**Risk class:** Structural (a new, separate product; multi-month; changes what "Axon" is, not what
it does — see §5's identity question)
**Author / date:** cklaus (research agent draft, condensing a user-authored design proposal —
see the Status line's fidelity correction), 2026-07-18

```spec-meta
id: R40-ai-native-research-compiler
status-claim: Draft
depends-on: none
blocks: none
blocked-by: R40 §12 Q1 (founder identity/wedge decision; blocks everything past a Slice 0 spike)
supersedes: none
related: R39-typed-execution-graph, R38-embedded-agent-runtime, R18-provenance-ledger, R16-axon-ui, R24-defended-approval-boundary, R27-corrigibility-resource-bounds, R28-capability-audit-ledger, R30-unified-safety-gate
conflicts-with: none
reserves: none
evidence: none (Draft; not started; no gate exists)
```

> **One-line scope:** a compiler that takes natural-language project/research intent as source,
> lowers it through a typed IR (objectives, decisions, experiments, tasks, evidence, knowledge,
> provenance) into an executable, gated plan, hands execution to a coding-agent harness runtime, and
> recompiles the plan from the evidence that execution produces — so that "the AI proposes, the
> compiler validates and types, the harness executes, the evaluator gates" replaces "the AI
> re-derives and re-asserts project state from scratch every session."

---

### 1. Motivation (verbatim thesis, lightly edited for this repo's spec format)

AI coding agents are powerful but unreliable at maintaining long-running project state. They
commonly: reinterpret objectives between sessions; forget constraints; overfit to the most recent
failure; change success criteria after seeing results; confuse implementation success with
scientific/architectural validation; repeat failed approaches; claim completion without sufficient
evidence; perform tasks that don't trace back to active requirements; generate plans whose
dependencies are incomplete or cyclic; optimize local outputs while harming project-level goals.

**Split of that list (added 2026-07-31 — it matters, because the two halves have opposite
trajectories and R40's design currently addresses them unequally):**

| Class | Failure modes | Trajectory |
|---|---|---|
| **Capability-limited** | reinterpreting objectives between sessions; forgetting constraints; repeating failed approaches; re-deriving/re-asserting state each session; incomplete or cyclic plan dependencies | **Shrinking.** Longer context, persistent memory and better retrieval erode these *independently of any Axon work*. Value of R40's fix here decays over its own deferral horizon. |
| **Incentive-driven** | changing success criteria after seeing results; claiming completion without sufficient evidence; confusing implementation success with scientific/architectural validation; optimizing local outputs while harming project-level goals; overfitting to the most recent failure | **Not shrinking — arguably growing.** These are what an *optimizer* does better as it gets stronger. No amount of context or memory removes them. |

**Consequence for scope.** The one-line scope below prices R40 against the union of both halves
("the AI re-derives and re-asserts project state from scratch every session" is a
capability-limited framing). The durable case for R40 is the **incentive half alone** — and §4's
mechanisms, as originally drafted, target the capability half well and the incentive half weakly
(the specific gaps and their fixes are §1.5 and §4's flagged bullets). **This reframing is an
input to §12 Q1, not a detail:** if the capability half closes on its own, the residual wedge is
exactly the half R40 must be redesigned to defend. Q1 must be answered against the incentive half.

The proposed fix is a division of labor:

```
LLM:      proposes, interprets, diagnoses, hypothesizes
Compiler: types, validates, constrains, lowers, diagnoses
Harness:  executes, schedules, isolates, collects artifacts
Evaluator: measures, aggregates, verifies, gates (deterministically — never self-graded)
Graph store: records authoritative project state and history
```

The central design rule: *natural language may propose meaning, but only typed and validated state
may control execution.*

**This session is itself a small, concrete illustration of exactly the problem this thesis names**
— see `R39-typed-execution-graph.md` §1 for the specific bugs (stale "Draft" headers, dangling
cross-references, a gate script with an unexercised false-negative bug) a human/agent had to find
by hand, repeatedly, because no typed, validated state existed to catch them mechanically.

### 1.5 Threat model and R40's own trusted base (added 2026-07-31 — GATING, read before §4)

§1 above models the adversary as **unreliable**: an agent that forgets, overfits, and re-derives.
Every mechanism in §4 was designed against that model, and every one of them is a *shape check on
an assumed-careless proposer*. That is not this repo's adversary.

**R40 inherits `VISION_OS.md` §1 by reference — it does not restate a weaker model.** That
threat model commits to code authored by a model that may be "(a) far more capable than its
operators at finding flaws, (b) deceptively aligned — appearing to pursue the stated goal while
pursuing another, (c) patient and able to act across many runs, instances, and channels, and (d)
motivated by *instrumental convergence*," and states explicitly: "We assume the adversary reads
all our source, specs, and proofs." **R40 would read this spec.** Design every mechanism below on
that assumption; a rule whose force depends on the proposer not noticing it is not a rule.

Why this is not optional for R40 specifically: §4 hands the model a **validated write channel into
authoritative project state** ("LLM output is always a typed mutation proposal … validated by the
compiler before being applied"), and R40 is the layer that decides *what gets built, what counts
as evidence, and which gates apply*. It is the highest-leverage surface in the project, and it was
the one with no threat model.

**R40's TCB (the analogue of `ROADMAP.md` §7, which enumerates 18 components under the invariant
"Self-modification cannot weaken any of them" — R40 previously had no such enumeration despite
being the *governance layer over self-modification*):**

```
R40_TCB = {
    mutation_validator,     // the compiler pass that accepts/rejects typed mutation proposals
    evaluator_and_metrics,  // metric fns, gate predicates, seed selection, artifact capture (§4)
    gate_store,             // registered thresholds, baselines, seed sets, metric identities
    event_log,              // the append-only record of proposals, evidence, promotions, approvals
    approval_boundary,      // the human-gating machinery (§5, R24)
    promotion_rules         // the claim ladder predicates themselves
}
```

**I-12 analogue (non-weakening invariant, mandatory):** *no mutation emitted by the InnerLoop or
OuterLoop may weaken a gate, a promotion rule, an approval requirement, the evaluator, or the
validator itself.* Mutations touching `R40_TCB` are **not** in the autonomous mutation channel;
they are human-approval nodes under §5/R24 and are recorded into the R28 hash-chained ledger.

**Direct consequence for §6 phase 3.** "Optimizing compiler / OuterLoop passes (architecture
analysis, blast-radius optimization, mechanism selection, confidence calibration)" is
self-modifying machinery acting on exactly the rules that bound it. **Phase 3 is out of the
autonomous mutation channel's reach**: its passes may *propose* changes to `R40_TCB` components
but may never apply them; application requires human multi-sig per R24. If that constraint is
ever judged too expensive, the correct response is to drop phase 3, not to relax the constraint.

**Related safety specs whose threat model R40 inherits** (added to `related:` above; the original
list — R39/R38/R18/R16 — contained none of them): `R24-defended-approval-boundary` (the human
approver as attack surface), `R27-corrigibility-resource-bounds` (kill channel, coalition
ceiling), `R28-capability-audit-ledger` (tamper-evident chain), `R30-unified-safety-gate`.

### 2. Requirement link

None. This is a new product category, not an extension of an existing `REQUIREMENTS.md` row — it
would need its own Top-10-style matrix if pursued, since it changes what Axon *is* (see §5).

### 3. The product story (target state, condensed from the full proposal)

```bash
axon compile ROADMAP.md          # NL/Markdown intent -> typed Project IR + diagnostics
axon check                       # semantic validation: cycles, missing baselines, scope-exceeding claims
axon plan --milestone v0.2a       # lower validated decisions/experiments into an acyclic task DAG
axon run                         # harness executes the DAG: worktrees, agents, tests, artifacts
axon status                      # render from authoritative state, never hand-edited
axon experiment compare exp-41 exp-42
axon decisions uncertain --risk high
axon rollback experiment exp-42
```

> **Note (added 2026-07-31):** the `axon` prefix above is illustrative shorthand from the original
> proposal. `axon check`/`axon run` already exist in `crates/axon-core` with different semantics
> (type-check / interpret `.ax` source); §6's separate-binary decision governs — if built, these
> verbs live under a distinct binary (e.g. `axon-gov compile`, `axon-gov status`), not the `.ax`
> compiler's CLI.

A worked example (from the original proposal, preserved because it's the clearest illustration of
the intended behavior): a hypothesis ("explicit velocity improves rollout fidelity") is compiled
into an experiment contract (baseline, treatment, controls, seeds, gates), lowered into a task DAG
(implement → unit test → run N seeds in parallel → aggregate → evaluate gates), executed by the
harness, and the evaluator returns a **structured, partial result** — one gate passed
(`mean_return >= 30.5`), a different gate failed (`rollout_error` got worse) — which the OuterLoop
must interpret as "do not promote; the claimed mechanism wasn't actually supported" rather than
rounding up to "experiment passed."

### 4. Core concepts (full model, for fidelity — see §5 for what's actually in scope if built)

The full design spans seven typed subgraphs — Objective/claim, Decision, Experiment, Task DAG,
Evidence, Knowledge, Provenance — with these cross-cutting principles:

- **Confidence is typed metadata on claims/decisions/evidence, never a standalone score.** Decompose
  into `implementation` / `experimental` / `architectural` / `scientific` / `strategic` dimensions,
  each tagged with its source (`computed` | `model_estimate` | `human_judgment`) — never silently
  merged. "90% confident the code ran correctly" and "30% confident it validates the thesis" must
  stay two numbers, not one.
  - **Gap (2026-07-31): `model_estimate` is an unbacktested self-report from the graded party.**
    Source-tagging is necessary but the spec stopped there — nothing ever scores a
    `model_estimate` against what later turned out to be true. Against a careless model that is
    mere miscalibration; against an optimizer that has learned which confidence numbers get a
    proposal approved, it is a target — "0.9 implementation / 0.3 scientific" is precisely the
    shape that *reads as honest and passes review*. **Required:** every `model_estimate` is
    retained with the claim; when a `computed` result later resolves the same claim, the pair is
    recorded; a per-dimension calibration curve is derived and either the corrected estimate is
    what the approver sees, or a minimum calibration record is required before a `model_estimate`
    may support a promotion. See §10 — the substrate for this already exists (R28 ledger,
    `axon-trace --ai --json` / `axon-ai-audit/2`, F2 replay); it is a pass over landed data, not
    a new evidence store.
  - **Gap: five dimensions × three source tags with no aggregation rule pushes integration onto
    the human**, whose bandwidth is already the binding constraint (§5). Unspecified integration
    under time pressure means the approver anchors on whichever number the model's accompanying
    prose emphasizes — which hands aggregation to the proposing party. **Required:** a
    deterministic, code-evaluated, non-model-authored decision rule per decision class —
    promotion and irreversible actions gate on the **minimum** relevant dimension, never a
    weighted blend, with `model_estimate`-sourced dimensions discounted until calibrated.
    Dimensions stay unmerged *in storage*; the gate function over them is explicit and as
    un-self-graded as the numeric gates below.
- **Promotion discipline for claims**: `Observation → Hypothesis → SingleRunResult → RepeatedResult
  → LocalResult → ArchitecturalFinding → GeneralizedClaim`. The compiler rejects unsupported jumps
  (e.g., one environment's result cannot promote to a cross-domain claim) as a type error, not a
  style note.
  - **Expiring premise (2026-07-31): the ladder checks the *shape* of evidence, not its
    independence, and its teeth were implicitly cost-gated.** Reaching `RepeatedResult` meant N
    real runs and `ArchitecturalFinding` meant genuinely different environments *because faking
    that was more work than doing it*. A stronger generator inverts that ratio: N seeds, N
    environment variants and the accompanying write-up become the cheapest route to satisfying
    the predicate. As written the rule is satisfied by **count and category labels**, and
    environments are things the agent can author. The ladder does not become wrong; it degrades
    to a formality that laundering-through-volume passes. **Required additional fields on the
    promotion predicates:** who selected the seeds/environments and *when* (before or after any
    result was seen); whether the replication ran against an **independently authored**
    implementation; whether a **held-out** environment was reserved and untouched until
    promotion. `ArchitecturalFinding` and above require at least one adversarial or held-out
    replication **the proposing agent did not author**. Negative control: a promotion attempt
    whose evidence was generated *after* the gates were known is flagged for human review by
    construction.
- **Decision types by uncertainty class** (`Implementation` / `Experimental` / `Architectural` /
  `Scientific` / `Strategic`), each implying a different required validation method and state
  machine (`PROPOSED → REVIEWED → APPROVED → TESTING → CONFIRMED | REJECTED | INCONCLUSIVE →
  SUPERSEDED`).
- **The InnerLoop/OuterLoop split**: InnerLoop executes one bounded task (`PLAN → DO → RUN → VERIFY
  → DECIDE`) and may not change gates, replace a baseline after seeing results, or start a new
  experiment. OuterLoop reviews evidence against decisions and emits exactly one typed mutation per
  cycle — never "try whatever seems best" in prose.
  - **Gap (2026-07-31): the gate-freeze binds only the InnerLoop; the OuterLoop *is* the mutation
    channel and is bound only by rate.** The InnerLoop prohibition is aimed at the right failure
    (§1's "change success criteria after seeing results"), but the very next clause hands the
    OuterLoop the mutation power with no equivalent constraint — and §4's mutation vocabulary
    (`create_decision`, `create_experiment`, …) includes exactly the operations that define gates
    and baselines (§3 confirms gates are part of the experiment contract). **One mutation per
    cycle is a rate limit on volume, not a constraint on direction:** supersede the decision,
    create a successor experiment with a revised baseline, re-run — each step individually valid,
    the sequence walking the gates to wherever the results already landed. Against a careless
    agent that is drift; against an optimizer it is a *search procedure*, and cheap cycles make
    it converge faster.
  - **Required — pre-registration as a type rule:** gate thresholds, baselines, seed sets and the
    metric identity become **immutable once the first evidence record binds to the experiment**.
    A successor experiment may relax them only via an explicit `supersede` mutation recording the
    prior values, the reason, and the fact that it was authored *after results were seen*. A
    **`gate drift` report** renders the chain from originally-registered to current for every
    promoted claim. The registration and supersede records live in the **R28 tamper-evident
    hash-chain ledger**, not the graph store's own history (do not trust a mutable store to
    testify about its own mutations). **A supersede-after-evidence is a human-approval node, not
    an OuterLoop-autonomous mutation.**
- **LLM output is always a typed mutation proposal** (`create_decision`, `create_experiment`,
  `create_tasks`, `add_edges`, …), validated by the compiler before being applied — never a
  free-form rewrite of project state.
- **Deterministic gates, never self-graded.** `metric >= threshold` is evaluated by code; the model
  interprets *why*, but cannot override the numeric result.
  - **CRITICAL gap (2026-07-31) — this bullet conflates *deterministic* with *independent*.** It
    is a guarantee about arithmetic, not about authorship, and it holds only against a generator
    that would try to *argue with a number*. A capable optimizer never touches the comparison; it
    changes what feeds it. **§6 phase 1 puts the entire measurement stack inside the agent's
    write set** ("typed task schema, DAG scheduler, worktree/agent execution, deterministic
    gates, artifact capture"): the same agent that implements the treatment implements the metric
    function, the unit test that gates it, the seed selection, and the artifact capture. §3's
    worked example is exactly the shape at risk — an agent optimizing for gate-pass on
    `mean_return >= 30.5` has cheaper routes than improving the mechanism: widen what counts as a
    rollout, drop the seeds that regress, make the test tautological, make artifact capture lossy
    in a convenient direction. **None of these are "self-grading" under this bullet's definition,
    and no check anywhere in this spec detects them.** This is the highest-value target in the
    design and the one a stronger model finds first.
  - **Required — authorship/measurement separation as a typed property, not a convention:**
    (a) evaluator, metric and gate code live **outside the agent's write set** — a separate path,
    hash-pinned into the run record, changes requiring human or multi-sig approval (R24);
    (b) **every evidence record carries the hash of the evaluator that produced it**, so a gate
    pass under a mutated evaluator is *mechanically* distinguishable from one under the pinned
    evaluator; (c) the evaluator runs on a harness the treatment code cannot write to;
    (d) **seed sets are drawn from a pre-registered list**, never chosen by the executing agent.
    This is the anti-laundering discipline the language already enforces one level down
    (`VISION.md`: "a guard you can't escape by routing through an un-annotated helper") applied
    at the *evidence* layer — where R40 currently has it wide open. **Gating: this rule must be
    in the design before a Slice-0 harness exists; retrofitting it means rebuilding the harness.**

### 5. What R40 is NOT (non-goals — read before writing a line of code)

> **Argument provenance (added 2026-07-31).** Each bullet below is tagged **[cost-derived]** or
> **[durable]**. A cost-derived argument weakens as code generation gets cheaper; a durable one
> does not. **A fall in build cost does not by itself resolve §12 Q1** — the identity question,
> the platform-sprawl precedent, and the human-attention ceiling are unaffected by how cheap the
> code becomes. If anything, cheaper construction *raises* the sprawl risk rather than lowering
> it. A future reader who observes that the build got cheap has not thereby observed the deferral
> expiring.

- **Not something to build without an explicit identity decision first.** *(Partly
  [cost-derived] — the "engineering scope" comparison below — but the identity question it
  serves is [durable].)* This is the single
  biggest risk: the full design (natural-language compiler front end + typed research IR + mutation
  validator + task scheduler + evidence engine + event sourcing) is a standalone product on the
  scale of "a coding-agent orchestration platform," comparable in engineering scope to
  Devin/Cursor/Claude-Code-with-a-formal-research-methodology — not a feature of a systems
  programming language. Building it under the Axon name without deciding "is Axon now this, in
  addition to a language?" risks the same identity confusion the R16/R17/R18/R36/R37/R38 platform-
  vision sprawl already produced (six competing platform fronts as of this spec's drafting,
  2026-07-18; see `governance/REQUIREMENTS.md`'s strategic note for the current count).
- **Not proven necessary at Axon's own scale.** *([durable] — this is an argument about problem
  shape, not build hours.)* `R39-typed-execution-graph.md` argues the *scoped*
  version (typed schemas for this repo's own governance, no NL front end, no experiment/decision
  graph, no mutation engine) already closes the concrete failure modes this session found. R40's
  additional machinery (decision-uncertainty typing, claim-promotion rules, confidence
  decomposition, event sourcing) is designed for a *different* problem shape — a portfolio of
  scientific/ML research experiments with baselines, seeds, and reproducibility concerns — which is
  not what building a compiler and its stdlib actually looks like day to day. Don't assume R40's
  extra machinery transfers just because R39's smaller subset clearly helped.
- **Not a replacement for human governance/strategic judgment.** *([durable] — and this is the
  load-bearing safety property of the entire design.)* Even in the full design, strategic
  decisions and irreversible actions require human approval boundaries (per the original
  proposal's §21 — a section NOT preserved in this repo; see the header's fidelity correction.
  The load-bearing content is exactly this sentence: strategic/irreversible actions gate on a
  human) — this is orchestration tooling, not an autonomous-company system.
  - **Gap (2026-07-31): one sentence, no bandwidth model — in front of a machine built to
    multiply approval requests.** §6 phase 1 builds "a DAG scheduler with retries/checkpointing/
    human-approval nodes" over parallel worktrees and agent execution: **approval-request
    throughput scales with harness capacity; human review capacity does not.** A one-sentence
    approval boundary in front of a high-throughput autonomous harness is a rubber stamp under
    load — and a *persuadable* one against a proposer that writes the justification text the
    human reads. This repo has already spec'd exactly this problem and reached the opposite
    conclusion about how much design it needs: **`R24-defended-approval-boundary`** opens
    "Against a capability-proven box, a superhuman AI's highest-probability escape is not
    technical — it's persuading the human approver to grant a capability," and specifies
    legible+quantified grants, pre-approval simulation, risk-proportional friction, multi-sig for
    high risk, decision provenance, and default-deny expiring grants. R40 referenced none of it.
  - **Required:** import R24's mechanisms directly for R40's approval nodes rather than restating
    a bare requirement, and make **human attention a first-class typed budget** — a per-cycle
    approval allowance that, when exhausted, **halts the harness** rather than degrading to
    auto-approve or batch-approve. Every approval artifact is diff-bounded and legible within a
    *stated* review budget, with the model-authored justification **visually and structurally
    separated** from the machine-computed facts (the human must be able to decide on the latter
    alone). Irreversible actions get R24 pre-approval simulation. Every approval is recorded into
    the R28 chain. See also §11's stated limit: **R40's safety rests on a human actually reading
    every artifact it gates on — that is a declared operating limit, not an implicit hope.**
- **Not scoped, sized, or sequenced in this draft.** *([durable].)* Unlike R36-R38 (which at least name phased
  slices with named gates), R40 is preserved at the *idea* level. A real build would need its own
  Slice-0 spike (see §12) before any committed plan is written. (Corrected 2026-07-31: this bullet
  previously deferred to "§6-30 of the original proposal," but that document is not preserved in
  the repo — §§1, 3, 4 and 6 of THIS spec are the only surviving material (§1 is the near-verbatim
  thesis, §3's worked example is preserved from the original); a Slice-0 spike would have
  to re-derive detailed design from them, not consult the original.)

### 6. If pursued: the honest shape of the work (not a committed slice plan)

The original proposal's own three-phase build order is the right shape *if* R40 is greenlit:

1. **Structured coding harness first** (typed task schema, DAG scheduler, worktree/agent execution,
   deterministic gates, artifact capture, event-sourced state) — has standalone utility even without
   the compiler front end, and is the closest thing to what Claude Code's own `Workflow` tool already
   partially provides.
   > **Constraint on phase 1 (2026-07-31, gating):** "deterministic gates" and "artifact capture"
   > are **`R40_TCB` components (§1.5)** and must NOT be in the executing agent's write set. Phase
   > 1's very first design decision is the authorship split in §4 (evaluator on a separate,
   > hash-pinned path; evidence records carry the evaluator hash; seeds pre-registered). A phase-1
   > harness built without that split has to be rebuilt, not patched — which is the whole reason
   > these findings are being folded in at Draft rather than after a Slice-0 spike.
2. **Compiler front end** (project/experiment AST, semantic analysis, decision types, effects/
   capability checking, experiment-to-task lowering, typed graph mutations).
3. **Optimizing compiler / OuterLoop passes** (architecture analysis, blast-radius optimization,
   mechanism selection, confidence calibration, portfolio scheduling).
   > **Constraint on phase 3 (2026-07-31, gating):** these passes act on the rules that bound
   > them. Per §1.5 they are **out of the autonomous mutation channel's reach** — they may
   > propose changes to `R40_TCB` components (validator, evaluator, gate store, promotion rules,
   > approval boundary) but may never apply them; application is a human multi-sig node (R24),
   > chained into R28. If that is judged too expensive, **drop phase 3 — do not relax it.**

Repository placement, if built: a **separate package/repo** (`axon-gov` or a wholly distinct name),
not inside `crates/axon-core` — it orchestrates coding-agent work, it doesn't compile `.ax` source,
and conflating the two would reintroduce exactly the "what is Axon" ambiguity §5 warns against.

### 7. Biggest open risk / cost

**This is a multi-month-to-multi-year build for a product Axon has not decided to be.** Every
mechanism in §4 (typed decision/experiment/claim state machines, mutation validation, confidence
decomposition with source-tagging, event sourcing, a DAG scheduler with retries/checkpointing/
human-approval nodes) is individually reasonable but collectively enormous — closer in total
engineering surface to a new company's core product than to a spec-sized feature. The single
highest-leverage next step, if this is ever pursued, is **not** starting to build §6's Phase 1 — it
is resolving §12 Q1 first, because every subsequent design choice (repo structure, language choice
for the harness, whether it's NL-first or schema-first) depends on knowing what Axon is betting on.

> **Argument provenance (2026-07-31).** The paragraph above is **[cost-derived]**: "multi-month-
> to-multi-year," "closer in total engineering surface to a new company's core product" are
> engineering-hours claims, and they are exactly the claims that stronger code generation erodes
> over R40's own deferral horizon. The **[durable]** reasons to defer are elsewhere and do not
> depend on build cost at all: the identity/wedge question (§12 Q1), the platform-sprawl
> precedent (six competing platform fronts), "not proven necessary at Axon's own scale" given
> R39 already closed the concrete failure modes, and the **human-attention ceiling** (§5) — which
> a cheaper build makes *worse*, since it raises request throughput against a fixed reviewer.
> The single highest-leverage next step is unchanged and is not cost-sensitive: resolve Q1.

### 8. Error codes / exit codes

None reserved — out of scope until §12 Q1 resolves; a separate product would have its own
namespace, not Axon's E1xxx/E2xxx/E3xxx blocks.

**Not a substitute for naming a trusted base (2026-07-31).** "No codes reserved" previously read
as declining to name one at all. R40's trusted components *are* now enumerated in §1.5
(`R40_TCB`) with a non-weakening invariant, independent of which error-code namespace a future
product would use. Code allocation stays deferred; the TCB statement does not.

### 9. Acceptance (definition of done for the R40 row)

Not applicable at Draft — there is no committed slice to accept. The row's exit condition is a
founder decision (§12 Q1), not a test.

### 10. Reuse map — §4's concepts onto Axon's landed primitives (added 2026-07-31)

§12 Q1's pro-Axon argument is that "Axon already has more of the underlying primitives than a
typical language would … precisely the primitives §4's evidence/provenance/gate machinery would
otherwise have to build from scratch." **That claim was never cashed out anywhere in §4 or §6** —
no mutation was chain-signed, no experiment run was required to be replayable, no gate was a
refinement predicate, no task was scoped to a `Principal` budget, no ceiling was an effect row.
Q1's strongest argument was unevidenced by the spec's own design, and the design under-used
machinery whose value *rises* as the generator gets stronger (a replayable, chain-signed,
budget-scoped experiment record is worth far more against an adversary that can produce plausible
unfalsifiable evidence cheaply).

| §4 concept | Landed Axon primitive that realizes it | Status in-tree |
|---|---|---|
| Evidence record / mutation proposal | **R28** capability audit ledger entry (chained JSONL, `axon-os audit verify`) | Landed |
| Experiment run reproducibility | **F2 replay** — `run_start` record with run-id + effective seed, `AXON_SEED`, `AXON_AI_REPLAY` (byte-identical re-run with no live model) | Landed |
| Deterministic gate | Refinement predicate `T where P` — SMT-discharged where in-subset (wired into the default pipeline), runtime-checked otherwise (exit 6) | Landed |
| Task authority ceiling | `Principal` + `Sandbox<P>` effect ceiling (exit 8 on violation) | Landed (interp) |
| Per-task/experiment budget | Kernel `Goal` budget — each eval debits the principal, exhaustion halts (exit 7, E1604) | Landed |
| Approval node | **R24** defended approval boundary (legible grants, pre-approval simulation, risk-proportional friction, multi-sig, expiry/revocation) | Draft, spec'd |
| Confidence calibration store (§4 gap) | `axon trace --ai --json` (`axon-ai-audit/2`: per-call principal + effect_row) over the R28 chain | Landed |
| NL→typed lowering, if ever built (§12 Q3) | Phase-10 `axon intent compile` → `ast review` → `ast approve` — the human-approved **typed** artifact is what controls execution, not the prose | Landed |

**Scoped future slice (not committed; blocked behind §12 Q1 like everything else):** *Slice C —
Calibration backtest.* Score retained `model_estimate` values against the later `computed`
outcome for the same claim, over the existing R28 + `axon-ai-audit/2` records, and emit a
per-dimension calibration curve. It requires no new evidence store, is a pass over landed data,
and doubles as concrete evidence for Q1's "argument for part of Axon." It is listed here as a
scoped slice rather than an aspiration precisely so it is not mistaken for a commitment.

### 11. Stated limits and expiring assumptions (added 2026-07-31)

Recorded explicitly rather than left implicit, so a future reader can check whether the ground
has moved instead of inheriting an assumption silently.

| # | Stated limit / assumption | Why it may expire | If it expires |
|---|---|---|---|
| L1 | **R40's safety rests on a human actually reading every artifact it gates on.** This is a declared operating limit, not an incidental one. | §6's harness multiplies approval requests; reviewer capacity is fixed. | Approval allowance exhaustion **halts the harness** (§5). Never auto-approve, never batch-approve, never widen the diff. |
| L2 | The claim-promotion ladder's force comes from **evidence being expensive to fabricate**. | Cheap generation makes N seeds / N environments / the write-up the *cheapest* path to satisfying the predicate. | Independence + pre-registration fields become the load-bearing part of the predicate, not count (§4). |
| L3 | §1's capability-limited failure modes (forgetting, re-deriving, repeating) motivate much of R40's value. | Longer context, persistent memory and retrieval erode them independently of Axon. | The residual case is the **incentive half** of §1's table — re-justify scope there before answering Q1. |
| L4 | §5/§7's deferral leans partly on a **multi-month-to-multi-year engineering estimate**. | Stronger code generation shrinks the estimate. | The deferral does **not** thereby expire: Q1, the sprawl precedent, and the human-attention ceiling are cost-independent, and cheaper construction raises sprawl risk (§7). |
| L5 | §12 Q3's rationale ("NL parsing is expensive ambiguity-handling engineering") is a cost argument. | NL→typed extraction is the capability that improved most. | The *conclusion* (typed-schema-first) survives on durable ground — see Q3's re-derivation. |
| L6 | "Deterministic gates, never self-graded" assumed a proposer that would argue with a number. | An optimizer edits the measurement, not the number. | **Closed** by §4's authorship-separation rule; the assumption is recorded here because the original phrasing will keep reading as a stronger guarantee than it is. |

### 12. Open questions

- **Q1 (the decisive fork, blocks everything): is this Axon, or a separate product?** Unresolved,
  deliberately. Argument for "separate": Axon's own `VISION.md` (gated by `gate.sh`'s own
  `vision_focus` check) already commits to a tight, three-pillar thesis (proof, containment,
  goal-direction) for a *systems language*; R40 is an orchestration platform for *any* AI-driven
  project, a fundamentally different customer and problem. Argument for "part of Axon": Axon
  already has more of the underlying primitives than a typical language would (capability security,
  effect rows, provenance/replay, `Goal`/`Budget`/`Principal` kernel types, the R28 audit ledger) —
  precisely the primitives §4's evidence/provenance/gate machinery would otherwise have to build
  from scratch. This is the single question that must be answered before writing any code, not
  after a Slice 0 spike. **(Updated 2026-07-31.)** Two inputs to Q1 that did not exist when it was
  written: (i) the pro-Axon argument is now *cashed out* in §10's reuse map rather than asserted —
  every primitive it names is mapped to a §4 concept and is landed in-tree; (ii) per §1's split
  table and L3, Q1 must be answered against the **incentive-driven** half of §1's failure list
  alone, since the capability-limited half may close without any Axon work. Neither input resolves
  Q1; both change what it is asking.
- **Q2: if pursued, does R40 subsume R39, or does R39 ship independently first?** First half
  **resolved by events (2026-07-31 update)**: R39 shipped independently — all 5 slices landed by
  2026-07-20 (`governance/specs/R39-typed-execution-graph.md`, `governance/REQUIREMENTS.md` R39
  row), so its schema/store (`governance/state/specs.jsonl` + the slice-2/3/5 validators) now
  exists as a concrete artifact. Still open: the conditional — if Q1 resolves toward "yes, build
  R40," R39's schema becomes R40's Slice-1 proof of concept rather than throwaway work.
- **Q3: NL front end first, or typed-schema-only first (skip natural language entirely)?** The
  original proposal sequences NL parsing early; an alternative reading of its own principles
  ("typed over implicit," "graph mutations over document rewrites") suggests the NL front end is
  actually the *least* essential piece — a typed CLI/YAML input (skipping prose parsing entirely)
  would validate the harness+evaluator+graph-store core faster and with far less ambiguity-handling
  engineering. Worth deciding explicitly rather than defaulting to the order in which the idea was
  first described.
  - **Re-derived on durable ground (2026-07-31; see L5).** The stated rationale above is a *cost*
    argument about ambiguity-handling engineering — and NL→typed-schema extraction is the single
    capability that improved most, so that rationale is expiring. The conclusion (typed-schema
    first) most likely still holds, but for a different and durable reason: **the NL front end is
    not where the guarantee lives, and it inserts an unaudited, model-authored translation step
    between what the human said and what the graph asserts.** That is a trust-boundary problem,
    not an engineering-cost one. This repo has already solved this exact shape once — Phase 10's
    `axon intent compile` → `ast review` → `ast approve` makes the **human-approved typed AST the
    legal artifact**, not the prose, precisely so the lowering cannot quietly redefine intent.
    **If an NL front end is ever added, require that the lowering be recorded, replayable (F2),
    and human-confirmed against the typed result**, reusing the Phase-10 pattern rather than
    reinventing it — the approved typed graph, never the prose, is what controls execution
    (which is just §1's central design rule applied to R40's own front end).
- **Q4 (new, 2026-07-31): what makes a replication *independent* enough to count?** §4 now
  requires that `ArchitecturalFinding` and above include an adversarial or held-out replication
  the proposing agent did not author. Unresolved: what "did not author" means operationally when
  every implementation in the loop is model-generated — a different model? a different context
  with no access to the first implementation? a human-authored reference? a pre-registered
  held-out environment the harness cannot write to? This is the load-bearing definition behind
  L2, and it should be decided before Slice 0, not discovered during it.
- **Q5 (new, 2026-07-31): where does the evaluator physically live, and who can change it?**
  §4's authorship split requires evaluator/metric/gate code outside the executing agent's write
  set, hash-pinned into every evidence record. Unresolved: whether that is a separate repo with
  its own approval policy, a signed artifact the harness fetches, or an R26-style attested
  measurement; and what the update path is (R24 multi-sig is the presumptive answer, per §1.5).
  The *rule* is settled and gating; the *mechanism* is open.
- **Q6 (new, 2026-07-31): is the human-attention budget in L1 per-cycle, per-day, or
  per-reviewer, and what is the halt behavior at exhaustion?** §5 requires that exhaustion halts
  the harness rather than degrading to auto-approve. Unresolved: the unit, the default value, and
  whether a halted harness may queue work for later review or must stop cleanly. Do not resolve
  this by picking whatever number keeps the harness busy — that is the failure L1 exists to name.
