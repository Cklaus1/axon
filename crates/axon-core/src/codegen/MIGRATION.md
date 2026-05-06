# IR.3 Migration Playbook

Concrete recipes for migrating each codegen module from direct
`self.builder.*` / `self.context.*` / `self.module.*` calls to the
`self.ir.*` IR-trait calls.  Pair this doc with `ir.rs` (the trait
surface) and `ir_inkwell.rs` (the bounded inkwell impl).

**Migration order** (smallest first; each batch validated by a fresh
`cargo build -p axon-core` before moving to the next):

1. `asi.rs`           — ~30 sites, smallest cross-section
2. `option_result.rs` — ~30 sites
3. `types.rs`         — ~20 sites
4. `output.rs`        — ~10 sites
5. `match_pat.rs`     — ~80 sites
6. `expr.rs`          — ~700 sites (the hardest)
7. `builtins.rs`      — ~3,000 sites (bulk of the work)
8. `mod.rs`           — ~200 sites

## ⚠️ Architectural constraint discovered post-Step-0

`InkwellBackend<'ctx>` (as written in `ir_inkwell.rs`) **owns its own
`Module<'ctx>` and `Builder<'ctx>`** — separate from
`Codegen::module` / `Codegen::builder`.  Each `inkwell::Module` is an
isolated symbol table.

**Implication**: if asi.rs migrates and declares `__axon_verify_panic`
through `self.ir.add_function(…)`, the function lands in
`self.ir.module`.  But un-migrated callers (in expr.rs, mod.rs, etc.)
look up symbols via `self.module.get_function(…)` — which scans the
*legacy* module, not the IR-backed one.  **The function isn't found.
Linker error at runtime / call returns None, etc.**

This means **partial migration is syntactically validatable but
cannot link end-to-end**.  IR.3 batches still land safely as design
drafts (they compile, types check), but a runnable binary requires
EITHER:

  a) **Atomic IR.3 + IR.4** — migrate all 7 modules in one PR, then
     replace `Codegen::module/builder` with `Codegen::ir`'s.  The legacy
     fields disappear; only one Module survives.  All call sites land
     simultaneously.

  b) **Re-architect `InkwellBackend` to share** Codegen's Module +
     Builder via borrow / shared-owner pattern.  Then per-batch
     migration is buildable end-to-end at every step.  This is the
     better option but requires rewriting `ir_inkwell.rs::new` to take
     existing module/builder by reference, plus arena-field placement
     decisions.

**Recommendation**: pursue (b) before any IR.3 batch.  Option (a) is
all-or-nothing and can't be staged.  Option (b) has a clear scope:
~30-line refactor of `ir_inkwell.rs::new` + `Codegen::new` + adjust
trait signatures where they take `&mut self`.

Until (b) is done, treat IR.3 migrations as design drafts only.  Don't
attempt to validate them via `cargo build`.

## Step 0 — Wire `ir` into Codegen

Once-per-codebase change before any per-module migration:

```rust
// codegen/mod.rs

pub struct Codegen<'ctx> {
    pub context: &'ctx Context,    // KEEP for now (incremental migration)
    pub module: Module<'ctx>,       // KEEP for now
    pub builder: Builder<'ctx>,     // KEEP for now
    pub ir: super::ir_inkwell::InkwellBackend<'ctx>, // NEW
    // … other fields unchanged …
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        let ir = super::ir_inkwell::InkwellBackend::new(context, module_name); // NEW
        Self { context, module, builder, ir, … }
    }
}
```

Caveat: this constructs *two* LLVM modules per codegen run (the legacy
one and the IR-backed one).  That's fine during migration — the legacy
one still works, the IR one is empty until callers populate it.  Once
IR.4 lands, the legacy fields are removed and only `ir` survives.

## Per-call recipes

### Type construction

| Before | After |
|---|---|
| `self.context.i64_type()` | `let t = self.ir.t_i64();` |
| `self.context.i8_type().ptr_type(AddressSpace::default())` | `let t = self.ir.t_ptr();` |
| `self.context.struct_type(&[a.into(), b.into()], false)` | `let t = self.ir.t_struct(&[ta, tb], false);` |
| `self.context.bool_type()` | `let t = self.ir.t_bool();` |
| `self.context.void_type().fn_type(&[…], false)` | `let t = self.ir.t_fn(&[…], None, false);` |

### Constants

