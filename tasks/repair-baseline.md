# S1 baseline (repair/int @ f83c6f86)

`./scripts/gate.sh` (non-strict), `CARGO_TARGET_DIR=.doc-build/rep-int`, 2026-10-17: rc 0, `✅ gate PASSED`,
387 binaries, 6710 passed, 0 failed, 12 ignored, real 29m56s (lanes started building in the last minutes).
"REDTEAM FAILED: blocking deploy" (log line 4882) is expected demo output.

Known pre-existing: 4 serde-json-only clippy errors in `lsp.rs`; relative `cortex repair --workspace` exits 22 (review #27).
