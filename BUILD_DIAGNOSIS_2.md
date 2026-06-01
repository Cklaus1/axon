# Axon native-build stall — live-process diagnosis (CORRECTS BUILD_DIAGNOSIS.md)

**Date:** 2026-06-01
**Method:** `gdb` attach to the live `rustc` worker thread during a real
`cargo build -p axon-core` (codegen feature) on the reference machine
(32-core / 124 GiB, the same class BUILD_DIAGNOSIS.md used), plus
`-Ztime-passes`. This is the instrument BUILD_DIAGNOSIS.md lacked — its
`-Zself-profile` run was killed before flushing its string table, and
`gdb`/`perf` were "not installed". They are now.

**Verdict in one line:** The stall is **NOT in LLVM codegen.** It is in
**`rustc_monomorphize::collector::collect_items_rec`** — monomorphization
*collection* — recursing ~3,380 frames deep, with the hot tip in the
**trait solver** (`normalize_projection_term` / `structurally_relate_tys` /
`Generalizer::tys`) normalizing inkwell's deeply-generic associated types.

---

## 1. The evidence (three consistent live samples + the full stack)

The worker thread (one thread at ~93% CPU, all others idle — the documented
single-threaded shape) had a **4,757-frame** stack. Root → tip:

```
run_compiler
  codegen_and_build_linker
    encode_and_write_metadata → encode_metadata → encode_crate_root
      exported_generic_symbols_provider_local
        collect_and_partition_mono_items          ← the codegen-time query
          collect_crate_mono_items
            collect_items_rec   × ~3,380 deep      ← MONOMORPHIZATION COLLECTION
              ...recursive item references...
                normalize_projection_term          ← trait solving at each node
                structurally_relate_tys / Generalizer::tys
                  (operating on inkwell's generic IntValue/PointerValue/… types)
```

- **3,323 of ~3,380 frames are `collect_items_rec`** (counted across a sample).
- The deepest non-recursive frames pin the entry: `collect_and_partition_mono_items`
  invoked from `exported_generic_symbols` / `encode_metadata`.
- `-Ztime-passes` shows every *small* pass completing in milliseconds
  (`codegen_to_LLVM_IR` 0.07s, `LLVM_passes` 0.08s, `type_check_crate` 0.68s) —
  then the process sits inside the single `collect_and_partition_mono_items`
  span that **never prints** because it never completes. So even LLVM's passes,
  when reached, are fast; the wall-clock is consumed *before* them, in collection.

## 2. Why this corrects BUILD_DIAGNOSIS.md

BUILD_DIAGNOSIS.md concluded (§5): *"It is LLVM-IR generation … for the
inkwell-generic-heavy codegen module."* Its reasoning was sound but indirect —
it inferred "backend" from the `cargo check` (4.5s) vs `cargo build` (unbounded)
split and from `cargo llvm-lines` also timing out. That split is **real but was
mis-attributed**:

- `cargo check` is fast because it **does not run `collect_and_partition_mono_items`**
  — that query is triggered at codegen time (via `exported_generic_symbols` /
  metadata encoding), which `check` skips. So "check fast, build slow" does NOT
  isolate *LLVM*; it isolates *everything codegen-gated*, and **mono-collection
  is the first such thing, before any LLVM IR is generated for the giant functions.**
- `cargo llvm-lines` also times out because it, too, must run mono-collection
  first (it needs the monomorphized items to emit IR). It never reached IR
  emission — consistent with the stall being *upstream* of IR generation.

So the chain "check fast → build slow → llvm-lines slow" is all explained by
**mono-collection**, not LLVM. BUILD_DIAGNOSIS.md saw the right symptom and
named the wrong organ.

## 3. Why mono-collection is pathological here

`collect_items_rec` walks the call graph of reachable generic instantiations.
`codegen/builtins.rs` calls ~800 heavily-generic inkwell builder methods
(`build_int_add`, `build_call`, `build_extract_value`, …), each generic over
inkwell's value/type universe. Collecting their monomorphized closure means
**normalizing associated-type projections** (`Builder::build_*` returns
associated types) for thousands of distinct instantiations — and each
normalization recursively relates types (`structurally_relate_tys`) over
inkwell's nested generics. The recursion depth (~3,380) and the trait-solving
tip are the cost. It is super-linear in the number of distinct generic
instantiation *shapes*, concentrated in the two giant functions that inline
hundreds of these calls.

