# Tech Spec — R1d: Single-Source Builtins

**Status:** 📋 Draft (2026-06-04) — the structural simplification that compounds.
**Requirement:** R1 (native pipeline) — supports the whole stdlib without the
per-builtin "triple-write" tax. Extends `R1-codegen-build-unblock.md` (which
moves inline-IR builtins to `axon-rt` for *build speed*) with the *authoring*
half: make adding/changing a builtin **one edit, not four**.

**Decisive fork:** *How does one builtin stop being defined in 3–4 hand-synced
places?*
- **(a) Proc-macro** `#[axon_builtin]` on each axon-rt fn that emits the sig
  table entry + codegen declaration. Powerful but adds a proc-macro crate,
  compile-time cost, and a second metaprogramming layer to learn.
- **(b) One declarative registry** — a single `const BUILTIN_EXTERNS: &[…]`
  table (axon\_name, extern symbol, LLVM param/ret shape, semantic return Type)
  that codegen *iterates* to declare all externs, paired with the Rust impl in
  axon-rt. Plain data; no macro magic; greppable.
- **→ Resolve: (b) declarative registry.** It matches the codebase's existing
  style (the `BUILTINS` table is already a `const &[BuiltinFn]`), needs no new
  crate, and a human can read the whole builtin surface in one file. The
  load-bearing simplification is collapsing the ~120 hand-written
  `declare_builtins` blocks into table rows — a macro is overkill for that.

---

## 1. The tax this removes

Today a single builtin lives in 3–4 hand-synced sites (measured 2026-06-04):
1. `builtins.rs` `BUILTINS` table (184 rows) — the source-level signature.
2. `interp.rs` match arm (186 arms) — the reference semantics (the I-2 oracle).
3. codegen — **either** ~120 lines of hand-emitted LLVM IR in
   `codegen/builtins.rs` (3787 LOC) / `codegen/expr.rs` (4285 LOC), **or** an
   `axon-rt` extern + a branch in the 388-line `emit_call` `if name == "…"`
   chain.
4. `infer.rs`/`declare_builtins` `fn_return_types` (the return Type), often a
   third manual insert.

A new builtin = 3–4 edits that the parity harnesses (22 of them) catch only
*after* they drift. The two largest codegen files are this boilerplate.

## 2. Target shape (per builtin, post-R1d)

```rust
// crates/axon-rt/src/builtins/<group>.rs  — the ONE canonical impl
#[no_mangle]
pub extern "C" fn __axon_abs_i64(n: i64) -> i64 { n.checked_abs()... }
```

```rust
// crates/axon-core/src/codegen/builtin_externs.rs — ONE registry row
ExternSig {
    axon_name: "abs_i64",
    symbol:    "__axon_abs_i64",
    params:    &[L::I64],          // LLVM param shape
    ret:       L::I64,
    sem_ret:   Type::I64,          // for fn_return_types
}
```

`declare_builtins` becomes: `for e in BUILTIN_EXTERNS { declare_one(e) }` —
where `declare_one` builds the `fn_type` from `e.params`/`e.ret`, does the
get-or-`add_function`, and the two `insert`s. The ~120 hand-written blocks for
already-extern builtins collapse to ~120 table rows + one 15-line loop.

`L` is a tiny LLVM-shape enum (`I64`, `F64`, `I1`, `Str` (= `{i64,i8*}`),
`Slice`, `Ptr`, `OutParam(...)`) → resolved to `BasicMetadataTypeEnum` by
`declare_one`. It covers the by-value-scalar + str-struct + out-param shapes
that 90% of externs already use. Builtins with bespoke call-site lowering
(`to_str` polymorphic dispatch, the arr_*/dict_* inline loops) **stay as
emit_call special-cases** — R1d is for the straight `declare → link` externs,
which are the bulk and the boilerplate.

## 3. Why this is also the build-speed fix (composes with R1)

R1 moves inline-IR builtins into axon-rt to delete their IR-generation cost.
R1d is the *authoring ergonomics* that makes those moves cheap and keeps them
from regressing: once a builtin is a registry row + an axon-rt fn, there is no
hand-emitted IR to drift, and `cargo llvm-lines` for that builtin is ~0. The
two specs are one migration done twice-over: R1 deletes the IR volume, R1d
deletes the duplication that produced it.

## 4. Slices (each gated, native==interp via existing harnesses)

1. **The registry + `declare_one` loop**, seeded with the builtins *already*
   extern (`__axon_abs_i64`, `__axon_str_*`, channels, the new `__axon_dict_*`).
   Net: delete the matching hand-written `declare_builtins` blocks, replace with
   rows. No behavior change — the same `functions`/`fn_return_types` entries,
   proven by the full parity suite + gate. This is the load-bearing slice;
   everything after is volume.
2. **Migrate a batch of pure inline-IR builtins** (the str/math family
   `str_to_upper`/`str_to_lower`/`str_trim`/…, the `min/max_i32` group) from
   hand-emitted IR to axon-rt Rust + registry rows. Each batch: write the Rust
   fn (matching the interpreter's exact semantics — I-2), add the row, delete
   the inline block, gate. The parity harnesses are the safety net (a Rust port
   that drifts from the interp fails them).
3. **Cross-check / single-source the sig table**: derive (or assert at a test)
   that every `BUILTINS` row with an extern has a matching `BUILTIN_EXTERNS`
   row and vice-versa, so the two tables can't drift. Optionally generate the
   `BUILTINS` doc/sig from the registry where they overlap.
4. **Document the authoring path** in `CLAUDE.md` "Adding a New Builtin" (today
   a 5-step list) → "1. write the axon-rt fn, 2. add the registry row, 3. add a
   parity case" — and retire the inline-IR step.

## 5. Scope / honesty

R1d does **not** touch the I-2 oracle (interp stays the reference; codegen links
the same Rust the interp's semantics mirror — in fact a later step could have
the interp *call* the axon-rt fn directly, collapsing the double-impl entirely,
but that's out of scope here). It does **not** convert the bespoke call-site
builtins (`to_str`, arr_*/dict_* loops) — those earn their special-cases. The
win is concentrated where the boilerplate is: the ~120 straight externs and
their hand-written declaration blocks.

**Testable gates:** (1) slice 1 is a pure refactor — the entire test suite +
all 22 parity harnesses + `gate.sh --strict` stay green with zero behavior
change; (2) each migration batch keeps its builtins' parity harness green; (3)
the cross-check test (slice 3) fails if the two tables drift. Indexed in
`governance/specs/README.md`.
