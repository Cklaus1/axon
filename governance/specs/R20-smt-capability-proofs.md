# SMT-Backed Capability Attenuation Proofs (mint / budget)

**Spec ID:** `R20-smt-capability-proofs`
**Status:** ✅ Slices 0–3 LANDED (2026-06-25). Slice 0 spike (mint O1), Slice 1 (full O1∧O2 proof + `kernel.rs` differential grid tripwire + `Discharged` wiring + E1610 build gate + `smt_mint_parity.sh`), Slice 2 (relational refinements for user attenuating fns — params threaded into the predicate encoder + runtime param-scope fix; `examples/refinement_attenuation.ax`), Slice 3 (TCB attestation: content-addressed `MINT_OBLIGATION_SPEC ⊕ verdict` digest pinned in `TCB_MANIFEST_DIGEST`, boot-checked → E1611). 12 R20 tests; smt suite green.
**Risk class:** Structural (extends the SMT encoder; strengthens I-11, mechanizes I-12)
**Requirement tie:** `REQUIREMENTS.md` R6 (capability/registry) + `governance/specs/R11-capability-minting.md` (the runtime minting this proves) + ROADMAP §7 TCB (`smt_solver`, `capability_minter`).
**Author / date:** scoped by Claude, 2026-06-25.

---

## 1. The gap, in one line

