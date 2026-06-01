# Testing Standard — What "Deep Testing" Means Here

An autonomous system cannot lean on a human to "eyeball it." Tests are the *only* evidence the
builder has that its work is correct. So tests here are held to a higher bar than a human team's:
they must encode the spec, catch adversarial inputs, and guard the seams where this specific
architecture drifts.

This is the *doctrine*. `BUILD_PROTOCOL.md` Gates 2 & 4 say *when* to test; this says *how* and *how much*.

---

## The six test layers (a feature earns its layers by risk class)

### 1. Unit — the logic in isolation
- Lives next to the code (`#[cfg(test)] mod tests` in the module).
- Tests one function's contract: given X, returns Y; given bad X, returns the right error.
- Fast, deterministic, no I/O.

### 2. Integration — the phase in context
- `crates/axon-core/tests/*.rs`. Tests a pipeline phase against realistic input
  (`integration_fixtures.rs` pattern).
- Asserts diagnostics by **error code** (E0101, W0002, …), not by message text — messages change,
  codes are contract.

### 3. CLI end-to-end — the whole pipeline a user hits
- `tests/cli_run.rs`. Runs the actual `axon` binary on a real `.ax` file.
- **Asserts observable behavior only:** exit code, stdout substring, stderr error code. Never internal
  structure — these tests must survive refactors.
- This is the **primary** evidence layer for interpreter features. Every user-facing feature gets one.

### 4. Adversarial / fuzz — hostile input
- Malformed source, deeply nested expressions, recursion bombs, oversized literals, unicode edge cases.
- **The contract: graceful failure, never host abort.** A catchable panic or clean diagnostic is a pass;
  SIGABRT, stack overflow, or hang is a *defect in the language*, not the test.
- The project already enforces this (`RECURSION_LIMIT`, `MAX_EXPR_DEPTH`, 1 GiB worker stack). New
  recursive/parsing surface must add its own adversarial test.

### 5. Property-based — invariants over generated inputs
- For any law the feature claims: `from_str(to_str(x)) == x`, `f(a,b) == f(b,a)`, `sort(sort(x)) == sort(x)`,
  `parse(render(ast)) ≅ ast`.
- Until R8 lands the in-language `forall`, write these as Rust loops over representative inputs (and as
  the *first* consumer of `forall` once it ships — dogfood it).

### 6. Journey / red-team — the user's whole path, adversarially
This is the layer that catches what every layer above misses. Layers 1–5 test *units and seams*; this
tests the **integration between the CLI, the user's intent, and the success/failure signal** — where the
highest-trust bugs actually live (see `governance/BUG_HUNT_2026-05-31.md`: all 30 findings were in this
space, none catchable by a unit test).

**Run quarterly, after any CLI/exit-code/error-path change, and before declaring a requirement DONE.**

- **Drive the real product across ICP journeys**, not isolated functions: first-run, PRD/goal intake,
  optimization, KPI/`trace` review, capability config, multi-file integration, formatter round-trip,
  deployment gates.
- **Use messy realistic behavior:** vague/incomplete input, contradictory edits, retries after failure,
  wrong file extensions, missing env vars, concurrent runs, degenerate args (empty, inverted, max).
- **The non-negotiable invariant this layer guards — the honesty of the success signal:**
  - Every failure mode (panic, type error, failed deploy-gate, invalid goal file, missing module,
    missing file) **must exit non-zero.** An autonomous loop / CI driving Axon depends entirely on this.
    A test that runs a failing program and asserts `exit != 0` is worth more than 100 unit tests here.
  - **No silent success on degenerate input:** undefined fn name, inverted args, overflow, empty program
    must error, not return a plausible-looking value (Bugs #5, #6, #19, #27).
  - **Diagnostics to stderr, program output to stdout** — always, so pipelines stay clean.
  - **Error messages teach the fix:** the correct syntax, the missing env var, the required sections.
- **Encode the findings as permanent regression tests** in `tests/cli_run.rs` — once a journey bug is
  found, it gets a CLI e2e test so it can never silently return.
- **Verify the reproduction clean before logging a Critical.** Exit codes especially: a shell pipe
  (`axon run x | head`) returns the *last* command's exit code, not axon's. Re-run as
  `axon run x >/dev/null 2>&1; echo $?` before claiming "exits 0". The 2026-05-31 hunt shipped a false
  Critical exactly this way — the test harness masked the real signal. The adversary must also be
  adversarial *toward its own methodology*.

> **Why this is mandatory for an auto-ASI:** an autonomous builder that only runs its own green unit
> suite is grading its own homework. It writes code, writes a test that passes, and concludes success —
> while the integrated CLI exits 0 on a crash and the founder's CI deploys a broken artifact. This layer
> is the adversary the builder cannot be for itself on the happy path. Schedule it; do not skip it
> because "the unit tests are green."

---

## Architecture-specific seams (test these or they will drift)

These are the places *this* codebase silently breaks. Every relevant change tests the seam.

- **Interpreter ↔ codegen parity.** Interpreter is the reference semantics. Any feature in both paths
  needs a parity test: same `.ax`, assert identical observable output. Codegen drift is invisible until
  someone builds native — which is exactly when you can't afford surprises.
- **AST exhaustiveness.** Adding an AST node touches ~10 exhaustive matches. The compile *is* the test —
  build with `--features codegen` so the codegen arms are checked, not just the default `--no-default-features`.
- **Lexer/parser greedy rules.** Float-eats-dot (`t.0.1`), ASI continuation (leading operator), interp
  braces — these have bitten before (see memory files). New surface near them gets a targeted test.
- **Capability boundary.** Every `@[contained]`/policy change needs a paired *allow* test (compliant code
  passes) and *deny* test (violating code is rejected with the right E-code).
- **Provenance / determinism.** Optimizer and `@[adaptive]` features assert provenance accumulates and
  that pure-read queries (`goal_best_score`, `goal_count`) do **not** mutate it.

---

## Coverage expectations

Not a line-coverage number (gameable). Instead, **behavioral coverage** — every branch of the spec's
behavior table has a test:

- Every error code the feature can emit has a test that triggers it.
- Every documented input shape has a test (the happy path *and* each rejection).
- Every boundary named in the spec (min/max/empty/zero) has a test.
- Every "returns Option/Result" has both the `Some`/`Ok` and `None`/`Err` test.

If the spec lists a behavior and no test exercises it, coverage is incomplete regardless of the line %.

---

## Performance testing

- Hot-path changes carry a benchmark or an assertion (e.g. "converges in < N evals", "trace length < cap").
- Perf claims in a commit message must be backed by a number in a test or the commit body, measured, not
  asserted from intuition.
- Regression guard: if a perf property mattered enough to claim, it matters enough to pin with a test that
  fails if it regresses.

---

## Test hygiene

- **Red first, always.** A test that never failed proves nothing (`BUILD_PROTOCOL.md` Gate 2).
- **One behavior per test**, named for the behavior (`goal_run_random_finds_global_optimum_on_multimodal`),
  so a failure name tells you what broke.
- **Deterministic.** Seed RNG, pin time, avoid network. Flaky tests are worse than no tests — they train
  the builder to ignore red.
- **Self-cleaning.** Temp files in `std::env::temp_dir()`, removed on both success and failure paths.
- **Assert the contract, not the implementation.** Exit codes and error codes over internal field values.

---

## The gate

`cargo test --workspace --no-default-features` green **and** `cargo clippy` clean is the bar for *every*
commit. Not "the tests I added" — the *whole suite*. An unsupervised system that commits on a partial run
will accumulate silent breakage. Green-whole-suite-or-revert is non-negotiable.
