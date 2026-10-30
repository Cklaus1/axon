# checker-soundness-2 - DRAFT spec

**Status: AUTHORIZED for build 2026-10-29 (after Step 1 review; human: "approved, do it"). §8 NH-1 decided (a), 2026-10-29.**

### Step 1 review

Two reviews of draft 814cc58d: `agent://AuditCS2` (CS2A-1..18) and `agent://DevloopCS2` (CS2D-1..10). Before deciding, I re-ran every
cited probe (`/var/tmp/r51-tmp/CS2A/*.ax`, plus my own under `/var/tmp/r51-tmp/CS2Draft/rev/`) or read the cited line at 4a6436da.

| review | blocker | major | minor | accepted | accepted, disposition amended | rejected |
|---|---|---|---|---|---|---|
| CS2A | 4 | 5 | 9 | 16 | 2 (#2, #6) | 0 |
| CS2D | 1 | 6 | 3 | 9 | 1 (#5) | 0 |

| id | disposition (one line) |
|---|---|
| CS2A-1 | accepted: x1/x2/x6/x7 re-run, each c=0 and prints `7`/`1`/`6`/`3` -> CS2-4 tuple rule replaced by a per-component domain product; the four probes are controls |
| CS2A-2 | accepted, disposition amended: h6/h7 re-run c=0 `2`/`2`. The proposed "implicit type param" would treat the value type as rigid inside the body (a declared `T` is rigid: `rev/tp_rigid.ax` is E0306 "already fixed to `T`"), which refuses `corrigible.ax:20` `dict_set(state, to_str(step), step)` and `set.ax:23`. -> E-5: the value type of a bare `Dict` in a signature is solved from the body, and generalized only if the body leaves it free (CS2-3b step 3) |
| CS2A-3 | accepted: h10 c=0 `1`; `dict_each` :1958 is `fn(str, V)`, `http_sse` :429/:435 is `fn(str) -> ()` -> unit-return fn params leave the closure's return free |
| CS2A-4 | accepted: `interp/builtins.rs:2655-2662` accepts an Int key on purpose; `l_sumbyf_i`, `l_numkey` c=0 `3` -> `arr_sum_by_f64` takes `fn(T) -> Num` |
| CS2A-5 | accepted: `a_pow_exp` (`pow_i64(2, 4294967296)`) c=0 prints `1`; `exp as u32` truncates at :2969 and `axon-rt/src/lib.rs:1728` -> exponent range guard before `checked_pow` |
| CS2A-6 | accepted, anchor amended: `scopes` is at `mut_borrow.rs:121` (not :149), `define` at :202; f2/f3 re-run c=0 `1\|100` / `1\|5` -> per-binding ids |
| CS2A-7 | accepted: f1 c=0 `0\|1\|2`, f5 c=0 `0\|10\|20` -> listed as conservative refusals and pinned in the E0607 test |
| CS2A-8 | accepted: `builtins.rs:2508-2512`, `interp.rs:10395`, `taint_tests.rs:2112/2513`, `pin.rs:171/218` read; they match the literal `"Dict"` -> sig strings keep `Dict`, consumers migrated in one commit, g01 row M3910 |
| CS2A-9 | accepted: `rnd.ax` panics at `interp/builtins.rs:3023`, `a1_rep.ax` at :3234 (`capacity overflow`); `min_i64`/`max_i64` are at :2896 -> both added to the CS2-2 audit; native `__axon_str_repeat` (`axon-rt/src/lib.rs:2471`) has the same `repeat` |
| CS2A-10 | accepted: `implicit_route` (`caps_verb.rs:498`) needs `axon run` rc 0, and CS2-7's twin uses the same `LEAK` helper -> 7 rewrites |
| CS2A-11 | accepted: anchors re-read and corrected in place (cli_run :14600, :28915; `migrated_pow_i64_matches_interpreter` axon-rt :3919; `fn_bounds` checker.rs:528; generic struct literal infer.rs:1664-1693; opportunities :2842/:2848; value.rs:646; `instantiate_sig` infer.rs:2228; `verify_predicate_offence` effects.rs:219) |
| CS2A-12 | accepted: M3900 killer renamed; M3927 and the native rows get cargo killers in `integration_fixtures`; `mean_i128` parity case added; block release note added (`build-loop-gate-lane.md:52`, script :12084) |
| CS2A-13 | accepted: `rev/g_unbounded_impl.ax` c=0 prints `9` -> a pinned conservative refusal in the CS2-5 test; `g1_mk` is a control |
| CS2A-14 | accepted: `builtins.rs:2020` "Values come back as `str`" -> `dict_from_str -> Dict<str>` |
| CS2A-15 | accepted: `arr_find` :678, `arr_any` :684, `arr_all` :690, `arr_count_if` :696, `http_sse` :428/:434 read -> list completed; `arr_any` (`rev/h_any_bad.ax` c=0, panics) in the fail-first test |
| CS2A-16 | accepted: NH-2 moved to §0 out of scope |
| CS2A-17 | accepted: `o2_shadow_fn` c=2 E0307 re-run -> CS2-6 adds the local-before-top-level rule, o2 is a control |
| CS2A-18 | accepted: `run_program_capturing` skips check (`interp.rs:3034-3045`); `axon run` checks (`main.rs:6129` -> :7535) -> `[INFERENCE]` notes removed |
| CS2D-1 | accepted: `scripts/gate.sh:76` refuses `--fast\|--base=*` -> hand-off row replaced |
| CS2D-2 | accepted: `gate.sh --lane --plan` re-run in `cs2`: `audit=0b29b054b6a8`, 422 `RUN` lines, footer "NOT RUN (strict-only): ... codegen-gated tests, ... parity suite" -> `--lane` not used for inner loops |
| CS2D-3 | accepted: `/var/tmp/v022-cells/cell-M387*` timestamps 10:02:42 -> 10:23:39 for 3 passes of 8-11 rows, 8-22 s per cell -> cost rows rewritten |
| CS2D-4 | accepted: `gl-gate-2.log` cli_run "900 passed ... finished in 276.62s"; stages 659.1 s preflight, S16 529.6, S20 299.7, S03 225.3, S06 224.2 -> §5 uses these |
| CS2D-5 | accepted, count amended: the row table was re-cut to fit CS2A findings into 40 ids, so 38 rows are under `crates/axon-core/src` and the count line becomes **568**, not 569 |
| CS2D-6 | accepted: `AXON_REFERENCE.md:343-344` and :434 render the sig strings; drift test `cli_run.rs:25737` -> regenerate per commit, never hand-merge |
| CS2D-7 | accepted -> §6 conflict rules |
| CS2D-8 | accepted -> lane A order CS2-4 -> CS2-6 -> CS2-3a -> CS2-1 -> CS2-5 -> CS2-3b -> CS2-8 |
| CS2D-9 | accepted -> explicit gate in the merge table |
| CS2D-10 | accepted: E0606 is explained at `spec/axon-for-llms.md:204`, `spec/language-tour.md:346` -> E0607 gets the same mention |

Material design changes: CS2-1 (binding ids, documented conservative refusals), CS2-2 (pow exponent range, `random_i64`, `str_repeat`/`arr_repeat`, native rows),
CS2-3a (unit-return params, `Num` for `arr_sum_by_f64`), CS2-3b (bare-`Dict` signatures solved from the body; sig strings keep `Dict`), CS2-4 (tuple domains),
CS2-6 (local-before-top-level). CS2-5, CS2-7 and CS2-8 change only in tests and counts. The cross-spec docs-truth integration steps from Main are in §6.

---

Base: `main` 4a6436da. Every anchor below was read at 4a6436da in `/home/cklaus/projects/axon-lanes/cs2`. Every reproduction was run with
`/var/tmp/r51-tmp/main-bin/axon`. That binary was built at 0bd2b518, and `crates/` did not change between that commit and 4a6436da. Each probe was run as `axon check F` and then `axon run F`, with `AXON_PATH=examples/stdlib`. The probe sources are in
`/var/tmp/r51-tmp/CS2Draft/*.ax`. Their outputs are in `/var/tmp/r51-tmp/CS2Draft/ALL.txt` and their one-line sources in `SRC.txt`. Review probes are in
`/var/tmp/r51-tmp/CS2A/` (AuditCS2) and `/var/tmp/r51-tmp/CS2Draft/rev/` (mine).
`[INFERENCE]` marks anything that was not observed.

The defect class is the same as checker-soundness: **a program that `axon check` accepts (rc 0) and that then panics or computes a wrong answer at run time.**

## 0. Source entries

| item | source (`tasks/opportunities.md`) | one line |
|---|---|---|
| CS2-1 | O-GAP-03 (:2853) | A closure's write to a captured binding is silently dropped |
| CS2-2 | O-GAP-06 (:2866) | Arithmetic builtins wrap or saturate where the operators panic, and `assert_eq_f64` has three different meanings |
| CS2-3 | O-GAP-01 (:2842), O-GAP-02 (:2848) | Closures passed to higher-order builtins are only arity-checked, and `Dict` values are untyped |
| CS2-4 | O-GAP-04 (:2858) | Exhaustiveness is checked only for enum, `Option` and `Result` scrutinees |
| CS2-5 | O-GAP-05 (:2862) | A method call on an unbounded generic param is accepted, and generic struct arguments are not enforced |
| CS2-6 | O-SF-04 (:2788) | A callee that is not a name skips the arity and type checks |
| CS2-7 | O-S1-15 (:2638), `[DECIDED by human 2026-10-18: ENFORCE]` | Refinements on enum variant fields and on lambda params are not checked |
| CS2-8 | O-SP-02 (:2723), `tasks/capability-surface-spec.md` Q2 (:341, :351) | E1209 (impure refinement) is shallow: wrapping the effect in a user fn hides it |

### Out of scope (logged, not dropped)

- **O-GAP-07** (`:2872`) [REVISED: CS2A-16 - was NH-2]. Plain recursion stops at depth 6000, and `arr_max_i64([])` panics instead of returning an `Option`. Changing a builtin's return type is an API change and the depth limit is a runtime-resource question; both need their own spec, where the product question (`Option` return vs a check-time refusal of a provably empty literal) is asked. Until then the panic stays and is documented in stdlib.md. O-GAP-07 stays **open** in `tasks/opportunities.md` (no closure stamp; the integrator appends "not built by checker-soundness-2; needs its own spec"). It is named only in this prose, never in this spec's headings or first 40 lines, so `spec_ledger.py` rule 5 does not require it closed (§6).
- **O-GAP-08** (`:2876`). Common language forms are refused: `if let`, `Ok(())`, `for (a, b) in pairs`, `impl T {}`, `?` on `Option`, `loop`, tuple-variant enums. These are language-surface additions, not soundness fixes, so they get their own spec.
- **O-GAP-09..11.** Not soundness items. O-GAP-09/11 belong to the docs-truth spec.
- **Shift semantics.** `shl(1, 64)` and `1 << s` with `s = 64` both print `1`, because the shift count is masked (`interp/value.rs:638`, `Shl => wrapping_shl`; the operator overflow helper `int_overflow` is at :646 [REVISED: CS2A-11]). The builtin and the operator agree, so there is no skew for CS2-2 to fix. Whether an over-wide shift should panic is a language question with no opportunity entry yet; the integrator logs one.
- **Bare-variant patterns** (`R =>` binds instead of comparing, `x_enum_bare` W0004): observation only, 0 tracked uses; the integrator logs it.

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | CS2-1, CS2-2, CS2-3 | Silent wrong answers (CS2-1, and CS2-2's pow, sum and mean), plus the largest remaining check-then-panic class (CS2-3) |
| P2 | CS2-4, CS2-5, CS2-7, CS2-8 | Check-then-panic, or an unenforced contract. CS2-7 is human-decided |
| P3 | CS2-6 | Check-then-panic on a rare shape: one tracked use, and it is valid |

## 2. Items

### CS2-1 (P1) Closure assignment to a captured binding - O-GAP-03

**Problem.** A closure captures by copy, into a cell that persists across its calls (T40, `interp.rs:88-110`). Native codegen matches this: the capture is heap-allocated, and the integration test `a_closure_write_to_a_captured_binding_persists_across_calls` (`tests/integration_fixtures.rs:199`) pins `1,2,3,outer=0`.
A write inside the closure therefore updates the closure's private cell. The enclosing binding never sees it, and neither does any other closure that captured the same name.

The spec says captures are by-move (`spec/compiler-phase3.md:322`, C02: "the compiler treats the moved-in variables as moved in the enclosing scope"). `spec/language-tour.md` §13 (:351-393) promises nothing about writes to captures. Today the checker accepts both a write to a capture and a read of the outer name after the closure is created, so the author's model (shared mutable state, as in JS, Python `nonlocal`, or Rust `FnMut`) silently gets a different answer.

Anchors:
- `mut_borrow.rs:121`: `scopes: Vec<HashMap<String, Local>>`; `define` (:202-206) overwrites in place, and a popped scope's index is reused by the next block [REVISED: CS2A-6].
- `mut_borrow.rs:193-198`: `lookup` returns `(Local, captured)` using `closure_floor`.
- `mut_borrow.rs:570-583`: the `Lambda` arm.
- `mut_borrow.rs:653-665`: the `Assign` arm. It already refuses a `&mut`-param write inside a closure, with E0605 ("cannot assign `&mut` parameter ... inside a closure").
- `resolver.rs:2788-2900`: `fill_captures`.
- `interp.rs:6949-6965`: the capture write-back.

**Repro** (all check rc 0):

| probe | shape | run |
|---|---|---|
| `c1_cap` | `let count = 0; let inc = \|\| { count = count + 1 }; inc(); inc(); println(count)` | `0` (an author expects 2) |
| `c1_two` | `add` writes `n`; `get = \|\| n`; `add(); get()` | `0` |
| `c1_nested` | the inner closure writes the outer closure's capture `n`, and the outer closure then returns `n` | `0`, `0` |
| `c1_nested_local` | the inner closure writes `m`, a `let` local to the outer closure | `1` (expects 6) |
| `c1_field` / `c1_index` | `p.v = 9` / `a[0] = 9` inside the closure, then read outside | `1` / `1` |
| `c1_loop` | `while` body: `println(n); let f = \|\| { n = n + 1 }; f()` | `0 0 0` |
| `rev/f5_after` | loop-created `\|k\| { last = k * 10; last }`, then `println(last)` after the loop | `0 10 20 0` (expects 20) |
| `c1_factory` (control) | `fn mk() -> fn() -> i64 { let n = 0; \|\| { n = n + 1; n } }`, then `c(); c()` | `2`: correct counter semantics |
| `c1_shadow` (control) | `\|n: i64\| { n = n + 1; n }`. The param is not a capture | `2`, then outer `0`: correct |
| `c1_shadow_let` (control) | `\|\| { let n = 7; n = n + 1; n }` | rc 0, W0002 shadow note; correct |
| `CS2A/f2_shadow` (control) [REVISED: CS2A-6] | the writing closure, then `let n = 100` re-declared in the same scope and read | `1\|100`, W0002 |
| `CS2A/f3_sibling` (control) [REVISED: CS2A-6] | two sibling `if true { let n ... }` blocks; the first block's closure writes its `n` | `1\|5` |
| `CS2A/f4_field`, `f6_param`, `f7_dict`, `f8_retclosure_block` (controls) | closure stored in a struct field; factory over a param; `dict_set` through a captured handle (not an `Assign`); aliasing the closure | `1\|2`, `12`, `1`, `2` |
| `c1_toplet` (control) | the closure writes a top-level `let` | already rc 2, E0001 |

**Decision: refuse at check when the divergence is observable. Keep the copy-capture semantics.**

The copy semantics and a by-reference semantics give the same observable result in exactly one case: when nothing outside the writing closure can observe the binding after the closure is created. The counter and factory pattern (`c1_factory`, and the T40 tests) is that case, and it stays legal.

New code **E0607**: "a closure assigns captured `x`, but `x` is used outside that closure after it is created; the write is not visible there". The fix-it reads: "closures capture by copy. Return the new value from the closure, or keep the state in one closure."

A write W inside lambda L to binding X is refused when all three hold:
1. X resolves below L's `closure_floor`, i.e. it is captured (`lookup().1 == true`). A lambda param or a `let` inside L is never captured, which keeps `c1_shadow`, `c1_shadow_let` and every closure-local `let` assignable. W covers `Assign`, and `AssignTo` whose `place_root` (`mut_borrow.rs:131`) is X, which covers fields and indexes.
2. Either of these:
   - (a) after L's literal, while the same binding of X is live (same binding id, below), it is referenced outside L: read, assigned, place-assigned, or captured by another lambda (the c1_two case);
   - (b) L's literal sits inside a `while`/`for`/`loop` body that does not contain X's declaration. The back-edge re-captures X, so the literal itself is the later read (`c1_loop`).
3. Nesting composes. An inner lambda's capture of the outer lambda's local (`c1_nested_local`), or of the outer lambda's capture (`c1_nested`), is judged in the outer lambda's body scope.

Implementation lives in the `mut_borrow` walker. It owns scopes and the `closure_floor` already, and runs once per fn body (`checker.rs:1159`).
- [REVISED: CS2A-6] Each `define` assigns a fresh binding id (`Local` gains `id: u32`, a per-walker counter). On leaving a lambda, record `binding id -> lambda span` for every captured binding the lambda wrote. A `let` that redefines the name in the same scope, or the pop of the declaring scope, drops the record for the old id; a later use of the new binding (f2) or of a sibling block's binding (f3) has a different id and never hits.
- A `lookup` that resolves to a recorded id at a later point emits E0607 at that use. The help text names the lambda's line.
- A loop-depth stack (scope length at each loop entry) gives rule 2(b).

**Alternatives.**
- (i) Refuse every write to a capture. This breaks the counter pattern that T40 pins, and 27 Rust-embedded test programs write captures (measured scan, list below).
- (ii) Real by-reference capture: a shared cell for the outer binding too. This changes the T40 semantics on three engines (tree, VM, native closure env layout, `codegen/expr.rs:3630` `emit_lambda`). It would also contradict C02. It is the larger change and would remain open.
- (iii) Warn instead of refusing. A warning does not stop the wrong answer.
- (iv) Flow-sensitive rule 2 (a later use counts only after the closure's first *call*). It needs call tracking through aliases, fields and HOFs (f4, f8); rejected as the larger analysis for 0 tracked programs.

The chosen rule refuses only programs whose output may differ under the semantics the author probably meant.

**Error-code registration** (the four sites, as in checker-soundness CS-1):
- `pub const E0607` after `error.rs:69`;
- an `ALL_CODES` row after `error.rs:483`;
- the `error_codes_are_unique` list at `error.rs:757`;
- `AXON_REFERENCE.md` after :193, by regeneration (§6), not by hand.

`git grep E0607` is empty at 4a6436da. `a_code_not_marked_reserved_is_actually_emitted` (`error.rs:665`) needs the emit site to land in the same commit. [REVISED: CS2D-10] One-line E0607 mentions go next to E0606's in `spec/axon-for-llms.md:204` and `spec/language-tour.md:346` (§13 gains "closures capture by copy; a write to a capture that the enclosing scope reads later is E0607").

**Tracked impact (measured).**
- Tracked `.ax` files: **0** closures write a captured name (my scan over the 596 `git ls-files '*.ax'`, validated on the probes). Both the 290-file and the 596-file sweeps stay identical.
- Rust-embedded programs: 27 closure-writes-capture hits. These are `interp/sink_tests.rs:533`, `interp/taint_tests.rs:18` (3), `mut_borrow.rs:806`, `cli_run.rs:29331, 35413, 35500 (3), 35589, 35606 (2), 35729, 35859, 37307, 37354`, and `integration_fixtures.rs:199, 219`. Hits in `axon-cortex/src/locate.rs:107` are Rust code, a scanner false positive.
  - [REVISED: CS2A-18] `integration_fixtures.rs:199/219` call `run_program_capturing`, which does not run the checker (`interp.rs:3034-3045`), so they are unaffected and keep pinning T40.
  - The `cli_run` cases go through `axon run`, which runs the checker first (`main.rs:6129` -> :7535). Those whose program reads the outer name after the write become E0607 (candidates by scan: `cli_run.rs:29331, 35413, 35500, 35589, 35606, 35859, 37307, 37354` `[INFERENCE: outer-read classification from the scan, not run]`). The builder runs each named test. A test that pins the outer value after a write is rewritten to read through the closure, because what it pinned is the divergence; the T40 persistence assertion (`1,2,3`) stays.

**Fail-first test.**
- `axon-core`, `--no-default-features --test cli_run`, `closure_write_to_a_capture_read_outside_is_refused_e0607`.
- Inputs: c1_cap, c1_two, c1_nested, c1_nested_local, c1_field, c1_index, c1_loop, f5_after. Each gives rc 0 today and must give rc 2 with E0607.
- Pinned conservative refusals [REVISED: CS2A-7], asserted rc 2 + E0607 with a comment that the output would not differ: `CS2A/f1_readfirst` (outer read before the first call) and `CS2A/f5_loopwritefirst` (loop closure writes before it reads, no later outer read).
- Companion: `closure_counter_and_local_writes_stay_legal` stays rc 0 with the same output: c1_factory, c1_shadow, c1_shadow_let, and [REVISED: CS2A-6/CS2A "checked and correct"] f2_shadow, f3_sibling, f4_field, f6_param, f7_dict, f8_retclosure_block.

**False-refusal risk.** [REVISED: CS2A-7] Refused but not divergent: (i) an outer use on a path that never runs after the write; (ii) an outer read that runs before the closure's first call (f1); (iii) a loop-created closure that writes X before reading it, with no later outer read (f5). These are accepted as conservative; the fix-it gives the rewrite. Tracked count: 0.

[REVISED: build CS2-1] As built: every `define` in `mut_borrow.rs` takes a fresh binding id; a lambda records the ids of captured `Plain` bindings it assigns (whole, field or index root); after the literal, any use of a recorded id is E0607 (one per binding), and a literal inside a loop that does not declare the binding is E0607 at once (rule 2(b)). `&mut`/shared params keep their E0604/E0605 paths. The 8 `cli_run` tests the scan named that read the outer name after a write (`interp_arrays_keep_value_semantics_under_shared_representation`, `interp_strings_and_append_builders_keep_value_semantics`, `interp_string_and_array_builder_loops_are_linear`, `interp_closures_capture_only_their_free_variables_with_unchanged_semantics`, `interp_frame_slots_keep_scoping_capture_and_place_semantics`, `interp_builtin_closure_writing_a_captured_array_is_linear`, `vm_closure_capture_semantics`, `vm_closure_copied_path_and_errors`) are rewritten to read through the closure (or drop the outer read); the T40 persistence values stay. `integration_fixtures.rs:199/219`, `sink_tests`, `taint_tests` and `mut_borrow.rs:806` stay green unchanged (they do not go through the checker or do not read outer). M3893's guard is the id counter (`self.next_id += 1`): with ids, a redefinition or a sibling block gets a new id, so f2/f3 are the false-refusal markers; the `i < f.floor` capture test stays but is redundant with ids (a param or closure-local id is never used after the literal). The test lists put c1_two and f2_shadow first so the §4 markers are the first failure. Sweeps S290 and S596 are identical to the post-3a sweeps.

### CS2-2 (P1) Arithmetic builtins disagree with the operators; `assert_eq_f64` - O-GAP-06

**Problem.** The `+ - *` operators panic on i64 overflow (`interp/value.rs:646` `int_overflow`, "integer overflow: a + b exceeds i64", exit 101). Several builtins that compute the same arithmetic instead wrap, saturate, or panic in Rust.

**Repro** (all check rc 0):

| probe | program | run | operator equivalent |
|---|---|---|---|
| `a_pow` | `pow_i64(3, 40)` | `-6289078614652622815` rc 0 (wrapped) | `a_mul`: `m * 2` -> panic 101 |
| `a_pow_exp` [REVISED: CS2A-5] | `pow_i64(2, 4294967296)` | `1` rc 0 (exponent truncated to u32 0) | - |
| `a_sum` | `arr_sum_i64([MAX, 1])` | `9223372036854775807` rc 0 (saturated) | `a_fold` (`arr_fold(.., \|a, x\| a + x)`) -> panic 101 |
| `a_sumby` | `arr_sum_by([MAX, 1], \|x: i64\| x)` | `9223372036854775807` rc 0 | same |
| `a_mean2` | `arr_mean_i64([MAX, MAX, MIN, MIN])` (true mean -0.5) | `-2.30584e+18` rc 0 (a saturated partial sum: wrong) | - |
| `a_dictinc` | `dict_set(d, "k", MAX); dict_inc(d, "k")` | `thread '<unnamed>' panicked at crates/axon-core/src/interp/builtins.rs:5514:38: attempt to add with overflow` rc 101 (a Rust debug-overflow panic, not an Axon panic; release has no `overflow-checks` key (`Cargo.toml:48-52`), so a release build wraps silently) | `a_add` -> Axon panic 101 |
| `CS2A/rnd` [REVISED: CS2A-9] | `random_i64(MIN, MAX)` x20 | Rust panic `interp/builtins.rs:3023:32: attempt to add with overflow` rc 101 | - |
| `CS2A/a1_rep`, `rev/arep` [REVISED: CS2A-9] | `str_repeat("ab", MAX)` / `arr_repeat(1, MAX)` | Rust `capacity overflow` panic / `arr_repeat: cannot allocate ... 9223372036854775807 elements` rc 101 | - |
| `a_pow_neg` (control) | `pow_i64(2, 0 - 1)` | `axon: panic: pow_i64: negative exponent` rc 101 | - |
| `a_shl`, `a_shlop` (control) | `shl(1, 64)`, `1 << s` | both `1` | agree (out of scope, §0) |

**Audit of every i64 arithmetic builtin** (interpreter, `interp/builtins.rs` at 4a6436da):

| builtin | interp | native | decision |
|---|---|---|---|
| `pow_i64` | `wrapping_pow(exp as u32)` :2969 | `__axon_pow_i64` `axon-rt/src/lib.rs:1716-1728` `wrapping_pow(exp as u32)` (`codegen/builtin_externs.rs:177-182`) | [REVISED: CS2A-5] **panic** `integer overflow: pow_i64(b, e) exceeds i64` on both engines. For `exp > u32::MAX`: result `1` for `b == 1`, `1`/`-1` by parity for `b == -1`, `0` for `b == 0`, otherwise the panic. Then `checked_pow(exp as u32)` |
| `arr_sum_i64` | `saturating_add` :1861 | `ArrReduce::Sum` -> `w_int_add_sat` `codegen/expr.rs:4774-4777`, `build_wrappers.rs:304` | **panic** (checked_add), same message shape as `+` |
| `arr_sum_by` | `saturating_add` :2672 (comment :2664 says it saturates to match `arr_sum_i64`) | no native lowering (`git grep arr_sum_by crates/axon-core/src/codegen crates/axon-rt` is empty; CS2A confirmed "refused as unlowered") | **panic** |
| `arr_mean_i64` | sums with `saturating_add` :2266, then `/ len` | `ArrReduce::Mean` (`codegen/expr.rs:4931`) through the same saturating accumulator | **exact**: accumulate in i128, then divide as f64. A mean cannot overflow, so no panic is needed. Natively: an i128 accumulator in the loop, or the `Mean` arm calls an axon-rt helper; the builder picks the one with the smaller IR change |
| `dict_inc` | `n + 1` :5514 | `axon-rt/src/lib.rs:641` `cur + 1` | **panic** (checked_add, Axon panic; natively the rt's panic-and-exit-101 path used by `__axon_pow_i64`'s negative exponent) |
| `random_i64` [REVISED: CS2A-9] | `lo + (r % range) as i64` :3023 wraps when `hi - lo` exceeds i64 | `codegen/builtins.rs:3174-3215`: `hi - lo` with `w_int_sub`, wraps | **exact**: compute `lo as i128 + (r % range) as i128`, cast back (in range by construction). Natively the range is computed in i128 or the full-range case calls `rand` widened `[INFERENCE: native was read, not run with MIN/MAX]` |
| `str_repeat`, `arr_repeat` [REVISED: CS2A-9] | `.repeat(n)` :3234 (Rust `capacity overflow`); `arr_repeat` already refuses with an Axon panic | `axon-rt/src/lib.rs:2471` `src.repeat(count)` | `str_repeat`: `checked_mul(len, n)` else Axon panic `str_repeat: result too large` on both engines. `arr_repeat`: unchanged |
| `abs_i64`, `as_i64`/`f64_to_i64`, `wrapping_*` | `abs_i64(MIN)` panics (`a_abs`); float->int casts saturate (documented); `wrapping_*` wrap by name | - | unchanged: `abs` agrees with `0 - MIN`, and the casts and `wrapping_*` are documented |
| `clamp_i64`, `sign_i64` (:3004-3026), `min_i64`/`max_i64` (:2896-2903) [REVISED: CS2A-11] | no arithmetic overflow possible | - | unchanged |

The native path changes in the same commit as the interpreter (I-2). The operator overflow lowering already exists for `+`; the builder reuses its panic path for `ArrReduce::Sum` and makes `__axon_pow_i64` / `__axon_dict_inc` checked in axon-rt.

**`assert_eq_f64` - decided: exact IEEE `a == b` on all engines.**
- Interpreter: `if !(a == b || (a - b).abs() <= 1e-9)` (`interp/builtins.rs:3783`).
- Native: `FloatPredicate::OEQ` (`codegen/builtins.rs:1447-1449`).
- `spec/stdlib.md:516`: "exact float equality".
- `builtins.rs:250` doc: "Panic if `a != b`".

The interpreter is the reference semantics, but here it disagrees with its own documentation and with native. A tolerance hidden inside an "eq" assertion is the less obvious contract. Measured reliance on the tolerance:
- Each tracked `.ax` with an `assert_eq_f64` was re-run under `axon test` with the call replaced by an exact check: `examples/stdlib/asi_prelude.ax`, `reward.ax`, `uncertain.ax`, `crates/axon-core/tests/fixtures/phase12_coverage.ax`, and the tempt variant `tempt-unsat-doc-03/uncertain.ax`. There were **0** exact-only failures. The tempt variant already fails today (rc 3, `0.9 vs 1.3`, by design).
- The one Rust test that relies on the tolerance is `interp/vm/tests.rs:2622` `t_near` (`assert_eq_f64(0.1 + 0.2, 0.3)`), inside `assert_eq_f64_fails_on_nan_on_both_engines`. It asserts a pass (pass list :2643). Natively the same program fails today: a live engine divergence.

Change:
- `interp/builtins.rs:3783` [REVISED: CS2A-11] becomes `if !(a == b)`. NaN still fails and `inf == inf` still passes.
- `t_near` moves from the passing list to the failing list; the doc comment at `vm/tests.rs:2612-2616` is updated [REVISED: CS2A-12].
- The `builtins.rs:250` doc becomes "Panic unless `a == b` exactly (IEEE; NaN never equal). For a tolerance write `assert(abs_f64(a - b) <= eps)`".
- g01 **M76** (`v022_g01_mutations.py:470-474`): its `old` string is the line that changes. The row is rewritten to `old: if !(a == b) {` (0 occurrences today, so unique once written) and `new: if a != a || (a - b).abs() > 1e-9 {`. The mutant restores the tolerance and is killed by `t_near` in the fail list [REVISED: CS2A-12 - the "(still NaN-kill)" note was misleading and is dropped]. Same killer.
- This decision has already been sent to `DocsDraft` for the doc lane.

**Tests that flip (they pin the old behaviour):**
- `cli_run.rs:14600` [REVISED: CS2A-11] `arr_sum_by_sums_a_projection_and_saturates_like_arr_sum_i64`: becomes `arr_sum_by_sums_a_projection_and_panics_like_the_operator`.
- `scripts/fuzz_parity.sh:464` `arr_sum_ovf`, and the `scripts/arr_reduce_parity.sh:9` note.
- `axon-rt/src/lib.rs:3919` `migrated_pow_i64_matches_interpreter` [REVISED: CS2A-11] (oracle `wrapping_pow`, :3917-3925) and `migrated_pow_i64_common_cases`.
- The `scripts/checked_arith_parity.sh` pow cases (:89, `pow_neg_exp`) gain overflow cases.
- The `builtins.rs:609` doc "Saturates on overflow", and the `str_repeat` / `random_i64` docs gain their limits.

**Fail-first tests.**
- `axon-core --no-default-features --test cli_run`, `arithmetic_builtins_overflow_like_the_operators`: a_pow, a_pow_exp, a_sum, a_sumby, a_dictinc each give rc 101 with `integer overflow`; a1_rep gives rc 101 with `str_repeat: result too large`; a_mean2 prints `-0.5`; rnd runs to rc 0 with 20 values in range. Today they give rc 0 or a Rust panic. Control: `a_pow_ok`, `a_pow_neg` unchanged.
- `interp::vm::tests::assert_eq_f64_fails_on_nan_on_both_engines` gains `t_near` in the fail list.
- [REVISED: CS2A-12] Native: `axon-core --test integration_fixtures`, `arithmetic_builtins_overflow_natively` (pattern of `verify_composite_predicate_is_enforced_or_refused_natively`, :2500: build and run on both engines). Cases a_pow, a_sum, a_dictinc (rc 101 both), a_mean2 (`-0.5` both). Skips when codegen is absent, like its model.
- Parity: `scripts/checked_arith_parity.sh` gains `pow_ovf`, `pow_exp_trunc`, `sum_ovf`, `dictinc_ovf` (rc 101 on both engines) and `mean_i128` (same output on both).

**False-refusal risk.** None at check, because this is runtime only. The 290- and 596-file sweeps are unaffected. The runtime behaviour change applies only to inputs that overflow today, where the old answer was wrong or a Rust panic.

### CS2-3 (P1) Typed higher-order builtins and typed `Dict` - O-GAP-01 + O-GAP-02

**Problem.** Builtin sig strings such as `"fn(T) -> U"` are parsed by `parse_type_str` (`infer.rs:263-330`), which has no `fn(` arm. Each lands as `Type::Deferred("fn(T) -> U")`, and `promote_type_params` (`infer.rs:362-376`, comment :360-361) leaves it alone. The affected builtins [REVISED: CS2A-15]: `builtins.rs:624` `arr_map`, :630 `arr_filter`, :636 `arr_fold` `fn(U, T) -> U`, :642 `arr_sort_by`, :678 `arr_find`, :684 `arr_any`, :690 `arr_all`, :696 `arr_count_if`, :702 `arr_sum_by`, :708 `arr_sum_by_f64`, :714 `arr_zip_with`, :428/:434 `http_sse`/`http_sse_post` `fn(str) -> ()`, :1922 `dict_map_values`, :1928 `arr_group_by`, :1934/:1940 `arr_max_by`/`arr_min_by`, :1946/:1952 `arr_take_while`/`arr_drop_while`, :1958 `dict_each` `fn(str, V)`, :1969 `arr_partition`, :1981 `dict_filter`.

The checker sees such a param and checks only the closure's arity (`checker.rs:5795-5815`, `param_is_fn`, with the NOTE at :5802). `Dict` likewise parses to the opaque `Deferred("Dict")`. `dict_get` returns `Option<T>` with T free (`builtins.rs:1879-1881`), and an annotation `Dict<str, i64>` resolves to `Deferred` (`infer.rs:717-718`, "other generics treated as deferred").

**Repro** (all check rc 0):

| probe | run |
|---|---|
| `h_filter`: `arr_filter([1,2,3], \|x\| x + 1)` | `arr_filter: predicate must return bool, got i64` 101 |
| `rev/h_any_bad`: `arr_any(xs, \|x\| x + 2)` [REVISED: CS2A-15] | `arr_any: predicate must return bool, got i64` 101 |
| `h_fold`: `arr_fold(["a"], 0, \|acc, x\| acc + x)` | `cannot apply Add to i64 / str` 101 |
| `h_named`: `arr_filter(["a"], big)` with `fn big(x: i64) -> bool` | `cannot apply Gt to str / i64` 101 |
| `l_maxby_b`: `arr_max_by([1,2], \|x\| x > 1)` | `key fn must return numeric, got bool` 101 |
| `d_get`: `dict_set(d, "a", 1)`, then `Some(v) => println("got " + v)` | `cannot apply Add to str / i64` 101 |
| `d_ann`: `let d: Dict<str, i64> = dict_new(); dict_set(d, "a", "x")`, then `v + 1` | same 101 |
| `d_param_bare`: `fn total(d: Dict) -> i64 { ..Some(v) => v + 1.. }` called with a str-valued dict | same 101 |
| `d_mixed`: `dict_set(d, "a", 1); dict_set(d, "b", "x")` | `2` rc 0 (heterogeneous dict) |
| `l_generic_ok` (control): `fn keep<T>(xs: [T], f: fn(T) -> bool) -> [T] { arr_filter(xs, f) }` | `2`, must stay rc 0 |
| `l_annot_lambda` (control): `arr_fold(xs, 0, \|acc: i64, x: i64\| acc + x)` | `6`, must stay rc 0 |
| `l_sumbyf_i`, `l_numkey` (controls) [REVISED: CS2A-4]: an i64 key to `arr_sum_by_f64` / `arr_max_by` | `3`, must stay rc 0 |
| `CS2A/h10_each_ret` (control) [REVISED: CS2A-3]: `dict_each(d, \|k, v\| dict_inc(out, k))`, `dict_each(d, \|k, v\| v + 1)` | `1`, must stay rc 0 |
| `rev/h_any_ok` (control): `arr_any(xs, \|x\| x > 2)` | `true` |
| `CS2A/h2_opt`, `h3_nested`, `h4_empty`, `h5_dictarr`, `h9_sortby_maxby` (controls) | each rc 0 today, must stay rc 0 |
| `CS2A/h6_bare_param` (control) [REVISED: CS2A-2]: `fn size(d: Dict) -> i64 { dict_len(d) }` called with an i64 dict and a str dict | `2` |
| `CS2A/h7_field` (control) [REVISED: CS2A-2]: `type Reg = { tags: Dict }` built with both kinds | `2` |
| `rev/h6b_put` (control): `fn put(d: Dict) { dict_set(d, "k", 1) }`, caller reads `v + 1` | `2` |

It splits into two sub-items, built in order **3a then 3b**. 3b needs 3a's `fn(` parsing for `dict_map_values`, `dict_filter` and `dict_each`, and CS2-5's `Type::App` for struct fields.

**CS2-3a - typed fn params of builtins.**
1. `parse_type_str` gains a `fn(P, ..) -> R` arm, which yields `Type::Fn(params, ret)`. `promote_type_params` gains a `Type::Fn` arm, so the T/U/V of a builtin's fn param become the same type variables as the rest of the sig. [REVISED: CS2A-3] `fn(P..)` with no `->` parses as unit return. When a builtin's fn param declares a unit return (`dict_each`, `http_sse`, `http_sse_post`), only the params are unified; the closure's return type is a fresh var left unconstrained, because the result is discarded.
2. `instantiate_sig` (defined at `infer.rs:2228`, called at :1375 [REVISED: CS2A-11]) instantiates them per call. A lambda argument already infers as `Type::Fn(fresh params, body)` (`infer.rs:1842-1860`), and a named fn as `Type::Fn(sig)` (the `Ident` arm, `infer.rs:1096-1116`). The existing `self.constrain(arg_ty, param_ty, "arg i of name")` (`infer.rs:1398-1402`) unifies them, so a mismatch is the standard E0102.
3. Numeric key fns: `arr_max_by`/`arr_min_by` declare `fn(T) -> f64`, but the interpreter accepts any numeric key, and tracked code passes i64 keys (`cli_run.rs:28915` [REVISED: CS2A-11]). A new sig placeholder `Num` is parsed to a fresh var recorded in `numeric_vars`. After solving, a var not resolved to `i64`/`i32`/`f64`/sized int is E0306 "key fn must return a number". `arr_max_by`, `arr_min_by` and [REVISED: CS2A-4] `arr_sum_by_f64` take `fn(T) -> Num` (the interpreter adds an Int key as f64 on purpose, `interp/builtins.rs:2655-2662`). `arr_sum_by` stays `fn(T) -> i64`.
4. Checker: `checker.rs:5795-5815` keeps its arity check, which gives a better message than a unification failure. Its "Deferred fn" branch is deleted once no builtin sig yields `Deferred("fn(")`. Clean cutover: no alias.
5. The rendered sigs in `AXON_REFERENCE.md` (:343-344) change where `Num` appears; regenerate (§6).
6. [REVISED: build CS2-3a] As built: the `Num` vars are `InferCtx::num_obligations` (one per call that instantiates `Num`), checked in `solve()`; a resolved var that is not an integer or a float is E0306 "key fn must return a number, got `T`" (a var still unresolved is left to the run time). `Num` is a builtin type-param name in the shared `is_builtin_type_param_name`, so both passes agree. A unit return stays `Unit` in the parsed sig (so `axon reference` and the arity hint still print `fn(str, V) -> ()`); the call site replaces it with a fresh var, only for a call that reaches a builtin. Inference skips unifying a closure whose arity differs from the fn param's, so a wrong-arity closure gets only the checker's E0306 (which names the shape) and no second E0102; for a user fn's `fn(..)` param this replaces the old E0102 with that E0306 (refused before and after). Sweeps S290 and S596 are identical to the post-CS2-6 sweeps.

Generic user fns cannot be passed by name today (`CS2A/h1_generic_named.ax` is already E0306), so 3a creates no new refusal there.

**CS2-3b - typed Dict values.**
1. A new `Type::Dict(Box<Type>)`, the value type. Keys are `str` only (`interp.rs:113-119`).
2. [REVISED: CS2A-8] **Sig strings keep the literal `Dict`.** `parse_type_str("Dict")` yields `Dict(V)` where `V` is the sig's own type param named `V`, so a sig links the dict to its value by naming the value `V`: `dict_set(d: Dict, k: str, v: V)`, `dict_get -> Option<V>`, `dict_get_or(.., default: V) -> V`, `dict_values -> [V]`, `dict_to_pairs -> [(str, V)]`, `dict_from_pairs([(str, V)]) -> Dict`, `dict_merge(Dict, Dict) -> Dict`, `dict_filter(Dict, fn(str, V) -> bool) -> Dict`, `dict_each(Dict, fn(str, V))`, `dict_inc(d: Dict<i64>, ..)`. Sigs whose dict value is not `V` write it: `dict_map_values(Dict, fn(V) -> U) -> Dict<U>`, `arr_group_by -> Dict<[T]>`, [REVISED: CS2A-14] `dict_from_str -> Dict<str>`, `dict_try_from_str -> Result<Dict<str>, str>`; `dict_to_str(Dict)`, `dict_to_json(Dict)` take any `V`. Sigs that name the value `T` today (`dict_set`, `dict_get`, `dict_get_or`) are renamed to `V`.
3. Annotations and signatures. `Dict<str, V>` resolves to `Dict(V)` (`infer.rs:702-718`). `Dict<K, V>` with K not `str` is E0102 "Dict keys are str". [REVISED: CS2A-2, amended]
   - An unannotated `let` gets one var (inferred from its writes and reads).
   - A bare `Dict` in a user fn's param or return type gets a var owned by that fn. It is **solved from the fn's body** before the fn's callers are inferred: the fns that carry a bare `Dict` are inferred callees-first in call-graph order (`infer_program` walks items in source order today, `infer.rs:2095`/:2101; a recursive SCC shares its vars). If the body fixes the var (`v + 1` on a `dict_get`, `dict_set(d, k, 1)`), the signature means `Dict<str, that type>` and callers must agree (E0102): this closes `d_param_bare`, and `put` in `h6b_put` becomes `Dict<str, i64>`. If the body leaves it free (`dict_len(d)` in `h6_bare_param`), the var is generalized: an implicit type param instantiated fresh per call.
   - The var is flexible inside the body, never rigid like a declared `<T>`. A declared `T` is rigid in the checker's builtin-arg check (`rev/tp_rigid.ax`: E0306 "the element type `T` was already fixed to `T`"); the implicit var never goes through that path, so `examples/asi/corrigible.ax:20` `dict_set(state, to_str(step), step)` fixes the var to i64 instead of being refused.
   - A bare `Dict` in a struct field is an implicit type param of the struct, through CS2-5's `Type::App`. A literal instantiates it fresh (`h7_field`); a bare struct name in a fn signature gets the same solve-or-generalize rule (`examples/stdlib/set.ax:17` `type Set = { d: Dict }`, `set_add` :23 writes `1`, so `Set` in `set_add` means `Set` over i64).
   - A fn that is generic over the value type on purpose can still write `fn f<V>(d: Dict<str, V>)`.
4. [REVISED: CS2A-8] Every reader of the literal `"Dict"` or `Deferred("Dict")` migrates in the same commit to a helper `is_dict_type_str(&str)` (`Dict` or `Dict<..>`) or to `Type::Dict(_)`: `builtins.rs:2510` (`is_heap_allocating_builtin`, used by `checker.rs:2604`/:2764), `interp.rs:10395` (sig drift test), `interp/taint_tests.rs:2112`/:2513, `interp/pin.rs:171`/:218, `checker.rs:7326`/:7910 (method-on-Dict checks), `checker.rs:410` (`DEFERRED_PREFIXES` loses `"Dict"`), `codegen/types.rs:160-161`, `codegen/builtin_externs.rs:71`, `codegen/builtins.rs:1318`/:1337/:1348. Codegen keeps `Dict` as the opaque handle; it reads the shared `crate::types::Type` (`checker.rs:26`), so `Type::Dict(_)` lowers exactly like `Deferred("Dict")` did. The builtins unit test `r17_heap_alloc_classifier_is_sound_for_hal_and_strings` (`builtins.rs:3051`) gains `dict_map_values`, `arr_group_by`, `dict_from_str` in its must-allocate list.

**Mixed-value dicts in tracked code (measured).** I classified the `dict_set` value kinds per dict variable in all 26 tracked `.ax` files that call `dict_set`. These six put values of different types into one dict:

| file | mix | in the 290 sweep? | change under NH-1(a) |
|---|---|---|---|
| `examples/asi/safe_bandit.ax` | `Agent` + i64 in `state` (:70-110: `"agent"` -> `initial_agent`, `"approved"`/`"refused"` -> counters) | yes (rc 0, W1210) | rewritten in CS2-3b's commit: the agent lives in a `let` rebound per round, the counters stay in `state: Dict<str, i64>`; output unchanged |
| `tests/fixtures/vm_depth/nest/drop_array.ax`, `drop_option.ax`, `drop_result.ax`, `drop_tuple.ax` | i64 + a value nested through the dict ("read back from an untyped dict, so it nests without a recursive type", drop_array.ax:4-6). AX-59 depth probes run by `scripts/vm_wasm_depth.sh` and `wasm_nesting_parity.sh` | no (596 only) | each wraps its nesting in a one-line recursive enum (`type N = Z \| L { v: [N] }`, the phase16 List form) so the dict holds one type; DEPTH and the chain length are unchanged `[INFERENCE: AX-59 owner confirms the drop path measured is still Elems/bounded_drop]` |
| `tests/fixtures/r51_s0b/leaf_sum_confusion.ax` | `Result` + `Option`: the fixture *is* the untyped-dict hole (its header, :1-4). Its test is `cli_run.rs:40069` `vm_same_both_engines` | no | becomes a check-refusal fixture (rc 2, E0102); `cli_run.rs:40069` asserts the refusal on both engines instead of the run-time confusion |

Typed values refuse all six. This is **NH-1**. The fallback if NH-1 is declined is in §8.

**Alternatives considered.**
- (i) Unify the closure only in the checker (`checker.rs:5802` path). This duplicates inference and loses the cross-arg constraint (fold's `U` from `init`).
- (ii) Monomorphic `Dict<str, Value>` with a runtime tag. This is the status quo.
- (iii) Enforce only explicit `Dict<str, V>` annotations. That fixes `d_ann` only; `d_get` and `d_param_bare` stay holes. It is NH-1(c), the fallback.
- (iv) [REVISED: CS2A-2] One var per declaration site for a bare `Dict` in a signature (the draft's E-5): refuses `h6_bare_param`/`h7_field`. A rigid implicit type param: refuses `corrigible.ax:20` and `set.ax:23`. Both rejected.

**Fail-first tests** (`axon-core --no-default-features --test cli_run`):
- `closure_passed_to_a_builtin_is_type_checked` (h_filter, h_any_bad, h_fold, h_named, l_maxby_b -> rc 2, E0102/E0306);
- `generic_and_annotated_closures_to_builtins_stay_accepted` (rc 0, same output as today): l_generic_ok, l_annot_lambda, l_sumbyf_i, l_numkey, h10_each_ret, h_any_ok, h2_opt, h3_nested, h4_empty, h5_dictarr, h9_sortby_maxby, and the `cli_run.rs:28915` i64-key program;
- `dict_values_are_typed` (d_get, d_ann, d_param_bare, d_mixed -> rc 2, E0102), with controls h6_bare_param, h7_field, h6b_put, and `examples/stdlib/set.ax`/`bandit.ax`/`examples/asi/corrigible.ax`/`llm_cache.ax` checked rc 0.

**False-refusal risk: the highest in this spec.** Every tracked file that uses a higher-order builtin or a Dict changes type-inference path. Gate: after 3a, and again after 3b, the all-tracked sweep (§5) must be identical except the named files. Any other changed row is triaged before merge. It is either a real latent bug in that file (the refusal is kept, the file fixed and named in the commit) or a false refusal (the typing is fixed). No row is waived. Residual risk named: a bare-`Dict` fn whose body writes two value types into its param is refused (that is NH-1's homogeneity, applied inside the fn).

### CS2-4 (P2) Exhaustiveness for non-enum scrutinees - O-GAP-04

**Problem.** `check_match_exhaustiveness` (`checker.rs:6206`) emits E0304 only for enum, `Option` and `Result` (:6312-6343). A match on i64, str, f64, tuples or bool with no catch-all is accepted. An arm with a guard is counted as covering its pattern (`x_opt_guard`).

**Repro:**

| probe | check | run |
|---|---|---|
| `x_int`: `match n { 0 => .., 1 => .. }`, `f(5)` | 0 | `no match arm matched` 101 |
| `x_str`: `match s { "a" => .., "b" => .. }` | 0 | 101 |
| `x_tuple`: `(bool, bool)` with 2 of 4 combos | 0 | 101 |
| `x_bool_half`: `match b { true => 1 }` | 0 | 101 |
| `x_unit_stmt`: an int match in statement position | 0 | 101 |
| `x_opt_guard`: `Some(x) if x > 0 => .., None => ..` | 0 | 101 |
| `x_bool_ok`, `x_int_bind` (`k => k * 2`) (controls) | 0 | `0` / `10` |
| `CS2A/x1_opt_tuple_full` (control) [REVISED: CS2A-1]: `(Option<i64>, bool)`: `(Some(x), _)`, `(None, true)`, `(None, false)` | 0 | `7` |
| `CS2A/x2_int_wild_comp` (control): `(i64, bool)`: `(_, true)`, `(_, false)` | 0 | `1` |
| `CS2A/x6_res_tuple` (control): `(Result, i64)`: `(Ok(v), k)`, `(Err(_), k)` | 0 | `6` |
| `CS2A/x7_nested_tuple` (control): `((bool, bool), bool)`, all 8 | 0 | `3` |
| `CS2A/x3_enum_tuple`, `x4_bool_bind`, `x5_litfn`, `x8_char` (controls) | 0 | each correct today |

**Change.** Unguarded arms decide coverage:
- int, str, f64, char: exhaustive only with a catch-all arm (`_` or a bare binding).
- bool: `true` + `false`, or a catch-all.
- [REVISED: CS2A-1] Tuple: flatten nested tuples. A component's domain is: bool -> {true, false}; enum -> its variants; `Option` -> {Some, None}; `Result` -> {Ok, Err}; any other type -> {*}. A domain value is covered by an unguarded arm whose pattern at that component is a wildcard, a binding, or that constructor with an irrefutable payload; {*} is covered only by a wildcard or a binding. The match is exhaustive iff every combination in the product is covered by one arm (cap 256 combinations; above the cap a catch-all is required).
- Enum, `Option`, `Result`: unchanged, except that a guarded arm no longer counts.

The message reads "non-exhaustive match on `i64` - add a `_ =>` arm" (E0304, `error.rs:454`, already "non-exhaustive match - a variant has no arm"). The description widens to "a value has no arm" (`error.rs:454`; `AXON_REFERENCE.md:164` by regeneration). This applies in expression and statement position alike. `spec/language-tour.md:210` ("`match` is exhaustive") and `spec/axon-for-llms.md:214` become true as written.

**Measured.**
- 1115 `match` blocks in tracked `.ax`, scanned for scalar/tuple scrutinees without a catch-all: 9 flagged. 8 were false positives on hand inspection (`examples/feature_tour.ax:22,56` and the hardset copy have `_ =>` with no comma; `examples/tuples.ax:35,37` and the copy are irrefutable `(a, b)`). `git grep 'match ('` in `*.ax` finds 7 tuple-scrutinee sites, none of the CS2A-1 shapes.
- The one real hit is `crates/axon-core/tests/fixtures/vm_match/nomatch.ax` (596 set, baseline rc 0). It deliberately exercises the runtime panic, through `vm_match_non_exhaustive_panics` (`cli_run.rs:37821`). It becomes rc 2 with E0304. The test is changed to assert the check refusal; the runtime panic stays covered through `run_program_capturing`, which does not check (`interp.rs:3034-3045`) [REVISED: CS2A-18].
- Guarded-arm reliance: 0 tracked `.ax`. The Rust-embedded hits from the guard scan were Rust code (`lockfile.rs`, `manifest.rs`, `locate.rs`), i.e. false positives.

**Fail-first test.** `non_exhaustive_scalar_and_tuple_matches_are_refused` (x_int, x_str, x_tuple, x_bool_half, x_unit_stmt, x_opt_guard -> rc 2, E0304), with every control above still rc 0 and the same output.

**False-refusal risk.** Guards on enum arms: a match that is exhaustive only through guards the author knows are total (`Some(x) if x > 0`, `Some(x) if x <= 0`) is refused. The fix is a catch-all. Accepted as conservative and measured at 0 tracked uses. A literal-pattern arm inside a tuple component (`(0, true)`) covers nothing for the {*} domain, so a tuple that enumerates every int it can see still needs a catch-all, as for a scalar.

### CS2-5 (P2) Methods on generic params; generic struct arguments - O-GAP-05

**Problem.** `checker.rs:4176-4182` drops the method check when the receiver is a generic param. The comment there says a stronger version "would look the method up on the declared bound ... needs the bounds threaded here". The bounds already exist: `fn_bounds` (fn -> `(param, traits)`, `checker.rs:528`, filled at :854-856 [REVISED: CS2A-11]) and `trait_defs` (:510-532). Separately, `B<i64>` resolves to `Deferred("B")` (`infer.rs:717-718`). A generic struct literal already gets fresh vars for the type params (`infer.rs:1664-1693`), but they are not tied into the literal's result type, so the annotation is never compared [REVISED: CS2A-11 - `infer.rs:1604-1620` is the enum-variant path].

**Repro:**

| probe | check | run |
|---|---|---|
| `g_method`: `fn f<T>(x: T) -> i64 { x.area() }`, `f(3)` | 0 | `no method area on type i64` 101 |
| `g_bounded_bad`: `T: Shape`, `x.perim()` (Shape has only `area`) | 0 | `no method perim on type Sq` 101 |
| `g_struct`: `let b: B<i64> = B { v: "s" }; b.v + 1` | 0 | `cannot apply Add to str / i64` 101 |
| `g_struct_param`: `fn get(b: B<i64>)` called with `B { v: "s" }` | 0 | same |
| `g_bounded_ok` (control) | 0 | `9` |
| `CS2A/g1_mk` (control) [REVISED: CS2A-13]: `mk<T>() -> B<T>` with annotations | 0 | `2x` |
| `CS2A/g2_conc_field`, `g4_nested_app`, `g5_bounded_closure` (controls) | 0 | `13`, correct, correct |
| `g_bound_violation` (control) | already rc 2, E0504 | - |
| `rev/g_unbounded_impl` (pinned refusal) [REVISED: CS2A-13]: unbounded `T`, `x.area()`, every caller passes `Sq` which has it | 0 | `9` |

**Change.**
1. A receiver whose type is generic param `T` of the current fn:
   - unbounded -> E0403 "no method `m` on type parameter `T` - it has no bound; add `T: Trait`";
   - bounded -> look `m` up in the declared methods of each bound trait (`trait_defs`), and E0403 if none declares it.

   `current_generic_params` (`checker.rs:509`) becomes a param -> bounds map built from `fn_bounds`, which replaces the bare name set.
2. A user generic type: new `Type::App(String, Vec<Type>)`. `B<i64>` resolves to `App("B", [i64])`.
   - The struct literal path (`infer.rs:1664-1693`) yields `App("B", vars)` from the vars it already creates.
   - Field access on `App` substitutes the args into the field type.
   - `App` unifies with `App` by name and args, and with the legacy `Struct("B")` by name, during the cutover inside `infer.rs` only.
   - `instantiate_sig` and `subst_ty` (`infer.rs:2246-2258`) recurse into `App` args [REVISED: CS2A-13].
   - Mismatch is E0102.

**Alternatives.** Name-mangled monomorphic structs (`Struct("B<i64>")`) need every lookup keyed twice. Erasure is the status quo. Checking an unbounded `T`'s methods per call site (template-style) would accept `g_unbounded_impl`, but it makes a generic fn's validity depend on its callers; rejected.

**Measured.**
- Generic user types in tracked `.ax`: 8 (`generics.ax` `Pair`, `phase22`, `phase28` `Pair`/`Triple`, `phase55`, `phase59` `Box`, `phase74` `Stack`, `Tagged`), all rc 0 in the 596 baseline.
- Bounded-generic method calls (tracked `.ax` and Rust-embedded): `phase62_trait_bounds.ax`, `conform_tests.rs` `area_of<T: Shape>`, `tests_v022_b.rs:88` `judge<T: Judge>`, `tests_v022_s4.rs:348`, `cli_run.rs:32212/32240`, `sealed_frames.rs:1985` `gm<T: Gm>`. Each calls a method its bound declares.
- Unbounded: 0.
- Both sweeps are expected identical.

**Fail-first test.** `generic_param_method_and_generic_struct_args_are_checked` (g_method, g_bounded_bad -> E0403; g_struct, g_struct_param -> E0102). Pinned refusal: g_unbounded_impl -> E0403 with the "add `T: Trait`" help. Controls: g_bounded_ok, g1_mk, g2_conc_field, g4_nested_app, g5_bounded_closure and the 8 generic-type fixtures stay rc 0.

**False-refusal risk.** [REVISED: CS2A-13] An unbounded `T` whose callers all pass a type with the method (g_unbounded_impl) is refused; the fix-it adds `T: Trait`. Tracked: 0. A trait method reached through a supertrait: Axon has no supertraits `[INFERENCE: no syntax found in compiler-phase3.md:47 type_param grammar]`. Default methods are declared in the trait, so they are found.

[REVISED: build CS2-5] As built: `current_generic_params` is a param -> bounds map (fn generics from `fn_bounds`, typedef/impl/trait generics with no bounds). A bound naming a trait the program does not declare leaves the call unchecked (conservative). A bounded `T` whose traits lack the method says "its bound `Shape` does not declare it" (the unbounded text is the spec's). `Type::App` lives in inference only: a generic fn's own signature keeps its `T` args as `TypeParam` inside `App` (`generalize_app_args`), and `CheckCtx::new`/`with_user_fn_sigs` erase `App("B", ..)` to the old `Deferred("B")` (`erase_app`), so no checker path changes; `codegen/types.rs` lowers `App` like `Deferred` (skipped) and `lsp.rs` prints `B<i64>` (both needed for the `codegen`/`serde-json` builds). The E0102 message names the mismatched argument ("expected i64, found str"). The 8 generic-type fixtures run byte-identical before and after; sweeps S290 and S596 are identical to the post-CS2-1 sweeps.

### CS2-6 (P3) Callees that are not a name - O-SF-04

**Problem.**
- `infer.rs:1336` computes `_callee_ty` and discards it.
- When there is no fn name, `infer.rs:1426-1430` infers the args and returns a fresh var.
- The checker's call checks (`checker.rs:4090-4131`, O-SF-01's `fn_value_type` at :705-743 and `check_args_against_sig` at :5652) run only for `Expr::Ident` callees.
- [REVISED: CS2A-17] Related, on the same path: a fn-typed param is typed as a top-level fn of the same name. `CS2A/o2_shadow_fn`: `fn apply(f: fn(i64) -> bool, v: i64) -> bool { f(v) }` plus a top-level `fn f(n: i64) -> i64` is E0307 "found i64"; without the top-level `fn f` (`o1_fnparam_bool`) it is rc 0 and prints `true`. A false refusal today.

**Repro:**

| probe | check | run |
|---|---|---|
| `n_index`: `let ops = [add]; ops[0](1)` | 0 | `lambda: expected 2 args, got 1` 101 |
| `n_call`: `mk()(1, 2, 3)`, with `mk() -> fn(i64, i64) -> i64` | 0 | `expected 2 args, got 3` 101 |
| `n_field`: `(h.f)(1, 2)`, with field `f: fn(i64) -> i64` | 0 | `expected 1 args, got 2` 101 |
| `n_type`: `ops[0](1, "x")` | 0 | `cannot apply Add to i64 / str` 101 |
| `n_ok` (control): `ops[0](1, 2) + mk()(3, 4)` | 0 | `10` |
| `CS2A/n2_hof_index` (control): a HOF-produced fn array and an if-selected fn | 0 | `12\|2` |
| `CS2A/o1_fnparam_bool` (control) | 0 | `true` |
| `CS2A/o2_shadow_fn` (control after the fix) | **2**, E0307 today | must become rc 0, `true` |

**Change.**
- In infer, when the callee is not a name, unify `_callee_ty` with `Fn(arg types, fresh ret)` and return the ret. An arity or type mismatch is E0102 at the call.
- In the checker, extend `fn_value_type` to `Index` of an array whose element type is `Fn`, to `Call` of a fn whose declared ret is `fn(..)`, and to `FieldAccess` of a fn-typed struct field.
- The `Call` arm then calls `check_args_against_sig` for any callee whose `fn_value_type` is known, which gives E0305 for arity and E0306 for an argument type, the same codes O-SF-01 gives for name callees.
- [REVISED: CS2A-17] A callee name that resolves to a local or param (in infer's scope and the checker's `scope`) uses that binding's type, never the top-level fn of the same name; the return-type check that gives o2's E0307 uses the param's declared `fn(..) -> bool`.

**Measured.** There is one non-name callee in tracked `.ax`: `crates/axon-core/tests/fixtures/vm_perf/big-compile.ax:20068` `table[n](n + off)`. It is valid and must stay rc 0. Both sweeps are expected identical (o2's shape, a fn param named like a top-level fn, has 0 tracked uses `[INFERENCE: not scanned separately; the sweeps would show any rc 2 -> 0 change, which is the correct direction]`).

**Fail-first test.** `a_call_through_a_non_name_callee_checks_its_signature` (n_index, n_call, n_field -> E0305; n_type -> E0306; n_ok, n2_hof_index, o1_fnparam_bool rc 0; o2_shadow_fn rc 2 -> rc 0, prints `true`).

**False-refusal risk.** An array of fns with different arities cannot type-check under unification anyway (an array literal's elements unify; `CS2A/n1_mixed` is already E0102), so no new refusal class arises.

[REVISED: build CS2-6] The local-callee rule also reaches a call to a local *lambda*: inference checks the call's argument kinds (number / `str` / `bool` / unit, through `[T]`/`Option`/`Result`/tuples) against the kinds the lambda's body gives its params, and never binds a param from a call (`let id = \|x\| x; id(1); id("s")` stays rc 0). This closes three check-then-panic programs in existing VM tests (`cli_run.rs` `vm_pure_bool_ops` `pure_bool_if`/`pure_bool_while`, `vm_superop_write_panics` `bool_index`: an `i64` passed where the body makes the param `bool`, or a `bool` used as an index), each of which panicked at run time; those tests now reach the lambda through an untyped wrapper `let g = \|x, y\| f(x, y)` so the VM panic path stays covered. Named residual: a lambda whose body passes its param to a builtin declared narrower than the run time accepts (`let g = \|x\| println(x); g(1)`) is now E0102; the same call written directly (`println(1)`) is already E0306, so this applies the language's existing rule. Both sweeps are unchanged by CS2-6.

### CS2-7 (P2) Enforce refinements on enum variant fields and lambda params - O-S1-15 (human-decided ENFORCE)

**Problem.**
- Interpreter, struct/enum literal (`interp/eval.rs:2692-2770`): the `RecordKind::Enum(enum_name, variant)` arm (:2699-2704) **returns the built value before** the Phase 5 construction checks (:2708 onward), which run only for `RecordKind::Struct`.
- Closures: `bind_closure_params` (`interp.rs:2795-2812`) binds without a check, and `ClosureCode` (`interp/sym.rs:282-300`) carries param names (`params: Box<[Sym]>`) but no types. A user fn's params are checked in `interp.rs:6182-6218`.
- Native: `emit_refine_preconditions` (`codegen/asi.rs:467`, called for fns at `codegen/mod.rs:1735`) is not called by `emit_lambda` (`codegen/expr.rs:3630`). `emit_refine_struct_checks` (`codegen/expr.rs:9245`) is not reached for `Enum::Variant {..}` literals (`emit_struct_lit` :9067, `name.contains("::")` :9073).
- Checker: `check_field_refinement` (`checker.rs:2056`, called at :4767) covers struct fields only, so `E::A { n: 0 }` with a constant is not E1209 (`r_variant_lit` rc 0, while `r_struct_lit` is E1209).
- Caps walker: `capabilities.rs:723` documents "refinements on enum variant fields (not checked at construction)". Its StructLit arm (:1763-1768) pushes `Walk::Type(name)` only when `hooks.structs` contains the name, so a variant literal `E::A {..}` does not walk the variant's field refinements. The Lambda arm (:1776-1782) already walks lambda param types and says so ("not checked by the interpreter today; followed anyway").

**Repro:**

| probe | check | run |
|---|---|---|
| `r_variant2`: `type E = A { n: Pos } \| B`, `E::A { n: 0 - 5 }` | 0 | `-5` rc 0 |
| `r_lambda`: `let f = \|x: Pos\| x * 2; f(0 - 5)` | 0 | `-10` rc 0 |
| `r_lambda_hof`: `arr_map([1, 0 - 2], \|x: Pos\| x * 2)` | 0 | `-4` rc 0 |
| `r_variant_lit`: `E::A { n: 0 }` (constant) | 0 | `0` |
| `r_lambda_lit`: `f(0)`, with `f = \|x: Pos\| ..` | 0 | `0` |
| `r_fn_ctrl` (control): `fn g(x: Pos)`, `g(0 - 5)` | 0 | `refinement violated: parameter x of g (= -5) ...` rc 6 |
| `r_struct_lit` (control) | rc 2, E1209 | - |

**Change** (I-2: interpreter, VM and native in one commit):
1. Interpreter, variant fields. In `eval.rs`, the Enum arm runs the per-field refinement checks that the Struct arm runs. Each variant field's declared type comes from the `enum_variant_fields` registry (the same one `infer.rs:1614-1620` reads). Violation: exit 6, "refinement violated: field `n` of `E::A` (= -5) violates the refinement `Pos`".
2. Interpreter, lambda params.
   - `ClosureCode` gains `param_refines: Option<Box<[Option<RefineId>]>>`, resolved when the lambda is lowered: `None` when no param is refined, so unrefined closures pay nothing.
   - `bind_closure_params` (`interp.rs:2795`) and the VM's closure entry check each refined param before binding: exit 6, "refinement violated: parameter `x` of lambda at L:C (= -5)".
   - The VM's fast-call decline (`interp/vm/compile.rs:757-758, 887`, `refine_preds_empty`) also declines closures with refined params.
3. Native: `emit_lambda` calls `emit_refine_preconditions` for refined params, and the enum-variant literal path calls `emit_refine_struct_checks` with the variant's fields.
4. Checker: `check_field_refinement` is also called for variant fields (constant -> E1209). `check_refinement_args` (`checker.rs:1905`) also runs when the callee's `fn_value_type` (CS2-6) is a lambda with refined params (constant arg -> E1209).
5. Caps walker: the StructLit arm pushes `Walk::Type(enum)` for a `E::V {..}` literal. The doc lines `capabilities.rs:723` and :1776-1777 are corrected. Every route the new runtime check runs (variant literal, closure call, closure passed to a builtin) is then walked. `verify_predicate_reaches` (:769) shares the maps and needs no change.

**Measured.** 0 tracked `.ax` files declare a refined enum variant field or a refined lambda param (scan of all 596). Both sweeps are expected identical.

**Fail-first tests.**
- `cli_run`: `refinements_on_variant_fields_and_lambda_params_are_enforced` (r_variant2, r_lambda, r_lambda_hof -> rc 6 under `axon run`; r_variant_lit, r_lambda_lit -> rc 2, E1209; r_fn_ctrl unchanged).
- `tests/caps_verb.rs`: `a_refined_variant_field_is_checked_at_construction_and_reached`, the twin of `a_refined_struct_field_is_checked_at_construction_and_reached` (:573-574).
- [REVISED: CS2A-12] Native: `axon-core --test integration_fixtures`, `refinements_on_variant_fields_and_lambda_params_are_enforced_natively` (pattern :2500; r_variant2 and r_lambda give rc 6 on both engines). Parity: one case each in `scripts/exit_code_parity.sh`.

**False-refusal risk.** None at check beyond constants (0 tracked). At run time only programs that violate a declared refinement change, which is the human's decision.

### CS2-8 (P2) Deep E1209: a refinement predicate may not reach an effect through a user fn - O-SP-02

**Problem.** `reject_impure_refinement` (`checker.rs:2848`) passes an empty `pure_fns` set to `collect_purity_violations` and keeps only the `"impure builtin"` kind (:2853-2866). A call to a user fn is never followed.

**Repro:**

| probe | check | run |
|---|---|---|
| `e_helper`: `fn leak(v) -> bool { let _ = append_file(P, "x"); true }`, `type W = i64 where leak(_)`, `f(1)` | **0** | `1`, and the file is written |
| `e_now`: helper `late` calls `now_ms()`, used in `where late(_)` | 0 | `1` |
| `e_direct` (control): `write_file` directly in the predicate | rc 2, E1209 | - |
| `e_pure_helper` (control): `fn pos(v) -> bool { v > 0 }` | 0 | `1` |
| `CS2A/e1_hof` (control): pure helpers using a lambda, a fn array, `assert`; `caps` reports nothing, `unknown:false` | 0 | correct |
| `CS2A/e3_fnparam` (control): a fn-param dispatch, `unknown:false` | 0 | correct |

`axon caps --json --fn leak e_helper.ax` reports `capabilities: ["fs:write"]` and `axon caps --fn late` reports `effects: ["Time"]`. The reach walker already knows. A direct `now_ms()` in a predicate is already E1209 (`CS2A/e2_now_direct`), so refusing Time through a helper is consistent.

**Re-measure of the capability-surface Q2 cost.**

`caps_verb.rs` tests that route a write through a refinement predicate with an effectful helper (`LEAK`, `type Lk = i64 where leak(_)`) and require `axon run` to perform it (`implicit_route`, :498-530):
- `a_refinement_predicate_at_a_let_annotation_is_reached` (:537);
- `..._at_a_helpers_parameter_is_reached` (:546);
- `..._on_the_return_type_is_reached` (:556);
- `an_inline_parameter_refinement_is_reached` (:565);
- `a_refined_struct_field_is_checked_at_construction_and_reached` (:574);
- `a_whole_struct_refinement_is_reached` (:584);
- [REVISED: CS2A-10] CS2-7's new `a_refined_variant_field_is_checked_at_construction_and_reached`, which uses the same `LEAK`.

That is **7** rewrites. `contained_refuses_a_write_reached_through_a_refinement_predicate` (:650) asserts check rc 2 containing E1001. It still gets rc 2; if E1001 is no longer emitted next to E1209 its assertion changes to E1209 (an 8th edit, `[INFERENCE: not run]`). `a_predicate_that_mentions_its_own_type_terminates_and_is_reached` (:669) uses the library API (`reachable_capabilities`) without check: unaffected. Inline-param and whole-struct refinements already go through `reject_impure_refinement` (`checker.rs:908`, :913; `CS2A/e6_inline_direct` is E1209), so the deep walk covers them.

Tracked `.ax` refinements that call a user fn: 3 (`examples/asi/world_model.ax:43` `fit_error`, the hardset copy, `examples/refinements.ax:32` `is_even`, `examples/stdlib/world.ax:45` `fit_error`). `axon caps --json --fn` reports `capabilities: []`, `effects: []` for all of them. Both sweeps are expected identical.

**Decision: include.** Q2 deferred this item to "a separate spec", and this is that spec. The 7 tests pin the walker's reach through refinement predicates. That reach stays pinned through the library API: each test keeps its `reachable_capabilities` assertion, the same form as :669, and its `axon run` half becomes an `axon check` assertion of rc 2 with E1209. The route is closed, so the test now pins the refusal. No capability coverage is lost, and M3926 is still killed by the library assertion.

**Change.** `reject_impure_refinement` walks each refinement predicate with the capabilities reach walker. The walker is the one `effects.rs:219` (`verify_predicate_offence`, CAP-1 E1104 [REVISED: CS2A-11]) already runs for `@[verify]` predicates, through `capabilities::verify_predicate_reaches`-style `WalkMaps` (`capabilities.rs:769-800`). Any reached capability or effect, `Time` included, is E1209: "refinement `W` reaches `append_file` (fs:write) through `leak`; a refinement predicate must be pure". Refinements are stricter than verify predicates, which allow a clock read.
- `reach.unknown` (a computed dispatch) is also E1209, deny by default.
- Measured: 0 tracked predicates reach unknown.
- The shallow builtin scan stays as the first pass, so `e_direct`'s message is unchanged.

**Alternative.** Treat only `@[pure]`-marked helpers as callable. That refuses the 3 pure tracked helpers (false refusals), so it is rejected.

**Fail-first test.** `cli_run`: `an_effectful_helper_in_a_refinement_is_refused_e1209` (e_helper, e_now, and an unknown-dispatch probe -> rc 2, E1209; e_pure_helper, e1_hof, e3_fnparam and `examples/refinements.ax` rc 0).

## 3. Error codes

The only new code is **E0607** (CS2-1). Everything else reuses an existing code:
- E0102 (CS2-3, CS2-5, CS2-6 infer);
- E0304 (CS2-4);
- E0305/E0306 (CS2-6, CS2-3 numeric key);
- E0403 (CS2-5);
- E1209 (CS2-7, CS2-8);
- runtime exit 6 / 101 (CS2-7, CS2-2).

E0607 is registered at the four sites listed in CS2-1. E0304's description widens (`error.rs:454`, and `AXON_REFERENCE.md:164` by regeneration) in CS2-4's commit.

## 4. g01 rows (M3890-M3929), refusal coverage, pci

**Id block.** [REVISED: CS2A-12] M3890-M3909 was reserved for GateLane (`tasks/build-loop-gate-lane.md:52`: "The GateLane block M3890-M3909 stays unused"); GateLane shipped with no rows, so the block is released here. The script's last comment says "M3878-M3889 spare" (`v022_g01_mutations.py:12084`), but M3878-M3880 are used by SND; the highest used id is M3880. M3930-M3939 belong to the docs-truth spec. This spec uses exactly M3890-M3929.

Row rules are as in `tasks/checker-soundness-spec.md` §4:
- one 8-tuple per row in `scripts/v022_g01_mutations.py` (format :12118-12124): id, description, **one** file, unique `old`, `new`, crate, target, **one** killing test (a cargo test; a script is never a killer);
- one `ATTACK_MARKERS` entry each (`scripts/v022_attack_markers.py`, format :3283-3300), whose `r'ATTACK: ... \[probe\]'` text appears in the killing test's assert message;
- `python3 -B scripts/g01_anchors.py` reports each `old` exactly once.

The exact `old` is the guard line the build writes, so the builder fixes it at implementation time. The guard each row removes is fixed here. Targets: cli_run `'--no-default-features --test cli_run'`; native rows `'--test integration_fixtures'` (crate `axon-core`), as M3830 (`v022_g01_mutations.py:11901-11904`); a row whose file is in `axon-rt` but whose killer is an `axon-core` test uses crate `axon-core`, as M3724 (:11586-11588); the `builtins.rs` unit row uses `'--no-default-features --lib'` with `builtins::tests::...`, as M58 (:385-390).

[REVISED: CS2A-1/5/8/9/12] The table was re-cut so the review's new guards fit in 40 ids. Guards with no row are covered by their fail-first test only: `random_i64` i128 and `str_repeat` checked_mul (a regression shows as a Rust panic, which the test's message assertion catches), `DEFERRED_PREFIXES` (cutover), `promote_type_params` (redundant with M3903), the CS2-1 `AssignTo` root (c1_field in the test), CS2-4's binding-arm catch-all (x_int_bind control), CS2-5's literal instantiation (redundant with M3918), and CS2-6's local-before-top-level rule (o2 control).

| id | item | guard the mutation removes | file | killing test |
|---|---|---|---|---|
| M3890 | CS2-1 | record a written capture by binding id | mut_borrow.rs | `closure_write_to_a_capture_read_outside_is_refused_e0607` |
| M3891 | CS2-1 | a later use of a recorded binding id emits E0607 | mut_borrow.rs | same (c1_two marker) |
| M3892 | CS2-1 | loop-literal rule 2(b) | mut_borrow.rs | same (c1_loop marker) |
| M3893 | CS2-1 | a redefinition or scope pop drops the record; params and closure-local `let`s are not captures (false refusal) | mut_borrow.rs | `closure_counter_and_local_writes_stay_legal` (f2/f3 markers) |
| M3894 | CS2-2 | `pow_i64` checked_pow | interp/builtins.rs | `arithmetic_builtins_overflow_like_the_operators` |
| M3895 | CS2-2 | `pow_i64` exponent > u32::MAX range guard | interp/builtins.rs | same (a_pow_exp marker) |
| M3896 | CS2-2 | `arr_sum_i64` checked_add | interp/builtins.rs | same (a_sum marker) |
| M3897 | CS2-2 | `arr_sum_by` checked_add | interp/builtins.rs | same (a_sumby marker) |
| M3898 | CS2-2 | `arr_mean_i64` i128 accumulator | interp/builtins.rs | same (a_mean2 marker) |
| M3899 | CS2-2 | `dict_inc` checked_add | interp/builtins.rs | same (a_dictinc marker) |
| M3900 | CS2-2 | native `__axon_pow_i64` checked | axon-rt/src/lib.rs | `migrated_pow_i64_matches_interpreter` (axon-rt `--lib`, oracle rewritten) [REVISED: CS2A-11] |
| M3901 | CS2-2 | native `ArrReduce::Sum`/`Mean` accumulator checked / i128 | codegen/expr.rs | `arithmetic_builtins_overflow_natively` |
| M3902 | CS2-2 | native `__axon_dict_inc` checked | axon-rt/src/lib.rs (crate `axon-core`) | same |
| M3903 | CS2-3a | `parse_type_str` `fn(` arm | infer.rs | `closure_passed_to_a_builtin_is_type_checked` |
| M3904 | CS2-3a | `Num` key var must resolve numeric | infer.rs | same (l_maxby_b marker) |
| M3905 | CS2-3a | a unit-return fn param leaves the closure's return free (false refusal) | infer.rs | `generic_and_annotated_closures_to_builtins_stay_accepted` (h10 marker) |
| M3906 | CS2-3b | `Dict` parses to `Dict(V)` | infer.rs | `dict_values_are_typed` (d_get marker) |
| M3907 | CS2-3b | `Dict<str, V>` annotation resolves to `Dict(V)` | infer.rs | same (d_ann marker) |
| M3908 | CS2-3b | a bare-`Dict` signature var is solved from the body before callers | infer.rs | same (d_param_bare marker) |
| M3909 | CS2-3b | `Dict` vs `Dict` unifies the value types | infer.rs | same (d_mixed marker) |
| M3910 | CS2-3b | `is_heap_allocating_builtin` recognizes `Dict<..>` rets | builtins.rs | `builtins::tests::r17_heap_alloc_classifier_is_sound_for_hal_and_strings` |
| M3911 | CS2-4 | scalar scrutinee needs a catch-all | checker.rs | `non_exhaustive_scalar_and_tuple_matches_are_refused` (x_int) |
| M3912 | CS2-4 | bool needs both literals | checker.rs | same (x_bool_half) |
| M3913 | CS2-4 | tuple product over component domains | checker.rs | same (x_tuple) |
| M3914 | CS2-4 | a guarded arm does not count | checker.rs | same (x_opt_guard) |
| M3915 | CS2-4 | `Option`/`Result` component domain (false refusal) | checker.rs | same (x1/x6 controls) |
| M3916 | CS2-5 | unbounded generic receiver -> E0403 | checker.rs | `generic_param_method_and_generic_struct_args_are_checked` (g_method) |
| M3917 | CS2-5 | bounded receiver: method looked up in the bound traits | checker.rs | same (g_bounded_bad) |
| M3918 | CS2-5 | `App` unify compares args | infer.rs | same (g_struct) |
| M3919 | CS2-6 | infer unifies a non-name callee with `Fn(args, ret)` | infer.rs | `a_call_through_a_non_name_callee_checks_its_signature` (n_type) |
| M3920 | CS2-6 | `fn_value_type` on `Index` | checker.rs | same (n_index) |
| M3921 | CS2-6 | `fn_value_type` on `Call`/`FieldAccess` | checker.rs | same (n_call) |
| M3922 | CS2-7 | interp Enum arm runs field refinements | interp/eval.rs | `refinements_on_variant_fields_and_lambda_params_are_enforced` (r_variant2) |
| M3923 | CS2-7 | `bind_closure_params` checks refined params | interp.rs | same (r_lambda) |
| M3924 | CS2-7 | VM declines refined closures from its fast path | interp/vm/compile.rs | same (r_lambda_hof) |
| M3925 | CS2-7 | checker variant-field constant -> E1209 | checker.rs | same (r_variant_lit) |
| M3926 | CS2-7 | caps walker pushes the enum for `E::V {..}` | capabilities.rs | `a_refined_variant_field_is_checked_at_construction_and_reached` |
| M3927 | CS2-7 | native `emit_lambda` preconditions | codegen/expr.rs | `refinements_on_variant_fields_and_lambda_params_are_enforced_natively` [REVISED: CS2A-12] |
| M3928 | CS2-8 | deep reach walk in `reject_impure_refinement` | checker.rs | `an_effectful_helper_in_a_refinement_is_refused_e1209` (e_helper) |
| M3929 | CS2-8 | `reach.unknown` refused | checker.rs | same (unknown-dispatch probe marker) |

M76 is rewritten under CS2-2 (its `old` line changes). That is not a new id.

**Refusal coverage.** After CS2-1, CS2-4 and CS2-8 land, run `python3 -B scripts/v022_refusal_coverage.py`, and add a row or an exemption for any new refusal site it names (seconds).

**pci.** [REVISED: CS2D-5] `scripts/pci_delta.py` (:1-20, :365-367) requires a `governance/notes/pci-themes/<hash8>.json` for every `crates/axon-core/src` commit and for every merge that is not a pure join (`PURE_JOIN`, :84); the note goes stale after any later src commit, and the line "N rows at this head" (`governance/notes/v022-pci-delta.md:591`, "530 rows at this head") is recomputed from the script. 38 rows above are under `crates/axon-core/src` (all except M3900 and M3902, which are in `axon-rt`), so the line becomes **568** and the range list gains `M3890-M3899, M3901, M3903-M3929`, in the same commit as the rows. Theme files are keyed by hash8, so they are written by the integrator only after the last merge and rebase (one per src commit plus one per non-pure-join merge). Then `python3 -B tasks/r51-pci-regen.py` (it installs the `merge=pci-note` driver) and `python3 -B scripts/pci_delta.py --check`, once. Lanes do not run `pci_delta --check` as a pass/fail check; it must fail on lane commits by construction.

## 5. Gates and cost budget (no paid spend)

The sweep is defined two ways:
- **S290**: `/var/tmp/r51-tmp/cs-sweep.sh <repo> <bin> <out>`. Baseline `/var/tmp/r51-tmp/sf-sweep-base.txt`: 290 files, rc0=274. Re-run on main-bin: IDENTICAL, 2.65 s (CS2A re-ran it: identical).
- **S596**: `/var/tmp/r51-tmp/CS2Draft/cs-sweep-all.sh`. The same loop over every `git ls-files '*.ax'`. Baseline `/var/tmp/r51-tmp/CS2Draft/sweep-all-base.txt`: 596 files, rc0=512, other=84, 6.0 s (CS2A re-ran it: byte-identical). The integrator copies the script and baseline into the build harness; they are not committed here.

Expected deltas (everything else identical):
- S290: `examples/asi/safe_bandit.ax` would go rc 0 -> 2 under NH-1(a). It is rewritten in CS2-3b's commit, so S290 stays identical. Under the NH-1 fallback it is untouched.
- S596:
  - `drop_array`/`drop_option`/`drop_result`/`drop_tuple.ax` stay rc 0 after their rewrite (NH-1(a)); untouched under the fallback;
  - `r51_s0b/leaf_sum_confusion.ax` goes rc 0 -> 2 (E0102) under NH-1(a), which is correct: it is the closed hole; unchanged under the fallback;
  - `vm_match/nomatch.ax` goes rc 0 -> 2 (E0304), which is correct: a non-exhaustive match.

[REVISED: CS2D-1/2/3/4] Timings below are measured unless marked.

| gate | one-run cost | trigger | skip rule |
|---|---|---|---|
| S290 + S596 | ≈ 9 s total, offline (measured) | every checker/infer/mut_borrow commit, and after 3a and again after 3b | never. Pass = identical except the named files |
| targeted `cargo test -p axon-core --no-default-features --test cli_run -- <names>` | ≈ 15-60 s incremental `[INFERENCE from g01 cells: incremental rebuild + one filtered test in 8-22 s]`; minutes when the target dir is cold. Full cli_run: 276.6 s (`gl-gate-2.log`, 900 passed) | each item | never |
| reference drift test `cli_run.rs:25737` (`axon_reference_is_in_sync_with_the_binary`) | inside the targeted run | every item (each changes a sig, a doc or `ALL_CODES`) | never |
| `cargo test -p axon-core --test caps_verb` | `[unmeasured]` | CS2-7, CS2-8 | never for those |
| `--lib interp::vm::tests`, `--lib builtins::tests` | `[unmeasured]` | CS2-2, CS2-7 / CS2-3b | never for those |
| `cargo test -p axon-core --test integration_fixtures -- <native names>` and `cargo test -p axon-rt --lib migrated_pow` | `[unmeasured]`, needs codegen | CS2-2, CS2-7 | never for those |
| fresh codegen build + `AXON_HARNESS_STRICT=1 scripts/exit_code_parity.sh`, `checked_arith_parity.sh`, `fuzz_parity.sh`, `arr_reduce_parity.sh` | `[unmeasured]` | CS2-2, CS2-7 (both touch codegen/axon-rt) | never for those |
| `scripts/vm_wasm_depth.sh`, `wasm_nesting_parity.sh` | `[unmeasured]` | CS2-3b (drop fixtures rewritten under NH-1(a)) | never for 3b |
| per-lane hand-off [REVISED: CS2D-1]: the item's targeted `cargo test` names, `python3 -B scripts/g01_anchors.py`, `python3 -B scripts/v022_refusal_coverage.py` | ≈ 1-2 min | once per item before hand-off | not evidence |
| g01 lane feedback `python3 -B scripts/v022_g01_mutations.py --only=<item ids> OUT.json` [REVISED: CS2D-3] | ≈ 10-25 s per cli_run row (measured on the snd run, `/var/tmp/v022-cells/cell-M387*`); one item's 2-9 rows ≈ 1-3 min. Native/axon-rt rows `[unmeasured]` | after each item's commit | not evidence |
| `gate.sh --lane` [REVISED: CS2D-2] | not used: the stored audit (0b29b054) is older than the base and `gate.sh` changed since, so `--plan` selects 422 units, and its footer skips codegen-gated tests and the parity suite, which CS2-2/CS2-7 need. A lane that wants it runs `python3 -B scripts/gate_lane.py audit` at 4a6436da first (`[unmeasured]`, slow) | - | - |
| integrator finish: rows + markers + CHANGELOG, theme files, `r51-pci-regen.py` + `pci_delta --check` (≈ 10 s observed), refusal coverage, reference drift, `spec_ledger.py` | ≈ 5 min | once, after the last merge | never |
| **g01 `--only=M76,M3890-M3929`** | ≈ 10-15 min serial (41 rows at the observed rate) plus a baseline build; native rows `[unmeasured]` | once at the integrated head | never |
| **`./scripts/gate.sh --strict`** [REVISED: CS2D-4] | 38-46 min (`gl-gate-2.log`, `snd-gate-6.log`; trust-root preflight alone 659 s); one retry doubles it to ≈ 1.5 h | once at the integrated head; required because CS2-2 and CS2-7 touch codegen/axon-rt | never |
| cortex lane `cargo test -p axon-cortex --features ai` | 10-15 min | only if a benchmark program's check rc changes (none expected) | skip otherwise |
| paid receipt regress | ≈ $0.14 | not triggered: no `axon-cortex/src` or driver change | skip; **$0 paid spend budgeted** |

Steps whose cost could double: a strict-gate retry from reference or pci drift (+40 min; the finish row's pre-checks prevent it); theme files written before the last rebase; CS2-3b waiting on NH-1; a false refusal found by the 3a sweep (rework of 3a); CS2-8's rebase onto CS2-7's `caps_verb.rs`.

## 6. Sequencing and lanes

Two lanes, split by file ownership:

| lane | items, in order | files |
|---|---|---|
| **A: checker** | [REVISED: CS2D-8] CS2-4 -> CS2-6 -> CS2-3a -> CS2-1 -> CS2-5 -> CS2-3b -> CS2-8 | `checker.rs`, `infer.rs`, `types.rs`, `mut_borrow.rs`, `error.rs`, `builtins.rs` (sig strings, `is_heap_allocating_builtin`), `interp.rs:10395` / `interp/pin.rs` / `interp/taint_tests.rs` / `codegen/types.rs` / `codegen/builtin_externs.rs` / `codegen/builtins.rs` (the `"Dict"` readers, 3b only), `capabilities.rs` (CS2-8 read path), fixtures for 3b and CS2-4 |
| **B: runtime** | CS2-2 -> CS2-7 | `interp/builtins.rs`, `interp/eval.rs`, `interp.rs`, `interp/sym.rs`, `interp/vm/*`, `codegen/expr.rs`, `codegen/asi.rs`, `codegen/builtins.rs` (`random_i64`), `axon-rt/src/lib.rs`, parity scripts, `capabilities.rs` (StructLit arm), `builtins.rs` docs (:250, :609, `str_repeat`, `random_i64`), `checker.rs` (CS2-7 :1905/:2056/:4767) |

Order rationale and dependencies:
- CS2-4 and CS2-6 are small and decision-free, so they land first.
- CS2-6 precedes CS2-3a, because both edit the infer `Call` arm (`infer.rs:1336`, :1426) and the S596 sweep after 3a then measures both together. 3a runs early because it carries the highest false-refusal risk.
- CS2-5 -> CS2-3b (`Type::App` for `Dict<str, V>` annotations and for struct implicit params), and 3a -> 3b (`fn(` parsing). CS2-5 and 3b both add `Type` variants: serial.
- CS2-1 is independent (mut_borrow and error.rs), and it is the only new code, so error.rs edits stay serial.
- CS2-3b waits for NH-1.
- CS2-7 needs CS2-6 (`fn_value_type` for lambda callees, step 4). CS2-8 needs CS2-7 (the `caps_verb.rs` twin, CS2A-10), so CS2-8 waits for the whole of lane B [REVISED: CS2D-9].

| step | owner | depends on | cost |
|---|---|---|---|
| A1 CS2-4, A2 CS2-6 | A | - | §5 per-item rows |
| merge A1-A2 | integrator | - | conflict checks below, ≈ 1-2 min |
| B1 CS2-2 | B | - (parallel with A) | targeted + vm tests + native tests + parity scripts + reference regen |
| merge B1 | integrator | A2 merged | regenerate `AXON_REFERENCE.md`, never hand-merge |
| B2 CS2-7 | B | rebased on A2 | as B1, plus `caps_verb` |
| A3 CS2-3a -> A4 CS2-1 -> A5 CS2-5 | A | A2 | per item; S596 identical after 3a |
| A6 CS2-3b | A | NH-1, A3, A5 | per item, plus `vm_wasm_depth`/`wasm_nesting_parity` |
| **gate:** merge B2 | integrator | B2 green | - |
| A7 CS2-8 | A | B2 merged (rebased onto its `caps_verb.rs`) | per item, plus `caps_verb` |
| integrator finish | integrator | everything | §5 |
| g01 `--only`, then `gate.sh --strict` once | integrator | finish | §5 |

[REVISED: CS2D-7] Files both lanes touch: `checker.rs`, `builtins.rs`, `AXON_REFERENCE.md`, `capabilities.rs`, `tests/caps_verb.rs`, `tests/cli_run.rs`, `tests/integration_fixtures.rs`, `CHANGELOG.md`, the g01 scripts. Conflict rules:
1. The integrator alone appends to `v022_g01_mutations.py`, `v022_attack_markers.py`, `CHANGELOG.md` and the pci note; lanes send row text, marker text and changelog lines in their reports.
2. In `cli_run.rs`, lane B puts its tests right after the renamed `arr_sum_by_..._panics_like_the_operator` (:14600) instead of at the end of the file; lane A appends at the end. In `integration_fixtures.rs`, lane B appends after :2500's test; lane A does not edit it.
3. After every merge that conflicts in an appended file: check that the file still ends with its closing `]`/`})`/`}`; run `python3 -B -c "import ast;ast.parse(open('scripts/v022_g01_mutations.py').read());ast.parse(open('scripts/v022_attack_markers.py').read())"` and `python3 -B scripts/g01_anchors.py`; run `cargo test -p axon-core --no-default-features --test cli_run --no-run`. (The closing-bracket loss has happened three times.)
4. [REVISED: CS2D-6] `AXON_REFERENCE.md` is never hand-merged. Every commit that changes a builtin sig, a builtin doc or an `ALL_CODES` row regenerates it with `cargo run -p axon-core --bin axon -- reference > AXON_REFERENCE.md`; at a merge conflict, regenerate from the merged binary. Lane B's `builtins.rs` doc edits are a lane-B edit to a lane-A file: merge serially.
5. CS2-7's `checker.rs` edits (`check_field_refinement` :2056/:4767, `check_refinement_args` :1905) are small and in regions lane A does not touch. `capabilities.rs`: B edits the StructLit arm (:1763); A's CS2-8 calls the walker without editing it.

**Docs-truth integration** (from DevloopDocs DTD-3/DTD-4, accepted by the integrator). Docs-truth merges first; it adds `scripts/spec_ledger.py` (a full-gate stage) and `crates/axon-core/tests/doc_snippets.rs` (S16). So CS2's integration must:
1. after the merge, in a follow-up commit (so `<sha>` is an ancestor of HEAD), stamp O-GAP-01..06, O-SF-04, O-S1-15 and O-SP-02 closed in `tasks/opportunities.md` with a closure word `spec_ledger.py` accepts (`[BUILT ...]` or `[FIXED ...]`), and set the status to `**Status: BUILT <date> on `main` <sha>**`. O-GAP-07 is not built here and stays open (§0): `spec_ledger.py` accepts no "deferred" word, and its rule 5 checks only O-ids in a spec's Source paragraph (first 40 lines) and in `#`-headings, where this spec never names O-GAP-07. Pass `python3 -B scripts/spec_ledger.py` rc 0. Item ids `CS2-n` are unique across `tasks/*-spec.md` (ledger rule 0; checked by DocsDraft);
2. fix any doc snippet a new CS2 refusal breaks (`doc_snippets` failing in S16) in the same lane as the refusal;
3. whichever of docs-truth and CS2 lands second regenerates `AXON_REFERENCE.md` and re-runs `python3 -B tasks/r51-pci-regen.py`; the pci note is written last.

## 7. Engineering decisions (decided here)

| id | question | decision | why |
|---|---|---|---|
| E-1 | CS2-1: refuse vs by-reference capture | narrow refusal (E0607) when the divergence is observable, keyed by binding id | keeps C02 and T40; no engine change; f1/f5-shaped conservative refusals pinned, 0 tracked |
| E-2 | `assert_eq_f64` | exact IEEE on all engines | matches stdlib.md:516 and native; 0 tracked tests rely on the tolerance; removes a live tree/native divergence (`t_near`) |
| E-3 | overflowing arithmetic builtins | panic like the operators; `arr_mean_i64` and `random_i64` exact via i128; `pow_i64` exponent range checked; `str_repeat` checked size | one overflow rule; no Rust panic reaches the user |
| E-4 | HOF typing | parse `fn(..)` in builtin sigs and unify in infer; a unit-return fn param leaves the closure's return free; `Num` for numeric keys incl. `arr_sum_by_f64` | uses existing instantiation; fold's `U` constraint crosses args; keeps h10, l_sumbyf_i, l_numkey |
| E-5 | bare `Dict` in a signature [REVISED: CS2A-2] | value var solved from the fn body (callees first; flexible, never rigid); generalized per call if the body leaves it free; struct fields are implicit struct params | closes d_param_bare; keeps h6, h7, corrigible.ax, set.ax |
| E-6 | CS2-8 include vs defer | include; rewrite 7 `caps_verb` bodies to assert the refusal and keep the library reach assertion | this is the separate spec Q2 asked for; no reach coverage lost |
| E-7 | guarded arms in exhaustiveness | do not count | a guard can be false; 0 tracked reliance |
| E-8 | tuple exhaustiveness [REVISED: CS2A-1] | product over per-component domains (bool, enum, Option, Result; other types only by wildcard/binding), cap 256 | accepts every exhaustive tuple shape found (x1-x8) |
| E-9 | unbounded generic method calls [REVISED: CS2A-13] | refuse, fix-it adds the bound | a generic fn's validity must not depend on its callers; 0 tracked |
| E-10 | `Dict` sig strings [REVISED: CS2A-8] | keep the literal `Dict`; value named `V`; readers of `"Dict"` migrate to one helper | the string is read by 6 other subsystems |

## 8. Needs human (product / irreversible only)

- **NH-1. Is a `Dict` homogeneous in its value type?** (CS2-3b)
  - `[DECIDED by human 2026-10-29: (a) - a Dict holds one value type; mixed dicts are refused (E0102); the 6 tracked files change in CS2-3b's commit.]`
  - (a) Yes: one value type per dict, inferred or annotated. A mixed dict is refused (E0102). Exactly 6 tracked files change, all in CS2-3b's commit:
    1. `examples/asi/safe_bandit.ax`: the `Agent` moves out of `state` into a `let` rebound each round; `state` keeps only the i64 counters (`Dict<str, i64>`). Output unchanged; S290 row unchanged.
    2-5. `crates/axon-core/tests/fixtures/vm_depth/nest/drop_array.ax`, `drop_option.ax`, `drop_result.ax`, `drop_tuple.ax`: the value they nest through the dict becomes a one-line recursive enum, so the dict holds one type; DEPTH and chain length unchanged; `vm_wasm_depth.sh` and `wasm_nesting_parity.sh` re-run. S596 rows stay rc 0.
    6. `crates/axon-core/tests/fixtures/r51_s0b/leaf_sum_confusion.ax`: kept as the regression for the closed hole; it becomes rc 2, E0102, and `cli_run.rs:40069` asserts the refusal on both engines.
  - (b) Typed when the writes agree; a dict whose writes conflict silently stays dynamic. Unsound by construction, because the reads of a conflicted dict stay unchecked.
  - (c) Enforce only explicit `Dict<str, V>` annotations.
  - **Recommendation: (a).** An AI author cannot tell from a dict's type what `dict_get` returns, and that is exactly the check-then-panic class. The cost is 6 files and one re-justification of the AX-59 probes.
  - **Fallback if (a) is declined: (c).** CS2-3a lands unchanged. CS2-3b builds `Type::Dict(V)`, the `Dict<str, V>` annotation and the sig-string/reader migration, but an unannotated dict and a bare `Dict` in a signature keep a fresh value var per read (today's behaviour). Closed: `d_ann`, and any program that annotates. Still open and documented in stdlib.md: `d_get`, `d_param_bare`, `d_mixed`. 0 tracked files change (none of the 6 annotates); `leaf_sum_confusion.ax` stays a run-time test. g01: M3908 guards "an annotated `Dict<str, V>` param is enforced at the call" and M3909 "an annotated dict refuses a write of another type" (same ids, same killer).

No other choice in this spec is product or irreversible. (O-GAP-07, the former NH-2, is out of scope, §0.)
