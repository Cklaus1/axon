# Programmable Compiler / Meta-Execution Runtime

Forge generalizes Jai-like `#run` semantics without requiring Jai syntax.

Conceptual entry point:

```text
forge.meta.execute(meta_program, sir, phase, capabilities, limits) -> MetaResult
```

A meta-program SHOULD be able to inspect nodes/types/effects/references/callers/callees/reads/writes/resources, constant-evaluate expressions, generate data/code/SIR, clone/replace/specialize nodes, register transforms, emit diagnostics, query targets/resources, generate bindings, construct build inputs, invoke declared tests, and request Axon experiments.

## Capabilities

Default is deny. Capabilities may include filesystem read/write scopes, process spawn, network domains, environment keys, secret ids, compiler mutation scopes, build-graph mutation scopes, and test suites.

## Deterministic mode

When deterministic mode is requested, randomness MUST use a declared seed, wall-clock access MUST be virtualized or denied, undeclared environment/network access MUST be denied, and observed inputs MUST be captured in provenance.

## Resource limits

Meta execution MUST support wall time and memory limits and SHOULD support CPU, output bytes, process count, and tool-call limits.

## MetaResult

MUST include status, generated artifacts, semantic mutations/patches, diagnostics, observed inputs, consumed capabilities, resource usage, and provenance.
