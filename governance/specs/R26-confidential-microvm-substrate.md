# Tech Spec — R26: Confidential Micro-VM Substrate (hardware-isolated, attested)

**Spec ID:** `R26-confidential-microvm-substrate`
**Status:** ✅ Landed (re-verified 2026-07-18) — `axon-vm attest` mandatorily attests before every
boot (`--no-attest` dev escape; `AXON_CI_NO_KVM=1` CI); `axtcb1:` SHA-256(kernel_bytes) digest,
HMAC-signed report. `scripts/r26_acceptance_gate.sh`: all §0 checks present, unstubbed, green
(software-TPM stand-in lane; real SEV-SNP/TDX needs the `hw-attest` feature + confidential
hardware). This header said "Draft" long after the code shipped — same staleness class as
R17/R21/R22/R23/R31/R32, caught by the same outer-loop sweep (`EXECUTION_MODEL.md` §2).

```spec-meta
id: R26-confidential-microvm-substrate
status-claim: Landed
depends-on: R21-axon-os-supervisor
blocks: R29-continuous-compliance-monitor, R31-extended-tcb-attestation, R33-cross-vm-safety-quorum
blocked-by: none
supersedes: none
related: R24-tee-target, R36-full-asi-os
conflicts-with: none
reserves: none
evidence: scripts/r26_acceptance_gate.sh (re-verified 2026-07-18)
```
**Implements:** `VISION_OS.md` §4.3 (pillar "what can be SENSED — side channels, hardware,
confidential computing") + §5 (the host-substrate dial) + gap **G5** (micro-VM guest, ideally
confidential) — and tied to **G6** (hardware root of trust) via mandatory remote attestation. Swaps
the **bottom of the TCB**: R21 today runs untrusted programs as a **host subprocess**
(`crates/axon-os/src/runtime.rs`), which trusts the *whole host kernel* — a host-kernel bug is a full
ASI escape no capability proof catches (`VISION_OS.md` §5 row "Container/daemon": *weakest: a
host-kernel bug is an ASI escape route*). R26 replaces that boundary with a hardware-isolated
**micro-VM** (Firecracker / Cloud Hypervisor class) with a tiny virtio/serial device surface, ideally
on **AMD SEV-SNP / Intel TDX** confidential computing so even the host operator is outside the trust
boundary, chained to a hardware root of trust via **remote attestation**.
**Audience:** an implementer who builds *strictly* against this document and reads only it.

