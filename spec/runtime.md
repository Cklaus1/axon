# Axon Runtime ABI Specification

**Version**: Phase 1 (current implementation)  
**Compiler**: `crates/axon-core/src/codegen.rs`  
**Builtins table**: `crates/axon-core/src/builtins.rs`

This document is the authoritative reference for every ABI contract that sits between Axon-generated
LLVM IR and the C runtime environment. If this document disagrees with `codegen.rs`, the source
code wins; file a bug and update this document.

---

## 1. String Representation (`str`)

### Memory Layout

```
{ i64 len, ptr data }
```

In LLVM IR:

```llvm
%str = type { i64, ptr }
;               ^    ^
;               |    i8* — pointer to the first byte of the string data
;               byte count (not including the null terminator)
```

Field indices in GEP / `extractvalue`:

| Index | Type | Meaning |
|-------|------|---------|
| 0 | `i64` | Byte length of the string content |
| 1 | `ptr` (opaque, was `i8*`) | Pointer to the raw string bytes |

### Null termination

String data is **always null-terminated**. The null byte is not counted in `len`. This invariant
holds for both string literals (emitted as LLVM global constants) and heap-allocated strings
returned by `axon_concat`, `to_str`, and `to_str_f64`. Consumers that call C functions expecting
`const char*` (e.g. `puts`, `printf`) may use `data` directly without additional copying.

### Heap ownership

| Source | Allocation | Who frees |
|--------|------------|-----------|
| String literal | Compile-time global constant (`.rodata` / module global) | Never freed — static lifetime |
| `axon_concat` return value | `malloc`-allocated heap buffer | Caller is responsible (no GC in Phase 1) |
| `to_str` return value | Module-level static 32-byte buffer | **Not heap** — see note below |
| `to_str_f64` return value | Module-level static 32-byte buffer | **Not heap** — see note below |

> **Phase 1 thread-safety caveat**: `to_str` and `to_str_f64` write into module-level static
> buffers (`to_str_buf`, `to_str_f64_buf`). The returned `str` struct points into that buffer.
> Consecutive calls overwrite the buffer, so callers must consume or copy the value before making
> a second call. These functions are **not thread-safe**.

---

## 2. `Result<T, E>` Layout

### Canonical layout

```
{ i1 tag, [N x i8] payload }
```

In LLVM IR:

```llvm
%Result_T_E = type { i1, [N x i8] }
;                    ^    ^^^^^^^^^
;                    |    union payload — N = max(sizeof T, sizeof E), minimum 1
;                    discriminant
```

`N` is computed as:

```
N = max(sizeof(T), sizeof(E))
N = max(N, 1)          -- LLVM requires at least one byte
```

where `sizeof` follows the heuristic table in `llvm_sizeof` (`codegen.rs`):

| Axon type | Bytes |
|-----------|-------|
| `i8`, `u8`, `bool` | 1 |
| `i16`, `u16` | 2 |
| `i32`, `u32`, `f32` | 4 |
| `i64`, `u64`, `f64` | 8 |
| `str`, `[T]` (slice) | 16 (`{ i64, ptr }`) |
| `Option<T>` | 1 + sizeof(T) |
| `Tuple(...)` | sum of field sizes |
| `Struct`, `Enum` | 8 (conservative estimate) |
| `()` (Unit) | 0 |

### Tag convention

| `tag` value | Meaning |
|-------------|---------|
| `0` (false) | `Err(E)` — the payload contains the error value |
| `1` (true) | `Ok(T)` — the payload contains the success value |

This matches the `is_ok` flag passed to `emit_result` in `codegen.rs`:

```rust
// codegen.rs, emit_result:
let tag_val = tag_ty.const_int(if is_ok { 1 } else { 0 }, false);
```

And the pattern tests:

```rust
// Pattern::Ok  — checks tag == 1
// Pattern::Err — checks tag == 0
```

### Payload storage and extraction

Both `Ok` and `Err` arms store the payload into the `[N x i8]` array via an opaque pointer cast
and a plain `store`. Extraction is symmetric: alloca the array, `store` the array value, cast the
alloca pointer to the desired typed pointer, then `load`. This is done in `extract_result_payload`.

Example for `Result<i64, str>` (`N = max(8, 16) = 16`):

