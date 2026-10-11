# Preservation-first upgrade to Cortex 0.23

1. Authenticate the v0.22 parent and Forge v0.3 hashes in the manifest/locks.
2. Validate both embedded ZIPs independently before running any of their tools.
3. Apply v0.23 as an additive documentation/integration profile; do not overwrite completed 0.22 implementation blindly.
4. Reconcile each amendment with the current live repository before code changes; preserve dirty/untracked work.
5. Implement B287-B301 before any operational pilot. B302 requires the existing controlled-trial/evaluator infrastructure.
6. Keep Forge and Cortex source/version histories independently addressable.
7. Do not mark runtime qualified or improvement supported from package conformance alone.
