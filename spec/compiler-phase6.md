# Axon Compiler — Phase 6 Spec (ASI Layer 5)

**Goal**: Promote effects from a list of attribute keywords (`@[contained(IO, Net)]`) to a
*row-polymorphic effect system* with effect handlers. This is the substrate every later
phase rides: Principal capabilities (Phase 7) become rows; `Store<T>` operations carry
their effect row in their type; `LLM<Caps>` enforces budget metering as a typed effect;
`Sandbox<P>` (Phase 9) is the runtime counterpart of the static row.
**Builds on**: Phase 5 (`spec/compiler-phase5.md` — refinement + SMT).
**Timeline**: a multi-iteration ASI project — row-polymorphic effect inference,
handler lowering, and the substrate/surface gate sit on the critical path;
each can be drafted in one iteration cycle but the lowering needs a
validation cycle against the full fixture suite + a benchmark cycle to
confirm zero codegen overhead vs. Phase 5.

---

## Phase 6 Scope

### In Phase 6
- Effect rows on every fn signature (`fn f() -> T | {IO, Net}`)
- `pure` is the empty row; default for fns without an annotation
- Row variables for polymorphism (`fn map<E>(f: A -> B | {E}, xs: Vec<A>) -> Vec<B> | {E}`)
- Effect handlers (`with handler { … }` block) à la Koka / OCaml 5
- Built-in effects: `IO`, `Net`, `AI`, `Random`, `Time`, `Spawn`, `Chan`
- `@[contained]` attribute deprecated → effect row constraint
- Substrate / surface file markers (`substrate` / `surface` declaration at top of file)
- Effect-row diagnostics (E1306 + the E131x block — see §8; the originally-proposed
  E1300–E1308 collided with the AI-policy codes E1300–E1302)
- `@[pure]` from Phase 5 becomes `effect = {}` (the empty row); attribute kept as sugar

### Explicitly Out of Phase 6
```
Algebraic effects with continuations beyond shallow handlers → future
Effect inference for unannotated fns                       → Phase 8 (with goal/agent)
Cross-Principal effect masking                              → Phase 7
Runtime effect enforcement / sandbox                        → Phase 9
First-class effect values / reflection                      → future
Effect polymorphism over parameter direction (read/write)   → future
Linear effects (must-discharge)                             → future
```

---

## 1. Surface Syntax

### Effect row on a function signature

```ebnf
fn_def       ::= "fn" ident generic_params? "(" param_list ")" return_clause? eff_clause? block
return_clause ::= "->" type
eff_clause   ::= "|" "{" eff_list "}"
eff_list     ::= eff_term ("," eff_term)*
eff_term     ::= ident                  ; concrete effect (e.g. IO, Net)
              | row_var               ; lowercase identifier ⇒ row variable
              | "..." row_var         ; row-extension (rest of effects)
```

### Examples

```axon
// Pure (the empty row).  Default if no `|` clause.
fn add(a: i64, b: i64) -> i64 { a + b }
fn add_explicit(a: i64, b: i64) -> i64 | {} { a + b }

// One concrete effect.
fn fetch(url: str) -> Result<Bytes, str> | {Net} { /* ... */ }

// Multiple effects.
fn save(path: str, content: str) -> Result<(), str> | {IO, Time} { /* ... */ }

// Row-polymorphic: forwards whatever caller had.
fn map<A, B, e>(xs: Vec<A>, f: A -> B | {e}) -> Vec<B> | {e} { /* ... */ }

// Row-extension: function adds `IO` to whatever the callback's row is.
fn with_log<A, e>(f: () -> A | {e}) -> A | {IO, ...e} {
    println("calling")
    f()
}
```

### Effect handlers

```ebnf
handle_expr  ::= "with" handler_expr "{" block_body "}"
handler_expr ::= ident                  ; named handler (resolved by name)
              | inline_handler         ; structural

inline_handler ::= "handler" "{" handler_arm+ "}"
handler_arm   ::= "on" eff_term "(" pattern ")" "=>" expr
              | "return" "(" pattern ")" "=>" expr   ; final-value rewrite
```

### Examples

