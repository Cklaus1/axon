# Lessons — build-loop over `governance/reviews/2026-07-31-deep-review.md`

Mistake → rule. Appended only on a correction or failure, so it compounds.

---

## L001 — Never put a long-running job's input files in the session scratchpad

**Mistake:** Step 1's 9 triage agents read their finding batches from
`/tmp/claude-0/<project>/<session-id>/scratchpad/batches/*.md`. The session ended
mid-run; the scratchpad is keyed by session id and was cleared with it. On resume
the workflow relaunched with zero agents cached AND zero input files — the agents
would have read nothing and returned confident verdicts about an empty batch.

**Rule:** Any file a background/subagent job reads as input goes somewhere durable
(repo `.archive/`, or a committed path), never the session scratchpad. The
scratchpad is for outputs this turn consumes, nothing that has to survive a
process boundary. Corollary: after relaunching a resumed workflow, verify its
inputs still exist before assuming the resume is sound.

---

## L002 — A resumed workflow with 0 cached agents is a fresh run, not a resume

**Mistake:** Treated `resumeFromRunId` as if it guaranteed progress. Both stopped
jobs had `journal.jsonl` with zero `{"type":"result"}` lines — every agent was
mid-flight at teardown, so nothing was cached and the "resume" re-ran everything
from scratch at full cost.

**Rule:** Before relaunching, `grep -c '"type":"result"' journal.jsonl`. Zero means
budget for a full re-run and re-verify preconditions (inputs, working tree, branch)
as if starting cold. Non-zero means only the uncached tail re-runs.
---

## L003 — Background workflows do not survive `/compact`; a partial-build baseline is not a baseline

**Mistake (two parts, same root):**

1. Relaunched the 9-agent triage Workflow after it died with the session, then
   `/compact` ended the process and killed it *again*. Two full re-runs paid for,
   zero results. Workflows are process-bound; a long fan-out cannot straddle a
   compaction.
2. Recorded "2 baseline failures" from a run against a stale `target/`. The clean
   full-workspace run has **1**. Treating the partial number as the baseline would
   have permanently widened the gate — every later task would have been allowed to
   leave `wasm_browser_examples_run_identically_via_js_host` broken.

**Rule:** For work that must survive a process boundary, prefer many small
foreground/async `Agent` calls that persist each result to a durable path as it
finishes, over one long `Workflow` whose value only materialises at the end. Cost
of a kill then scales with one agent, not the whole fan-out.

And: a baseline is only valid from a **clean, complete** run. Never adopt gate
numbers from a partial or interrupted run — a too-loose gate is worse than no
gate, because it silently licenses regressions for the rest of the loop.
---

## L004 — `git stash pop` is not the inverse of `git stash push <pathspec>`

