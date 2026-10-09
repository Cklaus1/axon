# Axon Changelog

## `AXON_ENGINE=vm` compiles the scalar core of fn bodies to bytecode (R50 S1)

Second slice of the bytecode engine (`governance/specs/R50-register-vm.md`). Speed only: no behaviour change under either engine, and `tree` stays the default.

**Interpreter**
- **Under `AXON_ENGINE=vm`, fn bodies made of the scalar core run as flat ops** instead of one tree-walk: literals, identifier reads, blocks, `let`/`own`/`ref` (typed ones through the same `bind_let`), `=` to a local (AX-31 in-place appends first, as before), binary and unary operators (`&mut` aside), `if`, `while`, `for` over integer ranges, `return`/`break`/`continue`, `?`, `Some`/`None`/`Ok`/`Err`, string interpolation, and calls by name without `&mut` arguments. Every other node still runs on the tree-walker as one op. Comparisons in `if`/`while` conditions fuse with the branch; `x = l op r` and `while i < n` over locals and int literals are single ops that skip `Value` construction for int/float operands. An activation's operand stack lives in its pooled call frame. `vm: <fn> <n> ops, <k> tree nodes` (`AXON_VM_TRACE=1`) now reports the lowering: `fib` compiles with 0 tree nodes. The tree-walker's `If`/`While`/`For`/index arms now share `cond_bool` and `strict_int` with the engine (no change in what they do).
- A block, `while` iteration or `for` body that binds nothing (no `let`/`own`/`ref` anywhere below it) runs without its own scope under `vm`: that scope would stay empty, so it is unobservable.
- `scripts/vm_parity.sh` knows slice `S1` (its default): 284 files, 1,761 bodies, 25,090 lowered ops, 1,966 tree ops, 0 differ. `--repros S1`: `while` loop iteration 228 instructions (tree 1,150); a one-argument call 790 (budget 800; tree ≈ 910, was 1,072 and ≈ 1,106). A call by name resolves its callee once at compile time (`dispatch_named`), and `call_fn_in` binds parameters by index instead of draining the argument vector (cost only, both engines).
- **The shared call path is cheaper, cost only** (both engines; no change in what any call does). Call frames pool as `Box<Env>` and are cleared without drop glue for scalar bindings; `call_mut` takes its frame from that pool; the executing fn is a `Cell<Option<&FnDef>>` restored, with the call depth, by one guard; `call_fn_in` runs the common call (no attribute-driven step, no refinements) on a short path and pushes parameters and `goal_met` straight into the empty frame; under `vm` a call by name leaves its arguments on the operand stack, where a user-fn callee binds them from.

**Interpreter (R50 S4: lambdas)**
- **Under `AXON_ENGINE=vm`, a lambda is one op and its body compiles on its own** on its first run, kept with the lambda's shared code (`ClosureCode.compiled`), so `fold.ax`'s `main` and its `|acc, x| acc + x` run with 0 tree ops. A one-op scalar body (`acc + x` over int or float locals) is computed without entering the op loop. `AXON_VM_TRACE=1` names lambda bodies `<owner>::lambda#<i>` (source pre-order, nested lambdas included; `<fn>::verify::lambda#<i>` inside a `@[verify]` predicate; `<module>::lambda#<i>` for module `let`s, `refine` predicates and type refinements), printed once per body; a fn-value forwarder or other unresolved lambda prints `vm: tree <anon>: unresolved lambda` once per code instance. The `Expr::Lambda` arm is now the shared helper `make_closure` (no behaviour change).
- **A builtin -> closure call allocates nothing on the lent path, cost only** (both engines). `arr_fold`, `arr_map` and the other closure-taking builtins pass their one or two arguments straight into the parameters' scope instead of building a `Vec` per call, the closure runs on a pooled frame (bindings, scope marks and operand stack keep their capacity), and a call by local name drains its argument vector into the parameters and recycles it. The copied path (a closure with other references) still copies its capture cell per call. `arr_fold` copies an int element inline.
- `scripts/vm_parity.sh` knows slice `S4` (lowers `Lambda`): 284 files, 1,864 bodies, 25,332 lowered ops, 1,880 tree ops, 0 differ. `--repros S1,fold`: `fold.ax` 330 instructions per element (budget 350; tree ≈ 485, was ≈ 916); loop 225, one-argument call 793 (budget 800). The bytecode activation is inlined into each of its callers so the fn call path keeps its S1 cost.

## Engine selector for `axon run`, and a `match` guard no longer leaks its scope (R50 S0, compilebench AX-57)

First slice of the bytecode engine (`governance/specs/R50-register-vm.md`). It adds the plumbing and one fix to the reference interpreter; nothing is lowered yet, so `AXON_ENGINE=vm` runs every body through the tree-walker and behaves exactly like `tree`.

**Interpreter**
- **A `match` arm whose pattern or guard fails with `?` pops its scope** (AX-57, behaviour change). An `Err` out of an arm's guard (`Some(b) if fail()? > b`) or pattern, or out of a `while let` pattern, skipped the arm's `env.pop()`. The enclosing block then popped the arm's scope instead of its own, so the caller's bindings were shadowed by the callee's locals: after `f(&mut a)` returned through such a guard, a `let a = 99` inside `f` replaced the caller's array and `len(a)` panicked `expected str/array/dict, got i64`. The scope is now popped before the `Err` propagates.
- **`AXON_ENGINE=tree|vm` selects the engine for fn bodies** under `axon run`, `axon-run`, `axon test` and `axon goal`. The default is `tree`. Under `vm` each function in the function table compiles on its first run; through this slice every body compiles to a single fallback op over the tree-walker. Any other value exits 2 with `AXON_ENGINE must be "vm" or "tree" (got "<v>")` before the program runs. `AXON_VM_TRACE=1` prints one `vm: …` line per compiled body to stderr. The browser build (`axon-wasm`) exports `axon_set_engine(0 = tree, 1 = vm)` in place of the variable.
- The call dispatch, `let` binding and `&mut` call steps are now shared helpers (`dispatch_call`, `bind_let`, `call_mut`) so the engine can reuse them. No behaviour change.

