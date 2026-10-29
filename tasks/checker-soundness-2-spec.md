# checker-soundness-2 - DRAFT spec

**Status: DRAFT. Not authorized for build until Step 1 review.**

Base: `main` 4a6436da. Every anchor below was read at 4a6436da in `/home/cklaus/projects/axon-lanes/cs2`. Every reproduction was run with
`/var/tmp/r51-tmp/main-bin/axon`. That binary was built at 0bd2b518, and `crates/` did not change between that commit and 4a6436da. Each probe was run as `axon check F` and then `axon run F`, with `AXON_PATH=examples/stdlib`. The probe sources are in
`/var/tmp/r51-tmp/CS2Draft/*.ax`. Their outputs are in `/var/tmp/r51-tmp/CS2Draft/ALL.txt` and their one-line sources in `SRC.txt`.
`[INFERENCE]` marks anything that was not observed.

The defect class is the same as checker-soundness: **a program that `axon check` accepts (rc 0) and that then panics or computes a wrong answer at run time.**

## 0. Source entries

| item | source (`tasks/opportunities.md`) | one line |
|---|---|---|
| CS2-1 | O-GAP-03 (:2853) | A closure's write to a captured binding is silently dropped |
| CS2-2 | O-GAP-06 (:2866) | Arithmetic builtins wrap or saturate where the operators panic, and `assert_eq_f64` has three different meanings |
| CS2-3 | O-GAP-01 (:2840), O-GAP-02 (:2845) | Closures passed to higher-order builtins are only arity-checked, and `Dict` values are untyped |
| CS2-4 | O-GAP-04 (:2858) | Exhaustiveness is checked only for enum, `Option` and `Result` scrutinees |
| CS2-5 | O-GAP-05 (:2862) | A method call on an unbounded generic param is accepted, and generic struct arguments are not enforced |
| CS2-6 | O-SF-04 (:2788) | A callee that is not a name skips the arity and type checks |
| CS2-7 | O-S1-15 (:2638), `[DECIDED by human 2026-10-18: ENFORCE]` | Refinements on enum variant fields and on lambda params are not checked |
| CS2-8 | O-SP-02 (:2723), `tasks/capability-surface-spec.md` Q2 (:341, :351) | E1209 (impure refinement) is shallow: wrapping the effect in a user fn hides it |

### Out of scope (logged, not dropped)

- **O-GAP-07** (`:2872`). Plain recursion stops at depth 6000, and `arr_max_i64([])` panics instead of returning an `Option`. Changing a builtin's return type is an API change, so it is deferred to its own spec. See NH-2.
- **O-GAP-08** (`:2876`). Common language forms are refused: `if let`, `Ok(())`, `for (a, b) in pairs`, `impl T {}`, `?` on `Option`, `loop`, tuple-variant enums. These are language-surface additions, not soundness fixes, so they get their own spec.
- **O-GAP-09..11.** Not soundness items. They stay in opportunities.md.
- **Shift semantics.** `shl(1, 64)` and `1 << s` with `s = 64` both print `1`, because the shift count is masked (`interp/value.rs:638`, `wrapping_shl`). The builtin and the operator agree, so there is no skew for CS2-2 to fix. Whether an over-wide shift should panic is a language question. It is not logged here and needs its own opportunity entry.

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
| `c1_factory` (control) | `fn mk() -> fn() -> i64 { let n = 0; \|\| { n = n + 1; n } }`, then `c(); c()` | `2`: correct counter semantics |
| `c1_shadow` (control) | `\|n: i64\| { n = n + 1; n }`. The param is not a capture | `2`, then outer `0`: correct |
| `c1_shadow_let` (control) | `\|\| { let n = 7; n = n + 1; n }` | rc 0, W0002 shadow note; correct |
| `c1_toplet` (control) | the closure writes a top-level `let` | already rc 2, E0001 |

**Decision: refuse at check when the divergence is observable. Keep the copy-capture semantics.**

The copy semantics and a by-reference semantics give the same observable result in exactly one case: when nothing outside the writing closure can observe the binding after the closure is created. The counter and factory pattern (`c1_factory`, and the T40 tests) is that case, and it stays legal.

New code **E0607**: "a closure assigns captured `x`, but `x` is used outside that closure after it is created; the write is not visible there". The fix-it reads: "closures capture by copy. Return the new value from the closure, or keep the state in one closure."

A write W inside lambda L to name X is refused when all three hold:
1. X resolves below L's `closure_floor`, i.e. it is captured (`lookup().1 == true`). A lambda param or a `let` inside L is never captured, which keeps `c1_shadow`, `c1_shadow_let` and every closure-local `let` assignable. W covers `Assign`, and `AssignTo` whose `place_root` (`mut_borrow.rs:131`) is X, which covers fields and indexes.
2. Either of these:
   - (a) after L's literal, within the scope that declares X, the same binding is referenced outside L: read, assigned, place-assigned, or captured by another lambda (the c1_two case);
   - (b) L's literal sits inside a `while`/`for`/`loop` body that does not contain X's declaration. The back-edge re-captures X, so the literal itself is the later read (`c1_loop`).
3. Nesting composes. An inner lambda's capture of the outer lambda's local (`c1_nested_local`), or of the outer lambda's capture (`c1_nested`), is judged in the outer lambda's body scope.

Implementation lives in the `mut_borrow` walker. It owns scopes and the `closure_floor` already, and runs once per fn body (`checker.rs:1159`).
- On leaving a lambda, record `(scope index, name) -> lambda span` for every captured name the lambda wrote.
- `lookup` hits on a recorded entry at a later point emit E0607 at the later use. The help text names the lambda's line.
- A loop-depth stack (scope length at each loop entry) gives rule 2(b).

**Alternatives.**
- (i) Refuse every write to a capture. This breaks the counter pattern that T40 pins, and 27 Rust-embedded test programs write captures (measured scan, list below).
- (ii) Real by-reference capture: a shared cell for the outer binding too. This changes the T40 semantics on three engines (tree, VM, native closure env layout, `codegen/expr.rs:3630` `emit_lambda`). It would also contradict C02. It is the larger change and would remain open.
- (iii) Warn instead of refusing. A warning does not stop the wrong answer.

The chosen rule refuses only programs whose output would differ under the semantics the author probably meant.

**Error-code registration** (the four sites, as in checker-soundness CS-1):
- `pub const E0607` after `error.rs:69`;
- an `ALL_CODES` row after `error.rs:483`;
- the `error_codes_are_unique` list at `error.rs:757`;
- `AXON_REFERENCE.md` after :193 (drift guard `cli_run.rs:25750`).

`git grep E0607` is empty at 4a6436da. `a_code_not_marked_reserved_is_actually_emitted` (`error.rs:665`) needs the emit site to land in the same commit.

