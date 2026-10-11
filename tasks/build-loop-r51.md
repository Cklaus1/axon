# Build harness: R51 sealed-engine unification

Spec: `governance/specs/R51-sealed-engine-unification.md` (reviewed 2026-10-10; reviews in
`governance/reviews/2026-10-10-r51-*.md`). Branch: `r51/unify` in `/home/cklaus/projects/axon-lanes/r51`.
v0.22 reference checkout (read-only): `/home/cklaus/projects/axon-lanes/v022-ref` at `c9ed2ddd`.

## Resolved placeholders

| Placeholder | Value |
|---|---|
| `<TEST_CMD>` | `./scripts/gate.sh` (main's gate; `--strict` from S6). Inner-loop fast set: `cargo test -p axon-core --no-default-features` |
| `<RUN_CMD>` | `target/debug/axon test --seal <cand> <suite.ax>` (CLI) and the axon-psv runner (`cargo test -p axon-psv --test sealed_frames`) |
| `<SMOKE_SCENARIO>` | `tasks/r51-smoke/`: one sealed suite with an honest candidate and three attack candidates (global read `TABLE[0]`, PICK through a cast, taint through a frame-pool reuse). Signal before S6: every sealed invocation exits 2 with the R51 refusal on stderr and no `"type":"summary"` on stdout. From S6: honest → summary `passed`, each attack → its refusal message; `AXON_ENGINE=vm` → exit 2 VM refusal; `env -i` run → no `AXON_VM_TRACE=1` lines |
| `<FULL_SUITE_BUDGET>` | one `gate.sh` run (baseline measured in Step 0, `tasks/r51-baseline.md`) |
| `<RUN_BUDGET>` | unbounded (compound stop + poison ceiling) |
| `<REVIEW_MODEL>` | invoking model, adversarial self-review + 3 reviewer subagents at S7 |
| `<LANE_MODEL>` | Default: the invoking model. A lane whose change is fully specified and mechanical (registry re-anchor with the rows named, evidence copy, script guard with the behaviour given, doc rows) MAY run on a faster model (`anthropic/claude-sonnet-5-5`). Any lane that diagnoses a cause, touches `crates/axon-core/src/interp*`, or decides a seal/refusal question stays on the invoking model. The integrator re-runs the acceptance commands of a faster-model lane before merging: the IdleGuard lane (2 min) returned correct code with no evidence |
| `<MAX_SPEC_DRAFT_SUBAGENTS>` | 5 |

Baseline (Step 0, main 5864c423 + spec commit): `cargo test -p axon-core --no-default-features --no-fail-fast`
= 1772 passed, 0 failed. `gate.sh` result recorded in `tasks/r51-baseline.md`.

## Task DAG

```
S0.1 merge c9ed2ddd: core conflicts + refuse-all pin + compile refusal   [CRITICAL PATH]
 ├─ S0.2 scripts: binary rule, vm_parity/vm_perf_gate conversion, BINARY_VARS
 ├─ S0.3 cli_run hunk-merge + Bins::Named spawns
 └─ S0.4 registries: COMPLETENESS json, regen .md/REFERENCE, pci_delta THEMES, deferred-test registry + drift
      └─ S0.5 S0 acceptance gate (check/fmt/clippy, non-deferred tests, harness_binaries, parity_all, vm_* gates)
          └─ S1  T1a-d engine pin + cmd_test courtesy refusal          [CRITICAL PATH]
              └─ S0b non-seal interp fixes on both engines + T9
                  └─ S5 pin.rs, Seal state, frame provenance, interp sealed-set consumers + T4
                      └─ S2.1 Bindings type (taint-required API, raw-write cutover)
                          └─ S2.2 Env factory, frame pool, store-less panic
                              └─ S2.3 eval_t<const T>, taint.rs accumulator/pc/sticky, now_ns, operand() off
                                  └─ S3.1 ClosureVal option A + marks as typed fields
                                      └─ S3.2 global_ref everywhere, general_calls, sink classification + drift
                                          └─ S3.3 seal edges/casts/dict/chan on Rc model, conform.rs
                                              └─ S3.4 Kernel split, sealed RNG/token, no_such_fn, handlers
                                                  └─ S4 PICK + receiver dispatch, T5, T8.1
                                                      └─ S6.1 flip SEAL_RUNTIME_COMPLETE, T3/T6/T8 complete, registry empty
                                                          └─ S6.2 evidence re-anchor + T7 mutation runs
                                                              └─ S6.3 T2 re-measure, strict gate receipt, §14
                                                                  └─ S7 confirmation review → zero blockers
```

Longest chain: the whole spine (19 nodes); S0.2/S0.3/S0.4 are disjoint after S0.1 and are the only parallel
track. The rest is serial on purpose: every slice after S1 edits `interp.rs`/`interp/eval.rs`, and two lanes on
the same 13k-line files would merge-conflict on every commit.

**Self-check:** acyclic (listed order is topological). Covers spec §11 S0, S1, S0b, S5, S2, S3, S4, S6, S7 and
tests T1-T9 exactly once. needs-human N1 (merge into `main`, veto freeze) and N2 (certifying review) are not
nodes.

## Loops

**Inner loop (per task):** read the veto implementation of the item first (`git -C ../v022-ref show`/grep, with
the veto commit named in the spec), then the main counterpart → implement → verify (behaviour change ⇒ the test
fails first on the parent commit, passes after; port of a veto test ⇒ same name, same assertions, passes) →
mutation-verify each new seal test (disable the edge it covers; the test must fail; if not, suspect the premise)
→ caller check (a seal edge with no production caller is not built) → self-review: diff confined to the task;
no dead code, no TODO; obsolete main code deleted, not shimmed; unsealed hot path unchanged unless the task
says so → `cargo fmt` + `cargo clippy --no-default-features -p axon-core -- -D warnings` → artifact scan →
commit (`r51(Sn): …`).

**Artifact scan** (`grep -P` over files this run creates; a hit blocks the commit):
- secret-like: `(?i)(\bbearer\s+[a-z0-9._~+/=-]{16,}|(?<![a-z0-9_])(ak|sk)[-_=:][a-z0-9_=-]{10,}|\btoken\s*[=:]\s*[^\s\`'"<>]{12,})`
- local path: `(?<![\w./-])/(Users|Volumes|var/folders|private/tmp|tmp)/[^\s\`'"<>]+` (the four review reports
  quote `/tmp/r51` scratch evidence and are exempt; nothing else is)

**Outer loop:** next ready node in DAG order. Before code, open the spec section and the review finding ids
the node cites.

**Meta loop:** each correction or failure appends mistake → rule to `tasks/lessons.md`.

**Full-suite regression loop:** after S0.5, S1, S5, S2.3, S3.4, S4, S6.3: `./scripts/gate.sh` (and from S6
`--strict` via `run_managed.sh`), failure set diffed both ways against `tasks/r51-baseline.md`. A new failure
blocks unless it is on the known-failure list below.

**Smoke loop:** at the same tiers, run `tasks/r51-smoke/run.sh` in a temp dir with a bounded timeout and check
its signal (before S6: all refused; from S6: the honest/attack table). `git status --short --ignored` must show
nothing new afterwards.

**Mutation feedback loop (T7):** in a lane, re-run only what changed:
`python3 scripts/v022_g01_mutations.py --scope=all --only=<list> --changed-since=<prior OUT.json>[,...]
[--granularity=file|fn] OUT.json` (preview without running: `python3 scripts/v022_mut_select.py` with the same
selection flags and `--head=REV`). It selects rows no record ran or shows not KILLED, rows whose registry entry or
marker changed, and rows whose anchored code (file, or the enclosing Rust fn) or killing test's source changed
since the record's commit, and prints each row's reasons. **Rule: incremental for lane feedback, the full run at
the final head is the evidence.** The OUT is a sample (`only` + `changed_since`): `--merge` and `--check-status`
refuse it, and a T7 list or receipt never cites it. Selection cannot see a change outside the anchor and the test's
own source that alters what the test observes: replaying e946f06d's T7 lists against 3921a309, M3493/M3494 went
KILLED -> REFUSED_ELSEWHERE through a debug assert in `interp/env.rs`, which neither row anchors; `file` selected
both (369/439 rows, only because their test lives in the changed `interp.rs`), `fn` selected neither (115/439).

**PCI classification and lane merges.** Each `crates/axon-core/src` commit is classified by its own file,
`governance/notes/pci-themes/<hash8>.json` = `{"theme": ..., "text": ...}` (`scripts/pci_delta.py` loads the
directory; `--drop HASH8` forgets one). Two lanes that classify different commits merge without a conflict there.
A merge commit needs no file when it is a pure join: `git merge-tree --write-tree <p1> <p2>` is clean (exit 0,
replayed with the note driver below and nothing else) and its `crates/axon-core/src` tree equals that merge's;
the block then reads `<branch> merge | pure join: ...`. A merge that conflicted anywhere, or that changes the
interpreter beyond the join (an evil merge), stays UNCLASSIFIED until a file is written for it.
One-time setup per clone (the git config is the COMMON one, shared by every worktree of the repo):
`python3 tasks/r51-pci-regen.py --setup`, which sets `merge.pci-note.driver` to
`python3 -B scripts/pci_delta.py --merge-note %O %A %B` (`.gitattributes`: the note is `merge=pci-note`): a merge
keeps ours' mechanical block and merges the rest of the note three-way. Procedure for every lane merge: `git merge`
→ resolve real conflicts → commit → `python3 tasks/r51-pci-regen.py` (also re-installs the driver) → write a
themes file only for a commit it reports UNCLASSIFIED → `python3 -B scripts/pci_delta.py --check` PASS → commit
the note (the block pins the head, so the regen after a merge stays).

## Gates

- **Critical path:** S0.1 and S1. S0.1's extra proof: on the merge commit, every sealed entry (CLI `--seal`,
  psv runner, cortex child, in-crate `run_test_fn` with a sealed set) refuses, with and without `AXON_ENGINE`.
  S1's extra proof: T1a's 48-row table and the T1d drift test fail when a fourth `.engine` reader or a new
  `compile` caller is added (mutation). If either is BLOCKED the run stops.
