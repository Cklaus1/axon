# Tech Spec — R31: Extended TCB Attestation (chain R27 + R28 + R29 into axtcb1)

**Spec ID:** `R31-extended-tcb-attestation`
**Status:** ✅ Landed (re-verified 2026-07-18) — `axtcb1_ext` chains the R27/R28/R29 component
binary hashes into the R26 measurement (`axon-vm attest --extended-tcb`, `axon-vm-report/2`; R26
baseline report unchanged without the flag). `scripts/r31_acceptance_gate.sh` ALL PASS; feat commit
`d2d6dd4` (798-line `axon-attest` impl + 12 tests). This header said "Draft" long after the code
shipped — REQUIREMENTS.md and ROADMAP.md were both corrected for this back on 2026-07-18, but the
spec's own source-of-truth header was never actually touched until now. Same staleness class as
R17/R21/R22/R23/R32, caught by the same outer-loop sweep (`EXECUTION_MODEL.md` §2) — a reminder
that "REQUIREMENTS.md says it's fixed" and "the spec file itself is fixed" are not the same claim.
**Implements:** the full-stack host-safety attestation gap identified after R26: R26 measures the
*kernel image* and chains it to silicon, but a relying party cannot know from the R26 report alone
whether the host's kill-switch binary, audit ledger writer, or compliance monitor thread are also
the expected versions. An attacker who can replace the `axon-os` binary silently disables the
kill-switch (`@[corrigible]` + supervisor latch) even if the kernel measurement is intact. R31
closes this gap by extending the `axtcb1` measurement to chain **all four safety-stack components**
into one digest that a relying party can verify in a single check.
**Depends on:**
- `governance/specs/R26-confidential-microvm-substrate.md` — R26 attestation infrastructure,
  `axtcb1:` digest format, `AttestationReport` schema, `crates/axon-attest`, `axon-vm attest`
- `governance/specs/R27-corrigibility-resource-bounds.md` — `axon-os` kill-switch binary, latch,
  ledger, coalition modules folded into the R20 `axtcb1:` set (A6 of R27)
- `governance/specs/R28-capability-audit-ledger.md` (landed) — `axon-audit` binary, the durable
  ledger writer that records every capability-bearing event; `crates/axon-audit`
- `governance/specs/R29-continuous-compliance-monitor.md` (landed) — compliance monitor thread
  embedded in `axon-os`; watches for policy violations in real time; not a separate binary
**Audience:** an implementer who builds *strictly* against this document and reads only it.

```spec-meta
id: R31-extended-tcb-attestation
status-claim: Landed
depends-on: R26-confidential-microvm-substrate, R27-corrigibility-resource-bounds, R28-capability-audit-ledger, R29-continuous-compliance-monitor
blocks: R32-formal-corrigibility-proof, R33-cross-vm-safety-quorum
blocked-by: none
supersedes: none
related: R36-full-asi-os
conflicts-with: none
reserves: none
evidence: scripts/r31_acceptance_gate.sh (re-verified 2026-07-18)
```

