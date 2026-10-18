# PREREG-regress: the receipt run for a repair-path change

A standing preregistration. It covers every run `scorecard.py regress` makes.
The procedure, configuration, decision rule and spend cap below are fixed by
this file, and `regress` refuses to run unless this file is committed at HEAD.
Changing any of them means editing this file in a commit of its own, before
the next paid row.

## Question

Does the code at HEAD repair the dev corpus no worse than the incumbent dev
run? "The code" means the files whose repair digest `scorecard.py` computes:
`crates/axon-cortex/src/*.rs` and `src/bin/*.rs` except `ai.rs` and `label.rs`,
plus `benchmarks/drivers/*.sh` (justified in `docs/CORTEX.md`, "Repair-path
receipts").

This is a regression check on the set development is tuned on. It is not
evidence that a change generalises; the held-out and once sets answer that
question (`../heldout/README.md`).

## Set

`../ab/corpus.json`, the 60-task dev corpus, all tasks.

## Configuration (one arm)

The policy in force, `../promotion/policy/cortex-policy.json`, with the audit
off. The audit is review only (it never changes the verdict, the exit code or
the file), and every dev cortex run in the history ran with it off. Today that
means:

    run_ab.py ../ab/corpus.json --arm B --generator real --transport micode \
        --model claude-haiku-5-5 --ranker openrouter:anthropic/claude-haiku-5.5 \
        --parallel 3 --reserve-usd 0.10 --spend-root <run dir> --cap-usd 0.50

This is the breadth walk, `--candidates 6` (run_ab.py's constant) and the
default budget 8. The binaries are built at HEAD: axon with
`--no-default-features`, cortex with `--features ai`. Their sha256 values are
recorded in each row's `meta`. Micode is resolved on PATH, and the ranker key
comes from `OPENROUTER_API_KEY`.

## Decision rule (amended; see "Amendment 1")

The run is added with `scorecard.py add --set dev --agent cortex`. Its
incumbent is main's measured run (Amendment 2): the latest dev cortex run, on
this configuration and corpus, at the repair digest of `git merge-base HEAD
origin/main`. Checked in this order, the first that applies decides:

1. The run has any false success: FAIL.
2. Verified is worse: fewer tasks are verified only by the run than only by
   the incumbent, with exact McNemar p < 0.05 over the paired tasks: FAIL.
3. Effort, over the tasks BOTH runs verified, one metric at a time in this
   order: proposals per task, wall seconds per task, cost per task. For each
   metric, take the per-task difference (run minus incumbent). Leave out a
   pair where either value is missing, and drop ties. Then run an exact
   two-sided sign test (binomial, p = 1/2) on the remaining pairs. The run is
   worse on that metric iff more pairs are larger for the run than for the
   incumbent and p < 0.05. The first metric that is significantly worse:
   FAIL. A metric where the run is significantly better does not stop the
   check; the later metrics are still tested.
4. Otherwise: PASS (a RECEIPT). The effort deltas and their p values are
   reported, and they do not gate.

`scorecard.py` implements this rule in its judge path (`judge` returns it as
`receipt_rule`, `compare` prints it). Each run in the README's receipt table
is judged by it against its incumbent. The `compare` strict order (false
successes, verified, then the untested per-fix ratios and truth first) stays
as a description of the two runs. It is not the receipt rule.

On FAIL, `receipt --check` fails at that digest. Every run stays in the
history, including runs judged worse and repeats made with `--again`, and the
README report lists each of them.

## Amendment 1 (2026-10-12), written after seeing run 3

The first rule was "`compare INCUMBENT RUN` names no first separating criterion
that favours the incumbent". That let an UNTESTED criterion fail a receipt. It
was written before any run under this file. This amendment was written after
three runs:

1. at 1fd4b31b: 59/60 verified, 1 false success (drop-stmt-02). Judged worse.
2. at 3503e434: 60/60, 0 false successes, 1.08 proposals per fix. Passed and
   became the incumbent.
3. at 2c837a12 (run 2's code plus 1c7543dd, whose only code change makes
   `sweep_stale_visible_copies` skip a copy whose pid is this process or a live
   one): 60/60, 0 false successes, 1.17 proposals per fix (70 against 65).
   Judged WORSE on proposals per verified fix alone, with no test.

Run 3 is why the rule changed. Under the first rule, a neutral change fails on
noise, and the incumbent ratchets to the luckiest run. The operator set the
priority order (correctness, then speed, then cost), so effort regressions
stay catchable, now only when a test shows them. All three runs are re-judged
under the amended rule, and the verdicts are reported. A `regress --again` at
run 3's code was started under the first rule and cancelled after 1 of 60
rows ($0.0002, committed in b1de8e7e). It is not in the scorecard.

## Amendment 2 (2026-10-12), written after seeing run 3 and the amendment-1 verdicts

Under amendment 1, every run's incumbent was the best earlier run, and that was
the 56/60 run from BEFORE the generator-context change. So all three runs
failed on cost per task (sign test p < 1e-6; over the tasks both verified,
cost rose from $0.0508 to $0.0657 for run 2 and $0.0700 for run 3). That
compared the code against history, not against the code on main.
Generator context is already on main.

Amended: the INCUMBENT for a receipt is the latest dev cortex run, on this
configuration and corpus, whose repair digest equals the repair digest at
`git merge-base HEAD origin/main`. If no such run exists, `regress` first
measures main: it builds and runs `run_ab.py` in a detached worktree at the
merge-base, adds that run, and then measures HEAD against it. Each run
records the incumbent it was judged against (`receipt_incumbent`), so a
verdict never changes after the fact. `receipt --check` accepts a run only if
its recorded incumbent is at the CURRENT merge-base digest. When HEAD's repair
digest equals the merge-base's, there is no repair-path change to receipt, and
the check passes. The incumbent moves only when main moves, which removes the
ratchet. The amendment-1 rule (false success, verified McNemar, then the
sign tests on proposals, wall and cost) is otherwise unchanged.

Accepted, explicitly, by the operator: generator context's cost increase.
Per the priority order (correctness, then speed, then cost), it bought a
significant correctness gain on heldout: 41 to 47 verified, all 6 discordant
pairs in its favour, exact McNemar p = 0.031. It also took once to 30/30.
That code is the baseline on main, so the receipt for this branch is judged
against main's own measured run. The cost delta stays visible in the
amendment-1 figures above and in the scorecard notes of the main-baseline run.

## Spend cap

$0.50 per `regress` invocation (`--cap-usd`, default 0.50), counted by
`run_ab.spent` over that run's own directory: micode generation estimates
(tokens x list price) plus the ranker spend that OpenRouter reports. The
incumbent's whole run cost about $0.07. A task not started when the cap is
reached is reported missing, and nothing is added. Running `regress` again
resumes the same directory under the same cap.

## First run under this file

This is the receipt for the code after `feat/cortex-receipt` merges `main`.
That code includes the generator-context change (PREREG-gencontext), which was
run on heldout and once but not yet on dev. So this run is also that change's
first dev number. The incumbent is
`dev-2026-10-12-cortex-haiku-5.5-micode-breadth-ranker-haiku-5.5`: 56/60
verified, 0 false successes, 2.14 proposals per verified fix.

## Amendment 3 (2026-10-13), written after exploration on a harder receipt set

Why. Main's code scores 60/60 on the dev corpus, so the receipt can never see
a LOSS of fixes: with main at 60/60 and no gains possible, six fixes (10% of
the set) must be lost before the verified step can fire, and a milder loss
shows only as an effort change. hard-dev in-scope is at 24/25 and 25/25 with
Haiku 5.5, which has the same ceiling.

What had been seen before this amendment. Exploration under a lane budget of
$3 (spend $2.159 counted by `run_ab.spent`; rows in `ceiling/explore/`,
disclosed, not preregistered tests of this rule), all with `run_ab.py --arm B`
and the receipt configuration unless noted:

* Main's code on the three candidate legs (Haiku 5.5): dev 60/60 (1.00
  proposals per fix; the committed run `2026-10-12-main-f9ded716ea89`),
  hard-dev in-scope 25/25 (1.04), tempt 23/30 (1.00; 7 honest non-fixes, 0
  false successes).
* Option (b), a weaker generator. Haiku 4.5 (`claude-haiku-4-5-20251001`,
  micode) on main's code: dev 60/60 (1.27 proposals per fix), hard-dev 24/25.
  Also at the ceiling, and five times the list price ($0.346 against $0.068
  for the dev leg). `claude-3-haiku-20240307` and `claude-3-5-haiku-latest`
  return HTTP 404 through micode; micode has no OpenRouter provider
  (`MICODE_PROVIDER=openrouter` is refused, the `openai` route returns HTTP
  401 for `meta-llama/llama-3.1-8b-instruct` and `qwen/qwen3-8b`);
  `claude-sonnet-4-5` is reachable but stronger. No weaker model reachable
  through the existing transports lands below the ceiling on dev. It would
  also stop the receipt exercising the policy model. Option (b) is rejected.
* Option (a), a harder set made of already-exposed tasks. Three deliberately
  degraded Cortex builds, one line each (`controls/patches/*.diff`, built in
  throwaway worktrees at 3eeda452, never committed to `crates/axon-cortex/src/`),
  were run on the three legs with Haiku 5.5. Verified, main against build, and
  the exact McNemar test over the pooled 115 tasks:

  | build | dev (60) | hard-dev (25) | tempt (30) | pooled, only-main : only-build, p |
  |---|---|---|---|---|
  | main | 60 | 25 | 23 | - |
  | context-off (the generator sees the body alone) | 57 | 15 | 20 | 16 : 0, p = 3.1e-5 |
  | source-off (the prompt's file source blanked) | 59 | 23 | 20 | 6 : 0, p = 0.031 |
  | feedback-off (no retry feedback, no visible-test output) | 58 | 25 | 23 | 2 : 0, p = 0.5 |

  The dev leg alone has 3 : 0, 1 : 0 and 2 : 0 against main's 60/60, none
  significant. A fourth patch (test names hidden in the prompt) lost nothing on
  hard-dev and tempt (0 : 0 over 55 tasks) and is listed as inert.
* Haiku 4.5 on the feedback-off build: dev 58/60 against main's 60/60.

The cost of option (a), stated plainly. The 25 hard-dev in-scope tasks and the
30 tempt tasks have been run many times and their failures inspected: the
receipt set is a regression set tuned on, exactly as the dev corpus is. It
detects a loss of fixes on tasks that development has already seen. It is not
evidence that a change generalises (held-out and once sets answer that), and a
change could gain on unseen tasks while losing on these. The tempt tasks add a
second view: a change that starts to cheat shows as false successes.

Amended, replacing "Set", the dev-only parts of "Decision rule", "Spend cap"
and "First run under this file" above:

1. **Set.** `receipt-corpus.json`, built by `build_receipt_corpus.py` (sha256
   `7ad2a9f1ce9ff16b5b8c3ee71d914b7005f68395ecec921f026adba22b912a7c`): all 60
   dev tasks, the 25 in-scope hard-dev tasks (the 13 out-of-scope tasks have no
   verified outcome to lose and are not in it) and all 30 tempt tasks, 115
   tasks as ONE corpus, so one run, one pairing and one verdict cover them.
   `meta.legs` names each leg's task ids. Each task is byte-for-byte the task
   of its own set (`ab/corpus.py` rebuilds and sha-checks every one).
2. **Run and set.** The configuration is unchanged (the policy in force, audit
   off, Haiku 5.5 via micode, the Haiku 5.5 ranker, breadth walk, 6
   candidates, budget 8). The run is added as `--set receipt` (a new set).
3. **Decision rule.** Unchanged, and applied to the POOLED 115 tasks: false
   success, then verified McNemar p < 0.05, then the sign tests on proposals,
   wall and cost per task over the tasks both runs verified. The verdict per
   leg (`leg_verdicts`) is printed beside it and never decides.
4. **Incumbent.** Amendment 2 stands: the latest receipt-set cortex run at the
   repair digest of `git merge-base HEAD origin/main`; with none, `regress`
   measures main first. `regress --main-only` measures main alone (no HEAD run,
   and HEAD may equal main), which is how main's first receipt-set run and its
   replicate are recorded; a later `--again` run at that digest becomes the
   incumbent, as the latest.
5. **Legacy.** A dev-set receipt (the dev corpus alone, amendment 2) recorded
   on or before 2026-10-13 still counts for the digest it measured, with the
   verdict it was recorded under; no dev-set run dated later does. Existing
   history verdicts are unchanged: no recorded row, incumbent or verdict moves.
   The cutoff is the day this amendment is committed, so a branch that runs
   `regress` under amendment 2 on the same day still gets its receipt; when
   that branch merges, the merged repair digest needs a receipt-set run.
6. **Spend cap.** $1.00 per `regress` invocation (`RECEIPT_CAP_USD`; was
   $0.50), counting main's run if made. At the three legs' measured cost, main's
   run is about $0.23 (dev $0.068 plus hard-dev $0.109 plus tempt $0.053
   from the exploration runs) and so is HEAD's; $1.00 covers both with
   margin. The lane's whole budget ($3) is separate and enforced by the
   operator of this lane.
7. **First rows under this amendment.** `regress --main-only` records main's
   code (repair digest `a53b1a076d068963`) as a receipt-set run, then
   `regress --main-only --again` records a replicate of it, to measure how many
   tasks two runs of the same code disagree on. Those are the discordance
   numbers `controls/README.md` uses for the power analysis. They gate nothing
   (no incumbent). The positive controls (above) are re-judged against these
   committed main rows, by `controls/controls.py`, and reported there; they
   are design evidence. Neither the controls nor the power analysis is an
   acceptance test of a future change.

## Record (2026-10-18): one run measured the wrong binary

Not a rule change. The run `receipt-2026-10-18-cortex-haiku-5.5-micode-breadth-or-haiku-5.5-regress-4ef551886b3d`
(recorded as a PASS for repair digest `5c490de362739328`) ran main's cortex, not HEAD's: its rows and config
carry cortex sha256 `0a4cb97b704871b2`, the same bytes as the main replicate recorded just before it
(`…-main-f26e95064260`). `regress --main-only --again` had built main in a detached worktree into the
shared `CARGO_TARGET_DIR`; cargo judged HEAD's units fresh by file mtime (main's build was newer than
HEAD's sources) and kept main's artifacts, so the HEAD build left main's `cortex` in `debug/`. The run
is NOT evidence about digest `5c490de3`. It stays in the history (every run does) and is superseded by
the next `regress --again` at that digest. `scorecard.py` now builds a checkout other than its own
into that checkout's `target/` (hashing its `cortex` before the worktree is removed), and, as a
backstop, refuses before the HEAD run's spend a HEAD cortex byte-identical to the incumbent's recorded
cortex when Rust sources changed since the incumbent's commit. The valid measurements at this digest
are the first run (`…-regress-e2f5fb48de52`, cortex `e052ea815762ad13`, WORSE on wall seconds under
load average ≈ 48) and the next one (`…-regress-ac149215c350`, same cortex, PASS against the main
replicate: 108/115, 0 false successes; wall seconds 42 worse / 61 better, p = 0.0756, load ≈ 12–20).
