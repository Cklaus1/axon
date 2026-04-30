# Axon Compiler — Phase 1 Spec

**Goal**: Source file (`.ax`) → native binary. Basic type safety. No borrow checker.  
**Implementation language**: Rust  
**Backend**: LLVM (via `inkwell` crate — safe Rust bindings to LLVM C API)  
**Timeline**: 3-6 months  

---

## Scope

### In Phase 1
- Lexer + parser (done)
- Name resolution
- Type inference (concrete types only — no generics)
- Type checking (basic rules — see below)
- LLVM IR emission
- LLVM optimization passes (O0 debug / O2 release)
- Native binary output (x86-64, AArch64)
- `axon build`, `axon run`, `axon check`, `axon test`
- Structured error messages (JSON for AI, pretty for humans)
- `@[test]` execution

### Explicitly Out of Phase 1
```
Borrow checker           → Phase 2
Generics / traits        → Phase 2
Uncertain<T>             → Phase 2
Temporal<T>              → Phase 2
Goal type                → Phase 2
@[contained]             → Phase 2 (parsed + stored, not enforced)
@[corrigible]            → Phase 2 (parsed + stored, not enforced)
@[adaptive] / @[goal]    → Phase 2 (parsed + stored, not enforced)
@[agent]                 → Phase 3
@[verify]                → Phase 4
WASM / JS targets        → Phase 3
std.causal               → Phase 2
std.ai                   → Phase 3
Self-improving compiler  → Phase 4
```

---

## Pipeline

```
main.ax
  │
  ▼
[1] Lexer           token.rs / lexer.rs  (done)
  │
  ▼
[2] Parser          parser.rs            (done) → Program AST
  │
  ▼
[3] Name Resolution resolver.rs          → decorated AST with resolved references
  │
  ▼
[4] Type Inference  infer.rs             → constraint set
  │
  ▼
[5] Type Check      checker.rs           → typed AST or errors
  │
  ▼
[6] IR Emission     codegen.rs           → LLVM IR (inkwell)
  │
  ▼
[7] LLVM Passes     (via inkwell)        → optimized IR
  │
  ▼
[8] Native Binary   (LLVM target machine) → ./output
```

Each stage runs to completion or emits all errors it can find before stopping.
Never stop at first error — collect and report all.

---

## Crate Structure (Phase 1 additions)

```
crates/axon-core/src/
  token.rs        done
  lexer.rs        done
  ast.rs          done
  parser.rs       done
  lib.rs          done
  main.rs         done (extend with new subcommands)
  resolver.rs     NEW — name resolution
  infer.rs        NEW — type inference (constraint gen + unification)
  checker.rs      NEW — type rule validation
  types.rs        NEW — type representation (separate from AST)
  codegen.rs      NEW — LLVM IR emission via inkwell
  error.rs        NEW — structured error types + dual renderer
  builtins.rs     NEW — built-in types and functions
```

---

## Types (Phase 1)

### Primitive Types
```
i8  i16  i32  i64     signed integers
u8  u16  u32  u64     unsigned integers
f32  f64              floats
bool                  true / false
str                   owned UTF-8 string (heap)
()                    unit type (void equivalent)
```

### Compound Types (Phase 1)
```
Option<T>             where T is a concrete primitive or struct
Result<T,E>           where T and E are concrete
[T]                   slice / array (concrete T)
(A, B, C)             tuple (concrete types)
fn(A,B)->C            function type
struct / enum         user-defined (concrete fields only — no generics)
```

### Deferred Types (parsed, stored in AST, no inference or checking)
```
Uncertain<T>
Temporal<T>
Goal
Chan<T>
Generic params <T:Trait>
```

### Type Representation (types.rs)
```rust
pub enum Type {
    I8, I16, I32, I64,
    U8, U16, U32, U64,
    F32, F64,
    Bool,
    Str,
    Unit,
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    Slice(Box<Type>),
    Tuple(Vec<Type>),
    Fn(Vec<Type>, Box<Type>),
    Struct(String),
    Enum(String),
    Unknown,         // inference variable — resolved during unification
    Deferred(String) // Uncertain<T>, Temporal<T>, etc. — stored, not checked
}
```

