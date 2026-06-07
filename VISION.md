# Axon — Vision

> A short, high-level guide to *why* Axon exists and *what* it is trying to be.
> For shipped state see `STATUS.md`; for the plan see `ROADMAP.md`; for conventions
> see `CLAUDE.md`.

## The Core Problem

AI systems now *write and run code* and *take real-world actions* — but they do so
on languages and runtimes built for humans writing deterministic programs. Nothing in
that stack treats the things that actually make autonomous software dangerous as
first-class concerns:

- **No proof before action.** A program can do the wrong thing and you find out at
  runtime, in production, after the irreversible call.
- **No containment by default.** Any code can touch the filesystem, the network, or
  spend money; capability is ambient, not declared and bounded.
- **No goal-directedness.** "Try options, score them, keep the best under a budget" —
  the irreducible move of an agent — has to be hand-rolled as a loop, untyped and
  unaudited.
- **No durable audit.** What an agent did, why, under whose authority, and whether it
  can be replayed — none of it is a built-in artifact.

The result is AI software that is unsafe to trust, impossible to audit, and ungovernable
at scale.

## The Intended Solution

**Axon is an AI-first language, a userland OS, and a surface UX — one codebase, three
co-evolving layers.** It makes proof, containment, and goal-directedness the substrate
that everything else is built on, rather than libraries bolted onto a human language.

```
Surface UX     structured-prose intent → LLM-compiler → a typed AST the user APPROVES
   ↓
Surface .ax    goal / agent / for! maximize / simulate / verify / deploy
   ↓
Substrate .ax  refinement types · effect rows · principals · stores · SMT proof   (the IR)
   ↓
Userland OS    scheduler · store · supervision · LLM gateway · replay · audit · sandbox
```

The typed substrate is treated as an **IR**, not a human surface: optimized for
unambiguous audit and machine generation. Humans express *intent*; Axon's compiler
proposes a typed AST; **the approved AST — not the English — is the contract** that runs,
and the thing audits, bug reports, and rollbacks reference. No kernel ambition: it is a
userland OS (a daemon + library on Linux/macOS) whose "OS-ness" comes from its
abstractions, not its privilege level.

## Key Goals — the Three Load-Bearing Pillars

Every feature must advance at least one. If it advances none, it is out of scope.

1. **Proof.** Refinement types (`T where P`) checked as early as possible — statically
   via SMT when provable, at runtime otherwise. Earliest enforcement wins; a violated
   contract is a distinct, branchable signal, never a silent wrong answer.
2. **Containment.** Row-polymorphic effects and capability-bound principals. Code is
   **pure by default**; the right to do I/O, reach the network, spend a budget, or spawn
   a process is declared, typed, and attenuable — a principal can only ever mint a
   *subset* of what it holds.
3. **Goal-directedness.** Search is control flow. `for! maximize` and `goal { metric,
   constraints, budget, principal }` are not sugar — they are the native way an Axon
   program pursues an objective under bounds, with provenance recorded by construction.

## Technical Principles

- **No null, no exceptions.** `Option<T>` and `Result<T, E>` everywhere; `?` propagation.
- **Ownership without GC.** A simplified two-mode (`own` / `ref`) model.
- **Safety is enforced by the engine, never opt-out.** Audit trails, kill-switches
  (`@[corrigible]`), PII taint (`@[sensitive]`), and capability sandboxes (`@[contained]`)
  cannot be escaped by routing one call away — anti-laundering is transitive.
- **The interpreter is the reference oracle; native must match it byte-for-byte (I-2).**
  Every behavior — output *and* exit code — agrees between `axon run` and a compiled
  binary. When codegen cannot faithfully lower something, it refuses honestly (E0910)
  rather than silently miscompiling.
- **The stdlib defines the paradigm, not the syntax.** `Goal / Constraint / Budget /
  Principal / Agent / Store / Effect / Trace / Reward / Audit` — not `Vec / HashMap`. If
  the stdlib were the latter, we would have shipped Rust.
- **The TCB is enumerated, and self-modification cannot weaken it.** A system may not
  grant itself a capability it did not already hold (I-12).
- **Examples are the language's identity.** The public face is `examples/asi/` —
  optimization loops, multi-agent dialogues, budget-bounded planners,
  simulate→redteam→deploy — not `fibonacci.ax`.

## What Success Looks Like

- `divide(n, d: i64 where _ != 0)` is **proved safe at every call site**, and any
  obligation SMT cannot discharge is caught by a runtime check — soundness with no
  silent gaps.
- An effect/capability a function performs is visible in its type and **cannot be hidden**
  behind an un-annotated helper; a principal delegating to a sub-agent can only narrow
  authority and budget.
- A `goal { … }` runs a real search under a declared budget and principal, and every
  run produces a `(Trace, Seed)` that **re-executes deterministically** — replay, audit,
  and rollback are free.
- A non-expert states intent in structured prose, **reviews and approves the resulting
  typed AST**, and ships an agent whose every action is contained, metered, and audited.
- A `Risk ≥ High` change **automatically gates** through simulate → stress → redteam →
  verify → deploy, and a failure at any gate halts the deploy.

The north star: **autonomous software you can actually trust to run — provably bounded,
fully audited, and goal-directed — written by stating what you want and approving what
the machine proposes.**
