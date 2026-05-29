# Axon Session Status

**Last update**: 2026-05-29
**Branch**: `merge-asi-layer3` (== `main`, pushed to `origin/main`)
**Latest commit**: `7176f8f` — trait-method demo + e2e test (see "Shipped since" below)

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

## Shipped since (this session, ~30 ticks on `main`)

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
- **Tests/CI**: end-to-end CLI tests (`tests/cli_run.rs`), goal-demo + all-examples
  -parse regression locks, full builtin coverage (90/90). `dev.sh` is interpreter-first.

## Known language gaps (discovered this session — future work)

Found by building real programs; none block current work (there are working
idioms) but each is a clean future task:

- **No place assignment.** `xs[i] = v` and `s.field = v` don't parse — assignment
  targets must be a bare identifier. Mutate by rebuilding + rebinding the whole
  value (e.g. `s = observe(s, a)`). A real fix adds an lvalue AST node rippling
  through parser/resolver/infer/checker/codegen — a dedicated effort, not a tick.
- **No place assignment.** `xs[i] = v` / `s.field = v` don't parse; rebuild +
  rebind whole values. (AST-level: an lvalue node through ~12 files + codegen.)
- **No tuple expressions.** `(a, b)` doesn't parse (the type system has
  `Type::Tuple`, but there's no value syntax); use a struct.
- **No typed `let`.** `let x: T = e` doesn't parse; `let x = e` (inferred) only.
  (AST-level: `Let` has no type slot.)
- (Fixed this session: `len` accepts slices; structural `==`/`!=`; `type Name =
  A | B` sum-types; or-patterns; `match`/`if` as operands; `&&`/`||` precedence;
  nested braces in interpolation; functions returning enums; `for x in coll`.)

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
