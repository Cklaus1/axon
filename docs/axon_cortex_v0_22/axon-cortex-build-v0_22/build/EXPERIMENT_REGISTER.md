# Initial experiment register

These are preregistration templates, not findings. Fill the task manifests, allowed data, hardware, budget, primary metric, non-inferiority margin and stopping rule before results are examined. All statuses: Not run.

| Experiment | Question and challenger | Control / key confound | Evidence required to continue |
|---|---|---|---|
| E01 — Structured loop | Does AIR/structured observation improve task completion economics? | Same strong model, tools, primer, task budget and hidden verifier without AIR. | Task-quality and end-to-end cost/latency results, not tool-call count alone. |
| E02 — Candidate catalog | Does bounded target selection reduce invalid actions without hiding necessary targets? | Wider authorized catalog plus retrieval-only baseline. | Candidate recall, invalid-action rate, scope-expansion success and full task outcomes. |
| E03 — Reflex backend bakeoff | Which backend family best serves each bounded decision workload under one frozen Reflex ABI? Generative adapter vs direct option logits vs sequence scorer vs learned decision head. | Same state projection, question schema, ordered candidate manifests, authority, task family, verifier and budget. | Verified task outcomes; grouped accuracy/NLL/Brier/risk-coverage; latency/cost/memory; raw-score provenance; destructive state/candidate controls. |
| E03A — Shared-state speculation | Do conditional parallel target questions and shared-state/prefix reuse help? | Sequential operation→target and ordinary parallel calls on the same backend. | State-prefill, incremental question/option timing; context/cardinality sweeps; discarded speculative compute; cross-field validity; task quality. |
| E03B — State dependence | Does the backend actually use the supplied state and dynamic candidate semantics? | Correct-state result versus shuffled/empty/wrong/stale state, candidate reorder/rename, irrelevant-state injection and adversarial option sets. | State-sensitivity report and candidate-recall/task outcome deltas; reject state-insensitive shortcuts. |
| E04 — Calibration/router | Does empirically calibrated selective execution beat always-Reason or raw-threshold routing? | Same observation/model budget; include all-abstain and always-act sanity checks; preserve raw score provenance. | Proper scoring, selective risk/coverage, subgroup/OOD behavior, Verified Utility at Coverage, and task economics. |
| E05 — Small Reflex pilot | Can grouped eligible labels support a cheaper option-conditioned specialist over runtime-generated candidate sets? | Teacher/strong model, direct-logit/sequence baselines, small untrained model and simple classifier/ranker. | Learning curves, destructive state controls, calibration/coverage and data costs; teacher agreement separated from real task outcomes. |
| E05A — Question decomposition | Does decomposing one broad judgment into narrower typed signals plus deterministic composition improve verified decisions? | Original monolithic question using the same state/model/backend and a compute-matched control. | Protected task outcomes, calibration, latency/cost, failure modes, transfer; preserve both formulations and the composition-rule artifact. |
| E06 — Prediction value | Do outcome predictions improve check selection? | Dependency heuristic, base rates and no model. | Held-out forecasts plus real planner outcomes after charging prediction cost. |
| E07 — Active probes | Does experiment selection resolve ambiguity efficiently? | Fixed checks, random permitted probe and deliberate reasoning only. | Matched reset/control, total checks/cost, diagnosis and completed tasks. |
| E08 — Guarded specialization | Can one recurring reasoning pattern become a tool/rule/policy? | Approved incumbent with the same authority and task set. | Applicability envelope, independent quality gate, authority non-expansion and fallback behavior. |
| E09 — Representation | Does a new representation help on a novel family? | Existing model, standard representation and scrambled/removed concept. | Transfer and few-shot curves, total compute, source lineage and counterexamples. |
| E10 — Learning portfolio | Does meta-allocation improve learning efficiency? | Fixed round-robin or manually preregistered experiment order. | Progress per real budget on a fixed protected audit suite; no metric/task rewriting. |
| E11 — Host portability | Does a new host preserve the supported contract? | Original supported host/profile. | Actual effect, recovery, kill, quota and replay gates—not only compilation. |
| E12 — Compiler lowering | Does native/syntax integration justify maintenance cost? | Existing library/interpreter path. | Authoring fluency, semantic/effect parity, refusal cases, build/runtime costs and docs drift gates. |

## Stop or pivot conditions

Do not build a custom architecture merely because an adapter experiment underperforms. Check labels, grouped-split leakage, observation completeness, state sensitivity, candidate recall, option/tokenization semantics, calibration and baseline strength first. Stop a branch when its preregistered information budget is consumed or measured benefit is absent; retain the simple baseline and document the negative result.

A representation or world-model experiment with inconclusive transfer remains research. A failed safety gate cannot be compensated by task-score or latency improvements. Optional model research does not prevent shipping a useful hosted runtime.

