# Todo — derived view of commit history

Not a plan. This is what the commits on `governance-audit-2026-07-18` actually
show, so it can be rebuilt from `git log` if this file is lost.

## Done

| task | commit | what |
|---|---|---|
| Step 0 | `47e79c3` | baseline pinned: 1 failing test, not 2 |
| Step 1 | `47e79c3` | adversarial triage of the 20 criticals |
| Step 1b | `9d3af95` | full 185-finding triage folded into the DAG |
| **T1** | `d74d04c` | nested sandbox may only narrow the ceiling (F041, F013) |
| **T8** | `71e135b` | an IO grant no longer implies process spawn (P6-COV-01) |
| **T4** | `ac1a590` | `scan_effects` not evaded by whitespace / `mod` (OSK-P4-H1) |
| **T2** | `870469a` | `@[contained]` not launderable via string dispatch (F153) |
| T5+T6 | `1a89758` | MIT LICENSE added; README test count corrected |
| **T3** | `4dd69e2` | fs prefixes + net hosts actually constrain (OSK-P4-C2, F014, F040) |
| T7 | `aba53a2` | CI covers 19 crates + codegen/parity, not 1 crate and no codegen |
| **T9** | `3e579ff` | `axon fmt` no longer deletes `mod` declarations (P5-ECO-01, P5-31) |
| **T10** | `c0c4d55` | deploy approval binds the program text, not the filename (P7-SEC-01) |
| **T11** | `5a3282f` | link no longer cwd-dependent; un-skipped a dead parity gate (P5-15, DOC-01) |
| T12a | `88cd2e4` | guest image builds again (json-target-spec + EXIT trap scope) |
| O006 | `570dd95` | meta-test: harness success markers must be strings their scripts emit |
| **T13** | `de03d7f` | attest refuses an unverifiable hardware root-of-trust claim (OSK-P7-C1, partial) |
| O006b | `c9c5491` | harness skips countable + fatal under `AXON_HARNESS_STRICT` |
| **T15** | `c79034a` | `--features serde-json` compiles again (`axon lsp`/`parse --json`) + gate stage |
| **T14** | `6426c75` | wasm-parity corpus fixed — **suite now 426 passed / 0 failed** |
| O009 | `e459558` | drift test: the pure-compute corpus can no longer silently widen |
| O010 | `9cb7859` | persistent-bandit fixed /tmp path removed (mechanism undiagnosed — see correction) |
| O011 | `110`-era | persistent-learner same hardening (explicitly NOT a bug-fix claim) |
| **T16** | `—` | host_await worker gets a deep stack: abort/exit-134 → graceful limit (INTERP-H03) |
| **T17** | `1289b74` | deploy gates accept 0/1-arg (P5-01, Acid Test 2 works); legacy approvals ≠ tampering |
| O012 | logged | F062 per-call AI tier ignored natively — attempted, reverted, needs a `walk_expr` refactor first |

**Ten fixes landed; all four confirmed sandbox-escape CRITICALs are closed** (F013, F041, F153,
plus OSK-P4-C2 which triage rated critical). Each has a regression test verified
to FAIL before the fix — no fix landed against a test that would have passed
anyway, which is the defect class this audit exists to document.

## Not done, deliberately

- **O012 / F062** — verified real, fix reverted rather than rushed: it needs a
  generic `walk_expr` extracted first, because copy-pasting `expr_calls`'
  70-line recursion is how the "walker missed an arm" class recurs (already
  fixed 3x here: R6 taint, `@[contained]` helpers, T2 string dispatch).
- **~160 findings remain untriaged-for-implementation.** They are triaged for
  *severity* (`governance/reviews/2026-08-01-triage/full-185.json`) but no code
  was written for them. Effort profile: 59 trivial / 125 small / 112 medium /
  42 large.
- **8 needs-human decision groups** — see `build-loop.md`. Group A
  (exit-code semantics) blocks several confirmed criticals: `main()` returning
  3/6/8 collides with reserved exit codes and 256 reports success. Nothing
  downstream can be closed properly until that is decided.
- **O005 — CI's `cargo fmt` job is red on `main`** (41 diffs, from the parallel
  R21/R23 track). Not fixed: a unilateral reformat of another track's live files
  would collide with concurrent work.

## Verification standard used

Through T13: green = **no failing test other than
`wasm_interp_matches_native_on_pure_compute`**, the single clean-baseline
failure. From **T14 onward: zero failures** — that test was fixed rather than
tolerated, so the bar tightened mid-run. Every task was checked against the
standard in force when it landed, never against an aspirational "100% green".
