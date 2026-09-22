# Release verification discipline

## The invariant

**A push may happen before verification only if it is explicitly labelled
unverified. A release or completion claim may not.**

Pushing unverified work is a normal thing to do and is sometimes the right
call — but the label travels with it. Saying "pushed" is a statement about
git. Saying "verified", "green", "release-ready" or "complete" is a statement
about evidence, and it requires the evidence to exist, to be complete, and to
be bound to the exact commit being claimed.

## What a release-quality claim requires

All of these, for the exact HEAD being claimed:

1. **Clean tree.** `git status --porcelain` empty.
2. **Receipts bound to that commit.** `scripts/run_managed.sh verify <dir>
   --for <commit>` exits 0. A receipt for another commit is refused as STALE.
3. **False greens = 0 OPEN** in `AXON-COMPLETENESS.json`.
4. **Completeness and claims gates green** (`scripts/completeness.py`,
   `scripts/claims_gate.sh`).
5. **Every security- and runtime-critical crate green**, per
   `governance/release-verification.json`. The strict gate runs these itself.
6. **Strict gate green**, run from an immutable snapshot of that commit:
   `scripts/run_managed.sh start <name> --snapshot <commit> -- ./scripts/gate.sh --strict`.

## What does NOT count as verification

- **A launcher's exit status.** `nohup … &`, a wrapper script, or a shell that
  returns 0 says nothing about the child. MEASURED: a `cargo test` launched
  this way had its child killed partway through the largest suite; the shell
  exited 0 and the partial tally (704 of 1542 tests) was read as a pass.
- **A partial run.** Every suite that STARTS must also REPORT. cargo prints
  `Running <binary>` on start and `test result:` on finish; a killed suite
  prints only the first. `write_receipt` records both counts and `verify`
  refuses when they differ.
- **A risk argument.** "Only comments and formatting changed, so the tests
  cannot have broken" may well be true, and it is still not evidence. It is a
  reason to expect a green result, not a green result. Run it.
- **A green gate, for a crate the gate does not run.** The strict gate once
  COMPILED 20 of 22 crates while RUNNING the tests of 5. Compiling a test is
  not running it. `governance/release-verification.json` now states which
  crates must actually run, and `release_manifest.rs` fails the build if a
  workspace member is missing from it.
- **`pgrep` returning nothing.** A process disappearing is not a process
  succeeding.

## How to run something release-significant

Use `scripts/run_managed.sh`. Do not hand-roll another wrapper; if the
canonical mechanism cannot express a requirement, extend it there so every
caller gets the fix.

    D=$(scripts/run_managed.sh start <name> --snapshot HEAD -- <command>)
    scripts/run_managed.sh status  "$D"
    scripts/run_managed.sh verify  "$D" --for HEAD

`--snapshot` runs the job in a detached worktree at one commit, so edits to
the developer tree while it runs cannot contaminate the result. That is not
hypothetical: a strict gate was once launched on a clean tree, source files
were edited ~2.5 minutes in while cargo was still compiling those crates, and
the resulting PASS was attributable to neither state. It was discarded.

## Receipts

`write_receipt` records, per run: the command, HEAD, tree state and a digest
of the working-tree diff, source mode, toolchain, the CHILD's exit status,
start and finish times, a digest of the log, suites started, suites reported,
and tests passed/failed.

`verify` refuses a receipt that is stale (bound to a different commit than the
one being claimed), incomplete (suites started ≠ suites reported), failing
(non-zero child exit or any failed test), or whose log has changed since the
receipt was written.
