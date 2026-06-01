# Tech Specs — Index

Specs for the unbuilt high-risk requirements (`../REQUIREMENTS.md`). Each is **spec-first** because it has
an architectural fork that is cheap on paper and expensive once merged. Until a spec here reaches
**Reviewed**, do not write implementation code for that requirement (`../BUILD_PROTOCOL.md` Gate 1).

Each stub below states the **decisive fork** that the full spec must resolve first — that's the question
that gates everything else. Fill against `../SPEC_TEMPLATE.md`.

| Spec | Requirement | Status | The decisive fork |
|---|---|---|---|
| `R3-ai-primitive.md` | R3 — AI as primitive | 📝 Draft | **Reproducibility vs. capability.** *Resolved in the draft:* YES — replay requires it, so every AI call pins `(model, version, params_hash, prompt_hash, mode, cost)` into an `event:"ai_call"` provenance record (schema settled first, §4.3); routing (tier→model) and `@[ai(policy)]` layer on top. Live determinism is not faked — delivered via mock + hash-keyed replay (R9). Cost-optimal routing waits on Phase-7 `Budget` (§12 Q2). |
| `R6-capability-security.md` | R6 — capability/registry security | 🔲 Stub | **Content-addressing model.** Imports are content-addressed (hash = identity, no names). Decide the hash scheme, the lockfile format, and *when* the AI-audit-on-import runs (install-time vs. compile-time). This is a security boundary — under-spec = exfiltration. Pairs with `ARCHITECTURE_INVARIANTS.md` I-11/I-12. |
| `R7-targets.md` | R7 — cross-platform targets | 🔲 Stub | **Does wasm reuse the LLVM backend or need a separate codegen?** The PRD diagram implies LLVM→wasm is "free" but JS is a separate backend. Decide: (a) native+wasm share the inkwell path (blocked on R1's stalled build), (b) wasm gets its own lean backend. This decision blocks all of R7. Also: the native build must be unstalled (R1, `BUILD_DIAGNOSIS.md`) before *any* multi-target work is real. |
| `R10-self-improving-compiler.md` | R10 — self-improving compiler | 🔲 Stub | **Verification of a graduated pattern.** Before an AI-discovered optimization can be added as a compiler pass, how is it *proven* correct + faster on the full corpus, not just the discovering program? Decide the verification harness (equivalence checking + the benchmark corpus + the regression gate) *first* — an unverified self-modifying compiler is the single highest-risk component in the whole PRD. Hard dependency on R1 (native) + I-12 (self-mod can't weaken TCB). |

---

## Build order (from `../REQUIREMENTS.md` work queue)

These four are **not** the next things to build. The cheap-and-compounding work (R8 `forall`, finishing
R5 `#[goal]`, R9 `#[corrigible]`, and the confirmed bug-hunt fixes — overflow/seed/analytics) comes first.
These specs exist now so that *when* their requirement reaches the front of the queue, the architectural
fork is already resolved on paper rather than discovered mid-implementation.

**Recommended sequencing:**
1. Land the confirmed bug-hunt fixes (`../BUG_HUNT_2026-05-31.md`: #6, #19, #11, #4, #5) — they protect the
   success signal everything else depends on.
2. R8 `forall` → unblocks deeper property testing of all future work.
3. R5 `#[goal]` first-class + R9 `#[corrigible]` → finish the autonomy + safety surfaces that are closest.
4. **R3 spec → build** (the differentiation).
5. **R6 spec → build** (security gate for autonomous dep use).
6. **R1 native build** (focused effort, not a tick) → unblocks **R7** and **R10**.
7. **R7 spec → build**, then **R10 spec → build** (the recursive-improvement flywheel, last and most carefully).