> **Read this framing first.** R31 does **not** change the measurement algebra R26 established, nor
> the kill-switch R27 enforces, nor the audit rules R28/R29 will encode. What R31 changes is the
> **scope of what is measured**: from "the kernel image alone" to "the kernel image + every binary or
> module that constitutes the host safety stack." A relying party holding a valid R31 extended report
> knows not just what kernel is running, but that the *entire* host safety stack — kill-switch,
> ledger writer, compliance monitor — is the expected, unmodified version. This is the load-bearing
> guarantee: an attacker who can swap any single component changes the extended digest, and the
> relying party rejects the run before any work enters the system.

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey: `axon-vm attest --extended-tcb` produces a report where all 4 components appear | §5, §7 | `acc_a1_smoke_extended_tcb_journey` |
| **A2** | Byte-identical across runs: same binaries → same extended axtcb1 digest | §4.3, §7 | `acc_a2_byte_identical_across_runs` |
| **A3** | Quickstart commands execute: the §7 block runs verbatim against the built binary | §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Measurement is pure: no side effects; deterministic; no I/O in the core | §4.3, §7 | `acc_a4_hermetic_isolated_timeout` |
| **A5** | Tamper detection: replacing any one component binary changes the extended digest | §4.2, §7 | `acc_a5_tampered_component_detected` |
| **A6** | Canonical chaining order: kernel → axon-os → axon-audit → monitor (deterministic) | §4.1, §7 | `acc_a6_chaining_order_canonical` |
| **Core** | Missing component fails closed: absent binary → measurement refused, not silently omitted | §4.2 | `missing_component_fails_closed` |
| **Core** | Component detail in report: each component's path, size, and sha256 appear in the JSON | §3.2 | `component_version_in_report` |
| **Core** | R26 baseline backward-compatible: `axon-vm attest` (no flag) still works with kernel-only digest | §3.3, §5 | `r26_baseline_backward_compatible` |
| **Core** | Boot gate: `axon-vm run --extended-tcb` gates the boot on full-stack attestation | §4.4, §7 | `extended_tcb_wired_into_run` |
| **Gate** | The acceptance gate itself fails if any check above is missing or stubbed | §-Gate | `scripts/r31_acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does

R26 introduced mandatory attestation of the guest kernel image: the relying party verifies a
hardware-signed report whose measurement covers the kernel bytes and whose `axtcb1:` digest is
chained into the same signature. This is a strong baseline — it proves the kernel has not been
tampered with and is the expected Axon OS image.

But the host safety stack is now **four layers**, not one:

| Layer | Binary / artifact | Safety property it carries |
|---|---|---|
| R26 | Guest kernel image (`vmlinuz`) | Isolation substrate, hardware boundary |
| R27 | `axon-os` binary (kill-switch) | Supervisor latch, resource-acquisition bound, coalition ceiling |
| R28 | `axon-audit` binary (ledger writer) | Durable, tamper-evident audit trail of all capability events |
| R29 | Compliance monitor thread (inside `axon-os`) | Real-time policy-violation detection |

R26's `axtcb1:` covers only layer one. An attacker who can write to the host filesystem — without
touching the kernel image — can replace the `axon-os` binary with a version whose supervisor latch
always returns `Clear`, silencing the kill-switch. The relying party's R26 report would still verify
(kernel measurement is intact), but the safety guarantee it implies would be false.

R31 extends the `axtcb1` measurement to chain all four layers. The extended form —
`axtcb1_ext:` — is a single SHA-256 digest over the concatenation of all four component hashes
plus a version tag. A relying party verifying an R31 report knows the entire host safety stack is the
expected version; any replacement of any single component produces a different `axtcb1_ext:` and is
refused before any work enters the system.

### 1.2 What it explicitly does NOT do

- **No new isolation.** R31 widens measurement scope; it does not change the hardware boundary, the
  micro-VM device surface, or any Axon-level containment. Those are R26/R27 territory, unchanged.
- **No defense against host OS compromise** — an adversary with root on the host can replace any
  binary at measurement time and recompute the hashes consistently. R31 detects *binary substitution*
  that happens before or between measurements; a root adversary who controls the measurement process
  itself is out of scope (that boundary requires real hardware confidential computing, R26 §8 S7).
- **No dynamic integrity monitoring.** R31 measures binaries *at attestation time*; it does not
  watch for in-process code injection or JIT patching after the measurement is taken. Dynamic code
  injection defense requires memory encryption (SEV-SNP/TDX, R26 §3.1) — it is not a software-TPM
  claim.
- **No new binary formats or build systems** for R28/R29 — R31 specifies *how to measure* whatever
  artifact R28/R29 produce; it does not constrain how those artifacts are built.

### 1.3 Interface & tech constraints

- **Interface:** extends `axon-vm` (R26) with `--extended-tcb` flag on `attest` and `run`.
- **Language/crate:** extends `crates/axon-attest` (R26's measurement crate). No new top-level
  binary. Allowed new deps: none — `sha2` is already vendored, `serde_json` is already in scope.
- **Perf/security:** the extended measurement is **pure** — it reads binary bytes, hashes them, and
  produces a deterministic digest. No I/O beyond file reads; no clock, no random. The whole extended
  measurement path belongs in `measure.rs` (see §2), not in any impure seam.
- **Fail closed on every ambiguity:** a component whose path does not exist, is a symlink to a
  non-regular file, or produces a read error causes `measure_extended` to return `Err`, never a
  partial measurement with a silent zero-hash placeholder (except for the one explicitly sanctioned
  case described in §4.1 below for the R29 monitor slot).

---

## §2 — Architecture & modules

R31 extends `crates/axon-attest/src/` — the pure measurement crate R26 established. All new logic
is pure (no I/O, no clock, no random).

```
crates/axon-attest/src/
  measure.rs       EXTENDED: add `measure_extended(ComponentPaths) -> Result<ExtMeasurement, MeasureError>`.
                   Reads each component's bytes from the caller (I/O injected by the CLI layer),
                   chains them per §4.1, returns `ExtMeasurement`. PURE after byte-injection.  [PURE]
  component.rs     NEW: `ComponentEntry { name, path, size, sha256 }` + the 4-slot enum.        [PURE]
  report.rs        EXTENDED: `AttestationReport` gains `axtcb1_ext` + `components` fields (§3.2).[PURE]
  verify.rs        EXTENDED: when `--extended-tcb` flag is set, verify `axtcb1_ext` in addition
                   to R26's existing `axtcb1` check. Old path unchanged (§3.3).                 [PURE]
  cli.rs           EXTENDED: `attest --extended-tcb`, `run --extended-tcb`; reads component paths
                   from flags or the manifest, calls `measure_extended`, emits extended report.  [I/O seam]
crates/axon-attest/tests/
  r31_acceptance.rs   The A1–A6 + Core acceptance checks (named exactly per §0).
