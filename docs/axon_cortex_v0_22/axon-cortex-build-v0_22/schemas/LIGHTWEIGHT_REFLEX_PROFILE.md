# Lightweight fixed-task Reflex distillation — proposed v0.20 lab profile

**Owners:** CX-10 (eligible data), CX-20 (independent Lab), CX-22 (specialization), CX-25 (learning lifecycle), CX-06 (calibration), CX-23/CX-36 (learned artifact), CX-34/CX-37 (qualified execution). **Status:** Draft; no model trained or product gate executed by this package. This is a training/deployment qualification profile referenced through existing manifests/evidence, not a new wire format.

## 1. Candidate discovery and eligibility

Start with one low-risk fixed-domain diagnostic or inspection-category function. Register its exact input projection, question/criteria, labels, measured outcome and baseline. Discover candidates from recurring eligible decisions using frequency, label/domain stability, evidence quality, potential coverage and end-to-end cost, not raw repetition or model confidence alone. Exclude permission decisions, grants, protected verifier definitions, completion certification, destructive commands and arbitrary payload synthesis from learned replacement.

A fixed-question head is eligible only for its frozen task meaning and label map. A new tool catalog, changed label meaning, different prompt/instructions or different input projection requires an explicit compatibility determination; same label count is not sufficient. Dynamic tools/spans/entities need a separately qualified candidate-conditioned backend or retraining. Reordering is refused unless an explicit checked identity-preserving map has been qualified; do not reinterpret weight columns in place. Training success is never admission.

## 2. Data and teacher jobs

Use existing CX-10 records with episode/task/repository lineage, canonical input identity, selected action/candidates, teacher version and raw score origin, observed tool/check outcomes, label provenance, authorization and retention references. Distinguish teacher distributions from hard reviewed labels and independently verified outcomes. Do not treat user permission/approval, a zero process exit, tool-level Success, a completion declaration or an ambiguous outcome as a factual training label automatically.

Reviewed human corrections can override an eligible training target only through authenticated provenance and conflict adjudication; preserve both original and correction, applicable task version and reason. Machine-generated labels never acquire human provenance. Refuse conflicting exact duplicates unless adjudication explicitly resolves the conflict. Teacher probabilities are training targets where supported, not reliability weights or ground-truth probabilities.

A teacher-label job has stable task/teacher-resolved-version/endpoint identity, exact effective input identity, tenant/project/principal, eligible corpus and partition role, data-use/retention policy, grant reference, budget and evaluator epoch. Authenticate providers through the existing credential owner. Never store credentials in cache keys, records, logs or portable bundles. Cache reuse is restricted to this full scope; train, calibration, acceptance and protected-test jobs cannot share label visibility merely because the same input hashes match.

Use exact unique string IDs for joins, not row order. Record duplicates, missing and extra IDs as errors. Durable append records become committed only after complete validated writes; a torn final record is not completed work. Stable job/input idempotency identity survives restart, revision changes require a new job, stale/foreign locks refuse unsafe takeover, and ambiguous paid requests reconcile before retries. Respect cancellation, rate limits, provider data policy and root cost budget. Provider-specific idempotency is not assumed. The offline reference only models cache identity and completed-record parsing; it performs no request or storage locking.

## 3. Independent data roles

Create eligible grouped partitions: `train`, `selection`, `calibration`, `acceptance`, `test`. Related episodes, tasks, duplicated sources, shared-customer/problem variants and copied repositories belong to the appropriate connected group before splitting. Use deduplication plus group/component and temporal/repository holdouts as preregistered. Grouping is a leakage control, not proof of independent representative observations.

Fit TF-IDF vocabulary/statistics on training only. Frozen encoder extraction may process allowed feature inputs but never expose protected labels or mix evaluator caches. Fit head/candidate/epoch on training and selection; fit temperature on calibration; select the advisory cutoff on independent acceptance data; freeze all choices before evaluating the protected final test. Do not repeatedly use final-test reports to choose new heads or thresholds: retain evaluator submission/feedback budgets and require fresh protected evidence after adaptive reuse.

The Jimothy-inspired approximate development split is 50% selection / 25% calibration / 25% acceptance after reserving development data; these are reference ratios, not a guarantee every class has sufficient support. Rare classes that cannot support independent partitions remain unqualified or route to fallback. A benchmark repeatedly inspected during research is reported as development evidence, not a fresh deployment test.

## 4. Baseline and training arms

Compare the incumbent and a deterministic parser/rule where applicable, then training-fitted TF-IDF plus a linear head and a frozen permitted encoder plus a learned linear head. Jimothy/MiniLM are reference implementations, not mandatory dependencies or proved suitable coding models. Verify licensing, permitted teacher/model use and data disclosure before imports or downloads.

For a reproducible Jimothy-like reference arm: evaluate L2 strengths `0.001, 0.0001, 0.00001, 0.000001, 0`; cap epochs (default reference 200); stop after 30 epochs without selection-loss improvement; declare optimizer, seed, label loss and preprocessing. Select using the preregistered selection loss, then freeze the head. Temperature scaling may search [0.05, 20] on independent calibration labels with at least 30 eligible examples. These bounds/minima are experiment settings, not sufficiency guarantees. Record every candidate/epoch and any deliberate deviation rather than quietly adopting a more favorable search.

