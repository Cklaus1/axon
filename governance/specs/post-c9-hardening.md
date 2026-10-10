# Post-C9: hardening backlog (PSV-1 taint, refusal-site gate, build environment, test hygiene)

Status: **DRAFT, not normative, not under review, not in C9 scope.** Recorded 2026-10-09 at the operator's request
("take the non-blockers and write them up in a spec"). Nothing here changes what C9 accepts or claims. The C9 claims are the
ones in `governance/specs/v022-protected-suite-verdict.md` and `governance/specs/v022-psv-protocol.md` (amendments 88-117), and
where this document and those disagree, those win. This spec exists so that what was found, decided "not now" and written
down as a non-claim is not lost when the C9 branches are merged and the notes under `/var/tmp` are deleted.

Companion: `governance/specs/post-c9-keys-and-signatures.md` (items I-N: typed keys, signature container, root endorsement, LLVM 21,
host binding). It is cross-referenced here, not duplicated. It has no item M heading; the harden() inherited-state resets
(amendment 73, parked) are the item the brief calls M and are recorded below as PH-C13 only as a pointer.

These items touch the protected TCB (interpreter taint, trust-root parsing, the root launcher, the build environment). Each one
needs its own amendment, rows and a review round before it lands. No item may weaken an accepted C9 property.

## How to read

* **Ids.** `PH-<part><n>`. A heading `#### PH-A1.` is the item; Part F orders them; Part G maps every finding of the round-14 loop
  to an item or to "fixed in amendment 117".
* **Source** names a path or an amendment, never a memory. `r8`..`r13` are the review rounds; their reports were
  `/var/tmp/c9r<N>-findings-<GATE>.json` (scratch, not kept in the repo); the loop is `/var/tmp/c9r14-loop/confirmed-indexed.json`
  (53 findings; the repo keeps its table in `governance/notes/v022-psv1-loop-triage.md`).
* **Sev** is the severity the reviewer or triage gave when it was found (BLOCKER-class / MAJOR-ADJACENT / MINOR / FUTURE), not a
  re-rating. **Closing test** is what must exist before the item may be called closed: a row killed by its own attack, plus an
  honest control that keeps passing.
* **"Not re-verified"** means the source says the item existed in round N and this author did not find, in amendments 88-117, a text
  that closes it. It may be closed; it is listed so that someone checks rather than assumes.
* This document is held by `scripts/check_postc9_spec.py` (gate.sh; test `postc9_spec.rs`): every signature of the triage table
  is in Part G, every `PH-` heading is in Part F and the reverse, and the PSV-1 non-claims section of the verdict spec names this file.

## Part A. PSV-1 and the runtime taint (the long tail)

What C9 claims, in one line: a candidate cannot choose WHICH operator code runs, or which operator impl or width answers, through the
constructs and value routes the claim lists. What follows is what the claim does not cover.