**Mistake:** Used `git stash push <one-file>` … `git stash pop` repeatedly to
verify fails-before. On one iteration the pop applied a DIFFERENT, pre-existing
stash (another session's 374-line WIP), leaving 34 conflict markers in
`crates/axon-vm/src/main.rs` — a file this run never touched — and reporting a
conflict on a file I had not stashed.

Nothing was lost: `pop` retains the entry on conflict, so the other session's
work stayed in `stash@{0}`, and restoring the file to HEAD undid only what my
own command had done.

**Rule:** `pop` always takes `stash@{0}`, which is NOT necessarily the entry you
just pushed — any repo with pre-existing stashes can hand you someone else's
work. For the fails-before check, don't use the stash at all:

```
git diff <file> > /tmp/fix.patch     # save
git apply -R /tmp/fix.patch          # revert, run the test, expect FAIL
git apply  /tmp/fix.patch            # restore
```

That is idempotent, names the exact change, and cannot collide with an unrelated
stash. Corollary: `git stash list` before any pop in a shared/long-lived repo,
and treat a conflict in a file you never edited as a signal to STOP and inspect
rather than resolve.

---

# Lessons — build-loop over `AXON_FOR_RLM.md` (2026-08-06)

## L010 — `git commit -m "…"` with backticks executes them

**Mistake:** Committed a Step-1 review message written as `git commit -m "…the
\`help\` field…"`. Bash ran `help` and `message` as commands inside the
double-quoted string and spliced its own builtin-help table into the middle of
the commit message. The commit succeeded, so nothing failed loudly — the damage
was only visible by reading the message back. The one visible signal was a
stray `bash: message: command not found` in the tool output, easily read as
noise from an unrelated step.

**Rule:** Never pass a prose commit message via `-m` in double quotes. Prose
about code contains backticks by nature. Always
`git commit -F - <<'MSGEOF' … MSGEOF` with the delimiter QUOTED, which disables
substitution entirely. And after any commit whose message was assembled by a
shell, read it back with `git log -1 --format=%B` before moving on — a mangled
message is silent.

---

## L011 — a test can encode the very divergence you are removing

**Mistake:** T-R2/T-R3 made `axon run` emit the same diagnostics as `axon check`.
The tier-1 full-suite gate then failed `parse_error_prefix_is_not_doubled`, which
asserted `run`'s output contains `parse error:`. That prefix came from
`AxonError::Parse`'s Display, on the path `run` used and `check` did not — so the
test was pinning one of the divergences the task existed to remove. Per-task
tests all passed; only the full suite saw it.

The trap is that the obvious readings are both wrong. "It's just a stale test,
update it" risks deleting a real regression signal. "It's a regression, restore
the prefix" would have restored the divergence and quietly undone the task.

**Rule:** When a pre-existing test fails after a unification change, separate the
test's *intent* from the *incidental behaviour it happens to observe*. Here the
intent was bug #7 — the prefix appearing TWICE — and that is still assertable and
still asserted. What changed is only where the error's class is carried: prose
`parse error:` became `code: E0000`, which is strictly more machine-readable.
Rewrite the assertion to the intent, and say in the test why the observation
moved. If you cannot state the intent separately from the observation, treat it
as a real regression, not a stale test.

## L012 — a fix hint is advice someone will act on, so test that it compiles

**Mistake:** Shipped a `parse_help` row telling the reader that `char_at(s, i)`
returns a `str` and to compare with `str_eq(c, " ")`. It returns the BYTE VALUE
as an `i64`. A model that followed the hint got a fresh type error — strictly
worse than no hint, because it spent the repair round going the wrong way.

Every test passed. They asserted which WORDS the help contained
(`contains("str_eq")`), never that the advice was true. It was caught only by
reading a program the model produced *after* being given the hint, in a
measurement run that happened to exist.

**Rule:** Help text is code, not prose. For any hint that recommends a concrete
construct, add a test that writes that construct and asserts it type-checks —
`the_character_literal_advice_actually_compiles`. A word-presence assertion
tests the wording; only compiling the recommendation tests the advice. And
verify a builtin's signature in `builtins.rs` before describing it, however
obvious the return type seems: I was most confident about the row I was most
wrong about.

## L013 — `git checkout <file>` to undo a mutation destroys uncommitted work

**Mistake:** Reverted a mutation with `git checkout <file>` while that same file
held a NEW, uncommitted test. The checkout restored HEAD, taking the test with
it. Nothing warned; the test count silently dropped from 11 to 10.

**Rule:** To undo a mutation, invert the exact edit (a scripted replace back, or
`git apply -R` of a saved patch — see L004), never `git checkout <file>`. The
mutation is a small known change; the file may contain large unknown ones. Same
family as L004: a coarse git command aimed at a fine-grained undo.

## L014 — rebuilding `axon` with `--no-default-features` poisons the shared test binary

**Mistake:** Throughout the loop I rebuilt the CLI with
`cargo build -p axon-core --no-default-features --bin axon` for fast smoke
checks. That writes `target/debug/axon` — the SAME path the parity harnesses
invoke. `scripts/fuzz_parity.sh` needs the codegen build, so it reported
`FAIL … native build failed` for ~20 cases and the full suite went red.

It read exactly like a regression from the 10-caller conversion I had just made,
and `axon build` was one of the converted call sites — the most plausible
possible culprit. It was not: `axon build` worked when run directly, and the
harness passed in isolation.

**Rule:** Before treating a `*_parity.sh` failure as a regression, check which
`axon` binary is on disk (`axon build` on a trivial program: if codegen is
missing it fails immediately). The standing "isolated rerun first" rule for this
repo's harnesses exists for flakes; this is a second, sharper reason for it —
the shared artifact path means a fast local build and the full suite are
fighting over one file. Prefer `cargo build -p axon-core --bin axon` (default
features) before any full-suite run, or accept that the run is measuring
whatever was built last.

---

# Lessons — the HIGH-tier loop (2026-08-06)

## L015 — an opportunity entry can be stale in the CLOSED direction

**Mistake:** Carried O026 ("the in-guest effect policy defaults to OPEN — a
fail-open capability boundary") into a spec as a live `needs-human` decision,
with options and a recommendation. It was already fixed: AUDIT T48
(`axon-vm/src/main.rs:1114`) makes the launcher refuse rather than emit a null
policy, and its comment records that the guest denies on ambiguity too. I would
have handed someone a decision they had already made.

The loop did read every source entry's **text** carefully — that is what caught
six "decision needed" markers the one-line summaries had lost. What it did not
do was check whether the **code** had moved under the entry since it was written.

**Rule:** A backlog entry states what was true when someone wrote it. Before
planning from one, re-verify the defect against current code — the same
verify-first rule already applied to done-claims, in the other direction. Cheap
test: grep for the specific mechanism the entry describes and confirm it still
behaves as described. Applied to the other five here, four were still live and
one was not, so the check is not ceremonial.

## L016 — a gate's own expectations can go stale when the behaviour is deliberately changed

**Observed, not a mistake this run:** `r34_acceptance_gate.sh` fails at HEAD
because its chain-stamp section expects a run that T48 *intentionally* made
refuse. The gate is not detecting a regression; it is asserting the pre-fix
behaviour and calling the fix a failure.

**Rule:** When a fix changes a refusal boundary, grep the gate scripts for the
old behaviour in the same commit. A test suite is searched for this
automatically; `scripts/*.sh` is not, and a gate that fails for a stale reason
trains everyone to ignore it — which is how a real failure gets missed later.

## L017 — a one-way agreement assertion misses the drift that actually happened

**Mistake:** Wrote a test pinning two check pipelines together as "if the
library carries help, the CLI must too". Mutation-testing it — re-introducing
the exact historical drift — **passed**. The drift ran the other way: the
LIBRARY dropped help the CLI had, so `help.is_some()` was false and the
assertion never fired.

Made bidirectional (`assert_eq!(lib_has, cli_has)`), it failed immediately and
exposed a SECOND, live drift nobody knew about: the checker pass was dropping
`expected`/`found`/`fix` too, not just the resolver pass U5 had fixed.

**Rule:** For any "these two must agree" test, assert equality in both
directions, never implication in one. An implication is satisfied whenever its
antecedent is false, which is exactly the state a regression puts you in. And
when a mutation survives, suspect the test's PREMISE before its assertions —
here the premise was a direction, and it was the wrong one.

## L018 — "fix the defect" can be a capability removal in disguise

**Mistake:** Took O031 (native codegen links the AI runtime unconditionally, so
`axon build` makes live calls `axon run` refuses) as a defect to fix, on a
"fix all" instruction, and implemented the refusal. Three existing tests then
failed — and reading them showed they assert that AI programs MUST build
natively, with a deliberately drawn boundary around which shapes are refused.

So the "fix" removed a supported, tested capability. Reverted.

**Rule:** Before fixing a divergence between two paths, check whether the
behaviour on either side is *asserted by a test*. A test asserting the current
behaviour is a contract, and changing it is a product decision however
defect-shaped the divergence looks. The tests are where the intent lives when
the opportunity entry only records the observation.

## R42 build loop

- **A review's "blast radius zero" is a claim about what it searched, not about the repo.** Step 1's
  review answered R42 Q1 by counting `str_slice` callers in `.ax` files and integration tests, and
  concluded no caller depends on the buggy behaviour. It missed a **Rust unit test in `axon-rt`** that
  asserted the bug verbatim ("str_slice unicode mid-codepoint must match", pinning `""`). Worse than a
  failing test: the new refusal is `process::exit(101)`, so that one test killed the whole 70-test
  binary after 20 tests. **Rule:** when a change alters a builtin's behaviour, grep the runtime crate's
  own unit tests too, not just the language-level corpus — and remember a process-exiting path cannot
  be asserted in-process, so its contract has to live at the subprocess/.ax level.
- **Check for an existing harness before writing a new one.** I wrote `utf8_boundary_parity.sh`
  without noticing `str_utf8_parity.sh` already existed. It turned out to be genuinely complementary
  (different builtins, and agreement-only vs expected-value), but I found that out *after* writing it.
  **Rule:** `ls scripts/*_parity.sh` before adding one, and if a near-neighbour exists, state in the
  header why both should exist.
- **`cmd | tail` masks the exit code — again.** Mutation-testing the new gate, I read `GATE_EXIT=0`
  from `./gate.sh | tail -5; echo $?` and nearly concluded the gate failed to fail. It had exited 1
  correctly; `tail` reported its own status. **Rule:** when the exit code IS the result, redirect to a
  file and check `$?` directly. (Second occurrence — already in memory as `tail-pipe masks exit code`.)
- **Never `{:?}` an arbitrary interpreter `Value` in an error message.** `dict_to_json`'s "value has no
  JSON form ({other:?})" **stack-overflowed** on a closure: a closure's Debug rendering walks its
  captured environment, which can contain the very dict being serialized. The error path crashed harder
  than the error it was reporting. **Rule:** diagnostics naming an arbitrary value use a short type TAG
  (`value_type_tag`), never a rendering. Found only because the fixture exercised the Err branch — a
  fixture covering just the happy path would have shipped it.
- **Do not edit or rebuild while a tier-gate suite is running — it invalidates the gate, and the
  failures look exactly like regressions.** The T7 gate reported
  `all_examples_native_match_interp_under_mock` and `codegen_fuzz_parity_finds_no_divergence` FAILED. I
  had backgrounded the suite and then kept working: editing `builtins.rs`, rebuilding
  `target/debug/axon` repeatedly, and at one point deliberately mutating base64 padding. Both harnesses
  shell out to the binary they find on disk, so they tested a moving target. Re-run in isolation: both
  PASS. **Rule:** a gate run is a barrier — either wait for it, or work only on files no harness reads
  (and remember every `*_parity.sh` reads `target/debug/axon`, so "no shared files" is almost never
  true). Third occurrence of the concurrent-build trap; the first two were `--no-default-features`
  poisoning the same binary.
- **A test that names the edge case it covers can still not exercise it.** `date.ax`'s
  `test_pre_epoch_round_trip` asserted a 1969 date to prove the floor-division branches mattered, and
  said so in a comment. Mutating both era branches to plain `/` left all 7 tests GREEN. The premise was
  wrong: for 1969 the shifted year `ys` is 1969 and the shifted day `z` is ~719303 — both POSITIVE, so
  truncation and flooring agree. The branches only diverge for years ≤ 0 in the proleptic calendar.
  **Rule:** when a test exists to cover a branch, pick the input from the branch's CONDITION (`ys < 0`),
  not from the domain concept you associate with it ("before the epoch"). Confirmed by re-mutating after
  the fix — now both branches fail the test. This is the "suspect the premise before the assertions"
  case, and mutation testing is the only thing that finds it.
- **A sub-parser must be required to consume its whole input, or it silently truncates.**
  `parse_fmt_inner_expr` handed a `{...}` slot's contents to `Parser::parse_expr` and returned the
  result without checking `pos == tokens.len()`. `parse_expr` stops at the first token it cannot
  continue with, so `"a{2,3}"` compiled to the string `a2` — the `,3` discarded with no diagnostic
  anywhere in the pipeline. This is the worst failure shape available: not a crash, not an error, a
  DIFFERENT correct-looking answer. It mattered because every counted regex repetition (`\d{2,4}`) is
  exactly this shape, so a pattern searched for something other than what was written. **Rule:** any
  time a parser is invoked on a substring — format slots, attribute arguments, embedded DSLs, prose
  lifting — assert the input was consumed in FULL. "It parsed" is not "it parsed all of it". Worth
  grepping for other `Parser::new(...)` sub-parses that never check the position afterwards.
- **A capability probe must exercise the capability, not something adjacent that is cheaper to check.**
  Three R42 tests skipped when `axon build --help` exited non-zero — but the `build` verb and its flags
  are registered by the arg parser regardless of the `codegen` feature, so `--help` ALWAYS succeeds. The
  skip never fired; the tests instead asserted `E0910` against a binary replying "requires building axon
  with the `codegen` feature", which reads exactly like a parity regression. Second and third-through-
  fifth instances of one class: `qemu_boot_test.sh` had already been bitten and its fix comment already
  explained why, while `zephyr_qemu_gate.sh`, `atomic_ir_test.sh` and `gdt_layout_ir_test.sh` still
  carried the stale form (the latter two harmlessly, having a real probe behind it). **Rule:** probe by
  attempting the operation and matching its specific refusal text. A flag's PRESENCE describes the CLI
  surface; only behavior describes the build. Corollary: when you fix an instance of a class, grep for
  siblings in the same commit — the fix comment sitting in one file taught nobody.
- **"That was just my wrong config" was itself the wrong conclusion — the config I picked by accident is
  the config the GATE uses.** I verified the interpolation fix with `cargo test --workspace
  --no-default-features`, three `native_refuses_*` tests failed, and I wrote it off as my own
  mis-invocation because the R42 baseline (1835/0) was measured with codegen ON. Then I read
  `scripts/gate.sh:68`: the gate's own test stage is `cargo test -p axon-core --no-default-features`,
  and `axon()` resolves `CARGO_BIN_EXE_axon`, which is built in the TEST's feature config. So those
  three tests had been failing **gate.sh** ever since T4/T7 landed, and my R42 end-of-run report —
  which cited `cargo test --workspace` and the individual parity harnesses — never covered that stage.
  The report was accurate about what it measured and silent about what it did not.
  **Rules, in order of importance:**
  1. Before dismissing a failure as an artefact of how you invoked the tests, check what the GATE
     invokes. "I ran it wrong" and "the gate runs it that way" are indistinguishable from the failure
     text alone, and only one of them is harmless.
  2. State the configuration next to every pass/fail figure, and enumerate the configurations you did
     NOT run. A green number in one config says nothing about another.
  3. When a suite passes in the config you chose, that is the moment to run the config you didn't.
- **The concurrent-build trap has a self-inflicted form: a test that rebuilds the binary its siblings
  probe.** Two parity tests shell out to scripts running `cargo build -p axon-core`, which overwrites
  `target/debug/axon` — the exact path `CARGO_BIN_EXE_axon` gives every other test in the same binary,
  running in parallel threads. Inside `cargo test --no-default-features` both failed with "native build
  failed"; both PASS in isolation. **Rule:** when a harness test builds a toolchain artifact, ask what
  else reads that path concurrently. And when verifying, follow `gate.sh`'s ORDER (codegen-less tests,
  then `cargo build -p axon-core`, then harnesses) rather than inventing an order — the sequence in a
  gate script is usually load-bearing, not incidental. Fourth occurrence of this class overall.
- **Check the error-code ledger BEFORE writing a spec's header, not after.** R43's draft reserved
  E2210–E2214 while R42 explicitly held E2205–E2212 — a three-code overlap, caught only because I
  grepped the specs directory afterwards. This repo has been bitten by exactly this before (R21/R22/R23
  dual-claimed under parallel development, and the Phase-6 effect codes collided with AI-policy
  E1300–E1302, forcing the E131x block). **Rule:** reserving a code range is a WRITE to shared state.
  Grep `governance/specs/` and `error.rs` for the range first, state in the header which ranges
  neighbours hold, and when you allocate one mid-session go back and mark it allocated in the spec that
  reserved it — a range recorded as "unallocated, held" that is silently in use is worse than no ledger.
- **A "determinism" feature can silently change what programs COMPUTE — check the obvious
  implementation isn't the wrong one.** The obvious virtual clock freezes time: `now_ms()` always
  returns the same value. `tests/fixtures/io_builtins.ax` does `t = now_ms(); sleep_ms(1); t2 =
  now_ms(); if t2 > t { 1 } else { 0 }` — under a frozen clock `t2 == t`, the program takes the other
  branch, and a feature sold as "makes runs reproducible" would have quietly altered results. The fix
  is a MONOTONIC virtual clock (advance by `tick` per read, and let `sleep_ms` advance the timeline
  rather than block), which is both deterministic and faithful — and makes replaying a run that slept
  ten seconds instant. **Rule:** before implementing a determinism/mocking control, grep for programs
  that OBSERVE the thing being controlled and check they still compute the same answers. Determinism is
  a constraint on the run, not a licence to change semantics.
- **When you virtualize a resource, find EVERY reader of it — the second reader is the dangerous one.**
  I virtualized the clock via the `now_ms` builtin and declared the replay hole closed. Three more
  builtins were reading the real clock directly through the crate's private helper: `temporal_now`,
  `temporal_new` (which stamps `created_ms`) and `temporal_is_valid`. The consequence was worse than the
  gap I set out to fix: a program mixing `now_ms()` with `temporal_*` observed TWO DISAGREEING
  TIMELINES — one virtual, one real — so a `created_ms` compared against a `now_ms()` was arbitrary,
  and nothing reported it. Found by grepping `now_ms()` in the builtin table (5 hits, only 2 of which
  I had touched), not by reasoning. **Rule:** after adding an interception point, grep for the
  underlying primitive across the whole crate and route every caller through the new single resolution
  point — then add a harness check that two different readers agree, because that is the property a
  future fifth reader would break. Corollary: a doc comment naming the one legitimate remaining caller
  (here, the provenance log's real timestamp) is what stops the next person "fixing" it wrongly.
- **Put a harness's cheap checks BEFORE its expensive/skippable ones.** `clock_parity.sh` ran its
  interp-only checks after the native-build section, whose codegen-absent path exits early — so on a
  codegen-less build only 3 of 7 checks ran and it still printed a clean skip. The gate looked fine
  while testing less than half of what it claimed. **Rule:** order harness sections by what can skip:
  unconditional checks first, toolchain-dependent ones last. And when a harness reports "N passed",
  compare N across configurations — a silently smaller N is the tell.
- **A positioning document is a technical artifact and must be reviewed against the code like one.** I
  wrote a paper arguing "loud failure beats quiet wrongness", and its first draft was quietly wrong in
  five ways: it misnamed the function carrying its central design claim (`classify_call` vs
  `classify_call_paths`); it asserted Python's `re.sub` silently drops a bad group reference when Python
  actually RAISES (so the competitor comparison was backwards, in the section about correctness); its
  flagship anecdote was an unsourced statistic that in-repo data partly contradicted; its coverage table
  omitted two LIVE holes in the very property it claimed was closed; and two of its three "gateable"
  metrics were not gateable — one ("count of known silent-wrong-answer paths") is satisfied by not
  looking. **Rules:** (1) every factual claim in a doc gets a file:line, and claims about OTHER
  languages get an actual execution, not a memory; (2) a metric that counts what you know is a measure
  of search effort, not of the property — prefer process metrics (oracle-test coverage, found→closed
  latency) that cannot be gamed by looking away; (3) when a doc claims a property is closed, enumerate
  the property's surface and check every member, because the draft's own table was where both live bugs
  were hiding. Two real code bugs were found by reviewing prose, which is a good argument for writing
  the prose.
- **"The seam already exists, just wrap it" — verify the seam is REACHABLE, not just present.** A
  review handed me a clean design: every environmental effect funnels through `AxonHost`, so one
  `RecordingHost` closes the whole column. The trait was real (15 methods, save/restore guard,
  `dir_list` already sorted for reproducibility) and the conclusion was still wrong in two ways I
  found only by running code. (1) `HOST` was a `thread_local!` while `interp::on_deep_stack` runs
  every program on a freshly-spawned thread — so an installed host was invisible to the program it
  was installed for, and the seam **had never worked for its actual use case**. A 15-line probe
  printed `REAL-HOST-ERR` instead of the fake host's answer. Nothing caught it because every existing
  test called `with_host` on the installing thread; the shape that matters, "install a host, then run
  a program", had no test. (2) `read_line` called `std::io::stdin()` directly from the builtin, so
  "every effect goes through one trait" was false by one member — and stdin is exactly the channel an
  interactive agent run depends on. **Rules:** for any "just wire it up" premise, write the smallest
  program that exercises the END-TO-END path before designing on top of it; and when a doc claims a
  seam is universal, grep for the underlying primitive (`stdin`, `std::fs`, `Command::new`) rather
  than trusting the trait's method list. Sixth instance of this class.
- **A test that HANGS on regression is worse than one that fails.** My first version of the
  seam-reaches-the-program test probed `read_line`. With the seam broken, `DefaultHost::read_line`
  blocks on the real stdin — so the test wedged a `cargo test` run for ten minutes and looked like a
  slow suite rather than a caught bug. I initially misread it as host contention. Rewrote it to probe
  `read_file` (missing path → immediate Err → clean failure in 0.00s) and moved stdin coverage to the
  gate script, which replays with stdin CLOSED and is therefore strictly stronger AND hang-free.
  **Rule:** when choosing what a regression test probes, ask what the BROKEN path does — if the
  fallback blocks, waits, or retries, pick a different probe. Verify by mutating the source and
  confirming the failure is fast and specific.
- **A diagnostic that is confidently WRONG costs more than one that says nothing.** Measured, not
  reasoned: for six runs the RLM benchmark's repair round gained exactly zero tasks (5/8 → 5/8).
  Two diagnostic fixes took it to 5/8 → 6/8 across three runs with zero spread, first-try unchanged —
  so the gain is the repair round, and the task that moved is precisely the one whose diagnostic was
  fixed. The old text for an `i64` argument where `str` was expected read "change the argument's type
  or cast with `as str` if compatible". There is no such cast. The model spent its single repair
  round following that advice into a dead end. Same for a shadowing warning whose help said "drop the
  `let`" when the shadowed name was a BUILTIN — assigning to `len` is not the repair, and I shipped
  that wrong advice myself before catching it on the very next run. **Rule:** when writing a `help`,
  check the suggested fix actually compiles for the case that triggers it; a generic
  "cast/convert/change the type" tail is where wrong advice hides. For an agent in a repair loop the
  cost of a wrong hint is a whole iteration, which is worse than silence — this is the
  silent-wrong-answer class wearing a different hat.
- **The warning path is a diagnostic path, and it was the quiet one.** AXON_FOR_RLM §2 fixed `run`
  emitting bare prose where `check` emitted located JSON. The identical defect survived in the
  WARNING path of every command: resolver warnings have a span and (now) a fix, and both were dropped
  by an `eprintln!("warning: [{code}] {message}")`. So the benchmark's most-hit diagnostic reached the
  model with no file, no line, and nothing to repair toward. **Rule:** when auditing an output
  channel, enumerate it by SEVERITY as well as by command — "errors are structured" is not "diagnostics
  are structured". Also: warnings could not simply be pushed into the existing diagnostic list,
  because a non-empty list means exit 2 and an `error:` prefix — so the fix has to respect severity,
  not just reuse the emitter.
- **For a model, a language card is not documentation — it is the complete map, so an omission reads
  as an absence.** The RLM card said "this is the whole surface; there is no import and no standard
  library beyond it" while naming 36 of Axon's 331 builtins. It omitted 13 of 25 `str_*` functions,
  including `str_chars` (whose own doc calls it "the load-bearing character function"), `str_char_at`
  (the i-th CHARACTER as a one-character string) and `str_reverse`. So the model reached for `char_at`
  — which returns a BYTE — and wrote code that could not typecheck, in every task needing
  per-character work. Correcting the list alone moved first-try 5/8 → 7/8. **Rules:** (1) never let a
  card or primer claim completeness unless it IS complete, because the reader cannot discover
  otherwise and will treat a gap as a missing feature; (2) gate it — a test that every name the card
  mentions exists in the real compiler (probe it, don't parse a table) catches the inverse error too.
  And I reached the wrong diagnosis first: I claimed "there is no ergonomic one-character `str`" and
  cited R42's admission test to argue against adding a builtin, when `str_char_at` had existed the
  whole time. I asserted an absence without grepping the builtin table — the seventh instance of
  [[verify-design-assumptions]], this time against my own analysis rather than someone else's.
- **When a card change moves a benchmark, test whether you leaked a task hint.** My first corrected
  card ended "…or to build a string up one character at a time" and scored 8/8; stripped back to pure
  type facts it scored 7/8. That clause was a usage hint aimed at the string-reversal task, so ~1 of
  the 8 was measuring my prompt, not the language. **Rule:** after any primer edit that improves a
  score, run the minimal variant that states only facts and compare — the delta between them is the
  hint you accidentally gave. Related and unresolved: an accurate card must list `str_reverse`, but
  one task IS "reverse a string", so card accuracy and benchmark discrimination genuinely conflict.
  Fix the task set, not the card.
- **`CLAUDE.md` is the agent's language card, and the card lesson applies to it directly.** Measured
  the same session: a language card asserting "this is the whole surface" while naming 36 of 331
  builtins cost 3 of 8 RLM tasks, because for a reader who cannot check, an omission is
  indistinguishable from an absence. `CLAUDE.md` is that artifact for an agent working in this repo —
  read once, acted on at speed, not independently re-derived. So it gets the same treatment:
  `scripts/claims_gate.sh` checks the mechanically-checkable claims (every `axon <verb>` exists, every
  documented `AXON_*` var is actually read, every path/script named exists, and the doc may not IMPLY
  completeness). It found real staleness on its first run — `codegen.rs` named three times after it
  became a directory — and every check is mutation-verified. **Rules:** (1) gate the direction that is
  unambiguously a bug (a name promised that the code lacks); leave the omission direction ungated
  because it is a judgement call about length, and neutralise it by refusing to imply completeness
  instead; (2) when a doc states paths relative to a documented root, resolve them against that root —
  my first version flagged the correct `codegen/mod.rs` as missing, which would have taught the next
  person to weaken the check.
- **TOTAL-IFY a needlessly partial function; never invent an answer to make one "total".** `to_str(s)`
  where `s` was already a `str` was E0102 — the single most common first error on the `tasks_hard` set
  (9 of 36 attempts). That is a program failing for a reason that is not a bug: "the string form of a
  string" has exactly one sensible answer, and `to_string` is total in essentially every language, so
  the restriction was an accident rather than a design. Widened at all four sites (interp, infer,
  checker, codegen) with native==interp verified through a fn boundary and inside interpolation.
  **The boundary that stops this becoming "accept everything":** `parse_int("abc")` must NOT be
  total-ified to `0` — there is no sensible integer, so returning one would be a silent wrong answer,
  the worst outcome. It is total in its TYPE (`Result<i64,str>`) rather than in its value. **Rule:** if
  the rejected inputs have exactly ONE obvious answer, widen the domain; if they do not, keep it
  partial and widen the RETURN type. Axon is already inconsistent here — `str_char_at` returns `""`
  out of range (total) while `char_at` returns `-1` (total via sentinel) — so a per-builtin audit of
  "partial for a real reason, or by accident?" is the systematic version of this fix.
- **For an AI-targeted language, prefer a repair GRADIENT over a lookup table of anticipated
  mistakes.** Tempting conclusion from the failure data: alias the Python spellings (`not`/`and`/`or`,
  `'x'`) so the model succeeds first try. Rejected as the LOWEST-value tier, for reasons that
  generalise: (1) aliases only cover mistakes already observed, while a diagnostic that names the fix
  works on mistakes nobody pre-registered — and 5/8→6/8 was measured coming entirely from the repair
  round, so the diagnostic already recovers most of the value; (2) the mass was elsewhere — 27 of 36
  failures were SEMANTIC (to_str 9, invented `type_of` 6, dropped Result 6, array-indexed-by-string 6)
  vs 6 for spellings; (3) `and`/`or`/`not` are VALID IDENTIFIERS today (`let and = 1` compiles), so
  promoting them is a breaking change; (4) every alias enlarges the language card for zero added power,
  and the card is the artifact measured at 3-of-8 tasks. **Sharpened rule:** alias only when the
  CANONICAL form is the accident, not when the model's habit is. `!` is defensible (C-family); `'x'` is
  borderline-yes only because `'` steals no identifier.
- **The DEFAULT path of a diagnostic is where the help text must work — and demoting a severity can
  silently delete it.** E0302 (unused `Result`) was changed to a warning by default with
  `AXON_STRICT=1` to promote it back to an error. The change looked complete and the behaviour was
  right, but the check-phase WARNING printer was `eprintln!("warning: [{code}] {msg}")` — it dropped
  the span AND the `fix`. So the help naming `let _ = call()`, which is the entire reason a
  strict-by-default policy can be relaxed safely, was being thrown away *precisely on the path that
  had just become the default*. Caught only because the test asserted on the help text rather than on
  the code. **Rule:** when changing a diagnostic's severity, re-verify its OUTPUT on the new path, not
  just its classification — a severity change moves the diagnostic to a different printer, and this
  repo has now had three printers that dropped structured fields (errors, resolver warnings, check
  warnings). Enumerate output channels by severity, not just by command.
- **A test that writes a relative path litters the repo, because `cargo test`'s CWD is the crate
  dir.** My E0302 fixture ran `write_file("./e0302_probe.txt", …)` and left two stray files in the
  working tree — noticed only when `git status` showed them right before a commit. **Rule:** any
  fixture that exercises a WRITE must write into `std::env::temp_dir()` and remove it afterwards;
  interpolate the absolute path into the generated source rather than using a relative one. A stray
  file is minor on its own, but it is the same failure as a harness that leaves state behind — the
  next run is no longer starting from where you think.

## A forged fixture can make a vacuous check look tested (C11/C12, cortex)

`Runner::snapshot` returned the literal `snapshot_id: "s1"`. The
`StaleSnapshot` refusal compared grant-id to current-id — and the two could
never differ, because every snapshot had the same id. The check was
decoration. My negative gate passed anyway, because it built the "stale"
grant by hand-writing `"s0"`, a value the production path cannot produce.

**Rule:** when a test needs a "wrong" version of some state, produce it
the way production produces it (take a second real snapshot after a real
edit), not by constructing the literal. A hand-forged input proves the
comparison executes; it says nothing about whether the two sides can ever
differ. The mutation `content_digest(...)` → `"s1"` now kills the gate,
which is the property that was missing.

Same family as [[fixture-stops-exercising-its-invariant]] and
[[mutate-one-side-of-a-differential]]: the assertion ran, the invariant
didn't.

## Fail-closed is not the same as observed-and-failed

`verify` used `out.map(|o| o.status.success()).unwrap_or(false)`. The
VERDICT was right — a check that cannot run must not pass. But the recorded
evidence said `FAILED` for both a real failing test run and a binary that
never launched, so a reviewer reading the episode could not tell "the repair
is wrong" from "the verifier is broken." Those call for opposite responses.

**Rule:** a fail-closed default settles the verdict, not the record. Where a
result is written down for someone else to act on, keep three outcomes —
passed / failed / did-not-run — even when two of them share a verdict.

## A gate that only fails in-suite is measuring the harness, not the code

`claims_gate` reported two failures under `cargo test --workspace` and passed
every time I ran it by hand. Both were false positives with environmental
causes:

- It warned that `RUSTUP_TOOLCHAIN` was shadowing the pin. But **cargo exports
  that variable into every process it spawns**, set to the pinned toolchain
  with the host triple appended — so the check fired on presence when it meant
  to fire on disagreement. It could never have been quiet in-suite.
- Its bare-filename fallback ran `find .` with `-not -path ./target/*`, which
  FILTERS but still DESCENDS into `target/` — a directory being rewritten
  continuously by the very suite the gate runs inside. The race made an
  existing file read as missing, in-suite only.

**Rule:** when a check disagrees with itself between a manual run and a suite
run, the difference is the finding. Don't re-run hoping for green, and don't
label it flaky — reproduce the suite's environment (here: one env var, and a
concurrently-written build dir) and the cause is usually immediate.

**And the corollary that paid:** tightening the search to real source roots
exposed a defect the loose one had been hiding. Both cited documents live in
`.archive/` — the old `find .` was satisfying the claim from copies sitting in
stale agent worktrees under `.claude/`. A check with too wide a search space
passes for reasons unrelated to the claim. Same family as
[[example-sweep-is-not-the-corpus]].

## A lesson fixed in one gate does not propagate to its siblings

`r28_acceptance_gate.sh` carries a comment recording that `grep -r "$check"`
"passed on the name appearing anywhere, including in a comment or an
`#[ignore]`d body" — and it fixed itself by running the suite and requiring
each named check to REPORT ok. Three sibling gates written from the same
template still used the loose `grep -rqs "fn $name"`.

**Rule:** when a gate's own comment records a defect it hit, grep for the
broken pattern across every sibling before considering it closed. The comment
is evidence the pattern was once wrong everywhere it appears, not just where
someone noticed.

## A guard that names a different binary than the command it protects

`acceptance_gate.sh` step 5 tested `[ -x target/debug/axon ]` and then ran
`target/debug/axon-os`. Both directions were wrong: with `axon` absent a
runnable check was skipped as "interpreter absent", and with a custom
`CARGO_TARGET_DIR` the hardcoded path did not exist, both runs failed, and the
`diff` that followed reported "records are NOT byte-identical (A5 violation)"
— a reproducibility failure that never happened, attributed to the wrong
cause. Related: [[no-default-features-fakes-codegen-failures]],
[[skip-must-prove-its-own-reason]].

**Rule:** a guard must test the exact path the protected command invokes, and
a command whose failure would be misread downstream must be checked for its
own failure first.

## Two types with the same machine layout will be confused, silently

Three native-codegen defects found in one sweep, all the same shape: a value
reached a `match` arm meant for a DIFFERENT type that happens to share its
representation.

- An array is `{ i64 len, ptr data }` — byte-identical in LLVM to a `str` — so
  array `==` was handed to `str_eq`, which compared `len` BYTES of the element
  data. `[1,2] == [1,3]` answered `true` (both start with the low bytes of the
  integer 1) where the interpreter answers `false`.
- A struct hit the same arm; `P{x,y}` would have `y` read as a POINTER.
- An enum hit it too — and was caught, because its LLVM type differs enough to
  FAIL IR VERIFICATION. Someone noticed and added an arm.

That last point is the lesson. The enum case was found because the wrong type
crashed the verifier; the array case shipped because the wrong type was
*plausible*. **A layout collision that verifies is more dangerous than one that
crashes**, and the crashing sibling is evidence the collision class exists.
When you fix a "wrong arm" bug, enumerate every other type with that same
machine layout before closing it.

Related: [[golden-ir-shape-vs-content-gap]], [[probe-element-size-must-differ]].

## The test that should have caught it was oracle-only and used an equal pair

`struct_and_array_equality` existed and passed throughout. It could not have
caught the array bug for two independent reasons: it runs only `axon run` (the
interpreter IS the oracle, so it can never see a native divergence), and its
only array case is `[1,2] == [1,2]` — an EQUAL pair, where the broken code is
right by accident.

**Rule:** a test named for a feature is not coverage of that feature. For an
equality primitive the minimum is a pair that differs, differing in a position
the suspected wrong implementation would not examine — here, anywhere but the
first element. A pair differing in the first element would ALSO have passed
against the broken codegen. Direct instance of
[[probe-element-size-must-differ]].

## Refusing the type is easier than refusing the case, and usually wrong

Native enum `==` compares tags, which is exact unless both sides are the same
payload-carrying variant. The easy fix — refuse `==` on any enum with a payload
variant — would have rejected `Op::Zero == Op::Zero` and `Op::Add{..} !=
Op::Zero`, both correct under tag-compare and both real code in
`examples/feature_tour.ax`.

**Rule:** when a lowering is exact for some inputs and not others, the guard
belongs on the CASE, not the type. Measure the real usage first — grepping the
repo took one command and found exactly two call sites, both of which the
blunt fix would have broken.

## Read the repo's own review archive before probing for defects

I found `i64::MIN / -1` silently wrapping by differential probing, wrote the
fix, and only then discovered
`governance/reviews/2026-07-31-deep-review.md` §349 had logged it — same
defect, same severity assessment, and a recommendation naming the exact
mechanism I had just used (`is_trap` → `emit_arith_guard`). An hour of probing
reproduced a document already in the repo.

The archive also holds `2026-08-01-triage/full-185.json`: 338 findings, 261
confirmed, with NO open/closed status field. So the backlog cannot be read as
a work queue without re-verifying each item — which is itself the highest-yield
task available, because most have been silently closed by the AUDIT T-series
and the few that remain are invisible among them.

**Rule:** before hunting, check `governance/reviews/` and grep the finding's
cited file:line. Probing is for what nobody has written down yet.

## A grep of a condition is not an observation of a behaviour

I reported §317 (zero-budget optimizer runs unbounded) as OPEN because
`goal.rs:137` reads `let unlimited = max_evals <= 0;`. The line says exactly
that. Running it says the opposite: a zero-budget principal gets exit 7 after
ZERO evaluations, because a guard upstream never lets 0 reach that line. I
checked two optimizer strategies in case the guard was strategy-specific.

Same mistake shape as [[ambient-controls-were-inert]] — which was the reverse
error, a documented control that nothing enforced — and the fix is the same
one: **diff outcomes, don't read conditions.** A reachable-looking line is not
a reached line.

Related: my first probe for this used `to_f64`, which does not exist
(`as_f64` does), and reported "0 evaluations" — the right answer for the wrong
reason. A broken probe that agrees with your hypothesis is the most dangerous
kind. [[prove-the-probe-reached-its-target]]
