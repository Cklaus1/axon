# tuple-patterns - spec (TP-1..TP-4)

**Status: BUILT 2026-11-10 on `tp/build` (TP-1+TP-2 00dbca5b, TP-3 6fb5c873, TP-4 a82f13a9; lane head 16dbcfac). Build
authorized by the human 2026-11-10 ("go", on the recommendations: NH-1 (a), NH-2 (a)).**

### Step 1 review

Reviews: `agent://AuditTP` (TPA-1..7, conformance) and `agent://DevloopTP` (TPD-1..7, dev loop). Before accepting each finding I
checked it: I re-ran every reviewer probe in `/var/tmp/r51-tmp/review-TP/tpa/` with `/var/tmp/r51-tmp/eg-bin/axon` through
`LFT/run.sh`, re-read every cited line, and added probes of my own in `/var/tmp/r51-tmp/LFT/r2/`. Results are in §9. **Accepted 14,
rejected 0.**

| finding | sev | disposition | where |
|---|---|---|---|
| TPA-1: the numeric-leaf guard covered the whole element, and `kinds_conflict` never compares a non-scalar, so `u1` (`[i64]` as `str`), `u2` (`Option<i64>` as `i64`), `u3` (int as `[T]`) and `v5` (`[i64]` to `to_str`) still check clean and panic | major | accepted, with one change. TP-1 is now **skeleton binding**: only numeric leaves get a fresh var and an obligation, and every other leaf binds exactly. The obligation is "resolved and not a number". I re-ran all four probes: check 0, panic 101; twins refused. Added from my own probes: `Uncertain<..>`/`Temporal<..>` wrappers are stripped before the test, because `w2_unc`/`w7_unc_let` run today and the twin `w2t` is accepted. Traced every ICS-1 and CS4-3 control (TP-1 "Control trace") | TP-1 fix design, tests, D2, D7, §4 M5226/M5228 |
| TPA-2: a numeric component passed through a fn value (`u4_closure`) is never checked, because CS2-6 binds no argument | major | accepted, **CS2-6 extension chosen** (D6): one map and one row (M5227). In the CS2-6 argument loop, an argument still resolved to a component var is compared as its element leaf. This is sound under the NH-1 class: each refusal it adds has a refused twin (`u4t` E0102; my `z1_closure_runs` runs today and its twin `z1t` is E0102). The residual alternative would have left a known panic open for about 6 lines | TP-1 fix design step 5, D6, NH-1, §4 |
| TPA-3: NH-1 understated the refusals; it is a class. `v4` and `v6` run today and are refused | major | accepted. NH-1 is now a class statement with the observed examples (d2, d4, q4, q5, q7, q11, q18, q19, v4, v6, z1). I re-ran v4 (rc 0, `true`) and v6 (rc 0, `n`); their twins are E0306 and E0301 | worst-outcome, TP-1 table and tests, NH-1 |
| TPA-4: the shared test's assertion that the diagnostics contain "tuple pattern" fails for `s1`/`s3`, which refuse at "arithmetic operands" | major | accepted. Only the shape cases assert "tuple pattern" | TP-2 test |
| TPA-5: the NH-1 test's lines for q4..q19 were not given | minor | accepted. Lines added; I checked them against the probe sources | TP-1 tests |
| TPA-6: under TP-3's fallback, M5224 has no killer | minor | accepted. The fallback now drops M5224 | TP-3, §4 |
| TPA-7: M5219 is the end of the reserved EG-2 block; the last row used is M5211 (`:16041`); the EG-4 row is at `:15954` | minor | accepted. Re-read: `:16034` comment, `:16041` `'M5211'`, `:15954` `'M5150'`; no `M522x`/`M523x` outside this spec. M5220-M5239 still stands | §4 |
| TPD-1: the native test runs only under `--strict`, and its cost left out the LLVM build | major | accepted. Re-read `gate.sh:1131` (`--no-default-features`) and `:1273`; `/usr/lib/llvm-23` exists; there is no worktree `target/`. The lane now runs it once | §3, §5, §6 |
| TPD-2: the strict-gate cost used the best idle run | major | accepted. Re-read `enable-gaps-spec.md:505` (2634 s; 3273 s on the same row), `build-loop-checker.md:43` (83m35s), `asi-fixes-spec.md:546` (≈ 95 min) | §5 |
| TPD-3: the probe count was 98 files, not ~120 | minor | accepted (now 98 + 24 review/lane-2 probes) | §5 |
| TPD-4: the cortex receipt was not named | minor | accepted. `gate.sh:714`; `REPAIR_SOURCES` `scorecard.py:89` is axon-cortex only | §5 |
| TPD-5: no lane-level full `cli_run` | minor | accepted. `build-loop-r42.md:13` gives 488 s | §5, §6 |
| TPD-6: the doc line after `:78` reads as if it applies to `let`/`for` | minor | accepted. Moved to after `:86` (the "Pattern matching" block); `language-tour.md:188` and `AXON_REFERENCE.md` are intentionally unchanged | TP-1 Docs |
| TPD-7: §9 names 557e746c and §3 names 4199c91c | minor | accepted. A note was added (no `src/` change between the two) | §3 |

