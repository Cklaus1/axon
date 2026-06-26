# Axon OS — Vision

> **Thesis in one line.** Axon OS is the operating system for running code written by AI
> you don't trust: a **minimal trusted base that holds a machine-checked proof of what
> every program can and cannot do**, on top of which AI-synthesized software runs fenced-in,
> budgeted, and fully audited.
>
> This is a focused companion to `VISION.md` (the language+runtime vision). Where that doc
> says "software AI writes and runs on your behalf," this one commits to the sharpest version
> of that bet: **the OS exists to contain AI, especially superhuman AI — not to be a general
> OS.** Shipped state: `STATUS.md`. Plan: `ROADMAP.md`. This is the strategy the codebase has
> already been built toward.

---

## 0. The bet, stated plainly

A general-purpose OS is a **trap** for this project. To win there you must beat Linux/Redox on
drivers, filesystems, networking, scheduling, and a decade of hardening — as a young language —
and your differentiators (capabilities, effects, refinement proofs, R20's proven minting) become
dead weight nobody asked for. There is no wedge.

An **AI-containment OS** is the one product where *everything already built becomes load-bearing*:

- `@[contained]`, effect rows, refinement types, the SMT discharge, and R20's proven capability
  minting are **worthless to a general OS** (you trust your own processes) and are **the entire
  point** of an OS that runs code it doesn't trust.
- The stdlib was deliberately chosen to be `Goal / Principal / Budget / Effect / Store / Agent /
  World / Belief / Plan` instead of `Vec / HashMap / Iterator` (ROADMAP §2.6). That is a
  containment vocabulary, not a general-purpose one. A pivot to general-purpose throws it away.

The codebase has been voting for this OS for fourteen phases. This document makes it the explicit
strategy and draws the scope lines that keep it tractable.

---

## 1. Why ASI is the core — and why we *maximize* for it

Containment that is merely *good enough against human-speed adversaries* is a solved, commoditized
problem (containers, VMs, seccomp). It is "good enough" precisely because the adversary is not
smart enough to find every hole a runtime check leaves.

**A superhuman intelligence is exactly the adversary that finds the hole.** The moment the thing
inside the box is smarter than the people who built the box, the only containment worth anything is
containment that is **proven, not patched** — a property that holds for *all* inputs, including the
adversarial ones the AI will search for, rather than a check that happens to cover the cases its
authors imagined.

That is the whole reason Axon's proof machinery is worth its cost. For human-written code, proving
capability bounds is over-engineering. For ASI-written code, "this program provably cannot exceed
these capabilities, ∀ inputs" is the difference between safe and catastrophic. So we optimize for
ASI because **ASI is the only adversary that makes the proofs pay for themselves** — and an OS that
is genuinely safe against ASI is automatically safe against everything weaker.

### The honest boundary of the claim

Axon OS can prove: *this AI-written code cannot exceed the capabilities it was granted.* It **cannot**
prove the capabilities you granted were wise, stop a bad outcome *within* granted bounds (containment
≠ alignment), or close escapes *below* the proof boundary (hardware, side channels, an unproven
driver). The product claim is therefore narrow and defensible: **"the OS that gives you a proof about
what AI code can't do, on top of a minimal trusted base"** — not "the safe OS for AI." Overselling
total safety is where the thesis loses credibility.

---

## 2. Architecture: a tiny fixed substrate + a dynamic synthesized userland

```
        ┌──────────────────────────────────────────────────────────────┐
        │  DYNAMIC USERLAND  (untrusted — synthesized per intent)        │
        │   AI models · agents · tools · "apps" as ephemeral programs    │
        │   each a Principal: capabilities + budget + effect row         │
        └───────────────┬──────────────────────────────────────────────┘
                        │  intent → synthesize → PROVE bound → approve → run → audit
        ┌───────────────▼──────────────────────────────────────────────┐
        │  THE TCB  (trusted — small, enumerated, attested, un-weakenable)│
        │   refinement+SMT prover · effect enforcer · principal authority │
        │   capability minter (R20: PROVEN) · cost meter · scheduler ·    │
        │   supervisor · LLM gateway · sandbox · audit log · replay ·     │
        │   risk classifier · deploy gate                                 │
        └───────────────┬──────────────────────────────────────────────┘
                        │  one swappable boundary (TCB-size dial — §3)
        ┌───────────────▼──────────────────────────────────────────────┐
        │  HOST SUBSTRATE  (container / micro-VM / bare metal / wasm)     │
        └──────────────────────────────────────────────────────────────┘
```

The discipline that keeps this real:

1. **Microkernel-tiny TCB.** The trusted base is the enumerated list above (ROADMAP §7), nothing
   more. Everything else — every model, agent, tool, "app" — runs *above* the proof boundary as
   untrusted userland. seL4 proved a *small fixed* kernel and it took years; the proof surface only
   stays tractable if the TCB stays minimal. **Every feature must either shrink the TCB or
   strengthen a containment proof, or it is out of scope.**
2. **Smallest hardware surface.** An unproven driver is, against an adversarial AI, a full escape.
   Containment discipline means treating every unproven driver as an open hole until proven or
   removed. (This is why GPU/UI is *not* v1 — see §7.)
3. **The proof leads.** If we're adding general-OS conveniences, we've drifted back into the trap.

---

## 3. Where it runs — the host-substrate options

Because containment lives in the **language** (capabilities/effects/proofs), not the host, the host
substrate is just the bottom of the TCB. **The same Axon program + the same proofs run on all four
substrates unchanged** — swapping substrate is a *TCB-size and trust dial*, not a rewrite. This is the
portability win, and it means we don't have to choose one: we ship a portfolio and climb toward the
smallest TCB over time.

| Substrate | Isolation boundary | What the TCB must trust | Time-to-ship | Thesis fit | Status in repo |
|---|---|---|---|---|---|
| **Container / daemon** (the "userland OS", ROADMAP §2.1) | OS process + Axon caps | the **entire host kernel** + hypervisor | shortest — runs now | weakest: a host-kernel bug is an escape route an ASI can hunt | `axon` runs as a host process today; `AxonHost` trait (R7b) abstracts the ~6 host touchpoints |
| **Micro-VM** (Firecracker / Cloud Hypervisor style) | hardware virtualization + Axon caps | a tiny VMM + the host hypervisor | medium | **strong and pragmatic**: minimal device surface, hardware-enforced, no host-kernel reach without a hypervisor escape | not built — needs a virtio guest target (close cousin of R17 freestanding) |
| **Bare metal** (R17 capability-microkernel) | the Axon microkernel itself | only the CPU + firmware | longest | **purest**: smallest possible TCB, isolation proven in-language from the boot sector, no host kernel to attack — the seL4-for-AI north star | R17 Slices 0–3 **landed**: `@[entry]`/`@[panic_handler]`, inline asm, SMP atomics, `@[repr(C)]`, `no_alloc`, QEMU boot |
| **Browser / wasm** (R7c) | wasm sandbox + browser | the browser + JS engine (large, but battle-tested) | short — interp→wasm exists | good for **reach and the approval UI**; you trust a big sandbox you didn't build | `wasm32-unknown-unknown` via asyncify + `wasm32-wasip1` both run the interpreter today |

**Practical reading of "do we need X?"**

- **Run on Windows/Mac?** Yes, two ways, no bare metal required: the **container/daemon** form (an
  Axon process on the host — works now) for lowest friction, or the **micro-VM** form (Windows
  Hyper-V / macOS Virtualization.framework) for real hardware-enforced isolation. Bare metal does
  not "run on Windows" — it *replaces* it; that's the v3 north star, not how you reach users.
- **Boot on hardware?** That's the R17 bare-metal track — already booting under QEMU. It is the
  endgame (smallest TCB), deliberately *not* the first mile.
- **Browser/wasm?** Yes — it's the cheapest distribution and the natural home for the
  intent→approve UI. The browser is the sandbox; great for demos and reach, not for the strongest
  isolation claim (you trust the whole browser).
- **Micro-VMs?** This is the **sweet spot for v1/v2**: real hardware-enforced isolation with a tiny
  device surface, runs on any host with a hypervisor, and the AI cannot reach the host without a
  hypervisor/VMM escape — a far smaller and better-studied attack surface than a full host kernel.

**The recommended climb:** container + browser to *reach and demo now* → micro-VM as the *default
isolation for real workloads* → bare metal as the *north star* that minimizes the TCB to the
absolute floor. Each step shrinks what must be trusted; none of them rewrites the userland.

---

## 4. Apps, or a dynamic ASI-driven OS?

**Dynamic — and this is the most important product difference from every existing OS.** Axon OS does
not have installed apps (fixed binaries you trust). It has a **fixed minimal substrate + an
intent→synthesize→prove→run→audit loop** in which "applications" are *ephemeral, AI-synthesized,
capability-bounded programs*:

```
  user states intent (prose)         ── "summarize my unread invoices into a sheet"
    → an ASI model (a userland Principal) SYNTHESIZES Axon code
      → the compiler PROVES the capability bound (R20-style: ∀ inputs)
        → the user approves the PROOF / the typed AST — not the code
          → it runs SANDBOXED to exactly the granted caps + budget
            → every effect is audited; the run is deterministically replayable
```

An "app" is therefore a `(goal, capability-grant, proof, audit-trail)` tuple, not a binary. It can be
synthesized on demand, run once, and discarded — or pinned and reused. This is **already half-built**:
Phase-10 `axon intent compile` (prose → typed AST), Phase-12 approval UI (intent → review → approve →
deploy → trace), Phase-11 risk-typed deploy gate. The OS is the substrate that makes that loop *safe*:
the model can synthesize anything, but it provably cannot *do* anything it wasn't granted.

What stays fixed (the substrate, not apps): the TCB, the kernel services (`kernel.rs`: principal
registry, scheduler, supervisor, store, LLM gateway, kernel goal), the model-hosting runtime, the
memory store, the tool registry, the audit/replay engine. **Minimal fixed substrate; dynamic
synthesized userland.** Not "no apps" — "apps are proven-bounded programs, not installed trust."

---

## 5. Models, memory, training, real-time

The crucial inversion: **the AI model is a userland actor, never part of the TCB.** The OS never
trusts the model; it *bounds* it. The model is a Principal with capabilities + a budget, mediated on
every call.

- **Inference / model hosting.** The `LlmGateway` (kernel.rs) already mediates every model call with
  per-token cost metering and graceful fallback-on-overrun. v1 hosts models behind this gateway as a
  capability (`net` to a model endpoint, or a local-weights capability). The model's *outputs* are
  untrusted (taint-typed: `Tainted`/`Source` lattice already shipped) until validated.
- **Memory** is a layered, already-specified stack of value types, not a monolith:
  - *Durable state* — `Store<T, Consistency, Lifetime>` (at-least-once / linearizable variants).
  - *Epistemic memory* — `World<T>` (executable world model + MDL compression) and `Belief<T>`
    (Bayesian state). The model's understanding is a fitted, refinement-gated artifact.
  - *Audit memory* — the provenance log + deterministic `replay` engine: every run is a `(Trace,
    Seed)` pair reproducible byte-for-byte.
  - *Learning signal* — `Plan`, `Feedback`, `Reward`, `Schedule` (all shipped) carry the
    improve-loop's typed history.
  - *Distributed memory* — `Replicated<T>`, `Quorum<T>`, CRDTs, causal ordering (Phase 14) for
    multi-agent / multi-node state.
- **Training.** Training/fine-tuning happens **outside the TCB, by design** — you never put gradient
  descent in the trusted base. The OS is the **gym, not the athlete**: it provides the substrate a
  learning loop needs — deterministic replayable traces, reward/feedback types, the `goal_run`
  optimizer, the world-model *compress-to-fit* loop (Phase 9 Layer-3 / `world_model.ax`), and a
  full audit trail — and a learning agent runs *against* that substrate as untrusted userland. The
  OS's job is to make the learning loop *safe and reproducible*, not to perform it.
- **Real-time.** The cooperative scheduler (fibers, kernel.rs) plus the **resume runtime** (R15:
  `host_await`/suspend-to-host, the critical-path blocker for all interactive tiers) let an agent
  suspend on an external event and resume deterministically — the basis for live, event-driven
  agents (REPLs, frame loops, approval agents) on native, wasm, and browser substrates.

---

## 6. What we need to build (gap list, grounded)

**Already shipped (the foundation is real):** the capability/effect/refinement core; R20 SMT-proven
capability minting + boot attestation; the kernel services (principal/scheduler/supervisor/store/LLM
gateway/goal); provenance + deterministic replay; the risk-typed deploy pipeline; the
intent→AST→approve→deploy loop + approval UI; native codegen at interp parity; R17 bare-metal Slices
0–3 (boot under QEMU); wasm (browser + WASI) interpreter targets; the Tier-1/Tier-2 stdlib value types.

**The gaps that stand between here and v1:**

| # | Gap | Existing hook |
|---|---|---|
| G1 | **Promote the rest of the TCB to proven** — apply the R20 pattern (proof + tripwire + attestation) to `Budget`, `Effect`/`Sandbox` enforcement, and the scheduler's isolation, so the *whole* trusted base is machine-checked, not just the minter | R20 is the template; `verify.rs` Discharged + smt.rs |
| G2 | **A micro-VM guest target** — a virtio/serial freestanding image so the AI runs in a hardware-isolated VM with a tiny device surface (the pragmatic v1 isolation) | R17 freestanding substrate is the near-cousin; needs virtio + a VMM integration |
| G3 | **Resume runtime (R15)** — suspend-to-host, the shared blocker for every interactive/real-time tier (R7c browser, R13 native, R14 mobile) | R15 spec resolved; Slice 1 is the next implementable step |
| G4 | **Model-hosting substrate** — local-weights + remote model behind `LlmGateway` as a first-class capability, with the taint lattice gating model outputs into consequential actions | `LlmGateway` + `Tainted`/`Source` shipped; needs the local-weights capability + the validation gate wired |
| G5 | **Whole-TCB attestation + content-addressed boot** — generalize R20's `axtcb1:` digest to the entire trusted base; mismatch = boot fail (the seL4-style assurance story) | R20 `tcb_attestation_digest` + R10 `E1405` manifest precedent |
| G6 | **The intent→synthesize loop as the OS shell** — make "state a goal, get a proven-bounded program" the primary user surface, not a CLI afterthought | Phase-10 intent compile + Phase-12 UI are the prototype |

Explicitly **out of scope** for the focused thesis (the de-scope list that keeps it enforceable):
GPU/UI rendering stack (R13/R16 — the largest *unproven* attack surface in the roadmap), a
general-purpose driver model, a package manager beyond capability-typed distribution, async/await,
and any "general OS convenience" that doesn't shrink the TCB or strengthen a proof.

---

## 7. Roadmap — v1 / v2 / v3+

### v1 — The Containment Substrate (prove the thesis on real isolated workloads)
**Goal:** an AI agent runs a real task, end-to-end, inside hardware-enforced isolation, where the
compiler *proved* its capability bound, the user approved the *proof*, and every action is audited
and replayable.
- Harden the **whole TCB** to R20-grade proofs (G1) + whole-TCB attestation (G5).
- Ship the **micro-VM guest** (G2) as the default isolation; keep the **container/daemon** form for
  reach and the **browser** form for the approval UI. (Three substrates, one userland.)
- The **intent→synthesize→prove→approve→run→audit loop** as the product surface (G6), backed by the
  existing risk-typed deploy gate.
- Model hosting behind the metered `LlmGateway` with taint-gated outputs (G4).
- **Explicitly NOT in v1:** bare metal as a product, GPU/UI, general apps, general drivers.
- **Done when:** a non-trivial agent task runs in a micro-VM, its capability bound is SMT-proven and
  attested, a redteam catch is demonstrable, and the run replays byte-for-byte. (The Acid Tests,
  raised to hardware-isolated, proven-bound execution.)

### v2 — The Dynamic AI Userland (the OS becomes "state intent, get proven behavior")
**Goal:** AI-synthesized, capability-bounded programs are the primary way software exists on the
system; the memory + learning substrate is productized.
- ASI-synthesized "apps" as the default surface; the model proposes, the compiler proves, the human
  approves the proof.
- The **memory stack** productized: `Store`/`World`/`Belief` + provenance/replay as the agent's
  durable, epistemic, and audit memory.
- The **self-improve / feedback loop** (G3 resume runtime + `goal_run` + world-model compress) as a
  first-class, safe, reproducible service — the OS as the *gym*.
- **Multi-agent coordination** on the shipped distributed types (`Replicated`/`Quorum`/CRDT/causal
  ordering + cross-Principal `CoordGoal`).
- Micro-VM as the default; real-time/event-driven agents via the resume runtime.

### v3+ — The Bare-Metal Capability Microkernel (maximize the thesis: smallest TCB on Earth)
**Goal:** Axon OS boots on hardware as a capability microkernel whose isolation is proven *in the
language* from the boot sector up — the seL4-for-AI endgame, where the trusted base is the absolute
floor and there is no host kernel for an ASI to attack.
- R17 bare-metal completed past the QEMU SMP harness to real hardware; minimal proven device set.
- **Attested boot** of the whole TCB (G5 generalized): content-addressed, signed, multi-sig update
  path; mismatch = boot fail. Self-modification cannot weaken the trusted base (I-12), mechanized end
  to end.
- The reach for **formal verification** of the microkernel core (the proof surface is small by
  construction because the TCB was kept minimal since v1).
- This is where containment is *maximal* — and it is deliberately the **last mile, not the first**,
  because reach, real workloads, and the proven userland must exist before the smallest-possible-TCB
  substrate is worth the cost.

---

## 8. One-paragraph summary

Axon OS is not a general operating system and should never try to be one. It is the substrate for
running code written by AI you don't trust — built around a *minimal trusted base that holds a
machine-checked proof of what every program can and cannot do.* We optimize for ASI because a
superhuman adversary is the only one that makes proof-based containment (rather than patch-based)
worth its cost — and an OS safe against ASI is safe against everything weaker. Software on it is
*synthesized on demand and proven-bounded*, not installed and trusted; models are *bounded userland
actors*, not part of the trusted base; memory and learning are *substrate the OS provides to a gym*,
not things the OS does itself. It runs as a container today, a micro-VM for real isolation, in the
browser for reach, and — as its north star — on bare metal as a capability microkernel with the
smallest trusted base possible. The whole codebase has been building this OS for fourteen phases;
this is the decision to say so out loud and to defend the scope that makes it real.
