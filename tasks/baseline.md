# Step 0 — environment baseline

Recorded verbatim so every later gate compares against **this**, not against
"100% green". Full log: `.archive/baseline.txt` (2138 lines).

- **Commit:** `20eb218` · branch `governance-audit-2026-07-18`
- **Command:** `cargo test --workspace` (19 crates)
- **Wall time:** ~6 min; the `axon-core` `cli_run` suite alone is **315 s**,
  which sets the full-suite budget.

## Result

| crate/suite | result |
|---|---|
| `axon-core` unit (`lib`) | 569 passed, 0 failed, 1 ignored |
| `axon-core` `cli_run` | **418 passed, 1 FAILED** |
| all other 18 crates | ok (0 failed) |

**Aggregate: 1 failing test at baseline.**

## The one baseline failure

```
test wasm_interp_matches_native_on_pure_compute ... FAILED
  crates/axon-core/tests/cli_run.rs:13660
  wasm_parity: 35 pure-compute examples auto-discovered
    DIFF anthropic_stream.ax native=(code 1) wasm=(code 1)
      native stdout: stream error: http_sse_post requires the asi-runtime feature
                     or a network-capable host
      wasm   stdout: error: ANTHROPIC_API_KEY environment variable not set
  wasm_parity: 34 passed, 1 differ
```

Both targets exit 1. This is a **message** divergence on an unconfigured-network
path, not a compute divergence. Tracked as O001.

## Gate definition for this run

> Green = **no failing test other than `wasm_interp_matches_native_on_pure_compute`.**

Any task that introduces a second failure is reverted, not rationalised. A task
that *fixes* O001 tightens the gate to zero failures for everything after it.

## Correction to the earlier baseline

A partial run earlier in this session showed **2** failures, the second being
`wasm_browser_examples_run_identically_via_js_host` (26 examples linked vs a
floor of 28). On this clean full run that test **passes**. The earlier failure
was a stale/partial `target/` state, not a real defect — see O002 and L003.

## Ignored / not run

- `scripts/gate.sh --strict` extras (SMT tests) SKIP silently when `libz3` is
  absent — verify z3 presence before trusting a strict-gate pass.
- CI (`.github/workflows/ci.yml`) only runs 4 commands, all
  `-p axon-core --no-default-features`. **CI does not build codegen at all**, so
  a green CI is much weaker than a green local baseline.
