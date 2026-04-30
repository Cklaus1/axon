# Axon Compiler — Phase 4 Status

**Last updated**: 2026-04-28  
**Phase**: 4 (active implementation)  
**Spec**: `spec/compiler-phase4.md`  
**Previous spec**: `spec/compiler-phase3.md`  
**Runtime spec**: `spec/runtime.md`

---

## What Is Axon

Axon is an AI-first systems programming language. It compiles to native code via LLVM 17 using the
`inkwell` crate. The pipeline is:

```
Lexer → Parser → Resolver → fill_captures → Infer (HM) → Checker → Borrow → [Mono] → Codegen → LLVM → binary
```

The main crate is `crates/axon-core`. The runtime (channels, spawn) is `crates/axon-rt` (a Rust
staticlib). There is no separate C runtime — all ABI symbols (`__axon_chan_new`, `__axon_spawn`,
etc.) are implemented in Rust with `#[no_mangle] extern "C"`.

---

## Phase 3 Feature Status

| Feature | Status | Notes |
|---------|--------|-------|
| Generics | ✅ Complete | Parse, infer (TypeParam unification + generic structs), mono (monomorphization), codegen |
| Traits / vtable dispatch | ✅ Complete | TraitDef, ImplBlock, vtable constants, fat-pointer call emission |
| Closures with captures | ✅ Complete | Heap-env struct; env field pointers used directly (supports mutable closures); `(params) => body` arrow lambda syntax |
| `chan<T>()` syntax | ✅ Complete | New `Token::Chan`; `parse_chan_new()`; infer `chan::<T>` special-case |
| `Chan::new(n)` syntax | ✅ Complete | Original syntax still works |
| `ch.send(v)` / `ch.recv()` | ✅ Complete | Codegen via `__axon_chan_send` / `__axon_chan_recv` |
| `ch.clone()` | ✅ Complete | Infer returns `Chan<T>`; codegen calls `__axon_chan_clone` |
| `spawn { body }` | ✅ Complete | Lifts body to LLVM function; calls `__axon_spawn(fn_ptr, env_ptr)` |
| `select { arm+ }` | ✅ Complete | Polls channels; `__axon_select` returns index of ready channel |
| Borrow checker (lite) | ✅ Complete | `borrow.rs`: UseAfterMove, MoveBorrowed, BorrowConflict |
| `comptime` expressions | ✅ Complete | `comptime.rs`: tree-walking evaluator; module-level and inline |
| Span-threaded diagnostics | ✅ Complete | `Span` on InferError/CheckError/Diagnostic/BorrowError |
| `check_pipeline()` public API | ✅ Complete | Returns `Vec<PipelineDiagnostic>` with file:line:col |
| E0401 (field not found) | ✅ Complete | Canonical code; replaces E0309 for field access errors |
| Math builtins (Phase 3) | ✅ Complete | `sqrt`, `pow`, `floor`, `ceil` via LLVM intrinsics |
| `assert_eq_str`, `assert_eq_f64` | ✅ Complete | Option B from spec §Known Gaps |
| Runtime `__axon_chan_clone` | ✅ Complete | Added to `axon-rt/src/lib.rs` |

---

## Key Architectural Decisions

### 1. `chan<T>()` encoding in the AST

Rather than adding a new AST node for channel creation, `chan<i64>()` is parsed as:
```rust
Expr::Call {
    callee: Expr::StructLit { name: "chan::<i64>", fields: [] },
    args: [Literal::Int(16)],
}
```

The element type is encoded in the synthetic callee name. Both infer and codegen detect the
`"chan::<T>"` prefix and handle it specially.

### 2. Mutable closure captures

Captured variables are bound directly to **env struct field pointers** (not copied into local
allocas). This means `n = n + 1` inside a closure persists across calls — required for the
`make_counter` pattern. Previous code copied values into local allocas which broke mutable
closures.

