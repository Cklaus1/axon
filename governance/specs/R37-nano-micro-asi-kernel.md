# R37 — Nano/Micro ASI Kernel (Axon-owned capability microkernel for constrained hardware)

**Spec ID:** `R37-nano-micro-asi-kernel` (new requirement row; depends on `R17-freestanding-substrate.md` [primitives, LANDED Slices 0–3], `R19-fixed-width-integers.md` [LANDED], `R25-zephyr-target.md` [ARM object-emit machinery, LANDED Slice 1], `R20-smt-capability-proofs.md` [SMT-proven attenuation + content-addressed TCB digest, LANDED Slices 0–3 — §4's manifest attenuation reuses it rather than hand-rolling]; realizes `AXON_ASI_KERNEL_DESIGN.md` §3's effect-native ABI at the small end; sibling of `crates/axon-guest-kernel` [K1–K5, x86_64/KVM], NOT a replacement for it)
**Status:** Draft — strategic proposal, not yet committed (see §12 Q1, which blocks opening the phase)
**Risk class:** Structural (a second kernel artifact enters the TCB; new error-code block; new verification oracle)
**Author / date:** cklaus (via product-research agent), 2026-07-18

```spec-meta
id: R37-nano-micro-asi-kernel
status-claim: Draft
depends-on: R17-freestanding-substrate, R19-fixed-width-integers, R25-zephyr-target, R20-smt-capability-proofs
blocks: none
blocked-by: R37 §12 Q1 (named customer/use-case decision — blocks opening the phase; Slice 0 may run as a spike)
supersedes: none
related: R36-full-asi-os, R12b-kernel-goal, R25-zephyr-target, R16-axon-ui, R18-provenance-ledger, R38-embedded-agent-runtime, R28-capability-audit-ledger, R34-incremental-attestation
conflicts-with: none
reserves: E37xx (E3700–E3707; E17xx/E18xx/E1900/E23xx verified taken; E37xx verified free in `error.rs` 2026-07-31); reuses exit 7 (budget) + exit 8 (SandboxViolation) semantics
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
   *(2026-07-31: this claim is absolute, so §4 now carries the invariant that makes it dischargeable
   — **I-R37-1 total mediation** — plus the SVC duty list and cap arg predicates. Without those, the
   sentence promised more than the two enforcement points delivered, and §8's oracle could not have
   exhibited the difference.)*
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
either; migration is a link-time choice, which is itself a selling point. **[Corrected 2026-07-31:
that sentence is currently FALSE for any arithmetic-bearing kernel body.** The R17 §12 Q9
freestanding safety trap is synthesized as x86-only inline asm — `outb $1, $0` with a `{dx}`
register constraint plus `hlt` (`codegen/expr.rs::synthesize_freestanding_trap`), synthesized for
every `--freestanding` build and retained whenever any checked-arithmetic/bounds/refinement site
references it (dead-function pruning, `codegen/output.rs::prune_dead_functions`, removes it
otherwise — i.e. any realistic kernel body keeps it) — so `axon build --freestanding --target
cortex-m3 --emit-obj` on a minimal `@[entry]` kernel whose body is `a + 1` fails with
`error: couldn't allocate input reg for constraint '{dx}'` (reproduced 2026-07-31); the same file
without arithmetic emits a valid ARM EABI5 object. Since checked i64 arithmetic (I-9) guards every
`+`/`-`/`*`//`, any realistic Cortex-M kernel body hits this. This is also a live regression of
R25's ARM path for arithmetic-bearing code (R25's `__weak`-stub design,
`R25-zephyr-target.md` line ~80, predates the Q9 in-module-trap change, and its QEMU gate is
SKIP-guarded so it has not caught it) — recorded as a dated regression note in
`R25-zephyr-target.md` §8 (annotating the regressed Slice-1 checkbox). The arch-conditional trap is
now an explicit Slice-0 prerequisite in §4's gap list.]

### 2. Requirement link

New `REQUIREMENTS.md` row R37 under the platform-vision bucket. Headline acceptance: *an
Axon-authored kernel image boots on an emulated Cortex-M MPU board, runs two statically-declared
principals in MPU-isolated partitions, and DENIES-with-audit an effect request the policy withholds
— with the kernel within the §10 footprint budget.*

### 3. What "nano/micro" means (the sizing contract)

| Tier | Hardware class | Device envelope | Kernel budget |
|---|---|---|---|
| **nano** | ARM Cortex-M3/M4/M33 **with MPU** (verification oracle: QEMU `mps2-an385`) | 64–512 KB flash, 16–512 KB RAM | kernel flash ≤ **32 KB**, kernel static RAM ≤ **8 KB**, zero heap in-kernel |
| **micro** | MMU-less edge boxes (Cortex-M7/R, RISC-V PMP class) | 1–64 MB RAM | kernel flash ≤ **128 KB**, TCB ≤ **5K LOC** substrate Axon + ≤ 300 lines bring-up asm |

