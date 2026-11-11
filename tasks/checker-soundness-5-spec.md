# checker-soundness-5 - spec (CS5-1..CS5-5)

**Status: DRAFT (after Step 1 review: AuditCS5 6, DevloopCS5 7 findings, all accepted). Not authorized for build.**

Authorized for specification by the human 2026-11-11 ("do it", item #2: fix "checks clean, then panics at run" gaps). The Step 1
review (below) is folded in; the build still needs the human's authorization and answers to NH-1..NH-3. Nothing here is implemented.

### Step 1 review

Reviews: `agent://AuditCS5` (CS5A-1..6, conformance) and `agent://DevloopCS5` (CS5D-1..7, dev loop and cost). Before accepting each
finding I checked it at f14bc25f: I re-ran the auditor's probes `a15`, `a15t`, `a25`, `a26` with the reference binary (copied into
`CheckerSoundness5/r/` and `t/`, results in §9), re-read `infer.rs:3089-3096`, `:3113`, `:3192-3195`, `:3527-3543`, confirmed M5225's
`old` text (`v022_g01_mutations.py:16086-16089`) occurs once at `infer.rs:3113`, listed every active g01 row anchored in the edited
regions (§4), read `g01_anchors.py:2`, `gate.sh:718`, `v022_g01_mutations.py:49`, `gate.sh:1125-1131`, and diffed the reviewer's
copied-binary sweep against `base.tsv` (the same 6 rows change hash, rc unchanged). **Accepted 13, rejected 0.**