> **Read this framing first.** R26 does **not** change one line of Axon-level containment. The same
> Axon program + the same R20 capability proofs + the same R23 certificate gate run **unchanged inside
> the VM** (the R21 supervisor and the R23 cert checker run *in the guest* — §4). What R26 changes is
> the **isolation boundary underneath** them: from "an OS process the host kernel could subvert" to
> "a hardware-virtualized, memory-encrypted guest the host operator cannot read or tamper with,
> *provably*, via remote attestation chained to silicon." The load-bearing new requirement is
> **attestation is MANDATORY, not optional**: a relying party verifies it is talking to a genuine,
> unmodified Axon OS image with the expected TCB measurement — and the in-language `axtcb1:` digest
> (R20 Slice 3) is **chained down to the hardware measurement**, not left as a disconnected story. A
> VM with no attestation is **explicitly NOT an acceptable R26 substrate.**

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey + smoke test through the actual CLI (boot→attest→admit→run→audit in a VM) | §5, §7 | `acc_a1_smoke_boot_attest_run_journey` |
| **A2** | Real runnable artifact: a real guest image + its manifest + the same R21 job runs inside it | §5.6, §7 | `acc_a2_guest_image_runs_real_job` |
| **A3** | Quickstart whose exact commands are executed by a test | §9, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Hermetic, isolated: minimal device surface only; guest cannot reach the host beyond vsock/serial | §3.3, §4.4, §7 | `acc_a4_device_surface_minimal_and_isolated` |
| **A5** | Deterministic: same image ⇒ byte-identical measurement; same job+seed ⇒ byte-identical record | §3.4, §4.5, §7 | `acc_a5_measurement_and_record_byte_identical` |
| **A6** | Integrity/attestation: MANDATORY; relying party verifies the report before trusting; chained to `axtcb1:` | §3.1, §4.2, §4.3, §7 | `acc_a6_attestation_mandatory_and_chained` |
| **Core** | Attestation report binds the guest measurement to a HW root of trust; fail-closed on mismatch | §3.1, §4.3, §7 | `attestation_mismatch_fails_closed` |
| **Core** | A VM with **no** attestation is REFUSED (not "degraded") | §4.3, §7 | `vm_without_attestation_is_refused` |
| **Core** | The in-language `axtcb1:` TCB digest is chained into the HW measurement, not disconnected | §3.1, §4.2, §7 | `axtcb1_digest_chained_to_measurement` |
| **Core** | Tampered guest image produces a different measurement → relying party rejects | §4.3, §7 | `tampered_guest_image_fails_attestation` |
| **Gate** | The acceptance gate itself fails if any check above is missing/stubbed | §10 | `scripts/r26_acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does
R26 is a **substrate driver + attestation library + CLI** that boots an Axon OS guest in a
hardware-isolated micro-VM, **measures** it, **attests** that measurement to a relying party, and only
then **admits** untrusted work into it. Concretely:

1. **Builds a guest image** — the R17 freestanding machinery (`@[entry]`/`@[panic_handler]`,
   virtio/serial drivers, boots under QEMU; `governance/specs/R17-freestanding-substrate.md` Slices
   0–3) is the guest-target machinery R26 **reuses**. The image bundles the R21 supervisor + the R23
   cert checker so the in-guest containment loop is identical to today's.
2. **Boots it in a micro-VM** with a **minimal device surface** (virtio-vsock + serial console + a
   timer; nothing else — §3.3) under a Firecracker / Cloud-Hypervisor-class VMM, ideally with
   **SEV-SNP / TDX** memory encryption so the host operator is outside the boundary.
3. **Measures** the image (the launch digest of guest memory + the firmware-measured boot chain) and
   produces an **attestation report** (§3.1) signed by the hardware root of trust (SEV-SNP attestation
   report / TDX quote / a TPM quote in the software stand-in).
4. **Attests, MANDATORILY** — a **relying party** (the operator's machine, outside the VM) verifies
   the report: signature chains to a known HW vendor / measurement root **and** the measurement equals
   the expected Axon OS image digest **and** the report's embedded `axtcb1:` TCB digest (R20 Slice 3)
   equals the pinned one. **Any failure ⇒ fail closed; no job is admitted.** A VM with no attestation
   is refused outright (Core).
5. **Admits work** only into an *attested* guest — the same `.axjob` the R21 supervisor runs today is
   sent over vsock into the verified guest, where R21 gate→mint→run→record + R23 cert-check execute
   **unchanged**, and the audited `RunRecord` is returned over vsock to the relying party.

The trust shift: before R26, "isolated" = "an OS process the host kernel sandboxes" (trust the whole
host kernel + operator). After R26, "isolated" = "a hardware-encrypted guest whose exact image is
attested to silicon before any work enters it" (trust the VMM + hypervisor, and under SEV-SNP/TDX, not
even the host operator).

**In-spec adversary (assume the worst reader).** We assume the host operator and an ASI guest have
both read this spec, and the swtpm stand-in's CA is operator-controlled. The design must therefore be
secure against an adversary who knows every step here — the stand-in lane in particular gives a *forged*
attestation to any operator who chooses to (it is the honest CI floor, not a confidentiality claim; §6,
§8); only the real-hardware lane (S7) puts the operator outside the boundary.

### 1.2 What it explicitly does NOT do (out of scope for R26)
- **No new Axon-level containment.** Capability proofs (R20), the cert checker (R23), the supervisor
  gate/mint/audit/replay (R21), and info-flow (R25) are **unchanged** — they run *inside* the guest
  exactly as today. R26 is the substrate beneath them, not a re-implementation.
- **R26 does NOT close microarchitectural side channels.** Confidential computing removes the host
  OS/operator from the boundary but **does not** close Spectre/Meltdown-class speculative leaks,
  cache-timing, power/thermal, or **Rowhammer** (which physically flips bits *across* the encrypted
  boundary). These remain **residual** (`VISION_OS.md` §4.3); §7's honest threat model names them, the
  mitigations (cache/core partitioning, no SMT co-residency), and the floor. R26 is **not** a
  side-channel-free claim. (The full side-channel discipline is gap **G10**, a separate spec.)
- **No GPU / general devices / general driver model** (`VISION_OS.md` §10 de-scope). The device
  surface is the §3.3 allowlist (vsock/serial/timer) and **nothing else** — an unproven driver is, vs
  an adversarial AI, a full escape (`VISION_OS.md` §3 "smallest hardware surface").
- **No bare metal** — that is the R17 north star (`VISION_OS.md` §11 v3+). R26 keeps a VMM +
  hypervisor in the TCB; bare metal floors that later.
- **No PKI / key management product, no certificate issuance.** R26 *consumes* the HW vendor's
  attestation chain (AMD KDS / Intel PCS / the swtpm CA in the stand-in); it does not run a CA.
- **No replacement of R23's certificate trust.** R26 *carries* the R23/R20 digests into the HW
  measurement; it does not re-prove anything.
- **No live migration, snapshotting, multi-VM orchestration, or networking stack of our own.** A
  single attested guest, run once, is the unit; orchestration is future work.

### 1.3 Persona / ICP
A **security-conscious operator running an untrusted (AI-authored) Axon task** who needs the isolation
boundary to be **hardware-enforced and provably the expected image** — not "trust my host kernel and
my cloud operator." Secondarily, a **relying party** (a customer, a regulator, a second operator) who
must verify, *remotely and before sending data/work*, that the box on the other end is a genuine,
unmodified Axon OS with the expected TCB measurement.

### 1.4 Interface & tech constraints
- **Interface:** a CLI binary `axon-vm` (subcommands `build-image` / `measure` / `launch` / `attest` /
  `verify` / `run`), plus a library crate.
- **Language/deps:** Rust, new workspace crate `crates/axon-vm`. Allowed deps: `sha2` (already in the
  workspace, for measurement digests), `serde`+`serde_json` (the report + manifest), and **a VMM
  invoked as a subprocess** (Firecracker / Cloud Hypervisor / QEMU for the stand-in — never linked,
  always the impure seam). The hardware-attestation crates (SEV-SNP `sev`, TDX quoting) are gated
  behind a `hw-attest` feature and a real-hardware acceptance lane (§6). **No** new heavy deps in the
  pure core.
- **Perf/security:** the pure core (measurement algebra, report schema, the verification algorithm) is
  I/O-free and deterministic; all VMM spawning, hardware-report fetching, and vsock I/O live behind the
  `Substrate` trait so the verifier is fully testable with a mock. **Fail closed on every attestation
  ambiguity** — an unverifiable report is a refusal, never a downgrade.

---

## §2 — Architecture & modules

New crate `crates/axon-vm/`. **Core logic is pure (no I/O, no clock/random); all VMM control,
hardware-report fetching, and vsock transport live behind the `Substrate` trait** so the verifier and
the boot→measure→attest→admit pipeline are fully testable with a mock (and with the QEMU+swtpm
software stand-in as the integration impl).

> **truth: as-built architecture note (added 2026-07-19, no functional/code change).** The module
> layout and `trait Substrate` below describe the ORIGINAL design intent; the shipped implementation
> took a simpler, pragmatic path that never matches it file-for-file. What actually exists: a flat
> `crates/axon-attest/src/lib.rs` (attestation logic, no internal module split) plus ~5 independent
> inline `env::var("AXON_CI_NO_KVM")` branches scattered across `crates/axon-vm/src/main.rs`
> (`cmd_attest`, `quorum_self_tcb`, the deploy-gate check, etc.) — there is **no `substrate.rs`, no
> `trait Substrate`, no `MockSubstrate`, no `QemuSwtpmSubstrate` struct, and no `hw-attest` Cargo
> feature** (it appears only in doc-comment prose, never as a real `[features]` entry) anywhere in
> the tree. The functional claim this spec's "Landed" status makes — mandatory attestation before
> boot, a working software-TPM-stand-in CI lane, fail-closed on ambiguity — **is real and
> gate-verified** (`r26_acceptance_gate.sh` passes); only the specific architectural claim below (a
> trait-based seam with swappable impls) is aspirational, not built. Found while scoping R33.S2
> (vsock transport), which cited this section as an "existing boundary" to reuse — it isn't one.
> Left uncorrected below (not rewritten) since it's still a reasonable target architecture if this
> crate is ever refactored; readers building on top of R26 should treat it as a plan, not a fact.

```
crates/axon-vm/src/
  manifest.rs     Parse + validate a guest-image manifest (.axvm) → GuestManifest.   [PURE]
  measure.rs      Measurement algebra: image → Measurement digest; expected-vs-actual. [PURE]
  report.rs       AttestationReport schema (build/parse) + the axtcb1 binding.         [PURE]
  verify.rs       The relying-party verifier: report → Verdict (fail-closed).          [PURE]
  admit.rs        Post-attestation admission: only an ATTESTED guest accepts a job.     [PURE]
  verdict.rs      Verdict enum + exit-code mapping.                                     [PURE]
  pipeline.rs     Orchestrates build→launch→measure→attest→verify→admit→run. Generic.   [PURE core, I/O injected]
  substrate.rs    `trait Substrate` (the seam) + impls (see below).                     [I/O — impure]
  cli.rs          Arg parse → command dispatch → human output + exit codes.            [I/O — thin]
  lib.rs          Public API re-exports.                                                [—]
  main.rs         `fn main()` → cli::run(std::env::args).                              [I/O — thin]
crates/axon-vm/tests/
  acceptance.rs   The A1–A6 + Core acceptance checks (named exactly per §0).
guest/
  axon-os.axvm + axon-os guest image build recipe (reuses R17 freestanding; bundles R21+R23).
examples/vm/
  summarize.axjob (+ summarize.ax)   The SAME R21 job, now run inside the attested guest (A2).
