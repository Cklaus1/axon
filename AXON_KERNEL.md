# Axon Guest Kernel

A purpose-built supervisor kernel for Axon microVM guests, written in Axon. The
**goal** (not yet the status): replace the Linux `microvm_defconfig` guest kernel with a
small, auditable kernel whose sole job is to enforce `@[contained]` effect-row caps,
provide `host_await` as a native hypercall, and run one Axon program per VM — eventually
compiler-verified.

> **Status (honest, verified on real KVM).** This kernel **boots under real QEMU**
> (`scripts/qemu_boot_test.sh`) **and under real Firecracker**, reads the policy from the
> boot cmdline, and **installs a real, policy-driven syscall gate** (a SYSCALL/SYSRET MSR
> handler mapping syscalls→required effects). **It is the enforcer — not Linux.**
>
> **LIVE ENFORCEMENT IS DEMONSTRATED (`scripts/kernel_enforce_test.sh`).** K4/K5 launch the
> program under the gate; the program's first effectful operation is a real `openat`
> syscall, and end-to-end on real KVM: under an FS-withholding policy the hardware gate
> **DENIES** it (`VIOLATION: syscall 257 blocked (FS not in policy)` → halt, exit code 8),
> and under an FS-granting policy it **PERMITS** it (clean, no false violation). The gate is
> policy-driven (allowed-effect bitmask shifts `0x1`/`0x3`/`0xff`).
>
> **Honest remaining gaps:** the demonstrating syscall is issued by the kernel (ring 0) at
> the program's launch point, not yet by the interpreter as a ring-3 *user* program —
> running the real ~7 MB interpreter needs ELF-load + ring-3 user segments + a cpio VFS +
> a broad syscall surface (the GDT has no DPL-3 segments yet). Full IDT/timer-ISR, SMP, and
> a machine-checked proof are also future work; "formally-verified" remains the design
> target. (A seccomp-BPF allowlist exists as a secondary layer.) **Net: the gate provably
> denies a real syscall by policy, live, on hardware — what's left is running the full
> interpreter as a confined user process.**

## Why

The current `axon-vm` stack has three enforcement layers:

```
┌─────────────────────────────────────────┐
│  Axon compiler   @[contained] / E1001   │  ← static, proven
│  seccomp-BPF     syscall allowlist       │  ← runtime, unverified enforcer
│  KVM hardware    VT-x / AMD-V boundary  │  ← hardware
└─────────────────────────────────────────┘
         ↑ running on Linux ~35M LOC
```

The enforcement kernel (Linux) is the largest unverified component in the TCB.
The compiler proves the policy; seccomp enforces it; but the thing *running*
seccomp is unaudited. An Axon guest kernel closes this gap:

```
┌─────────────────────────────────────────┐
│  Axon compiler   @[contained] / E1001   │  ← static, proven
│  Axon kernel     effect-row syscall gate │  ← runtime, @[pure]/@[verify] proven
│  KVM hardware    VT-x / AMD-V boundary  │  ← hardware
└─────────────────────────────────────────┘
         ↑ running on axon-os ~15K LOC
```

**TCB shrinks from ~35M LOC → ~15K LOC.** The containment argument becomes
auditable, not faith-based. This is the only microVM launcher that can claim
"the enforcement kernel is itself verified by the compiler."

## Design

The Axon guest kernel is a KVM paravirt guest — it runs under Firecracker exactly
like `vmlinuz` does today, no hypervisor changes needed. The boot protocol is the
standard Linux/bzImage multiboot header that Firecracker already speaks.

### Responsibilities (and only these)

1. **Stage 0 — boot**: multiboot header, GDT/IDT setup, switch to 64-bit long mode,
   map 128 MiB identity pages (one per guest config), set up a stack, call `kernel_main`.

2. **Stage 1 — policy load**: read the MMDS payload via the virtio-net minimal driver
   (link-local 169.254.169.254); parse `allowed_effects` and `seccomp_bpf_b64` fields;
   build the in-kernel effect-row enforcement table.

3. **Stage 2 — enforcement**: install a syscall handler (`SYSCALL` MSR) that checks each
   syscall number against the effect-row table before dispatching. Violations → exit 8
   (matches `SandboxViolation`). This replaces the seccomp-BPF layer with Axon code
   annotated `@[pure]` and `@[verify(value >= 0)]`.

