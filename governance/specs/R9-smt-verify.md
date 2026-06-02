# Tech Spec — R9: SMT-backed `@[verify]` (Z3 refinement proof)

**Status:** ✅ Reviewed (2026-06-02)
**Requirement:** `../REQUIREMENTS.md` R9 — *Layer-1/3 alignment; Formal Verification.* The "no SMT-backed `#[verify]`" gap.
**Decisive fork:** *What does it mean to "prove" a `@[verify]` refinement statically, what fragment of functions can be discharged to an SMT solver in v1, and how is the heavy Z3 dependency isolated so it never touches the default interpreter/codegen build?* **→ Resolved below.**

---

## 1. Motivation

`@[verify(predicate)]` today is a **runtime** gate: the interpreter evaluates the predicate against the fn's return *after* it runs and panics (exit 3) if it fails (`verify.rs`, `interp.rs`). That catches a violation when it happens — but it does not *prove* the function can never violate the bound. The PRD's "Formal Verification" pillar and R9's acceptance — *"`#[verify]` refinement proven by Z3 on a sample"* — call for a **static proof**: for a function `f` with `@[verify(value OP K)]`, prove that for *all* inputs in the declared domain, `f`'s return satisfies `value OP K`, or produce a concrete counterexample input that violates it.

This is the difference between "we checked this run" and "this can never fail". For an alignment-critical gate (a deploy bar, a safety invariant), the latter is the real guarantee.

## 2. Requirement link

`../REQUIREMENTS.md` **R9** (62%). Quoted acceptance: *"`#[verify]` refinement proven by Z3 on a sample."* This slice delivers the proof path for a **bounded fragment** (constant + linear-arithmetic returns over integer inputs), discharged to Z3, behind an opt-in `smt` feature. The runtime gate is unchanged; the SMT proof is an *additional, stronger* check the user can request.

## 3. Surface

```axon
// A verify bound the SMT checker can PROVE for all i64 inputs.
@[verify(value >= 0)]
fn abs_diff(x: i64) -> i64 {
    if x >= 0 { x } else { 0 - x }     // SMT: ∀x. result >= 0  → proven
}

// A bound that does NOT hold — SMT finds a counterexample.
@[verify(value >= 0)]
fn bad(x: i64) -> i64 { x - 1 }        // SMT: x=0 → result -1 < 0 → E1102 + counterexample
```

```
axon check prog.ax                 # runtime @[verify] gate (today) — unchanged
axon verify prog.ax                # NEW: discharge each @[verify] to Z3, prove or counterexample
                                   # (requires building with --features smt; else a clear notice)
```

No `.ax` surface change — `@[verify]` is the existing attribute. `axon verify` is the new proof driver.

## 4. Semantics

### 4.1 The provable fragment (the fork resolution)

v1 proves `@[verify(value OP K)]` for functions whose body is an **integer expression over the parameters** built from: integer literals, parameter refs, `+ - *` (multiplication by a constant — keep it linear/decidable), comparison, and `if/else` (translated to SMT `ite`). The function is encoded as an SMT term `R(params)`; the verification condition is `∀ params. R(params) OP K`. Z3 checks the **negation** `∃ params. ¬(R(params) OP K)`:
- `unsat` → no violating input exists → **proven** (the bound holds for all inputs).
- `sat` → Z3's model is a **counterexample** (concrete param values that violate the bound) → **E1102**, reported with the inputs.

Anything outside the fragment (loops, calls to other fns, non-linear, float, string) → **not provable in v1**, reported as `Unsupported` (W1103), NOT a failure — the runtime gate still applies. This is the honest boundary: a clear "can't prove this yet" beats a false "proven".

### 4.2 Why a fragment, not everything

General program verification is undecidable; a useful v1 proves the *decidable, common* case (bounded integer arithmetic — exactly the shape of most numeric `@[verify]` bounds) and is explicit about the rest. Loops/recursion need invariants (a Phase-N extension); calls need summaries. Scoping to straight-line + `ite` integer arithmetic is the largest fragment Z3 discharges without invariant inference.

