# Validation evidence — v0.21

The untouched v0.20 baseline validator passed, and all **249** original offline tests passed. The source benchmark analyser self-test passed **4 cases**. The generated v0.21 pack passed **367 offline unit tests**, including **118 added tests**, in the recorded 10.306-second run. All 37 specs, 255 tasks and 424 gates were structurally validated with zero orphan gates.

The read-only upgrade planner matched all **1,213 source files** against the reviewed inventory, reported no changed/missing/unsafe paths, and identified all five legacy gate-script compatibility issues. No source files were changed. Untracked files in a different live checkout must still be reviewed separately; the planner does not enumerate or delete them.

See [machine-readable receipt](VALIDATION_RUN.json), [full v0.21 unittest log](v021_tests.log), [source planner report](upgrade_plan_reviewed_source.json), retained baseline logs and the final root `package_validation.json` receipt. The final checksum file and validation receipt are excluded from their own hash inventory to avoid self-reference; other delivered receipts and logs are hashed.

## What the added tests establish

They exercise model-free record validation, candidate/calibration/epoch identity, label-aligned permutation aggregation, stable tie ordering, strict checklist evidence, canonical context dependency closure, eligible routing, mutation-screen constraints, a bounded proposal state machine, usage accounting, source planning, research coverage and immutable-contract checks. One inherited package-mutation test was changed from a positional task lookup to the same B210 task ID so appending work packages does not change the test's meaning.

These tests do **not** run production provider caches, concurrency, endpoint authentication, effectful tools, real learned scorers or a statistical optimizer. They make selected invariants executable before source integration; the runtime gate plans remain distinct.

## Not executed

Rust/Cargo compilation and runtime tests were not executed because that toolchain was unavailable. No live model/provider inference, head/router training, RRSI search, GPU/performance benchmark, paper-result reproduction or MiCode peer round trip was executed. All **424 proposed product gates remain NOT_RUN**. This is a reviewed specification/build pack with reference fixtures, not a claim that those features are implemented in the actual Axon runtime.