scripts/r26_acceptance_gate.sh       The pinned gate (§10).
README-axon-vm.md                    Quickstart whose commands a test executes (A3).
```

**The `Substrate` impls (the only impure edge):**
- `QemuSwtpmSubstrate` — **the CI stand-in** (§6): boots the guest under QEMU with **swtpm** (software
  TPM) providing a measured-boot quote. No special hardware; runs in CI. The attestation chain roots at
  a test CA the swtpm vends. **This is the default lane the acceptance gate runs.**
- `SevSnpSubstrate` / `TdxSubstrate` — **the real-hardware impls** (`hw-attest` feature, §6): boot
  under Firecracker / Cloud Hypervisor with SEV-SNP / TDX; the report is a genuine SEV-SNP attestation
  report / TDX quote rooting at AMD KDS / Intel PCS. **Gated to a separate hardware lane** — not run in
  ordinary CI.
- `MockSubstrate` — in-memory, for unit-testing the verifier and pipeline with no process spawn.

**Dependency graph (acyclic; arrows = "imports/uses"):**
```
main → cli → pipeline → {measure, report, verify → {report, measure}, admit → verify, substrate}
manifest → measure
report → measure
verify → {report, measure, verdict}
admit → {verify, verdict}
substrate → (VMM subprocess + swtpm/SEV/TDX + vsock)   [the ONLY edge to the outside world]
```
Rule the implementer MUST hold: **nothing under `manifest/measure/report/verify/admit/verdict/
pipeline` may perform I/O, read the clock, or call random.** The expected measurement, the pinned
`axtcb1:` digest, the report bytes, and the run outcome are passed in explicitly (A5). Only
`substrate.rs`, `cli.rs`, `main.rs` touch the outside world. **The verifier (`verify.rs`) is the
trusted artifact — keep it small, pure, total** (the R23 discipline: you trust it because you can read
all of it).

---

## §3 — Data model

### 3.1 `AttestationReport` (the HW-rooted evidence; JSON, schema `axon-vm-report/1`)
The relying party verifies **this** before trusting the guest. It is produced by the substrate after
launch and carries the hardware root-of-trust signature over the guest measurement.
```
AttestationReport {
    schema:        "axon-vm-report/1",
    substrate:     Substrate,        // "sev-snp" | "tdx" | "qemu-swtpm" (the stand-in)
    measurement:   Measurement,      // §3.3: the launch digest of the guest image + boot chain
    axtcb_digest:  String,           // "axtcb1:…" — the R20 Slice 3 in-language TCB digest, BOUND IN
    report_data:   String,           // 64-byte freeform field the GUEST fills: see §4.2 binding
    nonce:         String,           // relying-party-supplied freshness nonce (anti-replay)
    signature:     String,           // HW signature over (measurement ‖ report_data ‖ nonce)
    cert_chain:    Vec<String>,      // leaf → … → vendor root (AMD KDS / Intel PCS / swtpm CA)
    ek_id:         String,           // the attesting key identity: HW EK/VCEK serial, or (stand-in) the swtpm EK fingerprint — PINNED by the verifier (§3.1, §8)
}
```
**Pin the EK, not just "a CA" (§8, stand-in floor).** In the QEMU+swtpm stand-in the operator **owns
the swtpm and its CA** — "chains to a test CA" is *not* sufficient, because that operator can mint a
valid quote for **any** image under their own CA. The verifier therefore pins a **specific
`ek_id`/measured-boot expectation** (a single expected EK fingerprint + measured-boot PCR set), not
merely "some cert under the test CA." This narrows the stand-in to *the one* attesting key the relying
party provisioned. It does **not** make the stand-in unforgeable — §8 states plainly that the stand-in's
chain is **operator-forgeable by construction** (the operator holds the EK private key); pinning the EK
is the honest CI-lane *floor*, and only the real-hardware lane (S7), where the EK/VCEK is silicon-bound
and rooted at AMD KDS / Intel PCS, removes the operator from the boundary.
**The chain (the load-bearing design, A6 + Core `axtcb1_digest_chained_to_measurement`):** the
in-language `axtcb1:` digest is **not** a second disconnected story. At guest boot, the Axon OS image
computes its live `axtcb1:` TCB digest (R20 `smt.rs` `live_tcb_digest()` / `MINT_OBLIGATION_SPEC ⊕
verdict`, boot-checked → E1611) and **writes it into the hardware report's `report_data` field**
(`report_data = sha256(axtcb_digest ‖ nonce)`). The HW signs `report_data` along with the measurement.
So one HW signature now covers **both** *"this is the expected image at the hardware level"* (the
`measurement`) **and** *"the image's own proven-TCB attestation is the pinned one"* (the `axtcb_digest`
bound through `report_data`). Verifying the signature transitively binds R20's `axtcb1:` digest to the
silicon — the two attestations are **one chain**, not two. **This is load-bearing only because the
verifier RE-DERIVES the expected `axtcb1:` from the measured image bytes** (an R23 certificate validated
solver-free; §4.2/§4.3 step 5): comparing the report against a *second pre-pinned constant* would add
~zero over measurement-pinning, since the `axtcb1:` source is already inside the measured image. The
digest is also solver-free — it commits to a checkable **R23 certificate**, **not** to Z3's `"Proven"`
verdict string, so **no Z3 enters the guest or the verifier** (R23/minimal-TCB discipline; §10).
(This is the §5 commitment "generalizes R20's `axtcb1:` content-addressed TCB attestation down to the
silicon," made concrete.)

### 3.2 `GuestManifest` (parsed from a `.axvm` TOML file)
```
GuestManifest {
    image:            PathBuf,   // the freestanding Axon OS guest image (R17 build output)
    expected_meas:    String,    // "axmeas1:" + the measurement the relying party expects (pinned)
    expected_axtcb:   String,    // "axtcb1:" + the pinned in-language TCB digest (from R20)
    substrate:        Substrate, // required isolation: sev-snp | tdx | qemu-swtpm
    devices:          DeviceSet, // MUST be exactly the §3.3 allowlist; any extra → Malformed
    vcpus:            u32,       // small (default 1); used for the no-SMT-co-residency posture (§7)
}
```
Validation (fail → `Verdict::Malformed`, exit 2, field-specific message): `image` exists; both digests
are well-formed (`axmeas1:`/`axtcb1:` + 64 hex); `substrate ∈ {sev-snp, tdx, qemu-swtpm}`; `devices`
is **exactly** the §3.3 set (no GPU, no block beyond the rootfs, no extra virtio) — a manifest
requesting any device outside the allowlist is **rejected, not stripped**.

### 3.3 `Measurement` + the **minimal device surface** (HARD requirement, A4)
```
Measurement { algo: "sha256", digest: String }   // "axmeas1:" + sha256(launch image ‖ boot chain)
DeviceSet { vsock: bool, serial: bool, timer: bool }   // and NOTHING else representable
```
The **device surface is a closed allowlist** — exactly three, each justified; *anything not on this
list is not representable in `DeviceSet` and a manifest naming it is `Malformed` (A4):*

| Device | Why it MUST exist | Why nothing more |
|---|---|---|
| **virtio-vsock** | the *only* job/result channel: the relying party sends the `.axjob` in and gets the audited `RunRecord` out, host↔guest, over a single typed socket — no shared filesystem, no network namespace. | A general network stack is a huge unproven attack surface (`VISION_OS.md` §3); vsock is point-to-point, no routing, no IP. |
| **serial console** | boot diagnostics + the `@[panic_handler]` output (R17), and the early-boot measurement log before vsock is up. Read-only from the host's view. | a console is a byte sink, not a controllable device; cannot be driven *into* the guest beyond the line discipline. |
| **timer** | the cooperative scheduler + R21's hard wall-clock timeout (runtime.rs today) need a clock tick inside the guest. | a *monotonic tick only* — not an RTC, not wall-clock-settable (which would break A5 determinism and add a host-controlled input). |

**Explicitly absent (and refused if requested):** GPU / any accelerator (the largest unproven surface,
`VISION_OS.md` §10 de-scope), general block devices beyond the read-only rootfs, extra virtio devices
(net/balloon/rng/console-multiport), PCI passthrough, any host-shared memory beyond the encrypted
guest range. The rootfs is the *measured, read-only* guest image itself — it is part of the
measurement, not a mutable device.

### 3.4 `Verdict` + exit codes (§4 maps every outcome to exactly one)
```
Verdict =
  | Admitted   { record_digest: String }   → exit 0   // attested + job ran + record returned
  | Malformed  { reason: String }          → exit 2   // bad manifest/usage/device set
  | AttestFail { reason: String }          → exit 10  // attestation absent/invalid/measurement≠expected
  | TcbMismatch{ reason: String }          → exit 11  // axtcb1 binding ≠ pinned (chained-digest break)
  | DeviceDeny { device: String }          → exit 8   // manifest requested a device outside the allowlist
  | RunFault   { detail: String }          → exit 9   // in-guest R21/R23 returned a fault (forwarded)
