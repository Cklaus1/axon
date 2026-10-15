# Gaps baseline (Step 0)

Tree: `main` b11841c8 + 072678c5 (artifact-admission allowlist for the DecisionBench fixture; without it
`gate.sh` stops at "artifact admission" on main since 37c8f460), branch `gaps/int`, host gpumaster, 2026-10-15.

| Command | Result | Wall |
|---|---|---|
| `./scripts/gate.sh` (`TMPDIR=/var/tmp/r51-tmp`, own `CARGO_TARGET_DIR`) | rc 0, `✅ gate PASSED`; 378 `test result` lines, 6622 passed, 0 failed, 12 ignored | 30 m 13 s |

Non-strict: the codegen/wasm parity wrappers are skipped (listed by the gate). Strict parity is measured per G4.

Known failures: none.
