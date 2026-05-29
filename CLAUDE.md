# Axon Language

AI-optimized, statically-typed systems language. Compiles to native via LLVM 17.

## Design Principles

- No null, no exceptions — `Option<T>` and `Result<T,E>` everywhere
- Ownership without GC — simplified two-mode ownership (`own`/`ref`)
- Structural typing — no `implements` keyword
- Comptime — zero-cost compile-time execution
- Multi-target — native, wasm, js (Phase 4+)
- AI-first — `@[agent]`, `@[goal]`, `@[verify]`, `@[adaptive]` annotations built-in

## File Extension

`.ax` — e.g. `main.ax`, `server.ax`

## Commands

```bash
# Build the fast interpreter CLI (no LLVM/codegen — sub-second):
cargo build -p axon-core --no-default-features --bin axon
cargo test                          # run all unit + integration tests

axon run   examples/hello.ax              # type-check + interpret (tree-walking, no codegen)
axon goal  examples/goals/hello-goal.md   # compile prose goal → .ax → check → run
axon check examples/hello.ax              # type-check only
axon test  examples/tests.ax              # run @[test] functions (in-process interpreter)
axon parse examples/hello.ax              # print AST as JSON   (needs --features serde-json)
axon build examples/hello.ax              # native AOT binary   (needs --features codegen; SLOW — see BUILD_DIAGNOSIS.md)
```

**Execution is interpreter-first.** `run`/`goal`/`test`/`check` work without the
`codegen` feature via the tree-walking interpreter (`interp.rs`). The native
LLVM/inkwell `codegen` feature is **on by default** but its build is
pathologically slow (does not finish — see `BUILD_DIAGNOSIS.md`), so it's
relegated to CI/release; develop against `--no-default-features`. Add
`--features asi-runtime` to enable live `ai_complete`/`ai_extract_*` (used by
`examples/asi/*` and `axon goal`).

## Compiler Pipeline

```
.ax source
  → Lexer    (token.rs / lexer.rs)      logos-based tokenizer
  → Parser   (parser.rs)                recursive descent → AST (ast.rs)
  → Resolver (resolver.rs)              name resolution, scope building
  → Infer    (infer.rs / types.rs)      Hindley-Milner type inference
  → Checker  (checker.rs)               semantic validation, diagnostics
  → Codegen  (codegen.rs)               LLVM IR via inkwell 0.4
  → Link     (main.rs)                  cc linker → native binary
```

## Crate Structure

```
crates/axon-core/src/
  token.rs      Token enum (logos derive macros)
  lexer.rs      Lexer::tokenize() → Vec<(Token, Span)>
  ast.rs        Program, Item, Expr, Stmt, Type, Pattern, Literal
  parser.rs     Recursive descent Parser → AST
  resolver.rs   SymbolTable, name resolution, scope analysis
  types.rs      Type enum, Substitution, unification
  infer.rs      Hindley-Milner inference, constraint solving
  checker.rs    Semantic rules R01-R12, diagnostics
  builtins.rs   BUILTINS table, builtin_sigs(), DEFERRED_ATTRS
  codegen.rs    LLVM IR codegen via inkwell
  error.rs      CompileError, Diagnostic types
  lib.rs        parse_source() public API
  main.rs       axon CLI (run/build/check/test/parse commands)
spec/
  compiler-phase1.md   Phase 1 spec (complete)
  compiler-phase2.md   Phase 2 spec (complete)
  compiler-phase3.md   Phase 3 spec (generics, traits, closures, channels)
  compiler-phase4.md   Phase 4 spec (LSP, formatter, multi-file, caching)
  grammar.ebnf         Formal EBNF grammar
  stdlib.md            Standard library reference
  runtime.md           C ABI / runtime function reference
  language-tour.md     Hands-on language walkthrough
examples/
  hello.ax             Hello world
  math.ax              Basic arithmetic
  structs.ax           Struct types and field access
  enums.ax             Enum ADTs and pattern matching
  slices.ax            Array/slice indexing
  options.ax           Option<T> usage
  while.ax             While loops (sum 1..10 = 55)
  interpolation.ax     String interpolation
  algorithms.ax        GCD, primes, Collatz, power
  modulo.ax            FizzBuzz with %
  logical_ops.ax       && and || operators
  floats.ax            Scientific notation floats
  escapes.ax           String escape sequences \n \t \\
  math_builtins.ax     abs_* min_* max_* builtins
  parse_int.ax         parse_int with Result matching
  comprehensive.ax     Multi-feature integration test
  stdlib_tests.ax      @[test] suite for all builtins
  tests.ax             assert_eq based unit tests
  should_fail_test.ax  @[test(should_fail)] demo
```

## Language Quick Reference