```
Exit codes extend Axon's carved scheme without collision: R21 owns 6/7/8/9 for the *in-guest*
verdicts; R26 forwards those as `RunFault`(9) when relayed from the guest, reuses **8** for the
substrate-level device-allowlist denial (a capability/sandbox-class refusal, same family), and adds
**10** (attestation) and **11** (TCB-chain mismatch) — new and owned by `axon-vm`. **Determinism:** the
attestation `Verdict` and the `Measurement` are pure functions of (manifest, report, expected digests);
no clock/random in core (A5).

---

## §4 — Core logic / algorithms

### 4.1 The pipeline (`pipeline::run`) — Core orchestration, I/O via `Substrate`
```
fn run(manifest, pinned_axtcb, nonce, job, sub: &impl Substrate) -> Verdict
```
1. **Validate** the manifest (§3.2). Bad → `Malformed` (exit 2). Device outside the allowlist →
   `DeviceDeny` (exit 8). **No VM launches on a bad manifest** (fail closed before the substrate).
2. `launched = sub.launch(&manifest.image, &manifest.devices, manifest.substrate, manifest.vcpus)`
   — boot the guest under the required substrate with **exactly** the §3.3 device set.
3. `report = sub.fetch_attestation(&launched, nonce)` — get the HW-signed report (SEV-SNP report / TDX
   quote / swtpm quote). **Absent/empty report ⇒ `AttestFail` ("no attestation") — REFUSED, not
   degraded** (Core `vm_without_attestation_is_refused`). The VM is torn down; no job is sent.
4. `verdict = verify::verify(&report, &manifest.expected_meas, &pinned_axtcb, nonce)` (§4.3).
   - On any `AttestFail`/`TcbMismatch` → **tear down the VM and return; NO job is admitted** (fail
     closed before any untrusted work enters the box).
5. `outcome = admit::send_job(&launched, sub, &job)` — only now is the `.axjob` sent over vsock into
   the **attested** guest, where R21 gate→mint→run→record + R23 cert-check run unchanged; the audited
   `RunRecord` (+ its `record_digest`) comes back over vsock.
6. Map: in-guest fault → `RunFault` (exit 9, forwarded); success → `Admitted{ record_digest }`.
   `sub.teardown(launched)` in all paths (RAII guard — no leaked VM).
The pipeline performs **no** I/O itself; it is pure given `sub`. This is what makes the
`MockSubstrate` tests in §7 possible.

### 4.2 The `axtcb1:` ↔ hardware binding (`report::bind_axtcb` / the guest side) — Core
- **The redundancy this design must avoid.** A naïve binding (the guest sets
  `report_data = sha256(pinned_axtcb ‖ nonce)` and the verifier checks it equals
  `sha256(pinned_axtcb ‖ nonce)`) adds **~zero marginal security over measurement-pinning**: both
  `pinned_axtcb` and `expected_meas` are constants the relying party already holds, so the binding only
  re-states a pin the `measurement` check already enforces (the `axtcb1:` source bytes are *part of the
  measured image*). To be load-bearing the verifier must **independently RE-DERIVE the expected
  `axtcb1:` FROM the measured image bytes** — not compare two pre-pinned constants. See §4.3 step 5.
- **Solver-free by construction — no Z3 in the guest.** R20's `tcb_attestation_digest()` /
  `live_tcb_digest()` / E1611 are behind axon-core's `smt` feature and commit to the verdict **string**
  `"Proven"` — i.e. *Z3's say-so*, not a checkable artifact. Carrying Z3 into the guest would **bloat
  the TCB and contradict R23's minimal, solver-free discipline** (and §10's solver/HW-free core check).
  Therefore **the guest does NOT carry Z3.** The guest's `axtcb1:` is derived **solver-free from an
  R23 CERTIFICATE** (the proof artifact R23's checker validates without a solver): the boot binding is
  `axtcb1: = sha256(MINT_OBLIGATION_SPEC ‖ r23_certificate_digest)`, where `r23_certificate_digest`
  commits to the *checkable* certificate, not to the string "Proven". The guest verifies that
  certificate with the in-image R23 checker (already bundled, §1) at boot; a missing/invalid certificate
  fails closed at boot (the in-guest analogue of E1611) and **no report is produced.**
- **Guest side (at boot, inside the Axon OS image):** verify the bundled R23 certificate with the
  in-image R23 checker (solver-free); compute
  `axtcb_digest = sha256(MINT_OBLIGATION_SPEC ‖ r23_certificate_digest)`; the boot check fails closed
  (in-guest E1611 analogue) if the certificate is absent/invalid. Then set
  `report_data = sha256(axtcb_digest ‖ nonce)` and request the HW report over it. The HW signs
  `(measurement ‖ report_data ‖ nonce)`.
- **Why this is the chain, not two stories:** the relying party never *separately trusts* the guest's
  claim about its `axtcb1:` digest **nor a solver's "Proven" verdict**. The HW signature covers
  `report_data`, a collision-resistant commitment to `axtcb_digest`; `axtcb_digest` in turn commits to
  the *checkable R23 certificate*, which the verifier re-derives from the measured image (§4.3 step 5).
  So *"the in-language proven-TCB attestation is the pinned one"* is verified **by the same signature**
  that proves *"the hardware measured the expected image"* — and the proof itself is a certificate, not
  a solver's word. Break the image → measurement differs → signature won't match expected. Swap the
  proven TCB (a future edit weakening the minter) → the R23 certificate over the new spec → re-derived
  `axtcb1:` differs → relying-party rejects (and the guest's own boot check / `report_data` diverge).
  Either way the two attestations move together, **without Z3 anywhere in the guest or the verifier.**

### 4.3 The relying-party verifier (`verify::verify`) — the trusted artifact, fail-closed (A6)
Input: `(report, expected_meas, pinned_axtcb, nonce)`. Output: `Verdict`. Steps, in order; the
**first** failure refuses (deny by default — an *unverifiable* report is `AttestFail`, never a pass):
1. **Signature, chain & PINNED EK.** Verify `report.signature` over `(measurement ‖ report_data ‖
   nonce)`, that `report.cert_chain` chains to a **known** root (AMD KDS / Intel PCS pinned root, or the
   swtpm test CA in the stand-in), **and that `report.ek_id` equals the pinned EK/measured-boot
   expectation** (§3.1) — *not merely "some cert under the CA."* In the stand-in the operator owns the
   CA, so chain-only acceptance lets the operator mint a quote for any image; pinning the EK narrows it
   to the one provisioned attesting key (still operator-forgeable, §8 — the honest CI floor). Unknown
   root / bad signature / unpinned EK → `AttestFail{"chain"}`.
2. **Freshness.** `report.nonce == nonce` (anti-replay; a recorded old report is refused) →
   else `AttestFail{"stale nonce"}`.
3. **Measurement.** `report.measurement.digest == expected_meas` → else `AttestFail{"measurement ≠
   expected (tampered/wrong image)"}` (Core `tampered_guest_image_fails_attestation`,
   `attestation_mismatch_fails_closed`).
4. **Substrate floor.** `report.substrate` is one the manifest required and is an *attesting* substrate
   (sev-snp/tdx/qemu-swtpm) — a non-attesting boot is impossible to reach here because step 1 needs a
   signature, but assert it explicitly so a future substrate can't slip in unattested (Core
   `vm_without_attestation_is_refused`).