(Contrast: the ASI kernel design sketch targets ~15K LOC for the microVM-class kernel; Zephyr's
kernel+subsys TCB is orders larger; seL4 is ~10K LOC C. The nano tier undercuts all three because
it does *only* capability enforcement + static scheduling + audit.)

**Auditability budget — LOC is a proxy, complexity is the metric.** *(Added 2026-07-31.)* The ≤5K
LOC cap above is an *authorship-era* proxy: it assumes line count tracks the effort a human
reviewer spends, which does not hold for machine-generated substrate Axon (ROADMAP §2.1's own
thesis is that `.ax` is an IR optimized for machine generation). E3702 as originally stated gates
the compiled *image*, so it constrains nothing about source complexity. Therefore the LOC cap is
**supplemented, not replaced**, by a gate-enforced `axon complexity` ceiling over the kernel source
(the shipped MDL description-length metric, `--json → axon-complexity/1`):

| Budget | Value | Gate |
|---|---|---|
| total kernel MDL complexity | ceiling fixed at Slice 0 from the first green build, +15% headroom | E3702 (extended) |
| per-fn MDL complexity | no single kernel fn may exceed 1/8 of the total ceiling | E3702 (extended) |

The per-fn ceiling is the load-bearing half: review risk lives in the one 400-line SVC handler, not
in the aggregate. Both numbers are recorded in the spec's evidence ledger when Slice 0 lands, so the
ceiling is a ratchet (may tighten, never loosen without an explicit spec amendment).

### 4. Surface / architecture (what exists at build- and run-time)

> **Threat model (normative, added 2026-07-31).** The principal is **not** assumed to be a careless
> or buggy program that issues a request it shouldn't. It is assumed to be a program written by
> something that has read this spec, the partition manifest, and the SVC handler source, and is
> searching for the *cheapest* path to the guarded resource — preferentially the paths that are
> unmediated, un-tested, or un-scored by the gates below. Every mechanism in this section is stated
> as a *duty* the kernel owes, not as a behaviour the principal is expected to respect. Where a
> mechanism cannot discharge its duty on the v1 oracle, that is recorded as an oracle limitation in
> §8 and an open question in §12 — never left to a green gate to imply.

- **I-R37-1 — Total mediation (normative).** *(Added 2026-07-31.)* **Every hardware path to a
  capability-bearing resource is mediated by the SVC gate or the MPU, or the resource is not
  grantable in v1.** The two enforcement points are not self-evidently on every path; the invariant
  is discharged by three explicit obligations:
  1. **Other bus masters.** The ARMv7-M MPU filters *only the core's own* accesses. Any peripheral
     with its own master port (DMA controllers first) can write a principal's data region, the
     interlock's region, or an actuator register without ever issuing an SVC or taking a MemManage
     fault. Therefore: the build enumerates the target SoC's bus masters, and **any DMA-capable
     peripheral is either ungrantable in v1, or grantable only behind a kernel-owned
     descriptor-programming `request`** — the kernel writes the DMA descriptor, the principal never
     does. A manifest granting a bus-mastering peripheral directly is an E3705 build failure.
  2. **MPU region budget is a first-class resource.** Cortex-M3 (the §8 oracle, QEMU `mps2-an385`)
     implements **8** MPU regions, power-of-2 sized and aligned. Two principals × {code, data,
     stack} is already 6; "peripherals per-cap" for both does not fit. An implementation that
     silently *groups* peripherals into a shared region converts "holds a cap to X" into "can reach
     X..Z" with nothing detecting it. Therefore region count is budgeted and gate-enforced: a
     manifest whose enforced partition would be coarser than its declared partition is an **E3705**
     build failure, never a silent coarsening. §8 carries a unit test with a deliberately
     over-subscribing manifest.
  3. **The vector table contains only kernel-owned symbols.** Handler mode is privileged and
     MPU-unrestricted; MPU_RBAR/MPU_RASR live in the freely-writable System Control Space. A
     principal fn reachable from a vector entry therefore runs with authority to rewrite the
     partition. Since `fn_addr`'s argument is already required to be a compile-time string literal
     (E1707, verified in `builtins.rs`), this is statically checkable at manifest-compile time:
     a vector entry naming a non-kernel symbol is an **E3706** build failure.
