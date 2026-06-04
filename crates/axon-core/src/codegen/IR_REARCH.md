> **SUPERSEDED (R1e, 2026-06-04):** the `IR` trait + arena shim this doc describes was abandoned (0 callers) and DELETED. The single IR-emission path is now `self.ir.{context,module,builder}` + `build_wrappers::w_*`. See `governance/specs/R1e-ir-backend-consolidation.md`. This file is kept for history.

# `InkwellBackend` Re-Architecture: Shared Module + Builder

**Status**: Design draft — no source changes yet.
**Goal**: Fix the dual-module split so IR.3 per-batch migrations are
buildable and linkable end-to-end at every step.

---

## 1. The Problem (concrete form)

`Codegen::new` (mod.rs line 171–177) creates two independent LLVM modules:

```rust
// mod.rs:171-177
let module  = context.create_module(module_name);          // legacy module
let builder = context.create_builder();
let ir      = InkwellBackend::new(context,
                  &format!("{}_ir", module_name));          // owns a SECOND module
```

`InkwellBackend::new` (ir_inkwell.rs:82-101) itself calls:

```rust
let module  = context.create_module(module_name);   // ir_inkwell.rs:83
let builder = context.create_builder();              // ir_inkwell.rs:84
```

Each `inkwell::Module` is a separate LLVM symbol table.  When asi.rs
calls `self.ir.add_function("__axon_verify_panic", …)`, the function
lands in `self.ir.module`.  The still-unmigrated `asi.rs` line 153
then calls `self.module.get_function("__axon_verify_panic")` — which
scans the *legacy* module and returns `None`.  Linker error or silent
wrong-codegen every time a partially-migrated call crosses the boundary.

---

## 2. Options Considered

### Option (a) — Borrowed references (`&'a mut Module<'ctx>`)

`InkwellBackend` holds borrows into `Codegen`'s owned fields:

```rust
pub struct InkwellBackend<'a, 'ctx: 'a> {
    pub context: &'ctx Context,
    pub module:  &'a mut Module<'ctx>,
    pub builder: &'a mut Builder<'ctx>,
    // arenas...
}
```

`Codegen` stores only the arenas as a separate type, while `module` and
`builder` remain in `Codegen`:

```rust
pub struct Codegen<'ctx> {
    pub context: &'ctx Context,
    pub module:  Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub ir:      IrArenas<'ctx>,   // arenas only, no module/builder
    // ...legacy fields...
}
```

`IrArenas<'ctx>` holds the five `Vec` arenas and `name_counter`.  The
`impl IR for …` lives on an ephemeral `IrHandle<'a,'ctx>` vended by a
method:

```rust
impl<'ctx> Codegen<'ctx> {
    fn ir(&mut self) -> IrHandle<'_, 'ctx> {
        IrHandle {
            context: self.context,
            module:  &mut self.module,
            builder: &mut self.builder,
            arenas:  &mut self.ir,
        }
    }
}
```

Call sites become `self.ir().iadd(a, b)` (note the `()` — a short-lived
borrow, not a stored field).

**Lifetime pain**: `IrHandle<'a,'ctx>` introduces a second lifetime
parameter.  Anywhere the caller holds an `IRValue` *and* calls a method
on `self` simultaneously, the borrow checker sees an overlap between the
ephemeral `&mut self.module` inside the handle and any other `&self` or
`&mut self` access in the same scope.  This surfaces heavily in
`emit_expr` and `emit_fn`, which mix IR emission with `self.locals`
lookups.  Every such site needs explicit re-borrows or temporary
variables.  The migration diff is mechanically predictable but large
(~700 sites in expr.rs alone).

**Verdict**: technically correct, no unsafe, but the borrow-split
friction per call site is high and makes the IR.3 migration harder, not
easier.

---

### Option (b) — `Rc<RefCell<>>` shared ownership

```rust
pub struct InkwellBackend<'ctx> {
    pub context: &'ctx Context,
    module:  Rc<RefCell<Module<'ctx>>>,
    builder: Rc<RefCell<Builder<'ctx>>>,
    // arenas...
}

pub struct Codegen<'ctx> {
    pub context: &'ctx Context,
    pub module:  Rc<RefCell<Module<'ctx>>>,
    pub builder: Rc<RefCell<Builder<'ctx>>>,
    pub ir:      InkwellBackend<'ctx>,
    // ...
}
```

