#!/usr/bin/env python3
"""The mechanical half of governance/notes/v022-pci-delta.md.

Everything that changed under crates/axon-core/src since the certified interpreter
(31413ca7) is listed from git, not from memory: per-file line counts and every
commit, each tagged with the theme (and amendment) it belongs to. The note embeds
the output between two markers; `--check` regenerates it at the head the note
names and fails on any difference.

A commit under crates/axon-core/src that THEMES does not classify is a FAILURE,
so a new interpreter change cannot reach the tree without being placed in the
note. `--check` also fails when a commit after the note's pinned head touches
crates/axon-core/src: the note is then stale and must be regenerated
(`scripts/pci_delta.py --emit HEAD`, paste between the markers, update the pin).

    python3 scripts/pci_delta.py --emit [REV]   # print the block for REV (default HEAD)
    python3 scripts/pci_delta.py --check        # compare with the note
"""
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NOTE = os.path.join(ROOT, "governance/notes/v022-pci-delta.md")
TRIAGE = os.path.join(ROOT, "governance/notes/v022-psv1-loop-triage.md")
BASE = "31413ca7"
PATHSPEC = "crates/axon-core/src"
BEGIN = "<!-- BEGIN MECHANICAL (scripts/pci_delta.py) -->"
END = "<!-- END MECHANICAL -->"

