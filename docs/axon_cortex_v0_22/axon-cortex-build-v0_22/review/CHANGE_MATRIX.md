# Review change matrix

Two review passes per requested subject; all source files are amended, no product completion asserted.

| Finding | Severity | Pass | Primary amended owner | Gates |
|---|---|---|---|---|
| NP-D01 | high | CX-36 / design | `specs/CX-36-neural-program-artifact-contract.md` | G36-naming, G23-format-owner |
| NP-D02 | high | CX-36 / design | `specs/CX-36-neural-program-artifact-contract.md` | G36-source, G36-container |
| NP-D03 | high | CX-36 / design | `specs/CX-36-neural-program-artifact-contract.md` | G36-identity, G11-release-binding |
| NP-D04 | high | CX-36 / design | `specs/CX-36-neural-program-artifact-contract.md` | G36-typed-io, G36-source-bound |
| NP-D05 | medium | CX-36 / design | `specs/CX-36-neural-program-artifact-contract.md` | G36-jobs, G15-neural-contract |
| NP-D06 | high | CX-36 / design | `specs/CX-36-neural-program-artifact-contract.md` | G36-applicability, G36-replay |
| NP-D07 | medium | CX-36 / design | `specs/CX-36-neural-program-artifact-contract.md` | G36-self-host, G36-source-bound |
| NP-A01 | critical | CX-36 / adversarial | `specs/CX-36-neural-program-artifact-contract.md` | G36-container |
| NP-A02 | critical | CX-36 / adversarial | `specs/CX-36-neural-program-artifact-contract.md` | G36-trust, G11-release-binding |
| NP-A03 | critical | CX-36 / adversarial | `specs/CX-36-neural-program-artifact-contract.md` | G36-import, G36-offline, G36-fallback, G03-fallback-recheck |
| NP-A04 | critical | CX-36 / adversarial | `specs/CX-36-neural-program-artifact-contract.md` | G36-adapter-isolation |
| NP-A05 | high | CX-36 / adversarial | `specs/CX-36-neural-program-artifact-contract.md` | G36-fallback, G36-jobs, G34-bounded-fallback |
| NP-A06 | critical | CX-36 / adversarial | `specs/CX-36-neural-program-artifact-contract.md` | G36-rollback, G34-revocation-race |
| NP-A07 | high | CX-36 / adversarial | `specs/CX-36-neural-program-artifact-contract.md` | G36-reliability, G20-target-separation |
| NP-A08 | high | CX-36 / adversarial | `specs/CX-36-neural-program-artifact-contract.md` | G36-replay, G10-retention-replay |
| NP-A09 | high | CX-36 / adversarial | `specs/CX-36-neural-program-artifact-contract.md` | G36-source-bound |
| BL-D01 | critical | build package / design | `tools/package_views.py` | G00-assurance-separation |
| BL-D02 | high | build package / design | `task_manifest.json` | G00-assurance-separation |
| BL-D03 | high | build package / design | `schemas/PROTOCOLS.md` | G32-closure-snapshot, G33-estimate-class, G34-bounded-fallback |
| BL-D04 | high | build package / design | `task_manifest.json` | G00-assurance-separation |
| BL-D05 | high | build package / design | `tools/validate_package.py` | G00-assurance-separation |
| BL-D06 | medium | build package / design | `build/IMPLEMENTATION_RELEASE_SLICES.md` | G00-assurance-separation |
| BL-D07 | medium | build package / design | `schemas/PROTOCOLS.md` | G06-reliability-event, G20-target-separation |
| BL-D08 | medium | build package / design | `specs/CX-31-cross-project-learning-plane.md` | G31-transfer-dimensions |
| BL-A01 | critical | build package / adversarial | `specs/CX-24-semantic-perception-retrieval.md` | G24-extraction-strength, G00-assurance-separation |
| BL-A02 | critical | build package / adversarial | `specs/CX-33-causal-active-experimentation-plane.md` | G33-estimate-class |
| BL-A03 | critical | build package / adversarial | `specs/CX-11-crystallization-admission.md` | G11-release-binding, G32-closure-snapshot, G34-revocation-race |
| BL-A04 | critical | build package / adversarial | `specs/CX-13-os-runtime.md` | G13-shadow-egress, G29-change-risk, G03-fallback-recheck |
| BL-A05 | high | build package / adversarial | `specs/CX-28-semantic-working-set-manager.md` | G28-pin-overflow, G10-retention-replay |
| BL-A06 | high | build package / adversarial | `specs/CX-01-evaluation.md` | G01-feedback-budget, G29-recursion-budget |
| BL-A07 | high | build package / adversarial | `specs/CX-27-semantic-supervisor-plane.md` | G27-finality, G34-bounded-fallback |
| BL-A08 | high | build package / adversarial | `specs/CX-05-reflex-inference.md` | G05-feature-profile |
| BL-A09 | high | build package / adversarial | `specs/CX-29-reflexive-self-application-plane.md` | G19-contract-approval, G23-format-owner |
| BL-A10 | high | build package / adversarial | `specs/CX-21-coding-frontier-benchmark-lab.md` | G21-policy-outcome, G36-reliability |

## Coordinated schema / lightweight Reflex amendment

See [design review](JIMOTHY_WEBMCP_DESIGN_REVIEW.md), [adversarial findings](JIMOTHY_WEBMCP_ADVERSARIAL_REVIEW.md) and [cross-project requirement map](../integration/SCHEMA_REFLEX_REQUIREMENTS.json). This amendment adds supported-subset lowering, explicit fixed-task distillation/evaluation, scoped teacher jobs, runtime qualification and fail-closed consumer cases; it preserves existing wire/artifact formats and external authority. Later learners/context/schema families remain optional.
