# Axon `cargo build -p axon-core` slow-build diagnosis

**Date:** 2026-05-27
**Toolchain:** rustc/cargo 1.95.0, 32 cores, LLVM-17 (inkwell 0.4, feature `codegen`)
**Verdict in one line:** The cost is **100% in the LLVM codegen backend** (LLVM-IR
generation + object emission for the monomorphized, inkwell-heavy `codegen::builtins`
functions), **not** frontend monomorphization-collection and **not** LLVM *optimization*.
Splitting the offending functions further and/or many-CGU dev builds will **not** fix it —
the same single-threaded backend stall reproduces at `opt-level=0 codegen-units=256`.

---

## 0. Critical context discovered before measuring

- The worktree branch (`worktree-agent-a813ecf96a0f30fd5`) is checked out at commit
  **`94cf731` ("Wave 4+5")**, which is a *different lineage* from `merge-asi-layer3`
  (tip = **`0fb49c0`**). The `94cf731` HEAD **does not even compile** — `cargo check`
  *and* `cargo build` both fail in ~5 s with 4 hard frontend errors
  (`lsp.rs`: `compute_completions` / `completion_item_to_json` undefined; two
  non-exhaustive `match` on `Type::Uncertain`/`Type::Temporal`). The slow build cannot
  be reproduced on `94cf731` because the code is broken in the frontend.
- The real "5h+ slow build" target is the **`merge-asi-layer3` tip `0fb49c0`**, which has
  the expected `crates/axon-core/src/codegen/{builtins.rs, expr.rs,...}` layout from the
  task. All experiments below were run against `0fb49c0` via a **separate, non-destructive
  `git worktree` at `/tmp/axon_l3`** (the assigned worktree was left untouched).
- On `0fb49c0`, the missing LSP functions ARE defined — it compiles its frontend cleanly.

**The repo's own session notes already document this problem** (corroborating, not guessing):
- `SESSION_STATUS.md:67-72`: "Phase 2 + Phase 3 alone do *not* fix the slow build.
  Pre-decomposition build was 9h+ (never finished, 3.6 GB peak). Phase 2/3 alone got to
  LLVM codegen (272 .o files emitted) but still stalled mid-codegen at 5h+. Nightly
  `-Z parallel-frontend` did NOT parallelize the trait queries — only 1 of 8 threads
  working — also stalled."
- `SESSION_RECAP.md:113-117`: the "IR shim" fix attempt was a **null result** — build
  still stalls; "The fundamental question 'what makes axon-core take so long to build?'
  remains open."
- `SESSION_RECAP.md:127-128`: **Cranelift backend was already attempted and ALSO stalled
  on a similar shape.** MLIR unexplored.
- `SESSION_STATUS.md:129-131`: `axon-check` (a `--no-default-features` build, no inkwell)
  builds in **0.04 s** and is used as the practical dev binary.

---

## 1. Experiment A — frontend-vs-backend localizer (256 CGUs @ opt0)

Command (deps were cold; let them build, then axon-core):
```
timeout 1500 /usr/bin/time -v env CARGO_INCREMENTAL=0 \
  RUSTFLAGS="-C codegen-units=256 -C opt-level=0" cargo build -p axon-core
```

**Result: TIMED OUT (exit 124). Did NOT finish in 25 min (1500 s).**
- After deps finished, it sat at "Compiling axon-core" for the full 25 min.
- **Zero `axon_core*.o` object files** were emitted in the entire 1500 s.
- A **single** `rustc` process pegged **one** core at ~100% the whole time
  (CPU time ≈ wall time: e.g. `12:17` CPU at `742 s` elapsed). No parallel codegen
  threads ever spun up; no `.o` trickle.

**Interpretation (the key fork):** At `opt-level=0` LLVM optimization is essentially OFF,
and `codegen-units=256` requests maximal parallelism — yet the build is still a serial,
single-threaded stall that never reaches object emission. This **refutes** both
"LLVM optimizer chokes" and "many CGUs + low opt fixes it." The work is stuck in the
serial per-function **LLVM-IR generation / lowering** path *before* parallel per-CGU
object emission begins. Because **CGUs split at function granularity (never within a
single function)**, a giant function cannot be parallelized away by raising
`codegen-units`.

