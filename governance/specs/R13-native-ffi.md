# Native FFI — Capability-Gated Native Modules

**Spec ID:** `R13-native-ffi` (ties to `REQUIREMENTS.md` — new row; gates the full-platform vision: UI/GPU/mobile/3D)
**Status:** Slices 1–3 landed interp-side (2026-06-25, branch `r13-ffi`); codegen E0910-refused (interp-only); slice 4 (real `axon-gfx` + codegen link) deferred
**Risk class:** Structural
**Author / date:** cklaus, 2026-06-07

> **⚠ This spec changes invariants I-11 and I-12 (it does not merely preserve them).** Native code runs with
> the host's ambient authority, which the Axon capability boundary cannot enforce *into* — so this spec is
> also an **invariant-change proposal** per `ARCHITECTURE_INVARIANTS.md`'s process, not a pure feature. See
> §7 (the TCB-delta accounting) and §12 Q1/Q6. Read that before treating "by construction" as the safety
> story — for effectful and ambient-authority native code, it is not.

---

### 1. Motivation

The entire platform vision in `AI_Language_Plan.md` (GPU-rendered UI, mobile, 3D) is built on three
Rust crates — `wgpu`, `vello`, `winit`. Axon today has **no way to call external native code**: the only
externs that exist are the fixed `__axon_*` builtins hand-declared in codegen and implemented in the
`axon-rt` staticlib (`codegen/builtin_externs.rs`). There is no `extern` keyword, no foreign-function
surface, and — critically — no way to do it *without renegotiating the invariants that make Axon Axon*:
**I-2** (the interpreter is the reference semantics; codegen must match it), **I-5** (ownership is two-mode,
use-after-free impossible by construction), **I-11** (the capability boundary is real and *total*), and
**I-12** (self-modification cannot weaken the TCB). A raw C FFI escape hatch would shatter all four. This
spec defines the design that keeps I-2 and I-5 intact *by construction* and confronts the **I-11/I-12
weakening head-on** rather than hiding it: **native modules** — curated Rust shim crates that expose a stable
C ABI to codegen *and* a Rust impl to the interpreter, where (a) the boundary marshals only an unforgeable,
**affine** handle and the frozen scalar/str/array layouts, and (b) a native module receives capabilities
*passed in by the caller* rather than reaching for ambient authority — the object-capability discipline
Axon already uses, extended *across* the FFI line instead of stopping at it.

### 2. Requirement link

New requirement row (proposed **R13 — Native FFI**). It is the unbuilt gate beneath the "full-platform
vision ≈ 15%" line in `REQUIREMENTS.md:41` ("UI/GPU/mobile/3D … remain greenfield"). Acceptance criterion
it must satisfy: *an Axon program can drive a native Rust library (the proof target is a `wgpu` window with
one cleared frame) through a declared, capability-gated interface, and the same program type-checks and runs
identically on interpreter and native codegen (I-2) or fails closed with a clean diagnostic where the
interpreter cannot provide the capability.*

### 3. Surface (what the user writes)

There is **no raw `extern "C"` in user Axon.** The user imports a **native module** — a Rust shim crate
registered with the compiler — and calls its functions like builtins. The shim author writes Rust; the Axon
user writes Axon. This is the deliberate fork (see §12): arbitrary C FFI is rejected; curated native modules
are the only surface.

```axon
// Common case — import a native module, call it like any builtin.
use native::gfx                       // resolves to the registered `axon-gfx` shim crate

@[contained(gfx: any)]                // capability grant required to import a native::* module (E1004 otherwise)
fn main() -> i64 {
  let win  = gfx::window_open(800, 600, "axon")   // -> Window  (opaque handle)
  let surf = gfx::surface(win)                     // -> Surface (opaque handle)
  gfx::clear(surf, 0.1, 0.1, 0.12, 1.0)           // r,g,b,a
  gfx::present(surf)
  gfx::window_close(win)
  0
}
```

