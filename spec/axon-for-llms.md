# Axon for LLMs

A single-file, task-oriented guide for an LLM writing Axon (`.ax`) code. Read this
first; reach for `spec/language-tour.md` (full tour), `spec/stdlib.md` (every builtin),
and `spec/grammar.ebnf` (exact syntax) only when you need more depth.

Axon is an AI-optimized, statically-typed systems language: no null, no exceptions,
Hindley-Milner inference, structural typing, capability sandboxing, and first-class
AI/agent annotations. It runs **interpreter-first** — you almost never need codegen.

---

## 1. Running code (do this, not `build`)

```bash
# Build the interpreter CLI once (sub-second, no LLVM):
cargo build -p axon-core --no-default-features --bin axon

axon run   file.ax     # type-check + interpret (tree-walking) — the default way to run
axon check file.ax     # type-check only
axon test  file.ax     # run @[test] functions
axon goal  goal.md     # compile a prose goal .md → .ax → run
```

`axon build file.ax` produces a native binary via LLVM (~3s), but for writing and
testing Axon you want `axon run` / `axon test`. Execution is identical between the
interpreter and native codegen by design.

Every program starts at `fn main()`. No imports — all builtins are in global scope.

---

## 2. Syntax in one screen

```axon
// Variables — mutable by default, no `let` on reassignment
let x = 42            // i64 (default integer)
let y = 3.14          // f64
let s = "hello"       // str (UTF-8, immutable)
let b = true          // bool
x = x + 1             // reassign — no `let`

// Functions — last expression is the return value (no `return` needed)
fn add(a: i64, b: i64) -> i64 { a + b }

// if / while / match are EXPRESSIONS
let label = if x > 0 { "pos" } else { "non-pos" }
while x < 10 { x = x + 1 }
for i in 0..n { /* i is i64, lo ≤ i < hi */ }

// Structs — declared with `type`, structural (no `implements`)
type Point = { x: f64, y: f64 }
let p = Point { x: 1.0, y: 2.0 }
println(to_str_f64(p.x))

// Enums (ADTs) — variants reference with `::`
type Shape = Circle { r: f64 } | Square { side: f64 }
let sh = Shape::Circle { r: 5.0 }

// Pattern matching — EXHAUSTIVE, `_` is the catch-all
let area = match sh {
    Shape::Circle { r }   => 3.14159 * r * r
    Shape::Square { side } => side * side
}

// String interpolation — `{expr}` evaluated at runtime; `{{`/`}}` for literal braces
println("hello {s}, x is {to_str(x)}")

// Lambdas
let double = |n| n * 2

// Operators:  + - * / %   == != < > <= >=   && ||
//   `/` and `%` on i64 are integer division/modulo.
//   `+` concatenates str ("a" + to_str(n)). `==` works on str.
```

---

## 3. No null, no exceptions — `Option` and `Result` everywhere

This is the core idiom. A function that can be absent returns `Option<T>`; one that can
fail returns `Result<T, E>`. There is no `null` and no `throw`.

```axon
fn find(xs: &[i64], target: i64) -> Option<i64> {
    for i in 0..len(xs) {
        if xs[i] == target { return Some(i) }
    }
    None
}

fn divide(a: i64, b: i64) -> Result<i64, str> {
    if b == 0 { Err("division by zero") } else { Ok(a / b) }
}

fn main() {
    match divide(10, 2) {
        Ok(n)  => println(to_str(n))
        Err(e) => eprintln(e)
    }
}
```

Use `?` to propagate an error early (only inside a fn returning `Result`):

```axon
fn parse_and_double(s: str) -> Result<i64, str> {
    let n = parse_int(s)?     // returns Err(...) early on failure
    Ok(n * 2)
}
```

---

## 4. Builtins you'll actually use

No imports — all available everywhere. Full list in `spec/stdlib.md`.

| Category | Builtins |
|---|---|
| I/O | `print` `println` `eprint` `eprintln` `read_line` `read_file` `write_file` |
| Convert | `to_str` (i64/f64/bool — polymorphic) `to_str_f64` `to_str_bool` `parse_int` `parse_float` `parse_bool` `i64_to_f64` `f64_to_i64` |
| String | `len` `str_eq` `str_contains` `str_starts_with` `str_ends_with` `str_slice` `str_index_of` `char_at` `str_to_upper` `str_to_lower` `str_trim` `str_replace` `str_repeat` `str_pad_start` `str_pad_end` |
| Math | `abs_i64` `abs_f64` `min_i64` `max_i64` `min_f64` `max_f64` `clamp_i64` `clamp_f64` `pow_i64` `sqrt_f64` `floor_f64` `ceil_f64` `round_f64` `sign_i64` |
| Time/Sys | `now_ms` `sleep_ms` `env_var` `exit` |
| Random | `random_i64(lo, hi)` `random_f64()` — seed with `AXON_SEED` for reproducibility |
| Test | `assert` `assert_eq` `assert_err` |

