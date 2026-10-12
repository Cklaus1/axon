# Can a cheap model reorder `cortex locate`'s candidates?

In the A/B PoC's arm B (`../ab/`, `cortex repair --candidates 6`), almost all
the wall time goes to a few tasks where the spectrum ranked the defective
function 3rd or 5th. `cortex repair` walks candidates depth-first. Each wrong
candidate burns 2–6 proposals of 3k–9.8k output tokens before `no_progress`.
After that, the true function is usually repaired on its first proposal. This
benchmark measures whether a cheap listwise ranker ("which of these six
functions is broken?") puts the true function first more often than the
spectrum does, and what that would save.

The ranker plays the `tiny-router` role of `crates/axon-reflex/src/shortlist.rs`:

- It may only **reorder** the eligible set (the spectrum's top 6). It can never
  add an id.
- Its output must be exactly one JSON object, `{"ranking": [ids]}`, either bare
  or as the whole response inside one fence.
- An unknown id, a duplicate, an empty list, extra keys, prose, no output, or a
  provider error is an **abstain**. On abstain the spectrum order is kept. A
  bad entry is never dropped to "repair" the output.
- Ids it leaves out keep their spectrum order after the ids it named. The
  shortlist contract allows an ordered subset.

## Method

1. **Spectrum.** `locrank.py locate` materializes each of the 60 tasks in
   `../ab/corpus.json` exactly as `run_ab.py` arm B does: the full `broken`
   file in a fresh temp workspace. It then runs `cortex locate --check HIDDEN
   --json` with the same arguments. The eligible set is the top 6.
   Reproducibility check: on all 60 tasks, the eligible lists equal arm B's
   recorded `ranked_top` (`eligible_mismatch: 0` in `results/summary.json`).
2. **Ranker input.** The ranker sees only what cortex holds at that point:
   - the mutant source with the hidden check's test block cut out (`given_a`);
   - the visible failing check's name and body;
   - the candidate ids, listed in spectrum order. The order is not labelled as
     a ranking.

   It never sees the hidden check, `how`, `class`, the mutation, or the true
   symbol. `ranker_prompt()` refuses to build a prompt if the hidden test is
   present.
3. **Providers.**

   | spec | what |
   |---|---|
   | `spectrum` | the incumbent; no model |
   | `scripted:oracle` / `scripted:adversary` / `scripted:reverser` | zero-cost grader self-tests |
   | `claude-cli:MODEL` | the signed-in `claude` CLI: `--tools ""`, own system prompt, `--json-schema` (ids are plain strings, **not** an enum, so the parser still decides validity), thinking off |
   | `openrouter:MODEL_ID` | OpenAI-compatible POST to `https://openrouter.ai/api/v1/chat/completions` with `temperature 0`, `max_tokens 4096`, `usage.include` |

   - If `OPENROUTER_API_KEY` is unset, the `openrouter:` provider refuses with
     a message and records nothing. It never skips silently.
   - `--reasoning off|low|…` sends `reasoning.enabled=false` or
     `reasoning.effort`, and the run is labelled `…#reasoning=off`.
   - On a 429 or 5xx, a call retries with bounded backoff: 7 attempts, waiting
     5→60 s. Retries are recorded per call.
   - `latency_s` is the successful attempt only. Queueing and backoff time are
     reported separately.
   - The upstream `provider` and the answering `model` are recorded per call.
     The key is never written: error bodies are redacted.
4. **Metrics** (per provider, n = 60):
   - truth-in-top-6 coverage;
   - top-1, top-3 and MRR (truth outside the eligible set counts as a miss);
   - abstain count, by reason;
   - latency p50/p90 over calls that returned;
   - $ per task and in total, taken from the CLI's `total_cost_usd` or
     OpenRouter's `usage.cost`.
5. **Projection** of arm B under a reordered walk. This is a
   **PROJECTION, not a measurement**; see the model below.

## Results (2026-10-12; n = 60 tasks, one run per configuration)

Coverage is 60/60 for every provider: the truth is always in the spectrum's
top 6, so a reorder can always reach it. The `(off)` rows are the same model
with `#reasoning=off`.

| ranker | top-1 | top-3 | MRR | abstain | latency p50 / p90 s | $/task |
|---|---|---|---|---|---|---|
| **spectrum** (baseline) | **25** | 53 | 0.649 | 0 | 0 | 0 |
| claude-cli `claude-haiku-4-5-20251001` | 59 | 60 | 0.989 | 1 (not_eligible) | 7.23 / 9.38 | 0.01621 |
| openrouter `anthropic/claude-haiku-5.5` | **60** | 60 | 1.000 | 1 (not_eligible, truth already rank 1) | 2.02 / 3.47 | 0.00068 |
| openrouter `anthropic/claude-haiku-5.5` (off) | 48 | 59 | 0.884 | 3 | 1.08 / 1.32 | 0.00055 |
| openrouter `openai/gpt-6-luna` | 57 | 59 | 0.967 | 0 | 3.79 / 6.05 | 0.00053 |
| openrouter `openai/gpt-6-luna` (off) | 50 | 59 | 0.899 | 0 | 1.14 / 1.54 | 0.00044 |
| openrouter `qwen/qwen3-coder-next` | 42 | 57 | 0.835 | 1 | 1.08 / 1.62 | 0.00044 |
| openrouter `typesafe/jev-router` | **60** | 60 | 1.000 | 0 | 2.54 / 4.32 | 0.01064 |
| openrouter `cohere/north-mini-code:free` | 56 | 60 | 0.964 | 11 (no_output: reasoning hit 4096) | 12.16 / 29.71 | 0 |
| openrouter `cohere/north-mini-code:free` (off) | 30 | 51 | 0.691 | 0 | 0.48 / 0.85 | 0 |
| openrouter `nvidia/nemotron-3-super-120b-a12b:free` | 52 | 56 | 0.911 | 8 (no_output) | 9.19 / 39.85 | 0 |
| openrouter `nvidia/nemotron-3-super-120b-a12b:free` (off) | 41 | 55 | 0.798 | 4 | 0.87 / 1.76 | 0 |
| openrouter `nvidia/nemotron-3.5-lightning:free` | 41 | 56 | 0.808 | 30 (no_output) | 44.88 / 60.34 | 0 |
| openrouter `nvidia/nemotron-3.5-lightning:free` (off) | 31 | 54 | 0.708 | 2 | 0.76 / 1.45 | 0 |

**Where the spectrum's rank 1 is a tie-break.** On 37 of 60 tasks the top
Ochiai score is shared, so rank 1 is decided by the tie-break, not by
evidence. In 33 tasks the truth is among the functions tied at the top.

| ranker | spectrum top score unique (n = 23): top-1 | top score tied (n = 37): top-1 |
|---|---|---|
| spectrum | 14 | 11 |
| claude-haiku-4.5 (CLI) | 23 | 36 |
| claude-haiku-5.5 | 23 | 37 |

Calling the ranker only when the top score is tied would miss 9 of the 23
"unique" tasks, where the spectrum's unique rank 1 is wrong.

**Call health.**
- No OpenRouter configuration had a provider error. All were under the 10%
  report threshold.
- One 429 retry occurred in total (nemotron-lightning, reasoning off).
- Queueing p50/p90 was 0 s.
- Upstream providers: Azure (Haiku 5.5), OpenAI (gpt-6-luna), Parasail (qwen),
  Cohere, Nvidia.
- Every `no_output` abstain is `finish_reason: length`: default reasoning used
  up the 4096-token budget. At that latency, a timeout-like abstain is the
  correct outcome for a ranker.

**`typesafe/jev-router`** (listed price −1, so you pay the routed model's
price). It routed to:

| routed model | calls | top-1 | $ total | $ max call |
|---|---|---|---|---|
| `openai/gpt-6.1-sol` | 51 | 51 | 0.4585 | 0.0413 |
| `anthropic/claude-opus-5.5` | 8 | 8 | 0.1790 | 0.0300 |
| `deepseek/deepseek-v4.1-flash` | 1 | 1 | 0.0010 | 0.0010 |

Upstream providers were OpenAI 51, Amazon Bedrock 8 and Together 1. No call
crossed the $0.05 per-call stop (`--max-call-usd 0.05`). The router matches
Haiku 5.5's accuracy at about 16× the price per task.

### Projection: arm B with the walk reordered

The projection uses the arm-B snapshot of
`poc-ab/.../ab/results/full/b.jsonl`. The run was complete: **60 rows**,
source sha16 `e716ee4c74a969d4`, taken 2026-10-12T17:48:21Z. The snapshot is
`results/armb_snapshot.jsonl`, with metadata in `.meta.json`.

**Model (stated, not measured).** It is applied to every arm-B row that ended
`verified_done` with the truth as the last candidate walked (54 of 60). The
other 6 rows are carried unchanged, because a reorder cannot reach them.

- Cortex walks the new order depth-first and stops at the truth.
- A candidate's cost does not depend on its position:
  - a candidate arm B attempted costs what it cost there (proposals, output
    tokens, $);
  - a wrong candidate arm B never reached costs the pooled mean of the 83
    observed wrong candidates: 4.04 proposals, 17,039 output tokens, $0.306;
  - the truth costs what it cost in arm B (in practice, its first proposal).
- No wrong candidate yields a verified repair. None did in arm B; whether an
  unreached one would is unknown.
- Wall time = 12.64 s × proposals + 0.00596 s × output tokens. This is a
  least-squares fit with no intercept over all 60 rows, R² = 0.984.
- The ranker's own measured latency and $ are added to every task, including
  the tasks where the spectrum was already right.

Projection over all 60 rows. Observed: 398 proposals, 13,938 s wall (14,005 s
fitted), $29.56 model spend. The proposals and wall columns give the change
against those figures.

| ranker | proposals | wall s | model $ (incl. ranker) |
|---|---|---|---|
| spectrum (sanity: must equal fitted) | 398 | 14,005 | 29.56 |
| claude-haiku-4.5 CLI | 168 (−58%) | 5,855 (−58%) | 12.27 |
| claude-haiku-5.5 | 160 (−60%) | 5,275 (−62%) | 10.82 |
| typesafe/jev-router | 160 | 5,315 | 11.42 |
| openai/gpt-6-luna | 180 | 6,127 | 12.35 |
| qwen/qwen3-coder-next | 255 | 8,891 | 17.86 |
| cohere/north-mini-code:free | 180 | 6,824 | 12.47 |

The five slow tasks the PoC named, operator-02, -04, -05, -07 and -11, took
1,979 s observed. They project to:

- **96 s** under Haiku 5.5, with every truth moved to rank 1;
- **395 s** under Haiku 4.5 CLI. Haiku 4.5 put `vcl_tick` first on operator-11
  but also named the ineligible `vcl_before`, so it abstained and that task
  keeps its spectrum walk (298 s).

Per-task figures for every ranker are in `results/summary.md`.

On a task the spectrum already gets right (about 6 s in arm B), the ranker
only adds cost:

- Haiku 5.5: about 2 s and $0.0007;
- Haiku 4.5 via the CLI: about 7 s and $0.016 (CLI p50 latency 7.2 s, of which
  6.1 s is API time).

That is the price of the reorder on the easy majority, and the projection
includes it.

## Self-tests ($0)

`python3 locrank.py selftest` checks the following:

- 10 parser cases: valid, whole-response fence, unknown id, duplicate, empty,
  prose around JSON, extra key, bare list, empty string, no output.
- The final-order rule.
- Three scripted providers over all 60 tasks, through the same text → parse
  path:
  - **oracle**: top-1 60/60, i.e. top-1 equals coverage, with 0 abstains;
  - **adversary**: injects `__injected__`; abstains 60/60 with reason
    `not_eligible`, and the final order equals the spectrum order;
  - **reverser**: 0 abstains; every truth rank is 7 − spectrum rank.

Result: `results/selftest/selftest_summary.json`.

## Reproduce

```bash
cargo build -p axon-core --no-default-features --bin axon && cargo build -p axon-cortex --bins
cd crates/axon-cortex/benchmarks/locrank       # AXON_BIN / CORTEX_BIN override target/debug
python3 locrank.py locate                      # spectrum lists, $0, ~6 s
python3 locrank.py selftest                    # $0
python3 locrank.py run --provider spectrum
python3 locrank.py run --provider claude-cli:claude-haiku-4-5-20251001 --parallel 3 --cap-usd 2
export OPENROUTER_API_KEY=...                  # never commit it; rows never contain it
python3 locrank.py run --provider openrouter:anthropic/claude-haiku-5.5 --cap-usd 5 --cap-provider-prefix openrouter:
python3 locrank.py run --provider openrouter:openai/gpt-6-luna --reasoning off --cap-usd 5 --cap-provider-prefix openrouter:
python3 locrank.py run --provider openrouter:typesafe/jev-router --max-call-usd 0.05 --cap-usd 5 --cap-provider-prefix openrouter:
python3 locrank.py snapshot-armb PATH/TO/ab/results/full/b.jsonl
python3 locrank.py summary                     # -> results/summary.{json,md}
```

Notes on running:

- A run appends one row per task to `results/<provider>.jsonl` and resumes
  where it stopped. Delete the file to rerun from scratch.
- `--cap-usd` stops launching calls once the recorded spend reaches the cap.
  It counts rows under `results/` (optionally only one provider prefix) and
  reserves $0.05 per call in flight. The arm-B snapshot is never counted.
- `--max-call-usd` stops a run after any single call costs more than the given
  amount.
- At most 3 parallel calls are allowed.

To try another OpenRouter model, run `run --provider openrouter:<id>`. Use
`--reasoning off` or `low` if its default reasoning is slow or exhausts the
4096-token budget. Then run `summary`.

## OpenRouter models

`results/openrouter_models_2026-10-12.json` is an extract of the public
`/api/v1/models` list and the per-model `/endpoints`, fetched
2026-10-12T16:47Z. The three proposed **paid** models, with prices in $ per
million tokens:

| id | prompt | completion | context | why |
|---|---|---|---|---|
| `anthropic/claude-haiku-5.5` | 0.10 | 0.50 | 1M | Successor of the PoC's Haiku 4.5, at 1/10 of Haiku 4.5's OpenRouter price ($1/$5). Same family, so it is the nearest like-for-like. Reasoning is optional (default medium); the price above holds below 100k prompt tokens. |
| `openai/gpt-6-luna` | 0.10 | 0.50 | 1.05M | Cheapest non-batch "fast tier" from a second vendor. Supports effort `none`. |
| `qwen/qwen3-coder-next` | 0.12 | 0.80 | 262k | Code-specialized open weights (80B MoE, 3B active). No reasoning, so lowest latency. Served by Parasail, which the account's provider allowlist permits. |

All three ran on all 60 tasks: about $0.03–0.04 per 60 tasks each. Batch
variants were skipped because latency is part of the question.

The three **free** models chosen are `cohere/north-mini-code:free` (code
model), `nvidia/nemotron-3.5-lightning:free` (fast, 3B active) and
`nvidia/nemotron-3-super-120b-a12b:free`. Other free models were unusable from
this account on 2026-10-12, according to `results/probes/probes.jsonl`:

| model | result |
|---|---|
| `poolside/laguna-{s,xs}-2.1:free` | 404: provider `poolside` is not in the account's allowed providers |
| `google/gemma-4-{31b,26b-a4b}-it:free` | 429: rate-limited upstream; 6 retries exhausted |
| `thinkingmachines/inkling-small:free` | 403: "only available on agentic harnesses" |

Free tiers may log prompts. The corpus is open-source example code from this
repository, so that was accepted.

## Spend (actual, from the recorded `total_cost_usd` / `usage.cost`)

| what | $ |
|---|---|
| Haiku 4.5 via the CLI, 60 tasks | 0.9723 |
| Haiku 4.5 configuration probes (4 calls on operator-02, `results/probes/`) | 0.1639 |
| OpenRouter: jev-router 0.6385, haiku-5.5 0.0408 + (off) 0.0331, gpt-6-luna 0.0321 + (off) 0.0265, qwen3-coder-next 0.0267, free models 0 | 0.7975 |
| **total** | **1.934** |

## Limits

- **n = 60 injected defects, one run each.** These are single-function
  mutants of otherwise-correct, readable example code: an operator swap, a
  constant, a boolean, an argument swap, a dropped statement, a comparison. A
  model that reads the code can spot the one odd line. Real defects are subtler
  and spread across functions, so 60/60 here is an upper bound on what to
  expect, not an estimate of it. No repeat runs, so run-to-run variance is not
  measured.
- **One configuration was tuned on a task from the test set.** The Haiku 4.5
  CLI setup (`--json-schema`, thinking off) was chosen after one probe on
  operator-02. The bare CLI wrote 11.8k tokens of prose with the JSON in a
  trailing fence: 83 s, $0.073, and an abstain under the strict parser. The
  probes are recorded in `results/probes/probes.jsonl`. The system prompt
  itself was fixed before any model saw a task.
- **Positional prior.** Candidates are listed in spectrum order. A model that
  echoed the list would score exactly the spectrum's numbers.
- **The projection is a model.** It assumes:
  - a candidate's cost does not depend on its position;
  - pooled-mean costs for wrong candidates arm B never reached;
  - no wrong candidate produces a verified repair;
  - a linear wall-time fit.

  It does not model the proposal budget (`--budget`) interacting with a
  different order, or a generator that does better or worse once given a
  different first candidate. The only way to confirm it is an arm-B rerun with
  the ranker in front.
- **Free routes change.** Model availability, upstream providers and rate
  limits differ by account and by hour. Ids and outcomes here are as of
  2026-10-12.