`PrincipalRegistry::mint` (`crates/axon-core/src/kernel.rs:122`) enforces capability
attenuation and budget conservation **by runtime construction** (`c_net = want_net &&
parent.net`; `grant = budget_grant.max(0).min(parent.budget.remaining())`), and three unit
tests assert it on *example* inputs (`kernel_mint_cannot_escalate:664`,
`kernel_budget_is_carved_from_parent:678`, `kernel_mint_matches_oracle_subset:650`). That is
**enforcement, not proof**: the contract holds because the current code happens to compute it,
and the tests check a handful of points. Nothing proves it holds **∀ inputs**, and nothing
stops a future edit to `mint` from silently weakening it (an I-12 hole — "self-modification
cannot weaken the TCB" is asserted but not mechanized for the minter).

This spec promotes the mint/budget contract from *runtime-clamped + spot-tested* to
**SMT-proven for all inputs**, wired into the same default-pipeline discharge machinery that
already elides proven `@[verify]`/refinement obligations (`verify.rs` `Discharged`,
`phase5-smt-discharge`). It is the first slice of "promote Principal/Budget/Effect/Sandbox to
kernel-enforced TCB primitives," and the foundation the capability-proof-receipt artifact
renders.

---

## 2. The proof obligations (formal)

Let `mint(parent, want_net, want_fs, want_exec, grant) → child` with the semantics in
`kernel.rs:122`:

```
c_net  = want_net  ∧ parent.net
c_fs   = want_fs   ∧ parent.fs_write
c_exec = want_exec ∧ parent.exec
rem    = max(0, parent.cap − parent.used)        // parent.budget.remaining()
g      = max(0, min(grant, rem))                 // clamp(grant, 0, rem)
child  = { net: c_net, fs_write: c_fs, exec: c_exec, budget: {used: 0, cap: g} }
parent.used' = parent.used + g                   // carve in place
```

Obligations to discharge **∀ parent state, ∀ request**:

- **O1 — Capability attenuation (I-11).**
  `child.net ⇒ parent.net  ∧  child.fs_write ⇒ parent.fs_write  ∧  child.exec ⇒ parent.exec`.
  A child can never hold a capability the parent lacks.

- **O2 — Budget non-inflation / carve (conservation).**
  `child.cap ≥ 0  ∧  child.cap ≤ rem  ∧  rem_after(parent) = rem − child.cap`,
  i.e. `rem_after(parent) + child.cap = rem` — total remaining budget across {parent, child}
  is conserved, never created.

- **O3 — Transitive seal (derived lemma, not a per-call obligation).**
  By induction over O1∧O2: any descendant's caps ⊆ every ancestor's caps, and Σ budgets over a
  subtree ≤ the root's. Proven once as a lemma; not re-checked per mint.

All three are **linear-integer + boolean** — trivially inside a decidable theory; Z3 dispatches
them instantly. **100% of the work is in the *encoding*, not the solving.**

---

## 3. Why this is new encoder surface (the real risk)

`smt.rs` today (`prove_one_int_conjunction:109`, `prove_one_f64:208`) encodes **one function
body as a single Int/Real term over its params** and proves `term OP constant`. The mint
obligation is outside that fragment in three ways, each a small, bounded extension:

| Need | Today | Extension |
|---|---|---|
| **Boolean atoms** (net/fs/exec, `∧`, `⇒`) | Int + Real fragments only | Add a `z3::ast::Bool` param/term path (mirrors the existing Int path's structure). |
| **Relational** (output vs input) | `body OP const` | Encode a *pair* of states (pre `parent`, post `child`) and prove a predicate relating them. |
| **Struct-valued** (4 fields) | scalar params | Model a Principal as a tuple of named Z3 consts (`p_net: Bool, p_cap: Int, p_used: Int, …`). |

This is the "struct whole-refinement / relational" class the Phase-5 notes deferred to runtime
(`phase5-pure-attribute`: "struct-whole refinements ... runtime-enforced by design"). R20 is
exactly the static-discharge of that class, scoped to the one highest-value instance (mint)
first. **Slice 0 below de-risks the encoder extension before any wiring.**

---

## 4. Design

Two ways to source the contract; this spec recommends **(B) a direct prover for the fixed
kernel primitive** for slice 1, with **(A) generalized relational refinements** as the
follow-on that makes it user-facing.

- **(A) Refinement-contract route.** State O1/O2 as relational refinements on the Axon oracle
  `examples/stdlib/principal_mint.ax` and the kernel `principal_mint` builtin signature; the
  extended encoder discharges them; the existing kernel==oracle parity tests bridge to the Rust
  impl. *Cleanest long-term (keeps I-2: the proof is about Axon-level semantics the interpreter
  defines), but needs relational-refinement surface syntax — heavier.*
- **(B) Direct TCB prover.** Add `prove_mint_obligations()` to `smt.rs` that encodes mint's
  **fixed, known** semantics (§2) directly into Z3 and proves O1+O2. Register the result in
  `Discharged` as a TCB lemma. *Simpler and more robust for a fixed primitive; no new surface
  syntax; the obligation text lives next to the code it constrains.* **Recommended for slice 1.**

**What discharge buys (runtime behavior is unchanged — I-2 safe):**
1. **Elision.** A proven O1/O2 lets the interpreter skip the `can_mint` pre-check / clamp guard
   as redundant (it provably can never fire) — same as proven `@[verify]` elision today. Default
   (smt off) behavior is byte-identical; the proof only removes a check that was always true.
2. **The I-12 tripwire.** If a future edit to `mint` makes O1 or O2 **unprovable or false**, the
   prover fails and emits **E1610** at build/load — the minter cannot be silently weakened. This
   is the mechanized form of "self-modification cannot weaken the TCB."

No codegen impact: the kernel registry is interp-only; native already E0910-refuses
`principal_mint`/`sandbox_*`. The proof runs under the opt-in `smt` feature and (per the
existing wiring) feeds the default pipeline's `Discharged` set.

---

## 5. Slices

| # | Scope | Effort | Gate |
|---|---|---|---|
| **0** | **Encoder spike.** Add the `Bool` + struct-tuple + two-state (pre/post) path to `smt.rs`; prove **O1 only** (pure boolean attenuation) for mint in isolation. | S | A `#[cfg(feature="smt")]` unit test in `smt.rs` proving `child.net ⇒ parent.net` ∀ (UNSAT on the negation). De-risks the whole spec — if Z3 boolean/relational encoding is awkward, it surfaces here for ~a day's cost, not after the wiring. |
| **1** | **Full obligation + wiring.** Encode O1+O2 (`prove_mint_obligations`); add to `Discharged`; elide the redundant runtime guard when proven; emit **E1610** when an obligation is unprovable/false. | M | New `scripts/smt_mint_parity.sh`: (a) under `smt`, O1+O2 are discharged; (b) default==smt run/exit byte-identical (mirrors `smt_discharge_parity.sh`); (c) a **mutation test** — a deliberately-broken `mint` (e.g. `c_net = want_net`, dropping `&& parent.net`) is REJECTED with E1610 (proves the tripwire bites). |
| **2** | **Generalize (route A).** Relational-refinement surface so user-written attenuating fns get the same proof: `fn f(p: Principal, …) -> (Principal where _.net implies p.net ∧ _.budget.cap <= budget_remaining(p))`. Makes R20 not mint-only. | S | Discharge a userland attenuating fn in `principal_mint.ax`; runtime fallback (exit 6) for the out-of-fragment case. |
| **3** | **Attestation hook (I-12).** Record the discharged mint obligation in the TCB manifest (content-address the `prove_mint_obligations` result), so "un-weakenable" is a boot-time check, not only a CI test. | S | Manifest entry present + a boot-mismatch test. Aligns with ROADMAP §7 attestation. |

Slice 0 → 1 is the committed core (effort ~M total). Slices 2–3 are independent follow-ons.

---

## 6. Error codes

- **E1610** — *capability-mint obligation could not be statically discharged* (proven false, or
  the minter was modified out of the provable fragment). Kernel E16xx band (ROADMAP §7; E1604 =
  kernel-goal budget). Hard build/load error under `smt`; the I-12 minter tripwire.

(No new runtime exit code: a *proven* obligation elides a check; an *unprovable* one is a
compile-time E1610, not a runtime fall-through, because mint is a fixed TCB primitive that must
always satisfy it — unlike user refinements, which fall back to runtime exit 6.)

---

## 7. Invariants touched

- **I-11 (capability boundary is total)** — **strengthened**: attenuation moves from
  runtime-clamped to ∀-inputs-proven.
- **I-12 (self-modification cannot weaken the TCB)** — **mechanized** for the minter (E1610
  tripwire in slice 1; boot attestation in slice 3). Previously asserted, not enforced.
- **I-2 (interpreter is reference; default behavior stable)** — **preserved**: discharge only
  elides an always-true runtime check; default (smt-off) runs byte-identical. The slice-1 gate
  asserts this explicitly (default==smt).

---

## 8. Non-goals

- Proving the scheduler / supervisor / LLM-gateway obligations (separate specs; same machinery).
- The f64 budget variant — kernel `Budget` is i64 (`kernel.rs:24`).
- User-facing relational-refinement syntax beyond slice 2's narrow form.
- Any codegen/native work — the kernel registry is interp-only by design.

---

## 9. Open questions (resolve before slice 1 opens)

| Q | Default |
|---|---|
| Route A (relational refinements) vs B (direct prover) for slice 1 | **B** — fixed primitive, no new surface syntax; A is slice 2. |
| Is E1610 a hard error, or a warning when `smt` is off? | Hard under `smt`; **silent no-op when `smt` is off** (matches `Discharged` default-empty: no feature ⇒ no obligation ⇒ unchanged behavior). |
| Should O3 (transitive seal) be proven now or deferred? | **Defer** — it's a derived lemma over O1∧O2; prove once O1/O2 land, only if a consumer needs the transitive form. |
| Encode the in-place parent debit (`used'`) as part of O2, or a separate framing obligation? | Fold into O2 as `rem_after = rem − child.cap`; no separate frame needed (mint touches only the parent's `used`). |
