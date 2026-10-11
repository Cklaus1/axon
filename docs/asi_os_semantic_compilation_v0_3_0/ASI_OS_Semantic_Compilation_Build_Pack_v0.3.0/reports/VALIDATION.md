# Validation Report — Build Pack v0.3.0

**Executed:** 2026-10-06. **Result:** PASS for offline package conformance and source-package alignment checks only.

This report records commands actually executed against this standalone build pack and the supplied Cortex/Forge source packages. It is not a repository-HEAD production release gate, independent security audit, performance benchmark, or live ASI OS integration test.

## Semantic Compilation package checks

| Command | Exit code | Result | Elapsed wall time in this environment |
|---|---:|---|---:|
| `python -B tools/check_pack.py --json` | 0 | 17 schemas; 3 valid bundles; 12 intentionally invalid plans rejected; 73 requirements mapped; 22 backlog tasks linked | 3.38 s |
| `python -B -m unittest discover -s tests -v` | 0 | **119 tests passed** | 17.10 s total command; unittest body 15.149 s |
| `python -B -m harness.simulate --case document` | 0 | Original artifact rejected; repaired artifact accepted; only unaffected normalization reused | 3.42 s |

The checker parsed 65 checked-in JSON files strictly at check time and made no network requests. The synthetic demo makes zero live provider calls and zero external business effects.

## Cortex / Forge source checks

The supplied Cortex v0.23 archive was validated using its documented `python -B` commands. Its validator reports `PASS`, package version 0.23, 303 composed tasks and 535 composed gates, with **0 product gates executed**. Its seven included integration tests passed. Captured evidence is in `upstream_cortex_v023_validation.json` and `upstream_cortex_v023_tests.txt`.

The embedded Forge v0.3 package validator reports `PASS`, version 0.3.0, 40 required paths, 17 schemas and 15 vectors. Captured output is in `upstream_forge_v03_validation.txt`.

Pinned source hashes:

```text
Axon Cortex v0.23  3fd2834a17ad7a7cb339c99d2b79c853d4ecb57a27585d7f7edf5928c62405d4
Axon Forge v0.3    e126f400a40829303b0007c2a0a6de4da219de7f7df7bf8c06e21f7c4718baa7
Semantic v0.2 base c082f0c81331b4e81acc00b589eb58e4f6222add2285f93b903fd05ece0b6b2b
```

## Alignment/adversarial evidence

The v0.3 requirement crosswalk contains all 73 normative requirements exactly once: 56 (76.7%) map primarily to existing Cortex/Forge owners, 13 (17.8%) are Semantic Compilation integration/lifecycle glue, and 4 (5.5%) are package/release-specific. This is a requirement-ownership classification, **not source-code similarity**.

The adversarial review records sixteen attack/failure modes. Folded fixes include canonical owner projection instead of duplicate IR authority; semantic-operation/mechanism separation; StrategyArtifact identity; CX-32/CX-33/CX-34 projections; hard-gates-before-ranking; Forge software-semantic ownership; explicit IntentIR + AIR → SIR lineage; stale-projection rechecks; cached-plan reauthorization; and independent effect receipts/evaluation.

Offline tests prove the shape/invariants of these contracts. They **do not** prove that arbitrary syntactically valid `cx32:*`, `cx33:*`, `cx34:*`, AIR or SIR references exist in a live canonical service. That requires live conformance adapters.

## Synthetic demonstration

The document fixture uses recorded synthetic draft outputs, not a model. Its deliberately narrow deterministic rubric rejects the incomplete draft. Revision 2 changes the draft-producing node, invalidates `draft`, `check`, `guard`, and `finish`, reuses only `normalize`, and reevaluates the exact repaired artifact before acceptance.

Coding and research bundles passed structural and selected semantic checks but were not executed. Learned auto-acceptance remains blocked because no trusted calibration/decision-policy integration is implemented.

## Environment

- Python: CPython 3.13.5
- OS: Linux 6.18.44 x86_64, glibc 2.41
- Reference dependencies are listed in `requirements.lock`.

The package uses `python -B` for validation to avoid generated `__pycache__` artifacts. Cortex v0.23's validator treats generated cache files as package pollution; running its unit tests without `-B` before validation can therefore create a test-order footgun. This is a packaging concern, not evidence of a Cortex runtime defect.

## Not tested or established

- live Cortex CX-19 IntentIR → CX-04 AIR lowering/round-trip;
- live CX-03 authority, CX-13 isolation/effect receipts, CX-32 UEG, CX-33 experiment or CX-34 resource integrations;
- live Forge SIR/Transform/Migration/BuildProgram execution;
- live MiCode coding-workflow integration;
- durable/parallel runtime, crash recovery and real external-effect reconciliation;
- production policy enforcement, signed approvals, sandbox escape resistance or tenant isolation;
- model/Jev accuracy or calibration;
- real-task quality noninferiority, latency improvement or cost reduction.

G0/G0A results establish package coherence and alignment invariants only. Production gates G1–G5 remain future implementation evidence.
