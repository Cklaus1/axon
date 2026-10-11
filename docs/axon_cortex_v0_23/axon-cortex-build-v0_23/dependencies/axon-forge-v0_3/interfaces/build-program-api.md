# Build Program API

```text
BuildProgram.execute(input, capability_request_ref, environment_ref) -> BuildGraph
```

A `BuildGraph` is deterministic data describing nodes/edges, declared inputs/outputs, tool identities, target constraints and content-addressed artifacts. BuildProgram execution requests external authority and cannot grant itself filesystem/network/process permissions.