**Gates**
- **`AXON_HARNESS_STRICT=1 scripts/parity_all.sh` fails a run in which a harness skipped without an allow-list entry** (R50 §11). `parity_all.sh` never read the variable: every skip counted as success and only `EXPECT_MIN_PASS=40` stood between a green run and a suite that had quietly stopped running harnesses. Under the variable, a skipped harness (a `PARITY_SKIP_WASM=1` skip included) fails the run unless `scripts/parity_allowed_skips.txt` lists it with a reason; an entry without a reason exits 2. The list starts with `android_compute_parity` (no Android NDK on the gate host) and `browser_compute_parity` (opt-in, `BROWSER_PARITY=1`). Without the variable the script behaves as before.
- **The wasm parity harnesses run the engine the caller selects.** `wasm_parity.sh`, `wasm_fs_parity.sh` and `wasm_host_await_parity.sh` pass `--env AXON_ENGINE=…` to wasmtime when `AXON_ENGINE` is set, and `wasm_browser_interp_parity.sh`'s driver forwards it to the `axon_set_engine` export before `axon_eval` (R50 §8).
- **New R50 gate scripts**: `scripts/vm_parity.sh` (both engines byte-identical over the example and fixture corpus), `scripts/vm_perf_gate.sh` (instruction budgets, `--repros` per-construct mode) and `scripts/vm_wasm_depth.sh` (wasm32 recursion depth under both engines), with fixtures in `crates/axon-core/tests/fixtures/vm_depth/`, `vm_perf/` and `vm_parity_skip.txt`.

## Cheaper variables, calls and literals in `axon run` (compilebench AX-53…AX-55)

Interpreter-cost fixes from compilebench's `AXON_FINDINGS.md`. They change speed only: output is byte-identical on every program measured, and the full suite passes with and without `codegen`.

**Interpreter**
- **A variable read is an index into a frame slot** (AX-53). Every read hashed the node to find its name, then scanned the whole environment backwards, so its cost grew with the number of locals: a loop reading one of 64 locals cost 4,864 instructions per iteration. The resolver now gives each binding (parameters, `let`, pattern and loop binders, captures) a slot in its function's or lambda's frame. That loop now costs 1,255, the same as with 16 locals.
- **A call decides its function's attributes once** (AX-54). Each call cloned the function name, scanned its attributes for `@[agent]`, `@[ai]`, `@[goal]` and the rest, and allocated a new argument vector and environment. The function table now stores those answers, and frames and argument buffers are reused.
- **Numeric literals are built once** (AX-55). `Int`, `Float` and `Bool` literals are read inline instead of rebuilt per evaluation, and binary operators on plain variables and literals no longer clone their operands.

Instructions (`perf stat`, release build) on the compilebench programs, before → after: `fib-recursive` 16.22 G → 12.56 G, `collatz` 81.02 G → 69.23 G, `mandelbrot` 54.49 G → 49.47 G, `arr-sum` 63.60 G → 59.98 G, `qsort` 114.54 G → 103.61 G. `axon run` still retires several times the instructions of CPython 3.14 on these programs. The remaining cost is per-node recursion and dispatch spread across the tree-walker (AX-18 stays open), which `governance/specs/R50-register-vm.md` addresses.

## Nested tuples and struct interpolation natively, honest `--version`, race-free parity tests (compilebench AX-48…AX-52)

Fixes for the fourth batch of defects compilebench recorded in its `AXON_FINDINGS.md`.

**Native build**
- **A tuple element that is itself a tuple destructures** (AX-48). `let (a, u) = t` followed by `let (b, k) = u` was refused with E0910 when `b` was read: the element binding got no semantic type, so the second destructure had nothing to index. A nested pattern `let (a, (b, c)) = …` is still a parse error.
- **Structs, enums, `Option`, `Result`, arrays and tuples interpolate** (AX-49). `"{p}"` with `p` a struct was refused with E0910 ("inner type is erased"). Interpolation now renders from the static type, through one generated helper per type, and prints what `axon run` prints, byte for byte. A bare `None` whose payload type is unknown at the site, and Decimal, Dict, channel, closure and Uncertain values, are still refused, with a message naming the type.

**Build**
- **`axon --version` names the commit it was built from, or `unknown`** (AX-50). `build.rs` asked git from the crate directory, so a source copy inside another repository reported that repository's commit, `-dirty`. It now trusts git only when the workspace is the repository root; a packager can set `AXON_GIT_SHA`. The build cache is keyed on the compiler binary's digest, so a shared `unknown` does not collide.
- **`cargo fmt --all --check` passes** (AX-52). One layout-only sweep, listed in `.git-blame-ignore-revs`.

**Tests**
- **The `cli_run` parity harnesses test the binary under test** (AX-51). The scripts each ran `cargo build` into the worktree's `target/debug` and ran that, so they tested a compiler the test run had not built, and parallel tests rebuilt it under each other (`dict_parity` saw `interp=127`, `clock_parity` skipped). `cli_run` now passes `AXON` and `AXON_RUN`, and `scripts/lib/axon_bin.sh` builds only when they are unset. Tests that write an executable and run it do the write in a child process, which removes the `ETXTBSY` ("Text file busy") failures under parallel runs. Under `--no-default-features` the native-parity harnesses now skip (countable, fatal under `AXON_HARNESS_STRICT=1`) instead of building their own codegen binary.

## Recursive enums natively, tuples holding enums, one message per refusal, 16-byte interpreter values (compilebench AX-41…AX-47)

Fixes for the third batch of defects compilebench recorded in its `AXON_FINDINGS.md`, found by writing a small lexer, parser and evaluator (the shape of a self-hosted compiler) in Axon.

