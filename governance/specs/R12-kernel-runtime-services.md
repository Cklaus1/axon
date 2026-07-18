# Tech Spec — R12: Kernel runtime services (Phase-7 tier)

**Status:** ✅ Landed (re-verified 2026-07-18) — all 5 slices + the R12b follow-on shipped, one
commit per slice, in the exact order this spec prescribes: Slice 1 `principal_authority`
(`6310d52`), Slice 2 `cooperative_scheduler` (`dc478aa`), Slice 3 live `supervisor_root`
(`9aa5d24`), Slice 4 durable `Store<T,C>` (`0eacbb5`), Slice 5 kernel `Goal<M>` + `LLM<Caps>`
(`3aae955`, "Phase 7 complete"), plus kernel `Goal` budget-scoping (`fdbb8b2`, R12b). Every named
gate in §4 below has a matching passing test: `cargo test --lib kernel::` — 20 passed, 0 failed
(mint/attenuation, scheduler fan-out + suspend/resume, supervisor restart + intensity-latch, store
linearizable/at-least-once, LLM real-token-count metering); `phase7_kernel_{principal_authority,
scheduler,supervisor,durable_store,llm_gateway}` in `cli_run.rs` — 5 passed, 0 failed;
`kernel_goal_*_r12b` tests cover the R12b follow-on. This header said "Draft" long after every
slice shipped — same staleness class as R17/R21/R22/R23/R26/R27/R28/R29/R31/R32, caught by the
same outer-loop sweep (`EXECUTION_MODEL.md` §2) — this time found by grepping every spec's own
gate-test names against the actual test suite rather than trusting REQUIREMENTS.md's Top-10 table
(R12 isn't a Top-10 row; the CLAUDE.md Phase Status table already said "✅ Complete (kernel)" for
Phase 7, but this spec file itself was never updated to match).

```spec-meta
id: R12-kernel-runtime-services
status-claim: Landed
depends-on: R11-capability-minting
blocks: none
blocked-by: none
supersedes: none
related: R20-smt-capability-proofs, R9b-smt-loop-invariants
conflicts-with: none
reserves: none
evidence: cargo test --lib kernel:: --no-default-features -p axon-core (20 tests, re-verified 2026-07-18); cargo test --test cli_run phase7_kernel (5 tests)
```
**Requirement:** `../REQUIREMENTS.md` R5/R6/R9 + ROADMAP §6–7 (Phase-7 "Runtime services" tier). The **kernel** counterparts of the userland TCB modules already shipped (`examples/stdlib/{principal_mint,supervisor_tree,store,llm_gateway,goal}.ax`).
**Parent:** `R11-capability-minting.md` (✅ Reviewed) landed the *userland* `capability_minter`; this spec scopes the *kernel* services it and the others become.

---

## 1. Motivation

Phase-7's four runtime-services TCB components each have a **userland realization** (a first-class `.ax` value with the safety property enforced by construction): `capability_minter` (principal_mint.ax), `supervisor_root` (supervisor_tree.ax), `Store` consistency (store.ax), `llm_gateway` (llm_gateway.ax). Userland proves the *semantics*. The **kernel** versions are what the runtime enforces *for* a program — live fibers under a scheduler, durable stores that survive a process, principal-tagged capability sets the compiler SMT-checks, AI calls metered against a real token count. These are the heaviest remaining R-series work because they need a **substrate that does not exist yet**: a cooperative scheduler and a principal-authority the rest hang off. This spec's job is to **sequence** them so each lands as a gated slice instead of a big-bang kernel.

## 2. Requirement link

Closes the "kernel ___ remaining Phase-7" gaps cited across R5 (kernel `Goal<M>`), R6 (principal-scoped audit), R9/R11 (SMT-proven cap subsets), R3 (live token-count). Dependencies: **I-2** (every kernel service must match its userland/interp semantics), **R11** (attenuation by construction), **R10** (the "no second mechanism" firewall posture), **R1** (codegen — kernel services that touch generated code need the native build, which landed).

## 3. The decisive fork

**Does the kernel get a real preemptive scheduler, or a cooperative one threaded through the interpreter's existing eager-spawn model?**

**→ Resolved: cooperative, built on the interpreter's existing spawn/channel machinery — NOT a preemptive OS-thread scheduler.** Per memory [[axon-cooperative-concurrency]] the interpreter already runs `spawn` eagerly and supports fan-out/collect via channels. A preemptive scheduler would be a second execution model to keep in sync with `interp.rs` (an I-2 drift risk) and is unnecessary for the ASI workloads (goal loops, supervised workers, AI gateways) which are I/O- and decision-bound, not compute-preempted. The kernel scheduler is a **cooperative run-queue of fibers** that yield at await points (channel recv, AI call, sleep), with the supervisor restarting failed fibers. This reuses the shipped concurrency, makes the supervisor *live* (not just a pure `restart_set` function), and stays single-model.

## 4. Slice sequence (each a gated build)

The substrate must come first; the four services hang off it. Ordered by dependency, each independently shippable + gated:

