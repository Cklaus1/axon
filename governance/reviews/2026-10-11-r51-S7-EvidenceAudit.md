# S7 review: R51 evidence (Evidence lane)

## Trees
- head: `/home/cklaus/projects/axon-lanes/r51-s7` at 3921a309 (detached)
- base: origin/main 5864c423
- reference: v0.22 `/home/cklaus/projects/axon-lanes/v022-ref` at c9ed2ddd

## Method
- Read spec §4 (Fork 4), §8 T1-T9, §10, §11, §12 Q5 and §14; tasks/build-loop-r51.md; tasks/r51-baseline.md; tasks/r51-t7-reanchor-map.md.
- Re-anchor check over 67 G01 rows. That is 41 sampled rows from e41d6225 and 2022fa06 (M59, M2600-M2602, M2622-M2624, M2732-M2740, M2745-M2747, M2755-M2759, M2925, M2946, M3230, M3231, M3471, M3472, M68, M89, M93, M921, M922, M1990, M1991, M1996, M1997, M3342-M3345), plus all 13 rows changed in 55c08cc2, M58/M59, and the new rows M3483-M3495. Each was compared with its v022-ref row and code. The helper scripts are `/var/tmp/r51-tmp/s7-Evidence/show.py` (prints a row in either tree) and `dups.py`/`overlap.py` (an exhaustive same-(file, old) and overlapping-span scan of active rows, head vs v0.22).
- Ported tests: 20 cited killing tests and 6 harness helpers compared head vs v0.22. I looked for removed asserts, loosened markers, changed counts, `#[ignore]`, early returns and cfg gates.
- `scripts/wasm_stack_budget.py`: I read every R51 ALLOW entry against the recursion it names.
- Receipts: each T1-T9 test located and read, and the governance/status records checked against §9 and §14.
- Commands I ran (all read-only):
  - `python3 scripts/pci_delta.py --check`: printed `pci_delta: PASS`, rc 0.
  - `python3 scripts/v022_refusal_coverage.py`: rc 0.
  - `python3 scripts/v022_refusal_coverage.py --without=M2600,M2601,M2602,M2624`: still passes, because the PinGuard is not a refusal site.
  - A Python join of `r51-t7-list{0,1,2}.json` against the registry: 439 core rows active, 439 listed, 439 records, no core row missing, no retired row listed.
  - A Python grouping of the T7 records by (old_sha256, new_sha256).
- No build was run.

## Summary

| id | sev | subject |
|---|---|---|
| S7-Evidence-1 | MAJOR | M3471/M3472: identical mutant, same test, counted as two kills (the M58 failure mode) |
| S7-Evidence-2 | MAJOR | Unsealed `nest_cost` frames grew; the change breaks Q5 ("unsealed frame sizes stay unchanged"); no old/new table; T2 PASS only in a commit message |
| S7-Evidence-3 | MINOR | M2755/M2756 collapsed onto one line (taint.rs:920), not recorded |
| S7-Evidence-4 | MINOR | M2600/M2601/M2602/M2624: all four now hit one PinGuard in `run_arm_sealed`; the per-path seal dispatch has no row |
| S7-Evidence-5 | MINOR | M3343 id reused for a different guard; v0.22's no_such_fn text guard has neither a row nor an EQUIV record |
| S7-Evidence-6 | MINOR | wasm_stack_budget: a `*_SEALED` constant with no instance passes silently |
| S7-Evidence-7 | MINOR | Spec T4 says 78 forms / 1248 calls; head has 79 / 1264, and the test asserts no count |
| S7-Evidence-8 | NOTE | `?` completion killing tests (M564-M566) now run unsealed; the sealed leg survives in sealed_frames.rs:296 |
| S7-Evidence-9 | NOTE | Closure-pick test dropped v0.22's StaticOnly gap leg (documented) |
| S7-Evidence-10 | NOTE | T8.7 pool test has no tainted-nonzero liveness control |
| S7-Evidence-11 | NOTE | Root-only early returns: they cannot cause a false kill; paired-disable SKIP_MARK misses `SKIP:`/`NOT EXERCISED:` (unchanged from v0.22); baseline gate rows do not record euid |
| S7-Evidence-12 | NOTE | wasm ALLOW: the `value` group's MAX_CAST_DEPTH is a refusal, not a stack bound (inherited); AxonType is labelled `source` |
| S7-Evidence-13 | NOTE | M58 STALE record runs M59's own test at c7dffde5 |
| S7-Evidence-14 | NOTE (pending) | §14 is empty; T7 records are at e946f06d, not H; Fork 4 M60/M89 records are at aad46c05 and M3260 has none; strict gate, release_check, smoke and perf at H are missing |