scripts/r31_acceptance_gate.sh   The pinned gate (§-Gate).
```

**The I/O boundary is explicit.** `measure.rs` and `component.rs` receive `&[u8]` byte slices, not
file paths. The CLI layer (`cli.rs`) reads the files and injects the bytes — the same discipline R26
applied to `verify.rs`. This makes every measurement step testable with in-memory byte vectors, no
filesystem fixture required for unit tests.

**Dependency graph (R31 additions only):**
```
cli [I/O] → measure_extended [PURE] → component [PURE]
           → report [PURE, extended]
verify [PURE, extended] → report
```
Nothing new is impure beyond the CLI edge that already existed. No cycles added.

---

## §3 — Data model

### 3.1 `ComponentEntry` — per-component measurement record
```rust
struct ComponentEntry {
    name:   String,    // canonical slot name: "kernel" | "axon-os" | "axon-audit" | "monitor"
    path:   String,    // filesystem path at measurement time (or "(embedded in axon-os)" for monitor)
    size:   u64,       // byte count of the measured binary (0 for the monitor slot — see §4.1)
    sha256: String,    // hex-encoded SHA-256 of the binary bytes
}
```

Four entries are always present in the report, in canonical order (§4.1). The `monitor` slot is
special: the R29 compliance monitor is a thread embedded in the `axon-os` binary, not a separate
artifact. Its `sha256` is set to the same value as the `axon-os` component's `sha256`, its `path` is
the string literal `"(embedded in axon-os)"`, and its `size` is 0. This is **documented explicitly**
and is the only case where a component hash is derived from another component's bytes rather than its
own. See §4.1 for the rationale.

### 3.2 `AttestationReport` — R31 extension (JSON, schema `axon-vm-report/2`)
The R26 `axon-vm-report/1` gains two new fields in the `measurement` object:

```json
{
  "schema": "axon-vm-report/2",
  "measurement": {
    "digest":      "...",
    "axtcb1":      "axtcb1:...",
    "axtcb1_ext":  "axtcb1-ext:...",
    "components": [
      {"name": "kernel",     "path": "dist/guest/vmlinuz",                "size": N, "sha256": "..."},
      {"name": "axon-os",    "path": "target/release/axon-os",            "size": N, "sha256": "..."},
      {"name": "axon-audit", "path": "target/release/axon-audit-writer",  "size": N, "sha256": "..."},
      {"name": "monitor",    "path": "(embedded in axon-os)",              "size": 0, "sha256": "<axon-os-sha256>"}
    ],
    "timestamp": N
  }
}
```

Fields:
- `digest` and `axtcb1` — unchanged from R26; the kernel-only measurement is preserved in full.
- `axtcb1_ext` — the new extended chain (§4.1). Present only when `--extended-tcb` was passed;
  absent in R26-compatible mode (§3.3).
- `components` — the four `ComponentEntry` records (§3.1). Always in canonical order; absent in
  R26-compatible mode.
- `timestamp` — seconds since Unix epoch at measurement time (informational, not hashed).
- `schema` bumped to `"axon-vm-report/2"` when `components`/`axtcb1_ext` are present.

### 3.3 Backward compatibility — the R26 baseline is preserved exactly
`axon-vm attest` (without `--extended-tcb`) produces an `axon-vm-report/1` report identical to what
R26 produced: only `digest` and `axtcb1` in the `measurement` object, no `components`, no
`axtcb1_ext`. The schema string stays `"axon-vm-report/1"`. No existing R26 relying-party verifier
is broken by R31.

When `--extended-tcb` is given, the report is `axon-vm-report/2` and contains **both** `axtcb1` and
`axtcb1_ext`. A relying party that only cares about R26 can still check `axtcb1`; an R31 relying
party additionally checks `axtcb1_ext` and `components`. The two digests are never the same value
(different prefix strings, §4.1), so they cannot be accidentally swapped.

---

## §4 — Core logic

### 4.1 The extended chain construction (`measure_extended`)

The canonical chaining order is **kernel → axon-os → axon-audit → monitor**, fixed and
non-negotiable. Implementers MUST NOT reorder slots, even if a component is absent — order change
is a protocol break that silently produces a different digest for the same set of binaries.

```
axtcb1_ext = "axtcb1-ext:" + hex( sha256(
    sha256(kernel_bytes)         ||   // slot 0 — R26 kernel; REQUIRED
    sha256(axon_os_bytes)        ||   // slot 1 — R27 kill-switch binary; REQUIRED
    sha256(axon_audit_bytes)     ||   // slot 2 — R28 audit ledger binary; see §4.2 for absent case
    sha256(axon_os_bytes)        ||   // slot 3 — R29 monitor (embedded in axon-os; same bytes as slot 1)
    b"axon-tcb-v1\n"                  // version tag; prevents length-extension attacks
))
```

The `"axtcb1-ext:"` prefix distinguishes the extended form from R26's `"axtcb1:"` base. A verifier
that accidentally treats the extended string as a base axtcb1 will see the prefix mismatch and must
fail closed — the prefix is load-bearing for safe fallback.

**Why sha256-of-sha256 for each slot rather than hashing all bytes directly?**
Hashing each component's sha256 separately before the outer hash means:
(a) components can be measured independently and in parallel,
(b) the `ComponentEntry.sha256` fields in the report are the *same* intermediate values the chain
uses — the report is self-verifiable by the relying party without re-reading the binaries, and
(c) altering a single component changes exactly one slot's inner hash, which propagates to the
outer hash; the structure is clean and auditable.

**The version tag** `b"axon-tcb-v1\n"` is a fixed constant appended after the four inner-hash
concatenation before the outer SHA-256. Its purpose is twofold: it prevents a length-extension
attack (an adversary who knows the outer hash of `H0||H1||H2||H3` cannot extend it to a valid hash
of `H0||H1||H2||H3||H3'` without the version tag blocking the extension), and it versions the
chaining protocol so a future `axon-tcb-v2\n` can change the construction without ambiguity.

