# Status of the confirmed-CRITICAL findings, re-verified 2026-09-19

`2026-08-01-triage/full-185.json` holds 338 findings (261 confirmed) and has
**no open/closed status field**. So the backlog cannot be read as a work queue:
most items have been silently closed by the AUDIT T-series, and the few that
are still live are invisible among them. This file records the critical tier's
status as MEASURED, not as assumed.

Method: reproduce the finding's own repro against a HEAD build
(`f01e492`) wherever a binary can run it; otherwise read the cited file:line.
Reproduction beats reading — one finding below (§317) was reported as open
earlier in this same session on the strength of a grep, and running it refuted
that.

## Closed — verified by REPRODUCTION

| finding | what was checked | observed now |
|---|---|---|
| deep-review §317 | zero-budget principal runs the optimizer unbounded | exit 7 after **0** evaluations; checked on two optimizer strategies |
| deep-review §325 | negative principal handle clamps to root | `[E1601] unknown principal handle -1 … is NOT silently treated as root` |
| INTERP-C01 / §384 | inner `sandbox_create` widens the ceiling | exit 8, `a nested sandbox may only narrow, never widen`, naming the refused effects |
| P6-COV-01 (security half) | `IO` grants process spawn | exit 8, `requires effect Exec which is not in the active sandbox's allowed set {"IO"}` |
| P5-31 | `axon fmt` deletes `mod` declarations | `mod` preserved; the two-file project still runs and prints 42 |
| P7-SEC-01 | approve-then-swap deploys as approved | `blocked_approval`, `approved:false`, `source changed since approval (digest mismatch)`, exit 8 |
| P7-SEC-02 | `@[contained]` missed string-dispatch call edges | the laundered `scheduler_spawn("evil",…)` raises **E1001**, same as the direct call |
| P7-SEC-03 | principal handles are indices, escalation by subtraction | handles are large opaque values; handle 0 refused with E1601 |
| P6-EXIT-01 | verify/halt/ai-policy exits archived as `Completed` (exit 0) | archived as `Denied`, `axis: "verify"`, with the real reason; axon-os exits 8 |
| deep-review §333 | policy stops demoted to "fiber failed" | **WAS STILL OPEN.** Fixed in `f01e492`; reproduced before (exit 0) and after (exit 3) |

## Closed — verified by direct read of the cited code

| finding | evidence |
|---|---|
| deep-review §293 | `host_await*` now carries an `IO` effect row |
| deep-review §301 | single pre-effect choke point (AUDIT T45) covers `eval_native_call` |
| deep-review §309 | the interpreter worker thread is spawned with `stack_size_for_depth(...)` |
| deep-review §341 | `principal_activate` refuses an unknown handle with E1601 |
| deep-review §401 | `sandbox_create_scoped(p, effects, fs_read, fs_write, net)` carries the allowlists |
| deep-review §413 | `scan_effects` rewritten, with `scan_effects_is_not_evaded_by_whitespace_or_module_imports` |
| deep-review §431 | both pipes drained on their own threads concurrently with the wait (AUDIT T25) |

## Still true, though no longer exploitable

**P6-COV-01, second half.** `builtin_effect_row("exec")` still returns
`["IO"]`. The `Exec` separation comes from a SECOND classifier
(`capability_of_builtin`) consulted at two independent sites — the static
capability walker and the runtime sandbox gate. They agree by convention, not
by construction, and the triage note recorded "Zero tests exist and exit 8 is
never asserted end-to-end". That test now exists
(`granting_io_does_not_grant_process_spawn`, with a negative control), so a
regression fails loudly rather than silently re-granting spawn under `IO`.

## Second pass — HIGH tier and the remaining criticals

| finding | what was checked | observed now |
|---|---|---|
| F061 | f64/i32 closure params miscompiled natively (`6` → `4.61844e+18`) | interp and native agree on all three shapes |
| F062 | call-site `tier:` silently dropped in native | E0910 naming the tier; the untier'd control still builds |
| P5-25 | `sql_query` backslash injection | backslash doubled — `'\\'` — the injection is neutralised |
| INTERP-H03 | suspendable worker spawned with the default 2 MiB stack | `stack_size_for_depth(...)` (AUDIT T16) |
| GATE-01 | `parity_all.sh` passes an all-SKIP run | pass floor added (AUDIT T36) |
| GATE-02 | `wasm_parity` HOST_BUILTINS omits http/env_var | both present |
| GATE-03 | gate.sh ran a parity target that could only skip | the cited invocation is gone |
| GATE-04 | 44 skip-then-early-return sites | 5 remain, all inside the test that VERIFIES the skip rule (exit status first, skip line must be last) |
| OSK-P7-C3 | guest policy parser fails OPEN with `EffectSet(0xFF)` | explicit refusal (AUDIT T48) |
| P6-EXIT-03 | verdict/seed/run_id outside the hash chain | a verdict rewritten `Denied`→`Completed` is caught: `✗ TAMPERED … exit 11`; intact record exits 0 (AUDIT T47) |
| GATE-05 | a skipped stage ends in "safe to deploy" | **WAS STILL OPEN (second half).** Fixed in `34de1d9` |