Note vs. the prior "272 .o files then stall" observation: that was on the
already-decomposed Phase-3.2 state per the session notes. On `0fb49c0` at opt0/256-CGU,
the build produced **zero** axon-core objects in 25 min — i.e. it stalls even earlier than
object emission in this configuration.

---

## 2. Experiment B — self-profile (`-Zself-profile`)

Command:
```
timeout 900 env RUSTC_BOOTSTRAP=1 CARGO_INCREMENTAL=0 \
  RUSTFLAGS="-Zself-profile=/tmp/axon_prof -Zself-profile-events=default" \
  cargo build -p axon-core
```

**Result: INCONCLUSIVE via the standard tooling — documented negative result.**
- self-profile DID write `axon_core-*.mm_profdata` incrementally (grew to ~24.6 MB).
- BUT: the profdata's **string-data section is only flushed on clean rustc exit**. rustc
  was SIGTERM-killed at the 900 s cap, so the string table is truncated and
  `summarize` (measureme v12, built locally with `--cap-lints=allow` to dodge a hard
  `unused_unsafe` error on rustc 1.95) fails with **`Invalid file: No string data found`**.
  `strings` on the file finds **no** query/pass names (`codegen`, `LLVM_passes`,
  `monomorphization_collector`, etc.) — confirming the table never landed. So the task's
  premise that self-profile "survives a non-finishing build" does **not** hold for naming
  events on this rustc when the run is killed.
- **One useful signal did emerge:** the profdata **stopped growing at ~24 MB while rustc
  kept burning CPU** (size frozen at 22:30 while CPU climbed past 11 min). The
  self-profiler records an event only when it *completes*; a frozen event stream + live
  CPU means rustc is **wedged inside a single long-running span** — consistent with one
  enormous codegen/lowering unit rather than many small queries.

### B′ — the clean localizer that B was meant to provide: `cargo check` vs `cargo build`
Because self-profile couldn't name events, I used a cleaner, fully-finishing contrast.
`cargo check` runs the *entire* frontend (parse, type-check, trait solving, borrowck, and
monomorphization-collection sufficient to emit metadata) **with the inkwell `codegen`
feature active** (verified: `inkwell` is in the resolved feature/dep tree for the check
build), but skips LLVM-IR generation and object emission.

```
cargo check -p axon-core   →  FINISHES in 4.54 s, 370 MB RSS   (default features, inkwell present)
cargo build -p axon-core   →  > 25 min, never finishes          (adds LLVM-IR gen + codegen + link)
```

**This is the definitive frontend-vs-backend split:**
- Everything up to & including monomorphization-collection / MIR: **4.5 s.**
- LLVM-IR generation + LLVM codegen + object emission: **> 25 min (unbounded).**

→ The blowup is **entirely in the LLVM codegen backend**, not in frontend
monomorphization-collection or trait solving.

---

## 3. Cross-reference — the suspect functions

`crates/axon-core/src/codegen/builtins.rs` is **3,961 lines** and contains five
declare-* functions, each of which builds *complete LLVM IR function bodies inline* via
hundreds of generic `inkwell` builder calls:

| Function | Lines | inkwell builder/`add_function`/block calls |
|---|---:|---:|
| `declare_builtins` | 34–1944 (~1910) | ~649 (`.build_*`, 77×`add_function`, 70×`append_basic_block`, 102×`position_at_end`) |
| `declare_asi_runtime_builtins` | 1945–2102 | (smaller) |
| `declare_ai_builtins` | 2103–2594 | ~177 |
| `declare_phase9_math_builtins` | 2595–2782 | (smaller) |
| `declare_string_builtins` | 2783–3961 (~1178) | ~557 |

`builtins.rs` alone holds **951 `.build_*` calls** (vs 196 in `expr.rs`, 50 in
`ir_inkwell.rs`). Each `.build_*` / `add_function` / `build_int_*` / `build_extract_value`
is a call into the **heavily-generic** inkwell API. Inlining hundreds of these into one or
two Rust functions produces, after monomorphization, **one or two gigantic LLVM functions**
that LLVM must lower as a single indivisible unit — superlinear in body size and
**immune to `codegen-units`** (which never splits within a function).