| Before | After |
|---|---|
| `i64_ty.const_int(42, true)` | `let v = self.ir.const_i64(42);` |
| `bool_ty.const_int(1, false)` | `let v = self.ir.const_bool(true);` |
| `f64_ty.const_float(3.14)` | `let v = self.ir.const_f64(3.14);` |
| `i64_ty.const_zero()` | `let v = self.ir.const_zero(t_i64);` |
| `struct_ty.get_undef()` | `let v = self.ir.const_undef(t_struct);` |

### Function / module operations

| Before | After |
|---|---|
| `self.module.add_function("foo", fn_ty, None)` | `let f = self.ir.add_function("foo", t_fn);` |
| `self.module.get_function("foo")` | `let f = self.ir.get_function("foo")?;` |
| `f.get_nth_param(0).unwrap()` | `let p = self.ir.fn_param(f, 0);` |

### Basic blocks

| Before | After |
|---|---|
| `self.context.append_basic_block(fn_val, "entry")` | `let b = self.ir.append_block(f, "entry");` |
| `self.builder.position_at_end(bb)` | `self.ir.position_at_end(b);` |
| `self.builder.get_insert_block()` | `self.ir.current_block()` |
| `bb.get_terminator().is_some()` | `self.ir.block_terminated(b)` |

### Memory operations

| Before | After |
|---|---|
| `self.builder.build_alloca(t, "name").unwrap()` | `let p = self.ir.alloca(t);` |
| `self.builder.build_load(t, ptr, "name").unwrap()` | `let v = self.ir.load(t, ptr);` |
| `self.builder.build_store(ptr, val).unwrap()` | `self.ir.store(ptr, val);` |
| `self.builder.build_struct_gep(t, ptr, 0, "name").unwrap()` | `let g = self.ir.struct_gep(t, ptr, 0);` |
| `self.builder.build_pointer_cast(p, target_ty, "n").unwrap()` | `let c = self.ir.ptr_cast(p, target_ty);` |

Notice the **`name: &str` parameter is dropped**.  The IR backend
synthesizes names internally (`v_0`, `v_1`, …) — they don't affect
semantics.

### Aggregate ops

| Before | After |
|---|---|
| `self.builder.build_extract_value(sv, 0, "n")` | `let v = self.ir.extract_value(agg, 0);` |
| `self.builder.build_insert_value(sv, val, 0, "n")` | `let new_agg = self.ir.insert_value(agg, val, 0);` |

### Integer arithmetic

| Before | After |
|---|---|
| `self.builder.build_int_add(a, b, "n").unwrap()` | `let r = self.ir.iadd(a, b);` |
| `self.builder.build_int_sub(a, b, "n").unwrap()` | `let r = self.ir.isub(a, b);` |
| `self.builder.build_int_mul(a, b, "n").unwrap()` | `let r = self.ir.imul(a, b);` |
| `self.builder.build_int_signed_div(a, b, "n").unwrap()` | `let r = self.ir.idiv_signed(a, b);` |
| `self.builder.build_int_signed_rem(a, b, "n").unwrap()` | `let r = self.ir.irem_signed(a, b);` |
| `self.builder.build_int_neg(a, "n").unwrap()` | `let r = self.ir.ineg(a);` |
| `self.builder.build_int_compare(IntPredicate::EQ, a, b, "n")` | `let r = self.ir.icmp(IRIntPred::Eq, a, b);` |
| `self.builder.build_left_shift(a, b, "n").unwrap()` | `let r = self.ir.shl(a, b);` |
| `self.builder.build_right_shift(a, b, sign, "n").unwrap()` | `let r = self.ir.shr(a, b, sign);` |
| `self.builder.build_and(a, b, "n").unwrap()` | `let r = self.ir.iand(a, b);` |
| `self.builder.build_or(a, b, "n").unwrap()` | `let r = self.ir.ior(a, b);` |
| `self.builder.build_xor(a, b, "n").unwrap()` | `let r = self.ir.ixor(a, b);` |
| `self.builder.build_not(a, "n").unwrap()` | `let r = self.ir.inot(a);` |

### Width conversions

| Before | After |
|---|---|
| `self.builder.build_int_truncate(a, t, "n").unwrap()` | `let r = self.ir.int_truncate(a, t);` |
| `self.builder.build_int_z_extend(a, t, "n").unwrap()` | `let r = self.ir.int_zext(a, t);` |
| `self.builder.build_int_s_extend(a, t, "n").unwrap()` | `let r = self.ir.int_sext(a, t);` |

