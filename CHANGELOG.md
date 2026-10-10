# Axon Changelog

## wasm32: deep values drop, deep source is refused, `host_await_val` copies nothing (compilebench AX-59, AX-60, AX-61)

Under wasmtime's default 512 KiB stack the wasm32 interpreter no longer traps (exit 134) on three shapes. Native behaviour is unchanged.

- **AX-59:** dropping a value nested 100,000 deep (an enum list, a dict of dicts, a closure chain) runs at most 16 levels of drop glue and defers the rest (`interp.rs` `bounded_drop`, `DictMap`); exit 0 under both engines.
- **AX-60:** the wasm32 front end refuses source nested deeper than 224 levels (native: 4,000, unchanged) with E0000 `expression nesting too deep (limit 224)`, exit 2, reported at the start of the statement or expression that holds it. Every recursive descent (expressions, primaries, patterns, types, `else if` links, match subjects and guards) is charged, and each root expression's AST height is checked, so an operator chain of more than 223 terms is refused too. A parenthesised or block nesting level charges two units, so those accept 110 levels.
- **AX-61:** `host_await_val`/`host_await_val_opt` on wasm32 no longer deep-copies a non-`str` payload that neither wasm substrate can send; it reports `no host driver` (exit 101). A `Chan` inside the payload is still refused first with the same path as native.

`scripts/wasm_stack_budget.py` checks both new bounds on each build (release: drop 16 × 1,024 B, parser 224 × 1,936 B per unit ≤ 448 KiB; debug: 16 × 2,416 B, 224 × 1,568 B). `scripts/vm_wasm_depth.sh` adds drop, `host_await` and front-end probes, and fails a nesting probe the front end refuses. `scripts/wasm_nesting_parity.sh` backs the new `cli_run.rs` tests.

## wasm32 deep recursion panics instead of trapping (compilebench AX-56)

On wasm32 the interpreter's recursion guard now fires at depth 128 instead of 450, and a native-stack byte budget backs it. Under wasmtime's default native stack (512 KiB) a recursion between about 136 and 318 deep trapped the module (`wasm trap: call stack exhausted`, exit 134) before the 450 guard could fire. A recursive call nested inside expressions, `with` bodies, handler arms, builtin callbacks or lambdas (several levels of each per recursion step) uses more stack per level and could trap far below any fixed depth. So on wasm32 every recursion through the interpreter is charged against a budget of `max_depth × 3584` bytes (448 KiB at the default guard): 54 functions carry a guard (52 in the interpreter, 2 in the bytecode compiler), chosen so every cycle through `Interp::eval` and through the bytecode compiler's `Compiler::expr`/`stmt` passes one, and each charges its own native frame plus the deepest chain of unguarded functions it can call (`nest_cost`, measured from the Cranelift x86-64 code of each profile). Exceeding the budget gives the same `recursion limit exceeded (128)` panic and exit 101 that native gives. Measured 2026-10-09, the shallowest default-stack depth of the four `tests/fixtures/vm_depth/` chains is 157 (tree, fn → closure → fn, debug, depth limit raised), above 1.2 × the guard (154). Under the default depth limit the budget, which charges the tree's fn and lambda levels what a VM level costs, stops the tree's closure chain before the guard, at 104 (debug) / 93 (release); the VM reaches 126. `scripts/vm_wasm_depth.sh` checks the budget on both builds under test (`scripts/wasm_stack_budget.py`: the call graph's cycle through `eval`, every frame and unguarded tail, no recursion through indirect calls), requires every default-stack chain depth to reach 1.2 × the guard, and runs 31 nesting probes (`tests/fixtures/vm_depth/nest/`: up to ten nested lambdas, `arr_fold` callbacks, `for` or `while let` loops per step, a chain of 160 distinct fns, a `break` through `call_fast`, a body that is one `Tree` op, a 240-deep expression compiled at depth, a 700-term operator chain at the bottom of the recursion in an assignment, a compiled body, a `with` body and a body too large to compile), each of which must panic rather than trap; with `--require-default-stack` the default engine (`vm`) must also complete at least the tree-walker's depth on every chain and probe. A host that gives the module a larger stack can raise both limits with `AXON_MAX_DEPTH`. Recursion over the depth of a value (dropping, comparing or formatting a deeply nested value) is outside the interpreter's call graph and not charged; a deep enough value still traps on wasm32 (compilebench AX-59, open: dropping a 3,000-node enum list traps the debug build), and so does the front end on deep source (compilebench AX-60, open: the parser at 273 nesting levels in release, the checker on an operator chain of 1,301 terms in debug and 832 in release) and recursion over one source construct's depth near the budget's edge (compilebench AX-61, open: copying a 700-term closure body for `host_await_val` traps at depth 67-77). Expression walks at run time no longer recurse (`ast::walk_expr` keeps its own stack), and an effect handler's frame points into the source instead of deep-cloning its arms and `with` body on every entry, a cost-only change to the tree-walker. The bytecode compiler's recursion is charged like the tree-walker's, and `scripts/wasm_stack_budget.py` fails on any other recursion reachable from `eval` that its `ALLOW` list does not classify (value depth, source depth, a fixed bound, std, panic path). A `break` or `continue` caught inside a compiled body no longer keeps a second op-loop frame live while the body runs on, so a recursion after it costs the VM no more stack than before it. Likewise a lambda body compiles in a frame that returns before the body runs, and the tree's lambda level is charged what a compiled VM lambda level uses, so recursion through lambdas alone (no fn level) no longer makes the default VM panic shallower than the tree. The checker also fits the constants of the compile frames (`compile_body`, `compile_lambda`), which sit outside every recursion: each now covers `compile` and the `Vec` growth under it (1,504 / 1,024 and 1,616 / 976 bytes, debug / release). Native is unchanged (6,000, no stack budget).

