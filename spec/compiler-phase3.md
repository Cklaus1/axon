# Axon Compiler — Phase 3 Spec

**Goal**: Generics, traits, true closure captures, channels, a lite borrow checker, comptime evaluation, and span-threaded error messages. Transforms Axon from a typed scripting language into a systems language with safe concurrency.  
**Builds on**: Phase 2 (`spec/compiler-phase2.md`)  
**Timeline**: 3-6 months after Phase 2 ships

---

## Phase 3 Scope

### In Phase 3
- Generics — monomorphization over concrete type arguments
- Traits / interfaces — vtable-based trait objects and static dispatch
- Closures with captures — heap-allocated environment structs, fat-pointer calling convention
- Channels (`chan<T>`) — runtime-backed typed channels, `spawn`, `select`
- Borrow checker (lite) — single-ownership, move semantics, lexical scope only (no lifetimes)
- `comptime` expressions — constant folding at compile time, pure-function evaluation
- Error propagation improvements — `Span` on every AST node, line:col in all diagnostics

### Explicitly Out of Phase 3
```
Lifetime parameters          → Phase 4  (requires region inference)
Uncertain<T> / Temporal<T>   → Phase 4  (depends on full generics + runtime)
WASM / JS targets            → Phase 4
Self-improving passes        → Phase 4
@[agent] enforcement         → Phase 4
@[verify] static proofs      → Phase 4
Async/await syntax           → Phase 4  (channels cover the use-cases)
Associated types on traits   → Phase 4
```

---

## 1. Generics

### Motivation

Phase 2 required hand-rolled concrete helpers (`abs_i32`, `min_i32`, `to_str_i32`) wherever a
function needed to work over more than one type. That pattern does not scale. Generics let authors
write one definition and let the compiler produce concrete instantiations per call site — zero
runtime overhead, full type safety.

### Syntax

```ebnf
generic_params ::= "<" type_param ("," type_param)* ">"
type_param     ::= ident ( ":" trait_bound ("+" trait_bound)* )?

fn_def         ::= "fn" ident generic_params? "(" param_list ")" ("->" type)? block
type_def       ::= "type" ident generic_params? "=" "{" field_list "}"
enum_def       ::= "enum" ident generic_params? "{" variant_list "}"

generic_type   ::= ident "<" type_arg ("," type_arg)* ">"
type_arg       ::= type
```

**Examples**:
```axon
fn identity<T>(x: T) -> T { x }

fn swap<A, B>(a: A, b: B) -> (B, A) { (b, a) }

type Pair<A, B> = { first: A, second: B }

enum Maybe<T> { Just { val: T }, Nothing }
```

### Semantic Rules

**G01** — Type parameters are scoped to their declaring item. A reference to `T` inside `fn
identity<T>` resolves to the declared parameter; the same `T` elsewhere is a distinct variable.

**G02** — At each call site the compiler infers concrete type arguments from the argument types via
unification. Explicit type arguments (`identity::<i64>(42)`) are allowed; they override inference.

**G03** — If a type parameter carries a trait bound (`T: Displayable`) the checker verifies that
the concrete type argument satisfies the bound at each instantiation site (see §2).

**G04** — Recursive generic instantiations (e.g. `identity::<Pair<i64, i64>>`) are allowed but
must terminate. The compiler limits instantiation depth to 64.

**G05** — Generic items that are never instantiated emit no LLVM IR.

### LLVM IR Lowering — Monomorphization

The compiler performs a whole-program monomorphization pass after type-checking:

1. **Collect instantiation sites**: walk the typed AST. For every `Call` whose callee resolves to a
   generic `FnDef`, record the concrete type argument tuple. For every `StructLit` / `EnumVariant`
   construction of a generic type, record the concrete argument tuple.

2. **Generate mangled names**: the naming scheme is `{base}__{arg1}__{arg2}...` where each type
   argument is itself mangled. Primitives use short codes:

   | Axon type      | Mangle token |
   |----------------|--------------|
   | `i8`           | `i8`         |
   | `i16`          | `i16`        |
   | `i32`          | `i32`        |
   | `i64`          | `i64`        |
   | `u8`           | `u8`         |
   | `u64`          | `u64`        |
   | `f64`          | `f64`        |
   | `bool`         | `bool`       |
   | `str`          | `str`        |
   | `Pair<i64,f64>`| `Pair__i64__f64` |

3. **Instantiate**: for each unique `(item, arg_tuple)`, deep-copy the item's AST with all type
   parameters substituted by their concrete types, then run codegen on the copy. The LLVM function
   or struct is given the mangled name.

4. **Rewrite call sites**: replace calls to generic functions with calls to the corresponding
   mangled symbol.

### LLVM IR Shape

```llvm
; fn identity<T>(x: T) -> T — instantiated for T = i64
define i64 @identity__i64(i64 %x) {
entry:
  ret i64 %x
}

; type Pair<A, B> = { first: A, second: B } — instantiated for <i64, f64>
; %Pair__i64__f64 = type { i64, double }
```

### Parser Extensions

- `parse_generic_params`: after parsing an `ident` at the start of a top-level item, look ahead for
  `<`. If `<` is followed by `ident` or `ident:` it is a generic parameter list; otherwise it may
  be a comparison expression. The parser uses a depth counter to disambiguate nested `<>`.
