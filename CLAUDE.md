# Axon Language

AI-optimized, statically-typed systems language. Compiles to native via LLVM 17.

## Design Principles

- No null, no exceptions — `Option<T>` and `Result<T,E>` everywhere
- Ownership without GC — simplified two-mode ownership (`own`/`ref`)
- Structural typing — no `implements` keyword
- Comptime — zero-cost compile-time execution
- Multi-target — native, wasm, js (Phase 4+)
- AI-first — `@[agent]`, `@[goal]`, `@[verify]`, `@[adaptive]` annotations built-in

## File Extension

`.ax` — e.g. `main.ax`, `server.ax`

## Commands

```bash
# Build the fast interpreter CLI (no LLVM/codegen — sub-second):
cargo build -p axon-core --no-default-features --bin axon
cargo test                          # run all unit + integration tests

axon run   examples/hello.ax              # type-check + interpret (tree-walking, no codegen)
axon goal  examples/goals/hello-goal.md   # compile prose goal → .ax → check → run
axon check examples/hello.ax              # type-check only
axon test  examples/tests.ax              # run @[test] functions (in-process interpreter)
axon parse examples/hello.ax              # print AST as JSON   (needs --features serde-json)
axon complexity examples/hello.ax         # MDL description-length metric over the AST (per-fn + total; --json → axon-complexity/1)
axon trace                                # summarize the provenance log: per-@[adaptive]-fn score trajectory (--fn NAME, --json)
axon trace --ai                           # AI-call audit trail: per-fn ai_complete calls, tier→model, mode (live/mock/replay/fallback), metered cost, and the goal each served (--json → axon-ai-audit/1)
axon build examples/hello.ax              # native AOT binary   (codegen is now DEFAULT; builds in ~3s — see BUILD_RESOLVED.md)
axon --version                            # e.g. "axon 0.1.0 (02cd617)" — semver + git SHA (build.rs); "-dirty" if uncommitted

# Phase-10 Hello-Goal CLI flow (compile → review → approve → deploy):
axon intent compile examples/goals/hello-goal.md   # prose .md → typed .ax skeleton (--out path; --json → axon-intent-compile/1)
axon ast review    examples/goals/hello-goal.ax    # type-check + list fns/attrs/effects (--json → axon-ast-review/1)
axon ast approve   examples/goals/hello-goal.ax    # record human sign-off (writes <file>.ax.approved)
axon deploy        examples/goals/hello-goal.ax    # safety-gate pipeline → run (--gate verify; --json → axon-deploy/1)
axon redteam       examples/goals/hello-goal.ax    # run redteam_check fn (--json → axon-redteam/1)
```

**Execution is interpreter-first.** `run`/`goal`/`test`/`check` work without the
`codegen` feature via the tree-walking interpreter (`interp.rs`). The native
LLVM/inkwell `codegen` feature is **on by default and now builds in ~3s** —
the long-standing "build never finishes" stall was a `serde-json` × `codegen`
default-feature collision (recursive AST serde derives × codegen
monomorphization), fixed by dropping `serde-json` from `default`
(`BUILD_RESOLVED.md`). `cargo build -p axon-core` produces the native `axon`
compiler; `axon build foo.ax` emits a native binary. **Do not enable
`codegen` + `serde-json` together** until the AST derives are decoupled — that
combo reintroduces the stall. `axon parse`/`lsp` (JSON) opt in with
`--features serde-json` (interpreter build, no codegen). Add `--features
asi-runtime` to enable live `ai_complete`/`ai_extract_*` (used by
`examples/asi/*` and `axon goal`).

### Interpreter env vars

| Var | Effect |
|---|---|
| `AXON_SEED` | Seed the RNG (`u64`) for reproducible `random_*` runs |
| `AXON_MAX_DEPTH` | Raise the recursion-depth ceiling (default 6000, clamped to 1,000,000). The interpreter thread stack scales with it, so the graceful "recursion limit" panic always fires before a real stack overflow |
| `AXON_AI_MOCK` | Use deterministic stub AI responses instead of live calls |
| `AXON_AI_REPLAY` | Path to an LLM-call replay cache: every `ai_complete` is memoized by `(prompt, model)` — a first run records `(response, tokens)`, a re-run replays it verbatim (no live call / mock / API key) so an AI run is exactly reproducible (ROADMAP §9.5 F2) |
| `AXON_PATH` | Colon-separated module search path for `mod` imports |

