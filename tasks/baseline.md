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

## [FINAL 2026-08-01] Suite is 430 passed / 0 failed, no known flakes

Two separate things had to be fixed to get here, and they were different:

1. **T14** fixed the one genuinely failing test (`wasm_parity.sh` classified
   host-touching examples as pure compute). That took the gate from
   "one accepted failure" to zero — on a clean, serial run.
2. **O004** fixed the last *flake*: only 9 of 21 wasm-building scripts took the
   shared `flock`, so the rest clobbered their intermediates under parallel
   load. Full-suite runs varied (26 / 11 / 5 examples linked against a floor of
   28) until all ten remaining harnesses took the lock.

`cargo test -p axon-core --no-default-features --test cli_run`
→ **430 passed, 0 failed, 388s.**

Green now means green under FULL-SUITE LOAD, not just standalone. That
distinction cost three attempts to learn: two harness "fixes" measured
standalone looked correct and were regressions.

## [SUPERSEDED] Gate tightened to ZERO failures

The one baseline failure was **fixed**, not tolerated — see T14 / `6426c75`.
Root cause: `scripts/wasm_parity.sh` classified four host-touching examples
(`env_var` / `http_*`) as "pure compute", so the corpus diverged on an
unconfigured-network path. `cli_run` is now **426 passed, 0 failed**.

> **Green, from T14 onward, means ZERO failing tests.**

Everything committed before `6426c75` was verified against the older, looser
bar ("no failure other than `wasm_interp_matches_native_on_pure_compute`"), which
was correct at the time — that test was genuinely red for reasons unrelated to
those changes.

## Gate definition for this run (superseded — kept for audit)

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