**Tracked impact (measured).**
- Tracked `.ax` files: **0** closures write a captured name (my scan over the 596 `git ls-files '*.ax'`, validated on the probes). Both the 290-file and the 596-file sweeps stay identical.
- Rust-embedded programs: 27 closure-writes-capture hits. These are `interp/sink_tests.rs:533`, `interp/taint_tests.rs:18` (3), `mut_borrow.rs:806`, `cli_run.rs:29331, 35413, 35500 (3), 35589, 35606 (2), 35729, 35859, 37307, 37354`, and `integration_fixtures.rs:199, 219`. Hits in `axon-cortex/src/locate.rs:107` are Rust code, a scanner false positive.
  - Those that read the outer name afterwards (e.g. `integration_fixtures.rs:199` prints `outer={n}`, and `cli_run.rs:35500` prints `txt`) become E0607 **only if their harness runs the checker**. `integration_fixtures.rs` calls `run_program_capturing`, the interpreter without check `[INFERENCE: not traced to the end]`.
  - The `cli_run` cases go through `axon run` (`vm_run`, `cli_run.rs:36928`). That runs check first `[INFERENCE]`.
  - The builder runs each named test. A test that pins the outer value after a write is rewritten to read through the closure: what it pinned is the divergence. The T40 persistence assertion (`1,2,3`) stays.

**Fail-first test.**
- `axon-core`, `--no-default-features --test cli_run`, `closure_write_to_a_capture_read_outside_is_refused_e0607`.
- Inputs: c1_cap, c1_two, c1_nested, c1_nested_local, c1_field, c1_index, c1_loop. Each gives rc 0 today and must give rc 2 with E0607.
- Companion: `closure_counter_and_local_writes_stay_legal` (c1_factory, c1_shadow, c1_shadow_let) stays rc 0 with the same output.

**False-refusal risk.** A program whose later outer read is on a path that never runs after the closure's write (a dead branch) is refused. This is conservative and is accepted. The fix-it gives the legal rewrite.

### CS2-2 (P1) Arithmetic builtins disagree with the operators; `assert_eq_f64` - O-GAP-06

**Problem.** The `+ - *` operators panic on i64 overflow (`interp/value.rs` `int_overflow`, "integer overflow: a + b exceeds i64", exit 101). Several builtins that compute the same arithmetic instead wrap, saturate, or panic in Rust.

**Repro** (all check rc 0):

| probe | program | run | operator equivalent |
|---|---|---|---|
| `a_pow` | `pow_i64(3, 40)` | `-6289078614652622815` rc 0 (wrapped) | `a_mul`: `m * 2` -> panic 101 |
| `a_sum` | `arr_sum_i64([MAX, 1])` | `9223372036854775807` rc 0 (saturated) | `a_fold` (`arr_fold(.., \|a, x\| a + x)`) -> panic 101 |
| `a_sumby` | `arr_sum_by([MAX, 1], \|x: i64\| x)` | `9223372036854775807` rc 0 | same |
| `a_mean2` | `arr_mean_i64([MAX, MAX, MIN, MIN])` (true mean -0.5) | `-2.30584e+18` rc 0 (a saturated partial sum: wrong) | - |
| `a_dictinc` | `dict_set(d, "k", MAX); dict_inc(d, "k")` | `thread '<unnamed>' panicked at crates/axon-core/src/interp/builtins.rs:5514:38: attempt to add with overflow` rc 101 (a Rust debug-overflow panic, not an Axon panic; a release build wraps silently `[INFERENCE: no overflow-checks key in Cargo.toml, so release wraps]`) | `a_add` -> Axon panic 101 |
| `a_pow_neg` (control) | `pow_i64(2, 0 - 1)` | `axon: panic: pow_i64: negative exponent` rc 101 | - |
| `a_shl`, `a_shlop` (control) | `shl(1, 64)`, `1 << s` | both `1` | agree (out of scope, §0) |

**Audit of every i64 arithmetic builtin** (interpreter, `interp/builtins.rs` at 4a6436da):

| builtin | interp | native | decision |
|---|---|---|---|
| `pow_i64` | `wrapping_pow` :2969 | `__axon_pow_i64` `axon-rt/src/lib.rs:1716-1728` `wrapping_pow` (`codegen/builtin_externs.rs:177-182`) | **panic** `integer overflow: pow_i64(b, e) exceeds i64` on both (checked_pow) |
| `arr_sum_i64` | `saturating_add` :1861 | `ArrReduce::Sum` -> `w_int_add_sat` `codegen/expr.rs:4774-4777`, `build_wrappers.rs:304` | **panic** (checked_add), same message shape as `+` |
| `arr_sum_by` | `saturating_add` :2672 (comment :2664 says it saturates to match `arr_sum_i64`) | no native lowering found (`git grep arr_sum_by crates/axon-core/src/codegen axon-rt` is empty) `[INFERENCE: refused as unlowered]` | **panic** |
| `arr_mean_i64` | sums with `saturating_add` :2266, then `/ len` | `ArrReduce::Mean` (`codegen/expr.rs:4931`) through the same saturating accumulator | **exact**: accumulate in i128, then divide as f64. A mean cannot overflow, so no panic is needed |
| `dict_inc` | `n + 1` :5514 | `axon-rt/src/lib.rs:641` `cur + 1` | **panic** (checked_add, Axon panic) |
| `abs_i64`, `as_i64`/`f64_to_i64`, `wrapping_*` | `abs_i64(MIN)` panics (`a_abs`); float->int casts saturate (documented); `wrapping_*` wrap by name | - | unchanged: `abs` agrees with `0 - MIN`, and the casts and `wrapping_*` are documented |
| `clamp_i64`, `sign_i64`, `min/max_i64`, `random_i64` (:3004-3026, :2945-2962) | no arithmetic overflow possible | - | unchanged |

The native path must change in the same commit as the interpreter (I-2). The operator overflow lowering already exists for `+`. The builder reuses its panic helper for `ArrReduce::Sum` and calls a checked `__axon_pow_i64` / `__axon_dict_inc` from axon-rt `[INFERENCE: the helper name was not read; the operator lowering in codegen/expr.rs is the source]`.

**`assert_eq_f64` - decided: exact IEEE `a == b` on all engines.**
- Interpreter: `if !(a == b || (a - b).abs() <= 1e-9)` (`interp/builtins.rs:3783`).
- Native: `FloatPredicate::OEQ` (`codegen/builtins.rs:1447-1449`).
- `spec/stdlib.md:516`: "exact float equality".
- `builtins.rs:250` doc: "Panic if `a != b`".