## Compiler Pipeline

```
.ax source
  → Lexer    (token.rs / lexer.rs)      logos-based tokenizer
  → Parser   (parser.rs)                recursive descent → AST (ast.rs)
  → Resolver (resolver.rs)              name resolution, scope building
  → Infer    (infer.rs / types.rs)      Hindley-Milner type inference
  → Checker  (checker.rs)               semantic validation, diagnostics
  → Codegen  (codegen.rs)               LLVM IR via inkwell 0.4
  → Link     (main.rs)                  cc linker → native binary
```

## Crate Structure

```
crates/axon-core/src/
  token.rs      Token enum (logos derive macros)
  lexer.rs      Lexer::tokenize() → Vec<(Token, Span)>
  ast.rs        Program, Item, Expr, Stmt, Type, Pattern, Literal
  parser.rs     Recursive descent Parser → AST
  resolver.rs   SymbolTable, name resolution, scope analysis
  types.rs      Type enum, Substitution, unification
  infer.rs      Hindley-Milner inference, constraint solving
  checker.rs    Semantic rules R01-R12, diagnostics
  builtins.rs   BUILTINS table, builtin_sigs(), DEFERRED_ATTRS
  codegen.rs    LLVM IR codegen via inkwell
  error.rs      CompileError, Diagnostic types
  lib.rs        parse_source() public API
  main.rs       axon CLI (run/build/check/test/parse commands)
spec/
  compiler-phase1.md   Phase 1 spec (complete)
  compiler-phase2.md   Phase 2 spec (complete)
  compiler-phase3.md   Phase 3 spec (generics, traits, closures, channels)
  compiler-phase4.md   Phase 4 spec (LSP, formatter, multi-file, caching)
  grammar.ebnf         Formal EBNF grammar
  stdlib.md            Standard library reference
  runtime.md           C ABI / runtime function reference
  language-tour.md     Hands-on language walkthrough
examples/
  hello.ax             Hello world
  math.ax              Basic arithmetic
  structs.ax           Struct types and field access
  enums.ax             Enum ADTs and pattern matching
  slices.ax            Array/slice indexing
  options.ax           Option<T> usage
  while.ax             While loops (sum 1..10 = 55)
  interpolation.ax     String interpolation
  algorithms.ax        GCD, primes, Collatz, power
  modulo.ax            FizzBuzz with %
  logical_ops.ax       && and || operators
  floats.ax            Scientific notation floats
  escapes.ax           String escape sequences \n \t \\
  math_builtins.ax     abs_* min_* max_* builtins
  parse_int.ax         parse_int with Result matching
  comprehensive.ax     Multi-feature integration test
  stdlib_tests.ax      @[test] suite for all builtins
  tests.ax             assert_eq based unit tests
  should_fail_test.ax  @[test(should_fail)] demo
```

## Language Quick Reference

```axon
// Variables
let x = 42            // i64
let y = 3.14          // f64
let s = "hello"       // str
let b = true          // bool
x = x + 1            // reassignment (no let)

// Functions
fn add(a: i64, b: i64) -> i64 { a + b }

// Structs
type Point = { x: f64, y: f64 }
let p = Point { x: 1.0, y: 2.0 }
println(to_str_f64(p.x))

// Enums
type Shape = Circle { r: f64 } | Square { side: f64 }
let s = Shape::Circle { r: 5.0 }

// Control flow
if x > 0 { "pos" } else { "non-pos" }
while i < 10 { i = i + 1 }
match val { Ok(n) => n  Err(e) => 0 }

// Error handling
fn parse(s: str) -> Result<i64, str> {
    let n = parse_int(s)?
    Ok(n * 2)
}

// String interpolation
println("hello {name}, age {to_str(age)}")

// Operators: + - * / %   == != < > <= >=   && ||

// Builtins: println print eprint eprintln
//           to_str (polymorphic over scalars: i64/f64/bool) parse_int len
//           to_str_f64 to_str_bool (explicit forms; to_str now covers them)
//           abs_i32 abs_f64 min_i32 max_i32
//           assert assert_eq assert_err

// Testing
@[test]
fn test_add() { assert_eq(add(2, 3), 5) }

@[test(should_fail)]
fn test_panic() { assert(false) }

// AI annotations (deferred, emit info not errors)
@[goal("maximize throughput")]
@[adaptive]
@[agent]
@[verify]

// Capability sandboxing — @[contained] (enforced by `axon check`, E1001/E1004)
//   fs:    [read("./data/"), write("./out/")]   allowlist of path prefixes
//   net:   ["api.example.com", "*.trusted.io"]  allowlist of hosts (leading * glob)
//   exec:  none | any                           process spawning
//   never: [read("/etc/"), write("/"), net("*"), exec]   hard deny (overrides allowlist)
//   env:   reading env_var(...) is DENIED inside @[contained] (E1001) — env is an
//          ungrantable ambient secret channel; read it OUTSIDE the boundary, pass the value in
@[contained(fs: [write("./out/")], net: ["api.example.com"], exec: none)]
fn scorer() -> i64 { /* compiler refuses any I/O outside the declared caps */ 0 }
```

