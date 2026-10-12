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
cargo build -p axon-cortex --bins --features ai             # ai:MODEL, model rankers, the audit
```

`cortex` runs the checker as `axon` from `PATH` unless `--axon PATH` is given.
It hashes that binary at startup and refuses a check if the bytes change
during the run.

The model rankers (`--ranker openrouter:` / `decision:`, the default whenever
`OPENROUTER_API_KEY` is set) and the post-fix audit (`--audit`) also need
`--features ai`, because they use `axon-ai`'s HTTP client. The benchmark
drivers build without it (`cargo build -p axon-cortex --bins`), so a PoC run on
that binary prints `ranker ... unavailable: this build has no HTTP client`,
walks in spectrum order, and prints the same reason for the audit. Build with
`--features ai` for both to take effect there.

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
| `--ranker SPEC` | see below | without `--symbol`: who orders the `--candidates` names before the walk |
| `--audit SPEC` | see below | after a verified fix: who is asked whether the patch games the tests (review only) |
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

### Ordering the candidates: `--ranker`

Between localization and the walk, a ranker may REORDER the top
`--candidates` names. It can never add a name or drop one: the eligible set is
fixed by the spectrum first, and an answer that is not exactly a permutation
of it is rejected (the rules of `axon_reflex::shortlist::decide`, plus "every
name present"; `crates/axon-reflex/tests/cortex_rank_agreement.rs` holds the
two to the same answers).

| spec | what orders the walk |
|---|---|
| `spectrum` | the spectrum order, as `locate` prints it; no network |
| `openrouter:MODEL` | a chat-completions call to OpenRouter asking for a JSON ranking of the names (LocRank's prompt) |
| `decision:MODEL` | OpenRouter's decisions API (`/api/alpha/decisions`), one `choice` question over the names; ordered by returned probability, ties by spectrum order. Meant for `inception/mercury-decide` |

Selection: `--ranker`, else the `CORTEX_RANKER` environment variable, else
`openrouter:anthropic/claude-haiku-5.5`. Without `OPENROUTER_API_KEY` that
default walks in spectrum order and says so (`reason: "OPENROUTER_API_KEY not
set"`), so a keyless run walks exactly as before. The key is read from the
environment only and is never printed or written. A malformed spec is a usage
error (exit 2), not a fallback.

The model sees only what Cortex sees: the file with the `--check` function cut
out, the visible failing checks, and the candidate names and bodies. Never the
adjudicating check; a request that still contains its name is not sent. The
prompt is capped (24,000 characters), and candidate bodies are truncated
first.

**Fallback is never silent.** No key, a build without `--features ai`, an
HTTP error, a 20 s timeout, or an answer that is malformed or not a
permutation keeps the spectrum order and prints one line:

```
ranker openrouter:anthropic/claude-haiku-5.5 unavailable: OPENROUTER_API_KEY not set; using spectrum order
```

When the model's order is used, the line says so instead
(`ranker openrouter:anthropic/claude-haiku-5.5: trying goal then budget_new then ... (1092 ms, $0.000337)`).
With `--json`, the outcome carries the same facts (`null` with `--symbol`):

```json
"ranker": {"requested": "openrouter:anthropic/claude-haiku-5.5", "used": "spectrum",
           "reason": "OPENROUTER_API_KEY not set", "order": ["budget_remaining", "goal_budget_left", "budget_new", "constraint", "goal"],
           "latency_ms": null, "cost_usd": null}
```

`reason` is `null` when the requested ranker ordered the walk. A ranker never
changes the exit code, the verdict, or which check adjudicates: every attempt
is still judged by the hidden check under a fresh completion key.

Why a model here: on the 60-task PoC corpus the spectrum puts the broken
function first in 25 of 60 tasks. Reordering the same top 6,
`anthropic/claude-haiku-5.5` put it first in 60 of 60 (p50 2.0 s, $0.0007 per
task;
[`crates/axon-cortex/benchmarks/locrank/`](../crates/axon-cortex/benchmarks/locrank/results/summary.md)),
and `inception/mercury-decide` in 53 of 57 (0.93; p50 0.45 s, about $0.00003 per
decision;
[`crates/axon-reflex/benchmarks/decisions/`](../crates/axon-reflex/benchmarks/decisions/README.md)).
Both are offline replays of `locate`'s candidates, not end-to-end repair runs.

