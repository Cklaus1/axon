# Cortex: repairing one function under a check you trust

`cortex` (crate `crates/axon-cortex`) repairs the body of ONE function in an
Axon file and reports whether a check of your choosing accepted the result.
The check is "hidden": its name and source are never put in front of whatever
writes the patch. Every outcome has its own exit code.

All output below was taken from real runs on this tree (2026-10-12). Temp paths
are shortened to `$W`.

## When to use it

Use it when:

- you have a `.ax` file with a defect in a single function body,
- you can write an `@[test]` that FAILS now and would pass once the defect is
  fixed, and you want that test to be the only judge of "fixed",
- you want the file left exactly as it was if the repair does not succeed.

Do not use it for:

- defects spanning more than one function. Each attempt replaces one body; a
  fix that needs two edits is reported as `no_progress` / `budget_exhausted`.
- changing signatures, adding functions, or editing tests. The generator only
  ever supplies the text between one function's braces.
- deciding what the program should do. If you cannot write the hidden check,
  Cortex has nothing to adjudicate with.

## Build

```bash
cargo build -p axon-core --no-default-features --bin axon   # the checker
cargo build -p axon-cortex --bins                           # cortex
cargo build -p axon-cortex --bins --features ai             # only if you want ai:MODEL
```

`cortex` runs the checker as `axon` from `PATH` unless `--axon PATH` is given.
It hashes that binary at startup and refuses a check if the bytes change
during the run.

## The example used below

`examples/cortex/pricing.ax` has one defect: `discount` uses `>` where the rule
is "100 or more".

```axon
// 10% off orders of 100 or more.
fn discount(total: i64) -> i64 {
    if total > 100 { total / 10 } else { 0 }
}
```

It has five `visible_*` tests (one fails: `visible_boundary_order`) and one
`hidden_discount_boundary` test that is passed as `--check`. `axon test` on the
file reports 2 failures; that is intended.

```
$ axon test examples/cortex/pricing.ax
running 6 tests
test visible_small_order ... ok (0.0ms)
test visible_large_order ... ok (0.0ms)
test visible_boundary_order ... FAILED
  assertion failed: 100 != 90
test visible_subtotal ... ok (0.0ms)
test visible_shipping_threshold ... ok (0.0ms)
test hidden_discount_boundary ... FAILED
  assertion failed: 0 != 10

test result: FAILED. 4 passed, 2 failed (0ms total)
```

`bash examples/cortex/run.sh` runs every case in this guide on temp copies and
fails if any exit code differs from the expected one.

## Writing the hidden check

`--check NAME` must name an `@[test]` function in the file. Rules, all enforced:

- It must FAIL on the file as given. A check that already passes would accept
  the file unchanged, so Cortex refuses it (exit 26) before doing anything.
- It must be a test. A plain function name, or a name that matches nothing, is
  a usage error (exit 2):

  ```
  $ cortex repair --file pricing.ax --check discount ...
  no test named `discount` ran in pricing.ax. `--check` must name an @[test] function: a name matching nothing can never accept a repair, and a name that is merely a FUNCTION is not an adjudicator at all.
  exit=2
  ```

- It is never a repair candidate and is never put in the generator's prompt.
  It is also not used as localization evidence; only the OTHER failing tests
  are.
- Check the behaviour you actually want, at the boundary that is broken. The
  check is the whole definition of "done": `hidden_discount_boundary` asserts
  99, 100 and 250, so a body that is right only at 100 does not pass.

At least one other failing test is needed for `locate` (and for `repair`
without `--symbol`) to have evidence. With `--symbol` none is needed.

## `cortex locate`: what is broken?

Ranks the non-test functions by how strongly the failing visible tests point
at them (Ochiai score). Nothing is written.

```
$ cortex locate --workspace $W --file pricing.ax --check hidden_discount_boundary --axon $AXON
0.5774  discount
0.5774  final_price
0.5000  shipping
0.5000  subtotal
```

Ties are common: here `discount` and `final_price` are both reached by the one
failing test and by the same passing ones. `--json` gives every candidate with
its score plus the failing/passing split:

```
$ cortex locate --workspace $W --file pricing.ax --check hidden_discount_boundary --axon $AXON --json
{"failing":["visible_boundary_order"],"passing":["visible_small_order","visible_large_order","visible_subtotal","visible_shipping_threshold"],"ranked":[{"score":0.5773502691896258,"symbol":"discount"},{"score":0.5773502691896258,"symbol":"final_price"},{"score":0.5,"symbol":"shipping"},{"score":0.5,"symbol":"subtotal"}],"receipts":null,"schema":"cortex-locate/1"}
```

