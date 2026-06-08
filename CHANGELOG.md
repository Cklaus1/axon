# Axon Changelog

## Tooling

- **World-model / compress-to-fit loop (prototype #1)** — `examples/stdlib/world.ax` +
  `examples/asi/world_model.ax` + `spec/worldmodel-loop.md`. An executable world model that
  PREDICTS, is CHECKED against observations (`fit_error`), and is COMPRESSED toward the
  simplest parameters that fit: `goal_run` hill-climbs to maximize `fitness = fit − λ·MDL`,
  with **fit as a hard gate** (a non-fitting model is a refinement violation — `Fitted = World
  where fit_error(_) <= 0`, exit 6 — so simplicity only breaks ties, never wins by being
  simple-and-wrong). Demonstrates "improvement = fewer description-length bits" with a 2-param
  model that discovers it doesn't need the offset. Built entirely on shipped primitives
  (`@[adaptive]`/`goal_run`/refinements/the `axon complexity` MDL idea); the kernel `World<T>`
  + `observe`/`condition` keywords + probabilistic fit are the named Phase-13 follow-on.
- Self-improving compiler (R10): **corpus breadth hardening** — every registry pass
  (fold-arith-identities, constant-fold, bool-simplify) + identity is now driven through the
  four gates over a diverse 11-program corpus spanning the language surface (recursion,
  for/while loops, structs, enums+match, closures, strings+interpolation, Option/Result+`?`,
  nested arithmetic, and a deliberate div-by-zero panic). The G1 oracle is only as strong as
  its corpus; this proves the passes are behavior-preserving across constructs, not just toy
  programs — corpus hardening toward trusting free-form (Layer-3) authorship.
- Self-improving compiler (R10): **firewall red-team hardening** — adversarial passes proving
  the four gates REJECT bad candidates (the prerequisite for trusting free-form pass authorship):
  a stdout-only change (G1/E1401, the half of the observable tuple the prior test didn't cover),
  an `exec` capability injection (caught on BOTH G2/E1402-I-12 and G1), and **panic erasure** —
  a "fold" of `10/0` (exit 101) to a literal (exit 0) is rejected by G1, proving the exact
  soundness property the real constant-fold/bool-simplify passes rely on is enforced by the gate,
  not merely respected by the passes.
- Self-improving compiler (R10): a **third** oracle-verified optimization pass — `bool-simplify`
  (`!true`→`false`, `!false`→`true`, `!(!x)`→`x`) — added to the closed template registry and
  proven through the four-gate harness (G1/G2/G3 over the real corpus). All three rewrites are
  provably total and behavior-preserving (`!` on a bool never panics; double-negation preserves
  the operand's single evaluation, so it's sound regardless of operand purity). The registry now
  carries three passes — widening what the bounded proposer (`discover`) can select.
- Self-improving compiler (R10): a **second** oracle-verified optimization pass —
  `constant-fold` (integer-literal arithmetic, `2 + 3` → `5`) — added to the closed template
  registry and proven through the four-gate harness (`axon improve verify --pass
  constant-fold` clears G1 correctness + G2 capability-safety + G3 regression over the real
  examples corpus). Matches the interpreter's CHECKED arithmetic exactly: it never folds an
  overflow or division-by-zero, so a runtime panic is preserved (folding it would be a G1
  failure). Demonstrates the **"simpler, not just faster" improvement axis** — folding
  strictly reduces `axon complexity` MDL bits with identical behavior. The registry is now
  proven extensible; `--pass` resolves any registered template.
- `axon complexity <file> [--json]` — a minimum-description-length (MDL) metric over the
  typed AST: the *bits* to describe the program, per function and whole-program, with a
  per-kind cost breakdown. Deterministic, format-invariant (AST-based, not text), and
  monotone. The "measure of simplest program" a compression loop minimizes — the reusable
  fitness primitive for the world-model / `goal { minimize complexity, subject_to: fits_obs }`
  pattern. `--json` emits a stable `axon-complexity/1` object for tools/agents.

## Phase 6 (current) — Row-polymorphic effects + handlers

### Effect system
- Effect rows on fn signatures: `fn f() -> T | {IO, Net, ...e}` (parse + `axon fmt` + `axon doc`)
- Builtin effect catalog (`builtin_effect_row`); subsumption checker (E1310) with
  TRANSITIVE anti-laundering — an effect can't hide behind an un-annotated helper
- Effect-laundering holes closed across the shared walkers (with-blocks, for-loops,
  spawn/select/comptime, lambda bodies) in both the effect checker and the
  capability/import-edge walks
- Handler discharge (E04): inline AND named handlers (`handler NAME = handler {…}`,
  resolved via parser desugar) discharge their arms' effects
- `resume` runtime semantics (interpreter): shallow, single-shot, tail-resumptive —
  a handled builtin's result is replaced by `resume(v)` and the body continues; an
  arm runs outside its own handler (no self-interception)
- Native codegen LOWERS the tail-resumptive direct-builtin handler subset
  (byte-parity with the interpreter, `handler_resume_parity.sh`); everything outside
  the subset is honestly E0910-refused (never silently miscompiled)
- `substrate`/`surface` file markers (E1306); `@[contained]`→effect-row bridge
- Cross-annotation consistency: `@[pure]` + a non-empty row → E1207; a `@[contained]`
  capability contradicting a too-small row → E1310

### Soundness & security fixes
- `@[contained]` path-traversal sandbox escape closed (`./out/../etc` no longer
  matches a `./out/` allowlist) — E1001
- Import-edge capability check (E1203) now sees capabilities inside
  with/spawn/select/comptime (was laundering past the importer's ceiling)
- Refinement predicates must be pure — an impure builtin (`now_ms`, `random_i64`) in
  a `where` clause is rejected (E1209)
- `@[total]` now rejects `while` loops (incl. hidden in a lambda) — termination can't
  be established for unbounded loops (E1208); requires bounded `for`/recursion
- Native↔interpreter (I-2) exit-code parity: AI-policy conditions (E1300–E1302) exit 5;
  native panics exit 101 to match the interpreter; `exit_code_parity.sh`

### Phase 5 — Refinement types + `@[pure]`/`@[total]` (no Z3 yet)
- `@[pure]` purity checker (E1207); `@[total]` termination checker (E1208)
- Refinement types `T where <pred>` (named + inline), transparent to the base type,
  with constant-predicate obligations at arg/return/struct-field sites (E1209)
- Refinement contracts on NON-constant values are now enforced at runtime on BOTH
  sides — the spec's Z3-free fallback (§4, `--proof-timeout 0`: "every predicate
  becomes a runtime check"):
  - PRECONDITIONS: at function entry a parameter `p: T where P` has `P` evaluated
    with `_` bound to the actual argument.
  - POSTCONDITIONS: at every return site a fn `-> T where P` has `P` evaluated with
    `_` bound to the returned value (the dual hole — `f(x:i64) -> Positive { x - 100 }`
    used to return a negative value with no error).
  - STRUCT CONSTRUCTION: each refined field, and any whole-struct `where` predicate
    (`type Range = {lo,hi} where _.lo <= _.hi`, binder `_` = the instance), checked
    when the struct is built.
  - LET BINDINGS: a `let/own/ref p: T where P = …` annotation checked against the
    bound value (and the previously-missing CONSTANT case is now a static E1209 too).
  A violation (any site) exits 6 (REFINE_VIOLATION_EXIT_CODE), distinct from a
  @[verify] bound (3) and a bug-panic (101). Enforced in BOTH the interpreter and
  native codegen (byte-identical exit codes, `exit_code_parity.sh`); predicates
  outside the lowerable subset are E0910-refused in codegen, never silently skipped.
- The SMT backend (`smt.rs`, `axon verify`, opt-in `smt` feature) statically discharges
  what it can prove — `@[verify]` bounds, refinement returns, and refinement
  arg-forwarding subtyping; the runtime checks above cover the rest in the default build.
- Remaining: WIDEN SMT static discharge so a provable obligation elides its runtime check
  in the default pipeline (today the runtime check always fires for non-constant cases)

### Phases 3–4 — Generics/traits/closures/channels; LSP/fmt/doc/multi-file
- Generics, structural traits, closures with captures, channels, borrow checker,
  comptime, spans (Phase 3)
- LSP (hover/diagnostics), formatter, doc generator, incremental compile,
  multi-file, cross-compile (Phase 4)

### ASI layers — `Uncertain<T>`/`Temporal<T>`, `@[verify]`/`@[adaptive]`, goals
- `goal_run` hill-climb, `ai_complete`/`ai_extract*`, `@[agent]` audit trail (incl.
  transitive), `@[corrigible]` kill-switch (exit 4), `@[sensitive]` PII taint (E1206)

## Phase 2

### Compiler features
- Struct types: `type Point = { x: f64, y: f64 }`, field access, struct literals
- Enum ADTs: tagged union layout, `Type::Variant { field }` constructors, pattern matching
- Slice/array indexing: `arr[i]`, heap-allocated backing
- While loops: `while cond { body }`, assignment rebinding `x = expr`
- Lambdas: `|x| expr` lowered to `__lambda_N` module-level functions
- String interpolation: `"hello {name}"` lowered to `axon_concat` chains
- Modulo operator: `%` (`BinOp::Rem`)
- Logical operators: `&&` and `||`
- String escape sequences: `\n`, `\t`, `\\`, `\"`, `\r`, `\0`
- Float scientific notation: `1.5e10`, `3.14e-3`
- Block comments: `/* ... */`

### Extended builtins
- `assert_eq(a: i64, b: i64)` — equality assertion with values
- `assert_err(tag: bool)` — assert Result is Err
- `to_str_f64(n: f64) -> str` — float to string
- `len(s: str) -> i64` — string byte length
- `parse_int(s: str) -> Result<i64, str>` — string to integer
- `abs_i32`, `abs_f64`, `min_i32`, `max_i32` — math operations
- `axon_concat(a: str, b: str) -> str` — string concatenation (runtime)

### Bug fixes
- `Result<T,E>` canonical union layout `{i1, [max(sizeof T, sizeof E) x i8]}` — fixes phi-node type mismatch in if/else
- `eprint`/`eprintln` now correctly write to stderr
- `to_str`/`to_str_f64` use heap-allocated buffers (not static — re-entrant)
- Array literals use heap allocation (prevents dangling pointer on return)
- `?` operator correctly extracts typed Ok payload
- `parse_int` Err variant stores valid empty str struct
- LLVM module verification before JIT/AOT emission
- Lambda emission saves/restores `local_types`
- `build_return(None)` replaced with typed zero-value return for non-void functions
- Unsigned integer widening uses `zext` not `sext`
- Cyclic type variable substitution now detected and broken (no infinite loop)
- `abs_i32`, `min_i32`, `max_i32` parameters changed to `i64` so integer literals pass without explicit cast
- Implicit signed integer widening (`i8→i16→i32→i64`) allowed in infer, checker, and codegen call sites
- `@[test]` attribute syntax fixed throughout (was incorrectly `#[test]` in some docs and comments)

### Test infrastructure
- `@[test(should_fail)]` — subprocess-based test that passes when program panics
- `axon test` now runs all tests as subprocesses (prevents one panic killing the suite)
- Test functions validated to have zero parameters before execution

### CLI improvements
- `axon parse` outputs valid JSON (not Rust Debug format)
- `axon run/build/check/test` validate `.ax` file extension
- `axon run --release` flag for optimized builds
- Standardized exit codes: 0=success, 1=I/O error, 2=compile error, 3=test failure

### Specs written
- `spec/compiler-phase2.md` — Phase 2 feature spec
- `spec/compiler-phase3.md` — generics, traits, closures, channels, borrow checker, comptime, spans
- `spec/compiler-phase4.md` — LSP, formatter, doc gen, incremental compilation, multi-file, cross-compile
- `spec/grammar.ebnf` — regenerated to match current parser (was stale from Phase 1)
- `spec/stdlib.md` — standard library reference with all 17 builtins
- `spec/runtime.md` — C ABI / runtime function reference
- `spec/language-tour.md` — hands-on language walkthrough

### Developer tooling
- `dev.sh` — `./dev.sh full` runs complete CI pipeline
- 17 example programs in `examples/`

## Phase 1

### Compiler features
- Lexer: `logos`-based tokenizer for all Axon tokens
- Parser: hand-written recursive descent, produces full AST
- Resolver: name resolution, scope analysis, `collect_top_level` two-pass
- Type inference: Hindley-Milner with constraint solving and `Substitution`
- Type checker: 12 semantic rules (R01–R12), Levenshtein suggestions
- Codegen: LLVM IR via `inkwell 0.4`, JIT execution, AOT native binary
- Linker: system `cc` via `which` crate

### Builtins (Phase 1)
- `print`, `println`, `eprint`, `eprintln`
- `assert(bool)`
- `to_str(i64) -> str`
- `format(template: str) -> str`

### CLI
- `axon run <file>` — compile and run
- `axon build <file>` — compile to native binary
- `axon check <file>` — type-check only
- `axon test <file>` — run `@[test]` functions
- `axon parse <file>` — print AST

### Language features
- Functions with parameters and return types
- `let`/`own`/`ref` bindings
- `if`/`else` expressions
- `match` expressions with patterns
- `Result<T,E>` and `Option<T>` types
- `Ok(x)`, `Err(e)`, `Some(x)`, `None` constructors
- `?` postfix error propagation operator
- Basic arithmetic: `+`, `-`, `*`, `/`
- Comparisons: `==`, `!=`, `<`, `>`, `<=`, `>=`
- Block expressions, `return` statement
- Recursive functions
- Deferred AI annotations: `#[agent]`, `#[goal]`, `#[adaptive]`, `#[verify]`, etc.