```axon
// Error case 1 — no capability grant. Importing a native module is an I/O capability.
use native::gfx
fn main() -> i64 { gfx::window_open(1,1,"x") }     // E1004: `native::gfx` requires a `gfx` capability grant
```

```axon
// Error case 2 — passing a value the boundary ABI forbids (a non-handle struct by value).
use native::gfx
type Big = { a: i64, b: str, c: f64 }
fn main() -> i64 { gfx::window_open_ex(Big{...}) } // E1701: type `Big` is not FFI-representable at the native boundary
```

**Shim-author surface (Rust, not Axon)** — a native module is a crate that exports a registration manifest.
This is the *generalization of `BUILTIN_EXTERNS`*: instead of one hardcoded table, native modules contribute
rows.

```rust
// axon-gfx/src/lib.rs  — authored once, in Rust, by the shim maintainer.
axon_native_module! {
    name: "gfx",
    capability: "gfx",                         // the @[contained] grant key
    effects: &["IO"],                          // effect row contributed to callers (Phase-6 bridge)
    functions: [
        // axon name        symbol                 params (FFI types)        ret
        fn window_open(w: i64, h: i64, title: str) -> Handle = "__axon_gfx_window_open",
        fn surface(win: Handle)                     -> Handle = "__axon_gfx_surface",
        fn clear(s: Handle, r: f64, g: f64, b: f64, a: f64) -> Unit = "__axon_gfx_clear",
        fn present(s: Handle)                        -> Unit   = "__axon_gfx_present",
        fn window_close(win: Handle)                 -> Unit   = "__axon_gfx_window_close",
    ],
}
// Each symbol is a plain `#[no_mangle] extern "C"` fn in this crate, exactly like axon-rt's builtins.
// The SAME crate is compiled into BOTH the native link (for codegen) and the compiler binary
// (for the interpreter to call in-process) — one impl, two engines, I-2 by construction.
```

### 4. Semantics (what it does)

The FFI boundary ABI is deliberately **narrow** — the same shapes already proven across the `axon-rt`
boundary (I-6 frozen layouts), plus one new opaque `Handle`. No arbitrary user structs cross the line in v1.

| Input class | Behavior |
|---|---|
| `use native::M` with capability granted | Resolver loads module `M`'s manifest; its functions enter scope under `M::`. Codegen declares each symbol as an extern (same path as `BUILTIN_EXTERNS`) and links the shim staticlib. Interpreter dispatches each call to the registered in-process Rust fn. |
| `use native::M` **without** a matching `@[contained(M: …)]` (or effect grant) | **E1004** at check time — importing a native module is a capability acquisition; an ungranted import fails closed (I-11). No link, no run. |
| call with scalar args (`i64/i32/f64/bool`) | Passed by value, identical ABI on both engines. |
| call with `str` / array args | Passed by the frozen `{i64 len, ptr data}` ABI (I-6); on wasm32 the scalar-expanded form (R7-targets §12 Q6). Interp passes the equivalent Rust `&str`/slice. |
| call returning/taking a **resource** `Handle` (a `Window`, `Surface`, file, socket) | An **opaque, affine handle** = `{i64 tag, i64 payload}` where `payload` is an index into a per-module handle table (NOT a raw pointer — a raw pointer would let Axon forge native pointers, breaking I-4/I-11). A resource handle is `own` (move semantics): it is **consumed** by passing it to a consuming fn (`window_close(win)`) and the borrow checker rejects any later use — **use-after-free is a compile-time error (I-5), not a runtime liveness check.** Borrowing a handle to a non-consuming fn (`clear(ref surf, …)`) leaves the owner valid. Not `Copy`, no arithmetic, unforgeable. |
| call returning/taking a **value** handle (an interned color, a font id — no owned resource behind it) | A `Copy` handle, same `{tag,payload}` layout but with *no* drop obligation. The manifest marks which handle types are values vs resources; only resource handles are affine. |
| call passing a non-FFI-representable type (user struct by value, `Option<T>`, closure, generic `T`) | **E1701** at check time. The representable set is exactly: scalars, `str`, `[T]` of scalars, `Handle` (value or resource), `Unit` return. |
| a resource handle that goes out of scope without being consumed | Borrow-checker **E06xx** (the existing "unconsumed `own` value" class) — a leaked native resource is a *compile-time* diagnostic, same as a leaked owned Axon value. (The handle table also drops it as a backstop, but the contract is static.) |
| a *forged or stale* handle reaches the boundary anyway (e.g. via `unsafe`-equivalent or a codegen bug) | The shim's handle table returns a poisoned-slot error → the call returns a `Result::Err` or panics-gracefully (exit 101, I-4). The boundary never aborts the host. This is defense-in-depth *beneath* the static affine guarantee, not the primary mechanism. |
| interpreter run where the module's native backend is unavailable (e.g. no GPU/display, headless CI) | The module's interp impl returns a clean `Err`/refusal, surfaced as a graceful panic or a `Result` — **never** a segfault. Headless = capability-absent, handled like any missing capability (I-9, no silent success). |
| native codegen of a call whose arg/ret is outside the representable set | **E1701** refused at check time — never reaches codegen (mirrors the existing E0910 out-of-subset refusal pattern). |

**The parity contract (I-2) — and its limit.** Because the shim crate is compiled into both the linked
binary *and* the compiler process, "interp vs codegen" is "same Rust fn, two call sites." Divergence is
possible *only* in the **marshalling layer** — the two engines reach that one Rust fn by different paths (the
interpreter passes in-process `Value`s; codegen passes the C ABI from LLVM), and that boundary is precisely
where the wasm i64↔i32 saga lived (R7-targets §12). So "parity by construction" means *modulo marshalling*,
and marshalling is the hard part: it gets the fuzzer. Native modules join the `R1f` differential-parity
corpus for their **pure / value-returning** calls.

But an *effectful* call (`gfx::clear`, `present`) has **no diffable observable output** — the effect is
pixels, a window, a buzz, not stdout/exit/return. For these, byte-identical Rust execution is trivially true
and tells us nothing about *program* semantics, so the differential-output oracle does not apply. **I-2 is
restored for effectful modules by a call-trace oracle**, modelled on R3's AI-call provenance: each native
call appends `(module, fn, marshalled-arg-hash, handle-ids)` to a provenance trace, and parity = the two
engines produce the **identical call trace** for the same program. This is a known, named limitation with a
mechanism — not a silent escape from the verification framework. (Pixel/visual correctness of a *module* is
the shim author's responsibility inside the TCB, not an Axon-level I-2 claim.)

### 5. Type rules

New surface types, threaded through HM (`infer.rs`) and the checker:

- **`Handle`** — an opaque, nominal type per native module (`gfx::Window`, `gfx::Surface` are distinct
  nominal handles so the checker rejects `clear(win, …)` when `surface` was wanted). Internally all handles
  share the `{i64,i64}` layout but unify only with themselves. No subtyping, no arithmetic. A handle is one of
  two kinds, declared in the manifest:
  - **resource handle** — `own` (affine). Threads through the existing borrow checker exactly as an owned
    struct does: consumed by a consuming fn, borrowable by `ref`, unconsumed-at-scope-end is the existing
    E06xx leak diagnostic. **This is what makes native use-after-free a compile error (I-5), not a runtime
    check** — the spec's single most important type rule.
  - **value handle** — `Copy`, no drop obligation (interned/immutable native values).
- **FFI-representable predicate** — a new check `is_ffi_repr(ty)` returning true only for the §4 set. Used at
  every native-call arg/ret site. This is the type-system half of E1701.
- **Effect-row bridge (Phase-6).** A native module declares an effect row (`effects: &["IO", "Net"]`); a
  call to it contributes those effects to the caller's row. An un-annotated caller importing an `IO` module
  is the existing **E1310** subsumption error — no new mechanism, the anti-laundering walker already covers
  it transitively.
- **Capability bridge (Phase-4/R6/R11) — attenuated, not all-or-nothing.** `@[contained(gfx: …)]` extends the
  existing `ContainedSpec` grammar with native-module grant keys. The `collect_caps_expr` import-edge walker
  (already closed against laundering, see `import-edge-walker-gap.md`) treats `use native::M` as a capability
  edge. Crucially, the grant is **structured, not `any`** — a module declares named sub-capabilities and the
  grant names a subset (`@[contained(gfx: [present, clear])]`), and these are **R11-mintable/attenuable** like
  any other capability (a sub-Principal can be handed `gfx: [present]` only). A native module that itself
  performs `net`/`fs` declares those as *its own* `@[contained]` requirements, which compose into the
  caller's via the existing host-allowlist attenuation — `net` through a native module is **not** a blanket
  grant; it carries the same host allowlist `@[contained(net: […])]` already enforces. (`gfx: any` appears in
  the §3 example only as shorthand for "all of gfx's sub-caps"; it is not the only form.)
- No changes to Option/Result/generics inference; native fns are monomorphic and non-generic in v1.

### 6. Error codes

**⚠ Code-band correction (landed 2026-06-25).** The draft proposed **E1700–E1704**, but R17 (just landed,
merge `4ed9fcd`) took the entire **E170x** band: E1700 (unsafe-outside-substrate), E1701 (HAL-without-cap),
E1702 (freestanding-no-entry), E1703 (surface-reaches-Hal), E1704 (`@[no_alloc]` heap), E1706
(atomic-ordering). R13 therefore uses the next free band, **E18xx**, verified against `error.rs` before
reserving (I-14). The implemented mapping:

| Code | Trigger | Message shape |
|---|---|---|
| E1800 | `use native::M` for an unregistered module name | `unknown native module 'M' — no shim crate registered under that name` |
| E1801 | native call arg/ret type not FFI-representable (also arity) | `type 'T' is not FFI-representable at the native boundary (allowed: scalars, str, [scalar], Handle)` |
| E1802 | handle of type/module A passed where B's handle expected | `expected handle 'B::H', found 'A::H' — handles do not cross modules` |
| E1803 | arithmetic / indexing / forging on a `Handle` | `'Handle' is opaque — it cannot be used in arithmetic / indexed` |
| (reuse) E1004 | `use native::M` call without a matching `@[contained(M: …)]` grant | `'native::M' requires a 'M' capability grant — add @[contained(M: any)]` |
| (reuse) E0601 | resource `Handle` used after being consumed (move-after-move) | the existing borrow-checker "use after move" message |
| (gap) | resource `Handle` unconsumed at scope end (leak) | **NOT enforced this slice** — the borrow checker is move-tracking, not linear must-use; an unconsumed handle surfaces only as W0006 (unused binding) + the runtime handle-table backstop drops it. A hard must-consume leak diagnostic is deferred (it would need a new linear-use pass affecting all `own` values, out of R13 scope). |
| (reuse) E1310 | native module's effect not declared in caller's closed row | (existing effect-subsumption message) |

The use-after-consume case reuses the existing borrow-checker **E0601** (handle moves go through the
standard `own`/affine machinery — the spec's preferred option) rather than minting a new code. The
capability-denied import reuses **E1004**; the effect-not-declared case reuses **E1310** (the existing
anti-laundering subsumption walker). A native call reaching the codegen pipeline is refused at emit with
**E0910** (interp-only this slice — see §11; sound-by-refusal, same as `host_await`).

### 7. Invariants touched

**Preserved by construction:**

- **I-2 (parity)** — *preserved, modulo the marshalling layer*: one Rust impl, two engines, with the
  marshalling boundary fuzzed (pure calls) and trace-checked (effectful calls). See §4's parity-contract note
  — this is weaker than a flat "preserved" and the spec says so.
- **I-5 (ownership, no use-after-free)** — *preserved*: resource handles are affine (`own`), so native
  use-after-free is a **compile-time** error via the existing borrow checker. This is the design choice that
  keeps native resource management inside Axon's ownership model instead of a GC-adjacent runtime liveness
  check.
- **I-4 (never abort the host)** — *preserved*: the boundary marshals errors as `Result`/graceful panic;
  handles are unforgeable indexed slots, so Axon cannot pass a bad pointer that segfaults the host. Forbids
  raw-pointer FFI.
- **I-6 (frozen IR layouts)** — *preserved*: reuses `str`/array layouts; adds `Handle = {i64,i64}` as a new
  frozen layout (this spec is its definition).
- **I-9 (no silent success)** — *preserved*: a missing native capability (headless GPU) is a clean refusal,
  not a no-op.

**⚠ CHANGED — this section is the invariant-change proposal (`ARCHITECTURE_INVARIANTS.md` process):**

- **I-11 (capability boundary is *total*)** — **weakened from *enforced* to *enforced-at-the-edge +
  trusted-within*.** The capability check gates *whether Axon may call a native module*; it does **not** and
  cannot enforce *what the module's Rust does* — native code runs with the host's full ambient authority
  (`gfx` could, absent mitigation, open a socket). I-11's "total" no longer holds for code behind the FFI
  line. **Mitigations that bound the damage (in increasing strength; v1 ships at least #1+#3):**
  1. **Object-capability passing into the module** — a native module receives only the handles/capabilities
     the caller hands it and has *no* Axon-level path to ambient `fs`/`net`/`exec`; a module that needs them
     declares its own `@[contained]` requirements, attenuated and composed into the caller's grant (§5). This
     extends Axon's ocap discipline across the FFI line rather than stopping at it.
  2. **Out-of-process / wasm-sandboxed native modules** — the strongest form: the OS (or a wasm sandbox)
     bounds the module so the guarantee is *enforced*, not *trusted*. Deferred past v1 (IPC/marshalling cost),
     but the design keeps the handle-table indirection that makes it possible later without a surface change.
  3. **Attenuated, R11-minted, budgeted native-module capabilities** (§5) + content-addressed registry (below)
     — a module can be handed strictly less than the parent holds, and the grant is auditable.
- **I-12 (self-mod cannot weaken TCB)** — **changed: native modules *are* new TCB, so the TCB is no longer
  fixed-by-construction; it grows by an explicit, gated act.** A self-improving pass (R10) **must not** be
  able to add a native module — registration is a privileged, multi-sig, out-of-band act (Q4), and the
  registry is content-addressed (R6 lockfile) so the set of trusted native code is pinned and a graduated
  pass cannot mint new ambient authority for itself. R10's G2 capability-diff gate must treat "introduces /
  changes a `use native::M`" as a TCB-expanding change that **fails the gate** unless separately authorized.

**TCB-delta accounting (mandatory, per the I-12 change).** This requirement is the first that pulls
third-party native code into the trusted base. Each native module's spec must state the crates it adds to the
TCB and their transitive footprint. For the v1 `gfx` module that is, at least: `wgpu`, `winit`, `vello`, and
their transitive C/system-driver dependencies (Vulkan/Metal/DX12 loaders) — on the order of hundreds of
thousands of lines of non-Axon code that now sit *inside* the boundary Axon's whole value proposition is
about keeping small. **This number is a first-class cost of the platform vision and belongs in the
governance ledger, not buried in a dependency tree.** The mitigation strategy (ocap-in, sandbox-later) exists
specifically to bound what that code can reach despite its size.

### 8. Test plan (maps 1:1 to §4)

- [ ] Unit: `is_ffi_repr` accepts the allowed set, rejects user-struct/Option/closure/generic.
- [ ] Unit: handle nominal-distinctness in `infer.rs` (gfx::Window ≠ gfx::Surface).
- [ ] Integration: a **mock native module** (`axon-mock-native`, no GPU — just an in-memory counter) imported
      and called; both engines produce identical output.
- [ ] CLI e2e: `axon run` (interp) and `axon build && ./bin` (codegen) on the mock-module program → identical
      stdout + exit code.
- [ ] Adversarial: ungranted import → E1004; cross-module handle → E1702; handle arithmetic → E1703;
      non-repr arg → E1701; use-after-free handle → graceful Err (not segfault).
- [ ] Property (invariant I-4): fuzz handle payloads with garbage indices → always graceful, never abort.
- [ ] Parity (interp↔codegen): the mock module joins `scripts/fuzz_parity.sh`'s corpus.
- [ ] Journey: the `wgpu` proof target (window + clear + present) builds and runs natively (gated; needs a
      display — runs in the manual/`#[ignore]` tier until CI has a GPU, like the AOT-wasm gates).
