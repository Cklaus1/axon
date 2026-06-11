# Axon Compiler — Phase 13 Spec (ASI Layer 6)

**Goal**: Add a tractable probabilistic refinement fragment to Axon. Distributions are
first-class values; moments (`E[_]`, `Var[_]`) and tail probabilities (`P(_ ≤ k)`) appear as
predicates in refinement types and are discharged statically via interval arithmetic for
constant parameters, or checked at runtime otherwise.
**Builds on**: Phase 5 (`spec/compiler-phase5.md` — refinement types + SMT), Phase 6
(`spec/compiler-phase6.md` — row-polymorphic effects, `Random` effect row).
**Timeline**: Two slices. Slice 1 (distribution builtins + userland library) is a single
iteration. Slice 2 (language-level `E[_]`/`Var[_]`/`P(_ ≤ k)` syntax in refinement
predicates, discharged via interval arithmetic) is a follow-on iteration.

---

## Phase 13 Scope

### In Phase 13

**Slice 1 — Runtime builtins + userland library**

- 12 new builtins covering the three tractable distribution families:
  Gaussian (normal), Beta, and Categorical.
- Pure moment / CDF builtins: these are deterministic closed-form computations;
  they carry the empty effect row (`| {}`).
- Impure sampling builtins: each draw is non-deterministic; they carry `| {Random}`.
- Userland library (`examples/stdlib/distribution.ax`): struct wrappers
  (`Gaussian`, `Beta`, `Categorical`) with validated constructors, query functions,
  and sampling functions built from the builtins.
- `observe` and `sample` as builtin-level names (not keywords — no grammar change in
  Slice 1). `sample` is an alias alias for the `*_sample` builtins; `observe` records
  a sampled value against a distribution for later conditioning (deferred to Slice 2).

**Slice 2 — Language-level probabilistic refinement predicates (future iteration)**

- Predicate syntax extensions: `E[dist]`, `Var[dist]`, `P(dist <= k)` inside
  `T where <pred>`.
- Interval-arithmetic discharge for constant distribution parameters.
- Runtime fallback (exit 6, `REFINE_VIOLATION_EXIT_CODE`) when parameters are
  non-constant.
- `condition` keyword: `condition(dist, event)` narrows the distribution by a
  Boolean observation; returns the posterior as a new distribution value.
- Effect-row integration: `observe`/`condition` carry the `Random` effect row (they
  modify the probabilistic context, which is a side channel on the RNG state).

### Explicitly Out of Phase 13

```
General probabilistic SMT (full Bayesian inference)  → future
MCMC / particle filter runtime                       → future
Multivariate distributions                           → future
Mixture models                                       → future
Continuous-time / stochastic process types           → future
Belief<T> / World<T> kernel primitives               → future
Tensor autodiff / gradient through sampling          → future
```

---

## 1. The Three Tractable Families

Phase 13 targets distributions where moments and the CDF have exact closed-form
expressions that can be evaluated by the compiler's interval-arithmetic engine without
an SMT call.

| Family | Parameters | Support | E[X] | Var[X] | CDF closed-form? |
|---|---|---|---|---|---|
| Gaussian(μ, σ) | μ ∈ ℝ, σ > 0 | ℝ | μ | σ² | Yes (error function) |
| Beta(α, β) | α > 0, β > 0 | [0, 1] | α/(α+β) | αβ/((α+β)²(α+β+1)) | Yes (incomplete beta) |
| Categorical(p₀…pₙ) | pᵢ ≥ 0, Σpᵢ = 1 | {0…n-1} | Σ(i·pᵢ) | Σ(i²·pᵢ) − E[X]² | Yes (prefix sum) |

"Closed-form" here means the interval-arithmetic engine can bracket the result to
arbitrary precision using only the four arithmetic operations, square root, and
the error function approximation built into the compiler's constant-folder.

---

## 2. New Builtins (Slice 1)

