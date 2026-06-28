# Axon Deployment Targets

How to run Axon programs, and which target fits your situation.

---

## Target matrix

| Target | Substrate | Build command | Startup | Peak perf | `@[contained]` enforcement |
|---|---|---|---|---|---|
| **Interpreter** | Tree-walking eval | `axon run foo.ax` | ~50–200 ms | ~10–50× below native | Compiler static check |
| **Native binary** | Host OS process | `axon build foo.ax` | ~0 ms | Full CPU (≈ C) | Compiler static check |
| **WASM browser** | Chrome/Firefox JS engine | `wasm-pack` / `axon-wasm` cdylib | ~200 ms+ | ~2–5× below native | Compiler check + browser sandbox |
| **WASM/WASI** | wasmtime / Wasmer | `cargo build --target wasm32-wasip1` | ~50 ms | ~1.5–2× below native | Compiler check + WASI capability grants |
| **MicroVM (cold)** | Firecracker + Linux guest | Native binary wrapped in VM | ~125–330 ms cold | ~5–15% below native | Compiler check + hardware VM boundary |
| **MicroVM (snapshot)** | Firecracker snapshot restore | `axon-vm launch --fast` | **~3–8 ms** | ~5–15% below native | Compiler check + hardware VM boundary |
| **Unikernel (Nanos)** | Nanos kernel, unmodified ELF | `ops run ./binary` | ~12 ms TTR | ~5–10% below native | Compiler check + kernel boundary |
| **Embedded / RTOS** | Zephyr (Cortex-M), bare-metal | `axon build --target thumbv7m` | instant | hardware-dependent | `@[no_alloc]` + MPU |
| **eBPF** | Linux kernel verifier | R23 target | N/A | kernel datapath speed | Kernel verifier (formal) |
| **TEE** | Gramine / SGX enclave | `@[enclave]` + E1810 | ~1–3 s cold | Near-native inside enclave | Hardware attestation + compiler |

> **Execution is interpreter-first.** `axon run`, `axon test`, `axon goal`, and `axon check`
> all work without the `codegen` feature. `axon build` requires `--features codegen`
> (on by default in release builds — ~3 s build time).

---

## Choosing a target

### For development and iteration

Use the **interpreter** (`axon run foo.ax`). No LLVM required, sub-second build, all ASI
builtins and `@[contained]` checks work identically.

### For shipping a fast CLI or server

Use **native** (`axon build foo.ax`). LLVM-compiled Axon is competitive with C for compute
workloads. The `@[contained]` checks are compile-time only — zero runtime overhead.

### For untrusted or AI-generated code

Use **MicroVM** (native binary inside Firecracker) or **WASM/WASI**. Both give two
independent enforcement layers (compiler + OS/hardware boundary). See the security section
below.

### For browser apps and interactive approval flows

Use **WASM browser** (`axon-wasm` cdylib, `crates/axon-wasm`). The `host_await` builtin
suspends Axon via `wasm-opt --asyncify`, awaits a JS Promise, and resumes — this is how
the Phase-12 web UI approval flow works. No FS or Net access by default.

### For kernel/embedded targets

Use the **bare-metal / RTOS** path (R17). Axon can write kernel code via `@[no_alloc]`,
inline `asm!`, and the Zephyr RTOS target. This does NOT modify your host OS kernel —
it produces a freestanding binary that runs on target hardware or QEMU.

---

## Does Axon modify the host OS kernel?

No. Axon is a language that compiles to native binaries or WASM. It runs on top of your
existing OS like any compiled program. No kernel modules, no drivers, no patches.

The R17 "kernel" work lets you *write* a kernel *in* Axon (for embedded targets, QEMU
boot, Zephyr RTOS). It has no effect on the machine you compile on.

---

## `@[contained]` performance overhead

`@[contained]` is a **static compiler guarantee**, not a runtime sandbox:

- **Compile time:** O(AST size) capability graph walk. Unmeasurable for programs up to
  several thousand lines.
- **Runtime native:** zero. Allowed calls go through with no added indirection. The
  compiler refused everything else at build time.
- **Runtime interpreter:** one capability-set lookup per I/O builtin call. I/O itself is
  orders of magnitude slower, so this is unmeasurable in practice.

The cost of `@[contained]` is entirely at compile time, not at runtime.

**Security caveat:** because enforcement is compiler-side, a bug in the capability walker
is a security bypass with no runtime trip-wire. For adversarial workloads, pair
`@[contained]` with MicroVM or WASI — two independent enforcement layers.

---

## Security model per target

| Target | Boundary | What a compiler bug means |
|---|---|---|
| Native on host OS | OS process only | Full host access if `@[contained]` bypass found |
| MicroVM | VM hardware boundary + compiler | Guest still can't cross VM boundary |
| WASM browser | Browser sandbox + compiler | No syscalls, no FS regardless of compiler bug |
| WASM/WASI | WASI capability grants + compiler | Only explicitly granted paths/hosts accessible |
| TEE (SGX) | Hardware enclave + compiler + attestation | Code integrity guaranteed by hardware |
| eBPF | Kernel verifier (formal) + compiler | Verifier independently rejects unsafe programs |

The interpreter and native-on-host targets rely entirely on the compiler's checker. All
other targets add an independent hardware or OS boundary.

---

## Performance numbers in more detail

These are approximate ratios for typical compute workloads. I/O-bound programs see
smaller differences; SIMD-heavy programs may see larger ones.

```
Native binary               1×          (baseline)
MicroVM (snapshot restore)  1.05–1.15×  VM entry/exit overhead; ~3–8 ms cold start from pool
MicroVM (cold boot)         1.05–1.15×  same perf ceiling; 125–330 ms first-boot penalty
Unikernel (Nanos)           1.05–1.10×  ~12 ms TTR; unmodified musl ELF, no porting needed
WASM/WASI (wasmtime)        1.5–2×      JIT + indirect call overhead
WASM browser                2–5×        JIT + JS bridge + Asyncify unwind/rewind on host_await
Interpreter                 10–50×      tree-walking eval; no JIT
```

`host_await` (suspend/resume) adds ~0.5–2 ms per suspension in the native worker-thread
substrate, ~300 µs over vsock (under `axon-vm`), and ~1–5 ms in the browser (Asyncify
unwind+rewind). For most interactive workflows this is imperceptible.

> **KVM tuning note:** Linux 6.1 introduced a `KVM_CREATE_VM` latency regression.
> If cold-boot times regress on a 6.1+ host, add `kvm.nx_huge_pages=never` to the
> kernel command line. See `AXON_VM.md` for additional boot tuning knobs.

---

## Quick reference

```bash
# Fastest iteration
axon run examples/hello.ax

# Fastest execution
axon build examples/hello.ax && ./hello

# Untrusted AI-generated code (MicroVM example with Firecracker)
axon build goal.ax -o goal
firectl --kernel vmlinux --root-drive rootfs.img --ncpus 1 --mem-size 128 -- /goal

# WASM browser (approval flow UI)
cd crates/axon-wasm && wasm-pack build --target web
# then open examples/browser/interactive.html

# WASM/WASI
cargo build -p axon-core --target wasm32-wasip1
wasmtime target/wasm32-wasip1/debug/axon.wasm -- run foo.ax

# Embedded / RTOS (QEMU Cortex-M, requires Zephyr SDK)
bash scripts/zephyr_qemu_gate.sh
```

See `ENVIRONMENTS.md` for how to install and verify each platform environment.
