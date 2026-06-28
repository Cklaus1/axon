# Axon ASI-Native Kernel — Design Sketch

*A kernel whose primitives are capabilities, effects, principals, budgets, and goals —
not processes, files, sockets, and syscalls. The design target, with an honest line drawn
between vision and what exists today.*

> **Status:** this is a **design sketch**, not a shipped kernel. What exists today is the
> substrate (boot + a policy-driven syscall gate that **provably denies a real syscall on
> KVM** — see `AXON_KERNEL.md` and `scripts/kernel_enforce_test.sh`). Everything below
> describes where that substrate would go if built out ASI-first. Each section ends with a
> **Today** line stating what is real now.

---

## 1. Why a different kernel at all

The project's thesis is *capabilities as language features* — `@[contained]`, effect rows,
refinement types. That thesis has a soft underbelly: at runtime, enforcement currently
bottoms out in either the language interpreter (Layer 2) or a syscall gate that **translates
POSIX syscall numbers into effect bits**. POSIX is an ambient-authority API — `open`,
`socket`, `execve`, `getenv` all assume the caller may name anything. The gate claws
authority back *after the fact*, syscall by syscall.

An ASI-native kernel inverts this: **the only thing a principal can do is request a typed
effect it holds a capability for.** There is no ambient authority to claw back, because
there is none to begin with. The "syscall surface" is the effect-row algebra the language
already speaks. The kernel and the language share one vocabulary end to end.

The payoff is the project's strongest differentiator: a **measured, minimal, auditable TCB**
(target ~15K LOC) that *is* the capability model, versus ~35M LOC of Linux that merely hosts
a policy bolted on top.

**Today:** the gate maps Linux syscall numbers → effect bits (`enforce.rs::required_effect`).
The POSIX layer is still present; the inversion below is the design.

## 2. The object model — five kernel-native primitives

These replace `process / uid / fd / file / socket`. Each is an unforgeable kernel object.

| Primitive | Replaces | What it is |
|---|---|---|
| **Principal** | process / uid | The unit of execution & authority: an unforgeable identity + a capability set + a budget + (optionally) a goal. |
| **Capability** | fd / file-perm | An unforgeable token granting one typed effect over one named resource (`FsRead("/models/")`, `Net("api.openai.com")`). Held, never ambient. |
| **Effect** | syscall | A typed, capability-checked request: the ENTIRE privileged ABI. The effect-row algebra (`IO/FS/Net/AI/Exec/Random/…`) is the syscall table. |
| **Budget** | rlimit / cgroup | A first-class, metered ceiling on cost (CPU, memory, tokens, irreversibility). Every effect debits it; exhaustion is a clean stop. |
| **Goal** | — (new) | An objective + metric a principal pursues; the scheduler is aware of it. The unit of *intent*. |

### 2.1 Principal — the subject of everything

A Principal is the only thing that runs. It carries:

```
Principal {
    id:        u128            // unforgeable, kernel-assigned
    caps:      CapabilitySet   // the typed effects it may request
    budget:    Budget          // metered ceiling; debited per effect
    goal:      Option<Goal>    // what it is trying to achieve
    latch:     KillLatch       // one-way corrigibility latch (§5)
    parent:    Option<u128>    // the principal that spawned it
}
```

The defining rule is **attenuation**: a principal may spawn a child, but the child's `caps`
and `budget` are a SUBSET of the parent's — never a superset. Authority only narrows down
the tree. This is the object-capability model made the kernel's spine, and it is how an ASI
that spawns sub-agents is bounded *by construction*: no descendant can out-authorize its
ancestor.

**Today:** single hard-coded principal (`root`), `allowed_effects` parsed from the boot
cmdline. No spawn, no attenuation, no per-principal budget.

## 3. Effects as the syscall surface

There is no `open(2)`. There is one kernel entry — `request` — and a typed effect:

```
// the WHOLE privileged ABI (sketch)
fn request(eff: Effect, on: ResourceName, args: Args) -> Result<Handle, Denied>

enum Effect { FsRead, FsWrite, Net, Exec, Ai, Random, Clock, … }   // = the effect row
```

`request` checks `eff`/`on` against the calling principal's `caps` (and the IFC labels, §6,
and the budget, §4). A request the principal doesn't hold a capability for is `Denied` —
the same `VIOLATION` the current gate already emits, but as the *only* failure mode of the
*only* entry point, rather than a special case bolted onto 300+ POSIX syscalls.

Why this matters: **the kernel cannot be tricked by a syscall it didn't think to gate.**
With POSIX, every new syscall is a new hole to classify (`required_effect` must enumerate
them; an unknown syscall is a judgment call). With one effect entry, the attack surface is
the effect algebra — finite, total, and shared with the compiler.

**Today:** `request` is the POSIX `syscall` instruction; the gate classifies the number.
The single-entry effect ABI is the design.

## 4. Budget — structural resource bounds for an ASI

Every effect has a cost, and the kernel debits the principal's budget before granting it:

```
request(Ai, model, prompt)   // debits budget by metered token cost
request(Exec, …)             // debits by a high irreversibility weight
```

Budget exhaustion stops the principal cleanly (the R12 `KernelGoal` exit-7 semantics
generalized to the universal resource model). This is the structural answer to *"how do you
bound a self-improving agent's resource use?"* — not a monitor that watches and reacts, but
a ceiling the kernel debits and enforces below the principal's reach. Irreversibility is a
budget axis: actions that can't be undone cost more, so a low-budget principal literally
cannot afford a catastrophic action.