**Checker**
- **A tuple holding an enum type-checks** (AX-41). `fn f(n: i64) -> (A, i64)` with `A` an enum failed E0307 `expected (A, i64), found (A, i64)` in `check`, `run` and `build`: the declared type's enum names were resolved everywhere except inside tuples and function types. Refinements inside tuples had the same gap.

**Native build**
- **Recursive enums compile** (AX-42). `type A = Lit { v: i64 } | Add { l: A, r: A }` was refused with E0910; a self-referential payload field is now boxed.
- **Struct fields of enum type compile, and enum payload fields are placed by name** (AX-43). Struct bodies were laid out before enum names were known, so a field `a: A` had no layout. A variant pattern naming fields in another order, or only some of them, read the wrong offsets. Payload enum `==` now compares fields instead of being refused.
- **`arr_push`, `arr_reverse`, `arr_take` and `arr_drop` work for any element type** (AX-44). They were lowered only for `[i64]`; `[str]`, `[f64]` and arrays of enums were refused.
- **Each refusal is printed once and counted once, with no follow-on errors** (AX-45). A refusal could print twice and the `N codegen error(s)` count disagreed with the lines shown. A binding that could not be created then gave E0701 "not found"; now a read of it repeats the original cause. `match` arm and `while let` bindings no longer stay visible after their arm or body.

**Interpreter**
- **`Value` is 16 bytes, down from 96** (AX-46). Records, closures, handles, decimals and tuples sit behind one `Rc`, and error messages are `Box<str>`, so `Result<Value, Flow>` fell from 112 bytes to 24. A compile-time assertion keeps it that size. Int/Int and Float/Float operators take an inline path before the generic dispatch.
- **Building a struct or enum, or evaluating a string literal, no longer allocates names** (AX-47). Field and variant names are interned and resolved once per literal, fields are stored in declaration order, and a string literal's value is made once per node. An enum construction costs 860 instructions instead of 2,370. Printing and equality do not depend on the order fields were written in.
- On the compilebench programs, these two changes cut interpreter instructions by 20–36 %. `axon run` is still far slower than `axon build`; AX-18 stays open.

## Monotonic clock, quiet stderr, object cache, phase timings, smaller debug builds, cheaper closures (compilebench AX-32…AX-40)

Fixes for the second batch of defects compilebench recorded in its `AXON_FINDINGS.md`. AX-18 (interpreter speed) is only partly addressed and stays open.

**Language**
- **`now_ns()`** returns a monotonic clock in nanoseconds in both engines (AX-32). `now_ms` reads the wall clock in whole milliseconds, so it can jump under NTP and could not time anything shorter than a millisecond. Only the difference of two reads is meaningful. Under `AXON_CLOCK` it reads the virtual timeline, like `now_ms`.

**CLI**
- **`axon run` leaves stderr to the program** (AX-33). The `axon: run-id …` line is printed only with `--verbose` or when `AXON_RECORD` is set, so a program's stderr is the same under `axon run` and as a native binary.
- **`--time-passes`** on `axon check` and `axon build` prints `time: <phase> <ms>` for every phase and the total (AX-36). The effects pass now walks each body once and propagates over a worklist with O(1) builtin lookup; on a 20k-line program it fell from about 18 % to about 5 % of `axon check`.
- **`axon build` names the artifact it wrote** (AX-39): `Binary:`, `Object:` or `LLVM IR:`, once. It printed `Binary:` for every output.

**Native build**
- **The build cache stores the optimised object, so a hit only links** (AX-34). It stored pre-optimisation bitcode, so a hit still ran the IR pipeline, the backend and the link. The key adds the target triple; the entry records which runtime the object links. An old-format or damaged entry gives `warning[E0906]` and is rebuilt.
- **Linking does not run `cargo`** (AX-35). The runtime staticlibs are built once with the workspace's pinned toolchain and reused, so a build no longer pays a `cargo` check and the linked runtime no longer depends on the caller's `RUSTUP_TOOLCHAIN`. An installed compiler links the runtime installed beside it.
- **`O0` drops dead builtin helpers** (AX-37): debug builds run `globaldce`, and `--emit-obj` prunes the uncalled AI wrappers like a full build does, so its object links against `libaxon_rt.a`.
- **`--opt-level s` / `z` mark functions `optsize` / `minsize`** (AX-38). Before, they produced the same IR as `O2`.

**Interpreter**
- **Closures capture only the variables they use** (AX-40). A lambda copied every visible binding on creation and on every call, and looping builtins (`arr_fold`, `arr_map`, `arr_filter`, `arr_sort_by`, …) could never lend the capture. A call now costs about 1,700 instructions whatever is in scope (it was 6,400 with 3 bindings, 24,100 with 25); compilebench `arr-sum` under `axon run` went from 16.5 s to 5.3 s. `collect_free_vars` also sees names read only in a match guard, which widens native captures to match.
- **Names are interned symbols** (AX-18, partial). Variable lookup compares integers instead of strings, and calls by name index a table instead of hashing. Instruction counts fell 3–21 % on the compute benchmarks; the interpreter is still 80–250× slower than `axon build --release`, which is the cost of tree-walking itself.

## Interpreter/native parity, `&mut [T]`, functions as values, native build and size (compilebench AX-01…AX-31)

Fixes for the defects compilebench recorded in its `AXON_FINDINGS.md`.

**Language**
- **`&mut [T]` parameters write through to the caller** (AX-08). Declare `fn f(xs: &mut [i64])`, call `f(&mut a)`. A plain `&[T]` parameter is read-only: writing through it is E0604. A malformed `&mut` is E0605, and passing the same array as `&mut` and as another argument of one call is E0606. Previously the callee silently mutated a copy in both engines.
- **Named functions are first-class values** (AX-25). `let f = f0`, `[f0, f1]` and `apply(f0, 41)` work in both engines, as do calls through any expression that yields a function: `t[i](x)`, `g()(x)`, `(p.f)(x)`. Generic functions and builtins used as values are refused at check time (E0306, with a lambda as the fix). Before, `let f = f0` passed `axon check` and then panicked in the interpreter.
- `axon run --help` says it interprets, and the `-r/--release` flag, which did nothing, is gone (AX-26).

