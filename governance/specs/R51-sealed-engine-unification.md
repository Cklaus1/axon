# R51 — One interpreter for v0.22's sealed runs and R50's bytecode engine

**Spec ID:** `R51-sealed-engine-unification`
**Status:** Implementing (all slices S0-S7 built on `r51/int`; not merged into `main`). Reviewed 2026-10-10 (Step 1
adversarial review: VmPin, TaintPort, S0Merge, EvidenceGates; reports under `governance/reviews/2026-10-10-r51-*.md`).
Open questions resolved (§12). S6.1 (2026-10-11): `SEAL_RUNTIME_COMPLETE = true`; sealed runs execute on the tree
engine with the seal, the deferred-test registry is empty. Per-slice receipts are in §14.
**Risk class:** Structural and security-critical (re-homes the PSV-1 runtime seal onto a rewritten data model; adds a refusal on the default engine).
**Author / date:** 2026-10-10, after merging `origin/main` (5864c423) into the v0.22 head (`c9r16/integrate15`, c9ed2ddd) was found to be a re-implementation, not a merge.

```spec-meta
id: R51-sealed-engine-unification
status-claim: Implementing
depends-on: R50-register-vm
blocks: none
blocked-by: none
supersedes: none
related: R50-register-vm, v022-protected-suite-verdict, v022-psv-protocol, v022-ADR-003-protected-certification-deferral, post-c9-hardening
conflicts-with: none
reserves: none planned (a sealed-run engine refusal reuses an existing exit code; §6)
evidence: scripts/vm_parity.sh; scripts/vm_wasm_depth.sh; scripts/vm_perf_gate.sh; tasks/r51-smoke/run.sh; mutation receipts governance/status/r51-t7-*.json; spec evidence ledger is §14
```

R47–R49 are reserved by `R46-in-flight-operations.md` §2, so this spec takes R51.

Review markers: `[REVISED: was …, reason: <finding id>]`. Finding ids refer to the four review reports.

---

### 1. Motivation

Two lines of work diverged at `abe2ca36`:

- **main** made `axon run` fast. AX-40/46/47/53/54/55 rewrote the interpreter's data model
  (`Value::Struct(Rc<StructVal>)` holding `Fields(Vec<(Sym, Value)>)`, `Enum(Rc<EnumVal>)`,
  `Closure(Rc<ClosureVal>)`, `Handle(Rc<HandleVal>)`, Rc'd `Tuple`/`Decimal`, `SizedInt` carrying `IntWidth`;
  `Env { vars: Vec<(Sym, Value)> }` with slot-resolved access), and R50 added a bytecode engine that is the
  default (`Engine::DEFAULT = Engine::Vm`).
- **v0.22** (branch `v022/veto`) made a candidate's verdict trustworthy. The PSV protocol runs candidate code
  against a hidden check; the PSV-1 runtime seal (taint carried by the evaluator, seal edges where sealed code
  reaches operator code, the PICK bit, the sealed-only static check) is written against the OLD model:
  `Env.vars: Vec<(String, Value, u8)>` with a per-binding taint byte, `HashMap` struct fields, unboxed enums
  and closures, and edges on the tree path only (`seal_call`, `seal_dispatch`, `seal_width`, `seal_global`,
  `seal_refine`, the return casts, `SEALED_*_MARK`).

Git interleaves the two APIs hunk by hunk; no selection of hunks compiles and keeps both. Keeping two
interpreters is not an option either: every feature, fix and builtin would be written twice, v0.22 would keep
falling behind (137 commits at the time of writing), and main would never carry the protections.

**The hazard.** R50's compiled bodies (`run_body_vm`, `exec`) and fast calls (`Op::CallFast`,
`CallFastLocalInt` → `call_fast` / `call_fast_arg`) skip `call_fn_in`'s dispatch, where every seal edge lives.
A naive merge would run sealed candidate code on the default engine with no seal edges and no taint: PSV-1
silently off, every test green.

[REVISED: the hazard was stated for the VM only, reason: VmPin-4.] **The tree engine on main has the same
hazard without any VM involvement.** Main's AX-40..55 and R50 S2 fast paths sit where veto put seal edges:
the `plain` split inside `call_fn_in` (`call_fn_common` skips everything but params, `goal_met`, body and
`scalar_return`); raw `self.globals` reads in `index_in_place` (`TABLE[0]`), `field_in_place` (`CFG.k`),
`dispatch_named` step 4 (closure-constant call), `ident_unbound`, the Index arm existence test and
`vm/mod.rs:2408`; fn lookup through `fn_of_sym`/`fn_table`/`callees`; and `operand()`, which reads BinOp
operands without `eval` and therefore without taint. Three of these re-open holes veto closed in C9 round 9.

### 2. Requirement link