Zero BLOCKER. Two MAJOR.

## Findings

### S7-Evidence-1: MAJOR: M3471 and M3472 are one mutant and one test, counted twice
- **Claim broken:** the claim that each T7 row proves an independent guard (spec Fork 4: rows "are re-anchored and re-run; their kills count only on the re-run"). This is the same failure mode as M58, which e41d6225 re-anchored onto M59's line.
- **Head**, scripts/v022_g01_mutations.py:11097-11102. Both rows are
  `'crates/axon-core/src/interp/taint.rs', '        let pick = if sticky != 0 && self.t_holds_op(v) {', '        let pick = if false && sticky != 0 && self.t_holds_op(v) {', 'axon-core', '--lib', 'interp::taint_tests::an_operator_value_the_candidate_picked_is_never_dispatched_on'`.
  Only the descriptions differ ("operator fn's result" vs "operator closure's result").
- **v0.22**, registry rows M3471/M3472: two sites.
  - M3471 is the fn site, `if self.taint.sticky.get() & taint::VAL != 0 && self.t_holds_op(&result) { body_t |= taint::PICK; }` (v022 interp.rs:4674-4676).
  - M3472 is the closure site, `if T && self.taint.sticky.get() & taint::VAL != 0 { if let Ok(v) = &out { ... ret_t |= taint::PICK;` (v022 interp.rs:4869-4876).
- **Observed:** the grouping of r51-t7-list*.json by (old_sha256, new_sha256) puts M3471 and M3472 in one group with the same test. They are the only R51-introduced pair that shares both the mutant and the test. Inherited pairs such as M2700/M2764 differ in crate and test.
- **Head code:** taint.rs:913-921 `t_frame_exit` is shared. Its callers are interp.rs:5371 (fn) and interp.rs:6141 (closure). v0.22's closure-site PICK block (the old text of M3472) appears 0 times on head.
- **Smallest fix:** either
  - re-anchor M3472 on the closure call, interp.rs:6141 `            self.t_frame_exit(entry_t, body_t, &v);` -> `            self.t_frame_exit(entry_t, body_t, &Value::Unit);` (closure-site pick only), and re-run it; or
  - record M3472 as STALE_REFACTORED -> M3471 with a `--only=M3472` replacement record, as was done for M58.

### S7-Evidence-2: MAJOR: unsealed frame sizes grew; Q5 and T2's "stated old/new" have no receipt
- **Claims broken:**
  - Spec §12 Q5 (spec:407-409): "the unsealed instantiation's frame sizes stay unchanged; any `nest_cost` change is stated old/new; depth ratios unchanged".
  - T2 (spec:262-264): "`nest_cost` constants may be re-measured, each change stated old/new bytes, required depth ratios unchanged".
  - The R51 rule that unsealed behaviour is unchanged.