- [ ] Red test that must fail first: `native_call_without_capability_is_E1004`.

### 9. Acceptance criteria (the done gate)

- [ ] Test `mock_native_module_interp_codegen_parity` passes (one impl, two engines, identical output).
- [ ] Test `native_import_requires_capability` passes (E1004 on ungranted import).
- [ ] Test `non_ffi_repr_arg_refused` passes (E1701 at check time).
- [ ] Test `handle_is_unforgeable` passes (E1703 + graceful Err on bad handle, never a segfault).
- [ ] The mock native module is in the differential parity fuzzer corpus and green.
- [ ] (Stretch / manual tier) `examples/gfx/window_clear.ax` opens a wgpu window and clears one frame on
      native, behind a `#[ignore]`d journey test until CI has a GPU.

### 10. Performance budget

The boundary is a plain C call — zero marshalling for scalars/handles, the existing `{len,ptr}` pass for
str/array. No per-call allocation. Budget: a native call is within 2× of an `axon-rt` builtin call (guarded
by the same micro-bench harness R10 G4 uses). Handle-table lookup is O(1) (slab index).

### 11. Rollout & rollback

**Status (landed 2026-06-25, branch `r13-ffi`):** Slices 1–3 LANDED interp-side; codegen is honest-stop
E0910-refused (interp-only this iteration).

