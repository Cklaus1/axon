# Axon Session Status

**Last update**: 2026-05-31
**Branch**: `merge-asi-layer3` (pushed to `origin/merge-asi-layer3`)
**Latest commit**: `f86e6c4` — `safe_self_improve` flagship demo composing
the full ASI stack (see "Shipped this session" below).

Snapshot of current state. Companion to `ROADMAP.md` (forward plan),
`STATUS.md` (Phase-4 shipped state), `BUILD_DIAGNOSIS.md` and
`CODEGEN_WRAPPER_PROTOTYPE.md` (native-build investigation).

---

## The headline: interpreter-first, ASI surface complete enough to compose

The native LLVM/inkwell `codegen` build of `axon-core` still **doesn't
finish** (historically 5h+; the `#[inline(never)]` wrapper fix is applied
but is a constant-factor reduction, not asymptotic — see
`BUILD_DIAGNOSIS.md`). The interpreter is the active execution path.

But the *language surface* shipped through the interpreter is now wide
and consistent enough that an ASI program can be written entirely in
pure Axon, with safety primitives, an optimizer, persistence, and a
17-builtin goal-introspection suite — all composable. The `safe_self_improve`
flagship demo (`examples/asi/safe_self_improve.ax`) ties the whole stack
together: `mod`-imports a Tier-1 Agent, runs the multi-arg optimizer over
an action catalog, gates each step through the safety quartet, and
demonstrates a latching kill-switch.

## Shipped this session (long form below — see commit log for the full record)

### Language / interpreter

- **Tuples** end-to-end: `(a, b, …)` literals, `t.0` / nested `t.0.1` access
  (parser splits Float-lexed `0.1`), `let (a, b) = e` destructuring with
  stmt-level splicing into the enclosing scope (so destructuring works
  inside `while` / `for` / `if` / `while let` bodies, not just blocks),
  `(a, b)` patterns in `match`.
- **Place assignment**: `xs[i] = v` and `s.field = v`, including nested
  `o.inner.value = 42` and `s.xs[1] = 99`.
- **Typed bindings**: `let x: T = e` / `own` / `ref` enforce the annotation.
- **Closure ergonomics**: `|x: i64| -> i64 { body }` — explicit return-type
  annotation now parsed (discarded; HM still infers).
- **`as` cast operator** (ROADMAP F8): `expr as Type` is parser sugar for
  the polymorphic `as_<type>` builtins; precedence higher than arithmetic
  (matches Rust). Replaces `f64_to_i64(x)` / `i64_to_f64(x)` at call sites.
- **Raw strings** (F16): `r"…"` — no escape processing, no `{}` interpolation.
- **Composite `@[verify]` predicates** (F6): the interpreter binds
  `value` / `confidence` / `source_tag` into a fresh env and evaluates the
  predicate Expr at runtime. `@[verify(value >= 50 && confidence >= 0.8)]`,
  OR predicates, function calls in the predicate all work.
- **`@[verify]` panic enrichment**: panic messages name the breaching
  ident, both `value` and `confidence`, and the goal-search input.
- **Channels**: added `c.try_recv() -> Option<T>` (non-blocking) and
  `c.len() -> i64` (queue depth) for drain-pattern loops.
- **Robustness preserved**: `RECURSION_LIMIT`, `MAX_EXPR_DEPTH`, 1 GiB
  worker-thread stack, graceful panics on adversarial input.

### Optimizer (the goal_* suite)

- **Multi-arg `@[adaptive]`** (F1 numeric, F9 closed) via coordinate
  descent over `fn(i64, …) -> i64` AND `fn(f64, …) -> f64`. Powell's-
  method-style joint-direction step after each sweep collapses the
  constant-factor convergence rate of cyclic CD on correlated dims —
  linear-regression weights now converge to machine precision in 3000
  evals (vs ~1e-3 without Powell).
- **Wide-step seed**: from a fresh `cur = [0; N]`, step starts at
  `max_evals * 4` so the first probes leap across the plausible range
  and the halving cascade binary-searches toward the peak.