Both lambda syntaxes are supported:
- `|params| body` — pipe syntax (original)
- `(params) => body` — arrow syntax (added Phase 3; spec-canonical form)

The parser uses lookahead (`is_paren_lambda`) to disambiguate `(params) => body` from a
parenthesized expression `(expr) * something`.

### 3. assert_eq generics (Option B)

The spec §Known Gaps offered two options. We chose **Option B**: explicit typed variants
`assert_eq_str(a: str, b: str)` and `assert_eq_f64(a: f64, b: f64)`, rather than a generic
`assert_eq<T: Eq>`. The existing `assert_eq(a: i64, b: i64)` handles the most common case.
Full generic `assert_eq<T: Eq>` deferred to Phase 4 when trait-bounded generics are stable.

### 4. Borrow checker type inference

The borrow checker (`borrow::check_fn`) receives only function **parameter** types, not local
variable types. When a local variable's type is unknown, `is_copy` defaults to `false` (treats as
move type). This is correct for structs, strings, and closures, and is conservative for any
unknown type (may produce false-positive move errors for custom Copy types without explicit
annotation — acceptable for Phase 3 lite).

### 5. E0309 → E0401 migration

Field-not-found errors used `E0309` through Phase 2. Phase 3 introduces `E0401` as the canonical
code matching the spec. The checker still has `E0309` defined (for backward compatibility with
tests) but all three field-access error sites now emit `E0401`. The checker test
`r11_field_access_on_non_struct` accepts either code.

### 6. `Span` strategy — statement-granularity

Full span threading (every `Expr` node has a `Span`) requires a large AST refactor. Phase 3
instead tracks `current_stmt_span` in `InferCtx` which is set at each `Stmt` boundary in Block
inference. This gives statement-granularity locations without touching every `Expr` variant.
Dummy spans fall back to file-only or no-location output.

---

## Integration Test Fixtures

All fixture tests are in `crates/axon-core/tests/integration_fixtures.rs`.

| Fixture | Test | Expectation |
|---------|------|-------------|
| `borrow_errors.ax` | `borrow_errors_fixture_detected` | Must produce borrow errors (UseAfterMove/MoveBorrowed) |
| `chan_spawn.ax` | `chan_spawn_fixture_parses_cleanly` | No errors (Chan::new + chan<T>() both work) |
| `channels.ax` | `channels_fixture_parses_cleanly` | No errors (chan<T>(), ch.clone(), spawn, ch.recv()) |
| `closure_captures.ax` | `closure_captures_parses_cleanly` | No errors |
| `closures.ax` | `closures_fixture_type_checks_cleanly` | No errors (lambda captures + mutable counter) |
| `comptime_consts.ax` | `comptime_consts_parses_cleanly` | No errors |
| `generics.ax` | `generics_fixture_type_checks_cleanly` | No errors (generic fns + `Pair<A,B>` struct) |
| `select.ax` | `select_fixture_parses_cleanly` | No errors |
| `spans.ax` | `spans_fixture_emits_e0401_with_location` | Must produce E0401 |
| `traits.ax` | `traits_fixture_type_checks_cleanly` | No errors |

---

## Runtime ABI (`crates/axon-rt/`)

The runtime is a Rust staticlib. All symbols exported with `#[no_mangle] extern "C"`:

| Symbol | Signature | Notes |
|--------|-----------|-------|
| `__axon_chan_new` | `(capacity: i64) -> *void` | Arc<Chan> wrapped in raw pointer |
| `__axon_chan_send` | `(chan: *void, val: i64)` | Blocks until receiver ready |
| `__axon_chan_recv` | `(chan: *void) -> i64` | Blocks until sender ready |
| `__axon_chan_clone` | `(chan: *void) -> *void` | Increments Arc refcount |
| `__axon_chan_drop` | `(chan: *void)` | Decrements Arc refcount |
| `__axon_select` | `(chans: **void, n: i64) -> i64` | Spin-poll, returns ready index |
| `__axon_spawn` | `(fn_ptr: *void, env: *void)` | `std::thread::spawn` |
| `__axon_print` | `(ptr: *u8, len: i64)` | Writes UTF-8 to stdout |
| `__axon_sqrt/pow/floor/ceil` | math | f64 → f64 |
| `__axon_read_line` | `(out_len: *i64, out_ptr: **u8)` | Out-param ABI; reads stdin line |
| `__axon_read_file` | `(path_ptr, path_len, out_len: *i64, out_ptr: **u8)` | out_len<0 on error |
| `__axon_write_file` | `(path_ptr, path_len, content_ptr, content_len, out_err_len: *i64, out_err_ptr: **u8)` | out_err_len>0 on error |
| `__axon_sleep_ms` | `(ms: i64)` | Sleeps current thread |
| `__axon_now_ms` | `() -> i64` | Wall-clock ms since Unix epoch |

**Note**: The codegen (in `codegen.rs`) registers `__axon_chan_send` and `__axon_chan_recv` as
functions named `"send"` and `"recv"` for MethodCall dispatch. `"clone"` maps to
`__axon_chan_clone`.

---

## Phase 4 Feature Status

