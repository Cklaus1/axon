# Build harness — the two RLM follow-up specs (loop 2)

Covers `tasks/draft-rlm-repair-measurement.md` (spec A) and
`tasks/draft-diagnostic-delivery.md` (spec B) as **one loop with one DAG**. They
are run together because they overlap in file scope (`parse_help.rs`, the
diagnostic paths) and because each full-suite run costs ~7 min — two separate
loops would duplicate every baseline and regression for no isolation benefit.

**Resolved placeholders**

| slot | value |
|---|---|
| `<TEST_CMD>` | `cargo test --workspace` |
| `<RUN_CMD>` | `./target/debug/axon` |
| `<FULL_SUITE_BUDGET>` | 6m42s–7m21s (measured, 3 runs last loop) |
| `<RUN_BUDGET>` | unbounded |
| `<REVIEW_MODEL>` | self, adversarial (no `fable` here) |

**Baseline — adopted, not re-run.** `tasks/final-regression.txt`: **1765 pass /
0 fail**, `BASELINE_FAILURES = { }`, at `92dced6`. The only delta from there to
this loop's start is two markdown files (`git diff --stat 92dced6..HEAD`), which
cannot affect a test. Re-running would cost 7 minutes to reproduce a number
whose inputs provably did not change. The empty failure set again makes the gate
exact: any failing test is a new failure.

---

## DAG

```
 TIER 1 — measure first (constraint A2b: any new help content confounds the ceiling)
   U1  A1  primed-repair arm; parameterise run_arms; 3 trials; which tasks, not how many
       └── atlas only. Blocks U5 and U6 by measurement order, not by code.

 TIER 2 — diagnostic delivery (spec B)
   U2t B3  the verb × corpus matrix, written FIRST and failing  ──┐
   U2  B1  convert the 10 callers  ★CRITICAL PATH★               ─┤ U2t is U2's gate
   U3  B2  delete run_check_pipeline (needs ALL 10 converted)   ←──┘
   U4  E2  grep scripts/ + tests/ for `[CODE] message` consumers (precondition of U3)

 TIER 3 — the remaining help rows (must follow U1)
   U5  B4  const/var help at the resolve tier
   U6  A2  `or`/`and` → `||`/`&&` parse_help row
   U7  A2  diagnose the lexer `unexpected character` — INVESTIGATION, may yield
           an opportunity rather than code. No row without a reproduction.
```

**Acyclicity + coverage.** Topological order U1, U2t, U4, U2, U3, U5, U6, U7 —
eight nodes, no cycle. Spec coverage: A1→U1, A2→U6+U7, A2b→the tier boundary,
A3→already landed, B1→U2, B2→U3, B3→U2t, B4→U5, E1/E2→U4. Every item addressed
once. `needs-human`: spec A Open Q2 (grow `r9::TASKS`) — resolved as *do not*,
excluded, run more trials instead.

## Critical path — U2, gated by U2t

U3 cannot happen until all ten conversions land, and U2 is the largest diff. Its
extra gate is not another regression test but the **verb × corpus equivalence
matrix** (U2t): *every verb that type-checks a program must emit the same
diagnostics for it as `check` does*. Written first and failing, so U2 is done
exactly when it passes — no partial conversion satisfies it.

Two properties the matrix must keep from T-R3's version:
- **Ask `check` first, compare only where `check` already fails.** Verbs like
  `deploy`/`test`/`goal` execute; a program that fails type-checking stops at the
  diagnostic stage under all of them, so nothing runs.
- **Assert a minimum comparison count**, or a corpus that matches nothing passes
  vacuously.

## Loops

Inner, outer, meta, regression and smoke as in `tasks/build-loop-rlm.md` §Loops
— unchanged, and not restated. Two additions specific to this loop:

- **Comparability loop (U1 only).** After parameterising `run_arms`, re-run
  `bin/axon_engine`'s arms and diff against the committed transcript in
  `results/`. The claim "the existing binary is unchanged" is a test here, not an
  assertion — that is the whole reason for parameterising rather than
  duplicating.
- **Smoke, this loop:** `axon deploy` on a containment violation must show file,
  line and help (spec B's own acceptance), *and* last loop's `let mut` smoke must
  still pass. Both have concrete signals; the deploy one is red today.

## Stop condition

```
DONE = U1–U7 each DONE or blocked-and-logged
   AND cargo test --workspace shows no NEW failures vs { }
   AND every spec item addressed or needs-human
   AND both smoke scenarios hit their concrete signals
```
