# Axon Changelog

## Phase 2 (current)

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
