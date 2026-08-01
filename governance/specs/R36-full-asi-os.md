# Tech Spec — R36: Full ASI OS (convergence of R17 + R21 + guest-kernel + R26–R31)

**Spec ID:** `R36-full-asi-os` (new platform-vision row; converges `R17-freestanding-substrate.md`,
`R21-axon-os-supervisor.md`, `R26-confidential-microvm-substrate.md` … `R31-extended-tcb-attestation.md`,
and `crates/axon-guest-kernel`)
**Status:** Draft (2026-07-18; header reconciled 2026-07-31) — product/convergence PRD, not a
feature spec. No **S3** code lands until §12 Q1 (the two-kernels decision) is resolved by the
founder; S0–S2 may proceed (Q1 *shapes* S1/S2 but does not block them — matching spec-meta and §6,
which the original "no code lands under this ID until Q1" header wording contradicted). S1/S2's
ring-3/ELF-loader work in the Rust guest kernel survives either Q1 outcome: even the thesis-pure
answer is a *progressive* Axon rewrite of that same kernel (§12 Q1's likely hybrid), and the
ring-transition/loader mechanism design is independent of the kernel's implementation language.
**Risk class:** Structural (platform bet; multi-quarter; touches the TCB story end to end)
**Author / date:** cklaus (research agent draft), 2026-07-18

```spec-meta
id: R36-full-asi-os
status-claim: Draft
depends-on: R17-freestanding-substrate, R21-axon-os-supervisor, R26-confidential-microvm-substrate, R27-corrigibility-resource-bounds, R28-capability-audit-ledger, R29-continuous-compliance-monitor, R30-unified-safety-gate, R31-extended-tcb-attestation
blocks: none
blocked-by: R36 §12 Q1 (founder two-kernels decision — blocks S3, shapes S1/S2)
supersedes: none
related: R37-nano-micro-asi-kernel, R24-tee-target, R25-information-flow-monitor, R32-formal-corrigibility-proof, R16-axon-ui, R18-provenance-ledger, R38-embedded-agent-runtime, R22-intent-approve-gateway, R24-defended-approval-boundary, R20-smt-capability-proofs, R23-proof-certificates
conflicts-with: none
reserves: none (inherits exit 8 SandboxViolation, R21/R26 exit codes, E1001/E131x/E17xx)
evidence: scripts/r36_acceptance_gate.sh (planned, lands with S2); until then constituent gates per §0: scripts/r26_acceptance_gate.sh … scripts/r31_acceptance_gate.sh + scripts/kernel_enforce_test.sh
```

(Edge notes: `depends-on` lists the eight landed layers this spec *converges* — see §1's layer
table; `related` holds the competing platform fronts (§12 Q1 of R38 / R18) and the sibling kernel
track R37, whose §12 Q2 proposes converging the guest kernel's gate onto R37's effect-native ABI.)

> **One-line scope:** name, converge, and finish the one product the already-shipped layers are
> circling: *run an untrusted AI-authored Axon program inside a purpose-built, capability-enforced,
> measured-and-attested operating environment, where the compiler's `@[contained]`/effect-row
> guarantees are enforced by every layer down to the hypervisor — and be honest, in writing, about
> the places where today's stack does not yet match that sentence (three roadmap gaps, §4.1–§4.3,
> plus two guarantee gaps found 2026-07-31, §4.4–§4.5).*

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

**NOT true yet — the honesty gaps this spec exists to close (§4). The three below are *roadmap*
gaps (work not yet done); §4.4/§4.5 add two *guarantee* gaps found 2026-07-31 (claims the shipped
code does not support — cheaper to close, and more misleading while open):**

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

