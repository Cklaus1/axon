# Semantic Migrations

A Migration is a Transform specialized for large, repeatable codebase changes.

It adds scope, match predicate, rewrite, validation, rollback metadata, progress, idempotency declaration, and semantic-equivalence class.

Workflow: discover pattern -> author Migration -> dry-run semantic diff -> sandbox apply -> compile -> targeted tests -> full gates -> Axon experiment.

Migrations MUST support dry-run and semantic diff. They SHOULD be idempotent where feasible.
