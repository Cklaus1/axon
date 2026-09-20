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

| | |
|---|---|
| verified, and the file really is clean | 32 / 44 (72.7%) |
| verified but the file still fails | **0** |
| failed, workspace restored byte-for-byte | 12 / 12 |

The 12 failures are all exit 21 — the ranking put the broken function outside
the top three, so the loop went in circles and said so. None of them left the
workspace altered.

Read the 72.7% as an upper bound on repair, not as a repair rate: a real
generator proposes text that may be wrong, and this one cannot.
