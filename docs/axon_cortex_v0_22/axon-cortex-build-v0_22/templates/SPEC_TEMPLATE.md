# Cortex specification template

Use the repository's actual governance template on import. Package metadata is only a staging format; map it to the real spec-meta schema and reserve official IDs through the existing registry.

```yaml
id: CX-NEW
status: Draft
authority: Proposed
depends_on: []
implementation_evidence: []
```

## Intent and source basis

State the requirement and exact source/measurement. Separate attachment claims, reproduced evidence and new proposals.

## Decisive fork

Name the alternatives, chosen path, rationale and revisit condition. Do not hide a product/TCB decision inside implementation detail.

## Required contracts

Define types, errors, effects, authority, nondeterminism, state transitions, budgets, concurrency, privacy, compatibility and rollback. List what the subsystem cannot guarantee.

## Integration

Identify existing Axon seams, exact supported engines/hosts, type/reference changes, and dependencies. Proposed paths remain labeled until confirmed.

## Acceptance gates

For each gate, include a positive fixture, adversarial/negative case, observable result, allowed uncertainty, executable command after implementation, and artifact/version binding. Missing required evidence blocks completion.

## Build slices and exclusions

Break work into reviewable units with exact outputs and prerequisites. Keep research experiments and optional platform work off the core dependency path unless evidence makes them necessary.

## Open decisions and evidence

List genuine unresolved choices, safe blocked behavior, owner and evidence needed. Do not use “complete” until actual executable evidence exists under the repository's governance.
