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
| `selftest.py` | no model, no spend: build.py (incl. `reference.py`), the once guard against a bare origin, the spend sum, the grader on two-run/exit/stdout fixtures, the driver against a fake `micode`; `GENVIAB_REQUIRE_BINS=1` turns a missing binary into a FAIL (the gate's `genviab-selftest` stage, S23) |
| `rejected-ideas.json` | sources not ported (`author`) and tasks build.py excluded (`mechanical`) |
| `PREREG-genviab.md` | the protocol, **DRAFT** (not frozen) |
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
`reference_author` is not `human:<name>`, that has no visible example or whose ceiling grants `exec`. A once
reference is `reference.ax` or `reference.py` (PREREG §4, option B; `reference.py` only in `once/`). A
`reference.py` imports only `sys`, `os`, `os.path`, `json`, `csv`, `re`, `math`, `collections`, `itertools`,
`functools`, `string` and is graded by its own runner (`gv.grade_python_reference`): a raised exception is a
fault whatever the exit status, a clean environment, `PYTHONHASHSEED=0`, the fs_read prefixes bound only when
staged, and the Axon reference's 1 MiB stdout bound. `genviab.py` refuses a paid once run unless the PREREG
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
  run sees only `PATH`, `LANG`, `LC_ALL`, `PYTHONHASHSEED` (ignored under `-I`) and bwrap's `PWD`, and an fs_read prefix is
  bound only when that run staged it; the dev arm c rows below were recorded before that change.

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
  has no visible example: its ceiling withholds fs_read, so no run can stage an input and `build.py`'s
  `hidden` check would refuse any visible one.
- **Ceiling effect:** on the current card and prompt Haiku 5.5 verifies 14/14 non-tempting `a`/`e`/`i`/`j`
  dev tasks in arms a, b and c. `h01`-`h12` exist to find headroom (spec GV-4).

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
| GV-4 headroom (Haiku, `h01`-`h12`, a × 3, c × 3) | 4856068d | not run yet: after GV-1 lands (spec §5 step 4) | | | |

Total paid dev spend: $0.1589 nominal (`python3 -B genviab.py spent`). `verdict` on run 4 (dev, not a
measurement): PASS, 14/14, Clopper-Pearson [0.768, 1.0], not ADVERSE. The GV-4 selection rules bind from
the dev manifest with `h01`-`h12` (`dev/manifest.json` sha256 `27d61918…`, committed before any paid call):
no h-task is dropped, edited or re-graded after the first paid call.

Run 1 found a corpus defect, not a model property: 14 intents had a second paragraph under `# Intent`
stating the output format, but the loop shows the generator only the first paragraph (`Intent::goal`), while
arm c's prompt showed it all. 8 of arm a's 9 false acceptances were format misses (a missing trailing newline,
a different summary). Fixed in both places: the intents are one paragraph, `build.py` rejects a second
paragraph, and arm c's prompt uses the same goal (`gv.goal`). Run 1 is kept as recorded.

Haiku 5.5 is diagnostic only (owner decision Q2). Opus 5.5 is the gating model, and only the once run
uses it.
