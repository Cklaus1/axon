# Axon Build Protocol — How Every Feature Gets Built

**Audience:** the autonomous builder (and any human who joins).
**Contract:** every change to Axon follows this lifecycle. No exceptions for "small" changes —
small changes that skip the gates are exactly where regressions enter an unsupervised system.

This protocol is the *process spine*. It references but does not duplicate:
`DEFINITION_OF_DONE.md` (the exit checklist), `TESTING_STANDARD.md` (what tests to write),
`CODE_REVIEW_RUBRIC.md` (what to self-check), `ARCHITECTURE_INVARIANTS.md` (what must never break).

---

## The lifecycle (8 gates)

Each feature passes through these gates **in order**. A gate that fails sends you back, not forward.

```
0. FRAME      → know the requirement + acceptance criteria
1. SPEC       → write/locate the tech spec (SPEC_TEMPLATE.md)
2. RED TEST   → write the failing test that encodes the acceptance criteria
3. IMPLEMENT  → smallest change that makes the test green
4. WIDEN      → adversarial/edge/property tests; codegen↔interp parity
5. REVIEW     → self-review against CODE_REVIEW_RUBRIC.md
6. GATE       → full suite + clippy green BEFORE commit; commit only on green
7. VERIFY     → run it for real (example/demo), update docs + REQUIREMENTS.md
```

---

### Gate 0 — FRAME

- Find the requirement in `governance/REQUIREMENTS.md`. If the work doesn't map to a requirement,
  either it's a bug-fix (allowed, but still gated below) or it's scope creep (stop, add a row first).
- State the **acceptance criterion** in one sentence: "this is done when `<observable>`."
- Decide the **risk class** (drives how heavy gates 1, 4, 5 are):
  - **Trivial** (typo, doc, single-line obvious fix) → gates 1 and 4 may be light; 2,3,6 still mandatory.
  - **Standard** (new builtin, stdlib fn, demo) → full lifecycle, normal depth.
  - **Structural** (AST node, pipeline phase, type rule, security boundary, codegen) → full lifecycle,
    *maximum* depth: spec required, parity required, adversarial tests required.

### Gate 1 — SPEC

- **Structural** changes: a tech spec (`SPEC_TEMPLATE.md`) is **required before code**. Architectural
  forks are cheap on paper and expensive in a merged compiler.
- **Standard** changes: a spec is optional but the acceptance criterion + test plan must be written in
  the commit body or the test's doc-comment.
- The spec names the **error codes** the feature introduces (E-codes / W-codes) — invented at spec time,
  not improvised in code.

### Gate 2 — RED TEST (test-first is mandatory)

- Write the test that encodes Gate 0's acceptance criterion. **Run it. Watch it fail for the right reason.**
  A test that was never red proves nothing.
- Place it per `TESTING_STANDARD.md` (CLI e2e in `tests/cli_run.rs`, unit in the module, etc.).
- For interpreter features: the test asserts *observable behavior* (exit code, stdout, error code), not
  internal structure — so it survives refactors.

### Gate 3 — IMPLEMENT

- Smallest change that turns the red test green. Resist building Gate-4 generality now.
- Match surrounding code's idiom, naming, comment density (read the neighbors first).
- If implementing touches the AST: **every** exhaustive `match` across the pipeline must be updated in
  the same change (parser, resolver, infer, checker, borrow, capabilities, codegen, fmt, mono, comptime).
  Build with `--features codegen` to catch the codegen arms the default build skips.

### Gate 4 — WIDEN (this is where "deep testing" lives)

Per risk class, add until the feature is *trustworthy*, not just *passing*:

- **Edge cases:** empty, zero, negative, max, off-by-one, unicode, the boundary the spec named.
- **Adversarial:** malformed input, deep nesting, recursion bombs, hostile sizes. The interpreter must
  *fail gracefully* (catchable panic / clean error), never SIGABRT/overflow the host.
- **Property tests** (when applicable): invariants like round-trip (`from_str(to_str(x)) == x`),
  commutativity, idempotence, monotonicity.
- **Parity:** if the feature exists in both interpreter and codegen, a parity test asserts identical
  observable output. Interpreter is the reference semantics (`ARCHITECTURE_INVARIANTS.md` I-7).
- **Performance:** if the change is on a hot path or claims a perf property, a benchmark/assertion guards it.

### Gate 5 — REVIEW

- Self-review the full diff against `CODE_REVIEW_RUBRIC.md` **as an adversary trying to find the bug.**
- For Structural changes, run a second independent pass (or spawn a review sub-agent) — single-pass
  self-review misses its own blind spots.
- Fix findings before Gate 6. Do not commit "will fix in follow-up" on a correctness finding.

### Gate 6 — GATE (the commit barrier)

- **Run the full workspace suite + clippy. Commit ONLY on green.** `&&`-chain them so a red state
  cannot land (`gate-commits-on-tests` memory; this has bitten the project before).
  ```bash
  cargo test --workspace --no-default-features 2>&1 | grep -E "FAIL|error\[" && echo RED || \
  (cargo clippy -p axon-core --no-default-features --no-deps 2>&1 | grep -E "^(error|warning)" && echo LINT || \
   git add -A && git commit -F <msg-file>)
  ```
- Commit message: what + *why*, names the requirement (Rn), cites the acceptance test, ends with the
  required `Co-Authored-By` trailer. No backticks/`$()`/`<` in `-m` strings (shell mangles them; use `-F`).
- Branch first if on the default branch. Push only when the task authorizes it.

### Gate 7 — VERIFY

- **Run the feature for real** — an example, a demo, the actual CLI path a user hits. Tests can pass while
  the integrated experience is broken.
- Update: the relevant `examples/`, `spec/` or `stdlib.md`, `SESSION_STATUS.md`, and **`REQUIREMENTS.md`
  `%`/`Status`** in the same commit that completed the feature.
- If the requirement reached DONE, the commit body cites the now-passing acceptance test.

---

## Anti-patterns (auto-reject in self-review)

- **Green without red.** A test added already-passing tests nothing. Always see it fail first.
- **Skipping parity** because "the interpreter is what runs." Codegen drift is silent until someone builds native.
- **Widening before red-green.** Build the general thing only after the specific thing works and is tested.
- **`unwrap()`/`expect()` on user-reachable paths.** User input → `Result`/graceful panic, never host abort.
- **Silent capability widening.** Any change that lets code reach net/fs/exec must update `ARCHITECTURE_INVARIANTS.md`
  TCB section and have an explicit allow/deny test.
- **Doc drift.** Shipping behavior without updating the spec/status is how the next builder inherits a lie.

---

## Cadence for autonomous loops

When running unsupervised (e.g. `/loop`), one "tick" = one feature through all 8 gates, ending in a
green gated commit + push. Do not batch multiple unverified features into one tick — small, verified,
reversible steps are how an unsupervised system stays correct. If a tick can't reach Gate 6 green,
revert to the last green state and record why; never leave the tree red.
