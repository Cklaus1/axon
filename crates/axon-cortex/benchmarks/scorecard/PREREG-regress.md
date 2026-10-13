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
