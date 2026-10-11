# S7 confirmation review: R51 sealed path (reviewer: Seal)

## Trees
- head: /home/cklaus/projects/axon-lanes/r51-s7 @ 3921a309 (release build `cargo build --release -p axon-core --no-default-features --bin axon`, target-s7-Seal)
- base: origin/main 5864c423
- reference: v0.22 /home/cklaus/projects/axon-lanes/v022-ref @ c9ed2ddd (release build, target-s7-Seal-v022)

## Method
1. Mapped every v0.22 seal edge (seal_call, seal_dispatch/seal_method, seal_width(_unary), seal_global, seal_refine, return casts, SEALED_*_MARK -> SealMarks, PICK, no_such_fn, contain_loop_control, TestEnd) to its head counterpart and walked main-only paths (operand/binop_operands, index_in_place/field_in_place, assign_in_place(_shaped), write_place, call_mut_with/call_mut_sealed, chan_method, dispatch_named, impl_method(_sealed), call_fast*, first_entry_on_tree, Op::Tree, run_handler_arm/run_arm_sealed, with_abort_enter/exit, eval_prob_pred, goal paths, slot captures). Two read-only scouts (EdgeMap, FalsePaths) plus own reading.
2. Enumerated every `::<false>` / unsealed-only call site at head: each is behind `self.seal.active`, the `T` const, `entry.plain` (never set for a sealed fn), or VM-only code; `engine_for` gives a sealed run the tree and `compile` asserts `!seal.active`. Nothing moved in ffe2b413/3561022b (`sealed_branch!`, call_mut split, struct_refine_sealed, run_arm_sealed, with_abort_enter/exit, eval_prob_pred) is reachable unsealed-only while the seal is active.
3. Black-box: 45+ sealed suites (honest + attack variant each) run through the real CLI (`env -i`, AXON_PATH, AXON_PATH_EXCLUSIVE=1, `axon test <suite> --json --seal <cand>`) on head AND v0.22, flagging verdict divergence and attack passes. Harness: /var/tmp/r51-tmp/s7-Seal/{run.sh,run022.sh}, /var/tmp/r51-tmp/s7-Seal/b/{gen.sh,diff.sh}.

## Results (observed)
- `tasks/r51-smoke/run.sh <head> post`: PASS honest passes, PASS no VM trace, PASS AXON_ENGINE=vm refused, PASS attack_index/attack_field/attack_tuple refused.
- Engine pin: unset -> runs (tree); `tree` -> runs; `vm` -> rc=2 "a sealed run executes on the tree engine only ..."; `TREE`/`Vm`/`garbage`/empty -> rc=2 "AXON_ENGINE must be vm or tree"; `AXON_VM_TRACE=1 AXON_VM_EAGER=1` -> 0 `vm` lines; property test (`@[forall]`) under `AXON_ENGINE=vm --seal` -> rc=2.
- Attacks not in taint_tests.rs, all REFUSED on head with verdict identical to v0.22 (no DIVERGE): closure picked by index from tainted fn value (c01), compound index (c02), `&mut` int write-back (c03), strlen index (c04), while-count pick (c05), late capture (c06/c20), in-place swap (c07, c15, c15b), if-expr flag (c08), dict-of-closures (c09, c28), `P(dist <= k)` probability predicate into an index (c11, eval_prob_pred path), `&mut` candidate/operator arrays (c13/c14), filter callback (c18), nested place assign (c23/c24), slot rebind in loop (c29), global initialised from candidate (c30), abort-capable handler arm (c31), match picking a fn value (d01), for-loop trip count (d02), sort comparator order (d03), fold accumulator pick (d04), filter survivors (d05), tuple-field closure (d06), `&mut` setk index (d07), Option branch (d08), `scheduler_spawn(key())` (d09), `sandbox_run(sb, key()+"_n")` (d10), goal_run/goal_best_input on a candidate @[adaptive] (g1), width via `a = a + 1` assign fast path (w1), `xs[0] = xs[0] + 1` index-in-place (w2), shift (w3), impl dispatch through array+tuple (m1), property test (p1). Every honest variant that does not route through a candidate-derived selector passes (c00, d02/h, w1-3/h).
- Completion evidence (`--completion-key-stdin`): honest -> `status ok` + completion token, identical token on head and v0.22; `exit(0)` in candidate -> `status ok` with NO completion token on both; `break`/`continue` in candidate fn or closure -> failed "`break`/`continue` outside a loop"; `return` inside a candidate lambda -> contained.
- Name shadowing (candidate defines `strict`, `accept`, a second impl of the operator trait) -> E0002 refused, both trees.