```llvm
%Result_i64_str = type { i1, [16 x i8] }

; Construct Ok(42)
%r = alloca %Result_i64_str
%tagptr  = getelementptr %Result_i64_str, ptr %r, i32 0, i32 0
%payptr  = getelementptr %Result_i64_str, ptr %r, i32 0, i32 1
store i1 true, ptr %tagptr
store i64 42, ptr %payptr          ; writes 8 bytes into the 16-byte payload

; Extract the i64 from an Ok arm
%arr     = extractvalue %Result_i64_str %result_val, 1   ; [16 x i8]
%arrtmp  = alloca [16 x i8]
store [16 x i8] %arr, ptr %arrtmp
%val     = load i64, ptr %arrtmp   ; reinterprets first 8 bytes as i64
```

### The `?` operator

The `?` (early-return-on-Err) operator checks `tag == 1`. If the tag is `0` (Err), the entire
`Result` struct is returned from the enclosing function unchanged. If the tag is `1` (Ok), the
raw payload (`[N x i8]` value) is extracted and control continues. The calling convention for
functions that use `?` requires that the outer function also returns `Result<T, E>` with a
compatible payload size.

---

## 3. Slice / Array Layout (`[T]`)

### Memory layout

```
{ i64 len, ptr data }
```

In LLVM IR:

```llvm
%slice_T = type { i64, ptr }
;                  ^    ^
;                  |    pointer to the first element (typed as ptr to T in GEP)
;                  element count
```

This is structurally identical to `str` (both are 16 bytes on 64-bit targets). The difference is
semantic: `str` carries bytes, slices carry elements of type `T`.

Field indices:

| Index | Type | Meaning |
|-------|------|---------|
| 0 | `i64` | Number of elements (not bytes) |
| 1 | `ptr` | Pointer to the element storage |

### Array literal codegen

An array literal `[a, b, c]` is lowered to:

1. Alloca a fixed-size LLVM array `[N x T]` on the stack.
2. Store each element via GEP indices `[0, i]`.
3. Cast the alloca pointer to `ptr`.
4. Build and return a `{ i64, ptr }` struct with `len = N` and `data = cast_ptr`.

The backing storage lives on the **stack** (the alloca). Callers must not store the slice pointer
beyond the lifetime of the enclosing stack frame.

### Index expression `receiver[index]`

Index codegen extracts the `data` pointer (field 1), then GEPs at `[idx]` using the element type
derived from the slice's semantic type annotation, then loads. No bounds checking is performed in
Phase 1; out-of-bounds access is undefined behaviour.

---

## 4. Current Runtime Functions

All functions in this section are declared and defined inline in `declare_builtins` in
`codegen.rs`. They are emitted into every compiled module. There is no separate shared library;
the runtime is embedded in the module IR.

### `println`

```c
void println(struct { int64_t len; char *data; } msg);
```

Calls `puts(msg.data)`. `puts` appends a newline. Writes to stdout.

**Ownership**: does not take ownership of the string; does not free `data`.  
**Called by**: `println(expr)` in Axon source.

---

### `print`

```c
void print(struct { int64_t len; char *data; } msg);
```

Calls `printf("%s", msg.data)`. No trailing newline. Writes to stdout.

**Ownership**: does not take ownership of the string; does not free `data`.  
**Called by**: `print(expr)` in Axon source.

---

### `eprintln`

```c
void eprintln(struct { int64_t len; char *data; } msg);
```

Phase 1 implementation: calls `puts(msg.data)` — same as `println`. Writes to stdout (not stderr).

> **Known limitation**: Phase 1 does not route `eprintln`/`eprint` to `stderr`. This is a planned
> fix for Phase 2. The semantic intention is stderr output.

**Ownership**: does not take ownership; does not free `data`.  
**Called by**: `eprintln(expr)` in Axon source.

---

### `eprint`

```c
void eprint(struct { int64_t len; char *data; } msg);
```

Phase 1 implementation: calls `printf("%s", msg.data)` — same as `print`. Writes to stdout.

> **Known limitation**: same as `eprintln` above.

**Ownership**: does not take ownership; does not free `data`.  
**Called by**: `eprint(expr)` in Axon source.

---

### `to_str`

```c
struct { int64_t len; char *data; } to_str(int64_t n);
```

Converts an `i64` to its decimal ASCII representation using `snprintf(buf, 32, "%lld", n)`.
The `data` pointer in the returned struct points to a **module-level static 32-byte buffer**
(`to_str_buf`). The `len` field is the number of characters written (not including the null
terminator).

**Ownership**: caller does not own the buffer. The buffer is overwritten on the next call to
`to_str`. Do not hold the returned pointer across another call.  
**Thread safety**: not thread-safe (shared static buffer).  
**Called by**: `to_str(n)` in Axon source; also by string interpolation `"{n}"` when `n: i64`.

