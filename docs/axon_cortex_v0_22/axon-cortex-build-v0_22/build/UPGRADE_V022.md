# Preservation-first upgrade to Axon 0.22

This procedure installs/reviews documentation and implements additive source deltas. It is **not** a command to overwrite the current repositories with the uploaded snapshots.

## Intake

Verify the delivered ZIP SHA-256 against the separately received receipt. Check its member inventory before extraction. Stage in a new directory; reject absolute/traversal names, symlinks and collisions. Keep the original 0.21/Fabric/source archives unchanged. Read actual target AGENTS files and capture `git status`/diffs, staged/untracked work, source revisions and current build/evidence reports for both repositories.

Run the supplied validator and tests in a trusted isolated Python environment. Then compare target source read-only:

```bash
python -B tools/plan_upgrade_v022.py --axon /path/to/axon --micode /path/to/micode
```

The planner hashes reviewed files and reports extra files without executing repository code. Exit 2 means drift/unsafe/missing input requiring reconciliation, not permission to revert the target to the archive. A matching archive inventory still does not establish a clean Git checkout or a successful build. Review dirty/untracked state separately.

## Documentation placement and version axes

Suggested new Axon documentation destination: `docs/axon_cortex_v0_22/axon-cortex-build-v0_22`. Preserve the older v0.15/v0.20/v0.21 directories until their consumers and CI references are migrated deliberately. Do not rename source CX-35 or old B/G/MX/ACF IDs. Do not bump either Cargo workspace to 0.22.

Copy/adapt the delivered MiCode consumer delta into its existing support program as a clearly labeled coordinated Axon-0.22 amendment. The supplied roadmap is v0.3; a separate v0.14/v0.15 external support pack was not supplied. Do not overwrite that unknown external history or invent a global support version. Keep native MX status vocabulary and update live use-site evidence only after implementation.

The bundled Fabric v0.1.0 reference stays byte-preserved. New integration requirements live outside it; a future Fabric version can be independently issued when those owners accept a revised pack. Current source and compatibility notes take precedence over obsolete intake assumptions, not over safety constraints.

## CI/package gate migration

The old source gate has v0.15 path/schema/CLI/count assumptions. Review its caller before replacing it. The new wrapper template is:

```bash
python -B /path/to/axon-cortex-build-v0_22/tools/repo_gate_v022.py --run-package-tests
```

This runs only authenticated package conformance/tests, not product gates. It accepts zero *errors/product gates executed* as valid counts and follows explicit checksum exclusions. Never refresh hashes merely to suppress unexplained tampering. Update CI source/runtime gates separately and require nonzero intended tests.

## Source implementation order

Use B255–B284 for the selected contracts/protected execution/peer path and B285 for joint runtime qualification. Preserve old accepted implementations by mapping fresh evidence to the required use sites. Do not wait for all CLM/Jev/learned-routing/specialization tasks, or import the historical B250 all-provider closure into this new pilot. B286 is a separately approved improvement study.

Keep feature activation off until current source/host/peer evidence qualifies the specific route. Run legacy and new focused tests, then actual interoperability and physical profile gates. Missing compiler, KVM, provider or peer prerequisites produce explicit BLOCKED/NOT_RUN. Do not replace them with a mock and assert production readiness.

## Rollback and data migration

Documentation rollback does not roll back execution state. Retain old readers and new sidecar readers while referenced data lives. No lossy re-encoding of existing event hashes; use named migration/projection receipts. Drain/cancel/reconcile active protected jobs and liabilities before disabling the new route. Rollback of a policy uses a current-authority fenced decision to a still-valid predecessor; otherwise pause. Never fall back from required microVM execution to an ordinary process.

No source files, policy pointer, trust-judge verdict or workload were changed by generating this package.
