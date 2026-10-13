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

## Decision rule (fixed now)

The run is added with `scorecard.py add --set dev --agent cortex`. Its
incumbent is the latest earlier dev cortex run on this configuration and corpus
that was not itself judged worse. The run gets a RECEIPT iff:

1. it has 0 false successes, and
2. `compare INCUMBENT RUN` names no first separating criterion that favours
   the incumbent. The criteria come in the scorecard's strict order: false
   successes, verified (exact McNemar, p < 0.05), proposals per verified fix,
   cost per verified fix, wall seconds per verified fix, truth first.

Otherwise the change is judged WORSE and `receipt --check` fails at that
digest. Every run stays in the history, including runs judged worse and
repeats made with `--again`, and the README report lists each of them.

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
