# R50 — Bytecode engine for `axon run`

**Spec ID:** `R50-register-vm`
**Status:** Implementing (S0, S1 landed). Reviewed at revision 10 after nine adversarial reviews (2026-10-09, `reviewer`). The first eight,
verdict "incorrect" (first 2 blockers and 11 must-fix, second 4 must-fix and 8 smaller, third 6 must-fix
and 6 smaller, fourth 7 must-fix and 5 smaller, fifth 5 must-fix and 7 smaller, sixth 5 must-fix and 6
smaller, seventh 2 must-fix and 3 smaller, eighth 2 must-fix and 2 smaller), and the ninth, verdict
"correct" (0 blockers, 0 must-fix, 4 nits), are answered in §15.
**Risk class:** Structural (a second execution path for the reference engine)
**Author / date:** 2026-10-09, from compilebench AX-18 (interpreter cost) after AX-53..AX-55.

```spec-meta
id: R50-register-vm
status-claim: Implementing
depends-on: none
blocks: none
blocked-by: none
supersedes: none
related: R2a-type-map-threading, R0-interp-module-split, R44-accumulating-session, R7-targets
conflicts-with: none
reserves: none (no new diagnostic or exit codes; env vars AXON_ENGINE and AXON_VM_TRACE are registered in env_registry.rs at S0)
evidence: scripts/vm_parity.sh; scripts/vm_wasm_depth.sh; scripts/vm_perf_gate.sh (budgets met from S6); spec review evidence is §15
```

R47–R49 are named as planned in `R46-in-flight-operations.md` §2 (that file is untracked in the main
checkout, so `verify_all_specs.sh` does not see it), so this spec takes R50. It builds on the
AX-53/AX-54/AX-55 interpreter changes (frame slots, precomputed call flags, pooled frames and argument
buffers), merged to `main` at `886aae53` together with this spec's first commit.

---

### 1. Motivation

`axon run` is a tree-walker over the AST. After AX-53..AX-55 it still retires 3.7× (fib-recursive), 2.7×
(collatz), 3.3× (mandelbrot), 2.5× (arr-sum) and 5.4× (qsort) the instructions CPython 3.14.4 needs for the
same algorithm (perf `instructions:u`, compilebench run `20261009T013441Z` against run `20261008T142344Z`'s
`python` cells; §10). The
IPC is the same as CPython's, so the gap is work per operation. That work is per-node recursion through
`Interp::eval`, a node-address hash probe to resolve every name (`Resolution::var`), a
`Result<Value, Flow>` return per node, and a `match` over 37 `Expr` variants (38 arms) per node. These are costs of
walking the tree, not of a hot spot that a local fix removes.

The interpreter is what the wasm playground, `axon test`, the goal optimizer, R10's equivalence oracle and
every program native codegen refuses (E0910) run on, so its speed is user-visible. Win: `axon run`
executes the five compilebench compute programs in fewer instructions than CPython, with byte-identical
observable behaviour.

### 2. Requirement link

