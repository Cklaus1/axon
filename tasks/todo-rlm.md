# TODO — `AXON_FOR_RLM.md`

A derived view of commit history, not a source of truth. On resume, rebuild
these boxes from `git log`, and per-task attempt counts from
`tasks/attempts.log` (commits cannot record an attempt that failed before
producing one).

## Tier 1 — diagnostics

- [x] **T-R1** — parse-tier help table (§1 + §3 merged) — `c1a14cb`
  - [x] `parse_help.rs`: closed table, pure fn, `None` by default
  - [x] wired into all three production paths (`cmd_check`, `check_pipeline`, LSP)
  - [x] `tests/parse_help_probe.rs` — runs the REAL pipeline, so a parser rewording fails loudly
  - [x] mutation-verified (mut arm, walrus adjacency guard)
  - [x] one unreachable guard found by mutation and deleted rather than kept
- [x] **T-R2** — `run` emits located, structured parse diagnostics (§2 half A) — `bd3916a`
- [x] **T-R3** — `run` stops flattening check-tier diagnostics (§2 half B) ★critical path★ — `bd3916a`
  - [x] extra gate: corpus equivalence, `run` vs `check`, byte-identical, ≥7 comparisons asserted
  - [x] mutation-verified: restoring the flattening call fails the corpus gate on `type-err`
- [x] **T-R4** — E1001's help moves into the `help` field (§2b) — this commit
  - [x] fixed at the shared `push`, reusing `diag_schema::split_help` (one splitter, both paths)
  - [x] mutation-verified: copying instead of moving fails the duplication assertion
- [x] tier-1 full-suite regression vs `tasks/baseline-rlm.md`
- [x] tier-1 smoke: `axon run` on `let mut count` → E0000 + line + help, exit 2

## Tier 2 — the gate measurement

- [x] **T-R5** — LLM-shaped language card + R9 ×3 with spread (D5) — atlas `9138327`
  - [x] `LANGUAGE_CARD` — negates the priors by name, worked example, output contract
  - [x] `bin/axon_card.rs` — three trials, spread not average, contemporaneous README control
  - [x] measurement run and reported

## Pruned — `needs-human` (D6)

- [ ] §4 `--require-contained` — **NOT BUILT.** Gated behind the undefined "usable
      bar" by the spec's own sequencing. Not stubbed and not built behind a
      default-off flag. D3/D4 record how it *would* be built when someone sets
      the bar.
- [ ] §5 accumulating session — **NOT BUILT.** Same gate; and it is four changes
      (session store, accumulation model, tail-execution semantics, CLI), the
      third of which has no meaning until someone decides what a cell is.