---

## Name Resolution (resolver.rs)

Single-pass over the AST. Builds a symbol table, resolves every `Ident` to its declaration.

### Rules
```
1. All top-level items collected first (fns, types, enums, mods)
   → allows forward references between top-level items

2. Function bodies resolved in declaration order
   → local let/own/ref bindings are sequential (no forward refs in bodies)

3. Use declarations resolved to their module members
   → mod server; use server.{listen, Router} — both must exist

4. Duplicate names: error at declaration site

5. Unresolved name: error at use site with suggestion (closest match by edit distance)
```

### Errors (name resolution)
```
E0001  undefined name '{name}' — did you mean '{suggestion}'?
E0002  '{name}' already defined in this scope (first defined at {loc})
E0003  module '{mod}' not found
E0004  '{item}' not exported from '{mod}'
```

---

## Type Inference (infer.rs)

Simple constraint-based inference — not full Hindley-Milner.
Enough to infer `let x=42` → `i32` and return types from function calls.

### Algorithm
```
1. Walk typed AST from name resolution
2. For each expression, generate constraints:
     add(a:i32, b:i32)->i32  →  constraint: return type = i32
     let x=42                →  constraint: x = typeof(42) = i32
     let y=add(x, 1)         →  constraint: y = return_type(add) = i32
3. Unify constraints (union-find)
     if two constraints conflict → type error
4. Substitute resolved types back into AST
5. Any remaining Unknown after unification → error E0101
```

### Literal Type Rules
```
42          → i32  (default integer)
42u64       → u64  (explicit suffix)
3.14        → f64  (default float)
3.14f32     → f32
true/false  → bool
"hello"     → str
()          → ()
```

### Numeric Coercions (Phase 1 — explicit only)
```
No implicit numeric coercions.
i32 + i64 → E0201 type mismatch — use explicit cast: a as i64 + b
```

---

## Type Checker (checker.rs)

Validates rules after inference. All rules checked; all violations collected.

### Rules Enforced in Phase 1

**R01 — Option<T> must be handled**
```
// Error: using Option<T> as T directly
let x:Option<i32>=map.get("k")
let y=x+1              // E0301: Option<i32> used as i32 — match or unwrap_or first
```

**R02 — Result<T,E> must be handled**
```
fn process()->Result<(),Error>{
  write_file()       // E0302: Result<(),Error> ignored — use ? or match
  Ok(())
}
```

**R03 — ? operator only in Result-returning functions**
```
fn bad(){
  fetch(url)?        // E0303: ? used in fn returning () — change return to Result
}
```

**R04 — match exhaustiveness (Option and Result only in Phase 1)**
```
match opt{
  Some(x) => use(x)  // E0304: non-exhaustive match — missing None arm
}
```

**R05 — function call arity**
```
add(1,2,3)           // E0305: add expects 2 args, got 3
```

**R06 — function call type agreement**
```
add("a","b")         // E0306: arg 0 expects i32, got str
```

**R07 — return type agreement**
```
fn add(a:i32,b:i32)->i32{ "wrong" }  // E0307: expected i32, found str
```