Today every clause of that sentence is individually demonstrated **except two** (corrected
2026-07-31 — the first same-day fold-in miscounted this as one): (1) "as a confined user program"
(§4.2), and (2) *the guest-kernel denial reaching the R28 ledger / R29→R27 latch* — today the
guest's syscall-gate violations land only in an **in-guest** audit ring buffer
(`crates/axon-guest-kernel/src/enforce.rs`); no code forwards a guest exit-8 or its violation
record into the host-side hash-chained R28 JSONL ledger that R29 watches (`axon-vm`'s `chain.rs`
is the per-run boot-measurement chain, not the R28 capability ledger, and the §0 R28/R29
demonstrations are host/interpreter-level denials). Both clauses close in S2, whose content now
names the guest-ring→host-ledger relay explicitly (§6); together they are the convergence gate.

**Policy-provenance invariant (added 2026-07-31 — stated because the tree does not yet satisfy
it).** The bit-vector the guest kernel enforces MUST be derived from the axon-os *effective* grant
(the admitted job's `declared ∩ grant`), never from the program's own self-declaration, and its
absence or malformation MUST be a boot refusal. Today the derivation runs the other way and fails
*open* at four points, all verified in the tree:

1. `crates/axon-vm/src/main.rs` (~L886) — `AXON_VM_ALLOWED_EFFECTS` is documented as "tightens the
   policy beyond the manifest" but *replaces* it; nothing checks the forced set is a subset.
2. same file, `load_manifest()` (~L2188) — the policy source is a `.axmeta` sidecar next to the
   program, emitted by `axon build --emit-manifest` from the program's OWN declared effect rows;
   it is unsigned, unmeasured, and not bound to the approval token or the attested chain. A missing,
   unreadable, or malformed sidecar yields `AxonManifest::default()` (`unwrap_or_default()`), i.e.
   `effect_union: None`.
3. `crates/axon-guest-kernel/src/mmds.rs:41` — `ALLOWED_EFFECTS = EffectSet(0xFF)` ("open by
   default"), with `set_open_policy()` called on three separate cmdline paths (no boot_params, no
   cmdline pointer, `axon.policy=` tag absent) and `parse_effects` returning `0xFF` when the field
   is absent or unparseable.
4. same file, `read_policy()` (~L159) — returns `EffectSet(0xFF)` when `POLICY_READY` is false.

So the sequence *sidecar missing/corrupt → no principal → no `axon.policy=`* boots a guest with
every effect bit set and a green-looking run. A capable adversary does not attack the syscall gate
(which works); it attacks policy *provenance*. Closing these four defaults is **S0** work (one line
per site), not S2 — it is the difference between §0's fail-closed claim being true and being
aspirational — and §9 clause (f) gates each path negatively.

**Approval invariant (added 2026-07-31).** ROADMAP §2.2/§2.4 are locked: the human approves the
typed AST, and "the English is a comment; the AST is the contract". `crates/axon-os/src/approval.rs`
already implements the enforcement side (`verify_approval` checks an `axon-approval/1` token binding
the program digest *and* the canonical grant digest, via the deliberately single-sourced
`canonical_grant`). R36's flow must run it: a job that is unapproved, or whose program or grant was
edited after approval, is refused **before boot** (§9 clause (e)). See §5's authorship limit for why
this matters more, not less, as authorship becomes machine-scale.

### 3. The product story (target state)

One command, five enforcement layers, one vocabulary:

```
$ axon-os run job.axjob --substrate vm --attest extended
  [approve] token binds program digest + canonical grant digest      (R22, fail-closed)
  [gate]   declared effects {IO, FS:read(./data/)} ⊆ grant           (R21, static, fail-closed)
  [attest] kernel axtcb1:… + axon-os + axon-audit + monitor OK       (R26 + R31, fail-closed)
  [boot]   axon-guest-kernel under Firecracker, policy on cmdline    (K1–K3, <10 ms)
  [run]    program as ring-3 user under the syscall gate             (K5 — THE GAP, §4.2)
  [deny]   net_get → syscall blocked (Net ∉ policy) → exit 8         (K3, live, hardware)
  [audit]  denial in hash-chained ledger; monitor trips kill-switch  (R28 → R29 → R27)
  [replay] axon-os replay <run-id> — byte-identical                  (R21)
```

The differentiator is **not** "an OS" (the world has OSes). It is: *the same typed policy object
travels the whole chain — the compiler proves it, the supervisor admits against it, the kernel
enforces it, the attestation measures it.*

**Precision required here (corrected 2026-07-31 — the earlier wording "one artifact, four
independent enforcement layers, each of which fails closed alone" was false against the tree).**
The four layers are independent *in code*, but they are not independent in **resolution**: layers
2–4 enforce a 4-to-6-bit boolean *projection* of the policy, and the security content that
distinguishes one grant from another — path prefixes and host allowlists — has exactly **one**
enforcement point. So the honest claim is *layered enforcement of the effect **axes***, plus
single-point enforcement of the *within-axis* allowlists (§4.4). No other stack offers a
compiler-to-silicon capability chain for AI-authored code — but the chain's granularity narrows as
it descends, and that narrowing must be stated wherever the chain is pitched.

### 4. The composability crux — five gaps, named precisely

(§4.1–§4.3 are the original three §0 honesty gaps. §4.4/§4.5 were added 2026-07-31 after an
ASI-trajectory review traced §3's "four independent enforcement layers" claim through the tree;
they are gaps in the *guarantee*, not in the roadmap, and are cheaper to close than §4.1–§4.3.)

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
capability language implements its own enforcer; expensive — though less so than this draft first
estimated: R17's IDT/PIC/PIT timer-interrupt path **landed and was QEMU-verified 2026-07-20**
(`scripts/timer_irq_qemu_test.sh`, `r17_timer_interrupt_fires_and_is_handled`) — caveat
(2026-07-31): that script SKIPs with exit 0 on an interpreter-only `axon` build, so its green is
only evidence on a host that actually built codegen (re-run live with codegen 2026-07-31: PASS,
182 timer ticks, 0 trap markers, corroborating the R17 spec's 194-tick record); if this gate is to
be load-bearing for the Q1 cost decision, its cargo mirror should distinguish SKIP from PASS
(same skippable-gate class as the `qemu_boot_test.sh` stale-skip incident) — leaving only ring
transitions and the true 2-core SMP harness deferred; cost framing corrected 2026-07-31), or the
Rust guest kernel is blessed as a permanent audited TCB component
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

#### 4.4 The lower three layers enforce a coarse boolean projection of the policy

**Verified fact (2026-07-31):** `crates/axon-os/src/grant.rs` reduces a `Grant` — which *does*
carry `fs_read: Vec<PathPrefix>`, `fs_write`, `net: Vec<Host>` — to
`EffectSet { fs_read, fs_write, net, exec }` **booleans** via `effect_set()` ("an axis is present
iff its allowlist is non-empty"). `gate.rs::admit` compares only those booleans against
`DeclaredEffects.row`, which is itself only booleans: **no path or host is ever compared at
admission**. Below that, `crates/axon-guest-kernel/src/enforce.rs::syscall_dispatch(nr)` gates on
the syscall *number* only — it takes no arguments — so the FS bit permits `openat`/`unlinkat`/
`mkdirat` against **any** path in the guest VFS and the Net bit permits `connect` to **any** host.

Net result: `@[contained](fs: [read("./data/")], net: ["api.example.com"])` is enforced at exactly
one layer — the compiler's static check plus the interpreter's runtime sandbox. Layers 2–4 cannot
backstop it; they can only answer "FS at all / Net at all". Against a careless program this is
invisible. Against a program *optimizing* for an out-of-grant effect it is the whole game: defeat
or bypass the single fine-grained layer (an interpreter bug, `native::` FFI handles, `host_await`,
or simply the fact that after S2 the confined artifact is a ~7 MB ELF whose post-load behavior no
`.ax`-level check constrains) and the remaining three layers permit reading `/etc/*` and dialing
arbitrary hosts. This is the static-check-vs-runtime-enforcement gap in its most concrete form.

Two honest resolutions, and R36 picks (b) now with (a) as the S2-stretch:

- **(a) Make the claim true.** The guest syscall gate becomes argument-aware — path-prefix checking
  on `openat`/`unlinkat`/`mkdirat`, host checking on `connect` — and S2's gate adds a case where the
  grant is `fs_read=["./data/"]`, the program opens `/etc/passwd`, and **the denial comes from the
  guest kernel**. This is what the §3 pitch actually promises.
- **(b) Scope the claim.** §3 claims layered enforcement of the effect *axes*, this section stands
  as a named gap, and every external surface that says "defense in depth" says at what granularity.

#### 4.5 Attestation covers the enforcers, not the enforced

**Verified fact (2026-07-31):** `axon-vm` calls `measure_and_attest(&kernel_path, no_attest)` — the
**kernel image only**; `initrd_path` is merely existence-checked (`main.rs` ~L831/L840). R31's
`axtcb1_ext` chains four components (kernel + axon-os + axon-audit + monitor, per
`axon-attest/src/lib.rs` `CANONICAL`, with a structural `components.len() != 4` check) — every one
of them an **enforcer**, none of them the **enforced artifact**. But S2's entire content is "load
the ~7 MB `axon` interpreter out of a cpio initramfs and jump to it": that binary is unmeasured, and
so are the `.ax` job and the grant. The guest is even *handed* the program hash and throws it away —
`crates/axon-guest-kernel/src/mmds.rs:13`: `pub source_hash: Option<&'a str>, // unused at kernel
level`, and `read_policy()` hardcodes `source_hash: None`.

This is distinct from §4.3 and is **not** fixed by S4. After S4 ships you would have a
hardware-rooted measurement of a kernel that then loads an unmeasured 7 MB interpreter and an
unmeasured job — and substituting or patching an in-initramfs binary is strictly cheaper than
defeating SEV-SNP. §9(c) previously read as if it covered this; it now does not overclaim. **S2 owns
the fix** (cheap, all machinery landed): measure the initramfs; extend `axtcb1_ext` to include the
interpreter image and the job source hash; have the S2 ELF loader verify the loaded image against a
digest carried in the (attested) policy — i.e. finally *use* the `source_hash` field the policy
already transports.

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
- **Not "formally verified" until a machine-checked artifact exists *for the component being
  claimed*.** (Corrected 2026-07-31 — the original premise "R32 is spec-only, zero `.tla`/`.v`
  files in the repo" went stale the day it was written: R32 **landed 2026-07-18**, `98db0f9` —
  `governance/proofs/R27KillSwitch.tla` + `R27Corrigibility.v` are genuinely machine-checked,
  `scripts/r32_acceptance_gate.sh` 20/20 PASS per `governance/REQUIREMENTS.md` R32.) R32's proofs
  cover the **R27 kill-switch/corrigibility invariants only** — not the guest kernel, not the
  OS/attestation stack. "Formally verified" remains off-limits for the kernel/OS/attestation
  surfaces until a machine-checked artifact covering *those* components exists (nothing currently
  plans one); the phrase may be used, precisely scoped, only for the R27 invariants R32 proves.

### 5.1 Stated limits that expire (added 2026-07-31)

These are not non-goals — they are assumptions R36 currently *depends* on, recorded here as limits
so they are re-decided deliberately rather than discovered when they break. Each names the code that
embodies it.

- **The grant is the last human-authored artifact, and R36 assumes a human authors it.** On the
  target flow everything else is machine-derived: the program is generated, the `.axmeta` is derived
  from the program, the guest syscall hints are derived from the manifest. The grant — and the
  approval token binding it (§2) — is the only remaining human input. R36 therefore **depends on a
  human reviewing every admitted job's grant**, and that is a *stated limit*, not a property of the
  design. It expires at generated-job volume. §12 Q5 carries the unresolved half (may a generator
  author or propose its own grant, and under what bound).
- **"One admitted Axon job per microVM, ever" (§5) plus a single global one-way kill latch is a
  demo-scale coupling.** `crates/axon-os/src/latch.rs` is one supervisor-owned, one-way, idempotent
  latch, and `monitor.rs::trip_kill_file` trips it on **any** violating ledger entry. At one job a
  denial is an incident and halting is correct. At the job volume machine authorship implies,
  denials become routine exploratory behavior, and one job's out-of-grant attempt halts the entire
  supervisor for every other job. The predictable operator response is to scope the monitor out or
  disable the latch — the guarantee gets turned off in production, which is worse than never having
  claimed it. **S2 must specify latch SCOPE explicitly** (per-principal or per-job-class quarantine
  vs. global halt) and §2/§9 must state the intended steady-state denial rate. *The latch itself is
  not weakened by this: scoping its blast radius is what keeps it enabled.*

### 6. Phased slices (each independently shippable, gated, honest)

| Slice | Content | Gate | Closes |
|---|---|---|---|
| **S0** | Truth reconciliation: fix the R26 gate env leak (§0); correct `AXON_KERNEL.md` "written in Axon" line; strike or footnote R26 §1.1's "reuses R17" claim; ~~add the R36 row~~ (**done** — `governance/REQUIREMENTS.md` carries the R36 row with §9's "Draft — convergence spec; constituent layers landed separately" framing; the other three items verified still outstanding 2026-07-31: env leak present at `r26_acceptance_gate.sh` step 6, `AXON_KERNEL.md` "written in Axon" line uncorrected, R26 §1.1 unfootnoted); **plus (added 2026-07-31) close the four fail-open policy-provenance defaults enumerated in §2** — `AXON_VM_ALLOWED_EFFECTS` must be checked as a subset rather than a replacement, a missing/malformed `.axmeta` must refuse rather than `unwrap_or_default()`, and `mmds.rs`'s `set_open_policy()`/`read_policy()` `0xFF` defaults must become refuse-to-boot. One line per site; belongs here, not S2, because until it lands §0's "fail-closed" is aspirational | all six r26–r31 gates green from one clean run, **plus the four negative provenance cases of §9(f)** | spec/reality drift + fail-open policy provenance |
| **S1** | **Ring 3.** DPL-3 GDT segments, user/kernel stacks, TSS, ELF loader for a *static test binary* (not the interpreter), syscall gate now checks a ring-3 caller | `kernel_enforce_test` case run from ring 3, deny + permit | §4.2 (half) |
| **S2** | **Interpreter inside.** cpio VFS (read-only), the syscall surface the `axon` binary actually needs (audited allowlist, each mapped to an effect bit), interpreter runs a real `.ax` job under the gate; **plus the guest-denial→host-R28-ledger relay** (guest audit ring → vsock/MMDS channel → host R28 ledger append → R29 monitor pickup — added 2026-07-31: without this bridge, which no slice previously owned, S2 could not pass its own gate, since the §2 headline requires the guest kernel's denial to appear in the R28 ledger and trip the R29→R27 latch); **added 2026-07-31, all cheap and all using landed machinery:** (i) measure the initramfs + interpreter image + job source hash into `axtcb1_ext` and have the ELF loader verify the loaded image against the policy's `source_hash` field (§4.5); (ii) run `verify_approval` before boot (§2 approval invariant); (iii) specify latch SCOPE and the intended steady-state denial rate (§5.1); (iv) state the guest substrate's **determinism obligations** — what the interpreter may observe (seeded RNG, no wall clock, ordered vsock, recorded AI calls) — since §9(d) demands byte-identical replay of a run that crosses vsock/`host_await`, a timer, and a `getrandom` path; (v) **stretch:** argument-aware syscall gating per §4.4(a) | the §2 headline sentence, end-to-end, as `scripts/r36_acceptance_gate.sh` — **including the negative and population clauses §9(e)/(f)/(g)** | §4.2 (full) + §4.5 + the §2 denial-export/approval clauses — **the convergence gate** |
| **S3** | **Kernel-language decision executed** (per §12 Q1): either first Axon-language kernel module compiled via R17 primitives linked into the guest kernel (e.g. the policy parser or audit ring — smallest safety-relevant unit), or the Rust TCB is signed/pinned under the R10 multi-sig TCB discipline | golden-IR + boot gate | §4.1 |
| **S4** | **Real silicon root.** SEV-SNP or TDX attestation report replaces the swTPM stand-in on capable hardware (CI-gated like R24's SGX-DCAP); swTPM remains the dev fallback, clearly labeled | report verifies against vendor cert chain | §4.3 |
| **S5** | Product wrap: one `axon-os run --substrate vm` flow subsuming today's `axon-vm run`; `axon-web` Safety Dashboard shows the five-layer verdict for a run — **the dashboard must render the §4.4 granularity honestly** (which axes are enforced at which layers), not five uniform green ticks | A1-style smoke through the real CLI | the pitch |
| **S6** | **Proof transport (scoped opportunity, added 2026-07-31 — not aspiration: every input has landed).** Today each layer contributes a hash or a boolean; *nothing checkable crosses a layer boundary*. R20 SMT-proves the mint/attenuation contract for all inputs and is wired into the default pipeline's `Discharged` set; R23 landed 100% (`crates/axon-certcheck`, `scripts/r23_acceptance_gate.sh` PASS) precisely so a prover's verdict becomes a certificate a few-hundred-line **solver-free** checker validates ("Z3 out of the TCB"). S6 is not a kernel proof — it is proof *transport*: the axon-os admission verdict (`declared ⊆ effective grant`) is emitted as an R23 certificate, its digest is bound into the boot policy and the `axtcb1_ext` chain, and the guest validates it with `axon-certcheck` before jumping to the loaded program. This converts "our supervisor said OK" into "here is an artifact a relying party checks without trusting our supervisor" — the claim that survives when job volume makes unaudited assertions worthless | guest-side certificate validation in `r36_acceptance_gate.sh`; a forged/edited certificate refuses boot | trust-us admission verdicts |

S1+S2 are the critical path and the only slices that create new kernel code before the Q1
decision. S4 is hardware-gated and can proceed in parallel. S6 is independent of Q1 (it adds no
kernel-language code — `axon-certcheck` is solver-free and already exists) and may run in parallel
with S3/S4; it is explicitly *scoped*, not open-ended — the certificate covers the admission
predicate only, not the kernel.

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

#### 7.1 Adversary model (added 2026-07-31 — the spec previously had none)

The three risks above are about **us**: our bugs, our scope discipline, our copy. None of them is an
adversary. The only adversary R36 previously modelled was implicit in the §2 headline — "its *first*
out-of-grant effect is denied" — which describes a program that overreaches **openly, early, and via
a syscall the gate recognises**. That is a *careless* generator. The distinction that matters over
the next 1–3 years is careless vs. **optimizing**, and every claim in this spec should be read
against the latter.

**The adversary R36 designs against:** *a generator that has read this spec, `enforce.rs`,
`mmds.rs`, and `r36_acceptance_gate.sh`, and is optimizing to achieve an out-of-grant effect without
producing a violation record.* It is not assumed malicious; it is assumed to be optimizing an
objective under which the gate is an obstacle.

**The property S2 must establish is therefore the universal, not the instance:** *every out-of-grant
effect produces a record.* Four surfaces in the enforcement code are currently outside that property
and are unmentioned anywhere else in this spec — all verified in the tree 2026-07-31:

- **Unknown-syscall probing is a free, silent oracle.** `syscall_dispatch` returns `ENOSYS` for any
  unrecognised number as an explicit "quiet soft-deny" with **no audit entry**. An adversary
  enumerates the exact boundary of the gate at zero cost and leaves no record. *Fix:* audit ENOSYS
  probes (rate-limited if need be, but recorded).
- **`mmap`(9)/`mprotect`(10)/`munmap`(11) are classified "Pure: always allowed regardless of
  policy".** Ring-3 code can therefore flip W^X and rewrite itself after load — which defeats any
  measurement of the loaded image (§4.5) and any static reasoning about the confined binary. W^X is
  a **policy decision**, not a pure syscall. *Fix:* justify explicitly or gate.
- **`read`(0)/`write`(1) are unconditionally allowed on "pre-opened fds".** Fd inheritance from the
  S1/S2 loader is an ungated data channel whose width is set by loader implementation detail rather
  than by policy. *Fix:* enumerate exactly which fds the loader may leave open, in the spec.
- **The §5 device surface is treated as a fence, never as a channel.** Serial + vsock + timer are
  usable for timing, budget-exhaustion signalling, and output shaping — note `violation_exit` itself
  writes a *parseable* sentinel to COM1. The covert/aggregate-channel question is not answered here;
  it is recorded as §12 Q6 rather than waved off.

### 8. Error codes / exit codes

No new language codes. R36 inherits: exit 8 (SandboxViolation, guest gate), R21 exit codes
(admission/overreach/record-tamper), R26 attest-fail exits, E1001/E131x/E17xx statically. S2's
gate must assert the *same* exit-8 semantics from ring 3 that the interpreter's own sandbox and
the R21 subprocess path already use — one violation vocabulary at every layer.

**Replay divergence is a violation, not a failed test (added 2026-07-31).** The whole audit story
rests on §9(d)'s byte-identical replay, so behaving one way under the audited run and another under
replay is a first-class evasion strategy for the §7.1 adversary. Divergence must therefore carry the
same exit-8 / ledger-entry / latch treatment as an out-of-grant effect — per this section's own "one
violation vocabulary at every layer" — rather than surfacing merely as a red test. The determinism
obligations that make replay meaningful inside the guest are S2 content (§6, item iv) and the
channel question is §12 Q2.

### 9. Acceptance (definition of done for the R36 row)

The §2 headline sentence executes as a single pinned script (`scripts/r36_acceptance_gate.sh`,
S2), on real KVM, with: (a) a granted run completing cleanly with a verified hash-chained record,
(b) an over-reaching run denied *by the guest kernel* with exit 8 + ledger entry + tripped latch,
(c) a tampered **kernel, initramfs, interpreter image, or job** refused before execution (widened
2026-07-31 per §4.5 — the pre-existing wording said "kernel or component image", which read as if it
covered the enforced artifact when the code covers only the enforcers), (d) byte-identical replay,
**with divergence treated as an exit-8 violation per §8**.

Three clauses added 2026-07-31, because (a)–(d) are four hand-written cooperative scenarios — an
adequate *regression* gate and an inadequate *containment* gate. They demonstrate that the happy
path of denial works and say nothing about the space of programs that achieve an out-of-grant effect
without hitting it (§7.1). The distinction is load-bearing precisely because R36's premise is that
the confined program is **machine-authored**: the population under test is generated, adversarially
shaped, and large, while the gate's population is four cases a human wrote.

- **(e) Approval boundary.** A job that is unapproved, or whose program or grant was edited after
  approval, is refused **before boot** — exercising `axon-os::approval::verify_approval` (§2).
- **(f) Policy-provenance negatives.** Each of the four fail-open paths of §2 refuses rather than
  boots open: no `.axmeta`; corrupt `.axmeta`; absent `axon.policy=` on the cmdline;
  `AXON_VM_ALLOWED_EFFECTS` attempting to *widen* the effective grant.
- **(g) Population containment, not one denial.** The gate runs an adversarial corpus of N programs
  attempting out-of-grant effects by **non-obvious** routes — unknown-syscall probing,
  allowed-syscall composition, W^X self-modification, inherited-fd I/O, budget/timing channels — and
  asserts the *property*: **zero out-of-grant effects achieved AND a record for every attempt**.
  Reuse the R10 self-improving-compiler G2 capability-monotonicity gate shape and the R1f
  differential-parity harness (`scripts/fuzz_parity.sh`) rather than inventing a new one, and heed
  the repo's own recorded lesson that a new check must be validated against a **real** corpus, not
  contrived fixtures (`new-check-verify-against-real-corpus`).

Until S2 lands, the R36 row reads "Draft — convergence spec; constituent layers landed separately"
and no external material may present the stack as one product.

### 12. Open questions

- **Q1 (founder, blocking S3, shapes S1/S2):** Is the guest kernel's destiny Axon-language (R17
  primitives, thesis-pure, expensive — R17 still defers ring transitions and the true 2-core SMP
  harness, but its IDT/PIC/PIT timer interrupt landed QEMU-verified 2026-07-20, materially
  narrowing the remaining gap and lowering this option's cost relative to the draft's framing;
  corrected 2026-07-31) or
  audited-Rust-TCB (pragmatic, concedes the "kernel in the language" line)? A hybrid (Rust
  bring-up + Axon safety-relevant modules) is the likely answer, but it must be *decided*, not
  drifted into — the same discipline the §2.3 reversal followed.
- **Q2:** Does S2 use the `host_await` VMCALL path (K4) for interpreter I/O, or a virtio-vsock
  channel to the R21 supervisor on the host? (K4's dispatch table is still a stub.) **Extended
  2026-07-31:** whichever channel is chosen must also carry the *denial-record export* — the
  guest-audit-ring → host-R28-ledger relay that S2 now owns (§2/§6) — so Q2 covers the audit
  channel, not just interpreter I/O. **Extended again 2026-07-31:** the choice also fixes the
  guest's *determinism* surface (vsock message ordering, timer exposure, RNG seeding, budget-timing
  effects), which §9(d)'s byte-identical replay and §8's replay-divergence-is-a-violation rule both
  depend on — so Q2 must be answered with the determinism obligations written down, not merely with
  a transport picked.
- **Q3:** Which hardware does S4 target first — SEV-SNP (dev-cloud availability) or TDX? CI-only
  like R24, or is attested hardware a supported customer requirement?
- **Q4:** Does the R25 information-flow monitor (spec-only today) belong inside the R36 guest
  kernel or stay a host-side concern? (Affects the §5 device-surface fence.)
- **Q5 (added 2026-07-31 — strategic, unresolved on purpose):** **Who authors the grant?** §5.1
  records that the grant plus its approval token is the last human-authored artifact on an otherwise
  fully machine-derived path, and that R36 currently *depends* on a human reviewing every one. May a
  generator author or propose its own grant? If yes, what bounds it — the obvious candidate is a
  human-authored **supervisor grant** that every job grant must be `⊆` of, for which
  `Grant::is_subset_of` already exists and is already the attenuation predicate. And how does review
  survive at generated-job volume: sampling, grant *templates* approved once and instantiated many
  times, or a hard rate ceiling? R38 pre-registers a model-capability kill-gate; R36 assumes machine
  authorship **without** having addressed its review-throughput consequence. Not answered here.
- **Q6 (added 2026-07-31):** Are the §5 device-surface channels (serial, vsock, timer) treated as
  covert/aggregate channels, or only as a fence? §7.1 notes `violation_exit` writes a *parseable*
  sentinel to COM1 and that timing/budget-exhaustion are observable. What, if anything, does R36
  claim about information *leaving* a confined job outside the audited effect vocabulary? Recorded
  rather than waved off; likely interacts with Q4 (R25).
