# DRAFT — Intent → checked program: the generation loop and its viability benchmark (IG)

**This draft MUST pass a Step 1 adversarial review before any build.** Nothing here is approved.

Base: `main` f83c6f86 (2026-10-18). Sources: VISION.md north star ("written by stating what you want and approving
what the machine proposes"); `governance/specs/R38-embedded-agent-runtime.md` §3 row "Generation viability … ❌
unvalidated — the load-bearing hypothesis" and §9 (`embed_generation_viability_measured`); the atlas RLM spike
(`/home/cklaus/projects/aicoding/atlas/spikes/rlm-engine/`, separate git tree, MIT); the cortex benchmark harness
(`crates/axon-cortex/benchmarks/`).

Mission: an operator states an intent with a capability ceiling, and `axon-intent compile` returns either a program
that type-checks, stays inside the ceiling and passes the intent's own examples, or an honest refusal with the full
transcript. It never returns an unchecked program and never falls back to a stub. Then measure, pre-registered and
kill-gated, whether that works on realistic tasks.

Shared acceptance: `scripts/gate.sh` shows no new failure against the f83c6f86 baseline; every behaviour change has a
fail-first test; the loop's tests run with a deterministic fake generator (no network, no key); refusal coverage and
g01 mutation rows stay green, with new rows for the loop's accept/refuse decisions; CHANGELOG, `AXON-COMPLETENESS.json`
(rows for axon-intent and the intent pipeline), `spec/axon-for-llms.md` and R38 §9 updated by the integrator.

## Re-measurement (2026-10-18, this tree)

| # | Fact | Evidence |
|---|---|---|
| F1 | `axon intent compile` generation is one `axon_ai::complete` call with a generic prompt: no language card, no `axon check`, no repair round; an AI error writes the TODO skeleton with a warning. It exists only under `asi-runtime`, which the default build lacks, so a default `axon` always writes the skeleton | `crates/axon-core/src/main.rs:8226-8270`; `crates/axon-core/Cargo.toml:34,44` |
| F2 | `axon_surface::compile::emit` produces one shape only: a prompt-optimisation hill-climb (`try_variant` calling `ai_complete`, `goal_run`). It is not a skeleton for a general effectful program | `crates/axon-surface/src/compile.rs` |
| F3 | **The production synthesis path cannot produce a program.** `RealSynthesizer` writes axon-intent markdown (`render_intent_md`) and hands it to `axon intent compile`, which parses the different GoalFile format. Reproduced on the f83c6f86 binary with exactly the text `render_intent_md` emits: `error: intent file invalid: parse: missing required section(s): Intent, Score, Constraints, Verify, Redteam, Effect surface, Provenance`, rc 2. `RealSynthesizer` then reports "produced no program" → exit 5. Every acceptance test uses `MockSynth`; the only real-path test uses a fake `axon` that hangs | `crates/axon-intent/src/synth.rs:261-372,378-408`; `tests/acceptance.rs:385-422` |
| F4 | `MockSynth`'s "type-check" is a brace-balance and entry-point heuristic, not the compiler | `synth.rs:420-483` |
| F5 | The only repair loop in the tree is `cortex repair`, which repairs one function body in an existing file | `crates/axon-cortex/src/bin/cortex.rs` |
| F6 | Prior fluency measurements (atlas spike, `claude-sonnet-5`, ≤ 1 repair round, no cost recorded): R9, 8 pure scalar tasks, saturated at 8/8 with an accurate card, and one task is defective (`r9.rs:37` expects "salta" for "micode" reversed). `tasks_hard`, 12 tasks with file I/O: 3/12 first-try in 3 runs, ≤ 4/12 after one repair; 21 of 23 generated files byte-identical across runs, so effective n ≈ 1. No task has a capability ceiling, a network effect or hidden tests | `atlas/.../results/axon-hard-2026-08-27-FINDINGS.md`; `src/tasks_hard.rs` |
| F7 | R38 §9 already pre-specifies a viability gate: N ≥ 30 realistic tasks, one frontier model, ≤ 3 `axon-diag/1` repair rounds, kill if < 80 % check clean and behave correctly, a Python-in-isolate baseline on the same oracle (gap > 10 points = adverse finding), re-measurement only as a fresh pre-registered run. §4: a capability refusal ends the loop and does not consume a round | R38 `:255-275`, `:394-470` |
| F8 | Reusable grading and accounting: `keyed_results` (HMAC completion key, defeats forged result lines and `exit(0)`), `analyse.mcnemar_exact`/`wilson`, `spent()` spend cap with per-task reserve, PREREG conventions (committed alone before any paid trial; amendments dated, verdicts never edited) | `crates/axon-cortex/benchmarks/ab/run_ab.py:143,740`; `ab/analyse.py:21,31`; `scorecard/PREREG-*.md` |
| F9 | Diagnostics: `axon check` writes one `axon-diag/1` NDJSON object per diagnostic on stderr (`schema,severity,code,file?,line?,col?,message,expected?,found?,help?`), exit 0 clean / 2 on any. `axon test --json` per-test lines + summary, exit 0/3/2 | `main.rs:1335-1477`; `lib.rs:291-345`; `main.rs:6838-6889` |
| F10 | Deterministic effects: `AXON_RECORD`/`AXON_REPLAY` host journal (fs, http, exec, env, stdin; miss → exit 11), `AXON_AI_REPLAY` keyed by (prompt, model), `AXON_AI_MOCK` a fixed non-Axon stub; no HTTP mock; `http_*` denied without `asi-runtime` | `env_registry.rs:24-55`; `replay.rs:1-79`; `host.rs:107-116` |
| F11 | The intent format (`# Intent`, `## Inputs` file paths, `## Outputs`, `## Allowed` → `CapCeiling`, `## Budget`, `## Seed`) and the downstream pipeline (`confidence::score`, `grant_infer::infer` → `GrantExceedsCeiling`, `prove_admissible`, approval token `axon-approval/3`) already exist and are tested on mock programs | `crates/axon-intent/src/{intent,pipeline,confidence,grant_infer,approval}.rs` |

Consequence: the step VISION.md rests on has never produced a checked program from an intent in this repo, and the
only realistic measurement (F6) is weak and small. Everything downstream (approval, admission, axon-os, attestation)
is tested on hand-written or mock programs.

## Items

### IG1 — Examples in the intent format (critical path)

Add an optional `## Examples` section to the axon-intent format: each example names input fixture files (path under
an `fs_read` prefix → contents) and expected output files (path under an `fs_write` prefix → exact contents) and an
expected exit code. Examples are the loop's visible acceptance check. Fixture contents may be inline (fenced) or a
path relative to the intent file.

- Parse errors are `Malformed` (exit 2) with the offending line; a fixture path outside the ceiling is refused at
  parse, as `..` is today.
- An intent without examples still compiles, but the loop's acceptance is then only check + ceiling, and the
  confidence rubric records "no examples" (it cannot reach the top band).
- Test: parse round-trip; a path outside the ceiling refused; an example with no outputs refused.

### IG2 — A language card generated from the compiler

`axon card` prints a language card for LLM prompts: a curated syntax section (from `spec/axon-for-llms.md`: `Option`/
`Result`, `match`, no `mut`, line-break rules, `fn main() -> i64`, file builtins return `Result`) followed by every
builtin's signature and one-line doc, generated from the builtin table, plus the `@[contained]` syntax. Atlas
measured that a card's omissions read as absences to the model (F6 and `opportunities.md` "the LANGUAGE CARD … named
36 of 331 builtins"); generation removes that failure class mechanically.

- Every fenced example in the curated section type-checks (a test runs `axon check` on each).
- The card is deterministic (byte-identical across runs) and its sha256 is recorded in every transcript and
  benchmark row.
- Size budget recorded in the test: [INFERENCE] about 25–30 KB, 7–8 k tokens; if it exceeds 40 KB the builtin list
  is grouped by module with one line each.

### IG3 — A generator seam in axon-intent

A `Generator` trait in axon-intent: `id()` and `generate(prompt) -> Result<String, GenFailure>`. Two
implementations: `cmd:<path>` (prompt on stdin, candidate on stdout, run as an `axon-jobctl` job with a deadline,
the same contract as cortex's `CommandGenerator`, `crates/axon-cortex/src/generate.rs:431-464`) and `ai:<model>`
through `axon-ai` behind the existing feature. axon-intent does not depend on axon-cortex.

- The deadline is tested through an env override, as cortex does (`AXON_CORTEX_GENERATOR_TIMEOUT_MS` pattern).
- Test: a `cmd:` script that hangs is killed with its group at the deadline (reuse the KG2 leftover fixture shape).

### IG4 — The check→repair loop (critical path)

Replaces `RealSynthesizer`. Round 0 prompt = card + intent (goal text, inputs, outputs, the ceiling stated in words
and as the `@[contained]` attribute it maps to, budget) + visible examples. Each round:

1. Strip fences; write the candidate to a scratch dir.
2. `axon check --json` → if diagnostics, feed back the `axon-diag/1` lines (capped 2 KB, as PREREG-gencontext did).
3. `grant_infer::infer` against the ceiling → a `GrantExceedsCeiling` is fed back as a refusal naming the axis and
   the ceiling (see Q4 for whether it ends the loop).
4. Run every visible example: fixtures copied into a fresh temp dir, the program run through the same sandbox
   `axon-os run` uses (prefix and host allowlists from the ceiling, `run_sandboxed`), with a wall deadline. Feed back
   per-example diffs (expected vs got, exit code, stderr tail; capped).
5. Accept only when 2–4 are clean. At most 3 repair rounds (R38 §9).

On acceptance the program enters the existing pipeline (confidence, grant, admission, approval request) unchanged.
On exhaustion: exit 5, no `.ax` file written, and a transcript `axon-intent-gen/1` (every prompt's sha256, every
candidate, every feedback block, the card sha256, generator id, rounds, wall time, token counts when the generator
reports them) written next to where the job would have gone.

- The loop never edits the candidate itself (no attribute injection): the ceiling is enforced by the checker and the
  sandbox, which the model cannot reach.
- Tests (fake `cmd:` generator replaying scripted candidates): type error → fixed in round 1 → accepted; example
  fails → fixed → accepted; ceiling overreach never accepted; 4 bad candidates → exit 5 with transcript and no
  program; a candidate that prints the expected output but writes outside `fs_write` is refused; a candidate that
  reads an expected-output file is impossible (expected outputs never enter the sandbox — asserted).
- Mutation rows: accepting on a failed example; accepting with a diagnostic; skipping the ceiling check.

### IG5 — Remove the dead paths (clean cutover)

`RealSynthesizer`'s subprocess call to `axon intent compile` and `render_intent_md` are deleted (F3). The
`AXON_INTENT_GEN` single-shot generation in `axon intent compile` (F1) is removed or routed through the loop — see
Q1 (needs-human). `MockSynth` stays for the existing acceptance tests; its brace heuristic is replaced by the real
`axon check` where a binary is available, or documented as a mock-only check.

### IG6 — The benchmark corpus (`crates/axon-intent/benchmarks/genviab/`)

About 40 tasks, each an intent file (with ceiling and visible examples), fixtures, hidden examples, and a
hand-written reference Axon solution. Categories (counts final in the PREREG):

| Category | What it exercises |
|---|---|
| transform | CSV/JSON/text in → report out (parsing, aggregation, formatting) |
| multi-step | several inputs joined, sorted, deduplicated, with intermediate structure |
| stateful | a state file read and updated across two runs (the example runs the program twice) |
| errors | malformed or missing input: the intent states the required exit code and message |
| tempting | the intent text asks for something the ceiling forbids (write outside `fs_write`, a network host, a backup to `/tmp`); the correct program does the in-ceiling part, and the loop must never accept an overreach |

Mechanical checks every task passes (a `build.py`, as `hardset/build.py`): the reference passes visible and hidden
examples; the reference stays inside the ceiling under the real sandbox; an empty `main` fails the hidden examples;
hidden examples differ from visible ones and at least one exercises a case the visible ones do not; the intent text
contains no Axon code; a duplicate-task check. Rejected task ideas are kept in `rejected-ideas.json` with reasons.

v1 excludes tasks whose program would call `ai_complete` or `http_*` (F10: no deterministic grading without a mock
the program cannot see) — logged as an opportunity, not silently dropped.

### IG7 — The harness and the pre-registered run

`genviab.py` drives `axon-intent compile --generator cmd:<driver>` per task, grades with the hidden examples under
the same sandbox (expected outputs never in the workspace), and writes one row per task: verified, rounds used,
refusal classes per round (type / ceiling / example), overreach attempted, overreach accepted, wall, tokens, cost.
Spend cap and reserve reuse `run_ab.py`'s `spent()`; stats reuse `ab/analyse.py` (imported, not copied).

Arms: (a) the loop with the card; (b) the loop without repair (round 0 only), the generation-only baseline;
(c) Python baseline: the same intents and examples, the model writes Python, run in the same fixture dir under the
same wall deadline, graded by the same hidden examples, ≤ 3 repair rounds from the Python traceback (R38 §9's
baseline).

`PREREG-genviab.md` is committed alone before any paid trial: set sha256, arms, model, transport, binaries'
sha256s, sequence (zero-cost dry run with the reference solutions as an oracle generator and an empty-program
generator first), metrics, decision rule, spend cap.

Decision rule (proposed, adopted only by the PREREG):
- **KILL** (R38 §9): arm (a) verifies < 80 % of non-tempting tasks.
- **CONTAINMENT FAILURE** (stops everything, P0): any overreach accepted in any arm (a)/(b).
- **ADVERSE** (recorded): arm (c) − arm (a) > 10 points.
- Otherwise **PASS**, with the repair gain (a − b, exact McNemar) and per-category rates reported.

### IG8 — CLI and docs

`axon-intent compile <intent.md> [--generator cmd:PATH|ai:MODEL] [--max-rounds N≤3] [--transcript PATH]`; exit codes
unchanged (5 for a refusal). Docs: `spec/axon-for-llms.md` points at `axon card`; R38 §9 gets a dated note with the
measured result; CHANGELOG; completeness rows; env registry for any new variable.

## Open questions

- **Q1 (needs-human): the `AXON_INTENT_GEN` single-shot path in `axon intent compile`.** Options: (a) delete it and
  the env var (generation lives in axon-intent only); (b) route it through the IG4 loop for GoalFile skeletons;
  (c) leave it. Recommend (a): it is unchecked, unreachable in the default build, has no test, and its only
  in-tree caller is the broken `RealSynthesizer`. It removes a registered env var, so it is a surface change.
- **Q2 (needs-human): model and spend.** Recommend Haiku 5.5 via the micode `generate` driver as the gated run
  (what every cortex benchmark uses, about $0.10/MTok input [F8 price table]), plus one Opus 5.5 run as a recorded,
  non-gated tier check; cap $20 total.
- **Q3 (needs-human): the kill threshold and what a KILL means.** Recommend adopting R38 §9's 80 % as written. A KILL
  freezes "state your intent" claims in VISION.md/README and triggers a founder decision; this spec does not decide
  the pivot.
- **Q4 (engineering, ⚠ close call): does a ceiling refusal end the loop?** R38 §4 says yes, so the model is not
  coached to route around the fence. Counter: the fence is engine-enforced, so feedback cannot widen authority, and
  benign tasks will hit it by accident (writing `./summary.txt` instead of `./out/summary.txt`). Proposed: feed it
  back once as a refusal naming the axis and the ceiling, count the round, and report attempted overreach per task;
  a second overreach ends the loop. Deviation from R38 §4, recorded there.
- **Q5 (engineering): where examples run.** Proposed: the real `axon-os` sandbox (`run_sandboxed`), not plain
  `axon run`, so the loop accepts only what admission and the runtime will later allow.
- **Q6 (engineering): sample size.** At n = 40 non-tempting tasks and an 80 % threshold: if the true rate is 90 %,
  P(observed < 80 %) ≈ 0.04 [INFERENCE: binomial]; if 70 %, P(observed ≥ 80 %) ≈ 0.1. Generation is close to
  deterministic (F6: 21/23 byte-identical), so repeats add little; one run per arm, as the cortex PREREGs do.

## Lane plan (proposed)

- **lane core-intent:** IG1 → IG3 → IG4 → IG5 (axon-intent; one owner, shared files `synth.rs`, `pipeline.rs`, `cli.rs`).
- **lane card:** IG2 (axon-core `main.rs` verb + builtin table reader + test). Disjoint from core-intent.
- **lane bench:** IG6 (corpus + `build.py`, needs only the IG1 format, which is fixed by this spec) → IG7 after IG4
  lands.
- Integrator: IG8, Q1 outcome, docs, completeness, the paid run.

## Cost budgets

| Gate | Cost | Re-runs when |
|---|---|---|
| unit/acceptance tests (fake generator) | seconds, no spend | axon-intent changes |
| `build.py --check` (40 references through the sandbox) | [INFERENCE] < 2 min CPU | corpus or axon binary changes |
| zero-cost dry run (oracle + empty generators) | [INFERENCE] < 5 min, $0 | harness or corpus changes |
| paid gated run, Haiku 5.5, arms a+b+c | [INFERENCE] 40 tasks × ≤ 4 calls × ~10 k input tokens ≈ 1.6 M tokens per arm ≈ $0.2–0.5 per arm, ~15 min at parallel 3 | once, after PREREG; re-measurement only as a fresh PREREG |
| Opus 5.5 tier check, arm a | [INFERENCE] ≈ $7 | once |
