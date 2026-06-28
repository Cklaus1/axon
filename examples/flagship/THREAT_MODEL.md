# Flagship Threat Model

A demo that only ever says "look what we stop" is marketing. This document says
what Axon's capability sandbox stops, **what it does not**, and what you have to
trust for the guarantee to hold. The honest boundaries are the point — a sandbox
you can't draw the edges of is one you can't rely on.

Scope: the `@[contained]` capability system as exercised by `examples/flagship/`
(the `demo.sh` four-layer stack and the `compare_docker.sh` foil).

---

## 1. The claim

> An Axon program annotated `@[contained(fs: F, net: N, exec: E)]` cannot perform
> a filesystem, network, or process-spawn effect outside `F`/`N`/`E` — and a
> violation is caught **before the program runs** (compile error E1001), not at
> the point of damage. The policy is **derived from the program's types**, so it
> cannot drift out of sync with the code.

This is a *capability-confinement* property, enforced in four independent layers
(compile-time check → runtime dispatch re-check → operator kill-switch → kernel
attestation). A defeat of any one layer leaves the others standing.

## 2. Attacker model

The adversary is **the code itself** — an agent body that is malicious,
prompt-injected, or simply buggy. Concretely the attacker can:

- write arbitrary Axon source inside a `@[contained]` function (the "agent body");
- attempt any builtin: `read_file`, `ai_complete`/`http_*`, `exec`, `env_var`, …;
- try to *launder* a forbidden effect through a helper function, a string
  interpolation, an `import`, a `with`/`spawn`/`comptime` block, or an impl method;
- try to read an ambient secret (an env var, a host credential) it was never granted.

The adversary does **not** control: the Axon compiler binary, the interpreter/codegen,
the host OS/kernel, or the operator issuing the kill-switch. Those are the TCB (§5).

## 3. In scope — what the demo actually stops

| # | Attack | Stopped by | Evidence in the demo |
|---|--------|-----------|----------------------|
| 1 | Read a host file outside `fs` grant (`/etc/passwd`) | Layer 1 compile check (E1001) | `agent_task_evil.ax` refuses to build |
| 2 | Reach the network outside `net` grant (`ai_complete`, `http_*`) | Layer 1 (E1001) | same |
| 3 | Spawn a process when `exec: none` (`exec("curl", …)`) | Layer 1 (E1001) | same |
| 4 | Read an ambient secret (`env_var`) — env is **ungrantable** | Layer 1 (E1001), fail-closed | `contained-env-deny` |
| 5 | Launder a forbidden effect through a helper / interpolation / import / impl method | Transitive effect-walk (E1001/E1310) | `transitive-laundering-holes` regression set |
| 6 | Bypass the static check (hypothetically) and call the builtin anyway | Layer 2 runtime dispatch re-check | `axon run` refuses at the call site |
| 7 | Keep running after the operator decides to stop it | Layer 3 R27 kill-latch (exit 4, one-way) | `axon-os kill` halts < 1s |
| 8 | Tamper with the audit record of what it did | Layer 3 hash-chained ledger (R28) | `axon-os verify` breaks on tamper |
| 9 | Boot under a swapped/tampered guest kernel | Layer 4 R26 attestation (axtcb1 digest) | `axon-vm attest`; wrong digest → refused boot |

The contrast claim (`compare_docker.sh`): a hand-written Docker+seccomp profile
blocks **1 of 3** of attacks 1–3 (network); the file read and process spawn survive
because a syscall filter cannot express "open anything but /etc/passwd" or "exec to
start but never spawn." Axon stops all three. This is a statement about *policy
expressiveness, provenance, and timing*, **not** a claim that Axon's runtime
isolation is stronger than a container's (see §4).

## 4. Out of scope — what this does NOT stop

State these first when someone asks "is it secure?" The answer is "against this
attacker, for these effects — and explicitly not these things":

- **A malicious compiler/toolchain.** The guarantee is only as good as the `axon`
  binary that emits it. We do not (yet) ship reproducible builds or a verifying
  compiler; if the compiler is backdoored, all bets are off. This is the top TCB risk.
- **Covert / side channels.** `@[contained]` governs *explicit* effects. It does
  **not** stop timing channels, cache/Spectre-class leaks, resource-exhaustion
  signalling, or data encoded in scheduling. A confined agent can still leak bits
  through how long it runs.
- **Memory-safety exploitation of the runtime.** If the interpreter or generated
  code has a memory-safety bug, a crafted program might escape the language-level
  boundary. The interpreter is Rust (safe by default) but the codegen/runtime FFI
  surface is not formally verified.
- **Confidentiality of the attestation substrate.** Layer 4's default is a
  **software-TPM stand-in** (`substrate: software-tpm-v1`) with **no memory
  encryption**. It proves *which kernel image booted*; it does **not** protect the
  guest's RAM from a privileged host. Real confidentiality needs SEV-SNP / TDX
  hardware — supported as a substrate but not the demo default.