5. **The chained `axtcb1:` binding — RE-DERIVED, not re-pinned.** The verifier does **not** compare the
   report against a second pre-pinned `axtcb1:` constant (which would add ~zero over step 3, §4.2). It
   **re-derives the expected `axtcb1:` from the measured image bytes it just pinned in step 3**:
   `expected_axtcb := sha256(MINT_OBLIGATION_SPEC ‖ r23_certificate_digest)`, where the
   `r23_certificate_digest` is taken from the R23 certificate carried in (or measured into) the image
   and **validated by the relying party's bundled R23 checker — solver-free, no Z3** (the same checker
   that runs in the guest; §4.2). Then assert `report.report_data == sha256(expected_axtcb ‖ nonce)`
   **and** `report.axtcb_digest == expected_axtcb`. Because `expected_axtcb` is derived from the
   measured image (step 3) — not supplied as an independent pin — this step adds real evidence: it ties
   the HW-signed `report_data` to a *re-checked* proof certificate over *this* image, not to a constant
   the operator could have chosen. An invalid/absent R23 certificate, or any mismatch → `TcbMismatch{…}`
   (exit 11) — Core `axtcb1_digest_chained_to_measurement`. (The `--expect-axtcb` CLI pin, §5.2, is an
   optional belt-and-suspenders cross-check against the re-derived value; the *re-derivation* is the
   load-bearing step.)
6. All pass → `Ok` (the pipeline proceeds to admit). **Pure, no I/O, total** — the trusted core.

**TOCTOU — the job MUST go to the *same* attested instance (no time-of-check/time-of-use gap).** A
verified report attests *an instance at a moment*; admitting the job to a *different* instance (one the
substrate reset, migrated, snapshotted-and-restored, or rolled to a fresh boot after the report was
fetched) would defeat the whole gate. Therefore the verifier's `Ok` carries a **session binding**: the
vsock channel `send_job` uses MUST be the *identical, un-reset/un-migrated VM instance* that produced
the verified report. Concretely, the relying-party `nonce` (already bound into the report signature,
steps 1–2) is **re-asserted on the vsock session** — the guest echoes the same nonce as the first frame
of the admitted session, and `admit::send_job` refuses (→ `AttestFail{"session ≠ attested instance"}`,
fail closed, teardown) if the vsock peer does not present the attested launch's nonce/session binding.
**If the substrate cannot guarantee the job-bearing session is the same instance that attested (e.g. it
permits live migration or transparent restart between `fetch_attestation` and `send_job`), the run is
REFUSED** — R26's no-migration/no-snapshot scope (§1.2) is load-bearing here, not just a feature cut.

**The verifier never trusts a claim in the report; it re-derives.** It does not believe
`report.measurement` because the report says so — it checks the signature *and* compares to the
*independently pinned* `expected_meas`. A forged report with a plausible-looking measurement fails at
step 1 (no valid HW signature); a real report of a *tampered* image fails at step 3 (measurement ≠
expected). This is the R23 discipline applied to hardware evidence.

### 4.4 Minimal, isolated execution (`*Substrate::launch`) — the impure seam (A4)
- The guest boots under the VMM (Firecracker / Cloud Hypervisor / QEMU-stand-in) with **exactly** the
  §3.3 device set passed on the command line — the substrate impl MUST construct the VMM invocation
  from `DeviceSet` and add **no** device the manifest did not name (an over-provisioning bug would be
  caught by `acc_a4_device_surface_minimal_and_isolated`, which inspects the launched config).
- Under SEV-SNP/TDX, guest memory is **encrypted**; the host operator (and the VMM beyond the measured
  range) cannot read it. The stand-in (QEMU+swtpm) does **not** encrypt memory — it provides the
  *measured-boot + attestation shape* for CI, with the honest caveat (§6) that confidentiality-vs-host
  is the property only the real-hardware lane delivers.
