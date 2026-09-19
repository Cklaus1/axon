# Cortex Phase 3 — attribution, and an inconclusive transfer test

Two experiments. One produced a clean result; the other **failed to produce a
measurement at all**, and saying so is the point of this file.

---

## 3a — Component attribution (280 episodes, 8.6 min)

Phase 2's ON arm bundled two things into attempt N+1: the previous attempt AND
the compiler's diagnostic. Four arms separate them. Every arm pays the same tool
cost; they differ only in what the model is SHOWN.

| arm | what attempt N+1 sees | repaired | vs blind | 95% CI (over tasks) |
|---|---|---|---|---|
| `blind` | the same prompt again | 39/70 (56%) | — | control |
| `prev` | + its previous attempt, no diagnostic | 35/70 (50%) | **−5.7pp** | [−14.3, +1.4] |
| `diag` | + the diagnostic, WITHOUT the attempt | 45/70 (64%) | **+8.6pp** | [+1.4, +20.0] |
| `both` | + attempt and diagnostic | 55/70 (79%) | **+22.9pp** | [+0.0, +51.4] |

**`prev` is the control that matters** and it earns its place: it adds context of
the same shape and comparable length as `diag`, but carries no information the
model did not already produce. It does **not** help — it trends slightly
negative. So the Phase 2 effect is **not "more context"**.

**The diagnostic carries the information, but needs the attempt to be
actionable.** `diag` alone gives +8.6pp; `both` gives +22.9pp. The clearest case
is `dict_method_syntax`: 0% for blind, prev AND diag — and **100%** for both. A
diagnostic about code the model cannot see is far weaker than the same
diagnostic next to the code it refers to.

Per-task:

| task | blind | prev | diag | both |
|---|---|---|---|---|
| dict_method_syntax | 0% | 0% | 0% | **100%** |
| str_field_len | 50% | 30% | 90% | 100% |
| string_interp | 50% | 30% | 60% | 40% |
| option_match | 0% | 0% | 0% | 10% |
| python_colon_block | 90% | 100% | 100% | 100% |
| enum_variant_called | 100% | 90% | 100% | 100% |
| result_propagate | 100% | 100% | 100% | 100% |

Sign tests remain non-significant at n=7 tasks (`diag` p=0.125, `both` p=0.188).
**Task count, not episode count, is the binding constraint on significance.**

---

## 3b — Transfer to Python: INCONCLUSIVE, not negative

The alternative explanation for Phase 2 is unflattering and plausible: the gain
may be nothing more than *"show a model a compiler error for a language it has
never seen"*. That would not transfer.

7 Python repair tasks, same design, 140 episodes:

| arm | repaired | attempts/ep |
|---|---|---|
| OFF | 70/70 (**100%**) | 1.01 |
| ON | 70/70 (**100%**) | 1.00 |

**This measures nothing.** The control is at the ceiling and solves every task
on the first attempt, so there is no headroom for feedback to help — the ON arm
never even reaches a second attempt. It is the same ceiling failure as Phase 1,
reproduced in a language the model knows.

The correct reading is **"the experiment could not detect transfer"**, NOT
*"the effect does not transfer"*. Those are different claims and only the first
is supported.

**What answering it actually requires:** Python tasks hard enough to put the
control at 40–70%. Single-function repair with an obvious intent is nowhere near
that for this model. Candidates: multi-file changes, tasks whose failure is a
silent wrong answer rather than a traceback, or bugs whose diagnosis needs
behaviour the error message does not name.

---

## Standing after three phases

**Supported:** grounded iteration beats blind iteration on Axon tasks
(+27pp Phase 2, +22.9pp replicated in 3a), and the effect is attributable to the
DIAGNOSTIC rather than to added context, with the previous attempt acting as a
necessary carrier rather than the cause.

**Not supported:** that this generalises beyond a language the model does not
know. The one experiment aimed at that question returned no signal because it
was mis-designed, and it has not been re-run.

**Known harm:** one Axon task is reliably worse under the full treatment
(`string_interp`, 50% → 40% here; `enum_variant_called` −7pp in Phase 2), and
Phase 1's `count_skips_last` showed the mechanism — added deliberation
displacing a simple correct edit.