#### PH-A1. Operator-typed receivers with impls (cluster h): CLOSED in amendment 121; the residual
Sources: triage 21, 44, 51; verdict "Operator values (the PICK mark)"; amendments 117 item 3 and 121. Sev BLOCKER-class. The exemption for a
receiver of an operator-defined type is kept for a value the operator named, and refused for a value sealed code PICKED (a candidate-keyed
table, a branch, an index, a sealed generic or closure that hands one back, a comparator's order or survivors), by a third taint bit, `PICK`, that a
value carries only while it is or holds an operator-typed value: field data from sealed code never raises it, so the honest `Sq { s: val() }.area()` stays
accepted (taint_tests `an_operator_value_the_candidate_picked_is_never_dispatched_on`; rows M3460-M3478). What is left (open, small): a pick through a path
the edges above do not name (a `select` arm, a `Chan` receive by a candidate-chosen channel, an operator struct stored in a dict by a candidate-written key and
read by an operator literal); an over-refusal (`let r = if cand_ok() { A {..} } else { A {..} }; r.rub()` is refused although both arms are one type);
identity proper still needs PH-A19.
Closing test for the residual: each unnamed edge gets an attack and the honest control keeps passing.

#### PH-A2. Omission (a skipped store, a withheld send, an absent callback)
Sources: r11 PSV-1 MAJOR-ADJACENT, amendment 106 item 7, triage 15/31/32/34/45 ("what a FIX does not close"). Sev MAJOR-ADJACENT (stated residual).
A candidate that does NOT call an operator callback, withholds a `send`, makes a `?`, a handler arm or a scheduler fiber skip an operator
store, selects what the operator's branch does on absence. Taint marks what happens AFTER an exit and what is written, not what is
not written. Not closable by value taint. Candidate directions: an absence-sensitive sink (an operator read of a store that a sealed
frame could have skipped is tainted by "may have been skipped": a per-store dirty-by-omission bit set when any sealed frame ran an
exit-capable path before the store), or the process split of PH-A19, where absence is an explicit RPC result.
Closing test: the five exit-shaped attacks refused where the operator reads the un-written store.

#### PH-A3. Tables of precomputed operator verdicts indexed by a candidate value
Sources: r11 PSV-1 MAJOR-ADJACENT, r12 PSV-1 ("not closable by value taint"), amendment 106 item 7. Sev MAJOR-ADJACENT (stated residual).
A table of closures indexed by a candidate value is refused; a table of verdicts (bools, numbers) indexed the same way is not, because a verdict is data.
This is the honest-suite rule "never let candidate data choose between a strict and a lenient check". Mechanical form: a lint
(`axon check --suite`) that flags an index/key expression of candidate origin into an operator-built collection. Closing test: the lint
on the executed repro, no false positive on `dict_get(expected, name)` keyed by an operator literal.

#### PH-A4. A branch on candidate data into a weaker check; short-circuit control flow
Sources: verdict "NOT claimed" (first sentence), amendment 106 item 7, r13 PSV-1 MINOR ("add control-flow-by-short-circuit to what is refused or admit it").
Inherent: the suite compares the candidate's output with an expected value, so output influences the verdict. Residual is a rule for
suite authors (verdict "What an honest suite must do"). Listed so the next claim review checks that `&&`/`||`/`?` are in the stated constructs (they are, in the verdict at 117).

#### PH-A5. Integer handles and authority values
Sources: amendments 100 and 102 ("Not examined"), 106 (1), 108 (stays outside), verdict. Sev FUTURE.
Sandbox, principal, goal, fiber, supervisor ids a sealed value can choose as keys of operator kernel state; authority VALUES (an effect list, a budget)
given to `sandbox_create*`. The taint reaches the argument; no sink refuses it. A handle sink was weighed and NOT done: every handle an
operator mints after a candidate-driven goal run carries the kernel sticky taint, so it would refuse honest suites. Direction: handles
become opaque typed values minted per frame (a sealed frame's `sandbox_run(h)` with a handle it did not mint is refused by construction),
which is PH-A19 again. Closing test: `sandbox_run(sb_chosen_by_candidate, ..)` refused; the honest mint-then-run passes after a goal run.

#### PH-A6. Paths, URLs and `ai_complete` prompts a tainted value supplies
Sources: amendments 100, 102 (2), 106, 108; verdict. Sev FUTURE.
The taint flows into the RESULT through World taint; nothing refuses the ARGUMENT. A candidate-chosen path/URL/prompt lets the candidate steer which file or
endpoint the operator reads. Candidate sink rule: a World-read builtin whose path/URL/prompt argument has VAL taint is refused unless the
argument is listed as candidate-addressable. Cost: honest suites that read a fixture named by the candidate. Closing test: attack per builtin
family (file, dir, http, exec argv, prompt) + the honest literal-path control.

#### PH-A7. Native codegen and native registries
Sources: amendments 102 (3), 106, 108, 112 ("Not examined: `native::` modules other than `gfx`"), r10-r12 SENTINEL FUTURE (gfx/axon-domain registries per-Interp, not per-kernel). Sev FUTURE.
(a) `axon build` binaries have no taint at all (and codegen refuses what it cannot enforce); a protected suite must never be built natively: assert
that in the runner (a test that the runner refuses a native suite exists only as prose). (b) Amendment 108 made the registry call site world state;
nothing is known per native module (`modbus`, `fhir`, `fix`, any later one). Closing test: a drift test that enumerates `native::` modules and requires a
taint class per module, the same shape as `every_builtin_has_a_taint_class`.

#### PH-A8. Hidden-state classes not yet classed World or Kernel; containers of state
Sources: verdict ("an operator-built class holding hidden state that is not yet classed"), amendment 108 table (`Uncertain`/`Temporal` holding a dict,
session-like state, kernel/world beyond the coarse classes: NOT EXAMINED), amendment 106 (`to_str` of a container not driven by a test), triage cluster e. Sev MAJOR-ADJACENT.
Round 14 found five members (clock via Pure builtin, disk-backed dstore classed Kernel, global token RNG, zoned-call provenance push and append, goal-loop RNG)
by running attacks, not by enumeration. The remainder is open by construction (see PH-A19). Closing test for the interim: the state-class registry of PH-A19 (every
mutable field and static of `Interp`, `Kernel` and the runtime crates has a declared class) with a drift test that fails on an unclassified one.

#### PH-A9. Goal-loop draws and metrics
Sources: triage 47, 48 (fixed for the executed shapes), amendment 117 item 4. Sev MAJOR-ADJACENT (unexecuted instances).
A goal search READS the world cell the operator's `srand` wrote (M3325/M3326) and runs operator metrics under `t_loop_pc`. The `ai_cost_spent` and virtual-clock
instances of finding 48 share the mechanism and were NOT executed against the fixed tree (only the file instance was). The RNG-coupling WRITE edge is an equivalent
(removed). Closing test: execute the `ai_cost_spent` and `AXON_CLOCK` variants against the fixed tree, one row each, killed by own attack.

#### PH-A10. Emitters readable through /proc
Source: triage 30, verdict "Emitters". Sev MINOR (narrowed).
`print`/`println`/`eprint`/`eprintln` are `Pure`; with stdout a regular file, `read_file("/proc/self/fd/1")` observes sealed output untainted. The protected runner gives pipes.
Close by classing emitters World when the fd is a regular file, or by making the runner assert pipe stdio (a runner test, not a taint change). Closing test: the runner refuses a regular-file stdout.

#### PH-A11. Existence oracle: module loader text for an operator module FILE
Source: triage 6. Sev MAJOR-ADJACENT (narrowed). A sealed `mod X`/`use X` that reaches an operator module file gets different text from a missing module (the loader names the operator path).
Fix is in the loader's error taxonomy: one text for "not found" and "found but not readable from a sealed frame".

#### PH-A12. Existence oracle: candidate global initializer under the merged check (CLOSED in amendment 121)
Source: triage 7. Sev BLOCKER-class. CLOSED: a sealed module-level `let` initializer is walked by the sealed-only checker (`check_sealed_predicate`), and located at the item; twin rows `top-level let: ...` at every placement (M3453, M3458). Kept as a heading for traceability. Was: A candidate's module-level `let` initializer is judged by the merged check, so did-you-mean text and struct-literal acceptance
differ for an operator name. Fix: judge sealed global initializers by the sealed-only check first (as fn bodies are), and drop did-you-mean from sealed diagnostics.

#### PH-A13. Existence oracle: sealed refinement ITEM predicate with no span (CLOSED in amendment 121)
Source: triage 33. Sev MAJOR-ADJACENT. CLOSED: the parser's end-of-input span is no longer the dummy `0..0`, the resolver locates a predicate at its item, and a merged diagnostic that names no line joins the backstop (M3450-M3454, M3456-M3457). Was: Predicate nodes of a sealed refinement item carry no span, so the merged diagnostic is attributed to the operator's file and survives the split. Give the parser
spans for refinement-item predicates; closing test the unresolved-fn predicate attack.

#### PH-A14. Existence oracle: bindings named like an operator global
Source: triage 23. Sev MAJOR-ADJACENT (narrowed). The merged E0004 backstop's name walk does not model shadowing, so a local, parameter, `for` variable or pattern binding named like an operator
global is refused where a fresh name is accepted. Fix: make the backstop scope-aware or run only the sealed-only check on sealed files.

#### PH-A15. Existence oracle: a name the SUITE references but does not define
Source: triage 22. Sev MAJOR-ADJACENT (narrowed). `AXON_PATH=suite:candidate`: a module, fn, type or constant the suite names but its tree does not define resolves from the candidate directory.
Honest rule today: ship everything the suite names. Mechanical fix: the runner resolves suite modules with `AXON_PATH_EXCLUSIVE` over the suite tree alone and the candidate only through the sealed path.

#### PH-A16. Existence oracle: a candidate that DEFINES an operator name; native module and effect names
Sources: amendment 112 item 3 (residual, FUTURE SENTINEL item 2), r13 SENTINEL FUTURE (trait-impl duplicate check), amendment 112 ("Not examined": native module names, the effect handler's effect name). Sev MINOR.
Duplicate definition is refused E0002 where a fresh name is accepted: readable from source, cannot shadow anything, needs one run per guessed name; class "names are readable from suite source under an IO grant" (accepted).
Not examined: native module names and an unresolved named effect handler.

#### PH-A17. The runner's existence-oracle test lists positions, not the language
Sources: r10/r11 SENTINEL MINOR ("closed on five paths; a dozen others"), amendment 117 (f) (24 more positions), r13 SENTINEL (the runner twin for the static path is vacuous). Sev MINOR.
Each round closed the listed positions and the next found another. Direction: generate the positions from the grammar (every place `Expr`/`Item`/`Type`/`Pattern` carries a user-written name) and run the
operator-name/missing-name pair for each: a differential test, so a new construct without a pair fails. Closing test: the generated sweep, and a planted unpaired construct refused.

#### PH-A18. Honest-program cost (over-refusal)
Sources: r9 PSV-1 MINOR (dispatch inside `with handler` bodies and arms always refused), r9 SENTINEL MINOR (a candidate's own first-class fn values called with arguments refused as "operator closure" calls),
amendments 102 and 106 cost lists, r11 PSV-3 ("stated cost 2-5% understates its measurement"). Sev MINOR (fail-closed).
Fail-closed false refusals tax honest suite authors and push them toward lenient suites. Track as a measured list, not as proof of soundness. Closing test: each over-refusal becomes an HONEST control that passes once a precise rule exists.

#### PH-A19. RECOMMENDED DIRECTION: replace case-by-case hooks with a coverage-complete design
Sources: the six-pass loop (findings per pass 14, 15, 8, 6, 4, 6; never two consecutive clean passes), amendments 102, 106, 108, 117. Sev architectural.

*The evidence.* Amendments 100, 102, 106, 108, 114 and 117 each closed a "class" and the next review found another member of it. Round 14 made the pattern
explicit: of 53 findings, 44 were closed by a local edit (a missing `t_loop_pc`, a Pure builtin that read the clock, a `Tys::closed` that stopped at a refinement) and each edit is a point fix on a
hook list (`CONTROL_TABLE`, `CALLBACK_BUILTINS`, `PURE_BUILTINS`, `WORLD_BUILTINS`, the sealed-only check's positions). The drift tests hold those lists complete against `Expr`
and `BUILTINS`, but not against the interpreter's STATE or against the Rust loops that call back into operator code; both gaps produced round-14 blockers.

*Option 1: keep hooks, add sweeps.* Cheapest per finding; each finding costs a row, a test and an amendment. A pass of nine reviewers still finds 4-6 new members. No termination argument.

*Option 2: taint-by-default with an allow-list.* Every mutable piece of interpreter state (fields of `Interp`/`Kernel`, statics, thread-locals, registries, process-global clock/RNG, files the runtime opens) must be declared once in
a state registry with a class (`Pure`, `Kernel`, `World`, `PerFrame`); state not in the registry cannot compile (a `Cells` wrapper whose only constructor takes the class; a drift test over `static`, `thread_local!`, `Cell`/`RefCell`/`Mutex` fields in `interp*`, `kernel.rs`, `native.rs`, `clock.rs`). On the control side, the single dispatch
point for "operator code runs from Rust" (`call_cb`, goal loops, scheduler pass, handlers) is the only function that can call `call_fn`, and it takes the pc taint as a required argument. This turns "a hook someone forgot" into a compile error and
closes PH-A8/PH-A9 by construction. It does not close identity (PH-A1), handles (PH-A5), paths (PH-A6), omission (PH-A2) or native (PH-A7). Effort M-L; keeps the single-interpreter model; honest-suite cost unchanged.

*Option 3: split the operator interpreter from sealed code (capability style).* The candidate runs in its own `Interp`/`Kernel`/registries (a separate instance, then a separate process under the PSV-2 sealing it already has). The operator reaches it
only through `call(name, args) -> Data`, where `Data` is an inert, serialisable value tagged tainted as a whole, and operator closures reach the candidate only as re-entrant callbacks. There is no shared state to class: the clock, RNG, dstore, provenance, token
stream, sandbox table and native registries are per instance by construction. Candidate-minted handles, operator-typed values with impls and closures cannot cross as anything but data, which closes PH-A1, PH-A5 and PH-A7(b) outright and shrinks PH-A2 to "what the
RPC returned on absence". Residual: the output-influences-verdict rule (PH-A4), paths/URLs/prompts (PH-A6), the OS (files remain shared; the PSV sandbox bounds them). Cost: a marshalling layer and re-entrancy, closures passed to candidate fns (the PSV-3 text already warns
that assertions in a closure handed to the candidate run only if the candidate calls it), a slower call path, a rewrite of the sealing tests. Effort L.

**Recommendation.** Stop adding point hooks after C9: take Option 2 immediately as the bridge (it is the cheapest change that makes the largest remaining class, hidden state and loops that run operator code, unrepresentable and gives the next review a finite surface), and plan Option 3 as the successor design because it is the only one with a termination argument for
identity, handles and native registries. Do not claim more than the verdict claims until Option 2's registry drift test is green.

## Part B. The refusal-site gate

Gate: `scripts/v022_refusal_coverage.py`; tests `refusal_coverage_gate` (axon-core). Counts below are from `--remainder` at this branch's base (integrate13 + the docs merge):
REMAINDER 207 (208 at `b974655d`: amendment 121 rowed the observer prune, M3480-M3482), OBSERVED-NOT-ROWED 252, DEFAULTS NOT FOLLOWED 113, VALUE FLOWS NOT FOLLOWED 45 (+1 cross-file const). The claim is true only relative to gate-visible forms; these are the invisible ones.

#### PH-B1. STILL BLIND: values built by computation
Source: gate STILL BLIND line 1; amendment 107; r10 EQUIVALENCE MAJOR-ADJACENT (REMAINDER is a lower bound); r12 (46 -> 23 double count corrected). 45 sink arguments computed (a `format!` of variables, a path joined at run time, a value through two locals or a parameter).
Direction: an expression-level data-flow in the scanner (syn) instead of regex. Closing test: a planted computed argument at a sink is a site.

#### PH-B2. STILL BLIND: spawn through a wrapper not in the tables
Source: STILL BLIND line 2; r11 EQUIVALENCE (the root launch path: argv, PATH and constants handed to `sealed_exec::command` are not sites; executed edits survive the whole axon-fabric suite; partly closed by amendment 107). Wrapper callers' literals are followed only at the inner call; script/file bytes handed to a child are not seen. Closing test: a new wrapper fails the gate in both directions, as `EXEC_CONSTRUCTORS` does.

#### PH-B3. STILL BLIND: struct literals by type name, nested literals
Source: STILL BLIND line 3; r11 EQUIVALENCE MINOR (a `Profile` literal field flip survives). Only Config/Cfg/Authority/Policy/Manifest/Trust (and any field named `owner`) are sites. Direction: any struct literal in a protected crate with a bool/enum/uid field that a refusal reads.

#### PH-B4. STILL BLIND: defaults outside the protected crates, computed defaults, the type behind `unwrap_or_default()`
Source: STILL BLIND line 4; amendment 110; r12 EQUIVALENCE MAJOR-ADJACENT (fail-open flips on dev-vs-protected and evidence-class discriminators survived), r13 (113 sited / 113 computed are two independent counts). 113 computed defaults not followed; axon-core, axon-os, axon-cortex, axon-vm are scanned for refusals, not defaults.
Closing test: per-default fail-open flip on every `val_default` of the REMAINDER (PH-B8) killed or exempted with a checkable reason.

#### PH-B5. STILL BLIND: absent-value decisions spelled other than the listed combinators
Source: STILL BLIND line 5; amendment 115 (round 13 found two such spellings "there may be a third"). `if let None/Err(_) = x { .. }` blocks, block-bodied match arms, a `filter` that turns Some into None, `ok_or(..)` supplying a default, a `_ =>` arm, arms bound to a name, the same decision via a helper fn. A site's row covers THAT edit, not every sub-predicate.
Direction: classify by effect (a branch that changes accept/refuse on absence), not by spelling.

#### PH-B6. STILL BLIND: signing/verification/MAC inputs through parameters or fns not in the sink tables
Source: STILL BLIND line 6; amendment 110 (15 signing inputs rowed). `SIGN_SINKS`/`SIGN_BUILDERS` are not checked against every `.sign(` of the scope in both directions; a context spelled in a format string by a variable; peer implementations outside the repository. Closing test: a new `.sign(` not in the tables fails the gate. Cross-ref: post-c9-keys-and-signatures.md I, J.

#### PH-B7. STILL BLIND: uid or mode as operand of a comparison; cross-file consts; non-Option<u32> parameters
Source: STILL BLIND lines 7 and 8; r11 EQUIVALENCE MINOR (constants that are decisions). A literal compared inside a refusal; a const used at a sink in another file written bare through `use`; a `u32` uid or `&str` flag passed down is followed only at the inner sink.

#### PH-B8. REMAINDER by category (207 guard sites no test observes alone)
Source: `--remainder`, amendments 98, 103, 110, 115. Counted, not claimed covered. Counts now: `val_default` 70, `const_tag` 36, `okor_unjudged` 16, `okor_nodefault` 12, `const_path` 11, `py_guard` 11, `const_bound` 8, `other` 8, `const_table` 7, `const_text` 7, `okor_closed` 4, `const_exit` 3, `const_other` 3, `val_comb` 2, `val_field` 3, `okor_offroute` 2, `val_stdio` 2, `unlink_job` 1, `val_arm` 1 (sum 208).
Disposition per category (propose, then execute one by one): `val_default` (70, highest risk: fail-open on protected routes, "survey-only evidence", r13 EQUIVALENCE MINOR) get a flip test each; `const_tag`/`const_path`/`const_text`/`const_table`/`const_bound`/`const_exit`/`const_other` (75) are constants that are decisions: pin the value in a test that reads the production constant (not a copy);
`okor_*` (34) see PH-B9; `py_guard` PH-B10; `other`/`val_comb`/`val_field`/`val_stdio`/`val_arm`/`unlink_job` (17) individually. Closing test: REMAINDER count falls with every row, the gate's printed count equals this table (the checker does not require it; the next review does).

#### PH-B9. `ok_or` / `ok_or_else` / `map_err` refusals; the okor_offroute label
Sources: r8 EQUIVALENCE MAJOR-ADJACENT (~150 single-line `ok_or(..)?` refusals are not sites), r9, r10/r11 (the `okor_offroute` label was false for protected-route code: `parse_check_suite_ref`, `CheckRegistry::load`; "3 of 6 sites remain" at r11; not re-verified after amendment 115).
Counts now: `okor_unjudged` 16, `okor_nodefault` 12, `okor_closed` 4, `okor_offroute` 2. Direction: judge `okor_unjudged` by a flip test; re-derive the offroute label from the call graph from the protected entry points, not from a hand list.

#### PH-B10. Python guards (`scripts/guest_build_env.py`) and other non-Rust decisions
Sources: amendments 98 (11 `py_guard`), 99, 111, r9 EQUIVALENCE MAJOR-ADJACENT (build environment outside the gate), STILL BLIND line 10. 65 covered + 77 exempt Python guards; 11 REMAINDER (`guest_build_env.py` 1129, 1133, 1265, ...). Python other than that file, and the shell scripts (`trust_root_preflight.sh`, `ns_run`, the kit) are not scanned at all. Rows for `peel`, a literal `eval` program and the `alias` keyword were written and DROPPED for want of ids (amendment 111 (c)); their shapes remain in `--selftest`, unrowed.

#### PH-B11. Constants that are decisions (limits, ages, flags)
Sources: r11 EQUIVALENCE MINOR (one survives), amendment 103 ("survey of the fabric constants stopped at `MAX_REASONS`"), `const_*` rows in PH-B8. Timeouts and liveness bounds are outside the rule by decision (r8, r9: "stated, unchanged"). Closing test: each decision constant observed at its boundary value, liveness bounds named in an exemption with a bound test.

#### PH-B12. The re-survey record and OBSERVED entries are self-attested
Sources: STILL BLIND line 9; r12 EQUIVALENCE MAJOR-ADJACENT (salt not bound to head, a record with nothing re-measured passes), r13 EQUIVALENCE MINOR (never-run record with self-written logs, KILLED with exit code 0); amendments 110 and 115 (derived, floored, logged; logs must look like logs). Residual stated by 110/115: "a whole log can still be FABRICATED by the person running the freeze".
Also 252 OBSERVED-NOT-ROWED entries are a measurement, not a row (r11 EQUIVALENCE MINOR, a fourth disposition nothing re-verifies), and a row that deletes a redundant pair (`git_cmd`'s `GIT_NO_LAZY_FETCH` + `protocol.allow`) credits each member though only the pair is shown observed (r10 EQUIVALENCE FUTURE).
Direction: the re-survey runs in the freeze harness itself, producing the log, not the freezer.

#### PH-B13. The guest runner's and fabric's unrowed defaults on protected routes (`val_default` hot list)
Sources: r12/r13 EQUIVALENCE, amendment 115 item list, REMAINDER entries in `axon-fabric/src/{backend,readiness,custodian,observer_service,privileged_launcher,protected_host}.rs`, `axon-loop/src/store.rs` (11 consecutive `val_default` sites at 601-633), `axon-guest-init/src/main.rs`, `axon-psv/src/{runner,lib}.rs`. Prioritised first because a fail-open default on a protected route is the class that survived the whole suite in rounds 12 and 13.

## Part C. Build environment and `ns_run`

Files: `scripts/guest_build_env.py`, `scripts/opkit_ns_isolate*`, `scripts/opkit_ns_drift.py`, `scripts/test_opkit_ns.sh`, `scripts/trust_root_preflight.sh`. Amendments 90, 92, 97, 101, 105, 109, 111, 113.

#### PH-C1. A forged `OPKIT_HOST_NS` from the caller's own PID namespace
Source: amendment 113 ("What this does NOT stop"), amendment 116 (b). A caller that is PID 1 of a private PID namespace and forges `OPKIT_HOST_NS` passes the host-mount-namespace precondition; the test `init-forged` records it as a control. It has root in a namespace it made: the check stops a mistake, not an adversary. Direction: derive the host namespace id from a root-owned file or the launcher, not the environment.

#### PH-C2. Escape syscalls never executed
Source: r13 FIELD-ORIGIN MINOR (wording: "NOT executed by anyone ... a hostile CAP_SYS_ADMIN step"), amendments 101, 105. The header says a hostile CAP_SYS_ADMIN step is not executed; round 11 measured root with full caps doing `mount -o remount,rw /` and `dd`. Closing test: an executed escape per syscall class (mount, pivot_root, setns, open_by_handle_at, ptrace, unshare) inside the stand-in, each refused.

#### PH-C3. Capability-drop evidence executed once, not independently reproduced
Source: r13 FIELD-ORIGIN MINOR (setuid-root copy, file-capability copy, setpriv re-adds, ambient raise). One execution is evidence of an outcome, not of a property. Closing test: the five probes as a committed script with a recorded run on two hosts (the local one and a second).

#### PH-C4. The drift gate is a text matcher
Source: amendments 101, 105, 109, 111 ("remains a text matcher; its shapes are the ones known"), r10-r13 FIELD-ORIGIN (nine, ten, thirteen further shapes found in successive rounds; `sed -i`, `tar -x -C`, `rsync` into a real destination pass). A deny-by-mention gate accepts what it does not recognise.
Direction: do not extend the matcher. Make the wrapper the only way to run the kit (the launcher execs the kit under `ns_run` itself and the kit refuses to run when its mount namespace is the host's), then the matcher is a lint, not a boundary.

#### PH-C5. UTF-16/32 service configs outside the NUL rule
Source: amendment 113 unfinished (c). The decoder accepts UTF-16/32 only when the result is >= 90% ASCII printable; non-ASCII-heavy UTF-16 and files over the 64 KiB read cap are not covered (the cap refuses the second). Closing test: a UTF-16 config with identities in non-ASCII text is refused, not read as noise.

#### PH-C6. `service_ids` shapes
Sources: r10 FIELD-ORIGIN MINOR (fails open for realistic shapes; uid only, not gid or running processes), r11, r12, r13 (a YAML `- uid: N` item, fixed by amendment 113), amendment 109/111 (b) (no text-key row for the unclassified-key refusal; no row for the root check at the top of `ns_run`: indistinguishable from `unshare`'s own refusal). Shapes outside the deployment (nested JSON, TOML tables, systemd drop-ins with `User=` templated, `DynamicUser`) are unparsed; a file that mentions an identity key but yields nothing has a guard for text, not for JSON. Closing test: a "mentions but yields nothing" refusal for every parser.

#### PH-C7. The preflight open-write probe runs on real trust files
Source: `scripts/trust_root_preflight.sh` (the `open-write` probe, `exec 3<>"$1"` as each service user, around line 281). The probe opens the REAL trust files read-write, without writing a byte, to ask the kernel for the write-permission verdict. `<>` does not truncate, but it is an open of a protected file by a possibly-misconfigured identity, it can take locks and trigger
inotify watchers, and in a mistake it succeeds against a live file. Direction: probe with `access(W_OK)`/`faccessat` as that identity, or on a root-owned bind-mounted copy with identical mode/owner/ACL. Closing test: the preflight runs against a read-only snapshot and still refuses the planted writable file.

#### PH-C8. The kit writes through operator-supplied descriptors and environment
Sources: amendments 101, 105, 109, 113; r10-r13 FIELD-ORIGIN (fd 0-2 allowlist admits host console, VT, serial devices, sockets and pipes; `OPKIT_RW` trusts `$TMPDIR` and a system-path list; `OPKIT_SCRATCH` unvalidated; non-directory inherited descriptors reach the host); the 2026-10-09 incident recorded in amendment 116 item 9 (see PH-E3). fds above 2 are refused/closed since 101; 0-2 remain the caller's. Direction: `ns_run` opens its own 0-2 (null for stdin, a pipe or file inside the scratch for 1-2) and takes output via a named path inside the scratch. Closing test: a writable regular file outside the scratch on fd 1/2 is refused with the command not run (the helper already refuses; the missing half is that no TEST may hand one, see PH-E3).

#### PH-C9. The read-only root is a mount flag, not a boundary
Source: r11 FIELD-ORIGIN MAJOR-ADJACENT, amendment 105. `/dev` devices and a hidden host `/proc` stay reachable, so "a write by ANY verb to a place nobody listed fails with EROFS" is overstated (amended in 105; the stand-in still shares device nodes). Direction: a minimal /dev (tmpfs with null/zero/random/urandom only), a fresh /proc for the PID namespace.

#### PH-C10. Build-uid dedication, reaper and begin() trees
Sources: r9 FIELD-ORIGIN MAJOR-ADJACENT ("dedicated uid" is a docstring; the reaper skipped State=Z and thread-only processes; amendment 97 answered), r9/r10 MINOR (`begin()` leaves three trees build-uid-owned and outside the per-uid lock; base paths leak through `/proc` cmdlines; the lock directory residual). Not re-verified as closed beyond amendment 97 and 111.

#### PH-C11. The kit judges the guest manifest and artifacts in place, then installs later
Source: r8 FIELD-ORIGIN MAJOR-ADJACENT (carried from round 7, "not re-verified as fixed"). A time-of-check/time-of-use gap on builder-owned files. Direction: copy to a root-owned staging path, judge the copy, install the copy. Closing test: replace the file between judge and install; the install refuses.

#### PH-C12. Host-side kit and status records
Sources: r8 PSV-7 (the kit's machine-id compare degenerates to `None == None` when the record and the host both lack a machine-id; Stage 7 `b263_qualification` is a bare sha pin in a mutable status file; `axon-custodian --dev` is not gated by `TEST_TRUST_BUILD` in production builds, FUTURE), r8 PSV-5 (the execution leg of a protected trial is shape-checked only; manifest `backend_profile` not joined to receipt `backend_profile_ref`; `limits`/`matched_checks` not joined loop-side), r8 PSV-4 (`derive` and `interpret_linux_result` read `result.json` in two independent reads). Mostly not re-verified. Host binding of the B263 record is `post-c9-keys-and-signatures.md` N (not duplicated).

#### PH-C13. Pointers to companion items
`post-c9-keys-and-signatures.md` I-K (typed keys, container, root endorsement), L (LLVM 21; PH-A7 depends on native being excluded until then), N (host binding). Amendment 73 (`harden()` inherited-state resets) is parked, not in any open spec file: when it is brought back it needs this document's PH-C8 (descriptors and environment) as input.

## Part D. Operational and test hygiene

#### PH-D1. Shared-uid test design
Sources: amendment 111 (`uid_claim` moved to one module), A259 in `v022-psv-negative-matrix.md` (the build-uid lock test failed whenever ANY process of uid 4242 existed on the host), r13 FIELD-ORIGIN FUTURE (a hostile uid-owning process cannot turn a failing test into a pass, only cause failure or exhaustion; "every test owns a distinct uid" is per thread, not per test binary).
Direction: allocate test uids from a range reserved in a lock file per host with a cross-process claim; a test fails with a clear message when a foreign process owns the claimed uid instead of flaking.

#### PH-D2. Fabric tests that hang under load
Sources: amendments 102 (item ~6359), 110 (~7143), 112 (7650): `a_callers_scheduling_state_never_reaches_the_root_launch` (its child is `SCHED_IDLE`, pinned) hangs on a loaded host and passes in 1 s idle; skipped on gpumaster. `privileged_launcher` hung 70 minutes on gpumaster (amendment 112). Closing test: a bounded timeout with a diagnostic, and a CPU-quota-independent child (`SCHED_OTHER` with a nice value, or a deterministic stub).

#### PH-D3. Namespace and root-helper tests are not calibrated on gpumaster
Sources: `/var/tmp/c9r4-rules/common.md` (calibration list), amendments 109, 111, 112, 114 (`readiness::a_narrowing_list_the_verifier_cannot_stat_is_not_read_as_absent` needs `unshare`/`setpriv`), 116. Calibration (2026-10-05) covered the cargo suites; not the guest image build/boot, root-helper/namespace tests or anything needing /etc. Direction: either calibrate (run the identical-count listing for those tests on both hosts) or tag them `needs_ns` with a documented local-only evidence rule. The `IDENTICAL` listing today is of two hosts only (amendment 116 (e)).

#### PH-D4. M3038 is WITHDRAWN, not proven
Source: amendment 116 item 4 and Unfinished (a). The row (the checker judges a sealed call against the sealed impls' methods) was retired on four cells in 114; the full-suite condition could not be shown (the `axon-ledger` clean-tree baseline failed because `/etc/passwd` on the host was damaged, `attribution_integrity`; gpumaster's `axon-fabric` suite hangs, PH-D2). The guard stays in `checker.rs`, unrowed.
To bring it back: either (a) a Rust test that runs `checker::check` over a merged program with an operator impl and a sealed call, so the edit is killed by its own attack without the sealed-only check shadowing it; or (b) a full-suite run on a host whose `/etc/passwd` is whole and whose `axon-fabric` suite completes under the harness. Then reinstate the row, marker, `EQUIV_RECORD`, `EQUIVALENT_DID`/`RETIRED` membership and `GUARD_SETS` entry.

#### PH-D5. Withdrawn amendment-100/102 runner rows M2603, M2604, M2605, M2607
Sources: amendment 102 (withdrawn), r11 EQUIVALENCE MINOR, r11 PSV-3 MINOR. The unit twins are honest, but the runner-leg kill is observable only in a configuration the shipped binary never runs (`TAINT_FORCE_ON=false` default in unit tests: a static-only configuration; the production pair is static + taint). A four-cell record is impossible because the whole-package-suite cell cannot be green. Direction: run the unit suite under both configurations in the harness (D6) and re-issue the rows with the production pair.

#### PH-D6. Unit-test default `TAINT_FORCE_ON=false`
Source: r11 PSV-3 MINOR. The gate's am96/am100 unit rows judge static layer only; the production pair is covered by runner legs only. Closing test: the harness runs the taint unit suite with the force flag in both settings and reports both.

#### PH-D7. Stale status records made at freeze
Sources: amendments 112 (a), 116 (b). `governance/status/v022-resurvey.json` is for an earlier gate digest; `governance/status/v022-psv-paired-disable.json` still carries M186's obsolete record and every record is stale under the currency rule (59 of 59). Both are made at the freeze head, last; the freeze manifest cannot be produced from a tree without them. Closing test: the freeze command refuses a stale record by name, and the freeze runbook lists the order (gate digest, resurvey, paired-disable, manifest).

#### PH-D8. The 59 paired-disable records
Source: amendment 112 (a). 59 paired-disable records exist (`governance/status/v022-psv-paired-disable.json`); all 59 are stale under the currency rule and must be remade at the freeze head against the final gate digest; the four-cell form is in `/var/tmp/c9r4-rules/common.md` (base refused, retired guard off refused by its sibling, sibling off refused by the retired guard, both off the attack succeeds). Cost is large (full suites per row); batch them on gpumaster slots and record host identity. See also PH-D5.

#### PH-D9. Mutation runs and suites at different commits
Source: amendment 116 Unfinished (c). The mutation runs ran at `102a1090` and the suites at the commit in the table; the commits between change only governance text. The freeze requires them at the SAME commit: re-run at the freeze head.

#### PH-D10. PSV-6 timing boundaries and remaining PSV-3 doc minors
Sources: r8 PSV-6 MINOR (no test observes the 300 ms mid-spawn wait or the prune/expiry boundary second), r8-r13 PSV-3 MINOR items (delta-note staleness, gate rows weaker than unit rows, claim sentence counts). Most PSV-3 minors were closed by amendments 112 and 114 (the note's gate table is compared on label, package and result); the list is not individually re-verified here. Closing test for PSV-6: a boundary test at expiry-1s, expiry, expiry+1s using the virtual clock.

## Part E. Process lessons

#### PH-E1. Why rounds did not converge
Evidence: rounds 8-13 produced 126 reported items (12 BLOCKER, 32 MAJOR-ADJACENT, 70 MINOR, 12 FUTURE by the reviewers' own labels); the round-14 loop confirmed 53 more in six passes (14, 15, 8, 6, 4, 6).
(1) Each fix closed an INSTANCE and was written up as a CLASS; the next reviewer found the sibling. (2) The closure lists (constructs, builtins, positions) were enumerations; the state and Rust-loop surfaces had no list. (3) Review lenses overlapped: a MAJOR-ADJACENT in one gate was often a BLOCKER of another's claim. (4) Claim wording followed evidence only after a review caught it (amendments 98, 106, 112, 117).
(5) The same people writing fix, row, and claim made the self-attested parts (re-survey) the weakest.

#### PH-E2. The find-until-dry loop design (round 14)
Six passes, 154 agents, a finding confirmed only with an executed repro against a prebuilt binary, deduplicated by signature, triaged FIX or NARROW-CLAIM by replay on the fixed tree. What worked: executed repros ended arguments; the signature gave a stable id; the replay triage prevented unsound fixes (finding 21: the obvious fix measurably refused an honest program). What did not: no stop rule beyond "two clean passes" which was never reached; findings per pass were not independent (agents saw the fixed hooks). Guidance: fix the loop's stop rule in advance (pass count and a residual budget), keep passes blind to each other, and score the claim by what the loop did NOT reach.

#### PH-E3. The /etc/passwd incident of 2026-10-09 and the safety rule for probes
Source: amendment 116 item 9. The first five lines of `/etc/passwd` on the review host were overwritten at 2026-10-09 01:14:46 -0400 by the text of `ns_run`'s own refusal (`LEAK: descriptor 2 is a WRITABLE regular file (/etc/passwd) ...`), because a test or probe handed the helper stderr opened read-write on the real file. `/etc/passwd-` holds the original; gpumaster is intact. The helper behaved correctly; the caller was the defect. Not repaired (never write under /etc); operator action: compare `/etc/passwd` with `/etc/passwd-` and restore.
**Rule for probes.** A probe or test of anything that refuses to write must never be given a real host file as an output descriptor, even to see the refusal. Use a regular file inside its own scratch (`scripts/test_opkit_ns.sh` does), or a pipe. A harness that runs a destructive-primitive probe wraps it so that fd 0-2 are rebound to the scratch before it starts, and runs it inside a private mount namespace. Add this to `/var/tmp/c9r4-rules/common.md`'s successor in the repo (it is scratch today). Cross-ref PH-C8.

#### PH-E4. Guidance for the next review cycle
Lenses: keep the nine claim reviewers, but give each a falsifiable claim sentence and ask for executed repros only. Verification: the reviewer's repro is re-run by a second agent on the fixed tree. Class-level sweeps with drift tests: a fix to an instance must come with a test that enumerates the class from its source of truth (the grammar, `BUILTINS`, the state registry of PH-A19, `Command::new`, `.sign(`), so the next finding is a drift-test failure rather than a review finding. Promote only after zero blockers in two consecutive passes with fresh agents. Never merge a reviewer's cleaned scope into the claim (round 12's FIELD-ORIGIN review was stopped by a safety check; its list of unrun probes is PH-C2).
Hygiene: judge by exit codes, never head-truncated output; record host identity; do not weaken a bound or a row to pass.

## Part F. Prioritised backlog

Order is risk reduction per effort, highest first. Effort: S under a day, M a few days, L a review cycle or more. "Depends on" names PH ids.

| id | item | source | sev | proposed closing test / row | effort | depends on |
|---|---|---|---|---|---|---|
| PH-E3 | Probe rule: never hand a real host file as an output fd | amendment 116 item 9 | BLOCKER-class (incident) | harness wraps destructive probes: fd 0-2 rebound to scratch; a test that a probe given /etc fd is refused before it runs | S | |
| PH-C7 | Preflight open-write probe on real trust files | trust_root_preflight.sh ~281 | MAJOR-ADJACENT | probe via `faccessat`/snapshot; planted writable file refused | S | |
| PH-B13 | Fail-open `val_default` on protected routes | r12/r13 EQUIVALENCE, REMAINDER | MAJOR-ADJACENT | a flip row per site, killed by own attack | M | |
| PH-A10 | Emitter readable via /proc (runner asserts pipe stdio) | triage 30 | MINOR | runner refuses a regular-file stdout | S | |
| PH-D4 | M3038 withdrawn | amendment 116 item 4 | MAJOR-ADJACENT | Rust test on `checker::check` over merged program, or whole-host full-suite run | S | |
| PH-A9 | Goal-loop `ai_cost_spent` and clock instances unexecuted | triage 48 | MAJOR-ADJACENT | two executed rows | S | |
| PH-D7 | Stale status records at freeze | amendments 112, 116 | MAJOR-ADJACENT | freeze refuses a stale record by name; runbook order | S | |
| PH-A15 | Dangling suite reference resolves from the candidate | triage 22 | MAJOR-ADJACENT | `AXON_PATH_EXCLUSIVE` over the suite tree; attack refused | S | |
| PH-A11 | Module-loader existence text | triage 6 | MAJOR-ADJACENT | one text for operator file / missing | S | |
| PH-A12 | Candidate global initializer under merged check (CLOSED, amendment 121) | triage 7 | BLOCKER-class | done | - | |
| PH-A13 | Refinement item predicate span (CLOSED, amendment 121) | triage 33 | MAJOR-ADJACENT | done | - | |
| PH-A14 | Binding named like an operator global | triage 23 | MAJOR-ADJACENT | scope-aware backstop | M | |
| PH-C11 | Kit judges in place, installs later (TOCTOU) | r8 FIELD-ORIGIN | MAJOR-ADJACENT | swap between judge and install refused | M | |
| PH-C4 | Drift gate is a text matcher; make the wrapper the only way | r10-r13 FIELD-ORIGIN | MAJOR-ADJACENT | kit refuses to run in the host mount namespace; matcher demoted to lint | M | PH-C1 |
| PH-C1 | Forged `OPKIT_HOST_NS` | amendment 113 | MINOR | host id from a root-owned source | S | |
| PH-B4 | Defaults outside protected crates, computed defaults | STILL BLIND 4 | MAJOR-ADJACENT | per-default flip across axon-os/core/cortex/vm | L | PH-B13 |
| PH-B2 | Spawn wrappers, root launch argv/PATH | r11 EQUIVALENCE | MAJOR-ADJACENT | both-direction check of wrappers | M | |
| PH-B6 | Sign/verify sinks not checked both directions | STILL BLIND 6 | MINOR | new `.sign(` not in tables fails | S | |
| PH-A19 | State registry (Option 2), then process split (Option 3) | six-pass loop | architectural | drift test over statics, thread_locals, Cells; then split | L | |
| PH-A8 | Hidden-state classes / containers of state | verdict, amendment 108 | MAJOR-ADJACENT | state registry drift test | M | PH-A19 |
| PH-A6 | Paths, URLs, prompts as tainted arguments | amendments 100-108 | FUTURE | attack per builtin family + honest literal control | M | |
| PH-A5 | Integer handles / authority values | amendments 100-108 | FUTURE | opaque per-frame handles | L | PH-A19 |
| PH-A1 | Operator-typed receiver: the residual after the PICK mark (amendment 121) | triage 21, 44, 51 | BLOCKER-class | an attack per unnamed edge + honest control | M | PH-A19 |
| PH-A2 | Omission | r11, amendment 106 | MAJOR-ADJACENT | absence-sensitive sink | L | PH-A19 |
| PH-A3 | Verdict tables keyed by candidate value | r11, r12 | MAJOR-ADJACENT | suite lint | M | |
| PH-A7 | Native module classes; native build excluded | amendments 102-112 | FUTURE | taint class per `native::` module | M | PH-A19, LLVM item L |
| PH-A17 | Generated existence-oracle sweep | r10-r13 SENTINEL | MINOR | grammar-derived pair sweep | M | |
| PH-A16 | Define-an-operator-name oracle, native names | amendment 112 | MINOR | decision: accept (documented) or unify | S | |
| PH-A18 | Honest-program over-refusals | r9, amendments 102, 106 | MINOR | HONEST control per case | M | |
| PH-A4 | Control by short-circuit, output-selects-verdict | verdict | MINOR | suite lint | S | PH-A3 |
| PH-B1 | Computed values at sinks (45) | STILL BLIND 1 | MAJOR-ADJACENT | syn-based data flow | L | |
| PH-B3 | Struct literals by type name | STILL BLIND 3 | MINOR | any literal with refusal-read fields | M | |
| PH-B5 | Absent-value spellings | STILL BLIND 5 | MAJOR-ADJACENT | classify by effect | L | |
| PH-B7 | uid/mode comparison, cross-file consts | STILL BLIND 7-8 | MINOR | extend scan | M | |
| PH-B8 | REMAINDER by category (207) | `--remainder` | MAJOR-ADJACENT | count falls per row | L | PH-B13 |
| PH-B9 | `ok_or` refusals and offroute label | r8-r11 | MAJOR-ADJACENT | call-graph derived label; flip test | M | |
| PH-B10 | Python/shell guards, dropped rows | amendments 98, 111 | MINOR | rows for peel/eval/alias; scan scripts | M | |
| PH-B11 | Decision constants, liveness bounds | r8, r9, r11 | MINOR | boundary tests | M | |
| PH-B12 | Self-attested re-survey, OBSERVED-NOT-ROWED | r12, r13 | MAJOR-ADJACENT | harness-produced log | M | |
| PH-C2 | Escape syscalls never executed | r13 | MINOR | executed escapes in stand-in | M | |
| PH-C3 | Cap-drop evidence once | r13 | MINOR | committed probe, two hosts | S | |
| PH-C5 | UTF-16 configs | amendment 113 | MINOR | refusal for non-ASCII UTF-16 | S | |
| PH-C6 | `service_ids` shapes | r10-r13 | MINOR | mentions-but-yields-nothing refusal | M | |
| PH-C8 | Kit writes through caller-supplied fds | amendments 101-113 | MAJOR-ADJACENT | `ns_run` owns fd 0-2 | M | PH-E3 |
| PH-C9 | Read-only root is a mount flag | r11 | MAJOR-ADJACENT | minimal /dev, fresh /proc | M | |
| PH-C10 | Build-uid dedication, begin() trees | r9, r10 | MINOR | not re-verified; check | S | |
| PH-C12 | Kit machine-id, stage-7 pin, custodian --dev, receipt joins | r8 PSV-4/5/7 | MINOR | each a refusal row | M | |
| PH-C13 | Companion pointers (keys, signatures, LLVM, host binding, harden) | post-c9-keys-and-signatures.md | n/a | see that file | n/a | |
| PH-D1 | Shared-uid test design | amendment 111, A259, r13 | MINOR | reserved uid range with claim | M | |
| PH-D2 | Hanging fabric tests | amendments 102, 110, 112 | MINOR | bounded timeout, deterministic child | S | |
| PH-D3 | Namespace tests not calibrated on gpumaster | rules, amendments 109-116 | MINOR | calibrate or tag local-only | M | |
| PH-D5 | Withdrawn runner rows M2603/M2604/M2605/M2607 | amendment 102, r11 | MINOR | rows with production pair | M | PH-D6 |
| PH-D6 | `TAINT_FORCE_ON=false` unit default | r11 PSV-3 | MINOR | both settings | S | |
| PH-D8 | 59 paired-disable records | amendment 112 | MAJOR-ADJACENT | remade at freeze head | L | PH-D7 |
| PH-D9 | Runs and suites at different commits | amendment 116 | MINOR | same commit at freeze | S | PH-D7 |
| PH-D10 | PSV-6 timing; PSV-3 doc minors | r8-r13 | MINOR | boundary test; re-verify list | S | |
| PH-E1 | Why rounds did not converge | rounds 8-14 | process | n/a | n/a | |
| PH-E2 | Loop design | round 14 | process | stop rule fixed in advance | n/a | |
| PH-E4 | Next review cycle guidance | rounds 8-14 | process | n/a | n/a | |

## Part G. Traceability: the 53 confirmed loop findings

Source: `governance/notes/v022-psv1-loop-triage.md` (the checker reads both tables). "fixed in amendment 117" (or 121, which closed five of the narrowed ones) means the executed shape is fixed
with rows M3300-M3351; a spec item in the last column names a residual the fix does not close. "narrowed" means NARROW-CLAIM: not fixed, listed in the verdict's non-claims, and carried by the named item.

<!-- BEGIN TRACE (scripts/check_postc9_spec.py reads this table) -->
| idx | signature | disposition | spec item |
|---|---|---|---|
| 0 | `pure-class-builtin-reads-shared-clock` | fixed in amendment 117 | - |
| 1 | `clock-read-by-pure-builtin-no-world-taint` | fixed in amendment 117 | - |
| 2 | `match-guard-no-subject-pc` | fixed in amendment 117 | - |
| 3 | `callback-body-store-not-pc-tainted` | fixed in amendment 117 | - |
| 4 | `sealed-check-skips-struct-refinement-predicate-names` | fixed in amendment 117 | - |
| 5 | `sealed-check-skips-dyn-trait-name` | fixed in amendment 117 | - |
| 6 | `module-loader-existence-oracle-text` | narrowed | PH-A11 |
| 7 | `static-oracle-global-initializer-merged-check` | fixed in amendment 121 | PH-A12 |
| 8 | `static-oracle-dyn-trait-position` | fixed in amendment 117 | - |
| 9 | `static-oracle-predicate-expr-kinds` | fixed in amendment 117 | - |
| 10 | `builtin-callback-count-store-untainted-no-pc` | fixed in amendment 117 | - |
| 11 | `clock-reader-classed-pure-temporal` | fixed in amendment 117 | - |
| 12 | `kernel-class-for-disk-backed-dstore` | fixed in amendment 117 | - |
| 13 | `pure-builtin-reads-ambient-clock-no-world-taint` | fixed in amendment 117 | - |
| 14 | `global-token-rng-shared-across-kernels-no-taint` | fixed in amendment 117 | - |
| 15 | `scheduler-fiber-panic-isolation-skips-or-reruns-operator-store-untainted` | fixed in amendment 117 | PH-A2 |
| 16 | `goal-run-eval-count-store-untainted-no-pc` | fixed in amendment 117 | - |
| 17 | `chan-pop-under-pc-unmarked` | fixed in amendment 117 | - |
| 18 | `callback-stop-store-without-param-clean` | fixed in amendment 117 | - |
| 19 | `adaptive-provenance-push-no-kernel-mark` | fixed in amendment 117 | - |
| 20 | `select-receiver-choice-not-in-lost` | fixed in amendment 117 | - |
| 21 | `dispatch-operator-type-exempt-no-val-taint` | fixed in amendment 121 | PH-A1 |
| 22 | `dangling-operator-reference-resolved-from-candidate` | narrowed | PH-A15 |
| 23 | `merged-e0004-backstop-name-walk-no-shadowing-oracle` | narrowed | PH-A14 |
| 24 | `adaptive-provenance-write-in-call_fn-not-marked-kernel` | fixed in amendment 117 | - |
| 25 | `sealed-only-check-array-tuple-leaf` | fixed in amendment 117 | - |
| 26 | `while-cond-reevaluation-no-pc` | fixed in amendment 117 | - |
| 27 | `drop-while-callback-count-no-pc` | fixed in amendment 117 | - |
| 28 | `resume-feed-untainted-replay` | fixed in amendment 117 | - |
| 29 | `adaptive-provenance-file-world-read-unmarked` | fixed in amendment 117 | - |
| 30 | `pure-class-emitter-readable-via-proc-fd` | narrowed | PH-A10 |
| 31 | `handler-abort-skips-operator-stores-no-sticky` | fixed in amendment 117 | PH-A2 |
| 32 | `handlerdone-effect-exit-not-in-has_exit` | fixed in amendment 117 | PH-A2 |
| 33 | `refine-item-predicate-dummy-span-merged-diag` | fixed in amendment 121 | PH-A13 |
| 34 | `handler-abort-exit-skips-operator-body-no-sticky` | fixed in amendment 117 | PH-A2 |
| 35 | `refine-def-closed-regardless-of-base` | fixed in amendment 117 | - |
| 36 | `refine-name-closed-ignores-base-union` | fixed in amendment 117 | - |
| 37 | `operator-sandbox-ceiling-not-applied-to-sealed-kernel` | fixed in amendment 117 | - |
| 38 | `sealed-goal-variant-fns-get-no-seal-call-existence-oracle` | fixed in amendment 117 | - |
| 39 | `goal-eval-holdout-plain-text-before-seal-resolution` | fixed in amendment 117 | - |
| 40 | `sortby-merge-comparator-count-no-pc-variant` | fixed in amendment 117 | - |
| 41 | `sandbox-ceiling-not-applied-to-sealed-frame-per-kernel-active-sandbox` | fixed in amendment 117 | - |
| 42 | `refine-def-closed-base-shapes-beyond-bare-union` | fixed in amendment 117 | - |
| 43 | `adaptive-provenance-file-append-no-world-mark-executed` | fixed in amendment 117 | - |
| 44 | `dispatch-operator-type-exempt-ctor-closure-pick` | fixed in amendment 121 | PH-A1 |
| 45 | `sched-fiber-prebody-failure-bit-no-acc-taint` | fixed in amendment 117 | PH-A2 |
| 46 | `drift-holders-reason-prose-unchecked-feed` | fixed in amendment 117 | - |
| 47 | `kernel-class-goal-loop-advances-world-rng-stream` | fixed in amendment 117 | PH-A9 |
| 48 | `goal-loop-count-hidden-world-state-ai-cost-clock` | fixed in amendment 117 | PH-A9 |
| 49 | `callback-stop-decided-by-in-body-candidate-read-no-pc` | fixed in amendment 117 | - |
| 50 | `deferred-prefix-names-accepted-as-known-types-in-sealed-only-check` | fixed in amendment 117 | - |
| 51 | `dispatch-operator-type-exempt-via-sort-order` | fixed in amendment 121 | PH-A1 |
| 52 | `width-rule-top-level-sizedint-only-soft-wrapper-hides-width` | fixed in amendment 117 | - |
<!-- END TRACE -->

The 44 fixed and 9 narrowed findings add to 53; 0 are "other". The round 8-13 reports are not signature-keyed; their non-blocker items are carried by the item ids above, with these two exceptions that this author could not trace to a closing amendment or to an item and lists for the next reviewer: r8 PSV-1 MINOR (pin.rs header does not list `&mut` write-back; wording only) and r8 SENTINEL MINOR (`let y: i64 | u8 = v; y.ok()` union-annotation counts as closed; amendments 96 and 117 (c) touch closedness but no text names this exact case).
