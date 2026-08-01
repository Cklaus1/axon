# R17 — Freestanding Substrate + Trusted HAL (bare-metal Axon)

**Spec ID:** `R17-freestanding-substrate` (new requirement row; depends on `R13-native-ffi.md`, `R7-targets.md`; extends ROADMAP §3 substrate/surface + §7 TCB; **reverses ROADMAP §2.3** — see §12 Q1)
**Status:** 🚧 Implementing (re-verified 2026-07-18) — **COMMITTED (founder decision 2026-06-19; §12 Q1 resolved, ROADMAP §2.3 reversed).** The leading word here said "Draft" for weeks after Slices 0-3 landed — misleading at a skim even though the detail immediately following was accurate; same staleness class as R17's siblings (R21/R22/R23/R26/R27/R28/R29/R31/R32/R12/R14/R1b/R1c/R1d), caught by the same outer-loop sweep (`EXECUTION_MODEL.md` §2), just not fixed at the leading word until now. Slice 0 LANDED (076c445): `@[entry]`/`@[panic_handler]`, `Hal` effect row, HAL builtins, `axon build --freestanding`. **Slice 1 LANDED (bf97c55):** hex/binary/underscore integer literals, `asm(...)` expression (real LLVM inline asm in codegen, E0910 in interp), `@[naked]`/`@[interrupt]` → LLVM naked attribute / x86-interrupt CC 83, `hlt`/`cli`/`sti` builtins emit real inline asm, `scripts/kernel.ld` + `--linker-script` CLI option. Demo: `examples/kernel/hello_kernel_slice1.ax`. **Slice 2 LANDED (a7b262c):** SMP atomics `atomic_{load,store,fetch_add,cas}_i64` with a compile-time memory-order literal (0=relaxed…4=seq_cst) → real LLVM atomics (`atomicrmw add … seq_cst` etc.); golden-IR gate `axon_smp_atomic_counter_is_race_free`; new `axon build --emit-llvm`. **Slice 3 LANDED:** `@[repr(C)]`/`@[packed]`/`@[align]` struct layout (golden-IR `axon_repr_c_gdt_layout_byte_exact` → `<{ i16, i16, i8, i8, i8, i8 }>`) + `@[no_alloc]`→E1704 (transitive). Demos: `examples/kernel/hello_kernel_slice2.ax`, `hello_kernel_slice3.ax`. **`fn_addr(name) -> i64` LANDED (2026-07-20, §12 Q7):** the missing function-address-as-value primitive `axon_kernel_handles_timer_interrupt` needed — E1707 band, `Hal`-gated, codegen-only (interp `E0910`), compile-time-literal-validated (mirrors Slice 2's `atomic_ordering_arg`), lowers to a constant-folded `ptrtoint (ptr @fn to i64)` (confirmed via `--emit-llvm`). **IDT-construction path now fully unblocked at the primitive level (2026-07-20):** building a real IDT gate descriptor also needs splitting `fn_addr`'s i64 result into u16/u32 fields, which surfaced that the `x as uN` cast operator had no callees (`R19-fixed-width-integers.md` Slice D) AND that struct-literal field stores into narrow `@[packed]` fields were silently width-wrong (a real memory-corruption bug, also fixed in Slice D, found via this exact IDT construction). With both fixed, a `@[repr(C)] @[packed] IdtEntry` filled via `fn_addr` + `shr`/`bit_and` + `as u16`/`as u32` now compiles to byte-exact, correctly-narrow field stores (verified via `--emit-llvm`). **`lidt(idtr_addr) -> ()` LANDED (2026-07-20, same day, §12 Q8):** hand-written inline asm (`port_out_u8`/`port_in_u8` precedent, not the operand-less `asm(...)` surface — confirmed that surface can't carry a dynamic pointer operand either); verified past IR text via `objdump -d` on the real object file, showing the actual encoded `0f 01 18  lidt (%rax)`. **§12 Q8 was then CORRECTED the same day:** its own "needs a new address-of-static primitive" conclusion was wrong — the fixed-physical-address idiom already proven for the SMP counter (`hello_kernel_slice2.ax`) works for the IDT/IDTR too, no new primitive needed; `examples/kernel/hello_kernel_timer_irq.ax` type-checks and compiles to a valid object file using exactly that approach. **§12 Q9 (found AND RESOLVED same day):** linking that example found a real, separate, structural gap — checked-arithmetic overflow panics (I-9) call an external `__axon_arith_panic` symbol only `axon-rt` (a full host runtime) implements, and freestanding builds never link `axon-rt` at all. Fixed by defining a minimal in-module trap (debugcon marker + `hlt` loop) for the three implicit safety checks (arith/bounds/refine) instead of declaring an external symbol, gated on a new `Codegen.freestanding` flag. **`axon_kernel_handles_timer_interrupt` now PASSES for real**: `hello_kernel_timer_irq.ax` boots under QEMU and the timer interrupt fires 194 times in 2 seconds with zero spurious panic markers — the full chain (IDT construction → `lidt` → PIC remap → PIT programming → `sti` → real hardware interrupt → `@[interrupt]` handler → EOI → repeated firing) verified end to end, not just IR-inspected. Gated by `scripts/timer_irq_qemu_test.sh` + cargo test `r17_timer_interrupt_fires_and_is_handled`. The full 2-core QEMU SMP harness remains separately deferred — the only remaining **QEMU-verified** acceptance gap, now pinned as the concrete unchecked §9 item `axon_smp_2core_counter_exact` (added 2026-07-31; the golden-IR proxy stands as the *unit* gate only, per §9's now-explicit single-instruction-lowering policy). **It is NOT the only open acceptance item** (corrected same day — the first fold-in said "only remaining gap" while its own §9 audit left Slice-0 gates open): the `unsafe_outside_substrate_is_e1700` adversarial test and the `no_surface_file_can_name_unsafe` corpus sweep remain unauthored (§9 Slice 0), the E1701 test is re-scoped to Slice 4 (E1701 has no emission site — enforcement-debt, not test-debt; §6), and §8's Adversarial/Property boxes remain unchecked.
**ASI-trajectory pass (2026-07-31):** the systems engineering here is sound and largely landed; the
**containment framing is what does not survive the project's own stated trajectory** (ROADMAP §2.1 "`.ax`
is an IR optimized for machine generation" + §2.4 "the typed AST is the legal artifact the user must
approve"). Three verified breaks, all folded in below: (1) the substrate/surface boundary is a
self-declared **opt-OUT** file token — absent ⇒ substrate, 0 of 170 example files declare `surface` — so
generated code lands in the unsafe dialect by default (§3); (2) `derive_risk_from_ast` does not know the
word `Hal`, so the language's most privileged effect derives **Risk = Low** and skips the Phase-11 gate
chain, while `_ => {}` makes every *future* axis default-lowest (§7.1); (3) the approval artifact a human
signs reduces effects to a **boolean**, so nothing distinguishes `| {IO}` from raw MMIO (§3). Alongside
these, the project's landed proof/audit machinery is unused on exactly the primitives that need it most —
`ptr_from_addr`/`volatile_*` take an unconstrained runtime `i64` with no refinement while the same spec
demands compile-time literals for the atomics ordering and `fn_addr`'s name (§5, Slice 4b); `@[contained]`
has **no `hal` axis** despite §5 claiming otherwise (§5); and `axon-audit`'s `EffectKind` has no `Hal`
variant (§7.2). Each fix is bounded and mechanical and is scoped as **Slice 4a / 4b** (§11); nothing here
relaxes an existing kill-gate. Strategic questions the pass raised but did not answer are §12 Q10–Q12.
**Stated limit of the spec as written:** its containment story assumes a human reviews every generated
artifact *and* can tell hardware access apart from ordinary I/O in the review output — the second half is
false today.

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
reserves: E1708, E1709 (Slice 4b HAL address refinement — reserved 2026-07-31)
evidence: r17_slice1_qemu_boot_writes_axon_s1 (Slice 1, real QEMU boot); axon_smp_atomic_counter_is_race_free (Slice 2, golden-IR); axon_repr_c_gdt_layout_byte_exact (Slice 3); r17_fn_addr_is_hal_impure_alloc_free (§12 Q7); r19_fixed_width_as_casts_are_known_pure_general_purpose (§12 Q8/R19 Slice D); r17_timer_interrupt_fires_and_is_handled (§12 Q8/Q9, real QEMU boot — `axon_kernel_handles_timer_interrupt`) — all re-verified 2026-07-20
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

> **CORRECTED 2026-07-31 — "by construction" is currently opt-OUT, and that is a stated, expiring
> assumption, not a guarantee.** The substrate/surface boundary this whole section rests on is a
> *self-declared optional leading identifier*: `parse_program` (`parser.rs:434-443`) reads `surface` /
> `substrate` if present and its own comment states **"Absent ⇒ substrate (the Phase 1–5 default)"**.
> Measured against the tree 2026-07-31: of **170** `.ax` files under `examples/`, **0** declare `surface`
> and **10** declare `substrate` — the other 160 are *implicitly* substrate and therefore implicitly hold
> the unsafe vocabulary. Combined with §4's last row (a plain hosted `axon build` lowers the HAL
> primitives unconditionally), the reachable state is: an unmarked `.ax` file, built with the default
> `axon build`, compiles arbitrary-address writes into a normal host binary.
>
> **The expiring assumption, stated explicitly rather than left implicit.** This default was
> proportionate while `.ax` was human-authored and R17 code was rare and hand-written. It is **not**
> load-bearing under ROADMAP §2.1 ("treat `.ax` as an IR, not a human-authored surface; optimize for
> machine generation") plus the Phase-10 `AXON_INTENT_GEN=1 axon intent compile` LLM body-generation
> path, which emits `.ax` with **no marker at all** (verified 2026-07-31: no `surface` emission anywhere
> in `crates/axon-intent/src/`). Machine-generated code therefore lands in the *unsafe* dialect by
> default. Two further points follow and must not be papered over: (a) a self-declared marker is not a
> capability — an untrusted generator grants it to itself on line 1; (b) E1701 has **no emission site**
> anywhere in `crates/` (§6), so `Hal` is not a capability at all today. Until Slice 4, read every
> "capability-gated" phrase in this spec as **"file-marker-gated; capability enforcement is Slice 4."**
>
> **Required before Slice 4 (Slice 4a, §11):** invert the default for the machine-generation path — an
> unmarked file, and unconditionally any file produced by `axon intent compile` / the surface compiler,
> parses as `surface`; `substrate` requires an explicit marker. Make the marker a *reviewed, bound*
> property rather than a self-declaration: include it in the R22 approval token's covered bytes
> (`crates/axon-intent/src/approval.rs`) so flipping a file to `substrate` after approval invalidates the
> token, and gate `axon build` on a checked-in substrate allowlist whose diff requires the §7/R10
> multi-sig. **No kill-gate is relaxed by this change** — it strictly narrows what compiles.

**The approval artifact does not currently carry the Hal axis (ADDED 2026-07-31).** ROADMAP §2.4 makes
the typed AST the legal artifact a human must approve, and R17 introduces the most dangerous vocabulary
in the language while adding *nothing* to that review surface. `cmd_ast_review` (`main.rs:4881-4945`)
emits per fn: name, signature, attribute **names**, `verified` (bool), and `effects` — which is literally
`f.effect_row.is_some()`, a bare boolean. It does not emit *which* effects, does not emit the file's
substrate/surface mode, and does not emit which builtins are reached. A reviewer approving
`fn serial_write(s: str) -> Unit | {Hal}` sees the same JSON shape as one carrying `| {IO}`. This is the
**stated limit** of R17's threat model as written: *the spec's containment story currently assumes a human
reviews every generated artifact AND can tell hardware access apart from ordinary I/O in the review
output — the second half is false today.* Slice 4a therefore bumps the schema to `axon-ast-review/2`,
adding per fn `effect_set` (the concrete resolved row, transitively including builtin effects — the E1704
reachability walker at `checker.rs:1753` already computes this) and `hal_calls` (the R17 HAL builtins
reached, direct and transitive), plus a top-level `file_mode` (`"surface"`/`"substrate"`/`"unmarked"`).
The web UI's AST Review pane renders any non-empty `hal_calls` as a blocking banner requiring a distinct
confirmation, and `effect_set` + `file_mode` bind into the R22 approval token so a post-approval edit that
adds `Hal` invalidates it (§8 adversarial: `approved_ast_cannot_gain_hal_after_signoff`).

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

> **CORRECTED 2026-07-31 (re-grounded same day — the first correction's premise was false):** the R13-FFI
> route above is unbuildable for freestanding code, but **not** for the reason first stated. The earlier
> claim ("R13's native-FFI mechanism is interp-only and codegen-`E0910`-refused") is wrong against the tree
> and against `R13-native-ffi.md` itself: R13 Slice 4 (native-codegen FFI lowering, forge-safe C ABI)
> LANDED 2026-06-25 (5f6b086) — native calls are codegen-lowered, and only the `modbus`/`fhir`/`fix`
> modules are E0910-refused (`crates/axon-core/src/native.rs` `is_codegen_refused`). The real exclusion is
> **linkage** — the same structural class as this spec's own §12 Q9 `__axon_arith_panic` finding: R13
> native calls lower to shim symbols implemented in the host-runtime staticlib, which a freestanding image
> never links (a kernel has no host runtime underneath it). Q8 proved
> the workable alternatives in practice: `lidt` landed as a **native HAL builtin** (hand-written
> `create_inline_asm` in `codegen/expr.rs`, the `port_out_u8` precedent), and pre-Axon bring-up glue lives
> in the **nasm boot stub** (`examples/kernel/boot_stub.asm`, the Slice-1 precedent). The deferred SMP AP
> bring-up (INIT-SIPI-SIPI real-mode trampoline) therefore goes in the boot stub or a new native builtin —
> **not** through R13 FFI. See §12 Q2 (superseded default).

This split *is* the TCB boundary: the native-intrinsic set is fixed and audited; everything else is asm the
HAL author writes (in the boot stub or as an audited native builtin) and the multi-sig TCB review (R10/§7)
signs.

#### 3.5.1 TCB admission gate — the intrinsic set is *not* currently fixed (ADDED 2026-07-31)

The word "fixed" above is contradicted by this spec's own §12 narrative. Chasing **one** acceptance test
(`axon_kernel_handles_timer_interrupt`), the native intrinsic set grew by **`fn_addr`** (Q7),
**`lidt`** (Q8), and a **synthesized in-module freestanding trap** for the three implicit safety checks
(Q9) — all on **2026-07-20**, each adding a new path to hardware or to panic semantics.
`governance/ARCHITECTURE_INVARIANTS.md` I-11 requires that any new path to a protected resource "updates
this file, the ROADMAP.md TCB section, and ships paired allow + deny tests". None of that ran: §8's
Adversarial box is unchecked, `unsafe_outside_substrate_is_e1700` is unauthored (§9 Slice 0), and no deny
test exists for `fn_addr` or `lidt`.

**Why this is a trajectory problem and not a paperwork one.** When a human writes the kernel, "the
compiler is missing primitive X" arrives at human pace and each request gets scrutiny. When kernel code is
machine-authored against these same acceptance tests, that request arrives continuously, and every
accepted one permanently enlarges the thing I-12 says self-modification may not weaken. R17 has **no
admission gate, no budget, and no deny-test requirement** for TCB growth; today the only forcing function
is the effort of hand-writing the codegen arm — exactly the cost that is collapsing.

**Policy (binding for Slice 4 and after; retro-filing required for the three 2026-07-20 additions):**
every new native intrinsic requires

1. an entry in a checked-in **`governance/state/tcb-intrinsics.md`** manifest: name, rationale, the
   acceptance item that forced it, and the landing commit;
2. **paired allow + deny tests**, as I-11 already mandates — the deny test being the surface / non-`Hal`
   refusal, not merely a happy-path lowering check;
3. the §7 / R10 **multi-sig signature** recorded against the manifest entry;
4. a stated **cap on the intrinsic count for v1** — this spec claims the set is "fixed", so the number is
   written down: **v1 ceiling = 24 native HAL intrinsics** (the landed set plus headroom for the deferred
   SMP bring-up path). Exceeding it is a spec amendment under §7's process, not an implementation
   decision.

Gated by a convergence test (the `r1e_direct_ir_emission_stays_confined` precedent, which §12 Q9 records
as having already caught real drift): **`hal_builtins_match_tcb_manifest`** fails when a HAL builtin
appears in `builtins.rs` without a manifest entry, or when the manifest count exceeds the cap.

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
| inline `asm(...)` (Slice 1) | emits the instruction(s) with declared clobbers; malformed constraint → E1705 *(specified but NOT implemented as of 2026-07-31 — currently a raw LLVM error; see §6)* |
| `@[naked]` fn / `@[interrupt]` fn (Slice 1) | no prologue/epilogue / `x86-interrupt` ABI (CPU frame + `iret`) |
| atomic op + ordering (Slice 2) | lowers to the LLVM atomic with the named memory order; `Send`/`Sync`-equiv governs cross-core sharing |
| `@[repr(C)]`/`@[packed]`/`@[align(N)]` (Slice 3) | exact field layout for hardware structs; overrides the I-6 *default* layout (opt-in, §7) |
| `@[no_alloc]` fn that reaches a heap-allocating builtin (Slice 3) | **E1704** — ISR/early-boot allocation-free guarantee, enforced like `@[pure]`/`@[total]` |
| hosted (default) build | **unchanged for surface code** — the unsafe surface is gated by the `substrate` file marker (+ E0910 interp refusal) **only**. *(Re-corrected 2026-07-31: the earlier same-day claim that the `--freestanding` flag was part of the gate conjunction is false — codegen lowers `ptr_from_addr`/`volatile_*`/atomics/`fn_addr`/`lidt` unconditionally, with no `self.freestanding` check in any `emit_call` arm; the flag drives only link mode/reloc, the E1702 entry check, and Q9 trap synthesis. A plain hosted `axon build` of a substrate-marked file therefore DOES compile raw-memory writes to arbitrary addresses into a normal host binary — an explicitly-accepted exposure until a codegen-side freestanding gate on the HAL primitives is added; see §11.)* |

> **ADDED 2026-07-31 — every runtime safety gate this project owns is structurally blind to HAL code.**
> §6's inversion note (HAL is codegen-only, interp-`E0910`-refused) is the right architectural call, but
> the spec never traces its consequence. *Everything* the project uses to decide whether code is safe to
> run executes under the **interpreter**: `axon deploy`'s gate chain runs each gate fn via
> `interp::run_named_fn_as_bool`; `axon redteam` runs `redteam_check` in-process; Phase-9
> `Sandbox<P>`/`sandbox_run` effect-ceiling enforcement is interp-only; the Layer-3 self-improving-compiler
> firewall's G1 gate is an interp oracle; `axon trace --replay` replays through the interpreter. A program
> whose payload lives in HAL primitives is therefore **inert under every one of those** and live only under
> `axon build`. Combined with the hosted-build exposure in the row above and the default-substrate finding
> (§3), a generated artifact can pass review, redteam, sandbox, and deploy gates while carrying behavior
> none of them can observe. §11's "blast radius: confined to substrate files" understates this: substrate
> is the *default*.
>
> **Two required consequences, both narrowing (Slice 4a):** (1) **close the exposure** rather than accept
> it — gate the HAL `emit_call` arms on `self.freestanding` (the flag and `set_freestanding` already
> exist, threaded from `build_ir_and_link`), so a hosted `axon build` of a HAL primitive is an **error**,
> not a silent lowering; (2) state the residual invariant: **no interp-oracle result (redteam, sandbox,
> deploy gate, Layer-3 G1) may be cited as safety evidence for a program whose AST reaches `Hal`** —
> those artifacts require the QEMU oracle plus the §7 multi-sig, and `axon deploy` must refuse to
> green-light such a program on interp evidence alone. §8 adversarial items:
> `hosted_build_of_hal_primitive_is_refused`, `deploy_refuses_hal_program_on_interp_evidence`.

### 5. Type rules

- New type: **raw pointer `*T`** (`Type::RawPtr(Box<Type>)`) — substrate-only; the checker refuses it in any
  `surface`-reachable signature except behind a HAL boundary that re-wraps it in `Option`/`Result` (so I-3's
  "no null in safe code" holds — a raw null never escapes the substrate as a bare `*T`).
- New intrinsic sigs: `ptr_from_addr: fn(u64) -> *T`, `volatile_load: fn(*T) -> T`,
  `volatile_store: fn(*T, T) -> Unit`, `asm: <intrinsic, special-cased in the parser like Phase-8 blocks>`,
  the atomic family `atomic_{load,store,cas,fetch_add}: fn(*T, …, Ordering) -> …`.
  **CORRECTED 2026-07-31 — the landed sigs are over plain `i64`, not `*T`, which makes §7's I-3
  preservation argument vacuous.** `ptr_from_addr` is `params: &[("addr", "i64")], ret: "i64"`
  (`builtins.rs:1765-1770`, with the comment "opaque handle; codegen lowers to ptr"), and the whole
  `volatile_*`/atomic family follows. Addresses therefore cross every boundary as **ordinary integers**,
  so I-3's "a raw null never escapes the substrate as a bare `*T`" is true only because a bare `*T` never
  exists at the builtin surface at all — it is not a preserved property. Either type these intrinsics over
  `*T` for real (Slice 4b) or restate I-3's row honestly; do not leave the argument standing on a type
  the implementation does not use.
- **Unused proof machinery on exactly the primitives that need it most (ADDED 2026-07-31 — scoped as
  Slice 4b, not aspiration).** §4 says raw pointer arithmetic and int↔ptr casts are "UB … the author's
  responsibility (asserted, SMT-checked where in fragment)" and Slice 4 promises "SMT-proven page-table
  invariants". Nothing in the landed surface does any of this: `ptr_from_addr` takes an **unconstrained
  runtime `i64`** with no refinement and no region constraint, and codegen lowers whatever expression is
  passed (`emit_expr(&args[0])` → `build_int_to_ptr`, `codegen/expr.rs:8873-8890`). Yet this spec proves
  the cheap pattern **twice already**: the atomics `ordering` arg MUST be a compile-time literal (E1706)
  and `fn_addr`'s target MUST be a compile-time string literal naming a known fn (E1707), both validated
  in `codegen/expr.rs:700-770`. Meanwhile Phase-5 refinements are landed at all four obligation sites with
  default-pipeline SMT discharge, and §12 Q9's own fix made `__axon_refine_panic` **freestanding-safe**
  (the `'R'` debugcon trap) — so refinement enforcement demonstrably works in kernel mode. Today nothing
  statically distinguishes `volatile_store_u8(ptr_from_addr(0x3F8), b)` (serial) from a store to a
  computed page-table entry. "The author was careful" is a property of human authorship; it does not
  survive machine authorship, whereas *"is this address in a declared MMIO region?"* is exactly the shape
  of obligation a generator can be **required to discharge** and a checker can verify.
  **Slice 4b:** declare MMIO/kernel regions once per image (a substrate-level region table — the linker
  script already implies one) and type `ptr_from_addr: fn(a: u64 where in_region(a)) -> *T`, with SMT
  discharging the **constant** case at compile time (the common one — every landed kernel example uses
  address constants) and the freestanding `'R'` trap catching the computed case at runtime. For Slice 4
  and above, require the address to be a compile-time literal or a literal ± a constant-bounded offset,
  mirroring the E1706/E1707 precedent, under the reserved **E1708** (out-of-region / non-constant address)
  and **E1709** (region table malformed or absent) codes.
- New return type **`never`** (the divergent/`!` type) for `@[entry]`/`@[panic_handler]` — unifies with any
  type (bottom); a fn typed `never` that returns is a type error.
- New effect tag **`Hal`** in the effect catalog (`builtin_effect_row`); composes with Phase-6 subsumption
  (E1310) and `@[contained]` exactly like `Net`/`Fs`/`Exec`/`Gfx`(R16). **CORRECTED 2026-07-31 — the
  `@[contained]` half of that sentence is false against the tree.** The subsumption half is true (the
  walker is generic over effect names). But `parse_contained_spec` (`parser.rs` ~630-733) accepts exactly
  `fs`, `net`, `exec`, `never`, and registered native-module names — its own error message reads
  *"@[contained] unknown clause `{key}`, expected fs/net/exec/never …"* (`parser.rs:729`) — and
  `NeverClause` (`ast.rs:556-566`) is `Read | Write | Net | Exec | Spawn`. **There is no `hal:` grant axis
  and no `never: [hal]` deny clause.** Consequence:
  `@[contained(fs: [], net: [], exec: none, never: [exec, net("*")])]` — which reads as *maximal*
  containment to a human reviewer and to any tool that greps the annotation — leaves full MMIO, port I/O,
  `cli`, and arbitrary-physical-address writes untouched. This matters *more* for a capable generator, not
  less: `@[contained]` is the project's advertised containment primitive and is published in `CLAUDE.md`,
  so a generator that reads the docs learns precisely which axis is unmodelled. It is also structural, not
  a one-off — `@[contained]` **enumerates the axes it knows**, so every future effect is un-denyable by
  construction.
  **Resolution (Slice 4a, and the strictly better of the two options):** make `@[contained]`
  **deny-by-default over the whole `builtin_effect_row` catalog** — any effect not explicitly granted is
  denied, with an explicit `all` escape hatch for migration — so new axes are refused rather than silently
  permitted; add a `hal:` grant axis and a `never: [hal]` clause wired into the same checker path as
  fs/net/exec. Ship with the paired allow + deny tests I-11 already requires. Until that lands, this row's
  claim is narrowed to: **`Hal` is enforced by the substrate marker + E1310 subsumption ONLY, and is
  outside `@[contained]`'s vocabulary.**
- `@[repr(C)]`/`@[packed]`/`@[align(N)]` attach to `TypeDef` and drive codegen's struct layout (threads a new
  `Repr` field through the parser → checker → codegen `llvm_type_from_axon`).
- Unsigned ops: the existing `U8..U64` variants must route to the *unsigned* LLVM ops (zext, `udiv`/`urem`,
  unsigned `icmp`, logical `lshr`) — verify-and-complete, not new (§12 Q5).

### 6. Error codes

New block **E17xx / W17xx** (E16xx is R16 UI; 17xx is clear).

| Code | Trigger | Message shape |
|---|---|---|
| E1700 | unsafe primitive (`*T`, `volatile_*`, `asm`, `ptr_from_addr`, `@[hal]`) used in a `surface` file | `unsafe substrate primitive `volatile_store` not allowed in a surface file; move to a `substrate` HAL` — **PARTIALLY IMPLEMENTED (corrected 2026-07-31):** the only live E1700 emission is the `*T` raw-pointer *type* syntax (parser.rs:1197); `volatile_*`/`ptr_from_addr`/`@[hal]` named in a surface file are not E1700-refused directly — they are caught, at best, indirectly by the E1306/E1310 effect machinery. Either add the surface-builtin refusal or narrow this row's claim for good |
| E1701 | `@[hal]` fn body runs without the `Hal` capability minted to its Principal | **NOT IMPLEMENTED (corrected 2026-07-31):** no E1701 emission site exists anywhere in `crates/axon-core/src/` — the code exists only as a const in error.rs and the registered-codes list. Hal-capability minting is §11 Slice 4's deliverable; enforcement (and the `hal_without_capability_is_e1701` test) land there. Message shape when implemented: `fn `serial_write` needs the `Hal` capability; not granted by Principal `driver`` |
| E1702 | freestanding (no-runtime) build missing `@[entry]` or `@[panic_handler]` | `freestanding target needs `@[entry]` and `@[panic_handler]`; missing: panic_handler` |
| E1703 | surface fn transitively reaches a `Hal` fn without declaring `\| {Hal}` | **DELIVERED VIA PHASE-6 MACHINERY, not as E1703 (corrected 2026-07-31):** no E1703 emission site exists; Hal confinement actually rides E1306 (surface effect-row refusal, parser.rs:899) + the E1310 subsumption walker (effects.rs) — `checker.rs` contains zero Hal-specific logic. The code stays reserved; either emit it from the walker for the Hal case or formally retire it in favor of E1306/E1310. Nominal message: `fn `f` reaches Hal effect via `g` but its row omits {Hal}` |
| E1704 | `@[no_alloc]` fn reaches a heap-allocating builtin | `fn `isr_handler` is `@[no_alloc]` but calls `str_concat` (heap); ISR-unsafe` |
| E1705 | inline `asm` constraint/clobber malformed | **NOT IMPLEMENTED (corrected 2026-07-31):** despite Slice 1 landing, no E1705 validation exists anywhere in `crates/axon-core/src/` (the code is absent from `error.rs`'s registered list — E1700–E1704, E1706, E1707 only). A malformed constraint/clobber currently surfaces as a raw LLVM build error at codegen/assembly time, not an Axon diagnostic. The code stays reserved; implement before (or as part of) any further asm-heavy work — e.g. the SMP bring-up path — or formally retire it |
| E1706 | atomic builtin `ordering` arg is not a compile-time literal in 0..=4 (Slice 2) | `atomic_load_i64 ordering must be a compile-time literal (0=relaxed…4=seq_cst), not a runtime expr` |
| E1708 | *(reserved 2026-07-31, Slice 4b)* HAL address argument is not a compile-time literal (or literal ± constant-bounded offset), or is provably outside every declared MMIO/kernel region | `ptr_from_addr address must lie in a declared region; `base + n` is unconstrained` |
| E1709 | *(reserved 2026-07-31, Slice 4b)* substrate region table malformed or absent in a freestanding build | `freestanding image declares no MMIO region table; ptr_from_addr cannot be discharged` |
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
| **I-11** (capability boundary is real and total) | **EXTENDED — but the "gated identically to Net/Fs/Exec" claim is FALSE against the tree as of 2026-07-31; see the block below** | *aspiration:* hardware access becomes the new `Hal` capability axis. *Reality:* `Hal` is registered in the effect catalog and the subsumption walker only; it is absent from the risk derivation, `@[contained]`, and the audit ledger. The "strengthens I-11" claim holds **only after** the consumer checklist below is complete. |
| **I-12** (self-modification cannot weaken the TCB) | **preserved in principle; unenforced in practice until §3.5.1 lands** | the HAL + `@[panic_handler]` + `@[global_allocator]` are TCB components governed by the existing rule + multi-sig (R10/§7) — but the intrinsic set grew three times in one day (2026-07-20) with none of that process actually run. §3.5.1 is the admission gate that makes this row true. |

#### 7.1 `Hal` is invisible to the deploy risk pipeline — a raw-MMIO fn derives Risk = Low (ADDED 2026-07-31)

Phase 11's risk typing is the project's mechanism for forcing high-risk code through
`simulate → stress → redteam → verify` before deploy, and it derives risk **from effect rows**.
`derive_risk_from_ast` (`crates/axon-core/src/main.rs:5236-5281`) matches exactly `"exec" => 3`,
`"net" => …`, `"fs" | "io" => …`, and **`_ => {}`**. `Hal` — the effect covering `ptr_from_addr`,
`volatile_store_*`, `cli`/`sti`/`hlt`, `lidt`, `fn_addr`, and the SMP atomics (`builtins.rs:2164-2274`) —
hits the `_` arm. A function declaring `| {Hal}` therefore derives **Risk = 0 (Low)**, and `cmd_deploy`
runs only the short `redteam_check → assert_deployable` path: **no simulate, no stress.** The language's
single most privileged effect is classified as its least risky.

Worse for the trajectory question, `_ => {}` is an **open-world default-PERMIT**: every effect axis added
after Phase 11 (`Hal`, `Bpf` per R23, whatever R36/R37 add) is silently Low until someone remembers to
register it. That is precisely the shape a generator optimizing against the *acceptance gate* rather than
the goal will find — it need not defeat the gate, only pick the effect the gate has no row for.

**Required (Slice 4a):** register `Hal` in `derive_risk_from_ast` as **Critical (≥ Exec — hardware access
is strictly more privileged than process spawn)**, and change `_ => {}` to **fail closed**: an
unrecognized effect name raises risk to **High** and emits a diagnostic naming the unregistered axis, so a
new effect can never be quietly Low. Acceptance item (§9 Slice 4a):
**`hal_fn_derives_critical_risk_and_runs_full_pipeline`** — `axon deploy --json` on a `| {Hal}` program
reports `risk: critical` and `stages_run` containing simulate/stress/redteam.

#### 7.2 The effect-axis consumer checklist (ADDED 2026-07-31)

The I-11 "extension" claim is true only once a new axis is registered in **every** consumer of the effect
catalog. Adding an axis to `builtin_effect_row` alone is *not* extending the capability boundary. Any
future axis inherits this checklist:

| Consumer | Location | `Hal` status 2026-07-31 |
|---|---|---|
| effect catalog / subsumption walker (E1310) | `effects.rs` | ✅ registered |
| deploy risk derivation (Phase 11) | `main.rs:5236` `derive_risk_from_ast` | ❌ absent → derives Low (§7.1) |
| `@[contained]` grant + `never:` deny axes | `parser.rs` `parse_contained_spec`, `ast.rs` `NeverClause` | ❌ absent → un-denyable (§5) |
| Phase-9 `Sandbox<P>` effect ceiling | interp `sandbox_run` | ❌ moot — HAL is E0910 under interp (§4 note) |
| capability audit ledger (R28) | `crates/axon-audit/src/lib.rs` `EffectKind` | ❌ absent — variants are `FS \| Net \| AI \| Exec \| Random \| IO`, no `Hal`; `from_str` rejects anything else |
| approval artifact (`axon ast review`) | `main.rs:4881` | ❌ effects reduced to a bool (§3) |

**`Hal` has no audit vocabulary — the most privileged axis is the only one with no provenance trail.**
Because `EffectKind` has no `Hal` variant, hardware access **cannot be recorded in the immutable ledger
even in principle**; and because HAL is codegen-only (§6), there is no call-time recording path either.
So on the audit dimension `Hal` is the *opposite* of "gated identically" — the six less-privileged axes
are ledgered and the most privileged is not. As authorship shifts to generators, the audit chain is what
makes after-the-fact attribution possible, and hardware access is the class where attribution matters
most. **Slice 4:** add `EffectKind::Hal` (+ the Phase-9 F3 effect-row tag) and require Hal-capability
minting to write a ledger entry at **GRANT time** — a freestanding image cannot reach the ledger, so
record *which Principal was minted `Hal`, over which image content-hash, signed by the §7 multi-sig* —
paired with the image hash recorded at build time so a booted image ties back to an approved grant.
Acceptance item: **`hal_grant_is_ledgered`**, alongside `hal_without_capability_is_e1701`.

### 8. Test plan (maps 1:1 to §4)

> *(Checklist trued-up 2026-07-31 — these boxes sat unchecked long after the corresponding slices landed,
> contradicting the header; each checked item now cites its real gate.)*

- [x] **Unit (layout + static effect-row table):** `@[repr(C)]`/`@[packed]` produce the exact LLVM struct
      layout (golden IR); atomics/`fn_addr` carry `Hal` in the static builtin effect-row table. *Gates:
      `scripts/gdt_layout_ir_test.sh` (field-store-width-strengthened, R19 Slice D),
      `scripts/atomic_ir_test.sh`, `r17_atomics_carry_hal_effect_and_are_impure`,
      `r17_fn_addr_is_hal_impure_alloc_free` (builtins.rs unit tests — these assert the table entries
      only, nothing more).*
- [ ] **Unit (uncovered halves — split out 2026-07-31, vacuous-pass audit; the box above previously
      claimed all four properties on gates that test two):** `never` unifies as bottom (implemented at
      infer.rs:1854–1855, no cited gate tests it); `*T`/`volatile_*`/`ptr_from_addr` parse + type-check
      only in substrate (no such test exists — only the `*T` case has live enforcement, per §6); the
      `Hal` row propagates **transitively** through the walker (uncited). Named future tests:
      `r17_never_unifies_as_bottom`, `r17_volatile_substrate_only`, `r17_hal_row_transitive`.
- [x] **Integration (the acid test):** build a minimal `.ax` kernel with no host runtime, boot it under
      **QEMU**, assert it writes the expected bytes. *Gates: `scripts/qemu_boot_test.sh` /
      `r17_slice1_qemu_boot_writes_axon_s1` (debugcon "axon s1"), plus `scripts/timer_irq_qemu_test.sh` /
      `r17_timer_interrupt_fires_and_is_handled` (≥5 timer ticks, 0 panic markers).*
- [x] **CLI e2e (observable):** `axon build kernel.ax --freestanding` emits a bootable image; running it
      under QEMU halts with the expected output. *Same QEMU gates as above.*
- [ ] **Adversarial:** unsafe in a surface file (E1700); `@[hal]` without capability (E1701); missing
      panic_handler (E1702); surface laundering `Hal` through a helper (E1703 — the transitive case);
      `@[no_alloc]` ISR that allocates (E1704); HAL primitive under `axon run` (E0910). *Partially gated
      (2026-07-31 audit): `no_alloc_isr_rejects_heap_call_e1704` exists; E1700 (the `*T` case only) and
      E1702 enforcement are live in parser.rs/main.rs but `unsafe_outside_substrate_is_e1700` was never
      authored — still open. E1701 has NO enforcement to test (no emission site; see §6) — its test is
      re-scoped to Slice 4 with the capability-minting deliverable, not counted as Slice-0 test-debt.*
- [ ] **Property (invariant) — RESPECIFIED 2026-07-31; the original sweep was vacuous as written:**
      ~~no `surface` file in the whole corpus can name any unsafe primitive~~. As specified it quantifies
      over files that declare `surface`, and **there are zero such files**
      (`grep -rl '^surface' examples --include=*.ax` returns none across 170 files), so
      `no_surface_file_can_name_unsafe` would go **green on its first run while checking nothing** — the
      exact vacuous-pass class this project has already been bitten by (glob sweeps that pass over an
      empty set). It also measures the wrong direction for machine authorship: the interesting question is
      not "do the files that opted into safety stay safe" but **"which files hold unsafe power, and did
      anyone approve that"**. Replaced by **`unsafe_reach_matches_substrate_allowlist`**, over **ALL**
      `.ax` files: (a) compute the set that reaches any R17 HAL primitive, directly or transitively (the
      E1704 reachability walker at `checker.rs:1753` already does this); (b) assert that set is
      **non-empty** and **exactly equals** a checked-in `governance/state/substrate-allowlist` — a new
      unsafe file fails the gate until listed; (c) assert every member declares `substrate` **explicitly**
      rather than relying on the default marker; (d) assert every non-member is parse-clean under
      `surface` mode. The allowlist diff carries the §7/R10 multi-sig. That converts a vacuous check into
      the actual containment boundary, and it keeps meaning something when most files are model-authored.
- [ ] **Adversarial (trajectory items — ADDED 2026-07-31, Slice 4a):**
      `approved_ast_cannot_gain_hal_after_signoff` (post-approval `Hal` addition invalidates the R22
      token); `hosted_build_of_hal_primitive_is_refused` (§4 exposure closed by a codegen-side
      `self.freestanding` gate); `deploy_refuses_hal_program_on_interp_evidence` (no interp-oracle result
      is safety evidence for a `Hal`-reaching program); `hal_fn_derives_critical_risk_and_runs_full_pipeline`
      (§7.1); `unregistered_effect_axis_fails_closed_to_high_risk` (§7.1's `_ => {}` fix);
      `hal_builtins_match_tcb_manifest` (§3.5.1).
- [x] **Parity:** N/A interp↔codegen for HAL leaves (codegen-only, §6 note); QEMU is the oracle. Non-hardware
      substrate logic keeps normal interp↔codegen parity. *By-construction (E0910 interp refusal).*
- [ ] **Journey/red-team:** the "axon booted" kernel *(done — QEMU gate)* + a timer-interrupt handler
      *(done — `timer_irq_qemu_test.sh`)* + an SMP atomic counter incremented from 2 cores *(NOT done —
      the deferred 2-core harness, see §9 Slice 2's unchecked item)*, each verified under QEMU.

### 9. Acceptance criteria (the done gate — per slice)

> **Side-effect-bounding policy (ADDED 2026-07-31 — stated alongside the golden-IR-proxy policy in
> Slice 2).** Every QEMU gate in this spec is currently a **positive-only existence assertion**, and the
> landed scripts confirm it: `qemu_boot_test.sh` asserts the debugcon *contains* `"axon s1"`;
> `timer_irq_qemu_test.sh` counts `grep -o "T" | wc -l ≥ 5` and `grep -oE "[ABR]" | wc -l == 0`
> (`timer_irq_qemu_test.sh:136-137`); the deferred `axon_smp_2core_counter_exact` asserts the counter
> reads exactly 200000. **None constrains the rest of the image's behavior** — extra MMIO writes, extra
> port I/O, memory touched outside the mapped region, extra debugcon output beyond the counted markers, or
> what runs before and after the measured window. §6 rightly moves the parity oracle to "emulator
> behavior", but the oracle today is a substring/count check, not a transcript. For human-written kernel
> code that is normal and fine; for code produced by a generator optimizing the gate predicate, "boots and
> emits ≥5 `T` with 0 `ABR`" is trivially satisfiable while doing anything else — and §8's Journey/red-team
> row treats these same gates as red-team evidence, which they are not.
>
> **Policy: an acceptance gate for substrate code must bound the observable side-effect set, not merely
> witness the intended one.** Concretely: (1) convert each QEMU gate from a positive count to an
> **exact-transcript** assertion (the full expected debugcon byte sequence; unexpected bytes = fail — the
> counts already exist, so this is a small change); (2) add negative-space coverage via QEMU's own
> instrumentation (`-d` / port-I/O and memory-write tracing) checked against an allowlist derived
> mechanically from the `ptr_from_addr`/`port_out_u8` address constants appearing in the source. Apply it
> to `axon_smp_2core_counter_exact` **before** that harness is written, while it is still cheap.

**Slice 0 (v0 — "it boots"):** 🟡 Implementation LANDED (076c445) — **done-gate NOT fully met.**
*(Re-corrected 2026-07-31: the earlier same-day "✅ LANDED" stamped the slice done while two of its three
acceptance bullets sat unchecked — a slice cannot be ✅ against its own unmet done-gate. The code shipped;
the open items below are the remaining gate.)*
- [x] `axon_kernel_boots_qemu_serial_hello` — a no-runtime `.ax` kernel boots under QEMU and writes output.
      *Satisfied (under a different name) by Slice 1's `scripts/qemu_boot_test.sh` /
      `r17_slice1_qemu_boot_writes_axon_s1`, which subsumes this criterion (real QEMU boot + debugcon
      bytes).*
- [ ] `unsafe_outside_substrate_is_e1700` passes. *(2026-07-31 audit: an E1700 refusal is live for the
      `*T` raw-pointer type syntax — parser.rs:1197, the ONLY E1700 emission site in the tree — but the
      named test does not exist; still open. Note the live enforcement is narrower than §3/§6 state:
      `volatile_*`/`ptr_from_addr`/`@[hal]` named in a surface file are not E1700-refused directly, only
      caught indirectly by the Phase-6 effect machinery — see the corrected §6 rows.)*
- `hal_without_capability_is_e1701` — **re-scoped out of Slice 0 to Slice 4 (2026-07-31).** E1701 has no
      emission site anywhere in `crates/` (it exists only as a registered code in error.rs), and the test
      *cannot* be authored before Hal-capability minting exists — which is §11 Slice 4's deliverable.
      Listing it here mislabeled enforcement-debt as test-debt; it is now a Slice-4 acceptance item.
- [ ] ~~`no_surface_file_can_name_unsafe` (corpus sweep)~~ → **superseded 2026-07-31 by
      `unsafe_reach_matches_substrate_allowlist`** (§8 Property): the original sweep quantified over
      `surface`-declaring files, of which the corpus has **zero**, so it would have passed vacuously on
      first run. The replacement sweeps ALL `.ax` files, asserts the HAL-reaching set is non-empty and
      equals a multi-sig-gated `governance/state/substrate-allowlist`, and requires every member to
      declare `substrate` explicitly. Still open.

**Slice 1 (asm + interrupts):** ✅ LANDED (bf97c55) — hex/binary literals, `asm(...)` real codegen, `@[naked]`/`@[interrupt]`, `hlt`/`cli`/`sti` inline asm, linker script.
- [x] Hex/binary/underscore integer literals parse and evaluate correctly.
- [x] `asm(...)` emits real LLVM inline asm in codegen; E0910 in interpreter.
- [x] `@[naked]` → LLVM "naked" attribute; `@[interrupt]` → x86-interrupt CC 83.
- [x] `hlt`/`cli`/`sti` HAL builtins emit real inline asm (not const_zero placeholders).
- [x] `scripts/kernel.ld` + `--linker-script` CLI option wired into freestanding link.
- [x] `r17_slice1_qemu_boot_writes_axon_s1` — kernel boots under QEMU and writes "axon s1" to debugcon (76860bc). Uses multiboot1 + boot_stub.asm (32→64 mode switch) + port_out_u8 for QEMU debugcon; test skips gracefully if nasm/qemu absent.
- [x] `axon_kernel_handles_timer_interrupt` — full IDT + PIC + timer ISR fires under QEMU. **PASSES for
  real (2026-07-20; checkbox trued-up 2026-07-31 — it sat unchecked/"deferred" after landing):** gated by
  `scripts/timer_irq_qemu_test.sh` + cargo test `r17_timer_interrupt_fires_and_is_handled`
  (integration_fixtures.rs) — 194 ticks in 2s, zero panic markers; full chain per §12 Q8/Q9.

**Slice 2 (SMP + atomics):** ✅ LANDED (a7b262c).
- [x] `atomic_load_i64`/`atomic_store_i64`/`atomic_fetch_add_i64`/`atomic_cas_i64` (substrate-only, Hal effect, E0910 in interp). The trailing `ordering` arg is a compile-time integer literal (0=relaxed,1=acquire,2=release,3=acq_rel,4=seq_cst); non-literal/out-of-range → E1706 (codegen). Each lowers to the real LLVM atomic with the named order (`atomicrmw add … seq_cst`, `load atomic … acquire`, `store atomic … release`, `cmpxchg … seq_cst monotonic`). `Send`/`Sync` cross-core sharing is governed by the same Hal effect-subsumption walker (E1310; surface files can't declare `| {Hal}`, E1306).
- [x] `axon_smp_atomic_counter_is_race_free` — **golden-IR proxy** (`scripts/atomic_ir_test.sh`): the SMP counter increment lowers to a single `atomicrmw add … seq_cst`, the load-bearing race-freedom property. A full 2-core QEMU SMP boot harness is heavier infra and is **deliberately deferred** (see the unchecked item below). New `axon build --emit-llvm` (IR-text dump) added for the golden inspection. Demo: `examples/kernel/hello_kernel_slice2.ax`. *(Citation repaired 2026-07-31: this item previously justified the deferral with a quote — 'per §9: "a pure-codegen golden-IR test is acceptable as the unit gate"' — that appears nowhere in §9 or anywhere else in this spec; it was a dangling self-reference. The actual policy is now stated explicitly:)* **Golden-IR-proxy policy:** a pure-codegen golden-IR check is acceptable as the *unit* gate only when the property under test is a single-instruction lowering (here: the increment IS one `atomicrmw add … seq_cst` — the script asserts instruction *content*, not just shape, per the R19-Slice-D lesson that shape-only goldens shipped a real memory-corruption bug). It is NOT acceptable as the *acceptance* gate for multi-core behavior: it proves nothing about AP bring-up, per-core stacks, identity mapping, or actual cross-core contention — which is exactly what the unchecked harness item below exists to cover.
- [ ] `axon_smp_2core_counter_exact` (`scripts/smp_qemu_test.sh`) — **the deferred 2-core QEMU SMP harness, the only remaining QEMU-verified acceptance item** (the Slice-0 test-authoring gates — E1700 adversarial test, surface sweep — and the Slice-4-rescoped E1701 test remain open alongside it; see Slice 0 and the header). *(Added 2026-07-31: the header called this "the only remaining gap" but §9 contained no unchecked item, test name, or pass condition for it — the done-gate could not fail because it did not exist. Same-day correction: the "only remaining acceptance item in this spec" wording overclaimed against §9's own Slice-0 audit.)* Concrete pass condition: QEMU `-smp 2`; the BSP boots via the existing `boot_stub.asm` path and brings up **1 AP** via INIT-SIPI-SIPI (real-mode trampoline in the pre-Axon boot stub or a new native builtin — **not** R13 FFI, per §3.5's correction / §12 Q2); each core gets its own stack and runs inside the (16 MiB) identity-mapped region; each core performs **K = 100000** `atomic_fetch_add_i64(…, 1, seq_cst)` increments on the shared counter; after both signal completion (an atomic done-flag per core), the BSP reads the counter and writes it to debugcon; the test asserts the value is **exactly 2·K = 200000** (any torn/lost update fails). Timeout: 30s wall. Standard skip clause: skips gracefully (like `qemu_boot_test.sh`/`timer_irq_qemu_test.sh`) if `nasm`/`qemu-system-x86_64` are absent or the build lacks codegen. Mirrored as a cargo test `r17_smp_2core_counter_is_exact` in `integration_fixtures.rs`.

**Slice 3 (layout + no_alloc):** ✅ LANDED.
- [x] `@[repr(C)]`/`@[packed]`/`@[align(N)]` drive struct layout: `@[packed]` lowers to LLVM's packed-struct form (`<{ … }>`, no inter-field padding), `@[repr(C)]` keeps declaration-order C layout. Golden-IR `axon_repr_c_gdt_layout_byte_exact` (`scripts/gdt_layout_ir_test.sh`): the GDT entry lowers byte-exact to `%GdtEntry = type <{ i16, i16, i8, i8, i8, i8 }>`. `@[align(N)]` is parsed/accepted; the LLVM struct *type* carries only the packed bit, so explicit alignment applies at allocation sites (the struct-type golden is the load-bearing layout check).
- [x] `@[no_alloc]` fn reaching a heap-allocating builtin / string interpolation / a transitively-allocating helper → **E1704** (enforced like `@[pure]`/`@[total]`, transitive — closes the laundering hole). `no_alloc_isr_rejects_heap_call_e1704` passes. Heap classification (`is_heap_allocating_builtin`) is a conservative over-approximation (heap-typed return OR known mutator); HAL/atomic leaves classify allocation-free. Demo: `examples/kernel/hello_kernel_slice3.ax`.

### 10. Performance budget

- Freestanding image size: a "hello-serial" kernel ≤ **64 KB** (no `axon-rt` host bloat — the no-runtime mode
  must link none of the hosted externs). ~~Guarded by an image-size check.~~ **CORRECTED 2026-07-31: no
  such check exists** — no size assertion appears anywhere in `scripts/`, and the QEMU gates assert only
  debugcon content. Minor as a correctness matter, but this is the one gate in the spec that would notice
  a generated kernel quietly pulling in far more than the author intended — a plausible symptom of the
  §3/§4/§7.1 failure modes and a cheap tripwire that is currently **not armed**. **Fix:** add the
  assertion to `qemu_boot_test.sh` (the image is already built there — one `stat`/`wc -c` comparison) and
  **print the measured size in the test output** so drift is visible continuously, not only at the
  threshold. If 64 KB is no longer right after the Q8 16 MiB identity-map change, restate the budget
  rather than leaving an unenforced number in the spec.
- `volatile_*` / atomics must lower to a *single* instruction (no wrapper-call overhead) — verified by IR
  inspection in the unit golden-IR tests.
- ISR latency / context-switch budgets: deferred to a real scheduler slice (out of scope for v1).

### 11. Rollout & rollback

**Gated by the `substrate` file marker** (surface refusal + E0910 interpreter refusal); the
`--freestanding` CLI flag (`axon build … --freestanding`, `main.rs`) gates **link mode, the E1702 entry
check, and Q9 trap synthesis only** — it does NOT gate the unsafe primitive lowering (re-corrected
2026-07-31, see §4 last row). *(Also corrected: earlier drafts said "feature-flagged behind
`--features freestanding`", but no such cargo feature exists — `crates/axon-core/Cargo.toml` defines no
`freestanding` feature (its features: `default`/`codegen`/`serde-json`/`asi-runtime`/`smt`/`gfx-wgpu`).
The real, landed gates are: the `substrate`-file requirement, the E1700/E1306 surface refusals, E0910
interpreter refusal, and — for link mode only — the `--freestanding` CLI flag. Slice 4 implementers
should extend those gates, not look for a compile-time feature; adding a codegen-side
`Codegen.freestanding` refusal on the HAL primitives would close the hosted-substrate-build exposure.)*
The default hosted build, every example, and all ~700 tests are untouched (a `surface` file can never
reach the unsafe surface — that refusal, not the flag, is the load-bearing gate). Sliced, each
independently revertible:

| Slice | Deliverable | Revertible? |
|---|---|---|
| **0 — boots** | freestanding target + no-runtime mode (`@[entry]`/`@[panic_handler]`) + raw `*T`/`volatile_*`/`ptr_from_addr` (substrate+`Hal`-gated) → QEMU serial-hello | yes — pure addition behind flag |
| **1 — asm/interrupts** | inline `asm`, `@[naked]`, `@[interrupt]` (x86-interrupt ABI), IDT handling | yes |
| **2 — SMP/atomics** | atomic intrinsics + memory ordering + `Send`/`Sync`-equivalent | yes |
| **3 — layout/no_alloc** | `@[repr(C)]`/`@[packed]`/`@[align]` + `@[no_alloc]` checker | yes |
| **4a — machine-authorship containment** *(ADDED 2026-07-31; strictly narrowing, no kill-gate relaxed — should land BEFORE Slice 4, since it is what makes Slice 4's capability meaningful)* | default-`surface` for unmarked + generator-emitted files, `substrate` marker bound into the R22 approval token + a multi-sig substrate allowlist (§3); `axon-ast-review/2` with `effect_set`/`hal_calls`/`file_mode` + the UI blocking banner (§3); `Hal` → Critical in `derive_risk_from_ast` and `_ => {}` → fail-closed High (§7.1); `@[contained]` deny-by-default over the effect catalog + `hal:` / `never: [hal]` (§5); codegen-side `self.freestanding` refusal closing the hosted-build exposure (§4); the §3.5.1 TCB-intrinsic manifest + cap + retro-filing of `fn_addr`/`lidt`/the freestanding trap | yes — each item is an independent tightening |
| **4 — capability HAL + TCB** | the trusted HAL as a content-addressed TCB component; `Hal`-capability minting (R11) **+ E1701 enforcement + the `hal_without_capability_is_e1701` test (re-scoped here from Slice 0, 2026-07-31 — E1701 cannot exist before minting does)** **+ `EffectKind::Hal` and grant-time ledgering, `hal_grant_is_ledgered` (§7.2, added 2026-07-31)**; SMT-proven page-table invariants | yes |
| **4b — HAL address refinement** *(ADDED 2026-07-31)* | `ptr_from_addr: fn(a: u64 where in_region(a)) -> *T` over a substrate region table, SMT-discharged for the constant case, E1708/E1709; the `*T`-vs-`i64` sig mismatch resolved so I-3's row is non-vacuous (§5) | yes |

**Blast radius:** confined to `substrate` files (the `--freestanding` flag additionally scopes link
mode). *(Re-corrected 2026-07-31: "confined to `--freestanding` builds" was false — a hosted native build
of a substrate file compiles HAL primitives, per §4.)* The unsafe primitives have no interpreter path
(E0910) and no surface path (E1700/E1306), so they cannot affect surface or interpreted execution; a
hosted *native* build of substrate code remains the accepted exposure noted in §4. The riskiest
part is the *invariant amendments* (§7) — each gated to the slice that needs it, via the standard process.

### 12. Open questions

1. **(strategic — RESOLVED 2026-06-19, marker added 2026-07-31)** The §2.3 reversal: is "build a core Axon
   OS from the ground up" a real commitment or an exploration? **Resolved: COMMITTED by founder decision
   2026-06-19** (see header + spec-meta); ROADMAP §2.3 reversed; Slices 0–3 are landed. The original
   default ("treat as exploration; defer Slices 1–4") is superseded by that decision and by the landed
   slices themselves. This entry read as an open blocker for weeks after the header said resolved —
   stale-marker corrected 2026-07-31.
2. **(§3.5 — default SUPERSEDED 2026-07-31)** Native intrinsic vs R13-FFI-to-asm boundary: exactly which
   privileged ops are native vs trampolined. *Original default: the pervasive/perf-critical set is native
   (raw ptr/volatile/atomics/hlt); one-off bring-up (lgdt/lidt/mode-switch/SMP-trampoline) is R13 asm.*
   **Superseded:** the R13 leg of this default is dead — for the **linkage** reason, not the E0910 reason
   Q7 originally cited *(re-grounded 2026-07-31: R13 Slice 4 codegen-lowers native calls, only
   `modbus`/`fhir`/`fix` are E0910-refused; but R13 calls target host-runtime shim symbols a freestanding
   image never links — Q9's `__axon_arith_panic` precedent, same structural class)*. The proven
   replacement split (Q8 precedent): the
   native set stays minimal as before; one-off privileged sequences land as either **hand-written native
   HAL builtins** (`lidt`, `port_out_u8` — `create_inline_asm` in `codegen/expr.rs`) or **pre-Axon nasm
   boot-stub code** (`boot_stub.asm`); the SMP AP trampoline takes the boot-stub path. See the §3.5
   correction block.
3. **(§3 — RESOLVED-BY-DEFAULT, marker added 2026-07-31)** `unsafe` ergonomics: a block (`unsafe { }`) vs
   an attribute (`@[hal]`/`@[unsafe]` fn). **Resolved: the default won in practice** — every landed slice
   implements the attribute + substrate-file gate (`@[hal]`, E1700); no `unsafe { }` block exists. Revisit
   only if intra-fn granularity is ever needed.
4. **(§3, Slice 0 — RESOLVED 2026-07-31, decision diverged from the default)** Bootloader: bring-your-own
   (Limine / multiboot2 / GRUB) vs custom. *Original default: BYO (multiboot2 or Limine handoff).* **What
   was actually built (Slice 1, 76860bc): a minimal multiboot1 nasm boot stub**
   (`examples/kernel/boot_stub.asm` — multiboot1 header + 32→64 long-mode switch + identity map, now
   16 MiB per Q8), chosen because QEMU's multiboot1 ELF loader rejects a 64-bit ELF directly (see the
   stub's own comments) and a ~150-line stub was cheaper than a Limine dependency. Axon owns everything
   post-handoff, as intended.
5. **(§5 — RESOLVED via R19; stale-marker corrected 2026-07-31)** Unsigned-integer support. The 2026-06-19
   audit below was accurate *then*: `U8..U64` types existed but were non-functional (`let a: u32 = 1` →
   E0102), making unsigned support a build prerequisite. **That prerequisite has since been fully
   delivered** by `R19-fixed-width-integers.md`: Slice A (construction surface, E1900 range check), Slice B
   (width-correct interp arithmetic), Slice C (native codegen parity — `scripts/unsigned_parity.sh` green,
   byte-identical interp==native), and Slice D (the `as` cast family + the packed-struct field-store
   width-corruption fix, landed 2026-07-20 and cited by this spec's own header/evidence). This entry read
   as an open Slice-0 blocker for over a month after R19 landed. *Historical audit (2026-06-19, now moot):
   literals defaulted to `i64` with no coercion to unsigned; repro `let a: u32 = 1` → E0102.*
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
   ("lgdt/lidt/mode-switch is R13 asm") didn't resolve this either: R13 native calls lower to shim
   symbols implemented in the host-runtime staticlib, which a freestanding image never links — unusable
   for a kernel with no host runtime underneath it. *(Corrected 2026-07-31: this passage originally
   claimed R13 was "interp-only, codegen-E0910-refused" — false; R13 Slice 4 codegen-lowers FFI and was
   already landed when this Q was written. The exclusion is linkage, per the re-grounded §3.5 block /
   Q2.)*

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

   **RESOLVED same day (2026-07-20).** Took candidate (a). Added a `freestanding: bool` field to
   `Codegen` (`set_freestanding`, mirrors the existing `set_target_is_wasm`) threaded from
   `build_ir_and_link` in `main.rs`. Added `synthesize_freestanding_trap` (`codegen/expr.rs`, the
   R1e-convergence-allowlisted file for raw IR emission — `port_out_u8`/`lidt`'s hand-written
   `create_inline_asm` already lives there): DEFINES a real in-module function (not an external
   declaration) that ignores its arguments (no formatting without a host runtime), writes a
   distinguishing marker byte to the QEMU debugcon port (0xE9, the diagnostic convention every
   other R17 example already uses) via the same `outb` inline asm `port_out_u8` uses, then halts
   forever. Wired into the three IMPLICIT/automatic safety checks a substrate author can't opt out
   of — `__axon_arith_panic` ('A'), `__axon_bounds_panic` ('B'), `__axon_refine_panic` ('R') — when
   `self.freestanding` is set; left `__axon_verify_panic`/`__axon_assert_eq_*_panic`/
   `__axon_msg_panic`/`__axon_random_inverted_panic` as still-external, since those are all
   explicit, opt-in API calls (`@[verify]`, `assert_eq`, `assert`, `random_*`) a kernel author can
   simply avoid, unlike arith/bounds/refine which the compiler inserts unconditionally.

   **Verified both directions, not just the happy path:** `hello_kernel_timer_irq.ax` now links AND
   boots — confirmed via a real QEMU run that the timer interrupt fires **194 times in 2 seconds**
   (~97 Hz, matching the programmed PIT divisor), with **zero** spurious 'A'/'B'/'R' trap markers
   (the IDT-fill loop's real arithmetic never actually overflows, as expected). Separately confirmed
   the trap itself is correct, not just inert: a deliberate `i64::MAX + 1` overflow in a standalone
   probe produces exactly one 'A' byte on debugcon, then the kernel halts cleanly (no crash, no
   further output, no memory corruption) — the trap fires exactly when it should and is silent
   otherwise. **This is the full, real `axon_kernel_handles_timer_interrupt` acceptance test,
   genuinely passing end to end** — IDT construction (§12 Q8's fixed-address idiom) → `lidt` → PIC
   remap → PIT programming → `sti` → a real hardware interrupt → an Axon-compiled `@[interrupt]`
   handler → EOI → repeated firing. Landed as `scripts/timer_irq_qemu_test.sh` (mirrors
   `qemu_boot_test.sh`'s structure) and the cargo test `r17_timer_interrupt_fires_and_is_handled`
   (`crates/axon-core/tests/integration_fixtures.rs`), asserting ≥5 ticks and 0 panic markers.

   One process note: the fix initially landed the trap-synthesis method in `codegen/mod.rs`, which
   the `r1e_direct_ir_emission_stays_confined` convergence test correctly caught as a NEW raw-IR
   file outside its allowlist — moved to `expr.rs` (where the identical-style `port_out_u8`/`lidt`
   hand-rolled asm already lives) and the test passed clean. A useful confirmation that this
   session's existing architectural-convergence tests catch real drift, not just synthetic cases.

10. **(added 2026-07-31 — ASI-trajectory pass; OPEN, strategic, not decided here) Does inverting the
    default file mode to `surface` break the existing corpus, and who owns the migration?** §3's
    correction requires that an unmarked `.ax` file parse as `surface`. Against the tree that flips **160
    of 170** example files from implicitly-substrate to surface. Most will not care (they name no unsafe
    primitive and no raw effect-row syntax), but `surface` also rejects raw effect-row syntax (E1306), so
    the true blast radius is *unmeasured*. Open sub-questions: (a) is the migration a mechanical sweep
    (add `substrate` to the files that need it, driven by the same reachability walker as
    `unsafe_reach_matches_substrate_allowlist`) or does it need per-file judgement? (b) does the inversion
    apply only to generator-emitted files first (narrower, shippable immediately) with the corpus-wide
    flip as a later, separately-gated step? (c) `.ax` is also the R22 approval artifact — does flipping a
    file's mode invalidate previously issued approval tokens, and is that desirable (fail-closed) or
    disruptive? **No default is asserted here on purpose**: the measurement (how many corpus files
    actually fail under `surface` mode) is a one-command experiment and should be run before choosing.
    Do **not** resolve this by weakening the inversion.

11. **(added 2026-07-31 — ASI-trajectory pass; OPEN) What is the QEMU oracle's authority, given HAL is
    invisible to every interpreter-based gate?** §4's added block establishes that redteam / sandbox /
    deploy-gate / Layer-3 G1 evidence is worthless for `Hal`-reaching programs, leaving QEMU as the sole
    behavioral oracle. But §9's new side-effect-bounding policy also establishes that the QEMU gates
    currently assert existence, not bounded behavior. So the question this spec cannot answer today: **is
    an exact-transcript + traced-port-I/O QEMU run sufficient evidence to deploy machine-authored kernel
    code, or does substrate code require a categorically different acceptance form** (e.g. a proof
    obligation discharged statically per Slice 4b, with QEMU as corroboration rather than authority)?
    Relevant prior art inside this project: Slice 2's golden-IR-proxy policy already draws exactly this
    unit-vs-acceptance distinction. Deciding it changes what Slice 4b is *for*, so it should be settled
    before 4b is scoped in detail.

12. **(added 2026-07-31 — ASI-trajectory pass; OPEN) What is the standing budget for TCB growth, and what
    happens when the cap in §3.5.1 is hit?** §3.5.1 writes down a v1 ceiling of 24 native HAL intrinsics
    because "fixed" needs a number, but the number is an estimate, not a derivation. Open: (a) is the
    right unit *count of intrinsics*, or something closer to the real risk surface (lines of hand-written
    `create_inline_asm`, distinct privileged instructions reachable, distinct hardware resources
    touched)? (b) when the cap binds, is the correct response a spec amendment, or is the cap a genuine
    stop — i.e. does the deferred SMP AP bring-up path get *refused* rather than granted a new intrinsic,
    pushing it into the nasm boot stub (which is outside the language and therefore outside the growth
    ratchet)? The §3.5 correction already establishes the boot stub as the preferred home for one-off
    privileged sequences, which argues (b) is the intended answer — but that makes the cap a real
    forcing function, not paperwork, and it should be adopted deliberately.
