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

### Boot attestation — the R26 gate was not actually mandatory

| task | commit | what |
|---|---|---|
| **T32** | `—` | `axon-vm run`'s "mandatory kernel attestation" had two bypasses, both reproduced against the built binary against a tampered kernel. **(a) Trust-on-first-use.** A missing `~/.axon/kernel_baseline.sha256` made the CURRENT kernel the baseline — `attestation: baseline established`, exit 0, tampered image booted. The file is user-writable at a predictable path, so anyone who can swap the kernel can `rm` the pin. **(b) An env var.** `AXON_CI_NO_KVM=1` — inherited, not a flag, not typed at the boot site — short-circuited the gate to `Ok` on any host, including with a correct baseline present (P7-KRN-05 / P4-OS-16) |

Fixed by making a missing pin a REFUSAL (exit 10, `no pinned baseline`), removing
the env bypass entirely so `--no-attest` (which warns) is the only one, adding
`attest --pin-baseline` as the explicit operator action that blesses a kernel
(refused for a mock/absent kernel; overwriting needs `--repin`), and adding
`run --expect-digest <sha256>` so the pin can come from off the box entirely and
outrank a planted baseline file. `REQUIREMENTS.md:72` claimed "mandatory kernel
attestation before every boot" throughout; that row now records when it became
true.

The pattern from T29/T31 again, from a third angle: **the bypass was in the
same function as the check.** Both `test_attest_ci_mock_mode` and the TOFU branch
were tested — as features, asserting they returned `Ok`. A test can pin a hole
open just as easily as it can catch one.

`scripts/axon_kernel_gate.sh` Layer 3 now pins the kernel it just built rather
than relying on TOFU. That gate is red on this host for an unrelated,
pre-existing reason (O025: a halted guest hangs `axon-vm run` forever — same
under `--no-attest`).

### Guest outcome vs launcher plumbing — the microVM reported its own success

| task | commit | what |
|---|---|---|
| **T33** | `—` | `axon-vm run`'s `"ok"` was `result.is_ok()` — whether the LAUNCHER drove the Firecracker API, not what the guest did. The headline containment case, an agent whose `openat` the in-guest syscall gate DENIES, reported `{"ok": true, "exit_code": 124}` and **process exit 0**, after burning the full 45s deadline. Three causes: the guest's `-VIOLATION8` serial sentinel had no parser in `axon-vm` at all; the guest's ACPI-S5 power-off (port 0x604) is a no-op under Firecracker, so it span in `hlt` until killed; and `--json` printed and fell off the end of `cmd_run`, so `axon-vm run --json` exited **0 unconditionally** whatever the guest did (P7-KRN-04) |

Before → after on the same command, same image:

```
BEFORE  {"ok": true,  "exit_code": 124}                    process exit 0   20.2s
AFTER   {"ok": false, "exit_code": 8, "guest_outcome":     process exit 8    0.16s
         "violation"}
```

Fixed on both sides. Host: the serial drain watches for `-VIOLATION8` /
`-PANIC<n>` / `-EXIT<n>`; the guest's own verdict is authoritative and is acted on
even if it then fails to power off (a substrate that ignores every shutdown
mechanism must not be able to turn a refusal into a success); `ok` means the guest
reached a definite end and exited 0; `--json` propagates the exit code; schema →
`axon-vm-run/2` with a new `guest_outcome` field. Guest: shut down via **i8042
reset (0x64 ← 0xFE), which is what Firecracker actually implements** — hence
`reboot=k` in the boot args — keeping S5 and the ISA debug-exit device as QEMU
fallbacks. A clean run went from a 45s timeout to 157 ms.

**`axon_kernel_gate.sh` Layer 3 had never once observed the violation it tests.**
`run` derives the guest policy from the program's `.axmeta`, and the evil agent
cannot have one — the compiler refuses it (3× E1001), so `build --emit-manifest`
produces nothing, and with no manifest the policy defaults to OPEN. The check was
passing FS to an agent whose whole purpose is to be denied FS; it failed on a
timeout, not on a denial. The gate now states the grant explicitly. That default
is logged as **O026** — unfixed, because closing it changes every unmanifested
run and is an operator's call, not a bug fix to slip into an unrelated commit.

### Deploy risk + gates — the incentive was inverted and the gates were open

| task | commit | what |
|---|---|---|
| **T34** | `—` | `derive_risk_from_ast` read only DECLARED effect rows and `@[contained]` args on top-level fns. Executed: a program that really `exec`s a shell and `write_file`s, declaring nothing, derived `risk:"low"` and deployed (both side-effect files appeared on disk); a program declaring `\| {Exec}` that does nothing derived `critical`. **Declaring your effects honestly made you look dangerous; hiding them made you look safe** (P7-SEC-06) |
| **T34** | `—` | at Risk ≥ High a MISSING pipeline gate counted as a passed one, and the gates were read from the very file under review. A Critical-risk deploy ran with `stages_run:[]`, exit 0, and executed the program. `main.rs`'s own comment conceded the open gate. A program cannot be its own red team (P7-SEC-07) |

Risk is now the **max** of what the program does — via
`capabilities::program_capabilities`, which already had the right answer and was
never consulted — and what it declares. That walker follows impl-method bodies,
string-dispatch call sites and comptime initializers, i.e. exactly the laundering
routes the old top-level item-walk missed by construction. A declaration can
still only RAISE. The dead `@[contained]` attr-arg scan (parsed into `f.contained`,
never left in `attrs`) is gone.

At Risk ≥ High a missing gate now BLOCKS (exit 1, `blocked_gate`,
`missing:<names>`), and the blocked deploy does not run the program — verified by
its absent side-effect file. `--gates <path>` lets the DEPLOYING operator supply
gates from outside the artifact; `--allow-missing-gates` restores the old
behaviour with a warning on every run, the same single-explicit-bypass shape as
`--no-attest` in T32. Below High the open-gate convention is unchanged — the
finding is about the tier where the pipeline is claimed to be mandatory.

**Two existing tests encoded the defect as expected behaviour** and had to be
corrected, not worked around: my own T30 test asserted `--risk high` deploys
exit 0, and `r33_acceptance_gate.sh` deploys `hello.ax` at `--risk high`
expecting success. Both now pass `--allow-missing-gates`, because both are
measuring something else (risk-level parsing; the quorum gate) and would
otherwise be measuring this instead.

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

**Twenty-two fixes landed; all four confirmed sandbox-escape CRITICALs are closed** (F013, F041, F153,
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
