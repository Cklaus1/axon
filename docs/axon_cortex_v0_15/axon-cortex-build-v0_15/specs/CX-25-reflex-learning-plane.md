---
id: CX-25
title: "Reflex Learning Plane: separated learner, sampler and guarded model publication"
status: Draft
authority: Proposed
depends_on: ["CX-10", "CX-11", "CX-20", "CX-21", "CX-22"]
first_stage: M7
implementation_evidence: []
---

# CX-25 — Reflex Learning Plane

## 1. Purpose

Parallax-like asynchronous RL systems show the engineering value of separating a learner from the live inference sampler and moving versioned weights between them. Cortex needs that systems separation, but it must preserve independent admission: a learner may continuously produce candidates; it may not mutate the active protected model merely because a training step completed.

CX-25 defines a **Learning Plane** for candidate model/adaptor updates and a separate **Serving/Sampler Plane** for active inference.

## 2. Core invariant

```text
experience → learner → candidate artifact → shadow sampler/evaluation
                                      ↓
                               CX-11 admission
                                      ↓
                                active sampler
```

No training process has credentials or API authority to replace the active artifact directly.

## 3. Components

- **Experience queue** — eligible, versioned records only; preserves corpus role and behavior-policy lineage.
- **Learner** — trains/evaluates candidate weights/adapters under pinned code/data/config.
- **Candidate registry** — immutable candidate IDs/digests, parent artifact, training lineage and metrics.
- **Shadow sampler** — serves candidate inference against replay/live-shadow traffic without taking effectful actions.
- **Admission bridge** — exports immutable CX-20/CX-21 evidence to CX-11.
- **Active sampler** — only independently admitted artifacts.

Training and serving may use different runtimes/frameworks provided the artifact conversion has conformance evidence.

## 4. Online and asynchronous learning

Sampling and learning may run concurrently. Mixed-policy datasets are allowed only when every decision records the exact behavior-policy/model version and selection probability when known. Historical data must not be treated as on-policy by omission.

Candidate weights may be published frequently to the candidate registry, but live protected serving changes only at admission boundaries.

## 5. Weight transport

Weight/adaptor transport is content-addressed and versioned. It records source learner framework, destination runtime, tensor/adapter format, checksums, conversion code and compatibility manifest. Partial transfer, stale version or incompatible runtime fails closed.

Direct GPU-to-GPU streaming is an optional optimization; correctness/reproducibility of the artifact identity is normative.

## 6. Reward / objective sources

Learning objectives may use verified task outcomes, proper scoring rules, contrastive/deletion labels, teacher distributions, world-model targets or curriculum rewards. The objective source is explicit. Model-authored reward is not equivalent to protected verifier evidence.

For sequential coding trajectories, preserve episode/step identity, intermediate evidence and final verified outcome so credit-assignment experiments remain possible.

## 7. Shadow and canary operation

Candidate inference initially runs in replay or shadow mode. A shadow candidate may recommend actions but cannot authorize or execute them. Promotion evidence includes selective quality/coverage, transfer, calibration, latency/compute, drift, and whole-system effect under CX-21 where applicable.

Canary activation, if later supported, is itself an admitted deployment artifact with rollback triggers; it is not an escape hatch around CX-11.

## 8. Acceptance gates

**G25-candidate-only:** learner output is registered as a non-active candidate; attempts by the learner/training worker to mutate the active sampler or admission configuration are denied and audited.

**G25-shadow:** a candidate can process replay/live-shadow episodes and produce comparable decisions while being structurally unable to execute actions or satisfy completion evidence.

**G25-version:** every sampled decision records exact active/candidate model, behavior-policy and encoding versions; mixed-policy training with missing lineage is rejected.

**G25-rollback:** an admitted model can be reverted to the previous-good immutable artifact without losing episode/evidence lineage; candidate/training state does not rewrite historical outcomes.

**G25-transport:** corrupt, partial or semantically incompatible weight/adapter transport is detected before serving; converted artifacts have reproducible checksums and conformance evidence.

**G25-mixed-policy:** an asynchronous/mixed-policy experiment reports policy lag and uses an objective/evaluation method compatible with that lag; it may not present mixed-policy data as clean on-policy evidence.

## 9. First build slice

Do not start with online RL. Train one tiny Reflex candidate from an immutable CX-20 suite in an isolated learner process, publish it to a content-addressed candidate registry, load it into a shadow sampler, replay fixed decisions, produce an evidence bundle, and demonstrate that only a mocked independent admission step can change the active sampler pointer.


## v0.13 self-application challengers
The learning plane may train candidate successors for active internal cognitive components, but publication remains candidate-only until CX-11/CX-29 independent admission. Active sampler/host authority is not transferred to the learner.
