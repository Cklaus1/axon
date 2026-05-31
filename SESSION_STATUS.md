# Axon Session Status

**Last update**: 2026-05-29
**Branch**: `merge-asi-layer3` (== `main`, pushed to `origin/main`)
**Latest commit**: `e0efa67` — robustness hardening + 12 demos (see "Shipped since" below)

Snapshot of current state. Companion to `ROADMAP.md` (forward plan),
`STATUS.md` (Phase-4 shipped state), `BUILD_DIAGNOSIS.md` and
`CODEGEN_WRAPPER_PROTOTYPE.md` (native-build investigation).

---

## The headline: execution is interpreter-first

The native LLVM/inkwell `codegen` build of `axon-core` does **not finish**
(historically 5h+). It is now **diagnosed and worked around**, not blocking:

- **Diagnosis** (`BUILD_DIAGNOSIS.md`): the cost is 100% LLVM-IR *generation*
  in the backend — rustc lowering the monomorphized, inkwell-generic-heavy
  `codegen::builtins` functions. Serial and CGU-immune. `cargo check` = ~4.5s;
  `cargo build` unbounded. NOT frontend monomorphization, NOT LLVM
  optimization, NOT one optimizable function.
- **Workaround (shipped)**: a codegen-free tree-walking interpreter
  (`crates/axon-core/src/interp.rs`) is now the execution path. The `axon`
  binary builds **without** the `codegen` feature in sub-seconds:
  `cargo build -p axon-core --no-default-features --bin axon`.
- **Fix (APPLIED + landed)**: the `#[inline(never)]` non-generic-wrapper fix is
  now fully applied — both giant codegen functions (`declare_builtins` +
  `declare_string_builtins`) route their ~971 inkwell `.build_*` calls through
  `codegen/build_wrappers.rs`; 0 raw calls remain; `cargo check` (codegen) is
  clean. `CODEGEN_WRAPPER_PROTOTYPE.md` measured the per-function win
  (LLVM-IR ~43%↓, RSS ~36%↓, ~1.7–3× faster). Constant-factor (not asymptotic) — turns a multi-hour
  build into a fraction, not into seconds (so native stays a CI/beefy-machine
  artifact). This differs from the failed trait-based IR-shim because
  non-generic wrappers monomorphize each inkwell call exactly once.

## Shipped since (this session, on `main`)

- **Codegen wrapper fix fully applied** (above) + CI **clippy `-D warnings` gate
  green** (was red, 33 lints). `check`/`test`/`clippy` green; `fmt-check` is
  pre-existing red (intentional hand-formatting style — a CI-policy call).
- **Tier-1 ASI-safety stdlib** (`examples/stdlib/`): `budget` · `constraint` ·
  `principal` · `uncertain` combinators, composed by `safe_action` (the unified
  act/deny gate), + `asi_prelude` scoring helpers. All tested, key-free.
- **Goal surface is rich**: every prose section is actionable (Inputs/Outputs→
  sigs, Verify→gate, Redteam→check, Implementation→bodies, Budget→`max_evals`);
  author-overrides (`try_variant`/`assert_deployable`/`redteam_check`); prelude
  auto-bundling. Demos: optimize/verified/redteam/compose/flagship + `learn`.
- **Mock-LLM mode** (`AXON_AI_MOCK=1`): the `ai_*` builtins return deterministic
  stubs, so all ASI demos run with no key/network/feature.
- **Observability loop works on the interpreter**: `@[adaptive]` returns persist
  to the provenance JSONL → `axon trace` / `analyze.py` / `run.sh analyze`.
- **Cross-run self-improvement** (`AXON_GOAL_CONTINUE=1`): `goal_run` resumes its
  hill-climb from the best prior input; `run.sh improve` converges across runs.
  `axon goal --iterate N <file.md>` drives this autonomously in one command —
  learn-goal converges 100→136→164→184→200 over 6 runs.