```axon
// Static handler — defined at module scope, named, reusable.
handler retry_on_net = handler {
    on Net(req) => {
        match req {
            Ok(r)  => resume(r)
            Err(_) => resume(retry(req))
        }
    }
    return(v) => v
}

// Use it:
fn pull_data() -> Bytes | {Net} { /* ... */ }

fn safe_pull() -> Bytes {
    with retry_on_net {
        pull_data()
    }
}
// safe_pull's row is {} — the handler discharges Net.

// Inline handler.
fn log_io_calls() -> () | {Net} {
    with handler {
        on IO(payload) => {
            eprintln("IO call: {payload}")
            resume(())
        }
        return(v) => v
    } {
        save("/tmp/x", "hello")?
        fetch("https://example.com")?
        ()
    }
}
```

### Substrate/surface file markers

The first non-comment token of a `.ax` file may be `substrate` or `surface`:

```axon
// At top of file:
substrate

// or
surface
```

If absent, the default is `substrate` (preserves Phase 1–5 behavior; surface is opt-in).
The marker tunes which constructs the checker accepts:

| Construct | substrate | surface |
|---|---|---|
| Direct effect-row syntax `\| {Net}` | ✅ | ❌ E1306 |
| `unsafe` block (Phase 6+) | ✅ | ❌ |
| Manual memory ops (Phase 6+) | ✅ | ❌ |
| Untyped FFI (Phase 6+) | ✅ | ❌ |
| `goal`/`agent`/`for!` constructs (Phase 8) | ⚠️ warn | ✅ |
| `effect` definitions | ✅ | ❌ |
| `handler` definitions | ✅ | ⚠️ warn |
| `for!`/`with` blocks | ✅ | ✅ |
| Refinements (`T where P`) | ✅ | ✅ |
| Annotations (`@[goal]`, `@[verify]`, `@[adaptive]`) | ✅ | ✅ |

The intent: substrate is for library authors and effect/refinement plumbing; surface is
for product engineers writing goals/agents/loops with safe defaults.

---

## 2. Semantic Rules

**E01 (default row)** — A `fn` with no `| {…}` clause has effect row `{}` (pure).

**E02 (subsumption)** — A function whose declared row is `R` may not call any function
whose declared row is not a subset of `R` (or covered by an active handler in scope).
Concretely: at every call site, the called function's row, after handler discharge, must
be `⊆ R`.

**E03 (closing rows)** — A row variable `e` is a fresh metavariable in the type-inference
phase. At a leaf call (one not inside a handler) it must unify with a concrete row.
Unbound row variables at the top level of `main` produce E1312 (📋 reserved) unless the program's
`main` declared a row of `{IO, ...e}` (escape hatch for top-level effects, see §3).

**E04 (handler discharge)** — A `with H { e }` block's effective row is `effects(e) − discharged_by(H)`.
A handler that discharges `Net` removes `Net` from the body's row in the surrounding
context.

**E05 (forbidden in pure)** — A `pure` function (empty row) may not call:
- Any function with a non-empty row (unless wrapped in a handler that discharges all).
- AI builtins (`ai_complete`, `ai_extract*`) — these carry `{AI, Net}`.
- IO builtins (`println`, `eprintln`, `read_file`, `write_file`) — `{IO}`.
- Random / time builtins (`random_*`, `now_ms`, `sleep_ms`) — `{Random}` / `{Time}`.
- Channel ops — `{Chan}`.
- Spawn — `{Spawn}`.

**E06 (effect row in refinement predicates)** — Refinement predicates (Phase 5) admit
calls only to `effect = {}` functions. The Phase-5 `@[pure]` attribute is exactly
sugar for an empty row. Predicates may not depend on AI/IO/Random/Time results.

**E07 (handler scope)** — A `with` block introduces a handler that is active only
inside its body. Handlers do not leak across function boundaries unless captured
explicitly into a closure.

**E08 (handler resumption)** — Inside a handler arm, `resume(value)` resumes the
captured continuation with `value` as the result of the operation that was intercepted.
Phase 6 supports **shallow** handlers only: each operation's continuation may be
resumed at most once. Multi-shot handlers (resume more than once) are deferred.

**E09 (substrate/surface gate)** — In a `surface`-marked file, raw `| {…}` syntax is
rejected (E1306). Surface code declares effects through annotations (`@[uses(Net)]`,
later `effects: {Net}` field on Goal/Agent) which lower to substrate effect rows.

