# Axon Optimization and Experiment Protocol

## Canonical Cortex binding
Under the Cortex v0.23 profile, optimization does not create a private Axon protocol. Every compiler, repair, evaluator, retrieval, routing, resource-placement or transform variant is a versioned **CX-29 StrategyArtifact** (or a projection to the repository's equivalent), and every causal comparison is attached to a **CX-33 canonical experiment identity**. Outcomes are projected into **CX-32 evidence**; physical-resource observations are epoch-bound **CX-34 projections** only. Candidate strategies cannot mutate the protected CX-21 evaluator, admission rules, hard constraints, experiment stopping rule or their own promotion authority.

Hard correctness, security, authority and required-evidence gates run **before** Pareto/utility ranking. A cheaper or faster candidate that fails a hard gate is ineligible, not a lower-scoring alternative. Runtime dispatch still rechecks current CX-03/CX-13/CX-34 state because optimization-time projections are non-authoritative.

## Optimization domains
Node selection changes resources or deterministic implementations. Plan optimization changes graph shape. Artifact repair changes outputs. Compiler optimization changes generation strategy. Evaluator optimization changes the evidence-gathering policy. Treat each as an independently versioned experimental object; do not conflate a better answer with a better compiler.

## Safe transformations first
Only registry-authorized pure work may be deduplicated or cached without an additional effect contract. Parallelization requires independent data and nonconflicting effects. Replacing a model with code requires semantic equivalence for the defined task class. Downsizing a model requires outcome evidence; matching schemas is not sufficient.

Speculative execution is limited to isolated/read-only candidates with reserved budgets. A candidate score is a forecast; the selected candidate must execute and pass independent checks. No duplicate live payments, messages or deployments for an A/B comparison.

## Minimum comparison
Use four arms: current interpreted baseline; compiled-only; compiled plus evaluation; compiled plus evaluation and bounded repair. Record cold-start compilation separately from warm plan reuse. Compare a fixed model/tool set to isolate architectural effects, then optionally compare each strategy's optimized resource choices.

Primary denominator is tasks attempted, not only accepted easy cases. Report successful tasks, failures, abstentions, repairs, cost per attempted task, total cost divided by successes, p50/p95 latency, acceptance errors and human preference when applicable. If there are zero successes, cost per success is undefined/infinite, not zero.

## Experiment identity and randomization
Use experiment, task, trial, arm and run IDs. A repeated task/arm gets a distinct trial and run. Randomize or counterbalance arm ordering; use paired task-level analyses where appropriate. Include routing propensity when adaptive policies select arms. Retries do not create additional independent observations.

## Quality gate before economics
Pre-register task slices, labels, risk classes, quality metric, noninferiority margin, confidence level, sample-size/power analysis, multiple-comparison policy and stopping rule. A statement of “no measurable drop” without enough statistical power is not evidence of equivalence. Rare catastrophic failures need dedicated scenario coverage, not just aggregate averages.

Use a frozen final holdout that compilation and repair cannot access. Maintain separate calibration data for decision thresholds. Never promote a strategy solely because its own evaluator prefers it. Independent reviewers/human labels are needed where outcomes are subjective.

## Exploration policy
Default is record-only with exploration fraction 0 and automatic promotion false. Early experiments are offline. A later operator-approved epsilon or contextual policy can explore only within a safe eligible set and a separate capped exploration budget. Do not let the optimizer decide its own unrestricted risk budget. More complex adaptive exploration follows logged-policy evaluation, not precedes it.

## Promotion and rollback
Promotion requires an immutable experiment report, exact source/config/plan/evaluator identities, required security gates, quality noninferiority evidence, an approved economic/latency improvement and explicit owner signoff. Shadow, canary and staged rollout come afterward. Maintain a known-good strategy pointer and revocation path. A better new model triggers benchmark admission, not unconditional migration.

## Economics formula
Let T include all attempted tasks, S the independently accepted successes, and C total billed/estimated compute cost including compilation, failures, repair and verification. Report C/|T| and C/|S| separately, alongside coverage |S|/|T|. Amortize reusable compilation costs over a declared reuse horizon and show the sensitivity to that assumption. Do not mix unknown cost into a numeric zero.

The utility helpers in `harness/metrics.py` demonstrate arithmetic only. No real strategy is trained or promoted by this archive.

## Experience cost-field accounting
The four stage fields `compile_microusd`, `execute_microusd`, `evaluate_microusd` and `repair_microusd` are disjoint totals, including failed attempts within their respective stages. `failed_attempts_microusd` is an informational subset of those totals and MUST NOT be added again. A charge is assigned once to its lifecycle stage; repair generation and repair execution are both assigned to the repair stage. This preserves total cost without double-counting retries.
