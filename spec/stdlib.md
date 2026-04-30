# Axon Standard Library Specification

**Version**: Phase 2  
**Applies to**: `axon-core` builtins as defined in `crates/axon-core/src/builtins.rs`

---

## 1. Philosophy

Axon's standard library is minimal and intrinsic — every builtin is compiler-known and injected
into the global scope before your program begins. There is no `use std::io`, no import to forget,
no module to install. If a function appears in this document it is available everywhere, in every
Axon file, without qualification. This keeps the language accessible for small scripts while
leaving the door open for an optional `std` module (planned for Phase 3+) that will carry
higher-level utilities such as file I/O, networking, and collections behind explicit imports.

---

## 2. Builtin Functions

### I/O

#### `print(msg: str) -> ()`
*Phase added: 1*

Write `msg` to standard output without a trailing newline. Use when you need to build output
incrementally on a single line.

```axon
print("loading")
print("...")
println("")   // newline when done
```

---

#### `println(msg: str) -> ()`
*Phase added: 1*

Write `msg` to standard output followed by a newline. The most common output function for
line-oriented output.

```axon
println("hello, world")
```

---

#### `eprint(msg: str) -> ()`
*Phase added: 1*

Write `msg` to standard error without a trailing newline. Use for diagnostic or progress output
that should not mix with program output.

```axon
eprint("warning: ")
```

---

#### `eprintln(msg: str) -> ()`
*Phase added: 1*

Write `msg` to standard error followed by a newline. The standard way to emit error messages and
warnings.

```axon
eprintln("error: file not found")
```

---

### Assertions

Assertion builtins are designed for use inside `@[test]` functions. They panic with a diagnostic
message on failure, which the test runner interprets as a test failure.

#### `assert(cond: bool) -> ()`
*Phase added: 1*

Panic if `cond` is `false`. Use for invariants that must hold regardless of inputs.

```axon
assert(result > 0)
```

---

#### `assert_eq(a: i64, b: i64) -> ()`
*Phase added: 2*

Panic with a message of the form `"assertion failed: {a} != {b}"` if the two `i64` values differ.
Prefer this over `assert(a == b)` because the failure message includes both values.

```axon
assert_eq(factorial(5), 120)
```

> **Phase 1 note**: In Phase 1 the type-checker had not yet run when builtins were seeded, so
> `assert_eq` accepted `str` operands as a placeholder. Phase 2 locks the signature to `i64, i64`.
> A generic version (`assert_eq<T>`) will arrive in Phase 3 once traits and generics are in place.

---

#### `assert_err(tag: bool) -> ()`
*Phase added: 2*

Panic if `tag` is `true` (indicating `Ok`). This builtin is intended for testing error paths:
pass the tag field extracted from a `Result` value, and the test fails if the result was
unexpectedly successful.

```axon
// In a test: expect safe_div to return Err when dividing by zero.
match safe_div(10, 0) {
    Ok(_)  => assert_err(true)   // forces failure
    Err(_) => assert_err(false)  // passes
}
```

> **Phase 2 limitation**: `assert_err` takes an explicit `bool` tag rather than a `Result<T,E>`
> directly. Phase 3 will replace this with a generic `assert_err<T, E>(r: Result<T,E>)` once
> trait bounds are available.

---

### String

#### `len(s: str) -> i64`
*Phase added: 2*

Return the number of bytes in `s`. Because Axon strings are UTF-8 byte sequences, this is the
byte length, not the character count. For ASCII strings the two are identical.

```axon
let n = len("hello")   // n == 5
```

---

#### `to_str_f64(n: f64) -> str`
*Phase added: 1*

Convert a floating-point number to its string representation using `"%.6g"` formatting (six
significant digits, trailing zeros removed, scientific notation for very large or very small
values).

```axon
let s = to_str_f64(3.14159)   // "3.14159"
let t = to_str_f64(1000000.0) // "1e+06"
```

---

#### `format(template: str) -> str`
*Phase added: 1*

Interpolate `template` and return the result as a new string. In practice you will rarely call
`format` directly — the compiler desugars string interpolation expressions (`"hello {name}"`) into
`format` calls automatically.

