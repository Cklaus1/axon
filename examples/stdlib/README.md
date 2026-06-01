# examples/stdlib — Axon ASI primitives

Small, pure, reusable Axon modules: the building blocks the goal/agent surface
composes. Each runs key-free (`axon test <file>` / `axon run <file>`).

## The ASI-safety quartet

Four orthogonal axes of safe autonomous action — an agent should act only when
all hold at once:

| Module | Axis | Type / key functions |
|---|---|---|
| `budget.ax` | **how much** (bounded resources) | `Budget { used, cap }`; `budget_new/spend/remaining/exhausted/ok/used_pct` |
| `constraint.ax` | **correctness** (hard must-hold rules) | `Constraint { name, satisfied }`; `within/at_most/enforce/both` |
| `principal.ax` | **permission** (capabilities) | `Principal { name, net, fs_write, exec }`; `sandboxed/principal/require_cap` |
| `uncertain.ax` | **confidence** (calibrated belief) | combinators over `Uncertain<i64>`: `u_confident/u_and/u_or/u_max_conf` |

`safe_action.ax` **composes** them: `safe_to_act` folds confidence + budget +
authorization + a hard quality constraint into one act/deny decision, and
`deny_reason` names the first failing axis.

## Oversight (corrigibility)

The quartet gates a *single* action; `supervisor.ax` adds the *temporal* axis.
A `Supervisor { budget, strikes, max_strikes, halted }` watches the stream of an
agent's actions over time: `observe` debits the budget on an approved action and
adds a strike on an unsafe/unaffordable one, tripping a **kill-switch** at
`max_strikes`. The halt **latches** — once tripped, every later action is
refused, even a safe one. This is the overseer an agent cannot out-vote or
reset, the control counterpart to the capability work in `examples/goals/`.

## Scoring helpers

`asi_prelude.ax` — numeric helpers for turning raw model outputs into bounded
scores/confidences: `normalize_score`, `score_to_confidence` /
`confidence_to_score`, `length_score`, `budget_ok/remaining/used_pct`,
`weighted2`/`mean2`, `bound_i64`/`bound_f64`. (`axon-surface` auto-bundles
several of these into goals that reference them — see `examples/goals/`.)

`reward.ax` — composable metric algebra for multi-objective agents.
`Reward { name, score, max }` carries an explicit upper bound so combinators
can normalize across metrics on different scales. `reward_unit` maps to
`[0, 1]`; `reward_blend` does a weighted sum, `reward_scale` re-weights a
single metric, `reward_penalize` subtracts a cost dimension, `reward_min` /
`reward_max` give "must do well on BOTH" / "either axis is fine" composition.
Lets demos drop the magic-number score expressions buried in
`@[adaptive]` fns.

## Run them

```bash
cargo build -p axon-core --no-default-features --bin axon   # the interpreter CLI

axon test examples/stdlib/budget.ax        # 5 tests
axon test examples/stdlib/uncertain.ax     # 5
axon test examples/stdlib/constraint.ax    # 5
axon test examples/stdlib/principal.ax     # 3
axon test examples/stdlib/safe_action.ax   # 5
axon test examples/stdlib/supervisor.ax    # 5
axon test examples/stdlib/arrays.ax        # 5
axon test examples/stdlib/asi_prelude.ax   # 8
axon test examples/stdlib/reward.ax        # 8
axon run  examples/stdlib/safe_action.ax   # the composed gate in action
axon run  examples/stdlib/supervisor.ax    # the kill-switch latching mid-stream
axon run  examples/stdlib/arrays.ax        # argmax over a candidate score set
```

`arrays.ax` adds read-only aggregation/selection over slices (`sum`, `max_of`,
`min_of`, `contains`, `count_at_least`, and `argmax` — the "pick the best of N
candidates" primitive agents use to choose an action).

## Notes

- These are standalone tutorial modules (each has `@[test]`s + a demo `main`).
  To `use` a helper across files instead of inlining it, see
  `examples/modular/` (importable, `main`-free modules + `AXON_PATH`).
- The math builtins `clamp_i64`/`clamp_f64`, `min_f64`/`max_f64`, `sign_i64`,
  `pow_i64` are **now implemented in the interpreter** (full 90/90 builtin
  coverage), so new code can call them directly; `asi_prelude.ax`'s inline
  `bound_*` clamps predate that and are kept for continuity.
