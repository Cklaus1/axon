# PREREG-gencontext: does Cortex with generator context match the plain agent?

Written and committed BEFORE any paid trial of this change. The git history of
this file and of `results/gencontext/` is the evidence of that order. Anything
decided after the first paid trial is a deviation, reported in
`results/gencontext/README.md`, never edited in here.

## Background

On `heldout.json` (`PREREG-heldout.md`, `results/README.md`) the plain micode
agent verified 47/47 and both Cortex arms 41/47 (paired exact McNemar
p = 0.031), same model. All 6 misses were generation failures: the true
function was attempted, usually first. Prompt capture on h-wrong-var-03 and
h-constant-03 showed why: the generator saw only the function BODY - no
signature, no type definitions, no visible test, and on a rejection only "did
not fix the problem".

The change under test (commit `ef58330b`): the generator is shown the ranker's
view (`rank::build_view`) - the file with the hidden check cut out (signature,
types, siblings, comments), the visible failing tests' source and what they
print now - and on each rejection the visible tests' output (`CheckRefused`) or
the compiler's diagnostics (`DidNotCompile`), each gathered by running the file
with the hidden check cut out, bounded to 2 KB. The hidden check's name, source
and output are never shown. When the view cannot be built (leak guard, no
visible failing check), the generator gets the old body-only prompt and the run
says so (`generator_context` in `--json`, a line on stderr).

## Question

Primary: on `once.json` (30 planted tasks, never run with a model by anyone),
does Cortex with the new generator context match the plain micode agent?

## Set

`once.json`, sha256
`3c66ddd64c0af066df6af0cccba11e8b03c542eb80141ff85070490d4d3b995b`, frozen.
Spectrum rank of the truth: 1 for 22 tasks, 2-4 for 8 (`README.md`).

Note on `README.md` rule 4 (once is run per release candidate that already won
on dev and heldout): this candidate has NOT yet been shown to win on heldout.
The once run is made now because the operator chose it, to measure the change
on tasks no prompt work has seen. Its number is reported, not optimised: no
generator, prompt or ranker change is chosen on it.

## Arms (same model, same binaries)

Generator / agent `claude-haiku-5-5` via micode 0.3.0 (`ANTHROPIC_BASE_URL`
unset). Cortex built `--features ai` from this branch, axon built
`--no-default-features` from this branch (sha256 of both recorded in each
row's `meta`); one cortex binary for both B arms. Breadth-first walk,
`--candidates 6`, default budget 8, `CORTEX_AUDIT=off`.

| arm | command (`../ab/run_ab.py once.json ...`) | rows |
|---|---|---|
| A plain agent | `--arm A --agent real --transport micode --model claude-haiku-5-5` | `results/gencontext/once/a.jsonl` |
| B Cortex, spectrum order (the default config) | `--arm B --generator real --transport micode --model claude-haiku-5-5 --ranker spectrum` | `results/gencontext/once/b-spectrum.jsonl` |
| B' Cortex + Haiku 5.5 ranker | same as B, `--ranker openrouter:anthropic/claude-haiku-5.5` | `results/gencontext/once/b-ranker.jsonl` |

Every invocation also passes `--reserve-usd 0.10 --spend-root
results/gencontext --cap-usd 2`. The three arms run at the same time with
`--parallel 1` each (at most 3 tasks in flight on the shared machine). A
ranker call that fails falls back to spectrum order visibly; such rows are
kept and counted, not re-run. A task that crashes the harness is reported
missing, not re-run.

## Sequence

1. Dry run, zero cost: B `--generator oracle --ranker spectrum` on `once.json`
   with these binaries (`results/gencontext/dryrun/`); must verify 30/30 with
   0 false successes. If it does not, stop: no paid run.
2. EXPLORATORY, run first and doubling as the field and spend check: B
   (spectrum) on `heldout.json`, 47 tasks
   (`results/gencontext/heldout-exploratory/b-spectrum.jsonl`), compared with
   the old 41/47. Labelled exploratory because that set and two of its
   transcripts were inspected to diagnose the failure. Its result changes
   nothing that follows: if it surfaces a HARNESS defect (a row that cannot
   be read, a cost that cannot be priced), only the harness is fixed and the
   fix is reported as a deviation; the generator and prompt are not changed.
3. Primary: A, B and B' on `once.json`, 30 tasks each.

No once-set pilot: every once task is spent the first time a model sees it.

## Metrics (recomputed from rows by `../scorecard/scorecard.py`)

Per arm: verified (`verified_clean`), false successes, proposals per verified
fix (B, B'), wall time (sum of per-task `wall_s`), generator model time (sum
`api_ms`, B arms), estimated cost (micode tokens x list price) plus
OpenRouter-reported ranker spend, generator-context fallbacks (B arms: tasks
where any candidate's `generator_context.used` is `body_only`). Paired on task
id: both / only one / neither, exact two-sided McNemar on verified.

## Decision rule (ordered; fixed now)

For each of B and B' against A, in this order:

1. **False successes must be 0.** Any false success in a Cortex arm: that arm
   does NOT match, whatever else holds.
2. **Verified.** The arm MATCHES the plain agent iff BOTH:
   * it is not significantly worse: NOT (arm verified < A verified AND exact
     two-sided McNemar p < 0.05), AND
   * it is within 2 tasks: A verified - arm verified <= 2.

   Otherwise it does NOT match.
3. **Effort, reported, not decisive:** proposals per verified fix and total
   wall time (arm vs A), then cost per verified fix incl. ranker.

The headline verdict is B's (the default configuration); B' is reported by
the same rule. If the plain agent still wins, the report names the tasks it
fixed that Cortex did not and the captured reason from the rows, and nothing
is tuned on the once set.

## Spend cap

$2.00 total across every paid run under this prereg (heldout exploratory and
all once arms): `run_ab.spent` over `results/gencontext/` (`--spend-root
results/gencontext --cap-usd 2`) counts micode generation estimates plus
OpenRouter ranker spend. Micode dollars are estimates (OAuth, no bill). Tasks
not started at the cap are reported missing.

## Scorecard

Each paid run is added with `../scorecard/scorecard.py add --set once|heldout`
after its rows are committed, `--config-json` carrying the commit recorded in
the rows' `meta`, then `scorecard.py report` regenerates `README.md` and
`progress.svg`.
