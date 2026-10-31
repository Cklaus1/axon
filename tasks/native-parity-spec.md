# native-parity - DRAFT spec

**Status: BUILT 2026-10-31 on np/int 7e4a1489 (NP-1..NP-7; authorized 2026-10-31 after Step 1 review, human: "do it all" on the integrator's recommendations, NH-1..NH-3 (a)).**
`[REVISED: integrator 2026-10-31]` Step 0 (`emit_msg_panic`, `current_src_fn`, the `gate.sh:1207` filter) is built by lane A as its first
commit; lane B merges it before B2. checker-soundness-3 is not drafted yet, so native-parity's `infer.rs` hunk lands first and
checker-soundness-3 adapts to it (reverses the §6 cross-spec order).

### Step 1 review

Two reviews of the draft: `agent://AuditNative` (NPA-1..8) and `agent://DevloopNative` (NPD-1..8). Before deciding, I re-ran AuditNative's
probes (`/var/tmp/r51-tmp/NPA/{a1,a2,a3,a4,i1,i3,r1,r2,r6,bm}.ax`, all with `par.sh` on the 77cf51fc binary) and read every line cited.
DevloopNative's timings come from its own build and are quoted as its measurements.

| review | blocker | major | minor | accepted | rejected |
|---|---|---|---|---|---|
| NPA | 1 | 1 | 6 | 8 | 0 |
| NPD | 0 | 5 | 3 | 8 | 0 |

| id | disposition (one line) |
|---|---|
| NPA-1 | accepted (blocker): a1, a2, a4 re-run: check 0, interp `2`, native build fails ICmp; a3 DIFF `2`/`1`. A literal cast down would turn those refusals into wrong answers -> NP-1 step 2 widens the subject by semantic signedness and never truncates; M4122 re-targeted, M4136 added |
| NPA-2 | accepted: i3 re-run DIFF (`in-method\|5` / `5`), i1 refused E0910 today; `mono.rs:372-378` sends every FnDef to `fns`/`generic_fns`, so the `other_items` FnDef branch (`:598-602`) is dead and impl bodies are never renamed -> NP-3 step 2 walks impl methods |
| NPA-3 | accepted: `infer.rs:2818`, `:3350`, `:3374`, `:3883-3886`, `:3960` read; `infer_program(&mut self, program: &Program)` (:2754), the only mutation `program.items.insert` (`main.rs:7750`) -> callers listed, the `[INFERENCE]` replaced with this evidence |
| NPA-4 | accepted: r6 (one type) and r2 (two types) refuse E0910 `cnt__unknown`; r1 control SAME -> §0 entry, NP-3 acceptance excludes recursive generics |
| NPA-5 | accepted: bm re-run: interp `0\|1` exit 101 "outside a loop in `go`", native `0\|1\|2\|3` exit 0; `emit_fn` has the unmangled `f.name` (`codegen/mod.rs:1492`, `:1841-1851`) -> NP-4 names the source fn; residual kept only for mono-mangled generics |
| NPA-6 | accepted (integrator: take it): an all-irrefutable tuple pattern keeps `true_val` without inspecting the subject; E0910 only for a refutable component on a non-struct subject |
| NPA-7 | accepted: t21 was not in the case list -> added, so M4121's marker is reachable |
| NPA-8 | accepted: the IR test also asserts the last arm's branch targets `%match_miss` and that the module verifies |
| NPD-1 | accepted: `v022_attack_markers.py:1-9` (the LAST panic must match) -> §4 rule: a shared killer checks every probe, then panics once listing each diverging probe |
| NPD-2 | accepted: the `both` helper's build assert (`integration_fixtures.rs:2709-2713`) carries no `ATTACK:` text -> a build failure of a non-control probe is reported as `ATTACK: ... [<probe>] (build: ...)` |
| NPD-3 | accepted: `gate.sh:1207` filters `--lib codegen::builtin_externs`, and codegen is `#[cfg(feature = "codegen")]` (`lib.rs:12`) -> the integrator widens the filter to `codegen::` |
| NPD-4 | accepted: the cost table takes DevloopNative's measured rows; `parity_all` 21m42s and `--strict` 43m24s are `gate.sh:1214-1215` |
| NPD-5 | accepted: `gate.sh:99-100` (`--lane` cannot combine with `--strict`) -> per-lane brief rewritten; `parity_all.sh` runs once in the integrator's gate |
| NPD-6 | accepted (integrator note): `build-loop-r51.md:90-92` ("incremental for lane feedback, the full run at the final head is the evidence"), shard 0 alone (`:108-117`) -> gate row amended |
| NPD-7 | accepted: lanes append their own killers; the integrator resolves append conflicts |
| NPD-8 | accepted: `asi-replay-spec.md:310-311`, `:411`, `:431` confirm asi-replay B1 edits `env_registry.rs:40` and `CLAUDE.md:272` and merges after NP lane C -> §6 |

Design changes: NP-1 (subject widening, irrefutable shortcut, M4136), NP-3 (impl-method bodies, caller list), NP-4 (source fn name),
§4 (shared-killer protocol, 17 rows), §5/§6 (costs, lane brief, gate row, merge order). Needs-human: decided by the human (§8).

Base: `main` 77cf51fc. Every anchor below was read at 77cf51fc in `/home/cklaus/projects/axon-lanes/specs3`. Every reproduction was run with
`/var/tmp/r51-tmp/main77-bin/axon` (`axon --version` = `axon 0.1.0 (77cf51fc)`, codegen-capable), `AXON_PATH=examples/stdlib`.
A probe is run by `/var/tmp/r51-tmp/NativeParity/par.sh F.ax`: `axon check F`, then `axon run F` (interpreter, the reference), then
`axon build F -o exe` (default opt level, no `--release`) and `./exe`, each with a timeout (run 60 s), comparing stdout, exit code and the
first stderr line. Probe sources: `/var/tmp/r51-tmp/ImplAuditCS2/` (the O-CS2-01 probes; all 180 re-run, results in
`/var/tmp/r51-tmp/NativeParity/implaudit_all.txt`) and my own under `/var/tmp/r51-tmp/NativeParity/{t,u,x,p,m,b}/`. `[INFERENCE]` marks
anything not observed.

The defect class: **a program that `axon check` accepts, whose native binary gives a different answer from `axon run` (wrong stdout, exit 0
where the interpreter panics, or a hang).** The interpreter is the reference (`AGENTS.md:31-33`: "native codegen follows the interpreter,
never the reverse"), so in every item below the native side changes, except NP-6 (interpreter vs its own documentation) and NP-7 (docs).

## 0. Source entries

| item | source (`tasks/opportunities.md`) | one line |
|---|---|---|
| NP-1 | O-CS2-01 (:2954) | A tuple pattern always matches natively: its literal, enum and nested sub-patterns are never tested |
| NP-2 | O-CS2-01 (:2957, h31/h32) | A match with no matching arm falls through natively (exit 0, a zero value) instead of panicking |
| NP-3 | O-CS2-01 (:2956-2957, w31) | A generic fn called at two or more types, or from an `impl` method [REVISED: NPA-2]: the call is silently dropped natively |
| NP-4 | found by this spec's native sweep (§5) | `break`/`continue` outside a loop in a callee or closure: native continues, the interpreter panics |
| NP-5 | O-CS2-01 (:2957, h11) | `arr_range` over a span wider than i64 returns `[]` natively; an allocation refusal is reported as "stack overflow" |
| NP-6 | O-DT-01 (:2941) | `min_i32`/`max_i32` do not truncate in the interpreter; native and the doc strings do |
| NP-7 | O-GAP-07 (:2901) | Recursion depth: interpreter 6000, native unbounded up to the OS stack; `arr_max_i64([])` panics |

### Out of scope (logged, not dropped)

The integrator logs each of these as an opportunity entry (`O-NP-01..`) in `tasks/opportunities.md`.
- **Native false refusals that are not wrong answers.** In the §5 sweep, 125 tracked programs fail `axon build` while `axon run` works
  (113 E0910, 7 IR verification failures, 3 E0701, 2 compiler aborts exit 134 on `vm_depth/nest/{compile_deep,expr}.ax`). The IR failures
  include `headroom/programs/{paging,semver,units}.ax` ("Function return type does not match operand type of return inst!") and
  `vm_match/chan.ax` ("Incorrect number of arguments passed to called function!"). A `Result` component inside a tuple pattern
  (t22, ImplAudit w14) fails IR verification ("Call parameter type does not match function signature!"). Generic shapes refused E0910:
  g01 (result used), g05 (tuple of type params), g07 (generic struct), g09 (generic called from a generic body), g10, d15. A build refusal
  is the loud failure mode, so none of these is a soundness item.
- [REVISED: NPA-4] **Recursive generic fns** are refused E0910 natively at one or more types: the self-call records an instantiation with `T`
  unresolved, giving a `__unknown` instance (`/var/tmp/r51-tmp/NPA/r6.ax` one type, `r2.ax` two types: "fn `cnt__unknown` has parameter `x`
  of type unknown"). NP-3's site keys cover only the original non-generic and impl bodies, so this stays a loud refusal.
- **Other interpreter/native differences in the sweep.** `fixtures/ai_extract_generic.ax`: interpreter exit 101 "value of type ai_extract is
  not callable", native exit 0 with output (not classified: which side is right needs the AI-builtin owner). `fixtures/phase24_concurrency.ax`:
  interpreter 101 "recv on an empty channel - the interpreter runs `spawn` bodies eagerly", native hangs (timeout 124); this is the
  documented eager-spawn limitation.
- **`ast::Pattern::Struct` fallback** (`codegen/match_pat.rs:420`, same line as the tuple bug). The parser builds `Pattern::Struct` only for an
  `Enum::Variant` path (`parser.rs:3693-3736`); plain-struct patterns do not parse (probe t/s02: E0000). The enum case is tested earlier in
  `emit_pattern_test` [INFERENCE: the fallback is unreachable]; NP-1 leaves it unchanged.
- **The five inline copies of the `__axon_msg_panic` call** (`codegen/expr.rs:1312`, `:4709`, `:5664`, `:8332`, `codegen/builtins.rs:190`) are
  not migrated to the new helper (§3 step 0); unrelated restyle.
- **O-CS2-02** (h24 wrapper lambda; payload-literal exhaustiveness) belongs to checker-soundness-3. NP-2 is independent of it (§3).

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | NP-1, NP-2, NP-3 | Silent wrong answers on common shapes (tuple match, non-exhaustive match, a generic helper used at two types), including a hang (NP-1 `while let`) |
| P2 | NP-4, NP-5 | Silent wrong answers on rare shapes (loop control escaping a frame; an `arr_range` span above i64::MAX) |
| P3 | NP-6, NP-7 | NP-6 is a builtin behaviour change (needs human, §8 NH-1). NP-7 is docs only; its code options are NH-2/NH-3 |

## 2. Items

### NP-1 (P1) Tuple patterns always match natively - O-CS2-01

**Problem.** `emit_pattern_test` answers "matches" for every tuple pattern without looking at the components:
`codegen/match_pat.rs:419-420`:
`// Plain struct / tuple patterns: phase 1 — always match (wildcard semantics).` /
`ast::Pattern::Struct { .. } | ast::Pattern::Tuple(_) => true_val.into(),`. The bindings are still bound
(`emit_pattern_bindings`, `match_pat.rs:706-760`, extracts each element), which is why binding-only tuple patterns are right. The first arm
whose pattern is a tuple is therefore always taken. `while let` uses the same test (`codegen/expr.rs:3044`), so a `while let (true, m) = ..`
never exits. A literal compare whose widths differ (an `i32`/`u8` scrutinee against the i64 literal, `match_pat.rs:190-195`
`build_int_compare(IntPredicate::EQ, s, l, ..)`) fails IR verification; that shape becomes reachable inside tuples once the components are
tested, so it is fixed here too.

**Root cause: proven** by the line above and by every row below: the native answer is always the first tuple arm's body.

**Reproduction** (check rc 0 for every row; interpreter output vs native output, `|` = newline):

| probe | shape | interp | native |
|---|---|---|---|
| ImplAudit v22 | `(0, 0)`, `(0, y)`, `(x, _)` on `(3, 4)` | `3` | `0` |
| ImplAudit v25 | four `(bool, bool)` literals on `(true, false)` | `2` | `3` |
| ImplAudit w08 | `(C::R, C::R)`, `(C::R, C::G)`, `(C::G, _)` | `2` | `0` |
| ImplAudit w09 | nested `((0, true), _)`, `((n, _), s)` | `4` | `0` |
| t01, t02, t03, t24 | int literal in a param / local / expression / returned tuple | `7\|4`, `4`, `5`, `7` | `100\|100`, `100`, `100`, `100` |
| t06, t20 | str literal component | `10\|0`, `2` | `5\|5`, `1` |
| t07, u07 | bool literal, `(true, _)` then `(_, true)` then `_` | `-5`, `b\|none` | `5`, `both\|both` |
| t08, t27 | nested tuple literal; 3-tuple | `6`, `9` | `1`, `0` |
| t09, t26 | unit enum variant component | `15`, `3` | `5`, `1` |
| t10, t23 | `(None, k)` first; enum payload in tuple | `105`, `107` | `1`, `1` |
| t14, t15 | `Some((0, y))`; tuple in an enum field | `300`, `300` | `4`, `4` |
| t17 | statement position | `true` | `zero-true` |
| t21, u04 | wildcard/binding first, literal second | `3`, `5` | `100`, `2` |
| t25, u09 | tuple match in a loop | `21`, `19` | `4`, `10` |
| u03 | tuple match inside a lambda | `20` | `3` |
| u01 | `while let (true, m) = step(n)` | `3` | hangs, killed at 60 s (exit 124) |
| u05, t/s01, t/s04 | `match n { 0 => .., _ => .. }`, `n: i32` / `u8` | `100`, `200`, `100` | build fails: "IR verification failed: Both operands to ICmp instruction are not of the same type!" |
| [REVISED: NPA-1] NPA a1, a2, a4 | `x: u8` vs `256`; `i32` vs `4294967297`; `u8` vs `-1` (on 255) | `2`, `2`, `2` | build fails: same ICmp error |
| NPA a3 | `(4294967297, _)` on an `(i32, i64)` holding `(1, 0)` | `2` | `1` |

Controls, identical today: t04 (all wildcards) `9`, t05 (bindings only) `34`, t11 (`(x, y) if x > y`) `9|9`, t12 (`let (a, b) = ..`) `34`,
u06 (`Some(0)` literal in an `Option` payload, no tuple) `100|4`, ImplAudit v26, w10, w11.

**The change** (`codegen/match_pat.rs`):
1. Replace the `Tuple(_)` half of :420 with an arm `ast::Pattern::Tuple(pats)`. [REVISED: NPA-6] If every component is
   `pattern_is_irrefutable` (:425), return `true_val` without inspecting the subject (today's answer; a binding on a subject with no layout keeps
   its deferred poison, :724-729). Otherwise, if `subject` is a `StructValue`, start with `acc = true`, and for each component `i` extract element
   `i` (`build_wrappers::w_extract_value`, as `emit_pattern_bindings` does at :711-716) and set
   `acc = self.emit_test_when(acc, &pats[i], |s| s.emit_pattern_test(&pats[i], elem_i, elem_sem_i))` (`emit_test_when`, :435-455, already
   short-circuits and skips irrefutable components). Return `acc`. A nested tuple recurses through the same arm. If `subject` is not a
   `StructValue`, `record_error` an E0910 ("native codegen cannot test this tuple pattern; run under `axon run`") and return `false_val` - never
   `true`.
2. [REVISED: NPA-1, blocker] `emit_pattern_test` (:177) gains a `sem: Option<&Type>` parameter: `emit_match` passes its `subject_sem_ty`
   parameter; the tuple arm passes element `i` of `Type::Tuple` (narrowed as `emit_pattern_bindings` does, :706-710); every other caller
   (`codegen/expr.rs:3044`, `match_pat.rs:303`, `:333`, `:363`, `:486`) passes the type it has at hand, else `None`. In the `Literal` arm
   (:190-195) the literal is **never truncated**. When the subject's int width is below 64 bits, widen the subject to i64 - `zext` when `sem`
   is `u8`/`u16`/`u32`, `sext` when it is `i8`/`i16`/`i32` - and compare at i64; a literal outside the subject's range then never matches, as in
   the interpreter (a1, a2, a4 -> `2`). `bool` is `i1` on both sides and is untouched. A narrower subject with `sem` = `None` records E0910 and
   returns `false`: that shape fails IR verification at build today, so this adds no refusal.
3. `Pattern::Struct` keeps its current fallback (§0).
4. [REVISED: build lane A] `pattern_is_irrefutable` also answers `true` for a tuple whose components are all irrefutable (nested
   `((a, b), c)`), so such a tuple keeps `true` on any subject and `emit_test_when` skips it; the old non-recursive form would have refused
   E0910 on a binding-only nested tuple over a subject with no layout, which matched before. The `Ok`/`Err` tests read the `[N x i8]`
   payload at the `Result`'s `Ok`/`Err` sem type (`extract_result_payload`, as the bindings do) before testing the inner pattern: without
   it `Ok(0)` compared an array with an int and fell to the literal arm's `_ => true` (probe `okl`: `Ok(0) => 0, Ok(n) => n, Err(_) => 1`
   on `Ok(5)`: interp `5`, 77cf51fc native `0`; added to the NP-1 killer; x03 needs it too). That fallback,
   a literal pattern on a value native cannot compare, now records E0910 and returns `false` instead of `true`. S608 after the change: no
   native row differs from `base-77cf51fc.txt` (two interpreter-only hash changes, `persistent_bandit.ax`, `mobile/lifecycle.ax`, both
   BUILDFAIL in the baseline and nondeterministic interpreter output).

**Fail-first test.** `integration_fixtures`: `tuple_patterns_test_every_component_natively` (`#[cfg(feature = "codegen")]`, built on the
`both` helper of `refinements_on_variant_fields_and_lambda_params_are_enforced_natively`, `tests/integration_fixtures.rs:2687-2715`, with the §4
shared-killer protocol): v22, v25, w08, w09, t06, t09, t14, t17, t21, t27, u01 (native run under a 10 s timeout), u05, a1, a2, a3, a4 each
assert interpreter and native agree on exit code and stdout [REVISED: NPA-1, NPA-7]; before the change every case fails (u01 by timeout; u05,
a1, a2, a4 at build, reported per NPD-2 as `ATTACK: ... [<probe>] (build: <stderr>)`).

**g01 rows.**

| id | guard the mutation removes | file | killing test |
|---|---|---|---|
| M4120 | the `Tuple(pats)` arm returns `true_val` again (component tests dropped) | codegen/match_pat.rs | `tuple_patterns_test_every_component_natively` (v22 marker) |
| M4121 | only component 0 is folded into `acc` | codegen/match_pat.rs | same (t21 marker: literal in component 1) |
| M4122 | [REVISED: NPA-1] the subject is not widened (compared at its own width, or the literal truncated to it) | codegen/match_pat.rs | same (a1 marker: build failure or `1`) |
| M4136 | [REVISED: NPA-1] an unsigned subject is sign-extended | codegen/match_pat.rs | same (a4 marker: 255 as `u8` sign-extends to -1 and matches) |

**Acceptance.** Every row of the table prints the interpreter's output natively (a1-a4 print `2`); u01 exits 0 printing `3`; the controls are
unchanged; t22 either builds and matches the interpreter or still refuses at build (never a silent wrong answer). No literal is ever cast to a
narrower type.

**False-refusal guard.** The §5 native sweep S608: no SAME row changes and no SAME row becomes BUILDFAIL. `scripts/all_examples_parity.sh`
and `scripts/vm_parity.sh` green.

**Gate stages.** Lane A brief (§5); the integrator's `parity_all.sh` inside `gate.sh --strict`.

**Cost.** One codegen build per edit cycle, `integration_fixtures` run, 4 g01 rows (native rows, 15-45 s each measured on the last batch).

### NP-2 (P1) A match with no matching arm falls through natively - O-CS2-01 h31/h32

**Problem.** The last arm's "no match" edge branches to the merge block (`match_pat.rs:74-110`, `miss_preds`), and the phi gets a zero for it:
`match_pat.rs:164-169` ("Every "no arm matched" edge also flows to merge_bb ... the value is never used. `let zero = val_ty.const_zero();`").
The value is used: the function returns it. In statement position there is no phi and execution simply continues. The interpreter panics
`no match arm matched` (`interp/eval.rs:505`; VM `interp/vm/mod.rs:2417`), exit 101.

**Which side is right (task question 2).** The interpreter, per `AGENTS.md:31-33`. Native must panic identically: exit 101 and the stderr
line `axon: panic: no match arm matched`. `__axon_msg_panic` (`axon-rt/src/lib.rs:3547`) prints `axon: panic: {msg}` and exits
`RUNTIME_PANIC_EXIT_CODE` = 101 (`:3479`), so it gives byte-identical stderr. The same rule decides h11 (NP-5).

**Reproduction** (check rc 0 for each):

| probe | interp | native |
|---|---|---|
| ImplAudit h31 `match o { Some(0) => 0, None => 1 }` on `Some(5)` | exit 101, `axon: panic: no match arm matched` | exit 0, prints `0` |
| ImplAudit h32 `E::A { s: "x" }`, `E::B` on `E::A { s: "y" }` | exit 101, same | exit 0, prints `0` |
| x/x03 `Ok(0) => 0, Err(_) => 1` on `Ok(5)` | exit 101, same | exit 0, prints `0` |
| u/u02 statement position, then `println("after")` | exit 101, same | exit 0, prints `after` |

Non-exhaustive int, bool, tuple and guarded matches are already refused at check (E0304; x01, x02, x04, x05, ImplAudit h30, h34, h35), so
today's reachable shapes are payload literals. If checker-soundness-3 refuses those at check, the native miss path remains for any shape a
later checker change misses; the native fix is defence in depth, so its killing test does not depend on a check-clean program.

**The change.**
1. Step 0 helper (§6): `pub(super) fn emit_msg_panic(&mut self, msg: &str)` in `codegen/mod.rs`: hosted, call `__axon_msg_panic(msg, len)`
   (declared unconditionally at `codegen/builtins.rs:3597-3604`) then `unreachable`; freestanding (`self.freestanding`, `mod.rs:355`), call a
   trap synthesized by `synthesize_freestanding_trap("__axon_msg_trap", void(), b'P')` (`codegen/expr.rs:984`; precedent
   `__axon_bounds_panic`, `builtins.rs:3588`) then `unreachable`.
2. In `emit_match`, unless the last arm is unguarded and `pattern_is_irrefutable` (`match_pat.rs:425`), create a `match_miss` block. The last
   arm's false edges (test and guard) branch to it instead of `merge_bb`; it calls `emit_msg_panic("no match arm matched")`. Delete the zero
   incoming (`:164-169`) and the `miss_preds` bookkeeping that feeds it.

**Fail-first tests.**
- `codegen::match_pat::tests::a_match_miss_edge_panics` (new `#[cfg(test)] mod tests` in `match_pat.rs`, built like
  `freestanding_trap_tests`, `codegen/expr.rs:12308-12322`): parse h31's source with `parse_source`, `Codegen::new` + `emit_program`, assert the
  IR of `f` contains a `match_miss` block calling `@__axon_msg_panic`, that the last arm's conditional branch targets `label %match_miss`, that
  no `match_val` phi has a `zeroinitializer`/`0` incoming from it, and that `module.verify()` is `Ok` [REVISED: NPA-8]. Fails today (no such
  block). It needs no checker, so it holds whatever checker-soundness-3 does. Gated by the widened `gate.sh:1207` filter (§6, NPD-3).
- `integration_fixtures`: `a_match_with_no_matching_arm_panics_natively_like_the_interpreter`: h31, h32, x03, u02 -> both engines exit 101
  with the same stderr. Fails today (native exit 0). If a probe stops passing `axon check` (checker-soundness-3), the case asserts both
  `axon run` and `axon build` exit 2 instead, and the IR test above stays the killer.
- [REVISED: build lane A] The IR test parses h31's `f` with `main` reduced to `f(Some(5))`: a bare `Codegen` (no builtin registry) records
  E0910 for `println`. Its sibling `an_irrefutable_last_arm_emits_no_match_miss` covers the acceptance line below (`_` last arm: no
  `match_miss`, module verifies); that unguarded irrefutable last arm now branches straight to its body.

**g01 rows.**

| id | guard the mutation removes | file | killing test |
|---|---|---|---|
| M4123 | the last arm's false edge branches to `merge_bb` again | codegen/match_pat.rs | `codegen::match_pat::tests::a_match_miss_edge_panics` (target `'--lib'`, crate `axon-core`) |
| M4124 | `match_miss` built but `emit_msg_panic` not called (falls into `unreachable` only) | codegen/match_pat.rs | same |
| M4125 | `emit_msg_panic` hosted branch omits the call | codegen/mod.rs | `a_match_with_no_matching_arm_panics_natively_like_the_interpreter` (h31 marker) |

**Acceptance.** The four probes exit 101 natively with `axon: panic: no match arm matched` on stderr and nothing on stdout that the
interpreter does not print. A match ending in an unguarded `_`/binding arm emits no `match_miss` block.

**False-refusal guard.** S608 identical except NP-4's expected rows; R17 freestanding tests in `integration_fixtures` (`:1837-1900`) green.

**Gate stages / cost.** As NP-1; 3 g01 rows.

### NP-3 (P1) A generic fn called at two or more types, or from an impl method: the calls are dropped natively - O-CS2-01 w31

**Problem.** [REVISED: NPA-2] Monomorphisation renames a call site in a fn body only when the generic fn has exactly one distinct
instantiation in the program, and never renames a call site in an `impl` method body. `mono.rs:576-591` ("If a generic function has multiple
distinct instantiations, call-site renaming for it isn't yet supported — leave as-is and let codegen warn"; `if v.len() == 1 { Some(..) } else
{ None }`); the rename loop covers `prog.fns` and `other_items`' `Item::FnDef` (`:596-602`) only, but `MonoContext::new` sends every
`Item::FnDef` to `fns` or `generic_fns` (`:372-378`), so that branch is dead and `Item::ImplBlock` (`ast.rs:37`) methods are untouched. The
emitted program contains `put__i64` and `put__str` but the call still says `put`, which names no function. `emit_call` then resolves nothing,
and `codegen/expr.rs:12061` `let fn_v = maybe_fn_v?;` returns `None`: the call vanishes. The honest-error guard just above (`:12034-12058`)
fires only for BUILTINS names. Nothing warns. In statement position the program runs on (wrong output, exit 0); when the result is used, a
later "produced no value" E0910 refuses the build (g01, NPA i1).

**Root cause: proven** by those two lines plus the IR: `axon build p/d14_generic_id_two_types.ax --emit-llvm` defines `@put__i64` and
`@put__bool`, and `@main` contains no call to either (`/var/tmp/r51-tmp/NativeParity/p/d14.ll:1487-1510`). The bug is not dict-specific.

**Reproduction** (check rc 0 for each; `/var/tmp/r51-tmp/NativeParity/p/`):

| probe | shape | interp | native |
|---|---|---|---|
| ImplAudit w31 | `fn put<V>(d: Dict<V>, ..)` at two value types | `2` | `0` |
| d12, d13 | `put<V>(d: Dict<str, V>, ..)` at `i64` and `str` | `1\|1` | `0\|0` |
| d14 | same, `put` prints | `put x\|put y\|1\|1` | `0\|0` |
| g02 | `fn log<T>(x: T, msg: str) { println(msg) }` at `i64` and `str` | `one\|two\|end` | `end` |
| [REVISED: NPA-2] NPA i3 | `tell<T>` at one type, called from an `impl` method, result discarded | `in-method\|5` | `5` |
| NPA i1 | same, result used | `in-method\|7` | build refused E0910 "could not lower the body of `go` (`S__go`)" |
| g06 | same inside `if` / `while` | `one\|loop\|loop` | (empty) |
| g08 | `fn tell<T>(x: T) -> i64`, result discarded, two types | `told\|told` | (empty) |

Controls, identical today: d01-d11, d16, d17 (one instantiation each), g03 (`log` at one type), g04 and g12 (two element types of `[T]`
whose instantiations both mangle to `__unknown`, so they dedup to one), g11.

**The change.**
1. **Fail-safe** (`codegen/expr.rs:12060-12061`): when `maybe_fn_v` is `None` and the callee is an `ast::Expr::Ident`, `record_error` an E0910
   ("native codegen could not resolve the call to `{name}` (a generic fn used at several types?); building it would silently drop the call.
   The interpreter supports it; run under `axon run`.") and return an i64 zero, as the builtin guard does. A dropped call can then only be a
   refusal.
2. **Per-site rename.** `InferCtx::call_instantiations` (`infer.rs:759`, push at `:2044-2045`) records, with each `(name, type_args)`, the
   site key `callee.as_ref() as *const Expr as usize` (precedent: `method_receiver_vars`, keyed by the receiver's address, `infer.rs:789`).
   `drain_instantiations` (`:2911`) returns `(name, args, site)` and keeps its dedup on `(name, args)` for `request_fn` while returning every
   site. `mono::monomorphise` (`mono.rs:563`) builds `site -> mangled` and renames (site map first, then `single_rename`) while walking the
   **original** program's non-generic `FnDef` bodies (cloned by `MonoContext::new`, `output.fns.push(f.clone())`, `mono.rs:377`) and
   [REVISED: NPA-2] every method body of each `Item::ImplBlock` in `other_items`; the dead `Item::FnDef` branch (`:598-602`) is deleted. A call
   still unresolved hits step 1. Callers updated [REVISED: NPA-3]: `main.rs:3190-3192`, `:3288-3289`, `:8033-8036`; in `infer.rs` the
   substitution loop `:2818` (`for (_, type_args) in &mut self.call_instantiations`) and the unit tests at `:3350`, `:3374`, `:3883-3886`,
   `:3890` (`drain_instantiations_deduplicates`) and `:3960`.
   Address stability [REVISED: NPA-3, was `[INFERENCE]`]: `infer_program(&mut self, program: &Program)` (`infer.rs:2754`) infers through
   borrows; every later pass takes `&Program`; the only mutation before codegen is `program.items.insert` (`main.rs:7750`), which moves `Item`s,
   not the boxed `Expr` nodes; `build_ir_and_link` receives that same `program` (`main.rs:8033-8036`). If a key still misses, step 1 refuses.

**Fail-first tests.**
- `integration_fixtures`: `a_generic_fn_used_at_two_types_runs_every_call_natively`: d12, d14, g02, g06, g08, w31, i3, i1 [REVISED: NPA-2] ->
  interpreter stdout == native stdout, both exit 0. Fails today (native drops the calls; i1 refused at build, reported per NPD-2).
- `codegen::expr::tests::an_unresolved_user_callee_is_refused_e0910` (codegen lib unit test, freestanding_trap_tests style): `Codegen` over
  `fn main() -> i64 { ghost(1)\n 0 }` (checker bypassed) -> `codegen_errors()` contains `E0910`. Fails today (empty).

**g01 rows.**

| id | guard the mutation removes | file | killing test |
|---|---|---|---|
| M4126 | the site map is ignored (only `single_rename` applies) | mono.rs | `a_generic_fn_used_at_two_types_runs_every_call_natively` (d12 marker) |
| M4127 | infer records no site key | infer.rs | same (g02 marker) |
| M4128 | the unresolved-Ident E0910 fail-safe | codegen/expr.rs | `codegen::expr::tests::an_unresolved_user_callee_is_refused_e0910` (`'--lib'`) |

**Acceptance.** The eight probes match the interpreter natively; controls unchanged; a call that is still unresolved refuses with E0910.
Recursive generic fns stay refused (§0, NPA-4) and are not in scope.

**False-refusal guard.** S608: no SAME row becomes BUILDFAIL (this is the measurement for step 1's reach). `integration_fixtures`,
`all_examples_parity.sh`. `axon check` output is unchanged for every S608 file (no checker change).

**Gate stages / cost.** As NP-1, plus `cargo test -p axon-core --no-default-features --lib infer`; 3 g01 rows.

### NP-4 (P2) `break`/`continue` outside a loop in a callee or closure: native runs on

**Problem.** `codegen/expr.rs:736-747`: `Break`/`Continue` branch to `self.loop_stack.last()` if there is one, and otherwise emit nothing
and return `None`; the code after the `break` runs. `emit_lambda` (`codegen/expr.rs:3630`) does not clear `loop_stack` (`mod.rs:254`), so a
lambda defined inside a loop branches to a block in another function. The interpreter panics at the frame edge with
`` `break`/`continue` outside a loop in `stop` `` or `... in a closure` (`interp.rs:1396-1419`, `FrameEdge::loop_control_escape`); R51 S0b
pins this for tree and VM (`tests/cli_run.rs:40318`, fixtures `tests/fixtures/r51_s0b/`), not for native.

**Reproduction** (check rc 0 each):

| probe | interp | native |
|---|---|---|
| `fixtures/r51_s0b/escape_fast_local_int.ax` | `1\|2\|3`, exit 101, `` axon: panic: `break`/`continue` outside a loop in `stop` `` | `1\|2\|3\|4\|5\|6\|unreachable: the loop ended silently`, exit 0 |
| `fixtures/r51_s0b/escape_closure.ax` | `0\|1`, exit 101, `... in a closure` | `0\|1\|2\|3\|unreachable: the loop ended silently`, exit 0 |
| `fixtures/r51_s0b/escape_fast_inline.ax` | `0\|10\|20`, exit 101, ``... in `skip` `` | `0\|10\|20\|30\|40\|50\|unreachable: the loop skipped silently`, exit 0 |
| b/b02 `continue` in a fn called from `while` | `1`, exit 101 | `1\|2\|3\|4\|after`, exit 0 |
| b/b01 `break` in a lambda defined inside `for` | `0\|1`, exit 101 | build fails: "IR verification failed: Referring to a basic block in another function!" |

**The change.** (1) With `loop_stack` empty, `Break`/`Continue` call `emit_msg_panic` with the interpreter's text: edge = "a closure" when
`fn_val`'s name starts with `__lambda_` (`expr.rs:3637`), else `` `{name}` `` where [REVISED: NPA-5] `name` is the source fn name: a new
`Codegen` field `current_src_fn: String`, set from `f.name` at the top of `emit_fn` (`codegen/mod.rs:1492`; `f.name` is the unmangled name for
an impl method, `mod.rs:1841-1851`) and saved/restored around `emit_lambda`. An impl method then says `go` like the interpreter (NPA bm). A
monomorphised generic fn's `FnDef` is renamed by mono (`put__i64`) where the interpreter names `put`: exit code equal, stderr differs;
documented residual. Predicate and handler-arm edges are unreachable natively (AuditNative: `escape_handler_arm.ax` refused E0910,
`escape_predicate.ax` refused E1211 at check) and are not changed. (2) `emit_lambda` takes `loop_stack` (`std::mem::take`) before the body and
restores it after.

**Fail-first test.** `integration_fixtures`: `loop_control_never_leaves_its_frame_natively`: the three r51_s0b escape fixtures, b01, b02, NPA
bm [REVISED: NPA-5] -> both engines exit 101, identical stdout and stderr. Fails today (b01 at build, reported per NPD-2).

**g01 rows.** M4129: the empty-`loop_stack` panic is skipped (codegen/expr.rs; killer above, b02 marker). M4130: `emit_lambda` keeps the outer
`loop_stack` (codegen/expr.rs; same killer, b01 marker).

**Acceptance / guard.** S608: the three escape rows go DIFF -> SAME; nothing else changes. `vm_parity.sh` (S0b fixtures) green.
**Cost.** 2 g01 rows.

### NP-5 (P2) `arr_range` over a huge span returns `[]`; allocation refusal misreported - O-CS2-01 h11

**Problem.** `emit_arr_i64_range` (`codegen/expr.rs:7788-7814`) computes `raw = end - start` with a wrapping sub (:7795) and tests `raw > 0`.
For `arr_range(-9223372036854775807, 9223372036854775807)` the difference wraps to -2, the count is 0, and native returns `[]`. The
interpreter computes the length in i128 (`interp/builtins.rs:1810-1820`) and panics through `alloc_array_err` (`:447-449`). Separately,
`emit_arr_i64_build` multiplies `count * elem_size` with a wrapping mul (`expr.rs:7718`) and `emit_malloc` (`codegen/mod.rs:685`) never checks
for null, so an allocation the allocator refuses dies in the SIGSEGV handler (`axon-rt/src/lib.rs:1362`, 1372), which says "stack overflow".

**Reproduction.**

| probe | interp | native |
|---|---|---|
| ImplAudit h11 (span 2^64-2) | exit 101, `axon: panic: arr_range: cannot allocate an array of 18446744073709551614 elements (memory allocation failed because the computed capacity exceeded the collection's maximum)` | exit 0, prints `0` |
| p/r01 `arr_range(0, i64::MAX)`, p/r02 (2^62), p/r03 `arr_repeat(_, i64::MAX)` | exit 101, `arr_range`/`arr_repeat: cannot allocate ...` | exit 101, `axon: panic: stack overflow (recursion too deep) — native uses the OS stack limit; ...` |
| m/r05 `arr_range(0, 2^40)` (8 TiB; no ulimit) | exit 101, `... 1099511627776 elements (memory allocation failed because the memory allocator returned an error)` (run with `ulimit -v 16000000`) | exit 101, "stack overflow ..." |
| p/r04 `arr_range(-3, 3)` (control) | `6` | `6` |

**Which side is right.** The interpreter (AGENTS.md:31-33). Exit 101 already matches for the allocation cases; the message must match too.

**The change** (codegen/expr.rs + one axon-rt fn).
1. `emit_arr_i64_range`: `pos = end SGT start`; `count = end - start` read as unsigned (the wrapped difference equals the true difference for
   every `end > start`).
2. Before `emit_malloc` in `emit_arr_i64_build`/`emit_arr_build_elem` (`expr.rs:7683`, `:7700`): if `count > isize::MAX / elem_size` (unsigned
   compare), call `__axon_alloc_array_panic(name, count, 0)`; after `malloc`, if the pointer is null, call it with kind 1. `name` is the
   builtin (`arr_range`, `arr_repeat`) passed down by the two callers (`expr.rs:11447`, `:11455`).
3. `axon-rt/src/lib.rs`: `#[no_mangle] pub extern "C" fn __axon_alloc_array_panic(name_ptr: *const u8, name_len: i64, count: u64, kind: i64) -> !`
   printing `axon: panic: {name}: cannot allocate an array of {count} elements (memory allocation failed because {reason})`, reason = `the
   computed capacity exceeded the collection's maximum` (kind 0) / `the memory allocator returned an error` (kind 1), exit 101; declared next
   to `__axon_msg_panic` (`codegen/builtins.rs:3597`). These are the std `TryReserveError` texts the interpreter prints; the rt test pins them
   against `Vec::<i64>::new().try_reserve_exact(..)`'s `Display` so a toolchain change cannot drift them. Freestanding: the `__axon_msg_trap`.

**Fail-first test.** `integration_fixtures`: `huge_arrays_panic_natively_like_the_interpreter` (Linux only; the native binary and `axon run`
both under `ulimit -v 16000000`): h11, r01, r03, r05 -> exit 101 and identical stderr; r04 control. Fails today (h11 exit 0; the others say
"stack overflow"). §4 shared-killer protocol applies.

**g01 rows.** M4131 `pos` from `raw > 0` again (killer above, h11 marker). M4132 capacity guard removed (r01 marker). M4133 null check removed
(r05 marker). All codegen/expr.rs.

**Acceptance / guard.** All rows match; `fuzz_parity.sh` array reducers and `checked_arith_parity.sh` green; S608 unchanged.
**Cost.** 3 g01 rows; one axon-rt clippy target.

### NP-6 (P3) `min_i32`/`max_i32` do not truncate in the interpreter - O-DT-01

**Problem.** Three engines, three stories:
- Docs: `builtins.rs:138-149` say "(truncated to i32 range)", params `i64`, `ret: "i32"`; `spec/stdlib.md:254-255`, `:267-268` say "the
  interpreter does not truncate today ... O-DT-01".
- Interpreter (tree and VM share the arm): `interp/builtins.rs:2904-2911` (`"min_i32" | "min_i64"`, `as_int(..).min(..)`): no truncation.
  (O-DT-01 cites :2896-2900; the arms moved.) `abs_i32` truncates (`:1789-1797`, `(n as i32)`).
- Native: `__axon_min_i32(a: i32, b: i32)` (`axon-rt/src/lib.rs:1704`, `:1714`); the extern sig is `L::I32` (`codegen/builtin_externs.rs:198-216`),
  so the args are truncated to their low 32 bits at the call.

**Reproduction** (m/m01, check 0, run 0 both): `min_i32(3000000000, 1)`, `max_i32(3000000000, 1)`, `min_i32(5000000000, 6000000000)`,
`max_i32(0 - 3000000000, 0)`, `abs_i32(5000000000)`, then three in-range calls. Interpreter (`AXON_ENGINE=vm` and `tree` identical):
`1|3000000000|5000000000|0|705032704|7|2|9`. Native: `-1294967296|1|705032704|1294967296|705032704|7|2|9`. m/m03:
`let x: i32 = min_i32(3000000000, 1)` then `x + 1`: interpreter `1|2`, native `-1294967296|-1294967295` - the interpreter's `i32` binding holds
a value outside i32. m/m02 (i32::MIN/MAX args) agrees on both. `fuzz_parity.sh` never saw this: its i64 edges stop at ±1e9 (`:122`), inside
i32, though it fuzzes both names (`:329-330`).

**The change (NH-1 (a), decided by the human 2026-10-31).** Split the arms: `"min_i32"` -> `Value::Int((as_int(a)? as i32).min(as_int(b)? as i32) as i64)`,
same for `max_i32`; `min_i64`/`max_i64` unchanged. Rewrite `stdlib.md:254-255`, `:267-268` to "truncates each argument to its low 32 bits,
like `abs_i32`" and drop the O-DT-01 note. No axon-rt change. Tracked uses (`examples/math_builtins.ax:5-7`, `examples/stdlib_tests.ax:19-24`)
are all in i32 range: unchanged.

**Fail-first test.** `cli_run` (appended at the end of `tests/cli_run.rs`): `min_max_i32_truncate_their_arguments_like_abs_i32` (m01 and m03
under tree and VM -> the native column above). Plus a native case in `integration_fixtures` `min_max_i32_agree_natively`.

**g01 rows.** M4134 `min_i32` arm without `as i32`; M4135 same for `max_i32` (interp/builtins.rs; killer the cli_run test, target
`'--no-default-features --test cli_run'`).

**Guard.** S608 unchanged (no tracked out-of-range use); `doc_snippets` green after the stdlib.md edit. **Cost.** 2 g01 rows, no codegen build
for the interpreter part.

### NP-7 (P3) Recursion depth and `arr_max_i64([])` - O-GAP-07

**Facts.**
- The interpreter limit is `RECURSION_LIMIT = 6_000` (`interp.rs:2323`, non-wasm; wasm32 128 at `:2348`), from 07a3cd66 (2026-05-29,
  "interp: bound recursion gracefully + run on a large stack (no more SIGABRT)"): the debug-build eval frame is ~128 KB, so the limit fires
  before a ~1 GiB thread stack overflows (`interp.rs:2317-2321`). `AXON_MAX_DEPTH` raises it to at most 1,000,000 (`:2495`); the thread stack
  is `depth × 320 KiB`, clamped to [1.25 GiB, 244 GiB] (`:2508`, `:2513`, `:2522`, `stack_size_for_depth` `:2550`).
- Native has no depth counter: `__axon_install_recursion_guard` (`axon-rt/src/lib.rs:1372`) turns the OS stack overflow into exit 101 and
  "stack overflow (recursion too deep)" (`:1362`). `scripts/recursion_guard_parity.sh` asserts only infinite recursion (exit 101 both).

**Reproduction** (`fn s(n) = n + s(n - 1)`, p/rec_*, m/inf):

| depth | interp (default) | native (default opt) | native `--release` |
|---|---|---|---|
| `s(5998)` | `17991001`, exit 0 (tree 0.09 s / 382 MB; VM 0.51 s / 57 MB) | same | - |
| `s(5999)` | exit 101 `recursion limit exceeded (6000) in `s`` | `17997000`, exit 0 | - |
| `s(100000)` | exit 101 | `5000050000`, exit 0 | - |
| `s(150000)` | exit 101 | exit 101 "stack overflow ..." | - |
| `s(500000)` / `s(10^7)` | exit 101 | exit 101 | `125000250000` exit 0 / exit 101 |
| `AXON_MAX_DEPTH=1000000`, `s(999998)` | tree 12.08 s, 54.1 GB max RSS; VM 0.07 s, 91 MB | | |
| `AXON_MAX_DEPTH=100001`, `s(100000)` | exit 101 (needs n+2 frames); tree 1.38 s / 5.5 GB, VM 0.73 s / 2.9 GB | | |
| infinite `rec(n + 1)` | exit 101, 0.04-0.07 s | exit 101 | |

`arr_max_i64([])`: both engines exit 101 with `axon: panic: arr_max_i64: array is empty` (m/a01); the doc string says so (`builtins.rs:612-616`).
Parity holds; this half of O-GAP-07 is a product question only (NH-3).

**Options for the limit** (NH-2): (a) keep 6000 and document the native divergence; (b) raise the default (blast radius: per-run stack
reserve grows 320 KiB per level, 100,000 = 31 GiB of address space, which Linux heuristic overcommit refuses on hosts with less RAM + swap,
`interp.rs:2516-2521`; `cli_run.rs:5325-5350` pins 7000 > default; `CLAUDE.md:266`, `env_registry.rs:28` -> `AXON_REFERENCE.md:83`;
recursion_guard_parity gets slower: tree 1.38 s per 10^5 levels); (c) a native depth counter honouring `AXON_MAX_DEPTH` (an inc/dec plus compare
on every native call, all native perf gates move, and native programs that run today start failing); (d) tail calls (the reported shape
`n + s(n - 1)` is not a tail call, so it does not help); (e) per-program configuration (that is `AXON_MAX_DEPTH` today).

**The change (NH-2 (a), decided by the human 2026-10-31).** Docs only: `env_registry.rs:28` and `CLAUDE.md:266` gain "native binaries have no depth
counter; they recurse until the OS stack overflows (exit 101, \"stack overflow\")"; `AXON_REFERENCE.md` regenerated; `spec/stdlib.md` gets the
same sentence where `AXON_MAX_DEPTH` is first described [INFERENCE: section to be chosen by the builder]. No g01 row (docs); the existing
`axon_max_depth_raises_recursion_limit` (`cli_run.rs:5325`) and `env_registry` drift test cover the touched lines. O-GAP-07 then gets the
stamp `[CLOSED ... native-parity NP-7: documented; NH-2 (a), NH-3 (a)]`.

## 3. Shared mechanics

**Error codes.** No new code. E0910 (NP-1 non-struct tuple subject, NP-3 fail-safe); runtime exit 101 (NP-2, NP-4, NP-5).

**Parity-harness placement (task question 5).** Two homes, both used:
- **Killers in `integration_fixtures`** (`#[cfg(feature = "codegen")]`, the `both` helper pattern, `:2687-2715`, with the §4 shared-killer
  protocol): g01 needs a cargo test ("a script is never a killer", checker-soundness-2 §4), and the native rows M3724/M3830/M3927 use this
  target. Two codegen lib unit tests (NP-2, NP-3) cover what no check-clean program can reach. [REVISED: NPD-3] They run under `--strict` only
  once `gate.sh:1207` widens `--lib codegen::builtin_externs` to `--lib codegen::` (the `--no-default-features` stage compiles codegen out,
  `lib.rs:12`); the integrator makes that edit and updates its comment.
- **Coverage in a new `scripts/native_shapes_parity.sh`**, swept by `parity_all.sh` (its `for h in scripts/*_parity.sh` loop, `:102-103`):
  every probe named in §2 (about 45 cases), CASES-array style as `checked_arith_parity.sh`, comparing stdout, exit code and stderr, sourcing
  `lib/harness_skip.sh`/`lib/axon_bin.sh` like `all_examples_parity.sh:17-20` (skips when codegen is absent; not listed in
  `parity_allowed_skips.txt`). One harness, not one per item: the per-harness overhead in `parity_all` is the binary probe and build setup.
  Measured per case: ~1.4 s (check + run + build + run, `par.sh v22`). `fuzz_parity.sh` edges are not widened (that would move every i64
  fuzzer); NP-6 adds fixed cases instead.

**Native build per lane.** Every item except NP-6's interpreter part needs the codegen `axon` (default features) in its lane, rebuilt after
each edit, and the lane's killers run against it. The last batch failed at the gate precisely because lanes skipped this. [REVISED: NPD-5]
Lanes do not run `parity_all.sh`; the integrator's `gate.sh --strict` does, once (brief in §5).

## 4. g01 rows (M4120-M4136), markers, pci

17 of the block M4120-M4149 are used (M4137-M4149 spare). Row format as checker-soundness-2 §4 (`scripts/v022_g01_mutations.py` 8-tuples; one
`ATTACK_MARKERS` entry each in `scripts/v022_attack_markers.py`, whose `ATTACK: ... [probe]` text is in the killer's assert message;
`g01_anchors.py` finds each `old` exactly once). The exact `old` is the guard line the build writes; the guard each row removes is fixed in §2.
Targets: native rows `'--test integration_fixtures'`, crate `axon-core` (as M3830, `v022_g01_mutations.py:11901-11904`); codegen lib rows
`'--lib'` (default features); M4134/M4135 `'--no-default-features --test cli_run'`. Rows land in the same commit as their fix.

**Shared-killer protocol** [REVISED: NPD-1, NPD-2]. A row is KILLED only when the LAST panic of its test matches its marker
(`v022_attack_markers.py:1-9`). A killer shared by several rows (M4120-M4122/M4136, M4126/M4127, M4129/M4130, M4131-M4133) therefore checks
every probe before failing: it collects one `ATTACK: <attack> [<probe>] ...` line per diverging probe and panics once with all of them; each
row's marker is `ATTACK: [^\n]*\[<probe>\]`. A build failure of a non-control probe is one such line, `ATTACK: <attack> [<probe>] (build:
<stderr>)`, never the `both` helper's plain `"[{tag}]: build: ..."` assert (`integration_fixtures.rs:2709-2713`), which carries no `ATTACK:`.

**pci.** All 17 rows target `crates/axon-core/src`, so the "N rows at this head" line (`governance/notes/v022-pci-delta.md:611`, 570 at
77cf51fc) becomes 587 if this spec lands alone (the integrator recomputes it from the script after the other specs' rows). Every commit touching
`crates/axon-core/src` gets `governance/notes/pci-themes/<sha8>.json`, written by the integrator after the last rebase, then
`tasks/r51-pci-regen.py` and `python3 -B scripts/pci_delta.py --check`.

**Refusal coverage.** NP-1 and NP-3 add E0910 sites: run `python3 -B scripts/v022_refusal_coverage.py` and add a row or exemption.

**Frozen benchmark corpus.** No item changes `axon check` (no checker edit), so `crates/axon-cortex/benchmarks/fence_sweep.py`
`COMPILE_CAUGHT` (checker refusals, `:21-24`, `:160`) needs no entry and no receipt ceiling moves. NP-6 changes interpreter output only for
`min_i32`/`max_i32` arguments outside i32; no tracked benchmark program calls them (grep of `crates/axon-cortex/benchmarks`: 0 uses).

## 5. Gates and cost budget (no paid spend)

**S608 native sweep** (the false-refusal and behaviour guard): `/var/tmp/r51-tmp/NativeParity/sweep/np-sweep1.sh <repo> <bin> <file>` over
`git ls-files '*.ax'` (608 files), `-P8`. Baseline `base-77cf51fc.txt`: 153 skip (no `fn main` or check rc != 0), 324 SAME, 6 DIFF, 125 BUILDFAIL;
90.8 s wall. Expected after the batch: the three `r51_s0b/escape_*` rows DIFF -> SAME (NP-4); every other row byte-identical (in
particular no SAME -> BUILDFAIL from NP-3's fail-safe). The integrator copies script and baseline into the build harness; they are not committed.

| stage | wall time |
|---|---|
| codegen test build, cold / incremental after one axon-core edit | 29 s / 8-15 s (DevloopNative, measured, load avg 15-50, 32 cores) |
| `cargo test -p axon-core --test integration_fixtures` | 23 s including relink; tests 2.05 s (140 tests, DevloopNative) |
| `cargo test -p axon-core --lib codegen::` | 25 s cold / 15 s incremental; 25 tests, ~0 s (DevloopNative) |
| five axon-core clippy variants | 10-20 s each, ~70 s total, warm deps (DevloopNative) |
| one native probe build | 0.05-0.4 s; first build on a host 13.75 s (compiler cache; DevloopNative) |
| `scripts/native_shapes_parity.sh` (~50 cases) | [INFERENCE] 15-70 s, from 1.4 s/case measured (`par.sh v22`) |
| `scripts/parity_all.sh` | 21m42s (`gate.sh:1214`, 2026-10-15) |
| S608 sweep | 90.8 s (measured) |
| g01, 17 rows `--only` | [INFERENCE] 8-12 min (per row: two incremental rebuilds + baseline + run) |
| `scripts/gate.sh --strict` | 43m24s (`gate.sh:1215`) |
| full sharded g01 at the final head | not measured here; the integrator's standing cost (`build-loop-r51.md:108-117`) |

**Per-lane brief** [REVISED: NPD-5]. `gate.sh --lane` cannot combine with `--strict` (`gate.sh:99-100`), so lanes run these stages directly.
- Lanes A and B, every edit cycle: `cargo test --locked -p axon-core --test integration_fixtures`, `cargo test --locked -p axon-core --lib
  codegen::`, `scripts/native_shapes_parity.sh`.
- Lanes A and B, before READY: `cargo clippy --locked -p axon-core -- -D warnings` for each variant the gate runs: `--no-default-features`
  (gate.sh:965), `--no-default-features --features serde-json --all-targets` (:943), `--no-default-features --all-targets` (:1177),
  default/codegen `--all-targets` (:1186), `--no-default-features --features smt --all-targets` (:1515); lane B also the runtime-crate clippy
  (:999, axon-rt) and `cargo test --locked -p axon-core --lib codegen::builtin_externs` (NP-5 adds an extern). Then
  `cargo test --locked -p axon-core --no-default-features` (:1069), `all_examples_parity.sh`, `vm_parity.sh`, `checked_arith_parity.sh`,
  `exit_code_parity.sh`, `recursion_guard_parity.sh` (lane B also `fuzz_parity.sh`), `scripts/vm_wasm_depth.sh`, the S608 sweep, and a g01
  `--only` of the lane's own rows.
- Lane C: `cargo test --locked -p axon-core --no-default-features --test cli_run`, `--no-default-features --test doc_snippets`, the
  `--no-default-features` clippy variants, `fuzz_parity.sh` (needs the codegen binary; no codegen edit), the S608 sweep, g01
  `--only M4134,M4135`.
- Every lane: `python3 -B scripts/spec_ledger.py`, `python3 -B scripts/g01_anchors.py`, `python3 -B scripts/pci_delta.py --check` (after the
  integrator's themes file). `AXON_REFERENCE.md` is regenerated with `axon reference`, never hand-merged. `parity_all.sh` runs once, inside the
  integrator's `gate.sh --strict`.

## 6. Sequencing and lanes

| step | owner | files | per-commit duties | after |
|---|---|---|---|---|
| 0 | integrator | `codegen/mod.rs`: `emit_msg_panic` helper (NP-2 step 1) and the `current_src_fn` field (NP-4); `scripts/gate.sh:1207` filter -> `codegen::` (NPD-3) | themes file; default `--all-targets` clippy | - |
| A1 NP-1 | lane A | `codegen/match_pat.rs`, `codegen/expr.rs:3044` (one call-site argument) | fix + killer + rows M4120-M4122, M4136 + markers in one commit; themes file | step 0 |
| A2 NP-2 | lane A | `codegen/match_pat.rs` (+ its unit-test module) | same; rows M4123-M4125 | A1 |
| B1 NP-3 | lane B | `codegen/expr.rs` (:12060), `mono.rs`, `main.rs`, `infer.rs` hunk (:759, :2044, :2818, :2911, tests :3350-:3960) | same; rows M4126-M4128 | step 0 |
| B2 NP-4 | lane B | `codegen/expr.rs` (:736-747, `emit_lambda`) | same; rows M4129-M4130 | B1 |
| B3 NP-5 | lane B | `codegen/expr.rs` (:7683-7816, :11447, :11455), `axon-rt/src/lib.rs`, `codegen/builtins.rs` (one decl) | same; rows M4131-M4133 | B2 |
| C1 NP-6 | lane C | `interp/builtins.rs` (i32 arms :2904-2911), `spec/stdlib.md`, `builtins.rs` doc strings (:142, :148) | same; rows M4134-M4135; `AXON_REFERENCE.md` regen | - |
| C2 NP-7 | lane C | `env_registry.rs:28`, `CLAUDE.md:266`, `spec/stdlib.md` | docs; `AXON_REFERENCE.md` regen; no rows | C1 |
| integrate | integrator | [REVISED: NPD-7] lanes A/B append their killers at the end of `integration_fixtures.rs` and lane C its test at the end of `cli_run.rs`; the integrator resolves the append-only conflicts, assembles `scripts/native_shapes_parity.sh` from the cases lanes send, and owns g01/marker files, `CHANGELOG.md`, `tasks/opportunities.md` (stamps + §0 logs), `AXON_REFERENCE.md` regen, pci | one themes file per non-pure-join merge commit; `r51-pci-regen.py`; `pci_delta.py --check` | A, B, C |
| gate | integrator | [REVISED: NPD-6] g01 `--only M4120-M4136` (lane feedback), then `gate.sh --strict` once, then the full sharded g01 run at the final head (shard 0 alone, `build-loop-r51.md:108-117`) as the evidence | - | integrate |

Lanes A, B and C are file-disjoint except `codegen/expr.rs` (A1 changes one call argument at :3044; B owns the rest) and `codegen/mod.rs`
(step 0 only). Merge order inside the spec: step 0, A1, A2, B1, B2, B3, C1, C2.

Cross-spec merge order:
- **checker-soundness-3** owns `infer.rs`. NP B1's `infer.rs` hunk (the call-site key) merges **after** checker-soundness-3's `infer.rs`
  commits; lane B rebases the hunk onto them and, if they change `call_instantiations` or `drain_instantiations`, adapts to the new shape. Its
  draft was not available to me (no `tasks/checker-soundness-3*` file in this worktree), so the textual overlap is unverified [INFERENCE]; I
  told `CheckerSoundness3` on 2026-10-31.
- **asi-replay** lane B1 edits `interp/builtins.rs:6224-6281`, `env_registry.rs:40`, `CLAUDE.md:272` and `cli_run.rs` after `:34882`
  (`asi-replay-spec.md:411`); NP lane C edits `interp/builtins.rs:2904-2911`, `env_registry.rs:28`, `CLAUDE.md:266` and appends at the end of
  `cli_run.rs`. Disjoint lines [INFERENCE: clean git merges]. NP lane C merges first, as `asi-replay-spec.md:431` already orders [REVISED: NPD-8].
- No `axon-ai` or `examples/asi` file is touched.

## 7. Engineering decisions (decided here)

1. **Native panics like the interpreter for a match miss, loop escape and allocation refusal** (exit 101, identical stderr), rather than
   refusing at build: the build cannot know the value. Options: refuse every non-exhaustive match natively (false refusal of programs the
   interpreter runs); trap without a message (exit code parity only). Chosen: message parity via `__axon_msg_panic`, as `arr_max_i64` does.
2. **NP-3 per-site rename keyed by callee address**, with an E0910 fail-safe. Options: (i) resolve in codegen from argument types
   (`bind_type_params`, `codegen/mod.rs:2075`) - cannot bind a type param that appears only in the return type or a `Dict` value (a dict is an
   opaque `i8*` natively, `codegen/types.rs:157-173`); (ii) refuse every multi-instantiation generic natively - false refusal of a common shape;
   (iii) per-site key (chosen) - exact, and a missed key degrades to (ii), never to a wrong answer.
3. **One coverage harness** (`native_shapes_parity.sh`) plus cargo killers, not one script per item (cost, §3).
4. **NP-1 E0910 on a non-struct tuple subject** instead of `true`, unless every component is irrefutable (then `true`, today's answer)
   [REVISED: NPA-6]: unknown layout must refuse, but only where a component must actually be tested.
5. **Freestanding**: the new panics use a synthesized trap (`__axon_msg_trap`); message parity is not defined without a stderr.
6. **NP-4 edge name** [REVISED: NPA-5]: the source fn name from `emit_fn`'s `f.name` (impl methods name `go`, like the interpreter); only a
   mono-mangled generic keeps its mangled symbol (exit code equal, stderr names `put__i64`), since recovering the base name needs a mono-side
   table for one rare message.
7. **Literal compare width** [REVISED: NPA-1]. Options: (i) truncate the literal to the subject's width (turns today's build refusals into
   wrong answers, a1/a2/a4); (ii) widen the subject to i64 by its semantic signedness (chosen: exact for every literal); (iii) refuse every
   narrower-int literal pattern (false refusal of u05/s01/s04). An unknown signedness refuses, which is today's outcome for that shape.

## 8. Needs human

`[DECIDED by human 2026-10-31: NH-1 (a) min_i32/max_i32 truncate like abs_i32; NH-2 (a) keep the 6000 default, document that native has no limit; NH-3 (a) arr_max_i64([]) keeps panicking]`

- **NH-1 (NP-6): `min_i32`/`max_i32` semantics.** (a) Truncate each argument to its low 32 bits in the interpreter, like `abs_i32`, matching
  native and the doc strings. (b) Remove `min_i32`/`max_i32` (language-surface removal; 2 tracked example files use them). (c) Make native
  not truncate (an `i32`-typed result then holds values outside i32, m03). **Recommendation: (a)** - the only option where the declared
  return type is true, and it changes no tracked program's output.
- **NH-2 (NP-7): the recursion default.** (a) Keep 6000 and document that native has no depth counter. (b) Raise the default (address-space
  reserve and overcommit risk on small hosts, tests and docs above). (c) A native depth counter (per-call cost; native programs that run
  today would fail). **Recommendation: (a).** The interpreter's limit is a resource guard, native's answers past 6000 are correct, and
  `AXON_MAX_DEPTH` already serves deep legitimate recursion.
- **NH-3 (NP-7): `arr_max_i64([])` return type.** (a) Keep the documented panic (parity holds on both engines). (b) Return `Option<i64>`
  (API change for every caller and the min/max family, `builtins.rs:612-630`). **Recommendation: (a).**

No paid spend. No other choice here is product or irreversible.