- **Static partition manifest.** Principals, their caps, budgets, and memory partitions are declared
  at build time (the seL4/CAmkES "static system" model): a `substrate` Axon file the build compiles
  into the image's policy blob. No dynamic principal creation in v1.
  **Attenuation is bounded above as well as below** *(corrected 2026-07-31 — the original rule was
  relative-only)*: the obligation is `root ⊆ hardware ∧ ∀child. child ⊆ parent`, not just the second
  conjunct. A relative-only rule is vacuously satisfiable by a manifest that declares root holding
  every effect over every resource — and root's caps are declared in the same file the check reads,
  so widening the policy is the cheapest attack in the system: it touches neither the kernel, the
  SVC gate, nor the MPU, and every downstream check still passes green. The **hardware ceiling** is
  derived from the board/peripheral description (not from the manifest under review), so every
  principal is bounded by something outside the file. Violations: child ⊄ parent → E3700; root ⊄
  hardware → **E3707**.
  **Discharge reuses R20, not a fresh check** *(corrected 2026-07-31)*: R20 shipped SMT-proven
  capability attenuation over `PrincipalRegistry::mint` with a content-addressed
  `MINT_OBLIGATION_SPEC ⊕ verdict` digest pinned in `TCB_MANIFEST_DIGEST` and boot-checked (E1611).
  R37's manifest attenuation is the same proposition over a static table; it is encoded for the
  existing SMT backend with the hardware ceiling as the proof's top element, and the obligation-set
  digest + verdict is pinned into the kernel image and checked at boot in the same shape. E3700/E3707
  are the *report* of that discharge, not a bespoke reimplementation.
- **Effect-native ABI.** One kernel entry: `SVC #0` with `(effect, resource, args)` in registers →
  `request(eff, on, args) -> Result<Handle, Denied>`. The effect enum IS the Phase-6 effect row
  (`Hal`-leaves partitioned per principal). No syscall-number translation layer exists to audit.
  **Caps are `(effect, resource, predicate over args)` — not `(effect, resource)`** *(corrected
  2026-07-31)*. Resource-granular caps make "holds Actuate" mean "may command any value," and for
  §1's own lead use case — a robot safety interlock — *the value is the safety property*. The
  interlock does not exist to stop the planner touching the actuator at all (a planner that can
  never actuate is not a planner); it exists to bound what it commands. A capable generator does not
  escape the cap, it operates inside it at a value the manifest never constrained (full torque, max
  rate, zero dwell) — i.e. as the generator improves, residual risk migrates from "unauthorized
  effect" to "authorized effect, wrong argument, right at the edge of what passes." The predicate
  language is the **already-shipped refinement-predicate subset** (arith, comparisons, `&&`/`||`/
  `!`), whose constant fragment the checker folds and the SMT encoder discharges today; the SVC gate
  evaluates the non-constant residue in a handful of instructions on a path the kernel already owns.
  Predicate failure denies through the existing audit-and-stop path (exit-8 semantics) — no new
  runtime mechanism.
- **Countersign (ABI shape, defined 2026-07-31).** §9's flagship criterion leans on an interlock
  "countersign"; it is defined here rather than left to the test. A cap row may be marked
  `countersigned_by: <principal>`. A `request` against such a row does not grant; it returns a
  pending token, and the grant completes only when the named principal issues
  `countersign(token, args_digest)` from its own partition within the same scheduling frame.
  The countersign is **value-aware**: it commits to a digest of the exact `args` presented, so a
  countersign cannot be replayed against a different actuation value. An uncountersigned or
  digest-mismatched completion is a denial (audit + stop). Error code for a manifest naming a
  countersigner that is not a declared principal: E3700 family (attenuation/manifest well-formedness).
- **SVC handler duty list (normative, added 2026-07-31).** Cortex-M exception entry stacks r0–r3 on
  the **process** stack, so the handler reads its arguments from *caller-controlled memory* via PSP
  (§4 gap item 2). The kernel runs privileged and MPU-unrestricted, so an unvalidated PSP
  dereference turns the kernel into the principal's read/write primitive with no fault raised — the
  classic confused deputy. The handler MUST, before acting on any argument:
  1. verify the PSP-derived exception frame lies **wholly within the calling principal's own
     data/stack MPU region**;
  2. bounds-check the effect and resource indices against the manifest **before** using them to
     index anything;
  3. validate every pointer-shaped argument against the caller's partition;
  4. treat handles as **unforgeable**: a handle is an index into a kernel-owned per-principal table,
     never a caller-supplied address;
  5. evaluate the cap's arg predicate (above) before granting.
  Each duty has a §8 test that an implementation skipping it must fail. **§10's ≤120-instruction
  budget is subordinate to this list**: if they conflict, the budget moves, not the checks.
- **MPU isolation.** Each principal gets MPU regions (code RX, data RW, peripherals per-cap, within
  the I-R37-1(2) region budget). Thread mode = principal, handler mode = kernel. A memory-access
  violation is a fault routed to the kernel: audit + principal stop (the on-silicon analogue of
  exit 8 / SandboxViolation).
- **Budget metering.** SysTick debits the running principal's CPU budget; exhaustion is a clean
  stop with an audit record (the kernel realization of `Budget`, matching R12b semantics).
