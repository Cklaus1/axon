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
