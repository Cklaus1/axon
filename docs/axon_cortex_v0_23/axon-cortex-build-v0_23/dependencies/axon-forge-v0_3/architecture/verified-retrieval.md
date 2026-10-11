# Verified Executable-Example Retrieval

`forge.knowledge.resolve(query, context)` SHOULD prefer current stdlib/module source, passing repository examples, and conformance tests over prose-only retrieval.

A retrieved code example SHOULD carry source/version plus whether it currently compiles, compiler/backend identity, tests passed, and verification hash/date. Forge MAY compile minimal snippets on demand.
