# R51 post-S7 implementation review: 3921a309..9b15996e (r51-int)

Verdict: **CORRECT for the interpreter and its tests. INCORRECT in one evidence script.**
- crates/: no BLOCKER and no MAJOR. No seal hole, no oracle, and no unsealed change that §3 does not declare was found.
- Evidence scripts: one MAJOR. `scripts/pci_delta.py`'s pure-join rule accepts, as a "pure join", a merge that carries interpreter content from a commit nobody listed or classified. It was demonstrated in a scratch repo.
- Other findings: MINOR and NOTE only.

## Findings

| id | sev | file:line | claim | evidence |
|---|---|---|---|---|
| F1 | MAJOR | scripts/pci_delta.py:64-73, 94-113 | The pure-join rule accepts a merge that brings an edit from an unlisted, unclassified commit into crates/axon-core/src. The note then labels it "no change beyond the commits it joins". The route is a rename across the src boundary: side A does `git mv crates/foo/a.rs crates/axon-core/src/a.rs`, and side B edits crates/foo/a.rs, which is outside the PATHSPEC. ort carries B's edit into src/a.rs without a conflict, and the merge's src equals merge-tree's src. B's commit is not in `git log -- crates/axon-core/src` (block(), line ~151), so no row lists it. Before this patch every merge that touched src needed a hand-written themes entry, so the patch introduces this hole. No test covers it. | Scratch repo /var/tmp/r51-tmp/pjdemo, with the 9b15996e pci_delta.py copied in. After the merge, src/a.rs = `fn b() { unsafe_backdoor() }` (B's edit). `git log -- crates/axon-core/src` lists only `Merge branch 'B' into A`, `A: move a.rs into the interpreter` and `base`; B is absent. `pure_join(M, ...)` returned `('B merge', 'pure join: ... no change beyond the commits it joins')`. |
| F2 | MINOR | scripts/pci_delta.py:71-72, 100-102 | The replay is said to be "hermetic", but the verdict depends on repo, user or system git config. `merge.default` (and also merge.directoryRenames / renames) is not pinned. With `merge.default=union`, a real content conflict under src is not reported as conflicted, so a conflicted src merge is classified as a pure join. | /var/tmp/r51-tmp/pjdemo2: both sides edit the same line of src/lib.rs, merged with merge.default=union. Output `('B merge', 'pure join: ...')`. After `git config --unset merge.default`, the same commit gives `without merge.default: None`. Fix: add `-c merge.default=text -c merge.renames=true -c merge.directoryRenames=conflict` (or refuse renames that cross PATHSPEC). |
| F3 | MINOR | governance/specs/R51-sealed-engine-unification.md:521 | §14's "T7 final head" row combines runs at three heads. Two rows (M3268/M3269) were chosen by `v022_mut_select.py --granularity=fn` and then run with plain `--only`, so the receipt has no NOT EVIDENCE label. That breaks the patch's own rule (tasks/build-loop-r51.md:90-92: "the full run at the final head is the evidence ... a T7 list or receipt never cites it"). The substance holds: between 0c28bd03 (where the 442 rows ran) and 9b15996e, the only change under crates/axon-core/src is a test assertion message in resolver.rs, and 0f6a92d4..9b15996e changes nothing in crates/ or scripts/. The ledger wording overstates the procedure, not the result. | `git diff --stat 0c28bd03 9b15996e -- crates scripts`: resolver.rs 3 lines (the `"ATTACK: a duplicate ..."` assert text inside `mod tests`), harness_integrity.rs, v022_attack_markers.py, v022_g01_mutations.py, v022_paired_disable.py. `git diff --stat 0f6a92d4 9b15996e -- crates scripts`: empty. tasks/r51-baseline.md:80: "`v022_mut_select.py --changed-since=<4 lists> --granularity=fn` then `--only=M3268,M3269`". |
| F4 | NOTE | scripts/v022_attack_markers.py (M60), crates/axon-core/src/resolver.rs:3401-3406 | M60 is reinstated ACTIVE, and its only killer asserts the wording of the diagnostic. The new marker comment says that with M60 off "the duplicate is still refused, by M69's dispatch check in its words". So M60's "ATTACK" is a message-text difference, not a duplicate impl getting through. It still counts among the "444 active interpreter rows ... KILLED by their own attack". The text is §3-declared behaviour (S7-UnsealedParity-7), so this is not a defect. But M60 should not be read as seal evidence. | Marker: `'M60': r"ATTACK: a duplicate \`impl Show2 for Option<i64>\` was not refused as a duplicate impl"`. Comment: "With M60 off the duplicate is still refused, by M69's dispatch check in its words". |
| F5 | NOTE | scripts/lib/require_idle.py:14, 27-39 | The docstring promises "Anything unreadable is a usage error (exit 2), never a pass". In the code, an unreadable `/proc/<pid>/stat` is treated as "process gone" and dropped, so the guard can report idle. The impact is limited to wasted runs: the guard decides no verdict, and callers treat any exit other than 0 or 75 as fatal. | `except OSError: return None` and `except (IndexError, ValueError): return None` in read_stat. |
| F6 | NOTE | scripts/gate.sh:514-516 | A stale comment still says the trust-root preflight "stays in the serial cargo spine". It now runs in the background through `par` and is joined before the lockfile stage (behaviour checked: joined and failure-checked before PASS). | Comment `# stays in the serial cargo spine, after the PCI surfaces.)` vs `par "v0.22 trust-root preflight mechanism (dev mode)" stage_trust_root_preflight`. |

No BLOCKER. The only MAJOR is F1, which is in the evidence tooling (the PCI-delta note), not in the interpreter.

## What was checked and found correct

### Interpreter (crates/axon-core/src)

- **`depth_exceeded` now runs `seal_call` first** (interp.rs:5039).
  - Every native depth check goes through `enter_fn` -> `depth_exceeded`: grep shows interp.rs:4911/4947 and vm/mod.rs:2773/2821/2943/3174.
  - The VM fast paths that test `>= max_depth` only decline (they return None or false) and fall back to `enter_fn`.
  - `seal_call` is a pure check: `seal.active && frame_sealed && !fn_is_sealed`. With the seal off it is Ok, so unsealed text is unchanged.
  - An operator caller at the limit gets the depth text naming the callee. That is operator-visible only.
  - The new test sweeps limits 1..39 and requires both kinds of refusal to occur, and never "in `expected`".
- **`run_test_fn` now returns Err exactly when `failure_exit()` is set.**
  - Its only non-test caller is improve.rs:264 (`test_passes_as_expected`).
  - The verdict matches `axon test` (main.rs:6502-6520): `(Some(_), true)` passes and `(Some(why), false)` fails.
  - The should_fail case is covered by the new improve test.
  - Declared in §3 (row 82) and in CHANGELOG.
- **VM `run_test_body`: a body that contains `Op::Return` now runs on the tree.**
  - `Op::Return` is the only op that ends the op loop early with `Ok`. The `?` op stays `Err(Flow::Return)`.
  - `BranchReturn` is only built by the S8 peephole for a body that *is* a tail `if l op r { val } else {..}` (compile.rs `branch_return`), so it is a completion, not an early end.
  - Loops share the body's `ops` (`Body.loops` holds ranges), so a `return` inside a loop is seen. Lambdas compile separately.
  - The only effect is choosing the engine, never the value (the tree gives the same result).
- **Refinement-name rule** (resolver.rs:1219-1263).
  - `BUILTIN_GENERICS` shrinks to Option/Result/Chan, and the deferred names are now matched exactly through `checker::DEFERRED_PREFIXES.contains`.
  - The checker matches the same names exactly at checker.rs:406, 7001 and 7016, so `GoalScore`/`DictTable` cannot resolve as a type position without a definition. The struct/enum lookup still refuses defined names.
  - No sealed reach opened: a candidate refinement can attach only to an annotation the operator wrote, and an undefined `DictTable` annotation is E0308 at check.
  - Declared in §3 (resolver row) and in CHANGELOG.
- **Duplicate-impl text uses `axon_type_name`.** Text only. Declared in §3 and in CHANGELOG.
- **`loop_control_escape` for `HandlerArm`.** Text only, for a flow that was already an error (frame_edge). Declared in §3. The fixture + cli_run case asserts the exact declared text on tree, eager VM and VM.
- **Stack sizing.**
  - `STACK_BYTES_PER_FRAME` 256->320 KiB, `MIN_STACK_BYTES` 1->1.25 GiB, clamped at `MAX_STACK_BYTES` = 1e6 x 256 KiB.
  - For every depth d <= 1e6, `min(d x 320K, 1e6 x 256K) >= max(d x 256K, 1 GiB)`, so no depth reserves less than 5864c423 did. The unit test asserts this at 7 depths.
  - wasm32 is saturating and has no thread.
- **Tests.**
  - `a_sealed_call_that_binds_nothing_still_meets_the_seal_edge` has an honest control.
  - The crossing_tests goal_eval case compares against `sealed_no_fn_msg` for both an operator name and a missing name, which is a real oracle check.
  - The sealed_frames size assert (79 x 8) pins the spec T4 count.

### harness_integrity.rs

- **`env_remove("AXON_GIT_SHA")`** (line 107).
  - build.rs:20-24 embeds a set `AXON_GIT_SHA` verbatim.
  - Before this change, `a_mutation_run_restores_the_interpreter_after_every_row` (which reads `-dirty` from the miniature's binary) saw the outer tree's identity. So it was vacuous on a clean outer tree and failed spuriously on a dirty one.
  - Removing the variable makes it observe the miniature's own identity. It is now *less* vacuous.
  - The dirty-tree tests (lines ~200-216, 677-705) key on git status or synthetic `tree_clean`, not on this variable.
- **`--allow-busy` on paired-disable.**
  - No test in crates/ exercises the idle guard (grep busy|idle), so nothing that existed became vacuous.
  - The guard is untested, which is F5-adjacent. Not a regression.

### Scripts

- **v022_refusal_coverage.py: byte-identical output confirmed by running both versions.**
  - Method: `git archive` of 9b15996e into /var/tmp/r51-tmp/rcov/{old,new}, with the 3921a309 script in `old`.
  - `--remainder`: 427 lines each, identical.
  - `--without=M60,M89,M3343,...`: identical.
  - On the 3921a309 tree, old vs new script: identical.
  - Wall time 21.53 s -> 6.58 s (3.27x).
- **gate.sh (traced, not executed).**
  - `par_join` waits on each pid (`wait "${PAR_PIDS[$i]}" || rc=$?`). There is no bare `wait`.
  - A failed stage, or one killed (143) or unknown (127), fails the gate through `fail`.
  - The trust-root background join is unconditional and comes before PASS.
  - `--fast` exits before the PASS line and is refused with `--strict`, `--profile` and `--nextest`, whatever the argument order.
  - run_managed records `gate_tier=fast gate_run=no gate_strict=no`, and release_check requires `gate_run=yes`/`gate_strict=yes`.
  - No stage of the old gate was dropped or made conditional.
- **v022_mut_select / `--changed-since`.**
  - The output carries `only` + `changed_since.evidence = NOT_EVIDENCE`.
  - `--merge` and `v022_mutation_status.problems` refuse any doc with `only` set.
  - Selection compares whole trees blob by blob, so changes that arrive through a merge are included.
- **paired_disable / vm_perf_gate idle guard.**
  - Exit 75 is the only code `--allow-busy` waives. Any other non-zero exit is fatal: paired_disable "refused: the idle guard itself failed"; vm_perf_gate exits.
- **pci_delta theme migration.** 238 json files = 237 entries of the prior THEMES dict + 4bf993b8 (scout count, spot-checked).
- **pci_delta_note.rs.** It builds a real evil merge (refused) and conflicted-in-src merges resolved to one side or to merge-tree's own tree (refused), with controls.

## Checks the main agent should run

- **F1:** add a refusal of renames across the PATHSPEC boundary to pure_join, plus a pci_delta_note.rs case. For example, compare `git diff --name-status -M <parent>..<merge>` for any R entry whose source is outside and destination inside crates/axon-core/src, or run merge-tree with `-c merge.renames=false` and require that the src trees still match.
- **F2:** pin `merge.default`, `merge.renames` and `merge.directoryRenames` on the merge-tree command line.
- Then run `cargo test -p axon-core --no-default-features --test pci_delta_note` and `python3 scripts/pci_delta.py --check`.
