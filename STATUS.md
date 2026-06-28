# Axon — Status

**Last updated**: 2026-06-28
**Branch**: `main`
**Version**: `axon 0.1.0`

This is the single current-status document. For **forward planning** see `ROADMAP.md`;
for the **authoritative phase/feature reference** see the Phase Status table in `CLAUDE.md`.
Superseded build-diagnosis and one-off goal docs are archived under
`.archive/superseded-2026-06/`.

---

## What Axon Is

AI-optimized, statically-typed systems language. Compiles to native via LLVM 17
(`inkwell`), with a codegen-free tree-walking interpreter as the primary execution
path. Thesis: **safety as language features** — capabilities, effects, refinement
types, and corrigibility are first-class, not libraries. The interpreter is the
reference oracle; native codegen must agree with it byte-for-byte (invariant I-2,
enforced by the `scripts/*_parity.sh` harnesses).

Pipeline: `Lexer → Parser → Resolver → Infer (HM) → Checker → {Interp | Codegen→LLVM→binary}`

## Build Health

| Build | Status | Time |
|-------|--------|------|
| Interpreter CLI (`cargo build -p axon-core --no-default-features --bin axon`) | ✅ green | ~10s |
| Native codegen (`cargo build -p axon-core`, default features) | ✅ green | ~3s incremental |

The historical "build never finishes" stall is resolved — it was a `serde-json` ×
`codegen` default-feature collision (`.archive/superseded-2026-06/BUILD_DIAGNOSIS*.md`,
`BUILD_RESOLVED.md`). **Do not enable `codegen` + `serde-json` together** until the AST
serde derives are decoupled.

## Scale

| Metric | Value |
|--------|-------|
| Workspace crates | 19 (`axon-core` is ~105K LOC; ~150K total) |
| `.ax` examples | 158 |
| `@[test]` functions in examples | 447 |
| Rust `#[test]` files | 117 |
| Parity / acceptance gate scripts | 86 (`scripts/*.sh`); 49 are `*_parity.sh` |
| Spec docs | 14 (`spec/`) |

## Phase / Roadmap Completion

Phases 1–14+ and the R-series (through R31, with R32–R34 specced) are tracked as
complete in the `CLAUDE.md` Phase Status table — 19 phase rows. Highlights:
refinement types + SMT discharge (Phase 5), row-polymorphic effects + suspend/resume
runtime across native/wasi/browser (Phase 6), kernel services (Phase 7), the Layer-3
self-improving compiler, the Phase-10 prose→AST surface + Hello-Goal CLI flow, the
web approval UI (Phase 12), and the Tier-2 distributed/probabilistic/simulation stdlib
(Phase 14+). The latest landed work is the ASI safety stack (R26–R31): confidential
microVM substrate + attestation, corrigibility kill-switch, audit ledger, and extended
TCB attestation.

> **Caveat (verify claims against gates).** Status docs in this repo have historically
> lagged code in both directions. Treat the gate suite — not prose — as the source of
> "done". See Verification below.

## Verification

Run the gate suite to validate claims:

```bash
scripts/parity_all.sh         # all *_parity.sh — interp vs native byte-parity (I-2)
scripts/acceptance_gate.sh    # axon-os R21 §10 acceptance (presence + anti-stub + journey)
scripts/gate.sh --strict      # the full strict gate
```

**Last verified run (2026-06-28):**

- `parity_all.sh` (PARITY_SKIP_WASM=1): **33 passed, 15 skipped, 1 failed** of 49.
  The 15 skips are all `wasm_*` (toolchain skipped); the interp↔native byte-parity
  invariant (I-2) holds across all 33 native harnesses.
- `acceptance_gate.sh`: **OK** — every R21 §0 check present, unstubbed, and green
  (88 axon-os tests pass; same-job+seed record is byte-identical).

**Known gap (the one parity FAIL):** `all_examples_parity` — 4 examples build-fail
under native codegen: `http_get`, `http_sse`, `anthropic_stream`, `trainloop_stream`.
All four use the `http_get`/`http_sse` network builtins, which are **interpreter-only**
(not yet lowered to codegen); 35/39 other examples match byte-for-byte. The gap is
codegen network-builtin coverage, **not** interpreter correctness or a soundness
divergence.

## Repo Hygiene Notes

- Build artifacts (`agent_task`, `*.rlib`, `dist/`) are gitignored, not committed.
- Merged feature/worktree branches are pruned periodically; live work lives in
  `.claude/worktrees/` worktrees (gitignored).
- One corrupt zero-byte ref (`refs/heads/worktree-agent-a1a0476b66333a42f`) needs a
  manual `rm .git/refs/heads/...` — git plumbing can't repair an unresolvable ref and
  the sandbox blocks writes into `.git/`.
