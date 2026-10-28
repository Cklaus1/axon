# small-fixes — DRAFT spec

**Status: DRAFT (after Step 1 review). Not authorized for build.**

**Step 1 review** (2026-10-19): conformance audit `agent://AuditSmallFixes` (SFA#1–14) and dev-loop review
`agent://DevloopSmallFixes` (SFD#1–14). 28 findings: 1 blocker, 10 major, 17 minor. 28 accepted (SFA#13 by
justification rather than re-tier); 0 rejected. Each finding was re-checked before it was accepted (command or line
read noted inline).

| Id | Sev | Disposition |
|---|---|---|
| SFA#1 | major | accepted: re-ran the late-then-EOF case (15 late lines spliced, rc 0); EOF arm now honours `late`; the check-side residual is logged out of scope |
| SFA#2 | major | accepted (merged with SFD#5): the idle guard stays as it is, plus a new quiet-spell test for M3842 |
| SFA#3 | minor | accepted: rule for when quiet-after-exit is observed |
| SFA#4 | major | accepted: read `locate.rs:349-355` + M3749 (`g01 :11662-11665`); the M3749 test is rewritten to bypass `defined_fns` |
| SFA#5 | major | accepted: the ghost test calls `ghost` in code |
| SFA#6 | minor | accepted: `hole_spans` re-lexes each gap |
| SFA#7 | minor | accepted: re-measured 33 of 42 stage names leak, 5 527 dirs, 216 MB; `cortex_cli_*` 90 MB |
| SFA#8 | minor | accepted, option (a): read README :116-118; delete `misc.json#6`, don't move it |
| SFA#9 | minor | accepted (= SFD#3): cargo filter syntax |
| SFA#10 | minor | accepted (merged with SFD#10/#14): read `hx2-spec.md:281-284`, `gate-lane-spec.md:18` |
| SFA#11 | minor | accepted: read M2303/M2304 (`g01 :8166-8173`) |
| SFA#12 | minor | accepted: the FU-3 :405 amendment is stated |
| SFA#13 | minor | accepted by justification: P1 kept, reason added |
| SFA#14 | minor | accepted: re-read `runner.rs:3723-3735` |
| SFD#1 | blocker | accepted: read `gate.sh:335-336, 614-616, 660-663, 797`; sequencing rewritten so the receipt comes before the full gate |
| SFD#2 | major | accepted: last 8 receipts cost $0.1287–$0.1408 (`history.jsonl` `cost_usd_total`); `RECEIPT_CAP_USD = 1.00` (`scorecard.py:117`) |
| SFD#3 | major | accepted (= SFA#9) |
| SFD#4 | major | accepted: read `scripts/v022_attack_markers.py:1-13`; one killer and one marker per row |
| SFD#5 | major | accepted: load-robust quiet-spell test |
| SFD#6 | major | accepted: the after-exit unit test is ordered by `readable_now` waits, not sleeps |
| SFD#7 | minor | accepted: fu01 CLI timeout / hang-guard policy |
| SFD#8 | major | accepted: observed `build.py --bogus` and `--help` (with `/bin/false` binaries) enter build mode |
| SFD#9 | minor | accepted: `file` confirms the prebuilt binaries carry debug_info; reviewer's timings are cited as theirs |
| SFD#10 | minor | accepted: GL registry row |
| SFD#11 | minor | accepted: SF-3 acceptance uses a private TMPDIR |
| SFD#12 | minor | accepted: cost table split |
| SFD#13 | minor | accepted: cost table; SF-2 gets a full `--test cli` run |
| SFD#14 | minor | accepted (merged into SFA#10) |

**Step 1 review of SF-5 / SF-6** (2026-10-28): conformance audit `agent://AuditSF56` (SF56A#1–8) and dev-loop review
`agent://DevloopSF56` (SF56D#1–6). 14 findings: 2 blockers, 3 majors, 9 minors. All 14 were accepted and none
rejected. Each was re-checked first, with the command or line read noted.

| Id | Sev | Disposition |
|---|---|---|
| SF56A#1 | blocker | accepted: read `runner.rs:815-832` (`compiles` counts only `"severity":"error"` lines), :1610, :1774-1777 and :1804-1820. The shim prints a note line; OQ-5b (b) is rejected. Supersedes SF56D#2's error-severity shim line. |
| SF56A#2 | blocker | accepted: `toplet`/`toplam` re-run (check rc 0; run prints `6`/`97`), and `resolver.rs:1229-1238` read. Adds `top_level_lets`, guard cases and M3854. 20 top-level `let` names in the 290 files, ∩ builtins = 0. |
| SF56A#3 | major | accepted: same rollback path as #1; the runner/CORTEX wording drops E1203 |
| SF56A#4 | major | accepted: reader list extended. Every frozen corpus (1 017 files), `hardset`/`tempt` `build.py --check`, 360 decision-set programs and 1 626 receipt rows were checked under a simulated SF-5 rule, with 0 flips |
| SF56A#5 | minor | accepted: OQ-5's reason rewritten; still engineering |
| SF56A#6 | minor | accepted: re-ran the lock probe (E0901 + E0001 without `AXON_PATH`; E1202 with it) |
| SF56A#7 | minor | accepted: W1210 emit lines :1839/:1887/:1902; `eval.rs:2817-2840` |
| SF56A#8 | minor | accepted: M3849 mutates to `{ 0 } else { 0 }`, which keeps `failed` used |
| SF56D#1 | major | accepted: `git rev-parse sf/core` → ef19a064; `sf/int` contains d38abb5b. `sf/core` starts from `sf/int` (Sequencing); shim and C14c use `stage()`'s `StagedWs` |
| SF56D#2 | minor | accepted: marker body named and its premise added (test rc 0, check rc 0); the shim line is a note, per SF56A#1 |
| SF56D#3 | minor | accepted: `exit_code_parity.sh:29,55-56` read; strict parity commands, and SKIP is a failure |
| SF56D#4 | minor | accepted: LaneCore-exit cost row; g01 per-row estimate aligned with the SF-1/SF-2 row |
| SF56D#5 | minor | accepted: `reference_gate.sh` ran (in sync); the help string is only at main.rs:54. The harness's `:561` → `:585` is passed to Main (integrator-owned) |
| SF56D#6 | minor | accepted: exact smoke expectations, in each item's Acceptance; the harness § Smoke is integrator-owned |

Base: `main` 7f110693 (worktree `specs-next`). Source: `tasks/opportunities.md` O-FU-01 (:2671), O-FU-02 (:2674),
O-FU-07 (:2692), O-S1-21 (:2659). These line numbers are at HEAD; the worktree copy is shifted by uncommitted triage
edits (SFA checked list). Reproductions used the prebuilt binaries `/var/tmp/r51-tmp/hx-bin/{axon,cortex}`, which are
current for `crates/axon-cortex/src`, with `TMPDIR=/var/tmp/r51-tmp`. Scratch went under
`/var/tmp/r51-tmp/specs-SmallFixes/`.

## Out of scope and cross-spec dependencies

| Not here | Why |
|---|---|
| NH1 / KG2 leftover contract (leftovers after a normal exit are not killed; joins are bounded) | Settled (`tasks/killgroup-spec.md:175-212`). SF-1 does not reopen it. It narrows the FU-3 generator wording (`tasks/followup-spec.md:405`), which was a review revision, not a human decision; see SF-1 item 6. |
| Check-side drain residual: late bytes, then EOF (`runner.rs` `joined_output` :3727-3735, doc :3723-3726) | [REVISED: SFA#1, SFA#14; was "Only the generator side has the gap"]. FU-3 maps a STOPPED check drain to `BoundExceeded::unread()`. A descendant of the check wrapper that writes after the check exited and then closes inside `DRAIN_GRACE` still gets its bytes into the parsed output, which is the same residual SF-1 closes on the generator side. Not reproduced on the check side. **For Main to log as an opportunity.** |
| Remaining temp-dir leaks: about 99 non-`stage` prefixes, ≈10.4 k dirs, ≈114 MB (`cortex_cli_*` from `tests/cli.rs` `workspace()` :43-50: 8 204 dirs / 90 MB) | [REVISED: SFA#7; was "~130 prefixes"]. Outside O-FU-07. Logged as **O-SP-05** (`tasks/opportunities.md:2722`). |
| O-FU-03/O-FU-04 | Covered by `tasks/unattended-auth-spec.md` §6 E5. |
| Recognising `pub fn` / generic `fn x<T>(` in `defined_fns` | Today's text rule (`locate.rs:98-108`) is kept. SF-2 only stops matching inside strings and comments. |

**Sibling specs.** [REVISED: SFA#10, dev-loop SFD#10, SFD#14]
- `tasks/hx2-spec.md:281-284`: SF-1 (`generate.rs`) and SF-2 (`locate.rs`) both change `crates/axon-cortex/src`.
  Either one landing before hx2's FU-11 voids T1's pin and forces a re-cut of T2. So hx lands after both, or pins its
  cortex binary. SF-2 also changes `cortex locate` ranks.
- `tasks/gate-lane-spec.md:18`: whichever of the two specs lands second adds SF-4's gate-step registry row (SF-4 item 5).
- There is no dependency on checker-soundness or capability-surface. SF-1 to SF-4 do not touch `crates/axon-core/src`.
  SF-5 (`main.rs`) and SF-6 (`resolver.rs`, `error.rs`) do: each of those commits needs a pci theme json (integrator),
  and the count line `v022-pci-delta.md:585` gains their rows. SF-6 builds on CS-4 (`call_prefers_builtin`, landed
  47012208).

## Priority tiers

| Tier | Item | Digest | Source |
|---|---|---|---|
| P1 | SF-1 generator drain: a writer still writing after the generator exited is not its answer | **changes** (`src/generate.rs`) | O-FU-01 |
| P2 | SF-2 lexer-aware `defined_fns` / `test_fns` / `called_in` | **changes** (`src/locate.rs`) | O-FU-02 |
| P3 | SF-4 `hardset/build.py` exit status, argument check, `--check` default, gate step | neutral (`benchmarks/hardset/`) | O-S1-21 |
| P3 | SF-3 conformance `stage()` cleans up its workspace | neutral (`tests/`) | O-FU-07 |
| P2 | SF-6 a builtin-shadowing fn used as a value is refused (E0306), as a call to it would reach the builtin | neutral (`crates/axon-core/src`) | O-CS-03 |
| P3 | SF-5 `axon check` exits 0 when only notes/warnings were emitted | **changes** (`src/runner.rs` comments; cortex acceptance changes through `axon check`) | O-CS-04 |

[REVISED: SFA#13] SF-1 is P1, although O-FU-01 is [medium] and its sibling FU-3 was P2: the failure is a false
success. Junk is spliced into the file and the run exits rc 0 (`verified_done`), as observed below.

**Repair digest.** [REVISED: dev-loop SFD#2; was "≈ $0.14, ≈ 5 min … after both land"]
- SF-1 and SF-2 change the digest. They need one paid receipt (`scorecard.py regress`) on the integrated, committed
  head, after the last `crates/axon-cortex/src` edit (fmt/clippy fixes included). No waiver.
- Expected cost: $0.13–0.14 (the last 8 receipts were $0.1287–$0.1408) and about 5 min of model time (115 tasks,
  Σ 892 s at `--parallel 3`), plus a build of axon and `cortex --features ai` at HEAD.
- The cost doubles (≈ +$0.14, plus a cold build in main's detached worktree) if main has no receipt run at the
  merge-base digest. Today it has one (`regress-ac149215c350`).
- The receipt is repeated (`regress --again`) if main's repair digest moves before merge, and redone for any later
  repair-path edit. A fmt/clippy-only edit may be waived.
- Cap: $1.00 per invocation (`RECEIPT_CAP_USD`, `scorecard.py:117`).
- SF-3, SF-4 and SF-6 are digest-neutral (SF-6 touches only `crates/axon-core`).
- SF-5 edits `runner.rs` comments (repair path, `scorecard.py:89`), so it changes the digest. The receipt above also
  covers it if SF-5 lands before that receipt. SF-5 also changes what cortex accepts, without a cortex code change: a
  body whose only diagnostic is a note/warning now reaches `verified_done`. The receipt measures that.

---

## SF-1 (P1) Generator drain: a slow leftover writer is not "idle" — O-FU-01

**Problem.**
- `drain_capped` (`crates/axon-cortex/src/generate.rs:535-570`) marks a drain `pending` only in one case: at the
  instant `stop` is set, one non-blocking look (`readable_now`, :574-582) returns bytes (:552-556).
  `generator_output` (:589-596) rejects only `pending` drains.
- `propose` reaps the job (:716). It then calls `join_within(DRAIN_GRACE = 2 s, …)` (:717-718;
  `axon-jobctl/src/lib.rs:419, 455-485`), which lets the drains run on.
- Everything a leftover writes in that window is appended to the answer:
  - If the leftover is between writes at the stop instant, the drain looks "idle".
  - If it closes the pipe within the grace, the drain sees EOF (`Ok(0) => break`, :558).
  - Either way, the cut or polluted stream is accepted.
- The opportunity's literal fix ("pending = writer still open / not at EOF") is wrong. In the KG2/NH1 case the leftover
  (`sleep 600 &`, `tests/cli.rs:2329-2383`) also holds the pipe open with no EOF, and its answer must be kept. Neither
  EOF nor POLLHUP can separate the two cases. Only **bytes arriving after the generator was reaped and after the drain
  had emptied the pipe** can.
- A normal generator cannot produce such bytes: all of its writes finish before the leader exits, and `reap` waits for
  the leader (SFA#1).

**Reproductions** (observed with the 7f110693 binaries). Fixture `crates/axon-cortex/fixtures/broken.ax`, where
`double` returns `n + 2`.
```
(a) gen.sh:  cat >/dev/null; ( i=0; while [ $i -lt 100 ]; do printf '    // leftover %s\n' $i; i=$((i+1)); sleep 0.05; done ) & ; printf '    n * 2\n'; exit 0
    cortex repair --workspace ws --file broken.ax --check hidden_completion --axon …/axon --write-prefix broken.ax --generator cmd:…/gen.sh
    → "verified_done after 3 step(s) — repaired `double`"  rc=0  real 2.51 s; 40 lines `// leftover 0..39` spliced into `double`
(b) gen2.sh: cat >/dev/null; ( i=0; while [ $i -lt 15 ]; do sleep 0.05; printf '    // late %s\n' $i; i=$((i+1)); done ) & ; printf '    n * 2\n'; exit 0
    → "verified_done after 3 step(s)"  rc=0  real 1.25 s; grep -c '// late' ws2/broken.ax → 15     [REVISED: SFA#1, re-run]
```
Notes on the reproductions:
- (a) keeps writing past the grace; (b) finishes and closes inside it.
- Comment text is used so the junk compiles.
- The order of `n * 2` and `// leftover 0` in (a) is racy (SFA saw the reverse), so the tests never assert it.

**Change** (all in `generate.rs`; jobctl unchanged).
1. **The `exited` flag.**
   - `propose` creates `let exited = Arc::new(AtomicBool::new(false))` beside `stop` (:676).
   - It passes `Arc::clone(&exited)` to both `drain_capped` calls as a new last argument, appended after
     `Arc::clone(&stop)`. That keeps the M2303/M2304 originals (`job.child_mut().stdout.take(),\n
     MAX_GENERATOR_READ as u64,`, `g01 :8166-8173`) byte-identical. [REVISED: SFA#11]
   - It stores `true` (Release) right after `job.reap()` returns (:716) and before `join_within`. On the
     deadline/kill paths the result is `Unavailable` anyway.
2. **The drain's wait.** `drain_capped` gains `exited: Arc<AtomicBool>`. It replaces `axon_jobctl::wait_readable` with a
   local tri-state wait that uses the same 50 ms `poll` as `lib.rs:427-445`: `Readable` | `Stopped` | `Idle` (the poll
   timed out). Two state flags:
   - `quiet_after_exit`: [REVISED: SFA#3] after every `Ok(n > 0)` read and on every `Idle` timeout, if `exited`
     (Acquire) is set and `!readable_now(fd)`, set `quiet_after_exit`.
   - `late`: set when a read returns `n > 0` while `quiet_after_exit` is already set.
3. **How the drain ends.**
   - `Ok(0)` (EOF, including the `take(cap)` end) → `pending = late`, then break. [REVISED: SFA#1; was "EOF stays the
     whole-answer signal"] Bytes that arrived after the post-exit empty observation are not the answer, whether or not
     their writer closes within the grace.
   - `Stopped` → `pending = late || (readable_now(fd) && read > 0)`. Today's peek is kept.
   - `GenDrain.pending` keeps its name. Its doc (:520-524) is rewritten to: "bytes reached the pipe after the generator
     was reaped and the drain had emptied it (a writer other than the generator), or bytes were waiting at stop".
4. **Reporting.** `generator_output` and `read_answer` are unchanged, so a late writer yields
   `GenerationFailure::Unavailable("`…` output could not be read to EOF: it was still being written when the drain
   grace ran out")`. This is exit 24, as in the kg2 deadline test.
5. **Residuals**, stated in the doc comment because they cannot be fixed on one shared pipe:
   - Bytes a leftover writes BEFORE the generator is reaped (e.g. `// leftover 0` in (a)) look the same as the answer.
   - So do bytes written in the window between reap and the drain's first empty observation.
   - A writer that writes at least once more after that observation is caught.
6. **Behaviour change.** [REVISED: SFA#1, SFA#12] A generator that backgrounds a child which writes to the inherited
   pipe after the generator exits is now `Unavailable`. Before, the answer was whatever arrived before EOF or before the
   2 s grace ran out. **This amends followup-spec FU-3 (:405)**: a drain idle at stop keeps its bytes only if nothing
   was written after the post-exit empty observation. NH1 (no killing) and KG2 (bounded join) are unchanged.

**Why NH1/KG2 still hold.** KG2's leftover `sleep 600 &` holds the pipe but never writes to it (`$!` goes to a file, `cli.rs:2335`):
1. After reap, the drain reads the buffered answer and sees the pipe empty (`quiet_after_exit`).
2. No byte follows, so `late` stays false.
3. At stop the peek is empty, so `pending = false`. The answer is kept and the run exits 0.

**Tests** (fail-first where stated). Load-robustness rule [REVISED: dev-loop SFD#5–7]: no verdict depends on a
wall-clock bound. Ordering comes from `readable_now` waits (10 s deadline, 10 ms polls) or from the reap itself. A
starved run can only let a mutant survive, which costs a g01 row re-run, never a red gate on correct code.
- **CLI, fail-first:** `fu01_a_leftover_still_writing_after_the_generator_exits_is_not_its_answer` (`tests/cli.rs`).
  - Generator: reproduction (a), with the leftover pid written by the generator (`echo $! > left.pid`) and killed by
    `Proc` drop.
  - Run: `repair_with_generator(&ws, &gen, "120000")` (:2277), output to files, 300 s hang guard as in kg2
    (`cli.rs:2337-2366`). No wall-clock assertion.
  - Asserts: exit 24; `broken.ax` byte-identical to the fixture; output names "could not be read to EOF". ATTACK text:
    "a leftover still writing after the generator exited was spliced into the answer".
  - Fails today (rc 0, observed).
  - Load argument: the leftover writes for about 5 s. After the reap, correct code needs only one empty observation and
    then one more write inside the grace, and there are about 40 such chances. If the grace runs out first, stop finds
    `late` or the peek, and the run is rejected either way.
- **CLI, fail-first:** `fu01_a_leftover_that_writes_after_exit_then_closes_is_not_its_answer`. [REVISED: SFA#1]
  - Same harness as the first CLI test.
  - Leftover: `g=$$; ( while kill -0 $g 2>/dev/null; do sleep 0.02; done; i=0; while [ $i -lt 15 ]; do sleep 0.1;
    printf '    // late %s\n' $i; i=$((i+1)); done ) &`. `kill -0` succeeds on a zombie, so the subshell starts writing
    only after cortex has reaped the generator.
  - Load argument: ordered by the reap, with 15 chances. If the writes overrun the grace, stop rejects the run instead
    of EOF, so correct code is red under no load.
  - Fails today: reproduction (b), rc 0.
- **Unit:** `a_drain_written_to_after_exit_is_not_a_whole_answer` (`generate::drain_tests`, :764). [REVISED: dev-loop
  SFD#6; was "a thread writes every 50 ms; stop after 300 ms"]
  - Setup: `UnixStream` pair; `exited = true`; answer written; drain started (fd taken before the move).
  - Three rounds of: wait until `!readable_now(fd)`, sleep 100 ms, write `b"x"`.
  - Then wait until `!readable_now(fd)`, set stop, join.
  - Assert `pending`. ATTACK text: "bytes written after the generator exited were taken as its whole answer".
  - No byte is pending at stop, so only `late` can set `pending`.
- **Unit:** `a_late_write_then_eof_is_not_a_whole_answer`. Same as above, but `drop(w)` instead of stop. Assert
  `pending`. ATTACK text: "a late write followed by EOF was taken as the whole answer".
- **Unit:** `a_quiet_spell_before_exit_does_not_make_the_answer_late`. [REVISED: SFA#2, dev-loop SFD#5]
  - Setup: `exited = false`; drain started on an empty pipe; sleep 200 ms; write the answer.
  - Wait until `!readable_now(fd)`, then store `exited = true`, set stop, join.
  - Assert `!pending` and `bytes == answer`. ATTACK text: "an answer written before the generator exited was rejected
    as a late write".
  - Correct code cannot fail under load: no byte arrives after exit, and none is pending at stop.
- **Guards** (green after; body unchanged except the new `exited` argument). [REVISED: SFA#2; was "green before and
  after"]
  - `a_drain_stopped_while_idle_keeps_its_bytes` (:782), with `exited = true` stored before the drain starts.
  - `a_drain_stopped_with_data_pending_is_not_a_whole_answer` (:798).
  - `kg2_a_leftover_holding_the_generators_pipes_does_not_stall_the_loop` (CLI, unchanged).

**g01 rows** (block M3840–M3859, `scripts/v022_g01_mutations.py`, `_FUG`). [REVISED: dev-loop SFD#4] Each row names
ONE killing `(crate, target, test)`. Each gets an `ATTACK_MARKERS` entry in `scripts/v022_attack_markers.py` in the
same commit, matching the killing assertion's "ATTACK: …" text. Without markers the mutation run refuses to start
(`v022_attack_markers.py:11-13`).

| Row | Mutation | Killer |
|---|---|---|
| M3840 | `late` never set | `--lib` `a_drain_written_to_after_exit_is_not_a_whole_answer` |
| M3841 | `exited` never stored in `propose` | `--test cli` `fu01_a_leftover_still_writing_after_the_generator_exits_is_not_its_answer` |
| M3842 | `quiet_after_exit` set without checking `exited` | `--lib` `a_quiet_spell_before_exit_does_not_make_the_answer_late` |
| M3843 | EOF arm ignores `late` (`Ok(0) => break`) | `--lib` `a_late_write_then_eof_is_not_a_whole_answer` |

Re-anchor M3743 (`readable_now`'s poll line :581) if its text moves. Re-check that these stay byte-identical:
`v022_refusal_coverage.py:1717` (`let mut p = p.take(cap);`), M3602 (:717-718), M2303/M2304.

**Acceptance.**
- Both fu01 CLI tests are red before and green after.
- The three new units and both guards are green.
- All `r2_*`, `kg2_*` and `fu3_*` CLI tests stay green.
- M3840–M3843 are killed with their markers.
- `g01_anchors.py` is clean.

---

## SF-2 (P2) Lexer-aware `defined_fns` / `test_fns` / `called_in` — O-FU-02

**Problem.** FU-7 moved only `fn_body` (`locate.rs:116-119`) onto `runner::scan_code` (`runner.rs:245`). Its siblings
still scan raw text:
- `test_fns` (:67-90): `@[test` / `@[forall` at line start, then the next `fn` line.
- `defined_fns` (:98-108): a line starting `fn NAME(`.
- `called_in` (:122-145): `NAME(` anywhere in the body.

All three feed `rank` (:219) and `reached_by_all` (:291), i.e. `cortex locate` and the repair candidate list
(`bin/cortex.rs:881-885`).

**Reproduction (observed).** Command: `cortex locate --workspace . --file F --check hidden_completion --axon …/axon
--json`. Every file passes `axon check` (rc 0). `double` is the defect, and `visible_repro` calls `helper`, which calls
`double`.

| File: extra text | ranked | correct |
|---|---|---|
| `c_comment.ax`: `// was: unrelated(n)` in `helper` | `double, helper, unrelated` | `double, helper` |
| `c_string.ax`: `let s = "unrelated(";` in `helper` | `double, helper, unrelated` | `double, helper` |
| `d_ghost.ax`: `/*\nfn ghost(n: i64) -> i64 { n }\n*/` + `// ghost(n)` | `double, ghost, helper` (no such fn) | `double, helper` |
| `t_test.ax`: `/*\n@[test] */` above `fn double` | `helper` — **the defect is dropped** | `double, helper` |
| `i_interp.ax`: `helper` body `"{to_str(double(n))}"` | `double, helper` | same (must stay) |

**Constraint found while reproducing.** Non-raw strings interpolate `{expr}` (`ast.rs:352-356` `FmtStr`). The holes
are parsed by `parser.rs:80-165`: `{{` / `}}` are literal braces, and nested `{}` are depth-counted. Today's raw
`called_in` sees `double(` inside a hole (row `i_interp`). A plain move to `scan_code` would drop it and lose the real
callee, so holes count as code for calls.

**Change** (`locate.rs` only). The `scan_code` body is untouched, so the refusal anchors
`v022_refusal_coverage.py:1832-1844` stay.
1. **`fn code_view(src) -> Option<CodeView>`.**
   - One `scan_code` pass that records the visited (code) byte positions.
   - `None` on lex `Err`, consistent with FU-7: no structure from text that does not lex.
   - Derived `line_starts_in_code`: the first non-blank byte of a line is a code byte.
   - Derived `hole_spans` [REVISED: SFA#6; was "each gap that starts with `"`"]. `scan_code` does not report where a
     string or comment ends, so neighbouring tokens merge into one gap. Each gap is therefore re-lexed left to right:
     - `"` up to the first unescaped `"` is one string token. It is raw if preceded by a one-character word `r`
       (`runner.rs:261-265`).
     - `//` up to `\n`, and `/*` up to `*/`, are comments.
     - Holes are taken only inside non-raw string tokens, by the parser's rule: skip `{{`, match `}` at depth 0.
   - Residual, stated: string literals nested inside a hole are treated as code.
2. **`defined_fns`, `test_fns`.** Today's per-line logic, applied only to lines whose first non-blank byte is code.
   `test_fns` also requires the following `fn` line to be code. `None` → empty `Vec`.
3. **`called_in(body, defined)`.** Run `code_view` on the body slice. A `NAME(` match counts only if its first byte is a
   code byte or lies inside a hole span. The non-identifier-before rule (:132-136) is unchanged.
4. **Doc** on `defined_fns` (:92-97) updated: still crude (line-start text), now lexer-aware.
5. **M3749 test rewrite.** [REVISED: SFA#4] After this change `defined_fns` is empty for a source that does not lex.
   So `a_source_that_does_not_lex_has_no_reach` (:349-355), which goes through `reach` → `defined_fns`, would stay
   green under M3749 (`g01 :11662-11665`). The test is rewritten to call
   `reachable(src, "caller", &["helper".to_string()])`. Its ATTACK text is unchanged.

**Tests** (`locate.rs` `fn_body_tests`, :313-314). Fail-first ones, each with an "ATTACK: …" assertion:
- `a_call_in_a_comment_is_not_a_call`.
- `a_call_in_a_string_is_not_a_call`.
- `a_definition_in_a_block_comment_is_not_defined`. [REVISED: SFA#5; was row `d_ghost`, whose only call was in a
  comment] Source: `"/*\nfn ghost(n: i64) -> i64 { n }\n*/\nfn helper(n: i64) -> i64 {\n    ghost(n) + double(n)\n}\n
  fn double(n: i64) -> i64 { n * 2 }\n"`. Assert `ghost ∉ reach(src, "helper")`. The source need not type-check.
- `a_test_attribute_in_a_block_comment_does_not_make_a_check`.

Guard, green before and after: `a_call_in_an_interpolation_hole_is_a_call`.

**g01 rows.** [REVISED: dev-loop SFD#4; renumbered after SF-1 gained M3843] Each row has one killer, all with target
`--lib`, and each has an `ATTACK_MARKERS` entry.

| Row | Mutation | Killer |
|---|---|---|
| M3844 | `called_in` code-byte filter removed | `a_call_in_a_comment_is_not_a_call` |
| M3845 | hole spans ignored | `a_call_in_an_interpolation_hole_is_a_call` |
| M3846 | `defined_fns` line-in-code filter removed | `a_definition_in_a_block_comment_is_not_defined` |
| M3847 | `test_fns` filter removed | `a_test_attribute_in_a_block_comment_does_not_make_a_check` |

Re-anchor `v022_refusal_coverage.py:2542-2544` (`.then(|| name.to_string())`) if it moves.

**Acceptance.**
- The five tests behave as stated.
- M3844–M3847 are killed.
- **M3749 is still killed (re-verified with `--only M3749`).** [REVISED: SFA#4]
- The full `--test cli` passes once, because locate feeds the repair candidates (SFD#13).
- `build.py --check` (SF-4) exits 0, so in-scope ranks stay ≤ `CANDIDATES` (6).

---

## SF-3 (P3) Conformance `stage()` cleans up — O-FU-07

**Problem.** `stage` (`tests/conformance.rs:39-45`) creates `$TMPDIR/cortex_<name>_<pid>`, and nothing removes it.
O-FU-07 cites `stage("gen_unauthorized")` (:1589) in `cxg_c12_a_generator_supplies_the_body_and_nothing_else`
(:1262). That test actually stages NINE dirs (:1310, 1354, 1378, 1411, 1438, 1451, 1486, 1544, 1589) and removes none.

**Observed.**
- `ls -d /var/tmp/r51-tmp/cortex_gen_<x>_* | wc -l` → 183 for each of the nine `gen_*` names (dated 2026-10-10 to
  10-18).
- [REVISED: SFA#7, re-measured] Of the 42 distinct `stage("…")` names, 33 leak: 5 527 dirs, 216 MB (`du -sch`). The
  others already remove their dirs.
- For scale: 15 918 `cortex_*` dirs and 335 MB in total.

**Change.**
- `stage` returns `(PathBuf, StagedWs)`, where `StagedWs(PathBuf)` implements `Deref<Target = Path>`, `AsRef<Path>` and
  `AsRef<OsStr>`.
- `Runner::new(axon_bin(), &ws)` takes `impl Into<PathBuf>` (`runner.rs:650`), so it keeps compiling via
  `From<&T: AsRef<OsStr>>`.
- `Drop` runs `remove_dir_all` **unless `std::thread::panicking()`**, so a failed test keeps its workspace for
  debugging.
- Call sites that no longer compile get `&*ws`. The existing explicit `remove_dir_all(&ws)` lines (:220, 285, …) keep
  working.

**Fail-first test.** `stage_removes_its_workspace_when_dropped` (`conformance.rs`): `let (_, ws) =
stage("drop_probe"); let p = ws.to_path_buf(); drop(ws); assert!(!p.exists(), "ATTACK: …")`. It compiles against
today's `PathBuf` and fails.

**Acceptance.** [REVISED: dev-loop SFD#11; was a count over the shared TMPDIR, which concurrent worktrees also write]
- The test is red before and green after.
- `d=$(mktemp -d /var/tmp/r51-tmp/sf3.XXXXXX); TMPDIR=$d cargo test -p axon-cortex --test conformance -- --test-threads
  1` (any thread count works) leaves no `cortex_*` dirs for any `stage` name in `$d`. `ls -d $d/cortex_* 2>/dev/null |
  wc -l` → 0, except dirs of tests that failed.
- No g01 row: the change is test-only, and g01 mutates shipped source.

---

## SF-4 (P3) `hardset/build.py` exit status, arguments, `--check` default — O-S1-21

**Problem.**
- `crates/axon-cortex/benchmarks/hardset/build.py:362-363` calls `main()` and discards its return. `--check`
  (:332-335, `return 1 if bad else 0`) therefore always exits 0.
- `--check` with no file runs `load_specs([])` (:182-187).
- [REVISED: dev-loop SFD#8] Any other argument falls through to BUILD mode (:336-358). Build mode re-mines and rewrites
  `mining.json`, `hard-dev.json` and the sha-pinned `hard-once.json` (`protected.py:47`).

**Reproductions (observed).**
- A copy of `defects/core.json#0` with `edits[0].old` = `"THIS TEXT DOES NOT OCCUR"`: `build.py --check …/broken.json`
  → `FAIL broken.json#0 … edit `old` occurs 0 times`, **rc=0**.
- `build.py --check` with no file → no output, **rc=0**.
- `build.py --check defects/*.json` → 57 OK and 1 FAIL (`misc.json#6 … duplicate of an earlier spec`), wall 1.66 s.
- `AXON_BIN=CORTEX_BIN=/bin/false build.py --check defects/*.json` → 58 FAIL, rc 0.
- [REVISED: SFD#8, re-run] With `/bin/false` binaries, both `build.py --bogus` and `build.py --help` print
  `src/algorithms.ax is not a green snapshot with >= 2 tests`. That is build mode.

**Finding.** `misc.json#6` is a deliberate, documented reject (`hardset/README.md:136-139`: dropped as identical to
`error_handling.ax::safe_div`, `core.json:109`). With the exit fix alone, checking the committed files would exit 1
forever.

**Change.**
1. `if __name__ == "__main__": sys.exit(main())`. Build mode keeps returning `None` (exit 0): rejects there are
   "REPORTED and left out" by design (docstring :27-28).
2. [REVISED: dev-loop SFD#8] `main()` accepts exactly `[]` (build) or `["--check", FILE...]`.
   - `-h` / `--help` prints the usage (:4-5) and exits 0.
   - Any other argument prints the usage to stderr and exits 2.
   - Both happen before any binary runs or any file is written.
3. `--check` with no FILE means `sorted(glob(DEFECTS/*.json))`, the build mode's list (:344). Usage line :5 is updated.
4. [REVISED: SFA#8; was "move to `rejected-ideas.json`"] **Delete `misc.json#6`.**
   - `rejected-ideas.json` holds author-rejected ideas only: "records all 56", "author reports, verbatim"
     (`README.md:116-118`, :206).
   - `README.md:136-139` already records this drop and its reason. Append "removed from `defects/` so `--check` is
     clean".
   - The renumbering of `misc.json#7–#15` is harmless: nothing references `misc.json#N` (greps by SmallFixes and SFA).
   - `hard-dev.json` / `hard-once.json` are unaffected, because the spec was already rejected. [INFERENCE: full rebuild
     not run]
5. Gate: see OQ-2 (canon: a full-gate step). [REVISED: SFA#10, dev-loop SFD#10]
   - It runs as its own stage (inside `stage_begin`/`stage_end` once GL-1 has landed).
   - If GL-2 has landed, add a row to `governance/gate-lanes.json`:
     - class: selectable
     - `needs`: [B1 (axon build, gate.sh ≈:699), B2 (cortex build, ≈:714)]
     - read-set: `crates/axon-core/src/**`, `crates/axon-cortex/src/**`,
       `crates/axon-cortex/benchmarks/{common.py,ab/corpus.py,ab/run_ab.py,heldout/mine.py,hardset/**}`
   - Otherwise gate-lane adds the row when it lands second.

**Fail-first test.** `crates/axon-cortex/benchmarks/hardset/selftest_check.py`, binary-free, using `/bin/false` for both
binaries:
- (a) `build.py --check <tmp spec whose edit does not occur>` → rc 1. Today rc 0; the spec fails in `validate`
  :146-148 before any binary runs.
- (b) `build.py --check` with no file → rc 1, with one `FAIL` line per spec in `defects/*.json`. The expected count is
  derived from the files, not a literal (SFD#9). Today: no output, rc 0.
- (c) [REVISED: SFD#8] `build.py --help` → rc 0; `build.py --bogus` → rc 2. Neither prints "is not a green snapshot"
  (today both do, observed).

It runs in `stage_protected_sets` (`gate.sh:625-629`). That stage needs no binaries, and the self-test takes ≈ 0.1 s
(SFD#9).

**Acceptance.**
- The self-test is red before and green after.
- `build.py --check`, with built binaries, exits 0 with one OK per remaining spec (57).

---

## SF-5 (P3) `axon check` exits 2 on a note or a lint warning alone — O-CS-04 (DRAFT, after Step 1 review)

Anchors were re-read at `sf/int` b34ffd02 and still hold at 28be0dbd. `crates/axon-core` is unchanged since ef19a064
(`git diff --stat ef19a064 HEAD -- crates/axon-core` is empty). Probes were run with the ck-int axon (ef19a064), in
`/var/tmp/r51-tmp/draft-core/`.

**Lane head.** [REVISED: SF56D#1] The `conformance.rs` line numbers below are `sf/int`'s, which contains SF-3
(d38abb5b). At ef19a064, `fn cxg_c14b` is at :1959 and `stage()` returns a plain path. `sf/core` was at ef19a064
(`git rev-parse sf/core`), and Main starts it from `sf/int` (≥ 28be0dbd) before SF-5. Every fail-first run is on that
head.

**Problem.**
- `cmd_check` (`crates/axon-core/src/main.rs:1479`) runs the pipeline (:1555). It then chains three text lints onto
  `located` (:1559-1566):
  - `mut_notes`: I0002, severity `note` (:8314);
  - `floor_division_warnings`: W0007, `warning` (:8256);
  - `empty_block_warnings`: W0008, `warning` (:8285).
- It exits 0 only when `located` and `string_errors` are both empty (:1603-1606). Otherwise it prints and exits 2
  (:1608-1621). It never reads the severity.
- Pipeline warnings never reach `located`. `run_check_pipeline_located` prints them and moves on, with the comment "a
  check-phase WARNING … must NOT join the error set — that would fail `check` with exit 2" (main.rs:7543-7546;
  resolver warnings :7463). So W0003 and W0006 exit 0, while I0002, W0007 and W0008 exit 2.
- Only `cmd_check` turns the lints into an exit code. `cmd_run` prints them and carries on (:6188-6196). Those are the
  lints' only callers: grep for `mut_notes(` etc. finds only main.rs:1562-1564 and :6188-6194.
- **The documented contract.**
  - `axon check --help`: "Exit codes: 0 = no errors, 2 = type errors." (main.rs:52-54).
  - I0002 is pinned as a non-error: `parse_help_probe.rs:490-506` asserts severity `note`, "must not be an error or a
    warning".
  - W0007 is pinned as advisory under `run` ("W0007 is advisory, not fatal", `cli_run.rs:30815-30820`).
  - `AXON_REFERENCE.md` lists the codes (:289-303) and states no check exit code.

**Reproduction (observed).**

| File (source) | Diagnostics | `check` rc |
|---|---|---|
| `letmut.ax` (`let mut x = 1; x = x + 1; println(to_str(x))`) | note I0002 | **2**; `axon run` rc 0, prints `2` |
| `w7.ax` (`let x = 7 // 2`) | warning W0007 | **2** |
| `w8.ax` (`let d = {}`) | warnings W0006, W0008 | **2** |
| `unused.ax` (`let x = 1`) | warning W0006 | 0 |
| `fnval.ax` (SF-6) | warning W0003 | 0 |
| `mixed.ax` (`let mut x = 1` + `let y: i64 = "s"`) | W0006 ×2, error E0102, note I0002 | 2 |
| `lock/prog.ax` (`AXON_PATH=<dir> check --locked`, no `axon.lock`) [REVISED: SF56A#6; without `AXON_PATH` it is E0901 + E0001] | error E1202 (string path, no line) | 2 |

On a tty (`script -qc`) the note prints as `error: letmut.ax:1:28: note[I0002]: …`, rc 2.

**What reads check's rc on lint-only source** (from ScoutCheckRc's reads, completed by SF56A#4; flips marked):
- **Flips.** `crates/axon-cortex/tests/conformance.rs:1994-2153` `cxg_c14b_a_body_the_checker_objects_to_never_reaches_verified_done`.
  - Its premise asserts that check fails on the I0002 note alone (:2034-2044).
  - The learner's rejection must be `CheckerObjected` carrying I0002 (:2126-2140).
- **Behaviour flips, code unchanged.** `runner.rs:2335-2341` `checker_exit_clean` (`Ok(c == 0)`), the VerifiedDone
  gate (:1952-1962). A lint-only body now verifies.
- **Flips in step with the gate.** The grader: `benchmarks/ab/run_ab.py:276` `final_compiles = … returncode == 0`,
  defined in `plainverify/PREREG-plainverify.md:35-36` as "`axon check` exits 0 on the file as left". Cortex and the
  grader therefore stay aligned, which is the invariant C14b was written for.
- **Flips as intended; no test pins it (scout).**
  - Corpus/mutant selection by check rc: `benchmarks/guessing.py:32,63`, `oracle.py:35,63`, `adversary.py:35,65`.
    No tracked benchmark `.ax` contains `let mut` (grep), and the sweep below finds no lint-only file.
  - `crates/axon-intent/src/synth.rs:331-338` (`ok = … p.code == Some(0)` on `axon check`).
  - `scripts/axon_session.py:224-227` (a cell is refused on rc ≠ 0).
  - [REVISED: SF56A#4; the reader list was incomplete] Also, each re-checked as stated below:
    - the grader's first `compiles`, `ab/run_ab.py:261` (it gates the keyed results);
    - corpus/mutant selection: `ab/corpus.py:162,176,305`, `hardset/build.py:111`, `headroom/build.py:269`
      (`rc == 0 and "E1201" not in out`), `heldout/mine.py:116`, `out_of_scope.py:39,49,74`, `real_model.py:56,214`,
      `tempt/build.py:376`, `topk.py:27,54`;
    - axon-reflex `benchmarks/decisions/build_set.py:221`, `build_usecases.py:232,400,411`, and
      `spotcheck_usecases.py:84-86` (a `typecheck` label is `{0: True, 2: False}` of check rc);
    - `experiments/cortex-phase1/runner.py:130` (a concluded experiment, not re-run).
  - **Frozen-set result: no frozen set and no committed pool changes.** Measured on the ck-int axon, with SF-5's rule
    simulated by a wrapper (`/var/tmp/r51-tmp/draft-core/sf5sim/axon`). For `check`, the wrapper turns rc 2 with no
    `"severity":"error"` line into 0; `severity_for` gives every non-`W`/`I` code `error`
    (`diag_schema.rs:81-87`), so string errors count too.
    - Every task in the seven frozen corpora was rebuilt by `corpus.load` (sha-checked): `ab/corpus.json`,
      `hardset/hard-dev.json`, `hardset/hard-once.json`, `heldout/heldout.json`, `heldout/once.json`,
      `tempt/tempt.json` and `scorecard/receipt-corpus.json`.
    - Its pristine, broken and given_a files (1 017 files) were checked. **0 print I0002/W0007/W0008, and 0 change
      rc under the rule.** All are rc 0 except one `heldout` broken/given_a pair, rc 2 with an error, so it stays 2.
    - `hardset/build.py --check` (57 OK lines, rc 0) and `tempt/build.py --check` (rc 0, same sha256): identical
      output with the real and the simulated axon (`/var/tmp/r51-tmp/draft-core/frozen/*-{real,sim}.txt`).
    - axon-reflex `decision_set.jsonl` (500 items): every Axon program in a check-labelled item's state (360
      programs: loc, repair, tamper, route, route55, audit, axbool `typecheck`, improve) gives the same rc under both
      rules, and none prints a lint code. None of the 122 proposed patches adds `let mut`, `let x = {}` or a `x // N`
      shape.
    - Recorded receipt rows (`ab/results-regress/**`, 1 626 rows): 1 has `final_compiles: false`, and it mentions no
      lint code and no `let mut`.
    - Pools: the 290-file sweep has no lint-only file. No mutator can introduce I0002/W0007/W0008 text: comments and
      strings are never sites (`heldout/mutators.py:44,204-207`), and nothing writes `let mut`, `//` or `= {}`.
    - Forward note: headroom's `acc-init` (`headroom/mutators.py:23,201`) only rewrites an existing `let mut x = LIT`.
      It has no site in any pool today, because a `let mut` file failed check. After SF-5, a NEW pool file with `let
      mut` could be admitted and gain acc-init sites; that affects only a future rebuild.
    - `headroom/build.py --check` was not run: it refuses a binary other than the one recorded in `meta.headroom`
      (`build.py:11`), and no headroom corpus is tracked (only the calibration report).
- **Does not flip** (asserts content, not check rc):
  - `cli_run.rs:30801-30820` (W0007; `run` rc 0) and :31810-31850 (W0008);
  - `pipeline_parity.rs` (code sets);
  - `parse_help_probe.rs:325-389` (rc never read);
  - cortex `tests/cli.rs:1350-1366` (W0002, already rc 0);
  - `benchmarks/fence_sweep.py:155-160` (error codes only).
- **Sentences that state the old rule:**
  - `docs/CORTEX.md:641-645` ("`check` exits 2 on any diagnostic, a note included");
  - `runner.rs:1953-1963` and :2332-2333;
  - `conformance.rs:1994-2004`.

**Change.** OQ-5 = (a), OQ-5b = (a).
1. **`main.rs:1620-1621`.** The early exit (:1603-1606) and the emit loop stay as they are. `exit(2)` becomes:
   ```rust
   let failed = !string_errors.is_empty() || located.iter().any(|d| d.severity == "error");
   // Fix 8: exit 2 for compile errors. A note or warning alone is not one (O-CS-04).
   exit(if failed { 2 } else { 0 });
   ```
   `string_errors` holds only errors (E1203, E1201/E1202). Dev-mode W1210 is printed at once and never pushed
   (main.rs:1803-1807 doc; emitted at :1839, :1887, :1902 [REVISED: SF56A#7]), so it does not count.
2. **Help text (main.rs:54).** It becomes "Exit codes: 0 = no error (warnings and notes alone exit 0), 2 = at least one
   error." [REVISED: SF56D#5] This string is not in `AXON_REFERENCE.md` (grep count 0), so `reference_gate.sh` cannot
   drift on it.
3. **Cortex wording; no cortex code change.**
   - `runner.rs:1953-1963` and :2332-2333 lose "a note included". [REVISED: SF56A#3; was "`check` refuses what `test`
     does not: the E1203 import ceiling"] They say instead that check's exit is the grader's acceptance
     (`run_ab.py:276`). Error-severity diagnostics are already reverted by the rollback observe
     (`runner.rs:1774-1777`), so the gate is a backstop: it holds VerifiedDone to exactly the grader's rule, even if
     `check`'s exit and its printed severities ever disagree.
   - `docs/CORTEX.md:641-645` gets the same rewording, and its example names an error instead of the I0002 note.
   - `runner.rs` is a repair-path file (`REPAIR_SOURCES`, `scorecard.py:89`), so this edit **changes the digest**. The
     receipt the DAG already runs after the cortex lane covers it (harness :66-67). SF-5 must merge before CORTEX.
4. **`cxg_c14b` rewrite** (OQ-5b). The test keeps the gate exercised without a real check-only body. Its Runner's
   `axon_bin` becomes a shim script.
   - [REVISED: SF56D#1] The shim lives inside the `StagedWs` workspace from `stage()` (`conformance.rs:41-77` on
     `sf/int`). C14c takes its workspace only from `stage()`, never a hand-made path.
   - [REVISED: SF56D#2] The stubborn and learner arms propose the marker body
     `"\n    let objected_marker = n * 2\n    objected_marker\n"`. A premise assertion checks that the real axon gives
     `test` rc 0 and `check` rc 0, with no diagnostic, on it. If it failed a test, the arm would be rejected before
     the gate (`runner.rs:1952-1962`) and M3851 would survive.
   - [REVISED: SF56A#1, blocker] The shim (`#!/bin/sh`, mode 0755) is:
     `case "$1" in check) grep -q objected_marker "$2" && { printf '%s\n'
     '{"schema":"axon-diag/1","severity":"note","code":"I0002","file":"broken.ax","line":2,"col":5,"message":"shim"}'
     >&2; exit 2; };; esac; exec <axon_bin()> "$@"`.
     - The line MUST be note-severity. `observe` counts only `"severity":"error"` lines (`compiles = diags.is_empty()`,
       `runner.rs:815-832`), and the rollback observe after every patch (:1610, :1774-1777) reverts an error-severity
       body as `DidNotCompile` (:1804-1820), before `checker_exit_clean` (:1964).
     - With an error line (the first draft's `E0102`, and SF56D#2's shim), the learner's `CheckerObjected` assertion
       fails unmutated and M3851 survives.
     - `test`, `caps` and every other verb pass through (`runner.rs:741, 814, 2336, 2443, 3612, 3811`).

   `Runner::new` takes any path (`runner.rs:650`), and `run_axon_check` runs `<axon_bin> check <file>`
   (`runner.rs:737-741`). The stubborn and learner arms are unchanged except for the body. The diagnostic asserted at
   :2137-2140 stays `I0002`/`note`, now from the shim. The NOTED premise (:2020-2044) moves to the new C14c. The
   stubborn-arm message gains "ATTACK: a body `axon check` refuses reached VerifiedDone".
5. `CHANGELOG.md` entry at merge.

**Fail-first tests.**
- **`cli_run`, fail-first:** `a_note_or_lint_warning_alone_does_not_fail_check`.
  - Cases: the `letmut`, `w7` and `w8` sources above. For each, `axon check` rc 0, and stderr carries the code (I0002 /
    W0007 / W0008) and no `"severity":"error"`. For `letmut`, also `run` rc 0 with stdout `2`.
  - ATTACK text: "ATTACK: `axon check` failed a program whose only diagnostics are notes or warnings".
  - Today it fails with rc 2 (table above).
- **`cli_run`, guard** (green before and after): `a_lint_beside_an_error_still_fails_check`.
  - `mixed`: rc 2, with both E0102 and I0002 printed.
  - `check --locked`, with `AXON_PATH` set to the temp dir and no lock: `util.ax` = `fn helper(n: i64) -> i64 { n + 1 }`
    and the program `mod util\nuse util.{helper}\nfn main() -> i64 {\n    let mut x = helper(5)\n    x\n}\n`. Expect rc
    2, with E1202 and I0002.
  - ATTACK text: "ATTACK: `axon check` exited 0 on a program with an error beside a lint".
- **conformance, fail-first:** `cxg_c14c_a_lint_only_body_reaches_verified_done`.
  - Stage as C14b, then run a real-axon Runner whose generator proposes NOTED (`"\n    let mut m = n\n    m * 2\n"`).
  - Assert: real `axon test` rc 0 and `axon check` rc 0 with I0002 on stderr (the old premise, inverted);
    `EpisodeOutcome::VerifiedDone`; the file keeps `let mut`.
  - ATTACK text: "ATTACK: a body whose only diagnostic is a note was refused as CheckerObjected".
  - Today it fails: the stubborn arm of C14b shows that NOTED never verifies (:2092-2095).
- **conformance:** `cxg_c14b` as rewritten. Green after; it is M3851's killer.

**g01 rows** (LaneCore block M3848–M3855). Each row has one killer and an `ATTACK_MARKERS` entry matching its ATTACK
text.

| Row | File | Mutation | Killer |
|---|---|---|---|
| M3848 | main.rs | `located.iter().any(\|d\| d.severity == "error")` → `!located.is_empty()` | `--no-default-features --test cli_run` `a_note_or_lint_warning_alone_does_not_fail_check` |
| M3849 | main.rs | `exit(if failed { 2 } else { 0 })` → `exit(if failed { 0 } else { 0 })` [REVISED: SF56A#8; was `exit(0)`, which leaves `failed` unused] | `--no-default-features --test cli_run` `a_lint_beside_an_error_still_fails_check` |
| M3850 | main.rs | `!string_errors.is_empty() \|\| ` deleted | same test (the `--locked` case: only a string error beside a note) |
| M3851 | `crates/axon-cortex/src/runner.rs` | `Some(c) => Ok(c == 0),` → `Some(_) => Ok(true),` (:2338) | axon-cortex `--test conformance` `cxg_c14b_…` (with the note-level shim, SF56A#1) |

- M3851 is the proof behind the claim in `v022_refusal_coverage.py:1581-1586` (`_CHK`): "the gate is exercised by
  conformance cxg_c14b (which fails when it is neutralised)". No g01 row anchors on `checker_exit_clean` today (grep).
- M3850's case is the guard's `--locked` program (with `AXON_PATH`): I0002 + E1202, rc 2 (SF56A#6, SF56D checked).
- [REVISED: SF56D#4] M3851 is the first g01 row whose killer is axon-cortex `--test conformance`. Today's cortex
  killers are only `--bin cortex`, `--lib` and `--test check_executor`/`cli`/`gen_context`/`generator_cap` (grep). So
  its first `--only` run also confirms that the harness takes it.

**Sweep expectation: empty diff.** Baseline `/var/tmp/r51-tmp/sf-sweep-base.txt`:
- 0 lines with W0007 or W0008;
- 0 `rc=2` lines without an E code (`grep 'rc=2' | grep -vc 'codes=.*E[0-9]'` → 0);
- the sweep's code regex is `[EW][0-9]{4}` (`cs-sweep.sh:12`), so I codes never appear in it.
- `let mut` as a keyword occurs in 0 of the 290 files. The only textual hit is `examples/stdlib/agent.ax:198`, `let
  mut_strikes = 2` (an identifier; baseline `rc=0 codes=`).

**Codegen / parity.** None. Only `cmd_check`'s exit changes; no engine changes.

**pci.** The main.rs commit is a `crates/axon-core/src` commit, so it needs a theme json (integrator).
- M3848–M3850 add 3 to the core-row count line: `governance/notes/v022-pci-delta.md:585` ("507 rows at this head";
  `pci_delta.py --check` PASS, SF56D). Its range list gains `M3848-M3850`. [REVISED: SF56D#5; the harness's `:561`
  should read `:585` (integrator-owned file)]
- M3851 targets axon-cortex and is not counted.

**Acceptance.**
- `a_note_or_lint_warning_alone_does_not_fail_check` and `cxg_c14c` are red before and green after.
- `a_lint_beside_an_error_still_fails_check` and the rewritten `cxg_c14b` are green.
- `accepting_mut_still_tells_the_reader_it_did_nothing` and `python_floor_division_is_warned_not_silently_miscomputed`
  are unchanged and green.
- `letmut.ax`: check rc 0 with the I0002 note on stderr. `mixed.ax`: rc 2.
- [REVISED: SF56D#6] Smoke (for the harness's § Smoke 4; `build-loop-small-fixes.md` is integrator-owned).
  - Setup: `cp /var/tmp/r51-tmp/draft-core/{letmut,w7,w8,mixed}.ax /var/tmp/r51-tmp/sf-smoke/`.
  - `axon check letmut.ax` → rc 0, stdout empty, stderr exactly one line with `"severity":"note","code":"I0002"` and
    no `"severity":"error"`.
  - `axon check w7.ax` → rc 0 with W0007; `axon check w8.ax` → rc 0 with W0008.
  - `axon run letmut.ax` → stdout `2`, rc 0.
  - Control: `axon check mixed.ax` → rc 2, with both `E0102` and `I0002`.
- M3848–M3851 are killed with their markers. `g01_anchors.py` and `v022_refusal_coverage.py` are clean.
- The sweep diff is empty.

**Not in scope.** The tty prefix `error:` on a note or warning (main.rs:1613 for `check`; `emit_pipeline_diag`
:8330-8335 for `run`; observed above). It is reported to Main as a new opportunity.

---

## SF-6 (P2) A builtin-shadowing fn used as a value checks clean, then runs the builtin — O-CS-03 (DRAFT, after Step 1 review)

**Problem.** The layers disagree about what `abs_i64` names in value position.
- **Resolver.**
  - A user fn replaces the builtin's symbol (`resolver.rs:1047`).
  - `check_fn_value_ref` (:946-982), called for every non-callee `Ident` (:1931-1935), therefore sees `Symbol::Fn` and
    refuses nothing.
  - It refuses only `Symbol::Builtin` (:948) and generic fns (:956), with E0306. The reason given is that builtins are
    "dispatched by name at the call site … there is no single function value either engine could hand out" (:940-942).
  - W0003 (:1055-1074) says "the builtin takes precedence, so this function will not be called, except with a `&mut`
    argument". Its gate is `call_prefers_builtin(&f.name, false)` (:1055), whose rule is `is_builtin && !P/E/Var &&
    !&mut-arg` (`builtins.rs:2452-2454`).
- **Inference.**
  - `Expr::Ident` types a value from `fn_sigs` (`infer.rs:1096-1105`).
  - For a shadowed name, `fn_sigs` holds the BUILTIN's sig, because `insert_user_fn_sig` skips the user sig
    (`infer.rs:565-567`).
  - A call through a local has no arity check (see the `j`/`k` probes; checker path `checker.rs:4012-4031` per the
    scout, `[INFERENCE: not traced]`).
- **Interpreter.**
  - `ident_unbound` (`interp/eval.rs:2817-2840` [REVISED: SF56A#7; `fn_value(` is called at :2835]) returns
    `fn_value(name, arity, …)` (:232). That is a closure with the
    USER fn's arity that calls by NAME, and its comment says "exactly the path a direct `f(..)` call takes … The
    resolver refuses builtins and generic fns here".
  - The by-name call takes `builtin_first` (`eval.rs:1678-1680`).
  - So `f(1, 2)` calls the builtin with 2 args and panics, and a same-arity value silently runs the builtin.
- **Native codegen.**
  - A shadowing user fn is emitted as `__axon_user_<name>` (`codegen/mod.rs:970-976`, scout).
  - The value path looks up `name` itself (`codegen/expr.rs:168-178`, whose comment reads "Builtins and generic fns
    are not values (resolver E0306)"). The result is E0701.
- **Top-level `let` constants share the symbol.** [REVISED: SF56A#2, blocker] `Item::LetDef` defines
  `Symbol::Fn { param_names: vec![] }` and may silently replace a builtin (`resolver.rs:1229-1238`: the duplicate error
  is skipped when `prev` is `Symbol::Builtin`). It draws no W0003, because W0003 is only on `Item::Fn` (:1055). A
  name-only guard would therefore also refuse these programs, which work today.

**Reproduction (observed; `/var/tmp/r51-tmp/draft-core/sf6/`; ck-int axon unless noted).** Every shadowing case
also prints W0003.

| Probe | Source (body of `main`) | `check` | `run` |
|---|---|---|---|
| `fnval` | user `abs_i64(x, y)`; `let f = abs_i64; f(1, 2)` | rc 0 | rc 101 `abs_i64: expected 1 args, got 2` |
| `f_val_two_one` | user `abs_i64(x, y)`; `let f = abs_i64; f(-3)` | rc 0 | rc 101 `lambda: expected 2 args, got 1` |
| `e_map_two` | user `abs_i64(x, y)`; `arr_map([1], abs_i64)` | rc 0 | rc 101 `lambda: expected 2 args, got 1` |
| `b_val_same` | user `abs_i64(x) = x + 100`; `let f = abs_i64; f(-3)` | rc 0 | rc 0, prints `3` (builtin; user fn gives 97) |
| `a_map_same` | user `abs_i64(x) = x + 100`; `arr_map([1, -2], abs_i64)` | rc 0 | rc 0, prints `1`, `2` (builtin) |
| `c_lambda` | user `abs_i64(x, y)`; `let f = \|x: i64\| abs_i64(x); f(-3)` | rc 0 | rc 0, prints `3` (builtin, by CS-4) |
| `g_mut` | user `abs_i64(xs: &mut [i64])`; `let f = abs_i64` | rc 2, E0605 | rc 2 |
| `o_prob_pred` | user `P(x) = x + 1`; `let f = P; f(1)` | rc 0, no W0003 | rc 0, prints `2` (user fn) |
| `h_builtin_val` | no user fn; `let f = abs_i64; f(-3)` | rc 2, E0306 | rc 2 |
| `i_builtin_map` | no user fn; `arr_map([1, -2], abs_i64)` | rc 2, E0306 | rc 2 |
| `n_param_shadow` | user `abs_i64(x, y)`; `ap(abs_i64)`, `ap(f: fn(i64, i64) -> i64)` | rc 2, E0102 "found fn(i64) -> i64" (the builtin's type) | rc 2 |
| `toplet` (`s1/`) [SF56A#2] | top-level `let abs_i64 = 5`; `println(to_str(abs_i64 + 1))` | rc 0, no W0003 | rc 0, prints `6` |
| `toplam` (`s1/`) [SF56A#2] | top-level `let abs_i64 = \|x: i64\| x + 100`; `let f = abs_i64; f(-3)` | rc 0, no W0003 | rc 0, prints `97` |

**Native build.** ck-int has no `codegen` feature. With the codegen binary `.doc-build/tv-port-cg/debug/axon`
(`axon 0.1.0 (6f4332f6-dirty)`, NOT ef19a064):
- `build` of `fnval`, `b_val_same` and `a_map_same` fails in codegen with `E0701 identifier 'abs_i64' not found in
  current scope`;
- `build` of `h_builtin_val` stops at the front end with E0306, "1 error(s); build aborted".

No native binary was run: `libaxon_rt.a` is not found beside that binary.

**Precedence chosen** (OQ-6 = (a)). A name means the same thing in value position as in a no-`&mut` call:
`call_prefers_builtin(name, false)`.
- When that is true, the value names the builtin, and a builtin is not a value: E0306, as for `h_builtin_val`.
- Then checker, interpreter and codegen agree, because all three stop at the same front-end refusal (`run` and
  `build` run the resolver first, per `h_builtin_val`). Neither engine changes.
- Each case required by the brief:
  - **Higher-order builtin** (`arr_map(xs, abs_i64)`): E0306, same arm.
  - **Lambda wrapper** (`|x: i64| abs_i64(x)`): unchanged. It is a call, so it reaches the builtin (CS-4), and it is the
    E0306 fix-it's own repair.
  - **`&mut` shadowing** (CS-4's exception): a value never carries a `&mut` argument, so the value of a shadowing fn
    is refused. A shadowing fn with `&mut` params is already refused as a value by E0605 (`g_mut`), and keeps E0605
    beside the new E0306. Direct `abs_i64(&mut a)` still runs the user fn
    (`mut_arg_call_to_shadowing_fn_still_runs_the_user_fn`, `cli_run.rs:40549`).
  - **Same arity** (`b_val_same`, `a_map_same`): E0306. These run rc 0 today, printing the builtin's result.
  - **`P`/`E`/`Var`**: `call_prefers_builtin` is false, so the value is the user fn, as in a call (`o_prob_pred`
    unchanged).
  - **Top-level `let` named like a builtin** (`toplet`, `toplam`): unchanged. The constant is the value, as today; it
    is not a fn, so W0003's "the builtin takes precedence" does not describe it.

**Change** (`crates/axon-core/src` only).
1. **`check_fn_value_ref`** (`resolver.rs:946-982`) gets a second refusal arm:
   `Some(Symbol::Fn { .. }) if crate::builtins::call_prefers_builtin(name, false) && !self.top_level_lets.contains(name)
   => …`.
   - [REVISED: SF56A#2] `Resolver` gains `top_level_lets: HashSet<String>`, declared and initialised beside
     `generic_fns` (:839, :866) and filled in the `Item::LetDef` arm (:1229).
   - It uses the builtin's params (the `BUILTINS` lookup at :949-953, shared with the builtin arm, not copied).
   - Message: "`{name}` names the builtin `{name}` here (your `fn {name}` shadows it, W0003), and a builtin cannot be
     used as a value — only a non-generic user-defined `fn` can be passed, stored or bound by name".
   - Fix-it: "rename your `fn {name}` to pass it by name, or wrap the builtin in a lambda — `|{list}| {name}({list})`".
   - The `kind == "a builtin"` test (:962) becomes a bool, so both builtin arms get the untyped lambda.
2. **E0306's description** (`error.rs:456`) gains "or a user fn that shadows a builtin (W0003)". The integrator
   regenerates `AXON_REFERENCE.md:166` (`reference_gate.sh` regenerates from the compiler's tables).
3. **Comments.** These become wrong by omission and are corrected: `interp/eval.rs:2828` ("The resolver refuses builtins
   and generic fns here") and `codegen/expr.rs:168-171`. There is no other interp or codegen change.
4. **`pipeline_parity.rs` `cases`** gains `fnval`, so that `lib::check_pipeline` (playground/LSP) is shown to refuse
   it too. The resolver is shared, so this is a guard, green before and after.
5. `CHANGELOG.md` entry at merge.

**Fail-first tests.**
- **`cli_run`, fail-first:** `a_shadowing_fn_used_as_a_value_is_refused`.
  - Cases: `fnval`, `f_val_two_one`, `e_map_two`, `b_val_same` and `a_map_same`.
  - For each: check rc 2, with E0306 naming `abs_i64` and the word `builtin`, and also W0003. `run` rc 2 (never 101,
    never stdout).
  - ATTACK text: "ATTACK: a builtin-shadowing fn used as a value checked clean".
  - Today it fails with check rc 0 (table above).
- **`cli_run`, guard** (green before and after): `a_lambda_or_a_prob_pred_keeps_its_value_rule`.
  - `c_lambda`: check rc 0 with W0003 and no E0306; run prints `3`.
  - `o_prob_pred`: check rc 0, no E0306; run prints `2`.
  - `g_mut`: rc 2, E0605.
  - `toplet`: check rc 0, no E0306; run prints `6`. `toplam`: check rc 0, no E0306; run prints `97`. [REVISED: SF56A#2]
  - ATTACK text: "ATTACK: a value the precedence rule allows was refused".
- **`integration_fixtures.rs`, fail-first** (`#[cfg(feature = "codegen")]`, beside
  `shadowed_builtin_call_reaches_the_builtin_on_both_engines` :2414):
  `a_shadowing_fn_value_is_refused_before_native_codegen`.
  - `axon build fnval.ax`: rc ≠ 0, stderr carries E0306 and no `E0701`.
  - ATTACK text: "ATTACK: a builtin-shadowing fn value reached native codegen".
  - Today: E0701 (observed on the 6f4332f6 codegen binary; to be re-observed red on the lane head).
- Unchanged and green: `call_to_shadowed_builtin_is_typed_against_the_builtin` (:40522),
  `mut_arg_call_to_shadowing_fn_still_runs_the_user_fn` (:40549), `user_fn_named_like_a_prob_pred_does_not_warn_w0003`
  (:40565), `user_fn_shadowing_a_builtin_warns_w0003` (:17873), and the existing builtin/generic E0306 tests
  (`cli_run.rs:11538-11555`, :8188-8213).

**g01 rows.**

| Row | File | Mutation | Killer |
|---|---|---|---|
| M3852 | resolver.rs | the new arm's guard `if crate::builtins::call_prefers_builtin(name, false)` → `if false` | `--no-default-features --test cli_run` `a_shadowing_fn_used_as_a_value_is_refused` |
| M3853 | resolver.rs | in the new arm, `call_prefers_builtin(name, false)` → `is_builtin(name)` (drops the `P`/`E`/`Var` exception) | `--no-default-features --test cli_run` `a_lambda_or_a_prob_pred_keeps_its_value_rule` |
| M3854 | resolver.rs | ` && !self.top_level_lets.contains(name)` deleted [REVISED: SF56A#2] | same test (the `toplet`/`toplam` cases) |

- The anchor text `call_prefers_builtin(name, false)` is unique in resolver.rs: W0003's call is
  `call_prefers_builtin(&f.name, false)`.
- M3853's anchor must include enough of the arm to differ from M3852's. The lane takes M3853's anchor as the arm line
  and M3852's as the guard alone, if `g01_anchors.py` requires distinct anchors.
- M3855 stays spare.

**Sweep expectation: empty diff.**
- Of the 343 builtin names in `AXON_REFERENCE.md`'s builtin table (excluding `Chan::new`), only `pow_i64` is also
  defined as a fn in the 290 files (`comm -12` of the two name lists).
- It is defined at `examples/algorithms.ax:45` and `crates/axon-cortex/benchmarks/hardset/src/algorithms.ax:45`, and
  used only as a direct call (:59, `pow_i64(2, 10)`). No non-call reference exists (grep).
- These are the baseline's only W0003 lines (2, both `rc=0 codes=W0003,`), and they stay identical.
- [REVISED: SF56A#2] The 290 files define 20 distinct top-level `let` names; none is a builtin (`comm -12` → 0), so
  the sweep cannot see the top-level-let case. The guard test covers it.

**Codegen / parity** (the LaneCore exit runs these; there is no codegen source change, so they are non-regression).
- `cargo test -p axon-core --test integration_fixtures -- shadow` (default features include `codegen`,
  `Cargo.toml:34`). All 137 fixtures take 0.69 s at base (`ck-gate.log:11564`).
- [REVISED: SF56D#3] `AXON=<sf-core codegen axon> AXON_HARNESS_STRICT=1 ./scripts/exit_code_parity.sh`,
  `AXON_HARNESS_STRICT=1 ./scripts/closure_capture_parity.sh` (fn values and closures) and
  `AXON_BIN=$CARGO_TARGET_DIR/debug/axon ./scripts/vm_parity.sh` (tree vs VM). All PASS at base (`ck-gate.log:11595`,
  :11600, :11625).
  - Without `$AXON`, `exit_code_parity.sh` cold-builds codegen axon into a `mktemp -d` target on every run (:29,
    :55-56). Both codegen scripts exit 0 when codegen cannot be built (headers :21-22), so a SKIP line counts as a
    failure here (`AXON_HARNESS_STRICT=1` makes it fatal, `scripts/lib/axon_bin.sh:119`).
  - `gate.sh --strict` re-runs all three through `parity_all.sh` (:1054) and the codegen fixtures (:1033).

**pci.** The resolver.rs/error.rs commit needs a theme json (integrator). M3852–M3854 add 3 to the count line, with
range `M3852-M3854` (SF-5 + SF-6: 507 → 513).

**Acceptance.**
- `a_shadowing_fn_used_as_a_value_is_refused` and `a_shadowing_fn_value_is_refused_before_native_codegen` are red before
  and green after.
- The guard and the unchanged CS-4 tests are green.
- `fnval.ax`: check rc 2 with E0306 and W0003; run rc 2.
- The `integration_fixtures` filter and the three parity scripts pass, with no SKIP line.
- M3852–M3854 are killed with their markers. `g01_anchors.py` and `v022_refusal_coverage.py` are clean; the new
  `emit_error` site is covered or exempted as the script reports.
- The sweep diff is empty.
- [REVISED: SF56D#6] Smoke (for the harness's § Smoke 5). Setup: also copy
  `/var/tmp/r51-tmp/draft-core/sf6/{fnval,b_val_same,c_lambda,o_prob_pred}.ax` and `draft-core/s1/toplet.ax` to
  `/var/tmp/r51-tmp/sf-smoke/`.
  - `axon check fnval.ax` → rc 2; stderr has a `W0003` line and an `"severity":"error","code":"E0306"` line whose
    message contains `` `abs_i64` names the builtin `abs_i64` ``.
  - `axon run fnval.ax` → rc 2, stdout empty, no `panic:` on stderr.
  - `axon check b_val_same.ax` → rc 2 with E0306 (it was rc 0 and printed `3`).
  - Controls: `axon run c_lambda.ax` → stdout `3`, rc 0; `axon run o_prob_pred.ax` → stdout `2`, rc 0;
    `axon run toplet.ax` → stdout `6`, rc 0.

**Not in scope** (observed, ck-int).
- A call through ANY fn value is not arity- or type-checked:
  - `j_user_arity`: `fn add2(x, y)`, `let f = add2; f(1)`;
  - `k_lambda_arity`: `let g = |x: i64, y: i64| x + y; g(1)`;
  - `l_user_type`: `f(1, "s")`.
- All three give check rc 0 and run rc 101 (`lambda: expected 2 args, got 1`; `cannot apply Add to i64 / str`).
- A user higher-order param IS checked: `m_param`, `ap(add2)` with `f: fn(i64) -> i64`, gives E0102, rc 2.
- SF-6 removes O-CS-03's instance by refusing the reference; it does not close this hole. It is reported to Main as a
  new opportunity (OQ-6b).

---

## Gates and cost budget

[REVISED: dev-loop SFD#3, SFD#12, SFD#13; was an 8-row table with invalid cargo filters] Measured figures are SmallFixes'
own or the dev-loop reviewer's (marked SFD). Test-execution times come from `fu-gate-2.log`, per SFD#13.

| Gate | One run | Scarce resource | Re-run trigger | Skip rule | Waits on it |
|---|---|---|---|---|---|
| `cargo test -p axon-cortex --lib -- generate::drain_tests locate::` | 0.35 s of tests (SFD); the rest is incremental rebuild [INFERENCE 1–3 min] | CPU | each SF-1/SF-2 edit | — | — |
| `cargo test -p axon-cortex --test cli -- fu01_ kg2_ r2_ fu3_` | a few seconds of tests (fu01 waits up to DRAIN_GRACE) plus rebuild; the first run also builds axon | CPU | SF-1 edits; for SF-2, the whole `--test cli` once (7.5–8.1 s of tests, SFD) | SF-3/4 | — |
| `TMPDIR=<private> cargo test -p axon-cortex --test conformance` | 3.7–4.2 s of tests (SFD) plus rebuild | CPU | SF-3 | SF-1/2/4 | — |
| `v022_refusal_coverage.py` + `g01_anchors.py` | 7.5 s + 0.2 s (SFD) | none | every SF-1/SF-2 edit | — | — |
| `hardset/selftest_check.py` | ≈ 0.1 s | none | SF-4 | — | — |
| `build.py --check` (built binaries) | 1.1–1.8 s at load 15–20; ≤ 7 s on one core; 546 child processes (SFD); 1.66 s (SmallFixes) | CPU; axon and cortex built | SF-2, SF-4, or any change to its read-set (SF-4 item 5) | inner loop: read-set unchanged | — |
| g01 `--only M3840…M3847` and `M3749` | one mutated rebuild + one test per row [INFERENCE 1–3 min; more for M3841, the `--test cli` row] | CPU | after each SF-1/SF-2 commit | rows of an untouched item | needs a clean committed tree and markers |
| `cargo fmt --all -- --check` + `cargo clippy --locked -p axon-cortex --all-targets --features ai -- -D warnings` (gate.sh:797) | [INFERENCE 1–3 min warm] | CPU | before the receipt | — | receipt |
| cortex lane `cargo test -p axon-cortex --features ai` | 10–15 min | CPU | once, after the last src edit | — | receipt |
| receipt `scorecard.py regress` | $0.13–0.14, ≈ 5 min model time, plus HEAD build; ×2 if main has no receipt at the merge-base digest | **money**, API key, clean tree | a later digest edit; main's digest moving; a "worse" verdict | never waived (fmt/clippy-only edit: may waive) | full gate, merge |
| SF-5/SF-6 `cargo test -p axon-core --no-default-features --test cli_run -- <the 4 named tests>` | seconds of tests plus the cli_run rebuild [INFERENCE 2–5 min]; the whole `cli_run` is 893 tests in 471.79 s (`ck-gate.log`) | CPU | each SF-5/SF-6 edit | — | — |
| SF-5 `TMPDIR=<private> cargo test -p axon-cortex --test conformance -- cxg_c14` | conformance 138 tests in 3.0–3.9 s (`ck-gate.log`) plus rebuild | CPU | SF-5 | SF-6 | — |
| SF-6 `cargo test -p axon-core --test integration_fixtures -- shadow` (codegen) + `exit_code_parity.sh`, `closure_capture_parity.sh`, `vm_parity.sh` (commands as in SF-6 § Codegen / parity) [REVISED: SF56D#3] | 137 fixtures in 0.69 s at base, plus one codegen build of axon in `.doc-build/sf-core` [INFERENCE ≥ 5 min cold], then the scripts; per-script time not logged; a SKIP line is a failure | CPU | SF-6 | SF-5 | — |
| LaneCore exit `cargo test -p axon-core --no-default-features` [REVISED: SF56D#4] | ≥ 8 min [INFERENCE: cli_run alone is 471.79 s, `ck-gate.log`] | CPU | once per lane exit | — | merge |
| `cs-sweep.sh` vs `/var/tmp/r51-tmp/sf-sweep-base.txt` | ≈ 3 s (290 files) | none | after the SF-5 and SF-6 commits | — | — |
| g01 `--only M3848,…,M3854` [REVISED: SF56D#4; was "≈ 30 s per row"] | one mutated axon + test-binary rebuild per row [INFERENCE 1–3 min per row]; M3851 is the first `--test conformance` killer, so its first run also confirms the harness takes it | CPU | after each SF-5/SF-6 commit | rows of an untouched item | needs a clean committed tree and markers |
| `scripts/reference_gate.sh` [REVISED: SF56D#5; was "[not measured]"] | 0.02 s (measured, SF56D) | none | SF-6 (E0306 row); SF-5's help text is not in the reference | — | full gate |
| full `./scripts/gate.sh` | 32–39 min (last 38m51s, 6967/0/12) | shared host, all cores | any commit after it | none; `--fast` is not evidence | merge |

For SF-1 to SF-4, no pci theme json or `pci_delta.py` work is needed. SF-5 and SF-6 each need a theme json per
`crates/axon-core/src` commit, and +3 (M3848–M3850) and +3 (M3852–M3854) on `v022-pci-delta.md:585` (integrator).

## Sequencing

[REVISED: dev-loop SFD#1 (blocker); was "cortex lane + full gate → receipt → merge"] Neither the full gate nor `--fast`
can pass on a changed repair digest until the receipt is committed. `scorecard.py receipt --check` runs in the first
parallel group in both: `gate.sh:335-336` (`--fast`) and `:614-616` via `par` at :660, joined at :663. `regress`
refuses a dirty tree (`scorecard.py:1420` doc). Any `crates/axon-cortex/src` edit after the receipt means a new
digest, so the receipt must be re-run (or waived if behaviour-neutral), and then the full gate re-run.

```
SF-1, SF-2, SF-3, SF-4 (each: inner-loop rows above; g01 --only for its own rows on its own commit)
 → integrate on one head; cargo fmt --all -- --check; cargo clippy --locked -p axon-cortex --all-targets --features ai -- -D warnings;
   python3 -B scripts/v022_refusal_coverage.py; python3 -B scripts/g01_anchors.py; build.py --check
 → cortex lane `cargo test -p axon-cortex --features ai`      (no crates/axon-cortex/src edit after this point)
 → commit → scorecard.py regress (clean tree, OPENROUTER_API_KEY) → commit results-regress/, history.jsonl, README.md, progress.svg
 → full ./scripts/gate.sh on that head (stage_cortex_receipt now passes) → merge
```
- SF-1 and SF-2 touch disjoint source files and may run in parallel. Both append to `v022_g01_mutations.py` and
  `v022_attack_markers.py` with disjoint ids; the integrator merges them.
- SF-4 lands before or with SF-2, so that SF-2's rank acceptance is an exit code.
- SF-3 is independent.
- SF-5 and SF-6 (LaneCore, after their Step 1 review) are independent of SF-1 to SF-4 and of each other. They append
  disjoint ids (M3848–M3851, M3852–M3854) to the same two scripts.
- [REVISED: SF56D#1] `sf/core` starts from `sf/int` (≥ 28be0dbd, which contains SF-3 d38abb5b), not from ef19a064:
  SF-5's conformance edits are written against SF-3's `stage()` / `StagedWs`.
- SF-5 edits `runner.rs` (repair path). It lands before the cortex lane run and the receipt, or the receipt is re-run.
  Its `cxg_c14b`/`cxg_c14c` run in that cortex lane.
- SF-6 does not wait on the receipt, because it is digest-neutral. Its codegen fixture and parity scripts run before
  the full gate.

## Open questions (Step 2 classification)

- **OQ-1 (engineering, canon): what signals a slow writer in SF-1.**
  - (a) Pending = no EOF at stop. Rejected: breaks KG2/NH1, because that leftover never closes.
  - (b) POLLHUP. Rejected: the same as EOF on a pipe.
  - (c) Scan `/proc/*/fd` for holders of the write end. Rejected: racy, Linux-only, and cannot tell a silent holder
    from a writer.
  - (d) **Late bytes after reap plus an empty pipe, at EOF and at stop (chosen).** [REVISED: SFA#1 adds "at EOF"] It
    separates a silent holder (keep) from a writer (reject) and needs no jobctl change. Residuals are stated in SF-1
    item 5.
- **OQ-1b (engineering, canon): apply SF-1 to stderr too?**
  - (a) Both pipes (chosen): matches FU-3's symmetric `generator_output`, and stderr carries the `Declined` reason.
  - (b) Stdout only: a leftover chatty on stderr would then not cost the answer. Revisit only if a real generator trips
    it.
- **OQ-1c (engineering, canon): is narrowing the FU-3 :405 wording a reopening of NH1?**
  - (a) No (chosen). NH1 settles "leftovers are not killed" and KG2 "the join is bounded"; both are unchanged. The
    "idle keeps its bytes" sentence was a Step-1 review revision of FU-3, not a human decision.
  - (b) Treat it as needs-human. Rejected: no product or irreversible exposure, and only the false success changes.
- **OQ-2 (engineering, canon): should `build.py --check` join a gate?** The O-S1-21 note assumed it is too costly to
  run in every gate.
  - Cost: [REVISED: dev-loop SFD#9; was "caveat: debug binaries may be slower"] ≈ 1–2 s (1.06 / 1.79 s at load 15–20,
    HARDSET_JOBS=6, SFD), ≤ 7 s worst case (6.60 s on one core), 5–10 s of CPU, 546 child processes per run. The
    prebuilt binaries measured are already debug builds (`file` → `with debug_info, not stripped`).
  - (a) **Full gate, right after the R1f fence sweep (`gate.sh:726-729`), with `AXON_BIN` / `CORTEX_BIN =
    $(built_bin …)` (chosen).** The binaries are already built there, and it turns FU-7/SF-2's "ranks stay ≤
    CANDIDATES" from a manual note into a check.
  - (b) Cortex lane only. GL's registry row (SF-4 item 5) decides lane selection anyway.
  - (c) Not gated.
- **OQ-3 (needs-human, scope): the remaining temp-dir leaks** (≈114 MB, 34 % of the 335 MB; `cortex_cli_*` 8 204 dirs
  / 90 MB). [REVISED: SFA#7; was "≈ 99 % of the 335 MB"]
  - (a) Leave them logged.
  - (b) Reuse SF-3's drop guard from `tests/common` in a later batch. **Recommended**: same mechanism, zero digest
    impact.
  - (c) Widen SF-3 now.
  - Already logged by Main as **O-SP-05** (`tasks/opportunities.md:2722`). Scheduling it is the human's call; SF-3 stays
    at O-FU-07's scope.
  - `[DECIDED by human 2026-10-27: (b) — a later batch reuses SF-3's drop guard; SF-3 stays at O-FU-07's scope.]`
- **OQ-4 (engineering, canon): `misc.json#6`.** [REVISED: SFA#8; was "(a) move to `rejected-ideas.json` (chosen)"]
  - (a) **Delete it from `defects/`; README :136-139 already records the drop (chosen).**
  - (b) Move it to `rejected-ideas.json`. Rejected: that file holds author ideas only (README :116-118, :206).
  - (c) Add an `"expect_reject"` field that `build.py` honours. Rejected: a new schema for one row.
  - (d) Keep it and accept a red `--check`. Rejected.
- **OQ-5 (engineering, canon): SF-5, what a note or lint warning does to `axon check`'s exit.**
  - (a) **Exit 0 unless an error-severity diagnostic was emitted (chosen).**
    - It matches the documented contract, `axon check --help` "0 = no errors, 2 = type errors" (main.rs:52-54).
    - It matches I0002's pinned severity ("must not be an error or a warning", `parse_help_probe.rs:502`), and how
      `check` already treats W0003/W0006/W1210.
    - The grader (`run_ab.py:276`, `PREREG-plainverify.md:35-36`) and cortex (`checker_exit_clean`) keep one rule,
      so no false-success window opens.
  - (b) Make I0002 an error with a fix-it. Rejected:
    - It reverses M5, which made the parser accept `let mut` (main.rs:8223-8228; `parse_help_probe.rs:490-506`).
    - It does nothing for W0007/W0008, which exit 2 the same way.
  - (c) Leave it and document "any diagnostic exits 2". Rejected: it contradicts `--help` and W0003/W0006.
  - [REVISED: SF56A#5; was "no documented contract changes"] It is not needs-human: the code moves to the `--help`
    contract (main.rs:52-54). The secondary statements of the old rule (`docs/CORTEX.md:642`, the runner comments) are
    rewritten in the same change, and the new CHANGELOG entry supersedes `CHANGELOG.md:91`. A re-run of the concluded
    plainverify study (`PREREG-plainverify.md:36`) would grade lint-only files as compiling; the frozen-set check in
    SF-5 found none.
  - One effect is visible to the receipt: a cortex body whose only diagnostic is a lint now verifies. The paid receipt
    the batch already runs measures it.
- **OQ-5b (engineering, canon): what keeps C14b's gate exercised once no real body is refused by check alone.**
  - (a) **A shim `axon_bin` that prints a note-severity line and exits 2 from `check` on a marked body (chosen).** It
    is deterministic, and M3851 kills the gate's neutralisation. [REVISED: SF56A#1] The line must be a note: an
    error line is reverted by the rollback observe (`runner.rs:1774-1777`, :815-832) before the gate.
  - (b) A real check-only error that `axon test` still runs. Rejected [REVISED: SF56A#1; was "Untested"]: after SF-5
    any real check-only refusal is error-severity (the E1203 ceiling is enforced only in `cmd_check`, main.rs:1534),
    so the rollback observe reverts it as `DidNotCompile` before the gate. After SF-5 the gate is unreachable with the
    real axon and is kept as defense in depth.
  - (c) Delete C14b. Rejected: `v022_refusal_coverage.py:1585` cites it as the gate's witness.
- **OQ-6 (engineering, canon): SF-6, the meaning of a shadowing fn's name in value position.**
  - (a) **The same precedence as a no-`&mut` call, `call_prefers_builtin(name, false)`, so it names the builtin, and
    the existing builtin-value refusal E0306 applies (chosen).**
    - It keeps every layer at one rule with no engine change.
    - It refuses only programs that already draw W0003 and either panic (`fnval`) or silently run the builtin
      (`b_val_same`).
    - The 290-file sweep has none.
  - (b) The value means the user fn (infer with the user sig; interp calls by entry; codegen via `__axon_user_<name>`).
    Rejected:
    - `let f = abs_i64; f(x)` and `abs_i64(x)` would reach different fns.
    - W0003's "this function will not be called" becomes false.
    - It needs three engine changes and new parity coverage.
  - (c) Make builtins first-class values. Rejected: per-builtin closures in both engines, against the stated design
    (resolver.rs:940-942).
  - (d) O-CS-03's direction (builtin precedence plus a W0003 at the reference). Folded into (a): a warning would
    leave `b_val_same` running the builtin silently and still let `fnval` panic.
- **OQ-6b (engineering, scope): calls through any fn value are not arity/type-checked** (`j_user_arity`,
  `k_lambda_arity`, `l_user_type`: check rc 0, run rc 101).
  - It is out of SF-6's scope, because no shadowing is involved.
  - Recommended: Main logs it as a new opportunity, which needs its own spec (checker call-through-local typing,
    `checker.rs:4012-4031`).