- Ships in slices, each independently revertible:
  1. **LANDED — Handle type + `is_ffi_repr` + E18xx codes** (front-end). The opaque nominal `Handle` is
     carried as `Type::Deferred("native:<module>:<name>:<r|v>")` (nominal-distinct, non-`Copy` for resource
     handles → affine via the existing borrow checker); `is_ffi_repr` is the E1801 predicate; arithmetic /
     indexing on a handle is E1803.
  2. **LANDED — registry + GPU-free `native::gfx` mock + dual-engine plumbing.** The single source of truth
     is `crates/axon-core/src/native.rs` (a static `NativeModule`/`NativeFn` registry, the generalization of
     `BUILTIN_EXTERNS`). The interpreter dispatches each `gfx::*` call to the in-process `GfxMock` (a frame
     counter + per-module slab handle table — NO wgpu/winit/GPU). A full round-trip
     (`window_open → surface → clear → present → frame_count → surface_close → window_close`) runs under
     `axon run`. A consuming call moves the handle (affine); a forged/stale slab index is a graceful `Err`,
     never a host abort (I-4). **Codegen: E0910-refused at emit** (sound-by-refusal, same discipline as
     `host_await`) — the `axon-rt` mock-symbol link + handle `{i64,i64}` ABI marshalling is a deferred
     follow-up slice; the interpreter is the reference engine.
  3. **LANDED — capability + effect bridge.** `use native::gfx` without `@[contained(gfx: any)]` granting the
     module is **E1004** at check time (fail-closed; the grant is parsed into `ContainedSpec.native_grants`).
     A native call's `IO` effect bridges into a caller's closed effect row via the existing **E1310**
     subsumption + anti-laundering walker.
  4. **DEFERRED — `axon-gfx` shim crate** (wgpu/winit/vello), the first real module, AND the **codegen
     mock-shim link** (slice-2 codegen half). Gated, manual-tier journey test.
