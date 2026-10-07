# v0.19 adversarial review — RLCD/Jev-derived design

## Threat 1 — reverse-engineering speculation becomes architecture fact

**Attack:** implement packed tree attention because a blog says that is how Jev works, then make ACE depend on it.  
**Control:** CX-14 treats it as an optional reference arm; ACE exposes semantics/features, not topology. Separate-execution equivalence and contamination probes gate the optimization arm only.

## Threat 2 — softmax laundering

**Attack:** label the selected option's softmax score `confidence=0.97` and use it as a 97% correctness estimate.  
**Control:** CX-05/CX-06/CX-37 separate raw decision distribution from empirical correctness. A correctness estimate requires a matching immutable calibration artifact/key and evidence.

## Threat 3 — calibration survives a semantic change

**Attack:** reuse a calibrator after candidate ordering, candidate construction, working-set projection, model checkpoint or question wording changes.  
**Control:** those fields are calibration-key inputs; mismatch makes correctness unavailable unless an explicit transfer artifact is validated.

## Threat 4 — ECE gaming

**Attack:** optimize one bucketed ECE number while worsening sharpness/NLL or hiding sparse bins.  
**Control:** Brier + NLL are primary proper scores; reliability/support/uncertainty and selective-risk views accompany them. ECE cannot close the gate alone.

## Threat 5 — IIA overconstraint

**Attack:** reject a superior listwise model because adding an alternative changes relative probabilities.  
**Control:** IIA is a falsification/diagnostic expectation for Luce-family reference arms only. Candidate-set sensitivity remains measured and part of calibration scope.

## Threat 6 — packed sibling leakage

**Attack:** candidate/question branch A influences B through attention, positions, cache reuse or implementation bugs.  
**Control:** packed-vs-separate equivalence, sibling-token perturbations, branch-order randomization and position-reset probes.

## Threat 7 — fan-out races become semantic bugs

**Attack:** fuse an AnswerDependent question with its prerequisite and obtain an answer against nonexistent state.  
**Control:** only dependency-compatible questions may FANOUT; per-question identity/failure is retained; answer-dependent nodes remain sequenced.

## Threat 8 — shortlist makes the chooser look perfect

**Attack:** stage 1 drops the correct candidate, then evaluate only the final chooser on survivors.  
**Control:** report shortlist recall/omission separately, bind final calibration to shortlist policy, and preserve full-set→shortlist lineage.

## Threat 9 — independent score becomes joint probability

**Attack:** renormalize arbitrary retrieval/independent scores over a shortlist and call them calibrated probabilities.  
**Control:** prohibited by CX-05/CX-26; only a model/calibrator validated for the declared event may produce the claim.

## Threat 10 — confidence bypasses authority

**Attack:** `P(delete)=0.999` executes destructive work or closes a task.  
**Control:** external risk policy and normal capability/PermissionGate/AcceptanceContract remain mandatory. High-risk policies may require independent verification even at high probability.

## Threat 11 — feedback becomes self-ratifying online learning

**Attack:** the model selects actions, labels itself from ambiguous outcomes and immediately deploys the new calibrator/model.  
**Control:** outcome joins create shadow candidates only; protected evaluation and CX-11 admission own activation.

## Threat 12 — benchmark speed substitutes for system value

**Attack:** optimize model-call latency while increasing context preparation, verifier burden, retries, shortlist misses or data egress.  
**Control:** outer-loop evidence records end-to-end latency/cost/quality/verification/effect outcomes and preserves the incumbent as a valid winner.

## Bottom line

The research is useful when converted into falsifiable contracts. It is dangerous if used as justification for a Jev clone, a magic confidence scalar or a new authority path.
