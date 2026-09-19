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

## Not verified here

These need binaries or environments outside this pass and are NOT claimed
either way: `OSK-P4-C1`, `OSK-P4-C2`, `OSK-P7-C1`, `OSK-P7-C3` (attest / vm /
guest-kernel), `P6-COV-02` (ledger truncation), `P6-EXIT-03` (record hash
chain), `P4-INT-01`, `P4-OS-02`, `P5-15`, `P5-DOC-01`, `P5-ECO-01`, `DOC-01`.

An unverified finding is not a closed finding — that is the whole reason this
file distinguishes the three sections above.
