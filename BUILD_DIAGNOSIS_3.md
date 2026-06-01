# Axon native-build stall — bisection result (FINAL root cause)

**Date:** 2026-06-01
**Method:** `#[cfg]`-gated bisection of the codegen passes + live gdb sampling on the
reference machine (32-core/124 GiB). Builds run timed; "finishes fast" vs "still
grinding at the same depth past 150s" is the signal.

**Verdict in one line:** The stall is a **tight recursive cycle in rustc's type
normalization (`structurally_relate_tys`, ~905 deep, only 62 unique stack
addresses across ~3,000 frames)** triggered by **inkwell's generic trait
hierarchy** — NOT by the volume of builtin/inkwell calls. Removing code does not
help; the cost is per-distinct-instantiation trait normalization that even a
minimal reachable inkwell surface drags in.

---

## 1. What the experiments proved (three independent removals, all NEGATIVE)

| Experiment | What was removed | Result |
|---|---|---|
| **Migration** (13 builtins → axon-rt) | ~16% of IR-builder calls | stall unchanged (BUILD_DIAGNOSIS_2.md addendum) |
| **Lever 2** (wrap all 204 direct inkwell calls) | all direct `self.ir.builder.build_*` | stall unchanged |
| **Bisect 1** (gate string/math/ai/asi sub-passes) | 4 builtin groups | still 3,184-deep `collect_items_rec` at 147s |
| **Bisect 2** (gate ENTIRE `declare_builtins`) | all 994 `.into()` + every builtin body | **depth 3,380 at 193s — IDENTICAL to the full build** |

Bisect 2 is decisive: with the **entire** `declare_builtins` compiled out (the
single largest inkwell-generic function, ~1,800 lines, 994 `.into()`s), the
mono-collection stall is **byte-for-byte the same depth/timing**. The builtins
are not the trigger. Neither is call *volume* — three separate volume reductions
changed nothing.

## 2. The signature: a tight recursive cycle, not broad collection

Live gdb on bisect-2's worker:

- **~3,000 frames, but only 62 UNIQUE stack addresses.** A broad
  "too many items to collect" stall would show thousands of distinct addresses.
  62 means **one small set of functions calling each other in a deep cycle** —
  a single pathological recursive *type normalization*, repeated.
- **905 of the frames are `rustc_type_ir::relate::structurally_relate_tys`** —
  rustc structurally comparing two types, recursing into their generic
  arguments, ~900 levels deep. The rest is `normalize_projection_term` /
  `normalize_canonicalized_projection` (associated-type projection).
- Entry: `collect_crate_mono_items → items_of_instance → collect_items_rec →
  normalize_erasing_regions → structurally_relate_tys (×905)`.

`structurally_relate_tys` recursing ~900 deep means rustc is normalizing a type
whose structure nests ~900 generic layers before bottoming out (or whose
projection chain does). That is the shape of **inkwell's trait hierarchy**:
`BasicValue: AnyValue`, `BasicType: AnyType`, the `BasicValueEnum`/`AnyValueEnum`
/`BasicMetadataValueEnum` conversions, and the `From`/`TryFrom`/`Into` blanket
impls between them. Each `.into()` on an inkwell value/type forces normalization
of a `<X as Into<Y>>::...` projection whose associated-type chain walks the whole
hierarchy. Mono-collection does this **once per distinct instantiation shape**,
and the hierarchy is deep enough that a handful of shapes blow the recursion up.

## 3. Why every "reduce the calls" lever failed (the mechanism, finally clear)

- Mono-collection normalizes **trait obligations per distinct generic shape**,
  not per call site. 800 calls of the same `build_int_add::<...>` shape cost the
  same normalization as 1. So cutting call *count* (migration, wrapping) is
  irrelevant — the *set of distinct inkwell instantiation shapes reachable* is
  unchanged as long as ANY code still uses inkwell's generic API.
- A `#[inline(never)]` wrapper is a new monomorphized item that **still contains**
  the generic inkwell call, so its shape is still collected and normalized.
  Wrapping relocated the shape; it didn't remove it.
- Gating builtins left `expr.rs`, `mod.rs`, `ir_inkwell.rs`, `match_pat.rs`,
  `option_result.rs`, `asi.rs` — all still calling inkwell — so the same shapes
  are reachable. Bisect-2 confirms: the trigger is in the **always-reachable
  shared codegen path**, and it's inkwell's hierarchy, not our code volume.

## 4. What this means — the levers that CAN work (and the ones that can't)

**Cannot work (falsified):**
- ❌ Migrating more builtins to axon-rt. (Bisect-2: removing ALL builtins = no change.)
- ❌ More `#[inline(never)]` wrapping. (Lever 2: 204→0 direct calls = no change.)
- ❌ Splitting giant functions. (Doesn't reduce distinct instantiation shapes.)

**Might work (untested, ranked by leverage):**
1. **Isolate ALL inkwell use behind a non-generic crate boundary.** Move every
   inkwell call into a small `axon-llvm` crate that exposes ONLY non-generic
   functions (concrete types in/out, no inkwell types in the public API). The
   main crate then never instantiates inkwell's generic hierarchy — those shapes
   are collected once, inside `axon-llvm`, where the giant functions don't live.
   This is Lever 2's principle **at the crate boundary**, which is where
   mono-collection actually partitions. The in-crate wrappers failed precisely
   because they're in the same crate; a crate boundary is the real firewall.
   **This is the highest-leverage structural fix and the natural next experiment.**
2. **`-Znext-solver` (nightly).** The hot frame IS the old trait solver's
   `structurally_relate_tys`/`normalize_projection_term`. The next-gen solver
   rewrites exactly this. First attempt (`-Znext-solver=globally`) **SIGSEGV'd**
   on inkwell's types (a solver bug, not our code) — but `=coherence` or a
   pinned newer nightly might handle it. Worth a few one-flag builds.
3. **Pin/patch inkwell or reduce its trait surface.** If a specific inkwell
   version's blanket impls are the depth driver, a newer/older inkwell, or a
   local `#[derive]`-free facade, could flatten the hierarchy. Needs the actual
   offending type named (see §5).
4. **Accept interpreter-first permanently.** `axon check`/`run` are instant;
   the test suite is green; R7/R10 already use the interpreter as reference.
   Native codegen becomes a "someday / CI-only" goal, not a blocker. **This is
   the pragmatic default if (1) is too costly.**

## 5. To name the EXACT offending type (next diagnostic step)

The 62-address cycle is small enough to symbolize fully with a **debug-assertions
rustc** or `RUST_BACKTRACE` on an `-Zincremental-verify-ich` ICE, OR by building
a 50-line standalone repro that calls the suspected inkwell conversion in a loop
and watching whether IT stalls. The standalone-repro path (à la
`CODEGEN_WRAPPER_PROTOTYPE.md`) is the cheapest: take one `BasicValueEnum`
round-trip through the `.into()` chain that codegen uses, instantiate it at N
distinct types, and measure collection time. If it reproduces, the exact
conversion is named and (1)/(3) become targeted.

---

**Bottom line:** We have now *definitively* ruled out the "reduce inkwell call
volume" family (migration, wrapping, splitting) via bisection — three batches of
that work did not move the stall, and gating 100% of builtins changed nothing.
The cost is rustc normalizing inkwell's deep generic trait hierarchy, per
distinct instantiation shape, in a tight ~900-deep recursive cycle. The only
structural fix with a real chance is a **non-generic crate boundary around all
inkwell use** (§4.1); the cheapest possible fix is **`-Znext-solver`** if a
non-crashing nightly exists (§4.2); and **interpreter-first remains a fully
valid place to stop** (§4.4).