## v0.4 required external-experience experiments

- **Bridge conformance:** MiCode episode round-trip, migration, Unknown/Denied/Failed preservation, authority non-transfer.
- **Source-value ablation:** self-only vs MiCode-external vs generated experience under matched task/evaluator budgets.
- **Repository-history value:** static snapshot extraction vs history/outcome-aware extraction.
- **Knowledge transfer:** extracted pattern active vs ablated/scrambled on held-out repository families.
- **Crystallization economics:** baseline multi-step workflow vs promoted guarded skill/tool, including verification and deoptimization cost.

- **E14K — Kev-style coding Reflex:** small pretrained backbone + adapter + block-causal isolated branches + pointer/listwise head versus strongest simple Reflex baselines; report learning curves, packing speedup and transfer.
- **E14P — Permutation invariance:** canonical-order/shuffle augmentation/permutation-consistency objectives under matched compute; report flips, probability spread, NLL/Brier, selective task quality and downstream outcomes.
- **E14N — Candidate absence:** ordinary-only candidate sets versus typed `NONE`/`OBSERVE_MORE`/`ESCALATE` augmentation on missing-target and distractor fixtures.
- **E14T — Coding Transfer Frontier:** same-repo → unseen-repo → unseen-family/task-family → cross-language ladder with fixed quality/coverage/compute criteria.


## v0.9 experiment families

### EXP-CSC-01 — Specialized representation bakeoff

**Question:** for one stable recurring coding function, does a rule/template, specialized decision model or neural program improve verified utility versus general Reflex/THINK under the same applicability envelope?

Freeze the semantic contract, eligible data, transfer partitions, quality floor, resource envelope and fallback before comparing. Report training/compile cost and resident artifact cost, not only call latency.

### EXP-NP-01 — Shared-base learned-function runtime

**Question:** do multiple small learned function artifacts over one pinned base provide useful local/offline economics without semantic identity/fallback ambiguity?

Measure cold/hot load, resident memory, switch latency, cache isolation, typed-output validity, artifact mismatch behavior and offline completeness. A hosted compile/infer service is an optional reference arm, not the success criterion.


## Self-application experiment family

Register `SELFAPP-*` experiments with incumbent/challenger component revisions, ImprovementIntent, corpus roles, replay equivalence class, shadow/no-effect receipt, protected evidence and activation/rollback lineage. Initial families: model routing, working-set selection, semantic rule/skill selection and context GC.

## v0.16 review experiments

- **NP-format/adversarial:** deterministic source/manifest round-trip and malformed archive/identity controls; no inference-quality claim.
- **NP-source-bound diagnostic pilot:** exact parser versus current incumbent versus one local learned candidate; measure both hallucination and omitted-diagnostic recall, downstream repair quality, all latency/cost and abstentions.
- **Reliability target study:** same cohort/event, compare top probability, entropy/margin and a separately fit correctness estimator; keep panel agreement and task success separate.
- **Shadow intervention control:** change context/definition policy while fixing upstream state/catalog; separate observational replay from actual alternate execution, and charge shadow disclosure/compute.
- **Release race drill:** prepare/select an artifact, revoke/change scope before dispatch, attempt stale cache and previous-good activation; no protected effect may occur.

All hypotheses, label semantics, denominators, allowed exposures, statistical rules and stopping budgets are registered before protected outcomes. A rejected challenger or inconclusive estimate is recorded, not retried under a rewritten exam.

## ACE comparison registration

Register core interoperability, three-family model comparison, Neural Program learned-function and native-state experiments separately. Freeze task/contract, representation/order/visibility, model/processor/runtime, data-role/family partitions, hardware, warm/cold boundaries, selection/calibration, attempt/fallback budgets and outcome metrics. Unavailable arms are not zero cost or passing baselines. Include exact parser/SELECT-COPY where applicable and permit rejection/inconclusive.

## v0.20 schema / lightweight Reflex supplement

Apply [schema frontend](SCHEMA_DECISION_FRONTEND.md) and [lightweight distillation](LIGHTWEIGHT_REFLEX_DISTILLATION.md) under existing owner contracts. B210–B213 are model-free schema/active-output/source/authority seams; B214–B216 are governed fixed-task data/training/independent acceptance; B217–B219 qualify runtime/disposition and real peer use; B220 maintains offline package conformance. B221/B222 are optional context/richer-learner hypotheses, never success prerequisites for the new bounded profiles.

Preserve exact task/labels/source/runtime identity, partition/evaluator custody, null-threshold/OOD fallback, local permissions and independent completion. No new .reflex/ABI owner, schema-plan-to-trained-artifact shortcut, in-place self-training or head-only speed claim. [Coordinated apply order](APPLY_V020_WITH_MICODE_V014.md) preserves current live evidence.
