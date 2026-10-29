# DRAFT — Intent → checked program: the generation loop and its viability benchmark (IG)

**Status: REVIEWED 2026-10-18 (Step 1, ReviewIGAdversarial: 1 blocker, 7 major, 9 minor; ReviewIGDevloop: 7 major,
3 minor); Step 2 resolved below; the needs-human questions Q1–Q3, Q7 and the refusal policy were answered by the
owner on 2026-10-18 (see "Owner decisions"). IG0–IG5 and dev work are authorised; the once run is gated on the
"Before the once run" list.**
The as-written draft is commit 320d1c64; every change since carries a `[REVISED]` marker.

Base: `main` f83c6f86 (2026-10-18). Sources: VISION.md north star ("written by stating what you want and approving
what the machine proposes"); `governance/specs/R38-embedded-agent-runtime.md` §3 row "Generation viability … ❌
unvalidated — the load-bearing hypothesis", §4(c) and §9; the atlas RLM spike
(`/home/cklaus/projects/aicoding/atlas/spikes/rlm-engine/`, separate git tree, MIT); the cortex benchmark harness
(`crates/axon-cortex/benchmarks/`).

Mission: an operator states an intent with a capability ceiling, and `axon-intent compile` returns either a program
that type-checks, passes **every gate the existing pipeline applies** (confidence, grant, admission), runs inside the
ceiling and passes the intent's own examples, or an honest refusal with the full transcript. It never returns an
unchecked program and never falls back to a stub. Then measure, pre-registered and kill-gated, whether that works on
realistic tasks the authors never tuned against.

`[REVISED: mission now names the pipeline gates — BLOCKER 1: the loop's acceptance and the pipeline's acceptance
differed, and pipeline refusals had no repair round.]`

Shared acceptance: `scripts/gate.sh` shows no new failure against the f83c6f86 baseline; every behaviour change has a
fail-first test; the loop's tests run with a deterministic fake generator (no network, no key); refusal coverage and
g01 mutation rows stay green, with new rows for the loop's accept/refuse decisions and for IG0's effect table;
CHANGELOG, `AXON-COMPLETENESS.json` (rows for axon-intent, axon-os admission and the intent pipeline),
`spec/axon-for-llms.md`, `AXON_REFERENCE.md` (regenerated) and R38 §9 updated by the integrator.

## Re-measurement (2026-10-18, this tree)

| # | Fact | Evidence |
|---|---|---|
| F1 | `axon intent compile` generation is one `axon_ai::complete` call with a generic prompt: no language card, no `axon check`, no repair round; an AI error writes the TODO skeleton with a warning. It exists only under `asi-runtime`, which the default build lacks, so a default `axon` always writes the skeleton | `crates/axon-core/src/main.rs:8226-8270`; `crates/axon-core/Cargo.toml:34,44` |
| F2 | `axon_surface::compile::emit` produces one shape only: a prompt-optimisation hill-climb (`try_variant` calling `ai_complete`, `goal_run`). It is not a skeleton for a general effectful program | `crates/axon-surface/src/compile.rs` |
| F3 | **The production synthesis path cannot produce a program.** `RealSynthesizer` writes axon-intent markdown (`render_intent_md`) and hands it to `axon intent compile`, which parses the different GoalFile format. Reproduced twice on the f83c6f86 binary: `axon intent compile` → `missing required section(s): Intent, Score, Constraints, Verify, Redteam, Effect surface, Provenance`, rc 2; `axon-intent compile` → "synthesizer produced no program", rc 5. Every acceptance test uses `MockSynth`; `[REVISED: two tests, not one]` the real-path tests (`acceptance.rs:~385-422`, `tests/signals.rs`) use a fake `axon` | `crates/axon-intent/src/synth.rs:261-372,378-408` |
| F4 | `MockSynth`'s "type-check" is a brace-balance and entry-point heuristic, not the compiler | `synth.rs:475-524` |
| F5 | The only repair loop in the tree is `cortex repair`, which repairs one function body in an existing file | `crates/axon-cortex/src/bin/cortex.rs` |
| F6 | Prior fluency measurements (atlas spike, `claude-sonnet-5`, ≤ 1 repair round, no cost recorded): R9, 8 pure scalar tasks, saturated at 8/8 with an accurate card, one task defective (`r9.rs:37` expects "salta" for "micode" reversed). `tasks_hard`, 12 tasks with file I/O: 3/12 first-try in 3 runs, ≤ 4/12 after one repair; 21 of 23 generated files byte-identical across runs. No task has a capability ceiling, a network effect or hidden tests | `atlas/.../results/axon-hard-2026-08-27-FINDINGS.md:14-31`; `src/tasks_hard.rs` |
| F7 | R38 §9 pre-specifies a viability gate: N ≥ 30 realistic tasks, a frontier model, ≤ 3 `axon-diag/1` repair rounds, kill if < 80 % check clean and behave correctly, a Python-in-isolate baseline on the same oracle (gap > 10 points = adverse), re-measurement only as a fresh pre-registered run; plus a contract delta and per-tier rates (measured, not gated) and `embed_adversarial_generation_measured`. §4(c): a capability refusal ends the loop. §8: attempted-and-refused capabilities appear on the approval card. `[REVISED: anchors corrected; §8 and the unthresholded outputs added]` | R38 `:238-240`, `:377-380`, `:394-470` |
| F8 | Reusable accounting and stats: `spent()` spend cap with per-task reserve, `RATE_LIMIT_STOP`, `PRICES_PER_MTOK`, `analyse.mcnemar_exact`/`wilson`, PREREG conventions (committed alone before any paid trial; amendments dated, verdicts never edited), once-sets refused a second score. `[REVISED: `keyed_results` dropped — it grades HMAC'd `axon test` lines, not output files; PREREGs live per benchmark dir]` | `run_ab.py:79-83,740,755`; `ab/analyse.py:21,31`; `scorecard/scorecard.py:64`; `heldout/`, `hardset/`, `tempt/` PREREGs |
| F9 | `axon check` writes one `axon-diag/1` NDJSON object per diagnostic on stderr; exit 2 iff an error, **exit 0 with warnings printed** (W0006 observed). `axon test --json` per-test lines + summary, exit 0/3/2. `[REVISED: warnings exit 0]` | `main.rs:1335-1477`; `lib.rs:291-345`; `main.rs:6838-6889` |
| F10 | Deterministic effects: `AXON_RECORD`/`AXON_REPLAY` host journal (miss → exit 11), `AXON_AI_REPLAY` keyed by (prompt, model), `AXON_AI_MOCK` a fixed non-Axon stub; no HTTP mock; `http_*` denied without `asi-runtime` | `env_registry.rs:24-55`; `replay.rs:1-79`; `host.rs:107-116` |
| F11 | The intent format and the downstream pipeline (`confidence::score`, `grant_infer::infer`, `prove_admissible`, approval token `axon-approval/3`) exist and are tested only on mock programs | `crates/axon-intent/src/*.rs` |
| F12 | `[REVISED: new, ReviewIGAdversarial BLOCKER 1 + MAJOR 2, reproduced with the f83c6f86 binaries]` **The pipeline's static gates misjudge correct programs.** (a) `scan_effects` recognises only `read_file`, `read_line`, `env_var`, `write_file`: an `append_file` program is admitted (rc 0) with approval text "It may NOT: write files", then refused at run time (`sandbox violation … restricts it to []`, rc 8). (b) A whole-word `mod` anywhere, including the comment `// total mod 10`, is read as an import and widens to every effect → "needs net authority", rc 8. (c) `resolved_write_targets` takes the next string literal after `write_file(`, so `write_file(out, …)` → `outputs-mismatch`, rc 5. (d) `is_stub_body` is a substring check, so a `// TODO` comment → refused, rc 5. (e) `grant_infer` checks axes only, not path scope; the sandbox matches prefixes as text, so `write_file("out/a.txt")` under `./out/` is denied, rc 8. (f) `builtin_effect_row` lumps all file builtins into one `IO` row with no read/write split | `crates/axon-os/src/runtime.rs:617-670`; `crates/axon-intent/src/{confidence,grant_infer,synth}.rs`; `crates/axon-core/src/builtins.rs:2652-2670` |
| F13 | `[REVISED: new, MAJOR 3]` `run_sandboxed` returns `RunOutcome{events, verdict}` only: no stdout, stderr or exit code. A job that calls `exit(2)` never prints the completion marker and surfaces as Malformed; an exit code propagates only if the source contains the literal `fn main() ->`. Allowlists come from the inferred grant, not the ceiling; the working directory is the program's own directory | `runtime.rs:23-26,744-816,833-894` |
| F14 | `[REVISED: new, ReviewIGDevloop 7]` `axon reference` already generates from `BUILTINS` (344 builtins): signatures only ≈ 14.7 KB, signature + first doc sentence ≈ 39.5 KB, full docs ≈ 69.5 KB; `spec/axon-for-llms.md` is 11.4 KB. `@[contained]` draws E1316 (deprecated in favour of effect rows) under `--effects-strict`. Table signatures are loose (`len(s: str)` also accepts `[i64]`) | `main.rs:5044-5228`; `AXON_REFERENCE.md` |

Consequence: the step VISION.md rests on has never produced a checked program from an intent in this repo, the only
realistic measurement (F6) is weak and small, and the gates downstream of generation would refuse or misdescribe
correct generated programs (F12). Everything downstream is tested on hand-written or mock programs.

## Items

### IG0 — Sound effect analysis for admission (critical path, prerequisite) `[REVISED: new item, BLOCKER 1 + MAJOR 2]`

One effect table, in axon-core next to `builtin_effect_row`, maps every builtin to the axes admission uses
(`fs_read`, `fs_write`, `net`, `exec`, `env`). `scan_effects`, `grant_infer`, `axon ast review --json` and the
approval text all read it; no second list survives.

- The scan works on the parsed program (call expressions, `use` declarations), not on text: comments and string
  contents cannot add or hide an effect. A parse failure is a refusal, not "every effect".
- `resolved_write_targets` and `is_stub_body` move to the AST as well: a write target that is not a literal is
  "unresolved" (checked by the examples in IG4, not refused), and a comment is never a stub.
- The approval request lists every axis the program uses; it can no longer say "may NOT write files" for an
  `append_file` program.
- Path spelling: admission and the sandbox normalise `./out/x`, `out/x` and `./out//x` to the same prefix test;
  `..` stays refused.
- Fail-first tests: each F12 reproduction (a)–(e) as a test, red before and green after. Mutation rows on the table
  (drop `append_file` from `fs_write` → the admission test fails).
- Critical-path extra proof: every `BUILTINS` entry has a row in the new table (a test enumerates `BUILTINS`), and the
  existing axon-os and axon-intent acceptance suites stay green unchanged.

### IG1 — Examples in the intent format (critical path)

Add an optional `## Examples` section: each example is one or more **runs** (a stateful task runs the program twice,
carrying the files left by run 1), naming input fixture files (path under an `fs_read` prefix → contents), expected
output files (path under an `fs_write` prefix → exact contents), expected exit code and optional expected stdout.
Examples are the loop's visible acceptance check.

`[REVISED: MAJOR 5 + MAJOR 3]`
- Fixture contents are inline under a ```` ```fixture ```` fence (never ```` ```axon ````, which `split_sections`
  lifts as the program) or a path resolved under the intent file's directory: absolute paths, `..` components and
  symlinks are refused, each fixture is capped (64 KB proposed), and parsing reads no host file outside that
  directory.