- **Evidence:** `git diff 5864c423 3921a309 -- crates/axon-core/src/interp.rs` (constants are `b(debug, release)`, interp.rs:1625). Unsealed-instance constants grew:
  - `EVAL b(192, 288)` -> `EVAL_ARM b(192, 304)`
  - `CALL_FN_ENTRY b(112, 192)` -> `b(112, 224)`
  - `CALL_FN_IN b(160, 176)` -> `b(160, 208)`
  - `CALL_FN_IN_GENERAL b(224, 528)` -> `CALL_FN_FRAME b(272, 576)`
  - `CALL_BUILTIN b(496, 704)` -> `b(496, 720)`
  - `CALL_MUT b(160, 320)` -> `b(208, 320)`
  - `BIND_LET b(128, 208)` -> `b(160, 224)`
  - `CALL_MUT_OP b(288, 416)` -> `b(336, 432)`
  - `FINISH_RECORD b(160, 384)` -> `b(160, 432)`
  - `DISPATCH_RESUME b(224, 176)` -> `b(224, 208)`
  - `CALL_FAST_ARG`, `CALL_FAST_INLINE`, `CALL_FAST`, `CALL_MUT_GENERAL`, `EVAL_NATIVE_CALL`, `CALL_FN_GOAL`, `GOAL_EVAL_HOLDOUT`, `RUN_GOAL*` also grew.
- governance/notes/v022-pci-delta.md:322 (3561022b) gives two examples ("e.g. EVAL_ARM release 288 -> 304, CALL_FN_FRAME 224/528 -> 272/576") and concedes "on wasm32 the recursion-limit panic fires a few levels earlier on some nest probes (vm_wasm_depth.sh PASS ...)".
- There is no full old/new table in the spec, the baseline or the ledger. `vm_wasm_depth.sh PASS` appears only in commit messages and THEMES; tasks/r51-baseline.md has no row for ffe2b413, 3561022b or 3921a309.
- **Smallest fix:**
  - Add an old/new table for every changed constant to §14 or the baseline.
  - Amend Q5 to the decision actually taken: unsealed wasm32 frames grow by at most N bytes; ratios unchanged.
  - Commit `vm_wasm_depth.sh` and `vm_perf_gate.sh` (+ `--repros`) logs at H.

### S7-Evidence-3: MINOR: M2755 and M2756 collapsed onto one line, not recorded
- **Head:** both rows have `'crates/axon-core/src/interp/taint.rs', '        self.taint.acc.set(entry_t | body_t | sticky | pick);', '        self.taint.acc.set(entry_t | body_t | pick);'`.
- **v0.22:** two interp.rs sites:
  - M2755 `            body_t |= self.taint.sticky.get() & taint::VAL;\n`
  - M2756 `        ret_t |= self.taint.sticky.get() & taint::VAL;\n`
- The T7 records share the sha pair. The tests differ (`taint_survives_every_carrier_a_value_can_travel_by` / `every_hook_of_the_taint_has_an_attack_of_its_own`) and so do the markers (attack_markers.py:2534-2535), so each kill shows its own attack. This is tolerable under v0.22's own M2600 = M2602 precedent, but it is undocumented.
- **Fix:** add a registry note that both rows mutate `t_frame_exit` (callers interp.rs:5371 and :6141).

### S7-Evidence-4: MINOR: the four handler-arm pin rows now share one site
- **Head** eval.rs:2357-2361 (`run_arm_sealed`): `self.handler_edge_into(arm_sealed, payload).and_then(|()| { let _pin_guard = crate::interp::PinGuard { cell: &self.pin_fn, prev: self.pin_fn.replace(pin_owner),`.
  - It is reached from both the bare-tail path (`let outcome = if self.seal.active { self.run_arm_sealed(` eval.rs:2252) and the general path (eval.rs:2301).
  - M2600/M2602's old text is a substring of M2601/M2624's, so all four apply to the same span.
- **v0.22** had two sites: eval.rs:1803-1806 (12-space indent) and eval.rs:1842-1845 (8-space indent).
- **Observed:** `v022_refusal_coverage.py --without=M2600,M2601,M2602,M2624` still passes, so no other check names this site.
- Each test still sees its attack (the mutant is a superset). But no row now guards the per-path `if self.seal.active` routing at :2252 and :2301.
- **Fix:** add a registry note, or re-anchor M2600 and M2601 on the two dispatch lines (e.g. `let outcome = if self.seal.active {` -> `let outcome = if false {`, each unique by its context).

