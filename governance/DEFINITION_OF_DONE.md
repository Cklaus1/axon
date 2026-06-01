# Definition of Done

A feature is **done** when every box below is checked. This is a *binary* gate — "mostly done"
is not done. The autonomous builder treats an unchecked box as a blocking defect, not a TODO.

Copy this checklist into the feature's final commit body (or PR description) with each box
checked and the evidence inline.

---

## 1. Correctness

- [ ] The acceptance criterion from `BUILD_PROTOCOL.md` Gate 0 is met and demonstrated.
- [ ] A test exists that was **red before** the implementation and is **green after** (cite it).
- [ ] All error paths return `Result`/`Option` or a *catchable* panic — no host process abort on any
      user-reachable input.
- [ ] Exhaustive `match`es across the pipeline are complete (built with `--features codegen` to confirm
      codegen arms compile).

## 2. Testing depth (see `TESTING_STANDARD.md`)

- [ ] Unit tests for the core logic.
- [ ] Integration/CLI e2e test asserting *observable* behavior (exit code / stdout / error code).
- [ ] Edge cases covered: empty, zero, negative, max, boundary, unicode where relevant.
- [ ] Adversarial input tested: malformed, oversized, deeply nested — fails gracefully.
- [ ] Property test where an invariant exists (round-trip, commutativity, idempotence, monotonicity).
- [ ] **Interpreter↔codegen parity** test if the feature exists in both paths.

## 3. Quality bar

- [ ] `cargo test --workspace --no-default-features` is **fully green** (0 failures).
- [ ] `cargo clippy -p axon-core --no-default-features --no-deps` is **clean** (0 warnings, 0 errors).
- [ ] Self-review against `CODE_REVIEW_RUBRIC.md` done; all correctness findings resolved.
- [ ] No `unwrap()`/`expect()`/`panic!` reachable from user input without a graceful-failure test.
- [ ] Code matches surrounding idiom, naming, and comment density.

## 4. Performance

- [ ] No new allocation in a hot loop without justification.
- [ ] If the change claims or affects a performance property, a benchmark/assertion guards it.
- [ ] No accidental O(n²) where O(n) was available (or it's documented as acceptable at expected scale).

## 5. Security / capability boundary

- [ ] No new path reaches net/fs/exec without going through the declared-capability boundary.
- [ ] If the TCB surface changed, `ARCHITECTURE_INVARIANTS.md` §TCB is updated **and** an allow/deny
      test guards the new boundary.
- [ ] No secrets, keys, or absolute private paths committed.

## 6. Documentation & traceability

- [ ] Public surface (builtin, syntax, error code) documented in `spec/stdlib.md` / relevant `spec/*.md`.
- [ ] A runnable `examples/` artifact demonstrates the feature (for user-facing features).
- [ ] `SESSION_STATUS.md` reflects the new capability.
- [ ] `governance/REQUIREMENTS.md` `%`/`Status`/`Gap` updated; if a requirement reached DONE, the
      passing acceptance test is cited.
- [ ] Commit message names the requirement (Rn), states *why*, ends with the `Co-Authored-By` trailer.

## 7. Reversibility

- [ ] The change is a small, self-contained, revertible commit (not a multi-feature batch).
- [ ] The tree is green at commit time — `git revert <sha>` would leave a working build.

---

## Severity tiers (what "done" requires by risk class)

| Risk class | Mandatory boxes |
|---|---|
| **Trivial** (doc/typo/obvious one-liner) | §1.3, §3.1, §3.2, §7. (Test still required if behavior changes at all.) |
| **Standard** (builtin, stdlib fn, demo) | All of §1, §2 (minus parity if single-path), §3, §6, §7. |
| **Structural** (AST, pipeline phase, type rule, codegen, security boundary) | **Every box, no exceptions.** Plus a spec (§ `SPEC_TEMPLATE.md`) and a second review pass. |

If you cannot truthfully check a mandatory box, the feature is **not done** — keep it on the branch,
record the blocker, and do not mark the requirement complete.
