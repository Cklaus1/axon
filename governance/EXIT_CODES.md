# Exit-code ledger

**One process, three crates, one number space.** `axon-core` (the interpreter),
`axon-rt` (the native runtime) and `axon-os`/`axon-vm` (the supervisor and the
microVM launcher) all terminate the *same* process, and a supervisor branches on
the number it gets. There was no single place recording who owns which code, so
the only way to pick a free one was to grep three crates and hope — the same
hazard that produced the E13xx collision (Phase-6 effects vs AI-policy) and the
R42/R43 E22xx overlap.

This file is that place. **Claim a code here in the same commit that introduces
it.** Derived from the actual constants, not from memory:
`grep -rn "EXIT_CODE: i32 = " crates/`.

| Code | Constant | Crate | Meaning |
|---|---|---|---|
| 0 | — | — | Success |
| 1 | — | `axon-core/main.rs` | Generic CLI failure (unreadable file, bad args) |
| 2 | — | `axon-core/main.rs` | **Static** failure — parse / type / check diagnostics, and a refused run configuration |
| 3 | `VERIFY_FAILED_EXIT_CODE` | core + rt | `@[verify]` predicate violated at runtime |
| 4 | `HALTED_EXIT_CODE` | core + os | `@[corrigible]` kill-switch latched; a post-halt call fails closed |
| 5 | `AI_POLICY_EXIT_CODE` | core | AI policy refused the call (offline / budget / tier — the E13xx block) |
| 6 | `REFINE_VIOLATION_EXIT_CODE` | core + rt | Refinement-type contract violated (param / return / struct / let) |
| 7 | `GOAL_BUDGET_EXIT_CODE` | core | A kernel `Goal` exhausted its principal's budget (E1604) |
| 8 | `SANDBOX_VIOLATION_EXIT_CODE` | core | `sandbox_run` caught an effect outside the declared ceiling |
| 9 | `RESOURCE_BOUND_EXIT_CODE` | os | A carved resource bound (budget_cap / persist_cap) was exceeded |
| 10 | `COALITION_BOUND_EXIT_CODE` | os | R33 coalition ceiling exceeded |
| **11** | `REPLAY_DIVERGENCE_EXIT_CODE` | core | **A replay is not the run its journal describes** (see below) |
| 12 | `CONTAINMENT_VIOLATION_EXIT_CODE` | os | Monitor detected a containment violation |
| 13 | `QUORUM_BLOCKED_EXIT_CODE` | vm | Quorum not met |
| 14 | `QUORUM_ATTEST_FAIL_EXIT_CODE` | vm | Quorum attestation failed |
| 15 | `CHAIN_VERIFY_FAIL_EXIT_CODE` | vm | Attestation-chain verification failed |
| 101 | `RUNTIME_PANIC_EXIT_CODE` | rt | Panic — a crash, i.e. a bug, distinct from every enforcement code above |

**Next free: 16.** (Avoid 126/127/128+N — the shell uses those for
not-executable, not-found, and killed-by-signal.)

## What a PROGRAM may exit with

The table above is a vocabulary a supervisor trusts. That only works if a program
cannot forge it, so `main`'s return and `exit(n)` are judged differently — the
difference is whether the program *stated* a status or merely *produced a value*.

| | `exit(n)` — stated | falling out of `main` — an answer |
|---|---|---|
| 0, 1, 16..=125 | as written | as written |
| 2..=15, 101 (this table) | **as written** | 1, with the reason on stderr |
| 126..=255 | as written | as written |
| outside 0..=255 | 1, with the reason | 1, with the reason |

**Stating a status is deliberate**, so the ledger vocabulary stays available to
userland: a deploy gate written in Axon says `exit(3)` and means the same "policy
rejection" the `@[verify]` gate means (BUG_HUNT #26/#34 — every deploy-gate
rejection is one exit class). The surface compiler emits exactly that.

126 and up are the shell's by convention (not-executable, not-found,
killed-by-signal-N), and are deliberately NOT reserved here — that is a statement
about how a shell reports its own failures, not a claim this project makes on the
number, and ordinary answers land there.

**A value that falls out of `main` is an answer**, and two things went wrong when
it was passed through unchanged. Both were measured, not imagined:

- *Silent truncation.* `fn main() -> i64 { 3240 }` was observed by the caller as
  **168** — a status is one byte. The number the program produced was not the
  number anyone saw, and nothing said so. Found when a benchmark program computed
  its answer correctly, printed it, returned it, and scored as a failure.
- *Impersonating a guard.* A program whose answer happens to be 6 was
  indistinguishable from a refinement violation, and 11 from a replay divergence.
  Not hypothetical: four checked-in examples returned 2 or 3 as ordinary "blocked"
  signals, and three cases in `exit_code_parity.sh` returned 3, 7 and 10 as
  ordinary values.

Implemented in `interp::returned_exit_status` / `interp::stated_exit_status`, and
mirrored for native in `axon-rt` (`__axon_main_status` / `__axon_exit_status`).
The two engines must agree on the status AND on the sentence they print;
`scripts/exit_code_parity.sh` checks both.

## Why enforcement codes are not 101

Each code above 2 exists so a supervisor can distinguish *"the program is wrong"*
from *"a guard did its job"*. Collapsing them into 101 would make a working
kill-switch indistinguishable from a segfault, which defeats the point of having
the guard: an agent supervisor that cannot tell refusal from breakage cannot
respond correctly to either.

## 11 — replay divergence

`AXON_REPLAY=<journal> axon run p.ax` serves every host effect from a recorded
journal. Exit 11 means the run departed from that journal in one of four ways:

- it called a host method the journal does not have next,
- it called the right method with different arguments,
- it ran past the end of the journal,
- it finished having consumed only *part* of the journal (it did less).

The first divergence point is reported on stderr. Crucially, **the program under
replay cannot suppress this** — `ReplayHost` returns an ordinary `Err` that a
program is free to catch, so the exit code is decided after the program ends,
from state the program never touched. A guard the audited subject can silence is
not a guard, and a clean exit vouching for a transcript of a run that never
happened is worse than having no replay at all.

Owner: `crates/axon-core/src/replay.rs`. Gated by `scripts/replay_host_gate.sh`.