---

### `to_str_f64`

```c
struct { int64_t len; char *data; } to_str_f64(double n);
```

Converts an `f64` to its string representation using `snprintf(buf, 32, "%.6g", n)`. The format
`%.6g` produces up to 6 significant digits, using exponential notation when the exponent is less
than -4 or greater than or equal to the precision. The returned `data` pointer points to a
**module-level static 32-byte buffer** (`to_str_f64_buf`).

**Ownership**: same caveats as `to_str` — static buffer, not thread-safe.  
**Called by**: `to_str_f64(n)` in Axon source; also by string interpolation `"{n}"` when `n: f64`.

---

### `axon_concat`

```c
struct { int64_t len; char *data; } axon_concat(
    struct { int64_t len; char *data; } a,
    struct { int64_t len; char *data; } b
);
```

Concatenates two strings. Allocates a new heap buffer of size `a.len + b.len + 1` via `malloc`,
copies `a.data` then `b.data` into it, and writes a null terminator at `a.len + b.len`.

Returns `{ total_len, buf }` where `total_len = a.len + b.len`.

**Ownership**: the returned `data` pointer is a `malloc`-allocated buffer. The caller is
responsible for freeing it. Phase 1 has no automatic memory management; string interpolation
chains that call `axon_concat` multiple times leak all intermediate buffers.  
**Called by**: string interpolation expressions `"prefix{expr}suffix"` — each interpolated
segment is joined left-to-right via successive `axon_concat` calls.

---

### `len`

```c
int64_t len(struct { int64_t len; char *data; } s);
```

Extracts and returns field 0 (the length) of the `str` struct. Equivalent to `s.len` in C.

**Ownership**: does not modify the string.  
**Called by**: `len(s)` in Axon source.

---

### `parse_int`

```c
struct { int8_t tag; int8_t payload[8]; } parse_int(
    struct { int64_t len; char *data; } s
);
```

Parses the string `s` as a base-10 integer using `strtoll(s.data, &endptr, 10)`.

- If `endptr != s.data` (at least one digit was consumed): returns `Ok(parsed_i64)` — `tag = 1`,
  payload contains the `i64` value stored as 8 bytes.
- If `endptr == s.data` (no digits consumed): returns `Err` — `tag = 0`, payload contains 8 zero
  bytes.

The `Result<i64, str>` type has `N = max(8, 16) = 16` bytes of payload in the canonical union
layout; however, `parse_int` is declared with a fixed `{ i1, [8 x i8] }` layout because `strtoll`
errors carry no message — the Err payload is ignored. Call sites that match the result with a
pattern must account for this layout discrepancy if they expect the generic canonical form.

**Ownership**: does not allocate heap memory; does not take ownership of `s.data`.  
**Called by**: `parse_int(s)` in Axon source.

---

### `assert`

```c
void assert(int1_t cond);
```

If `cond` is `0` (false): prints `assertion failed\n` via `printf`, then calls `exit(1)`.  
If `cond` is `1` (true): returns normally.

**Called by**: `assert(expr)` in Axon source.

---

### `assert_eq`

```c
void assert_eq(int64_t a, int64_t b);
```

If `a != b`: prints `assertion failed: values not equal\n` via `printf`, then calls `exit(1)`.  
If `a == b`: returns normally.

**Called by**: `assert_eq(a, b)` in Axon source (compares two `i64` values).

---

### `assert_err`

```c
void assert_err(int1_t tag);
```

Panics if `tag == 1` (i.e., the result is `Ok`). Prints
`assertion failed: expected Err, got Ok\n` and calls `exit(1)`.

`tag` is the discriminant field (field 0) of a `Result` struct.

**Called by**: `assert_err(tag)` in Axon source; typically used in test functions to assert that
a `Result` is an `Err`.

---

### Math builtins (`abs_i32`, `abs_f64`, `min_i32`, `max_i32`)

These are declared in the `BUILTINS` table in `builtins.rs` and are visible to the type checker,
but their LLVM IR bodies are **not** emitted by `declare_builtins` in Phase 1. They are expected
to be user-defined or supplied via link-time from libc/libm. Calls to these names will link
successfully only if the program provides implementations.

---

## 5. Panic Protocol

Axon programs have no structured panic unwinding in Phase 1. The panic protocol is:

1. Print a diagnostic message to stdout via `printf`.
2. Call `exit(1)`.
3. Emit `unreachable` in the LLVM IR following the `exit` call (so the IR verifier is satisfied).

