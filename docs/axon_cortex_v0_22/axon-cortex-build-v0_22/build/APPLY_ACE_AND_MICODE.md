# Apply Axon v0.18 and synchronize MiCode v0.12

## 1. Apply the owner documents first

Use `Axon_Cortex_Build_Files_v0_16` as the actual baseline of this patch. Import v0.18 into the existing managed Axon build/spec owner, not a new runtime folder. Preserve unrelated source, user changes, historical source locks and valid live implementation evidence. This package's NOT_RUN means **this document review ran no product gates**; do not overwrite genuine live receipts. Old PASS evidence that no longer binds changed inputs/contracts must be invalidated or re-evaluated, not copied blindly.

Review [source precedence](../integration/ACE_DECISIONS.json). CX-36 r0.2, `.nps`/`.np` schemas and its reference reader are unchanged. Do not import the older ACE reference format or duplicate CX-23/CX-34/CX-11 ownership. Allocate conflicting task/gate IDs through a recorded mapping. Regenerate local index/tasks/gates/master with the repository's supported tools.

## 2. Apply the MiCode consumer update

MiCode v0.12 is a focused alignment of v0.10, not a fresh support architecture. It keeps MX-33 and the existing bridge. Its `AXON_CONTRACT_LOCK.json` pins the new owner profile, mapping, projection schema and unchanged Neural Program formats. The prior lock and legacy synthetic projections remain as history.

Locate the actual managed `minotes`/Axon-support directory instead of assuming its path. Preserve unrelated notes and live evidence. Bind the installed `/build-loop` identity. Implement T188–T191 only after resolving the appropriate upstream contract mappings; a pinned document does not establish a running peer.

## 3. Implement in parallel after contract reconciliation

Axon AN0/AN1 and MiCode A0/A1 can progress without all future models, while remaining subject to their foundational permission/evidence tasks. Then run Axon AN2/MiCode A4 jointly on a recorded pair of repository revisions and exact owner contract hashes. Review admission, egress, credentials, data ownership and capability limits independently on each side.

Acceptance sequence: real request → real producer → owner-validated result → MiCode local permission (if action follows) → actual test/check evidence → protected completion decision. Test failure/cancellation and next-turn recovery. No separate public Jev service is required; use one existing authorized backend through the existing Axon seam.

## 4. Validation commands for these files

Axon: `python -B -m unittest discover -s tests -v`; `python -B tools/package_views.py`; `python -B tools/validate_package.py`.

MiCode: `python -B -m unittest discover -s tests -v`; `python -B tools/package_views.py --check`; `python -B tools/validate_package.py`.

The release check records the tested package digests. These checks do not call APIs, run Axon/MiCode, load models, establish signer trust or certify a sandbox. Dependency setup follows each package's requirements file. Do not treat document PASS as implementation acceptance.
