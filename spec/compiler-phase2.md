# Axon Compiler — Phase 2 Spec

**Goal**: Close all Phase 1 gaps, add structs/enums, working slices, lambdas, extended builtins, and unsigned arithmetic. Set foundation for Phase 3 (borrow checker, generics, traits).  
**Builds on**: Phase 1 (`spec/compiler-phase1.md`)  
**Timeline**: 1-3 months after Phase 1 ships

---

## Phase 2 Scope

### In Phase 2
- Result<T,E> coherent union layout (fixes match on mixed payload types)
- Struct declaration, construction, and field access
- Enum declaration and match dispatch (user-defined ADTs)
- Slice indexing (`receiver[i]`)
- Lambda / anonymous functions (basic closure capture — by copy)
- Extended builtin functions: `assert_eq`, `assert_err`, `len`, `to_str` overloads
- Unsigned arithmetic: `udiv`, `urem`, `UGT/ULT/UGE/ULE` comparisons
- `@[test(should_fail)]` attribute — a test passes only if it panics
- String interpolation: `"hello {name}"` → format call at runtime
- `parse_int`, `parse_float` builtins
- Deferred-attribute info messages (warn without error when `@[agent]`, `@[goal]`, etc. appear)

### Explicitly Out of Phase 2
```
Borrow checker          → Phase 3  (requires MIR-like mid-IR)
Generics / traits       → Phase 3  (requires monomorphization pass)
Uncertain<T>            → Phase 3  (depends on generics)
Temporal<T>             → Phase 3  (depends on generics)
Green threads / chan     → Phase 3  (requires runtime)
WASM / JS targets       → Phase 3
Comptime execution      → Phase 4
Self-improving passes   → Phase 4
```

---

## Result<T,E> — Coherent Union Layout

### Problem (Phase 1)
`emit_result(true, ok_val)` built `{i1, T}` and `emit_result(false, err_val)` built `{i1, E}`.
When T ≠ E (e.g. `Result<i64, str>`), the two branches produce LLVM structs of different types,
which makes the phi node in `if/else` ill-typed.

### Solution (Phase 2)
Both arms produce the same canonical layout: `{ i1, [max(sizeof T, sizeof E) x i8] }`.

The codegen threads `current_result_types: Option<(Type, Type)>` through all function bodies.
When emitting `Ok(val)` or `Err(val)` inside a function returning `Result<T,E>`:

1. Compute payload size = max(sizeof T, sizeof E), min 1 byte.
2. Alloca `{ i1, [N x i8] }`.
3. Store tag (1 for Ok, 0 for Err) into field 0.
4. Get ptr to payload field. Load `val` into it (the first sizeof(val) bytes).
5. Return the loaded struct value.

When extracting via match `Ok(v)` or `Err(e)`:

1. Extract tag (field 0), check expected value.
2. Extract payload (field 1) → `[N x i8]`.
3. Alloca `[N x i8]`, store payload.
4. Cast alloca ptr → typed value, load → bound variable.

### LLVM IR shape
```
; fn safe_div(a: i64, b: i64) -> Result<i64, str>
; sizeof i64 = 8, sizeof str = 16 → payload = [16 x i8]

define { i1, [16 x i8] } @safe_div(i64 %a, i64 %b) {
entry:
  ; ...
  ; Ok(a / b)
  %result = alloca { i1, [16 x i8] }
  %tagptr = getelementptr { i1, [16 x i8] }, ptr %result, i32 0, i32 0
  store i1 1, ptr %tagptr            ; tag = Ok
  %payptr = getelementptr { i1, [16 x i8] }, ptr %result, i32 0, i32 1
  store i64 %divresult, ptr %payptr  ; write i64 into first 8 bytes
  %rv = load { i1, [16 x i8] }, ptr %result
  ret { i1, [16 x i8] } %rv
}
```

---

## Struct Support

### TypeDef → LLVM Named Struct
```axon
type Point = { x: f64, y: f64 }
```

In Pass 1 (declare pass), for every `TypeDef`:
1. Collect field types in declaration order.
2. Register an LLVM opaque named struct: `context.opaque_struct_type("Point")`.
3. Set its body: `named_struct.set_body(&[f64, f64], false)`.
4. Store field name→index in `struct_fields: HashMap<String, Vec<String>>`.

### Struct Literal
```axon
let p = Point { x: 1.0, y: 2.0 }
```