### Float arithmetic

Same pattern as integer (`build_float_add` → `fadd`, `build_float_compare(FloatPredicate::OEQ, …)` → `fcmp(IRFloatPred::Oeq, …)`).

### Float ↔ Int

| Before | After |
|---|---|
| `self.builder.build_signed_int_to_float(a, t, "n")` | `let r = self.ir.sitof(a, t);` |
| `self.builder.build_float_to_signed_int(a, t, "n")` | `let r = self.ir.ftosi(a, t);` |

### Control flow

| Before | After |
|---|---|
| `self.builder.build_unconditional_branch(b).unwrap()` | `self.ir.br(b);` |
| `self.builder.build_conditional_branch(c, t, e).unwrap()` | `self.ir.cond_br(c, t, e);` |
| `self.builder.build_switch(s, default, &cases)` | `self.ir.switch(s, default, &cases);` |
| `self.builder.build_return(None).unwrap()` | `self.ir.ret_void();` |
| `self.builder.build_return(Some(&v)).unwrap()` | `self.ir.ret(v);` |
| `self.builder.build_unreachable().unwrap()` | `self.ir.unreachable();` |
| `self.builder.build_select(c, t, e, "n").unwrap()` | `let v = self.ir.select(c, t, e);` |

### Calls

| Before | After |
|---|---|
| `self.builder.build_call(fn_v, &args, "n").unwrap()`<br/>`  .try_as_basic_value().left()` | `let v = self.ir.call(f, &args);  // returns Option<IRValue>` |
| `self.builder.build_indirect_call(ft, fp, &args, "n")` | `let v = self.ir.call_indirect(ft, fp, &args);` |

### Output

| Before | After |
|---|---|
| `self.module.verify()` | `self.ir.verify()` |
| `self.module.print_to_file(path)` | `self.ir.write_ir_text(path)` |
| `self.module.write_bitcode_to_memory().as_slice().to_vec()` | `self.ir.emit_bitcode()` |

## Common gotchas

### `name: &str` parameter

Inkwell's `build_*` methods take a name string for the SSA register
("add", "tmp", etc.).  The IR trait drops this — `InkwellBackend`
synthesizes unique names internally.  If you need a specific name for
debugging IR, the impl of `next_name()` in `ir_inkwell.rs` can be
extended to take a hint.

### `unwrap()` on builder calls

Inkwell builder methods return `Result<…, BuilderError>` since v0.4.
The IR trait wraps these and panics on builder error (which only
happens for misuse like inserting into a terminated block).  Migration
removes the `.unwrap()` calls.

### Predicate enums

`inkwell::IntPredicate::EQ` → `super::ir::IRIntPred::Eq`.  Drop the
`use inkwell::IntPredicate;` import; add `use super::ir::IRIntPred;`.

### Generic-typed param patterns

Some inkwell methods need a typed param (e.g. the type passed to
`build_load`).  In the IR trait this becomes an `IRType` handle —
unify all type creation through `self.ir.t_*()` so the handle is
captured early in the surrounding scope.

### `BasicValueEnum` matching

A few sites match on `BasicValueEnum::IntValue(_)` or similar to
distinguish kinds.  In IR-trait land, use the `ty_of(handle)` query
(currently a TODO stub — finish in IR.4) to dispatch by type kind.
For now, leave such sites on the legacy `self.builder` path; they'll
be the last to migrate.

## Validation between batches

After each module migrates:

```bash
# Fast: parse + non-codegen lib check
cd /tmp/axon-check
cargo build --release

# Slow but real: codegen-feature build
cd /home/cklaus/projects/axon
RUST_MIN_STACK=16777216 cargo build -p axon-core
```

A full `cargo build` after every batch isolates failures to the most
recent migration.  If a batch breaks the build, `git revert` and
shrink the batch (one method at a time if needed).

## What done looks like

After IR.3 finishes:
* All codegen modules call `self.ir.*` for inkwell operations.
* `self.builder` / `self.context` / `self.module` are still present but
  unused — IR.4 removes them.

After IR.4 finishes:
* `Codegen<'ctx>` has only `ir: InkwellBackend<'ctx>` for LLVM access.
* The 5,000+ inkwell-generic call sites are gone; only the 80 in
  `ir_inkwell.rs` remain.

After IR.5 finishes (validation):
* `cargo build -p axon-core` completes in <30 minutes on canonical
  hardware (vs. 5h+ today).  This is the metric of success.
