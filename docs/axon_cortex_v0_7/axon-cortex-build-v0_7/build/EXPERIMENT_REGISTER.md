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
