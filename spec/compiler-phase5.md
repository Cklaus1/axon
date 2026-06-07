# Axon Compiler — Phase 5 Spec (ASI Layer 4)

**Goal**: Promote `@[verify]` from runtime panic to compile-time proof. Add refinement types
(`T where <pred>`), wire an SMT solver into the type checker, and make every later ASI layer
(effects, probabilistic types, info-flow, budgets) sit on a substrate of *proved* predicates
rather than runtime-trapping ones.
**Builds on**: Phase 4 (`spec/compiler-phase4.md`) and ASI Layers 1–3 (Uncertain<T>, Temporal<T>,
`@[verify]`, `@[adaptive]`, `@[contained]`).
**Timeline**: a multi-iteration ASI project — solver integration, refinement
type-system extensions, and termination checking all sit on the critical
path; each can be drafted in one iteration cycle but requires a validation
cycle against the full Phase 1–4 fixture suite before merging.

---

## Phase 5 Scope

### In Phase 5
- Refinement types (`T where <pred>`) — first-class, named or anonymous
- SMT backend (Z3) wired into the checker via a new `verify.rs::prove()` entry point
- Static `@[verify]` — predicates on AI-sourced values proved at compile time when the
  source lattice is `Known`; runtime fallback when `Runtime`
- Refinement-aware subtyping (E1200–E1206)
- Termination analysis (decreasing-measure heuristic + `@[total]` attribute)
- Comptime predicate evaluation — refinement check on constant arguments uses the existing
  `comptime.rs` evaluator before invoking the solver
- Predicate language: arithmetic, booleans, equality, comparisons, structural projection,
  bounded calls into `@[pure]` functions, `confidence(x)` and `validity(x)` projections on
  Uncertain/Temporal values
- `@[pure]` attribute and effect annotation `pure` on fn signatures (the slim slice of the
  Layer-5 effect system needed to admit user predicates into the solver)

### Explicitly Out of Phase 5
```
Full effect rows                  → Phase 6 (Layer 5)
Distribution<T> / observe / sample → Phase 7 (Layer 6)
Tensor<T, dims...> + autodiff      → Phase 8 (Layer 7)
Information-flow / taint lattice  → Phase 9 (Layer 8)
Goal-directed search as control   → Phase 10 (Layer 9)
Multi-agent / distributed types   → Phase 11 (Layer 10)
Cost / budget as a type           → Phase 11 (Layer 10)
Liquid-style inference of preds   → future
Universally quantified refinements over generics → future (only `forall T` over closed
                                                   primitive sets is in scope here)
Termination over mutual recursion → future
```

---

## 1. Refinement Types

### Motivation

Today an Axon function that "must take a positive integer" either panics, returns
`Result<T, str>`, or relies on `@[verify]` to check at runtime. None of these prevent the
caller from passing zero — they only diagnose the failure. A refinement type `i64 where _ > 0`
moves the obligation onto the caller and discharges it once at the call site, statically.

The same machinery proves obligations on AI-sourced values: an `Uncertain<i64>` whose
confidence-source lattice is `Known` becomes safe to unwrap into a refined integer when the
predicate `confidence(x) >= 0.9` is provable.

### Syntax

```ebnf
refined_type   ::= type "where" predicate
predicate      ::= pred_or
pred_or        ::= pred_and ("||" pred_and)*
pred_and       ::= pred_atom ("&&" pred_atom)*
pred_atom      ::= "!" pred_atom
                 | "(" predicate ")"
                 | comparison
                 | call_pred
                 | bool_lit
                 | path

comparison     ::= expr ("==" | "!=" | "<" | ">" | "<=" | ">=") expr
call_pred      ::= ident "(" expr ("," expr)* ")"
expr           ::= ... (subset of normal Axon expressions; see §1.4)

named_refine   ::= "type" ident generic_params? "=" type "where" predicate
```