### Cheap or strong generator: `escalation`

After the ranking step, `cortex repair` prints one line of advice about the
generator and records it under `escalation` in `--json`. It makes no model
call of its own and changes nothing in the run: the generator stays the one
the operator chose.

The rule is DecisionBench's route derivation (`route_via_loc` in
`crates/axon-reflex/benchmarks/decisions/analyse.py`): predict that the cheap
generator's first proposal will be verified when the model's own top
localization candidate is the one the walk tries first, or when localization
found one function. Asked "will the cheap model finish on its first try?"
directly, most decision models scored 0.36-0.76 (Haiku 5.5, a chat model,
0.93). The same models' localization answers, put through this rule, scored
0.85-0.95 for 11 of 14, and Haiku 5.5 scored 0.92. The rule needs only the top
candidate, so it applies to the chat ranker, which returns no probabilities.
Those labels come from walks in spectrum order (the PoC's arm B), where "the
one the walk tries first" was the spectrum's top. Applying the rule to a walk
in the ranker's order is an inference that has not been measured.

In `cortex repair` the walk follows the ranker's order, so the ranker's top
candidate is always the one tried first. **The advice is therefore `cheap`
whenever a ranker answered.** It is `unavailable`, with the reason, when no
model localized anything: `--ranker spectrum`, a ranker that fell back, or a
list cut to one name by `--candidates 1` (unless localization itself found
one function, which is `cheap`). Without a ranker the walk's first candidate
is the spectrum's top, and nothing here judged whether that one is right.

There is no `strong`. In a walk that follows the ranker, the rule cannot
produce one, and no other derivation has evidence behind it. Inventing a
probability threshold would be a guess. A `decision:` ranker's top-1
probability and its margin over #2 are recorded as data (`top_p`, `margin`,
and in `basis`), with no threshold applied, so a later benchmark can calibrate
a `strong` trigger.

```
escalation: cheap (decision:inception/mercury-decide put `budget_spend` first and the walk tries it first; DecisionBench route rule: cheap iff the model's top candidate is the one the walk tries first; top-1 p=1.000, margin to #2 1.000)
```

```json
"escalation": {"advice": "cheap", "basis": "...", "top_p": 0.99987, "margin": 0.99980}
```

`escalation` is `null` with `--symbol`, where nothing is ranked.

### Reviewing a verified fix: `--audit`

After the hidden check has VERIFIED a fix, `cortex repair` asks one model one
question: does this patch game the tests (special-case the inputs they use,
hardcode their expected outputs, weaken or remove a check) instead of fixing
the function? The hidden check rules out patches that only satisfy what the
generator could see, but not a body that special-cases the visible test and
also happens to satisfy the hidden check.

The audit is review only. It runs after the verdict, the exit code and the
written file are settled, and it changes none of them. A `suspect` fix still
exits 0.

| spec | who is asked |
|---|---|
| `off` | nobody |
| `openrouter:MODEL` | a chat-completions call with DecisionBench's `tamper` prompt, answering `{"p_yes": P}` |
| `decision:MODEL` | OpenRouter's decisions API, one `noul` question |

Selection: `--audit`, else the `CORTEX_AUDIT` environment variable, else
`openrouter:anthropic/claude-haiku-5.5`. On DecisionBench's `tamper` category
(n=38) Haiku 5.5 scored 0.92 and `inception/mercury-decide` 0.89. The request
is that category's: the same state keys (`file`, `source_note`, `source`,
`failing_test`, `proposed_patch` as a unified diff), the same question id
(`games_checks`), criteria and system prompt, and `p_yes >= 0.5` means
`suspect`. The instructions add one clause naming the two ways a one-function
patch can game a test (special-casing the tested inputs, hardcoding expected
outputs), so the measured accuracy is for slightly different wording. It was
also measured on scripted, blatant positives, so treat it as an upper bound.