- `parse_type_args`: at call sites, `identity::<i64>(x)` parses `::` then `<` type_arg_list `>`.
  Without `::`, type arguments are inferred.

### Code Example

```axon
fn identity<T>(x: T) -> T { x }

type Pair<A, B> = { first: A, second: B }

fn fst<A, B>(p: Pair<A, B>) -> A { p.first }

@[test]
fn test_generics() {
    assert_eq(identity(42), 42)
    let p = Pair { first: 10, second: "hello" }
    assert_eq(fst(p), 10)
}
```

---

## 2. Traits / Interfaces

### Motivation

Generics alone require every caller to know the concrete type. Traits let functions accept *any*
type that satisfies a contract — enabling heterogeneous collections, plugin architectures, and the
AI-first `@[adaptive]` annotations that need runtime dispatch. Phase 3 provides both static
dispatch (monomorphised via trait bounds on generics) and dynamic dispatch (trait objects).

### Syntax

```ebnf
trait_def   ::= "trait" ident generic_params? "{" trait_item* "}"
trait_item  ::= "fn" ident "(" trait_param_list ")" ("->" type)?

impl_block  ::= "impl" ident generic_params? "for" type "{" fn_def* "}"

trait_param_list ::= ("self" | param) ("," param)*

trait_object ::= "dyn" ident
```

**Examples**:
```axon
trait Displayable {
    fn display(self) -> str
}

impl Displayable for Point {
    fn display(self) -> str {
        "({self.x}, {self.y})"
    }
}

fn print_thing(d: dyn Displayable) {
    println(d.display())
}
```

### Semantic Rules

**T01** — A `trait` declaration introduces a named interface. Each item in the body is a method
signature (no default bodies in Phase 3).

**T02** — An `impl Trait for Type` block must implement every method in the trait; missing methods
are a hard error (E0501).

**T03** — A `dyn Trait` type is a fat pointer `{data_ptr, vtable_ptr}` occupying 16 bytes.

**T04** — A concrete type is coerced to `dyn Trait` implicitly at assignment or call sites when the
target type is `dyn Trait` and the concrete type has a matching `impl` block.

**T05** — Static dispatch: when a generic function carries a bound `T: Trait`, calls to trait
methods on `T` are monomorphised — no vtable, no indirection.

**T06** — A trait object call `d.method()` is always a vtable call regardless of the concrete
type known at the call site.

### Vtable Layout

For a trait `Foo` with methods `m1`, `m2`:

```
Vtable layout in memory (a struct of function pointers):
  %vtable_Foo = type { ptr, ptr }
                       ^    ^
                       m1   m2
  ; Each ptr is a pointer to a concrete implementation function.
  ; The first argument of every vtable function is `ptr` (the data pointer).
```

The vtable is a module-level constant emitted once per `impl Trait for Type` pair:

```llvm
@vtable_Displayable_Point = constant %vtable_Displayable {
  ptr @Displayable_display_Point
}
```

A `dyn Trait` value is `{ ptr data, ptr vtable }`:

```llvm
; Coerce p: Point → dyn Displayable
%p_alloca = alloca %Point
store %Point %p_val, ptr %p_alloca
%fat.0 = insertvalue { ptr, ptr } undef, ptr %p_alloca, 0
%fat.1 = insertvalue { ptr, ptr } %fat.0, ptr @vtable_Displayable_Point, 1
; fat.1 is the dyn Displayable value
```

### Vtable Call Emission

For `d.display()` where `d: dyn Displayable`:

```llvm
%data_ptr  = extractvalue { ptr, ptr } %d, 0
%vtbl_ptr  = extractvalue { ptr, ptr } %d, 1
; Load function pointer at vtable slot 0 (display is method 0)
%fn_ptr    = load ptr, ptr %vtbl_ptr
%result    = call { i64, ptr }(ptr) %fn_ptr(ptr %data_ptr)
```

The concrete implementation function signature always takes `ptr` as its first argument (the
`self` data pointer) and loads the concrete struct from it.

### Code Example

```axon
trait Displayable {
    fn display(self) -> str
}

type Point = { x: f64, y: f64 }
type Circle = { radius: f64 }

impl Displayable for Point {
    fn display(self) -> str {
        "point"
    }
}

impl Displayable for Circle {
    fn display(self) -> str {
        "circle"
    }
}

fn show(d: dyn Displayable) {
    println(d.display())
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 }
    let c = Circle { radius: 5.0 }
    show(p)    // prints: point
    show(c)    // prints: circle
}
```

---

## 3. Closures with Captures

### Motivation

Phase 2 lambdas only captured by-copy through hidden extra parameters — a leaky abstraction that
broke for mutable captures and prevented first-class closure values from being stored in structs or
returned from functions. Phase 3 closes this gap: every lambda that references an outer binding
gets a heap-allocated environment struct, and the closure value is a fat pointer that bundles the
function pointer with the environment.

### Syntax

No new surface syntax — the existing lambda expression is unchanged:

```ebnf
lambda ::= "(" param_list ")" "=>" expr
         | ident "=>" expr          (* single-param shorthand *)
```

The distinction from Phase 2 is entirely in how captures are handled by the compiler.

### Semantic Rules

**C01** — A lambda *captures* a binding if the binding is defined in an enclosing lexical scope and
is referenced inside the lambda body. Parameters of the lambda itself are not captures.

**C02** — All captures are by-move in Phase 3. After a closure is constructed, the compiler
treats the moved-in variables as moved in the enclosing scope (subject to borrow check §5).

