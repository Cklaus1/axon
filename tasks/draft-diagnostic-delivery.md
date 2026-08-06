# DRAFT — Diagnostic delivery completeness

**Status:** DRAFT — not reviewed, not scheduled. Must pass a build-loop Step 1
adversarial review before anyone runs Step 0 against it.
**Source:** `tasks/opportunities.md` O-RLM-01, O-RLM-02, from the 2026-08-06
build-loop over `AXON_FOR_RLM.md`.
**Risk class:** Standard (touches ten CLI verbs; no new public surface)

## Why

The RLM build fixed `axon run`: it now emits the same structured, located,
help-carrying diagnostics `axon check` does. That fix was scoped to one verb
because that is the verb the spec measured.

`run_check_pipeline` — whose entire body is `format!("[{code}] {message}")` over
typed diagnostics, because it passes `""` as the source so no span can resolve —
has **eleven** callers. Ten remain:

```
crates/axon-core/src/main.rs:2354, 2447, 2526, 3032, 3136, 3974, 4519, 5014, 5485, 5936
```

covering `test`, `deploy`, `ast review`, `doc`, `redteam` and others. Each drops
`help`, `file`, `line`, `col`, `expected` and `found` and then re-derives JSON
from the flattened string by regex. So `axon test` and `axon deploy` report
diagnostics with no location, and the containment refusal (E1001) that `deploy`
exists to surface arrives without its help — the exact defect §2b fixed for
`check`/`run`.

Decision D2 of the original build assumed this wrapper had one caller and could
simply be deleted. That was wrong, and it is why this is a separate spec rather
than scope creep in the last one.

## Items

### B1 — convert the remaining ten callers

Each becomes `run_check_pipeline_located(program, &src, &path)` plus
`emit_pipeline_diag`, the two helpers `cmd_run`/`cmd_check` already share. Not
mechanical in one respect: several callers **consume the strings**, not just
print them (`ast review`'s JSON output, `deploy`'s gate reporting), so each site
needs reading before conversion.

### B2 — delete `run_check_pipeline` once callerless

This is what D2 originally intended and could not have. Two functions differing
only in how much they discard is how the defect arose; once nothing calls the
lossy one, it goes. If a caller genuinely wants strings, it converts at its own
call site.

### B3 — generalise the equivalence gate to a verb × corpus matrix

T-R3's `run_and_check_emit_identical_diagnostics_across_a_corpus` asserts `run`
and `check` agree byte-for-byte over every diagnostic-producing program in
`examples/` plus hand-written cases. Extend the same test to every converted
verb: **every verb that type-checks a program must emit the same diagnostics for
it.** That is one assertion covering all ten conversions, and no partial
conversion satisfies it.

Keep the existing minimum-comparison-count assertion, so a corpus that silently
matches nothing cannot pass vacuously.

### B4 — help at the resolve tier (`const`, `var`)

`AXON_FOR_RLM.md` §1 names both. Probing showed they lex as ordinary identifiers
and fail at name **resolution** (`cannot find name \`const\` in this scope`), so
`parse_help` is never called for them and cannot be. Same fix one tier down: a
help row on the unresolved-name diagnostic when the name is a known foreign
keyword. Currently pinned as a negative test
(`parse_help_probe.rs::const_and_var_do_not_reach_the_parse_tier`) so the tier
fact is not re-discovered — that test must be updated, not deleted, when this
lands.

## Open questions (resolve at Step 2)

1. **Do any of the ten verbs deliberately want terse output?** `axon fmt` and
   `axon doc` are not compiler-diagnostic surfaces in the same sense. Check each
   before assuming uniformity is desirable — the goal is that no verb *silently
   discards* information, not that every verb prints identically.
2. **Is the `[CODE] message` string format load-bearing anywhere?** Any consumer
   parsing it (a script, a test asserting on it) breaks. B3's matrix will catch
   test consumers; external scripts it will not. Grep `scripts/` before B2.
3. **Does B4 belong here or with the RLM measurement spec?** It is a diagnostics
   change, but its *evidence* is the fluency number. Proposal: build it here,
   measure it there.

## Acceptance

- Ten callers converted; `run_check_pipeline` deleted.
- The verb × corpus matrix passes, with a minimum-count assertion.
- `cargo test --workspace` shows no new failures against the then-current
  baseline (`tasks/baseline-rlm.md` records the method; re-capture, do not reuse
  the numbers).
- One worked example in the commit: `axon deploy` on a containment violation now
  showing file, line and help.