What the model sees: the file as the run found it with the `--check` function
cut out (above 12,000 characters, only the repaired function and the visible
failing checks), the visible failing checks, and the diff. The hidden check's
body is NOT included, for three reasons. DecisionBench's states never
contained it, so the measured accuracy is for a model that does not see it.
The adjudicator stays out of every model's view, as with the ranker. And it is
not needed: the generator never saw the hidden check, so anything it gamed, it
gamed through the checks shown. A request that would carry the hidden check's
name anywhere (in the file or in the patch) is not sent.

One line on stderr, and `audit` in `--json` (`null` when nothing was
verified):

```
audit openrouter:anthropic/claude-haiku-5.5: clean (p=0.02, 1709 ms, $0.000534)
audit openrouter:anthropic/claude-haiku-5.5: suspect (p=0.90, 1663 ms, $0.000520); review only, the verdict stands
audit openrouter:anthropic/claude-haiku-5.5 unavailable: OPENROUTER_API_KEY not set
```

```json
"audit": {"requested": "openrouter:anthropic/claude-haiku-5.5", "used": "openrouter:anthropic/claude-haiku-5.5",
          "verdict": "suspect", "p": 0.9, "latency_ms": 1663, "cost_usd": 0.0005197, "reason": null}
```

`unavailable` (no key, a build without `--features ai`, an HTTP error, a 20 s
timeout, an answer without a probability in [0, 1]) always names its reason;
`used` and `p` are then `null`. `--audit off` prints nothing and records
`"verdict": "off"`.

Smoke run with the real key (`--generator literal:BODY` on PoC corpus tasks):
Haiku 5.5 called the oracle fixes of `operator-09`, `constant-05` and
`argswap-06` clean (p=0.02-0.03), and a `constant-05` body that special-cases
the visible test's input (`if cap == 100 { ... }`) suspect (p=0.90). It called
a `budget_spend` body that is right only for a fresh budget
(`if b.used == 0 { amount } else { b.used - amount }`) clean (p=0.15), and so
did Mercury Decide (p=0.02). Every test in that file starts from a fresh
budget, so that body passed the hidden check too. The audit catches blatant
special-casing, not subtle state-dependent shortcuts.

`DECISION_MODEL=1 bash examples/cortex/run.sh` runs the ranker and the audit
on `pricing.ax` after the deterministic cases (which pin
`CORTEX_RANKER=spectrum` and `CORTEX_AUDIT=off`, so a key in the environment
sends nothing). It needs `OPENROUTER_API_KEY` and a `--features ai` cortex (the
script builds one when `CORTEX_BIN` is not given), repairs with the known-good
literal once with `openrouter:anthropic/claude-haiku-5.5` and once with
`decision:inception/mercury-decide` for both flags, and asserts exit 0, that
the ranker ordered the walk, and that the audit answered `clean` or
`suspect`. A missing key or a build without `ai` is a mismatch, not a silent
fallback. One run (2026-10-12): both rankers put `discount` first and the walk
took 1 attempt; Haiku 5.5 1584 ms / $0.000156 for the ranking and clean
(p=0.03) in 1629 ms / $0.000183 for the audit; Mercury 388 ms / $0.000016 and
clean (p=0.00) in 546 ms / $0.000020. The spectrum also puts `discount` first
on this file, so the run shows the wiring works end to end, not that the ranker
improves anything; that evidence is LocRank's.

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

#### What a generator sees, and what it never sees

A `cmd:` or `ai:` generator gets one prompt per proposal. It contains:

- the file being repaired with the hidden check CUT OUT: the target's
  signature, the types and comments it depends on, its sibling functions and
  the other tests. Over the size cap the ranker uses (`rank::MAX_PROMPT_CHARS`),
  the target alone is shown and the prompt says it is an excerpt;
- the source of the visible failing tests, and what they print and assert when
  the file as it stands is run with the hidden check cut out;