### Slice 1 — `principal_authority` (the root the rest need)
A runtime registry of live `Principal`s (id, parent, caps, budget) with the R11 attenuation enforced *in the kernel* mint path, not just userland. Kernel `mint` returns a handle; the registry tracks the live tree for audit. **Gate:** a kernel mint produces an attenuated child (cap = want ∧ parent.cap, budget carved); a mint attempting escalation is structurally impossible (returns the clamped child), asserted by test. This is R11's kernel half — its E1210 band.

### Slice 2 — `cooperative_scheduler` (the substrate)
A fiber run-queue over the interpreter's spawn model: fibers yield at await points; the scheduler round-robins ready fibers; a fiber's failure is observable (not a process abort). **Gate:** N fibers fan-out/collect with deterministic output under `AXON_SEED`; a panicking fiber is caught and reported, not fatal. Builds on [[axon-cooperative-concurrency]] (send-before-recv ordering).

### Slice 3 — `supervisor_root` goes live
Wire `supervisor_tree.ax`'s pure `restart_set`/`on_failure` logic to the Slice-2 scheduler: a supervised fiber that fails is *actually restarted* per its strategy (one_for_one/all/rest), with the max-restart-intensity latch tripping a real halt (Flow::Halted, exit 4) on a crash loop. **Gate:** a fiber that fails K times within the window halts its subtree (not the process); restart strategies produce the documented live restart sets.

### Slice 4 — kernel `Store<T,C>` (durable)
Promote `store.ax`'s consistency variants to a runtime store that **persists** across a process (the Lifetime axis store.ax left orthogonal): linearizable = exactly-once + monotonic version, at_least_once = re-apply. Persistence via the existing provenance NDJSON append path (replayable, deterministic). **Gate:** a value written under linearizable survives a fresh process (replay) and dedups a retried op_id; at_least_once double-counts — the store.ax headline test, now across a process boundary.

### Slice 5 — kernel `Goal<M>` + `LLM<Caps>` (the consumers)
With authority + scheduler + store in place, the kernel `Goal<M>` (SMT-checked constraint via R9b, principal-scoped via Slice 1) and the live `LLM<Caps>` gateway (real token-count metering from the model response, closing R3's last gap — replaces today's length estimate) become assembly of the lower slices. **Gate:** a kernel goal runs under a principal's budget and refuses to exceed it; an LLM call charges the *real* returned token count (verified against a mock that reports a known count).

## 5. Semantics / invariants

- **I-2:** every kernel service must match its userland `.ax` module's observable semantics (the userland module is the reference; a kernel-vs-userland parity test per slice, mirroring the codegen↔interp parity harnesses).
- **I-12 (attenuation):** Slice 1 enforces it by construction (R11); no kernel path grants a cap a principal lacks.
- **I-13 (provenance un-opt-out):** kernel fibers/AI-calls/store-writes emit the same provenance records the interpreter does (principal-tagged, the R6 audit-stream gap).
- **Determinism:** the scheduler is `AXON_SEED`-reproducible (run-queue order is a deterministic function of spawn order + the seed).

## 6. Error codes

New E16xx band (kernel runtime): E1601 cap-escalation attempt (should be unrepresentable — a defense-in-depth assert), E1602 supervisor crash-loop halt (exit 4), E1603 store consistency violation, E1604 kernel-goal budget exceeded. (Distinct from R11's E1210 userland-mint band.)

## 7. Test plan

Each slice has a userland↔kernel parity test (the userland module is the oracle) + a determinism test under `AXON_SEED`. The cross-cutting acceptance: a single demo that mints a principal, spawns supervised worker fibers under it, has them write to a durable store and call an LLM under a budget — and survives a worker crash-loop by halting just that subtree. This is the Phase-7 capstone demo (`examples/asi/kernel_orchestrator.ax`, to be written in Slice 5).

## 8. Scope / non-goals

- **In:** the five slices above, cooperative-scheduler model, kernel parity with userland modules, E16xx band.
- **Out:** preemptive scheduling; distributed/replicated stores (store.ax's Quorum/CRDT future); a real network LLM (mock + the existing asi-runtime live path); GC/memory-management changes. Each slice is gated and reversible; the substrate (Slices 1–2) must land before the consumers (3–5).

## 9. Open questions

- **Q1 (scheduler fairness):** round-robin vs priority — start round-robin (deterministic, simplest); add priority only if a real workload starves. 
- **Q2 (store persistence format):** reuse provenance NDJSON vs a dedicated log — reuse NDJSON first (replay machinery exists), split out only if contention shows.
- **Q3 (where does the scheduler live — interp.rs vs a new kernel.rs?):** new `kernel.rs` module, interp-feature, so the codegen build is untouched; the interp drives fibers (consistent with I-2). Confirm before Slice 2.
- **Q4 (live token-count source):** Slice 5 needs the model response's token field — available from the asi-runtime live path; under mock, the mock reports a deterministic count. Confirm the asi-runtime response surface exposes it before Slice 5.