**The R29 monitor slot** — the compliance monitor (R29) is a thread inside `axon-os`, not a
separate binary. Using `sha256(axon_os_bytes)` for this slot is therefore **correct by construction**:
if the `axon-os` binary is the expected version, the compliance monitor code it embeds is also the
expected version (they are the same ELF). The slot is not redundant — it is a commitment that the
monitor code is present and untampered, anchored to the same binary bytes as the kill-switch.
An implementer who extracts the monitor thread into a separate binary in a future refactor MUST
update this spec and bump `axon-tcb-v2\n`.

### 4.2 Fail-closed cases (missing or unreadable components)

- **`kernel_bytes` absent or unreadable** → `MeasureError::ComponentMissing("kernel")` — refused;
  no extended digest is produced. The kernel is always REQUIRED.
- **`axon-os` absent or unreadable** → `MeasureError::ComponentMissing("axon-os")` — refused.
  Slots 1 and 3 both depend on `axon-os`; a missing binary means neither the kill-switch nor the
  monitor can be verified.
- **`axon-audit` absent or unreadable** — this is the one case where a controlled fallback exists:
  R28 is still in progress, so an absent `axon-audit` binary is **permitted** at measurement time
  with the slot filled by `0x00 * 32` (32 zero bytes). The report MUST record
  `{"name":"axon-audit","path":"<absent>","size":0,"sha256":"0000...0000"}` so the relying party
  knows the slot was unfilled. A relying party that requires a specific `axon-audit` hash MUST
  reject a report with the zero-fill. The zero-fill is a deliberate, auditable sentinel, not a
  silent omission. Once R28 ships, the zero-fill path is removed; `acc_a5_tampered_component_detected`
  MUST test that replacing the real `axon-audit` binary with a different binary changes the digest
  (validating the non-zero-fill path).
- **Symlink or non-regular file** at any component path → `MeasureError::ComponentNotRegularFile` —
  refused. Symlinks are a classic TOCTOU vector; the measurement reads only regular files.
- **Partial read** (file shrinks or is replaced between `open` and `read`) → the read error
  propagates as `MeasureError::IoError`; the outer measurement is refused, not partial.

### 4.3 Determinism (A2/A4)

The extended measurement is a **pure function of the four byte slices** plus the fixed version tag.
There is no clock, no random, no ambient state. Two invocations on the same binaries produce
byte-identical `axtcb1_ext` values. This holds even across different hosts, because:
- The chaining order is fixed (§4.1).
- The version tag is a fixed constant.
- SHA-256 is deterministic.
- The `timestamp` field in the report is **not** included in the hash.

`acc_a2_byte_identical_across_runs` runs `measure_extended` twice on the same bytes in the same
process and asserts equality; `acc_a4_hermetic_isolated_timeout` asserts the function makes no
syscalls beyond the byte reads that are injected by the caller.

### 4.4 Wiring extended measurement into `axon-vm run --extended-tcb` (boot gate)

When `axon-vm run` is invoked with `--extended-tcb`:
1. Before launching the VM, the CLI measures all four component paths (§4.1) and produces the
   extended report.
2. Any `MeasureError` → **refuse to boot**; print the failing component name and exit 12
   (`EXTENDED_TCB_MEASURE_FAIL`; §5.3). The VM is never spawned.
3. The extended `axtcb1_ext` from step 1 is passed to the verifier alongside the R26 `axtcb1`.
4. The verifier must accept **both** — `axtcb1` (kernel chain, R26) AND `axtcb1_ext` (full stack,
   R31) — for the run to proceed. Either mismatch → `AttestFail`, exit 10.
5. Only after both verify does the pipeline proceed to admit the job.

This is realized by `extended_tcb_wired_into_run` (§7): a run with `--extended-tcb` against a
manifest whose expected `axtcb1_ext` does not match the measured extended digest → refused exit 10,
no job admitted.

---

## §5 — API / CLI

### 5.1 `axon-vm` CLI extensions