All 12 builtins are registered in `builtins.rs` and handled by the interpreter
(`interp.rs`). The 8 pure builtins are also lowered in the native codegen path
(`codegen.rs` via `axon-rt` extern calls); the 4 impure sampling builtins emit
`E0910` if reached in `axon build` (interp-only, consistent with Phase 9 Sandbox).

### 2.1 Pure Builtins — Gaussian

```
gaussian_pdf(mu: f64, sigma: f64, x: f64) -> f64
```
Probability density at `x` for a Gaussian with mean `mu` and standard deviation
`sigma`. Requires `sigma > 0`; panics otherwise (runtime check).
Formula: `(1 / (sigma * sqrt(2π))) * exp(-0.5 * ((x - mu) / sigma)^2)`.

```
gaussian_cdf(mu: f64, sigma: f64, x: f64) -> f64
```
Cumulative distribution function P(X ≤ x) for Gaussian(mu, sigma).
Returns a value in [0.0, 1.0].
Formula: `0.5 * (1 + erf((x - mu) / (sigma * sqrt(2))))`.
The error function `erf` is approximated via the Abramowitz & Stegun rational
polynomial (max error < 1.5 × 10⁻⁷).

### 2.2 Pure Builtins — Beta

```
beta_mean(alpha: f64, beta_b: f64) -> f64
```
Mean of Beta(alpha, beta_b). Returns `alpha / (alpha + beta_b)`.
Requires `alpha > 0`, `beta_b > 0`; panics otherwise.

```
beta_variance(alpha: f64, beta_b: f64) -> f64
```
Variance of Beta(alpha, beta_b).
Returns `(alpha * beta_b) / ((alpha + beta_b)^2 * (alpha + beta_b + 1.0))`.

```
beta_cdf(alpha: f64, beta_b: f64, x: f64) -> f64
```
Cumulative distribution function P(X ≤ x) for Beta(alpha, beta_b).
Returns 0.0 for x ≤ 0.0 and 1.0 for x ≥ 1.0. Uses the regularized incomplete
beta function computed via continued-fraction expansion (Lentz's algorithm,
convergence tolerance 1e-10).

### 2.3 Pure Builtins — Categorical

```
categorical_mean(probs: [f64]) -> f64
```
Expected value of Categorical(probs): `Σ(i * probs[i])` for i in 0..len(probs).
Requires `len(probs) > 0`; panics on empty array.

```
categorical_variance(probs: [f64]) -> f64
```
Variance of Categorical(probs): `Σ(i^2 * probs[i]) - mean^2`.

```
categorical_cdf(probs: [f64], k: i64) -> f64
```
Cumulative probability P(X ≤ k): prefix sum `Σ probs[i]` for i in 0..min(k+1, len).
Returns 0.0 for k < 0 and 1.0 for k ≥ len(probs).

### 2.4 Impure Builtins — Sampling (effect row: `Random`)

```
gaussian_sample(mu: f64, sigma: f64) -> f64        | {Random}
```
Draws one sample from Gaussian(mu, sigma) using the Box-Muller transform applied to
two uniform draws from the interpreter's seeded RNG (`AXON_SEED`).
Requires `sigma > 0`; panics otherwise.

```
beta_sample(alpha: f64, beta_b: f64) -> f64        | {Random}
```
Draws one sample from Beta(alpha, beta_b) using Johnk's method for small
parameters and the Cheng (BB) algorithm for large. Returns a value in [0.0, 1.0].
Requires `alpha > 0`, `beta_b > 0`; panics otherwise.

```
categorical_sample(probs: [f64]) -> i64            | {Random}
```
Draws one sample from Categorical(probs) via inverse-CDF transform on a uniform
draw. Returns an index in `[0, len(probs))`.
Requires `len(probs) > 0`; panics on empty array.

### 2.5 Effect-Row Summary

