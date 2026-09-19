# Architecture review: from an ambitious roadmap to a buildable program

## Verdict

Keep the central direction: observed world state → bounded actions → typed decisions → capability-gated effects → independent evidence → controlled learning. Replace the long feature staircase with a small executable vertical slice, a proof-of-value program, and separately gated research tracks.

The original roadmap mixes four kinds of work: safety infrastructure, cognitive execution, empirical learning, and open-ended research. They need different acceptance standards. A passing type checker cannot establish calibration; a calibrated choice cannot establish permission; a prediction cannot establish a real-world outcome; a test corpus cannot prove arbitrary program equivalence.

This review is grounded in the attached documents, not a source-code audit. The changes below are proposals. Evidence references resolve in [SOURCES.md](SOURCES.md).

## 1. Existing Axon guarantees were overstated

S1 p.23 calls the simulation world a parametric one-dimensional linear model and describes several abstractions as userland types. S1 pp.83–84 describes prototype compression work and future work for whole-program candidate complexity. That is valuable prior art, not a general predictive software model.

More urgently, p.16 explicitly says the ambient effect ceiling is interpreter-only and ignored by a native binary. Page 23 describes absent deployment gate functions as open. Page 12 narrows the FFI trust boundary, while p.90 still uses stronger invariant wording. Page 86 labels cross-VM features as implementing with remaining integration gaps. These are not reasons to discard Axon; they are reasons to establish an engine-by-engine evidence map before composing new guarantees.

**Change:** M0 records commit-pinned evidence for every relied-on property. Cortex's consequential path fails closed on a missing required gate, unsupported engine, or unavailable enforcement. Initially support interpreter orchestration plus an externally isolated tool runner; no inferred native parity. See CX-00, CX-13, CX-15.

## 2. Too many new primitives and premature language work

The original list promotes “learn,” “discover,” “abstract,” and other research concepts into prospective language primitives before their contracts are stable. That risks introducing syntax whose semantics are just a provider call.

**Change:** Start AIR as a versioned typed execution representation with a small set of operations. Keep representation search and discovery as library-level workflows. Add language syntax only after repeated, measured usage demonstrates the need. Do not make R2a a blocker for an isolated runtime experiment, but do make authoritative type-map integration a gate before new cross-engine compiler semantics. See CX-04 and CX-15.

## 3. A list of objects is not an authorization system

A model selecting a valid symbol can still choose the wrong symbol. A valid edit can introduce a malicious build script. A registered test may run arbitrary subprocesses or dependencies. An opaque ID is not intrinsically unforgeable, and a model can repeat another session's ID.

**Change:** Bind every grant to a runtime principal, session, snapshot, action kind, resource scope, expiry, budget, and server-side record. Validate generated payloads independently. Treat builds/tests as untrusted executable workloads. Include symlinks, redirects, untracked files, dependencies, and executable arguments in the threat model. Never pass model text to a shell. See CX-03/CX-13.

## 4. The action-space compiler can prune away the solution

The prior design focuses on preventing impossible actions, but can make a task unsolvable by exposing only a small, wrong neighborhood. Observation compression can omit precisely the clue that matters. Starting from a broken repo may prevent a full AST from being available.

**Change:** Preserve partial observations and explicit unknowns; include permission-bounded INSPECT, SEARCH, and EXPAND_SCOPE. Measure candidate recall separately from decision accuracy. Hierarchical action retrieval handles large repositories; retrieval must not grant new authority. New files and new tools are proposals accepted through a manifest, not impossible forever merely because they did not exist at observation time. See CX-02/CX-03.

## 5. The execution protocol is missing crash and race semantics

“Check freshness, then act” leaves a race between checking and writing. Git HEAD alone misses dirty files, toolchain changes, generated data, and other agents. Retrying after a crash can duplicate an external effect.

**Change:** Use immutable workspace snapshots, per-action read/write sets, compare-and-swap or locked commit, content hashes, idempotency keys, and durable action states. An interrupted non-idempotent operation becomes OutcomeUnknown until reconciliation, not automatically retryable. Rollback restores code/model artifacts; it does not undo an email, network request, or other external effect. See CX-03/CX-10/CX-13.