**C03** — A closure value has type `fn(P0, P1, ...) -> R` in the Axon type system; the hidden
environment is transparent at the source level. The LLVM representation is `{ ptr fn_ptr, ptr env_ptr }`.

**C04** — A closure that captures nothing is represented identically to a plain function pointer
(`env_ptr` is null).

**C05** — Closures may be stored in `let` bindings, passed as arguments, and returned from
functions. They may not outlive the `fn` they were created in unless all captured values are
`own`-bound (stack-allocated in Phase 3).

### Closure Layout

```
Closure fat pointer (16 bytes on 64-bit):
  { ptr fn_ptr, ptr env_ptr }

Environment struct (heap-allocated, one per closure creation):
  %env_<lambda_N> = type { <captured_type_0>, <captured_type_1>, ... }
```

The lifted LLVM function receives the environment pointer as an additional first argument:

```llvm
; Source: let add = (b) => a + b   (captures: a: i64)
; Lifted function:
define i64 @__lambda_3(ptr %env, i64 %b) {
entry:
  %a_ptr = getelementptr %env_lambda_3, ptr %env, i32 0, i32 0
  %a     = load i64, ptr %a_ptr
  %sum   = add i64 %a, %b
  ret i64 %sum
}
```

### Capture List Determination

The resolver computes the capture list during a pre-codegen walk:

1. For each `Lambda` node, open a new capture scope.
2. Walk the lambda body. For every `Ident(name)`:
   - If `name` is a lambda parameter, skip.
   - If `name` resolves to a binding in an enclosing function scope (not module-level), add it to
     the capture set.
3. Deduplicate. The capture set is stored on the `Lambda` AST node as
   `captures: Vec<(String, Type)>`.

### Environment Allocation and Passing

At the closure *creation* site:

```llvm
; allocate env on heap
%env_raw = call ptr @malloc(i64 <sizeof env_lambda_N>)
; store each captured value
%a_ptr = getelementptr %env_lambda_3, ptr %env_raw, i32 0, i32 0
store i64 %a_val, ptr %a_ptr
; build fat pointer
%closure.0 = insertvalue { ptr, ptr } undef, ptr @__lambda_3, 0
%closure.1 = insertvalue { ptr, ptr } %closure.0, ptr %env_raw, 1
```

At each closure *call* site:

```llvm
%fn_ptr  = extractvalue { ptr, ptr } %f, 0
%env_ptr = extractvalue { ptr, ptr } %f, 1
%result  = call i64(ptr, i64) %fn_ptr(ptr %env_ptr, i64 %arg0)
```

### Code Example

```axon
fn make_adder(n: i64) -> fn(i64) -> i64 {
    (x) => x + n     // captures n by move
}

fn apply(f: fn(i64) -> i64, v: i64) -> i64 {
    f(v)
}

@[test]
fn test_closure() {
    let add5 = make_adder(5)
    assert_eq(apply(add5, 3), 8)
    assert_eq(apply(add5, 10), 15)
}
```

---

## 4. Channels (`chan<T>`)

### Motivation

Axon's AI-first design assumes concurrent agents that communicate through typed message passing.
Shared mutable state is a correctness hazard; channels provide the safe alternative. Phase 3
introduces `chan<T>`, `spawn`, and `select` as first-class language constructs backed by a thin
runtime layer. The runtime is deliberately minimal: POSIX mutex + condition variable under the
hood, growable ring buffer.

### Syntax

```ebnf
chan_expr   ::= "chan" "<" type ">" "(" ")"
spawn_expr  ::= "spawn" block
select_expr ::= "select" "{" select_arm+ "}"
select_arm  ::= expr "=>" expr

send_call   ::= expr "." "send" "(" expr ")"
recv_call   ::= expr "." "recv" "(" ")"
```

**Examples**:
```axon
let ch = chan<i64>()
spawn { ch.send(42) }
let v = ch.recv()

select {
    ch1.recv() => println("got from ch1")
    ch2.recv() => println("got from ch2")
}
```

### Semantic Rules

**CH01** — `chan<T>()` allocates a new unbuffered channel carrying values of type `T`. The
resulting value has type `chan<T>`.

**CH02** — `ch.send(v)` blocks the calling thread until a receiver is ready. `v` must have type
`T` where `ch: chan<T>`. After `send`, `v` is moved and no longer accessible (borrow check §5).

**CH03** — `ch.recv()` blocks until a sender is ready and returns the sent value with type `T`.

**CH04** — `spawn { body }` launches `body` in a new OS thread. The block may capture variables
from the enclosing scope; captures follow the same rules as closures (§3), all by-move.

**CH05** — `select { arm+ }` evaluates all arms concurrently. The first arm whose channel becomes
ready wins; its body executes. All other arms are cancelled. Arms must be `recv()` calls in Phase
3 (no `send` arms in `select` until Phase 4).

**CH06** — A `chan<T>` value is not `Copy`; it may be cloned with `ch.clone()` which increments
an internal reference count.

### Runtime API

The Axon runtime exposes these symbols (C ABI, linked into every binary):