The implicit binder `_` refers to the value of the refined type. Inside a struct refinement
`type Range = { lo: i64, hi: i64 } where _.lo <= _.hi`, `_` refers to the whole struct
instance.

### Examples

```axon
type Positive  = i64 where _ > 0
type NonEmpty  = str where str_len(_) > 0
type Probability = f64 where _ >= 0.0 && _ <= 1.0

fn divide(n: i64, d: i64 where _ != 0) -> i64 { n / d }

fn first_byte(s: NonEmpty) -> i64 { char_at(s, 0) }

type Range = { lo: i64, hi: i64 } where _.lo <= _.hi

fn clamp(r: Range, x: i64) -> i64 {
    if x < r.lo { r.lo } else if x > r.hi { r.hi } else { x }
}

// Inline anonymous refinement on a return type:
fn abs_pos(n: i64) -> (i64 where _ >= 0) { if n < 0 { -n } else { n } }
```

### Semantic Rules

**R01 (subtyping)** — `T where P` is a subtype of `T`. Coercing in the *opposite* direction
(`T → T where P`) requires the checker to prove `P` for the source value. Coercing between two
refinements (`T where P → T where Q`) requires proving `P ⇒ Q`.

**R02 (call-site proof)** — At every call `f(arg₁, …, argₙ)` whose declared parameter type is
`Tᵢ where Pᵢ`, the checker emits a proof obligation `Γ ⊢ Pᵢ[argᵢ/_]` where `Γ` is the
*path condition* — the conjunction of all branch predicates active at that program point
(see §1.5).

**R03 (return-site proof)** — A function with declared return `T where P` must satisfy `P[e/_]`
for every return expression `e`, again under the path condition active at the return site.

**R04 (struct construction)** — `MyStruct { f₁: e₁, f₂: e₂ }` whose declared type is
`{ f₁: T₁, f₂: T₂ } where P` requires the proof `P[(struct e₁ e₂)/_]`. Field-level refinements
on `Tᵢ` are discharged independently before the whole-struct predicate.

**R05 (assignment & rebinding)** — Assignment `x = expr` is treated as rebinding x at the type
of the LHS; if the LHS's declared type is refined, the predicate is proved for the new value.
Bare `let x = expr` *infers* the strongest refinement deducible from `expr` and the path
condition (see §1.6).

**R06 (named refinements)** — `type Foo = T where P` introduces a *nominal* type. Two named
refinements with structurally identical predicates do not unify. Use anonymous `T where P` for
structural use.

**R07 (refinement on generics)** — `Vec<i64 where _ > 0>` is allowed. Quantification over the
*generic parameter itself* (`forall T: Eq, T where ...`) is **out of scope** for Phase 5; only
refinements over concrete or instantiated types are admitted.

**R08 (predicate purity)** — Every function call appearing in a predicate must resolve to a
function annotated `@[pure]` (see §2). Impure calls in predicates are rejected with E1207.

### Predicate Language Surface

A predicate is a subset of Axon expressions returning `bool`:

| Allowed | Not allowed (Phase 5) |
|---|---|
| `+ - * / %` on `i64`, `f64` | floating-point trig / log / exp |
| `== != < > <= >=` | bitwise (`& \| ^ << >>`) — phase 6 |
| `&& \|\| !` | string ops other than `str_len`, `str_eq` |
| Field projection `_.f.g` | mutation, allocation, side effects |
| Calls into `@[pure]` fns (depth ≤ 4) | calls into AI builtins (`ai_complete`, etc.) |
| `confidence(_)`, `validity(_)`, `value(_)` | unbounded recursion |
| Constant array indexing `a[k]` for literal `k` | dynamic indexing |
| `comptime`-evaluable expressions of any shape | arbitrary runtime expressions |

Predicates that escape this language are not rejected at parse time — they are accepted by the
parser but the checker emits **E1202** ("unsupported predicate; will be checked at runtime")
and falls through to a `__axon_verify_panic` site, exactly mirroring today's Layer-3 behavior.
This preserves the escape hatch.