AST node: `Expr::StructLit { name: String, fields: Vec<(String, Expr)> }`.

Parser: after parsing an `ident`, if the next token is `{` and the token after that is `ident :`, parse as a struct literal rather than a block.

Codegen:
1. Look up LLVM named struct type for `name`.
2. Emit each field expression.
3. Alloca the struct type.
4. GEP + store each field.
5. Load and return.

### Field Access
```axon
p.x
p.name
```

Codegen for `FieldAccess { receiver, field }`:
1. Emit receiver (should be a struct value or alloca).
2. Look up struct type in `struct_fields` to get field index.
3. If receiver is loaded struct value:
   - Alloca the struct type, store the value.
   - GEP to the field ptr (using named struct type + field index).
   - Load field value.
4. Return loaded field value.

---

## Enum Support (User-Defined ADTs)

### EnumDef
```axon
enum Shape { Circle { radius: f64 }, Rect { w: f64, h: f64 }, Point }
```

Phase 2 lowers enums to tagged unions:
```
{ i32 tag, [max(sizeof variant payloads) x i8] payload }
```

Each variant gets an integer tag (0, 1, 2, ...).  
Variant names are registered in `enum_variants: HashMap<String, Vec<(String, usize, Vec<Type>)>>`.

### Enum Match
```axon
match shape {
    Shape::Circle { radius } => ...
    Shape::Rect { w, h } => ...
    Shape::Point => ...
}
```

Pattern `Struct { name: "Circle", fields: [...] }` →
1. Extract tag (field 0).
2. Compare to tag for "Circle".
3. If matching: extract payload ([N x i8]), cast, load Circle fields.

*Note: Enum variant constructors (`Shape::Circle { radius: 1.0 }`) require parser support for `::` paths — Phase 2 addition.*

---

## Slice Indexing

```axon
let arr = [1, 2, 3]
let x = arr[1]
```

Slice struct: `{ i64 len, ptr data }`.

Codegen for `Index { receiver, index }`:
1. Emit receiver → slice struct value.
2. Alloca slice struct, store, then GEP to get the data ptr.
3. Emit index → i64 value.
4. GEP into data ptr with the element type and index.
5. Load and return element.

Requires knowing element type T from `Type::Slice(T)`. Phase 2 threads this from inference.

---

## Lambda / Anonymous Functions

```axon
let f = (a, b) => a + b
let result = f(3, 4)
```

Phase 2 lowering:
1. Emit as a named module-level function: `__lambda_N` for each lambda.
2. Parameters typed as `i64` by default (no type annotation on lambda params — inferred from context in Phase 3).
3. Body emitted normally.
4. Return the function pointer as an opaque `ptr`.

Closure capture (by copy):
- Captured variables are passed as hidden extra parameters.
- Phase 2: only value-captured (no mutable closure captures — Phase 3).

---

## Extended Builtins

### assert_eq(a: i64, b: i64)
Fails with message `"assertion failed: {a} != {b}"` if `a != b`.

### assert_err(tag: i1)
Takes a Result tag value. Fails if tag == 1 (Ok). Used in test contexts.
Phase 2 limitation: requires the tag to be passed explicitly until generics land.

### len(s: str) -> i64
Extracts field 0 (length) from the `{ i64, ptr }` str struct.

### len_slice(s: [T]) -> i64
Extracts field 0 from the `{ i64, ptr }` slice struct.

### to_str overloads
- `to_str_i32(n: i32) -> str` — formats i32 via snprintf("%d")
- `to_str_f64(n: f64) -> str` — formats f64 via snprintf("%.6g")
- `to_str_bool(b: bool) -> str` — returns "true" or "false"

Phase 2 note: `to_str` remains i64-only in the Axon type system. Overloads are for internal codegen use, accessible via integer coercions at call sites.

### parse_int(s: str) -> Result<i64, str>
Calls C `strtol`. Returns Ok(n) or Err("invalid integer").

### exit(code: i64)
Calls C `exit(code)`.

---

## Unsigned Arithmetic

Phase 1 used signed variants for all integer ops. Phase 2 uses the Axon semantic type to select:

```
Operation     Signed type    Unsigned type
div           sdiv           udiv
rem           srem           urem
< > <= >=     SLT/SGT/SLE/SGE   ULT/UGT/ULE/UGE
```

