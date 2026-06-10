# Tech Spec — R9b: SMT `@[verify]` over loops (invariant inference)

**Status:** 📋 Planned / Not Started (Draft 2026-06-03) — the design is resolved (see the
decisive fork in `README.md`) but NO implementation exists: `smt.rs` discharges only
straight-line `i64`/`f64` fragments (`encode_block` returns `Unsupported` for While/For), and
all §-acceptance criteria below are unchecked. This is correctly a 0%-built item, distinct from
R9's shipped straight-line SMT.
**Requirement:** `../REQUIREMENTS.md` R9 — *Layer-1/3 alignment; Formal Verification.* Extends `R9-smt-verify.md` (straight-line integer + float fragment, ✅ Reviewed/landed) past its hard boundary: loops.
**Parent boundary:** R9-smt §4.2 / §10 scope loops OUT — *"Loops/recursion need invariants (a Phase-N extension)."* This is that extension.

---

## 1. Motivation

R9's SMT path (`--features smt`, Z3) PROVES `@[verify(value OP K)]` for straight-line integer/float bodies (`+ - *`, `ite`). The single largest unprovable-but-common shape it punts to `W1103 Unsupported` is **the loop** — `while`/`for` accumulators, the exact bodies a numeric `@[verify]` bound most wants to guard (a running sum stays in range, a counter never overflows a cap, a decay stays ≥ 0). Proving a loop requires a **loop invariant**: a predicate true on entry, preserved by each iteration, and strong enough to imply the post-condition. General invariant *synthesis* is undecidable; the tractable v1 is a **bounded, template-driven inference** over the same decidable integer fragment R9 already encodes.

## 2. Requirement link

`../REQUIREMENTS.md` **R9** (78%). Acceptance anchor unchanged: *`@[verify]` bounds proven by Z3*. This widens the proven fragment from straight-line to **single-loop linear** bodies. Dependencies: **R9-smt** (the encoder, `smt.rs`, Z3 Int/Real — reused wholesale), **I-2** (interp is reference — a `proven` loop must behave identically at runtime).

## 3. Surface (what the user writes / runs)

No new syntax. The *same* `@[verify(value OP K)]` now also discharges a function whose body contains one loop:

```axon
@[verify(value >= 0)]
fn clamp_decay(start: i64, steps: i64) -> i64 {
    let mut acc = start
    let mut i = 0
    while i < steps {
        if acc > 0 { acc = acc - 1 }   // monotone non-increasing, floored at 0
        i = i + 1
    }
    acc                                 // invariant: acc >= 0 ∧ acc <= start
}
```

```
axon verify clamp_decay.ax --features smt   # PROVES value>=0 via inferred invariant
```

## 4. Semantics

### 4.1 The provable fragment (the fork resolution)

**Decisive fork: inference strategy — (a) template/Houdini, (b) abstract interpretation (interval/octagon), or (c) CHC/PDR via Z3's `fixedpoint` engine?**

