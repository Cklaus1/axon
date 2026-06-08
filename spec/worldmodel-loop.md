# Axon Spec — Executable World Models + the Compress-to-Simplest Loop

**Goal**: Make Axon a first-class substrate for the *world-model / skill-acquisition* loop —
a program (often agent-authored) that **predicts**, is **checked against observations**, and
is **compressed toward the simplest program that fits** the data (MDL; ARC-AGI "skill-
acquisition efficiency"). The base model is the *proposer*; Axon is the fixed *verifier +
fitness function* every base-model gain rides on.

**Builds on (all shipped on `merge-asi-layer3`)**:
- `@[adaptive] fn(i64)->i64` + `goal_run(name, target, max_evals)` — the search/optimizer
  (interp `run_goal` hill-climb; `examples/asi/optimize.ax` is the canonical loop).
- `axon complexity` — the MDL description-length metric over the AST (the "measure of
  simplest"; `crates/axon-core/src/complexity.rs`, `axon-complexity/1` JSON).
- Refinement runtime checks at all four obligation sites — the "fit-to-observation" predicate
  enforcement (`T where P`, exit 6 on violation, I-2 byte-identical).
- The R10 self-improving-compiler gate harness (G1 interp oracle / G2 cap-diff / G3
  regression) — the precedent that "an improvement is admitted only after the gates prove it".

**Status of this doc**: design spec for **prototype #1**. Defines the surface, the loop
semantics, and the resolved design forks. Implementation is sequenced in §7; v1 is
deliberately a *userland* realization on shipped primitives (the same two-track move as
`goal.ax`/`agent.ax` landing ahead of kernel services), with the kernel `World<T>` type
named, not faked.

---

## Scope

### In scope (v1)
- A **`World` userland module** (`examples/stdlib/world.ax`): a first-class value bundling a
  model's parameters, a `predict` function, and an observation set, with `observe` /
  `fit_error` / `refine` operations expressed on shipped primitives.
- The **compress objective**: `goal { minimize complexity, subject_to: fits_observations }`
  realized as a scoring function `fitness = fit_term − λ·complexity_term` that `goal_run`
  maximizes — turning "simplest program that fits" into a concrete, optimizable scalar.
- A **first-class example** (`examples/asi/world_model.ax`): predict → observe → refine over a
  small synthetic system, printing the description-length shrinking as fit holds.
- A **`complexity` library entry point** so a running program can read its own / a candidate's
  MDL bits (today `axon complexity` is CLI-only; v1 exposes `program_complexity` to the
  interpreter as a builtin `complexity_bits(...)` over an AST handle — see §4 fork F3).

### Explicitly out of scope (named, not built)
- The **kernel `World<T>` / `Counterfactual<T>` types** with first-class `observe`/`condition`
  keywords (that is Phase-13 probabilistic + a dedicated type; v1 is a userland struct).
- **Free-form agent authorship** of the world-model program (the proposer writing arbitrary
  `.ax`) — gated exactly like R10 discovery: the agent proposes, the gates dispose. v1 uses a
  bounded parameter search, not arbitrary code synthesis.
- A **probabilistic** fit term (`P(_ ≤ k)`, likelihoods) — v1 fit is a deterministic error
  metric; the Bayesian form is Phase-13.

---

## 1. The loop, precisely

```
            ┌──────────────────────────────────────────────┐
            │  proposer (LLM or bounded search)            │
            │   emits candidate model parameters / form    │
            └───────────────────┬──────────────────────────┘
                                │  candidate
                                ▼
   ┌─────────────────────────────────────────────────────────────┐
   │  Axon — the fixed verifier + fitness function                │
   │                                                              │
   │  1. predict(candidate, inputs)        → predictions          │
   │  2. observe: compare to observations  → fit_error  (lower=better) │
   │  3. fits?  refinement check `error <= tol`  (T where P)       │
   │  4. complexity_bits(candidate)        → description length    │
   │  5. fitness = fit_score − λ·complexity   (the scalar to MAX)  │
   └───────────────────┬──────────────────────────────────────────┘
                        │  fitness  (recorded by goal_run provenance)
                        ▼
            goal_run hill-climbs → keeps the candidate with the best
            fitness that ALSO satisfies the fit refinement.
```

The key property: **a candidate that does not fit (fails the refinement) is disqualified
regardless of how simple it is** — exactly as `goal.ax`'s hard `Constraint` disqualifies an
under-target agent. Simplicity is maximized *subject to* fit, never traded against it. This is
what stops the compression loop from compressing into something subtly wrong (the soundness
concern the thread raised: a naive "shortest program" reward degenerates without the fit
constraint as a hard gate).

## 2. `World` userland module surface (`examples/stdlib/world.ax`)

```axon
// A world model: parameters + a prediction rule + the observations to fit.
// v1 models a 1-parameter family (slope) over integer (x → y) samples; the
// shape generalizes to a parameter vector via the multi-arg goal_run path.
type World = {
    slope: i64,            // the model parameter(s) — the thing search varies
    n: i64,                // number of observations
}

// predict(w, x) — the executable model. Pure; this is the "program".
fn predict(w: World, x: i64) -> i64 { w.slope * x }

// observation accessors (a held-out dataset; in a real demo, loaded/extracted).
fn obs_x(i: i64) -> i64 { i }
fn obs_y(i: i64) -> i64 { 3 * i }   // ground truth slope = 3 (unknown to search)

// fit_error(w) — total absolute prediction error over the observations.
// LOWER is better; 0 = perfect fit. This is the "checked against observations".
fn fit_error(w: World) -> i64 {
    let mut e = 0
    for i in 0..w.n {
        e = e + abs_i64(predict(w, obs_x(i)) - obs_y(i))
    }
    e
}

// A refinement makes "fits" a TYPE-level contract: a FittedWorld is one whose
// error is within tolerance. Constructing one with a bad slope is a refinement
// violation (exit 6) — fit is enforced, not hoped.
type FittedWorld = World where fit_error(_) <= 0
```

`abs_i64`, `for`, struct field access, and whole-struct refinements are all shipped. The
`where fit_error(_) <= 0` is a whole-struct refinement calling a `@[pure]` helper — exactly
the predicate subset the refinement runtime check + (eventually) SMT support.

## 3. The compress objective on `goal_run`

`goal_run` maximizes the score of an `@[adaptive] fn(i64)->i64`. We encode the candidate
parameter as that i64 and return the fitness:

```axon
// NOTE the verified API shapes (confirmed against builtins.rs):
//   - goal_run(name, target: f64, max_evals: i64) -> f64   (returns best SCORE)
//   - goal_best_input(name, target: f64) -> i64            (returns best INPUT)
//   - @[adaptive(metric, target)] target is an f64 literal (`1000.0`)
//   - @[pure] is TRANSITIVE: predict/obs_y/fit_error must all be @[pure].
@[adaptive(metric: world_fitness, target: 1000.0)]
fn world_fitness(slope: i64) -> i64 {
    let w = World { slope: slope, n: 8 }
    let err = fit_error(w)
    let bits = complexity_bits_of_world(w)     // §4: the MDL term
    // fitness = a large constant for fit, minus error, minus a small complexity
    // penalty. A perfect fit (err=0) dominates; among equal-fit candidates the
    // simpler (fewer bits) wins. λ small so fit always outranks simplicity.
    1000 - 100 * err - bits
}

fn main() -> i64 {
    let best_score = goal_run("world_fitness", 1000.0, 64)        // hill-climb 64 evals
    let best_slope = goal_best_input("world_fitness", 1000.0)     // recover the learned param
    let w = World { slope: best_slope, n: 8 }
    println("learned slope {to_str(best_slope)}, error {to_str(fit_error(w))}")
    fit_error(w)   // exit 0 iff a perfect-fitting model was found
}
```

This is "minimize complexity subject to fits_observations" expressed on the shipped optimizer:
the `1000 - 100*err` term makes any unfit candidate score far below any fit one (fit is the
hard gate), and the `- bits` term breaks ties toward the simplest. **No new optimizer is
needed** — the contribution is the *fitness encoding* + the complexity term. **Verified**: with
`n=8` observations and ground-truth slope 3, `goal_run` recovers slope 3 (score 1000, error 0,
exit 0) reliably across seeds.

## 4. New builtin: `complexity_bits` (the metric, callable at runtime)

Today `program_complexity` (complexity.rs) is reachable only from the `axon complexity` CLI.
v1 exposes the MDL measure to a running program so the loop can score candidates.

**Resolved fork F3 — what does `complexity_bits` measure?** A candidate world model in v1 is a
*parameter*, not a distinct AST, so a per-candidate AST measure is degenerate (all candidates
share the `predict` AST). v1 therefore measures the **encoded parameter cost** — `int_bits` of
each parameter (the description length of the *learned content*), reusing `complexity::int_bits`.
This is the MDL-honest term for a parametric model: a model with slope `3` is cheaper to
describe than slope `1000000`, and a 1-parameter model is cheaper than a 5-parameter one.
(When prototype #1 graduates to agent-authored *program forms*, `complexity_bits` switches to
the full `program_complexity` over the candidate's AST — the metric is already built; only the
binding target changes.)

```
complexity_bits_of_world(w: World) -> i64   // userland helper: sum int_bits(param)
```

Implemented in v1 as an `.ax` helper over the shipped `abs_i64` + a tiny bit-count (no new
kernel builtin needed for the userland slice); a kernel `complexity_bits(<ast handle>)`
builtin is the Phase-13 form when candidates are programs.

## 5. Design forks (resolved)

- **F1 — fit vs simplicity: trade-off or hard gate?** → **Hard gate.** Fit is a refinement
  (`FittedWorld`) / a dominating fitness term; simplicity only breaks ties among fitting
  candidates. Rationale: a tradeoff lets the loop "win" by being simple-and-wrong (the thread's
  soundness concern). Mirrors `goal.ax`'s hard `Constraint`.
- **F2 — who proposes candidates?** → **v1: bounded `goal_run` search** over the parameter
  space (no code synthesis). Free-form proposal is the agent layer, gated like R10 discovery
  (propose→verify→keep). Keeps v1 sound and shippable; names the agent path.
- **F3 — what does `complexity_bits` measure?** → **v1: encoded parameter cost** (`int_bits`
  of the learned parameters); **future: full AST `program_complexity`** when candidates are
  programs. (See §4.)
- **F4 — `World` a kernel type or userland struct?** → **v1: userland struct** (`world.ax`),
  same two-track precedent as `goal.ax`/`agent.ax`. Kernel `World<T>` + `observe`/`condition`
  keywords are Phase-13.
- **F5 — deterministic or probabilistic fit?** → **v1: deterministic error metric**
  (`fit_error`); probabilistic likelihood is Phase-13 (needs `Distribution<T>`).

## 6. Implementation plan (v1, sequenced)

1. **`examples/stdlib/world.ax`** — the `World`/`predict`/`fit_error`/`FittedWorld` module +
   `@[test]`s: a perfect-fit slope yields error 0; a wrong slope yields error > 0; constructing
   a `FittedWorld` from a wrong slope is a refinement violation (exit 6); `complexity_bits` of
   slope 3 < slope 1000.
2. **`examples/asi/world_model.ax`** — the end-to-end loop: `goal_run` over `world_fitness`
   finds the ground-truth slope, prints the shrinking description length as fit is achieved.
   Gated in `cli_run.rs` (runs clean, exit 0; the `stdlib_module_acceptance_suites_pass` glob
   already covers `world.ax`'s tests).
3. **(optional, if F3-kernel pulled forward)** a `complexity_bits` interpreter builtin over an
   AST handle — deferred to Phase-13 per F3; v1 stays userland.
4. **Docs**: CHANGELOG + ROADMAP §10.5 ("prototype #1 landed: userland world-model +
   compress-to-fit loop on shipped primitives").

## 7. Verification checklist

- [ ] `axon test examples/stdlib/world.ax` — all `@[test]`s pass (fit math, refinement
      violation exits 6, complexity ordering).
- [ ] `axon run examples/asi/world_model.ax` — finds the ground-truth slope, exit 0, prints a
      monotone-shrinking description length.
- [ ] I-2: `axon build examples/asi/world_model.ax` runs byte-identically to `axon run`
      (it uses only shipped, codegen-supported primitives; whole-struct refinement on a
      `@[pure]` call must lower or honestly E0910-refuse).
- [ ] `axon complexity examples/asi/world_model.ax --json` reports a sensible MDL figure.
- [ ] `scripts/gate.sh --strict` green; the stdlib-acceptance glob covers `world.ax`.

## 8. Carry-forward

- **Phase-13**: kernel `World<T>`/`Distribution<T>`/`Counterfactual<T>`, `observe`/`condition`/
  `sample` keywords, probabilistic fit (F5), AST-level `complexity_bits` (F3).
- **Agent layer**: free-form candidate proposal gated like R10 discovery (F2) — the proposer
  writes a candidate, the fit refinement + complexity objective + the gate harness dispose.
- **Self-improving compiler tie-in**: a graduated *program* candidate IS a self-improvement in
  the R10 sense (behavior-preserving + simpler) — the two loops converge on one gate.