Implementation: thread `current_binop_lhs_sem: Type` through `emit_binop` to select the correct LLVM instruction.

---

## @[test(should_fail)]

A test tagged `@[test(should_fail)]` passes only if the test function calls `exit(1)` (i.e., an assertion fires). Phase 2 JIT runner:
- For normal tests: pass if JIT call returns normally.
- For `should_fail` tests: run in a subprocess, pass if exit code ≠ 0.

Subprocess approach avoids the Phase 1 limitation where `exit(1)` kills the entire test process.

---

## String Interpolation

```axon
let name = "world"
let msg = "hello {name}"
```

Phase 2 parse: the lexer produces a `Token::FmtStr(parts)` where parts is a list of `FmtPart::Literal(str)` or `FmtPart::Expr(str)`.

Codegen: for each format expression, call `to_str` if needed, concatenate via `snprintf` into a heap-allocated buffer.

Phase 2 limitation: only `{ident}` interpolation (no format specifiers, no nested exprs).

---

## Error Message Improvements (Phase 2)

All Phase 1 type rules (R01–R12) should fire with node IDs and fix suggestions:

```
error[E0301]: Option<i64> used as i64 at main.ax:12:5
  12 │   let y = x + 1
     │           ^ expected i64, found Option<i64>
  help: use x.unwrap_or(0) or match x { Some(v) => v, None => 0 }
```

Phase 2 adds line/column tracking to all AST nodes (currently spans are not stored in Phase 1 AST).

---

## Struct + Result Verification Checklist

Phase 2 is done when these programs compile and run correctly:

### options.ax
```axon
fn safe_div(a: i64, b: i64) -> Result<i64, str> {
    if b == 0 { Err("division by zero") }
    else { Ok(a / b) }
}
fn main() {
    let x = safe_div(10, 2)
    match x {
        Ok(v) => println(to_str(v))
        Err(e) => eprintln(e)
    }
}
```
Expected: prints `5`

### structs.ax
```axon
type Point = { x: f64, y: f64 }

fn distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x
    let dy = a.y - b.y
    // sqrt not in phase 2 stdlib — return sum of squares for now
    dx * dx + dy * dy
}

fn main() {
    let a = Point { x: 1.0, y: 2.0 }
    let b = Point { x: 4.0, y: 6.0 }
    println(to_str(distance(a, b)))
}
```
Expected: prints `25`

### slices.ax
```axon
fn sum(arr: [i64]) -> i64 {
    let total = 0
    let i = 0
    // Phase 2: no while loops yet — unroll manually
    total
}

fn main() {
    let arr = [10, 20, 30]
    println(to_str(arr[0]))
    println(to_str(arr[1]))
    println(to_str(arr[2]))
}
```
Expected: prints 10, 20, 30

---

## Pipeline Changes (Phase 2)

```
main.ax
  │
  ▼
[1] Lexer           token.rs    (extended: FmtStr token)
  │
  ▼
[2] Parser          parser.rs   (extended: StructLit, EnumVariant, :: paths)
  │
  ▼
[3] Name Resolution resolver.rs  (extended: enum variants, module paths)
  │
  ▼
[4] Type Inference  infer.rs    (extended: struct field types, enum variants)
  │                              ↓ expr_types map wired into checker
  ▼
[5] Type Check      checker.rs  (extended: R01-R12 fire with spans)
  │
  ▼
[6] IR Emission     codegen.rs  (major: Result union, struct/enum, slice index,
  │                              lambda, unsigned ops, extended builtins)
  ▼
[7] LLVM Passes                 (unchanged)
  │
  ▼
[8] Native Binary               (unchanged)
```

---

## New Error Codes (Phase 2)

```
E0401  struct '{name}' has no field '{field}'
E0402  struct literal missing field '{field}' — '{name}' requires: {fields}
E0403  enum variant '{name}::{variant}' does not exist
E0404  non-exhaustive enum match — missing variant: '{variant}'
E0405  cannot index non-slice type '{ty}'
E0406  lambda capture of '{name}' by reference not yet supported (use by copy)
E0407  integer literal overflow: {n} does not fit in {ty}
E0408  format interpolation requires a str-convertible type, found {ty}
```

---

## Dependencies (no additions for Phase 2)

Phase 2 uses no new Cargo dependencies. All LLVM functionality needed is already available through inkwell 0.4.