```axon
// Variables
let x = 42            // i64
let y = 3.14          // f64
let s = "hello"       // str
let b = true          // bool
x = x + 1            // reassignment (no let)

// Functions
fn add(a: i64, b: i64) -> i64 { a + b }

// Structs
type Point = { x: f64, y: f64 }
let p = Point { x: 1.0, y: 2.0 }
println(to_str_f64(p.x))

// Enums
type Shape = Circle { r: f64 } | Square { side: f64 }
let s = Shape::Circle { r: 5.0 }

// Control flow
if x > 0 { "pos" } else { "non-pos" }
while i < 10 { i = i + 1 }
match val { Ok(n) => n  Err(e) => 0 }

// Error handling
fn parse(s: str) -> Result<i64, str> {
    let n = parse_int(s)?
    Ok(n * 2)
}

// String interpolation
println("hello {name}, age {to_str(age)}")

// Operators: + - * / %   == != < > <= >=   && ||

// Builtins: println print eprint eprintln
//           to_str to_str_f64 parse_int len
//           abs_i32 abs_f64 min_i32 max_i32
//           assert assert_eq assert_err

// Testing
@[test]
fn test_add() { assert_eq(add(2, 3), 5) }

@[test(should_fail)]
fn test_panic() { assert(false) }

// AI annotations (deferred, emit info not errors)
@[goal("maximize throughput")]
@[adaptive]
@[agent]
@[verify]
```

## Key Invariants

- All integers default to `i64`; `i32` exists for interop
- `str` is `{ i64 len, ptr data }` in LLVM IR, always null-terminated
- `Result<T,E>` canonical layout: `{ i1 tag, [max(sizeof T, sizeof E) x i8] }` — tag 0=Err, 1=Ok
- `Option<T>` layout: `{ i1 tag, T }` — tag 0=None, 1=Some
- Arrays/slices: `{ i64 len, ptr data }` with heap-allocated backing
- Lambdas lower to `__lambda_N` module-level functions (captures: Phase 3)
- `@[agent]`, `@[goal]` etc. emit I0001 info diagnostic, not errors

## Adding a New Builtin

1. `builtins.rs` — add entry to `BUILTINS` array
2. `codegen.rs` — declare LLVM function in `declare_builtins`, handle in `emit_call`
3. `infer.rs` — `builtin_sigs()` auto-populates; add `Type::` mapping to `fn_return_types`
4. `checker.rs` — usually automatic via `check_call_arity_and_types`
5. `examples/` — add usage example and test

## Phase Status

| Phase | Status | Features |
|-------|--------|---------|
| 1 | ✅ Complete | Functions, if/else, match, Result/Option, basic builtins, JIT+AOT |
| 2 | ✅ Complete | Structs, enums, slices, lambdas, while, `%`/`&&`/`||`, string interp, extended builtins, IR bug fixes |
| 3 | ✅ Complete | Generics, traits, closures w/ captures, channels, borrow checker, comptime, spans |
| 4 | ✅ Complete | LSP, formatter, doc gen, incremental compile, multi-file, cross-compile |
| ASI 1–3.6 | ✅ Merged | `Uncertain<T>`, `Temporal<T>`, `@[verify]`, `@[adaptive]`, `goal_run`, `ai_complete`, `ai_extract*`, `@[contained]` |
| 5 | 📋 Drafted | Refinement types + SMT (Z3) — see `spec/compiler-phase5.md` |
| 6 | 📋 Drafted | Row-polymorphic effects + handlers — see `spec/compiler-phase6.md` |
| 7+ | 📋 Roadmapped | `Principal`, `Store`, `Supervisor`, `LLM<Caps>`, goal/agent surface — see `ROADMAP.md` |
| 10 v0–v1 | ✅ Landed | Codegen-free tree-walking interpreter (`interp.rs`) → `axon run`/`test`/`goal`; prose→AST surface compiler (`axon goal`, lifts ` ```axon ` blocks); ASI builtins in the interpreter. Native codegen build diagnosed as serial LLVM-IR-gen (`BUILD_DIAGNOSIS.md`); fix prototyped (`CODEGEN_WRAPPER_PROTOTYPE.md`) |

## Forward Roadmap

Forward planning lives in `ROADMAP.md`. Highlights:
- Two-track architecture: typed `.ax` is an IR; user-facing surface is structured-prose forms compiled to AST
- Userland OS, not kernel replacement
- 14-type Tier-1 stdlib (`Goal`, `Constraint`, `Budget`, `Principal`, `Effect`, `Store`, etc.)
- TCB enumerated; self-modification cannot weaken it
- `examples/asi/` is the public face, not `examples/fibonacci.ax`

## ASI Demo Set

`examples/asi/` ships end-to-end ASI workflows on already-shipped primitives. Each demo is the
forcing function for upcoming phase work — friction encountered while building lands in
`ROADMAP.md` §9.5 as concrete spec items.

| Demo | Pattern |
|---|---|
| `optimize.ax` | ai_complete + parse + adaptive + verify + redteam |
| `classify.ax` | ai_extract_uncertain_i64; confidence in score |
| `summarize.ax` | composite metric (length × LLM-judged coverage) |
| `code_review.ax` | two cooperating @[adaptive] fns (proposer + critic) |

CLI surface simulated as `examples/asi/run.sh` (the eventual Phase-10 `axon goal …` shape).

## Design Reference

Full language design: `/home/cklaus/projects/BTask/packages/bcode/AI_Language_Plan.md`
