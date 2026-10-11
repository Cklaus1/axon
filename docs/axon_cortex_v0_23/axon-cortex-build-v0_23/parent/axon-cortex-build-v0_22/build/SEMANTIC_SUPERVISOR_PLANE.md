# Semantic Supervisor Plane build guide

This guide implements CX-27 without granting a learned supervisor authority.

## Build order

1. Freeze `SupervisorObservation`, `SupervisorAssessment` and `SupervisorDecisionRecord` schemas.
2. Add shadow-only assessment over resettable coding episodes.
3. Version semantic definitions independently from worker prompts/models.
4. Implement deterministic intervention policy with hysteresis and attempt bounds.
5. Add `STEER` and `REQUEST_VERIFY` before any stop/retry behavior.
6. Benchmark detection quality and intervention utility against protected outcomes.
7. Route any reusable supervisor policy through CX-11 admission.

## Non-negotiable boundaries

- supervisor scores never create tool authority;
- worker self-report is evidence only;
- `PROPOSE_FINISH` is not `VerifiedComplete`;
- hidden verifier internals are never injected into the supervisor request;
- missing supervisor service cannot fail toward more authority;
- live steering remains feature-gated until shadow evidence exists.

## First experiment

Construct fixtures for: meaningful progress, repeated same-strategy failure, off-track edits, AGENTS/rule drift, missing tests, premature completion and genuine completion. Replay the same worker state across alternative semantic definitions/backends and compare against protected labels.
