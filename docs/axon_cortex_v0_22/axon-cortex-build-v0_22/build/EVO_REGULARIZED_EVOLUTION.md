# EVO — regularized harness evolution

**Owners:** CX-29; CX-08/12/33 experiment policy; CX-11 admission. **Tasks:** B234–B236, optional B251. **Reference:** RES-RRSI in the [source registry](../integration/RESEARCH_SOURCES.json).

## Source-derived lesson and Axon adaptation

RRSI regularizes harness edits around a frozen backbone through bounded proposals, history-aware exploration, a critic and pruning. This pack adopts that design direction, not its benchmark scores. Axon additionally protects authority, separates adaptive selection from final evaluation and requires independent admission. Existing compiler rewrite search in `axon-core/src/improve.rs` is a different intervention class and is not replaced.

## Proposal contract

A proposal identifies the incumbent content digest, hypothesis, touched component IDs and exact diff, source/runtime/model versions, authorized edit surface, mutation budget, schedule position, parent experiment and all prior related hypotheses. Store successful, rejected, inconclusive, failed and no-op proposals. Novel wording does not create a new hypothesis identity when the actual intervention is identical. Each repetition receives a new attempt ID.

Classify intervention as `harness`, `model_or_head`, `compiler_rewrite`, `context_policy`, or `evaluation_policy_proposal`. Evaluation-policy proposals cannot run as ordinary agent candidates on the same test whose score selects them. Head fitting uses CX-25 and separate data release rules. A harness-only experiment pins all model revisions so a provider upgrade is not misattributed to an edit.

The initial proposer can be deterministic with a fixed edit budget. An RRSI-inspired annealed schedule is a qualified alternative, not mandatory magic constants. Freeze initial/final component limits, annealing function, iteration horizon and exploration budget before reading outcomes. Preserve enough budget to test neglected components, while refusing edits to authority, protected evaluation, audit integrity and emergency stop behavior.

## Experiment and selection protocol

Use immutable train/exploration, validation/selection, calibration/threshold and final-test partitions appropriate to the component. Group split by repository/task family and time where relevant. Log every exposure to a held-out set. Freeze task assignments, paired seeds, baseline calls, total spending limits, quality/noninferiority margins and stopping policy.

A leakage critic screens likely benchmark-specific changes before costly trials. Its rejection is logged and reviewable; critic approval is not proof against contamination. Always test generalization. A deterministic policy enforces protected paths and semantic boundaries independently of the critic's answer.

Compare paired task outcomes and full costs. Declare a noise floor and multiplicity/sequential-testing method suitable to repeated adaptive proposals; do not repeatedly sample a fixed holdout until a favorable p-value appears. An ordinary confidence interval after adaptive cherry-picking is not automatically valid. Statistical method, confidence level and minimum detectable effect must be selected in the actual experiment preregistration, not invented as universal constants by this pack.

`INCONCLUSIVE` retains the incumbent and evidence. An edit may be useful at higher cost only within a predeclared tradeoff policy; cheaper but measurably worse is not a default win. Report all failed/abstained tasks. The final held-out set must not flow into future training or proposal memory without a new split and disclosure.

## Pruning and governance

A pruner may propose removing an optional mechanism that is redundant under protected regression coverage. It cannot remove safety, audit, cancellation, recovery, attribution or required checks simply because their average benchmark contribution is near zero. Pruning is itself an intervention, evaluated and admitted like an addition.

Selection produces a candidate and evidence bundle for CX-11; it does not activate the candidate. Preserve independent proposer, evaluator and approver roles even when the same model family assists multiple roles. Meta-policy optimization is B251, slower and separate, with its own holdouts. It can optimize exploration parameters but not lower the immutable safety floor or rewrite promotion criteria.

## Build and falsification

Build hypothesis memory and deterministic protected-edit rejection first. Add the critic and fixed policy next, then annealed scheduling as an arm. Test repeated task/arm IDs, replayed rejected proposals, benchmark-answer hardcoding, grader edits through symlinks or indirect configuration, tiny noisy gains, uncounted expensive critic calls, and pruning of rarely exercised recovery. Run actual source tests before linking product gate evidence.

The executable reference checks only protected component membership, history identity and a simplified predeclared acceptance envelope. It does not implement a statistical optimizer, leakage detector or RRSI reproduction. Those are runtime qualification obligations.
