# Axon build-package adversarial review

**Basis:** supplied v0.15 ZIP and original standalone CX-36. No live Axon code, model benchmark or production sandbox was tested.

**Review outcome:** findings below are folded into the proposed v0.16 documents. Adversarial cases remain product acceptance obligations unless explicitly identified as executed inert-format/document checks. This is a separate review pass, not a separate independent reviewer.

## BL-A01 — CRITICAL

**Source:** CX-24 proposed observe_derived output and semantic composition

Schema/span-correct learned extraction can be stored as observed fact, then support a false verified claim.

**Folded change:** Use ExtractedClaim and explicit source/semantic validation; no strength-upgrade solely from a format receipt.

**Amended owners:** `specs/CX-24-semantic-perception-retrieval.md`; `specs/CX-32-universal-evidence-graph.md`.

**Acceptance:** `G24-extraction-strength`, `G00-assurance-separation`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-A02 — CRITICAL

**Source:** CX-33 counterfactual gate permits validated estimator as realized evidence

A well-calibrated simulated outcome can be labeled realized and feed self-promotion.

**Folded change:** Estimates remain estimates regardless of validation; only actual experiment outcomes receive realized class; distinguish upstream matching from treatment-altered effective input.

**Amended owners:** `specs/CX-33-causal-active-experimentation-plane.md`; `build/CAUSAL_ACTIVE_EXPERIMENTATION.md`.

**Acceptance:** `G33-estimate-class`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-A03 — CRITICAL

**Source:** CX-11/CX-32/CX-34 independent admission and source invalidation

A candidate passes checks against evidence that changes/is revoked before publication or dispatch.

**Folded change:** Snapshot evidence closure and generation; bind subject and deployment target; revalidate eligibility at dispatch and rollback; no stale registry lease.

**Amended owners:** `specs/CX-11-crystallization-admission.md`; `specs/CX-32-universal-evidence-graph.md`; `specs/CX-34-cognitive-scheduler-capability-registry.md`.

**Acceptance:** `G11-release-binding`, `G32-closure-snapshot`, `G34-revocation-race`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-A04 — CRITICAL

**Source:** CX-29 early low-risk routing/context policies; CX-13 shadow guarantees

Read-only/shadow can transmit secrets, create persistent state or materially change evaluation context; low-risk label hides deployment consequences.

**Folded change:** Risk is per proposed change/effect/project; no-task-effect shadow has egress/storage/resource grants; mandatory controls prefilter all backend fallbacks.

**Amended owners:** `specs/CX-13-os-runtime.md`; `specs/CX-29-reflexive-self-application-plane.md`; `build/REVIEW_INVARIANTS.md`.

**Acceptance:** `G13-shadow-egress`, `G29-change-risk`, `G03-fallback-recheck`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-A05 — HIGH

**Source:** CX-28 context pins, recompute and async selection

Pins exceed budget, recomputation is unavailable/stale, or a delayed context decision drops required evidence.

**Folded change:** Hard pins cannot disappear to make room; stop/escalate on overflow; materialize exact model-visible receipts and revalidate snapshot/authority before use.

**Amended owners:** `specs/CX-28-semantic-working-set-manager.md`; `build/REVIEW_INVARIANTS.md`.

**Acceptance:** `G28-pin-overflow`, `G10-retention-replay`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-A06 — HIGH

**Source:** CX-20/21 locked tests and CX-29 recursive proposals

Repeated aggregate eval feedback can overfit locked tasks despite no raw test access; self-optimization can recursively spend its own budget.

**Folded change:** Protected submission/feedback ledger, role separation, preregistered thresholds, fresh final cohorts when needed; lineage-aware depth/fanout/global budget.

**Amended owners:** `specs/CX-01-evaluation.md`; `specs/CX-20-reflex-research-lab.md`; `specs/CX-29-reflexive-self-application-plane.md`.

**Acceptance:** `G01-feedback-budget`, `G29-recursion-budget`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-A07 — HIGH

**Source:** CX-27 supervision and baseline builder prompts

Supervisor confidence or inability to find more work becomes completion authority; stop loops or revival defeat operator intent.

**Folded change:** Supervisor only proposes finish; protected evidence required, terminal cancellation latch and bounded intervention/fallback.

**Amended owners:** `specs/CX-27-semantic-supervisor-plane.md`; `build/COMPLETION_CRITIC.md`; `build/REVIEW_INVARIANTS.md`.

**Acceptance:** `G27-finality`, `G34-bounded-fallback`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-A08 — HIGH

**Source:** CX-05 state reuse and earlier model-state discussion

Uniform interfaces can imply question isolation, cross-model cache portability or snapshot/fork guarantees a backend does not implement.

**Folded change:** Negotiated feature profiles, exact model/runtime/prompt/isolation scope and typed unsupported response; no latent-state compiler added by implication.

**Amended owners:** `specs/CX-05-reflex-inference.md`; `schemas/PROTOCOLS.md`; `build/REFLEX_RUNTIME.md`.

**Acceptance:** `G05-feature-profile`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-A09 — HIGH

**Source:** CX-16 MiCode bridge and CX-10 evidence/data trust

First-party imported episodes become treated as authoritative confinement, ground truth or immutable approval.

**Folded change:** Source authentication grants only provenance; local host/validator contracts and bridge versions remain explicit; unsupported stronger semantics refuse.

**Amended owners:** `specs/CX-29-reflexive-self-application-plane.md`; `review/MICODE_MIGRATION.md`; `build/OWNERSHIP_AND_COMPATIBILITY.md`.

**Acceptance:** `G19-contract-approval`, `G23-format-owner`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## BL-A10 — HIGH

**Source:** CX-21 whole-system benchmark and new correctness-head decisions

Selecting easy examples/abstaining on the rest inflates retained accuracy and may mask worse system utility.

**Folded change:** Evaluate full operational policy, coverage, fallback cost, abstention, final task evidence and locked thresholds; do not promote from confidence alone.

**Amended owners:** `specs/CX-21-coding-frontier-benchmark-lab.md`; `specs/CX-06-routing-calibration.md`.

**Acceptance:** `G21-policy-outcome`, `G36-reliability`. Verification scope: `PRODUCT_GATE_NOT_RUN`.

## Remaining evidence required

Run live source reconciliation before choosing modules or claiming features exist. Production trust-store/signature verification, archive fuzzing, protected process/network isolation, GPU/adapter races, real model quality/calibration and cross-project transfer all still require execution in Axon. Included format checks establish only the stated source/container contract. A document gate passing never changes a product gate from `NOT_RUN`.