| Feature | Status | Notes |
|---------|--------|-------|
| Multi-file compilation (`axon build *.ax`) | ✅ Complete | `parse_source_files()` parallel parse; `merge_programs()` namespace merge; E0903 duplicate detection |
| Parallel test execution (`--jobs N`) | ✅ Complete | `run_tests_parallel()` with work-queue + mpsc; `--jobs 0` = auto CPU count |
| JSON test output (`--json`) | ✅ Complete | NDJSON per test + final summary line |
| Error codes E0901–E0906 | ✅ Complete | Module-not-found, circular import (E0902), duplicate name, cross-compile, cache |
| Trait validation E0501–E0504 | ✅ Complete | E0501 unknown trait; E0502 missing method; E0503 sig mismatch; E0504 bound not satisfied |
| E0902 circular import detection | ✅ Complete | `load_module_recursive()` DFS with loading_stack; transitive `use` loading |
| AXON_PATH stdlib search | ✅ Complete | `axon_search_dirs()` + `load_use_decls()`; E0901 for not-found; wired into CLI `run_check_pipeline` |
| LSP server (`axon lsp`) | ✅ Complete | `lsp.rs` JSON-RPC 2.0; hover+definition+publishDiagnostics |
| Formatter (`axon fmt [--check]`) | ✅ Complete | `fmt.rs` AST pretty-printer; 4-space indent, spaces around ops; idempotent |
| Doc generator (`axon doc`) | ✅ Complete | `doc.rs` — `///` comment extraction; Markdown output; `--out` flag |
| Incremental compilation cache | ✅ Complete | `cache.rs` SHA-256 `.axc`; `--no-cache`/`--cache-dir`; `axon cache clean` |
| Cross-compilation (`--target`) | ✅ Complete | `--target <triple>`; `emit_object_and_link`; `cross.toml` linker; E0904/E0905 |
| Phase 4 I/O builtins | ✅ Complete | `read_line`, `read_file`, `write_file` with Result<> wrapping; runtime in `axon-rt` |
| Phase 4 time builtins | ✅ Complete | `sleep_ms`, `now_ms`; direct calls to `__axon_sleep_ms`/`__axon_now_ms` |
| Phase 5 string builtins | ✅ Complete | `str_eq`, `str_contains`, `str_starts_with`, `str_ends_with`, `str_slice`, `str_index_of`, `char_at` |
| `str == str` operator | ✅ Complete | `BinOp::Eq/NotEq` on `StructValue` pair delegates to `str_eq` builtin |
| `to_str_bool` | ✅ Complete | Converts bool to `"true"` or `"false"` str |
| Phase 5 conversion | ✅ Complete | `parse_float` via strtod; returns `Result<f64, str>` |
| Phase 5 math | ✅ Complete | `abs_i64`, `min_i64`, `max_i64` — i64-native variants (no truncation to i32) |
| `break` / `continue` | ✅ Complete | Keywords added to lexer, parser, resolver, infer, checker, borrow, fmt, comptime, mono, codegen; `loop_stack` in CodeGen drives branch targets |
| Phase 6 string builtins | ✅ Complete | `str_to_upper`, `str_to_lower`, `str_trim`, `str_trim_start`, `str_trim_end`, `str_replace`, `str_repeat` |
| Phase 6 system builtins | ✅ Complete | `env_var` (getenv → Result<str,str>), `exit` (wraps C exit()) |
| `str_len` | ✅ Complete | Extracts the byte-length field from the str struct |
| Phase 7 string pad | ✅ Complete | `str_pad_start`, `str_pad_end` — left/right pad with fill char to a minimum width |
| Phase 7 float math | ✅ Complete | `min_f64`, `max_f64` — float-native min/max via fcmp+select |
| Phase 7 clamp | ✅ Complete | `clamp_i64`, `clamp_f64` — clamp value to [lo, hi] |
| `parse_bool` | ✅ Complete | `str → Result<bool, str>`; accepts "true"/"false" via strncmp |
| Phase 7 random | ✅ Complete | `random_i64(lo, hi)` and `random_f64()` via C rand() |
| Phase 8 for-in loop | ✅ Complete | `for i in start..end { body }` — integer range iteration; desugared to cond/body/incr/exit basic blocks in codegen |
| Phase 9 numeric casts | ✅ Complete | `i64_to_f64`, `f64_to_i64` — LLVM sitofp/fptosi; `abs_i64`, `abs_f64`, `sign_i64` |
| Phase 9 integer math | ✅ Complete | `pow_i64` (iterative), `sqrt_f64`, `floor_f64`, `ceil_f64`, `round_f64` via C libm |
| Phase 10 `@[test]` fixture | ✅ Complete | Fixture exercising `@[test]` annotated functions with `assert_eq` calls for Phases 8+9 |
| Phase 11 format strings | ✅ Complete | Fixture + auto-coerce FmtStr exprs to str (i64→to_str, f64→to_str_f64, bool→to_str_bool) |
| Phase 12 coverage | ✅ Complete | Fixture covering `to_str`, `parse_int`, `assert_eq_str/f64`, `char_at`, edge cases |
| Phase 13 structs | ✅ Complete | Fixture: struct literals, field access, nested structs, enum-with-struct-payload match |
| Phase 14 `?` operator | ✅ Complete | Fixture: Result/Option propagation with `?`, chained safe_div, Option::Some/None |
| Phase 15 higher-order fns | ✅ Complete | Fixture: apply, apply_twice, compose, make_adder, make_counter, fold_range |
| Phase 16 recursive types | ✅ Complete | Fixture: linked list (Nil/Cons) and binary tree (Leaf/Node) via recursive enums |
| Phase 17 match patterns | ✅ Complete | Fixture: guards, nested Option/Result, struct-payload enums, recursive Expr evaluator |
| Phase 18 string algorithms | ✅ Complete | Fixture: count_char, palindrome, digit_sum, find_char, str_hash, str builtin combos |
| Phase 19 numeric algorithms | ✅ Complete | Fixture: GCD, LCM, prime test, count_primes, Fibonacci (iterative), ipow (fast) |
| Phase 20 state machines | ✅ Complete | Fixture: traffic light, lexer-style scanner, running Stats accumulator, closure guard |
| Phase 21 error patterns | ✅ Complete | Fixture: parse_natural, first_nonzero, option_or, result_map_double, parse_ratio |
| Phase 22 generics usage | ✅ Complete | Fixture: Pair<A,B>, identity, always, is_some/is_none/is_ok/is_err, zip_options |
| Phase 23 traits in practice | ✅ Complete | Fixture: Printable, Comparable, Summable with Vec2/Vec3/Score impls |
| Phase 24 concurrency | ✅ Complete | Fixture: chan<T>, spawn, select, pipeline, fan-out patterns (parse + type-check) |
| Phase 25 integration | ✅ Complete | Fixture: mini interpreter — env lookup, eval, binops, if-expr, error propagation |
| Phase 26 comptime | ✅ Complete | Fixture: module-level constants, local comptime, bool flags, loop bounds, arithmetic, nested |
| Phase 27 advanced loops | ✅ Complete | Fixture: break early exit, continue skip, nested break, accumulator, result capture |
| Phase 28 generic types | ✅ Complete | Fixture: Pair/Triple structs, identity/constant/flip_apply, Option/Result generic helpers |
| Phase 29 mutual recursion | ✅ Complete | Fixture: is_even/is_odd, collatz, forward references, Ackermann, digit-parity |
| Phase 30 comprehensive | ✅ Complete | Fixture: structs/enums/traits/generics/closures/error-handling/comptime combined |
| Phase 31 ownership | ✅ Complete | Fixture: own/ref bindings, mixed let/own, ref arithmetic, ref in loops |
| Phase 32 string builtins | ✅ Complete | Fixture: str_slice, str_replace, str_repeat, case, trim, index_of, pad, to_str |
| Phase 33 math builtins | ✅ Complete | Fixture: abs_i64, min_i64, max_i64, clamp_i64, range_min/max, distance, median3 |
| Phase 34 float ops | ✅ Complete | Fixture: f64 literals, i64↔f64 cast, floor/ceil/sqrt/pow, abs_f64, parse_float |
| Phase 35 nested types | ✅ Complete | Fixture: Option<Result<>>, Result<Option<>>, nested structs, Option<Option<>> flatten |