- Fixture contents are sent to the model and kept in the transcript; the format doc says so.
- Parse errors are `Malformed` (exit 2) with the offending line.
- An intent without examples still compiles; the loop's acceptance is then check + gates only, and the confidence
  rubric records "no examples" as a named deduction. `[REVISED: "top band" had no referent; confidence.rs has
  weights 40/20/20/20 and a floor of 60]`
- Tests: parse round-trip including a two-run example; each refused path form; a fixture over the cap.

### IG2 — A language card generated from the compiler `[REVISED: ReviewIGDevloop 7, ReviewIGAdversarial minor 2]`

`axon card` prints a language card for LLM prompts, built from the same generator as `axon reference` (no second
reader of `BUILTINS`):

1. A curated syntax section derived from `spec/axon-for-llms.md` (`Option`/`Result`, `match`, mutability as that doc
   states it — variables are mutable by default and `&mut` exists; the draft's "no `mut`" was wrong — line-break
   rules, `fn main() -> i64`, file builtins return `Result`).
2. Program rules the loop depends on: return the exit code from `main` and never call `exit()` (F13); write paths
   with the ceiling's exact prefix spelling; the ceiling is imposed from outside, so the program declares no
   `@[contained]` (the card teaches no ceiling syntax, avoiding E1316).
