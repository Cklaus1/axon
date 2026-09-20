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

## Results (2026-09-20, 80 trials)

The first version took every function a failing check calls directly and
refused whenever more than one turned up:

| | |
|---|---|
| correct | 8.8% |
| wrong | 0.0% |
| refused | 91.2% |

Never wrong, and nearly useless: a real check calls several helpers where the
hand-written fixture called exactly one.

Ranking by Ochiai over the call spectrum — which uses the PASSING checks as
evidence, and follows calls transitively:

| call depth | top-1 | top-3 | truth absent from the ranking |
|---|---|---|---|
| 1 (direct only) | 51.2% | 73.8% | 20.0% |
| 2 | 58.8% | 88.8% | 5.0% |
| **4 (shipped)** | 57.5% | **90.0%** | **3.8%** |
| 8 | 57.5% | 90.0% | 3.8% |

Two things decided the design. Top-1 is only 57.5%, but the true function is at
rank 2 in half of the cases where rank 1 is wrong — so the run walks the top
three rather than stopping at one. And depth stops paying past 4, so that is
where it is capped.

A wrong candidate costs budget and never costs correctness: the hidden check
adjudicates every attempt, and each attempt starts from the state the run
found.
