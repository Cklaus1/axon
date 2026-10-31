# asi-replay - DRAFT spec

**Status: BUILT 2026-10-31 on rp/int f866d9d6 (RP-1..RP-3; authorized 2026-10-31 after Step 1 review; human: "do it all" on the integrator's recommendations, NH-1 no, NH-2 approve).**

### Step 1 review

There were two reviews of the first draft: `agent://AuditReplay` (RPA-1..6) and `agent://DevloopReplay` (RPD-1..9). Before deciding each one, I re-ran its probe on main77 or read the cited line at 77cf51fc. The scratch evidence is in `/var/tmp/r51-tmp/AsiReplay/v2/`.

| review | major | minor | accepted | accepted, disposition amended | rejected |
|---|---|---|---|---|---|
| RPA | 3 | 3 | 6 | 0 | 0 |
| RPD | 4 | 5 | 9 | 0 | 0 |

| id | disposition (one line) |
|---|---|
| RPA-1 | Accepted. The block assigned to this spec is M4150-M4169. All ids are renumbered (§5). Integrator choice |
| RPA-2 | Accepted, option (a) (integrator choice). Re-ran: with the first-draft fixtures, replacing `goal_run(...)` by the start variant's score still printed the asserted best in 5 of 6 demos. The generator now grades answers per variant, so exactly one non-start variant reaches the asserted best (observed per-variant scores in RP-1). New g01 row M4158 makes `goal_run` return its first evaluation |
| RPA-3 | Accepted. The generator holds copies of the prompts, so `--write` alone cannot fix drift. The hint and the Acceptance text are rewritten |
| RPA-4 | Accepted. `run.sh:19` defaults `DEMO` to `optimize.ax`. The RP-3 scope text now covers `hello-goal` with an extract demo |
| RPA-5 | Accepted. Re-ran summarize gate 0.95: fixture rc 3, mock rc 3 (today's mock rc is 0) |
| RPA-6 | Accepted. Column renamed to "entries (+1 header line)". The counts were re-measured on the graded generator |
| RPD-1 | Accepted, as RPA-1 |
| RPD-2 | Accepted. Both mock branches would carry the same `ai_replay_store(...)` line, so each row's `old` is now two lines: the store line plus that arm's stub line |
| RPD-3 | Accepted. The hint names the generator data. E-9's "single source" claim was wrong: the expected lines live in the test and the answers in the generator |
| RPD-4 | Accepted, using the env allow-list (integrator choice). Re-ran all 42 demos under `env_clear`-equivalent `env -i` with a temp HOME/XDG, temp `AXON_BANDIT_STATE`/`AXON_LEARNER_STATE`, absolute `AXON_PATH` and a temp cwd: the rc table is identical to §1, and stable over 3 passes |
| RPD-5 | Accepted. RP-3 tests print the marker texts. RP-1 reports every failing demo, in a fixed order. M4152's marker names `classify` |
| RPD-6 | Accepted. Warm timings re-measured: 8-42 ms per demo, 3 passes |
| RPD-7 | Accepted. The g01 cost row is split by row kind (§6) |
| RPD-8 | Accepted. `native-parity-spec.md:384`, `:389` and `:467` confirm that lane C edits `env_registry.rs:28` and `CLAUDE.md:266` (§7) |
| RPD-9 | Accepted. pci theme text added to RP-3 |

**Needs human decided:** `[DECIDED by human 2026-10-31: NH-1 no - hand-authored fixtures, no paid recording; NH-2 approve - replay cache before mock for ai_extract_uncertain_*]`

Base: worktree `specs3`, branch `specs/cs3` = `main` 77cf51fc. Every anchor below was read at 77cf51fc. Every probe ran on `/var/tmp/r51-tmp/main77-bin/axon` (`axon 0.1.0 (77cf51fc)`). Scratch files are under `/var/tmp/r51-tmp/AsiReplay/`.

**No real model call was made.** Three facts make this certain:
- The shell had no key variable. `env | cut -d= -f1` listed only `ANTHROPIC_BASE_URL`, `ANTHROPIC_DEFAULT_HAIKU_MODEL` and `OPENAI_BASE_URL`, with no `*KEY*` name. Keys are read from `AXON_AI_API_KEY` / `ANTHROPIC_API_KEY` (`crates/axon-ai/src/lib.rs:271`, `:275`).
- Every probe ran under `env -i PATH=/usr/bin:/bin HOME=<scratch> XDG_CACHE_HOME=<scratch>`, plus only `AXON_PATH`, `AXON_SEED=42`, the two state variables (RPD-4 re-runs), and `AXON_AI_MOCK` / `AXON_AI_REPLAY`.
- The binary has no live path compiled in. An extract miss prints `axon: panic: ai_extract_uncertain_i64 requires --features asi-runtime (or set AXON_AI_MOCK=1)`, and an `ai_complete` miss is E1300, from the `#[cfg(not(feature = "asi-runtime"))]` arm (`interp/builtins.rs:6183-6222`).

## 0. Source entries

| item | source (`tasks/opportunities.md`) | one line |
|---|---|---|
| RP-1 | O-GAP-10 (:2928) | No offline test shows a model-backed ASI demo succeeding. Add hand-authored replay fixtures for the six model demos, plus a test that asserts each one reaches its deploy gate, having selected the one best variant |
| RP-2 | O-GAP-10 (:2928) | The all-demos test passes on any exit code except 101. Pin every demo's exit code under mock, in a hermetic environment |
| RP-3 | found here (the integrator logs an O- entry) | `ai_extract_uncertain_*` checks the mock before the replay cache and never records a mock answer, so `./run.sh replay` diverges for 5 of the 6 model demos |

O-GAP-10's anchors, re-checked at 77cf51fc:
- `axon-ai/src/lib.rs:438` (`MOCK_AI_COMPLETE`) is correct.
- `:810-813` are the start of the structured mock (`:810-823`).
- **The test anchor is stale.** `asi_demo_set_runs_without_crashing` is at `crates/axon-core/tests/cli_run.rs:21438-21505`, not `:21118-21170`.

### Out of scope (logged, not dropped)

- **Native replay.** A compiled binary refuses to run under `AXON_AI_REPLAY` (exit 2; `crates/axon-rt/src/lib.rs:2089`, row `:2118`). The fixtures are interpreter-only by design, so no native leg is added.
- **A strict replay mode for users** (a miss is a divergence, exit 11, through `replay.rs:463` `diverge` / `:1131` `finish`). It is useful to users, but it is a new env var, so it is language surface. RP-1 detects misses without it (E-2). The integrator logs an O- entry.
- **Extracts leave no audit record.** `ai_extract_uncertain_*` writes no `ai_call` provenance record and no cost on any path (`interp/builtins.rs:6224-6281`, against `ai_complete`'s records at `:6104`, `:6135` and `:6168`). The live path also stores tokens `0` (`:6244`, `:6270`). This is an audit gap, not a replay gap. Logged.
- **`persistent_bandit` / `persistent_learner` keep state across runs**, by default in `/tmp/axon_persistent_*.txt` (`persistent_bandit.ax:33-36`, `persistent_learner.ax:32-35`). Over three back-to-back passes their exit code stayed 0, but `persistent_bandit`'s stdout changed. RP-2 pins only exit codes, and now points both state variables at a temp dir `[REVISED: RPD-4]`. Logged.
- **A real-model recording.** NH-1, decided no.
- The O-GAP-10 stale test anchor: the integrator fixes it when it stamps the entry.

## 1. Inventory of `examples/asi/` (42 `.ax`, plus `run.sh`, `README.md` (398 lines), `bench.sh`, `watch.sh`, `analyze.py`, `llm_proxy.py`)

**Which demos call a model.** The scan strips `//` comments, then greps for `ai_complete(` / `ai_extract*(` / `ai_extract::<`:
- 9 files have a real call site. All of them use only `ai_complete` and `ai_extract_uncertain_i64`.
- `ad_optimizer.ax` and `contained.ax` mention AI calls in comments only (`ad_optimizer.ax:26-31` uses a local stand-in, `contained.ax:13-14`).
- `taint_flow`, `risk_monitor` and `safety_stack_demo` use user fns or strings named `ai_*`, not the builtins.
- `examples/stdlib/uncertain.ax` and `world.ax` have no live call site.
- `@[goal]` / `@[adaptive]` / `goal_run` never call a model themselves.

**How it was run.** All 42 ran under mock with `/var/tmp/r51-tmp/AsiReplay/runmock.sh`, which is `env -i ... AXON_PATH=examples/asi:examples/stdlib AXON_SEED=42 AXON_AI_MOCK=1 axon run <f>` on an isolated copy of the tree.
- The rc list was byte-identical over 3 passes (md5 `412355bb...`). Each run took 0.01-0.08 s.
- `[REVISED: RPD-4]` The rc list was identical again under RP-2's allow-list (absolute `AXON_PATH`, temp cwd/HOME/XDG and state files), md5 `97fe50f3...` on two further passes.

| demo | call sites | mock rc | where it stops under mock (observed) |
|---|---|---|---|
| `optimize` | 2 `ai_complete` | 3 | `Best observed score: 0 / 100`. The stub has no digits. `verify failed in deploy_gate: confidence 0 >= 0.9 is false` |
| `classify` | 2 `ai_extract_uncertain_i64` | 3 | `Best observed score: 71 / 100`. `confidence 0.71 >= 0.8 is false` |
| `summarize` | 1 + 1 | **0** | `Deploy gate: PASS - confidence >= 0.85`, `Confirmed best score: 90`. It passes on the meaningless stub, so its mock run proves nothing |
| `code_review` | 1 + 1 | 3 | `Best observed review score: 33 / 100`. `confidence 0.33 >= 0.75 is false` |
| `search_rank` | 2 extract | 3 | `Best observed score: 0 / 100`. `confidence 0 >= 0.8 is false`. The redteam never runs |
| `pricing` | 1 extract | 3 | `Best observed score: 30 / 100` (margin points only). `confidence 0.3 >= 0.7 is false` |
| `ai_fallback` | 1 `ai_complete` | 1 | Prints `classify(positive text) = Mock summary: ...`. The mock overrides the declared fallback, so the demo's own `"neutral"` check fails. Without mock: rc 0, `= neutral`, provenance `mode:"fallback"` x2 |
| `llm_cache` | 1 `ai_complete` | 1 | rc 1 is its success code (`llm_cache.ax:76-77`: "Exit 1 iff the cache delivered the expected savings"). Without mock: rc 5, E1300 |
| `contained_violation` | 1 `ai_complete` | 2 | Refused at check, E1001 at `:23` (by design). Excluded from today's test (`cli_run.rs:21457-21461`) |

The remaining 33 are offline:
- rc 0 (24): `ad_optimizer agent_metacognition allocate budget_audit categorical_goal constrained_goal contained coord_attested corrigible deliberative_agent experiment_zone kernel_orchestrator local_search pareto persistent_bandit persistent_learner planner rank risk_monitor safety_stack_demo self_improve supervised_agent taint_flow world_model`.
- rc 1 (8): `bandit_ucb goal_attribute learn_linear learn_linear_f64 multi_objective safe_bandit safe_self_improve word_freq`. Each is a computed value: stderr is empty apart from warnings, and the last line is normal output.
- rc 88 (1): `parallel_score`, which returns `best` ("score(11) = 88").

So 5 of the 6 named model demos stop at their deploy gate under mock (exit 3). The sixth (`summarize`) passes, but on the stub.

Today's test (`cli_run.rs:21484-21489`) fails only on exit 101 or on four stderr substrings. It would pass if any demo went to exit 2 (a new refusal), 5 (E1300) or 11.

## 2. Existing machinery (anchors)

**`AXON_AI_REPLAY`, the AI-call cache** (`crates/axon-core/src/interp/provenance.rs:487-580`):
- Path: `ai_replay_path` `:498-503`, unset or empty means off.
- Key: `sha256_hex("{model}\0{prompt}")` (`ai_replay_key` `:505-507`).
- Line: `<key> <tokens> <hex(response)>` (`:494-495`, written at `:568-572`). It is appended (`:573-578`), and only on a miss (`:558-560`).
- Lookup (`:536-556`) is a linear scan. Lines whose first token is not the key are skipped, so `#` header lines are harmless: a fixture with two `#` lines replayed `pricing` to rc 0.
- An extract entry stores `<value>|<confidence>`. `replay_uncertain` (`interp/builtins.rs:69-85`) turns a malformed entry into an `Err` value naming the cache.

**What each part of the key is:**
- `ai_complete`: `model = tier.api_model()` (`interp/builtins.rs:6087`). This is env-overridable (`ai_routing.rs:69-73`; Balanced default `claude-sonnet-4-6`). Probe: a cache keyed on `claude-sonnet-4-6` replayed `hi there`. With `AXON_AI_MODEL_BALANCED=other` the same run went to rc 5, E1300. **A model override makes every fixture miss.**
- `ai_extract_uncertain_i64` / `_f64`: `model` = the builtin name (`:6232-6236`, `:6262`). It does not depend on the model or the temperature.
- `ai_complete` params are fixed (`max_tokens=default;temperature=default`, `:5985`), so the temperature is not in the key.

**Precedence.** The two builtins check the cache and the mock in opposite orders. Probes: `/var/tmp/r51-tmp/AsiReplay/rp3/{p,q}.ax`.

| builtin | order | under `AXON_AI_MOCK=1` + `AXON_AI_REPLAY` | does mock record? |
|---|---|---|---|
| `ai_complete` (`:6098` lookup, `:6121` mock, `:6142` store) | replay, then mock, then live/fallback | the cache wins: prints `hi there` | yes, 1 line stored |
| `ai_extract_uncertain_i64` (`:6226` mock, `:6236` lookup) and `_f64` (`:6258`, `:6262`) | mock, then replay, then live | **the mock wins**: prints `1 0.9`, `1 0.9` although the cache holds `3|0.8`, `2.5|0.7` | **no**: 0 lines stored, and a later replay without mock exits 101 |

**What a miss does.** The cases below (prompt drift, model override, empty cache) were all observed. The first two rows are the test binary's builds (no `asi-runtime`).

| on a miss | `ai_complete` | extract |
|---|---|---|
| no mock, no key, no fallback | rc 5, E1300 (`:6216-6221`) | rc 101 panic (`:6252`) |
| no mock, `@[ai(policy(fallback))]` in scope | **silent**: the fallback value is returned, provenance `mode:"fallback"` (`ai_fallback` + empty cache: rc 0) | n/a |
| `AXON_AI_MOCK=1` | **silent**: the stub answer is returned **and appended to the fixture** (drifted `optimize` + mock: rc 0, cache 32 -> 36 lines, provenance 4 `mock` + 51 `replay`) | **silent**: the stub is returned, nothing is stored |
| `asi-runtime` build with a key | a **live billed call**, stored | a live billed call, stored (`:6244`) |
| `asi-runtime` build without a key | `Err` value; the program decides `[INFERENCE]` | `Err` value; e.g. `classify.ax:71` scores it 0 `[INFERENCE]` |

**Host journal: `AXON_RECORD` / `AXON_REPLAY` / `axon replay`** (`crates/axon-core/src/replay.rs`). Matching is sequential and fails closed on any miss (`:519-543`), with exit 11 (`:75`, latched by `diverge` `:463`, enforced at `main.rs:6297-6301`). Model calls are **not** in this journal: AI calls keep their own cache (`replay.rs:5-9`, `:521-522`). So the journal cannot replace the AI cache for fixtures.

**`axon trace --replay <run-id>`** (`main.rs:4661`). It re-runs the source recorded in the run's `run_start` record, with the stored seed (`provenance.rs:582-636`), and refuses if the source digest changed. AI answers come only from `AXON_AI_REPLAY`. Observed: an `optimize` run, then `trace --replay <rid>` with the same cache, gave rc 0 and byte-identical stdout.

**`./run.sh replay`** (`examples/asi/run.sh:98-119`). It records under mock into a temp cache, then replays without mock. Observed with `AXON=<main77> DEMO=<d>.ax`:
- `optimize`: rc 0, "reproducible".
- `classify`, `summarize`, `code_review`, `search_rank`, `pricing`: **rc 1, "replay diverged"**. The extract answers were never recorded (above). This is RP-3.

**`goal_run`** (`interp/builtins.rs:4048-4054` -> `interp/goal.rs:656` `run_goal` -> `hill_climb_i64` `:700-808`). Every demo here hill-climbs a one-`i64` adaptive fn. `[REVISED: RPA-2]`
- The search starts at 0. If the first probe already hits the target it returns at once (`:737-739`).
- The first step is `max(16, 4*max_evals)` (`:754-759`). It probes `x±step`, keeps any improvement and halves the step on a stall.
- So the climb is deterministic: under equal scores it visits variants `0, 0, ..., then v = (step mod n)` as the step halves.

**Decision (E-1).** Fixtures reuse the `AXON_AI_REPLAY` cache format unchanged, with `#` header lines. No new format and no core change are needed for RP-1.

## 3. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | RP-1 | The only evidence that a model-backed demo works end to end: prompt -> parse -> score -> `goal_run` selecting the best variant -> `@[verify]` -> redteam. Today 5 of 6 never get past the gate offline |
| P1 | RP-2 | Today a demo newly refused at check (rc 2), or one that falls to E1300 (rc 5), passes the test. A false refusal of a public-face demo would go unnoticed |
| P2 | RP-3 | A silent wrong source: under mock + replay, extracts serve the stub instead of the recording. The documented `run.sh replay` fails for 5 of 6 demos. A builtin behaviour change; NH-2 approved |

## 4. Items

### RP-1 (P1) Replay fixtures for the six model demos, and an offline success test - O-GAP-10

**Problem.** See §1. Under mock, `optimize`, `classify`, `code_review`, `search_rank` and `pricing` stop at `deploy_gate` with exit 3. `summarize` passes on the stub. No test shows any of them succeeding.

**Fixture options.** [DECIDED: NH-1 no -> (a)]

| option | what it proves | cost | determinism |
|---|---|---|---|
| (a) **hand-authored, generated** (chosen) | Given plausible model answers, the demo's whole pipeline runs: prompt construction, parsing, scoring, `goal_run` choosing the one best variant over the start variant `[REVISED: RPA-2]`, the `@[verify]` gate and the redteam branch | $0, offline. Generator 0.03 s | exact |
| (b) recorded once from a real model | (a), plus "these prompts worked on model X on date D" | NH-1: ≈ 130 calls, ≤ $2 cap (§9). Needs an `asi-runtime` build (no gate stage builds one) and a key | exact once recorded, but the outcome is whatever the model did, and may fail a gate |
| (c) both | (a) asserted; (b) committed beside it with rc-only asserts | (b)'s cost | exact |

**Graded fixtures `[REVISED: RPA-2]`.** The first-draft fixtures gave every variant the same answers. So `goal_run` never chose anything, and a `goal_run` that returned its first evaluation still printed the asserted best score in 5 of 6 demos.

Simulated by replacing `goal_run("<fn>", …)` with `i64_to_f64(<fn>(0))` in scratch copies. With the first-draft fixtures:

| demo | rc | printed best |
|---|---|---|
| optimize | 0 | 99 |
| classify | 0 | 94 |
| summarize | 0 | 93 |
| code_review | 0 | 99 |
| search_rank | 0 | 93 |
| pricing | 3 | 56 |

The generator now gives exactly one non-start variant the "good" answers: `BEST = {optimize: 4, classify: 3, summarize: 2, search_rank: 2, code_review: seed 24}`. Each is the first non-zero variant the climb reaches. Every other variant gets degraded answers, as below.

Per-variant scores were observed by replacing `goal_run` with a loop over every variant (`/var/tmp/r51-tmp/AsiReplay/v2/scores_*.txt`):

| demo | degraded answer for non-best variants | best (unique) | every other variant | `goal_run`-returns-first-eval sim |
|---|---|---|---|---|
| optimize | sample 2 answered `0000` | 99 at 4 | 66 (x7) | rc 3, `Best observed score: 66 / 100` |
| classify | same labels, confidence x0.8 | 94 at 3 | 89 (x5) | rc 0, `89 / 100` |
| summarize | each summary + " That is the main point of the paragraph above, in short." (over 12 words), coverage still `1|0.95` | 93 at 2 | 71 (x4) | rc 3, `71 / 99` |
| search_rank | clean `0|0.85` (redteam answer unchanged) | 93 at 2 | 85 (x4) | rc 0, `85 / 100` |
| code_review | `fn divide(a: i64, b: i64) -> Result<i64, str> { Ok(a / b) }`, critique `1|0.95` | 99 at seed 24 | 33 (x128, seeds -64..64) | rc 3, `33 / 100` |
| pricing | unchanged: price already varies demand | 83 at 1 | 56, 55, 67, 80 | rc 3, `56 / 100` |

- The climb never leaves the covered range. code_review visits 0, ±48, 24, −24, 48, 0, 36, 12, 30, 18, 27 (12 evals) `[derived from goal.rs:754-806; consistent with 13 replay records observed]`.
- The asserted lines are identical to the first draft's (table below).

**Reproduction (observed, graded generator** `/var/tmp/r51-tmp/AsiReplay/gen/asi_replay_fixtures_v2.py` **-> `gen/out2/`).** Each run copied the fixture to a temp dir, then ran `env -i PATH=/usr/bin:/bin HOME=<tmp> XDG_CACHE_HOME=<tmp>/cache AXON_PATH=<abs asi>:<abs stdlib> AXON_SEED=42 AXON_BANDIT_STATE=<tmp>/b AXON_LEARNER_STATE=<tmp>/l AXON_AI_REPLAY=<copy> axon run <abs>/examples/asi/<d>.ax` with cwd = `<tmp>`, no mock and no key:

| demo | entries (+1 header line) `[REVISED: RPA-6]` | rc | warm ms (3 passes) `[REVISED: RPD-6]` | fixture after run | provenance `ai_call` modes | key stdout lines |
|---|---|---|---|---|---|---|
| optimize | 32 | 0 | 11-17 | byte-identical | 55 `replay` | `Best observed score: 99 / 100` · `Deploy gate: PASS — score wrapped as Uncertain<i64> with confidence >= 0.9` · `Confirmed best variant score: 99` · `Redteam: PASS — no hallucinated phone on adversarial input` |
| classify | 36 | 0 | 11-26 | identical | none (extracts write none) | `Best observed score: 94 / 100` · `Deploy gate: PASS — Uncertain<i64> with confidence >= 0.8` · `Confirmed best score: 94` · `Redteam: PASS — model expressed uncertainty on ambiguous input` |
| summarize | 21 | 0 | 23-35 | identical | 48 `replay` | `Best observed score: 93 / 99` · `Deploy gate: PASS — confidence >= 0.85` · `Confirmed best score: 93` |
| code_review | 131 | 0 | 17-32 | identical | 13 `replay` | `Best observed review score: 99 / 100` · `Deploy gate: PASS — confidence >= 0.75` · `Confirmed best score: 99` |
| search_rank | 10 | 0 | 9-42 | identical | none | `Best observed score: 93 / 100` · `Deploy gate: PASS — confidence >= 0.8` · `Confirmed best score: 93` · `Redteam: PASS — variant 0 ignored prompt injection in document content` |
| pricing | 5 | 0 | 8-15 | identical | none | `Best observed score: 83 / 100` · `Deploy gate: PASS — confidence >= 0.7` · `Confirmed best composite score: 83` |

The dash in those lines is U+2014, as the demos print it.

**Prompt-drift probe (observed on the first-draft fixtures; the keys are unchanged).** One prefix was changed by one character: `only` -> `ONLY` at `optimize.ax:58`, and an extra space at `classify.ax:46`.
- No mock: `optimize` rc 5, E1300; `classify` rc 101, panic. The fixture is unchanged. **Loud.**
- With `AXON_AI_MOCK=1`: `optimize` rc 0, and the fixture grew 32 -> 36. **Silent, and it pollutes the fixture.** So the test never sets the mock and replays from a copy.

**The change.**
1. **`scripts/asi_replay_fixtures.py`** (new). Port `/var/tmp/r51-tmp/AsiReplay/gen/asi_replay_fixtures_v2.py`, with the data from `asi_replay_fixture_data.json` inlined as Python literals. The prompt strings are copies of the demos' strings.
   - The docstring says so, and states the `BEST` rule: one non-start variant per demo is the best one, and it is the first non-zero variant `hill_climb_i64` reaches.
   - Header line: `# AXON_AI_REPLAY fixture for examples/asi/<d>.ax - hand-authored by scripts/asi_replay_fixtures.py; do not edit`.
   - Modes:
     - `--write`: writes `examples/asi/fixtures/<d>.replay`.
     - `--check`: regenerates into a temp dir and byte-compares. rc 1 names the first differing file.
2. **`examples/asi/fixtures/{optimize,classify,summarize,code_review,search_rank,pricing}.replay`** (new, generated; 6 files; the first draft's PoC total was 46,130 bytes, about the same size `[INFERENCE]`). This subdirectory is outside every sweep:
   - the all-demos test uses a non-recursive `read_dir` with an `.ax` filter (`cli_run.rs:21453-21456`);
   - `all_examples_parity.sh:70` globs `examples/asi/*.ax`;
   - `build_outcome_regression.sh` uses `-name '*.ax'`.
3. **Test `asi_model_demos_reach_their_deploy_gate_on_replay_fixtures`** in `cli_run.rs`, inserted directly after `asi_demo_set_runs_without_crashing` (after `:21505`).
   - **Order:** for each demo in the fixed order `optimize, classify, summarize, code_review, search_rank, pricing`. Every demo runs; failures are collected and reported together `[REVISED: RPD-5]`.
   - **Setup:** a per-test temp dir `T = temp_dir()/axon_asifx_<pid>`, with the fixture copied to `T/<d>.replay` and a fresh `T/<d>/cache`.
   - **Command** `[REVISED: RPD-3/RPD-4]`: `axon().arg("run").arg(<abs demo path>)`, then `.env_clear().env("PATH","/usr/bin:/bin").env("HOME",T).env("XDG_CACHE_HOME",T/<d>/cache).env("AXON_PATH","<asi>:<stdlib>").env("AXON_SEED","42").env("AXON_BANDIT_STATE",T/bandit.txt).env("AXON_LEARNER_STATE",T/learner.txt).env("AXON_AI_REPLAY",T/<d>.replay).current_dir(T)`.
     - Precedent for `env_clear`: `cli_run.rs:40520-40532`.
     - `<asi>`/`<stdlib>` are the absolute paths built at `:21450-21452`.
     - No key, mock, model or budget variable can reach the run.
   - **Assertions:**
     - (i) rc == 0;
     - (ii) every key line in the table above appears as an exact stdout line;
     - (iii) the copy is byte-identical to the committed fixture afterwards;
     - (iv) every `"event":"ai_call"` record in `T/<d>/cache/axon/provenance.jsonl` has `"mode":"replay"`. There must be at least 1 for `optimize`, `summarize` and `code_review`; no `mock`, `live` or `fallback`.
   - **Failure message:** one line per failing demo, `ATTACK: a model-backed ASI demo no longer reaches its deploy gate on its replay fixture [<d>]`, then rc, the first non-warning stderr line and the missing key lines. Then the hint `[REVISED: RPA-3/RPD-3]`: `if a prompt in examples/asi/<d>.ax changed, copy the new prompt text into the <d> data in scripts/asi_replay_fixtures.py, run python3 -B scripts/asi_replay_fixtures.py --write, and update the expected lines for <d> in this test if the demo's scores changed`.
4. **Test `asi_replay_fixtures_match_their_generator`**, right after test 3. It runs `python3 -B scripts/asi_replay_fixtures.py --check` (python3 precedent: `tests/spec_ledger.rs:60`). It fails on a hand-edited fixture.
5. **`examples/asi/README.md`**: a short "Offline replay fixtures" paragraph in the "Running" section (heading `:193`; the key-free mock recipe is at `:201`).
   - It states what the fixtures prove (option (a) row) and what they do not: that a real model gives these answers.
   - It gives the run command, which copies first: `cp examples/asi/fixtures/classify.replay /tmp/c.replay && AXON_AI_REPLAY=/tmp/c.replay axon run examples/asi/classify.ax`.
   - It warns that running the committed file under mock or with a key appends to it.
   - Do not use an `axon` code fence: `doc_snippets.rs:40` checks this README.

**Fail-first.** Before the fixtures exist, test 3 fails on all six demos (the fixture file is missing). Under today's mock, all five gate demos give rc 3. Mutation evidence is in §5.

**Acceptance.**
- Tests 3 and 4 pass, and `--check` rc 0.
- A one-character prompt edit in any of the six demos makes test 3 fail with the hint (drift probe above).
- The committed fixtures are unchanged after `cargo test`.
- `[REVISED: RPA-3]` Test 4 does not detect demo/generator prompt drift (the generator never reads the demos); test 3 does.
- `[REVISED: RPA-2]` Mutating `goal_run` to return its first evaluation fails test 3 (M4158).

**False-refusal guard.** No checker, parser or runtime change. Controls that stay byte-identical:
- the 42-demo mock rc table (§1);
- `ai_replay_reproduces_a_recorded_run_without_the_live_model_f2` (`cli_run.rs:1211`), `asi_hello_goal_acid_test_loop_runs_end_to_end` (`:8945`), `asi_demo_replay_and_audit_commands_work_end_to_end` (`:8990`), `ai_extract_uncertain_is_stamped_ai_sourced_on_mock_and_replay_paths` (`:34795`);
- `all_examples_parity.sh` (no `.ax` added).

**Gate stages.** S16 (`cargo test -p axon-core --no-default-features`, `gate.sh:1063-1072`), which holds cli_run. Plus the per-commit duties in §7.

**Cost.** 6 replay runs ≈ 90-130 ms warm on main77 (8-42 ms each, 3 passes) `[REVISED: RPD-6]`. `[INFERENCE]` ≈ 1 s in the debug test binary. Generator 0.03 s (measured).

### RP-2 (P1) Pin every ASI demo's exit code under mock - O-GAP-10

**Problem.** `asi_demo_set_runs_without_crashing` (`cli_run.rs:21438-21505`) fails only on rc 101 or on `parse error` / `cannot find` / `type mismatch` / `IR verification` in stderr. A demo newly refused at check (rc 2 with any other message), an E1300 (rc 5), a divergence (rc 11), or a changed computed result all pass.
- Its environment is not hermetic. It inherits the developer's `AXON_AI_REPLAY`, `AXON_BUDGET_TOKENS`, model overrides, `AXON_ENGINE`, `AXON_GOAL_CONTINUE` and the state variables. Under mock a cache beats the mock for `ai_complete`, so an exported cache can turn `optimize` from rc 3 to rc 0 `[REVISED: RPD-4]`.
- Provenance and `/tmp/axon_persistent_*.txt` are written to shared locations.

**Reproduction (simulated mutation).** Changing `llm_cache.ax:77` `{ 1 } else { 0 }` to `{ 0 } else { 0 }` moves the mock rc 1 -> 0, and the test passes. Classify's gate raised to 0.99 (M4150): mock rc 3 -> 3, the test passes.

**The change.** In place, keeping the name:
1. A `const EXPECTED: &[(&str, i32)]` holds all 42 demos with the §1 rc, now including `contained_violation` = 2.
2. Every `.ax` in the directory must have a row, and every row must have a file. Failure message: `ATTACK: an ASI demo is missing from the pinned exit-code table [<name>]`.
3. rc must equal the row. Failure message: `ATTACK: an ASI demo exited with a different code under mock [<name>]`, then expected/got and the first stderr line.
4. For `contained_violation`, stderr must also contain `E1001`.
5. Keep the panic and compile-error checks. Report all failures together.
6. `[REVISED: RPD-4]` Use the same `env_clear` allow-list as RP-1 test 3, without `AXON_AI_REPLAY` and with `AXON_AI_MOCK=1`, in its own temp dir (fresh `XDG_CACHE_HOME`; state files in the temp dir; `current_dir` = temp dir). `AXON_AI_REPLAY` is never inherited. Observed: the rc table is unchanged under this environment (§1).

Rewrite the header comment. It currently says the test asserts only "no crash". It should say exit codes are pinned because rc 2/5/11 is exactly the regression to catch, and that a demo whose rc changes legitimately updates the row in the same commit.

**Fail-first.** M4154 (§5) survives today's test and is killed by the new one.

**Acceptance.** The test passes on 77cf51fc plus this change. Removing any row, or adding an `.ax` without a row, fails with the named message.

**False-refusal guard.** This *is* the guard: any other spec's change that newly refuses a demo now fails here, so the build loop has to look at it. Every rc in the table was observed 5 times identically (3 under `env -i`, 2 under the allow-list).

**Gate stages.** S16. **Cost.** None added: the same 41 runs, plus 1 (`contained_violation`, 0.01 s).

### RP-3 (P2) Extract builtins: replay before mock, and record the mock answer [DECIDED: NH-2 approve]

**Problem.** See §2, Precedence. `ai_extract_uncertain_i64` / `_f64` consult `ai_mock_enabled()` (`interp/builtins.rs:6226`, `:6258`) before `ai_replay_lookup` (`:6236`, `:6262`), and the mock branch never calls `ai_replay_store`. `ai_complete` does both the other way round (`:6098`, `:6142`), and its comment states the contract: "Checked BEFORE mock/live" (`:6083`). Consequences:
- Under mock plus a cache, an extract silently returns the stub instead of the recorded answer.
- `./run.sh replay` (whose comment, `run.sh:100`, describes only `ai_complete`) diverges for every demo that extracts.

**Reproduction (observed).** `/var/tmp/r51-tmp/AsiReplay/rp3/p.ax`, the program from `cli_run.rs:34812-34819` with value and confidence printed:

| run | output |
|---|---|
| cache `3|0.8`, `2.5|0.7`, no mock | `3 0.8` / `2.5 0.7`, rc 0 |
| the same cache with mock | `1 0.9` / `1 0.9` (mock wins) |
| mock + empty cache | `1 0.9` / `1 0.9`, cache 0 lines |
| then replay without mock | rc 101 |

`run.sh replay` results are in §2.

**The change** (`interp/builtins.rs:6224-6281`, extract arms only):
1. In both arms, move the `ai_replay_lookup` hit block (with its comment, `:6229-6238` / `:6261-6264`) above the `if ai_mock_enabled()` block.
2. In each mock branch, put `ai_replay_store(as_str(&args[0])?, name, "1|0.9", 0);` on the line directly before the arm's existing `ok!(...make_uncertain_ai(Value::Int(1), 0.9)...)` (i64) or `ok!(...make_uncertain_ai(Value::Float(1.0), 0.9)...)` (f64) line. `replay_uncertain` parses `1` as f64 1.0, which is the same value.

With `AXON_AI_REPLAY` unset, lookup returns `None` (`provenance.rs:537`) and store is a no-op (`:562`). So **mock-only runs, the native path and every parity harness are unchanged**: native refuses `AXON_AI_REPLAY`, and the native mock is `axon-ai/src/lib.rs:812-823`, untouched.

Docs in the same commit:
- `env_registry.rs:40`: "memoizes `ai_complete` and `ai_extract_uncertain_*` by (prompt, model / builtin name)".
- `CLAUDE.md:272`, the same wording.
- The `run.sh:100` and `:113` comments.
- `AXON_REFERENCE.md:94` is regenerated by the integrator (§7).

**Scope `[REVISED: RPA-4]`.** Behaviour changes only when both `AXON_AI_MOCK` and `AXON_AI_REPLAY` are set:
- `run.sh replay` now reproduces the extract demos.
- `run.sh hello-goal` with its default `optimize.ax` (`run.sh:19`) is unchanged. With an extract demo (`DEMO=classify.ax`) its step 5 (`:147-148`) now reproduces too: the same fix as `replay`.
- Tests `:1211` / `:8945` / `:8990` use `ai_complete` only. `:34795` removes `AXON_AI_REPLAY` on its mock leg.

**Fail-first tests** (`cli_run.rs`, appended after `:34882`, reusing the `:34843-34862` cache-building pattern):
- `ai_extract_replay_hit_wins_over_mock_and_a_mock_answer_is_recorded`.
  - (a) The cache plus `AXON_AI_MOCK=1` must print `3 0.8` / `2.5 0.7`. Today it prints `1 0.9`. One assertion per leg, with messages `ATTACK: an ai_extract_uncertain_* replay entry lost to the mock [i64]` and `[f64]` `[REVISED: RPD-5]`.
  - (b) Mock plus an empty cache must leave 2 lines keyed by the builtin names. Today 0 lines. Messages `ATTACK: a mock ai_extract_uncertain_* answer was not recorded to AXON_AI_REPLAY [i64]` / `[f64]`, one per leg: check the key `sha256("ai_extract_uncertain_f64\0" + prompt)` separately. A no-mock replay must then print `1 0.9` / `1 0.9`; today it gives rc 101.
- `asi_run_sh_replay_reproduces_the_extract_demos`, right after it. It runs `harness(run.sh).arg("replay")` with `DEMO=<d>.ax`, `AXON_AI_MOCK=1` and the RP-1 allow-list, for `classify`, `summarize`, `code_review`, `search_rank` and `pricing`, as `:8990-9018` does for `optimize`. It asserts success and "reproducible".
  - Today: rc 1, "replay diverged" (observed).
  - Failure message: `ATTACK: a mock ai_extract_uncertain_* answer was not recorded to AXON_AI_REPLAY [i64 via run.sh <d>]`.
  - `[INFERENCE]` `harness` (`:44`) supplies `AXON`.
  - `[REVISED: build RpBuilder 2026-10-31]` The test does NOT export `AXON_AI_MOCK=1`. `run.sh replay`'s record leg sets `AXON_AI_MOCK="${AXON_AI_MOCK:-1}"` itself, and its replay leg (`run.sh:110` at 77cf51fc) inherits the caller's environment, so an exported `AXON_AI_MOCK=1` mocks the replay leg too. Observed: with it exported, the test passed before the fix (all five demos "reproducible"); without it, all five failed with rc 1 before the fix and pass after it. `harness` supplies `AXON`, but `env_clear` drops it, so the test sets `AXON` to the test binary's `axon` again.

**Acceptance.** Both tests pass. The RP-2 table is unchanged. Test `:34795` is unchanged.

**False-refusal guard.** No check-time change. The controls are the RP-2 table, the RP-1 test (it sets no mock, so it is unaffected) and `scripts/parity_all.sh`.

**pci theme `[REVISED: RPD-9]`** `governance/notes/pci-themes/<sha8>.json`:

```json
{"theme": "asi-replay RP-3", "text": "INTERPRETER: ai_extract_uncertain_i64/_f64 consult AXON_AI_REPLAY before AXON_AI_MOCK and record the mock answer (1|0.9) to the cache; behaviour changes only when both variables are set; mock-only, live and native paths unchanged"}
```

`v022_refusal_coverage.py` is not triggered: no `Err(`/`refuse(` site is added inside a `SEAL_FN` region.

**Gate stages.** S16, plus the full commit duties in §7: this commit touches `crates/axon-core/src`. **Cost.** ≈ 1-2 s of tests `[INFERENCE]`.

## 5. g01 rows (AsiReplay block M4150-M4169) and attack markers `[REVISED: RPA-1/RPD-1/RPD-2/RPD-5]`

Format as at `scripts/v022_g01_mutations.py:12145-12150`:
`(id, description, file, original, replacement, 'axon-core', '--no-default-features --test cli_run', killer)`.
Markers go in `scripts/v022_attack_markers.py`, as at `:3390-3393`. Each `original` occurs exactly once in its file (counted with grep at 77cf51fc for M4150-M4154 and M4158; M4155-M4157 anchor on code RP-3 adds). Rows and markers land in the same commit as their item.

| id | item | file | original -> replacement | killer (test) | evidence |
|---|---|---|---|---|---|
| M4150 | RP-1 | `examples/asi/classify.ax` | `@[verify(confidence >= 0.8)]` -> `@[verify(confidence >= 0.99)]` | `asi_model_demos_reach_their_deploy_gate_on_replay_fixtures` | simulated on the graded fixtures: fixture rc 0 -> 3; mock rc 3 -> 3 (survives today) |
| M4151 | RP-1 | `examples/asi/search_rank.ax` | `    if safe == 1 {` -> `    if safe == 2 {` | same | simulated: fixture rc 3, no `Redteam: PASS` line; mock rc 3 (redteam never reached) |
| M4152 | RP-1 | `crates/axon-core/src/interp/builtins.rs` | `:79` `make_uncertain_ai(v, c)` -> `make_uncertain_ai(v, 0.9)` | same | simulated (every extract confidence set to 0.9 in the graded fixtures): classify `95` (≠ 94), summarize `90`, search_rank `90`; code_review and pricing unchanged. `:34795` prints only `source_tag` `[INFERENCE: survives it]` |
| M4153 | RP-1 | `examples/asi/summarize.ax` | `@[verify(confidence >= 0.85)]` -> `@[verify(confidence >= 0.95)]` | same (RP-2 also kills it) | `[REVISED: RPA-5]` simulated: fixture rc 0 -> 3; mock rc 0 -> 3 |
| M4154 | RP-2 | `examples/asi/llm_cache.ax` | `dict_len(cache) == unique { 1 } else { 0 }` -> `dict_len(cache) == unique { 0 } else { 0 }` | `asi_demo_set_runs_without_crashing` | simulated: mock rc 1 -> 0, no crash |
| M4155 | RP-3 | `interp/builtins.rs` (i64 arm) | `                    ok!(replay_uncertain(&cached, name, false));` -> `                    if !ai_mock_enabled() { ok!(replay_uncertain(&cached, name, false)); }` (a cache hit is ignored under mock: today's precedence) | `ai_extract_replay_hit_wins_over_mock_and_a_mock_answer_is_recorded` | probe (a) |
| M4156 | RP-3 | same, i64 mock branch | two lines `ai_replay_store(as_str(&args[0])?, name, "1|0.9", 0);` + `\n` + the i64 stub line `ok!(Value::Ok(VBox::new(make_uncertain_ai(Value::Int(1), 0.9))));` (each at its landed indent) -> the stub line alone | same, and `asi_run_sh_replay_reproduces_the_extract_demos` | probe (b) |
| M4157 | RP-3 | same, f64 mock branch | as M4156 with `Value::Float(1.0)` -> the stub line alone | `ai_extract_replay_hit_wins_over_mock_and_a_mock_answer_is_recorded` | probe (b), f64 leg |
| M4158 | RP-1 `[REVISED: RPA-2]` | `crates/axon-core/src/interp/goal.rs` | `        if best_dist <= f64::EPSILON {\n            return Ok(best_score);\n        }\n        // Coarse-then-fine` (`:737-740`; unique, 1 occurrence) -> the same with `if best_dist >= 0.0 {` (`hill_climb_i64` returns its first evaluation) | `asi_model_demos_reach_their_deploy_gate_on_replay_fixtures` | simulated (`goal_run` replaced by the start variant's score): five demos change their best line, four of them fail the gate (§4 RP-1 table) |

Markers (regex, re.S, searched in the panic message):

| ids | marker |
|---|---|
| M4150 / M4152 | `ATTACK: a model-backed ASI demo no longer reaches its deploy gate on its replay fixture \[classify\]` (M4152 changes classify first in the fixed order `[REVISED: RPD-5]`) |
| M4151 | the same, `\[search_rank\]` |
| M4153 | the same, `\[summarize\]` |
| M4158 | the same, `\[optimize\]` |
| M4154 | `ATTACK: an ASI demo exited with a different code under mock \[llm_cache\]` |
| M4155 | `ATTACK: an ai_extract_uncertain_\* replay entry lost to the mock \[i64\]` |
| M4156 | `ATTACK: a mock ai_extract_uncertain_\* answer was not recorded to AXON_AI_REPLAY \[i64\]` |
| M4157 | `ATTACK: a mock ai_extract_uncertain_\* answer was not recorded to AXON_AI_REPLAY \[f64\]` |

- Mutations in `examples/` are normal g01 practice: M3930 mutates `spec/axon-for-llms.md`.
- M4158 mutates `interp/goal.rs`, which no spec in this batch edits.
- M4159-M4169 stay unused.

No checker change, so no new refusals:
- `crates/axon-cortex/benchmarks/fence_sweep.py` `COMPILE_CAUGHT` is untouched.
- The receipt ceiling is unaffected.

## 6. Gates and cost budget ($0 paid; NH-1 decided no)

| gate | one-run cost | trigger | skip rule |
|---|---|---|---|
| RP-1/RP-2/RP-3 test bodies inside cli_run | ≈ 90-130 ms warm on main77 (6 replays); `[INFERENCE]` ≈ 2-4 s debug in total, parallel inside cli_run's 276.6 s | each item | never |
| targeted `cargo test -p axon-core --no-default-features --test cli_run -- asi_ ai_extract_replay` | ≈ 15-60 s incremental `[INFERENCE]` | each commit | never |
| `python3 -B scripts/asi_replay_fixtures.py --check` | 0.03 s (measured) | RP-1, any edit to a model-demo prompt | never |
| `python3 -B scripts/g01_anchors.py` | seconds, no cargo | every commit that adds rows or touches `examples/asi/{classify,search_rank,summarize,llm_cache}.ax`, `interp/goal.rs` or the extract arms | never |
| full cli_run (S16 component) | 276.6 s (CS2 spec §5, `gl-gate-2.log`) | integration | never |
| `cargo clippy -D warnings`, every variant the gate runs: `-p axon-core --no-default-features --all-targets`; `--no-default-features --features serde-json --all-targets` (`gate.sh:943`); `--features smt`; default/codegen `--all-targets` | `[unmeasured]` | every commit (test code is linted under `--all-targets`) | never |
| `scripts/parity_all.sh` (≈ 21 min), the `scripts/*_parity.sh` it runs, `integration_fixtures`, `vm_wasm_depth.sh` | parity ≈ 21 min (given); others `[unmeasured]` | B1 (it touches `interp/builtins.rs`) | skippable for A1/A2 (no src change) |
| `doc_snippets` | inside S16 | A2 (README), B1 (CLAUDE.md) | never for those |
| `spec_ledger.py`; `pci_delta.py --check` after `tasks/r51-pci-regen.py` with `governance/notes/pci-themes/<sha8>.json` | ≈ 10 s (pci, CS2-observed) | `spec_ledger` every commit; pci for B1 and any non-pure-join merge | never for B1 |
| reference drift (`axon_reference_is_in_sync_with_the_binary`) | inside cli_run | after the integrator's regen | never |
| g01 `--only=M4150-M4158` `[REVISED: RPD-7]` | 4 `examples/` rows: one filtered test each, no rebuild, ≈ 5-15 s `[INFERENCE]`. 5 src rows (`builtins.rs` x4, `goal.rs` x1): 8-22 s per cell (CS2 measured). ≈ 2-4 min | once at the integrated head | never |
| **`./scripts/gate.sh --strict`** | 42-44 min measured; a retry doubles it | once at the integrated head | never |

What could double the cost:
- A strict-gate retry, from reference drift if `AXON_REFERENCE.md` was hand-merged: +43 min.
- Another spec's commit that legitimately changes a demo rc and has to update RP-2's table. One line, but a gate re-run if found late.

## 7. Sequencing and lanes (build-ready) `[REVISED: Main item 5, RPD-8]`

The integrator assigns worktree and branch names. There are two lanes and three commits, all branched from the integration base.

| commit | lane | items | files touched | order |
|---|---|---|---|---|
| A1 | A | RP-2 | `crates/axon-core/tests/cli_run.rs` (rewrite `:21438-21505` in place); `scripts/v022_g01_mutations.py` + `scripts/v022_attack_markers.py` (row M4154) | first: it is the guard for everything after it |
| A2 | A | RP-1 | `scripts/asi_replay_fixtures.py` (new); `examples/asi/fixtures/*.replay` (6 new); `cli_run.rs` (2 tests inserted right after the A1 test); `examples/asi/README.md` (Running section); g01/markers (M4150-M4153, M4158) | after A1 |
| B1 | B | RP-3 | `crates/axon-core/src/interp/builtins.rs:6224-6281`; `crates/axon-core/src/env_registry.rs:40`; `examples/asi/run.sh` (comments `:100`, `:113`); `CLAUDE.md:272`; `cli_run.rs` (2 tests after `:34882`); g01/markers (M4155-M4157); `governance/notes/pci-themes/<sha8>.json` | independent of A; may run in parallel |

**Per-commit duties (every lane brief lists all of them):**
- A1 and A2:
  - the targeted cli_run tests;
  - `cargo clippy -D warnings` in all four variants (§6);
  - `doc_snippets` (A2);
  - `python3 -B scripts/asi_replay_fixtures.py --check` (A2);
  - `python3 -B scripts/g01_anchors.py`;
  - `python3 -B scripts/spec_ledger.py`;
  - no pci theme: no `crates/axon-core/src` change.
- B1, all of the above, plus:
  - `scripts/parity_all.sh`;
  - `integration_fixtures`;
  - `vm_wasm_depth.sh`;
  - the pci theme (text in RP-3) and `python3 -B tasks/r51-pci-regen.py`, then `python3 -B scripts/pci_delta.py --check`.
- g01 rows and attack markers land in the item's own commit. Each lane appends its rows as one block at the end of the MUTATIONS list and of `ATTACK_MARKERS`, under a `# asi-replay (ids M4150-M4169)` comment. The integrator resolves any append conflict; ids are disjoint by block.
- Lanes do not edit `CHANGELOG.md`, `tasks/opportunities.md` or `AXON_REFERENCE.md`. They report a CHANGELOG line and the O-GAP-10 stamp text to the integrator.

**Integrator duties, in order:**
1. Merge A1, A2, then B1. B1 merges after native-parity lane C, which also edits `interp/builtins.rs` (i32 arms near `:2903`, disjoint from `:6224-6281`), `env_registry.rs:28` and `CLAUDE.md:266` (`native-parity-spec.md:384`, `:389`, `:467`). Those hunks are disjoint from B1's `:40` / `:272` `[INFERENCE: a clean git merge]`.
2. After each `cli_run.rs` merge, run `cargo test -p axon-core --no-default-features --test cli_run --no-run` and `python3 -B -c "import ast,sys;[ast.parse(open(f).read()) for f in sys.argv[1:]]" scripts/v022_g01_mutations.py scripts/v022_attack_markers.py`. RP-2 edits in place, RP-1 inserts after `:21505` and RP-3 after `:34882`; checker-soundness-3 and native-parity append at the end of the file.
3. Regenerate `AXON_REFERENCE.md` (`axon reference`) once, after both B1 and native-parity C. Never hand-merge it.
4. Write one pci theme per `crates/axon-core/src` commit (B1, C's commits) and per non-pure-join merge, then run `tasks/r51-pci-regen.py`.
5. Add the CHANGELOG entry. Stamp O-GAP-10 (with the corrected test anchor) and log the new O- entries from §0 (RP-3 found here; strict replay mode; the extract audit gap; the persistent-demo state).
6. Run g01 `--only=M4150-M4158`, then `./scripts/gate.sh --strict` once at the integrated head.

**Cross-spec files:**
- `checker.rs`, `infer.rs`, the parser, `error.rs`, `codegen/*`, `crates/axon-rt` and `crates/axon-ai` are **not touched**.
- `interp/goal.rs` is only the target of a g01 mutation (M4158), never edited.
- If another spec's commit changes a demo's rc, it updates RP-2's row in that commit.

## 8. Engineering decisions (decided here)

| id | question | decision | why |
|---|---|---|---|
| E-1 | fixture format | the `AXON_AI_REPLAY` cache format unchanged, plus `#` header lines | lookup already skips them (observed). Works with `run.sh`, `trace --replay` and `axon run`. No core change |
| E-2 | how drift fails loudly | run with no mock, no key and no model overrides (env allow-list); assert rc 0, the exact lines, an unchanged fixture copy, and all provenance modes `replay` | Observed: a miss is then rc 5 or 101 (loud). `mode:"fallback"` catches a fallback-served miss. The unchanged copy catches any store. Alternative: a new strict env var (language surface, logged §0) |
| E-3 | hand-authored or recorded | hand-authored via a committed generator (NH-1 decided) | free and offline; it proves the pipeline |
| E-4 | where the test lives | `cli_run.rs`, S16, beside the all-demos test | same stage and helpers (`axon()`) |
| E-5 | trajectory or whole prompt space | whole reachable space, graded so one non-start variant is best `[REVISED: RPA-2]` | `goal_run` must make a real choice. Covering every variant keeps a changed search budget from causing a miss. Unused lines are harmless |
| E-6 | source-digest staleness check | no | it would force regeneration on comment edits. The behaviour check catches exactly prompt changes |
| E-7 | model id in keys | the default Balanced id (`claude-sonnet-4-6`); `env_clear` keeps out every `AXON_AI_MODEL_*` | observed: an override makes every key miss |
| E-8 | replay from the committed file or a copy | a temp copy | under mock or a key a miss appends (observed 32 -> 36) |
| E-9 | assert scores exactly | yes (`94`, `93`, ...) `[REVISED: RPD-3]` | catches a confidence mutation (M4152: 94 -> 95) and a selection mutation (M4158). The expected lines and the fixture answers live in two files (test, generator), so a change to one is reviewed against the other |
| E-10 | env control in the tests `[REVISED: RPD-4]` | `env_clear` + allow-list, temp cwd/HOME/XDG/state files | a deny-list missed `AXON_AI_REPLAY`, `AXON_ENGINE`, `AXON_GOAL_CONTINUE` and the state variables; precedent `cli_run.rs:40520-40532`; the rc table is unchanged under it (observed) |

## 9. Needs human

**NH-1. A paid recording from a real model** (option (b)/(c), §4 RP-1). `[DECIDED by human 2026-10-31: NH-1 no - hand-authored fixtures, no paid recording]`

The estimate is kept for a later revisit:
- At most the reachable prompt space: 32 + 36 + 18 + ~26 + 10 + 5 ≈ **130 calls**, ≤ 65 k tokens.
- ≈ $0.20 at the Balanced meter rate (3000 µ$/1k, `ai_routing.rs:96`); ≈ $1 at the strong rate.
- Cap $2 per full recording `[INFERENCE: provider list prices not verified]`.

**NH-2. RP-3 changes builtin behaviour** when `AXON_AI_MOCK` and `AXON_AI_REPLAY` are both set. `[DECIDED by human 2026-10-31: NH-2 approve - replay cache before mock for ai_extract_uncertain_*]`

Nothing else here is product-level or irreversible. The strict replay mode (§0) is logged for its own spec and not asked here.