## 4. What this means for the R1 fix (migration to axon-rt)

**Good news — the migration attacks the right lever, for a better reason than we thought.**
Moving a builtin from inline inkwell-IR to an `extern "C"` axon-rt function
deletes **all of its generic inkwell instantiations** from axon-core's
mono-collection graph (an extern is a single non-generic declaration —
`add_function(name, None)`). So each migrated builtin removes its subtree from
the `collect_items_rec` walk. This reduces the *count of distinct generic
instantiations to collect*, which is exactly the super-linear input — **the same
asymptote argument the R1 spec made, but the mechanism is mono-collection +
trait-normalization, not LLVM-IR lowering.**

The `#[inline(never)]` wrappers already shipped (`CODEGEN_WRAPPER_PROTOTYPE.md`)
also help here and possibly *more* than their LLVM-IR-line measurement suggested:
each wrapper collapses N call-site instantiations of a generic inkwell method to
**one** instantiation (inside the wrapper). That directly shrinks the
mono-collection set. (The prototype measured IR *lines*; the real win may be in
*instantiation count* / collection time, which it didn't measure.)

**Caveat — the finish threshold is still empirical.** We've cut ~16% of
IR-builder calls (951→798). Whether that has cut enough *distinct
instantiations* to make collection terminate in reasonable time is unknown until
measured. The depth-3,380 recursion suggests there is still a large generic
closure to collect.

## 5. Concrete, higher-leverage levers this diagnosis unlocks

Because the cost is **mono-collection of generic inkwell instantiations**, not
LLVM, these become first-class options (some NOT previously implied):

1. **Keep migrating builtins to axon-rt** (the R1 plan) — each deletes its
   instantiation subtree. Prioritize the functions with the most *distinct*
   inkwell generic call shapes (`declare_builtins` ~649 calls,
   `declare_string_builtins` remaining). **This is still the main lever.**

2. **Reduce generic instantiation shapes via the wrappers** — extend the
   `#[inline(never)]` non-generic wrapper coverage to *every* remaining inkwell
   call in the giant functions. A non-generic wrapper is collected **once**;
   the generic call it replaces is collected per-distinct-type. This is cheap
   (mechanical) and directly targets the measured cost. Measure
   instantiation-count, not IR-lines.

3. **`-Zshare-generics=yes`** (already default in dev) and
   **`-Zinline-mir=no`** are worth A/B-testing now that we know it's a
   middle-end/collection cost, not LLVM.

4. **Split the two giant functions into many small `fn`s** — previously deemed
   low-value because "codegen-units can't split a function." But for
   *mono-collection* the relevant thing is that smaller functions with fewer
   distinct generic calls each have smaller collection subtrees, and the
   recursion is shallower per item. Worth re-testing in combination with (1).

5. **A `cargo check`-gated dev loop stays correct** (BUILD_DIAGNOSIS.md §6):
   collection only runs at codegen time, so `--no-default-features` /
   `axon-check` remains instant. Unchanged recommendation.

## 6. How to reproduce / re-measure

```bash
# Live root-cause sample (what this doc did):
RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 RUSTFLAGS="-Ztime-passes" \
  cargo build -p axon-core >/tmp/r1_trace.log 2>&1 &
# wait ~60s for axon_core's rustc to enter the grind, then:
RUSTC=$(pgrep -f 'rustc.*axon_core')
gdb -p $RUSTC -batch -ex "set pagination off" -ex "thread apply all bt" \
  | grep -E 'collect_items_rec|collect_and_partition|normalize_projection|structurally_relate'
# A worker stack dominated by collect_items_rec = mono-collection stall (this finding).
```

`scripts/r1_build_measure.sh` tracks the machine-independent progress metric
(IR-builder-call count) per batch. Add an instantiation-count proxy if pursuing
lever (2).

---

**Bottom line:** The native build is not hung and not in LLVM — it is grinding in
**monomorphization collection + trait normalization** of inkwell's generic
instantiations, ~3,380 frames deep. The R1 migration is the correct fix (it
deletes instantiation subtrees), and the already-shipped `#[inline(never)]`
wrappers help more than their IR-line metric showed. The open question is purely
empirical: how many more builtins must move before collection terminates — which
`scripts/r1_build_measure.sh` charts per batch.