```axon
let name = "world"
let msg  = format("hello {name}")  // "hello world"
// Or equivalently, the shorthand:
let msg2 = "hello {name}"
```

---

#### `axon_concat(a: str, b: str) -> str`
*Phase added: 1*

Concatenate two strings and return the result. This is an internal builtin used by string
interpolation desugaring; you can call it directly but `"a" + "b"` or template strings are
usually more readable.

```axon
let s = axon_concat("foo", "bar")   // "foobar"
```

> **Note**: `axon_concat` is a low-level building block. Future phases will provide an `+`
> operator overload for `str` via the `Concat` trait.

---

### Conversion

#### `to_str(n: i64) -> str`
*Phase added: 1*

Convert an integer to its decimal string representation. The standard way to turn a number into
text for output or concatenation.

```axon
println(to_str(42))        // prints: 42
println("result: " + to_str(count))
```

---

#### `parse_int(s: str) -> Result<i64, str>`
*Phase added: 2*

Parse `s` as a base-10 integer. Returns `Ok(n)` on success or `Err("invalid integer")` if `s`
does not represent a valid decimal integer (empty string, non-digit characters, overflow).

```axon
match parse_int("123") {
    Ok(n)  => println(to_str(n))
    Err(e) => eprintln(e)
}
```

---

### Math

#### `abs_i32(n: i64) -> i32`
*Phase added: 1*

Return the absolute value truncated to a 32-bit integer. The parameter accepts `i64` so
integer literals can be passed without explicit type annotation.

```axon
let a = abs_i32(-7)   // 7
```

---

#### `abs_f64(n: f64) -> f64`
*Phase added: 1*

Return the absolute value of a 64-bit float.

```axon
let a = abs_f64(-2.5)  // 2.5
```

---

#### `min_i32(a: i64, b: i64) -> i32`
*Phase added: 1*

Return the lesser of two integers (result truncated to i32 range). Parameters accept `i64`
so integer literals can be passed without explicit type annotation.

```axon
let m = min_i32(3, 7)   // 3
```

---

#### `max_i32(a: i64, b: i64) -> i32`
*Phase added: 1*

Return the greater of two integers (result truncated to i32 range). Parameters accept `i64`
so integer literals can be passed without explicit type annotation.

```axon
let m = max_i32(3, 7)   // 7
```

---

## 3. Builtin Availability Table

