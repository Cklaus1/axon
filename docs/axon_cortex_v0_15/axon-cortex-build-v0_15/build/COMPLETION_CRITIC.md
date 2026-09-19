# Completion Critic build guide

A cheap completion critic helps detect premature agent stops; it never becomes the authoritative DONE mechanism.

## Contract

```text
agent proposes stop
      ↓
Completion Critic
  "likely incomplete?" + missing-clause candidates
      ↓
if suspicious: continue/inspect unresolved intent evidence
      ↓
protected AcceptanceContract verifier
      ↓
VerifiedComplete | Incomplete | Unknown
```

The critic consumes the approved IntentIR/BuildContract, current progress/evidence graph and unresolved acceptance clauses. It may emit `Continue`, `LikelyComplete`, `ObserveMore`, or `Blocked`, plus references to potentially missing clauses. It cannot rewrite the contract or certify completion.

## Build sequence

1. Construct premature-stop fixtures from coding episodes where a local milestone was mistaken for completion.
2. Run in shadow mode at every proposed stop.
3. Measure false-continue and false-complete rates against protected completion evidence.
4. Add clause-level missing-work extraction.
5. Only after evidence, allow the critic to request another planning turn; verifier remains final authority.

## Required tests

- all checks pass but critic says continue: verifier still closes the task;
- critic says complete but one hidden acceptance criterion fails: task remains incomplete;
- model prose claims success with no evidence: critic/verifier do not accept it;
- approved scope is exhausted and missing work requires new authority: critic returns Blocked/requests resolution rather than widening scope.
