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
| **O004** | `f9f3869` | all wasm harnesses take the shared build lock — **suite 430/0, no known flakes** |
| **T19** | `—` | checked arithmetic in the guest-kernel bump allocator (OSK-L02) |
| **T20** | `—` | unknown principal handle refused, not silently treated as root (F006) |

### axon-os supervisor sweep — found by EXECUTING code-read findings

| task | commit | what |
|---|---|---|
| **T23** | `—` | `AXON_*` operator controls forwarded through `env_clear` — the R28 ledger was never written for ANY supervised run; mock/replay were inert (OSK-P4-H4) |
| **T24** | `—` | a job that never ran no longer seals as `Completed` — parse failure sealed `Completed{2}`, AI-policy refusal `Completed{5}` (O016/O018) |
| **T25** | `—` | child pipes drained concurrently — >64 KiB output deadlocked, blamed as `Denied{time}`; 30s → 0.50s (OSK-P4-H3) |
| **T26** | `—` | `axon-os kill` trips every live latch — `--killable --monitor` polled nothing while printing "🛑 kill tripped" (OSK-P4-H6) |
| **T27** | `—` | compliance monitor drains the ledger before exiting — violations in the last ~100 ms were reported as a clean run (OSK-P4-H5) |

### Formatter — found the same way (running the tool, not reading it)

| task | commit | what |
|---|---|---|
| **T28** | `—` | `axon fmt` no longer emits source it cannot re-parse. `1e400` overflows to infinity at lex time; the renderer wrote the bare word `inf`, then appended `.0`, producing `inf.0` — so formatting a VALID file produced one that fails `E0001 cannot find name 'inf'`, in place, exit 0. Also `1e100` → a 103-char literal. (O021) |

The four `examples/*.ax` files dirty at session start were not an edit — `axon fmt`
on a clean checkout reproduced them byte-for-byte. Restored to committed state.

**The test gap matters more than the bug.** `fmt.rs` already had five round-trip
tests; every one asserts `out1 == out2` — idempotence only, never comparing
against the input. A stable-but-wrong rendering passes all five. Added
`assert_float_fidelity` (output must parse; every float must survive with an
identical bit pattern). Third data-loss bug in this one tool — `emit_program`
still carries the `AUDIT T9` note about `mod` declarations having been silently
DELETED — which argues the AST-based architecture is the problem, not any arm.

### Purity / safety-flag sweep — executed-repro first, per the re-triage result

| task | commit | what |
|---|---|---|
| **T29** | `—` | `@[pure]` could launder any impure operation through a lambda — the purity walker listed `Expr::Lambda` in its terminal LEAF arm, so it never looked inside a closure body. `arr_fold(xs, 0, \|a, x\| { println("boom") a + x })` in a `@[pure]` fn checked exit 0, while the same `println` written directly is E1207 (P4-FE-01) |
| **T29** | `—` | the four R15 `host_await*` builtins were in neither `is_impure_builtin` nor `builtin_effect_row`, so `@[pure] fn g() -> str { host_await("hi") }` checked clean — they suspend to the host and resume with its reply, which is I/O plus unbounded nondeterminism (P4-INT-02) |
| **T30** | `—` | `axon deploy --risk criticl` (one typo) reported `risk:"low"`, `stages_run:[]`, `status:"deployed"`, exit 0. `.unwrap_or(0)` collapsed an unparseable level to the WEAKEST one and the `== -1` guard beneath it was dead code — `parse_risk_level` returns Option and never yields -1. Fail-open on a safety flag (P4-PROD-05) |

T29 is the transitive-laundering class again, third instance after `@[contained]`
and `@[sensitive]`: a guard that inspects only the immediate body is escapable by
moving the work one hop. Worth a systematic sweep of every walker's leaf arm
rather than waiting for the next report — `Expr::InlineAsm` is still a leaf there.

### Attestation-chain integrity

| task | commit | what |
|---|---|---|
| **T31** | `—` | chain **truncation** was undetectable in both verify paths. `chain.rs`, the CLI help and the R34 spec all claimed removal was detectable; only INTERIOR deletion was, and only interior deletion was tested. Chopping the tail off a 3-entry chain reported `CHAIN OK: 1 entries` exit 0; erasing it reported `CHAIN OK: 0 entries`; truncate-then-re-export reported `EXPORT OK`. Added `--expect-head`/`--expect-count` pins (OSK-P7-H3 / P7-KRN-06 / P6-COV-02) |

Linkage cannot catch truncation unaided — every prefix of a valid chain is a
valid chain, and `--genesis` pins the ROOT while truncation moves the TIP. The
pin has to come from outside, so **nothing yet stores one**: where the tip should
live (R33 quorum state / R28 ledger / external attestation) is on the
needs-human list, not something a local fix can close.

### R16 Axon UI spec (design work, no code)

| section | what |
|---|---|
| §1b | prior-art survey; **decision: we do not build the renderer** |
| §1c | **the whole chosen stack is pre-1.0** — Vello alpha, Masonry/Xilem experimental, Blitz pre-alpha; only Taffy mature, only Slint stable (and not permissively licensed) |
| §1d | **decision: the renderer seam MUST admit a CPU path** — decisive reason is that a GPU-only renderer cannot be gate-tested |
| §3a | rich text marked **a11y-BLOCKED** (externally gated on AccessKit, not on our slice order) |
| §3b | Parley + AccessKit as **type obligations**, not backend details |
| §3c | read Blitz's wiring — yielded the stable-identity requirement |
| §3d | **ASI view semantics** — uncertainty / pending / streaming / agency as one problem |
| §3d(a) | first ASI slice, to implementation depth, **with a stated falsifier** |
| §5 | stable `View` node identity + `.key()` |
| §6a | E2109/E2113–E2116 allocated |
| §7.2 | **empirical status of the controls §7.1 depends on** — the audit findings above, in the spec |
| §9.0 | cross-slice acceptance obligations; every criterion must execute and assert an artifact |
| §11a | §3d(b)(c)(d) need three types the language **does not have** |

**Nineteen fixes landed; all four confirmed sandbox-escape CRITICALs are closed** (F013, F041, F153,
plus OSK-P4-C2 which triage rated critical). Each has a regression test verified
to FAIL before the fix — no fix landed against a test that would have passed
anyway, which is the defect class this audit exists to document.

## Not done, deliberately

- **O012 / F062** — verified real, fix reverted rather than rushed: it needs a
  generic `walk_expr` extracted first, because copy-pasting `expr_calls`'
  70-line recursion is how the "walker missed an arm" class recurs (already
  fixed 3x here: R6 taint, `@[contained]` helpers, T2 string dispatch).
- **The execution re-triage result (the most useful thing to carry forward).** 8 code-read-only
  findings were re-checked by RUNNING them: 8 answers changed — 2 false (OSK-L03 `--latest`,
  INTERP-H04 budget bypass), 2 true-but-understated, 1 true-and-mis-attributed, 3 true as written.
  Reading never once predicted the outcome. Of the findings worked all session, every code-read
  one cost a revert and every executed-repro one landed cleanly. **Work the remaining findings in
  that order** — the triage JSON records which is which ("REPRODUCED" vs "verified by direct read").
- **~150 findings remain untriaged-for-implementation.** They are triaged for
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
