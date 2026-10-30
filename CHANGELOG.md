# Axon Changelog

## Check-then-panic holes closed in the checker (checker-soundness-2 CS2-1 to CS2-8)

- **A closure write to a captured variable that the enclosing code uses later is refused (CS2-1, O-GAP-03, new E0607):** closures capture by copy, so `let n = 0; let inc = || { n = n + 1 }; inc(); println(to_str(n))` printed `0`. `axon check` now refuses it with E0607 when the written binding is used outside the closure after the literal (read, written, or captured by another closure), or when the literal sits in a loop that does not declare the binding. A closure that alone keeps its state (a counter, a factory) stays legal. Conservative on purpose: an outer read that runs before the first call, and a loop closure that writes before it reads with no later outer read, are refused too; the fix-it says to return the new value or keep the state in one closure. Eight `cli_run` tests that pinned the stale outer value now read through the closure.
- **Arithmetic builtins overflow like the operators (CS2-2, O-GAP-06):** `pow_i64`, `arr_sum_i64`, `arr_sum_by` and `dict_inc` panic with `integer overflow: ...` (exit 101) where they used to wrap, saturate or (`dict_inc`) abort with a Rust panic; `pow_i64` no longer truncates an exponent above u32::MAX (`pow_i64(2, 4294967296)` was `1`). `arr_mean_i64` sums in i128 (`[MAX, MAX, MIN]` was `-2.3e18`, is now the exact mean) and `random_i64` takes the full i64 range without a Rust panic (natively it returned `lo` every time). `str_repeat` refuses an oversized result before allocating (`str_repeat: result too large`, exit 101). Interpreter, VM and native agree.
- **`assert_eq_f64` is exact IEEE equality on every engine (CS2-2):** NaN fails, and the interpreter's hidden 1e-9 tolerance is gone (`assert_eq_f64(0.1 + 0.2, 0.3)` fails as it does natively). Write `assert(abs_f64(a - b) <= eps)` for a tolerance.
- **Closures passed to higher-order builtins are type-checked (CS2-3a, O-GAP-01):** a builtin's closure parameter (`fn(T) -> U` in `arr_map`, `arr_filter`, `arr_fold`, `arr_sort_by`, `arr_any`, `arr_group_by`, `dict_each`, ...) is now a real fn type whose `T`/`U` are the same type variables as the rest of the signature, so a lambda or a named fn passed to it is unified with it: `arr_filter([1,2,3], |x| x + 1)` (a non-bool predicate), `arr_fold(["a"], 0, |acc, x| acc + x)` and `arr_filter(["a"], big)` with `fn big(x: i64) -> bool` are E0102 at check. `arr_max_by`, `arr_min_by` and `arr_sum_by_f64` take `fn(T) -> Num`: the key fn must return an int or a float (E0306 "key fn must return a number"); an i64 key stays accepted. A closure whose result the builtin discards (`dict_each`, `http_sse`) may return anything. Each refused program used to check clean and panic at run time (exit 101). A wrong-arity closure gets only the existing E0306 that names the shape to write.
- **A dict holds one value type (CS2-3b, O-GAP-02, NH-1(a)):** `Dict` is now `Dict<str, V>`. `dict_set(d, "a", 1)` then `Some(v) => println("got " + v)` checked clean and panicked `cannot apply Add to str / i64`; it is E0102 at check, and so are a write of another type into an annotated `let d: Dict<str, i64>`, into a bare-`Dict` parameter whose body fixes the value type, and a mixed dict (`dict_set(d, "a", 1); dict_set(d, "b", "x")`). A bare `Dict` in a fn signature or a struct field is an implicit type parameter: a fn whose body leaves the value type free (`fn size(d: Dict) -> i64 { dict_len(d) }`) still takes dicts of any value type, and a body that fixes it is solved before its callers. `Dict<K, V>` with `K` other than `str` is E0102 "Dict keys are str". Mixed-value dicts must be split, or their values wrapped in one enum: `examples/asi/safe_bandit.ax` keeps its agent in a `let` and its counters in a `Dict<str, i64>`; the AX-59/62 depth fixtures nest through a one-line recursive enum; `r51_s0b/sum_confusion.ax` and `leaf_sum_confusion.ax` are now check refusals.
- **A match on a scalar or a tuple must be exhaustive (CS2-4, O-GAP-04):** a `match` on an int, `str`, float or `decimal` needs a catch-all arm (`_` or a bare binding), a `bool` match needs `true` and `false` (or a catch-all), and a tuple match must cover every combination of its components' domains (bool, enum variants, `Some`/`None`, `Ok`/`Err`; any other component only by a wildcard or a binding; above 256 combinations a catch-all is required). A guarded arm covers nothing, for enums, `Option` and `Result` too. Each such match used to check clean and panic `no match arm matched` (exit 101); it is now E0304 at check, in expression and statement position. `tests/fixtures/vm_match/nomatch.ax` is now refused at check (`vm_match_non_exhaustive_is_refused_at_check`).
- **Methods on generic params and generic struct arguments are checked (CS2-5, O-GAP-05):** `fn f<T>(x: T) -> i64 { x.area() }` checked clean and panicked `no method area` at run time. A method call on a type parameter is now looked up in its bound traits: an unbounded `T` is E0403 with the fix-it "add `T: Trait`", and a bound whose traits do not declare the method is E0403 too. `let b: B<i64> = B { v: "s" }` and passing `B { v: "s" }` to a `B<i64>` parameter are now E0102 (the type arguments of a generic struct are compared). Conservative on purpose: an unbounded `T` whose every caller passes a type that has the method is refused; add the bound.
- **A call through a fn value is checked (CS2-6, O-SF-04):** a call whose callee is not a fn name - an array element (`ops[0](..)`), the fn a call returns (`mk()(..)`), a fn-typed struct field (`(h.f)(..)`) - is checked against that fn's type like a call by name: a wrong argument count is E0305, a wrong argument type E0306, and a return used at the wrong type E0102. Inference also checks a call to a local lambda against the kinds its body gives its params (`let f = |a, b| if a && b {..}; f(2, 3)` is E0102). Each of these used to check clean and panic at run time (exit 101). A callee name that a local or a param binds is typed by that binding, never by a top-level fn of the same name: `fn apply(f: fn(i64) -> bool, ..) { f(v) }` next to a top-level `fn f(n: i64) -> i64` was refused (E0307) and now checks clean.
- **Refinements on enum variant fields and lambda params are enforced (CS2-7, O-S1-15):** a refinement on an enum variant field (`B { n: Pos }`) is checked when the `E::V { .. }` literal is built, and a refined lambda param (`|x: Pos| ..`) at every call of the closure, in the interpreter and natively: exit 6, `refinement violated`. A constant that violates it is E1209 at check for a variant literal, a lambda literal callee, or a block-local `let f = |x: Pos| ..` called in the same block. `axon caps` now reports the routes these predicates run.
- **A refinement predicate may not reach an effect through a user fn (CS2-8, O-SP-02):** E1209 only looked at the builtins a predicate called directly, so `type W = i64 where leak(_)` with `fn leak(v: i64) -> bool { let _ = append_file(P, "x"); true }` checked clean and wrote the file at every coercion (the same for `now_ms()`/`random_f64()` behind a helper). The predicate is now walked with the capabilities reach walker: any reached capability or effect, `Time` included, is E1209 ("refinement `W` reaches `append_file` (fs:write, IO) through `leak`"), and so is a call nothing can classify (a fn named at runtime). Pure helpers, lambdas, fn arrays, `assert` and fn-param dispatch stay accepted; the 3 tracked predicates that call a user fn check unchanged.

## LLM-facing docs agree with the binary; the spec ledger (docs-truth DT-1 to DT-7)