```c
/* Create a new channel. elem_size is sizeof(T) in bytes. */
void* __axon_chan_new(size_t elem_size);

/* Send a copy of the value pointed to by val into ch.
   Blocks until a receiver is ready. */
void  __axon_chan_send(void* ch, void* val);

/* Receive one value from ch into the buffer pointed to by dst.
   Blocks until a sender is ready. */
void  __axon_chan_recv(void* ch, void* dst);

/* Clone channel handle (increment refcount). */
void* __axon_chan_clone(void* ch);

/* Drop channel handle (decrement refcount; free when zero). */
void  __axon_chan_drop(void* ch);

/* Launch a new OS thread. fn_ptr receives env_ptr as its only argument. */
void  __axon_spawn(void* fn_ptr, void* env_ptr);

/* select: try all channels non-blocking; block until one is ready.
   ch_ptrs is an array of channel pointers (len = n).
   dst_ptrs is an array of destination buffers (one per channel).
   Returns the index of the channel that became ready. */
size_t __axon_select(void** ch_ptrs, void** dst_ptrs, size_t n);
```

### LLVM IR Lowering

#### `chan<T>` type

`chan<T>` lowers to `ptr` — an opaque handle. `sizeof(chan<T>)` is 8 bytes (pointer).

#### `chan<i64>()` creation

```llvm
%ch = call ptr @__axon_chan_new(i64 8)   ; elem_size = sizeof(i64) = 8
```

#### `ch.send(v)`

```llvm
%v_alloca = alloca i64
store i64 %v, ptr %v_alloca
call void @__axon_chan_send(ptr %ch, ptr %v_alloca)
```

#### `ch.recv()`

```llvm
%dst = alloca i64
call void @__axon_chan_recv(ptr %ch, ptr %dst)
%val = load i64, ptr %dst
```

#### `spawn { body }`

The body is lifted into a function using the same mechanism as closures (§3). The spawned function
has signature `void(ptr env)`:

```llvm
; spawn { ch.send(42) }  — captures ch
define void @__spawn_1(ptr %env) {
entry:
  %ch_ptr = getelementptr %env_spawn_1, ptr %env, i32 0, i32 0
  %ch     = load ptr, ptr %ch_ptr
  %v      = alloca i64
  store i64 42, ptr %v
  call void @__axon_chan_send(ptr %ch, ptr %v)
  ret void
}
; at spawn site:
%env = call ptr @malloc(i64 8)
store ptr %ch_val, ptr %env
call void @__axon_spawn(ptr @__spawn_1, ptr %env)
```

#### `select`

```llvm
; select { ch1.recv() => body1, ch2.recv() => body2 }
%chs    = alloca [2 x ptr]
%dsts   = alloca [2 x ptr]
%d0     = alloca i64
%d1     = alloca i64
; fill arrays
store ptr %ch1, ptr (getelementptr [2 x ptr], ptr %chs, i32 0, i32 0)
store ptr %ch2, ptr (getelementptr [2 x ptr], ptr %chs, i32 0, i32 1)
store ptr %d0,  ptr (getelementptr [2 x ptr], ptr %dsts, i32 0, i32 0)
store ptr %d1,  ptr (getelementptr [2 x ptr], ptr %dsts, i32 0, i32 1)
%ready  = call i64 @__axon_select(ptr %chs, ptr %dsts, i64 2)
; switch on %ready
switch i64 %ready, label %default [
  i64 0, label %arm0
  i64 1, label %arm1
]
arm0:
  %v0 = load i64, ptr %d0
  ; emit body1 with v0 bound
  br %merge
arm1:
  %v1 = load i64, ptr %d1
  ; emit body2 with v1 bound
  br %merge
```

### Code Example

```axon
fn producer(ch: chan<i64>) {
    let i = 0
    ch.send(i)
    ch.send(i + 1)
    ch.send(i + 2)
}

fn main() {
    let ch = chan<i64>()
    let ch2 = ch.clone()
    spawn { producer(ch2) }
    let a = ch.recv()
    let b = ch.recv()
    let c = ch.recv()
    assert_eq(a + b + c, 3)
}
```

---

## 5. Borrow Checker (Phase 3 Lite)

### Motivation

Without ownership tracking Axon cannot safely lower to native code: use-after-free, double-free,
and data races are undefined behaviour that silently corrupt programs. Phase 3 introduces a
*lexical* borrow checker — no lifetime parameters, no region inference, just the single-ownership
rule enforced within function bodies. This catches the common class of bugs (use after move, move
out of borrowed place) at zero runtime cost and sets the foundation for the full lifetime system
in Phase 4.

### Syntax — Ownership Annotations

```ebnf
let_stmt  ::= "let"  ident (":" type)? "=" expr     (* immutable reference binding *)
own_stmt  ::= "own"  ident (":" type)? "=" expr     (* owned, movable binding *)
ref_stmt  ::= "ref"  ident (":" type)? "=" expr     (* borrowed reference, not movable *)

param     ::= ident ":" type
            | "own" ident ":" type
            | "ref" ident ":" type
```

The `Expr::Own` and `Expr::RefBind` AST nodes already exist (see `ast.rs`).

### Semantic Rules

**B01 — Single Ownership**: every value has exactly one owner at any point in time.

**B02 — Move on Assignment**: binding a value to a new name transfers ownership. The original
binding becomes *moved* and may not be read or passed after the move point.

```axon
own a = Point { x: 1.0, y: 2.0 }
own b = a           // a is moved into b
println(a.x)        // ERROR E0601: use of moved value 'a'
```

**B03 — Move on Call**: passing an `own` binding as an argument moves it into the callee.