### S7-Evidence-5: MINOR: M3343 re-targeted under the same id
- **Head** registry :11011: "goal_eval's lookup sees only a sealed caller's own fns". Old text `.filter(|f| !(self.seal.active && self.frame_sealed.get()) || self.fn_is_sealed(f));` (goal.rs:1321).
- **v0.22** row: "goal_eval gives a sealed caller a different text for a missing name", old `return self.no_such_fn(\n name,\n format!("goal_eval: `{name}` is not a defined function"),`.
- That text is still at head goal.rs:1323-1326, and no row mutates it.
- The registry comment (:11005-11009) argues the text mutant is now equivalent, but there is no EQUIV_RECORD. The marker also changed: `'goal_eval name: the text differs'` -> `'goal_eval name of a &mut fn: the text differs'`.
- **Fix:** give the lookup guard a new id (as was done for M3495) and retire v0.22's M3343 with an EQUIV record.

### S7-Evidence-6: MINOR: sealed constants can pass the wasm budget unchecked
- scripts/wasm_stack_budget.py:311 roots the walk at `name_of(f) == "eval_arm"`. :459-461 prints `no instance (inlined in this profile)` and `continue`s, which is not a failure. That fallback predates R51 (base :404).
- The 17 R51 `*_SEALED` constants (interp.rs:1705-1722) are checked only because `eval_arm::<true>` happens to be reachable from `eval_arm::<false>` through the inlined `eval` (eval.rs:262-266) -> `eval_tainted` (taint.rs:1203).
- **Fix:** fail when a constant that is non-zero in this profile has no instance, or seed the roots with `eval_arm_sealed` too.

### S7-Evidence-7: MINOR: T4 count in the spec is stale
- Spec T4: "78 forms × 8 placements × 2 = 1248".
- Head sealed_frames.rs:1786-1869 has 79 rows; the R51 addition is :1831 `("goal_eval name of a &mut fn", ...)`. v0.22 has 78. The test asserts no count.
- **Fix:** update the spec to 79/1264, or add `assert_eq!(FORMS.len(), 79)`.

### S7-Evidence-8: NOTE: the `?` completion tests run unsealed
- Head interp.rs:8178 and :8211 build one unsealed program and run it through `outcome` (tree plus VM eager/lazy). v0.22 interp.rs:5946-5948 used `sealed_outcome("psv3q", ...)`.
- M564/M565/M566 are still killed (r51-t7 records: `ATTACK: a test that ended early at `?` was counted complete (t_find): Ok(Completed)`).
- The sealed shape is still covered through the real runner at axon-psv/tests/sealed_frames.rs:296 `a_test_ended_early_by_a_confused_question_mark_gets_no_token`. No action needed.

### S7-Evidence-9: NOTE: the StaticOnly leg of `an_operator_closure_the_candidate_picked_is_never_called` was dropped
- v022 taint_tests.rs:~247-258 asserted that the static layer alone leaves `index` and `dict key` open.
- Head keeps TaintOnly, Both and the liveness leg, and documents the removal (taint_tests.rs:~1386-1391).
- That leg was a gap-documentation assert, not a refusal, so nothing is let through.

### S7-Evidence-10: NOTE: the T8.7 pool test has no liveness control
- taint_tests.rs:1213 `a_reused_pooled_frame_starts_clean` asserts only that two taints are 0. An all-zero taint channel would pass it; other tests catch such a channel.
- **Fix:** add one tainted call asserting non-zero.

