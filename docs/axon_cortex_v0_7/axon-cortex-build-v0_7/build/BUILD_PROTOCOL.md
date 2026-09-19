# Builder protocol: specification to evidenced implementation

## Authority and status

Every package specification starts Draft. Import does not make it Reviewed or Approved. First inspect Axon's existing spec template, `spec-meta` requirements, R-ID registry, invariant ledger, exit ledger and R39 evidence graph. Map CX identifiers into that system deliberately; preserve the source package IDs as aliases in migration notes.

Do not overwrite the repository's build/governance protocol with this file. This is a proposed Cortex-specific supplement, subject to its existing gates.

## Repository intake

Record repository root, branch/commit, dirty files, environment/toolchain and local changes. Do not reset, clean or overwrite user work. Read prior specs/tasks/prototypes before designing replacements. Reproduce source-referenced gates only after checking what they execute and ensuring a safe environment.

The Axon attachment documents commands including `cargo build -p axon-core --no-default-features --bin axon`, `axon check`, `axon test` and parity scripts; those are prior-art leads, not commands executed by this package. The real repo may have changed. A failing or unavailable command is evidence to record, not something to hide with `|| true`.

Create an intake map with exact implemented interfaces, engine support, required dependencies and PASS/FAIL/SKIPPED/UNAVAILABLE evidence. Resolve document contradictions through code and tests; propose an invariant change when needed.

## Work-package contract

A work package identifies spec requirements, upstream slices, permitted paths, protected files, expected interfaces, tests/fixtures, acceptance gates, migration/rollback plan, resource budget and stop rules. Before code, state the decisive fork and why the alternative was rejected. Limit the package to one reviewable behavior change or tightly coupled contract migration.

Mark proposed paths as proposed until confirmed. Never declare an unimplemented acceptance script passing merely because its filename is in a spec.

## Inner build loop

1. Reproduce the defect or define a negative fixture that would fail without the feature.
2. Inspect the precise code/data seam and tests; gather only the needed context.
3. Implement the smallest change inside the allowed scope.
4. Run formatting/type/unit checks relevant to the seam, then the negative/positive contract tests.
5. If behavior spans interpreter/native, add parity or explicit-refusal cases. For host effects, test actual isolation rather than mocks alone.
6. Inspect the diff and evidence for accidental scope, budget, authority, ABI or documentation changes.
7. Record a result artifact tied to commit/diff/environment. Retry within budget or stop with a localized blocker.

A failing acceptance criterion is not repaired by weakening its assertion. A wrong test can be changed only through explicit review with a reproducible justification and impact on prior results.

## Outer build loop

At slice completion, the integrator runs seam/integration gates from a clean controlled workspace and compares the simple incumbent. Merge only after shared contract compatibility, adversarial cases, generated reference/docs, and rollback checks pass. Update source specs, requirement status and evidence together.

At milestone completion, demonstrate the end-to-end workflow with the actual interfaces—not a collection of mocks. A mock conformance gate and a real enforcement gate are separate records. Re-run the baseline when environment/model/toolchain changed.

## Meta build loop

After several slices or an unexpected regression, review whether the bottleneck is observation, action availability, data, inference, orchestration, tests, or poor task framing. Run a bounded ablation, not a global rewrite. Reprioritize future work based on evidence; do not let a meta agent rewrite admission policy or mark its own research successful.

Architecture changes require an ADR/spec amendment, dependency impact, invariant/TCB analysis, migration and fresh gates. “A better design” is a proposal until it survives the same independent process.

## Reflex backend and corpus protocol

When implementing B17–B20, treat the Axon Reflex ABI as the controlled variable and the backend as the experimental variable. Do not change the question schema, candidate-construction policy, hidden verifier, or authority envelope between backend comparisons unless the experiment is explicitly about that change.

Required sequence:

1. Freeze the backend-neutral question/response schema and probability-provenance enum.
2. Add conformance fixtures for dynamic candidate sets, multi-token options, abstention, invalid scores, stale state and branch conditions.
3. Add at least one adapter from each feasible family: generative constrained output, direct option logits, sequence scoring, and learned/option-conditioned head. Missing families are reported as unavailable, not silently substituted.
4. Build the grouped decision corpus from eligible episodes; keep teacher labels distinct from independently verified outcomes.
5. Run destructive controls: shuffled/empty/wrong/stale state, candidate reorder/rename, irrelevant context, adversarial candidates and candidate-omission/scope-expansion.
6. Record state-prefill, incremental question/option time, memory/cache mode, total speculative work, raw scores and calibrated values with exact model/tokenizer/runtime revisions.
7. Fit calibration only on permitted calibration splits and evaluate on grouped held-out task families.
8. Compare selective task utility, coverage, quality, latency and cost against the locked simple baseline before adopting a backend or router.

