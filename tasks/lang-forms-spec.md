# lang-forms - DRAFT spec

**Status: DRAFT (after Step 1 review). Not authorized for build.**

Four language additions the human decided on 2026-11-01 (`tasks/checker-soundness-3-spec.md:545-547`, the `[DECIDED ...]` line after
the §8 table): NH-1 `()` unit value, NH-2 `if let`, NH-3 `for (a, b) in`, NH-12 module search in the entry file's directory, searched
last. All line anchors were read at `a5310878` (branch `asi/next`). Reproductions ran with the reference binary
`/var/tmp/r51-tmp/asi-bin/axon` (`axon 0.1.0 (a5310878)`, default features, so `axon build` works) in `/var/tmp/r51-tmp/LangForms/`.
Anything I did not observe is marked `[INFERENCE]`.

## Step 1 review

Two reviews: `AuditLF` (conformance, LFA#1-11) and `DevloopLF` (dev loop and gates, LFD#1-10), plus integrator rulings 1-7.
I re-checked each finding before deciding (probe re-run with the reference binary, or the cited line re-read at a5310878). Every change
in the body is marked `[REVISED: <id>]`. **Totals: 21 findings, 21 accepted (LFD#5, LFD#7 and LFD#8 accepted with a correction,
LFD#4 accepted but moot), 0 rejected.**

| finding | claim | my check | disposition |
|---|---|---|---|
| LFA#1 | natively a `()` arm is silently skipped | re-ran u22: run rc 101 `no match arm matched`, native binary prints only `done` rc 0; u21b (`() => println("a"), _ => println("b")`): run `b\ndone`, native `done` only. Cause read: `codegen/expr.rs:609` `self.emit_expr(subject, fn_val)?` | **accepted** (ruling 2): new LF-1 step 4 (lower, or E0910 for other patterns), g01 M4661/M4676, integration_fixtures test |
| LFA#2 | "no accepted program changes output" is false | re-ran u20: check rc 0 (W0008 only), run prints `other\nb` | **accepted** (ruling 3): intended behaviour change, CHANGELOG line, u20 added to the test |
| LFA#3 | an axon-os job runs `{stem}.wrap.ax` from a staging dir, so its entry dir is not `program.parent()` | read `axon-os/src/runtime.rs:1261-1271` (wrapper written into `StagingDir`, 0700, `:244`) and `:901-962` | **accepted** (ruling 4): no sibling search for jobs; `job_module_dir_sets` unchanged; M4671 withdrawn; O-LF-03 logged |
| LFA#4 | collection-`for` span taken after the body | read `parser.rs:2100-2101` | **accepted** (ruling 5): span captured at `parse_for` entry; g01 M4662 |
| LFA#5 | the for-pattern parser moves the three-clause `for` error, so its help stops firing | re-ran c3.ax: E0000 2:10 "unexpected token: LParen, expected identifier" + "Axon has no three-clause `for` …"; read `parse_help.rs:466-472` | **accepted** (ruling 5): LF-3 keeps the help; cli_run test on real source; g01 M4663 |
| LFA#6 | the "existing test" is confirmed and will flip | `cli_run.rs:44583` `fn module_not_found_names_the_use_line` (M3989's killer); `:6641` writes a flat file in `temp_dir()` | **accepted**: LF-4 test paragraph rewritten |
| LFA#7 | anchor drift / mislabels | re-read `lib.rs:965-1000` (sealed guard), `main.rs:1022`/`:1048` (digest), `main.rs:7224-7228` (`sealed_module_dirs()` branch), `main.rs:1530-1540` (`@[contained]` import-edge cap check + `--locked`) | **accepted**: anchors fixed, row relabelled |
| LFA#8 | W0004 fires only after `Wildcard`/`Ident`, so `if let (a, b) = t` gets no warning | read `checker.rs:6641` and `pattern_irrefutable` `:7112-7123` | **accepted**, second option: the `if let` W0004 uses `pattern_irrefutable` |
| LFA#9 | fmt prints a non-`If` else through `emit_block_body` | read `fmt.rs:698-703` | **accepted**: fmt's `If` arm also takes a recognised `if let` Match |
| LFA#10 | no-else wording needs the constraint reversed | read `infer.rs:2356-2363` | **accepted**: special case spelled out |
| LFA#11 | relative entry `parent()` is `""`; group-writable not covered | `Path::new("main.ax").parent()` is `Some("")` [INFERENCE: std doc behaviour, not run] | **accepted** (ruling 6, D9): `""` → `.`, `0o020` also skipped, fail-closed |
| LFD#1 | `native_shapes_parity.sh` is run by no gate | grep of `scripts`/`governance`: only the script itself; `gate.sh:1194-1196` runs `--test integration_fixtures` under `--strict` | **accepted** (ruling 1): one integration_fixtures test per item; the script stays a dev-loop aid, not called gated |
| LFD#2 | a codegen test in `cli_run` never runs | `gate.sh:1069`, `:1528-1530` run cli_run with `--no-default-features` | **accepted**: `build_cache_key_covers_the_sibling` in integration_fixtures only |
| LFD#3 | strict-gate time stale | `gate.sh:1217`, `:1537`: 43m24s (2026-10-15). The ≈95 min figure is the integrator's brief's, not measured by me | **accepted**, with attribution |
| LFD#4 | M4671's killer is a function, axon-os lib tests ungated | `runtime.rs:749` is a fn | **accepted, moot**: M4671 withdrawn under ruling 4 |
| LFD#5 | depth-test pointer wrong; the chain test is native-only, so an interpreter twin is needed | `cli_run.rs:507` `fn else_if_chain(links, pick)` confirmed. But `native_else_if_chain_is_bounded_ax67` (`:516-596`) is a plain `#[test]` with no `cfg(feature = "codegen")`, driving `axon run`/`axon check`/`axon-run`, so it already runs under `--no-default-features` | **accepted with correction**: a sibling `else_if_let_chain_is_bounded` (new generator) is M4647's killer; no separate twin needed |
| LFD#6 | only `axon`-fenced blocks in the `DOCS` files are checked | `doc_snippets.rs:254` (`b.fence != "axon"` → error), `:33-40` | **accepted** |
| LFD#7 | `AXON_REFERENCE.md` is generated; lane C owns the row text | `cmd_reference` `main.rs:5329` reads `env_registry::ALL_ENV_VARS` (`main.rs:5356`); the row text is `env_registry.rs:33`, not `main.rs` | **accepted with correction**: lane C edits `env_registry.rs:33` and regenerates in the same commit |
| LFD#8 | under EXCLUSIVE the diagnostic is E0003, not E0901 | re-ran the spec's m1 shape (`mod mathutil` + `use`): `env -u AXON_PATH AXON_PATH_EXCLUSIVE=1 axon run mx/main.ax` → only E0001 "cannot find name `square`", rc 2 (no E0901, no E0003). LFD's own `m/main.ax` has a bare `use` with no `mod` → E0003 + E0001, rc 2 | **accepted with correction**: assertion is rc ≠ 0, stdout lacks `16`, an E0001 for `square`; the missing module-not-found line is logged as O-LF-04 |
| LFD#9 | several "fail-first" tests pass today | consistent with today's resolver (no entry-dir search) | **accepted**: relabelled "guards (pass today)"; the world-writable test's skip line is its fail-first assertion |
| LFD#10 | sweep depends on untracked scratch files | `/var/tmp/r51-tmp/CS2Draft/cs-sweep-all.sh` is untracked | **accepted**: dev-loop only, not a gate stage |

## 0. Source entries

- `tasks/checker-soundness-3-spec.md` §8 NH-1, NH-2, NH-3, NH-12 and the DECIDED line (`:545-547`).
- `tasks/opportunities.md` O-GAP-08 class (c) ("Common forms are refused: `if let`, `loop`, `?` on Option, inherent `impl`, tuple
  variants, `..base`, `Ok(())`"); this spec builds the `if let`, `Ok(())` and `for (a, b)` parts.
- `tasks/gaps-spec.md:200` (NH1, "search the entry file's directory for modules", needs-human, not built) and
  `tasks/approval-residue-spec.md:271` (H1, the same item, excluded 2026-10-18). The 2026-11-01 decision supersedes both: build it,
  searched LAST, never for a world-writable staging directory.
- `spec/axon-for-llms.md:251-253` gotcha #10 (the sibling-module trap), removed by LF-4.

### Out of scope (logged, not dropped)

| item | why not here | where it goes |
|---|---|---|
| NH-5 (`==` on composite values natively) | human: waits until native lowers composite equality | stays in cs3 §8 |
| NH-4 (inherent `impl T { }`) | human: deferred | stays in cs3 §8 |
| NH-6..NH-11, NH-13 | human: declined | cs3 §8 |
| **Tuple patterns are unchecked in `match`** (found here). q01 `match (1, 2, 3) { (a, b) => .. }`, q02 `match 5 { (a, b) => .. }`: `axon check` rc 0, `axon run` rc 101 `axon: panic: no match arm matched`. q05 `match (1, "s") { (a, b) => to_str(b + 1) }`: check rc 0, run rc 101 `axon: panic: cannot apply Add to str / i64` (`infer.rs:2975-2978` binds tuple-pattern components with no subject type) | a checker soundness defect in `match`, independent of the new forms; `if let (a, b) = t` inherits it (§2 LF-2) and LF-3 deliberately avoids it | new opportunity **O-LF-01** (silent acceptance → runtime panic), for the next checker-soundness batch |
| **Fewer names than tuple elements is accepted** (q07: `let (a, b) = p` with `p: (i64, i64, i64)`, check rc 0, prints `3`) | existing `let` semantics (`parser.rs:2350-2387` emits one `.i` per name); LF-3 inherits it and says so | **O-LF-02** |
| **Native `Result<(), E>`** (d01, u13: `Ok({})` in a `-> Result<(), str>` fn) builds to E0910 ("could not lower the body of `g` to a value") or `IR verification failed: Function return type does not match operand type of return inst!` | pre-existing native gap (O-NP-01 class); LF-1 is parse-only, so `Ok(())` behaves exactly like today's `Ok({})` natively | O-NP-01 (comment: `Ok(())` now reachable) |
| Native `arr_enumerate` on `[str]` and `str + str` (d03) | pre-existing E0910 refusals, not wrong answers | O-NP-01 |
| `let`-chains (`if let P = e && c`), or-patterns in `if let` (`if let A \| B = e`), let-else (`let P = e else { }`). (Mixing `if let` and plain `if` in one `else if` chain IS in scope, LF-2.) | not requested | not logged as wanted |
| Iterating a dict directly (`for (k, v) in d`) | design decision LF-3 D4: dicts stay non-iterable; the diagnostic is fixed instead | - |
| `[REVISED: LFA#3, ruling 4]` **axon-os jobs get no sibling search.** A job runs a wrapper `{stem}.wrap.ax` written into a fresh 0700 `StagingDir` (`axon-os/src/runtime.rs:244-273`, `:1261-1271`), so the child's entry dir is that staging dir (it holds only the wrapper [INFERENCE: nothing else is written there]), not `program.parent()`; `job_module_dir_sets` (`:749-770`) stays unchanged. Known consequence: `query_imports_digest` (`:969-978`) runs `axon imports <program>`, whose entry dir IS `program.parent()`, so after LF-4 an approval can bind a closure that includes a sibling module the job's run then fails to find (E0901). That fails closed (the run refuses); it is not a hole [INFERENCE: not reproduced, needs an axon-os job run] | not requested; the fix needs one entry-dir definition shared by `run` and `imports` | **O-LF-03** opportunity: pass the entry dir to jobs (e.g. a hidden `--entry-dir` given to both `run` and `imports` by `job_child_command`), then append it last in `job_module_dir_sets` |
| `[REVISED: LFD#8]` **No module-not-found line for a missing `mod` under `AXON_PATH_EXCLUSIVE=1` with an empty search list** (found while checking LFD#8): `mod mathutil` + `use mathutil.{square}` → only E0001 "cannot find name `square`", rc 2; a bare `use` without `mod` gives E0003 "module `mathutil` not found". Without EXCLUSIVE the same file gives E0901 at the `mod` line | pre-existing diagnostic gap; fails closed | **O-LF-04** |

## 1. Priority tiers

| tier | item | why |
|---|---|---|
| P1 | LF-1 `()` unit value, incl. the `()` pattern fix and the native unit-subject `match` fix | most common refused form (`Ok(())`); the `()` pattern is a check-passes/run-panics hole today (u06), and natively a `()` arm is silently skipped (u22) `[REVISED: LFA#1]` |
| P1 | LF-3 `for (a, b) in xs` | second most common; fixes the `let (a, b)` dummy span and `axon fmt`'s `__tup_` leak on the way |
| P1 | LF-2 `if let` / `else if let` | the largest parser change; desugar with a recognisable catch-all |
| P2 | LF-4 entry-directory module search, last | changes module resolution (security-relevant); four resolvers must agree |
| P2 | LF-3b dict `for` diagnostic | today's message blames `len` (p23) |
| P3 | LF-5 docs and CS3-5 help rows | must land with LF-1..LF-4 (the old help would misdirect) |

## 2. Items

Shared design principle (decided, §7 D1): **every new surface form is a parse-time desugaring into AST nodes that already exist.** No new
`Expr`/`Pattern` variant, so the checker, infer, interpreter (tree and VM), native codegen, wasm, effects/capabilities walks and the
AX-10 JSON serde see only shapes they handle today. The cost is paid in two places: diagnostics must still point at the user's text
(spans), and `axon fmt` (a pure AST printer, `crates/axon-core/src/fmt.rs:11`) must recognise each desugaring and print the sugar
back. Desugared code is recognised only by synthetic name prefixes (existing convention: `__forarr_`/`__fori_`, `fmt.rs:1097-1136`
`as_desugared_foreach`; `__tup_`, `parser.rs:31-46` `push_stmt_or_splice_destructure`). This spec adds the prefixes `__iflet_` and
`__forpat_`. A user identifier can already start with `__`; the convention's existing collision risk is unchanged (§7 D6).

---

### LF-1 (P1) `()` is the unit value

**Today.** `spec/grammar.ebnf:484-488` names `UNIT ::= '(' ')'` and says it is "currently parsed as a grouped expression `( expr )`
where the inner expression is absent; in practice the unit value is represented as an empty block or by the Named type "()" and does not
appear as a standalone literal in the current parser." The type `()` parses (`grammar.ebnf:167`); the pattern `()` parses
(`parser.rs:3674-3679`, `Pattern::Tuple(vec![])`); the expression does not: `parse_primary_inner`'s `LParen` arm
(`parser.rs:3341-3365`) checks `is_paren_lambda` (`:583`), then advances and calls `parse_expr`, which sees `)`.

Reproductions (`/var/tmp/r51-tmp/LangForms/recheck.txt`):

```
=== u01_ok_unit.ax
fn run() -> Result<(), str> {
    println("hi")
    Ok(())
}
fn main() { match run() { Ok(_) => println("ok"), Err(e) => println(e) } }
--- check   rc 2
{"schema":"axon-diag/1","severity":"error","code":"E0000","file":"u01_ok_unit.ax","line":3,"col":9,"message":"unexpected token: RParen, expected expression","help":"Axon has no `()` value — the unit value is an empty block: `Ok({})`, `return {}`, `let u: () = {}`"}
```

`u02` `let u = ()` → same E0000 at 2:14; `u03` `return Ok(())` at 3:16; `u04` `let r: Result<(), str> = Ok(())` at 2:34. All rc 2
for both `check` and `run`.

**The `()` pattern is a hole today** (check passes, run panics):

```
=== u06_unit_pattern.ax
fn f() -> Result<(), str> { Ok({}) }
fn main() {
    match f() { Ok(()) => println("unit"), Err(e) => println(e) }
}
--- check  rc 0
--- run    axon: panic: no match arm matched   rc 101
```

Also u11 (`let u: () = k()` then `match u { () => .. }`) and p17 (`let u: () = {}` then `match u { () => .. }`): check rc 0, run rc
101, same panic. Cause: the interpreter's `Pattern::Tuple` arm (`interp/eval.rs:2920-2934`) requires `Value::Tuple`; the unit value is
`Value::Unit` (`interp/value.rs:907`, `(Unit, Unit) => return true`). The checker treats `Pattern::Tuple(vec![])` as irrefutable
(`checker.rs:7115`), so it is never refused.

`[REVISED: LFA#1]` **Natively a `()` arm is silently skipped** (the first version said "native already matches `()`"; wrong). The
pattern test itself returns `true` (`codegen/match_pat.rs:462-466`, vacuous for `()`), but the `Match` arm of `emit_expr`
(`codegen/expr.rs:607-619`) evaluates the subject with `let subj_val = self.emit_expr(subject, fn_val)?;` (`:609`). A unit-typed
subject lowers to no value, so the `?` leaves the whole `match` with no codegen error recorded: no arm runs and the build succeeds
[INFERENCE for the mechanism; the effect is observed]:

```
=== u22.ax                                            === u21b.ax (AuditLF's u21.ax)
fn k() -> () { {} }                                   fn k() -> () { {} }
fn main() {                                           fn main() {
    match k() { () => println("unit") }                   match k() { () => println("a"), _ => println("b") }
    println("done")                                       println("done")
}                                                     }
check rc 0                                            check rc 0
run:    axon: panic: no match arm matched  rc 101     run:    b\ndone  rc 0
native: done  rc 0  (the arm is skipped)              native: done  rc 0  (neither arm runs)
```

As a value (u21: `let s = match k() { () => "a", _ => "b" }`, `println(s)`, `println("done")`): run `b\ndone`; `axon build` refuses
with E0910 "`s` is read, but native codegen produced no value for its binding" + "argument 1 of the call to `println` could not be
lowered by native codegen". So the silent skip hits statement-position matches only.

Native today for the unit-value shapes (d07, built and run): `fn h(n: i64) -> () { if n > 0 { println("pos") return {} } ... }`,
`fn k() -> () { {} }`, `let _u: () = {}` → `axon run` and the native binary both print `pos\nnonpos\ndone`, rc 0.

**Fix design (decided).**
1. Parser, `parse_primary_inner` `LParen` arm (`parser.rs:3341`): after the `is_paren_lambda` check (unchanged, so `() => 7` stays a
   lambda; p18 runs today and prints `7`), if the token after `(` is `)`, consume both and return `Expr::Block(vec![])` - the same node
   `{}` builds. No `paren_depth` bookkeeping is needed (nothing is parsed inside).
2. Interpreter, `interp/eval.rs:2920`: `Pattern::Tuple(pats)` with `pats.is_empty()` matches `Value::Unit` (and still matches an empty
   `Value::Tuple` should one ever exist). The VM uses the same `match_pattern` (`interp/vm/mod.rs:2391`), so it is fixed by the same
   line. `[REVISED: LFA#2, ruling 3]` This is a soundness fix **and an intended behaviour change**. (a) Programs that pass `check`
   and use a `()` pattern with no other arm that matches (u06, u11, p17, u22) panic today and run after. (b) Programs that pass
   `check` and put a `()` pattern next to a fallback arm change from the fallback arm to the `()` arm. Observed today (u20):

   ```
   fn f() -> Result<(), str> { Ok({}) }
   fn main() {
       match f() { Ok(()) => println("unit"), _ => println("other") }
       let u: () = {}
       match u { () => println("a"), _ => println("b") }
   }
   check rc 0 (one W0008, the `= {}`); run prints `other\nb`. After the fix: `unit\na`.
   ```

   The `_` arms get no W0004 because `checker.rs:6641` counts only `Wildcard`/`Ident` as catch-alls. A `while let () = f() { .. }`
   loop changes from exiting at once to looping [INFERENCE: not run]. Tracked corpus: a grep of `**/*.ax` for `() =>`, `()) =>`,
   `while let ()`, `if let ()` at a5310878 finds only zero-parameter lambdas (`closures.ax:17`, `phase15_higher_order.ax:53`) and
   `select` arms, no `()` pattern, so no tracked program changes. CHANGELOG states the change (LF-5).
3. Grammar: `UNIT` becomes a real primary: `primary ::= ... | '(' ')'` with the note "the unit value; the same value as an empty
   block `{}`".
4. `[REVISED: LFA#1, ruling 2]` **Native, `codegen/expr.rs:607-619`.** Replace the `?` at `:609`. Snapshot
   `let errors_before = self.codegen_errors.len();` (the existing "did it refuse?" idiom, `codegen/mod.rs:386`, used at
   `expr.rs:11295`/`:11345`), then call `self.emit_expr(subject, fn_val)`:
   - `Some(v)`: unchanged.
   - `None` and `self.codegen_errors.len() > errors_before`: return `None` (today's behaviour; the refusal is already reported).
   - `None` with no new error (a value-less, i.e. unit, subject) and every arm's pattern is `Pattern::Tuple(vec![])` or
     `Pattern::Wildcard`: call `emit_match` with the placeholder subject `self.ir.context.i8_type().const_zero().into()`. Both patterns
     test `true` without reading the subject (`match_pat.rs:197`, `:462-466`) and bind nothing, so the placeholder is never read; the
     first arm whose guard holds runs, as in the interpreter.
   - `None` with no new error and any other arm pattern (a binding, a literal, an enum pattern, LF-2's `__iflet_N` catch-all): record
     `codegen error [E0910]: native codegen cannot match on a value-less (unit) subject with this pattern. The interpreter supports
     it; run under `axon run`.` and return `None`.

   Never a silent skip. After the fix u22 prints `unit\ndone`, u21b `a\ndone` and u21 `a\ndone` on both engines.
   Alternatives considered: (i) refuse every value-less subject with E0910: smaller, but refuses `match k() { () => .. }`, the
   spelling LF-1 makes natural, and the lowering is about 15 lines. (ii) Bind the placeholder for binding patterns too: rejected,
   because a bound unit read natively would print the placeholder byte where the interpreter prints `()` (a silent wrong answer).
   `if let () = k() { .. }` (LF-2) is therefore an E0910 natively; it also gets the irrefutable-`if let` W0004.

Alternatives considered:
- *A new `Literal::Unit`.* Rejected: `Literal` is matched in about a dozen files (checker, infer, interp, vm compile, codegen expr/escape,
  complexity, borrow, fmt, rewrite_dsl, verify); every one would need a unit case, the AX-10 JSON shape gains a variant, and
  native/interp would have two unit spellings to keep in parity. The empty block is already the one unit value every consumer handles.
- *Lower the pattern `()` to `_` at parse time.* Rejected: it would also accept `()` against a non-unit subject silently. Keeping
  `Pattern::Tuple(vec![])` keeps today's type behaviour and fixes only the runtime.

**What each consumer sees.** Checker/infer, interpreter, VM, codegen, wasm, effects: an `Expr::Block(vec![])`, as for `{}`; no
change. W0008 (`lib.rs:1787-1821`, a source-text scan for `= {}`) does not fire on `= ()` (it scans for `{`), which is correct: `()`
cannot be mistaken for a dict literal. Spans: the statement span is the user's statement (`Expr` carries no span; diagnostics use the
statement span). AX-10 JSON (`axon parse --json`): `()` serializes as an empty `Block`; documented, no schema change.

**`axon fmt` (decided, §7 D3).** fmt cannot tell `()` from `{}` (same node). It prints the empty block in **value position**
(let value, `return` value, a call or `Ok`/`Err`/`Some` argument) as `()`, and keeps `{}` in **body position** (fn body, `if`/`else`
branch, loop body, match arm body). Today fmt prints `let u: () = {}` (observed, fm02b), which is exactly what W0008 warns about; after
the change fmt never produces a W0008. Tracked corpus impact: `git grep` at a5310878 finds `Ok({})` in 0 `.ax` files (4 lines in `.md`),
`return {}` in 0 `.ax`, a line ending `= {}` in 0 `.ax`; `=> {}` (body position, unchanged) in 10 `.ax` lines. So no tracked `.ax`
file's fmt output changes [INFERENCE: fmt is run on .ax files only, `cli_run.rs` fmt tests use inline sources].
Alternative: keep printing `{}` everywhere. Rejected because fmt would then turn `let u = ()` into a W0008 warning.

Native: `()` is the same AST as `{}`, so native behaviour for the unit *value* equals today's `{}` behaviour exactly: d07-shapes build
and run; `Result<(), E>` stays a pre-existing native refusal (§0). Matching on a unit subject is fixed by step 4 `[REVISED: LFA#1]`.

**Fail-first tests** (new `cli_run.rs` block `// lang-forms LF-1`, inline sources):
- `unit_value_paren_is_the_empty_block`: u01, u02, u03, u04 → check rc 0; u01 run prints `hi\nok`; `let u = ()` then
  `println(if u == {} { "same" } else { "diff" })` [INFERENCE: `==` on unit works in the interpreter, `values_equal` `(Unit, Unit)`] prints
  `same`. ATTACK marker: `ATTACK: Ok(()) was refused at parse`.
- `unit_pattern_matches_the_unit_value`: u06, p17 → run rc 0, prints `unit`; `[REVISED: LFA#2]` u20 → prints `unit\na` (today
  `other\nb`). ATTACK: `ATTACK: a () pattern passed check and panicked`.
- Guard (passes today) `[REVISED: LFD#9]`: `paren_lambda_with_no_params_stays_a_lambda`: p18 `let f = () => 7` prints `7` (observed
  today); `let g = ()` then `g` used as a value → no E0000 (this half fails today). Kills M4641.
- fmt: `fmt_unit_value_prints_parens` in `fmt.rs` tests: `let u: () = ()`, `return ()`, `Ok(())`, `match x { _ => {} }`, `if c {}` round
  trip idempotent, value positions print `()`, body positions print `{}`.
- `[REVISED: LFD#6]` `doc_snippets` checks the new doc examples only where they sit in a `DOCS` file (`tests/doc_snippets.rs:33-40`)
  inside a block fenced exactly `axon` (`:254`).
- `[REVISED: LFD#1, LFA#1, ruling 1-2]` Native (gated): new `tests/integration_fixtures.rs` test
  `unit_match_runs_the_unit_arm_on_both_engines`, `#[cfg(feature = "codegen")]`, written like
  `shadowed_builtin_call_reaches_the_builtin_on_both_engines` (`integration_fixtures.rs:2412-2457`: write the source into a
  `temp_dir()` subdir, `axon run`, `axon build -o … --no-cache`, run the binary, compare stdout). Programs and required stdout on
  BOTH engines: u22 → `unit\ndone` (today run rc 101, native `done`); u21b → `a\ndone` (today run `b\ndone`, native `done`); u21 → `a\ndone`
  (today native E0910); d07 in `()` spelling (`return ()`, `fn k() -> () { () }`, `let _u: () = ()`) → `pos\nnonpos\ndone`
  [INFERENCE: same AST as the observed d07]. Assert messages carry `ATTACK: a () arm was skipped natively`.
- Native refusal (gated, same file): `unit_match_with_a_binding_arm_is_refused_natively`. u23: `fn k() -> () { {} }` +
  `match k() { u => println("x") }` + `println("done")`. Observed today: check rc 0; `axon run` prints `x\ndone`; `axon build` succeeds
  and the binary prints only `done`, rc 0 (the same silent skip). Required: `axon run` unchanged; `axon build` exits non-zero with
  `E0910` and "value-less (unit) subject" on stderr. This turns a native build that succeeds today with a wrong answer into a
  refusal; it is not a false refusal (the program never ran correctly natively). ATTACK: `ATTACK: a unit match arm was skipped
  natively without a refusal`.
- Dev-loop aid only (not gated): an `lf1_unit` case in `scripts/native_shapes_parity.sh` may mirror the first test.

**Acceptance.** u01-u04, u06, p17, u20, u22 pass check and run as stated; p18 unchanged; fmt rule as above; both integration_fixtures
tests green under `gate.sh --strict` (`:1196`); CS3-5 unit row and its probe changed (LF-5).

---

### LF-2 (P1) `if let PAT = EXPR { A } else { B }`

**Today.** `parse_if` (`parser.rs:2518-2540`) expects `If`, then `parse_logical()`, which sees `let`.

```
=== i01_if_let.ax
fn main() {
    let o = Some(3)
    if let Some(v) = o { println(to_str(v)) }
}
check rc 2 / run rc 2:
{"schema":"axon-diag/1","severity":"error","code":"E0000","file":"i01_if_let.ax","line":3,"col":8,"message":"unexpected token: Let, expected expression","help":"Axon has no `if let` — write `match o { Some(v) => …, None => … }` (`while let` exists)"}
=== i03_else_if_let.ax
    if false { println("a") } else if let Some(v) = o { println(to_str(v)) }
→ the same E0000 at 3:39, rc 2.
```

`while let` exists (`ast.rs:378` `Expr::WhileLet`, parsed at `parser.rs:2015-2031` with `parse_pattern` then `parse_logical`); i04 runs
today (`1\n2`).

**Desugaring (decided).** In `parse_if`, after `expect(If)`, if `eat(Let)`:

```
if let PAT = EXPR { A }                 =>  match EXPR { PAT => { A }, __iflet_N => {} }
if let PAT = EXPR { A } else { B }      =>  match EXPR { PAT => { A }, __iflet_N => { B } }
if let PAT = EXPR { A } else if ...     =>  match EXPR { PAT => { A }, __iflet_N => <the parsed else-if expr> }
```

`PAT` via `parse_pattern()` (the full pattern grammar `match` uses, incl. or-patterns? **no**: `parse_match` splits `|` itself at
`parser.rs:2466-2474`; `if let A | B = e` is out of scope, a `|` after the pattern gives today's parse error). `EXPR` via
`parse_logical()`, the same entry `while let` uses (`parser.rs:2021`), so struct-literal / `{` ambiguity is resolved the same way as
`while let` and `if`. `N` is `self.pos` (unique per parse, as `__tup_{}` uses at `parser.rs:2364`). The catch-all arm binds a name
rather than `_` so that fmt can tell an `if let` from a user-written `match ... _ =>` (D2). `else if` / `else if let` chains recurse
through `self.nested(Self::parse_if)` exactly as today (`parser.rs:2531`), so the AX-60/AX-67 depth charge per chain link is kept.
Because `parse_if` is also reached from `parse_expr_inner` (`parser.rs:1950`) and `parse_primary_inner` (`:3340`), `if let` works as a
statement and as a value (`let x = if let Some(v) = o { v } else { 0 }`).

Verified today, the desugared shape (z01, `/var/tmp/r51-tmp/LangForms/z01_iflet_bind.ax`, catch-all named `__iflet_k`):

```
fn get(o: Option<i64>) -> i64 { match o { Some(v) => v, __iflet_1 => 0 } }
fn main() {
    let o = Some(3)
    match o { Some(v) => { println(to_str(v)) }, __iflet_2 => {} }
    let s = Some("abc")
    let n = match s { Some(t) => len(t), __iflet_3 => 0 }
    println(to_str(n))
    match s { Some(t) => { println(t) }, __iflet_4 => {} }
    println(to_str(get(Some(4)) + get(None)))
    let x = 5
    let r = match x { y => y + 1, __iflet_5 => 0 }
    println(to_str(r))
}
```

`axon check` rc 0 with one warning, `{"code":"W0004",...,"message":"unreachable match arm: an earlier `_`/binding arm already covers
every value","help":"remove this arm, or move it above the catch-all"}` (the irrefutable `y` arm; note: no line/col in the diagnostic).
`axon run` and the native binary (`axon build`, rc 0) both print `3\n3\nabc\n4\n6`. No W0006 for the unused `__iflet_k` (leading `_`), no
use-after-move on `s` after matching it twice. Earlier native probes d02/d09 (same shape with `_`) also matched the interpreter.

**Decisions.**
- *No-else form value type is unit*, as `if` without `else` (`infer.rs:2378-2381`). A non-unit then-body is E0102. Today's wording for
  the desugared form is the match-arm one, observed on the hand-written equivalent (p26: `match o { Some(v) => v, _ => {} }` → E0102
  3:5 "type mismatch in match arm type (expected i64), found ()"), while `if c { 5 }` gives (p25) E0102 3:12 "type mismatch in
  if-without-else body (expected ()), found i64". **Decided:** the if-without-else wording, "type mismatch in `if let` without `else`
  (expected ()), found i64". Alternative (accept the match-arm wording) rejected: it names a construct the user did not write.
  `[REVISED: LFA#10]` How: the generic loop (`infer.rs:2358-2361`) calls `self.constrain(arm_ty, first_ty.clone(), "match arm type")`,
  i.e. it constrains the unit second arm to the first arm's type, which yields "(expected i64), found ()". So the `Expr::Match` arm
  (`infer.rs:2347-2364`) gets a special case before that loop: when `arms.len() == 2`, `arms[1].guard.is_none()`,
  `arms[1].pattern` is `Pattern::Ident(n)` with `n.starts_with("__iflet_")` and `arms[1].body` is `Expr::Block(b)` with `b.is_empty()`,
  infer both arms as today (same `cond_depth` bracketing), then call `self.constrain(first_ty, Type::Unit, "`if let` without `else`
  body")` (the reversed order, exactly as `If` does at `:2380`) and return `Type::Unit`. Every other match takes the generic loop
  unchanged. `if let P = e { A } else {}` is the same AST, so it gets the same rule; that is the right type rule for it too.
- *Bindings scope:* `PAT`'s bindings are in scope in `A` only (they are match-arm bindings). Not in `B`, not after the statement.
- *Exhaustiveness:* the catch-all arm makes every `if let` exhaustive; E0304 never fires on it.
- *Irrefutable `PAT`* (`if let x = e`, `if let _ = e`, `if let (a, b) = t`, `if let () = u`). **Decided:** the checker warns with
  "irrefutable `if let`: the pattern always matches, so the `else` branch never runs" and help "write `let PAT = EXPR` (a tuple
  pattern) or drop the `if let`". A warning, not an error: Rust warns too, and refusing would be a false refusal of a program that
  runs. `[REVISED: LFA#8]` Today's W0004 fires only after a `Wildcard`/`Ident` arm (`checker.rs:6641`), so a tuple `PAT` would get no
  warning (AuditLF's w4.ax, `match t { (a, b) => …, __iflet_3 => {} }`, checks clean). So the rule is a pre-check placed before the
  generic W0004 loop (`checker.rs:6617-6659`): if the match is an `if let` desugaring (2 arms, the second unguarded
  `Pattern::Ident(n)` with `n.starts_with("__iflet_")`) and arm 0 is unguarded and `self.pattern_irrefutable(&arms[0].pattern)`
  (`checker.rs:7112-7123`: binding, `_`, tuple of irrefutables incl. `()`, known-struct destructure), push the `if let` W0004 and
  skip the generic loop for this match (its only possible finding would be the same arm). Otherwise the generic loop runs unchanged.
- *Tuple patterns in `if let`* inherit `match`'s unchecked tuple patterns (O-LF-01): `if let (a, b) = t` with `t: (i64, i64, i64)`
  will panic at run like q01. Not fixed here; the spec's docs do not show tuple patterns in `if let` examples.
- *`if let ... && cond` / let-chains:* not supported. `EXPR` is `parse_logical()`, so `if let Some(x) = o && c { }` parses `o && c` as
  the subject and is refused by infer (E0102-class) [INFERENCE: exact code]. Documented in the gotchas.

Alternatives considered:
- *New `Expr::IfLet` node.* Rejected: about 30 consumer files match on `Expr` (the `WhileLet` grep alone lists checker, borrow,
  capabilities, codegen expr/escape, complexity, interp, vm compile, effects ...), and native would need new lowering; the desugaring
  reuses `match`, which native already lowers (verified above).
- *Catch-all `_`.* Rejected: fmt could then only resugar by shape, and would rewrite a user's two-arm `match x { P => a, _ => b }`
  into `if let`, a visible rewrite of code the user wrote.
- *Bind the subject first (`ref __iflet_N = EXPR`).* Not needed: `match` already evaluates the subject once, and z01 shows no move
  problem.

**Consumers.** Checker/infer/borrow/effects/capabilities/interp/VM/codegen/wasm: a `Match` with a binding catch-all, already handled
(z01). Spans: the statement span is the `if` (user text). W0004 has no line/col today (z01), which is pre-existing; not changed here.
LSP: hover/definition operate on the desugared AST; the builder checks that completions do not offer `__iflet_*` names
[INFERENCE: whether lsp.rs filters `__` locals today was not confirmed - `grep '__' lsp.rs` finds no filter; if completions list
match-arm bindings, add `!name.starts_with("__")` at the completion source]. AX-10 JSON: a `Match`; no schema change.

**`axon fmt` (decided).** Add `as_desugared_if_let(arms)` next to `as_desugared_foreach` (`fmt.rs:1097`): a `Match` with exactly two
arms, the second unguarded with `Pattern::Ident(n)` and `n.starts_with("__iflet_")`. Print `if let PAT = SUBJ { A }`; if the second
body is an empty block, no `else`; if it is an `If` or another desugared `if let`, print `else ` + that expression (chain); otherwise
`else { B }`. `if let P = e { A } else {}` normalizes to the no-else form (equivalent). The generic `Match` arm (`fmt.rs:726-745`)
otherwise unchanged. `[REVISED: LFA#9]` The plain-`if` side of a mixed chain needs a change too: fmt's `If` arm (`fmt.rs:698-703`)
prints `else` + `emit_expr(e)` only for `Expr::If` and sends anything else through `emit_block_body`, so `if c { .. } else if let
.. { .. }` would print as `else { if let .. }`. The match at `:700-703` gains a second chain case: an `Expr::Match` for which
`as_desugared_if_let` returns `Some` is printed with `emit_expr` like `Expr::If`.

**Fail-first tests** (`cli_run.rs` block `// lang-forms LF-2`):
- `if_let_runs`: i01 prints `3`; i02 (`fn get(o: Option<i64>) -> i64 { if let Some(v) = o { v } else { 0 } }`) prints `4\n0` for
  `get(Some(4))`, `get(None)`; i03 prints `3`; a 3-link chain `if let Some(v) = a { .. } else if let Ok(w) = r { .. } else if c { .. }
  else { .. }` takes each branch once across four calls. ATTACK: `ATTACK: if let was refused at parse`.
- `if_let_binding_is_scoped_to_the_then_block`: `if let Some(v) = o { } println(to_str(v))` → E0001 "cannot find name `v`".
- `if_let_without_else_must_be_unit`: `if let Some(v) = o { v }` → E0102 with the new wording, line of the `if`.
- `irrefutable_if_let_warns`: `if let x = 5 { .. } else { .. }` → rc 0, W0004 with the new wording; `[REVISED: LFA#8]` also
  `let t = (1, 2)` + `if let (a, b) = t { .. } else { .. }` → rc 0, the same W0004 (no warning at all today on the desugared twin, w4.ax).
- `if_let_value_and_statement_forms_match_their_match_twins`: for each case, the `match` twin prints the same output.
- fmt: `fmt_if_let_round_trips` (no-else, else, else-if-let chain, else-if chain, `[REVISED: LFA#9]` and a mixed chain
  `if c { .. } else if let Some(v) = o { .. } else { .. }` that must print `else if let`, not `else { if let`; idempotent; output
  contains `if let`, not `__iflet_`); `fmt_user_match_with_wildcard_is_not_resugared`.
- Depth `[REVISED: LFD#5, with correction]`: the chain test is `native_else_if_chain_is_bounded_ax67` (`cli_run.rs:516-596`, generator
  `else_if_chain(links, pick)` at `:507`). Despite its name it is a plain `#[test]` (no `cfg(feature = "codegen")`) driving
  `axon run`, `axon check` and `axon-run`, so it already runs in the `--no-default-features` cli_run stage; no separate interpreter
  twin is needed. Add a generator `else_if_let_chain(links, pick)` (each link ` else if let {i} = n {{ {i} }}`, a literal pattern) and
  a sibling test `else_if_let_chain_is_bounded` with the same two halves: 3000 links run and pick first/middle/last/else; 5000 and
  30000 links give exit 2 with "expression nesting too deep (limit 4000)" from every front end. It is M4647's killer.
- `[REVISED: LFD#1, ruling 1]` Native (gated): `tests/integration_fixtures.rs` test `if_let_matches_on_both_engines`
  (`#[cfg(feature = "codegen")]`, same pattern as LF-1's): z01 written with `if let` (value form in `get`, statement forms, a 2-link
  `else if let` chain) → `axon run` and the native binary both print `3\n3\nabc\n4\n6`. The irrefutable `y` line of z01 stays as a
  `match` (it is not an `if let`). ATTACK: `ATTACK: if let diverged natively`. `native_shapes_parity.sh` may carry a mirror case as a
  dev-loop aid only.

**Acceptance.** i01/i03 run; the tests above; z01-in-sugar prints `3\n3\nabc\n4\n6` via run and native in the gated test; CS3-5 `if let`
row and its probe changed (LF-5).

---

### LF-3 (P1) `for (a, b) in xs`

**Today.** `parse_for` (`parser.rs:2056-2145`) calls `expect_ident()` after `for` (`:2067`).

```
=== f01_for_tuple.ax
fn main() {
    let pairs = [(1, "a"), (2, "b")]
    for (n, s) in pairs {
        println("{to_str(n)}={s}")
    }
}
check rc 2 / run rc 2:
{"schema":"axon-diag/1","severity":"error","code":"E0000","file":"f01_for_tuple.ax","line":3,"col":10,"message":"unexpected token: LParen, expected identifier","help":"`for` binds one name — write `for p in xs { let (a, b) = p … }`"}
```

f02 `for (i, s) in arr_enumerate(["x", "y"])` → same at 2:10. The advised spelling f03 (`for p in pairs { let (n, s) = p ... }`) runs:
`1=a\n2=b`.

Related defects found on the advised spelling (both fixed here because LF-3 builds on it):
- **Dummy spans.** `parse_let`'s tuple destructure builds every generated statement with `span: Span::dummy()`
  (`parser.rs:2372`, `:2384`), and the splice keeps them (`parser.rs:39-41`). Observed: q06 `let (a, b, c) = (1, 2)` → E0401 at **1:1**
  "tuple index 2 out of bounds (length 2)"; p22 (`for p in [1, 2] { let (a, b) = p ... }`) → two E0401 at **7:1** "i64 has no field
  '0'" / "'1'" (the `let` is on line 4).
- **fmt leaks the desugaring.** `axon fmt` on `let (n, s) = p` prints `let __tup_34 = p` / `let n = __tup_34.0` / `let s = __tup_34.1`
  (fm02/fm02b, rc 0, and `fmt --check` then passes on that output).

**Desugaring (decided).** In `parse_for`, if `at(LParen)` after `for`: parse a *for-pattern* (D4 grammar below), expect `in`, parse the
collection; the range form is refused (`for (a, b) in 0..n` → E0000 "a `for` over a range binds one integer name"). Then exactly the
existing collection desugar (`parser.rs:2091-2145`) with the loop variable `__forpat_N` and, prepended to the body, one `let` per bound
name:

```
for (a, (b, _)) in xs { body }
=> { ref __forarr_N = xs
     for __fori_N in 0..len(__forarr_N) {
         let __forpat_N = __forarr_N[__fori_N]
         let a = __forpat_N.0
         let __forpat_N_1 = __forpat_N.1
         let b = __forpat_N_1.0
         body } }
```

Nested tuples use an intermediate `__forpat_N_<path>` (not `.1.0`, which the lexer may read as a float [INFERENCE]); `_` emits no `let`.
`[REVISED: build lf/ab]` Every tuple level's synthetic name also carries its arity: `__forpat_N_n<K>` (nested: `<parent>_<k>_n<K>`).
Reason: `_` emits no `let`, so without the arity fmt cannot print a trailing `_` slot back (`for (a, _) in xs` would print as the
refused `for (a) in xs`); `as_desugared_foreach` reads K from the name.
`[REVISED: LFA#4, ruling 5]` Every generated statement carries one span captured at `parse_for` entry: `let span = self.current_span();`
moves from after `parse_for_body()` (`parser.rs:2101`, where it is the span of the token after the closing `}`) to the top of
`parse_for`, before `self.expect(&Token::For)` (`:2058`), so it is the `for` keyword's span (user text), never `Span::dummy()`. This
applies to every collection-`for` (plain `for x in xs` too), so any infer diagnostic raised on a generated statement of an existing
loop moves from the line after the loop to the `for` line (observed today: p23's `len` error at 8:5, the `println` after the loop).
Tests that pin the old post-loop line flip [INFERENCE: none found by the reviewers; the builder greps `cli_run.rs` for E0102/E0401
expectations on loops]. Element access goes through `FieldAccess` `.i`, which infer types (`checker.rs:7551` "tuple index {i} out of
bounds") - unlike a `match` tuple pattern (O-LF-01) - so `for (a, b) in [(1, "s")] { to_str(b + 1) }` is a check error, not a run
panic.

Verified today, the desugared shape (d08, with `__p`/`__tup_` names written by hand): `[(1, 10), (2, 20)]` sum of products plus an
`arr_enumerate([5, 6, 7])` loop → `axon run` `70`, native binary `70`, rc 0; d11 (direct `__tup_7.0` / `.1` lets) native rc 0, same
output; d05 (`dict_to_pairs(d)` pairs) run and native both `a=1\nb=2`. d03 (`[str]` elements via `arr_enumerate`, `str` concatenation)
runs (`1=a\n2=b\n0x\n1y\n140`) and is refused natively with E0910 (`arr_enumerate` on `[str]`; `Add` on `Str`) - a pre-existing native
refusal, not a wrong answer.

**Decisions.**
- *D4 grammar.* `for_pat ::= IDENT | '(' for_pat_elem ( ',' for_pat_elem )+ ','? ')'`, `for_pat_elem ::= IDENT | '_' | '(' ... ')'`
  (nesting allowed, at least two elements per tuple; `for (a) in xs` is refused with "a one-name pattern needs no parentheses"; no
  literals, no enum patterns - those are refutable and `for` has no else).
- *Arity* follows `let (a, b) = p` exactly: more names than elements → E0401 "tuple index k out of bounds", now at the `for` line; fewer
  names → accepted, the rest ignored (q07: today's `let` prints `3`; logged as O-LF-02, not changed here so `for` and `let` agree).
- *`for (i, x) in arr_enumerate(xs)`* and `arr_zip(xs, ys)` work through the generic path (both return `[(T, U)]`,
  `AXON_REFERENCE.md:337,367`); no special builtin is added.
- *`for k in d` / `for (k, v) in d` over a `Dict`* stays unsupported (LF-3b): dicts are not ordered collections; `dict_to_pairs(d)`
  (`AXON_REFERENCE.md:441`, key order) and `dict_keys(d)` (`:433`) are the deterministic spellings.
- *`let (a, b) = e` itself* is fixed in the same item: spans (above) and fmt resugaring.
- `[REVISED: LFA#5, ruling 5]` *The three-clause `for` help survives.* Today c3.ax (`for (i = 0; i < 2; i = i + 1) { println("x") }`)
  gets E0000 2:10 "unexpected token: LParen, expected identifier" with help "Axon has no three-clause `for` — iterate a range with
  `for i in 0..n { … }`, or write the counter loop with `while`: …" (observed). The help fires at `parse_help.rs:466-472` on
  `msg.contains("expected identifier") && w.first() == Some(&"for") && line.contains(';')`. With the `at(LParen)` branch the error
  moves inside the for-pattern (at `=`), so that condition would stop matching. **Decided:** every error the for-pattern parser
  raises contains the fixed text "in a `for` pattern": "unexpected token: {tok} in a `for` pattern, expected a name, `_` or `(`" (at
  an element) and "unexpected token: {tok} in a `for` pattern, expected `,` or `)`" (after an element). The help condition becomes
  `(msg.contains("expected identifier") || msg.contains("in a `for` pattern")) && w.first() == Some(&"for") && line.contains(';')`, so
  a for-pattern whose element or separator is not IDENT/`_`/`(`/`,`/`)` on a line containing `;` keeps the three-clause help.
  Alternative: make the for-pattern parser say "expected identifier": rejected, it is false after a name (an identifier was found).
  The synthetic probe at `parse_help.rs:626-630` stays; a second probe case with the new message goes beside it. That file is lane A's
  (it already edits rows 1 and 3).

Alternatives considered:
- *Desugar to `let (a, b) = __forpat_N` and reuse `parse_let`'s `__tup_` splice.* Rejected: two synthetic layers, and fmt would have to
  peel both.
- *Desugar the body to `match __forpat_N { (a, b) => body }`.* Rejected: match tuple patterns are untyped (O-LF-01, q05 panics at run).
- *A `Pattern` field on `Expr::For`.* Rejected: new AST shape for every consumer (D1).

**`axon fmt` (decided).**
1. Extend `as_desugared_foreach` (`fmt.rs:1097-1136`): when the user variable starts with `__forpat_`, consume the following `let`s
   whose value is `Ident(__forpat_*).i` and rebuild the pattern; print `for (a, (b, _)) in coll { rest }`. `_` slots are the indices with
   no `let`. The existing test `fmt_for_in_collection_resugars` (`fmt.rs:1418`) stays green.
2. New `as_desugared_let_tuple(stmts, i)`: a `Let { name: __tup_N, value: e }` followed by `Let { name: x_k, value: Ident(__tup_N).k }`
   for k = 0..m-1 → print `let (x_0, ..., x_{m-1}) = e`. This removes today's leak.

**Fail-first tests** (`cli_run.rs` block `// lang-forms LF-3`):
- `for_tuple_pattern_runs`: f01 → `1=a\n2=b`; f02 → `0x\n1y` (spelled `"{to_str(i)}{s}"`); nested `for (i, (a, b)) in
  arr_enumerate(arr_zip([1, 2], [10, 20]))` sums `i + a * b` → `51`; `for (_, s) in pairs` prints `a\nb`. ATTACK: `ATTACK: for (a, b) in
  was refused at parse`.
- `for_tuple_pattern_is_typed`: `for (a, b) in [(1, "s")] { println(to_str(b + 1)) }` → check rc 2 (E0102-class, the `+` on `str`)
  [INFERENCE: exact code], never run rc 101. ATTACK: `ATTACK: a for-tuple binding was untyped`.
- `for_tuple_arity_error_points_at_the_for`: `for (a, b, c) in [(1, 2)]` → E0401 "tuple index 2 out of bounds (length 2)" on the
  `for` line (fails today on the hand-written twin's 1:1). ATTACK: `ATTACK: a destructure error landed on line 1`.
- `let_tuple_error_points_at_the_let`: q06 → E0401 at 2:5 (today 1:1); p22 → at 4:9 [INFERENCE: column of `(` or of `let`; the builder
  pins whichever the span design yields, line 4 is required].
- `for_tuple_over_range_is_refused`: `for (a, b) in 0..3 { }` → E0000 with the new message.
- `[REVISED: LFA#5]` `three_clause_for_keeps_its_help`: real source c3.ax (`fn main() {` / `for (i = 0; i < 2; i = i + 1) {
  println("x") }` / `}`) → `axon check` rc 2, E0000 on line 2 whose help contains "Axon has no three-clause `for`" (passes today,
  fails with the for-pattern parser unless the condition is widened; M4663's killer). ATTACK: `ATTACK: the three-clause for lost its
  help`.
- fmt: `fmt_for_tuple_round_trips`, `fmt_let_tuple_round_trips` (fm02 → output contains `let (n, s) = p`, no `__tup_`).
- `[REVISED: LFD#1, ruling 1]` Native (gated): `tests/integration_fixtures.rs` test `for_tuple_pattern_matches_on_both_engines`
  (`#[cfg(feature = "codegen")]`): d08 in sugar (`for (a, b) in pairs`, `for (i, x) in arr_enumerate(..)`) → `70` on both engines; d05
  in sugar (`for (k, v) in dict_to_pairs(d)`) → `a=1\nb=2` on both. ATTACK: `ATTACK: for (a, b) diverged natively`.
  `native_shapes_parity.sh` may carry a mirror case as a dev-loop aid only.

**Acceptance.** f01/f02 run; spans and fmt fixed; c3.ax keeps its help; the gated native test green; CS3-5 `for` row and its probe
changed (LF-5).

### LF-3b (P2) `for` over a dict names the real problem

p23 (`for p in d { let (k, v) = p ... }` with `d = dict_new()`) → check rc 2, `E0102` at 8:5 "type mismatch in arg 0 of `len`
(expected str), found Dict<str, i64>" (and W0006 for `v`). Line 8 is `println("x")`, after the loop; `len` is the desugaring's call.
The checker already explains `for` over a `str` (`checker.rs:4859-4923`, E0402 "`for x in ...` iterates an ARRAY, and a `str` is not
one"; p21/p24 at 4:5), but infer refuses the `len` first for a Dict. **Fix (decided):** in infer's `len` special case
(`infer.rs:2039-2063`), when the argument is `Ident(n)` with `n.starts_with("__forarr_")` and its type is `Dict`, push E0402 "`for x in
...` iterates an ARRAY, and a Dict is not one" with help "iterate `dict_to_pairs(d)` for sorted (key, value) pairs, or `dict_keys(d)`".
`[REVISED: LFA#4]` The location comes from infer's current statement span, i.e. the generated statement's span, which is the token
after the loop today (`parser.rs:2101`); the line-4 location needs LF-3's span move to `parse_for` entry, which therefore lands in
the same lane before or with this fix. Test `for_over_dict_names_dict_to_pairs` (p23 → E0402 on line 4, the `for` line; the
assertion checks the line, so it also kills M4662). g01 rows M4664 and M4662.

---

### LF-4 (P2) `mod util` also searches the entry file's directory, last

**Today.** `axon_search_dirs` (`crates/axon-core/src/lib.rs:530-562`) builds: `AXON_PATH` entries (`:533-541`); return early under
`AXON_PATH_EXCLUSIVE=1` (`:545-547`); `$HOME/.axon/lib` (`:549-553`); `<bindir>/../lib/axon` (`:555-560`). The entry file's directory is
never searched (also stated in `CLAUDE.md:399-402`: "THE ENTRY FILE'S OWN DIRECTORY IS NEVER SEARCHED").

```
/var/tmp/r51-tmp/LangForms/m1/main.ax:   mod mathutil / use mathutil.{square} / fn main() { println(to_str(square(4))) }
/var/tmp/r51-tmp/LangForms/m1/mathutil.ax: pub fn square(x: i64) -> i64 { x * x }
$ env -u AXON_PATH axon check m1/main.ax     rc 2
{"schema":"axon-diag/1","severity":"error","code":"E0901","file":"m1/main.ax","line":2,"col":1,"message":"module `mathutil` not found\n  searched:\n    /root/.axon/lib/mathutil.ax (not found)\n    /var/tmp/r51-tmp/asi-bin/../lib/axon/mathutil.ax (not found)\n  hint: AXON_PATH is unset - set it to the directory containing `mathutil.ax` (e.g. `AXON_PATH=examples/stdlib axon run ...`)"}
{"schema":"axon-diag/1","severity":"error","code":"E0001","file":"m1/main.ax","line":3,"col":13,"message":"cannot find name `square` in this scope",...}
$ (cd m1 && AXON_PATH=. axon run main.ax)    16, rc 0
```

A tracked example has the same trap: `examples/modular/agent.ax` (`mod scorelib` at :10, `scorelib.ax` beside it) → without
`AXON_PATH`: E0901 at 11:1 + two E0001, rc 2; with `AXON_PATH=examples/modular`: rc 0 (W1210 no lock entry).

**Every resolver must agree** (survey by ModSearchScout, anchors re-read):

| site | what it does | change |
|---|---|---|
| `lib.rs:530` `axon_search_dirs` | the list | new parameter `entry_dir: Option<&Path>`; appended LAST, after `<bindir>`, and never under `AXON_PATH_EXCLUSIVE=1` (the early return at `:545-547` stays before it). `[REVISED: LFA#11]` An empty `entry_dir` (`Path::new("main.ax").parent()` is `Some("")`) maps to `.` before the permission and temp-dir checks of D5 |
| `main.rs:7311-7316` `load_program_modules` | the one load every verb shares (G1a) | passes `source_path.parent()` (needs the path; its two callers `:7502`, `:8797` have it) |
| `main.rs:8043` `run_build_pipeline` | build cache key: hashes resolved module bytes (`hasher_input.extend_from_slice(m.as_bytes())`) | passes the entry dir, so a sibling module enters the cache key and the import closure |
| `[REVISED: LFA#7]` `main.rs:1530-1540` (the `@[contained]` import-edge capability check plus `--locked` verification: `resolve_use_files_transitive(&program, &search_dirs)` with `axon_search_dirs` at `:1538`), `:1658` (`resolve_modules_or_exit`, `axon lock`), `:1959` (`verify_lock_tamper`) | resolver #2 (`lib.rs:764-905` `resolve_use_files`) | same entry dir. At `:1538` this is a security requirement, not only consistency: without it a sibling module's capabilities would skip the import-edge check. At `:1658`/`:1959` it makes `axon lock` pin exactly what `run` loads |
| `lsp.rs:104` `axon_search_dirs(None)` | LSP | passes the document's directory for a `file://` URI, `None` otherwise; `tests/lsp_multi_file.rs:52-53` set `AXON_PATH_EXCLUSIVE=1`, unaffected |
| `axon-os/src/runtime.rs:749-770` `job_module_dir_sets` | admission scan's copy of the search order | `[REVISED: LFA#3, LFD#4, ruling 4]` **unchanged.** A job's child runs the wrapper `{stem}.wrap.ax` from a private staging dir (`runtime.rs:1261-1271`), so its entry dir is not `program.parent()`; jobs get no sibling search in this spec. Residual `axon imports` mismatch and the follow-up: O-LF-03 (§0) |
| `main.rs:5685`, `:5881` (`axon session` probes/cells, written to `temp_dir()`) | in-process | pass `None` |
| `main.rs:1391-1412` `cmd_caps --base-dir DIR` (used by `axon-cortex/src/fence.rs:115-150`, which stages the text in `temp_dir()`) | `use` resolves relative to DIR | entry dir = DIR when given, else the file's parent; stdin (`-`) → `None` |
| `[REVISED: LFA#7]` `lib.rs:965-1000` sealed guard (`resolver::sealed_module_dirs`; the run-side branch is `main.rs:7224-7228`) | under `axon test --seal`, an operator module beats a sealed one | entry dir is NOT searched while sealing is active (sealed runs keep today's resolution exactly) |

**The writable-directory rule (decided, §7 D5, D9).** `[REVISED: LFA#11, ruling 6]` After mapping `""` to `.`, the entry directory is
skipped, fail-closed, when any of these holds: `std::fs::metadata(dir)` fails; it is world-writable (`mode & 0o002`, which covers
`/tmp` and `/var/tmp`, mode 1777); it is group-writable (`mode & 0o020`); or it is equal to `std::env::temp_dir()` (both
canonicalized; a canonicalize failure counts as "skip"). This is what keeps axon-web safe: it writes each request to the flat
file `temp_dir()/axon_web_<sha>.ax` (`axon-web/src/api.rs:507-516`) and runs `axon run <tmp>` (`:52`), so its entry dir is the shared
temp dir, where any local user could plant `util.ax`. The same rule covers `axon session` and cortex's caps staging even if a caller
forgets `None`. `axon-intent/src/genloop.rs:965-971` stages in a private 0700 directory, which is searched (it holds only what the
generator wrote). Windows: no mode bits; only the `temp_dir()` equality applies [INFERENCE: no Windows CI].

**Why "last" changes no program that resolves today.** First match wins (`lib.rs:1017-1034`). Every module found today is found in an
earlier directory and still wins. Only programs that fail with E0901 today can change, and they change to resolving. Approval tokens
bind an `imports_digest` of the closure (`[REVISED: LFA#7]` enforced by `imports_digest_mismatch` `main.rs:1022` and
`enforce_required_imports_digest` `main.rs:1048`); a program whose closure changes was not runnable before, so no approved program's
digest changes [INFERENCE: no approval was minted for a program that fails check]. The axon-os `imports`-vs-run exception is O-LF-03.

**E0901 text.** The `searched:` list (`lib.rs:1163-1180`) includes the entry-dir line when it was searched, and a line `<dir>/util.ax
(not searched: <reason>)` when it was skipped, with `<reason>` one of `group- or world-writable directory`, `the system temp
directory`, `cannot read its permissions`, so the refusal explains itself. The `AXON_PATH is unset` hint stays.

**Existing tests** `[REVISED: LFA#6]` (anchors re-read):
- `cli_run.rs:44583` `module_not_found_names_the_use_line` (M3989's killer, `v022_g01_mutations.py:12491-12494`) **flips**: its
  "entry" case writes `main.ax` (`mod mathutil` / `use mathutil.{square, cube}`) and `mathutil.ax` side by side in
  `temp_dir()/axon_cs3_7_<pid>` (made by `create_dir_all`, so neither group/world-writable [INFERENCE: umask 022] nor `temp_dir()`),
  runs `check(&main, None)` and requires E0901 on line 2. After LF-4 that module resolves. Rewrite: the entry case uses a new file
  `e_main.ax` (`mod nomod` / `use nomod.{square}`, no `nomod.ax` anywhere) and keeps the same assertion (E0901, `"line":2,`, the file
  name), and the comment "the module beside main is not searched (documented)" goes. The nested and circular cases run with
  `AXON_PATH_EXCLUSIVE=1` and are unaffected; the "Control" case stays. The test must still kill M3989 (the builder re-runs that row).
- `cli_run.rs:6641` `missing_module_error_hints_axon_path_when_unset` writes a flat file directly in `temp_dir()`: entry dir =
  `temp_dir()` → skipped → still E0901 with the `AXON_PATH` hint (it asserts only `E0901` and `AXON_PATH`), plus the new skip line.
- `tests/pci_isolation.rs:72` `an_exclusive_module_path_never_falls_through` stays green unchanged (EXCLUSIVE).

**Tests** (`cli_run.rs` block `// lang-forms LF-4`; temp dirs created with mode 0700 unless stated). `[REVISED: LFD#9]` Fail-first
tests fail at a5310878; guards pass today and exist to kill a mutant.
- Fail-first `sibling_module_resolves_last`: m1 → `16`, no `AXON_PATH`. ATTACK: `ATTACK: a module beside its importer was not found`.
- Guard (passes today) `axon_path_still_wins_over_sibling`: same name in `AXON_PATH` and beside main, different bodies → the
  `AXON_PATH` one runs. Kills M4666.
- Guard (passes today) `exclusive_never_searches_the_entry_dir` `[REVISED: LFD#8, with correction]`: m1 (with `mod mathutil`) under
  `env -u AXON_PATH AXON_PATH_EXCLUSIVE=1` → rc ≠ 0, stdout does not contain `16`, stderr has E0001 "cannot find name `square`".
  Observed today: exactly that one E0001 line, rc 2, for both `check` and `run` (no E0901, no E0003; a bare `use` without `mod` gives
  E0003 instead). Do not assert E0901. Kills M4667.
- `world_writable_entry_dir_is_not_searched`: m1 in a dir `chmod 0o777`, and again in a dir `chmod 0o770` (group-writable) → rc 2,
  E0901, and (the fail-first assertion; E0901 alone already holds today) the `searched:` list contains `(not searched: group- or
  world-writable directory)`. ATTACK: `ATTACK: a module was resolved from a world-writable directory`.
- Fail-first `relative_entry_path_searches_dot`: `axon run main.ax` with `current_dir` = m1's 0700 dir → `16` (kills a mutant that
  skips `""` instead of mapping it to `.`).
- Guard (passes today) `sealed_run_ignores_the_entry_dir`: an `axon test --seal` case where a sealed candidate and its helper sit
  beside the suite → today's result. Kills M4669.
- Fail-first `lock_pins_the_sibling_module`: `axon lock m1/main.ax` then `axon run --locked` → rc 0; editing `mathutil.ax` → the
  lock-tamper refusal.
- Fail-first `caps_base_dir_is_the_entry_dir`: `main.ax` (`mod util` / `use util.{f}`) in 0700 dir S, `util.ax` only in 0700 dir D;
  `axon caps S/main.ax --base-dir D --json` → rc 0 (resolved from D) [INFERENCE: today E0901, since `set_current_dir(D)`
  (`main.rs:1406-1410`) does not add D to the search list]. Kills M4672.
- Fail-first `contained_import_edge_checks_a_sibling_module` `[REVISED: LFA#7]`: a `@[contained]` main that grants no IO, with
  `mod util` / `use util.{f}`, and `util.ax` beside it whose `pub fn f()` calls `println` → rc 2 with E1203 (the import-edge
  capability error, `main.rs:1548`, `:1578`). Today the module is not found (E0901), so the assertion is on E1203. Program text is
  copied from an existing E1203 test [INFERENCE: exact grant syntax]. Kills M4677.
- `[REVISED: LFD#2, ruling 1]` Fail-first, gated `build_cache_key_covers_the_sibling` in `tests/integration_fixtures.rs` **only**
  (`#[cfg(feature = "codegen")]`; `cli_run` runs only with `--no-default-features`, `gate.sh:1069`, `:1528-1530`): m1 built
  natively without `AXON_PATH` prints `16`; then `mathutil.ax` changes to `x * x + 1` and a second `axon build` (cache on, no
  `--no-cache`) prints `17`. ATTACK: `ATTACK: the build cache reused a binary after its sibling module changed`.
- `[REVISED: LFA#3, ruling 4]` ~~axon-os: `job_module_dir_sets` unit test~~: withdrawn (no axon-os change).
- LSP: `lsp_multi_file.rs` new test without EXCLUSIVE: a `file://` main beside its module gets no E0901 [INFERENCE: the session helper
  forces EXCLUSIVE at `:53`; the new test needs its own env].

**Acceptance.** All of the above; `env -u AXON_PATH axon check examples/modular/agent.ax` → rc 0 (today E0901 at 11:1, rc 2);
gotcha #10 removed; axon-os unchanged (O-LF-03 logged).

Alternatives considered:
- *Search the importing module's own directory for transitive uses.* Rejected for now: one search list per program is easier to audit;
  a transitive module found through `AXON_PATH` is already in a searched directory.
- *Search it first.* Rejected by the human (changes existing resolution).
- *Opt-out env var for axon-web.* Kept as a fallback only; the permission rule protects every flat-temp-file consumer without each one
  remembering a flag.

---

### LF-5 (P3, lands with each item) Docs and CS3-5 help rows

The CS3-5 rows tell users to rewrite exactly these forms; once a form parses, its row never fires on it (the parser no longer reports
that token). They must still be edited, because their probes assert the old text and one row would misdirect a near-miss:
- `parse_help.rs:145-151` (`()`): delete the row. The probe cases `unit_ok`, `unit_let` (`tests/parse_help_probe.rs:677-686`) move to
  `cli_run.rs` as must-run cases (LF-1 tests).
- `parse_help.rs:152-160` (`if let`): delete; probe cases `if_let`, `else_if_let` (`:687-697`) move to LF-2 tests. A remaining refused
  near-miss, `if let A | B = e`, gets no special help (today's parse error).
- `parse_help.rs:161-167` (`for (a, b)`): change the help to the cases still refused: a range `for (a, b) in 0..n` ("a `for` over a range
  binds one integer name") - produced by the parser itself (LF-3), so this row is deleted and its probe `for_tuple` (`:698-702`) moves.
  `[REVISED: LFA#5]` The separate three-clause row (`parse_help.rs:466-472`) is NOT deleted; its condition is widened (LF-3).
- `the_refused_form_advice_actually_compiles` (`parse_help_probe.rs:765-800`): the "unit block", "match for if let", "let tuple in
  for" cases stay (they are still valid Axon); add the sugared spellings beside them.
- `scripts/v022_g01_mutations.py` M3984's killer `refused_forms_point_at_the_axon_spelling` loses three cases but keeps the `impl` case
  M3984 mutates; re-run that row (§4).

`[REVISED: LFD#6]` Docs. Examples are checked by `tests/doc_snippets.rs` only when they sit in a `DOCS` file (`doc_snippets.rs:33-40`:
`spec/axon-for-llms.md`, `spec/language-tour.md`, `spec/stdlib.md`, `README.md`, `CLAUDE.md`, `examples/asi/README.md`) inside a block
fenced exactly ```` ```axon ```` (`:254`). `spec/grammar.ebnf`, `AXON_REFERENCE.md` and `CHANGELOG.md` are not checked; the integrator
re-reads them.
- `spec/grammar.ebnf`: `primary` (`:331-341`) gains `'(' ')'`; `UNIT` note (`:484-488`) rewritten; `if_expr` (`:259`) gains
  `'if' 'let' pattern '=' logical block [ 'else' (if_expr | block) ]`; `for_expr` (`:249`) gains `for_pat`; comment lines `:236-262`
  updated to say both are desugared and how.
- `spec/axon-for-llms.md`: §2 syntax block (`:39-135`) shows `Ok(())`, `if let`, `for (i, x) in arr_enumerate(xs)`; gotcha #10
  (`:251-253`) deleted and the list renumbered; new gotcha: "`if let` has no `&&` chains and no `|` patterns; `for (k, v)` needs
  `dict_to_pairs(d)` - a dict is not iterable".
- `spec/language-tour.md`: §5 Control Flow (`:97-130`) gets `if let` and `for (a, b)`; the primitive-types table row `()` (`:44`)
  says "`()` or `{}`"; the modules section notes the sibling search.
- `[REVISED: LFD#7, with correction]` `AXON_REFERENCE.md:89` (`AXON_PATH` row) is generated: `cmd_reference` (`main.rs:5329`) prints
  `axon_core::env_registry::ALL_ENV_VARS` (`main.rs:5356`), and the row text is `crates/axon-core/src/env_registry.rs:33`
  (`("AXON_PATH", "colon-separated module search path for `mod`/`use` imports")`). Lane C edits that string to add "; then the entry
  file's directory, last (never group/world-writable or the temp dir, never under `AXON_PATH_EXCLUSIVE`, never while sealing)" and
  regenerates `AXON_REFERENCE.md` (`cargo run -p axon-core --bin axon -- reference > AXON_REFERENCE.md`, the fix text of the drift
  test `cli_run.rs:26475-26528`) in the same commit; that drift test is the check. Never edited by hand (cs3 §3).
- `CLAUDE.md:396-404` comment ("THE ENTRY FILE'S OWN DIRECTORY IS NEVER SEARCHED") rewritten.
- `CHANGELOG.md`: one section "Language forms: `()`, `if let`, `for (a, b)`, sibling modules (lang-forms LF-1..LF-5)" with a line per item,
  incl. the u06 `()` pattern panic fix and the fmt `__tup_` leak fix. `[REVISED: LFA#2, LFA#1, LFA#4, ruling 3]` Plus explicit
  behaviour-change lines: (1) "a `()` pattern now matches the unit value: a `match` that passed check and took a fallback arm next to
  a `()` arm now takes the `()` arm (`Ok(())`, `()` arms), and one that panicked with `no match arm matched` now runs"; (2) "native:
  a `match` on a unit-valued subject runs its `()`/`_` arm instead of being skipped; any other pattern there is refused with E0910";
  (3) "diagnostics raised on a collection-`for`'s generated statements now point at the `for` line, not the line after the loop".

## 3. Shared mechanics

- Each commit touching `crates/axon-core/src` is followed by a `pci:` theme (`governance/notes/pci-themes/<sha8>.json`, `{"theme",
  "text"}`), then `python3 tasks/r51-pci-regen.py`, then `python3 scripts/pci_delta.py --check` (cs3 §3 procedure).
- g01 rows and `ATTACK:` markers land in the same commit as their fix (`scripts/v022_g01_mutations.py`, `scripts/v022_attack_markers.py`);
  each killing test's assert message carries the marker text.
- Fail-first programs are inlined as string literals; no test reads `/var/tmp`.
- `scripts/v022_refusal_coverage.py`: the new refusals (range `for` pattern, one-name parenthesized pattern) are in `parser.rs`, not in a
  protected file or seal/conform/cast region (`SCOPE_FILES`/`SCOPE_FN_REGIONS`, `v022_refusal_coverage.py:91,312-313`) [INFERENCE: run
  the script; if it lists a site, add a row]. LF-4's skip in `axon_search_dirs` is not an `Err(`. `[REVISED: LFA#1]` LF-1's new native
  E0910 site (`codegen/expr.rs`, unit-subject match) is a `record_error`, not an `Err(`; the builder runs the script and adds a row if
  it lists the site [INFERENCE].
- `[REVISED: LFD#1, ruling 1]` Interpreter is the reference; native follows through the desugaring, except LF-1 step 4, which is a
  real codegen change. **Native parity is gated only through `tests/integration_fixtures.rs`**, which `scripts/gate.sh:1194-1196` runs
  under `--strict` (`cargo test --locked -p axon-core --test integration_fixtures`, default features, so `codegen` is on). Each item
  has one `#[cfg(feature = "codegen")]` test there that runs `axon run`, `axon build --no-cache -o`, the binary, and compares stdout
  (LF-1 two tests, LF-2 one, LF-3 one, LF-4 `build_cache_key_covers_the_sibling`). `scripts/native_shapes_parity.sh` is run by no
  gate (only the script itself names it); a lane MAY add mirror cases there as a dev-loop aid, and nothing in this spec claims it is
  gated. `integration_fixtures.rs` is integrator-owned: each lane hands its test as a block appended at the end of the file.

## 4. g01 rows (M4640-M4679)

Format as `scripts/v022_g01_mutations.py:12470-12495` (`(id, title, file, old, new, crate, test args, killer)`); the builder writes the
exact `old` line after the fix lands. Target `axon-core --no-default-features --test cli_run` unless named. `[REVISED: LFD#1, ruling 1]`
Rows whose killer is in `integration_fixtures` use test args `'--test integration_fixtures'` with default features (the `codegen`
feature on), as existing rows do (`v022_g01_mutations.py:11904`, `:12242`, `:12729`).

| id | item | guard removed by the mutant | file | killer | marker |
|---|---|---|---|---|---|
| M4640 | LF-1 | `()` → empty block in `parse_primary_inner` (mutant: fall through to `parse_expr`) | parser.rs | `unit_value_paren_is_the_empty_block` | `ATTACK: Ok(()) was refused at parse` |
| M4641 | LF-1 | the `is_paren_lambda` check stays first (mutant: unit check before it) | parser.rs | `paren_lambda_with_no_params_stays_a_lambda` | `ATTACK: () => was parsed as unit` |
| M4642 | LF-1 | `Pattern::Tuple([])` matches `Value::Unit` (mutant: `return Ok(false)`) | interp/eval.rs | `unit_pattern_matches_the_unit_value` | `ATTACK: a () pattern passed check and panicked` |
| M4643 | LF-1 | fmt prints `()` in value position (mutant: `{}`) | fmt.rs | `fmt::tests::fmt_unit_value_prints_parens` (`--lib`) | `ATTACK: fmt wrote a W0008 empty block` |
| M4644 | LF-2 | `eat(Let)` branch in `parse_if` | parser.rs | `if_let_runs` | `ATTACK: if let was refused at parse` |
| M4645 | LF-2 | else arm body = parsed else expr (mutant: always `{}`) | parser.rs | `if_let_runs` (else/chain cases) | same |
| M4646 | LF-2 | catch-all is `Pattern::Ident(__iflet_N)` (mutant: `Wildcard`) | parser.rs | `fmt::tests::fmt_if_let_round_trips` | `ATTACK: fmt rewrote if let as match` |
| M4647 | LF-2 | `else if` link goes through `self.nested` (mutant: direct call) | parser.rs | `else_if_let_chain_is_bounded` (`[REVISED: LFD#5]`, cli_run, `--no-default-features`) | `ATTACK: an else-if-let chain overflowed the stack` |
| M4648 | LF-2 | infer's `if let` without `else` wording (mutant: match-arm wording) | infer.rs | `if_let_without_else_must_be_unit` | `ATTACK: if let without else named a match arm` |
| M4649 | LF-2 | W0004 `if let` pre-check uses `pattern_irrefutable` (`[REVISED: LFA#8]` mutant: `matches!(.., Wildcard \| Ident(_))` only) | checker.rs | `irrefutable_if_let_warns` (tuple case) | `ATTACK: an irrefutable if let named a match arm` |
| M4650 | LF-2 | fmt recognizer requires the `__iflet_` prefix (mutant: any binding catch-all) | fmt.rs | `fmt::tests::fmt_user_match_with_wildcard_is_not_resugared` | `ATTACK: fmt rewrote a user match` |
| M4651 | LF-2 | fmt prints `else` chains (mutant: `else { .. }` always) | fmt.rs | `fmt::tests::fmt_if_let_round_trips` | `ATTACK: fmt broke an else-if-let chain` |
| M4652 | LF-3 | `at(LParen)` branch in `parse_for` | parser.rs | `for_tuple_pattern_runs` | `ATTACK: for (a, b) in was refused at parse` |
| M4653 | LF-3 | field-access lets (mutant: `match` tuple pattern body) | parser.rs | `for_tuple_pattern_is_typed` | `ATTACK: a for-tuple binding was untyped` |
| M4654 | LF-3 | generated lets carry the pattern span (mutant: `Span::dummy()`) `[REVISED: build lf/ab]` the arity E0401 alone did not kill it (the checker keeps the last real statement span, the `for` loop's own); the killer also asserts the W0006 for an unused pattern name sits at the `for` (with the mutant it has no line at all) | parser.rs | `for_tuple_arity_error_points_at_the_for` | `ATTACK: a destructure error landed on line 1` |
| M4655 | LF-3 | `parse_let` destructure span (mutant: `Span::dummy()` at today's `:2372`) `[REVISED: build lf/ab]` mutant moved to the field lets' span (today's `:2384`), and, as for M4654, the killer also asserts the unused-name W0006 sits at the `let` (E0401 kept the enclosing statement's span, so it alone did not kill it) | parser.rs | `let_tuple_error_points_at_the_let` | same |
| M4656 | LF-3 | nested pattern intermediate (mutant: skip nesting lets) | parser.rs | `for_tuple_pattern_runs` (nested case) | `ATTACK: a nested for pattern bound nothing` |
| M4657 | LF-3 | `_` emits no let (mutant: binds `_`) `[REVISED: build lf/ab]` **dropped as EQUIVALENT**: g01 run, survived; `let _ = elem.k` is unreadable and silent (leading `_`), so no engine output differs | parser.rs | - | - |
| M4658 | LF-3 | range form refused (mutant: accepted) | parser.rs | `for_tuple_over_range_is_refused` | `ATTACK: for (a, b) over a range was accepted` |
| M4659 | LF-3 | fmt `__forpat_` resugar | fmt.rs | `fmt::tests::fmt_for_tuple_round_trips` | `ATTACK: fmt leaked __forpat_` |
| M4660 | LF-3 | fmt `__tup_` resugar | fmt.rs | `fmt::tests::fmt_let_tuple_round_trips` | `ATTACK: fmt leaked __tup_` |
| M4661 | LF-1 | `[REVISED: LFA#1, ruling 2]` unit-subject match lowered with the placeholder (mutant: restore `self.emit_expr(subject, fn_val)?`) | codegen/expr.rs | `unit_match_runs_the_unit_arm_on_both_engines` (`--test integration_fixtures`) | `ATTACK: a () arm was skipped natively` |
| M4662 | LF-3 | `[REVISED: LFA#4, ruling 5]` span captured at `parse_for` entry (mutant: `let span = self.current_span();` back after `parse_for_body()`) | parser.rs | `for_over_dict_names_dict_to_pairs` | `ATTACK: for over a dict blamed len` |
| M4663 | LF-3 | `[REVISED: LFA#5, ruling 5]` `\|\| msg.contains("in a `for` pattern")` in the three-clause help condition (mutant: dropped) | parse_help.rs | `three_clause_for_keeps_its_help` | `ATTACK: the three-clause for lost its help` |
| M4664 | LF-3b | Dict branch in infer's `len` special case | infer.rs | `for_over_dict_names_dict_to_pairs` | `ATTACK: for over a dict blamed len` |
| M4665 | LF-4 | entry dir appended in `axon_search_dirs` | lib.rs | `sibling_module_resolves_last` | `ATTACK: a module beside its importer was not found` |
| M4666 | LF-4 | appended after `<bindir>` (mutant: before `AXON_PATH`) | lib.rs | `axon_path_still_wins_over_sibling` | `ATTACK: a sibling module shadowed AXON_PATH` |
| M4667 | LF-4 | after the EXCLUSIVE early return (mutant: before it) | lib.rs | `exclusive_never_searches_the_entry_dir` | `ATTACK: EXCLUSIVE searched the entry dir` |
| M4668 | LF-4 | world-writable skip (`mode & 0o002`) | lib.rs | `world_writable_entry_dir_is_not_searched` (0o777 case) | `ATTACK: a module was resolved from a world-writable directory` |
| M4669 | LF-4 | skip while sealing | lib.rs | `sealed_run_ignores_the_entry_dir` | `ATTACK: a sealed run searched the entry dir` |
| M4670 | LF-4 | lock resolver passes the entry dir | main.rs | `lock_pins_the_sibling_module` | `ATTACK: the lock pinned a different closure` |
| ~~M4671~~ | LF-4 | `[REVISED: LFA#3, LFD#4, ruling 4]` **withdrawn**: axon-os `job_module_dir_sets` is unchanged in this spec (O-LF-03). The id stays unused. | - | - | - |
| M4672 | LF-4 | `--base-dir` is the entry dir for `caps` | main.rs | `caps_base_dir_is_the_entry_dir` (new) | `ATTACK: caps resolved from the staging dir` |
| M4673 | LF-2 | `[REVISED: LFD#1, ruling 1]` `eat(Let)` branch in `parse_if` (same mutant as M4644, proves the gated native test is live) | parser.rs | `if_let_matches_on_both_engines` (`--test integration_fixtures`) | `ATTACK: if let diverged natively` |
| M4674 | LF-3 | `[REVISED: LFD#1, ruling 1]` `at(LParen)` branch in `parse_for` (same mutant as M4652) | parser.rs | `for_tuple_pattern_matches_on_both_engines` (`--test integration_fixtures`) | `ATTACK: for (a, b) diverged natively` |
| M4675 | LF-4 | `[REVISED: LFD#2, ruling 1]` `run_build_pipeline` passes the entry dir (mutant: `None`) | main.rs | `build_cache_key_covers_the_sibling` (`--test integration_fixtures`) | `ATTACK: the build cache reused a binary after its sibling module changed` |
| M4676 | LF-1 | `[REVISED: LFA#1, ruling 2]` E0910 for any other pattern on a value-less subject (mutant: placeholder for every pattern) | codegen/expr.rs | `unit_match_with_a_binding_arm_is_refused_natively` (`--test integration_fixtures`) | `ATTACK: a unit match arm was skipped natively without a refusal` |
| M4677 | LF-4 | `[REVISED: LFA#7]` `@[contained]` import-edge check at `main.rs:1538` gets the entry dir (mutant: `None`) | main.rs | `contained_import_edge_checks_a_sibling_module` (new, cli_run: a `@[contained]` main without IO importing a sibling whose `pub fn` calls `println` → E1203; program text copied from an existing E1203 test [INFERENCE: exact grant syntax]) | `ATTACK: a sibling module skipped the import-edge capability check` |
| M4678 | LF-4 | `[REVISED: LFA#11, ruling 6]` group-writable skip (`mode & 0o020`) | lib.rs | `world_writable_entry_dir_is_not_searched` (0o770 case) | `ATTACK: a module was resolved from a group-writable directory` |
| M4679 | LF-4 | `[REVISED: LFA#11, ruling 6]` `""` entry dir maps to `.` (mutant: skips it) | lib.rs | `relative_entry_path_searches_dot` | `ATTACK: a relative entry path lost its sibling module` |

## 5. Gates and cost budget (no paid spend)

| stage | wall | source |
|---|---|---|
| all-tracked check sweep `bash /var/tmp/r51-tmp/CS2Draft/cs-sweep-all.sh <repo> <bin> <ABSOLUTE out>` `[REVISED: LFD#10]` **dev-loop only, not a gate stage** (script and baseline are untracked scratch files) | 7.9 s | measured at a5310878: `files=700 rc0=613`, baseline `/var/tmp/r51-tmp/LangForms/s-all-a5310878.txt` (2 E0901 files: `examples/sealed/suite/accept.ax`, `tasks/r51-s6/q1/suite/accept.ax`; both `mod sol` with no `sol.ax` beside them, so LF-4 leaves them E0901 [INFERENCE: re-check after the build]) |
| `AXON=<bin> bash scripts/native_shapes_parity.sh` `[REVISED: LFD#1]` **dev-loop aid only; no gate runs it** | 3.8 s (55 shapes OK) | measured with the reference binary |
| `cargo test --locked -p axon-core --test integration_fixtures` `[REVISED: LFD#1]` (the gated native route; `gate.sh:1194-1196`, `--strict` only, links LLVM) | not measured | 5 new tests, each one `run` + one `build` + one binary run per program [INFERENCE: a few seconds per test once built] |
| `cargo test -p axon-core --no-default-features --test cli_run --no-run` cold / warm | 26 s / 4.4 s | measured by DevloopCS3 (cs3 §5) |
| `--test parse_help_probe` | 4.6 s | cs3 §5 |
| `--lib fmt::tests` | not measured | [INFERENCE: < 10 s warm] |
| S12 serde-json stage (`scripts/gate.sh:938-958`: clippy, `program_json_tests` AX-10 round trip, `lsp::tests`, `lsp_multi_file`) | not measured | no AST type changes, so AX-10 is a regression check only |
| one g01 row / 39 active rows (M4640-M4679 minus withdrawn M4671; M4657 may drop as equivalent) | ~15 s / ~10 min for cli_run killers | 15 s per row measured in cs3 §5; the 5 `integration_fixtures` rows rebuild with default features (LLVM) and take longer [INFERENCE: not measured] |
| `[REVISED: LFD#5]` `else_if_let_chain_is_bounded` (5000/30000-link chains through `axon run`, `axon check`, `axon-run`) | not measured | runs inside the cli_run stage |
| `scripts/gate.sh --lane` | per-lane subset | `tasks/gate-lane-spec.md` (GL-4: `--lane` is NOT the gate; `--lane --plan` prints the selection) |
| `scripts/parity_all.sh` | ≈ 21 min | `gate.sh:1216-1219` comment ("21m42s strict") |
| full `scripts/gate.sh --strict` `[REVISED: LFD#3]` | 43m24s recorded (`gate.sh:1217`, `:1537`, measured 2026-10-15); ≈ 95 min on this host per the integrator's brief (not measured by me) | run at the integrator after all lanes land; budget two runs to allow one fix-and-retry |

Per-lane inner loop: edit, the item's `cli_run` killers (~5 s warm), the sweep (8 s, dev-loop only; the sweep exports
`AXON_PATH=examples/stdlib:examples/asi:examples/modular:examples/domain` (`cs-sweep-all.sh:8`), so `examples/modular/agent.ax` is
already rc 0 there and **no sweep delta is allowed** for any LF item; LF-4's effect is tested by `sibling_module_resolves_last` and an
`env -u AXON_PATH axon check examples/modular/agent.ax` → rc 0 acceptance run), the lane's own `integration_fixtures` test by name
(`cargo test -p axon-core --test integration_fixtures <name>`), optionally native_shapes_parity (4 s), the lane's g01 rows.
`gate.sh --lane` selects units from changed files and is not the gate (`--lane` refuses `--strict`, `gate.sh:100`). `parity_all`
and the strict gate once, at the integrator.

## 6. Sequencing and lanes

| lane | items | files |
|---|---|---|
| A | LF-1, LF-3, LF-3b | `parser.rs` (`parse_primary_inner` LParen, `parse_for` incl. the span move, `parse_let`), `interp/eval.rs:2920`, `[REVISED: LFA#1]` `codegen/expr.rs:607-619` (unit-subject match), `fmt.rs`, `infer.rs` (len special case `:2039-2063`), `parse_help.rs` rows 1 and 3 and `[REVISED: LFA#5]` the three-clause condition `:466-472` (+ its probe beside `:626-630`), `tests/parse_help_probe.rs` |
| B | LF-2 | `parser.rs` (`parse_if` only), `infer.rs` (`:2347-2364` Match arm special case), `checker.rs` (`:6617-6659` W0004 pre-check), `fmt.rs` (new recognizer + `If` arm `:698-703`), `parse_help.rs` row 2 |
| C | LF-4 | `lib.rs` (`axon_search_dirs`, E0901 text, sealed guard `:965-1000`), `main.rs` (callers listed), `lsp.rs:104`, `[REVISED: LFD#7]` `env_registry.rs:33` + regenerated `AXON_REFERENCE.md` (same commit), tests `lsp_multi_file.rs`. `[REVISED: LFA#3]` `axon-os/src/runtime.rs` dropped |

File-overlap graph: A–B share `parser.rs` (different fns: `parse_primary_inner`/`parse_for`/`parse_let` vs `parse_if`), `fmt.rs`
(different fns; B also edits the `If` arm at `:698-703`, A does not), `infer.rs` (different fns), `parse_help.rs` (adjacent rows
145-167; A also edits `:466-472`: **land A then B**, or one owner). C shares nothing with A/B. A's `codegen/expr.rs` edit is in the
`Match` arm only. Integrator-owned: `cli_run.rs` (blocks appended at end), `tests/integration_fixtures.rs` (blocks appended at end),
g01/marker files, `CHANGELOG.md`, docs, `tasks/opportunities.md` (O-LF-01..O-LF-04; O-GAP-08 class (c) partial stamp).
`AXON_REFERENCE.md` moves to lane C (generated from `env_registry.rs`).

Cross-spec: `tasks/asi-fixes-spec.md` (AsiFixes) and `tasks/ceilings-spec.md` (Ceilings) may touch `parser.rs`, `checker.rs`, `infer.rs`,
`interp/builtins.rs`, `resolver.rs`, `main.rs`, `lib.rs`. Ceilings' CE-5 step counter covers loops in `interp/eval.rs`/`interp/vm/*`;
LF-3 adds no loop node (it builds the existing `Expr::For`, `parser.rs:2082,2121`), so CE-5 covers it unchanged (told to Ceilings by
message). Ceilings anchors `lib.rs`/`main.rs` lines in other functions; whichever lands second re-reads anchors. No LF item touches
`interp/builtins.rs` or `resolver.rs`.
AsiFixes AF-3 (new E0409, bare variant name in a refutable pattern; `checker.rs` Match around `:4753`, disjoint from LF-2's
`:6617-6659`) skips `Pattern::Ident` names starting with `__` (`__iflet_N`) and never checks `let` positions (`__forpat_` lets).
Its fail-first `bare_variant_in_if_let_is_e0409` (`if let Green = c` → E0409, `if let C::Green = c` passes, no flag on `__iflet_N`)
needs LF-2 (lane B) to land first (agreed by message with AsiFixes). LangForms adds no error codes.

## 7. Engineering decisions (decided here)

1. **D1 - Desugar at parse time; no new AST node.** Every consumer (incl. native and AX-10 JSON) sees only existing shapes; native
   parity follows from shapes verified natively today (d07, z01, d08, d11, d05). `[REVISED: LFA#1]` One exception is a real codegen
   change, not a desugaring: LF-1 step 4 (D8), because a unit-subject `match` was already silently skipped natively.
2. **D2 - `if let` catch-all is a binding `__iflet_N`, not `_`**, so fmt recognizes it by name (z01: no W0006, works natively).
3. **D3 - fmt prints the sugar back** for all three forms and also fixes the existing `__tup_` leak. A formatter that rewrites user
   code into the desugaring is a visible change; for `()` vs `{}` the rule is "value position `()`, body position `{}`".
4. **D4 - `for` patterns are tuples of names/`_`, nested allowed; dicts stay non-iterable.** Arity semantics equal `let`'s.
5. **D5 - The entry dir is searched last, never under EXCLUSIVE, never while sealing, never if group/world-writable or the temp dir.**
   One list per program (not per importing module).
6. **D6 - Synthetic prefixes stay a convention.** A user naming a variable `__iflet_3` gets fmt resugaring of their own match; this is
   the same class as today's `__forarr_` recognizer and is accepted.
7. **D7 - The `()` pattern runtime fix is part of LF-1**, since `Ok(())` in patterns becomes the natural spelling once `Ok(())` parses.
8. **D8 - `[REVISED: LFA#1, ruling 2]` Native unit-subject `match`: lower it when every arm is `()` or `_`, refuse with E0910
   otherwise, never skip.** Lowering is about 15 lines in the `Match` arm; binding the placeholder would print a wrong value natively.
9. **D9 (was LF-NH1, decided by ruling 6) - The entry directory is the entry file's directory, for the whole program.** Not each
   importing file's directory. Reason: it is the narrower rule, gives one auditable search list per program, and can be widened later
   without breaking any program (a wider search only adds modules where today's would report E0901).
10. **D10 (was LF-NH2, decided by ruling 6) - Skip by rule, fail-closed.** The entry dir is skipped when it is world-writable
   (`0o002`), group-writable (`0o020`), equal to `temp_dir()`, or its metadata cannot be read; a relative entry (`parent()` == `""`)
   maps to `.` before these checks. In-process callers (`axon session`) still pass `None`. Reason: an explicit opt-out per caller
   alone fails open when a new staging caller forgets it.
11. **D11 (was LF-NH3, decided by ruling 6) - fmt prints `()` in value position, `{}` in body position.** Reason: no tracked `.ax`
   file's fmt output changes (counts in LF-1), and keeping `{}` would make fmt emit W0008-warned code.

## 8. Needs human

`[REVISED: ruling 6]` **None.** LF-NH1, LF-NH2 and LF-NH3 were decided by the integrator and are now §7 D9, D10 and D11.

## 9. Verification log (reference binary, a5310878, `/var/tmp/r51-tmp/LangForms/`)

| probe | result |
|---|---|
| u01-u04 `()` value | rc 2, E0000 "unexpected token: RParen, expected expression" + CS3-5 help |
| u06, u11, p17 `()` pattern | check rc 0, run rc 101 `no match arm matched` |
| d07 unit via `{}` | run = native: `pos\nnonpos\ndone` |
| d01, u13 `Result<(), str>` native | E0910 / IR verification failure (pre-existing) |
| p18 `() => 7` | `7` |
| `[REVISED]` u20 `()` pattern beside a fallback arm | check rc 0 (W0008 only); run `other\nb`; native E0910 (body of `f` returning `Result<(), str>`; `u` has no value) |
| `[REVISED]` u22 `match k() { () => println("unit") }` | check rc 0; run rc 101 `no match arm matched`; native `done` only, rc 0 |
| `[REVISED]` u21b `() => println("a"), _ => println("b")` | run `b\ndone`; native `done` only, rc 0 |
| `[REVISED]` u21 `let s = match k() { () => "a", _ => "b" }` | run `b\ndone`; native E0910 "`s` is read, but native codegen produced no value for its binding" |
| `[REVISED]` u23 `match k() { u => println("x") }` | check rc 0; run `x\ndone`; native `done` only, rc 0 |
| `[REVISED]` c3 three-clause `for` | E0000 2:10 "unexpected token: LParen, expected identifier" + three-clause help |
| `[REVISED]` m1 shape under `env -u AXON_PATH AXON_PATH_EXCLUSIVE=1` (`run`, `check`) | only E0001 `square`, rc 2; bare-`use` variant: E0003 + E0001 |
| i01, i03 `if let` | rc 2 E0000 3:8 / 3:39 "unexpected token: Let" + help |
| i04 `while let` | `1\n2` |
| z01 binding-catch-all desugar | run = native `3\n3\nabc\n4\n6`; W0004 for the irrefutable arm |
| p25 / p26 | E0102 "if-without-else body" 3:12 / "match arm type" 3:5 |
| f01, f02 `for (a, b)` | rc 2 E0000 3:10 / 2:10 + help |
| f03 advised spelling | `1=a\n2=b` |
| d08, d11, d05 desugared `for` | run = native |
| d03 `[str]` shapes | run ok; native E0910 (pre-existing) |
| q06, p22 `let (..)` errors | E0401 at 1:1 and 7:1 (dummy spans) |
| q07 fewer names | rc 0, `3` |
| q01, q02, q05 tuple `match` | check rc 0, run rc 101 (O-LF-01) |
| p23 `for` over dict | E0102 on `len` at 8:5 |
| fm02 `axon fmt` | prints `__tup_34` lets |
| m1, examples/modular/agent.ax | E0901 without `AXON_PATH`; rc 0 with it |
| all-tracked sweep | 700 files, rc0 613, 7.9 s |
| native_shapes_parity.sh | 55 OK, 3.8 s |

Not observed: anything marked `[INFERENCE]`; no cargo build or test was run.
