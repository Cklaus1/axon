# Truth Resolver API

```text
truth.resolve(question, scope, bootstrap_pack_ref?) -> TruthResolution
```

`TruthResolution` returns ordered evidence items with source class, revision/hash, compiler probe when used, confidence, conflicts and executable examples when available. Resolution ordering follows the normative local-truth precedence in `SPEC.md`.
