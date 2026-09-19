# Cortex Phase 2 — signal check

**Question:** does grounded iteration change measurable task outcomes, at equal
attempt budget?

**Budgets:** 200 episodes / 480 min. **Consumed: 200 episodes, ~7 min.**
Episode budget bound.

**Setup:** `Qwen/Qwen2.5-Coder-7B-Instruct`, bf16, local, **temperature 0.7 /
top-p 0.95** with recorded seeds, 400 max new tokens, **3 attempts per episode
in BOTH arms**, 7 tasks, paired.

**Arms — one difference only:**

- `OFF` (blind iteration): attempt N+1 sees the same prompt as attempt N.
- `ON` (grounded iteration): attempt N+1 additionally sees its previous attempt
  and the compiler's real diagnostic on it.

Attempt count, model, tasks, tools, decoding and token limit are identical, so
this measures the INFORMATION, not extra compute.

## Integrity — PASS

Including the check Phase 1 failed: **repeats are now genuine samples.** 12 of
14 (task, arm) groups produced more than one distinct patch, averaging 4.6
distinct patches per group. Under Phase 1's greedy decoding this was 1.

## Task set — calibrated, not assumed

The Phase-1 task class was **saturated**: 11 of 12 candidate bug-repair tasks
had a control pass-rate of 0.88–1.00, and most produced a single distinct reply
even at temperature. A saturated class measures the class, not the treatment.

The class that discriminates is **Axon-specific constructs the model cannot have
memorised** — it invents `!i64`, `effect net io`, `CapabilitySandbox::new`, none
of which exist. **6 of 13 candidates were dropped as not actually broken**
(Axon permits reassignment without `mut`; it *has* `for x in xs`; an
"add this attribute" task cannot be judged by a hidden test that passes without
it). The 7 survivors each fail with a distinct diagnostic: E0403, E0401, E0404,
E0102 ×2, E0001, and a parse error.

## Result

| arm | n | repaired | attempts/ep | prompt tok/ep | completion tok/ep | gen s/ep |
|---|---|---|---|---|---|---|
| OFF | 100 | 51 (51%) | 2.20 | 252 | 89 | 1.76 |
| ON | 100 | **78 (78%)** | 2.03 | 409 | 86 | 1.71 |

Per-task, paired:

| task | OFF | ON | delta |
|---|---|---|---|
| dict_method_syntax | 0% | 87% | **+87pp** |
| str_field_len | 40% | 100% | **+60pp** |
| string_interp | 29% | 50% | +21pp |
| option_match | 0% | 14% | +14pp |
| python_colon_block | 93% | 100% | +7pp |
| result_propagate | 100% | 100% | 0 |
| enum_variant_called | 100% | 93% | **−7pp** |

Per-task mean delta **+26.1pp**, bootstrap 95% CI over tasks **[+5.1, +50.8]**.
Sign test: 5 of 6 changed tasks improved, **p = 0.109** — not significant at
n=7 tasks.

## What this does and does not support

**Supported:** on this task class, grounded iteration raises the completion rate
substantially, and the bootstrap CI excludes zero. The effect is *mechanistic*,
not mysterious — it is concentrated exactly where the model holds a confident
wrong prior about Axon (`d.get(k)`, `s.length`) and the compiler contradicts it
by name. `invalid_program` failures fall from 49 to 20.

**Not supported:** any general claim. Seven tasks is a small set, the sign test
is p=0.109, and over half the aggregate effect comes from two tasks. One task
got **worse**, which is the predicted failure mode and matches Phase 1's
`count_skips_last`.

**Cost, measured not assumed:** ON costs **+62% prompt tokens** (409 vs 252) and
one extra tool call per episode. Completion tokens and generation wall-clock are
flat, and ON uses slightly *fewer* attempts (2.03 vs 2.20) — it converges sooner
when it converges.

## Phase 3 must establish

- **Transfer.** All 7 tasks are Axon-syntax tasks from one repo. The obvious
  alternative explanation is that this measures "showing the model a compiler
  error for a language it does not know", which would not transfer to a language
  it does know. Phase 3 must test a repo and language the model has seen.
- **Component attribution.** The ON arm bundles the previous attempt AND the
  diagnostic. Which one carries the effect is unknown and is a one-variable
  ablation.
- **More tasks.** n=7 is the binding constraint on significance, not episodes —
  200 episodes over 7 tasks cannot beat p=0.109.