| Name | Phase | Category | Notes |
|------|-------|----------|-------|
| `print` | 1 | I/O | No trailing newline |
| `println` | 1 | I/O | Adds trailing newline |
| `eprint` | 1 | I/O | Writes to stderr, no newline |
| `eprintln` | 1 | I/O | Writes to stderr with newline |
| `assert` | 1 | Assertions | Panics on false condition |
| `assert_eq` | 2 | Assertions | Compares two `i64` values; generic form in Phase 3 |
| `assert_err` | 2 | Assertions | Expects a Result error tag; generic form in Phase 3 |
| `len` | 2 | String | Byte length of a string |
| `to_str_f64` | 1 | String | `f64` → `str` with `%.6g` format |
| `format` | 1 | String | String interpolation; called implicitly by `"{expr}"` |
| `axon_concat` | 1 | String | Internal concatenation primitive |
| `to_str` | 1 | Conversion | `i64` → decimal `str` |
| `parse_int` | 2 | Conversion | `str` → `Result<i64, str>` |
| `abs_i32` | 1 | Math | Absolute value for `i32` |
| `abs_f64` | 1 | Math | Absolute value for `f64` |
| `min_i32` | 1 | Math | Minimum of two `i32` values |
| `max_i32` | 1 | Math | Maximum of two `i32` values |
| `read_line` | 4 | I/O | Read a line from stdin; blocks until newline |
| `read_file` | 4 | I/O | Read file contents → `Result<str, str>` |
| `write_file` | 4 | I/O | Write string to file → `Result<(), str>` |
| `sleep_ms` | 4 | Time | Sleep current thread for N milliseconds |
| `now_ms` | 4 | Time | Wall-clock time as ms since Unix epoch |
| `str_eq` | 5 | String | Content equality for two `str` values |
| `str_contains` | 5 | String | Check if string contains a substring |
| `str_starts_with` | 5 | String | Check if string begins with prefix |
| `str_ends_with` | 5 | String | Check if string ends with suffix |
| `str_slice` | 5 | String | Return substring `s[start..end]`; indices clamped to valid range |
| `str_index_of` | 5 | String | Byte index of first `needle` occurrence, or -1 |
| `char_at` | 5 | String | Byte value (0-255) at index `i`, or -1 if out of range |
| `to_str_bool` | 5 | Conversion | `bool` → `"true"` or `"false"` |
| `parse_float` | 5 | Conversion | `str` → `Result<f64, str>` |
| `abs_i64` | 5 | Math | Absolute value for `i64` (no truncation) |
| `min_i64` | 5 | Math | Minimum of two `i64` values |
| `max_i64` | 5 | Math | Maximum of two `i64` values |
| `str_to_upper` | 6 | String | ASCII uppercase copy of `s` |
| `str_to_lower` | 6 | String | ASCII lowercase copy of `s` |
| `str_trim` | 6 | String | Strip leading and trailing ASCII whitespace |
| `str_trim_start` | 6 | String | Strip leading ASCII whitespace |
| `str_trim_end` | 6 | String | Strip trailing ASCII whitespace |
| `str_replace` | 6 | String | Replace all occurrences of `from` with `to` |
| `str_repeat` | 6 | String | Concatenate `s` repeated `n` times |
| `env_var` | 6 | System | Read environment variable; `Ok(value)` or `Err("not set")` |
| `exit` | 6 | System | Terminate the process with the given exit code |
| `str_len` | 7 | String | Byte length of `s` |
| `str_pad_start` | 7 | String | Left-pad `s` to `width` bytes using first byte of `fill` |
| `str_pad_end` | 7 | String | Right-pad `s` to `width` bytes using first byte of `fill` |
| `min_f64` | 7 | Math | Minimum of two `f64` values |
| `max_f64` | 7 | Math | Maximum of two `f64` values |
| `clamp_i64` | 7 | Math | Clamp `i64` value to `[lo, hi]` |
| `clamp_f64` | 7 | Math | Clamp `f64` value to `[lo, hi]` |
| `parse_bool` | 7 | Conversion | Parse `"true"`/`"false"` → `Result<bool, str>` |
| `random_i64` | 7 | Random | Pseudo-random `i64` in `[lo, hi)` via C `rand()` |
| `random_f64` | 7 | Random | Pseudo-random `f64` in `[0.0, 1.0)` via C `rand()` |
| `for i in lo..hi` | 8 | Syntax | Integer range loop; `i` is `i64`, body repeats for `lo ≤ i < hi` |
| `i64_to_f64` | 9 | Conversion | Convert `i64` → `f64` (sitofp) |
| `f64_to_i64` | 9 | Conversion | Truncate `f64` → `i64` toward zero (fptosi) |
| `abs_i64` | 9 | Math | Absolute value of `i64` |
| `abs_f64` | 9 | Math | Absolute value of `f64` |
| `sign_i64` | 9 | Math | Sign of `i64`: −1, 0, or 1 |
| `pow_i64` | 9 | Math | `base^exp` for non-negative `exp`, iterative |
| `sqrt_f64` | 9 | Math | Square root via `libm sqrt` |
| `floor_f64` | 9 | Math | Floor via `libm floor` |
| `ceil_f64` | 9 | Math | Ceiling via `libm ceil` |
| `round_f64` | 9 | Math | Round-half-away-from-zero via `libm round` |

---

## 4. Type Coercions at Call Sites

Axon does not have implicit coercions between unrelated types in general, but the code generator
applies a small set of automatic width adjustments at function call sites when the argument type
and the declared parameter type differ. These coercions happen silently — the type checker does
not see them; they are purely a codegen convenience.

### Integer width coercion

When an integer argument's bit width does not match the declared parameter's bit width:

- **Narrowing (wider → narrower)**: the value is truncated. Example: passing an `i64` where an
  `i32` is expected discards the high 32 bits. This can silently lose information; a future
  warning pass will flag it.
- **Widening (narrower → wider)**: the value is sign-extended (`sext`). Example: passing an `i32`
  where an `i64` is expected produces a 64-bit value with the sign of the original `i32`.

```axon
// abs_i32 takes i64 and returns i32; literal integers are i64 by default.
let x = abs_i32(-5)
```

### Float-to-integer coercion

When an `f64` argument is passed to a parameter declared as an integer type, the compiler emits
`fptosi` (float-to-signed-integer truncation). The fractional part is discarded; no rounding.

```axon
// Hypothetical: to_str takes i64 but you pass f64 — codegen truncates.
// Avoid relying on this; use explicit conversion instead.
```

### Integer-to-float coercion

When an integer argument is passed to a parameter declared as `f64`, the compiler emits `sitofp`
(signed-integer-to-float). This is lossless for integers that fit in the 53-bit mantissa of an
`f64` (all `i32` values and most `i64` values in practice).

```axon
// abs_f64 takes f64; passing an integer literal triggers sitofp.
let a = abs_f64(5)   // 5.0
```

### No coercions between unrelated types

There is no implicit coercion between `str` and numeric types, between `bool` and numeric types,
or between struct types. Such conversions require an explicit call to `to_str`, `parse_int`, or
a user-defined conversion function.

---

## 5. Known Issues and Phase 2 Fixes

The following bugs existed in the Phase 1 stdlib implementation and are being corrected in Phase 2.
This section documents the bug and the fix so that runtime and codegen changes can be audited.

### `eprint` / `eprintln` wrote to stdout (bug, being fixed in Phase 2)

**Bug**: The Phase 1 `eprint` and `eprintln` implementations called `printf("%s", msg.data)` and
`puts(msg.data)` respectively — the same as `print` and `println`. Both wrote to file descriptor 1
(stdout) instead of file descriptor 2 (stderr).

**Fix (Phase 2)**: `eprint` now calls `fprintf(stderr, "%s", msg.data)` and `eprintln` calls
`fprintf(stderr, "%s\n", msg.data)`. The C symbol `stderr` is resolved from libc at link time;
no new Cargo dependency is required.

**Impact**: Any Phase 1 program that relied on `eprint`/`eprintln` output appearing on stdout will
behave differently after Phase 2. This is an intentional breaking fix; the old behaviour was wrong.

---

### `to_str` / `to_str_f64` used a static buffer (not thread-safe, being fixed in Phase 2)

**Bug**: Both functions wrote their result into a module-level 32-byte static buffer
(`to_str_buf`, `to_str_f64_buf`) and returned a `str` struct whose `data` pointer pointed into
that buffer. Consecutive calls overwrote the buffer, making the previously returned `str` invalid.
The functions were not thread-safe and could not be called concurrently.

**Fix (Phase 2)**: Both functions now allocate a fresh `malloc` buffer on each call:

```c
char *buf = (char*)malloc(32);
snprintf(buf, 32, "%lld", n);   // to_str
// OR
snprintf(buf, 32, "%.6g", n);   // to_str_f64
```

The returned `str.data` pointer now points into a heap-allocated buffer. The caller is responsible
for freeing it (same ownership model as `axon_concat`). The functions are now re-entrant and safe
to call from multiple threads.

**Impact on callers**: callers that held a `str` returned by `to_str` across a second call to
`to_str` will no longer see their value silently overwritten. However, they now hold a
`malloc`-allocated buffer that is never freed in Phase 2 (no GC). Long-running programs that call
`to_str` frequently will grow heap usage; this is addressed in Phase 3 string ownership work.

---

### `format` is listed as a builtin but is not a runtime function

**Bug / design clarification**: `format` appears in the Phase 1 builtin table and stdlib
documentation as a callable function, but it is not implemented as a runtime function in the
generated LLVM IR. String interpolation (`"hello {name}"`) is desugared by the compiler into
`FmtStr` AST nodes, which the code generator expands into `to_str` calls and `axon_concat` chains
directly — there is no `@format` function emitted.

