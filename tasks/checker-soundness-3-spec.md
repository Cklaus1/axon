# checker-soundness-3 - DRAFT spec

**Status: BUILT 2026-10-31 on int/31 63dcb967 (CS3-1..CS3-7, lanes A/B/C merged; integrator duties §6 done: parity cases, CHANGELOG, opportunities stamps and O-CS3-01..06). The §8 language additions (NH-1..NH-12) are NOT authorized and NOT built: they wait for a human decision. NH-13: declined (w14 stays legal). Was: AUTHORIZED for build 2026-10-31 (after Step 1 review; human: "do it all" on the integrator's recommendations) for CS3-1..CS3-7 only.**
`[REVISED: integration 2026-10-31] c18 is not in native_shapes_parity.sh: its native build fails IR verification on Some(Some(x)), a pre-existing native bug (O-CS3-04).`

### Step 1 review

Two reviews of the draft: `agent://AuditCS3` (findings 1-8, one blocker) and `agent://DevloopCS3` (CS3D-1..11). Before deciding I re-ran
the cheap probes in `/var/tmp/r51-tmp/CS3Audit/` on main77 (a1, a2, e9, t9, b1, b2, b3, w05b: results as the audit states) or read the
cited line (np-int `infer.rs:3072`/`:2143`, `native_shapes_parity.sh:291,301`, `native-parity-spec.md:3,561`, `doc_snippets.rs:35-40`,
`AXON_REFERENCE.md:164`, `vm/tests.rs:1964`, `resolver.rs:4122-4138`, `parser.rs:1835,1855`, `lib.rs:1035-1042`). Integrator rulings
(2026-10-31) are applied to Audit #2, #3, #4, #5 and CS3D-1/2. 19 accepted (4 with amended disposition), 0 rejected.

| id | disposition (one line) |
|---|---|
| Audit-1 (blocker) | accepted: a1 (`if s == "bob"{`) and a2 run today (check 0, `yes\|1`, `1\|a\|b`); `LBrace` dropped from CS3-6's token list, a1/a2 added as controls |
| Audit-2 | accepted (ruling): default-matrix rule for incomplete signatures; the cap applies only where CS2-4 applies it, and a walk over the cap elsewhere accepts; e9 and t9 (both check 0, `1`) added as controls |
| Audit-3 | accepted, disposition amended (ruling): b2/b3 are non-exhaustive (typed twin b1 is E0304 today) and panic on uncovered inputs; named as CS3-1 consequences, b2 an expected rc 2 case; engineering, not needs-human |
| Audit-4 | accepted (ruling): the default build records no obligation nested in `if`/`match`/`&&`/`\|\|` inside the wrapper body, so w14 stays rc 0; refusing it is NH-13 |
| Audit-5 | accepted (ruling): native-parity is BUILT on np/int; CS3-1's commit removes `np2_h31` and `np2_u02` from `scripts/native_shapes_parity.sh` |
| Audit-6 | accepted: nested `use` lines (`lib.rs:1035-1042` collects module `use` paths) get the importing module's file label; resolver test literals updated |
| Audit-7 | accepted, disposition amended: M3988 killer is y05 (`let` is a keyword, y06 does not kill); M3979 is retargeted to the nesting rule (killer w14/w05b) because with that rule a `Conflicted` mutant only changes programs that panic on every call |
| Audit-8 | accepted: superseded in wording by CS3D-1 (same facts) |
| CS3D-1 | accepted (ruling): lanes branch from main after native-parity merges; loop at :3072, `? inner type` at :2143 post-NP; NP lane-table cite :561 |
| CS3D-2 | accepted: cli_run cited by fn name; all line anchors re-read at the post-NP base |
| CS3D-3 | accepted: every program is inlined into its test; no test reads `/var/tmp` |
| CS3D-4 | accepted: the in-crate VM test is modelled on `r51_t1b_run` with `vm_eager = true`, both engines, `f`'s body selected |
| CS3D-5 | accepted: `diagnostics_for_refused_forms` split per lane; row 6 is written after CS3-1 in the new walk's fix-it code |
| CS3D-6 | accepted: `doc_snippets` does not scan `spec/compiler-phase3.md` (`doc_snippets.rs:35-40`) |
| CS3D-7 | accepted: `AXON_REFERENCE.md` lists code descriptions only (E0304 :164); no regen expected |
| CS3D-8 | accepted (reviewer's measurement, 14 s, not re-run) |
| CS3D-9 | accepted: the pci theme is a follow-up `pci: theme for <sha8>` commit |
| CS3D-10 | accepted: `native_shapes_parity.sh` and `fence_sweep.py` are integrator-owned |
| CS3D-11 | accepted, disposition amended: `MergeError.span: Option<Span>`; `UseDecl`/`ModDecl` take `span: Span` with `serde(default)` (Span derives Default, per audit) |

Drafted 2026-10-31 at `main` 77cf51fc. Every anchor below was read at 77cf51fc. Every "observed" result comes from
`/var/tmp/r51-tmp/main77-bin/axon` (`axon 0.1.0 (77cf51fc)`, a codegen build): `axon check` rc, then `axon run` rc and output. "Old" means
`/var/tmp/r51-tmp/main-bin/axon` (`0bd2b518`, before CS2). The probe sources and raw outputs are under `/var/tmp/r51-tmp/CheckerSoundness3/`
(`D/` below). Result files: `D/pay-new.txt` (payload matches), `D/gap-final.txt` (O-GAP-08 forms), `D/w-results.txt` (wrapper lambdas).
Probes are run by `D/run2.sh <files>`, which prints check rc, run rc, stdout joined with `|`, and the first error as `code line:col message || help`.
Anything not observed is marked `[INFERENCE]`.

## 0. Source entries

| item | source (`tasks/opportunities.md`) | one line |
|---|---|---|
| CS3-1 | O-CS2-02 (:2963), second bullet; plus a CS2-4 regression found while drafting | Literal patterns inside a payload pass check and then panic. Also, CS2-4 refuses valid tuple matches whose components have refutable payloads |
| CS3-2 | O-CS2-02 (:2963), first bullet | Calling an untyped lambda through an untyped wrapper skips the CS2-6 kind check (h24) |
| CS3-3 | O-CS2-03 (:2971) | E1209 has no line or column |
| CS3-4 | O-GAP-08 (:2906), found while classifying "inherent `impl`" | A bare `self` in an `impl` method is refused with E0308 `unknown type 'Self'`, although the parser and the docs support it |
| CS3-5 | O-GAP-08 (:2906), class (b) | Refused forms whose diagnostics point the wrong way (`loop`, `impl T {`, `if let`, `for (a, b)`, `..`, `r#"`, `()`, tuple variants, `== Some`, `?` on Option, the enum fix-it) |
| CS3-6 | O-GAP-08 (:2906), "no raw string literal" | A `"` inside a string literal ends it silently, and the rest of the line runs as separate statements: a silent wrong answer |
| CS3-7 | O-GAP-08 (:2906), `mod util` row | E0901 (module not found) has no line or column |

### Out of scope (logged, not dropped)

- **The O-GAP-08 language additions** (class (c) in §2.5). They are product decisions and sit in §8 Needs human. None is in a build tier.
- **Native wrong answer on a tuple payload split** (`D/m/c19_opt_tuple_payload_split.ax`). `match o { Some((true, n)) => n, Some((false, _)) => 0, None => 1 }`
  gives interpreter `5` and native `6` (check 0, build 0). This belongs to native-parity (O-CS2-01). I sent it to `NativeParity` on 2026-10-31,
  and `tasks/native-parity-spec.md:141-145` now carries it as an NP case.
- **Bare variant names in patterns bind instead of comparing.** In `D/bv/bv01_order.ax`, `match c { Green => 2, Red => 1, Blue => 3 }` with
  `C::Red` prints `2`: check rc 0 with W0004 only, run rc 0, native `2`. The enum branch counts `Pattern::Ident(name)` as covering the variant of
  that name (`checker.rs:6766`). A filtered `git grep` of tracked `.ax` found no such arm. CS2 already logged this as an observation
  (`tasks/checker-soundness-2-spec.md:79`). The integrator files it as **O-CS3-01**: a silent wrong answer, and a candidate to promote W0004 to an
  error when the name is a variant of the subject's enum.
- **Spaced string adjacency.** `let j = r"{" a "}"` (`D/y/y03_raw_spaced.ax`) is check 0 and prints `{`. It runs as three statements on one
  line, the same rule that makes `let x = 1 let y = 2` legal (`D/y/y04`, check 0, prints `3`). CS3-6 refuses only the zero-gap case (§2.6).
  The integrator logs this residual as **O-CS3-02**.
- **Number-kind residuals of CS2-6.** `f64` against `i64` is one "number" kind (`infer.rs:325-346` `kinds_conflict`). `D/w/w10_numeric_req`
  and `D/w/w13_num_direct` (check 0, panic `cannot apply Mul to f64 / i64`) and `D/x/x09_float_index` (panic `expected i64, got f64`) stay open.
  CS3-2 does not change the kind lattice.
- **E0001 cascade after E0901.** When a module fails to load, every name its `use` imports also gets E0001 "introduce `square` with
  `let square = ...`" (`D/g/38_modules_main.ax` with `AXON_PATH` unset). CS3-7 fixes only the location. The integrator logs the cascade as **O-CS3-03**.

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P0 | CS3-1 | False refusals on `main` (a CS2-4 regression: 5 valid programs the old binary ran correctly), plus 10 check-then-panic shapes |
| P1 | CS3-2, CS3-4, CS3-6 | Check-then-panic (CS3-2); a false refusal of a documented form (CS3-4); a silent wrong answer (CS3-6) |
| P2 | CS3-3, CS3-5 | Diagnostics without a location, or pointing the wrong way. Repair loops depend on them |
| P3 | CS3-7 | A location on a file-level error |

## 2. Items

### CS3-1 (P0) Exhaustiveness by usefulness: payload literals, and the CS2-4 tuple regression - O-CS2-02

**Problem.** There are two failures, and both come from how coverage is decided.

(a) **Holes.** An Option or Result scrutinee is exhaustive as soon as any unguarded arm is `Some(_)`-shaped (`checker.rs:6643-6698`:
`matches!(a.pattern, Pattern::Some(_))` ignores the payload pattern). An enum is exhaustive when every variant name appears (`:6754-6769`; the
`filter_map` keeps the name and drops `fields`). So `Some(0)` covers all of `Some`, and `E::A { s: "x" }` covers all of `A`.

(b) **False refusals (a regression from CS2-4, 00984298).** Inside a tuple, `flatten_cover` (`:6918-6987`) gives an Option, Result or enum
component a `Variants` domain (`:6969-6977`). A cell covers a variant only when its payload is irrefutable (`cell_covers`, `:7045-7051`). So
`(Some(true), _)` and `(Some(false), _)` each fail to cover `Some`, although together they do.

**Reproduction** (`D/pay-new.txt`; sources in `D/m/`, `D/ImplAuditCS2` copies at `D/h*.ax`):

| probe | shape | main77 | old 0bd2b518 |
|---|---|---|---|
| h31 | `match o { Some(0) => 0, None => 1 }`, called with `Some(5)` | check 0, run 101 `no match arm matched` | same |
| h32 | `E::A { s: "x" } => 0, E::B => 1`, called with `s: "y"` | check 0, run 101 | same |
| h31b/c/e/f/h/i/j | `Ok("x")`; `Some(Some(0))`; `E::A { b: true }`; `Some(true)`; `S::Paint { c: C::Red }`; two-field hole; `Some("a")` | each check 0, run 101 | - |
| h31g | lambda subject `\|o\| match o { Some(0) => 0, None => 1 }` | check 0, run 101 | - |
| h31d (control, true positive) | `(Some(0), _)`, `(None, _)` | check 2, E0304 `no arm covers (Some(_), _)` | - |
| **c05** | `match (o, n) { (Some(true), _) => 1, (Some(false), _) => 2, (None, _) => 3 }` | **check 2**, E0304 1:40 `no arm covers (Some(_), _)` | check 0, `6` |
| **c16** | the same shape over `(Result<bool, str>, i64)` | **check 2**, E0304 1:45 | check 0, `6` |
| **c17** | `(E::A { b: true }, _)`, `(E::A { b: false }, _)`, `(E::B, _)` | **check 2**, E0304 2:29 | check 0, `6` |
| **c18** | `(Some(Some(x)), _)`, `(Some(None), _)`, `(None, _)` | **check 2**, E0304 1:48 | check 0, `5` |
| **c20** | `(Some((true, n)), _)`, `(Some((false, _)), _)`, `(None, _)` | **check 2**, E0304 1:47 | check 0, `5` |
| controls c01-c04, c06-c09, c11-c15, c19, c21, q02 | `Some(0)` then `Some(_)`; both bools; nested Option; `Ok(0)` then `Ok(n)`; payload literal then binding; guard plus wildcard; generic payload; 9-variant enum; lambda in `arr_fold`; omitted enum fields | each check 0, run 0, correct output | - |

**The change.** Replace the three coverage paths (the Option/Result branch at :6643-6698, the enum name set at :6754-6789, and the CS2-4
product walk in `check_value_exhaustiveness` / `cell_covers`, :6822-7058) with one recursive usefulness walk (Maranget, "Warnings for pattern
matching", 2007). The question it answers: is the wildcard row useful against the matrix of unguarded arm patterns?
- **Constructors per type.** `bool`: `true`/`false`, arity 0. `Option`: `Some`(1) and `None`(0). `Result`: `Ok`(1) and `Err`(1). An enum or
  sum type: its variants, with arity equal to the declared field count, in declaration order. A tuple: one constructor of arity n. A known
  struct: one constructor, its fields. int/str/float/decimal: infinite, so only a default row covers them.
- **Field order and types for enum variants.** Field order comes from `enum_variant_fields` (`:890-893`). Field types come from a new map,
  registered beside it in the item pass (`:885-893`), using `axon_type_to_type` on `EnumVariant.fields` (`ast.rs:150-152`). A field that is
  omitted from a struct pattern is a wildcard (q02 is legal today: check 0, `9`).
- **Specialization.** `[REVISED: Audit-2]` If the head constructors in the column form the type's complete signature, specialize per
  constructor c: keep the rows whose head is c and expand them to their sub-patterns. A wildcard or binding head expands to c's arity of
  wildcards. Every other row is dropped. Otherwise (an incomplete signature, including an all-wildcard column) recurse on the default matrix
  only (Maranget). So e9's all-binding `E::A { a, .., i }` arm costs one recursion per column, not 2^9.
- **Infinite domains.** The column is covered only through the default matrix, made of the rows whose head is a wildcard or binding.
- **Unknown component types** (`Var`, `Deferred`, `TypeParam`, `Unknown`). Read the domain off the patterns with the existing
  `domain_from_patterns` (`:6991-7026`). When the patterns do not decide it, stop and **accept**, as CS2-4 does (`:6916-6917`).
- **Unknown subject types.** A subject whose type is a `Var` (h31g) goes through the same walk instead of `_ => {}` (`:6811`), under the same
  rule: undecided means accept.
- **Named consequences** `[REVISED: Audit-3, integrator ruling]`. Two programs that run today become E0304: `CS3Audit/b2.ax`
  (`let f = |s| match s { "a" => 1, "b" => 2 }; f("a")`, check 0, `1`) and `CS3Audit/b3.ax` (h31g called only with `Some(0)` and `None`,
  check 0, `1`). Both are non-exhaustive matches that panic on an uncovered input. Their typed twin `CS3Audit/b1.ax` (`fn f(s: str)`) is E0304
  today. Closing them is the purpose of CS3-1 (exhaustiveness was decided and built in CS2-4), so this is engineering, not a product decision.
- **Guarded arms** cover nothing (unchanged; `:6639`).
- **Witness and messages.** The witness is printed as today: `no arm covers `{witness}`; add that arm or a `_ =>` arm`. When the whole
  constructor is missing at top level, keep the exact texts `missing Some(_) arm` / `missing None arm` / `missing Ok(_) arm` / `missing Err(_) arm`
  and `missing variant '{v}'`, because existing tests pin them. The enum fix-it text changes in CS3-5 (6).
- **Budget.** `[REVISED: Audit-2, integrator ruling]` The 256-combination cap and its E0304 "more than 256 combinations" message apply only
  where CS2-4 applies them today: tuple and scalar subjects (`check_value_exhaustiveness`, :6839-6841), with the same exemption when an
  irrefutable unguarded arm is present (t9: a 9-bool tuple with one all-binding arm, check 0, `1`). A walk on any other subject (Option,
  Result, enum, `Var`) that exceeds 256 leaf checks **accepts**. That adds no new refusal.

**Fail-first tests** (`crates/axon-core/tests/cli_run.rs`, appended):
- `payload_literal_holes_are_refused`: h31, h31b, h31c, h31e, h31f, h31g, h31h, h31i, h31j, h32, b2 `[REVISED: Audit-3]`, and
  `Some(n) if n > 0 => n, None => 0` must each be rc 2 with E0304. Before the fix, every case except the guard case is rc 0 (observed above).
- `payload_matches_that_cover_stay_legal`: c01-c09, c11-c21 (c10 is a parse error and is excluded), q02, and `CS3Audit/e9.ax`, `t9.ax`,
  `a3.ax`, `a4.ax`, `a5.ax` `[REVISED: Audit-2]` must each be rc 0, with today's outputs. Before the fix, c05, c16, c17, c18 and c20 are rc 2.
- `non_exhaustive_scalar_and_tuple_matches_are_refused` (cli_run, CS2-4) must stay green unchanged. So must h31d.
- `[REVISED: CS3D-3]` Every program is inlined into its test as a string literal. No test reads `/var/tmp`.

**g01 rows:** M3970-M3977 (§4). CS2-4's rows M3911-M3915 (`scripts/v022_g01_mutations.py:12318-12337`) target lines this item rewrites, such
as `Type::Option(_) => variants(&["Some", "None"]),` at :6969. The same commit re-anchors those five rows to the new code, with the same
guards and the same killer (`non_exhaustive_scalar_and_tuple_matches_are_refused`), and `g01_anchors.py` must report each `old` exactly once.

**Acceptance.** Both new tests pass. All 5 regression probes print the old binary's output. All 3 sweeps (§5) are identical except for files
named in this item. `native_shapes_parity.sh` gains c05, c16, c17, c18, c19 and c20. `[REVISED: Audit-5]` The same commit removes
`CASES[np2_h31]` and `CASES[np2_u02]` (np-int `scripts/native_shapes_parity.sh:291`, `:301`). After CS3-1 both are check refusals, and the
harness counts a build error as FAIL (`scripts/lib/harness_skip.sh:88`, per audit). NP-2's IR test `a_match_miss_edge_panics`
(np-int `codegen/match_pat.rs:990`) stays the killer.

**False-refusal guard.** The 19 controls above, plus e9, t9, a3, a4, a5. S290, S596 and frozen-corpus sweeps must be byte-identical. A newly refused frozen benchmark
task goes into `COMPILE_CAUGHT` (`crates/axon-cortex/benchmarks/fence_sweep.py:64`) together with its receipt-ceiling consequence. Expected:
none `[INFERENCE: a scan of tracked .ax for payload literals was not done; the sweep decides]`.

**Gate stages.** cli_run (`--no-default-features`), clippy on all 4 variants, `parity_all.sh`, `integration_fixtures`, g01 rows, sweeps.
**Cost.** About 250 lines of checker.rs net (3 paths removed, 1 added) `[INFERENCE]`; 8 g01 rows at about 4 min.

### CS3-2 (P1) Kind of an untyped lambda param through a wrapper - O-CS2-02 (h24)

**Problem.** CS2-6 checks a call through a fn value in `infer.rs:3063-3107` (main77; `:3072` post-NP, `[REVISED: CS3D-1]`). It applies the
substitution, and when `kinds_conflict(param, arg)` (`:325-346`) holds it reports E0102. It never binds a param from a call (`:3058-3062`), so
that `let id = |x| x` stays polymorphic. In `let f = |x| str_len(x); let g = |y| f(y); g(5)`, the obligation `f(y)` has arg `Var(y)`, so there
is no conflict and nothing is recorded. Then `g(5)` compares `Var(y)` against `i64`, again with no conflict.

**Reproduction** (`D/w-results.txt`):
- h24 (`/var/tmp/r51-tmp/ImplAuditCS2/h24_lambda_param_kind_wrong_via_wrapper.ax`): check 0, run 101 `expected str, got i64`.
- w02 (chain f, g, k), w11 (`let t = y f(t)`), w17 (wrapper returned from a lambda): check 0, run 101 `expected str, got i64`.
- w08 (`if a + b * a` through `g`): run 101 `if condition must be bool, got i64`. w09 (bool index): run 101 `expected i64, got bool`.
- Already refused today: w01 (named fn), w06 (`arr_map(.., g)`), w07 (direct), w12 (builtin in one branch), w18 (named fn param), each E0102.
- Controls: w03 `3`, w04 `1s` (polymorphic `id`), w05 `6` (y reaches both `str_len` and `+ 1`; called on the numeric branch), w14 `0`,
  x10 `0`. All check 0, run 0.

**The change.** Add derived kinds, as a fixpoint over `call_obligations` that never unifies anything:
- When an obligation's argument resolves to an unresolved `Var` v and the callee's param in that position resolves to a concrete kind, record
  `derived[v] = param` (a side map, `HashMap<u32, Type>`).
- `[REVISED: Audit-4, integrator ruling]` Record only from obligations that are **not nested** in an `if`/`else` branch, a `match` arm, or an
  operand of `&&`/`||` inside the wrapper lambda's body. This is a depth counter in infer, incremented while inferring those sub-expressions
  and stored on `CallObligation`. A call that may not run then never derives a kind. h24, w02, w11, w08 and w09 still close (their calls are
  unconditional); w14 and w05 stay rc 0.
- When v already holds a different kind, mark it `Conflicted` and record nothing more.
- Repeat until no entry changes. This covers w02's chain.
- `kinds_conflict` is then evaluated with each unresolved `Var` replaced by its `derived` entry, when there is one.
- `Conflicted` and absent entries behave as today (accept). w05 stays rc 0.

**The named consequence** `[REVISED: Audit-4]`. None for the default build. Refusing w14 (`let g = |y, c| if c { f(y) } else { 0 }; g(5, false)`,
which runs today and prints `0`, while the direct twin w12 is E0102) would be a new refusal of a running program. It is NH-13 (§8).
`[REVISED: implementation audit 2026-10-31]` "None for the default build" is wrong: CS3-2 refuses (E0102) a wrapper called with a
wrong-kind argument even in a branch that never runs (c01, c03, c05, w16). The direct-call rule already gave E0102 on those shapes at
base, so this is ruled correct: the programs are ill-typed. It is a consumer-visible newly refused shape (CHANGELOG).

**Resolving the CS2-6 `[REVISED: build]` conflict** (`checker-soundness-2-spec.md:436`). Three cli_run VM tests reach a lambda through
`let g = |x, y| f(x, y)` precisely so that check passes and the VM panic path runs: `vm_pure_bool_ops` cases `pure_bool_if` and
`pure_bool_while`, and `vm_superop_write_panics` case `bool_index` (cited by fn and case name, `[REVISED: CS3D-2]`). After CS3-2 all three are E0102 at check (they are the w08/w09
shapes), so `axon run` never reaches the VM. Decision: the VM panic path stays covered **below the checker**.
- `[REVISED: CS3D-4]` Move the three cases into `crates/axon-core/src/interp/vm/tests.rs` as `vm_bool_panics_without_check`, modelled on
  `r51_t1b_run` (`vm/tests.rs:1964`), not `run_both` (:635, which hard-codes its source). For each source: `parse_source`, then
  `Interp::build_with(&prog, &[], Engine::Vm)`, `vm_eager = true`, `vm_trace = false`, `init_globals()`, and `call_fn(fns["main"], vec![])`.
  Assert `Err(Flow::Panic(m))` with `m` equal to today's text (without the CLI's `axon: panic: ` prefix). Repeat with `Engine::Tree` for the
  same text. For `f` (the first `lambda_codes()` entry, `interp/sym.rs:869`), assert the compiled body is `Some`, `tree_nodes() == 0`, and
  `pure_ops()` (`vm/mod.rs:311`) is (1,0) for the if case and (1,1) for the while case. `bool_index` asserts the panic text only, as cli_run
  does. Without `vm_eager`, `f` never compiles and the panic would come from the tree engine `[INFERENCE: per CS3D-4's reading, not compiled]`.
- In cli_run, drop the three cases (`vm_pure_bool_ops`' `[PureBoolCase; 3]` becomes 1; `vm_superop_write_panics` drops case `bool_index`,
  12 to 11; cited by fn name, `[REVISED: CS3D-2]`). Add them to `wrapper_lambda_param_kind_is_checked` as rc 2 E0102 cases (each wrapper is
  an unconditional `let g = |x, y| f(x, y)`).
- Rejected alternative: keep a wrapper shape the checker cannot see. That depends on a hole staying open.

**Fail-first tests:**
- `wrapper_lambda_param_kind_is_checked` (cli_run): h24, w02, w11, w08, w09 and the 3 moved programs must be rc 2 E0102. Today they are rc 0.
- `wrapper_lambda_controls_stay_legal` (cli_run): w03, w04, w05, w14, `CS3Audit/w05b.ax` (`g(5, false) + g("ab", true)`, check 0, `8`) and
  x10 must be rc 0 with the outputs above. They pass before the fix; they kill M3979.
- `vm_bool_panics_without_check` (`interp/vm/tests.rs`): passes before and after the fix. It is the coverage move, not a fail-first test.
- w17 is not acceptance. Whether its returned lambda's `Var` reaches the obligation is `[INFERENCE]`.

**g01 rows:** M3978-M3980. **Acceptance:** the tests above, the sweeps identical, and the three VM panics still asserted in-crate.
**False-refusal guard:** w03, w04, w05, w05b, w14, x10; `let id = |x| x; id(1); id("s")` (CS2-6's control); the sweeps.
**Gate stages:** cli_run, the `axon-core --lib` vm tests, clippy on 4 variants, `vm_wasm_depth.sh`, g01. **Cost:** about 60 lines in
infer.rs and about 80 lines of test moves `[INFERENCE]`.

### CS3-3 (P2) E1209 carries the refinement's line - O-CS2-03

**Problem.** `reject_impure_refinement` (`checker.rs:3047-3134`) builds both E1209 forms with `.at(&file, 0, 0).with_span(self.current_span)`
(`:3069`, `:3127-3128`). Its callers are the item-registration pass (`:1011-1012` for `RefineDef`, `:1017-1019` for a `TypeDef` refinement),
where `current_span` is still the dummy span (`:685`). Both items carry a span: `RefineDef.span` (`ast.rs:58`) and `TypeDef.span`
(`ast.rs:128`). The synthetic inline refinement also gets a real span (`parser.rs:1343`, `self.sp(start, end)`).

**Reproduction.** `D/e_direct.ax`, `D/e_helper.ax`, `D/e_inline.ax`, `D/e_struct.ax` and `D/x/x04_multi_line_refine.ax` each give check 2, and
the E1209 JSON has no `line`/`col` keys. In the same run, `e_direct`'s E1211 has `"line":1,"col":1`. That is the precedent: E1211 passes the
item span (`checker.rs:1462-1491`, `.with_span(span)` with `r.span`/`t.span`). The constant-violation E1209 (`D/x/x03`) already has `4:5`.

**The change.** Add a `span: Span` parameter to `reject_impure_refinement`. The callers pass `r.span` and `td.span`, and both pushes use
`.with_span(span)`. Decision §7.3: use the item's span, not the reached call's. The walker's violations are `(String, &'static str)` (`:3053`)
and carry no span, and the message already names the callee and the path.

**Fail-first test.** `e1209_carries_the_refinement_line` (cli_run): `--json` E1209 for e_direct has `"line":1`, and for x04 `"line":3` (the
`type P` line). e_helper, e_inline and e_struct have the line of their `type`/`fn` item. Today there is no line key.
**g01:** M3981. **False-refusal guard:** none needed (location only). The sweeps must still be identical.
**Gate:** cli_run, clippy. **Cost:** about 10 lines.

### CS3-4 (P1) Bare `self` in an `impl` method is refused - found under O-GAP-08

**Problem.** The parser types a bare `self` param as `Named("Self")` (`parser.rs:1283-1290`: "Bare `self` without `: Type` annotation (e.g.
`fn greet(self)`) uses `Self`"). The checker binds `Self` only inside a `trait` (`checker.rs:1722-1730`). An `impl` body is checked by
`check_fn` with no `Self` in scope (`:1658-1662`), so the type check ends at `checker.rs:7372` E0308 `unknown type 'Self'`. The interpreter
already resolves `Self` against the impl type (`interp/conform.rs:108`, `:347`). Codegen has no `"Self"` case (grep of `codegen/*.rs`).

**Reproduction** (`D/s/`, `D/alt/`, `D/z/`):
- s03 `impl Greet for Dog { fn greet(self) -> str { "woof" } }`: check 2, E0308 5:31.
- a11 (self unused) and z01: E0308 5:31.
- a04 (`fn incr(self) -> Counter`): E0308 6:13.
- The documented example `spec/compiler-phase3.md:1106-1115` (`D/s/s01_phase3_traits.ax`): E0308 4:31.
- The old binary gives the same results (pre-existing).
- Control s02 with `self: Dog`: check 0, run `woof`, native build 0, `./s02n` prints `woof`.
- Trait *declarations* with a bare `self` are legal today (`tests/fixtures/phase23_traits_in_practice.ax:6`, rc 0).

**The change.** In `parse_impl_block` (`parser.rs:1790`), after each method is parsed, replace a param whose type is the bare-self
`Named("Self")` with `blk.for_type.clone()`. Then checker, infer, interpreter and codegen all see the AST that s02 produces. `Self` written
anywhere else in an impl method is not changed (not probed). Rejected alternative: bind `Self` in the checker's ImplBlock arm. That fixes only
the checker and leaves codegen with an unknown `Self` `[INFERENCE]`.

**Fail-first test.** `bare_self_in_impl_method_is_the_impl_type` (cli_run): s03, a11, a04 and the compiler-phase3 example must be rc 0 with
`woof`, `woof`, `1`, and the documented output. Today they are rc 2. Native: the integrator adds s03 to the parity fixture list.
**g01:** M3982. **False-refusal guard:** this item removes refusals. Expected sweep deltas: none, because no tracked `.ax` uses a bare `self` in
an `impl` (a `git grep` for `fn \w+(self)` in `.ax` found only trait declarations and fixtures that pass).
**Gate:** cli_run, parity_all (native), `doc_snippets` (unaffected: `spec/compiler-phase3.md` is not scanned, `doc_snippets.rs:35-40`,
`[REVISED: CS3D-6]`). **Cost:** about 15 lines.

### CS3-5 (P2) Diagnostics for refused forms - O-GAP-08 class (b)

O-GAP-08 is classified per form in this table. Class (a) means supported already (the finding is stale). Class (b) means the refusal is
intended, but the message points the wrong way: in scope here. Class (c) means a language addition: §8, not built. Messages are from
`D/gap-final.txt`.

| form | probe | rc / observed first error | class | the fix in this item |
|---|---|---|---|---|
| `Ok(())`, `let u = ()`, `return ()` | b_unit, b_unit2, g01b/e/f | 2; E0000 `unexpected token: RParen, expected expression`, no help | b + c (NH-1) | 1 |
| `-> Result<(), str>` with `Ok({})` | b_unit4, g01d, alt/a01 | 0 (`ok`) | a | - |
| `if let Some(v) = o` (also `else if let`) | c04, g02b, g02d | 2; E0000 3:8 `unexpected token: Let, expected expression` | b + c (NH-2) | 2 |
| `while let` | c05_while_let | 0, `1\|2` | a (stale) | - |
| `for (a, b) in pairs` / `for (i, x) in arr_enumerate(xs)` | 22_tuples, g03, g03b, g03d | 2; E0000 `unexpected token: LParen, expected identifier` | b + c (NH-3) | 3 |
| `let (n, s) = p` in a `for` body | g03c, alt/a03 | 0 | a | - |
| inherent `impl T { }` | 06_struct_impl, c14, g04b | 2; E0000 `unexpected token: LBrace, expected For` | b + c (NH-4) | 4 |
| trait default method body | 52, g05b, g52b | 2; E0000 `unexpected token: LBrace, expected Fn` | b + c (NH-7) | 5 |
| `?` on Option in an Option fn | 64_option_q | 2; E0303 "`?` ... only ... `Result` ... change the function's return type" | c (NH-6); message accurate | - |
| `?` on Option in a Result fn | g06b | 2; E0102 `type mismatch in ? inner type (expected Result<?1, ?2>), found Option<str>`, no help | b | 7 |
| `o == Some(1)`, `o != None`, `r == Ok(1)` | 62, g07b, g07e, g07c | 2; E0301 "value of type `Option<i64>` cannot be used directly ... (expected i64)" (g07e: `(expected <unknown>)`) | b + c (NH-5) | 8 |
| `==` on structs, arrays, unit variants | 61_eq_variants | 0, `true\|true\|true` | a | - |
| `P { x: 9, ..base }` | 71_struct_update_syntax | 2; E0000 4:28 `unexpected token: DotDot, expected identifier` | b + c (NH-8) | 6 |
| `loop { ... break v }` | 33_loop_kw, g09b | 2; E0001 "cannot find name `loop` ... introduce `loop` with `let loop = ...`" | b + c (NH-9) | 9 |
| `while true` + `break` | g09c, g09d | 0 (`5`, `10`) | a | - |
| tuple variants `Circle(f64)` | c03, g10b, g10e, g10d (pattern) | 2; E0000 `LParen, expected item (...)` / `expected identifier` / `expected FatArrow` | b + c (NH-10) | 10 |
| `Shape::Circle(1.0)` call | g10c | 2; E0404 "Axon variants carry NAMED fields ..." | a (good message) | - |
| `r"..."` raw string | g11c, g11g, g11h | 0 (`C:\dir\file`) | a (stale: `token.rs:212`) | - |
| `r#"..."#` | g11b, g11i | 2; E0000 Hash, help "`#` does not start a comment ..." | b + c (NH-11) | 11 |
| `r"{"a": 1}"` (a `"` inside) | g11, x01, y01, y02 | E0001 `a` (g11), or **check 0 and a wrong output** | CS3-6 | - |
| `mod util` beside main, `AXON_PATH` unset | 38_modules_main, g12 | 2; E0901 `None:None` + searched dirs + AXON_PATH hint | a (documented, `spec/axon-for-llms.md:248-251`) + c (NH-12) | CS3-7 |
| `E::B { .. }` from the E0304 fix-it | q01 (after q04's fix-it) | 2; E0000 5:19 `unexpected token: DotDot, expected identifier` | b (our own fix-it) | 6 |

**The changes.** All parse-tier rows go in `parse_help.rs` (`parse_help`, :92), keyed on the parser message plus the line's words (`line_at`
:62, `words` :81), in the same style as the three-clause `for` rule (:332-339).
1. `RParen, expected expression` with `(` right before it: "Axon has no `()` value - the unit value is an empty block: `Ok({})`,
   `return {}`, `let u: () = {}`". `D/alt/a01` (check 0) shows each of these works.
2. `Let, expected expression` with the previous word `if`: "Axon has no `if let` - write `match o { Some(v) => ..., None => ... }`
   (`while let` exists)".
3. `LParen, expected identifier` with first word `for` and no `;` on the line (the three-clause rule at :333 needs `;`): "`for` binds one
   name - write `for p in xs { let (a, b) = p ... }`".
4. `LBrace, expected For` with first word `impl`: "Axon has no inherent `impl T { ... }` - write a free fn `fn name(t: T) -> ...`, or a
   `trait` plus `impl Trait for T`".
5. `LBrace, expected Fn` inside a `trait` (the line is a `fn` signature): "a trait declares signatures only - put the body in each
   `impl Trait for T`, or in a free fn".
6. `DotDot, expected identifier`. On a line with `=>` (a pattern): "patterns have no `..` - omitted fields are ignored (`E::A { b }`), and a
   unit variant has no braces (`E::B`)". Otherwise: "Axon has no struct update `..base` - list every field: `P { x: 9, y: base.y }`". Also
   rewrite the enum fix-it (`checker.rs:6785`) so it emits valid syntax: `E::B => { /* handle */ }` for a variant with no fields
   (`enum_variant_fields`, :890), else `E::A { f1: _, f2: _ } => { /* handle */ }`.
7. `?` on Option in a Result fn (`infer.rs` `Expr::Question`, :2130, mismatch label `"? inner type"` :2138): add the help "`?` works on `Result` only - convert with
   `match o { Some(v) => v, None => return Err(...) }`". The message stays.
8. E0301 for `==`/`!=` (`checker.rs:5376-5379` call `check_not_option_used_as_value`, :5744-5778, for every binop). For `BinOp::Eq`/`NotEq`
   the message becomes "`==` does not compare `Option` values - match it: `match o { Some(1) => true, _ => false }`", with no "(expected T)".
   The same goes for `Result` (`check_not_result_used_as_value`, :5705-5716). Arithmetic keeps today's text. `lib.rs:1594-1627` (the dedup of
   twin E0301s) keys on (file, line, col, code), so it stays valid.
9. `loop`: add `"loop" => ("Rust", "`while true { ... }` - `break` leaves it")` to `foreign_keyword_help` (`resolver.rs:494-517`). That
   replaces the "introduce `loop` with `let loop`" text emitted at :2106.
10. Tuple variants: the `LParen` errors on a line containing `|` after `type X =`, inside `enum X {`, or after `X::Y` in a pattern get the E0404
    wording: "variants carry NAMED fields: `Circle { r: f64 }`, built `Shape::Circle { r: 1.0 }`, matched `Shape::Circle { r }`".
11. `Hash, expected expression` when the byte before `#` is `r`: "Axon has no `r#\"...\"#` - a raw string is `r\"...\"` and cannot contain
    `\"`; escape it in a normal string (`\"{{\\\"a\\\": 1}}\"`)". This rule is checked before the comment rule (`parse_help.rs:285`).

**Fail-first tests.** `refused_forms_point_at_the_axon_spelling` (`crates/axon-core/tests/parse_help_probe.rs`, which already pins
`parse_help` rows, e.g. :228) gets one case per row 1-6 and 10-11, using the probe sources above; each asserts the help line. Today every help
is `-`. `[REVISED: CS3D-5]` Rows 7, 8 and 9 go in one cli_run fn per lane: `diagnostics_for_refused_forms_checker` (lane A: row 8's message
and the row 6 fix-it), `diagnostics_for_refused_forms_infer` (lane B: row 7's help), and `diagnostics_for_refused_forms_resolver` (lane C:
row 9's help). The fix-it row: q04's fix-it text, pasted as an arm, must check rc 0 (today the paste is q01, rc 2). Row 6's fix-it is
written after CS3-1 lands, in the new walk's witness/fix-it code (CS3-1 deletes :6754-6789), and M3986's `old` is taken from that text.
**g01:** M3983-M3986. **False-refusal guard:** no rc changes. Every row is help text, except the fix-it text. The sweeps must stay identical
by rc and codes. **Gate:** cli_run, `parse_help_probe`, the `parse_help.rs` unit tests, `doc_snippets`. `[REVISED: CS3D-7]` No
`AXON_REFERENCE.md` diff is expected: it lists code descriptions only (E0304 :164), and no code or description changes;
`scripts/reference_gate.sh` must stay green. **Cost:** about 120 lines.

### CS3-6 (P1) A `"` inside a string literal ends it silently - O-GAP-08

**Problem.** The lexer ends a string at the next `"`: the raw-string regex is `r"[^"]*"` (`token.rs:212`), and normal strings work the same
way. The parser takes the literal as a whole expression (`parser.rs:3333-3345`). Statements may share a line (`D/y/y04`, check 0, `3`), so the
rest of the line becomes separate expression statements.

**Reproduction** (`D/y/`, `D/x/`, `D/k/`, `D/g/`):
- y01 `let j = "{{"a"}}"`: check 0, run 0, prints `{`.
- x01 `let j = r"{"a"}"`: check 0, prints `{`.
- y02 `let hi = 2; let j = r"say "hi" now"`: check 0, prints `say `. **Silent wrong answer.**
- k08 (no `hi` bound): E0001 2:20 `hi`. g11 `r"{"a": 1}"`: E0001 2:17 `a`, "did you mean `j`?" (misdirects).
- Controls y05 (`"x"+"y"`, `r"a\b"`, `len(s)`): check 0, `2|a\b!|xy`.
- `[REVISED: Audit-1]` Controls `CS3Audit/a1.ax` (`if s == "bob"{ ... }`, `while s != "bob"{ ... }`, `match s { "bob"=> 1, _ => 2 }`:
  check 0, `yes|1`) and `CS3Audit/a2.ax` (`match s == "bob"{ ... }`, `for c in ["a", "b"]{ ... }`: check 0, `1|a|b`). A `{` right after a
  string is valid Axon.

**The change.** In the primary-expression arm for `Token::Str`/`Token::RawStr` (`parser.rs:3333-3345`), after the literal is consumed: if the
next token starts at the exact byte where the literal ended (`self.spans`, parallel to `tokens`, `parser.rs:258-275`) and that token is an
`Ident`, `Str`, `RawStr`, `Int` or `Float` (`[REVISED: Audit-1]`: not `LBrace`), return a parse error. The error: "this string ends at the `\"` before `{next}` - write a
`\"` inside a string as `\\\"`; a raw string `r\"...\"` cannot contain `\"`".

**Evidence that it refuses nothing valid.** `D/adjscan2.py` (a string, or `r"`, closed on the same line and followed with no gap by an
alphanumeric, `_`, `"` or `{`, a superset of the rule) found 0 sites in the 608 tracked `.ax` files (`D/adjscan2.out`) and 0 in the 324
frozen corpus programs, extracted through `ab/corpus.load` as the frozen sweep does (`D/adjscan2-frozen.out`). A zero count does not make a
form invalid (Audit-1); a1/a2 are the controls that pin it. Spaced adjacency (y03) is O-CS3-02.

**Fail-first test.** `adjacent_string_literal_is_refused` (cli_run): y01, x01, y02, g11 must be rc 2 E0000 with the new message. Today
y01/x01/y02 are rc 0. Controls y04, y05, g11c, g11g, g11h, a1, a2, and `D/y2/y06_spaced_ident.ax` (`let s = "a" let t = 1`, check 0, prints
`a|1`) must stay rc 0 with today's output.
**g01:** M3987 (the guard), M3988 (the zero-gap condition; killer y05 `[REVISED: Audit-7]`). **Gate:** cli_run, sweeps. **Cost:** about 25 lines.

### CS3-7 (P3) E0901 names the `use` line - O-GAP-08

**Problem.** `MergeError` (`lib.rs:349-353`) has no span, and `UseDecl`/`ModDecl` (`ast.rs:157-166`) carry none. `run_check_pipeline_inner`
pushes load errors with line 0 (`main.rs:7378`: "MergeErrors carry no span ... line/col stay 0").

**Reproduction.** `D/g/38_modules_main.ax` with `AXON_PATH` unset: check 2, E0901 at `None:None`, with the searched directories and the
`AXON_PATH` hint (`lib.rs:1143-1151`). With `AXON_PATH=.` it gives rc 0 and `16|27`.

**The change.** Add `span: Span` to `UseDecl` and `ModDecl`, with `#[cfg_attr(feature = "serde-json", serde(default))]` as `RefineDef.attrs`
does (`Span` derives `Default`). Set it in the parser (`parser.rs:1835`, `:1855`). Add `span: Option<Span>` to `MergeError` (old JSON without
the key reads as `None` `[INFERENCE: serde derive default for Option]`), filled where the not-found error is built from the `use` path. The
`main.rs:7387-7395` push then uses its line/col. Cyclic-module errors also get the `use` span.
`[REVISED: Audit-6]` Loading recurses into loaded modules' `use` lines (`lib.rs:1035-1042`). Use the span with the entry-file label only when
the failing `use` is in the entry file. Otherwise set `MergeError.file` to the importing module's path and label the diagnostic with it.
`[REVISED: CS3D-11]` Update the `UseDecl {}`/`ModDecl {}` literals in the resolver unit tests (`resolver.rs:4122-4138`) and the `MergeError {`
sites in `lib.rs` (:376, :934, :974, :1005, :1077, :1088, :1149).
**Fail-first test.** `module_not_found_names_the_use_line` (cli_run, `AXON_PATH` removed from the child env, both modules written to a
temp dir by the test): `--json` E0901 has `"line":2` (the `use mathutil...` line). A second case puts the missing `use` inside the loaded
module and asserts that module's file name and line. Today there is no line.
**g01:** M3989. **Gate:** cli_run, `--features serde-json` tests (the `axon parse` JSON shape gains `span`), clippy. **Cost:** about 40 lines.

## 3. Shared mechanics

- `[REVISED: CS3D-9]` Each commit that touches `crates/axon-core/src` is followed by a `pci: theme for <sha8>` commit adding
  `governance/notes/pci-themes/<sha8>.json`, then `python3 tasks/r51-pci-regen.py`, then `pci_delta.py --check`.
- g01 rows and `ATTACK:` markers (`scripts/v022_g01_mutations.py`, `scripts/v022_attack_markers.py`) land in the same commit as their fix. Each
  killing test's assert message carries the marker text.
- `AXON_REFERENCE.md` is regenerated with `axon reference`, never edited by hand. Expected: no diff (CS3D-7).
- `[REVISED: CS3D-3]` Each fail-first and control program is inlined into its test as a string literal (or copied under
  `crates/axon-core/tests/fixtures/`). No test reads `/var/tmp`; `D/` and `CS3Audit/` paths in this spec are evidence only.
- `cli_run.rs` is shared. Each lane appends its new test fns in one block at the end of the file. CS3-2 also edits `vm_pure_bool_ops` and
  `vm_superop_write_panics` in place (cited by fn name, `[REVISED: CS3D-2]`). The integrator merges.

## 4. g01 rows (M3970-M3989)

The row format is CS2's (`checker-soundness-2-spec.md:534-541`): one file, a unique `old`, one cargo-test killer. The builder writes the exact
`old` line. Target `--no-default-features --test cli_run` unless named.

| id | item | guard the mutation removes | file | killing test |
|---|---|---|---|---|
| M3970 | CS3-1 | specialization drops rows whose head is another constructor (mutant: keep them) | checker.rs | `payload_literal_holes_are_refused` |
| M3971 | CS3-1 | a wildcard head expands to arity wildcards (mutant: drop wildcard rows) | checker.rs | `payload_matches_that_cover_stay_legal` (c01) |
| M3972 | CS3-1 | the bool constructors `true`/`false` (mutant: bool is an infinite domain) | checker.rs | same (c02, c05) |
| M3973 | CS3-1 | an omitted enum field is a wildcard (mutant: never covers) | checker.rs | same (q02) |
| M3974 | CS3-1 | an infinite domain needs the default matrix (mutant: literal heads count as complete) | checker.rs | `payload_literal_holes_are_refused` (h31) |
| M3975 | CS3-1 | a tuple component uses the recursive walk (mutant: CS2-4's irrefutable-payload test) | checker.rs | `payload_matches_that_cover_stay_legal` (c05, c16, c17) |
| M3976 | CS3-1 | a `Var` subject goes through the walk (mutant: `_ => {}`) | checker.rs | `payload_literal_holes_are_refused` (h31g) |
| M3977 | CS3-1 | variant field types are registered (mutant: empty map, so payload bools are read off the patterns) | checker.rs | same (h31e: `true` only) `[INFERENCE: with patterns only, h31e still has a Bool domain; if the mutant survives, the builder replaces this row with "the variant's field order", killed by c21]` |
| M3978 | CS3-2 | an obligation records `derived[v]` (mutant: skip) | infer.rs | `wrapper_lambda_param_kind_is_checked` (h24) |
| M3979 | CS3-2 | `[REVISED: Audit-7]` an obligation nested in `if`/`match`/`&&`/`\|\|` records no kind (mutant: depth ignored) | infer.rs | `wrapper_lambda_controls_stay_legal` (w14, w05b) |
| M3980 | CS3-2 | the fixpoint repeats (mutant: one pass) | infer.rs | `wrapper_lambda_param_kind_is_checked` (w02) |
| M3981 | CS3-3 | `.with_span(span)` on E1209 (mutant: `self.current_span`) | checker.rs | `e1209_carries_the_refinement_line` |
| M3982 | CS3-4 | the bare-self rewrite in `parse_impl_block` | parser.rs | `bare_self_in_impl_method_is_the_impl_type` |
| M3983 | CS3-5 | the `loop` row of `foreign_keyword_help` | resolver.rs | `diagnostics_for_refused_forms_resolver` |
| M3984 | CS3-5 | the `impl T {` row | parse_help.rs | `refused_forms_point_at_the_axon_spelling` (`--test parse_help_probe`) |
| M3985 | CS3-5 | the `==` branch of the E0301 message | checker.rs | `diagnostics_for_refused_forms_checker` |
| M3986 | CS3-5 | the valid enum fix-it (mutant: `{ .. }` text; `old` from post-CS3-1 code) | checker.rs | same (q04 fix-it paste) |
| M3987 | CS3-6 | the zero-gap refusal | parser.rs | `adjacent_string_literal_is_refused` (y02) |
| M3988 | CS3-6 | `next.start == lit.end` (mutant: `>=`) | parser.rs | same (y05 control: `r"a\b"` then a newline then `println`, refused by the mutant) `[REVISED: Audit-7]` |
| M3989 | CS3-7 | `MergeError.span` used for line/col | main.rs | `module_not_found_names_the_use_line` |

## 5. Gates and cost budget (no paid spend)

| stage | wall | source |
|---|---|---|
| S290 sweep: `bash /var/tmp/r51-tmp/cs-sweep.sh <repo> <bin> <ABSOLUTE out>` | 6.5 s | measured on main77; baseline `D/s290-main77.txt` (302 files, rc0 286). The script `cd`s into the repo, so a relative out path writes into the worktree |
| S596 sweep: `bash /var/tmp/r51-tmp/CS2Draft/cs-sweep-all.sh <repo> <bin> <ABSOLUTE out>` | 10.1-13.5 s | measured; `D/s596-main77.txt` (608, rc0 521) |
| frozen corpora: `AXON_BIN=<bin> python3 D/frozen_check_sweep.py <repo> <out>` | 6.5 s | measured; `D/frozen-main77.txt` (324, rc0 321) |
| `cargo test -p axon-core --no-default-features --test cli_run --no-run`, cold / after touching checker.rs | 26 s / 4.4 s + about 0.1 s per filtered test | measured by DevloopCS3 `[REVISED: CS3D]` |
| `--lib interp::vm` (43 tests), cold / after touching infer.rs | 15 s / 4.5-5.5 s | measured by DevloopCS3 |
| `--test parse_help_probe` | 4.6 s | measured by DevloopCS3 |
| one g01 row | about 15 s (two checker.rs rows in one invocation: 31 s) | measured by DevloopCS3 |
| 20 rows (M3970-M3989) | about 5-8 min | 15 s per row measured; infer.rs and main.rs rows `[INFERENCE]` |
| `g01_anchors.py` / `v022_refusal_coverage.py` | 0.3 s / 14 s | measured by DevloopCS3; no new protected site expected (CS3D-8) |
| clippy ×4, `integration_fixtures` (codegen), `native_shapes_parity.sh`, S04 fence sweep | not measured | `[INFERENCE]` |
| `parity_all.sh` | ≈ 21 min | lessons |
| full `scripts/gate.sh --strict` | 42-44 min | lessons |

`[REVISED: CS3D]` Per-lane inner loop, no strict gate: edit, then `cargo test -p axon-core --no-default-features --test cli_run <killer>`
(about 5 s warm), plus `--lib interp::vm` for lane B and `--test parse_help_probe` for lane C. Then the three sweeps (about 25 s) and that
lane's g01 rows with `--scope=all --only=<ids> OUT.json` (about 15 s each). A lane is ready for the strict gate in under 10 min of machine
time after the edit. `parity_all` and the strict gate run once, at the integrator.

Re-running the three sweeps on main77 reproduced all three baselines byte-identically (2026-10-31). The integrator copies the scripts and
baselines into the build harness; they are not committed. Every lane brief lists these stages:
- `cargo clippy -D warnings` for axon-core `--no-default-features`, `--features serde-json`, `--features smt`, and default (codegen)
  `--all-targets`.
- `scripts/parity_all.sh`, `scripts/*_parity.sh` (including NativeParity's `native_shapes_parity.sh` with the 6 CS3-1 files).
- `integration_fixtures`, `vm_wasm_depth.sh`, `doc_snippets`.
- `scripts/spec_ledger.py`, `scripts/g01_anchors.py`, `pci_delta.py --check`, `scripts/v022_refusal_coverage.py`, `scripts/reference_gate.sh`.
- The three sweeps, which must be identical to the baselines except for deltas named in the items: CS3-4 removes refusals; the others expect none.

## 6. Sequencing and lanes

| lane | items | files |
|---|---|---|
| A | CS3-1, CS3-3, CS3-5 rows 6 (fix-it) and 8 | `checker.rs` |
| B | CS3-2, CS3-5 row 7 | `infer.rs`, `interp/vm/tests.rs` (no other spec owns it) |
| C | CS3-4, CS3-5 rows 1-5, 9-11, CS3-6, CS3-7 | `parser.rs`, `parse_help.rs`, `resolver.rs`, `ast.rs`, `lib.rs` (MergeError), `main.rs` (:7387-7395), `tests/parse_help_probe.rs` |

Integrator-owned: `cli_run.rs` (each lane's block appended at the end, plus B's in-place edits of the two VM tests), the g01/marker files,
`CHANGELOG.md`, `tasks/opportunities.md` (stamps for O-CS2-02, O-CS2-03 and O-GAP-08's class (b) part; it files O-CS3-01..03), `AXON_REFERENCE.md`,
and `[REVISED: CS3D-10]` `scripts/native_shapes_parity.sh` (CS3-1 adds 6 cases and removes `np2_h31`/`np2_u02`; CS3-4 adds s03) and
`crates/axon-cortex/benchmarks/fence_sweep.py` (a `COMPILE_CAUGHT` entry, if any, lands with CS3-1 and its scorecard ceiling note).

Cross-spec order `[REVISED: CS3D-1, Audit-5, Audit-8, integrator ruling]`:
- native-parity is BUILT on np/int (`np-int/tasks/native-parity-spec.md:3`, integrate commit db1f0642) and merges to main first. **Every CS3
  lane branches from main after that merge, and every line anchor in this spec is re-read at that base** (CS3D-2: other lanes also insert
  lines into `cli_run.rs` above the cited tests).
- NP-1 (lane-table row `A1 NP-1`, np-int `native-parity-spec.md:561`) fixes the native tuple-match arm, so CS3-1's re-accepted tuple matches
  are lowered correctly natively.
- NP's `infer.rs` hunks (`call_instantiations`/`drain_instantiations` and tests) do not touch the `call_obligations` loop. After NP the loop
  is at `infer.rs:3072` (was :3063) and `"? inner type"` at `:2143` (was :2138). CS3-2 has no textual overlap (checked with `git diff`).
- CS3-1 turns h31/h32/u02 into check refusals. NP-2's fixture case then asserts `axon run`/`axon build` exit 2
  (`integration_fixtures.rs:2948-2953`, per audit), and its IR test `a_match_miss_edge_panics` stays the killer.
- No `axon-ai` or `examples/asi` file is touched (asi-replay disjoint).

## 7. Engineering decisions (decided here)

1. **One usefulness walk replaces three coverage paths** (CS3-1).
   - Options: (i) patch `cell_covers` to OR sibling rows per variant. This fixes c05 but not nested cases (c18, c20). (ii) Add payload checks
     to the Option and enum branches separately. That leaves two algorithms that must agree.
   - Chosen: the standard algorithm (Maranget, default matrix for incomplete signatures). It is exact for finite constructors, and it accepts
     whenever a type is undecided. `[REVISED: Audit-2]` The 256 cap stays where CS2-4 has it; elsewhere an over-budget walk accepts (e9, t9).
   - `[REVISED: Audit-3]` b2/b3 (untyped lambdas matching a `str` with no catch-all) become E0304. They panic on any uncovered input, and the
     typed twin b1 is E0304 today; this is the existing rule reaching a `Var` subject, not a new rule.
2. **Derived kinds, no unification** (CS3-2).
   - Option: bind the wrapper's `Var` to the callee param. That would make `g` monomorphic, and would refuse w05-like programs where the two
     uses disagree.
   - Chosen: a kind-only side map with a `Conflicted` state, recording kinds only from calls at nesting depth 0 of the wrapper body
     (`[REVISED: Audit-4]`). w14 and w05b stay rc 0, so the default build refuses no running program. Refusing w14 is NH-13.
3. **E1209 at the item span** (CS3-3), not the reached call's: the walker's violations carry no span.
4. **Bare self is rewritten in the parser** (CS3-4), so every back end sees one AST.
5. **CS3-6 refuses only zero-gap adjacency to `Ident`/`Str`/`RawStr`/`Int`/`Float`.** Spaced adjacency is legal statement juxtaposition
   elsewhere, and `"bob"{` is a valid `if`/`while`/`match` head (a1, a2; `[REVISED: Audit-1]`).
6. **The VM panic path is tested below the checker** (CS3-2), not kept alive through an unchecked shape.

## 8. Needs human

Language additions from O-GAP-08. Evidence of how often models write each form (`D/evidence.py`, `D/form_scan.py`):
- The GapLang probes (197 model-written files, written to find gaps, so biased toward these forms) give the counts in the table.
- Tracked `examples/**/*.ax` (182) and benchmark `.ax` (120) have 0 of every form. They must compile, so this is not evidence of demand.
- The benchmark result JSONL records (8478) and the RLM transcripts (`tasks/*rlm*.txt`, `attempts.log`) have 0 of every form. The records
  mostly carry no program text `[INFERENCE]`.

So there is **no corpus evidence beyond GapLang**. The ranking uses GapLang counts and `[INFERENCE]` from Rust/Python habits.
`spec/axon-for-llms.md` gotchas document only #10 (AXON_PATH); none of the other forms is listed as unsupported.

| NH | form | GapLang files | recommendation |
|---|---|---|---|
| NH-1 | `()` unit value (`Ok(())`) | 3 | **Add.** `spec/grammar.ebnf:484-487` already names `UNIT ::= '(' ')'` and says the parser does not accept it; the type `()` and `Ok({})` work, so this is parser-only (lower `()` to the empty block) |
| NH-12 | `mod util` searches the importing file's directory | 13 use `mod` | **Add, searched last** (after `AXON_PATH`, `~/.axon/lib`, `<bindir>`), so no existing resolution changes. It removes gotcha #10 |
| NH-5 | `==`/`!=` on Option/Result | 2 | **Add after native-parity** lowers composite equality. The interpreter already compares structurally (g07d: a struct holding an Option, check 0, `true\|false\|true`), but native refuses with E0910 (`D/nat/build-g07d_opt_in_struct.log`). Allowing it now creates a check-pass, build-fail path |
| NH-2 | `if let` | 1 | **Add**, desugared to `match`. `while let` already exists (`ast.rs:370-376`), so the asymmetry surprises |
| NH-3 | `for (a, b) in` | 1 | **Add**, desugared to `for p in xs { let (a, b) = p ... }` (g03c works) |
| NH-4 | inherent `impl T { }` | 2 | **Defer** until CS3-4 lands; then decide whether impl methods may exist without a trait (method lookup changes) |
| NH-6 | `?` on Option | 1 | **Decline for now.** E0303 is accurate, and an implicit Option-to-Result conversion picks an error value for the user |
| NH-7 | trait default methods | 1 | **Decline.** A free fn works (g05c, check 0) |
| NH-8 | `..base` struct update | 1 | **Decline.** CS3-5 row 6 names the spelling |
| NH-9 | `loop` | 1 | **Decline.** `while true` + `break` works (g09c, g09d); CS3-5 row 9 says so |
| NH-10 | tuple-variant enums | 1 | **Decline.** Named fields are a stated design (E0404's own help) |
| NH-11 | `r#"..."#` | 1 | **Decline unless NH-1-level demand appears.** `r"..."` exists, and CS3-6 makes a stray `"` a refusal instead of a wrong answer |
| NH-13 | not a form: refuse a wrapper lambda whose kind-conflicting call sits in a branch (w14 `let g = \|y, c\| if c { f(y) } else { 0 }`) | 0 (w14 is a probe of mine) | **Decline.** w14 runs today and prints `0`; refusing it is a check-time refusal of a program that never fails. The direct twin w12 is already E0102, so the asymmetry is documented, not closed |

No item in §2 adds language surface, changes a builtin's behaviour or return type, or spends money.