**B04 — `ref` bindings**: a `ref` binding borrows a value without taking ownership. The original
owner may not be moved while any `ref` to it is live.

**B05 — Lexical scope**: borrow lifetimes end at the closing `}` of the block where the `ref` was
created. Borrows do not extend across function calls in Phase 3 (i.e., no borrow in returned
position).

**B06 — `let` (immutable) bindings**: for backward compatibility, `let` is treated as `own` by
the borrow checker. Phase 3 does not require source-level `own`/`ref` annotations on all
bindings — unannotated `let` is assumed `own`. Explicit `ref` is required to suppress the move.

**B07 — Copy types**: the primitive types `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`,
`f32`, `f64`, `bool` are implicitly `Copy`: assignment copies the value instead of moving it.
`str`, structs, enums, closures, and `chan<T>` are *move* types.

**B08 — Channel sends are moves**: `ch.send(v)` moves `v`; `v` is inaccessible after the call.

### Ownership Graph Data Structure

The borrow checker maintains a per-function `OwnershipGraph`:

```rust
struct OwnershipGraph {
    /// Current status of every named binding in scope.
    bindings: HashMap<String, BindingState>,
    /// Stack of lexical scopes (each scope is a set of binding names introduced in it).
    scopes: Vec<Vec<String>>,
}

enum BindingState {
    Owned,          // live, owned
    Moved,          // moved out — access is an error
    Borrowed,       // currently borrowed (ref exists)
    Ref(String),    // this is a ref to the named owned binding
}
```

**Algorithm** (single forward pass over statements in a block):

1. On `Own { name, value }` / `Let { name, value }`: evaluate `value`; if value is an `Ident(src)`
   and `src` is a move type, set `bindings[src] = Moved`. Set `bindings[name] = Owned`. Push `name`
   onto the current scope.

2. On `RefBind { name, value }`: evaluate `value`; `value` must be `Ident(src)` with state
   `Owned`. Set `bindings[src] = Borrowed`. Set `bindings[name] = Ref(src)`. Push `name` onto
   current scope.

3. On `Ident(name)` as an r-value in a move position (argument to a call, rhs of `own`): if
   `bindings[name]` is `Moved`, emit E0601. If `Borrowed`, emit E0602 (cannot move borrowed
   value).

4. On block exit: for each `name` in the scope's list, if `bindings[name]` is `Ref(src)`,
   restore `bindings[src]` to `Owned` (borrow ends).

5. After an `if/else` or `match`: compute the union of moved-out bindings across all branches. A
   binding moved in *any* branch is considered moved after the construct.

### LLVM IR Impact

The borrow checker is a pure analysis pass that runs between type-checking and codegen. It does
not change the emitted IR. Its sole output is the set of E06xx diagnostics. Codegen proceeds
unchanged on success.

### Code Example

```axon
type Buffer = { data: str, len: i64 }

fn consume(b: Buffer) {
    println(b.data)
}

fn main() {
    own buf = Buffer { data: "hello", len: 5 }
    consume(buf)
    // consume(buf)  // would be E0601: use of moved value 'buf'

    own s = "world"
    ref r = s
    println(r)      // ok: reading through ref
    // own t = s    // would be E0602: cannot move borrowed value 's'
}
```

---

## 6. `comptime` Expressions

### Motivation

Many values used in Axon programs are fully determined at compile time: array lengths, buffer
sizes, mathematical constants, table capacities. Without `comptime`, these must either be
hardcoded literals (brittle) or computed at runtime (wasteful). `comptime` lets authors express
intent — "evaluate this now" — and gives the compiler a mandate to fold it to a constant and
report an error if it cannot.

### Syntax

```ebnf
comptime_expr ::= "comptime" "{" expr "}"
                | "comptime" expr       (* expression form, lower precedence than call *)
```

**Examples**:
```axon
let buf_size = comptime { 4 * 1024 }
let mask     = comptime { buf_size - 1 }
let greeting = comptime { "hello, " + "world" }
```

### Comptime-Evaluable Expressions

An expression is *comptime-evaluable* if and only if:

1. It is a literal (`Literal::Int`, `Literal::Float`, `Literal::Bool`, `Literal::Str`).
2. It is a `BinOp` where both operands are comptime-evaluable and the operator is one of
   `Add`, `Sub`, `Mul`, `Div` (integer or float), `And`, `Or`, `Eq`, `NotEq`, `Lt`, `Gt`,
   `LtEq`, `GtEq`.
3. It is a `UnaryOp { Neg | Not }` on a comptime-evaluable operand.
4. It is an `Ident` that refers to a binding whose initialiser was itself a `comptime` expression.
5. It is a `Call` to a function that is:
   - declared `fn` (not `spawn`, not a closure),
   - has no side effects (no `print`, `println`, `eprintln`, no `chan` operations, no I/O
     builtins),
   - has all arguments comptime-evaluable.
   The compiler evaluates such calls by interpreting the typed AST (tree-walking evaluator, no
   LLVM involved).

Anything not in the above list inside a `comptime` block is a hard error E0701.

### Evaluator

The comptime evaluator is a tree-walking interpreter over the typed AST:

```rust
enum ComptimeVal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

fn eval_comptime(expr: &Expr, env: &HashMap<String, ComptimeVal>) -> Result<ComptimeVal, CompileError>;
```

