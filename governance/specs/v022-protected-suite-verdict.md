# v0.22 Protected suite verdict (microVM) — acceptance surface

Status: **DRAFT, frozen before implementation** (operator direction, 2026-09-27). This document
defines what the microVM suite-verdict path must do and how it is judged. It does not certify
anything.

## The rule this workstream lives under

> **Development-host protocol tests may prove the mechanism. They must never satisfy any of the
> three remaining protected-readiness components.**

The development host (this WSL machine, ADR-001 D1) builds the protocol and tests it
adversarially. Only the PROTECTED host certifies it.

The rule is enforced by `scripts/protected_verifier_ready.py`, not by convention:
- `protected_backend`, `g01_on_protected_backend` and `pci_on_protected_backend` are each PASS
  only with a **protected-host certification record**: a JSON document with a detached
  `axon-evidence-signature/1` that verifies under an operator key in
  `profiles/linux-microvm/trusted_issuers/`.
- Verification is by `axon-fabric verify-evidence`, the same code Fabric uses for the B263
  qualification record. The script passes the committed issuer directory itself; no environment
  variable or flag can redirect it.
- That directory holds no key until the operator installs one (ADR-001 D5/D6). Agents never hold
  or create the key, and never commit a `.pub` there. So no record produced on the development
  host can verify, and the three components stay NOT_RUN however many dev tests pass.
- A gate registration row or an unsigned proof document is necessary but NOT sufficient for these
  three components.

Operator qualification steps are separate from coding, and are never reported as protected
success by the implementation:
- the B263 signature (S3-6);
- installing the trusted issuer key;
- deploying the preflight observer (ADR-002).

## Architecture

```
operator suite + candidate
        ↓
sealed guest inputs            (PSV-2)
        ↓
preflight observer attests the launch   (PSV-6, ADR-002)
        ↓
Firecracker guest              (PSV-7)
        ↓
trusted guest runner           (PSV-1, PSV-3)
        ↓
PCI completion proof           (PSV-3)
        ↓
guest verifier verdict         (PSV-4)
        ↓
Fabric protected receipt       (PSV-4, PSV-5)
        ↓
G01 authentication             (PSV-5)
        ↓
binding / intake               (the registered verification-binding gates)
```

## Acceptance surface

Each clause names what must hold. The negative matrix below names how each one fails closed.

**PSV-1 — The operator's suite, not the candidate's checks.**
- The guest executes the operator-registered suite (`check_registry`, pinned by WorkspaceVersion
  and entry) and exactly the task's registered acceptance test (`task_acceptance`).
- Candidate bytes can never define, add or select the rubric (G01-r22-verifier-separation, in the
  guest).