Closes the "not merged with origin/main" note on `engineering_qualified` in `governance/v022_release_claims.json`
(v0.22 branch). Acceptance anchors: the engine-pin test (§8 T1), [REVISED: was "`scripts/vm_parity.sh` and
`scripts/vm_perf_gate.sh` unchanged", reason: S0Merge-6, EG-6, EG-7] the R50 gates passing under the merged
binary rule with identical corpus and verdicts and the §10 cost bound (§8 T2), the v0.22 PSV-1 suites green on
the unified tree (§8 T3-T6).

### 3. Surface

[REVISED: was "No language change", reason: S0Merge-3, S0Merge-8.] No new syntax. Behaviour changes:

| Situation | Today on main | After R51 |
|---|---|---|
| `axon run` / `goal` / `test` without `--seal` | VM by default | unchanged engine selection |
| a sealed run (`axon test --seal`, the axon-psv runner child, the sealed check child) | n/a on main (no seal) | tree-walker with taint, always (after S6); **refused** before S6 |
| a sealed run with a VM requested (`AXON_ENGINE=vm` or the in-process `set_engine(Vm)` override) | n/a | **refused** with a stated message; never silently falls back, never runs on the VM |
| a sealed run on a tree whose runtime seal is incomplete (slices S0-S5) | n/a | **refused**, whatever the engine (`SEAL_RUNTIME_COMPLETE = false`) |
| a callee's `break`/`continue` escaping its fn | ends the caller's loop silently | contained; the call is an error (veto 4e7851bc, bcf9c0a7, 2175cc1b) |
| an `@[test]` fn returning `Err` or exiting through `?` [REVISED: was "returning `Err` or exiting through `?`", reason: S7-UnsealedParity-8.] — or ending early on `None` (`return None`, which leaves the same trace as a `?` on `None`) | `passed` | fails: a test passes only if its body completed (veto 89256bd3, 91c2f676). An early `return None` fails with "the test ended on `Err`, or early on `None` (a `return None` or a `?` on `None`): it did not complete"; v0.22 passed it (its `axon test` passed every early end), R51 fails it because it cannot be told from the `?` exit; a tail `None` still completes. `axon improve`'s G3 test-flip check reads the same verdict |
| `Uncertain` values derived from several sources | first source's tag | least-trusted `source_tag`; `ai_extract_uncertain_*` is stamped AI (veto ea9ba38d, 6de01247) |
| checker | — | v0.22 checker changes arrive with S0: E0004 (sealed-only static refusal), E0505 (an impl for a type the runtime represents as another), `srand` classed Random, `temporal_now/new/is_valid` classed Time, match guards walked by `walk_expr`; [REVISED: added, reason: S7-UnsealedParity-5.] a method call on `()` with no impl for `()` is E0403 at check (was a runtime panic after earlier output; veto fa0643ac), and the deferred type names `Uncertain`/`Temporal`/`Goal`/`Dict` are exact names, so an undefined `DictFoo` is E0308 "unknown type" (was accepted; veto 3849fadb, amendment 117) |
| [REVISED: rows below added, reason: S7-UnsealedParity-1..4.] a `break`/`continue` in an effect-handler arm whose `with` sits inside a loop | ends that loop | an error at the arm's edge: "`break`/`continue` cannot leave an effect-handler arm (a loop around the `with` is not the arm's loop)" (veto 2175cc1b) |
| an effect-handler arm's completion inside a nested `with` | caught by the nearest `with` (the program in vm/tests.rs `t_callee` prints 107) | caught only by the `with` that installed the handler (prints 7; veto 5e454380) |
| resolver: a second definition of an impl (same trait and type), of a method a value of one type dispatches to (from two traits), of a trait, of a module-level `let`, of a named refinement | the later one wins (or the second is ignored) | E0002 "… is defined more than once in this program" (veto 2175cc1b, abaf8bbd) |
| resolver: a named refinement that reuses a type-level name: a builtin type, `Option`/`Result`/`Chan`, a deferred name (`Uncertain`, `Temporal`, `Goal`, `Dict`, exact), a struct or enum, any generic parameter of the program | the refinement attaches to every annotation with that name | E0002 "the type name `T` (a refinement cannot reuse a type's name) …" (veto 5e454380, 36ab754c). The deferred names are exact, as in the checker: v0.22 refused any name STARTING with one (`GoalScore`, 4cc6360e), a rule from before the checker took exact names (3849fadb); R51 accepts `GoalScore` (S7-UnsealedParity-6) |
| a parameter declared bare `Uncertain` or `Temporal` (no type argument) | the wrapper is dropped to its inner value (`u.confidence` panics "field access on non-struct") | the wrapper is kept, as for `Uncertain<T>` (veto 3cf23eef) |
| `assert_eq_f64` with a NaN operand | passes | fails "assertion failed: NaN != …" (veto 5e454380) |
| an `@[forall]` case that ends through `exit(0)`, `exit(n)`, a refinement violation or another non-panic stop | the case passes | the case fails with its reason (`exit(0)` "ended the property case before it completed", veto 5e454380; the others through the flow's message, veto 4e7851bc) |

"Sealed check child" means the `axon test --seal` process that axon-cortex's `LocalInterpreterExecutor` spawns
(`axon-cortex/src/runner.rs`). It is unrelated to `axon-fabric/src/sealed_exec.rs` (hash-and-exec of authority
programs). [REVISED: added, reason: VmPin-11.]

```text
$ AXON_ENGINE=vm axon test --seal cand suite.ax
error: a sealed run executes on the tree engine only (AXON_ENGINE=vm cannot carry the seal); unset AXON_ENGINE
$ axon test --seal cand suite.ax          # before S6
error: sealed runs are refused on this build: the runtime seal is not complete (R51 SEAL_RUNTIME_COMPLETE)
```

### 4. Semantics

#### Fork 1 — which engine runs sealed code

- (a) **Tree only, pinned. Chosen for this spec.** The seal machinery already exists for the tree path, has
  [REVISED: was "~2290", reason: EG-2] 2281 active mutation rows (+9 sibling-only) and fifteen review rounds
  behind it. Re-homing it onto the new data model is the whole job; also re-homing it into a second engine
  doubles the security-critical surface.
- (b) Taint inside the VM. **Deferred** to a later spec (§12 Q1): only worth it if sealed runs prove too slow.
  Sealed runs are suite verdicts, not hot loops; R50's speed matters for `goal`/`@[adaptive]` re-runs, which
  are unsealed.

[REVISED: was "the seal state set in `main.rs` (and by the axon-psv runner) is passed into the build", reason:
VmPin-1, VmPin-7, VmPin-9.] **Where the pin lives.** The axon-psv, axon-cortex and axon-fabric crates do not
link axon-core; they spawn `axon test --seal` with a cleared environment, so in production a sealed child sees
`AXON_ENGINE` unset and would get `Engine::DEFAULT = Vm`. In-crate library tests seal runs without main.rs. So
the pin is computed in `Interp::build`, in one expression with `seal.active`, from the same input
(`resolver::sealed_module_dirs()`):

```text
sealed = !sealed_module_dirs().is_empty()
if sealed:
    if !SEAL_RUNTIME_COMPLETE            -> refuse (runtime seal incomplete)
    if override == Some(Vm) || AXON_ENGINE == "vm" -> refuse (VM requested)
    engine = Tree                       # AXON_ENGINE unset / "tree" / override Tree
else:
    engine = engine_at_build()          # unchanged
```

The sealed check runs before the override and environment reads. The refusal is an `Err` returned by the run
entry (`run_test_fn*`, `run_program*`, `run_property_test*`); only the CLI turns it into exit 2. `cmd_test`
also refuses early, before any NDJSON line (a courtesy; the build-time pin is the guarantee). `--seal` with
an invalid `AXON_ENGINE` value refuses like any other invalid value (exit 2).

[REVISED: was "A second, independent check in `run_body` …", reason: VmPin-3.] **Defence in depth.**
`vm::compile::compile` is the only constructor of `vm::Body`, and every VM execution path (`run_body_vm`,
`exec`, `call_fast`, `call_fast_arg`, `call_fast_inline`/`leaf_call`, `leaf_arm`, `call_fast_local_int_slow`,
`fast_body`/`leaf_value`/`leaf_store`/`exec_on`, `call_mut_op`/`call_mut_general`/`moves_call`/`run_moves`,
`run_lambda`/`compile_lambda`, `fold_leaf`, deferred compile in S9) needs one. `compile` refuses (panics) while
`ix.seal.active`: no `Body` can exist in a sealed run, at zero hot-path cost. The engine gates (`run_body`,
`run_lambda`, `fold_leaf`, and S0b's `@[test]`-frame twin `run_test_body`) are named in T1's drift test.

[REVISED: added, reason: VmPin-4a.] **Under seal every call takes the general path.** The seal crossing check
is the first statement of `call_fn_in`, before the `plain` split. The split condition becomes a build-time
bool `self.general_calls = !refine_preds.is_empty() || seal.active` (one bool; unsealed cost unchanged).

#### Fork 2 — where per-binding taint lives

- (a) Widen `Env.vars` to `(Sym, Value, u8)`. Rejected: changes main's hot-path layout for every run, sealed
  or not, and R50's perf gate measures exactly that path.
- (b) **A parallel taint vector indexed by the same slot, populated only in a sealed run. Chosen.**
  [REVISED: was "`Option<Vec<u8>>` … unsealed runs carry `None` … `define_var`/`assign_var` gain taint-aware
  twins used by the seal code; the name-check scan fallback keeps both vectors in step", reason: TaintPort-3,
  -4, -10, -11.] Rules:
  - Env's binding storage is private to a `Bindings` type. Every mutating method takes the taint (`t: u8`);
    there is no taint-free binding method, so no site can drop a taint by omission (veto's rule, interp.rs:552).
    Raw `env.vars` access outside `Bindings` does not compile. The raw writes main has today (`bind_params`
    extend, `goal_met` push, closure lend swap and copy-path extend, slotted param push, positional write-back
    zip, `define_var` padding) become `Bindings` methods that keep both vectors in step.
  - Whether an Env carries taints is a property of the run, not of the Env. Envs are created through an
    `Interp`-owned factory that knows `seal.active` (fresh predicate envs, `from_snapshot`, the frame pool);
    a pooled frame is emptied on give and comes back with a taint store in a sealed run. Reading a taint from
    an Env without a store during a sealed run panics. A missing store never reads as 0.
  - A binding's value and its taint are always read through one index-returning lookup (`get_var_idx`); the
    name-scan fallback returns the index, never a reference.
  - `debug_assert!(taints.is_empty() || taints.len() == vars.len())` after every `Bindings` method; in a sealed
    run the same check runs in release builds and a mismatch panics (fail-closed).
  - The tree path runs in every unsealed VM run (S9 first entries, `Op::Tree`, unresolved lambdas), so taint
    must cost nothing there: the evaluator is monomorphised as veto's `eval_t<const T: bool>`; the unsealed
    instantiation contains no taint code. `vm_perf_gate.sh` measures it (§10).
  - Under seal, `eval_binop` takes the evaluated path: no `operand()` fast path (VmPin-4d).
  - New builtin `now_ns` is classed `WORLD_BUILTINS`, like `now_ms` (TaintPort-14).

#### Fork 3 — taint and identity on Rc'd values

[REVISED: was "every value that crosses a seal edge is deep-copied … address-keyed taint tables extend to …
`StructVal` fields via `&mut` write-through", reason: TaintPort-1, -2, -12.] Values cross seal edges by Rc
share exactly as on v0.22. No value is deep-copied at an edge.

- Value-semantic payloads (`Str`, `Array`, `Struct`, `Enum`, `Tuple`, `Decimal`, `Handle`) are copy-on-write or
  immutable on main, so no alias can be written. Their taint rides on the binding and on the accumulator
  (Fork 2), never on an address. Invariant: no taint is keyed by the address of a copy-on-write payload.
- The reference-semantic objects are the same set as on v0.22 (Dict, Chan, closure capture cell), and so are
  their mechanisms: dict snapshots and checks at edges (`dict_edge_in`/`dict_edge_out`/`replaced_ok`), channel
  contract stamping, `objs`/`picked` keyed by `Rc::as_ptr` with a strong clone as the reuse guard. A drift test
  asserts that `addr_of` covers exactly Dict, Chan and Closure.
- **Closure representation (decision, TaintPort-5, VmPin-5, TaintPort-15): option A.**
  `ClosureVal { code, captured: Rc<RefCell<Vec<(Sym, Value)>>>, contract: Option<Rc<Contract>>, seal: SealMarks,
  cap_taints }`. A cast allocates a new `ClosureVal` that shares `captured`, so veto's per-reference contract
  semantics hold. `picked` and every identity test key on `Rc::as_ptr(&captured)`. AX-40's lend test becomes
  `strong_count(captured) == private_refs`. Seal marks (`SEALED_CLOSURE_MARK`, `SEALED_FNVAL_MARK`,
  `PIN_FN_MARK`, `CAP_T`) are typed fields, never entries of the slot-ordered `captured` vector (which would
  shift `param_base`). `size_of::<Value>() == 16` still holds.
- The PICK bit attaches to the value as before; its edges (index chosen by the candidate, builtin selector,
  branch yield, place write, exit-decided result, sealed hand-back) are re-derived on the new constructors.
- [REVISED: added, reason: VmPin-4b/c/e, TaintPort-8.] Main-only sinks are classified as taint touch points
  and/or seal edges, each listed in a drift test: `operand`, `binop_operands`, `index_in_place`,
  `field_in_place`, `assign_in_place(_shaped)`, `write_place`, `call_mut_with`, `chan_method`, `dispatch_call`,
  `dispatch_named`, `impl_method`, `call_closure_arg`, `cond_bool`, `logic_rhs`, `short_circuits`,
  `finish_record`, `bind_let`, `bind_params`, `call_fn_common`, `call_fn_in_general`, `scalar_return`. Every
  global read goes through `global_ref` (veto's drift test, extended to `interp/vm/`). Fn lookup sinks include
  `fn_of_sym`, `fn_of_def`, `fn_table[`, `callees`. `seal_method`/`seal_dispatch` stay in the tree MethodCall
  arm; VM `Op::MethodCall` skipping that arm is irrelevant under the pin and is recorded for Q1.

#### Fork 4 — what happens to v0.22's evidence

Code-anchored evidence does not survive a port. [REVISED: was "Rows anchored in `crates/axon-core/src/interp*`
(~263)", reason: EG-2.] Rows anchored in `crates/axon-core/src` (426 active at c9ed2ddd: 360 `interp*`, 66 in
resolver/main/checker/lib/builtins/parser/seal_split/ast/kernel) are re-anchored and re-run; their kills count
only on the re-run. [REVISED: was "EQUIVALENT retirements in `interp*` are RESET and re-argued", reason: EG-3.]
The four retired rows in `crates/axon-core/src` (M58 interp.rs, M89 interp/builtins.rs, M60 resolver.rs, M3260
main.rs) are re-retired on executed four-cell records (`v022_paired_disable.py --only=`) at the S6 head, or
re-activated if a cell fails. [REVISED: M58 is re-retired as STALE_REFACTORED -> M59 on an executed replacement
record, not a four-cell one, reason: v0.22's separate `call_fn` break/continue arm that M58 mutated does not exist
on main (old text 0 times); the one `frame_edge` arm M59 mutates guards every frame edge, and with it removed
M58's own fn-boundary attack succeeds. e41d6225 had re-anchored M58 onto M59's text, so no four-cell could hold.]
[REVISED: was "Evidence anchored outside the interpreter … carries over
unchanged", reason: S0Merge-5.] Evidence anchored outside `crates/axon-core/src` carries over **only if** its
`old` text still matches exactly once on the merged tree and its killing test is outside `interp*` and passes at
S6; every other row is re-run like an `interp*` row. `pci_delta.py`'s classification and the git-pinned notes are
regenerated at S0 and again at S6 (EG-1).

### 5. Type rules

None.

### 6. Error codes

No new code. [REVISED: was "the existing usage/refusal code the sealed runner already uses for a malformed
sealed invocation", reason: VmPin-6.] Exit 2: the code `axon test` uses for a resolve, check or type refusal
(including E0004) and R50 §3's invalid-`AXON_ENGINE` code. The message goes to stderr only and is emitted before
any NDJSON line, so stdout never contains a `{"type":"summary"…}` line. Consumers fail closed: psv maps it to
Unknown, cortex to Err ("no summary"). T1 asserts all three.

### 7. Invariants touched

- **I-2 (tree-walker is the reference):** unchanged and strengthened — sealed runs are reference-engine runs.
- **PSV-1 runtime seal:** must hold on the unified tree exactly as narrowed on v0.22 (claim and NON-CLAIMS in
  `v022-protected-suite-verdict.md`); no new non-claims may be introduced by the port without a stated reason.
- **R50 parity and perf:** [REVISED: was "unsealed behaviour and cost unchanged", reason: S0Merge-3.] unsealed
  cost within §10; unsealed behaviour unchanged except the bug fixes listed in §3, which hold on both engines.
- **No sealed run executes unprotected at any commit.** From the S0 merge commit on, `SEAL_RUNTIME_COMPLETE =
  false` refuses every sealed run; it flips to `true` only in S6, with T3-T8 green at that commit.
  [REVISED: added, reason: S0Merge-1, VmPin-2.]

### 8. Test plan

- **T1 engine pin** [REVISED: was "`AXON_ENGINE=vm` refuses; unset runs on the tree (`AXON_VM_TRACE=1`
  prints nothing); a drift test …", reason: VmPin-7, -8, -9, -10]:
  - T1a pure truth table (unit, no env mutation): `engine_for(sealed, complete, override, env)` over sealed
    {false, true} × complete {false, true} × override {none, Tree, Vm} × `AXON_ENGINE` {unset, tree, vm,
    garbage}. Sealed rows are only `Ok(Tree)` (complete, no VM requested) or a refusal; unset → Tree.
    `AXON_VM_EAGER` is not an input.
  - T1b library: after a sealed `run_test_fn` every `fn_table[i].compiled` cell and every resolved lambda body
    cell is empty; `compile` panics if called under seal.
  - T1c CLI: refusal message on stderr, exit 2, no summary line on stdout; with `env -i` (the production
    shape) a sealed run produces no `AXON_VM_TRACE=1` lines.
  - T1d drift: the set of functions that read `.engine` equals {`run_body`, `run_test_body`, `run_lambda`,
    `fold_leaf`, the trace read}; `vm/tests.rs` is the only writer of `.engine` after build; the callers of `compile` are exactly
    `compile_entry` and `compile_lambda`; every caller of a VM entry point outside `vm/` is in the allow-list
    {`call_fn_common`, `call_fn_frame`, `call_closure_frame`, builtins `arr_fold`} [REVISED: was `call_fn_in_general`,
    `call_closure_owned_by`; S2.3 step 4 moved their bodies into the const-generic frames].
  - Sealed-state tests hold one lock that every `Interp::build` in the test binary respects (or the seal state
    is a build parameter), so a concurrent unrelated test never sees `seal.active`.
- **T2 no regression** [REVISED: was "pass unchanged", reason: S0Merge-6, EG-6, EG-8]: `vm_parity.sh`,
  `vm_wasm_depth.sh`, `vm_perf_gate.sh` (+ `--repros`) pass after conversion to the merged binary rule, with
  identical corpus, tolerances and verdicts; `nest_cost` constants may be re-measured, each change stated
  old/new bytes (the table is in §10 "Stack" [REVISED: added, reason: S7-Evidence-2]), required depth ratios
  unchanged. `cargo test -p axon-core --no-default-features --test
  cli_run` passes with `CARGO_TARGET_DIR` unset (in-tree `target/`; six wasm harnesses hardcode it).
- **T3 v0.22 interpreter suites** [REVISED: was "v0.22's `taint_tests.rs` ported", reason: EG-5]: every
  v0.22-only axon-core test (144 lib tests: interp::tests 96, interp::taint_tests 39, resolver 5, seal_split,
  parser, kernel, conform 1 each; and the integration files axon_test_outcomes, dispatch_rule_switch_is_test_only,
  harness_binaries, harness_integrity, pci_delta_note, pci_isolation, postc9_spec, psv_test_selection,
  refusal_coverage_gate, test_completion) is ported under the same name and passes;
  `scripts/v022_pci_gates.sh` PASS ([REVISED: was "93 rows", reason: S7-Evidence-5: a row for v0.22 M3343's
  goal_eval no-such-name text killer] 94 rows). Drift tests are re-derived for main's identifiers, sweep
  `interp/sym.rs` and `interp/vm/*.rs` (except `vm/tests.rs`) with a cut that matches `mod tests {` only, and
  each asserts a minimum number of matched sites (TaintPort-9, VmPin-10).
- **T4 oracle twin** [REVISED: wording, reason: EG-10]: `sealed_frames::a_sealed_caller_is_refused_in_the_same_
  words_for_an_operator_name_and_a_missing_one` ([REVISED: was "78 forms × 8 placements × 2 = 1248", reason:
  S7-Evidence-7: R51 adds the form `goal_eval name of a &mut fn`; the test now asserts 79 forms and 8
  placements] 79 forms × 8 placements × 2 = 1264 runner `check` calls, static
  layer) and `every_position_the_sealed_walk_visits_has_a_twin_row` pass. The match-guard visit is re-applied to
  main's iterative `ast::children` with veto's guard test (TaintPort-13, S0Merge-10).
- **T5 receiver PICK** [REVISED: added the Fails control and liveness, reason: EG-11]:
  `interp::taint_tests::an_operator_value_the_candidate_picked_is_never_dispatched_on` (11 Ok, 1 Fails, 19
  Refused, under `TaintOnly` and `Both`, attacks live with rules off) and
  `sealed_frames::an_operator_value_the_candidate_picked_is_refused_through_the_runner`.
- **T6 runner legs:** axon-psv `sealed_frames` and `runner`, axon-fabric `psv_dispatch`, `check_effects`,
  `attestation`, `cortex_via_fabric`, `observer_service` green. [REVISED: added, reason: VmPin-7.] Plus one leg
  each through the psv runner (`env_clear`) and cortex `with_clean_env` asserting a tree run.
- **T7 mutation evidence** [REVISED: mechanics stated, reason: EG-2, EG-4, EG-12]: the 426 re-anchored rows run
  with `--scope=all --only=<ids>` in ≤3 lists at the S6 head on a clean committed tree, each OUT.json committed
  under `governance/status/` with its commit; every row KILLED by its own attack, 0 REFUSED_ELSEWHERE, 0 stale; a
  `baseline=error` row is re-run alone and reported. Every one of the 2281 active rows' `old` text occurs exactly
  once in the merged tree, or the row is in the S6 re-anchor list. New rows + attack markers for the pin refusal,
  the `compile` refusal, each engine gate's consult and the `general_calls` seal branch, in functions named so
  `SEAL_FN` selects them (`seal_engine_pin`, …). Refusal-coverage gate plain and `--freeze` exit 0 with REGIONS /
  EXEMPT / SCOPE_FN_REGIONS re-anchored and the new REMAINDER recorded. `pci_delta.py --check` PASS after every
  main-side commit under `crates/axon-core/src` (82 at 5864c423) and every R51 commit is classified (EG-1).
- **T8 attack classes new to main's model** [REVISED: was "a candidate keeps an alias to an operator
  struct/closure … mutates it", which cannot happen under COW, reason: TaintPort-12]: refused, with an honest
  control each:
  1. PICK laundering by identity change: cast re-wrap, `SendValue` round-trip, `fn_value` forwarder; including a
     candidate PICKing an operator closure and passing it through a `fn(..)`-typed parameter that the operator
     then calls.
  2. Taint-vector desync at each former raw `vars` write (positional probe: tainted binding, then untainted, used
     at a sink).
  3. Fresh predicate envs keep the candidate value's taint.
  4. Handler-arm and continuation snapshots keep taint.
  5. AX-55/AX-31 in-place reads and writes (`operand`, `index_in_place`, `field_in_place`,
     `assign_in_place(_shaped)`, `write_place`) with a tainted local at a width/dispatch/name sink.
  6. Lend path: capture taints move into the frame and back.
  7. Frame pool reuse carries no stale taint and never returns a store-less frame in a sealed run.
  8. Name-scan fallback reads the taint of the same binding.
  9. The C9 round 9 global holes (`TABLE[0]`, `CFG.k`, `PAIR.0`, closure-constant call) on main's fast paths.
- **T9 non-seal fixes on both engines** [REVISED: added, reason: S0Merge-3]: loop control escaping a fn is
  contained, a test passes only if its body completed, `Uncertain` provenance keeps the least-trusted tag; each
  under `AXON_ENGINE=tree` and `vm`, including through `call_fast`/`CallFastLocalInt`, with `vm_parity.sh` corpus
  entries.

### 9. Acceptance criteria

[REVISED: was "All of §8 green at one commit; a strict gate … CITABLE at that commit; one confirmation review
round … with zero blockers", reason: EG-9, EG-13.] At head H:

- a CITABLE strict receipt: `scripts/run_managed.sh start <name> --snapshot H --mem-max 40G --deadline 18000 --
  ./scripts/gate.sh --strict`, `run_managed.sh verify <rundir> --for H`, `scripts/release_check.sh H`;
- separate receipts at H for T2 (the four commands) and T7 (mutation OUT.json files with `commit == H`, refusal
  coverage `--freeze`), all listed in §14 by path;
- the guest boot leg, `b263_qualify`, MiCode gates and Android are SKIPs on gpumaster: stated non-results, not
  passes;
- one confirmation review round on PSV-1, PSV-2 and SENTINEL against the ported code with zero blockers. It is a
  non-certifying engineering review; no freeze manifest, signature or guest boot is produced or claimed
  (ADR-003).

### 10. Performance budget

[REVISED: was "no measurable change … within the gate's existing tolerance", which has no tolerance to measure,
reason: EG-7.] Unsealed: each of `vm_perf_gate.sh`'s five medians and each `--repros` row ≤ 1.01 × its value at
5864c423 (instructions:u, 3 runs): fib-recursive 2,298,930,456; collatz 18,580,211,600; mandelbrot
5,068,914,361; arr-sum 4,879,384,240; qsort 11,401,042,655; repros loop 92.2, call 244.1, part 218.1, fold 58.0,
fastcall 244.1, mutcall 274.2, foldmod 95.0, swap 258.3. Sealed: tree-engine speed (today's v0.22 cost); no
target; suite-verdict wall time recorded at S6 (Q1).

**Stack** [REVISED: added, reason: S7-Evidence-2, S7-UnsealedParity-10.] R51 grew the unsealed instantiation's
frames (the per-run-kind evaluator, `f<const T: bool>`), so Q5's "frame sizes stay unchanged" did not hold. The
decision taken:

- *wasm32* (T2, ffe2b413 + 3561022b): every `nest_cost` constant equals the frame need `wasm_stack_budget.py`
  measures; no constant is below its frame; `NEST_PER_DEPTH` (3584), the wasm guard (128), the
  `vm_wasm_depth.sh` corpus and its thresholds (1.2× the guard, VM ≥ tree) are unchanged. Unsealed constants
  grew by at most 48 bytes each. Old → new, `debug/release` bytes (`git diff 5864c423 3921a309 --
  crates/axon-core/src/interp.rs`):

  | Constant | 5864c423 | R51 |
  |---|---|---|
  | `EVAL_ARM` (was `EVAL`) | 192/288 | 192/304 |
  | `CALL_FN_FRAME` (was `CALL_FN_IN_GENERAL`) | 224/528 | 272/576 |
  | `CALL_CLOSURE_FRAME` (was `CALL_CLOSURE_OWNED_BY`) | 256/336 | 256/336 |
  | `RUN_TEST_BODY` (new; R51's `@[test]` body runner, which charged `RUN_BODY_TREE` before T2) | — | `RUN_BODY_VM + RUN` |
  | `EVAL_PROB_PRED` (new; moved out of `eval_call`'s frame) | — | 208/224 |
  | `BIND_LET` | 128/208 | 160/224 |
  | `CALL_BUILTIN` | 496/704 | 496/720 |
  | `CALL_FAST` | 208/288 | 224/288 |
  | `CALL_FAST_ARG` | 208/272 | 208/288 |
  | `CALL_FAST_INLINE` | 256/304 | 256/320 |
  | `CALL_FN_ENTRY` | 112/192 | 112/224 |
  | `CALL_FN_GOAL` | 464/928 | 480/928 |
  | `CALL_FN_IN` | 160/176 | 160/208 |
  | `CALL_MUT` | 160/320 | 208/320 |
  | `CALL_MUT_GENERAL` | 176/288 | 176/304 |
  | `CALL_MUT_OP` | 288/416 | 336/432 |
  | `DISPATCH_RESUME` | 224/176 | 224/208 |
  | `EVAL_NATIVE_CALL` | 240/320 | 256/320 |
  | `FINISH_RECORD` | 160/384 | 160/432 |
  | `GOAL_EVAL_HOLDOUT` | 112/400 | 128/400 |
  | `RUN_GOAL` | 672/1008 | 672/1024 |
  | `RUN_GOAL_MULTISTART` | 640/1152 | 672/1152 |
  | `RUN_GOAL_WARM` | 656/1024 | 656/1040 |

  Every other constant is unchanged. Sealed-only (new, charged only by a sealed run's instances; `0` where that
  profile inlines the function): `EVAL_ARM_SEALED` 336/656, `EVAL_BLOCK_SEALED` 128/0, `EVAL_BINOP_SEALED`
  336/0, `EVAL_CALL_SEALED` 400/0, `DISPATCH_CALL_SEALED` 176/304, `DISPATCH_NAMED_SEALED` 176/0,
  `BIND_LET_SEALED` 160/256, `RUN_LOOP_BODY_SEALED` 112/128, `CHAN_METHOD_SEALED` 128/112,
  `ASSIGN_IN_PLACE_SEALED` 176/0, `CALL_FN_FRAME_SEALED` 272/592, `CALL_CLOSURE_FRAME_SEALED` 352/416,
  `IMPL_METHOD_SEALED` 160/256, `EVAL_TAINTED` 144/144, `CALL_FN_SEALED` 288/176, `CALL_MUT_SEALED` 192/384,
  `RUN_ARM_SEALED` 480/144, `STRUCT_REFINE_SEALED` 288/112.

  Reach (`vm_wasm_depth.sh`, deepest completed depth under wasmtime's default stack, tree/VM, 5864c423 → R51):
  debug closure 157/371 → 158/367 (required ≥ 154), plain 238/982 → 242/953, mut 485/833 → 471/774, with
  184/216 → 187/217; release closure 163/258 → 163/260, plain 253/648 → 245/635, mut 373/550 → 369/541, with
  197/209 → 190/205. Nest probes: release tree reach drops 1–7 levels on most
  (refined 126 → 119, with_chain 107 → 102, mut_with3 117/126 → 112/122); debug reach unchanged except 1-level
  drops on break_fast, lambda_chain, lambda_self and mut_with3. Every probe past its reach still ends in the
  recursion-limit panic (exit 101), never a trap; VM ≥ tree on every probe. Receipt: tasks/r51-baseline.md.

  *Merge into `main` (r51/merge-main, 34217115)* [REVISED: added, reason: main gained R50 S10–S13 after R51
  branched.] The merged build's frames differ from both sides; each constant below is again the frame
  `wasm_stack_budget.py` measures on the merged build (none below it; `vm_wasm_depth.sh` PASS debug and release).
  Constants whose merged value is not R51's (every other constant keeps the R51 column above):

  | Constant | main 8de8e43e | R51 75f385b5 | merged 34217115 |
  |---|---|---|---|
  | `RUN` | 336/528 | 336/528 | 336/544 |
  | `CALL_FAST` | 208/320 | 224/288 | 224/320 |
  | `CALL_FAST_ARG` | 208/320 | 208/288 | 224/336 |
  | `CALL_FAST_INLINE` | 256/416 | 256/320 | 240/416 |
  | `CALL_MUT` | 160/320 | 208/320 | 208/336 |
  | `CALL_MUT_OP` | 288/416 | 336/432 | 320/432 |
  | `COMPILE_STMT` | 800/672 | 800/656 | 800/672 |
  | `STORE_LOCAL_STACK_SLOW` | 192/208 | 192/208 | 192/224 |
- *native*: the guard counts calls and the interpreter thread reserves `max_depth × STACK_BYTES_PER_FRAME`
  (floor `MIN_STACK_BYTES`). R51's release `eval_arm::<false>` frame is 1024 B (5864c423's `eval`: 912 B);
  stack per call level grew 3.5–12.3% (release) and 6.1–10.5% (debug) on the tree engine, up to 19.3% on the
  debug VM, and a recursion nested 260–280 expressions deep at `down(5990)` aborted with a stack overflow where
  5864c423 completed it (S7-UnsealedParity-10). f89f4d43 scales the reserve ×1.25 (`STACK_BYTES_PER_FRAME`
  256 → 320 KiB, `MIN_STACK_BYTES` 1 → 1.25 GiB; at the default limit 6000 the thread reserves 1.83 GiB, was
  1.46 GiB), capped (4bb4a0e1) at 5864c423's largest reserve, 1,000,000 × 256 KiB, so `AXON_MAX_DEPTH` near the
  ceiling still spawns its thread (uncapped, ×1.25 asked for more than heuristic overcommit maps past ~800,000).
  It is address space: resident memory is unchanged (max RSS of the same program equal within noise).
  Every call depth × nesting depth cell that completed or hit the recursion limit on 5864c423 does so again,
  release and debug, tree and VM. Nesting deep enough still overflows, on 5864c423 too (tasks/opportunities.md).

### 11. Rollout & rollback

[REVISED: order and contents, reason: S0Merge-1, -2, -3, -4, -6, -7, -9, -11, TaintPort-6, -7, VmPin-2.]
Slices, each a commit (or a short run of commits) that builds and passes its non-deferred tests, on branch
`r51/unify`:

- **S0** merge `c9ed2ddd` (no `--ff`). Take main's `interp.rs` and `interp/{eval,builtins,goal,proptest,value}.rs`;
  do not add veto's `interp/{taint,taint_tests,pin,conform}.rs` yet. The merge resolution itself carries the
  **refuse-all pin** (`SEAL_RUNTIME_COMPLETE = false`) and the `compile` refusal, so no commit ever runs a sealed
  run unprotected. Core conflicts: `ast.rs` iterative walker plus the match-guard visit (and an AX-56 depth case
  for `walk_type_names`), `builtins.rs` union, `error.rs` veto's list, `main.rs` both hunks, `codegen/expr.rs`
  main's, `axon-wasm` doc union. Scripts: binary-selection lines take veto's form (`cargo build …` then `.
  scripts/lib/axon_bin.sh; use_built VAR bin`, or `named_bin`); every other main hunk kept verbatim;
  `vm_parity.sh`/`vm_perf_gate.sh` converted; `AXON_RUN` added to `BINARY_VARS`; test-spawned scripts run
  `Bins::Named` (`AXON`, `AXON_RUN` = this test run's `CARGO_BIN_EXE_*`). `cli_run.rs` hunk-merged (main 176 new
  tests, veto 4), all spawns through `script_spawn::script(…, Bins::Named(…))`. `AXON-COMPLETENESS.json`
  hand-merged, `.md` and `AXON_REFERENCE.md` regenerated; error registry (E0906 main; E0004, E0505 veto) has no
  collision; no requirement renumbering (main adds only R50). Deferred tests carry `#[ignore = "R51 Sn:
  <reason>"]` and are listed in `governance/status/r51-deferred-tests.json`; a drift test fails if the ignore
  set differs from the registry. Classify the 82 main-side commits in `pci_delta.py` THEMES and regenerate the
  note. The real conflict set is 61 files (5 interp, 56 non-interp; `git merge-tree --write-tree origin/main
  c9ed2ddd`). [REVISED: was "registries (renumbered) … LAND15b", reason: S0Merge-9, -11.] Acceptance: `cargo
  check --workspace --all-targets`, fmt, clippy, the non-deferred test set, `harness_binaries`, `parity_all`
  (`AXON_HARNESS_STRICT=1`), `vm_parity.sh`, `vm_wasm_depth.sh`, `vm_perf_gate.sh`. `gate.sh --strict` is first
  required at S6.
- **S1** T1 (a-d) and the CLI courtesy refusal in `cmd_test`.
- **S0b** non-seal interpreter fixes on both engines (veto 4e7851bc, bcf9c0a7, 2175cc1b, 89256bd3, 91c2f676,
  ea9ba38d, 6de01247): `contain_frame`/`contain_loop_control`, real `TestEnd`/`run_test_fn_outcome`,
  `test_body_finished`/`test_frame_depth`, `Uncertain` provenance, `type_name_of` as `pub(crate)`; T9.
- **S5** [REVISED: was "S5 static layer: `pin.rs`, sealed-only check, `Parser::eof_span`, per-item spans,
  resolver sealed set", most of which lands in S0, reason: S0Merge-4, TaintPort-6.] interp-side static layer:
  `interp/pin.rs`, the `Seal` state and frame provenance in `Interp`, `Interp::build` pins, the interp
  consumers of the sealed set (`span_in_sealed` edges); T4.
- **S2** taint channel (Fork 2): `Bindings`, the Env factory, `eval_t<const T>`, `taint.rs` with
  accumulator/pc/sticky taints, `now_ns` class; T3 taint subset, T8.2-8 subset.
- **S3** seal edges, casts, marks, dict snapshots and channel contracts on the Rc'd model (Fork 3), closure
  representation A, `global_ref`, `general_calls`, the main-only sink classification, `interp/conform.rs`, plus
  [REVISED: added, reason: TaintPort-7] the per-frame `Kernel` split (96 field refs through `k()`), sealed RNG
  stream and `kernel.rs` sealed token domain, `no_such_fn` uniform text, handler `with_frame` /
  `handler_may_answer`; T3, T8. [REVISED: added at S2.3 step 4.] The frame-taint readers that are seal edges
  land here with them: `taint.catchable` (veto builtins.rs:474-482, read at the call crossing, interp.rs:3620)
  and the scheduler fiber's sealed-entry sticky; the TYP strip on a parameter or return whose closed type pins
  the cast (`t_pins`, veto interp.rs:4144, 4372); a sealed frame's own `frame_sealed` at `with_frame`, so a fn
  frame's parameters take ALL (until then `t_params` gives ALL only to a closure called from a sealed frame);
  the sealed kernel's provenance store in `t_provenance_write`. S4 adds the PICK mark at frame exit.
- **S4** PICK bit and receiver dispatch rule; T5, T8.1.
- **S6** flip `SEAL_RUNTIME_COMPLETE`; remove the deferred-test registry entries; evidence re-anchor and re-run
  (Fork 4, T7); T6; T2 re-measured; strict gate; §14 filled.
- **S7** confirmation review; fix to zero blockers.

Rollback: before S6, the branch keeps working with the seal absent, and every sealed run is refused (exit 2;
psv Unknown, cortex Err). `v022/veto` remains the reference for v0.22's protected behaviour until S7 passes.
Merging `r51/unify` into `main` and retiring `v022/veto` are operator decisions (needs-human, §12 N1).

### 12. Open questions — resolved

[REVISED: all questions resolved or classified at Step 2.]

- **Q1** Do sealed runs need VM speed? *Engineering, resolved:* no VM for sealed runs in R51; S6 records
  suite-verdict wall time; if a realistic suite is too slow, that is a new spec (logged in
  `tasks/opportunities.md`), not R51 scope.
- **Q2** Can the taint vector be free on the unsealed path? *Engineering, resolved (reframed for the tree path,
  TaintPort-10):* yes by construction — `eval_t<const T>` monomorphisation, unsealed instantiation without
  taint code, VM sites `debug_assert!` that no taint store exists; S2 proves it with `vm_perf_gate.sh` (§10).
- **Q3** Does deferred compile (R50 S9) or `AXON_VM_EAGER` change a sealed run? *Answered (VmPin-11):* no.
  `vm_eager` is read only inside VM-only paths (vm/mod.rs:1322, 1412); nothing compiles at build. T1b/T1d cover
  S9 structurally.
- **Q4** Where does the pin live? *Engineering, resolved:* `Interp::build` (Fork 1).
- **Q5** How does the wasm32 frame budget absorb the seal? *Engineering, resolved (EG-6):* the seal compiles on
  every target; [REVISED: was "the unsealed instantiation's frame sizes stay unchanged", which R51 did not meet,
  reason: S7-Evidence-2] the unsealed instantiation's frames grew (wasm32: at most 48 bytes per `nest_cost`
  constant; native: release evaluator frame 912 → 1024 B), each `nest_cost` change is stated old/new and the
  native thread stack reserve is scaled to match, in §10 "Stack"; depth ratios unchanged.
- **Q6** Closure contract representation? *Engineering, resolved:* option A (Fork 3).
- **Q7** Port veto's non-seal interpreter fixes, which change unsealed behaviour? *Engineering, resolved* ⚠
  close call: yes (S0b). They are bug fixes v0.22 already ratified, and leaving them out would make the unified
  tree fail v0.22's own `test_completion` and loop-control suites.
- **N1** *needs-human:* merging `r51/unify` into `main`, and how `v022/veto` work continuing after c9ed2ddd is
  carried across (re-port or freeze). Recommendation: merge after S7 with a PR; freeze `v022/veto` for
  interpreter changes once S6 lands. Not executed by this build.
- **N2** *needs-human:* a certifying (human/independent) review. Out of scope under ADR-003; not claimed.

### 13. Dependency DAG

[REVISED: was "R50 → S0 → S1 → S2 → S3 → {S4, S5} → S6 → S7", reason: S0Merge-1, TaintPort-6.]

R50 (landed) → S0 (merge + refuse-all pin) → S1 (T1) → S0b → S5 → S2 → S3 → S4 → S6 → S7.

### 14. Evidence ledger

Every receipt below is committed; per-head detail (commands, counts, wall times, infra failures) is in
`tasks/r51-baseline.md`. Base for every comparison: `origin/main` 5864c423 (`cargo test -p axon-core
--no-default-features --no-fail-fast` 1772 passed, 0 failed; `./scripts/gate.sh` PASSED).

| Slice / item | Head | Receipt | Result |
|---|---|---|---|
| S0 + S1 + S0b | d099fba1 | `gate.sh`; axon-core suite | gate PASSED except the 4 stages deferred to S6; 1855 passed, 0 failed |
| S5 | d1a2ed34 | `gate.sh`; suite; `vm_perf_gate.sh` | PASSED (same 4 deferred); 1861/0; fib-recursive +0.92% (inside §10) |
| S2 + S0b-perf | 4bbc1563 / d7fc0031 | `gate.sh`; suite; `vm_perf_gate.sh --repros` | PASSED (same 4 deferred); 1896/0; call/fastcall 232.1 (base 244.1), every median ≤ 1.01× |
| S3 | af91688e | `gate.sh`; suite; psv `sealed_frames`, fabric `check_effects` | PASSED (same 4 deferred); 1979/0 |
| S4 + T3 | 59a72f8d | suite; vm_parity; `pci_delta --check` | 2020/0; 0 differ; PASS |
| S6.1 (seal flip) | 371d3c07 | `gate.sh` (all stages, none deferred) | **gate PASSED**, rc 0, 52 min |
| S6.2 (T6 legs, T7 rows M3484-M3494, audit gaps) | c9266cd0 | suite; axon-psv; axon-cortex; vm_parity | 2183/0; 84/0; 78/0; 0 differ |
| T7 mutation evidence (superseded by the final-head row) | e946f06d | `git show e946f06d:governance/status/r51-t7-list{0,1,2}.json` (`--scope=all --only`, clean tree) | **439/439 KILLED by their own attack**, 0 REFUSED_ELSEWHERE, 0 survivors, 0 stale; every non-retired row's old text occurs once (2294 rows) |
| T2 depth | f3a92b26 | `tasks/r51-s7/stack/vm_wasm_depth.log` | `vm_wasm_depth.sh` PASS rc 0; every nest probe VM ≥ tree |
| §10 perf | f3a92b26 | `tasks/r51-s7/stack/vm_perf_gate.log` | programs and `--repros S1,S2` PASS |
| S6.3 / Q1 sealed suite-verdict wall time | c9266cd0 fixture `tasks/r51-s6/q1` | `run.sh <axon> 5`, medians | sealed (tree) 1.386 s; unsealed VM 0.179 s; unsealed tree 0.466 s. About 3× the unsealed tree, 7.7× the VM. No target (§10); VM speed for sealed runs logged in `tasks/opportunities.md` |
| S7 confirmation review | 3921a309 | `governance/reviews/2026-10-11-r51-S7-{SealSoundness,UnsealedParity,EvidenceAudit}.md` | SealSoundness 0 blockers / 0 major; UnsealedParity 0 / 4 major; EvidenceAudit 0 / 2 major. Every MAJOR fixed or declared in §3: r51/s7decl 9e67b796, r51/s7stack 36396ef2, r51/s7reg 7e9693cb |
| T7 final head | aae0690e | `governance/status/r51-t7-list{0,1,2,3}.json` (`--scope=all --only`, clean tree, one head) | **444/444 KILLED by their own attack**, 0 REFUSED_ELSEWHERE, 0 survivors, 0 stale. 444 = the 442 rows of the e946f06d lists after S7 + M60 and M89, reinstated as active rows (their retired-guard full-suite cells failed on tests only they satisfy) |
| S6 paired-disable | 2b0e7438 / 0f6a92d4 | `governance/status/v022-psv-paired-disable.json` | 58/58 records hold. M58 STALE_REFACTORED -> M59 holds; M3260 holds (full suite SUITE_OK, no skip) after harness_integrity stopped inheriting cargo's leaked `AXON_GIT_SHA`; M60 and M89 moved back to active rows (above) |
| Post-S7 implementation review | 9b15996e | `governance/reviews/2026-10-11-r51-postS7-implementation-review.md` | crates/: 0 blocker / 0 major. Evidence scripts: F1 MAJOR (a rename into crates/axon-core/src passed as a pure join) and F2/F5/F6 fixed on r51/pjfix (merged aae0690e; `pci_delta_note` 11/11 incl. two new refusal cases, each failing with its fix reverted); F3 closed by the single-head T7 row above; F4 NOTE kept (M60's killer checks the E0002 duplicate-impl text, which is the row's property) |
| S6 strict gate + smoke | 0d011f8d | `./scripts/gate.sh --strict` (`CARGO_TARGET_DIR` unset; `tasks/r51-s6/final/gate-strict-0d011f8d.summary`); `tasks/r51-smoke/run.sh <axon> post`; Q1 `run.sh <axon> 5` | **gate PASSED**, rc 0, 30 min, 6554 tests passed / 0 failed; 45 harnesses SKIPPED for missing host prerequisites (MiCode peer, `axonb263` user, guest image, Android NDK; listed in the summary), none counted as a pass. The first strict run (c7e6d98c) failed on this spec's `Reviewed` claim with `evidence: none`, corrected by `truth:` commit 41626fa9. Smoke: 6/6 PASS. Q1 medians: sealed 1.349 s, unsealed VM 0.178 s, unsealed tree 0.445 s |