# commit (first 8 hex) -> (theme, what it does to pass/fail, by the commit's own message)
THEMES = {
    "abf72e0b": ("review fixes B1/B3 (main.rs)", "runner/test-CLI hardening from review wf_d725935a-7ed"),
    "bae904b8": ("keyed failure token (main.rs)", "the interpreter keys every FAILURE it decides, so a printed failure line is not a verdict: narrowing"),
    "a4a05f28": ("key and completion", "K is unreachable from candidate code: narrowing"),
    "e9a13f72": ("key and completion", "the raw key read is unix-only so the wasm32 build compiles: build fix"),
    "91c2f676": ("key and completion", "sealed handlers act only on sealed operations; completion is the body's end: narrowing"),
    "f2a12aab": ("key and completion", "row attack (Option-shaped test) where completion is the only guard: evidence"),
    "8f8e9755": ("sealed modules, module path", "an unreadable suite module never falls through to the candidate: narrowing"),
    "08903fdf": ("sealed modules, module path", "a sealed module's use resolves only in the sealed dirs: narrowing"),
    "38273ef1": ("sealed modules, module path", "one file per module name, suite first: narrowing"),
    "32fdea5c": ("sealed modules, module path", "the sealed-module rule holds whoever imports: narrowing"),
    "8e9858f4": ("RNG isolation", "a sealed frame may not reseed the process RNG: narrowing"),
    "b8bbdb74": ("RNG isolation", "a sealed frame may not DRAW from the process RNG: narrowing"),
    "bce231ad": ("RNG isolation", "RNG hardening completeness, M254 false retirement: narrowing"),
    "8a7962fb": ("RNG isolation", "per-kernel RNG isolation: narrowing"),
    "7da2fe71": ("rows, style, harness", "rustfmt and a row re-anchor: no intended change in semantics"),
    "e1e2a22f": ("rows, style, harness", "row cells and attack messages: no intended change in semantics"),
    "2a397d93": ("rows, style, harness", "row routes: no intended change in semantics"),
    "73834357": ("rows, style, harness", "+23 interp.rs lines with two defect fixes found making rows honest"),
    "48a077ee": ("amendment 74", "predicate primitives are refusal sites; keyed_outcome rowed"),
    "ba1bad03": ("amendment 74", "merge of c9r4c/gate"),
    "e8537726": ("amendment 53", "a value is cast at every declared boundary; an operator method name is the operator's: narrowing (A86)"),
    "3cf23eef": ("amendment 60", "the cast keys on the dispatch type; a local is never called by name: narrowing"),
    "8a5419de": ("amendment 60", "scalar-arm attacks reach their arms; M1155 compiles"),
    "fa0643ac": ("amendment 72", "at a seal crossing every type position is determined from the operator side or refused; E0505 refuses an impl for f32/isize/usize: narrowing"),
    "68176b53": ("amendment 72", "M1148/M1669 attacks: row repair"),
    "6b793da8": ("amendment 72", "the operator's dicts are snapshotted at a seal crossing and verified at every edge back: narrowing"),
    "3b9b9092": ("amendment 78", "a position the operator held is judged by what it held, deeply and strictly: narrowing"),
    "d29d4ef6": ("amendment 78", "row repairs (M1672, M1843, M1845)"),
    "2bf1d9d0": ("amendment 82", "comment-only (TestEnd doc)"),
    "40ca1092": ("amendment 82", "merge of c9r4c/claims; comment-only in this path"),
    "12c6685e": ("amendment 83", "the dispatch rule: operator code never selects an operator impl by a type nothing on the operator side determined (untyped dict/channel/lambda reads must be annotated; pin.rs): narrowing"),
    "554951b6": ("amendment 83", "unit-test case only (an unannotated lambda parameter case in interp.rs tests): no production change"),
    "9e19e961": ("amendment 83", "unit test and comments only (interpolation/comparison of an untyped read select no operator impl): no production change"),
    "b971194c": ("amendment 83", "clippy: the arithmetic arm of the pin walk (interp/pin.rs) collapsed into a guard: no change in what is refused"),
    "70692659": ("amendment 83", "unit-test attack text only (M1996: a shift truncates where + and * panic): no production change"),
    # origin/main merged into v022/veto (PR #8). These commits were written and tested on main, NOT
    # reviewed against the PCI claims: none is a PCI narrowing, and what they do to pass/fail was
    # not analysed there. The ones that touch the interpreter are listed as such so a reviewer reads them.
    "378da246": ("merged from main (AX findings)", "native agent_action audit record gains effect_row/principal; interp.rs and interp/builtins.rs change only by a `cfg` re-export and a visibility change: no change in what the interpreter evaluates or refuses"),
    "96a31eb8": ("merged from main (AX findings)", "native codegen only (place assignment, str + str, allocas): the interpreter is untouched"),
    "13f01eb8": ("merged from main (AX findings)", "INTERPRETER: arr_sort_by becomes a stable merge sort (interp/builtins.rs): not PCI-reviewed"),
    "be8576af": ("merged from main (AX findings)", "INTERPRETER: arrays become shared Rc values, cheaper user calls (interp.rs, eval.rs, value.rs): not PCI-reviewed; the seal's cast and dict-snapshot code was adapted to the Rc layout in the merge"),
    "36227ec7": ("merged from main (AX findings)", "native codegen only (non-escaping array literals on the stack): the interpreter is untouched"),
    "1727775e": ("merged from main (AX findings)", "LANGUAGE + INTERPRETER: `&mut [T]` parameters write through (parser, checker, interp.rs, eval.rs, new E0604-E0606): not PCI-reviewed; the merge routes `call_fn_mut` through the same seal edges as `call_fn`"),
    "edfe3e2d": ("merged from main (AX findings)", "native codegen, plus checker concat typing and resolver capture analysis (`axon check` accepts different programs): interpreter evaluation is untouched"),
    "19310764": ("merged from main (AX findings)", "LANGUAGE + INTERPRETER: named functions are first-class values (resolver, checker, effects, eval.rs): not PCI-reviewed"),
    "099ff710": ("merged from main (AX findings)", "build/cache/CLI only (runtime location, compiler-identity cache key): the interpreter is untouched"),
    "c4d307bd": ("merged from main (AX findings)", "CLI help text only: no change in what is refused"),
    "6493c2f2": ("merged from main (AX findings)", "native codegen/link only (LLVM pipeline, --opt-level): the interpreter is untouched"),
    "82b83298": ("merged from main (AX findings)", "native link flags only: the interpreter is untouched"),
    "1d01b014": ("merged from main (AX findings)", "INTERPRETER: shared copy-on-write strings, in-place append, lent closure captures (interp.rs, eval.rs, builtins.rs, value.rs): not PCI-reviewed; closures resolve a local through `call_local_closure` while the amendment-60 rule (a local is never called by name elsewhere) is kept"),
    "a4af40a1": ("merge of origin/main (PR #8)", "joins the commits above; combines main's `call_fn_in`/`call_fn_mut` with the seal as one `call_fn_sealed` path, adds `&mut T` and Rc-array arms to conform.rs/pin.rs, keeps amendments 53-83 on the merged call path: no intended change in what is refused (rows re-run on this head)"),
    "aa0dd7eb": ("env registry (integrate6)", "three TEST-ONLY re-exec markers registered (AXON_EQ_ALONE/GIT_ENV/HARDEN); a registry row for a var only tests read: no change in what is refused"),
    "fe00e4c5": ("merge of c9r4c/psv1e (integrate6)", "joins the psv1e dispatch-analysis rebuild (pin.rs `Tys`, amendment 88) with main's `&mut T` type and Rc layout (a `RefMut` arm in the closed-type walk, no env duplicate in the closure call): no intended change in what is refused"),
    "9acbb453": ("merge of origin/main (PR #8)", "merge fixes: `input_arg` read from the args (the verify panic's input suffix), the per-frame stack budget raised to 512 KiB because the merged debug frame is ~265 KB: no change in what is refused"),
    "dab96417": ("amendment 88", "the dispatch analysis trusts only what the operator chose: candidate fns/types/lets, local-name shadowing and trait names no longer determine a receiver; keys carry the owning fn; fail-closed lookup; operator-defined runtime values dispatch; the address cache is removed (pin.rs, interp.rs, eval.rs): narrowing on candidate-influenced receivers, widening only for operator-only polymorphism"),
    "edde3d5d": ("amendment 88", "clippy: an unused `mut` removed in pin.rs; no change in what is refused"),
    "f4a7a2dc": ("amendment 94", "a `&mut` write-through value is cast and verified at the seal edge back, and a `&mut` operand is open in the dispatch analysis; a sealed frame cannot take an operator fn as a first-class value; a union annotation does not pin a receiver (interp.rs, interp/eval.rs, interp/pin.rs, interp/conform.rs): narrowing"),
    "13374667": ("amendment 94", "the edge-back cast also runs on an aborted call; a provenance mark on fn values dropped (redundant with the creation edge); rows and tests: narrowing, no widening"),
    "2d70adf5": ("amendment 94", "the held-value judgement at the `&mut` edge is not strict, so an honest fill of an empty output array passes while the declared type judges the position (conform.rs `strict` parameter): the one deliberate relaxation, relative to the first form of this amendment, not to the merged tree"),
    "2c0203e0": ("amendment 96", "sandbox_run's result is cast to its declared i64 at the seal crossing; every read of an operator global goes through one lookup (`global_ref`) so the FieldAccess/Index fast paths and the closure-constant call apply the edge; a sealed frame's fn value carries its own mark (an operator fn value stays unmarked) and cannot replace an operator closure; unary `-x`/`~x` is judged by the width arm; drift tests for global reads and for builtins that run user code: narrowing"),
    "12cf6c55": ("amendment 96", "unit test only (the value of a with-handler expression is undetermined until pinned: a stated cost): no production change"),
    "8caedb78": ("amendment 94", "rustfmt only (interp.rs): no change in what is refused"),
    "b024c06e": ("amendment 88", "pin.rs: a redundant sealed-let filter removed (the push guard is the one rule); test-only otherwise; no change in what is refused"),
    "cc74956e": ("amendment 100", "a handler arm and its continuation replay run under the pin owner of the fn that INSTALLED the handler, not the one that performed the effect; the name argument of every name-resolving builtin (sandbox_run, scheduler_spawn, goal_*, kernel_goal_create) must be an operator-chosen name (Ctx::npure) or the call is refused; an operator fn and a missing one read the same to sealed code; drift tests: narrowing, no widening"),
    "c62e9b02": ("amendment 102", "a runtime taint: a value sealed code produced carries a taint every operation of the evaluator propagates (an accumulator around `eval`, a taint per binding, per shared object and per module-level let, an early-exit taint for a fn result and for stores), and an operator frame refuses to use a tainted value to SELECT operator code: the name given to a name-resolving builtin, an operator closure the candidate picked (out of a table by a key, index or branch) and then called, the impl a method call dispatches to (receiver type), and the width of fixed-width arithmetic; the static pin analysis is kept underneath; a plain `axon run` is unchanged (one branch per eval); drift tests for builtin classes, dict writers, dispatch sites, value holders and swept files: narrowing, no widening"),
    "83175bed": ("amendment 106", "a channel is one shared object with one taint: every channel method and every `select` arm examined takes it into the result, and a sealed frame's mutating access (send, recv, try_recv) marks it; a kernel or world write stores the control taint it ran under; a builtin runs its callbacks under the control taint of its arguments; string interpolation and the stringifying builtins take the taint of every shared object they print; a sealed caller is refused in ONE text on every path (a name, a global, a field or index, `goal_run`, a goal constraint, `kernel_goal_create`: `goal_run` of an operator fn no longer completes); routing and drift tests for every dict, kernel and array reader, 38 hunted programs: narrowing, no widening"),
    "1a75557b": ("amendment 106", "test only (the oracle test reads the verdict as Debug text; no production change)"),
    "ee1c7056": ("amendment 108", "a comparison (`==`, `!=`, ordering) takes the taint of every shared object (dict, channel) inside either operand, however deep, and every builtin argument is walked deep (no table of names): `if d == e {lenient} else {strict}` over a dict the candidate filled is refused like `dict_get_or`; a sealed method call is judged by the methods sealed impls define and a run-time method miss reads like an operator method (existence oracle, method path); a `native::` call is World state (a sealed call marks it, an operator call reads it back); drift and routing tests, 12 rows: narrowing, no widening"),
    "b8f176d9": ("amendment 108", "test only (a clippy `useless_format` in a taint test): no production change"),
    "8abeb02b": ("amendment 108", "builtin arguments are walked deep EXCEPT the first argument of ten builtins that only count, key into or append (`SHALLOW_FIRST_ARG`, fail-closed: unlisted is deep), which keeps a loop of a million `dict_set`s linear; tests: narrowing, no widening beyond amendment 108's first form"),
    "65d407d8": ("amendment 114", "the right operand of `&&`/`||` runs under the left's control taint (sealed runs, a cold function), a refused match guard, a `?` and a `select` arm raise theirs, `?` counts as an exit, the sealed files are checked by a second pipeline over the sealed items alone with the checker refusing the missing name wherever the operator's was (struct literal, lambda parameter, enum/trait/refinement types, generic bound, verify and refinement predicates); taint::CONTROL_TABLE drift-tested against ast.rs: narrowing, no widening"),
    "5d333913": ("amendment 114", "test and registry only (a taint test for the type a `?` carries, plant controls for the note check): no production change"),
    "2e68236b": ("amendment 114", "the sealed-only check holds the operator's `mod` of a candidate module (found by the fabric suite), so a candidate whose module uses its own helper resolves; an operator module the candidate does not ship is still missing: narrowing relative to the first form of amendment 114, no widening of what base accepted"),
    "da68ec7e": ("amendment 114", "test only (the module test loads sol.ax): no production change"),
    "c4ea5684": ("amendment 117", "follow-up to 3849fadb found by the full mutation run: two guards that proved redundant are removed (a sealed-caller check after goal_name_is_known; the operator-pop branch of t_chan_access, covered by t_chan_choice) and a test case for an aborting arm's value: no change in what is refused"),
    "3849fadb": ("amendment 117", "the clear, localized members of the round-14 find-until-dry loop (44 of 53 findings): `temporal_new`/`temporal_is_valid` are World and Time (a clock reader is no Pure builtin); a match guard, every `while`/`while let` condition evaluation after the first, every builtin callback after the first (`call_cb`) and every Rust loop that runs operator code (`t_loop_pc`: goal searches, the scheduler pass) run under the control taint of what decided them; an operator pop and the channel a `select` looked at are marked; an abort-capable `with` body treats a branch on tainted data as a possible exit and sealed code that ran in it raises the sticky taint, as does a scheduler fiber that ran sealed code and failed; a resume value keeps its taint; `dstore_*` is World, a zoned call's provenance push is kernel state and its append is world state; a refinement type pins what its base pins; a width inside an `Uncertain`/`Temporal` is a width; `sandbox_run` copies the operator's sandbox entry into the sealed kernel for the call; the principal token stream is per registry; the sealed-only check resolves `dyn`, array/tuple elements, refinement/`where`/`@[verify]`/whole-struct predicates and the exact deferred type names, and the goal variants and `goal_eval` give a sealed caller the missing-name text: narrowing, no widening (the claim itself is NARROWED to what is enforced)"),
    "b7aaa9c1": ("amendment 121", "the existence oracle at every placement and the operator-typed value the candidate picked (first cut): the parser's end-of-input span is no longer the dummy `0..0`; the resolver locates a type, a refinement and a top-level `let` at its item and keeps the run's own sealed set (an HONEST `fn lim(n: i64 where n >= 0)` is no longer refused for naming its parameter); a merged diagnostic with no line joins the backstop; a sealed top-level `let` initializer is name-checked; a third taint bit, `PICK`, marks an operator-typed value (or one holding it) that sealed code selected (an index, a builtin's read on a selector it chose, a branch, a store under its control, an exit it decided, a hand-back from a sealed fn or closure) and the dispatch rule refuses a receiver that carries it: narrowing, no widening"),
    "d7574f7d": ("amendment 121", "the split moves to `seal_split.rs` (library, so a gate row runs its test), the honest-registry control is made to bind the candidate's taint, doc and test wording: no change in what is refused beyond b7aaa9c1"),
    "45347cb7": ("amendment 121", "the backstop's first branch no longer appends the sealed-only diagnostics (empty there); three older rows re-anchored; the refusal-coverage exemption of the observer prune dropped (that file is not under crates/axon-core/src; the commit touches main.rs only by removing one line): no change in what is refused"),
    "416a6683": ("amendment 121", "the parser unit test that is M3450's own attack (`an_item_that_ends_the_file_has_a_real_span_in_its_own_file`); no production change in this commit (parser.rs gains a test only)"),
    # origin/main 5864c423 merged into r51/unify (R51 S0). Written and tested on main, NOT reviewed against
    # the PCI claims; R51 keeps every sealed run refused until the seal is re-homed onto this interpreter (S6).
    "cf286ad6": ("merged from main (R51 S0)", "INTERPRETER: feat(builtins): now_ns monotonic nanosecond clock in both engines (AX-32): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "f16d5295": ("merged from main (R51 S0)", "outside the interpreter (main.rs): fix(cli): axon run leaves stderr to the program; run-id only with --verbose or AXON_RECORD (AX-33): the interpreter is untouched"),
    "37ff592f": ("merged from main (R51 S0)", "outside the interpreter (cache.rs, codegen/link.rs, codegen/mod.rs, codegen/output.rs, error.rs, lib.rs, main.rs): perf(build): cache the optimised object so a hit only links (AX-34): the interpreter is untouched"),
    "855d93a5": ("merged from main (R51 S0)", "outside the interpreter (main.rs): fix(cli): axon build names the artifact it wrote, not always Binary (AX-39): the interpreter is untouched"),
    "e432b391": ("merged from main (R51 S0)", "outside the interpreter (codegen/link.rs, codegen/output.rs, main.rs): perf(codegen): run globaldce at O0 and prune AI wrappers for --emit-obj (AX-37): the interpreter is untouched"),
    "673e3004": ("merged from main (R51 S0)", "outside the interpreter (codegen/link.rs, env_registry.rs): perf(codegen): link prebuilt runtimes without running cargo, pinned to the workspace toolchain (AX-35): the interpreter is untouched"),
    "75f623b0": ("merged from main (R51 S0)", "outside the interpreter (builtins.rs, effects.rs): perf(effects): walk each body once and propagate effects over a worklist; O(1) builtin lookup (AX-36): the interpreter is untouched"),
    "4ddfbff7": ("merged from main (R51 S0)", "outside the interpreter (codegen/link.rs, codegen/output.rs, lib.rs, main.rs, time_passes.rs): feat(cli): axon check/build --time-passes per-phase timing report (AX-36): the interpreter is untouched"),
    "58934b2b": ("merged from main (R51 S0)", "outside the interpreter (codegen/link.rs, main.rs): perf(codegen): --opt-level s/z mark every definition optsize/minsize (AX-38): the interpreter is untouched"),
    "b43383a6": ("merged from main (R51 S0)", "merge commit (Merge branch 'ax/frontend-timing' into ax/cb-integration); joins its branch, no change beyond its parents"),
    "009c3291": ("merged from main (R51 S0)", "merge commit (Merge branch 'ax/codegen-size' into ax/cb-integration); joins its branch, no change beyond its parents"),
    "a107086b": ("merged from main (R51 S0)", "merge commit (Merge branch 'ax/runtime-libs' into ax/cb-integration); joins its branch, no change beyond its parents"),
    "1780bcde": ("merged from main (R51 S0)", "merge commit (Merge branch 'ax/build-cache' into ax/cb-integration); joins its branch, no change beyond its parents"),
    "32c7f3bf": ("merged from main (R51 S0)", "INTERPRETER: perf(interp): closures capture only their free variables; syms instead of names (AX-40, AX-18): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "11c44343": ("merged from main (R51 S0)", "INTERPRETER (via merge): merge commit (Merge branch 'ax/interp-closures' into ax/cb-integration); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false)"),
    "05132402": ("merged from main (R51 S0)", "INTERPRETER: style(interp): FxHasher reads 8-byte words with as_chunks (clippy): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "b156e0fd": ("merged from main (R51 S0)", "outside the interpreter (checker.rs, types.rs): fix(checker): resolve enums and refinements inside tuples and fn types (AX-41): the interpreter is untouched"),
    "213acb7b": ("merged from main (R51 S0)", "outside the interpreter (codegen/enum_layout.rs, codegen/expr.rs, codegen/match_pat.rs, codegen/mod.rs, codegen/types.rs): fix(codegen): lay out struct fields of enum type; place enum payload fields by name (AX-43): the interpreter is untouched"),
    "f7c0cf4f": ("merged from main (R51 S0)", "outside the interpreter (codegen/asi.rs, codegen/expr.rs, codegen/match_pat.rs, codegen/mod.rs): fix(codegen): each refusal printed and counted once; no E0701 cascade after a refusal (AX-45): the interpreter is untouched"),
    "79ab856e": ("merged from main (R51 S0)", "outside the interpreter (builtins.rs, codegen/expr.rs, codegen/mod.rs): fix(codegen): native arr_push/reverse/take/drop over any element type (AX-44): the interpreter is untouched"),
    "9e952186": ("merged from main (R51 S0)", "INTERPRETER: perf(interp): 16-byte Value, Box<str> Flow, Sym-keyed records, cached string literals (AX-46, AX-47): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "9d73a8b6": ("merged from main (R51 S0)", "merge commit (Merge branch 'ax/native-arr-push' into ax/cb-integration2); joins its branch, no change beyond its parents"),
    "da8d4b2d": ("merged from main (R51 S0)", "INTERPRETER: style(interp): rustfmt the AX-46/AX-47 record and Flow changes: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "837ab524": ("merged from main (R51 S0)", "INTERPRETER: perf(interp): inline Int/Float binop fast path shared with eval_binop_vals (AX-46): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "eb34ee05": ("merged from main (R51 S0)", "INTERPRETER: test(interp): pin record print order, equality, variant matching and narrow-field coercion (AX-47): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "25b5f63d": ("merged from main (R51 S0)", "outside the interpreter (codegen/enum_layout.rs, codegen/expr.rs, codegen/match_pat.rs, codegen/types.rs): fix(codegen): lower recursive enums by boxing self-referential payload fields (AX-42): the interpreter is untouched"),
    "a507e2ec": ("merged from main (R51 S0)", "merge commit (Merge branch 'ax/native-enum-layout' into ax/cb-integration2); joins its branch, no change beyond its parents"),
    "b92a7962": ("merged from main (R51 S0)", "merge commit (Merge branch 'ax/codegen-diag' into ax/cb-integration2); joins its branch, no change beyond its parents"),
    "d91575b8": ("merged from main (R51 S0)", "outside the interpreter (codegen/match_pat.rs): fix(codegen): enum field pattern test goes through build_wrappers::w_and (R1e): the interpreter is untouched"),
    "e46ec5a0": ("merged from main (R51 S0)", "INTERPRETER (via merge): merge commit (Merge branch 'ax/interp-value' into ax/cb-integration2); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false)"),
    "d1334115": ("merged from main (R51 S0)", "INTERPRETER: style: rustfmt sweep, layout only (compilebench AX-52): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "1a6282f5": ("merged from main (R51 S0)", "outside the interpreter (codegen/mod.rs): fix(codegen): give a tuple element binding its semantic type so an inner tuple destructures natively (AX-48): the interpreter is untouched"),
    "a7fc7778": ("merged from main (R51 S0)", "outside the interpreter (codegen/display.rs, codegen/expr.rs, codegen/mod.rs): fix(codegen): render interpolated structs, enums, Option/Result natively like the interpreter (AX-49): the interpreter is untouched"),
    "0e4030eb": ("merged from main (R51 S0)", "merge commit (Merge branch 'ax/struct-interp' into ax/cb-integration3); joins its branch, no change beyond its parents"),
    "c2385099": ("merged from main (R51 S0)", "outside the interpreter (codegen/build_wrappers.rs, codegen/display.rs): fix(codegen): display helpers emit through build_wrappers (R1e); w_float_ext: the interpreter is untouched"),
    "2c6266c0": ("merged from main (R51 S0)", "INTERPRETER: perf(interp): resolve locals to frame slots; read scalar binop operands in place (AX-53, AX-55): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "c3c34ecc": ("merged from main (R51 S0)", "INTERPRETER: perf(interp): user-fn call decides attrs once, reuses frames and arg buffers (AX-54): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "ff5ceb8e": ("merged from main (R51 S0)", "INTERPRETER (via merge): merge commit (Merge branch 'ax/interp-calls' into ax/cb-integration4); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false)"),
    "99b8b099": ("merged from main (R51 S0)", "INTERPRETER: R50 S0: engine selector, run_body, FnEntry.compiled, trace, extracted dispatch_call/bind_let/call_mut, AX-57 scope-leak fix: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "7af37c53": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: bytecode engine lowers the scalar core (literals, locals, blocks, let, assign, operators, if/while/for, return/break/continue, ?,...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "40881a3a": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: inline the extracted eval helpers' fast paths (tree loop.ax back under S0): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "d1346ad6": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: vm exec keeps Break/Continue catching off the common return path: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "58a6b01e": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: calls by name resolve the callee at compile time (dispatch_named); call_fn_in binds params by index: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "0ff60f9c": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: blocks, while iterations and for bodies that bind nothing push no scope; body exit builds no Unit to drop: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "3c7a1716": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: call frames pool as Box<Env> (a pointer moves, not the frame); frame clear forgets scalar bindings instead of running drop glue: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "fb35ba68": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: call_mut takes its frame from the call-frame pool (take_frame/give_frame) instead of allocating one per call: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "547b6029": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: current_fn holds the executing fn's &FnDef in a Cell (no Rc clone, no RefCell flag); one CallGuard restores it and the call depth: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "325f5a89": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: call_fn_in's common path (FnEntry::plain, no refinements) is guards, arity, bind, goal_met, body, result; attribute-driven steps ...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "2affff15": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: call_fn_in pushes params and goal_met into the empty frame (the define_var slot == len case, without its checks): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "4bd88b06": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: a VM call by name leaves its arguments on the operand stack (StackTail); a user-fn callee binds them from there, no take_args/rec...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "abf5976b": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: a bytecode activation's operand stack lives in its pooled frame (Env::stack) instead of the vm_stacks pool: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "733c4806": ("merged from main (R51 S0)", "INTERPRETER: R50 S1: env_pool is Vec<Box<Env>> on purpose (allow clippy::vec_box): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "3a67ec2c": ("merged from main (R51 S0)", "INTERPRETER: R50 S3: lower match, while let and method calls (MethodRecv); extract chan_method/impl_method: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "ef20af0c": ("merged from main (R51 S0)", "INTERPRETER: R50 S2: aggregates, index/field reads and place writes lower to ops: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "765bb811": ("merged from main (R51 S0)", "INTERPRETER (via merge): merge commit (Merge ax/r50-s3 (R50 S3 match, while let, methods)); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false)"),
    "89017cc5": ("merged from main (R51 S0)", "INTERPRETER: R50 S4: lambdas compile (make_closure, ClosureCode.compiled), allocation-free builtin -> closure call; fold.ax 330/element: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "12713cbd": ("merged from main (R51 S0)", "INTERPRETER (via merge): merge commit (Merge ax/r50-s4 (R50 S4 lambdas, allocation-free builtin -> closure call)); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false)"),
    "91796cac": ("merged from main (R51 S0)", "outside the interpreter (cache.rs): spec(R50): S2-S4 landed, evidence ledger; cache unit tests use per-test scratch dirs (parallel-test race): the interpreter is untouched"),
    "93877287": ("merged from main (R51 S0)", "INTERPRETER: R50 S5 (WIP): CallFast op (proven fast VM->VM call, inline-operand args), CallMut op over call_mut_with, fast_call_blocker + vm: slow tra...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "f0ce9745": ("merged from main (R51 S0)", "INTERPRETER: R50 S5: fast-call tests (mutcall no tree nodes, slow per flag, param/return semantics, tier clearing, depth limit, depth-before-arity, ha...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "fd8816b7": ("merged from main (R51 S0)", "INTERPRETER: R50 S5: allocation-free call_mut (both engines), CallMut op, leaf fast-call bodies; fastcall 257/300, mutcall 562/600: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "3eb79f1a": ("merged from main (R51 S0)", "INTERPRETER (via merge): merge commit (Merge ax/r50-s5 (R50 S5 fast calls, allocation-free call_mut, CallMut op)); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false)"),
    "8470d24c": ("merged from main (R51 S0)", "INTERPRETER: R50 S7: Pure and PureLoop ops, fold_leaf in arr_fold; vm: pure / vm: fold-leaf trace lines: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "0899ae24": ("merged from main (R51 S0)", "INTERPRETER (via merge): merge commit (Merge ax/cb-integration4 (R50 S5) into ax/r50-s7); joins its branch, no change beyond its parents; not PCI-reviewed, sealed runs refused on this tree (R51 SEAL_RUNTIME_COMPLETE = false)"),
    "08f0e6fe": ("merged from main (R51 S0)", "INTERPRETER: R50 S8 (WIP, gate not met): IndexDefine, WriteIndexIndex, BinStack, CallFastLocalInt, BranchReturn/leaf_arm, Moves &mut fast path: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "34f2bf90": ("merged from main (R51 S0)", "INTERPRETER: R50 S8: CallMut caches a frame-free MovesCall form (swap fused to one Swap move); fib trace op counts: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "a9176ce5": ("merged from main (R51 S0)", "INTERPRETER: R50 S6: AXON_ENGINE defaults to vm; docs (I-2 sentence, env rows, reference, wasm export): not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "db2d2eee": ("merged from main (R51 S0)", "INTERPRETER: R50 S9: deferred compile (second entry unless hot on entry), AXON_VM_EAGER, vm: defer trace; compilebench AX-58: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "64d4f997": ("merged from main (R51 S0)", "INTERPRETER: R50 rev 26 / AX-56: wasm32 recursion panics instead of trapping (guard 128, native-stack byte budget checked by scripts/wasm_stack_budget...: not PCI-reviewed; on this tree every sealed run is refused (R51 SEAL_RUNTIME_COMPLETE = false), so no sealed run executes it"),
    "13ee4b51": ("R51 S0 merge", "joins main 5864c423 with v022/veto c9ed2ddd taking MAIN's interpreter (interp.rs, interp/{eval,builtins,goal,proptest,value}.rs); veto's taint.rs/taint_tests.rs/pin.rs/conform.rs are NOT on this tree, so the seal they enforce is absent. Refuse-all pin in the same commit: Interp::build refuses any sealed run (SEAL_RUNTIME_COMPLETE = false), `axon test --seal` exits 2 before any result line, vm::compile refuses a sealed interp. Every PCI row that needs a seal edge is deferred (governance/status/r51-deferred-tests.json) until R51 S6"),
    "56389c53": ("R51 S0b", "INTERPRETER: ports veto's non-seal fixes onto main's interpreter on both engines (loop control contained at every frame edge, TestEnd/run_test_fn_outcome completion, Result/Option return-boundary check, least-trusted Uncertain source_tag; veto 4e7851bc, bcf9c0a7, 2175cc1b, 89256bd3, 91c2f676, ea9ba38d, 6de01247): not PCI-reviewed on this tree; every sealed run is still refused (R51 SEAL_RUNTIME_COMPLETE = false)"),
    "a45dc364": ("R51 S1", "engine pin: Interp::build selects the engine through the pure vm::engine_for(sealed, SEAL_RUNTIME_COMPLETE, override, AXON_ENGINE), which decides a sealed run first: refused while the seal is incomplete (unchanged on this tree, SEAL_RUNTIME_COMPLETE = false), refused when the VM is requested (AXON_ENGINE=vm or set_engine(Vm)) or AXON_ENGINE is invalid, else the tree; `axon test --seal` refuses the same before any result line. Unsealed selection unchanged. T1a-d (truth table, no compiled body under seal, CLI, drift sweeps over interp/ incl. vm/): no change in what is refused on this tree (every sealed run still is)"),
    "ea565f8e": ("R51 S1 merge", "joins R51 S0b (56389c53) and S1 (a45dc364); the only overlaps are appended tests (vm/tests.rs, cli_run.rs) and this table; interp.rs, vm/mod.rs and main.rs auto-merged: no change beyond the two commits it joins; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "f7ccb9e6": ("R51 S1", "test only (vm/tests.rs T1d): S0b's run_test_body is listed as an engine gate and exec_raw as a VM entry; no production change; every sealed run is still refused"),
    "d60ea237": ("R51 S5", "INTERPRETER static layer: ports v0.22's interp/pin.rs verbatim and adds the Seal state (sealed fns/globals/refines/types, operator method names), pins and frame provenance (with_frame, fn_is_sealed) to Interp; Interp::build reads the sealed set once and hands it to build_in, so tests pass it without publishing it; Seal and Pins are computed only for a non-empty sealed set (an unsealed build does no new work). No seal edge consumes them yet (S2/S3/S4); not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "afd9e294": ("R51 S0b perf", "INTERPRETER cost only: S0b's return boundary (sum_return) tests the value's tag first (ret_needs_check: only Struct..=Err can be asked anything) and reads one FnEntry::ret byte (RetCheck replaces ret_is_scalar + ret_sum); leaf_call checks such a result out of line (leaf_return_checked), declining a confused Result/Option to the full path as before. Same refusals on both engines (soft unwrap, Result/Option confusion); new cli_run test + fixture for the leaf-call refusal; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "426067dc": ("R51 S2.1", "INTERPRETER Env: binding storage moves into a private Bindings type (interp/env.rs) with a parallel taint vector present only in a sealed run; every binding writer takes the taint, the raw env.vars writes (bind_params, goal_met, closure lend/copy/write-back, VM param pushes, &mut move-in/out) become Bindings methods, get_var_idx is the one index-returning lookup, and the vectors are checked in step after every method (release panic when the store is present). Unsealed runs carry no store and every tree site passes taint 0; behaviour unchanged; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "6be3a637": ("R51 S2.2", "INTERPRETER Env factory: Env's constructors become private to interp/env.rs and every Env of a run is made by the Interp (new_env, env_from_snapshot, the frame pool moved into env.rs), which gives it a taint store exactly when seal.active; a given frame is emptied (taints with bindings), a snapshot whose store does not match the run panics, and a taint read from a store-less Env panics (never reads 0). Unsealed runs: same envs, no store, behaviour unchanged; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "8a0a268c": ("R51 S2 perf", "INTERPRETER cost-only: the Bindings taint store becomes a Vec plus a store flag, and the bytecode engine (never sealed) writes and removes bindings through const-generic _t::<false> forms, so the unsealed call path has no branch on the store (R50 §10 call-path budget restored); the release-mode in-step check runs on sealed writes and taint reads only (debug builds check every mutation); two pre-existing clippy lints in interp tests fixed. Behaviour unchanged; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "99f7927f": ("R51 S2.3 step 1", "INTERPRETER taint state: ports v0.22 interp/taint.rs (taint bits, Taint accumulator/pc/sticky cells, builtin class tables with now_ns classed WORLD like now_ms, builtin/provenance/native/loop hooks) onto main's Sym/slot Bindings, adds the Interp.taint field and records a module-level let's taint at init_globals in a sealed run. Nothing reads the taint yet (eval_t hook sites are the next step); behaviour unchanged; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "2ccc64c3": ("R51 S2.3 step 2a", "INTERPRETER evaluator dispatch: eval() branches once on seal.active to eval_tainted (the v0.22 accumulator wrapper, reading a binding's taint through the same slot lookup as its value) or eval_arm::<false>; eval_arm, eval_block, run_loop_body, eval_binop, eval_call and operand are const-generic over the run, recursing through eval_t::<T>, so the unsealed instantiation is main's evaluator. No taint hook yet; behaviour unchanged; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "819fe549": ("R51 S2.3 step 2b", "INTERPRETER taint hooks on the sealed instantiation only (eval_t::<true>): a block restores the accumulator between statements; let, refinement-predicate and pattern bindings carry the value's taint; assignments (plain, in place, place writes) store it with the pc/sticky control taint; comparisons read operands deep and untype, with no in-place operand read; &&/|| through short_circuit_tainted; if/match/while/while-let/for/select branch under t_branch; return and ? set the return taint (? also sticky); fmt-strings, channel send, handler arms (abortable count, payload and return-arm taint), native calls (World) and builtin dispatch (t_builtin_in, pc guard, t_builtin_out). dispatch_named, dispatch_call, match_pattern, bind_let, chan_method and assign_in_place become const-generic, the bytecode engine passing VM_SEALED=false. Unsealed runs: the T=false instantiation is main's code; behaviour unchanged; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "d21ed278": ("R51 S2.3 step 3", "TEST-ONLY taint unit tests (interp/taint_tests.rs, cfg(test)): drive the evaluator's sealed instantiation eval_t::<true> on a build_with Interp (the refuse-all pin bypassed, the build_in refusal pinned) and kill every S2 hook in a mutation table; every_builtin_has_a_taint_class, the stringifier/emitter and call_builtin drift tests; match_pattern_rt becomes pub(super) for them. Deferred registry re-slots the 8 S2 entries (the taint sinks are S3, the clock class is ported so S6) and adds one S3 unit test. Interpreter behaviour unchanged; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "e12ac5fa": ("R51 S2 merge", "joins S0b-perf (afd9e294) and S2 (426067dc..d21ed278); interp.rs, vm/mod.rs, sym.rs and eval.rs auto-merged, only this table and the note conflicted; no change beyond the commits it joins; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "6afab02a": ("R51 S2.3 step 4", "INTERPRETER frame and closure taint on the sealed instantiation only: call_fn_in_general and the closure call become const-generic frames (call_fn_frame / call_closure_frame::<T>); call_fn_common (plain entries only, and FnEntry::new now takes seal.active so no entry of a sealed run is plain) binds with 0. A frame binds its parameters with the arguments' taint plus pc (t_params), saves and restores the caller's pc/sticky (FrameCtl), reads taint.ret or taint.last at exit (t_body) and leaves entry|body|sticky in acc (t_frame_exit); refinement predicates, the postcondition, goal_met (ALL when a goal search ran) and struct-refinement predicates carry the frame's taints. A closure's capture cell becomes a typed Captures (bindings plus one taint per binding, sealed run only) moved by the lend, copied by the copy path and written back (TaintPort-5). t_loop_pc after builtin callbacks, goal evaluations and scheduler fibers; t_provenance_write at the zoned-call provenance writes. Unsealed: same code via ::<false>; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "191c8f51": ("R51 S2.3 step 4", "TEST-ONLY drift lists: the T1d VM-entry allow-list (vm/tests.rs) and user_code_runners' PRIMITIVES (interp.rs, cfg(test)) name the const-generic frames call_fn_frame and call_closure_frame that 6afab02a moved the VM entries into; spec T1d text revised to match. Interpreter behaviour unchanged; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "fd3c3ff0": ("R51 S3a", "INTERPRETER per-frame Kernel: the handle-addressed kernel tables (provenance stores, corrigibility latch, current principal, goal constraint, principals, scheduler, supervisors, stores, LLM gateways, kernel goals, sandboxes, active sandbox) move into a Kernel struct, kernels: [Kernel; 2] selected by k() from frame_sealed, so a sealed frame reaches only its own (the sealed registry draws tokens from TOKEN_DOMAIN_SEALED). Every RNG draw and reseed goes through Interp::rng_next/rng_reseed, refused for operator code above a sealed frame; the sealed kernel has its own stream seeded by a SHA-256 derivation, the operator kernel keeps main's process-global stream (unsealed sequence pinned bit-identical). sandbox_run copies the entered ceiling into the sealed kernel for the call. frame_sealed is never set in an unsealed run, so it reaches kernels[0] only; no seal edge enters a sealed frame yet; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "51a993cf": ("R51 S3a", "TEST-ONLY: PrincipalRegistry::is_sealed (cfg(test)) so a kernel unit test pins that the sealed kernel's registry is built with new_sealed; the call_builtin drift scan skips interp/kernel_tests.rs (a unit-test module calling the dispatcher directly, as taint_tests.rs does). Interpreter behaviour unchanged; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "be0636cb": ("R51 S3b", "INTERPRETER closure representation A (spec Fork 3, TaintPort-5/-15): ClosureVal holds its capture cell as Rc<RefCell<Captures>> plus a per-reference contract and typed SealMarks (sealed_origin, sealed_fnval), never capture entries; make_closure/fn_value set them only in a sealed run, the lend test counts the cell's holders, Value stays 16 bytes. Ports v0.22 interp/conform.rs (casts, Cx, closure and channel contracts, dict snapshots and edges, replaced_ok, handler edges) onto main's Sym/Rc value model with Interp fields chan_contracts/dict_snaps/dict_epoch/user_traits/trait_impls/refine_bases, parse_type_text, Fields::iter_mut, t_chan_access/t_chan_choice in the select and method-call arms (sealed instantiation only), addr_of covering a closure's capture cell. The seal edges are not called from production yet (S3c); unit tests in interp/conform_tests.rs. Unsealed behaviour unchanged; not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "1215159c": ("R51 S3b", "TEST-ONLY unit tests (interp/conform_tests.rs, cfg(test)) for the channel access hooks t_chan_access/t_chan_choice in the method-call and select arms, plus their drift test. Interpreter behaviour unchanged: not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "769f45f2": ("R51 S3b", "TEST-ONLY unit tests (interp/conform_tests.rs) that kill the conform.rs mutation survivors (dict snapshot bound, epoch retake, first sealed mutation, fresh snapshot kept, dict-in-dict walk, array element judgement, cast depth bound, enum name); cast_at widened to pub(super) for the depth test, no behaviour change. Not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "0faf4372": ("R51 S3 merge", "joins S3a (fd3c3ff0..51a993cf, Kernel split) and S3b (be0636cb..769f45f2, closure representation A, conform.rs); only the interp.rs mod declarations, this table and the note conflicted, resolved by keeping both sides; no change beyond the commits it joins; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "0cc34976": ("R51 S3d", "INTERPRETER seal sinks inside evaluation, every check gated on seal.active or the sealed instantiation (T): globals read only through global_ref (seal_global; is_global for the in-place existence tests, eval.rs and vm/mod.rs), the name rule at the eval call site and the name-resolving builtins (seal_name_args via pin.rs Ctx::npure, t_check_names), fn_by_name/no_such_fn with the uniform sealed text (scheduler_spawn, kernel_goal_create, goal_run_constrained, ident_unbound, unknown call), seal_width/seal_width_unary/t_check_width in eval_binop and the unary arm, seal_refine at the param/return/let/record refinement sites, seal_type and cast_field in finish_record, the let-annotation cast with the t_pins TYP strip, seal_method/seal_dispatch in the tree MethodCall arm (impl_method_sealed), chan_created and chan_send_check/dict_edge_in on the chan builtin and send; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "56095a12": ("R51 S3d", "TEST-ONLY: interp/sink_tests.rs drives every S3d edge on a build_with sealed Interp, pins global_ref and the main-only sink classification by source sweep, ports the Pure-builtin ambient-state sweep (veto taint_tests.rs:2427); taint_tests' call_builtin site sweep skips the new test module; 14 deferred-registry entries re-slotted (edge ported, needs a live sealed run); no behaviour change"),
    "57e4b8cc": ("R51 S3c", "INTERPRETER seal crossing (spec §11 S3, TaintPort §1.2): call_fn_in_general's sealed instantiation goes through call_fn_sealed (seal_call, dict_edge_in on the way in, with_frame under the callee's provenance and pin owner, catchable/abortable sticky, dict_edge_out on the way out); call_fn_frame casts params (bind_params_sealed, fn_cx per activation, t_pins TYP strip, a sealed frame's params take ALL) and the return (seal_ret); call_mut_with's sealed path casts the &mut hand-back (replaced_ok_top); the closure frame casts arguments and result across the seal (closure_args_check/closure_ret_check, param_types pins, dict edges); handler frames carry sealed/operator_frames/pin_owner, arms run through handler_edge_into/back under with_frame and handler_may_answer gates the replay feed; dict_set/dict_remove/dict_inc mark dict_mutated; sandbox_run casts a sealed fn's result to i64; goal_eval and sandbox_run refuse a sealed caller with the uniform no_such_fn text. Every addition is gated on seal.active or the T=true instantiation; unsealed path unchanged. Every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "c8d21f1f": ("R51 S3c", "INTERPRETER scheduler pass (veto builtins.rs:468-482, amendment 117 loop findings 15/45): builtin_scheduler_run_once raises taint.catchable around each fiber's call, so call_fn_sealed's crossing sticky fires inside a fiber, and sets the sealed-entry sticky (VAL) after a fiber that entered a sealed frame (taint.entries moved) and failed; in an unsealed run entries never moves, so its behaviour is unchanged. Plus TEST-ONLY unit tests (interp/crossing_tests.rs, interp/taint_tests.rs) for handler_edge_into/back, the fiber pass, and the t_pins TYPE strips at a fn parameter, a closure parameter and an operator return. Not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "9453352e": ("R51 S3 merge", "joins S3c (57e4b8cc..582ced51, the seal crossing) onto S3d (0cc34976..d9881963, seal sinks); conflicts resolved by keeping both sides (handler_may_answer from S3c, both drift-scan skips, both THEMES blocks) and S3c's more specific ignore reasons, with the registry's by_slice recounted from its entries; no change beyond the commits it joins; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "72009129": ("R51 S3 merge", "INTERPRETER dict_set/dict_remove/dict_inc call dict_mutated without an outer seal.active guard, as veto does (builtins.rs:5286/5318/5470); dict_mutated returns at once when the seal is inactive, so an unsealed run does the same work less one branch. TEST-ONLY: the main-only sink classification names call_fn_sealed for call_fn_in_general (S3c moved the sealed instantiation behind the crossing). Both were drift tests from S3d that S3c's code tripped at the merge. Not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "2bf3e4a4": ("R51 S3e", "INTERPRETER init_globals in a sealed run (veto interp.rs:3292-3303): init_globals_sealed runs each module-level initialiser under its own definition's provenance (with_frame) in a fresh env, so a candidate's `let` meets the call and global-read edges (an earlier global is read through global_ref) and its kernel effects land in the candidate's kernel; the let's taint is recorded by t_set_global. The unsealed path is unchanged (one shared env, no taint bookkeeping, now without the dead seal.active branch). Plus TEST-ONLY sink tests (every taint carrier into the name rule, the name half of channel taint, own fn values, definition provenance, a candidate impl answering an operator method) and the last 5 S3 deferred-registry entries re-slotted (4 to S6, 1 to S4 for PICK). Not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "d25cae8c": ("R51 S3e", "TEST-ONLY: rustfmt of interp/sink_tests.rs (line wrapping of the S3e tests); no behaviour change; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "65eda84a": ("R51 S4b", "INTERPRETER receiver dispatch rule at run time (veto taint.rs:1414-1476): impl_method_sealed (the tree MethodCall arm under T) takes the receiver's own taint and calls t_check_dispatch after seal_method/seal_dispatch; t_check_dispatch returns at once unless the receiver carries TYP or PICK (new taint bit, veto taint.rs:71), and t_check_dispatch_tainted refuses an untyped or picked receiver when the method selects between operator impls. t_rules (veto taint.rs:725) gates t_check_names/t_check_width/t_check_dispatch: always true outside cfg(test). Per-thread cfg(test) switches DISPATCH_RULE_OFF (read by seal_name_args, seal_dispatch, seal_width, seal_width_unary) and TAINT_FORCE_ON (read by t_rules), default off. goal.rs: run_goal/_warm/_random/_categorical/_multistart/_tournament apply seal_call to the goal fn, and unknown_goal_name gives a sealed caller the no-such-fn text (veto goal.rs, interp.rs:4946). TEST-ONLY interp/dispatch_tests.rs; sink_tests turn TAINT_FORCE_ON on where they judge the runtime taint. The VM and unsealed runs never reach the new code (impl_method_sealed only under T; seal_call returns unless seal.active); every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "5fea0339": ("R51 S4b", "INTERPRETER make_closure records the creating frame's pin owner in a sealed run (cv.seal.pin_owner = pin_fn, veto PIN_FN_MARK eval.rs:967-972); dispatch_named in a sealed run refuses a non-closure value a local holds (not_callable) instead of falling through to a fn of the same name (veto eval.rs local-call rule, amendment 60); both under seal.active, so an unsealed run is unchanged. TEST-ONLY T8.1 dispatch tests (undetermined positions, width/name, goal names) under Both/TaintOnly/StaticOnly/Neither with controls; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "bbf8be12": ("R51 S4b", "TEST-ONLY: v0.22's S4 dispatch rows ported under their v0.22 names in interp/tests_v022_s4.rs (included at the end of interp.rs mod tests; ignored R51 S6, they need a live sealed run); tests/dispatch_rule_switch_is_test_only.rs un-ignored and extended to TAINT_FORCE_ON; drift tests for the goal-name call edge and the impl_method/dispatch_named sink classification; no behaviour change; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "3bd4709b": ("R51 S4b", "TEST-ONLY: #[ignore] reasons of the dispatch-rule deferred tests (interp.rs unit tests, axon-psv sealed_frames) re-slotted to S6 with the registry; no behaviour change; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "ce42eeb2": ("R51 S4b", "TEST-ONLY: operator_arithmetic_never_runs_at_a_width_the_candidate_chose moved from interp/sink_tests.rs to interp::tests (interp/tests_v022_s4.rs, the v0.22 module path) with v0.22's runner half, which asserts SEALED_RUN_REFUSED until S6; sink_tests helpers with_ix and WIDTH widened to pub(super); no behaviour change; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "5205763e": ("R51 S6 gaps", "INTERPRETER (both engines, S0b-class correctness fix) assert_eq_f64 is stated positively, !(a == b || |a-b| <= 1e-9), as veto builtins.rs:3745, so a NaN operand FAILS on the tree and the VM (they share the arm) as it does natively (OEQ); inf == inf still passes, and every non-NaN pair behaves as before. INTERPRETER seal edge (veto goal.rs:274/373/476/554, interp.rs:4959): run_goal_random/categorical/multistart/tournament call seal_call after the name lookup and unknown_goal_name gives a sealed caller the uniform no-such-fn text, so a sealed caller cannot tell an operator fn from a missing name; both are no-ops unless seal.active and the frame is sealed, so an unsealed run is unchanged. TEST-ONLY: vm/tests.rs NaN parity test, sink_tests.rs goal-search seal test and the clock-builtin World/Time check (veto taint_tests.rs:2498-2516), an ATTACK marker on the T1c CLI refusal asserts. Not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "748f0715": ("R51 S6 gaps", "TEST-ONLY: no_arm_of_a_pure_builtin_reads_or_advances_ambient_state moves verbatim from interp/sink_tests.rs to interp/taint_tests.rs, v0.22's module path, so the M3323/M3329 rows and the am117 gate row name a test that exists; no behaviour change; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "ddab14e1": ("R51 S6 gaps", "INTERPRETER revert of 5205763e's goal-name seal edge (the seal_call after the lookup in run_goal_random/categorical/multistart/tournament and the seal-aware unknown_goal_name): r51/s4b 65eda84a ports the same edge and merges first, so goal.rs and unknown_goal_name return to the base exactly; the unsealed path never changed. TEST-ONLY: the duplicate sink_tests goal-search test is dropped (S4b's dispatch_tests cover it). Not PCI-reviewed; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
    "23be800c": ("R51 S6 gaps merge", "joins r51/gaps (5205763e..5a739bf7: NaN assert_eq_f64, M3483 row, no_arm move to interp::taint_tests, M3342 re-anchor) onto S4b (27836b1c); only this table and the note conflicted, resolved by keeping both sides; goal.rs/unknown_goal_name are S4b's (the gaps lane reverted its own copy); no change beyond the commits it joins; every sealed run is still refused (SEAL_RUNTIME_COMPLETE = false)"),
}


# `--plant KEY:OLD=>NEW` (KEY: note | verdict | gates | spec | matrix | triage) changes the text of one input IN MEMORY before
# `--check` judges it: the control that shows a wrong table row, a wrong result, a wrong package or a wrong
# claim list is REFUSED (crates/axon-core/tests/pci_delta_note.rs). A plant that matches nothing is an error, so
# a control cannot pass because it planted nothing.
PLANTS = {}


def read(key, path):
    text = open(path).read()
    for old, new in PLANTS.get(key, []):
        if old not in text:
            print(f"pci_delta: --plant {key}: {old!r} is not in {os.path.basename(path)}", file=sys.stderr)
            sys.exit(3)
        text = text.replace(old, new, 1)
    return text


def git(*a):
    return subprocess.run(["git", *a], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def block(rev):
    full = git("rev-parse", rev).strip()
    out = [f"generated-at: {full}", "", f"`git diff --numstat {BASE}..{full[:8]} -- {PATHSPEC}`:", "",
           "| file | added | removed |", "|---|---|---|"]
    ta = tr = 0
    for line in git("diff", "--numstat", f"{BASE}..{full}", "--", PATHSPEC).splitlines():
        a, r, f = line.split("\t")
        ta += int(a); tr += int(r)
        out.append(f"| `{f}` | {a} | {r} |")
    out.append(f"| total | {ta} | {tr} |")
    out += ["", f"`git log --reverse {BASE}..{full[:8]} -- {PATHSPEC}`:", "",
            "| commit | theme | what it does to pass/fail (from its message) |", "|---|---|---|"]
    missing = []
    n = 0
    for line in git("log", "--reverse", "--format=%H %s", f"{BASE}..{full}", "--", PATHSPEC).splitlines():
        h, subj = line.split(" ", 1)
        n += 1
        t = THEMES.get(h[:8])
        if t is None:
            missing.append(f"{h[:8]} {subj}")
            t = ("UNCLASSIFIED", "")
        out.append(f"| {h[:8]} | {t[0]} | {t[1]} |")
    out.append(f"| {n} commits | | |")
    return "\n".join(out), missing


def registry_ranges():
    """{amendment: (lo, hi, ids)} for the mutation rows tagged `(amN` in their description
    (amendments 102 onward tag their rows): the ranges the note and the claim quote are
    DERIVED from the registry here, never retyped."""
    sys.path.insert(0, os.path.join(ROOT, "scripts"))
    import v022_g01_mutations as reg  # noqa: E402

    out = {}
    for r in reg.MUTATIONS:
        mm = re.search(r"\(am(\d+)\b", r[1])
        if mm:
            out.setdefault(int(mm.group(1)), []).append((int(r[0][1:]), r[7].split("::")[-1], r[5], r[6], r[7]))
    return {n: (min(i for i, *_ in v), max(i for i, *_ in v), v) for n, v in out.items()}


GATES = os.path.join(ROOT, "scripts/v022_pci_gates.sh")
# The cargo invocation the gate script builds for each row, as the lines that build it say.
INVOCATION = (
    'cargo test --locked -q -p "$pkg" "${feat[@]}" "${sel[@]}" -- --exact $names',
    'if [ "$target" = lib ]; then sel=(--lib); else sel=(--test "$target"); fi',
)
# Amendments in the note's table that are not interpreter-EVALUATION deltas the claim lists:
# 74 made predicate primitives refusal sites and rowed `keyed_outcome` (test CLI, not evaluation).
NOT_IN_CLAIM_LIST = {74}


def gate_rows():
    """[(label, package, target, [exact test names])] parsed from the script's ROWS array, and the
    script text. The invocation is the one the script builds from those fields, checked below."""
    text = read("gates", GATES)
    rows = [
        (m.group(1).strip(), m.group(2), m.group(3), m.group(4).split())
        for m in re.finditer(r'^  "([^"|]*)\|([^"|]*)\|([^"|]*)\|([^"|]*)"$', text, re.M)
    ]
    return rows, text


def gate_runs(rows):
    """{(package, cargo target selector, exact test name)} the gate runs: `--lib` for `lib`, else
    `--test <target>`, which is how registry rows spell their target."""
    out = set()
    for _, pkg, target, names in rows:
        sel = "--lib" if target == "lib" else f"--test {target}"
        out.update((pkg, sel, n) for n in names)
    return out


def doc_drift():
    """The amendment -> matrix-rows and amendment -> mutation-ids statements of the note and of
    the claim, each checked against the file that owns it."""
    bad = []
    note = read("note", NOTE)
    spec = read("spec", os.path.join(ROOT, "governance/specs/v022-psv-protocol.md"))
    verdict = read("verdict", os.path.join(ROOT, "governance/specs/v022-protected-suite-verdict.md"))
    matrix = read("matrix", os.path.join(ROOT, "governance/specs/v022-psv-negative-matrix.md"))
    have = {int(m.group(1)) for m in re.finditer(r"^\|\s*A(\d+)\s*\|", matrix, re.M)}
    reg = registry_ranges()
    grows, gates = gate_rows()
    runs = gate_runs(grows)
    for shape in INVOCATION:
        if shape not in gates:
            bad.append(f"the gate script no longer builds its cargo invocation as pci_delta.py parses it: missing `{shape}`")
    for line in note.splitlines():
        m = re.match(r"\| amendment (\d+) \|(.*)", line)
        if not m:
            continue
        n = int(m.group(1))
        mr = re.search(r"matrix (A\d+)(?:-(A\d+))?", m.group(2))
        if mr:
            lo = int(mr.group(1)[1:])
            hi = int(mr.group(2)[1:]) if mr.group(2) else lo
            # The amendment's own text states its rows (and the integrator rewrites that text
            # when it renumbers), so the note must say what the amendment says.
            i = re.search(r"^%d\. \*\*" % n, spec, re.M)
            st = re.search(r"matrix rows? A(\d+)(?:-A(\d+))?", spec[i.start() : i.start() + 3500]) if i else None
            if st:
                slo = int(st.group(1))
                shi = int(st.group(2)) if st.group(2) else slo
                if (slo, shi) != (lo, hi):
                    bad.append(f"the note gives amendment {n} matrix rows A{lo}-A{hi}; the amendment says A{slo}-A{shi}")
            miss = [k for k in range(lo, hi + 1) if k not in have]
            if miss:
                bad.append(f"the note gives amendment {n} matrix rows A{lo}-A{hi}; the matrix has no A{miss[0]}")
        if n in reg:
            want = f"M{reg[n][0]}-M{reg[n][1]}"
            if want not in m.group(2):
                bad.append(f"the note gives amendment {n} mutation ids other than the registry's {want}")
    for n, (lo, hi, ids) in sorted(reg.items()):
        if f"M{lo}-M{hi}" not in note:
            bad.append(f"the note does not give amendment {n}'s registry range M{lo}-M{hi}")
        # Every row of the amendment fails a test some gate row RUNS: the package, the cargo
        # target and the exact name of the invocation the script builds (not a substring of it).
        for _, t, pkg, tgt, full in ids:
            if (pkg, tgt, full) not in runs:
                bad.append(f"amendment {n}: the test {full} ({pkg} {tgt}) of a mutation row is run by no gate row")
    for m in re.finditer(r"am(\d+) \(M(\d+)-M(\d+)\)", verdict):
        n = int(m.group(1))
        if n in reg and (int(m.group(2)), int(m.group(3))) != reg[n][:2]:
            bad.append(f"the claim quotes am{n} as M{m.group(2)}-M{m.group(3)}; the registry says M{reg[n][0]}-M{reg[n][1]}")
    # The coverage table of the note is the gate script's rows plus the am102 sweep: label, package/target
    # AND the result each row prints (`PASS n/n`, n the number of named tests), not the labels alone.
    t = re.search(r"\| row \| package/target \| result \|\n\|[-|]*\|\n((?:\|.*\n)+)", note)
    if not t:
        bad.append("the note has no gate-row coverage table")
    else:
        lab = lambda x: re.sub(r"\s+", " ", x).strip()
        tab = sorted((lab(c[1]), lab(c[2]), lab(c[3])) for c in (l.split("|") for l in t.group(1).splitlines()))
        want = sorted(
            [(lab(l), f"{pkg}/{tg}", f"PASS {len(ns)}/{len(ns)}") for l, pkg, tg, ns in grows]
            + [(lab("am102 sweep (only the taint on: exactly the two static-only programs differ)"), "axon-core/lib (PSV1T_TAINT_ONLY)", "PASS")]
        )
        if tab != want:
            only_t = [x for x in tab if x not in want][:3]
            only_s = [x for x in want if x not in tab][:3]
            bad.append(f"the note's coverage table ({len(tab)} rows) is not the gate script's rows plus the sweep ({len(want)}): table-only {only_t}, script-only {only_s}")
    # The claim's two amendment lists are derived, not retyped: the delta amendments are the note's table
    # rows (bar those that are not evaluation deltas), and the amendments whose ARMS are verified to fail a
    # gate row are am100 and every amendment the registry tags.
    delta = sorted({int(x) for x in re.findall(r"^\| amendment (\d+) \|", note, re.M)} - NOT_IN_CLAIM_LIST)
    m = re.search(r"plus amendments ([0-9/]+) \(the delta", verdict)
    if not m or [int(x) for x in m.group(1).split("/")] != delta:
        bad.append("the claim's delta amendment list is not the note's table: expected " + "/".join(map(str, delta)) + ", the claim says " + (m.group(1) if m else "nothing"))
    arms = sorted({100} | set(reg))
    m = re.search(r"and the arms of\s+((?:am\d+, )*am\d+(?: and am\d+)?) \(", verdict)
    got = sorted(int(x) for x in re.findall(r"am(\d+)", m.group(1))) if m else None
    if got != arms:
        bad.append("the claim's list of amendments whose arms are verified is not am100 plus the registry's: expected " + ", ".join(f"am{x}" for x in arms) + ", the claim says " + (m.group(1) if m else "nothing"))
    # The PSV-1 claim's list of what is NOT claimed names every finding the loop's triage table decided
    # NARROW-CLAIM (amendment 117), and nothing else: a finding cannot be narrowed in the table and
    # silently dropped from the claim, and the claim cannot keep an entry the table no longer narrows.
    tri = read("triage", TRIAGE)
    tm = re.search(r"<!-- BEGIN TRIAGE.*?-->\n(.*?)\n<!-- END TRIAGE -->", tri, re.S)
    if not tm:
        bad.append("the triage table has no BEGIN TRIAGE / END TRIAGE markers")
        narrowed, decided = set(), {}
    else:
        decided = {}
        for line in tm.group(1).splitlines():
            c = [x.strip() for x in line.strip().strip("|").split("|")]
            if len(c) >= 4 and c[0].isdigit():
                decided[c[1].strip("`")] = c[3]
        narrowed = {sig for sig, dec in decided.items() if dec == "NARROW-CLAIM"}
        for sig, dec in decided.items():
            if dec not in ("FIX", "NARROW-CLAIM", "NOT-A-RUBRIC-ISSUE"):
                bad.append(f"the triage table gives finding `{sig}` the decision {dec!r}")
        if len(decided) < 53:
            bad.append(f"the triage table has {len(decided)} findings, the loop confirmed 53")
    nm = re.search(r"<!-- BEGIN PSV-1 NON-CLAIMS.*?-->\n(.*?)\n<!-- END PSV-1 NON-CLAIMS -->", verdict, re.S)
    if not nm:
        bad.append("the claim has no BEGIN PSV-1 NON-CLAIMS / END PSV-1 NON-CLAIMS list")
        listed = set()
    else:
        listed = set(re.findall(r"^\s+- `([a-z0-9_-]+)`\s*$", nm.group(1), re.M))
    for sig in sorted(narrowed - listed):
        bad.append(f"the finding `{sig}` is NARROW-CLAIM in the triage table and is not in the claim's non-claim list")
    for sig in sorted(listed - narrowed):
        bad.append(f"the claim's non-claim list names `{sig}`, which the triage table does not mark NARROW-CLAIM")
    # The list of non-retired rows whose target is the interpreter, with its count.
    sys.path.insert(0, os.path.join(ROOT, "scripts"))
    import v022_g01_mutations as regm  # noqa: E402

    ids = sorted(int(r[0][1:]) for r in regm.MUTATIONS if r[2].startswith(PATHSPEC) and r[0] not in regm.RETIRED)
    runs, a = [], ids[0]
    for x, y in zip(ids, ids[1:] + [None]):
        if y != x + 1:
            runs.append(f"M{a}" if a == x else f"M{a}-M{x}")
            a = y
    want_list = f"{len(ids)} rows at this head, recomputed from the script): " + ", ".join(runs)
    if want_list not in note:
        bad.append("the note's list of interpreter mutation rows is not the registry's: expected " + want_list[:120] + " ... " + want_list[-80:])
    return bad


def main():
    args = sys.argv[1:]
    while "--plant" in args:
        i = args.index("--plant")
        spec = args[i + 1]
        key, _, rest = spec.partition(":")
        old, sep, new = rest.partition("=>")
        if not sep or key not in ("note", "verdict", "gates", "spec", "matrix", "triage"):
            print("pci_delta: --plant KEY:OLD=>NEW with KEY in note|verdict|gates|spec|matrix|triage", file=sys.stderr)
            return 2
        PLANTS.setdefault(key, []).append((old, new))
        del args[i : i + 2]
    sys.argv[1:] = args
    if len(sys.argv) >= 2 and sys.argv[1] == "--emit":
        # `--drop HASH8` forgets one classification: the control that shows an
        # unclassified commit is refused (crates/axon-core/tests/pci_delta_note.rs).
        if "--drop" in sys.argv:
            THEMES.pop(sys.argv[sys.argv.index("--drop") + 1], None)
        b, missing = block(sys.argv[2] if len(sys.argv) > 2 else "HEAD")
        print(BEGIN + "\n" + b + "\n" + END)
        if missing:
            print("UNCLASSIFIED: " + "; ".join(missing), file=sys.stderr)
            return 1
        return 0
    if len(sys.argv) == 2 and sys.argv[1] == "--check":
        text = open(NOTE).read()
        m = re.search(re.escape(BEGIN) + r"\n(.*?)\n" + re.escape(END), text, re.S)
        if not m:
            print("pci_delta: the note has no mechanical block")
            return 1
        pin = re.search(r"^generated-at: ([0-9a-f]{40})$", m.group(1), re.M)
        if not pin:
            print("pci_delta: the block names no generated-at head")
            return 1
        want, missing = block(pin.group(1))
        bad = []
        if missing:
            bad.append("unclassified commit(s): " + "; ".join(missing))
        if want != m.group(1):
            bad.append("the note's block is not what git says at its own pinned head")
        rows = len(re.findall(r'^  "[^"]*\|[^"]*\|[^"]*\|[^"]*"$', open(os.path.join(ROOT, "scripts/v022_pci_gates.sh")).read(), re.M))
        for q in re.findall(r"v022_pci_gates[^\n]{0,40}?(\d+) rows", open(NOTE).read()):
            if int(q) != rows:
                bad.append(f"the note quotes {q} gate rows, the script has {rows}")
        if re.search(r"v022_pci_gates\.sh`?,? \d+ rows", open(os.path.join(ROOT, "governance/specs/v022-protected-suite-verdict.md")).read()):
            bad.append("the verdict spec quotes a hand-typed gate row count")
        bad += doc_drift()
        later = git("log", "--format=%h %s", f"{pin.group(1)}..HEAD", "--", PATHSPEC).strip()
        if later:
            bad.append("commits after the pinned head touch " + PATHSPEC + " (regenerate the note):\n" + later)
        if bad:
            print("pci_delta: FAIL\n" + "\n".join(bad))
            return 1
        print("pci_delta: PASS")
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
