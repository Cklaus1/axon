# checker-soundness — DRAFT spec

**Status: Step 1 reviewed; needs-human decided (§7). Authorized for build by human 2026-10-27, as one batch with `tasks/capability-surface-spec.md` (harness: `tasks/build-loop.md`).**

### Step 1 review

Two reviews: `agent://AuditChecker` (CSA#1–20) and `agent://DevloopChecker` (CSD#1–18). I re-ran every
cited probe (`/var/tmp/r51-tmp/review-CSA/me/*.ax`) and read every cited line before deciding.

| review | blocker | major | minor | accepted | partial | rejected |
|---|---|---|---|---|---|---|
| CSA | 1 | 14 | 5 | 19 | 1 (#14) | 0 |
| CSD | 0 | 12 | 6 | 18 | 0 | 0 |

| id | disposition (one line) |
|---|---|
| CSA#1 | accepted: Uncertain `source_tag: i64`; Temporal has no `source_tag` (`vt.ax` re-run: check 0, run panic 101) → CS-2 binder table |
| CSA#2 | accepted: an int literal against an f64 operand is refused; the decodable simple shape stays accepted (`vf3.ax`, `rf.ax` re-run: panic 101) → CS-1/CS-2, E-6 |
| CSA#3 | accepted: E-2 now refuses only binders bound for that return shape; `agent_task.ax` added to non-regression |
| CSA#4 | accepted: CS-3(a) covers every return shape (`vu.ax` re-run: rc 0, prints 5) |
| CSA#5 | accepted: one predicate pass for all files, E1211 not E0001; the sealed union rule is unsound (`multi.ax` re-run: prints 3 then panic 101) |
| CSA#6 | accepted, plus one addition: unused/nested RefineDef binds `{_}`; the predicate pass refuses `return`/`break`/`continue` explicitly (instead of relying on the bool constraint); 2 tests listed |
| CSA#7 | accepted: measured blast radius replaces the old `[INFERENCE]` (re-ran learn 136/200 and file-budget 0/100: both `deploy gate: passed` rc 0) |
| CSA#8 | accepted: new item CS-13 (goal target extraction, compile.rs:327-334); NH-1(a) depends on it |
| CSA#9 | accepted: README.md:52-53 cited as the conflicting doc; E-1 rationale rewritten; phase5.md:377-387 added to NH-1; `verify_ai_source.ax` listed |
| CSA#10 | accepted: CS-4 change rewritten (`&mut` dispatch, borrow/LSP sig lookups, P/E/Var, codegen); `mutshadow.ax` re-run: W0003, prints 8 |
| CSA#11 | accepted: CS-6 discards only `return`-terminated branches (`brkmove2.ax` re-run: E0601, correct today) |
| CSA#12 | accepted: arrays copy on `let` per language-tour.md:319 (`arrloc.ax` prints 4); NH-3 gains option (d); CS-8 gotcha row corrected |
| CSA#13 | accepted: the `(` case is a parser bug at parser.rs:2989 (read) → new CS-10; the gotcha row is withdrawn |
| CSA#14 | partial: o24_ret/o24_unit reproduced (re-run) → new CS-11/CS-12; `gate-lane-spec.md:14` is not my file → reported to Main, not edited |
| CSA#15 | accepted: comma `--only`, ATTACK markers, one file and one killer per row, pci line rule (merged with CSD#6/#7) |
| CSA#16 | superseded by the cross-spec decision CAPA#1 (Main): purity is E1104 in `effects::check_effects` |
| CSA#17 | accepted: four-site code registration, plus the missing E1210 in the all-codes list |
| CSA#18 | accepted: radix hint differs between engines (interp trims, axon-rt does not) `[INFERENCE: native not run]` |
| CSA#19 | accepted: infer has no deferred obligations; CS-5 adds a post-`solve()` list |
| CSA#20 | accepted: tally corrected (merged with CSD#4) |
| CSD#1 | accepted: `./scripts/gate.sh --strict` at the integrated head (codegen touched) |
| CSD#2 | accepted: native cases go in `exit_code_parity.sh` + `integration_fixtures.rs` (codegen); `pipeline_parity.rs` gets check-pipeline cases |
| CSD#3 | accepted: the false "examples stay covered" sentence is replaced |
| CSD#4 | accepted: sweep counts redone with `AXON_PATH`; measurement only, not a gate |
| CSD#5 | accepted: 290-file check-outcome sweep (≈ 2 s) is a gate row; CS-1 non-regression widened to all 108 benchmark programs |
| CSD#6 | accepted: every `crates/axon-core/src` row adds 1 to the pci line (561); the head is 44c8311c, not b789a8f6 |
| CSD#7 | accepted: g01 row cost and prerequisites in §5 |
| CSD#8 | accepted: killers swapped (now M3834/M3835); a new non-diverging guard test |
| CSD#9 | accepted (same as CSA#3) |
| CSD#10 | accepted (same as CSA#7) |
| CSD#11 | accepted: unknown callee test and acceptance added to CS-2 |
| CSD#12 | accepted: `--no-default-features` targeted command |
| CSD#13 | accepted: `--fast --base=main` row and full g01 `--scope=all` row |
| CSD#14 | accepted: pci batching paragraph |
| CSD#15 | accepted: fixture placement rules (vm_parity, build-outcome baseline) |
| CSD#16 | accepted: cortex trigger includes verify enforcement changes |
| CSD#17 | accepted (with CSA#9): `verify_ai_source.ax`; "wrapper inner" means Uncertain/Temporal only |
| CSD#18 | accepted: a fresh codegen build row; native is still unobserved |

Base commit: `7f110693` (`specs/next` = `main`). Source opportunities (`tasks/opportunities.md`):
**O-S1-22** (a struct-field refinement that names its own field passes check and panics at run), **O-S1-23**
(`@[verify]` predicate names are not checked), **O-FU-06** (builtin-shadow arity, plus the author-notes
items it collects, opportunities.md:2689-2694; `/var/tmp/r51-tmp/AuthorC2-NOTES.md`, `AuthorC3-NOTES.md`),
and the reproduced **O-S1-24** items. [REVISED: CSA#14 — triage note opportunities.md:2670 sends them here]

**The defect class.** A program that `axon check` accepts with rc 0, and that then either dies at run time
with an interpreter-internal panic (rc 101) or has an annotation that is silently not enforced. A second
class is the reverse: valid programs that the checker refuses. The interpreter (`crates/axon-core/src/interp.rs`)
is the reference semantics (AGENTS.md), so the checker must accept exactly what it can run.

**How repros were run.** All repros use `/var/tmp/r51-tmp/hx-bin/axon`. It is interpreter-only: `axon build`
reports that it needs the `codegen` feature. Sources are in `/var/tmp/r51-tmp/specs-CheckerSoundness/` and
`/var/tmp/r51-tmp/review-CSA/me/`. **No native run was possible.** The only other codegen binary found is stale
(CSD#18), so every native claim is `[INFERENCE]` from codegen source.

## 1. Out of scope and cross-spec dependencies

| item | where | reason |
|---|---|---|
| verify **purity** (O-S1-19), `effect_union` (O-S1-20), walker merge (O-FU-05) | `tasks/capability-surface-spec.md` | [REVISED: cross-spec CAPA#1 — was "hooks its purity check into CS-2's `check_verify_predicate` and uses E1103"] Its purity check is **E1104 in `effects::check_effects`**, independent of `check_verify_predicate`, because it needs effect inference that the checker lacks. Ordering between the two specs is review order only, and `error.rs` edits land serially. Cap-surf relies on CS-2's E1103 for an unbound verify callee (`is_ok`, its :135), which is covered by CS-2's `verify_predicate_unknown_callee_is_refused_e1103`. |
| O-FU-03/04 | `tasks/unattended-auth-spec.md` §6 E5 | not this theme |
| O-S1-24 | **here** | [REVISED: CSA#14 — was "stays logged; only the `len` case reproduced"] Four of its five items reproduce: match-pattern `len` → CS-5; `&[T]` `let` → CS-7; `return` in a match arm → CS-11; `type Unit` → CS-12. "A param used in two match arms" does not reproduce: `o24_arms.ax` checks rc 0 and prints 3 `[INFERENCE: the probe shape may differ from the author's]`. Mark O-S1-24 closed only when CS-5, CS-11 and CS-12 have landed and NH-3 is decided. `tasks/gate-lane-spec.md:14` still calls it "not reproduced" (reported to Main; not my file). |
| `to_str` on a `?`-bound f64 | — | not reproduced: `n_tostr.ax` checks rc 0 and prints `1.5` rc 0 |
| a distinct exit class for interpreter-internal panics | — | a runtime change, not a checker one |
| a hello-goal score metric | — | CS-13 only stops mis-reading the 280 length cap; designing a real score target is product work |

**Benchmark programs.** The 108 benchmark programs (hardset 42, headroom 46, tempt 20) must keep checking
rc 0 under every rule below [REVISED: CSD#5 — was "headroom/hardset"]. They use param refinements
(`histogram.ax:38`) and `@[verify]` on `value`, `confidence` and `score` (`gradebook.ax:26`, `interest.ax:8`,
`redteam-goal.ax:33`). `tasks/hx2-spec.md` should build its set after CS-1/CS-2 land, or re-check it then.

## 2. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | CS-1, CS-2, CS-4 | check-accepted programs panic (rc 101) or call the wrong signature |
| P2 | CS-3, CS-5, CS-11 | silently unenforced `@[verify]`; false E0102 on valid `len` / divergent match arms |
| P3 | CS-6, CS-7, CS-8, CS-9, CS-10, CS-12, CS-13 | false E0601; move model; docs; invariant test; parser newline; `type Unit`; goal target |

## 3. Items

### CS-1 (P1) Refinement predicates are not name- or type-checked — O-S1-22

**Problem.** The checker defers refinement predicate checking.
- `checker.rs:1235-1271` (`Item::RefineDef`) leaves predicate well-formedness to "later sub-slices" and checks only `if self.sealed_at(..)` (1259-1270).
- `resolver.rs:1473-1507` resolves refinement predicates only for sealed files. It binds `_` plus the **union** of param names over all uses (`refined_param_names`, resolver.rs:2419-2437). That union is unsound: see `multi.ax` below [REVISED: CSA#5].
- An unbound name is reported as E0001 (resolve_expr), not as a predicate-specific code.
- Inline `T where P` desugars to `__refine_N` RefineDefs (parser.rs:1313-1341).

The interpreter binds the following names in each position:

| position | binds | anchor |
|---|---|---|
| param `n: T where P` | `_`, `n` | interp.rs:6202-6205 |
| `let p: T where P` | `_`, `p` | interp/eval.rs:964-966 |
| return `-> (T where P)` | `_`, all params (body locals are invisible: `retloc.ax` panics on `k`) | interp.rs:6553-6559 |
| struct field `{ v: T where P }` | `_` only | interp/eval.rs:2726-2727 |
| whole struct `{..} where P` | `_` = instance | interp/eval.rs:2749-2757 |
| nested only (`(Pos, i64)`, `Option<Pos>`, array element, fn-type param) or unused | nothing is checked at run time | [REVISED: CSA#6] no binding site found |

The documented binder is `_` only (`spec/compiler-phase5.md:84-86`; `ast.rs:44-58`). Module globals and
user fns are visible at run time (`vg.ax`: global `LIM` → verify failed, rc 3).

**Repro** (`axon check F`, then `axon run F`):

| file | snippet | check | run |
|---|---|---|---|
| `s22.ax` | `type Amt = { value: i64 where value > 0 }`; `mk(5)` | rc 0 | `axon: panic: undefined identifier \`value\`` rc 101 |
| `r_const.ax` | same, `Amt { value: 5 }` | rc 0 | same panic, rc 101 |
| `r_whole.ax` | `type R = { lo: i64, hi: i64 } where lo <= hi` | rc 0 | `panic: undefined identifier \`lo\`` rc 101 |
| `r_param.ax` | `fn f(n: i64 where nosuch > 0)` | rc 0 | `panic: undefined identifier \`nosuch\`` rc 101 |
| `r_named.ax` | `type Pos = i64 where nosuch > 0` | rc 0 | panic, rc 101 |
| `r_rel.ax` | `fn f(lo: i64, hi: i64 where hi >= lo)` | rc 0 | `panic: undefined identifier \`lo\`` rc 101 |
| `r_type.ax` | `type Pos = i64 where _ && true` | rc 0 | `panic: cannot apply And to i64 / bool` rc 101 |
| `multi.ax` [REVISED: CSA#5] | `type Pos = i64 where n > 0; fn f(n: Pos); fn g(m: Pos)` | rc 0 | prints `3`, then `panic: undefined identifier \`n\`` rc 101 |
| `rf.ax` [REVISED: CSA#2] | `type P = f64 where _ >= 0` | rc 0 | `panic: cannot apply GtEq to f64 / i64` rc 101 |
| `escape_predicate.ax` [REVISED: CSA#6] | `fn solve(n: i64 where if n > 2 { break } else { true })` | rc 0 | prints `40 41 42`, then `panic: \`break\`/\`continue\` outside a loop in a predicate` rc 101 |
| `r_ok.ax` (control) | `{ value: i64 where _ > 0 }`, `mk(0)` | rc 0 | `refinement violated: field \`value\` of \`Amt\` (= 0) …` rc 6 |
| `r_paramname.ax`, `r_let.ax` (controls) | `n: i64 where n > 0`; `let p: i64 where p > 0 = k` | rc 0 | rc 6 / prints 3 |

**Change.** [REVISED: CSA#5 — was "lift the sealed-only branch; sealed behaviour kept"]

1. **One predicate-scope pass for all files, sealed included.** It replaces the sealed branch at resolver.rs:1490-1507, and runs in the resolver after items are collected.
   - Scope: module items, builtins, and the binders from a new `refine_binders(program, refine_name) -> Vec<(String, Type)>`. This replaces `refined_param_names`.
   - Binders are the **intersection** over the refinement's direct use sites (table above). A RefineDef with no direct position use (unused, or used only nested) binds `{_}` [REVISED: CSA#6].
   - An unbound name or callee inside a refinement predicate is **E1211**, never E0001. This pass owns the predicate, so there is no double report.
   - `return`, `break` and `continue` anywhere in a predicate are E1211 "a predicate cannot leave its frame" [REVISED: CSA#6 — explicit, not via the bool constraint].
   - The sealed checker pass at checker.rs:1259-1270 stays as an additional pass.
2. **Checker type pass.** Infer the predicate in that scope and constrain it to `bool`. Add a new `InferCtx` entry that takes explicit bindings.
   - An integer literal compared with an f64 operand is E1211 "`{lit}` is i64 and the operand is f64: write `{lit}.0`" [REVISED: CSA#2].
3. **Diagnostic E1211.** It is free (E1210 is at `error.rs:116`, and `git grep E1211` is empty outside `tasks/`).
   - Messages: "refinement predicate of `{T}` names `{x}`, which is not in scope here" / "… is `{ty}`, not bool" / "ill-typed operand …" / "cannot leave its frame".
   - Fix hint: "the value is `_` (inside a whole-struct refinement: `_.field`)".
   - A field refinement that names its own field is E1211 unless NH-2 = (a).
4. **Register E1211 (and E1103) at four sites** [REVISED: CSA#17]:
   - the `pub const` in error.rs (drift-guarded by cli_run.rs ≈20968-21055);
   - the description table (error.rs:503-514);
   - the all-codes list (error.rs:751-752), and add the missing E1210 there;
   - `AXON_REFERENCE.md` (217-228).

**Tests that change** [REVISED: CSA#6]:
- `integration_fixtures.rs:2299` `type Pos = i64 where self > 0` is used only nested (:2356). It is prepended to all 11 cases of `ax41_enums_inside_tuples_and_other_containers_check_clean` (:2366-2371), which then hit E1211. Change the fixture to `_ > 0`.
- `r51_s0b_loop_control_never_leaves_its_frame_on_either_engine` (cli_run.rs:39988; `escape_predicate` case) pins run rc 101. Change that case to expect check rc 2 + E1211. Runtime containment stays covered by `interp::tests::loop_control_does_not_escape_through_a_predicate` (M59's killer, `--lib`; unaffected).
- `axon-fabric/tests/check_effects.rs:1159-1209` (`a_candidate_predicate_cannot_end_the_operators_test`) asserts only "not Passed" for `return`-in-predicate candidates. It stays green `[INFERENCE: not run]`; re-run it in the fabric suite.

**Fail-first tests.** All in `crates/axon-core/tests/cli_run.rs`. Each is check rc 0 today and must be rc 2 + E1211 after.

| test | probe(s) |
|---|---|
| `struct_field_refinement_naming_its_field_is_refused` | s22 |
| `whole_struct_refinement_with_bare_field_names_is_refused` | r_whole |
| `refinement_predicate_unknown_name_is_refused` | r_param, r_named, r_rel |
| `refinement_named_by_two_different_params_is_refused` | multi |
| `refinement_predicate_non_bool_is_refused` | r_type |
| `refinement_predicate_int_literal_against_f64_is_refused` | rf |
| `predicate_with_return_or_break_is_refused` | escape shape, for param, return and verify |

Non-regression:
- `param_and_let_refinements_naming_their_binding_still_check` (r_paramname, r_let).
- The 290-file check sweep (§5) unchanged except the files named above.
- `pipeline_parity.rs` `cases` (:54-74) gains s22, so E1211 also reaches `lib::check_pipeline` [REVISED: CSD#2].

**Acceptance.** Every repro row with run rc 101 fails `axon check` with E1211 (rc 2). It also fails `axon build`, through `run_build_pipeline` → `check_program_located` (main.rs:7821/7831). Control rows are unchanged. `spec/compiler-phase5.md` gains the per-position binding table.

### CS-2 (P1) `@[verify]` predicates are not name- or type-checked — O-S1-23

**Problem.**
- `verify::check_verify` (verify.rs:380-439) decodes only `confidence OP literal`, and returns silently for anything else.
- The checker checks a verify predicate only when the fn is sealed (checker.rs:1391-1400).
- The sealed resolver scope (resolver.rs:1555-1574) sits inside the fn scope (1537-1543), so it wrongly accepts params, which the runtime does not bind.
- The interpreter (`finish_call_cold`, interp.rs:6602-6751) binds the fields of the return value in a fresh env (6669-6682, 6736-6742). An `Uncertain` has `value`, `confidence` and `source_tag: i64` (0/1/2, `make_uncertain_tagged` interp.rs:7812-7836). A `Temporal` has **no** `source_tag` (checker field list `value|confidence|horizon_ms|decay|valid_until_ms`, checker.rs:6449-6452) [REVISED: CSA#1 — was "`source_tag: str` for Uncertain/Temporal"].
- Return refinements do bind params (interp.rs:6553-6559).

**Repro.**

| file | snippet | check | run |
|---|---|---|---|
| `s23.ax` | `@[verify(value >= lo && value <= hi)] fn clampv(x, lo, hi: i64) -> i64` | rc 0 | `axon: panic: undefined identifier \`lo\`` rc 101 |
| `s23undef.ax` | `@[verify(value >= nosuch)]` | rc 0 | `panic: undefined identifier \`nosuch\`` rc 101 |
| `s23type.ax` | `@[verify(value && true)]` on `-> i64` | rc 0 | `panic: cannot apply And to i64 / bool` rc 101 |
| `vt.ax` [REVISED: CSA#1] | `@[verify(source_tag >= 0 && value > 100)] … -> Temporal<i64>` | rc 0 | `panic: undefined identifier \`source_tag\`` rc 101 |
| `vf3.ax` [REVISED: CSA#2] | `@[verify(value >= 0 && value <= 1)]` on `-> f64` | rc 0 | `panic: cannot apply GtEq to f64 / i64` rc 101 |
| unknown callee [REVISED: CSD#11] | `@[verify(is_ok(write_file("/nonexistent/out","x")))]` | rc 0 | `panic: call to unknown function \`is_ok\`` rc 101 |
| `vf.ax`, `vf2.ax` (controls) | `@[verify(value >= 0)]` / `>= 1` on `-> f64` | rc 0 | rc 0 / `verify failed … value 0.5 >= 1 is false` rc 3 (cmp_f64 widening, interp.rs:6640-6642, 6721) |
| `s23ret.ax` (control) | `-> (i64 where _ >= lo && _ <= hi)`, result 11 | rc 0 | `refinement violated: the return value of \`clampr\` (= 11)…` rc 6 |
| `v_conf_scalar.ax` (control) | `@[verify(confidence >= 0.5)]` on `-> i64` | rc 2, E1101 | — |

**Change.** Add `Checker::check_verify_predicate(&mut self, f: &FnDef)`, called for every fn with `f.verify`,
next to checker.rs:1391-1400. The sealed path is kept, and the resolver's sealed verify scope (1555-1573) defers
unbound names to this pass so they are not reported twice [REVISED: CSA#5].

- **Scope:** module items, builtins, and these binders:
  - `value: <scalar return | inner T of Uncertain<T>/Temporal<T>>`;
  - for `Uncertain<T>`, also `confidence: f64` and `source_tag: i64`;
  - for `Temporal<T>`, also `confidence: f64`;
  - the params, only if NH-1 = (a).
  - "Wrapper inner" means Uncertain/Temporal only [REVISED: CSD#17].
- **Type rules:**
  - The predicate is constrained to `bool`.
  - An integer literal against an f64 operand is E1103 (same text as CS-1). The decodable simple shape `value|confidence OP literal` stays accepted, because runtime widening makes it correct today [REVISED: CSA#2, OQ E-6].
  - `return`/`break`/`continue` inside a verify predicate is E1103 (CS-1 rule).
- **Diagnostic E1103.** It is free: E1101 is at verify.rs:42 / error.rs:103, E1102 at error.rs:104, and `git grep E1103` is empty outside `tasks/`. Register it at the four sites listed in CS-1.
  - E1103 covers names, types and binders only. Purity is cap-surf's E1104 [REVISED: cross-spec CAPA#1].
- **Binder collision (OQ E-2), when NH-1 = (a):** a param whose name equals a binder **bound for this return shape** is E1103. The binders are `value` always; `confidence` for Uncertain/Temporal; `source_tag` for Uncertain only [REVISED: CSA#3/CSD#9 — was "any param named `value`/`confidence`/`source_tag`"].
  - `examples/flagship/agent_task.ax:38-39` (`@[verify(value >= 0)] fn score(…, confidence: i64) -> i64`) is therefore unaffected.
- **If NH-1 = (a):** the interpreter's composite path (interp.rs:6669-6682, 6736-6742) defines the params from the call env before the binders, mirroring 6553-6559.

**Behaviour that changes under NH-1** [REVISED: CSA#7/CSD#10 — replaces "tests 6254 … 9671, `[INFERENCE: which ones]`"]

Measured with `AXON_AI_MOCK=1 AXON_SEED=42 axon goal X-goal.md` on a scratch copy of `examples/goals/`:

**(a) bind params.** Three goals go from rc 0 to rc 3:

| goal | best score | target |
|---|---|---|
| `hello-goal` | 100 | 280 |
| `learn-goal` | 136 | 200 |
| `file-budget-goal` | 0 | 100 |

The same holds for `axon run examples/goals/hello-goal.ax`. I re-ran learn and file-budget: both print `deploy gate: passed`, rc 0 today.
- Unchanged:
  - `compose`, `constrained` and `optimize` reach exactly 100 vs 100;
  - `sandboxed` scores 88 vs 70;
  - `redteam` scores 91 vs 90 and is already rc 3 from its redteam gate;
  - `agent`, `flagship` and `verified` use `confidence`.
- **Re-baselined cli_run tests: 0** `[INFERENCE: assertions read, not run after the change]`.
  - :6254 is a capability refusal.
  - :6287 is 100 ≥ 100.
  - :6303 accepts 0/3/5.
  - :6412 and :9671 are confidence gates.
  - :8944-8980 runs `run.sh` with `DEMO=optimize.ax` (`examples/asi/run.sh:16`).
  - The learn tests (:8435, :8467) assert scores.
  - `--iterate` survives rc 3 (main.rs:4232-4247).
  - `replay_host_gate.sh:348` compares best-score lines only.
- **Pin sites to rewrite:**
  - `examples/goals/README.md:50-53` ("documentary … pass-through");
  - the `examples/goals/*.md` Verify sections that promise a pass;
  - hello-goal stays rc 3 permanently until CS-13 lands [REVISED: CSA#8].

**(b) refuse params.** These break:
- the axon-surface unit test `compile.rs:419` (`assert!(ax.contains("@[verify(score >= 70)]"))`);
- `examples/goals/hello-goal.ax:77`;
- `examples/goals/redteam-goal.ax:33`;
- `crates/axon-cortex/benchmarks/hardset/src/goals/redteam-goal.ax:33`, which triggers the cortex lane.

`compile.rs:245` must then emit a comment instead of the gate.

**Fail-first tests** (cli_run.rs; each is check rc 0 today):

| test | probe | after |
|---|---|---|
| `verify_predicate_unknown_name_is_refused_e1103` | s23undef | rc 2 + E1103 |
| `verify_predicate_unknown_callee_is_refused_e1103` [REVISED: CSD#11] | `is_ok(write_file(..))` | rc 2 + E1103 |
| `verify_predicate_non_bool_is_refused_e1103` | s23type | rc 2 + E1103 |
| `verify_temporal_source_tag_is_refused_e1103` [REVISED: CSA#1] | vt | rc 2 + E1103 |
| `verify_predicate_int_literal_against_f64_is_refused_e1103` [REVISED: CSA#2] | vf3 | rc 2 + E1103 |
| `verify_predicate_param_names_bind_or_refuse` | s23 | NH-1 (a): check rc 0 and run prints 5; (b): rc 2 E1103 |

Non-regression:
- `vf.ax` stays rc 0.
- `flagship_good_agent_checks_clean_and_runs` (cli_run.rs:5817) stays green [REVISED: CSA#3].
- `pipeline_parity.rs` `cases` gains s23undef [REVISED: CSD#2].

**Acceptance.** No `@[verify]` program that checks clean panics with "undefined identifier", "call to unknown function" or "cannot apply" [REVISED: CSD#11]. Goal exits match the NH-1 branch, and README.md:50-53 says so.

### CS-3 (P2) `@[verify]` is accepted but silently not enforced — O-S1-23

**Problem.** [REVISED: CSA#4 — was "scalar branch only"]

- **Scalar returns.** The simple shape is enforced only `if ident == "value"` (interp.rs:6716-6721). This contradicts its own comment at 6717-6720, which says other idents "fall through to the composite path".
- **Uncertain/Temporal returns.** The simple-shape match maps any other ident, or a non-numeric field, to `None`, and then nothing is checked (interp.rs:6636-6644).
- **Other returns.** Returns that are neither scalar nor a bare Uncertain/Temporal struct get no check at all (interp.rs:6616-6623 needs a `Value::Struct`).
- **Native.** Codegen arms only the decodable `value|confidence OP K` and "silently emit nothing" for composites (codegen/mod.rs:1531-1570, comment 1539-1540) `[INFERENCE: no fresh codegen binary]`.

**What the docs say** [REVISED: CSA#9]:
- `spec/axon-for-llms.md:238-240` documents verify as a "Runtime-checked postcondition".
- All 13 `examples/goals/*-goal.md` head the Verify block "gated at runtime".
- The one conflicting doc is `examples/goals/README.md:52-53` ("documentary … pass-through"); rewrite it with NH-1.

**Repro.**

| file | snippet | check | run |
|---|---|---|---|
| `s23simple.ax` | `@[verify(lo >= 100)] fn f(x: i64, lo: i64) -> i64 { x }`, `f(5, 0)` | rc 0 | prints `5`, rc 0 |
| `vu.ax` [REVISED: CSA#4] | the same predicate on `-> Uncertain<i64> { uncertain_dyn_i64(x, 0.5) }` | rc 0 | prints `5`, rc 0 |
| `v_str.ax` | `@[verify(value == "no")] fn f(x: str) -> str`, `f("yes")` | rc 0 | prints `yes`, rc 0 |

**Change.**

- **(a) Interp, every return shape.** A decodable simple-shape predicate goes through the composite path when either:
  - its ident is not a binder bound for that shape, or
  - the observed field is not numeric.

  It is then enforced (NH-1 = a) or already refused by CS-2 (NH-1 = b).
- **(b) Checker, in `check_verify_predicate`.** A verify on a return the runtime cannot check (not scalar, not Uncertain/Temporal) is E1103 "this `@[verify]` is never checked: `{ty}` returns are not supported" (OQ E-1).
  - Blast radius [REVISED: CSA#9/CSD#17]: `crates/axon-core/tests/fixtures/verify_ai_source.ax:6` (`@[verify(confidence >= 0.8)]` on `-> Result<Uncertain<i64>, str>`) is unenforced today, contrary to its comment at :3.
  - Rewrite it to return `Uncertain<i64>`, and re-record its `scripts/build_outcome_baseline.tsv:149` row.
  - `integration_fixtures.rs:1485-1496` stays green, because its pipeline calls `verify::check_verify` only (:87-90).
- **(c) Codegen.** Lower composite or param-reading predicates like refinement predicates (`refine_predicate_is_lowerable`, asi.rs:837-896), with `value` aliased to the return. Otherwise refuse with E0910 (asi.rs:362-440 precedent). Never drop them silently.

**Fail-first tests:**

| test | where | today | after |
|---|---|---|---|
| `verify_simple_shape_on_param_is_not_silently_skipped` | cli_run | `s23simple` and `vu`: run rc 0 | rc 3 (NH-1 a) or check rc 2 (b) |
| `verify_on_str_return_is_refused` | cli_run | `v_str`: rc 0 | check rc 2 E1103 |
| `verify_composite_predicate_is_enforced_or_refused_natively` [REVISED: CSD#2 — was in `pipeline_parity.rs`] | `crates/axon-core/tests/integration_fixtures.rs`, `#[cfg(feature = "codegen")]` | — | composite verify exits 3 natively like the interp, or E0910 at build |

Also add native parity cases to `scripts/exit_code_parity.sh` next to :373-384: a composite verify and `vu`'s shape.

**Acceptance.** No verify predicate is accepted by both `check` and `build` and then skipped by either engine.

### CS-4 (P1) Calls to a builtin-shadowing fn are typed against the user signature — O-FU-06

**Problem.** Every layer disagrees about which fn a shadowed call reaches.

- **Resolver.** It warns W0003 "the builtin takes precedence, so this function will not be called" (resolver.rs:1018-1041; AXON_REFERENCE.md:285; `governance/BUG_HUNT_2026-05-31.md:477` #33 [REVISED: CSA#10 — two rows are numbered #33]).
- **Interpreter.** It dispatches local closure → builtin → user fn (interp/eval.rs:1551-1556). There are two exceptions:
  - `P`/`E`/`Var` fall through to a user fn (eval.rs:1455-1461; CERTIFICATION.md:68-69);
  - a call with a `&mut` argument goes to the **user fn** via `call_mut_entry` (eval.rs:1787-1793) [REVISED: CSA#10].
- **Inference.**
  - It inserts builtin sigs (infer.rs:432-440).
  - Pass 1 overwrites them with placeholders (infer.rs:587-593), and pass 2 with the user sig (infer.rs:656).
  - The checker's arity check reads that table (checker.rs:5264-5303, E0305).
  - P/E/Var calls return `F64` before `fn_sigs` is read (infer.rs:1027-1030).
- **Borrow driver.** It looks up the user fn's **own** param types in `fn_sigs` by name (main.rs:7608-7620, lib.rs:1400-1409, lsp.rs:202-210).
- **Codegen.** The user fn overwrites the table entry (codegen/mod.rs:1064), and inline-lowered builtins may still win (escape.rs:265-267: "codegen may dispatch either one") `[INFERENCE: native not observed]`.

**Repro.**

| file | snippet | check | run |
|---|---|---|---|
| `fu06ln.ax` | `fn ln(a, b, c, d: i64) -> i64`; `ln(1, 2, 3, 4)` | rc 0, W0003 | `axon: panic: ln: expected 1 args, got 4` rc 101 |
| `shadow_exp.ax` (cli_run.rs:17873's program) | `fn exp(x: i64) -> i64`; `exp(1)` | rc 0 | `axon: panic: expected f64, got i64` rc 101 |
| `mutshadow.ax` (control) [REVISED: CSA#10] | `fn abs_i64(xs: &mut [i64]) -> i64`; `abs_i64(&mut a)` | W0003 | prints `8` rc 0: the user fn runs |
| `examples/algorithms.ax:45` (control) | `pow_i64`, same sig as the builtin | — | prints `1024` rc 0 |

Across the 182 examples, `algorithms.ax` is the only W0003.

**Change.** [REVISED: CSA#10 — was "keep the builtin sig at infer.rs:656"]

1. Add one shared predicate, `builtins::call_prefers_builtin(name, call_has_mut_arg) -> bool`. It is false for P/E/Var and for any call with a `&mut` argument. It is used by:
   - the resolver's W0003 (resolver.rs:1019);
   - inference;
   - eval dispatch;
   - codegen's `emit_call` and its inline-builtin arms.
2. Call-site typing uses the builtin sig when the predicate holds, and the user sig otherwise. Keep a separate `user_fn_sigs` map for user fns.
3. The fn's own param types for borrow checking and LSP come from `user_fn_sigs` (main.rs:7610, lib.rs:1400, lsp.rs:202), not from the shared table. The body is already checked from its FnDef (infer.rs:1969-1980, checker.rs:1349ff; read by CSA).
4. W0003 is not emitted for P/E/Var. Its text says "… will not be called, except with a `&mut` argument".

**Fail-first tests:**
- `call_to_shadowed_builtin_is_typed_against_the_builtin` (cli_run):
  - fu06ln → rc 2 E0305 "function `ln` takes 1 argument but 4 were supplied";
  - shadow_exp → rc 2 with a type error on arg 0;
  - both are rc 0 today.
- Extend cli_run.rs:17873 to assert the check rc.
- Non-regression: `mut_arg_call_to_shadowing_fn_still_runs_the_user_fn` (mutshadow: W0003, prints 8).
- Native [REVISED: CSD#2 — was `pipeline_parity.rs`]: an `exit_code_parity.sh` case plus an `integration_fixtures.rs` codegen test where a user `pow_i64` with the builtin's sig returns a different value. Both engines must print the builtin's result.

**Acceptance.**
- fu06ln and shadow_exp fail check.
- mutshadow and `examples/algorithms.ax` are unchanged.

### CS-5 (P2) `len(x)` with `x` still a type variable is forced to `str` — O-FU-06 + O-S1-24

**Problem.** infer.rs:1089-1094 runs `if !matches!(arg_ty, Type::Slice(_)) { constrain(arg_ty, Str) }`. An unresolved `Type::Var` (from `?` or a match-pattern binding) is therefore pinned to `str`, and later unification with `[T]` fails. The checker's twin (checker.rs:5574) only skips a concrete `Slice`. This is a false positive, not a panic.

**Repro.**
- `n_q.ax` (`let fields = parse_line(line)?`, `Ok(len(fields))`): check rc 2, `E0102 … arg 0 of \`len\` (expected str), found [str]`.
- `n_match.ax` (`Ok(q) => … len(q)`): rc 2, `found [i64]`.

**Change.** [REVISED: CSA#19 — was "the checker's `is_deferred` mechanism"] Add a post-`solve()` obligation list to `InferCtx`, modelled on `call_instantiations` (infer.rs:1185-1187, 1877-1882). For a `Var` argument, record "resolves to `str` or `[T]`" and check it after solving. `len(42)` must still error.

**Tests:**
- `len_of_question_bound_array_checks` (n_q: rc 2 → rc 0, prints `3`).
- `len_of_match_bound_array_checks` (n_match → prints `2`).
- `len_of_int_still_refused` (stays E0102).

### CS-6 (P3) A move inside an early-`return` branch poisons the fallthrough — O-FU-06 (AuthorC3 #5)

**Problem.**
- `borrow.rs:300-323` unions the moved bindings of both branches.
- `return b` moves `b` (borrow.rs:374-376), so the fallthrough path sees `b` as moved.
- The documented rule (compiler-phase3.md:697-698) is the plain union.

**Repro.** `n_ret.ax` (`if now <= b.last_tick { return b }`, then `b.tokens`): check rc 2, two `E0601 … moved value \`b\`` at lines 7 and 8.

**Change.** [REVISED: CSA#11 — was "`Return`/`Break`/`Continue` branches"]
- A branch whose block's last statement is `Return`, or a call to `exit`, contributes no moves to the union.
- `break`/`continue` branches keep their moves. Their state flows past the loop (borrow.rs:411-447), so discarding them would accept a use after a move. `brkmove2.ax` is correctly E0601 at line 12 today.
- Amend compiler-phase3.md:697-698 to match.

**Tests:**
- `early_return_move_does_not_poison_fallthrough` (rc 2 → rc 0, prints `10`).
- New guard tests [REVISED: CSD#8 — the old name "stays" did not exist]:
  - `move_in_non_diverging_branch_still_e0601`;
  - `move_before_break_still_e0601_after_loop` (brkmove2).

### CS-7 (P3, gated on NH-3) `let x = s` moves a `str`, array or `&[T]` param — O-FU-06 (AuthorC2 #1) + O-S1-24

**Repro.**

| file | param type | check |
|---|---|---|
| `m_str.ax` (`let core = s; len(core) + len(s)`) | `str` | rc 2, `borrow[E0601]: use of moved value` |
| `m_slice.ax` | `&[i64]` | rc 2, E0601 |
| `m_arr.ax` | `[i64]` | rc 2, E0601 |
| `m_struct.ax` | struct | rc 2, E0601 |
| `m_i64.ax` (control) | `i64` | rc 0 |
| `arrloc.ax` [REVISED: CSA#12] | **local** `let b = a` (array), then `len(a) + len(b)` | rc 0, prints `4` |

`is_copy` (borrow.rs:58-78) lists only numerics, bool and value handles.

**Status.** [REVISED: CSA#12 — was "`&[T]` aliasing by `let` is not addressed by any doc read"]
- `str` moving is documented: B07 lists "`str`, structs, enums, closures, and `chan<T>`" as *move* types (compiler-phase3.md:654-656; B02/B06 at 632-652).
- Arrays are documented as values:
  - language-tour.md:319: "Arrays are values: `let b = a` copies";
  - axon-for-llms.md:184: "`&[T]` is **read-only**: arrays are values".
- The borrow checker agrees for locals (`arrloc`) and disagrees for params (`m_arr`, `m_slice`).
- No code change until NH-3 is decided.

### CS-8 (P3) Docs: two gotchas — O-FU-06

[REVISED: CSA#13 — the `(` line-continuation row is withdrawn. It is a parser bug, now CS-10, not a doc gap.]

| gap | observed | doc fix |
|---|---|---|
| `parse_int` | `n_parse.ax`: `parse_int("x")` → Err `could not parse \`x\` as a base-10 integer`; `parse_int(" 3")` → `Ok(3)`. `spec/stdlib.md:204-205` says `Err("invalid integer")` and does not trim. [REVISED: CSA#18 — was "code agrees with itself"] Both engines trim before parsing. The radix hint differs: the interpreter tests the trimmed input (interp/builtins.rs:1584, 1591); axon-rt tests the raw input (lib.rs:2674), so `parse_int(" 0x1F")` gets the hint only in the interpreter `[INFERENCE: native not run]` | rewrite stdlib.md:204-205 from builtins.rs:117. Separate code fix in this item: axon-rt lib.rs:2674 trims before the hint test, and `' 0x1F'` is added to `scripts/parse_int_err_parity.sh` |
| moves on `let x = y` | CS-7 repros | gotcha in axon-for-llms.md §6: "`let x = y` moves a `str` or struct (B07); arrays copy (language-tour.md:319)". Held until NH-3 is decided [REVISED: CSA#12 — was "moves a `str`, struct or array"] |

The doc rows need no `crates/axon-core/src` change. The axon-rt fix touches `crates/axon-rt` only (no pci theme; native parity is evidenced by `--strict`).

### CS-9 (P3) Invariant test: a check-accepted fixture never hits an interpreter-internal panic

**Measured** [REVISED: CSD#4/CSA#20 — the first sweep ran without `AXON_PATH` and mis-tallied "5 other non-zero"; the categories summed to 183]

The re-run used `AXON_PATH` and `unshare -n` (DevloopChecker, twice; 32 s and 35 s wall time). Of 182 `examples/**/*.ax`: 24 have no `main`, 10 fail check, and 148 ran.
- Exit codes: 113×0, 19×1, 6×3, 3 timeouts, 2×101, and one each of 88, 6, 49, 47, 218.
- Both rc 101 are environmental (`fhir_demo` request error, `modbus_demo` network unreachable).
- **Zero internal panics.**
- Exit codes are stable across runs, but stdout is not (`asi/persistent_bandit`, `mobile/lifecycle`).
- **Measurement only, not a gate.** A gate would need `vm_parity.sh`-style isolation (:15-20) and an exclusion list: `domain/fhir_demo`, `domain/modbus_demo`, `jobs/runaway`, `mobile/lifecycle`, `r27/killable_agent`.

**Change.**
- New fixture dir `crates/axon-core/tests/fixtures/check_sound/*.ax`, holding every CS-1…CS-4 repro.
- One test in cli_run.rs, `check_clean_fixtures_never_panic_internally`. For each fixture, if check is rc 0, then run must not exit 101 with stderr matching `undefined identifier|expected \d+ args, got|cannot apply|call to unknown function|expected \w+, got`.
- [REVISED: CSD#3 — was "Examples stay covered by `all_examples_parity.sh`"] Examples and benchmarks are covered at **check level** by the 290-file sweep (§5). The run-level invariant is covered only by these fixtures. No existing harness asserts "no internal panic" over examples: `all_examples_parity.sh` is strict-only, skips subdirectories such as `goals/` and `flagship/`, counts a newly rejected example as `by_design`, and passes a 101 that happens on both engines.
- **Fixture placement** [REVISED: CSD#15]:
  - Each fixture lands in the same commit as its fix, because `vm_parity.sh` (fast tier) runs every check-clean `fixtures/**` program with `fn main`.
  - A fixture that lands earlier goes in `fixtures/vm_parity_skip.txt` with a reason.
  - Re-record `scripts/build_outcome_baseline.tsv` (`build_outcome_regression.sh --record`) when the fixtures land.

### CS-10 (P3) A field access followed by a newline and `(` parses as a method call — O-FU-06 (AuthorC3 #6) [REVISED: CSA#13, new]

**Problem.** The postfix call arm refuses a `(` on a new line (parser.rs:3008-3011, `Some(Token::LParen) if !self.preceded_by_newline()`). The `.field` branch has no such guard (parser.rs:2989 `if self.eat(&Token::LParen)`), so `b.tokens⏎(…)` becomes `MethodCall`.

**Repro.**
- `n_paren.ax`: rc 2, `E0001 cannot find name \`missing\`` + `E0403 \`tokens\` is a data field … cannot be called`.
- `paren2.ax`: `g()` (`let m = x + 1⏎(m + 1) / 2`) is fine; the same file's `f()` (`… - b.t⏎(…)`) fails the same way.

**Change.** Apply `!self.preceded_by_newline()` at parser.rs:2989.

**Test.** `field_access_then_newline_paren_is_a_new_statement` (n_paren: rc 2 → rc 0, prints `3`).

### CS-11 (P2) `return` inside a match arm or if-branch is typed `()` — O-S1-24 [REVISED: CSA#14, new]

**Problem.** The `Expr::Return` arm yields `Type::Unit` (infer.rs:1310-1320, comment "block type isn't used after it"). `break`/`continue` likewise yield `Type::Unit` (infer.rs:1714). `body_diverges` (infer.rs:160-168) patches only the tail-of-body case. `Type::Never` exists and unifies with anything (types.rs:93, infer.rs:2098).

**Repro.**
- `o24_ret.ax` (`let v = match o { Some(x) => x, None => return Err("none") }`): rc 2, `E0102 type mismatch in match arm type (expected i64), found ()`.
- `brk.ax`: rc 2, `if-else branch types (expected ()), found i64`.

**Change.** The `Return`, `Break` and `Continue` arms yield `Type::Never`. `Return` still constrains its value against `ret_ty`, as it does today.

**Tests.**
- `return_in_match_arm_unifies_with_the_arm_type` (o24_ret: rc 2 → rc 0, prints 3).
- `break_in_if_branch_unifies` (brk).
- Non-regression: `fn f() -> i64 { return "s" }` stays E0102.

### CS-12 (P3) `type Unit` (or `str`, `Option`) silently aliases a builtin type — O-S1-24 [REVISED: CSA#14, new]

**Problem.** `types.rs:86-94` resolves `"()" | "unit" | "Unit"`, `str`, `f64` and the other builtin names before any user type. A user `type Unit = { n: i64 }` is accepted, but every use means `()`.

**Repro.**
- `o24_unit.ax`: rc 2, `E0102 type mismatch in function body (expected ()), found Unit` + E0401.
- `u_str.ax` (`type str = { n: i64 }`) and `u_opt.ax` (`type Option = …`): check rc 0, silently.

**Change.** Refuse a type definition whose name resolves through `types.rs:86-94` (or is `Option`/`Result`/`Uncertain`/`Temporal`) with new **E0005** "`{name}` is a builtin type name; rename the type". E0005 is free (`git grep E0005` empty) and belongs to the name band (E0001 cannot find name, E0004 sealed). Register it at the four sites listed in CS-1.

**Test.** `type_named_like_a_builtin_is_refused_e0005` (o24_unit, u_str: rc 0/2 → rc 2 E0005).

### CS-13 (P3, gated on NH-1 = a) The goal generator takes a non-score bound as the score target [REVISED: CSA#8, new]

**Problem.**
- `extract_target_from_predicate` (crates/axon-surface/src/compile.rs:327-334) returns the first i64 token anywhere in the prose predicate.
- hello-goal.md's predicate is `coverage >= 7.0 AND faithfulness >= 8.0 AND length_chars <= 280`, so the extracted target is the length cap, 280 (compile.rs:236-249 emits `@[verify(score >= 280)]`).
- The score is on a 0–100 scale (hello-goal.ax:148-154), so under NH-1 = (a) hello-goal can never pass.

**Change.** Take the target only from a `score <op> <int>` clause. Otherwise use the existing fallback (`unwrap_or(70)`, compile.rs:237), and say so in the emitted comment. Regenerate `examples/goals/hello-goal.ax`.

**Test.** An axon-surface unit test next to compile.rs:419: the hello-goal predicate yields target 70, and `score >= 70` still yields 70. This is not a repair-digest file (`crates/axon-surface`), so no paid regress.

## 4. g01 rows, refusal coverage, pci

[REVISED: CSA#15/CSD#6/CSD#7/CSD#8 — rows split to one file and one killer each; markers; count rule]

**Row rules.** Each row is one 8-tuple in `scripts/v022_g01_mutations.py`, in the format of 11700-11712: id, description, **one** file, unique `old`, `new`, crate, target, **one** killing test.
- Each row needs one `ATTACK_MARKERS` entry in `scripts/v022_attack_markers.py` (format at :3146). The runner refuses every mutation run on drift (v022_g01_mutations.py:8818-8829).
- The killing test's assert message carries the `ATTACK:` text that the marker matches.
- `python3 -B scripts/g01_anchors.py` must report each `old` exactly once.
- Target for cli_run killers: `'--no-default-features --test cli_run'`.

| id | guard removed | file | killing test (one) |
|---|---|---|---|
| M3820 | the all-files refinement predicate scope pass | resolver.rs | `refinement_predicate_unknown_name_is_refused` |
| M3821 | binder intersection over use sites | resolver.rs | `refinement_named_by_two_different_params_is_refused` |
| M3822 | refinement predicate `bool` constraint | checker.rs | `refinement_predicate_non_bool_is_refused` |
| M3823 | int literal vs f64 refusal (refinement) | infer.rs | `refinement_predicate_int_literal_against_f64_is_refused` |
| M3824 | the call of `check_verify_predicate` | checker.rs | `verify_predicate_unknown_name_is_refused_e1103` |
| M3825 | verify `bool` constraint | checker.rs | `verify_predicate_non_bool_is_refused_e1103` |
| M3826 | per-shape binder set (no Temporal `source_tag`) | checker.rs | `verify_temporal_source_tag_is_refused_e1103` |
| M3827 | predicate frame-escape refusal | resolver.rs | `predicate_with_return_or_break_is_refused` |
| M3828 | unsupported-return verify refusal | checker.rs | `verify_on_str_return_is_refused` |
| M3829 | non-binder simple shape routed to the composite path | interp.rs | `verify_simple_shape_on_param_is_not_silently_skipped` |
| M3830 | native composite verify lower-or-E0910 | codegen/mod.rs | `verify_composite_predicate_is_enforced_or_refused_natively` (target `'--test integration_fixtures'`, default features) |
| M3831 | builtin-sig call typing (`call_prefers_builtin`) | infer.rs | `call_to_shadowed_builtin_is_typed_against_the_builtin` |
| M3832 | builtin type name refusal | resolver.rs | `type_named_like_a_builtin_is_refused_e0005` |
| M3833 | parser newline guard on `.field(` | parser.rs | `field_access_then_newline_paren_is_a_new_statement` |
| M3834 | `len` Var deferral | infer.rs | `len_of_question_bound_array_checks` [REVISED: CSD#8 — was `len_of_int_still_refused`] |
| M3835 | early-return branch discard | borrow.rs | `early_return_move_does_not_poison_fallthrough` [REVISED: CSD#8] |
| M3836 | `Return` typed `Never` | infer.rs | `return_in_match_arm_unifies_with_the_arm_type` |

M3837–M3839 are spare, for the NH-1 binder-collision row (`verify_param_named_like_a_bound_binder_is_refused`) and NH-2/NH-3 rows. M3827's refusal lives in the resolver's predicate-scope pass (CS-1 step 1), which walks both refinement and verify predicates, so its file is resolver.rs.

**Refusal coverage.** `scripts/v022_refusal_coverage.py` scans `interp/conform.rs` (SCOPE_FILES, :91-93) and the seal-edge fns (SEAL_FN, :103; language_regions :295-309). The refinement branch M3820 replaces sits in `resolve_items` (resolver.rs:1445), whose name does not match SEAL_FN, so it is outside the fn-name scan `[INFERENCE: the script's resolver.rs REGIONS spans (:1368, :1455, :2366) were not read]`. Run the script after M3820 lands, and row or exempt any new site it names (seconds).

**pci** [REVISED: CSD#6/CSD#14 — was "+1 per row on an interp file"]

- **Row count.** Every row above whose file is under `crates/axon-core/src` adds 1 to the count line at `governance/notes/v022-pci-delta.md:561`. That is all of M3820–M3836: 470 at 7f110693 (the generated block pins `44c8311c`, :19) becomes 487, plus any spare used. The line's range list gains `M3820-M3836` verbatim (pci_delta.py:355-367).
  - Edit the line in the same commit that adds the rows.
  - cap-surf's M3800–M3807 edit the same line: land serially, and re-edit after the merge.
- **Theme files.** Each `crates/axon-core/src` commit (CS-1…CS-6, CS-10…CS-12) needs its own `governance/notes/pci-themes/<hash8>.json`. One theme string may be shared, and tests and rows go in the same commit (followup-spec.md:599). Theme files and the regen are integrator-only (build-loop-followup.md:66).
- **Regen.** The integrator runs one `python3 -B tasks/r51-pci-regen.py` plus `python3 -B scripts/pci_delta.py --check` per merge batch.
  - In-lane, the `pci_delta_note` test and `--fast` report a stale note until then (pci_delta.py:421-423).
  - CS-7, CS-8 (axon-rt), CS-9 (tests only) and CS-13 (axon-surface) need no theme.

## 5. Gates and cost budget

| gate | one-run cost | re-run trigger | skip rule |
|---|---|---|---|
| targeted `cargo test -p axon-core --no-default-features --test cli_run <names>` [REVISED: CSD#12] | compile-dominated, minutes `[unmeasured]`; full cli_run ≈ 544 s | each item | never |
| check-outcome sweep: `axon check` with `AXON_PATH=examples/stdlib:examples/asi:examples/modular:examples/domain` over `git ls-files 'examples/*.ax' 'crates/axon-cortex/benchmarks/*.ax'` (290 files) [REVISED: CSD#5] | ≈ 2 s, offline, deterministic | any resolver/checker/infer/borrow/parser commit | never. Pass = each file's rc and codes are identical to the 7f110693 baseline (274 rc 0 / 16 rc 2, list in CSD#5), except files an item names |
| fresh codegen build + `AXON_HARNESS_STRICT=1 scripts/exit_code_parity.sh` [REVISED: CSD#18] | `[unmeasured]` | CS-3(c), CS-4 lanes | skip only if neither touches `codegen/` |
| g01 lane feedback: `python3 -B scripts/v022_g01_mutations.py --only=<that item's ids, comma-separated> OUT.json` [REVISED: CSD#7 — was `--only=M3820..M3830`] | per row: a test build + run + an interp rebuild; cli_run rows ≈ 4–8 min each `[INFERENCE]` | after each item's commit (clean, committed tree) | not evidence |
| `./scripts/gate.sh --fast --base=main` [REVISED: CSD#13] | ≈ 15–25 min `[INFERENCE]`, saturates the host | once per item before hand-off | never; not evidence |
| `g01_anchors.py`, `v022_refusal_coverage.py`, `pci_delta.py --check` | seconds | any src commit | never |
| g01 `--scope=all` [REVISED: CSD#13] | ≈ 35 min parallel (measured 2048–2177 s) | once at the final head | never |
| **`./scripts/gate.sh --strict`** [REVISED: CSD#1 — was the default gate] | ≈ 43–44 min (43m24s measured, gate.sh:1039; parity_all alone 21m42s) | once at the integrated head; required because CS-3(c)/CS-4 touch codegen (strict runs `parity_all.sh`, `all_examples_parity.sh`, `--test integration_fixtures`, gate.sh:999-1080) | never; the default and `--fast` gates prove nothing about I-2 |
| cortex lane `cargo test -p axon-cortex --features ai` [REVISED: CSD#16] | 10–15 min | a benchmark program's check rc/codes change, or a change in whether one of its 7 `@[verify]` programs is enforced (hardset `ad_optimizer`, `pricing`, `self_improve`, `summarize`, `goals/redteam-goal`; headroom `gradebook`, `interest`). Only `redteam-goal` changes, and only under NH-1 (either branch) | skip otherwise |
| fabric suite `check_effects` | `[unmeasured]` | CS-1/CS-2 (predicate frame-escape refusal) | never once CS-1/CS-2 land |
| paid receipt regress | ≈ $0.14 | repair-digest change | not triggered: no `crates/axon-cortex/src` or driver change |

## 6. Sequencing

```mermaid
graph LR
  NH1[NH-1 decided] --> CS2 --> CS3
  NH1 --> CS13
  NH2[NH-2 decided] --> CS1
  CS2 -. review order, error.rs serial .-> CapSurf[capability-surface CAP-1 E1104]
  CS1 --> CS9
  CS2 --> CS9
  CS4 --> CS9
  NH3[NH-3 decided] --> CS7 --> CS8
  CS5
  CS6
  CS10
  CS11
  CS12
```

[REVISED: cross-spec CAPA#1 — the CS-2 → CapSurf edge is review order only, not a code hook.]

CS-4, CS-5, CS-6, CS-10, CS-11 and CS-12 are independent of every decision and can land first. CS-1 can land with NH-2 = (b) and be relaxed later, since relaxing a refusal breaks no program. Codes are added to `error.rs` serially: E1211, E1103, E0005, then cap-surf's E1104.

## 7. Open questions

Step 2 classification. Engineering questions are decided here and are canon for the build. Needs-human questions are recommended only.

| id | question | options | decision / recommendation | class |
|---|---|---|---|---|
| NH-1 | Do `@[verify]` predicates see the fn's params? | (a) bind params, which enforces `score >= N`; (b) refuse with E1103 and change compile.rs:245 | **Recommend (a).** Return refinements already bind params (interp.rs:6553-6559). `spec/compiler-phase5.md:377-387` documents param-reading verify forms [REVISED: CSA#9]. axon-for-llms.md:238 and the goal headers say "gated at runtime". **Cost** [REVISED: CSA#7/CSD#10]: hello, learn and file-budget goals go rc 0 → 3; 0 cli_run tests re-baselined; README.md:50-53 rewritten; CS-13 is needed or hello-goal stays rc 3 [REVISED: CSA#8]. (b) costs compile.rs:419 plus 3 checked-in programs and the cortex lane. | **needs-human** (language behaviour, user-visible exits) |
| NH-2 | Does a struct-field refinement bind the field's own name? | (a) yes, like param/let (interp.rs:6203-6205, eval.rs:965-966); (b) no, `_` only, as documented (phase5.md:84-86) | **Recommend (a).** Consistent with the other named-slot positions. It is additive (only programs that panic today change), except where the field name shadows a module item. | **needs-human** (extends documented semantics) |
| NH-3 | Should `let x = y` move a `str`, array or `&[T]` param? | (a) keep current behaviour; (b) `str` copy-like; (c) `&[T]` `let` makes a read-only alias; (d) array and `&[T]` params copy on `let`, per language-tour.md:319 [REVISED: CSA#12] | **Recommend (d)** for arrays and `&[T]`: the docs say arrays are values, and locals already copy (`arrloc`). Keep `str` a move type, as B07 says explicitly; native `str` descriptor ownership was not examined. | **needs-human** (memory model) |
| E-1 | A verify on an unsupported return type | error E1103; warning; leave | **Error E1103.** [REVISED: CSA#9 — was the phase5.md:331-334 citation, which is about refinement lowering] axon-for-llms.md:238 and every goal header say "gated at runtime", so an unenforceable gate must not compile. | engineering — canon |
| E-2 | A param named like a binder (NH-1 = a) | error; the binder shadows the param; the param shadows the binder | **E1103, only for binders bound for that return shape** [REVISED: CSA#3/CSD#9] (`agent_task.ax` unaffected) | engineering — canon |
| E-3 | Codes for unbound or non-bool predicates | reuse E0001/E0102; new E1103 (verify) + E1211 (refinement) | **New codes.** The fix text is predicate-specific. [REVISED: cross-spec CAPA#1 — was "CapabilitySurface already keys on E1103"] Cap-surf relies on E1103 only for an unbound verify callee (its :135); purity is its own E1104. | engineering — canon |
| E-4 | Where the CS-9 invariant lives | extend `all_examples_parity.sh`; a new run-sweep script; a cli_run fixture test | **cli_run fixture test.** No codegen and no network needed; the examples sweep is not deterministic (CSD#4) and holds zero instances. | engineering — canon |
| E-5 | Native composite verify | lower via refinement lowering; always E0910; leave | **Lower, else E0910** (asi.rs:837-896 decides) | engineering — canon |
| E-6 | An int literal against an f64 operand in a predicate [REVISED: CSA#2, new] | refuse with E1211/E1103 and a `.0` fix; widen in the interp (and codegen); leave | **Refuse**, keeping the decodable simple shape. It matches ordinary code (`x >= 0` with `x: f64` is E0102), and widening would be a language change. | engineering — canon |
| E-7 | `return`/`break`/`continue` inside a predicate [REVISED: CSA#6, new] | refuse at check; leave to runtime containment | **Refuse at check** (E1211/E1103). Runtime containment (M59) stays as defence in depth. | engineering — canon |
| E-8 | A user type named like a builtin type [REVISED: CSA#14, new] | refuse with E0005; let the user type shadow the builtin | **Refuse.** Today the program already fails or silently means `()`; shadowing would change what existing names mean. | engineering — canon |

**Human decisions** `[DECIDED by human 2026-10-27: all three recommendations accepted]` — now canon for the build:
- **NH-1 → (a)** verify predicates bind the fn's params. Consequences in scope: hello, learn and file-budget goals change rc 0 → 3 unless CS-13 lands; README.md:50-53 is rewritten; CS-13 is in scope.
- **NH-2 → (a)** a struct-field refinement binds its own field name, like param and `let` refinements.
- **NH-3 → (d)** array and `&[T]` params copy on `let`; `str` stays a move type.