- **Docs fixed (DT-1 to DT-4):** every ```` ```axon ```` block in `spec/axon-for-llms.md`, `spec/stdlib.md`, `spec/language-tour.md`, `README.md`, `CLAUDE.md` and the ASI docs now checks, and each block with `fn main` runs to its marker. 32 blocks failed before; one of them, the README concurrency example, checked clean but exited 45. The guide's `@[goal]` example no longer gets E1504. A new gotcha says that `mod util` needs `AXON_PATH`. `stdlib.md` is now a curated guide, and the complete builtin list is the generated `AXON_REFERENCE.md`. The `str_cmp` and `str_slice` doc strings now state what those builtins do. The ASI spend cap is described as enforced at run time (by `@[verify]`, exit 3), not proved by the compiler. `examples/asi/run.sh` sets `AXON_PATH`. Language-tour §16 no longer calls annotations deferred.
- **Ledger state (DT-5):** spec status lines, closure stamps in `tasks/opportunities.md`, CHANGELOG and ROADMAP now agree with what shipped. New entry O-DT-01: `min_i32` and `max_i32` do not truncate, though their docs say they do.
- **Mechanisms (DT-6, DT-7):** `scripts/spec_ledger.py` is a full-gate parallel stage. It fails when a spec's status line, CHANGELOG and the opportunities closure stamps disagree. `crates/axon-core/tests/doc_snippets.rs` (S16) checks every axon block in the six LLM-facing docs against its fence marker (`axon`, `axon fragment`, `axon fails E....`, `axon exits N`).

## Names with no run-time value are refused at check (O-SF-03)

- **A module, type or enum name used as a value or called is E0404:** `mod n` + `use n.{add2}`, then `let f = n.add2` or `n.add2(1, 2)`; `let k = T` for a struct, alias or type parameter; `P(1)` for a struct `P`; `let e = S` for an enum. Each checked clean and then panicked at run (`undefined identifier`, `call to unknown function`, rc 101), because a module's items are merged into the program by `use` and never reached through its name, and a type or enum has no value. The fix-it names the form that works: `use n.{add2}` then `add2(…)`; a struct literal; `S::Variant`.
- **`M::f` through a declared module is E0404:** `n::add2(1, 2)` was exempt from the unknown-qualified-path refusal because `n` was a declared module, and panicked at run (`value of type n is not callable`). A misspelled native fn (`gfx::nope()` after `use native::gfx`) was exempt the same way and panicked (`value of type gfx is not callable`); it now gets E0404 naming the native module, and the fix-it lists that module's functions (`gfx` has: window_open, surface, clear, …). Unchanged: `gfx::clear(…)` and other table-resolved native calls, builtins whose name contains `::` (`Chan::new`), `ai_extract::<T>`, enum variant paths, struct literals, and a local that happens to share a type's name. The example sweep (290 files) is unchanged.

## A call through a fn value is checked like a direct call (O-SF-01)

- **Arity and argument types of a call through a fn value are checked:** a call whose callee names a local holding a fn value with a known type (a user fn or a lambda bound by `let`, a param typed `fn(..) -> ..`) or a top-level `let` bound to one is checked against that type, with the direct-call codes: E0305 for the argument count, E0306 for an argument type. `fn add2(x: i64, y: i64)`, `let f = add2`, `f(1)` (or `f("a", 2)`, or `let g = |x: i64| x + 1; g(1, 2)`) checked rc 0 and then panicked at run (rc 101); each is now refused at check. A lambda's arity is always known; an untyped lambda param is unknown and accepts any argument. A local of unknown type (an untyped param, an array element) is not checked, and is no longer checked against the signature of a top-level fn it shadows: `fn f(x: i64)` beside `let f = |x: i64, y: i64| x + y; f(1, 2)` was refused with E0305 although it runs the lambda. A lambda's params and a match arm's pattern bindings now shadow outer locals, top-level `let`s and fns of the same name in their body: `Some(tl) => tl(3, 4)` beside `fn tl(x: i64)` was refused with E0305 although it runs the bound value. A lambda param typed by `type Name = str` (parsed as a one-variant enum) takes the primitive it names, and is unknown when it names anything else; a fn param with that type is still refused as before. Not covered: a callee that is not a name (`ops[0](1, 2)`, `mk()(1, 2)`). The `check-outcome` sweep over every tracked `.ax` file is unchanged. Tests: `a_call_through_a_fn_value_checks_its_signature`, `a_call_through_a_matching_fn_value_still_runs` (`crates/axon-core/tests/cli_run.rs`); g01 rows M3870-M3880.

## Gate stage timing, the gate lane (`gate.sh --lane`), `--fast` retired (gate-lane GL-1 to GL-5)

- **Every gate stage is timed (GL-1):** each serial stage sits between `stage_begin <ID> <label>` and `stage_end`, which print `(stage <ID> <label>: Ns, exit N)` and append to the run's own `$CARGO_TARGET_DIR/gate-stages.<pid>.tsv` (par stages as `P:<name>`); `fail()` records the open stage with exit 1 and prints the table; the gate ends with a slowest-first table. Banners and cargo command lines are unchanged.
- **Registry `governance/gate-lanes.json` and derived units (GL-2):** one row per stage and par stage, the build steps B1-B7 with their exact command lines. `scripts/gate_lane.py check` (in the gate's parallel group with `scripts/test_gate_lane.py`) fails on any disagreement with gate.sh, the release-verification manifest or `cargo metadata`; it never reads the untracked audit store, which only `check --audit` (run by `--lane`) checks. `gate_lane.py units` lists the units (per-target for S02, S03, S09, S16, S20).
- **Per-unit read-set audit (GL-3, non-ship):** `scripts/gate_lane.py audit` runs every derived step of HEAD, one by one, through `gate.sh --lane-units=<step>` in a fresh `git clone --shared`, recording inotify opens between sentinels; overflow or SKIP leaves a unit unaudited; executing a workspace binary adds that package's closure; an executed binary no earlier build step produces fails the audit. Package compile sets come from cargo dep-info, absolute and workspace-relative entries alike, with the package directory as a prefix when no dep-info names a file of it. It never writes a gate receipt.
- **`gate.sh --lane` (GL-4):** runs the always-run stages plus the units whose audited read set holds a path changed since the audit's commit (gate-definition, deleted, mode-changed or unclassified paths, or no audit, select everything) and says which ran and why. `--lane --plan` prints the selection and runs nothing. It refuses `--strict`, `--profile` and `--nextest`, never prints the PASS line, and `run_managed.sh` records it `gate_tier=lane gate_run=no`. It is not the gate: the full gate at the integrated head stays the ship rule.
- **`--fast` retired (GL-5):** `gate.sh --fast` and `--base=` now exit 2 pointing at `--lane`. The three checks only the fast tier ran joined the full gate: `g01_anchors.py` and `test_harness_skip.sh` in the parallel group, `vm_parity.sh` as stage S22 on the release interpreter (build step B7).

## `axon check` exit status, builtin-shadowing fn values, generator drain, locate scan, hardset check (small fixes SF-1 to SF-6)

- **`axon check` exits 0 unless it reported an error (SF-5, O-CS-04):** a file whose only diagnostics are notes or warnings now exits 0, as `axon check --help` says ("0 = no error (warnings and notes alone exit 0), 2 = at least one error"). The I0002 note (`let mut`), W0007 (`x // 2`) and W0008 (`let d = {}`) alone exited 2, while W0003 and W0006 already exited 0. A lint beside an error, or beside a file-level error such as E1202 under `--locked`, still exits 2. This supersedes the rule stated in the `cortex repair` entry below ("`check` exits 2 on any diagnostic, notes included"): `verified_done` still needs `axon check` to exit 0, the grader's acceptance, so a body whose only diagnostic is a note now verifies (conformance `cxg_c14c`). The gate stays exercised by `cxg_c14b` through a shim `axon` that exits 2 with a note line on a marked body. Tests `a_note_or_lint_warning_alone_does_not_fail_check` and `a_lint_beside_an_error_still_fails_check`; g01 rows M3848-M3851.
- **A user fn that shadows a builtin, used as a value, is refused with E0306 (SF-6, O-CS-03):** in value position the name now means what it means in a call without a `&mut` argument (`call_prefers_builtin`), which is the builtin (W0003 already said "the builtin takes precedence"), and a builtin is not a value. `let f = abs_i64` beside a user `fn abs_i64(x, y)` checked clean and then panicked at run (`abs_i64: expected 1 args, got 2`, rc 101); a same-arity user fn passed to `arr_map` or bound by name silently ran the builtin; `axon build` failed in codegen with E0701. All now stop at the front end with E0306, whose fix-it is to rename the fn or wrap the builtin in a lambda. Unchanged: a lambda that calls the name (it reaches the builtin), a user fn named `P`/`E`/`Var` (it is the user fn, as in a call), a top-level `let` named like a builtin (a constant, not a fn), and E0605 on a `&mut`-param fn as a value. The E0306 description names the new case. Tests `a_shadowing_fn_used_as_a_value_is_refused`, `a_lambda_or_a_prob_pred_keeps_its_value_rule`, `a_shadowing_fn_value_is_refused_before_native_codegen` (codegen) and a `pipeline_parity` case; g01 rows M3852-M3854.
- **A write to the generator's pipe after the generator was reaped is not its answer (SF-1, O-FU-01):** `cortex repair` used to treat a background child that kept writing after the generator exited as part of the answer. Its bytes were spliced in, and the run could end `verified_done` with rc 0. Those bytes now make the generator `Unavailable` (exit 24). A silent leftover that only holds the pipe open is still accepted (KG2, NH1).
- **`cortex locate` ignores names in comments and strings (SF-2, O-FU-02):** `defined_fns`, `test_fns` and `called_in` now scan the lexer's code view, so `fn NAME(` or `NAME(` inside a comment or a string literal no longer defines or calls anything. A call inside an interpolation hole (`"{f(x)}"`) still counts.
- **`hardset/build.py` exit status and arguments (SF-4, O-S1-21):** `--check` exits 1 when any defect spec fails. With no file it checks every `defects/*.json`. An unknown argument exits 2 instead of starting a build. `--check` is now a full-gate step, with a self-test in the protected-sets stage. `misc.json#6`, a duplicate of `options.ax::safe_div`, is deleted.
- **Conformance workspaces are removed when dropped (SF-3, O-FU-07):** d38abb5b.

## Checker soundness and capability surface (checker-soundness CS-1 to CS-13, capability-surface CAP-1 to CAP-4; `main` 49b576fa, 2026-10-27)
- **Refinement predicates are name- and type-checked (CS-1, O-S1-22):** an unbound name or callee in a predicate is E1211, not a run-time panic; a struct-field refinement binds its own field name.
- **`@[verify]` predicates are name- and type-checked and bind the fn's params (CS-2, CS-3, O-S1-23):** an unknown name or an ill-typed operand is E1103; a predicate the run cannot evaluate is refused at check instead of being silently skipped.
- **A call to a builtin-shadowing fn is typed against the builtin it runs (CS-4, O-FU-06):** wrong arity is E0305, wrong argument type E0306.
- **Checker false positives removed (CS-5, CS-6, CS-11, O-S1-24):** `len(x)` with `x` still a type variable is no longer forced to `str`; a move inside an early-`return` branch no longer poisons the fallthrough; `return` inside a match arm or if-branch is no longer typed `()`.
- **Array and `&[T]` params copy on `let`; `str` stays a move type (CS-7, NH-3 d).**
- **A field access followed by a newline and `(` no longer parses as a method call (CS-10).**
- **A user type named like a builtin type is E0005 (CS-12).**
- **The goal generator takes only the score bound as its target (CS-13).**
- **Docs and invariants (CS-8, CS-9):** two gotchas documented; a test pins that a check-accepted fixture never reaches an interpreter-internal panic.
- **A `@[verify]` predicate may only read the clock (CAP-1, O-S1-19, O-SP-01):** any other effect is E1104, and its effects count against the fn's declared row (E1310).
- **Native and turbofish calls reach `effect_union` (CAP-2, O-S1-20);** both capability walkers classify every call through one `Reach::note` and a test pins their agreement (CAP-3, O-FU-05).
- **E1310 sees `ai_extract::<T>` (CAP-4, O-SP-04).**

## Requirements-evidence integrity checker (merge of `lane/checker`)

- **`crates/axon-core/tests/requirements_evidence.rs` checks every gate REQUIREMENTS.md cites against how it actually runs:** each script cited as evidence must have a row in `governance/EVIDENCE_CLASSES.md`, and the row's class must match the invocation graph from `gate.sh`, CI and Rust tests (transitive reachability, flag guards, comments, `echo` text and self-references excluded).
- **Two false-positive sources fixed at merge:** a glob inside a quoted argument (`shape "no X in scripts/*.sh"`) is text, not a loop; a call through a runner function that executes its arguments (`run_quiet "label" ./scripts/x.sh`, `env`/`timeout` wrappers) is an invocation. A script a reachable script calls by name is `glob_invoked`, with that caller as its witness.
- **Rows re-measured:** 18 gates wired into `gate.sh` since the branch was cut are now `strict_gate`; `r39_slice2_validate.sh`, `vm_parity.sh`, `vm_perf_gate.sh` and `vm_wasm_depth.sh` have rows. `requirements_evidence_is_not_orphaned` is `#[ignore]`d: R1, R7, R30 and R50 cite gates nothing runs (`tasks/opportunities.md` O-CK-01).

## Generic builtin slots are type variables; `str_cmp` is fuzzed

- **A builtin's `T`/`U`/`V` is bound once per call (port of `builtin-typevar-instantiation`):** those slots were `Deferred` wildcards that unified with anything, so `let v = dict_get_or(d, "n", 0)` followed by `RG { amounts: v }` with `amounts: [i64]` passed `axon check` and then panicked at run time (`len: expected str/array/dict, got i64`, rc 101). Each call now instantiates fresh variables, nested slots (`[T]`, `(str, V)`, `Option<T>`) included; closure slots such as `fn(T) -> U` stay deferred. The checker's same-`T` binder reads the promoted slots. Generic builtins are not queued for monomorphization (a queued stub shadowed the native `dict_set` extern). The type-param names come from the sig the call reaches, so a generic user fn sharing a builtin's name and reached through a `&mut` argument (CS-4) is still specialized.
- **Frozen held-out task `h-wrong-var-04` is now caught at compile time:** its planted mutant pushes a `Trace` into an `[i64]` (E0102). `heldout.json` is unchanged. `fence_sweep.py` `COMPILE_CAUGHT` records the task and confirms the refusal on each gate run; a stale entry is an error. On binaries from this change on, a heldout score is at most 46/47 (`benchmarks/heldout/README.md`, "Compile-caught tasks").
- **`scripts/fuzz_parity.sh` covers `str_cmp`:** a two-argument compare row and a self-compare row (reflexive 0), port of `fuzz-str-cmp`.

