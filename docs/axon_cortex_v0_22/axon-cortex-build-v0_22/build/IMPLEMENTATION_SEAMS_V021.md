# v0.21 implementation seams and source test handoff

Every path below exists in the uploaded source and is hash-pinned in [RUNTIME_SEAM_MAP](../integration/RUNTIME_SEAM_MAP.json). A listed file is an integration seam, not proof the new feature is already implemented. Proposed adapters/tests must be added through existing owners; no new seven-crate architecture is required.

## B223, B224, B249

Existing paths: `scripts/cortex_package_gate.sh`; `governance/cortex_gate_execution_registry.json`; `governance/cortex-v015/CX-35-reflex-serving.md`.

Preserve historical gate registry and source IDs. Review a new package-gate invocation using tools/repo_gate_v021.py; stage documents under a new path only after approval.

Required source tests: Existing gate compatibility plus dry-run/no-mutation and old/new profile checks. No product PASS derived from document tests.

## B225, B227, B228, B229, B230

Existing paths: `crates/axon-reflex/src/lib.rs`; `crates/axon-reflex/src/bin/sidecar.rs`; `crates/axon-reflex/tests/principal_isolation.rs`; `crates/axon-reflex/tests/protocol_hardening.rs`.

Preserve ReflexBackend, legacy Decision and Refusal behavior. Add negotiated research capability handling and provider adapters under the existing crate; any new files are proposed, not already implemented.

Required source tests: Old client/new service and new client/old service; bounds and duplicate fields; wrong principal; epoch reload; CLM truncation/cache; incomplete Jev/pijev replies.

## B226, B237, B238, B248

Existing paths: `crates/axon-ai/src/lib.rs`; `crates/axon-core/src/kernel.rs`; `crates/cortex-policy-adapter/src/main.rs`.

Connect eligible model/role candidate views to existing CX-34 design and current AI gateway. The actual learned registry adapter is build work, not presumed to exist. Keep policy adapter and kernel grants authoritative.

Required source tests: Private candidate filtering before provider calls; revoked/unhealthy/incompatible routes; cache/session transfer; bounded selector recursion and fallback.

## B232, B233, B241, B242, B250

Existing paths: `crates/axon-cortex/src/generate.rs`; `crates/axon-cortex/src/select.rs`; `crates/axon-cortex/src/runner.rs`; `crates/axon-cortex/src/episode.rs`; `crates/axon-cortex/tests/conformance.rs`.

Use PatchGenerator and deterministic authorization/check paths. Extend proposal records and evaluation evidence without altering claimed/observed or final verification semantics. Insert one selected verified-proposal barrier, not a second executor.

Required source tests: Actual final checks and hidden-check isolation; repaired digest mismatch; permission denial; cancellation/unknown effect; duplicate commit; full task outcome.

## B239, B240, B245

Existing paths: `crates/axon-cortex/src/generate.rs`; `crates/axon-cortex/src/episode.rs`; `crates/axon-core/src/replay.rs`.

Connect CX-28 original-history projection to prompt assembly and existing record/replay seams. Retention and event authority remain with their owners. Do not assume a complete context VM already exists in these files.

Required source tests: Consuming-tokenizer budgets; required pin/tool-pair closure; repeated compaction and prefix mismatch; retained/deleted source references; no fresh provider calls during replay.

## B234, B235, B236, B251

Existing paths: `crates/axon-core/src/improve.rs`; `crates/axon-core/src/rewrite_dsl.rs`; `crates/axon-cortex/src/runner.rs`; `crates/axon-cortex/benchmarks/analyse_real_model.py`.

Add harness-experiment orchestration under CX-29/CX-08 with independent admission. Existing compiler improvement remains a separate intervention class. Reuse frozen task runners and task/attempt evidence; do not pretend compiler rewrite search is RRSI.

Required source tests: Protected-path/semantic changes; hypothesis history and repeated-trial IDs; holdout exposure; budget/noise-aware comparison; independent admission and pruning regressions.

## B231, B246, B252, B253

Existing paths: `crates/axon-reflex/src/lib.rs`; `crates/axon-cortex/benchmarks/real_model.py`; `crates/axon-cortex/benchmarks/analyse_real_model.py`.

Implement CX-20/CX-25 qualification and learning lifecycle as explicit new work. Existing source does not demonstrate CLM head training or ANN routing. Retain immutable encoder/head/data identities and candidate disposition.

Required source tests: Task/repository splits and unchosen-not-negative labels; model-weight pins; qualification versus serving epoch; exact versus ANN shortlist recall; safe rollback.

## B243, B244, B245

Existing paths: `crates/axon-ai/src/lib.rs`; `crates/axon-core/src/kernel.rs`; `crates/axon-core/src/replay.rs`; `crates/axon-audit/src/lib.rs`; `crates/axon-cortex/benchmarks/real_model.py`; `crates/axon-cortex/benchmarks/analyse_real_model.py`.

Extend observed usage categories while preserving the kernel spend authority and existing audit/replay. Use stable identity joins instead of positional joins. Treat total task costs as evidence, not a second debit path.

Required source tests: Overlapping input categories; duplicate call IDs; cancelled charges; unknown liabilities; cross-principal joins; all-assigned-task denominator; replay without billing again.

## B247

Existing paths: **MiCode source not supplied**.

Await actual MiCode source/support pack before creating its source patch or task-ID mapping. Included addendum is a proposed consumer requirement only.

Required source tests: Actual owner/provider/consumer pins, unsupported-profile refusal, cancellation and next-turn recovery, independent permission and completion.

## B254

Existing paths: `crates/axon-cortex/src/generate.rs`; `crates/axon-cortex/src/runner.rs`.

Optional best-of-N proposal orchestration only after bounded live pilot. No direct speculative tools.

Required source tests: All branch costs charged; exact selected-byte verification; concurrency, cancellation and at-most-once commit under actual source runtime.

## Source test execution

Run the actual workspace toolchain and repository gate entry points after inspection. The initial targeted commands are `cargo test -p axon-reflex` and `cargo test -p axon-cortex`, plus relevant AI/kernel/replay/policy tests and the normal source CI suite. The review environment did not contain Cargo/rustc, so no such run is claimed. Unit test names and runtime gate evidence must come from the real modified source, not these planned descriptions.
