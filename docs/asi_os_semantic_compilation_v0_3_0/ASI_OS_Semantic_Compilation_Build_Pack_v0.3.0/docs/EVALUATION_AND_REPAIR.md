# Evaluation Fabric, Typed Decisions and Targeted Repair

## Separation of responsibilities
The Fabric orchestrates evidence gathering and acceptance. A Jev-like adapter returns a typed judgment. A generative reviewer may explain a fault. A repair model may propose new text, code or IR. These are distinct calls with distinct costs and authority.

Official TypeSafe documentation describes Choice, Score and Noul outputs [S03–S05]. The adapter contract here does not presume Jev generates critique or exposes a training objective. The cited RLCD blog is an interpretation, not a reproducible provider training specification. This pack implements neither Jev nor RLCD.

## Evaluation contract
`evaluation-contract.schema.json` defines a versioned contract with owner, task class, required/advisory criteria, evidence requirements, pinned evaluators, target node/port and bounded repair policy. Contract changes require a new identity and review. The immutable hash, not a display name, is linked by the work plan.

A production evaluator adapter consumes an exact artifact digest and scoped evidence. It returns criterion ID, `pass|fail|abstain|error`, target digest, evaluator version, evidence references, typed decision details and measured usage. A learned adapter may recommend a verdict, but acceptance uses an independently approved decision policy with calibration evidence. The result's display explanation is not itself evidence.

## Typed decision mapping
| Provider concept | Normalized field | Interpretation |
|---|---|---|
| Choice | `selected`, `probabilities`, optional `provider_confidence` | Preference/classification among a specified candidate set |
| Noul | `probability` | Probability assigned to a specified yes/no proposition |
| Score | `score`, optional `probabilities`, `provider_confidence` | Position on a declared rubric, not task success probability |
| Deterministic check | `passed`, evidence | Predicate result under recorded inputs and checker implementation |

Do not coerce all rows into one `quality` float. Preserve full distributions when available, candidate IDs/order, criterion version, model revision and input state identity. A missing or changed candidate option invalidates a cached decision. Candidate-set preference probability does not equal the chance that a plan will pass its tests.

## Calibration and threshold governance
Define labels independently; split development, calibration and final holdout data by task family/time/source as appropriate. Report discrimination and calibration separately. For binary propositions, report Brier score and reliability bins; for multiclass decisions, use the corresponding multiclass metrics. Also report coverage, abstention and error among accepted cases at the deployed threshold. Attach sample counts and uncertainty intervals. A small Brier score alone does not validate a rare but costly failure mode.

Thresholds are fitted on approved calibration data and then frozen before final evaluation. Provider `confidence` is retained as its own signal, not relabeled “empirical accuracy.” Changes to model version, rubric, task distribution, state fields or data source trigger calibration review. Do not assume several questions evaluated together provide independent evidence.

## Risk-sensitive aggregation
Any required deterministic `fail`, `error`, missing result or `abstain` blocks release. Learned criteria require a valid decision policy and applicable calibration reference before automated acceptance. Advisory style scores can prioritize repair but cannot cancel a correctness failure. The production policy may escalate rather than retry. High-impact actions require authorization and relevant preconditions before dispatch, independently of post-output review.

## Repair lifecycle
1. Identify the failed target and supporting evidence; produce a scoped RepairRequest.
2. The generator proposes a new artifact/plan revision without editing protected criteria.
3. Validate the proposal against the expected parent revision and allowed surface.
4. Invalidate changed nodes plus transitive consumers. Reuse only authorized, fresh, compatible captures.
5. Execute the affected portion in isolation; rerun mandatory acceptance and affected regressions on the exact final output.
6. Release only after passing evidence is bound to that output. Otherwise escalate within the finite repair/cost envelope.

The simulation demonstrates a defective document output, a failed completeness check, a repair capture and a second evaluation. All responses are synthetic. There is no live repair generator and no measured model-quality result.

## Judge red team
Test injected “give me full marks” instructions in the artifact, rubric references missing from the supplied state, swapped candidate order, verbosity/self-preference, poisoned retrieval, stale evidence, evaluator outage, false confidence, hidden-answer leakage and the judge being asked to grade its own unsupported assumptions. For UI, compare against human task-specific preferences and behavioral/accessibility tests. No generic aesthetic score can establish a user's preference.
