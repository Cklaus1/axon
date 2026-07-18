# Tech Spec — R36: Full ASI OS (convergence of R17 + R21 + guest-kernel + R26–R31)

**Spec ID:** `R36-full-asi-os` (new platform-vision row; converges `R17-freestanding-substrate.md`,
`R21-axon-os-supervisor.md`, `R26-confidential-microvm-substrate.md` … `R31-extended-tcb-attestation.md`,
and `crates/axon-guest-kernel`)
**Status:** Draft (2026-07-18) — product/convergence PRD, not a feature spec. No code lands under this ID
until §12 Q1 (the two-kernels decision) is resolved by the founder.
**Risk class:** Structural (platform bet; multi-quarter; touches the TCB story end to end)
**Author / date:** cklaus (research agent draft), 2026-07-18

> **One-line scope:** name, converge, and finish the one product the already-shipped layers are
> circling: *run an untrusted AI-authored Axon program inside a purpose-built, capability-enforced,
> measured-and-attested operating environment, where the compiler's `@[contained]`/effect-row
> guarantees are enforced by every layer down to the hypervisor — and be honest, in writing, about
> the three places where today's stack does not yet match that sentence.*

---

### 0. Read this first: what is true today (verified 2026-07-18)

Every claim below was re-verified against code, gates, and live runs on this host — not against
spec headers (which this repo has repeatedly found stale in *both* directions,
`governance/REQUIREMENTS.md` rows R21/R22/R23/R31).

**TRUE and demonstrated end-to-end, live, on real KVM:**

- `scripts/kernel_enforce_test.sh` **PASSES on this host**: an Axon-policy-driven syscall gate
  inside a bare-metal guest kernel, booted under real Firecracker, **denies a real `openat`
  (syscall 257) when the policy withholds FS** (halt, exit 8 = SandboxViolation) and permits it
  when FS is granted. The enforcer is the guest kernel — not Linux, not seccomp.
- `axon-vm run` refuses to boot an unattested or digest-mismatched kernel image
  (`b85bd83` — attestation is mandatory in the run path, not optional).
- The host safety stack is real and gated green (all re-run 2026-07-18): R27 kill-switch/latch
  19/19, R28 capability audit ledger (post-`0bfa74d` fix: every capability-bearing builtin is
  audited, not just `ai_complete`), R29 compliance monitor 24/24 (a denied-effect entry trips the
  R27 latch), R31 extended-TCB digest (`axtcb1_ext` chains kernel + axon-os + axon-audit +
  monitor; swapping any one component changes the digest) all PASS.
- R21 `axon-os` (admission gate: declared effects ⊆ grant, fail-closed; attenuated Principal
  minting; hash-chained tamper-evident run records; deterministic replay) is 100% landed
  (`9f12499`, runtime-ceiling fix `59f84c0`).

**Gate caveats found during verification (harness, not product):**

- `scripts/r26_acceptance_gate.sh` currently **FAILS on a self-inflicted env leak**: step (6)
  `export AXON_CI_NO_KVM=1` bleeds into step (7)'s `cargo test -p axon-vm`, where
  `measure_and_attest_inner` short-circuits to `Ok` in mock mode, so
  `test_attest_fails_on_tampered_kernel` fails. With the env var unset the test passes. Fix: unset
  the var before step 7 (one line). The R26 *substance* (attestation-gated boot) is intact.
- `scripts/r30_acceptance_gate.sh` exceeds a 4-minute cap on this WSL2 host (it chains the four
  constituent gates, each with cargo builds + real KVM); the constituents pass individually.

**NOT true yet — the three honesty gaps this spec exists to close (§4):**

1. **R17 and R26 do not compose.** They are disjoint kernel mechanisms (§4.1).
2. **The AI agent does not run inside the from-scratch kernel.** The live enforcement demo's
   syscall is issued at ring 0 by the kernel at the program launch point; the ~7 MB Axon
   interpreter is not yet loaded as a ring-3 user program (§4.2).
3. **Attestation is a software stand-in.** qemu-swtpm SHA-256 measurement; no SEV-SNP/TDX memory
   encryption; the host operator is inside the trust boundary. `axon-vm attest` prints this caveat
   itself (§4.3).