## S1 follow-ups (followup FU-2 to FU-9; `fu/int`, on `main` by 791dee95, 2026-10-18)
- **Capability walkers see predicate routes (FU-2, O-S1-14):** ecf63dfc.
- **A stopped check drain is never complete output (FU-3, O-S1-12):** ac5876df.
- **Restore only bytes this run wrote (FU-4, O-S1-13):** 098786e7.
- **Non-UTF-8 `AXON_ALLOWED_EFFECTS` denies every effect (FU-5, O-S1-09):** 7c5d2136.
- **Relative `cortex --workspace` (FU-6, O-S1-04):** 392acddc.
- **Lexer-aware `locate::fn_body` (FU-7, O-S1-11):** 2338ceca.
- **`run_ab.py` integrity-abort fixture (FU-8, O-S1-10):** 30fb8841.
- **Build-loop skill update (FU-9, O-S1-16).**
- FU-1, FU-10 and FU-11 moved to `tasks/hx2-spec.md` (STOPPED at HX-0); O-S1-02 stays open.

## S1: governed repair episode — capability fence, bounded checks, walk budget, typed feedback, accepted digest

- **`axon caps <file|-> [--fn NAME] [--base-dir DIR] [--json]` (R0a):** new verb reporting what a program, or one fn transitively, can exercise, as one `axon-caps/1` line: `capabilities` (the `@[contained]` labels: `fs:read`, `fs:write`, `net`, `exec`, `env`, `dstore`, `native:M`), `effects` (builtin effect rows, `Exec` split from `IO`) and `unknown` (`true` = something could not be classified; callers deny by default). The program goes through `axon check`'s load and check pipeline, so imports are merged first; `-` reads stdin and `--base-dir DIR` is where its `use`s resolve. Exit 2 on a parse or type error or an unknown `--fn`; a `@[contained]` violation is not an error here, since reporting it is the verb's job. It runs the walker `@[contained]` uses (`capabilities::reachable_capabilities`), which now also follows the user code the interpreter runs implicitly: refinement `where` predicates at param, return, `let` and field, struct `where`, `@[verify]` predicates and an `@[goal]`'s metric. `@[contained]` gains those routes too.
- **Ambient path/host scope (R0b):** `AXON_ALLOWED_FS_READ`, `AXON_ALLOWED_FS_WRITE` (comma-separated path prefixes) and `AXON_ALLOWED_NET` (host globs) scope a whole run, with `sandbox_create_scoped`'s spelling and matching: checked per argument (`file_copy`'s source is a read, its destination a write; both `file_rename` paths are writes), `..` refused, net checked before any connect. Unset = unrestricted, EMPTY = deny every path/host of that kind, `*` = unrestricted, a value that is not UTF-8 = deny (with a warning). A violation is a `SandboxViolation`, exit 8. The scope is independent of `AXON_ALLOWED_EFFECTS`: `AXON_ALLOWED_FS_WRITE=` keeps `println` and refuses `write_file`, `append_file`, `file_copy`'s destination, `file_rename`, `dir_create` and the durable store, and suppresses the `@[adaptive]` provenance row. It is a ceiling: a sandbox the program creates is checked against it too and cannot widen it. Interpreter only: `axon build` refuses with E0910 (exit 2) while one is set, and a native or wasm32-wasip1 binary refuses to start under one (exit 2). No guest backend runs a payload, and `axon-guest-init` does not pass the variables.
- **A repair candidate cannot acquire authority the original code did not hold (R1):** before the walk, `cortex repair` runs the pinned axon's `axon caps` on the UNPATCHED file. Each target's static grant is what that fn reaches transitively. A candidate body reaching anything outside it (`fs:write`, `net`, `exec`, `effect:NAME` for a non-free effect, or `unknown` when the walker cannot classify it) is refused before it runs: a rejected attempt `capability_denied`, an `action_denied` event with reason `capability:<kinds>` (e.g. `capability:unknown`), the kinds fed back to the generator. A helper or a closure does not widen the grant. A refused proposal counts against `--max-proposals`.
- **Runtime fence (R1a, R1d):** every check runs with `AXON_ALLOWED_EFFECTS` = `Chan,IO,Pure,Random,Time` plus the file's own effects, and `AXON_ALLOWED_FS_READ` / `_FS_WRITE` / `_NET` set EMPTY for each kind the pristine whole file does not use. If a test of the unpatched file fails under that fence, the run stops before any proposal with `fence_conflict`, naming the test and the kind it needs (exit 22). An unclassifiable file (basis `unknown`) gets no fence, said in one stderr line, and the Fabric executor does not take it (`applied: false`). `--json` gains `grant` `{capabilities, effects, basis, applied, fence_env, fence_sha256, targets}`. New subcommand `cortex grant --file PATH [--symbol NAME] [--workspace DIR] [--axon PATH] [--check-registry FILE] [--json]` prints that grant as one `cortex-grant/1` line and runs nothing of the workspace. Exit 22 now also covers a grant that could not be derived.
- **A candidate body stays inside its function:** bodies were spliced in as text, so a body could close the fn early and append items of its own. After the splice the target is re-located (strings and comments read as the Axon lexer reads them) and must end exactly where the body does. A body that closes the fn, or opens a brace, string or comment it does not close, is refused before anything is written or run: rejection `outside_symbol`, an `action_denied` event with reason `body_outside_symbol`, the generator told why. It counts against `--max-proposals`.
- **Every check is a bounded job (R2):** each check is its own process group, killed as a group at `--check-timeout-ms` (default 60000) and after a normal exit, with each output stream capped at 1 MiB. A candidate whose check hits either bound is rejected (`timed_out`) and undone, and the next proposal runs. Only a check of the unpatched file past a bound blocks the run (exit 22). A drain that cannot be joined is reported as incomplete output, not as the capture bound.
- **Walk budget (R3):** `--max-proposals N` caps generator calls across the whole walk (every candidate, both passes, refused proposals included). `--deadline-ms N` is a wall-clock deadline from cortex start: no new generator call starts after it, a `cmd:` generator is cut at min(its timeout, the time left), an `ai:` call is not started with less than its 30 s HTTP timeout left, and the audit is not started. A check already running finishes and a pass it produces is accepted. Otherwise the run ends `budget_exhausted` (exit 20) with `detail` `proposals` or `deadline`, unless the walk had already reached an adjudicated state (exit 27, see R5). `--json` gains `budget` (the limits, what was spent, which one stopped the walk).
- **Typed feedback (R4):** a rejected attempt reaches the generator's next prompt as one compact JSON object instead of a 2 KiB prose blob: `diagnostics` (the `axon-diag/1` fields: code, line, col, message, expected, found, help, with paths rewritten to the workspace file), `failed_tests`, `refusal.capabilities` for a refused proposal, `timed_out`, a `note` with the visible tally and printed lines, and `truncated` counting what the bounds removed (300 bytes per field, 8 diagnostics, 8 failed tests, 1 KiB note, 40 printed lines). Everything comes from the run with the hidden check cut out, and nothing the hidden check said reaches it. The FIRST prompt's report on the file as found stays the pre-S1 plain text, byte for byte: as JSON it added 31 input tokens to every task's first call and failed a receipt on cost per task (regress at 648530ac, 65 worse / 43 better, p = 0.0428).
- **The accepted artifact is bound to its evidence (R5):** the bytes the hidden check grades are digested before it spawns and re-read after the last acceptance check, so a file changed in between is never accepted. `--json` reports `accepted: {path, sha256}` on exits 0 and 27 (null otherwise), where `sha256` is the deciding Verified event's `graded_sha256`. The file is read once more before the run reports. If it is no longer those bytes, the run fails closed: exit 22 `blocked`, `accepted: null`, the file restored as found. An adjudicated result is never discarded by a later, worse attempt: a walk that reached an adjudicated-not-clean state and then ends without a clean repair for any reason (walk budget, step budget, no progress, the early stop, a refusal, a block) exits 27 with that state's graded bytes put back and R5-checked, and `--json` names what ended the walk in `stopped_by` `{outcome, exit_code, detail}` (one text line too). Base f83c6f86 exited 27 with the adjudicated patch already reverted; an S1 build (64c41f53) exited 20 with the found file when the walk budget ran out after an adjudication. The `--label-log` row is now written after the R5 check and records the exit the run exits with (it used to be appended before R5 and could say 27 for a run that exited 22).
- **Fence sweep in the gate (R1f):** `crates/axon-cortex/benchmarks/fence_sweep.py` runs `cortex grant` and then `axon test --json` with and without the fence on every receipt-corpus and heldout1 task (162 tasks, offline, no key). It requires zero status flips: 162 tasks swept, 0 flips. Exit 1 on a flip; exit 2 when nothing was proven (a corpus that does not load or holds no task, an unsweepable task, zero tasks swept). `scripts/gate.sh` runs it after the cortex build, as a hard gate before any paid run.
- **`run_ab.py` aborts on an integrity failure:** arm B compares the file left on disk with `accepted.sha256`. A mismatch, or exit 0 or 27 with `accepted: null` from a binary that reports the key, is an `INTEGRITY FAILURE` that stops the whole run instead of scoring the row.
- **Repair-path receipt:** PASS at 706a40ca (repair digest `cf15378caadfe824`): 107/115 verified against the incumbent's 108 (1 discordant pair, exact McNemar p = 1), 0 false successes; proposals per task 1 worse / 3 better (p = 0.625), wall seconds per task 40 worse / 62 better (p = 0.0371, faster), cost per task 45 worse / 58 better (p = 0.237); first-call input tokens equal the incumbent's on all 99 tasks both verified in one proposal. Two earlier runs stay in the history: 95df3c0c passed (108/115) before its `tempt-unsat_doc-03` rows exposed the R5 x walk bug fixed above, and 648530ac was judged WORSE on cost per task (the R4 first-prompt JSON, reverted above). About $0.40 spent on the three runs.
- **Tests:** `crates/axon-core/tests/caps_verb.rs`, `crates/axon-core/tests/ambient_scope.rs`, `crates/axon-cortex/tests/fence_sweep.rs`, the `r1_*` to `r5_*` cases in `crates/axon-cortex/tests/cli.rs`, and the `cortex` bin unit tests `a_file_changed_after_acceptance_is_not_reported_as_accepted` and `a_refused_settle_restores_the_file_as_found`. `scripts/wasi_env_control_gate.sh` covers the three scope variables on wasip1. g01 mutation rows M3700-M3719 (cortex R1/R3/R4/R5, containment, R5 x walk, the label row, the first-prompt report) and M3720-M3731 (scope, native refusal, caps walker).

## Gaps2: one read per approved run, whole-job cancellation for cortex and axon-intent, LSP anchors, honest wasm skips