Compare Brier/NLL, reliability/coverage curves, per-class and critical-slice performance, input-length effects and OOD/absence failures. Scores for ordinal tasks require a separate target: exact winning level and expected-score error are not interchangeable. Fixed-head agreement and complete-invocation/task-success evaluation are separately reported.

## 5. Conservative advisory threshold selection

Reference grid: `0, 0.5, 0.6, 0.7, 0.8, 0.85, 0.9, 0.925, 0.95, 0.975, 0.99, 0.995, 1`.

Predeclare the target event, target accuracy, confidence level, minimum accepted independent representatives, coverage/latency objective, subgroup obligations and search/submission budget. For each threshold count correct accepted independent representatives and compute a one-sided exact binomial lower bound. Allocate the family error budget across all searched thresholds and any additional models/subgroups/comparisons used for the release claim; the 13-way Bonferroni reference only covers one fixed grid for one frozen model/event. Do not imply it covers an unbounded adaptive model search.

Select the highest coverage qualifying cutoff, breaking ties toward the lower threshold, only when the lower bound meets the target and accepted independent support is at least the preregistered minimum (reference 30). Thirty successes alone generally do not establish a high target after correction. If no candidate qualifies, emit `threshold:null` with `insufficient_data` or `target_not_met`. No qualified threshold means no threshold-based automatic use, not a zero threshold. Acceptance estimates do not protect against distribution shift.

For grouped evidence choose one deterministic representative per group without consulting correctness/probability; preserve its selection rule. Report representative-level evidence separately from all-row metrics. Per-class support and critical-class error limits can veto aggregate success. Product acceptance cannot be reduced to overall accuracy or an attractive pooled confidence number.

The recommendation is advisory. SDK/reflex predictions may still return without a cutoff; CX-06/local policy owns whether a calibrated result can route a task. Readiness, applicability, OOD, permission and independent verification remain separate checks.

## 6. Qualified deployment identity and shared assets

Bind task meaning, ordered label identities/typed label mapping, input projection, preprocessing, encoder revision/asset hashes, tokenizer, learned head weights, calibration/evaluation references, precision/quantization, runtime version/device class and batch-shape/execution profile. This is detached applicability/evidence metadata referenced via existing CX-23/CX-34/CX-37 contracts. Preserve unchanged CX-36 `.nps`/`.np` schemas and ACE v1 wire unions.

A calibration result applies only to the qualified identity. New precision, runtime, tokenizer, encoder, projection, label map or unqualified batching mode withdraws compatibility until revalidated. Record single-input and batch numerical parity and threshold-crossing disagreements; do not dismiss decision changes as harmless floating-point noise. Reject overlong/truncated input unless an explicitly qualified projection exists. The reference MiniLM 256-wordpiece limit is not a generic Axon limit.

Multiple exact-compatible heads may share immutable encoder assets and authorized feature computation. Cache keys bind content/projection/encoder/tokenizer/precision/runtime and tenant/project/principal/evaluator role; immutable weight sharing is not permission to share private embeddings, mutable adapters or KV state. Reference-counted lifetimes, cancellation, eviction and memory budgets must prevent one head's disposal from invalidating another's live work. No hidden cross-runtime reuse or unadapted base-model fallback.

## 7. OOD, shadow, promotion and de-specialization

The applicability guard combines exact identity with declared domain/input-length/state-availability checks and qualified novelty/OOD signals. Missing or unsupported OOD evidence cannot be replaced with high softmax confidence. OOD estimates are imperfect; keep rejected/ambiguous classes in eval and retain an authorized incumbent fallback. Do not train the live artifact in place.

Run the candidate in shadow mode first. It must not execute extra tool actions or gain new disclosure rights. If candidate routing changes context or later inputs, classify it as a separate treatment rather than a passive identical-input comparison. Evaluate rejected as well as accepted samples to avoid selection bias. Outcomes may trigger a new candidate/recalibration proposal, not self-publication.

Independent CX-11 admission binds immutable model, qualification, scope and current policy. Recheck revocation at dispatch, pin the in-flight generation, and switch only at a declared boundary. On drift, corruption, revoked data/consent or quality regression: stop new uses, preserve evidence, select a qualified previous-good/authorized incumbent, reconcile unknown in-flight effects before retry, and narrow/retrain/retire. Revoking a cache or local file is not evidence that a remote provider deleted retained data.

## 8. Economics and staged expansion

Measure the full cascade: input projection and tokenization; cold/warm model load; feature/head latency; validation and permission checks; actual tool execution and verification; fallback and error recovery; memory/bandwidth; training, labeling, storage and maintenance amortized over realistic eligible volume. One existing frontier call may already make several decisions; do not multiply apparent savings by fictitious separate calls. Report p50/p95, useful eligible coverage and measured verified utility with uncertainty. Lower latency alone cannot compensate for a protected quality/authority failure.

The experiment can conclude `rejected` or `inconclusive` with a reproducible reason; no production activation is required for experiment completion. Richer MLP/code/trace/world-model heads, agent/tool routing and context-relevance filtering are optional later hypotheses. Working-set experiments preserve mandatory pins and restoration/fallback; no elimination-of-compaction promise follows. Bounded action IDs do not eliminate arguments, dispatch, tool calls or verification.

Primary observations are recorded in [source notes](../review/JIMOTHY_WEBMCP_SOURCES.json). The implementation/policy requirements above are proposed Axon adaptations, not claims that upstream already supplies them.
