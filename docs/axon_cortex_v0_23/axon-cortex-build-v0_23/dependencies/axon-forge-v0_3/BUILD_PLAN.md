# Forge v0.3 Build Plan

## P0 contracts
1. canonical SIR + semantic diff;
2. Transform/TransformResult;
3. external capability request + meta execution;
4. structured diagnostics;
5. Forge experiment/evidence projections into Axon;
6. provenance/resource projections;
7. conformance validator.

## P1 reference language/toolchain path
Use an existing language/compiler. Build one frontend -> SIR adapter, one backend adapter, structured diagnostics, compile/repair loop, and bootstrap/truth resolver.

## P2 transformations and migrations
Add transform registry, semantic diff verification, AI-generated transform sandbox, migration dry-run/idempotency and BuildProgram -> BuildGraph.

## P3 strategy evolution
Expose compiler pipeline/build/repair/retrieval strategies as profiles of canonical Axon StrategyArtifacts. Use CX-33 experiments and CX-32 evidence; no Forge-local promotion authority.

## P4 advanced compiler work
Only after the above: target-specialization, alternative IRs/backends, hardware-aware placement, self-hosting or a new source language.