A user who writes `format("hello {name}")` does not call a runtime function; the compiler
transforms the string literal argument into the same `FmtStr` expansion it would use for
`"hello {name}"` written directly. The `format` identifier in the source is therefore misleading:
it implies a function call but behaves like a syntactic shorthand.

**Recommended usage**: use string interpolation syntax `"hello {name}"` directly. Do not rely on
`format(...)` being callable as a function at runtime; this behaviour is not guaranteed.

**Phase 3 resolution**: `format` will either be removed from the builtin table entirely (users use
interpolation syntax) or reimplemented as a proper runtime function that accepts a format string
and a variable argument list. The decision will be made during Phase 3 stdlib planning. Until
then, calling `format(...)` explicitly (rather than using `"..."` interpolation syntax) is
unsupported and may produce unexpected results or a compiler error.

---

### `abs_i32`, `abs_f64`, `min_i32`, `max_i32` not implemented in codegen until Phase 2 fix

**Bug**: These four math builtins were declared in the `BUILTINS` constant in `builtins.rs`
(making them visible to the type checker and name resolver) but their LLVM IR function bodies were
not emitted by `declare_builtins` in Phase 1. Calls to these functions would compile without a
type error but fail at link time with an undefined symbol error.

**Fix (Phase 2)**: `declare_builtins` now emits LLVM IR bodies for all four functions using
`intrinsic`-style IR:

- `abs_i32` / `abs_f64`: emit the standard `if n < 0 { -n } else { n }` IR pattern.
- `min_i32` / `max_i32`: emit a comparison + select IR pattern.

These are not calls to libm; they are self-contained IR sequences with no external dependencies.

---

## 6. Planned Phase 4 Additions

The following builtins are planned for Phase 4. They are not present in Phase 1, 2, or 3.

### Floating-point extended math

```axon
sqrt(n: f64) -> f64    // square root; calls C sqrt from libm
pow(b: f64, e: f64) -> f64  // exponentiation; calls C pow from libm
floor(n: f64) -> f64   // round toward negative infinity; calls C floor
ceil(n: f64) -> f64    // round toward positive infinity; calls C ceil
```

These are promoted from "Phase 3 planned" (see §5 below) to confirmed Phase 4 additions after
being specified in `spec/compiler-phase3.md` new-builtins list. If Phase 3 lands them early,
Phase 4 treats them as already shipped.

---

### `assert_eq_str` and generic `assert_eq`

Pending resolution of the `assert_eq` generics decision (see `spec/compiler-phase3.md` known
gaps):

**If Phase 3 ships type-specific variants**:
```axon
assert_eq_str(a: str, b: str) -> ()
    // panics with diff-style message if a != b
assert_eq_f64(a: f64, b: f64) -> ()
    // panics if a != b (exact float equality)
```

**If Phase 3 ships generics + Eq trait**:
```axon
assert_eq<T: Eq>(a: T, b: T) -> ()
    // replaces the i64-only Phase 2 version
```

Phase 4 will have at least one of the above. Both type-specific and generic forms will exist in
Phase 4 regardless of which path Phase 3 took, for backward compatibility.

---

### I/O builtins

```axon
read_line() -> str
    // Read one line from stdin (up to and not including the newline).
    // Blocks until a newline is received. Returns an empty string on EOF.

read_file(path: str) -> Result<str, str>
    // Read the entire contents of the file at `path` as a UTF-8 string.
    // Returns Ok(contents) or Err(error_message).

write_file(path: str, content: str) -> Result<(), str>
    // Write `content` to the file at `path`, creating or truncating it.
    // Returns Ok(()) on success or Err(error_message).
```

These are low-level intrinsics backed by POSIX `read`/`write` system calls (or Win32 equivalents
on Windows). Higher-level file I/O will be provided by `std::io` once the module system is
operational.

---

### Time builtins

```axon
sleep_ms(ms: i64) -> ()
    // Suspend the current thread for at least `ms` milliseconds.
    // Backed by POSIX nanosleep (or Win32 Sleep).

now_ms() -> i64
    // Return the current wall-clock time as milliseconds since the Unix epoch.
    // Backed by clock_gettime(CLOCK_REALTIME) or equivalent.
```

