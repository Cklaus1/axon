# authoring-eval - spec (AE-1..AE-7)

**Status: DRAFT (after Step 1 review). Not authorized for build.**

A standing, per-release, per-model measure of how well models write Axon. The goal is one number per model per release that
says whether a language or tooling change made Axon easier or harder for models to write, so future work can be ranked by its
measured effect. Nothing here is implemented. Every line anchor was read at `9919af87` (branch `specs/pm`, worktree
`/home/cklaus/projects/axon-lanes/pm-spec`, = `main`). The probe binaries are `/var/tmp/r51-tmp/eg-bin/{axon,axon-intent}`
(`axon 0.1.0 (557e746c)`). I ran no cargo build and made no paid call. The only model calls were a $0 CERE probe (§10). Anything I
did not observe is marked `[INFERENCE]`.

### Step 1 review

Reviews: `agent://AuditAE` (AEA-1..24, conformance) and `agent://DevloopAE` (AED-1..15, dev loop). Before accepting a finding I
re-read its cited lines, and re-ran what could be re-run: the `unwrap`/index/division shapes (`/var/tmp/r51-tmp/EvalSpec/r1/`), the
call rows, the host's timers, and a new power simulation of the declared clustered test (`calib2.py`, §10 #10). The pre-review draft
is kept at `/var/tmp/r51-tmp/EvalSpec/spec-v1-before-step1.md`. **Accepted 39, rejected 0** (4 pairs merged, 3 accepted with changes).

| finding | sev | disposition | where |
|---|---|---|---|
| AEA-01 two references are refused by `build.py:112-117` and blank the reference in `gv.py:85-87` | blocker | accepted: `ae` set mode in `gv.load_task`/`build.py` | AE-3 Change |
| AEA-02 any generator but `micode` runs `oracle.sh` in arm a (`genviab.py:411`); `model_substituted` is micode-only (`:682`) | blocker | accepted: one `driver_for(a)`; selftest asserts `http` never runs `oracle.sh` | AE-1 |
| AED-1 / AEA-04 `SignFlip.ci(0.05)` is a 90% CI and `sign_flip_p` is one-sided (`stats.py:43`, `:138-167`) | blocker | accepted (merged): two-sided p = 2·min, CI at `alpha=0.025` | §3, D6 |
| AED-2 / AEA-03 the power table ignored family clustering | blocker | accepted (merged), with a change: re-simulated with families and the real sign-flip (`calib2.py`). Under a uniform effect clustering costs little (null SD 1.57 → 1.66-1.75 pts at k=1); the table is replaced and the concentrated-effect case stays `[INFERENCE]` | §3 |
| AED-3 the gate fired per model and per tier with no multiplicity control | blocker | accepted: primary model only, Holm across models, confirmation rerun | NH-5, AE-4 report |
| AEA-05 `Faulted` includes `steps`/`wall`/`verify`/budget/refine/signal (`runtime.rs:1265-1271`, `:1624-1642`) | major | accepted, with a change: M2 = `axis: runtime` only; the broader "fails at runtime" is kept as its own column M2f | §2 |
| AEA-06 gate-refused candidates never ran examples (`genloop.rs:512-536` before `:541`) | major | accepted: M2 denominator is check-clean ∧ gates passed | §2 |
| AEA-07 non-final rounds are graded on visible examples only (`gv.py:236-240`) | major | accepted: M2a counts visible runs from the transcript | §2 |
| AEA-08 Axon has no `unwrap` (my probe: `unwrap(x)` E0001, `x.unwrap()` E0403; `v[len(v)+3]` and `10 / z` check 0, panic rc 101) | major | accepted | AE-2 fixtures, `PANIC_KINDS` |
| AEA-09 release identity omits `axon-intent` and the prompt; `genviab.py:84-86` already records both kinds of sha | major | accepted | §3 Design, schema |
| AEA-10 `gv.AXON`/`AXON_INTENT` are import-time globals (`gv.py:46-48`) | major | accepted: binaries are a parameter | AE-4 |
| AEA-11 `REGRESSION` required T1 Δ < 0 | major | accepted: headline-only flag; `T1-REGRESSION` is a separate advisory label | AE-4 report |
| AEA-12 T3 headroom never measured on a strong model; saturation judged on CERE only | major | accepted: Opus k=1 on T3 candidates in the pilot; any-model saturation rule; out-of-band rule | AE-3, §6 |
| AEA-13 a canary in a prompt-visible file is echoed; other placements are refused (`build.py:118-120`) | major | accepted: `canary` key in `task.json` | AE-3 |
| AEA-14 no rule for model retirement or silent CERE weight swaps | major | accepted: model series, bridge-model run, `/v1/models` pin | §3, schema |
| AEA-15 / AED-13 strong-model escalation was unpaired and had no schema place or version rule | major / minor | accepted (merged): `design: unpaired-time`, same set version and served id, never flagged; or both binaries; regrade of stored round-0 replies when prompts match | §3 |
| AED-4 no skip rule; the binary sha changes every commit (`axon-core/src/main.rs:31` embeds `AXON_GIT_SHA`) | major | accepted: CERE reply cache keyed by prompt sha, 2% replay | AE-1, AE-7 |
| AED-5 equal round-0 prompts are re-sampled per binary | major | accepted: `paired_mode: regrade` | §3, AE-4 |
| AED-6 AE-5's "measured effect" regresses to the mean | major | accepted: fresh paired pre/post run on a pre-fixed subset | AE-5 |
| AED-7 wall times extrapolated from easy tasks | major | accepted: §6 rows replaced; PREREG rewrites them from the pilot | §6 |
| AED-8 AE-7 host and window unspecified; the token is another lane's | major | accepted: gpumaster, Saturday 12:00-20:00, own token (NH-6). I confirmed the host name, `cere-nightly-gpu.timer` 02:00 and the 02:00/02:47 cron jobs; I did not re-read the journal durations | AE-7, NH-6 |
| AED-9 genviab intents set `tokens: 50000` (`dev/h08-best-scores/intent.md:20`), which binds before 3 repairs (`PREREG-genviab.md:58-66`) | major | accepted: `ae` tasks set 120000; budget stop is censored | §2, AE-3 |
| AEA-16 truncation at the declared `max_tokens` is the model's output | minor | accepted: model failure, `ended_class: truncated` | §2, AE-1, D9 |
| AEA-17 the Haiku route was unspecified | minor | accepted, with a change: Haiku stays on micode (OAuth); the e01 Haiku call row has 2255 output tokens, so the 1024 cap is the custom route's; acceptance adds a long-T3 check | AE-1 |
| AEA-18 "micode under-counts tokens on a caching provider" is false (h08 row: 3979 in + 8192 `cache_read`; `micode_program_log.py:83-84` sums them) | minor | accepted: sentence replaced | AE-1 |
| AEA-19 probe tallies included the infra row | minor | accepted: 4/4 scored, arm b 3/4 | §10 #5 |
| AEA-20 no rule for `tempting` tasks | minor | accepted: `ae` has none | §2 |
| AEA-21 `build.py --set-dir` does not exist (`build.py:63`, `:464-465`) | minor | accepted | AE-3 Change |
| AEA-22 / AED-14 per-tier MDE claim unsupported | minor | accepted (merged): replaced from `calib2.out` | §3 |
| AEA-23 no-outcome-record runs and the token-budget stop (`genloop.rs:326-327`) had no class | minor | accepted | §2 |
| AEA-24 AE-5 stanzas could leak task text into the public repo | minor | accepted | AE-5 |
| AED-10 the Opus line understated its arithmetic | minor | accepted: ≈ $40-65, cap $75 | §6, NH-2 |
| AED-11 arm c is 12-18% of arm a's cost (README results table), not 5% | minor | accepted | AE-6 |
| AED-12 the 3-point escalation trigger was uncalibrated; "new M2s" undefined | minor | accepted: A/A run in the pilot; trigger from its null SD | §3, AE-3 |
| AED-15 S23 cost of the new selftests unbounded (`scripts/gate.sh:1227-1231`, `governance/gate-lanes.json:69`) | minor | accepted: ≤ 60 s added | §6, AE-4 |

## 0. Source entries

