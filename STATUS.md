# Axon — Status

**Last updated**: 2026-09-17
**Branch**: `diag-hint-and-location`
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

Measured 2026-09-17, not carried forward. The commands are below each figure's
row in `ARCHITECTURE.md` §8 — re-measure rather than trusting this table, which is
exactly the kind that drifts.

| Metric | Value |
|--------|-------|
| Workspace crates | 19 (`axon-core` ~99.7K LOC; ~146.8K across `crates/*/src`) |
| `.ax` examples | 174 |
| `@[test]` functions in examples | 462 |
| Rust files containing `#[test]` | 131 |
| Gate / harness scripts | 110 (`scripts/*.sh`); 54 are `*parity*.sh` |
| Spec docs | 14 (`spec/`) + 66 governance specs (`governance/specs/R*.md`) |

For the **exhaustive, generated** surface — every CLI verb, builtin, attribute,
diagnostic code and environment variable — see `AXON_REFERENCE.md`. It is produced
by `axon reference` from the compiler's own tables and gated in both directions,
so unlike this table it cannot go stale silently.

## Phase / Roadmap Completion

Phases 1–14+ and the R-series (through R34) are tracked as complete in the
`CLAUDE.md` Phase Status table — 19 phase rows. Highlights:
refinement types + SMT discharge (Phase 5), row-polymorphic effects + suspend/resume
runtime across native/wasi/browser (Phase 6), kernel services (Phase 7), the Layer-3
self-improving compiler, the Phase-10 prose→AST surface + Hello-Goal CLI flow, the
web approval UI (Phase 12), and the Tier-2 distributed/probabilistic/simulation stdlib
(Phase 14+). The latest landed work is the ASI safety stack (R26–R31): confidential
microVM substrate + attestation, corrigibility kill-switch, audit ledger, and extended
TCB attestation.

**Landed since (2026-09-16/17):**

- **R44 — the accumulating typed session** (`axon session`, all six slices). Bind a
  name in one cell, read it in the next; every cell re-type-checks the WHOLE
  accumulated program, so redefining a name in a way that breaks an earlier
  binding is a compile error before anything executes (**E2400**). `--protocol
  jsonl` for a host driver, `--transcript` + `AXON_RECORD` for a replayable pair.
- **R45 — `--require-contained`** on `check` / `run` / `session`: containment
  becomes a GRANT rather than an opt-in (**E1005**). Applies to `main` and
  everything it transitively reaches. Known limit, stated in `--help`: functions
  dispatched by name at runtime are not covered.
- **A generated, gated reference.** `axon reference` emits the whole surface from
  the compiler's own tables; `ALL_CODES` and `ALL_ENV_VARS` make diagnostic codes
  and environment variables introspectable for the first time. A new verb,
  builtin, code or variable fails the test suite until it is documented.
- **`ARCHITECTURE.md`** (how it is put together and why) and **`COMPARISON.md`**
  (against Rust/Go/C/C++/Python/JS-TS, and against seccomp/gVisor/Firecracker/WASM).
- Ten inspection-surface defects, all of one class: an absent or unknown fact
  reported as a benign one. The worst was `axon redteam` reporting "safe" on a
  file `axon deploy` blocks — the two verbs read the same `redteam_check` through
  different mechanisms.

With R44 and R45, all five of `AXON_FOR_RLM.md`'s recommendations are landed.

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

**Last verified run (2026-09-17):**

- `gate.sh --strict`: **PASSED.** Note it had been RED since 2026-09-08 — a
  `too_many_arguments` clippy error in `axon-rt` that CI does not catch, because
  CI runs `cargo check/test/fmt/clippy` and a parity job, not `gate.sh`. Running
  the narrower per-crate clippy reports "clean" without reaching it.
- `verify_all_specs.sh --run all`: **CLEAN** — every non-Draft spec's `evidence:`
  command was actually re-run and passed, not merely present.
- `parity_all.sh`: **51 passed / 2 skipped / 0 failed** of 53. The 2 skips are
  `android_compute_parity` (NDK absent) and `browser_compute_parity` (needs
  headless Chrome + chromedriver), both reporting "(toolchain absent)" — the
  documented expected set. A first reading of 50/3 was a FLAKE from concurrent
  cargo builds competing for `target/debug/axon`; the clean re-run is 51/2.
  Re-run parity on an otherwise idle box, and read the named SKIP lines rather
  than the count.
- `acceptance_gate.sh`: **OK** — every R21 §0 check present, unstubbed and green.

> **CI does not run the parity suite.** The job was named "Codegen +
> native/interp parity" and runs `cargo build` + `cargo test` only — it has never
> invoked `parity_all.sh`. Invariant I-2 is therefore enforced by someone
> remembering to run it locally, not by the pipeline. The name is corrected and
> `.github/workflows/ci.yml` records what closing the gap costs
> (`PARITY_SKIP_WASM=1 EXPECT_MIN_PASS=38`, ~15 min/run; 38 is measured, and the
> default floor of 40 rejects that run by design). Run `scripts/parity_all.sh`
> before trusting a green CI on anything touching codegen.
- `cargo test -p axon-core`: 661 lib + 551 `cli_run`, 0 failed (default features);
  653 + 540 + 132 + 24 + 8, 0 failed under `--no-default-features` (the config CI
  runs).

**Earlier verified run (2026-06-28):**

- `parity_all.sh`: **0 failed** of 49. With the wasm rust targets registered and a
  codegen `axon` binary present, **47 pass / 2 skip** — all 13 `wasm_*` harnesses
  pass (verified). The 2 genuine skips are non-wasm toolchain tiers:
  `android_compute_parity` (Android NDK absent) and `browser_compute_parity`
  (opt-in, needs headless Chrome + chromedriver). The interp↔native/AOT-wasm
  byte-parity invariant (I-2) holds across every runnable harness.
  - _Caveat:_ run the suite with a **codegen** `axon` in `target/debug` (`cargo
    build -p axon-core --bin axon`). If a `--no-default-features` (codegen-less)
    binary is left there, the 3 codegen-dependent browser harnesses
    (`wasm_browser_io/parity`, `wasm_examples`) skip cleanly as "toolchain absent"
    rather than run — a harness binary-detection quirk, not a divergence.
- `acceptance_gate.sh`: **OK** — every R21 §0 check present, unstubbed, and green
  (88 axon-os tests pass; same-job+seed record is byte-identical).

**Interp-only-by-design (sound E0910 refusals, not failures):** 4 examples
(`http_get`, `http_sse`, `anthropic_stream`, `trainloop_stream`) use the network
`http_*` builtins, which native codegen **soundly refuses** (E0910 — runs under
`axon run` instead of miscompiling). `all_examples_parity` asserts this refusal and
counts them as interp-only, not `BUILD-FAIL`; 35/39 other examples match byte-for-byte.
The wasm toolchain is provisioned by `scripts/setup-environments.sh` (browser tier).

## Repo Hygiene Notes

- Build artifacts (`agent_task`, `*.rlib`, `dist/`) are gitignored, not committed.
- Merged feature/worktree branches are pruned periodically; live work lives in
  `.claude/worktrees/` worktrees (gitignored).
- One corrupt zero-byte ref (`refs/heads/worktree-agent-a1a0476b66333a42f`) needs a
  manual `rm .git/refs/heads/...` — git plumbing can't repair an unresolvable ref and
  the sandbox blocks writes into `.git/`.
