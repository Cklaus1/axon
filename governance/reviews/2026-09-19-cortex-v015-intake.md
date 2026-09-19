# Cortex build package v0.15 — intake review

Placed at `docs/axon_cortex_v0_15/` (114 files, 1.9MB). v0.7 kept: it is the
source cited by `crates/axon-cortex/src/lib.rs` and three `tasks/*.md`.

## Integrity — verified, not assumed

`SHA256SUMS_v0_15.json` checked in BOTH directions: **113/113 files match**,
zero mismatches, zero missing, and the only file present-but-unlisted is the
manifest itself (which cannot contain its own hash). The package also ships
`tools/validate_package.py`; run against the placed copy it returns
`result: PASS`, `errors: []`.

## What it is, in numbers

| | |
|---|---|
| specifications | 35 (CX-00 … CX-34) |
| work packages | 152 (B00 … B151) |
| proposed product gates | **247** |
| gates with `product_result` other than `NOT_RUN` | **0** |
| work packages with `status` other than `Not started` | **0** |

The validator states its own scope without being asked: *"Documentation
consistency only. No Axon build, runtime, safety gate or model benchmark
executed."* Every changelog from v0.8 onward repeats it.

**That honesty is the single most important property of this package, and it
is the thing most easily destroyed on intake.**

## The intake risk, stated plainly

This repo currently has an evidence-integrity defect: of 37 `scripts/*.sh`
cited as evidence in `governance/REQUIREMENTS.md`, **7 are invoked by nothing**
(10 before today's wiring). A requirement can read as verified while its cited
gate has never run.

v0.15 proposes **247 more gates**, all `NOT_RUN`. The package is scrupulous
about saying so. But the moment those IDs are copied into a status table — a
roadmap row, a REQUIREMENTS entry, a "coverage" summary — the `NOT_RUN` field
does not travel with them, and 247 unexecuted checks become 247 apparent ones.
That would multiply the existing defect by roughly seven.

**Recommendation: do not import gate IDs into any status document without
carrying their execution class as a field.**

## What to do differently — five concrete changes

**1. Adopt the package's own vocabulary instead of inventing one.**
`gate_manifest.json` already carries `implementation_status` and
`product_result` per gate. That is precisely the execution-class schema the
repo needs and lacks. Extend the value set rather than designing fresh:
`strict_gate` · `glob_invoked` · `conditional` · `recorded_benchmark` ·
`external_hardware` · `orphaned` · `NOT_RUN`.

**2. Make the evidence link checkable, in both directions.**
The repo's env registry and `AXON_REFERENCE.md` already have two-directional
drift gates; `REQUIREMENTS.md`'s evidence column does not. A gate that reads
the register, extracts every cited `scripts/*.sh`, and fails when a
claimed-verified requirement cites an orphaned script would have caught all
seven. Applying the same rule to imported CX gates makes v0.15's honesty
enforced rather than merely stated.

**3. Wire `tools/validate_package.py` into `scripts/gate.sh`.**
It is a real check that currently runs nowhere — the same class as the 16
orphaned scripts found today. It is fast, has no toolchain needs, and would
catch a package edited after intake.

**4. Sequence from the DAG, not the document.**
`task_manifest.json` gives every work package `depends_on`, `stage`, `lane`
and `gate_targets`. Exactly **one** task has no dependencies (`B00`), and the
stages are M0 (5) → M1 (30) → M2 (41) → Compiler (4), across six lanes
(contracts 13, evaluation 27, runtime 25, executor 6, models 5, observer 1).
The 655KB `AXON_CORTEX_MASTER.md` is not the artifact to plan from; the
manifests are machine-readable and can be gated.

**5. No rework is owed to the existing crate.**
`schemas/PROTOCOLS.md` grew 15.8KB → 24.1KB across v0.8–v0.15, and the diff is
**strictly additive**: zero removed or changed lines. The four contracts
`crates/axon-cortex` implements (CX-00 contract, CX-02 observation, CX-03
grants, CX-10 evidence) are byte-identical. The crate is 4-of-35 specs and
INCOMPLETE, but it is not STALE — a distinction worth stating because the
remedies differ entirely.

## What this review does not establish

Nothing here executes a Cortex gate, benchmark, or model evaluation. Integrity,
document consistency, DAG shape and schema-compatibility are checked; the
claims inside the 35 specs are not. The package says the same about itself, and
that agreement should not be mistaken for independent confirmation.