| item | entry | sev |
|---|---|---|
| AE-1..AE-7 | O-PM-01 (`tasks/opportunities.md:3508-3514`): no standing per-release, per-model measure of first-try `axon check` pass rate, repair rounds to green, false-success rate and a Python baseline. The direction is a fixed task set on the genviab harness, several models, a trend file like `crates/axon-cortex/benchmarks/scorecard/history.jsonl` | high |
| same | PM12 (`AI_Language_Plan.md:2127-2131`): "make it a standing product metric". Never specced | - |
| same | ROADMAP §9.5 (`ROADMAP.md:533`): a friction gap list (F1-F16) with an append rule and no metric | - |
| AE-3 (headroom) | O-IG-03 (`opportunities.md:3139-3143`, MEASURED 2026-11-02): genviab dev has no headroom. Haiku 5.5 verified 14/14, then 12/12 on h01-h12 in all 3 replicates | medium |

### What exists, and why there is no number today

- **genviab** (`crates/axon-intent/benchmarks/genviab/`, README `:1-133`) is the IG7 harness:
  - Arm a runs `axon-intent compile --generate --generator cmd:<driver>` (`genviab.py:414`). That is the check → gates → visible-examples
    loop with ≤ 3 repairs (`genloop.rs:51`, feedback ≤ 2048 B `:57`). `verified` = accepted ∧ the program passes the visible and hidden
    examples (`genviab.py:437`). `false_acceptance` = accepted ∧ ¬verified (`:438`).
  - Arm b grades round 0 of arm a (`:449-451`). Arm c is Python under bwrap (`:534-588`).
  - The prompt is `axon card` + task + answer rules (`genloop.rs:893-895`), all compiled into `axon-intent`. The card is 31,582 B / 534
    lines on 557e746c (§10). `spec/axon-for-llms.md` (19,045 B) is not in the prompt.
- **Saturation.**
  - genviab dev: 14/14, and 12/12 × 3 on h01-h12 (README `:89-91`, `:112-125`).
  - Cortex sets: dev 60/60 (`crates/axon-cortex/benchmarks/README.md:526`); FU-1 calibration 517/546, with multi:3 at 104/105; hx2
    erase 19/20 and sparse 20/20; hx3 erase-long 0.790, above its band [0.3, 0.7] after pass 2, so it was not frozen
    (`opportunities.md:2785-2791`).
  - Each was one difficulty level built for one reference model. Once that model solved it, no task was left to show a change.
- **The once set is empty** (`genviab/once/manifest.json` `"n": 0`), and `PREREG-genviab.md` is DRAFT.
- **What the harness cannot do today** (each checked in §10):
  - **(a)** target any model but micode's default route. `--generator` is `oracle|empty|micode` (`genviab.py:816`), and any other value
    would fall through to the oracle driver (`:411`). The driver passes only `provider.model` (`drivers/micode-program.sh:47`). An
    unpriced model is a hard stop (`ab/run_ab.py:101-102`).
  - **(b)** separate "checks clean, then panics" from wrong output in a row. The grade keeps `failed_examples` only (`gv.py:306-321`). The
    run's `ended` is only in the transcript (`genloop.rs:754-790`).
  - **(c)** keep a held-out set out of the public GitHub repo.

### Out of scope (logged, not dropped)

| what | why not here | where it lives |
|---|---|---|
| Opt-in field telemetry from real users' `intent compile` runs (PM12's literal form) | Needs consent, privacy and a collection service; a lab eval answers the ranking question first | log **O-AE-01** `[low]` |
| Tasks that call AI builtins (`extract`, `ai_complete`) | Grading needs deterministic replay fixtures inside the ceiling sandbox; `gv.intent_env` drops `AXON_AI_MOCK` (`gv.py:255-259`) | log **O-AE-02** `[medium]` |
| An arm with `spec/axon-for-llms.md` (or other docs) in context | A prompt-content experiment, not the standing metric; run it as a one-off A/B on the frozen set | log **O-AE-03** `[low]` |
| Tool-using agentic authoring (micode with tools, multi-file projects, `mod`) | A different product surface; the loop is tool-less by design (`micode-program.sh:16-17`) | log **O-AE-04** `[low]` |
| Overreach (`tempting` tasks) | IG7's measure (`genviab.py:740`); `ae` has no `tempting` category | genviab IG7 |
| The IG7 once verdict (`PREREG-genviab.md`, once-ledger) | AE reuses the code paths but never the once set, ledger or decision rule | genviab IG7 |
| A release process (tags, versions) | `0.1.0`, no release workflow | O-PM-04 |
| The non-expert usability measure | Humans, not models | O-PM-02 |
| Native/LLVM backend runs | The loop and grader use the interpreter; native parity has its own gates | unchanged |

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | AE-1, AE-2, AE-3, AE-4 | Without them there is no number: a model route with real limits, per-candidate failure classes, a task set with headroom that is not public, and a runner that writes the trend file |
| P2 | AE-5, AE-6 | Turns the number into a work ranking (failure classes → opportunities) and fixes the Python baseline's cadence |
| P3 | AE-7 | Weekly unattended CERE screen; useful only after AE-4 has run by hand twice |

## 2. Metrics (exact definitions; canon for AE-2/AE-4)

**Unit.** One *task-trial* = one (task, model, binary, trial seed). A row is **infra** when the existing rule says so: missing
transcript, infrastructure failure, a generator failure that is not a model failure, or model substitution (`genviab.py:455-458`,
`:682-684`). AE adds one infra case: a reply cut by a transport cap **below** the declared `max_tokens` (AE-1). Infra rows leave
every denominator. A run with more than 5% infra rows is **INVALID** and writes no trend row. The `ae` set has no `tempting` tasks.

| id | name | numerator | denominator |
|---|---|---|---|
| M1 | **FT-pass** (headline) | round-0 candidate passes `axon check`, the gates, and every visible **and** hidden example (`b_verified`, `genviab.py:449-451`) | scored task-trials |
| M1c | FT-check | round-0 candidate `check_ok` (`genloop.rs:140`) | scored task-trials |
| M2 | **CCP** (check-clean panic) | candidates of the denominator with ≥ 1 graded run (visible + hidden) whose `ended` is `faulted (status ..): ⚠ DENIED: .. (axis: runtime)` (`genloop.rs:771-781`, `verdict.rs:70-72`, the `_` arm of `runtime.rs:1624-1642`) | round-0 candidates with `check_ok` whose gates passed (`b_stage` reached `examples` or `accepted`) |
| M2s | CCP-soundness | the subset of M2 whose panic kind is **type-class** (AE-2: the checker should have refused it, e.g. `cannot apply Add to str / i64`) | same as M2 |
| M2f | check-clean runtime failure | as M2, but any non-sandbox `faulted` run (any axis: `runtime`, `steps`, `wall`, `verify`, `ai-policy`, `budget`, refine, halted/signal) or `timed out` | same as M2 |
| M2a | CCP-any-round | as M2, over every check-clean, gate-passing candidate in every round, counting its **visible** example runs from the transcript (non-final rounds are never graded on hidden examples, `gv.py:236-240`) | those candidates, all rounds |
| M3 | **Solve@r**, r = 0..3 | the loop accepted at round ≤ r **and** the accepted program is verified (hidden examples too) | scored task-trials |
| M3t | turns to green | among task-trials verified within the cap, the mean index of the accepting round (0-3) | verified task-trials |
| M4 | false acceptance | accepted ∧ ¬verified (`genviab.py:438`): the loop's own false success | accepted task-trials |
| M5 | Python gap | FT-pass(arm c) − FT-pass(arm a), and Solve@3 the same way, on the same tasks | paired tasks |

