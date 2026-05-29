# Axon ASI Demo — Autonomous Prompt Optimizer

The first end-to-end ASI workflow on Axon, written entirely against
**already-shipped** primitives (no Phase-5+ features required). It exists
as the forcing function described in `ROADMAP.md` §10 — every awkward
edge surfaced while building this becomes a concrete spec line for the
Phase 5–10 work.

## What it does

Given a small held-out test set of phone-number-extraction examples, the
demo searches over 8 prompt variants for one that scores ≥ 90/100,
runtime-verifies the result has confidence ≥ 0.9, and finishes with a
redteam pass on adversarial input.

```
goal:        extraction_score ≥ 90/100, confidence ≥ 0.9
search:      goal_run live hill-climb over try_variant(i64) -> i64
verify:      @[verify(confidence >= 0.9)] on the deploy_gate result
redteam:     adversarial input must not yield a numeric extraction
provenance:  ~/.cache/axon/provenance.jsonl (append-only NDJSON)
```

## What this exercises (all three pillars)

| Pillar | Mechanism |
|---|---|
| **Goal-directedness** | `goal_run("try_variant", 100.0, 24)` drives the hill-climb |
| **Proof** (runtime fallback) | `@[verify(confidence >= 0.9)]` on `deploy_gate` |
| **Containment** (partial) | `Uncertain<i64>` carries confidence through; `uncertain_dyn_i64` marks the source as Runtime |

## Files

```
examples/asi/
├── optimize.ax    # demo #1 — phone-number extractor (ai_complete + parse_int)
├── classify.ax    # demo #2 — sentiment classifier (ai_extract_uncertain_i64)
├── summarize.ax   # demo #3 — composite-metric summarizer (length × coverage)
├── supervised_agent.ax # demo #4 — capability UNDER control (latching kill-switch); key-free
├── deliberative_agent.ax # demo #5 — constrained optimization (best PERMITTED action); key-free
├── planner.ax     # demo #6 — multi-step planning with safety lookahead (recursive-enum tree); key-free
├── pareto.ax      # demo #7 — multi-objective decision-making (Pareto frontier, value vs risk); key-free
├── allocate.ax    # demo #8 — bounded resource allocation (0/1 knapsack under a budget); key-free
├── run.sh         # Phase-10 CLI surface, simulated as bash subcommands
└── README.md      # this file
```

`run.sh` defaults to `optimize.ax`; pass a different demo via `DEMO=classify.ax ./run.sh run`.

Unlike demos #1–#3 (which call an LLM), `supervised_agent.ax` is a pure
deterministic demonstration that an optimizing agent stays inside a safety
boundary an overseer enforces — it banks the value it safely can, then a
two-strike kill-switch latches and every later action (even a safe one) is
refused. Run it directly: `axon run examples/asi/supervised_agent.ax`.

## Observability

Three scripts in `examples/asi/` give performance + progress visibility without
any compiler changes. Together they answer "how far along is this run, where
is the time going, and what should I optimize?"

| Script | What it shows | When to use |
|---|---|---|
| `bench.sh` | wall-clock + per-fn score statistics + eval-gap latencies for one run | after a single demo run |
| `watch.sh` | live tail of provenance.jsonl with sparkline of scores | in a side terminal during a run |
| `analyze.py` | cross-run statistics, plateau detection, **suggested max_evals**, recommendations to cull variants | after several runs, to tune config |

```bash
# One-shot bench:
./bench.sh                                  # benches optimize.ax
DEMO=classify.ax ./bench.sh                 # benches classify.ax
./bench.sh --json                           # NDJSON for automation

# Live progress (run in second terminal):
./watch.sh                                  # tracks try_variant
FN=critique_change ./watch.sh               # tracks code_review's adaptive fn

# Cross-run analysis:
python3 analyze.py                          # full report
python3 analyze.py --fn try_variant         # filter to one fn
python3 analyze.py --tail 100               # last 100 records only
python3 analyze.py --json | jq .            # machine-readable
```

### What gets measured today (zero compiler changes)

Source: `~/.cache/axon/provenance.jsonl` already records `(ts_ms, fn, event,
payload, score)` per `@[adaptive]` call. From that we derive:

- **per-iteration wall-clock**: gaps between consecutive `ts_ms` values
- **score trajectory**: sequence of return scores
- **convergence speed**: evals to reach 90%/95% of best
- **plateau length**: longest stretch with no improvement
- **score variance**: signal noise in the second half

### What requires future work

| Gap | Why it's hard today | Path forward |
|---|---|---|
| Per-LLM-call latency (vs. whole-iteration) | `ai_complete` is a black box from the program's view; no hook between request issue and response | **Available now** via `llm_proxy.py` + `ANTHROPIC_BASE_URL` (see below); long-term Phase-7 `LLM<Capabilities>` runtime |
| Input/output token counts | not exposed by `ai_complete` return value | **Available now** via `llm_proxy.py` (parses `usage` from response); long-term Phase-7 returns `(reply, usage)` pair |
| $-cost meter | requires token counts × model price | **Available now** via `llm_proxy.py` (per-model price table); long-term Phase-7 `Budget<R...>` with runtime halt on overrun |
| Build time per crate / module | rustc data exists but isn't surfaced | `cargo build --timings` produces `target/cargo-timings/cargo-timing-*.html` (no Axon work needed) |

### Per-LLM-call observability via `llm_proxy.py`

`crates/axon-ai` honors `ANTHROPIC_BASE_URL` — point it at a local proxy to log every API call:

```bash
# Terminal A — start the proxy
python3 examples/asi/llm_proxy.py
# → listening on http://localhost:8088, log: llm_calls.jsonl

# Terminal B — run a demo through it
export ANTHROPIC_BASE_URL=http://localhost:8088
export ANTHROPIC_API_KEY=sk-ant-...
./examples/asi/run.sh run

# Each line in llm_calls.jsonl:
# {"path":"/v1/messages","model":"claude-sonnet-4-6","tools_on":false,
#  "prompt_chars":1240,"in_tokens":312,"out_tokens":48,"cost_usd":0.001656,
#  "latency_ms":847,"status":200,"resp_bytes":612,"ts_ms":1746381234567}
```

The proxy is stdlib-only (~120 lines). Replace with **Helicone** (`oai.helicone.ai`),
**Langfuse** (SDK wrapper), or **Braintrust** (tracing) when production-scale is needed —
the `ANTHROPIC_BASE_URL` env-var keeps the integration to one variable.

### Build profiling (for the compiler itself)

```bash
RUST_MIN_STACK=16777216 cargo build -p axon-core --timings
# Then open: target/cargo-timings/cargo-timing-<latest>.html
```

Shows per-crate compilation time, codegen-unit costs, and dependency hot
paths. Useful when answering "why is this build 70 minutes."

## Running

These demos run via the **interpreter** (no LLVM/codegen needed). Two ways:

```bash
# Build the codegen-free interpreter CLI once (seconds):
cargo build -p axon-core --no-default-features --bin axon

# (a) Key-free, deterministic — mock LLM responses (ideal for CI/demos):
AXON_AI_MOCK=1 ./target/debug/axon run examples/asi/optimize.ax

# (b) Live inference — needs an API key + the LLM bridge feature:
cargo build -p axon-core --no-default-features --features asi-runtime --bin axon
ANTHROPIC_API_KEY=sk-ant-... ./target/debug/axon run examples/asi/optimize.ax

# Type-check only:
./target/debug/axon check examples/asi/optimize.ax
```

With **`AXON_AI_MOCK=1`** the `ai_complete`/`ai_extract_*` builtins return
deterministic stubs, so the full search → `@[verify]` gate pipeline runs
end-to-end with no key or network. Most demos then block at their deploy gate
(the generic mock can't hit each demo's bar); `summarize` deploys at 90.

Without a key *and* without the mock, every `ai_complete` returns `Err`,
variants score 0, and an enforced `@[verify(confidence >= …)]` gate panics —
itself a useful failure-mode demonstration.

> Note: `run.sh` (below) predates the interpreter and drives the native codegen
> build, which is pathologically slow / may not finish (see `BUILD_DIAGNOSIS.md`).
> Prefer `axon run` as shown above; the `run.sh` table documents the eventual
> `axon goal …` CLI shape.