---

## 3. Built-in Effects (Phase 6 catalog)

```
effect IO       // print, file read/write
effect Net      // HTTP, TCP, DNS, anything off-machine
effect AI       // model inference (any LLM call)
effect Random   // random_*, any nondeterministic source
effect Time     // now_ms, sleep_ms, validity-checking on Temporal<T>
effect Spawn    // spawn { … }
effect Chan     // chan<T>(), .send, .recv, select
effect Verify   // panicking on @[verify] miss (Phase-3.5 path)
```

Future phases extend this list: `effect Store` (Phase 7), `effect Audit` (Phase 9),
`effect Sandbox` (Phase 9), `effect Risk` (Phase 11), etc.

### Builtin row assignments (binding)

| Builtin | Row |
|---|---|
| `add`, `mul`, `to_str`, `parse_int`, `str_*`, `i64_to_f64`, `f64_to_i64`, `abs_*`, `min_*`, `max_*`, `confidence`, `value`, `validity` | `{}` |
| `println`, `eprint`, `eprintln`, `print` | `{IO}` |
| `read_line`, `read_file`, `write_file` | `{IO}` |
| `now_ms`, `sleep_ms` | `{Time}` |
| `random_i64`, `random_f64` | `{Random}` |
| `env_var`, `exit` | `{IO}` |
| `ai_complete`, `ai_extract*` | `{AI, Net}` |
| `chan<T>()`, `chan.send/recv/clone`, `select` | `{Chan}` |
| `spawn { … }` | `{Spawn}` |
| `goal_run` | `{AI, Net, IO}` *(it can re-call adaptive fns + log + read provenance)* |

### Top-level escape hatch

`fn main()` may declare any row, including a row variable, but for ergonomic reasons
the default declared row of `main` is `{IO, Net, AI, Random, Time, Spawn, Chan, ...e}`
where `e` is open. This means user code in `main` can do anything; subordinate
functions still must declare rows or be pure.

---

## 4. `@[contained]` Migration

Phase 3 / Layer-3 introduced `@[contained(IO, Net)]` as an attribute that informs the
checker. Phase 6 turns this into a *desugaring*:

```axon
@[contained(IO, Net)]
fn save_remote(url: str, body: str) { /* ... */ }

// Equivalent under Phase 6:
fn save_remote(url: str, body: str) -> () | {IO, Net} { /* ... */ }
```

The attribute is kept as sugar for one phase; a future deprecation notice (the reserved
E1316 — see §8) will flag it after Phase 7. The existing `@[contained]` errors E1001–E1004
keep their own codes — they are NOT aliased onto the effect-row block (an earlier draft
proposed aliasing them onto E1300–E1303, but those numbers are taken by the AI-policy
diagnostics, so the effect rules live in the E131x block instead).

---

## 5. Type Inference

The infer pipeline is extended with effect-row unification:

1. **Bare types unify as before** (HM).
2. **Effect rows unify as multisets** with row-variable extension. Row-variable
   matching uses a *closed-world* assumption inside a function body and an *open-world*
   assumption across function boundaries.
3. **At each call site**, the callee's row is substituted by the call's type-arg
   substitution and required to be `⊆` the caller's declared row (after handler
   discharge along the path).
4. **Handler effects** are computed by `effects(handler_arm) = ⋃ row(arm.body)`. The
   handler's *discharged* set is `{eff_term | on eff_term(...) => ... in handler}`.
5. **Default row** for fns without `| {…}` is the empty row, *fixed* (not a row
   variable), to preserve "pure-by-default" semantics.

### Inference complexity

Row unification is decidable and runs in O(n·m) for n call sites with rows of size
m. The added cost on existing programs is negligible because most functions have row
size 0 or 1.

---

## 6. Codegen

