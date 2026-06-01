# Code Review Rubric — Self-Review Before Every Commit

`BUILD_PROTOCOL.md` Gate 5 requires this pass. Review your own diff **as an adversary trying to find the
bug**, not as the author hoping it's fine. For Structural changes, run it twice (or spawn an independent
review sub-agent) — single-pass self-review is blind to its own assumptions.

Go in this order: **correctness first** (a beautiful wrong answer is still wrong), then safety, then
quality. Stop and fix any correctness/safety finding before committing.

---

## A. Correctness (find the bug)

- [ ] **Does it actually do what the acceptance criterion says?** Re-read Gate 0's one-sentence criterion;
      trace the code against it. Don't trust the test passed — trace it.
- [ ] **Off-by-one / boundary:** first element, last element, empty, single-element, max. Each handled?
- [ ] **Every `match` exhaustive and correct** — not just compiling, but the right arm for each case.
      AST changes: did *all* pipeline matches get the new arm (built `--features codegen`)?
- [ ] **Error paths return the right thing.** Every `Result`/`Option` branch tested mentally: does the
      `Err`/`None` case do the correct thing, or just compile?
- [ ] **No silent-wrong-value (I-9).** Could this return a plausible number on bad input instead of
      erroring? Overflow, undefined lookup, inverted args, empty collection — what does each *return*?
- [ ] **Concurrency:** any shared mutable state? File appends? Could two runs corrupt it? (I-12, Bug #12.)

## B. Safety (the boundaries that matter for an autonomous system)

- [ ] **No host abort on user input (I-4).** Any `unwrap()`/`expect()`/array-index/`panic!` reachable from
      user-supplied source or data? If yes, is there a graceful-failure test?
- [ ] **Success signal honest (I-8).** Does every failure path here lead to a non-zero exit? Diagnostics to
      stderr, program output to stdout?
- [ ] **Capability boundary (I-11).** Does this add or widen a path to net/fs/exec? If so: invariant updated,
      allow + deny tests added?
- [ ] **No secrets / absolute private paths / keys** in code, tests, or fixtures.
- [ ] **Determinism (I-10).** Did you introduce time/RNG/iteration-order nondeterminism into a graded path?

## C. Exhaustive testing (did you actually go deep — `TESTING_STANDARD.md`)

- [ ] **Red-first:** the new test failed before the change, for the right reason.
- [ ] **Each behavior-table row has a test.** Each error code has a test that triggers it.
- [ ] **Adversarial input tested:** malformed, oversized, deeply nested, unicode.
- [ ] **Parity test** if dual-path (interp↔codegen).
- [ ] **Property test** if an invariant exists (round-trip, commutativity, idempotence).
- [ ] **Whole suite green + clippy clean** — not just the new tests.

## D. Quality & reuse (after correctness — never instead of it)

- [ ] **Reuse:** does a helper/builtin already do this? (e.g. `dict_get_or`, `values_equal`, `next_rand_u64`.)
      Don't reimplement an existing pattern.
- [ ] **Simplification:** can a `match`-on-`Option` become `dict_get_or`/`unwrap_or`? Is there dead code,
      a redundant clone, a needless intermediate collection?
- [ ] **Naming & idiom:** matches the neighbors? Comment density matches surrounding code?
- [ ] **No premature generality:** built the specific thing the test needs, not a speculative framework.
- [ ] **Performance:** no accidental O(n²), no allocation in a hot loop without reason.

## E. Documentation & traceability

- [ ] Public surface documented (`spec/stdlib.md` / relevant spec).
- [ ] `SESSION_STATUS.md` + `REQUIREMENTS.md` updated if the capability/percentage changed.
- [ ] Commit message: what + *why*, names the requirement (R<n>), cites the now-passing test, correct
      `Co-Authored-By` trailer, no shell-hostile chars in `-m` (use `-F`).

---

## Adversarial prompts to ask yourself (the bug is usually behind one of these)

- "What input makes this return a wrong value *without* erroring?" (I-9 — the worst class.)
- "If I run this in a pipe / in CI / concurrently, what breaks?"
- "What does the *error* message tell a confused new user? Does it teach the fix or just state the failure?"
- "Did I test the `None`/`Err` branch, or just write it?"
- "If `git revert` this commit, is the tree still green?"
- "Would the journey/red-team layer (`TESTING_STANDARD.md` L6) catch something my unit tests can't?"

A review that finds nothing on a Structural change is a review that wasn't adversarial enough. Look again.
