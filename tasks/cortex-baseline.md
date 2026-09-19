# Step 0 baseline — Axon Cortex build loop

Captured 2026-09-19T00:44:51-04:00 → 00:58:47-04:00 (13m56s) on
`diag-hint-and-location`, tree clean.

**TEST_CMD:** `cargo test --workspace`
**FULL_SUITE_BUDGET:** ~14 min (one measured run)

## Failure set (explicit test IDs, not a count)

```
editing_an_imported_module_invalidates_the_build_cache
```

Everything else green.

## Characterisation — FLAKE, not a regression

Passes **3/3 in isolation**. It shells out to `axon build`, and 31 cli_run
tests do the same; under parallel workspace execution they contend on the
shared `target/` directory. This is the repo's own documented
"concurrent builds fake failures" class.

It is therefore a pre-existing flake: it does NOT block, and per the loop rule
it is logged to `opportunities.md` rather than fixed inside a Cortex task.

**Diffing rule for this run:** compare BOTH ways. A new failure beyond this set
blocks. This ID unexpectedly passing is not evidence of improvement — it is the
expected case. Any OTHER baseline-listed ID passing without a task claiming it
would mean the baseline drifted, and gate results from before that point are
unverified.

## Smoke signal (Step 0 requirement)

`RUN_CMD` for this run is the Cortex conformance binary built by task C11.
Until it exists the smoke slot is UNFILLED — deliberately, rather than
pointing it at an existing Axon command that would pass without exercising
any Cortex code. C11 is the task that fills it; if C11 is blocked, the run
reports the smoke test as never-run rather than substituting a passing proxy.
