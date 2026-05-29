# Axon

A statically-typed, expression-oriented systems language that compiles to native code via LLVM 17.

```axon
fn fibonacci(n: i64) -> i64 {
    if n <= 1 { n }
    else { fibonacci(n - 1) + fibonacci(n - 2) }
}

fn main() -> i64 {
    let result = fibonacci(10)
    assert_eq(result, 55)
    0
}
```

## Features

- **HM type inference** — Hindley-Milner with generics, traits, and type classes
- **Algebraic types** — structs, enums with payloads, pattern matching with guards
- **Ownership semantics** — lite borrow checker for `own`/`ref` bindings
- **Comptime evaluation** — `comptime` expressions evaluated at compile time
- **Closures** — first-class, heap-captured mutable closures
- **Concurrency** — typed channels (`chan<T>`), `spawn`, `select`
- **Result/Option** — `?` propagation, `Some`/`None`/`Ok`/`Err` constructors
- **Traits** — vtable dispatch, trait bounds, `impl Trait for Type`
- **Generics** — monomorphization, generic structs and functions
- **LSP server** — hover, go-to-definition, diagnostics (JSON-RPC 2.0)
- **Formatter** — `axon fmt` idempotent pretty-printer
- **Doc generator** — `axon doc` extracts `///` comments to Markdown
- **Incremental cache** — SHA-256 keyed `.axc` bitcode cache
- **Cross-compilation** — `--target <triple>` via `cross.toml` linker config

## Pipeline

```
Lexer → Parser → Resolver → fill_captures → Infer (HM) → Checker → Borrow → Mono → Codegen → LLVM → binary
```

## Quick Start

Axon runs via a **codegen-free tree-walking interpreter** — the `axon` CLI
builds in seconds without LLVM. (The native LLVM/codegen build is currently
slow / CI-only; see `BUILD_DIAGNOSIS.md`.)

```bash
# Build the interpreter CLI (fast, no LLVM):
cargo build -p axon-core --no-default-features --bin axon

# Run / type-check / test a file:
./target/debug/axon run   hello.ax
./target/debug/axon check hello.ax
./target/debug/axon test  hello.ax

# Compile a structured-prose goal file → AST → run it (Phase-10 two-track flow):
./target/debug/axon goal examples/goals/optimize-goal.md

# LLM-backed goals/demos run key-free with deterministic stubs:
AXON_AI_MOCK=1 ./target/debug/axon goal examples/goals/hello-goal.md

# `axon build` (native binary) and `axon parse --json` / `axon lsp` need extra
# features: --features codegen and --features serde-json respectively.
```

## Language Tour

### Functions and types

```axon
fn add(a: i64, b: i64) -> i64 { a + b }

fn greet(name: str) -> str {
    "Hello, {name}!"
}
```

### Structs and enums

```axon
type Point = { x: f64, y: f64 }

enum Shape {
    Circle { radius: f64 },
    Rect   { width: f64, height: f64 },
}

fn area(s: Shape) -> f64 {
    match s {
        Shape::Circle { radius } => pow(radius, 2.0) * 3.14159,
        Shape::Rect { width, height } => width * height,
    }
}
```

### Error handling

```axon
fn parse_positive(s: str) -> Result<i64, str> {
    let n = parse_int(s)?
    if n > 0 { Ok(n) }
    else { Err("must be positive") }
}
```

### Closures and higher-order functions

```axon
fn make_counter() -> () -> i64 {
    let n = 0
    () => { n = n + 1; n }
}

fn apply_twice(f: (i64) -> i64, x: i64) -> i64 {
    f(f(x))
}
```

### Traits

```axon
trait Printable {
    fn display(self: Self) -> str
}

type Vec2 = { x: f64, y: f64 }

impl Printable for Vec2 {
    fn display(self: Vec2) -> str {
        "({self.x}, {self.y})"
    }
}
```

### Generics

```axon
type Pair<A, B> = { first: A, second: B }

fn swap<A, B>(p: Pair<A, B>) -> Pair<B, A> {
    Pair { first: p.second, second: p.first }
}
```

### Concurrency

```axon
fn producer(ch: chan<i64>) -> () {
    let i = 0
    while i < 10 {
        ch.send(i)
        i = i + 1
    }
}

fn main() -> i64 {
    let ch = chan<i64>()
    spawn { producer(ch.clone()) }
    let sum = 0
    let i = 0
    while i < 10 {
        sum = sum + ch.recv()
        i = i + 1
    }
    sum            // 0 + 1 + … + 9 = 45
}
```

