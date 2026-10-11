# CX-30 v0.23 Forge integration amendment — semantic repository improvement

CX-30 uses Forge as the semantic change substrate. Improvement candidates SHOULD use Transform or Migration artifacts where semantic support exists, emit semantic diffs, compile/test exact artifacts and submit evidence to canonical Axon experimentation. Text edits remain available as a lower-information fallback.

## v0.23 acceptance gates

**G30-r23-semantic-transform:** A repository optimization based on Forge Transform records applicability, semantic diff, exact output artifact and verification requirements before comparison.

**G30-r23-migration:** Large repetitive refactors use a Migration dry-run with scope, rewrite, idempotency expectation and rollback/validation metadata before application.

**G30-r23-build-program:** Programmable build logic produces an inspectable deterministic BuildGraph and does not hide undeclared tool/effect dependencies.