**Step 2 classification.** Each open question is classified. D1-D8 (§7) are engineering decisions and are **canon in this spec**:
the lane builds them as written, and a deviation needs a `[REVISED]` note and an integrator ruling. That covers TPA-2's choice (D6)
and the wrapper and generic-var-leaf rules (D7, D8). D3 (dead-arm refusals) is canon under checker-soundness-4 ruling 5 / D9. Two
questions change what running programs do, so they stay with the human (§8): **NH-1** (the TP-1 refusal class) and **NH-2** (TP-4,
which adds one dead-arm refusal, as CS4's NH-4 did).

One entry from `tasks/opportunities.md`: O-LF-01, the part checker-soundness-4 CS4-3 left open. I also considered O-LF-02, and it is
**not** in this batch (§0 "Out of scope"). Every line anchor was read at `4199c91c` (branch `specs/lf`, worktree
`/home/cklaus/projects/axon-lanes/lf-spec`, = `origin/main`). The reference binary is `/var/tmp/r51-tmp/eg-bin/axon`
(`axon 0.1.0 (557e746c)`, default features). `557e746c` is an ancestor of `4199c91c`. `git diff --stat 557e746c 4199c91c --
crates/axon-core crates/axon-rt` lists only `crates/axon-core/tests/cli_run.rs` (6 lines), so the binary was built from the anchored
`src/`. Probe files are in `/var/tmp/r51-tmp/LFT/p/`. `LFT/run.sh` prints the check rc, the first error `code:line`, the run rc,
stdout and the panic line. The ICS-1 audit probes are in `/var/tmp/r51-tmp/ICS/p7`, `p9`. I ran no cargo build or test. Anything I
did not observe is marked `[INFERENCE]`, and every "after" column is the design's prediction.

Worst-outcome check, per item:
- TP-1 turns programs that check clean and then panic into check refusals. `[REVISED: TPA-3]` **It also refuses a class of programs
  that run today: every use whose matching `let (a, b) = t` twin is refused, except where only the numeric width differs.** Observed
  running examples: d2, d4, q4, q5, q7, q11, q18, q19, v4, v6 and z1. Six of them are pinned as legal by CS4-3's
  `tuple_pattern_controls_stay_legal`. That is NH-1. TP-1 must not refuse the ICS-1 width cases (an int component used as
  `u8`/`i32`/`f64`); refusing them is the regression that withdrew CS4-3's first build.
- TP-2, TP-3 and TP-4 turn check-clean panics into refusals. They also refuse four dead arms that run today (§7 D3; same class as
  CS4 D9).

## 0. Source entries

| item | entry (tasks/opportunities.md) | sev |
|---|---|---|
| TP-1 | O-LF-01 (`:3468-3475`): component misuse (`match (1, "s") { (a, b) => to_str(b + 1) }`) checks rc 0 and panics. The PARTLY FIXED stamp (`:3472-3475`, CS4-3 d4ea99f2, narrowed by 3c19c6de after ICS-1) leaves components untyped | medium |
| TP-2 | new, the same entry's mechanism: a nested tuple sub-pattern is bound with `None`, so `((a, b), c)` on `((1, 2, 3), 4)` checks rc 0 and panics `no match arm matched` (`p/n2_nested_arity.ax`) | medium |
| TP-3 | new, the same mechanism: a tuple pattern inside `Some(..)`/`Ok(..)`/`Err(..)` is never shape- or type-checked (`p/s1_some_ty.ax`, `p/s3_ok_ty.ax` panic; `p/s2_some_arity.ax`, `p/h4_ok_arity.ax` run a dead arm) | medium |
| TP-4 (P3, optional) | new, the same mechanism: a tuple pattern in a struct or variant pattern's field (`W::P { t: (a, b) }`) is never checked (`p/e1_variant_tuple.ax` panics, `p/e3_variant_arity.ax` runs a dead arm) | low |

### Why components were left untyped (the ICS-1 finding)

CS4-3's first build bound each component to the subject's element type (`Some(&var_i)`; the g01 mutant M4714 keeps that code,
`scripts/v022_g01_mutations.py:14594-14597`). The ICS-1 implementation audit found it refused programs that run today (and were not
in that spec's refusal list). The cause is integer-literal defaulting together with literal-only width coercion:
- An int literal infers to `i64` and a float literal to `f64` (`infer.rs:1855-1856`). A tuple expression keeps each element's own
  inferred type (`Expr::Tuple`, `infer.rs:2140-2147`), so `(1, 2)` is `(i64, i64)`.
- Only a literal coerces to a narrower int annotation: `try_int_literal_coercion` (`infer.rs:1257-1280`) fires only when the value is
  `Expr::Literal(AstLiteral::Int(_))`, and is called from the `let` annotation path (`infer.rs:1900-1910`). The `BinOp` R19 Slice B
  rule (`infer.rs:1755-1767`) also skips the width constraint only for a literal operand. A name holding an int is a strict `i64`:
  `let a = 1; let x: u8 = a` is E0102 (`ICS/p7/w1_plain.ax`), and `let a = 1; sh(a)` with `sh(x: u8)` is E0306 (`p/b4_var_u8.ax`).
- The interpreter does not care about widths. `match (1, 2) { (a, b) => { let x: u8 = a; .. } }` prints `1` (`ics_w4`).
  `let x: f64 = a` keeps the int value, so `w6` prints `1` and `f1_f64_arith` (`x + 0.5`) panics `cannot apply Add to i64 / f64`.

So a component bound to `i64` becomes as strict as a `let`-bound int, and every int component used at another width became E0102.
The `let` destructure twin is already that strict: `let (a, b) = t; let x: u8 = a` is E0102 (`ICS/p7/w2_letdestr.ax`), because the
parser desugars it into `.0`/`.1` field reads (`parser.rs:2479-2500`) and the tuple field read returns the element type
(`infer.rs:2350-2362`). The `match` path was left lenient on purpose. TP-1 keeps that leniency **for numbers only**, because a width
mismatch is not a kind mismatch and the interpreter runs it. `[REVISED: TPA-1]` The leniency now applies to each numeric **leaf**,
not to a whole element that holds a number: `Option<i64>` binds as `Option<?c>`, not as an unchecked var.

### Out of scope (logged, not dropped)

| what | why it is not here | where it lives |
|---|---|---|
| **O-LF-02** `let (a, b) = p` with `p: (i64, i64, i64)` (`p/l1_let_short.ax` rc 0, `3`; same for `for (a, b) in [(1, 2, 3)]` `l3`, a lambda's `let` `l5`, nested `for (i, (a, b))` `l6`, `let (a, _) = ..` `l8`) | **Considered; does not fit this batch.** (1) It is a different mechanism. There is no pattern at inference time: `parse_let` (`parser.rs:2460-2500`) desugars to `let __tup_N = e` plus one `let name = __tup_N.i` per name, `for_pat_lets` (`parser.rs:66`) does the same for `for`, and `push_stmt_or_splice_destructure` (`parser.rs:31`) splices those lets into the enclosing block so the names outlive it. A missing name means a missing `.i` read, which nothing can refuse. A longer pattern is already E0401 (`l2_let_long`, `checker.rs:7693`). (2) The behaviour is documented: lang-forms LF-3 inherits it on purpose so `for` and `let` agree (`tasks/lang-forms-spec.md:60`, `:472`). (3) Every fix refuses programs that run today (l1, l3, l5, l6, l8), which is a language change needing its own human decision. (4) The fix touches `parser.rs` and `fmt.rs` (the `__tup_` round trip, `fmt.rs:1259-1273`), not `infer.rs`. Direction for a follow-up: keep the field-let desugar and add one statement that carries the arity (e.g. a synthetic `let _ = __tup_N` with an arity mark that infer checks under the CS4-3 concrete-subject rule), so scoping and fmt are unchanged | O-LF-02 (unchanged) |
| Unresolved subjects: an untyped lambda param (`p/u1_lambda.ax`), a generic `T` (`p/u2_generic.ax`), a generic call's result (`p/a5_generic_ret.ax`, `p/h5_generic_ret_arity.ax`) | The subject is a `Var`/`TypeParam` at bind time. Checking it would refuse `l1`-shaped programs whose arms of two arities both run (CS4A-4). By design of CS4-3 | stays in the O-LF-01 stamp |
| A typed lambda param is untyped in inference: `\|p: (i64, str)\| match p { (a, b, c) => 3, _ => 0 }` checks rc 0 and prints `0` (`p/a2_lambda_typed_arity.ax`); `p/u4_lambda_typed.ax` panics | `Expr::Lambda` binds every param to a fresh var and ignores its annotation (`infer.rs:2422`, `:2429-2431`). That is a lambda typing gap, not a pattern gap | log as **O-TP-01** `[low]` |
| A non-tuple sub-pattern of a tuple pattern (`(Some(a), b)` on `(Some("x"), 2)`, `p/r3_nested_some_in_tuple.ax` panics; CS4's `t_nested_ty`) | Binding it with the element would type the `Some` payload fully, as Fix #8 does at the top level. That brings back the ICS-1 width refusals for `(Some(1), 2)` with `a` used as `u8`. A kind-only rule for the payload is a separate design | log as **O-TP-02** `[low]` |
| A struct or variant pattern's plain field (`S::C { r } => to_str(r + 1)` with `r: str`, `p/e2_variant_field.ax` panics) | Not a tuple pattern. `Pattern::Struct` binds every field with `None` (`infer.rs:3072-3076`). TP-4 types only tuple sub-patterns of declared tuple fields | log as **O-TP-03** `[medium]` |
| Mixed int/float components (`(a, b)` on `(1, 2.5)`, `a + b`, `p/d6m_int_plus_float.ax` panics `cannot apply Add to i64 / f64`; `f1_f64_arith`) | TP-1's numeric leaf leniency (the ICS-1 cases) accepts any number for any number. Refusing it needs width-correct numbers (R19 Slice B), not pattern typing | stays in the O-LF-01 stamp as the numeric residual |
| `[REVISED: TPA-1]` A component whose element is still a type var at bind time (a generic fn's tuple result, e.g. `fn two<T>(x: T) -> (T, i64)` with `two("s")`, then `a + 1`; `LFT/r2/y5_two_misuse.ax` check 0, panic 101) | Var leaves bind exactly, like the `let` twin (D8). The twin is just as lenient for generic results: `y1t`/`y3t` check 0. Closing it is generic instantiation work, not pattern typing `[INFERENCE: stays a panic]` | stays in the O-LF-01 stamp |
| `[REVISED: TPA-2]` A numeric component wrapped before it reaches a fn value (`f(Some(a))` through a lambda) | D6 maps only an argument that is exactly a component var | stays in the O-LF-01 stamp `[INFERENCE: not probed]` |
| Literal sub-patterns (`("a", y)` on `(i64, i64)`) | O-CS4-04, unchanged (control `t_lit_ty`) | O-CS4-04 |

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | TP-1, TP-2 | Check rc 0 and then a runtime panic. The claimed property "checked programs do not hit type panics" is not enforced for the most common destructuring form after `let` |
| P2 | TP-3 | Same, inside `Some`/`Ok`/`Err` payloads. Less common in the corpus (§3) |
| P3 (optional) | TP-4 | Tuple fields in struct/variant patterns. Rare; NH-2 |

## 2. Items

### TP-1 (P1) Tuple-pattern components are typed; numbers only by kind (O-LF-01 remainder)

**Anchors (4199c91c, `crates/axon-core/src/infer.rs`).**
- `:3019-3109` `bind_pattern_with_subj`. `:3028-3035` `Pattern::Ident` binds `subj_ty` when given, else a fresh var. `:3088-3107`
  `Pattern::Tuple(pats)`: `concrete` (`:3089-3099`) is `subj_ty` when it is not `Var`/`Deferred`/`Unknown`/`TypeParam`/`DynTrait` and
  `pats` is non-empty. A concrete subject is constrained to `Tuple(fresh vars)` (`:3100-3103`, the CS4-3 shape check). Then **every
  sub-pattern is bound with `None`** (`:3104-3106`). The comment at `:3077-3087` records the ICS-1 narrowing.
- `:2038-2066` `Expr::Match` infers the subject once and passes `Some(&subj_ty)` to every arm (`:2048`, `:2056`, `:2062`). `:2993-3009`
  `infer_match_arm_with_subj`. `if let` is a two-arm match (`ast.rs:468` `is_if_let_desugar`, used at `:2053`). `while let` passes
  `Some(&expr_ty)` (`:2490-2497`).
- `:3412-3504` `solve`. Constraints are unified first (`:3415-3418`), then post-solve obligations run against the substitution:
  `len_obligations` (`:3420-3428`), `call_obligations` (`:3438-3486`; CS2-6 compares with `kinds_conflict`, `:3462-3475`), and
  `num_obligations` (`:3491-3502`).
- `:338-365` `kinds_conflict(param, arg)`: true only for two scalars of different kinds (number / `str` / `bool` / unit), compared
  through `[T]`, `Option`, `Result` and same-length tuples. Any number against any number is false. `:329-336` `has_unresolved`,
  `:105-110` `type_is_unresolved`. `types.rs:137` `is_numeric` (int, float, decimal). `types.rs:312` `map_leaves`.
- `:576-581` `InferError::mismatch(origin, expected, found)` is E0102 "type mismatch in {origin}".
- Builtins as they bear on components: `println`'s param is `str` (`infer.rs:4005-4009` unit test). `to_str` accepts any scalar, `str`
  or unresolved arg without constraint (`:2727-2757`). `len` defers to a post-solve `[T]`-or-`str` check (`:3420-3428`).

**Reproduction** (`LFT/p/`, reference binary; full log §9).

| probe | shape | check | run | after TP-1 |
|---|---|---|---|---|
| `c1_comptype` | `match (1, "s") { (a, b) => println(to_str(b + 1)) }` | 0 | 101 `cannot apply Add to str / i64` | E0102 :2 |
| `c2_iflet` | `let t = (1, "s")`; `if let (a, b) = t { .. b + 1 .. }` | 0 | 101 (same) | E0102 :3 |
| `c3_param` | `fn f(t: (i64, str)) -> i64 { match t { (a, b) => b + 1 } }` | 0 | 101 (same) | E0102 :2 |
| `c4_ret` | subject `mk()`, `fn mk() -> (i64, str)` | 0 | 101 (same) | E0102 :3 |
| `c5_index` | subject `arr_enumerate(["a", "b"])[0]`, `s + 1` | 0 | 101 (same) | E0102 :3 |
| `c7_strtoint` | `(a, b) => .. h(b)`, `h(x: i64)` | 0 | 101 `cannot apply Mul to str / i64` | E0102 :3 |
| `c8_lenint` | `(a, b) => .. len(a)` on `(1, "s")` | 0 | 101 `len: expected str/array/dict, got i64` | E0102 :2 |
| `g1_cross` | `match (1, "s") { (a, b) => println(to_str(a + b)) }` | 0 | 101 `cannot apply Add to i64 / str` | E0102 :2 |
| `a3_field_subject` | subject `r.t`, field `t: (i64, str)` | 0 | 101 | E0102 :4 |
| `a6_for_elem` | `for p in [(1, "s")] { match p { (a, b) => .. b + 1 .. } }` | 0 | 101 | E0102 :2 |
| `a8_index_misuse` | subject `ps[0]`, `ps = [(1, "s")]` | 0 | 101 | E0102 :3 |
| `r2_str_len_int` | `(a, b) => .. str_len(a)` on `(1, "s")` | 0 | 101 `expected str, got i64` | E0102 :2 |
| `d2_println_int` | `match (1, "s") { (a, b) => println(a) }` | 0 | **0**, `1` | **new refusal** E0102 :2 (NH-1) |
| `d4_eq_str_int` | `(a, b) => if b == 1 {..}` on `(1, "s")` | 0 | **0**, `n` | **new refusal** E0102 :2 (NH-1) |
| CS4 control `ics_q4` (`LFT/p/q4`) | int component, `let s: str = a` | 0 | **0**, `1` | **new refusal** (NH-1) |
| `ics_q5` | int component to `sh(x: str)` | 0 | **0**, `1` | **new refusal** (NH-1) |
| `ics_q7` | int component into a `str` field | 0 | **0**, `1` | **new refusal** (NH-1) |
| `ics_q11` | bool component to `ni(x: i64)` | 0 | **0**, `true` | **new refusal** (NH-1) |
| `ics_q18` | str component, `let x: i64 = a` | 0 | **0**, `s` | **new refusal** (NH-1) |
| `ics_q19` | float component, `let x: str = a` | 0 | **0**, `1.5` | **new refusal** (NH-1) |
| `[REVISED: TPA-3]` `v4_println_bool` (`tpa/`) | `match (true, 1) { (a, b) => println(a) }` | 0 | **0**, `true` | **new refusal** E0102 :2 (NH-1) |
| `[REVISED: TPA-3]` `v6_eq_opt_str` (`tpa/`) | `(a, b)` on `(Some("x"), 1)`, `if a == "x"` | 0 | **0**, `n` | **new refusal** E0102 :2 (NH-1) |
| `[REVISED: TPA-2]` `z1_closure_runs` (`LFT/r2/`) | `(a, b)` on `(1, 2)`, `let g = \|x\| sh(x); g(a)`, `sh(x: str) -> i64 { 1 }` | 0 | **0**, `1` | **new refusal** E0102 :3 (NH-1, D6) |
| `[REVISED: TPA-1]` `u1_slice_as_str`, `u2_option_as_int`, `u3_generic`, `v5_tostr_numarr` (`tpa/`) | `[i64]` given to `str_len`; `Option<i64>` as `let x: i64`; an int as `first<T>(xs: [T])`; `[i64]` given to `to_str` | 0 | 101 (`expected str, got []`; `cannot apply Add to Option / i64`; `indexing non-array (i64)`; `to_str: expected a scalar ..`) | E0102 :2, :2, :3, :2 |
| `[REVISED: TPA-2]` `u4_closure` (`tpa/`) | `(a, b)` on `(1, "s")`, `let f = \|x\| str_len(x); f(a)` | 0 | 101 `expected str, got i64` | E0102 :2 (D6) |
| `[REVISED: TPA-1]` `v1_tostr_opt`, `v2_tostr_arr`, `v3_tostr_struct` (`tpa/`, not in the audit text) | `to_str` of an `Option<str>` / `[str]` / struct component | 0 | 101 `to_str: expected a scalar ..` | E0102 :2, :2, :3 |
| `[REVISED: TPA-1]` `w4_int_as_bool` (`LFT/r2/`) | `(a, b)` on `(1, 2)`, `if a {..}` | 0 | 101 `if condition must be bool, got i64` | E0102 :2 |
| ICS `w4`..`w9`, `v3`, `v1` | int component used as `u8`/`i32`/`f64`, also inside `Some((a, b))` (`v1`) | 0 | 0 (`1`, `44`, `1`, `3`, `s6`, `7`, `1`, `3`) | **unchanged** (controls; the ICS-1 regression guard) |
| `d1_tostr_str`, `d3_interp_int`, `d5_float_mix`, `k2_u8_cast`, `k4_f64_cast`, `k5_u8_wrap_param`, `a9_ok_tuple_lit` | valid uses | 0 | 0 (`s`, `1s`, `5`, `1`, `1.5`, `300`, `4`) | unchanged (new controls) |
| `[REVISED: TPA-1]` `w1_swap_u8`, `y1_two_u8`, `y2_two_param_u8`, `y3_two_f64` (`LFT/r2/`) | generic fn tuple result (var leaves) used at `u8`/`f64` | 0 | 0 (`2`, `1`, `3`, `1`) | unchanged (controls, D8; twins `w1t`, `y1t`, `y2t`, `y3t` check 0) |
| `[REVISED: TPA-1]` `w2_unc`, `w7_unc_let`, `w8_decimal` (`LFT/r2/`) | int component given to an `Uncertain<i64>` param; `let x: Uncertain<i64> = a`; `let x: Decimal = a` | 0 | 0 (`3`, `k`, `k`) | unchanged (controls, D7) |
| `[REVISED: TPA-1]` `w3_arr_u8`, `w5_opt_u8`, `z2_closure_num` (`LFT/r2/`) | `[i64]` component as `[u8]`; `Option<i64>` as `Option<u8>`; an int component through a numeric lambda | 0 | 0 (`1`, `k`, `4`) | unchanged (controls: numeric leaves inside a structure) |
| `e_ok` (`tpa/`) | the TP-4 control | 0 | 0, `s1` | unchanged |
| `d6m_int_plus_float`, `f1_f64_arith` | number vs number | 0 | 101 `cannot apply Add to i64 / f64` | unchanged (numeric residual, §0) |

Each "new refusal" shape has a `let` twin that is refused today: `q4t` E0102 :3, `q11t` E0306 :4, `q18t` E0102 :3, `d2t` E0306 :3,
`d4t` E0102 :3, `v4t` E0306 :3, `v6t` E0301 :3, `z1t` E0102 :6. The panicking shapes' twins are refused too: `l4_let_comp`
(`let (a, b) = (1, "s"); b + 1`) is E0301 :3, and `u1t` E0306, `u2t` E0102, `u3t` E0102, `u4t` E0102, `v1t` E0306, `v5t` E0306.

**Fix design.** `[REVISED: TPA-1, TPA-2]` In `Pattern::Tuple(pats)` (`:3088-3107`), keep `concrete` and the shape constraint
unchanged. Change only how sub-patterns are bound when the subject is concrete. A non-concrete subject binds every sub-pattern with
`None`, as today.
1. Keep the fresh `vars` and constrain `st` to `Type::Tuple(vars.clone())` (CS4-3).
2. Element types known now: if `st` is structurally `Type::Tuple(ts)` with `ts.len() == pats.len()`, `elems = Some(ts)`. Otherwise
   (a shape mismatch, which step 1 already reports) `elems = None`, and every sub-pattern is bound with `None`, as today.
3. For sub-pattern `i`:
   - `Pattern::Ident(name)`: **skeleton binding.** Bind `name` to `elems[i].clone().map_leaves(&mut |t| if t.is_numeric() { let c =
     self.fresh(); push (t, c, name, self.current_stmt_span) onto tuple_component_obligations; c } else { t })`. `map_leaves` is at
     `types.rs:312` and `is_numeric` (int, float, decimal) at `types.rs:137`. The new `InferCtx` field
     `tuple_component_obligations: Vec<..>` is declared and initialised next to `num_obligations` (`:924`, `:1060`). Every
     non-numeric leaf, including a `Var`/`TypeParam` leaf (D8), binds exactly, as the `let` twin's `.i` read does. So `str`, `bool`,
     unit, struct, enum and the shape of `[..]`/`Option`/`Result`/nested tuples are exact, and only the numbers inside are free.
   - `Pattern::Tuple(_)`: TP-2.
   - every other sub-pattern (`Wildcard`, `Literal`, `None`, `Some`/`Ok`/`Err`, `Struct`): `None`, as today (O-TP-02, O-CS4-04).
4. In `solve`, after the `num_obligations` loop (`:3491-3502`), so that `len` and fn-value obligations have already bound what they
   bind: for each obligation `(t, c, name, span)`, let `used = strip(subst.apply(&c))`. `strip` removes `Uncertain(..)` and
   `Temporal(..)` wrappers (D7), because `unify` binds a `Var` before its soft wrapper rule (`infer.rs:3517-3534` before
   `:3601-3604`), so `c` can resolve to `Uncertain<i64>`. Push `InferError::mismatch(&format!("use of tuple pattern component
   `{name}`"), &t, &used).with_span(span)` iff `!type_is_unresolved(&used) && !used.is_numeric() && used != Type::Never`. That reads
   as "type mismatch in use of tuple pattern component `a` (expected i64), found str". The obligation binds nothing.
5. `[REVISED: TPA-2]` CS2-6 extension (D6). At the start of the CS2-6 loop (`:3438`), build `comp: HashMap<u32, Type>`: for each
   tuple-component obligation whose `subst.apply(&c)` is `Type::Var(r)`, insert `r -> t`. That is keyed by the representative, so a
   component var unified with another var is still found. In the argument loop (`:3462-3465`), after `let arg = subst.apply(arg)`,
   replace `arg` by `comp[r]` when `arg` is `Type::Var(r)` and `r` is in the map. Then `kinds_conflict(param, arg)` compares the
   derived param kind with the element's leaf: for `u4`, `str` against `i64` gives E0102 "arg 0 of a call through a fn value", the
   same error its twin `u4t` gets today. The extension binds nothing. Only an argument that is exactly a component var is mapped.

Why this shape:
- Numbers stay lenient, one leaf at a time. An int component used as `u8`/`i32`/`f64`/`Decimal`/`Uncertain<i64>` resolves `c` to a
  number (after `strip`). So `ics_w4..w9`, `v1`, `v3`, `w2`, `w7`, `w8` keep running, and the M4714 regression cannot come back
  while each numeric leaf gets its own var.
- Non-numeric structure is exact, so cross-component misuse and shape misuse are caught. In `g1_cross`, `b: str` exactly, so `a + b`
  resolves `c_a` to `str` (`BinOp::Add` constrains the operands equal, `:1767`), and the obligation fires. In `u1`, `a: [?c]`, and
  `str_len(a)` is refused at the use. `u3`'s `c` resolves to `[T]`, which is not a number.
- **Refusal invariant** `[INFERENCE: argued, not built]`: every program TP-1 refuses has a refused `let` twin. The skeleton differs
  from the twin's exact element type only in that numeric leaves are free vars, so any unification it fails, the twin also fails.
  The obligation fires only when a numeric leaf's use resolved to a non-number (after `strip`), and the twin's `i64`/`f64` cannot
  unify with that either (`unify`, `:3506-3648`; `Never` excluded, wrappers stripped). D6 compares exactly what CS2-6 compares for
  the twin. The converse does not hold, which is the point: width-only differences (the ICS-1 class) stay legal.
- Spans: an exact-binding refusal reports at the use (existing origin, e.g. "arithmetic operands"). An obligation refusal reports at
  the statement that holds the pattern. All test cases are one-line arms, so both are the same line.

**Control trace** `[REVISED: TPA-1]` (by reading the rule against each control's source, `cli_run.rs:47131-47236`;
`[INFERENCE: traced, not built]`). Each stays legal:
- `ics_w4`/`w5`/`w6`/`w9`: `a` is `?c` (element `i64`); `let x: u8`/`f64` and the `u8` field bind `c` to a number, so no error.
  `ics_w7`/`v3`: `sh(a)` binds `c = u8`. `ics_w8`: `a: ?c` becomes `i32`, `s: str` exact, `s + to_str(..)` is str + str.
  `ics_v1` (`Some((a, b))`, TP-3): payload `(i64, i64)` gives two numeric vars, and `sh(a)` binds `u8`.
- `t_ok`: `(0, s)` binds `s: str` exact, and `s + "!"` is fine. `(n, s)` binds `n: ?c`; `to_str(?c)` returns `str` unconstrained
  (`:2746`), so `c` stays unresolved and nothing errors. `(Some(a), b)` on `(Option<i64>, i64)`: `Some(a)` is bound with `None`
  (D5), `b: ?c`, and `a + b` unifies two vars. `((a, b), c)` (TP-2): a nested concrete `(i64, i64)` gives numeric vars.
- `t_ok_arr` / `g5`: `(n, s)` binds `n: ?c`, `s: str`, and `to_str(n) + s` / `s + to_str(n)` is str + str. `d[0]` `(a, b) => a * b`
  is two vars. `while let (a, b) = q` is two vars.
- `t_ok_iflet`: `(1, b)` binds `b: str`, and `println(b)` is fine; `(a, "s")` binds `a: ?c`, and `to_str(a)` is fine.
- `t_generic`: `swap<A, B>(p: (A, B))` gives elements `TypeParam(A)`/`TypeParam(B)`, which bind exactly (D8), and `(b, a)` matches
  the declared `(B, A)`. At the caller, `swap((1, "x"))` gives a `Tuple` of instantiated vars, which bind exactly and resolve as the
  twin does.
- `t_lit_ty`: `("a", y)` binds `y: ?c`; `(x, _)` binds `x: ?c'`; the arm join unifies two vars.
- `g4`: `Some((a, (_, c)))` (TP-3 + TP-2) gives `a`, `c` numeric vars, and `a + c` unifies them. `h<A, B>`'s `Some((_, _))` has
  wildcards only.
- `t_ok_lambda`, `t_tparam`, `l1`, `l2`, `g2`, `g3`: the subject is a `Var`/`TypeParam`, so nothing changes. `t_wild`, `unit`,
  `unit_on_int`: no non-empty tuple sub-pattern binds a name.
- New controls, measured legal today (§9): `w1`, `y1`, `y2`, `y3` (var leaves bind exactly, and the twins check 0), `w2`, `w7`, `w8`
  (wrappers and Decimal), `w3`, `w5` (numeric leaves inside `[..]`/`Option`), `z2` (D6 maps `a` to `i64`, and the lambda param has
  no derived kind).
Running programs this rule refuses beyond the reviewer's list, found while tracing: `w4_int_as_bool` panics today (so it is not a
running-program refusal) and `z1_closure_runs` (D6, added to NH-1). I found no other shape. The class statement in NH-1 covers any
I missed, because each such refusal has a refused `let` twin.

`[INFERENCE]` The checker (`checker.rs`) does not see pattern-bound component types: it reported nothing for c1..c8 today, while it
reports `l4`'s let twin as E0301. The build may still show a checker code first on some case. The lane then records the observed code
in its fail-first log and the test pins it with a `[REVISED]` note; a looser assert is never the remedy.

**Error code decision.** E0102 (D1). No new code; no `AXON_REFERENCE.md` diff.

**Docs.** `[REVISED: TPD-6]` `spec/axon-for-llms.md`, after `:86` (the "Pattern matching" comment block, inside a
`doc_snippets`-tested code block, `doc_snippets.rs:35`). Add one `//` comment line: `// a tuple pattern's components take the element
types (numbers at any width): (n, s) on (i64, str) makes s + 1 an error`. It goes there and not after `:78`, because `:77`-`:78` are
the `for`/`let` destructure desugars, which this spec does not change (D2). `spec/language-tour.md:188` (only `for` destructuring)
and `AXON_REFERENCE.md` (E0102 already covers it) are intentionally unchanged. Run `doc_snippets` (§5).

**Fail-first tests** (`crates/axon-core/tests/cli_run.rs`, appended at the end under `// -- tuple-patterns TP-1..TP-4 --`; helpers
`sf6_check_and_run` `:42297` and `cs4_first_error` `:47059`; one-line `fn main() {\n{body}\n}\n` programs copied from the probe files,
so the lines are as in the tables):
- `tuple_pattern_components_are_typed`: c1, c2, c3, c4, c5, c7, c8, g1, a3, a6, a8, r2. `[REVISED: TPA-1, TPA-2]` Plus u1 :2,
  u2 :2, u3 :3, u4 :2, v1 :2, v2 :2, v3 :3, v5 :2, w4 :2. Each must give check rc 2, E0102 first at the table's line, and `run` rc 2
  with empty stdout. Fails on base (rc 0). Marker `ATTACK: a tuple pattern component of the wrong type checked clean`.
- `tuple_pattern_component_misuse_that_runs_is_refused` (NH-1 option (a)): `[REVISED: TPA-5]` d2 :2, d4 :2, q4 :2, q5 :3, q7 :3,
  q11 :3, q18 :2, q19 :2, and `[REVISED: TPA-3]` v4 :2, v6 :2, z1 :3, with the same asserts. Fails on base (rc 0, and each prints).
  Marker `ATTACK: a tuple pattern component used as another kind checked clean`.
- `tuple_pattern_controls_stay_legal` (`cli_run.rs:47127`), edited:
  - remove `ics_q4`, `q5`, `q7`, `q11`, `q18`, `q19` (`:47231-47236`; they move to the test above);
  - add `ics_v1` (`ICS/p9/v1_some_tuple.ax`, `3\n`), `tp_d1` (`s\n`), `tp_d3` (`1s\n`), `tp_d5` (`5\n`), `tp_k2` (`1\n`), `tp_k4`
    (`1.5\n`), `tp_k5` (`300\n`), and `tp_r1` (`p/r1_struct_comp.ax`, `Ok((a, pp)) => a * pp.y`, `6\n`; a struct component is
    exactly typed and its field read must still check);
  - `[REVISED: TPA-1]` add `tp_w1` (`2\n`), `tp_y1` (`1\n`), `tp_y2` (`3\n`), `tp_y3` (`1\n`), `tp_w2` (`3\n`), `tp_w7` (`k\n`),
    `tp_w8` (`k\n`), `tp_w3` (`1\n`), `tp_w5` (`k\n`), `tp_z2` (`4\n`) (sources `LFT/r2/*.ax`);
  - rewrite the doc comment's `[REVISED: ICS-1]` sentence: components bind the element's skeleton, and numeric leaves are free.
  It passes on base (every added case is rc 0 today, §9).

**g01 rows**: M5220, M5221, `[REVISED: TPA-1, TPA-2]` M5226, M5227, M5228. Re-point M4713 and M4714 (§4).

**Acceptance.** The three tests pass. The sweep shows no tracked file going from rc 0 to nonzero (§3). `ICS/p7/w4..w9`, `p9/v1`,
`p9/v3`, and `LFT/r2/` w1, w2, w3, w5, w7, w8, y1, y2, y3, z2 stay rc 0 with today's stdout.

### TP-2 (P1) A nested tuple pattern is checked against its element type

**Anchors.** Same arm, `infer.rs:3104-3106`: a nested `Pattern::Tuple` is bound with `None`. Its own `concrete` is then `None`, so it
is neither shape-checked nor typed. The CS4-3 controls `t_ok` (`((a, b), c)` on `((1, 2), 3)`) and `g4` (`Some((a, (_, c)))`) must
stay legal (`cli_run.rs:47132-47136`, `:47186-47189`).

**Reproduction.**

| probe | shape | check | run | after TP-2 |
|---|---|---|---|---|
| `n1_nested_ty` | `match ((1, "s"), 2) { ((a, b), c) => println(to_str(b + c)) }` | 0 | 101 `cannot apply Add to str / i64` | E0102 :2 |
| `n2_nested_arity` | `((a, b), c)` on `((1, 2, 3), 4)` | 0 | 101 `no match arm matched` | E0102 :2 |
| `n3_nested_nontuple` | `((a, b), c) => .., _ => println("o")` on `(5, 4)` | 0 | **0**, `o` | **new refusal** E0102 :2 (dead arm, D3) |
| `n4_nested_ok` | `((a, b), c) => println(b + to_str(a + c))` | 0 | 0, `s3` | unchanged (control) |

**Fix design.** In TP-1 step 3, bind a `Pattern::Tuple` sub-pattern with `Some(&elems[i])`. When `elems[i]` came from a structural
`Tuple`, it is concrete if the outer element is (a tuple literal, an annotated param, a call result). The nested arm then applies the
CS4-3 shape check and TP-1 recursively. When `elems[i]` is a var (`elems = vars`), the nested `concrete` is `None`, as today.

**Fail-first test**: `tuple_pattern_nested_and_payload_shapes_are_checked` (shared with TP-3): n1, n2, n3 give rc 2, E0102 first at
:2, and `run` rc 2 with empty stdout. `[REVISED: TPA-4]` Only the shape cases (n2, n3, s2, h4) also assert that the diagnostics
contain `tuple pattern`. n1, s1 and s3 refuse at the use ("arithmetic operands" or the component origin). Fails on base. Marker
`ATTACK: a nested tuple pattern of the wrong shape checked clean`. Controls: add `tp_n4` (`s3\n`) to
`tuple_pattern_controls_stay_legal`.

**g01 row**: M5222.

### TP-3 (P2) A tuple pattern inside `Some`/`Ok`/`Err` is checked against the payload type

**Anchors.** `infer.rs:3037-3071`: `Ok`/`Err`/`Some` constrain the subject to `Result(fresh, fresh)` / `Option(fresh)` (Fix #8) and
bind the inner pattern with `Some(&inner_var)`, a fresh `Var`. For an inner `Pattern::Tuple`, `concrete` rejects the `Var`
(`:3093`), so the payload tuple is never checked. An inner `Pattern::Ident` is fully typed through `inner_var` (`ICS/p7/w3_some.ax`
`Some(a)`, `let x: u8 = a` is E0102 today). TP-3 does not change that.

**Reproduction.**

| probe | shape | check | run | after TP-3 |
|---|---|---|---|---|
| `s1_some_ty` | `match Some((1, "s")) { Some((a, b)) => .. b + 1 .., None => .. }` | 0 | 101 `cannot apply Add to str / i64` | E0102 :2 |
| `s3_ok_ty` | subject `mk()`, `fn mk() -> Result<(i64, str), str>`, `Ok((a, b)) => .. b + 1 ..` | 0 | 101 (same) | E0102 :3 |
| `s2_some_arity` | `Some((a, b))` on `Some((1, 2, 3))`, `_ => println("other")` | 0 | **0**, `other` | **new refusal** E0102 :2 (dead arm, D3) |
| `h4_ok_arity` | `Ok((a, b))` on `Result<(i64, i64, i64), str>`, `_ => println("o")` | 0 | **0**, `o` | **new refusal** E0102 :3 (dead arm, D3) |
| `s4_some_ok`, ICS `v1` | valid; `v1` uses an int component as `u8` | 0 | 0 (`s1`, `3`) | unchanged (controls) |

**Fix design.** In each of the three arms, when the inner pattern is a `Pattern::Tuple` and `st` is structurally the matching
container at bind time (`Type::Option(t)` for `Some`, `Type::Result(o, e)` for `Ok`/`Err`), bind the inner pattern with `Some(t)`
(resp. `o`, `e`) instead of `Some(&inner_var)`. The Fix #8 constraint stays as it is. Every other inner pattern is unchanged. The
components inside then follow TP-1, so `v1` (`Some((a, b))`, `sh(a)` with `sh(x: u8)`) stays legal: each numeric leaf binds a free
var. `[INFERENCE]` A call to a non-generic fn yields its declared return type at bind time: the CS4-3 shape check already fires on
`match mk() { (a, b) .. }` with a 3-tuple return (`p/h1_call_arity.ax` E0102 :3), so `s3`/`h4`'s `Result` should be structural too.
If the build shows otherwise, TP-3 falls back to `inner_var` for those, the lane drops `s3`/`h4` from the test with a `[REVISED]`
note, and `[REVISED: TPA-6]` drops M5224, which then has no killer, noting it under §4.

**Fail-first test**: s1, s3, s2, h4 in `tuple_pattern_nested_and_payload_shapes_are_checked`. Controls: `tp_s4` (`s1\n`), plus
`ics_v1` (TP-1).

**g01 rows**: M5223 (`Some`), M5224 (`Ok`).

### TP-4 (P3, optional, NH-2) A tuple pattern in a struct or variant field is checked against the declared field type

**Anchors.** `infer.rs:3072-3076` `Pattern::Struct { fields, .. }` binds every field's pattern with `None`. Declared field types are
in `struct_fields` (`:929`; generic structs list their params in `struct_generic_params`, `:931`) and `enum_variant_fields` (`:960`).
`Expr::StructLit` resolves an `Enum::Variant` or struct name against them at `:2151-2175`. Patterns parse `W::P { t: (a, b) }`
(grammar `spec/grammar.ebnf:507-509`).

**Reproduction.**

| probe | shape | check | run | after TP-4 |
|---|---|---|---|---|
| `e1_variant_tuple` | `enum W { P { t: (i64, str) }, N }`; `W::P { t: (a, b) } => .. b + 1 ..` | 0 | 101 `cannot apply Add to str / i64` | E0102 :4 |
| `e3_variant_arity` | `t: (i64, i64, i64)`; `W::P { t: (a, b) } => .., _ => println("other")` | 0 | **0**, `other` | **new refusal** E0102 :4 (dead arm, D3) |
| `e2_variant_field` | `S::C { r } => to_str(r + 1)`, `r: str` | 0 | 101 | unchanged (O-TP-03) |

**Fix design.** In `Pattern::Struct { name, fields }`, resolve `name` the way `Expr::StructLit` does (`Enum::Variant` through
`enum_variant_fields`, else `struct_fields`). For each `(fname, pat)` where `pat` is a `Pattern::Tuple` and the declared type of
`fname` is a `Type::Tuple` with no unresolved leaf (so generic fields are skipped), bind `pat` with `Some(&declared)`. Every other
field keeps `None`. An unknown name or field keeps `None` (the checker reports it, as today).

**Fail-first test**: `tuple_pattern_in_a_struct_field_is_checked`: e1, e3 give rc 2, E0102 first at :4, `run` rc 2 with empty
stdout. Marker `ATTACK: a tuple pattern in a struct field checked clean`. Control: `tp_e_ok` (`W::P { t: (a, b) } => println(b +
to_str(a))` on `W::P { t: (1, "s") }`, `s1\n`) in `tuple_pattern_controls_stay_legal`. `[REVISED: TPA review]` Probed by the
audit: `tpa/e_ok.ax` is rc 0 and prints `s1` today (I re-ran it).

**g01 row**: M5225.

## 3. Shared mechanics

- **Corpus sweep (fail-first gate step, before and after every item).** `/var/tmp/r51-tmp/Soundness4/sweep.sh <repo> <bin>
  <ABSOLUTE out.tsv>` runs `axon check` on every `git ls-files '*.ax'` from the file's directory and records path, rc and sha256 of
  the diagnostics. **The script `cd`s into `<repo>` first, so a relative `<out>` is written inside the worktree. Always pass an
  absolute path** (observed while measuring the baseline, §9). Baseline at 4199c91c with the reference binary: **717 files, 626 rc 0,
  91 rc 2**, two runs byte-identical, 0.94 s wall (`/var/tmp/r51-tmp/LFT/base.tsv`, `base2.tsv`). `[REVISED: TPD-7]` The binary was
  built at 557e746c. There is no `crates/axon-core/src` change between 557e746c and 4199c91c, so the baseline is valid for 4199c91c.
  Rule: no file goes from rc 0 to
  nonzero, since this spec names no tracked file. An rc 2 -> 0 change or a diagnostic hash change goes in the lane's report.
  Dev-loop only (untracked script).
- **Blast radius estimate (from reading, `[INFERENCE]` until the sweep runs).** A scan of all 717 tracked `.ax` files found tuple
  match arms only at `examples/tuples.ax:38` and its copy `crates/axon-cortex/benchmarks/hardset/src/tuples.ax:38`: `(a, b) => a +
  b` on `let m = (10, 20)`, both numeric, both rc 0 today. That includes the 28 genviab files under
  `crates/axon-intent/benchmarks/genviab/`. No tracked `.ax` file has `if let (`, `while let (` or a tuple inside a `Some`/`Ok`/`Err`
  pattern. Expected after: 0 files change. Tuple patterns in programs embedded in tests, read one by one (all kind-consistent, so
  expected unchanged): `cli_run.rs:7598` (`(a, a)`, E0002 test), `:7625`, `:34340` (`Ok((a, pp)) => a * pp.y`, the `tp_r1` shape),
  `:37518`, `:37738`, `:43991`, `:44046`, `:44051`, `:44071`, `:44088` (lambda subject, unchecked), `:44098`, `:46183`, the CS4-3
  tests `:47079-47247`, and `integration_fixtures.rs:2860-2884` (the native tuple-pattern cases: v22, v25, w08, w09, t06, t09, t14,
  t17, t21, t27, u01, okl, t04, t05, t11, t12, u06, v26). The full `cli_run` and `integration_fixtures` runs in the strict gate are
  the check.
- **Probe re-run after the build.** Run `LFT/run.sh` with `AXON=<built axon>` over `LFT/p/*.ax`, `LFT/r2/*.ax`,
  `/var/tmp/r51-tmp/review-TP/tpa/*.ax`, `ICS/p7/*.ax` and `ICS/p9/*.ax`. Every rc change against §9 must be a row of this spec's
  tables. Any other change is a stop: report it, do not adjust the design in the lane.
- Each behaviour-changing commit touching `crates/axon-core/src` gets a pci theme (`governance/notes/pci-themes/<sha8>.json`
  `{"theme","text"}`, then `python3 tasks/r51-pci-regen.py`, then `python3 scripts/pci_delta.py --check`) and a CHANGELOG line
  (TP-1: components typed by kind, numbers lenient, the NH-1 refusals named; TP-2/3: nested and payload shapes; TP-4: struct fields).
- g01 rows (`scripts/v022_g01_mutations.py`, tuple format of `:14587-14605`: `(id, title, file, old, new, crate, test args,
  killer)`) and `ATTACK:` markers (`scripts/v022_attack_markers.py`, `ATTACK_MARKERS.update({..})` as at `:4022-4030`) land in the
  fix commit. Verify each with `python3 scripts/v022_g01_mutations.py --only=M4713,M4714,M5220,...` (the `--only=` form, CS4D-1).
- `python3 scripts/v022_refusal_coverage.py`: run once. TP-1 adds a new E0102 push site (the obligation loop in `solve`). D6 reuses
  CS2-6's existing push. If the script lists the new site, add a coverage row, as AF-3 did for E0409.
- Docs: TP-1's `spec/axon-for-llms.md` line (after `:86`). Then `cargo test --locked -p axon-core --no-default-features --test
  doc_snippets`, `python3 -B scripts/spec_ledger.py` PASS, `cargo fmt --all -- --check`, clippy on `axon-core`.
- `tasks/opportunities.md`, by the integrator: stamp O-LF-01 FIXED for concrete subjects, keeping the residuals (unresolved subjects;
  numbers mixed across widths; non-tuple sub-patterns), and log O-TP-01..03 (§0).
- Test programs are inlined; no test reads `/var/tmp`. The interpreter is the reference. TP-1..4 are inference-only, so native
  follows: `tuple_patterns_test_every_component_natively` (`integration_fixtures.rs:2853-2922`, `cfg(feature = "codegen")`) must
  stay green `[INFERENCE: codegen does not read the binding types changed here]`. `[REVISED: TPD-1]` The default gate compiles it out
  (`gate.sh:1131` `--no-default-features`), and only `--strict` runs it (`gate.sh:1273`). So the lane runs it once with default
  features (codegen, LLVM 23 at `/usr/lib/llvm-23`) before handing over.

## 4. g01 rows (block M5220-M5239)

`[REVISED: TPA-7]` The highest reserved g01 block at 4199c91c ends at **M5219** (enable-gaps EG-2 block M5210-M5219, comment at
`scripts/v022_g01_mutations.py:16034`; its last row is M5211 at `:16041`). The EG-4 block M5150-M5209 has its comment at `:15946`
and its first row at `:15954`. No `M522x`/`M523x` appears in `scripts/`, `crates/` or `tasks/` except in this spec. So this spec
reserves **M5220-M5239**. I told peer CapLoad by message to start any new block at M5240. Nine ids are used and eleven are left.
Target `axon-core --no-default-features --test cli_run`. The builder writes the exact `old` text after the fix lands.

| id | item | guard removed by the mutant | file | killer | marker |
|---|---|---|---|---|---|
| M5220 | TP-1 | `[REVISED: TPA-1]` skeleton binding of an `Ident` sub-pattern (mutant: bind it with `None`) | infer.rs | `tuple_pattern_components_are_typed` (`g1_cross`, `u1`) | `ATTACK: a tuple pattern component of the wrong type checked clean` |
| M5221 | TP-1 | the obligation's push condition in `solve` (mutant: `false`) | infer.rs | `tuple_pattern_components_are_typed` (`c8_lenint`, `r2_str_len_int`) | same |
| M5222 | TP-2 | a nested `Pattern::Tuple` bound with `Some(&elems[i])` (mutant: `None`) | infer.rs | `tuple_pattern_nested_and_payload_shapes_are_checked` (`n2`) | `ATTACK: a nested tuple pattern of the wrong shape checked clean` |
| M5223 | TP-3 | `Some`'s structural payload for an inner tuple (mutant: `inner_var`) | infer.rs | same test (`s2`, `s1`) | same |
| M5224 | TP-3 | `Ok`'s structural payload for an inner tuple (mutant: `inner_var`); `[REVISED: TPA-6]` dropped if TP-3's fallback applies | infer.rs | same test (`s3`, `h4`) | same |
| M5225 | TP-4 | the declared tuple field type for a field's tuple sub-pattern (mutant: `None`) | infer.rs | `tuple_pattern_in_a_struct_field_is_checked` (`e3`) | `ATTACK: a tuple pattern in a struct field checked clean` |
| M5226 | TP-1 | `[REVISED: TPA-1]` the `!used.is_numeric()` term (mutant: `kinds_conflict(&t, &used)`, the draft's rule) | infer.rs | `tuple_pattern_components_are_typed` (`u3`: `c` resolves to `[T]`, so there is no scalar conflict) | same as M5220 |
| M5227 | TP-1 / D6 | `[REVISED: TPA-2]` the CS2-6 component map lookup (mutant: `arg` unchanged) | infer.rs | `tuple_pattern_components_are_typed` (`u4`) | same as M5220 |
| M5228 | TP-1 / D7 | `[REVISED: TPA-1]` `strip` of `Uncertain`/`Temporal` (mutant: no strip) | infer.rs | `tuple_pattern_controls_stay_legal` (`tp_w2`) | `ATTACK: a valid tuple match was refused \[tp_w2\]` |
| M4713 (re-pointed) | CS4-3 | `constrain(st, Tuple(vars..), "tuple pattern")` (mutant: dropped). The `old` text changes to `vars.clone()` | infer.rs | `tuple_pattern_is_type_checked` | unchanged |
| M4714 (re-pointed) | CS4-3 / TP-1 | **the numeric-leaf replacement** (mutant: `map_leaves` keeps numeric leaves, so int components bind `i64` exactly, which is the ICS-1 regression) | infer.rs | `tuple_pattern_controls_stay_legal` (`ics_w4`, the first case to fail) | `ATTACK: a valid tuple match was refused \[ics_w4\]` (unchanged) |

M4715 (the concrete-subject guard) should keep its `old` text `if let Some(st) = concrete {`. The builder checks that. The `Err`
twin of M5224 gets no row (same code shape as `Ok`).

## 5. Gates and cost budget (no paid spend; no AI path touched)

| stage | wall | source |
|---|---|---|
| corpus sweep (per item, before/after) | 0.7-0.94 s | measured at 4199c91c (§9; DevloopTP 0.74 s) |
| `[REVISED: TPD-3]` probe re-run `LFT/p` + `ICS/p7,p9` (98 files), plus `LFT/r2` (16) and `review-TP/tpa` (22) | 1.0 s for the 98 (DevloopTP, measured); the rest under 1 s | measured |
| `[REVISED: TPD-4]` cortex receipt (`scorecard.py receipt --check`, `gate.sh:714`) | 0.8 s, $0 | no file under `REPAIR_SOURCES` (`crates/axon-cortex/benchmarks/scorecard/scorecard.py:89-91`, axon-cortex only) changes; PASS at 4199c91c (DevloopTP); no regress/waive |
| `cargo test -p axon-core --no-default-features --test cli_run --no-run` cold / warm | 26 s / 4.4 s | DevloopCS3, quoted in checker-soundness-4 §5 |
| `cargo test --locked -p axon-core --no-default-features --test cli_run tuple_pattern` (5 tests, warm) | ~5-10 s | lang-forms §5 (~5 s per item's killers) |
| `[REVISED: TPD-5]` full `cargo test --locked -p axon-core --no-default-features --test cli_run` | ~8 min (488 s, `build-loop-r42.md:13`) | lane, once after the last item, before strict |
| `--lib infer::` unit tests | not measured | [INFERENCE: < 10 s warm] |
| `[REVISED: TPD-1]` `cargo test --locked -p axon-core --test integration_fixtures tuple_patterns_test_every_component_natively` (default features = codegen, LLVM 23 at `/usr/lib/llvm-23`) | ~10 min cold build [INFERENCE] (`harness-hygiene-spec.md:415`), 3-13 s warm (`build-loop-checker.md:192`) | the default gate compiles it out (`gate.sh:1131`); only `--strict` runs it (`gate.sh:1273`). Lane runs it once |
| `--test doc_snippets` | not measured | [INFERENCE: < 30 s warm] |
| one g01 row / 11 rows | ~15 s / ~3 min | 15 s per row (cs3 §5) |
| `[REVISED: TPD-2]` `scripts/gate.sh --strict` (integrator) | 2634-3273 s idle (enable-gaps §8, `:505`); 83m35s under 3-lane load (`build-loop-checker.md:43`); integrator figure ≈ 95 min (`asi-fixes-spec.md:546`) | budget 1 run + 1 retry: 1.5 h idle, up to 3.2 h loaded |

Lane cost [INFERENCE: estimates]: ~75 lines in `infer.rs` (one field, the `Pattern::Tuple` arm, `strip`, three payload arms, the
`Struct` arm, one `solve` loop, the CS2-6 map), 4 new tests + one edited test (~200 lines of `cli_run.rs`), 11 g01 rows, one doc
line. About 2.5 h of implementation including the sweep, the probe re-run and g01. `[REVISED: TPD-1, TPD-2, TPD-5]` Add ~20 min for
the full `cli_run` and the native build, then 0.75-1.6 h per strict run (1.5-3.2 h with a retry), $0.

## 6. Sequencing and lanes

One lane (all items edit `bind_pattern_with_subj`). Commits: TP-1 + TP-2 (one commit: TP-2 is one match arm inside TP-1's loop), then
TP-3, then TP-4 if NH-2 says yes. Files: `crates/axon-core/src/infer.rs` (`:329` helper, `:924`/`:1060` field, `:3037-3107`,
`:3491-3503`), `crates/axon-core/tests/cli_run.rs` (append; edit `:47115-47247`), `spec/axon-for-llms.md:78`,
`scripts/v022_g01_mutations.py`, `scripts/v022_attack_markers.py`, `CHANGELOG.md`, pci theme. Integrator-owned:
`tasks/opportunities.md`. Cross-batch: anyone editing `infer.rs` `bind_pattern_with_subj` or `solve` (check `git log 4199c91c..` on
the file at build time; re-read every anchor before editing).

Order of work `[REVISED: TPD-1, TPD-5]`: (1) write all tests and run them on base. Every refusal test must fail and every control
must pass (fail-first). (2) Baseline sweep (absolute out path). (3) The fix. (4) Targeted tests, sweep, probe re-run, g01 `--only`,
doc_snippets, spec_ledger, fmt, clippy. (5) The full `--no-default-features` `cli_run` once, then the native test once (lane, before
handing over). (6) Hand over to the integrator for strict.

## 7. Engineering decisions (decided here; canon for the build)

`[REVISED: Step 2]` D1-D8 are engineering decisions and are canon in this spec. Each changes only checker internals, or refuses only
programs of a class already decided (D3). NH-1 and NH-2 (§8) are the language-behaviour questions.

1. **D1 - E0102 for every new refusal**, as CS4-3 (D6 there). An exact-binding refusal uses the existing origin at the use. An
   obligation refusal uses the origin "use of tuple pattern component `{name}`". D6 uses CS2-6's "arg {i} of a call through a fn
   value".
2. **D2 - numbers stay lenient inside tuple patterns, one leaf at a time** `[REVISED: TPA-1]`. Each numeric leaf of an element binds
   a fresh var, which may resolve to any number. Every other leaf binds exactly. This keeps every ICS-1 case legal. It also keeps
   the numeric residual (`d6m`, `f1`) as a run panic, because a width-correct rule needs R19 Slice B. The same leniency does not
   reach `let (a, b)` (strict today, `w2_letdestr`), and this spec does not change `let`.
3. **D3 - refuse, do not warn, the dead arms that run today:** `n3_nested_nontuple`, `s2_some_arity`, `h4_ok_arity` and, with TP-4,
   `e3_variant_arity`. Same class and precedent as checker-soundness-4 D9 (`t_iflet`, `t_mixed_arity`, `t_struct_subj`), decided by
   integrator ruling 5 there. None is a tracked file.
4. **D4 - O-LF-02 is not in this batch** (§0). It is a parser-desugar change with its own documented behaviour (LF-3) and its own
   refusals of running programs.
5. **D5 - non-tuple sub-patterns of a tuple stay unbound** (O-TP-02). Typing them reuses Fix #8's full payload typing, which would
   bring back the ICS-1 width refusals one level down.
6. **D6 - `[REVISED: TPA-2]` a component var given to a fn value is checked by CS2-6 as its element leaf** (TP-1 step 5), rather
   than logged as a residual. It is one map plus one row (M5227), and it binds nothing. Each refusal it adds has a refused `let`
   twin, because CS2-6 already compares the twin's `i64` the same way (`u4t`, `z1t` E0102). So it stays inside the NH-1 class and
   needs no separate human question. The residual alternative would leave `u4`, a known check-clean panic, open.
7. **D7 - `[REVISED: TPA-1]` `Uncertain<..>`/`Temporal<..>` wrappers are stripped before the "is a number" test.** `unify` binds
   `c` to the wrapper (Var first, `:3517`), and the `let` twin is accepted through the soft wrapper rule (`:3601-3604`; `w2t` check
   0). Without the strip, `w2_unc` and `w7_unc_let`, which run today, would be refused (M5228).
8. **D8 - `[REVISED: TPA-1]` var and type-param leaves bind exactly**, as the twin's `.i` read does. A generic tuple result used at
   another width stays legal, because the twins are (w1t, y1t, y2t, y3t check 0). Misuse through such a leaf (`y5`) stays the
   generic-instantiation residual (§0).

## 8. Needs human

1. **NH-1 - TP-1 refuses a class of programs that run today.** `[REVISED: TPA-3]` TP-1 refuses every program whose matching `let (a,
   b)` twin is refused, except where only the numeric width differs (and for `Uncertain`/`Temporal` wrappers around a number). This
   is a class, not a list. Observed running examples:
   - `d2` (int component to `println`), `d4` (`str` component `== 1`), `q4` (`let s: str = a`, int), `q5` (int to `sh(x: str)`), `q7`
     (int into a `str` field), `q11` (bool to an `i64` param), `q18` (`let x: i64 = a`, str), `q19` (`let x: str = a`, float);
   - `v4` (bool component to `println`) and `v6` (`Option<str>` component `== "x"`) (audit);
   - `z1` (int component through a lambda to `sh(x: str)`, via D6) (mine).
   Each prints today because the interpreter passes the value through. Each one's twin is refused today (q4t, q11t, q18t, d2t, d4t,
   v4t, v6t, z1t). Six of them (q4, q5, q7, q11, q18, q19) are pinned as legal by CS4-3's controls. Options:
   - (a) TP-1 as designed (skeleton binding + obligations + D6). The class becomes E0102, and the six controls move to
     `tuple_pattern_component_misuse_that_runs_is_refused`.
   - (b) Exact non-numeric binding only, with no obligations and no D6. That refuses `q11`, `q18`, `d4`, `v4`, `v6` and keeps `q4`,
     `q5`, `q7`, `q19`, `d2`, `z1` legal. It leaves `c8`, `r2`, `g1_cross`, `u3`, `u4` as check-clean panics, and drops M5221,
     M5226, M5227.
   - (c) TP-1 not built. TP-2..TP-4 only shape-check nested and payload tuples (every component is still bound with `None`), and
     O-LF-01's component misuse stays open.
   **Recommend (a).** For kinds, the rule becomes the one `let` already follows, so a program refused after `let (a, b) = t` is also
   refused after `match t { (a, b) => .. }`, while the width leniency ICS-1 protected is kept. CS4-3 pinned the q* cases because the
   withdrawn binding left them legal; I found no recorded decision that they should be legal. (b) leaves five known panics in place.
2. **NH-2 - Is TP-4 (P3, optional) in this batch?** It adds one refusal of a running program (`e3`, a dead arm, D3). Options: (a) yes:
   ~15 lines, one test, one row (M5225); (b) no, and the struct-field case joins O-TP-03. **Recommend (a)**: same mechanism, and
   `e1` is a check-clean panic. (Classified like CS4's NH-4: an optional item whose only cost is a running-program refusal.)

`[DECIDED by human 2026-11-10: NH-1 (a) - TP-1 as designed (skeleton binding + obligations + D6); the six CS4-3 controls move to
`tuple_pattern_component_misuse_that_runs_is_refused`. NH-2 (a) - TP-4 is in this batch (M5225).]`

For the human's information (canon under D3, not open): this batch also refuses `n3`, `s2`, `h4` (and `e3` with TP-4), all dead arms
in untracked probes. No tracked file is expected to change (§3).

## 9. Verification log (reference binary `/var/tmp/r51-tmp/eg-bin/axon`, 557e746c; `/var/tmp/r51-tmp/LFT/`)

Command per probe: `LFT/run.sh <files>` = `axon check F` then `axon run F` (stdin from the shell, default env). Logs:
`LFT/all.log` (first pass), `LFT/all2.log` (`p/*.ax`, 85 files, 1.30 s, re-run after writing this spec: every row below matches),
`LFT/q.log`, `LFT/h.log`.

| probe(s) | check | run |
|---|---|---|
| `c1_comptype`, `c2_iflet`, `c3_param`, `c4_ret`, `c5_index`, `a3_field_subject`, `a5_generic_ret`, `a6_for_elem`, `a8_index_misuse`, `e1_variant_tuple`, `e2_variant_field`, `n1_nested_ty`, `s1_some_ty`, `s3_ok_ty`, `u1_lambda`, `u2_generic`, `u4_lambda_typed`, `g2_cross_nested`, `r3_nested_some_in_tuple` | 0 | 101 `cannot apply Add to str / i64` |
| `g1_cross` | 0 | 101 `cannot apply Add to i64 / str` |
| `c7_strtoint` | 0 | 101 `cannot apply Mul to str / i64` |
| `c8_lenint` | 0 | 101 `len: expected str/array/dict, got i64` |
| `r2_str_len_int` | 0 | 101 `expected str, got i64` |
| `n2_nested_arity` | 0 | 101 `no match arm matched` |
| `d6m_int_plus_float`, `f1_f64_arith` | 0 | 101 `cannot apply Add to i64 / f64` |
| `n3_nested_nontuple`, `s2_some_arity`, `e3_variant_arity`, `h4_ok_arity`, `h5_generic_ret_arity` | 0 | 0 (`o`, `other`, `other`, `o`, `o`) |
| `a2_lambda_typed_arity` | 0 | 0, `0` |
| `d1_tostr_str`, `d2_println_int`, `d3_interp_int`, `d4_eq_str_int`, `d5_float_mix`, `d7_i32_ret` | 0 | 0 (`s`, `1`, `1s`, `n`, `5`, `1`) |
| `q4`, `q5`, `q7`, `q11`, `q18`, `q19` (the CS4 `ics_q*` sources) | 0 | 0 (`1`, `1`, `1`, `true`, `s`, `1.5`) |
| `n4_nested_ok`, `s4_some_ok`, `a9_ok_tuple_lit`, `b1_typed_ok_u8`, `b3_lit_direct_u8`, `k2_u8_cast`, `k4_f64_cast`, `k5_u8_wrap_param`, `r1_struct_comp`, `u3_l1` | 0 | 0 (`s3`, `s1`, `4`, `3`, `1`, `1`, `1.5`, `300`, `6`, `2\|3`) |
| `ICS/p7/w4`..`w9`, `ICS/p9/v3` (`ICS/p9/v1` from the earlier pass) | 0 | 0 (`1`, `44`, `1`, `3`, `s6`, `7`, `1`; `v1` `3`) |
| twins: `ICS/p7/w1_plain`, `w2_letdestr`, `q4t`, `q18t`, `d4t_eq_twin`, `f2_f64_twin`, `f3_f64_lit`, `k3_u8_annot_tuple`, `d6_int_plus_float_twin` | 2, E0102 (:3, :4, :3, :3, :3, :3, :2, :2, :3) | 2 |
| twins: `b2_let_u8_twin`, `b4_var_u8`, `q11t`, `d2t_println_int_twin` | 2, E0306 (:5, :4, :4, :3) | 2 |
| `d7t_i32_ret_twin` | 2, E0307 :2 | 2 |
| `l4_let_comp` | 2, E0301 :3 | 2 |
| `k1_u8_lit300` | 2, E1900 :2 | 2 |
| `d3t_interp_int_twin`, `d1t_tostr_str_twin` | 0 | 0 (`1`, `s`) |
| shape check today: `a1_index_arity` (`(i, s, t)` on `(i64, str)`), `a6b_for_elem_arity`, `h1_call_arity`, `h2_field_arity`, `h3_for_arity` | 2, E0102 "type mismatch in tuple pattern (expected (?0, ?1)), found (i64, i64, i64)" (:3, :2, :3, :4, :2) | 2 |
| O-LF-02: `l1_let_short`, `l3_for_short`, `l5_let_lambda`, `l6_for_nested_short`, `l8_let_wild_short` | 0 | 0 (`3`, `3`, `3`, `3`, `1`) |
| O-LF-02: `l2_let_long`; `l7_let_nontuple` | 2, E0401 :2 "tuple index 2 out of bounds (length 2)"; 2, E0401 :2 "i64 has no field '0'" | 2 |
| `a7_struct_pattern` (`P { x }` on a plain struct), `c6_alias` (`type Pair = (i64, str)`) | 2, E0000 (struct patterns of a plain struct and tuple type aliases do not parse; not used) | 2 |
| baseline sweep x2 (`Soundness4/sweep.sh`, absolute out) | 717 files, 626 rc 0, 91 rc 2, byte-identical, 0.94 s; `examples/tuples.ax` and the hardset copy rc 0 | - |
| corpus scan (tracked `.ax`, tuple arms / `if let (` / `while let (` / `Some((` patterns) | 2 arm sites (`examples/tuples.ax:38`, hardset copy) | - |
| g01 ids (`grep -oE '\bM[0-9]{4}\b' scripts/v022_g01_mutations.py \| sort -u`) | 2686 distinct, highest M5219 | - |
| `[REVISED: Step 1]` reviewer probes `review-TP/tpa/*.ax` (22), re-run by me | `u1`, `u2`, `u3`, `u4`, `v1`, `v2`, `v3`, `v5`: check 0, run 101 (`expected str, got []`; `cannot apply Add to Option / i64`; `indexing non-array (i64)`; `expected str, got i64`; `to_str: expected a scalar .. got Option` / `got []` / `got P` / `got []`). `v4`: 0, `true`. `v6`: 0, `n`. `e_ok`: 0, `s1`. Twins: `u1t` E0306 :3, `u2t` E0102 :3, `u3t` E0102 :4, `u4t` E0102 :4 ("arg 0 of a call through a fn value"), `v1t` E0306 :3, `v4t` E0306 :3, `v5t` E0306 :3, `v6t` E0301 :3. `x1` E0301 :3, `x2` E0306 :4, `x3` E0301 :4 (let twins of c1/c7 report checker codes). `z1_c5_arity` E0102 :2 | - |
| `[REVISED: Step 1]` my probes `LFT/r2/*.ax` (16) | `w1_swap_u8` 0, `2`; `w1t` 0, `2`; `w2_unc` 0, `3`; `w2t` 0, `1`; `w3_arr_u8` 0, `1`; `w4_int_as_bool` 0, run 101 `if condition must be bool, got i64`; `w5_opt_u8` 0, `k`; `w6_swap_misuse` 0, run 101 `cannot apply Add to str / i64`; `w7_unc_let` 0, `k`; `w8_decimal` 0, `k`; `y1`/`y1t` 0, `1`/`1`; `y2`/`y2t` 0, `3`/`1`; `y3`/`y3t` 0, `1`/`1`; `y5_two_misuse` 0, run 101; `z1_closure_runs` 0, `1`; `z1t` E0102 :6; `z2_closure_num` 0, `4` (`y4` used a non-existent builtin, E0001, discarded) | - |
| `[REVISED: TPA-7]` g01 rows (`grep -noE "'M52[0-9]{2}'"`; `sed -n 15946p;15954p;16034p;16041p`) | last row M5211 at `:16041`; block comments at `:15946`, `:16034`; first EG-4 row at `:15954`; no `M52[2-3][0-9]` elsewhere | - |
| `[REVISED: Step 1]` DevloopTP citations re-read | `gate.sh:1131`, `:1273`, `:714`; `build-loop-checker.md:43` (83m35s), `:192`; `asi-fixes-spec.md:546`; `harness-hygiene-spec.md:415`; `build-loop-r42.md:13`; `doc_snippets.rs:35`; `language-tour.md:188`; `scorecard.py:89`; `/usr/lib/llvm-23` exists; no `target/` in the worktree | - |

Not observed: anything marked `[INFERENCE]`. No cargo build or test was run, so every "after" column and every line number of a new
refusal is the design's prediction. In particular I did not observe: the exact first diagnostic code for each new refusal (D1
predicts E0102; the let twins `x1`-`x3` show the checker reports such misuse with E0301/E0306 when it sees the type, and it does not
see pattern components today); whether `s3`/`h4`'s `Result` subject is structural at bind time; whether the native test stays green;
and the control trace (read, not built). The first baseline sweep wrote `base.tsv`/`base2.tsv` into the worktree, because the script
`cd`s into it with a relative out path. I moved both files to `/var/tmp/r51-tmp/LFT/` (the worktree is clean again), and §3 now
requires an absolute path.