**PSV-2 — Separately sealed, digest-bound inputs.**
- Candidate tree and suite tree are delivered to the guest as two separately sealed inputs, each
  bound by digest (candidate WorkspaceVersion; suite id@version#entry).
- The guest runner refuses to start if either digest differs from what the launch manifest names.
- The candidate reaches the suite only as a module path, and is sealed per PCI (E0004).

**PSV-3 — PCI inside the guest.**
- The affirmative completion token (a fresh per-run key and a token per completed test) is
  produced and checked by the trusted runner INSIDE the guest.
- Sealing, containment and per-provenance kernels run in the guest interpreter exactly as
  certified for the local path (`governance/proofs/v022-pci/CERTIFICATION.md`).
- A pass without completion evidence is Unknown.

**PSV-4 — Fabric signs only a guest-path verdict.**
- Fabric attests (`acf-receipt-attestation/2`) a verdict only when it came from the protected
  guest path.
- Backend `linux-microvm-protected`, under a current B263 qualification.
- The verdict carries the guest runner's completion evidence.
- A verdict from any other backend, or reconstructed outside the guest, is never attested as
  protected.

**PSV-5 — The receipt binds everything that makes the verdict mean something.**
- The signed receipt (or the attestation's signed binding) names:
  - guest image, kernel and runtime (guest `axon`) digests;
  - suite id, version, entry and test;
  - candidate tree (WorkspaceVersion);
  - task, trial, attempt and operation;
  - verifier identity and key;
  - the preflight observation digest (PSV-6);
  - the launch manifest digest.
- Intake and EVL verify each join (G01 authentication, then the registered binding gates).

**PSV-6 — Fresh observer evidence joins the exact launch (ADR-002).**
- A `PreflightObservation`, signed by the observer's own key (authorised by the operator root key,
  distinct from every other role), is bound to a custodian nonce and epoch, and names:
  - host profile;
  - Fabric and Firecracker revisions;
  - guest image, kernel, verifier, suite and policy digests;
  - the intended launch manifest digest.
- Fabric consumes it and cannot mint it.
- Intake rejects a receipt whose observation digest or launch manifest digest does not join, or
  whose observation is stale, replayed or from another launch.

**PSV-7 — Development cannot pass as protected.**
- The local interpreter backend, a development-class evaluation, and a receipt from any host
  without a current B263 qualification cannot satisfy the protected profile (ADR-001 D3), in code
  and in the readiness derivation.

## Negative matrix (each must fail closed, and be tested)

| Case | Expected refusal point |
|---|---|
| Replayed verdict or receipt (same operation, fresh journal) | Fabric withholds or intake refuses: nonce/observation not fresh (PSV-6) |
| Wrong guest image or kernel | Observer/launch-manifest join fails (PSV-5/6); Fabric refuses an unqualified profile |
| Wrong suite, version or test | Guest runner refuses (PSV-2); intake refuses (registered acceptance, PSV-1) |
| Candidate changed after sealing | Guest runner digest check (PSV-2); receipt input-tree join (PSV-5) |
| Stale or replayed observer evidence | Intake: observation nonce/epoch/age (PSV-6) |
| Wrong verifier key | G01 authentication refuses (PSV-5) |
| Missing completion evidence | Verdict Unknown (PSV-3) |
| Mismatched trial/attempt/operation | Intake identity joins (PSV-5) |
| Local/dev backend presented as protected | Protected class counts nothing (PSV-7, D3) |
| Unsigned or self-signed protected-host certification | Readiness stays NOT_RUN (the rule above) |

## How the three readiness components are earned

| Where | What | Earns |
|---|---|---|
| Dev host | The protocol and the negative matrix above, adversarially | Nothing in readiness: *protocol proven* |
| Protected host | B263-qualified `linux-microvm-protected` run; the operator signs the certification record | `protected_backend` PASS |
| Protected host | G01 authenticity through the guest path; operator-signed record | `g01_on_protected_backend` PASS |
| Protected host | PCI surface through the guest path; operator-signed record | `pci_on_protected_backend` PASS |

When all seven components are PASS, PROTECTED_VERIFIER_READY = READY. Stage 7 and CX-21 then
still need their own frozen documents.

## The certification record the readiness script checks

`governance/proofs/v022-protected/<component>.json`, plus
`governance/proofs/v022-protected/<component>.json.sig`:

```json
{
  "schema": "axon-v022-protected-certification/1",
  "component": "protected_backend | g01_on_protected_backend | pci_on_protected_backend",
  "host_profile": "linux-microvm-protected",
  "axon_sha": "<40 hex>",
  "micode_sha": "<40 hex>",
  "b263_qualification_sha256": "<the signed B263 evidence the run used>",
  "evidence": ["<proof files in governance/proofs/ this certifies>"],
  "certified_at": "YYYY-MM-DDTHH:MM:SSZ"
}
```

The `.sig` is an `axon-evidence-signature/1` over the record's exact bytes, from an operator key
in `profiles/linux-microvm/trusted_issuers/`. The readiness script requires, beyond the gate rows
and proof documents the component already needs:
- a valid signature;
- a `component` equal to the component;
- `host_profile` equal to `linux-microvm-protected`;
- every listed evidence file to exist.
