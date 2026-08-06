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
generation arm got.

> **[REVISED: implementation inverted. The draft said "a new function, not by
> changing `run_arms`" — that is a directive to duplicate a ~60-line function,
> which is precisely the defect T54 fixed at the start of this session.]**
>
> Two copies of a measurement loop drift, and when they drift the numbers stop
> being comparable — the one property this spike cannot survive losing, which is
> the very thing the duplication was meant to protect.
>
> Instead: **add a `repair_primer: &str` parameter to `run_arms`** and have
> `bin/axon_engine.rs` pass `""`. Comparability is then not a matter of trust —
> it is provable, because the existing binary's output must be byte-identical
> before and after, and that is checkable by diffing a re-run against the
> committed transcript in `results/`. One loop, one set of semantics, and the
> no-change claim is a test rather than an assertion.

Report per D5: three runs, spread shown, never averaged. **Record which tasks
pass, not only how many** — 5/8 twice with a different 5 is a different result
from 5/8 twice with the same 5, and the count cannot distinguish them.

**What each outcome means, written down before the run so it cannot be
rationalised after:**

- **primed-repair > 5/8** — the diagnostics work is load-bearing and the ceiling
  is higher than the gate currently shows. Re-open the §4/§5 decision with the
  new number.
- **primed-repair = 5/8** — [REVISED: the draft said this would mean the
  failures are "not repairable by *any* diagnostic". That is an overclaim the
  experiment cannot support.] It means they are not repairable by **this card
  plus this diagnostic in one round**. A different diagnostic, a second round,
  or a larger card are all untested. Report it as a ceiling *for this
  configuration*, which is still a real answer to the gate.

### A2 — the remaining failure rows

> **[REVISED: two of the three rows are wrong. The table was written from what
> the model emitted, without probing what the compiler does with it — the exact
> mistake T-R1's review caught and this draft repeated.]**

Probed against `axon 0.1.0 (f16626b)`:

| observed | actual tier | verdict |
|---|---|---|
| `or` / `and` | **E0000**, parse | ✅ a real `parse_help` row |
| `v.max()`, `s.len()` | **E0403**, unknown-method | ❌ **row deleted — already solved** |
| one `unexpected character` | lexer | ⚠️ still needs diagnosis first |

**The method-syntax row is deleted, and its deletion is a finding.** `s.len()`
yields ``E0403 no method `len` on type `str` `` *with a populated `help` field*,
through **both** `check` and `run` — verified. That is the unknown-method tier
`AXON_FOR_RLM.md` §1 already credits as working. So there is nothing to build.

What that reframes: the model failed the array task **while being shown correct,
well-formed help**. The help is not the missing ingredient there — the unprimed
repair round (A1) is. This is direct evidence for A1's priority and against
adding more help rows before measuring it.

So A2 reduces to **one row** (`or`/`and` → `||`/`&&`) plus **one investigation**
(the lexer rejection — diagnose before speccing; do not write a table row for a
message nobody has reproduced).

### A2b — sequencing constraint discovered by review

**A1 must be measured BEFORE A2's row or B4 lands.** Both change what the model
is shown, so landing them first confounds the ceiling measurement with new help
content, and the run could not attribute a move to either cause. A1 first, then
the rows, then re-measure if desired.

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
   noise; a 1-run 6/8 does not.

   **[RESOLVED — `needs-human`, and therefore OUT of this build.]** The only fix
   is a larger task set, and `r9::TASKS` is shared by every engine in the
   benchmark: growing it silently re-baselines Rhai, Lua, CPython and bash, and
   the numbers already published against the old set stop meaning anything.
   That is not an engineering call about this spec — it is a decision about
   whether the benchmark keeps its identity, and it belongs to whoever owns the
   comparison. Recommendation: **do not grow `TASKS`.** If more statistical
   power is wanted, run more *trials* of the same eight, which costs only model
   calls and breaks nothing. This build runs three trials and reports the
   spread, exactly as D5 requires, and states the resolution limit rather than
   working around it.

## Acceptance

- Three primed-repair runs, spread reported, transcripts committed to
  `atlas/spikes/rlm-engine/results/`.
- A2's first two rows land with probe cases in `parse_help_probe.rs` that run the
  real pipeline (per T-R1's anti-drift rule).
- A written statement of which A1 outcome occurred and what it implies for D6 —
  **without setting the bar**, which remains the human's call.
