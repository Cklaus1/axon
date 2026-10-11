# EVL — frozen checklists and independent evidence

**Owners:** CX-01 and CX-21; CX-11 admission; CX-27 independent supervision. **Tasks:** B232/B233. **Reference:** RES-CHECKEVAL.

CheckEval motivates decomposing broad judgments into traceable binary questions. Its text-evaluation results do not prove programming correctness. Axon adopts evidence-linked criteria while preserving deterministic tests and unknown observations.

## Rubric contract

Freeze a rubric ID/version/digest, requirement and criterion IDs, domain/scope, authoritative applicability predicates, whether each criterion is hard-required or advisory, allowed evidence types, judge policy, aggregation and thresholds. Define an actual expected behavior, not a vague label such as “good code.” Criterion text is data, not a prompt that authorizes tools.

A result binds the rubric, task/attempt/candidate digest, criterion ID, evidence references, judge/runtime identity and `PASS`, `FAIL` or `UNKNOWN`. `NOT_APPLICABLE` is permitted only when a frozen applicability rule is demonstrably false. Missing criteria, duplicate criterion IDs, unavailable evidence and contradictory observations cannot yield automatic success. Unknown required criteria remain blocking. Advisory criteria may be summarized but cannot dilute or outvote a hard failure.

Keep criterion generation outside candidate execution. A candidate may suggest a new rubric for future review, but cannot alter the rubric selecting that candidate. Do not optimize weights on the final test set or use model self-confidence as evidence that applicability is false.

## Preserve source truth

The existing runner distinguishes observed checks from claims and generated proposals. Keep `CheckRun` for actually executed processes, `Claimed` for text claims and `Proposed` for generator outputs. Preserve the last final `Verified` result and hidden-check separation. An earlier good intermediate patch does not make a later failing episode successful.

A CLM/Jev/LLM checklist answer is a judge claim with provenance. It is not an observed filesystem or process result. Programmatic checks decide programmatic facts. Human UX/UI judgments are separately labeled preference evidence tied to the task, not fabricated objective truth. Judges sharing models, training data or copied rationales have correlated errors; agreement is not automatically independent confirmation.

## Evidence and qualification

Require candidate digest, actual command/effect evidence where relevant, evaluator revision and evidence availability. Freeze separate training, tuning, calibration and final-test roles. Report per-criterion failure/unknown counts, hard-pass rate, selective coverage, disagreement and final task outcomes. Calibrating a judge on evaluation data and then calling those same judgments an independent final test is invalid.

Build a deterministic checklist aggregator before integrating model judges. Test missing/extra/duplicate IDs, criterion swaps, candidate-controlled N/A, unsupported evidence, inconsistent final outcomes and copied self-reports. Evaluate model-assisted criteria against qualified human or deterministic labels on a held-out domain. No checklist PASS alone enables artifact admission or effect authorization.

The reference fixture implements strict aggregation for a small closed rubric. Runtime evidence authenticity and real judge accuracy remain separate product gates.