### Path-Condition Tracking

The checker maintains a stack of boolean predicates capturing the program-point context:

```
PathCtx = Vec<Pred>
```

Pushed on `if cond { … }` (push `cond`), `else { … }` (push `!cond`), `while cond { … }`
(push `cond` for the body); `match` arms push the variant tag and any guard. Popped on block
exit. Every proof obligation is discharged under the conjunction of the current `PathCtx`.

Loops are handled conservatively in Phase 5: the body's path condition includes the loop guard
but *not* any inferred loop invariant (no abstract interpretation in this phase). This means
loops that depend on a variant-establishing invariant must annotate one explicitly via a
`@[invariant(P)]` attribute on the loop or fall through to runtime check (E1202).

### Refinement Inference for `let`

For `let x = e` the checker computes the *most precise* refinement it can deduce:

- Constant: `let x = 7` → `x: i64 where _ == 7`
- Comptime-evaluable: same, post evaluation
- Branch merge: in `if c { 5 } else { 9 }` → `i64 where _ == 5 || _ == 9`
- Otherwise: the declared or inferred bare type

These inferred refinements live only in the checker's environment — they do not appear in
diagnostics or LSP hover unless the user explicitly opted in via `@[show_refinements]` on the
function. This keeps default error messages readable.

---

## 2. `@[pure]` Attribute and `pure` Effect

### Motivation

Predicates may call user functions, but those functions must be free of effects: no I/O, no
allocation visible to the predicate, no AI calls, no channel ops, no comptime panics. Phase 5
introduces a single attribute — `@[pure]` — and a single effect-row token — `pure` — sufficient
to admit user functions into the predicate language. The full effect system arrives in Phase 6.

### Surface

```axon
@[pure]
fn abs(n: i64) -> i64 { if n < 0 { -n } else { n } }

@[pure]
fn in_range(lo: i64, hi: i64, x: i64) -> bool { lo <= x && x <= hi }
```

### Semantic Rules

**P01** — A `@[pure]` function may only call other `@[pure]` functions and a fixed allowlist of
intrinsic builtins (full list in §6).

**P02** — A `@[pure]` function may not perform I/O, AI calls, channel ops, `spawn`, mutation of
captured state, or `panic`.

**P03** — A `@[pure]` function must terminate — see §3.

**P04** — Builtins implicitly carry purity: `add`, `sub`, `mul`, `div`, `mod`, `eq`, `lt`,
`str_len`, `str_eq`, `confidence`, `validity`, `value`, `abs_i64`, `min_i64`, `max_i64`,
`clamp_i64`, `i64_to_f64`, `f64_to_i64`, `floor_f64`, `ceil_f64`, `round_f64`, `sqrt_f64`,
`pow_i64`, `char_at`, `str_index_of`, `str_starts_with`, `str_ends_with`, `str_contains`,
`str_slice`, `str_pad_start`, `str_pad_end`, `str_repeat`, `str_replace`, `str_to_lower`,
`str_to_upper`, `str_trim`, `str_trim_start`, `str_trim_end`. AI/IO/time/random builtins are
**impure**: `ai_complete`, `ai_extract*`, `read_line`, `read_file`, `write_file`, `sleep_ms`,
`now_ms`, `random_i64`, `random_f64`, `env_var`, `exit`, `goal_run`.

**P05** — `@[pure]` is checked at definition time (E1207 on violation) and propagates: a
non-`@[pure]` function calling a `@[pure]` one is fine; the reverse is not.

---

## 3. Termination

The solver only admits predicates whose call graph terminates. Phase 5 ships a deliberately
simple termination checker.

### `@[total]` Attribute

```axon
@[total]
@[pure]
fn fact(n: i64 where _ >= 0) -> i64 { if n == 0 { 1 } else { n * fact(n - 1) } }
```

