# Flight plan — `AXON_FOR_RLM.md`

**Mission.** Make Axon's compiler teach a model that is writing Axon badly — put
`help` on the parse tier where 100% of the measured failures land, stop `axon run`
throwing that help (and the location) away, then measure whether it moved R9.

**Engineering decisions (Step 2), all canon**

- D1 parse help — free-form per site / LLM-generated / **closed table, pure fn of (seen token, source at offset)** → chosen: deterministic, offline, one row per habit.
- D2 §2 fix — add a second path / **delete the lossy `run_check_pipeline` wrapper** → chosen: two functions differing only in what they discard is how this bug happened.
- D3 `--require-contained` scope — check-only / **check + run + build** → chosen: a check-only gate is bypassed by calling `run`, the verb an RLM host actually uses. *(Records the decision; §4 is not built — see below.)*
- D4 containment default — file-level opt-in / **per-function, including `main`** → chosen: opt-in would let one helper's annotation silently widen every other function.
- D5 gate measurement — one run / **three runs, spread reported** → chosen: 8 tasks × 1 shot, one task = 12.5pp; the spec's own Lua citation is three runs.
- ⚠ close call: **T-R4 runs sequentially, not dispatched in parallel** — its files are provably disjoint from the T-R1→T-R3 chain, so it qualifies, but a 3-deep chain does not repay the dispatch overhead.

**needs-human — excluded from this run (D6)**

The "usable bar" that §5's sequencing gates §4 and §5 behind is never given a
number, and setting it is a product judgement, not an engineering one. Pruned
subtree: **2 of 7 spec sections** — §4 `--require-contained` and §5 the
accumulating session, the two largest. Neither is stubbed and neither is built
behind a default-off flag. Surfaced for the human: §4 has standalone value to
Axon-as-a-language that does not depend on the RLM verdict at all.

**Step 1 `[REVISED]` markers** — §2 (mechanism corrected, scope widened: `run`
loses `file`/`line`/`col` too, and its parse tier emits no JSON at all) · §3
(merged into §1 — same edit, same function) · §4 (recommendation upheld, 3
under-specifications resolved) · §5 (confirmed; it is four changes, left gated) ·
sequencing (adopted as the DAG; gate is executable here, n=8 needs repeats) ·
**§2b added** — E1001 puts its help inside `message`, so the one diagnostic §4
exists to produce is the one whose help a consumer cannot read.

**Critical path — T-R3**, `run` stops flattening diagnostics. Everything else's
value is invisible without it. Extra gate, beyond regression tests: **`axon run`
and `axon check` must emit byte-identical diagnostic JSON for the same program**
— a round-trip equivalence proof over a corpus, which no partial fix satisfies.

**Shape.** 5 tasks · 2 tiers · longest chain 3 (T-R1→T-R2→T-R3) · 1 parallel
track (T-R4) · `cargo test --workspace` · baseline **1744 pass / 0 fail**, so the
gate is exact · budget unbounded · full-suite run 7m21s · two repos (build in
`axon`, measure in `atlas/spikes/rlm-engine`).

**First three tasks**

1. **T-R1** — `parse_help.rs`: closed table keyed on the token seen, `mut` first (the measured case), then `const`/`var`/`def`/`function`/`;`/`:=`/`->`. Wired into `cmd_check`, `check_pipeline`, `lsp`.
2. **T-R2** — `cmd_run` parses with `parse_source_located` and emits a structured `E0000` with line/col/help instead of bare prose.
3. **T-R3** — `cmd_run` checks with `run_check_pipeline_located`; delete the flattening wrapper; prove run/check JSON equivalence.

Smoke is red at the start by construction: `axon run` on `let mut count` emits
no `E0000` JSON, no `help`, no line — all three greps miss today.