### S7-Evidence-11: NOTE: root-only tests and skips
- **Harness:** `cargo_test` in v022_g01_mutations.py records a kill only on `--- FAILED` with a marker-matched panic. A non-root early return therefore makes a row SURVIVE; it can never produce a false kill. The T7 records carry `"euid": 0`.
- **No new gates:** `git diff c9ed2ddd 3921a309` adds no `#[ignore]`, `skip_unless_root`, `NOT EXERCISED` or `SKIP:` gate, and the deferred registry is empty (count 0).
- **Head `#[ignore]`s all carry reasons:** improve.rs:1556, journal.rs:91, restart_matrix.rs:67, intake_dec_driver.rs:31, interop_evo_driver.rs:27, pointer.rs:512, r27_acceptance.rs:809. None is R51's.
- **Paired-disable skip detection:** `SKIP_MARK` (v022_paired_disable.py) is identical in v0.22 and on head. It matches `skipped:` only, so a non-root full-suite cell would count psv_test_selection.rs:256 `NOT EXERCISED:` and policy_refusals.rs:18 `SKIP:` as passes. Latent: every record so far is euid 0.
- **Baseline:** the tasks/r51-baseline.md gate rows ("2632 passed") do not record euid.
- **Fix:** record euid per gate row, and widen SKIP_MARK.

### S7-Evidence-12: NOTE: wasm ALLOW bounds
- **`bounded (taint depth)`** (wasm_stack_budget.py:103-106): real. Each `go` decrements a u8 `d` and checks it; roots are 3/32/3/3/4 (taint.rs:1016-1162, 1251, 1309, 1319).
- **`source`** (:96-102): source-bounded. `parse_type_text` reparses `chan::<T>`, which parser.rs:2474 rendered from source. The `parser::Parser>::` regex is broader than needed. `Tys::closed` visits each definition once per path, not "once" (pin.rs:236-240).
- **`value`** (:84-95): the label is honest, but MAX_CAST_DEPTH = 1_000_000 (conform.rs:432; v0.22 :402) is a refusal, not a stack bound. This is inherited from v0.22, and the code is dead on wasm32 (axon-run never seals).
- **AxonType** is matched by the earlier `source` entry (:78) although `value_type` now builds AxonType trees from value depth (conform.rs:900-901). This affects the label only.

### S7-Evidence-13: NOTE: M58 STALE record
- governance/status/v022-psv-paired-disable.json, M58: `"status": "STALE_REFACTORED" ... "replacement": "M59", "replacement_state": "REPLACEMENT_KILLED", "commit": "c7dffde5..."`.
- The replacement cell runs M59's own test (`loop_control_does_not_escape_through_a_predicate`), not M58's `an_escaped_break_or_continue_from_a_function_body_does_not_pass_a_test` (interp.rs:8055). This matches the M176 -> M293 precedent.
- The M58 marker was removed (attack_markers.py:498-499) and its GUARD_SETS pair dropped. Consistent.

### S7-Evidence-14: NOTE (pending at final head, per assignment): §14 gaps
- Spec §14 (:425-427) still reads "Empty (no slice landed)". Missing receipts:
  1. `run_managed.sh` strict gate at H, plus `verify --for H`.
  2. `release_check.sh H`.
  3. T7 at H. The records are `"commit": "e946f06d..."`, which predates ffe2b413/3561022b (interpreter moves) and 55c08cc2 (13 rows rewritten: M97, M2600, M2601, M2602, M2624, M3236, M3239, M3259, M3301, M3303, M3312, M3466, M3467). Rows with unchanged text in moved code also need a re-run: M2371-M2373, M2745-M2747, M2937, M3309, M3321.
  4. Fork 4 re-retirements at H. M60 and M89 are still `EQUIVALENT_DID` at `aad46c05` (v0.22). M3260 has no record (0 hits). M58 is at c7dffde5.
  5. Refusal coverage `--freeze` log at H.
  6. T2 logs (see S7-Evidence-2).
  7. Smoke (`tasks/r51-smoke/run.sh`) receipt.
  8. §10 perf at H after 3561022b.
- `pci_delta.py --check` PASS observed at H.