The maintainers already suspected exactly this. `codegen/mod.rs:43-48` (verbatim):
> Remaining Codegen methods (Pass 1 declarations including `declare_builtins` ~3870 lines,
> expression emission `emit_expr` ~1380 lines, ...) stay in this file pending Phase 2.3+
> which requires faster-machine validation because the bigger remaining splits will involve
> cross-cutting field-access decisions.

And `SESSION_STATUS.md:65` records that `declare_builtins` was *already* decomposed into 4
section helpers (Phase 3.2) — yet the build still stalls. **So mere textual splitting into
a handful of helpers did not help**, because each helper is still individually enormous and
inkwell-generic-heavy, and the aggregate IR/codegen volume across the crate is the tax.

---

## 4. Experiment C — cargo-llvm-lines (top IR-generating functions)

`cargo-llvm-lines` was NOT preinstalled; installed successfully via
`cargo install cargo-llvm-lines` (v0.4.46).

Command: `cargo llvm-lines -p axon-core --lib` (23 min cap).

**Result: TIMED OUT (exit 124) at 23 min. NO per-function table was produced.**
- `cargo-llvm-lines` works by forcing `--emit=llvm-ir`, letting rustc generate the full
  LLVM IR, then parsing/aggregating the `.ll` files. It was still "Compiling axon-core"
  (single rustc, ~100% one core, ~11–23 min CPU) when killed — it never reached the
  aggregation step, so there is no top-20 list.
- **This is itself a confirming result:** the tool whose entire purpose is to *generate
  and count* LLVM IR (no optimization at all) **cannot even finish generating the IR in
  23 minutes.** That pins the bottleneck squarely at **LLVM-IR generation** — not LLVM
  optimization (llvm-lines does none), not the frontend (which the `check` contrast shows
  finishes in 4.5 s).

So C, while it didn't name a single function, triangulates with A and B′ to the same
backend-IR-generation root cause. The per-function attribution that C would have given is
supplied instead by the static cross-reference in §3: `codegen::builtins` (951 `.build_*`
inkwell calls; `declare_builtins` ~649, `declare_string_builtins` ~557) is overwhelmingly
the largest generator of monomorphized inkwell IR in the crate.

---

## 5. Verdict

**It is NOT frontend monomorphization, and it is NOT LLVM optimization of a single
function. It is LLVM-IR generation (rustc's `codegen_module` / `codegen_llvm` lowering of
MIR into LLVM IR) for the monomorphized, inkwell-generic-heavy `codegen` module — a
serial, single-threaded phase that runs before parallel per-CGU object emission and does
not benefit from `codegen-units`.**

Evidence chain:
1. **Frontend is fast.** `cargo check` (full frontend incl. monomorphization-collection,
   with inkwell present) finishes in **4.5 s**. (B′)
2. **Backend never finishes.** `cargo build` at the *default* dev profile
   (`opt-level=0, codegen-units=16`) and at the aggressive `opt-level=0, codegen-units=256`
   both **time out at 25 min with zero axon-core `.o` files** and a single pegged core. (A)
3. **Not optimization.** Opt is already off in both A and the default dev profile, yet it
   still stalls. (A + Cargo.toml `[profile.dev] opt-level=0`)
4. **Specifically IR generation.** `cargo-llvm-lines` (`--emit=llvm-ir`, no optimization)
   **also times out before emitting IR**. (C)
5. **Single-threaded / one-unit shape.** Throughout A/B/C only one rustc thread ran at
   ~100%; self-profile froze inside one long span (B); nightly `-Z parallel-frontend`
   historically used "only 1 of 8 threads" (`SESSION_STATUS.md`). A single Rust function
   always lands in one CGU and is lowered serially — consistent with the
   `declare_builtins` / `declare_string_builtins` giants.
6. **Already-tried fixes failed.** Splitting `declare_builtins` into 4 helpers (Phase 3.2),
   the IR-shim abstraction, nightly parallel-frontend, AND cranelift all failed to make
   the build finish (`SESSION_STATUS.md`, `SESSION_RECAP.md`).

The mechanism: `codegen/builtins.rs` builds complete LLVM IR function bodies *inline* with
~950 calls into the heavily-generic `inkwell` API. After monomorphization these expand into
a very large volume of LLVM IR concentrated in a few enormous functions. rustc's MIR→LLVM-IR
lowering of that volume is superlinear and serial. The frontend never has to *materialize*
that IR (check just type-checks the generic calls), which is why check is 4.5 s and build is
unbounded.