The exit code is always **1** for assertion failures and panics. Programs that exit normally
(reach the end of `main`) return exit code **0** — `main` is lowered to `i32 main()` and the
compiler appends `ret i32 0` when the function returns `Unit`.

No stack unwinding, no destructors, no `Drop` trait. All in-flight heap allocations are leaked on
panic.

Functions that trigger panic:

| Trigger | Message printed | Exit code |
|---------|----------------|-----------|
| `assert(false)` | `assertion failed\n` | 1 |
| `assert_eq(a, b)` when `a != b` | `assertion failed: values not equal\n` | 1 |
| `assert_err(1)` (got Ok, expected Err) | `assertion failed: expected Err, got Ok\n` | 1 |

There is no user-callable `panic()` builtin in Phase 1. Idiomatic panic is spelled `assert(false)`.

---

## 6. Enum Layout

User-defined enums lower to a tagged-union struct:

```llvm
%MyEnum_enum = type { i32, [N x i8] }
;                     ^     ^^^^^^^^^
;                     tag   payload — N = max payload size across all variants, minimum 1
```

The tag is an `i32` (not `i1`) to accommodate more than two variants. Tag values are assigned
by variant declaration order starting at 0. Variant fields are packed into the payload at
consecutive byte offsets (no padding in Phase 1) in declaration order.

This is distinct from `Result<T,E>` (which uses `i1` as the discriminant) and `Option<T>`
(which uses `i1` as the discriminant).

---

## 7. Option<T> Layout

```llvm
%Option_T = type { i1, T }
;                   ^   ^
;                   |   inner value (undefined when tag is 0)
;                   discriminant: 0 = None, 1 = Some
```

For `Option<Unit>`, the inner type has no LLVM representation; the entire option collapses to a
single `i1`.

---

## 8. Phase 3 Planned Runtime Additions

> The following symbols are **planned / not yet implemented**. They are specified in
> `spec/compiler-phase3.md` (§4, Channels) and will be provided by a new `axon-rt/` static
> library (`libaxon_rt.a`). None of these symbols exist in the current compiler or runtime.

### `__axon_chan_new`

```c
/* Create a new unbuffered channel for elements of elem_size bytes.
   Returns an opaque handle. */
void* __axon_chan_new(size_t elem_size);
```

Allocates a new channel handle. The channel is unbuffered (rendezvous semantics): `send` blocks
until a receiver is ready, and vice versa. Internally backed by a POSIX mutex + condition
variable.

`elem_size` is `sizeof(T)` for a `chan<T>`. For example, `chan<i64>()` passes `elem_size = 8`.

---

### `__axon_chan_send`

```c
/* Send the value pointed to by val into ch.
   Blocks until a receiver calls __axon_chan_recv on the same channel. */
void __axon_chan_send(void* ch, void* val);
```

Copies `elem_size` bytes from `*val` into the channel's internal transfer buffer, then blocks
until a receiver consumes the value. After this call returns, `val` may be reused or freed; the
channel has its own copy.

In Axon source, `ch.send(v)` lowers to:

```llvm
%v_alloca = alloca i64
store i64 %v, ptr %v_alloca
call void @__axon_chan_send(ptr %ch, ptr %v_alloca)
```

---

### `__axon_chan_recv`

```c
/* Receive one element from ch into the buffer pointed to by dst.
   Blocks until a sender calls __axon_chan_send on the same channel. */
void __axon_chan_recv(void* ch, void* dst);
```

Blocks until a sender is ready. Copies `elem_size` bytes from the channel's transfer buffer into
`*dst`. The caller is responsible for providing a buffer of at least `elem_size` bytes.

In Axon source, `ch.recv()` lowers to:

```llvm
%dst = alloca i64
call void @__axon_chan_recv(ptr %ch, ptr %dst)
%val = load i64, ptr %dst
```

---

### `__axon_chan_clone`

```c
/* Increment the channel's reference count and return the same handle.
   Both handles refer to the same underlying channel. */
void* __axon_chan_clone(void* ch);
```

Called by `ch.clone()` in Axon source. Both the original and cloned handles must be dropped (via
`__axon_chan_drop`) for the channel to be freed.

---

### `__axon_chan_drop`

```c
/* Decrement the channel's reference count.
   Frees the channel when the count reaches zero. */
void __axon_chan_drop(void* ch);
```

The borrow checker (Phase 3) and the drop-insertion pass are responsible for emitting calls to
this function at the end of a `chan<T>` value's lexical scope.

