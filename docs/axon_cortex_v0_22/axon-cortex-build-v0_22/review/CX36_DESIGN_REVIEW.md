# CX-36 design review

**Basis:** supplied v0.15 ZIP and original standalone CX-36. No live Axon code, model benchmark or production sandbox was tested.

**Review outcome:** findings below are folded into the proposed v0.16 documents. Adversarial cases remain product acceptance obligations unless explicitly identified as executed inert-format/document checks. This is a separate review pass, not a separate independent reviewer.

## NP-D01 — HIGH

**Source:** Original CX-36 sections 9–10/29; v0.15 CX-23 and CX-34

The proposal creates compiler/runtime/registry contracts already owned elsewhere. Parallel implementations would diverge.

**Folded change:** CX-23 keeps lifecycle, CX-36 owns naming/source/package contracts, CX-34 registry and CX-11 admission. No synthesized CX-35.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `specs/CX-23-neural-program-runtime-skill-compiler.md`; `build/OWNERSHIP_AND_COMPATIBILITY.md`.

**Acceptance:** `G36-naming`, `G23-format-owner`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-D02 — HIGH

**Source:** Original CX-36 section 3: exact .nps syntax MAY evolve; sections 4–5 package/identity notation

There is not enough syntax, schema, canonicalization or inventory detail to implement compatible loaders.

**Folded change:** Adopt explicit proposed v1 strict JSON .nps plus bounded stored-ZIP .np and machine schemas/reference fixtures.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `schemas/json/neural-program-source.schema.json`; `schemas/json/neural-program-manifest.schema.json`.

**Acceptance:** `G36-source`, `G36-container`. Verification scope: `INERT_FORMAT_TESTS`.

## NP-D03 — HIGH

**Source:** Original CX-36 sections 4–5/9: artifact id/content digest and evaluation receipts bundled with executable identity

An implementation can create self-hash or evaluation-subject cycles and change executable identity after testing.

**Folded change:** Domain-separated manifest digest excludes self id; inventory binds assets; freeze package before evaluation; detached releases bind admission and calibration.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `schemas/PROTOCOLS.md`.

**Acceptance:** `G36-identity`, `G11-release-binding`. Verification scope: `INERT_IDENTITY_TESTS; PRODUCT_GATE_NOT_RUN`.

## NP-D04 — HIGH

**Source:** Original CX-36 sections 1/6: learned typed output and ValidJson examples

Schema validity can be confused with correct extraction/repair or proof of a refinement.

**Folded change:** Return ProposedValue with separate structure/source/semantic validation and explicit missing/refusal/truncation states.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `specs/CX-23-neural-program-runtime-skill-compiler.md`.

**Acceptance:** `G36-typed-io`, `G36-source-bound`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-D05 — MEDIUM

**Source:** Original CX-36 sections 10/24: combined backend methods and hardware/model-format list

Training privileges, serialization formats and hardware placement are conflated.

**Folded change:** Separate compiler/runtime logical interfaces and negotiated operations; bind one runtime target/profile per executable identity.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `build/NEURAL_PROGRAMS.md`.

**Acceptance:** `G36-jobs`, `G15-neural-contract`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-D06 — HIGH

**Source:** Original CX-36 sections 14–15/25: applicability must reject OOD and deterministic replay

Perfect OOD recognition and cross-runtime deterministic replay are not established by the source.

**Folded change:** Hard-check known applicability; measure undetected shift; distinguish recorded replay from fresh numerical/statistical reproduction and unavailable replay.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `build/REVIEW_INVARIANTS.md`.

**Acceptance:** `G36-applicability`, `G36-replay`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-D07 — MEDIUM

**Source:** Original CX-36 sections 19/33: specialization ladder and first BuildLog pilot

A linear ladder and mandatory model success can displace better exact software or select-only solutions.

**Folded change:** Targets are alternatives. Benchmark parser/select-copy plus learned/generative baselines; rejection is a valid pilot outcome; measure omissions as well as hallucinations.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `build/NEURAL_PROGRAMS.md`; `specs/CX-22-cognitive-specialization-compiler.md`.

**Acceptance:** `G36-self-host`, `G36-source-bound`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## Remaining evidence required

Run live source reconciliation before choosing modules or claiming features exist. Production trust-store/signature verification, archive fuzzing, protected process/network isolation, GPU/adapter races, real model quality/calibration and cross-project transfer all still require execution in Axon. Included format checks establish only the stated source/container contract. A document gate passing never changes a product gate from `NOT_RUN`.
