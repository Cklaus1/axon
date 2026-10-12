# v0.16 — reviewed Neural Programs and hardened build contracts

Date: 2026-09-21. Basis: supplied v0.15 ZIP plus standalone CX-36; document review, not an external implementation audit.

## Additions and consolidation

Added the reviewed CX-36 source/artifact/portable-contract spec; preserved CX-23 lifecycle ownership, CX-34 registry and CX-11 admission. Added strict JSON source/manifest schemas, inert positive/negative format fixtures, four review passes and a 34-finding change ledger. CX-35 remains reserved, not fabricated.

Added B152–B164 Neural Program source/container/runtime-integration/pilot tasks and B165–B173 review-integration tasks. Existing B00–B151 IDs stay stable. Twenty previously unassigned gates receive task owners. Early instrumentation, evidence and minimal registry tasks no longer wait for mature self-improvement clients. Optional hosted importer B157 is not on any mandatory release path.

## Safety corrections

Separated schema validity, source attribution, semantic validation, statistical correctness and authority. Bound exact executable subject before evaluation; detached release metadata avoids hash cycles. Added archive/resource checks, current revocation/rollback eligibility, private/offline controls, adapter/cache identity, bounded fallback/cancellation and self-optimization recursion, protected evaluator feedback, explicit pin overflow, truthful replay/retention and realized-vs-estimated evidence.

## Package integrity

Regenerated complete master/spec index/task/gate views directly from modular sources. Validator now checks view freshness, gate-to-task coverage, full dependency orders, schemas/examples, claim statuses and complete hash inventory; adversarial mutation tests must actually run. Older checksum snapshots and stale validation outputs were removed rather than being relabeled current. Historical changelogs remain.

## Scope

No Axon source code, training run, runtime sandbox, GPU backend, hosted model call or product acceptance gate was executed. MiCode package was not changed. Final document/test counts are generated in package_validation.json, not copied from an earlier summary.
