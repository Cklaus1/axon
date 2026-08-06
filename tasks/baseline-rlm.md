# Step 0 baseline — build-loop over `AXON_FOR_RLM.md`

Every later gate compares against **this**, not against an aspirational
"100% green". Full log: `tasks/baseline-rlm-full.txt`.

- **Commit:** `3ba5b47` (spec review) · branch `governance-audit-2026-07-18`
- **Command:** `cargo test --workspace` (19 crates)
- **Wall time:** `7m20.780s` — this is `<FULL_SUITE_BUDGET>`; one tier's batch of
  tasks must fit inside one run of it.
- **Process exit:** `0`

## Result

**1744 passed · 0 failed · 1 ignored.**

## The failure set, as a set

```
BASELINE_FAILURES = { }        # empty
```

Recorded as a set rather than as a count or as prose, because the regression
loop diffs it in **both** directions and a count cannot be diffed. The
consequence of it being empty is that the gate is exact: *any* failing test
after any task is a new failure and blocks, with no judgement call about whether
it was pre-existing.

One `grep FAILED` hit in the log is `REDTEAM FAILED: blocking deploy` at line
599 — the expected stdout of the redteam acceptance test asserting that it
blocks. It is a passing test printing the word FAILED, not a failure. Noted
because a naive `grep -c FAILED` on this log returns 1 and would look like a
baseline failure to the next reader.

## Drift watch

The prior run's baseline (`tasks/baseline.md`, 2026-07-18) recorded **1 failing
test**, `wasm_interp_matches_native_on_pure_compute`, later fixed. The suite has
since grown from 430 to 1744 tests. Both numbers are consistent with that file's
own `[FINAL 2026-08-01]` note. This is a re-capture, not a drift signal.

That prior run also documented a real flake class under full-workspace parallel
contention (`wasm_browser_*`, `persistent_learner_demo_*`) — non-isolated fixed
`/tmp` paths. None flaked in this capture, but if one goes red mid-run the
standing rule applies: **isolated rerun first**; only a second isolated failure
is a regression.
