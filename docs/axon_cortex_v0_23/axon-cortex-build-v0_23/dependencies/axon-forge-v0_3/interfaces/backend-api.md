# Backend API

```text
Backend {
  identity()
  capabilities()
  validate(SIR, resource_graph)
  estimate(SIR, resource_graph)
  lower(SIR, options)
  compile(lowered, build_context)
  execute(artifact, inputs, runtime_context)
  collect_metrics(execution)
}
```

A backend MUST declare supported SIR features, toolchain identity, deterministic-build guarantees, diagnostic capabilities, and resource requirements.