**R08 — no null (null keyword doesn't exist — parse error, not type error)**

**R09 — no throw/raise (doesn't exist in grammar — parse error)**

**R10 — unknown type annotation**
```
let x:Banana=...     // E0308: unknown type 'Banana'
```

**R11 — struct field access on non-struct**
```
let x=42
x.name               // E0309: i32 has no field 'name'
```

**R12 — deferred types are transparent (no errors, just stored)**
```
let x:Uncertain<i32>=...  // parsed, stored as Deferred("Uncertain<i32>"), no checking
```

---

## Error Message Format

Every error has two renderings from the same `AxonError` struct.
Same design as the language PRD — AI gets JSON, humans get pretty.

### Struct
```rust
pub struct AxonError {
    pub code:     &'static str,           // "E0301"
    pub message:  String,                 // human summary
    pub node_id:  String,                 // "#fn_process.body.stmt_2"
    pub file:     String,                 // "main.ax"
    pub line:     u32,
    pub col:      u32,
    pub expected: Option<String>,
    pub found:    Option<String>,
    pub fix:      Option<String>,         // suggested fix, machine-applicable when possible
    pub severity: Severity,              // Error | Warning | Info
}
```

### JSON renderer (--format=json or piped output)
```json
{
  "error": "E0301",
  "node": "#fn_main.body.stmt_3",
  "file": "main.ax",
  "line": 12,
  "col": 5,
  "expected": "i32",
  "found": "Option<i32>",
  "fix": "use x.unwrap_or(0) or match x { Some(v) => v, None => 0 }"
}
```

### Pretty renderer (terminal)
```
error[E0301]: Option<i32> used as i32 at main.ax:12:5
  12 │   let y=x+1
     │         ^ expected i32, found Option<i32>
  help: use x.unwrap_or(0), or match x { Some(v) => v+1, None => 0 }
```

### Format selection
```
axon check main.ax              → pretty (terminal default)
axon check main.ax --json       → JSON (for AI tooling)
axon check main.ax 2>&1 | ...  → auto-detects pipe, switches to JSON
```

---

## Known-But-Deferred Attributes

Attributes parsed and stored in AST. Not enforced in Phase 1.
Compiler emits `info` (not warning, not error) on first use.

```
@[target(wasm)]       info: wasm target deferred to Phase 3, building native
@[target(native)]     honored — native is the only Phase 1 target
@[adaptive(...)]      info: adaptive zones deferred to Phase 2
@[goal(...)]          info: goal optimization deferred to Phase 2
@[agent]              info: agent modules deferred to Phase 3
@[contained(...)]     info: containment enforcement deferred to Phase 2
@[corrigible(...)]    info: corrigibility enforcement deferred to Phase 2
@[sensitive(...)]     info: sensitivity enforcement deferred to Phase 2
@[verify]             info: formal verification deferred to Phase 4
@[ai(...)]            info: AI policy enforcement deferred to Phase 3
@[layout(...)]        info: memory layout control deferred to Phase 2
@[test]               honored — tests run in Phase 1
@[test(should_fail)]  honored
```

---

## LLVM IR Emission (codegen.rs)

Uses `inkwell` crate — idiomatic Rust wrapper around LLVM C API.

### Type Mappings
```
Axon type        LLVM type
─────────────────────────────────────────
i8               i8
i16              i16
i32              i32
i64              i64
u8               i8   (signedness in ops, not storage)
u16              i16
u32              i32
u64              i64
f32              float
f64              double
bool             i1
str              { i64, ptr }   (length + heap pointer)
()               void
Option<T>        { i1, T }      (tag + value)
Result<T,E>      { i1, [max(sizeof(T), sizeof(E))] }  (tag + union)
[T]              { i64, ptr }   (length + pointer)
struct S         LLVM named struct with field types
```

### Function Emission
```
// Axon
fn add(a:i32, b:i32)->i32 { a+b }

// LLVM IR
define i32 @add(i32 %a, i32 %b) {
entry:
  %result = add i32 %a, %b
  ret i32 %result
}
```

### Option<T> Emission
```
// Axon
match opt {
  Some(x) => x+1,
  None    => 0
}

// LLVM IR
%tag = extractvalue { i1, i32 } %opt, 0
%val = extractvalue { i1, i32 } %opt, 1
br i1 %tag, label %some, label %none
some:
  %result_some = add i32 %val, 1
  br label %merge
none:
  br label %merge
merge:
  %result = phi i32 [ %result_some, %some ], [ 0, %none ]
```

### Result<T,E> with ? Operator
```
// Axon
let x=parse(data)?

// LLVM IR
%res = call { i1, i32 } @parse(...)
%ok  = extractvalue { i1, i32 } %res, 0
br i1 %ok, label %cont, label %early_ret
early_ret:
  %err = extractvalue { i1, i32 } %res, 1
  ret { i1, i32 } { i1 0, i32 %err }    ; propagate Err
cont:
  %x = extractvalue { i1, i32 } %res, 1 ; extract Ok value
```

### Block Expressions (last expr is return value)
```
// Axon
fn compute()->i32 { let a=1; let b=2; a+b }

// LLVM IR — SSA form, no explicit return needed until block end
define i32 @compute() {
entry:
  %a = i32 1
  %b = i32 2
  %result = add i32 %a, %b
  ret i32 %result
}
```

### Spawn (Phase 1 — sequential stub)
```
// Axon
spawn{ fetch(url) }

// Phase 1: runs inline (no goroutine yet — goroutine runtime is Phase 3)
// compiler emits info: spawn runs sequentially in Phase 1
call void @fetch(ptr %url)
```

---

## CLI (Phase 1)

```
axon build <file.ax>           compile to native binary (output: ./a.out or --out=<name>)
axon build <file.ax> --release optimize (O2) vs default debug (O0)
axon run   <file.ax>           build + execute (passes remaining args to program)
axon check <file.ax>           type check only, no binary produced
axon check <file.ax> --json    emit errors as JSON (one object per line)
axon parse <file.ax>           print AST as JSON  (done)
axon test  <file.ax>           run all @[test] functions, report pass/fail
axon test  <file.ax> --filter=<name>  run matching tests only
```

### Exit codes
```
0    success
1    compile error
2    type error
3    link error
4    internal compiler error (ICE) — always prints bug report template
```

### Internal Compiler Error format
```
axon: internal compiler error at codegen.rs:142
  please report at github.com/axon-lang/axon/issues
  include: axon version, OS, and this file:
  --- snip ---
  { "ice": true, "stage": "codegen", "node": "#fn_main.body.stmt_3", ... }
  --- snip ---
```

---

## Test Execution (@[test])

```
// Axon
fn add(a:i32,b:i32)->i32{a+b}
@[test] fn test_add(){ assert(add(2,3)==5) }
@[test(should_fail)] fn test_wrong(){ assert(add(2,3)==6) }
```

```
axon test main.ax

running 2 tests
test test_add        ... ok  (0.3ms)
test test_wrong      ... ok  (0.1ms) — correctly failed
test result: ok. 2 passed, 0 failed (0.4ms total)
```

### Built-in test assertions (Phase 1)
```
assert(expr)                    fails if expr is false
assert_eq(a, b)                 fails if a != b, prints both values
assert_err(result)              fails if result is Ok
assert(expr, "message")        custom failure message
```

---

## Dependencies (Cargo additions for Phase 1)

```toml
inkwell   = { version = "0.4", features = ["llvm17-0"] }   # LLVM bindings
# LLVM 17 must be installed on the system (apt/brew install llvm-17)
```

---

## Phase 1 Sample: Full Round-Trip

This program must compile and run correctly in Phase 1:

```
// hello.ax
fn greet(name:str)->str{
  "hello {name}"
}

fn safe_div(a:i32,b:i32)->Result<i32,str>{
  if b==0{ Err("division by zero") }
  else{ Ok(a/b) }
}

fn main()->Result<(),str>{
  let msg=greet("axon")
  let result=safe_div(10,2)?
  Ok(())
}

@[test] fn test_greet(){ assert_eq(greet("world"),"hello world") }
@[test] fn test_div(){ assert_eq(safe_div(10,2),Ok(5)) }
@[test] fn test_div_zero(){ assert_err(safe_div(10,0)) }
```

```
axon build hello.ax && ./a.out    # exits 0
axon test  hello.ax               # 3 passed
axon check hello.ax --json        # no output (no errors)
```

---

## Verification Checklist

Phase 1 is done when:

- [ ] `axon build hello.ax` produces a running binary
- [ ] `axon test` runs `@[test]` functions and reports correctly
- [ ] `axon check --json` emits valid JSON errors (one per line)
- [ ] All 12 type rules (R01-R12) catch their target errors
- [ ] All 4 name resolution errors (E0001-E0004) fire correctly
- [ ] Deferred attributes emit `info`, not errors
- [ ] ICE handler fires on intentional panic in compiler with structured output
- [ ] Option<T> match compiles to correct branch IR
- [ ] Result<T,E> with `?` compiles to correct early-return IR
- [ ] `--release` binary is measurably smaller/faster than `--debug`