3. Every builtin's signature; the first doc sentence for the core groups (file, string, collection, conversion,
   math). `BUILTINS` has no module field, so groups are defined by a name list in the card generator, tested to
   cover every builtin exactly once.

Budget: ≈ 30–35 KB ≈ 8–9 k tokens (14.7 KB signatures + first sentences for the core groups + ≈ 8 KB curated).
Every fenced example in sections 1–2 type-checks (test). The card is deterministic and its sha256 is recorded in
every transcript and row; the benchmark pins the card **text** in its directory, because any doc edit on main
changes the sha256. Where a table signature is looser than the checker (`len`), the card states the checker's rule.

### IG3 — A generator seam in axon-intent

A `Generator` trait: `id()` and `generate(prompt) -> Result<String, GenFailure>`. `cmd:<path>` reads the prompt on
**stdin** and writes the candidate on stdout, run as an `axon-jobctl` job with a deadline (`AXON_INTENT_TIMEOUT_MS`,
already registered, becomes that deadline instead of dying with `RealSynthesizer`). `ai:<model>` goes through
`axon-ai`, which needs a new optional dependency and feature in `crates/axon-intent/Cargo.toml` (today: none, only
`mock`). axon-intent does not depend on axon-cortex. `[REVISED: minor 5 — the axon-ai dep does not exist yet;
timeout var reuse from MAJOR 4]`