| finding | sev | disposition | where |
|---|---|---|---|
| CS5A-1: CS5-2 as drafted constrains an element that is still a `Var`; `a15` (`\|p\| match (p, 1) { (Some(a), _) => 1, (Ok(b), _) => 2, _ => 3 }`, both arms run) is rc 0 `3` today and would be refused | major | accepted: the `(_, Some(elem))` arm is guarded by `!type_is_unresolved(elem)`; `a15` is a control | CS5-2 fix, controls, D1 |
| CS5A-2: the M5247 killer `u07` never uses `a`/`b`, so the arity-guard mutant survives | major | accepted: killer is `r/a26_m5247_killer` (`(a, b)` on `id((1, "s", 3))`, `str_len(a)`; rc 0 `o`); `u07` stays a control | §4 |
| CS5A-3: `u11` is fixed by CS5-1 (`declared_field` resolves by the pattern's name, `infer.rs:3192-3195`), not a generic-field residual | minor | accepted: `u11` moves to the CS5-1 reproduction; D3 names only `f06` | §0, CS5-1, D3 |
| CS5A-4: one helper, two nouns; CS5-2 would name a component index | minor | accepted: the helper takes the noun; CS5-2 passes the payload's bound name; CS5-3 says "lambda param" | CS5-1 step 3, CS5-2, CS5-3, D2 |
| CS5A-5: CS5-4's place in `solve` and its push were ambiguous | minor | accepted: after the CS2-6 call-obligation pass (`:3527-3595`), before the TP-1 loop (`:3617`); non-numeric leaves get their own `kinds_conflict` loop | CS5-4 fix |
| CS5A-6: CS5-5 would add E0405 to `a25` (`D::Nope`, already E0001:5) and change its hash | minor | accepted: also require `V` to be a variant of `X`; `a25` is a control (E0001 only) | CS5-5 |
| CS5D-1: CS5-1 removes M5225's anchor (`infer.rs:3113`); `g01_anchors.py` runs in every strict gate (`gate.sh:718`) | major | accepted: re-anchor M5225 in the CS5-1 commit; run `g01_anchors.py` per item; lane-end `--only` covers the rows anchored in edited code | §4, §6 |
| CS5D-2: diagnostic hashes include the binary's path (E0901 names `<bin>/../lib/axon/...`); 6 tracked rows change hash with a byte-identical copy | major | accepted: the baseline is the lane's own build of f14bc25f swept from the fixed lane path; rc is compared separately | §3 |
| CS5D-3: `v022_g01_mutations.py` refuses an uncommitted tree (`:49`) | minor | accepted: g01 `--only` runs after the item's commit | §6 |
| CS5D-4: the probe re-run defaults to the reference binary (`run.sh:3`) and shares one stderr file | minor | accepted: explicit `AXON=` command, one run at a time, diff against `final.txt` | §3 |
| CS5D-5: acceptance depends on untracked `/var/tmp` tooling with no compare command | minor | accepted: acceptance is `diff` rc 0 with recorded commands; a missing `/var/tmp/r51-tmp` stops the build | §3 |
| CS5D-6: cost rows were `[INFERENCE]`; strict already runs the native test | minor | accepted: TP lane figures quoted (the reviewer's file-timestamp estimates are marked as such) | §5 |
| CS5D-7: gate stages unnamed; the sweep does not cover inline `cli_run` sources | minor | accepted | §3, §5 |

Base: `main` f14bc25f (worktree `axon-lanes/specs-next2`, branch `specs/next2`). Reference binary: `/var/tmp/r51-tmp/sn2-bin/axon`
(`axon 0.1.0 (f14bc25f)`, interpreter build). Probes: `/var/tmp/r51-tmp/CheckerSoundness5/{p,q,r,t,w}/*.ax`, run by
`CheckerSoundness5/run.sh` (`axon check F`, then `axon run F`; it prints check rc, first error code:line, run rc, stdout, the panic
line). The full table of 201 probes is `CheckerSoundness5/final.txt` (2.24 s wall; 197 at draft, `final-v1.txt`, plus the review's
`r/a15`, `r/a25`, `r/a26`, `t/a15t_toplevel_twin`). Every check/run result below was observed with
that binary at f14bc25f unless it is marked `[INFERENCE]`.

The theme: programs that `axon check` accepts (rc 0) and that then panic at run (rc 101). Every gap here is one fact: a pattern binds a
name without the type the checker already knows, so the misuse of that name is invisible until the interpreter hits it.

## 0. Source entries

| item | entry (tasks/opportunities.md) | sev |
|---|---|---|
| CS5-1 | O-TP-03 (`:3499-3501`): `S::C { r } => to_str(r + 1)` with `r: str` checks clean and panics. `Pattern::Struct` binds every field pattern except a tuple one with `None` | medium |
| CS5-2 | O-TP-02 (`:3495-3497`): `(Some(a), b)` on `(Some("x"), 2)`, then misuse of `a`, checks clean and panics. The entry asks for a kind-only payload rule | low |
| CS5-3 | O-TP-01 (`:3490-3493`): a typed lambda param is untyped in inference (`LFT/p/u4_lambda_typed.ax` panics, `a2_lambda_typed_arity.ax` runs a dead arm) | low |
| CS5-4 (P3, optional) | O-LF-01 remainder (`:3468-3475`, the PARTLY FIXED stamp): "A tuple pattern on a still-unresolved subject (untyped lambda param, generic) stays unchecked" | medium |
| CS5-5 (P3) | O-AF-04 (`:3444-3447`), specced as checker-soundness-4 CS4-5 (`tasks/checker-soundness-4-spec.md:516-554`). NH-4 there was DECIDED (a) by the human 2026-11-11 (`:711-712`): CS4-5 is built in this batch | low |

Survey: `opportunities.md` has 13 entries whose text says it checks clean / rc 0 (a scan of every `###` entry body). 10 carry a
closure stamp (FIXED/BUILT/...). The open ones are O-TP-01, O-TP-02 and O-TP-03, all three items here. Open entries that mention
`panic` and no check-clean claim (O006, O-NP-03, O-NP-04) are native/gate entries, not this class.

### Not reproduced / not in this batch

| candidate | probe (observed) | disposition |
|---|---|---|
| O-AF-01, loop-carried moves | not probed: no new evidence sought | **Out of scope by human decision** (2026-11-02, asi-fixes §6.11; reconfirmed 2026-11-11 via Main). Not specced, no question |
| Generic enum field (`B::H { v }` with `v: T`) | `p/f06_generic_enum` check 0, run 101 | out (D3): the field's declared type is `Deferred("T")` (unresolved), so there is no concrete type to bind. Generic instantiation work. `[REVISED: CS5A-3]` `p/u11_generic_struct_field` was listed here; it is a generic-returning subject with a non-generic field and moves to CS5-1 |
| A tuple pattern inside a generic fn's body on a `T` param | `p/u02_generic_t` check 0, run 101 | out: the param is a `TypeParam` in the body and never resolves (instantiation work, as tuple-patterns §0) |
| `let (a, b) = e` / `for (a, b) in e` / `t.1` on a late-resolved `e` | `p/u05_generic_ret_twin`, `w/w15_for_tuple_generic`, `p/u13_field_read_generic`, `w/w01_generic_struct_read`: check 0, run 101 | out: these desugar to field reads (`parser.rs:2460` `__tup_`), not patterns; `Expr::FieldAccess` on an unresolved receiver returns a fresh var (`infer.rs:2380-2381`). Logged for a later batch |
| Mixed int/float numeric leaves (`a + b`, `a` int, `b` f64) | `p/g15_some_float_int_mix`, `w/w02_tp1_float_hole`, `w/w06_field_f64_hole`: check 0, run 101 `cannot apply Add to i64 / f64` | out: the numeric residual of tuple-patterns D2; a width-correct rule needs R19 Slice B |
| A component wrapped before it reaches a fn value (`g(Some(a))`, `g` an untyped lambda) | `t/x01_wrapped_component_lambda` check 0, run 101 | out: tuple-patterns TPA-2 residual (CS2-6 maps only an argument that IS a pattern var) |
| A qualified pattern of another enum in `while let`, or nested in `Some`/a tuple | `p/v10_whilelet_foreign`, `p/v03_some_foreign`, `p/v07_tuple_foreign`: check 0, run 0, dead arm | not covered by CS4-5's decided design; NH-2 |

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | CS5-1 | The required item. A field of a struct or variant pattern is the most common destructuring form after `let`; 226 field bindings in 20 tracked files |
| P1 | CS5-2 | Same mechanism one level down (a `Some`/`Ok`/`Err`/variant sub-pattern of a tuple pattern); the same helper closes it |
| P2 | CS5-3 | A typed lambda param ignores its annotation. The fix is the same skeleton binding on the param |
| P3 (optional, NH-3) | CS5-4 | A solve-time check; more design risk than the rest |
| P3 (decided) | CS5-5 | CS4-5 carried forward; decided by the human 2026-11-11 |

## 2. Items

### CS5-1 (P1) A struct or variant pattern's field is bound against its declared type

**Anchors (f14bc25f).**
- `infer.rs:3107-3115` `bind_pattern_with_subj`, `Pattern::Struct`: `declared` is `self.declared_tuple_field(name, fname)` only for a
  `Pattern::Tuple` field pattern (TP-4); every other field pattern (`Ident`, `Some`/`Ok`/`Err`, a nested `Struct`) is bound with `None`.
  The comment at `:3104-3106` names O-TP-03.
- `infer.rs:3191-3197` `declared_tuple_field`: resolves `Enum::Variant` through `enum_variant_fields` and a struct through
  `struct_fields`, then keeps the type only when it `matches!(ty, Type::Tuple(_)) && !has_unresolved(ty)`.
- `infer.rs:3174-3186` `tuple_component_skeleton` (TP-1): each numeric leaf becomes a fresh var plus a `tuple_component_obligations`
  entry; every other leaf is kept exactly.
- `infer.rs:3617-3631` (`solve`): the TP-1 obligation loop refuses a numeric leaf's var that resolved to a non-number (not
  `Never`, not `DynTrait`) with E0102 "use of tuple pattern component `{name}`".
- `interp/eval.rs:2962-2985`: the interpreter matches a struct pattern field by field (`match_pattern` on each field value), so the
  bound value has the declared field's type at run time.
- Shorthand `S::C { r }` and rename `S::C { r: x }` both reach `:3108` as `(fname, pat)` (`p/f01_tp03`, `p/f03_rename` behave the same).

**Reproduction.** (`run.sh <probe>`; every row check rc 0, run rc 101.)

| probe | shape | run panic |
|---|---|---|
| `p/f01_tp03` (the O-TP-03 reproducer, = `/var/tmp/r51-tmp/tp03.ax`) | `S::C { r } => println(to_str(r + 1))`, `r: str` | `cannot apply Add to str / i64` |
| `p/f03_rename` | `S::C { r: x } => .. x + 1` | same |
| `p/f07_struct_field_field` | field of struct type, `p.x + 1`, `x: str` | same |
| `p/f08_opt_field_add` | `o: Option<str>`, `o + 1` | `cannot apply Add to Option / i64` |
| `p/f12c_arr_field_misuse` | `xs: [str]`, `str_len(xs)` | `expected str, got []` |
| `p/f13_iflet`, `p/f14_whilelet` | the same field in `if let` / `while let` | `cannot apply Add to str / i64` |
| `p/f16_bool_add` | `b: bool`, `b + 1` | `cannot apply Add to bool / i64` |
| `p/f19_str_to_intparam` | `r: str` passed to `fn h(x: i64)` | `cannot apply Mul to str / i64` |
| `p/f23_param_subject` | subject is a typed fn param `v: S` | `cannot apply Add to str / i64` |
| `p/f24_lambda_closure` | `n: i64` passed to `\|x\| str_len(x)` | `expected str, got i64` |
| `p/f27_nested_variant` | `S::C { w: W::P { r } }` | `cannot apply Add to str / i64` |
| `p/f31_fn_field` | `f: fn(i64) -> i64`, `f("s")` | `cannot apply Add to str / i64` |
| `p/f32_tostr_arr` | `xs: [i64]`, `to_str(xs)` | `to_str: expected a scalar ... got []` |
| `p/f33_sum_type` | `type S = A { r: str } \| B { n: i64 }` | `cannot apply Add to str / i64` |
| `p/f37_field_ok_payload` | `S::C { o: Ok(x) }`, `o: Result<str, i64>`, `x + 1` | `cannot apply Add to str / i64` |
| `q/s08_nested_struct_in_some` | `Some(W::P { r })` on `Option<W>` | `cannot apply Add to str / i64` |
| `p/u11_generic_struct_field` `[REVISED: CS5A-3]` | `match id(S::C { r: "a" }) { S::C { r } => .. r + 1 }`, `S` not generic, subject a late-resolved `id(..)` return; `declared_field` resolves by the pattern's name, so the subject's type does not matter | `cannot apply Add to str / i64` |

The let/field-read twins are refused today, so the checker already has the rule; only the pattern binding loses the type:
`p/f02t_struct_read` (E0306:4), `t/f08t_opt_twin` (E0301:4), `t/f16t_bool_twin` (E0301:4), `p/f18t_struct_read` (E0306:5),
`p/f25t_struct_read` (E0102:4), `t/f32t_tostr_arr_twin` (E0306:4), `t/u13t_concrete_twin` (E0301:3), `t/f24t_lambda_twin` (E0102:5).

**Fix design.**
1. Rename `declared_tuple_field` to `declared_field` and drop the `matches!(ty, Type::Tuple(_))` half of its filter: it returns the
   declared field type whenever that type has no unresolved part (`!has_unresolved`). A generic field (`v: T`) still yields `None`
   (D3).
2. In the `Pattern::Struct` arm, for each `(fname, pat)` with `Some(decl)` from `declared_field(name, fname)`, call a new helper
   `bind_sub_pattern(pat, &decl, name, noun, scope)` (shared with CS5-2; `[REVISED: CS5A-4]` it takes the obligation's noun). This
   replaces the TP-4 call at `:3113`, which is M5225's g01 anchor: `[REVISED: CS5D-1]` M5225 is re-anchored in the same commit (§4).
   For a field pattern the name is `fname` and the noun is "pattern field":
   - `Pattern::Ident(n)` binds `tuple_component_skeleton(decl, n)` (TP-1's rule: numeric leaves free, checked by kind in `solve`;
     every other leaf exact);
   - `Pattern::Tuple(_)` keeps TP-4's call, `bind_pattern_with_subj(pat, scope, Some(decl))`;
   - `Pattern::Some`/`Ok`/`Err`/`Struct` call `bind_pattern_with_subj(pat, scope, Some(&skel))` with
     `skel = tuple_component_skeleton(decl, fname)`. The `Some`/`Ok`/`Err` arms (`:3056-3101`) already constrain the subject to
     `Option(fresh)`/`Result(fresh, fresh)` and bind the payload through it, so a payload `Ident` gets the skeleton's leaf (a free
     var for a number: `p/f30_opt_int_some_u8` `let y: u8 = x` on `Option<i64>` stays legal);
   - `Wildcard`/`Literal`/`None` bind nothing, as today (literal sub-patterns stay unchecked: O-CS4-04, control `q/s12`).
   With no declared type (`None`), the field pattern is bound with `None`, as today.
3. The obligation entry gains its origin noun (D2): `tuple_component_obligations` becomes `(leaf, var, name, noun, span)`
   (`[REVISED: CS5A-4]`). TP's entries keep "tuple pattern component", so their text "use of tuple pattern component `{name}`" is
   unchanged; CS5-1 entries say "use of pattern field `{name}`".

**After CS5-1** (prediction, `[INFERENCE]` until built): every reproduction row is refused by `axon check` (rc 2) at the use line,
and `axon run` stops at the same refusal (rc 2, empty stdout). An exactly-bound leaf (`str`, `bool`, `[T]`, `Option<..>`, a struct,
a fn) refuses with the twin's own diagnostic (E0301/E0306/E0102, listed above). A numeric leaf used as another kind refuses with
E0102 "use of pattern field `n`".

**Behaviour change (programs that run today and would be refused).** All are untracked probes; each concrete twin is refused today.

| probe | run today | refusal | class |
|---|---|---|---|
| `p/f02_int_println` (`n: i64`, `println(n)`) | rc 0, `5` | E0102 (num var used as `str`) | NH-1 |
| `p/f18_int_to_strparam` (`n: i64` to `fn sh(x: str)`) | rc 0, `1` | E0102 | NH-1 |
| `p/f25_int_eq_str` (`r: str`, `r == 1`) | rc 0, `n` | E0102 (twin `f25t` E0102:4) | NH-1 |
| `q/s02_join_str_int` (arms join `v: str` and `w: i64`) | rc 0, `a` | E0102 (twin `s02t` E0102:1) | NH-1 |
| `p/f36_field_some_dead` (`S::C { r: Some(x) }` on `r: str`) | rc 0, `o` (dead arm) | E0102 "type mismatch in Some pattern" | D4 (dead arm) |

**Controls (must stay rc 0 with today's stdout).** `p/f04b_u8_via_fn` (`8`), `p/f05_int_field_as_f64` (`7`), `p/f12_arr_field`
(`2`), `p/f15_ok_str` (`a3`), `p/f17_bool_ok` (`t`), `p/f26_field_wild_lit` (`one`), `p/f28_dyn_field` (`k`, int field as
`Decimal`), `p/f29_unc_field` (`3`, int field to `Uncertain<i64>`), `p/f30_opt_int_some_u8` (`4`), `p/f35_two_arms_same_name`
(`3`), `p/f38_iflet_variant_int_u8` (`1`), `p/f39_generic_fn_enum_field` (`2`), `p/f40_interp_field` (`n=5`),
`p/f41_tostr_field_int` (`5`), `p/f42_struct_payload_nested` (`6`), `q/s07_arm_join_num` (`4`: arms join `i64` and `u8`, legal only
because both leaves are free), `q/s12_lit_field_wrong` (`o`), `q/s13_dict_field` (`0`), `r/f45_f64_field_int_lit` (`6`),
`r/f46_field_shadow` (`a!|6`), `r/f47_enum_field_nested_ok` (`b`), `r/f49_generic_struct_field` (`a!`), `w/w17_field_some_u8_control`
(`4`), `w/w18_tostr_bool_field` (`true`).

**Corpus.** A text emulation over the 717 tracked `.ax` files (`CheckerSoundness5/cs51_emul.txt`) found 226 field bindings in
20 files, all rc 0 today: 189 numeric declared type, 4 `str`, 1 `bool`, 17 other (enum/struct/array), 15 with no declaration found.
The `str`/`bool` ones are `sexpr.ax:165,172,179` (`Node::Atom { text }`) and `planner.ax:31` (`Plan::Step { name, safe }`), each used
as its own type. A heuristic scan for a field name used against its kind found 0 rows. Expected tracked changes: **none**
`[INFERENCE: text emulation, not a checker run]`.

**Fail-first test.** `cli_run.rs`, appended after `tuple_pattern_in_a_struct_field_is_checked` (`:48647-48664`, the end of the
tuple-patterns block that starts at `:48497`): `struct_pattern_fields_are_typed`. Cases: every reproduction row above (17 sources,
inlined). Each must give check rc 2 with its first error at the use line, and `run` rc 2 with empty stdout. The existing helper
`tp_assert_refused` (`:48502`) pins E0102; add `cs5_assert_refused(attack, tag, src, code, line)` beside it on top of
`sf6_check_and_run` (`:42297`) and `cs4_first_error` (`:47059`), and pin each case's code to the value observed after the fix (the
twins above predict it). Marker `ATTACK: a struct or variant pattern field of the wrong type checked clean`. Fails on base (every case
is rc 0 today). Controls go to the new `pattern_binding_controls_stay_legal` (CS5-1..CS5-4, modelled on
`tuple_pattern_controls_stay_legal` `:47133`): each must check rc 0 and run rc 0 with the stdout listed. The NH-1 and D4 rows go to
`pattern_binding_misuse_that_runs_is_refused` (modelled on `tuple_pattern_component_misuse_that_runs_is_refused` `:48577`), only
if NH-1 is (a).

**g01 rows**: M5240, M5241.

**Acceptance.** The three tests pass. Sweep: no tracked file changes rc or diagnostic hash (§3). Probe re-run: every reproduction row
is rc 2/rc 2, every control row unchanged. `axon-for-llms.md:87` gains "a struct or variant pattern's fields take the declared field
types (numbers at any width)" in the same comment style (doc_snippets keeps passing).

### CS5-2 (P1) A non-tuple sub-pattern of a tuple pattern is bound against its element

**Anchors.** `infer.rs:3131-3166`, `Pattern::Tuple`: once the subject is concrete and the arity matches (`elems`), an `Ident`
component binds `tuple_component_skeleton(elem, name)` (`:3155-3158`) and a nested `Tuple` binds `Some(elem)` (`:3159-3161`, TP-2).
Every other sub-pattern falls to `_ => self.bind_pattern_with_subj(pat, scope, None)` (`:3162`). The comment at `:3128-3130` names
O-TP-02.

**Reproduction.** Every row check rc 0, run rc 101 `cannot apply Add to str / i64` (`g02`, `g06`, `g13`, `g14`, `g18` the same).

| probe | shape |
|---|---|
| `p/g01_r3` (= `LFT/p/r3_nested_some_in_tuple.ax`, the O-TP-02 reproducer) | `(Some(a), b)` on `(Some("x"), 2)`, `a + 1` |
| `p/g02_ok_in_tuple` | `(Ok(a), b)` on `(Result<str, i64>, i64)`, `a + b` |
| `p/g06_variant_in_tuple` | `(W::P { r }, b)`, `r: str`, `r + b` |
| `p/g13_nested_some_some` | `(Some(Some(a)), b)`, `a + b` |
| `p/g14_some_tuple_in_tuple` | `(Some((a, s)), b)`, `s + b` |
| `p/g18_err_in_tuple` | `(Err(e), b)` on `mk() -> (Result<i64, str>, i64)`, `e + b` |

Twin refused today: `t/g01t_twin` (E0102:4). `p/g08_some_int_misuse` (`println(a)` on a `Some(1)` component) is already E0306:2;
`p/g08b_some_int_misuse` (`str_len(a)` behind a `to_str`) checks 0 and panics `expected str, got i64` and is fixed by the same rule.

**Fix design.** `[REVISED: CS5A-1, CS5A-4]` Replace the `_` fallback at `:3162` with
`(_, Some(elem)) if !type_is_unresolved(elem) => self.bind_sub_pattern(pat, elem, name, "pattern binding", scope)` (CS5-1's helper),
keeping every other case on `bind_pattern_with_subj(pat, scope, None)`. The guard matters: `concrete` (`:3132-3142`) accepts a tuple
subject whose element is still a `Var`, and the `Some`/`Ok`/`Err` arms constrain whatever subject type they are given
(`:3089-3096`), so an unguarded element var would be pinned to `Option` by one arm and to `Result` by the next, refusing `r/a15`,
which runs (control below). `name` is the payload's bound name when the sub-pattern binds exactly one `Ident` (`Some(a)` -> `a`),
else the component index (`#0`), so the message reads "use of pattern binding `a`". This is the kind-only payload rule O-TP-02 asks
for: a payload's numeric leaves are free vars, so the ICS-1 width cases one level down stay legal (`p/g03_some_u8_control`
`let x: u8 = a` on `Some(1)`, rc 0 `1`).

**Behaviour change.** `p/g09_some_int_println` (`println(a)` on `Some(1)`; rc 0, `1`; twin `g09t` E0102:3) becomes E0102: NH-1.
`p/g04_some_on_int_dead` (`(Some(a), b)` on `(5, 2)`) and `p/g05_some_on_str_dead` (on `("s", 2)`) run a dead arm (rc 0, `o`) and
become E0102 "type mismatch in Some pattern" (twin `t/g04t_some_on_int_twin` E0102:3): D4.

**Controls.** `p/g03_some_u8_control` (`1`), `p/g07_some_ok_control` (`x2`), `p/g19_some_dyn` (`k`), `r/a15_var_elem_multi` (`3`;
`[REVISED: CS5A-1]` an element var matched as `Some` in one arm and `Ok` in another, both called; its top-level twin
`t/a15t_toplevel_twin`, which matches the var itself, is already E0102:2 "type mismatch in Ok pattern" and stays so), and every case
of `tuple_pattern_controls_stay_legal`.

**Corpus.** A scan of every tracked `.ax` for a tuple-pattern arm holding `Some(`/`Ok(`/`Err(`/`X::Y` found 0 sites. Expected
tracked changes: none.

**Fail-first test.** `tuple_sub_patterns_are_typed` (after `struct_pattern_fields_are_typed`): the six reproduction rows plus
`g08b`, `cs5_assert_refused`, marker `ATTACK: a sub-pattern of a tuple pattern of the wrong type checked clean`. Controls into
`pattern_binding_controls_stay_legal`; `g09`, `g04`, `g05` into `pattern_binding_misuse_that_runs_is_refused` (NH-1 (a)).

**g01 rows**: M5242, M5243.

**Acceptance.** Tests pass; the probes re-run as predicted; sweep shows no tracked change.

### CS5-3 (P2) A typed lambda param is bound to its annotation

**Anchors.** `infer.rs:2433-2473`, `Expr::Lambda`: `param_types` is `params.iter().map(|_| self.fresh())` (`:2438`), so the
annotation (`ast.rs:421-423` `LambdaParam { name, ty: Option<AxonType> }`) is never read in inference; `:2445-2447` binds each param
to its fresh var. CS3-2's `own` (`:2453-2459`) collects the params that are `Type::Var`, and CS2-6 later checks call arguments by kind
against them.

**Reproduction.**

| probe | check | run |
|---|---|---|
| `LFT/p/u4_lambda_typed.ax` (`\|p: (i64, str)\| match p { (a, b) => to_str(b + 1) }`) | 0 | 101 `cannot apply Add to str / i64` |
| `p/l17_typed_lambda_opt` (`\|o: Option<(i64, str)>\| match o { Some((a, b)) => b + 1, None => 0 }`) | 0 | 101, same |
| `LFT/p/a2_lambda_typed_arity.ax` (`(a, b, c)` on a `(i64, str)` param) | 0 | 0, `0` (dead arm) |

Twins: `t/u4t_fn_twin` (the same body as a `fn` with a typed param) E0102:1; `t/a2t_fn_twin` E0102:1 "type mismatch in tuple
pattern"; `t/l17t_fn_twin` E0102:1. Direct misuse of a typed scalar param is already caught through CS2-6 at the call
(`p/l01_typed_lambda_direct` E0102:3, `p/l05` E0102:3), which is why only pattern subjects show the gap.

**Fix design.** At `:2438`, a param with `ty: Some(t)` gets the skeleton of `self.resolve_ast_type(t)` with name `p.name` and
`[REVISED: CS5A-4]` noun "lambda param" (TP-1's `tuple_component_skeleton`, extended with the noun as in CS5-1 step 3); a param
without an annotation keeps `self.fresh()`. A numeric annotation therefore stays a free var (it stays in `own`, CS2-6/CS3-2 unchanged),
which keeps `p/l02_typed_lambda_u8_lit` (`2`), `p/l12_typed_lambda_arr_map_u8` (`3`), `p/l13_typed_lambda_u8_as_i64` (`2`, a `u8` param
used as `i64`; the fn twin `t/l13t_fn_twin` is E0102:1) legal. A non-numeric annotation binds exactly. The `match p` subject is then
concrete, so TP-1..TP-3 apply to it.

**Behaviour change.** `p/l15_typed_lambda_println_int` (`println(a)` on a `(i64, str)` param; rc 0, `1`) becomes E0102: NH-1.
`LFT/p/a2_lambda_typed_arity.ax` becomes E0102 (CS4-3 arity): D4.

**Controls.** `p/l02`, `p/l07_typed_lambda_arr_map` (`4`), `p/l12`, `p/l13`, `p/l14_typed_lambda_tuple_u8` (`s1`), `p/l16_typed_lambda_param_to_fn`
(`3`), `p/l18_typed_lambda_dyn` (`k`), `p/l19_typed_lambda_generic_call` (`6`), `q/l25_unify_probe` (`1`), `q/l28_typed_lambda_ok`
(`s1`), `r/l32_typed_tostr_ok` (`3!`).

**Corpus.** 33 tracked files hold typed lambda params (all rc 0): 231 `i64`, 11 `bool`, 8 `f64`, 7 `str`, a few tuples. The ones
followed by `match` are `vm_depth/nest/lambda_chain.ax` (130 `|x: i64|` lambdas) and `examples/{anthropic_stream,http_sse,
trainloop_stream}.ax` (`|event: str|`, matched against str literals). Expected tracked changes: none `[INFERENCE]`.

**Fail-first test.** `typed_lambda_params_are_typed`: `u4`, `l17` (refused), `a2` (D4; in the misuse test under NH-1 (a)), marker
`ATTACK: a typed lambda param of the wrong type checked clean`. Controls into `pattern_binding_controls_stay_legal`.

**g01 rows**: M5244, M5245.

**Acceptance.** Tests pass; probes as predicted; sweep shows no tracked change; O-TP-01 closes.

### CS5-4 (P3, optional) A tuple pattern on a subject resolved only by `solve` is checked by kind

**Anchors.** `infer.rs:3131-3141`: the subject is left unchecked when it is a `Var`/`Deferred`/`Unknown`/`TypeParam`/`DynTrait` at
bind time, and its components are then bound with `None`. The comment at `:3116-3123` gives the reason: constraining it in one arm
would refuse a later arm of another arity that runs (TP control `l1`, `|p| match p { (a, b) => 2, (a, b, c) => 3 }` called with both
arities). `solve` already resolves late types before its obligation loops (`:3501-3507` unify, then `:3535-3541` and `:3617`).

**Reproduction.** Each: check 0, run 101 `cannot apply Add to str / i64`.

| probe | subject |
|---|---|
| `p/u03_generic_ret` | `match id((1, "s")) { (a, b) => .. b + 1 }`, `fn id<T>(x: T) -> T` |
| `p/u04_generic_ret_let` | the same through a `let` |
| `q/b02_arr_find_tuple` | `match arr_find(xs, \|p\| true) { Some((a, b)) => .. b + 1 }` on `[(i64, str)]` |
| `q/b05_arr_find_field_read` | the same via `t.1` (field read: stays out, see §0) |
| `p/u01_lambda`, `w/w09_untyped_lambda_variant`, `w/w16_untyped_lambda_let_destr` | an untyped lambda param |

**Fix design.** In the `Pattern::Tuple` arm, when `concrete` is `None` and `subj_ty` is `Some(Var)`, record a deferred entry
`(subject, arity, [(i, name, var)])` with a fresh var bound per `Ident` component. In `solve`, `[REVISED: CS5A-5]` after the CS2-6
call-obligation pass (`:3527-3595`, which binds a component-return var to the call's use) and before the TP-1 loop (`:3617`), apply
`subst` to each deferred subject; if it is a `Tuple` of the same arity, take each component's element skeleton leaf by leaf:
- a numeric leaf is pushed to `tuple_component_obligations` (`(leaf, var, name, "pattern binding", span)`), so the TP-1 loop checks it
  (used must be a number);
- every other leaf goes to a new loop right after: `kinds_conflict(leaf, subst.apply(var))` (`:353`) gives E0102 "use of pattern
  binding `{name}`".

An arity mismatch, a subject still unresolved, or a non-tuple is skipped: nothing is refused that the bind-time rule would not refuse
(D5). Nothing is bound or unified by this step, so the l1 control is untouched. Because the pass runs after CS2-6 built its `comp` map
(`:3535-3542`), a deferred component passed to a fn value is not mapped to its element (the same residual as `x01`, §0).

**Behaviour change.** `p/u12_generic_ret_println` (`println(a)` on `id((1, "s"))`, rc 0 `1`) becomes E0102: NH-1. No dead-arm
refusals (`p/u07_generic_ret_arity` stays rc 0, `o`).

**Not verified.** Whether an untyped lambda param's var resolves by the end of `solve` (CS2-6 says "nothing is bound" for kind
derivation, `:3526`): `u01`/`w09`/`w16` may stay unchecked `[INFERENCE]`. The test pins only `u03`, `u04`, `b02`; the lambda rows
are recorded as observed after the build.

**Controls.** `p/u06_generic_ret_ok` (`s1`), `p/u07_generic_ret_arity` (`o`), `p/u10_generic_ret_u8` (`1`), `r/u15_arr_find_ok`
(`s1`), `r/u16_arr_find_u8` (`1`), `r/a26_m5247_killer` (`o`; `[REVISED: CS5A-2]` `(a, b)` on `id((1, "s", 3))` with `str_len(a)`,
the M5247 killer: with the arity guard removed `a` maps to `i64` and is refused), TP controls `l1`, `g2`, `g3`.

**Corpus.** Not emulated (needs the solved types). The sweep at build time decides; any tracked rc change stops the item.

**Fail-first test.** `tuple_pattern_on_a_late_resolved_subject_is_checked`: `u03`, `u04`, `b02`, marker `ATTACK: a tuple pattern on
a late-resolved subject checked clean`.

**g01 rows**: M5246, M5247.

**Acceptance.** Test passes; controls stay; sweep no tracked change.

### CS5-5 (P3, decided) A qualified pattern of a different enum is refused (carries CS4-5)

Decided by the human 2026-11-11: NH-4 of `tasks/checker-soundness-4-spec.md` is (a), CS4-5 is built in this batch (Main's message;
recorded at `checker-soundness-4-spec.md:711-712`). The design below is CS4-5's (`:516-554`), re-anchored at f14bc25f.

**Anchors (re-read at f14bc25f).**
- `parser.rs:3925-3931`: `C::Red` parses to `Pattern::Struct { name: "C::Red", fields: [] }` (`full_name`), `:3954-3962` both forms.
- `checker.rs:4745-4751`: `Expr::Match` calls `check_match_exhaustiveness(&subj_ty, arms, node_path)`, the only call site. `if let`
  desugars to a `match` (`parser.rs:2634-2640`, LF-2), so it is covered; `while let` (`Expr::WhileLet`) is not.
- `checker.rs:6717` `check_match_exhaustiveness`; `:6780-6806` the E0405 literal-pattern check; `:6813-6819` the early `return` when an
  unguarded `_` or ident arm exists; `:6825-6829` `enum_name` from the subject (`Type::Enum(n)`, or a sum type reaching it as
  `Type::Struct(n)`).
- `error.rs:51` and `:475` (was `:473` in the CS4 spec): E0405 "literal pattern's type can't match the match subject";
  `AXON_REFERENCE.md:186` carries the same line. `axon card` does not list E04xx codes (`axon card | grep -c E04` = 0), so the card
  digest is unchanged.

**Reproduction.** `Soundness4/lf/v2q_noall.ax` (`match c { D::Up => 2, _ => 1 }` on a `C`): check 0, run 0, `1`. `CS4A/en.ax`
(`C` and `D` both have `Up`): check 0, run 0, `1|1` (and `AXON_ENGINE=vm`: rc 0, `1|1`). `Soundness4/lf/v2q_ok.ax`: rc 0, `6`
(control). Mine: `p/v01_v2q` (`1`), `p/v02_iflet_foreign` (`no`), `p/v04_sumtype_foreign` (`no`), `p/v05_fields_foreign` (`no`),
`p/v06_generic_enum_foreign` (`no`), `p/v11_guarded_foreign` (`no`): all check 0, run 0, dead arm. `p/v09_same_enum_ok` (`g`) control;
`p/v08_unknown_enum_qual` is already E0001:4.

**Fix design (CS4-5's, plus one condition).** In `check_match_exhaustiveness`, before the wildcard `return` (`:6817`): hoist
`enum_name` (`:6825-6829`) above it; when it is `Some(e)` and a top-level arm pattern is `Pattern::Struct { name }` with
`name = "X::V"`, `X != e`, `X` a known enum and `[REVISED: CS5A-6]` `V` one of its variants (`self.enum_variants[X].contains(V)`;
an unknown variant keeps its E0001 alone, control `r/a25_foreign_unknown_variant`, check 2 E0001:5 today), emit E0405 "this match
is on `C`, but `D::Up` is a variant of `D` - it can never match",
help "use a `C::` variant". Compare the enum NAME only (a generic enum has no type args here). Broaden E0405's description at
`error.rs:51` and `:475` to "a pattern's type can't match the match subject" and regenerate `AXON_REFERENCE.md` (`axon reference`;
`axon_reference_is_in_sync_with_the_binary`, `cli_run.rs:26475`, enforces it).

**Behaviour change (decided).** Refuses `v2q_noall`, `en`, and the `v0x` dead-arm probes above (`v01`, `v02`, `v04`, `v05`, `v06`,
`v11`). Not refused (NH-2): `v10` (`while let`), `v03` (`Some(D::Up)`), `v07` (`(D::Up, n)`), `v12` (a struct pattern on an `Option`
subject: `enum_name` is `None`).

**Corpus.** 1191 `match` blocks in the tracked corpus scanned for arms qualified by two different enums: 0. Expected tracked changes:
none.

**Fail-first test.** cli_run `foreign_enum_variant_pattern_is_refused` (CS4-5's name and cases): `v2q_noall` E0405 (its `_` arm pins
the placement), `en` E0405, `v2q_ok` rc 0; add `v04` (sum type) and `v11` (guarded) as E0405 cases, and `a25` as a case whose
diagnostics hold E0001 and no E0405. Marker `ATTACK: a pattern of another enum checked clean`.

**g01 row**: M4719 (reserved by checker-soundness-4 §4, outside this block; `grep` finds no `M4719` in `scripts/` at f14bc25f).

**Acceptance.** Test passes; sweep no tracked change; `axon reference` diff is the E0405 line only; O-AF-04 closes.

## 3. Shared mechanics

- **Corpus sweep** (fail-first gate step, before and after every item): `/var/tmp/r51-tmp/Soundness4/sweep.sh <repo> <bin> <ABSOLUTE
  out.tsv>` runs `axon check` on every `git ls-files '*.ax'` from the file's directory and records path, rc and sha256 of the
  diagnostics. The script `cd`s into the repo, so the out path must be absolute. Reference at f14bc25f with `sn2-bin/axon`:
  **717 files, 626 rc 0, 91 rc 2**; two runs byte-identical (`CheckerSoundness5/base.tsv`, `base3.tsv`, sha256 prefix
  `c34a7a5fcc613d1f`); 1.4 s real (`-P 12`).
  `[REVISED: CS5D-2]` A diagnostic can name the binary's own path (E0901 prints `<bin>/../lib/axon/...`): a byte-identical copy of
  the reference binary at another path changes the hash of 6 tracked rows (`examples/asi/{bandit_ucb,persistent_bandit,safe_bandit,
  safe_self_improve}.ax`, `examples/sealed/suite/accept.ax`, `tasks/r51-s6/q1/suite/accept.ax`) with every rc equal
  (`review-CS5/CS5D/sweep-copied.tsv` vs `base.tsv`, re-diffed by me). So `base.tsv` is for reference only. The lane's baseline is
  its own build of f14bc25f, copied to one fixed path (`$LANE/bin/axon`) and swept as `$LANE/sweep-base.tsv`; every later build is
  copied to the same path. Rule: `diff $LANE/sweep-base.tsv $LANE/sweep-<item>.tsv` must exit 0 (no rc and no hash change), unless
  this spec names the file (it names none); `diff <(cut -f1,2 A) <(cut -f1,2 B)` reports rc changes on their own. Any difference
  stops the item and goes to the integrator. The sweep covers tracked `.ax` files only (CS5D-7); the inline sources of the ~1014
  `cli_run` tests are covered by the full `cli_run` run at lane end and by strict's S16.
- **Probe re-run** `[REVISED: CS5D-4]`: `cd /var/tmp/r51-tmp/CheckerSoundness5 && AXON=$LANE/bin/axon ./run.sh p/*.ax q/*.ax r/*.ax
  t/*.ax w/*.ax > $LANE/probes-<item>.txt` (201 probes, ~2.5 s), formatted as `final.txt` was (the `sed` in §9), then
  `diff final.txt $LANE/probes-<item>.txt`. The only differences allowed are the reproduction and behaviour-change rows the item
  names. Without `AXON=` the script silently uses the reference binary (`run.sh:3`), and it writes one fixed `.runerr`, so only one
  re-run at a time.
- **Acceptance tooling** `[REVISED: CS5D-5]`: "sweep shows no tracked change" in each item means the sweep `diff` above exits 0, and
  "probes as predicted" means the probe `diff` holds only the named rows; both commands go in the lane notes. If `/var/tmp/r51-tmp`
  (sweep.sh, run.sh, the probes) is gone, the build stops; it does not skip the check.
- **Tests:** all new tests live in `crates/axon-core/tests/cli_run.rs` after `:48664` (the end of the tuple-patterns block), run as
  `cargo test --locked -p axon-core --no-default-features --test cli_run <name>`. No `.ax` fixture is added; sources are inlined.
- **Native:** no codegen change. `integration_fixtures.rs:2855` (`tuple_patterns_test_every_component_natively`, codegen feature) is
  run by `gate.sh --strict` (the TP strict log shows it at `tp-gate.log:13657`), so the lane runs it only if it wants an early
  signal (CS5D-6).
- **Gate stages** `[REVISED: CS5D-7]`: an `axon-core/src` change runs S16 (`gate.sh:1125-1131`, `cargo test --locked -p axon-core
  --no-default-features`, which holds every new test), S17 (codegen rebuild), S22 (`vm_parity.sh`), the `G01 mutation anchors` stage
  (`gate.sh:718`), and strict's codegen clippy, `integration_fixtures`, and parity stages. Under `--strict` every stage runs. One
  `gate.sh --strict` covers all five items.

## 4. g01 rows (block M5240-M5259)

The block is free at f14bc25f: `grep -rn 'M524[0-9]\|M525[0-9]'` over `scripts/`, `crates/`, `tasks/` finds nothing (only
`tuple-patterns-spec.md:409`, which says a new block starts at M5240). Rows go after the tuple-patterns block
(`v022_g01_mutations.py:16050-16053` comment, rows from `:16054`) in the same 8-tuple form (`id, why, file, old, new, crate, args,
test`); markers go in `v022_attack_markers.py` after the tuple-patterns `ATTACK_MARKERS.update` (near `:4452`). The builder writes the
exact `old` text after the fix lands (it must occur once in the file). Rows M5248 and M5249 were added by the Step 1 review.

| id | item | guard removed by the mutant | file | killer | marker |
|---|---|---|---|---|---|
| M5240 | CS5-1 | `bind_sub_pattern`'s `Ident` skeleton binding for a struct field (mutant: bind with `None`) | infer.rs | `struct_pattern_fields_are_typed` (`f01`) | `ATTACK: a struct or variant pattern field of the wrong type checked clean` |
| M5241 | CS5-1 | `declared_field` returns non-tuple types (mutant: restore the `Type::Tuple` filter) | infer.rs | same (`f16`, `f32`) | same |
| M5242 | CS5-2 | the `(_, Some(elem))` arm in `Pattern::Tuple` (mutant: `None`) | infer.rs | `tuple_sub_patterns_are_typed` (`g01_r3`) | `ATTACK: a sub-pattern of a tuple pattern of the wrong type checked clean` |
| M5243 | CS5-2 | the payload skeleton (mutant: pass `elem` exactly instead of its skeleton) | infer.rs | `pattern_binding_controls_stay_legal` (`g03`, `f30`, `s07`) | `ATTACK: a valid pattern match was refused \[cs5_g03\]` |
| M5248 | CS5-2 | `[REVISED: CS5A-1]` the `!type_is_unresolved(elem)` guard (mutant: `true`) | infer.rs | `pattern_binding_controls_stay_legal` (`a15`) | `ATTACK: a valid pattern match was refused \[cs5_a15\]` |
| M5244 | CS5-3 | the annotated-param binding at `Expr::Lambda` (mutant: `self.fresh()`) | infer.rs | `typed_lambda_params_are_typed` (`u4`) | `ATTACK: a typed lambda param of the wrong type checked clean` |
| M5245 | CS5-3 | the skeleton on an annotated param (mutant: bind the resolved annotation exactly) | infer.rs | `pattern_binding_controls_stay_legal` (`l13`) | `ATTACK: a valid pattern match was refused \[cs5_l13\]` |
| M5246 | CS5-4 | the deferred-subject push in `solve` (mutant: `if false`) | infer.rs | `tuple_pattern_on_a_late_resolved_subject_is_checked` (`u03`) | `ATTACK: a tuple pattern on a late-resolved subject checked clean` |
| M5247 | CS5-4 | the arity guard on a deferred subject (mutant: `true`) | infer.rs | `pattern_binding_controls_stay_legal` (`a26`; `[REVISED: CS5A-2]` was `u07`, which binds nothing it uses, so the mutant survived) | `ATTACK: a valid pattern match was refused \[cs5_a26\]` |
| M4719 | CS5-5 | the foreign-enum E0405 emit (mutant: `if false`) | checker.rs | `foreign_enum_variant_pattern_is_refused` (`v2q_noall`) | `ATTACK: a pattern of another enum checked clean` |
| M5249 | CS5-5 | `[REVISED: CS5A-6]` the "`V` is a variant of `X`" condition (mutant: `true`) | checker.rs | `foreign_enum_variant_pattern_is_refused` (`a25`: E0001, no E0405) | `ATTACK: a pattern of another enum checked clean` |

M5250-M5259 stay free for implementation-review fixes. A CS5-4 drop (NH-3 (b)) drops M5246/M5247.

**Existing rows the edits touch** `[REVISED: CS5D-1]`. `g01_anchors.py` ("every ACTIVE G01 mutation row still anchors exactly
once", `:2`) runs in every gate (`gate.sh:718`, `par "G01 mutation anchors"`), so a removed anchor fails strict. I loaded every
active row whose file is `infer.rs` and searched its `old` text at f14bc25f; these anchor once inside code CS5-1..CS5-4 edit:
- removed: **M5225** (`infer.rs:3113`, `self.bind_pattern_with_subj(pat, scope, declared.as_ref());`, `v022_g01_mutations.py:16086-16089`),
  replaced by CS5-1's `bind_sub_pattern` call. Re-anchor it in the CS5-1 commit onto `bind_sub_pattern`'s `Pattern::Tuple` branch
  (mutant: `Some(decl)` -> `None`), killer unchanged (`tuple_pattern_in_a_struct_field_is_checked`).
- in the `Pattern::Tuple` arm CS5-2 edits: M4713 (`:3146`), M4715 (`:3144`), M5220 (`:3156`), M5222 (`:3160`); in the `Some`/`Ok`
  payload code next to it: M5223 (`:3098`), M5224 (`:3067`); in `tuple_component_skeleton`, which gains the noun: M4714 (`:3177`).
- in `solve`, where the obligation tuple gains its noun (CS5-1) and CS5-4 adds its pass: M5227 (`:3567`), M5229 (`:3571`), M5230
  (`:3587`), M5228 (`:3618`), M5221 (`:3619`), M5226 (`:3620`), M5231 (`:3622`), M3919 (`:3574`), M3904 (`:3602`).

Each item runs `python3 -B scripts/g01_anchors.py` after its commit (rc 0 required) and repoints any row whose text it changed, as
the TP lane did (`cafa3cb8`, "g01 repoint M5221/M5226"). The lane-end g01 `--only` runs the new rows plus every row listed here.

## 5. Gates and cost budget (no paid spend: no AI path is touched)

| step | when | cost (this host) | basis |
|---|---|---|---|
| Lane baseline: build f14bc25f to `$LANE/bin/axon`, sweep + probes | lane start | one interpreter build + ~5 s | sweep 1.4 s, probes 2.24 s measured |
| `[REVISED: CS5D-6]` targeted `cargo test --locked -p axon-core --no-default-features --test cli_run <name>`, fail-first and after the fix | per item | ~5-10 s warm; ~12-16 s after touching `infer.rs`; minutes if the target dir is cold | `tuple-patterns-spec.md:440`; the reviewer's estimate from TP g01 file timestamps `[INFERENCE]` |
| sweep + probe diff + `g01_anchors.py` | per item, after its commit | ~5 s | sweep, probes measured; `g01_anchors.py` not timed |
| g01 `--only` (11 new rows + the 17 touched rows above) | lane end, committed tree | ~12-16 s per row, about 6-8 min | TP lane: 15 rows in <= 174 s (`tp-sweep-tpi.tsv` 01:36:22 -> `tp-g01-tpi.json` 01:39:16, file times I read) `[INFERENCE: per-row]` |
| doc_snippets, spec_ledger, fmt, clippy | lane end | minutes; `spec_ledger` 1.11 s | spec_ledger measured |
| Full `--no-default-features` `cli_run` | lane end, once | 276.6-488 s | `tuple-patterns-spec.md:441` |
| Native `integration_fixtures` test | optional (strict runs it) | 3-13 s warm; ~10 min if LLVM is cold | `tuple-patterns-spec.md:443` |
| strict stages: S16 axon-core tests / S17 codegen rebuild / S22 vm parity | inside strict | 633.6 s / 2.3 s / 86.4 s | `tp-gate.log:8241,8243,8251` (read) |
| `scripts/gate.sh --strict` | integrator, once | **~50 min** idle (44-55), ~84 min under 3-lane load | Main's 2026-11-10/11 figure; `tuple-patterns-spec.md:446` |
| Budget | | 1 strict run + 1 retry | a second failure goes back to the lane |

## 6. Sequencing and lanes

One lane (all items touch `infer.rs` except CS5-5, which touches `checker.rs`, `error.rs`, `AXON_REFERENCE.md`). Order: CS5-5 (independent,
decided), CS5-1 (adds `declared_field`, `bind_sub_pattern`, the obligation noun), CS5-2 (reuses the helper), CS5-3, then CS5-4 if
NH-3 is (a). One commit per item. `[REVISED: CS5D-3]` Per item: (1) write its tests and run them on base (every refusal case fails,
every control passes); (2) the fix (CS5-1 also re-anchors M5225); (3) targeted tests, sweep diff, probe diff; (4) commit; (5)
`g01_anchors.py` on the committed tree (`v022_g01_mutations.py:49` refuses an uncommitted tree, and so the per-item g01 runs move to
lane end). Lane end: one g01 `--only` over every row in §4, doc_snippets, spec_ledger,
fmt, clippy, the full `cli_run` once (the native test is optional: strict runs it); then hand over to the integrator for `gate.sh --strict`. CHANGELOG and the
`opportunities.md` stamps (O-TP-01/02/03, O-LF-01's remainder, O-AF-04) are written in the build, not before.

## 7. Engineering decisions (decided here; canon for the build)

Each changes only checker internals, or refuses only programs of a class already decided (D4). NH-1..NH-3 (§8) are the
language-behaviour questions.

1. **D1 - one binding rule.** Every pattern binding whose declared or element type is known binds that type's skeleton
   (`tuple_component_skeleton`): numeric leaves are free vars checked by kind in `solve`, every other leaf is exact. This is ICS-1's
   leniency (a number may be used at any width) and tuple-patterns D2, applied to struct fields, tuple sub-patterns and typed lambda
   params. It keeps every TP control legal, since TP's own bindings are unchanged. `[REVISED: CS5A-1]` A known type means one with no
   unresolved top-level part: an element or field that is still a `Var`/`Deferred`/`Unknown`/`TypeParam`/`DynTrait` at bind time
   keeps today's untyped binding, as CS4-3 leaves an unresolved subject unchecked (control `a15`).
2. **D2 - diagnostics.** An exact leaf refuses at the use with the existing diagnostic (the let twin's). A numeric leaf's obligation
   refuses with E0102; `[REVISED: CS5A-4]` the obligation entry carries its origin noun, so CS5-1 reads "use of pattern field
   `{name}`", CS5-2 and CS5-4 "use of pattern binding `{name}`" (`name` the bound payload name, else the component index), CS5-3
   "use of lambda param `{name}`", and TP's text "use of tuple pattern component `{name}`" is unchanged (no TP test text changes).
3. **D3 - generic declared types stay unbound.** A field whose declared type has an unresolved part (`v: T`) binds `None`, as TP-4
   did. `f06` stays a panic (§0). `[REVISED: CS5A-3]` `u11` is not this case (its field `r: str` is concrete) and is fixed by CS5-1.
4. **D4 - refuse, do not warn, dead arms.** A sub-pattern that cannot match its declared type (`f36`, `g04`, `g05`, `a2`) is refused,
   the class decided by checker-soundness-4 D9 and tuple-patterns D3.
5. **D5 - CS5-4 checks kinds only.** No binding, no unification, no arity refusal at solve time; an unresolved subject is skipped.
6. **D6 - CS5-5 is CS4-5 as decided**, with E0405's description broadened; the card is unaffected.
7. **D7 - docs.** `spec/axon-for-llms.md:87` gains the struct-field sentence (CS5-1); CS5-3 needs no doc line (an annotation now means
   what a reader expects). `AXON_REFERENCE.md` changes only by CS5-5's E0405 line.

## 8. Needs human

1. **NH-1 - refuse running programs whose concrete twin is refused?** CS5-1..CS5-4 refuse programs that run today and print a
   right-looking answer, because a number bound from a pattern is used where a `str` is expected (the interpreter's `println`/`==`
   accept it): `f02` (`println(n)`, `5`), `f18` (`1`), `f25` (`n`), `s02` (`a`), `g09` (`1`), `l15` (`1`), `u12` (`1`). In each case
   the same program written with a `let`/field read is refused today (`f02t` E0306, `f25t` E0102, `s02t` E0102, `g09t` E0102, ...).
   None is tracked. Options: (a) refuse them, as tuple-patterns NH-1 (a) decided 2026-11-10 for tuple components; (b) refuse only the
   uses that panic, which needs an exemption for `println`/`==`/joins of a numeric binding (a second rule, and the let twins stay
   refused, so the language disagrees with itself); (c) warn instead. **Recommend (a)**: one rule for a name whatever binds it, the
   same class the human accepted for TP-1, and a model learns one behaviour.
2. **NH-2 - extend CS5-5's E0405 to `while let` and to nested sub-patterns?** `v10` (`while let D::Up = c` on a `C`: the loop never
   runs), `v03` (`Some(D::Up)`), `v07` (`(D::Up, n)`) are the same "can never match" fact and run a dead arm today. Options: (a) yes:
   call the check from `Expr::WhileLet` and walk `Some`/`Ok`/`Err`/tuple sub-patterns (~20 lines, two more refusals classes, still 0
   tracked hits); (b) no: build CS4-5 exactly as decided and log the residual as a new opportunity. **Recommend (a)**: the decided
   reason ("a silent dead arm") holds for each, and the corpus has no multi-enum match.
3. **NH-3 - is CS5-4 (P3) in this batch?** It closes the most common late-resolved shape (`match arr_find(..) { Some((a, b)) => ..}`)
   but adds a solve-time pass whose reach over untyped lambda params is unverified. Options: (a) yes, with D5's kind-only rule and the
   sweep as the stop rule; (b) no, log it and keep O-LF-01's remainder open. **Recommend (a)**: the reproductions use stdlib calls a
   model writes often, the rule refuses nothing the concrete twin does not, and a tracked rc change stops it.

## 9. Verification log (reference binary `/var/tmp/r51-tmp/sn2-bin/axon`, f14bc25f)

| what | result |
|---|---|
| `axon --version` | `axon 0.1.0 (f14bc25f)` |
| `run.sh` over 197 probes (`final-v1.txt`), then 201 with the review's four (`final.txt`, 2.24 s; rows unchanged except order, `diff` checked) | every check/run figure quoted in §2 |
| `final.txt` format | `./run.sh p/*.ax q/*.ax r/*.ax t/*.ax w/*.ax 2>&1 \| sed -E 's/err=\[\{"schema"[^]]*"code":"([A-Z0-9]+)".*"message":"([^"]{0,50}).*$/err=[\1 \2]/' \| sed -E 's/^([^ ]+)\.ax/\1/' \| cut -c1-150` |
| Step 1 review probes: `r/a15_var_elem_multi`, `t/a15t_toplevel_twin`, `r/a26_m5247_killer`, `r/a25_foreign_unknown_variant` | 0/0 `3`; 2 E0102:2 "type mismatch in Ok pattern"; 0/0 `o`; 2 E0001:5 |
| review checks: `infer.rs:3113` holds M5225's `old` text once; active `infer.rs` g01 rows anchored in the edited regions (script over `v022_g01_mutations.py`) | 17 rows, listed in §4 |
| `diff` of `review-CS5/CS5D/sweep-copied.tsv` (copied binary) against `base.tsv`, hash column | 6 rows differ (§3), rc column identical |
| `tp-gate.log` stage lines | S16 633.6 s, S17 2.3 s, S22 86.4 s; native TP test `ok` at `:13657` |
| `/var/tmp/r51-tmp/tp03.ax` | check rc 0; run `axon: panic: cannot apply Add to str / i64` |
| `run.sh Soundness4/lf/v2q_noall.ax Soundness4/lf/v2q_ok.ax CS4A/en.ax`; `AXON_ENGINE=vm axon run CS4A/en.ax` | 0/0 `1`; 0/0 `6`; 0/0 `1\|1`; rc 0 `1\|1` |
| sweep x2 (`base.tsv`, `base3.tsv`) | 717 files, 626 rc 0, 91 rc 2, identical |
| corpus scans (field bindings, tuple sub-patterns, typed lambdas, multi-enum matches) | 226 / 0 / 33 files / 0 |
| `python3 -B scripts/spec_ledger.py` | PASS after writing this file |

Not observed: anything marked `[INFERENCE]`. No cargo build or test was run, so every "after" column, every predicted diagnostic code
and line, and the CS5-4 reach are the design's prediction. The corpus figures for CS5-1..CS5-3 are text scans, not checker runs; the
build's sweep is the measurement.