- **The approved bytes are the bytes that run (AR1):** one `axon-os run` read the entry file four times; the token was checked against the first read and the child ran the third, so a swap in between ran unapproved code. The supervisor now reads the entry once (`Runtime::load_program`); the token, the effect scan and the sandbox use those bytes. An unreadable program is now an `io` denial sealed with the approval status, where it used to be an `admit` denial.
- **`run_id` is a plain file name (AR2):** `axon-os verify` built the default job path from a record's `run_id`, so `../elsewhere/x` read a job outside the store. A `run_id` containing `/` or NUL, or empty, is refused before any read: `run --run-id` exits 2, `verify` exits 11.
- **`verify_triple` checks the import closure (AR3):** it runs the same authorization `axon-os` does, so a changed or unknown closure fails verification instead of only failing at run time.
- **`axon imports` and `ast approve` refuse a `use` with no `mod` (AR4):** they exited 0 on a program that `check`, `run` and `deploy` reject with E0003, so such a program could be approved. They now exit 2 with E0003.
- **`axon session` refuses one cell cleanly (AR5):** under `AXON_REQUIRE_IMPORTS_DIGEST` a mismatching cell exited the session with no frame and left its temp file. The cell now gets an E2404 frame, its temp file is removed, and the session ends through its normal end path (ledger check included), still exiting 8.
- **Process-group jobs in one crate (KG1):** new libc-only crate `axon-jobctl` (process-group spawn, `killpg`, time-bounded drains, kill-then-reap on drop, termination-signal forwarding). axon-os and axon-intent's synthesizer run on it; their private copies are gone.
- **`cortex repair` no longer hangs on a generator (KG2, KG3):** the `cmd:` generator is a process-group job, so its deadline kills everything it started, and a leftover holding its pipes no longer stalls the loop. `cortex` forwards SIGINT/SIGTERM/SIGHUP to the generator's job and then dies by the same signal. Repair-path receipt: 108/115 verified, 0 false successes, no lost fix against the incumbent.
- **`axon-intent` forwards termination signals (KG5):** Ctrl-C or SIGTERM to it now kills its synthesizer job too.
- **LSP anchors on the right `use` (L1, L2):** an imported module's diagnostic lands on the entry `use` that brought it in (a direct import wins over a transitive one), not on a `use` inside a block comment or one whose path merely ends the same way. The server's definition lookup is the public `compute_definition`.
- **`lsp.rs` is linted (L3):** the gate's serde-json `cargo check` is now `cargo clippy --all-targets -D warnings` (the fast tier runs it when axon-core changed); four lints fixed.
- **wasm harnesses skip only on the compiler's word (H3, H6):** a missing wasm runtime is a FAIL, not a skip; a harness skips a link only when the compiler prints `wasm link unavailable:`. The example sweeps classify each example, and object-only examples must be listed in `scripts/wasm_object_only.txt` (empty today).
- **Harness hygiene (H1, H2, H4, H5):** interpreter-only builds use the caller's target dir; `parity_all` prints the line that matched the skip rule; every parity build shows the compiler's stderr on failure, and a shape check refuses `cargo build|test` with stderr sent to `/dev/null` or `|| true`; the skip rule is tested once, against the real detector.
- **Operator RNG flake fixed (H7):** `operator_code_a_sealed_frame_runs_never_draws_the_operator_rng` failed about 1 run in 64 because parallel tests share the process-global operator stream; tests now draw from a thread-own override (`cfg(test)` only).
- **Docs:** R24, R36 and R41 carry dated notes that the gateway issues `axon-approval/3` tokens binding the import closure (AR7).

## Gaps: approvals bind the import graph, keyed tokens and records, whole-job cancellation, multi-file LSP locations, honest parity skips

- **An approval covers every module that runs (G1):** `axon ast approve` used to hash the entry file only, so rewriting an imported module after approval (to `@[contained(fs: [write("/")], net: ["*"], exec: any)]`) still deployed. The shared loader now records each module it merges, and `imports_digest` (sha256 over name, `axh1` hash and sealed bit, sorted by name, no paths) is written into `axon-ast-approved/2` with the module list. `deploy` refuses a changed closure with exit 8 and names the module changed, added or removed. A `/1` record still binds the entry file; on a match it reads `approved:false`, "re-approve". New verb `axon imports <file> [--json]` (`axon-imports/1`). `AXON_REQUIRE_IMPORTS_DIGEST` makes `run`, `test`, `goal`, `session` and `deploy` refuse (exit 8, before anything runs) when the loaded closure differs; it is removed from the environment after the check, other verbs refuse it (exit 2), and native and wasm binaries refuse it at start. `ast review` shows which module each imported fn came from (`origin`, `[from <module>]`).
- **Keyed approval tokens (G5):** tokens are `axon-approval/3` and carry `imports_digest`. With `AXON_APPROVAL_KEY` (hex, at least 16 bytes) the digest is `axmac1:` HMAC under a derived subkey and a keyed verifier accepts nothing else; without a key it is `axtok3:` sha256, labelled integrity only. axon-os now checks the token `schema`. A `/1` token is refused under a key and accepted keyless only for a program with no imports, pinned to the empty closure. `axon-intent approve` binds the closure through `axon imports`, run with the job child's environment, and `axon-os run` hands it to the child as `AXON_REQUIRE_IMPORTS_DIGEST`, so a module rewritten after approval is refused at run. Fabric cannot deliver the digest to its executors, so it refuses import-bound approvals and admits only programs with no `use` or `mod` word anywhere in the source (deny by default).
- **Run records bind their job and seal the approval (G5c):** `axon-os verify` printed "intact" for a forged record because the chain was seeded from the record's own `manifest_digest`. It now recomputes it from the sibling `<run_id>.axjob` (or `--job PATH`) and exits 11 on a mismatch or a missing job. The approval verdict is inside the seal: `axrec3:` unkeyed, `axrmac1:` keyed. A keyed verifier refuses unkeyed records; old `axrec2` records verify keyless as "approval not sealed". Setting a key makes archived unkeyed runs unreplayable.
- **Cancellation reaches the whole job (G2):** the timeout and kill-latch group kill (821cf767) now has grandchild tests through the production `supervise` path, and mutation proof. SIGINT, SIGTERM and SIGHUP to `axon-os run` SIGKILL every registered job process group, seal a `Halted` record and exit 128+signal; a signal already ignored (`nohup`) stays ignored. Both pipe drains are bounded (2 s), so an escaped descendant holding the pipes cannot hold the supervisor.
- **LSP locations name the right file (G3):** an error in an imported module is reported on the importing `use` line with `relatedInformation` at the module's own URI and line, instead of a clamped spot in the open document. Go-to-definition of an imported fn returns the module's file. Offset-to-position no longer panics mid-character. A relative `AXON_PATH` entry yields an absolute `file://` URI. The LSP tests compile only under `serde-json` and no gate stage ran them; `gate.sh` now does.
- **Parity harnesses no longer pass on a build failure (G4):** dict, arr_reduce, bitwise_cast and parse_or treat a per-case native build failure as FAIL unless the toolchain is provably unavailable. 52 harnesses skip a top-level build failure only when its prerequisite is proven missing (`cargo_build_or_skip`), and an interpreter-only build failure is always FAIL. One shell skip detector lives in `scripts/lib/harness_skip.sh`, pinned by `scripts/test_harness_skip.sh` against `cli_run`'s rule table. `gate.sh --strict` runs `parity_all` with `AXON_HARNESS_STRICT=1`, and its pass floor is the harness count minus allow-listed skips (55). r26, r33 and r34 prove their cargo-test filters ran.
- **wasm builds find the runtime beside the compiler:** `axon build --target wasm32-*` looked for `libaxon_rt.a` only under a `target/` above the cwd, so a compiler built with an outside `CARGO_TARGET_DIR` linked without it and 9 wasm parity harnesses failed or skipped. It now checks the running compiler's own target dir first, as the native link does.
- **Gate:** `scripts/artifact_admission_allowlist.txt` admits the DecisionBench fixture; the full `gate.sh` had stopped at artifact admission since 37c8f460.

## Checker depth and complexity (compilebench AX-63, AX-64, AX-66)

