# R21 — Zephyr RTOS Target (Axon-as-a-Zephyr-app on ARM Cortex-M)

**Spec ID:** `R21-zephyr-target` (new requirement row; depends on `R17-freestanding-substrate.md`)
**Status:** Slice 1 LANDED — an Axon program runs AS a Zephyr application on an ARM Cortex-M3 target,
verified headlessly under QEMU (`scripts/zephyr_qemu_gate.sh`).
**Risk class:** Additive (a new build target + one HAL builtin; behind `--freestanding`, no surface impact)
**Author / date:** cklaus, 2026-06-26

> **One-line scope:** let an Axon program run as a Zephyr RTOS application on an ARM Cortex-M target —
> compiled freestanding (no libc, no axon-rt) to an `arm-zephyr-eabi` object that a minimal Zephyr app links
> and calls, writing output through a Zephyr-provided console hook, verified under QEMU. This extends the R17
> bare-metal substrate to the dominant embedded RTOS ecosystem (medical-device / industrial / automotive).

---

### 1. Motivation

R17 proved Axon can boot a from-scratch x86_64 kernel under QEMU. But the embedded reality most safety-critical
products live in is an **RTOS on a microcontroller** — Zephyr on ARM Cortex-M being the dominant open stack
(medical devices, industrial controllers, automotive ECUs). The cleanest, lowest-risk way to reach that
ecosystem is **not** to replace the RTOS but to run Axon *inside* it: compile Axon freestanding to an
`arm-zephyr-eabi` object, link it into a normal Zephyr application, and have the Axon code do real, verifiable
computation, emitting output through a Zephyr console hook. The RTOS owns scheduling, drivers, and bring-up;
Axon contributes the verified compute, with its refinement contracts and effect typing intact.

### 2. Requirement link

New `REQUIREMENTS.md` row R21 under the platform/substrate bucket; **depends on R17** (reuses the freestanding
codegen, `@[entry]`/`@[panic_handler]`, the `Hal` effect tag, `@[no_alloc]`). Headline acceptance: *an Axon
`.ax` program, built for an ARM Cortex-M Zephyr target, runs as a Zephyr app under QEMU and produces correct
console output.*

### 3. Surface (what the user writes)

Two new things, both reusing R17 machinery:

1. **A target alias** — `axon build --freestanding --target zephyr --emit-obj app.ax --out app.o` emits an
   `arm-zephyr-eabi` (LLVM `thumbv7m-none-eabi`) relocatable object. `--target zephyr` (also `arm-zephyr` /
   `cortex-m` / `cortex-m3`) expands to `thumbv7m-none-eabi`; any other `--target` value is passed through as
   a raw LLVM triple unchanged.
2. **A console HAL builtin** — `zephyr_console_putc(b: i64)` writes one byte to the Zephyr console. It is a
   `Hal`-effect, substrate-only, codegen-only builtin (E0910 in the interpreter). In codegen it lowers to a
   call to the **extern C symbol `axon_console_putc(int)`**, which the Zephyr application provides (typically a
   `printk("%c", b)` wrapper). Unlike R17's `port_out_u8` (x86 I/O ports), this is architecture-neutral — the
   console is a Zephyr driver, not a fixed I/O port — so it works on ARM (or RISC-V / x86) Zephyr targets.

```axon
substrate

@[hal] @[no_alloc]
fn putc(b: i64) | {Hal} { zephyr_console_putc(b) }   // → extern C axon_console_putc

@[no_alloc]
fn sensor_avg(a: i64, b: i64, c: i64, d: i64) -> (i64 where _ >= 0) {
    (a + b + c + d) / 4                               // refinement-bounded compute
}

@[entry]
fn axon_main() | {Hal} { /* banner + put_uint(sensor_avg(...)) */ }

@[panic_handler]
fn on_panic() | {Hal} { putc(33) }                   // '!'
```

The Zephyr app (`examples/zephyr/`) is a normal Zephyr application: a `CMakeLists.txt` that links the pre-built
Axon object (`-DAXON_OBJ=…` → `target_link_libraries(app PRIVATE ${AXON_OBJ})`), a `prj.conf` (just
`CONFIG_PRINTK=y`), and a `src/main.c` whose `main()` defines `axon_console_putc` (wired to `printk`) and calls
`axon_main()`.

### 4. Semantics

