# Axon Session Status

**Last update**: 2026-06-10
**Branch**: `merge-asi-layer3` (pushed to `origin/merge-asi-layer3`)
**Latest commit**: `baebc92` — merge remote-tracking branch; latest feature commit
`044b73e` Layer-3 DSL widening (fold-logical rule kind, red-teamed).

Snapshot of current state. Companion to `ROADMAP.md` (forward plan),
`STATUS.md` (Phase-4 shipped state), `BUILD_RESOLVED.md` (native build
root-cause and resolution).

---

## The headline: native build solved; full stack through Phase 8 + Layer-3 DSL

The native LLVM/inkwell `codegen` build of `axon-core` **finishes in ~3–4
seconds**. The long-standing stall (historically 5h+) was a `serde-json` ×
`codegen` default-feature collision — recursive AST `Serialize`/`Deserialize`
derives drove rustc's monomorphizer into exponential type-relation work. Fixed
by dropping `serde-json` from `default` (see `BUILD_RESOLVED.md`). `cargo
build -p axon-core` produces a working native `axon` binary; `axon build
foo.ax` emits a real native binary. Do **not** enable `codegen + serde-json`
together.

The interpreter remains the primary execution path for `run`/`test`/`goal`/
`check` (codegen-free, sub-second). Native and interpreter outputs are
byte-identical on all `examples/` under `AXON_AI_MOCK=1`.

Beyond the build fix, the following major milestones have landed since the
prior session snapshot:

- **Phase 5 SMT discharge** wired into the DEFAULT pipeline (proven
  for-all-inputs refinement/verify checks are statically elided; `514e059`).
- **Phase 6 effects + replay-based multi-shot resume** landed (`7a79772`,
  E1314); handler discharge (E04) for inline + named handlers.
- **Phase 7 kernel services** — R12 slices 1–5:
  principal/scheduler/supervisor/Store/LLM\<Caps> (`3aae955`). One gap
  remains: kernel `Goal\<M>` is unbuilt — only the `LLM\<Caps>` half of
  slice 5 shipped (`kernel.rs:485` is a comment stub + `LlmGateway`); no
  `KernelGoal` struct or principal-scoped goal builtins yet.
- **Phase 8 surface** — `for!` search + `goal{}` block desugar to `goal_run`
  at parse time (`221a5d0`).
- **Multi-provider LLM backend** — `AXON_AI_PROVIDER=anthropic|openai` +
  `AXON_AI_BASE_URL`/`API_KEY` + `.env` support; trainloop gateway and
  NIM/OpenRouter work via OpenAI-compat codec (`a6205b3`).
- **Layer-3 self-improving RewriteSpec DSL** — AI-authored compiler passes as
  DATA; firewall rejects unsound paths; `fold-logical` rule kind added and
  red-teamed (`983395f` / `3da3509` / `044b73e`).
- **R7 cross-platform — AOT-wasm now broad (2026-06-10).** Beyond native, the
  LLVM codegen now AOT-compiles to **wasm32-wasip1** (headless, runs 30/30
  deterministic examples under `wasmtime`, byte-identical to interp) AND
  **wasm32-unknown-unknown** (the **WASI-FREE browser target**, runs 29/29
  deterministic examples via a JS host, with a real openable demo in
  `examples/browser/`). The "i64→i32 ABI retarget" the docs called blocking was
  a misdiagnosis — the gap was missing `#[cfg(wasm32)]` extern variants (str/
  array/dict all ported) + browser libc shims (malloc/puts/write/number-format
  externs). Codegen now emits no libc `snprintf`/`strtoll`. Gated by
  `scripts/wasm_*_parity.sh`. Remaining R7: real wasm-bindgen DOM/canvas +
  interactive/async (gated on the Phase-6 `resume` runtime); js/mobile.

## Shipped after the first refresh (since `5926377`)

Nine more commits since the first SESSION_STATUS refresh. The two
new headline pieces:

### Dict primitive + dict stdlib (10 builtins)

The ASI workhorse: string-keyed map backed by
`Rc<RefCell<BTreeMap<String, Value>>>` — reference-shared like `Chan`
so mutating calls update one underlying state. Keys are `str` only
(95% coverage without needing `Hash + Eq` on the full Value enum).

| Builtin | Purpose |
|---|---|
| `dict_new() -> Dict` | empty |
| `dict_get(d, k) -> Option<T>` | lookup |
| `dict_set(d, k, v)` | insert/overwrite |
| `dict_has(d, k) -> bool` | membership |
| `dict_remove(d, k) -> Option<T>` | delete + return prior |
| `dict_len(d) -> i64` | count |
| `dict_keys(d) -> [str]` | BTreeMap order |
| `dict_values(d) -> [T]` | key-sorted |
| `dict_map_values(d, f) -> Dict` | transform every value |
| `dict_each(d, f)` | side-effect iterate (k, v) |
| `dict_merge(d1, d2) -> Dict` | right-biased union |
| `arr_group_by(xs, key_fn) -> Dict[str, [T]]` | bucket array → dict |

`Dict` allow-listed as a deferred type name so user code can write
`fn f(d: Dict)` parameter annotations.

### Bandit module + safe_bandit demo (closes UCB1 as Tier-1)