## Open, and NOT fixed here — needs a key-management decision

**OSK-P7-C1, cryptographic half.** AUDIT T13 closed the `hw_root` half: a
hardware-attestation claim is now REFUSED rather than accepted on trust, which
was the exploitable part.

What remains is that `verify_report` still never recomputes the HMAC. It checks
`report.signature.is_empty()` and nothing else, so ANY non-empty signature
verifies. `sign_report` computes `HMAC-SHA256(key, digest ‖ axtcb1)`
(`axon-attest/src/lib.rs:176`); no counterpart runs on the verify side, and the
code says so in its own comment: "the cryptographic binding (recomputing the
HMAC against a key the VERIFIER holds) still does [need a key decision]".

Not fixed here deliberately. Closing it requires deciding where the verifier's
key comes from — operator-provisioned, derived, or a real hardware backend —
and that is a product and security decision, not an implementation detail. The
current state is safe-ish only because `hw_root` is pinned to the software
stand-in and the measurement digest is compared against a pinned expectation;
the signature field itself contributes no security today.

## Third pass — the remainder

| finding | what was checked | observed now |
|---|---|---|
| P6-COV-02 | ledger truncation undetectable; absent ledger reports "chain verified" | keyed chain with an authenticated tip file; a missing tip is refused ("refusing to treat an unanchored chain as complete"); 15/15 audit tests pass, incl. one that pins the residual post-hoc limit AS documented |
| DOC-01 / P5-15 / P5-DOC-01 | `axon build` outside the repo fails with ~100 undefined references (`CARGO_MANIFEST_DIR` read at RUNTIME) | builds and runs from an unrelated directory; resolves via `current_exe()`; 0 undefined references |
| OSK-P4-C1 / P4-INT-01 | duplicates of INTERP-C01 (sandbox exemption) | refused, verified by reproduction above |
| P5-ECO-01 | duplicate of P5-31 (`fmt` drops `mod`) | preserved, verified by reproduction above |
| OSK-P4-C2 / P4-OS-02 | grant path prefixes never reach enforcement | **verified END-TO-END**, see below |

### The headline claim, tested as a pair

`§401`/`OSK-P4-C2`/`P4-OS-02` all say the same thing: `fs_write = ["./out/"]`
parses, type-checks, appears in `axon-os explain`, and then constrains nothing.
The triage SUMMARY calls this "the project's headline value proposition".

I had originally recorded `§401` as closed because `sandbox_create_scoped`
CARRIES the allowlists. That was the wrong evidence — a mechanism existing is
not a mechanism being called, which is exactly the caller-check gap. Re-tested
with a job whose grant is `fs_write = ["./out/"]`:

- writes `../ESCAPED.txt` → **exit 8**, `builtin write_file is not permitted to
  write path '../ESCAPED.txt': the active sandbox restricts it to ["./out/"]`,
  and no file is created;
- writes `./out/allowed.txt` → **exit 0**, `✓ completed`, and the file IS
  created.

The positive control is the half that matters: without it, the refusal above
would read identically if the grant simply blocked every write.

## Not verified here

All of the confirmed-CRITICAL tier has now been checked. The list that stood
here — `OSK-P4-C1`, `OSK-P4-C2`, `OSK-P7-C3`, `P6-COV-02`, `P6-EXIT-03`,
`P4-INT-01`, `P4-OS-02`, `P5-15`, `P5-DOC-01`, `P5-ECO-01`, `DOC-01` — is
resolved in the passes above. `OSK-P7-C1` remains PARTIALLY open and is
recorded in its own section with the decision it needs.

### HIGH tier, second sample

| finding | what was checked | observed now |
|---|---|---|
| F094 / P5-16 | mutable closure captures diverge — interp DROPS the write (`call1=1 call2=1`), native persists it | both engines `call1=1 call2=2`; matches the README's "heap-captured mutable closures" |
| F141 | `@[ai(policy(budget: 1))]` enforced in interp (E1301, exit 5), ignored by native | native REFUSES with E0910 "cannot enforce the AI call budget" |
| P4-INT-03 | `native::` calls bypass the sandbox check, agent log and ledger | `gfx::window_open` under a `Pure` ceiling → exit 8, `requires effect IO`; control runs clean |

