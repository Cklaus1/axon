# Axon 0.22 — executed validation and limits

**Result: PASS for package conformance and model-free reference tests. Runtime qualification: NOT RUN.**

This report describes actions actually executed while building the archive. The build delivers proposed specifications, implementation plans, reference semantics and tests, not an implemented Rust integration or a production-qualified compute profile.

## Executed checks

| Check | Actual result | Scope / evidence |
|---|---|---|
| Parent v0.21 package validator | PASS | Baseline consistency and integrity before edits. |
| Parent v0.21 unit suite | 367 passed | [Baseline log](baseline_v021_tests.log). |
| v0.22 unit suite | **567 passed; 0 failures, 0 errors, 0 skipped** | 200 new tests; [structured case results](TEST_RESULTS.json) and [full log](TEST_LOG.txt). |
| Complete package validator | PASS at the final seal | 37 specs, 287 tasks, 504 proposed CX gates, no orphan gates; [non-recursive final receipt](../package_validation.json). |
| Original Fabric validator, run without modifying reference files | PASS | 22 fixture/parser cases and 1 canonical-identity check; [report](FABRIC_REFERENCE_REVALIDATION_V022.json). |
| Parent-row and protected-byte checks | PASS | 255 task rows, 424 gate rows, protected neural/ACE bytes and all 40 original Fabric files retained. |
| Paired source inventory check | **2,072 / 2,072 matching** | Axon 1,213 and MiCode 859; no changed, missing, extra or unsafe paths; [report](PAIRED_SOURCE_CHECK_V022.json). |
| Original input-archive digest recheck | PASS, all four unchanged | [Archive report](INPUT_ARCHIVE_CHECK_V022.json). |
| Empty live-evidence check | **NOT_QUALIFIED, expected exit 2** | 156 required live obligations, zero submitted; [report](EMPTY_EVIDENCE_CHECK_V022.json). |

The Fabric case count is separate from the 567-test unit suite, not a claim of 22 real executions. Matching full source inventories is a byte-preservation check, not a line-by-line audit or a Git-cleanliness proof. The source review was targeted and static.

## Commands

From the trusted extracted package root:

```bash
python -B tools/validate_package.py
python -B tools/run_review_tests.py
python -B tools/plan_upgrade_v022.py --axon /path/to/axon --micode /path/to/micode
python -B integration/compute-fabric-v0_1-reference/tools/validate_package.py --report /outside/reference/fabric-report.json
python -B tools/check_v022_runtime_evidence.py --evidence fixtures/closed_loop_v022/empty-runtime-evidence.json
```

`run_review_tests.py` writes test receipts/logs and therefore changes the checksum inventory after running. Use `python -B -m unittest discover -s tests -v` to rerun without rewriting those receipts. An explicitly reviewed authoring update can regenerate views and hashes; a recipient should not refresh hashes to hide unexplained modifications.

## Evidence and authority boundaries

The bounded operational profile carries **78 new CX gates + 40 legacy CX obligations + 38 Fabric gates = 156 requirements**. The two additional new CX study gates belong to optional B286. Gate counts are obligations, not implemented features. All 504 CX gates and all 55 original Fabric product gates remain `NOT_RUN` in this delivery.

The evidence checker only checks structure/completeness. Even a fully populated table of claimed PASS records returns `REQUIRES_EXTERNAL_VERIFICATION`, not runtime qualification. Tests verify that forged, duplicated, synthetic and zero-assertion evidence cannot create a green release. Authenticated issuers, actual check results and current authority must be established by the implemented owners.

The Python journal, reservation and policy-pointer models are volatile executable semantics. They do not establish durable storage, real concurrency correctness, guest confinement, process cleanup or live policy uptake. Fixture activation remains labeled synthetic and never evidence of measured improvement.

## Environment and unexecuted scopes

Tested with Python **3.13.5** and jsonschema **4.26.0** in the build container. Neither `cargo` nor `rustc` was available. No Rust/source test suite, Linux/KVM qualification, live provider inference, model training, actual peer round trip, production policy activation, production rollback or statistical improvement study was executed.

**Engineering-qualified: false. Policy-activated: false. Measured-improvement-supported: false.** The actual runtime qualification must be carried out under B255–B285 against the current sources and suitable infrastructure. B286 is an independent optional benefit study; rejected or inconclusive candidates remain valid outcomes.

## Final integrity

The final package checksum manifest covers all delivered files except itself and the explicitly non-recursive `package_validation.json` receipt. The separate ZIP checksum covers the entire archive. This report does not contain a checksum of itself or recursively hash the final validation receipt. Original reports are archived under `history/v0_21/` and retain their historical scope.
