# DRAFT — Finish the RLM fluency measurement

**Status:** DRAFT — not reviewed, not scheduled. Must pass a build-loop Step 1
adversarial review before anyone runs Step 0 against it.
**Source:** `tasks/opportunities.md` O-RLM-07, O-RLM-08, from the 2026-08-06
build-loop over `AXON_FOR_RLM.md`.
**Risk class:** Trivial (measurement + a closed-table extension; no new surface)

## Why

`AXON_FOR_RLM.md`'s sequencing makes the fluency number the **gate** for whether
§4 (`--require-contained`) and §5 (the accumulating session) are worth building.
That gate was measured on 2026-08-06 and came back **5/8, stable across six
runs**, against a same-day README-primer control of 3/8.

But the number is a floor of unknown tightness, because the measurement channel
is broken in a way the run itself exposed: **`repair_prompt` takes no primer**
(`atlas/spikes/rlm-engine/src/axon_engine.rs`), so every repair call is zero-shot
regardless of what the generation arm was given. The model reverts to Rust
between the two calls, and it did so measurably — on the vowel task the *first*
generation wrote `let i = 0` correctly and the *repair* introduced `let mut i`.

So "post-repair +0" is not evidence that better diagnostics do not help repair.
It is evidence that the experiment cannot see whether they do. **A decision about
§4/§5 should not be taken on a ceiling nobody has measured.**

## Items

### A1 — a primed-repair arm (the decisive one)

Add a fourth arm in which the repair prompt carries the same language card the
generation arm got. Implement as a **new function**, not by changing `run_arms`,
so `bin/axon_engine.rs`'s published zero-shot-repair numbers stay comparable —
that comparability is the one property the whole spike cannot survive losing.

Report per D5: three runs, spread shown, never averaged.

**What each outcome means, written down before the run so it cannot be
rationalised after:**

- **primed-repair > 5/8** — the diagnostics work is load-bearing and the ceiling
  is higher than the gate currently shows. Re-open the §4/§5 decision with the
  new number.
- **primed-repair = 5/8** — the remaining failures are not repairable by *any*
  diagnostic, and 5/8 is the real ceiling for this task set. That is a genuine
  answer to the gate, not a null result.

### A2 — the three remaining failure rows

With the card, `mut` is gone from first generation. What fails instead:

| observed | Axon wants | tier |
|---|---|---|
| `or` / `and` | `\|\|` / `&&` | parse |
| `v.max()`, `s.len()` | `str_len(s)`; loop an array with `while` | parse (method syntax) |
| one `unexpected character` | — | **lexer — needs investigation first** |

The first two are more rows in `crates/axon-core/src/parse_help.rs` plus probe
cases, exactly the shape T-R1 established. The third is not yet understood and
must be diagnosed before it is specced — do not write a table row for a message
nobody has reproduced.

### A3 — fold the `diag_differed` instrument fix into the record

Already landed (atlas `bdb8ae0`), listed here so the next reader knows the
metric changed meaning mid-history: before that commit it counted filename
differences as diagnostic differences.

## Open questions (must be resolved by Step 2, not during the build)

1. **Does A1's primed repair still count as R9?** R9's method is "no examples,
   no retries, no error feedback" for first-try, and a *repair* round is already
   outside that. Priming it moves further. Proposal: report it as a clearly
   separate, non-comparable arm — the same treatment the README primer already
   gets — never merged into the R9 column.
2. **Is 8 tasks enough to detect an A1 improvement at all?** One task is 12.5pp.
   If primed repair moves 5/8 → 6/8, three stable runs distinguish that from
   noise; a 1-run 6/8 does not. May need a larger task set, which is a bigger
   change than this spec covers.

## Acceptance

- Three primed-repair runs, spread reported, transcripts committed to
  `atlas/spikes/rlm-engine/results/`.
- A2's first two rows land with probe cases in `parse_help_probe.rs` that run the
  real pipeline (per T-R1's anti-drift rule).
- A written statement of which A1 outcome occurred and what it implies for D6 —
  **without setting the bar**, which remains the human's call.