- **Turn cap.** 3 repairs, so ≤ 4 generation calls: `genloop::MAX_REPAIR_ROUNDS` (`genloop.rs:51`), with genviab's feedback (check
  diagnostics, then gate refusal, then example diffs, ≤ 2048 B). An unsolved task-trial is censored at the cap. It counts in the
  Solve@r denominators and is never averaged into M3t. Every `ae` task's `## Budget` sets `tokens: 120000` so that the round cap, not
  the token budget, binds (genviab's 50000 allows round 0 + 2 repairs, `PREREG-genviab.md:58-66`). A stop on the token budget
  (`genloop.rs:326-327`) is censored like the cap and recorded as `budget_stop`; above 2% of task-trials it is printed as a warning.
- **"False success" has two meanings here. Both are reported, and neither replaces the other.** M2 is the language's false success:
  `axon check` said yes, and the run panicked. M4 is the loop's false success: the pipeline accepted a program the hidden tests reject.
- **`ended_class`** (one per graded run, AE-2): `ok`, `wrong-output`, `exit-mismatch`, `panic` (M2), `resource` (axes `steps`, `wall`,
  `budget`), `refine`, `halted` (incl. signal kills, `runtime.rs:1265-1271`), `other-fault` (`verify`, `ai-policy`), `timeout`,
  `sandbox-refused`, `no-outcome-record` (`gv.py:298`), `compile-failed`. Only `panic` counts in M2; every class is its own column.
- **`truncated`** (`finish_reason == length` at the declared `max_tokens`) is a model failure: the round's candidate fails, and the
  count is its own column. A language change that lengthens programs must show up, not vanish into infra.
- **Headline.** M1 is the one number per (model, release), and the release-to-release delta is the paired Δ of M1 (§3).
  Rationale (D2): repair hides friction, so Solve@3 saturates first. In GV-4, arm a was 12/12 three times while arm b was 12, 10 and
  10 of 12 (README `:105`).

## 3. Statistics

- **Design.** Paired by task and trial seed: the same task set version, seeds, model and driver, run on two binaries (the previous
  release's and the candidate's) in the same session. The card, the prompt template, the repair feedback and the grader come from the
  binary pair under test (`axon` + `axon-intent`), and are part of the release. Two modes, recorded as `paired_mode`:
  - **`regrade`**: when every task's round-0 prompt sha256 is equal for base and candidate (no card or template change), round 0 is
    generated **once** per (task, trial) and that reply is graded under both binaries. Round-0 metrics (M1, M1c, M2) then carry no
    sampling noise in Δ. Repair rounds run per binary.
  - **`resample`**: otherwise. Both binaries generate; trials are paired by seed.
- **Test.** The statistic is the mean per-task difference in k-trial success rate. The test is the family-clustered sign-flip of
  `crates/axon-cortex/benchmarks/headroom/stats.py` (`program_sums` with family as the program, `SignFlip`, `:100-156`): two-sided
  p = min(1, 2·min(p_greater, p_less)) from `sign_flip_p(..., alternative=)` (`:159-163`), and the 95% CI is
  `SignFlip(...).ci(alpha=0.025)` (`ci` inverts two one-sided tests at `alpha`, `:138-139`; `ci90` at `:166-167` is **not** used).
- **Noise model.** Per-task success rates in hx3 n5a DEV-H55 (109 tasks × 3, Haiku 5.5, mean 0.807) fit a beta-binomial with
  intra-task correlation ρ = 0.80 (`calib.py`). A task is mostly "always" or "never" solved by one model, which is why pairing is
  powerful. `calib2.py` adds families: a task copies its family's p with probability ρ_fam, else draws its own (the marginal stays the
  fit), and applies the declared two-sided clustered test (checked against `stats.SignFlip` on 3 draws). Effect = uniform logit shift.
  Two-sided α = 0.05; 400 draws per power cell, 600 per null.

  | set mean | n tasks | families (m, ρ_fam) | k | null SD of Δ | size | power at Δ = 5 / 8 / 10 / 15 pts |
  |---|---|---|---|---|---|---|
  | 0.81 | 240 | 240 (1, 0) | 1 | 1.57 | 0.022 | 0.81 / 1.00 / 1.00 / 1.00 |
  | 0.81 | 240 | 60 (4, 0.3) | 1 | 1.75 | 0.033 | 0.78 / 1.00 / 1.00 / 1.00 |
  | 0.81 | 240 | 60 (4, 0.6) | 1 | 1.66 | 0.025 | 0.76 / 0.98 / 1.00 / 1.00 |
  | 0.81 | 240 | 60 (4, 0.6) | 3 | 0.96 | 0.038 | 1.00 / 1.00 / 1.00 / 1.00 |
  | 0.60 | 240 | 120 (2, 0.5) | 1 | 1.96 | 0.027 | 0.61 / 0.96 / 1.00 / 1.00 |
  | 0.60 | 240 | 60 (4, 0.6) | 1 | 1.99 | 0.023 | 0.63 / 0.95 / 0.99 / 1.00 |
  | 0.60 | 240 | 60 (4, 0.6) | 3 | 1.11 | 0.025 | 0.99 / 1.00 / 1.00 / 1.00 |
  | 0.60 (one tier) | 60 | 30 (2, 0.5) | 1 | 3.96 | 0.007 | 0.09 / 0.25 / 0.40 / 0.84 |
  | 0.60 (one tier) | 60 | 30 (2, 0.5) | 3 | 2.19 | 0.018 | 0.37 / 0.80 / 0.94 / 1.00 |
  | 0.81 (one tier) | 60 | 30 (2, 0.5) | 1 | 3.30 | 0.000 | 0.04 / 0.26 / 0.46 / 0.87 |
  | 0.60 (TS) | 40 | 20 (2, 0.5) | 1 | 5.14 | 0.005 | 0.03 / 0.07 / 0.13 / 0.41 |

- **Smallest change to detect (planning values, re-measured by the pilot).** On the full 240-task set: **5 points** with CERE k = 3
  (power ≥ 0.98); **8 points** with one paid k = 1 run (power ≥ 0.95; 5 points only 0.6-0.8). Per-tier numbers are **descriptive
  only**: at k = 1 their MDE is about 15 points, at k = 3 about 8; TS (n = 40) detects nothing below 15. They never gate.
- **What the simulation does not cover** `[INFERENCE]`:
  - A uniform shift moves every family. An effect confined to a few families (one construct) has the full design effect of the
    clustering and less power; not simulated. The pilot's A/A run (below) and ρ_fam estimate re-run the table on real rows.
  - The fit is from cortex repair tasks, not authoring tasks.
  - CERE trials are treated as independent draws; that k seeds give independent replies was not observed (one seed probed).