Do not infer calibration from normalized logits or entropy. Do not fabricate probabilities for label-only adapters. Do not compare backend A on one candidate catalog to backend B on a different catalog and call the result an inference benchmark.

Question decomposition is itself an experiment. When splitting a broad judgment into narrower subquestions, register the deterministic composition rule, compare against the original formulation, and preserve both versions in the corpus. A better decomposition may become a reusable Reflex template; it does not automatically become a safety gate.

See [REFLEX_BACKEND_BAKEOFF](REFLEX_BACKEND_BAKEOFF.md), [REFLEX_DECISION_CORPUS](REFLEX_DECISION_CORPUS.md), and [REFLEX_CALIBRATION_LAB](REFLEX_CALIBRATION_LAB.md).

## Resource and stop policy

Before running a work package, set wall time, maximum model/tool calls, retries, compute/storage and maximum candidate submissions. Suggested development defaults are finite and local: at most two automatic retries of a failed action, three consecutive no-progress iterations, and a 30-step task ceiling. These are proposed starting limits, not measured optimal settings or production guarantees.

Stop immediately on attempted authority expansion, suspected secret exposure, unbounded subprocesses, unknown non-idempotent outcomes, policy/signer mismatch or corruption of protected evaluation artifacts. Ordinary implementation failures can be retried inside budget; safety uncertainty is not a reason to disable the control.

## Definition of done for a slice

The behavior is implemented and documented; positive/negative/adversarial cases pass; engine/host support is explicit; evidence names exact commands and artifacts; no protected gate was weakened; source reference and generated docs are updated where required; rollback is tested; remaining limitations are recorded. Owner review occurs under the actual repository workflow.

Required gates report five states. Only PASS satisfies a required gate. SKIPPED, UNAVAILABLE and INCONCLUSIVE prevent a completion claim for that guarantee. Product results cannot be inferred from the package validator.

## Builder handoff

Leave a concise report: implemented change, files/digests, commands/results, observed limitations, failed hypotheses, next unblocked slice and stop reason. Keep secrets and private model reasoning out of handoff text. Link auditable artifacts rather than copying enormous logs into the next prompt.

## External backend and model intake (v0.3)

Before an external model/SDK participates in protected evaluation, complete [DEPENDENCY_ADOPTION](DEPENDENCY_ADOPTION.md) and [REFLEX_CONFORMANCE](REFLEX_CONFORMANCE.md). Do not begin by copying benchmark claims into acceptance criteria. Pin effective dependencies, reproduce a minimal example, run Cortex conformance fixtures, then register matched experiments.

A question-design review precedes every new Reflex family. Mechanically decidable freshness, authorization, arithmetic, authenticated-check and structural invariants stay deterministic. For semantic questions, record scope, candidates, evidence required, abstention/absence behavior and exactly how the answer is consumed.

Candidate recall is evaluated before decision quality. If the correct/suitable action is missing from the catalog, the failure belongs to observation/candidate construction rather than the Reflex backend.

## External experience / MiCode protocol (v0.4)

For slices using MiCode or external repositories, record the source-system/repository revisions, schema/migration versions, data-use/license policy, omissions, semantic-object mapping quality, evaluator version and whether the evidence is observed, teacher-labeled, simulated or inferred. Reproduce consequential knowledge claims locally before promotion. Never let a successful external episode substitute for an Axon capability grant, verifier result or invariant check.

## Intent-first build discipline (v0.5)

For user/system goals that originate as natural or structured prose, freeze the typed Intent IR and its approval/evidence requirements before executing consequential work. Builder changes that alter Intent IR semantics, semantic rendering, ambiguity handling or lowering require explicit fixtures showing old/new behavior; they cannot be justified solely by the downstream model producing nicer code. A self-improvement builder may propose `ImprovementIntent` but cannot edit the gate/evaluator that admits its own candidate.
