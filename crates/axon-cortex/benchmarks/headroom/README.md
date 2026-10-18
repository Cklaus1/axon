# headroom: the hx held-out set (spec FU-1)

Every older set is saturated for the incumbent generator. hx is a task set
aimed at a 30–70% incumbent solve rate, built mechanically from repo programs
(stratum D) and from independently written programs (stratum C,
`programs/`). The pre-registration is `PREREG.md`.

| file | role |
|---|---|
| `build.py` | builds the task pool, splits it by program, `--check`s a built set |
| `mutators.py` | mutation operators: ../heldout/mutators.py's plus the subtle ones, composition of disjoint edits |
| `calibrate.py` | empirical-Bayes cell rates on dev, cell selection, cross-fit, h45 check, writes `hx-dev.json`/`hx-test.json` |
| `stats.py` | sign-flip test on program sums, CI by test inversion, stratum permutation, FU-10 verdict |
| `power.py` | power simulation that sizes hx-test |
| `selftest.py` | binary-free self-test of all of the above (the gate runs it) |

## Workflow

```sh
# 1. build the pool (binaries are required arguments; their sha256 is recorded)
python3 build.py --axon AXON --cortex CORTEX --seed S --split-seed T \
    --out /tmp/hx-pool.json --report /tmp/hx-build-report.json
python3 build.py --check /tmp/hx-pool.json --axon AXON --cortex CORTEX   # rebuild and compare
python3 build.py --inputs-digest                                       # builder-input digest for PREREG

# 2. pilot and calibrate on dev (run_ab.py --trials 3, rows keyed by (id, trial))
python3 calibrate.py --pool /tmp/hx-pool.json --rows DEV-H55.jsonl --rows-h45 DEV-H45.jsonl \
    --seed S --cap-seed C --report /tmp/calibration.json --write-sets .

# 3. size the test split from dev counts and measured pilot costs
python3 power.py --dev-rows DEV-H55.jsonl --cost-success 0.0013 --cost-failure 0.005 \
    --cap-usd 25 --seed S --out /tmp/power.json

# 4. after the run: criterion (i), strata (FU-1b), FU-10 route 2
python3 stats.py compare --base T0.jsonl --cand T1.jsonl --corpus hx-test.json --seed S
python3 stats.py strata --rows T0.jsonl --cand T1.jsonl --corpus hx-test.json --seed S
python3 stats.py route2 --t1 T1.jsonl --t2 T2.jsonl --corpus hx-test.json --seed S
```

Run any script with `--help` for its exact arguments.

## Decisions this code makes (beyond the spec text)

- **In scope means locatable.** Every task's symbol is ranked within
  run_ab's `--candidates` by `cortex locate` and is in `reached_by_all`, for
  every class (the spec names `reached_by_all` for `caller` only).
- **Per-program cap after cell selection.** The <= 8 tasks per test program
  cap is drawn by `calibrate.py` (`build.cap_per_program`, seeded) after the
  kept-cell filter and before any test run; capping first would shrink the
  test set for no gain. It never looks at outcomes.
- **Per program and cell at most `--per-cell-program` (2) tasks**, distinct
  symbols first, so one program cannot dominate a cell.
- **Empirical-Bayes evidence is task-level** (a cell's evidence is the sum of
  its tasks' k-trial rates over its task count): trials of one task are not
  independent, so counting them as such would overstate the evidence and shrink too little.
- **Cross-fit** selects cells on one fold's refitted shrinkage (candidates =
  cells eligible on all of dev) and measures the other fold's raw rate.
- **Power** uses the normal limit of the sign-flip null inside the
  simulation; `power.py --exact` cross-checks with the test itself.
- **caps eligibility.** The target is `@[contained]`, or narrower than its
  program (`axon caps --fn` lists no fs/net/exec kind while the program
  does); its pristine `--fn` caps never list fs/net/exec.
- **Knobs are derived, not stored opinions.** `--check` recomputes the knob
  from the recorded operator list (`ops`), the call depth or the failure
  text. The operator list itself is not re-derivable from the broken file;
  `--check` verifies that it fits the class and the knob.

## Limits

- The spent ledger and `PROTECTED_SETS` pins (../protected.py) stop
  accidental reruns of a protected set; they are not a security boundary. A
  copied or renamed corpus file bypasses them.
- Stratum D programs are public; the stratum permutation in `stats.py
  strata` is descriptive (`contamination-suspect`), not a gate.
- Many stratum D programs have one non-test function, where locating is
  trivial; the cell mix of stratum D is mostly `multi`.
- Built pools, reports and calibration outputs are not committed until the
  freeze (FU-1c step 5); only `hx-dev.json` and `hx-test.json` are.