- **Seal invariant at every commit:** `SEAL_RUNTIME_COMPLETE` stays `false` until S6.1, and S6.1 flips it only
  in the commit where T3-T8 are green.
- **Poison-task ceiling:** attempts 1-2 diagnose/re-plan/retry; attempt 3 fails ⇒ park `blocked`, block
  dependents, continue with what is unblocked. Append to `tasks/attempts.log` before each attempt; a retry with
  the same plan is not a retry.
- **Repo conventions:** no new `R*`/`E****`/exit codes; any new `AXON_*` var gets an `env_registry.rs` row;
  `AXON_REFERENCE.md` and `AXON-COMPLETENESS.md` regenerated when their sources change; tests that spawn
  scripts use `Bins::Named`; `CARGO_TARGET_DIR` unset for `cli_run`.

## Known-failure list

Empty. Entries need: test, spec row or opportunities entry, what the assertion waits on, class
(deterministic / load-dependent), and tallies across gate logs.

## Stop condition

```
DONE = every node DONE or blocked-and-logged
   AND gate.sh --strict CITABLE at the head, no NEW failures vs tasks/r51-baseline.md
   AND T1-T9 green and §10 perf bound met at the head
   AND smoke signal met (honest passes, every attack refused, VM refused)
   AND S7 review: zero blockers
   AND spec §14 lists every receipt by path
```