> **Implementation status.** The **interpreter** implements real shallow,
> single-shot, tail-resumptive handlers: a `with handler { on E(p) => arm } { body }`
> intercepts a builtin in `body` carrying effect `E`; `resume(v)` makes `v` the
> operation's result and the body continues; an arm that returns without resuming
> replaces the `with` block; a `return(v)` arm rewrites the body value. The arm runs
> *outside* its own handler (shallow), so a self-effecting arm does not loop.
> **Codegen handler lowering (this section) is NOT yet implemented** — native codegen
> still *erases* handlers (lowers `with H { body }` to `body`). This is a documented,
> bounded **interp↔native (I-2) divergence** confined to programs that actually
> resume a builtin effect; none ship under `examples/`, and `all_examples_parity`
> would catch any that did, so the gap cannot silently widen. Closing it = building
> the lowering below.

Effect rows have no runtime representation. The checker discharges them statically;
codegen emits the same LLVM IR it would have without them. This means:

- No allocation per effect.
- No vtable lookup for handlers — handlers are inlined at the `with` block boundary.
- Existing performance characteristics are preserved.

Handlers compile to:
1. A **frame** that captures the block's continuation as a fresh function.
2. **Replacement** of the `effect_op(args)` call inside the body with a dispatch
   that calls into the matching `on` arm.
3. The arm's `resume(v)` lowers to a tail call into the captured continuation
   with `v`.

This is the standard zero-cost effect-handler lowering used by Koka and Multicore
OCaml; Axon's restriction to *shallow* handlers means no full delimited-continuation
runtime is needed — a single function-pointer + frame pointer per active handler
suffices.

---

## 7. Implementation Plan

### File map

```
crates/axon-core/src/
  effects.rs      (NEW) effect row, row variable, unification, subset check, builtin assignments
  handlers.rs    (NEW) handler AST + resolve + frame layout for codegen
  ast.rs          +EffectRow on FnDef; +Expr::WithHandler; +Item::HandlerDef; +Item::EffectDef
  parser.rs       parse `| {…}` clause, `with handler { … }`, `effect`, `handler`, file markers
  types.rs        Type::Function gains an `eff: EffectRow` field
  infer.rs        row unification; substitution into rows; row-variable management
  checker.rs      E1306 + E131x block; E01–E09 enforcement; substrate/surface gate
                  (effects.rs holds the E1310 subsumption pass)
  codegen.rs      handler frame + resume lowering; `with` block IR; row stripping at codegen entry
  builtins.rs     +eff field per BuiltinFn; bind rows from §3 catalog
crates/axon-core/Cargo.toml
  (no new external deps; effect rows are pure-Rust data)
```

### Pipeline position

```
Lexer → Parser → Resolver → fill_captures → Infer (HM + row) → Checker → Borrow → Refine → Effects-check → [Mono] → Codegen → LLVM → binary
                                                                    ↓
                                                       NEW: effects.rs::check_program
```

The dedicated `Effects-check` pass runs after `Refine` (Phase 5) and before
monomorphization. Refinement constraints are checked first (so a refinement violation
is reported before an effect violation, which is usually more informative).

### Backwards compatibility

- Every Phase 1–5 example and fixture continues to compile unchanged: no `| {…}`
  clauses exist anywhere, so all fns get the empty row, all builtins are looked up
  via the new `eff` field, and `@[contained]` keeps working through the migration
  alias.
- The `substrate`/`surface` marker is *opt-in*; absence means `substrate` semantics,
  which is what every current file expects.
- The runtime is unchanged; effect rows are erased before codegen.

---

## 8. New Error Codes

> **Numbering note.** This section originally proposed `E1300–E1308` for effect-row
> diagnostics, but `E1300/E1301/E1302` were already taken by the Layer-3 AI-policy
> conditions (offline-no-fallback / AI-budget / unknown-tier — see `error.rs` and
> `spec/runtime.md` §5; those exit with the dedicated AI-policy code 5). To avoid a
> collision the effect-row diagnostics use the **free E1310 block**, plus the already-shipped
> **E1306** for the substrate/surface gate. The authoritative registry is `error.rs`; the
> table below reflects what is implemented (✅) versus reserved-but-not-yet-emitted (📋).