**Today:** `KernelGoal` budget metering exists in the *userland* kernel services
(`kernel.rs`); the bare-metal kernel has no per-principal budget.

## 5. Corrigibility as a kernel invariant

The kill-switch is not a service — it is a kernel primitive below every principal:

```
KillLatch { tripped: AtomicBool }   // one-way: false → true, never back
```

- An **out-of-band operator channel** (a privileged port / hypercall the principal cannot
  address) can trip any principal's latch.
- The scheduler checks the latch every quantum; a tripped latch halts the principal within
  one quantum (exit 4 / HALTED), and the halt is irreversible from inside.
- **No principal can clear its own latch** — it lives in kernel memory the principal holds
  no capability to write. A self-modifying principal can rewrite its own code; it cannot
  reach the latch.

The kernel's standing invariant: *self-modification cannot weaken corrigibility.* This is
the R27 latch + the R32 corrigibility proof, but as a kernel-enforced property rather than a
userland convention — the strongest place to put it.

**Today:** the latch exists in `axon-os` userland (R27) with a TLA+/Coq proof of the
property; it is not yet a bare-metal kernel object.

## 6. Information-flow labels in the kernel

The Phase-14 lattice (`Tainted`/`Trusted`/`Secret`/`Public`) becomes kernel-tracked metadata
on data crossing effect boundaries. The kernel can then refuse a *flow*, not just an effect:
a `Secret`-labeled buffer cannot be the argument to a `Net`/`Ai` effect, regardless of the
principal's capabilities. This closes the exfiltration class structurally at the kernel, the
same way `@[contained]` closes it at compile time — defense in depth across the two layers.

**Today:** the lattice exists as a userland stdlib type; the kernel does not track labels.

## 7. Goal-directed scheduling

The scheduler is goal-aware. Instead of round-robin threads, it allocates compute to
principals weighted by `(budget, priority, goal-progress)`, and can pre-empt a principal
whose goal is met (stop early) or whose metric regresses (the adaptive-optimizer semantics).
This makes *intent* a kernel concept: the kernel knows what each principal is for, and can
reason about stopping it. (The most speculative section — a research direction, not a 2024
feature.)

**Today:** no scheduler beyond single-program launch.

## 8. Boot-time measured TCB

The kernel measures itself + its policy at boot (the R26 `axtcb1` chain) and refuses to run
under a non-blessed measurement. The TCB is enumerated and minimized; any self-modification
of the kernel passes through the Layer-3 firewall's capability-monotonicity gate, which
cannot admit a change that weakens a measured invariant (§5, §3). Confidentiality (guest RAM
vs. a privileged host) requires SEV-SNP/TDX; the default is a software-TPM stand-in.

**Today:** attestation (R26–R31) measures the kernel image and produces an `axtcb1` digest;
the substrate is a software-TPM stand-in, not hardware memory encryption.

## 9. "Written in Axon" — the self-hosting end state

The split that makes this tractable: a **thin unsafe substrate** (boot, MSRs, page tables,
interrupt handlers, the latch's atomic) in Rust/asm — the part Axon can't express — and a
**policy core** (the effect algebra, capability checks, budget accounting, IFC, the scheduler's
goal logic) that is exactly the pure, total, refinement-typed code Axon is *good at*. The end
state: the policy core is compiled by Axon's own (eventually verified) codegen, so "the
enforcement kernel is itself verified by the compiler" stops being aspirational. This is
downstream of substantial compiler work (a no-runtime freestanding Axon target with the
maturity the R17 slices begin) — it is the horizon, not the next sprint.

**Today:** the kernel is Rust `no_std`; Axon emits only the R17 freestanding banner-kernel
slice. The policy core in Axon is the long-horizon goal.

## 10. Migration path (incremental, from the current substrate)

The current kernel boots and provably denies a syscall. The ASI-native design is reachable
in order, each step independently testable:

1. **Effect-request ABI** — replace the POSIX-number translation with a single `request(eff,…)`
   entry; the existing gate logic moves behind it.
2. **Principal as a kernel object** — identity + capability set; attenuating `spawn`.
3. **Budget metering** — per-principal debit on every effect; clean exhaustion stop.
4. **Corrigibility latch** — the one-way kill latch as a kernel primitive, scheduler-checked.
5. **IFC labels** — track `Secret`/`Tainted` across effect boundaries; refuse forbidden flows.
6. **Goal-aware scheduler** — multi-principal, intent-weighted.
7. **Policy core in Axon** — once the freestanding Axon target matures.

Steps 1–4 are concrete systems work on the existing substrate (the hard part — boot, the
syscall gate, live denial — is done). Steps 5–7 are research-grade.

---

## The honest one-paragraph summary

Today there is a real bare-metal kernel that boots on QEMU and Firecracker and **provably
denies a real syscall by policy, live, on KVM**. It is POSIX-syscall-shaped, single-principal,
and Rust-written. *This document* is the design for turning that substrate into an ASI-native
kernel: effects as the only ABI, principals/capabilities/budgets/goals as the primitives,
corrigibility and information-flow as kernel invariants, a measured minimal TCB, and —
eventually — the policy core written in and verified by Axon itself. It is a multi-quarter
research program, not a feature; the value of writing it down is to make the moonshot
*legible and sequenced* rather than vague.