- **Multi-file modules** (`mod`+`use`+`AXON_PATH`) work via the interpreter;
  see `examples/modular/`.
- **ASI demo set spans the decision-pattern spectrum** (`examples/asi/`, all
  key-free): #1–3 LLM tasks (optimize/classify/summarize), #4 `supervised_agent`
  (greedy stream under a latching kill-switch), #5 `deliberative_agent`
  (constrained single choice), #6 `planner` (multi-step lookahead, recursive
  enums), #7 `pareto` (multi-objective frontier), #8 `allocate` (0/1 knapsack),
  #9 `rank` (in-place selection sort), #10 `local_search` (userland black-box
  hill-climbing), #11 `contained` (compile-time capability sandbox), #12
  `parallel_score` (fan-out/collect). Capability *and* control, demonstrated.
  `run.sh demos` runs the key-free tour.
- **Language hardened by build-and-fix probing**: `len` on slices; structural
  `==`/`!=`; `type Name = A | B` sum-types; or-patterns; `match`/`if` as operands;
  `&&`/`||` precedence (correctness bug); nested braces in interpolation; enum
  returns; `for x in <collection>`; **place assignment** (`xs[i]=v`, `s.f=v`,
  nested too); `chan<T>` as a type. Each gated + regression-tested.
- **Concurrency (cooperative, single-threaded)**: `spawn` runs eagerly, channels
  are shared FIFOs, `send`/`recv`/`select` work — fan-out/collect runs (see
  `parallel_score.ax`); request/response (block-on-later-send) doesn't. Memory
  `axon-cooperative-concurrency`.
- **Safety checks ENFORCED in the CLI** (were dead): `@[contained]` capability
  sandbox (E1001), static `@[verify]` (E1101), borrow (E0601) — all run in
  `run_check_pipeline`, guarded by `*_rejected_by_check` tests against drift.
- **Robust on adversarial input** (no process aborts): deep recursion →
  `RECURSION_LIMIT` panic; deep nesting → `MAX_EXPR_DEPTH` parse error; the CLI
  runs on a 1 GiB-stack worker thread. Bad builtin args fail fast/gracefully.
- **Ownership model mapped**: pass collections as `&[T]` (borrow); `for-in`
  borrows; `ref` is binding-only (memory `axon-ownership-idioms`).
- **Tests/CI**: end-to-end CLI tests (`tests/cli_run.rs`, 44) + 286 lib + 101
  integration; `all_examples_typecheck_clean` runs the full pipeline over every
  example. `dev.sh` is interpreter-first. Commits gated on a green suite.

## Top remaining work (prioritized)

1. **Tuples + native codegen of new AST nodes.** Place assignment (single-level
   *and* nested) and typed `let`/`own`/`ref` (`let x: T = e`, enforced) now work
   in the interpreter. Remaining language gap: tuple expressions (`(a, b)`).
   Native codegen lowering of the newer AST nodes (`AssignTo`, etc.) is still a
   TODO — codegen leaves them unsupported via its catch-all.
2. **Live ASI** — set `ANTHROPIC_API_KEY` (+ `--features asi-runtime`) to run the
   LLM demos for real instead of `AXON_AI_MOCK=1`.
3. **Native build** — the `#[inline(never)]`-wrapper fix is applied; validate an
   end-to-end `--features codegen` build on a beefy machine / CI.

## Known language gaps (discovered this session — future work)

Found by building real programs; none block current work (there are working
idioms) but each is a clean future task:

- **Concurrency is cooperative (single-threaded).** `spawn`/`chan`/`send`/`recv`
  now run in the interpreter — `spawn` bodies execute eagerly, so fan-out/collect
  (workers produce, main consumes) works; patterns where a spawned task must
  *block* on a value sent later (request/response) don't — send before recv.
  `select` fires the first arm whose channel is ready (cooperative, value-less).