`to_str` covers i64, f64, and bool now; the `to_str_f64`/`to_str_bool` forms still exist.

---

## 5. Testing

```axon
fn add(a: i64, b: i64) -> i64 { a + b }

@[test]
fn test_add() {
    assert_eq(add(2, 3), 5)
    assert(add(1, 1) == 2)
}

@[test(should_fail)]
fn test_must_panic() { assert(1 == 2) }   // passes only if the body panics
```

Run with `axon test file.ax`. `assert_eq` is `i64`-only and prints both values on failure.

---

## 6. Gotchas (the things that bite an LLM)

1. **Pass collections by borrow: `&[T]`.** Declare `fn f(xs: &[i64])` and call `f(&arr)`.
   A bare `[T]` param or a missing `&` at the call site is error `E0601` — the fix is to
   add `&`. (`ref` is a binding mode, not a parameter mode.)

2. **Keep an expression on one line, or end the line with the operator.** A binary
   operator that *leads* the next line breaks parsing. Do this:
   ```axon
   let z = a +
           b          // OK — operator trails
   ```
   not `let z = a` then a new line starting with `+ b`.

3. **`match` must be exhaustive.** Cover every variant or add `_ =>`.

4. **Integer vs float is explicit.** `/` on `i64` is integer division. Use
   `i64_to_f64` / `f64_to_i64` to convert; there's no silent numeric coercion between
   unrelated types (str↔num, bool↔num all require explicit conversion).

5. **No `return` needed** for the tail expression; use `return` only for early exit.

6. **Strings are immutable and UTF-8.** `len` is the *byte* length. Compare with `==` or
   `str_eq`.

7. **Default integer is `i64`.** `i32`/unsigned exist mainly for interop; reach for `i64`.

---

## 7. AI / capability features (Axon's reason to exist)

These are why an LLM would target Axon specifically. Annotations attach to functions.

```axon
// Goal-directed optimization: the runtime searches @[adaptive] params to maximize @[goal].
@[goal("maximize score")]
@[adaptive]
fn tune(threshold: i64) -> i64 { /* return a score */ threshold * 2 }

// Runtime-checked postcondition on the scalar return value.
@[verify(value >= 0)]
fn clamp_nonneg(x: i64) -> i64 { max_i64(x, 0) }
```

**Capability sandboxing** — the compiler refuses any I/O a function didn't declare:

```axon
@[contained(fs: [write("./out/")], net: ["api.example.com"], exec: none)]
fn worker() -> i64 { /* may only write under ./out/ and reach that host */ 0 }
```
- `fs`: allowlist of path prefixes — `read("…")` / `write("…")`.
- `net`: allowlist of hosts (leading `*` glob).
- `exec`: `none` | `any` (process spawning).
- `never: [...]`: hard deny, overrides the allowlist.
- **`env_var(...)` is denied inside `@[contained]`** (error `E1001`) — env is an
  ungrantable ambient secret channel. Read it *outside* the boundary and pass the value in.

**Refinement types** constrain values; violations exit with code 6 at runtime:
```axon
fn safe_div(a: i64, b: i64 where b != 0) -> i64 { a / b }
```

**Effect rows** annotate what a function does, e.g. `fn fetch() -> str | {Net}`. Effects
can't be laundered through an un-annotated helper (checker enforces transitively).

For end-to-end AI workflows (LLM call → parse → adaptive tune → verify → redteam) study
`examples/asi/` — that directory is the language's intended public face.

---

## 8. A complete idiomatic program

```axon
type Stats = { count: i64, total: i64 }

fn summarize(xs: &[i64]) -> Stats {
    let total = 0
    for i in 0..len(xs) { total = total + xs[i] }
    Stats { count: len(xs), total: total }
}

fn mean(s: Stats) -> Result<i64, str> {
    if s.count == 0 { Err("empty") } else { Ok(s.total / s.count) }
}

fn main() {
    let data = [10, 20, 30, 40]
    let s = summarize(&data)
    match mean(s) {
        Ok(m)  => println("mean over {to_str(s.count)} items is {to_str(m)}")
        Err(e) => eprintln("error: {e}")
    }
}

@[test]
fn test_mean() {
    assert_eq(summarize(&[1, 2, 3]).total, 6)
}
```

Run it: `axon run file.ax` — then `axon test file.ax` for the test.
