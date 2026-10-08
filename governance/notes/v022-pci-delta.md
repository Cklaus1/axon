# PCI delta since the certified interpreter (31413ca7)

Status: MUTABLE note (amendments 82 and 84). It does not amend
`governance/proofs/v022-pci/CERTIFICATION.md`, which stays as certified and says what it says:
axon `31413ca7`, the LOCAL backend, the EMPTY effect ceiling, 94/94 mutations, with surfaces 18
(non-empty grant) and 19 (microVM) scoped out. This note says what the guest interpreter is NOW,
relative to that, and how much of the difference is covered.

## (a) Interpreter changes since 31413ca7 (generated)

The block below is produced by `scripts/pci_delta.py` from git, not written by hand. A drift test
(`crates/axon-core/tests/pci_delta_note.rs`) regenerates it at the head it names and fails if it
differs, if a commit under `crates/axon-core/src` is not classified in the script's `THEMES`, or if a
later commit touches `crates/axon-core/src` (the note is then stale: `python3 scripts/pci_delta.py
--emit HEAD`, paste between the markers).

<!-- BEGIN MECHANICAL (scripts/pci_delta.py) -->
generated-at: 83175bed8e38eb1d5854c9914b345a27ec377ec5

`git diff --numstat 31413ca7..83175bed -- crates/axon-core/src`:

| file | added | removed |
|---|---|---|
| `crates/axon-core/src/ast.rs` | 86 | 8 |
| `crates/axon-core/src/builtins.rs` | 2 | 2 |
| `crates/axon-core/src/cache.rs` | 70 | 2 |
| `crates/axon-core/src/capabilities.rs` | 18 | 1 |
| `crates/axon-core/src/checker.rs` | 209 | 57 |
| `crates/axon-core/src/codegen/asi.rs` | 8 | 0 |
| `crates/axon-core/src/codegen/build_wrappers.rs` | 15 | 1 |
| `crates/axon-core/src/codegen/builtins.rs` | 6 | 0 |
| `crates/axon-core/src/codegen/escape.rs` | 509 | 0 |
| `crates/axon-core/src/codegen/expr.rs` | 1489 | 435 |
| `crates/axon-core/src/codegen/link.rs` | 747 | 291 |
| `crates/axon-core/src/codegen/match_pat.rs` | 43 | 36 |
| `crates/axon-core/src/codegen/mod.rs` | 197 | 8 |
| `crates/axon-core/src/codegen/option_result.rs` | 11 | 6 |
| `crates/axon-core/src/codegen/output.rs` | 60 | 20 |
| `crates/axon-core/src/complexity.rs` | 1 | 0 |
| `crates/axon-core/src/doc.rs` | 1 | 0 |
| `crates/axon-core/src/effects.rs` | 51 | 0 |
| `crates/axon-core/src/env_registry.rs` | 4 | 0 |
| `crates/axon-core/src/error.rs` | 20 | 11 |
| `crates/axon-core/src/fmt.rs` | 5 | 0 |
| `crates/axon-core/src/infer.rs` | 3 | 2 |
| `crates/axon-core/src/interp.rs` | 6698 | 1216 |
| `crates/axon-core/src/interp/builtins.rs` | 424 | 325 |
| `crates/axon-core/src/interp/conform.rs` | 1789 | 0 |
| `crates/axon-core/src/interp/eval.rs` | 822 | 180 |
| `crates/axon-core/src/interp/goal.rs` | 27 | 26 |
| `crates/axon-core/src/interp/pin.rs` | 944 | 0 |
| `crates/axon-core/src/interp/proptest.rs` | 35 | 15 |
| `crates/axon-core/src/interp/taint.rs` | 1058 | 0 |
| `crates/axon-core/src/interp/taint_tests.rs` | 1416 | 0 |
| `crates/axon-core/src/interp/value.rs` | 15 | 7 |
| `crates/axon-core/src/kernel.rs` | 2 | 2 |
| `crates/axon-core/src/lib.rs` | 114 | 13 |
| `crates/axon-core/src/main.rs` | 202 | 93 |
| `crates/axon-core/src/mono.rs` | 2 | 0 |
| `crates/axon-core/src/mut_borrow.rs` | 800 | 0 |
| `crates/axon-core/src/parser.rs` | 39 | 1 |
| `crates/axon-core/src/resolver.rs` | 334 | 64 |
| total | 18276 | 2822 |

