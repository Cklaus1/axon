# R17 — Freestanding Substrate + Trusted HAL (bare-metal Axon)

**Spec ID:** `R17-freestanding-substrate` (new requirement row; depends on `R13-native-ffi.md`, `R7-targets.md`; extends ROADMAP §3 substrate/surface + §7 TCB; **reverses ROADMAP §2.3** — see §12 Q1)
**Status:** 🚧 Implementing (re-verified 2026-07-18) — **COMMITTED (founder decision 2026-06-19; §12 Q1 resolved, ROADMAP §2.3 reversed).** The leading word here said "Draft" for weeks after Slices 0-3 landed — misleading at a skim even though the detail immediately following was accurate; same staleness class as R17's siblings (R21/R22/R23/R26/R27/R28/R29/R31/R32/R12/R14/R1b/R1c/R1d), caught by the same outer-loop sweep (`EXECUTION_MODEL.md` §2), just not fixed at the leading word until now. Slice 0 LANDED (076c445): `@[entry]`/`@[panic_handler]`, `Hal` effect row, HAL builtins, `axon build --freestanding`. **Slice 1 LANDED (bf97c55):** hex/binary/underscore integer literals, `asm(...)` expression (real LLVM inline asm in codegen, E0910 in interp), `@[naked]`/`@[interrupt]` → LLVM naked attribute / x86-interrupt CC 83, `hlt`/`cli`/`sti` builtins emit real inline asm, `scripts/kernel.ld` + `--linker-script` CLI option. Demo: `examples/kernel/hello_kernel_slice1.ax`. **Slice 2 LANDED (a7b262c):** SMP atomics `atomic_{load,store,fetch_add,cas}_i64` with a compile-time memory-order literal (0=relaxed…4=seq_cst) → real LLVM atomics (`atomicrmw add … seq_cst` etc.); golden-IR gate `axon_smp_atomic_counter_is_race_free`; new `axon build --emit-llvm`. **Slice 3 LANDED:** `@[repr(C)]`/`@[packed]`/`@[align]` struct layout (golden-IR `axon_repr_c_gdt_layout_byte_exact` → `<{ i16, i16, i8, i8, i8, i8 }>`) + `@[no_alloc]`→E1704 (transitive). Demos: `examples/kernel/hello_kernel_slice2.ax`, `hello_kernel_slice3.ax`. **`fn_addr(name) -> i64` LANDED (2026-07-20, §12 Q7):** the missing function-address-as-value primitive `axon_kernel_handles_timer_interrupt` needed — E1707 band, `Hal`-gated, codegen-only (interp `E0910`), compile-time-literal-validated (mirrors Slice 2's `atomic_ordering_arg`), lowers to a constant-folded `ptrtoint (ptr @fn to i64)` (confirmed via `--emit-llvm`). **IDT-construction path now fully unblocked at the primitive level (2026-07-20):** building a real IDT gate descriptor also needs splitting `fn_addr`'s i64 result into u16/u32 fields, which surfaced that the `x as uN` cast operator had no callees (`R19-fixed-width-integers.md` Slice D) AND that struct-literal field stores into narrow `@[packed]` fields were silently width-wrong (a real memory-corruption bug, also fixed in Slice D, found via this exact IDT construction). With both fixed, a `@[repr(C)] @[packed] IdtEntry` filled via `fn_addr` + `shr`/`bit_and` + `as u16`/`as u32` now compiles to byte-exact, correctly-narrow field stores (verified via `--emit-llvm`). **`lidt(idtr_addr) -> ()` LANDED (2026-07-20, same day, §12 Q8):** hand-written inline asm (`port_out_u8`/`port_in_u8` precedent, not the operand-less `asm(...)` surface — confirmed that surface can't carry a dynamic pointer operand either); verified past IR text via `objdump -d` on the real object file, showing the actual encoded `0f 01 18  lidt (%rax)`. **§12 Q8 was then CORRECTED the same day:** its own "needs a new address-of-static primitive" conclusion was wrong — the fixed-physical-address idiom already proven for the SMP counter (`hello_kernel_slice2.ax`) works for the IDT/IDTR too, no new primitive needed; `examples/kernel/hello_kernel_timer_irq.ax` type-checks and compiles to a valid object file using exactly that approach. **§12 Q9 (new, same day):** linking that example found a real, separate, structural gap instead — checked-arithmetic overflow panics (I-9) call an external `__axon_arith_panic` symbol that only `axon-rt` (a full host runtime) implements, and freestanding builds never link `axon-rt` at all. Every non-trivial arithmetic op in a freestanding kernel fails to link; no R17 example had ever actually exercised this before (the only one ever linked+booted, Slice 1, does no runtime arithmetic at all). Candidate fixes sketched, not decided. Remaining: §12 Q9 (the arithmetic-panic linking gap) blocks `hello_kernel_timer_irq.ax` from actually linking/booting; once resolved, PIC remap + PIT programming + a QEMU golden-output gate (already written into the example) complete `axon_kernel_handles_timer_interrupt`. The full 2-core QEMU SMP harness remains separately deferred (golden-IR proxy stands per §9).
**Risk class:** Structural (introduces the language's *only* `unsafe` surface; amends I-3/I-4/I-5/I-6, extends I-11/I-12)
**Author / date:** cklaus, 2026-06-12

```spec-meta
id: R17-freestanding-substrate
status-claim: Implementing
depends-on: R13-native-ffi, R7-targets
blocks: R23-ebpf-target, R25-zephyr-target
blocked-by: none
supersedes: none
related: R36-full-asi-os, R37-nano-micro-asi-kernel
conflicts-with: none
reserves: none
evidence: r17_slice1_qemu_boot_writes_axon_s1 (Slice 1, real QEMU boot); axon_smp_atomic_counter_is_race_free (Slice 2, golden-IR); axon_repr_c_gdt_layout_byte_exact (Slice 3) — all 3 re-verified 2026-07-18
```

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
| E1706 | atomic builtin `ordering` arg is not a compile-time literal in 0..=4 (Slice 2) | `atomic_load_i64 ordering must be a compile-time literal (0=relaxed…4=seq_cst), not a runtime expr` |
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

**Slice 1 (asm + interrupts):** ✅ LANDED (bf97c55) — hex/binary literals, `asm(...)` real codegen, `@[naked]`/`@[interrupt]`, `hlt`/`cli`/`sti` inline asm, linker script.
- [x] Hex/binary/underscore integer literals parse and evaluate correctly.
- [x] `asm(...)` emits real LLVM inline asm in codegen; E0910 in interpreter.
- [x] `@[naked]` → LLVM "naked" attribute; `@[interrupt]` → x86-interrupt CC 83.
- [x] `hlt`/`cli`/`sti` HAL builtins emit real inline asm (not const_zero placeholders).
- [x] `scripts/kernel.ld` + `--linker-script` CLI option wired into freestanding link.
- [x] `r17_slice1_qemu_boot_writes_axon_s1` — kernel boots under QEMU and writes "axon s1" to debugcon (76860bc). Uses multiboot1 + boot_stub.asm (32→64 mode switch) + port_out_u8 for QEMU debugcon; test skips gracefully if nasm/qemu absent.
- [ ] `axon_kernel_handles_timer_interrupt` — full IDT + PIC + timer ISR fires under QEMU (deferred to Slice 2; requires port_in_u8 for PIC mask reads, @[repr(C)] for IDT entries).

**Slice 2 (SMP + atomics):** ✅ LANDED (a7b262c).
- [x] `atomic_load_i64`/`atomic_store_i64`/`atomic_fetch_add_i64`/`atomic_cas_i64` (substrate-only, Hal effect, E0910 in interp). The trailing `ordering` arg is a compile-time integer literal (0=relaxed,1=acquire,2=release,3=acq_rel,4=seq_cst); non-literal/out-of-range → E1706 (codegen). Each lowers to the real LLVM atomic with the named order (`atomicrmw add … seq_cst`, `load atomic … acquire`, `store atomic … release`, `cmpxchg … seq_cst monotonic`). `Send`/`Sync` cross-core sharing is governed by the same Hal effect-subsumption walker (E1310; surface files can't declare `| {Hal}`, E1306).
- [x] `axon_smp_atomic_counter_is_race_free` — **golden-IR proxy** (`scripts/atomic_ir_test.sh`): the SMP counter increment lowers to a single `atomicrmw add … seq_cst`, the load-bearing race-freedom property. A full 2-core QEMU SMP boot harness (boot the APs, both hammer a shared counter, assert the exact final value) is heavier infra and is **deliberately deferred**; the golden-IR check proves the property directly off the emitted instruction (per §9: "a pure-codegen golden-IR test is acceptable as the unit gate"). New `axon build --emit-llvm` (IR-text dump) added for the golden inspection. Demo: `examples/kernel/hello_kernel_slice2.ax`.

**Slice 3 (layout + no_alloc):** ✅ LANDED.
- [x] `@[repr(C)]`/`@[packed]`/`@[align(N)]` drive struct layout: `@[packed]` lowers to LLVM's packed-struct form (`<{ … }>`, no inter-field padding), `@[repr(C)]` keeps declaration-order C layout. Golden-IR `axon_repr_c_gdt_layout_byte_exact` (`scripts/gdt_layout_ir_test.sh`): the GDT entry lowers byte-exact to `%GdtEntry = type <{ i16, i16, i8, i8, i8, i8 }>`. `@[align(N)]` is parsed/accepted; the LLVM struct *type* carries only the packed bit, so explicit alignment applies at allocation sites (the struct-type golden is the load-bearing layout check).
- [x] `@[no_alloc]` fn reaching a heap-allocating builtin / string interpolation / a transitively-allocating helper → **E1704** (enforced like `@[pure]`/`@[total]`, transitive — closes the laundering hole). `no_alloc_isr_rejects_heap_call_e1704` passes. Heap classification (`is_heap_allocating_builtin`) is a conservative over-approximation (heap-typed return OR known mutator); HAL/atomic leaves classify allocation-free. Demo: `examples/kernel/hello_kernel_slice3.ax`.

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
7. **(added 2026-07-20, RESOLVED 2026-07-20) `axon_kernel_handles_timer_interrupt` needed a
   genuinely missing language primitive, not just wiring — the primitive now LANDED.** Sized this
   task before starting it (it was flagged as "unblocked by `@[repr(C)]` for IDT entries; deferred
   to a wiring slice" — implying only assembly remained). Confirmed via an exhaustive search
   (AST/parser/builtins/checker/infer/codegen, the full spec text, every `examples/kernel/*.ax`
   file, and `asm(...)`'s actual operand model) that Axon had **no way to obtain a function's
   address as a usable value from `.ax` source** — no `fn_addr` builtin, no address-of operator
   applicable to a function identifier (`&` is array/slice-borrow only, per `infer.rs`'s own
   "Phase 1: transparent" comment), and `asm(...)`'s `outputs`/`inputs`/`clobbers` sections are raw
   opaque strings with no operand-binding syntax at all (so there's no `sym(...)`-style symbol
   operand either). `@[interrupt]` itself does nothing beyond setting the LLVM `x86-interrupt`
   calling convention — no automatic registration, no symbol table. Q2's own default
   ("lgdt/lidt/mode-switch is R13 asm") didn't resolve this either: R13's native-FFI mechanism is
   interp-only (codegen `E0910`-refuses it), unusable for a freestanding kernel that must run under
   native codegen (there is no OS to interpret under).

   **Landed the primitive:** `fn_addr(name: str) -> i64` (E1707 band), `Hal`-effect-gated like every
   other R17 HAL builtin (substrate-only, codegen-only — `E0910` in interp, since a function's
   address is meaningless in a tree-walking interpreter). `name` MUST be a compile-time string
   literal naming a function defined in the program; codegen validates this the same way
   Slice 2's atomics validate their memory-order literal (`atomic_ordering_arg` precedent,
   mirrored as `fn_addr_target` in `codegen/expr.rs`) — a non-literal argument or an unknown
   function name is a **codegen** error (E1707), not silently accepted. Implementation resolves
   the function's `FunctionValue` from the existing `self.functions` map (the same map
   `asi.rs`'s adaptive-fn registration already uses to take a function's address for a different
   purpose) and lowers to `ptrtoint (ptr @fn to i64)`, confirmed via `axon build --emit-llvm` to
   constant-fold at compile time with no runtime cost. Return type is plain `i64` (not `u64`),
   matching 100% of existing R17 HAL builtin precedent (`volatile_load_u8`/`_u32` etc. all use
   `i64` internally despite the `_u8`/`_u32` naming). Verified: happy path compiles and the emitted
   IR is a compile-time `ptrtoint` constant; non-literal argument and unknown-function-name both
   correctly fail closed with E1707 at build time (exit 1); `cargo test -p axon-core --lib` (574
   tests) and both clippy variants clean.

   **Still not done** (deliberately out of scope for this landing — this closed the *primitive*,
   not the acceptance test): the actual IDT gate-descriptor construction, PIC/PIT programming, and
   the QEMU-based `axon_kernel_handles_timer_interrupt` boot test itself. `fn_addr` is the
   mechanism that unblocks that work; building the IDT and wiring the timer ISR is real systems
   work for a future iteration.

8. **(added 2026-07-20, `lidt` LANDED same day — but exposed a second missing primitive)**
   Sized the next step of the timer-interrupt work (loading the constructed IDT via `lidt`)
   before assuming `asm(...)` covers it, per `SESSION_STATUS.md`'s own explicit note not to
   assume `lidt` is "just asm". Confirmed it isn't: `asm(...)`'s `outputs`/`inputs`/`clobbers`
   sections are raw string literals with no operand-binding syntax (`parser.rs`'s
   `parse_inline_asm`), so a real dynamic pointer operand can't be threaded through the
   user-facing syntax at all — matching Q7's finding for `fn_addr`. Landed `lidt(idtr_addr: i64)`
   as a new HAL builtin instead, following the exact `port_out_u8`/`port_in_u8` precedent
   (hand-written `create_inline_asm` in codegen with a real operand, not the generic `asm(...)`
   surface): template `"lidt ($0)"`, constraint `"r,~{memory}"` (any general-purpose register
   holding the IDTR structure's address, register-indirect addressing). Verified past IR-text
   inspection this time — `axon build --emit-obj` + `objdump -d` on the resulting object file
   shows the real encoded instruction: `0f 01 18  lidt (%rax)`, confirming LLVM's actual
   assembler accepts the template/constraint pair, not just that the IR looks plausible.

   **While sizing how a real caller would use `lidt`, found the SECOND missing primitive:**
   `lidt` only consumes an address — the caller must construct the 10-byte IDTR structure (and,
   for the full acceptance test, the 256-entry IDT array itself) and obtain ITS address. Axon has
   no mechanism for this today. `ptr_from_addr` goes the wrong direction (a known FIXED physical
   constant → pointer, per `hello_kernel_slice2.ax`'s `COUNTER_ADDR` idiom — it does not give you
   the address of something Axon itself allocated). `fn_addr` (Q7) only takes function addresses.
   There is no `&`-of-a-local, no address-of-a-struct/array operator, and critically **no
   `static`/global-variable concept at all** — confirmed by grep, the language has no keyword or
   surface for a linker-placed, known-address, mutable global (which is how real kernels place
   their IDT/GDT in practice; a real freestanding kernel needs the IDT to live somewhere with a
   STABLE address, not a stack frame that unwinds). This is a real, structural gap — building it
   means either (a) an address-of-a-local-with-escape-analysis primitive (raises real lifetime/
   aliasing questions Axon's ownership model doesn't have a story for yet), or (b) a `static`
   storage class (a new declaration form, its own design surface: mutability, zero-init, linker
   section placement, `@[hal]` interaction). Neither is "just wiring" — this is its own properly-
   scoped design task for a future iteration, not decided here. The `256`-entry IDT array
   compounds this further (even with an address-of-static primitive, filling 256 gate
   descriptors needs either a `for` loop over array-index assignment into a fixed-size array
   type, or 256 unrolled gate-fill calls — array-of-struct construction/mutation ergonomics in
   substrate code haven't been exercised at this scale yet either).

   **CORRECTION (2026-07-20, same day): the above was wrong — no new primitive is needed.**
   Before attempting the design task Q8 called for, tried the FIXED-PHYSICAL-ADDRESS idiom
   already established and proven for the SMP counter (`hello_kernel_slice2.ax`'s
   `let COUNTER_ADDR = 0x100000` + `ptr_from_addr` + atomics) — a freestanding kernel's "globals"
   don't need Axon-level storage at all; they're just known physical-address constants the linker
   script reserves, exactly like MMIO. Wrote a real test
   (`examples/kernel/hello_kernel_timer_irq.ax`): the IDT and IDTR live at fixed addresses
   (`let IDT_ADDR = 0x300000`), a `for i in 0..256` loop fills each gate via `volatile_store_*`
   directly at `IDT_ADDR + i*16` (no `@[repr(C)]` struct value ever constructed — sidesteps that
   path entirely), and the IDTR is filled the same way before `lidt(IDTR_ADDR)`. This
   **type-checks and compiles to a valid object file** — confirmed, not assumed (`axon check` +
   `axon build --freestanding --emit-obj`, disassembled to inspect the generated loop + calls).
   The two "for a future iteration" primitives in this open question were never actually needed.
   Same lesson as [[r26-substrate-trait-aspirational]] (memory) — a prior iteration's own "this
   needs new design work" conclusion was wrong, found by trying the existing-tooling path FIRST
   before assuming a new primitive is required, not after.

   **Extended `examples/kernel/boot_stub.asm`'s identity-mapped region from a single 2 MiB page to
   eight (16 MiB)**, since `IDT_ADDR = 0x300000` (3 MiB) falls outside the original 1-page mapping
   and the kernel image itself already occupies enough of the first 2 MiB that the IDT can't safely
   share it. Re-verified `scripts/qemu_boot_test.sh` (Slice 1's real QEMU boot test, the only
   existing consumer of `boot_stub.asm`) still PASSES after the change — additive, not a behavior
   change for anything that only used the first 2 MiB.

9. **(added 2026-07-20, same day — found while linking `hello_kernel_timer_irq.ax`) Checked-
   arithmetic overflow panics cannot link in a freestanding build — a real, structural, previously
   undiscovered gap, distinct from Q7/Q8.** Every signed `+`/`-`/`*`/`/`/`%` is checked by default
   (I-9) and its overflow/div-by-zero branch calls the external symbol `__axon_arith_panic`
   (`codegen/builtins.rs` ~3536, unconditionally declared — no freestanding-mode branch exists to
   skip or reroute it). That symbol is implemented in `axon-rt` (`crates/axon-rt/src/lib.rs`,
   using `std::string`/`format!`/`std::process` — a full host runtime), and freestanding builds
   never link `axon-rt` at all (`scripts/qemu_boot_test.sh` links only `boot_stub.o` + the Axon
   kernel object) — by design, since a kernel has no host OS underneath it to provide one. Linking
   `hello_kernel_timer_irq.ax` failed with `undefined reference to __axon_arith_panic` at every
   non-trivial arithmetic site (`i * IDT_ENTRY_SIZE`, `IDT_ADDR + …`, `base + 4`, …) — confirmed
   this was never exercised before: `hello_kernel_slice1.ax` (the only R17 example EVER actually
   linked+booted, `qemu_boot_test.sh`) does no runtime arithmetic at all, only literal port/address
   constants; `hello_kernel_slice2.ax`/`hello_kernel_slice3.ax` (which DO have arithmetic, e.g.
   `i = i + 1`) are only ever exercised via `--emit-llvm` golden-IR checks
   (`atomic_ir_test.sh`/`gdt_layout_ir_test.sh`), which never link. **This blocks essentially any
   non-trivial freestanding kernel logic**, not just the timer-interrupt path — any substrate code
   doing real arithmetic (a loop counter, an offset computation, anything beyond fixed literals)
   hits this wall at link time. Candidate fixes (not decided here, needs its own sizing pass):
   (a) codegen synthesizes a minimal, `no_std`-safe trap directly in the freestanding module itself
   (e.g. write a marker byte to debugcon + `hlt` loop, no external symbol at all) instead of
   declaring an external `__axon_arith_panic` when compiling `--freestanding`; (b) a tiny separate
   freestanding-safe runtime stub crate linked in for substrate builds only. `hello_kernel_timer_irq.ax`
   is committed as a demo of the now-CORRECTED Q8 (compiles, type-checks) but does **not** yet link
   or boot — blocked on this, not Q8.
