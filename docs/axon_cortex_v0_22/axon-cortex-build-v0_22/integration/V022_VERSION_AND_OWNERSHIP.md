# Version and ownership reconciliation

The umbrella build-pack version is **0.22**. It does not redefine the independent source versions or wire protocols.

| Axis | Evidence / action |
|---|---|
| Axon source | Uploaded byte inventory; reported source label `4cceb89` not Git-verified. Preserve current Cargo version and source CX-35. |
| MiCode source | Uploaded byte inventory; filename label `8ffc2504` not Git-verified. Cargo workspace is `0.2.0` in supplied source; do not change for this documentation release. |
| MiCode embedded support | v0.3 / Intended in supplied docs. Apply the 0.22 consumer delta through existing MX owners; do not imply external v0.15 delivery. |
| Cortex research provenance | Original 0.21 research files and source pins retained, no new independent paper/model reproduction claimed. |
| Fabric document | Original v0.1.0 byte-preserved; M0–M2 mapped, M3/M4 deferred. |
| Proposed new sidecar | `axon.closed-loop/1` family, requires explicit negotiation and real bilateral implementation before use. |
| Existing ACE/Reflex/neural records | No silent schema/digest changes. Historical owner rows/IDs preserved. |

The earlier 0.21 `SOURCE_OWNER_MAP.json` and `V021_INPUT_LOCK.json` are retained historical input records. Their statement that no MiCode source was supplied applies to that prior build only. The current source truth is `V022_INPUT_LOCK.json` and `V022_OWNER_MAP.json`. No code infers current readiness from the historical peer version string.

New source claims need a path/line hash and live use-site evidence. New recommendations are not backdated as source findings. The archival prior assessment is context, not a runtime qualification artifact.