| Builtin | Effect row |
|---|---|
| `gaussian_pdf`, `gaussian_cdf` | `\| {}` (pure) |
| `beta_mean`, `beta_variance`, `beta_cdf` | `\| {}` (pure) |
| `categorical_mean`, `categorical_variance`, `categorical_cdf` | `\| {}` (pure) |
| `gaussian_sample` | `\| {Random}` |
| `beta_sample` | `\| {Random}` |
| `categorical_sample` | `\| {Random}` |

---

## 3. Userland Library (Slice 1)

`examples/stdlib/distribution.ax` — a self-contained stdlib file that provides struct
wrappers and derived operations over the 12 builtins. This is the *recommended*
userland interface; callers should prefer these wrappers over calling raw builtins.

### 3.1 Struct Types

```axon
type Gaussian    = { mu: f64, sigma: f64 }
type Beta        = { alpha: f64, beta_b: f64 }
type Categorical  = { probs: [f64] }
```

### 3.2 Validated Constructors

```axon
fn gaussian_new(mu: f64, sigma: f64) -> Gaussian
fn beta_new(alpha: f64, beta_b: f64) -> Beta
fn categorical_new(probs: [f64]) -> Categorical
```

Constructors validate parameters and panic with a descriptive message on violation:
- `gaussian_new`: panics if `sigma <= 0.0`.
- `beta_new`: panics if `alpha <= 0.0` or `beta_b <= 0.0`.
- `categorical_new`: panics if `len(probs) == 0` or if the sum of `probs` is not
  within 0.001 of 1.0.

### 3.3 Pure Query Functions

All return `f64` unless noted. None carry a `Random` effect.

```
gaussian_mean(g: Gaussian) -> f64           — returns g.mu
gaussian_std(g: Gaussian) -> f64            — returns g.sigma
gaussian_prob_lte(g: Gaussian, x: f64) -> f64  — P(X ≤ x) via gaussian_cdf
gaussian_density(g: Gaussian, x: f64) -> f64   — pdf via gaussian_pdf

beta_mu(b: Beta) -> f64                     — mean via beta_mean
beta_sigma2(b: Beta) -> f64                 — variance via beta_variance
beta_prob_lte(b: Beta, x: f64) -> f64       — P(X ≤ x) via beta_cdf

categorical_mu(c: Categorical) -> f64       — mean via categorical_mean
categorical_sigma2(c: Categorical) -> f64   — variance via categorical_variance
categorical_prob_lte(c: Categorical, k: i64) -> f64  — P(X ≤ k) via categorical_cdf
```

### 3.4 Sampling Functions

```
dist_sample_gaussian(g: Gaussian) -> f64    | {Random}
dist_sample_beta(b: Beta) -> f64            | {Random}
dist_sample_categorical(c: Categorical) -> i64  | {Random}
```

These wrap the impure sampling builtins. Callers in `@[contained]` scopes must declare
`{Random}` in their effect ceiling or be blocked by E1310.

---

## 4. Slice 2 — Probabilistic Refinement Predicates (Future)

This section specifies the language-level extensions planned for the follow-on Slice 2
iteration. It is recorded here to lock the design, not as shipped work.

### 4.1 Syntax Extensions

```ebnf
; Added to the Phase 5 predicate grammar:
prob_pred  ::= "E" "[" expr "]" cmp_op expr
             | "Var" "[" expr "]" cmp_op expr
             | "P" "(" expr cmp_op expr ")"
```

Examples:

```axon
// A distribution whose mean is at most 0.5.
type LowMeanBeta = Beta where E[_] <= 0.5

// A distribution with variance below 0.01.
fn narrow_dist(b: Beta where Var[b] < 0.01) -> f64 { beta_mu(b) }

// A Gaussian where there is at most a 5% chance of exceeding 3σ.
type TightGaussian = Gaussian where P(_ > 3.0) < 0.05

// Inline on a parameter:
fn price(g: Gaussian where E[g] > 0.0 && Var[g] < 100.0) -> f64 { gaussian_mean(g) }
```

The implicit binder `_` inside `E[_]`, `Var[_]`, `P(_ op k)` refers to the
distribution value being refined.

### 4.2 Interval-Arithmetic Discharge