---

### `__axon_spawn`

```c
/* Launch a new OS thread. The thread calls fn_ptr(env_ptr) and then exits.
   The caller is responsible for ensuring env_ptr outlives the thread
   (typically by allocating it with malloc and freeing inside the thread body). */
void __axon_spawn(void* fn_ptr, void* env_ptr);
```

`spawn { body }` lowers the body to a closure-like function `void __spawn_N(ptr env)` and calls
`__axon_spawn` with that function pointer and a `malloc`-allocated environment struct capturing
all free variables (using the same mechanism as closures, §3 of Phase 3 spec).

```llvm
; spawn { ch.send(42) }
%env = call ptr @malloc(i64 8)
store ptr %ch_val, ptr %env
call void @__axon_spawn(ptr @__spawn_1, ptr %env)
```

---

### `__axon_select`

```c
/* Block until one of n channels becomes ready for receive.
   ch_ptrs[i]  — pointer to the i-th channel handle.
   dst_ptrs[i] — pointer to a buffer of elem_size bytes for the i-th channel's value.
   Returns the index (0-based) of the channel that received a value. */
size_t __axon_select(void** ch_ptrs, void** dst_ptrs, size_t n);
```

Implements the `select { ch1.recv() => body1, ch2.recv() => body2 }` construct. The returned
index is used in a `switch` on the LLVM IR side to dispatch to the appropriate arm body. All
channels in `ch_ptrs` are polled; the first one that becomes ready wins and the others are
cancelled for this iteration.

Only `recv()` arms are supported in Phase 3. `send` arms in `select` are deferred to Phase 4.

```llvm
; select { ch1.recv() => body1, ch2.recv() => body2 }
%chs   = alloca [2 x ptr]
%dsts  = alloca [2 x ptr]
%d0    = alloca i64
%d1    = alloca i64
; populate arrays ...
%ready = call i64 @__axon_select(ptr %chs, ptr %dsts, i64 2)
switch i64 %ready, label %default [
  i64 0, label %arm0
  i64 1, label %arm1
]
```

---

## 9. Known Bugs Fixed in Phase 2

The following bugs existed in the Phase 1 runtime and are corrected in Phase 2. Code that
depended on the buggy behaviour must be updated.

### `eprint` / `eprintln` wrote to stdout — fixed to use fd 2

**Was**: Both functions called `puts` / `printf` which write to `stdout` (fd 1).  
**Now**: Both call `fprintf(stderr, ...)` which writes to `stderr` (fd 2). The symbol `stderr` is
provided by libc; no additional link dependency is required.

**Diagnostic impact**: programs that parsed `eprint` output from stdout will no longer see that
output there. Redirect stderr with `2>&1` to capture both streams.

---

### `to_str` / `to_str_f64` used a single static 32-byte buffer — fixed to use malloc-allocated buffers

**Was**: Both functions wrote into a module-level static buffer (`to_str_buf`, `to_str_f64_buf`).
The returned `str.data` pointer pointed into the static buffer. A second call overwrote the buffer,
invalidating any previously returned pointer. The functions were not thread-safe.

**Now**: Each call allocates a fresh 32-byte heap buffer via `malloc`. The returned `str.data`
pointer is always valid and independent of subsequent calls. The functions are re-entrant.

**Memory ownership change**: callers now hold a `malloc`-allocated buffer. No automatic free is
performed (no GC in Phase 2). Long-running programs accumulate unreleased buffers. This is
accepted for Phase 2; Phase 3 string ownership work will address it.

---

### `parse_int` Err variant stored `i64(0)` instead of a valid `str` struct — fixed

**Was**: When `strtoll` returned no digits (the input was not a valid integer), the Phase 1
implementation stored 8 zero bytes into the payload of the `Result` struct and set `tag = 0`
(Err). The 8-byte zero payload was not a valid `str` struct (a `str` is 16 bytes: `{i64 len, ptr data}`).
Any code that matched `Err(msg)` and then accessed `msg.len` or `msg.data` read garbage.

**Now**: The Err payload is the string `"invalid integer"` encoded as a proper `str` struct
(`{len: 15, data: ptr_to_global_literal}`). The constant literal is emitted as a module-level
global. The `parse_int` return type is now the full canonical `Result<i64, str>` layout
(`{ i1, [16 x i8] }`) rather than the Phase 1 `{ i1, [8 x i8] }` layout. Call sites that
hardcoded the 8-byte layout must be updated.

---

### Array literals used stack-allocated backing data — fixed to heap-allocate