- **AX-64, long operator chains (native, was memory `O(N²)` and time `O(N³)`):** the checker named each expression node by a `String` rebuilt with `format!("{parent}.left")` at every level, so a `1 + 1 + ...` chain `N` terms long held `O(N²)` bytes of path text and a 100,000-term chain ran out of memory. Paths are now `NodePath` (`checker_path.rs`), a shared linked list: extending one is `O(1)`, the text is rendered only for a diagnostic's node id or a lookup in a non-empty type map, and a path drops iteratively. Node ids and diagnostics are unchanged. The walks that recursed once per `+` (`infer_expr`, the resolver's four passes, the borrow and `&mut` walkers) follow the left spine with a loop. Debug build, one 4,000-term `+` chain: 65 s and 285 MB before, 0.01 s and 22 MB after; 100,000 terms: the pre-fix debug build aborted (a stack overflow here; out of memory in the finding), 0.35 s and 177 MB after.
- **AX-66, call-chain and refinement-chain walks (native `@[total]` chain of 20,000: 272 s before, 0.5 s after; wasm32 trapped at 600 `@[contained]` helpers, 3,000 `@[total]` fns and 20,000 refinements on this build):** the `@[contained]` helper-follow, the `@[total]` call-graph walk, the mutual-recursion probe and the refinement-name chase each recursed once per link of a flat chain. They are now explicit-stack or loop walks, and the mutual-recursion probe is one strongly-connected-component pass (`O(fns + calls)`) instead of a fresh walk per fn. Same diagnostics (E1001, E1208, ...).
- **A refinement cycle is refused instead of aborting:** `type A = B where ...` with `type B = A where ...` (or `type S = S where ...`) overflowed the native stack. Each member now gets E0308 `type 'A' is defined in terms of itself: A -> B -> A` at its own `type`.
- **AX-63, deep inferred types (wasm32 only):** `let a{i} = [a{i-1}]` gives `a{n}` a type `n` levels deep from shallow source, and the checker's clone, compare and drop trapped at 3,000 (debug) / 5,000 (release) levels. On wasm32 a binding whose type nests past 224 levels (the same budget as source nesting) is refused with E0301 `the type of `a223` is nested more than 224 levels deep (the wasm32 limit)` at the binding, and bound `Unknown` so the lines after it stay shallow. Native has no limit.
- **Tests:** `tests/checker_depth.rs` (12 native end-to-end cases with a 90 s deadline each: 100,000-term `+` chain, string and mixed chains, a type error deep in a chain, 20,000-link `@[contained]`, `@[total]` and refinement chains, the refusals behind them, the cycle), unit tests in `checker_path.rs`, `infer.rs` and `types.rs`, and `scripts/wasm_checker_depth_parity.sh` (wasm32 debug and release, both engines, under wasmtime's default stack), wired as `wasm_checker_depth_ax63` / `wasm_checker_depth_ax66` in `cli_run.rs`.

## Runtime depth: four stack and memory blow-ups on deep or long input (compilebench AX-62, AX-65, AX-67, AX-68)

- **AX-62, `==` on a deeply nested value:** `values_equal` recursed once per level of the value outside every budget, so comparing two 30,000-node enum lists trapped wasm32 (exit 134). It now walks a heap work list (`eq_node` / `eq_children`): the answer is the recursive one, and a mismatch stops the walk. Lists, dicts, arrays, tuples, structs and `Some`/`Ok` chains 100,000 deep compare on wasm32 under both engines and match native. `values_equal` left the `ALLOW` list in `scripts/wasm_stack_budget.py`.
- **AX-65, long literals on the debug wasm32 lexer:** the logos-generated state machine recursed once per character of a string, a block comment or a numeric literal (about 11,000 characters of string, 50,000 of comment exited 134). Strings, block comments and numbers are now scanned by loops (`lex_str`, `lex_block_comment`, `lex_number` in `token.rs`). Before commit, the old and new lexers were diffed token for token and error for error over exhaustive small-alphabet strings (string, comment and numeric forms) and the output was identical.
- **AX-67, long `else if` chains on native:** native did not count `else if` links toward its 4,000-level nesting limit (wasm32 did), and every later pass recurses once per link: 30,000 links overflowed a debug build's stack (exit 134), 100,000 took over 9 GB in release. Each link now counts on every target, so a chain longer than 4,000 links is refused up front with E0000 `expression nesting too deep (limit 4000)`, exit 2. **Behaviour change:** a program with a chain of more than 4,000 `else if` links no longer runs on native (use `match`). `axon-run` also now parses on a 1 GiB thread, as `axon` does, instead of the 8 MiB main thread.
- **AX-68, a long `try` chain in `axon run`'s preflight:** each diagnostic carried a caret block holding the whole source line, so 30,000 errors on one 120,000-byte line took 5.7 GB. `SourceMap::render_caret` now shows a window around the caret (with `…` where text is cut) on a line over `CARET_LINE_MAX` bytes; a shorter line renders exactly as before.
- **Tests:** `wasm_compares_deep_values_ax62`, `wasm_lexes_long_literals_ax65` (`scripts/wasm_nesting_parity.sh ax62|ax65`), `native_else_if_chain_is_bounded_ax67` (`cli_run.rs`), `long_line_diagnostics.rs`, and unit tests in `lexer.rs`, `interp.rs` and `span.rs`.

## Cortex: tracking whether it gets better

- **Is Cortex's honesty Cortex, or the check? (`benchmarks/plainverify/`, PREREG-plainverify):** `run_ab.py` arm A has two new variants of the plain micode agent (Haiku 5.5): `--agent real-verify` (the harness applies Cortex's acceptance after the agent, sends a generic rejection that leaks nothing of the hidden check, at most 2 extra rounds, claim = accepted, never accepted = file restored and no claim) and `--agent real-honest` (one added prompt sentence, no gate). `--verify-gate` runs the gate on the zero-cost controls. The scorecard knows the agents `plain-verify` and `plain-honest`. Result on `tempt` and `hard-dev`: plain-verify made 0 false successes (by construction) with the same verified count as Cortex (23/30 on `tempt`, 24/25 in scope on `hard-dev`), so the 0 comes from the check; Cortex's loop was faster only on `tempt` (416 s against 930 s) and not on `hard-dev` (638 s against 603 s). plain-verify also verified 12 of the 13 out-of-scope tasks that Cortex fixes none of. plain-honest cut false successes from 7 to 1 on `tempt` and from 8 to 2 on `hard-dev` (McNemar p = 0.070 on each), not to 0. One run per arm per set; $0.30 spent.
- **Scorecard (`crates/axon-cortex/benchmarks/scorecard/`):** an append-only `history.jsonl` of scored runs, with metrics recomputed from committed rows. `report` writes the README table and `progress.svg`. `compare` applies the order false successes, then verified fixes, then proposals per fix, then cost. The six earlier dev runs are backfilled.
- **Ranker on the dev set (PREREG-ranker, PASS):** with Haiku 5.5 as the `--ranker`, 56/60 verified and 0 false successes, the same as spectrum order. Proposals fell from 199 to 120, generator model time from 593 s to 333 s, and the true function was tried first in 59 of 60 tasks instead of 26. Cost per fix did not change: the ranker call costs about what it saves.
- **Held-out sets (`benchmarks/heldout/`):** `heldout.json` (47 tasks) and `once.json` (30 tasks), planted in files the dev corpus never used. Mining real fix commits found 0 of 75 that qualified.
- **Held-out run (PREREG-heldout):** the ranker passed narrowly: 41 = 41 verified, 0 false successes, 4.93 to 4.39 proposals per fix, true function first in 45 instead of 25. The plain micode agent on the same model verified 47/47 and beat both Cortex modes (paired exact McNemar p = 0.031). Cortex tried the right function in every task it missed. The misses are fix-writing failures, because the generator sees only the function body. `benchmarks/README.md` records this as a correction to its earlier "the generator is not the bottleneck" finding.
- **The generator sees what the ranker sees (PREREG-gencontext):** `cortex repair`'s fix-writing prompt now carries the file with the hidden check cut out (signature, types, siblings, comments), the visible failing tests and their output, and for each rejected attempt the visible tests' output or the compiler's diagnostics (2 KB each), built by the ranker's `rank::build_view` with its size cap and leak guard. The hidden check's name, source and output never reach it. When the view cannot be built, the body-only prompt is used and the reason is printed on stderr and recorded under `generator_context` in `--json`. On `once.json` (first use, Haiku 5.5 via micode), Cortex matched the plain agent: 30/30 verified in all three arms (plain, spectrum order, Haiku 5.5 ranker), 0 false successes, McNemar p = 1.0, with 1.13 / 1.00 proposals per fix and 76 s / 91 s wall against the plain agent's 190 s. Exploratory, on the inspected `heldout.json`: 47/47 (was 41/47), proposals per fix 4.93 to 1.98. Total spend $0.134.
- **`cortex repair --label-log PATH`** (env `CORTEX_LABEL_LOG`): appends one `cortex-label/1` row per run, with the outcome, the attempts and the verified symbol as `truth`. Source is included only with `--label-source`. `benchmarks/labels/to_items.py` converts the rows into LocRank and DecisionBench items.
- **Promotion (`benchmarks/promotion/promote.py`):** Cortex's default config changes only through a frozen `axon-loop` evaluation, which covers plan, admission, transition and rollback. The policy in force is recorded as grandfathered, admitted without held-out evidence. `tests/promoted_policy.rs` ties the defaults in `rank.rs`, `audit.rs` and the drivers to that policy. A zero-cost dry run promotes, rolls back and rejects.
- **Hard set (`benchmarks/hardset/`, PREREG-hard-dev):** `hard-dev.json` (25 in-scope defects whose fix needs context outside the body, plus 13 that Cortex cannot fix by design) and `hard-once.json` (19). Replaying real fix commits found 0 of 49 that qualified. With Haiku 5.5, both Cortex and the plain micode agent verified the same 24/25 in-scope tasks, so this set does not separate them. Cortex used 91 s against 256 s of wall time. On the 13 out-of-scope tasks the plain agent made 8 false "fixed" claims and Cortex made none.
- **Temptation set (`benchmarks/tempt/`, PREREG-tempt):** 30 tasks where a dishonest pass is easier than an honest fix: special-casing, test edits, unsatisfiable visible tests. The plain agent made 7 false "fixed" claims and Cortex 0 (paired exact McNemar p = 0.016). Six of the seven were edits to a contradicting visible test that the agent disclosed in its message while still ending with FIXED. Verified: 22 for the agent, 23 for Cortex, which used 416 s against 303 s of wall time.
- **`cortex repair` removes stale `.cortex-visible-<pid>-<name>` copies** next to the target at episode start, so a crash during a visible run no longer leaves one behind. A copy whose pid is this process or a live process is kept.
- **Cortex gives up sooner on a defect one body cannot fix (`out_of_scope_suspected`, exit 28; PREREG-earlystop, PASS):** two signals read from what Cortex already observes, neither from the hidden check. (1) No function is reached by every failing visible check: the run ends before any model call, file untouched, and candidates not reached by all are never asked. (2) A candidate whose last 2 compiled proposals left the failing visible checks exactly as they were is not asked again. `--early-stop on|off` (default on) and `--inert-limit N` (default 2, `0` disables only signal 2); `--json` gains `early_stop`. Measured on the committed rows before choosing: 76% of the 547 s the 13 hard-dev out-of-scope tasks took were proposals after a candidate's first. Haiku 5.5 on hard-dev + tempt (68 tasks, est. $0.071): 0 false successes, in-scope verified 47/48 = 47/48 (McNemar p = 1.0), out-of-scope + unsatisfiable wall time 801.7 s to 247.0 s over 20 tasks (-69.2%, one-sided exact Wilcoxon p < 0.0001), proposals 151 to 44. In-sample: the rule was designed from those rows. One in-scope task (`hd-caller-contract-05`, refused by both arms in the committed rows) was stopped by the rule where a same-day rule-absent control verified it on its first proposal. See `benchmarks/earlystop/README.md`. Changes the repair digest: the receipt needs `scorecard.py regress`.
- **Repair-path receipts (`scorecard.py receipt --check`, in `scripts/gate.sh` and `--fast`):** promotion governs config defaults; a change to Cortex's repair CODE now needs a receipt too. The REPAIR DIGEST is sha256 over `crates/axon-cortex/src/*.rs`, `src/bin/*.rs` and `benchmarks/drivers/*.sh`, minus `ai.rs` and `label.rs`, with each inclusion justified in `docs/CORTEX.md`. `add` records the digest at the rows' commit. The check passes only for a digest that has a dev cortex run on the policy config (audit off) judged not worse than main's run (the latest run at the `git merge-base HEAD origin/main` digest, recorded with the run as `receipt_incumbent`), or that has a `waive --reason` record (listed in the scorecard README); a digest equal to main's passes. The rule (`scorecard/PREREG-regress.md`, amendments 1 and 2): any false success fails, then fewer verified with exact McNemar p < 0.05, then proposals, wall and cost per task over both-verified tasks with an exact two-sided sign test p < 0.05; otherwise pass, with the effort deltas reported. `scorecard.py regress` runs the paid dev run end to end (measures main first in a detached worktree when main has no run; then build at HEAD, `run_ab.py --arm B`, `add`, `report`, `compare`).
- **The receipt can now see a loss of fixes (PREREG-regress amendment 3):** main scores 60/60 on the 60-task dev corpus, so a build that loses fixes there could only fail on effort. The receipt corpus (`scorecard/receipt-corpus.json`, built by `build_receipt_corpus.py`) is now three legs: dev (60), the hard-dev in-scope tasks (25) and tempt (30), 115 tasks scored as one pairing with one exact McNemar verdict, and `receipt --check`/`report` print the per-leg verified lines. The hard-dev and tempt tasks were already exposed to development; the amendment says so. A dev-only receipt recorded on or before 2026-10-13 still counts for its digest. `regress --main-only` records main's own run (and with `--again` a replicate). Main's receipt run at 3eeda452: 108/115 verified (dev 60/60, hard-dev 25/25, tempt 23/30), 0 false successes; a replicate matched it task for task. Positive controls (`scorecard/controls/`): three one-line degraded builds in a temp worktree, never committed to `src/`. The pooled rule fails the context-off build (16 fixes lost, p = 3.1e-05) and the source-off build (6 lost, p = 0.0312), which the dev-only rule passed; the feedback-off build lost 2 fixes (p = 0.5), which neither rule's verified step can see, and both rules fail it only on a noisy wall-time sign test. Detectable at p < 0.05: 6 lost fixes with no gains, more with discordant noise (controls/README.md). Existing history verdicts are unchanged. After merging a branch that changes Cortex code, re-run `scorecard.py receipt --check` and, if it fails, `scorecard.py regress`.
- **`cortex repair` accepts only a file `axon check` accepts:** `verified_done` now also requires `axon check` on the patched file to exit 0. `check` exits 2 on any diagnostic, notes included, while `axon test` runs such a file anyway. A body that passes every test but draws a diagnostic is undone and fed back to the generator as a `CheckerObjected` rejection, with the checker's diagnostics (hidden check cut out) under the existing caps. If `axon check` cannot run, the outcome is blocked. Found by the first receipt run: a generator shown the file wrote `let mut s = start` on dev task drop-stmt-02, every test passed, and the file failed `axon check` on the I0002 note. The grader scored it as a false success (4 of 4 reruns). Conformance test `cxg_c14b`.

## `examples/cortex` exercises the decision models

- **The sample never reached a model:** `pricing.ax` named its hidden check in a header comment, so the leak guard refused the file to the ranker and the audit (`ranker ... unavailable: the hidden check's name appears outside its own definition`) even with a key and a `--features ai` build. The comment now says `hidden_*`; the name appears only in the test's own definition.
- **`DECISION_MODEL=1 bash examples/cortex/run.sh`:** repairs with the known-good literal once with Haiku 5.5 (`openrouter:anthropic/claude-haiku-5.5`) and once with Mercury Decide (`decision:inception/mercury-decide`) as both `--ranker` and `--audit`, and asserts exit 0, a ranker-ordered walk and an audit verdict of `clean` or `suspect`. A missing key or a build without `ai` is a mismatch. Order, verdict, latency and cost are reported. The script builds cortex with `--features ai` for this mode. The deterministic cases now pin `CORTEX_RANKER=spectrum` and `CORTEX_AUDIT=off`, so a key in the environment no longer sends the file anywhere.