`now_ms` is not comptime-evaluable (it reads the system clock at runtime).

---

## 7. Planned Phase 3 Additions

Phase 3 brings generics, traits, channels, and comptime evaluation. The stdlib grows accordingly.

### Floating-point math

```axon
sqrt(n: f64) -> f64    // square root via C sqrt; not comptime-evaluable
pow(base: f64, exp: f64) -> f64   // exponentiation via C pow
floor(n: f64) -> f64              // round toward negative infinity
ceil(n: f64) -> f64               // round toward positive infinity
```

### Generic assertions

```axon
assert_eq_str(a: str, b: str) -> ()
// Panics with a diff-style message if a != b. The Phase 3 string-specialised version
// of assert_eq, bridging until full generic assert_eq<T: Eq> is available.
```

### Channel operations

Once `chan<T>` is a first-class type, the following become builtin:

```axon
chan_clone(ch: chan<T>) -> chan<T>
// Increment the channel's reference count and return a second handle to the same channel.
// Backed by __axon_chan_clone in the Axon runtime library.
```

The send/recv operations (`ch.send(v)`, `ch.recv()`) are method syntax on `chan<T>` rather than
free functions; they are handled by the compiler directly rather than appearing in the BUILTINS
table.

### Trait-based display

Phase 3 introduces the `Displayable` trait. Once it lands, `println` and friends will accept
`dyn Displayable` in addition to `str`, removing the need to call `to_str` / `to_str_f64`
explicitly in most output code:

```axon
trait Displayable {
    fn display(self) -> str
}
// Built-in impl for i64, f64, bool provided by the compiler.
```

### `comptime` functions

Several pure mathematical functions become comptime-evaluable in Phase 3, meaning the compiler
can fold them at compile time when called inside a `comptime { }` block:

```axon
let AREA = comptime { pow2(8) }   // 64 — folded at compile time
```

Any user-defined `fn` that has no side effects (no I/O, no channel operations) is also eligible
for comptime evaluation when all its arguments are comptime-evaluable.

---

## 8. Naming Conventions

Axon builtins follow a consistent naming scheme. New builtins and user-defined conversion helpers
should follow the same pattern.

### Conversion functions

The general form is `<operation>_<source_type>` for type-specific variants and `<operation>` when
the type is unambiguous from context.

| Pattern | Example | Meaning |
|---------|---------|---------|
| `to_str` | `to_str(n: i64)` | Primary `to_str`: converts the most common numeric type (`i64`) to `str` |
| `to_str_<type>` | `to_str_f64(n: f64)` | Type-specific variant when the primary is already claimed |
| `parse_<type>` | `parse_int(s: str)` | Parse a `str` into the named type; always returns `Result` |

**Rationale**: `to_str` without a suffix claims the `i64 → str` slot because `i64` is Axon's
default integer type. Variants for other types append the source type as a suffix (`_f64`, `_i32`,
`_bool`). Parse functions always return `Result<T, str>` to force callers to handle malformed
input.

### Math functions

Type-specific math builtins append the type suffix:

| Pattern | Example | Meaning |
|---------|---------|---------|
| `<op>_<type>` | `abs_i32`, `abs_f64` | Type-specific unary operation |
| `<op>_<type>` | `min_i32`, `max_i32` | Type-specific binary operation |

Once generics land in Phase 3, the type suffix will be dropped from generic versions:
`abs<T: Numeric>(n: T) -> T`. The concrete `_i32` / `_f64` variants will remain as aliases for
backward compatibility but may be deprecated in Phase 4.

### New builtins checklist

When adding a new builtin to `crates/axon-core/src/builtins.rs`:

1. Choose a name that follows the patterns above.
2. Add an entry to the `BUILTINS` constant with a non-empty `doc` string (enforced by test).
3. Add it to the availability table in this document under the correct category.
4. Note which phase introduced it.
5. If the function has a type-specific name now but will gain a generic form later, document that
   in the entry's phase note.