## Key Invariants

- All integers default to `i64`; `i32` exists for interop
- `str` is `{ i64 len, ptr data }` in LLVM IR, always null-terminated
- `Result<T,E>` canonical layout: `{ i1 tag, [max(sizeof T, sizeof E) x i8] }` — tag 0=Err, 1=Ok
- `Option<T>` layout: `{ i1 tag, T }` — tag 0=None, 1=Some
- Arrays/slices: `{ i64 len, ptr data }` with heap-allocated backing
- Lambdas lower to `__lambda_N` module-level functions (captures: Phase 3)
- `@[agent]`, `@[goal]` etc. emit I0001 info diagnostic, not errors

## Adding a New Builtin

1. `builtins.rs` — add entry to `BUILTINS` array
2. `codegen.rs` — declare LLVM function in `declare_builtins`, handle in `emit_call`
3. `infer.rs` — `builtin_sigs()` auto-populates; add `Type::` mapping to `fn_return_types`
4. `checker.rs` — usually automatic via `check_call_arity_and_types`
5. `examples/` — add usage example and test

## Phase Status

| Phase | Status | Features |
|-------|--------|---------|
| 1 | ✅ Complete | Functions, if/else, match, Result/Option, basic builtins, JIT+AOT |
| 2 | ✅ Complete | Structs, enums, slices, lambdas, while, `%`/`&&`/`||`, string interp, extended builtins, IR bug fixes |
| 3 | ✅ Complete | Generics, traits, closures w/ captures, channels, borrow checker, comptime, spans |
| 4 | ✅ Complete | LSP, formatter, doc gen, incremental compile, multi-file, cross-compile |
| ASI 1–3.6 | ✅ Merged | `Uncertain<T>`, `Temporal<T>`, `@[verify]`, `@[adaptive]`, `goal_run`, `ai_complete`, `ai_extract*`, `@[contained]` |
| 5 | 🚧 In progress | **Landed (no Z3):** `@[pure]` purity checker (E1207), `@[total]` termination checker (E1208), and refinement types `T where <pred>` — named + inline (bare param `d: i64 where _!=0` and parenthesized `-> (T where P)`), transparent to base, with **constant-predicate obligations** at arg/return/struct-field sites (E1209; predicate subset: arith, comparisons, `&&`/`\|\|`/`!`, `str_len`, `str_eq`). **NON-constant refinement contracts are enforced at RUNTIME at ALL FOUR obligation sites** (the spec's Z3-free `--proof-timeout 0` fallback): PRECONDITIONS (param `p: T where P` at fn entry), POSTCONDITIONS (fn `-> T where P` at each return), STRUCT CONSTRUCTION (refined fields + whole-struct `where _.lo<=_.hi`, `_`=instance), and LET/OWN/REF bindings (`let p: T where P`, plus the constant case now a static E1209); a violation exits **6** (`REFINE_VIOLATION_EXIT_CODE`); enforced in BOTH interp + native codegen (byte-identical, `exit_code_parity.sh`), out-of-subset codegen cases E0910-refused. The SMT backend (`smt.rs`, opt-in `smt` feature, `axon verify`) statically discharges what it can prove (@[verify] bounds, refinement returns, arg-forwarding subtyping), and **SMT discharge is now wired into the DEFAULT pipeline** (514e059) — proven for-all-inputs `@[verify]`/refinement-return checks are statically elided at run/build, not just under `axon verify`. The SMT encoder now also proves postconditions built from the **bound builtins** `min_i64`/`max_i64`/`abs_i64` (+ f64) and **logical connectives** `&&`/`\|\|`/`!` (clamp/bound fns — the common provable shape; checked abs preserves the `abs_i64(i64::MIN)` panic) — and those bound builtins are folded consistently by all four constant/verification evaluators (SMT encoder, comptime, checker `const_eval_int`, the Layer-3 DSL `fold-bound-builtin` rule). **Remaining:** struct whole-refinement static discharge (`where _.lo<=_.hi` class — a flow-sensitive construction-site obligation, deferred-by-design as runtime-enforced). See `spec/compiler-phase5.md` |
| 6 | ✅ Complete | Row-polymorphic effects + handlers. **Landed:** effect-row syntax `fn f() -> T \| {IO, Net}` (parse + fmt); builtin effect catalog (`builtin_effect_row`); subsumption checker (E1310) with **transitive** anti-laundering (no hiding an effect behind an un-annotated helper); `@[contained]`→effect-row bridge; `substrate`/`surface` file markers (E1306); the effect walker recurses `with`/`for`/assign bodies (no laundering hole); **handler discharge (E04)** for inline *and* named handlers (`handler NAME = handler { on E(p)=>… }` defs resolve via parser desugar). Effect codes are **E1306 + the E131x block** (the spec's nominal E1300–E1308 collided with AI-policy E1300–E1302). **Suspend-across-host-event resume runtime LANDED** (`host_await(req)->reply` / `host_await_opt`, R15): native via a worker-thread substrate, wasm32-wasip1 via direct stdin, and the **BROWSER (wasm32-unknown-unknown) via `wasm-opt --asyncify`** — the same `host_await` import unwinds the wasm to JS, awaits a Promise, and rewinds to resume (B1/B2/B3, the `crates/axon-wasm` interpreter cdylib + `examples/browser/interactive.html`). So interactive Axon (REPLs, the approval agent, frame loops) runs on all three substrates; this **unblocks R7c/R13/R14**. Codegen E0910/E1315-refuses `host_await` (sound-by-refusal, interp-only). **Also landed (this iteration):** E03 row-variable propagation through higher-order forwarders (`invoked_param_indices`/`callback_arg_effects` closes row variables at call sites); E1316 `@[contained]` deprecation notice via `axon check --effects-strict`; `examples/asi/optimize.ax` annotated with `\| {AI, Net}` effect rows on all AI-calling fns. Checklist item on host_await full-Value payloads is deferred as str-serialization covers the browser/tool boundary today. See `spec/compiler-phase6.md` and `governance/specs/R15-resume-runtime.md` |
| 7 | ✅ Complete (kernel) | `Principal` authority registry, cooperative scheduler, live `Supervisor`, durable `Store<T,C>`, `LLM<Caps>` gateway with per-token cost metering — R12 slices 1–5 (3aae955). **Kernel `Goal` LANDED (fdbb8b2, R12b spec):** `KernelGoal` runs the optimizer scoped to a `Principal`'s budget — each eval debits the principal, exhaustion stops with exit 7 (`E1604`); builtins `kernel_goal_create/run/best_score/spent/budget_left` (interp-only, codegen E0910-refused). Remaining = the *language-level* `Goal<M>`/`Budget<R>` value-type surface sugar (the kernel realizations are their semantic cores) |
| 8 | ✅ Complete | `for!` search + `goal{}` block desugar to `goal_run` at parse time (221a5d0); `goal { … subject_to: "fn" }` → `goal_run_constrained` (hard feasibility gate), `goal { … choices: N }` → `goal_run_categorical` (unordered choice; mutually exclusive with `subject_to`); `agent{}`/`search` underspecified, deferred |
| 9 | ✅ Complete | **Replay, audit, sandbox.** Slice 1 LANDED: every `axon run` / `axon goal` stamps a `run_start` event to the provenance log (run-id + effective RNG seed + source path); `axon trace --replay <run-id>` looks up the record and re-executes the source with the same seed → deterministic `(Trace, Seed)` pair for every run (F2 CLI wrapper, ROADMAP §9.5). The run-id is printed to stderr and is the replay handle. **F3 LANDED (Slice 2):** every capability audit record (`ai_call`, `agent_action`) now carries `effect_row` (the row-polymorphic effect tag: "AI"/"FS"/"Net"/"Exec") and `principal` (the name of the executing principal, default "root"). `principal_activate(handle)` sets the current principal for audit attribution; `principal_current_name()` reads it back. `axon trace --ai --json` schema bumped to `axon-ai-audit/2`; human view shows `principal:` when non-root. **F5 LANDED (Slice 3):** `Sandbox<P>` runtime effect enforcement — `sandbox_create(principal, allowed_effects)` registers a sandbox with a comma-separated effect ceiling; `sandbox_run(sandbox, fn_name, arg)` executes a named function inside it; any builtin whose effect row exceeds the ceiling raises `SandboxViolation` (exit 8, distinct from panic/verify/halt/ai-policy/refine/goal-budget). Interp-only (codegen E0910-refused). Phase 9 is now complete. |
| 9 (Layer-3) | 🚧 Prototype | Self-improving compiler: AI-authored passes as DATA (`RewriteSpec` DSL); 4-gate firewall (`verify_pass`: G1 interp-oracle correctness, G2 capability-monotonicity, G3 test-preservation, G4 perf) rejects unsound paths. Closed rule vocabulary: `fold-int-literal`, `fold-arith-identity`, `simplify-bool-not`, `fold-const-branch`, `fold-logical` (red-teamed), and `fold-bound-builtin` (min/max/abs over literals, checked-abs preserves the i64::MIN panic, firewall-cleared with a built-in red-team) |
| 10 v0–v1 | ✅ Landed | Codegen-free tree-walking interpreter (`interp.rs`) → `axon run`/`test`/`goal`; prose→AST surface compiler (`axon goal`, lifts ` ```axon ` blocks); ASI builtins in the interpreter. Native codegen build diagnosed as serial LLVM-IR-gen (`BUILD_DIAGNOSIS.md`); fix prototyped (`CODEGEN_WRAPPER_PROTOTYPE.md`) |
| 10 v1.1 | ✅ Landed | **Phase-10 Hello-Goal CLI flow:** `axon intent compile` (prose .md → typed .ax; `--json → axon-intent-compile/1`), `axon ast review` (type-check + structured item list; `--json → axon-ast-review/1`), `axon ast approve` (human sign-off → `.ax.approved` marker), `axon deploy` (safety-gate pipeline: type-check → run, `--gate verify` treats exit 3 as blocking; `--json → axon-deploy/1`), `axon redteam` (execute `redteam_check` fn; `--json → axon-redteam/1`). All five Phase-10 CLI verbs now exist; `*.ax.approved` in `.gitignore`. **Remaining:** LLM-driven body generation (v1 lifts author-written blocks; v1.1 still stubs missing bodies with `TODO:`). |

## Forward Roadmap

Forward planning lives in `ROADMAP.md`. Highlights:
- Two-track architecture: typed `.ax` is an IR; user-facing surface is structured-prose forms compiled to AST
- Userland OS, not kernel replacement
- 14-type Tier-1 stdlib (`Goal`, `Constraint`, `Budget`, `Principal`, `Effect`, `Store`, etc.)
- TCB enumerated; self-modification cannot weaken it
- `examples/asi/` is the public face, not `examples/fibonacci.ax`

## ASI Demo Set

`examples/asi/` ships end-to-end ASI workflows on already-shipped primitives. Each demo is the
forcing function for upcoming phase work — friction encountered while building lands in
`ROADMAP.md` §9.5 as concrete spec items.

| Demo | Pattern |
|---|---|
| `optimize.ax` | ai_complete + parse + adaptive + verify + redteam |
| `classify.ax` | ai_extract_uncertain_i64; confidence in score |
| `summarize.ax` | composite metric (length × LLM-judged coverage) |
| `code_review.ax` | two cooperating @[adaptive] fns (proposer + critic) |
| `world_model.ax` | executable world model: predict → fit-check → compress-to-simplest (fit a refinement gate + MDL objective; `spec/worldmodel-loop.md`) |
| `constrained_goal.ax` | constrained search (`subject_to`): `goal_run_constrained` hill-climbs only over candidates a boolean constraint accepts — a hard feasibility gate, not a soft penalty |
| `categorical_goal.ax` | categorical search: `goal_run_categorical` treats the arg as an UNORDERED choice index (no gradient) — exhaustive over the set, finds an isolated best a hill-climb would miss |

CLI surface simulated as `examples/asi/run.sh` (the eventual Phase-10 `axon goal …` shape).

## Design Reference

Full language design: `/home/cklaus/projects/BTask/packages/bcode/AI_Language_Plan.md`