## `axon build` no longer runs cargo on every build after a no-op workspace config edit

The workspace runtime lib counted as stale whenever any input (a runtime source, a manifest, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/config.toml`) was newer than the lib. An edit cargo judges irrelevant, such as a comment in `.cargo/config.toml`, made cargo confirm the lib without rewriting it, so the lib stayed older than the edit and every later `axon build` ran cargo again. The runtime stamp now carries the time the confirming cargo run started (its mtime), and inputs older than that count as already judged (`codegen/link.rs` `runtime_freshness`).

## The bytecode engine compiles a body on its second entry unless it is hot (R50 S9)

Under `AXON_ENGINE=vm` (the default) a fn or lambda body now runs its first entry on the tree-walker and compiles on its second, unless it holds a `while`, a `while let`, a `for` that is not a literal range of at most 32 iterations, or a loop inside a loop; those still compile on their first entry. Code that runs once no longer pays a compile it never reuses (compilebench AX-58). Same stdout, stderr, exit code and provenance. `AXON_VM_EAGER=1` restores compile-on-first-entry, and `AXON_VM_TRACE=1` prints `vm: defer <name>` for each deferred first entry.

Instructions retired (`perf stat -e instructions:u`, release, core 6, tree / vm on the same binary): `big-compile` (1,002 fns, each called once) 768.78 M / 768.33 M, which was 1.076× the tree-walker before; fib-recursive 2.30 G, collatz 18.58 G, mandelbrot 5.07 G, arr-sum 4.88 G and qsort 11.40 G under vm, all still under CPython 3.14.4.

## `axon run` executes fn bodies on the bytecode engine by default (R50 S6)

`AXON_ENGINE` now defaults to `vm`; `AXON_ENGINE=tree` selects the reference tree-walker, which is unchanged. Same stdout, stderr, exit code, panic text and provenance under both (`scripts/vm_parity.sh`: 293 files, 1,899 bodies, 0 differ; `parity_all.sh` strict under each engine: 53 passed, 2 allowed skips). On wasm32-unknown-unknown `axon_set_engine` defaults to 1.

Instructions retired (`perf stat -e instructions:u`, release, compilebench programs; CPython 3.14.4 median in brackets): fib-recursive 2.30 G [3.42 G], collatz 18.54 G [26.06 G], mandelbrot 5.07 G [15.02 G], arr-sum 4.88 G [23.62 G], qsort 11.43 G [18.97 G]. Under wasmtime's default stack the VM completes at least the tree's depth on every chain of `tests/fixtures/vm_depth/` (`vm_wasm_depth.sh --require-default-stack`).

## `AXON_ENGINE=vm` compiles the scalar core of fn bodies to bytecode (R50 S1)

Second slice of the bytecode engine (`governance/specs/R50-register-vm.md`). Speed only: no behaviour change under either engine, and `tree` stays the default.

**Interpreter**
- **Under `AXON_ENGINE=vm`, fn bodies made of the scalar core run as flat ops** instead of one tree-walk: literals, identifier reads, blocks, `let`/`own`/`ref` (typed ones through the same `bind_let`), `=` to a local (AX-31 in-place appends first, as before), binary and unary operators (`&mut` aside), `if`, `while`, `for` over integer ranges, `return`/`break`/`continue`, `?`, `Some`/`None`/`Ok`/`Err`, string interpolation, and calls by name without `&mut` arguments. Every other node still runs on the tree-walker as one op. Comparisons in `if`/`while` conditions fuse with the branch; `x = l op r` and `while i < n` over locals and int literals are single ops that skip `Value` construction for int/float operands. An activation's operand stack lives in its pooled call frame. `vm: <fn> <n> ops, <k> tree nodes` (`AXON_VM_TRACE=1`) now reports the lowering: `fib` compiles with 0 tree nodes. The tree-walker's `If`/`While`/`For`/index arms now share `cond_bool` and `strict_int` with the engine (no change in what they do).
- A block, `while` iteration or `for` body that binds nothing (no `let`/`own`/`ref` anywhere below it) runs without its own scope under `vm`: that scope would stay empty, so it is unobservable.
- `scripts/vm_parity.sh` knows slice `S1` (its default): 284 files, 1,761 bodies, 25,090 lowered ops, 1,966 tree ops, 0 differ. `--repros S1`: `while` loop iteration 228 instructions (tree 1,150); a one-argument call 790 (budget 800; tree ≈ 910, was 1,072 and ≈ 1,106). A call by name resolves its callee once at compile time (`dispatch_named`), and `call_fn_in` binds parameters by index instead of draining the argument vector (cost only, both engines).
- **The shared call path is cheaper, cost only** (both engines; no change in what any call does). Call frames pool as `Box<Env>` and are cleared without drop glue for scalar bindings; `call_mut` takes its frame from that pool; the executing fn is a `Cell<Option<&FnDef>>` restored, with the call depth, by one guard; `call_fn_in` runs the common call (no attribute-driven step, no refinements) on a short path and pushes parameters and `goal_met` straight into the empty frame; under `vm` a call by name leaves its arguments on the operand stack, where a user-fn callee binds them from.

**Interpreter (S2: aggregates, index and field reads, place writes)**
- **Under `AXON_ENGINE=vm`, array, tuple and struct/enum literals, `xs[i]`, `p.f`, `t.N` and place writes (`xs[i] = v`, `p.f = v`, `g[i][j] = v`, `cfg.row[i] = v`) run as ops**, in the tree-walker's evaluation order and with its panic texts. `E[..]`/`Var[..]` reads stay one tree op. A place rooted at a call (`f()[i] = v`) evaluates the value and the index expressions, then panics `invalid assignment target` without evaluating the root, as before. `if a[j] < x` over locals (or against an int literal) is one fused read-compare-branch op, and a `for` iteration whose variable is alone in its scope rewrites it in place instead of popping and re-pushing the scope (unobservable).
- The `Index`, `FieldAccess`, `StructLit` and `AssignTo` arms now call extracted helpers (`index_in_place`, `index_value`, `field_in_place`, `finish_record`, `place_index`, `write_place`) that both engines share. No behaviour change: a `SizedInt` index still panics `expected i64, got i32` on a read and is accepted on a write.
- `scripts/vm_parity.sh` knows slice `S2`: 284 files, 1,761 bodies, 27,965 lowered ops, 662 tree ops, 0 differ. `--repros S1,part`: a partition-loop iteration (`part.ax`) 243 instructions (budget 250; tree ≈ 1,293); `while` loop 238, one-argument call 789.
**Interpreter (R50 S3: `match`, `while let`, method calls)**
- **Under `AXON_ENGINE=vm`, `match`, `while let` and method calls run as ops.** A `match` keeps its subject on the operand stack, and each arm calls `match_pattern` into the shared `Env`. A guard takes the arm only for a plain `true`, so an `Uncertain<bool>` guard is false and does not panic. The arm's scope is popped on every way out, including a guard that fails with `?`. A `while let` pushes the pattern's scope and then the body's for each iteration, as `run_loop_body` does. A method call evaluates the receiver first. A channel receiver then runs `chan_method`, which evaluates the argument nodes itself (only `send` evaluates one). Any other receiver evaluates its arguments left to right and calls `impl_method` with them still on the stack. An arm, `while let` pattern or `while let` body that binds nothing runs without its scope; an empty scope is unobservable.
- The `MethodCall` arm's channel dispatch and impl-method dispatch are now shared helpers (`chan_method`, `impl_method`); `call_fn` takes any `CallArgs`. No behaviour change.
- `scripts/vm_parity.sh` knows slice `S3`, which lowers `Match WhileLet MethodCall`; enum construction is a `StructLit` and belongs to S2's row. With S2 and S3 merged: 290 files, 1,791 bodies, 33,879 lowered ops, 230 tree ops, 0 differ. `--repros S1,part`: `while` loop 228, one-argument call 759, partition-loop iteration 226.
**Interpreter (R50 S4: lambdas)**
- **Under `AXON_ENGINE=vm`, a lambda is one op and its body compiles on its own** on its first run, kept with the lambda's shared code (`ClosureCode.compiled`), so `fold.ax`'s `main` and its `|acc, x| acc + x` run with 0 tree ops. A one-op scalar body (`acc + x` over int or float locals) is computed without entering the op loop. `AXON_VM_TRACE=1` names lambda bodies `<owner>::lambda#<i>` (source pre-order, nested lambdas included; `<fn>::verify::lambda#<i>` inside a `@[verify]` predicate; `<module>::lambda#<i>` for module `let`s, `refine` predicates and type refinements), printed once per body; a fn-value forwarder or other unresolved lambda prints `vm: tree <anon>: unresolved lambda` once per code instance. The `Expr::Lambda` arm is now the shared helper `make_closure` (no behaviour change).
- **A builtin -> closure call allocates nothing on the lent path, cost only** (both engines). `arr_fold`, `arr_map` and the other closure-taking builtins pass their one or two arguments straight into the parameters' scope instead of building a `Vec` per call, the closure runs on a pooled frame (bindings, scope marks and operand stack keep their capacity), and a call by local name drains its argument vector into the parameters and recycles it. The copied path (a closure with other references) still copies its capture cell per call. `arr_fold` copies an int element inline.
- `scripts/vm_parity.sh` knows slice `S4` (its default; lowers `Lambda`). With S2, S3 and S4 merged: 290 files, 1,894 bodies, 34,183 lowered ops, 120 tree ops, 0 differ. `--repros S1,part,fold`: `fold.ax` 330 instructions per element (budget 350; tree ≈ 485, was ≈ 916); loop 229, one-argument call 759 (budget 800), partition-loop iteration 227. The bytecode activation is inlined into each of its callers so the fn call path keeps its S1 cost.
**Interpreter (R50 S5: call costs)**
- **Under `AXON_ENGINE=vm`, a call by name to a plain fn skips `dispatch_call`** (cost only). A `CallFast` op is used when the callee is statically a fn-table entry, no local of that name is in scope, the name is not a builtin, there is no `tier:`, the program declares no refinement, and none of `is_agent`, `ai_metered`, `corrigible`, `adaptive`, `experiment`, `has_goal`, `has_ref_mut`, `is_main` or `has_epilogue` is set. The call keeps `call_fn_entry` -> `call_fn_in`'s steps and their order: a pooled frame, the depth check, the `current_fn` swap, the arity check, the parameter soft unwrap and sized-int coercion, `goal_met`, clearing a pending call tier, and the soft unwrap of a scalar return. All of these are restored on every exit. Arguments that are locals or literals are read straight into the callee's frame. A body that is one scalar operation, or one `a[i] = v`, runs without entering the op loop. `AXON_VM_TRACE=1` prints `vm: slow <fn>: <flag>` for each call that a flag keeps off the fast path.
- **A `&mut` call is a `CallMut` op and allocates nothing, cost only** (both engines). `call_mut` takes its frame and argument buffer from the pools, binds the arguments straight into the frame, and moves each borrowed binding back from the frame without a move-back buffer. Under `vm`, the borrowed locals are resolved at compile time and a statement call's `()` is not pushed. Behaviour is unchanged, including the unconditional write of `tier`. `Call(&mut)` is no longer a `tree-op` shape. A frame of scalars alone is now emptied by its length.
- `scripts/vm_parity.sh` knows slice `S5` (its default; lowers `Call(&mut)`). It reports 290 files, 1,894 bodies, 32,243 lowered ops, 111 tree ops and 0 differ. `--repros S1,part,fold,S5` gives these costs: a fast one-argument call 257 instructions (budget 300, was 759; tree ≈ 888, was ≈ 911), a one-`&mut`-argument call 562 (budget 600, was ≈ 1,856; tree ≈ 1,697, was ≈ 2,085), loop 233, partition-loop iteration 231, and `fold.ax` 341 per element.
**Interpreter (R50 S7: pure scalar regions)**
- **Under `AXON_ENGINE=vm`, side-effect-free scalar expressions run in registers** (cost only). A *pure tree* is built only from locals, `Int`/`Float`/`Bool` literals and binary operators. When it has two or more operators and is the value of a non-AX-31 `=` to a local, of an untyped `let`, or of an `if`/`while` condition, a `Pure` op computes it ahead of the generic ops, using register code specialised on the kinds the locals held on its first run. On any decline the generic ops evaluate the whole expression again, and they produce the panic or slow path exactly. Declines are: a local that is unbound or not a plain `Int`/`Float`/`Bool`, mixed kinds, overflow, `/ 0` or `% 0`, a non-`Bool` `&&`/`||` operand or branch value, and any operator other than `==`/`!=`/`&&`/`||` on two `Bool`s. A `while` whose condition and body (only untyped `let x = <pure>` and `x = <pure>`) are pure becomes a `PureLoop` op. It reads its locals once, iterates in registers without the iteration scope, and writes the assigned locals back when the condition goes false. On a decline it restores the iteration's start and hands that iteration to the generic loop. `arr_fold` over a closure whose compiled body is one pure tree runs the elements after the first without a closure call (`fold_leaf`), until an element declines. `AXON_VM_TRACE=1` adds `vm: pure <name> <p> exprs, <q> loops` after a body line, and `vm: fold-leaf <name>` once per lambda code.
- `scripts/vm_parity.sh` knows slice `S7` (its default; no new lowered variant). It reports 292 files, 1,897 bodies, 32,392 lowered ops, 111 tree ops and 0 differ. `scripts/vm_perf_gate.sh` gains `--programs SEL`, the `foldmod` row (`foldmod.ax`, arr-sum's `acc + x % m` body, budget 200), and the `loop_generic.ax` base: `loop.ax` plus a never-taken `if`, which `call1.ax` and `mutcall.ax` now carry too. The `call`, `fastcall` and `mutcall` rows subtract it, because `loop.ax` is now a `PureLoop`. Instructions under `vm`: mandelbrot 24.20 G -> 5.06 G (budget 15.02 G), arr-sum 34.18 G -> 4.88 G (budget 23.62 G), collatz 20.60 G -> 20.10 G. Per unit: `loop.ax` 92 per iteration (was 233), `foldmod.ax` 95 per element (was ≈ 675), `fold.ax` 58 (was 341), one-argument fast call 261, `&mut` call 565, partition-loop iteration 235.
**Interpreter (R50 S8: qsort and fib superops)**
- **Under `AXON_ENGINE=vm`, qsort's and fib's hot shapes are single ops** (cost only, no behaviour change). Every one of them takes the generic ops' path, in the same evaluation order, whenever its fast condition fails. The fused ops are:
  - `let t = a[i]` (`IndexDefine`) and `a[i] = a[j]` (`WriteIndexIndex`, which reads the value before the index);
  - `fib(n - 1) + fib(n - 2)`'s stack-plus-stack add (`BinStack`);
  - a fast call whose one argument is `local ± literal` (`CallFastLocalInt`);
  - a leading `if n < 2 { n }`, which returns before the rest of the body (`BranchReturn`). A fast call to such a body returns that arm's value without a frame.
- **A `&mut` call to a body made only of element moves within the borrowed array runs without a frame** (cost only). Examples are qsort's `swap` (`let t = a[i]; a[i] = a[j]; a[j] = t`, which becomes one swap) and `a[0] = i`. After its first call, the `CallMut` op caches the call resolved against its operands. It then moves the elements of the caller's binding in place, after checking every index. It declines, changing nothing, in all of these cases: the array is shared, an index is out of bounds or not an int, an operand is unbound, the depth limit is reached, or the callee is off `call_fn_in`'s common path. The full call then runs as before, panics included.
- `scripts/vm_parity.sh` knows slice `S8` (its default; no new lowered variant). It reports 293 files, 1,899 bodies, 32,375 lowered ops, 111 tree ops and 0 differ. `scripts/vm_perf_gate.sh` gains the `swapcall.ax` fixture and the `swap` row (`swapcall - loop_generic`, budget 730). Instructions under `vm`: fib-recursive 4.29 G -> 2.30 G (budget 3.42 G), qsort 26.01 G -> 11.43 G (budget 18.97 G). Per unit: `swap` call 1,283 -> 259, one-`&mut`-argument call 565 -> 275, collatz 18.54 G. All five programs and every repro row pass.

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