- **Early exit** on exact target hit (saves the trailing evals).
- **Fair per-dim rotation**: caps each dim's evals per sweep at
  `max_evals / n_dims` so the f64 path doesn't let one dim monopolize
  via its tight resolution floor.
- **Provenance store** captures `(input_tuple, score)` per call — i64
  AND f64 prefixes recorded in parallel stores.

### Goal-introspection builtins (17 in the goal_* namespace)

| Builtin | Shape | Purpose |
|---|---|---|
| `goal_run(name, target, max_evals)` | mutator | live hill-climb |
| `goal_continue(name, target, max_evals)` | mutator | warm-start from prior best |
| `goal_clear(name)` | mutator | drop provenance for fresh experiment |
| `goal_best_score(name, target)` | pure read | best observed (no mutation) |
| `goal_count(name)` | pure read | O(1) trace size |
| `goal_best_input(name, target)` | pure read | winning probe (first dim) |
| `goal_best_inputs(name, target)` | pure read | full winning tuple (i64) |
| `goal_best_inputs_f64(name, target)` | pure read | full winning tuple (f64) |
| `goal_history(name)` | pure read | `[(i64, f64)]` trace |

### Stdlib (39 array + 3 string + 2 cast primitives shipped this session)

- Constructors: `arr_range`, `arr_push`, `arr_repeat`, `arr_concat`, `arr_flatten`, `arr_chunk` (6)
- Reductions: `arr_sum` / `arr_max` / `arr_min` / `arr_mean` × {i64, f64} + `arr_std_f64` (9)
- Index-returning: `arr_argmax` / `arr_argmin` × {i64, f64}, `arr_index_of` (5)
- Higher-order: `arr_map`, `arr_filter`, `arr_fold`, `arr_sort_by`, `arr_zip`, `arr_contains`, `arr_find` (7)
- Slicing/reordering: `arr_reverse`, `arr_take`, `arr_drop`, `arr_unique` (4)
- Polymorphic via deferred type params (`T`, `U`) so `arr_map([1,2], |x| i64_to_f64(x))` type-checks.
- Strings: `str_split`, `str_join`, `str_digits_only`
- Casts: `as_f64`, `as_i64` (polymorphic; also the parser-sugar `expr as T`)

### Userland stdlib (closes F10 / F12 userland)

- **`examples/stdlib/reward.ax`** — composable metric algebra. `Reward { name, score, max }` with
  `reward_unit / blend / scale / penalize / min / max`. 8 tests.
- **`examples/stdlib/agent.ax`** — Tier-1 Agent state machine. Bundles `Principal` +
  budget + supervisor + history counters; `agent_step` evaluates the full safety
  quartet (`halted → unauthorized → over-budget → under-confident → under-quality`)
  and the kill-switch latches at `max_strikes`. 7 tests.

### ASI demos (18 in `examples/asi/`)

- #14 `learn_linear.ax` — fit `y = a·x + b` over i64 weights via multi-arg `@[adaptive]`.
- #15 `persistent_learner.ax` — cross-process self-improvement (read_file + write_file).
- #16 `learn_linear_f64.ax` — continuous-domain regression; recovers `(0.5, 1.25)` to f64 epsilon.
- #17 `multi_objective.ax` — accuracy-vs-cost Pareto search using `reward.ax`.
- #18 `safe_self_improve.ax` — **FLAGSHIP**. `mod`-imports `stdlib/agent.ax`,
  uses `goal_run` over an 8-action catalog, gates each step through `agent_step`,
  demonstrates two-strike kill-switch latching.

### ROADMAP §9.5 friction items (9 of 15 closed this session)

| # | Title | Status |
|---|---|---|
| F1 | `goal_run` only single-arg `i64` | ✅ for i64^N and f64^N |
| F2 | `ai_complete` replay | open (Phase 9) |
| F3 | Provenance shape | open (Phase 7+9) |
| F4 | Budget meter (tokens) | open (Phase 7) |
| F5 | Runtime sandbox | open (Phase 9) |
| F6 | `@[verify]` predicate language | ✅ for single-Uncertain composite |
| F7 | `str_digits_only` | ✅ |
| F8 | `as` cast | ✅ |
| F9 | Multi-arg `@[adaptive]` | ✅ |
| F10 | Reward shaping | ✅ userland (`reward.ax`) |
| F11 | `@[adaptive]` logs inputs | ✅ (interp; native ABI ext. = Phase 7) |
| F12 | `Agent` type | ✅ userland (`agent.ax`) |
| F13 | Structured-prose surface depth | open (Phase 10) |
| F14-15 | Web UI / pipeline | open (Phase 11/12) |
| F16 | Raw string literals | ✅ |

