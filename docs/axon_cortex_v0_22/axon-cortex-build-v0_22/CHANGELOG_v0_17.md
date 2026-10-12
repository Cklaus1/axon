# v0.17 — ANEA owner integration and MiCode contract alignment

Baseline: reviewed Axon v0.16 and CX-36 r0.2. Added no CX spec; kept CX-35 reserved. All existing task/gate IDs are preserved; B174–B187 and 19 owner gate amendments are added. Existing dependencies are unchanged.

Added the Axon-owned ANEA execution profile, restricted host/vocabulary mappings, portable record projection, inert reference checks/fixtures, 86-requirement source crosswalk, explicit source-resolution ledger and AN0–AN5 release profiles. Updated owners, protocols, runtime/conformance/build guides, source/decision/status/index/task/gate/master views and hashes.

Reviewed CX-36 source, artifact formats, reference reader and inert .np bytes are unchanged. The older ANEA artifact encoding, packaging-after-evaluation order and combined compiler/runtime concept are explicitly superseded. Raw score origins and lifecycle authority retain lossless mappings and unknowns. All optional token/latent/canvas/world-model research remains off the core dependency path.

Companion MiCode v0.11 consumes the new owner contract while retaining its local PermissionGate, build loop, verifier and existing MX-33. No live product files or services were changed; all product gates remain NOT_RUN.
