# Structured Diagnostics

Compiler/verifier feedback SHOULD be normalized into a typed API.

A Diagnostic includes severity, stable code, message, SIR node id when available, source span, expected/actual semantic state, causal chain, notes, repair categories, compiler identity, and provenance.

Preferred repair loop:

`agent.generate -> forge.compile -> Diagnostic[] -> semantic context query -> agent.repair -> forge.compile`

MiCode SHOULD group cascading errors by root cause where possible.