---

## Phase 3 Carried-Forward Known Gaps

1. **AXON_PATH / module file loading**: ✅ `use a::b` searches filesystem recursively via
   `load_module_recursive()` with E0902 circular import detection.

2. **String memory management**: `axon_concat` mallocs and never frees. Deferred to Phase 5.

3. **Trait validation (E0501–E0504)**: ✅ Fully implemented in checker.
   - E0501: unknown trait in impl block
   - E0502: impl missing required method
   - E0503: impl method signature mismatch
   - E0504: call-site bound checking via `fn_bounds` + `impl_table`
   - AST: `FnDef.generic_bounds: Vec<(String, Vec<String>)>` added; parser captures `T: Bound + Bound2`

4. **`assert_eq` full generics**: Option B (typed variants) active. Full generic `assert_eq<T: Eq>`
   deferred to Phase 5 (requires trait bounds in monomorphization).

---

## Phase 4 Architectural Decisions

### 1. `parse_source_files()` — parallel parse via `std::thread`

Each source file is lexed and parsed in its own thread. The results (or errors) are collected
into a `Vec<(String, Program)>` in command-line order. Uses `Arc<Mutex<>>` to coordinate:

```rust
pub fn parse_source_files(paths: &[PathBuf]) -> Result<Vec<(String, Program)>, Vec<String>>
```

### 2. `merge_programs()` — E0903 duplicate detection

Items from all files are merged into a single `Program`. Each named item (`FnDef`, `TypeDef`,
`EnumDef`, `TraitDef`, `ModDecl`, `LetDef`) is tracked by name; a duplicate produces E0903 and
the second definition is **dropped** so downstream passes see a consistent AST. Unnamed items
(`ImplBlock`, `UseDecl`) are always included.

### 3. Parallel test runner — work-queue + `mpsc`

Workers pull test indices from a `Arc<Mutex<Vec<usize>>>` stack and send `(idx, TestResult)`
pairs back over an `mpsc::channel`. Results are reassembled into declaration order. The
`--jobs 0` flag resolves via `std::thread::available_parallelism()`.