`@[total]` requires the checker to discharge a *decreasing-measure* obligation: for every
recursive call site, find a strictly decreasing well-founded measure on the arguments. Phase 5
supports the following automatic measures:

1. A single `i64` parameter that is bounded below (refinement `_ >= K`).
2. A single `Vec<T>` parameter where the recursive call passes `tail(v)` (length-decreasing).
3. A user-supplied measure: `@[total(measure: <expr>)]` whose expression is a `@[pure]` `i64`
   function of the parameters; the checker proves `measure(args') < measure(args)` at each
   recursive call.

Mutual recursion is **out of scope** for Phase 5 (E1203).

`@[pure]` *requires* a termination proof. `@[total]` is the explicit form; the checker also
accepts purely non-recursive `@[pure]` functions silently.

---

## 4. SMT Backend

### Choice — Z3

Phase 5 picks **Z3** via the Rust `z3` crate (already widely deployed; mature in 2026). The
solver runs in-process; no daemon. A future phase may add CVC5 or a small native solver for
the common fragment, but the SMT-LIB-style API is portable.

### Translation

The compiler lowers an Axon predicate into SMT-LIB2 sorts:

| Axon | SMT-LIB |
|---|---|
| `i64`, `i32`, `i16`, `i8`, `u64`, `u8` | `(_ BitVec 64)` (with appropriate signed ops) |
| `f64` | `(_ FloatingPoint 11 53)` |
| `bool` | `Bool` |
| `str` (length-only reasoning) | uninterpreted sort `Str` plus `(declare-fun str-len (Str) Int)` |
| `Uncertain<T>` | uninterpreted with `(confidence Uncertain) → Real`, `(value Uncertain) → T` |
| `Temporal<T>` | analogous with `validity`, `value` |
| Struct types | `(declare-datatype …)` records |
| Enum types | algebraic datatypes with one constructor per variant |
| `Vec<T>` (length-only) | uninterpreted with `(vec-len Vec) → Int` |

Pure functions called inside predicates are inlined up to depth 4; deeper calls are
*uninterpreted*: the function name becomes a fresh SMT function symbol and the solver may not
discharge predicates that depend on its body. The depth bound is configurable via
`AXON_PROOF_DEPTH` (default 4, max 16).

### Solver Result Mapping

| Z3 result | Axon checker action |
|---|---|
| `unsat` (predicate is `valid`) | obligation discharged |
| `sat` (counter-model exists) | E1200 with concrete counter-example formatted as Axon source |
| `unknown` after timeout | E1206 ("solver timeout, predicate undecidable") — falls through to runtime check |
| Translation failure | E1202 — falls through to runtime check |

Default solver timeout: **2000 ms** per obligation. Configurable via `AXON_PROOF_TIMEOUT_MS`.
A `--proof-timeout 0` flag disables SMT entirely (every predicate becomes a runtime check, useful
for bisecting compile regressions).

> **Implemented ahead of the Z3 backend (status):** the runtime-check fallback itself is
> LANDED and is the *current default* for non-constant refinement preconditions — the SMT
> backend above is not yet wired in, so today every non-constant precondition takes the
> runtime path rather than being statically discharged. A parameter `p: T where P` has `P`
> evaluated at **function entry** with `_` bound to the actual argument; a violation exits **6**
> (`REFINE_VIOLATION_EXIT_CODE`), distinct from a `@[verify]` postcondition (3) and a bug-panic
> (101). This is enforced symmetrically in the interpreter (`Interp::call_fn`) and native codegen
> (`emit_refine_preconditions`), with byte-identical exit codes (`scripts/exit_code_parity.sh`,
> invariant I-2). Codegen lowers the same predicate subset the constant-folder supports
> (literals, `_`, arithmetic, comparisons, `&&`/`||`/`!`, `_.field`, `str_len`/`str_eq`, and
> calls to `@[pure]` fns); a predicate outside that subset is honestly E0910-refused at build
> time, never silently skipped. When the Z3 backend lands, a *provable* precondition is
> discharged statically and its runtime check is elided; an *unprovable* one continues to take
> this runtime path.