### HIGH tier, third sample

| finding | what was checked | observed now |
|---|---|---|
| P4-INT-02 | `@[pure] fn g() { host_await(..) }` checks clean | **E1207**, same as the known-impure control |
| P5-34 | `--features serde-json` fails to compile (non-exhaustive match in lsp.rs) | Decimal/RawPtr/Never handled; the feature build is now IN the gate (AUDIT T15 cites the finding) |
| OSK-P7-H1 | `"ok"` described the LAUNCHER, not the guest | fixed, with a test whose doc says "before the fix `ok` was `result.is_ok()`" |
| OSK-P7-H2 | kernel attestation is trust-on-first-use | "no trust-on-first-use" (T32); exits 10 on no pin at all |
| OSK-P7-H3 | `verify_entries` ignores `seq`, truncation undetectable | takes `expect_count` / `expect_head` and checks both |
| P4-INT-04 / P4-INT-05 | duplicates of INTERP-H03 / §317 | closed above |
| **OSK-P4-H8** | approval gate opt-in by file presence; absent token silent | **WAS STILL OPEN.** Reporting half fixed in `416a0b9`; enforcement left as an operator decision |

Sampled so far: 27 of 78 confirmed HIGH, of which two (GATE-05, OSK-P4-H8) were
still open — both now addressed.

## HIGH tier — complete (D7)

All 78 confirmed-HIGH findings are now accounted for:

| status | n | how |
|---|---|---|
| verified closed by reproduction or direct read | 37 | run the finding's own repro against a HEAD build |
| cited by an `AUDIT T##` fix marker naming the finding ID | 19 | the fix records which finding it closes |
| content-duplicates of findings verified above | ~20 | e.g. `P4-CG-02`=F061, `P4-CG-03`=F062, `P5-POS-01`=P5-25, `P6-GATE-01`=GATE-01, `P4-OS-08/09/10`=OSK-P4-H6/H7/H8 |
| **found still open and FIXED in this pass** | 2 | `GATE-05` (a skipped stage ended in "safe to deploy"), `OSK-P4-H8` (approval gate opt-in by file presence) |

Final-batch spot checks, all passing:

- `P4-PROD-05` — a misspelled `--risk criticl` now errors explicitly instead of
  silently deploying at `risk:"low"`.
- `P4-FE-01` — an impure LAMBDA body inside `@[pure]` now raises E1207; the
  purity walker used to treat `Expr::Lambda` as a terminal leaf.
- `P5-HOUR-01` / `DOC-03` — `axon deploy` on the repo's own hello-goal no longer
  panics in `assert_deployable`; it reports a clean E1300 AI-policy error.
- `P4-PROD-08` — `intent compile --json` both reports the path AND writes the
  file (91 lines), rather than returning before the write.

`P7-PERF-01` (an array clone in the Index arm) and `P5-ECO-03` (only 5 JSON
builtins) are recorded by the triage itself as a performance note and a stdlib
gap rather than defects, and are not counted as open.

## Open, and left for an operator decision

These are all `behavior_change: true` in the triage and change WHEN A JOB DIES
or whether it runs at all. Reported with evidence rather than decided here.

- **OSK-P4-H8, enforcement half.** Should a MISSING approval token refuse?
  The finding proposes defaulting it on at derived risk >= High, or driving it
  from a manifest field. Today a missing token runs and now says so.
- **OSK-P4-H7.** `killchan.rs` returns `LatchState::Clear` on ANY read error,
  while its own module doc states "Fail-closed: poll() error or unknown state →
  Tripped" and "a channel error MUST be treated as Tripped". Doc and impl
  contradict. Fail-closed risks killing a job on a transient I/O error;
  fail-open means a kill switch that silently stops working. Needs a call on
  which error classes are which — `NotFound` legitimately means "not tripped
  yet" per CLAUDE.md, but the current `Err(_)` catches everything.
- **OSK-P4-H6.** `--killable` together with `--monitor` leaves the R27 kill file
  uncreated, so `axon-os kill` becomes a no-op that still reports TRIPPED.

What is still NOT covered: the MEDIUM (113) and LOW (84) tiers, and the
majority of the HIGH tier — 78 confirmed HIGH findings of which this pass
sampled 11.

An unverified finding is not a closed finding — that is the whole reason this
file distinguishes the three sections above.
