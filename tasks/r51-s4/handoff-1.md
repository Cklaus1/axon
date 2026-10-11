# S4 handoff 1 (S4Pick analysis, no code)

I hit the 200-request budget before writing any S4 code, so slice S4 is not done. None of the six assignment items are implemented, and nothing has been built, tested, mutation-checked or perf-measured.

**What changed on `r51/s4`:** only two merges of `r51/int`. The first is af91688e, with the S3c x S3d fixes you asked me to take. The second brings in 11f36710 (S3e), which added a 16th S4 entry to the registry. There are no S4 commits and no `pci_delta.py` THEMES entries.

**Registry:** still 199 entries, with 16 marked S4 (9 in `sealed_frames.rs`, 1 in `dispatch_rule_switch_is_test_only.rs`, 6 interp.rs unit tests). None have been un-ignored or re-slotted.

**Your S6 probe:** I closed none of the failures you listed and did not run the probe on this branch. They all trace to the missing PICK bit, the runtime dispatch check and the dispatch-rule switch.

The full veto ↔ main site mapping is in the structured result I submitted. These are the parts that will cost the next attempt the most time:

- **The toggles race on main.** Main has no `SEALED_DIRS_TEST_LOCK`, so veto's global `DISPATCH_RULE_OFF` / `TAINT_FORCE_ON` would race between parallel sealed unit tests. I chose per-thread, test-only switches, each declared on one line directly under `#[cfg(test)]` so the switch drift test can see them. `TAINT_FORCE_ON` defaults to off, as in veto.
- **Some existing tests need updating once the switch exists.** Two sink tests rely on the runtime taint refusing: the name-rule test and the width part of the arithmetic test. They will need to turn `TAINT_FORCE_ON` on.
- **The builtin pick needs per-argument taints.** Main's `dispatch_named` doesn't have them, and the VM shares that function. The plan is for the sealed path of `eval_call` to compute the selector and leave it in a sealed-only cell that `dispatch_named` reads, the same pattern as `set_call_tier`. That keeps the VM's hot path and the function signature unchanged.
- **The three identity-change attacks are covered by design:**
  - Cast re-wrap: the new closure shares the capture cell, so the mark survives.
  - `SendValue` round-trip: this builds fresh captures, but the result still carries the argument's taint, so the evaluator re-marks it. It can be tested with an echo host on the test thread.
  - `fn(..)`-typed parameter: the binding takes full taint from the sealed caller, and a note at the local-closure call marks it.
- **The frame-exit PICK needs a signature change.** `t_frame_exit` must take the result value. Its two callers are at interp.rs:5209 and :5928.

**Still to do: all of S4.**
1. The PICK mark, its edges and the refusing sinks.
2. The receiver dispatch rule, plus the two test switches.
3. The T8.1 attacks, each with a control.
4. The T5 port, diffed against veto.
5. Settling all 16 registry entries.
6. Extending the drift tests to cover the new sinks.

After that come the THEMES entries, the mutation table, fmt/clippy and the full suite, parity, and perf at head versus 5abfc0bc in the same worktree.
