# Changelog v0.14

## Repository-wide continuous improvement

- Added **CX-30 — Repository Improvement Plane** to generalize governed self-improvement from MiCode/Axon internals to arbitrary owner-approved repositories.
- Added immutable `ProjectImprovementContract`, `ProjectBaseline`, `ProjectImprovementCandidate` and `RepositoryExperimentReceipt` concepts.
- Added generic versioned project adapters for observe/build/test/benchmark/analyze/verify/rollback rather than language-specific optimizer assumptions.
- Added project-local baseline, risk-class, isolation, evidence, replay and rollback gates.
- Added **CX-31 — Cross-Project Learning Plane** for family-aware pattern mining, held-out repository transfer and scope-qualified promotion.
- Added repository transfer tiers P0–P5 and explicit family/cluster contamination controls.
- Added de-generalization: shared artifacts may narrow, split or demote when later evidence shows negative transfer.
- Added `CrossProjectPatternCandidate`, `TransferEvaluationReceipt`, `SharedCapabilityProposal` and `ApplicabilityRevision` protocol concepts.
- Added `build/REPOSITORY_OPTIMIZATION.md` and `build/CROSS_PROJECT_LEARNING.md`.
- Added B128–B139 and G30/G31 acceptance gates.
- Defined three optimization scopes: project-local optimizer → cross-project learner → platform self-optimizer.
- Preserved the invariant that shared knowledge never bypasses a receiving project's local authority/acceptance contract.
