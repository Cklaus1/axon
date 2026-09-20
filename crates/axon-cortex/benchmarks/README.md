# Localization accuracy, measured

`cortex locate` ranks the functions a defect might be in. These scripts measure
how often it is right, against real Axon code rather than a fixture written to
be repairable.

```bash
cargo build -p axon-core --bin axon && cargo build -p axon-cortex --bins
python3 benchmarks/topk.py ../../examples/stdlib/*.ax ../../examples/*.ax
```

## Method, and what it does not cover

A defect is injected into one function at a time — a single operator swap in
its body — and the mutant is KEPT only if the file still compiles and at least
one non-hidden `@[test]` now fails. A mutant that breaks the build or that no
check notices is discarded: neither asks the question. The hidden check is the
file's last `@[test]`, excluded from the evidence exactly as the production
path excludes it.

An operator swap is a real behavioural change that still compiles, which is the
class localization is for. It is **not** representative of every defect — a
missing branch, a wrong constant, or a defect spanning two functions would
exercise the same machinery differently, and none of those are measured here.

Scoring is four-way rather than pass/fail. A correct name, a wrong name, an
honest refusal and an error have four different remedies, and collapsing
refusals into failures would make a system that knows what it does not know
look identical to one that guesses.

## A correction, recorded

The first run of this benchmark chose the adjudicating check up front, as the
file's last `@[test]`. That check usually did not exercise the injected defect,
so it PASSED on the broken file — and 54 of 80 runs reported `verified_done`,
most of them at step 1, having changed nothing at all. The loop was reporting,
accurately, on an experiment that could never have concluded.

Two things changed as a result. The harness now picks the adjudicator per
mutant, from the checks that actually FAIL, and requires a second failing check
to remain as visible evidence. And `cortex repair` refuses outright when the
named check already passes (exit 26), because a check that would accept the
file unchanged cannot witness a repair of it.

The earlier figures (8.8% / 57.5% / 90.0%) are **withdrawn**, not restated: the
trial set below (44) is not comparable to the earlier 80, and the binary is not
the same binary.

## Results (2026-09-20, 44 trials)

### Localization

| | |
|---|---|
| top-1, **sole** top-ranked candidate | 38.6% |
| top-1 including alphabetical tie-breaks | 50.0% |
| top-2 | 79.5% |
| top-3 | 93.2% |
| truth absent from the ranking | 0.0% |

The first row is the honest headline. The others count trials where the truth
tied at the top and won an alphabetical tie-break, which is an accident of
spelling rather than evidence — the per-trial rows in the JSON carry
`tie_at_top` so the distinction stays checkable.

Two numbers decided the design: the truth is never absent from the ranking, and
it reaches rank 3 in 93% of trials. So the run walks the top three instead of
stopping at one, and a tie became an ordered plan rather than a dead end.

### The loop itself, with a perfect generator

The generator is handed the body the file had before it was broken, so it
cannot propose anything wrong. Every remaining failure is the control loop's.

| `--candidates` | verified & file really clean | false successes | workspace restored on failure |
|---|---|---|---|
| 1 | 20 / 44 (45.5%) | 0 | 24/24 |
| **3 (default)** | **38 / 44 (86.4%)** | **0** | 6/6 |
| 5 | 40 / 44 (90.9%) | 0 | 4/4 |

Every verified run repaired the function that was actually broken, and no run
ever reported success on a file that still fails. Failures leave the workspace
byte-identical.

Walking is worth 41 points over stopping at the first candidate. The step from
3 to 5 buys 4.5 more for 67% more episodes — and each episode is a model call,
so the default stays at 3 and the knob is documented rather than raised.

Read these as an upper bound on repair, not as a repair rate: a real generator
proposes text that may be wrong, and this one cannot.

#### What the first version of this table got wrong

It reported 32/44 at `--candidates 3`. Two control-loop defects found
afterwards account for the gap, and both were invisible to every hand-written
fixture:

* **a file that compiles WITH WARNINGS could never be repaired at all.** A
  clean file makes selection claim completion, the claim is refused, and that
  refusal is what drives it to propose a patch. A warned file made selection
  run a check instead — and running a check changes no bytes, so the next step
  saw the same workspace and the same choice and stopped as `NoProgress`,
  before a patch was ever considered. Nine trials failed this way, every one
  with the right function ranked first or second. Every fixture in this crate
  compiled clean; most of `examples/stdlib/*.ax` does not.
* **an exact-name verdict rule had been over-applied.** It is right for the
  adjudicating check, which is an exact test name the operator supplied. It is
  wrong for the visible check, which is named after the SYMBOL under repair —
  no test is ever called `gcounter_increment`, so that verdict was permanently
  false and the loop learned nothing from running it.