```
axon-vm attest <guest.axvm> [--extended-tcb]
    [existing] Boot → fetch HW attestation report → print it (JSON).
    [R31] With --extended-tcb: also measure kernel + axon-os + axon-audit + monitor; extend the
    report with axtcb1_ext and the components array. Missing required component → exit 12.
    Output schema: axon-vm-report/2 (with --extended-tcb) or axon-vm-report/1 (without).

axon-vm run <guest.axvm> <job.axjob> [--extended-tcb] [--nonce N]
    [existing] Full journey: launch → attest+verify → send job → return record.
    [R31] With --extended-tcb: the full-stack measurement is verified BEFORE the VM boots;
    any extended-TCB mismatch refuses the boot (exit 12 for measure failure, exit 10 for
    attestation mismatch). The extended measurement is printed alongside the kernel-only one:
    "✓ extended TCB: axtcb1-ext:… (4/4 components verified)" before the job is admitted.

axon-vm verify <report.json> --expect-meas M [--expect-axtcb1-ext E] --nonce N
    [existing] Relying-party verifier.
    [R31] With --expect-axtcb1-ext: also verifies report.measurement.axtcb1_ext == E.
    Prints each component entry and its verification status.
```

### 5.2 Library API (`crates/axon-attest`)

```rust
// component.rs (PURE)
pub struct ComponentEntry { pub name: String, pub path: String, pub size: u64, pub sha256: String }
pub struct ComponentPaths { pub kernel: &[u8], pub axon_os: &[u8],
                            pub axon_audit: Option<&[u8]>, /* None → zero-fill, §4.2 */ }

// measure.rs (PURE after byte injection)
pub fn measure_extended(paths: ComponentPaths) -> Result<ExtMeasurement, MeasureError>;
pub struct ExtMeasurement { pub axtcb1_ext: String, pub components: Vec<ComponentEntry> }

// verify.rs (PURE, extended path)
pub fn verify_extended(report: &AttestationReport, expected_ext: &str) -> Result<(), VerifyError>;
```

### 5.3 Exit-code additions (no collision with R26/R27)

| Code | Const | Meaning |
|---|---|---|
| 10 | `ATTEST_FAIL` | R26-inherited: attestation signature/measurement mismatch |
| 11 | `TCB_MISMATCH` | R26-inherited: axtcb1 chain break |
| 12 | `EXTENDED_TCB_MEASURE_FAIL` | R31 new: component missing/unreadable at extended measurement time |

