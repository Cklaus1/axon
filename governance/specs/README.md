# Tech Specs — Index

Specs for the unbuilt high-risk requirements (`../REQUIREMENTS.md`). Each is **spec-first** because it has
an architectural fork that is cheap on paper and expensive once merged. Until a spec here reaches
**Reviewed**, do not write implementation code for that requirement (`../BUILD_PROTOCOL.md` Gate 1).

Each stub below states the **decisive fork** that the full spec must resolve first — that's the question
that gates everything else. Fill against `../SPEC_TEMPLATE.md`.

| Spec | Requirement | Status | The decisive fork |
|---|---|---|---|
| `R3-ai-primitive.md` | R3 — AI as primitive | 📝 Draft | **Reproducibility vs. capability.** *Resolved in the draft:* YES — replay requires it, so every AI call pins `(model, version, params_hash, prompt_hash, mode, cost)` into an `event:"ai_call"` provenance record (schema settled first, §4.3); routing (tier→model) and `@[ai(policy)]` layer on top. Live determinism is not faked — delivered via mock + hash-keyed replay (R9). Cost-optimal routing waits on Phase-7 `Budget` (§12 Q2). |
| `R6-capability-security.md` | R6 — capability/registry security | 📝 Draft | **Content-addressing model.** *Resolved in the draft:* `axh1:`-tagged SHA-256 over raw source bytes (not AST); committed TOML `axon.lock` (name→hash→bytes, `--locked` CI mode); AI-audit runs at **acquisition** (`axon add`/`lock`), verdict pinned in the lockfile, re-validated by hash on compile (no AI call at build). Static capability checker (E1001–E1004) stays the hard I-11 gate; the audit is defense-in-depth, not the sole boundary. New E12xx band; import-edge cap-widening = E1203. |
| `R7-targets.md` | R7 — cross-platform targets | 📝 Draft | **Does wasm reuse the LLVM backend or need a separate codegen?** *Resolved: the fork was a false binary.* A **third option** unblocks R7 now — compile the pure-Rust tree-walking **interpreter** (`interp.rs`, no LLVM) to `wasm32`, running `.ax` in-browser with **identical results by construction** (I-2). Real work = abstracting the ~6 host touchpoints (fs/env/thread/sleep) behind an `AxonHost` trait. AOT-wasm via LLVM (option B) stays **R1-blocked** (§12 Q1); a lean wasm backend (C) is last-resort. New E0907/E0908/W0910; Slice A is deliverable, Slice B gated. |
| `R10-self-improving-compiler.md` | R10 — self-improving compiler | 📝 Draft | **Verification of a graduated pattern.** *Resolved:* a four-gate harness — G1 correctness (the **interpreter is the equivalence oracle**, I-2: `interp(P(c))==interp(c)` over the whole content-addressed corpus, never an AI judgment), G2 capability-diff (I-12, E1402), G3 full-suite regression, G4 perf. **Crucial split:** G1–G3 are verifiable NOW (interpreter-side, R1-independent); only G4 (measurably faster) is R1-gated, so a pass may prove correct+safe but never claim `faster` until R1. Graduation needs multi-sig of root Principals (the compiler can't graduate its own passes) + boot attestation. New E14xx band; proposes I-17. |

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