The Phase 5 SMT path is extended: before handing off to Z3, the checker tries an
*interval-arithmetic pre-pass* for probabilistic predicates whose distribution
parameters are compile-time constants:

1. Evaluate the distribution parameters in the constant-folder.
2. Compute tight bounds on `E[dist]` and `Var[dist]` using the closed-form
   formulas from §1.
3. For `P(dist <= k)` with constant `k`, evaluate `gaussian_cdf` / `beta_cdf` /
   `categorical_cdf` exactly.
4. If the resulting interval satisfies the predicate, emit `Discharged` (no runtime
   check). If the interval definitively falsifies it, emit E1200 with a
   counter-example. If the interval is inconclusive, fall through to the runtime
   check (exit 6).

For non-constant parameters, the runtime fallback (exit 6) is always taken.

**Discharge table for constant parameters:**

| Predicate | Discharged when |
|---|---|
| `E[Gaussian(mu, sigma)] <= k` | `mu <= k` (exact) |
| `E[Beta(a, b)] <= k` | `a/(a+b) <= k` (exact) |
| `Var[Beta(a, b)] < k` | `a*b/((a+b)^2*(a+b+1)) < k` (exact) |
| `P(Gaussian(mu, sigma) <= k) >= p` | `gaussian_cdf(mu, sigma, k) >= p` (exact) |
| `P(Categorical(ps) <= k) >= p` | prefix sum `>= p` (exact) |

### 4.3 `observe` and `condition` (Slice 2 Keywords)

In Slice 2, `observe` and `condition` are promoted from builtin-level names to
first-class keywords with special parser handling:

```axon
// observe: record a sampled value; returns it unchanged but adds evidence to the
// ambient probabilistic context for `condition` calls in the same scope.
let x = observe(gaussian_sample(g))

// condition: narrow a distribution by a boolean observation; returns a new
// distribution value whose moments reflect the evidence.
let g_pos = condition(g, x > 0.0)
```

`condition` for Gaussian + linear constraint uses exact conjugate update; for
other families it uses rejection sampling (finite-iteration, bounded by
`AXON_CONDITION_MAX_ITERS`, default 1000). Effect row of `condition` is `| {Random}`.

### 4.4 Runtime Checking — Slice 2

Probabilistic refinement predicates that are not statically discharged are
evaluated at runtime at all four obligation sites (§2 of Phase 5):

- **PRECONDITION**: at function entry for a `dist: D where E[dist] <= k` parameter.
- **POSTCONDITION**: at each return site for a `-> D where P(D <= k) >= p` return type.
- **STRUCT CONSTRUCTION**: for a named refinement type built over a distribution.
- **LET/OWN/REF binding**: for `let d: D where Var[d] < 0.01`.

Runtime evaluation calls the corresponding builtin (e.g. `beta_mean`) and checks
the predicate. A violation exits **6** (`REFINE_VIOLATION_EXIT_CODE`), byte-identical
between interp and native.

---

## 5. Error Codes (Phase 13)

Phase 13 error codes use the E15xx block (reserved for probabilistic/distribution
errors; E14xx is reserved for Phase 14 distributed types).

```
E1500  probabilistic predicate not dischargeable — interval arithmetic inconclusive for
       constant parameters; predicate cannot be proved or refuted statically.
       Falls through to runtime check. (Analogous to E1202 for general predicates.)
E1501  distribution parameter out of domain — static proof that a parameter violates
       the domain constraint (e.g. sigma <= 0.0 with a constant argument).
       Counter-example is the offending constant.
E1502  empty probability simplex — `categorical_new` called with an empty array or a
       probs array that the constant-folder can prove sums to 0.
E1503  distribution predicate not provable — interval arithmetic produces a definitive
       falsification of a probabilistic refinement (e.g. E[Beta(1, 1)] <= 0.3 is false
       since E[Beta(1,1)] = 0.5). Counter-example gives the closed-form witness.
E1504  `condition` with unsatisfiable event — the conditioning event has probability 0
       under the given distribution (proved via CDF arithmetic); this is a division-by-zero
       in the posterior update.
E1505  `observe` outside probabilistic scope — `observe(...)` called without an enclosing
       distribution-typed binding that the evidence could update. Warning only.
```

