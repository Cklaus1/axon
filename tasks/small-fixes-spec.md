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
- There is no dependency on checker-soundness or capability-surface. Nothing here touches `crates/axon-core/src`, so
  no pci theme json and no `pci-delta` row line are needed.

## Priority tiers

| Tier | Item | Digest | Source |
|---|---|---|---|
| P1 | SF-1 generator drain: a writer still writing after the generator exited is not its answer | **changes** (`src/generate.rs`) | O-FU-01 |
| P2 | SF-2 lexer-aware `defined_fns` / `test_fns` / `called_in` | **changes** (`src/locate.rs`) | O-FU-02 |
| P3 | SF-4 `hardset/build.py` exit status, argument check, `--check` default, gate step | neutral (`benchmarks/hardset/`) | O-S1-21 |
| P3 | SF-3 conformance `stage()` cleans up its workspace | neutral (`tests/`) | O-FU-07 |

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
- SF-3 and SF-4 are digest-neutral.

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
| full `./scripts/gate.sh` | 32–39 min (last 38m51s, 6967/0/12) | shared host, all cores | any commit after it | none; `--fast` is not evidence | merge |

No pci theme json / `pci_delta.py` work is needed: there is no `crates/axon-core/src` change.

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
- **OQ-4 (engineering, canon): `misc.json#6`.** [REVISED: SFA#8; was "(a) move to `rejected-ideas.json` (chosen)"]
  - (a) **Delete it from `defects/`; README :136-139 already records the drop (chosen).**
  - (b) Move it to `rejected-ideas.json`. Rejected: that file holds author ideas only (README :116-118, :206).
  - (c) Add an `"expect_reject"` field that `build.py` honours. Rejected: a new schema for one row.
  - (d) Keep it and accept a red `--check`. Rejected.