- **The host kernel / hypervisor.** We *shrink* the TCB with a purpose-built guest
  kernel, but a KVM/Firecracker or host-Linux 0-day is below our boundary.
- **Denial of service.** A confined agent can spin, allocate, or wedge itself.
  Budgets (R12) bound spend, and the kill-switch bounds wall-clock, but in-bounds
  resource abuse is not prevented.
- **Correctness of the grant.** If the operator writes `@[contained(net: ["*"])]`,
  Axon faithfully allows everything. The system enforces the policy you wrote; it
  does not decide the policy is *wise*.

## 5. Trusted Computing Base (what you must trust)

In rough order of how much you're betting on it:

1. **The Axon compiler** (`axon check`/`build`) — the capability proof originates here.
2. **The interpreter / codegen + `axon-rt`** — Layer-2 dispatch enforcement and the
   generated binary.
3. **The runtime enforcer — the `axon-guest-kernel` (a bare-metal kernel, NOT Linux).**
   Verified on real KVM. *What is real:* `axon-vm` boots the purpose-built kernel under
   Firecracker (and under QEMU), it reads the policy from the boot cmdline, **installs a
   real syscall-enforcement gate** (a SYSCALL/SYSRET MSR handler mapping each syscall to a
   required effect; un-granted effects return `VIOLATION`), and the gate is
   **policy-driven** (the allowed-effect bitmask shifts with the policy — `0xff` open vs
   `0x8` under a one-effect policy). It loads the Axon interpreter ELF from the initramfs
   and exits cleanly (~190ms). *What is NOT yet real (verified by running it):* the
   interpreter is entered with **no argv/program**, so it **does not actually execute an
   Axon program** — a probe program produces no output even under a permissive policy. So
   the program-execution path is unfinished, and consequently a **live denial** (a running
   program refused a gated syscall) **cannot yet be demonstrated** — the blocker is program
   execution, not the gate. Full IDT/timer-ISR, SMP, and a machine-checked proof are also
   future work. A **seccomp-BPF** allowlist (from the `.axmeta`) exists as a secondary
   layer. Net: the kernel boots and the enforcement gate is real and policy-driven, but
   end-to-end *"a sandboxed Axon program runs and is denied a syscall"* is not yet working.
4. **The attestation root** (`axon-attest`, R26→R31 `axtcb1` chain) — and, in the
   default config, the software-TPM stand-in.
5. **The operator** — assumed honest and available to trip the kill-switch.

Axon's design rule: **self-modification cannot weaken the TCB** (the Layer-3
self-improving compiler runs behind a 4-gate firewall incl. capability-monotonicity).
That is a design invariant of this repo, not an externally audited claim.

## 6. Assumptions

- The agent is compiled and run *by Axon* (a program that bypasses the compiler
  entirely — e.g. raw machine code — is a different threat model: that's what Layer 4
  attestation + Layer 1–2 in-language enforcement jointly address, but only for
  artifacts that actually go through the toolchain).
- The `.axmeta` manifest consumed by `axon-vm` is the one Axon emitted (it is, by
  construction, in the build pipeline; out-of-band tampering is detected by attestation).
- "Compile-time" refusal assumes the predicate/effect is statically decidable; the
  residual runtime checks (Layer 2, exit codes 4/6/8) cover the rest fail-closed.

## 7. How to attack this demo (please do)

The fastest way to make this credible is to break it. Good attempts:

- Find an effect-laundering path that reaches a builtin without an E1001
  (helper chains, `with`/`spawn`/`comptime`, impl methods, interpolation, imports —
  several such holes have been found and closed; find the next).
- Get a `@[contained(net: [])]` program to open a socket at runtime.
- Produce a kernel image whose `axtcb1` digest collides with the blessed one.
- Defeat the kill-switch latch from inside the agent.

If you find one, that's a finding worth more than another feature. File it against
the relevant layer.

## 8. Red-team log (attacks actually run)

A first adversarial pass ran 15 laundering attacks against
`@[contained(fs: [], net: [], exec: none)]`, each trying to reach a forbidden builtin
(`read_file`/`ai_complete`/`exec`/`env_var`) by a different route:

- **Refused at compile time (E1001) — 13/15:** direct call; 1-hop helper; 2-hop helper
  chain; inline closure; higher-order (closure passed to an invoker); `match` arm;
  string interpolation; `comptime` block; `spawn` block; `env_var` direct; `env_var`
  via helper; `ai_complete` via helper. The transitive effect-walk holds across all
  these boundaries.
- **One soundness gap, currently fail-closed — `let f = read_file; f(path)`:** aliasing
  a builtin to a local binding passes `axon check` (the capability checker does not yet
  flag builtin *references*, only direct calls). It does **not** exfiltrate: the
  interpreter has no first-class builtin value, so it panics `undefined identifier
  read_file` at runtime — the file is never read (Layer 2 fail-closed). **Open fix:**
  the static checker should reject a forbidden builtin appearing as a value, not only
  as a callee — otherwise this becomes a live hole the day builtins are first-class.

This log is partial by construction. The point of §7 is to extend it.