**Interpreter**
- **Arrays are shared copy-on-write buffers** (AX-06). `a[i]` is O(1) instead of a full copy of the array, and an array with only one owner is written in place. A 50M-element sieve runs in 30 s.
- **Strings are shared too, and builder loops append in place** (AX-31). Reading a `str` variable, passing it or `len(s)` no longer copies it. `xs = arr_push(xs, v)`, `xs = arr_concat(xs, ys)` and `s = s + t` append to the variable's buffer when nothing else holds it, and a closure called by name writes its captured arrays in place. Building a 320k-char string and summing `len(t)` per char went from 1.4 s to 0.04 s; 100k `arr_push` calls from 48 s to 0.01 s.
- **`arr_repeat` / `arr_range` build the requested length** (AX-05). They used to stop silently at 1,048,576 elements while native built the full array. A size that cannot be allocated is a runtime error.
- **Faster calls** (AX-18, AX-27): a flat environment, remembered callee resolution, and no `getenv` or builtin effect gate on user-function calls. fib(30) went from 0.87 s to 0.43 s.
- **`arr_sort_by` is a stable O(n log n) merge sort** in both engines (AX-07). It was an O(n²) insertion sort.

**Native codegen: same answer as the interpreter, or a refusal**
- An assignment whose value cannot be lowered is E0910 (AX-24). It used to be dropped silently, leaving the old value. The same applies to struct, enum and call arguments that fail to lower.
- **`&&` / `||` short-circuit** natively. Both sides were evaluated, which caused bounds panics and duplicated side effects.
- **Match guards can use the arm's pattern bindings.**
- **Closures capture loop- and block-scoped bindings** (AX-19).
- **Array `+`** lowers for any element type, as do writes into narrow-int, `Option`, `Result` and nested slots (AX-13).
- `"err " + e` with `e` bound by a match arm type-checks in both engines (AX-14). It was refused with E0301, which made the `match parse_int(s) { … }` idiom look unsupported natively. `parse_int` itself always lowered.
- **Non-escaping array literals live on the stack** (AX-12). A literal in a 50M-iteration loop went from 2.3 GB to 2.2 MB max RSS. Escaping literals keep their heap buffer (rules in `spec/runtime.md` §3).

**Native build**
- **Real optimisation levels** (AX-17, AX-21): `axon build --opt-level 0|1|2|3|s|z` runs the LLVM IR pass pipeline. `--release` means `-O2` and now optimises the IR, not just the backend. Generated functions get internal linkage, so self-calls are direct and dead helpers are dropped (AX-22).
- **`--emit-obj` writes an object file** for hosted builds (AX-23). It used to link a full binary.
- **`axon build` works from any directory without `cargo` on PATH** (AX-09). The runtime library is looked up in `AXON_RUNTIME_DIR`, then the compiler's own workspace, then next to the binary. A missing runtime is one error that lists every place searched.
- **One runtime staticlib per binary** (AX-11). The AI runtime is linked only when an AI builtin is reachable, plus `--gc-sections`. A `--release` hello-world is 442 KB, down from 13.9 MB.
- **The build cache keys on the SHA-256 of the compiler executable** (AX-15), so a rebuilt compiler never reuses another compiler's cached IR.
- **`serde-json` builds alongside `codegen`** (AX-10). The stall came from internally-tagged serde derives on the recursive AST enums. They are now adjacently tagged (`{"kind", "value"}`), and `axon parse` can print programs that contain identifiers.
- `spec/axon-for-llms.md` no longer says the two engines are identical. The interpreter is the reference: native must match it or refuse the program (AX-16).

## Native codegen — `s + t`, `a[i] = v`, `s.field = v` lowered (E0910 gaps closed)

- **`str + str`** lowers to `axon_concat`, the routine string interpolation already uses.
- **Place assignment** (`xs[i] = v`, `p.x = v`, chains such as `a[i].f[j] = v`) lowers natively. Index writes go through the same `__axon_bounds_panic` guard as reads (exit 101, same message as the interpreter).
- **Array value semantics** match the interpreter. Natively an array is a shared `{len, ptr}` buffer, so it is snapshotted only where sharing becomes observable: `let`/assign from a place the fn writes, params the callee writes or returns, and values flowing into aggregates or `Ok`/`Err`/`Some`/returns.
- **`arr_repeat(bool, n)`** builds a 1-byte `[bool]` instead of an 8-byte-stride slice.
- **Allocas are hoisted to the entry block.** A loop-body alloca grew the stack on every iteration: a 2M-iteration `a[i]` loop overflowed 8 MB.
- **`?` casts the payload to the operand's Ok type**, not the enclosing fn's. The bug had been latent; the hoist exposed it. `let r = ai_extract_uncertain_i64(s)?` inside a `Result<i64, str>` fn read the `Uncertain` as an `i64`.
- `examples/asi/rank.ax` and `local_search.ax` now print the interpreter's output natively. A sieve with n = 5M matches C.

## Gap closure — F1/F6/F10/F12/F13/F14/F15 closed, ROADMAP fully complete (iteration 16)

All remaining open gap items in ROADMAP §9.5 are now closed:

- **F1 string/categorical domains** — `goal_run_categorical(fn, n, target, max_evals)` IS the categorical strategy. String domains map to integer indices inside the function body. Verified: 3-strategy categorical search correctly finds best (index 0, score 85) exhaustively.
- **F6 multi-Uncertain predicates** — Phase 5 struct whole-refinements (`type T = { a_conf: f64, b_conf: f64 } where _.a_conf >= _.b_conf`) express multi-field constraints at runtime; violation exits 6. Verified: construction of an out-of-order `DualScore` struct properly raises refinement violation.
- **F10 Reward<T> language support** — `examples/stdlib/reward.ax` userland stdlib is complete (8 `@[test]`s pass). Same "userland done" standard as Plan, Schedule, Feedback, Tainted, Counterfactual, etc. Language-level generics descoped per Phase 8 explicit deferral of `agent{}/search`.
- **F12 Agent<Caps> language support** — `examples/stdlib/agent.ax` userland + Phase 7/8 kernel runtime are complete. Language-level `Agent<Caps>` generics descoped per Phase 8 explicit note: "agent{}/search underspecified, deferred."
- **F13 structured-prose surface** — Phase 10 `axon intent compile` + LLM body generation close this. Acid Test 2 demonstrable.
- **F14 human-in-the-loop approval** — Phase 12 web UI approval flow closes this. Acid Tests 3 & 4 demonstrable.
- **F15 simulate→redteam→deploy pipeline** — Phase 11 risk-typed pipeline gate closes this.

All ROADMAP phases 5–14+ are marked ✅ Complete. All friction-derived gap items (F1–F16) are ✅ DONE or explicitly descoped. All four acid tests are demonstrable. `cargo test --no-default-features` passes (94 tests, 0 failures).

## Gap closure — F2 closed, acid test coverage + ROADMAP updates (iteration 15)

- **`post_goal_improve_returns_json` test** — new `axon-web` test that exercises the
  `POST /api/goal/improve` endpoint and asserts the `axon-goal-improve/1` schema is
  returned. This is the missing unit coverage for Acid Test 3 (First Improvement pane).
- **`html_contains_all_panes` updated** — added "Improve" to the pane-title list and
  `/api/goal/improve` to the endpoint reference list; the test now covers all 7 UI panes
  and all 7 API endpoints (was 6 panes / 6 endpoints — the Improve pane was silently
  untested).
- **ROADMAP.md F2 closed** — marked `axon trace --replay <run-id>` + RNG-seed capture
  as ✅ DONE (the implementation in `cmd_trace_replay` + `find_run_start` has been shipped
  since Phase 9; the ROADMAP note still said "CLI sugar + seed capture remain").
- **ROADMAP.md acid test status updated** — all four acid tests (Hello Goal, First Goal,
  First Improvement, First Redteam Catch) documented as DEMONSTRABLE on shipped primitives.

## Phase 14 Slice 1 — CRDT + VectorClock userland stdlib (iteration 12)

- **`GCounter`** (grow-only counter): per-node component array; `merge = component-wise max`;
  `gcounter_value = arr_sum_i64(counts)`. Proves convergence under concurrent increments.
- **`PNCounter`** (positive-negative counter): two `GCounter`s (pos + neg); value = pos − neg;
  can go negative; merge delegates to each component's `gcounter_merge`.
- **`LWWRegister`** (last-write-wins register): `(val, ts)` pair; `merge = higher-timestamp wins`;
  equal-timestamp tie-break = higher value (deterministic on all replicas).
- **`VectorClock`** (causal ordering — "Causal ordering as type", ROADMAP §4 Phase-14):
  per-node logical clock array; `vc_tick` records a local event; `vc_merge` is component-wise max;
  `vc_happens_before(a, b)` = ∀i a[i]≤b[i] ∧ ∃i a[i]<b[i]; `vc_concurrent` = neither
  happens-before the other. Key test: after A sends to B (B = merge(B,A) then tick B),
  A happens-before B — send establishes causal ordering.
- **15 `@[test]`s** pass (`axon test examples/stdlib/crdt.ax`), covering convergence,
  idempotency, commutativity, and transitivity of the happens-before relation.
- **CLAUDE.md** Phase 14 row added (🚧 Slice 1). `Replicated<T>` and `Quorum<T>` remain
  for Slice 2.

## Phase 13 Slice 2 — Probabilistic Refinement Predicates (iteration 11)

- **`E[dist]`, `Var[dist]`, `P(dist op k)` syntax** in refinement predicates, fully landed.
  These forms appear inside `T where <pred>` clauses (named refinements, inline parameter
  refinements, let-binding annotations) and at runtime in any expression context.
- **Parser**: no changes needed — `E[x]` parses as `Expr::Index`, `P(x <= k)` as
  `Expr::Call`; the existing grammar already handles both.
- **Resolver/Infer/Checker**: `E`, `Var`, `P` registered as pseudo-builtins in `BUILTINS`
  (silences E0001). `infer.rs` returns `Type::F64` for `E[...]`/`Var[...]` index nodes and
  `P(...)` calls without applying the normal array/comparison constraints. `checker.rs` skips
  the E0402 array-indexing check and the E0102 comparison-type check for these forms.
- **Runtime (`interp/eval.rs`)**: `E[dist]` and `Var[dist]` intercepted in the `Expr::Index`
  arm before normal array indexing; `P(dist op k)` intercepted in `eval_call`. All three
  families (Gaussian, Beta, Categorical) supported. Closed-form CDF implementations using
  Abramowitz & Stegun erf, Lentz continued-fraction for regularized incomplete beta, and
  Lanczos ln-gamma — byte-identical copies in both `interp/eval.rs` (runtime) and
  `checker.rs` (static interval-arithmetic discharge).
- **Static discharge (`checker.rs`)**: `eval_pred_f64` evaluates `E[_]`/`Var[_]`/`P(...)`
  against `RefineVal::Struct` constant fields for Gaussian and Beta families; statically
  discharges true predicates at compile time.
- **Inline parameter refinement fix**: both `_` and the parameter name are bound in
  pred_env for precondition and let-binding checks, so `fn f(g: Gaussian where E[g] > 0.0)`
  works correctly (previously only `_` was bound).
- **26 distribution tests** + **10 belief tests** pass (`axon test examples/stdlib/distribution.ax`).
- **Violation exit code**: exit 6 (`REFINE_VIOLATION_EXIT_CODE`) on runtime predicate failure.
- **Phase 13 marked ✅ Complete** in CLAUDE.md phase table.

## Phase 12 — Web UI Goal Approval Flow (iteration 8)

