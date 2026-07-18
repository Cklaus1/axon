# Axon Governance — How an Autonomous Builder Ships at Max Quality

This directory is the **operating system for building Axon without human supervision**. It exists so an
auto-ASI (and any human) builds every feature to a world-class bar — specified, tested deeply, reviewed
adversarially, and verified — instead of grading its own happy-path homework.

Read these in order; they form a closed loop.

| File | Role | Read it when |
|---|---|---|
| **`REQUIREMENTS.md`** | The map. Top-10 requirements ↔ PRD ↔ current % ↔ gaps ↔ owning spec ↔ acceptance tests. | Before starting *any* work — find your requirement. |
| **`BUILD_PROTOCOL.md`** | The lifecycle. 8 gates every feature passes: frame → spec → red test → implement → widen → review → gated commit → verify. | Every feature, every time. |
| **`SPEC_TEMPLATE.md`** | The spec format. Motivation, surface, semantics table, error codes, invariants, test plan, acceptance, rollback — plus the required `spec-meta` front-matter, §13 Dependency DAG, and §14 Evidence ledger. | Before code on any Structural change (Gate 1). |
| **`EXECUTION_MODEL.md`** | The truth-maintenance layer: task DAG, evidence graph ("a claim without a re-runnable evidence pointer is not a valid status"), spec cross-reference front-matter, and the inner/outer loop split. Mechanized by `scripts/verify_all_specs.sh`. | Editing any status claim; running an outer-loop sweep; claiming a new spec number. |
| **`TESTING_STANDARD.md`** | The depth doctrine. 6 test layers incl. **Layer 6 journey/red-team** — the layer that catches what units can't. | Writing tests (Gates 2 & 4). |
| **`CODE_REVIEW_RUBRIC.md`** | The self-review. Adversarial checklist, correctness-first. | Before every commit (Gate 5). |
| **`DEFINITION_OF_DONE.md`** | The exit gate. Binary checklist; "done" means every box checked. | Closing a feature (Gate 7). |
| **`ARCHITECTURE_INVARIANTS.md`** | The never-break rules (I-1..I-15). Cite them in reviews; changing one needs a proposal. | Continuously; any change near a boundary. |
| **`specs/`** | Tech specs for the unbuilt high-risk requirements (R3/R6/R7/R10), each framed around its decisive fork. | Before building those requirements. |
| **`BUG_HUNT_2026-05-31.md`** | The first full-journey red-team. 30 findings; the confirmed ones are the top fix queue. | Now — it's the current defect backlog. |

---

## The core idea

A human team gets quality from **division of distrust**: the author writes, a reviewer doubts, QA breaks
it, a PM checks it solved the user's problem. An autonomous builder has to play **all four roles against
itself** — and the failure mode is that it's a weak adversary to its own work (it wrote a passing test, so
it believes the feature works). These files force the distrust to be real:

- The **red test** (Gate 2) makes "it works" mean "the failure I predicted is now fixed," not "it compiled."
- The **journey/red-team layer** (Testing L6) makes the builder drive its *own product* like a confused,
  hostile, retrying user — the only way to find the integration bugs unit tests structurally cannot see
  (the 2026-05-31 hunt found 30, zero of them unit-test-catchable).
- The **invariants** make "don't break the foundation" a checkable list, not a hope.
- The **requirements matrix** makes "are we done" a traceable fact, not a feeling.

## The honesty rule

The bug hunt that seeded this directory shipped a **false Critical** because a shell pipe masked an exit
code. That's not embarrassing — it's the point: *even adversarial testing must verify its own
reproductions clean before believing them.* An autonomous system that trusts its first reading of any
signal — a test result, an exit code, a benchmark — will accumulate confident wrongness. Verify clean,
distinguish confirmed from suspected, retract when wrong. That discipline is now in `TESTING_STANDARD.md`
Layer 6 and is the meta-invariant of the whole suite.
