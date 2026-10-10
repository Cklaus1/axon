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
generated-at: 13ee4b51980e6e54748567c6435c2b2b499aa774

`git diff --numstat 31413ca7..13ee4b51 -- crates/axon-core/src`:

| file | added | removed |
|---|---|---|
| `crates/axon-core/src/ast.rs` | 201 | 88 |
| `crates/axon-core/src/builtins.rs` | 44 | 10 |
| `crates/axon-core/src/cache.rs` | 284 | 40 |
| `crates/axon-core/src/capabilities.rs` | 18 | 1 |
| `crates/axon-core/src/checker.rs` | 608 | 106 |
| `crates/axon-core/src/clock.rs` | 58 | 3 |
| `crates/axon-core/src/codegen/asi.rs` | 14 | 18 |
| `crates/axon-core/src/codegen/build_wrappers.rs` | 25 | 1 |
| `crates/axon-core/src/codegen/builtin_externs.rs` | 8 | 0 |
| `crates/axon-core/src/codegen/builtins.rs` | 6 | 0 |
| `crates/axon-core/src/codegen/display.rs` | 514 | 0 |
| `crates/axon-core/src/codegen/enum_layout.rs` | 451 | 0 |
| `crates/axon-core/src/codegen/escape.rs` | 509 | 0 |
| `crates/axon-core/src/codegen/expr.rs` | 2305 | 680 |
| `crates/axon-core/src/codegen/link.rs` | 1610 | 337 |
| `crates/axon-core/src/codegen/match_pat.rs` | 335 | 147 |
| `crates/axon-core/src/codegen/mod.rs` | 363 | 106 |
| `crates/axon-core/src/codegen/option_result.rs` | 11 | 6 |
| `crates/axon-core/src/codegen/output.rs` | 118 | 69 |
| `crates/axon-core/src/codegen/types.rs` | 5 | 4 |
| `crates/axon-core/src/complexity.rs` | 1 | 0 |
| `crates/axon-core/src/doc.rs` | 1 | 0 |
| `crates/axon-core/src/effects.rs` | 291 | 108 |
| `crates/axon-core/src/env_registry.rs` | 7 | 0 |
| `crates/axon-core/src/error.rs` | 22 | 13 |
| `crates/axon-core/src/fmt.rs` | 5 | 0 |
| `crates/axon-core/src/host.rs` | 54 | 0 |
| `crates/axon-core/src/infer.rs` | 3 | 2 |
| `crates/axon-core/src/interp.rs` | 2207 | 1417 |
| `crates/axon-core/src/interp/builtins.rs` | 592 | 547 |
| `crates/axon-core/src/interp/eval.rs` | 1152 | 547 |
| `crates/axon-core/src/interp/goal.rs` | 55 | 52 |
| `crates/axon-core/src/interp/proptest.rs` | 20 | 23 |
| `crates/axon-core/src/interp/sym.rs` | 1155 | 0 |
| `crates/axon-core/src/interp/value.rs` | 491 | 543 |
| `crates/axon-core/src/interp/vm/compile.rs` | 1691 | 0 |
| `crates/axon-core/src/interp/vm/mod.rs` | 3286 | 0 |
| `crates/axon-core/src/interp/vm/pure.rs` | 959 | 0 |
| `crates/axon-core/src/interp/vm/tests.rs` | 980 | 0 |
| `crates/axon-core/src/kernel.rs` | 89 | 23 |
| `crates/axon-core/src/lib.rs` | 116 | 21 |
| `crates/axon-core/src/main.rs` | 789 | 241 |
| `crates/axon-core/src/mono.rs` | 2 | 0 |
| `crates/axon-core/src/mut_borrow.rs` | 800 | 0 |
| `crates/axon-core/src/parser.rs` | 69 | 3 |
| `crates/axon-core/src/replay.rs` | 40 | 17 |
| `crates/axon-core/src/resolver.rs` | 534 | 66 |
| `crates/axon-core/src/seal_split.rs` | 85 | 0 |
| `crates/axon-core/src/time_passes.rs` | 189 | 0 |
| `crates/axon-core/src/types.rs` | 28 | 0 |
| total | 23200 | 5239 |