## `cortex repair`

```
cortex repair --file PATH --check NAME [--symbol NAME] [options]
```

| option | default | meaning |
|---|---|---|
| `--workspace DIR` | `.` | directory to operate in; `--file` is relative to it |
| `--symbol NAME` | none | repair only this function; skips localization |
| `--candidates N` | 3 | without `--symbol`: how many ranked candidates to try, in order |
| `--budget N` | 8 | maximum steps per candidate, summed over both passes of the walk |
| `--write-prefix P` | none | path prefix the episode may write; repeatable. With none, nothing may be written |
| `--generator SPEC` | `none` | where patch bodies come from (below) |
| `--axon PATH` | `axon` | the checker binary |
| `--principal NAME` | `agent` | recorded as the acting principal |
| `--json` | off | machine-readable outcome on stdout |

The `--fabric-*` and `--check-registry` options route checks through the
`axon-fabric` submit path; they are not covered here (see `cortex --help`).

Without `--symbol`, Cortex ranks candidates as `locate` does and walks the top
`--candidates` BREADTH-first. Pass 1 gives each candidate, in rank order, one
proposal (one generator call, adjudicated by the hidden check). If none is
verified, pass 2 resumes each candidate that pass 1 only paused, in rank
order, with the rest of its own `--budget` and the rejected attempts its
episode already collected. A candidate that stopped for its own reason in
pass 1 (going in circles, no such symbol, no generator) is not resumed. No
candidate gets more steps or proposals than it would have had if it ran
alone. A refusal or a blocked environment still ends the whole walk at once.
A single target (an explicit `--symbol`, or a ranking with one name) is never
paused.

Each attempt starts from the file as the run found it, under a fresh grant,
and each one is adjudicated by the hidden check.

Why breadth: with a real model, a wrong candidate usually spends its whole
budget on several long proposals before it gives up, while the true function,
once reached, is often repaired by its first proposal. Depth-first, a truth at
rank 3 therefore waited for two complete wrong episodes. Breadth-first it
waits for two proposals. The cost: a truth at rank 1 that needs a second
proposal now waits for one proposal from every other candidate. There is no
switch back to depth-first, because no consumer needs that order. The figures
are in the CHANGELOG entry for this change.

### Granting write access: `--write-prefix`

There is no default grant. The prefix is compared with the workspace-relative
path of `--file` as written (`pricing.ax` here).
`--write-prefix ./` does NOT cover `pricing.ax` (tried: exit 23), and neither
does an empty prefix. A prefix that does not cover the file is refused before
any generator is asked, and the file is untouched:

```
$ cortex repair --workspace $W --file pricing.ax --check hidden_discount_boundary --axon $AXON --write-prefix src/ --generator 'literal:if total >= 100 { total / 10 } else { 0 }'
from failing check(s) visible_boundary_order: trying discount (0.58) then final_price (0.58) then shipping (0.50)
refused after 2 step(s)
  `pricing.ax` is outside the granted region
-- expected exit 23, got 23: ok
-- after the refusal: pricing.ax is byte-identical to the original: ok
```

### Generators

| spec | where the body comes from |
|---|---|
| `none` (default) | nowhere; a run that needs a patch stops with exit 24 |
| `literal:BODY` | the text you give, unchanged |
| `cmd:PATH` | your program: prompt on stdin, body on stdout; 120 s per call (`AXON_CORTEX_GENERATOR_TIMEOUT_MS`) |
| `ai:MODEL` | a model through `axon-ai`; only in a `--features ai` build |

Whatever the source, the body goes through the same grant check, compile check
and hidden check.

#### `literal:` - no model

Use it when you know the fix and want it adjudicated rather than hand-applied:

```
$ cortex repair --workspace $W --file pricing.ax --check hidden_discount_boundary --axon $AXON --write-prefix pricing.ax --generator 'literal:if total >= 100 { total / 10 } else { 0 }'
from failing check(s) visible_boundary_order: trying discount (0.58) then final_price (0.58) then shipping (0.50)
verified_done after 3 step(s) — repaired `discount`
-- expected exit 0, got 0: ok
-- the repaired file keeps the patch and all its tests pass: ok
```

A wrong body that still compiles is rejected and the file restored:

```
$ cortex repair --workspace $W --file pricing.ax --check hidden_discount_boundary --axon $AXON --symbol discount --write-prefix pricing.ax --generator 'literal:total / 10'
no_progress after 6 step(s)
  tried 1 candidate(s) (discount); none satisfied `hidden_discount_boundary`, so no single-symbol repair among them was enough
  patch_symbol_body
-- expected exit 21, got 21: ok
-- after the failed repair: pricing.ax is byte-identical to the original: ok
```

A `literal:` body is offered to every candidate in turn. Without `--symbol`,
the correct `discount` body is also tried on `final_price` etc. if `discount`
did not pass; use `--symbol` when you know where it goes.

#### `cmd:` - a real model through the `claude` CLI

`crates/axon-cortex/benchmarks/drivers/claude_cli.sh` reads Cortex's prompt on
stdin, calls `claude -p` with every tool disallowed, and writes one function
body to stdout. Credentials stay in the `claude` CLI's own configuration.

| variable | default | effect |
|---|---|---|
| `CORTEX_MODEL` | `claude-haiku-4-5-20251001` | passed as `claude --model` |
| `CORTEX_MODEL_LOG` | `/dev/null` | one JSON line per call: model, tokens, cost, candidate, proposal number, body, how the body was extracted |

`claude` must be on `PATH` (here: `/root/.local/bin/claude`, version 2.1.289).
Run from a directory outside the repository: `claude` loads a `CLAUDE.md` from
its working directory, and the driver inherits cortex's.

```bash
cd "$W"
export CORTEX_MODEL=claude-haiku-4-5-20251001
export CORTEX_MODEL_LOG="$W/model-log.jsonl"
cortex repair --workspace "$W" --file pricing.ax --check hidden_discount_boundary \
  --axon "$AXON" --write-prefix pricing.ax \
  --generator "cmd:$REPO/crates/axon-cortex/benchmarks/drivers/claude_cli.sh"
```

`REAL_MODEL=1 bash examples/cortex/run.sh` does exactly this after the
deterministic cases. One run of it (2026-10-12, Haiku 4.5), recorded before
the walk became breadth-first, so it shows each candidate's proposals
together. The outcome is reported, not asserted:

```
== REAL_MODEL: repair with cmd:.../crates/axon-cortex/benchmarks/drivers/claude_cli.sh (model claude-haiku-4-5-20251001)
from failing check(s) visible_boundary_order: trying discount (0.58) then final_price (0.58) then shipping (0.50)
`discount` did not repair it; trying `final_price`
`final_price` did not repair it; trying `shipping`
budget_exhausted after 8 step(s)
  tried 3 candidate(s) (discount, final_price, shipping); none satisfied `hidden_discount_boundary`, so no single-symbol repair among them was enough
-- exit 20 (not asserted)
-- discount() now reads:
fn discount(total: i64) -> i64 {
    if total > 100 { total / 10 } else { 0 }
}
-- model log (one line per proposal):
   candidate=discount proposal=1 cost_usd=0.067229 tokens_out=2327 extraction=clean_fence body='    if total >= 100 { total - total / 10 } else { total }\n'
   candidate=discount proposal=2 ... body='if total > 200 { total - total / 5 } else if total > 100 { total - total / 10 } else { total }\n'
   candidate=discount proposal=3 ... body='    if total > 500 { total * 7 / 10 } else if total > 300 { ... } else { total }\n'
   candidate=discount proposal=4 ... body='if total > 100 { total - total / 20 } else { total }\n'
   candidate=final_price proposal=1..4 ...   (rewrites of final_price, all rejected)
   candidate=shipping proposal=1 extraction=prose_plus_fence body='if total >= 50 { 0 } else { 5 }\n'
   candidate=shipping proposal=2 extraction=raw_code body='I need more context to identify the semantic error. ...'
   candidate=shipping proposal=3 extraction=raw_code body='if total >= 50 { total } else { total + 5 }'
   candidate=shipping proposal=4,5 extraction=raw_code body="I'm unable to access the file system ..."
   total cost_usd=0.4618
```

What this one episode shows, and why it failed:

- The plumbing works: 13 proposals, each logged with its candidate, cost and
  extraction status; the episode ended with the file byte-identical to the
  original.