### Counter-Example Reporting

When Z3 returns `sat`, the checker reads the model and formats counter-examples in Axon
source:

```
error[E1200]: predicate not provable at call site
  --> divide.ax:7:14
   |
 7 |     divide(x, y)
   |              ^ argument here
   |
   = required: i64 where _ != 0
   = found:    i64
   = counter-example: y == 0  when  x == 1
   = note: add `if y != 0 { divide(x, y) } else { … }` or refine the type of `y`.
```

---

## 5. Static `@[verify]` — Bridge with ASI Layers 1–3

Layer-3 today inserts a `__axon_verify_panic` call at runtime. Phase 5 keeps the runtime check
as a fallback but inserts a static obligation **first**:

1. Checker reaches `@[verify(P)]` on an item.
2. If every Uncertain/Temporal source feeding `P` has lattice value `Known` (Layer-3.5
   classification), translate `P` to SMT and try to discharge it.
3. On `unsat` → emit no runtime check (zero overhead).
4. On `sat` → E1200 with counter-example, refusing to compile.
5. On lattice value `Runtime` for any source, or on `unknown`/translation-failure →
   keep the existing runtime check unchanged.

This means the existing `__axon_verify_panic` code path becomes the slow path only. The fast
path is no code at all.

### Predicate Forms Newly Admitted

```axon
@[verify(confidence(temperature) >= 0.9)]
fn report(temperature: Uncertain<f64>) -> str { … }

@[verify(value(score) >= 0.0 && value(score) <= 1.0)]
fn classify(score: Uncertain<f64>) -> str { … }

@[verify(validity(price) > now_ms())]   // Temporal<T>
fn act(price: Temporal<f64>) -> Order { … }
```

`now_ms` is impure and therefore *not* admitted in `@[pure]` predicates — but `@[verify]`
predicates may reference a small set of *side-effect-free reads* (`now_ms`, `env_var`)
because the runtime check is the fallback, not the source of truth. The checker translates
these to fresh SMT constants; predicates that depend on them remain `unknown` and fall
through to the runtime check, which is the correct behavior.

---

## 6. New Builtins

```
confidence(u: Uncertain<T>) -> f64    @[pure]
value(u: Uncertain<T>) -> T           @[pure]
validity(t: Temporal<T>) -> i64       @[pure]
```

These are projections; they already exist in spirit (the checker has `confidence_get` and
`validity_get` internal hooks), but Phase 5 promotes them to first-class `@[pure]` builtins so
they may appear in user predicates.

---

## 7. Implementation Plan

### File Map

```
crates/axon-core/src/
  refine.rs         (NEW) refined-type AST, predicate expr, path-condition stack, inference
  smt.rs            (NEW) Z3 translation, model formatter, timeout config, depth-bounded inlining
  total.rs          (NEW) termination checker (decreasing-measure heuristic + @[total])
  pure.rs           (NEW) purity check; populates @[pure] table
  ast.rs            +Refined(Type, Predicate); +Predicate enum; @[pure]/@[total] attrs
  parser.rs         parse `where <pred>`, `@[pure]`, `@[total]`, `@[total(measure: …)]`
  types.rs          Type::Refined; subtype rules updated
  infer.rs          path-condition stack; refinement inference for let
  checker.rs        emits proof obligations into a worklist; calls smt::prove
  verify.rs         (existing) becomes the runtime fallback emitter; static path delegates here
  comptime.rs       hooked for constant-arg pre-discharge
crates/axon-core/Cargo.toml
  z3 = "0.12"
```

### Pipeline Position

```
Lexer → Parser → Resolver → fill_captures → Infer (HM) → Checker
                                                         ├── Borrow
                                                         ├── pure-check  (NEW)
                                                         ├── total-check (NEW)
                                                         └── refine-prove (NEW, calls smt.rs)
                                                              ↓
                                              [Mono] → Codegen → LLVM → binary
```