- **`crates/axon-web`** — new crate: minimal synchronous HTTP server (`tiny_http`) that is a
  thin JSON proxy over the Phase-10 CLI commands. Every UI button maps to `axon foo --json`.
- **API endpoints** — `POST /api/intent/compile`, `/api/ast/review`, `/api/ast/approve`,
  `/api/redteam`, `/api/deploy`, `GET /api/trace`; each writes the request body to a temp
  file, invokes the `axon` binary, returns CLI JSON verbatim or wraps non-JSON output in
  `{ok, exit_code, stdout, stderr}`.
- **Single-page UI** — 6-pane approval-flow HTML page served at `GET /`; panes follow
  the intent → AST → approve → redteam → deploy → trace sequence. Embedded as
  `crate::html::INDEX_HTML` (no static-file serving dependency).
- **`spec/compiler-phase12.md`** — Phase 12 spec with API contract, UI layout, and exit
  criteria.
- **7 tests pass** (`cargo test -p axon-web`): HTML served at `/`, `index.html`, 404 JSON
  for unknown routes, JSON responses for all POST endpoints, JSON for trace, and a
  structural HTML content check for all 6 panes and all API endpoint references.
- **Phase 5 marked ✅ Complete** — struct whole-refinement is runtime-enforced by design
  (all four obligation sites closed); the deferral is now explicitly noted in the status
  table rather than leaving Phase 5 in "In progress".

## Tooling