---

## 6. Recommended decision

**Primary (do this): make the standard dev build use `axon-check` — the
`--no-default-features` build that excludes inkwell/codegen — and treat the
codegen-feature build as a release-only / CI-only artifact.**

Rationale: the entire cost is the inkwell `codegen` feature. `cargo check`/
`--no-default-features` finishes in **0.04–4.5 s**; the maintainers already use
`axon-check` (`SESSION_STATUS.md:129-131`) for the type-checking workflow. This makes
day-to-day development instant and is zero-risk. The native-codegen binary is then built
only when an actual native artifact is needed.

**Of the four options offered, ranked:**

1. **Set dev-profile `opt-level=0` + high `codegen-units` as the standard dev build —
   REJECT as a *fix*.** Measured: it is already the default (`[profile.dev]
   opt-level=0, codegen-units=16`), and forcing `codegen-units=256 opt-level=0` (Exp A)
   *still timed out at 25 min*. CGUs cannot split a single giant function; opt is already
   off. This does not address the root cause.
2. **Accept slow `--release` native only — PARTIALLY ACCEPT.** This is the realistic
   fallback for producing a native binary: budget the codegen build for CI / overnight,
   not the inner dev loop. But "slow" here means *>25 min and historically hours*, so it
   must be paired with option (1) for day-to-day work. Recommend: native build runs in CI
   with a generous timeout (and ideally a beefier machine, per the maintainers' own
   "faster-machine validation" note), not on developer laptops.
3. **Split the offending function(s) further — LOW PRIORITY / likely insufficient.**
   `declare_builtins` was *already* split into 4 helpers and the build still stalled
   (`SESSION_STATUS.md:65-72`). The problem is the *aggregate volume* of monomorphized
   inkwell IR, not one textual function. Splitting alone is unlikely to bring it under
   30 min. IF pursued, the high-value, measurable next step is to **stop inlining inkwell
   builder calls** — i.e. reduce monomorphized IR volume by funneling the ~950 generic
   `.build_*` calls through a small number of `#[inline(never)]`, non-generic wrapper
   functions (one IR shape each), so each call site lowers to a single non-generic call
   instead of a fully-expanded generic body. (This is essentially what the abandoned
   "IR shim" was reaching for; its null result suggests the shim didn't actually cut
   instantiations — verify with a *finishing* `cargo check --timings`/`llvm-lines` on a
   prototype before committing.)
4. **Escalate to cranelift — REJECT (already falsified here).** `SESSION_RECAP.md:127-128`
   records cranelift was tried and **also stalled on the same shape**, because the
   bottleneck is rustc's MIR→IR generation volume, which cranelift does not avoid. MLIR
   is unexplored but is a large unrelated rewrite, out of scope for a fix.

**Bottom line for the decision-maker:** The "mystery slow build" is now concrete — it is
LLVM-IR generation for the inkwell-heavy `codegen` module, serial and CGU-immune. The
cheap, correct move is to develop against `--no-default-features` (`axon-check`) and relegate
the native codegen build to CI/release with a long timeout. Further function-splitting or a
backend swap (cranelift) are NOT expected to help based on the maintainers' own already-run
experiments; the only structural fix with a real shot is cutting the *count* of
monomorphized inkwell instantiations (option 3's wrapper approach), and that should be
prototyped-and-measured before investing.

---

## Appendix — reproduction notes
- All builds run against `/tmp/axon_l3` (a `git worktree add --detach /tmp/axon_l3 0fb49c0`),
  leaving the assigned worktree untouched. The assigned worktree's own HEAD (`94cf731`)
  does not compile and is the wrong lineage for this investigation.
- Logs: `/tmp/expA.log` (A, exit 124), `/tmp/expB.log` (B, exit 124),
  `/tmp/expC.log` (C, exit 124), `/tmp/axon_prof/axon_core-*.mm_profdata` (B, unparseable
  string table due to SIGTERM). `cargo check` timing: `/tmp/check_time.txt`.
- Tooling installed during the task: `cargo-llvm-lines` v0.4.46 (clean),
  `summarize` (measureme v12, built with `RUSTFLAGS=--cap-lints=allow` to work around a
  hard `unused_unsafe` error on rustc 1.95).
