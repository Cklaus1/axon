# Corpus / Source-of-Truth Resolution

Forge SHOULD make current executable/local evidence authoritative for coding agents.

Default priority: compiler behavior -> repo source -> installed modules/stdlib -> conformance tests -> canonical examples -> local spec -> version-matched docs -> external docs -> model prior.

A KnowledgeResult SHOULD record query, selected source, source hash/version/location, verification method, conflicts, and confidence.

Forge MAY synthesize minimal compiler-probe programs to experimentally establish semantics when safe and cheap.
