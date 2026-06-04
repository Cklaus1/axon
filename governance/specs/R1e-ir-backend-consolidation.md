# Tech Spec — R1e: IR-Backend Consolidation (one IR-emission path)

**Status:** 🟡 Slice 1 LANDED (2026-06-04, `dfe4836`) — the dead `IR` trait +
arena shim is DELETED (~1200 LoC removed: `codegen/ir.rs` gone, `ir_inkwell.rs`
1003→73 lines). Single real IR path = `self.ir.{context,module,builder}` +
`build_wrappers::w_*`; stale IR.3/IR.4 comments rewritten; MIGRATION.md /
IR_REARCH.md marked SUPERSEDED. **Drift tripwire LANDED** (`cli_run.rs`
`r1e_dead_ir_trait_stays_deleted` + `r1e_direct_ir_emission_stays_confined`):
source-invariant tests fail if `ir.rs`/`impl IR`/`IRValue…` reappear or if a NEW
file grows direct `.builder.build_*` (a second IR path spreading) — verified to
bite, then pass. Remaining: slice 2 — converge the 165 `expr.rs` direct-`build_*`
stragglers onto `w_*` (the tripwire allowlist shrinks to
`[build_wrappers.rs, ir_inkwell.rs]` as that completes).
— the cleanup that retires a dead abstraction
and collapses the codegen IR surface to ONE path.
**Requirement:** R1 (native pipeline). Sibling to `R1d-single-source-builtins.md`
(which single-sources *builtins*); this one single-sources the *IR-emission
mechanism* itself. Both are structural-debt paydown that compounds: R1d removes
duplicate builtin *definitions*, R1e removes duplicate ways to *emit IR*.

**Decisive fork:** *codegen has accumulated THREE ways to emit the same LLVM IR
— which one survives, and what gets deleted?*

The three coexisting surfaces (measured 2026-06-04 across `codegen/*.rs`):
1. **`build_wrappers::w_*`** — 1787 call sites. `#[inline(never)]`, non-generic
   wrappers over `Builder::build_*`. The de-genericization that actually helped.
2. **`self.ir.builder.build_*`** — 154 direct generic call sites (all in
   `expr.rs`; 69 `build_struct_gep`, 41 `build_gep`, 20 `build_select`, …). The
   un-migrated legacy generic path.
3. **The `IR` trait + `InkwellBackend: IR` arena shim** (`ir.rs` + the trait
   `impl` in `ir_inkwell.rs`) — **0 real call sites**. Handle-arena abstraction
   (`self.ir.iadd(a,b) -> IRValue(u32)`) that codegen was *supposed* to migrate
   to. It never happened.

- **(a) Finish IR.4** — migrate all 1941 sites onto the `self.ir.*` trait
  (arena handles), delete `build_wrappers` *and* the direct `.builder.build_*`
  calls, leaving the `IR` trait as the single seam (the original IR.1–IR.5 plan).
- **(b) Adopt `build_wrappers` as the one path, delete the dead `IR` trait/impl
  shim** — finish migrating the 154 stragglers in `expr.rs` onto `w_*`, then
  delete `ir.rs` + the `impl IR for InkwellBackend` block (keeping the
  `InkwellBackend` *struct* purely as the `context/module/builder` holder).

**→ Resolve: (b).** The evidence is one-sided. The `IR`-trait path is **not
half-finished — it is abandoned and partly non-functional**, while
`build_wrappers` is the *proven* fix that is already 92% of the surface
(1787 / 1941). Three independent pieces of in-tree evidence settle it:

1. **The trait shim has zero real callers and is dead code by its own
   declaration.** `ir.rs:46` is `#![allow(dead_code)] // Trait is unused until
   the impl phase`; `ir_inkwell.rs:35` is `#![allow(dead_code)] // Used once the
   codegen modules migrate`. A grep for `self.ir.{iadd,isub,alloca,load,store,
   call,const_i64,t_i64,ret}` across every real codegen module
   (`asi/builtins/expr/match_pat/mod/option_result/output/types/link.rs`)
   returns **nothing**. The only callers of the trait methods are the two unit
   tests inside `ir_inkwell.rs` itself.
