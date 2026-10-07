# Implementation-agent bootstrap — Axon v0.21

You are upgrading the current Axon repository using the reviewed `axon-cortex-build-v0_21` pack. Treat the current source, uncommitted work and executed repository evidence as authoritative implementation facts; the pack supplies proposed contracts and tasks, not permission to replace that work.

First read README, `review/SOURCE_REVIEW_V021.md`, `build/UPGRADE_V021.md`, `integration/V021_INPUT_LOCK.json`, `integration/SOURCE_OWNER_MAP.json`, `build/RESEARCH_INTEGRATION_V021.md` and the selected release profile. Run the pack validator, unit tests and read-only upgrade planner. Record current source commit/dirty state and investigate inventory drift. Do not reset, clean, stash, overwrite old vendored packs, auto-download models or enable shell proxies.

Reconcile B223/B224 first. The source uses CX-35 for Reflex serving whereas the package reserves that ID; preserve the source name and map to CX-05. The legacy package gate expects old checksum/receipt/CLI behavior; a path substitution alone is insufficient. Package NOT_RUN is not a request to reset live implementation evidence. Keep source-generated gate evidence separate from package fixture PASS.

Preserve CX-36 r0.2, `axon.nps/1`, `axon.np/1`, `axon.cjson/1`, existing ACE v1 JSON schemas, the old `axon-reflex/1` profile and Cargo workspace version unless a reviewed real protocol/runtime change requires otherwise. EVO/DEC/EVL/RTR/CVM/SPX/TEL are aliases of existing owners, not a mandate for seven services or duplicate registries. Candidate views use CX-34; evidence uses CX-10 and the existing audit/replay system; permission remains deterministic; independent admission remains CX-11.

Use current `ReflexBackend`, `PatchGenerator`, runner authorization/check logic, policy adapter, AI gateway, kernel budget and recording/replay seams. The current Reflex core ignores input and returns a deterministic decision string; keep it as a fixture, never label it CLM/Jev. Extend new behavior through explicit negotiated capability profiles with compatibility tests.

Implement one bounded task at a time following the manifest dependencies. Start with package conformance and deterministic guards. For each task produce the exact diff, source-level tests, relevant gate evidence and any limitations. Add negative tests before implementation where practicable. Preserve hidden evaluator separation, claimed/observed distinctions and the final whole-task outcome. No score or checklist answer may authorize a tool or bypass required checks.

CLM requires pinned model/head/encoder/tokenizer/pooling identity, no silent truncation, principal-isolated caches and atomic epochs. Jev/pijev require complete label-aligned distributions and wrapper-bound calibration. RRSI-inspired evolution cannot edit graders, grants, audit or admission. Context projections rebuild from originals and preserve mandatory dependency closures. Speculation initially produces isolated responses/patches only; repairs invalidate old verification and all branches share a bounded cost/deadline. Unknown usage and unknown effects remain unknown until reconciled.

Run real Rust/source tests under the live toolchain; the supplied Python receipts are not a substitute. Learned providers stay disabled/shadow until independently qualified on frozen tasks and actual model revisions. Rejected/inconclusive research outcomes are valid. Do not make reproducing published SOTA or a fixed savings percentage a release dependency.

The MiCode v0.15 file is only a proposed consumer addendum; no consumer source/archive was reviewed. Do not claim that peer updated or tested. Before any peer work, inspect the actual consumer, map its real task IDs and pin exports in its own reviewed upgrade.

Finish each bounded slice with a factual report: files changed, preserved behavior, tests actually executed, product gates with source evidence, unresolved blockers and rollback path. Never convert package-wide gates to PASS from a documentation run or self-approve a research strategy.