The interpreter is the reference semantics, but here it disagrees with its own documentation and with native. A tolerance hidden inside an "eq" assertion is the less obvious contract. Measured reliance on the tolerance:
- Each tracked `.ax` with an `assert_eq_f64` was re-run under `axon test` with the call replaced by an exact check: `examples/stdlib/asi_prelude.ax`, `reward.ax`, `uncertain.ax`, `crates/axon-core/tests/fixtures/phase12_coverage.ax`, and the tempt variant `tempt-unsat-doc-03/uncertain.ax`. There were **0** exact-only failures. The tempt variant already fails today (rc 3, `0.9 vs 1.3`, by design).
- The one Rust test that relies on the tolerance is `interp/vm/tests.rs:2622` `t_near` (`assert_eq_f64(0.1 + 0.2, 0.3)`), inside `assert_eq_f64_fails_on_nan_on_both_engines`. It asserts a pass. Natively the same program fails today: a live engine divergence.

Change:
- `builtins.rs:3783` becomes `if !(a == b)`. NaN still fails and `inf == inf` still passes.
- `t_near` moves from the passing list to the failing list.
- The `builtins.rs:250` doc becomes "Panic unless `a == b` exactly (IEEE; NaN never equal). For a tolerance write `assert(abs_f64(a - b) <= eps)`".
- g01 **M76** (`v022_g01_mutations.py:470-474`): its `old` string is the line that changes. The row is rewritten to `old: if !(a == b) {` and `new: if a != a || (a - b).abs() > 1e-9 {` (still NaN-kill). Same killer.
- This decision has already been sent to `DocsDraft` for the doc lane.

**Tests that flip (they pin the old behaviour):**
- `cli_run.rs:14590` `arr_sum_by_sums_a_projection_and_saturates_like_arr_sum_i64`: becomes `..._panics_like_the_operator`.
- `scripts/fuzz_parity.sh:464` `arr_sum_ovf`, and the `scripts/arr_reduce_parity.sh:9` note.
- `axon-rt/src/lib.rs` `migrated_pow_i64_matches_interp` (oracle `wrapping_pow`, ~:3917-3925) and `migrated_pow_i64_common_cases`.
- The `scripts/checked_arith_parity.sh` pow cases (:89, `pow_neg_exp`) gain an overflow case.
- The `builtins.rs:609` doc "Saturates on overflow".

**Fail-first test.**
- `axon-core --no-default-features --test cli_run`, `arithmetic_builtins_overflow_like_the_operators`: a_pow, a_sum, a_sumby and a_dictinc each give rc 101 with `integer overflow`, and a_mean2 prints `-0.5`. Today they give rc 0 (a_dictinc gives a Rust panic).
- `interp::vm::tests::assert_eq_f64_fails_on_nan_on_both_engines` gains `t_near` in the fail list.
- A native case goes into `scripts/checked_arith_parity.sh` (`pow_ovf`, `sum_ovf`, `dictinc_ovf`: rc 101 on both engines).

**False-refusal risk.** None at check, because this is runtime only. The 290- and 596-file sweeps are unaffected. The runtime behaviour change applies only to inputs that overflow today, where the old answer was wrong.

### CS2-3 (P1) Typed higher-order builtins and typed `Dict` - O-GAP-01 + O-GAP-02

**Problem.** Builtin sig strings such as `"fn(T) -> U"` (`builtins.rs:624` `arr_map`, :630 `arr_filter`, :636 `arr_fold` `fn(U, T) -> U`, :642 `arr_sort_by`, :702 `arr_sum_by`, :708 `arr_sum_by_f64`, :714 `arr_zip_with`, :1928 `arr_group_by`, :1934/:1940 `arr_max_by`/`arr_min_by`, :1922 `dict_map_values`, :1958 `dict_each`, :1969 `arr_partition`, :1981 `dict_filter`) are parsed by `parse_type_str` (`infer.rs:263-330`), which has no `fn(` arm. Each lands as `Type::Deferred("fn(T) -> U")`, and `promote_type_params` (`infer.rs:362-376`, comment :360-361) leaves it alone.

The checker sees such a param and checks only the closure's arity (`checker.rs:5795-5815`, `param_is_fn`, with the NOTE at :5802). `Dict` likewise parses to the opaque `Deferred("Dict")`. `dict_get` returns `Option<T>` with T free (`builtins.rs:1879-1881`), and an annotation `Dict<str, i64>` resolves to `Deferred` (`infer.rs:717-718`, "other generics treated as deferred").

**Repro** (all check rc 0):

| probe | run |
|---|---|
| `h_filter`: `arr_filter([1,2,3], \|x\| x + 1)` | `arr_filter: predicate must return bool, got i64` 101 |
| `h_fold`: `arr_fold(["a"], 0, \|acc, x\| acc + x)` | `cannot apply Add to i64 / str` 101 |
| `h_named`: `arr_filter(["a"], big)` with `fn big(x: i64) -> bool` | `cannot apply Gt to str / i64` 101 |
| `l_maxby_b`: `arr_max_by([1,2], \|x\| x > 1)` | `key fn must return numeric, got bool` 101 |
| `d_get`: `dict_set(d, "a", 1)`, then `Some(v) => println("got " + v)` | `cannot apply Add to str / i64` 101 |
| `d_ann`: `let d: Dict<str, i64> = dict_new(); dict_set(d, "a", "x")`, then `v + 1` | same 101 |
| `d_param_bare`: `fn total(d: Dict) -> i64 { ..Some(v) => v + 1.. }` called with a str-valued dict | same 101 |
| `d_mixed`: `dict_set(d, "a", 1); dict_set(d, "b", "x")` | `2` rc 0 (heterogeneous dict) |
| `l_generic_ok` (control): `fn keep<T>(xs: [T], f: fn(T) -> bool) -> [T] { arr_filter(xs, f) }` | `2`, must stay rc 0 |
| `l_annot_lambda` (control): `arr_fold(xs, 0, \|acc: i64, x: i64\| acc + x)` | `6`, must stay rc 0 |

It splits into two sub-items, built in order **3a then 3b**. 3b needs 3a's `fn(` parsing for `dict_map_values`, `dict_filter` and `dict_each`.

