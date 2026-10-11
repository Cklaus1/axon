# Meta API

Required semantic operations include equivalents of:

```text
meta.inspect(node)
meta.type_of(node)
meta.effects(node)
meta.capabilities(node)
meta.references(node)
meta.callers(function)
meta.callees(function)
meta.reads(node)
meta.writes(node)
meta.constant_evaluate(expr)
meta.generate(fragment)
meta.clone(node)
meta.replace(old, new)
meta.specialize(function, assumptions)
meta.register_transform(transform)
meta.emit_diagnostic(diagnostic)
meta.query_target(target)
meta.query_resource(resource)
meta.request_experiment(descriptor)
```

Mutation calls MUST be capability checked, provenance-bearing, and included in semantic diff.