> The interpreter runs concurrency **cooperatively** (single-threaded): a `spawn`
> body runs eagerly, so the producer above fully queues its values before `main`
> drains them — fan-out/collect works. Patterns where a spawned task must *block*
> on a value `main` sends later need the native runtime. `select` takes the first
> ready channel.

### Comptime

```axon
fn buffer_size() -> i64 {
    comptime { 1024 * 1024 } // folded once; here the interpreter evaluates it inline
}

fn is_debug() -> bool {
    comptime { false }
}
```

## Testing

```axon
@[test]
fn test_add() {
    assert_eq(add(2, 3), 5)
    assert_eq(add(-1, 1), 0)
}
```

Run with `axon test file.ax` or `axon test --jobs 0` for parallel execution.

## Builtins

**I/O**: `print`, `println`, `eprint`, `eprintln`, `read_line`, `read_file`, `write_file`  
**String / collections**: `len` (str *and* arrays), `str_len`, `str_eq`, `str_contains`, `str_starts_with`, `str_ends_with`, `str_slice`, `str_index_of`, `char_at`, `str_to_upper`, `str_to_lower`, `str_trim`, `str_replace`, `str_repeat`, `str_pad_start`, `str_pad_end`, `i64_to_str_radix`  
**Math**: `abs_i64`, `min_i64`, `max_i64`, `clamp_i64`, `pow_i64`, `abs_f64`, `min_f64`, `max_f64`, `sqrt`, `pow`, `floor`, `ceil`, `round_f64`, `random_i64`, `random_f64`  
**Conversion**: `to_str`, `to_str_f64`, `to_str_bool`, `parse_int`, `parse_float`, `parse_bool`, `i64_to_f64`, `f64_to_i64`  
**System**: `env_var`, `exit`, `sleep_ms`, `now_ms`  
**Assert**: `assert`, `assert_eq`, `assert_eq_str`, `assert_eq_f64`, `assert_err`  
**ASI**: `ai_complete`, `ai_extract_uncertain_i64`, `ai_extract_uncertain_f64`, `goal_run`, `uncertain_new`, `uncertain_new_f64`, `uncertain_confidence`, `temporal_new`, `temporal_at`, `temporal_now` — the LLM/goal/uncertainty/temporal primitives (live with `--features asi-runtime` + `ANTHROPIC_API_KEY`, or deterministic stubs via `AXON_AI_MOCK=1`)

## Project Structure

```
crates/
  axon-core/          # Compiler, LSP, formatter, doc generator
    src/
      lexer.rs        # Logos-based tokenizer
      parser.rs       # Recursive descent parser
      resolver.rs     # Name resolution
      infer.rs        # HM type inference
      checker.rs      # Type/arity/ownership error checker
      borrow.rs       # Lite borrow checker
      comptime.rs     # Comptime evaluator
      mono.rs         # Generic monomorphization
      codegen.rs      # LLVM IR emission via inkwell
      lsp.rs          # JSON-RPC 2.0 LSP server
      fmt.rs          # AST formatter
      doc.rs          # Doc comment extractor
      cache.rs        # Incremental .axc cache
    tests/
      integration_fixtures.rs   # 69 integration tests
      fixtures/                 # .ax source fixtures
  axon-rt/            # Runtime staticlib (channels, spawn, I/O, math)
spec/                 # Language specification
examples/             # Sample programs
```

## Status

Phases 1–4 complete (functions/structs/enums/generics/traits/closures/LSP). The
current execution path is a codegen-free **interpreter** (`interp.rs`, full
builtin coverage); the native LLVM build is diagnosed + structurally fixed but
slow (`BUILD_DIAGNOSIS.md`). Phase-10 **`axon goal`** compiles structured-prose
goals to runnable AST, with a goal-safety stdlib (`examples/stdlib/`: Budget,
Constraint, Principal, Uncertain + composition) and key-free demos
(`examples/goals/`). See [SESSION_STATUS.md](SESSION_STATUS.md) for current
state and [STATUS.md](STATUS.md) for the Phase-4 feature matrix.

**Test suite**: 246 tests (189 unit + 57 integration), all passing.

## License

MIT