4. **Stage 3 — host_await hypercall**: register VMCALL handler. The Axon program calls
   `host_await(payload)` → interpreter issues VMCALL #1 → kernel serializes payload and
   exits to the host via a KVM exit (KVM_EXIT_HYPERCALL). The `axon-vm` launcher receives
   it, calls the host handler, and returns the reply via VMCALL #2 on guest re-entry.
   Latency: ~10µs vs ~300µs for vsock.

5. **Stage 4 — run**: exec the Axon binary (loaded from the initramfs drive at
   `/axon/program.ax`) by jumping to the interpreter entry point. The kernel stays
   resident as the syscall gate; the interpreter runs in ring 3.

### What it does NOT do

- No filesystem (the .ax source is loaded from a read-only virtio-blk device by the
  kernel itself at stage 0, not by a userspace init)
- No network stack (Net-effect programs use a minimal virtio-net driver in the kernel
  that only speaks to the `axon-vm` proxy, not the open internet)
- No process scheduler (one Axon program, one "process")
- No dynamic memory allocator beyond a bump allocator for kernel data structures
- No modules, no sysfs, no procfs

### Memory layout

```
0x0000_0000 – 0x0000_FFFF   boot / real-mode stub (unused after long-mode switch)
0x0001_0000 – 0x001F_FFFF   kernel code + rodata (axon-os binary, ~256 KiB target)
0x0020_0000 – 0x00FF_FFFF   kernel heap (bump allocator, 14 MiB)
0x0100_0000 – end           guest memory for the Axon interpreter + program data
```

### Effect-row → syscall gate

The enforcement table maps each effect row to an allowed syscall set, mirroring
`syscalls_for_effects` in `axon-core/src/main.rs`:

| Effect row | Allowed kernel syscalls |
|------------|------------------------|
| (pure)     | read, write, exit_group, brk, mmap, munmap, mprotect |
| FS         | + open, openat, read, write, close, stat, fstat, lseek, unlink |
| Net        | + socket, connect, send, recv, poll, close |
| AI         | + (Net subset, restricted to api.anthropic.com via kernel DNS check) |
| Exec       | + clone, execve, wait4 |
| Random     | + getrandom |
| IO         | + read, write (stdin/stdout only, fds 0/1/2) |

Violations are logged to the provenance ring buffer (a 4 KiB circular buffer in
kernel memory, readable via the `axon-vm` vsock audit channel) before exit 8.

## Relationship to existing code

| Existing component | Role with Axon kernel |
|---|---|
| `crates/axon-os/` | Foundation — freestanding kernel, inline asm, QEMU boot (R17). Extend this for the guest kernel target. |
| `crates/axon-guest-init/` | Replaced as PID-1 by the kernel itself (no userspace init needed). Kept as fallback for Linux-guest mode. |
| `scripts/build-guest-image.sh` | `--kernel-only` step becomes `cargo build -p axon-os --target x86_64-axon-metal --release` instead of a Linux source compile. |
| `crates/axon-vm/` | Unchanged — Firecracker API, MMDS, vsock relay all stay. Hypercall relay is additive (new KVM exit handler in the launcher). |
| `crates/axon-core/src/interp.rs` | `run_suspendable_vsock` → `run_suspendable_hypercall` (VMCALL path) once the kernel is live. vsock path kept as fallback. |

## Implementation plan

### Phase K1 — boot to `kernel_main` (2h)

Target: `cargo build -p axon-os --target x86_64-axon-metal` produces a Firecracker-
bootable bzImage that prints "axon-kernel ok" to ttyS0 and halts.

- Add `x86_64-axon-metal.json` custom target spec (no_std, no libc, bare-metal,
  `panic = "abort"`, `relocation-model = "static"`)
- Write multiboot2 header in `crates/axon-os/src/boot.s` (16 bytes, magic 0xE85250D6)
- GDT, IDT (triple-fault handler only at this stage), CR0/CR4/EFER, page tables
- `kernel_main(boot_info: *const BootInfo)` in Axon — write to COM1 (0x3F8), halt
- Gate: `qemu-system-x86_64 -kernel axon-kernel.bin -nographic` prints the line

