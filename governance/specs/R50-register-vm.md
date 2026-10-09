# R50 — Bytecode engine for `axon run`

**Spec ID:** `R50-register-vm`
**Status:** Draft (revision 3). Two adversarial reviews (2026-10-09, `reviewer`, both verdict "incorrect":
first 2 blockers and 11 must-fix, second 4 must-fix and 8 smaller) are answered in §15; a third review of
this revision is pending.
**Risk class:** Structural (a second execution path for the reference engine)
**Author / date:** 2026-10-09, from compilebench AX-18 (interpreter cost) after AX-53..AX-55.

```spec-meta
id: R50-register-vm
status-claim: Draft
depends-on: none
blocks: none
blocked-by: none
supersedes: none
related: R2a-type-map-threading, R0-interp-module-split, R44-accumulating-session, R7-targets
conflicts-with: none
reserves: none (no new diagnostic or exit codes; env vars AXON_ENGINE and AXON_VM_TRACE are registered in env_registry.rs at S0)
evidence: none
```

R47–R49 are named as planned in `R46-in-flight-operations.md` §1, so this spec takes R50. It builds on
the AX-53/AX-54/AX-55 interpreter changes (frame slots, precomputed call flags, pooled frames and argument
buffers), which land on `main` before S0 in the same integration as this spec.

---

### 1. Motivation

`axon run` is a tree-walker over the AST. After AX-53..AX-55 it still retires 3.7× (fib-recursive), 2.7×
(collatz), 3.3× (mandelbrot), 2.5× (arr-sum) and 5.5× (qsort) the instructions CPython 3.14.4 needs for the
same algorithm (perf `instructions:u`, compilebench sources, release `--no-default-features` build). The
IPC is the same as CPython's, so the gap is work per operation. That work is per-node recursion through
`Interp::eval`, a node-address hash probe to resolve every name (`Resolution::var`), a
`Result<Value, Flow>` return per node, and a full `match` over 36 `Expr` arms per node. These are costs of
walking the tree, not of a hot spot that a local fix removes.

The interpreter is what the wasm playground, `axon test`, the goal optimizer, R10's equivalence oracle and
every program native codegen refuses (E0910) run on, so its speed is user-visible. Win: `axon run`
executes the five compilebench compute programs in fewer instructions than CPython, with byte-identical
observable behaviour.

### 2. Requirement link

`REQUIREMENTS.md` row **R50** (added with this spec): "the interpreter executes compute code within
CPython's instruction count". Acceptance anchor: `scripts/vm_perf_gate.sh` (§10) plus `scripts/vm_parity.sh`
(§8). It also closes compilebench AX-18 (S3): "the remaining gap is the tree-walking design itself;
closing it needs a bytecode VM or similar".

### 3. Surface (what the user writes)

No language change. Two environment variables:

| Var | Values | Effect |
|---|---|---|
| `AXON_ENGINE` | `tree` (default until S6), `vm` (default from S6) | Which engine runs fn and lambda bodies under every interpreter entry (`axon run`, `axon-run`, `axon test`, `axon goal`). On `wasm32-unknown-unknown`, which has no environment, the `axon_set_engine` export selects it instead (§4 Activation). Any other value: exit 2 with `AXON_ENGINE must be "vm" or "tree" (got "<v>")`. |
| `AXON_VM_TRACE` | `1` | One stderr line per fn or lambda body, the first time it runs: `vm: <name> <n> ops, <k> tree nodes`, or `vm: tree <name>: <reason>` when the whole body stays on the tree-walker. Off by default. It never changes stdout or the exit code. |

```text
$ AXON_ENGINE=vm AXON_VM_TRACE=1 axon run fib.ax
vm: main 9 ops, 0 tree nodes
vm: fib 11 ops, 0 tree nodes
2178309
$ AXON_ENGINE=vm AXON_VM_TRACE=1 AXON_DUMP_BINDINGS=/tmp/b.json axon run prog.ax
vm: tree main: binding capture
...
```

### 4. Semantics (what it does)

#### Fork 1 — where the engine keeps locals

- (a) A separate register file per activation (first draft). **Rejected by review.** `call_fn_in`
  binds params and `goal_met` into its `Env`. `call_fn_mut` reads `&mut` params back out of that `Env`
  (interp.rs `call_fn_mut`). Refinement postconditions evaluate against it. `main`'s binding capture
  snapshots it. `match_pattern` binds into it. Closure capture reads it in `LambdaInfo` order. A register
  file would need a handoff in every one of those places, and every missed handoff is a silent divergence.
