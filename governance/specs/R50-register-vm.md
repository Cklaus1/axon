# R50 — Bytecode engine for `axon run`

**Spec ID:** `R50-register-vm`
**Status:** Landed (S0-S9; S6 made `vm` the default at `a9176ce5`); revision 27 adds S10-S12, Draft pending review. Reviewed at revision 10 after nine adversarial reviews (2026-10-09, `reviewer`). The first eight,
verdict "incorrect" (first 2 blockers and 11 must-fix, second 4 must-fix and 8 smaller, third 6 must-fix
and 6 smaller, fourth 7 must-fix and 5 smaller, fifth 5 must-fix and 7 smaller, sixth 5 must-fix and 6
smaller, seventh 2 must-fix and 3 smaller, eighth 2 must-fix and 2 smaller), and the ninth, verdict
"correct" (0 blockers, 0 must-fix, 4 nits), are answered in §15. Revision 11 adds slices S7 and S8
because §12 Q3 came true at S4 (measured, §15 "Revision 11"); revisions 12-14 answer the tenth to twelfth
reviews; the twelfth judged it ready once its four text fixes landed (revision 14), so the S7/S8
additions are Reviewed at revision 14 and S7 and S8 may start. Revision 15 adds slice S9 (deferred
compile, compilebench AX-58; §4 S9, §15 "Revision 15"); revision 16 answers its review (2 must-fix,
4 smaller; §15 "Revision 16") and records AX-56's wasm32 guard change (§4 Recursion); revision 17
answers the fourteenth review (2 must-fix, 2 smaller; §15 "Revision 17") with AX-56's wasm32
stack budget (§4 Recursion); revision 18 answers the fifteenth (2 must-fix, 2 smaller; §15
"Revision 18") by charging measured frame bytes; revision 19 answers the sixteenth (2 blockers,
1 must-fix; §15 "Revision 19") by guarding the whole recursive call-graph component and checking it
mechanically; revision 20 answers the seventeenth (2 must-fix, 1 smaller; §15 "Revision 20") by
narrowing the stack guarantee to interpreter recursion (data-depth recursion is compilebench AX-59)
and re-deriving the code citations; revision 21 answers the eighteenth (1 blocker, 3 must-fix,
1 smaller; §15 "Revision 21") by charging the bytecode compiler's recursion, classifying every
other recursion reachable from `eval`, and giving each engine its own fn-body frame; revision 22
answers the nineteenth (1 blocker, 1 must-fix, 1 smaller; §15 "Revision 22") by making run-time
expression walks iterative, pointing handler frames into the source instead of cloning it,
compiling a fn body in a frame that returns before the body runs, and checking each budget
constant's guard site; revision 23 answers the twentieth (1 blocker, 1 must-fix; §15 "Revision
23") by stating that source-depth recursion is outside the budget (compilebench AX-61) and charging
the VM's op loop to the tree's fn level; revision 24 answers the twenty-first (1 must-fix, 2
smaller; §15 "Revision 24") by catching a `break` without a second op-loop frame; revision 25
answers the twenty-second (1 must-fix, 4 smaller; §15 "Revision 25") by compiling a lambda body in
a frame that returns before it runs and charging the tree's lambda level the VM's op loop;
revision 26 answers the twenty-third (verdict "correct": 0 blockers, 0 must-fix, 2 nits; §15
"Revision 26") by checking the compile frames' constants. The twenty-third judged revision 25
ready once its nits were fixed, so revision 26 is Reviewed and the AX-56 change it records lands.
Revision 27 (2026-10-10; §15 "Revision 27") adds S10 (array elements in pure loops), S11 (a
pure-`i64` function tier) and S12 (compilebench AX-59/AX-60/AX-61 wasm32 traps), each prototyped
and measured first; they are Draft until an adversarial review passes them. Revision 28 answers the
twenty-fourth review of them (2 blockers, 6 must-fix, nits; §15 "Revision 28"); revision 29 answers
the twenty-fifth (3 blockers, 2 must-fix, 4 nits; §15 "Revision 29"); revision 30 answers the
twenty-sixth (3 must-fix, 1 nit; §15 "Revision 30"); revision 31 answers the twenty-seventh (2
must-fix, 2 nits; §15 "Revision 31"); revision 32 answers the twenty-eighth (2 must-fix, 1 nit;
§15 "Revision 32"); revision 33 answers the twenty-ninth (1 blocker, 1 must-fix, 1 nit; §15
"Revision 33"); revision 34 answers the thirtieth (3 must-fix, 1 nit; §15 "Revision 34"); a
thirty-first review is pending.
**Risk class:** Structural (a second execution path for the reference engine)
**Author / date:** 2026-10-09, from compilebench AX-18 (interpreter cost) after AX-53..AX-55.

```spec-meta
id: R50-register-vm
status-claim: Landed
depends-on: none
blocks: none
blocked-by: none
supersedes: none
related: R2a-type-map-threading, R0-interp-module-split, R44-accumulating-session, R7-targets
conflicts-with: none
reserves: none (no new diagnostic or exit codes; env vars AXON_ENGINE and AXON_VM_TRACE are registered in env_registry.rs at S0, AXON_VM_EAGER at S9)
evidence: scripts/vm_parity.sh; scripts/vm_wasm_depth.sh; scripts/vm_perf_gate.sh; spec review evidence is §15
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

No language change. Three environment variables:

| Var | Values | Effect |
|---|---|---|
| `AXON_ENGINE` | `tree` (default until S6), `vm` (default from S6) | Which engine runs fn and lambda bodies under every interpreter entry (`axon run`, `axon-run`, `axon test`, `axon goal`). On `wasm32-unknown-unknown`, which has no environment, the `axon_set_engine` export selects it instead (§4 Activation). Any other value: exit 2 with `AXON_ENGINE must be "vm" or "tree" (got "<v>")`. |
| `AXON_VM_TRACE` | `1` | Seven kinds of stderr line. Per fn or lambda body, the first time it is compiled (its first run, or from S9 its second; §4 S9): `vm: <name> <n> ops, <k> tree nodes`, or `vm: tree <name>: <reason>` when the whole body stays on the tree-walker. Per `Tree` op compiled into a body: `vm: tree-op <name> <Variant>[(<shape>)]`, where `<shape>` is one of `Call(struct-lit)`, `Call(P)`, `Call(computed)`, `Index(E\|Var)` and, through S4, `Call(&mut)` (§8). From S5, per call that a flag keeps off the fast path: `vm: slow <fn>: <flag>` (§4 S5). From S7, per body that holds S7 ops, right after its body line: `vm: pure <name> <p> exprs, <q> loops` (`<p>` `Pure` ops, `<q>` `PureLoop` ops); and per lambda code the first time `arr_fold` runs it in registers: `vm: fold-leaf <name>` (§4 S7). From S9, per body whose first entry runs on the tree-walker, at that entry: `vm: defer <name>` (§4 S9). From S11, per fn built on the pure-`i64` tier, once: `vm: purefn <name> <n> ins` (§4 S11). `<name>` is the fn's name (`Type::method` for an impl method) or, for a lambda body, `<owner>::lambda#<i>`. `<owner>` is the enclosing fn; `<fn>::verify` for a lambda inside fn `<fn>`'s `@[verify]` predicate (resolved after the body, sym.rs:871-875); or `<module>` for a lambda the resolver reaches through a module item (a module `let`, a `refine` predicate or a type's refinement; sym.rs:691-697). `<i>` is the lambda's 0-based position among its owner's lambdas in source (pre-)order, nested ones included; for `<module>` the count runs over those items in source order. A code with `compiled: None` (`LambdaInfo::of`, `fn_value`, `SendValue`) has no name and no first-run state (`LambdaInfo::of` builds a fresh code per evaluation, eval.rs:644-650): it prints the literal line `vm: tree <anon>: unresolved lambda` once per code instance, and `vm_parity.sh` excludes those lines from its body count. Off by default. It never changes stdout or the exit code. |
| `AXON_VM_EAGER` | `1` | From S9: under `AXON_ENGINE=vm`, compile every body on its first entry instead of deferring a body that is not hot on entry to its second (§4 S9). Read once, when the `Interp` is built; `wasm32-unknown-unknown` has no environment and never sets it. Same stdout, stderr and exit code either way, except on wasm32 near the stack budget (§4 Recursion): compiled and tree frames differ in size, so the depth at which the budget panic comes can move by many levels (a 130-fn chain with 40 parentheses per call: deferred 28, eager 126, debug). The `vm_` cli tests and `vm_parity.sh` use it so a body run once is still compiled code under test. |

```text
$ AXON_ENGINE=vm AXON_VM_TRACE=1 axon run fib.ax
vm: defer main
vm: defer fib
vm: fib 6 ops, 0 tree nodes
vm: purefn fib 8 ins
2178309
$ AXON_ENGINE=vm AXON_VM_EAGER=1 AXON_VM_TRACE=1 axon run fib.ax
vm: main 5 ops, 0 tree nodes
vm: fib 6 ops, 0 tree nodes
vm: purefn fib 8 ins
2178309
$ AXON_ENGINE=vm AXON_VM_TRACE=1 AXON_DUMP_BINDINGS=/tmp/b.json axon run prog.ax
vm: tree main: binding capture
...
```

### 4. Semantics (what it does)

#### Fork 1 — where the engine keeps locals

- (a) A separate register file per activation (first draft). **Rejected by review.** `call_fn_in`
  binds params and `goal_met` into its `Env`. `call_fn_mut` reads `&mut` params back out of that `Env`
  (interp.rs:3375-3401 `call_fn_mut` at `886aae53`). Refinement postconditions evaluate against it. `main`'s binding capture
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

S7's registers (§4 S7) are not typed registers in that sense. They live only inside one op (a `Pure`
expression, one `PureLoop` run, one `fold_leaf` run), are filled from the `Env` with a kind check on
every entry, and any value other than a plain `Int`, `Float` or `Bool` makes the op decline to the
generic ops. No static type is assumed.

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
unwrap and no panic, so an `Uncertain<bool>` or any other value is false (eval.rs:341); `for` bounds and
index reads take `Int` only (`strict_int`, below), while place writes also accept a sized int
(`place_index`):

| Extracted helper | From | Contents (in eval's order) |
|---|---|---|
| `dispatch_call(callee, argv, tier, env)` | `eval_call` after argument evaluation | `resume` handling (multi-shot replay or `Flow::Resume`); set or clear `current_call_tier` exactly as now; then 1. a local holding a `Value::Closure` *by runtime value* (`let max = 3; max(a, b)` still calls the builtin); 2. builtin (precedence over a same-named user fn), skipped through the `callees` memo for names already proven not to be builtins (eval.rs:1219-1242); 3. `call_fn_entry` (pooled frame, `has_ref_mut` refusal; interp.rs:3811-3826); 4. global closure; 5. the unknown-function panic |
| `bind_let(kind, name, slot, ann, value, env)` | `Let`/`Own`/`RefBind` arms (eval.rs:268-275) | refinement predicate check in a fresh `Env` binding `_` and the name, then `Flow::RefineViolation` on false; then sized-int coercion; then `define_var` |
| `chan_method(recv, method, args: &[Expr], env)` and `impl_method(recv, method, argv, env)` | `Expr::MethodCall` arm (eval.rs:474-489) | The arm evaluates the receiver first. For a `Value::Chan` receiver, `recv`/`try_recv`/`len`/`clone` and the unknown-method panic never evaluate the arguments, and `send` evaluates only `args[0]`; that path is `chan_method`, which takes the unevaluated argument nodes and evaluates them itself exactly as the arm does. Every other receiver evaluates all arguments left to right and then calls `impl_method`. The engine lowers a method call as: evaluate the receiver; op `MethodRecv` calls `chan_method` with the argument nodes when the receiver is a channel, and otherwise falls through to the compiled argument evaluation and `impl_method`. The receiver's type is known only at run time, so the branch is a run-time check. |
| `make_closure(lambda_expr, env)` | `Expr::Lambda` arm | `res.lambda(expr)` (else `LambdaInfo::of`); captures built from `env` in `captures` order |
| `assign_in_place(s, slot, name, value, env)` | `Assign` arm (eval.rs:277-288, 1619-1702) | already a function: it matches the AX-31 shapes by syntax, then returns `Ok(false)` without evaluating anything unless the local currently holds a `Str` or `Array`; the engine calls it first for every local `Assign`, exactly as the arm does (§4 lowered set) |
| `call_mut(callee, args: &[Expr], tier, env)` | `eval_call_mut` + `call_fn_mut` (eval.rs:1214-1261, interp.rs:3375-3401 at `886aae53`) | the `&mut` move-out / call / move-back protocol, unchanged in behaviour, including the unconditional write of `tier` to `current_call_tier` (eval.rs:1253 at `886aae53`); S5 makes it allocation-free (pooled frame and buffers), which the tree-walker gets too |
| `cond_bool(v, what)` (S1) | `If` and `While` arms (eval.rs:310-322, 361-371) | `soft_inner` unwrap of an `Uncertain` or `Temporal` (value.rs:45-60), then `Bool(b)` gives `b`; any other value panics `if condition must be bool, got <t>` or `while condition must be bool, got <t>` (`what` picks the word). The S1 fused compare-and-branch takes this path whenever the comparison does not yield a plain `Bool`. Not used for match guards |
| `strict_int(v)` (S1) | `eval_int` (eval.rs:1634-1638 at `886aae53`; now `strict_int`, eval.rs:178-189) | `Int(n)` gives `n`; anything else, a `SizedInt` included, panics `expected i64, got <t>`. `for` (S1, eval.rs:403-430): evaluate `start` and pass it through `strict_int`, then evaluate `end` and pass it through `strict_int` (eval.rs:410-411; a bad `start` panics before `end` runs), before the variable is bound; then per iteration the `i <= e` (inclusive) or `i < e` test, `env.push()`, define the variable, the body as `run_loop_body` runs it, `env.pop()`, `i += 1` |
| `index_in_place(s, slot, name, idx, env)` and `index_value(arr, idx)` (S2) | `Index` arm (eval.rs:523-536) | The arm's two paths stay two ops; both convert the index with `strict_int` (eval.rs:529, 534), so a `SizedInt` index panics on a read although `place_index` accepts it on a write. An `Ident` receiver that is bound locally or in `globals` is read in place: the engine evaluates the index only after that existence test, converts it, then calls `index_in_place`. Any other receiver, an unbound `Ident` included (which then runs the `Ident` op's chain: `fn_of_sym`, then the undefined-identifier panic), is evaluated first, then the index, converted, then `index_value` with the `i64`. Same bounds and non-array panic texts |
| `field_in_place(s, slot, name, f, field, env)` (S2) | `FieldAccess` arm (eval.rs:491-502) | An `Ident` receiver: `get_var`, then `globals`, then the undefined-identifier panic (no `fn_of_sym` step, unlike the `Ident` arm), then `field_of`; any other receiver is evaluated, then `field_of` |
| `finish_record(expr, name, vals, env)` (S2) | `StructLit` arm (eval.rs:546-571) | `res.record_lit(expr)` (else `RecordLit::of`) is known at compile time. When its `empty` is set (a literal with no fields and no whole-struct refinement, sym.rs:590-598) the engine emits that value as a constant and calls nothing. Otherwise the caller evaluates the field expressions in source order (the arm's order) into `vals`, and the helper does the rest exactly as the arm: slot placement, per-field sized-int coercion, enum versus struct, then the per-field and whole-struct refinement checks (`Flow::RefineViolation`, exit 6) |
| `place_index(v)` and `write_place(base, slot, steps, value, env)` (S2) | `AssignTo` arm (eval.rs:296-301) and `flatten_place` (interp.rs:4727-4752) | The value is evaluated first. Then the index expressions of the place, in `flatten_place`'s order (outermost `Index` node first, walking toward the base: `g[i][j] = v` evaluates `j` before `i`), each converted by `place_index` (`as_int`, then the `negative index` panic). `write_place` is phase 2: `get_var_mut` with the undefined-variable panic, the copy-on-write walk and its panic texts. A place whose root is not an `Ident` (`f()[i] = v`, which `axon check` accepts; the parser takes any `Index`/`FieldAccess` as a place, parser.rs:1807-1817) compiles to the value, then the index expressions down to that root through `place_index`, then `Panic("invalid assignment target")` (`flatten_place`'s `_` arm, interp.rs:4747); the root is never evaluated. So every `AssignTo` lowers in S2 and none is a `Tree` op |

#### Activation