**CS2-3a - typed fn params of builtins.**
1. `parse_type_str` gains a `fn(P, ..) -> R` arm, which yields `Type::Fn(params, ret)`. `promote_type_params` gains a `Type::Fn` arm, so the T/U/V of a builtin's fn param become the same type variables as the rest of the sig.
2. `instantiate_sig` (`infer.rs:1372-1424`) then instantiates them per call. A lambda argument already infers as `Type::Fn(fresh params, body)` (`infer.rs:1842-1860`), and a named fn as `Type::Fn(sig)` (the `Ident` arm, `infer.rs:1096-1116`). The existing `self.constrain(arg_ty, param_ty, "arg i of name")` (`infer.rs:1398-1402`) unifies them, so a mismatch is the standard E0102.
3. Numeric key fns: `arr_max_by`/`arr_min_by` declare `fn(T) -> f64`, but the interpreter accepts any numeric key, and tracked code passes i64 keys (`cli_run.rs:28895`). A new sig placeholder `Num` is parsed to a fresh var recorded in `numeric_vars`. After solving, a var not resolved to `i64`/`i32`/`f64`/sized int is E0306 "key fn must return a number". `arr_sum_by` stays `fn(T) -> i64` and `arr_sum_by_f64` stays `fn(T) -> f64`.
4. Checker: `checker.rs:5795-5815` keeps its arity check, which gives a better message than a unification failure. Its "Deferred fn" branch is deleted once no builtin sig yields `Deferred("fn(")`. Clean cutover: no alias.

**CS2-3b - typed Dict values.**
1. A new `Type::Dict(Box<Type>)`, the value type. Keys are `str` only (`interp.rs:113-119`).
2. `parse_type_str("Dict")` -> `Dict(V)`, with V a sig type var. Sigs are rewritten: `dict_new() -> Dict<V>`, `dict_set(d: Dict<V>, k: str, v: V)`, `dict_get -> Option<V>`, `dict_get_or(.., default: V) -> V`, `dict_inc(d: Dict<i64>, ..)`, `dict_map_values(Dict<V>, fn(V) -> U) -> Dict<U>`, `dict_filter(Dict<V>, fn(str, V) -> bool) -> Dict<V>`, `dict_each(Dict<V>, fn(str, V))`, `dict_merge(Dict<V>, Dict<V>) -> Dict<V>`, `dict_values -> [V]`, `dict_to_pairs -> [(str, V)]`, `dict_from_pairs([(str, V)]) -> Dict<V>`, `arr_group_by -> Dict<[T]>`. `dict_to_str`/`dict_from_str` keep their documented value kind `[INFERENCE: read the interp arms at build time and type them as they behave]`.
3. Annotations. `Dict<str, V>` resolves to `Dict(V)` (`infer.rs:702-718`). `Dict<K, V>` with K not `str` is E0102 "Dict keys are str". A bare `Dict` in a fn param, return or struct field gets **one fresh var per declaration site**, the way an unannotated `let` does. A fn generic over the value type writes `fn f<V>(d: Dict<str, V>)`.
4. Codegen: `Dict` stays the opaque handle (`codegen/types.rs:160-161`). Codegen does not consume `infer`'s `Type` (`git grep InferCtx crates/axon-core/src/codegen` is empty `[INFERENCE: no other consumer found]`). The checker's `DEFERRED_PREFIXES` (`checker.rs:410`) loses `"Dict"`.

**Mixed-value dicts in tracked code (measured).** I classified the `dict_set` value kinds per dict variable in all 26 tracked `.ax` files that call `dict_set`. These six put values of different types into one dict:

| file | mix | in the 290 sweep? |
|---|---|---|
| `examples/asi/safe_bandit.ax` | `Agent` + i64 in `state` | yes (rc 0, W1210) |
| `tests/fixtures/vm_depth/nest/drop_array.ax`, `drop_option.ax`, `drop_result.ax`, `drop_tuple.ax` | i64 + a value nested through the dict ("read back from an untyped dict, so it nests without a recursive type", drop_array.ax:4-6). These are AX-59 depth probes run by `scripts/vm_wasm_depth.sh` and `wasm_nesting_parity.sh` | no (596 only) |
| `tests/fixtures/r51_s0b/leaf_sum_confusion.ax` | `Result` + `Option`: the fixture *is* the untyped-dict hole (its header, :1-4). Its test is `cli_run.rs:40069` `vm_same_both_engines` | no |

Typed values refuse all six. This is **NH-1**.

Under the recommended (a):
- `safe_bandit.ax` is rewritten in the same commit to keep the agent in a typed `let` and the counters in a `Dict<str, i64>`, so the 290-sweep row for it stays rc 0.
- The four drop probes wrap their nesting in a one-line recursive enum (`type N = Z | L { v: [N] }`, the phase16 List form), so the depth chain is unchanged in length `[INFERENCE: AX-59 owner confirms the drop path measured is still Elems/bounded_drop]`.
- `leaf_sum_confusion.ax` becomes a check-refusal test (rc 2, E0102). The hole it guarded at run time is now closed at check.