- Blast radius if wrong: contained to programs that `use native::*`. A bug cannot affect existing pure-Axon
  programs (the import path is the only entry).

### 12. Open questions

- **Q1 (the decisive fork — RESOLVED in this draft): raw C FFI vs curated native modules.** Resolved:
  **curated native modules.** Raw `extern "C"` in user Axon is rejected because it cannot preserve I-2 (the
  interpreter has nothing to link against), I-4 (raw pointers segfault the host), or I-11 (an arbitrary
  symbol bypasses the capability boundary). Native modules — dual-compiled Rust shims gated by capabilities —
  are the only surface that keeps all four invariants. This mirrors the *existing* `BUILTIN_EXTERNS`/`AxonHost`
  patterns rather than inventing a foreign mechanism.
- **Q2 (handles: raw pointer vs indexed table).** Resolved in §4: **indexed table** (unforgeable). Open
  sub-question: per-module table vs one global slab. Lean per-module (isolation; a module reset frees only
  its handles).
- **Q3 (callbacks — Axon closures called *from* native, e.g. winit's event loop) — THE central dependency,
  not a deferrable detail.** The GPU UI needs the native event loop to call back into Axon `update`/`view`.
  A *synchronous, bounded, non-reentrant* callback ABI already exists (`MILESTONE.md`: "runtime-callback …
  used by `dict_each`") — and that is sufficient **only** for the §9 proof target: one static frame
  (`window_open → clear → present → close`, no input, no loop). It is **not** sufficient for any interactive
  app. A winit loop / `requestAnimationFrame` / a mobile lifecycle is a *long-lived, reentrant, suspending*
  callback: Axon must **suspend** a computation, yield to the OS-owned loop, and **re-enter** it on an
  external event while native resources are live. That is a continuation/`resume` mechanism, and Axon's home
  for it is **Phase-6 algebraic effects + handlers + `resume`** — specifically the *unbuilt* half (per
  `CLAUDE.md`: "`resume`/shallow-continuation runtime" remains; "handlers erase to their body today"). See the
  **Dependencies** note below. v1 ships the static-frame ABI; the interactive event-loop bridge is **R-UI**
  and is **gated on the Phase-6 resume runtime**, stated honestly rather than waved through.

#### Dependencies — the two-tier reality of the platform vision

| Target | Needs |
|---|---|
| **Static frame** (clear the screen; §9 proof target) | R13 + the existing synchronous callback ABI. **No Phase-6.** |
| **Any interactive / event-driven / async-I/O app** (every real UI) | R13 **+ the Phase-6 `resume`/shallow-continuation runtime** (currently unbuilt). |

This corrects the earlier framing that "the GPU UI does not depend on phases 5–8." It is true for the
*type-system* parts of those phases and for a single static frame. It is **false** for interactive apps: the
inverted-control-flow event loop (here), browser async I/O (R7c Q4), and the mobile OS lifecycle (R14 Q3) are
the *same* problem, and its principled answer is the Phase-6 continuation runtime. The platform vision's true
critical path is **R13 → Phase-6 resume runtime → R-UI**, not R13 → R-UI.
- **Q4 (who may author/register a native module).** A native module is TCB. v1: only modules in the compiler's
  built-in registry (`axon-gfx`, `axon-platform`). User-authored native modules require the R6 content-address
  + audit pipeline and are out of scope for v1. **This is a hard gate, not a default.**
- **Q5 (threading).** wgpu/winit have main-thread requirements on some platforms. The handle table and event
  loop must pin to the thread that created them. Cross-thread handle use → E17xx or graceful Err. Detailed in
  R-UI.
