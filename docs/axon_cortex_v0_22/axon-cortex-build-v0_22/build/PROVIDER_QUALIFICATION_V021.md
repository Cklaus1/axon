# Provider qualification recipes — v0.21

These are build/run plans, not executed research results. The [source registry](../integration/RESEARCH_SOURCES.json) records what was actually inspected. Never substitute a paper's metric for an Axon gate result. Rejected/inconclusive outcomes are valid.

## Common preregistration

Record input source commit/archive, provider code commit, model/weight digest, tokenizer/pooling/precision, runtime/container/device, task/repository split, evaluator and hidden-check revisions, candidate construction, budget, price revision, seeds, trial IDs, stopping/multiplicity method and acceptance/rollback policy. Freeze these before seeing outcomes. A provider whose API cannot expose a stable model revision has an explicit reproducibility limitation; qualify a bounded observation window, detect drift, and do not claim bitwise replay from a fresh call.

Use disjoint fitting, selection, calibration/threshold and final-test evidence. Require authority/privacy checks before transmitting data. Keep a deterministic/no-model control, and compare end-to-end tasks with all costs. Minimum sample sizes and margins are workload-specific preregistration inputs, not invented universal values. No unrestricted benchmark downloads or model pulls occur during package validation.

| Family | First bounded experiment | Controls | Required falsifiers / outcome |
|---|---|---|---|
| RRSI | Single allowed harness component; frozen model; explicit proposal history | Fixed incumbent and unregularized proposer at equal search budget | Leakage, grader edits, repeated-holdout selection, tiny noise, pruning safety; independent generalization disposition |
| CLM | Small immutable tool/skill candidate set from existing registry | Exact deterministic/simple embedding and qualified Jev where available | All-bad and singleton sets, permutations/ties, candidate duplication, long-input truncation, cold/warm cache, epoch/principal mismatch |
| Jev/RLCD | One binary/choice family already in the existing Reflex contract | Rules and incumbent provider | Unsupported scope, malformed/partial probabilities, transport failure, OOD, raw probability versus correctness calibration |
| CheckEval | Frozen code-review checklist with deterministic requirements separated | Actual tests and qualified human labels for subjective criteria | Missing criteria, candidate-controlled N/A, self-reported checks, shared-judge correlation; final task truth preserved |
| TinyRouter/TRINITY/Semantic Router | Eligible model/role pool for one task family | Static, cheapest eligible and best single at equal whole-task budget | New/unknown models, session switches/cache loss, policy leakage, recursion, revoked providers; domain-stratified results |
| CliffCompaction | Shadow original-history projection on long repair traces | No compaction where feasible and current working-set policy | Re-compaction, rewritten prefixes, missing originals, pins over budget, tool-pair/image loss; critical-context recall and final success |
| TypeLLM/pijev | Fixed bounded categorical permutation schedule | Single fixed presentation and a declared randomized presentation policy | Label alignment, incomplete calls, finite-sample invariance limits, question grouping, ordinal meaning; full wrapper cost/calibration |
| RLM-Cascade | One cheap draft plus one bounded verifier, isolated patch proposal | Direct incumbent at matched total budget | Repaired digest mismatch, tool effects, cancellation, unknown cost, over-budget fallback; actual final checks and total cost |

## Initial CLM and Jev execution constraints

The inspected CLM repository is pinned in provenance, but the model weights and local deployment are not. Resolve and hash them before executing B227. Do not download arbitrary `.pt` checkpoints or enable remote code based on an unverified README. Review artifact trust and loader policy under existing neural/ACE contracts. Scope endpoint authentication, state retention and cache namespaces independently of the model's mathematical scoring function.

Do not copy earlier conversational model names, prices or threshold examples into configuration. They were illustrations, not a verified deployment catalog. Determine actual permitted providers from the live registry. An API that reports candidate-conditional probabilities still needs independent calibration for any correctness-driven automatic acceptance.

## Release disposition

A recipe can end `NOT_RUN`, `REJECTED`, `INCONCLUSIVE`, `QUALIFIED_SHADOW`, or `ADMITTED_BOUNDED_SCOPE`. Only the existing independent authority may produce the latter. Record missing artifacts, inaccessible sources and unsupported capabilities explicitly. The document pack remains valid even when an attractive research result does not reproduce on Axon's workload.