**Alternatives considered.**
- (i) Unify the closure only in the checker (`checker.rs:5802` path). This duplicates inference and loses the cross-arg constraint (fold's `U` from `init`).
- (ii) Monomorphic `Dict<str, Value>` with a runtime tag. This is the status quo.
- (iii) Enforce only explicit `Dict<str, V>` annotations. That fixes `d_ann` only; `d_get` and `d_param_bare` stay holes. It is NH-1(c).

**Fail-first tests** (`axon-core --no-default-features --test cli_run`):
- `closure_passed_to_a_builtin_is_type_checked` (h_filter, h_fold, h_named, l_maxby_b -> rc 2, E0102/E0306);
- `generic_and_annotated_closures_to_builtins_stay_accepted` (l_generic_ok, l_annot_lambda, plus an i64 key to `arr_max_by` -> rc 0);
- `dict_values_are_typed` (d_get, d_ann, d_param_bare, d_mixed -> rc 2, E0102).

**False-refusal risk: the highest in this spec.** Every tracked file that uses a higher-order builtin or a Dict changes type-inference path. Gate: after 3a, and again after 3b, the all-tracked sweep (§5) must be identical except the six named files. Any other changed row is triaged before merge. It is either a real latent bug in that file (the refusal is kept, the file fixed and named in the commit) or a false refusal (the typing is fixed). No row is waived.

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

**Change.** Unguarded arms decide coverage:
- int, str, f64, char: exhaustive only with a catch-all arm (`_` or a bare binding).
- bool: `true` + `false`, or a catch-all.
- Tuple: exhaustive if some unguarded arm is irrefutable component-wise. Otherwise, when every component is bool or a fieldless enum, the arms are expanded over the product (cap 256 combinations; above the cap a catch-all is required).
- Enum, `Option`, `Result`: unchanged, except that a guarded arm no longer counts.

The message reads "non-exhaustive match on `i64` - add a `_ =>` arm" (E0304, `error.rs:454`, already "non-exhaustive match - a variant has no arm"). The description widens to "a value has no arm". This applies in expression and statement position alike. `spec/language-tour.md:210` ("`match` is exhaustive") and `spec/axon-for-llms.md:214` become true as written.

**Measured.**
- 1115 `match` blocks in tracked `.ax`, scanned for scalar/tuple scrutinees without a catch-all: 9 flagged. 8 were false positives on hand inspection (`examples/feature_tour.ax:22,56` and the hardset copy have `_ =>` with no comma; `examples/tuples.ax:35,37` and the copy are irrefutable `(a, b)`).
- The one real hit is `crates/axon-core/tests/fixtures/vm_match/nomatch.ax` (596 set, baseline rc 0). It deliberately exercises the runtime panic, through `vm_match_non_exhaustive_panics` (`cli_run.rs:37821`). It becomes rc 2 with E0304. The test is changed to assert the check refusal, and a runtime-only path for the panic is kept by building the test's program through `run_program_capturing` (no check) `[INFERENCE: the builder confirms which harness bypasses check]`.
- Guarded-arm reliance: 0 tracked `.ax`. The Rust-embedded hits from the guard scan were Rust code (`lockfile.rs`, `manifest.rs`, `locate.rs`), i.e. false positives.
- Bare-variant patterns (`R =>` for an enum's `R`): 0 in tracked `.ax`. Today they bind and are not compared, so `x_enum_bare` prints `1` for `C::B` with W0004. Out of scope, logged as an observation.

**Fail-first test.** `non_exhaustive_scalar_and_tuple_matches_are_refused` (x_int, x_str, x_tuple, x_bool_half, x_unit_stmt, x_opt_guard -> rc 2, E0304), with the controls still rc 0.

**False-refusal risk.** Guards on enum arms: a match that is exhaustive only through guards the author knows are total (`Some(x) if x > 0`, `Some(x) if x <= 0`) is refused. The fix is a catch-all. This is accepted as conservative and measured at 0 tracked uses.

### CS2-5 (P2) Methods on generic params; generic struct arguments - O-GAP-05

**Problem.** `checker.rs:4176-4182` drops the method check when the receiver is a generic param. The comment there says a stronger version "would look the method up on the declared bound ... needs the bounds threaded here". The bounds already exist: `generic_bounds` (fn -> `(param, traits)`, built in `check_program`, ~`checker.rs:525-530`) and `trait_defs` (:510-532). Separately, `B<i64>` resolves to `Deferred("B")` (`infer.rs:717-718`), and a struct literal's field of type `T` is never tied to the annotation (`infer.rs:1604-1620`).

**Repro:**

| probe | check | run |
|---|---|---|
| `g_method`: `fn f<T>(x: T) -> i64 { x.area() }`, `f(3)` | 0 | `no method area on type i64` 101 |
| `g_bounded_bad`: `T: Shape`, `x.perim()` (Shape has only `area`) | 0 | `no method perim on type Sq` 101 |
| `g_struct`: `let b: B<i64> = B { v: "s" }; b.v + 1` | 0 | `cannot apply Add to str / i64` 101 |
| `g_struct_param`: `fn get(b: B<i64>)` called with `B { v: "s" }` | 0 | same |
| `g_bounded_ok` (control) | 0 | `9` |
| `g_bound_violation` (control) | already rc 2, E0504 | - |

**Change.**
1. A receiver whose type is generic param `T` of the current fn:
   - unbounded -> E0403 "no method `m` on type parameter `T` - it has no bound; add `T: Trait`";
   - bounded -> look `m` up in the declared methods of each bound trait (`trait_defs`), and E0403 if none declares it.
   
   This needs `current_generic_params` to become a param -> bounds map, which replaces the bare name set (`checker.rs:509`).
2. A user generic type: new `Type::App(String, Vec<Type>)`. `B<i64>` resolves to `App("B", [i64])`.
   - A struct literal of generic `B` instantiates B's params with fresh vars, constrains each field, and yields `App("B", vars)`.
   - Field access on `App` substitutes the args into the field type.
   - `App` unifies with `App` by name and args, and with the legacy `Struct("B")` by name, during the cutover inside `infer.rs` only.
   - Mismatch is E0102.

**Alternatives.** Name-mangled monomorphic structs (`Struct("B<i64>")`) need every lookup keyed twice. Erasure is the status quo.

**Measured.**
- Generic user types in tracked `.ax`: 8 (`generics.ax` `Pair`, `phase22`, `phase28` `Pair`/`Triple`, `phase55`, `phase59` `Box`, `phase74` `Stack`, `Tagged`), all rc 0 in the 596 baseline.
- Bounded-generic method calls (tracked `.ax` and Rust-embedded): `phase62_trait_bounds.ax`, `conform_tests.rs` `area_of<T: Shape>`, `tests_v022_b.rs:88` `judge<T: Judge>`, `tests_v022_s4.rs:348`, `cli_run.rs:32212/32240`, `sealed_frames.rs:1985` `gm<T: Gm>`. Each calls a method its bound declares.
- Unbounded: 0.
- Both sweeps are expected identical.

**Fail-first test.** `generic_param_method_and_generic_struct_args_are_checked` (g_method, g_bounded_bad -> E0403; g_struct, g_struct_param -> E0102). Controls: g_bounded_ok and the 8 generic-type fixtures stay rc 0.

**False-refusal risk.** A trait method reached through a supertrait. Axon has no supertraits `[INFERENCE: no syntax found in compiler-phase3.md:47 type_param grammar]`. Default methods are declared in the trait, so they are found.

### CS2-6 (P3) Callees that are not a name - O-SF-04

**Problem.**
- `infer.rs:1336` computes `_callee_ty` and discards it.
- When there is no fn name, `infer.rs:1426-1430` infers the args and returns a fresh var.
- The checker's call checks (`checker.rs:4090-4131`, O-SF-01's `fn_value_type` at :705-743 and `check_args_against_sig` at :5652) run only for `Expr::Ident` callees.

**Repro:**

| probe | check | run |
|---|---|---|
| `n_index`: `let ops = [add]; ops[0](1)` | 0 | `lambda: expected 2 args, got 1` 101 |
| `n_call`: `mk()(1, 2, 3)`, with `mk() -> fn(i64, i64) -> i64` | 0 | `expected 2 args, got 3` 101 |
| `n_field`: `(h.f)(1, 2)`, with field `f: fn(i64) -> i64` | 0 | `expected 1 args, got 2` 101 |
| `n_type`: `ops[0](1, "x")` | 0 | `cannot apply Add to i64 / str` 101 |
| `n_ok` (control): `ops[0](1, 2) + mk()(3, 4)` | 0 | `10` |

**Change.**
- In infer, when the callee is not a name, unify `_callee_ty` with `Fn(arg types, fresh ret)` and return the ret. An arity or type mismatch is E0102 at the call.
- In the checker, extend `fn_value_type` to `Index` of an array whose element type is `Fn`, to `Call` of a fn whose declared ret is `fn(..)`, and to `FieldAccess` of a fn-typed struct field.
- The `Call` arm then calls `check_args_against_sig` for any callee whose `fn_value_type` is known, which gives E0305 for arity and E0306 for an argument type, the same codes O-SF-01 gives for name callees.

**Measured.** There is one non-name callee in tracked `.ax`: `crates/axon-core/tests/fixtures/vm_perf/big-compile.ax:20068` `table[n](n + off)`. It is valid and must stay rc 0. Both sweeps are expected identical.

**Fail-first test.** `a_call_through_a_non_name_callee_checks_its_signature` (n_index, n_call, n_field -> E0305; n_type -> E0306; n_ok rc 0).

**False-refusal risk.** An array of fns with different arities cannot type-check under unification anyway (an array literal's elements unify), so no new refusal class arises.

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
- `cli_run`: `refinements_on_variant_fields_and_lambda_params_are_enforced` (r_variant2, r_lambda, r_lambda_hof -> rc 6 under `axon run`; r_variant_lit, r_lambda_lit -> rc 2, E1209).
- `tests/caps_verb.rs`: `a_refined_variant_field_is_checked_at_construction_and_reached`, the twin of `a_refined_struct_field_is_checked_at_construction_and_reached` (:573-574).
- Native parity: one case each in `scripts/exit_code_parity.sh` (rc 6 on both engines).

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

`axon caps --json --fn leak e_helper.ax` reports `capabilities: ["fs:write"]` and `axon caps --fn late` reports `effects: ["Time"]`. The reach walker already knows.

**Re-measure of the capability-surface Q2 cost.**

`caps_verb.rs` tests that route a write through a refinement predicate with an effectful helper (`type Lk = i64 where leak(_)`) and require `axon run` to perform it (`implicit_route`, :497-530):
- `a_refinement_predicate_at_a_let_annotation_is_reached` (:537);
- `..._at_a_helpers_parameter_is_reached` (:546);
- `..._on_the_return_type_is_reached` (:556);
- `an_inline_parameter_refinement_is_reached` (:565);
- `a_refined_struct_field_is_checked_at_construction_and_reached` (:574);
- `a_whole_struct_refinement_is_reached` (:584).

That is **6**. `contained_refuses_a_write_reached_through_a_refinement_predicate` (:650) asserts check rc 2 containing E1001. It still gets rc 2, and the builder confirms E1001 is still emitted next to E1209 `[INFERENCE]`, so it is 7 at most. `a_predicate_that_mentions_its_own_type_terminates_and_is_reached` (:669) uses the library API (`reachable_capabilities`) without check: unaffected. This matches Q2's 6-7.

Tracked `.ax` refinements that call a user fn: 3 (`examples/asi/world_model.ax:43` `fit_error`, the hardset copy, `examples/refinements.ax:32` `is_even`, `examples/stdlib/world.ax:45` `fit_error`). `axon caps --json --fn` reports `capabilities: []`, `effects: []` for all of them. Both sweeps are expected identical.

**Decision: include.** Q2 deferred this item to "a separate spec", and this is that spec. The 6 tests pin the walker's reach through refinement predicates. That reach stays pinned through the library API: each test keeps its `reachable_capabilities` assertion, the same form as :669, and its `axon run` half becomes an `axon check` assertion of rc 2 with E1209. The route is closed, so the test now pins the refusal. That is a rewrite of 6 test bodies, with no capability lost.

**Change.** `reject_impure_refinement` walks each refinement predicate with the capabilities reach walker. The walker is the one `effects.rs:237-290` (`verify_predicate_offence`, CAP-1 E1104) already runs for `@[verify]` predicates, through `capabilities::verify_predicate_reaches`-style `WalkMaps` (`capabilities.rs:769-800`). Any reached capability or effect, `Time` included, is E1209: "refinement `W` reaches `append_file` (fs:write) through `leak`; a refinement predicate must be pure". Refinements are stricter than verify predicates, which allow a clock read.
- `reach.unknown` (a computed dispatch) is also E1209, deny by default.
- Measured: 0 tracked predicates reach unknown.
- The shallow builtin scan stays as the first pass, so `e_direct`'s message is unchanged.

**Alternative.** Treat only `@[pure]`-marked helpers as callable. That refuses the 3 pure tracked helpers (false refusals), so it is rejected.

**Fail-first test.** `cli_run`: `an_effectful_helper_in_a_refinement_is_refused_e1209` (e_helper, e_now -> rc 2, E1209; e_pure_helper and `examples/refinements.ax` rc 0).

## 3. Error codes

The only new code is **E0607** (CS2-1). Everything else reuses an existing code:
- E0102 (CS2-3, CS2-5, CS2-6 infer);
- E0304 (CS2-4);
- E0305/E0306 (CS2-6, CS2-3 numeric key);
- E0403 (CS2-5);
- E1209 (CS2-7, CS2-8);
- runtime exit 6 / 101 (CS2-7, CS2-2).

E0607 is registered at the four sites listed in CS2-1. E0304's description widens (`error.rs:454` and `AXON_REFERENCE.md:164`) in CS2-4's commit.

## 4. g01 rows (M3890-M3929), refusal coverage, pci

Row rules are as in `tasks/checker-soundness-spec.md` §4:
- one 8-tuple per row in `scripts/v022_g01_mutations.py` (format :12118-12124): id, description, **one** file, unique `old`, `new`, crate, target, **one** killing test;
- one `ATTACK_MARKERS` entry each (`scripts/v022_attack_markers.py`, format :3283-3300), whose `r'ATTACK: ... \[probe\]'` text appears in the killing test's assert message;
- `python3 -B scripts/g01_anchors.py` reports each `old` exactly once.

The exact `old` is the guard line the build writes, so the builder fixes it at implementation time. The guard each row removes is fixed here. cli_run target: `'--no-default-features --test cli_run'`. For codegen rows, copy the target of the nearest existing codegen row in the script `[INFERENCE: not chosen here]`.

| id | item | guard the mutation removes | file | killing test |
|---|---|---|---|---|
| M3890 | CS2-1 | record a captured name written by the lambda | mut_borrow.rs | `closure_write_to_a_capture_read_outside_is_refused_e0607` |
| M3891 | CS2-1 | a later outer use of a recorded binding emits E0607 | mut_borrow.rs | same |
| M3892 | CS2-1 | loop-literal rule 2(b) | mut_borrow.rs | same (c1_loop marker) |
| M3893 | CS2-1 | `AssignTo` place-root counts as a write | mut_borrow.rs | same (c1_field marker) |
| M3894 | CS2-1 | a lambda param or local `let` is not a capture (false refusal) | mut_borrow.rs | `closure_counter_and_local_writes_stay_legal` |
| M3895 | CS2-2 | `pow_i64` checked_pow | interp/builtins.rs | `arithmetic_builtins_overflow_like_the_operators` |
| M3896 | CS2-2 | `arr_sum_i64` checked_add | interp/builtins.rs | same |
| M3897 | CS2-2 | `arr_sum_by` checked_add | interp/builtins.rs | same |
| M3898 | CS2-2 | `arr_mean_i64` i128 accumulator | interp/builtins.rs | same (a_mean2 marker) |
| M3899 | CS2-2 | `dict_inc` checked_add | interp/builtins.rs | same |
| M3900 | CS2-2 | native `__axon_pow_i64` checked | axon-rt/src/lib.rs | `migrated_pow_i64_matches_interp` (rewritten oracle) |
| M3901 | CS2-3a | `parse_type_str` `fn(` arm | infer.rs | `closure_passed_to_a_builtin_is_type_checked` |
| M3902 | CS2-3a | `promote_type_params` Fn arm | infer.rs | same (h_fold marker) |
| M3903 | CS2-3a | `Num` key var resolved numeric | infer.rs | same (l_maxby_b marker) |
| M3904 | CS2-3a | generic fn param stays accepted (false refusal) | infer.rs | `generic_and_annotated_closures_to_builtins_stay_accepted` |
| M3905 | CS2-3b | `Dict` parses to `Dict(V)` | infer.rs | `dict_values_are_typed` |
| M3906 | CS2-3b | `Dict<str, V>` annotation resolves to `Dict(V)` | infer.rs | same (d_ann marker) |
| M3907 | CS2-3b | bare `Dict` in a signature gets a per-site var | infer.rs | same (d_param_bare marker) |
| M3908 | CS2-3b | `Dict` vs `Dict` unifies the value types | infer.rs | same (d_mixed marker) |
| M3909 | CS2-3b | `Dict` removed from `DEFERRED_PREFIXES` | checker.rs | same (d_get marker) |
| M3910 | CS2-4 | scalar scrutinee needs a catch-all | checker.rs | `non_exhaustive_scalar_and_tuple_matches_are_refused` |
| M3911 | CS2-4 | bool needs both literals | checker.rs | same (x_bool_half) |
| M3912 | CS2-4 | tuple product expansion | checker.rs | same (x_tuple) |
| M3913 | CS2-4 | a guarded arm does not count | checker.rs | same (x_opt_guard) |
| M3914 | CS2-4 | a binding arm is a catch-all (false refusal) | checker.rs | same (x_int_bind control) |
| M3915 | CS2-5 | unbounded generic receiver -> E0403 | checker.rs | `generic_param_method_and_generic_struct_args_are_checked` |
| M3916 | CS2-5 | bounded receiver: method looked up in the bound traits | checker.rs | same (g_bounded_bad) |
| M3917 | CS2-5 | `App` unify compares args | infer.rs | same (g_struct) |
| M3918 | CS2-5 | struct literal instantiates the type params | infer.rs | same (g_struct_param) |
| M3919 | CS2-6 | infer unifies a non-name callee with `Fn(args, ret)` | infer.rs | `a_call_through_a_non_name_callee_checks_its_signature` |
| M3920 | CS2-6 | `fn_value_type` on `Index` | checker.rs | same (n_index) |
| M3921 | CS2-6 | `fn_value_type` on `Call`/`FieldAccess` | checker.rs | same (n_call) |
| M3922 | CS2-7 | interp Enum arm runs field refinements | interp/eval.rs | `refinements_on_variant_fields_and_lambda_params_are_enforced` |
| M3923 | CS2-7 | `bind_closure_params` checks refined params | interp.rs | same (r_lambda) |
| M3924 | CS2-7 | VM declines refined closures from its fast path | interp/vm/compile.rs | same (r_lambda_hof) |
| M3925 | CS2-7 | checker variant-field constant -> E1209 | checker.rs | same (r_variant_lit) |
| M3926 | CS2-7 | caps walker pushes the enum for `E::V {..}` | capabilities.rs | `a_refined_variant_field_is_checked_at_construction_and_reached` |
| M3927 | CS2-7 | native `emit_lambda` preconditions | codegen/expr.rs | the exit_code_parity rc-6 case `[INFERENCE: that script is not a cargo target; if g01 needs a cargo killer, use the codegen test nearest emit_refine_preconditions]` |
| M3928 | CS2-8 | deep reach walk in `reject_impure_refinement` | checker.rs | `an_effectful_helper_in_a_refinement_is_refused_e1209` |
| M3929 | CS2-8 | `reach.unknown` refused | checker.rs | same (unknown-dispatch probe marker) |

M76 is rewritten under CS2-2 (its `old` line changes). That is not a new id.

**Refusal coverage.** After CS2-1, CS2-4 and CS2-8 land, run `python3 -B scripts/v022_refusal_coverage.py`, and add a row or an exemption for any new refusal site it names (seconds).

**pci.** Every row above whose file is under `crates/axon-core/src` (all except M3900) adds 1 to the count line at `governance/notes/v022-pci-delta.md:591` ("530 rows at this head"). With M3927 that is 39 rows, and the range list gains `M3890-M3899, M3901-M3929`. Edit the line in the same commit as the rows. Theme files and the regen are integrator-only, as in checker-soundness §4.

## 5. Gates and cost budget (no paid spend)

The sweep is defined two ways:
- **S290**: `/var/tmp/r51-tmp/cs-sweep.sh <repo> <bin> <out>`. Baseline `/var/tmp/r51-tmp/sf-sweep-base.txt`: 290 files, rc0=274. I re-ran it on main-bin: IDENTICAL, 2.65 s.
- **S596**: `/var/tmp/r51-tmp/CS2Draft/cs-sweep-all.sh`. The same loop over every `git ls-files '*.ax'`. Baseline `/var/tmp/r51-tmp/CS2Draft/sweep-all-base.txt`: 596 files, rc0=512, other=84, 6.0 s. The integrator copies the script and baseline into the build harness; they are not committed here.

Expected deltas (everything else identical):
- S290: `examples/asi/safe_bandit.ax` would go rc 0 -> 2 under NH-1(a). It is rewritten in CS2-3b's commit, so S290 stays identical.
- S596:
  - `drop_array`/`drop_option`/`drop_result`/`drop_tuple.ax` stay rc 0 after their rewrite;
  - `r51_s0b/leaf_sum_confusion.ax` goes rc 0 -> 2 (E0102), which is correct: it is the closed hole;
  - `vm_match/nomatch.ax` goes rc 0 -> 2 (E0304), which is correct: a non-exhaustive match.

| gate | one-run cost | trigger | skip rule |
|---|---|---|---|
| S290 + S596 | ≈ 9 s total, offline | every checker/infer/mut_borrow commit, and after 3a and again after 3b | never. Pass = identical except the named files |
| targeted `cargo test -p axon-core --no-default-features --test cli_run <names>` | minutes, compile-dominated; full cli_run ≈ 544 s (checker-soundness §5) | each item | never |
| `cargo test -p axon-core --test caps_verb` | `[unmeasured]` | CS2-7, CS2-8 | never for those |
| `--lib interp::vm::tests` | `[unmeasured]` | CS2-2, CS2-7 | never for those |
| fresh codegen build + `AXON_HARNESS_STRICT=1 scripts/exit_code_parity.sh`, `checked_arith_parity.sh`, `fuzz_parity.sh`, `arr_reduce_parity.sh` | `[unmeasured]` | CS2-2, CS2-7 (both touch codegen/axon-rt) | never for those |
| `scripts/vm_wasm_depth.sh`, `wasm_nesting_parity.sh` | `[unmeasured]` | CS2-3b (drop fixtures rewritten) | never for 3b |
| g01 lane feedback `python3 -B scripts/v022_g01_mutations.py --only=<item ids> OUT.json` | cli_run rows ≈ 4-8 min each `[INFERENCE, from checker-soundness §5]`; 40 rows ≈ 3-5 h serial | after each item's commit | not evidence |
| `g01_anchors.py`, `v022_refusal_coverage.py`, `pci_delta.py --check` | seconds | any src commit | never |
| `./scripts/gate.sh --fast --base=main` | ≈ 15-25 min `[INFERENCE]` | once per lane before hand-off | not evidence |
| **g01 `--only=M3890,...,M3929`** (the new rows, plus M76) | as above, ≈ 35 min parallel at `--scope=all` scale `[INFERENCE]` | once at the integrated head | never |
| **`./scripts/gate.sh --strict`** | ≈ 43-44 min (checker-soundness §5, measured there) | once at the integrated head; required because CS2-2 and CS2-7 touch codegen/axon-rt | never |
| cortex lane `cargo test -p axon-cortex --features ai` | 10-15 min | only if a benchmark program's check rc changes (none expected) | skip otherwise |
| paid receipt regress | ≈ $0.14 | not triggered: no `axon-cortex/src` or driver change | skip; **$0 paid spend budgeted** |

## 6. Sequencing and lanes

Two lanes, split by file ownership:

| lane | items, in order | files |
|---|---|---|
| **A: checker** | CS2-4 -> CS2-6 -> CS2-5 -> CS2-1 -> CS2-3a -> CS2-3b -> CS2-8 | `checker.rs`, `infer.rs`, `types.rs`, `mut_borrow.rs`, `error.rs`, `AXON_REFERENCE.md`, `builtins.rs` (sig strings), `capabilities.rs` (CS2-8 read path), fixtures for 3b and CS2-4 |
| **B: runtime** | CS2-2 -> CS2-7 | `interp/builtins.rs`, `interp/eval.rs`, `interp.rs`, `interp/sym.rs`, `interp/vm/*`, `codegen/expr.rs`, `codegen/asi.rs`, `axon-rt/src/lib.rs`, parity scripts, `capabilities.rs` (StructLit arm) |

Order rationale and shared files:
- CS2-4 and CS2-6 are small and decision-free, so they land first.
- CS2-6 precedes CS2-3, because both edit the infer `Call` arm (`infer.rs:1336`, :1426).
- CS2-5's `Type::App` precedes 3b's `Type::Dict`. Both add `Type` variants and their match arms: serial.
- CS2-1 is independent (mut_borrow and error.rs), but it is the only new code, so error.rs edits stay serial.
- CS2-3b waits for NH-1.
- CS2-8 is last in lane A because its six `caps_verb` rewrites collide with CS2-7's new `caps_verb` test (same file). B lands CS2-7 first, and A rebases.
- **Cross-lane:** CS2-7's checker edits (`check_field_refinement` :2056/4767, `check_refinement_args` :1905) are small and in regions lane A does not touch. CS2-7 step 4's lambda-callee case needs CS2-6's `fn_value_type` extension, so B merges after A's CS2-6. `capabilities.rs` is touched by B (StructLit arm :1763) and A (CS2-8 calls the walker, no edit to the walker expected); merge serially.
- One integrator owns `v022_g01_mutations.py`, `v022_attack_markers.py` and the pci count line. Lanes hand over row text in their reports.

## 7. Engineering decisions (decided here)

| id | question | decision | why |
|---|---|---|---|
| E-1 | CS2-1: refuse vs by-reference capture | narrow refusal (E0607) when the divergence is observable | keeps C02 and T40; no engine change; refuses only programs whose answer differs |
| E-2 | `assert_eq_f64` | exact IEEE on all engines | matches stdlib.md:516 and native; 0 tracked tests rely on the tolerance; removes a live tree/native divergence (`t_near`) |
| E-3 | overflowing arithmetic builtins | panic like the operators; `arr_mean_i64` exact via i128 | one overflow rule; mean has no overflow to report |
| E-4 | HOF typing | parse `fn(..)` in builtin sigs and unify in infer | uses existing instantiation; fold's `U` constraint crosses args |
| E-5 | bare `Dict` in a signature | one fresh var per declaration site | same rule as an unannotated `let`; a generic fn writes `<V>` |
| E-6 | CS2-8 include vs defer | include; rewrite 6 `caps_verb` bodies to assert the refusal and keep the library reach assertion | this is the separate spec Q2 asked for; no reach coverage lost |
| E-7 | guarded arms in exhaustiveness | do not count | a guard can be false; 0 tracked reliance |

## 8. Needs human (product / irreversible only)

- **NH-1. Is a `Dict` homogeneous in its value type?** (CS2-3b)
  - (a) Yes: one value type per dict, inferred or annotated. A mixed dict is refused. 6 tracked files change: `safe_bandit.ax` is rewritten; the 4 AX-59 drop probes get a recursive-enum wrapper; `leaf_sum_confusion.ax` becomes a refusal test.
  - (b) Typed when the writes agree; a dict whose writes conflict silently stays dynamic. This is unsound by construction, because the reads of a conflicted dict stay unchecked.
  - (c) Enforce only explicit `Dict<str, V>` annotations. `d_get` and `d_param_bare` stay holes.
  - **Recommendation: (a).** An AI author cannot tell from a dict's type what `dict_get` returns, and that is exactly the check-then-panic class. The cost is 6 files and one re-justification of the AX-59 probes.
- **NH-2. O-GAP-07 return types** (`arr_max_i64([])` panics; recursion depth 6000). Changing `arr_max_i64`/`arr_min_i64` to return `Option<i64>` breaks every caller.
  - **Recommendation:** a separate spec. Keep the panic for now and document it in stdlib.md. That spec decides `Option` versus a check-time refusal of a provably empty literal.

No other choices here are product or irreversible.