**→ Resolved: (a) template-driven inference, falling through to (c) Z3 CHC when templates miss — never (b) as a separate analyzer.** Rationale:
- **(a) templates** are cheap, deterministic, and explain themselves (the inferred invariant is reportable). v1 enumerates a small fixed template family over the loop's *modified* integer variables: `Σ cᵢ·vᵢ ⋈ K` and per-variable bounds `lo ≤ v ≤ hi` (ranges + linear combinations — the **octagon-expressible** invariants, which cover accumulator/counter/decay loops). For each candidate, Z3 discharges the three Hoare conditions (init ⇒ inv, inv ∧ guard ∧ body ⇒ inv′, inv ∧ ¬guard ⇒ post). A candidate surviving all three is a *proof*.
- **(c) Z3 CHC/PDR** (`z3::fixedpoint`, the Spacer engine) is the principled general fallback: encode the loop as Constrained Horn Clauses and let Z3 *synthesize* the invariant. Reserved for when no template fits, behind the same `smt` feature. It can diverge/timeout — so it runs with a wall-clock bound and, on timeout, yields `W1103 Unsupported` (never a false proof).
- **(b) abstract interpretation** is rejected as a *separate* mechanism: it would be a second semantic model to keep in sync with the encoder (an I-2-adjacent drift risk, like R10's "no second mechanism" posture). The octagon *shape* is captured as templates fed to the one trusted oracle (Z3), not as an independent fixpoint engine.

### 4.2 The loop shape v1 accepts

A function with **exactly one** `while`/`for` whose body is straight-line integer arithmetic (the R9 fragment) over a fixed set of mutable `i64` locals, with a linear guard. Nested loops, loop-carried calls, float loops, and break/continue → `W1103 Unsupported` in v1 (honest boundary, runtime gate still applies). `for v in a..b` desugars to the `while` form already in the AST.

### 4.3 Why this is still sound

Every proof obligation is discharged by **Z3 on the existing R9 encoder** — the invariant is a *witness*, not a trusted oracle. A wrong template simply fails one of the three Hoare checks and is discarded; only an invariant that Z3 confirms on all three is accepted. So R9b cannot produce a false `proven` even if the template heuristics are weak — the worst case is `Unsupported`, never an unsound proof. (This is the R10-style firewall: propose cheaply, verify with the one trusted mechanism.)

### 4.4 Determinism

Template enumeration is a fixed, ordered list; Z3 is deterministic per query. The CHC fallback runs with a fixed seed + fixed timeout (timeout → deterministic `Unsupported` on a given machine; the timeout bound is documented as machine-sensitive and kept generous). Reproducible per §R9 4.4.

### 4.5 Behavior table

| Function shape vs bound | Result |
|---|---|
| single linear loop, an octagon-template invariant proves the bound | **proven** (+ the inferred invariant reported) |
| single linear loop, no template fits but Z3 CHC synthesizes an invariant in time | **proven** (invariant from Spacer) |
| single linear loop, bound actually violable | **E1102** + counterexample (init + iteration count) |
| single linear loop, neither templates nor CHC converge in the time bound | **W1103 Unsupported** (runtime gate applies) |
| nested loops / break / float loop / loop-carried call | **W1103 Unsupported** (v1 boundary) |
| built without `--features smt` | clean no-op notice, exit 0 |

## 5. Type rules

No type changes. The encoder reads the existing AST; the loop's modified-variable set is computed by a straightforward assignment scan (already needed for the borrow checker). Domain: `i64` locals (the v1 fragment); a float loop is `Unsupported` (Z3 Real loop invariants are a follow-on).

## 6. Error codes

| Code | Trigger | Message |
|---|---|---|
| **E1102** | (reused) loop bound violable; counterexample = initial state + iteration count | `` @[verify] bound `{pred}` is violated for `{fn}` at {init} after {n} iterations (SMT counterexample) `` |
| **W1103** | (reused) loop outside v1 (nested / break / float / no invariant found in time) | `` @[verify] on `{fn}`: could not infer a loop invariant (v1: single linear i64 loop); runtime gate still applies `` |

No new error band — R9b reuses E1102/W1103. (The *reason* string distinguishes "out-of-fragment" from "invariant search exhausted".)

## 7. Invariants touched

- **I-2 (interp is reference):** the loop proof is an additional static guarantee; a `proven` loop runs identically. **Preserved** — the invariant is Z3-checked, never trusted (§4.3).
- **Determinism:** §4.4 (fixed template order + seeded/timeout-bounded CHC).
- **Dependency isolation:** still entirely under `--features smt`; the default build never links Z3. The CHC fallback uses `z3::fixedpoint` (same crate, no new dep). **Preserved.**

## 8. Test plan (maps 1:1 to §4.5)

Red test first: **`smt_proves_loop_accumulator_bound`** — `@[verify(value >= 0)]` on the `clamp_decay` body (§3) proves via an inferred `acc >= 0` invariant; an off-by-one variant (`acc = acc - 1` without the `acc > 0` guard) yields E1102 with a counterexample. Fails today (loops → W1103).

- [ ] **Unit (smt):** modified-variable scan; the three Hoare VCs built for a candidate invariant; a surviving template → proven, all-fail → next template → CHC → timeout → Unsupported.
- [ ] **Proof (template):** a monotone accumulator / bounded counter / floored decay → proven, with the invariant reported.
- [ ] **Proof (CHC fallback):** a loop no template fits but Spacer solves → proven; a deliberately hard loop → Unsupported within the time bound (no false proof, no hang).
- [ ] **Counterexample:** an unbounded accumulator vs a cap → E1102 with init + iteration count.
- [ ] **Boundary:** nested loop / `break` / float loop → W1103, not a false proof.
- [ ] **Isolation:** default build (no `smt`) unaffected — proven by the standard gate.

## 9. Acceptance criteria (the done gate)

- [ ] `smt_proves_loop_accumulator_bound` passes under `--features smt` (proof + counterexample halves).
- [ ] At least one CHC-fallback proof passes and one hard loop is `Unsupported` within the time bound (the fallback neither hangs nor lies).
- [ ] `axon verify` reports the inferred invariant on a `proven` loop (explainability).
- [ ] default build green without `smt` (Z3 never linked).

R9 may rise 78% → ~85% on this slice (single linear-loop invariants). Nested loops, float-loop Real invariants, and inter-procedural summaries remain explicitly out.

## 10. Performance budget

Template enumeration is O(small fixed family × 3 Z3 calls); the CHC fallback is bounded by a wall-clock timeout (default a few seconds, documented machine-sensitive). `axon verify` on a non-loop fn is unchanged (R9 path). No effect on `build`/`run`/`check` (Z3 is verify-only, feature-gated).

## 11. Rollout & rollback

Additive under `--features smt`: extends `smt.rs` with a loop-invariant module; the straight-line path is untouched. Rollback = the loop branch returns `W1103` (today's behavior). No surface, no default-build, no runtime change.

## 12. Open questions

- **Q1 (template family breadth):** start with intervals + 2-variable octagon relations (covers accumulator/counter/decay). Widening to general `Σcᵢvᵢ` polyhedra is a follow-on if real `@[verify]` loops need it — measure against actual demo bodies before broadening.
- **Q2 (CHC timeout value):** pick a default that proves the test corpus without hanging CI; make it overridable (`AXON_SMT_TIMEOUT_MS`) like `AXON_MAX_DEPTH`. Confirm the timeout is deterministic *enough* on the gate machine (else gate the CHC test behind a slow-tests flag and keep only the template proofs in the standard `smt` test set).
- **Q3 (reporting the invariant):** print the inferred predicate (`# proven via invariant: acc >= 0 ∧ acc <= start`) — high explainability value, low cost. In v1 for template proofs (the predicate is known); CHC-synthesized invariants are reported best-effort (Spacer's may be large).