Both `Codegen` and `InkwellBackend` clone the same `Rc` at construction.
`self.ir.add_function(…)` borrows `module.borrow_mut()` and operates on
the same symbol table as `self.module.borrow_mut()`.

**Problems**:

1. `inkwell::Module<'ctx>` and `Builder<'ctx>` are not `Clone` and hold
   raw LLVM pointers.  Wrapping them in `Rc<RefCell<>>` means every
   call site becomes `self.module.borrow_mut().add_function(…)` — more
   syntactic noise than the current direct field access.
2. `RefCell` panics at runtime on aliased borrows.  Two calls to IR
   methods in the same statement can cause a double-`borrow_mut` panic
   that won't appear until runtime testing.
3. `Rc` is not `Send`, which rules out any future multi-threaded
   compilation pipeline.

**Verdict**: wrong tool for a 'ctx-lifetime-scoped struct.  The dynamic
borrow overhead and ergonomic cost outweigh the simplicity.

---

### Option (c) — Move module + builder into InkwellBackend (recommended)

`InkwellBackend` owns `module` and `builder` (as it does today, but with
a correct construction contract).  `Codegen` holds *only* `InkwellBackend`
and delegates all LLVM access through `self.ir.*`.  The legacy
`Codegen::module`, `Codegen::builder`, `Codegen::context` fields are
*removed up front* rather than at IR.4.

The key insight: the reason the dual-module split felt safe was that
"IR.4 will remove the legacy fields later."  But the correct move is to
do that removal *as part of re-architecting*, which is what the
migration was going to do anyway — just in one atomic step for the
struct layout, not for the call sites.

```rust
// ir_inkwell.rs — new constructor

impl<'ctx> InkwellBackend<'ctx> {
    /// Adopt an externally-created module + builder.
    /// The caller (Codegen::new) creates them once; the backend takes ownership.
    pub fn adopt(
        context: &'ctx Context,
        module:  Module<'ctx>,
        builder: Builder<'ctx>,
    ) -> Self {
        Self {
            context,
            module,
            builder,
            values:      Vec::new(),
            types:       vec![None],   // slot 0 = void
            blocks:      Vec::new(),
            functions:   Vec::new(),
            globals:     Vec::new(),
            fn_types:    Vec::new(),
            name_counter: 0,
        }
    }

    /// Keep the existing `new` for standalone tests and benchmarks.
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module  = context.create_module(module_name);
        let builder = context.create_builder();
        Self::adopt(context, module, builder)
    }
}
```

```rust
// mod.rs — new Codegen struct layout

pub struct Codegen<'ctx> {
    /// All LLVM access goes through the IR backend.  Direct
    /// `self.context.*`, `self.module.*`, `self.builder.*` calls are
    /// replaced by `self.ir.*` as each module is migrated (IR.3).
    /// The raw inkwell fields are accessible via `self.ir.context`,
    /// `self.ir.module`, `self.ir.builder` during the transition.
    pub ir: ir_inkwell::InkwellBackend<'ctx>,

    // ── legacy fields that are NOT inkwell ──────────────────────────
    locals:          HashMap<String, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>,
    functions:       HashMap<String, FunctionValue<'ctx>>,
    struct_fields:   HashMap<String, Vec<String>>,
    fn_return_types: HashMap<String, Type>,
    local_types:     HashMap<String, Type>,
    current_result_types: Option<(Type, Type)>,
    lambda_counter:  u32,
    fmtstr_counter:  u32,
    enum_variants:   HashMap<String, Vec<(String, usize, Vec<Type>)>>,
    fndefs:          HashMap<String, ast::FnDef>,
    generic_fn_params: HashMap<String, Vec<String>>,
    trait_defs:      HashMap<String, ast::TraitDef>,
    vtable_globals:  HashMap<(String, String), inkwell::values::GlobalValue<'ctx>>,
    fn_axon_params:  HashMap<String, Vec<ast::AxonType>>,
    vtable_thunk_types: HashMap<(String, String), inkwell::types::FunctionType<'ctx>>,
    comptime_env:    HashMap<String, crate::comptime::ComptimeVal>,
    loop_stack:      Vec<(inkwell::basic_block::BasicBlock<'ctx>,
                          inkwell::basic_block::BasicBlock<'ctx>)>,
    current_lambda_env: Option<(PointerValue<'ctx>, StructType<'ctx>,
                                HashMap<String, u32>)>,
    current_adaptive_fn:       Option<String>,
    adaptive_registry_targets: Vec<String>,
    current_verify_fn:         Option<(String, &'static str, f64)>,
}
```

