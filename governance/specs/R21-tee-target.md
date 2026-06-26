# R21 — TEE / Confidential-Computing Target (enclave-gated Secret declassification)

**Spec ID:** `R21-tee-target` (new requirement row; depends on `examples/stdlib/tainted.ax` info-flow lattice, `R6-capability-security.md`, Phase 6 effect rows; extends ROADMAP §6 Containment pillar)
**Error code:** `E1810` (first free code after the E1800–E1803 R13-FFI block; E1700–E1712 and E1900 avoided)
**Status:** Draft — **Slice 1 (type rule) LANDED.** Slice 2 (gramine-direct simulated run) + Slice 3 (cloud attestation workflow) per §6.
**Risk class:** Additive (a new effect `Tee` + one checker rule + four interp-only builtins; no existing behaviour changed)
**Author / date:** cklaus, 2026-06-26

> **One-line scope:** run an Axon workload inside a trusted execution environment, with the info-flow
> `Secret` lattice composing with the enclave boundary — "compute on data you can't read," enforced by
> the type system. A sealed `Secret` may only be *unsealed/declassified inside* an `@[enclave]` region;
> the checker REFUSES unsealing it anywhere else (E1810). This is the real, **locally-verifiable**
> differentiator and is enforced with **no TEE hardware**.

---

### 1. Motivation

Confidential computing (SEV-SNP, TDX, SGX) lets you run a workload on a host you don't trust: the CPU
encrypts enclave memory so neither the host OS, the hypervisor, nor a co-tenant can read it; a hardware
attestation quote proves to a remote party *which exact image* is running before any secret is released.

Axon already has the *type-level* half of this story: the info-flow lattice in `examples/stdlib/tainted.ax`
(`Secret`/`Public` confidentiality axis, `Tainted`/`Trusted` integrity axis). What was missing is the
**boundary**: a place where a `Secret` may be declassified, and a compile-time guarantee that declassification
happens **only** there. R21 adds that boundary as the `@[enclave]` region and makes the rule a checker rule —
so it is enforceable and testable on a host with **no TEE hardware**, which is exactly the honest-boundary
constraint (this host exposes only `sme`; there are no `/dev/sev`, `/dev/sgx_enclave`, `/dev/tdx_guest` nodes).

### 2. The deliverable, split by what is REAL vs SIMULATED vs TYPE-ENFORCED

This is the crystal-clear honesty boundary — the same discipline R14 applied to iOS:

| Layer | What it is | Where it runs | Honesty |
|---|---|---|---|
| **TYPE-ENFORCED** | `@[enclave]`/`tee_unseal` rule (E1810): a sealed Secret is unsealed ONLY in-enclave | THIS host, `axon check` | **REAL** — it is a pure type/checker rule, gate-tested. The genuine differentiator. |
| **SIMULATED** | Executing the enclave workload under `gramine-direct` (no SGX hardware) | THIS host, `scripts/tee_sim_run.sh` | **SIMULATED** — `gramine-direct` runs the binary as if in an enclave but on any CPU; seal/unseal are identity, the measurement is a stub. |
| **REAL-ATTESTED** | A hardware-rooted SEV-SNP/SGX attestation quote | A confidential cloud runner, `.github/workflows/tee.yml` | **NOT produced here.** Runs REMOTELY on a confidential VM. We do **not** fake an attestation. |

### 3. Surface

```axon
// Seal a confidential value (allowed ANYWHERE):
let sealed = tee_seal(salary, 2)          // level 2 = confidential

// Declassify INSIDE the enclave (the ONLY legal site):
@[enclave]
fn enclave_average(a: i64, b: i64) -> i64 | {Tee} {
    let x = tee_unseal(a, 2)              // OK — we are in-enclave
    let y = tee_unseal(b, 2)
    (x + y) / 2                           // only the aggregate leaves
}

// Declassify OUTSIDE the enclave → E1810:
fn leak(sealed: i64) -> i64 | {Tee} {
    tee_unseal(sealed, 2)                 // ERROR E1810
}
```

