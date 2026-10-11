# Implementation-agent bootstrap — Cortex v0.23 / Forge v0.3

Implement the v0.23 Forge integration as an additive change to the current Axon/Cortex/MiCode repositories.

Read in order: `README.md`, `integration/V023_VERSION_AND_OWNERSHIP.md`, `integration/V023_OWNER_MAP.json`, `integration/V023_FORGE_CROSSWALK.json`, `build/FORGE_INTEGRATION_V023.md`, `build/FORGE_RUNTIME_SEAMS_V023.md`, `build/WORK_PACKAGES_V023.md`, then the relevant `amendments/` and standalone Forge package.

Do not create a second experiment DB, evidence graph, capability registry, admission pointer or resource inventory. Forge owns program semantics/compiler programmability only. Preserve current user work, run targeted tests, record exact evidence, and stop on an ownership conflict rather than inventing a new authority.