Code 12 is distinct from 10/11 — it signals a measurement-time problem (can't even read the binary)
rather than an attestation-time mismatch (binary was read and hashes differently than expected).
Never collapse 12 into 10.

---

## §6 — Build order (TDD; each slice: test first, seen to fail, then passes)

| Slice | Deliverable | Pinned check (written first) |
|---|---|---|
| **S1** | `component.rs` data types + the canonical 4-slot ordering; unit tests for ordering invariant. | `acc_a6_chaining_order_canonical` |
| **S2** | `measure_extended` pure function: hash chain + version tag + fail-closed error cases. | `acc_a5_tampered_component_detected`, `missing_component_fails_closed` |
| **S3** | Extended `AttestationReport` schema (`axon-vm-report/2`); round-trip JSON serde. | `component_version_in_report` |
| **S4** | R26 backward compat: `attest` without `--extended-tcb` produces schema `axon-vm-report/1`, unchanged. | `r26_baseline_backward_compatible` |
| **S5** | `verify_extended` in `verify.rs`; extended flag on `axon-vm attest`. CLI I/O seam (file reads injected). | `acc_a1_smoke_extended_tcb_journey`, `acc_a2_byte_identical_across_runs` |
| **S6** | `axon-vm run --extended-tcb` boot gate: measure before VM launch, refuse on fail. | `extended_tcb_wired_into_run` |
| **S7** | Acceptance gate `scripts/r31_acceptance_gate.sh` + quickstart commands test. | `acc_a3_quickstart_commands_execute`, `acc_a4_hermetic_isolated_timeout` |

**Definition of "done" per slice:** the slice's named check existed, was seen to fail, now passes;
the full `axon-attest` suite is green; no workspace regression; R26 baseline tests still pass
(`r26_baseline_backward_compatible` is green at every slice, not just S4).

**Slice risk (S2):** the zero-fill path for `axon-audit` (R28 not yet shipped) must not become a
silent default. Ensure `acc_a5_tampered_component_detected` explicitly tests the non-zero-fill path
(i.e., a real `axon-audit` binary is provided and a tampered copy produces a different digest). If
only the zero-fill path is exercised, the test is vacuous and the gate must reject it (anti-stub
check, §-Gate item 2).

---

## §7 — Test plan (happy + adversarial; every named test is normative)

**Unit (pure, fast — no filesystem, bytes injected):**
- `acc_a6_chaining_order_canonical` — constructing the same 4-component set in different orderings
  always serializes to the canonical `["kernel","axon-os","axon-audit","monitor"]` order; a reordered
  input does not produce the same `axtcb1_ext` as the canonical one (ordering matters).
- `acc_a5_tampered_component_detected` — for each of the four slots: replace that slot's bytes with
  one bit flipped; assert the resulting `axtcb1_ext` differs from the baseline. All four cases must
  be exercised (anti-vacuous guard: assert 4 distinct tamper-detected outcomes).
- `missing_component_fails_closed` — pass `None`/empty bytes for each REQUIRED component (`kernel`,
  `axon-os`); assert `MeasureError::ComponentMissing` is returned, never a partial `ExtMeasurement`.
  Pass `None` for `axon-audit` (optional); assert the zero-fill sentinel appears in `components` and
  the resulting `axtcb1_ext` contains the zero-hash in slot 2 (not a random or missing value).
- `acc_a2_byte_identical_across_runs` — call `measure_extended` twice with the same byte slices in
  the same process; assert `axtcb1_ext` is byte-identical both times.
- `acc_a4_hermetic_isolated_timeout` — `measure_extended` makes no syscalls beyond the injected byte
  reads; assert the pure function is total (no panics on valid inputs, even large ones).
- `component_version_in_report` — a well-formed `ExtMeasurement` serializes to JSON with exactly 4
  `components` entries; each entry has non-empty `name`, `path`, `sha256`; `monitor.sha256 ==
  axon-os.sha256`; `monitor.size == 0`; `monitor.path == "(embedded in axon-os)"`.
- `r26_baseline_backward_compatible` — `AttestationReport` with only R26 fields (no `axtcb1_ext`,
  no `components`) round-trips without error; R26 `verify` still accepts it; `schema` remains
  `"axon-vm-report/1"` unless `axtcb1_ext` is present.

**Integration (real filesystem fixtures, CLI subprocess):**
- `acc_a1_smoke_extended_tcb_journey`:
  1. `axon-vm attest --kernel dist/guest/vmlinuz --extended-tcb` → assert exit 0; parse JSON;
     assert `components | length == 4`; assert `axtcb1_ext` starts with `"axtcb1-ext:"`.
  2. `axon-vm verify <report> --expect-axtcb1-ext <value> --nonce N` → assert
     "✓ extended TCB: 4/4 components verified".
  3. Flip one byte in a copy of the `axon-os` binary; re-run `attest --extended-tcb` against the
     copy; assert `axtcb1_ext` differs from step 1. Assert the `verify` step rejects the modified
     report with the original expected value (exit 10).
- `extended_tcb_wired_into_run` — `axon-vm run job.axjob --extended-tcb` with a manifest whose
  `expected_axtcb1_ext` matches the measured value → job runs; with a mismatched expectation → exit
  10, job never sent (assert via the vsock send-count mock).
- `acc_a3_quickstart_commands_execute` — execute the §8 quickstart block verbatim; assert all
  commands exit 0 and produce the documented output patterns.

**Adversarial:**
- Symlink at a component path → `MeasureError::ComponentNotRegularFile`; refused.
- File replaced between open and read (simulated via an in-process truncation) → `MeasureError::IoError`.
- Report with `axtcb1_ext` prefix wrong (e.g., `"axtcb1:"` instead of `"axtcb1-ext:"`) → `VerifyError::PrefixMismatch`; refused.
- Report where `monitor.sha256 ≠ axon-os.sha256` → `VerifyError::MonitorSlotMismatch`; refused (the
  verifier re-derives the expected monitor hash from the `axon-os` component entry and cross-checks).

---

## §8 — Threat model / invariants / edge cases

### 8.1 Threats R31 closes

- **Kill-switch replacement.** An attacker replaces `axon-os` with a binary whose latch always
  returns `Clear`. The `axon-os` slot hash changes; `axtcb1_ext` changes; the relying party's pinned
  expectation no longer matches; the run is refused before the VM boots.
- **Audit suppression.** An attacker replaces `axon-audit` with a no-op that discards events. The
  `axon-audit` slot hash changes; `axtcb1_ext` changes; refused.
- **Downgrade attack.** An attacker forces use of an older (buggy) version of any component. The
  older binary has a different sha256; `axtcb1_ext` changes; refused. The component version is
  pinned by the relying party's expected digest, not by a version string the attacker can forge.
- **Monitor removal.** An attacker strips the compliance monitor thread from a custom `axon-os`.
  The resulting binary is different bytes; slot 1 and slot 3 both change; `axtcb1_ext` changes.

### 8.2 Threats R31 does NOT close (named, not hidden)

- **Host OS compromise (root).** A root-level attacker can replace any binary *and* recompute the
  hashes to match the expected values if the measurement is performed in the same OS the attacker
  controls. R31 detects substitution in a *trustworthy* measurement context; a compromised
  measurement host can forge everything. This is the hardware confidential computing boundary (R26
  S7): only real SEV-SNP/TDX removes the host operator from the measurement trust boundary.
- **Dynamic code injection.** After measurement, an attacker with code-injection primitives (e.g.,
  `ptrace`, `/proc/mem`) can alter a process's running code without changing the on-disk binary.
  R31's measurement is over disk bytes at attestation time; runtime memory integrity is not covered.
  Memory encryption (SEV-SNP/TDX, R26 §8) is the defense; it is out of R31's scope.
- **Component path aliasing.** If the relying party and the measured host disagree on which path
  corresponds to the `axon-os` binary (e.g., the attacker has a different binary at
  `target/release/axon-os` vs. the one the supervisor actually runs), R31 cannot detect the
  discrepancy. The manifest MUST canonicalize component paths, and the relying party MUST verify the
  `path` fields in the report match the expected deployment layout.

### 8.3 Invariants (assert in tests)

- **I-1 (four slots, canonical order).** The `components` array always has exactly four entries in
  order `[kernel, axon-os, axon-audit, monitor]`. A report with fewer, more, or reordered entries
  is `Malformed`.
- **I-2 (monitor slot derivation).** `monitor.sha256 == axon-os.sha256` always. The verifier
  MUST re-derive and cross-check; it never trusts the report's claim.
- **I-3 (prefix distinctness).** `axtcb1` always starts with `"axtcb1:"` and never `"axtcb1-ext:"`;
  `axtcb1_ext` always starts with `"axtcb1-ext:"` and never `"axtcb1:"`. Mixing them is a
  `VerifyError::PrefixMismatch`.
- **I-4 (R26 preserved).** Any code path that produces an `axon-vm-report/2` report also produces
  valid `axtcb1` (the kernel-only R26 value) in the same report. R31 never removes or replaces the
  R26 measurement.
- **I-5 (fail closed).** Missing required component → `MeasureError`, refused. Verifier cannot
  produce `Ok` for a report missing `axtcb1_ext` when `--extended-tcb` was requested.

### 8.4 Edge cases

- **R28 not yet shipped.** `axon-audit` is absent on the host. The zero-fill path (§4.2) produces a
  report the relying party can inspect; a relying party that pins the zero-fill (`0x00*32`) explicitly
  accepts it; any other expected value is refused. Once R28 ships, the zero-fill path is removed and
  tests are updated.
- **R29 thread not yet a separate binary.** Always covered by the `axon-os` hash in slot 3; no
  special case needed during R31's lifetime unless R29 is refactored into a separate binary.
- **Large binaries.** The `axon-os` binary may be large (several MB). The measurement is streaming
  (read in chunks, feed to `sha2::Sha256::update`); no full-binary buffer in memory needed. The
  CLI layer reads and injects bytes in a streaming fashion.
- **Concurrent writes to a component binary** during measurement (e.g., a `cargo install` running
  in parallel). The measurement reads what is on disk at the time of the `open` syscall; a partial
  write may produce an inconsistent hash. Operators MUST quiesce writes to component paths during
  attestation. R31 cannot defend against concurrent writes — it reports whatever bytes it reads.

---

## §-Gate — `scripts/r31_acceptance_gate.sh` (pinned; FAILS if any §0 check missing or stubbed)

The gate is described here; the actual script is deferred until R28/R29 reach stable artifact
paths. The gate MUST:

1. **Presence check** — `grep` the R31 test sources and assert every named check from §0 exists:
   `acc_a1_smoke_extended_tcb_journey`, `acc_a2_byte_identical_across_runs`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_hermetic_isolated_timeout`,
   `acc_a5_tampered_component_detected`, `acc_a6_chaining_order_canonical`,
   `missing_component_fails_closed`, `component_version_in_report`,
   `r26_baseline_backward_compatible`, `extended_tcb_wired_into_run`.
   Any missing name → gate fails.

2. **Anti-stub check** — assert no acceptance test is `#[ignore]`d / `todo!()` /
   `unimplemented!()` / `assert!(true)`. Exception: a single `#[ignore = "R28 not yet shipped"]`
   on the non-zero-fill tamper assertion inside `acc_a5_tampered_component_detected` is permitted
   until R28 ships; the gate allows *exactly* this one annotation by its R28 reason string.
   The zero-fill path of `acc_a5` MUST still run and pass (the carve-out covers only the real-binary
   tamper assertion, not the zero-fill sentinel assertion).

3. **Anti-vacuous tamper check** — assert `acc_a5_tampered_component_detected` exercises at least 3
   distinct slot-tamper cases (kernel, axon-os, and at least one of axon-audit/monitor). A test that
   tampers only one slot passes vacuously; the gate counts the tamper variants and fails if < 3.

4. **Canonical-order proof** — assert `acc_a6_chaining_order_canonical` verifies that a
   `components` array with entries in a non-canonical order produces a different `axtcb1_ext` than
   the canonical order (not just that the canonical order serializes correctly).

5. **R26 regression check** — run `cargo test -p axon-attest` with `--test r26_acceptance` (the
   existing R26 tests) and assert all still pass. R31 must not regress R26.

6. **Run** `cargo test -p axon-attest` (all R31 tests green) **and** execute the §8 quickstart
   commands verbatim against the built `axon-vm` binary **and** run `acc_a1` driving the real CLI.

7. **Schema version check** — assert a report produced with `--extended-tcb` has
   `"schema": "axon-vm-report/2"` and a report produced without has `"schema": "axon-vm-report/1"`.

8. Exit 0 only if all of the above pass; print which check failed otherwise. Wire
   `r31_acceptance_gate.sh` into the repo's `gate.sh --strict` after R28/R29 reach stable paths.

---

## §8 — Quickstart (these exact commands are executed by `acc_a3`)

```bash
# R31 Extended TCB Attestation

# 1. Measure the full safety stack (kernel + axon-os + axon-audit + compliance monitor):
axon-vm attest --kernel dist/guest/vmlinuz --extended-tcb

# 2. Inspect the extended report — all 4 components must appear:
axon-vm attest --kernel dist/guest/vmlinuz --extended-tcb --json | jq '.report.measurement.components | length'
# → 4

# 3. Pin the extended digest for your deployment:
axon-vm attest --kernel dist/guest/vmlinuz --extended-tcb --json \
  | jq -r '.report.measurement.axtcb1_ext'
# → "axtcb1-ext:…"

# 4. Boot with full-stack attestation check (refuses if any component is wrong):
axon-vm run job.ax --kernel dist/guest/vmlinuz --extended-tcb

# 5. Verify a report includes all 4 component hashes and that monitor == axon-os:
axon-vm attest --kernel dist/guest/vmlinuz --extended-tcb --json \
  | jq '(.report.measurement.components[] | select(.name=="monitor") | .sha256) ==
         (.report.measurement.components[] | select(.name=="axon-os")  | .sha256)'
# → true

# 6. Watch a tampered axon-os binary be detected (replace with a copy, flip one byte):
cp target/release/axon-os /tmp/axon-os-tampered
printf '\x00' | dd of=/tmp/axon-os-tampered bs=1 seek=4096 count=1 conv=notrunc 2>/dev/null
axon-vm attest --kernel dist/guest/vmlinuz \
  --axon-os /tmp/axon-os-tampered --extended-tcb --json \
  | jq '.report.measurement.axtcb1_ext'
# → "axtcb1-ext:<different value from step 3>"
```

---

## §-Definition of Done

**Per slice (S1–S7):** the slice's named checks existed, were seen to fail, now pass; the full
`axon-attest` suite is green; no regression in R26 tests or the workspace.

**Per milestone (R31 complete):**
- `axon-vm attest --extended-tcb` produces an `axon-vm-report/2` JSON report with `axtcb1_ext`
  and a 4-entry `components` array in canonical order.
- `axon-vm attest` (without flag) produces an `axon-vm-report/1` report identical to R26 output
  (`r26_baseline_backward_compatible` passes).
- Replacing any one of kernel / axon-os / axon-audit with a different binary produces a different
  `axtcb1_ext` (`acc_a5_tampered_component_detected` passes for ≥3 slot variants).
- `axon-vm run --extended-tcb` refuses to boot if the extended digest does not match the pinned
  expectation (exit 10); refuses to measure if a required component is absent (exit 12).
- The `monitor` slot's sha256 equals the `axon-os` sha256 in every report; the verifier
  re-derives and cross-checks this (never trusts the report's claim).
- Two invocations on the same binaries produce byte-identical `axtcb1_ext` (`acc_a2` passes).
- `scripts/r31_acceptance_gate.sh` exits 0 with every §0 check green.

Only then is R31 done.

---

## §-Notes for the implementer (do NOT deviate without updating this spec)

- Keep `measure_extended` and `verify_extended` **pure**. File reads belong in the CLI layer;
  inject bytes as `&[u8]`. If you reach for `std::fs` inside `measure.rs`, you are in `cli.rs`'s
  job — the same rule R26 applied to `verify.rs`.
- **The canonical order is a protocol.** Do not accept a `components` array in any other order, even
  if "the bytes are the same." Order matters for the outer SHA-256; reordering silently produces a
  different digest that no relying party's pinned expectation will match. Assert ordering in every
  code path that builds a `components` Vec.
- **The monitor slot is NOT redundant.** It is a commitment that the monitor thread is present in
  the `axon-os` binary you measured. Document it as such in `--help` and in any operator-facing
  error message, so an operator who strips the monitor does not think "slot 3 = 0x00*32 is fine."
- **The zero-fill for axon-audit is a named, auditable sentinel, not a default.** Its presence in
  a report is a signal to the relying party that R28 has not yet shipped on this host. Do not
  silently zero-fill any other slot; zero-fill is a compile-time constant specific to the
  `axon-audit` slot during the R28-pending window.
- **Schema version is a contract.** `axon-vm-report/1` means no extended fields; `axon-vm-report/2`
  means both `axtcb1` and `axtcb1_ext` are present. A report that has `axtcb1_ext` but claims
  schema `/1` is `Malformed`; reject it.
- **Exit codes 10/11/12 are distinct.** 10 = attestation mismatch (measured correctly, doesn't
  match expected); 11 = TCB chain break (R26-inherited); 12 = extended measurement failure (can't
  read a required component). Never collapse 12 into 10.
- R31 changes no Axon-level containment, no R27 kill-switch logic, no R26 hardware boundary. If you
  find yourself editing `kernel.rs`, `latch.rs`, or `substrate.rs`, you are out of scope.