The proof phase runs **after** `Borrow` and **before** monomorphization. Refinement
constraints are checked on the generic AST; monomorphization preserves them.

### Parser Changes

```rust
// new ast.rs nodes
pub enum Type {
    …,
    Refined { base: Box<Type>, pred: Box<Predicate> },
}

pub enum Predicate {
    Bool(bool),
    Cmp(BinOp, Expr, Expr),
    And(Box<Predicate>, Box<Predicate>),
    Or(Box<Predicate>, Box<Predicate>),
    Not(Box<Predicate>),
    Call { fn_name: String, args: Vec<Expr> },
    Path(Vec<String>),     // identifier or _.f.g
}
```

### Solver Pseudo-Code

```rust
pub fn prove(pred: &Predicate, gamma: &PathCtx, env: &TypeEnv) -> ProofResult {
    let ctx = z3::Context::new(&z3::Config::new());
    let solver = z3::Solver::new(&ctx);

    for assumption in gamma.iter() {
        solver.assert(&translate(assumption, &ctx, env)?);
    }

    let goal = translate(pred, &ctx, env)?;
    solver.assert(&goal.not());

    solver.set_param("timeout", AXON_PROOF_TIMEOUT_MS);
    match solver.check() {
        SatResult::Unsat   => ProofResult::Discharged,
        SatResult::Sat     => ProofResult::Refuted(format_model(&solver.get_model()?)),
        SatResult::Unknown => ProofResult::Unknown,
    }
}
```

### Backwards Compatibility

- Every example in `examples/*.ax` and every fixture in `crates/axon-core/tests/` continues to
  compile unchanged. There are no refinements in any current source file, so no proof
  obligations exist for legacy code.
- Existing `@[verify]` annotations behave identically on legacy code (lattice = `Runtime` for
  every value not produced by a Layer-1 constructor, so the runtime-check fallback is taken).
- The `axon-rt` runtime is unchanged. `__axon_verify_panic` remains.

---

## 8. New Error Codes

```
E1200  predicate not provable at call/return site (with counter-example from solver model)
E1201  contradictory refinement (predicate is unsatisfiable on its own — type is uninhabited)
E1202  unsupported predicate construct — falls through to runtime check
E1203  recursive function in @[pure] / @[total] context cannot be proved terminating
E1204  struct invariant violated (whole-struct predicate fails after field assignment)
E1205  refinement narrowing failure (assignment from `T where P` to `T where Q`, P does not
       imply Q)
E1206  SMT solver timed out — predicate undecidable in {timeout} ms; falls through to runtime
E1207  @[pure] function calls non-pure function or builtin
E1208  predicate references runtime-impure builtin in a @[pure] context
E1209  named refinement type {name} has free type parameter not bound by an enclosing item
```

Errors E1202 and E1206 are **warnings under `--strict-proofs`** (the default for `axon test`)
and **errors under `--strict-proofs=hard`** (used by `axon build --release`).

---

## 9. CLI Summary (Phase 5 additions)

```
axon build <file> --proof-timeout <ms>     SMT timeout per obligation (default 2000)
axon build <file> --proof-depth <n>        max purity-inline depth (default 4, max 16)
axon build <file> --no-proofs              treat all predicates as runtime checks (debugging)
axon build <file> --strict-proofs          treat E1202/E1206 as errors
axon build <file> --emit-smt <out>         dump SMT-LIB queries to a file (debugging)
axon check <file> --explain E1200          print extended doc for an error code
```

`AXON_PROOF_TIMEOUT_MS` and `AXON_PROOF_DEPTH` environment variables provide the same
controls.

---

## 10. Dependencies (Phase 5 Additions)

```toml
# Cargo.toml additions for Phase 5
z3      = "0.12"        # Z3 bindings (links libz3 dynamically)
# z3 already pulls in `z3-sys`; no other new deps required.
```