### 4.3 Dependency isolation (a hard constraint)

The `z3` crate (links system `libz3`) is added **only** under a new `smt` feature, OFF by default — mirroring `asi-runtime`. The default `cargo build` (codegen) and the interp build are **untouched**: no z3 symbol, no link, no compile-time cost. `axon verify` without the feature prints a clear "build with --features smt" notice and exits 0 (no-op), so the binary still works everywhere. This respects the project's dependency discipline (the serde×codegen collision lesson, `BUILD_RESOLVED.md`).

### 4.4 Determinism

Z3 is deterministic for a fixed query (same VC → same unsat/sat+model). The encoding is a pure function of the AST. So `axon verify` is reproducible.

### 4.5 Behavior table

| Function shape vs bound | Result |
|---|---|
| straight-line/`ite` integer arith, bound holds ∀ inputs | **proven** (unsat negation) |
| same, bound violated for some input | **E1102** + counterexample model |
| loops / recursion / other-fn calls / float / string | **W1103 Unsupported** (runtime gate still applies) |
| built without `--features smt` | clear notice, no-op exit 0 |

## 5. Type rules

No type changes. The SMT encoder reads the existing AST; `@[verify]` keeps its shape. The encodable domain is `i64` params + `i64` return (the fragment); a non-i64 fn is `Unsupported`.

## 6. Error codes

| Code | Trigger | Message |
|---|---|---|
| **E1102** | SMT found a counterexample: the `@[verify]` bound is violable | `` @[verify] bound `{pred}` is violated for `{fn}` at {inputs} (SMT counterexample) `` |
| **W1103** | The fn is outside the provable fragment (loop/call/float/…) | `` @[verify] on `{fn}` not statically provable (v1 proves straight-line integer arithmetic); runtime gate still applies `` |

(E1101 "verify bound not satisfied at runtime" already exists; E1102 is the *static* sibling.)

## 7. Invariants touched

- **I-2 (interpreter is reference):** the SMT proof is an *additional* static guarantee, not a replacement for runtime semantics; a `proven` fn behaves identically at runtime. **Preserved.**
- **Determinism:** the proof is reproducible (§4.4).
- No invariant changes — this adds a verification capability, gated behind a feature.

## 8. Test plan

Red test that must fail first: **`smt_proves_nonneg_and_finds_counterexample`** — `@[verify(value >= 0)]` on `if x>=0 {x} else {0-x}` proves (unsat negation); on `x - 1` finds the counterexample `x=0`. Fails today (no SMT path). *(Gated on `--features smt`; `#[cfg(feature="smt")]`.)*

- [ ] **Unit (smt):** the AST→SMT encoder for literal / param / +,-,* / ite; the VC builder negates the bound; unsat→proven, sat→counterexample.
- [ ] **Proof:** a true bound → proven; a false bound → counterexample with concrete inputs.
- [ ] **Unsupported:** a fn with a loop or a call → W1103, not a false proof.
- [ ] **Isolation:** the DEFAULT build (no `smt`) still compiles, and `axon verify` without the feature is a clean no-op notice — proven by the standard gate (which never enables `smt`).

## 9. Acceptance criteria

- [ ] `smt_proves_nonneg_and_finds_counterexample` passes under `--features smt`.
- [ ] `smt_reports_unsupported_for_loops` passes.
- [ ] default build unaffected (gate green without `smt`).

R9 may rise 62% → ~75% on this slice (the SMT proof path for the integer fragment). Loop/recursion invariants, float theory, and inter-procedural summaries remain.

## 10. Scope / non-goals

- **In:** `smt` feature + `z3` dep (system libz3); AST→SMT encoder for the straight-line integer fragment; `axon verify` driver; E1102/W1103; tests under `#[cfg(feature="smt")]`.
- **Out:** loops/recursion (need invariants); non-linear / float / bitvector theories; inter-procedural; changing the runtime gate; enabling `smt` by default.