**Was**: Array literals `[a, b, c]` were lowered to a fixed-size LLVM `alloca` on the stack.
The returned slice struct's `data` pointer pointed into the stack frame. Storing the slice or
returning it from a function produced a dangling pointer after the frame was released.

**Now**: Array literal codegen emits a `malloc` call sized to `N * sizeof(T)` and stores
elements into the heap buffer. The returned slice struct's `data` pointer is a stable heap
pointer that outlives the creating stack frame.

**Performance note**: heap allocation for every array literal is more expensive than stack
allocation. A future optimisation pass (escape analysis) will move allocations back to the stack
when the slice is proven not to escape. For Phase 2 this is the correct-by-default choice.

---

## 10. Memory Management

### Current Model (Phase 1 and Phase 2)

Axon uses **manual malloc with no free** for all heap-allocated values. This is intentional for
Phase 1 and 2: it avoids the complexity of a garbage collector or explicit drop pass while the
language semantics are still being defined. The trade-off is that long-running programs leak
memory.

The following values are heap-allocated and never freed:

| Value | Allocation site | Notes |
|-------|----------------|-------|
| String literals | Compile-time global (`.rodata`) | Static lifetime; no free needed |
| `axon_concat` result | `malloc` in `axon_concat` body | Leaks in Phase 1/2 |
| `to_str` result (Phase 2+) | `malloc` per call | Leaks in Phase 2 |
| `to_str_f64` result (Phase 2+) | `malloc` per call | Leaks in Phase 2 |
| Array literals (Phase 2+) | `malloc` per literal | Leaks in Phase 2 |
| Closure environments | `malloc` at closure creation | Leaks in Phase 3 |
| Channel handles | `malloc` in `__axon_chan_new` | Freed via `__axon_chan_drop` (ref-counted) |

### `axon_concat` Memory Semantics

The result of `axon_concat(a, b)` is a heap-allocated buffer of size `a.len + b.len + 1` bytes.
The caller receives a `str` struct whose `data` pointer owns this buffer. No free is ever called
in Phase 1 or 2.

String interpolation `"prefix{expr}suffix"` with N interpolation points generates N-1
`axon_concat` calls. All intermediate buffers are leaked. For a string with 3 interpolated
values, 2 intermediate buffers are allocated and leaked per interpolation evaluation.

### Phase 3 Plan: Define Ownership Semantics

Phase 3 must choose a memory strategy for strings and arrays. The options under consideration:

**Arena allocation** — All heap strings and arrays within a function call are allocated from a
per-call arena. The arena is freed in bulk when the function returns. Pros: zero per-string
overhead, deterministic free point, no borrow checker integration needed. Cons: strings that
need to outlive their creating function (e.g. returned strings) require a different allocator or
a copy to a longer-lived arena.

**Explicit drop via `axon_str_drop`** — The borrow checker (Phase 3) treats `str` as a move
type. When the owning binding goes out of scope, the compiler inserts a call to `axon_str_drop`:

```c
void axon_str_drop(struct { int64_t len; char *data; } s) {
    free((void*)s.data);   // only for non-literal strings
}
```

The compiler must distinguish heap-allocated strings (from `axon_concat`, `to_str`, etc.) from
literal strings (`.rodata` — must not be freed). A flag bit in the `str` struct or a separate
"is_owned" boolean is needed.

**Reference counting** — Wrap every `str` in a reference-counted header. Increment on clone,
decrement on drop; free when count reaches zero. Adds 8 bytes per string and two memory
operations per assignment. No borrow checker required. Highest overhead of the three options.

The chosen approach will be documented here and in `spec/stdlib.md` before Phase 3 codegen
begins. Until then, all string memory leaks are known and accepted.

---

## Appendix: C Stdlib Dependencies

The current runtime relies on the following C standard library symbols, resolved from the host
process at link time (or JIT symbol resolution):

| Symbol | Header | Usage |
|--------|--------|-------|
| `puts` | `<stdio.h>` | `println`, `eprintln` |
| `printf` | `<stdio.h>` | `print`, `eprint`, assertion messages |
| `snprintf` | `<stdio.h>` | `to_str`, `to_str_f64` |
| `exit` | `<stdlib.h>` | panic protocol |
| `malloc` | `<stdlib.h>` | `axon_concat` heap allocation |
| `memcpy` | `<string.h>` | `axon_concat` buffer copy |
| `strtoll` | `<stdlib.h>` | `parse_int` |

No other external symbols are required in Phase 1.