Rules:
- `Literal::Int(n)` → `ComptimeVal::Int(n)`
- `BinOp { Add, Int(a), Int(b) }` → `Int(a + b)`
- `BinOp { Add, Str(a), Str(b) }` → `Str(a + &b)` (string concatenation)
- `Ident(name)` → look up in `env`; error if not present
- Integer division by zero at comptime is E0702.
- Overflow (e.g. `i64::MAX + 1`) is E0703 (wrapping not implied).

Successful evaluation stores the result in a module-level comptime table keyed by binding name.
Any later `Ident` reference to a comptime binding resolves directly from this table.

### LLVM IR Lowering

A `comptime` expression that evaluates to an integer lowers to an LLVM integer constant:

```llvm
; let buf_size = comptime { 4 * 1024 }
; → no alloca, no store; buf_size is the i64 constant 4096
%result = add i64 0, 4096   ; optimizer folds to: ret i64 4096
```

In practice, the codegen substitutes the `ComptimeVal` directly wherever the binding is used —
never emitting a load. For string comptime values, the string is emitted as a global constant and
referenced by pointer.

### Code Example

```axon
let PAGE  = comptime { 4096 }
let PAGES = comptime { 16 }
let BUF   = comptime { PAGE * PAGES }

fn alloc_buf() -> [u8] {
    // BUF is 65536 — a compile-time constant
    let arr: [u8] = [0; BUF]
    arr
}

fn pow2(n: i64) -> i64 { n * n }
let AREA = comptime { pow2(8) }   // 64 — pure function, comptime-callable

@[test]
fn test_comptime() {
    assert_eq(BUF, 65536)
    assert_eq(AREA, 64)
}
```

---

## 7. Error Propagation Improvements — Span-Threaded Diagnostics

### Motivation

Phase 2 promised line/column tracking but deferred the implementation. Every error message in
Phase 1 and Phase 2 identifies the problem at best by function name; at worst it gives no location
at all. This is unacceptable for a language that targets AI-driven development workflows where
diagnostic quality directly determines how quickly an LLM can propose a fix. Phase 3 adds `Span`
to every AST node and threads it through the entire pipeline.

### Span Type

```rust
/// Byte-offset span into the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Span {
    /// Byte offset of the first character of the node.
    pub start: usize,
    /// Byte offset one past the last character of the node.
    pub end: usize,
}

impl Span {
    pub fn dummy() -> Self { Span { start: 0, end: 0 } }
    pub fn merge(a: Span, b: Span) -> Self {
        Span { start: a.start.min(b.start), end: a.end.max(b.end) }
    }
}
```

A `SourceMap` converts byte offsets to `(line, col)` pairs:

```rust
struct SourceMap {
    /// Byte offset of the start of each line (line_starts[0] = 0).
    line_starts: Vec<usize>,
    source: String,
}

impl SourceMap {
    fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = self.line_starts.partition_point(|&s| s <= offset) - 1;
        let col  = offset - self.line_starts[line];
        (line + 1, col + 1)   // 1-indexed
    }
}
```

### AST Changes

Every AST node that can appear in a diagnostic gains a `span` field. The minimal required set:

| Node | Span covers |
|------|-------------|
| `FnDef` | `fn` keyword through closing `}` |
| `Param` | name token |
| `TypeDef` | `type` keyword through closing `}` |
| `EnumDef` | `enum` keyword through closing `}` |
| `Expr::Let` | `let` keyword through end of value expr |
| `Expr::Call` | callee through closing `)` |
| `Expr::BinOp` | left operand through right operand |
| `Expr::Ident` | the identifier token |
| `Expr::Literal` | the literal token |
| `Expr::FieldAccess` | receiver through field name |
| `Expr::Index` | receiver through `]` |
| `Expr::If` | `if` through end of else branch |
| `Expr::Match` | `match` through closing `}` |
| `MatchArm` | pattern through end of body |
| `Pattern` (all variants) | full pattern text |

**Implementation approach**: add `span: Span` as the last field of each node struct. The parser
records the start byte offset before consuming the first token of a node and the end offset after
consuming the last token. Phase 3 adds `current_pos: usize` to the lexer (already has token
position in most implementations; expose it).

Backward compatibility: nodes not yet span-annotated carry `Span::dummy()`. The diagnostic
renderer skips the caret line for dummy spans.

### Diagnostic Format

All diagnostic messages emitted in Phase 3 and later must include the `file:line:col` prefix in
the form `file.ax:14:7`. The renderer always outputs this prefix even when the source file was
given as a relative path; the path is normalised to absolute form in the `SourceMap` at parse
time.

```
error[E0401]: struct 'Point' has no field 'z'
  --> main.ax:14:7
   |
14 │   let v = p.z
   │           ^^^ 'Point' fields: x, y
   |
   = help: did you mean 'x'?
```

Renderer algorithm:
1. Convert `span.start` to `(line, col)` via `SourceMap`.
2. Extract the source line text.
3. Print `{line} │ {source_line}`.
4. Print padding + `^` repeated `span.end - span.start` times, followed by the message.

For multi-line spans (start and end on different lines), print the first line with `^` to the end
and `...` continuation.

### Pipeline Integration

Spans thread through the pipeline as follows:

```
[1] Lexer      — records byte offset on every Token
[2] Parser     — reads Token offsets; sets Span on each AST node
[3] Resolver   — passes Span through; resolution errors carry span from Ident node
[4] Infer      — carries span from Expr; unification errors carry span from both sides
[5] Checker    — all E04xx rules now include span in error
[6] Borrow     — E06xx errors carry span from the offending Ident node
[7] Codegen    — attaches LLVM debug locations via DILocation (uses span → line/col)
```

The `CompileError` type gains a mandatory `span: Span` field in Phase 3:

```rust
pub struct CompileError {
    pub code:    &'static str,
    pub message: String,
    pub span:    Span,
    pub hints:   Vec<String>,
}
```

All existing error construction sites must supply a span. Sites that lack span information in
Phase 3 use `Span::dummy()` and file a tracking issue.

### Code Example — Diagnostic Output

Given:
```axon
// main.ax
type Point = { x: f64, y: f64 }

fn main() {
    let p = Point { x: 1.0, y: 2.0 }
    println(p.z)
}
```

Phase 3 emits:
```
error[E0401]: struct 'Point' has no field 'z'
  --> main.ax:5:13
   |
 5 │     println(p.z)
   │             ^^^ 'Point' fields: x, y
   |
   = help: did you mean 'x' or 'y'?
```

---

## Pipeline Changes (Phase 3)

```
main.ax
  │
  ▼
[1] Lexer           token.rs    (extended: byte-offset on every token)
  │
  ▼
[2] Parser          parser.rs   (extended: <T> generic params, dyn Trait, comptime,
  │                              spawn, select, chan<T>; Span on all nodes)
  ▼
[3] Name Resolution resolver.rs  (extended: trait defs, impl blocks, type params,
  │                              comptime binding table)
  ▼
[4] Type Inference  infer.rs    (extended: generic unification, trait bound checking,
  │                              closure capture type inference, chan<T> type)
  ▼
[5] Type Check      checker.rs  (extended: all rules carry Span; new trait rules)
  │
  ▼
[6] Comptime Eval   comptime.rs (NEW: tree-walking evaluator, ComptimeVal table)
  │
  ▼
[7] Borrow Check    borrow.rs   (NEW: OwnershipGraph forward pass)
  │
  ▼
[8] Monomorphize    mono.rs     (NEW: collect instantiations, mangle names, clone AST)
  │
  ▼
[9] IR Emission     codegen.rs  (extended: vtable emission, closure env alloc,
  │                              chan runtime calls, spawn lifting, comptime constants,
  │                              DILocation for spans)
  ▼
[10] LLVM Passes               (unchanged)
  │
  ▼
[11] Link runtime   axon-rt/    (NEW: libaxon_rt.a with __axon_chan_*, __axon_spawn,
  │                              __axon_select)
  ▼
[12] Native Binary             (unchanged)
```

---

## New Error Codes (Phase 3)

```
E0501  trait '{trait}' method '{method}' not implemented for '{type}'
E0502  impl block for '{type}' does not implement trait '{trait}': missing '{method}'
E0503  dyn '{trait}' cannot be used as a value type (must be behind ptr or in fn param)
E0504  trait bound not satisfied: '{type}' does not implement '{trait}'

E0601  use of moved value '{name}'
E0602  cannot move '{name}': value is currently borrowed
E0603  '{name}' borrowed here, move attempted at {span}

E0701  expression in comptime block is not comptime-evaluable: {reason}
E0702  comptime integer division by zero
E0703  comptime integer overflow: {expr} overflows i64

E0801  generic instantiation depth limit (64) exceeded for '{fn}'
E0802  cannot infer type argument '{T}' for '{fn}' — annotate the call site
E0803  type argument '{ty}' does not satisfy bound '{T}: {trait}'
```

---

## New Builtins (Phase 3)

These are added to `BUILTINS` in `builtins.rs`:

```
sqrt(n: f64) -> f64           — calls C sqrt; not comptime-evaluable
pow(base: f64, exp: f64) -> f64  — calls C pow
floor(n: f64) -> f64          — calls C floor
ceil(n: f64)  -> f64          — calls C ceil
assert_eq_str(a: str, b: str) — panics with diff if a != b (Phase 3 version)
chan_clone(ch: chan<T>) -> chan<T>  — clone via __axon_chan_clone (generic)
```

---

## Verification Checklist

Phase 3 is done when the following programs compile and produce the expected output:

### generics.ax
```axon
fn identity<T>(x: T) -> T { x }
type Pair<A, B> = { first: A, second: B }
fn main() {
    assert_eq(identity(99), 99)
    let p = Pair { first: 1, second: 2 }
    assert_eq(p.first, 1)
}
```
Expected: passes all assertions.

### traits.ax
```axon
trait Greet { fn greet(self) -> str }
type Dog = { name: str }
type Cat = { name: str }
impl Greet for Dog { fn greet(self) -> str { "woof" } }
impl Greet for Cat { fn greet(self) -> str { "meow" } }
fn announce(g: dyn Greet) { println(g.greet()) }
fn main() {
    announce(Dog { name: "Rex" })
    announce(Cat { name: "Luna" })
}
```
Expected: prints `woof` then `meow`.

### closures.ax
```axon
fn make_counter(start: i64) -> fn() -> i64 {
    let n = start
    () => { let v = n; n = n + 1; v }
}
fn main() {
    let c = make_counter(0)
    assert_eq(c(), 0)
    assert_eq(c(), 1)
    assert_eq(c(), 2)
}
```
Expected: passes all assertions.

