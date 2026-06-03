# Tech Spec — R11: Capability minting with attenuation (the runtime `capability_minter`)

**Spec ID:** `R11-capability-minting` (Phase-7 runtime services; realizes I-12 at the runtime/delegation edge)
**Status:** Reviewed (2026-06-02) — userland slice landed
**Risk class:** Structural (TCB component)
**Author / date:** autonomous build, 2026-06-02

---

## 1. Motivation

ROADMAP §6 row 7 (Phase 7, Runtime) requires: *"A Principal can mint a sub-Principal with a strict subset of capabilities + budget."* That mint logic is the **`capability_minter`** TCB component (ROADMAP §7). The user-visible win: an orchestrator can delegate a bounded slice of its authority to an agent it does not fully trust — handing over *fewer* capabilities and a *carved* budget — with a mechanical guarantee that the delegate can never exceed what it was given, even transitively. Without that guarantee, delegation is the exfiltration path: a sub-agent that can mint itself `net` when its parent lacked it has escaped the capability boundary (I-11) and weakened the TCB (I-12).

## 2. Requirement link

ROADMAP §6 row 7 + the `Principal` Tier-1 type row (`id, parent, capabilities, audit_log, mintable_subset`). Invariants: **I-12** (self-modification cannot weaken the TCB — *"may never grant itself a capability it didn't already hold"*) and **I-11** (the capability boundary is real and total). This spec is I-12's **runtime** counterpart — R6 realizes I-11/I-12 at the *import/registry* edge (content hash + audit on `axon add`); R11 realizes them at the *runtime delegation* edge (minting a sub-Principal).

The hard property both gates share is **ATTENUATION**: authority can only shrink as it is delegated.

## 3. Surface (what the user writes)

```axon
// A root holds full authority + a budget. Minting carves a strict subset.
let r = root("orchestrator", true, true, true, 1000)          // net, fs_write, exec, budget
let (r2, worker) = mint(r, "fetch-worker", true, false, false, 300)
//        ^ debited parent     ^ child: net only, budget 300; orchestrator now has 700

// A child re-requesting more than the parent holds gets only what the parent had:
let (_w2, sub) = mint(worker, "sub", true, true, true, 500)
//   sub.fs_write == false, sub.exec == false  (worker never held them)
//   sub.budget.cap == 300                      (clamped to the worker's remaining)
```

Error case (the safety property, stated as what *cannot* happen): there is **no** call to `mint` that returns a child holding a capability the parent lacks, or a child budget exceeding the parent's remaining. Escalation is unrepresentable, not rejected-at-runtime.

## 4. Semantics

| Input class | Behavior |
|---|---|
| `mint` with child caps ⊆ parent caps | Child gets exactly the requested subset; grant carved from parent. |
| `mint` requesting a cap the parent lacks | That cap is `false` on the child (`want_X && parent.X`). No error, no escalation. |
| `mint` with `budget_grant` ≤ parent remaining | Child capped at the grant; parent debited by the grant. |
| `mint` with `budget_grant` > parent remaining | Grant clamped to parent remaining; parent → 0. Authority not manufactured. |
| `mint` with negative `budget_grant` | Clamped to 0 (`max(grant,0)`). |
| chain root → child → grandchild | Each hop can only drop caps/budget: grandchild ⊆ child ⊆ root. |
| no-cap (sandboxed) root mints anything | Every child holds no caps — the floor is sealed. |
| `authorize(p, needs…)` | True iff `p` holds every needed cap AND `!budget_exhausted`. |

**Carve invariant:** after any mint, `sum(remaining budgets of all live principals) ≤ root cap`. Minting moves authority, never creates it.

## 5. Type rules