```rust
// mod.rs — new Codegen::new

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module  = context.create_module(module_name);
        let builder = context.create_builder();
        let ir      = ir_inkwell::InkwellBackend::adopt(context, module, builder);
        Self {
            ir,
            locals: HashMap::new(),
            // ... all other fields as before ...
        }
    }
}
```

Note that `Codegen` no longer has `context`, `module`, or `builder` as
direct fields.  Un-migrated code that reads `self.context` becomes
`self.ir.context`; `self.module` becomes `self.ir.module`; `self.builder`
becomes `self.ir.builder`.  These are simple mechanical renames with no
semantic change — they still reach the same underlying LLVM objects.

---

### Option (d) — `unsafe` pointer aliasing / `ManuallyDrop`

Put `module` and `builder` in `ManuallyDrop` with raw-pointer sharing
between `Codegen` and `InkwellBackend`.  Possible, but requires unsafe
and introduces UAF risk if drop order is wrong.  Rejected: no benefit
over option (c).

---

## 3. Recommendation: Option (c)

**Move module + builder into `InkwellBackend` from day one.**

Rationale:

1. **Zero extra lifetimes** — single `'ctx`, no `'a` parameter.
   Option (a) requires a second lifetime everywhere `IrHandle` is used
   or stored, which cascades into every method signature in all eight
   codegen sub-modules.

2. **Single symbol table from first migration** — any function declared
   via `self.ir.add_function(…)` is visible to `self.ir.module.get_function(…)`
   and also to the legacy lookup `self.ir.module.get_function(…)` that
   un-migrated code uses after the rename.  No dual-module split.

3. **Minimal rename surface for the transition** — the only textual
   change in un-migrated files is `self.module` → `self.ir.module`,
   `self.builder` → `self.ir.builder`, `self.context` → `self.ir.context`.
   A `sed` one-liner handles the bulk:
   ```
   s/self\.module\b/self.ir.module/g
   s/self\.builder\b/self.ir.builder/g
   s/self\.context\b/self.ir.context/g
   ```
   IR.3 migrations can then replace each `self.ir.module.add_function(…)`
   with `self.ir.add_function(…)` (the IR-trait form) one batch at a time,
   with each batch validated end-to-end.

4. **IR.4 becomes trivial** — when all call sites in a file are migrated
   to the IR-trait form, no remaining `self.ir.module.*` / `self.ir.builder.*`
   / `self.ir.context.*` uses exist in that file.  IR.4 is just removing
   the `pub context/module/builder` visibility from the backend struct,
   not restructuring anything.

5. **No runtime overhead** — option (b)'s `RefCell` dynamic-borrow cost
   and `Rc` clone overhead are absent; option (c) has identical hot-path
   performance to the current code.

---

## 4. Lifetime Trade-offs Summary

| Option | Lifetimes | Runtime cost | Migration friction | Safe |
|--------|-----------|--------------|-------------------|------|
| (a) borrowed refs | `'a,'ctx` on `IrHandle` | none | high (two-lifetime split per call) | yes |
| (b) `Rc<RefCell<>>` | `'ctx` only | `RefCell` borrow per call; `Rc::clone` at init | medium | yes (panic risk) |
| (c) move into IR | `'ctx` only | none | low (mechanical rename) | yes |
| (d) unsafe ptr | `'ctx` only | none | medium | no |

---

## 5. Migration Impact by File

### `mod.rs`

- Remove `pub context`, `pub module`, `pub builder` fields from `Codegen`.
- Add `pub ir: ir_inkwell::InkwellBackend<'ctx>` (already present as of
  the current commit at mod.rs:166 — just remove the duplicate creation
  of module/builder at lines 171-177 and thread them through `adopt`).
- Global rename: ~200 call sites of `self.context.*`, `self.module.*`,
  `self.builder.*` → `self.ir.context.*`, `self.ir.module.*`,
  `self.ir.builder.*`.  All semantically inert.
- `Codegen::new` shrinks by 3 lines (no separate module/builder init).
- `emit_fn`, `declare_vtable_thunks`, `emit_vtable_thunks`,
  `emit_vtable_globals`: rename only.

### `asi.rs`

