# Axon Autonomous Max — Build at Max Quality

## The loop prompt

When running unsupervised (`/loop`), use this prompt to build features at maximum quality:

```
You are building the Axon language compiler. Read all governance docs in order:
governance/README.md → REQUIREMENTS.md → BUILD_PROTOCOL.md → SPEC_TEMPLATE.md →
TESTING_STANDARD.md → CODE_REVIEW_RUBRIC.md → DEFINITION_OF_DONE.md →
ARCHITECTURE_INVARIANTS.md → BUG_HUNT_2026-05-31.md

Then pick the highest-priority open item from the backlog (BUG_HUNT first, then
REQUIREMENTS.md work queue). For each item:

1. FRAME: Read the requirement, state acceptance criterion, decide risk class
2. SPEC: Write/locate tech spec if structural change
3. RED TEST: Write failing test first, run it, watch it fail for the right reason
4. IMPLEMENT: Smallest change that turns the test green
5. WIDEN: Edge cases, adversarial, property tests, parity (interp ↔ codegen)
6. REVIEW: Self-review against CODE_REVIEW_RUBRIC.md as adversary
7. GATE: Full suite + clippy green BEFORE commit. &&-chain: test && clippy && commit
8. VERIFY: Run the feature for real (example/demo), update docs + REQUIREMENTS.md

Rules:
- Every Structural change updates ALL exhaustive matches across the pipeline
- Commit only on green (test + clippy). Never commit red.
- One feature per tick. Small, verified, reversible steps.
- If a tick can't reach Gate 6 green, revert to last green state.
- Cite requirement (Rn) and acceptance test in commit body.
- Update REQUIREMENTS.md %/Status in the same commit that completes the feature.
- Follow the honesty rule: verify clean before believing, distinguish confirmed from suspected.
- If you find a new bug, log it in BUG_HUNT_2026-05-31.md with severity and area.
```

## How to use

Run the loop with:

```bash
# In the /loop dynamic mode, paste the prompt above
# Or use the workflow tool to run a multi-agent build cycle
```

The loop iterates through the backlog, one item at a time, following all 8 gates.
Each tick = one feature through the full lifecycle, ending in a green gated commit.