- The model's first `discount` proposal fixed the boundary (`>=`) but
  returned the discounted PRICE instead of the discount amount. It could not
  know which was meant: the prompt Cortex builds contains the file name, the
  function name, the current body, and compiler facts (`compiles: true`,
  warning counts), plus why earlier attempts were rejected. It does NOT
  contain the comment above the function, the other functions, or the failing
  visible test's assertion message. Here that is exactly the information that
  says what `discount` should return. Prompt content is a property of
  `generate::build_prompt`, not of the driver.
- Two `shipping` proposals were prose ("I need more context"), passed through
  as `raw_code` and rejected as bodies.

This is one trial; do not read a rate into it. The measured rates are below.

#### `ai:MODEL` - built-in model client

Only in a build with `--features ai`; in a default build it is a usage error:

```
$ cortex repair ... --generator ai:claude-haiku-4-5-20251001
this build has no model-backed generator: rebuild with `--features ai`. Refusing to run with `none` instead, which would report a build without a generator as a task that needed no repair.
exit=2
```

With the feature, the key comes from `ANTHROPIC_API_KEY` or `AXON_AI_API_KEY`.
Without either, the run stops before proposing anything:

```
$ cortex repair --file pricing.ax --check hidden_discount_boundary --write-prefix pricing.ax --axon $AXON --generator ai:claude-haiku-4-5-20251001
needs_input after 2 step(s)
generator unavailable: ANTHROPIC_API_KEY (or AXON_AI_API_KEY) is not set (generator `ai:claude-haiku-4-5-20251001`)
exit=24
```

`AXON_AI_MOCK=1` replaces the model with `axon-ai`'s deterministic stub (the
stub's text is not a valid repair; that run ended `no_progress`, exit 21).
`axon-ai` also reads `AXON_AI_PROVIDER` (e.g. `openai`) and
`AXON_AI_BASE_URL`; those were not exercised for this guide. No `ai:` episode
with a real key was run for this guide.

## Exit codes

Every one of these was produced on `pricing.ax` (or, for 27, on
`crates/axon-cortex/fixtures/pre_existing_failure.ax`).

| code | outcome | file afterwards | what to do |
|---|---|---|---|
| 0 | `verified_done` | patched | nothing; the hidden check passed and nothing regressed |
| 2 | usage error | untouched | fix the command line (bad `--check`, unknown `--generator`, `ai:` without the feature) |
| 20 | `budget_exhausted` | restored | raise `--budget`/`--candidates`, give `--symbol`, or the defect is not a one-body fix |
| 21 | `no_progress` | restored | proposals stopped changing anything useful; the generator is not helping |
| 22 | blocked | untouched | the checker could not run (e.g. `--axon /nonexistent/axon`); fix the environment |
| 23 | `refused` | untouched | the grant does not cover the file; widen `--write-prefix` deliberately |
| 24 | `needs_input` | restored | a patch was needed and no usable body came back; supply or fix `--generator` |
| 25 | `no_such_symbol` / no target | untouched | `--symbol` does not exist, or no candidate could be ranked |
| 26 | check cannot witness | untouched | `--check` already passes; name a check that currently fails |
| 27 | `adjudicated_not_clean` | patched (KEPT) | the check passed and nothing regressed, but tests that were already failing still fail; look at those separately |

Exit 27, on the fixture that has two independent defects:

```
$ cortex repair --file pre_existing_failure.ax --check hidden_target --symbol target --write-prefix pre_existing_failure.ax --axon $AXON --generator 'literal:n * 2'
adjudicated_not_clean after 3 step(s)
`hidden_target` passed and nothing regressed, but 1 check(s) that were already failing still fail: visible_other
exit=27
12:fn target(n: i64) -> i64 {n * 2}
```

Exit 26:

```
$ cortex repair --workspace $W --file pricing.ax --check visible_small_order --axon $AXON --write-prefix pricing.ax --generator 'literal:...'
`visible_small_order` already passes on pricing.ax, so it cannot witness a repair: it would accept this file unchanged. Name a check that currently FAILS, or there is nothing here to prove.
```

Others, one line each from the same session:

```
--axon /nonexistent/axon      -> the checks could not be run: check executable /nonexistent/axon cannot be resolved: No such file or directory (os error 2)   exit=22
--symbol nope                 -> no_such_symbol after 2 step(s) / `nope` is not defined in pricing.ax                                                     exit=25
--symbol discount --budget 1  -> budget_exhausted after 1 step(s)                                                                                       exit=20
(no --generator)              -> needs_input after 2 step(s) / a patch body for `discount` and no generator was supplied                               exit=24
--generator foo               -> unknown --generator `foo` (expected `none` or `ai:MODEL`)                                                             exit=2
```