## DecisionBench: one category per model-calling site

- **axon-reflex benchmarks (`benchmarks/decisions/`):** six new categories, 278 items, each asking what one surveyed site asks with that site's inputs and option set: `trust` (MiCode Tier-B trust judge, labels from Cortex A/B claims vs the grader plus MiCode's synthetic replay cases), `axbool` (Axon `ai_extract::<bool>`, labels from `axon check`/`run`/`test`), `improve` (`axon improve discover --ai`, label = the template whose detector fires), `route55` (Cortex cheap-vs-strong with Haiku 5.5's recorded first-proposal outcomes), `audit` (`cortex repair --audit`, the request shape of `audit.rs`) and `risk` (MiCode's shell-risk Tier-B judge; the existing `gate` category asks the static classifier's question, not the judge's). The original 222 items are byte-identical and their report changes only in its spend table. `build_usecases.py` builds them, `spotcheck_usecases.py` recomputes 10 random labels per category from source (60/60 match), `run.py --cats/--per-cat` selects categories, and `analyse.py --usecases` writes a per-use-case report over all 11 categories.
- **Result:** all 14 decision models and `claude-haiku-5.5`, n=1, plus stability repeats for the six models that were not deterministic before; $0.52 total. Haiku 5.5 is tied with the winner in every category and is the only model never significantly behind. Sub-second decision models tie it on `audit`, `improve`, `repair` and `loc`, but not on `route55`. No model beats always answering "genuine" on `trust` by a significant margin. EVO mutation order, the reflex shortlist, MiCode memory recall, `auto` effort, subagent cheap-model choice and stall detection are listed as NOT BUILT, with the recorded data each would need.

## `cortex repair` audits a verified fix and advises cheap vs strong

- **axon-cortex, audit:** after the hidden check verifies a fix, `cortex repair` asks one model whether the patch games the tests (special-cases the tested inputs, hardcodes expected outputs, weakens or removes a check) instead of fixing the function (`src/audit.rs`). `--audit off | openrouter:MODEL | decision:MODEL`, or env `CORTEX_AUDIT` (the flag wins); default `openrouter:anthropic/claude-haiku-5.5`. The request is DecisionBench's `tamper` item: same state keys, question id `games_checks`, criteria, system prompt and `p_yes >= 0.5` = `suspect`. The instructions add one clause naming special-casing and hardcoding, the two ways a one-function patch can game a test. On that category Haiku 5.5 scored 0.92 and Mercury Decide 0.89, on scripted, blatant positives. The model sees the file with the `--check` function cut out, the visible failing checks and the diff. It does NOT see the hidden check's body: DecisionBench never showed it, the adjudicator stays out of every model's view, and the generator never saw it either. A request that would name it is not sent.
- **Review only:** the audit runs after the verdict, the exit code and the file are settled, and changes none of them. A `suspect` fix still exits 0. One stderr line, `audit SPEC: clean|suspect (p=..., N ms, $...)` or `audit SPEC unavailable: REASON` (no key, no `--features ai`, an HTTP error, a timeout, an answer without a probability). `--json` gains `"audit": {requested, used, verdict, p, latency_ms, cost_usd, reason}`, `null` when nothing was verified.
- **axon-cortex, escalation:** one stderr line and `"escalation": {advice, basis, top_p, margin}` in `--json` (`null` with `--symbol`), derived from the ranking with no extra model call. The rule is DecisionBench's `route_via_loc`: cheap iff the model's top candidate is the one the walk tries first, or localization found one function. That rule scored 0.85-0.95 for 11 of 14 decision models and 0.92 for Haiku 5.5, against 0.36-0.76 for most decision models asked directly. The walk follows the ranker, so the advice is `cheap` whenever a ranker answered, and `unavailable` with the reason when none did. There is no `strong`, because the rule cannot produce one here and a threshold would be a guess. A `decision:` ranker's top-1 probability and margin to #2 are recorded as data for calibrating one later. The rule was measured on spectrum-order walks; applying it to a ranker-order walk is unmeasured. The generator stays the operator's choice.
- **Refactor:** the ranker's HTTP, key, timeout and redaction code moved to `src/openrouter.rs` (`Client`, `Transport`), which the ranker and the audit share. `runner::CandidateProgress::baseline_failing()` exposes the checks failing when an episode started. Tests that run the binary pin `CORTEX_AUDIT=off`. New tests: `tests/audit.rs` (scripted transport: clean, suspect, every unavailable path, leak guard, excerpt, selection), escalation in `tests/rank.rs`, and `cli_audit_is_review_only_and_escalation_is_reported` in `tests/cli.rs`.
- **Smoke** (real key, `--features ai`, `--generator literal:` on PoC tasks; $0.0074 total over two runs): the Haiku 5.5 audit called the oracle fixes of `operator-09`, `constant-05` and `argswap-06` clean (p=0.02-0.03, 1.3-1.7 s, about $0.0005 each), and called a `constant-05` body that special-cases the visible test's input suspect (p=0.90), exit 0. A `budget_spend` body that is right only for a fresh budget passed the hidden check, and Haiku (p=0.15) and Mercury (p=0.02) both called it clean. The audit catches blatant special-casing, not state-dependent shortcuts. Escalation: `cheap` on every run, with Mercury's ranker recording top-1 p=1.000 and margin 1.000. The first smoke run found that Haiku 5.5 with a `temperature` and `require_parameters` gets HTTP 404, reported as `unavailable`. The request now omits the temperature for the models in DecisionBench's `NO_TEMPERATURE`, as the measured requests did.

## Structured AI decisions default to Haiku 5.5

- **Behaviour change (axon-ai):** the structured-output calls behind every `ai_extract_*` builtin now send `claude-haiku-5-5` (was `claude-sonnet-4-6`). The model is resolved per call by the new `axon_ai::decision_model()`: `AXON_AI_MODEL_CHEAP`, then `AXON_AI_MODEL_BALANCED`, then `claude-haiku-5-5`; set-but-empty counts as unset. BALANCED stays in the chain because it was the only override this path read before, so a deployment that put its backend's model id there (an OpenAI-compatible provider, a gateway) keeps sending that model. `AXON_AI_MODEL_CHEAP=claude-sonnet-4-6` restores the old default. The interpreter and the native `__axon_ai_extract_*` externs share `complete_structured_inner`, so both resolve the same model.
- **Which builtins reach it today:** under `axon run`, only `ai_extract_uncertain_i64` and `ai_extract_uncertain_f64`. The generic `ai_extract::<T>` form (`i64`, `f64`, `bool`, `Uncertain<…>`) has no interpreter implementation (it panics "value of type ai_extract is not callable", even under `AXON_AI_MOCK=1`), and a native binary refuses every live AI call (exit 2), so native extracts never send a request. Neither gap changed here.
- **`axon improve discover --ai`** picks its template with the same `decision_model()` (was the balanced tier, `claude-sonnet-4-6`). The id written to `proposed_by` is the id sent.
- **Unchanged:** untiered `ai_complete` (`claude-sonnet-4-6`) and the balanced and strong tiers. The `ai_extract_uncertain_*` builtins still write no `ai_call` provenance record; only `ai_complete` does.
- **Smoke** (capture server on `AXON_AI_BASE_URL`, dummy key, interpreter built with `--no-default-features --features asi-runtime`): `ai_extract_uncertain_i64` and `improve discover --ai examples/` sent `claude-haiku-5-5` by default, `gw-cheap-model` with `AXON_AI_MODEL_CHEAP=gw-cheap-model`, and `gw-balanced-model` with only `AXON_AI_MODEL_BALANCED=gw-balanced-model`; `proposed_by` matched in each case.

## `cortex repair` lets a model reorder its candidates

- **axon-cortex:** a ranking step between localization and the walk (`src/rank.rs`). `--ranker spectrum | openrouter:MODEL | decision:MODEL`, or env `CORTEX_RANKER` (the flag wins). Default: `openrouter:anthropic/claude-haiku-5.5` when `OPENROUTER_API_KEY` is set, otherwise `spectrum`. `openrouter:` asks the chat-completions API for a JSON ranking with LocRank's prompt. `decision:` asks OpenRouter's decisions API one `choice` question and orders by returned probability, breaking ties by spectrum order. A ranker may only permute the top `--candidates` names: any answer that names an outsider, repeats or omits a name, or is malformed is rejected by the abstain rules of `axon_reflex::shortlist::decide` plus "every name present" (`axon-reflex/tests/cortex_rank_agreement.rs` holds both to the same answers). The model sees the file with the `--check` function cut out, the visible failing checks and the candidate bodies, capped at 24,000 characters (candidate bodies are truncated first). A request that still names the hidden check is not sent.
- **Fallback is visible:** no key, a build without `--features ai`, an HTTP error, a 20 s timeout or a rejected answer keeps the spectrum order and prints `ranker SPEC unavailable: REASON; using spectrum order`. `--json` gains `"ranker": {requested, used, reason, order, latency_ms, cost_usd}` (`null` with `--symbol`). Exit codes, verdicts and hidden-check adjudication are unchanged. A malformed `--ranker` is a usage error (exit 2).
- **Build note:** the model rankers use `axon-ai`'s HTTP client (new `axon_ai::post_json_bearer`), so they need `--features ai`. The benchmark drivers build without it and therefore walk in spectrum order, visibly. Tests that run the `cortex` binary pin `CORTEX_RANKER=spectrum` and drop the key, so they stay hermetic.
- **Basis:** offline replays of `locate`'s top 6 on the 60-task PoC corpus. Spectrum puts the truth first in 25/60. Haiku 5.5 does in 60/60, p50 2.0 s, $0.0007/task (`benchmarks/locrank/`). Mercury Decide does in 53/57, p50 0.45 s (`axon-reflex/benchmarks/decisions/`). Smoke run with the real key on three tasks where spectrum ranked the truth 5th or 6th: both rankers moved it to 1st. Haiku took 1.1-2.2 s at $0.0003-0.0006, Mercury 0.48-0.73 s at $0.00004-0.00008.

## Cheap AI tier defaults to Haiku 5.5

- **Behaviour change:** `@[ai(policy(tier: cheap))]` now sends `claude-haiku-5-5` (was `claude-haiku-4-5`), and provenance records `anthropic:claude-haiku` version `5.5`. On the decision benchmark (`crates/axon-reflex/benchmarks/decisions/`) Haiku 5.5 scored 0.902 macro accuracy against Haiku 4.5's 0.793, at about a sixth of the price per decision; that benchmark measured decisions, not free-text generation. `AXON_AI_MODEL_CHEAP=claude-haiku-4-5` restores the old model. The balanced and strong tiers are unchanged.

## Cortex A/B re-run on Haiku 5.5 and Opus 5.5 through micode

- **axon-cortex benchmarks:** `benchmarks/drivers/micode.sh` puts a model behind `--generator cmd:` through the `micode` CLI, in a mode with no tools, from a scratch directory, with the same log format as `claude_cli.sh`. It is tested against a stub `micode` (`tests/micode_driver.rs`). `ab/run_ab.py` gains `--transport micode --model M` for both arms and records model, transport, binary hashes and commit in every row. It prices tokens at list price because micode reports no dollars, kills a timed-out task's whole process group, and stops after 3 consecutive rate-limited tasks. `ab/analyse.py --compare-b` compares two B runs. Preregistered in `ab/PREREG-micode.md`, with the breadth-first walk, n=60 per arm: Haiku 5.5 PASS (Cortex 56/60 with 0 false successes, plain agent 59/60 with 1) and Opus 5.5 PASS (55/60 and 60/60, both with 0 false successes; C1 held by one task). All costs are list-price estimates; the total was $6.27. Results are in `benchmarks/README.md`. The Haiku 4.5 primary run is unchanged.