---

### 1. Motivation

The repo has quietly built four layers that share one vocabulary — Principal / effect row / budget
/ audit / attestation — and each layer is individually landed and gated:

| Layer | Artifact | What it proves |
|---|---|---|
| Language | `@[contained]`, effect rows, refinement types, E1001/E131x | *statically*, what a program may do |
| Userland supervisor | R21 `crates/axon-os` | admission (effects ⊆ grant), attenuated minting, hash-chained audit, replay |
| Guest kernel | `crates/axon-guest-kernel` (K1–K5) | *live* syscall-level enforcement of the same effect bits, inside a Firecracker microVM, ~15K-LOC-class TCB instead of Linux's ~35M |
| Measurement/attestation | R26 `axon-attest`/`axon-vm` + R31 `axtcb1_ext` | the *exact* kernel + host safety stack running is the expected one, fail-closed |

No single document states the product these compose into, which kernel is *the* kernel, or what
"done" means. Meanwhile ROADMAP §2.3 — which originally killed the kernel ambition ("userland OS
forever") — was **explicitly reversed 2026-06-19** for the R17 track, on exactly this thesis:
"Principals/effects/`@[contained]` ≈ seL4 caps, SMT-proven isolation." So a Full ASI OS is
**on-thesis, not scope creep** — *provided* the pitch matches reality. Today it would not: the
three §0 gaps mean the sentence "run an AI agent on a from-scratch attested OS" is roughly
two-thirds true. R36's job is to (a) be the single source of truth for what is shipped vs.
aspirational, and (b) define the shortest slices that make the sentence fully true.

### 2. Requirement link

Opens a new `REQUIREMENTS.md` row **R36** in the platform-vision bucket (alongside R16/R17).
Headline acceptance (the sentence, made checkable):

> *An untrusted `.ax` program, admitted by `axon-os` under a declared grant, runs **as a confined
> user program inside the Axon guest kernel** in a Firecracker microVM whose kernel + host safety
> stack are **measured and attested before boot** (R26/R31); its first out-of-grant effect is
> denied **by the guest kernel's syscall gate** (exit 8) and the denial appears in the R28 audit
> ledger and trips the R29→R27 kill-switch path — all from one CLI invocation.*

Today every clause of that sentence is individually demonstrated **except** "as a confined user
program" (§4.2) — that clause is the convergence gate.

### 3. The product story (target state)

One command, five enforcement layers, one vocabulary:

```
$ axon-os run job.axjob --substrate vm --attest extended
  [gate]   declared effects {IO, FS:read(./data/)} ⊆ grant           (R21, static, fail-closed)
  [attest] kernel axtcb1:… + axon-os + axon-audit + monitor OK       (R26 + R31, fail-closed)
  [boot]   axon-guest-kernel under Firecracker, policy on cmdline    (K1–K3, <10 ms)
  [run]    program as ring-3 user under the syscall gate             (K5 — THE GAP, §4.2)
  [deny]   net_get → syscall blocked (Net ∉ policy) → exit 8         (K3, live, hardware)
  [audit]  denial in hash-chained ledger; monitor trips kill-switch  (R28 → R29 → R27)
  [replay] axon-os replay <run-id> — byte-identical                  (R21)
```

The differentiator is **not** "an OS" (the world has OSes). It is: *the policy the kernel enforces
is the same typed object the compiler proved, the supervisor admitted, and the attestation
measured* — one artifact, four independent enforcement layers, each of which fails closed alone.
No other stack offers a compiler-to-silicon capability chain for AI-authored code.

### 4. The composability crux — three gaps, named precisely

#### 4.1 Two kernels, zero overlap (R17 ⊥ R26)

**Verified fact:** R26's microVM path boots `crates/axon-guest-kernel` — **bare-metal Rust**
(`boot.s` + `#![no_std]` `main.rs`, custom `x86_64-axon-metal.json` target, default backend of
`scripts/build-guest-image.sh`) — or falls back to **Linux 6.1 microvm_defconfig** (legacy
backend, seccomp enforcement, ~35M LOC TCB). R17's Axon-language bare-metal work
(`examples/kernel/hello_kernel_slice{1,2,3}.ax`, `axon build --freestanding`, multiboot boot under
QEMU, `127811e`) is referenced **nowhere** in `crates/axon-vm`, `crates/axon-guest-kernel`, or
`build-guest-image.sh`. R26's spec §1.1 says it "reuses the R17 freestanding machinery" — that
claim is **aspirational, not implemented**. Two kernel efforts share a thesis and share no code.

This is not duplicated waste — it is a fork in the road that must be chosen (§12 Q1): either the
guest kernel is progressively rewritten in Axon *using* R17's primitives (thesis-pure: the
capability language implements its own enforcer; expensive; R17 still lacks IDT/timer wiring, true
SMP, ring transitions), or the Rust guest kernel is blessed as a permanent audited TCB component
(pragmatic: ~1K LOC today, already enforcing live; concedes "kernel written in the language" —
which `AXON_KERNEL.md`'s opening line currently overstates and should be corrected either way).

#### 4.2 The agent is not inside yet (the K5 gap)

Per `AXON_KERNEL.md`'s own honesty block (verified against `enforce.rs`/`hypercall.rs`): the
enforcement demo's `openat` is issued **by the kernel at ring 0** at the program launch point. The
GDT has no DPL-3 segments; there is no ELF loader, no cpio VFS, no user/kernel stack switching,
and the syscall surface is a handful of numbers. Running the real ~7 MB `axon` interpreter as a
confined user program — the thing the pitch describes — is unstarted. Under the Linux guest
backend the interpreter *does* run today, but then the enforcer is Linux+seccomp and the
15K-LOC-TCB claim evaporates. **You may claim small-TCB OR agent-runs-inside — not both, today.**

#### 4.3 Attestation is software-rooted

`axon-vm attest` measures with SHA-256 via a qemu-swtpm-class stand-in and prints "no memory
encryption vs the host" to stderr. No SEV-SNP report, no TDX quote; the host operator is inside
the trust boundary. R31's extended chain is real (component-swap detection works) but the root of
the chain is a file hash, not silicon. R24's SGX/Gramine work is a separate CI-only track.

### 5. What R36 is NOT (non-goals — explicit, permanent)

- **Not a POSIX-compatible general-purpose OS.** No login, no shell, no multi-user, no package
  manager, no self-hosting. One admitted Axon job per microVM, ever. The POSIX syscall numbering
  in `enforce.rs` is an interop shim, slated for inversion per `AXON_ASI_KERNEL_DESIGN.md` §1.
- **Not a driver platform.** Serial + virtio-vsock + timer is the entire device surface. No GPU,
  no networking stack of our own, no filesystem beyond an initramfs/VFS read surface for the
  interpreter. R17's own spec says it plainly: the HAL/driver layer below the language is "the
  hard 90%" — R36 does not buy that 90%; it *fences it out* of the product definition. A job that
  needs the network gets it as a typed, audited `host_await` effect over vsock — never a NIC.
- **Not a Linux fork or hypervisor.** KVM + Firecracker remain the (attested-toward) trust anchor.
- **Not "formally verified" until a machine-checked artifact exists.** R32 is spec-only (zero
  `.tla`/`.v` files in the repo); marketing may not use the phrase before R32 lands.

### 6. Phased slices (each independently shippable, gated, honest)

| Slice | Content | Gate | Closes |
|---|---|---|---|
| **S0** | Truth reconciliation: fix the R26 gate env leak (§0); correct `AXON_KERNEL.md` "written in Axon" line; strike or footnote R26 §1.1's "reuses R17" claim; add the R36 row | all six r26–r31 gates green from one clean run | spec/reality drift |
| **S1** | **Ring 3.** DPL-3 GDT segments, user/kernel stacks, TSS, ELF loader for a *static test binary* (not the interpreter), syscall gate now checks a ring-3 caller | `kernel_enforce_test` case run from ring 3, deny + permit | §4.2 (half) |
| **S2** | **Interpreter inside.** cpio VFS (read-only), the syscall surface the `axon` binary actually needs (audited allowlist, each mapped to an effect bit), interpreter runs a real `.ax` job under the gate | the §2 headline sentence, end-to-end, as `scripts/r36_acceptance_gate.sh` | §4.2 (full) — **the convergence gate** |
| **S3** | **Kernel-language decision executed** (per §12 Q1): either first Axon-language kernel module compiled via R17 primitives linked into the guest kernel (e.g. the policy parser or audit ring — smallest safety-relevant unit), or the Rust TCB is signed/pinned under the R10 multi-sig TCB discipline | golden-IR + boot gate | §4.1 |
| **S4** | **Real silicon root.** SEV-SNP or TDX attestation report replaces the swTPM stand-in on capable hardware (CI-gated like R24's SGX-DCAP); swTPM remains the dev fallback, clearly labeled | report verifies against vendor cert chain | §4.3 |
| **S5** | Product wrap: one `axon-os run --substrate vm` flow subsuming today's `axon-vm run`; `axon-web` Safety Dashboard shows the five-layer verdict for a run | A1-style smoke through the real CLI | the pitch |

S1+S2 are the critical path and the only slices that create new kernel code before the Q1
decision. S4 is hardware-gated and can proceed in parallel.

### 7. Biggest open risk / cost

**The K5 gap (S1/S2) is real systems-kernel work with no shortcut** — ring transitions, an ELF
loader, and a just-big-enough syscall surface are exactly the code where a bug is a sandbox
escape, in the component whose entire pitch is being the auditable enforcer. Budget for it like a
security-critical subsystem (spec-first, adversarial review, differential testing against the
Linux-backend behavior), not like a demo. Second-order risk: **scope gravity** — every future "can
the job just…" request pulls toward re-growing Linux; §5's non-goals are the fence, and changes to
them require the same founder-decision discipline as the §2.3 reversal. Third: the swTPM stand-in
being mistaken (by users or by our own copy) for confidential computing — every surface that
prints a measurement must carry the stand-in caveat until S4.

### 8. Error codes / exit codes

No new language codes. R36 inherits: exit 8 (SandboxViolation, guest gate), R21 exit codes
(admission/overreach/record-tamper), R26 attest-fail exits, E1001/E131x/E17xx statically. S2's
gate must assert the *same* exit-8 semantics from ring 3 that the interpreter's own sandbox and
the R21 subprocess path already use — one violation vocabulary at every layer.

### 9. Acceptance (definition of done for the R36 row)

The §2 headline sentence executes as a single pinned script (`scripts/r36_acceptance_gate.sh`,
S2), on real KVM, with: (a) a granted run completing cleanly with a verified hash-chained record,
(b) an over-reaching run denied *by the guest kernel* with exit 8 + ledger entry + tripped latch,
(c) a tampered kernel or component image refused before boot, (d) byte-identical replay. Until S2
lands, the R36 row reads "Draft — convergence spec; constituent layers landed separately" and no
external material may present the stack as one product.

### 12. Open questions

- **Q1 (founder, blocking S3, shapes S1/S2):** Is the guest kernel's destiny Axon-language (R17
  primitives, thesis-pure, expensive — and R17 needs IDT/timer + ring work it has deferred) or
  audited-Rust-TCB (pragmatic, concedes the "kernel in the language" line)? A hybrid (Rust
  bring-up + Axon safety-relevant modules) is the likely answer, but it must be *decided*, not
  drifted into — the same discipline the §2.3 reversal followed.
- **Q2:** Does S2 use the `host_await` VMCALL path (K4) for interpreter I/O, or a virtio-vsock
  channel to the R21 supervisor on the host? (K4's dispatch table is still a stub.)
- **Q3:** Which hardware does S4 target first — SEV-SNP (dev-cloud availability) or TDX? CI-only
  like R24, or is attested hardware a supported customer requirement?
- **Q4:** Does the R25 information-flow monitor (spec-only today) belong inside the R36 guest
  kernel or stay a host-side concern? (Affects the §5 device-surface fence.)
