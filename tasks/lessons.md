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