`git log --reverse 31413ca7..13ee4b51 -- crates/axon-core/src`:

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
| cf286ad6 | merged from main (R51 S0) | INTERPRETER: feat(builtins): now_ns monotonic nanosecond clock in both engines (AX-32): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| f16d5295 | merged from main (R51 S0) | outside the interpreter (main.rs): fix(cli): axon run leaves stderr to the program; run-id only with --verbose or AXON_RECORD (AX-33): the interpreter is untouched |
| 37ff592f | merged from main (R51 S0) | outside the interpreter (cache.rs, codegen/link.rs, codegen/mod.rs, codegen/output.rs, error.rs, lib.rs, main.rs): perf(build): cache the optimised object so a hit only links (AX-34): the interpreter is untouched |
| 855d93a5 | merged from main (R51 S0) | outside the interpreter (main.rs): fix(cli): axon build names the artifact it wrote, not always Binary (AX-39): the interpreter is untouched |
| e432b391 | merged from main (R51 S0) | outside the interpreter (codegen/link.rs, codegen/output.rs, main.rs): perf(codegen): run globaldce at O0 and prune AI wrappers for --emit-obj (AX-37): the interpreter is untouched |
| 673e3004 | merged from main (R51 S0) | outside the interpreter (codegen/link.rs, env_registry.rs): perf(codegen): link prebuilt runtimes without running cargo, pinned to the workspace toolchain (AX-35): the interpreter is untouched |
| 75f623b0 | merged from main (R51 S0) | outside the interpreter (builtins.rs, effects.rs): perf(effects): walk each body once and propagate effects over a worklist; O(1) builtin lookup (AX-36): the interpreter is untouched |
| 4ddfbff7 | merged from main (R51 S0) | outside the interpreter (codegen/link.rs, codegen/output.rs, lib.rs, main.rs, time_passes.rs): feat(cli): axon check/build --time-passes per-phase timing report (AX-36): the interpreter is untouched |
| 58934b2b | merged from main (R51 S0) | outside the interpreter (codegen/link.rs, main.rs): perf(codegen): --opt-level s/z mark every definition optsize/minsize (AX-38): the interpreter is untouched |
| b43383a6 | merged from main (R51 S0) | merge commit (Merge branch 'ax/frontend-timing' into ax/cb-integration); joins its branch, no change beyond its parents |
| 009c3291 | merged from main (R51 S0) | merge commit (Merge branch 'ax/codegen-size' into ax/cb-integration); joins its branch, no change beyond its parents |
| a107086b | merged from main (R51 S0) | merge commit (Merge branch 'ax/runtime-libs' into ax/cb-integration); joins its branch, no change beyond its parents |
| 1780bcde | merged from main (R51 S0) | merge commit (Merge branch 'ax/build-cache' into ax/cb-integration); joins its branch, no change beyond its parents |
| 32c7f3bf | merged from main (R51 S0) | INTERPRETER: perf(interp): closures capture only their free variables; syms instead of names (AX-40, AX-18): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 11c44343 | merged from main (R51 S0) | INTERPRETER (via merge): merge commit (Merge branch 'ax/interp-closures' into ax/cb-integration); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false) |
| 05132402 | merged from main (R51 S0) | INTERPRETER: style(interp): FxHasher reads 8-byte words with as_chunks (clippy): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| c62e9b02 | amendment 102 | a runtime taint: a value sealed code produced carries a taint every operation of the evaluator propagates (an accumulator around `eval`, a taint per binding, per shared object and per module-level let, an early-exit taint for a fn result and for stores), and an operator frame refuses to use a tainted value to SELECT operator code: the name given to a name-resolving builtin, an operator closure the candidate picked (out of a table by a key, index or branch) and then called, the impl a method call dispatches to (receiver type), and the width of fixed-width arithmetic; the static pin analysis is kept underneath; a plain `axon run` is unchanged (one branch per eval); drift tests for builtin classes, dict writers, dispatch sites, value holders and swept files: narrowing, no widening |
| 83175bed | amendment 106 | a channel is one shared object with one taint: every channel method and every `select` arm examined takes it into the result, and a sealed frame's mutating access (send, recv, try_recv) marks it; a kernel or world write stores the control taint it ran under; a builtin runs its callbacks under the control taint of its arguments; string interpolation and the stringifying builtins take the taint of every shared object they print; a sealed caller is refused in ONE text on every path (a name, a global, a field or index, `goal_run`, a goal constraint, `kernel_goal_create`: `goal_run` of an operator fn no longer completes); routing and drift tests for every dict, kernel and array reader, 38 hunted programs: narrowing, no widening |
| 1a75557b | amendment 106 | test only (the oracle test reads the verdict as Debug text; no production change) |
| b156e0fd | merged from main (R51 S0) | outside the interpreter (checker.rs, types.rs): fix(checker): resolve enums and refinements inside tuples and fn types (AX-41): the interpreter is untouched |
| 213acb7b | merged from main (R51 S0) | outside the interpreter (codegen/enum_layout.rs, codegen/expr.rs, codegen/match_pat.rs, codegen/mod.rs, codegen/types.rs): fix(codegen): lay out struct fields of enum type; place enum payload fields by name (AX-43): the interpreter is untouched |
| f7c0cf4f | merged from main (R51 S0) | outside the interpreter (codegen/asi.rs, codegen/expr.rs, codegen/match_pat.rs, codegen/mod.rs): fix(codegen): each refusal printed and counted once; no E0701 cascade after a refusal (AX-45): the interpreter is untouched |
| 79ab856e | merged from main (R51 S0) | outside the interpreter (builtins.rs, codegen/expr.rs, codegen/mod.rs): fix(codegen): native arr_push/reverse/take/drop over any element type (AX-44): the interpreter is untouched |
| 9e952186 | merged from main (R51 S0) | INTERPRETER: perf(interp): 16-byte Value, Box<str> Flow, Sym-keyed records, cached string literals (AX-46, AX-47): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 9d73a8b6 | merged from main (R51 S0) | merge commit (Merge branch 'ax/native-arr-push' into ax/cb-integration2); joins its branch, no change beyond its parents |
| da8d4b2d | merged from main (R51 S0) | INTERPRETER: style(interp): rustfmt the AX-46/AX-47 record and Flow changes: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 837ab524 | merged from main (R51 S0) | INTERPRETER: perf(interp): inline Int/Float binop fast path shared with eval_binop_vals (AX-46): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| eb34ee05 | merged from main (R51 S0) | INTERPRETER: test(interp): pin record print order, equality, variant matching and narrow-field coercion (AX-47): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 25b5f63d | merged from main (R51 S0) | outside the interpreter (codegen/enum_layout.rs, codegen/expr.rs, codegen/match_pat.rs, codegen/types.rs): fix(codegen): lower recursive enums by boxing self-referential payload fields (AX-42): the interpreter is untouched |
| a507e2ec | merged from main (R51 S0) | merge commit (Merge branch 'ax/native-enum-layout' into ax/cb-integration2); joins its branch, no change beyond its parents |
| b92a7962 | merged from main (R51 S0) | merge commit (Merge branch 'ax/codegen-diag' into ax/cb-integration2); joins its branch, no change beyond its parents |
| d91575b8 | merged from main (R51 S0) | outside the interpreter (codegen/match_pat.rs): fix(codegen): enum field pattern test goes through build_wrappers::w_and (R1e): the interpreter is untouched |
| e46ec5a0 | merged from main (R51 S0) | INTERPRETER (via merge): merge commit (Merge branch 'ax/interp-value' into ax/cb-integration2); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false) |
| ee1c7056 | amendment 108 | a comparison (`==`, `!=`, ordering) takes the taint of every shared object (dict, channel) inside either operand, however deep, and every builtin argument is walked deep (no table of names): `if d == e {lenient} else {strict}` over a dict the candidate filled is refused like `dict_get_or`; a sealed method call is judged by the methods sealed impls define and a run-time method miss reads like an operator method (existence oracle, method path); a `native::` call is World state (a sealed call marks it, an operator call reads it back); drift and routing tests, 12 rows: narrowing, no widening |
| b8f176d9 | amendment 108 | test only (a clippy `useless_format` in a taint test): no production change |
| 8abeb02b | amendment 108 | builtin arguments are walked deep EXCEPT the first argument of ten builtins that only count, key into or append (`SHALLOW_FIRST_ARG`, fail-closed: unlisted is deep), which keeps a loop of a million `dict_set`s linear; tests: narrowing, no widening beyond amendment 108's first form |
| d1334115 | merged from main (R51 S0) | INTERPRETER: style: rustfmt sweep, layout only (compilebench AX-52): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 1a6282f5 | merged from main (R51 S0) | outside the interpreter (codegen/mod.rs): fix(codegen): give a tuple element binding its semantic type so an inner tuple destructures natively (AX-48): the interpreter is untouched |
| a7fc7778 | merged from main (R51 S0) | outside the interpreter (codegen/display.rs, codegen/expr.rs, codegen/mod.rs): fix(codegen): render interpolated structs, enums, Option/Result natively like the interpreter (AX-49): the interpreter is untouched |
| 0e4030eb | merged from main (R51 S0) | merge commit (Merge branch 'ax/struct-interp' into ax/cb-integration3); joins its branch, no change beyond its parents |
| c2385099 | merged from main (R51 S0) | outside the interpreter (codegen/build_wrappers.rs, codegen/display.rs): fix(codegen): display helpers emit through build_wrappers (R1e); w_float_ext: the interpreter is untouched |
| 2c6266c0 | merged from main (R51 S0) | INTERPRETER: perf(interp): resolve locals to frame slots; read scalar binop operands in place (AX-53, AX-55): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| c3c34ecc | merged from main (R51 S0) | INTERPRETER: perf(interp): user-fn call decides attrs once, reuses frames and arg buffers (AX-54): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| ff5ceb8e | merged from main (R51 S0) | INTERPRETER (via merge): merge commit (Merge branch 'ax/interp-calls' into ax/cb-integration4); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false) |
| 99b8b099 | merged from main (R51 S0) | INTERPRETER: R50 S0: engine selector, run_body, FnEntry.compiled, trace, extracted dispatch_call/bind_let/call_mut, AX-57 scope-leak fix: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 7af37c53 | merged from main (R51 S0) | INTERPRETER: R50 S1: bytecode engine lowers the scalar core (literals, locals, blocks, let, assign, operators, if/while/for, return/break/continue, ?,...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 40881a3a | merged from main (R51 S0) | INTERPRETER: R50 S1: inline the extracted eval helpers' fast paths (tree loop.ax back under S0): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| d1346ad6 | merged from main (R51 S0) | INTERPRETER: R50 S1: vm exec keeps Break/Continue catching off the common return path: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 58a6b01e | merged from main (R51 S0) | INTERPRETER: R50 S1: calls by name resolve the callee at compile time (dispatch_named); call_fn_in binds params by index: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 0ff60f9c | merged from main (R51 S0) | INTERPRETER: R50 S1: blocks, while iterations and for bodies that bind nothing push no scope; body exit builds no Unit to drop: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 3c7a1716 | merged from main (R51 S0) | INTERPRETER: R50 S1: call frames pool as Box<Env> (a pointer moves, not the frame); frame clear forgets scalar bindings instead of running drop glue: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 65d407d8 | amendment 114 | the right operand of `&&`/`||` runs under the left's control taint (sealed runs, a cold function), a refused match guard, a `?` and a `select` arm raise theirs, `?` counts as an exit, the sealed files are checked by a second pipeline over the sealed items alone with the checker refusing the missing name wherever the operator's was (struct literal, lambda parameter, enum/trait/refinement types, generic bound, verify and refinement predicates); taint::CONTROL_TABLE drift-tested against ast.rs: narrowing, no widening |
| fb35ba68 | merged from main (R51 S0) | INTERPRETER: R50 S1: call_mut takes its frame from the call-frame pool (take_frame/give_frame) instead of allocating one per call: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 547b6029 | merged from main (R51 S0) | INTERPRETER: R50 S1: current_fn holds the executing fn's &FnDef in a Cell (no Rc clone, no RefCell flag); one CallGuard restores it and the call depth: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 325f5a89 | merged from main (R51 S0) | INTERPRETER: R50 S1: call_fn_in's common path (FnEntry::plain, no refinements) is guards, arity, bind, goal_met, body, result; attribute-driven steps ...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 2affff15 | merged from main (R51 S0) | INTERPRETER: R50 S1: call_fn_in pushes params and goal_met into the empty frame (the define_var slot == len case, without its checks): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 4bd88b06 | merged from main (R51 S0) | INTERPRETER: R50 S1: a VM call by name leaves its arguments on the operand stack (StackTail); a user-fn callee binds them from there, no take_args/rec...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| abf5976b | merged from main (R51 S0) | INTERPRETER: R50 S1: a bytecode activation's operand stack lives in its pooled frame (Env::stack) instead of the vm_stacks pool: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 733c4806 | merged from main (R51 S0) | INTERPRETER: R50 S1: env_pool is Vec<Box<Env>> on purpose (allow clippy::vec_box): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 5d333913 | amendment 114 | test and registry only (a taint test for the type a `?` carries, plant controls for the note check): no production change |
| 2e68236b | amendment 114 | the sealed-only check holds the operator's `mod` of a candidate module (found by the fabric suite), so a candidate whose module uses its own helper resolves; an operator module the candidate does not ship is still missing: narrowing relative to the first form of amendment 114, no widening of what base accepted |
| 3a67ec2c | merged from main (R51 S0) | INTERPRETER: R50 S3: lower match, while let and method calls (MethodRecv); extract chan_method/impl_method: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| ef20af0c | merged from main (R51 S0) | INTERPRETER: R50 S2: aggregates, index/field reads and place writes lower to ops: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 765bb811 | merged from main (R51 S0) | INTERPRETER (via merge): merge commit (Merge ax/r50-s3 (R50 S3 match, while let, methods)); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false) |
| 89017cc5 | merged from main (R51 S0) | INTERPRETER: R50 S4: lambdas compile (make_closure, ClosureCode.compiled), allocation-free builtin -> closure call; fold.ax 330/element: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 12713cbd | merged from main (R51 S0) | INTERPRETER (via merge): merge commit (Merge ax/r50-s4 (R50 S4 lambdas, allocation-free builtin -> closure call)); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false) |
| 91796cac | merged from main (R51 S0) | outside the interpreter (cache.rs): spec(R50): S2-S4 landed, evidence ledger; cache unit tests use per-test scratch dirs (parallel-test race): the interpreter is untouched |
| 93877287 | merged from main (R51 S0) | INTERPRETER: R50 S5 (WIP): CallFast op (proven fast VM->VM call, inline-operand args), CallMut op over call_mut_with, fast_call_blocker + vm: slow tra...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| f0ce9745 | merged from main (R51 S0) | INTERPRETER: R50 S5: fast-call tests (mutcall no tree nodes, slow per flag, param/return semantics, tier clearing, depth limit, depth-before-arity, ha...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| fd8816b7 | merged from main (R51 S0) | INTERPRETER: R50 S5: allocation-free call_mut (both engines), CallMut op, leaf fast-call bodies; fastcall 257/300, mutcall 562/600: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 3eb79f1a | merged from main (R51 S0) | INTERPRETER (via merge): merge commit (Merge ax/r50-s5 (R50 S5 fast calls, allocation-free call_mut, CallMut op)); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false) |
| 8470d24c | merged from main (R51 S0) | INTERPRETER: R50 S7: Pure and PureLoop ops, fold_leaf in arr_fold; vm: pure / vm: fold-leaf trace lines: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 0899ae24 | merged from main (R51 S0) | INTERPRETER (via merge): merge commit (Merge ax/cb-integration4 (R50 S5) into ax/r50-s7); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false) |
| 08f0e6fe | merged from main (R51 S0) | INTERPRETER: R50 S8 (WIP, gate not met): IndexDefine, WriteIndexIndex, BinStack, CallFastLocalInt, BranchReturn/leaf_arm, Moves &mut fast path: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 34f2bf90 | merged from main (R51 S0) | INTERPRETER: R50 S8: CallMut caches a frame-free MovesCall form (swap fused to one Swap move); fib trace op counts: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| a9176ce5 | merged from main (R51 S0) | INTERPRETER: R50 S6: AXON_ENGINE defaults to vm; docs (I-2 sentence, env rows, reference, wasm export): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| db2d2eee | merged from main (R51 S0) | INTERPRETER: R50 S9: deferred compile (second entry unless hot on entry), AXON_VM_EAGER, vm: defer trace; compilebench AX-58: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 3849fadb | amendment 117 | the clear, localized members of the round-14 find-until-dry loop (44 of 53 findings): `temporal_new`/`temporal_is_valid` are World and Time (a clock reader is no Pure builtin); a match guard, every `while`/`while let` condition evaluation after the first, every builtin callback after the first (`call_cb`) and every Rust loop that runs operator code (`t_loop_pc`: goal searches, the scheduler pass) run under the control taint of what decided them; an operator pop and the channel a `select` looked at are marked; an abort-capable `with` body treats a branch on tainted data as a possible exit and sealed code that ran in it raises the sticky taint, as does a scheduler fiber that ran sealed code and failed; a resume value keeps its taint; `dstore_*` is World, a zoned call's provenance push is kernel state and its append is world state; a refinement type pins what its base pins; a width inside an `Uncertain`/`Temporal` is a width; `sandbox_run` copies the operator's sandbox entry into the sealed kernel for the call; the principal token stream is per registry; the sealed-only check resolves `dyn`, array/tuple elements, refinement/`where`/`@[verify]`/whole-struct predicates and the exact deferred type names, and the goal variants and `goal_eval` give a sealed caller the missing-name text: narrowing, no widening (the claim itself is NARROWED to what is enforced) |
| c4ea5684 | amendment 117 | follow-up to 3849fadb found by the full mutation run: two guards that proved redundant are removed (a sealed-caller check after goal_name_is_known; the operator-pop branch of t_chan_access, covered by t_chan_choice) and a test case for an aborting arm's value: no change in what is refused |
| b7aaa9c1 | amendment 121 | the existence oracle at every placement and the operator-typed value the candidate picked (first cut): the parser's end-of-input span is no longer the dummy `0..0`; the resolver locates a type, a refinement and a top-level `let` at its item and keeps the run's own sealed set (an HONEST `fn lim(n: i64 where n >= 0)` is no longer refused for naming its parameter); a merged diagnostic with no line joins the backstop; a sealed top-level `let` initializer is name-checked; a third taint bit, `PICK`, marks an operator-typed value (or one holding it) that sealed code selected (an index, a builtin's read on a selector it chose, a branch, a store under its control, an exit it decided, a hand-back from a sealed fn or closure) and the dispatch rule refuses a receiver that carries it: narrowing, no widening |
| d7574f7d | amendment 121 | the split moves to `seal_split.rs` (library, so a gate row runs its test), the honest-registry control is made to bind the candidate's taint, doc and test wording: no change in what is refused beyond b7aaa9c1 |
| 45347cb7 | amendment 121 | the backstop's first branch no longer appends the sealed-only diagnostics (empty there); three older rows re-anchored; the refusal-coverage exemption of the observer prune dropped (that file is not under crates/axon-core/src; the commit touches main.rs only by removing one line): no change in what is refused |
| 416a6683 | amendment 121 | the parser unit test that is M3450's own attack (`an_item_that_ends_the_file_has_a_real_span_in_its_own_file`); no production change in this commit (parser.rs gains a test only) |
| 64d4f997 | merged from main (R51 S0) | INTERPRETER: R50 rev 26 / AX-56: wasm32 recursion panics instead of trapping (guard 128, native-stack byte budget checked by scripts/wasm_stack_budget...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it |
| 13ee4b51 | R51 S0 merge | joins main 5864c423 with v022/veto c9ed2ddd taking MAIN's interpreter (interp.rs, interp/{eval,builtins,goal,proptest,value}.rs); veto's taint.rs/taint_tests.rs/pin.rs/conform.rs are NOT on this tree, so the seal they enforce is absent. Refuse-all pin in the same commit: Interp::build refuses any sealed run (SEAL_RUNTIME_COMPLETE = false), `axon test --seal` exits 2 before any result line, vm::compile refuses a sealed interp. Every PCI row that needs a seal edge is deferred (governance/status/r51-deferred-tests.json) until R51 S6 |
| 147 commits | | |
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

`scripts/v022_pci_gates.sh` has 93 rows at this head: the original 18 (surfaces 1-21), ten
added by amendment 84, one group per delta amendment (53, 60, 72 incl. its dict snapshot, 78), two for
amendment 83's dispatch rule (integration), and six for amendment 94 (the `&mut` edge-back cast, the `&mut` operand in the dispatch analysis, the fn-value seal edge and the Rc/copy-on-write observation, each unit and runner where both exist), nine for amendment 96 (sandbox_run's cast, the one global-read edge, the fn-value mark, the unary width arm, the classified user-code builtins and the handler-expression cost), and six for amendment 100 (a handler arm's pin owner, the name sinks, the existence oracle and the drift tests), and seven for amendment 102 (the runtime taint: the closure picks, the names, the impl and width, the carriers, an ordinary run, the drift tests and the runner leg; plus the sweep step that runs every rule-on interpreter test with only the taint on), and five for amendment 106 (channel state and the areas not hunted before, every reader of shared state after a sealed write, the existence oracle on every path TESTED, the drift tests, and the runner leg), and five for amendment 108 (comparisons and builtin arguments read the shared objects inside them, the existence oracle on the method path, a native registry as world state, the runner leg of the comparison, and the runner row of the method path at check time), and six for amendment 114 (a module the operator declares for the candidate resolves its own helper, the control taint of every conditional, repeated or exiting form, the drift test of the control table, its runner leg, the existence oracle on the static path, and the note's own planted-change checks), each with
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
| am72 absent return type is () | axon-core/lib | PASS 1/1 |
| am72 channel stamped at creation | axon-core/lib | PASS 1/1 |
| am72 closure args strict at a crossing | axon-core/lib | PASS 1/1 |
| am83 arithmetic width arm | axon-core/lib | PASS 1/1 |
| am83 dispatch rule | axon-core/lib | PASS 2/2 |
| am83 dispatch rule | axon-psv/sealed_frames | PASS 1/1 |
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
| am100 handler arm pin owner (runner leg: corroboration only, the unit rows are the evidence) | axon-psv/sealed_frames | PASS 3/3 |
| am100 name sinks | axon-core/lib | PASS 5/5 |
| am100 name sinks (runner leg: corroboration only, the unit rows are the evidence) | axon-psv/sealed_frames | PASS 3/3 |
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
| am106 shared state and the name rule (runner leg, corroboration) | axon-psv/sealed_frames | PASS 2/2 |
| am108 comparisons and builtins read the shared objects inside their operands | axon-core/lib | PASS 3/3 |
| am108 the existence oracle on the method path | axon-core/lib | PASS 1/1 |
| am108 a native registry a sealed frame wrote is tainted | axon-core/lib | PASS 1/1 |
| am108 comparison (runner leg, corroboration) | axon-psv/sealed_frames | PASS 1/1 |
| am108 the existence oracle on the method path, at check time (runner: the only witness of the checker path) | axon-psv/sealed_frames | PASS 1/1 |
| am114 control taint: every conditional, repeated or exiting form, attack and control | axon-core/lib | PASS 1/1 |
| am114 drift: every Expr variant and both short-circuits have a row in the control table | axon-core/lib | PASS 1/1 |
| am114 control taint (runner leg, corroboration) | axon-psv/sealed_frames | PASS 1/1 |
| am114 a module the operator declares for the candidate resolves the candidate's own helper | axon-psv/sealed_frames | PASS 1/1 |
| am114 the note's checks refuse a wrong result, target, claim list or gate package | axon-core/pci_delta_note | PASS 3/3 |
| am114 the existence oracle on the static path, every position the test lists, text and accept/refuse (runner: the only witness) | axon-psv/sealed_frames | PASS 1/1 |
| am117 types and widths: a refinement pins what its base pins, a width inside Uncertain/Temporal, the operator's sandbox ceiling on a sealed fn | axon-core/lib | PASS 1/1 |
| am117 drift: no Pure arm touches ambient state; the durable store is World; provenance writes are marked | axon-core/lib | PASS 3/3 |
| am117 drift: every closure-taking builtin has a callback row and calls through call_cb; every Rust loop raises the control taint; RNG draws are coupled | axon-core/lib | PASS 3/3 |
| am117 drift: the holder reasons name things that exist | axon-core/lib | PASS 1/1 |
| am117 the sealed kernel's token stream is its own | axon-core/lib | PASS 1/1 |
| am117 the clock is not a Pure builtin (runner: the Time ceiling refuses it) | axon-psv/sealed_frames | PASS 1/1 |
| am117 the PSV-1 non-claim list names every NARROW-CLAIM finding of the triage table | axon-core/pci_delta_note | PASS 1/1 |
| am121 a diagnostic about a sealed item names the sealed file wherever the item sits | axon-core/lib | PASS 1/1 |
| am121 an item that ends a file has a real span in its own file | axon-core/lib | PASS 1/1 |
| am121 a spanless merged diagnostic joins the backstop | axon-core/lib | PASS 1/1 |
| am121 the operator value the candidate picked is never dispatched on (attacks and honest controls) | axon-core/lib | PASS 1/1 |
| am121 the operator value the candidate picked (runner leg, corroboration) | axon-psv/sealed_frames | PASS 1/1 |
| am121 every position the sealed walk visits has a twin row | axon-psv/sealed_frames | PASS 1/1 |
| am121 a cheat module at byte 0 is refused by the sealed-only check, not the backstop | axon-psv/runner | PASS 1/1 |
| am121 a live nonce's record survives another request's prune | axon-fabric/observer_service | PASS 1/1 |
| am102 sweep (only the taint on: exactly the two static-only programs differ) | axon-core/lib (PSV1T_TAINT_ONLY) | PASS |

Mutation rows whose target is `crates/axon-core/src` and which are not retired (`MUTATIONS` minus `RETIRED`, 426 rows at this head, recomputed from the script): M59, M61-M62, M65, M67-M70, M72-M79, M82-M83, M85-M88, M90-M97, M219, M243, M246-M248, M250-M251, M260, M270-M272, M436, M560-M566, M651-M667, M920-M939, M1140-M1157, M1660-M1681, M1730-M1731, M1734, M1764-M1765, M1840-M1845, M1847-M1848, M1936-M1938, M1975, M1990-M1997, M2171-M2178, M2370-M2376, M2461, M2470-M2480, M2600-M2602, M2606, M2608-M2624, M2700-M2767, M2910-M2928, M2930-M2946, M3030-M3037, M3039-M3042, M3230-M3251, M3253-M3259, M3267-M3270, M3300-M3307, M3309-M3323, M3325-M3349, M3351, M3450-M3458, M3460-M3479

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
| amendment 106 | M2910-M2946 (M2929 not issued; M2940-M2946 runner legs), matrix A239-A244 |
| amendment 108 | M3030-M3042 (M3033, M3035, M3038 runner leg, WITHDRAWN by amendment 116: retired by amendment 114 behind M3243 on four cells, the full-suite condition never shown), matrix A249-A253 |
| amendment 114 | M3230-M3270 (M3242-M3253, M3260, M3267-M3270 static names, runner; M3256-M3259 runner legs of the control taint; M3261-M3266 the note's own checks; M3252 not issued), matrix A281-A285 (written A280-A284) |
| amendment 117 | M3300-M3351 (M3308, M3324, M3350 not issued: the first two were drawn and found equivalent; M3346-M3349 runner legs; the drift rows M3321-M3323, M3326, M3327, M3329, M3344, M3345), matrix A291-A300 (the claim's non-claim list is checked by `pci_delta.py`, no row) |
| amendment 121 | M3450-M3482 (M3459 not issued; M3260 retired EQUIVALENT against M3456; M3450-M3458 the existence oracle at every placement, M3460-M3478 the PICK mark, M3479 the closure-helper scan, M3480-M3482 the observer prune), matrix A303-A308 |

FREEZE OBLIGATION, not a present fact: the claims spec says these rows are re-run at the frozen head.
What the freeze procedure must show is a joined paired-disable run at the frozen head in which each of
the rows above is killed by its own attack (or retired with its four cells). This task and note have
re-run, at c9r10/psv1h e0f9acd8 on gpumaster (clean clone, two shards, `v022_g01_mutations.py --scope=all --only=<ids>`), all 203 active rows above: 203/203 KILLED by their own attack (101 + 102), 0 REFUSED_ELSEWHERE, 0 stale. That is a sample run (`--only`), not the scope run the freeze requires, and it re-ran no retired row; the gate rows (the gate script, 55 rows now) ran on gpumaster and passed. Per-amendment kill evidence is in each amendment's text.

Coverage of the PRODUCTION PAIR (the static layer plus the taint, as shipped): it is covered only by the
`Both` columns of amendment 102's tests, the runner legs and the gate's sweep step. A static guard removed
from the production route ALONE is not observable, because the unit-test switch `TAINT_FORCE_ON` is
`cfg(test)` and no shipped binary has the taint off; so the runner rows of amendment 100 (`axon-psv`; four
of its five duplicates were withdrawn by amendment 102) and the runner legs of amendments 96 and 100 are CORROBORATION of the
unit rows, and are labelled so in the gate script. So are the runner legs of amendments 106, 108 and 114, each labelled
`(runner leg, corroboration)` there: the production route refuses those attacks by the taint first, so each unit row is the
evidence and the runner row is a second witness (am108's M3033 and M3035; am114's M3256-M3259). Two runner families are NOT
corroboration and are not labelled so: the method oracle at check time (am108, M3038, now WITHDRAWN by amendment 116; its guard is unrowed) and the names twin (am114, M3242-M3253)
test the static CHECK, which only the real `axon test --seal` binary runs, so for the checker path the runner test is the only witness.

Round 12 (c9r12/psv1v, `9a95c9ce`, gpumaster, `--only=<ids>`, two shards): all 316 active rows whose target is `crates/axon-core/src` were re-run, 316/316 KILLED by their own
attack, 0 REFUSED_ELSEWHERE, 0 stale. Still a sample run (`--only`), not the scope run the freeze requires.

Round 14 (c9r14/psv1x, amendment 117; src `c4ea5684`, rows/markers at `b3034408`-era script plus the follow-ups, gpumaster, clean clones, `v022_g01_mutations.py --scope=all --only=<ids>`, two shards): ALL 398 active rows whose target is `crates/axon-core/src` (am102, am106, am108, am114 included, retired rows skipped) were re-run, 398/398 KILLED by their own attack, 0 REFUSED_ELSEWHERE, 0 survived. The first full run (three shards, before the follow-up) found 12 older rows whose anchor text the source change had moved (re-anchored: M2478, M2172, M2174 and others kept their text by restructuring the edit) and six that survived (M2925, M2946: a guard made redundant by `goal_name_is_known`, removed; M3319: an operator-pop branch made redundant by `t_chan_choice`, removed; M2761: its resume-based attack is now also caught by the feed taint, a new aborting-arm case is the row's attack; M2172/M2174: compile errors from the refinement table's new type). One shard was run twice because a concurrent job rebuilt the shared interpreter (`interpreter binary changed during the run`); the second run is the one counted. Still a sample run (`--only`), not the scope run the freeze requires. The gate script (85 rows) exited 0 locally; axon-core (`--no-default-features --no-fail-fast`), axon-psv and the axon-fabric tests `psv_dispatch`, `check_effects`, `attestation`, `cortex_via_fabric` exited 0 on gpumaster at `0ffceec3`; clippy `-D warnings` (default and `--no-default-features`), `cargo fmt --check`, `v022_refusal_coverage.py` plain and `--freeze`, `psv_matrix_check.py` and `pci_delta.py --check` exit 0. Cost (release build, `axon run`, seven runs each, alternating order, gpumaster, src `1f2e5196`): tight loop 2.02 s old / 2.03 s new, fib(32) 1.64 / 1.64, `arr_map` 1.84 / 1.83, `&&` loop 1.08 / 1.08 (medians): inside noise.

Round 15 (c9r15/psv1y, amendment 121; src `416a6683`, gpumaster, clean clones, `v022_g01_mutations.py --scope=all --only=<ids>`, two lists): ALL 427 active rows whose target is `crates/axon-core/src` (am102, am106, am108, am114, am117 and am121 included, retired rows skipped) were re-run at `bab8806b`: 425/427 KILLED by their own attack, 0 REFUSED_ELSEWHERE, 0 stale; the two survivors were M3450 (the parser's end-of-input span: given its own attack, `parser::tests::an_item_that_ends_the_file_has_a_real_span_in_its_own_file`, then 3/3 killed with M3456 and M3457 at `b708386a`) and M3260 (the candidate's `use` set aside for the merged check: made redundant by the backstop for a merged diagnostic that names no line, retired EQUIVALENT against M3456 under the four-cell rule, reported separately and never counted killed; its paired-disable record is in the amendment's evidence table). The first full run also found three older rows whose anchor text amendment 121 moved (M2718, M3243, M3268: re-anchored with the same attack) and a refusal-coverage exemption for the observer prune that the new rows contradict (dropped: REMAINDER 207). The gate script (93 rows) exited 0 on gpumaster at `b708386a`; axon-core (`--no-default-features --no-fail-fast`) exited 0 there; axon-psv and the axon-fabric tests `psv_dispatch`, `check_effects`, `attestation`, `cortex_via_fabric`, `observer_service` exited 0 at `bab8806b`; clippy `-D warnings` (default and `--no-default-features`), `cargo fmt --check`, `v022_refusal_coverage.py` plain and `--freeze`, `psv_matrix_check.py`, `check_postc9_spec.py` and `pci_delta.py --check` exit 0. Still a sample run (`--only`), not the scope run the freeze requires. Cost (release build, `axon run` outside a sealed run, seven runs each, three binaries alternating, a host at load 8, medians): `fib(32)` 2.497 s at `e8b8b48a` / 2.513 s at `27697781` / 2.510 s here, a 20M-iteration `while` loop 3.319 / 3.373 / 3.278, `arr_map` 5 x 1M elements 2.497 / 2.465 / 2.527, a 10M-iteration `&&` loop 1.835 / 1.848 / 1.868: every difference is inside the 3-7 % spread of the runs of one binary.

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
