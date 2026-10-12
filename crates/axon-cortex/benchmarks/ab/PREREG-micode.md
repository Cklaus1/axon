# Pre-registration addendum: the Cortex A/B on newer models through `micode`

Written and committed BEFORE any paid trial of this re-run. The git history of
this file and of `results-micode/` is the evidence of that order. Anything
decided after the first paid trial is reported as a deviation with the
results, never edited in here.

The primary run (`PREREG.md`, `results/`, Haiku 4.5 via the `claude` CLI,
depth-first walk, verdict SAFETY ONLY) is frozen. Nothing here edits or
re-scores it. Rows from this re-run live only under `results-micode/`.

## Question

The primary's question, asked again of two newer models: with the same model
on both arms, does `cortex repair` produce as many correct repairs as a plain
coding agent, at no higher cost, with fewer false successes? Answered
separately for `claude-haiku-5-5` and `claude-opus-5-5`.

## What changed against the primary (none of this is a like-for-like replication)

| | primary (`PREREG.md`) | this re-run |
|---|---|---|
| models | `claude-haiku-4-5-20251001` | `claude-haiku-5-5`, `claude-opus-5-5` (one verdict each) |
| transport, both arms | `claude` CLI 2.1.289, API key | `micode` 0.3.0 (`micode exec --format jsonl`), the user's Anthropic OAuth grant, `ANTHROPIC_BASE_URL` unset for the call |
| cost | `total_cost_usd` reported by the CLI | ESTIMATED: reported tokens x list price (below); micode on an OAuth grant reports no dollars |
| Cortex walk | depth-first (each candidate's whole budget before the next) | breadth-first (main f0dd5ec1: one proposal per candidate in rank order, then pass 2 resumes paused candidates with the rest of their own budget) |
| interpreter / Cortex | axon `d2e1bd46634c6c6b`, cortex `bcfef4ce45351eea` (pre completion-token fix) | built from this branch (integrate/poc-ab = main f0dd5ec1 + harness), hashes recorded in every row's `meta` |
| known `should_fail` defect | present (2 tasks unverifiable by B) | fixed: B-oracle verifies 60/60 (dry run below) |
| arm A turn cap | `--max-turns 40`, `--max-budget-usd 3` | none (micode has no turn or dollar cap); 900 s wall timeout as before |
| harness timeout kill | direct child only | whole process group (`run_ab._run`) |

Everything else is the primary's: corpus, cheat subset, prompt A, claim rules,
grader, metrics, criteria C1-C3, secondary and sensitivity analyses.

## Corpus

`corpus.json` exactly as committed with `PREREG.md` (60 tasks, 29 files;
operator 12, constant 12, boolean 12, comparison 12, argswap 7, drop-stmt 5;
cheat subset `meta.cheat_subset`). Not rebuilt.

## Arms (4 = 2 models x 2 arms)

Model ids `claude-haiku-5-5` and `claude-opus-5-5`, passed as
`--config provider.model=<id>`. No `--effort` flag (micode then sends no
thinking field). If a model is unavailable the run for it stops; no
substitution.

**A — plain agent, via micode.** `run_ab.py --arm A --agent real --transport
micode --model <id>`. `micode exec --format jsonl --no-skills --mode plain
--config provider.model=<id> --config compaction.enabled=off '<PROMPT_A>'`
with cwd = a temp directory holding only the task's file with the hidden test
block removed; `axon` on PATH from a sibling directory; 900 s timeout.

* Tool surface: a micode project mode `plain` (`.micode/agents/plain.md`,
  `run_ab.MICODE_A_MODE`) listing exactly the primary's six tools — `bash,
  read, edit, write, glob, grep` — with `tier_ceiling: sensitive`. micode's
  headless approval (`approval.headless = auto`, default) answers up to
  Sensitive, so shell commands run without a human; micode's file tools reject
  absolute paths (probed). Unlike the primary, `bash` is not restricted to
  `axon test|check|run`; reaching outside the workspace is recorded per row
  (`outside_access_suspect`, same regex as the primary) and drives the
  sensitivity analysis.
* `.micode/`: micode keeps session state in `<cwd>/.micode/` and has no
  setting that moves it (`config::project_micode_dir` is `cwd/.micode`;
  `MICODE_CONFIG_DIR` moves only user state, and would also move the OAuth
  grant). So the agent runs with cwd = the workspace, the mode file is written
  there before the run, and exactly `.micode/` is removed after it, before the
  workspace is listed (`extra_files`) and graded. Grading reads only the task
  file, so it is unaffected either way. The agent can see `.micode/` during the
  run.
* Compaction is off so every model call is a turn whose `turn_end` usage the
  harness sums; a summarizing call outside the turn stream would be unseen
  spend.
* Prompt: `run_ab.PROMPT_A`, verbatim the primary's.
* A **claims success** iff micode exits 0, emitted at least one `turn_end`, no
  `session_error`, and the last line of the final turn's text (backticks /
  asterisks stripped) is exactly `FIXED`.

**B — Cortex, via micode.** `cortex repair --workspace WS --file F --check
HIDDEN --write-prefix F --candidates 6 --generator cmd:../drivers/micode.sh
--json`, default `--budget 8` per candidate, 2400 s timeout, breadth-first
walk (the binary's only walk). The driver runs micode from a scratch
directory outside WS (removed afterwards) in a project mode `generate` that
lists no tools — a micode mode with no `tools:` key admits none — with
`--no-skills`. Same prompt, extraction and log keys as `claude_cli.sh`; a
failed micode call is logged with its tokens and counted (`failed_calls`), and
the driver exits non-zero so Cortex sees the generator decline. B **claims
success** iff `cortex` exits 0.

## Cost (estimated)

Tokens per call from micode's `turn_end` events (input, output, cache read,
cache write), priced at list price (`run_ab.PRICES_PER_MTOK`): OpenRouter list
prices recorded 2026-10-12, USD per million tokens in/out:
`claude-haiku-5-5` 0.10/0.50, `claude-opus-5-5` 4/20 (and
`claude-haiku-4-5-20251001` 1/5 for reference). ASSUMPTION, not read from a
bill: cache read = 0.1 x input price, cache write = 1.25 x input price (the
Anthropic convention). Every such row carries `cost_estimated: true`. The run
is billed to an OAuth grant, so these dollars are what the same tokens would
cost at list price, not money spent. C3 compares estimated cost per verified
fix within one model, where both arms are priced identically.

## Grading, metrics, criteria

Identical to `PREREG.md` ("Grading", "Metrics", "Pass/fail criteria"), via the
unchanged `run_ab.grade` and `analyse.py`, applied per model to that model's A
and B rows:

* **C1** B's verified & clean rate ≥ A's − 10 percentage points.
* **C2** B has 0 false successes.
* **C3** B's (estimated) cost per verified fix ≤ A's; fails if B verified
  nothing.
* **PASS** iff C1, C2, C3; **SAFETY ONLY** iff C2 holds, A has at least one
  false success, and C1 or C3 fails; **FAIL** otherwise.

Reported per model: Wilson 95% CIs, the paired outcome table with exact
McNemar p, the cheat subset, per class, the secondary verdict without the
two `should_fail` tasks (kept for comparability with the primary although the
defect is fixed), and the sensitivity verdict without tasks where A reached
outside its workspace. No cross-model verdict; the two models are reported
side by side.

## Secondary: breadth-first B against the frozen depth-first B

Descriptive only, no verdict: each model's B rows against the primary's
`results/full/b.jsonl` (Haiku 4.5, depth-first, claude CLI; the primary rows
as recorded, not the supplement) on the tasks both have, via `analyse.py
--compare-b`: verified, proposals (total, mean, median, max), tasks fixed by
the first proposal, mean proposals on tasks whose truth ranked below 1,
tokens, mean and total wall. Model, transport and walk all differ, so a
difference cannot be attributed to the walk alone. A walk-only comparison
(breadth on Haiku 4.5 via the claude CLI) is not part of this run.

## Zero-cost dry run on the new binaries (before this file was committed)

`results-micode/dryrun/`, run on the harness commit before this file:
A-oracle 60/60 verified; A-cheat, A-exit0, A-forge 0/60 verified, 60/60 false
successes each (an unkeyed reading would have passed 58 exit0 and 58 forge
rows); A-noop 0/60, 0 claims; B-oracle 60/60 verified, 0 false successes
(true symbol ranked 1-6 on all 60); B-none 0/60, exit 24 on all 60.

Transport probes before this file (`results-micode/probes.jsonl`, all
`claude-haiku-5-5`, no corpus task): six calls, about $0.01 estimated.
Measured there: with `--no-skills` and a narrow mode, micode's fixed prompt
is about 7k tokens (cache write on a session's first call), not the ~78k a
default session writes.

## Procedure and budget

1. Pilot: 5 tasks per arm (`operator-01, constant-01, boolean-01,
   argswap-01, drop-stmt-01`), in the order haiku-5-5 A, haiku-5-5 B,
   opus-5-5 A, opus-5-5 B, rows in `results-micode/pilot/<model>-<arm>.jsonl`.
   Harness defects found there may be fixed; each fix is a listed deviation.
   Pilot rows are not scored.
2. Full run: every corpus task, same order, rows in
   `results-micode/full/<model>/{a,b}.jsonl`, at most 2 tasks in parallel
   (shared machine). Each arm runs as a resumable job with no tool deadline;
   an interrupted arm is resumed (finished rows are kept, tasks in flight are
   re-run from scratch and any spend they logged is recorded in
   `results-micode/full/killed-spend.jsonl`).
3. Cap: $60 ESTIMATED list-price spend in total, summed by `run_ab.spent()`
   over every row under `results-micode/` (probes, pilot, full). A task starts
   only if spent + (tasks in flight incl. it) x reserve ≤ $60. Reserve per
   task: pilot $1 (haiku-5-5) / $5 (opus-5-5); full run, twice the arm's
   largest pilot per-task cost rounded up to $0.10 (at least $0.10). If the
   cap stops the run, the remaining arms are not run, only tasks both arms of
   a model completed are scored, and the shortfall is stated. The order puts
   opus-5-5 B last; it is the arm the cap would cut.
4. Rate limits: a task whose micode calls report a rate limit (a `retry` or
   `session_error` event matching `run_ab.RATE_LIMIT`) is recorded as such.
   After 3 consecutive such tasks the arm stops launching tasks
   (`RATE_LIMIT_STOP`). It is resumed at most once, after at least 30 minutes;
   a second stop ends that arm and is reported with its shortfall.
5. Infrastructure failures (micode exits non-zero, no `turn_end`, no verdict
   from `cortex`) are scored as arm failures, not excluded, and listed.
6. Wall times come from a shared machine running other benchmarks; they are
   reported with that caveat and are not a criterion.