Builtins (all **interp-only**; codegen E0910-refuses them, sound-by-refusal):

| Builtin | Effect | Semantics |
|---|---|---|
| `tee_seal(val, level) -> i64` | `Tee` | seal at a confidentiality level; allowed anywhere |
| `tee_unseal(sealed, level) -> i64` | `Tee` | declassify; **only legal in `@[enclave]`** (E1810 elsewhere) |
| `tee_in_enclave() -> bool` | `Tee` | runtime probe; reads `AXON_TEE_ENCLAVE=1` (the gramine manifest sets it) |
| `tee_attest_measurement() -> str` | `Tee` | SIMULATED launch measurement (`AXON_TEE_MEASUREMENT` or a stub) |

`@[enclave]` is a known attribute (no W0001). `Tee` is an effect-row tag carried by `builtin_effect_row`;
it flows through the Phase-6 effect checker like `Hal`/`Net` (a Tee call leaking into an unannotated context
is E1310).

### 4. The rule (E1810) — the gate-able core

For every fn (and impl method) NOT annotated `@[enclave]`, the checker scans its body for `tee_unseal`
call sites and emits one E1810 per site. **Lexical by design**: a `tee_unseal` reached through a helper
trips E1810 on the *helper* (which must itself be `@[enclave]`), so there is no un-annotated fn that may
unseal — no laundering hole. Implemented in `checker.rs::check_enclave_unseal` / `count_tee_unseal`,
mirroring the `@[no_alloc]`/E1704 walker.

**Gate test (LANDED):** `tee_unseal_outside_enclave_rejected_e1810` (`tests/integration_fixtures.rs`,
fixture `r21_tee_unseal_e1810.ax`) asserts exactly 2 E1810 (a direct leak + a laundering helper) and that
the `@[enclave]` fn is clean. Passes under `scripts/gate.sh`.

### 5. Why this is sound with no hardware

The guarantee R21 makes locally is **not** "the host can't read the data" (that needs hardware). It is:
"in this program, a sealed Secret is only ever *unsealed in source positions the author marked `@[enclave]`*."
That is a property of the AST, checkable statically, and it is the property that *composes* with hardware:
when you later run the `@[enclave]` fn inside a real SEV-SNP guest, you know — by the type system — that the
cleartext never appears outside it. The hardware enforces confidentiality of the enclave's memory; the type
system enforces that declassification is confined to the enclave. Together they are confidential computing.

### 6. Slices

- **Slice 1 — type rule (LANDED):** `@[enclave]` + `tee_*` builtins + E1810 + the gate test. §4.
- **Slice 2 — gramine-direct simulated run:** `scripts/tee_sim_run.sh` builds a native Axon binary
  (`axon build`) and runs it under `gramine-direct` with a manifest that sets `AXON_TEE_ENCLAVE=1`.
  SKIP-guards (exit 0 with a clear notice) if `gramine-direct` is absent — it is not installed on this host.
  *Note:* the `tee_*` builtins are interp-only, so the binary used under gramine is a host workload that
  demonstrates the enclave EXECUTION; the type guarantee is the `axon check` rule, run separately.
- **Slice 3 — cloud attestation (`.github/workflows/tee.yml`):** on a confidential runner builds for real
  SGX (Gramine-SGX) or SEV-SNP and verifies a genuine attestation quote. YAML-valid here; the real run is
  remote. We do **not** fake a quote.

### 7. Non-goals / honesty

- No hardware attestation is produced on this host. None.
- `tee_seal`/`tee_unseal` are NOT encryption in the simulation — they are the identity on the payload. The
  value-level confidentiality contract is the `Secret` lattice in `tainted.ax`; R21 adds the *enclave
  boundary* and its *compile-time confinement*, not a crypto implementation.
- Codegen does not lower the `tee_*` builtins (E0910-refused) — interp-only, same discipline as
  `host_await`/kernel/sandbox builtins.