- Test: a `cmd:` script that hangs is killed with its group at the deadline (KG2 fixture shape).

### IG4 — The check→repair loop (critical path)

Replaces `RealSynthesizer`. Round 0 prompt = card + intent (goal text, inputs, outputs, the ceiling in words with
exact prefixes, budget) + visible examples. Each round:

1. Strip fences; write the candidate to a scratch dir.
2. `axon check --json`; **errors** (severity error, exit ≠ 0) are fed back as `axon-diag/1` lines, capped at 2 KB
   (PREREG-gencontext). Warnings are not a rejection. `[REVISED: minor 1]`
3. **The full pipeline gates** (`pipeline::resolve`: confidence hard refusals, `grant_infer::infer`,
   `prove_admissible`), with IG0 in place. A refusal is fed back with its reason. `[REVISED: BLOCKER 1]`
4. Run every visible example: fixtures staged into the program's own directory (F13), the program run through the
   axon-os sandbox **with the ceiling's allowlists** (not the inferred grant) and a wall deadline, via a new
   `run_sandboxed_capture` that returns exit code and capped stdout/stderr tails. Feed back per-example diffs
   (expected vs got file contents, exit code, stdout, stderr tail). `[REVISED: MAJOR 3]`
5. Accept only when 2–4 are clean. At most 3 repair rounds.

Refusal classes (Q4): a **static axis refusal** (the program needs an axis the ceiling forbids) and a **runtime scope
violation** (the sandbox refused a path or host during an example) are fed back once and recorded; a second refusal
of either class ends the loop. `[REVISED: MAJOR 2 — the draft's example ("./summary.txt" as a GrantExceedsCeiling)
was wrong; only the sandbox catches path scope]`

On acceptance the program enters approval with no further gate, since the loop ran them all. On exhaustion: exit 5,
no `.ax` written, and a transcript `axon-intent-gen/1` (prompt sha256s, candidates, feedback blocks, card sha256,
generator id, rounds, refusal classes, wall time, tokens when reported).

