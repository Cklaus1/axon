# Axon v0.15 build-package design review

**Basis:** supplied v0.15 ZIP and original standalone CX-36. No live Axon code, model benchmark or production sandbox was tested.

**Review outcome:** findings below are folded into the proposed v0.16 documents. Adversarial cases remain product acceptance obligations unless explicitly identified as executed inert-format/document checks. This is a separate review pass, not a separate independent reviewer.

## BL-D01 — CRITICAL

**Source:** Supplied v0.15 AXON_CORTEX_MASTER.md vs individual CX-32/CX-33/CX-34 and manifests

The master mainly contains v0.14 material/counts and only appends a v0.15 summary; a builder reading it misses authoritative new contracts.

**Folded change:** Generate master from every current modular Markdown document with source digests; validator rejects stale derived views.

**Amended owners:** `tools/package_views.py`; `tools/validate_package.py`; `AXON_CORTEX_MASTER.md`.

**Acceptance:** `G00-assurance-separation`. Verification scope: `DOCUMENT_MUTATION_TESTS`.

## BL-D02 — HIGH

**Source:** Supplied gate_manifest.json compared to task_manifest.json

Twenty declared acceptance gates have no work package assigned; original validator nevertheless passes.

**Folded change:** Assign each original orphan gate to a concrete task and require zero orphan/unknown gate references.

**Amended owners:** `task_manifest.json`; `build/TASKS.md`; `review/BASELINE_GAPS.json`; `tools/validate_package.py`.

**Acceptance:** `G00-assurance-separation`. Verification scope: `DOCUMENT_MUTATION_TESTS`.

## BL-D03 — HIGH

**Source:** Supplied schemas/PROTOCOLS.md vs CX-32–CX-34 and v0.15 summary

Evidence/causal/scheduler type shapes are not folded into the shared protocols; claims outpace interface definitions.

**Folded change:** Add owner-linked logical record shapes, separate model confidence provenance and wire/source schemas, and require exact protocol linkages.

**Amended owners:** `schemas/PROTOCOLS.md`; `build/OWNERSHIP_AND_COMPATIBILITY.md`.

**Acceptance:** `G32-closure-snapshot`, `G33-estimate-class`, `G34-bounded-fallback`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-D04 — HIGH

**Source:** Supplied B116/B118/B141/B143/B148 dependency chains

Early instrumentation/registry work waits on the later learned subsystems it should make possible. No literal cycle, but unnecessary critical-path coupling.

**Folded change:** Decouple incumbent instrumentation and minimal evidence/registry adapters; add bounded release profiles and complete dependency closures.

**Amended owners:** `task_manifest.json`; `release_profiles.json`; `build/IMPLEMENTATION_RELEASE_SLICES.md`.

**Acceptance:** `G00-assurance-separation`. Verification scope: `DOCUMENT_DAG_TESTS`.

## BL-D05 — HIGH

**Source:** Supplied tools/validate_package.py and validation reports

Validation is largely structural and does not establish generated-view parity, gate coverage, exact examples or complete hash inventory. Published report truncates topological lists.

**Folded change:** New validator checks schemas, source/manifests/views, complete DAGs, task gate ownership, inert example validation and package inventory. Actual mutation tests replace asserted self-test receipts.

**Amended owners:** `tools/validate_package.py`; `tools/package_views.py`; `tests/test_package_validation.py`.

**Acceptance:** `G00-assurance-separation`. Verification scope: `DOCUMENT_MUTATION_TESTS`.

## BL-D06 — MEDIUM

**Source:** v0.15 stage labels and broad bootstrap prompts

Contract intake stages, whole-subsystem maturity and optional research can be read as one huge implementation commitment.

**Folded change:** Define contract-vs-task dependencies, exit-task release slices, explicit intake before implementation, and no forced successful research candidate.

**Amended owners:** `build/IMPLEMENTATION_RELEASE_SLICES.md`; `build/BOOTSTRAP_PROMPT.md`; `build/BUILD_PROTOCOL.md`.

**Acceptance:** `G00-assurance-separation`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-D07 — MEDIUM

**Source:** CX-05/CX-06 and protocols probability origins

Calibration is treated in some notation as a raw source; generic confidence can hide the prediction event.

**Folded change:** Keep generated/token/head distribution origin separate from calibrator, applicability/OOD score and empirical event-specific correctness.

**Amended owners:** `schemas/PROTOCOLS.md`; `specs/CX-06-routing-calibration.md`; `build/REFLEX_CALIBRATION_LAB.md`.

**Acceptance:** `G06-reliability-event`, `G20-target-separation`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-D08 — MEDIUM

**Source:** CX-31 P0–P5 and prior T0–T5 tiers

Transfer labels can imply an unjustified total ordering from one repo/language/family to all others.

**Folded change:** Report repo/family/language/task novelty as separate dimensions with contamination status, grouped uncertainty and receiving-project admission.

**Amended owners:** `specs/CX-31-cross-project-learning-plane.md`; `schemas/PROTOCOLS.md`.

**Acceptance:** `G31-transfer-dimensions`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## Remaining evidence required

Run live source reconciliation before choosing modules or claiming features exist. Production trust-store/signature verification, archive fuzzing, protected process/network isolation, GPU/adapter races, real model quality/calibration and cross-project transfer all still require execution in Axon. Included format checks establish only the stated source/container contract. A document gate passing never changes a product gate from `NOT_RUN`.
