# genviab: does intent -> checked program work?

The generation-viability benchmark of `tasks/intent-gen-spec.md` (IG6, IG7). An operator states an
intent with a capability ceiling; `axon-intent compile --generate --generator cmd:<driver>` asks a model
for a program and runs the check->repair loop; the harness grades the accepted program on hidden
examples through the same `axon check`, gates and ceiling-scoped sandbox.

| File | What |
|---|---|
| `gv.py` | task format, grading (Axon through the `axon-intent` lift path, Python under the pinned bwrap) |
| `build.py` | builds and checks `dev/` and `once/`: mechanical checks, `<set>/manifest.json` (sha256 per file) |
| `genviab.py` | runs arm a (+ b) and arm c; spend, rate-limit stop, once-ledger; `report`, `verdict` (the IG7 decision rule), `spent` |
| `drivers/micode-program.sh` | the model behind `cmd:` through `micode` (tool-less mode, scratch dir, one log row per call) |
| `drivers/oracle.sh` | the zero-cost generator: the task's reference, or an empty program |
| `selftest.py` | no model, no spend: build.py, the once guard, the spend sum, the grader on two-run/exit/stdout fixtures, the driver against a fake `micode` |
| `rejected-ideas.json` | sources not ported (`author`) and tasks build.py excluded (`mechanical`) |
| `PREREG-genviab.md` | the protocol, **DRAFT** (not frozen) |
| `results/` | `dryrun/` (oracle, empty), `dev/` (paid Haiku runs); `calls/*.jsonl` are the spend source |

```sh
export AXON_BIN=$CARGO_TARGET_DIR/debug/axon      # axon-intent is taken from the same directory
python3 -B build.py --check                       # exit 0 ok, 1 FAIL, 2 bad arguments
python3 -B selftest.py
python3 -B genviab.py run --set dev --arm a --generator oracle --out results/dryrun/dev-a-oracle
python3 -B genviab.py run --set dev --arm a --generator micode --out results/dev/haiku-a-N \
        --cap-usd 2.5 --reserve-usd 0.05          # spend counts every calls/*.jsonl under results/
python3 -B genviab.py report results/dev/haiku-a-N
python3 -B genviab.py verdict results/dev/haiku-a-N --c-dir results/dev/haiku-c-N   # refuses a run on another manifest
```

## The once set

`once/` is an empty scaffold (`once/manifest.json`, n = 0). Its tasks, examples and references are written
by hand by a human after `PREREG-genviab.md` freezes; `build.py` rejects a once task whose
`reference_author` is not `human:<name>` or that has no visible example. `genviab.py` refuses a paid once
run unless the PREREG is FROZEN and pins `once/manifest.json`, ledgers the run in `once-ledger.jsonl`
before the first call, and refuses a second score.

## Arms

- **a**: the loop, ≤ 3 repair rounds. `verified` = the pipeline accepted the program **and** it passes the
  visible and hidden examples. False acceptance = accepted ∧ ¬verified.
- **b**: arm a's round-0 candidate from the transcript, graded the same way. No generation call.
- **c**: Python 3 with the same goal, inputs, outputs, ceiling and visible examples, ≤ 3 repair rounds from the
  traceback or example diffs, every run under `bwrap` with the ceiling's binds (`gv.BWRAP_*`).

## The dev set (tuning data, never a measurement)

15 tasks: the 12 portable atlas `tasks_hard` tasks (`a04`-`a15`), `examples/intents/summarize` and
`overbroad`, `examples/jobs/overreach` (tempting). Not ported, with reasons: `rejected-ideas.json`.

- **The atlas tasks carry NO visible example.** Each has one human-computed data point (atlas
  `expected`); it is the hidden example, and a visible one would be invented data. So on these tasks the
  loop accepts on `axon check` + gates alone, with no example feedback: dev numbers measure
  check+gates-only acceptance and are not comparable with a once set that has visible examples. Never copy
  this property to the once set.
- The references were written by a model (`reference_author: model:IgBench (dev only)`); `build.py`'s
  `human-value` check requires each to reproduce the source's human expected value. The once set's
  references are written by hand.
- No dev task is `stateful` or `errors`: no IG6 source has a two-run task or a stated exit code. The
  grader's two-run, exit-code and stdout paths are tested by `selftest.py` on a synthetic fixture.
- **Ceiling effect:** on the current card and prompt Haiku 5.5 verifies 14/14 non-tempting dev tasks in
  arms a, b and c. The dev set has no headroom left to tune against.

## Results

| Run | Corpus | Arm a | Arm b | Arm c | Spend (nominal, `calls/*.jsonl`) |
|---|---|---|---|---|---|
| `dryrun/` oracle / empty | a0100121 | 14/14 / 0/14 | 14/14 / 0/14 | – / 0/14 | $0 |
| `dev/smoke-a` (1 task) | 49829e13 | 1/1 | 1/1 | – | $0.0019 |
| `dev/haiku-*-1` | 49829e13 | 6/14 | 5/14 | 14/14 | $0.0376 + $0.0040 |
| `dev/haiku-*-2` | a0100121 | 14/14 | 14/14 | 14/14 | $0.0309 + $0.0042 |

Run 1 found a corpus defect, not a model property: 14 intents had a second paragraph under `# Intent`
stating the output format, but the loop shows the generator only the first paragraph (`Intent::goal`), while
arm c's prompt showed it all. 8 of arm a's 9 false acceptances were format misses (a missing trailing newline,
a different summary). Fixed in both places: the intents are one paragraph, `build.py` rejects a second
paragraph, and arm c's prompt uses the same goal (`gv.goal`). Run 1 is kept as recorded.

Haiku 5.5 is diagnostic only (owner decision Q2). Opus 5.5 is the gating model, and only the once run
uses it.
