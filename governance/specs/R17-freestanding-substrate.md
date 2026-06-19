# R17 — Freestanding Substrate + Trusted HAL (bare-metal Axon)

**Spec ID:** `R17-freestanding-substrate` (new requirement row; depends on `R13-native-ffi.md`, `R7-targets.md`; extends ROADMAP §3 substrate/surface + §7 TCB; **reverses ROADMAP §2.3** — see §12 Q1)
**Status:** Draft — **COMMITTED (founder decision 2026-06-19; §12 Q1 resolved, ROADMAP §2.3 reversed).** Building. First slice = `R19-fixed-width-integers.md` (unsigned ints), then the trusted HAL + boot-under-QEMU (Slice 0).
**Risk class:** Structural (introduces the language's *only* `unsafe` surface; amends I-3/I-4/I-5/I-6, extends I-11/I-12)
**Author / date:** cklaus, 2026-06-12

> **One-line scope:** give Axon the *minimal, capability-gated, substrate-only* low-level escape hatch a
> from-scratch OS needs (raw memory, volatile MMIO, inline asm, atomics, layout control, a no-host runtime),
> **without** loosening safety for one line of `surface` code. The unsafe surface is small, enumerated, lives
> in the TCB, and is granted by capability — the seL4 / Rust-kernel / Singularity model, expressed in Axon's
> existing substrate + effect + Principal machinery.

---

### 1. Motivation

Axon today has **no `unsafe`** — by design (no null, ownership, Option/Result, "the checker forbids unsafe on
the surface"). That is correct for an application language and is most of what makes a *safe* OS tractable.
But a kernel must, in a bounded core, touch raw hardware: read/write a device register at a fixed physical
address (MMIO), execute privileged instructions (`cli`/`lgdt`/`in`/`out`/`wrmsr`/`hlt`), service interrupts,
do lock-free SMP synchronisation, and run with **no host runtime** (no libc, no host allocator, no
`std::process` panic). A source audit confirms the precise gaps: no raw-pointer type or `volatile`; no
`asm`/`naked`/`atomic` keywords; `axon-rt` is assumed on every target (`R13-native-ffi.md` FFI is still
*Draft*). The fixed-width integer vocabulary **already exists** (`Type::{I8,I16,I32,I64,U8,U16,U32,U64}`),
which removes one expected blocker — though the maturity of unsigned *operations* must be verified (§12 Q5).
This spec adds the missing escape hatch as a **trusted substrate**, so the OS above it is safe-by-construction
and the unsafe core is auditable, capability-bounded, and (where possible) SMT-checked.

### 2. Requirement link

Opens a **new `REQUIREMENTS.md` row R17** under the platform-vision bucket (alongside R16). It advances the
"build a core Axon OS from the ground up" objective and **depends on R13** (native FFI is the *alternative*
seam for hardware code — §3.5). The headline acceptance: *an Axon `.ax` kernel, built for a freestanding
`*-unknown-none` target with no host runtime, boots under QEMU and drives hardware (serial/VGA) through the
capability-gated substrate primitives — and no `surface` file gains any unsafe power in the process.*

> **Hard truth up front (the §2.3 reversal): this reopens a decision the roadmap marked closed.** ROADMAP
> §2.3 ("the kernel ambition is killed — userland OS forever") is the *explicit* prior commitment. This spec
> only makes sense if that reversal is **deliberate**, not drift. And it does **not** shrink the real cost:
> per the R16 framing, the brutal 90% of a from-scratch OS lives *below* the language (hardware bring-up,
> drivers, a GPU driver, the unsafe HAL itself). R17 is the *enabling 10%* — necessary, not sufficient.

### 3. Surface (what the user writes)

Two new things: a **freestanding build mode** (no host runtime) and a **substrate-only unsafe primitive set**,
both capability-gated. The unsafe surface is reachable **only** from a `substrate` file (ROADMAP §3) that
carries the hardware-access capability — a new effect tag **`Hal`** on the same footing as `Net`/`Fs`/`Exec`.

```axon
substrate                                    // unsafe primitives are substrate-only (surface files: E1700)

// A freestanding kernel: no host runtime. The user supplies entry + panic + (optional) allocator.
@[entry]                                      // the bare-metal entry symbol (replaces host _start/main)
fn kmain() -> never {                         // `never` = diverges; a kernel does not return
    serial_init()
    serial_write("axon booted\n")
    loop { hlt() }                            // hlt() is an inline-asm intrinsic (Slice 1)
}

@[panic_handler]                              // replaces I-4's host-abort; runs on any panic
fn on_panic(info: PanicInfo) -> never { serial_write("PANIC\n"); loop { hlt() } }

// The HAL: the ONLY place raw hardware is touched. Gated by the `Hal` capability + substrate.
@[hal]                                        // grants the Hal capability to this fn's body
fn serial_write(s: str) -> Unit | {Hal} {     // effect row MUST declare {Hal} (transitive, anti-laundering)
    let port: *u8 = ptr_from_addr(0x3F8)      // raw pointer from a physical address (substrate-only)
    for b in str_bytes(s) {
        volatile_store(port, b)               // volatile MMIO write — not elided by the optimizer
    }
}

// Hardware descriptor tables need exact layout (Slice 3).
@[repr(C)] @[packed]
type GdtEntry = { limit_lo: u16, base_lo: u16, base_mid: u8, access: u8, flags: u8, base_hi: u8 }
```

**Surface stays safe — by construction.** A `surface` file (an app, a userland service) **cannot** name
`*T`, `volatile_*`, `asm`, `ptr_from_addr`, or `@[hal]`; attempting it is **E1700**. A surface fn that
*transitively* reaches a `Hal` fn must declare `| {Hal}` or be refused (**E1703**, reusing the Phase-6
effect-subsumption walker — no laundering hardware access behind an un-annotated helper).

**Error cases:** `volatile_store` in a surface file → E1700; a `@[hal]` body called without the `Hal`
capability minted to its Principal → E1701; a freestanding build with no `@[entry]`/`@[panic_handler]` →
E1702; a surface caller reaching `Hal` without declaring it → E1703.

### 3.5 Native unsafe intrinsic **vs** R13 FFI-to-asm — the TCB-minimisation policy

For every hardware primitive there are two ways to provide it: **(a)** a native Axon unsafe intrinsic (this
spec), or **(b)** an asm/Rust trampoline called via R13 FFI. **Policy: prefer the smallest native surface +
R13 for the rest, to keep the TCB minimal.** Concretely:

- **Native intrinsics (this spec):** the primitives with no high-level equivalent that are *pervasive* and
  *perf-critical* — raw ptr, `volatile_load`/`store`, `ptr_from_addr`, atomics, `hlt`/`cli`/`sti`. These earn
  their place in the language because wrapping each in an FFI call would be absurd overhead.
- **R13 FFI trampolines:** one-off privileged sequences and bring-up glue (`lgdt`/`lidt`/mode switches/SMP
  trampolines) — written in a few lines of asm, called once. No reason to grow the language for these.

This split *is* the TCB boundary: the native-intrinsic set is fixed and audited; everything else is asm the
HAL author writes and the multi-sig TCB review (R10/§7) signs.

### 4. Semantics (what it does)

| Input class | Behavior |
|---|---|
| `ptr_from_addr(a)` in a substrate `@[hal]` fn | yields a raw `*T` pointing at physical address `a`; no checks (substrate-unsafe) |
| `volatile_load(p)` / `volatile_store(p, v)` | emits a non-elidable LLVM `load volatile`/`store volatile`; ordering w.r.t. other volatiles preserved |
| raw `*T` arithmetic / `int↔ptr` cast | permitted in substrate-unsafe only; UB is the author's responsibility (asserted, SMT-checked where in fragment) |
| same primitive in a **surface** file | **E1700** at check — never compiles, no runtime path |
| `@[hal]` fn whose Principal lacks the `Hal` capability | **E1701** at check — capability is granted by construction (R11 minting), not ambient |
| surface fn transitively reaching `Hal`, not declaring `\| {Hal}` | **E1703** (effect-subsumption walker; transitive) |
| freestanding build, missing `@[entry]` or `@[panic_handler]` | **E1702** at link — a no-host image needs both |
| a panic in a freestanding build | invokes the user `@[panic_handler]` (halt/serial/reboot) — **never** `std::process` (I-4 reframed, §7) |
| inline `asm(...)` (Slice 1) | emits the instruction(s) with declared clobbers; malformed constraint → E1705 |
| `@[naked]` fn / `@[interrupt]` fn (Slice 1) | no prologue/epilogue / `x86-interrupt` ABI (CPU frame + `iret`) |
| atomic op + ordering (Slice 2) | lowers to the LLVM atomic with the named memory order; `Send`/`Sync`-equiv governs cross-core sharing |
| `@[repr(C)]`/`@[packed]`/`@[align(N)]` (Slice 3) | exact field layout for hardware structs; overrides the I-6 *default* layout (opt-in, §7) |
| `@[no_alloc]` fn that reaches a heap-allocating builtin (Slice 3) | **E1704** — ISR/early-boot allocation-free guarantee, enforced like `@[pure]`/`@[total]` |
| hosted (default) build | **entirely unchanged** — none of the above exists unless `--features freestanding` AND a `substrate` file opts in |

### 5. Type rules

- New type: **raw pointer `*T`** (`Type::RawPtr(Box<Type>)`) — substrate-only; the checker refuses it in any
  `surface`-reachable signature except behind a HAL boundary that re-wraps it in `Option`/`Result` (so I-3's
  "no null in safe code" holds — a raw null never escapes the substrate as a bare `*T`).
- New intrinsic sigs: `ptr_from_addr: fn(u64) -> *T`, `volatile_load: fn(*T) -> T`,
  `volatile_store: fn(*T, T) -> Unit`, `asm: <intrinsic, special-cased in the parser like Phase-8 blocks>`,
  the atomic family `atomic_{load,store,cas,fetch_add}: fn(*T, …, Ordering) -> …`.
- New return type **`never`** (the divergent/`!` type) for `@[entry]`/`@[panic_handler]` — unifies with any
  type (bottom); a fn typed `never` that returns is a type error.
- New effect tag **`Hal`** in the effect catalog (`builtin_effect_row`); composes with Phase-6 subsumption
  (E1310) and `@[contained]` exactly like `Net`/`Fs`/`Exec`/`Gfx`(R16).
- `@[repr(C)]`/`@[packed]`/`@[align(N)]` attach to `TypeDef` and drive codegen's struct layout (threads a new
  `Repr` field through the parser → checker → codegen `llvm_type_from_axon`).
- Unsigned ops: the existing `U8..U64` variants must route to the *unsigned* LLVM ops (zext, `udiv`/`urem`,
  unsigned `icmp`, logical `lshr`) — verify-and-complete, not new (§12 Q5).

### 6. Error codes

New block **E17xx / W17xx** (E16xx is R16 UI; 17xx is clear).

| Code | Trigger | Message shape |
|---|---|---|
| E1700 | unsafe primitive (`*T`, `volatile_*`, `asm`, `ptr_from_addr`, `@[hal]`) used in a `surface` file | `unsafe substrate primitive `volatile_store` not allowed in a surface file; move to a `substrate` HAL` |
| E1701 | `@[hal]` fn body runs without the `Hal` capability minted to its Principal | `fn `serial_write` needs the `Hal` capability; not granted by Principal `driver`` |
| E1702 | freestanding (no-runtime) build missing `@[entry]` or `@[panic_handler]` | `freestanding target needs `@[entry]` and `@[panic_handler]`; missing: panic_handler` |
| E1703 | surface fn transitively reaches a `Hal` fn without declaring `\| {Hal}` | `fn `f` reaches Hal effect via `g` but its row omits {Hal}` |
| E1704 | `@[no_alloc]` fn reaches a heap-allocating builtin | `fn `isr_handler` is `@[no_alloc]` but calls `str_concat` (heap); ISR-unsafe` |
| E1705 | inline `asm` constraint/clobber malformed | `asm clobber `xyz` is not a known register` |
| W1710 | `@[hal]` fn / `@[unsafe]` region containing no unsafe operation | `unnecessary `@[hal]` — `f` performs no hardware access` |
| E0910 (reuse) | a substrate unsafe primitive reached by the **interpreter** (no hardware) | `bare-metal substrate primitives require a freestanding codegen build; not available under `axon run`` |

> **Note — the I-2 / interpreter relationship is INVERTED here vs R16.** UI was interp-authoritative,
> codegen-refused. The HAL is the opposite: hardware primitives have **no interpreter semantics** (there is no
> hardware under `axon run`), so they are **codegen-only** and the interpreter **refuses** them (E0910). The
> interpreter remains the reference oracle for all *non-hardware* substrate logic; HAL leaves are validated by
> running the actual image under QEMU (the parity oracle moves to emulator behavior, not interp↔codegen).

### 7. Invariants touched

This spec is **also a multi-invariant-amendment proposal** (each carve-out follows
`ARCHITECTURE_INVARIANTS.md` §"How to change an invariant"; the actual edits land *with the code* per process
step 3, like `R16a`). **The safety thesis is preserved by *bounding*, not by avoidance:** every carve-out is
gated to `substrate` files + the `Hal` capability + the TCB, so no `surface` code is affected.

| Invariant | Change | How safety is preserved |
|---|---|---|
| **I-3** (no null, no exceptions) | **amended**: a raw `*T` may be null | *only inside substrate-unsafe*; a raw null never escapes to safe code as a bare `*T` (HAL re-wraps in Option/Result at the boundary). Safe Axon: unchanged. |
| **I-4** (user code never aborts the host) | **amended**: in a freestanding build there *is* no host | reframed to "panic routes to the declared `@[panic_handler]` (a TCB component), never to UB"; hosted builds unchanged. |
| **I-5** (two-mode ownership, no GC) | **amended**: raw pointers bypass the borrow checker | *substrate-unsafe only*; the borrow checker still governs all safe code; HAL discipline is asserted + SMT-checked where in fragment. |
| **I-6** (frozen IR layouts) | **extended**: `@[repr(C)]`/`@[packed]`/`@[align]` user layout control | I-6 freezes the *default* layouts; repr is an *opt-in override* for hardware structs, not a change to defaults. |
| **I-11** (capability boundary is real and total) | **EXTENDED, not broken** | hardware access becomes the new `Hal` capability axis, gated identically to Net/Fs/Exec. This *strengthens* I-11. |
| **I-12** (self-modification cannot weaken the TCB) | **preserved** | the HAL + `@[panic_handler]` + `@[global_allocator]` are TCB components; the existing rule + multi-sig (R10/§7) govern them. |

### 8. Test plan (maps 1:1 to §4)

- [ ] **Unit:** `*T`/`volatile_*`/`ptr_from_addr` parse + type-check only in substrate; the `Hal` effect row
      propagates transitively; `never` unifies as bottom; `@[repr(C)]`/`@[packed]` produce the exact LLVM
      struct layout (golden IR).
- [ ] **Integration (the acid test):** build a minimal `.ax` kernel for `x86_64-unknown-none` with no host
      runtime, boot it under **QEMU `-nographic`**, assert it writes the expected bytes to the **serial port**.
- [ ] **CLI e2e (observable):** `axon build kernel.ax --target x86_64-unknown-none --freestanding` emits a
      bootable image; running it under QEMU exits/halts with the expected serial output.
- [ ] **Adversarial:** unsafe in a surface file (E1700); `@[hal]` without capability (E1701); missing
      panic_handler (E1702); surface laundering `Hal` through a helper (E1703 — the transitive case);
      `@[no_alloc]` ISR that allocates (E1704); HAL primitive under `axon run` (E0910).
- [ ] **Property (invariant):** no `surface` file in the whole corpus can name any unsafe primitive (a static
      sweep — guards the I-3/I-5 carve-outs from leaking past the substrate boundary).
- [ ] **Parity:** N/A interp↔codegen for HAL leaves (codegen-only, §6 note); QEMU is the oracle. Non-hardware
      substrate logic keeps normal interp↔codegen parity.
- [ ] **Journey/red-team:** the "axon booted" kernel + a timer-interrupt handler (Slice 1) + an SMP atomic
      counter incremented from 2 cores (Slice 2), each verified under QEMU.

### 9. Acceptance criteria (the done gate — per slice)

**Slice 0 (v0 — "it boots"):**
- [ ] `axon_kernel_boots_qemu_serial_hello` — a no-runtime `.ax` kernel boots under QEMU and writes to serial.
- [ ] `unsafe_outside_substrate_is_e1700` and `hal_without_capability_is_e1701` pass.
- [ ] `no_surface_file_can_name_unsafe` (corpus sweep) passes — the safety carve-out does not leak.

**Slice 1 (asm + interrupts):**
- [ ] `axon_kernel_handles_timer_interrupt` — an `@[interrupt]` ISR set in an IDT fires under QEMU.

**Slice 2 (SMP + atomics):**
- [ ] `axon_smp_atomic_counter_is_race_free` — two cores increment an atomic; final value is exact.

**Slice 3 (layout + no_alloc):**
- [ ] `axon_repr_c_gdt_layout_byte_exact` (golden) + `no_alloc_isr_rejects_heap_call_e1704`.

### 10. Performance budget

- Freestanding image size: a "hello-serial" kernel ≤ **64 KB** (no `axon-rt` host bloat — the no-runtime mode
  must link none of the hosted externs). Guarded by an image-size check.
- `volatile_*` / atomics must lower to a *single* instruction (no wrapper-call overhead) — verified by IR
  inspection in the unit golden-IR tests.
- ISR latency / context-switch budgets: deferred to a real scheduler slice (out of scope for v1).

### 11. Rollout & rollback

**Feature-flagged behind `--features freestanding`** + opt-in per `substrate` file; the entire unsafe surface
is **off** otherwise. The default hosted build, every example, and all ~700 tests are untouched (the flag
gates all of it; a `surface` file can never reach it). Sliced, each independently revertible:

| Slice | Deliverable | Revertible? |
|---|---|---|
| **0 — boots** | freestanding target + no-runtime mode (`@[entry]`/`@[panic_handler]`) + raw `*T`/`volatile_*`/`ptr_from_addr` (substrate+`Hal`-gated) → QEMU serial-hello | yes — pure addition behind flag |
| **1 — asm/interrupts** | inline `asm`, `@[naked]`, `@[interrupt]` (x86-interrupt ABI), IDT handling | yes |
| **2 — SMP/atomics** | atomic intrinsics + memory ordering + `Send`/`Sync`-equivalent | yes |
| **3 — layout/no_alloc** | `@[repr(C)]`/`@[packed]`/`@[align]` + `@[no_alloc]` checker | yes |
| **4 — capability HAL + TCB** | the trusted HAL as a content-addressed TCB component; `Hal`-capability minting (R11); SMT-proven page-table invariants | yes |

**Blast radius:** confined to the `freestanding` feature + substrate files. The unsafe primitives have no
interpreter path (E0910) and no surface path (E1700), so they cannot affect hosted execution. The riskiest
part is the *invariant amendments* (§7) — each gated to the slice that needs it, via the standard process.

### 12. Open questions

1. **(strategic — BLOCKS opening the phase)** The §2.3 reversal: is "build a core Axon OS from the ground up"
   a real commitment or an exploration? R17 only opens if the kernel ambition is *deliberately* un-killed.
   *Default: treat as exploration; land Slice 0 (the bootable proof) as a spike, defer Slices 1–4 pending a
   real OS commitment + a customer/use-case.*
2. **(§3.5)** Native intrinsic vs R13-FFI-to-asm boundary: exactly which privileged ops are native vs
   trampolined. *Default: the pervasive/perf-critical set is native (raw ptr/volatile/atomics/hlt); one-off
   bring-up (lgdt/lidt/mode-switch/SMP-trampoline) is R13 asm. Minimise the native set = minimise the TCB.*
3. **(§3)** `unsafe` ergonomics: a block (`unsafe { }`) vs an attribute (`@[hal]`/`@[unsafe]` fn). *Default:
   attribute + substrate-file gate (matches Axon's attribute-heavy, keyword-light design; coarser but
   simpler; revisit if intra-fn granularity is needed).*
4. **(§3, Slice 0)** Bootloader: bring-your-own (Limine / multiboot2 / GRUB) vs custom. *Default: BYO
   (multiboot2 or Limine handoff); Axon owns everything post-handoff — writing a bootloader is not on the
   critical path.*
5. **(§5 — AUDITED 2026-06-19, code-verified via the interp path; worse than feared)** Unsigned-integer
   support: the `U8..U64` *types* are recognized by name (`Type::from_name`) and **enforced** by the checker,
   but they are **non-functional** — you cannot even bind a literal: `let a: u32 = 4000000000` is **E0102**
   ("expected u32, found i64") because integer literals default to `i64` with **no coercion/inference to
   unsigned**. So unsigned ops are moot — there's no way to construct an unsigned value in the first place.
   **Conclusion: unsigned fixed-width support is a BUILD PREREQUISITE for Slice 0, not a present asset** (a
   type-inference change in `infer.rs`/`checker.rs` for literal defaulting + annotation coercion, then the
   unsigned ops `udiv`/`urem`/`lshr`/unsigned `icmp`/zext, then codegen parity). This is a separate Structural
   slice (spec-first), scoped ahead of the bare-metal HAL. *Repro: `let a: u32 = 1` → E0102.*
6. **(§7)** `@[global_allocator]`: does v1 require a user allocator, or support a no-alloc kernel (static
   allocation only)? *Default: no-alloc first (Slice 0 boots with zero heap); the allocator hook lands when a
   slice needs `str`/array/closure heap ops in kernel context.*
