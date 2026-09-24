# axon-loop-contracts schemas

These files are BYTE COPIES, not generated and not hand-edited:

| File | Source |
|---|---|
| `closed-loop-{policy,transition,context,episode}.schema.json` | Axon Cortex build-pack v0.22, `schemas/json/closed-loop-*.schema.json` |
| `acf-compute-request.schema.json` | v0.22 pack, `integration/compute-fabric-v0_1-reference/contracts/compute_request.schema.json` |
| `acf-execution-receipt.schema.json` | v0.22 pack, `integration/compute-fabric-v0_1-reference/contracts/execution_receipt.schema.json` |

The package schemas are normative; the Rust types follow them. Two tests keep
the two in step:

* `checked_in_schemas_are_the_package_bytes` pins each file's sha256, so a local
  edit is a deliberate, visible divergence rather than drift;
* `checked_in_schemas_agree_with_rust_serialization` walks each schema alongside
  the Rust serialization of the package fixture and requires every object to be
  closed and the serialized keys to equal the schema's `properties`.

Rules the Rust side enforces BEYOND these schemas (all from the package's
`CLOSED_LOOP_PROFILE_V022.md` / `tools/closed_loop_reference.py`): strict JSON
ingest (duplicate and escaped-alias keys, floats, |int| > 2^53−1, depth > 32,
input > 1 MiB), `next_epoch == expected_epoch + 1` on a transition, and the
cross-document checks in `src/checks.rs`.
