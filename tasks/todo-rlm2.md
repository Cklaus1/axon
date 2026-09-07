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

## Tier 4 — the deciding measurement
- [ ] **Re-measure with the CORRECTED char-literal advice — RAN, but NOT ISOLATING.** Ran 2026-09-07
      against axon `671d7e5`; atlas `37e355a`, artifact
      `measurements/card-gate-corrected.txt` (+ `.raw.txt`).
      - **8/8 first try, stable across all three runs**, in BOTH the card and
        primed-repair arms — 48/48 task-runs, no repair invoked. Prior ceiling
        was 5/8 with the same three tasks failing in all six runs; the named
        cause `c == ' '` had advice that was wrong on first write, corrected in
        `831895a`.
      - Contemporaneous README-primer control: **5/8** (repairing to 6/8), same
        gateway, same day. Both its hard failures are I0002 `mut` — the model
        reverting to Rust with the card out of view. That control rules out the
        gateway or model having drifted into the 8/8 on their own — it says
        nothing about the diagnostics (see the attribution bullet below).
      - The earlier blocker was STALE: the `axon-language-card` worktree still
        had `bin/axon_card.rs`, and the atlas dirt was entirely under `crates/`,
        nothing in `spikes/`. Checking cost less than believing it.
      - **DOES NOT CLOSE O-RLM-11.** The run confounds the diagnostics with FOUR
        card changes that landed after the 5/8 baseline — `1170645` (char
        literals in the card, measured 5/8→7/8 *on its own*), `15d7eea` (the
        card's builtin list was false, "worth 3 tasks"), `7de6450`, `faa326e`.
        `spec-high-security-gates.md` S4 states the violated constraint: a card
        change "must be measured SEPARATELY from S3, or neither result is
        attributable". The README control does not repair this — it is a
        different primer, never carrying the char-literal guidance, so 8-vs-5 is
        card-vs-README, not diagnostics-vs-none. Tightest surviving bound: the
        card alone reached 7/8 on 08-06, *before* the corrected advice existed,
        so the diagnostics work is bounded above by 7→8 and may be worth zero.
      - **The isolating run**, still to do: hold the card fixed at today's text,
        vary only the axon binary — `671d7e5` vs a build predating `831895a`.
        One variable. Cheap; the harness and gateway are both known-good now.
      - **Does not show:** the set is now SATURATED (every arm reads 8/8, so it
        cannot resolve anything above its ceiling); the repair path is untested
        at 8/8 because nothing needed repairing; `differed` is 0/8, so nothing
        here bears on check-then-run. Bar remains needs-human (D6) — reported,
        not decided.

## needs-human
- [ ] Growing `r9::TASKS` for statistical power — would silently re-baseline
      every other engine in the benchmark. Recommendation: don't; run more
      trials. Not adopted.