- **Variants per family.** ≤ 2 variants per family unless the pilot's ρ_fam < 0.3; then ≤ 4.
- **CERE nondeterminism.** Two probes (§10 #7): at temperature 0, the same prompt twice gave the same reply (sha `7cb232c35356`); at
  temperature 0.7 with `seed: 1`, the same prompt twice gave the same reply (`83ce71ac54a9`). So CERE trials use temperature 0.7 with
  `seed = trial index`, each trial is replayable (D4), and AE-1's reply cache relies on it. Every run replays a seeded sample of cache
  hits; a mismatch disables the cache for that run and records D4 as violated. Paid models use the provider default temperature (and
  seed where the API supports it); their trials are never replayed or cached.
- **Model series.** Each model has a `series` id: provider, requested model and, for CERE, the sha256 of its full `/v1/models` entry
  and the server build (the name `Qwen3.8-Flash-Next` alone would not show a re-deploy). A served-id or pin change starts a new series
  through a **bridge-model** run (old and new model, same binary, same set version, `design: bridge-model`). Δ is never computed
  across series or set versions.
- **Screen, then escalate.**
  - **Screen (weekly, AE-7):** CERE, k = 3, full set, both binaries. Free.
  - **Release:** the cheap paid pair (Haiku 5.5 + the OpenRouter model, NH-1), k = 1, full set, both binaries, always. The screen is
    not a gate at a release.
  - **Between releases**, the weekly screen escalates to the Haiku pair when its 95% CI excludes 0, or |Δ| ≥ 2.5 × the null SD
    measured by the pilot's A/A run, or on an M2s instance whose (family, panic kind) never appeared in `history.jsonl` for this set
    version.
  - **Strong model** (Opus 5.5, NH-2 approval per use): `design: unpaired-time` against a stored Opus run on the same set version and
    served model id (`ae.py run --base-run <run_id>`), with arm c rerun as the drift control. When the round-0 prompts equal the stored
    run's, the stored round-0 replies are re-graded under the new binary at $0 and M1 is paired after all. An unpaired row prints the
    difference with `drift: arm c Δ = x` and never `IMPROVED`/`REGRESSION`. Without a matching stored run, both binaries run (cost ×2).
  - Every escalation runs the **full** frozen set. A subset chosen from screen results would be selected on outcome.
- **Multiplicity.** Each run reports every model, but the only gating comparison is the pre-declared **primary model** (Haiku 5.5)
  at Holm-adjusted α = 0.05 across the models of that run, confirmed by a rerun with fresh seeds (NH-5).
- **M2 is a rare-event count, not a test.** The probe saw 0 faults in 5 check-clean candidates. M2/M2s/M2f are reported as k/n with an
  exact (Clopper-Pearson) interval, and every M2 instance is listed (task family, panic kind, program sha). One M2s instance is a
  checker-soundness bug report whatever its rate.

## 4. Items

### AE-1 (P1) A provider-neutral model route with explicit limits (CERE and OpenRouter), and a CERE reply cache

**Anchors.**
- `genviab.py:816`: `--generator` choices `oracle|empty|micode`. `:411`: arm a's driver is `MICODE_DRIVER if micode else ORACLE_DRIVER`.
- `:73`: `MICODE_DRIVER`, called directly by arm c (`:551`).
- `drivers/micode-program.sh:47,54`: only `provider.model`, and `ANTHROPIC_BASE_URL` is unset.
- `drivers/micode_program_log.py:42-63`: the call row; `:83-84` sums input + output + cache tokens into `axon-intent-tokens`.
- `ab/run_ab.py:90-105`: `PRICES_PER_MTOK`, with `SystemExit` for an unknown model.
- `genviab.py:682-684`: `model_substituted`, micode only.

**Observed (§10).**
- micode reaches CERE with `--config provider.active=custom --config provider.base_url=http://127.0.0.1:18307/v1
  --config provider.model=Qwen3.8-Flash-Next` and the bearer token in `MICODE_API_KEY`.
- Through that route, 1 of 6 arm-a tasks (h08) became an **infra** row: micode reported `turn_length_truncated` at exactly 1024 output
  tokens. I found no micode key for the output cap in its `--config` key list. On micode's Anthropic route the cap is higher: the e01
  Haiku call row has 2255 output tokens.
- micode reports the cached prefix as `cache_read` (h08: 3979 input + 8192 `cache_read` ≈ the 11,637-token prompt), and totals and cost
  include it.

**Change.**
- New `drivers/http-program.py`: one OpenAI-compatible `/chat/completions` call per invocation. It takes the prompt on stdin and writes
  the reply verbatim on stdout.
  - Explicit `max_tokens` (default 8192), `temperature` and `seed`.
  - The output contract is the same as `micode-program.sh`: an `axon-intent-tokens: N` stderr line and exits 0/1/4.
  - The log row has the same keys plus `provider`, `base_url_host`, `temperature`, `seed`, `max_tokens`, `finish_reason`,
    `cached_tokens`, `reply_sha256`, `cache_hit`, and `cost_usd` when the provider returns one (OpenRouter `usage.cost`).
  - `finish_reason == "length"` at the declared `max_tokens` → the call succeeds with `truncated: true` (a model failure, §2). A cut
    below the declared cap (a transport limit) → infra.
- Providers are named, not free-form:
  - `cere`: base `http://127.0.0.1:18307/v1`, token from `/etc/cere/cere-t4.env` key `CERE_TOKEN_AE` (to be issued, NH-6), $0,
    deterministic.
  - `openrouter`: `https://openrouter.ai/api/v1`, key from `/root/.openrouter.env`, with `provider.data_collection: "deny"` in every
    request (D8).
  - The driver reads secrets from the file into its own environment, never argv, and never logs them.
- **Haiku 5.5 stays on `micode-program.sh`** (the OAuth grant, nominal $, the genviab convention). It has no temperature, seed or
  `max_tokens` control; its trials are not replayable.
- `genviab.py run --generator http --provider cere|openrouter`. Arm a (`:411`) and arm c (`:551`) both resolve the driver through one
  `driver_for(a)`, which maps `oracle`/`empty` to `oracle.sh` and nothing else. `model_substituted` (`:682`) applies to every model
  generator (`micode`, `http`).
- **CERE reply cache** (deterministic providers only). Key: (round prompt sha256, series id, temperature, seed, max_tokens, driver
  sha256). A hit returns the stored reply and logs `cache_hit: true`; grading always reruns on the binary under test. Never keyed on
  a binary sha (it changes every commit, `axon-core/src/main.rs:31`). Each run re-queries a seeded 2% sample of hits (≥ 20 calls); a
  `reply_sha256` mismatch disables the cache for the rest of that run and is recorded. The cache lives under `<private>/cache/`. Paid
  replies are never cached.
- `PRICES_PER_MTOK` gains `Qwen3.8-Flash-Next: (0, 0)` and each OpenRouter model in the set, priced from the OpenRouter models page on
  the day it is added (the existing convention, `run_ab.py:84-89`). `spent()` prefers a row's billed `cost_usd` over the estimate.
- `model_substituted` compares served and requested model after one normalisation rule per provider (OpenRouter's served `model`
  `[INFERENCE: may carry a dated suffix; recorded verbatim; the rule is fixed in the selftest]`).

**Fail-first / selftest** (`selftest.py`, no model). A fake HTTP server on a loopback port (stdlib `http.server`) serves canned replies:
- `--generator http` never invokes `oracle.sh` in arm a or arm c (red today: `:411` falls through);
- `finish_reason: length` at the declared cap → `truncated`, scored as a failed round; a canned cut below it → infra;
- an unpriced model → `Refused` before the first call;
- the token never appears in any file under the out dir (the check of `/var/tmp/r51-tmp/eg-cere/q1.sh:49`);
- `seed`/`temperature`/`max_tokens` reach the request body; the cached-token field is recorded;
- a second identical call is a cache hit with zero requests to the server; a forced replay with a different canned reply disables the
  cache and records the mismatch.

**Acceptance.**
- `genviab.py run --set dev --arm a --generator http --provider cere --model Qwen3.8-Flash-Next` runs the 28 dev tasks with 0 infra rows.
  The same with `--arm c`.
- Haiku through micode on the 5 longest T3 references' prompt sizes: 0 truncations below 8192 output tokens.
- `selftest.py` passes under `GENVIAB_REQUIRE_BINS=1`.

### AE-2 (P1) Rows carry per-candidate failure classes (check codes, run endings, panic kinds)

**Anchors.**
- `genviab.py:429-454`: the arm-a row keeps `round_stages`, `grade` and `b_grade`.
- `gv.py:306-321`: `grade_axon` returns `failed_examples` only; `:288`, `:298`: `sandbox_refused`, `no_outcome_record_runs`.
- `genloop.rs:754-790`: `ended` is `exit N` / `did not compile: ..` / `faulted (status ..): <verdict.legible()>` / `sandbox refused: ..`
  / `timed out after N ms`. `verdict.rs:63-87`: `legible()`, `Denied` prints `(axis: <axis>)`.
- `runtime.rs:1624-1642` `fault_verdict`: interpreter panics fall to axis `runtime`.
- `genloop.rs:141`: `report.diagnostics` are `axon-diag/1` lines.

**Change.**
- `gv.grade_axon` also returns `runs: [{example, run, which: visible|hidden, ended_class, panic_kind, passed}]`, with `ended_class` as
  in §2, parsed from `ended` (the axis for `faulted`).
  - `panic_kind` (for `ended_class == panic`) comes from a table `gv.PANIC_KINDS`, regexes on the panic reason: `type-class` (the
    checker should refuse: `cannot apply .* to`, `expected .* found`, `no match arm matched` on a type the checker typed), `partial-op`
    (index out of bounds, integer division by zero, explicit `panic`/`assert`), `other`. The regexes are written from the
    interpreter's panic strings at build time `[INFERENCE: not enumerated here]`; `other` is reported, never dropped.
- Arm-a rows gain: `diag_codes_per_round` (from the transcript), `check_ok_per_round`, `gates_ok_per_round`, `first_green_round`
  (accepting round if verified, else null), `b_check_ok`, `b_ccp`, `b_ccp_kind`, `b_runtime_fail` (M2f), `ccp_any_round` (visible
  runs, M2a), `truncated_rounds`, `budget_stop`.
- No grading-loop change is needed for coverage. The loop already runs every example (`genloop.rs:545-563`) and stops only the later
  runs of a failing example (`:829-831`, "later runs depend on this one's files"). So `runs` holds each example's first failing run, and
  M2 counts a candidate once whatever the number of faulting examples.
- Arm-c rows gain the Python analogue: `exception_type` per failed run and `syntax_error` (the existing `stage: "check"`, `gv.py:470`).

**Fail-first.** Four fixtures in `selftest.py`, graded through the real `axon`/`axon-intent`:
- `ccp-type.ax`, the O-LF-01 shape `match (1, "s") { (a, b) => to_str(b + 1) }`, which checks rc 0 and panics `cannot apply Add`
  (`tuple-patterns-spec.md:57`). Expected: `b_ccp = true`, kind `type-class`. Once TP-1 lands it becomes a check refusal, so the
  fixture pins whichever known-open residual is current at build time `[INFERENCE: e.g. tuple-patterns-spec.md:92]`.
- `partial.ax`: `v[len(v) + 3]` on a 2-element list (my probe: check rc 0, `panic: index 5 out of bounds (len 2)`, rc 101). Expected:
  kind `partial-op`.
- `steps.ax`: a loop that exhausts the step budget. Expected: `ended_class: resource`, `b_ccp = false`, `b_runtime_fail = true`.
- `wrong.ax` (clean exit, wrong stdout). Expected: `wrong-output`, `b_ccp = false`.

All four are red today: no field exists.

**Acceptance.** `genviab.py report` on the probe-shaped run prints the M1-M5 lines and an "ended class" table. The four fixtures pass.

### AE-3 (P1) The authoring task set `ae`: tiers with headroom, a pilot before freezing, kept out of the public repo

**Tiers.** One goal per tier. Sizes are for v1; candidates are authored at 1.5×.

| tier | purpose | n (v1) | target FT-pass | content |
|---|---|---|---|---|
| T1 regression | detect a change that breaks what works | 60 | ≥ 0.85 for the screen model | single-function transforms, parsing/formatting, `Result` error exits; the genviab a/e/h shapes |
| T2 program | the bulk of the headline | 80 | [0.4, 0.75] for the screen model and the primary model | multi-step file programs, state across runs, error paths, structs/enums/`match`, `Dict`/`[T]` |
| T3 hard | headroom for strong models | 60 | [0.15, 0.5] for the **strong** model | 80-200-line programs: several cooperating functions, generics, closures, invariants across steps, ceiling-limited I/O |
| TS soundness-leaning | raise the M2 base rate | 40 | any | goals whose natural solution uses the constructs open checker residuals live in (tuple destructuring in `match`, `Option`/`Result` payload patterns, mixed int/float arithmetic, string slicing, indexing); every ROADMAP §9.5 F-item gets ≥ 1 task |

- **Total:** 240 tasks. A *family* is one source idea; ≤ 2 variants (≤ 4 if the pilot's ρ_fam < 0.3, §3). Clusters for §3 are families.

**Sources.**
- Ideas only (never code) from: genviab's task shapes (`dev/`, 28); `examples/asi/` (42 top-level `.ax`) and the PM11 template gallery
  domains (`AI_Language_Plan.md:2122-2125`: support triage, invoice extraction, research summarizer, runbook bot, without the AI calls,
  O-AE-02); compilebench's 9 programs (`/home/cklaus/projects/aicoding/compilebench/benchmarks/`, golden outputs) for T1 only; open
  soundness residuals (`opportunities.md` O-LF/O-TP entries) for TS.
- Tasks are new text with new data. A public repo program is never a task.

**Format.** genviab's `intent.md` + `task.json` (`build.py` checks, `build.py:322-343`), plus:
- every task has ≥ 1 visible example (never copy the atlas property, README `:77-81`); hidden examples stage inputs no visible run
  stages (B3-2);
- `## Budget` `tokens: 120000` (§2);
- **two** references, `reference.ax` and `reference.py`, must agree on every example (a cross-check that replaces "a human computed
  the value");
- `task.json` carries `family`, `tier` and a `canary` GUID. The canary is never rendered into a prompt.

**Change (loader and checks; Lane 1 files).**
- `gv.load_task` and `build.py` gain an `ae` set mode, selected by a set root that is not `dev`/`once` (`build.py --set-dir <dir>`,
  today only `--check`/`--set`, `build.py:63`, `:464-465`). In it: exactly `reference.ax` and `reference.py` are required (today two
  references are refused, `build.py:112-117`, and blank `reference`, `gv.py:85-87`); `reference` is the `.ax` one (the oracle
  generator, `genviab.py:410`, needs it); a new check `refs-agree` grades both on every example; `family`/`tier`/`canary` are
  required; `tokens` ≥ 120000; `check_once` and `check_human_value` are skipped.
- Selftest: a two-reference fixture loads with `reference` = the `.ax` text; a disagreeing pair fails `refs-agree`; a missing canary
  fails.

**Authorship and review.** Drafted by a model or a person (NH-3). Each task is reviewed by a human for ambiguity before the pilot.
Tasks that every pilot model fails on every trial get a second review.

**Contamination.** The repo is public on GitHub (`https://github.com/Cklaus1/axon.git`, `genviab.py:92`). Anything committed there can
enter a future model's training data, and the eval would then measure memory.
1. **Storage.** Task text, examples, references, per-task rows, transcripts and the CERE reply cache live outside the public repo: a
   private repo (NH-4 default `Cklaus1/axon-ae-private`), mirrored at `/root/axon-ae/`.
2. **What the public repo holds:** the harness, the manifest's sha256, per-tier counts, opaque task ids (`ae-<sha8>`), and aggregate
   results only. No candidate programs: they reveal solutions.
3. **Canary.** AE-4 greps every reply for **every** `ae` canary and flags a canary from a *different* task than the prompt's (a reply
   cannot learn it from its own prompt, which never contains one anyway). AE-4's selftest greps `git ls-files` content for every
   canary.
4. **Provider exposure.** Paid calls send task text to providers. OpenRouter requests set `data_collection: "deny"` (AE-1); whether the
   Anthropic OAuth route trains on traffic is not established `[INFERENCE]`. Every paid call is counted per task (`exposures`). A task
   with more than 50 paid exposures is due for rotation `[INFERENCE: threshold is a judgement call; recorded so it can be tuned]`.
5. **Retirement into public.** Retired tasks may be published to `ae-dev` (public, for harness work). Once public, never scored again.

**Pilot, then freeze** (`PREREG-ae.md`, modelled on `headroom/PREREG-hx3.md`):
- **Pool:** 360 candidates. CERE k = 3 and Haiku 5.5 k = 1, arms a and c. Opus 5.5 k = 1, arm a, on the 90 T3 candidates only (NH-2).
- **A/A run:** same binary, CERE seeds {0,1,2} vs {3,4,5}, and Haiku k = 1 twice, on the full pool. It gives the clustered null SD
  directly, the escalation trigger (§3) and ρ_fam.
- **Defect gates** (task-level, outcome-blind for difficulty): the oracle verifies; the empty program fails; `refs-agree`; a task every
  model fails on every trial goes to a second human review and is fixed or dropped *for ambiguity only*.
- **Difficulty is tuned at tier level, never by dropping tasks a model failed.** If a tier's pilot FT-pass is outside its band, it gets
  more or fewer harder variants by a pre-registered knob (size, number of steps, error paths) and is re-piloted once. Still out of band
  after that: the tier ships at its measured rate and PREREG records it; if T2 or T3 is ≥ 0.85 for its band model, the set is not
  frozen (no headroom, the O-IG-03 failure).
- Then a seeded draw takes each tier's n from the survivors, with the seed in PREREG before any pilot call.
- The pilot re-estimates ρ and ρ_fam and reruns `calib2.py` on its rows; if the screen's power at 5 points is < 0.8, k rises before n
  does. PREREG records the measured s/trial and calls/trial, and §6 is rewritten from them.
- **Freeze:** commit `PREREG-ae.md` (FROZEN) with the manifest sha256, seeds, k per model, the primary model, the §2 definitions and the
  A/A null SDs. After the freeze it only gets appended rows.

**Versioning and retirement.**
- The set is `ae-<major>.<minor>`. A minor bump is defect fixes; tasks fixed after the freeze are re-run on the previous binary in the
  same run. A major bump is rotation. Δ is computed only within one version.
- A **bridge-set run** (old and new versions, same binary, same models) links the trend line across a major bump.
- **A tier saturates** when **any** model's tier FT-pass (screen, primary or strong) is ≥ 0.85 on T2/T3 on its last two runs. Then the
  tier's 25% most-solved families are retired to `ae-dev`, replacements come from the piloted reserve, and the major version bumps.
- T1 is meant to be near-saturated and is never retired for that reason.

**Acceptance.**
- 240 tasks frozen; `build.py --check --set-dir <private>/ae-1` exit 0.
- `PREREG-ae.md` holds the pilot table (per tier: FT-pass and Solve@3 per pilot model, ρ, ρ_fam, A/A null SD, power at 5/8 points).
- No task text or canary under the public repo (AE-4's selftest).

### AE-4 (P1) `ae.py`: the paired multi-model runner, the trend file and the report

**Change.** New `crates/axon-intent/benchmarks/genviab/ae.py`. It imports `gv`/`genviab` the way genviab imports `analyse` and
`run_ab` (`genviab.py:64-69`). No second grader.

- **Binaries are a parameter.** `gv.AXON`/`gv.AXON_INTENT` are read at import (`gv.py:46-48`), and `run_arm_a` (`genviab.py:414`),
  `gv.lift`/`grade_axon` (`gv.py:262-273`) and `card_sha256` (`genviab.py:149-153`) read them. Each gains a `bins` argument
  (default: the env globals), so one process alternates binaries without mutating globals under `--parallel`.

`ae.py run --set-dir <private>/ae-1 --label <release> (--base-bin <dir> | --base-run <run_id>) --cand-bin <dir> --models <file>
--cap-usd X`:
- For each (model, arm a, binary, trial) it runs `run_arm_a` on the frozen set. Rows go to
  `<private>/runs/<run_id>/<model>/<bin>/rows.jsonl`. Binaries alternate per task so that load or drift hits both equally.
- `paired_mode` (§3) is decided before the first call from the per-task round-0 prompt sha256 of both binaries (the prompt is
  rendered locally; no model call). In `regrade` mode the round-0 reply is generated once and graded under both.
- Arm c runs once per (model, run): Python does not depend on the binary.
- **Refuses when:** the manifest sha differs from PREREG; `--cap-usd` would be exceeded (the pre-task check of `genviab.py:661-670`);
  the models file names an unpriced model; or `--base-run` names a run on another set version or series.
- **Writes no trend row when:** infra > 5%; the run was cut short (the AE-7 window or the cap, `INCOMPLETE`); or the base and
  candidate cards are byte-equal while `--expect-card-change` is set.

**Trend file.** `crates/axon-intent/benchmarks/genviab/ae/history.jsonl` (public, aggregates only), one row per (run, model), appended
like `scorecard.py:1084`. Schema `axon-ae/1`:

```
{"schema":"axon-ae/1","run_id","date","label","design":"paired"|"unpaired-time"|"bridge-model"|"bridge-set",
 "paired_mode":"regrade"|"resample"|null,"set":{"version","manifest_sha256","n":{"T1","T2","T3","TS"}},
 "driver_sha256","prereg_sha256",
 "base":{"git_head","axon_sha256","axon_intent_sha256","card_sha256","prompt_set_sha256"} | {"run_id_ref"},"cand":{...same},
 "model":{"series","requested","served","provider","models_entry_sha256","temperature","seed_policy","k"},
 "metrics":{"cand":{"ft_pass","ft_check","ccp":[k,n],"ccp_soundness":[k,n],"runtime_fail":[k,n],"ccp_any_round":[k,n],
            "solve_at":[s0,s1,s2,s3],"turns_mean","false_acceptance":[k,n],"truncated":[k,n],"budget_stop":[k,n],
            "per_tier":{...}},"base":{...same}},
 "paired":{"delta_ft_pass","ci95":[lo,hi],"p_two_sided","p_holm","n_tasks","n_families","delta_solve3"} | null,
 "flag":"REGRESSION"|"IMPROVED"|"NO CHANGE"|null,"t1_regression":bool,
 "python":{"ft_pass","solve3","gap_ft","gap_solve3","drift_vs_last"} | null,
 "classes":{"check":{"E0001":n,...},"panic":{"type-class":n,...},"ended":{...}},"classes_delta":{...},
 "infra":[k,n],"cache":{"hits","misses","replayed","mismatches"},
 "cost":{"usd_billed","usd_nominal","calls","tokens_in","tokens_out"},"wall_s","rows_sha256","notes"}
```

`prompt_set_sha256` = sha256 of the sorted per-task round-0 prompt sha256s for that binary.

**Report** (`ae.py report <run_id>`). One table per model:
- FT-pass base → cand with Δ [95% CI], two-sided p and Holm p, then a flag:
  - `REGRESSION`: the headline CI is entirely below 0;
  - `IMPROVED`: entirely above 0;
  - `NO CHANGE` otherwise;
  - `T1-REGRESSION` is printed beside it when also T1 Δ < 0 (advisory, descriptive);
  - an `unpaired-time` row prints the difference and `drift: arm c Δ = x` and no flag.
- FT-check, CCP k/n with every instance (family, kind, program sha), M2f, Solve@0..3, turns, false acceptance, truncated, budget stops,
  the Python gap, per-tier rows (marked descriptive), infra, cache hits/misses, cost and wall.
- Then the top 10 failure classes with base/cand counts (AE-5).

**Fail-first / selftest** (in `selftest.py`, gate stage S23, `scripts/gate.sh:1227-1231`). Oracle and empty generators over a 6-task
fixture set, two copies of the same binary:
- Δ is exactly 0 and `paired_mode` is `regrade`;
- the oracle FT-pass is 1.0 and the empty one 0;
- a task's base row is graded with the base binary (two fixture binaries that differ only in a marker in `--version`);
- the CI call passes `alpha=0.025` and p is two-sided (a fixture with a known sign-flip p);
- the trend row validates against the schema; a row whose `rows_sha256` mismatches is refused;
- a canary in a fixture file under `git ls-files` fails the check.

The AE-1/2/4 additions keep S23's added wall under 60 s on the gate host; the lane measures it once and records it in the README.

**Acceptance.**
- One CERE screen on two real binaries writes one trend row and a report.
- A second invocation with the same seeds reproduces CERE FT-pass exactly (with the cache off), or the report names the replies whose
  sha differs (D4).

### AE-5 (P2) Failure classes rank the work

**Change.** `ae.py classes <run_id>...` aggregates, over all models and runs since the last set version:
- **(a)** round-0 check errors by `axon-diag/1` code, with the first 80 chars of each message normalised (names stripped). Example from
  the probe's a07 run: `E0000 unexpected token: Fn`, `E0001 cannot find name 'arr_new'` (a hallucinated builtin), `E0102
  if-without-else body`.
- **(b)** M2 by panic kind and construct; M2f by `ended_class`.
- **(c)** **stall rate** per code: the share of rounds whose code reappears in the next round (PM12's "diagnostics that most often
  stall a model").

Each class maps to an owner area in a checked-in table `ae/classes.json`:

| class | area |
|---|---|
| E0001 on a name not in the card | card/docs |
| E0001 on a real builtin | naming/stdlib |
| E0000 | syntax, or the card's grammar |
| type codes | checker or diagnostics |
| M2s | checker soundness |
| `partial-op` | stdlib totality |
| wrong output | task or model, no action |

**Promotion rule.** A class becomes, or updates, an `opportunities.md` entry with its tally when it accounts for ≥ 3% of scored
round-0 task-trials or ≥ 5 families, **and** appears for ≥ 2 models. The entry states the **upper bound** on FT-pass gain (the class's
share of round-0 failures). Every M2s instance is logged regardless of rate.

**Public text only.** A public entry carries the code, class, counts, and families as opaque ids. An M2s repro is a minimised program
written by hand, not derived from task text.

**Measured effect.** The class's task subset is fixed from runs **before** the fix. After the fix ships, that subset is re-run as a
fresh paired pair (pre-fix binary and post-fix binary, same session, new seeds), and that paired Δ is appended as the measured
effect. A Δ against the original failing run is never reported: those tasks were selected for failing and regress toward their mean.

**Acceptance.** On a run with seeded fixtures, the class table, the stall rates and a draft opportunities stanza are printed. The
stanza is not written to `opportunities.md` automatically, and contains no task text.

### AE-6 (P2) The Python baseline arm: run it every release for cheap models

- **What it adds:** the product's "how far behind Python" number (M5), and a **drift control**. Python does not change between Axon
  releases, so a move in arm c on the same tasks means the model or the endpoint moved. That is what makes the strong model's
  `unpaired-time` comparison (§3) readable; it detects drift but does not correct for it.
- **Cost:** 12-18% of arm a's on dev tasks (README results table: $0.0036/$0.0308, $0.0042/$0.0309, GV-4 $0.0071/$0.0388), less on the
  band set where arm a makes more calls. Probe: arm c calls used 275-661 input tokens against ~11.6k for arm a's card + task (§10).
  About $0.0003-0.0006 per task with Haiku.
- **Recommendation:** every release, k = 1, for every model except the strong one; for the strong model, on every escalation (it is
  the drift control there). Never twice in one run: it depends only on (model, set).

**Acceptance.** `history.jsonl` rows carry `python` for those cases and `null` otherwise. The report prints the gap with a paired CI
over tasks and `drift_vs_last` (arm c Δ against the model's previous run on the same set version).

### AE-7 (P3) Weekly CERE screen

A systemd timer on **gpumaster** (the host of `cere-nightly-gpu.timer`) runs `ae.py run` on the CERE model weekly, Saturday 12:00
local, never 01:30-08:00 (`cere-nightly-gpu` starts 02:00; cron starts `trainloop-daily` at 02:00 and `resolve-pairs` at 02:47). It
uses `--parallel 3` and its own CERE token (NH-6). It aborts as `INCOMPLETE`, writing no trend row, if not done by 20:00.

It compares the last labelled release with `main` HEAD. Binaries are built from a clean worktree into a dedicated `CARGO_TARGET_DIR`
and cached by commit under `<private>/bins/<sha>/`; the base is never rebuilt. With the reply cache (AE-1), a week whose prompts did
not change makes only the replay sample's calls. It appends a trend row with `label: "weekly-<date>"`. It never escalates by itself:
an escalation trigger (§3) goes into the weekly digest for the integrator.

**Acceptance.** Two consecutive weekly rows exist. A week with no merged change makes 0 cache-miss calls apart from the replay sample
and shows `NO CHANGE`.

## 5. Shared mechanics

- **Cadence.** Weekly CERE screen (AE-7). Per release, the cheap paid pair (Haiku 5.5 + one non-Anthropic model via OpenRouter,
  NH-1), both binaries. The strong model quarterly, or on escalation. Until O-PM-04 defines releases, a "release" is a commit the
  integrator labels.
- **Pins per run:** the `axon` and `axon-intent` sha256, `card_sha256`, `prompt_set_sha256`, the driver sha, `micode_version` where
  used, the model series (served id, CERE `/v1/models` entry sha), the manifest sha and the PREREG sha.
- **Re-run rule.** A CERE reply is reused when its cache key matches (AE-1). Grading always reruns. A paid reply is never reused,
  except that a stored round-0 reply is re-graded in `regrade` mode when the prompts match (§3).
- **Private/public split.** `<private>/runs/` holds rows, transcripts and candidate programs; `<private>/cache/` the CERE replies;
  public `history.jsonl` holds aggregates. `rows_sha256` links the two without revealing either.

## 6. Gates and cost budget

All wall and call figures for band-set tasks are `[INFERENCE]` from the probe (outputs 319-1024 tokens, 1.7-5.6 s per call on CERE,
so ≈ 130-190 tokens/s per stream) and the tier bands (1-3k-token outputs, ≈ 1.7 calls per trial). PREREG replaces them with the
pilot's measured values. Prices are `run_ab.py:91-92` list prices, themselves unverified.

| stage | wall | $ per run | scarce resource | source |
|---|---|---|---|---|
| selftest (S23 additions) | ≤ 60 s added | 0 | gate CPU | AE-4 |
| pilot, CERE k = 3, arms a + c, 360 tasks, one re-pilot, A/A (seeds 3-5) | 3-6 h, split by tier across windows | 0 | CERE GPU, outside 01:30-08:00 | 8-20 s/call, 1.7 calls/trial |
| pilot, Haiku k = 1, arms a + c, 360 tasks, plus A/A | 1-1.5 h | ≈ $4 nominal | OAuth grant | $0.0027/call (15.8k in + 2.3k out at $0.10/$0.50, e01 row) × 1.7 × 720 + arm c |
| pilot, Opus k = 1, arm a, 90 T3 candidates | ≈ 1 h | ≈ $20 nominal, cap $25 | OAuth / paid $ (NH-2) | $0.109/call × ≈ 2 calls × 90 |
| weekly screen, CERE k = 3, 240 tasks | 1-2 h with cache misses; ≈ 10 min when nothing changed | 0 | CERE GPU, Saturday 12:00-20:00 | AE-1 cache, AE-7 |
| per release, Haiku k = 1 (`regrade` when prompts match, else 2 binaries) + arm c | 0.5-1 h | $1.5 (regrade) to $3 nominal | OAuth grant | 240-480 trials × 1.7 × $0.0027 + 240 × $0.0005 |
| per release, one OpenRouter model | 0.5-1 h | ≤ $5 billed (cap) | paid $ | price unknown until NH-1 |
| escalation, Opus k = 1, new binary + arm c | 1-2 h | ≈ $40-65 nominal; cap $75 | OAuth / paid $ | $0.109/call × 1.5-2.2 calls × 240 + ≈ $0.02 × 240 arm c |

Every paid run sets `--cap-usd` to `spent` + the line above and a reserve of 3× one task's estimate (the genviab convention, README
`:121-123`). A cap-interrupted run is `INCOMPLETE`. Authoring 360 tasks with two references each is the dominant cost and is not a $
line: model drafting plus human review time (NH-3).

## 7. Sequencing and lanes

1. **Lane 1 (harness):** AE-1, then AE-2 (one lane; both edit `genviab.py`/`gv.py`/`selftest.py`), then AE-3's loader/`build.py`
   change, then AE-4, then AE-5. No paid spend. Verified on the dev set with CERE only. No `crates/*/src` change: only gate stage S23
   covers it.
2. **Lane 2 (set):** AE-3 authoring starts in parallel once §2 and the task format are fixed. The pilot needs Lane 1 through AE-4.
3. **Integrator:** freezes `PREREG-ae.md`; runs the first per-release pair (base = current `main`, candidate = the next labelled
   commit; a card-only change is the cleanest first effect to measure); turns on AE-7 after two hand runs.

AE-6 is a policy inside AE-4 and needs no lane.

## 8. Engineering decisions (canon for the build)

- **D1** AE lives in `genviab/` (`ae.py`, `ae/`, `PREREG-ae.md`) and reuses `gv.py` grading and `run_arm_a`/`run_arm_c`, with the
  binaries as a parameter. The IG7 once path, ledger and verdict are untouched.
- **D2** The headline is M1 FT-pass (round-0 verified on visible + hidden). Solve@3 is secondary.
- **D3** Turn cap = `MAX_REPAIR_ROUNDS` (3) with genloop's feedback; `ae` tasks budget 120000 tokens. No AE-specific prompt: the
  prompt is the binary's.
- **D4** CERE runs at temperature 0.7 with seed = trial index, max_tokens 8192, records `reply_sha256`, and caches replies by prompt
  sha with a 2% replay.
- **D5** Pairing: same tasks, seeds, model and driver, both binaries in one session, alternating per task; `regrade` when round-0
  prompts match.
- **D6** The test is the family-clustered sign-flip of `headroom/stats.py`, two-sided p = 2·min of the one-sided p's, 95% CI =
  `ci(alpha=0.025)`. Gating uses the primary model at Holm-adjusted α = 0.05 plus a confirmation rerun.
- **D7** Infra rows are excluded from denominators. A run with > 5% infra is invalid.
- **D8** The held-out set and the reply cache are private. The public repo gets aggregates and opaque ids. OpenRouter calls carry
  `data_collection: deny`.
- **D9** A cut below the declared `max_tokens` is infra; `finish_reason == length` at the declared cap is a model failure (`truncated`).
- **D10** M2 counts `axis: runtime` faults only; M2f is the broader runtime-failure column. Both are counts with instances, never
  gated on a p-value.
- **D11** Δ is computed only within one set version and one model series; bridges link them.

## 9. Needs human

1. **NH-1 Model set.** Options: (a) CERE only; (b) CERE + Haiku 5.5; (c) (b) + one non-Anthropic mid-tier model via OpenRouter;
   (d) (c) + Opus 5.5 every release. **Recommend (c)**, with Opus quarterly or on escalation: authoring friction is model-specific, and
   one model from another lab keeps the number from tracking one vendor's habits. Haiku 5.5 is the primary (gating) model.
2. **NH-2 Paid budget.** Options: (a) ≤ $5 per release; (b) ≤ $10 per release, an Opus escalation ≤ $75 approved per use, and a
   one-time pilot ≤ $30 (Haiku ≈ $4 + Opus T3 ≈ $20); (c) a fixed monthly envelope. **Recommend (b).** The Haiku pair is ≈ $1.5-3
   nominal (OAuth) and the OpenRouter model is capped at $5.
3. **NH-3 Task mix and authorship.** No customer answers have been given. Proposed default: the AE-3 tiers, with T2/T3 domains from the
   PM11 gallery (data/file pipelines, triage, extraction without AI calls, runbook steps). Authorship options: (a) human-written (slow,
   the IG6 standard); (b) model-drafted, cross-checked by two references, human-reviewed; (c) model-drafted only. **Recommend (b)**, with
   the drafting model's own scores reported separately for the first set version: h01-h12 were written by Opus 5.5 and Haiku saturated
   them (README `:73-74`, `:112-125`).
4. **NH-4 Private storage.** Options: (a) a private GitHub repo; (b) a host-local directory with backups; (c) encrypted files in the
   public repo. **Recommend (a)**: it has history and access control, and (c) still publishes the ciphertext forever.
5. **NH-5 Does it gate anything?** Options: (a) advisory only; (b) advisory for the first 3 releases; then a release is blocked only
   when the primary model (Haiku 5.5, paired, same session) shows headline `REGRESSION` at Holm-adjusted α = 0.05 across the models run
   **and** a confirmation rerun with fresh seeds reproduces CI < 0, unless a human waives it. Per-tier (`T1-REGRESSION`) and other
   models' flags stay advisory. Every new M2s instance needs an opportunities entry before the release (a bookkeeping rule, not a
   statistical gate); (c) block on any model's headline regression. **Recommend (b).** Gating before the noise is measured on the real
   set would block on chance; per model and per tier, 3 models × 5% false flags is ≈ 14% per release.
6. **NH-6 CERE credential and window.** Options: (a) reuse `CERE_TOKEN_TRAINLOOP` (another lane's credential; AE load cannot be told
   apart on the endpoint); (b) the operator issues `CERE_TOKEN_AE` in `/etc/cere/cere-t4.env`, and AE runs only Saturday 12:00-20:00.
   **Recommend (b).** `[INFERENCE: whether port 18307 shares a GPU with the nightly suite's tickets was not observed]`.

## 10. Verification log (what I ran)

Scratch: `/var/tmp/r51-tmp/EvalSpec/`. Binaries: `eg-bin/axon` `557e746c`. No cargo build, no paid call, no commit.

| # | command | result |
|---|---|---|
| 1 | `curl http://127.0.0.1:18307/v1/models` | `Qwen3.8-Flash-Next`, `max_model_len` 262144 |
| 2 | `micode exec ... provider.active=custom provider.base_url=…18307/v1` without a token | HTTP 401 "missing or invalid bearer token" |
| 3 | the same with the token from `/etc/cere/cere-t4.env` in `MICODE_API_KEY` | reply ok; 48 in / 2 out tokens |
| 4 | `micode exec --config zz.bogus=1` (key list) | `provider.base_url`, `provider.active`, `provider.model`, ...; no output-cap key found |
| 5 | `probe.py run --set dev --arm a --generator micode --model Qwen3.8-Flash-Next --only a07,a15,h03,h06,h08,h10 --parallel 3` (genviab.py unchanged; in-process patches: driver → `cere-program.sh`, $0 price) | 13.3 s wall. 5 non-tempting rows, 1 infra (h08: micode `turn_length_truncated` at 1024 output tokens; row has 3979 in + 8192 `cache_read`). Scored: verified 4/4, arm b 3/4, false acceptances 0. Calls 8, 1.7-5.6 s each. a07 needed 3 rounds: E0000 → E0001 (`arr_new`) + E0102 → accepted. 7 candidates, 5 check-clean, 0 faulted |
| 6 | `probe.py run --set dev --arm c ... --only a07,a15,h03,h06` | 4.6 s wall, 4/4 verified, 5 calls, 275-661 input tokens per task |
| 7 | `determinism.sh` (4 direct calls: card + one task prompt) | prompt 11,637 tokens. Temperature 0 ×2: same reply sha `7cb232c35356`. Temperature 0.7 seed 1 ×2: same `83ce71ac54a9`. Cached 0, then 8,192. 0.84-1.6 s. Token not in any output file |
| 8 | `calib.py` (recorded rows only) | hx3 n5a DEV-H55 109×3: mean 0.807, ρ 0.803, beta a=0.20 b=0.05. FU-1 DEV-H55 182×3: mean 0.998. GV-4 round-0 replicates 9/9, 7/9, 7/9 |
| 9 | `axon card \| wc -c -l`, `wc` on `spec/axon-for-llms.md` | 31,582 B / 534 lines; 19,045 B / 403 lines |
| 10 | `calib2.py` (no model; numpy sign-flip, B = 2000) | the §3 table. Check vs `stats.SignFlip` (mc 4000): 0.180/0.189, 0.0010/0.0010, 0.0015/0.0030. Wall 117 s |
| 11 | `r1/p1-p4.ax` with `eg-bin/axon check`/`run` | `unwrap(x)` E0001 rc 2; `x.unwrap()` E0403 rc 2; `v[len(v)+3]` check 0, `panic: index 5 out of bounds (len 2)` rc 101; `10 / z` (z = 0) check 0, `panic: integer division by zero` rc 101 |
| 12 | `systemctl list-timers`, `crontab -l`, `hostname`, key names in `/etc/cere/cere-t4.env` | `cere-nightly-gpu.timer` 02:00; cron `trainloop-daily` 02:00, `resolve-pairs` 02:47; host `gpumaster`; keys `CERE_TOKEN_TRAINLOOP`, `CERE_OPERATOR_TOKEN` |
| 13 | re-read of review anchors | `build.py:112-120`, `:464-465`; `gv.py:46-48`, `:85-87`; `genviab.py:84-86`, `:405-415`; `stats.py:43`, `:100-167`; `runtime.rs:1262-1292`, `:1622-1642`; `verdict.rs:63-87`; `genloop.rs:320-330`, `:505-542`, `:752-792`; `axon-core/src/main.rs:26-34`; `PREREG-genviab.md:58-66`; README results table; `scripts/gate.sh:1225-1233`; `gate-lanes.json:69`; e01 Haiku call row (15816 in / 2255 out) |

Total CERE task-runs: 10 (6 arm a + 4 arm c), plus the 4 direct calls and 2 one-line micode checks.

**Not verified:**
- wall times at 240 tasks on band-set tasks, and CERE under other lanes' load; whether different seeds give different replies;
- OpenRouter prices and served-model naming; Haiku/Opus list prices;
- the interpreter's full panic-string list for `PANIC_KINDS`;
- ρ and ρ_fam on authoring tasks (the calibration used cortex repair rows), and power for an effect confined to a few families;
- the journal durations of `cere-nightly-gpu` (reported by DevloopAE, not re-read), and whether 18307 shares its GPU;
- whether the Anthropic OAuth route trains on traffic.