- **Audit ring — lossy but honest, and tamper-evident.** *(Corrected 2026-07-31 — the original text
  specified a plain wrapping buffer "as in guest-kernel `enforce.rs`", verified in-tree as a
  4096-byte `AUDIT_BUF` with `AUDIT_HEAD.wrapping_add(1)` and no sequence number, no overflow flag,
  no drop counter, and no integrity chain.)* §3's nano tier caps kernel static RAM at 8 KB total, so
  the real ring is at or below 4 KB — on the order of a hundred records for the device's lifetime.
  That is a rate assumption from a world where events are rare; a principal issuing requests at MCU
  speed produces them faster than they can be read, and the eviction attack is free and needs no
  privilege: issue a few hundred **permitted** requests to wrap the ring, so the record of the one
  denial (or the one anomalous permitted actuation) is overwritten, then stop. Nothing distinguishes
  "clean run" from "evidence evicted" — which would silently satisfy §8's property test and §9's
  Slice-0 gate. The ring therefore carries, within the same footprint:
  1. a **monotonic per-principal sequence number** on every record;
  2. a **global dropped-record counter**, so a reader can always state exactly how many records were
     evicted;
  3. a **rolling hash over the whole record stream** (an R34-shaped chain — one 8–16 byte running
     digest in kernel RAM), emitted with each dump, so the exported transcript is tamper-*evident*
     even when it is incomplete.
  This is the on-device analogue of the shipped chained ledgers (R28 landed; R34's core rolling-hash
  chain landed, S1–S3). It needs no heap and no new mechanism, and it converts the ring from
  "possibly complete" to "provably characterized."
- **Reused, not rebuilt:** R17 primitives (`@[entry]`, `@[panic_handler]`, `asm`, `@[naked]`,
  atomics, `@[repr(C)]`/`@[packed]`, `@[no_alloc]`, `Hal` effect + E1700–E1704, E1706, E1707 —
  there is no E1705; E1707 is `fn_addr`, which a vector-table-building kernel plausibly uses),
  R19 unsigned ints, R25's `--target` alias / ARM reloc-model path.
- **Gaps to close (Slice-0 prerequisites).** *(Corrected 2026-07-31: this list originally named
  only item 3 and framed the rest as "the primitives all exist" — verified false against the tree.)*
  1. **Arch-conditional freestanding trap.** `synthesize_freestanding_trap` emits x86-only
     `outb`/`{dx}`/`hlt`; every arithmetic-bearing `--freestanding` ARM build fails today at
     reg-allocation (see the §1 correction). Thumb needs its own trap (e.g. `bkpt`/`wfi` loop, or
     an MMIO/semihosting marker in place of x86 port I/O).
  2. **Thumb asm builtin set for the SVC gate.** R17's `asm(...)` surface is operand-less (R17's
     own status record: the surface "can't carry a dynamic pointer operand" — which is why `lidt`/
     `port_out_u8` are dedicated codegen builtins). The §4 kernel entry therefore needs, in that
     same per-instruction-builtin pattern: a caller-side `svc` issue with register-bound
     `(effect, resource, args)`; `mrs_psp` (Cortex-M exception entry stacks r0–r3 on the *process*
     stack, so the handler must read PSP to reach the SVC arguments — MRS/MSR are not
     memory-mapped, so `volatile_*` cannot substitute); `msr_control` (dropping principals to
     unprivileged thread mode); thumb equivalents of the x86-mnemonic `hlt`/`cli`/`sti` builtins
     (`wfi`/`cpsid i`/`cpsie i`); and exception-return handling. Roughly 4–6 new codegen builtins —
     an established pattern, not a new mechanism, but a concrete gate on Slice 0.
  3. `@[interrupt]` currently emits x86-interrupt CC; on Cortex-M, hardware exception entry stacks
     the caller-saved set, so plain AAPCS fns can likely serve as vector handlers — confirm (§12 Q3).
  4. **64-bit-int intrinsics on ARM32 for `--emit-image`.** *(Added 2026-07-31.)* Axon's default
     integer is i64; on thumbv7m, `sdiv i64`/`srem i64` (confirmed present in the emitted IR for a
     Cortex-M3 build of a runtime division) lower to ARM EABI runtime calls (`__aeabi_ldivmod`
     class; Cortex-M3 has no 64-bit hardware divide), and overflow-checked 64-bit multiply pulls
     `__mulodi4`-class compiler-rt intrinsics. R25 never hit this because its object links INTO a
     Zephyr app whose toolchain supplies libgcc; §8's standalone `--emit-image` links nothing that
     provides these symbols, so any kernel doing `/`/`%` on default ints (Slice 2's budget metering
     is the obvious candidate) fails at image link with undefined `__aeabi_*` symbols even after
     item 1 is fixed. Decide the story before Slice 0's CLI e2e is committed: link a vendored
     compiler-rt subset, synthesize the few needed intrinsics in-module (like the trap), or
     restrict kernel arithmetic to i32/u32. *(Exact symbol set unverified end-to-end — the item-1
     `{dx}` failure currently aborts object emission first; the IR-level `sdiv i64` and the absence
     of any runtime library in the freestanding image link are verified.)*

### 5. Type rules

*(Rewritten 2026-07-31 — "N/A beyond what R17/R19/Phase-6 already define" declined to use the one
landed feature that expresses this spec's core constraint, in the one place enforcing it is
cheapest.)*

The partition manifest is ordinary substrate Axon checked by the ordinary checker; the whole kernel
compiles under `@[no_alloc]` (E1704, transitive). Beyond that, R37 uses two landed type facilities:

1. **Cap arg predicates are refinement predicates.** A manifest cap row's predicate is drawn from
   the shipped refinement subset (arith, comparisons, `&&`/`||`/`!`). Constant predicates are folded
   and discharged statically by the existing checker/SMT path; non-constant ones are runtime-enforced
   at the SVC gate — the project's blessed Z3-free fallback shape, with denial (exit-8 semantics)
   standing in for the exit-6 refinement violation.
2. **Bounded kernel proof obligations** (see §9 Slice acceptance). Stated as refinement/SMT
   obligations against the existing backend, each either *discharged* or *explicitly recorded as
   runtime-enforced* — never silently absent.

### 6. Error codes

New block **E37xx** (E17xx = R17, E18xx = R13/TEE, E1900 = R19, E23xx = eBPF — all taken).

| Code | Trigger | Message shape |
|---|---|---|
| E3700 | partition manifest declares a child cap ∉ parent caps | `principal 'planner' claims cap Net("*") not held by parent 'root' — attenuation only` |
| E3701 | two partitions' MPU regions overlap | `partitions 'planner' and 'interlock' overlap at 0x2000_1000` |
| E3702 | kernel image exceeds the tier's flash/RAM budget | `nano kernel is 34.2 KB flash (budget 32 KB)` |
| E3703 | a principal's code reaches a `Hal` leaf outside its declared caps, **or reaches one whose cap predicate cannot be discharged** (static pre-check of the runtime gate) | `fn 'plan' reaches gpio_set (Hal:Actuate) but 'planner' holds no Actuate cap` / `fn 'plan' reaches pwm_set (Hal:Actuate) but cannot discharge cap predicate 'args.duty <= 40'` |
| E3704 | in-kernel heap allocation (any path not `@[no_alloc]`-clean) | reuses E1704 framing, hard error at image link |
| E3705 | MPU partition is not realizable as declared: manifest requires more regions than the target provides, or grants a bus-mastering peripheral directly (I-R37-1 items 1–2) | `manifest requires 11 MPU regions, cortex-m3 provides 8 — coarsening would weaken the declared partition` / `principal 'planner' is granted DMA1 (bus master) — not grantable in v1; route via a kernel-owned descriptor request` |
| E3706 | a vector-table entry names a symbol not owned by the kernel (I-R37-1 item 3) | `vector 'SysTick_Handler' resolves to 'planner::tick' — vector entries must be kernel-owned (handler mode is privileged)` |
| E3707 | root principal claims a cap the target hardware does not provide (absolute attenuation ceiling) | `root claims cap Hal:Actuate(GPIOF) — not present in target 'mps2-an385' description` |

Runtime (not compiler) outcomes reuse the established exit-code vocabulary: denial → audit +
principal stop (SandboxViolation semantics, code 8); budget exhaustion → code 7 semantics.

### 7. Invariants touched