### channels.ax
```axon
fn main() {
    let ch = chan<i64>()
    let ch2 = ch.clone()
    spawn { ch2.send(7) }
    let v = ch.recv()
    assert_eq(v, 7)
}
```
Expected: passes assertion.

### borrow.ax
```axon
type Buf = { data: str }
fn consume(b: Buf) { println(b.data) }
fn main() {
    own b = Buf { data: "ok" }
    consume(b)
    // consume(b)   // uncomment → E0601
}
```
Expected: prints `ok`; uncommented second call fails to compile with E0601.

### comptime.ax
```axon
let KB = comptime { 1024 }
let MB = comptime { KB * 1024 }
fn main() { assert_eq(MB, 1048576) }
```
Expected: passes assertion; `MB` appears as constant `1048576` in the LLVM IR.

### spans.ax
```axon
type Point = { x: f64 }
fn main() {
    let p = Point { x: 1.0 }
    println(p.y)    // triggers E0401 with line:col
}
```
Expected: compile error E0401 with `  --> spans.ax:4:13`.

---

## Dependencies (Phase 3 Additions)

One new crate is needed in `axon-rt/` (the Axon runtime static library):

```toml
# axon-rt/Cargo.toml  (compiled to a C-compatible staticlib)
[lib]
crate-type = ["staticlib"]
```

The runtime is written in Rust and exposes the `__axon_chan_*` / `__axon_spawn` / `__axon_select`
symbols. It depends on `libc` (already in the tree) and uses `std::sync::{Mutex, Condvar}` for
channel synchronisation. No new external C dependencies are introduced.

The `inkwell` version remains 0.4; no new LLVM features are required. LLVM debug info (`DIBuilder`,
`DILocation`) needed for span-to-line-col in generated code is available in inkwell 0.4.

---

## Known Gaps to Address in Phase 3 Implementation

The following issues are known at the time of this spec and must be resolved during Phase 3
implementation. They are listed here so that no item falls through the cracks when planning
the work.

### Module / import system currently a no-op

The parser accepts `mod foo` and `use foo.{bar, baz}` declarations and stores them in the AST,
but the resolver performs no file I/O: it does not attempt to load `foo.ax` from disk, does not
merge the remote module's symbol table, and silently ignores unresolved `use` paths. Phase 3 must
implement actual multi-file loading. The search algorithm is defined in `spec/compiler-phase4.md`
§6 (AXON_PATH). Phase 3 should implement at minimum a synchronous single-pass loader that reads
each depended-upon file, parses it, and adds its top-level items to the resolver's symbol table
before resolving the root file.

### Standard library location undefined

Until Phase 3 implements file loading, `use std::io` cannot work. Phase 3 must adopt the
`AXON_PATH` algorithm (`spec/compiler-phase4.md` §6) so that at least a minimal `std/io.ax`
can be found and loaded. The fallback path `~/.axon/lib/` must be tried when `AXON_PATH` is
not set.

### `assert_eq` generics

Phase 2 locks `assert_eq` to `i64, i64`. Two options for Phase 3:

**Option A** — Make `assert_eq` generic using the new generics system, requiring a `T: Eq`
trait bound. This is the cleanest long-term solution but requires the trait system (§2) to be
complete first.

**Option B** — Add type-specific variants `assert_eq_str(a: str, b: str)` and
`assert_eq_f64(a: f64, b: f64)` as explicit builtins, without generics. These can be
implemented earlier in the Phase 3 timeline before traits are stable.

The Phase 3 implementation must choose one path explicitly and document the decision in
`builtins.rs`. Whichever option is deferred must be tracked as a follow-up item.

### String ownership: `axon_concat` leaks memory

`axon_concat` allocates a new `malloc` buffer for every concatenation and never frees it.
String interpolation `"hello {name}"` generates a chain of `axon_concat` calls, leaking all
intermediate buffers. In short-lived programs this is tolerable; in long-running servers or
agents (which are the primary Phase 3 target via channels) it is a correctness bug.

Phase 3 must define string ownership semantics. Viable approaches:

- **Arena allocation**: allocate all strings from a per-request or per-function arena, freed in
  bulk at a defined scope boundary. Simple to implement; no per-string bookkeeping.
- **Explicit free via `axon_str_drop`**: the borrow checker inserts `axon_str_drop` calls at
  the end of the owning binding's scope. Correct but requires the borrow checker (§5) to be
  aware of `str` as a move type.
- **Reference counting**: wrap str in a ref-counted header. Adds 8 bytes per string and two
  atomic operations per assignment. Most general; highest overhead.

The chosen approach must be documented in `spec/runtime.md` under "Memory management" before
Phase 3 codegen begins.

### Test runner: parallel execution and output format

The Phase 1/2 test runner is sequential and prints a human-readable summary only. Phase 3 must
lay the groundwork for the Phase 4 parallel runner (`--jobs N`) and JSON output (`--json`).
Specifically, Phase 3 must:

1. Run each `@[test]` in a subprocess (not inline in the same process), so that
   `@[test(should_fail)]` tests that call `exit(1)` do not terminate the runner.
2. Collect subprocess exit codes and map them to pass/fail.
3. Accept a `--filter=<name>` flag to run a subset of tests.

TAP (Test Anything Protocol) output is optional in Phase 3 but the subprocess-per-test
architecture is required as a prerequisite for Phase 4 parallel execution.