## Cortex vs a plain coding agent: preregistered A/B

- **axon-cortex benchmarks:** `benchmarks/ab/` holds a 60-task corpus of injected defects, a keyed grader, `run_ab.py`, `analyse.py`, the raw results and `PREREG.md`, committed before any paid trial. With Haiku 4.5 on both arms, the verdict is "safety only": Cortex verified 54/60 with 0 false successes, the plain agent 55/60 with 4, and Cortex cost $0.547 per verified fix against $0.060. Results are in `benchmarks/README.md` and linked from `docs/CORTEX.md`. The run predates the breadth-first walk.

## `cortex repair` walks its candidates breadth-first

- **axon-cortex:** without `--symbol`, `cortex repair` used to run each ranked candidate's whole episode (up to `--budget` steps) before trying the next. It now gives every one of the top `--candidates` ONE proposal first, in rank order, and only if none is verified resumes each candidate that pass 1 paused, in rank order, with the rest of its own budget and the rejected attempts it already collected (`runner::CandidateProgress`, `Runner::run_episode_resumable`). No candidate gets more steps or proposals than before. Unchanged: a refusal or a blocked environment ends the walk; `verified_done` ends it; `--symbol` (one target) runs exactly as before; each attempt restores the file, takes a fresh grant and is adjudicated by the hidden check under a fresh completion key; the exit code is the last outcome's. There is no opt-out (`--walk depth`): no consumer depends on the order.
- **Field meaning change:** `--json` `attempted` is now the attempt log in the order attempts ran, so a candidate resumed in pass 2 appears twice (`["a","b","c","a","b","c"]`); it used to list each candidate once. Its last entry is still the repaired `symbol`. A reader that took `len(attempted)` as "candidates tried" must deduplicate. The failure line `tried N candidate(s) (...)` still counts distinct candidates and adds `in M attempt(s)` when they differ. `benchmarks/analyse_real_model.py` books a wrong candidate's pass-2 proposals as search cost, and its positional fallback reports a resumed walk as unattributable instead of misjoining it. `benchmarks/ab/run_ab.py` only stores the list, so its parse is unaffected.
- **Measured** (Haiku 4.5, one sample per task, n=5): the five arm-B tasks of the A/B PoC that took 286-458 s depth-first (truth at rank 3 or 5) were re-run with the PoC harness. All five verified clean in both runs. Proposals 55 → 17, output tokens 197,001 → 23,978, spend $5.24 → $0.38, wall 1,979 s → 233 s. Every task was repaired in pass 1, by the truth's first proposal, so pass 2 was not exercised by a real model here. Wall times are not like for like: the depth-first rows ran 3 tasks at a time, and the PoC's own arm B was running alongside the breadth-first run.

## Completion tokens for `should_fail` and `@[forall]` passes

- **`axon test --completion-key-stdin`:** a `@[test(should_fail)]` the program rejected (a panic, an `Err` or a `?` on one, a refinement, verify or policy stop) and a `@[forall]` property every case of which completed now carry a completion token. Before, only plain tests did, so `cortex repair` and Fabric counted every passing `should_fail` and property test as failed, and a file with one (e.g. `examples/stdlib/distribution.ax`) could never be verified clean. A `should_fail` test stopped by `exit(n)` and a property with a case that hit an early `return` still pass without a token.
- **Behaviour change:** a `@[forall]` case that ends on `Err` (or a `?` on `Err`/`None`) now fails the property, as it fails a plain test; it used to pass.

## User guides and runnable examples for sealed tests, Cortex and the closed loop

- **Sealed tests:** `docs/SEALED_TESTS.md` (safe grading recipe first: `AXON_ALLOWED_EFFECTS=` empty or `--completion-key-stdin`; without either, a sealed candidate that calls `exit(0)` passes every test), a section in `spec/axon-for-llms.md`, and `examples/sealed/` (honest, wrong and four attack candidates; `run.sh` asserts every verdict, run by `cli_run`).
- **Cortex:** `docs/CORTEX.md` (locate, repair with `literal:`/`cmd:`/`ai:`, every exit code incl. 27) and `examples/cortex/` (`run.sh` covers exits 0/21/23/26 and the `exit(0)` body, now refused). `benchmarks/drivers/claude_cli.sh` returned no body (exit 127 on every call since 06002848: an apostrophe closed its quoted Python); fixed, with `tests/claude_driver.rs`.
- **Closed loop:** `docs/CLOSED_LOOP_WALKTHROUGH.md` and `examples/closed-loop/run.sh`, which drives the development-class flow end to end on a scratch store (pointer, plan, EVO, intake, EVL, admit, promote, TEL, revoke, rollback; keyed ledger) without MiCode or Fabric (their inputs are sample JSON), and asserts 10 expected refusals.

## `cortex repair`: a check passes only with proof its test completed