`REQUIREMENTS.md` row **R50** (added with this spec), acceptance: "`scripts/vm_parity.sh` (both engines
byte-identical, incl. stderr, provenance and ledger) and `scripts/vm_perf_gate.sh` (instructions at or below
CPython's on fib-recursive, collatz, mandelbrot, arr-sum, qsort)". Acceptance anchor: `scripts/vm_perf_gate.sh` (§10) plus `scripts/vm_parity.sh`
(§8). It also closes compilebench AX-18 (S3): "the remaining gap is the tree-walking design itself;
closing it needs a bytecode VM or similar".

### 3. Surface (what the user writes)

No language change. Two environment variables:

| Var | Values | Effect |
|---|---|---|
| `AXON_ENGINE` | `tree` (default until S6), `vm` (default from S6) | Which engine runs fn and lambda bodies under every interpreter entry (`axon run`, `axon-run`, `axon test`, `axon goal`). On `wasm32-unknown-unknown`, which has no environment, the `axon_set_engine` export selects it instead (§4 Activation). Any other value: exit 2 with `AXON_ENGINE must be "vm" or "tree" (got "<v>")`. |
| `AXON_VM_TRACE` | `1` | Three kinds of stderr line. Per fn or lambda body, the first time it runs: `vm: <name> <n> ops, <k> tree nodes`, or `vm: tree <name>: <reason>` when the whole body stays on the tree-walker. Per `Tree` op compiled into a body: `vm: tree-op <name> <Variant>[(<shape>)]`, where `<shape>` is one of `Call(struct-lit)`, `Call(P)`, `Call(computed)`, `Index(E\|Var)` and, through S4, `Call(&mut)` (§8). From S5, per call that a flag keeps off the fast path: `vm: slow <fn>: <flag>` (§4 S5). `<name>` is the fn's name (`Type::method` for an impl method) or, for a lambda body, `<owner>::lambda#<i>`. `<owner>` is the enclosing fn; `<fn>::verify` for a lambda inside fn `<fn>`'s `@[verify]` predicate (resolved after the body, sym.rs:769-771); or `<module>` for a lambda the resolver reaches through a module item (a module `let`, a `refine` predicate or a type's refinement; sym.rs:614-622). `<i>` is the lambda's 0-based position among its owner's lambdas in source (pre-)order, nested ones included; for `<module>` the count runs over those items in source order. A code with `compiled: None` (`LambdaInfo::of`, `fn_value`, `SendValue`) has no name and no first-run state (`LambdaInfo::of` builds a fresh code per evaluation, eval.rs:722-730): it prints the literal line `vm: tree <anon>: unresolved lambda` once per code instance, and `vm_parity.sh` excludes those lines from its body count. Off by default. It never changes stdout or the exit code. |

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
in an `eval` arm that the engine also needs, the slice the §13 DAG names (S0–S4) extracts it into a
function the arm then calls (no behaviour change), and both engines call that function. The three
condition rules differ and stay different: `if` and `while` unwrap an `Uncertain<bool>` or `Temporal<bool>` with `soft_inner`
and panic on a non-bool (`cond_bool`, below); a match guard is true only for `Value::Bool(true)`, with no
unwrap and no panic, so an `Uncertain<bool>` or any other value is false (eval.rs:308); `for` bounds and
index reads take `Int` only (`strict_int`, below), while place writes also accept a sized int
(`place_index`):

| Extracted helper | From | Contents (in eval's order) |
|---|---|---|
| `dispatch_call(callee, argv, tier, env)` | `eval_call` after argument evaluation | `resume` handling (multi-shot replay or `Flow::Resume`); set or clear `current_call_tier` exactly as now; then 1. a local holding a `Value::Closure` *by runtime value* (`let max = 3; max(a, b)` still calls the builtin); 2. builtin (precedence over a same-named user fn), skipped through the `callees` memo for names already proven not to be builtins (eval.rs:1169-1190); 3. `call_fn_entry` (pooled frame, `has_ref_mut` refusal; interp.rs:3321-3343); 4. global closure; 5. the unknown-function panic |
| `bind_let(kind, name, slot, ann, value, env)` | `Let`/`Own`/`RefBind` arms (eval.rs:156-197) | refinement predicate check in a fresh `Env` binding `_` and the name, then `Flow::RefineViolation` on false; then sized-int coercion; then `define_var` |
| `chan_method(recv, method, args: &[Expr], env)` and `impl_method(recv, method, argv, env)` | `Expr::MethodCall` arm (eval.rs:453-505) | The arm evaluates the receiver first. For a `Value::Chan` receiver, `recv`/`try_recv`/`len`/`clone` and the unknown-method panic never evaluate the arguments, and `send` evaluates only `args[0]`; that path is `chan_method`, which takes the unevaluated argument nodes and evaluates them itself exactly as the arm does. Every other receiver evaluates all arguments left to right and then calls `impl_method`. The engine lowers a method call as: evaluate the receiver; op `MethodRecv` calls `chan_method` with the argument nodes when the receiver is a channel, and otherwise falls through to the compiled argument evaluation and `impl_method`. The receiver's type is known only at run time, so the branch is a run-time check. |
| `make_closure(lambda_expr, env)` | `Expr::Lambda` arm | `res.lambda(expr)` (else `LambdaInfo::of`); captures built from `env` in `captures` order |
| `assign_in_place(s, slot, name, value, env)` | `Assign` arm (eval.rs:198-209, 1550-1632) | already a function: it matches the AX-31 shapes by syntax, then returns `Ok(false)` without evaluating anything unless the local currently holds a `Str` or `Array`; the engine calls it first for every local `Assign`, exactly as the arm does (§4 lowered set) |
| `call_mut(callee, args: &[Expr], tier, env)` | `eval_call_mut` + `call_fn_mut` (eval.rs:1214-1261, interp.rs:3375-3401) | the `&mut` move-out / call / move-back protocol, unchanged in behaviour, including the unconditional write of `tier` to `current_call_tier` (eval.rs:1253); S5 makes it allocation-free (pooled frame and buffers), which the tree-walker gets too |
| `cond_bool(v, what)` (S1) | `If` and `While` arms (eval.rs:279-300, 323-348) | `soft_inner` unwrap of an `Uncertain` or `Temporal` (value.rs:41-44), then `Bool(b)` gives `b`; any other value panics `if condition must be bool, got <t>` or `while condition must be bool, got <t>` (`what` picks the word). The S1 fused compare-and-branch takes this path whenever the comparison does not yield a plain `Bool`. Not used for match guards |
| `strict_int(v)` (S1) | `eval_int` (eval.rs:1634-1638) | `Int(n)` gives `n`; anything else, a `SizedInt` included, panics `expected i64, got <t>`. `for` (S1, eval.rs:373-400): evaluate `start` and pass it through `strict_int`, then evaluate `end` and pass it through `strict_int` (eval.rs:380-381; a bad `start` panics before `end` runs), before the variable is bound; then per iteration the `i <= e` (inclusive) or `i < e` test, `env.push()`, define the variable, the body as `run_loop_body` runs it, `env.pop()`, `i += 1` |
| `index_in_place(s, slot, name, idx, env)` and `index_value(arr, idx)` (S2) | `Index` arm (eval.rs:548-574) | The arm's two paths stay two ops; both convert the index with `strict_int` (eval.rs:551, 568), so a `SizedInt` index panics on a read although `place_index` accepts it on a write. An `Ident` receiver that is bound locally or in `globals` is read in place: the engine evaluates the index only after that existence test, converts it, then calls `index_in_place`. Any other receiver, an unbound `Ident` included (which then runs the `Ident` op's chain: `fn_of_sym`, then the undefined-identifier panic), is evaluated first, then the index, converted, then `index_value` with the `i64`. Same bounds and non-array panic texts |
| `field_in_place(s, slot, name, f, field, env)` (S2) | `FieldAccess` arm (eval.rs:506-524) | An `Ident` receiver: `get_var`, then `globals`, then the undefined-identifier panic (no `fn_of_sym` step, unlike the `Ident` arm), then `field_of`; any other receiver is evaluated, then `field_of` |
| `finish_record(expr, name, vals, env)` (S2) | `StructLit` arm (eval.rs:585-695) | `res.record_lit(expr)` (else `RecordLit::of`) is known at compile time. When its `empty` is set (a literal with no fields and no whole-struct refinement, sym.rs:533-541) the engine emits that value as a constant and calls nothing. Otherwise the caller evaluates the field expressions in source order (the arm's order) into `vals`, and the helper does the rest exactly as the arm: slot placement, per-field sized-int coercion, enum versus struct, then the per-field and whole-struct refinement checks (`Flow::RefineViolation`, exit 6) |
| `place_index(v)` and `write_place(base, slot, steps, value, env)` (S2) | `AssignTo` arm (eval.rs:216-270) and `flatten_place` (interp.rs:4136-4163) | The value is evaluated first. Then the index expressions of the place, in `flatten_place`'s order (outermost `Index` node first, walking toward the base: `g[i][j] = v` evaluates `j` before `i`), each converted by `place_index` (`as_int`, then the `negative index` panic). `write_place` is phase 2: `get_var_mut` with the undefined-variable panic, the copy-on-write walk and its panic texts. A place whose root is not an `Ident` (`f()[i] = v`, which `axon check` accepts; the parser takes any `Index`/`FieldAccess` as a place, parser.rs:1808-1817) compiles to the value, then the index expressions down to that root through `place_index`, then `Panic("invalid assignment target")` (`flatten_place`'s `_` arm, interp.rs:4158); the root is never evaluated. So every `AssignTo` lowers in S2 and none is a `Tree` op |

#### Activation

- `call_fn_in` is the only way into a compiled fn body. At the point where it evaluates `f.body`, it calls
  `run_body(entry, &f.body, env)`. With `AXON_ENGINE=vm`, no whole-body exclusion and a table entry, that
  runs the compiled code against `env`; otherwise it calls `eval`. Everything before and after the body is
  unchanged: depth guard, arity check, `current_fn`, agent and AI-budget guards (AX-54's `FnEntry` flags),
  param coercion, refinement pre- and postconditions, goal training, provenance, and the soft unwrap of the
  result. Through S4 **every** call, VM→VM included, goes through `dispatch_call` → `call_fn_in`. S5's fast
  call is the only bypass, under the S5 conditions.
- Compiled fn bodies live in `FnEntry`: it gains `compiled: Option<OnceCell<Body<'p>>>`, `Some` only for
  the entries `Interp::build` (interp.rs:2982) pushes into `fn_table` (interp.rs:3045) and filled on first run. That costs no
  lookup per call, so AX-54's removal of the `fn_of_def` probe stays intact. `FnEntry::new` builds table
  and owned entries the same way (sym.rs:373-414), so the `Option` is the discriminator: the owned
  `FnEntry::new(f, ..)` that `call_fn`/`call_fn_mut` build for a def missing from `fn_of_def`
  (interp.rs:3315, 3380) gets `compiled: None` and its body runs on the tree. Fn bodies are `&'p Expr`, so
  their `Tree` ops borrow the program.
- Compiled lambda bodies live in their `ClosureCode`. The resolver owns the resolved copy of every lambda
  body (sym.rs:957-985: `body: body.clone()`, resolved in the clone); the AST original is never resolved,
  so only the clone has slots. `ClosureCode` gains `compiled: Option<OnceCell<Body>>`. It is `Some` only for
  codes built by `Resolution::lambda`, and `None` for `LambdaInfo::of` codes (run-time AST clones),
  `fn_value` forwarders and `SendValue` deep copies (built by `SendValue::into_value`, interp.rs:1945, on
  the reply of `host_await_val`/`host_await_val_opt`, builtins.rs:3120, 3136), whose bodies run on the tree. A compiled lambda body
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
  and when `match_pattern` errs in `while let` (eval.rs:306, 308, 358: `?` before `env.pop()`). The
  enclosing scope's single pop on that `Err` removes the arm's mark instead of its own, so the enclosing
  scope's bindings stay visible. compilebench AX-57 shows it: in a fn whose body shadows its `&mut` param
  with `let a = 99`, a `?` failing in a match guard makes `call_fn_mut` read the shadow back, so `axon run`
  panics (`len: expected str/array/dict, got i64`) where the native build prints `3`. S0 fixes the three
  places in the tree-walker first, so each pops on `Err`, with AX-57's program as the cli red test
  (`vm_engine_scope_leak`, failing before the fix). The engine then has one rule, push/pop symmetry, and
  no leak to reproduce.
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
  fn calls, as they do on the tree. The engine cannot use less stack than the tree everywhere: wherever a
  call is made inside a `Tree` op (S0 bodies, `&mut` calls through S4, calls inside `with` bodies), the
  `run_body` and VM exec frames sit on top of the same `eval` frames the tree uses. On wasm32 an overflow
  traps the module, which I-4 forbids, and two stacks bound the depth there:
  - The 64 MiB linear-memory stack (`.cargo/config.toml` `-zstack-size`). Measured 2026-10-09 with
    `axon-run` at `886aae53` on `wasm32-wasip1` under wasmtime 49.0.0, with the host's native stack and the
    guard both lifted (`-W max-wasm-stack=4294967295`, `AXON_MAX_DEPTH=1000000`), the deepest depth the
    tree-walker completes before a memory fault is, debug / release: plain fn recursion 1,226 / 12,085;
    fn → closure → fn 1,011 / 9,574; `&mut` recursion (qsort's shape) 1,884 / 18,638 (the committed
    `vm_depth/mut.ax`, which recurses at the top level as qsort does, completes 2,789 / 25,571); recursion inside a
    `with` body 889 / 9,018. The tightest margin over 450 is 1.98× (`with`, debug). The requirement: under
    `AXON_ENGINE=vm`, in both profiles, every chain completes depth 585 (1.3 × 450) under that
    configuration, and with the guard in place (`-W max-wasm-stack=4294967295`, `AXON_MAX_DEPTH` unset)
    reports the exit-101 depth panic at 450.
  - The wasm engine's own native stack. Under wasmtime's default `max-wasm-stack` the tree-walker already
    traps before 450 on all four chains (deepest completed 136–318; compilebench AX-56). R50 does not fix
    that, and must not make it worse once the VM is the default. Through S5 the script only *reports* each
    chain's deepest completed VM depth under the default configuration next to the tree's, and a shortfall
    there does not fail a slice: at S0 every level of every chain runs `run_body` and the exec loop on top
    of the tree's own `eval` frames (the tree completes 260 on `plain.ax` in debug), so the VM starts
    below the tree until lowered ops replace `eval` frames. S6 does not flip the default while any VM
    depth is below the tree's: the S6 gate runs the script with `--require-default-stack`, which fails on
    any such chain (§13; §12 Q4 if it cannot be met).

  `scripts/vm_wasm_depth.sh` checks both. It builds `axon-run` for `wasm32-wasip1` in both profiles from
  the commit under test, runs the four chain programs in `tests/fixtures/vm_depth/` (`plain.ax`,
  `closure.ax`, `mut.ax`, `with.ax`; each recurses to the depth in its first line, `let DEPTH = 100`, which
  the script rewrites in a temporary copy per probe) under both engines of that same build, bisects the
  deepest completed depth in each configuration, checks the panic at 450, and prints one line per
  (profile, engine, chain, configuration). It fails when a chain misses 585 in the linear stack or the 450
  panic is wrong, and with `--require-default-stack` also when a default-stack VM depth is below the
  tree's. There is no run-time refusal: a slice that fails the script does not land.
  `wasm32-unknown-unknown` runs the same code with the same linear-stack setting; its native stack belongs
  to the browser and is AX-56's, not this gate's.

#### Lowered set per slice (anything else is a `Tree` op)

| Construct | Slice |
|---|---|
| Int/Float/Bool/Str/Decimal literals (constants built at compile time); every `Ident` read, as one op that runs `eval`'s whole chain: `get_var` with the op's slot, then `globals`, then `fn_of_sym` (a fn as a value, `fn_value`), then the undefined-identifier panic (eval.rs:121-141); block, untyped `let`/`own`/`ref`, typed ones through `bind_let` | S1 |
| `=` to a local: an `AssignInPlace` op calls `assign_in_place` with the value node first, exactly as the arm does (eval.rs:198-209). It returns `Ok(false)` after a syntactic match and one `get_var`, without evaluating anything, unless the local holds a `Str` or `Array`, so `i = i + 1` and `s = s + i` on ints fall through to the compiled value and `assign_var`. A non-AX-31-shaped `Assign` skips the op | S1 |
| `BinOp` (`&&`/`\|\|` short-circuit as eval does, the Uncertain paths through `eval_binop_vals`), `UnaryOp` other than `RefMut`, fused compare-and-branch for `if`/`while` conditions | S1 |
| `if`, `while`, `for` (integer ranges; the only `for` form, ast.rs:400-410), `return`, `break`, `continue`, `?`, `Some`/`None`/`Ok`/`Err`, `FmtStr` | S1 |
| calls without `&mut` arguments whose callee is an `Ident`, through `dispatch_call` (other callees: see the last row) | S1 |
| array/tuple/struct literals, index and field reads, `.N`, `AssignTo` place writes (`xs[i] = v`, `p.f = v`, chains) | S2 |
| `match` and `while let` (via `match_pattern` into the shared `Env`), enum construction, method calls via `MethodRecv` (`chan_method` / `impl_method`) | S3 |
| `Lambda` via `make_closure`; compiled lambda bodies run by `call_closure_owned_by` | S4 |
| fast VM→VM calls; `CallMut` through an allocation-free `call_mut` | S5 |
| `with` handlers, `spawn`, `select`, `asm`, `resume` (only inside `with` arms, which are `Tree` ops as a whole); an `Index` whose receiver is the identifier `E` or `Var` (eval.rs:536-545, the whole node, so both the moment path and its fall-through stay the tree's); any `Call` whose callee is a `StructLit` (`Chan::new(n)`, `chan::<T>`, native `M::fn`; eval.rs:423-452); `P(..)` (intercepted before argument evaluation, eval.rs:1098); any `Call` whose callee is neither an `Ident` nor a `StructLit` | `Tree`, permanently |

The compiler's `match` over `Expr` is exhaustive, with an explicit `Tree` arm for each variant that is not
lowered, so a new `Expr` variant does not compile until someone decides how to lower it.

#### S5 — call costs (required)

S1's per-call repro budget (800 instructions, §10) is the cost of the full `dispatch_call` → `call_fn_in`
path. fib-recursive's budget allows about 485 per call, body included (3,423,642,492 / 7,049,155 calls). qsort makes
15,004,221 `&mut` calls (13,670,662 `swap` and 1,333,559 `quicksort`, `main`'s included) and
25,092,348 partition-loop iterations (counted by a Python simulation of the benchmark source, same LCG
and pivot rule). So S5 is a planned slice that S6 depends on.

**Fast calls.** A VM→VM call may skip `dispatch_call` → `call_fn_in` only when the callee is statically a
fn-table entry and nothing else can claim the name: no local of that name is in scope (by resolution),
the name is not a builtin, and there is no `tier:`. In addition, every one of these `FnEntry` flags must
be clear: `is_agent`, `ai_metered`, `corrigible`, `adaptive`, `experiment`, `has_goal`, `has_ref_mut`,
`is_main`, `has_epilogue`. `refine_preds` must also be empty program-wide, the same test `call_fn_in` uses
for preconditions and for routing to `finish_call_cold`, where the return-type postcondition is checked
(interp.rs:3532-3561, 3634, 3750-3772). A fast call still performs, in
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
`vm: slow <fn>: <flag>`), for every flag above except `has_ref_mut`. That flag cannot be reached by a
checked program: E0605 (mut_borrow.rs:16-21) rejects a plain argument for a `&mut` param and a
`&mut`-taking fn used as a value, so every call to such a fn has a `&mut` argument and is a `CallMut`,
never a fast-call candidate. A unit test asserts that an `FnEntry` with `has_ref_mut` set is not
fast-call eligible.

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
| closure created on the tree and called from VM code: a closure made inside a handler arm or a continuation replay (`LambdaInfo::of`, `compiled: None`); a closure round-tripped through `host_await_val` under the host-await harness (`SendValue`, `compiled: None`); a resolver lambda passed through a channel (stays compiled); write-back to captures (T40); loop-scoped capture (AX-19) | same output |
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
| a `?` failing in a match guard of a fn whose body shadows its `&mut` param (AX-57's program) | same output: the caller gets its array back (`3`, as native prints today); scopes balanced (S0's tree-walker fix) |
| `f()[i] = v` where `f` and the index expression both print | the value, then the index, print; `f` never runs; panic `invalid assignment target`, exit 101 |
| `a = uncertain_new(10, 0.9)`: `if a > 5`, `while a > 5` (one iteration) and `match 1 { n if a > 5 => .., _ => .. }` | `if`/`while` take the inner `true`; the guard is false and `_` runs; same output |
| `let i: i32 = 1`: `for k in 0..i`, `xs[i]`, `f()[i]`, and `ys[i] = 9` | the first three panic `expected i64, got i32`, exit 101; the write succeeds |

### 5. Type rules

N/A. No type-system change.

### 6. Error codes

One user-reachable diagnostic, no new code: `AXON_ENGINE` set to anything but `vm` or `tree` exits 2 with
`AXON_ENGINE must be "vm" or "tree" (got "<v>")` on stderr before the program runs (§3), the same exit code
as other invalid invocations. On `wasm32-unknown-unknown`, `axon_set_engine` with a value other than 0 or 1
returns non-zero and leaves the engine unchanged. A malformed op stream is a host bug (`unreachable!`),
caught by the parity gates. Every other failure the user can reach is one of the tree-walker's existing
`Flow`s and messages.

### 7. Invariants touched

Preserves:

- **I-1:** runs after the same pipeline, in the `Interp` slot.
- **I-2:** the tree-walker is the reference; every lowered construct has parity, and every unlowered one
  *is* the reference code.
- **I-4:** fn activations keep the same depth guard. Lambda activations have no guard on either engine,
  and their Rust frames sit between guarded fn calls on both. The engine can use more stack per activation
  than the tree, so §4 Execution (Recursion) sets measured bounds instead, checked by
  `scripts/vm_wasm_depth.sh`: on wasm32 the VM completes 1.3 × 450 on four chains in the linear stack at
  every slice, and reports its default-native-stack depth next to the tree's; S6 does not flip the default
  while any VM depth is below the tree's (§4).
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
- [ ] Integration: `scripts/vm_parity.sh` over the example and fixture corpus (below) and
  `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` (§11) exercise both engines through the real binary.
  Journey/red-team: N/A; the only user-facing surface is two environment variables.
- [ ] S0 tree-walker fix: `match_pattern` / guard / `while let` errors pop their scope (eval.rs:306, 308,
  358). Red test first: `vm_engine_scope_leak` runs AX-57's program (a `&mut [i64]` param shadowed by
  `let a = 99`, then a `?` failing in a match guard) and asserts the caller's `len(a)` prints `3`, as the
  native build does. It fails before the fix with `len: expected str/array/dict, got i64`.
- [ ] CLI e2e (`tests/cli_run.rs`, prefix `vm_`): one test per behaviour row. Each runs the program under
  both engines and asserts identical stdout, stderr and exit code. Each also runs once with
  `AXON_VM_TRACE=1` and asserts the fns under test were compiled with the expected tree-node count, so
  parity is never vacuous.
- [ ] Parity corpus: `scripts/vm_parity.sh` builds the §10 binary (`cargo build --release -p axon-core
  --no-default-features --bin axon`; the tree-walker runs qsort in 5.8 s there against 57.7 s in debug)
  and runs, under both engines, every `examples/**/*.ax` that has a `main`, and every
  `crates/axon-core/tests/fixtures/**/*.ax` that `axon check` accepts and that defines `main` (31 of the
  157 fixtures are rejected by check on purpose and 2 have no `main`, at `886aae53`). Isolation: each run
  starts in a fresh temporary working directory with its own `TMPDIR`, `XDG_CACHE_HOME` and
  `AXON_AUDIT_LEDGER`, sets `AXON_LEARNER_STATE=learner.state` and `AXON_BANDIT_STATE=bandit.state`
  (relative, so they resolve inside that directory and every run sees the same strings:
  `examples/asi/persistent_learner.ax:91` prints its state path; without the variables both default to
  fixed `/tmp` paths), reads stdin from `/dev/null`, and sets `AXON_AI_MOCK=1`, `AXON_SEED=42`,
  `AXON_CLOCK=0:1`, `AXON_AUDIT_DETERMINISTIC=1` and no `--verbose`. Each run has a 30-second wall-clock
  limit. Imports resolve only through `AXON_PATH`, `~/.axon/lib` and `<binary dir>/../lib/axon`
  (`axon_search_dirs`, lib.rs:517-544; the last at 536-539), so the script exports an absolute
  `AXON_PATH`: the union `all_examples_parity.sh` uses (`examples/stdlib`, `examples/asi`,
  `examples/modular`, `examples/domain`), rooted at the repository.
  `tests/fixtures/vm_parity_skip.txt` lists files with a reason each; a listed file is not run. An
  unlisted example fails the gate when `axon check` rejects it; any unlisted file fails it when a run hits
  the time limit, or when its two tree runs (made before comparing engines) differ, which means it keeps
  state the isolation does not reach. The list starts with the 10 examples that have a `main` and that
  `axon check` rejects by design under that `AXON_PATH` (the 9 `examples/flagship/**`
  capability-violation programs and `examples/asi/contained_violation.ax`; checked 2026-10-09 at
  `886aae53`), and the three that do not finish without a host or by design
  (`examples/jobs/runaway.ax`, an intentional infinite loop; `examples/r27/killable_agent.ax`, 1e9
  iterations; `examples/mobile/lifecycle.ax`, a `host_await` lifecycle with no host driver). The script
  diffs stdout, stderr, exit code, the audit ledger and `provenance.jsonl`.
  Normalisation is one rule: drop the `ts_ms` and `run_id` keys from every provenance row. Both are
  wall-clock values the virtual clock does not reach (`now_ms` in interp.rs:4267, `generate_run_id` in
  main.rs:4329); every row has `ts_ms`, and the `run_start` row (provenance.rs:606-628) has `run_id`.
  Coverage comes from a third, undiffed run per file under `AXON_ENGINE=vm AXON_VM_TRACE=1`. Besides the
  per-body line (§3), the trace prints one `vm: tree-op <name> <Variant>[(<shape>)]` line per `Tree` op it
  compiles. `<shape>` names the exceptions inside a lowered variant: `Call(struct-lit)`, `Call(P)`,
  `Call(computed)` (a callee that is neither an `Ident` nor a `StructLit`), `Index(E|Var)`, and through S4
  `Call(&mut)`. The script prints `vm_parity: <n> files, <c> bodies, <l> lowered ops, <t> tree ops,
  0 differ`. It fails if any file differs, or if any `tree-op` line names a variant or shape that the
  script's per-slice list marks lowered. That list is the §4 lowered-set table, kept in the script and
  extended by each slice. The check is per op, not a total over the corpus, so adding or removing an
  example cannot fail it unless that example hits a construct the current slice claims to lower.
- [ ] Whole suite: `cargo test -p axon-core` (both feature sets) with `AXON_ENGINE=vm` exported, so the
  CLI children inherit it, and again with `AXON_ENGINE=tree`; both green.
- [ ] Adversarial: recursion bombs (VM-only, tree-only, alternating fn → closure → fn); 1M-element
  arrays; deep block and loop nesting; big-compile's 20k-line source; the re-entrancy row.
- [ ] Record/replay across engines on the existing replay-test programs.
- [ ] Property: the existing proptest generators (`interp/proptest.rs`) run under both engines, with
  outputs compared per case.
- [ ] Parity (interp↔codegen): the existing native-parity harnesses (`scripts/all_examples_parity.sh` and
  siblings) run their interpreter side under both engines: the S5 gate runs
  `AXON_ENGINE=vm AXON_HARNESS_STRICT=1 scripts/parity_all.sh`, and S6 runs it under each engine. After S6
  the default interpreter they compare against native codegen is the VM.
- [ ] wasm: `cargo check -p axon-core --target wasm32-unknown-unknown` and `wasm32-wasip1` with the
  features `CLAUDE.md` names. The wasm parity harnesses run both engines: S0 makes the wasip1 harnesses
  (`wasm_parity.sh`, `wasm_fs_parity.sh`, `wasm_host_await_parity.sh`) pass `--env AXON_ENGINE` to
  wasmtime, and `wasm_browser_interp_parity.sh` call `axon_set_engine` from `AXON_ENGINE`, so the
  `parity_all.sh` runs in the S5 and S6 gates cover wasm32 too.
- [ ] wasm depth: `scripts/vm_wasm_depth.sh` (§4 Execution, Recursion) gates every slice S0–S5 on the
  linear-stack bound and the 450 panic, comparing against the tree from the same commit; default-stack
  depths are reported. S6 runs it with `--require-default-stack`.
- [ ] Red tests first, one per slice (all in `tests/cli_run.rs`, named under the slice's gate prefix so
  the gate runs them). The S2, S3 and S4 programs each use only S1 constructs and those of their own
  slice, so each fails on any build without that slice and passes once it lands, whatever order those
  three independent slices land in (S5's may use all of S1–S4, which it depends on). Each asserts
  `<k> tree nodes` = 0 on the named bodies, never an op count:
  - S0 `vm_engine_scope_leak` (above);
  - S1 `vm_scalar_fib_no_tree_nodes`: trace line `vm: fib <n> ops, 0 tree nodes`; fails on S0, where
    every body is one `Tree` op;
  - S2 `vm_aggregate_part_no_tree_nodes`: `part.ax`'s `main` compiles with 0 tree nodes; fails without
    S2, where `a[j]` is a `Tree` op;
  - S3 `vm_match_option_no_tree_nodes`: a fn matching an `Option<i64>` with a guard, and a `while let`,
    compiles with 0 tree nodes; fails without S3;
  - S4 `vm_closure_fold_no_tree_nodes`: `fold.ax`'s `main` (which uses no S2 construct) and its lambda
    body `main::lambda#0` both compile with 0 tree nodes; fails without S4, where `Lambda` is a `Tree` op
    and lambda bodies are not compiled;
  - S5 `vm_fastcall_mutcall_no_tree_nodes` (`mutcall.ax`'s `main` has 0 tree nodes; fails on S4, where
    the call is a `Call(&mut)` `Tree` op) and `vm_fastcall_slow_<flag>` per flag except `has_ref_mut`
    (§4 S5; the `vm: slow` line; fails on S4, which prints none).

Every `tests/fixtures/` path in this spec is under `crates/axon-core/`.

### 9. Acceptance criteria

- [ ] `scripts/vm_parity.sh` exits 0 with 0 differing files and no `tree-op` line of a variant or shape
  the S5 list marks lowered.
- [ ] `scripts/vm_wasm_depth.sh --require-default-stack` exits 0.
- [ ] `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` exits 0 under `AXON_ENGINE=vm` (the default) and
  under `AXON_ENGINE=tree`.
- [ ] `vm_engine_scope_leak` and every per-slice red test (§8) pass.
- [ ] `cargo test -p axon-core --no-default-features` and `--features codegen` green under both
  `AXON_ENGINE` values.
- [ ] `scripts/vm_perf_gate.sh` exits 0 on the compilebench host (not a skip; see §10).
- [ ] `scripts/reference_gate.sh` in sync (two env vars registered).

### 10. Performance budget

`scripts/vm_perf_gate.sh` builds `cargo build --release -p axon-core --no-default-features --bin axon`
(the workspace release profile: opt 3, thin LTO, 1 codegen unit). It runs each of the five programs in
`tests/fixtures/vm_perf/`, verbatim copies of compilebench's `benchmarks/<b>/axon/main.ax`, with
`AXON_ENGINE=vm` under `perf stat -e instructions:u -r 3 taskset -c ${VM_PERF_CPU:-6}`. It fails on any
median above budget, or on any output differing from the program's golden. If `perf` cannot count, it
exits **2** with "not measured"; that is a failure, not a skip.

| Program | Budget: CPython 3.14.4 median, run `20261008T142344Z` | Tree-walker median, run `20261009T013441Z` (Axon `886aae53`, release) |
|---|---|---|
| fib-recursive | 3,423,642,492 | 12,600,620,670 |
| collatz | 26,058,032,109 | 69,175,361,906 |
| mandelbrot | 15,023,124,683 | 49,352,860,742 |
| arr-sum | 23,624,147,901 | 58,866,778,109 |
| qsort | 18,967,575,334 | 103,109,443,137 |

A median passes when it is at or below the budget; there is no rounding.

`--repros` mode gates per-construct costs on five programs in `tests/fixtures/vm_perf/`. Three are a
1M-iteration `while i < 1000000 { s = s + i; i = i + 1 }` loop. `loop.ax` is the bare loop; `call1.ax`
adds `s = s + id(i)` with `fn id(x: i64) -> i64 { x }`; `mutcall.ax` adds `touch(&mut a, i)` with
`fn touch(a: &mut [i64], i: i64) { a[0] = i }`. A construct's cost is the program's per-iteration count
minus `loop.ax`'s. `part.ax` is qsort's partition loop without the swap:
`for j in 0..1000000 { if a[j] < pivot { i = i + 1 } }` over `a = arr_repeat(0, 1000000)` with `pivot = 1`,
so the branch is taken every time; its cost is (instructions − those of the same program without the
`for` loop) / 1 M. `fold.ax` runs `arr_fold(xs, 0, |acc: i64, x: i64| acc + x)` ten times over
`xs = arr_range(0, 1000000)`, arr-sum's shape with a one-op body; its cost is (instructions − those of the
same program with the `arr_fold` line removed) / 10 M, per element.

| Repro | Slice | Budget | Tree-walker (Axon `886aae53`, release, 2026-10-09) |
|---|---|---|---|
| `loop.ax`, per iteration | S1 | ≤ 300 | 1,264 |
| one-argument call via `dispatch_call` → `call_fn_in` | S1 | ≤ 800 | ≈ 1,125 |
| `part.ax`, per partition iteration | S2 | ≤ 250 | 1,383 |
| `fold.ax`, per element: builtin → closure call, body included | S4 | ≤ 350 | 916 |
| one-argument fast call | S5 | ≤ 300 | n/a |
| one-`&mut`-argument call via `call_mut` | S5 | ≤ 600 | ≈ 2,607 |

The S1 call budget is for the general path, which every call keeps through S4 and flagged calls keep for
good. The program budgets need S4 and S5. fib(32) makes 7,049,155 calls, so 3,423,642,492 leaves about 485
instructions per call, body included: a 300-instruction fast call plus a body of about 10 ops. arr-sum
makes 50 M closure calls (10 folds over 5 M elements), so its budget leaves about 472 per element, body
(`acc + x % m`) and the builtin's loop included; the S4 `fold.ax` budget leaves about 120 of those for the
extra `%` and the variable read.

qsort is the gate's tightest program. Its budget of 18.97 G splits as follows; the terms marked
[INFERENCE] are estimates, not measurements:

| Term | Count | Per unit | Total |
|---|---|---|---|
| `&mut` calls, `mutcall.ax`-shaped (§4 S5) | 15,004,221 | 600 (S5 budget) | 9.00 G |
| `swap` bodies beyond `touch`'s one statement: two more statements, two index reads, one more index write, two more arguments [INFERENCE] | 13,670,662 | ≈ 150 | ≈ 2.05 G |
| `quicksort` prologues: bound check, `let pivot = a[hi]`, `let i = lo` [INFERENCE] | 1,333,559 | ≈ 300 | ≈ 0.40 G |
| `main`: `arr_repeat` and two 1M-iteration loops [INFERENCE] | 2 M | ≈ 400 | ≈ 0.80 G |
| partition iterations (the rest) | 25,092,348 | ≈ 268 | ≈ 6.72 G |

So a partition iteration has about 268 instructions, and `part.ax`'s S2 budget of 250 leaves under 20 for
error in the estimated terms. The slice budgets do not by themselves show that qsort's gate is
reachable. If it is not, Q3 applies.

sieve is reported but not gated: CPython clears multiples with one slice assignment that runs in C.

### 11. Rollout & rollback

`AXON_ENGINE` defaults to `tree` through S0–S5, so engine selection does not change while coverage grows.
Three changes touch the reference tree-walker, each with a CHANGELOG entry: the S0 scope-leak fix
(behaviour), and the S4 pooled closure call and S5 allocation-free `call_mut` (cost only).

- The scope-leak fix is the one intended change to reference behaviour. Today an `Err` out of
  `match_pattern` or a guard (eval.rs:306, 308, 358) skips the arm's `env.pop()`. The catcher's single
  pop (`run_loop_body` or `eval_block`, eval.rs:784-822) then removes the arm's mark instead of its own,
  so one scope mark too many survives and the enclosing scope's bindings stay visible. When the error is
  caught in the same frame (`Break`/`Continue` by `run_loop_body`, `HandlerDone` by `eval_with_handler`),
  later `env.snapshot()`s see those bindings, and a `Flow::Return` out of a guard's `?` makes
  `call_fn_mut`'s reverse scan read a body `let` that shadows a `&mut` param back instead of the param
  (compilebench AX-57: `axon run` panics where the native build prints `3`). After the fix those programs
  see the outer binding. Native codegen keeps no run-time scope stack, so it never had the leak; the fix
  moves the interpreter onto codegen's behaviour. The S0 gate runs `AXON_HARNESS_STRICT=1
  scripts/parity_all.sh` under the tree engine to confirm no other case changes.
- `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` is made real in S0. Today `parity_all.sh` never reads
  the variable (only `harness_skipped` in cli_run.rs:298 does, and gate.sh:731 advertises it), treats every
  skip as success, and enforces only `EXPECT_MIN_PASS=40`. S0 adds: with `AXON_HARNESS_STRICT=1`, a
  skipped harness fails the run unless `scripts/parity_allowed_skips.txt` lists it with a reason. The list
  is seeded with the two harnesses that skip on the compilebench host (measured 2026-10-09 at the
  revision-7 commit: 52 passed, 2 skipped of 54): `android_compute_parity` (no Android NDK) and
  `browser_compute_parity` (opt-in, `BROWSER_PARITY=1`). Without the variable the script behaves as today.
  CHANGELOG entry with S0.
- The pooled closure call (S4) and the allocation-free `call_mut` (S5) change cost only.

Each slice is a revertible commit series with its own gate. S6 flips the default to `vm` in one commit;
reverting it restores the tree default. `AXON_ENGINE=tree` remains the reference and the escape hatch.
After S6 lands, compilebench re-pins Axon, reruns its `axon-interp` row and records the numbers in AX-18;
that is a follow-up in the benchmark repo, not an R50 gate.

Blast radius if wrong: a lowering bug changes a program's output under `axon run`. The net is the parity
corpus, the whole suite under both engines, and the per-test trace assertions. Since an unlowered node is
the reference code, gaps cost speed, never correctness.

### 12. Open questions

- Q1 (non-blocking): unboxed `i64`/`f64` slots. This needs R2a's node→type map and a static proof that a
  slot cannot hold a soft value. Revisit with S6 profiles.
- Q2 (non-blocking): `call_builtin` is a `match name` (builtins.rs:800). If S6 profiles show it hot,
  resolve builtins to an index at compile time. That is a `call_builtin` refactor shared with the
  tree-walker, specified separately.
- Q3 (blocks R50.S6 only if it comes true): qsort, fib or arr-sum still misses its budget after S5 with
  every `--repros` budget met. For qsort that is possible by construction (§10: the estimated terms leave
  under 20 instructions of slack). Then the per-op cost of the S1-S3 ops is the gap. The response is a
  spec revision re-reviewed before S6 (for example an index-compare-branch superop for qsort's loop, or a
  fold-specialised closure call for arr-sum). The gate is never loosened in place.
- Q4 (blocks R50.S6 only if it comes true): after S5, `vm_wasm_depth.sh --require-default-stack` finds a
  chain whose default-native-stack VM depth is below the tree's. `with.ax` is the likely one: `with`
  bodies stay `Tree` ops, so every level keeps the tree's frames plus the VM's. The response is a spec
  revision re-reviewed before S6 (for example running a body on the tree when it is entered from inside a
  `Tree` op). The default is never flipped with the shortfall in place.

### 13. Dependency DAG

| Node | Depends-on / blocked-by | Gate (named test or script) | Status |
|---|---|---|---|
| R50.S0 selector (`AXON_ENGINE`, `axon_set_engine`), trace (body and `tree-op` lines), `run_body`, `FnEntry.compiled`, every body = one `Tree` op; extract `dispatch_call`, `bind_let`, `call_mut`; tree-walker scope-leak fix; `AXON_HARNESS_STRICT` in `parity_all.sh` with `scripts/parity_allowed_skips.txt` (§11); wasm harnesses forward the engine (§8 wasm); `vm_parity.sh`; `vm_perf_gate.sh`; `vm_wasm_depth.sh` and `tests/fixtures/vm_depth/` | — | `cargo test -p axon-core --test cli_run vm_engine_` (incl. `vm_engine_scope_leak`) + `scripts/vm_parity.sh` (empty lowered list) + `scripts/vm_wasm_depth.sh` + `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` (tree engine; §11) | done (`99b8b099`, `e3b2d35b`) |
| R50.S1 scalar core and calls; `AssignInPlace` (AX-31 shapes); extract `cond_bool`, `strict_int`; operand-stack pool | R50.S0 | `cli_run vm_scalar_` (incl. `vm_scalar_fib_no_tree_nodes`) + `vm_parity.sh` + `vm_perf_gate.sh --repros` (S1 rows) + `vm_wasm_depth.sh` | done (`cc23fb17`) |
| R50.S2 aggregates, index/field reads, place writes (`AssignTo`); extract `index_in_place`, `index_value`, `field_in_place`, `finish_record`, `place_index`, `write_place` | R50.S1 | `cli_run vm_aggregate_` (incl. `vm_aggregate_part_no_tree_nodes`) + `vm_parity.sh` + `vm_perf_gate.sh --repros` (S1 and `part.ax` rows) + `vm_wasm_depth.sh` | todo |
| R50.S3 match, patterns, enums, methods; extract `chan_method`, `impl_method` | R50.S1 | `cli_run vm_match_` (incl. `vm_match_option_no_tree_nodes`) + `vm_parity.sh` + `vm_wasm_depth.sh` | todo |
| R50.S4 lambdas; extract `make_closure`; `ClosureCode.compiled`; allocation-free builtin → closure call on the lent path (`Rc::strong_count(cv) == private_refs`, interp.rs:4054), the one `fold.ax` and arr-sum take: `arr_fold`/`arr_map`/... take argument buffers from `arg_bufs`, `call_closure_owned_by` drains its arguments into the params and hands the buffer to `recycle_args` (today `zip(args)` consumes it, interp.rs:4067), and its `Env` gets a pooled `marks` Vec (today `vec![acc, x.clone()]` per element, builtins.rs:1847, and `Env::from_snapshot` starts with an empty `marks`, so `env.push()` allocates, interp.rs:749-754, 4058-4066). The copied path (a closure with other references, e.g. `let f = \|..\| ..; arr_fold(xs, 0, f)`) keeps its per-call `Vec::with_capacity` (interp.rs:4061-4064) | R50.S1 | `cli_run vm_closure_` (incl. `vm_closure_fold_no_tree_nodes`) + `vm_parity.sh` + `vm_perf_gate.sh --repros` (S1 and `fold.ax` rows) + `vm_wasm_depth.sh` | todo |
| R50.S5 fast calls; allocation-free `call_mut` and `CallMut` op | R50.S2, R50.S3, R50.S4 | `cli_run vm_fastcall_` (incl. `vm_fastcall_mutcall_no_tree_nodes`, `vm_fastcall_slow_<flag>`) + `vm_parity.sh` + `vm_perf_gate.sh --repros` (all rows) + `vm_wasm_depth.sh` + `AXON_ENGINE=vm AXON_HARNESS_STRICT=1 scripts/parity_all.sh` | todo |
| R50.S6 default flip, docs | R50.S5; blocked-by Q3 or Q4 only if it comes true | whole suite under both engines + `vm_parity.sh` (S5 list) + `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` under each engine (VM default) + `vm_perf_gate.sh` + `reference_gate.sh` + `vm_wasm_depth.sh --require-default-stack` (default-stack VM depth ≥ tree on all four chains) | todo |

### 14. Evidence ledger

| Claim | Verify command | Expected | Last verified (commit @ date) | Result |
|---|---|---|---|---|
| S0 parity: both engines identical on the corpus | `scripts/vm_parity.sh` | `284 files, 1761 bodies, 0 lowered ops, 1761 tree ops, 0 differ` | `e3b2d35b` @ 2026-10-09 | PASS |
| S0 wasm depth: 585 in the linear stack, 450 panic, both engines and profiles | `scripts/vm_wasm_depth.sh` | exit 0; default stack VM below tree on all chains (debug plain 213 vs 230), reported only | `e3b2d35b` @ 2026-10-09 | PASS |
| S0 suite: both feature sets × both engines; strict parity under tree | `cargo test -p axon-core`; `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` | all green; 53 passed, 2 allowed skips of 55 | `e3b2d35b` @ 2026-10-09 | PASS |
| S1 parity | `VM_PARITY_SLICE=S1 scripts/vm_parity.sh` | `284 files, 1761 bodies, 25090 lowered ops, 1966 tree ops, 0 differ` | `cc23fb17` @ 2026-10-09 | PASS |
| S1 repros: loop ≤ 300, one-argument call ≤ 800 | `scripts/vm_perf_gate.sh --repros S1` | exit 0; loop 228.2, call 790.1 (tree at `cc23fb17`: about 1,149 and 909) | `cc23fb17` @ 2026-10-09 | PASS |
| S1 wasm depth | `scripts/vm_wasm_depth.sh` | exit 0 | `cc23fb17` @ 2026-10-09 | PASS |
| S1 suite: both feature sets × both engines; strict parity under tree | `cargo test -p axon-core`; `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` | all green; 53 passed, 2 allowed skips of 55 | `cc23fb17` @ 2026-10-09 | PASS |

### 15. Review resolution (2026-10-09)

| Finding | Resolution |
|---|---|
| [blocker] `&mut` call protocol undefined; lowering `&mut a` as a unary panics | Calls with `&mut` are `Tree` ops over the whole call through S4, and a `CallMut` op over `call_mut` from S5; the compiled callee writes its params into its own `Env`, so `call_fn_mut`'s read-back is unchanged (§4 Execution) |
| [blocker] Env↔register handoff (params, binding capture, postconditions, `&mut` read-back) | Fork 1(b): compiled bodies run on the same `Env`; `main` under capture runs on the tree |
| [must-fix] per-activation AI budget reset | No bypass of `call_fn_in` through S4; S5 requires `ai_metered` clear |
| [must-fix] borrowed register window across re-entrant builtins | Operand stack and argument buffers are per activation; no borrow across call-outs |
| [must-fix] typed `let` skips refinement | `bind_let` is shared by both engines |
| [must-fix] `match_pattern` needs an `Env`; no method helper | The `Env` is shared, so `match_pattern` is called as is; `chan_method` and `impl_method` extracted in S3 |
| [must-fix] closure representation and builtin-invoked closures | Same `ClosureVal`; `make_closure` shared; `call_closure_owned_by` runs the body in `ClosureCode.compiled` when it is `Some` |
| [must-fix] compiled-code key on `ClosureCode` address | No key: compiled code lives in `ClosureCode.compiled` inside the `Rc`, so it cannot outlive or be confused with another closure's code; `None` for `LambdaInfo::of`, `fn_value` and `SendValue` codes |
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
| parity floor counts bodies | Revision 3: floor on lowered ops plus ceiling on tree ops. Replaced in revision 4 by a per-op check (third review, below) |
| `AXON_ENGINE` unreachable on wasm32-unknown-unknown | `axon_set_engine` export (§4 Activation); wasm harnesses run both engines |
| `run_body` has no fn-table index | `FnEntry.compiled: OnceCell`; owned entries run on the tree (§4 Activation) |
| StructLit-callee calls and `P(x)` missing from the `Tree` list | Named in the permanently-`Tree` row; S1 lowers only `Ident` callees |
| AX-31 in-place append covers `x = x + y` and `arr_concat` | Revision 4: S1's `AssignInPlace` op calls `assign_in_place` first for every AX-31-shaped `Assign`, as the arm does; linear-cost row added |
| recursion claim false for lambdas; S5 omits arity and `recycle_args`; no operand-stack pool | I-4 reworded for lambda activations; S5 steps 4 (arity, after depth as in `call_fn_in`) and 9 added; S1 adds the pool; `dispatch_call` row names `call_fn_entry` and the `callees` memo |

Third review (2026-10-09, `reviewer`, verdict "incorrect": 0 blockers, 6 must-fix, 6 smaller), answered
in revision 4:

| Finding | Resolution |
|---|---|
| [must-fix] S1 repro gates measure the AX-31-shaped statements S1 leaves on the tree | S1 lowers every local `Assign` through an `AssignInPlace` op that calls `assign_in_place` first; it returns `Ok(false)` without evaluating anything for non-`Str`/`Array` locals, so `loop.ax` and `call1.ax` run compiled (§4 lowered set) |
| [must-fix] arr-sum budget has no slice for builtin → closure call overhead | S4 makes the builtin → closure call allocation-free (pooled argument buffer and `marks`), gated by a `fold.ax` `--repros` row (≤ 350 per element); Q3 extended to arr-sum (§10, §12, §13) |
| [must-fix] first-review resolutions describe `vm_id`, `call_method` and index keys | Rows rewritten for `ClosureCode.compiled`, `chan_method`/`impl_method` and the S5 `CallMut` op |
| [must-fix] parity runs share `/tmp` state | Fresh working directory and `TMPDIR` per run; `AXON_LEARNER_STATE`/`AXON_BANDIT_STATE` set per run; a file whose two tree runs differ fails unless listed with a reason (§8) |
| [must-fix] absolute floor/ceiling over a moving corpus | Replaced by a per-op check: no `tree-op` trace line may name a variant or shape the slice lowers (§8, §9) |
| [must-fix] I-4 stack requirement cannot hold | Replaced by a measured bound: stack bytes per activation recorded at S0 for four chains; on wasm32 450 activations of the costliest must fit with a 1.3× margin, or `axon_set_engine(1)` is refused there; wasm chain tests at S0, S4 and S5 (§4, §7, §8). Replaced in revision 5 by `scripts/vm_wasm_depth.sh` and no run-time refusal (fourth review, below) |
| §11 says behaviour unchanged, but S0's leak fix changes reference output | §11 names it as the one intended reference change, with a CHANGELOG entry and the interp↔codegen check |
| S5's "no refinement predicates" is ambiguous | `refine_preds` empty program-wide, the test `call_fn_in` uses (§4 S5) |
| qsort counts | Simulated counts (15,004,221 `&mut` calls, 25,092,348 loop iterations) replace the estimate (§4 S5, §10) |
| "closure made on VM, called by tree" cannot happen | Row rewritten around `LambdaInfo::of` and `host_await_val` closures; `SendValue` copies are made only there |
| owned `FnEntry` has no discriminator; `assign_in_place` signature | `FnEntry.compiled` is an `Option`, `Some` only for `fn_table` entries; helper row uses the real signature |
| template gaps (§6 diagnostic, §8 property and interp↔codegen bullets, trace counts source) | §6 lists the exit-2 message; §8 adds both bullets; counts come from a third, undiffed trace run |

Fourth review (2026-10-09, `reviewer`, verdict "incorrect": 0 blockers, 7 must-fix, 5 smaller), answered
in revision 5:

| Finding | Resolution |
|---|---|
| [must-fix] `send` listed as permanently `Tree` but lowered through `chan_method` | Removed from the permanent row; `chan_method` covers it (§4 lowered set) |
| [must-fix] no trace token for partly-lowered variants | The S1 `Ident` op runs `eval`'s whole chain (local, `globals`, `fn_of_sym`, panic), so module constants run compiled; `Index(E\|Var)` and `Call(computed)` added to the shape list and the permanent row (§4, §8) |
| [must-fix] qsort budget arithmetic spends the same 10 G twice | §10 splits 18.97 G term by term with exact CPython medians, leaving ≈ 268 per partition iteration; new `part.ax` repro (S2, ≤ 250; tree today 1,383); Q3 names qsort's slack as under 20 instructions |
| [must-fix] stack margin measured by a native test | Replaced by `scripts/vm_wasm_depth.sh`, which bisects depth on `wasm32-wasip1` under wasmtime in both profiles; measured tree baselines 889–18,638 in the linear stack (§4 Recursion) |
| [must-fix] refusal only on unknown-unknown, and the refusal state is unreachable | No run-time refusal on any target: a slice that fails `vm_wasm_depth.sh` does not land (§4 Recursion) |
| [must-fix] closure and `with` chains not measured on the tree | Measured: with the guard and host stack lifted, the tree completes ≥ 889 on every chain (1.98× 450 at worst). Under wasmtime's default native stack the tree already traps at 136–318, below the guard; filed as compilebench AX-56, outside this spec, and the VM must not complete less depth there than the tree |
| [must-fix] parity in a fresh cwd loses relative `AXON_PATH` | Absolute `AXON_PATH` exported; an example `axon check` rejects fails unless listed in `vm_parity_skip.txt`, which starts with the by-design rejections (§8) |
| S0 scope-leak red test cannot go red; §11 mechanism wrong | Red test `vm_engine_scope_leak` uses AX-57's program, which panics on `886aae53` and prints `3` natively; §4 and §11 say one extra mark survives and the enclosing scope's bindings stay visible |
| drifted citations | S4 row cites interp.rs:4058-4066; `Interp::build` (interp.rs:2982); `Assign` arm eval.rs:198-209; R46 §2 and its untracked state; 37 `Expr` variants, 38 arms |
| which closure calls S4 makes allocation-free | The lent path (interp.rs:4054), which `fold.ax` and arr-sum take; `call_closure_owned_by` drains and recycles its argument buffer; the copied path keeps its allocation (§13 S4) |
| §10 numbers unsourced and rounded | Both columns name their run and commit; budgets are exact CPython medians; at or below passes |
| S0/S4/S5 gates not named | `vm_engine_scope_leak`, `scripts/vm_wasm_depth.sh` and `tests/fixtures/vm_depth/` named in the S0, S4 and S5 rows (§13) |

Fifth review (2026-10-09, `reviewer`, verdict "incorrect": 0 blockers, 5 must-fix, 7 smaller), answered
in revision 6. It re-measured the revision-5 numbers (AX-57 panic, wasm debug depths, qsort counts, repro
costs) and found them correct.

| Finding | Resolution |
|---|---|
| [must-fix] §7 I-4 makes the default-stack comparison a failure; §4 makes it report-only; S0 cannot meet it | One rule: report-only through S5, enforced only by `--require-default-stack` at S6 (§4 Recursion, §7 I-4) |
| [must-fix] depth script not run at S1–S3, which change per-level stack use | `vm_wasm_depth.sh` in every S0–S5 gate, against the tree from the same commit (§8, §13) |
| [must-fix] S6 depth condition has no gate, criterion or open question | S6 gate runs `vm_wasm_depth.sh --require-default-stack`; §9 criterion; Q4 added and named in the S6 blocked-by cell |
| [must-fix] parity script never terminates on three examples | 30 s per-run limit and stdin from `/dev/null`; a run hitting the limit fails unless listed; `jobs/runaway.ax`, `r27/killable_agent.ax`, `mobile/lifecycle.ax` seeded with reasons (§8) |
| [must-fix] S1 red test not matched by the S1 gate filter; S2–S5 have no red test | Renamed `vm_scalar_fib_no_tree_nodes`; one named red test per slice under its gate prefix (§8, §13, §9) |
| S2 DAG row still owns the AX-31 shapes | S1 node names `AssignInPlace` (AX-31 shapes); S2 is aggregates, index/field reads, `AssignTo` |
| third-review stack row describes the replaced design | Marked replaced in revision 5 |
| import path cited as the binary's directory; skip seed names `bpf/bad_*` | `<binary dir>/../lib/axon` (lib.rs:536-539, inside `axon_search_dirs` at 517-544); seed is the 10 rejected examples with `main`, re-checked at `886aae53` (§8) |
| `host_await_opt` | `host_await_val_opt` (builtins.rs:3129-3131) |
| §11 "Two S0 changes" | Three changes named with their slices |
| §3 trace row lists one line kind | All three kinds and the shape tokens (§3) |
| `for` over arrays | `for` is integer ranges only (ast.rs:400-410) |

Sixth review (2026-10-09, `reviewer`, verdict "incorrect": 0 blockers, 5 must-fix, 6 smaller), answered
in revision 7. It confirmed all twelve fifth-review resolutions against code and runs.

| Finding | Resolution |
|---|---|
| [must-fix] per-run absolute state paths make `persistent_learner.ax` differ between two tree runs (it prints the path, line 91) | `AXON_LEARNER_STATE=learner.state`, `AXON_BANDIT_STATE=bandit.state`, relative to each run's fresh cwd (§8); the sixth review measured its corpus stable that way, and the seventh re-ran revision 7's corpus (144 examples: 157 with `main` minus 13 listed; 124 fixtures; 268 files) twice under the tree with no difference and no timeout |
| [must-fix] `vm_parity.sh` binary unnamed; the `vm_perf` fixtures exceed 30 s in debug | It builds the §10 release binary (§8) |
| [must-fix] no S2 helper for `StructLit`, `AssignTo`, in-place `Index`/`FieldAccess` | Six S2 helpers in the §4 table with their arms' order and texts (`index_in_place`, `index_value`, `field_in_place`, `finish_record`, `place_index`, `write_place`), named in the S2 DAG row; "S0/S3" replaced by the DAG's slices |
| [must-fix] a `has_ref_mut` fast-path test cannot be written (E0605) | Flag tests exclude `has_ref_mut`; a unit test asserts such an entry is not fast-call eligible (§4 S5, §8) |
| [must-fix] §11's interp↔codegen check not in the S0 gate | `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` (tree) in the S0 gate |
| red tests assume a linear S2→S3→S4 order | Each red program uses only S1 constructs and its own slice's; "fails without Sn" (§8) |
| fib op count pinned; lambda body names undefined | Tests assert only `0 tree nodes`; `<fn>::lambda#<i>` defined in §3 |
| stack setting for the 450-panic check unnamed | `-W max-wasm-stack=4294967295`, `AXON_MAX_DEPTH` unset (§4 Recursion); fixtures take their depth from a rewritten `let DEPTH` line |
| fixture corpus filter and root | `crates/axon-core/tests/fixtures`, check-accepted with `main`; the check-rejection failure applies to examples only (§8) |
| citations: `axon_search_dirs` span, `SendValue` closure site, `call_mut` tier | lib.rs:517-544; `SendValue::into_value` (interp.rs:1945) on the reply at builtins.rs:3120, 3136; `call_mut` takes `tier` (eval.rs:1253) |
| S6 gate lacks `vm_parity.sh` and the interp↔codegen harnesses; compilebench rerun is not a named check | Both added to the S6 gate and §9; the compilebench rerun moved to §11 as a follow-up |

Seventh review (2026-10-09, `reviewer`, verdict "incorrect": 0 blockers, 2 must-fix, 3 smaller), answered
in revision 8. It confirmed the other revision-7 resolutions against code and runs.

| Finding | Resolution |
|---|---|
| [must-fix] `parity_all.sh` never reads `AXON_HARNESS_STRICT`; skips pass while 40 harnesses pass | S0 implements strict mode with an allow-list seeded from a measured host run (§11, S0 DAG row) |
| [must-fix] `AssignTo` contract omits places not rooted at a local (`f()[i] = v`) | Value, reachable indexes, then `invalid assignment target`, root never evaluated (§4 helper row, re-run at `886aae53`: prints `v`, `i`, exit 101); behaviour row added |
| lambda trace names for module-scope and uncompiled codes; `<fn>` in §8 | `<module>::lambda#<i>` and `vm: tree <lambda>: unresolved lambda` (§3); §8 uses `<name>` |
| `finish_record` places the `empty` test after field evaluation; in-place index conversion has no helper | `empty` emitted as a constant at compile time; an `eval_int` helper added (revision 9 names it `strict_int` and moves it to S1) |
| §2 quote not verbatim; no Integration bullet; corpus count | REQUIREMENTS acceptance quoted verbatim; Integration bullet (§8); sixth-review row gives the measured 268 |

Eighth review (2026-10-09, `reviewer`, verdict "incorrect": 0 blockers, 2 must-fix, 2 smaller), answered
in revision 9. It confirmed all seven revision-8 resolutions against code and the release binary.

| Finding | Resolution |
|---|---|
| [must-fix] condition rules unstated: guards do not unwrap `Uncertain` or panic (eval.rs:308), `if`/`while` do, `for` bounds are `Int`-only | Stated in §4 Reference semantics; `cond_bool` and `strict_int` (with the `for` protocol) extracted in S1; behaviour rows for an `Uncertain` guard next to `if`/`while` and for `i32` bounds and indexes (re-run at `886aae53`: `if: true`, `guard false`; `xs[i]` panics, `ys[i] = 9` writes) |
| [must-fix] conversion on the computed-receiver `Index` path unnamed | Both `Index` paths use `strict_int` (eval.rs:551, 568); `index_value` takes the `i64` |
| lambda names for `@[verify]` predicates; `<lambda>` placeholder; first-run state of uncompiled codes | `<fn>::verify::lambda#<i>`; literal `<anon>`, printed once per code instance and excluded from the body count (§3) |
| `parity_all.sh` never runs under both engines | S5 gate runs it under `AXON_ENGINE=vm`; S6 and §9 under each engine (§8 Parity) |

Ninth review (2026-10-09, `reviewer`, verdict "correct": 0 blockers, 0 must-fix, 4 nits), answered in
revision 10. It confirmed all four revision-9 resolutions against code and the release binary.

| Finding | Resolution |
|---|---|
| wasm harnesses never forward `AXON_ENGINE` to the guest (wasm_parity.sh:117, wasm_fs_parity.sh:71, wasm_host_await_parity.sh:66) | S0 adds `--env AXON_ENGINE` and the browser harness's `axon_set_engine` call (§8 wasm, S0 DAG row) |
| `While`/`For` arm spans | eval.rs:323-348 and 373-400 |
| `for` bound order ambiguous | `start` evaluated and converted before `end` is evaluated (§4 `strict_int` row) |
| `soft_inner` also unwraps `Temporal` | Stated (§4, value.rs:41-44) |