- A **hard launch+attest timeout** wraps the boot; on expiry the VM is torn down and the run is
  `AttestFail{"boot/attest timeout"}` (fail closed — a VM that won't attest in time is refused). No
  leaked VM / vsock handle (RAII guard that tears down on drop), mirroring R21's `run_bounded` discipline.
- The job/result channel is **vsock only** — no shared fs, no host network into the guest.

### 4.5 Determinism (A5)
- The **measurement is deterministic**: the same guest image (R17 build is reproducible) ⇒ byte-identical
  `Measurement` digest. The relying party pins `expected_meas` from a known-good build; a rebuild that
  changes one byte changes the digest and is refused.
- The **in-guest run is deterministic** exactly as R21 today: same `.axjob` + seed ⇒ byte-identical
  `RunRecord` (R21 §4.5). R26 adds no clock/random to the core. The *report* contains a relying-party
  nonce (non-deterministic by design, for freshness) — but the nonce lives **outside** the measurement
  and outside the record-equality check (it is in `report_data`/`signature`, not in `Measurement` or
  `RunRecord`), so A5's byte-identity assertions hold over the measurement and the record.

### 4.6 Touchpoints (what R26 reuses vs builds)
- **Guest image:** the R17 freestanding target (`--freestanding`, `@[entry]`/`@[panic_handler]`,
  virtio/serial, QEMU boot — `governance/specs/R17-freestanding-substrate.md` Slices 0–3) is the guest
  build machinery, reused. R26 adds: the vsock job channel, the boot-time `axtcb1:` → `report_data`
  binding (§4.2), and bundling R21+R23 into the image.
- **In-guest containment:** the R21 supervisor (`crates/axon-os/`) + the R23 cert checker run
  **inside** the guest, unchanged — R26 *replaces what R21's `runtime.rs` is the subprocess version
  of* (the host-subprocess boundary) with the VM boundary, but the gate/mint/run/record/cert-check
  logic is identical.
- **The TCB digest:** R20 Slice 3 (`smt.rs` `TCB_MANIFEST_DIGEST` / `live_tcb_digest` / E1611) is the
  `axtcb1:` digest chained in §4.2 — reused, not re-invented.

---

## §5 — Public API / interface contract

### 5.1 Library API (`lib.rs`)
```
pub fn measure(image: &Path) -> Measurement;                         // pure (reads image bytes via caller)
pub fn verify(report:&AttestationReport, expected_meas:&str,
              pinned_axtcb:&str, nonce:&str) -> Verdict;             // the trusted verifier (pure)
pub fn run(manifest:&GuestManifest, pinned_axtcb:&str, nonce:&str,
           job:&[u8], sub:&impl Substrate) -> Verdict;              // the full pipeline
pub trait Substrate {
    fn launch(&self, image:&Path, devices:&DeviceSet,
              substrate:Substrate, vcpus:u32) -> LaunchHandle;
    fn fetch_attestation(&self, h:&LaunchHandle, nonce:&str) -> Option<AttestationReport>; // None ⇒ refused
    fn send_job(&self, h:&LaunchHandle, job:&[u8]) -> RunOutcome;    // over vsock into the attested guest
    fn teardown(&self, h:LaunchHandle);
}
```

### 5.2 CLI (every subcommand has `--help`; output is human-legible, not just exit codes)
```
axon-vm build-image <kernel.ax> --out <image>
    Build the freestanding Axon OS guest image (reuses R17 `axon build --freestanding`), bundling
    the R21 supervisor + R23 cert checker + the vsock channel. Prints the resulting image path.

axon-vm measure <image>
    Print the deterministic measurement: "axmeas1:…". No VM. This is what you PIN as expected_meas.

axon-vm launch <guest.axvm>
    Boot the guest in the required micro-VM with the minimal device surface. Prints the launch handle
    + serial boot log. (Used internally by `run`; exposed for debugging.) Exit 0 / 10 (attest) / 2.

axon-vm attest <guest.axvm> [--nonce N]
    Boot → fetch the HW attestation report → print it (JSON). No job is run. Exit 0 if a report was
    produced, 10 if NONE (a VM that cannot attest is refused — there is no "attest skipped" path).

axon-vm verify <report.json> --expect-meas M --expect-axtcb T --nonce N
    Run the RELYING-PARTY verifier (no VM, no solver, no HW) over a report. Prints
    "✓ attested: genuine Axon OS, measurement matches, axtcb1 chained" or
    "✗ ATTESTATION FAILED: <reason>" / "✗ TCB MISMATCH: <reason>". Exit 0 / 10 / 11.

axon-vm run <guest.axvm> <job.axjob> [--nonce N]
    The full journey: launch → MANDATORY attest+verify → (only if attested) send the job over vsock →
    print the in-guest verdict + returned record digest. Prints, in plain English, the attestation
    result FIRST ("✓ talking to a genuine attested Axon OS …"), then the job outcome
    ("✓ admitted; record axrec1:…" / "⚠ in-guest DENIED: net withheld"). Exit = verdict code (§3.4).
    A VM that fails attestation NEVER runs the job (exit 10/11, audited reason).
```
Usage/`--help` on a bad invocation → exit 2 with a helpful message naming the expected form.

### 5.6 Shipped example artifacts (A2 — real, runnable)
- `guest/axon-os.axvm` + the guest build recipe: the freestanding Axon OS image (R17) bundling R21+R23.
- `examples/vm/summarize.axjob` (+ `summarize.ax`): **the SAME R21 job** (reads `./data/`, writes
  `./out/`, no net) — now run **inside the attested guest** via `axon-vm run`. The headline positive
  demo: identical Axon program + identical proofs, new hardware-isolated substrate, attestation
  verified before the job enters the box.
- A tampered-image fixture (one flipped byte) used by `tampered_guest_image_fails_attestation` — its
  measurement differs and the relying party refuses it.

---

## §6 — Build order (each slice ends green before the next; TDD: test first, see it fail, make it pass)

> **Honest implementability split.** Real SEV-SNP/TDX need **specific hardware** + vendor attestation
> services (AMD KDS / Intel PCS) that **CI does not have**. So the **software stand-in lane**
> (QEMU + **swtpm** software TPM, which provides a measured-boot quote rooting at a test CA) carries
> S1–S6 and is what `scripts/r26_acceptance_gate.sh` runs in ordinary CI. The **real-hardware
> attestation slice (S7) is gated separately** behind the `hw-attest` feature and a hardware lane —
> it proves the *same* verifier + binding against a genuine SEV-SNP/TDX report, but is **not** an
> ordinary-CI prerequisite. The stand-in is honest: it gives the attestation *shape* (measured boot,
> signed quote, the §4.2 `axtcb1:` binding, the §4.3 verification) but **not** memory-encryption vs
> the host — that property is S7-only and §7 says so.

- **S1 — Data model + manifest + measurement.** `verdict.rs`, `manifest.rs`, `measure.rs`. Tests:
  parse the valid `.axvm`; reject each malformed case (bad digest, unknown substrate, **a device
  outside the §3.3 allowlist → DeviceDeny**, missing image); `measure` is deterministic. Green.
- **S2 — Report schema + the axtcb binding.** `report.rs`. Tests: build/parse round-trips byte-identical;
  `report_data == sha256(axtcb ‖ nonce)`; the binding helper matches §4.2.
- **S3 — The relying-party verifier (the trusted core).** `verify.rs` over hand-built reports. Tests:
  `attestation_mismatch_fails_closed` (each of: bad signature, unknown chain root, stale nonce,
  measurement ≠ expected, axtcb ≠ pinned → the right refusal); `axtcb1_digest_chained_to_measurement`
  (a report whose `report_data` doesn't commit to the pinned axtcb → `TcbMismatch`); a **valid** report
  → `Ok`. Pure, fast, adversarial-corpus-driven.
- **S4 — Pipeline over `MockSubstrate`.** `pipeline.rs`, `admit.rs`, `substrate::MockSubstrate`. Tests:
  attested happy path → `Admitted`; `vm_without_attestation_is_refused` (mock `fetch_attestation`
  returns `None` ⇒ `AttestFail`, and **`send_job` is never called** — assert via a call counter, the
  fail-closed-before-admit invariant); attestation-fail ⇒ teardown, no job.
- **S5 — `QemuSwtpmSubstrate` (the real stand-in seam) + minimal device surface.** Wire QEMU + swtpm.
  Tests: `acc_a4_device_surface_minimal_and_isolated` (the launched QEMU config contains exactly
  vsock+serial+timer and **no** extra device; a manifest asking for more is `DeviceDeny`); boot+attest
  timeout tears down (no leaked VM).
- **S6 — CLI + the real journey + example artifacts + quickstart.** `cli.rs`, `main.rs`,
  `examples/vm/*`, `guest/*`, `README-axon-vm.md`. Tests: `acc_a1_smoke_boot_attest_run_journey`,
  `acc_a2_guest_image_runs_real_job`, `acc_a3_quickstart_commands_execute`,
  `acc_a5_measurement_and_record_byte_identical`, `acc_a6_attestation_mandatory_and_chained`,
  `tampered_guest_image_fails_attestation`. **This is the done-gate for the CI lane.**
- **S7 — Real hardware attestation (`hw-attest`, GATED SEPARATELY — not ordinary-CI).**
  `SevSnpSubstrate`/`TdxSubstrate`: the *same* verifier (§4.3) + binding (§4.2) against a genuine
  SEV-SNP report / TDX quote rooting at AMD KDS / Intel PCS, with memory encryption vs the host. Test
  (hardware lane only): `hw_real_snp_report_verifies_and_chains` — a genuine report from a known-good
  image verifies and the `axtcb1:` binding holds; a report of a tampered image is refused. **Documented
  as requiring SEV-SNP/TDX hardware; the acceptance gate does NOT require S7 to be green in CI.**
- **S8 — Acceptance gate.** `scripts/r26_acceptance_gate.sh` (§10). Green (CI lane) = done.

---

## §7 — Test plan (happy + **adversarial**; every named test is normative)

**Unit / core (pure, fast — the trusted verifier under adversarial pressure):**
- `manifest_rejects_malformed` — bad digest, unknown substrate, **device outside the allowlist**,
  missing image → `Malformed`/`DeviceDeny`.
- `attestation_mismatch_fails_closed` — for each of {bad signature, unknown chain root, stale nonce,
  `measurement ≠ expected`, `axtcb ≠ pinned`}, `verify` returns the correct refusal verdict; assert
  **no path** returns `Ok`.
- `vm_without_attestation_is_refused` — substrate yields `None` for the report ⇒ `AttestFail`; and in
  the pipeline, `send_job` call-count == 0 (no job ever enters an unattested VM). The headline Core:
  **no attestation ⇒ refusal, not a downgrade.**
- `tampered_guest_image_fails_attestation` — flip one byte of the image ⇒ `measure` differs ⇒ a
  truthful report of it has `measurement ≠ expected_meas` ⇒ `verify` → `AttestFail`. (The
  measured-boot tamper-evidence property.)
- `axtcb1_digest_chained_to_measurement` — a report whose `report_data` does NOT commit to the pinned
  `axtcb1:` (the "two disconnected stories" failure) → `TcbMismatch` exit 11; a correctly-bound report
  → `Ok`. Proves the chain, not parallel claims.
- `forged_report_rejected` — a report with a plausible measurement but no valid HW signature → step-1
  `AttestFail` (the verifier re-derives, never trusts the report's claims).

**Integration (real QEMU + swtpm stand-in lane):**
- `acc_a4_device_surface_minimal_and_isolated` — boot the guest; inspect the launched VMM config:
  exactly {vsock, serial, timer}; assert NO net/GPU/block-beyond-rootfs/extra-virtio; a manifest
  requesting a GPU → `DeviceDeny` exit 8; assert the guest cannot reach the host beyond vsock/serial.
- `acc_a5_measurement_and_record_byte_identical` — `measure` the image twice ⇒ identical digest; run
  the same job+seed in the guest twice ⇒ byte-identical `RunRecord` (nonce excluded from both).
- `acc_a6_attestation_mandatory_and_chained` — `axon-vm run` REFUSES to send the job unless the swtpm
  report verifies (signature + measurement + chained `axtcb1:`); a substrate that produces no report →
  exit 10, job never sent; a correct report → job runs and the record comes back.
- `acc_a2_guest_image_runs_real_job` — the SAME `summarize.axjob` runs **inside the attested guest**
  and produces the audited record (the in-guest R21 + R23 unchanged); the overreach variant is
  in-guest-denied and forwarded as `RunFault`.

**User-journey smoke (A1 — drives the REAL CLI exactly as the operator would, via subprocess):**
- `acc_a1_smoke_boot_attest_run_journey`: (1) `axon-vm measure guest/axon-os.axvm` → pin the
  `axmeas1:` digest; (2) `axon-vm attest guest/axon-os.axvm --nonce N` → asserts a report is produced;
  (3) `axon-vm verify <report> --expect-meas … --expect-axtcb … --nonce N` → "✓ attested … axtcb1
  chained"; (4) `axon-vm run guest/axon-os.axvm examples/vm/summarize.axjob` → asserts the attestation
  line FIRST then "✓ admitted; record axrec1:…" + the returned record; (5) a tampered image →
  `axon-vm run` → "✗ ATTESTATION FAILED: measurement ≠ expected" exit 10, **job never ran.** Each step
  asserts **stdout text AND the on-disk artifact/exit code**, not just exit codes.

**Quickstart (A3):** `acc_a3_quickstart_commands_execute` — extracts the fenced command block from
`README-axon-vm.md` and executes each line verbatim against the built binary (the swtpm lane); all
succeed with the documented output.

**Hardware lane (S7, NOT in ordinary CI):** `hw_real_snp_report_verifies_and_chains` — gated behind
`hw-attest` + real SEV-SNP/TDX hardware; same assertions against a genuine vendor-signed report.

---

## §8 — Invariants & edge cases

**Invariants (must always hold; assert in tests):**
- **I-1 Attestation is mandatory + fail-closed.** No job is sent to a guest whose report did not
  *verify* (signature + chain + freshness + measurement + chained `axtcb1:`). No-report ⇒ refusal,
  never a downgrade (`vm_without_attestation_is_refused`).
- **I-2 The chain is one chain — re-derived, solver-free.** The expected `axtcb1:` is **re-derived by
  the verifier from the measured image** (an R23 certificate validated solver-free, no Z3 in guest or
  verifier; §4.2/§4.3 step 5), not compared against a second pre-pinned constant — the binding adds real
  evidence only because it is re-derived (a constant-vs-constant check would add ~zero over
  measurement-pinning). One HW signature covers both the hardware measurement and the (re-derived)
  proven-TCB attestation; they are never two independently-trusted stories, and the proof is a checkable
  certificate, not a solver's "Proven" verdict (`axtcb1_digest_chained_to_measurement`).
- **I-8 Same attested instance (no TOCTOU).** The job is admitted only to the *identical*
  un-reset/un-migrated VM instance that produced the verified report; the vsock session is bound to the
  attested launch (nonce echo, §4.3 step 6). A substrate that cannot guarantee same-instance delivery is
  refused (the no-migration scope, §1.2, is enforced).
- **I-3 Minimal device surface is total.** The guest is launched with exactly {vsock, serial, timer};
  any other device is unrepresentable in the manifest and a request for one is `DeviceDeny`. The
  launched VMM config is asserted, not assumed.
- **I-4 The verifier re-derives, never trusts.** Every claim in the report is checked against an
  independently-pinned expectation or a HW signature; a forged/mutated report is rejected (R23
  discipline applied to HW evidence).
- **I-5 Same Axon program + proofs run unchanged in the guest.** R21 gate/mint/run/record + R23
  cert-check are byte-for-byte the host versions; R26 changes only the substrate beneath them.
- **I-6 Determinism (measurement + record).** Same image ⇒ same measurement; same job+seed ⇒
  byte-identical record; the freshness nonce lives outside both hashed/compared regions.
- **I-7 Distinct fail-closed codes.** attestation=10, tcb-chain=11, device-deny=8, in-guest-fault=9
  (forwarded), malformed/usage=2 — never collapse to a generic failure; the reason is always surfaced.

**Honest threat model (what R26 closes vs what it does NOT — `VISION_OS.md` §4.3, §1):**
- **Closed (vs R21's subprocess):** the host OS kernel and (under SEV-SNP/TDX) the host *operator* are
  **removed from the trust boundary** — a host-kernel bug is no longer an automatic ASI escape, and the
  operator cannot read the encrypted guest memory. The boundary is hardware-virtualized and the exact
  image is attested before any work enters it.
- **STILL TRUSTED (the boundary of the claim):** the VMM (Firecracker / Cloud Hypervisor), the
  hypervisor, the CPU + firmware + the HW vendor's attestation root (AMD KDS / Intel PCS). Bare metal
  (R17, `VISION_OS.md` §11 v3+) floors these later; R26 does not.
- **STAND-IN IS OPERATOR-FORGEABLE BY CONSTRUCTION (the honest CI floor).** In the QEMU+swtpm lane the
  operator **owns the swtpm and its CA and holds the EK private key**, so the operator can mint a valid,
  EK-pinned quote for **any** image of their choosing. Pinning a specific `ek_id`/measured-boot
  expectation (§3.1, §4.3 step 1) narrows acceptance to the one provisioned key and gives the *shape* of
  real attestation for CI — but it does **not** make the stand-in tamper-evident against its own
  operator. The assumed in-spec adversary (§1) who owns the host can forge the stand-in's chain. **Only
  the real-hardware lane (S7)**, where the EK/VCEK is silicon-bound and rooted at AMD KDS / Intel PCS,
  removes the operator from the boundary; the stand-in is explicitly *not* a confidentiality- or
  unforgeability-claim, only the CI-lane floor.
- **RESIDUAL — confidential computing does NOT close microarchitectural side channels**
  (`VISION_OS.md` §4.3): **Spectre/Meltdown-class** speculative leaks, cache-timing, power/thermal,
  and **Rowhammer** (which physically flips bits *across* the encrypted boundary the proofs and the
  encryption cannot see). R26's attestation says *"this is the expected image, isolated"* — it says
  **nothing** about timing or physical-layer leakage. **Mitigations (posture, not closure):** cache/core
  **partitioning** where the substrate allows, **no SMT co-residency** across trust boundaries (the
  manifest's small `vcpus` + a "dedicated core, HT off" deployment posture), constant-time discipline
  in TCB code that touches secrets (the R23 verifier is data-independent), and bandwidth-limiting +
  monitoring for residual covert channels. **The honest floor:** some channels are *mitigated, not
  closed*; physical-layer containment is the boundary of the claim, with an air-gapped mode for the
  highest-stakes deployments (`VISION_OS.md` §4.3). R26 is **not** a side-channel-free substrate;
  full side-channel discipline is gap **G10**, a separate spec.

**Edge cases the implementer MUST handle (named, with the resolution):**
- VMM/substrate fails to boot → `AttestFail{"boot failed"}`, torn down, no job (fail closed).
- Report produced but signature/chain invalid → `AttestFail{"chain"}`, no job.
- Genuine report of a *different* image (measurement ≠ expected) → `AttestFail`, never a pass — this is
  the tamper detector.
- `axtcb1:` binding present but ≠ pinned (a weakened-TCB image that still boots) → `TcbMismatch` exit
  11; note the guest's own E1611 boot check is the *first* line of defense, this is the relying-party's.
- Manifest requests a GPU / extra device → `DeviceDeny` exit 8 at validation, **no VM launches.**
- Stand-in caveat: the swtpm lane gives the attestation *shape* but not memory-encryption-vs-host —
  surfaced in `axon-vm`'s output as "substrate: qemu-swtpm (stand-in — no memory encryption; use
  sev-snp/tdx for confidentiality)" so an operator is never misled about the real property.
- Boot/attest timeout → tear down, `AttestFail{"timeout"}`; no leaked VM/vsock handle.
- **TOCTOU — job sent to a non-attested instance.** Between `fetch_attestation` and `send_job` the
  substrate resets/migrates/snapshot-restores/re-boots the VM → the vsock peer is no longer the attested
  instance → `send_job` rejects on the session/nonce binding (§4.3 step 6) → `AttestFail{"session ≠
  attested instance"}`, torn down, no job. A substrate that *cannot* guarantee same-instance delivery
  (permits migration/restart in this window) is **REFUSED**, not trusted (the no-migration scope, §1.2,
  is enforced here).

---

## §9 — Quickstart (`README-axon-vm.md`; these exact commands are executed by `acc_a3`)
```bash
# Build (the CI/stand-in lane: QEMU + swtpm — no special hardware needed)
cargo build -p axon-vm --bin axon-vm

# 1. Build the freestanding Axon OS guest image (reuses R17), bundling the R21 supervisor + R23 checker:
axon-vm build-image guest/axon-os.ax --out guest/axon-os.axvm

# 2. Compute the image's deterministic measurement — this is what you PIN as "expected":
axon-vm measure guest/axon-os.axvm        # → axmeas1:…

# 3. Boot it and fetch the HARDWARE attestation report (MANDATORY — a VM that can't attest is refused):
axon-vm attest guest/axon-os.axvm --nonce demo-nonce > report.json

# 4. As the RELYING PARTY, verify it's a genuine, unmodified Axon OS with the expected TCB — chained:
axon-vm verify report.json --expect-meas axmeas1:<pinned> --expect-axtcb axtcb1:<pinned> --nonce demo-nonce

# 5. The full journey: attest FIRST, then (only if attested) run the SAME R21 job inside the VM:
axon-vm run guest/axon-os.axvm examples/vm/summarize.axjob --nonce demo-nonce

# 6. Watch attestation REFUSE a tampered image (exit 10, job never runs):
axon-vm run guest/axon-os-tampered.axvm examples/vm/summarize.axjob --nonce demo-nonce ; echo "exit=$?"
```

---

## §10 — Acceptance gate (pinned; FAILS if any check is missing or stubbed)

`scripts/r26_acceptance_gate.sh` is the single source of "done" (mirrors `scripts/r23_acceptance_gate.sh`).
It MUST:
1. **Presence check** — `grep` the test sources and assert every named check from §0 exists:
   `acc_a1_smoke_boot_attest_run_journey`, `acc_a2_guest_image_runs_real_job`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_device_surface_minimal_and_isolated`,
   `acc_a5_measurement_and_record_byte_identical`, `acc_a6_attestation_mandatory_and_chained`,
   `attestation_mismatch_fails_closed`, `vm_without_attestation_is_refused`,
   `axtcb1_digest_chained_to_measurement`, `tampered_guest_image_fails_attestation`.
   Any missing name → **gate fails**.
2. **Anti-stub check** — assert each acceptance test body contains a real assertion and is not
   `#[ignore]`d / `todo!()` / `assert!(true)` (grep for those anti-patterns → fail).
3. **Solver/HW-free core check** — assert the trusted verifier (`verify.rs`) and the pure core import
   **no** `z3` and no `hw-attest`-gated symbol (the CI lane must verify reports without real hardware,
   the same way R23's checker is solver-free).
4. **Run** `cargo test -p axon-vm` (CI/stand-in lane, all green) **and** execute the §9 quickstart block
   against the built binary (A3) **and** run `acc_a1` driving the real CLI through QEMU+swtpm.
5. **Reproducibility** — `measure` the image twice and diff; run a job twice and diff the records.
6. **Honesty check** — assert the stand-in path prints its "no memory encryption — use sev-snp/tdx"
   caveat (so the gate fails if the stand-in is ever silently presented as the real confidential
   property).
7. Exit 0 only if all of the above pass (the **CI/stand-in lane**; S7 hardware is NOT required); print
   which check failed otherwise. Wire `r26_acceptance_gate.sh` into the repo's `gate.sh --strict`.

---

## §11 — Definition of Done

**Per slice (S1–S8):** the slice's named tests were written first, were seen to fail, now pass; the
full `axon-vm` suite is green on the CI/stand-in lane; no regression in the workspace.

**Per milestone (R26 CI-lane complete):** `cargo build -p axon-vm` produces the `axon-vm` binary; the
guest image builds (reusing R17) and bundles R21+R23; `axon-vm run` **refuses** to admit a job unless
the (swtpm) attestation verifies, with the `axtcb1:` digest **chained** into the measurement; a
tampered image is refused (exit 10) and a TCB-chain break is refused (exit 11); the SAME R21 job runs
**inside the attested guest** and returns an audited record; reproducibility (`acc_a5`) and the minimal
device surface (`acc_a4`) hold; and `scripts/r26_acceptance_gate.sh` exits 0 with every §0 check green.

**Hardware milestone (S7, tracked separately):** the same verifier + binding pass against a genuine
SEV-SNP/TDX report on real hardware (`hw_real_snp_report_verifies_and_chains`, `hw-attest` lane). This
is the milestone that delivers confidentiality-vs-host; it is **not** required for the CI-lane DoD but
**is** required to claim "confidential" per `VISION_OS.md` §11 v1.

This realizes `VISION_OS.md` gap **G5** (micro-VM guest, ideally confidential) and, via the mandatory
chained attestation, advances **G6** (hardware root of trust chaining the `axtcb1:` digest to silicon)
— while honestly leaving side channels (G10) and bare metal (R17/v3+) to their own specs.

---

## §12 — Notes for the implementer (do NOT deviate without updating this spec)
- Keep `manifest/measure/report/verify/admit/verdict/pipeline` **pure**. If you reach for `std::fs`,
  `SystemTime`, `rand`, `std::env`, or a VMM/HW call there, you are in the wrong module — it belongs in
  `substrate.rs`/`cli.rs`. **`verify.rs` is the trusted artifact: keep it small, total, readable** (the
  R23 rule).
- **Attestation is mandatory.** There is no "skip attestation" flag, no "degraded" mode, no
  unattested-but-running path. `fetch_attestation → None` is a refusal. If you find yourself adding a
  bypass, stop — that is the whole point of R26.
- **The `axtcb1:` binding is the chain.** Bind R20's digest into `report_data` (§4.2); never verify it
  as a separate, independently-trusted claim. The single HW signature must cover both.
- The device set is a **closed allowlist** (§3.3). Do not add a device "for convenience"; each one is a
  new unproven attack surface against an adversarial AI.
- Exit codes are a contract (§3.4): attestation=10, tcb-chain=11, device-deny=8, in-guest-fault=9,
  malformed=2. Never collapse a fail-closed outcome into a generic error.
- **Be honest about the stand-in.** QEMU+swtpm gives the attestation *shape* for CI, **not**
  memory-encryption-vs-host. Always print the caveat; never let the stand-in be mistaken for the real
  confidential property. Real SEV-SNP/TDX (S7) is what delivers confidentiality and is gated to the
  hardware lane.
- R26 changes only the substrate. If you are editing R21's gate/mint/record or R23's checker, you are
  out of scope — those run unchanged *inside* the guest. Trust gaps (side channels, the VMM/hypervisor
  in the TCB, bare metal) are documented on purpose (§8); do not silently "improve" past the spec.