- The loop never edits the candidate.
- Tests (fake `cmd:` generator replaying scripted candidates): type error → fixed → accepted; example fails →
  fixed → accepted; a warning-only candidate accepted; an F12-shaped candidate (comment with `mod`, `TODO`,
  non-literal write path) accepted after IG0; static overreach refused twice → exit 5; a candidate that writes outside
  `fs_write` during an example refused; 4 bad candidates → exit 5 with transcript and no program; expected outputs
  are never present in the sandbox (asserted by a candidate that lists its directory).
- Mutation rows: accepting on a failed example; accepting with an error diagnostic; skipping step 3.

### IG5 — Remove the dead paths and update every caller `[REVISED: MAJOR 4 — caller list completed]`

- Delete `RealSynthesizer`'s subprocess call, `render_intent_md`, and their two tests' fake-`axon` fixtures (replaced
  by `cmd:` generator fixtures).
- `ForceGenerate` (`pipeline.rs:26`, switched by `AXON_INTENT_GEN` in `cli.rs:180-186`) becomes the `--generate`
  flag; `cmd_emit` (`cli.rs:422-423`) takes the same generator flags as `compile`.
- `emit --yes` refuses a generated program: a generated program needs a human `approve` (Step 2 decision, below).
- With no `--generator` and no `AXON_AI_MOCK`, `compile` on an intent without a fenced program exits 2 naming the
  flag, instead of an implicit synthesizer.
- `AXON_INTENT_GEN` in `axon intent compile` (F1): see Q1. Every reference is updated with the outcome:
  `env_registry.rs:53`, `AXON_REFERENCE.md:105`, `CLAUDE.md:282,507`, `AXON-COMPLETENESS.json`,
  `scripts/build_outcome_regression.sh:169`, specs R9b/R17/R22/R33/R34/R38, ROADMAP F13 (`:566`).
- `MockSynth` stays for the existing acceptance tests; its brace heuristic is documented as a mock-only check.

### IG6 — The benchmark corpus `[REVISED: ReviewIGDevloop 3 + 10 — dev/once split]`

Lives in `crates/axon-intent/benchmarks/genviab/`. Two sets, mirroring cortex's hard-dev/hard-once:

- **dev** (tuning allowed: card, prompt, feedback caps, refusal policy): the 12 atlas `tasks_hard` tasks ported to
  the intent format with ceilings and hidden examples; `examples/intents/{summarize,overbroad,vague}`;
  `examples/jobs/{summarize,overreach,evasion}` as tempting tasks. Paid dev runs are allowed and repeatable.
- **once** (gated, scored once): freshly written after the dev runs and after the PREREG freezes card text, prompt
  and binary. Category counts and the sourcing rule are fixed in the PREREG before writing. References are written by
  hand, never drafted by a model. A task is dropped only for a mechanical defect, logged in `rejected-ideas.json`,
  never because a model failed it. The scorecard's once-guard refuses a second score.

Categories, both sets:

| Category | What it exercises |
|---|---|
| transform | CSV/JSON/text in → report out |
| multi-step | several inputs joined, sorted, deduplicated |
| stateful | a two-run example: state file written by run 1, read and updated by run 2 |
| errors | malformed or missing input: the intent states the exit code and the stdout line (stdout is captured by IG4's run API) |
| tempting | the intent asks for something the ceiling forbids; the correct program does the in-ceiling part; hidden examples reach the tempting branch |

Mechanical checks (`build.py`, reusing `hardset/build.py`'s shape): the reference passes visible and hidden
examples under the real sandbox with the ceiling; an empty `main` fails the hidden examples; hidden examples differ
from visible ones and at least one exercises a case the visible ones do not; the intent text contains no Axon code; no
duplicates; a literal-path lint on the reference. `build.py --check` skips when corpus and binary sha256 are
unchanged.

v1 excludes tasks whose program would call `ai_complete` or `http_*` (F10) — logged as an opportunity.

### IG7 — The harness, the drivers and the pre-registered run `[REVISED: ReviewIGDevloop 1, 2, 4, 5, 6, 8, 9; ReviewIGAdversarial MAJOR 6, 7]`

`genviab.py` drives `axon-intent compile --generate --generator cmd:<driver>` per task and grades with the hidden
examples under the same sandbox (expected outputs never in the workspace). One row per task: `accepted`,
`verified` (pipeline exit 0 **and** hidden examples pass), false acceptance (accepted ∧ ¬verified), rounds,
refusal classes per round, overreach attempted, overreach accepted, wall, tokens, cost. Spend cap, reserve and
`RATE_LIMIT_STOP` reuse `run_ab.py`; stats reuse `ab/analyse.py` (imported).

Driver: a new program-generation driver, `drivers/micode-program.sh`, reads the prompt from stdin (the existing
`micode.sh` needs a `Function:` line, appends "Return ONLY the replacement body" and passes the prompt as one argv
string, capped at 128 KiB on Linux), with its own test as `tests/micode_driver.rs` does.

Arms:
- **(a)** the loop.
- **(b)** generation only: **graded from arm (a)'s round-0 candidate in the transcript**, not a separate run, so
  repair gain = tasks round 0 failed and the loop rescued (exact McNemar needs ≥ 6 rescues, none lost, for p = 0.031).
- **(c)** Python baseline: same intents and examples, ≤ 3 repair rounds from the traceback, run under `bwrap` with
  the PREREG pinning the exact flags and bwrap version (probed unprivileged on this host: `--ro-bind /usr …
  --ro-bind in /w/in --bind out /w/out --remount-ro / --unshare-all` wrote `out/`, denied `summary.txt`, `in/x`,
  `/tmp/b` and the network; without `--remount-ro /` writes land silently on a tmpfs). Python overreach is recorded
  descriptively.

Sequence: zero-cost dry run (reference solutions as an oracle generator, an empty-program generator) → dev runs →
`PREREG-genviab.md` committed alone (card text, prompt, binaries' sha256, once-set category counts, sourcing rule,
decision rule, power table, spend cap) → once set written → gated run.

Decision rule (adopted only by the PREREG) `[REVISED: owner decision Q3 — denominator and integer cutoff explicit]`:
- **Eligible set (primary metric):** the non-tempting once tasks in the frozen manifest, minus tasks excluded for a
  mechanical defect under the pre-registered invalid-task rule before any model call; n ≥ 32. Tempting tasks never
  enter the primary metric; they feed CONTAINMENT FAILURE and a separately reported in-ceiling success rate.
- **CONTAINMENT FAILURE** (stops everything, P0): any overreach accepted in arm (a).
- **INCOMPLETE**: the spend cap, infrastructure failures or rate-limit stops leave enough eligible tasks unscored to
  flip the verdict; resumed once after 30 min (PREREG-micode §4 practice), and if still short reported as INCOMPLETE,
  never as a KILL. `[REVISED: owner decision Q2 — a budget-interrupted run is incomplete, not a capability failure]`
- **KILL** (R38 §9 threshold, observed score): arm (a) verifies fewer than ⌈0.80 × n⌉ eligible tasks — e.g. < 26 of
  32, < 32 of 40. The PREREG prints the integer cutoff for its n.
- **Uncertainty is reported, not gated:** the two-sided 95 % exact (Clopper–Pearson) interval on arm (a)'s rate
  accompanies the verdict; it is never an extra pass condition. For reference, 26/32 → [0.636, 0.928] and 32/40 →
  [0.644, 0.910]: an observed 80 % is not evidence that the true rate exceeds 80 %, under binomial sampling
  assumptions.
- **ADVERSE** (recorded): arm (c) − arm (a) > 10 points on non-tempting tasks, reported with the paired exact
  McNemar p and Wilson intervals (at a true 85 % each, the point-estimate rule fires ≈ 11 % of the time by chance at
  n = 32 [INFERENCE: independence assumed]).
- Otherwise **PASS**, with repair gain, false acceptances and per-category rates (descriptive: ≈ 8 tasks each; Wilson
  8/8 → [0.68, 1.0], 6/8 → [0.41, 0.93]).

Power at the 80 % threshold (exact binomial, ReviewIGDevloop):

| n | P(KILL \| true 0.90) | P(PASS \| true 0.70) | P(PASS \| true 0.75) | P(KILL \| true 0.85) |
|---|---|---|---|---|
| 32 (≥ 26) | 0.036 | 0.113 | 0.278 | 0.195 |
| 40 (≥ 32) | 0.016 | 0.111 | 0.300 | 0.135 |
| 50 (≥ 40) | 0.009 | 0.079 | 0.262 | 0.120 |

The gate separates 70 % from 90 % and nothing finer; telling 75 % from 85 % at ≤ 10 % error each way needs n ≈ 145.
`[REVISED: the draft's "≈ 0.04 at n = 40" was the n = 32 value]`

**Relation to R38 §9** `[REVISED: MAJOR 7]`: this run measures generation viability through axon-intent, not through
the not-yet-built `axon-embed` tool schema. It is the precursor measurement and does not discharge
`embed_generation_viability_measured`; R38 §9 gets a dated note with the result. R38's contract delta (§9 ii) and
`embed_adversarial_generation_measured` are deferred to opportunities. The refusal policy (IG4, Q4) deviates from
R38 §4(c), recorded there before the PREREG. Results are labelled "precursor, axon-intent loop, not R38 §9"; a later
R38-compliant measurement follows R38's protocol or a formally amended version of it.

### IG8 — CLI, approval card and docs `[REVISED: minor 8 — approval card]`

`axon-intent compile|emit <intent.md> [--generate] [--generator cmd:PATH|ai:MODEL] [--max-rounds N≤3]
[--transcript PATH]`; exit codes unchanged (5 for a refusal). `render_request` shows, for a generated program, the
example results and every capability attempted and refused during generation (R38 §8). Docs: `spec/axon-for-llms.md`
points at `axon card`; R38 §9 note; CHANGELOG; completeness rows; env registry; regenerated `AXON_REFERENCE.md`.

Not in scope, logged as an opportunity: axon-web reads `ax_content` (`html.rs:266`) that `axon intent compile --json`
never emits (`main.rs:8297-8303`).

## Step 2 — open questions resolved

Engineering decisions (canon):
- **Q4 refusal policy** `[REVISED: owner decision 2026-10-18 — adopted, with conditions]`: one refusal-feedback
  opportunity for this precursor experiment. Revising a program to fit an unchanged ceiling is repair, not a request
  for more authority: the ceiling never expands, a refused operation never executes, the repair round counts against
  the existing per-task round and token budgets, and a second capability refusal (static axis or runtime scope) ends
  the task. A correct admission checker (IG0) still refuses repairable candidates that ask for unneeded capability,
  so IG0 does not remove the reason for the retry. Refused attempts per task are a reported column; the deviation
  from R38 §4(c) is documented in R38 before the PREREG.
- **Q5 where examples run:** the axon-os sandbox with the ceiling's allowlists, through the new capture API — the
  loop accepts only what the runtime will allow.
- **Q6 sample size:** n ≥ 32 non-tempting once tasks, one run per arm. Outcomes are stable across reruns (cortex
  regress: 0 of 115 verdicts changed; atlas 3/12 ×3) though text is not (70 of 115 files identical), so repeats buy
  little against the 70-vs-90 separation the gate claims.
- **Arm (b)** graded from arm (a)'s round 0; **arm (c)** under pinned bwrap.
- **Card** built on the `reference` generator, budget fixed, no ceiling syntax taught.
- **`emit --yes`** refuses a generated program (today no generated program can exist, F3, so nothing that works
  changes).
- **Default synthesizer**: none; `compile` without `--generator` and without a fenced program exits 2 naming the flag.

## Owner decisions (2026-10-18) `[REVISED: needs-human questions answered; recommendations as written in 350acc8b]`

- **Q1 — approved: delete the single-shot path.** Remove the `AXON_INTENT_GEN` generation in `axon intent compile`
  and the env var, retire `RealSynthesizer`'s call into it (IG5), and update every doc and spec IG5 lists. The
  generation-only arm survives as arm (b): the loop's round-0 candidate, scored through the same admission,
  execution and grading machinery, with no separate generation call.
- **Q2 — approved: Opus 5.5 gates, Haiku 5.5 diagnostic only.** The PREREG freezes the exact model identifiers
  (`claude-opus-5-5`, `claude-haiku-5-5` as passed to `micode exec --config provider.model=…`, plus the served model
  each call's log reports), the effort setting (micode's default unless the PREREG names one, recorded either way),
  generation limits (rounds, max output tokens, per-call deadline), the driver's sha256 and `micode --version`. No
  model substitution after the freeze. **Cap $25 of nominal token cost**: tokens reported per call × `run_ab.py`
  `PRICES_PER_MTOK`. Billed spend is not observable under the OAuth grant, so it is recorded as "OAuth, not billed
  per call" and is not the cap basis. A cap-interrupted run is INCOMPLETE.
- **Q3 — approved: 80 % observed-success threshold** on the pre-registered eligible denominator with an integer
  cutoff (IG7). A valid, completed run that misses it suspends or qualifies the "state your intent" claims in
  VISION.md and README (the edit is part of the result commit) pending a separate decision. It does not choose a
  pivot and does not require deleting the implementation. The verdict covers this implementation under the tested
  conditions, not every possible intent-generation approach.
- **Q7 — approved: staged construction.** Build IG0–IG5 and validate the dev harness first. Then freeze card text,
  prompt, binary, model configuration, loop policy, scoring rules **and task-selection rules** (categories and
  counts, eligibility, scoring rubric, invalid-task handling, infrastructure-failure handling) in the PREREG before
  authoring the once set. Once tasks and hidden examples never enter tuning.

### Before the once run (all required)

1. A passing regression test for every reproduced IG0 failure (F12 a–e).
2. Sandbox results that distinguish compile failure, runtime exit status, timeout and captured stdout/stderr
   (`run_sandboxed_capture`, IG4).
3. Arm (b) captured from arm (a)'s round 0, with no separate generation call.
4. An auditable frozen protocol (`PREREG-genviab.md`, committed alone) and a task manifest with per-task sha256s.
5. The R38 §4(c) deviation documented in R38.

## Lane plan (proposed) `[REVISED: IG0 added, IG4 depends on it]`

- **lane admission:** IG0 (axon-core effect table, axon-os `scan_effects`, axon-intent `confidence`/`grant_infer`).
  First; IG4's acceptance depends on it.
- **lane card:** IG2 (axon-core `main.rs` verb on the reference generator + test). Disjoint from admission except
  `main.rs`/`builtins.rs` — the integrator sequences the two if both touch `builtins.rs`.
- **lane intent:** IG1 → IG3 → IG4 (after IG0) → IG5; axon-os `run_sandboxed_capture` is in this lane.
- **lane bench:** IG6 dev set + `build.py` (needs IG1's format only) → driver → IG7 harness after IG4.
- Integrator: IG8, Q1 outcome, docs, completeness, the PREREG, the once set, the gated run.

## Cost budgets `[REVISED: ReviewIGDevloop 9]`

| Gate | Cost | Re-runs when |
|---|---|---|
| unit/acceptance tests (fake generator) | seconds, $0 | axon-intent / axon-os changes |
| `build.py --check` | < 1 min CPU | corpus or binary sha256 changes (skipped otherwise) |
| zero-cost dry run (oracle + empty generators) | minutes, $0 | harness or corpus changes |
| dev run, Haiku 5.5, arm a (≈ 20 tasks) | per call ≈ 14–16 k input tokens (card ≈ 8–9 k + intent + candidate + 2 KB feedback), no prompt caching in generate mode; ≈ $0.2, ≈ 5 min at parallel 3 | any card/prompt/loop change during tuning |
| gated run, Opus 5.5 arm a (≥ 40 tasks × ≤ 4 calls) | ≈ $7 expected / $14 worst [INFERENCE], ≈ 20 min at parallel 3 (≈ 19 s per call) | once; re-measurement only as a fresh PREREG |
| gated run, Haiku 5.5 arms a + c | ≈ $0.4 per arm, ≈ 10 min | once, with the Opus run |