`git log --reverse 31413ca7..83175bed -- crates/axon-core/src`:

| commit | theme | what it does to pass/fail (from its message) |
|---|---|---|
| 378da246 | merged from main (AX findings) | native agent_action audit record gains effect_row/principal; interp.rs and interp/builtins.rs change only by a `cfg` re-export and a visibility change: no change in what the interpreter evaluates or refuses |
| abf72e0b | review fixes B1/B3 (main.rs) | runner/test-CLI hardening from review wf_d725935a-7ed |
| bae904b8 | keyed failure token (main.rs) | the interpreter keys every FAILURE it decides, so a printed failure line is not a verdict: narrowing |
| a4a05f28 | key and completion | K is unreachable from candidate code: narrowing |
| e9a13f72 | key and completion | the raw key read is unix-only so the wasm32 build compiles: build fix |
| 8f8e9755 | sealed modules, module path | an unreadable suite module never falls through to the candidate: narrowing |
| 08903fdf | sealed modules, module path | a sealed module's use resolves only in the sealed dirs: narrowing |
| 38273ef1 | sealed modules, module path | one file per module name, suite first: narrowing |
| 8e9858f4 | RNG isolation | a sealed frame may not reseed the process RNG: narrowing |
| b8bbdb74 | RNG isolation | a sealed frame may not DRAW from the process RNG: narrowing |
| bce231ad | RNG isolation | RNG hardening completeness, M254 false retirement: narrowing |
| 8a7962fb | RNG isolation | per-kernel RNG isolation: narrowing |
| 7da2fe71 | rows, style, harness | rustfmt and a row re-anchor: no intended change in semantics |
| e1e2a22f | rows, style, harness | row cells and attack messages: no intended change in semantics |
| 2a397d93 | rows, style, harness | row routes: no intended change in semantics |
| 32fdea5c | sealed modules, module path | the sealed-module rule holds whoever imports: narrowing |
| 91c2f676 | key and completion | sealed handlers act only on sealed operations; completion is the body's end: narrowing |
| f2a12aab | key and completion | row attack (Option-shaped test) where completion is the only guard: evidence |
| 73834357 | rows, style, harness | +23 interp.rs lines with two defect fixes found making rows honest |
| e8537726 | amendment 53 | a value is cast at every declared boundary; an operator method name is the operator's: narrowing (A86) |
| 3cf23eef | amendment 60 | the cast keys on the dispatch type; a local is never called by name: narrowing |
| 8a5419de | amendment 60 | scalar-arm attacks reach their arms; M1155 compiles |
| 48a077ee | amendment 74 | predicate primitives are refusal sites; keyed_outcome rowed |
| fa0643ac | amendment 72 | at a seal crossing every type position is determined from the operator side or refused; E0505 refuses an impl for f32/isize/usize: narrowing |
| 68176b53 | amendment 72 | M1148/M1669 attacks: row repair |
| 6b793da8 | amendment 72 | the operator's dicts are snapshotted at a seal crossing and verified at every edge back: narrowing |
| ba1bad03 | amendment 74 | merge of c9r4c/gate |
| 3b9b9092 | amendment 78 | a position the operator held is judged by what it held, deeply and strictly: narrowing |
| 96a31eb8 | merged from main (AX findings) | native codegen only (place assignment, str + str, allocas): the interpreter is untouched |
| 13f01eb8 | merged from main (AX findings) | INTERPRETER: arr_sort_by becomes a stable merge sort (interp/builtins.rs): not PCI-reviewed |
| be8576af | merged from main (AX findings) | INTERPRETER: arrays become shared Rc values, cheaper user calls (interp.rs, eval.rs, value.rs): not PCI-reviewed; the seal's cast and dict-snapshot code was adapted to the Rc layout in the merge |
| 36227ec7 | merged from main (AX findings) | native codegen only (non-escaping array literals on the stack): the interpreter is untouched |
| 1727775e | merged from main (AX findings) | LANGUAGE + INTERPRETER: `&mut [T]` parameters write through (parser, checker, interp.rs, eval.rs, new E0604-E0606): not PCI-reviewed; the merge routes `call_fn_mut` through the same seal edges as `call_fn` |
| edfe3e2d | merged from main (AX findings) | native codegen, plus checker concat typing and resolver capture analysis (`axon check` accepts different programs): interpreter evaluation is untouched |
| 19310764 | merged from main (AX findings) | LANGUAGE + INTERPRETER: named functions are first-class values (resolver, checker, effects, eval.rs): not PCI-reviewed |
| 099ff710 | merged from main (AX findings) | build/cache/CLI only (runtime location, compiler-identity cache key): the interpreter is untouched |
| c4d307bd | merged from main (AX findings) | CLI help text only: no change in what is refused |
| 6493c2f2 | merged from main (AX findings) | native codegen/link only (LLVM pipeline, --opt-level): the interpreter is untouched |
| 82b83298 | merged from main (AX findings) | native link flags only: the interpreter is untouched |
| d29d4ef6 | amendment 78 | row repairs (M1672, M1843, M1845) |
| 1d01b014 | merged from main (AX findings) | INTERPRETER: shared copy-on-write strings, in-place append, lent closure captures (interp.rs, eval.rs, builtins.rs, value.rs): not PCI-reviewed; closures resolve a local through `call_local_closure` while the amendment-60 rule (a local is never called by name elsewhere) is kept |
| 2bf1d9d0 | amendment 82 | comment-only (TestEnd doc) |
| 40ca1092 | amendment 82 | merge of c9r4c/claims; comment-only in this path |
| 12c6685e | amendment 83 | the dispatch rule: operator code never selects an operator impl by a type nothing on the operator side determined (untyped dict/channel/lambda reads must be annotated; pin.rs): narrowing |
| 554951b6 | amendment 83 | unit-test case only (an unannotated lambda parameter case in interp.rs tests): no production change |
| 9e19e961 | amendment 83 | unit test and comments only (interpolation/comparison of an untyped read select no operator impl): no production change |
| b971194c | amendment 83 | clippy: the arithmetic arm of the pin walk (interp/pin.rs) collapsed into a guard: no change in what is refused |
| 70692659 | amendment 83 | unit-test attack text only (M1996: a shift truncates where + and * panic): no production change |
| dab96417 | amendment 88 | the dispatch analysis trusts only what the operator chose: candidate fns/types/lets, local-name shadowing and trait names no longer determine a receiver; keys carry the owning fn; fail-closed lookup; operator-defined runtime values dispatch; the address cache is removed (pin.rs, interp.rs, eval.rs): narrowing on candidate-influenced receivers, widening only for operator-only polymorphism |
| edde3d5d | amendment 88 | clippy: an unused `mut` removed in pin.rs; no change in what is refused |
| b024c06e | amendment 88 | pin.rs: a redundant sealed-let filter removed (the push guard is the one rule); test-only otherwise; no change in what is refused |
| a4af40a1 | merge of origin/main (PR #8) | joins the commits above; combines main's `call_fn_in`/`call_fn_mut` with the seal as one `call_fn_sealed` path, adds `&mut T` and Rc-array arms to conform.rs/pin.rs, keeps amendments 53-83 on the merged call path: no intended change in what is refused (rows re-run on this head) |
| 9acbb453 | merge of origin/main (PR #8) | merge fixes: `input_arg` read from the args (the verify panic's input suffix), the per-frame stack budget raised to 512 KiB because the merged debug frame is ~265 KB: no change in what is refused |
| fe00e4c5 | merge of c9r4c/psv1e (integrate6) | joins the psv1e dispatch-analysis rebuild (pin.rs `Tys`, amendment 88) with main's `&mut T` type and Rc layout (a `RefMut` arm in the closed-type walk, no env duplicate in the closure call): no intended change in what is refused |
| aa0dd7eb | env registry (integrate6) | three TEST-ONLY re-exec markers registered (AXON_EQ_ALONE/GIT_ENV/HARDEN); a registry row for a var only tests read: no change in what is refused |
| f4a7a2dc | amendment 94 | a `&mut` write-through value is cast and verified at the seal edge back, and a `&mut` operand is open in the dispatch analysis; a sealed frame cannot take an operator fn as a first-class value; a union annotation does not pin a receiver (interp.rs, interp/eval.rs, interp/pin.rs, interp/conform.rs): narrowing |
| 13374667 | amendment 94 | the edge-back cast also runs on an aborted call; a provenance mark on fn values dropped (redundant with the creation edge); rows and tests: narrowing, no widening |
| 2d70adf5 | amendment 94 | the held-value judgement at the `&mut` edge is not strict, so an honest fill of an empty output array passes while the declared type judges the position (conform.rs `strict` parameter): the one deliberate relaxation, relative to the first form of this amendment, not to the merged tree |
| 8caedb78 | amendment 94 | rustfmt only (interp.rs): no change in what is refused |
| 2c0203e0 | amendment 96 | sandbox_run's result is cast to its declared i64 at the seal crossing; every read of an operator global goes through one lookup (`global_ref`) so the FieldAccess/Index fast paths and the closure-constant call apply the edge; a sealed frame's fn value carries its own mark (an operator fn value stays unmarked) and cannot replace an operator closure; unary `-x`/`~x` is judged by the width arm; drift tests for global reads and for builtins that run user code: narrowing |
| 12cf6c55 | amendment 96 | unit test only (the value of a with-handler expression is undetermined until pinned: a stated cost): no production change |
| cc74956e | amendment 100 | a handler arm and its continuation replay run under the pin owner of the fn that INSTALLED the handler, not the one that performed the effect; the name argument of every name-resolving builtin (sandbox_run, scheduler_spawn, goal_*, kernel_goal_create) must be an operator-chosen name (Ctx::npure) or the call is refused; an operator fn and a missing one read the same to sealed code; drift tests: narrowing, no widening |
| c62e9b02 | amendment 102 | a runtime taint: a value sealed code produced carries a taint every operation of the evaluator propagates (an accumulator around `eval`, a taint per binding, per shared object and per module-level let, an early-exit taint for a fn result and for stores), and an operator frame refuses to use a tainted value to SELECT operator code: the name given to a name-resolving builtin, an operator closure the candidate picked (out of a table by a key, index or branch) and then called, the impl a method call dispatches to (receiver type), and the width of fixed-width arithmetic; the static pin analysis is kept underneath; a plain `axon run` is unchanged (one branch per eval); drift tests for builtin classes, dict writers, dispatch sites, value holders and swept files: narrowing, no widening |
| 83175bed | amendment 106 | a channel is one shared object with one taint: every channel method and every `select` arm examined takes it into the result, and a sealed frame's mutating access (send, recv, try_recv) marks it; a kernel or world write stores the control taint it ran under; a builtin runs its callbacks under the control taint of its arguments; string interpolation and the stringifying builtins take the taint of every shared object they print; a sealed caller is refused in ONE text on every path (a name, a global, a field or index, `goal_run`, a goal constraint, `kernel_goal_create`: `goal_run` of an operator fn no longer completes); routing and drift tests for every dict, kernel and array reader, 38 hunted programs: narrowing, no widening |
| 64 commits | | |
<!-- END MECHANICAL -->
<!-- END MECHANICAL -->



What is BY THEME (the `theme` and `what it does` columns; these are the commit messages' own
account and are NOT mechanically verified; the files and commits above are):

- Narrowing, by the commits' own messages (amendment 94, round 8: a `&mut` write-through value is cast and judged at the seal edge back and a `&mut` operand is open in the dispatch analysis, a sealed frame cannot take an operator fn as a value, a union annotation pins nothing): sealed-module resolution, RNG isolation, K unreachable from
  candidate code, sealed handlers, the keyed FAILURE token (a printed failure line is not a verdict;
  `main.rs`), the declared-boundary cast (amendment 53), the dispatch-key cast (60), seal-crossing
  positions, dict snapshots and E0505 (72), the held-value judgement (78).
- Cost of the runtime taint (amendments 102, 106) to a run that is NOT sealed, MEASURED (release build, min of 5, base
  `5e16d8b4` against the amendment 102 tree): a tight `while` loop +2.5%, `fib(35)` +4.7%, a closure `arr_map` loop +5.3%;
  that is **up to about 5%**, not zero (one `u8` per binding, a save/restore of two cells per call, one branch per call to pick the
  instance; every hook is compiled out of the ordinary instance). The same 120 example programs print byte-identical output with both
  binaries. Amendment 106 adds only `if T` hooks and one guard per builtin call in a SEALED run; measured against `3776884c`, an
  ordinary run and a sealed run differ by less than the 5% run-to-run noise of the shared host (amendment 106, item 8).
- MERGED FROM MAIN (PR #8, 2026-10-06; theme `merged from main (AX findings)`): 13 commits under
  `crates/axon-core/src` that were written and tested on `main` and were NOT reviewed against the PCI
  claims. Only 5 of the 13 are interpreter changes; the other 8 change no interpreter evaluation (six are native codegen, build/cache or CLI-help changes; `378da246` touches `interp.rs` and `interp/builtins.rs` only by a `cfg` re-export and a visibility change; `edfe3e2d` changes checker concat typing and resolver capture analysis, so `axon check` accepts different programs). The 5 are neither narrowing nor known to be neutral and need a
  reviewer's eye: shared Rc arrays and cheaper calls (`be8576af`), shared copy-on-write strings and lent
  closure captures (`1d01b014`), `&mut [T]` write-through (`1727775e`), first-class named fns
  (`19310764`), and the `arr_sort_by` rewrite (`13f01eb8`). The other 8 are as listed above. The merge adapted the seal to them (one `call_fn_sealed` path for `call_fn` and
  `call_fn_mut`; `&mut T` and Rc-array arms in `conform.rs`/`pin.rs`) and re-anchored three rows
  (M924, M1680, M1841). The per-frame stack budget was raised to 512 KiB (the merged debug frame is
  ~265 KB, which left no margin under the old 256 KiB).
  What refuses each bad route found in them, and no more than these rows show (amendment 94, the
  `am94` rows of `scripts/v022_pci_gates.sh`): `&mut` write-through (the value a candidate leaves in the
  operator's binding is cast and judged at the edge back; a `&mut` operand is undetermined to the
  dispatch rule), first-class fns (a sealed frame cannot take an operator fn as a value), and the Rc
  arrays' copy-on-write (a candidate's write to its array parameter does not reach the operator's copy:
  observed through the runner, with no guard of our own). Shared strings, lent closure captures and the
  `arr_sort_by` rewrite have no row of their own; the routes executed against them are listed in
  amendment 94 and were found closed by earlier rows.
- Evidence only (rows, harness, rustfmt, comments): `7da2fe71`, `e1e2a22f`, `2a397d93`, `f2a12aab`,
  `68176b53`, `d29d4ef6`, `8a5419de`, `2bf1d9d0`, `40ca1092`; `73834357` also carries two defect
  fixes (+23 `interp.rs` lines).
- Files the earlier version of this note omitted: `lib.rs` (+77, sealed module loading), `parser.rs`
  (+15, `parse_type_text`), `ast.rs` (+34, `walk_type_names`), `error.rs` (E0505), `kernel.rs`
  (comment). "Narrowing only" is the commit messages' claim and was not established for these by
  anything but those messages and the rows below.
- The one runner-visible semantic point: a completion token means the test body returned normally,
  which is NOT evidence that every assertion ran (an assertion inside a closure the candidate never
  calls does not run; executed to a keyed PASS by the PSV-3 reviewers). Suites must assert after the
  call.

## (b) Coverage: PCI gate rows, and the mutation rows

`scripts/v022_pci_gates.sh` has 67 rows at this head: the original 18 (surfaces 1-21), ten
added by amendment 84, one group per delta amendment (53, 60, 72 incl. its dict snapshot, 78), two for
amendment 83's dispatch rule (integration), and six for amendment 94 (the `&mut` edge-back cast, the `&mut` operand in the dispatch analysis, the fn-value seal edge and the Rc/copy-on-write observation, each unit and runner where both exist), nine for amendment 96 (sandbox_run's cast, the one global-read edge, the fn-value mark, the unary width arm, the classified user-code builtins and the handler-expression cost), and six for amendment 100 (a handler arm's pin owner, the name sinks, the existence oracle and the drift tests), and seven for amendment 102 (the runtime taint: the closure picks, the names, the impl and width, the carriers, an ordinary run, the drift tests and the runner leg; plus the sweep step that runs every rule-on interpreter test with only the taint on), and five for amendment 106 (channel state and the areas not hunted before, every reader of shared state after a sealed write, the existence oracle on every path, the drift tests, and the runner leg), each with
unit tests in `interp.rs`/`conform.rs` AND a real-runner test (`axon_psv::runner::run`, in
`crates/axon-psv/tests/sealed_frames.rs`). The gate fails if a named test is absent (grep), renamed,
filtered out or `#[ignore]`d (the passed count must equal the named count). Verified to discriminate:
renaming one named test in a copy of the script made it FAIL ("test ... not found" and "ran 0 test(s)").
Amendment 83's dispatch rule (psv1d) landed after amendment 84: its tests are the two `am83 dispatch rule`
rows; the dict-snapshot rows still name tests that exist after psv1d (their label "psv1d may replace" is dropped).

Run at `c9r4c/claims2` (veto `cdc39fc6` plus this amendment's script/doc changes; the rows through am78 were first run there at 28 rows), interpreter build,
exit 0 (the earlier rows below were last run at 34 rows, c9r4c/claims3 at veto 1204a925 plus amendment 89's four added rows; at c9r8/psv1f 10c7c916 the whole script, then 40 rows, exited 0 on gpumaster including the six am94 rows; at c9r9/psv1g, 29276036 plus the THEMES and note edits, the gate script's final line `PASS` with 49 rows, exit 0, run locally; the am94 and am96 rows are in the table):

| row | package/target | result |
|---|---|---|
| 1  shadowing | axon-fabric/check_effects | PASS 1/1 |
| 2  redefinition | axon-core/lib | PASS 2/2 |
| 2b second-trait method | axon-fabric/check_effects | PASS 1/1 |
| 3  symlink escape | axon-fabric/check_effects | PASS 1/1 |
| 4  ambient module path | axon-fabric/check_effects | PASS 2/2 |
| 4  ambient module path | axon-core/pci_isolation | PASS 1/1 |
| 5  path-list injection | axon-fabric/check_effects | PASS 1/1 |
| 6  verifier environment | axon-fabric/attestation | PASS 2/2 |
| 7  loop control | axon-core/lib | PASS 3/3 |
| 7  loop control | axon-fabric/check_effects | PASS 2/2 |
| 8/11/12 return, exit, Err | axon-core/lib | PASS 1/1 |
| 10 named handlers | axon-core/pci_isolation | PASS 1/1 |
| 13 completion evidence | axon-core/test_completion | PASS 2/2 |
| 13 completion evidence | axon-fabric/check_effects | PASS 3/3 |
| 14 exit code | axon-fabric/check_effects | PASS 1/1 |
| 17 working directory | axon-fabric/check_effects | PASS 1/1 |
| 21 sealed candidate | axon-core/lib | PASS 2/2 |
| 21 sealed candidate | axon-fabric/check_effects | PASS 2/2 |
| am53 declared-boundary cast | axon-core/lib | PASS 4/4 |
| am53 declared-boundary cast | axon-psv/sealed_frames | PASS 1/1 |
| am60 dispatch-key cast | axon-core/lib | PASS 3/3 |
| am60 dispatch-key cast | axon-psv/sealed_frames | PASS 1/1 |
| am72 seal-crossing positions | axon-core/lib | PASS 2/2 |
| am72 seal-crossing positions | axon-psv/sealed_frames | PASS 1/1 |
| am72 dict snapshot | axon-core/lib | PASS 1/1 |
| am72 dict snapshot | axon-psv/sealed_frames | PASS 1/1 |
| am78 held-value judgement | axon-core/lib | PASS 3/3 |
| am78 held-value judgement | axon-psv/sealed_frames | PASS 1/1 |
| am83 dispatch rule | axon-core/lib | PASS 2/2 |
| am83 dispatch rule | axon-psv/sealed_frames | PASS 1/1 |
| am72 absent return type is () | axon-core/lib | PASS 1/1 |
| am72 channel stamped at creation | axon-core/lib | PASS 1/1 |
| am72 closure args strict at a crossing | axon-core/lib | PASS 1/1 |
| am83 arithmetic width arm | axon-core/lib | PASS 1/1 |
| am94 &mut edge-back cast | axon-core/lib | PASS 4/4 |
| am94 &mut operand open in the dispatch analysis | axon-core/lib | PASS 2/2 |
| am94 &mut edge (dispatch and width arms) | axon-psv/sealed_frames | PASS 1/1 |
| am94 fn-value seal edge | axon-core/lib | PASS 2/2 |
| am94 fn-value seal edge | axon-psv/sealed_frames | PASS 1/1 |
| am94 shared-Rc/COW non-leak | axon-psv/sealed_frames | PASS 1/1 |
| am96 sandbox_run result cast at the crossing | axon-core/lib | PASS 1/1 |
| am96 sandbox_run result cast at the crossing | axon-psv/sealed_frames | PASS 1/1 |
| am96 one global-read edge | axon-core/lib | PASS 2/2 |
| am96 one global-read edge (runner leg: corroboration only, refused statically by E0004 before the runtime edge) | axon-psv/sealed_frames | PASS 1/1 |
| am96 fn value mark | axon-core/lib | PASS 1/1 |
| am96 unary width arm | axon-core/lib | PASS 1/1 |
| am96 unary width arm | axon-psv/sealed_frames | PASS 1/1 |
| am96 user-code builtins classified | axon-core/lib | PASS 2/2 |
| am96 handler-expression value is undetermined (stated cost) | axon-core/lib | PASS 1/1 |
| am100 handler arm pin owner | axon-core/lib | PASS 6/6 |
| am100 handler arm pin owner | axon-psv/sealed_frames | PASS 3/3 |
| am100 name sinks | axon-core/lib | PASS 5/5 |
| am100 name sinks | axon-psv/sealed_frames | PASS 3/3 |
| am100 existence oracle | axon-core/lib | PASS 1/1 |
| am100 drift: any globals mention, any runner of user code | axon-core/lib | PASS 2/2 |
| am102 runtime taint: closure picks | axon-core/lib | PASS 1/1 |
| am102 runtime taint: names | axon-core/lib | PASS 2/2 |
| am102 runtime taint: impl and width | axon-core/lib | PASS 1/1 |
| am102 runtime taint: carriers and control | axon-core/lib | PASS 3/3 |
| am102 runtime taint: an ordinary run takes none | axon-core/lib | PASS 1/1 |
| am102 drift: classes, writers, dispatch sites, value holders, frames, swept files | axon-core/lib | PASS 6/6 |
| am102 runtime taint (runner leg) | axon-psv/sealed_frames | PASS 3/3 |
| am106 shared state keeps its taint: channels, areas not hunted before | axon-core/lib | PASS 2/2 |
| am106 every reader of shared state is tainted after a sealed write | axon-core/lib | PASS 3/3 |
| am106 the existence oracle on every path | axon-core/lib | PASS 1/1 |
| am106 drift: channel methods, dict and kernel builtins, text-rendering builtins | axon-core/lib | PASS 4/4 |
| am106 shared state and the existence oracle (runner leg) | axon-psv/sealed_frames | PASS 3/3 |
| am102 sweep (only the taint on: exactly the two static-only programs differ) | axon-core/lib (PSV1T_TAINT_ONLY) | PASS |

Mutation rows whose target is `crates/axon-core/src` and which are not retired (`MUTATIONS` minus `RETIRED`, 296 rows at this head, recomputed from the script): M59, M61-M62, M65, M67-M70, M72-M79, M82-M83, M85-M88, M90-M97, M219, M243, M246-M248, M250-M251, M260, M270-M272, M436, M560-M566, M651-M667, M920-M939, M1140-M1157, M1660-M1681, M1730-M1731, M1734, M1764-M1765, M1840-M1845, M1847-M1848, M1936-M1938, M1975, M1990-M1997, M2171-M2178, M2370-M2376, M2461, M2470-M2480, M2600-M2602, M2606, M2608-M2624, M2700-M2767, M2910-M2939 (but M2929, never issued). Per delta:

| delta | rows (named in the amendment's own text) |
|---|---|
| sealed modules, RNG, key, completion | M560-M566; the PCI-scope rows M52-M97 |
| amendment 53 | M651-M667 (+ M566), matrix A86 |
| amendment 60 | M86-M89, M562-M563, M651-M652, M920-M939, M1140-M1157, matrix A86 |
| amendment 72 | M1660-M1681, matrix A96-A109 |
| amendment 74 | M1730-M1731, M1734, M1764-M1765 |
| amendment 78 | M1840-M1845, M1847-M1848, matrix A127-A130 |
| amendment 83 | M1990-M1997, M1975 (the off switch is cfg(test)-only), matrix A145-A148 |
| amendment 88 | M2171-M2178 (the rebuilt dispatch analysis) |
| amendment 94 | M2370-M2376, matrix A182-A184 |
| amendment 96 | M2470-M2480, matrix A190-A194 |
| amendment 100 | M2600-M2602, M2606, M2608-M2624 (M2603-M2605 and M2607, the runner-leg duplicates, withdrawn by amendment 102), matrix A208-A212 |
| amendment 102 | M2700-M2767, matrix A219-A224 |
| amendment 106 | M2910-M2939 (M2929 not issued), matrix A240-A245 |

FREEZE OBLIGATION, not a present fact: the claims spec says these rows are re-run at the frozen head.
What the freeze procedure must show is a joined paired-disable run at the frozen head in which each of
the rows above is killed by its own attack (or retired with its four cells). This task and note have
re-run, at c9r10/psv1h e0f9acd8 on gpumaster (clean clone, two shards, `v022_g01_mutations.py --scope=all --only=<ids>`), all 203 active rows above: 203/203 KILLED by their own attack (101 + 102), 0 REFUSED_ELSEWHERE, 0 stale. That is a sample run (`--only`), not the scope run the freeze requires, and it re-ran no retired row; the gate rows (the gate script, 55 rows now) ran on gpumaster and passed. Per-amendment kill evidence is in each amendment's text.

## (c) Surfaces 18 and 19, and what the protected profile does with a non-empty ceiling

Outside the certification, unchanged: surface 18 (a non-empty effect grant) and surface 19 (the
microVM). PSV-3's guest-interpreter claim covers the EMPTY ceiling only.

What the code does today (read, not executed; no code was changed):

- `submit.rs` derives the ceiling from the ADMITTED grant (`grants::effect_ceiling(grant.grant())`),
  never from an operator flag, and hands the same value to the host executor and to
  `GuestPolicy::for_grant(&req, &ceiling)`. For `for_grant`, `""` is `allowed_effects: []` (deny
  every effect); a non-empty ceiling becomes that list in the guest policy. `for_grant` refuses
  nothing but a policy too long for the cmdline.
- `axon_psv::protected_policy_ceiling` (root helper before the nonce is spent, Fabric where it binds
  the policy, and the guest runner) requires the policy to be the manifest's `policy_sha256` and to
  state `allowed_effects` explicitly (absent is refused). It returns the ceiling, empty or not.
- `backend::select` (about line 1211) refuses ANY restricting grant (`grants::restricts_effects`,
  which includes the certified EMPTY ceiling) unless the signed B263 record shows
  `x1_guest_policy_channel` as PASS (a waived BLOCKED x1 does not count). A fully unrestricting grant
  (all four axes) needs no x1.
- `signing::attestation_decision`: on the microVM backend the receipt is signed for evidence class
  `protected` OR `guest-unobserved` REGARDLESS of the effect ceiling. On any other backend a non-empty
  ceiling is refused (`KEY_REACHABLE`) and an empty one is signed only as `development`.

So, plainly: **under a non-empty ceiling a receipt CAN be signed `protected`**, provided the microVM
path ran and was observed (and x1 is PASS if the grant restricts anything). The ceiling is applied inside
the guest and the policy digest is joined to the observation, but the signed receipt names only the
policy digest, not the ceiling. A `protected` receipt therefore does NOT imply the PCI-certified scope.
This is an OPERATOR-VISIBLE SCOPE LIMIT, not a defect fixed: the operator chose "reword + delta note,
non-empty guest ceiling OUT of scope" (2026-10-05). Pinning an empty ceiling for protected trials in the
grant registry is a possible follow-up; the code does not enforce it.
