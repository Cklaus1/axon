# Lightweight Reflex experiment and promotion loop — v0.20

Read [the profile](../schemas/LIGHTWEIGHT_REFLEX_PROFILE.md), CX-06/CX-10/CX-20/CX-22/CX-25 and existing admission/runtime contracts. B214–B216 form a bounded shadow experiment; B217–B219 qualify optional deployment and joint use. Models do not need to succeed for the experiment to close honestly.

## Preregister before collecting results

Choose one stable low-risk diagnostic/inspection category. Record task/label meaning, current live baseline, exact input projection, eligible groups, held-out temporal/repository domain, measured event, privacy/teacher permissions, error/coverage/latency floors, OOD policy, protected-test submission budget, training candidates and stopping criteria. Use the template in `fixtures/schema_reflex/experiment-template.json` as an unfilled draft, never as evaluation evidence.

## Inner loop — one immutable candidate

Eligibility and label adjudication → grouped train/selection/calibration/acceptance/test partition → baseline/feature fit on allowed data → selected head → separate calibration → independent advisory-threshold selection → frozen final evaluation → signed independent decision or rejection. A resumed teacher job preserves task/model/endpoint/principal/partition/budget identity and committed records. No automatic data upload, dependency installation or encoder download is authorized by this guide.

Persist both all-row and independent-representative metrics, per-class support, Brier/NLL, coverage/accuracy curves, input lengths, OOD failures, runtime/batching parity and full cascade cost. A null cutoff, unsupported domain, insufficient critical-class support or no measured economic benefit leaves the incumbent selected. Do not weaken gates to force a classifier into production.

## Outer loop — shadow and deployment

Start with passive identical-input shadow decisions. Record disagreements without executing candidate tools. A changed context/routing/action sequence is an authorized experiment treatment, not passive observation. After frozen evaluation, submit candidate identity/evidence to CX-11; the learner cannot sign admission. Qualify the exact runtime/precision/batching profile and shared-encoder lifetimes separately. Test corruption, revocation between selection/dispatch, cancellation, unknown outcomes and fallback into the next turn.

The candidate may be a deterministic rule, TF-IDF model or a neural `.np`; use the existing corresponding artifact/registry lifecycle. Do not call TF-IDF inherently neural or force deterministic code into CX-36. Runtime lower bounds still require local permission and independent action/task checks.

## Meta loop — keep only useful specialization

Use verified outcomes, rejected cases, calibration drift and eligible-volume economics to propose recalibration, narrower applicability, another learner or retirement. Retraining emits a new candidate; no live weight/calibrator mutation. Extra encoders/MLPs/context filters/agent routes remain optional and use separate preregistration and evaluation budgets.

## Parallel build policy

After B210 freezes the seam, schema implementation and data/lab preparation can proceed in disjoint worktrees/scopes with one contract integrator. Serialize edits to shared schemas, enum meanings, evaluator policy, gate manifests, release locks and promotion code. Run union tests after integration. Limit child workers and root resource budgets; no worker can change the evaluation goal to pass its own task.