N/A at the userland layer — `Principal`/`Budget` are ordinary structs; attenuation is enforced in the `mint` function body, not the type system. (The **kernel** form — a language-level `Principal<Caps>` whose capability set is a refinement-typed subset relation checked by the SMT solver, so `mint`'s attenuation is *proven* not *tested* — is Phase-7 architectural work, §12 Q1.)

## 6. Error codes

The userland slice raises none (attenuation is by construction — there is no violating state to flag). The **kernel** slice reserves one for the day a primitive `mint` is exposed that could be mis-called across a trust boundary:

| Code | Trigger | Message shape |
|---|---|---|
| **E1210** (reserved) | A kernel `mint`/delegation attempts to grant a capability the minter does not hold | `` `{child}` cannot be granted `{cap}` — `{parent}` does not hold it (capability attenuation, I-12) `` |

Slots into the E12xx capability/registry band opened by R6 (E1201–E1205), continuing the security family.

## 7. Invariants touched

- **I-12 (self-modification cannot weaken the TCB):** this spec is I-12's runtime realization. A minted child's authority is a strict subset of its minter's by construction; no delegation path escalates. **Realized at the delegation edge.**
- **I-11 (capability boundary real and total):** minting cannot create authority outside the boundary — the carve invariant keeps the total bounded by the root. **Extended across the delegation edge.**
- **I-14 (stable codes):** E1210 reserved in the E12xx band. **Preserved.**
- No invariant changed — R11 *fulfills* I-12 for runtime delegation, as R6 did for imports.

## 8. Test plan

Red test that must fail first (conceptually — it passes because attenuation is by construction): **`test_mint_cannot_escalate_caps`** — a parent lacking `net` mints a child requesting `net`; assert the child does **not** hold `net`. A naive `mint` that copied the requested caps verbatim would fail this.

- [x] Unit/integration (`examples/stdlib/principal_mint.ax`, 8 `@[test]`): subset mint; cap-escalation impossible; budget carved from parent; over-grant clamped; chain stays attenuated; no-cap root mints only no-cap children; `can_mint` gate; `authorize` gate.
- [x] CLI e2e (`axon test stdlib/principal_mint.ax` → 8 passed): gated by `principal_mint_stdlib_module_tests_pass` in `cli_run.rs`.
- [x] Property (the carve invariant): `test_budget_is_carved_from_parent` asserts `child_remaining + parent_remaining ≤ root_cap`.
- [ ] Kernel parity (deferred): when a primitive `mint` lands, an SMT proof that the child capability set ⊆ parent's for all inputs (Q1).

## 9. Acceptance criteria

The userland slice is DONE (all pass):
- [x] `principal_mint_stdlib_module_tests_pass` — 8 @[test] green.
- [x] `test_mint_cannot_escalate_caps` — escalation impossible.
- [x] `test_chain_stays_attenuated` — transitive attenuation.
- [x] `test_overgrant_is_clamped_to_parent_remaining` — budget cannot be manufactured.

## 10. Performance budget

N/A — pure-value struct reconstruction, no hot path.

## 11. Rollout & rollback

Additive: a single self-contained `.ax` module + one cli test. `git revert` leaves a green tree. Zero blast radius — it adds a userland module, changes no compiler code. The kernel `mint` primitive (when specified) is a separate, larger commit gated on this spec reaching the kernel scope.

## 12. Open questions

- **Q1 (kernel `mint` + SMT attenuation):** the userland `mint` *tests* attenuation; a kernel `Principal<Caps>` would *prove* the child ⊆ parent subset relation via the Phase-5 SMT solver, making escalation a compile error (E1210) rather than a tested-absent state. Phase-7 + Phase-5 territory. Non-blocking.
- **Q2 (principal-tagged audit stream):** the ROADMAP `Principal` row carries an `audit_log`; tying every minted principal's actions into the R3/R4 provenance stream (so a delegated action is attributable to its minting chain) is the F3 cross-spec cleanup. Non-blocking.

## Review note (2026-06-02)

Status: **Reviewed** — the decisive fork (*attenuation by construction vs checked after the fact*) is resolved **by construction**: a post-hoc checker is a second mechanism that can be bypassed/skipped — exactly the I-12 failure mode — so escalation is made unrepresentable (`want_X && parent.X`, `min(grant, remaining)`) and there is nothing to verify or disable. This mirrors R4 ("no opt-out construct, rejected by design") and R10's G2 capability-diff. The after-the-fact check is kept only as future defense-in-depth (E1210), never the sole boundary — the same posture R6 takes (static checker is the hard gate; audit is defense-in-depth). Userland slice landed + gated; kernel form is the named Phase-7 follow-on.

---

## Appendix A — Supervisor tree (sibling Phase-7 runtime primitive, `supervisor_root`)

A second Phase-7 TCB component (ROADMAP §7 `supervisor_root`) lands at the userland layer alongside the minter, same two-track posture. ROADMAP §6 row 7 requires the `Supervisor` to run *"OTP-style trees (one_for_one / one_for_all / rest_for_one + backoff)."*

`examples/stdlib/supervisor_tree.ax` models a supervisor over N ordered child workers. The **decisive design point** is to factor the OTP semantics into a *pure* core — `restart_set(strategy, failed, n_children) -> [i64]`, the ascending list of child indices that restart when `failed` crashes — so each strategy is a precise, independently-testable rule rather than entangled with restart bookkeeping:

| Strategy (tag) | restart set when child `f` of `n` fails |
|---|---|
| `one_for_one` (0) | `[f]` — only the failed child; siblings are independent |
| `one_for_all` (1) | `[0..n)` — all children; shared fate |
| `rest_for_one` (2) | `[f..n)` — the failed child + everything started after it (later children depend on earlier) |

Boundary behavior verified by test: `rest_for_one` on the first child ≡ `one_for_all`; on the last ≡ `one_for_one`; an out-of-range `failed` restarts nothing.

`on_failure(s, failed, n) -> (Supervisor, [i64])` applies the strategy and the **max-restart-intensity backoff**: each failure increments the restart count; once it *exceeds* `max_restarts` the supervisor latches `halted` and restarts nothing — the OTP "too many restarts in the window" rule that abandons a crash loop instead of restarting forever (the corrigibility latch, like `supervisor.ax`'s single-agent kill-switch). The latch is one-way (mirrors R9 `@[corrigible]` / I-13's no-opt-out posture): a halted supervisor's count is frozen and every later failure is a no-op.

**Slice status:** userland landed + gated (`supervisor_tree_stdlib_module_tests_pass`, 8 `@[test]`s; the single-agent `supervisor.ax` is now also gated, 5 tests). The **kernel** `Supervisor` runtime service — actually (re)starting live child processes/fibers under the scheduler, with the restart strategy enforced by the runtime rather than computed by a userland fn — is the named Phase-7 follow-on (depends on `scheduler`, also Phase-7 TCB). No error codes (the userland core is total). No invariant changed; the intensity latch *uses* the same one-way-halt posture as I-13/R9.

---

## Appendix B — Store consistency variants (sibling Phase-7 runtime primitive)

A third Phase-7 runtime-services primitive (ROADMAP §6 row 7): *"`Store<T, Consistency, Lifetime>` ships with at-least-once and linearizable variants."* `examples/stdlib/store.ax` models the **Consistency** axis — the one with observable, testable semantics — over an i64 accumulator. (`T` fixed to i64; **Lifetime** = the persistence backend, orthogonal, already demoed by `persistent_learner.ax`'s on-disk sidecar.)

The **decisive design point** is to make the two variants differ in *exactly one observable*: how they treat a **retried operation** (an op delivered more than once — the everyday hazard whenever delivery is at-least-once and a client resends because it didn't see the ack). Modeling the divergence on op-replay, rather than on some internal flag, is what makes the consistency contract testable in a single assertion:

| Variant | retried `op_id` (already applied) | bookkeeping |
|---|---|---|
| `at_least_once` (0) | **re-applied** — double-counts; idempotency is the *caller's* job | none (no exactly-once promise to keep) |
| `linearizable` (1) | **deduped** — no-op, applied exactly once | `seen` set + monotonic `version` (the total-order stamp) |

The headline test (`test_retry_diverges_by_consistency`) applies the *same* retried op to both and asserts the resulting states **disagree** — that divergence is the entire reason the axis exists. `version` gives linearizable a single total order (bumps once per *distinct* applied op, not on a deduped retry); at-least-once maintains none (you cannot read an order out of it).

**Slice status:** userland landed + gated (`store_stdlib_module_tests_pass`, 7 `@[test]`s). The **kernel** `Store<T,C,L>` runtime service — a real persistent store whose consistency variant is *enforced by the runtime* (durable dedup log for linearizable; durable replay for at-least-once) across processes/nodes — is the named Phase-7 follow-on, and the distributed constructors (`Replicated`/`Quorum`/`CRDT`, ROADMAP Tier-2 Phase-14) build on it. No error codes; no invariant changed. This is the safety-relevant *semantics* of the store made explicit and test-pinned ahead of the kernel service.