```
E1306  ✅ effect-row syntax `| {…}` not allowed in a surface-marked file; use @[uses(...)] or move to substrate
E1310  ✅ effect-row leak: a call performs effect E ∉ the caller's declared row
                (covers spec rules E02 subsumption + E05 unhandled-at-top-level)

— reserved in the E131x block for the remaining Phase-6 rules (📋 not yet emitted) —
E1311  📋 effect E used in a @[pure] / refinement-predicate context
E1312  📋 unbound row variable {e} reaches main; declare or handle
E1313  📋 handler arm references unknown effect '{name}'
E1314  📋 resume() called twice in a shallow handler arm (multi-shot deferred)
E1315  📋 substrate-only construct in a surface file: {construct}
E1316  📋 @[contained(...)] deprecation notice; prefer `| {…}` row syntax
```

The Layer-3 `@[contained]` errors (E1001–E1004) remain their own codes; they are **not**
aliased onto the effect-row block (the original draft proposed aliasing them onto
E1300–E1303, which the AI-policy collision made impossible). A future deprecation that
bridges `@[contained]` to row syntax would emit the reserved E1316 notice, leaving the
E100x codes intact.

---

## 9. CLI Summary (Phase 6 additions)

```
axon build <file> --no-effects-check       skip effects pass (debug)
axon build <file> --effects-strict         demote E1316 deprecation notices to errors
axon check <file> --explain E1310          long-form doc for an effect-row error
axon doc <file> --effects                  include effect rows in generated docs
axon fmt <file>                            preserves effect-row clauses, normalizes order
```

---

## 10. Verification Checklist

Phase 6 is done when:

- [ ] `fn fetch() -> Bytes | {Net}` parses and the row appears in `axon doc` output.
- [ ] `fn pure_caller() { fetch() }` is rejected with E1310 (effect-row leak).
- [ ] `fn impure() -> Bytes | {Net} { fetch() }` is accepted.
- [ ] `with retry_on_net { fetch() }` returns `Bytes` with row `{}` in the calling
      context.
- [ ] Row variable `e` propagates through `map<A,B,e>(xs, f)` correctly: when `f` has
      row `{IO}`, the call has row `{IO}`; when `f` has row `{}`, the call has row
      `{}`.
- [ ] An `@[adaptive] fn try_variant(i: i64) -> i64 | {AI, Net}` is accepted; the
      Phase-3 hill-climb still works because `goal_run` has the same row.
- [ ] A surface-marked file using raw `| {Net}` produces E1306.
- [ ] A substrate-marked file using raw `| {Net}` is accepted.
- [ ] `@[contained(IO)]` continues to work but emits the E1316 deprecation notice in
      `--effects-strict`.
- [ ] Refinement predicate (Phase 5) using `now_ms()` is rejected with E1311.
- [ ] All Phase 1–5 examples compile unchanged.
- [ ] A new fixture `crates/axon-core/tests/integration_fixtures/effects.ax`
      exercises: row declaration, row variable, handler discharge, `with` block,
      surface/substrate markers, effect leak rejection, `@[contained]` deprecation.
- [ ] Codegen produces no measurable overhead vs. Phase 5 on the existing benchmarks.
- [ ] `examples/asi/optimize.ax` annotated with effect rows compiles, runs, and
      ROADMAP §9.5 gap F4 (budget meter) is closer to addressable: every `ai_complete`
      call now carries `{AI, Net}` in its type, ready for Phase-7 `LLM<Caps>` to
      meter.

---

## 11. Carry-Forward into Phase 7+

| Phase | Uses Phase 6 mechanism |
|---|---|
| 7 (Principal/Store/Supervisor) | Each `Principal` carries an *allowed* effect row; effect leak past row → E1310. `Store<T>` ops carry `{Store}` (phase-extended catalog). `LLM<Caps>` enforces budget by intercepting `{AI, Net}` via a handler. |
| 8 (Goal/Agent) | `agent { effects: { Net, AI } }` lowers to a function-row constraint; `for!` blocks track effect rows of their proposers. |
| 9 (Sandbox/Replay/Audit) | `Sandbox<P>` is the runtime counterpart of static row P; `Audit` is a new effect that wraps every other effect; replay engine intercepts every effect operation and records its arguments. |
| 11 (Risk/SimulationGate) | `Risk` is a *derived* property over the effect row + budget magnitude + irreversibility annotation. |

Every later layer composes by adding new effect names to the catalog and new
handlers in stdlib; the row machinery, the substrate/surface gate, and the
discharge rules are written **once**, in Phase 6.
