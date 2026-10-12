# CX-36 adversarial review

**Basis:** supplied v0.15 ZIP and original standalone CX-36. No live Axon code, model benchmark or production sandbox was tested.

**Review outcome:** findings below are folded into the proposed v0.16 documents. Adversarial cases remain product acceptance obligations unless explicitly identified as executed inert-format/document checks. This is a separate review pass, not a separate independent reviewer.

## NP-A01 — CRITICAL

**Source:** Original CX-36 section 23: validate archive/package without concrete parser policy

Hostile package can use traversal, duplicate members, misleading metadata, executables or unbounded allocation before trust is checked.

**Folded change:** Specify allowlisted stored-ZIP, exact inventory, path/header/type/hash/size checks, staging and isolated trusted loaders; add inert negative fixtures.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `tools/neural_contract_reference.py`; `tests/test_neural_contract.py`.

**Acceptance:** `G36-container`. Verification scope: `INERT_FORMAT_TESTS; PRODUCTION_PARSER_FUZZING_NOT_RUN`.

## NP-A02 — CRITICAL

**Source:** Original CX-36 sections 4/18/23: embedded evaluation/registry status

Attacker forges PASS/signatures in an otherwise digest-valid package or loads a candidate into active service.

**Folded change:** Integrity is not issuer authentication; detached release with trusted admission, candidate-only isolation and current dispatch status.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `specs/CX-11-crystallization-admission.md`.

**Acceptance:** `G36-trust`, `G11-release-binding`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-A03 — CRITICAL

**Source:** Original CX-36 sections 11/13/16: optional remote backend and fallback

Compile, download, telemetry or fallback can leak private source after local inference fails, or grant a denied operation under another backend.

**Folded change:** Preparation and invocation have separate grants; external compile/publish explicit; inherited budgets and hard constraints apply to each fallback; offline enforced at host.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `build/REVIEW_INVARIANTS.md`.

**Acceptance:** `G36-import`, `G36-offline`, `G36-fallback`, `G03-fallback-recheck`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-A04 — CRITICAL

**Source:** Original CX-36 sections 12/25: shared-base hot-swap/cache

Concurrent invocations can mix adapters, task prefix state or tenants, while receipts name the old model.

**Folded change:** Per-invocation exact revision binding and scoped caches; immutable in-flight handles, synchronized swaps and negative contamination tests.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `build/REFLEX_RUNTIME.md`.

**Acceptance:** `G36-adapter-isolation`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-A05 — HIGH

**Source:** Original CX-36 sections 16/19/25: fallback and lifecycle

Cycles, timeout retries and ambiguous remote compile completion can multiply cost or double-run work.

**Folded change:** Global request budget, fallback depth/visited set, cancellation receipts, idempotency scope and unknown-result reconciliation.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `specs/CX-34-cognitive-scheduler-capability-registry.md`.

**Acceptance:** `G36-fallback`, `G36-jobs`, `G34-bounded-fallback`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-A06 — CRITICAL

**Source:** Original CX-36 sections 19/23: previous-good rollback/immutable artifacts

Revocation after validation or a revoked previous-good release can cause unsafe reactivation. Code rollback can be falsely described as reversal of external effects.

**Folded change:** Dispatch checks current release/closure generation; rollback eligibility is checked again; in-flight outcomes reconciled; fail stopped if no eligible target.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `specs/CX-11-crystallization-admission.md`; `specs/CX-34-cognitive-scheduler-capability-registry.md`.

**Acceptance:** `G36-rollback`, `G34-revocation-race`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-A07 — HIGH

**Source:** Original CX-36 sections 15/19/21: calibrated reliability and learning lineage

High self-reported confidence, teacher agreement or leaked examples can be laundered into P(correct) or protected release evidence.

**Folded change:** Name the predicted event/cohort; keep raw score origin and calibration separate; grouped eligible data, non-test fits and selective policy evaluation.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `specs/CX-20-reflex-research-lab.md`; `schemas/PROTOCOLS.md`.

**Acceptance:** `G36-reliability`, `G20-target-separation`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-A08 — HIGH

**Source:** Original CX-36 sections 21/25: immutable lineage and replay

Replay requirements can force unauthorized indefinite retention or imply erased payloads remain reproducible.

**Folded change:** Retain authorized minimal evidence, permit scoped tombstones/deletion, mark replay unavailable and invalidate dependent claims as appropriate.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `specs/CX-10-replay-learning-data.md`.

**Acceptance:** `G36-replay`, `G10-retention-replay`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## NP-A09 — HIGH

**Source:** Original CX-36 initial BuildLog pilot: preserve source facts

A valid span can point to irrelevant text; a format-correct summary may omit the only causal error or normalize numbers incorrectly.

**Folded change:** Validate snapshot/digest/byte range and exact copying separately from relevance/coverage; retain Unknown and include adversarial distractor/omission fixtures in product plan.

**Amended owners:** `specs/CX-36-neural-program-artifact-contract.md`; `build/NEURAL_PROGRAMS.md`.

**Acceptance:** `G36-source-bound`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## Remaining evidence required

Run live source reconciliation before choosing modules or claiming features exist. Production trust-store/signature verification, archive fuzzing, protected process/network isolation, GPU/adapter races, real model quality/calibration and cross-project transfer all still require execution in Axon. Included format checks establish only the stated source/container contract. A document gate passing never changes a product gate from `NOT_RUN`.