## What works now (via the interpreter, no LLVM)

| Command | Behavior |
|---|---|
| `axon run f.ax` | parse → resolve → infer → check → borrow → cap → verify → interp; exit code forwarded |
| `axon check f.ax` | static pipeline only |
| `axon test f.ax` | run `@[test]` fns in-process (honors `should_fail`) |
| `axon goal g.md` | prose `goal.md` → `.ax` (axon-surface) → check → run; `--iterate N` for autonomous convergence |
| `axon build f.ax` | native AOT — cfg(codegen) stub unless built `--features codegen` (the slow path) |
| `axon parse f.ax` / `axon lsp` | need `--features serde-json` |

- All `examples/*.ax` (deterministic) + 18 ASI demos run cleanly via the interpreter.
- LLM-using demos (optimize / classify / summarize / pricing / search_rank / code_review)
  panic without `AXON_AI_MOCK=1` or `--features asi-runtime` — that's intentional.
- **Tests green**: 517 across the workspace (15 + 286 lib + 83 cli_run + 101 integration
  + 23 + 10 + … ); `axon-core` clippy clean.

## Known language gaps (the still-open list)

- **Concurrency is cooperative (single-threaded).** `spawn`/`chan`/`send`/`recv`
  run in the interpreter. `spawn` bodies execute eagerly, so fan-out/collect
  works; request/response (block-on-later-send) doesn't. `try_recv` and `len`
  added this session so consumers can drain non-blockingly.
- **No HashMap / Set** — `arr_unique` is O(n²) until a hash primitive ships.
- **No `as` for composite types** — `expr as f64` works; `expr as Result<…>`
  is rejected (it would have to dispatch on the source shape, not a primitive
  conversion).
- **Module system has no qualified-name access** — `use mod.{items}` imports
  individually; there's no `mod::item` syntax.

## ASI builtins in the interpreter

- **Deterministic** (no external crate): the 17 `goal_*` introspection builtins,
  `@[adaptive]` logging (with i64 + f64 input prefixes), `Uncertain`/`Temporal`
  constructors (`uncertain_dyn_i64` / `_f64`, `temporal_new`), the
  `@[verify(predicate)]` runtime gate (simple shape + composite-predicate eval).
- **Live LLM** (`--features asi-runtime` or `AXON_AI_MOCK=1`):
  `ai_complete` / `ai_extract_uncertain_{i64,f64}` → `axon-ai` (reqwest).

## Phase-10 surface (`axon goal`)

`axon-surface` (`GoalFile::parse` + `compile::emit`) lifts ```axon` blocks
from the goal `.md` and scaffolds `TODO:` placeholders for absent helpers.
`examples/goals/learn-goal.md` runs autonomously with `axon goal --iterate
6` and converges to score=200 at input=12 via cross-run continuation
(`AXON_GOAL_CONTINUE=1`).

## Open / next (post-this-session)

The remaining ROADMAP §9.5 items (F2 replay, F3 provenance shape, F4 budget
meter, F5 sandbox, F13-15 surface/UI/pipeline) are all real Phase 7+ work
needing new compiler phases or runtime infrastructure beyond what the
interpreter can ship in single ticks. They open up when the corresponding
phase spec lands.

In-language opportunities still on the table:
1. HashMap / Set primitive (O(1) `arr_unique`, dict-shaped state).
2. Cross-file `mod` polish — qualified access, re-exports.
3. Native build validation on a beefy machine, once a CI invocation is
   feasible (the wrapper fix is applied; the constant-factor win is
   measured but the multi-hour build still needs CI scheduling).
4. Powell's-method generalization to longer line searches with a
   bracketing step instead of pure geometric doubling.