## Phase-10 CLI surface (simulated)

`run.sh` exposes the eventual `axon goal …` / `axon trace …` shape so
the data-model contract is exercised today:

| Subcommand | Future name | Status |
|---|---|---|
| `compile` | `axon ast review` | ✅ wraps `axon check` |
| `run` | `axon goal run` | ✅ wraps `axon run` |
| `trace` | `axon trace show <id>` | ✅ filters provenance log |
| `improve` | `axon goal improve <id>` | ✅ rerun continues search |
| `redteam` | `axon redteam <plan-id>` | ⚠️ part of `main` for now |
| `replay` | `axon trace replay <id>` | ❌ Phase 9 (deterministic LLM needed) |
| `log` | `axon log --principal <id>` | ✅ raw NDJSON dump |
| `clear` | (testing helper) | ✅ wipes provenance log |

## Failure modes (each one is the *useful* demo)

1. **Budget exhausts before convergence.** Set `max_evals: 4` in
   `goal_run` and the search halts early; deploy gate panics if the
   best observed score < 90.
2. **Constraint trips.** All variants score < 90 → `@[verify]` fires
   `__axon_verify_panic` → process aborts before any deploy.
3. **Sim/prod disagreement** — *not yet exercisable*. Today there's no
   simulator separate from the live LLM; Phase 9 introduces replay +
   sim and this becomes a real failure mode.
4. **LLM-compiler returns low-confidence AST** — *not yet
   exercisable*. The English-surface compiler is Phase 10.
5. **Redteam catches hallucination.** `redteam_one(0)` checks variant 0
   against `adversarial_text()`; a numeric reply means the prompt
   leaks structure even with no phone present.

## Friction-derived gap list (input to ROADMAP §9)

This is the actual point of the build. Each item below is a primitive
or workflow we wanted while writing the demo and could not get from
shipped surface area. Treat as a prioritized punch list for the next
phases.

### Hard gaps (block real use today)

1. **`goal_run` only live-hill-climbs `fn(i64) -> i64`.** Every other
   signature falls back to retrospective best-observed lookup.
   Implication: the search input must be encoded as a single integer.
   Variant catalog → integer index works for v1, but is awkward for
   anything with continuous parameters or string inputs.
   → Phase 8 (Goal/Agent surface) should generalize via a strategy
   parameter: `for!<HillClimb> maximize over Vec<Variant>`.

2. **No deterministic replay.** `ai_complete` is non-deterministic, so
   `axon trace replay` cannot reproduce a run. Every Hello-Goal claim
   about "auditable" is therefore weaker than it sounds.
   → Phase 9 (replay engine) is load-bearing for the audit story.

3. **No typed audit log.** `~/.cache/axon/provenance.jsonl` is a flat
   NDJSON of `(fn_name, score)`. There's no `Principal`, no effect row,
   no causal link to the goal that triggered the call.
   → Phase 7 (`Principal`, runtime services) + Phase 9 (`AuditEvent`).

4. **No budget meter.** The demo can spend unbounded API calls — there
   is no `Cost<tokens, $>` ceiling enforced at the runtime. We approximate
   by capping `max_evals` in `goal_run`, but token cost per call is
   invisible.
   → Phase 7 (`LLM<Capabilities>` mediates calls; `Budget<R...>` ticks).

5. **No sandbox for AI-emitted plans.** If the LLM proposed a tool call,
   nothing would prevent it from invoking arbitrary effects. The
   `@[contained]` attribute is static, not runtime.
   → Phase 9 (`Sandbox<P>`).

### Soft gaps (workable today, but ergonomically wrong)

6. **`@[verify]` predicate language is `confidence OP K` only.** Cannot
   express `value >= 0 AND confidence >= 0.9`, or relations between two
   Uncertain values. We split the gate into the runtime check on
   confidence and a separate `if score >= 90.0` branch in main.
   → Phase 5 (refinement types) generalizes the predicate language.