### 4. JSON test output — manual NDJSON (no serde_json in binary)

The binary crate avoids `serde_json` (it causes trait-solver explosion with inkwell). Test
results are serialised manually via format strings with `\\`/`"` escaping.

### 5. Incremental cache — `.axc` binary format

Each `.axc` file has an 8-byte magic header (`AXONCACH`), a 4-byte LE version string length, the
compiler version string, and then raw LLVM bitcode.  The cache key is SHA-256 over
(source bytes, compiler version).  On a cache hit, `compile_bitcode_to_binary()` loads the
bitcode into a fresh inkwell context and links without re-running the Axon frontend.

The serde_json restriction applies to test output only.  The LSP server (`lsp.rs`) is compiled
as a **library module** (not the binary crate) and uses `serde_json` freely — inkwell types are
not in scope there.

### 6. Cross-compilation — `emit_object_and_link()` extracted

The link step was refactored from `compile_to_binary` into a free function
`emit_object_and_link(module, output_path, release, target_triple)`.  Both the normal build path
and the cache-restore path call this function.  The linker is selected from:
1. `~/.config/axon/cross.toml` `[target.<triple>] linker = "..."` (cross-compiled targets)
2. Host `cc` / `clang` / `gcc` (native target)

### 7. LSP hover — word-at-cursor (no per-expression spans)

Phase 3 added only statement-granularity spans to avoid a large AST refactor.  The LSP hover
implementation therefore uses a **word-at-cursor** strategy: extract the identifier under the
cursor from the source text, then look it up in `InferCtx.fn_sigs` (function signatures) and
`InferCtx.struct_fields` (struct types).  Local variables are not hoverable in Phase 4.

### 8. E0504 — call-site trait bound checking

**Strategy**: Avoid touching all 63 `generic_params` references by adding a parallel
`generic_bounds: Vec<(String, Vec<String>)>` field to `FnDef` only (bounds on type/enum defs
are deferred to a future phase). The parser captures `T: Bound1 + Bound2` syntax and stores the
bounds alongside the plain name list.

**CheckCtx additions**:
- `impl_table: HashMap<String, HashSet<String>>` — built from `ImplBlock` items; maps
  `type_name → {trait names it implements}`.
- `fn_bounds: HashMap<String, Vec<(String, Vec<String>)>>` — maps `fn_name → [(param, traits)]`.
- `check_trait_bounds()` — called at the end of `check_call_arity_and_types`; for each TypeParam
  argument position with bounds, resolves the concrete arg type and checks `impl_table`.

**`resolve_expr_type` improvement**: `Expr::StructLit` now returns `Type::Struct(name)` when the
struct name is known in `struct_fields`, enabling scope-level tracking of struct-typed locals.

---

## Compile Performance

`cargo build --package axon-core` in debug mode takes **25–30 minutes** due to LLVM IR
generation for `codegen.rs` (5000+ lines using inkwell). The incremental cache helps on
recompiles that only touch non-codegen files. The `axon-rt` crate compiles in under 30 seconds.

**Workaround**: Set `RUST_MIN_STACK=16777216` before cargo commands to avoid SIGSEGV from
rustc stack overflow on deeply recursive types.

---

## Phase 4 Status

All Phase 4 features are complete. The compiler now ships:
- Multi-file builds + parallel parse + E0901–E0906 error codes
- Parallel test runner (`--jobs N`) + JSON test output (`--json`)
- AXON_PATH stdlib search
- Formatter (`axon fmt`)
- Doc generator (`axon doc`)
- Incremental cache (`~/.cache/axon/*.axc`, SHA-256 keyed)
- Cross-compilation (`--target <triple>`, `cross.toml` linker config)
- LSP server (`axon lsp`, JSON-RPC 2.0, hover + go-to-definition + diagnostics)