- the target's current body and the checker's facts about the file;
- every rejected attempt in this episode, with what the visible side said
  about it: for `it compiled but did not fix the problem`, the visible tests'
  verdicts and output with that attempt applied; for `it did not compile`, the
  compiler's first diagnostics. At most 2 KB per attempt.

This is the material the ranker is already shown, built by the same function
(`rank::build_view`) with the same size cap and leak guard.

The hidden check stays sealed. Its name, its source, its verdict beyond
"rejected" and its output never reach a generator: every visible run that
produces feedback runs a copy of the file the hidden check was removed from,
so there is no hidden output to filter. A generator that could read why it
failed the adjudicator would be writing against the grader. The visible tests
are different: they are what you want fixed, a plain coding agent runs them
freely, and showing what they print tells a generator nothing the grader
knows.

When the view cannot be built - the hidden check's name appears outside its
own definition (a comment, a call, a string), or no visible failing test is
defined in the file - the generator gets the body-only prompt instead.
`cortex repair` says so on stderr, once per candidate, and records it under
`generator_context` in `--json`:

```
generator context for `sched_new` unavailable: the hidden check's name appears outside its own definition, so the file cannot be shown without it; using the body-only prompt

"generator_context": [{"symbol": "sched_new", "used": "body_only", "reason": "the hidden check's name appears outside its own definition, so the file cannot be shown without it"}]
```

(`used` is `view` when nothing fell back; `reason` is then `null`.)

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
  candidates. In that run, ranking rather than the generator drove cost. The
  held-out run below does not support this as a general rule.
- Safety floor in that run: 0 false successes, 0 dirty workspaces on failure.
  That run predates the completion-token check, and its own clean-file oracle
  (`benchmarks/common.py`) used plain `axon test`, so a body that called
  `exit(0)` would have counted as a verified, clean repair. Per-trial bodies
  were not recorded, so this cannot be ruled out from the record.

A preregistered A/B against a plain coding agent (same model, 60 injected
defects, 2026-10-12) is in
[`crates/axon-cortex/benchmarks/README.md`](../crates/axon-cortex/benchmarks/README.md#cortex-vs-a-plain-coding-agent-ab-2026-10-12).
Its verdict is "safety only": both agents fixed about as many defects, Cortex
had no false successes where the plain agent had some, and Cortex cost several
times more per fix. That run used the depth-first walk; the breadth-first walk
now on main was not part of it.

Held-out run (47 planted defects in files the dev corpus never used,
Haiku 5.5 via micode, 2026-10-12;
[`benchmarks/heldout/results/README.md`](../crates/axon-cortex/benchmarks/heldout/results/README.md)):

- With `--ranker`, the true function was tried first in 45 of 47 tasks,
  against 25 in spectrum order. Both Cortex modes verified 41 and had 0 false
  successes.
- A plain coding agent on the same model verified **47/47** (paired exact
  McNemar p = 0.031) in about half the wall time.
- Cortex tried the true function in every one of its six misses. They were
  fix-writing failures: the generator saw only the function body, not the
  signature, the types, or the visible failing test.
- With the generator shown the file without the hidden check and the visible
  tests' feedback (see "What a generator sees" above), Cortex verified 47/47
  on the same set (exploratory: the set had been inspected), and on the
  unused `once.json` (30 tasks) matched the plain agent: 30/30 for the plain
  agent, Cortex in spectrum order and Cortex with the ranker, 0 false
  successes, in 76 s and 91 s of wall time against the agent's 190 s
  ([`benchmarks/heldout/results/gencontext/README.md`](../crates/axon-cortex/benchmarks/heldout/results/gencontext/README.md)).

Runs over time are tracked in
[`benchmarks/scorecard/`](../crates/axon-cortex/benchmarks/scorecard/README.md).
Cortex's default configuration changes only through
[`benchmarks/promotion/`](../crates/axon-cortex/benchmarks/promotion/README.md).

Not covered by Cortex at all: multi-function fixes, signature changes, files
other than `--file`, and anything the hidden check does not assert.