- (Fixed this session: `len` accepts slices; structural `==`/`!=`; `type Name =
  A | B` sum-types; or-patterns; `match`/`if` as operands; `&&`/`||` precedence;
  nested braces in interpolation; functions returning enums; `for x in coll`;
  place assignment `xs[i] = v` and `s.field = v` (incl. nested); typed bindings
  `let x: T = ...`; tuples — `(a, b, ...)` literals, `.N` access (inc. nested
  `.0.0`), `let (a, b) = e` destructuring, `(a, b)` patterns in `match`.)

## What works now (via the interpreter, no LLVM)

| Command | Behavior |
|---|---|
| `axon run f.ax` | parse → type-check → interpret; forwards exit code |
| `axon check f.ax` | type-check only |
| `axon test f.ax` | run `@[test]` fns in-process (honors `should_fail`) |
| `axon goal g.md` | prose `goal.md` → `.ax` (axon-surface) → check → run; `--emit` to print the `.ax` |
| `axon build f.ax` | native AOT — cfg(codegen) stub unless built `--features codegen` |
| `axon parse f.ax` / `axon lsp` | need `--features serde-json` |

- All 21 deterministic `examples/*.ax` run correctly under the interpreter.
- All 6 `examples/asi/*.ax` + `examples/goals/hello-goal.ax` run end-to-end
  (LLM calls gated behind `--features asi-runtime`; without an `ANTHROPIC_API_KEY`
  the `ai_*` builtins return `Err` and the `@[verify]`/redteam gates fire
  correctly).
- Tests green: ~16 interpreter unit tests, 280 lib + 101 integration, 8 surface.

## Feature map

- default = `codegen` + `serde-json` (the slow native path).
- `--no-default-features` → feature-light interpreter `axon` (the dev build).
- `asi-runtime` → live `ai_complete`/`ai_extract_*` via `axon-ai` (reqwest).
- `serde-json` → `axon parse --json`, `axon lsp`. NOTE: building the lib with
  `serde-json` + `--emit=link` (no codegen) SIGSEGVs rustc on this box while
  codegen-ing the serde derives on the recursive AST enums — that's why the
  default interpreter `axon` omits `serde-json`.

## ASI builtins in the interpreter (M2)

- Deterministic core needs no external crate: `goal_run` (hill-climb over an
  `@[adaptive] fn(i64)->i64`, else retrospective over an in-memory provenance
  store), `@[adaptive]` logging, `Uncertain`/`Temporal` constructors,
  `@[verify(confidence OP K)]` runtime gate (reuses `verify::decode_verify_predicate`),
  `f64_to_i64`/`i64_to_f64`.
- `ai_complete`/`ai_extract_uncertain_{i64,f64}` → `axon-ai`, gated on `asi-runtime`.

## Phase-10 surface (axon goal)

- `axon-surface`: `GoalFile::parse` (prose `.md` → sections) + `compile::emit`
  (→ `.ax`). v1 **lifts real function bodies** from the goal file's ` ```axon `
  fenced blocks (`author_code()`, all blocks except the Verify predicate) and
  emits `TODO:` scaffolding only for helpers the prose omits.
- `examples/goals/hello-goal.md` has an `## Implementation` section with real
  `test_input`/`build_prompt`/`score_output`/`adversarial_input` bodies.

## Open / next

1. Apply the `#[inline(never)]` wrapper fix to `codegen/builtins.rs`
   incrementally (`declare_builtins` first, measure, then `declare_string_builtins`),
   combined with function-splitting for CGU parallelism — IF a native artifact
   is wanted. Otherwise native stays CI/release-only.
2. Surface v1.1+: LLM-driven body *generation* from prose (roadmap), more
   `examples/goals/*.md`, round-trip `.ax → .md` drift check.
3. Branch hygiene: stale `track-*` / `worktree-agent-*` branches still exist
   (worktrees already removed); the work is merged into `main`.
4. `axon run` does not yet forward argv; type-incorrect programs run-panic only
   under paths that skip the checker (run/goal/check all check first).
