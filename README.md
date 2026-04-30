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

```bash
# Build
cargo build -p axon-core

# Run a file
./target/debug/axon run hello.ax

# Type-check only
./target/debug/axon check hello.ax

# Format
./target/debug/axon fmt hello.ax

# Run tests (@[test] annotated functions)
./target/debug/axon test hello.ax

# Start LSP server (reads JSON-RPC from stdin)
./target/debug/axon lsp
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
    let ch = chan<i64>(16)
    spawn { producer(ch.clone()) }
    let sum = 0
    let i = 0
    while i < 10 {
        sum = sum + ch.recv()
        i = i + 1
    }
    sum
}
```

### Comptime

```axon
let MAX: i64 = comptime { 1024 * 1024 }

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
**String**: `str_len`, `str_eq`, `str_contains`, `str_starts_with`, `str_ends_with`, `str_slice`, `str_index_of`, `char_at`, `str_to_upper`, `str_to_lower`, `str_trim`, `str_replace`, `str_repeat`, `str_pad_start`, `str_pad_end`  
**Math**: `abs_i64`, `min_i64`, `max_i64`, `clamp_i64`, `pow_i64`, `abs_f64`, `min_f64`, `max_f64`, `sqrt`, `pow`, `floor`, `ceil`, `round_f64`, `random_i64`, `random_f64`  
**Conversion**: `to_str`, `to_str_f64`, `to_str_bool`, `parse_int`, `parse_float`, `parse_bool`, `i64_to_f64`, `f64_to_i64`  
**System**: `env_var`, `exit`, `sleep_ms`, `now_ms`  
**Assert**: `assert_eq`, `assert_eq_str`, `assert_eq_f64`

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

Phase 4 complete — all compiler stages implemented and tested. See [STATUS.md](STATUS.md) for
the full feature matrix.

**Test suite**: 246 tests (189 unit + 57 integration), all passing.

## License

MIT
