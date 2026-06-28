# Goal: complete the remaining Axon roadmap requirements

Work through the UNFINISHED requirements of this project, in roadmap order,
one well-verified increment per iteration.

## Source of truth (read each iteration, in this order)
1. `ROADMAP.md` — forward Phases 5–14+, the three load-bearing pillars
   (Proof, Containment, Goal-directedness), and the friction-derived gap
   list. Anything not advancing a pillar is out of scope.
2. `CLAUDE.md` — Phase Status table (current truth of what's shipped) +
   project conventions. Follow its conventions exactly.
3. `spec/compiler-phase5.md`, `spec/compiler-phase6.md`, … — per-phase
   specs; the spec is the requirement.
4. `STATUS.md` is a stale Phase-4 snapshot — trust it only for Phases 3/4.

## Branch discipline (hard rules)
- Work ONLY on git branch `asiloop/roadmap` (create from current
  `merge-asi-layer3` HEAD if missing; check it out each iteration).
- NEVER commit to `merge-asi-layer3` or `main` directly.
- Pre-existing uncommitted changes (e.g. in `crates/axon-surface`,
  `crates/axon-wasm`) are NOT yours: never commit, revert, or extend them.

## Each iteration
1. Determine the next unshipped requirement: lowest-numbered incomplete
   phase / gap-list item per ROADMAP.md + the CLAUDE.md phase table.
2. Implement the smallest complete, verifiable slice of it.
3. Verify: `cargo build` and `cargo test` (workspace) must pass; run
   `./dev.sh` checks if applicable; phase acceptance criteria from the
   spec are the bar. A slice without passing verification does not count.
4. Update the relevant tracking docs (CLAUDE.md phase table / CHANGELOG.md)
   to reflect what actually shipped — keep the docs truthful.
5. Commit to the branch with a clear message: `feat(phaseN): …` or
   `fix/test/docs(…): …`.

## When to ask instead of guessing
Use `__ASILOOP_NEEDS_INPUT__ <question>` when a roadmap item requires a
design decision the docs leave open or contradict each other on (e.g.
spec vs. ROADMAP conflicts, SMT solver choice, breaking syntax changes).
Do not invent language-design decisions.

## Definition of done (long-horizon — verify ALL before declaring)
- Every forward phase and gap-list item in ROADMAP.md is shipped (or
  explicitly descoped in ROADMAP.md by an earlier human decision).
- Full workspace `cargo build` + `cargo test` pass.
- The acid tests named in ROADMAP.md pass end-to-end.
This will take many iterations; do not declare done early.