Currently accesses `self.module`, `self.builder`, `self.context` at
~30 sites (lines 32, 37, 40–45, 65, 68–86, 153, 160, 172, 174, 198,
205–217).  After the rename pass they become `self.ir.module.*` etc.
Then each can be converted to the IR-trait call in the IR.3 batch —
using `self.ir.get_function("__axon_verify_panic")` instead of
`self.ir.module.get_function(…)`.  The IR.3 asi.rs batch is then
self-contained and end-to-end linkable.

### `option_result.rs`, `types.rs`, `match_pat.rs`

Same pattern: rename pass first, then IR.3 batch per file.  No
structural change needed beyond the prefix rename.

### `output.rs`

Uses `self.module` at lines 32, 59, 62, 67, 80, 90.  After rename:
`self.ir.module.*`.  IR.3 batch maps these to:
- `self.ir.module.verify()` → `self.ir.verify()`
- `self.ir.module.print_to_file(…)` → `self.ir.write_ir_text(…)`
- `self.ir.module.write_bitcode_to_memory()` → `self.ir.emit_bitcode()`
- JIT execution engine: `self.ir.module.create_jit_execution_engine(…)` —
  this is *not* covered by the IR trait (no JIT method).  Keep as
  `self.ir.module.*` in output.rs until a `fn jit_run` method is added
  to the trait.

### `expr.rs`, `builtins.rs`

Largest files (~1760 and ~3960 LoC, ~700 and ~3000 call sites).  The
rename pass can be done mechanically with `sed`; the IR.3 batch for
these files is a separate, longer effort.  With option (c), those
batches are end-to-end valid from the first rename — no symbol-table
divergence.

### `ir_inkwell.rs` itself

Add `InkwellBackend::adopt(context, module, builder)` constructor
(~8 lines).  Keep `new` for tests.  No other changes.

The existing unit tests in `ir_inkwell.rs` (lines 893–990) call
`InkwellBackend::new(&ctx, "test_*")` directly — they continue to work
unchanged since `new` is preserved.

---

## 6. Recommended Execution Order

1. **Add `InkwellBackend::adopt`** in ir_inkwell.rs (~8 lines, no
   other changes).

2. **Rename pass** in mod.rs: replace the inline `let module = …` / `let
   builder = …` with `InkwellBackend::adopt(context, module, builder)`
   and remove the separate fields.  Mechanically replace all
   `self.context` → `self.ir.context`, `self.module` → `self.ir.module`,
   `self.builder` → `self.ir.builder` in all eight codegen files.  This
   is one PR, ~400 net line changes, zero semantic change.

3. **Validate** with `rustfmt --check` and the no-codegen-feature build
   (`/tmp/axon-check`).

4. **IR.3 asi.rs batch**: convert the ~30 `self.ir.module.*` /
   `self.ir.builder.*` sites in asi.rs to `self.ir.*` trait calls.
   Validate with full `cargo build -p axon-core` (on canonical hardware)
   — now linkable end-to-end because there is only one module.

5. Continue IR.3 batches per MIGRATION.md order.

---

## 7. Top Risks of Option (c)

**Risk 1 — `FunctionValue<'ctx>` arena redundancy.**
`Codegen::functions: HashMap<String, FunctionValue<'ctx>>` and
`InkwellBackend::functions: Vec<FunctionValue<'ctx>>` both store the
same inkwell `FunctionValue` objects.  After option (c) they reference
the *same* underlying LLVM module, so there is no data divergence, but
the two tables can get out of sync if a declaration goes through the
IR-trait path (`ir.add_function`) but the handle is looked up via the
legacy `Codegen::functions` HashMap (or vice versa).  Mitigation: during
IR.3, whenever a file migrates `module.add_function` to `ir.add_function`,
also remove the corresponding `self.functions.insert(…, fn_val)` if the
IR backend's `functions` Vec is now the canonical source.  Full
deduplication deferred to IR.4 once all callers are migrated.

**Risk 2 — `vtable_globals` and other fields holding inkwell-typed values.**
`Codegen` holds several fields typed on inkwell generics that are NOT
wrapped in the IR arena: `vtable_globals`, `loop_stack`,
`current_lambda_env`, `vtable_thunk_types`.  These remain as direct
inkwell types in `Codegen` even after option (c) — they are not
impacted by the module-sharing fix itself, but they mean `Codegen` still
imports inkwell in mod.rs after the rename pass.  This is expected and
acceptable for the transition period.  IR.4 will replace them with IR
handle types (`IRGlobal`, `IRBlock`) once all sites that produce/consume
those values are migrated.
