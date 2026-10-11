# Example Bundle Guide

All entries here are **synthetic fixtures and proposed contracts**, not deployment manifests, live model answers, real usage records or authorization receipts.

| Bundle | What it demonstrates | Execution status |
|---|---|---|
| `document` | Normalize -> draft -> nonempty check -> assertion -> return; separate required completeness criterion; targeted repair | Offline simulation implemented |
| `coding` | Isolated repository snapshot -> baseline tests -> proposed patch -> sandbox apply -> regressions -> exact patch return | Structural/selected semantic validation only; no real code runs |
| `research` | Retrieval -> normalization -> source check -> synthesis; separate required claim-support judgment | Structural/selected semantic validation only; learned acceptance disabled |

Each bundle has an intent under `intents/`, a protected evaluation-contract proposal under `evaluations/`, and a portable Semantic Plan under `plans/`. All pin `registry.json` and `../config/defaults.json` by identity digest. Registry implementation hashes identify symbolic fixture implementations, not actual binaries or source trees.

`document_captures.json` contains an intentionally incomplete draft and an improved draft. The simulator's completeness checker only tests for required terms; it is deliberately a toy, not a general completeness evaluator. It cannot establish that a real report is correct. The example proves the reject/repair/recheck plumbing.

`invalid/` holds twelve intentionally rejected plans. `invalid_cases.json` defines the expected diagnostic per case; some cases correctly trigger additional diagnostics. The package checker treats expected rejection as success, never as valid executable work.

The coding example requests isolated workspace writes, but the default configuration permits no external effects. Passing its candidate shape checks is not runtime admission. The research example has no approved learned decision policy; it cannot be automatically released under defaults. Real adapters, detailed schemas, evidence policies and authorization are later milestones.

`experience.json`, `event.json`, `evaluation_result.json` and `repair_request.json` demonstrate their envelope shapes. Their zero usage values are fixture values, not estimates of real services. Never ingest these rows into production optimization or promotion datasets.

The top-level alignment examples (`owner_profile.json`, `strategy_artifact.json`, `experiment_profile.json`, `evidence_projection.json`, `resource_projection.json`, `forge_binding.json`, and `compilation_record.json`) exercise the Cortex/Forge boundary contracts. They are projections/reference fixtures, not alternate owner stores or live service payload guarantees.