Build note: `libz3` must be present on the host (`apt install libz3-dev` on Ubuntu, `brew
install z3` on macOS). The Phase 5 build will fail informatively if missing.

---

## 11. Verification Checklist

Phase 5 is done when:

- [ ] `type Positive = i64 where _ > 0` parses, type-checks, and `let x: Positive = 0` is
      rejected with E1200 and a counter-example.
- [ ] `fn divide(n: i64, d: i64 where _ != 0) -> i64` produces E1200 at every caller that
      cannot prove `d != 0` from the path condition.
- [ ] `fn abs_pos(n: i64) -> (i64 where _ >= 0)` is *accepted* — the checker proves the
      return predicate from the body's two branches.
- [ ] `if y != 0 { divide(x, y) }` is accepted — the path condition `y != 0` discharges the
      callee's predicate.
- [ ] `match opt { Some(x) => divide(1, x) … }` is accepted *only* when `x` is itself
      refined; otherwise E1200.
- [ ] An `Uncertain<i64>` constructed via `uncertain_new(42, 0.95)` (Known source) and used
      under `@[verify(confidence(_) >= 0.9)]` discharges statically — no
      `__axon_verify_panic` is emitted in the LLVM IR.
- [ ] An `Uncertain<i64>` constructed via `uncertain_dyn_i64(...)` (Runtime source) keeps
      the runtime check.
- [ ] `@[pure] fn impure() { println("x") }` is rejected with E1207.
- [ ] `@[total] fn fact(n: i64 where _ >= 0) -> i64 { … }` is accepted; an unguarded
      `fact(n)` (no lower bound) is rejected with E1203.
- [ ] An `@[invariant(P)]`-annotated `while` loop carries `P` into the body's path
      condition and out at the loop exit.
- [ ] `--no-proofs` produces a binary identical to the Phase-4 build (modulo refinement-only
      type annotations, which become no-ops).
- [ ] `--emit-smt` produces a valid SMT-LIB2 file that Z3 can re-check independently.
- [ ] All Phase 1–4 examples and fixtures continue to pass `axon test` unchanged.
- [ ] A new fixture `crates/axon-core/tests/integration_fixtures/refine.ax` exercises:
      named refinements, anonymous refinements on params/returns, struct invariants, path
      conditions through `if`/`match`, refinement-via-Uncertain, E1200 counter-example
      formatting.
- [ ] `axon build` of `examples/divide_safe.ax` succeeds without runtime panics; `axon build`
      of `examples/divide_unsafe.ax` fails with E1200 referencing the offending call site.

---

## 12. Carry-Forward into Phase 6+

Phase 5 lays the substrate; later layers ride on it:

| Layer (future) | Uses Phase 5 mechanism |
|---|---|
| Effects (Phase 6) | `@[pure]` becomes one effect row among many; predicate language admits effect-row constraints. |
| Probabilistic types (Phase 7) | `Distribution<T>` is `T where <density predicate>`; `observe` is a path-condition push. |
| Tensor + autodiff (Phase 8) | `Tensor<T, [N, D]>` shapes are refinements over `Vec<i64>` dimension lists; shape-mismatch is E1200 with shape counter-example. |
| Information flow (Phase 9) | `Tainted<T>` and `Trusted<T>` are nominal refinements over `T` plus a typing-relation lemma proved by SMT. |
| Goal-directed (Phase 10) | `for! maximize x { … }` requires an objective predicate; SMT proves preconditions on the search domain. |
| Cost / budget (Phase 11) | `Cost<n>` is a refinement over an abstract counter; `@[budget(N)]` is E1200 if `cost <= N` cannot be proved. |

Every later layer *adds new predicate-language constructs* (effects, distributions, shapes,
flows, costs). The solver, the path condition, the proof-obligation worklist, and the
counter-example formatter are written **once**, in Phase 5.
