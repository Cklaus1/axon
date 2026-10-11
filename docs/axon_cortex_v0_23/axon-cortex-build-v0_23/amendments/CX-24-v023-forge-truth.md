# CX-24 v0.23 Forge integration amendment — coding truth resolution

For coding/compiler semantics, semantic retrieval follows Forge truth precedence: current compiler probes/source/conformance/examples/versioned specs before external docs and model priors. Conflicts are surfaced with provenance instead of resolved by confidence alone.

## v0.23 acceptance gates

**G24-r23-truth-order:** When local current compiler/source/conformance evidence conflicts with model prior or stale documentation, the resolver surfaces the conflict and ranks the current local evidence higher for that toolchain revision.

**G24-r23-executable-retrieval:** Retrieved coding examples identify source/revision and, when claimed executable, carry compiler/conformance evidence or remain explicitly unverified.

