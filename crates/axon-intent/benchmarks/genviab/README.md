# genviab: does intent -> checked program work?

The generation-viability benchmark of `tasks/intent-gen-spec.md` (IG6, IG7). An operator states an
intent with a capability ceiling; `axon-intent compile --generate --generator cmd:<driver>` asks a model
for a program and runs the check->repair loop; the harness grades the accepted program on hidden
examples through the same `axon check`, gates and ceiling-scoped sandbox.

| File | What |
|---|---|
| `gv.py` | task format, grading (Axon through the `axon-intent` lift path; Python under the pinned bwrap, arm c's runner and the once `reference.py` runner) |
| `build.py` | builds and checks `dev/` and `once/`: mechanical checks, `<set>/manifest.json` (sha256 per file) |
| `genviab.py` | runs arm a (+ b) and arm c; spend, rate-limit stop, once-ledger; `report`, `verdict` (the IG7 decision rule), `spent` |
| `drivers/micode-program.sh` | the model behind `cmd:` through `micode` (tool-less mode, scratch dir, one log row per call) |
| `drivers/oracle.sh` | the zero-cost generator: the task's reference, or an empty program |
| `drivers/http-program.py` | the CERE generator behind `cmd:` (authoring-eval AE-1): one OpenAI-compatible call, explicit limits, the reply cache |
| `ae.py` | the authoring-eval runner (AE-4/AE-5/AE-6): paired base/candidate runs on a private `ae` set, the `axon-ae/1` trend row, `report`, `classes`, `check-canaries` |
| `ae/history.jsonl`, `ae/classes.json` | the public trend file (aggregates only) and the AE-5 class table |
| `selftest.py` | no model, no spend: build.py (incl. `reference.py`), the once guard against a bare origin, the spend sum, the grader on two-run/exit/stdout fixtures, the driver against a fake `micode`, the AE-1..AE-7 paths against a loopback fake CERE; `GENVIAB_REQUIRE_BINS=1` turns a missing binary into a FAIL (the gate's `genviab-selftest` stage, S23) |
| `rejected-ideas.json` | sources not ported (`author`) and tasks build.py excluded (`mechanical`) |
| `PREREG-genviab.md` | the protocol, **DRAFT** (not frozen) |
| `PREREG-ae.md` | the authoring-eval protocol, **DRAFT** skeleton (frozen by the integrator after the pilot) |
| `results/` | `dryrun/` (oracle, empty), `dev/` (paid Haiku runs); `calls/*.jsonl` are the spend source |

```sh
export AXON_BIN=$CARGO_TARGET_DIR/debug/axon      # axon-intent is taken from the same directory
python3 -B build.py --check                       # exit 0 ok, 1 FAIL, 2 bad arguments
GENVIAB_REQUIRE_BINS=1 python3 -B selftest.py    # what the gate runs (S23); without it a missing binary is a skip
python3 -B genviab.py run --set dev --arm a --generator oracle --out results/dryrun/dev-a-oracle
python3 -B genviab.py run --set dev --arm a --generator micode --out results/dev/haiku-a-N \
        --cap-usd 2.5 --reserve-usd 0.05          # spend counts every calls/*.jsonl under results/
python3 -B genviab.py report results/dev/haiku-a-N
python3 -B genviab.py verdict results/dev/haiku-a-N --c-dir results/dev/haiku-c-N   # refuses a run on another manifest
```

## The once set

`once/` is an empty scaffold (`once/manifest.json`, n = 0). Its tasks, examples and references are written
by hand by a human after `PREREG-genviab.md` freezes; `build.py` rejects a once task whose
`reference_author` is not `human:<name>`, that has no visible example or whose ceiling grants `exec`. A task's
`## Allowed` is read as `axon-intent` reads it (`gv.section`/`gv.ceiling` mirror `intent.rs`: `- ` and `* `
bullets, the section ends only at a `# `/`## ` line outside a fence, whitespace is Rust's `trim()` set), and an
`## Allowed` line that is not a `- `/`* ` `key: value` bullet, or that holds a control character (a trailing
`\r` aside; IGP-1), refuses the task, so `build.py`'s relative-prefix and `exec: none` rules see
every line the lift grants (B3-4). A hidden example must stage an input no visible run stages; when the
ceiling grants no fs_read (no run can stage one, e.g. a tempting task forbidding fs_read), it must instead
run past every visible example's run count and there check a field (stdout, exit or an output path) to a
value no visible run expects for that field (B3-2, IGP-2), so the loop still gets a visible example and a
hard-coded answer still fails. A once
reference is `reference.ax` or `reference.py` (PREREG §4, option B; `reference.py` only in `once/`). A
`reference.py` imports only `sys`, `os`, `os.path`, `json`, `csv`, `re`, `math`, `collections`, `itertools`,
`functools`, `string` and is graded by `gv.grade_python_reference`: the runner arm c uses too (a raised
exception is a fault whatever the exit status, a clean environment, `PYTHONHASHSEED=0`, the fs_read prefixes
bound only when staged), with the Axon reference's 1 MiB stdout bound. `genviab.py` refuses a paid once run unless the PREREG
is FROZEN and pins `once/manifest.json`; it writes the run's row to `once-ledger.jsonl`, commits it and
pushes it to `main` of `https://github.com/Cklaus1/axon.git` before the first call. A once run is refused
when `origin` is not that URL, is unreachable, when the checkout is not `origin/main`, or when the push fails;
a second score of the same (manifest, arm, model) is refused.

## Arms

- **a**: the loop, ≤ 3 repair rounds. `verified` = the pipeline accepted the program **and** it passes the
  visible and hidden examples. False acceptance = accepted ∧ ¬verified.
- **b**: arm a's round-0 candidate from the transcript, graded the same way. No generation call.
- **c**: Python 3 with the same goal, inputs, outputs, ceiling and visible examples, ≤ 3 repair rounds from the
  traceback or example diffs, every run under `bwrap` with the ceiling's binds (`gv.BWRAP_*`). Since GV-3 the
  run sees only `PATH`, `LANG`, `LC_ALL`, `PYTHONHASHSEED` and bwrap's `PWD`, and an fs_read prefix is
  bound only when that run staged it. Since B3-1 arm c runs through the reference runner (`gv.PY_RUNNER`,
  `-B -s -P`): an uncaught exception fails the example whatever the exit status (the feedback says "raised an
  exception"), `PYTHONHASHSEED=0` binds, and the stdout bound stays 64 KiB. The dev arm c rows below were
  recorded before both changes (re-grading the 97 recorded dev arm c candidates under the B3-1 runner changes
  no grade).

## The dev set (tuning data, never a measurement)

28 tasks: the 12 portable atlas `tasks_hard` tasks (`a04`-`a15`), `examples/intents/summarize` and
`overbroad`, `examples/jobs/overreach` (tempting), `e01-score-average` (errors; intent drafted by a
model and adopted by the operator 2026-11-01, so dev-only), and `h01`-`h12` (spec GV-4: 3 stateful, 3 errors,
3 multi-step, 3 tempting, written by `claude-opus-5-5` for headroom; dev-only). Not ported, with reasons:
`rejected-ideas.json`.

- **The atlas tasks carry NO visible example.** Each has one human-computed data point (atlas
  `expected`); it is the hidden example, and a visible one would be invented data. So on these tasks the
  loop accepts on `axon check` + gates alone, with no example feedback: dev numbers measure
  check+gates-only acceptance and are not comparable with a once set that has visible examples. Never copy
  this property to the once set.
- The references were written by a model (`reference_author: model:IgBench (dev only)`); `build.py`'s
  `human-value` check requires each to reproduce the source's human expected value. The once set's
  references are written by hand.
- Before `h01`-`h12`, no dev task was `stateful` and `e01-score-average` was the only `errors` task. `h12`
  has no visible example: its ceiling withholds fs_read, so no run can stage an input, and when it was
  written `build.py`'s `hidden` check refused any visible one. Since B3-2 such a task may have a visible
  example (see "The once set"); `h12` is frozen after paid calls and stays as written.
- **Ceiling effect:** on the current card and prompt Haiku 5.5 verifies 14/14 non-tempting `a`/`e`/`i`/`j`
  dev tasks in arms a, b and c, and every `h01`-`h12` task in all 3 replicates of arms a and c (GV-4: no
  headroom found).

## Results

| Run | Corpus | Arm a | Arm b | Arm c | Spend (nominal, `calls/*.jsonl`) |
|---|---|---|---|---|---|
| `dryrun/` oracle / empty | a0100121 | 14/14 / 0/14 | 14/14 / 0/14 | – / 0/14 | $0 |
| `dev/smoke-a` (1 task) | 49829e13 | 1/1 | 1/1 | – | $0.0019 |
| `dev/haiku-*-1` | 49829e13 | 6/14 | 5/14 | 14/14 | $0.0376 + $0.0040 |
| `dev/haiku-*-2` | a0100121 | 14/14 | 14/14 | 14/14 | $0.0309 + $0.0042 |
| `dev/haiku-*-3` (HEAD f654f8a6) | a0100121 | 14/14 | 13/14 | 13/14 (2 false acceptances: arm c has no accept gate) | $0.0334 + $0.0038 |
| `dev/haiku-*-4` (HEAD 5c937873) | a0100121 | 14/14 | 14/14 | 14/14 | $0.0308 + $0.0036 |
| `dev/e01-haiku-a-{1,2,3}`, `e01-haiku-c-1` (e01 only, HEAD a5310878) | 56d1dd57 | 3/3 runs, each round 0 | 3/3 | 1/1 | $0.0083 + $0.0005 |
| `dryrun/dev-h-a-{oracle,empty}` (`--only h01..h12`) | 4856068d | 12/12 / 0/12 (1 false acceptance: h12) | 9/9 / 0/9 | – | $0 |
| `dev/h-haiku-a-{1,2,3}`, `h-haiku-c-{1,2,3}` (GV-4, `--only h01..h12`, HEAD ec3ce598) | 4856068d | 12/12, 12/12, 12/12 (0 false acceptances) | 12/12, 10/12, 10/12 | 12/12, 12/12, 12/12 (0 false acceptances) | a $0.0388 + $0.0420 + $0.0428, c $0.0071 + $0.0076 + $0.0075 (`spent` delta $0.1459) |

Total paid dev spend: $0.3048 nominal (`python3 -B genviab.py spent`). `verdict` on run 4 (dev, not a
measurement): PASS, 14/14, Clopper-Pearson [0.768, 1.0], not ADVERSE. The GV-4 selection rules bind from
the dev manifest with `h01`-`h12` (`dev/manifest.json` sha256 `27d61918…`, committed before any paid call):
no h-task is dropped, edited or re-graded after the first paid call.

**GV-4 headroom (spec GV-4, pre-registered): not found.** All six invocations completed in the spec's order
(a1, c1, a2, c2, a3, c3), with the oracle at 12/12 and the empty program at 0/12 verified on the same
binaries: `axon` sha256 `1ca22353…` and `axon-intent` `cd1f507c…`, built from ec3ce598, which includes GV-1.
Arm a verified every task in every replicate, so 0 of 12 tasks are hard (the threshold is 3), and no task
is possibly defective. Infra rows: 0. Arm b, round 0, missed h06 and h08 in a2 and h03 and h09 in a3; one
repair round fixed each (repair gain 2 + 2). Refused attempts across all rounds: 0, 1 and 2. **Known defect
(selection rule 3):** h12 has no visible example, which breaks the slot table's ">= 1 visible" (see above). So
arm a accepts any h12 program that passes check and gates; the empty program was accepted there and failed
the hidden examples (the dry run's 1 false acceptance). Haiku's h12 programs still verified 3/3.
Caps: `genviab.py --cap-usd` bounds the total of every `calls/*.jsonl` under `results/`, not one run's spend,
so before each run `--cap-usd` was set to `spent` + $0.25 (arm a) or + $0.05 (arm c), with reserves $0.02
and $0.005 (integrator-approved reading of the spec's per-invocation caps). Item spend was $0.1459 against the
$0.75 cap. O-IG-03 stays open: these 12 tasks do not separate Haiku's arms, and the spec forbids writing a
second batch inside this item.

Run 1 found a corpus defect, not a model property: 14 intents had a second paragraph under `# Intent`
stating the output format, but the loop shows the generator only the first paragraph (`Intent::goal`), while
arm c's prompt showed it all. 8 of arm a's 9 false acceptances were format misses (a missing trailing newline,
a different summary). Fixed in both places: the intents are one paragraph, `build.py` rejects a second
paragraph, and arm c's prompt uses the same goal (`gv.goal`). Run 1 is kept as recorded.

Haiku 5.5 is diagnostic only (owner decision Q2). Opus 5.5 is the gating model, and only the once run
uses it.

## authoring-eval (`ae`, tasks/authoring-eval-spec.md AE-1..AE-7)

One number per model per release: how well CERE Qwen (`Qwen3.8-Flash-Next`, NH-1) writes Axon. The set, rows,
transcripts, programs and the reply cache are private (`/root/axon-ae`, D8); this directory holds the harness, the
aggregate trend file `ae/history.jsonl` and no task text or canary.

- **Generator (AE-1).** `genviab.py run ... --generator http --provider cere [--model M] [--temperature 0.7]
  [--seed 0] [--max-tokens 8192] [--cache-dir DIR]`. `drivers/http-program.py` makes one `/chat/completions` call
  with the bearer `CERE_TOKEN_AE` read from `/etc/cere/cere-t4.env` (a missing key is a refusal, never a fallback);
  `--provider` takes only `cere`. A `length` stop at the declared `max_tokens` is a model failure (`truncated`); a
  cut below it, a served model other than the requested one (`model_substituted`) or a transport error is infra.
  The reply cache key is (prompt sha, model series, temperature, seed, max_tokens, driver sha); a seeded replay
  sample re-queries hits and a mismatch turns the cache off for the rest of the run (D4). Haiku stays on
  `micode-program.sh` for drafting only (D12).
- **Failure classes (AE-2).** Every arm-a row carries `round_stages`, `diag_codes_per_round`, `b_check_ok`,
  `b_ccp`/`b_ccp_kind`, `b_runtime_fail`, `b_ended_classes`, `ccp_any_round`, `first_green_round`; `gv.grade_axon`
  returns per-run `ended_class` and `panic_kind` (`gv.PANIC_KINDS`). `genviab.py report` prints M1-M5 and the
  ended-class table.
- **Set loader (AE-3).** `build.py --check --set-dir DIR` checks an `ae` set (both references, refs-agree,
  family/tier/canary/drafted_by, tokens >= 120000) and writes `DIR/manifest.json` / `DIR/rejected.json`, never the
  public files; `--candidates` checks a drafting pool before it has `reference.ax`. `genviab.py run --set-dir DIR
  [--candidates]` runs arm a/c on it and saves an accepted program under `<out>/programs/` (D12).
- **Runner (AE-4/AE-6).** `ae.py run --set-dir <private>/ae-1 --label L --base-bin DIR --cand-bin DIR [--seeds 0,1,2]`
  runs arm a under both binaries (alternating which goes first), `regrade` when every round-0 prompt is byte-equal
  (one reply graded under both), else `resample`, and arm c once; Δ FT-pass is the family-clustered sign-flip of
  `headroom/stats.py` (95% CI = `ci(alpha=0.025)`, two-sided p). `--confirm RUN_ID` reruns on seeds never used for
  the set version with the cache off. Refused: an unfrozen PREREG (unless `--pilot`, no trend row), a manifest or
  series other than PREREG's, a start 01:30-08:00. `report RUN_ID` resolves the flag; `check-canaries` greps the
  content of `git ls-files` for every set canary.
- **Classes (AE-5).** `ae.py classes RUN_ID...` ranks round-0 failure classes by `ae/classes.json`, prints stall
  rates per check code, every M2s instance and draft opportunities stanzas (printed only).
- **Weekly screen (AE-7).** `scripts/ae_weekly.py run` (unit files `scripts/systemd/ae-weekly.{service,timer}`,
  Saturday 12:00, **not installed**): last labelled release vs `origin/main` HEAD, label `weekly-<date>`, advisory,
  binaries cached by commit under `<private>/bins/<sha8>/` (the base is never rebuilt: `ae_weekly.py cache-bins DIR`),
  digest with the rerun trigger in `<private>/weekly/<date>.md`; it never starts a confirm run.

S23 cost: the AE selftests add ≈ 42 s (`selftest.py` 51.5 s at 8916f045 against 9.2 s at 1f71623d); AE-2's fixtures
wait ~20 s on one example timeout and run in a thread beside the rest.

AE-1 acceptance, CERE dev runs (2026-11-10, rows private under `/root/axon-ae/runs/`): arm a on the 28 dev tasks through
`CERE_TOKEN_AE`, 0/28 infra, 35 calls all `finish_reason: stop`, served model = requested; verified 21/24 non-tempting, M1
(round 0) 18/24; the earlier dev-only run through the trainloop token gave the same headline, and arm c 0/28 infra (23/24).