- `examples/stdlib/bandit.ax` — UCB1 multi-armed bandit as a reusable
  userland module. `Bandit { n_arms, sums, counts }` with the Dicts
  shared via Rc<RefCell> so `bandit_update` mutates in place.
  `bandit_select(b, t)` picks argmax(mean + sqrt(2·ln(t)/count)). 5 tests.
- `examples/asi/bandit_ucb.ax` (demo #20) — pure UCB1, mod-imports the
  module. Converges to arm-2 (true_mean=0.78) in 200 rounds.
- `examples/asi/safe_bandit.ax` (demo #21) — first program that mod-
  imports BOTH `bandit` and `agent`. Each round: bandit proposes, agent
  gates, refused → zero-reward fed back to bandit (so it learns to
  avoid). Converges to arm-2 (the only highest-reward safe arm).

### More functional combinators (10 more array + dict builtins)

  arr_any / arr_all / arr_count_if / arr_zip_with
  arr_max_by / arr_min_by / arr_take_while / arr_drop_while
  arr_enumerate / arr_partition

Plus math: `ln`, `log10`, `exp` (the bandit's UCB needs ln).

### Bug fixes uncovered while writing tests

1. **`parse_type_str` didn't parse tuple types**. `"([T], [T])"` was a
   single opaque `Deferred(...)` blob. Added tuple-parsing branch.
2. **`Expr::FieldAccess` on `Type::Tuple` returned `fresh()`** instead
   of the indexed element type. So `parts.0` was an unconstrained `?N`;
   the subsequent `len(parts.0)` fired `len`'s str-fallback (which
   constrains non-Slice args to `str`), poisoning every later use of
   that name. Added a `Type::Tuple` branch returning `elems[i].clone()`.

Both bugs had been needed for any builtin returning a tuple — only
just visible because `arr_partition` is the first tuple-returning
builtin shipped.

### Ergonomic ticks

- **`let _ = …` no longer warns W0002.** Underscore-prefixed names are
  exempt from the shadow check; `let _`, `own _`, `ref _` all silent.
  Real shadowing (`let x = …; let x = …`) still warns. Three demos
  (`safe_self_improve`, `safe_bandit`, `self_improve`) printed 2-3
  W0002 lines before useful output — now quiet.

### Updated totals

- **526 workspace tests pass** (was 517 at first refresh).
- **21 ASI demos** (was 18). New: `multi_objective`, `word_freq`,
  `bandit_ucb`, `safe_bandit` (and the older `safe_self_improve` /
  `learn_linear_f64`).
- **11 userland stdlib modules** (was 10): added `bandit`.
- **41 array + 10 dict + 17 goal_* + 3 channel + …** builtins. The
  data-shaping vocabulary is genuinely complete for any in-language
  ASI program; the gap list (no HashMap → CLOSED via Dict) is empty
  for sub-Phase-7 work.

---

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
| `axon build f.ax` | native AOT binary — codegen is DEFAULT (~3s build); byte-identical to interp on all `examples/` under `AXON_AI_MOCK=1` |
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
3. Phase 5 remaining: widen SMT static discharge so a provable obligation
   elides its runtime check in the default pipeline.
4. Phase 6 remaining: row-variable unification (E03, HM-level open rows),
   `resume`/shallow-continuation runtime (handlers currently erase to body).
5. Layer-3 DSL: expand the RewriteSpec rule vocabulary beyond the current
   four verified passes; wire the firewall into CI.
6. Powell's-method generalization to longer line searches with a
   bracketing step instead of pure geometric doubling.

## ASI build loop — iteration 1 (2026-06-19)
- LANDED: code-verified R17 §12 Q5 — unsigned types (U8..U64) recognized+enforced but NON-FUNCTIONAL (`let a: u32 = N` → E0102); corrected R17 spec + REQUIREMENTS R17 row; memory saved.
- VERIFIED: repro `let a: u32 = 1` → E0102 via fast interp (`axon check`).
- NEXT: the unsigned-int support fix is a Structural slice (infer.rs/checker.rs literal-typing → ops → codegen parity) — spec-first. Beyond it, top-value R16/R17/R18 work is §9 user-owned forks (wedge call).

## ASI build loop — iteration 5 (2026-06-19)
- LANDED: R19 Slice A (let-binding) — `let a: u32 = N` typechecks + runs; out-of-range → E1900; unsigned arithmetic soundly rejected (E0102) pending Slice B. infer.rs (literal-coercion + range-check) + error.rs (E1900 registered). Full fast suite green (901).
- NEXT: Slice A-cont (param/return/struct sites), then Slice B (width-correct interp ops via width-aware Value), then Slice C (codegen+parity).

## ASI build loop — iteration 6 (2026-06-19)
- LANDED: R19 Slice A-cont — struct-field literal coercion + E1900 (`Reg { flags: 65535 }` works; 99999 → E1900). Refactored let+struct to shared try_int_literal_coercion helper. Full suite green (1004).
- REVERTED: return-coercion — return path has separate checker E0307 + fn-body-type check beyond the infer constraint; half-doing it is unsound. Deferred to Slice A-cont-2.
- NEXT: param/call-arg coercion sites; then return (needs checker E0307 + fn-body path); then Slice B (width-correct ops).
