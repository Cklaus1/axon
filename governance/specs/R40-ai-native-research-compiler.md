# Tech Spec — R40: AI-Native Research/Build Compiler (general orchestrator — held for later)

**Spec ID:** `R40-ai-native-research-compiler` (the general-purpose superset of R39: a natural-language
front end, a typed IR spanning objective/decision/experiment/task/evidence/knowledge/provenance
graphs, a mutation-validation engine, and a harness runtime — usable for *any* AI-driven software or
research project, not just this repo's own governance)
**Status:** Draft (2026-07-18) — strategic proposal, captured in full because it is a coherent and
well-specified design, but **explicitly NOT founder-committed and not started.** This spec exists so
the idea is preserved with enough fidelity to build later, not because building it is recommended
now — see §5 and §12 Q1, which blocks everything past a possible Slice 0 spike.
**Risk class:** Structural (a new, separate product; multi-month; changes what "Axon" is, not what
it does — see §5's identity question)
**Author / date:** cklaus (research agent draft, transcribing a user-authored design proposal
in full), 2026-07-18

```spec-meta
id: R40-ai-native-research-compiler
status-claim: Draft
depends-on: none
blocks: none
blocked-by: R40 §12 Q1 (founder identity/wedge decision — is this Axon, or a separate product? —
  blocks everything past a Slice 0 spike)
supersedes: none
related: R39-typed-execution-graph, R38-embedded-agent-runtime, R18-provenance-ledger, R16-axon-ui
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
- **Promotion discipline for claims**: `Observation → Hypothesis → SingleRunResult → RepeatedResult
  → LocalResult → ArchitecturalFinding → GeneralizedClaim`. The compiler rejects unsupported jumps
  (e.g., one environment's result cannot promote to a cross-domain claim) as a type error, not a
  style note.
- **Decision types by uncertainty class** (`Implementation` / `Experimental` / `Architectural` /
  `Scientific` / `Strategic`), each implying a different required validation method and state
  machine (`PROPOSED → REVIEWED → APPROVED → TESTING → CONFIRMED | REJECTED | INCONCLUSIVE →
  SUPERSEDED`).
- **The InnerLoop/OuterLoop split**: InnerLoop executes one bounded task (`PLAN → DO → RUN → VERIFY
  → DECIDE`) and may not change gates, replace a baseline after seeing results, or start a new
  experiment. OuterLoop reviews evidence against decisions and emits exactly one typed mutation per
  cycle — never "try whatever seems best" in prose.
- **LLM output is always a typed mutation proposal** (`create_decision`, `create_experiment`,
  `create_tasks`, `add_edges`, …), validated by the compiler before being applied — never a
  free-form rewrite of project state.
- **Deterministic gates, never self-graded.** `metric >= threshold` is evaluated by code; the model
  interprets *why*, but cannot override the numeric result.

### 5. What R40 is NOT (non-goals — read before writing a line of code)

- **Not something to build without an explicit identity decision first.** This is the single
  biggest risk: the full design (natural-language compiler front end + typed research IR + mutation
  validator + task scheduler + evidence engine + event sourcing) is a standalone product on the
  scale of "a coding-agent orchestration platform," comparable in engineering scope to
  Devin/Cursor/Claude-Code-with-a-formal-research-methodology — not a feature of a systems
  programming language. Building it under the Axon name without deciding "is Axon now this, in
  addition to a language?" risks the same identity confusion the R16/R17/R18/R36/R37/R38 platform-
  vision sprawl already produced (six competing platform fronts as of this session, per
  `governance/REQUIREMENTS.md`'s strategic note).
- **Not proven necessary at Axon's own scale.** `R39-typed-execution-graph.md` argues the *scoped*
  version (typed schemas for this repo's own governance, no NL front end, no experiment/decision
  graph, no mutation engine) already closes the concrete failure modes this session found. R40's
  additional machinery (decision-uncertainty typing, claim-promotion rules, confidence
  decomposition, event sourcing) is designed for a *different* problem shape — a portfolio of
  scientific/ML research experiments with baselines, seeds, and reproducibility concerns — which is
  not what building a compiler and its stdlib actually looks like day to day. Don't assume R40's
  extra machinery transfers just because R39's smaller subset clearly helped.
- **Not a replacement for human governance/strategic judgment.** Even in the full design, strategic
  decisions and irreversible actions require human approval boundaries (§21 of the original
  proposal) — this is orchestration tooling, not an autonomous-company system.
- **Not scoped, sized, or sequenced in this draft.** Unlike R36-R38 (which at least name phased
  slices with named gates), R40 is preserved at the *idea* level. A real build would need its own
  Slice-0 spike (see §12) before any of §6-30 of the original proposal becomes a committed plan.

### 6. If pursued: the honest shape of the work (not a committed slice plan)

The original proposal's own three-phase build order is the right shape *if* R40 is greenlit:

1. **Structured coding harness first** (typed task schema, DAG scheduler, worktree/agent execution,
   deterministic gates, artifact capture, event-sourced state) — has standalone utility even without
   the compiler front end, and is the closest thing to what Claude Code's own `Workflow` tool already
   partially provides.
2. **Compiler front end** (project/experiment AST, semantic analysis, decision types, effects/
   capability checking, experiment-to-task lowering, typed graph mutations).
3. **Optimizing compiler / OuterLoop passes** (architecture analysis, blast-radius optimization,
   mechanism selection, confidence calibration, portfolio scheduling).

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

### 8. Error codes / exit codes

None reserved — out of scope until §12 Q1 resolves; a separate product would have its own
namespace, not Axon's E1xxx/E2xxx/E3xxx blocks.

### 9. Acceptance (definition of done for the R40 row)

Not applicable at Draft — there is no committed slice to accept. The row's exit condition is a
founder decision (§12 Q1), not a test.

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
  after a Slice 0 spike.
- **Q2: if pursued, does R40 subsume R39, or does R39 ship independently first?** Leaning: R39 ships
  independently regardless of Q1's answer (it's cheap, already-justified-by-evidence, and scoped to
  this repo only). If Q1 resolves toward "yes, build R40," R39's schema becomes R40's Slice-1 proof
  of concept rather than throwaway work.
- **Q3: NL front end first, or typed-schema-only first (skip natural language entirely)?** The
  original proposal sequences NL parsing early; an alternative reading of its own principles
  ("typed over implicit," "graph mutations over document rewrites") suggests the NL front end is
  actually the *least* essential piece — a typed CLI/YAML input (skipping prose parsing entirely)
  would validate the harness+evaluator+graph-store core faster and with far less ambiguity-handling
  engineering. Worth deciding explicitly rather than defaulting to the order in which the idea was
  first described.
