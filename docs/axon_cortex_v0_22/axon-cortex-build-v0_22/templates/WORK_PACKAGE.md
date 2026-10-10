# Work package template

Status: Draft / Not started. Replace placeholders before execution; do not interpret them as defaults.

## Identity and scope

Record work-package ID; linked CX/R requirement IDs; repository commit; upstream slices; owner/integrator; permitted paths; protected paths; and one-sentence behavior change.

## Evidence and decisive fork

Record prior art inspected, exact current failure/limitation, the competing designs, chosen design, rejected alternative and why. Identify what is documented versus reproduced. Attach the red/negative fixture or specify how to create it safely.

## Contract

Define inputs/outputs, types, versions, authority, effects, resource costs, error/refusal states, concurrency/recovery, engine/host support, migration and rollback. List non-goals explicitly.

## Implementation steps

Define the smallest sequenced edits, shared-interface coordination and checks after each edit. No broad opportunistic refactor. New parser/type/codegen work requires the compiler gate plan; new host effects require actual confinement tests.

## Acceptance

List required gate IDs, concrete test fixtures, exact command locations after implemented, environment and expected outputs/statuses. Distinguish mocked conformance from real enforcement. Define quality/calibration/coverage/resource margins where relevant before seeing results.

## Budget and stop rules

Set maximum wall time, steps, tool/model calls, retries, compute/cost/storage, candidate submissions and no-progress threshold. State immediate safety/secret/recovery stop conditions and escalation owner.

## Evidence after execution

Record actual commands, statuses, logs/reports, candidate and policy digests, baseline comparisons, failed attempts and unproven assumptions. No gate can be marked PASS from its planned name. Link the review and admission decision separately from the author's implementation claim.