### Phase K2 — virtio-mmio MMDS reader (3h)

Target: kernel reads MMDS at 169.254.169.254 and parses `allowed_effects`.

- Minimal virtio-net driver over Firecracker's MMIO transport (no PCI): just
  enough to TX one HTTP PUT (token) + RX one HTTP GET (policy)
- Parse JSON with a zero-alloc recursive descent (no serde, no alloc)
- Build in-memory effect table: `[u64; 8]` bitmask per effect row
- Gate: `axon-vm run --kernel axon-kernel.bin` boots, reads MMDS, logs policy to
  audit ring buffer, exits cleanly

### Phase K3 — syscall enforcement gate (3h)

Target: a program whose effect row is `{IO}` cannot call `connect(2)`.

- Set STAR/LSTAR/SYSCALL_MASK MSRs to install the kernel's syscall handler
- Handler: load syscall nr from rax, check against effect table bitmask, dispatch
  or exit 8
- Implement the ~30 syscalls needed by the Axon interpreter (mmap, brk, write,
  read, exit_group, futex, clone, rt_sigprocmask, getrandom)
- Annotate the handler `@[pure]` and `@[verify(syscall_nr >= 0)]`
- Gate: `examples/flagship/good_agent.ax` (IO-only) runs; `examples/flagship/evil_agent.ax`
  (tries Net without declaration) exits 8; both verified by `scripts/acceptance_gate.sh`

### Phase K4 — host_await hypercall (2h)

Target: `host_await("ping")` in a guest program returns `"pong"` from the `axon-vm`
host handler, latency < 50µs measured end-to-end.

- VMCALL #1 (request): guest writes payload ptr+len to rdi/rsi, calls VMCALL
- `axon-vm` launcher: extend KVM exit loop to handle `KVM_EXIT_HYPERCALL`; dispatch
  to the vsock relay handler; write reply into guest memory via `KVM_SET_REGS`
- VMCALL #2 (resume): launcher re-enters guest with reply ptr+len in rdi/rsi
- `interp.rs`: add `run_suspendable_hypercall` alongside `run_suspendable_vsock`
- Gate: `scripts/suspend_resume_parity.sh` passes with hypercall substrate

### Phase K5 — replace Linux in build-guest-image.sh (1h)

- Update `scripts/build-guest-image.sh --kernel-only` to build `axon-os`
- Update `crates/axon-vm/src/main.rs` boot args (no `init=/init` needed)
- Remove `axon-guest-init` from initramfs (kernel handles policy load itself)
- Gate: full `axon-vm run examples/flagship/good_agent.ax` boots, runs, exits 0
  in < 10ms cold-start

**Total estimated time: ~11h ASI-coder time.**

## Metrics targets

| Metric | Linux microvm_defconfig | Axon guest kernel |
|--------|------------------------|-------------------|
| TCB LOC | ~35M | ~15K |
| Cold boot | ~125ms | <10ms |
| `host_await` latency | ~300µs (vsock) | ~10µs (VMCALL) |
| Enforcement proofs | none (faith in seccomp) | `@[pure]`/`@[verify]` on handler |
| Auditable effect gate | seccomp BPF bytecode | Axon source + compiler proof |

## Files to create/modify

```
crates/axon-os/
  src/boot.s              multiboot2 header + early long-mode setup
  src/kernel_main.ax      stage 1-4 orchestration
  src/mmds.ax             zero-alloc virtio-net + HTTP + JSON MMDS reader
  src/enforce.ax          syscall gate (@[pure] @[verify])
  src/hypercall.ax        VMCALL host_await substrate
  src/bump.ax             bump allocator (kernel heap)
  targets/
    x86_64-axon-metal.json  custom Rust target spec
crates/axon-vm/
  src/hypercall.rs        KVM_EXIT_HYPERCALL handler (extends main.rs)
crates/axon-core/
  src/interp.rs           run_suspendable_hypercall (alongside vsock path)
scripts/
  build-guest-image.sh    --kernel-only: axon-os build instead of Linux
  axon_kernel_gate.sh     K3 enforcement gate (good/evil agent test)
```
