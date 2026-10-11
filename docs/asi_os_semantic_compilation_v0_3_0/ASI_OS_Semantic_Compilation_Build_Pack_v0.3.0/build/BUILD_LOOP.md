# Build Loop and Exact-Revision Gate

## Repeat per bounded change
Read the active milestone and its requirement/test IDs. Inspect the target code and record current HEAD. Describe the expected behavior and likely adversarial failure. Add a failing regression or capture fixture. Implement the smallest change using existing abstractions. Run the narrow test, contract checks and affected integrations. Perform an adversarial review focused on authority, effects, caching, evidence and budget boundaries. Fix findings and rerun.

Run the repository's formatter, linter, full relevant tests and dependency/lockfile gate. Freeze the final source revision. Build and test that exact revision; record artifact hashes and commands. No edits after the gate without a new gate. A documentation-only change to a protected rubric or configuration is still behavior-changing and invalidates relevant evidence.

## Required output record
```text
Milestone / requirement IDs:
Source revision and dirty-tree status:
Changed files:
Commands + exit codes:
Exact artifact/config/registry/evaluation digests:
Positive assertions verified:
Negative/adversarial assertions verified:
Tests not run and why:
External effects or data exposure:
Remaining blockers:
Decision: merge / hold / reject
```

## Stop conditions
Stop new execution when policy is unavailable, external effect state is ambiguous, a protected criterion was weakened, budget is unbounded, the test target cannot be proven to have executed, or a dependency revision is unknown in a context requiring a pinned version. Preserve evidence and report the exact blocker.

## Scope discipline
Do not combine compiler strategy changes, judge rubric changes and holdout changes into one unidentifiable improvement claim. Do not fork the existing config, task-ID, telemetry or policy subsystem merely to align names with this document. If an architecture conflict appears, add/update an ADR and get the relevant owner decision.
