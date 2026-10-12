# Non-destructive source upgrade to the v0.21 build pack

## What this delivery does and does not change

This ZIP contains proposed source-owner amendments, build tasks, test plans and executable model-free reference checks. It does not overwrite the user's repository. The supplied source snapshot is pinned by archive and per-file SHA256 because no source Git commit was established from the archive. A newer working tree is not assumed identical to that snapshot.

Retain source evidence, uncommitted work, old vendored documentation and historical gate receipts. No `git reset --hard`, `git clean`, automatic stash, bulk source extraction, governance reset or workspace-version bump belongs in the upgrade. A dirty tree is a condition to review, not permission to discard changes.

## Intake

Authenticate the archive through the normal download/source channel, inspect members for traversal/absolute paths/symlinks, and extract to a **new directory outside the working tree**. Do not unzip the pack over `crates`, `governance` or the old `docs/axon_cortex_v0_15` folder. The new package root is `axon-cortex-build-v0_21`.

Read [SOURCE_REVIEW_V021](../review/SOURCE_REVIEW_V021.md) and the [input lock](../integration/V021_INPUT_LOCK.json). Run the pack validator and model-free tests before copying anything. In a trusted environment:

```bash
python -B tools/validate_package.py
python -B -m unittest discover -s tests -v
python -B tools/plan_upgrade.py --repo /path/to/current/axon
```

The planner prints a current-versus-reviewed inventory and compatibility findings. It does not run source scripts or mutate the tree. Review changed and missing files before relying on old line-level findings. Treat source symlinks and unknown paths as review items, never follow them outside the root. Actual hash drift is not fixed by resealing this package.

## Reconcile source authority before implementation

Record the current Git branch/commit/status, source inventory and test baseline in the repository's normal evidence location. Retain existing reports and map historical task completion to actual files and executed tests. Package `Not started` and `NOT_RUN` state does not reset live source progress. Conversely, a document claiming a feature does not prove code implements it.

Keep the live source's CX-35 Reflex naming. Link it to package CX-05 and the related owners through `SOURCE_OWNER_MAP.json`; do not rename code to make the package's reserved ID appear unused. Keep `axon-reflex/1`, Cargo workspace `0.1.0`, CX-36 r0.2 and ACE v1 contracts unless a separately reviewed actual ABI change requires new versions.

## The package-gate script needs more than a path edit

The reviewed `scripts/cortex_package_gate.sh` points at v0.15 and expects a different checksum schema, `--report`, all-positive count values and a hashed validation receipt. v0.21 uses `axon-package-sha256/1`, validator `--output`, legitimate zero `orphan_gates`, and excludes the receipt plus checksum file to avoid a hash cycle.

Use the included `tools/repo_gate_v021.py` as a **new trusted wrapper template** rather than overwriting the old script blindly. It validates the new pack and optionally invokes explicitly requested pack unit tests; it never interprets them as product gate evidence. It writes only to stdout. The repository owner must review how CI invokes it, retains output and separately validates source-executed gate evidence. The old governance registry must not be populated with green results from package fixtures.

After approval, a reasonable documentation-only staging destination is `docs/axon_cortex_v0_21/axon-cortex-build-v0_21/`. Keep the old vendored pack untouched. Update source gate invocation and governance aliases as a small reviewable patch, not a global version substitution. Reconcile the existing invocation's path constraints and complete evidence contract before making it the default.

## Build order and exit conditions

Start B223/B224/B225/B249: intake, source gate compatibility, common negotiated decision contract and package conformance. Next implement deterministic evidence/checklist, eligibility, context, budget and speculative effects barriers using existing source seams. Add CLM/Jev and learned-router adapters in shadow after those boundaries. The source's deterministic Reflex backend remains a test/control; do not claim real inference from `decided(question)`.

For real code changes, rerun `cargo test -p axon-reflex`, `cargo test -p axon-cortex`, targeted policy/AI/kernel/replay tests and repository CI under the actual toolchain. Commands must be adjusted to the live workspace if it differs. These Rust commands were **not run in this review** because the execution environment lacks Cargo/rustc.

Only then run a separately authorized real provider pilot with source/model/price/environment pins, complete costs and independent final checks. MiCode interoperability waits for its actual source/pack. B251–B254 remain optional experiments, never prerequisites for a safe deterministic upgrade.

## Rollback and release maintenance

Rollback a research flag/registry epoch to a qualified incumbent without deleting the new evidence or old candidate artifacts. Revoke new dispatch where needed and reconcile in-flight effects/spend. Reverting documentation does not undo a committed source change or external effect; use the repository's normal reviewed revert and recovery procedures.

For intentional pack edits, update source manifests first, then generated views and owner-export hashes. Run tests, record their exact scope, regenerate the master if reports changed, explicitly refresh the full checksum inventory and validate it again. Never edit protected contract bytes as an incidental reseal. The original source and v0.20 archives remain immutable review inputs.
