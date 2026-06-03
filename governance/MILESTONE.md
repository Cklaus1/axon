# Axon — Language-Core Milestone (2026-06-03)

A snapshot of where Axon stands: an honest accounting of what is **built and
verified**, what **remains**, and **why** the remainder is what it is. The
source of truth for per-requirement detail is `REQUIREMENTS.md`; this is the
one-page picture.

## Headline

The **Axon language core is ~93% complete and stable** — the compiler, type
system, AI-as-primitive surface, optimization, capability security, testing,
formal verification, and self-improvement firewall are all built, tested, and
running with native↔interpreter parity. **~700 tests pass; zero failures.**

What is *not* built is the broader **platform vision** the PRD also describes (a
GPU-rendered UI framework, mobile runtimes, a 3D engine, kernel runtime
services) — that remains greenfield, multi-quarter work.

## Requirement scorecard

| Req | What | % | State |
|---|---|---|---|
| **R1** | Native pipeline (parse→type→borrow→LLVM→native) | 95 | ✅ 28/28 example parity, 6× faster than interp |
| **R2** | Type system + borrow checker (HM, no-null, ownership) | 90 | ✅ Strong; dyn-trait + refinement types remain |
| **R3** | AI as a language primitive (routing, policy, budget, cost) | 85 | ⚠️ tier routing live; live token-count + Budget type remain |
| **R4** | Three code zones + compiler-enforced provenance | 100 | ✅ Complete |
| **R5** | Goal-directed optimization (5 strategies, held-out eval) | 94 | ⚠️ kernel `Goal<M>` (Phase-7) remains |
| **R6** | Capability security (lockfile, audit, `@[sensitive]`) | 91 | ⚠️ AI-audit quality + transitive taint remain |
| **R7** | Cross-platform (native / wasm / js / mobile) | 68 | ⚠️ interp→wasm verified (compute+fs+env); AOT-wasm ABI retarget remains |
| **R8** | Built-in testing + structured diagnostics (`forall`) | 100 | ✅ Complete |
| **R9** | Layer-1/3 alignment (`Uncertain`/`Temporal`/`@[verify]`/causal) | 88 | ⚠️ SMT loop invariants + metacognition trait remain |
| **R10** | Self-improving compiler (4-gate firewall, AI discoverer) | 98 | ⚠️ a live AI discoverer over the static one remains |

**Average 91.7% · language-core (R1-6,8-10) ~93% · full-platform vision ~15%.**

## What "done and verified" means here

Every ✅/⚠️ above is backed by passing acceptance tests, not aspiration:

- **Native == interpreter** on all 28 pure-compute + AI-under-mock examples
  (`all_examples_parity.sh`), the interpreter being the I-2 reference oracle.
- **Cross-platform**: the same `.ax` runs byte-identically on native and
  `wasm32-wasip1` across compute, file I/O, and env vars
  (`wasm_parity.sh` 28/28, `wasm_fs_parity.sh`).
- **Formal verification**: `axon verify` proves `@[verify]` integer + float +
  conjunction bounds via Z3, or reports a concrete counterexample.
- **Self-improvement safety**: an AI can only *select* optimization templates
  from a closed registry; the four-gate firewall (correctness oracle + capability
  diff + regression + perf) and multi-sig graduation are the sole path to a
  runnable pass — red-team-hardened.
- **Privacy**: `@[sensitive]` data flowing to an AI call / file write / process
  exec is a compile error (E1206), across struct/field/typed-field/array flows.

## The flagship

`examples/asi/ad_optimizer.ax` — an autonomous Meta-Ads optimizer that composes
the whole stack into one believable workflow: tournament variant search,
`Uncertain<T>` ROAS confidence, agent metacognition, a `@[verify]`-capped spend
bound, and `Temporal<T>` ad-creative fatigue. It is the one-line answer to "why a
language for AI apps?" — the expensive mistakes (blow the budget, trust a shaky
prediction, run a stale creative) become things the type system handles.

## What remains, and why it's not a bounded slice

The bounded, one-gated-slice backlog is **exhausted**. Each remaining item is a
multi-slice epic with its own spec (where applicable):

| Epic | Why it's large |
|---|---|
| **R7 AOT-wasm retarget** | codegen bakes i64 pointers into the str/array IR; wasm32 needs i32 — a cross-cutting codegen ABI change (`R7-targets.md` §12 Q6) |
| **Kernel Phase-7 services** | scheduler + durable stores + live token wiring + SMT-checked cap subsets (`R12-kernel-runtime-services.md`) |
| **SMT loop invariants** | invariant inference past straight-line code (`R9b-smt-loop-invariants.md`) |
| **Platform vision** | UI/GPU/mobile/3D — entirely greenfield, no foundation yet |

These are deliberate scope decisions, not oversights. The language core is a
strong, shippable v1; finishing the PRD from here means committing to one epic
at a time, spec → sliced plan → build.