2. **The trait impl is structurally incapable of running codegen as written.**
   Its read-side methods are stubs that return a sentinel: `ty_of` →
   `IRType(0)` with `TODO(IR.4): proper type round-trip`; `t_get_named_struct`,
   `get_function`, `fn_param`, `fn_as_value`, `global_as_value` all return
   `IRValue(0)`/`IRFunction(0)` "placeholder — full lookup deferred";
   `phi_add_incoming` is an empty stub (`TODO(IR.4): add a separate phi arena`);
   `const_array` builds an `undef` array (`TODO(IR.2): finish`). Codegen's
   `expr.rs`/`match_pat.rs` rely on real struct-GEP type round-trips and phi
   incoming edges — both of which this impl drops on the floor. Migrating onto
   it would require *finishing* it first, i.e. the IR.4 work is not 90% done, it
   is ~0% done and the easy 10% is what exists.
3. **The wrapper file's own header records that the trait approach was tried and
   failed for the stated goal.** `build_wrappers.rs:22` — *"A trait surface (the
   earlier `ir_inkwell.rs` shim) did NOT help because its methods stayed generic
   / got inlined back, so the instantiation count was unchanged. These wrappers
   are structurally different: one concrete IR shape each, `#[inline(never)]`, so
   `Copies = 1` per wrapper."* The trait was the *first*, rejected attempt at the
   build-speed problem; `build_wrappers` is the *replacement* that worked.

And the second, smaller fork — **are the `w_*` wrappers still needed now that R1
resolved the build?**

- **(a) Inline them back** — R1 (`BUILD_RESOLVED.md`) made the build ~3–4s, so
  the wrappers are no longer load-bearing; revert to plain `self.ir.builder
  .build_*` everywhere for readability.
- **(b) Keep the `w_*` wrappers.**