## 6. Parallel fields are not a joint probability model

S2 pp.21–22 motivates speculative branch-specific questions. W2 describes independent evaluation, meaning execution isolation. It does not establish independent errors. Shared state/model/training can produce correlated mistakes. A parallel batch can also be slower and more expensive for high cardinality or multi-token candidates.

**Change:** Define a tagged-union Action and field-dependency graph. Speculate only effect-free decision outputs under explicit resource budgets; never speculate execution. Ask conditional target questions with the operation fixed in the question. Validate the selected branch and cross-field constraints. Model/inference network calls still consume authorized effects and cost. Benchmark sequential, parallel, and shared-prefix modes rather than requiring a single forward pass. See CX-05.

## 7. A probability is not a permission, a confidence certificate, or a proof

The illustrative 0.995 threshold was arbitrary. Confidence can denote category probability, uncertainty about the category distribution, or empirical reliability; those are different quantities. Valid JSON and normalized softmax cannot settle semantic correctness. W3 supports treating calibration as a measured property rather than a formatting property.

**Change:** Separate raw scores, supplied probabilities, calibration artifact, observed validity domain, abstention, and task-risk policy. An adapter without real probabilities reports them as unavailable. Apply hard authority/risk gates first, then a workload-specific empirical routing policy. Out-of-distribution cases and expired calibration cannot gain permission from a high score. See CX-05/CX-06.

## 8. Missing baselines make expensive architecture easy to rationalize

The previous roadmap would build many components before showing that the extra components help. More capabilities do not necessarily improve end-to-end performance. S1 pp.10–11 itself reports a small experiment in which a stateless control beat a stateful system on the studied metric.

**Change:** Preserve rules-only, one strong-model agent, structured AIR without Reflex, Reflex without a learned world model, and the candidate system as ablations. Match tools, budgets, observation access, primers, and task splits. Charge retrieval, preprocessing, failed calls, speculative branches, training amortization, and verification. Measure success per cost and latency, not model latency alone. See CX-01.

## 9. “No statistically significant loss” is the wrong promotion criterion

A tiny test can fail to detect even a substantial regression. Zero observed failures is not zero failure probability. Reusing the same holdout in an indefinite optimization loop turns it into training feedback.

**Change:** Pre-register non-inferiority margins, confidence intervals, minimum sample sizes/precision, subgroup treatment, cost targets, and test families. Reserve an independently held final audit set, rotate exhausted sets, limit submissions, and record every attempted candidate. Report inconclusive when evidence is insufficient. Safety gates remain separate from quality tradeoffs. See CX-01/CX-11.

## 10. Learned world models need a precise target and limits

A graph of symbols is observed state, not learned transition dynamics. A repository digest is not a Markov state. Predicting the effect of EDIT_SYMBOL before the actual patch exists leaves the action underspecified. A learned simulator can be wrong exactly where an optimizer finds its apparent best action; W4 studies related model-bias issues in model-based learning.

**Change:** Distinguish observations, beliefs, transition predictions, simulators, and real outcomes. Start with short-horizon measurable targets such as test outcomes, diagnostic families, and affected files. Condition edit predictions on the full candidate patch digest and environment. Allow uncertainty/abstention and forbid simulations from certifying completion. Compare useful test/planning decisions against no-model controls. See CX-07.

## 11. A conditional predictor is not automatically causal

Learning P(next | state, action) does not by itself identify intervention effects when actions were selectively chosen. Counterfactual claims need stronger assumptions than forecasting.

**Change:** Explicitly label observational, intervention-backed, and assumed structural claims. Start causal work with resettable sandbox interventions and matched controls; record which variables were held fixed. Information-gain scores use the current hypothesis model and can themselves be wrong. Evaluate downstream diagnosis, not just calculated entropy reductions. See CX-08.

## 12. MDL should not mean “simplest answer that fits yesterday”