## Summary table
| id | severity | location | claim | status |
|---|---|---|---|---|
| S7-Seal-1 | NOTE | resolver.rs:2435-2440 (v022-ref resolver.rs:2438 identical) | sealed set membership | symlinked candidate file escapes the seal; pre-existing, refused by axon-psv input check |
| S7-Seal-2 | NOTE | interp.rs:4906-4908 vs v022 interp.rs:3579-3581 | no_such_fn uniform text | depth check now precedes seal_call; not reachable without static-check bypass |
| S7-Seal-3 | NOTE | v022-protected-suite-verdict.md:251 | PSV-1 scope | `flag() \|\| strict(r)` short-circuit passes on both trees: documented operator obligation, not a selector |

No BLOCKER, no MAJOR.

## Findings

### S7-Seal-1 (NOTE) A candidate module that is a symlink out of the sealed directory is judged as operator code
- Location: crates/axon-core/src/resolver.rs:2435-2440 at 3921a309:
  `let p = p.canonicalize().unwrap_or(p); sealed.iter().any(|d| p.starts_with(d))`. v022-ref resolver.rs:2438 is the same line.
- Repro (observed): `cand/sol.ax -> /var/tmp/r51-tmp/s7-Seal/outside/sol.ax` with the c01 attack candidate: head `p1f0` (ATTACK-PASSES), v0.22 `p1f0`. The same candidate as a regular file is refused on both.
- Impact: none under the protected runner, which refuses any symlink in a guest input (crates/axon-psv/src/lib.rs:297-313). Not introduced by R51 (v0.22 parity). Recorded so a non-psv caller of `axon test --seal` knows the seal assumes symlink-free inputs.
- Smallest fix (optional): in `set_sealed_module_dirs`'s callers / the loader, refuse a symlinked module file under a sealed dir, or match on the un-canonicalized path as well.

### S7-Seal-2 (NOTE) `enter_fn` (depth text naming the callee) now runs before `seal_call`
- Head interp.rs:4906-4908: `fn call_fn_in(...) { nest_guard!(self, CALL_FN_IN); let _guard = self.enter_fn(entry.def)?;` then `call_fn_in_general` -> `call_fn_sealed` -> `seal_call`. v0.22 interp.rs:3579-3581 ran `self.seal_call(f)?` first in `call_fn_sealed`.
- At the depth limit a sealed call of an operator fn would read "recursion limit exceeded (...) in `strict`" instead of the uniform `sealed_no_fn_msg`. Not reachable from a candidate today: a candidate that names an operator fn is refused statically (observed: s2_depth_a -> E0001 "cannot find name `strict`", rc=2), and the name-resolving builtins check visibility before calling (builtins.rs:4458-4461, 4602-4605). Only matters for the T3/T8 static-bypass tests.
- Smallest fix: in `call_fn_in`, `if self.seal.active { self.seal_call(entry.def)?; }` before `enter_fn`, or drop the name from `depth_exceeded` for a sealed caller.

### S7-Seal-3 (NOTE) A candidate-controlled short-circuit that skips an operator check passes (both trees)
- Suite `assert(flag() || strict(r))` with `flag() -> true`, `solve -> 99`: head `p1f0`, v0.22 `p1f0`.
- Not a PSV-1 selector (no closure/fn value picked, no name, impl or width); the spec places it on the operator: v022-protected-suite-verdict.md:251 "never let candidate data ... choose between a strict and a lenient check (... a branch that runs a weaker check ...)". No action for R51.

## Verified-as-held (from earlier reviews / latest commits)
- `call_mut` sends a sealed run to `call_mut_sealed` before argument evaluation (eval.rs ~1777); `call_mut_with` is unsealed-only. c03/c13/c14/d07 refused with v0.22 parity.
- `sealed_branch!` takes the `t_branch` path only in the `T = true` instance; tainted branch picks (c08, d01, d08) refused.
- `run_arm_sealed`/`with_abort_enter/exit`: abort-capable arm attack (c31) refused.
- `eval_prob_pred` (cold, own guard) evaluates through the sealed evaluator: c11 refused.
- SendValue round trip (`ClosureVal::plain`, drops SealMarks) is reachable only via `host_await_val(_opt)` (builtins.rs:3178/3194), which needs a host driver that `axon test` never installs.