## Reading the result

Plain output: the first line names the failing visible checks and the
candidate order with scores; the last line(s) give the outcome, step count
and, on failure, which candidates were tried and the last action.

`--json` prints one object:

```
$ cortex repair ... --generator 'literal:if total >= 100 { total / 10 } else { 0 }' --json
keys:     attempted, detail, episode, episode_digest, exit_code, outcome, receipts, schema, steps, symbol
values:   outcome=verified_done exit_code=0 symbol=discount steps=3 attempted=['discount'] detail='' receipts=None
episode:  episode_id, events
events:   snapshot, snapshot, observed, action_allowed, claimed, verified, snapshot, observed, proposed,
          action_allowed, patch_applied, snapshot, observed, snapshot, snapshot, observed, action_allowed, claimed, verified
```

- `schema` is `cortex-repair/1`. `attempted` is the attempt log in the order
  the attempts ran: a candidate resumed in pass 2 appears twice (for example
  `["a", "b", "c", "a", "b", "c"]`), and `symbol` is its last entry on
  success. `episode_digest` is a digest over the event record.
- `snapshot` events carry content digests of the workspace; `patch_applied`
  carries `path`, `before_digest`, `after_digest`; `verified` carries `passed`
  and a `detail` such as ``claim_done=true: hidden check `hidden_discount_boundary` passed (3 test(s) ran)``.
- The first `claimed`/`verified` pair in a run is the baseline: the check is
  run before any patch, and its failure there is what makes it a witness.
- `receipts` is non-null only on the Fabric path.

## Relationship to sealed runs

The check is not run in-process. Cortex spawns the pinned `axon` binary as a
child (`axon test FILE --json --filter NAME --completion-key-stdin`, via
`LocalInterpreterExecutor`) and reads its NDJSON verdict. Every check gets a
fresh random key over stdin, and a test counts as passed only if its result
line carries the matching completion token (an HMAC the candidate cannot
compute); an `ok` line without one is a failure. The executor can also pass
`--seal DIR` (the candidate's modules cannot name anything the operator's suite
defines, E0004). The Fabric submit path (`crates/axon-fabric/src/submit.rs`)
sets both and checks the tokens itself.

## Why the completion token matters

Plain `axon test` reports a test whose process exits 0 part-way through as
`ok`. Before the checker required completion tokens, a body that exited before
the hidden test's assertions ran was accepted:

```
$ cortex repair --file pricing.ax --check hidden_discount_boundary --symbol discount --write-prefix pricing.ax --axon $AXON --generator $'literal:exit(0)\n    0'
verified_done after 3 step(s) — repaired `discount`
exit=0
```

Now the same run ends `no_progress` (exit 21) and the file is restored;
`examples/cortex/run.sh` asserts it. Forging the verdict by printing JSON
result or summary lines, with or without exiting, is refused too: a forged
line has no valid token. A test of your own that legitimately ends through
`exit(0)` therefore counts as failed when Cortex adjudicates it.

## Measured numbers and limits

From `crates/axon-cortex/benchmarks/README.md` (localization corpus: real Axon
examples, one injected defect per trial, five defect shapes; real-model run of
2026-09-20). Not re-measured for this guide.

- Localization: across 111 trials, the true function is in the top 3 in
  **89.2%** of cases, while the top candidate is the sole most-suspicious one
  in only 31.5%. That is why `--candidates` defaults to 3.
- Real model driving the loop: **76.9%** verified repairs over **13** trials.
  Small n; the README states its own caveats (one of the 13 was generator
  unavailable; transport quality and invalid-output cost unmeasured).
- Once the right function is the candidate, the model needed under one
  proposal per repair (0.92 mean); 7.15 proposals on average went to wrong
  candidates first. **91%** of inference spend ($8.50 of $9.29) went to wrong
  candidates. Ranking, not the generator, is the cost driver.
- Safety floor in that run: 0 false successes, 0 dirty workspaces on failure.
  That run predates the completion-token check, and its own clean-file oracle
  (`benchmarks/common.py`) used plain `axon test`, so a body that called
  `exit(0)` would have counted as a verified, clean repair. Per-trial bodies
  were not recorded, so this cannot be ruled out from the record.

Not covered by Cortex at all: multi-function fixes, signature changes, files
other than `--file`, and anything the hidden check does not assert.