A zero-residual constraint makes sense for some deterministic examples, not all noisy observations. Small training descriptions can overfit, erase rare cases, or omit causal state. The source already identifies the current complexity measure as a prototype requiring further calibration (S1 pp.83–84).

**Change:** Use preregistered fit tolerances appropriate to the domain, locked holdouts, and explicit residual/exception cost. Keep AST complexity labeled as a proxy, not a universal measure of understanding. Require improved transfer or downstream decisions for a new abstraction. See CX-07/CX-09.

## 13. Learning needs data eligibility, not automatic ingestion of every trace

The source warns that host journals can contain file contents, HTTP bodies, input, and environment values (S1 p.15). A detailed replay log is useful operational evidence and potentially dangerous training material. Teacher answers and agent self-assessments are not ground truth. Outcomes may arrive late or never.

**Change:** Separate access-controlled raw journals, redacted review traces, eligible learning records, and sealed evaluations. Record retention, provenance, consent/authority, label quality, delayed/censored outcomes, and contamination lineage. Permit audit evidence without permission to train on it. See CX-10.

## 14. Error attribution is uncertain and can be multi-causal

The proposed failure enum is useful, but demanding a single correct diagnosis before learning can freeze progress or produce fabricated explanations.

**Change:** Store a distribution or set of candidate causes with evidence and Unknown as a legitimate result. Confirm attribution through replay, component substitution, and controlled experiments. Update one component at a time by default; coordinated changes need interaction tests. See CX-10/CX-12.

## 15. The meta-loop must not grade its own exam

The preceding answer allowed changing evaluation suites and promotion thresholds while also prohibiting weakening the gate. Without a trust/approval split, those statements conflict. A learner could make every candidate pass by changing the metric or holdout.

**Change:** Separate learning workers from an independently controlled admission authority. Workers may propose new curricula, metrics, gates, proof tactics, or compiler changes. The authority versions and approves policy; incumbents and challengers are rerun under both old and new policies before a switch. Learning a prover strategy is permitted; rewriting the trusted proof checker is a TCB change. See CX-11/CX-12/CX-15.

## 16. Crystallization is optional specialization, not a law of intelligence

Not every useful reasoning task has a stable deterministic rule. An empirical student is not behaviorally equivalent to its teacher. Repeated task success may be highly context-dependent. Waiting for “millions of decisions” is also an arbitrary blocker for useful small-model experiments.

**Change:** Support several paths: prompt optimization, calibrated routing, retrieval, distilled adapters, verified tools, guarded rules, and restricted compiler rewrites. Every specialization records an applicability predicate and fallback/deoptimization path. Train small prototypes once a useful labeled pilot exists; expand only when learning curves justify it. See CX-11/CX-14.

## 17. OS scope must be staged, not forgotten or allowed to dominate

S1 contains both the early userland-only direction and an explicit bare-metal reversal. The later roadmap notes unresolved confinement and kernel/VM integration (pp.62–63, 86). We cannot silently declare the OS done or cancel the user's kernel goal.

**Change:** Deliver a hosted Cortex runtime first; then confined worker execution and operational controls; then portability; finally a separately approved bare-metal research branch. For Cortex, hard process authority, cancellation, recovery, and resource bounds precede desktop polish or kernel replacement. See CX-13.

## 18. Research milestones need evidence-based names

“Fluid intelligence starts here” or “fully self-improving” is not a measurable completion gate. A system can invent plausible abstraction names without improving any task.

**Change:** Require held-out novel problem families, few-shot learning curves, representation ablations, transfer tests, and bounded-compute comparisons. Report demonstrated capabilities and limits, not a universal intelligence claim. See CX-09/CX-12.

## What remains unchanged

The learning hierarchy, distinct decision/generation roles, dynamic observed targets, interpreter-first semantics, independent verification, and lowering expensive cognition into reusable machinery remain central. The package changes sequencing, precision, and evidence—not the long-term aim.

## Recommended first deliverable

A sandboxed repair of one small, intentionally broken Axon example: observe a partial program; choose an inspect or edit target; generate one bounded patch; run approved checks/parity when supported; produce an independently validated patch artifact; replay without live effects. No auto-merge, production deployment, kernel modification, or model self-training is required to demonstrate this first loop.

