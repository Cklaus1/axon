# R1 native build — RESOLVED. Root cause: serde × codegen feature collision.

**Date:** 2026-06-01
**Status:** ✅ **The native codegen build finishes in ~4 seconds.** The blocker is gone.

```
cargo build -p axon-core --no-default-features --features codegen --bin axon
  → Finished in 4.07s. Produces a working `target/debug/axon` (134 MB) that
    runs the full codegen pipeline (verified: emits an LLVM .o for a test .ax).
```

(The ONLY remaining issue is a trivial linker flag — `cc` needs `-no-pie` for the
final link — which is unrelated to the build stall and is a one-line fix in
`codegen/link.rs`.)

---

## The root cause (definitively proven, corrects BUILD_DIAGNOSIS 1/2/3)

The stall was **NOT** LLVM codegen (BUILD_DIAGNOSIS.md), **NOT** mono-collection of
inkwell calls (BUILD_DIAGNOSIS_2.md), and **NOT** inkwell's trait hierarchy
(BUILD_DIAGNOSIS_3.md). Those were all looking at the *symptom* (mono-collection
recursion in `structurally_relate_tys`) and blaming the wrong input.

**The actual cause: a collision between two unrelated DEFAULT features.**

- `serde-json` derives `Serialize`/`Deserialize` on the deeply-recursive AST enums
  `AxonType`, `Expr`, `Pattern` (`ast.rs`) — with `#[serde(tag = "kind")]`
  (internally-tagged, which generates especially heavy recursive (de)serialization
  code).
- `codegen` monomorphizes over those **same recursive enums** while lowering.
- With **both** features on (the default), rustc's monomorphization collector
  recurses pathologically through the combined serde-derive + codegen
  instantiation of those recursive types — the ~900-deep `structurally_relate_tys`
  cycle. Neither feature uses the other (`codegen` has ZERO serde references — the
  one hit in `codegen/output.rs` is a doc comment). They just both default-on and
  collide on the shared recursive AST types.

## The proof (apples-to-apples, same crate-type lib target)

| Build | serde-json | Result |
|---|---|---|
| `--features codegen` | **OFF** | **Finished 3.2s** ✅ |
| `--features codegen,serde-json` | **ON** | hung (same `collect_items_rec` / `structurally_relate_tys` stall, killed at 200s) ❌ |
| `--features codegen --bin axon` | OFF | **Finished 4.07s, working binary** ✅ |

Same toolchain, same machine, same target, only feature flag differs. The live
gdb stall signature with serde-ON is identical to every prior diagnosis
(`collect_items_rec` 3332 deep). Removing serde makes it vanish entirely.

## Why every prior hypothesis failed (and why this one is right)

- **`CODEGEN_WRAPPER_PROTOTYPE.md`'s 18,000-inkwell-call repro finished in 15s.**
  This was the clue that should have redirected us: if inkwell volume were the
  cause, the prototype (far more inkwell calls, but NO recursive serde-derived
  types) would have stalled too. It didn't — because the prototype lacked the
  serde × recursive-enum interaction.
- **Migration (13 builtins), Lever 2 (204→0 direct inkwell calls), and bisecting
  out ALL of `declare_builtins` each changed the stall by NOTHING** — because none
  of them touched serde or the AST enums. They were all aimed at inkwell, which was
  never the trigger.
- **`-Znext-solver` SIGSEGV'd / timed out** — the new solver hit the same recursive
  serde-derived types and overflowed (`recursed 63 times` in `fold_ty`),
  corroborating a recursive-type cause, not a volume cause.

The bisection (gating builtins) is what proved "not the builtins"; the cheap
feature-toggle test is what found the real cause. Lesson logged: **toggle features
before refactoring** — the serde test was a 2-build, 5-minute experiment that
would have saved the three migration batches and the nearly-started crate-boundary
refactor.

## The fix (choose one)

1. **Simplest (zero code): build native without serde-json.**
   `cargo build -p axon-core --no-default-features --features codegen --bin axon`.
   The `parse` (JSON AST dump) and `lsp` commands need serde-json; native codegen
   does not. Ship two build profiles: `codegen` (native) and `serde-json` (tooling),
   not both at once.
2. **Cleaner (small code): gate the AST serde derives out of codegen builds.**
   The `#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]` on
   `AxonType`/`Expr`/`Pattern` is what collides. If `parse`/`lsp` can use a
   hand-written JSON path (or a separate non-recursive DTO), the derives can be
   dropped and both features can coexist. Larger change; not needed for native to work.
3. **Best (decouple): make `default = ["codegen"]` and move `serde-json` out of
   default**, OR move the JSON/LSP tooling into a separate crate/binary that depends
   on serde, while the core codegen path never does. Then the default native build
   just works.

**Recommended:** ship (1) now (it already works — update CLAUDE.md / CI to build
native with `--no-default-features --features codegen`), fix the `-no-pie` linker
flag in `codegen/link.rs`, and file (3) as the clean follow-up. R1's acceptance
("a native binary of examples/*.ax runs and matches the interpreter") is now
reachable — it was a 4-second build away the whole time.

## Impact on the rest of the roadmap

- **R1** unblocks from 40% — native builds work; only the `-no-pie` link flag +
  the corpus parity test remain.
- **R7 (targets), R10 (self-improving perf gate), R4 (codegen provenance)** — all
  had "blocked on R1" §12 entries. Those unblock.
- The three migration batches (abs_i64 … str_slice) and Lever 2 are **not wasted**:
  they're correct, behavior-preserving, reduce interp↔codegen drift (#33/#36/#37),
  and shrink IR. They just weren't the build fix. Keep them.
- BUILD_DIAGNOSIS.md / _2 / _3 should be read as a **diagnostic journey** — each
  narrowed the symptom correctly but mis-attributed the cause until the feature
  toggle. This doc is the resolution.