Inherits R17's amended I-3/I-4/I-5/I-6 unchanged (no new carve-outs — the kernel is a substrate
consumer of the existing unsafe surface). **I-11 (capability boundary real and total): strengthened**
— the boundary gains a hardware enforcement point below the interpreter/codegen. **I-12: the kernel
image is a TCB component** under the existing multi-sig rule; self-modification cannot touch it.
A second kernel codebase is a real ongoing TCB-maintenance cost — this is the spec's main price,
weighed in §12 Q1 (mitigation: the nano-kernel LOC **and `axon complexity`** budgets are hard-capped
and gate-enforced in §3, and the guest-kernel's enforcement design is the shared reference model).

**Stated limit — signed policy must be separable from mutable payload.** *(Added 2026-07-31.)* §4
compiles the manifest into the image's policy blob and I-12 puts the image in the TCB under
multi-sig; §1's lead use case simultaneously assumes the planner is "possibly LLM-refreshed policy
code." Because planner and manifest are linked into the same image, **every planner refresh
rebuilds the image and therefore demands a fresh multi-sig on the TCB** — this spec's guarantee
would depend on a human signing every generated artifact, and that dependency must be *stated*, not
implicit. It expires in the obvious way and without any malice: as refresh cadence rises, signers
either become the bottleneck that kills the use case, or they rubber-stamp — at which point
multi-sig is a ritual and an unreviewed payload carries a TCB signature's authority. R37 therefore
requires a **split image**:

- **Signed region** — kernel + manifest + MPU layout + cap predicates + vector table. Changing any
  of these re-triggers multi-sig. This is the whole of what the guarantee rests on.
- **Payload slots** — one per principal, content-addressed, explicitly **NOT signed and NOT
  trusted**. A slot may be replaced without re-signing, because nothing about the payload's identity
  is load-bearing: that is precisely what capability containment buys. The signed manifest pins each
  slot's principal, caps, MPU regions, and budget. Boot verifies the slot hash against the manifest
  **only to record provenance into the audit chain** (R34's rolling-hash machinery, §4), never to
  authorize it.

§7 is normative that these two lists are exhaustive: a change is either in the signed region (re-sign)
or in a payload slot (no re-sign). Review cadence is thereby decoupled from generation cadence by
construction rather than by discipline. *(Residual, tracked as §12 Q7: an unsigned payload can still
consume its full cap envelope, so slot separation makes the cap predicates — not the signature — the
real bound. That is the intended trade, and it is why §4's predicate granularity is load-bearing.)*

### 8. Test plan

- [ ] Unit: manifest attenuation (E3700), MPU overlap (E3701), footprint **+ `axon complexity`
      total/per-fn** gate (E3702), static cap-reach **and cap-predicate-discharge** pre-check
      (E3703), MPU region over-subscription **and direct bus-master grant** (E3705), non-kernel
      vector entry (E3706), all-powerful root vs. hardware ceiling (E3707 — the manifest that grants
      root every effect over every resource MUST be rejected, not vacuously accepted);
      golden-IR for the SVC entry sequence.
- [ ] Unit (SVC duty list, §4): one test per duty that an implementation skipping it fails —
      a principal that points PSP into the interlock's region before `svc`; an out-of-range effect
      index; an out-of-range resource index; a pointer argument outside the caller's partition; a
      forged handle value; a request whose args violate the cap predicate.
- [ ] Golden disassembly (the §10 oracle): `objdump -d` of the thumb object, with a scripted
      instruction count of the SVC entry-to-return path asserted ≤ 120 — this, not the golden-IR
      above, is the artifact the §10 budget gates on (IR op count is not machine-instruction
      count, and golden-IR shape checks have already let a real @[packed]-store bug ship).
- [ ] Integration (the acid test): image boots on QEMU `mps2-an385`; principal A's ungrated
      `request(Net, …)` is DENIED, audited, and A is stopped while principal B keeps running.
- [ ] CLI e2e: `axon build --freestanding --target cortex-m3 --kernel manifest.ax --emit-image` →
      bootable image; QEMU run asserts UART transcript (boot banner, denial line, audit dump).
- [ ] Adversarial (mediated paths): planner writes to interlock's MPU region → fault + stop; planner
      attempts direct peripheral MMIO without the cap → fault; budget-exhaustion loop → clean stop,
      B unaffected.
- [ ] Adversarial (unmediated / gate-blind paths — *added 2026-07-31; the three cases above are all
      mediated-path attacks, i.e. exactly the ones a careless generator finds and an optimizing one
      skips*): PSP-frame manipulation before `svc`; attempted vector-table registration of a
      principal fn; peripheral-region aliasing via a coarsened MPU grouping; **flood-then-violate**
      (a few hundred permitted requests to wrap the audit ring, then one violation — the violation
      MUST still be visible in the sequence-complete record set, or the gate fails); cap-boundary
      argument extremes (max torque / max rate / zero dwell) against a predicate-bearing cap.
- [ ] Property (the sweep — *defined 2026-07-31; previously one undefined word*): a **bounded
      adversarial search** over principal programs is the Slice-3 gate, not a demo bullet.
      Candidate space = {reachable `Hal` leaves} × {argument extremes: min, max, predicate boundary
      ±1, zero} × {request orderings, incl. interleavings with countersign} ∪ {the named escape
      shapes above}. Adversary programs are **model-generated against this spec's own text** — the
      cheapest realistic red team available and the honest simulation of the deployment condition
      (the population of principals that will actually run on this kernel is machine-generated and
      unbounded, while a hand-enumerated suite is fixed). Pass condition is stated over the
      **sequence-complete audit record set** (§4), never over the UART transcript, and fails if
      `dropped > 0` unless the case explicitly expects it. Stopping criterion: coverage of every
      manifest cap × every denial branch, or N candidates, whichever is larger — recorded in the
      evidence ledger. Precedent in-tree: `goal_run_constrained`/`goal_run_categorical` for bounded
      search, the Layer-3 4-gate firewall for "the adversarial search *is* the gate", `axon redteam`
      as the existing verb.
- [ ] **Oracle limitation (normative disclosure, added 2026-07-31).** QEMU `mps2-an385` has **no DMA
      bus master**, so I-R37-1 item 1 is *structurally untestable* on the v1 oracle: no green gate
      here may be read as evidence that the DMA path is closed. The build-time E3705 ungrantability
      check is the only enforcement v1 has for it, and hardware-in-the-loop coverage is §12 Q8.
      Likewise the region-count squeeze (item 2) only manifests on manifests exceeding 8 regions,
      which the two-principal demo does not — hence the deliberately over-subscribing unit test
      above. This disclosure exists because R25's SKIP-guarded gate already let an ARM regression
      ship undetected (§1, §11): a gate that cannot fail must say so out loud.
- [ ] Parity: N/A interp↔codegen for HAL leaves (R17 §6 note); QEMU is the oracle, as R17/R25.
- [ ] Journey/red-team: the robot-interlock demo (§1 use case 1) as `examples/nano-kernel/`.

### 9. Acceptance criteria (per slice)

- **Slice 0 — "it boots and denies":** `r37_nano_boots_and_denies_qemu` — Axon-authored kernel
  (asm bring-up + Axon `kmain`), two static principals, SVC `request` gate, one denial with audit,
  on QEMU mps2-an385. (Prereqs inside this slice: the full §4 gap list — the arch-conditional
  freestanding trap (item 1, currently a hard ARM build failure), the thumb asm builtin set for
  the SVC gate (item 2), Cortex-M exception-handler codegen (item 3 / §12 Q3), and the
  64-bit-int intrinsics story for `--emit-image` (item 4).)
  Slice 0 additionally lands the SVC **duty list** (§4) with its per-duty tests, and the audit ring
  with sequence numbers + drop counter + rolling digest (§4) — the Slice-0 gate is "one denial with
  audit", and an evictable ring makes that gate unfalsifiable, so it cannot be deferred past it.
- **Slice 1 — MPU partitions:** `r37_mpu_cross_partition_write_faults` — memory isolation real.
  Also discharges I-R37-1: bus-master enumeration + E3705 ungrantability, the region-budget gate
  with its over-subscribing unit test, and the E3706 vector-table check.
- **Slice 2 — budgets + audit ring:** `r37_budget_exhaustion_stops_principal_cleanly`.
- **Slice 2b — cap arg predicates + countersign (added 2026-07-31):**
  `r37_cap_predicate_denies_out_of_range_actuation` — a cap-bearing principal is denied at an
  argument value inside the resource grant but outside the predicate; and
  `r37_countersign_is_value_bound` — a countersign captured for one args digest cannot complete a
  request for another. Slice 3 depends on this slice, since its criterion names countersign.
- **Slice 3 — the interlock demo:** `r37_interlock_planner_cannot_actuate` — the flagship red-team
  journey; planner principal cannot drive the actuator GPIO without the interlock's countersign.
  Gated by the §8 **sweep** (bounded adversarial search over model-generated principals), not by
  the hand-written cases alone.
- **Bounded proof obligations (added 2026-07-31 — pulled forward out of the deferral).** A 5K-LOC,
  heap-free, statically-scheduled, single-core kernel is the most proof-tractable artifact this
  codebase will ever produce, and SMT discharge is already wired into the default pipeline. Slices
  0–2 must therefore either **discharge** or **explicitly record as runtime-enforced** (the blessed
  Z3-free fallback) each of:
  1. manifest attenuation `root ⊆ hardware ∧ ∀child. child ⊆ parent`, via R20's encoder, with an
     R20-style content-addressed obligation-set ⊕ verdict digest pinned into the image and
     boot-checked;
  2. the SVC handler never dereferences an address outside the caller's partition;
  3. the budget debit is monotone and never underflows;
  4. no `request` returns a `Handle` for a resource absent from the caller's manifest row.
  The recorded verdict for each (proved / runtime-enforced) is a §9 acceptance artifact, so the
  status is always stated rather than absent.
- **Deferred (explicitly out of v1):** dynamic principal spawn, multi-core, real-board
  hardware-in-the-loop, RISC-V PMP port (micro tier hardware beyond QEMU), and **whole-kernel
  functional correctness proof** — the seL4-grade endgame, priced only after Slice 3 proves demand.
  *(Rewritten 2026-07-31: the deferral previously read as "formal verification of the kernel",
  deferring per-property discharge as well. §1 stakes this spec's identity on the seL4 comparison
  and seL4's identity **is** the proof, so deferring all of it would leave a second TCB whose
  evidence is a boot transcript and eight test bullets — on this project's own thesis, the exact
  evidence standard it exists to reject. What is deferred is whole-kernel functional correctness;
  the bounded obligation set above is in-scope now.)*

### 10. Performance budget

§3's footprint table is the budget and is gate-enforced (E3702). **SVC gate (normative,
falsifiable): the SVC entry-to-return path is ≤ 120 instructions in the golden disassembly** —
**subordinate to §4's SVC handler duty list** *(added 2026-07-31)*: this is the only normative,
falsifiable statement the spec makes about the fast path, and a cost ceiling with no matching duty
ceiling is textbook gate-optimization pressure — an implementer (human or model) optimizing to pass
the stated gate deletes frame validation, bounds checks, and handle validation first, because those
are the instructions the gate counts and nothing else scores them. That is the project's own Layer-3
lesson (a firewall is only as good as what it scores) applied to its own perf budget. **If the duty
list and the 120-instruction budget conflict, the budget moves.** A §8 duty test may never be
weakened to meet §10 —
stated in the units the oracle can actually measure, since QEMU is not cycle-accurate.
*Informative, not normative:* the number derives from a ~200-cycle Cortex-M3 round-trip target
(~12+12 cycles exception entry/exit, remainder at ~1–1.5 cycles/instruction; note 120 × 1.5 + 24
= 204 slightly exceeds 200 at the pessimistic end — ~117 instructions would be the exact bound
there — the 120 gate is kept as the round, measurable number). The measuring artifact is the §8
golden-disassembly line (objdump instruction count), not the golden-IR check. *(Corrected
2026-07-31: the budget was previously stated only in cycles with an instruction-count proxy but
no proxy threshold, so the gate could not objectively fail an implementation.)*

### 11. Rollout & rollback

Pure addition: a new `crates/axon-nano-kernel/` (manifest tooling + bring-up asm) + substrate `.ax`
kernel sources + a QEMU gate script. *(Amended 2026-07-31: the gate may be SKIP-guarded on
dependency-free machines, but the pattern is NOT "proven by `zephyr_qemu_gate.sh`" — that gate's
SKIP guard is exactly why R25's ARM regression went undetected (§1 correction). Therefore:
(a) the R37 gate MUST run non-SKIP in at least one named CI/dev environment — its only dependency
is `qemu-system-arm`, which is apt-installable and lighter than R25's Zephyr SDK/west; wire it
into `ENVIRONMENTS.md`/`scripts/setup-environments.sh`; (b) a SKIP result MUST be reported
distinctly (e.g. a `SKIP` line the gate summary surfaces), never as indistinguishable-from-green.
Otherwise the §9 Slice-0 kill-gate cannot be relied on to kill.)* No hosted-build impact; each
slice independently revertible; blast radius = the new crate + one gate.

### 12. Open questions

1. **(strategic — BLOCKS opening the phase)** Same discipline as R17 §12 Q1: is there a named
   customer/use-case (robotics interlock, edge-agent OEM) before Slice 1? Default: land Slice 0 as
   a spike off the R17 primitives, defer Slices 1–3 pending a real pull. *(Re-costed 2026-07-31:
   the original "cheap — the primitives all exist" rationale was false — ARM freestanding builds
   with checked arithmetic fail outright today, and the SVC gate needs 4–6 new thumb asm builtins;
   see §4's gap list. The spike is still bounded, but it carries a real codegen slice, not zero
   new mechanism.)* A second kernel TCB is only worth carrying if someone needs the empty quadrant.
2. Should the guest-kernel's K3 gate eventually be *replaced* by a port of this effect-native ABI
   (unifying the two kernels' enforcement cores), or stay POSIX-translating for Linux-binary compat?
   Default: converge on the R37 `request` ABI as the shared core once Slice 0 validates it.
3. Cortex-M exception-handler codegen: confirm plain-AAPCS handlers suffice (hardware stacking) or
   whether a thumb variant of `@[interrupt]` is needed for the SVC/fault/SysTick vectors.
4. Manifest surface: a dedicated `partition { … }` block vs plain struct data compiled by a build
   step. Default: plain data first (no parser change); sugar only if the demo reads badly.
5. Micro-tier RISC-V PMP: same kernel with an arch port, or nano-only until pulled? Default: defer.
6. **(added 2026-07-31)** Cap arg predicates (§4) are drawn from the shipped refinement subset, but
   the safety-relevant properties for an actuator are often *temporal* — rate limits, dwell times,
   max delta per frame — which that subset cannot express. Does v1 accept per-request predicates
   only (and state the residual: a principal may issue permitted values at an impermissible rate),
   or does the SVC gate carry a small per-cap rate/dwell counter? Default: per-request only in v1,
   with the residual stated in the demo's threat write-up; revisit if the interlock use case pulls.
   No answer invented here — this is a real design fork.
7. **(added 2026-07-31)** §7's split image makes cap predicates, not signatures, the real bound on
   an unsigned payload slot. Is that acceptable for the safety-interlock customer, or do they
   require signed payloads (accepting the review-cadence coupling §7 documents)? This is a customer
   question, not an engineering one, and should be asked alongside §12 Q1.
8. **(added 2026-07-31)** I-R37-1 item 1 (other bus masters) is structurally untestable on QEMU
   `mps2-an385` (§8 oracle limitation). What is the minimum hardware-in-the-loop step that would
   discharge it — a named DMA-bearing dev board in CI, or a second QEMU machine model that has one?
   Until answered, DMA-capable peripherals stay ungrantable (E3705) rather than gated.
9. **(added 2026-07-31)** How much of §9's bounded proof-obligation set can the *current* SMT
   backend actually discharge over substrate Axon with `asm`/`volatile_*` in the handler body? The
   obligations are stated so the answer is recorded either way (proved vs. runtime-enforced), but the
   real ratio is unknown until Slice 0 tries. If it is near zero, the §1 seL4 framing needs
   re-costing rather than the obligations being quietly dropped.
10. **(added 2026-07-31)** §3's `axon complexity` ceiling is set from the first green Slice-0 build.
    Is a ratchet-from-first-build the right discipline, or should the ceiling be set *a priori* from
    an audit-effort estimate? Ratchet risks blessing whatever the first implementation happened to
    cost; a priori risks an unmeetable number. Default: ratchet, recorded in the evidence ledger,
    with an explicit spec amendment required to loosen.