- (b) **The compiled body runs against the same `&mut Env` the tree-walker would use. Chosen.** Since
  AX-53 every local has a resolver-assigned slot in its call's `Env`, and `Env::define_var/get_var/
  assign_var/get_var_mut(sym, slot)` index `vars[slot]` with a name check and a scan fallback. A compiled
  op carries its `(sym, slot)` pair, so a local read is the same indexed access the tree-walker does now,
  without the node-address hash probe. Temporaries live on an operand stack owned by the activation. All
  binding state stays where the reference engine keeps it, so every `Env` consumer keeps working
  unchanged.

#### Fork 2 — how much is lowered

The first draft compiled whole functions or none. **Replaced:** because the engine shares the `Env`, any
node can run on the tree-walker in place. The op `Tree(&'p Expr)` calls `Interp::eval(expr, env)` and
pushes the result. A body is compiled op by op, and a node outside the lowered set (§4 table) becomes one
`Tree` op. Nothing outside the lowered set has a second implementation, and coverage grows slice by slice
with no all-or-nothing cliff. A whole body stays on the tree-walker only in the cases listed under
"Activation" below.

#### Fork 3 — type knowledge

Values stay `Value` (16 bytes). Int and Float operands take an inline fast path: the shared
`value::int_binop` / `float_binop`, the same functions `eval_binop` uses since AX-55. Everything else goes
to `eval_binop_vals`. Unboxed typed registers are rejected for this spec: no per-node type survives
checking (`infer_program` returns only a `Substitution`, and production passes the checker an empty
`expr_types`), and `Uncertain<T>`/`Temporal<T>` are soft-compatible with `T` (infer.rs:2012), so an
`i64`-annotated local can hold a soft struct at run time (§12 Q1).

#### Reference semantics

The tree-walker stays the reference (I-2). The engine must be observationally identical: stdout, stderr,
exit code, panic text, host journal, provenance JSONL and audit ledger. Semantics are not re-implemented.
Ops call the functions the tree-walker calls: operators (`int_binop`, `float_binop`, `eval_binop_vals`,
`eval_unary` for non-`RefMut` ops), `values_equal`, display, `coerce_to_sized`, `soft_inner`,
`match_pattern`, `call_builtin`, `call_fn_in`, `call_closure_owned_by`. Where the tree-walker inlines logic
in an `eval` arm that the engine also needs, S0/S3 first extract it into a function the arm then calls
(no behaviour change), and both engines call that function:

| Extracted helper | From | Contents (in eval's order) |
|---|---|---|
| `dispatch_call(callee, argv, tier, env)` | `eval_call` after argument evaluation | `resume` handling (multi-shot replay or `Flow::Resume`); set or clear `current_call_tier` exactly as now; then 1. a local holding a `Value::Closure` *by runtime value* (`let max = 3; max(a, b)` still calls the builtin); 2. builtin (precedence over a same-named user fn), skipped through the `callees` memo for names already proven not to be builtins (eval.rs:1169-1190); 3. `call_fn_entry` (pooled frame, `has_ref_mut` refusal; interp.rs:3321-3343); 4. global closure; 5. the unknown-function panic |
| `bind_let(kind, name, slot, ann, value, env)` | `Let`/`Own`/`RefBind` arms (eval.rs:156-197) | refinement predicate check in a fresh `Env` binding `_` and the name, then `Flow::RefineViolation` on false; then sized-int coercion; then `define_var` |
| `chan_method(recv, method, args: &[Expr], env)` and `impl_method(recv, method, argv, env)` | `Expr::MethodCall` arm (eval.rs:453-505) | The arm evaluates the receiver first. For a `Value::Chan` receiver, `recv`/`try_recv`/`len`/`clone` and the unknown-method panic never evaluate the arguments, and `send` evaluates only `args[0]`; that path is `chan_method`, which takes the unevaluated argument nodes and evaluates them itself exactly as the arm does. Every other receiver evaluates all arguments left to right and then calls `impl_method`. The engine lowers a method call as: evaluate the receiver; op `MethodRecv` calls `chan_method` with the argument nodes when the receiver is a channel, and otherwise falls through to the compiled argument evaluation and `impl_method`. The receiver's type is known only at run time, so the branch is a run-time check. |
| `make_closure(lambda_expr, env)` | `Expr::Lambda` arm | `res.lambda(expr)` (else `LambdaInfo::of`); captures built from `env` in `captures` order |
| `assign_in_place(expr, env)` | `Assign` arm (eval.rs:198-215, 1550-1632) | already a function; the engine calls it as the arm does (§4 lowered set) |
| `call_mut(callee, args: &[Expr], env)` | `eval_call_mut` + `call_fn_mut` (eval.rs:1214-1261, interp.rs:3375-3401) | the `&mut` move-out / call / move-back protocol, unchanged in behaviour; S5 makes it allocation-free (pooled frame and buffers), which the tree-walker gets too |

#### Activation

- `call_fn_in` is the only way into a compiled fn body. At the point where it evaluates `f.body`, it calls
  `run_body(entry, &f.body, env)`. With `AXON_ENGINE=vm`, no whole-body exclusion and a table entry, that
  runs the compiled code against `env`; otherwise it calls `eval`. Everything before and after the body is
  unchanged: depth guard, arity check, `current_fn`, agent and AI-budget guards (AX-54's `FnEntry` flags),
  param coercion, refinement pre- and postconditions, goal training, provenance, and the soft unwrap of the
  result. Through S4 **every** call, VM→VM included, goes through `dispatch_call` → `call_fn_in`. S5's fast
  call is the only bypass, under the S5 conditions.
- Compiled fn bodies live in `FnEntry`: it gains `compiled: OnceCell<Body<'p>>`, filled on first run. That
  costs no lookup per call, so AX-54's removal of the `fn_of_def` probe stays intact. The owned
  `FnEntry::new(f, ..)` that `call_fn`/`call_fn_mut` build for a def missing from `fn_of_def`
  (interp.rs:3315, 3380) is not a table entry; its body runs on the tree. Fn bodies are `&'p Expr`, so their
  `Tree` ops borrow the program.
- Compiled lambda bodies live in their `ClosureCode`. The resolver owns the resolved copy of every lambda
  body (sym.rs:957-985: `body: body.clone()`, resolved in the clone); the AST original is never resolved,
  so only the clone has slots. `ClosureCode` gains `compiled: Option<OnceCell<Body>>`. It is `Some` only for
  codes built by `Resolution::lambda`, and `None` for `LambdaInfo::of` codes (run-time AST clones),
  `fn_value` forwarders and `SendValue` deep copies, whose bodies run on the tree. A compiled lambda body
  refers to nodes of that same `ClosureCode.body` by raw `*const Expr` (for `Tree` ops and literal
  operands), not by `&'p Expr`. This is sound because `ClosureCode` is only ever built inside an `Rc`
  (interp.rs:1945, eval.rs:90, sym.rs:307 and 977), its `body` is never mutated or moved afterwards, and
  `compiled` is dropped with it. No
  compiled code is stored in `Interp`, and `call_closure_owned_by` holds the closure's `Rc<ClosureCode>`
  for the whole activation. `call_closure_owned_by` runs the
  compiled body and keeps its arity panic, its lend-and-write-back of the capture cell and its
  `private_refs` accounting. Builtins that call closures (`arr_fold`, `arr_map`, ... through
  `call_closure_arg`) reach compiled lambda bodies the same way.
- Whole-body exclusions (trace reason in parentheses):
  - `main` when `capture` is true, i.e. under session capture, `AXON_DUMP_BINDINGS` or `AXON_DUMP_SHAPES`
    (`binding capture`);
  - an owned (non-table) `FnEntry` (`not in fn table`);
  - a `ClosureCode` with `compiled: None` (`unresolved lambda`).

  Bodies not reached through `call_fn_in` or `call_closure_owned_by` are not compiled at all. These are
  module `let`s, handler arms, refinement predicates, goal-metric expressions and continuation replays.
- Engine selection: `AXON_ENGINE` is read once, when the `Interp` is built. On `wasm32-unknown-unknown`
  there is no environment (`axon-wasm` exposes only `axon_alloc`/`axon_eval`/output getters,
  crates/axon-wasm/src/lib.rs:38-150), so S0 adds an exported `axon_set_engine(engine: u32)` (0 = tree,
  1 = vm). It is set before `axon_eval` and defaults to the build default. The wasm parity harness calls it
  for both engines. `wasm32-wasip1` reads `AXON_ENGINE` like native.

#### Execution

- Scopes: the engine emits `ScopePush`/`ScopePop` at exactly the points where `eval` calls `env.push()` /
  `env.pop()`: block, loop iteration, match arm, while-let, `for`. The resolver's slot numbering assumes
  those points. On **every** exit from a scope, whether normal, a caught `Break`/`Continue`, or an error
  propagating out of the body, the engine pops the scopes it pushed one at a time, innermost first, as
  `eval_block` and `run_loop_body` do on every `Err` (eval.rs:784-823). It never truncates to a recorded
  `marks.len()`. `call_fn_mut` depends on this: it reads `&mut` params back by reverse name scan after the
  body returns on any outcome (interp.rs:3387-3399), so a callee that left a shadowing `let a` scope in
  place would hand the caller the shadow.
- The tree-walker today leaks one mark when `match_pattern` or a guard returns `Err` inside a match arm,
  and when `match_pattern` errs in `while let` (eval.rs:306, 308, 358: `?` before `env.pop()`). S0 fixes
  that in the tree-walker first, so it pops on `Err` in those three places, with a cli test (guard that
  panics inside a `with` handler that resumes). The engine then has one rule, push/pop symmetry, and no
  leak to reproduce.
- `Tree` ops: a `Tree(&Expr)` op calls `eval`, which pushes and pops its own scopes symmetrically
  (after the S0 fix), so `env.marks` is the same after the op as before it on every outcome. The engine's
  scope count does not change across a `Tree` op.
- Evaluation order is the tree-walker's, arm by arm: operands left to right, arguments left to right
  into the pooled argument buffer (`take_args`), the callee dispatched by `dispatch_call` after its
  arguments.
- Calls with any `&mut` argument (`UnaryOp::RefMut`) go through `call_mut`, the reference protocol
  itself: through S4 as one `Tree` op over the `Call` node, from S5 as a `CallMut` op calling `call_mut`
  with the argument nodes. Because a compiled callee writes its params into its own `Env` and pops its
  scopes symmetrically, `call_fn_mut`'s read-back is unchanged. `&mut` anywhere else is already an
  `eval_unary` panic on the tree, and becomes a `Tree` op with the same panic.
- Errors: every op returns `Result`. On `Err(flow)`:
  - `Flow::Break` / `Flow::Continue` from any op inside a loop body, a callee or a `Tree` op included, is
    caught by the innermost enclosing loop of this body, exactly as `run_loop_body` catches them;
  - `Flow::Return(v)` ends the body with `Ok(v)`, as `call_fn_in` and `call_closure_owned_by` expect;
  - everything else propagates unchanged after the scope pops above. That includes `Panic`, `Exit`,
    `RefineViolation`, `SandboxViolation`, `Resume`, `HandlerDone`, and a `Break`/`Continue` outside any
    loop of this body. `call_fn_in`'s Drop guards restore `call_depth` and `current_fn` on every path, as
    now.
- Re-entrancy: the operand stack and argument buffers belong to one activation. Argument buffers come
  from AX-54's `arg_bufs` pool. Operand stacks get a pool of their own, added in S1, since AX-54 has none.
  No borrow of any engine structure is held across a call-out (`call_builtin`, `call_fn_in`,
  `call_closure_*`, `call_mut`, `eval`). Nested runs each have their own activation: a builtin calling a
  compiled closure, a handler arm calling a compiled fn, `goal_run`, scheduler fibers.
- Recursion: a compiled fn body is entered only through `call_fn_in` (or S5's fast call), which keeps the
  depth guard (`AXON_MAX_DEPTH`, default 6,000; 450 on wasm32). Lambda activations have no depth guard on
  either engine (`call_closure_owned_by` never touches `call_depth`). Their Rust frames sit between guarded
  fn calls, as they do on the tree. The requirement, for fn and lambda activations alike: the engine's
  Rust stack use per activation must not exceed the `eval` frames it replaces. §8's wasm32 alternating
  chain, fn → closure → fn past the limit, checks this.

#### Lowered set per slice (anything else is a `Tree` op)

| Construct | Slice |
|---|---|
| Int/Float/Bool/Str/Decimal literals (constants built at compile time), locals (`get_var` with the op's slot), block, untyped `let`/`own`/`ref`, typed ones through `bind_let` | S1 |
| `=` to a local: an `Assign` whose value has an AX-31 in-place shape (`x = x + y`, `x = arr_push(x, ..)`, `x = arr_concat(x, ..)`) is a `Tree` op in S1; any other `Assign` evaluates and calls `assign_var` | S1 |
| `BinOp` (`&&`/`\|\|` short-circuit as eval does, the Uncertain paths through `eval_binop_vals`), `UnaryOp` other than `RefMut`, fused compare-and-branch for `if`/`while` conditions | S1 |
| `if`, `while`, `for` over ranges and arrays, `return`, `break`, `continue`, `?`, `Some`/`None`/`Ok`/`Err`, `FmtStr` | S1 |
| calls without `&mut` arguments whose callee is an `Ident`, through `dispatch_call` | S1 |
| array/tuple/struct literals, index and field reads, `.N`, `AssignTo` place writes (`xs[i] = v`, `p.f = v`, chains), all three AX-31 shapes through `assign_in_place` | S2 |
| `match` and `while let` (via `match_pattern` into the shared `Env`), enum construction, method calls via `MethodRecv` (`chan_method` / `impl_method`) | S3 |
| `Lambda` via `make_closure`; compiled lambda bodies run by `call_closure_owned_by` | S4 |
| fast VM→VM calls; `CallMut` through an allocation-free `call_mut` | S5 |
| `with` handlers, `spawn`, `select`, `asm`, `E[..]`/`Var[..]`, `send`, `resume`; any `Call` whose callee is a `StructLit` (`Chan::new(n)`, `chan::<T>`, native `M::fn`; eval.rs:423-452); `P(..)` (intercepted before argument evaluation, eval.rs:1098) | `Tree`, permanently |

The compiler's `match` over `Expr` is exhaustive, with an explicit `Tree` arm for each variant that is not
lowered, so a new `Expr` variant does not compile until someone decides how to lower it.

#### S5 — call costs (required)

S1's per-call repro budget (800 instructions, §10) is the cost of the full `dispatch_call` → `call_fn_in`
path. fib-recursive's budget allows about 480 per call, body included (3.4 G / 7.05 M calls). qsort's
allows about 1,000 per call for its roughly 18-19 M `&mut` calls ([INFERENCE] Lomuto at n = 1 M:
≈ n·ln n in-loop swaps plus ≈ 2 M recursive calls). So S5 is a planned slice that S6 depends on.

**Fast calls.** A VM→VM call may skip `dispatch_call` → `call_fn_in` only when the callee is statically a
fn-table entry and nothing else can claim the name: no local of that name is in scope (by resolution),
the name is not a builtin, and there is no `tier:`. In addition, every one of these `FnEntry` flags must
be clear: `is_agent`, `ai_metered`, `corrigible`, `adaptive`, `experiment`, `has_goal`, `has_ref_mut`,
`is_main`, `has_epilogue`. The fn must also have no refinement predicates. A fast call still performs, in
the order of `call_fn_entry` → `call_fn_in` (interp.rs:3321, 3403):

1. a pooled frame `Env`;
2. the depth check, with the same message (interp.rs:3409-3416);
3. the `current_fn` swap;
4. the arity check, with the same panic (interp.rs:3446-3453); it comes after the depth check, so a
   too-deep call with the wrong arity reports the depth limit, as now;
5. per-parameter soft unwrap and sized-int coercion from `param_coerce`;
6. the `goal_met` slot define;
7. clearing `current_call_tier` when it is set;
8. the soft unwrap of the result for `ret_is_scalar`;
9. `recycle_args`;
10. restoring all of the above on every exit path.

Budget: at most 300 instructions per fast one-argument call (§10 `--repros`). S5 is accepted only with a
cli test per flag showing that a flagged fn never takes the fast path (trace line
`vm: slow <fn>: <flag>`).

**`&mut` calls.** `call_mut` becomes allocation-free. Today it allocates `argv`, `borrowed`, an unpooled
`Env::new()` and `outs` per call. After S5 it uses the frame and argument pools and a pooled move-back
buffer, with unchanged behaviour. The VM lowers `&mut` calls to a `CallMut` op over `call_mut`. Budget:
at most 600 instructions per one-`&mut`-argument call (§10 `--repros`).

#### Behaviour table

Each row is a parity case. "Same" means identical stdout, stderr, exit code, provenance JSONL and audit
ledger under `AXON_ENGINE=vm` and `AXON_ENGINE=tree`.

| Input class | Behaviour |
|---|---|
| program using only lowered constructs | trace shows 0 tree nodes for its fns; same output |
| fn mixing lowered and unlowered nodes | `Tree` ops for the unlowered nodes; same output |
| i64 `+ - *` overflow, `/` and `%` by zero, `MIN / -1`, float NaN/inf printing | same panic text and exit 101 / same text |
| `Uncertain`/`Temporal` operands, conditions, arguments and returns | same values and confidences |
| sized ints in `let`, params, struct fields | same coercion and overflow text |
| refined `let x: Pos = e` violating its predicate | exit 6, same message |
| relational return refinement in a fn that reassigns a param | same verdict |
| `&mut` calls: VM→VM, VM→tree, tree→VM; a panic inside the callee | caller's binding restored, same output |
| `break`/`continue` inside a callee fn or a closure run by `arr_fold`, inside a VM loop | terminates or continues the caller's loop, as on the tree |
| `let max = 3; max(a, b)`; a local closure shadowing a fn; a builtin name shadowing a user fn | same dispatch |
| `f(.., tier: "x")` followed by an un-tiered `ai_complete` in a VM fn (`AXON_AI_MOCK=1`) | same tier and cost records |
| `@[ai(policy(budget: 2))]` fn called repeatedly, with unmetered helpers between its calls | E1301 on the same call |
| `@[agent]`/`@[adaptive]`/`@[experiment]` fns calling capability builtins | same provenance JSONL and agent records |
| recursion past the limit: VM-only, tree-only, alternating chains | same message, exit 101 |
| shadowing, nested loops with labels-free `break`/`continue`, early `return` from nested blocks | same output |
| aliasing `let b = a; b[0] = 9` | `a` unchanged |
| closure made on VM, called by tree (`SendValue` through a channel, a handler arm), and the reverse; write-back to captures (T40); loop-scoped capture (AX-19) | same output |
| effect performed in a VM fn under a tree `with` handler, single-shot and multi-shot | same output |
| VM fn passing a compiled closure to `arr_fold` inside a `with` whose handler resumes multi-shot | same output (re-entrancy) |
| `AXON_DUMP_BINDINGS`, `AXON_DUMP_SHAPES`, an R44 session cell | `main` runs on the tree (trace says `binding capture`); same dump |
| `AXON_RECORD` under one engine, then `AXON_REPLAY` under the other | replay succeeds |
| `axon test`, `axon goal`, proptest, R10 oracle | same results |
| wasm32 (`axon-wasm`, `axon-run` on wasip1) | same as the native interpreter |
| `AXON_ENGINE=bogus` | exit 2 with the §3 message |
| `&mut` callee that shadows its param with an inner `let`, then panics, breaks out of the caller's loop, or returns early from the inner block | caller's binding gets the param, never the shadow; same output |
| `ch.recv(log("x"))`, `ch.len(f())`, `ch.send(a, b())` on a channel receiver; the same calls on a struct receiver | arguments evaluated exactly where the tree evaluates them; same output |
| `Chan::new(n)`, `chan::<T>()`, native `M::fn(..)`, `P(x)` | same output (`Tree` ops) |
| `s = s + t`, `x = arr_push(x, v)`, `x = arr_concat(x, ys)` in a 100k-iteration loop | same output; instructions linear in the iteration count (in-place append kept) |
| match guard or `while let` pattern raising inside a `with` handler that resumes | same output; scopes balanced (S0's tree-walker fix) |

### 5. Type rules

N/A. No type-system change.

### 6. Error codes

None. A malformed op stream is a host bug (`unreachable!`), caught by the parity gates. Failures the
user can reach are the tree-walker's existing `Flow`s and messages.

### 7. Invariants touched

Preserves:

- **I-1:** runs after the same pipeline, in the `Interp` slot.
- **I-2:** the tree-walker is the reference; every lowered construct has parity, and every unlowered one
  *is* the reference code.
- **I-4:** fn activations keep the same depth guard. Lambda activations have no guard on either engine,
  and their Rust frames sit between guarded fn calls on both. The engine's Rust stack use per activation
  is at most that of the `eval` frames it replaces, so the guard bounds the stack as it does now.
- **I-8:** exit codes come from the same `Flow` mapping.
- **I-9:** the same overflow and undefined-name errors.
- **I-10:** the same evaluation order and RNG draws.
- **I-11:** capability gates stay in `call_builtin`/`call_fn_in`.
- **I-13:** provenance stays in `call_fn_in`, and S5 excludes every zone fn.

Changes none. In S6, `ARCHITECTURE.md`'s I-2 paragraph (line 50, "There are two execution engines") gains
one sentence: the interpreter runs fn bodies either by walking the tree or by executing compiled ops
against the same frames, and the tree-walker is the reference for both.

### 8. Test plan

- [ ] Unit tests (`interp/vm/`): scope push/pop placement matches `eval` for each scoped construct, and
  every exit path (normal, caught `Break`/`Continue`, propagated `Err`) pops exactly the scopes pushed; an
  exhaustive `Tree` fallback for each unlowered `Expr` variant; `compiled` is `None` for `LambdaInfo::of`
  codes, `fn_value` forwarders and `SendValue` copies.
- [ ] S0 tree-walker fix: `match_pattern` / guard / `while let` errors pop their scope (eval.rs:306, 308,
  358), with a cli test that observes the leak today (a resumed handler reading a shadowed binding).
- [ ] CLI e2e (`tests/cli_run.rs`, prefix `vm_`): one test per behaviour row. Each runs the program under
  both engines and asserts identical stdout, stderr and exit code. Each also runs once with
  `AXON_VM_TRACE=1` and asserts the fns under test were compiled with the expected tree-node count, so
  parity is never vacuous.
- [ ] Parity corpus: `scripts/vm_parity.sh` runs every `examples/**/*.ax` that has a `main`, and every
  `tests/fixtures/**/*.ax` that `axon run` accepts, under both engines. Each run sets `AXON_AI_MOCK=1`,
  `AXON_SEED=42`, `AXON_CLOCK=0:1`, `AXON_AUDIT_DETERMINISTIC=1`, its own `XDG_CACHE_HOME` and
  `AXON_AUDIT_LEDGER`, and no `--verbose`. The script diffs stdout, stderr, exit code, the audit ledger and
  `provenance.jsonl`. Normalisation is one rule: drop the `ts_ms` and `run_id` keys from every provenance
  row. Both are wall-clock values the virtual clock does not reach (`now_ms` in interp.rs:4267,
  `generate_run_id` in main.rs:4329); every row has `ts_ms`, and the `run_start` row (provenance.rs:606-628)
  has `run_id`. It prints
  `vm_parity: <n> files, <c> bodies, <l> lowered ops, <t> tree ops, 0 differ`. It fails if any file
  differs, if `<l>` falls below `VM_PARITY_MIN_LOWERED`, or if `<t>` rises above `VM_PARITY_MAX_TREE`. Each
  slice raises the first and lowers the second in the script, so sending constructs back to `Tree` fails
  the gate even though the body count is unchanged.
- [ ] Whole suite: `cargo test -p axon-core` (both feature sets) with `AXON_ENGINE=vm` exported, so the
  CLI children inherit it, and again with `AXON_ENGINE=tree`; both green.
- [ ] Adversarial: recursion bombs (VM-only, tree-only, alternating fn → closure → fn); 1M-element
  arrays; deep block and loop nesting; big-compile's 20k-line source; the re-entrancy row.
- [ ] Record/replay across engines on the existing replay-test programs.
- [ ] wasm: `cargo check -p axon-core --target wasm32-unknown-unknown` and `wasm32-wasip1` with the
  features `CLAUDE.md` names. The wasm parity harnesses run both engines: `axon_set_engine` on
  unknown-unknown, `AXON_ENGINE` on wasip1. A new case on each target recurses through alternating fn,
  closure and fn activations past 450 and asserts the graceful exit-101 message.
- [ ] Red test first (S1): `vm_compiles_fib_with_no_tree_nodes`, asserting the trace line
  `vm: fib <k> ops, 0 tree nodes` (`<k>` is fixed when S1 lands). It fails on S0, where every body is a
  single `Tree` op.

### 9. Acceptance criteria

- [ ] `scripts/vm_parity.sh` exits 0 with 0 differing files, at the S5 lowered-op floor and tree-op
  ceiling.
- [ ] `cargo test -p axon-core --no-default-features` and `--features codegen` green under both
  `AXON_ENGINE` values.
- [ ] `scripts/vm_perf_gate.sh` exits 0 on the compilebench host (not a skip; see §10).
- [ ] `scripts/reference_gate.sh` in sync (two env vars registered).
- [ ] compilebench reruns the `axon-interp` row at the S6 commit and records the numbers in AX-18.

### 10. Performance budget

`scripts/vm_perf_gate.sh` builds `cargo build --release -p axon-core --no-default-features --bin axon`
(the workspace release profile: opt 3, thin LTO, 1 codegen unit). It runs each of the five programs in
`tests/fixtures/vm_perf/`, verbatim copies of compilebench's `benchmarks/<b>/axon/main.ax`, with
`AXON_ENGINE=vm` under `perf stat -e instructions:u -r 3 taskset -c ${VM_PERF_CPU:-6}`. It fails on any
median above budget, or on any output differing from the program's golden. If `perf` cannot count, it
exits **2** with "not measured"; that is a failure, not a skip.

| Program | Budget (CPython 3.14.4) | Tree-walker after AX-53..55 (merged, release) |
|---|---|---|
| fib-recursive | 3.4 G | 12.56 G |
| collatz | 26.1 G | 69.23 G |
| mandelbrot | 15.0 G | 49.47 G |
| arr-sum | 23.6 G | 59.98 G |
| qsort | 19.0 G | 103.61 G |

`--repros` mode gates per-construct costs on three programs in `tests/fixtures/vm_perf/`, each a
1M-iteration `while i < 1000000 { s = s + i; i = i + 1 }` loop. `loop.ax` is the bare loop; `call1.ax`
adds `s = s + id(i)` with `fn id(x: i64) -> i64 { x }`; `mutcall.ax` adds `touch(&mut a, i)` with
`fn touch(a: &mut [i64], i: i64) { a[0] = i }`. A construct's cost is the program's per-iteration count
minus `loop.ax`'s.

| Repro | Slice | Budget | Tree-walker (merged, release) |
|---|---|---|---|
| `loop.ax`, per iteration | S1 | ≤ 300 | 1,264 |
| one-argument call via `dispatch_call` → `call_fn_in` | S1 | ≤ 800 | ≈ 1,125 |
| one-argument fast call | S5 | ≤ 300 | n/a |
| one-`&mut`-argument call via `call_mut` | S5 | ≤ 600 | ≈ 2,607 |

The S1 call budget is for the general path, which every call keeps through S4 and flagged calls keep for
good. The program budgets need S5. fib(32) makes 7,049,155 calls, so 3.4 G leaves about 480 instructions
per call, body included: a 300-instruction fast call plus a body of about 10 ops. qsort makes roughly
18-19 M `&mut` calls and 28 M comparison-loop iterations [INFERENCE: Lomuto at n = 1 M]. At 600 per
`&mut` call that is about 11 G, which leaves about 8 G for the loop and the swap bodies. qsort is the
gate's tightest program, and the risk is named in §12.

sieve is reported but not gated: CPython clears multiples with one slice assignment that runs in C.

### 11. Rollout & rollback

S0–S5 ship with `AXON_ENGINE` defaulting to `tree`, so `main`'s behaviour is unchanged while coverage
grows. Each slice is a revertible commit series with its own gate. S6 flips the default to `vm` in one
commit; reverting it restores the tree default. `AXON_ENGINE=tree` remains the reference and the escape
hatch.

Blast radius if wrong: a lowering bug changes a program's output under `axon run`. The net is the parity
corpus, the whole suite under both engines, and the per-test trace assertions. Since an unlowered node is
the reference code, gaps cost speed, never correctness.

### 12. Open questions

- Q1 (non-blocking): unboxed `i64`/`f64` slots. This needs R2a's node→type map and a static proof that a
  slot cannot hold a soft value. Revisit with S6 profiles.
- Q2 (non-blocking): `call_builtin` is a `match name` (builtins.rs:800). If S6 profiles show it hot,
  resolve builtins to an index at compile time. That is a `call_builtin` refactor shared with the
  tree-walker, specified separately.
- Q3 (blocks R50.S6 only if it comes true): qsort or fib still misses its budget after S5 with every
  `--repros` budget met. Then the per-op cost of the S1-S3 ops is the gap. The response is a spec revision
  re-reviewed before S6 (for example an index-compare-branch superop for qsort's loop). The gate is never
  loosened in place.

### 13. Dependency DAG

| Node | Depends-on / blocked-by | Gate (named test or script) | Status |
|---|---|---|---|
| R50.S0 selector (`AXON_ENGINE`, `axon_set_engine`), trace, `run_body`, `FnEntry.compiled`, every body = one `Tree` op; extract `dispatch_call`, `bind_let`, `call_mut`; tree-walker scope-leak fix; `vm_parity.sh`; `vm_perf_gate.sh` | — | `cargo test -p axon-core --test cli_run vm_engine_` + `scripts/vm_parity.sh` (floor 0) | todo |
| R50.S1 scalar core and calls; operand-stack pool | R50.S0 | `cli_run vm_scalar_` + `vm_parity.sh` + `vm_perf_gate.sh --repros` (S1 rows) | todo |
| R50.S2 aggregates, place writes, AX-31 shapes | R50.S1 | `cli_run vm_aggregate_` + `vm_parity.sh` | todo |
| R50.S3 match, patterns, enums, methods; extract `chan_method`, `impl_method` | R50.S1 | `cli_run vm_match_` + `vm_parity.sh` | todo |
| R50.S4 lambdas; extract `make_closure`; `ClosureCode.compiled` | R50.S1 | `cli_run vm_closure_` + `vm_parity.sh` | todo |
| R50.S5 fast calls; allocation-free `call_mut` and `CallMut` op | R50.S2, R50.S3, R50.S4 | `cli_run vm_fastcall_` + `vm_parity.sh` + `vm_perf_gate.sh --repros` (all rows) | todo |
| R50.S6 default flip, docs | R50.S5; blocked-by Q3 only if it comes true | whole suite under both engines + `vm_perf_gate.sh` + `reference_gate.sh` | todo |

### 14. Evidence ledger

| Claim | Verify command | Expected | Last verified (commit @ date) | Result |
|---|---|---|---|---|
| (none yet) | | | | |

### 15. Review resolution (2026-10-09)

| Finding | Resolution |
|---|---|
| [blocker] `&mut` call protocol undefined; lowering `&mut a` as a unary panics | Calls with `&mut` are `Tree` ops over the whole call; the compiled callee writes its params into its own `Env`, so `call_fn_mut`'s read-back is unchanged (§4 Execution) |
| [blocker] Env↔register handoff (params, binding capture, postconditions, `&mut` read-back) | Fork 1(b): compiled bodies run on the same `Env`; `main` under capture runs on the tree |
| [must-fix] per-activation AI budget reset | No bypass of `call_fn_in` through S4; S5 requires `ai_metered` clear |
| [must-fix] borrowed register window across re-entrant builtins | Operand stack and argument buffers are per activation; no borrow across call-outs |
| [must-fix] typed `let` skips refinement | `bind_let` is shared by both engines |
| [must-fix] `match_pattern` needs an `Env`; no method helper | The `Env` is shared, so `match_pattern` is called as is; `call_method` extracted in S3 |
| [must-fix] closure representation and builtin-invoked closures | Same `ClosureVal`; `make_closure` shared; `call_closure_owned_by` runs compiled lambda bodies by `vm_id` |
| [must-fix] compiled-code key on `ClosureCode` address | Keys are fn-table and resolver-lambda indices; run-time clones and forwarders have `vm_id: None` |
| [must-fix] `Break`/`Continue` arriving from callees; unwind restores state | Loops catch them from any op in the body, as `run_loop_body` does; guards are `call_fn_in`'s |
| [must-fix] call op dynamic dispatch and `tier:` | `dispatch_call` extracted from `eval_call` and shared |
| [must-fix] parity script ignores stderr and side channels | Diffs stdout, stderr, exit code, provenance JSONL and audit ledger |
| [must-fix] perf gate binary unnamed; SKIP passes | Build named; missing `perf` exits 2 and fails acceptance; repro budgets are a `--repros` mode |
| [must-fix] no REQUIREMENTS row | Row R50 added |
| [must-fix] in-flight AX-53/54/55 prerequisite not in spec-meta | Merged with this spec; the spec is written against the merged code |
| [nit] wasm alternating-chain recursion | Added to §8 |

Second review (2026-10-09, `reviewer`, verdict "incorrect": 0 blockers, 4 must-fix, 8 smaller), answered
in revision 3:

| Finding | Resolution |
|---|---|
| [must-fix] provenance cannot match: wall-clock `ts_ms`, random `run_id` | Parity drops those two keys from every row, sets `AXON_CLOCK=0:1`, runs without `--verbose`; the stderr run-id normalisation is removed (§8) |
| [must-fix] qsort budget unreachable with `&mut` calls on the tree permanently | `call_mut` becomes allocation-free and the VM lowers `CallMut` in S5, budget ≤ 600 per call; "permanently" removed; qsort arithmetic and Q3 in §10/§12 |
| [must-fix] fib budget needs S5 | S5 is a required slice that S6 depends on, with a ≤ 300 fast-call budget; §10 explains the S1 800 budget |
| [must-fix] compiled lambda bodies are not `&'p` nodes | Compiled lambda code lives in `ClosureCode.compiled` and points into that `Rc`'s own immutable body; it is built only for resolver codes (§4 Activation) |
| `call_method` evaluates args the tree skips for channels | Split into `chan_method` (unevaluated `&[Expr]`, evaluated as the arm does) and `impl_method` (§4 helpers); behaviour row added |
| scope unwinding on every exit path | Pop one by one on every exit, never truncate; S0 fixes the tree's three leaks first; behaviour row added (§4 Execution, §8) |
| parity floor counts bodies | Floor on lowered ops plus ceiling on tree ops (§8) |
| `AXON_ENGINE` unreachable on wasm32-unknown-unknown | `axon_set_engine` export (§4 Activation); wasm harnesses run both engines |
| `run_body` has no fn-table index | `FnEntry.compiled: OnceCell`; owned entries run on the tree (§4 Activation) |
| StructLit-callee calls and `P(x)` missing from the `Tree` list | Named in the permanently-`Tree` row; S1 lowers only `Ident` callees |
| AX-31 in-place append covers `x = x + y` and `arr_concat` | S1 lowers AX-31-shaped `Assign`s as `Tree`; S2 lowers all three through `assign_in_place`; linear-cost row added |
| recursion claim false for lambdas; S5 omits arity and `recycle_args`; no operand-stack pool | I-4 reworded for lambda activations; S5 steps 4 (arity, after depth as in `call_fn_in`) and 9 added; S1 adds the pool; `dispatch_call` row names `call_fn_entry` and the `callees` memo |