## 19. The System-One ecosystem changes the experimental order

The user-supplied Sept-2026 ecosystem map adds a useful implementation lesson: the Jev/System-One **programming shape** can be separated from the underlying model. Official adapter patterns, OpenJev-style logit/sequence scorers, MLX/shared-prefix implementations, option-conditioned learned heads, browser agents and calibration projects suggest a better research sequence than “build a custom Reflex model later.”

**Change:** freeze one Axon-owned backend-neutral Reflex ABI, then compare generative constrained output, direct option logits, sequence probability and learned option-conditioned heads on the same dynamic-candidate corpus. Add destructive state/candidate controls, grouped splits, shared-prefix/prefill measurements and typed probability provenance. Calibration and routing use protected real outcomes, not merely normalized model scores. Custom architecture work becomes a response to measured quality/serving bottlenecks rather than a roadmap assumption. See the three `build/REFLEX_*` documents and updated CX-05/CX-06.

This does not validate launch-week repo benchmarks or reproduce TypeSafe's private training stack. The ecosystem is used to improve experiment design, not as proof of performance.

## v0.4 review — MiCode changes the data/experimentation architecture, not the core execution path

The MiCode support plan does **not** justify replacing Cortex's M0–M7 core. It adds a valuable external experience plane and makes the self-optimizing-OS objective more explicit. Three additions matter: a strict MiCode↔Axon artifact bridge (CX-16), provenance-rich repository/history knowledge ingestion (CX-17), and a staged capability/native crystallization path (CX-18).

The key boundary is authority. MiCode can produce rich episodes and run challenger experiments, but its local grants, risk tiers and host properties cannot become Axon authority. Likewise, Axon can export policies/verifier profiles to MiCode without bypassing MiCode's gate. This keeps co-development scientifically useful without creating a distributed TCB by accident.

The build sequence therefore changes modestly: freeze the bridge schema early; ingest one episode once available; defer external-knowledge claims until representation/abstraction and data-governance machinery exist; demonstrate Pattern→Skill/Tool before optional compiler/runtime promotion. Approximately the existing M0–M7 execution/research program remains intact.

## v0.5 finding — the intent layer was under-specified

The original Axon roadmap had the correct product direction—structured prose compiled to typed Axon artifacts and explicitly reviewed—but the Cortex package mostly began from an already supplied `goal/task`. That skips a load-bearing boundary: natural language mixes objectives, constraints, preferences, authority requests and success evidence, and those cannot safely become implementation instructions in one opaque model translation. CX-19 inserts a typed Intent IR and semantic approval layer. This is not a rewrite of M0–M9; it makes the contract above AIR explicit and makes self-optimization auditable as `ImprovementIntent` rather than unrestricted mutation.

## v0.6 review conclusion — Reflex implementation, not Cortex redesign

The new Jev behavioral analysis does not overturn the Cortex architecture. It improves the implementation hypothesis for Reflex: shared state becomes a first-class measurable runtime contract; candidate order/composition become formal inference context; question dependency scheduling becomes explicit; listwise candidate interaction is prioritized in research; and distributions/proper scoring are treated as primary. Exact Jev backbone/training reconstruction remains unsupported and is excluded from the critical path.

## v0.7 review — Kev changes the bottleneck, not Cortex

Kev demonstrates that a Jev-style shared-state/isolated-question/pointer-readout model can be implemented and trained cheaply with a small pretrained backbone. The important negative result is transfer: familiar-source quality can approach the hosted reference while out-of-source quality remains far lower. Therefore the build should spend less effort speculating about an exotic undisclosed architecture and more effort on coding-world representation, verified MiCode/Axon experience, repository-level split discipline, transfer evaluation, OOD routing and curriculum.

The package is updated accordingly: a Kev-style model is an early reference arm, not a promoted default; locked tests and transfer partitions become explicit; permutation/absence augmentation are first-class experiments; and one canonical decision encoding owns train/eval/serve/replay semantics.