- `call_fn_in` is the only way into a compiled fn body. At the point where it evaluates `f.body`, it calls
  `run_body(entry, &f.body, env)`. With `AXON_ENGINE=vm`, no whole-body exclusion and a table entry, that
  runs the compiled code against `env`; otherwise it calls `eval`. Everything before and after the body is
  unchanged: depth guard, arity check, `current_fn`, agent and AI-budget guards (AX-54's `FnEntry` flags),
  param coercion, refinement pre- and postconditions, goal training, provenance, and the soft unwrap of the
  result. Through S4 **every** call, VM→VM included, goes through `dispatch_call` → `call_fn_in`. S5's fast
  call is the only bypass, under the S5 conditions.
- Compiled fn bodies live in `FnEntry`: it gains `compiled: Option<OnceCell<Body<'p>>>`, `Some` only for
  the entries `Interp::build` (interp.rs:3458) pushes into `fn_table` (interp.rs:3523-3527) and filled on first
  compile (the body's first entry, or from S9 its second; §4 S9). That costs no
  lookup per call, so AX-54's removal of the `fn_of_def` probe stays intact. `FnEntry::new` builds table
  and owned entries the same way (sym.rs:419-471), so the `Option` is the discriminator: the owned
  `FnEntry::new(f, ..)` that `call_fn`/`call_fn_mut` build for a def missing from `fn_of_def`
  (interp.rs:3315, 3380 at `886aae53`) gets `compiled: None` and its body runs on the tree. Fn bodies are `&'p Expr`, so
  their `Tree` ops borrow the program.
- Compiled lambda bodies live in their `ClosureCode`. The resolver owns the resolved copy of every lambda
  body (sym.rs:1079-1119: `body: body.clone()`, resolved in the clone); the AST original is never resolved,
  so only the clone has slots. `ClosureCode` gains `compiled: Option<OnceCell<Body>>`. It is `Some` only for
  codes built by `Resolution::lambda`, and `None` for `LambdaInfo::of` codes (run-time AST clones),
  `fn_value` forwarders and `SendValue` deep copies (built by `SendValue::into_value`, interp.rs:2377, on
  the reply of `host_await_val`/`host_await_val_opt`, builtins.rs:3140, 3156), whose bodies run on the tree. A compiled lambda body
  refers to nodes of that same `ClosureCode.body` by raw `*const Expr` (for `Tree` ops and literal
  operands), not by `&'p Expr`. This is sound because `ClosureCode` is only ever built inside an `Rc`
  (interp.rs:2422, eval.rs:215, sym.rs:342 and 1102), its `body` is never mutated or moved afterwards, and
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
  crates/axon-wasm/src/lib.rs:57-172), so S0 adds an exported `axon_set_engine(engine: u32)` (0 = tree,
  1 = vm). It is set before `axon_eval` and defaults to the build default. The wasm parity harness calls it
  for both engines. `wasm32-wasip1` reads `AXON_ENGINE` like native.

#### Execution

- Scopes: the engine emits `ScopePush`/`ScopePop` at exactly the points where `eval` calls `env.push()` /
  `env.pop()`: block, loop iteration, match arm, while-let, `for`. The resolver's slot numbering assumes
  those points. On **every** exit from a scope, whether normal, a caught `Break`/`Continue`, or an error
  propagating out of the body, the engine pops the scopes it pushed one at a time, innermost first, as
  `eval_block` and `run_loop_body` do on every `Err` (eval.rs:723-764). It never truncates to a recorded
  `marks.len()`. `call_fn_mut` depends on this: it reads `&mut` params back by reverse name scan after the
  body returns on any outcome (interp.rs:3387-3399 at `886aae53`), so a callee that left a shadowing `let a` scope in
  place would hand the caller the shadow.
  Two later exceptions leave the `Env` in the state the pushes and pops would have: S7's `PureLoop` does
  not execute the iteration scope of a loop nothing in which can observe the `Env`, and S8's `ForNext`
  overwrites the loop variable instead of popping and re-pushing a scope that holds only it (§4 S7, S8).
- The tree-walker today leaks one mark when `match_pattern` or a guard returns `Err` inside a match arm,
  and when `match_pattern` errs in `while let` (eval.rs:306, 308, 358 at `886aae53`: `?` before `env.pop()`). The
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
  depth guard (`AXON_MAX_DEPTH`, default 6,000; 128 on wasm32 since AX-56, 450 before). Lambda activations have no depth guard on
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
    `with` body 889 / 9,018. The requirement: under both engines, in both profiles, every chain
    completes 1.3 × the wasm32 guard under that configuration (585 while the guard was 450, 167 since
    AX-56).
  - The wasm engine's own native stack, reached first. Under wasmtime's default `max-wasm-stack`
    (512 KiB) the tree-walker trapped before 450 on all four chains (deepest completed 136–318 at
    `886aae53`; compilebench AX-56). One level is several interpreter frames of up to 704 bytes each (`eval`
    272 + 16 per frame and several per level on the tree; `run` 512 + 16 and `call_fast_arg` 256 + 16 per
    plain VM level, release, from `wasmtime compile` prologues; `call_builtin` 688 + 16). Through S0–S8 the script only
    *reported* each chain's default-stack VM depth next to the tree's, and S6 required VM ≥ tree
    (`--require-default-stack`; §12 Q4). After S9, AX-56 lowered the wasm32 guard to 128
    (interp.rs `RECURSION_LIMIT`). Measured 2026-10-09 with the stack budget below in place and
    `AXON_MAX_DEPTH` raised, the shallowest depth a chain completes before a trap is 157 (tree,
    fn → closure → fn, debug), so the recursion-limit panic comes before a trap on every chain, engine
    and profile, and the script requires every default-stack depth to reach 1.2 × the guard (154).
    Under the default depth limit the budget, which charges the tree's fn and lambda levels what a VM
    level costs, panics before the guard on some chains: the tree completes the closure chain to 104
    (debug) / 93 (release) and the release `with` chain to 124, the VM 126 on all four
    (`scripts/vm_wasm_depth.sh`'s `reach` configuration gates VM ≥ tree there too).

  `scripts/vm_wasm_depth.sh` checks both. It builds `axon-run` for `wasm32-wasip1` in both profiles from
  the commit under test, runs the five chain programs in `tests/fixtures/vm_depth/` (`plain.ax`,
  `plain_generic.ax` (S11: kept off the pure-`i64` tier), `closure.ax`, `mut.ax`, `with.ax`; each
  recurses to the depth in its first line, `let DEPTH = 100`, which
  the script rewrites in a temporary copy per probe) under both engines of that same build, bisects the
  deepest completed depth in each configuration, checks the depth panic at the guard under the default
  stack (`AXON_MAX_DEPTH` unset), and prints one line per (profile, engine, chain, configuration). It
  fails when a chain misses 1.3 × the guard in the linear stack or 1.2 × the guard under the default
  stack, or the guard panic is wrong (a trap included), and with `--require-default-stack` also when a
  default-stack VM depth is below the tree's. There is no run-time refusal: a change that fails the
  script does not land. `wasm32-unknown-unknown` runs the same code with the same guard; its native
  stack belongs to the browser and is not measured here.

  The call-depth guard counts fn calls, but stack per level grows with how deeply the recursive call
  sits inside expressions, `with` bodies, handler arms, builtin callbacks and lambdas. The fourteenth
  review trapped `w(127)` nested in nine parentheses inside a `with` body; the fifteenth trapped the
  tree-walker at depth 57 through five nested lambdas per step and at 39-42 through six nested
  `arr_fold` callbacks; the sixteenth trapped it through ten nested `for` loops (uncosted
  `run_loop_body`) and trapped the default VM through `call_fast` and `exec_catching`. So on wasm32
  the budget is `max_depth × NEST_PER_DEPTH` bytes (3,584; 448 KiB at the default guard, leaving
  64 KiB of wasmtime's 512 KiB for the run's base frames and the bounded leaf work above the deepest
  guarded frame; interp.rs `NEST_PER_DEPTH`, `nest_cost`, `nest_guard!`, `Interp::enter_nest`), and every
  recursion is charged against it by construction, not by a list of observed shapes. The recursive
  part of the interpreter is the strongly connected component holding `Interp::eval` in the module's
  call graph (79 functions debug, 58 release, measured 2026-10-09); the bytecode compiler's
  recursion (`Compiler::expr`/`stmt` and their helpers, 34 functions debug, 9 release) is a second
  component. 54 functions carry `nest_guard!` or `Compiler::nest` (52 in the interpreter, the two
  compiler entries), chosen so the unguarded part of each component has no cycle; each guarded
  function's `nest_cost` constant, per profile, covers its own Cranelift x86-64 frame (`sub rsp`
  plus return address and frame pointer, the largest over its instances, `0` where the profile
  inlines it) plus the deepest chain of unguarded functions it can call before the next guarded one.
  A constant defined as a sum of others (`RUN_BODY_TREE = RUN_BODY_VM + RUN`) charges their bytes,
  which must cover its own function's frame and tail: the tree's fn level charges what a VM level
  does above its own frame (the VM's fn body `run_body_vm`, which inlines `exec`, plus the op loop
  `run`, which a level whose call sits in a `Tree` op of a compiled body passes through), although
  the tree's `run_body_tree` frame is smaller than either. Likewise a lambda level: a compiled
  lambda body runs from `run_lambda`, inlined into `call_closure_owned_by`, through `exec` into
  `run`, and the tree's `run_lambda_tree` charges `RUN_LAMBDA_TREE = RUN`. The compiler
  gives up when the budget is spent and the body runs on the tree that time (`compile` returns
  `None`). A fn body is compiled in a frame of its own (`Interp::compile_body`, guarded by
  `COMPILE_BODY`), and so is a lambda body (`Interp::compile_lambda`, `COMPILE_LAMBDA`, which also
  applies S9's first-entry rule); each returns before the body runs, so the entry that compiles a
  body, and a body that never compiles (one too large for the budget left at its depth, retried on
  every entry), leave no compile frame live under the recursion. Their constants cover the compile
  frame and its unguarded tail (`compile` and the `Vec` growth under it: 1,504 / 1,024 bytes for a fn
  body, 1,616 / 976 for a lambda, debug / release), live only while that one compile runs. So every
  recursion step through
  the interpreter or the compiler charges at least the stack
  it uses, whatever the shape. Exceeding the budget gives the same `recursion limit exceeded
  (<max_depth>)` panic, exit 101. Any other recursion reachable from `eval` is outside the budget and
  must be classified in `wasm_stack_budget.py`'s `ALLOW` list: a fixed depth (`PURE_DEPTH` for pure
  trees, numeric helpers that recurse once), std's log-n algorithms, the panic path, or the depth of
  the source or of a value. Before S12, recursion over the depth of a value ran in the 64 KiB
  margin and a deep enough value trapped: dropping a 3,000-node enum list trapped the debug build
  with no call recursion, and a 600-node list dropped at depth 120 trapped it too (compilebench
  AX-59). From S12 a drop runs at most `DROP_INLINE_DEPTH` nested container drops on wasm32 and
  defers the rest (§4 S12); comparing and formatting a deep value still recurse per level and stay
  in `ALLOW` (compilebench AX-62). Recursion over source depth (parsing, checking, cloning, dropping
  or matching one construct) is outside the budget as well. Before S12 the front end bounded it only
  loosely: it trapped on nesting the parser recurses on at 273 levels in release, and the checker on
  a left-associative operator chain (which the parser builds in a loop) at 1,301 terms in debug and
  832 in release (compilebench AX-60). From S12 wasm32 refuses any expression higher than 224 (§4
  S12), which bounds every later walk over one expression. A walk over a chain of declarations
  (a call chain behind `@[contained]` or `@[total]`, a chain of refinement-type names) is not
  bounded by it: flat source traps it on wasm32 (compilebench AX-66, out of scope, §4 S12). Copying a 700-term lambda for `host_await_val`
  (`SendValue::from_value_at`) at the bottom of a recursion with a `with` and nine parentheses per
  level trapped at depth 67-77 under both engines and profiles (compilebench AX-61); from S12 wasm32
  copies no payload but a `Str`. The interpreter walks expressions without recursion
  (`ast::walk_expr` keeps its own stack, so `mentions_var` and the compiler's `binds` are flat), and
  a handler frame points into the source instead of cloning the arms and the `with` body on every
  entry. Four nesting probes hold a chain of the deepest height the front end accepts (700 terms
  before S12, 224 from it) at the bottom of the recursion: an `Assign` operand, a body compiled
  there, a `with` body, and a body too large to compile within the budget (`assign_chain`,
  `compile_chain`, `with_chain`, `fail_compile`).
  `scripts/wasm_stack_budget.py`, run by `vm_wasm_depth.sh` on both builds under test (`wasmtime
  compile`, `objdump -d`, `c++filt`, `wasm-objdump`), recomputes the components, frames and tails and
  fails when a constant is too small (for the guarded functions in a checked component, and for
  those reachable from `eval` outside one, `compile_body` and `compile_lambda`), a constant is not
  charged by exactly one guard site in the function it is named after, an unguarded cycle exists,
  adding every `call_indirect` edge
  (to each table function of the call's type; the entry `main` excepted) widens a component, or a
  recursion reachable from `eval` is neither checked nor in `ALLOW`
  [INFERENCE: the check is exact for the x86-64 code wasmtime emits on the gate host; other hosts'
  frames are not measured]. The script runs the 31 committed probes in
  `tests/fixtures/vm_depth/nest/` (`assign_chain`, `break_fast` (a two-argument call through three
  lambdas after a `break` out of a `with` body), `builtin`, `compile_chain`, `compile_deep` (a
  240-deep expression compiled on a deferred second entry at full depth), `distinct` (160 distinct
  fns, each entered once per level: the VM's first entry, §4 S9), `expr` (40 levels),
  `fail_compile`, `fold`, `fold10` (ten nested `arr_fold` callbacks), `handler_arm`, `interp`,
  `lambda`, `lambda10` (ten nested lambdas), `lambda_break` (six chained lambdas, each catching a
  `break` out of a `with` before the next call), `lambda_chain` (130 lambdas in a dict cycle, each
  compiled at depth), `lambda_self` (one lambda recursing through a dict, no fn level), `let_with`
  (a `let` and one `with`: the recursion
  runs in a `Tree` op through the op loop), `loop_for` (ten nested `for` loops), `loop_while_let`
  (ten nested `while let` loops), `loop_with` (six nested `for` loops inside a `with` body), `map`,
  `match`, `method`, `mixed6`, `mut_with3` (`&mut` recursion inside three `with` bodies),
  `question`, `refined`, `with_body` (a fn whose whole body is one `with`, so one `Tree` op),
  `with_chain`, `with_expr`) deep under the default stack; each must give the
  recursion-limit panic, not a trap, under both engines and in both profiles. It also bisects each
  probe's deepest exit-0 depth under the guard, and with `--require-default-stack` the VM's must be
  ≥ the tree's on every probe, as on every chain. A body the VM runs on the tree (an S9 first entry,
  or a body compiled to one `Tree` op, `Body::lone_tree`) runs as `run_body_vm` → `eval`, the
  tree-walker's own frames plus one, and a body whose recursion runs in a `Tree` op of a compiled
  body adds the op loop `run`; `run_body_tree` charges both (`RUN_BODY_VM + RUN`), so such a level
  costs the VM no more budget than the tree. The depth at which the budget panics depends on
  which frames each engine and compile mode keeps live, and can differ by many levels. Measured
  2026-10-09 on the revision-24 builds, deepest exit-0 depth under the default stack: `expr` tree 27,
  VM 126 (debug; 31 / 126 release); `lambda10` 23 / 49 (debug; 22 / 35 release); a 130-fn `distinct`
  chain with 40 parentheses per call, tree 27, deferred VM 28, `AXON_VM_EAGER=1` 126 (debug; 31 / 32
  / 126 release). The only
  gated relation is deferred VM ≥ tree, on every chain and probe.
  Native has no such budget: its interpreter thread stack is sized from `max_depth`.

#### Lowered set per slice (anything else is a `Tree` op)

| Construct | Slice |
|---|---|
| Int/Float/Bool/Str/Decimal literals (constants built at compile time); every `Ident` read, as one op that runs `eval`'s whole chain: `get_var` with the op's slot, then `globals`, then `fn_of_sym` (a fn as a value, `fn_value`), then the undefined-identifier panic (eval.rs:246-252, 1867-1883); block, untyped `let`/`own`/`ref`, typed ones through `bind_let` | S1 |
| `=` to a local: an `AssignInPlace` op calls `assign_in_place` with the value node first, exactly as the arm does (eval.rs:277-288). It returns `Ok(false)` after a syntactic match and one `get_var`, without evaluating anything, unless the local holds a `Str` or `Array`, so `i = i + 1` and `s = s + i` on ints fall through to the compiled value and `assign_var`. A non-AX-31-shaped `Assign` skips the op | S1 |
| `BinOp` (`&&`/`\|\|` short-circuit as eval does, the Uncertain paths through `eval_binop_vals`), `UnaryOp` other than `RefMut`, fused compare-and-branch for `if`/`while` conditions | S1 |
| `if`, `while`, `for` (integer ranges; the only `for` form, ast.rs:400-410), `return`, `break`, `continue`, `?`, `Some`/`None`/`Ok`/`Err`, `FmtStr` | S1 |
| calls without `&mut` arguments whose callee is an `Ident`, through `dispatch_call` (other callees: see the last row) | S1 |
| array/tuple/struct literals, index and field reads, `.N`, `AssignTo` place writes (`xs[i] = v`, `p.f = v`, chains) | S2 |
| `match` and `while let` (via `match_pattern` into the shared `Env`), enum construction, method calls via `MethodRecv` (`chan_method` / `impl_method`) | S3 |
| `Lambda` via `make_closure`; compiled lambda bodies run by `call_closure_owned_by` | S4 |
| fast VM→VM calls; `CallMut` through an allocation-free `call_mut` | S5 |
| no new `Expr` variant: `Pure` ops for pure scalar trees, `PureLoop` for pure `while` loops, `fold_leaf` in `arr_fold` (§4 S7) | S7 |
| no new `Expr` variant: in-place and fused index ops, `for` scope reuse, frames sized for body `let`s, fib-shaped arithmetic and call ops (§4 S8) | S8 |
| `with` handlers, `spawn`, `select`, `asm`, `resume` (only inside `with` arms, which are `Tree` ops as a whole); an `Index` whose receiver is the identifier `E` or `Var` (eval.rs:515-522, the whole node, so both the moment path and its fall-through stay the tree's); any `Call` whose callee is a `StructLit` (`Chan::new(n)`, `chan::<T>`, native `M::fn`; eval.rs:446-470); `P(..)` (intercepted before argument evaluation, eval.rs:1102); any `Call` whose callee is neither an `Ident` nor a `StructLit` | `Tree`, permanently |

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
(interp.rs:4087-4113, 4175, 4316-4340). A fast call still performs, in
the order of `call_fn_entry` → `call_fn_in` (interp.rs:3811, 3895):

1. a pooled frame `Env`;
2. the depth check, with the same message (interp.rs:3916-3919, 3965-3974);
3. the `current_fn` swap;
4. the arity check, with the same panic (interp.rs:3981-3983, 1591-1598); it comes after the depth check, so a
   too-deep call with the wrong arity reports the depth limit, as now;
5. per-parameter soft unwrap and sized-int coercion from `param_coerce`;
6. the `goal_met` slot define, except for a leaf body (one `Load`, one `Bin`, or one `a[i] = v`) that
   does not name `goal_met`, which never binds it (unobservable: nothing reads it);
7. clearing `current_call_tier` when it is set;
8. the soft unwrap of the result for `ret_is_scalar`;
9. returning the argument storage: the arguments go straight into the pooled frame (no `Vec` is formed,
   so there is no `recycle_args`); the tree path keeps `recycle_args` in `call_fn_in`;
10. restoring all of the above on every exit path.

Budget: at most 300 instructions per fast one-argument call (§10 `--repros`). S5 is accepted only with a
cli test per flag showing that a flagged fn never takes the fast path (trace line
`vm: slow <fn>: <flag>`), for every flag above except `has_ref_mut`. That flag cannot be reached by a
checked program: E0605 (mut_borrow.rs:16-21) rejects a plain argument for a `&mut` param and a
`&mut`-taking fn used as a value, so every call to such a fn has a `&mut` argument and is a `CallMut`,
never a fast-call candidate. A unit test asserts that an `FnEntry` with `has_ref_mut` set is not
fast-call eligible.

**`&mut` calls.** `call_mut` becomes allocation-free. Today it allocates `argv`, `borrowed`, an unpooled
`Env::new()` and `outs` per call. After S5 it uses the frame and argument pools, and each borrowed
binding is moved back straight from the callee's frame in argument order (no move-back buffer), with
unchanged behaviour. The VM lowers `&mut` calls to a `CallMut` op over `call_mut`. Budget:
at most 600 instructions per one-`&mut`-argument call (§10 `--repros`).

#### S7 — pure scalar regions (required; added in revision 11)

At S4 (`12713cbd`, release, `perf stat`) the VM retired 23.86 G on mandelbrot and 33.78 G on arr-sum
against budgets of 15.02 G and 23.62 G, and no planned slice targeted either. Profiles put mandelbrot's
cost in per-op dispatch and operand-stack traffic: about 2,110 instructions per inner iteration, about
1,307 of them in nine generic float `Bin` ops. arr-sum's is a full closure activation per element: 675
per element, against `fold.ax`'s 330, because its two-op body (`acc + x % m`) misses S4's one-`Bin` leaf
path. Prototypes on `12713cbd` reached 5.01 G and 5.38 G with golden output (§15 "Revision 11"). S7
specifies them.

A **pure tree** is an expression built only from an `Ident` that resolution binds to a local slot, `Int`,
`Float` and `Bool` literals, and `BinOp` nodes (every operator, `&&` and `||` included) over pure trees.
Evaluating one has no effect besides its value or its panic.

- **`Pure` op.** The compiler may emit one for a pure tree with at least two operator nodes when the tree
  is the value of an `Assign` to a local that is not AX-31-shaped, the value of an untyped `let`, or an
  `if`/`while` condition. It sits before the expression's generic ops (its twin). It either delivers the
  value to its sink (store to the local, define, or branch) and skips the twin, or declines and falls into
  the twin, which evaluates the whole expression again. The reads are pure, so the second evaluation is
  unobservable. It declines when a leaf local is unbound or holds anything but a plain `Int`, `Float` or
  `Bool` (`SizedInt`, `Uncertain`, `Temporal` and `Str` included); when an operator's two operands differ
  in kind (`Int` with `Float`); when an `Int` or `Float` operator has no arm in `int_fast`/`float_fast`
  (overflow, `/ 0`, `% 0`, `MIN / -1`, `&&` on numbers); when `&&`/`||` gets a non-`Bool`; when an
  operator other than `&&`, `||`, `==` and `!=` gets two `Bool`s (the tree panics `cannot apply <Op> to
  bool / bool`; `==`/`!=` on `Bool`s give `a == b`/`a != b`, `eval_binop_vals`, value.rs:731-732); or when a branch
  sink's value is not a `Bool` (the twin's `cond_bool` then panics). The same rules hold inside
  `PureLoop` and `fold_leaf`. Its results
  are `int_fast`/`float_fast`'s, which S1's unit tests pin to `int_binop`/`float_binop`. Its code may be
  specialised on the leaf kinds of its first run; every later run re-checks the kinds and declines on a
  mismatch. `&&`/`||` may evaluate both sides: a pure right side that would decline costs only the
  decline.
- **`PureLoop` op.** A `while` whose condition is a pure tree and whose body is only `let x = <pure>`
  (untyped) and `x = <pure>` to locals compiles to a `PureLoop` op ahead of the generic loop. On entry it
  reads every env local the loop reads or assigns (not the body's own `let`s) with `Pure`'s kind check;
  any failure declines before the first iteration. Each iteration runs the condition, then the statements
  in order, in registers. A body `let` is a register only, and the iteration's `ScopePush`/`ScopePop` is
  not executed. A statement whose result kind differs from its register's current kind declines. When the
  condition is false, the assigned env locals are written back once and execution continues after the
  loop. When any step declines, the registers are restored to the iteration's start, written back, and
  control jumps to the generic loop's condition, which re-runs that iteration and produces the panic or
  slow path exactly. Eliding the scope and the per-iteration writes is unobservable: nothing in the
  region can see the `Env` (no calls, no `Tree` ops, no closures, no snapshots), and the tree-walker's
  `while` checks nothing per iteration besides its condition (eval.rs:361-371), so there is no kill,
  step or depth check to keep.
- **`fold_leaf`.** In `arr_fold` (builtins.rs:1835-1870), after an element's general call, when the
  engine is `vm` and the closure's compiled body (S4) is one pure tree over its two parameters, its
  captured bindings and literals (a single `Bin` included), later elements may run in registers: the
  captured values are read once (a pure body cannot write them), the accumulator stays in a register,
  and each element gets the kind check. The first element always takes the general `call_closure_arg`
  call, which compiles the body. On a decline at element `i` (a kind outside the register kinds, overflow,
  `% 0`), element `i` and every later one take the general call, which produces the panic exactly.
  Skipping the closure frame is unobservable: a lambda call binds its parameters without coercion and has
  no depth guard (interp.rs:4612-4688), and a pure body neither writes its captures nor calls out. Under
  `AXON_ENGINE=tree` no compiled body exists, so `fold_leaf` never runs.

**`--repros` base after S7.** `loop.ax` becomes a `PureLoop` (prototype: 229 → 92 per iteration), while
`call1.ax`'s and `mutcall.ax`'s loops cannot, since their bodies call. From S7 the `call`, `fastcall`,
`mutcall` and `swap` rows subtract `loop_generic.ax` instead of `loop.ax`: `loop.ax` with `if i < 0 { s
= 0 }` appended to the body, a never-taken branch that makes the loop ineligible (it costs 50 per
iteration on the S4 VM: `loop.ax` 229,167,352, `loop_generic.ax` 279,207,377). So that it cancels, S7
appends the same `if` to the loop bodies of `call1.ax` and `mutcall.ax`, and `swapcall.ax` (S8) has it
from the start (measured at S4: exact for `call1.ax`, +10 for `mutcall.ax`, charged to the call; `swapcall.ax`'s
count moves by up to ±20 per call between runs, so its `if` cancels only within that spread).

Gate: red tests `vm_pure_mandel_loop` (mandelbrot's `main` prints `vm: pure main <p> exprs, 1 loops`)
and `vm_pure_fold_leaf` (arr-sum prints `vm: fold-leaf main::lambda#0`), both failing on S4, which prints
neither line; every `vm_pure_*` behaviour test of a `Pure` or `PureLoop` op places its expression in a sink (the
value of a non-AX-31-shaped `Assign`, an untyped `let` value, or an `if`/`while` condition) and asserts
the `vm: pure` line for that body, with `1 loops` for `vm_pure_overflow_replays` and
`vm_pure_sized_declines`, so it cannot pass without the op; `vm_pure_fold_decline` asserts `vm: fold-leaf
main::lambda#0` (printed at element 2, before the decline at element 5); behaviour tests `vm_pure_overflow_replays` (an `i64` overflow in the third iteration of a
`PureLoop` gives the tree's panic text and exit 101, and a `println` after the loop never runs), `vm_pure_sized_declines` (an `i32` local in a pure loop gives the tree's output),
`vm_pure_short_circuit` (`let r = b != 0 && a / b > 1` and `if b != 0 && a / b > 1` with `b = 0` print
what the tree prints), `vm_pure_bool_ops` (through untyped lambda parameters, which `axon check` accepts:
`let c = a < b && b < a` in the lambda body on `Bool`s, and `if`/`while` on `a + b * a` with `Int`s, each
giving the tree's panic and exit 101) and
`vm_pure_fold_decline` (`% 0` at element 5 of an `arr_fold` panics as the tree does);
`vm_perf_gate.sh --programs mandelbrot,arr-sum,collatz` and `--repros S1,part,fold,foldmod`.

#### S8 — qsort and fib superops (required; added in revision 11)

After S5's budgets, qsort and fib-recursive still need cuts no earlier slice makes. Measured at
`12713cbd`: a 3-argument `swap(&mut a, i, j)` body costs 801 beyond an empty-body call (revision 5's
§10 estimate was ≈ 150); qsort's `main` costs 1.09 G; fib's body ops (`n < 2`, two subtractions, the
add, the `if`, dispatch) cost about 290 per call, while the budget leaves 485 per call, so a
300-instruction fast call does not fit (§10). S8 adds cost-only ops. Each keeps the generic op's
evaluation order and takes the generic path, through the unchanged S2 helpers, whenever its fast
condition fails:

- `a[i] = v` to a local array: when the index is an `Int` in bounds and the array is uniquely owned
  (`Rc::get_mut` succeeds, which also excludes weak references), the element is replaced in place, which
  is what `write_place`'s `Rc::make_mut(items)[i] = v` does on such an array (interp.rs:4846-4851).
  Otherwise `place_index`, then `write_place`.
- Fused forms without the operand stack: `a[i] = x` (`x` a local or literal), `a[i] = b[j]` and
  `let t = a[i]` (untyped), where `a` and `b` are locals and `i` and `j` are locals or literals. The value
  is read first (`b[j]` with `index_in_place`'s conversion and panics), then the target index is
  converted, then the write: S2's order.
- `for` scope reuse: when an iteration's scope holds only the loop variable, `ForNext` overwrites that
  binding with the next value instead of `env.pop()`, `i += 1`, `env.push()` and `define_var`. The `Env`
  afterwards is the same.
- Frames sized for the body: a pooled frame keeps capacity for the callee's slots, its `let`s included,
  so a body `let` does not grow `vars` on a hot call.
- fib-shaped arithmetic and calls: a `local ± Int literal` op and a stack-plus-stack add with `int_fast`
  tried first, an argument computed straight into the call's argument buffer, and a
  compare-branch-return for a leaf arm. Each declines to `Bin` and to S1/S5's call path exactly as S1's
  ops do.

The op shapes are open; the budgets are fixed. Gate: `vm_perf_gate.sh` (all five programs) and
`vm_perf_gate.sh --repros` (every row, `swap` included), with the `swap` row as the red check (at
`12713cbd` the swap call costs about 2,641; with S5's 600-instruction `&mut` call and an 801 body it would
still cost about 1,400 [INFERENCE until S5 is measured]); behaviour tests `vm_superop_write_shared` (a
write to an array another binding holds leaves that binding unchanged), `vm_superop_write_panics`
(out-of-bounds, negative and non-`Int` indexes give the tree's texts, value evaluated before index),
`vm_superop_for_shadow` (a `for` body `let` shadowing the loop variable) and
`vm_superop_fib_overflow` (the `local ± literal` op at `i64::MAX` panics as the tree does).

#### S9 — deferred compile (required; added in revision 15)

S0–S8 compile every fn-table body and every resolved lambda body on its first entry, so a body entered
once pays its compile and gains nothing. compilebench AX-58 measured that on `big-compile` (1,002 fns,
each called once and holding a short literal `for`): run `20261009T115201Z` at `2a83e6ab` retired
1.076× the instructions of the tree-walker at `886aae53`.

- **Rule.** Under `AXON_ENGINE=vm`, a body that would be compiled (`compiled: Some`, no whole-body
  exclusion) compiles on its **second** entry; its first entry runs the body on the tree-walker (`eval`)
  inside the same `call_fn_in` or `call_closure_owned_by` activation. It still compiles on its first
  entry when it is *hot on entry*: it holds a `while`, a `while let`, a `for` whose bounds are not both
  integer literals or that runs more than 32 times (`long_for`, `COLD_FOR_MAX`, vm/mod.rs:114-132), or
  a loop inside a loop; loops in lambda bodies inside it count too. The resolver works this out while it
  walks the body anyway (`Builder`'s `loops`/`hot` counts, sym.rs:972-1010; a body is hot when `hot`
  grows while it is resolved, sym.rs:866-870 and 1109-1115), into `FnEntry.hot` (set by `Interp::build`
  from `Resolution::hot_fn`, interp.rs:3525) and `LambdaBody.hot`; an extra walk per first entry cost
  `big-compile` ≈ 5.4 k instructions per fn (§15 "Revision 15"). Each body records its first entry in an
  `entered: Cell<bool>` (`FnEntry`, sym.rs:409-410; `LambdaBody`, vm/mod.rs:252-253), set on that entry
  whichever path runs it (vm/mod.rs:1322, 1412).
- **Why it is unobservable.** Only the body step of the activation differs on a first entry: `eval`
  instead of the compiled ops, and the tree-walker is the reference. Depth guard, arity check, coercions,
  refinement checks, provenance and scope rules are the wrapper's, unchanged. Compiled code depends only on
  its body, so compiling at the second entry builds the same `Body` the first would have. S7's `Pure`
  ops re-check their leaf kinds on every run (§4 S7), so which run is first is cost only.
  A first entry reached from compiled code goes through S5's fast frame (`call_fast`, `call_fast_arg`,
  `call_mut_op`) into `fast_body`'s arm for an uncompiled or lone-`Tree` body, which runs it through
  `run_body` inside that frame (vm/mod.rs:2881-2884); before S9 that arm always compiled first.
  `call_mut_op` then reads `&mut` params back by slot when the frame holds no more than the params and
  `goal_met` (vm/mod.rs:3002). That stays exact after a tree body: `eval_block` pops its scope on every exit
  (eval.rs:723-738), so the body's own bindings are gone; a `define` at the base scope makes the frame
  longer and the read-back falls to the name lookup. `vm_defer_mutcall_after_tree_entry` covers it.
- **Consumers that assumed a compiled body after one entry.** `arr_fold` offers `fold_leaf` the rest
  of the array after element 1 and again after element 2 (builtins.rs:1858-1867): when the closure's body
  was deferred, element 2's call compiles it. `fold_leaf` folds nothing while the body is not compiled.
  S8's `CallMut` resolves and caches its moves form (`MovesCall`) only once the callee's body is
  compiled (vm/mod.rs:1850-1856); a call that finds it uncompiled takes `call_mut_op` and leaves the
  cache empty, so a later call still finds the moves form. S5's `leaf_arm`/`leaf_call` and S7's caches
  read the current compiled state on every call already.
- **`AXON_VM_EAGER=1`** restores compile-on-first-entry for every body (§3). The `vm_` cli tests run
  their VM leg with it, so a body called once is still compiled code under test, and
  `vm_same_both_engines` and `vm_fastcall_case` also run the default (deferred) leg and compare it with
  the tree. `vm_parity.sh` runs both legs (§8).
- On compilebench's five gated programs the hot code compiles on its first entry (collatz, mandelbrot
  and qsort `main`s hold `while` or computed-range `for` loops) or on its second (fib, arr-sum's fold
  lambda); arr-sum's `main`, a 10-iteration literal `for`, and fib's `main` now run on the tree.

Gate: red test `vm_defer_compiles_on_second_entry_unless_hot` (fails before S9: no `vm: defer` line,
every body compiled on its first entry), with `vm_defer_lambda_and_fold_leaf` and
`vm_defer_mutcall_after_tree_entry`; `vm_parity.sh` (slice S9: runs 3-5, §8); the whole suite under
both feature sets with `AXON_ENGINE` unset (the deferred VM), `tree`, and `vm` with `AXON_VM_EAGER=1`
(every body compiled on its first entry, so the bodies the suite's programs run once are compiled code
under test); `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` under the same three, whose wasip1 legs
(`wasm_parity.sh`, `wasm_fs_parity.sh`, `wasm_host_await_parity.sh`) forward `AXON_VM_EAGER` to
wasmtime along with `AXON_ENGINE`. The `wasm32-unknown-unknown` leg has only `axon_set_engine` and
runs deferred compile only. Also `vm_perf_gate.sh` (all five) and `--repros` (every row);
`vm_wasm_depth.sh --require-default-stack` (chains and nesting probes); and
`vm_perf_gate.sh --compile`: compilebench `big-compile` (a fixture copy), median of five, deferred
≤ 1.01 × tree on the same binary and ≤ 0.95 × eager (the red check: eager is about 1.09 × tree). Run to
run, the deferred and tree medians differ by under 0.2 %, so equality cannot be gated.

#### S10 — array elements in pure loops (required; added in revision 27)

At `5864c423` (S0–S9, VM default) compilebench `sieve` retired 53.53 G (run `20261010T015516Z`)
against CPython's 1.09 G. Any index in a loop body made the loop ineligible for S7's `PureLoop`, so
the whole loop ran generic ops: on a 1M-iteration loop with a `PureLoop`-ineligible base, `s = s +
xs[i]` cost 290 instructions per iteration more than the base and `if flags[i] { s = s + 1 }` 221
(release, `perf stat`), most of it the element's `Value::clone` and operand-stack traffic. S10
widens `PureLoop` to array elements, `if` statements and integer-range `for` loops:

- **Element leaf.** Inside a `PureLoop`'s trees, `xs[i]` is a leaf when `xs` is an identifier that
  resolution binds to a local slot, other than `E` and `Var` (the `Index` arm's moment predicates,
  eval.rs:512-522), and `i` is a pure tree. It declines when the index is not an `Int`, is negative
  or out of bounds, or when the element is not a plain scalar of the array's kind (the kind of its
  element 0 at loop entry). Outside a loop, `Pure` ops do not take element leaves.
- **Pure statements.** A loop body is pure when every statement is an untyped `let x = <pure>`
  (top level of the body only), `x = <pure>` to a local, `xs[<pure>] = <pure>` to a local array (as
  above), or `if <pure> { .. } else { .. }` (`else if` included) over pure statements, nested at
  most `PURE_DEPTH` (16) deep. An element write declines when the index fails as a read's would,
  when the old element is not a scalar of the array's kind, or when the value's kind differs from
  it (a write that would change an element's kind always runs on the generic loop).
- **`PureFor` op.** A `for v in a..b` (or `a..=b`) whose body is pure compiles to a `PureFor` op
  after the bounds are evaluated and checked (two `StrictInt` ops, compile.rs:398-401), ahead of the
  generic `ForTest`. The counter and `v` are registers; an assignment to `v` lasts to the end of its
  iteration, as on the tree, whose counter is separate from the binding (eval.rs:403-430). The
  iteration's scope is never pushed. On a decline the counter is left at the iteration that declined
  and control enters `ForTest`, which re-runs it.
- **Arrays.** The register code is built on the loop's first entry. If an array the loop indexes (at
  most four) is not then bound to a non-empty `Array` whose element 0 is a plain `Int`, `Float` or
  `Bool`, the build fails and the op declines on that entry and every later one. On a later entry
  the op checks only that each array is bound to an `Array`; element kinds are checked per access.
  The arrays are taken out of the `Env` while the loop runs and put back when it ends or declines;
  nothing in the region can see the `Env` (§4 S7). The first write to an array another binding
  shares copies it (`Rc::make_mut`), as `write_place` does (interp.rs), so the other binding keeps
  its elements. Each iteration logs its element writes (array, index, old element); a decline undoes
  them last-first, restores the registers to the iteration's start, writes the locals and arrays
  back, and the generic loop re-runs that iteration, which produces the panic or slow path exactly.
  A copy made before the decline stays made; Axon has no array identity (no pointer comparison
  exists in the interpreter), so the copy is unobservable.
- Env locals, body `let`s, literals and temporaries have eight registers each. A loop that needs more
  still emits its `PureLoop`/`PureFor` op (the trace counts it), but the build fails and the op
  declines on every entry, so the generic loop runs it.

Gate: red test `vm_index_sieve_loops` (sieve's `main`, bound 1000: `vm: pure main 1 exprs, 2 loops`;
S9 prints `0 loops`); behaviour tests `vm_index_writes_copy_and_replay_panics` (a write to an array
shared with another binding leaves that binding's elements; an `i64` overflow in an element sum in the
last iteration of an inclusive `for` gives the tree's panic), `vm_index_out_of_bounds_and_for_variable`
(an out-of-bounds write gives the tree's panic; assigning the `for` variable lasts one iteration) and
`vm_index_decline_undoes_the_iteration` (an `i32` element in an `i64` array declines after the same
iteration wrote another array; that write is made once); `vm_perf_gate.sh --programs sieve` with the
budget in §10 (S9 fails it at 53.53 G); every `vm_perf_gate.sh` program and `--repros` row;
`vm_parity.sh` (S10 lowers no new `Expr` variant, so its list is S9's); `vm_wasm_depth.sh
--require-default-stack` (the loop compiler's new recursions are over `PURE_DEPTH`-bounded trees);
the whole suite under the S9 modes.

#### S11 - pure-`i64` function tier (required; added in revision 27)

After S10, fib-recursive is the one compute program where the VM retires fewer instructions than
CPython but runs slower: S10 (`944fe056`) retired 2,295,409,337 instructions against CPython 3.14.4's
3,423,642,492 (run `20261008T142344Z`, 0.1189 s at about 3.6 instructions per cycle), and S9 ran
1.29× CPython's wall time (run `20261010T015516Z`, Axon `5864c423`) at about 2.9 instructions per
cycle. (sieve is slower too but retires 22× CPython's instructions; S10 is its slice.) A one-argument
call costs about 244 instructions on the generic fast-call path (`call_fast_arg`, a pooled `Env`
frame, the op loop re-entered per call). S11 runs qualifying fns as register code on an explicit
frame stack.

- **Qualifying.** A fn-table entry qualifies when it passes `fast_call_blocker` (S5), declares every
  param (at most eight) and its return as `i64` (the type named `i64`; an alias such as `type N = i64`
  disqualifies), its compiled body fits 255 registers, and its compiled ops (`pure_shape`,
  vm/purefn.rs) are only: a param `Load`, an `Int` `Const`, `Bin` over int arithmetic and comparisons
  (operands on the stack, params or literals in any mix except a param or literal on the left of a
  stack operand, as in `n + g(n, 1)`, which disqualifies); a branch whose
  condition is `param <cmp> int-literal` (`BranchLocalInt` without push, `BranchReturn` returning an
  int literal or a param) or a `bool` on the stack (`BranchFalse` without push); a forward `Jump`;
  `Return`; and calls that are proven fast calls (S5) to a qualifying fn (`CallFastLocalInt` whose
  operator is `+`, `-` or `*`, or `CallFast` with stack or inline literal/param arguments). A condition comparing two locals
  (`BranchLocalLocal`), with the literal on the left or a computed operand (`BranchCmp`), or using
  `&&`/`||` (`ShortCircuit`) disqualifies the fn: the tier is a minimal shape for fib-like recursion,
  not an `Int` subset. A refinement-typed, sized (`i32`) or `&mut` param, an `@[agent]` or goal fn,
  or an `Uncertain` return disqualifies it. A fn is decided with every undecided fn its calls reach (its
  group), so mutually recursive fns qualify together; a member that cannot qualify is marked and the
  group rebuilt without it. A fn whose body is not compiled yet (S9) or whose calls are not yet proven
  is retried on later calls and never qualifies after 64 tries.
- **Running.** The code (`FIns`: checked add, subtract and multiply with a register or constant, the
  other `Int` operators through `int_fast`, compare, branch, call, return) runs in `run`'s loop with a
  256-register window per frame on a pooled `Vec<i64>` and a pooled frame stack, with no `Env`, no
  `Value`, no op loop and no native recursion. Entry points: `CallFastLocalInt`, `CallFast` with stack
  arguments, and `call_fast_inline`. A callee whose body is one op keeps S5's `leaf_call`, which is
  cheaper; a one-op body whose op is a call (`fn a(n: i64) -> i64 { b(n) }`) skips the tier, `leaf_call`
  declines it and it takes a generic frame (a cost only; its callee still runs on the tier). An
  argument that is not a plain `Int` enters nothing.
- **Declining.** An instruction that would panic on the tree (overflow, `/` or `%` by zero,
  `MIN / -1`) or a call past the depth the limit leaves (`max_depth` minus the caller's depth, minus
  one for the outermost call) declines the whole outermost call. Nothing it did is observable (no
  effect, binding or output), so the caller replays the call on the generic path with nested tier
  entry off (`pure_replay`, restored after), which produces the tree's panic text, in the tree's
  order. A shift out of range wraps on the tree and in the tier alike. A declined call costs at most
  twice its generic cost, and only a call that panics declines.
- **Trace.** Under `AXON_VM_TRACE=1` each fn built on the tier prints `vm: purefn <fn> <n> ins` once.

Gate: red test `vm_purefn_fib_in_registers` (`vm: purefn fib 8 ins`; S10 prints no `purefn` line);
behaviour tests `vm_purefn_panics_replay`, `vm_purefn_depth_boundary` (a chain completes at
`AXON_MAX_DEPTH=N` and panics at `N - 1` as the tree does, entered from `main` and from depth 12),
`vm_purefn_not_qualified` (each disqualifier alone: a two-local compare, a literal on the left, a
computed operand, `&&`, `||`, and an `Uncertain` return on a builtin-free body),
`vm_purefn_replay_flag_restored`, `vm_purefn_mutual_and_multi_param`; unit
test `purefn_build_validates`; `vm_perf_gate.sh` (fib-recursive at most 1,141,214,164, §10; every other
program's median at most 0.5 % above its S10 median, recorded in the script and §10) and every
`--repros` row; `vm_parity.sh`; `vm_wasm_depth.sh --require-default-stack`, whose `plain` chain now
runs on the tier and whose `plain_generic` chain (a body the tier refuses) keeps the generic fast-call
frames probed; the script fails unless `plain` prints a `vm: purefn` line and `plain_generic` prints
none, in both profiles; the whole suite under the S9 modes.

#### S12 - wasm32 traps outside the stack budget (required; added in revision 27)

§4 Recursion leaves three recursions outside the wasm32 budget, each filed with a trap repro:
dropping a deeply nested value (compilebench AX-59), the front end on deep source (AX-60) and the
closure-body copy in `host_await_val` (AX-61). Under wasmtime's default stack each ends in `wasm trap:
call stack exhausted` (exit 134) instead of a panic or a diagnostic. S12 removes the three traps on
wasm32. Natively it changes two paths. First, the parser's string-interpolation slots now parse from
the enclosing literal's `expr_depth`, so a slot shares native's 4,000 limit with its literal, and a
refusal inside a slot is reported unwrapped. Second, `ast::children` now yields a `match` arm's guard,
so on both targets every pass built on `walk_expr` or `value_position_idents` sees guards. The
consumers are:
- borrow checking: a guard that reads an argument borrowed `&mut` in the same call is E0606 (before,
  the program ran);
- the resolver: W0002 names the self-referencing `let x = match .. { n if x > 0 => .. }` form;
- capability checks, and the checker's purity (E1207 for a non-pure call in an `@[pure]` fn's
  guard), `no_alloc`, totality and allocation scans;
- eval's `mentions_var`: the in-place append path no longer runs when the operand's guard assigns
  the target, so `axon run` output follows plain evaluation order (`s = s + match .. { n if { s = "zz"
  true } => "a", .. }` prints `starta`, before `zza`);
- the VM's `binds`;
- native codegen: the E0910 per-call `tier:` refusal (codegen/mod.rs:1228), so `axon build` refuses a
  guard that calls `ai_complete(.., tier: ..)`; `expr_calls` (the `goal_run` registry and the
  `ai_complete` attribute-tier refusal); escape analysis `collect_binders`; and `written_place_roots`
  (AX-08 copy-on-alias), so a guard that writes `a[0]` after `let b = a` no longer changes `b` in the
  built binary (`1 9 1`, as `axon run` prints; before `9 9 1`).
The CHANGELOG records each. Six cli tests gate six of these consumers: `native_guard_borrow_e0606`
(borrow checking), `native_guard_w0002` (the resolver), `native_guard_pure_e1207` (checker purity),
`native_guard_assign_in_place` (`mentions_var`), and `native_guard_tier_e0910` and
`native_guard_write_unaliases` (codegen E0910 and `written_place_roots`). The capability checks,
`no_alloc`, totality and allocation scans, the VM's `binds`, `expr_calls` and `collect_binders` have
no guard test of their own; they see guards only through the shared `ast::children` change those six
tests exercise. Native costs are unchanged. Apart from these two changes,
the reference semantics are unchanged on both targets, except that wasm32 refuses source deeper than
its nesting limit (below).

- **Drop (AX-59).** Any container can sit on a data cycle that grows without bound: the checker
  does not keep values shallow (below), so every container that holds a `Value` drops through the
  bound. `Value::Dict` holds a `DictMap` (a newtype over
  the `BTreeMap`, both targets), array and tuple elements an `Elems` (`Value::Array(Rc<Elems>)`), the
  `Some`/`Ok`/`Err` payload a `VBox`, a channel's queue a `Queue`, and a closure's code and capture
  cell a `Held`: newtypes on both targets, derefing to the old type, zero-cost natively. An untyped
  `dict_get` result unifies with any type (no occurs check, infer.rs:2047-2048), so arrays, options
  and tuples nest without bound through it. On wasm32 only, `Fields`, `ClosureVal`, `DictMap`,
  `Elems`, `VBox` and `Queue` implement
  `Drop`: a container drops its contents through `drop_bounded`, which runs the plain drop glue below
  `DROP_INLINE_DEPTH` (16) nested drops and past it moves the contents to a thread-local `DROP_LATER`
  list that the outermost drop empties after its own glue returns. Only contents owned uniquely at
  drop time move; a shared `Rc` is only decremented. No `Value` payload has a drop with an effect, so
  the order is unobservable. Native keeps the plain glue (its interpreter thread stack is sized from
  `max_depth`); a version on both targets cost native 1.3-4.3 % instructions (prototype).
- **Front end (AX-60).** On wasm32 `MAX_EXPR_DEPTH` is 224 (native stays 4,000): AX-56's rule, 448 KiB
  over the costliest unit, the parser's worst frame chain between two counted levels as
  `wasm_stack_budget.py` computes it from the disassembly (release 1,904 bytes: `parse_expr`,
  `parse_goal_block`, the precedence layers and `parse_postfix`; 224 × 1,904 = 426,496 ≤ 458,752;
  debug 1,664). On wasm32 `parse_expr`, `parse_pattern`, `parse_type_atom` and `parse_attr_atom` (a
nested `[` or `-` in an attribute argument) count a level as
  `parse_primary` already does (so a parenthesis, block, array literal or `Some` expression costs two
  levels and 110 of them nest; a `Some` pattern costs one), and so do an `else if`
  chain and a `match` subject and guard; a root
  expression (a statement, contract or constant, and a `let`'s inline-refinement predicate, which is
  kept outside the statement's tree) is checked with an iterative height walk over
  `walk_expr`'s children (a `match` arm's guard included), which bounds the operator and postfix chains
  the parser builds in a loop and
  so every later recursive walk over the AST (resolver, compiler, clone, and the checker's walk over
  expressions). The chains are also bounded while they are built: the operator layers and
  `parse_postfix` track the height of the node they build, and a node past the limit has its children
  dropped iteratively (`ast::clear_children`) and marks the parse refused, so the parser never holds a
  tree much taller than 224 and no error or drop path recurses per level; `parse_match` checks an arm's
  guard and body before an or-pattern copies them, and moves them into the last pattern's arm. On
  native every one of these helpers is a no-op. It does not bound the checker's recursion over *types*,
  which can be deep from shallow
  source (`let a{i} = [a{i-1}]` repeated); that is out of scope (compilebench AX-63). Past the limit
  the parse fails with E0000 `expression nesting too deep (limit 224)` at the start of the enclosing
  statement (for `let s = ((...` the `let`), exit 2. Only a chain cut defers the refusal. When an
  operator or postfix chain is cut while it is built (in an interpolation slot as well), the refusal
  waits for the end of its root expression, and a syntax error inside that root wins and is reported
  as natively (for example a 100,000-term chain ending in a dangling `+`). Every other refusal is
  reported at once as E0000 at the root's start, even when native would report a later syntax
  error. That covers parenthesis, block, pattern, type or `else if` depth, and a match arm body or
  inline-refinement predicate taller than the limit without a cut. So is an error after the root
  ends (a later statement or item, the enclosing `}`, the rest of a fn signature or attribute).
  A root is any expression the parser starts outside every counted level: a fn-body statement, a
  top-level `let`, a contract or constant, a `@[verify(..)]` predicate, the `where` predicate of a
  `type` definition or of a fn parameter or `let` annotation that is not inside a tuple type, and a
  top-level handler definition's arm body. Each is refused at its own first token (`type P = i64
  where ((..` at the predicate, column 20; `fn f(x: i64 where ((..` column 19; `@[verify(((..` column
  10; a 600-term handler arm body column 36), except a `let` annotation's predicate, which is refused
  at the `let`. Source outside any root is reported at the level where it trips: a type (a `type` definition, a fn signature, a field; for `type P =` and 300 `Option<`,
  column 1,578), a top-level handler definition's `on` pattern (300 nested `Some(`, column 1,150)
  and an attribute argument (the 225th `[` or `-`, column 234). A chain cut inside a type outside
  an expression (a tuple type's refinement predicate) is refused at the token after the outermost
  type that contains it, where the cut settles: `type P = (i64 where 1+..+1 > 0) where true` at its
  second `where` (column 1,226), `fn f(x: (i64 where 1+..+1 > 0))` at the parameter list's `)`
  (column 1,224), each 600 terms. `parse_type_def`'s speculative base-type parse passes a wasm32
  nesting refusal through instead of rewinding to the enum parse. Native keeps the rewind.
  Source that ran
  on wasm32 between the limit and its old trap point (in release, for example, a 224-819-term `+`
  chain) is now refused; the CHANGELOG states the limit.
- **`host_await_val` (AX-61).** Both wasm32 `host_await_yield` drivers take only a `Str` request, so a
  copy of any other payload is waste. On wasm32 `SendValue::request` returns a `Str` as today and
  otherwise scans the payload with an explicit stack in `from_value_at`'s child order, reporting a `Chan`
  or `Handle` with the same path text, and copies nothing; the call then ends in the existing `no host
  driver` panic. Native calls `from_value` exactly as today.

Out of scope, all recursing once per data level outside the budget and listed in
`wasm_stack_budget.py`'s `ALLOW` list: structural `==` and formatting of deep values (`values_equal`,
`display`) and cloning a deep value (`Value::clone`; a `Some`/`Ok` chain read back through `dict_get`
is cloned per step and completes about 2,700 levels in debug and 4,000 in release), all compilebench
AX-62; and the checker's recursion over deep inferred types (AX-63). Also out of scope, and not a
recursion in Axon's code: on the debug wasm32 build only, the logos-generated lexer recurses once per
character of a string literal or block comment, so a string of about 11,000 characters or a
50,000-character `/* */` comment exits 134 (release and native run them; compilebench AX-65,
which predates S12). This bounds the slot-chain probes, which stay below it. Also out of scope: three
checker walks that recurse once per link of a chain of declarations, not per level of nesting, so
the front-end limit does not reach them: `capabilities::check_expr` following a `@[contained]` fn's
call chain, `CheckCtx::total_can_reach` over `@[total]` fns, and `resolve_ast_type` chasing
refinement-type names (`type A{i} = A{i-1} where true`). Flat source traps each on wasm32 (release
at 600, 3,000 and 5,000 links) while native runs it; compilebench AX-66, which predates S12.

Gate: red tests `wasm_drops_deep_values_ax59`, `wasm_deep_source_gives_e0000_ax60`,
`native_nesting_limit_unchanged_ax60`, `native_guard_borrow_e0606`, `native_guard_w0002`,
`native_guard_assign_in_place`, `native_guard_pure_e1207`, `native_guard_tier_e0910`,
`native_guard_write_unaliases` and `wasm_host_await_val_no_copy_ax61` (§8), with
`wasm_nesting_parity.sh` (every case on both the debug and the release `axon-run.wasm`) `ax59`
covering enum, dict, closure, array, tuple, option and result chains
and `ax60` nested interpolation slots (each slot parses from the enclosing literal's depth), `+`
chains of 10,000, 30,000 and 100,000 terms, match guards of 300, 1,000 and 3,000 terms, a 3,000-term
arm body, a 5,000-term or-pattern arm body, inline-refinement predicates of 600 and 3,000 terms and
compound chains (each E0000, exit 2), 100,000-term chains followed by a syntax error, and slot chains
of 1,500 and 2,000 terms followed by a missing `)`, a dangling `+` or a stray token in the slot
(native's syntax error and position, exit 2), and the out-of-expression refusals above at their
stated columns (`tdef_where`, `tdef_enum`, `tdef_tuple_pred`, `attr_bracket`, `attr_neg`,
`handler_pat`, `tdef_chain`, `fn_param_chain`; native prints `ok`, exit 0); nest
probes `tests/fixtures/vm_depth/nest/{drop_list,drop_dict,drop_closure,drop_array,drop_tuple,drop_option,drop_result,host_await_closure,front_interp,front_arm,front_guard,front_or_arm,front_refine}.ax`
and the front-end probes in `vm_wasm_depth.sh`, which never exit 134 in either profile or engine,
and each front probe reaches a depth above 0 (the script fails a reach of 0)
(`drop_option`/`drop_result` run at 1,000, under the clone limit above);
`wasm_stack_budget.py` with four new checks (every `Value` drop component is acyclic once its
`drop_bounded` nodes are removed, so no sub-cycle avoids the bound, and `DROP_INLINE_DEPTH` times its
summed frame fits the 64 KiB margin: debug 16 × 2,784 B, release 16 × 1,232 B; the parser
component's worst frame chain times 224 fits 448 KiB; every recursive component reachable from
`Parser::parse_program` either contains a charged call or holds no `Parser` method and is classified
in `ALLOW`, and every charged name matches a `self.nested(Self::..)` site in parser.rs); `vm_perf_gate.sh`, whose `REF` column holds
fib-recursive's median at the S11 commit `2627dad7` next to the S10 medians, so every program's
`instructions:u` stays within 0.5 % of the code S12 is built on; the whole suite under the S9 modes.

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
| wasm32 (`axon-wasm`, `axon-run` on wasip1) | same as the native interpreter, except the depth at which the wasm32 stack budget panics, which can differ by many levels between engines and compile modes (§4 Recursion; deferred VM ≥ tree is gated) |
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
  `scripts/vm_wasm_depth.sh`: on wasm32 both engines complete 1.3 × the guard on every chain (five from S11) in the
  linear stack at every slice; S6 does not flip the default while any default-native-stack VM depth is
  below the tree's (§4); since AX-56 every default-stack depth must also reach 1.2 × the guard (128).
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
  358 at `886aae53`). Red test first: `vm_engine_scope_leak` runs AX-57's program (a `&mut [i64]` param shadowed by
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
  diffs stdout, stderr, exit code, the audit ledger and `provenance.jsonl`. From S9 the VM side is two
  diffed runs: `AXON_ENGINE=vm AXON_VM_EAGER=1` (every body compiled on its first entry, so code run once
  is still compiled code under test) and `AXON_ENGINE=vm` alone (the shipping deferred compile, §4 S9).
  Normalisation is one rule: drop the `ts_ms` and `run_id` keys from every provenance row. Both are
  wall-clock values the virtual clock does not reach (`now_ms` in interp.rs:4922, `generate_run_id` in
  main.rs:4329); every row has `ts_ms`, and the `run_start` row (provenance.rs:606-628) has `run_id`.
  Coverage comes from a last, undiffed run per file under `AXON_ENGINE=vm AXON_VM_EAGER=1 AXON_VM_TRACE=1`. Besides the
  per-body line (§3), the trace prints one `vm: tree-op <name> <Variant>[(<shape>)]` line per `Tree` op it
  compiles. `<shape>` names the exceptions inside a lowered variant: `Call(struct-lit)`, `Call(P)`,
  `Call(computed)` (a callee that is neither an `Ident` nor a `StructLit`), `Index(E|Var)`, and through S4
  `Call(&mut)`. The script prints `vm_parity: <n> files, <c> bodies, <l> lowered ops, <t> tree ops,
  0 differ`. It fails if any file differs, or if any `tree-op` line names a variant or shape that the
  script's per-slice list marks lowered. That list is the §4 lowered-set table, kept in the script and
  extended by each slice. The check is per op, not a total over the corpus, so adding or removing an
  example cannot fail it unless that example hits a construct the current slice claims to lower.
- [ ] Whole suite: `cargo test -p axon-core` (both feature sets) with `AXON_ENGINE=vm` exported, so the
  CLI children inherit it, again with `AXON_ENGINE=tree`, and again with `AXON_ENGINE=vm
  AXON_VM_EAGER=1` (every body compiled on its first entry, §4 S9); all three green.
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
- [ ] wasm depth: `scripts/vm_wasm_depth.sh` (§4 Execution, Recursion) gates every slice S0–S5, S7 and S8 on the
  linear-stack bound and the guard panic (450, 128 since AX-56), comparing against the tree from the
  same commit; default-stack depths were reported until AX-56 made 1.2 × the guard required. S6 and
  every slice from S9 run it with `--require-default-stack`.
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
    (§4 S5; the `vm: slow` line; fails on S4, which prints none);
  - S7 `vm_pure_mandel_loop` and `vm_pure_fold_leaf` (the `vm: pure` and `vm: fold-leaf` lines; fail
    on S4, which prints neither). S7 lowers no new `Expr` variant, so these assert trace lines, not tree
    nodes;
  - S8: the `swap` row of `vm_perf_gate.sh --repros` (§4 S8). S8 changes cost only, so its red check is
    a budget, and its `vm_superop_` tests are behaviour tests that pass before and after;
  - S9 `vm_defer_compiles_on_second_entry_unless_hot` (no `vm: defer` line before S9);
  - S10 `vm_index_sieve_loops` (sieve's `main` prints `vm: pure main 1 exprs, 2 loops`; S9 prints
    `0 loops`);
  - S11 `vm_purefn_fib_in_registers` (`vm: purefn fib 8 ins`; no `vm: purefn` line before S11);
  - S12 `wasm_drops_deep_values_ax59`, `wasm_deep_source_gives_e0000_ax60` and
    `wasm_host_await_val_no_copy_ax61` (each traps, exit 134, on wasm32 before S12), and
    `native_guard_borrow_e0606` (exit 0, prints `5 5`), `native_guard_w0002` (W0002 without the
    "re-declares" wording), `native_guard_assign_in_place` (prints `zza`
    and `3`), `native_guard_pure_e1207` (exit 0, prints `1`), `native_guard_tier_e0910` (`axon build`
    succeeds) and `native_guard_write_unaliases` (the binary prints `9 9 1`), each natively before
    S12's guard walk.

Every `tests/fixtures/` path in this spec is under `crates/axon-core/`.

### 9. Acceptance criteria

- [ ] `scripts/vm_parity.sh` exits 0 with 0 differing files (eager and deferred VM runs both diffed
  against the tree) and no `tree-op` line of a variant or shape the S9 list marks lowered (S7-S12
  lower no new variant, so it equals S5's).
- [ ] `scripts/vm_perf_gate.sh --compile` exits 0: compilebench `big-compile`, median of five, deferred
  VM ≤ 1.01 × tree and ≤ 0.95 × `AXON_VM_EAGER=1` on the same binary (S9).
- [ ] `scripts/vm_wasm_depth.sh --require-default-stack` exits 0 on debug and release, no nest probe
  exits 134, every front probe exits 0 or 2 with E0000 and reaches a depth above 0, and
  `scripts/wasm_stack_budget.py` passes on both (S12).
- [ ] `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` exits 0 under `AXON_ENGINE=vm` (the default),
  under `AXON_ENGINE=tree`, and under `AXON_ENGINE=vm AXON_VM_EAGER=1`.
- [ ] `vm_engine_scope_leak` and every per-slice red test (§8) pass.
- [ ] `cargo test -p axon-core --no-default-features` and `--features codegen` green under
  `AXON_ENGINE=vm`, `AXON_ENGINE=tree` and `AXON_ENGINE=vm AXON_VM_EAGER=1`.
- [ ] `scripts/vm_perf_gate.sh` exits 0 on the compilebench host (not a skip; see §10): all six
  programs, fib-recursive at most 1,141,214,164 (S11), sieve at most 26,764,304,040 (S10).
- [ ] `scripts/reference_gate.sh` in sync (three env vars registered).

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
same program with the `arr_fold` line removed) / 10 M, per element. `foldmod.ax` (S7) is `fold.ax` with
arr-sum's body, `|acc: i64, x: i64| acc + x % m` with a captured `let m = 7`, costed the same way.
`swapcall.ax` (S8) is the 1M-iteration loop with `swap(&mut a, 0, 1)` and qsort's three-statement `swap`
body; its cost is per call, body included. From S7, `call`, `fastcall`, `mutcall` and `swap` subtract
`loop_generic.ax` (§4 S7). `--programs SEL` runs a comma-separated subset of the five programs.

| Repro | Slice | Budget | Tree-walker (Axon `886aae53`, release, 2026-10-09) |
|---|---|---|---|
| `loop.ax`, per iteration | S1 | ≤ 300 | 1,264 |
| one-argument call via `dispatch_call` → `call_fn_in` | S1 | ≤ 800 | ≈ 1,125 |
| `part.ax`, per partition iteration | S2 | ≤ 250 | 1,383 |
| `fold.ax`, per element: builtin → closure call, body included | S4 | ≤ 350 | 916 |
| one-argument fast call | S5 | ≤ 300 | n/a |
| one-`&mut`-argument call via `call_mut` | S5 | ≤ 600 | ≈ 2,607 |
| `foldmod.ax`, per element: arr-sum's body | S7 | ≤ 200 | n/a (VM at `12713cbd`: ≈ 675) |
| `swapcall.ax`, per `swap` call, body included | S8 | ≤ 730 | n/a (VM at `12713cbd`: ≈ 2,641) |

The S1 call budget is for the general path, which every call keeps through S4 and flagged calls keep for
good. The program budgets need S4 and S5. fib(32) makes 7,049,155 calls, so 3,423,642,492 leaves about 485
instructions per call, body included: a 300-instruction fast call plus a body of about 10 ops. arr-sum
makes 50 M closure calls (10 folds over 5 M elements), so its budget leaves about 472 per element, body
(`acc + x % m`) and the builtin's loop included; the S4 `fold.ax` budget leaves about 120 of those for the
extra `%` and the variable read.

qsort is the gate's tightest program. Revision 5 split its 18.97 G with two estimated terms; at S4 the
`swap` body measured 801 beyond the call, not ≈ 150, so revision 11 re-splits it from measurements at
`12713cbd` (the term marked [INFERENCE] is a residual, not a measurement):

| Term | Count | Per unit | Total |
|---|---|---|---|
| partition iterations (`part.ax`, measured 227; S2 budget 250) | 25,092,348 | 227 | 5.70 G |
| `main` without `quicksort`'s work (measured) | — | — | 1.094 G |
| `quicksort` prologues and the rest beyond its call (residual) [INFERENCE] | 1,333,559 | ≈ 1,014 | ≈ 1.35 G |
| `quicksort`'s own `&mut` calls at the S5 budget | 1,333,559 | 600 | 0.80 G |
| `swap` calls, body included (the S8 `swap` row) | 13,670,662 | ≤ 730 | ≤ 9.98 G |

The total is 18.92 G, about 0.05 G under budget (the residual uses the same base as the S8 `swap` row,
`loop_generic.ax`, 2,638 per call at S4, so a base bias cancels between them), so the `swap` row's 730 is derived, not padded. The
slice budgets still do not by themselves prove qsort's gate; Q3 applies after S8.

sieve (S10) is gated against half the S9 VM's median instead: CPython clears multiples with one slice
assignment that runs in C (CPython 3.14.4: 1,085,960,442), which no loop compiler reaches. The S9 VM
retired 53,528,608,080 on it (run `20261010T015516Z`, Axon `5864c423`); the budget is 26,764,304,040.

From S11 fib-recursive's budget is a third of CPython's median, 1,141,214,164: at S10 (`944fe056`)
the VM retired 0.67× CPython's instructions on it (2,295,409,337 against 3,423,642,492, run
`20261008T142344Z`), and at S9 it ran 1.29× CPython's wall time at about 2.9 instructions per cycle
against CPython's 3.6, so the instruction lead has to be large enough to survive a lower IPC. The
tier must cost nothing where it is not entered: every other program's median may be at most 0.5 %
above its S10 median, which `vm_perf_gate.sh` records as a `REF` column and checks on every run
(median × 1000 > REF × 1005 fails). S10 medians (`944fe056` release, CPU 11): collatz
18,366,495,062; mandelbrot 5,039,973,411; arr-sum 4,979,381,330; qsort 10,939,169,020; sieve
24,568,573,662. fib-recursive's `REF` is its median at the S11 commit `2627dad7` (1,002,729,957,
the gate's own `--no-default-features` release build, CPU 13), which S12 and later slices must stay
within 0.5 % of; its budget stays 1,141,214,164.

### 11. Rollout & rollback

`AXON_ENGINE` defaults to `tree` through S0–S5, S7 and S8, so engine selection does not change while coverage grows.
Four changes touch the reference tree-walker, each with a CHANGELOG entry: the S0 scope-leak fix
(behaviour), and the S4 pooled closure call, S5 allocation-free `call_mut` and S7 `arr_fold` loop that
offers `fold_leaf` the remaining elements (cost only; `fold_leaf` never runs under the tree).

- The scope-leak fix is the one intended change to reference behaviour. Today an `Err` out of
  `match_pattern` or a guard (eval.rs:306, 308, 358 at `886aae53`) skips the arm's `env.pop()`. The catcher's single
  pop (`run_loop_body` or `eval_block`, eval.rs:723-764) then removes the arm's mark instead of its own,
  so one scope mark too many survives and the enclosing scope's bindings stay visible. When the error is
  caught in the same frame (`Break`/`Continue` by `run_loop_body`, `HandlerDone` by `eval_with_handler`),
  later `env.snapshot()`s see those bindings, and a `Flow::Return` out of a guard's `?` makes
  `call_fn_mut`'s reverse scan read a body `let` that shadows a `&mut` param back instead of the param
  (compilebench AX-57: `axon run` panics where the native build prints `3`). After the fix those programs
  see the outer binding. Native codegen keeps no run-time scope stack, so it never had the leak; the fix
  moves the interpreter onto codegen's behaviour. The S0 gate runs `AXON_HARNESS_STRICT=1
  scripts/parity_all.sh` under the tree engine to confirm no other case changes.
- `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` is made real in S0. Today `parity_all.sh` never reads
  the variable (only `note_harness_skip` at cli_run.rs:298 does, and gate.sh:731 advertises it), treats every
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
- Q2 (non-blocking): `call_builtin` is a `match name` (builtins.rs:802). If S6 profiles show it hot,
  resolve builtins to an index at compile time. That is a `call_builtin` refactor shared with the
  tree-walker, specified separately.
- Q3 (blocks R50.S6 only if it comes true): qsort, fib or arr-sum still misses its budget after S5 with
  every `--repros` budget met. For qsort that is possible by construction (§10: the estimated terms leave
  under 20 instructions of slack). Then the per-op cost of the S1-S3 ops is the gap. The response is a
  spec revision re-reviewed before S6 (for example an index-compare-branch superop for qsort's loop, or a
  fold-specialised closure call for arr-sum). The gate is never loosened in place. It came true at S4
  for mandelbrot and arr-sum (not on this list) and is projected for qsort and fib (§15 "Revision 11");
  revision 11 answers it with S7 and S8. It stays open for any miss after S8, with the same rule.
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
| R50.S2 aggregates, index/field reads, place writes (`AssignTo`); extract `index_in_place`, `index_value`, `field_in_place`, `finish_record`, `place_index`, `write_place` | R50.S1 | `cli_run vm_aggregate_` (incl. `vm_aggregate_part_no_tree_nodes`) + `vm_parity.sh` + `vm_perf_gate.sh --repros` (S1 and `part.ax` rows) + `vm_wasm_depth.sh` | done (`2d10cf99`) |
| R50.S3 match, patterns, enums, methods; extract `chan_method`, `impl_method` | R50.S1 | `cli_run vm_match_` (incl. `vm_match_option_no_tree_nodes`) + `vm_parity.sh` + `vm_wasm_depth.sh` | done (`765bb811`) |
| R50.S4 lambdas; extract `make_closure`; `ClosureCode.compiled`; allocation-free builtin → closure call on the lent path (`Rc::strong_count(cv) == private_refs`, interp.rs:4640), the one `fold.ax` and arr-sum take: `arr_fold`/`arr_map`/... take argument buffers from `arg_bufs`, `call_closure_owned_by` drains its arguments into the params and hands the buffer to `recycle_args` (today `zip(args)` consumes it, interp.rs:4067 at `886aae53`), and its `Env` gets a pooled `marks` Vec (today `vec![acc, x.clone()]` per element, builtins.rs:1847 at `886aae53`, and `Env::from_snapshot` starts with an empty `marks`, so `env.push()` allocates, interp.rs:796-802; 4058-4066 at `886aae53`). The copied path (a closure with other references, e.g. `let f = \|..\| ..; arr_fold(xs, 0, f)`) keeps its per-call `Vec::with_capacity` (interp.rs:4061-4064 at `886aae53`) | R50.S1 | `cli_run vm_closure_` (incl. `vm_closure_fold_no_tree_nodes`) + `vm_parity.sh` + `vm_perf_gate.sh --repros` (S1 and `fold.ax` rows) + `vm_wasm_depth.sh` | done (`12713cbd`) |
| R50.S5 fast calls; allocation-free `call_mut` and `CallMut` op | R50.S2, R50.S3, R50.S4 | `cli_run vm_fastcall_` (incl. `vm_fastcall_mutcall_no_tree_nodes`, `vm_fastcall_slow_<flag>`) + `vm_parity.sh` + `vm_perf_gate.sh --repros S1,part,fold,S5` + `vm_wasm_depth.sh` + `AXON_ENGINE=vm AXON_HARNESS_STRICT=1 scripts/parity_all.sh` | `3eb79f1a` |
| R50.S7 pure scalar regions: `Pure`, `PureLoop`, `fold_leaf`; `pure`/`fold-leaf` trace lines; `loop_generic.ax`, `foldmod.ax`, `--programs` | R50.S4 | `cli_run vm_pure_` (incl. `vm_pure_mandel_loop`, `vm_pure_fold_leaf`) + `vm_parity.sh` + `vm_perf_gate.sh --programs mandelbrot,arr-sum,collatz` + `vm_perf_gate.sh --repros S1,part,fold,foldmod` + `vm_wasm_depth.sh` | `850036e3` |
| R50.S8 qsort and fib superops (§4 S8); `swapcall.ax` | R50.S5, R50.S7 | `cli_run vm_superop_` + `vm_parity.sh` + `vm_perf_gate.sh` (all five) + `vm_perf_gate.sh --repros` (every row; `swap` is the red check) + `vm_wasm_depth.sh` + `AXON_ENGINE=vm AXON_HARNESS_STRICT=1 scripts/parity_all.sh` | `c048113e` |
| R50.S6 default flip, docs | R50.S5, R50.S7, R50.S8; blocked-by Q3 or Q4 only if it comes true (neither did) | whole suite under both engines + `vm_parity.sh` (S8 list) + `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` under each engine (VM default) + `vm_perf_gate.sh` + `reference_gate.sh` + `vm_wasm_depth.sh --require-default-stack` (default-stack VM depth ≥ tree on all four chains) | `a9176ce5` |
| R50.S9 deferred compile (§4 S9); `AXON_VM_EAGER`; `vm: defer` trace line | R50.S6, R50.S8 | `cli_run vm_defer_` + `cli_run vm_` (eager and default legs) + whole suite with `AXON_ENGINE` unset, `tree` and `vm` + `AXON_VM_EAGER=1` + `vm_parity.sh` (S9 list; eager and default runs) + `vm_perf_gate.sh` (all five) + `--repros` (every row) + `--compile` + `reference_gate.sh` + `vm_wasm_depth.sh --require-default-stack` + `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` under the same three | `db2d2eee` |
| R50.S10 array elements in pure loops (§4 S10); `PureFor`; `sieve.ax` and its `--programs` row; never-taken `if` in the repro bases made impure | R50.S9 | `cli_run vm_index_` (incl. `vm_index_sieve_loops`) + `vm_perf_gate.sh` (all six) + `--repros` (every row) + `vm_parity.sh` + `vm_wasm_depth.sh --require-default-stack` + whole suite under the S9 modes | code `944fe056`, which fails `vm_wasm_depth.sh` (`wasm_stack_budget.py`: `pure_stmts`/`pure_if` recursion not in `ALLOW`, `COMPILE_STMT` 16 bytes short in release); fixed with S11 |
| R50.S11 pure-`i64` function tier (§4 S11); `vm: purefn` trace line; fib-recursive budget a third of CPython's | R50.S10 | `cli_run vm_purefn_` (incl. `vm_purefn_fib_in_registers`) + `purefn_build_validates` + `vm_perf_gate.sh` (all six; `REF` check: others at most 0.5 % above S10) + `--repros` (every row) + `vm_parity.sh` + `vm_wasm_depth.sh --require-default-stack` (`plain` on the tier, `plain_generic` off it) + whole suite under the S9 modes | code `2627dad7`; review fixes `faf25a22`, `e5f754ab` |
| R50.S12 wasm32 traps (§4 S12): bounded drop of every `Value` container, front-end nesting limit 224 (interpolation slots included), no payload copy for `host_await_val`; `DictMap`/`Elems`/`VBox`/`Queue`/`Held`; nest probes; four `wasm_stack_budget.py` checks | R50.S10 | `cli_run wasm_drops_deep_values_ax59`, `wasm_deep_source_gives_e0000_ax60`, `native_nesting_limit_unchanged_ax60`, `native_guard_borrow_e0606`, `native_guard_w0002`, `native_guard_assign_in_place`, `native_guard_pure_e1207`, `native_guard_tier_e0910`, `native_guard_write_unaliases`, `wasm_host_await_val_no_copy_ax61` + `wasm_nesting_parity.sh` (debug and release; out-of-root refusals `tdef_*`, `attr_*`, `handler_*`, `*_chain`, `*_pred`) + `vm_wasm_depth.sh --require-default-stack` (debug and release; no probe exits 134) + `wasm_stack_budget.py` + `vm_perf_gate.sh` (every `REF` within 0.5 %, fib-recursive's from `2627dad7`) + whole suite under the S9 modes | code `a9c89e5e`, merged `e0f29182`; review fixes `91b13427`, `c33f521b`, `e5f754ab`, `fcd18f2b`, `2fdf7cc1`, `a875d2b8`, `8e4bd760`, `6ae2c76f` |

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
| S2-S4 parity (merged) | `VM_PARITY_SLICE=S4 scripts/vm_parity.sh` | `290 files, 1894 bodies, 34183 lowered ops, 120 tree ops, 0 differ` | `12713cbd` @ 2026-10-09 | PASS |
| S2/S4 repros: part ≤ 250, fold ≤ 350; S1 rows hold | `scripts/vm_perf_gate.sh --repros S1,part,fold` | exit 0; loop 229.2, call 759.1, part 227.1, fold 330.0 | `12713cbd` @ 2026-10-09 | PASS |
| S2-S4 wasm depth | `scripts/vm_wasm_depth.sh` | exit 0; default-stack `mut` chain VM below tree (debug 478 vs 533), reported only | `12713cbd` @ 2026-10-09 | PASS |
| S2-S4 suite: both feature sets × both engines; strict parity under tree | `cargo test -p axon-core`; `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` | all green (after the cache-test scratch-dir race fix); 53 passed, 2 allowed skips of 55 | `12713cbd` @ 2026-10-09 | PASS |
| S5 parity (merged on S0-S4) | `VM_PARITY_SLICE=S5 scripts/vm_parity.sh` | `290 files, 1894 bodies, 32243 lowered ops, 111 tree ops, 0 differ` | `3eb79f1a` @ 2026-10-09 | PASS |
| S5 repros: fastcall ≤ 300, mutcall ≤ 600; S1/S2/S4 rows hold | `scripts/vm_perf_gate.sh --repros S1,part,fold,S5` | exit 0; loop 233.2, call 257.1, part 231.1, fold 341.0, fastcall 257.1, mutcall 562.2 (`fd8816b7`) | `fd8816b7` @ 2026-10-09 | PASS |
| S5 wasm depth | `scripts/vm_wasm_depth.sh` | exit 0; default-stack VM ≥ tree on every chain (`fd8816b7`) | `fd8816b7` @ 2026-10-09 | PASS |
| S5 suite: both feature sets × both engines; strict parity under each engine | `cargo test -p axon-core`; `AXON_ENGINE=<e> AXON_HARNESS_STRICT=1 scripts/parity_all.sh` | all green; 53 passed, 2 allowed skips of 55, under tree and under vm | `3eb79f1a` @ 2026-10-09 | PASS |
| S5 programs (not gated until S6; S7/S8 targets) | `perf stat -e instructions:u` under `AXON_ENGINE=vm` | fib 4.243 G, collatz 20.602 G, mandelbrot 24.195 G, arr-sum 34.182 G, qsort 25.796 G (`fd8816b7`) | `fd8816b7` @ 2026-10-09 | over on 4 of 5 |
| S7 parity (merged on S0-S5) | `VM_PARITY_SLICE=S7 scripts/vm_parity.sh` | `292 files, 1897 bodies, 32392 lowered ops, 111 tree ops, 0 differ` (`0899ae24`) | `0899ae24` @ 2026-10-09 | PASS |
| S7 programs and repros | `scripts/vm_perf_gate.sh --programs mandelbrot,arr-sum,collatz`; `--repros S1,part,fold,foldmod` | exit 0; mandelbrot 5,057,593,503, arr-sum 4,879,377,354, collatz 20,103,241,433; loop 92.2, call 261.1, part 235.1, fold 58.0, foldmod 95.0 | `0899ae24` @ 2026-10-09 | PASS |
| S7 wasm depth | `scripts/vm_wasm_depth.sh` | exit 0; default-stack VM ≥ tree on every chain | `0899ae24` @ 2026-10-09 | PASS |
| S7 suite: both feature sets × both engines; strict parity under each engine | `cargo test -p axon-core`; `AXON_ENGINE=<e> AXON_HARNESS_STRICT=1 scripts/parity_all.sh` | all green; 53 passed, 2 allowed skips of 55, under tree and under vm | `850036e3` @ 2026-10-09 | PASS |
| S8 parity (merged on S0-S7) | `VM_PARITY_SLICE=S8 scripts/vm_parity.sh` | `293 files, 1899 bodies, 32375 lowered ops, 111 tree ops, 0 differ` | `c048113e` @ 2026-10-09 | PASS |
| S6 programs and repros (VM default): all five at or below CPython; every repro row | `scripts/vm_perf_gate.sh`; `--repros` | exit 0 both; fib-recursive 2,298,929,095, collatz 18,544,539,278, mandelbrot 5,069,612,114, arr-sum 4,879,380,031, qsort 11,434,122,532; loop 92.2, call 241.1, part 218.1, fold 58.0, fastcall 241.1, mutcall 275.2, foldmod 95.0, swap 259.3 | `a9176ce5` @ 2026-10-09 | PASS |
| S6 wasm depth (default stack required) | `scripts/vm_wasm_depth.sh --require-default-stack` | exit 0; release default stack tree/vm: plain 272/678, closure 170/277, mut 387/581, with 214/221 | `a9176ce5` @ 2026-10-09 | PASS |
| S8 suite: both feature sets × both engines; strict parity under each engine | `cargo test -p axon-core`; `AXON_ENGINE=<e> AXON_HARNESS_STRICT=1 scripts/parity_all.sh` | all green; 53 passed, 2 allowed skips of 55, under tree and under vm | `c048113e` @ 2026-10-09 | PASS |
| S6 gate (VM default) | `cargo test -p axon-core` (both feature sets; `AXON_ENGINE` unset, `tree`, `vm`); `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` (unset, `tree`); `vm_parity.sh`; `vm_perf_gate.sh`; `reference_gate.sh`; `vm_wasm_depth.sh --require-default-stack`; clippy both feature sets; `cargo fmt --check` | all exit 0; parity 53 passed, 2 allowed skips of 55 under each engine; 293 files, 0 differ; reference in sync (57 env vars) | `a9176ce5` @ 2026-10-09 | PASS |

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

Revision 11 (2026-10-09): §12 Q3 came true at S4, answered by adding S7 and S8. Measured on
`12713cbd` (S0-S4 merged, release, `--no-default-features`, `AXON_ENGINE=vm`, `perf stat -e
instructions:u -r 3`, core 6), and on prototypes of the S7/S8 ops built on that commit, every run
printing the golden output:

| Finding | Resolution |
|---|---|
| mandelbrot 23.86 G against 15.02 G; no slice targeted it. ≈ 2,110 per inner iteration, ≈ 1,307 in nine generic float `Bin` ops (callgrind, 60×40 copy) | S7 `Pure` and `PureLoop` (§4 S7). Prototype: `Pure` 18.67 G, typed 17.29 G, registers 15.35 G / 14.72 G, `PureLoop` 5.03 G |
| arr-sum 33.78 G against 23.62 G; 675 per element because the two-op body misses S4's leaf path | S7 `fold_leaf`; `foldmod` row ≤ 200. Prototype: 5.38 G |
| qsort 46.69 G against 18.97 G; `swap` body 801 beyond the call, not revision 5's ≈ 150; projected ≈ 28.1 G after S5 at 600 per `&mut` call | S8 index ops, `for` scope reuse, sized frames; §10 re-split from measurements; `swap` row ≤ 730. Prototype index ops: 40.82 G before S5 |
| fib-recursive 5.46 G against 3.42 G; body ops ≈ 290 per call, so S5's 300 fast call cannot fit 485 per call | S8 fib-shaped ops, gated by the program budget. Prototype `local ± literal` and stack add: 5.28 G before S5 |
| collatz 20.12 G, within 26.06 G | Unchanged; in the S7 `--programs` gate because S7's `Pure` op takes its `if` condition `x % 2 == 0` (its `while` body holds an `if`, so no `PureLoop`). Prototype with `Pure`/`PureLoop` 20.02 G; with the S8 ops too, 18.57 G |
| `loop.ax` becomes a `PureLoop` (229 → 92 per iteration), so rows that subtract it would charge the call for the loop's lost speed | `call`, `fastcall`, `mutcall`, `swap` subtract `loop_generic.ax` from S7, and their programs carry the same never-taken `if`, so it cancels (§4 S7) |
| Tenth review (2026-10-09, `reviewer`, verdict "incorrect": 0 blockers, 1 must-fix, 1 gap, 1 nit), answered in revision 12: `loop_generic.ax`'s `if` measured 50 (47 stacked), crediting each `swap` call ≈ 0.68 G in total; `Pure` silent on `Bool` operands outside `&&`/`||` and non-`Bool` branch values; collatz row misattributed | The `if` is added to `call1.ax`, `mutcall.ax` and `swapcall.ax` so it cancels; decline rules and `vm_pure_bool_ops` added; collatz row corrected |
| Eleventh review (2026-10-09, `reviewer`, verdict "incorrect": 0 blockers, 1 must-fix, 3 nits), answered in revision 13: `vm_pure_bool_ops`/`vm_pure_short_circuit` did not put the expression in a sink, so they could not fail; swap `if` cancels within ≈ 10; qsort split re-measured 18.922 G; value.rs:719-720 is `eval_binop_vals` | Tests use `let`/`if` sinks and assert the `vm: pure` line; cancellation stated as measured; split total 18.92 G with residual ≈ 1,014; citation renamed |
| Twelfth review (2026-10-09, `reviewer`, verdict "incorrect" pending four text fixes, then ready): `vm_pure_fold_decline` cannot assert a `vm: pure` line; `mutcall` `if` is +10 and `swapcall` bimodal; "19.06 G against `loop.ax`" contradicts the 2,641 swap figure; `PureLoop` tests should pin `1 loops` and the `Assign` sink is non-AX-31 only | Revision 14: fold test asserts `vm: fold-leaf`; cancellation restated as measured; base-bias sentence replaces the 19.06 G claim; `1 loops` and the AX-31 qualifier added |
| S5's gate named "all rows", which would include the S7 and S8 rows | S5 gate is `--repros S1,part,fold,S5` |

Revision 15 (2026-10-09): compilebench AX-58. At `2a83e6ab` (S0-S8, VM default) `big-compile`, 1,002
fns each entered once, retired 1.076× the tree-walker's instructions at `886aae53`: every body paid a
compile it never reused. Measured on the S9 branch (release, `--no-default-features`, `perf stat -e
instructions:u`, core 6). With the hot test as a body walk on first entry (`-r 1`): `big-compile` eager
841.0 M, deferred 775.4 M, tree 769.0 M; disabling the walk alone gave 770.0 M, so the walk cost
≈ 5.4 k per fn. With hotness computed by the resolver (`-r 3`, same binary, `AXON_ENGINE=tree` /
`vm`): `big-compile` 768.78 M / 768.33 M; hello 4.04 M / 4.04 M; fib-recursive 9.89 G / 2.30 G (its
`main` defers, `fib` compiles on its second entry); collatz 59.72 G / 18.58 G; mandelbrot 41.11 G /
5.07 G; arr-sum 34.73 G / 4.88 G (once `arr_fold` re-offers `fold_leaf` after element 2); qsort
75.52 G / 11.40 G. All five stay under their CPython budgets (§10).

| Finding | Resolution |
|---|---|
| A body entered once pays a compile it never reuses (`big-compile` 1.076× tree) | S9: compile on the second entry, unless hot on entry (a `while`, `while let`, a `for` over a non-literal or >32-iteration range, or nested loops) |
| Compiling only on the second entry would leave once-run `main` loops on the tree (with `main` forced onto the tree under `AXON_ENGINE=vm` via `AXON_DUMP_BINDINGS`: collatz 59.48 G, mandelbrot 41.20 G, against 18.58 G / 5.07 G compiled) | Hot-on-entry bodies keep compiling on their first entry |
| A body walk on first entry to test hotness cost ≈ 5.4 k per fn, leaving `big-compile` at 1.008× tree | The resolver computes hotness during the walk it already makes (sym.rs:972-1010); `big-compile` 768.33 M vs tree 768.78 M |
| `arr_fold` offered `fold_leaf` once, after element 1, before a deferred lambda body is compiled | Offered again after element 2 (builtins.rs:1856-1865) |
| S8's `CallMut` cached "no moves form" when the callee was not yet compiled, keeping qsort's `swap` on the slow path (12.02 G) | The cache fills only once the callee is compiled (vm/mod.rs:1733-1742): qsort 11.40 G |
| `vm_` cli tests pin compile-on-first-run trace lines | `AXON_VM_EAGER=1` restores it for those legs; default-engine legs added to `vm_same_both_engines` and `vm_fastcall_case`; `vm_parity.sh` diffs both |

Revision 16 (2026-10-09) answers the thirteenth review (`reviewer`, verdict "incorrect": 2 must-fix,
4 smaller; no observable divergence or cost hole found in the S9 code) and records compilebench AX-56.

| Finding | Resolution |
|---|---|
| [must-fix] "`big-compile` vm ≤ tree" sits inside run-to-run spread (default minus tree measured from +1.14 M to −0.83 M) and is checked by hand | `vm_perf_gate.sh --compile`: fixture copy of `big-compile`, median of five, deferred ≤ 1.01 × tree and ≤ 0.95 × eager (red check), in §4 S9 Gate, §9 and the §13 row. Measured on `5ea3398a`'s release binary: deferred 767,914,214, tree 768,523,096 (0.9992×), eager 841,046,867 (0.9130×) |
| [must-fix] since S6 "unset" is `vm`, so the suite and `parity_all.sh` legs no longer compile once-run bodies | The `vm` leg runs with `AXON_VM_EAGER=1` in the whole suite and in `parity_all.sh` (§4 S9 Gate, §13 row) |
| collatz 69 G / mandelbrot 49 G do not reproduce | Replaced with measured figures, configuration named (Revision 15 table) |
| §3 trace example shows pre-S9 output | Shows the deferred output and the `AXON_VM_EAGER=1` output, measured |
| S9's unobservability argument omits the fast-frame first entry | §4 S9 names `call_fast`/`call_fast_arg`/`call_mut_op` → `fast_body` and why `call_mut_op`'s positional read-back stays exact (eval.rs:721-735) |
| drifted citations | vm/mod.rs:1320, 1733-1742; interp.rs:3269, 3334-3338 |
| compilebench AX-56: on wasm32 under wasmtime's default 512 KiB native stack the tree-walker trapped at depth 136-318, below the 450 guard | Guard lowered to 128 on wasm32 (interp.rs `RECURSION_LIMIT`); `vm_wasm_depth.sh` requires 1.3 × the guard in the linear stack and 1.2 × the guard under the default stack (shallowest at `5ea3398a`: 162, tree closure chain, debug) (§4 Recursion, §7 I-4, §8) |

Revision 17 (2026-10-09) answers the fourteenth review (`reviewer`, verdict "incorrect": 2 must-fix,
2 smaller; every Revision 16 row and figure reproduced).

| Finding | Resolution |
|---|---|
| [must-fix] AX-56 claims a graceful panic, but a recursive call nested in expressions (`w(127)` nine parentheses deep in a `with` body) still traps on wasm32 under the default stack, on both engines and profiles | On wasm32 each guarded interpreter frame charges a stack-unit budget of `max_depth × 400` (`eval` 40, `run` 100, closure 41, builtin 51, `call_fn_in` 80), fitted on 15 shapes; exceeding it gives the recursion-limit panic, exit 101. The review's program now exits 101 in both profiles under both engines. `vm_wasm_depth.sh` runs ten committed nesting probes (`tests/fixtures/vm_depth/nest/`), each of which must panic, not trap (§4 Recursion) |
| [must-fix] the eager `parity_all.sh` leg never compiles once-run bodies on a wasm32 leg | `wasm_parity.sh`, `wasm_fs_parity.sh` and `wasm_host_await_parity.sh` forward `AXON_VM_EAGER`; §4 S9 Gate states that the `wasm32-unknown-unknown` leg covers deferred mode only |
| §9 and §8 gate the suite and `parity_all.sh` without the eager leg | Third run `AXON_ENGINE=vm AXON_VM_EAGER=1` added to §8 Whole suite and both §9 bullets |
| interp.rs:3329 shifted; `.cargo/config.toml` and AXON-COMPLETENESS still name 450 | interp.rs:3321, 3386-3390, 3388; `.cargo/config.toml` names 128 and the stack-unit budget; the `AXON_MAX_DEPTH` control row records AX-56 |

Revision 18 (2026-10-09) answers the fifteenth review (`reviewer`, verdict "incorrect": 2 must-fix,
2 smaller; Revision 17 rows 2-4 held).

| Finding | Resolution |
|---|---|
| [must-fix] the fitted stack-unit weights still let the tree-walker trap with closures or callbacks nested several levels per recursion step (five lambdas at 60, six `arr_fold` callbacks at 45, ten of either) | The weights are gone: each of the 25 interpreter functions seen in a recursion cycle charges its measured Cranelift x86-64 frame, in bytes, per profile, against `max_depth × 3,584` (448 KiB at the default guard); `vm_wasm_depth.sh` re-measures the frames from the build under test and fails when one outgrew its constant. Probes `lambda10`, `fold10`, `mixed6`, `handler_arm` and `refined` added; all 16 probes exit 101 under both engines and profiles (§4 Recursion) |
| [must-fix] the budget makes the VM panic on `&mut` recursion inside three `with` bodies at depths 111-126 that the tree completes | Byte charges replace the `run` 100 weight; probe `mut_with3` added; the script bisects every probe's deepest exit-0 depth under the guard and with `--require-default-stack` requires VM ≥ tree on every probe. Measured 2026-10-09: `mut_with3` reaches 126 on both engines and profiles, and the VM ≥ the tree on all 16 probes |
| `vm_wasm_depth.sh --help` stops inside the lengthened header | `--help` prints through the line before `set -uo pipefail` |
| the 162 shallowest-depth figure is stale and unpinned in CHANGELOG and AXON-COMPLETENESS | Remeasured with the byte budget and pinned to its date: 154 (tree closure chain, debug), equal to the 1.2 × guard requirement, in CHANGELOG, AXON-COMPLETENESS, R7-targets.md, REQUIREMENTS R7, interp.rs and §4 |

Revision 19 (2026-10-09) answers the sixteenth review (`reviewer`, verdict "incorrect": 2 blockers,
1 must-fix; it confirmed the frame check, the four chains, the 16 probes' panics, `--help`, the exit
codes, 154 and 448 KiB).

| Finding | Resolution |
|---|---|
| [blocker] `run_loop_body` is in the eval cycle but uncosted: ten nested `for` loops trap the tree at 103 (debug) / 84 (release), and loops inside a `with` body trap the VM | The costed set is no longer the functions seen in backtraces. `scripts/wasm_stack_budget.py` takes the strongly connected component holding `Interp::eval` in each build's call graph; 26 more functions got `nest_guard!` (51 in all), including `run_loop_body`, so the unguarded rest has no cycle, and each constant covers its frame plus its deepest unguarded tail (§4 Recursion). Probes `loop_for`, `loop_while_let` and `loop_with` added; all panic at the guard on both engines and profiles |
| [blocker] `call_fast` and `exec_catching` stay live per VM level uncosted: a two-argument call through three lambdas after a `break` out of a `with` body traps the default VM (release) at 111 | Both are guarded (`CALL_FAST`, `EXEC_CATCHING`), and the hand-kept list is replaced by the mechanical component check, which `vm_wasm_depth.sh` runs on both builds before probing. It also adds every `call_indirect` edge (to each table function of the call's type, the entry `main` excepted) and fails if that widens the component; it does not, in either profile. Probe `break_fast` added (the review's program): panics at the guard; reach tree 75 / VM 126 (debug), 73 / 96 (release) |
| [must-fix] a chain of distinct fns each called once makes the lazy VM panic below the tree's depth (94 vs 97 debug, 100 vs 105 release) | A deferred first entry now runs `run_body` → `eval` (the tree-walker's frames); `run_body_vm` with its charge is entered only for a compiled body or one compiled on this entry. Probe `distinct` added (160 distinct fns, nine parentheses deep, the last calling the first): reach 126 on both engines in both profiles |

Revision 20 (2026-10-09) answers the seventeenth review (`reviewer`, verdict "incorrect": 2 must-fix,
1 smaller; it confirmed every revision-19 resolution, the 80/58 component sizes, 51 guards, the
indirect-edge check and the reach figures, and found no trap in 10 more interpreter-recursion shapes).

| Finding | Resolution |
|---|---|
| [must-fix] leaf work above the deepest guard is unbounded: a depth-120 recursion that drops a 600-node enum list traps the debug VM (Rust drop glue recursing per list node) | Measured alone: dropping a 3,000-node list traps debug wasm32 and 8,000 traps release, with no call recursion; native drops 1,000,000. Filed as compilebench AX-59 (open; it predates AX-56). The guarantee is narrowed to recursion through the interpreter in §4 Recursion, the CHANGELOG, R7-targets, REQUIREMENTS R7, AXON-COMPLETENESS, `wasm_stack_budget.py` and the `NEST_PER_DEPTH` doc, each naming data-depth recursion (drop, compare, format) as outside the budget |
| [must-fix] the byte budget makes `AXON_VM_EAGER` change the exit code at the budget's edge (`break_fast` release: deferred panics at 97, eager completes it) | §3's `AXON_VM_EAGER` row and §4 Recursion state the exception: on wasm32 at the budget's edge the panic depth can differ by a level between engines and between eager and deferred compile; the gate compares the deferred VM with the tree only |
| code citations in §1-§14 drifted with this patch and earlier slices | 61 citations re-derived against the working tree by content and spot-checked; 10 that describe code a slice since replaced (`call_fn_mut`, `eval_int`, the S0 scope leak, the pre-S4 closure call) are pinned "at `886aae53`", where they were measured |
| (found by the suite, not the review) revision 19's `run_body` split left `fast_body`'s uncompiled arm calling `run_body_vm` directly, which now compiles unconditionally, so a fast call's first entry skipped S9's deferral (`vm_defer_mutcall_after_tree_entry` failed under every engine) | That arm calls `run_body` (vm/mod.rs:2797), which applies the first-entry rule; `run_body_vm` has `run_body` as its only caller |

Revision 21 (2026-10-09) answers the eighteenth review (`reviewer`, verdict "incorrect": 1 blocker,
3 must-fix, 1 smaller; it re-checked 86 citations, all 12 pinned ones exact). Measurements are on the
revision-21 `wasm32-wasip1` builds under wasmtime's default stack, guard 128.

| Finding | Resolution |
|---|---|
| [blocker] the bytecode compiler's recursion (`Compiler::expr`/`operands`/`binop`, one level per level of source nesting) is outside the budget: a 240-deep expression compiled on a deferred second entry at depth 33-35 traps the default VM (`cdm240.ax`), and the checker passed 59 other recursive components without looking at them | `Compiler::expr`/`stmt` charge `COMPILE_EXPR`/`COMPILE_STMT` through `Compiler::nest`; when the budget is spent `compile` returns `None` and the body runs on the tree that time. `wasm_stack_budget.py` checks every recursive component reachable from `eval` that holds a guarded function (the compiler's: 34 functions debug, 9 release, 2 guarded) and fails on any other recursion that its `ALLOW` list does not classify (a fixed depth: `PURE_DEPTH` = 16 now caps pure trees, numeric helpers recurse once; std; the panic path; source depth; value depth). Source depth is bounded by the parser, which traps first at 273 levels in release: filed as compilebench AX-60. Probe `compile_deep` added. `cdm240.ax`: every engine panics at 127, deepest exit-0 tree 19, VM 40 / 29, eager 50 / 35 (debug / release) |
| [must-fix] `RUN_BODY`'s release constant is too small after the `fast_body` edit (`run_body 80 <= 0`) | Constants re-fit to the checker's fixed point on both profiles; `run_body` is now a dispatcher inlined into `call_fn_in`, with `run_body_tree` and `run_body_vm` (`RUN_BODY_VM` 144 / 208) each guarded. The checker passes both profiles: eval component 81 / 60 functions, 60 / 49 guarded |
| [must-fix] the default VM panics at depths the tree completes when a body compiles to one `Tree` op (`wb3.ax`, a body that is one `with`: release tree 120, VM 108) | A lone-`Tree` body (`Body::lone_tree`) runs as `run_body_vm` → `eval`, and `RUN_BODY_TREE` is defined as `RUN_BODY_VM`, so a fn level charges the same bytes under both engines (the checker accepts an alias only when its bytes cover both functions). Cost: on wasm32 the tree-walker's fn level now charges the VM's 208 bytes in release, so the tree panics earlier than before on deep fn recursion; every chain still completes ≥ 1.2 × the guard under the default stack (shallowest: closure, tree 154 debug). `wb3.ax`: tree 111 / 114, VM 111 / 114. Probe `with_body` added; `mut_with3` 126 / 126 |
| [must-fix] "differs by a level" between engines and between eager and deferred compile is wrong by about 95 levels | §3's `AXON_VM_EAGER` row, §4 Recursion and the §4 behaviour-table wasm32 row now say the budget's panic depth can differ by many levels, with measurements (`expr` tree 28 / VM 126 debug; a 130-fn chain with 40 parentheses per call, deferred 28, eager 126 debug); the only gated relation is deferred VM ≥ tree |
| [nit] stale citations: line 99 `call_fn_mut`, `harness_skipped` at cli_run.rs:298, vm/mod.rs `1346`, `2795-2798`, `2916`, `1764-1771` | Pinned `call_fn_mut` at `886aae53` (interp.rs:3375-3401); cli_run.rs:298 is in `note_harness_skip`; vm/mod.rs citations re-derived after this revision's code edits (1319, 1384; 2836-2839; 2957; 1802-1811), and the other §1-§14 citations into the files it changed re-checked |

Revision 22 (2026-10-09) answers the nineteenth review (`reviewer`, verdict "incorrect": 1 blocker,
1 must-fix, 1 smaller). It confirmed the revision-21 measurements (shallowest default-stack chain 154,
`with_body` 111 / 114, `mut_with3` 126 / 126, the S9 citations).

| Finding | Resolution |
|---|---|
| [blocker] uncharged source-depth walks still trap wasm32 at depth under both engines: a left-associative chain is built by the parser in a loop, so its depth is not bounded at 273, and `mentions_var` (via `assign_in_place`) and the compiler's `binds` walk it whole at the bottom of the recursion | `ast::walk_expr` is iterative (its own heap stack, same pre-order), so `mentions_var`, `binds`/`expr_binds` and every other walk through it are flat. A handler frame (`HandlerFrame`, `HandlerArmRt`, `ResumeCtx`) now holds a `SrcRef` into the source instead of deep-cloning the arms and the `with` body on every entry; each frame lives only inside the `eval_with_handler` call that borrows that source (interp.rs `SrcRef`). Before the change a `with` per level whose body holds an 800-term chain trapped debug at depth 88 under both engines; it now panics at the guard. The front-end bound is the checker, which traps on a chain of 1,301 terms (debug) / 832 (release) before the run (`CheckCtx::check_expr`, `concat_*`): added to compilebench AX-60. The `host_await_val` closure clone (`SendValue::from_value`) of a 1,250-term chain at depth 126 (debug) panics, not traps. §4 Recursion, the CHANGELOG, R7, REQUIREMENTS, AXON-COMPLETENESS, interp.rs `NEST_PER_DEPTH` and the checker's `ALLOW` comment state the front-end bound; `walk_expr` left `ALLOW`. Probes `assign_chain`, `compile_chain`, `with_chain` (700-term chains at the bottom of the recursion) added; all panic at the guard under both engines and profiles |
| [must-fix] an abandoned compile makes the default VM panic below the tree (`gen_fail.py`, nine parentheses and a 600 / 720-term never-taken chain: tree 81 / VM 79 debug, tree 83 / VM 80 release) | The compile runs in `Interp::compile_body` (guarded by `COMPILE_BODY`), which returns before the body runs, so a body retried on every entry keeps no compile frame live under the recursion. Same repro now: tree 81 / VM 81 / eager 81 (debug), 83 / 83 / 84 (release). Probe `fail_compile` added: 76 / 76 debug, 79 / 79 release |
| [nit] the checker counts a function as guarded by name only, so a deleted or moved guard goes unseen | `wasm_stack_budget.py` scans the sources for `nest_guard!(self, X)` and `nest::<{ nest_cost::X }>` sites and fails unless every non-alias constant is charged by exactly one site, in the function named after it (54 sites, one per constant). The constants are again at the checker's fixed point after this revision's frame changes (`RUN_HANDLER_ARM` 192 debug, `CALL_BUILTIN` 704 release, `RUN_BODY_VM` 160 debug) |

`scripts/vm_wasm_depth.sh --require-default-stack` passes on the revision-22 builds: the shallowest
default-stack chain is closure, tree 154 (debug); every chain and the 27 probes panic rather than
trap, with VM ≥ tree. `cli_run` passes under both feature sets (787 tests), as do
`handler_resume_parity.sh` and `suspend_resume_parity.sh`.

Revision 23 (2026-10-09) answers the twentieth review (`reviewer`, verdict "incorrect": 1 blocker,
1 must-fix).

| Finding | Resolution |
|---|---|
| [blocker] source-depth `Expr::clone` still traps wasm32 below the guard: `SendValue::from_value_at` copies a closure body (`host_await_val`), and a 700-term chain the checker accepts, at the bottom of a recursion with a `with` and nine parentheses per level, traps at depth 67-77 | Reproduced on the revision-22 builds: traps at 67 (debug tree), 71 (debug VM), 74 (release tree), 77 (release VM), backtrace `<ast::Expr as Clone>::clone` repeated; native exits 0. The claim that the front end bounds source depth inside the margin was false and is withdrawn: §4, the checker's `ALLOW` comment, the `interp.rs` budget doc, the CHANGELOG, R7, REQUIREMENTS and AXON-COMPLETENESS now say source-depth recursion is outside the budget like value depth, filed as compilebench AX-61 (open) with this repro. It traps under both engines alike, so it is not an engine divergence |
| [must-fix] the default VM panics below the tree when a body is a `let` plus one `with` (release tree 79, VM 76): the body is not `lone_tree`, so each level keeps the op loop `run` live on top of `run_body_vm` | `RUN_BODY_TREE = RUN_BODY_VM + RUN`: the tree's fn level charges a VM level's fn body plus its op loop; the checker accepts a sum alias and checks it against its own function's frame and tail. Probe `let_with` (the review's `s1`) added; `--require-default-stack` gates it: debug tree 70 / VM 74, release 72 / 76 |

`scripts/vm_wasm_depth.sh --require-default-stack` passes on the revision-23 builds, with the
constants at the checker's fixed point on both profiles: the shallowest default-stack chain is
closure, tree 154 (debug); every chain and the 28 probes panic rather than trap, with VM ≥ tree.

Revision 24 (2026-10-09) answers the twenty-first review (`reviewer`, verdict "incorrect": 1 must-fix,
2 smaller).

| Finding | Resolution |
|---|---|
| [must-fix] a lambda level that catches a `break` out of a `with` and then recurses keeps `exec_catching` and a second `run` live, so the deferred VM panics below the tree (release, six chained lambdas: tree 22 / VM 20) | Fixed in the engine, not by charging the tree more: `exec_catching` is replaced by `catch_flow` (vm/mod.rs), which only sets the scopes, stack height and `pc` of the catching loop and returns; `exec` and `exec_on` run the body on from there in their own frame, so a caught flow keeps no second frame live. `EXEC_CATCHING` and its guard are gone (53 guarded functions, 51 in the interpreter). Probe `lambda_break` (the six-lambda shape) added: before the fix release tree 20 / VM 18; after it debug 20 / 21, release 20 / 20. The review's other shapes on the revision-24 builds (tree / deferred VM / eager): one lambda release 75 / 78 / 79, three lambdas 36 / 36 / 37, two lambdas with three parentheses 48 / 50 / 50 |
| [nit] revision 23's tree charge has an unrecorded cost: under the default depth limit the budget stops the tree's closure chain below the guard | Measured on the revision-24 builds, `AXON_MAX_DEPTH` unset: tree closure chain 109 (debug) / 99 (release), release `with` 124, every other chain and every VM chain 126. §4 now says the budget can stop the tree before the guard, and that 156 is the trap depth with the limit raised (154 is 1.2 × the guard) |
| [nit] stale frame sizes (§4, interp.rs) and revision-21 depth figures | Frame sizes re-read from the checker (release `eval` 272 + 16, `run` 512 + 16, `call_fast_arg` 256 + 16, `call_builtin` 688 + 16; interp.rs "16–704 bytes"); the §4 depth examples re-measured on the revision-24 builds (`expr` tree 27 / VM 126 debug, 31 / 126 release; `lambda10` 23 / 49 debug, 22 / 35 release; `distinct` 130 × 40 parentheses tree 27, deferred 28, eager 126 debug) |

`scripts/vm_wasm_depth.sh --require-default-stack` passes on the revision-24 builds, with the
constants at the checker's fixed point on both profiles (`CALL_FAST_ARG` and `CALL_MUT_OP` re-fit):
every chain and the 29 probes panic rather than trap, with VM ≥ tree.

Revision 25 (2026-10-09) answers the twenty-second review (`reviewer`, verdict "incorrect": 1 must-fix,
4 smaller).

| Finding | Resolution |
|---|---|
| [must-fix] recursion through lambdas alone makes the deferred VM panic below the tree: `run_lambda_cold` keeps its frame live under `exec` on the entry that compiles a lambda, and in release a compiled lambda level costs `RUN` (528) while the tree pays 512 (one self-recursive lambda: release tree 119 / VM 118; 130 lambdas each compiled at depth: release 119 / 112, debug 122 / 121) | Fixed as revision 22 did for fn bodies. `run_lambda` (inlined into `call_closure_owned_by`) runs a compiled body through `exec` itself; S9's first-entry rule and the compile move into the cold `compile_lambda` (`COMPILE_LAMBDA`), which returns before the body runs; the tree's lambda level runs in `run_lambda_tree`, charged `RUN_LAMBDA_TREE = RUN` (§4 Recursion). The two repros are probes `lambda_self` and `lambda_chain`. Measured on the revision-25 builds (`reach`, tree / VM): `lambda_self` and `lambda_chain` debug 117 / 127, release 110 / 118. The tree now panics earlier on lambda recursion under the default limit, as revision 23 made it for fn levels |
| [nit] the trap-depth figure is 154 in CHANGELOG, R7, interp.rs and the Revision 24 row, 156 in §4; the default-limit tree depths are only in §4 | Re-measured on the revision-25 builds: shallowest default-stack depth (limit raised) 157 (tree closure chain, debug), above 1.2 × the guard (154). CHANGELOG, R7, interp.rs and §4 say 157; the Revision 24 row says 156, its value then. CHANGELOG and R7 now carry the default-limit tree depths: closure chain 104 (debug) / 93 (release), VM 126 |
| [nit] three §4 S9 citations into vm/mod.rs shifted by revision 24 | Re-derived by content after this revision's edits, with every other vm/mod.rs citation in §1–§14 |
| [nit] `break_fast.ax` names the removed `exec_catching` | Names `catch_flow` |
| [nit] the chains are compared VM ≥ tree only with `AXON_MAX_DEPTH` raised | `scripts/vm_wasm_depth.sh` runs the chains in its `reach` configuration too (limit unset, deepest exit-0 depth) and, with `--require-default-stack`, requires VM ≥ tree there: debug plain / closure / mut / with tree 126 / 104 / 126 / 126, release 126 / 93 / 126 / 124, VM 126 on all |

`scripts/vm_wasm_depth.sh --require-default-stack` passes on the revision-25 builds, with the
constants at the checker's fixed point on both profiles (no constant changed): every chain and the
31 probes panic rather than trap, with VM ≥ tree on every chain (both configurations) and probe.
54 functions carry a guard (52 in the interpreter, 2 in the compiler). `cli_run vm_` and the
`interp::vm` unit tests pass under both feature sets; `scripts/vm_perf_gate.sh` passes in all three
modes.

Revision 26 (2026-10-09) answers the twenty-third review (`reviewer`, verdict "correct": 0 blockers,
0 must-fix, 2 nits).

| Finding | Resolution |
|---|---|
| [nit] `COMPILE_LAMBDA` (and `COMPILE_BODY`) is below its frame and tail: `compile_lambda` and `compile_body` are in no checked cycle, so `wasm_stack_budget.py` skips them | The checker now applies the frame-plus-unguarded-tail rule to every guarded function reachable from `eval` outside a checked component, the tail stopping at checked components and ALLOW recursions. It measured `compile` plus the `Vec` growth under it: `COMPILE_BODY` 144 / 208 -> 1,504 / 1,024, `COMPILE_LAMBDA` 160 / 224 -> 1,616 / 976 (debug / release), set at its fixed point. Constants with no instance in a profile (inlined) are reported as such. §4, interp.rs, the checker docstring and CHANGELOG say so |
| [nit] AXON-COMPLETENESS says 154 for the shallowest default-stack depth | Both files say 157 with the limit raised, 154 as the 1.2 × guard requirement, and the default-limit depths (tree closure chain 104 / 93, VM 126) |

`scripts/vm_wasm_depth.sh --require-default-stack` passes on the revision-26 builds with the
constants at the checker's fixed point (two changed, above): shallowest default-stack depth 157
(tree closure chain, debug), and under the default limit tree closure chain 104 / 93, VM 126 on every
chain. The full gate passes: `cargo test -p axon-core` under both feature sets with `AXON_ENGINE`
unset, `tree` and `AXON_VM_EAGER=1`; `AXON_HARNESS_STRICT=1 scripts/parity_all.sh` in the same three
modes (53 passed, 2 allowed skips); `reference_gate.sh`; clippy (both feature sets and wasm32) and
fmt; the completeness check; the spec lint. `nest_cost` is wasm32-only, so native cost is unchanged.

Revision 27 (2026-10-10) adds three slices from measurement, not from a review. Each was prototyped
on a copy of `5864c423` (S9) and then ported to a branch with its red test before the spec text was
written; the numbers below are `perf stat -e instructions:u -r 3` medians on the compilebench host.

| Slice | Why | Measured |
|---|---|---|
| S10 array elements in pure loops (§4 S10) | compilebench `sieve` (run `20261010T015516Z`): the S9 VM retired 53.53 G against CPython's 1.09 G; both loops ran generic ops because `xs[i]` and an `if` were not pure | 24.57 G on `944fe056` (budget 26.76 G, half the S9 median); `vm_index_sieve_loops` red on S9 (`0 loops`) |
| S11 pure-`i64` function tier (§4 S11) | after S10, fib-recursive is the one compute program where the VM retires fewer instructions than CPython (0.67×, S10 `944fe056` vs run `20261008T142344Z`) yet runs slower (S9 1.29× wall, run `20261010T015516Z`) | fib(32) 2,295,393,887 → 1,002,713,546; compilebench `warmup` 1,868,042,622 → 804,838,423 (`2627dad7`); `vm_purefn_fib_in_registers` red on S10 |
| S12 wasm32 traps (§4 S12) | compilebench AX-59 (deep value drop), AX-60 (deep source in the front end), AX-61 (`host_await_val` payload copy) trap below the guard | prototype: each repro exits 134 before and gives the tree's panic or E0000 after, under both engines and profiles; the parser's limit is 224 so its worst frame chain fits 448 KiB statically |

S10's code commit `944fe056` failed `vm_wasm_depth.sh`: `wasm_stack_budget.py` found the
`Compiler::pure_stmts`/`pure_if` recursion outside `ALLOW` (it is `PURE_DEPTH`-bounded) and
`COMPILE_STMT` 16 bytes short in release. Both are fixed in a separate commit before S12, and the
checker re-runs after S12.

Revision 28 (2026-10-10) answers the twenty-fourth review (`reviewer`, verdict "incorrect": 2
blockers, 6 must-fix, 1 nit group), the first review of S10-S12. Code fixes are `faf25a22` (tests,
perf gate) and `91b13427` (S12).

| Finding | Resolution |
|---|---|
| [blocker] S12: array, option and tuple chains still trap on drop; the checker does not refuse them (an untyped `dict_get` result unifies with anything) | Every `Value` container drops through `drop_bounded` on wasm32 (`Elems`, `VBox`, `Queue`, `Held` newtypes, zero-cost natively); the false checker claim is removed (§4 S12 Drop). Array and tuple chains at 100,000 exit 0 in both profiles and engines (12 runs; 134 at `e0f29182`). Option/result chains are bounded on drop; cloning one is not and is recorded (AX-62, below) |
| [blocker] S12: each string-interpolation slot parsed from depth 0, so nested slots bypassed the 224 limit and release trapped | A slot parses from the enclosing literal's `expr_depth` (both targets; native's 4,000 shared the same way). Three nested levels of 110 parentheses: E0000 at line 2 col 5, exit 2, both profiles and engines (release exited 134 before); probe `front_interp.ax`, `wasm_nesting_parity.sh ax60` case `interp` |
| (first blocker, cont.) `wasm_stack_budget.py` passed a drop component once any member was `bounded_drop::`, so sub-cycles avoiding it passed | The check now removes the `drop_bounded` nodes and fails unless the rest of the component is acyclic. It fails on `e0f29182` in both profiles and passes on `91b13427`; it also found an unbounded cycle through the closure drop, now fixed |
| [must-fix] S12: the height walk does not bound the checker on deep inferred types (`let a{i} = [a{i-1}]`) | Out of scope, filed as compilebench AX-63 (debug traps at N=3,000, release at 5,000; native prints `7`); §4 S12 no longer claims the walk bounds the checker's recursion over types |
| [must-fix] S11: the qualifying set is narrower than stated | §4 S11 Qualifying lists `pure_shape`'s exact ops; a two-local compare, a literal on the left, a computed operand and `&&`/`\|\|` disqualify. `vm_purefn_not_qualified` covers all four (no `vm: purefn` line, same output as tree) |
| [must-fix] S11: `plain.ax` now runs on the tier, so the generic fast-call frames are no longer depth-probed | New chain `tests/fixtures/vm_depth/plain_generic.ax` (no `vm: purefn` line), bisected in all three configurations: default-stack tree / VM debug 238 / 899, release 253 / 557 |
| [must-fix] S11: "every other program within 0.5 % of S10" had no check | `vm_perf_gate.sh` has a `REF` column of S10 medians (`944fe056`, recorded in §10) and fails a median more than 0.5 % above it; current medians are within ±0.000 % |
| [must-fix] S11 motivation: "fib is the one program slower than CPython" was false, and the run was S9, not S10 | Reworded (§4 S11, §10, Revision 27 table): fewer instructions than CPython yet slower; S10 instruction ratio and S9 wall ratio cited with their runs; sieve named as also slower |
| [must-fix] S10: `vm_index_writes_copy_and_replay_panics` overflowed at i=0; `vm_index_out_of_bounds_and_for_variable` could not tell a one-iteration assignment from counter coupling | The overflow now comes at i=3 of `0..=3` after three committed iterations; a new case prints `46` (coupling would print 10). Both fail when their expectation is broken |
| [nit] S10 array checks run at build only; over-budget loops still emit the op; `ForInit` names no op; stale `write_place` citation; E0000 position | §4 S10 states build-time vs per-entry checks and that an over-budget op declines on every entry; the bounds are two `StrictInt` ops; `write_place` cited by name; E0000 is reported at the enclosing statement's start (spec, CHANGELOG and `parse_expr` comment agree) |

Revision 29 (2026-10-10) answers the twenty-fifth review (`reviewer`, verdict "incorrect": 3
blockers, 2 must-fix, 4 nits), the second review of S10-S12. It found no correctness bug in S10 or
S11 (about 35 repros byte-identical under tree, vm and vm+EAGER); the blockers are wasm32 traps in
S12. Code fixes are `c33f521b` (`wasm_nesting_parity.sh` runs all three cases when `parity_all.sh`
calls it bare) and `e5f754ab`.

| Finding | Resolution |
|---|---|
| [blocker] S12: match guards bypass the height walk (`children()` of `Match` skipped `arm.guard`); a 3,000-term guard traps | `children()` pushes the guard; guards of 300, 1,000 and 3,000 terms give E0000, exit 2, on wasm32 debug and release under both engines (were 0/0/134/134 and 134 at 1,000 and 3,000); the `vm:` trace lines are unchanged on all 389 corpus files and `vm_parity.sh` passes (335 files, 0 differ) |
| [blocker] S12: a `let` inline-refinement predicate is kept outside the statement tree, so no height check saw it | The predicate is height-checked after parsing; 600- and 3,000-term predicates give E0000 (3,000 trapped) |
| [blocker] S12: over-tall chains trap during the parse: `parse_match` clones an arm per pattern before the root check, and a refused or erroneous root drops recursively | The operator layers and `parse_postfix` bound the height of what they build and drop a too-tall node's children iteratively (`ast::clear_children`), so the parser never holds a tree much taller than 224; `parse_match` checks guard and body first and moves them into the last arm (§4 S12). 10,000-, 30,000- and 100,000-term chains, a 3,000-term arm, a 5,000-term or-arm and compound chains give E0000 (all trapped); 100,000-term chains followed by a syntax error give native's syntax error, exit 2 (trapped). Parser frame: release 1,904 B (224 × 1,904 = 426,496 ≤ 458,752), debug 1,664 B; native output unchanged at ≤ 4,000 terms |
| [must-fix] S12: `front_some`/`front_type` were rejected by the checker (E0301) at every depth, so they tested nothing past the parse | Both end in a `match`; reach 110 / 222; new probes `front_arm`, `front_guard`, `front_or_arm`, `front_refine`; `vm_wasm_depth.sh` fails a front probe whose reach is 0 |
| [must-fix] S11: the Qualifying list was broader than `pure_shape` (Revision 28's "exact ops" was false) | §4 S11 states `CallFastLocalInt` qualifies only with `+`, `-`, `*`; a param or literal left of a stack operand disqualifies; at most 255 registers; the type named `i64` only |
| [nit] A `Some` pattern costs one level, not two | §4 S12: a `Some` expression costs two levels, a `Some` pattern one |
| [nit] §4 S12 said native code paths are unchanged; §9 forbade the exit 2 the front probes need | §4 S12 names the native interpolation-slot change; §9: no nest probe exits 134, every front probe exits 0 or 2 with E0000 and reaches above 0 |
| [nit] S11 tests that cannot fail: `uncertain` case disqualified by a builtin; no `\|\|` case; the `plain`/`plain_generic` tier split unchecked; one-op wording | Builtin-free `uncertain` body (removing the check at purefn.rs:173 fails it); `\|\|` case; `vm_wasm_depth.sh` asserts the `vm: purefn` line for `plain` and its absence for `plain_generic`; §4 S11 says a one-op body whose op is a call takes a generic frame |
| [nit] `FOp::Call.t` patched and validated but never read | The patch and validation are removed for `Call`; the doc names the ops that read `t` |

Revision 30 (2026-10-10) answers the twenty-sixth review (`reviewer`, verdict "incorrect": 0
blockers, 3 must-fix, 1 nit). It found no wasm32 trap left in the parser, and confirmed that S11's
Qualifying list matches `pure_shape` and that the parser budget figures hold. The code fix is
`fcd18f2b`.

| Finding | Resolution |
|---|---|
| [must-fix] S12: an interpolation slot settled its own chain cut, so E0000 beat a later syntax error (missing `)`, dangling `+`, stray token in the slot) | The slot runs its leftover check and then passes its `tall` flag to the outer parser, which settles at the root's end. All three shapes now give native's error and position on wasm32 debug and release under both engines (4:1, 3:1, 3:5); they are added to `wasm_nesting_parity.sh ax60`, next to a 1,500-term slot chain that still gives E0000 |
| [must-fix] S12: "a syntax error later in the same source wins" overclaimed | §4 S12 limits it to an error later in the same root after an operator or postfix cut (slots included). Parenthesis, block, pattern, type or `else if` refusals, and a refusal followed by an error in a later statement, are reported as E0000 at the root's start |
| [must-fix] S12: walking match guards changes native output (E0606 on a guard reading a `&mut`-borrowed argument, W0002 wording, capability checks), but §4 said native changes only in slots | §4 S12 names both native changes; red test `native_guard_borrow_e0606` (§8; exit 0 `5 5` before, E0606 exit 2 after); CHANGELOG states it |
| [nit] debug-wasm32 lexer traps on long string literals (about 11,000 characters) and block comments (50,000) were not listed out of scope | Listed in §4 S12's out-of-scope paragraph; filed as compilebench AX-65 (pre-existing, logos per-character recursion, debug only) |

Revision 31 (2026-10-10) answers the twenty-seventh review (`reviewer`, verdict "incorrect": 0
blockers, 2 must-fix, 2 nits). It found no way for a slot cut to escape settling (nested slots,
slots in refinements, guards, or-arms, contracts, constants, handlers, lambdas, `else if`, 100,000-term
slot chains) and every Revision 30 repro reproduced. Tests and scripts are in `2fdf7cc1`; no
interpreter code changed.

| Finding | Resolution |
|---|---|
| [must-fix] S12: the native-change list omitted two guard-walk effects: `mentions_var` in eval (in-place append no longer runs when a guard assigns the target; `zza` -> `starta`, `3` -> `2`), and the checker's purity scan (E1207 on a guard calling a non-pure fn) | §4 S12 lists every consumer of `walk_expr`/`value_position_idents` that now sees guards; cli tests `native_guard_assign_in_place` and `native_guard_pure_e1207` (both engines; before/after measured on `2627dad7` and `fcd18f2b`); the CHANGELOG lists each consumer |
| [must-fix] S12: the "other refusals" list missed height-checked arm bodies and predicates without a cut, and errors after the root that are not in a later statement | §4 S12 states the rule exactly: only a chain cut defers the refusal to its root's end; every other refusal, and any error after the root ends, gives E0000 at the root's start |
| [nit] the native `instructions:u` clause had no script; fib-recursive had no `REF` | `vm_perf_gate.sh` gives fib-recursive the S11-commit median (`2627dad7`, 1,002,729,957, the gate's own `--no-default-features` build; the reviewer's 1,010,695,541 came from a default-features binary); measured at `fcd18f2b`: 1,002,729,327, -0.000 % |
| [nit] `wasm_nesting_parity.sh` ran only the debug wasm | It runs every case on debug and release (216 ok lines, 108 each); an explicit `AXON_RUN_WASM` still runs one module |

Revision 32 (2026-10-10) answers the twenty-eighth review (`reviewer`, verdict "incorrect": 0
blockers, 2 must-fix, 1 nit). The code fix and tests are in `a875d2b8`.

| Finding | Resolution |
|---|---|
| [must-fix] S12: the native-change list left out four codegen `walk_expr` consumers, two of them observable: `axon build` now refuses a guard `tier:` with E0910 (it built before), and a guard that writes `a[0]` after `let b = a` no longer changes `b` in the binary (`9 9 1` -> `1 9 1`, as `axon run` prints) | §4 S12 and the CHANGELOG list the E0910 refusal, `expr_calls`, `collect_binders` and `written_place_roots`. Codegen cli tests `native_guard_tier_e0910` and `native_guard_write_unaliases` were added, with before/after measured on `2627dad7` and `a875d2b8` |
| [must-fix] S12: `parse_type_def` rewound on any error from its speculative base-type parse, so a type 300 `Option<` deep before `where` gave a false syntax error (1:16, `expected item`) on wasm32 instead of the refusal | On wasm32 a nesting refusal now passes through (`NEST_ALL && too_deep`), and native keeps the rewind. It is the only parse that rewinds on `Err`. `tdef_where`, `tdef_enum` and `tdef_tuple_pred` now give E0000 at the level that trips (1:1578, 1:1578, 1:132) on debug and release under both engines, and they are added to `wasm_nesting_parity.sh ax60`. §4 S12 states that a type outside an expression is refused at that level, not at a root's start |
| [nit] §10 gave no `REF` for fib-recursive; the §13 S12 row omitted the guard tests, the parity script and the perf-gate `REF` | §10 states fib-recursive's `2627dad7` `REF`; the §13 row names them all |

Revision 33 (2026-10-10) answers the twenty-ninth review (`reviewer`, verdict "incorrect": 1
blocker, 1 must-fix, 1 nit). The code fix and tests are in `8e4bd760`.

| Finding | Resolution |
|---|---|
| [blocker] S12: `parse_attr_atom` recursed once per nested `[` or `-` in an attribute argument with no charge, so `@[foo(x: [[..1..]])]` trapped on wasm32 (release at 4,000 levels, debug at 6,000, a 20,000 `-` chain in both); `wasm_stack_budget.py` looked only at components holding a charged call, so it could not see this | On wasm32 `parse_attr_atom` runs through `nested`: E0000 at 1:234 on debug and release under both engines; native still prints W0001 and `ok`, byte-identical. The budget script now fails any recursive component reachable from `parse_program` that has no charged call and holds a `Parser` method, and any charged name without a `self.nested(Self::..)` site; on `99e6aeb8` it fails with both lines. Cases `attr_bracket` and `attr_neg` in `wasm_nesting_parity.sh ax60` fail on the pre-fix build |
| [must-fix] S12: the refusal rule named only types as refused outside a root; a handler definition's `on` pattern and a chain cut inside a tuple type's predicate are refused elsewhere | §4 S12 states both positions (1:1150; the token after the outermost type, 1:1226 and 1:1224) and the attribute position. Cases `handler_pat`, `tdef_chain` and `fn_param_chain` assert them; the behaviour predates this revision |
| [nit] "cover them" overstated the guard tests: W0002 and several consumers had none | `native_guard_w0002` asserts the "re-declares" message and fails with the guard push removed from `ast::children`; §4 S12 names which consumers the six tests gate and which ride on the shared change |

Revision 34 (2026-10-10) answers the thirtieth review (`reviewer`, verdict "incorrect": 3 must-fix,
1 nit). The script fix and parity cases are in `6ae2c76f`.

| Finding | Resolution |
|---|---|
| [must-fix] S12: `wasm_stack_budget.py` built the parser components from direct calls only, so with `Parser::nested` not inlined every parser cycle became a `call_indirect` and the check passed at 320 B per unit | Reachability from `parse_program` runs over direct plus same-signature `call_indirect` edges; a widened parser recursion that differs from its direct component fails, as does a charged `parse_expr` outside any direct recursion; `PARSER_FN` matches generic, closure and v0-mangled Parser methods. The `inline(never)` mutant now fails with all three lines; HEAD passes both profiles with unchanged numbers (debug 224 × 1,664, release 224 × 1,904) |
| [must-fix] §4 Recursion said the front-end limit "bounds every later walk"; three checker walks over chains of declarations (`@[contained]` call chain, `@[total]` chain, refinement-type names) trap wasm32 on flat source | Filed as compilebench AX-66 (predates S12; the same build `99e6aeb8` traps). §4 Recursion narrows the claim to walks over one expression, and §4 S12's out-of-scope paragraph lists the three walks with their trap points |
| [must-fix] S12: the root definition missed item-level predicates and handler arm bodies, whose refusals are at the predicate's or body's first token | §4 S12 defines a root as any expression started outside every counted level, lists them, and gives each position (`type` predicate 1:20, fn parameter predicate 1:19, `@[verify]` 1:10, handler arm body 1:36, `let` annotation at the `let`). Parity cases `tdef_pred`, `fn_pred`, `verify_pred`, `handler_body` and `refine_paren300` assert them |
| [nit] §8 and §13 omitted `native_guard_w0002`; "two new checks" described four | Both list it; §4 and §13 say four checks and name the out-of-root parity cases |
