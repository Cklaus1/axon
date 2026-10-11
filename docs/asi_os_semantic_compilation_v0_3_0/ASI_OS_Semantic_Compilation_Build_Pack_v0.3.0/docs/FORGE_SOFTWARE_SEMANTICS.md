# Forge Software-Semantic Integration

Semantic Compilation and Forge solve different layers.

- **IntentIR** says what outcome is desired.
- **AIR / Semantic Plan** says what cognitive/execution work is proposed.
- **Forge SIR** says what a software program means.

For software/compiler tasks, the compiler may bridge AIR work into Forge SIR. That bridge is explicit, version-pinned, and evidence-bearing. The binding records both the originating IntentIR reference and the AIR reference so a compiler cannot silently jump directly from desired outcome to program semantics and lose execution-semantic lineage. This package does not copy Forge SIR, Transform, Migration, SemanticDiff, BuildProgram, Backend, diagnostic, provenance, or truth-resolution schemas.

## Preferred software path

```text
approved IntentIR
→ AIR candidate
→ source/frontend
→ Forge SIR
→ Transform/Migration
→ SemanticDiff
→ BuildProgram/backend
→ compile/test
→ CX-13 execution receipt
→ CX-32 evidence
→ CX-21/CX-33 comparison
→ CX-11/CX-18 admission/promotion
```

Text patches remain a fallback where semantic support is absent. When Forge support exists, semantic changes should preserve preconditions, invariants, semantic diff, verification requirements, exact artifacts, toolchain identity and rollback metadata.

## Truth order

For coding semantics, use the Forge/Cortex precedence: current compiler probes, repository source, installed libraries/modules, conformance tests, canonical examples, local versioned specs, version-matched docs, external docs, then model priors. Conflicts stay explicit and provenance-bearing.