## Checked OK (re-anchor)
- **Same guard as v0.22 on head:**
  - 55c08cc2 rows: M97 (now in `struct_refine_sealed` eval.rs:2783-2788), M3236 (eval.rs:474), M3239/M3259 (eval.rs:888; already identical in v0.22), M3301 (:517), M3303 (:541), M3312 (`with_abort_exit` :2114-2122), M3466 (:433-434), M3467 (:436-437).
  - e41d6225 rows: M59 (frame_edge interp.rs:738), M2622, M2623, M2732-M2740, M2745-M2747, M2757-M2759, M2925/M2946, M3230/M3231.
  - 2022fa06 rows: M68, M89, M93, M921, M922, M1990, M1991, M1996, M1997, M3342, M3344, M3345.
- **Exhaustive (file, old) duplicate scan vs v0.22.** The new groups are:
  - M1673/M2477 (different `new`; M2477 removes the fnval half of `is_candidates`, the same property as v0.22);
  - M2733/M2734 (different `new`);
  - M2755/M2756 (S7-Evidence-3);
  - M3471/M3472 (S7-Evidence-1).
  - The new overlap groups are M2600-family (S7-Evidence-4) and M86/M1675 (different spans and `new`).
- **New rows M3483-M3495:** each has a marker. Old texts are present, and the killing tests are real with honest controls (vm/tests.rs:1060, 1276, 1334). M3491, M3492 and M3495 sit outside `seal_*` functions; the refusal-coverage gate passes.
- **run_lambda row retirement (60cede72):** sound. `run_lambda` is reachable only in the T=false branch (interp.rs:6110).
- **T7 list coverage:** 439/439 active core rows are listed and recorded; none is retired.

## Checked OK (ported tests)
- No assert removed, no marker loosened, no count changed and no `#[ignore]` or early return added in:
  - taint_tests: `an_operator_value_the_candidate_picked_is_never_dispatched_on` (11/1/19 now asserted); `operator_side_control_flow_on_candidate_data_carries_its_taint_into_what_it_runs`.
  - tests_v022_b: `runtime_sealing_holds_without_the_static_check`; `a_sealed_caller_cannot_tell_an_operator_fn_from_a_missing_one` (strengthened).
  - Handler pin tests: dispatch, replay, closure, arithmetic.
  - sealed_frames: twin oracle (strengthened), `every_position_the_sealed_walk_visits_has_a_twin_row`, `an_operator_value_the_candidate_picked_is_refused_through_the_runner`.
  - interp.rs: `no_control_transfer_escapes_a_frame`; `a_test_completes_only_when_its_body_returns_normally` (strengthened, tree+VM); `an_escaped_break_or_continue_does_not_pass_a_test` (strengthened); the M58 and M59 tests.
  - fabric `check_effects::a_pass_needs_evidence_that_the_test_completed` (strengthened: exact verdicts).
- T6 legs (axon-psv runner.rs:714, axon-cortex check_executor.rs:669) are not root-gated.

## Per-T receipts (head)
- **T1a/T1b/T1c/T1d:** tests present with controls (vm/tests.rs:1060 48 rows; :1232-1254; cli_run.rs:37384 `env_clear`; T1d reader tests). The receipt is the suite run at e946f06d, not H. T1d spec text names the pre-e213debd readers [MINOR text].
- **T2:** no receipt (S7-Evidence-2).
- **T3:** the ported suites are present; `v022_pci_gates.sh` passed at 371d3c07 only.
- **T4:** test present (S7-Evidence-7).
- **T5:** taint_tests.rs:4081 under TaintOnly, Both and live; runner half at sealed_frames.rs:2109.
- **T6:** both legs present; fabric/psv/cortex suite counts at e946f06d.
- **T7:** records at e946f06d (S7-Evidence-14).
- **T8.1-T8.9:** attack plus control present; T8.7 see S7-Evidence-10.
- **T9:** cli_run r51_s0b_* (tree / eager VM / default VM) and vm_parity at c9266cd0.
