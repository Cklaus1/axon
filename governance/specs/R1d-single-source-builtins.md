# Tech Spec — R1d: Single-Source Builtins

**Status:** 🚧 Implementing (re-verified 2026-07-18) — **Slice 1 LANDED** (`b8f54fb`,
"BUILTIN_EXTERNS registry (R1d slice 1) — collapse 21 declare blocks"):
`crates/axon-core/src/codegen/builtin_externs.rs` exists exactly as designed, 25 registry rows,
`declare_one_extern`/`declare_builtin_externs` doing the iterate-and-declare this spec prescribes.
**Slice 2 landed a further batch 2026-07-18**: `sleep_ms`/`now_ms` (`daa01f7`) and `dict_merge`
(`eff8ce2`) were already in the registry; this session added the 7 f64 math intrinsics
`sqrt`/`pow`/`floor`/`ceil`/`exp`/`ln`/`log10` (all `llvm.*.f64` LLVM-intrinsic-backed, previously
a hand-written "Phase 3 math builtins" block in `codegen/builtins.rs` declaring the same functions
one `add_function` call at a time) — collapsed into 7 registry rows, the old block deleted. Verified
before migrating: none of the 7 have bespoke call-site lowering (a single `get_function("sqrt")`
reverse-dependency from `arr_std_f64` and the `sqrt_f64`/`floor_f64`/`ceil_f64` wrapper builtins
still resolves correctly, since `declare_builtin_externs()` always runs first in `declare_builtins`
regardless of which code path declares the underlying function). Re-verified: `cargo build -p
axon-core` clean; `codegen::builtin_externs::drift_tests` both PASS (32 rows now); the full
`scripts/fuzz_parity.sh` PASS including `sqrt`/`floor`/`ceil`/`exp`/`ln`/`log10` and the
`nan_sqrt_neg` edge case; `pow` (not in the fuzz corpus) checked manually via
`axon run`/`axon build` on `pow(2,10)`/`pow(2,0.5)`/`pow(-8,1/3)` — native matches interp exactly
incl. the NaN case. **Resolved 2026-07-18: the str/math family this slice originally named as its
next batch (`str_to_upper`/`str_to_lower`/`str_trim{,_start,_end}`) is confirmed NOT registry-
eligible, not merely unverified.** Read the actual codegen: each is a `str → str` out-param call
(`void __axon_str_to_upper(AxonStr, i64* out_len, i8** out_ptr)`) wrapped in a hand-built shim
function that allocas the two out-slots, calls the rt fn, then reassembles an `AxonStr` return via
`insert_value` — exactly the "bespoke call-site lowering" class `BUILTIN_EXTERNS`'s own top comment
excludes by name ("str_slice's out-param wrapper"). `ExternSig`/`declare_one_extern` only knows how
to declare-and-link a symbol, not synthesize an out-param-unwrapping function body; making this
batch registry-eligible would mean extending the registry's own capability model first (a new,
separate sub-slice), not a batch of ordinary row-adds. Also checked `i64_to_f64`/`f64_to_i64` as
alternative Slice 2 candidates: both fail differently — they're single-LLVM-instruction function
BODIES synthesized in codegen (not declarations of an existing external symbol), so they don't fit
the registry's declare-only model either, and converting them into real `__axon_*` externs would
add a genuine cross-compilation-unit call for what's currently one hardware instruction — a
regression, same class as the bitwise-op rejection above. **Confirmed exhaustively 2026-07-18: no
further simple candidates remain.** Scanned every `"__axon_\w+"` symbol reference in
`codegen/builtins.rs` + `codegen/expr.rs` against the registry (not just a `BUILTINS`-name diff —
the actual linked symbols). Every unregistered `__axon_*` symbol falls into one of the categories
already ruled ineligible: str-returning out-param wrappers (`str_repeat`/`str_slice`/`str_reverse`/
`str_replace`/`str_split`/`str_join`/`str_pad_*`/`str_trim*`/`str_to_upper`/`str_to_lower`/
`str_digits_only`/`str_count`), dict operations (already-established bespoke out-param family),
panic helpers invoked mid-expression rather than as a top-level call (`__axon_*_panic`), or
impure/effectful builtins with complex `Result`/`Option`/`Uncertain` return layouts
(`ai_extract_*`, `goal_run`, `spawn`, `select`, `chan_*`, `parse_*`, `read_file`/`write_file`,
`exec`, `provenance_*`, `register_*`). **Slice 2's simple-batch migration (declare-only externs and
LLVM intrinsics) is COMPLETE for this registry model** — 7 math intrinsics this session plus the
pre-existing 25 rows cover every eligible candidate found. Further Slice 2 progress requires
extending `ExternSig`/`declare_one_extern` to support synthesized out-param-unwrapping wrapper
bodies, which is real, separate structural work (a new sub-slice), not a batch of ordinary
row-adds. **Slice 3 (drift cross-check test) LANDED 2026-07-18**:
`codegen::builtin_externs::drift_tests` — `every_extern_row_matches_a_known_builtin_with_the_same_arity`
asserts every `BUILTIN_EXTERNS` row's `axon_name` (the join key the field comment reserved for
exactly this) resolves to a `BUILTINS` entry with the same param count, and
`no_duplicate_extern_registry_rows` catches accidental double-registration — both PASS on the
current 25 rows. This is the one-directional half of the spec's "vice-versa" ask (registry → source
table); the reverse direction (does every eligible `BUILTINS` row have a registry entry) isn't
cleanly automatable without a marker for "which builtins use the straight-extern path vs. a bespoke
call-site special case" — left as a known gap, not silently skipped. **Slice 4 (CLAUDE.md doc
update) LANDED 2026-07-18**: `CLAUDE.md`'s "Adding a New Builtin" section now presents two paths —
the R1d fast path (`builtins.rs` row → axon-rt fn → one `BUILTIN_EXTERNS` row → parity test, 4
steps folding the old codegen-declare + infer.rs-return-type edits into the single registry row)
for a plain scalar/handle extern, and the original 5-step recipe kept verbatim for bespoke
call-site builtins (out-params, dict get/set/remove/keys, `to_str`) that the registry deliberately
excludes. All 4 slices now landed (Slice 2 is the one exception — genuinely partial, not "done";
see above). This header said "Draft" with zero indication that a third of the design already
shipped; same staleness class as R17/R21/R22/R23/R26/R27/R28/R29/R31/R32/R12/R14/R1b/R1c, caught by
the same outer-loop sweep (`EXECUTION_MODEL.md` §2) — genuinely partial, not a clean flip.
**Slice 2 correction 2026-07-20**: sizing "extend `ExternSig`/`declare_one_extern` for out-param
synthesis" (per the "further Slice 2 progress requires..." note above) found the 2026-07-18
"exhaustive `__axon_*` symbol scan" had one real miscategorization: `str_count` was bucketed with
the str-returning out-param wrapper family (`str_replace`/`str_slice`/etc.) purely by name
resemblance — its actual codegen never used the out-param dance at all (`__axon_str_count(AxonStr,
AxonStr) -> i64` was a straight two-str-arg-to-scalar call, the exact shape `str_index_of` already
had a registry row for). Migrated it (33rd row) and deleted the now-redundant hand-written block;
native==interp verified manually (5 cases incl. empty needle, empty haystack, overlapping matches
— all byte-identical). This does NOT mean the "extend the registry for out-param synthesis" work is
done or unnecessary — the other ~10 named candidates (`str_replace`/`str_slice`/`str_reverse`/…)
are genuinely out-param wrappers and still need that real, separate structural extension; this was
just the one candidate that turned out not to need it after all.

```spec-meta
id: R1d-single-source-builtins
status-claim: Implementing
depends-on: R1-codegen-build-unblock
blocks: none
blocked-by: none
supersedes: none
related: R1b-str-return-abi, R1c-dict-runtime
conflicts-with: none
reserves: none
evidence: cargo test --lib codegen::builtin_externs -p axon-core (2 tests, 33 rows, re-verified 2026-07-20); scripts/fuzz_parity.sh (Slice 2 math batch); CLAUDE.md "Adding a New Builtin" (Slice 4); str_count native==interp manual verification 2026-07-20 (5 cases). Slice 2's simple-batch scope found one more real candidate 2026-07-20 (str_count, a miscategorized non-out-param builtin) after the 2026-07-18 scan missed it; the ~10 genuine out-param wrapper builtins still need the registry's out-param-synthesis extension, unstarted, separate structural work, not a batch-migration gap
```

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