---

## 6. Implementation Plan

### 6.1 Slice 1 File Map

```
crates/axon-core/src/
  builtins.rs      add 12 new builtin entries (§2)
  interp.rs        handle all 12 in the builtin dispatch arm
  codegen.rs       declare axon_rt extern stubs for the 8 pure builtins (E0910-refuse sampling)
  main.rs          no changes needed
crates/axon-rt/
  dist.c / dist.rs  (NEW) C or Rust implementations of gaussian_pdf/cdf, beta_{mean,var,cdf},
                    categorical_{mean,var,cdf} (pure, no allocation)
examples/stdlib/
  distribution.ax   (NEW) userland library (§3)
```

### 6.2 Slice 2 File Map (future)

```
crates/axon-core/src/
  refine.rs        +ProbPred variant (E[expr], Var[expr], P(expr op expr))
  smt.rs           +translate_prob_pred; interval pre-pass before Z3 dispatch
  ast.rs           +Predicate::Prob; +Expr::Observe, Expr::Condition
  parser.rs        parse E[…], Var[…], P(… op …), observe(…), condition(…, …)
  checker.rs       discharge path for constant distribution parameters
  interp.rs        runtime check at all four obligation sites for prob predicates
  codegen.rs       runtime check lowering (same four sites; E0910 for sampling in codegen)
```

### 6.3 Interpreter RNG Alignment

The three sampling builtins must use the same `AXON_SEED`-seeded RNG that the existing
`random_i64` and `random_f64` builtins use. They must also be replayable under
`AXON_AI_REPLAY` (the replay engine memoizes `(call_name, args)` → result tuples for
all impure builtins, not just AI calls). This means:

- `gaussian_sample(mu, sigma)` → memo key `("gaussian_sample", mu, sigma, rng_counter)`.
- `beta_sample(alpha, beta_b)` → memo key `("beta_sample", alpha, beta_b, rng_counter)`.
- `categorical_sample(probs)` → memo key `("categorical_sample", hash(probs), rng_counter)`.

The `rng_counter` advances monotonically across all impure draws in a run, matching
the behavior of `random_i64` / `random_f64`.

---

## 7. Dependencies (Phase 13 Additions)

No new crate dependencies are required for Slice 1. The numeric algorithms (Box-Muller,
Cheng BB, Lentz continued fraction) are implemented directly in `axon-rt` using only
`libm` (already linked on all targets). The Abramowitz & Stegun `erf` approximation uses
only polynomial arithmetic.

Slice 2 adds no new Cargo dependencies beyond Phase 5's Z3 integration.

---

## 8. Verification Checklist

### Slice 1 Exit Criteria

- [ ] All 12 builtins appear in `BUILTINS` (builtins.rs) and are handled in the
      interpreter dispatch (interp.rs).
- [ ] `gaussian_pdf(0.0, 1.0, 0.0)` returns a value within 0.0001 of `0.3989` (the
      standard normal density at 0).
- [ ] `gaussian_cdf(0.0, 1.0, 0.0)` returns a value within 0.001 of `0.5`.
- [ ] `beta_mean(1.0, 1.0)` returns exactly `0.5`.
- [ ] `beta_variance(2.0, 2.0)` returns a value within 0.0001 of `0.05` (exact: 1/20).
- [ ] `beta_cdf(1.0, 1.0, 0.5)` returns a value within 0.001 of `0.5` (uniform on [0,1]).
- [ ] `categorical_mean([0.25, 0.25, 0.25, 0.25])` returns a value within 0.0001 of `1.5`.
- [ ] `categorical_cdf([0.1, 0.4, 0.3, 0.2], 1)` returns a value within 0.0001 of `0.5`.
- [ ] `gaussian_sample(0.0, 1.0)` with `AXON_SEED=42` is deterministic across runs.
- [ ] `beta_sample(1.0, 1.0)` returns a value in [0.0, 1.0].
- [ ] `categorical_sample([0.0, 1.0])` always returns `1` (degenerate distribution).
- [ ] Sampling builtins with `AXON_AI_REPLAY` set replay their recorded values verbatim.
- [ ] `axon build` with a sampling builtin emits E0910 and refuses to compile.
- [ ] `axon test examples/stdlib/distribution.ax` passes all `@[test]` functions.
- [ ] All Phase 1–12 examples and fixtures continue to pass unchanged.