7. **No string→digit-only filter builtin.** `parse_int` rejects
   "415-555-0142" because of dashes. We work around it with
   "reply with last 4 digits as integer," but that pushes the
   parsing problem onto the LLM. A `str_digits_only(s) -> str` builtin
   would have been one line.
   → stdlib gap; cheap to add.

8. **No `f64` cast operator.** Had to use `f64_to_i64()`/`i64_to_f64()`
   builtins instead of `expr as i64`. This is a stylistic choice
   today; it'd be friendlier for goal-surface code to support `as`.
   → defer; not load-bearing.

9. **No multi-arg `@[adaptive]`.** The signature constraint (`fn(i64)
   -> i64`) for live hill-climb means we can't have
   `try_variant(prompt_id, model_id) -> score`. We'd want `goal_run`
   to take a domain spec and search multi-dimensionally.
   → Phase 8 generalizes.

10. **No reward-shaping syntax.** Currently the score *is* the metric.
    There's no way to declaratively say "score = accuracy − 0.1·tokens
    spent." We compute it imperatively.
    → Phase 8 (`Reward<T>` as signed Metric, with composition).

11. **`@[adaptive]` doesn't record inputs, only return values.** That
    means `goal_run`'s hill-climb has to start from input `0` every
    run; it can't warm-start from the best previous input.
    → Phase 7 ABI extension: log `(input, output)` pairs.

12. **No agent abstraction.** The "redteam" is just another fn; there's
    no `Agent` type with tools, effects, and a policy. Two agents
    cannot be composed.
    → Phase 7+8.

### Strategic gaps (the demo is silent on these)

13. **No structured-prose surface.** A non-programmer cannot author
    this demo today; they'd have to learn `.ax` syntax. The whole
    English→AST pipeline is Phase 10.
14. **No human-in-the-loop approval.** The demo runs end-to-end
    autonomously — there's no review screen between "best variant
    found" and "deploy_gate fires." Production needs the approval
    step (Phase 12 web UI).
15. **No `simulate → stress → redteam → verify → deploy` pipeline.**
    The demo conflates "score on test set" with "simulate"; there is
    no separate sim environment, no stress test, no deploy gate beyond
    `@[verify]`. Phase 11 risk-typed pipeline is the proper home.

## Cousins (the example set)

Five variations on the same loop, each landing one more pattern. Together
they replace `examples/fibonacci.ax` and friends as the public face of
Axon: Axon is the language for goal-driven adaptive systems, not a typed
scripting language.

| Cousin | Status | What it adds |
|---|---|---|
| `optimize.ax` | ✅ | Baseline shape: ai_complete + parse + adaptive + verify + redteam |
| `classify.ax` | ✅ | Structured-extraction path (ai_extract_uncertain_i64); confidence as part of score |
| `summarize.ax` | ✅ | Composite metric (length × coverage); coverage scored by a second LLM call |
| `code_review.ax` | ✅ | Two cooperating @[adaptive] fns (proposer + critic) — multi-agent pattern, F12 workaround |
| `search_rank.ax` | ✅ | Adversarial search: redteam attempts prompt injection inside ranked documents |
| `pricing.ax` | ✅ | Multi-objective composite reward (LLM demand × confidence  +  margin compliance) — F10 workaround |

## What this demo does *not* prove

- It does **not** prove Axon is non-trivially better than LangChain or a
  Python script with retries. The credibility win comes from `@[verify]`
  + provenance + the Phase-5 refinement story landing — not from this
  demo alone.
- It does **not** require the native codegen build. The demos run on the
  codegen-free interpreter; the pre-check is
  `cargo build -p axon-core --no-default-features --bin axon` then
  `axon check examples/asi/<demo>.ax`.
- It does **not** address determinism. Every metric reported is one
  observation under one LLM seed. Real evaluation needs N runs +
  variance bounds, which is Phase 13 work.

## Next concrete step

Run the demos via the interpreter — `AXON_AI_MOCK=1 axon run
examples/asi/<demo>.ax` for a key-free pass, or build `--features asi-runtime`
and set `ANTHROPIC_API_KEY` for live inference. Errors + manual workarounds get
folded back into ROADMAP §9 as friction-derived priority — this is the point.
