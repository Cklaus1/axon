# Axon — Vision

> A short guide to *why* Axon exists and *what* it is trying to be. The first half is for
> everyone; the **For engineers** section adds the precise terms. Shipped state: `STATUS.md`;
> plan: `ROADMAP.md`; conventions: `CLAUDE.md`.

Axon is a language and runtime for software that AI writes and runs on your behalf.
You say what you want; Axon turns it into an exact, reviewable plan you approve; then it
runs that plan **fenced-in, within a budget, and with a full record of everything it did.**

## The Problem

AI can now write code and take real actions — send email, move money, change systems — but
on tools built for humans writing ordinary programs. Those tools miss the four things that
make self-running software safe to trust:

- **They don't check before acting** — you find out something was wrong *after* it happened.
- **They don't fence off what code can touch** — any code can reach the network, disk, or
  your wallet; permission is assumed, not declared and limited.
- **They have no built-in "find the best option under a budget"** — the core move of an
  agent has to be hand-built every time, untracked.
- **They keep no real record** — what was done, why, under whose authority, replayable? None
  of it comes for free.

The result is AI software that's hard to trust, hard to audit, and hard to govern at scale.

## What Axon Is

Axon is three layers that grow together as one project:

```
You state intent      →  "keep support replies under 2s and over 90% satisfaction"
   ↓  Axon proposes an exact plan, and YOU approve it before anything runs
Approved plan (the contract)  →  the reviewed steps — not the English — are what runs
   ↓  it executes, fenced-in and recorded
Runtime                →  scheduler · model gateway · budget meter · audit · replay
```

The reviewed plan, not your sentence, is the real contract — it's what runs, and what audits
and rollbacks point to. Axon runs as an ordinary program on Linux/macOS (no operating-system
takeover); what makes it feel like an OS is its building blocks — goals, budgets,
permissions, agents, audited actions — not special privileges.

## What Makes It an AI Language

The capabilities that set Axon apart from an ordinary typed language — what an *AI-first*
language can be:

1. **Intelligence is built in.** Calling a model (complete a prompt, extract a typed value
   from a document) is a language primitive, like networking — not a bolted-on library.
2. **Every function declares how much it may change.** *Static* never changes (most code);
   *adaptive* may improve its own prompt/model within fixed limits; *agents* reason and use
   tools under hard "never/always" rules. You choose, per function.
3. **Pursuing a goal is a built-in.** "Try options, score them, keep the best under a
   budget" is native syntax with the search recorded automatically — not a hand-rolled loop.
4. **Uncertainty is part of the type.** A value from an AI carries its confidence, and you
   can require a minimum ("only act if at least 90% sure") — checked, not hoped.
5. **The right model, within your limits** — calls route to a local or cloud model by task,
   and you can cap cost or keep data on-device per function.
6. **Safety can't be quietly bypassed.** Spending limits, kill-switches, privacy tags, and
   permission fences are engine-enforced — you can't escape them by hiding the action one
   call deeper.

(Full list with shipped/in-progress status in the engineer section below.)

## The Three Goals

Every feature must advance at least one. If it advances none, it's out of scope.

1. **Proof — check before acting.** Catch a bad value as early as possible: when the code is
   written if provable, otherwise the moment it runs. A broken promise is a clear signal,
   never a silent wrong answer.
2. **Containment — least privilege by default.** Code can do nothing until granted; the right
   to use the network, spend money, or touch files is declared and bounded, and an agent can
   only hand a *smaller* slice of its authority to a helper.
3. **Goal-direction — search is how the program decides.** Pursuing an objective under limits
   is the native move, with the reasoning recorded as it goes.

## What Success Looks Like

- A "must not be zero" divisor is **proved safe where possible, caught at run time otherwise**.
- What a function may touch is **visible and impossible to hide**; an agent can only hand a
  helper a *narrower* slice of its authority.
- Pursuing a goal runs a real search under a stated budget, and **every run replays exactly**
  — so audit and rollback are free.
- A non-expert states a goal, **reviews and approves the plan**, and ships an agent whose
  every action is fenced-in, budgeted, and recorded.
- A high-risk change **automatically runs the gauntlet** (simulate, stress, red-team,
  verify) and is blocked if any stage fails.

The north star: **autonomous software you can actually trust to run — provably bounded,
fully audited, and goal-directed — written by stating what you want and approving what the
machine proposes.**

---

## For engineers

Precise terms and the design principles behind the plain language above.

**Technical principles**

- **No null, no exceptions** — `Option<T>` / `Result<T, E>` everywhere; `?` propagation.
  Ownership without GC (two-mode `own` / `ref`).
- **Safety is engine-enforced, never opt-out.** Audit trails (`@[agent]`), kill-switches
  (`@[corrigible]`), PII taint (`@[sensitive]`), capability sandboxes (`@[contained]`) use
  *transitive anti-laundering* (a guard you can't escape by routing through an un-annotated
  helper).
- **I-2 — the interpreter is the reference oracle; native matches it byte-for-byte.** Output
  and exit code agree between `axon run` and a compiled binary; codegen that can't faithfully
  lower something refuses honestly (E0910) rather than miscompile.
- **The stdlib defines the paradigm, not the syntax** — `Goal / Budget / Principal / Agent /
  Store / Effect / Trace / Audit`, not `Vec / HashMap`.
- **The TCB (trusted computing base — the code that must be correct for safety to hold) is
  enumerated, and self-modification cannot weaken it** (a system may not grant itself a
  capability it didn't already hold; invariant I-12).
- **Examples are the identity** — the public face is `examples/asi/`, not `fibonacci.ax`.

**AI-language showcase features — status** (per `STATUS.md` / `ROADMAP.md`):

| # | Capability | Status |
|---|---|---|
| 1 | AI as a primitive (`ai_complete`, `ai_extract<T>`, embed/tts) | shipped (`ai_complete`, `ai_extract*`) |
| 2 | Three code zones — static / adaptive / agent | shipped (adaptive hill-climb, agent audit) |
| 3 | Goal-directed search (`goal{…}`, `for! maximize`) | shipped (`goal_run`); surface keywords Phase 8 |
| 4 | Uncertainty as a type + bounds (`Uncertain<T>`, `@[verify]`) | shipped |
| 5 | Tiered model routing + per-fn AI policy/budget | partial (tier + cost meter; live token-count remains) |
| 6 | Proof via refinement types (`T where P`) | shipped (runtime both sides; SMT static via `axon verify`) |
| 7 | Containment + PII taint + kill-switch | shipped |
| 8 | Errors structured for AI, pretty for humans | shipped (`axon-diag/1`) |
| 9 | Token-dense, AST-first (approved AST is the contract) | shipped (substrate); surface compiler Phase 10 |
| 10 | Replay + provenance (`(Trace, Seed)`, `AXON_SEED`) | partial (userland + seeded RNG; kernel replay Phase 9) |

Refinement enforcement (#6): SMT static discharge (Z3, opt-in `smt`, via `axon verify`)
proves what it can; the default build checks the predicate at runtime — at entry for a
parameter, at every return for `-> T where P` — byte-identically in interpreter and native.
Architecture: surface UX → surface `.ax` → substrate `.ax` (the typed IR) → userland
runtime; see `ROADMAP.md` §3.