### Slice 2 Exit Criteria (future)

- [ ] `type LowMeanBeta = Beta where E[_] <= 0.5` parses and is accepted.
- [ ] `let b: LowMeanBeta = beta_new(1.0, 1.0)` is **statically discharged** (E[Beta(1,1)] = 0.5 ≤ 0.5).
- [ ] `let b: LowMeanBeta = beta_new(3.0, 1.0)` is rejected with E1503 and a counter-example
      showing E[Beta(3,1)] = 0.75 > 0.5.
- [ ] `fn narrow(b: Beta where Var[b] < 0.01) -> f64` accepts `beta_new(10.0, 10.0)` (Var ≈ 0.0119… wait, that's > 0.01 — the checker must compute and reject it with E1503).
- [ ] `gaussian_cdf` predicate discharge: `type StandardTail = Gaussian where P(_ > 1.96) < 0.03`
      accepts `gaussian_new(0.0, 1.0)` (P(X > 1.96) ≈ 0.025 < 0.03).
- [ ] `observe` / `condition` parse, type-check, and carry `| {Random}`.
- [ ] `condition(gaussian_new(0.0, 1.0), x > 0.0)` returns a half-normal posterior with
      mean ≈ 0.7979 (the folded-normal mean), verifiable via `gaussian_mean`.
- [ ] Probabilistic refinements at all four obligation sites (param / return / struct / let)
      emit runtime exit 6 when parameters are non-constant.
- [ ] E1500–E1505 are emitted at the correct sites with well-formed messages.

---

## 9. Relationship to the Phase 5 Refinement System

Phase 13 predicates slot into the Phase 5 obligation worklist without new plumbing:

```
Phase 5 checker worklist item:
  Obligation { pred: Predicate, gamma: PathCtx, site: ObligationSite }

Phase 13 extension:
  Predicate::Prob(ProbPred)   // E[expr], Var[expr], P(expr op expr)

Discharge order (Slice 2):
  1. Constant-folder: evaluate distribution parameters.
  2. Interval-arithmetic pre-pass (§4.2): try closed-form discharge / refutation.
  3. Fall through to Phase 5 SMT path (Z3) if inconclusive.
  4. Fall through to runtime check (exit 6) on SMT unknown/timeout.
```

This means the SMT solver is never called for constant-parameter probabilistic
predicates — interval arithmetic is complete for the tractable fragment. The solver
path remains for the (rare) case where a probabilistic predicate is combined with
a non-probabilistic one that Z3 can simplify.

---

## 10. Carry-Forward into Phase 14+

| Future layer | Uses Phase 13 mechanism |
|---|---|
| Distributed types (Phase 14) | `Belief<T>` wraps `Distribution<T>`; message-passing updates beliefs via `observe`/`condition`. |
| Tensor + autodiff | `Tensor<f64, [N]>` with a `Gaussian` prior over each weight; the refinement system proves moment bounds for Bayesian neural networks. |
| Goal-directed (Phase 8 / ROADMAP §9.5) | `for! maximize E[reward]` treats a `Distribution<f64>` reward signal as the optimization target; moment bounds constrain the search. |
| World model (ROADMAP worldmodel-loop) | The world model's `predict` step returns `Uncertain<T>` calibrated from `Distribution<T>` posteriors; `condition` is the Bayesian update after `fit_check`. |