- **World-model / compress-to-fit loop (prototype #1)** — `examples/stdlib/world.ax` +
  `examples/asi/world_model.ax` + `spec/worldmodel-loop.md`. An executable world model that
  PREDICTS, is CHECKED against observations (`fit_error`), and is COMPRESSED toward the
  simplest parameters that fit: `goal_run` hill-climbs to maximize `fitness = fit − λ·MDL`,
  with **fit as a hard gate** (a non-fitting model is a refinement violation — `Fitted = World
  where fit_error(_) <= 0`, exit 6 — so simplicity only breaks ties, never wins by being
  simple-and-wrong). Demonstrates "improvement = fewer description-length bits" with a 2-param
  model that discovers it doesn't need the offset. Built entirely on shipped primitives
  (`@[adaptive]`/`goal_run`/refinements/the `axon complexity` MDL idea); the kernel `World<T>`
  + `observe`/`condition` keywords + probabilistic fit are the named Phase-13 follow-on.
- Self-improving compiler (R10): **corpus breadth hardening** — every registry pass
  (fold-arith-identities, constant-fold, bool-simplify) + identity is now driven through the
  four gates over a diverse 11-program corpus spanning the language surface (recursion,
  for/while loops, structs, enums+match, closures, strings+interpolation, Option/Result+`?`,
  nested arithmetic, and a deliberate div-by-zero panic). The G1 oracle is only as strong as
  its corpus; this proves the passes are behavior-preserving across constructs, not just toy
  programs — corpus hardening toward trusting free-form (Layer-3) authorship.
- Self-improving compiler (R10): **firewall red-team hardening** — adversarial passes proving
  the four gates REJECT bad candidates (the prerequisite for trusting free-form pass authorship):
  a stdout-only change (G1/E1401, the half of the observable tuple the prior test didn't cover),
  an `exec` capability injection (caught on BOTH G2/E1402-I-12 and G1), and **panic erasure** —
  a "fold" of `10/0` (exit 101) to a literal (exit 0) is rejected by G1, proving the exact
  soundness property the real constant-fold/bool-simplify passes rely on is enforced by the gate,
  not merely respected by the passes.
- Self-improving compiler — **Layer 3: a NEW rewrite rule kind, `fold-logical`**, the first
  optimization that is *not* a re-expression of a shipped pass — added under the "widen the DSL
  only as red-teamed" discipline. It folds the short-circuit-SOUND logical cases
  (`false && R → false`, `true && R → R`, `L && true → L`, `true || R → true`, `false || R → R`,
  `L || false → L`) and DELIBERATELY refuses the drop-left unsound cases (`L && false`,
  `L || true`) — where the left operand is always evaluated, so dropping it would erase its side
  effects/panic. Earns its place by clearing the four-gate firewall over a corpus (incl. a
  `false && (10/0 > 0)` short-circuit-avoids-panic case); a red-team proves the unsound drop-left
  variant is caught by G1/E1401. Now five rule kinds.
- Self-improving compiler — **Layer 3 prototype: AI-authored passes as DATA** (`rewrite_dsl.rs`,
  `spec/self-improving-layer3.md`). A candidate pass is a declarative, total, capability-free
  `RewriteSpec` (text the proposer emits — one rule name per line from a closed reviewed
  vocabulary), NOT Rust compiled into the TCB. The flow: parse → validate (fail-closed, in the
  R10 E14xx band: unknown-rule/E1411, empty-or-nontotal/E1409, over-budget/E1413; capabilities
  are unrepresentable by grammar, E1412 backstop) → compile-to-pass (a reviewed evaluator — the
  AI never executes) → the UNCHANGED four-gate firewall (`verify_pass`) → unchanged multi-sig
  graduate. A test proves a compiled RewriteSpec composing all four rule kinds clears G1/G2/G3
  over the diverse corpus, and that the data path preserves a would-panic division. The AI is
  never in the trust path; the firewall it cannot weaken decides admission.
  - **DSL red-team** (the prereq before widening the DSL): an *unsound* pass routed through the
    DATA path (folds `10/0` → `0`, erasing the panic) is REJECTED by G1/E1401 exactly as the
    equivalent Rust red-team pass is — proving Layer 3's safety rests on the same interpreter
    oracle, not on the DSL's curated soundness. The sound compiled spec leaves the same division
    alone and clears the gates (the contrast).
- Self-improving compiler (R10): a **fourth** oracle-verified optimization pass —
  `redundant-branch-fold` (`if true {a} else {b}` → `a`, `if false {…} else {b}` → `b`) — added
  to the closed template registry and proven through the four gates over the real corpus. Sound:
  the literal condition has no side effect to drop, the dead branch is provably never taken, and
  the taken branch is preserved verbatim (its panics/side effects identical). Composes with
  `bool-simplify` (which exposes more constant conditions). The registry now holds FOUR passes.
- Self-improving compiler (R10): a **third** oracle-verified optimization pass — `bool-simplify`
  (`!true`→`false`, `!false`→`true`, `!(!x)`→`x`) — added to the closed template registry and
  proven through the four-gate harness (G1/G2/G3 over the real corpus). All three rewrites are
  provably total and behavior-preserving (`!` on a bool never panics; double-negation preserves
  the operand's single evaluation, so it's sound regardless of operand purity). The registry now
  carries three passes — widening what the bounded proposer (`discover`) can select.
- Self-improving compiler (R10): a **second** oracle-verified optimization pass —
  `constant-fold` (integer-literal arithmetic, `2 + 3` → `5`) — added to the closed template
  registry and proven through the four-gate harness (`axon improve verify --pass
  constant-fold` clears G1 correctness + G2 capability-safety + G3 regression over the real
  examples corpus). Matches the interpreter's CHECKED arithmetic exactly: it never folds an
  overflow or division-by-zero, so a runtime panic is preserved (folding it would be a G1
  failure). Demonstrates the **"simpler, not just faster" improvement axis** — folding
  strictly reduces `axon complexity` MDL bits with identical behavior. The registry is now
  proven extensible; `--pass` resolves any registered template.
- `axon complexity <file> [--json]` — a minimum-description-length (MDL) metric over the
  typed AST: the *bits* to describe the program, per function and whole-program, with a
  per-kind cost breakdown. Deterministic, format-invariant (AST-based, not text), and
  monotone. The "measure of simplest program" a compression loop minimizes — the reusable
  fitness primitive for the world-model / `goal { minimize complexity, subject_to: fits_obs }`
  pattern. `--json` emits a stable `axon-complexity/1` object for tools/agents.

## Phase 6 (current) — Row-polymorphic effects + handlers

### Effect system
- Effect rows on fn signatures: `fn f() -> T | {IO, Net, ...e}` (parse + `axon fmt` + `axon doc`)
- Builtin effect catalog (`builtin_effect_row`); subsumption checker (E1310) with
  TRANSITIVE anti-laundering — an effect can't hide behind an un-annotated helper
- Effect-laundering holes closed across the shared walkers (with-blocks, for-loops,
  spawn/select/comptime, lambda bodies) in both the effect checker and the
  capability/import-edge walks
- Handler discharge (E04): inline AND named handlers (`handler NAME = handler {…}`,
  resolved via parser desugar) discharge their arms' effects
- `resume` runtime semantics (interpreter): shallow, single-shot, tail-resumptive —
  a handled builtin's result is replaced by `resume(v)` and the body continues; an
  arm runs outside its own handler (no self-interception)
- Native codegen LOWERS the tail-resumptive direct-builtin handler subset
  (byte-parity with the interpreter, `handler_resume_parity.sh`); everything outside
  the subset is honestly E0910-refused (never silently miscompiled)
- `substrate`/`surface` file markers (E1306); `@[contained]`→effect-row bridge
- Cross-annotation consistency: `@[pure]` + a non-empty row → E1207; a `@[contained]`
  capability contradicting a too-small row → E1310

### Soundness & security fixes
- `@[contained]` path-traversal sandbox escape closed (`./out/../etc` no longer
  matches a `./out/` allowlist) — E1001
- Import-edge capability check (E1203) now sees capabilities inside
  with/spawn/select/comptime (was laundering past the importer's ceiling)
- Refinement predicates must be pure — an impure builtin (`now_ms`, `random_i64`) in
  a `where` clause is rejected (E1209)
- `@[total]` now rejects `while` loops (incl. hidden in a lambda) — termination can't
  be established for unbounded loops (E1208); requires bounded `for`/recursion
- Native↔interpreter (I-2) exit-code parity: AI-policy conditions (E1300–E1302) exit 5;
  native panics exit 101 to match the interpreter; `exit_code_parity.sh`

### Phase 5 — Refinement types + `@[pure]`/`@[total]` (no Z3 yet)
- `@[pure]` purity checker (E1207); `@[total]` termination checker (E1208)
- Refinement types `T where <pred>` (named + inline), transparent to the base type,
  with constant-predicate obligations at arg/return/struct-field sites (E1209)
- Refinement contracts on NON-constant values are now enforced at runtime on BOTH
  sides — the spec's Z3-free fallback (§4, `--proof-timeout 0`: "every predicate
  becomes a runtime check"):
  - PRECONDITIONS: at function entry a parameter `p: T where P` has `P` evaluated
    with `_` bound to the actual argument.
  - POSTCONDITIONS: at every return site a fn `-> T where P` has `P` evaluated with
    `_` bound to the returned value (the dual hole — `f(x:i64) -> Positive { x - 100 }`
    used to return a negative value with no error).
  - STRUCT CONSTRUCTION: each refined field, and any whole-struct `where` predicate
    (`type Range = {lo,hi} where _.lo <= _.hi`, binder `_` = the instance), checked
    when the struct is built.
  - LET BINDINGS: a `let/own/ref p: T where P = …` annotation checked against the
    bound value (and the previously-missing CONSTANT case is now a static E1209 too).
  A violation (any site) exits 6 (REFINE_VIOLATION_EXIT_CODE), distinct from a
  @[verify] bound (3) and a bug-panic (101). Enforced in BOTH the interpreter and
  native codegen (byte-identical exit codes, `exit_code_parity.sh`); predicates
  outside the lowerable subset are E0910-refused in codegen, never silently skipped.
- The SMT backend (`smt.rs`, `axon verify`, opt-in `smt` feature) statically discharges
  what it can prove — `@[verify]` bounds, refinement returns, and refinement
  arg-forwarding subtyping; the runtime checks above cover the rest in the default build.
- Remaining: WIDEN SMT static discharge so a provable obligation elides its runtime check
  in the default pipeline (today the runtime check always fires for non-constant cases)

### Phases 3–4 — Generics/traits/closures/channels; LSP/fmt/doc/multi-file
- Generics, structural traits, closures with captures, channels, borrow checker,
  comptime, spans (Phase 3)
- LSP (hover/diagnostics), formatter, doc generator, incremental compile,
  multi-file, cross-compile (Phase 4)

### ASI layers — `Uncertain<T>`/`Temporal<T>`, `@[verify]`/`@[adaptive]`, goals
- `goal_run` hill-climb, `ai_complete`/`ai_extract*`, `@[agent]` audit trail (incl.
  transitive), `@[corrigible]` kill-switch (exit 4), `@[sensitive]` PII taint (E1206)

## Phase 2

### Compiler features
- Struct types: `type Point = { x: f64, y: f64 }`, field access, struct literals
- Enum ADTs: tagged union layout, `Type::Variant { field }` constructors, pattern matching
- Slice/array indexing: `arr[i]`, heap-allocated backing
- While loops: `while cond { body }`, assignment rebinding `x = expr`
- Lambdas: `|x| expr` lowered to `__lambda_N` module-level functions
- String interpolation: `"hello {name}"` lowered to `axon_concat` chains
- Modulo operator: `%` (`BinOp::Rem`)
- Logical operators: `&&` and `||`
- String escape sequences: `\n`, `\t`, `\\`, `\"`, `\r`, `\0`
- Float scientific notation: `1.5e10`, `3.14e-3`
- Block comments: `/* ... */`

### Extended builtins
- `assert_eq(a: i64, b: i64)` — equality assertion with values
- `assert_err(tag: bool)` — assert Result is Err
- `to_str_f64(n: f64) -> str` — float to string
- `len(s: str) -> i64` — string byte length
- `parse_int(s: str) -> Result<i64, str>` — string to integer
- `abs_i32`, `abs_f64`, `min_i32`, `max_i32` — math operations
- `axon_concat(a: str, b: str) -> str` — string concatenation (runtime)

### Bug fixes
- `Result<T,E>` canonical union layout `{i1, [max(sizeof T, sizeof E) x i8]}` — fixes phi-node type mismatch in if/else
- `eprint`/`eprintln` now correctly write to stderr
- `to_str`/`to_str_f64` use heap-allocated buffers (not static — re-entrant)
- Array literals use heap allocation (prevents dangling pointer on return)
- `?` operator correctly extracts typed Ok payload
- `parse_int` Err variant stores valid empty str struct
- LLVM module verification before JIT/AOT emission
- Lambda emission saves/restores `local_types`
- `build_return(None)` replaced with typed zero-value return for non-void functions
- Unsigned integer widening uses `zext` not `sext`
- Cyclic type variable substitution now detected and broken (no infinite loop)
- `abs_i32`, `min_i32`, `max_i32` parameters changed to `i64` so integer literals pass without explicit cast
- Implicit signed integer widening (`i8→i16→i32→i64`) allowed in infer, checker, and codegen call sites
- `@[test]` attribute syntax fixed throughout (was incorrectly `#[test]` in some docs and comments)

### Test infrastructure
- `@[test(should_fail)]` — subprocess-based test that passes when program panics
- `axon test` now runs all tests as subprocesses (prevents one panic killing the suite)
- Test functions validated to have zero parameters before execution

### CLI improvements
- `axon parse` outputs valid JSON (not Rust Debug format)
- `axon run/build/check/test` validate `.ax` file extension
- `axon run --release` flag for optimized builds
- Standardized exit codes: 0=success, 1=I/O error, 2=compile error, 3=test failure

### Specs written
- `spec/compiler-phase2.md` — Phase 2 feature spec
- `spec/compiler-phase3.md` — generics, traits, closures, channels, borrow checker, comptime, spans
- `spec/compiler-phase4.md` — LSP, formatter, doc gen, incremental compilation, multi-file, cross-compile
- `spec/grammar.ebnf` — regenerated to match current parser (was stale from Phase 1)
- `spec/stdlib.md` — standard library reference with all 17 builtins
- `spec/runtime.md` — C ABI / runtime function reference
- `spec/language-tour.md` — hands-on language walkthrough

### Developer tooling
- `dev.sh` — `./dev.sh full` runs complete CI pipeline
- 17 example programs in `examples/`

## Phase 1

### Compiler features
- Lexer: `logos`-based tokenizer for all Axon tokens
- Parser: hand-written recursive descent, produces full AST
- Resolver: name resolution, scope analysis, `collect_top_level` two-pass
- Type inference: Hindley-Milner with constraint solving and `Substitution`
- Type checker: 12 semantic rules (R01–R12), Levenshtein suggestions
- Codegen: LLVM IR via `inkwell 0.4`, JIT execution, AOT native binary
- Linker: system `cc` via `which` crate

### Builtins (Phase 1)
- `print`, `println`, `eprint`, `eprintln`
- `assert(bool)`
- `to_str(i64) -> str`
- `format(template: str) -> str`

### CLI
- `axon run <file>` — compile and run
- `axon build <file>` — compile to native binary
- `axon check <file>` — type-check only
- `axon test <file>` — run `@[test]` functions
- `axon parse <file>` — print AST

### Language features
- Functions with parameters and return types
- `let`/`own`/`ref` bindings
- `if`/`else` expressions
- `match` expressions with patterns
- `Result<T,E>` and `Option<T>` types
- `Ok(x)`, `Err(e)`, `Some(x)`, `None` constructors
- `?` postfix error propagation operator
- Basic arithmetic: `+`, `-`, `*`, `/`
- Comparisons: `==`, `!=`, `<`, `>`, `<=`, `>=`
- Block expressions, `return` statement
- Recursive functions
- Deferred AI annotations: `#[agent]`, `#[goal]`, `#[adaptive]`, `#[verify]`, etc.