**→ Resolve: (b) keep them.** R1's resolution was a *serde × codegen
feature collision* (`BUILD_RESOLVED.md:1` — "Root cause: serde × codegen feature
collision"; the codegen feature has **zero** serde references). It did **not**
retract the inkwell-monomorphization cost the wrappers address —
`build_wrappers.rs` documents an independently-measured *−43% LLVM-IR, −36% RSS,
~1.7–3× faster* from the wrappers, on the codegen axis, orthogonal to the serde
axis. ROADMAP §7.5 (lines 573–574) records the wrappers as a *shipped*
mitigation, not a stopgap. Inlining 1787 generic instantiations back into the
giant `declare_builtins`/`emit_expr` functions would re-inflate exactly the
monomorphization pile-up the wrappers flattened — a regression with no upside
beyond cosmetics. The wrappers ARE the single concrete path; keep them and
finish converging onto them.

---

## 1. The state the comments claim vs. the state on disk

`codegen/mod.rs:258-266` documents an "IR.3 prep" in which *"both this field
[`self.ir`] AND the legacy `context/module/builder` fields are populated;
modules migrate one at a time per `MIGRATION.md`. IR.4 will remove the legacy
fields once every caller has migrated."*

Three of those claims are **false as of 2026-06-04**:

| Claim in the comment | Reality on disk |
|---|---|
| "legacy `context/module/builder` fields are populated" | The `Codegen` struct has **no** bare `context`/`module`/`builder` fields. `grep '^\s*(context\|module\|builder):' mod.rs` → none. The struct's only IR holder is `ir: InkwellBackend<'ctx>` (mod.rs:266). Every access is already `self.ir.context.*` / `self.ir.module.*` / `self.ir.builder.*`. **IR.4's stated job — "remove the legacy fields" — is already done; there are no separate legacy fields to remove.** |
| "modules migrate one at a time per `MIGRATION.md`" | **There is no `MIGRATION.md`** anywhere in the repo (`fd MIGRATION` → empty). The referenced migration tracker does not exist. |
| "IR.4 will remove the legacy fields once every caller has migrated to `self.ir.*`" | No caller has migrated to the trait-method `self.ir.*` surface. The migration that the field comment anticipates **never started**; a *different* migration (`build_wrappers`) happened instead and is 92% complete. |

So the IR.3/IR.4 narrative in `mod.rs` describes a plan (`ir.rs` §"Migration
plan", IR.1–IR.5) that was **superseded** by `build_wrappers` and never updated.
The honest status is below.

## 2. Honest status of each piece

- **`InkwellBackend` struct** (`ir_inkwell.rs:59`): **alive and load-bearing.**
  It is the single owner of `context` (`&'ctx Context`), `module`, and
  `builder` — exactly the "single Module per codegen run" that
  `Codegen::new` sets up via `adopt` (mod.rs:277). This is good and stays. Its
  five arena `Vec`s (`values`/`types`/`blocks`/`functions`/`globals`) and the
  `fn_types` table exist **only** to back the trait impl and are otherwise
  unused.
- **The `IR` trait** (`ir.rs:120`): **dead.** `#![allow(dead_code)]`, no real
  callers, never imported outside `ir_inkwell.rs`.
- **`impl IR for InkwellBackend`** (`ir_inkwell.rs:218`): **dead + incomplete.**
  Several methods are sentinel stubs (§Decisive-fork evidence #2). Only its two
  in-file unit tests (`add42_function_verifies`, `if_then_else_verifies`,
  `struct_alloca_load_store_verifies`) execute it.
- **`build_wrappers::w_*`** (`build_wrappers.rs`): **the real path.** 1787 sites,
  39 wrapper fns, the documented build-speed win, shipped per ROADMAP §7.5.
- **Direct `self.ir.builder.build_*`** (154 sites, all `expr.rs`): the
  **straggler** — the 8% of sites not yet routed through `w_*`. Mostly
  `build_struct_gep` (69) and `build_gep` (41), for which `build_wrappers`
  already has `w_struct_gep`/`w_gep`. These are a mechanical conversion, not a
  design gap.

Verdict in one line: **IR.4 is effectively DONE by accident (no legacy fields
exist), the `IR`-trait migration was ABANDONED in favor of `build_wrappers`, and
the only real remaining work is to finish converging the last 154 sites onto
`w_*` and delete the abandoned trait so there are not two-plus documented paths
that mislead the next reader.**

## 3. Target shape (post-R1e)

```
codegen/
  ir_inkwell.rs     ← InkwellBackend STRUCT only: { context, module, builder }
                       + adopt()/new(). No `impl IR`, no arenas. ~120 LoC.
  build_wrappers.rs ← the ONE IR-emission surface (w_* #[inline(never)]).
  ir.rs             ← DELETED (the abandoned trait + handle types).
```

Every IR-emitting line in codegen is one of:
- `build_wrappers::w_*(&self.ir.builder, …)` for `build_*` ops, or
- `self.ir.{context,module}.…` for type construction / function & global
  declaration (these are *not* generic-monomorphization hot spots — they are
  `i64_type()` / `add_function()` calls, cheap and fine as-is).

No third path. No `self.ir.builder.build_*` direct generic site survives. The
`mod.rs:258-266` field comment is rewritten to describe reality (one backend
struct, one wrapper surface) or deleted.

## 4. Slices (each gated; native==interp via the existing parity suite + `gate.sh`)

Every slice is a pure refactor — it changes *how* IR is emitted, never *what*.
The 22 parity harnesses + full test suite + `gate.sh --strict` are the safety
net: any slice that perturbs emitted IR semantics fails them.

1. **Delete the dead `IR` trait + impl (the load-bearing cleanup).** Remove
   `codegen/ir.rs`, the `impl IR for InkwellBackend` block and the five arenas /
   `fn_types` / `name_counter` / `store_*` / handle helpers from
   `ir_inkwell.rs`, and the `pub mod ir;` line + `IR` re-exports. Keep the
   `InkwellBackend` struct's `{context, module, builder}` + `adopt`/`new`. Port
   the three `ir_inkwell` unit tests to either build directly on
   `context/module/builder` or be dropped (they only ever tested the shim). Net:
   **−~600 LoC of dead code, one fewer documented IR path.** Zero behavior
   change — nothing called the deleted code. Gate: full suite + parity + `gate.sh`
   green, `cargo build -p axon-core` still ~3s.
2. **Converge the 154 `expr.rs` stragglers onto `w_*`.** Mechanical: each
   `self.ir.builder.build_struct_gep(…)` → `build_wrappers::w_struct_gep(&self.ir
   .builder, …)`, same for `build_gep`→`w_gep` (note: `w_gep` is `unsafe`,
   matching inkwell), `build_select`→`w_select`, `build_call`→`w_call`, etc. For
   the handful of ops with no existing wrapper (`build_bitcast` is the only one
   not in `build_wrappers` — 1 site), add the `#[inline(never)]` wrapper in the
   same shape as its neighbors. Do it in small batches (gep family, then select,
   then the singletons), gating after each. After this slice **the count of
   direct `self.ir.builder.build_*` sites is 0** — the verifiable done-signal.
3. **Rewrite the stale comments.** Replace `mod.rs:258-266` (the IR.3/IR.4
   "legacy fields / `MIGRATION.md`" narrative) with an accurate one: "`self.ir`
   is the single `InkwellBackend` owning the only module+builder; all `build_*`
   IR goes through `build_wrappers::w_*` for monomorphization control (ROADMAP
   §7.5); type/decl ops go through `self.ir.{context,module}`." Update `ir.rs`'s
   "Migration plan (informative)" — now in the deleted file, so it goes with it;
   leave a one-paragraph note in `build_wrappers.rs` or ROADMAP §7.5 recording
   that the trait approach was tried, measured not to help, and removed in R1e
   (so the next reader doesn't resurrect it).
4. **Add a drift tripwire (optional, cheap).** A `#[test]` (or a `gate.sh` grep
   line) asserting **zero** `self.ir.builder.build_` occurrences in
   `codegen/*.rs` outside `build_wrappers.rs` — so a future hand-emitted generic
   site can't silently re-introduce the second path. Mirrors R1d's "the two
   tables can't drift" cross-check.

## 5. Scope / honesty

- R1e does **not** change emitted IR, the runtime ABI, or any semantics — it is
  pure mechanism cleanup. The interpreter (I-2) stays the oracle; codegen still
  links the same `axon-rt`.
- R1e does **not** re-litigate the build-speed approach. `build_wrappers` won
  (measured), R1 resolved the *other* (serde) axis; this spec keeps both wins
  and just removes the abandoned third thing.
- R1e does **not** keep the `IR` trait "for a future cranelift/MLIR backend."
  That was `ir.rs`'s stated long-term rationale, but a *dead, partially-stubbed*
  trait with zero callers is not a usable seam — it is a maintenance trap that
  three comments already mislead readers about. If a second backend is ever
  built, a trait can be reintroduced *against real call sites that exist*, which
  is a sounder design than freezing a speculative, never-exercised interface
  now. Deleting it loses nothing real and removes the false "migration in
  progress" signal.
- Honest residue: after R1e there are still **two** legitimate (non-duplicative)
  surfaces — `w_*` for `build_*` ops and `self.ir.{context,module}` for
  type/decl ops. That is not a fork; they are disjoint operation classes (value
  emission vs. type/symbol declaration) and neither is a monomorphization hot
  spot in the way the `build_*` calls were. Collapsing those two into one is not
  a goal and would buy nothing.

**Testable gates:** (1) slice 1 deletes only code with zero callers — the entire
suite + 22 parity harnesses + `gate.sh --strict` stay green with zero behavior
change, and `cargo build -p axon-core` stays ~3s (no monomorphization
regression); (2) after slice 2, `rg 'self\.ir\.builder\.build_' codegen/ | grep
-v build_wrappers.rs` returns nothing — the single-path done-signal; (3) the
slice-4 tripwire fails if a direct generic `build_*` site reappears. Indexed in
`governance/specs/README.md`.
