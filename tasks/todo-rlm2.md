# TODO — loop 2 (the two RLM follow-up specs)

Derived from commit history; attempt counts live in `tasks/attempts.log`.

## Tier 1 — measure first (A2b)
- [x] **U1** A1 — primed-repair arm; `run_arms` parameterised, old bin proved
      byte-identical by a test pinning the literal — atlas `35ac5f4`
      - Result: 5/8 stable, identical to unprimed. Same three tasks fail in all
        six runs → structural, not noise. Cause identified: `c == ' '`.

## Tier 2 — diagnostic delivery (spec B)
- [x] **U2t** B3 — verb × corpus equivalence matrix, written first and failing
- [x] **U4** E2 — no consumer of the flattened `[CODE] message` form (grep clean)
- [x] **U2a** — `suggest` was nondeterministic across processes (found BY U2t)
- [x] **U2** B1 — all 10 callers converted — `831895a`
- [x] **U3** B2 — `run_check_pipeline` deleted — `831895a`
- [x] **U5** B4 — `const`/`var` help at the resolve tier — `8363eb9`
      - plus the `lib::check_pipeline` / `run_check_pipeline_located` drift

## Tier 3 — remaining rows (after U1)
- [x] **U6** A2 — `or`/`and` row + char-literal row + lexer location — `6b89557`
      - char-literal advice was WRONG on first write; corrected in `831895a`
- [x] **U7** A2 — the lexer `unexpected character` diagnosed: single-quoted
      character literals. Became U6's row rather than a separate task.

## Not done, and why
- [ ] **Re-measure with the CORRECTED char-literal advice.** The atlas working
      tree was switched to `main` by another session mid-run, taking
      `bin/axon_card.rs` out of the tree. My commits are safe and pushed
      (`35ac5f4`); the tree is dirty with another session's work, so checking my
      branch back out would disturb work this run did not create. Logged as
      O-RLM-11. **This is the one measurement that decides whether the
      diagnostics work pays.**

## needs-human
- [ ] Growing `r9::TASKS` for statistical power — would silently re-baseline
      every other engine in the benchmark. Recommendation: don't; run more
      trials. Not adopted.