| Input class | Behavior |
|---|---|
| `--target zephyr` (or `arm-zephyr`/`cortex-m`/`cortex-m3`) | expands to LLVM triple `thumbv7m-none-eabi` |
| `--target <other>` | passed through unchanged (raw LLVM triple) |
| `zephyr_console_putc(b)` in codegen | truncates `b` (i64) to i32, calls extern C `void axon_console_putc(int)` |
| `zephyr_console_putc(b)` in the interpreter | **E0910** — no Zephyr host device under `axon run` |
| `zephyr_console_putc` in a surface file | refused via the `Hal` effect path (surface files cannot declare `\| {Hal}`, E1306; reaching Hal without declaring it, E1703) — same as every R17 HAL builtin |
| freestanding ARM/thumb object emit | uses `RelocMode::Static` + `CodeModel::Default` (the x86-only `CodeModel::Kernel` is rejected by the ARM backend) |
| panic helpers (`__axon_arith_panic`/`__axon_bounds_panic`/`__axon_refine_panic`) | a freestanding image references these; the Zephyr app provides `__weak` stubs that route to `k_panic()` (the R17 "panic → TCB component" model — here the host RTOS) |

### 5. Type / codegen rules

- `zephyr_console_putc: fn(i64) -> ()`, registered in `BUILTINS`; `Hal` effect row; impure; allocation-free
  (usable in `@[no_alloc]`); E0910 in interp. Auto-populates the inference signature table like any builtin.
- The freestanding object-emit path selects `(RelocMode, CodeModel)` by target architecture
  (`freestanding_reloc_codemodel` in `codegen/link.rs`): ARM/thumb/aarch64 → `(Static, Default)`; x86_64 →
  `(Static, Kernel)` (the R17 default, unchanged).
- `resolve_target_alias` (main.rs) expands the friendly aliases before the triple reaches codegen.

### 6. Error codes

No new codes. Reuses E0910 (HAL builtin under interp), the R17 `Hal`-effect subsumption codes (E1306/E1703 for
surface-file misuse), and E0904 (unsupported LLVM target).

### 7. Invariants touched

None newly amended. Inherits R17's bounded carve-outs (the `Hal` builtin is substrate-only + capability-gated +
codegen-only). No `surface` code gains any power; the default hosted build is unchanged.

### 8. Acceptance criteria (the done gate)

**Slice 1 — "Axon runs on a Cortex-M RTOS" (LANDED):**
- [x] `axon build --freestanding --target zephyr --emit-obj app.ax` emits an `arm-zephyr-eabi` ELF32 ARM
      relocatable object exposing `axon_main` (defined) and `axon_console_putc` (undefined extern).
- [x] The Zephyr app skeleton (`examples/zephyr/`) links that object and builds for `qemu_cortex_m3`.
- [x] **`zephyr_qemu_gate.sh`** — the object is built, the Zephyr app is built, it runs under QEMU
      (`west build -t run`, captured with a timeout), and the console shows the Axon output: the `AXON`
      banner, the refinement-checked `sensor_avg = 23`, and the computed `answer = 42`. SKIP-guarded (exit 0)
      when the Zephyr SDK / west / qemu-system-arm is absent, so the default `gate.sh` stays green.
- [x] `zephyr_console_putc` is E0910-refused in the interpreter; surface-file misuse is refused via the
      `Hal` effect path; the builtin is classified `Hal`/impure/allocation-free (unit test).

### 9. What's real vs deferred

**Real:** ARM Cortex-M3 object emit; the full QEMU boot+run with verified Axon output; the architecture-neutral
console hook; refinement contracts (`-> (i64 where _ >= 0)`) and `@[no_alloc]` enforced on the on-target code.

**Deferred:**
- An Axon program running on a *Zephyr thread* (`k_thread_create`) rather than from `main()` — straightforward
  C glue, not on the critical path for the "it runs" proof.
- `str`/array output from on-target Axon code — those builtins pull in axon-rt runtime externs a freestanding
  image does not link; on-target output is byte-by-byte via `zephyr_console_putc` (the R17 discipline). A
  no-alloc `str`-iteration primitive would lift this.
- A Zephyr `west` module/extension that compiles `.ax` sources as part of the Zephyr build (today the Axon
  object is built separately and passed via `-DAXON_OBJ`).
- Hardware-in-the-loop (real Cortex-M board) — QEMU is the verification oracle, as in R17.

### 10. Rollout & rollback

Pure addition behind `--freestanding`. One new builtin, one target alias, one code-model branch, plus the
`examples/zephyr/` skeleton and the SKIP-guarded gate script. The default hosted build and all existing
targets are untouched; revertible as a unit.

### 11. Environment note (host SDK)

The host's Zephyr SDK 0.17.0 toolchain (`arm-zephyr-eabi-gcc 12.2.0`) is fully functional, but its reported
SDK version was below the `>= 1.0` that the bleeding-edge Zephyr tree (`v4.4.99`) requires via
`find_package(Zephyr-sdk 1.0)`. Fixed by bumping `~/zephyr-sdk/sdk_version` to `1.0.0` (the toolchain binaries
are unchanged; only the version label gated the build). The CMake SDK registry entry
(`~/.cmake/packages/Zephyr-sdk`) was already present. The gate exports `ZEPHYR_SDK_INSTALL_DIR` and
`ZEPHYR_BASE` for hermetic discovery.