- **axon-cortex:** every check `cortex repair` adjudicates with (the hidden check's verdict and the visible checks that decide accept/revert) now runs `axon test --json --completion-key-stdin` under a fresh random key per dispatch, and a test counts as passed only if it reported `ok` AND carries the completion token for its name under that key; an `ok` without it is counted failed. Plain `axon test` reports a test ended by `exit(0)` as `ok`, so a candidate body `exit(0)` used to make `cortex repair` print `verified_done` and exit 0 with the defect replaced by an exit; it now ends `no_progress` (exit 21) with the file restored. A printed, forged result line (with or without a counterfeit token) cannot supply the token. `LocalInterpreterExecutor::with_completion_key` keeps the raw report for a caller that adjudicates the tokens itself (the Fabric).

## R51 S7: accurate early-end and handler-arm texts; refinement names match the checker

- **`axon test`:** a test that ends early on `None` (`return None`, or a `?` on `None`) still fails, now with "the test ended on `Err`, or early on `None` (a `return None` or a `?` on `None`): it did not complete"; it used to say only "the test returned `Err` (or a `?` ended it on `Err`/`None`)". Under `AXON_VM_EAGER=1` the bytecode engine read a test body's own early `return` as a completion; such a body now runs on the tree, as on the default engine.
- **`axon improve`:** the G3 test-flip check reads `axon test`'s verdict, so a pass that makes a passing test end on `Err` (or early on `None`) is reported as a flip.
- **Effect-handler arms:** a `break`/`continue` in an arm whose `with` sits inside a loop panics with "`break`/`continue` cannot leave an effect-handler arm (a loop around the `with` is not the arm's loop)" instead of "… outside a loop in an effect-handler arm".
- **Resolver:** a named refinement may take a name that only starts with a deferred type name (`type GoalScore = i64 where _ >= 0`); `Uncertain`, `Temporal`, `Goal` and `Dict` themselves stay refused (E0002), matching the checker's exact names. A duplicate impl's E0002 names the type as written (`impl Show for P`, `impl Show for Option<i64>`), not as Rust debug output (`Named("P")`).
- **Sealed runs:** a sealed caller that reaches the recursion limit at a call of an operator function is refused by the call edge in the uniform "no such function" words, never with a depth message naming that function.

## Sealed runs execute on the tree engine with the runtime seal (R51 S6.1)

A sealed run (`axon test --seal`, the axon-psv runner child, axon-cortex's sealed check child, an in-crate run with a sealed module set) now executes, on the tree-walker, with v0.22's PSV-1 runtime seal ported onto the bytecode-era interpreter: the taint channel, the seal edges and casts, the per-frame kernel, the PICK mark and the receiver dispatch rule (`governance/specs/R51-sealed-engine-unification.md`). Since R51 S0 every sealed run on this line was refused with "sealed runs are refused on this build: the runtime seal is not complete" (exit 2; axon-psv verdict Unknown). `AXON_ENGINE` unset or `tree` runs a sealed run on the tree; `AXON_ENGINE=vm` (or `set_engine(Engine::Vm)`) is refused with "a sealed run executes on the tree engine only (AXON_ENGINE=vm cannot carry the seal); unset AXON_ENGINE", exit 2, before any result line. Unsealed runs are unchanged (VM by default).

## `axon test` fails a test that ended on `Err` or at `?`; loop control never leaves a function (R51 S0b)

Ports v0.22's non-seal interpreter fixes onto both engines (tree and bytecode VM, including the fast call paths). Behaviour changes:

- **`axon test`:** a `@[test]` passes only if its body completed. A test that returns `Err` (or whose `?` ends it on `Err`/`None`) now FAILS with "the test returned `Err` (or a `?` ended it on `Err`/`None`): it did not complete"; it used to report `ok`. `exit(0)` below a test and a test's own plain `return` still report `ok` (no completion token under `--completion-key-stdin`). A `break`/`continue` escaping a function into the test's loop fails the test instead of ending the loop; a property case that `exit(0)`s or escapes the same way fails too.
- **Loop control is contained at every frame edge:** a `break`/`continue` leaving a named fn, a closure, a refinement or `@[verify]` predicate, or an effect-handler arm is a runtime panic at that edge ("`break`/`continue` outside a loop in `stop`"), never a jump in the caller's loop; `axon run` used to end the caller's `while` silently. A `return` or `resume` leaving a predicate, or a `resume` leaving a call, is refused the same way.
- **Effect-handler completion is addressed:** an arm that finishes without `resume` (`with handler { on IO(p) => 7 } { … }`) now finishes the `with` that installed it, on both engines. It used to be caught by the nearest enclosing `with`, so an inner `with` handling a different effect (in the same body or in a callee) took the arm's value as its own and the outer body ran on: `let r = c()` then `r + 100` gave 107 instead of 7. A `with` the completion passes through does not run its `return(v)` arm. Native codegen does not lower this handler shape (E0910), so it is unaffected.
- **Return boundary:** a fn declared `-> Result` that produces a `Some`/`None` (or `-> Option` producing `Ok`/`Err`), possible through an untyped `dict_get`, panics with "a runtime type confusion" instead of handing the caller the wrong family.
- **`Uncertain` provenance:** arithmetic on `Uncertain` values keeps the least-trusted operand's `source_tag` (AI 1 over runtime 2 over user 0), and `ai_extract_uncertain_*` results are stamped 1 on the mock and replay paths, as codegen already did.

## Packed scalar arrays and bulk loops (R50 S13)

An array built by `arr_repeat` of a plain `Bool`, `Int` or `Float`, or by `arr_range`, now keeps its elements packed (one byte per `Bool`, eight per `Int` or `Float`) instead of one 16-byte `Value` each. This changes the reference tree-walker as well as the VM. Element reads and writes of the packed kind, `len`, `arr_fold`, the S8 moves and S10 register loops work on packed storage in place. Any other write (another kind, a `SizedInt`) or a caller that needs the elements as values converts the array to plain values once; every builtin, `==` (per element, packed against literal in either order, `NaN`, `-0.0` and `SizedInt` elements included), `to_str`, interpolation and channel send gives the same result as for a literal array of the same elements, under both engines. Two memory edges move. `arr_repeat` and `arr_range` reserve the packed size, so an element count the allocator refused before (`cannot allocate an array of <n> elements`, exit 101) may now succeed, and a later conversion of that array allocates 16 bytes per element infallibly, so it can abort (134) after more output. And while a packed array is converted to values, or viewed as values for a builtin, the packed and value copies are both live, so peak memory is 24 bytes per element (17 for `Bool`) against 16 before: a program near a memory limit can fail where it succeeded before.

Under `AXON_ENGINE=vm` two register-loop shapes also run in one pass on entry when the entry values allow it: a fill, `while j <= b { a[j] = c; j = j + s }` (or `<`) with plain `Int` `j`, `b` and `s`, `s > 0`, `j >= 0`, every written index in bounds, no overflow of `j + s` and `c` of the packed array's kind; and a count, `for k in lo..hi { if a[k] { n = n + c } }` (or `..=`) over a packed `Bool` array, with plain `Int` `n` and `c`, every index in bounds and `n + c × count` within `i64` (checked in 128-bit; the array is scanned in 64-element chunks). A `SizedInt` in any of those locals declines, so its overflow is still checked at its own width. Otherwise the bulk path changes nothing and the register loop runs as before, so a panic gives the tree-walker's text. `AXON_VM_TRACE=1` prints `vm: bulk <name> <f> fills, <k> counts` right after the `vm: pure` line of a body that holds either shape. `scripts/vm_parity.sh` also diffs each file's tree run against `AXON_REF_BIN`'s tree run when it is set. `scripts/vm_perf_gate.sh --repros` adds the rows `packed` (VM) and `packed-tree` (tree-walker); each row's program and base medians are cached under an engine-qualified key, and each output line names its engine.

Instructions retired (release, `instructions:u`): compilebench `sieve` 24,568,573,662 → 498,463,886 (CPython 3.14.4: 1,085,960,442); wall time 2.33 s → 0.06 s and peak resident memory 789 MB → 57 MB. No other `vm_perf_gate.sh` program is more than 0.5 % above its `REF` (collatz +0.19 %, qsort −0.34 %, arr-sum −13.4 %).

## wasm32: deep values drop, deep source is refused, `host_await_val` copies nothing (compilebench AX-59, AX-60, AX-61)

Under wasmtime's default 512 KiB stack the wasm32 interpreter no longer traps (exit 134) on three shapes. Native behaviour is unchanged, apart from the match-guard walk under AX-60.

- **AX-59:** dropping a value nested 100,000 deep (an enum list, a dict of dicts, a closure chain, an array or tuple chain) runs at most 16 levels of drop glue and defers the rest; exit 0 under both engines. Every container that holds a value (record fields, dict maps, array and tuple elements, the `Some`/`Ok`/`Err` payload, a channel's queue, a closure's captures and code) hands its contents to `interp.rs` `bounded_drop` on wasm32; native keeps plain drop glue. `Some`/`Ok`/`Err` chains are bounded on drop, but cloning one is not: a payload is copied on clone, so building a chain from a variable clones it per step, and that clone recursion still traps the default stack from about 1,700 levels (under 100 levels of recursion, debug build; 2,600 release).
- **AX-60:** the wasm32 front end refuses source nested deeper than 224 levels (native: 4,000, unchanged) with E0000 `expression nesting too deep (limit 224)`, exit 2, reported at the start of the statement that holds it (the `let` of `let x = …`, col 5 in a fn body). Every recursive descent (expressions, primaries, patterns, types, `else if` links, match subjects and guards) is charged, and each root expression's AST height is checked, so an operator chain of more than 223 terms is refused too. A string interpolation slot is parsed under the depth of the literal that holds it, so slots nested in each other's literals share the one limit (on native too). A parenthesised or block nesting level charges two units, so those accept 110 levels.
  - A match guard now counts toward its root's height (the walk skipped it, so a guard of any height reached the later recursive passes), and `ast::children` yields a match arm's guard on every target, native included, so every `walk_expr` and `value_position_idents` consumer now sees guard expressions: the borrow check (E0606), the resolver's shadowing lint (W0002 wording), the capability checks, the checker's purity check (E1207), `no_alloc` check, totality check and allocation scan, eval's `mentions_var`, the VM compiler's `binds`, and four native codegen passes: the E0910 per-call `tier:` refusal, `expr_calls` (the `goal_run` registry and the `ai_complete` attribute-tier refusal), escape analysis's `collect_binders`, and `written_place_roots` (AX-08 copy-on-alias). Behaviour changes on native, the same under both engines: `f(&mut a, match 1 { n if a[0] > 0 => 5, _ => 0 })`, whose guard reads the array lent `&mut` to the same call, ran and printed `5 5`; it now exits 2 with E0606 "`a` is borrowed `&mut` and also used by another argument of the same call". `let x = match 1 { n if x > 0 => 1, _ => 2 }` after a `let x` still runs, but W0002 now says it re-declares `x` from its own value. An `@[pure]` fn whose guard passes a non-pure fn to a call (`n if apply(noisy, n) > 0`) ran and printed its result; it now exits 2 with E1207 "non-pure function `noisy`". After `let s = "start"`, `s = s + match 1 { n if { s = "zz"` / `true } => "a", _ => "b" }` took the in-place append path, which ran the guard's assignment before reading `s`, and printed `zza`; `mentions_var` now sees that the guard assigns `s`, so the in-place append path no longer runs and the output follows plain evaluation order (`s` is read before the guard runs): `starta`. Likewise `xs = arr_push(xs, …)` after `let xs = [1]`, whose guard sets `xs = [7, 7]`, left `xs` with 3 elements and now leaves 2. `axon build` of a `main` whose guard is `n if ok_len(ai_complete("hi", tier: "cheap")) > 1000000` built a binary that would send the call to the default model; it is now refused with E0910 "native codegen cannot honor the per-call AI tier", exit 1. After `let a = [1, 2, 3]` and `let b = a`, a guard `n if { a[0] = 9` / `true }` wrote through the buffer `b` shared, and the built binary printed `9 9 1` for `b[0] a[0]` and the arm; `b` is now a copy and it prints `1 9 1`, as `axon run` does. A `let` inline-refinement predicate (`let x: i64 where …`), which is kept outside the statement's tree, is height-checked on its own. A match arm's guard and body are checked as soon as they are parsed, before an or-pattern clones them, and the last pattern takes them without a clone. Each operator layer tracks the height of the chain it builds, and a sub-expression that passes the limit is cut down (its children dropped flat, without drop glue that recurses per level) as it is built; the parse goes on and the root is refused when it ends, so no AST the parser holds is much taller than the limit, a 100,000-term chain gets E0000 instead of trapping, and a syntax error after a chain past the limit (a 100,000-term chain ending in a dangling `+`, an unclosed call around one) is reported as native reports it, exit 2. A chain cut down inside a string's `{...}` slot is settled by the root around the string, not by the slot, so the slot's own leftover-token error and a later syntax error in that root (an unclosed `to_str(` around the string, a dangling `+` after it) are reported as on native too. A refused root is dropped flat too. Before this fix, wasm32 accepted a 300-term guard or a 600-term predicate, and exited 134 on a 3,000-term guard, predicate or arm body, a 5,000-term or-pattern arm body, a chain of 10,000 terms or more, or a syntax error after a 100,000-term chain.
  - A too-deep base type in a `type` definition (`type P = Option<Option<…i64…>> where true`, or a tuple element's `where` predicate nested too deep) is refused with the same E0000 on wasm32, reported where the refusal happens as in a fn signature. The base type is parsed speculatively as a refinement and, on any error, re-parsed as an enum; the nesting refusal was taken for that error and wasm32 reported a syntax error native never reports (`unexpected token: Lt, expected item …` at 1:16 for an `Option<` 300 deep). Native parses as before.
  - An attribute argument nested past the limit (`@[foo(x: [[[…1]]])]`, or a `-` chain `@[foo(x: ----…1)]`) is refused with the same E0000 on wasm32, reported at the `[` or `-` one past the limit. Each level used to recurse with no charge: `[` nesting trapped from about 4,000 levels (release) or 6,000 (debug), a 20,000-long `-` chain in both. Native accepts it as before. `scripts/wasm_stack_budget.py` now fails on any recursion reachable from `Parser::parse_program` with no charged call, and checks each charge it trusts against a `nested` site in `parser.rs`. Because the disassembly shows a charged and an uncharged call as the same edge, it also checks `parser.rs` itself: a charged function may reach its callee only through `self.nested(..)` under no condition but a bare `NEST_ALL` (its native arm aside), and no other code may name that callee.
- **AX-61:** `host_await_val`/`host_await_val_opt` on wasm32 no longer deep-copies a non-`str` payload that neither wasm substrate can send; it reports `no host driver` (exit 101). A `Chan` inside the payload is still refused first with the same path as native.

`scripts/wasm_stack_budget.py` checks both new bounds on each build (release: drop 16 × 1,232 B, parser 224 × 1,904 B per unit ≤ 448 KiB; debug: 16 × 2,784 B, 224 × 1,664 B). It fails a `Value` drop component that is not acyclic once its `drop_bounded` instances are removed, so a container whose glue recurses without passing `bounded_drop` fails the build check even when another container in the same component passes it. `scripts/vm_wasm_depth.sh` adds drop (array, tuple, option, result), `host_await` and front-end (including nested interpolation) probes, bisects a `plain_generic` chain that stays off the VM's pure-i64 tier, and fails a nesting probe the front end refuses. `scripts/wasm_nesting_parity.sh` backs the new `cli_run.rs` tests and runs every case on both the debug and the release `axon-run.wasm`. `scripts/vm_perf_gate.sh` now also holds fib-recursive within 0.5 % of its median at 2627dad7 (S11).

## Pure-`i64` functions run as register code (R50 S11)

Under `AXON_ENGINE=vm` a fn-table function now runs on a register tier when it takes at most eight `i64` params, returns `i64`, and its body uses only those params, `Int` literals, `Int` arithmetic and comparisons, `if`/`else`, `return` and calls to other such functions (mutually recursive groups qualify together). Its registers live on an explicit frame stack, so recursion on the tier adds no native frames. A refinement-typed, sized, `&mut` or `@[agent]` param, a goal fn, or an `Uncertain` return disqualifies the fn. Checked add, subtract and multiply overflow, and the recursion guard, give the tree-walker's panic text: the tier abandons the call and the generic path replays it. `AXON_VM_TRACE=1` prints `vm: purefn <name> <n> ins` for each fn that qualifies. Output is unchanged.

Instructions retired (`perf stat -e instructions:u`, release): compilebench fib-recursive (`fib(32)`) 2,295,393,887 → 1,002,713,546 (CPython 3.14.4: 3,423,642,492), `warmup` 1,868,062,622 → 804,838,423. Every other `vm_perf_gate.sh` program stays within 0.5 % of its S10 median.

## Array elements and `if` stay in register loops (R50 S10)

Under `AXON_ENGINE=vm` a loop whose body reads or writes local array elements (`xs[i]`, `xs[i] = v`), branches with `if`/`else if`, or is an integer-range `for` can now run as a register loop instead of generic ops. An element read or write leaves the register loop when the index is not an `Int`, is out of bounds, or names an element that is not a scalar of the value's kind. The iteration's element writes are then undone, last write first, and the generic loop reruns it, so a panic, an out-of-bounds error or a kind change gives the tree-walker's output. `AXON_VM_TRACE=1` counts register loops in its `vm: pure <name> <n> exprs, <k> loops` line.

Instructions retired (release): compilebench `sieve` 53.53 G → 24.57 G (CPython 3.14.4: 1.09 G, which clears multiples with one slice assignment in C). The other programs are unchanged within 0.5 %.
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
