# R37 — Nano/Micro ASI Kernel (Axon-owned capability microkernel for constrained hardware)

**Spec ID:** `R37-nano-micro-asi-kernel` (new requirement row; depends on `R17-freestanding-substrate.md` [primitives, LANDED Slices 0–3], `R19-fixed-width-integers.md` [LANDED], `R25-zephyr-target.md` [ARM object-emit machinery, LANDED Slice 1]; realizes `AXON_ASI_KERNEL_DESIGN.md` §3's effect-native ABI at the small end; sibling of `crates/axon-guest-kernel` [K1–K5, x86_64/KVM], NOT a replacement for it)
**Status:** Draft — strategic proposal, not yet committed (see §12 Q1, which blocks opening the phase)
**Risk class:** Structural (a second kernel artifact enters the TCB; new error-code block; new verification oracle)
**Author / date:** cklaus (via product-research agent), 2026-07-18

```spec-meta
id: R37-nano-micro-asi-kernel
status-claim: Draft
depends-on: R17-freestanding-substrate, R19-fixed-width-integers, R25-zephyr-target
blocks: none
blocked-by: R37 §12 Q1 (named customer/use-case decision — blocks opening the phase; Slice 0 may run as a spike)
supersedes: none
related: R36-full-asi-os, R12b-kernel-goal, R25-zephyr-target, R16-axon-ui, R18-provenance-ledger, R38-embedded-agent-runtime
conflicts-with: none
reserves: E37xx (E3700–E3704; E17xx/E18xx/E1900/E23xx verified taken); reuses exit 7 (budget) + exit 8 (SandboxViolation) semantics
evidence: none (Draft; Slice-0 gate will be r37_nano_boots_and_denies_qemu per §9)
```

(Edge notes: `depends-on` = the shipped primitive sets §4 reuses (R17 substrate primitives, R19
unsigned ints, R25's ARM object-emit path). R25 also appears in `related` because it is the
*complementary* Zephyr on-ramp (§1) — same object, different TCB bet. R36 is the sibling
x86/KVM-class kernel track; its guest kernel may later adopt this spec's `request` ABI (§12 Q2).
The other platform fronts compete for focus, not for correctness.)

> **One-line scope:** a deliberately tiny, Axon-authored, seL4-style capability microkernel for
> MPU-class constrained hardware (Cortex-M, kilobytes-to-low-megabytes of RAM) whose ENTIRE
> privileged ABI is the effect-row algebra the language already speaks — the smallest possible
> trusted computing base that still gives an edge AI agent runtime `@[contained]`-grade capability
> isolation, with no host OS, no hypervisor, and no cloud underneath it.

---

### 1. Motivation — the empty quadrant

Axon's containment thesis currently enforces at three substrates, and none of them reaches
constrained hardware with *runtime* enforcement:

| Track | Substrate | Who owns the kernel | Runtime capability enforcement on-device? |
|---|---|---|---|
| R21/R26 | Linux process / Firecracker microVM | Linux / the Rust guest kernel | yes — but needs a server-class x86_64 host with KVM |
| `axon-guest-kernel` K1–K5 | bare-metal guest under KVM | **Rust** (1.2K LOC), x86_64 only | yes — SYSCALL-MSR gate, but it *translates POSIX* syscall numbers → effect bits, and requires a hypervisor |
| R17 | freestanding x86_64 QEMU | nobody yet — R17 is the *primitive set* (`@[entry]`, `asm`, atomics, `@[repr(C)]`), aimed at an eventual general-purpose OS ("the hard 90%": drivers, GPU, filesystem) | no kernel artifact exists on this track |
| R25 | Zephyr RTOS on Cortex-M3 | **Zephyr** (~large TCB); Axon is a linked object | **no** — refinements/`@[no_alloc]`/effect rows are checked at compile time, then the image is one flat trust domain; nothing at runtime stops a compromised or miscompiled component |

So the quadrant **{Axon-owned kernel} × {constrained hardware} × {runtime enforcement}** is empty.
That quadrant is where the flagship pitch ("AI code sandboxed by the compiler") becomes "AI code
sandboxed *down to the silicon*," and it is where these concrete users live — none served today:

1. **Robot safety-interlock co-processor.** A planner (possibly LLM-refreshed policy code) runs as an
   untrusted principal on an MCU; the actuator-enable line is a capability held only by a verified
   interlock principal. The planner *cannot* actuate directly — not "is checked," *cannot name the
   resource*. R25 can't do this (one flat Zephyr image); R26 can't (no Firecracker on a Cortex-M).
2. **Offline edge-inference agent.** A no-cloud-connectivity box (low-MB RAM) runs an autonomous
   agent with a metered budget and an audit ring buffer that survives the agent misbehaving. The
   enforcement kernel must be small enough to audit and to *fit*.
3. **Firmware-level guardrail for a larger untrusted AI system.** The nano kernel as the last-line
   monitor: the big system's outputs pass through a principal whose caps are exactly the allowed
   actuations, on hardware the big system cannot reach.

ROADMAP §2.3's reversal paragraph already names the thesis — "Principals/effects/`@[contained]` ≈
**seL4** caps" — but seL4's actual identity is a ~10K-LOC formally-verified *microkernel* for
embedded/safety-critical use, which is *this* spec's shape, not R17's general-OS trajectory. R37 is
the track where the seL4 comparison is earned rather than borrowed.

**Why "not just R17 but smaller":** R17 contains no kernel — it is the substrate-language spec
(and its Slice-4 endgame is an x86 general OS). The only existing kernel artifact (guest-kernel) is
Rust, x86/KVM, and POSIX-translating. R37 differs from both on all four load-bearing axes:
**(a) authorship** — written in substrate Axon (the first dogfooded Axon kernel; `@[no_alloc]`/E1704
makes it *compiler-provably* allocation-free, a claim no Rust kernel gets for free);
**(b) ABI** — effect-native from day one (`request(effect, resource)` via SVC; an MCU has no POSIX
to be compatible with, so the AXON_ASI_KERNEL_DESIGN inversion is *feasible here first* and only
retrofit-cost elsewhere); **(c) isolation mechanism** — Cortex-M thread/handler mode + **MPU**
regions (no MMU, no rings, no page tables — a different and much smaller mechanism than either
x86 kernel track); **(d) scheduler** — static, budget-aware, single-core (no SMP, no dynamic
principal creation in v1). Shared with R17: the entire primitive set. Shared code: essentially none.
That combination — same language primitives, disjoint kernel design space — is why this is a
top-level PRD and not an R17 sub-slice.

**Relationship to R25 (complementary, not competing):** R25 remains the on-ramp to the Zephyr
ecosystem (drivers, boards, certification artifacts) for users who accept Zephyr in the TCB. R37 is
for users whose requirement *is* the tiny TCB. The same `--freestanding` ARM object runs under
either; migration is a link-time choice, which is itself a selling point.

### 2. Requirement link

New `REQUIREMENTS.md` row R37 under the platform-vision bucket. Headline acceptance: *an
Axon-authored kernel image boots on an emulated Cortex-M MPU board, runs two statically-declared
principals in MPU-isolated partitions, and DENIES-with-audit an effect request the policy withholds
— with the kernel within the §10 footprint budget.*

### 3. What "nano/micro" means (the sizing contract)

| Tier | Hardware class | Device envelope | Kernel budget |
|---|---|---|---|
| **nano** | ARM Cortex-M3/M4/M33 **with MPU** (verification oracle: QEMU `mps2/an385`) | 64–512 KB flash, 16–512 KB RAM | kernel flash ≤ **32 KB**, kernel static RAM ≤ **8 KB**, zero heap in-kernel |
| **micro** | MMU-less edge boxes (Cortex-M7/R, RISC-V PMP class) | 1–64 MB RAM | kernel flash ≤ **128 KB**, TCB ≤ **5K LOC** substrate Axon + ≤ 300 lines bring-up asm |

(Contrast: the ASI kernel design sketch targets ~15K LOC for the microVM-class kernel; Zephyr's
kernel+subsys TCB is orders larger; seL4 is ~10K LOC C. The nano tier undercuts all three because
it does *only* capability enforcement + static scheduling + audit.)

### 4. Surface / architecture (what exists at build- and run-time)

- **Static partition manifest.** Principals, their caps, budgets, and memory partitions are declared
  at build time (the seL4/CAmkES "static system" model): a `substrate` Axon file the build compiles
  into the image's policy blob. No dynamic principal creation in v1; attenuation is a build-time
  check (child caps ⊆ parent caps — the same rule as `AXON_ASI_KERNEL_DESIGN.md` §2.1, statically).
- **Effect-native ABI.** One kernel entry: `SVC #0` with `(effect, resource, args)` in registers →
  `request(eff, on, args) -> Result<Handle, Denied>`. The effect enum IS the Phase-6 effect row
  (`Hal`-leaves partitioned per principal). No syscall-number translation layer exists to audit.
- **MPU isolation.** Each principal gets MPU regions (code RX, data RW, peripherals per-cap).
  Thread mode = principal, handler mode = kernel. A memory-access violation is a fault routed to the
  kernel: audit + principal stop (the on-silicon analogue of exit 8 / SandboxViolation).
- **Budget metering.** SysTick debits the running principal's CPU budget; exhaustion is a clean
  stop with an audit record (the kernel realization of `Budget`, matching R12b semantics).
- **Audit ring.** A fixed static ring buffer (as in guest-kernel `enforce.rs`) readable over UART —
  survives the workload, is the on-device provenance log.
- **Reused, not rebuilt:** R17 primitives (`@[entry]`, `@[panic_handler]`, `asm`, `@[naked]`,
  atomics, `@[repr(C)]`/`@[packed]`, `@[no_alloc]`, `Hal` effect + E1700–E1706), R19 unsigned ints,
  R25's `--target` alias / ARM reloc-model path. **Gap to close:** `@[interrupt]` currently emits
  x86-interrupt CC; on Cortex-M, hardware exception entry stacks the caller-saved set, so plain
  AAPCS fns can serve as vector handlers — a small codegen slice, not a new mechanism (§12 Q3).

### 5. Type rules

N/A beyond what R17/R19/Phase-6 already define. The partition manifest is ordinary substrate Axon
checked by the ordinary checker; the whole kernel compiles under `@[no_alloc]` (E1704, transitive).

### 6. Error codes

New block **E37xx** (E17xx = R17, E18xx = R13/TEE, E1900 = R19, E23xx = eBPF — all taken).

| Code | Trigger | Message shape |
|---|---|---|
| E3700 | partition manifest declares a child cap ∉ parent caps | `principal 'planner' claims cap Net("*") not held by parent 'root' — attenuation only` |
| E3701 | two partitions' MPU regions overlap | `partitions 'planner' and 'interlock' overlap at 0x2000_1000` |
| E3702 | kernel image exceeds the tier's flash/RAM budget | `nano kernel is 34.2 KB flash (budget 32 KB)` |
| E3703 | a principal's code reaches a `Hal` leaf outside its declared caps (static pre-check of the runtime gate) | `fn 'plan' reaches gpio_set (Hal:Actuate) but 'planner' holds no Actuate cap` |
| E3704 | in-kernel heap allocation (any path not `@[no_alloc]`-clean) | reuses E1704 framing, hard error at image link |

Runtime (not compiler) outcomes reuse the established exit-code vocabulary: denial → audit +
principal stop (SandboxViolation semantics, code 8); budget exhaustion → code 7 semantics.

### 7. Invariants touched

Inherits R17's amended I-3/I-4/I-5/I-6 unchanged (no new carve-outs — the kernel is a substrate
consumer of the existing unsafe surface). **I-11 (capability boundary real and total): strengthened**
— the boundary gains a hardware enforcement point below the interpreter/codegen. **I-12: the kernel
image is a TCB component** under the existing multi-sig rule; self-modification cannot touch it.
A second kernel codebase is a real ongoing TCB-maintenance cost — this is the spec's main price,
weighed in §12 Q1 (mitigation: nano-kernel LOC budget is hard-capped in §3, and the guest-kernel's
enforcement design is the shared reference model).

### 8. Test plan

- [ ] Unit: manifest attenuation (E3700), MPU overlap (E3701), footprint gate (E3702), static
      cap-reach pre-check (E3703); golden-IR for the SVC entry sequence.
- [ ] Integration (the acid test): image boots on QEMU `mps2/an385`; principal A's ungrated
      `request(Net, …)` is DENIED, audited, and A is stopped while principal B keeps running.
- [ ] CLI e2e: `axon build --freestanding --target cortex-m3 --kernel manifest.ax --emit-image` →
      bootable image; QEMU run asserts UART transcript (boot banner, denial line, audit dump).
- [ ] Adversarial: planner writes to interlock's MPU region → fault + stop; planner attempts direct
      peripheral MMIO without the cap → fault; budget-exhaustion loop → clean stop, B unaffected.
- [ ] Property: no principal transcript ever shows an effect outside its manifest caps (sweep).
- [ ] Parity: N/A interp↔codegen for HAL leaves (R17 §6 note); QEMU is the oracle, as R17/R25.
- [ ] Journey/red-team: the robot-interlock demo (§1 use case 1) as `examples/nano-kernel/`.

### 9. Acceptance criteria (per slice)

- **Slice 0 — "it boots and denies":** `r37_nano_boots_and_denies_qemu` — Axon-authored kernel
  (asm bring-up + Axon `kmain`), two static principals, SVC `request` gate, one denial with audit,
  on QEMU mps2/an385. (Prereq inside this slice: Cortex-M exception-handler codegen, §12 Q3.)
- **Slice 1 — MPU partitions:** `r37_mpu_cross_partition_write_faults` — memory isolation real.
- **Slice 2 — budgets + audit ring:** `r37_budget_exhaustion_stops_principal_cleanly`.
- **Slice 3 — the interlock demo:** `r37_interlock_planner_cannot_actuate` — the flagship red-team
  journey; planner principal cannot drive the actuator GPIO without the interlock's countersign.
- **Deferred (explicitly out of v1):** dynamic principal spawn, multi-core, real-board
  hardware-in-the-loop, RISC-V PMP port (micro tier hardware beyond QEMU), formal verification of
  the kernel (the seL4-grade endgame — priced only after Slice 3 proves demand).

### 10. Performance budget

§3's footprint table is the budget and is gate-enforced (E3702). SVC round-trip ≤ 200 cycles on
Cortex-M3 (IR/instruction-count golden as proxy under QEMU, which is not cycle-accurate).

### 11. Rollout & rollback

Pure addition: a new `crates/axon-nano-kernel/` (manifest tooling + bring-up asm) + substrate `.ax`
kernel sources + a SKIP-guarded QEMU gate script (pattern proven by `zephyr_qemu_gate.sh`). No
hosted-build impact; each slice independently revertible; blast radius = the new crate + one gate.

### 12. Open questions

1. **(strategic — BLOCKS opening the phase)** Same discipline as R17 §12 Q1: is there a named
   customer/use-case (robotics interlock, edge-agent OEM) before Slice 1? Default: land Slice 0 as
   a spike off the R17 primitives (cheap — the primitives all exist), defer Slices 1–3 pending a
   real pull. A second kernel TCB is only worth carrying if someone needs the empty quadrant.
2. Should the guest-kernel's K3 gate eventually be *replaced* by a port of this effect-native ABI
   (unifying the two kernels' enforcement cores), or stay POSIX-translating for Linux-binary compat?
   Default: converge on the R37 `request` ABI as the shared core once Slice 0 validates it.
3. Cortex-M exception-handler codegen: confirm plain-AAPCS handlers suffice (hardware stacking) or
   whether a thumb variant of `@[interrupt]` is needed for the SVC/fault/SysTick vectors.
4. Manifest surface: a dedicated `partition { … }` block vs plain struct data compiled by a build
   step. Default: plain data first (no parser change); sugar only if the demo reads badly.
5. Micro-tier RISC-V PMP: same kernel with an arch port, or nano-only until pulled? Default: defer.
