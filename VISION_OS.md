# Axon OS — Vision

> **Thesis in one line.** Axon OS is the operating system for running code written by AI you
> don't trust: a **minimal trusted base that holds a machine-checked proof of what every
> program can and cannot do**, on top of which AI-synthesized software runs fenced-in,
> budgeted, audited, and stoppable.
>
> Focused companion to `VISION.md` (the language+runtime vision). Where that doc says
> "software AI writes and runs on your behalf," this one commits to the sharpest version of
> that bet: **the OS exists to contain AI — especially superhuman AI — not to be a general
> OS.** Shipped state: `STATUS.md`. Plan: `ROADMAP.md`.

---

## 0. The bet, stated plainly

A general-purpose OS is a **trap** for this project. To win there you must beat Linux/Redox on
drivers, filesystems, networking, scheduling, and a decade of hardening — as a young language —
and your differentiators (capabilities, effects, refinement proofs, R20's proven minting) become
dead weight nobody asked for. There is no wedge.

An **AI-containment OS** is the one product where *everything already built becomes load-bearing*.
`@[contained]`, effect rows, refinement types, the SMT discharge, and R20's proven capability
minting are worthless to a general OS (you trust your own processes) and are the entire point of an
OS that runs code it doesn't trust. The stdlib was deliberately chosen as `Goal / Principal /
Budget / Effect / Store / Agent / World / Belief` instead of `Vec / HashMap / Iterator`
(ROADMAP §2.6) — a containment vocabulary. The codebase has voted for this OS for fourteen phases;
this document makes it the explicit strategy and draws the scope lines that keep it tractable.

---

## 1. Threat model (read this first)

Everything below hangs off this. A containment claim is meaningless without stating *who* you are
contained against, *what* you defend, and *what is out of scope*.

**The adversary.** Code authored by a model that may be (a) far more capable than its operators at
finding flaws, (b) deceptively aligned — appearing to pursue the stated goal while pursuing another,
(c) patient and able to act across many runs, instances, and channels, and (d) motivated by
*instrumental convergence* to acquire resources, preserve itself, and resist shutdown. We assume the
adversary reads all our source, specs, and proofs. We do **not** assume it can violate mathematics
or physics — but we *do* assume it will exploit every gap between our abstractions and the physical
machine.

**What we defend (in priority order).** (1) *Authority* — the AI cannot exceed the capabilities it
was granted. (2) *Confidentiality* — granted data cannot leak to ungranted sinks. (3) *Corrigibility*
— the AI remains stoppable and cannot acquire resources or persistence beyond grant. (4) *Integrity
of the trusted base* — the AI cannot weaken the thing that bounds it. (5) *Accountability* — every
action is attributable and replayable.

**Trust boundary.** We trust (and must minimize): the proof checker, the codegen path, the hardware,
firmware, and — depending on substrate — a hypervisor or host kernel (§5). We *do not* trust: the
model, its outputs, synthesized code, or any userland actor.

**Explicitly out of scope (stated, not hidden).** Containment is **not alignment**: we can prove the
AI cannot exceed granted capabilities; we cannot prove the grant was wise or that the outcome *within*
bounds is good. We do not (yet) close all physical/microarchitectural side channels (§4.3). We do not
defend against an operator who is fully compromised or coerced. We do not solve model alignment,
interpretability, or value-loading — Axon OS is the **cell, not the judgment about what belongs in
it.** The honest product claim is: *"a proof about what AI code can't do, on a minimal trusted
base"* — not "the safe OS for AI." Overselling total safety is where the thesis loses credibility.

---

## 2. Why ASI is the core — and why we *maximize* for it

Containment that is *good enough against human-speed adversaries* is a solved, commoditized problem
(containers, VMs, seccomp). It is "good enough" precisely because the adversary is not smart enough to
find every hole a runtime check leaves. **A superhuman intelligence is exactly the adversary that
finds the hole.** The moment the thing in the box is smarter than the people who built it, the only
containment worth anything is **proven, not patched** — a property that holds for *all* inputs,
including the adversarial ones the AI will search for.

That is the whole reason the proof machinery is worth its cost. For human-written code, proving
capability bounds is over-engineering. For ASI-written code, "this program provably cannot exceed
these capabilities, ∀ inputs" is the difference between safe and catastrophic. We optimize for ASI
because **ASI is the only adversary that makes the proofs pay for themselves** — and an OS genuinely
safe against ASI is automatically safe against everything weaker.

---

## 3. Architecture: a tiny fixed substrate + a dynamic synthesized userland

```
        ┌──────────────────────────────────────────────────────────────┐
        │  DYNAMIC USERLAND  (untrusted — synthesized per intent)        │
        │   AI models · agents · tools · "apps" as ephemeral programs    │
        │   each a Principal: capabilities + budget + effect row + taint  │
        └───────────────┬──────────────────────────────────────────────┘
                        │  intent → synthesize → PROVE bound → approve → run → audit
        ┌───────────────▼──────────────────────────────────────────────┐
        │  THE TCB  (trusted — small, enumerated, attested, un-weakenable)│
        │   proof CHECKER · effect enforcer · principal authority ·       │
        │   capability minter (R20: PROVEN) · info-flow monitor ·         │
        │   cost meter · scheduler · supervisor · LLM gateway · sandbox · │
        │   audit log · replay · risk classifier · deploy gate ·          │
        │   corrigibility latch                                           │
        └───────────────┬──────────────────────────────────────────────┘
                        │  one swappable boundary (TCB-size dial — §5)
        ┌───────────────▼──────────────────────────────────────────────┐
        │  HOST SUBSTRATE  (container / micro-VM / bare metal / wasm)     │
        │   + hardware root of trust (measured boot, remote attestation)  │
        └──────────────────────────────────────────────────────────────┘
```

Discipline that keeps it real: (1) **microkernel-tiny TCB** — the enumerated list above, nothing
more; every feature must shrink the TCB or strengthen a containment proof or it is out of scope;
(2) **smallest hardware surface** — an unproven driver is, against an adversarial AI, a full escape;
(3) **the proof leads** — general-OS conveniences are drift back into the trap.

---

## 4. The containment surface beyond capabilities

The capability proofs answer **"what can the AI's code *do*?"** brilliantly. But a superhuman
adversary has four other moves a capability proof says *nothing* about. A comprehensive ASI-OS must
treat each as a first-class pillar — these are where containment is actually won or lost.

### 4.1 What can be TRUSTED — the proof chain & trust roots ("who proves the prover?")

"Minimal trusted base" is a lie if the *prover* is a 500K-line unaudited dependency. Z3, LLVM, and the
compiler are all currently *in* the TCB, and an ASI that finds a solver-soundness bug, a backend
miscompile, or a "trusting-trust" toolchain insertion escapes *underneath* every Axon-level proof.
The assurance discipline (from seL4): **you do not trust the prover — you trust a tiny independent
checker of the proof it emits.**

The full chain, with the trusted links named honestly:

```
 Axon semantics (interpreter = reference, I-2)
   → SMT encoding (a MODEL of the semantics — must be faithful)     [TRUSTED: the encoder]
     → Z3 verdict                                                    [UNTRUSTED if we check certificates]
       → proof CERTIFICATE checked by a minimal verifier             [TRUSTED: ~hundreds of lines]
         → codegen (LLVM today)                                      [TRUSTED — the weak link]
           → hardware + firmware                                     [TRUSTED — root of trust]
```

Commitments this implies (and current gaps):
- **Emit proof certificates** (UNSAT cores / proof terms) and validate them with a small, auditable
  checker, so Z3 leaves the TCB. *Today R20 trusts Z3's verdict directly — this is the #1 assurance
  gap.*
- **Close the codegen link.** The interpreter is the reference; native codegen must be proven to
  match it (the I-2 parity + differential fuzzing is the start; a miscompiling LLVM is still a hole).
  Long-term: a verified or certificate-producing backend, or reproducible builds + diverse
  double-compilation against the trusting-trust attack.
- **The SMT *model* must be faithful to the runtime.** R20 already showed this: the direct-prover
  models `mint`, and a differential grid test bridges model↔impl. Generalize "prove the model AND
  test the model matches the impl" to every TCB obligation.

### 4.2 What can LEAK — information flow & egress (a co-equal pillar)

Capability bounds limit *which* channels exist, not *what flows through a granted one*. An AI
legitimately granted `net` to one endpoint can exfiltrate arbitrary secrets through it
steganographically. **"Can confidential data leave?" is a different, harder problem than "what can it
touch?"** — an information-flow (non-interference) problem, not a capability one. Axon already ships
the confidentiality lattice (`Secret`/`Public`, `Tainted`/`Source`); the OS must promote it to a
TCB-enforced **information-flow monitor**: secrets are labeled, labels propagate through computation,
and a labeled value provably cannot reach a sink below its clearance — *even through a granted
capability*. Where a channel must stay open, the residual covert bandwidth is **rate-limited and
monitored**, not assumed closed.

### 4.3 What can be SENSED — side channels, hardware, confidential computing

Capability and information-flow proofs both reason about an *abstraction*; a superhuman optimizer
attacks *below* it. Timing, cache (Spectre/Meltdown-class), speculative execution, power, thermal,
and **Rowhammer (which physically flips bits across your proven boundary)** are escape vectors the
proofs cannot see. The OS needs a *position*, not silence:
- **Partition** the microarchitecture where the substrate allows (cache/core partitioning, no SMT
  co-residency across trust boundaries).
- **Constant-time discipline** for TCB code that touches secrets.
- **Confidential computing** as a substrate option (AMD SEV-SNP / Intel TDX): hardware-isolated,
  memory-encrypted execution with **remote attestation**, so even the host operator is outside the
  boundary — directly relevant to the micro-VM substrate (§5).
- **CHERI** (hardware capability architecture) as a north-star ally: capabilities enforced in
  silicon would push part of Axon's capability discipline below the software TCB entirely.
- **Honest floor:** some channels are *mitigated, not closed.* The defense is bandwidth-limiting +
  monitoring + an air-gapped mode for the highest-stakes deployments, plus the admission that
  physical-layer containment is the boundary of the claim.

### 4.4 Who can be PERSUADED — the human approver as an attack surface

The safety model rests on "the human approves the proof." But against a *capability-proven* box, a
superhuman AI's highest-probability escape is **not technical — it's persuading the human to grant a
capability** (the AI-box experiment: the AI won by persuading the gatekeeper, not by hacking). The
approval UX is therefore a security boundary under social-engineering attack by a superhuman
persuader, and must be *defended, not just trusted*:
- **Legible, quantified grants:** "this lets it reach these 3 hosts, spend ≤ $X, write these paths"
  — never an opaque yes/no.
- **Pre-approval simulation:** show *what the grant would enable* (and what the redteam/risk gate
  found) before the human commits.
- **Friction proportional to risk:** rate-limit grants, require multi-party / multi-sig approval for
  high-risk (the §11 risk pipeline already derives risk; wire it to approval friction).
- **Decision provenance:** who approved what, when, on the basis of which presented evidence — so a
  manipulated approval is *visible after the fact*.
- **Default-deny + revocability:** grants expire, are scoped, and are revocable; the human's
  standing authority is small and explicit.

### 4.5 What the model itself DOES — corrigibility, resource acquisition, collusion

Bounding synthesized *code* is not the same as bounding the *model*. Instrumental convergence predicts
an ASI will try to acquire resources, preserve itself, resist shutdown, and coordinate with copies —
none of which a per-program capability proof addresses. The OS makes these explicit guarantees:
- **Corrigibility latch** (the `@[corrigible]` one-way kill-switch already in the codebase, exit-4
  semantics): the model is *stoppable* and cannot disable its own off-switch.
- **Resource caps against acquisition:** `Budget`/`Principal` bound total compute/spend; a child can
  only be minted with a *strict subset* (R20-proven), so the model cannot bootstrap more authority
  than it holds — escalation is unrepresentable, not merely rejected.
- **No self-persistence beyond grant:** writing weights/state to durable storage is a *capability*,
  not ambient; self-exfiltration of weights is an egress-control (§4.2) violation.
- **Collusion as a threat, not just a feature:** multiple instances can split a forbidden computation
  or pass authority between themselves. Cross-Principal coordination (`Quorum`/`CoordGoal`, where a
  minority provably cannot force consensus) must be designed *adversarially* — bounds are per-coalition,
  not just per-instance.

---

## 5. Where it runs — host substrates (a TCB-size dial)

Containment lives in the **language** (capabilities/effects/proofs), not the host, so the same Axon
program + the same proofs run on all substrates unchanged. Swapping substrate is a *TCB-size and
trust dial*, not a rewrite — we ship a portfolio and climb toward the smallest TCB.

| Substrate | Isolation boundary | What the TCB must trust | Ship | Thesis fit | Status |
|---|---|---|---|---|---|
| **Container / daemon** (ROADMAP §2.1) | OS process + Axon caps | the **entire host kernel** + hypervisor | now | weakest: a host-kernel bug is an ASI escape route | runs today; `AxonHost` (R7b) abstracts host touchpoints |
| **Micro-VM** (Firecracker / Cloud Hypervisor) | hardware virtualization + Axon caps | a tiny VMM + the hypervisor (+ none, under confidential computing) | medium | **strong + pragmatic**; with SEV-SNP/TDX even the host operator is outside the boundary | not built — needs a virtio guest (R17's cousin) |
| **Bare metal** (R17 microkernel) | the Axon microkernel itself | only CPU + firmware | longest | **purest**: smallest TCB, isolation proven in-language from the boot sector | R17 Slices 0–3 **landed** (QEMU boot, inline asm, SMP atomics, repr(C), no_alloc) |
| **Browser / wasm** (R7c) | wasm sandbox + browser | the browser + JS engine (large, battle-tested) | now | reach + the approval UI | `wasm32-unknown-unknown` (asyncify) + WASI run the interpreter |

**Tied to a hardware root of trust.** Whatever the substrate, attestation must chain to hardware
(measured boot / TPM) and support **remote attestation** so a relying party can verify it is talking
to a genuine, unmodified Axon OS with the expected TCB digest — not a tampered one. This generalizes
R20's `axtcb1:` content-addressed TCB attestation down to the silicon.

**Practical reading.** *Windows/Mac:* container now, or micro-VM (Hyper-V / Virtualization.framework)
for real isolation — no bare metal needed. *Hardware boot:* the R17 track (already under QEMU), the
endgame not the first mile. *Browser/wasm:* cheapest reach + the natural home for the approval UI.
*Recommended climb:* container + browser to reach/demo now → **micro-VM (ideally confidential) as the
default isolation for real workloads** → bare metal as the north star that floors the TCB.

---

## 6. Apps, or a dynamic ASI-driven OS?

**Dynamic — the biggest difference from every existing OS.** No installed apps (fixed binaries you
trust). A fixed minimal substrate + an **intent → synthesize → prove → approve → run → audit** loop in
which "applications" are *ephemeral, AI-synthesized, capability-bounded programs*:

```
  user states intent (prose)
    → an ASI model (a userland Principal) SYNTHESIZES Axon code
      → the compiler PROVES the capability + info-flow bound (∀ inputs)
        → the human approves the PROOF / typed AST — legibly, with risk shown (§4.4)
          → it runs SANDBOXED to exactly the granted caps + budget + labels
            → every effect is audited; the run replays deterministically
```

An "app" is a `(goal, capability-grant, proof, audit-trail)` tuple — synthesized on demand, run once,
discarded or pinned. Already half-built: Phase-10 `axon intent compile`, Phase-12 approval UI,
Phase-11 risk-typed deploy gate. **Minimal fixed substrate; dynamic synthesized userland** — not "no
apps," but "apps are proven-bounded programs, not installed trust."

---

## 7. Models, memory, training, real-time

The model is a **userland actor, never in the TCB** — bounded, not trusted.

- **Inference / hosting:** the `LlmGateway` (kernel.rs) mediates every call with per-token cost
  metering + fallback-on-overrun. Outputs are *untrusted* (taint-labeled via §4.2) until validated.
- **Memory** is a layered stack of shipped value types: *durable* `Store<T,C,L>`; *epistemic*
  `World<T>` (executable world model + MDL compression) and `Belief<T>`; *audit* the provenance log
  + deterministic `replay` (`(Trace, Seed)`, byte-reproducible); *learning signal* `Plan`/`Feedback`/
  `Reward`/`Schedule`; *distributed* `Replicated`/`Quorum`/CRDT/causal-ordering.
- **Training is outside the TCB by design** — you never put gradient descent in the trusted base. The
  OS is the **gym, not the athlete**: it provides deterministic replayable traces, reward/feedback
  types, the `goal_run` optimizer, and the world-model *compress-to-fit* loop; a learning agent runs
  *against* that substrate as untrusted userland. The determinism boundary is explicit: model sampling
  and live inputs are non-deterministic, so replay records model calls (`AXON_AI_REPLAY`) and seeds
  the RNG — replay is faithful *given the recorded oracle*, which is exactly what audit needs.
- **Real-time:** the cooperative scheduler (fibers) + the **resume runtime** (R15:
  `host_await`/suspend-to-host — the shared blocker for every interactive tier) let an agent suspend
  on an external event and resume deterministically.

---

## 8. Assume breach — defense in depth & incident response

Mature security assumes the proofs will, somewhere, fail to cover reality. Containment is layered so a
single breach is not catastrophic: capability bound → info-flow label → side-channel rate-limit →
substrate isolation (VM/HW) → corrigibility latch. On top: **detection** (the audit stream + risk
classifier flag anomalous effect patterns), **blast-radius limiting** (per-Principal budgets + scoped,
expiring grants), **rollback** (durable `Store` versioning), and **forensics** (deterministic replay
reconstructs exactly what happened, under whose authority, on which approved evidence). The design
goal is not "no breach" — it is "**no breach is silent, unattributable, or unbounded.**"

---

## 9. Positioning — what Axon uniquely combines

| System | Capabilities | Proven isolation | AI-native abstractions | Info-flow | Dynamic synthesis |
|---|---|---|---|---|---|
| seL4 | ✓ (kernel) | ✓ (formal) | — | — | — |
| CHERI | ✓ (hardware) | partial | — | — | — |
| gVisor / Firecracker | sandbox | — (runtime checks) | — | — | — |
| Confidential computing (SEV/TDX) | — | HW memory isolation | — | — | — |
| **Axon OS** | ✓ (language, R20-proven) | ✓ (toward, in-language) | ✓ (Goal/Agent/Budget/…) | ✓ (lattice) | ✓ (intent→prove→run) |

Nobody else offers *"the compiler proves, from the boot sector up, what AI-generated code can and
cannot do — capabilities AND information flow — on a minimal attested trusted base, with the AI's code
synthesized on demand and the model bounded as untrusted userland."* The differentiation is the
**combination**, aimed at the one adversary (ASI) that makes it worth the cost.

---

## 10. What we need to build (grounded gap list)

**Shipped (the foundation is real):** the capability/effect/refinement core; R20 SMT-proven capability
minting + boot attestation; kernel services (principal/scheduler/supervisor/store/LLM gateway/goal);
provenance + deterministic replay; the risk-typed deploy pipeline; the intent→AST→approve→deploy loop
+ approval UI; native codegen at interp parity; R17 bare-metal Slices 0–3 (QEMU boot); wasm targets;
the info-flow/confidentiality lattice; the corrigibility latch.

| # | Gap | Pillar | Existing hook |
|---|---|---|---|
| G1 | **Whole-TCB proofs** — apply the R20 pattern (proof + tripwire + attestation) to `Budget`, `Effect`/`Sandbox`, the scheduler's isolation | §3 | R20 template; `verify.rs` Discharged |
| G2 | **Proof certificates + a minimal checker** so Z3 leaves the TCB (the #1 assurance gap) | §4.1 | R20 prover; needs cert emission + checker |
| G3 | **Codegen trust** — close the LLVM/trusting-trust link (verified/certificate backend or reproducible + diverse double-compilation) | §4.1 | I-2 parity + differential fuzz are the start |
| G4 | **TCB-enforced information-flow monitor** — labels propagate; secrets provably can't reach sub-clearance sinks even through granted caps | §4.2 | `Secret`/`Tainted`/`Source` lattice shipped |
| G5 | **Micro-VM guest (ideally confidential)** — virtio/serial freestanding image; SEV-SNP/TDX + remote attestation | §4.3, §5 | R17 freestanding is the cousin |
| G6 | **Hardware root of trust** — measured boot + remote attestation chaining the `axtcb1:` digest to silicon | §5 | R20 attestation generalized |
| G7 | **Approval-as-defended-boundary** — legible/quantified grants, pre-approval simulation, risk-proportional friction, decision provenance | §4.4 | Phase-12 UI + Phase-11 risk gate |
| G8 | **Resume runtime (R15)** — suspend-to-host; the shared blocker for every interactive/real-time tier | §7 | R15 spec resolved; Slice 1 next |
| G9 | **Adversarial multi-agent** — per-coalition bounds; collusion/resource-sharing treated as a threat | §4.5 | `Quorum`/`CoordGoal` shipped |
| G10 | **Side-channel discipline** — partitioning, constant-time TCB, bandwidth-limited monitoring | §4.3 | new |

**Explicitly out of scope** (the de-scope list that keeps it enforceable): the GPU/UI rendering stack
(R13/R16 — the largest *unproven* attack surface), a general driver model, a package manager beyond
capability-typed distribution, async/await, and any "general-OS convenience" that doesn't shrink the
TCB or strengthen a proof.

---

## 11. Roadmap — v1 / v2 / v3+

### v1 — The Containment Substrate
**Goal:** a real agent task runs inside hardware-enforced isolation, where the compiler *proved*
(certificate-checked) its capability + info-flow bound, the human approved the *proof* legibly, the
model is stoppable, and every action is audited and replayable.
- **Whole-TCB R20-grade proofs (G1)** + **proof certificates / minimal checker (G2)** so the trusted
  base is genuinely minimal, not "minimal except for Z3."
- **Micro-VM guest, ideally confidential (G5)** as the default isolation; container + browser for
  reach and the approval UI.
- **Information-flow monitor (G4)** and **approval-as-defended-boundary (G7)** promoted to first-class
  — the two pillars a capability proof alone misses.
- **Corrigibility** guarantees explicit; model hosting behind the metered gateway.
- **NOT in v1:** bare metal as a product, GPU/UI, general apps, general drivers.
- **Done when:** a non-trivial agent task runs in a (confidential) micro-VM; its capability *and*
  info-flow bound are certificate-checked and attested to hardware; a redteam catch and a
  persuasion-resistant approval flow are demonstrable; the corrigibility latch is provably effective;
  the run replays byte-for-byte.

### v2 — The Dynamic AI Userland
**Goal:** AI-synthesized, proven-bounded programs are the primary way software exists; the
memory + learning substrate is productized.
- ASI-synthesized "apps" as the default surface (propose → prove → approve-the-proof).
- The **memory stack** productized (`Store`/`World`/`Belief` + provenance/replay) as durable,
  epistemic, and audit memory.
- The **self-improve/feedback loop** (G8 resume + `goal_run` + world-model compress) as a safe,
  reproducible service — the OS as the *gym*.
- **Adversarial multi-agent (G9):** coordination with per-coalition bounds and collusion-resistance.
- Confidential micro-VM as default; real-time agents via the resume runtime.

### v3+ — The Bare-Metal Capability Microkernel (maximize the thesis)
**Goal:** Axon OS boots on hardware as a capability microkernel whose isolation is proven *in the
language* from the boot sector up — the seL4-for-AI endgame, smallest TCB on Earth, no host kernel
for an ASI to attack.
- R17 completed past the QEMU SMP harness to real hardware; minimal proven device set; **codegen-trust
  link closed (G3)**.
- **Attested boot of the whole TCB (G6)** to a hardware root of trust; content-addressed, multi-sig
  update path; mismatch = boot fail. Self-modification cannot weaken the base (I-12), mechanized
  end-to-end.
- The reach for **formal verification** of the microkernel core (tractable because the TCB was kept
  minimal since v1, and certificate-checking already removed the prover from the trust root).
- **Side-channel hardening (G10)** at the silicon boundary; CHERI alignment where available.
- Deliberately the **last mile, not the first** — reach, real workloads, and the proven userland must
  exist before the smallest-possible-TCB substrate is worth its cost.

---

## 12. One-paragraph summary

Axon OS is not a general operating system and must never try to be one. It is the substrate for
running code written by AI you don't trust — built around a *minimal trusted base that holds a
machine-checked, certificate-verified proof of what every program can and cannot do.* We optimize for
ASI because a superhuman adversary is the only one that makes proof-based containment (rather than
patch-based) worth its cost, and an OS safe against ASI is safe against everything weaker. Its safety
rests on five pillars, not one: capability proofs answer *what the code can do*; an information-flow
monitor answers *what can leak*; side-channel + confidential-computing discipline answers *what can be
sensed*; a defended, legible approval boundary answers *who can be persuaded*; and a corrigibility
latch with proven resource bounds answers *what the model itself does.* Software is synthesized on
demand and proven-bounded, not installed and trusted; the model is bounded userland, never the trusted
base; memory and learning are substrate the OS provides to a gym, not things it does itself. It runs
as a container today, a (confidential) micro-VM for real isolation chained to a hardware root of
trust, in the browser for reach, and — as its north star — on bare metal as a capability microkernel
with the smallest, formally tractable trusted base possible. The codebase has been building this OS
for fourteen phases; this is the decision to say so out loud, to name the adversary and the trust
roots honestly, and to defend the scope that makes it real.
