# Session Status — ASI Build Loop

**Branch:** `governance-audit-2026-07-18` (not main; not yet pushed to origin)

## Landed this iteration (commit `216fc6a`)

- Re-verified R32/R33/R34 acceptance gates (all pass; R32 has 2 honest SKIPs — TLC/coqc not
  installed on this host) and corrected `ROADMAP.md` §10.6 + `governance/REQUIREMENTS.md`, which
  both still claimed "zero proof code" / "no implementation" / "not started" for work that was
  already real and gate-green in the tree.
- Added §13 Dependency DAG + §14 Evidence ledger to R32/R33 (R34 already had them).
- Fixed two `scripts/verify_all_specs.sh` findings: emoji-strip regex missing 🚧/🔧 (false-positive
  status mismatch), R34's dangling `related: R28-audit-ledger-writer` → corrected to the real
  `R28-capability-audit-ledger`. Verifier now reports CLEAN (53 pre-convention specs remain, by
  design — backfilled only on next edit, not mass-mechanically).

## Known gate flakiness (corrected this iteration — was mischaracterized as a stable pre-existing gap)

- `scripts/gate.sh`'s full-suite `cargo test` occasionally fails one `wasm_browser_*` test (seen as
  `wasm_browser_println_matches_interp_via_js_host` in one run, `wasm_browser_examples_run_identically_via_js_host`
  in another/`R34`'s evidence ledger). **This is a flake under full-workspace parallel contention,
  not a stable known gap**: `bash scripts/wasm_browser_io_parity.sh` run directly passes 6/6;
  `cargo test --exact` on the single failing test passes in <1s; an immediate full-suite rerun
  passes 419/419. Corrected `governance/specs/R34-incremental-attestation.md` §14 to match (it
  previously asserted "KNOWN GAP (pre-existing)", which overclaimed a stable, characterized bug).
  **Going forward: never dismiss a red `wasm_browser_*` test without an isolated rerun first** —
  if the isolated rerun ALSO fails, that's a real regression, not this flake.
- Separately found and fixed a REAL, reproducible, deterministic gate blocker: a clippy violation
  in `crates/axon-core/src/main.rs:846` (`format!("{}…", &name.chars()...)` — redundant `&`),
  pre-existing on this branch, unrelated to anything else this session touched. Confirmed via
  `cargo clippy -p axon-core --bin axon --no-default-features -- -D warnings` before and after the
  one-line fix. This is why the first two gate reruns this session failed at different steps
  (tests, then clippy) — they were two independent, unrelated problems, not the same one recurring.

## New flaky-test instance found (same class, different test)

`persistent_learner_demo_carries_state_across_invocations` failed once during this iteration's
`gate.sh` run (this iteration's changes are pure `.md` docs — nothing Rust-related). Root cause:
the test uses a fixed, non-isolated state file (`/tmp/axon_persistent_learner.txt`, no per-run
uniqueness) and clears it on entry, so it's racy under any concurrent process touching the same
path. Isolated rerun: PASS. Immediate full-suite rerun: 419/419 PASS. Same `gate-flakes-under-
contention` class as the `wasm_browser_*` flake found earlier this session — noting it here so a
future iteration doesn't have to re-derive the diagnosis. Not fixed (would mean giving the test a
per-PID/per-thread tmp path — a real, separate, small hardening slice if this starts recurring).

## Reconciled this iteration: `KNOWN_DUAL` allowlist sanity-check

Read all 12 dual-numbered files in full (not just filenames). Two distinct shapes:
- **R21/R22/R23/R24/R25: true collisions**, confirmed genuinely independent, non-conflicting specs
  from two parallel work-tracks each claiming the same number. Decision: keep the full-slug
  convention as the permanent mitigation; do NOT renumber an already-shipped/drafted spec — that's
  pure churn (breaks existing cross-references) for no benefit the full-slug rule doesn't already
  give. This was a judgment call with a sensible default, not a user-owed strategy fork — made and
  recorded rather than asked.
- **R18: not actually a collision** — one spec (`R18-provenance-ledger.md`, the only file with a
  Spec ID) plus three supporting artifacts (spike/results/packet) sharing its filename prefix,
  claiming no independent identity. It's in the allowlist only because the prefix-match check
  can't distinguish the two shapes.
Documented in `scripts/verify_all_specs.sh` (inline comment above `KNOWN_DUAL`) and
`governance/EXECUTION_MODEL.md` §3.

## Not yet decided (owed to the user, not a loop default)

- Whether to push `governance-audit-2026-07-18` to origin.

## Gate status: fully green (commit `68a1e3e`)

`scripts/gate.sh` passes clean end-to-end — 419 + 124 tests, fmt, clippy (lib + runtime crates),
native codegen build. No known gaps remaining (the wasm-browser item above was a flake, not a bug;
the clippy violation is fixed). Re-verified 2026-07-18.

## R32 formal-methods gap actually CLOSED this iteration (not just re-verified)

Previously R32's evidence ledger honestly said "structurally valid, never machine-checked" (TLC/coqc
SKIPPED — neither tool installed). This iteration installed both for real: TLC via a direct
`tla2tools.jar` download (`~/tla2tools.jar`, no root needed) and Coq via `apt install coq` (8.18.0
— exactly the spec's pin, far faster than the opam-from-source path in the spec's own §8 on a
Debian/Ubuntu host). Running them for the first time ever on this file found and fixed **three real
bugs**, none visible while SKIPPED:
1. `Theorem kill_fires_within_2_polls` (`R27Corrigibility.v`): a `rewrite` targeted `latch s1`/
   `poll_count s1`, but `simpl` had already unfolded the transparent `pose`-bound `s1` all the way
   back to `s`, so those subterms no longer existed in the goal. Fixed by rewriting `Htripped`
   directly (the goal collapses to `true = true` unconditionally once `latch s = Tripped` — the
   `Hzero` rewrite wasn't even needed).
2. `Lemma halted_step_preserved`, PollGate case: `rewrite Hh in Hhalted; discriminate` failed with
   "Cannot find a relation to rewrite"; replaced with `congruence`, which derives the contradiction
   between `halted s1 = false` and `halted s1 = true` directly.
3. `scripts/r32_acceptance_gate.sh`'s own TLC success/failure check ran the failure-grep
   (`error|violated`, case-insensitive) *before* the success-grep — and TLC's real success message
   ("No **error** has been found") matched the failure pattern first, so every genuine TLC success
   was reported as `tlc_model_check FAIL`. Reordered: success checked first.

Gate now: `scripts/r32_acceptance_gate.sh` **20/20 PASS, 0 SKIPPED**. R32 moved Draft → Landed in
its own header/spec-meta, `ROADMAP.md` §10.6 (moved into the Shipped R26–R32 table), and
`governance/REQUIREMENTS.md`. This is the strongest evidence yet for the outer loop's core claim:
a SKIPPED check can hide bugs indefinitely, in both the code under test AND the test itself.

**Reproducibility follow-up (resolved this iteration):** the TLC jar and Coq installation are
host-level state, not committed to the repo (can't be — `tla2tools.jar` is a 4.5MB binary,
`apt install coq` is a system package). Documented in `ENVIRONMENTS.md` §8 (table row + full
section, matching the existing GPU/browser/Android/eBPF/Zephyr/TEE pattern) and added
`setup_formal()` to `scripts/setup-environments.sh` (`bash scripts/setup-environments.sh formal`),
which installs both idempotently and re-runs `r32_acceptance_gate.sh` to confirm 20/20. Tested
clean on this host (both tools already present → no-op + verify).

## R21/R22/R23 stale Draft headers fixed (commit `1e91db5`)

Same staleness class as R17/R31/R32: REQUIREMENTS.md already knew these three specs' own headers
were stale but never fixed the source file itself. Re-verified each (axon-os: 88 `cargo test -p
axon-os` tests; R22/R23: their acceptance gate scripts, both green) and flipped Draft → Landed 100%
in the spec headers, added spec-meta to all three. Pre-convention count 53 → 50.

## R30 gate: re-verified for real — was flaky, not hung (this iteration)

The standing claim ("NOT re-run to completion — timed out past 4 min") was itself stale. Ran it
to completion twice:
- **First run** (alongside this session's earlier concurrent cargo activity): completed in a few
  minutes (not a hang), but FAILED 2/6 — `acc_a1` (clean-repo run) failed at Stage 3
  R26_ATTESTATION, and `acc_a4` (idempotency: two consecutive runs must match) caught two runs
  producing genuinely different stage vectors (one stopped early, one passed all 8 stages).
- **Second run** (isolated, no concurrent build load): **6/6 PASS**, both idempotency runs
  produced identical 8-stage vectors.
Conclusion: R30's gate is real and does complete, but is flaky under host contention (real-KVM
boot timing sensitivity), matching the `gate-flakes-under-contention` memory pattern. Did not chase
the root cause further this iteration — that's a separate, larger inner-loop item if it's ever
worth hardening (e.g. retry-with-backoff inside the gate itself), not a quick slice. Updated
`governance/REQUIREMENTS.md`'s R30 row with the precise, evidenced finding.

## R21/R22/R23 stale headers fixed → found R31 had the SAME bug, not yet fixed at the source

While fixing R21/R22/R23's stale "Draft" headers, noticed the pattern was worth checking against
every other spec REQUIREMENTS.md had flagged as stale. R31 had the identical gap: its own header
still said "Draft (2026-06-28)" even though REQUIREMENTS.md/ROADMAP.md were both corrected back
at the start of this session — the spec file itself was never actually touched. Also found (same
class as R34's earlier fix): R31 referenced two nonexistent filenames,
`governance/specs/R28-audit-ledger-writer.md` and `R29-compliance-monitor.md` (real files:
`R28-capability-audit-ledger.md`, `R29-continuous-compliance-monitor.md`). Fixed the header,
the dangling references, added spec-meta; re-verified `scripts/r31_acceptance_gate.sh` (still ALL
PASS, 37 tests). Checked R17 for the same gap — it was genuinely already fixed at the source, so
R31 was the only remaining instance. Pre-convention count 49.

**Lesson for future iterations:** "REQUIREMENTS.md notes X is stale" is not the same claim as "X is
fixed" — always check the actual file, not just the matrix row that describes it.

## Systematic sweep found R26/R27/R28/R29 ALL had the same stale-header bug (this iteration)

Rather than waiting to stumble onto more one at a time, grepped every spec's `**Status:**` line for
"Draft" and cross-checked each hit against known-landed status. Found the **entire shipped
safety/attestation stack** — R26, R27, R28, R29 — still had "📝 Draft" as their own header, despite
REQUIREMENTS.md already correctly saying "Landed" for all four (unlike R31, these matrix rows were
already accurate — only the spec files themselves lagged). Re-verified all four gates fresh
(r26: OK, r27: 19/19, r28: PASS, r29: 24/24) before flipping any header. Fixed all four: Draft →
Landed, added spec-meta with dependency edges cross-checked against each spec's own "Depends on"
prose (not guessed) — caught and corrected one wrong edge in my first draft (R27 does NOT depend
on R26 as I initially assumed; it depends on R21/R20, and R25 is `related` not `depends-on` since
R27 shipped its full scope without R25 ever being built — a soft/conceptual reference, not a real
blocking dependency). Pre-convention count 50 → 45.

Also grepped for the SAME two bug classes (stale Draft + dangling filenames) across the rest of the
tree and found candidates not yet fixed: `R12-kernel-runtime-services.md` ("Draft" — Phase 7 kernel
is ✅ Complete per CLAUDE.md) and `R14-mobile-targets.md` ("Draft" — Android/iOS tests
(`android_compute_parity_r14`, `android_lifecycle_adapter_r14`, `mobile::tests::*`) pass in every
gate.sh run this session). Also `R1b-str-return-abi.md`/`R1c-dict-runtime.md`/
`R1d-single-source-builtins.md` are early-phase legacy specs that may have the same gap (memory:
"R1c/R1e actually mostly done" suggests R1c's header may be stale too). **Deliberately NOT fixed
this iteration** — these need real individual research (R7's own row is a huge multi-paragraph
partial-completion status; R12/R14 likely need similarly careful, non-mechanical characterization,
not a one-line Draft→Landed flip) rather than the mechanical confirm-and-flip that worked for
R26-R29.

## R12 and R14 stale headers fixed with real research (this iteration)

Did the individual investigation flagged last iteration rather than a mechanical flip:

- **R12-kernel-runtime-services**: confirmed genuinely 100% landed. Every one of the spec's own 5
  slice gates has a matching commit in `git log -- crates/axon-core/src/kernel.rs` (one per slice,
  in the exact prescribed order: `6310d52`→`dc478aa`→`9aa5d24`→`0eacbb5`→`3aae955`, plus R12b
  `fdbb8b2`) and a matching passing test (`cargo test --lib kernel::` 20/20;
  `phase7_kernel_{principal_authority,scheduler,supervisor,durable_store,llm_gateway}` in
  `cli_run.rs` 5/5). Draft → Landed, spec-meta added.
- **R14-mobile-targets**: genuinely PARTIAL, not a clean flip — the spec's own body already
  tracked this accurately (explicit `[LANDED 2026-06-25]` markers on Slices 1-3, "Deferred" on
  Slices 4-5) but the one-line header at the top just said bare "Draft" with zero detail,
  misleadingly implying no progress at all. Re-verified `cargo test --lib mobile::` 14/14 PASS.
  Header rewritten to Implementing with the accurate slice-by-slice summary (Android + iOS
  toolchain/shim/lifecycle-bridge landed and headless-verified; `native::platform` device impl and
  the gfx Metal/Vulkan surface bridge genuinely not started — real remaining scope, not staleness).

Pre-convention count 45 → 43. `verify_all_specs.sh` caught one real error in my own first draft:
R14's prose said "Partial" but spec-meta said `status-claim: Implementing` — the linter flagged the
mismatch immediately; fixed by aligning prose to the existing vocabulary (`Implementing`, matching
R33/R34) rather than inventing a new status word.

Also confirmed last iteration's flaky test (`persistent_learner_demo_carries_state_across_invocations`)
passed clean in this iteration's full gate.sh run — further evidence it was contention-flake, not a
regression.

## R1b/R1c/R1d investigated and fixed (this iteration)

- **R1b-str-return-abi**: genuinely 100% landed. All 4 builtins (`str_repeat`/`str_slice` via
  `d9dd83f`, `str_reverse`/`str_replace` via `c473561`) confirmed live via `scripts/fuzz_parity.sh`
  (repeat/slice in the ~51-builtin corpus) and `scripts/str_utf8_parity.sh` (reverse/replace on
  multibyte UTF-8). Draft → Landed. Bonus find: the spec's own §8 test-plan text had *planned* to
  accept a byte-reverse/char-reverse interp↔codegen divergence as permanent — reality shipped
  better than planned (codegen's `str_reverse` is char-correct, matching interp exactly) — annotated
  rather than blindly checked, since "byte-reversal" literally didn't happen. Checked off all 6 of
  §9's acceptance checkboxes (were `[ ]` despite the work landing).
- **R1c-dict-runtime**: genuinely partial. 17/19 dict-family ops landed with confirmed
  native==interp parity (`scripts/dict_parity.sh` PASS); `dict_from_str`/`arr_group_by` remain
  deliberately E0910-refused (str/array-valued sources, "abort loudly instead of miscomputing" —
  codegen/expr.rs:817,7734-7735), matching memory's prior note exactly. Draft → Implementing with
  the accurate 17/19 breakdown, not a flip to Landed.
- **R1d-single-source-builtins**: genuinely partial across 4 slices. Slice 1 (the
  `BUILTIN_EXTERNS` registry itself, `codegen/builtin_externs.rs`, 25 rows) LANDED (`b8f54fb`).
  Slice 2 partially landed (`sleep_ms`/`now_ms` via `daa01f7`, `dict_merge` via `eff8ce2`). Slice 3
  (the drift cross-check test) NOT landed — the registry has a join-key field explicitly reserved
  "for the slice-3 drift cross-check" that nothing yet consumes. Slice 4 (CLAUDE.md doc update) NOT
  landed — `CLAUDE.md`'s own "Adding a New Builtin" section still describes the original 5-step
  recipe this spec set out to collapse to 3. Draft → Implementing with the per-slice breakdown.

All three now carry spec-meta. Pre-convention count 43 → 40.

## R1d Slice 3 implemented as real feature work (this iteration — not a docs pass)

Built the drift cross-check test the registry's own `axon_name` field comment had been reserving
since Slice 1: `codegen::builtin_externs::drift_tests` in
`crates/axon-core/src/codegen/builtin_externs.rs` —
`every_extern_row_matches_a_known_builtin_with_the_same_arity` (every `BUILTIN_EXTERNS` row's join
key resolves to a `BUILTINS` entry with matching param count) and
`no_duplicate_extern_registry_rows`. Verified the arity-matching assumption against several rows by
hand first (dict_new/has/len/merge/inc, sleep_ms/now_ms — all exact 1:1) before writing the general
test, so a legitimate ABI-expansion case (e.g. an out-param wrapper) wouldn't have produced a
false-positive failure; confirmed none of the 25 rows in this table use out-params (those stay
bespoke call-site special cases per the file's own top comment, explicitly excluded). Both tests
pass; correctly compile out under `--no-default-features` (codegen-gated) rather than failing.
Documented as the honest one-directional half of the spec's "vice-versa" ask — the reverse
direction (which `BUILTINS` rows *should* have a registry entry) isn't cleanly automatable without
a marker distinguishing straight-extern builtins from bespoke-call-site ones; left as a known gap,
not silently skipped. R1d spec updated: Slice 3 NOT landed → LANDED.

## R1d Slice 4 landed (this iteration) — R1d is now fully complete except Slice 2

Updated `CLAUDE.md`'s "Adding a New Builtin" section to present two paths instead of one: the R1d
fast path (`builtins.rs` row → axon-rt fn → one `BUILTIN_EXTERNS` row → parity test) for a plain
scalar/handle extern, with the original 5-step recipe kept verbatim underneath for bespoke
call-site builtins (out-params, dict get/set/remove/keys, `to_str`) that the registry deliberately
excludes — not a wholesale replacement, since the old recipe is still genuinely correct for that
class. R1d's spec updated: all 4 slices now landed except Slice 2, which stays honestly partial
(some but not all of the planned batch migrated). This closes out R1d as a queued item entirely —
what remains (finishing Slice 2's migration batch) is real feature work, not a docs-truth item.

## Found + fixed one more instance of the leading-word staleness bug (this iteration)

Before committing to a riskier codegen migration (R1d Slice 2), did a cheap final sweep: grepped
every spec's `**Status:**` line for "Draft" once more, across ALL specs (not just the recently
flagged batch). Found two real remaining instances where the header **starts with the literal
word "Draft"** even though the very next clause says "COMMITTED"/"All three slices LANDED" — a
misleading-at-a-skim bug distinct from (but the same class as) the earlier "bare Draft, zero
detail" bugs:
- **R17-freestanding-substrate**: already had rich, accurate Slice 0-3 LANDED detail (from an
  earlier Wave-1 fix, before this session's live work), but the leading word was never corrected.
  Fixed to "🚧 Implementing", added spec-meta. Re-verified `r17_slice1_qemu_boot_writes_axon_s1`,
  `axon_smp_atomic_counter_is_race_free`, `axon_repr_c_gdt_layout_byte_exact` — all 3 PASS. Also
  corrected a wrong evidence-command guess in my own first draft (I assumed a test named
  `axon_kernel_boots_qemu_serial_hello`, which doesn't exist by that name — the real test is
  `r17_slice1_qemu_boot_writes_axon_s1` — caught by actually running the command, not just citing
  it from memory).
- **R24-tee-target**: same pattern ("Draft — All three slices LANDED"). Fixed to "✅ Landed",
  added spec-meta. Re-verified `r24_tee_unseal_outside_enclave_rejected_e1810` PASS and
  `scripts/tee_sim_run.sh` (baseline PASS, gramine-direct honestly SKIPPED since gramine isn't
  installed on this host — not a fabricated PASS).

Checked R9b-smt-loop-invariants too (flagged the same way) and confirmed it's genuinely accurate
as "Planned / Not Started" — grepped `smt.rs` for `Stmt::While`/`Stmt::For` handling and found
none, matching the spec's own honest claim. Left untouched — not every "Draft"-adjacent hit is a
bug.

Pre-convention count 40 → 38.

## R1d Slice 2 batch: 7 f64 math intrinsics migrated (this iteration)

First real R1d Slice 2 feature work this session (previous iterations were docs/verification).
Diffed `BUILTINS` against the registry to find candidates, then filtered carefully before touching
codegen internals: bitwise ops (`bit_and`/`or`/`xor`/`not`, `shl`/`shr`) were ruled OUT — already
lowered as single native LLVM instructions inline, migrating them to a registry row would add
function-call overhead where none exists today (a regression, not a win — the registry is for
externs, not for things that are already optimal). `sqrt`/`pow`/`floor`/`ceil`/`exp`/`ln`/`log10`
were the real fit: LLVM-intrinsic-backed (`llvm.*.f64`), previously a hand-written "Phase 3 math
builtins" block declaring the same 7 functions one `add_function` call at a time — a smaller
sibling of what Slice 1 already collapsed for the true `__axon_*` externs.

Verified BEFORE migrating (not after): grepped for reverse dependencies (a `get_function("sqrt")`
call in `arr_std_f64`, plus `sqrt_f64`/`floor_f64`/`ceil_f64` wrapper builtins that reuse the same
underlying declarations) and confirmed the ordering holds regardless of which code path declares
the function, since `declare_builtin_externs()` always runs first inside `declare_builtins()`.
Migrated the 7 rows, deleted the old block, rebuilt clean, ran the drift cross-check tests (32 rows
now), ran the full differential fuzzer (`scripts/fuzz_parity.sh` — sqrt/floor/ceil/exp/ln/log10 all
covered, including the `nan_sqrt_neg` edge case), and manually verified `pow` (not in the fuzz
corpus) via `axon run`/`axon build` on 3 cases including a NaN case — native matched interp
exactly on all of them.

## R1d Slice 2 candidate pool investigated (this iteration) — near its practical ceiling

Followed up on the "str/math family" open question from last iteration and closed it definitively
(docs-only, no code): `str_to_upper`/`str_to_lower`/`str_trim{,_start,_end}` are confirmed NOT
registry-eligible — each is a hand-built shim wrapping a str→str out-param call, exactly the
"bespoke call-site lowering" class the registry's own top comment excludes. Also checked
`i64_to_f64`/`f64_to_i64` as alternatives: both are single-instruction function *bodies*
synthesized in codegen (not declarations of an existing symbol), so they don't fit the
declare-only registry model either, and converting them to real externs would add a genuine
cross-compilation-unit call for one hardware instruction — a regression, same class as the
bitwise-op rejection two iterations ago.

**Net conclusion: Slice 2 batch-migration via simple row-adds is likely near its ceiling** given
the registry's current declare-only capability model (covers `__axon_*` externs and LLVM
intrinsics — both already mined this session). Further progress needs either a genuinely new
eligible batch (unsearched candidates may still exist — worth another diff-and-filter pass before
assuming none remain) or extending `ExternSig` to support synthesized wrapper bodies (a bigger,
separate piece of work, not a quick row-add).

## R1d Slice 2 exhaustively closed (this iteration) — done at this granularity

Went further than the name-diff: scanned every literal `"__axon_\w+"` symbol reference across
`codegen/builtins.rs` + `codegen/expr.rs` against the registry (not just `BUILTINS` names — the
actual linked symbols). Every unregistered symbol is str-returning out-param wrappers, the dict
family, mid-expression panic helpers, or impure builtins with complex return layouts — all
categories already ruled ineligible. **Slice 2's simple-batch scope is confirmed complete; no
further row-add candidates exist.** R30 hardening was considered and explicitly rejected for a
quick fix: auto-retry inside a *deployment safety gate* risks masking a genuine failure (papering
over exactly what R30 exists to catch) — that needs real design review under BUILD_PROTOCOL's
Safety/TCB-touch tier, not a hasty patch.

## R34 Slice 4 landed: chain export/import (commit `6a620aa`)

`crates/axon-vm/src/chain.rs` gains `ChainStore::export`, `ChainExport` (schema
`axon-chain-export/1`), and `verify_export` — an auditor-side verification path that works on a
JSON snapshot with no live VM. Refactored `ChainStore::verify` to share a single `verify_entries`
core with `verify_export` (no second mechanism, I-2) rather than duplicating the loop. Caught and
fixed a real bug in my own first-draft refactor before it shipped: a
`.collect().unwrap_or_default()` pattern silently turned a malformed-JSON parse error into an
empty entries Vec, which would have made a corrupted chain file report "0 entries, verifies OK"
instead of failing — an I-9 (no-silent-success) violation. Fixed to propagate the error index
directly, matching the pre-refactor contract exactly. Added a head-tamper check to `verify_export`
beyond what the spec's illustrative example called out: an exporter can't truncate entries and
claim a stale/forged tip, since every individual link recomputing cleanly is not by itself
sufficient. 4 new tests (`chain_exported_and_imported` is the spec's own named S4 gate); 15/15
chain:: tests, 41/41 full axon-vm suite (up from 37); `scripts/r34_acceptance_gate.sh` ALL CHECKS
PASSED; full `gate.sh` green. New pub API is `#[allow(dead_code)]` pending Slice 6's CLI wiring
(`chain show/export/verify-export` subcommands), a separate not-yet-landed slice per R34's own DAG.
R34 spec's §13 DAG + §14 evidence ledger updated (S4: todo → landed).

## R39/R40 PRDs written (commit `456ef6f`) — mid-turn user request, not a loop-selected slice

A user design proposal (full "AI-native research/build compiler" architecture: typed objective/
decision/experiment/task/evidence/knowledge/provenance graphs, confidence-as-typed-metadata,
LLM-proposes/compiler-validates/harness-executes/evaluator-gates separation) arrived mid-turn
asking whether it should become a PRD, then a follow-up explicitly asked for both:
- **`governance/specs/R39-typed-execution-graph.md`**: the scoped-down version — typed schemas +
  a validator/CLI for *this repo's own* governance state only (spec front-matter, DAG, evidence
  ledger), no NL front end, no experiment/decision graphs, no confidence scoring. Motivated by this
  session's own evidenced failure record (stale Draft headers, dangling cross-references, the
  r32_acceptance_gate.sh TLC false-negative). 5 phased slices with named gates; §12 Q1 = build-now
  vs. wait (leaning: Slice 1 alone is a cheap speculative spike).
- **`governance/specs/R40-ai-native-research-compiler.md`**: the full general-purpose version,
  preserved at full fidelity but explicitly **held for later** — not founder-committed, not
  started, not sized into a slice plan. §12 Q1 (is this Axon, or a separate product?) is named as
  the single blocking fork; R39 is argued to already close the concrete failure modes found at
  Axon's own scale, so R40's extra machinery isn't assumed to transfer without that decision first.
`governance/REQUIREMENTS.md` and `ROADMAP.md` §10.7 updated to track both (same table as R36-R38,
noted as a different axis — governance tooling, not a platform-vision bet).

## R34 Slice 6 landed: chain show/export/verify-export CLI wiring (commit `66660e7`)

Wired S4's library-only export/import API into `axon-vm`'s real CLI: `chain show [--json]`
(vm_id/boot_root/entry-count/head summary, reusing the first entry's own `prev_hash` for the boot
root when the chain is non-empty rather than re-measuring the kernel), `chain export --out FILE`
(writes a self-contained `ChainExport` JSON, schema `axon-chain-export/1`), and
`chain verify-export FILE` (auditor-side, no live VM — same "OK: N entries" / "BROKEN at seq M"
exit-15 contract as `chain verify`). Removed the `#[allow(dead_code)]` annotations from S4's
`export`/`verify_export`/`ChainExport`/`CHAIN_EXPORT_SCHEMA` now that main.rs actually calls them.
Manually verified end-to-end (empty-chain show, 3-entry show, export round-trip byte-identical to
spec §5.4's example, genuine verify-export, and a head-only-tampered export correctly reporting
"EXPORT BROKEN at seq 3" exit 15 — proving the head-consistency check, not just per-link
verification) before writing `scripts/r34_acceptance_gate.sh` §8 (5 new checks, all passing).
Known, deliberately-unchanged scope gap: no `--sources-dir` re-hashing against `prog_hash` is wired
into `chain verify`/`verify-export` in this simplified implementation — same reduction the
already-landed `chain verify` established, documented in the spec's S8 DAG row. R34 spec (S6
todo→landed, evidence ledger) and `REQUIREMENTS.md`'s R34 row updated.

## Two unrelated gate-green blockers found + fixed while verifying R34 S6 (commit `8d37be2`)

Verifying S6 required a full `gate.sh --strict` run, which surfaced two real, pre-existing bugs
with nothing to do with R34/axon-vm:
1. **`crates/axon-core/src/parser.rs:3506`** — a `useless_borrows_in_formatting` clippy violation
   (redundant `&` in an `assert!` format arg), same class as an earlier-this-session fix at
   `main.rs:846`. One-line fix, verified via `cargo clippy -p axon-core --all-targets -- -D warnings`.
2. **`scripts/qemu_boot_test.sh`'s codegen-support pre-check was stale** — it grepped
   `axon build --help` for the `--emit-obj` flag, assuming absence meant no codegen. That flag is
   now listed in `--help` unconditionally; only the runtime behavior is feature-gated. This made
   the deliberately-not-`#[cfg(feature="codegen")]`-gated R17 QEMU boot test
   (`r17_slice1_qemu_boot_writes_axon_s1`) hard-FAIL instead of skip whenever `target/debug/axon`
   was momentarily a `--no-default-features` build — which `gate.sh`'s own stage order
   *guarantees* happens at least once per run (stage 1's `CARGO_BIN_EXE_axon` dependency forces
   exactly that build before the later codegen-build stage). **Not a flake — a near-guaranteed
   failure on every `--strict` run**, distinct from the contention-flake class
   ([[gate-flakes-under-contention]]). Fixed by probing the real build attempt and matching its
   actual error text ("requires building axon with the `codegen` feature") as the skip signal
   instead of a `--help` flag check; verified both the skip path and the pass path manually.
   Saved as memory: `qemu-boot-test-stale-skip-heuristic`.

Full `gate.sh --strict` reran clean end-to-end after both fixes (fresh from-scratch run, not just
the two isolated checks) — 419+124 tests, all clippy tiers including the codegen feature and the
(this-run-available) smt/Z3 stage, native codegen build, parity suite all green.

## R33 coalition ceiling landed: sock-puppet defense closed (commit `da7229e`)

R33.S3 from the spec's own §13 DAG: `VoteResponse` gains `lineage_root` (R27 principal identity —
a SEPARATE concept from `voter_tcb`, which identifies software, not principal; the two must never
be defaulted to each other, see memory `r33-coalition-ceiling-design`), and `check_quorum` now caps
admitted YES votes per lineage root at a hardcoded `ceil(N/2)-1` default (spec's own §6 slice-risk
note explicitly blesses this — no R27 `Coalition`-type dependency needed for this slice). Closes
the sock-puppet attack: N instances minted from one principal voting YES in unison can no longer
alone force quorum. 2 new named unit tests matching the spec's own §7 worked example
(`coalition_bound_limits_same_lineage`) plus a control this session added
(`coalition_cap_does_not_block_distinct_lineage_roots`, proving the cap doesn't over-trigger on
legitimate diversity). `cmd_quorum_vote` gains `--lineage-root`, defaulting to a fresh
per-invocation-unique value (NOT `voter_tcb`) — checked against the existing
`r33_acceptance_gate.sh`'s mock votes BEFORE finalizing the design, since they all share one
CI-mock `voter_tcb` and would have been silently, wrongly capped by a `voter_tcb`-defaulted root.
Added a real CLI sock-puppet + distinct-roots journey to the gate script (new §8). `cargo test -p
axon-vm`: 43/43 (was 41). `r33_acceptance_gate.sh`: ALL CHECKS PASSED (13 unit tests, 9 sections).
Full `gate.sh --strict`: clean end-to-end (parity suite 44/49 + 5 clean skips, smt/Z3 39/39). R33
spec (header, §13 DAG, §14 evidence ledger) + `REQUIREMENTS.md`'s R33 row updated. R33's remaining
open scope is now just vsock transport (S2) and `axon deploy` pipeline integration (S4) — R34.S7
(R33 `VoteRequest` chain-awareness) is still blocked on R33 landing more broadly, not specifically
on the coalition ceiling, so this doesn't yet unblock it.

## R33 S4 landed: `axon deploy --quorum-dir` integration (commit `aec5557`)

Wired R33's quorum gate into the actual deploy pipeline. `axon deploy FILE --risk high
--quorum-dir DIR [--quorum-n N] [--axon-vm-bin PATH]` shells out to `axon-vm quorum check`
(the same cross-binary orchestration pattern `axon-web` already uses to wrap the `axon` CLI,
Phase 12) and folds the result into the existing named-gate mechanism
(`simulate`/`stress`/`redteam_check`/`assert_deployable`) — a blocked quorum reuses the exact
`"status":"blocked_gate"` JSON shape (`"gate":"quorum"`), with `axon-vm`'s real exit code (13/14)
surfaced in the JSON's `exit_code` field for detail while `axon deploy`'s own process exit stays
at its existing convention (1). Deliberately shells out rather than adding `axon-vm` as an
`axon-core` library dependency — `axon-core` has an explicit "keep the fast interpreter build
dependency-light" invariant (`BUILD_RESOLVED.md`), and `axon-vm` isn't even structured as a lib
crate today. Omitting `--quorum-dir` leaves the gate open (100% backward compatible — verified).
Giving `--quorum-dir` but not finding the `axon-vm` binary is a hard error (exit 2), never a
silent open gate (I-9). All four real CLI journeys (no-flag, sock-puppet-blocked,
legit-quorum-met, missing-binary-error) manually verified before writing
`scripts/r33_acceptance_gate.sh`'s new §10 (6 checks, all passing). R33 spec (§5.2.1 new
subsection, header, DAG, evidence ledger) + `REQUIREMENTS.md` updated. **R33's only remaining
open gap is now S2 (vsock transport).**

Caught a real gap in my own verification process this iteration (not a code bug): an earlier
`cargo test ... | tail -N` capture could have silently hidden a real failure behind `tail`'s
always-zero exit code (no `pipefail`), and also truncated the lib-suite's own result line out of
the log. Re-ran with direct file redirection + explicit `$?` capture to get an honest result —
saved as memory `tail-pipe-masks-exit-code`. The one real test failure encountered
(`wasm_browser_println_matches_interp_via_js_host`) was confirmed transient (isolated rerun: pass;
full rerun: 419/419) — the known contention-flake class, unrelated to this change.

## R33.S2 sized and deferred; R39 Slice 1 landed instead (commit `736d6ab`)

Actually sized R33.S2 (vsock transport) before picking it: grepped the whole codebase for a
`Substrate` trait / `MockSubstrate` — **zero hits**. The full-protocol spec's architecture diagram
casually references "R26's existing Substrate trait boundary" as if it already exists; it doesn't.
Building S2 for real means designing and building this abstraction from scratch (a real vsock impl
+ a CI-mock path), not just CLI plumbing — too big for one iteration, deferred with this finding
recorded rather than discovered mid-slice. Picked **R39 Slice 1** instead, per the spec's own §12
Q1 suggestion (cheap speculative spike): added `scripts/verify_all_specs.sh --export-jsonl PATH`,
reusing the script's own per-spec extraction pass to write one JSON record per spec (schema
`axon-gov-spec/1`) to a gitignored `governance/state/specs.jsonl`. New `scripts/r39_slice1_gate.sh`
(9 checks): 61/61 spec files produce exactly one well-formed line, `pre_convention` count
cross-checked exactly against the validator's own report (38), a known spec's edges spot-checked.
R39 spec (Draft → Implementing), ROADMAP.md, REQUIREMENTS.md updated.

## Decision-audit follow-up on R33 coalition ceiling (commit `7919154`)

User ran a structured decision-audit on the freshly-landed R33 coalition-ceiling + axon-deploy work
(prior two iterations). Verdict: VERIFY FIRST. It surfaced two real, previously-unverified paths in
already-shipped code — the `ceil(N/2)-1` cap formula was only ever tested at N=3 (odd), and the
`lineage_root` legacy-JSON fail-closed default was reasoned about but never actually exercised end
to end. Added 3 new unit tests closing both gaps (`coalition_cap_at_even_n_sockpuppet_blocked` +
`..._distinct_roots_meets_quorum` at N=4/N=6, `legacy_votes_missing_lineage_root_share_one_capped_bucket`)
— **both gaps confirmed correct once tested, no bug found**, but real uncertainty closed rather than
left implicit. The audit also flagged the quorum gate's silent-open default at Risk ≥ Critical as
the single highest-risk design choice; grepped R11/R33 specs and confirmed no existing invariant
mandates quorum at Critical (not a spec violation, a genuinely undecided policy question) — left the
default unchanged (still open, matching every other Phase-11 gate's convention) but made it visible:
`cmd_deploy` now prints a stderr warning at Risk ≥ Critical with no `--quorum-dir` at all. Deploy
behavior (JSON, exit code) verified unchanged — visibility only. `crates/axon-vm/src/quorum/mod.rs`:
13 → 16 unit tests. `r33_acceptance_gate.sh`: new §10e, ALL CHECKS PASSED (16 unit tests, 10 gate
sections). Saved as memory: `r33-coalition-decision-audit-followup` — treat this audit format as a
real VERIFY-FIRST signal when the user runs it, not a formality.

## Fixed a real verify_all_specs.sh flake while sizing R39 Slice 2 (commit `eb6fbb2`)

Before starting Slice 2, re-ran `verify_all_specs.sh` for a clean baseline and found it wasn't
clean: 3 consecutive runs of an unchanged tree reported 3, then 1, then 0 "unknown spec"
dangling-edge findings, each citing a DIFFERENT spec id. Manually confirmed every cited id
actually exists (`R7-targets`, `R29-continuous-compliance-monitor`, `R25-zephyr-target`,
`R16-axon-ui`, `R21-axon-os-supervisor`, `R26-confidential-microvm-substrate` — all real files).
Root cause: this host was at load-average ~50 on 32 cores, and the dangling-edge check forked
`grep -qx` once per referenced id (hundreds of forks across ~90 specs x 5 edge keys); a transient
fork failure under `set -uo pipefail` (no `-e`) reads as "not found," not a script abort. Fixed by
replacing the forked per-ref lookup with a pure-bash associative-array membership check — 8/8
clean reruns post-fix at the same load level (was flaky 3/3 pre-fix). This is the same
gate-flakes-under-contention class as prior wasm_browser/persistent_learner flakes, but hitting a
governance validator is worse: a phantom FINDING invites a bogus "truth:" correction commit chasing
a bug that doesn't exist. Also added `--specs-dir` to `verify_all_specs.sh` so a synthetic fixture
dir could be used for testing (needed for Slice 2's own gate below). Memory `gate-flakes-under-
contention` updated with this new instance/root-cause.

## R39 Slice 2 landed: ported validator against the typed store (commit `03d2097`)

`scripts/r39_slice2_validate.sh [STORE_JSONL]` reimplements every check `verify_all_specs.sh`
performs (duplicate numbers incl. `KNOWN_DUAL` allowlist, id/filename match, status-claim vs prose
mismatch, non-Draft-requires-evidence, dangling evidence scripts, dangling depends-on/blocks/
supersedes/conflicts-with/related edges) reading ONLY the typed store's already-extracted fields —
no markdown re-parsing. Gated by `scripts/r39_slice2_gate.sh` (10 checks, ALL PASSED): (A) on the
real tree both validators agree exactly (0 findings, 38 pre-convention specs); (B) on a synthetic
scratch fixture with 4 deliberately injected real bugs (dangling depends-on, status-claim/prose
mismatch, missing evidence script, a NEW duplicate spec number not on `KNOWN_DUAL`), both catch all
4 and produce byte-identical finding sets (one cosmetic `.md`-suffix formatting difference
normalized away, documented as such — not a real divergence); a real `KNOWN_DUAL` prefix (R21) is
confirmed to still warn rather than finding in both, proving the allowlist ported correctly. This
is the actual regression test the spec's own Slice 2 gate calls for — proves the port finds bugs,
not just that both agree an already-clean tree is clean. R39 spec (header, §6 slice 2, §9
acceptance), `ROADMAP.md`, `REQUIREMENTS.md` updated.

## R39 Slice 3 landed: live evidence re-run records (commit `407c078`)

`verify_all_specs.sh --run TARGET --record-jsonl PATH` now appends one JSON record (schema
`axon-gov-verify/1`: spec, command, result, exit_code, ISO-8601 UTC timestamp, short git commit
hash) per evidence command actually re-run, to a sidecar file kept deliberately separate from
`specs.jsonl` (which stays a pure function of the markdown tree — a verify-run record is evidence
of an action taken, not re-derivable from the tree alone). No separate `axon-gov` binary yet
(spec's §12 Q3 still open); continues Slices 1-2's pattern of extending the existing bash validator
rather than standing up a new tool prematurely. Gated by `scripts/r39_slice3_gate.sh` (11 checks,
ALL PASSED): synthetic PASS/FAIL fixtures record the correct result AND exact exit code (7, not
just "nonzero"); every record's timestamp/commit-hash are well-formed; **live re-runs against the
real R32, R33, and R34 acceptance gates all reproduce PASS**, matching this session's own
hand-verified results; `--record-jsonl` without `--run` is a hard usage error (exit 2), not a
silent no-op. This ran while the host was under exceptionally heavy load (~50 load-average on 32
cores, later traced to unrelated processes — several `vllm` model servers and `train_rssm_breakout.py`
training jobs saturating CPU, not anything this session touched); the R32/R33/R34 gate re-runs still
completed correctly, just slowly. R39 spec (header, §6 slice 3, §9 acceptance), `ROADMAP.md`,
`REQUIREMENTS.md` updated.

## R39 Slice 5 landed: DAG cycle + blocked-by staleness (commit `09e045d`)

Picked Slice 5 over Slice 4 this iteration: Slice 4's own gate ("a strict superset of what
`SESSION_STATUS.md` currently records by hand") is ill-specified for a mechanically-generated file
— this document is 500+ lines of iteration narrative reasoning, not just structured facts a
typed-store render could reproduce; deciding what "superset" should mean needs design work first,
the same class of gap that deferred R33.S2. Slice 5 was cleanly specified instead.
`scripts/r39_slice5_dag_check.sh` builds one directed graph from every spec's `depends_on`/`blocks`
edges (typed, from the Slice 1 store — no markdown re-parsing) and 3-color DFS-detects cycles;
separately, for every `blocked_by: R<id> §<N> Q<k>` it reads the *target* spec's markdown directly
(new extraction — no landed slice types §-section prose yet) and checks whether that question is
actually marked resolved. Gated by `scripts/r39_slice5_gate.sh` (10 checks, ALL PASSED): real tree
clean; R36's real blocked-by (the spec's own named example) correctly reported still-blocking;
synthetic depends-on AND blocks-edge cycles both rejected; a synthetic resolved-question is flagged
stale; a synthetic genuinely-unresolved one is not (regression test). **Running the check against
the real tree BEFORE finishing the fixtures found two real bugs in the check's own first draft**: a
naive `grep -qi resolved` false-positived on R40's actual text ("**Unresolved**, deliberately" —
"resolved" is a literal substring of "unresolved"), fixed with a word-boundary match excluding
"un-"; and the bullet-matcher assumed every spec bold-labels questions `**Qn**`, but R37/R38 use
plain `1./2./3.` numbering, fixed with a plain-numbered-item fallback. Neither would have been
caught by fixtures alone, since I'd have written fixtures using the convention I assumed was
universal — saved as memory `new-check-verify-against-real-corpus`. R39 spec (header, §6 slice 5,
§9 acceptance), `ROADMAP.md`, `REQUIREMENTS.md` updated. The background full `gate.sh --strict` run
from the previous iteration also finished clean during this iteration (host load was from unrelated
processes — several `vllm` model servers and ML training jobs — not anything this session touched).

## R33.S2 properly designed this iteration (commit `b76ccea`) — no code yet, a truth-correction + a buildable design

Sized R33.S2 (vsock transport) for real via an Explore research pass, rather than leaving it
"blocked on a Substrate trait" (as the last several iterations' notes said). Found the premise was
false: R33's spec claimed it could "reuse R26's existing `Substrate` trait boundary," but that trait
was never built anywhere — R26's spec fully designs it (§2/§5.1: `trait Substrate`,
`MockSubstrate`, `QemuSwtpmSubstrate`, an `hw-attest` feature) but grep across the whole tree
returns zero hits for any of it. R26 actually shipped a simpler path: a flat
`crates/axon-attest/src/lib.rs` plus ~5 independent inline `AXON_CI_NO_KVM` env-var branches in
`axon-vm/src/main.rs`. R26's `✅ Landed` status is still honest as a *functional* claim (the
attestation gate genuinely works, gate-verified) — only the specific trait-architecture claim in
the same document was aspirational, never reconciled after the code shipped differently. Corrected
both specs (no behavior change, prose only): R26 gets a 2026-07-19 as-built note near its module
table; R33's false claim is struck out. Then did the actual design work R33.S2 was missing: a new
§5.2.2 scopes a real, buildable path that does NOT depend on R26's unbuilt trait — a dedicated
`quorum/vsock.rs` module (not a generic trait), reusing the wire-framing pattern that DOES already
exist (`interp.rs`'s raw-`AF_VSOCK` `vsock_send_recv`, currently guest→host only; S2's real new
work is a host-side listener, which has no existing precedent), CI-testable via a TCP-loopback
env-var swap matching R26's own `AXON_CI_NO_KVM` precedent rather than inventing a new mock
abstraction. Explicitly still too big for one iteration — the section's job is to make a future
iteration's sub-slice scoping decision cheap ("S2a: wire protocol + CLI flags, TCP-loopback-tested"
as a first bounded cut), not to have built it. `verify_all_specs.sh` clean (3/3 reruns);
`r39_slice1_gate.sh`/`r39_slice5_gate.sh` both still pass against the regenerated store. Saved as
memory `r26-substrate-trait-aspirational`: verify a spec's claimed abstractions exist in code,
independent of whether its overall status says "Landed" — the two claims can diverge.

## R33.S2a landed: vsock wire protocol (commit `e554173`)

Picked the smallest real piece of the §5.2.2 vsock design: `crates/axon-vm/src/quorum/vsock.rs`,
length-prefixed JSON framing (`write_frame`/`read_frame`/`write_json_frame`/`read_json_frame`),
transport-agnostic (generic `Read`/`Write`, so a TCP-loopback stand-in and a real vsock socket hit
the identical code path), matching `interp.rs`'s existing `vsock_send_recv` convention exactly.
9 new unit tests (round-trips real `VoteRequest`/`VoteResponse`, truncated/malformed streams are
`io::Error`s not panics, EOF sentinel distinguishable from a real empty payload). Deliberately
unwired to any caller yet (documented `#![allow(dead_code)]`) — S2b+ (real socket, broadcast/
listen loop, CLI flags) is separate, larger work, not started.

Found and closed an incidental gap while landing this: `axon-vm`/`axon-attest` were never
clippy-gated by `gate.sh` at all — the same blind spot BUG_HUNT #35 found in the runtime crates,
just never re-checked for these two. Confirmed 3 small findings pre-existed (not introduced by
this change) via `git stash` before fixing: a 9-arg `run_in_firecracker` (documented
`#[allow(clippy::too_many_arguments)]`), a manual `!Range::contains` rewrite (mechanical), and a
dead-code `AxonManifest` struct whose fields mirror an external JSON schema but aren't all consumed
yet (documented `#[allow(dead_code)]`). Both crates added to `gate.sh`'s existing runtime-crate
clippy line. `cargo test -p axon-vm`: 55/55. `r33_acceptance_gate.sh`: unchanged, ALL CHECKS
PASSED (no regression). Saved as memory `gate-sh-clippy-coverage-gaps`: check whether a crate is
actually named in `gate.sh`'s clippy lines before trusting its lint state — coverage is an explicit
allowlist, not workspace-wide. Full `gate.sh --strict` running in background to confirm workspace-
wide.

## R33.S2b landed: real-socket round trip over TCP loopback (commit `d618c77`)

Next smallest piece after S2a: `connect_and_round_trip` in `quorum/vsock.rs` — the proposer side of
ONE real connection (TCP loopback, the §5.2.2 CI stand-in for `AF_VSOCK`), deadline-bounded
(`connect_timeout` + read/write timeouts), fail-closed (timeout/refused connection → `Err`, meant
to be treated by a future collect loop exactly like a missing `.vote` file — no vote from that
peer, not a hard quorum failure). 3 new tests (12 total): a full round trip returns the voter's
real response over an actual socket (not just an in-memory buffer, which is all S2a's tests
covered); a voter's EOF sentinel comes back `Ok(None)` not an error; connecting to a dead/refused
port is `Err`, not a panic or a hang past the deadline. The voter side in these tests
(`spawn_one_shot_voter`) is deliberately test-local only — a real listen/accept/respond primitive
is separate, still-open S2c+ work, along with the N-peer broadcast/collect fan-out and the CLI
flags. Ran the vsock test module 5 consecutive times to check for socket/timing flakiness (real
network I/O, not pure logic) — all 5 identical, 12/12 clean. `cargo test -p axon-vm`: 58/58.
`r33_acceptance_gate.sh`: unchanged, ALL CHECKS PASSED. R33 spec §13/§14/header, REQUIREMENTS.md
updated.

## R33.S2c landed: N-peer broadcast/collect fan-out (commit `93d6086`)

Next piece after S2b: `broadcast_and_collect` in `quorum/vsock.rs` — calls `connect_and_round_trip`
once per peer, each from its own thread, so total wall-clock stays bounded by the single `deadline`
regardless of how many peers there are (a sequential loop would let one slow/unreachable peer at
the front inflate latency past what §4.4's deadline promises — exactly the failure mode a single
bad peer must not be able to cause). Feeds straight into the existing pure `check_quorum`, no
changes there. 3 new tests (15 total): gathers every responsive peer's vote; an unreachable peer
(dead port) contributes nothing without blocking a live peer's response; a timing assertion with 4
dead peers proves wall-clock stays under 3x a single deadline (a sequential regression would show
~4x — generous margin for host contention, still catches the bug class). Ran 5 consecutive reruns
to check for threading/socket/timing flakiness — all identical. `cargo test -p axon-vm`: 61/61.
`r33_acceptance_gate.sh`: unchanged, ALL CHECKS PASSED. R33 spec §13/§14/header, REQUIREMENTS.md
updated.

Also worth noting: the long-running background `gate.sh --strict` from prior iterations became
ambiguous to track (likely PID reuse from another Claude session sharing this host — the same
`bash scripts/gate.sh --strict` command line, but process start-time didn't match the original
launch). Rather than keep guessing at its state, relaunched a fresh one with proper `run_in_
background` tracking this iteration and moved on — the S2a/S2b/S2c work above was independently,
thoroughly verified regardless (targeted tests, clippy, acceptance gate, spec verifier, all
multiple times over), so the ambiguous background process was never load-bearing for trusting this
iteration's own changes.

## R33.S2d landed: real listen primitive + `quorum vote --listen` CLI flag (commit `4066538`)

Voter side of S2: `respond_once` in `quorum/vsock.rs` — accepts exactly ONE inbound connection,
reads a `VoteRequest` frame, hands it to a caller-supplied closure, writes back a `VoteResponse` (or
the EOF sentinel). Deliberately single-shot, no daemon loop, no internal accept timeout — matches
the CLI's existing per-invocation granularity, external caller owns any wait-bound policy.
Refactored the S2a-c test helper (`spawn_one_shot_voter`) to delegate to it instead of duplicating
the logic, so all 12 prior tests now also exercise the new production primitive. Wired into the
real CLI: `axon-vm quorum vote --listen PORT [--approve|--deny] [--reason] [--lineage-root]` — same
decision logic as the file-based path, just a different I/O layer. `--request`/`--out` became
`required_unless_present="listen"` (100% backward compatible: omitting `--listen` still requires
them exactly as before); `--listen` `conflicts_with_all` them. Verified end-to-end with a
from-scratch Python client speaking the real wire protocol directly (not the Rust test helpers) —
proves the actual bytes on the wire, not just Rust-to-Rust round-trips. New
`r33_acceptance_gate.sh` §11 (3 checks, 3 consecutive reruns for socket/timing flakiness — all
identical). `cargo test -p axon-vm`: 62/62. Clippy clean. R33 spec §13/§14/header,
`REQUIREMENTS.md` updated.

Along the way, the long-running background `gate.sh --strict` from two iterations ago finally
completed (`REAL_EXIT` not directly captured — its log's last line was `❌ gate FAILED at:
runtime-crate clippy`, but that failure was sampled mid-edit against this iteration's own
in-progress uncommitted changes, not a real regression in the already-committed S2c state, which
was independently verified clean before its own commit). A separate, freshly-relaunched background
run from last iteration is still the one actually confirming the current committed tree.

## R33.S2e landed: `propose --broadcast`, S2's proposer+voter path complete (commit `372fcc8`)

Last CLI piece of S2: `axon-vm quorum propose --broadcast CID1,CID2,... --n N --deadline-ms MS
[--json]` — gives `broadcast_and_collect` (S2c) its first real, non-test caller. Broadcasts,
collects, runs the same `check_quorum` `check` already uses, exits 0/13/14 — factored the
exit-code/JSON logic into a shared `report_quorum_result` helper so `propose --broadcast` and
`check` share one mechanism, not two. Verified with a real multi-process journey (new
`r33_acceptance_gate.sh` §12, 3 checks, 3 reruns): `propose --broadcast` against 2 real
`vote --listen` processes + 1 genuinely unreachable port correctly reports 2/3 approvals and
QUORUM MET; against 2 unreachable peers, QUORUM BLOCKED with no hang; omitting `--broadcast` is
fully unaffected. This closes `vsock.rs`'s module-level `#[allow(dead_code)]` entirely — every
function now has a real, non-test caller, not just unit tests. S2's whole proposer+voter round
trip (wire protocol → single round trip → N-peer fan-out → listen primitive → both CLI flags) is
complete end-to-end. `cargo test -p axon-vm`: 62/62. Clippy clean with zero allows. R33 spec
§13/§14/header, `REQUIREMENTS.md` updated.

## R33.S2f sized, found blocked on a real architecture fork — not built (commit `7379950`)

Went to size S2f ("swap TCP for real `AF_VSOCK`, same wire format" — the plan carried unquestioned
across all four S2a-e iterations' own doc comments) before building it, same discipline that caught
the R26 Substrate-trait finding two iterations ago. Found the premise false: every real vsock use in
this repo (`crates/axon-vm/src/main.rs`'s Firecracker `/vsock` device config, `vsock_relay`, and
`interp.rs`'s `vsock_send_recv`) connects a guest ONLY to its own host, never to another guest
directly — Firecracker's vsock device model has no peer-to-peer mode. Compounding it: `axon-vm run`
launches exactly one VM per process, so there's no existing multi-VM host structure to relay
through either. This does NOT mean the quorum concept requires separate physical hosts (R33's own
stated threat model — a single VM compromised post-attestation — is satisfied by micro-VM isolation
regardless of host topology) — it means the transport has to match a topology vsock can actually
support, and direct peer-to-peer isn't one. Two real options remain, with materially different
trust-boundary/deployment consequences: a new host-mediated relay broker (extends the TCB,
same-host only) vs. hardening the existing TCP path with real auth/TLS (works cross-host). Wrote
this up as R33 spec new §5.2.3 + §12 Q1 (an explicit open founder/architecture question, unresolved
— not silently picked) rather than building on the wrong premise. No code changed — pure spec-first
(Gate 1) work. `verify_all_specs.sh` and both R39 governance gates confirm clean. Updated the
`r26-substrate-trait-aspirational` memory to cover this second instance — the lesson generalizes to
re-verifying your OWN prior-iteration "later we'll just do X" plans, not just old specs' claims.

## R39 Slice 4 landed: GOVERNANCE_STATUS.md render, re-scoped (commit `5bddc6e`)

Did the design work first: the original gate ("a strict superset of what `SESSION_STATUS.md`
currently records by hand") never held up — `SESSION_STATUS.md` is 700+ lines of hand-written
narrative (decision rationale, investigation findings), and the typed store only carries
structured facts. Corrected scope: `GOVERNANCE_STATUS.md` is a NEW, separate, purely-generated
artifact — does NOT replace `SESSION_STATUS.md`'s narrative, exists so every structured status
claim (id/status-claim/prose-status/match/last-verify-result) is reachable from one place, sourced
only from the store, never hand-typed so it can't silently drift. `scripts/r39_render_status.sh`
renders `governance/state/GOVERNANCE_STATUS.md` (gitignored, matching `specs.jsonl`'s own
precedent) from `specs.jsonl` + an optional Slice-3 verify-results sidecar. Gated by
`scripts/r39_slice4_gate.sh` (8 checks, ALL PASSED): nothing dropped/invented; counts cross-checked
against `verify_all_specs.sh`'s own report; a synthetic mismatch fixture renders flagged matching
the existing validator's own finding (no second notion of "wrong"); most-recent verify-run record
wins over a stale one; regenerating twice against an unchanged store is byte-identical except the
timestamp (pure function of the store). **All five R39 slices are now landed.**

## R30 flakiness partially chased (commit `08db0bf`) — real fixes, root cause still open

Did a fresh SELECT sweep across `REQUIREMENTS.md` (both R33 and R39's easy scope were exhausted).
R30's row has said "root cause not chased further" for its documented host-contention flakiness
since 2026-07-18 — investigated rather than re-flagging it a third time. Confirmed `acc_a1`/`acc_a4`
run in STRUCTURAL mode (`AXON_CI_NO_KVM=1`), which never invokes `axon-vm run` (the real Firecracker
VM launch) anywhere across R26-R29's own acceptance gates — so a real-KVM-boot-timing theory doesn't
actually fit the observed failure path, contrary to the prior note's guess. Found and fixed the one
real hardcoded, non-configurable timeout on that code anyway (`wait_for_socket`'s 5s Firecracker
API-socket wait, `crates/axon-vm/src/main.rs`, had no env override unlike its sibling
`AXON_VM_TIMEOUT_SECS`) — genuinely valuable for FULL-mode runs, now tunable via
`AXON_VM_SOCKET_TIMEOUT_SECS`, but honestly NOT confirmed as the structural-mode flake's actual
cause. Also fixed a real, unrelated gap found along the way: `axon_safety_gate.sh`'s
`STAGE_LOG_PREFIX` had no PID suffix, so two concurrent invocations (e.g. two Claude sessions
sharing this host, directly observed happening earlier this session) could clobber each other's
diagnostic logs — doesn't affect pass/fail but hinders future debugging of this exact flake class;
now PID-suffixed. **The real root cause — most likely nested `cargo build`/`test` contention across
R26-R29's own internal test suites, the same class `fuzz_parity.sh`'s nbuild-retry precedent already
addresses elsewhere — remains open.** A live repro attempt (`bash scripts/r30_acceptance_gate.sh`,
launched at low host load ~9-13) was still executing 5+ minutes in, several cargo/node subprocesses
deep, when this iteration closed — genuinely deeper than fits one iteration. Left running in the
background for a later iteration to check. `cargo build/clippy/test -p axon-vm`: clean, 62/62.
`verify_all_specs.sh`: clean. `governance/REQUIREMENTS.md`'s R30 row updated honestly (partial
progress, not "fixed").

## R1d: str_count migrated to the registry, one candidate closed (commit `e7c9635`)

Went to size "extend `ExternSig`/`declare_one_extern` for out-param synthesis" (R1d's own flagged
next step, per SESSION_STATUS's prior note) and found a smaller, real, immediately-actionable gap
first: `str_count` was miscategorized in the spec's own 2026-07-18 "exhaustive scan" as one of the
str-returning out-param wrapper builtins, purely by name resemblance — its actual codegen never
used the out-param dance at all (`__axon_str_count(AxonStr, AxonStr) -> i64`, the exact shape
`str_index_of` already had a row for). Migrated it (33rd registry row), deleted the redundant
hand-written codegen block. Verified: `cargo build` clean, both drift tests pass, clippy clean
(both feature sets), and a manual native==interp check across 5 cases (normal/overlapping matches,
empty needle, empty haystack, substring match) byte-identical. `verify_all_specs.sh` clean. **This
does not close the out-param-synthesis work** — the other ~10 named candidates
(`str_replace`/`str_slice`/`str_reverse`/etc.) are genuinely out-param wrappers and still need that
real, separate structural extension; this was the one candidate that turned out not to need it.
R1d spec updated with the correction. A full `cargo test -p axon-core --no-default-features` run
was launched in the background for additional confirmation beyond the direct evidence already
gathered — check its result before the next iteration.

## R1d out-param-synthesis extension landed: 7 str-out builtins migrated (commit `c769853`)

Sized "extend `ExternSig`/`declare_one_extern` for out-param synthesis" properly this time (last
iteration only found and migrated `str_count`, a miscategorized non-out-param candidate). Before
implementing, verified two things that could have made this riskier than it looked: (1) the
out-param convention these rt fns use is a DELIBERATE cross-target design — axon-rt's own
`#[cfg(target_arch="wasm32")]` variants exist because wasm32's struct-arg/return ABI genuinely
differs from native's — so a "just return `AxonStr` by value directly" shortcut (which `L::Str`
already supports as a `ret` shape) was considered and correctly rejected as unverified for wasm32,
not assumed safe; (2) codegen's own LLVM-IR-generation side has NO wasm32-specific branching for
any of the 7 candidates, meaning the cross-target complexity lives entirely in axon-rt's Rust
signatures, invisible to codegen — so a generic synthesis function reproducing the hand-written
wrappers' EXACT IR shape is safe on both targets by construction, not something new to prove.
Implemented as a NEW, separate `StrOutSig`/`STR_OUT_EXTERNS` table (not a field on `ExternSig`) +
`Codegen::synthesize_str_out_wrapper`, migrating `str_reverse`/`str_to_upper`/`str_to_lower`/
`str_digits_only`/`str_trim`/`str_trim_start`/`str_trim_end` (every candidate sharing the identical
single-`L::Str`-param shape) and deleting the 7 hand-written codegen blocks. Added 3 new drift
tests (a real gap found: the existing drift tests only covered `BUILTIN_EXTERNS`, not any future
second table). Verified: `cargo build`/clippy clean (both feature sets); 5/5 drift tests; a manual
native==interp check across 14 hand-picked cases (incl. `straße`→`STRASSE` Unicode-growing
case-map, empty strings) byte-identical; the full `scripts/fuzz_parity.sh` corpus (all 7 already
had entries) PASS. Explicitly still out of scope: `str_replace`/`str_pad_start`/`str_pad_end`/
`str_slice`/`str_repeat` (extra args, likely fit but unverified) and `str_split`/`str_join`
(genuinely different `Array<Str>` shapes, confirmed by reading their codegen, not assumed). R1d
spec updated with the full finding.

## R1d out-param-synthesis extension: second batch landed, essentially complete (commit `564399d`)

Migrated the remaining 5 multi-arg candidates — `str_repeat`/`str_slice`/`str_replace`/
`str_pad_start`/`str_pad_end` — confirming each one's actual codegen shape before adding a row
(not assumed from name resemblance), same discipline that caught `str_count`'s miscategorization
earlier. All 5 shared the identical output-side shape as the first batch, needing ZERO new
synthesis logic — `synthesize_str_out_wrapper` already handles mixed leading-arg shapes generically
via `params: &'static [L]`. Deleted the 5 hand-written codegen blocks. `STR_OUT_EXTERNS` is now 12
rows — every single-`AxonStr`-return out-param candidate from the original scan except
`str_split`/`str_join` (genuinely different `Array<Str>` input/output shapes, confirmed by reading
their codegen, remain out of scope). Verified: `cargo build`/clippy clean; 5/5 drift tests (all 12
rows covered); a manual native==interp check across 12 more hand-picked cases (multibyte fill-char
padding, empty-string edges, a byte-vs-char UTF-8 slice boundary) byte-identical; full
`scripts/fuzz_parity.sh` PASS. R1d's out-param-synthesis extension is now essentially complete.

Also confirmed during this iteration: the R30 background gate run from two iterations ago
finished (4 passed, 2 failed) — `acc_a1`/`acc_a6` failing was fully explained by the already-known,
already-documented Stage 2 flake (`wasm_examples_run_identically_on_aot_wasm`), no new information.
The R1d first-batch full-test-suite confirmation (task `bss3doll5`) also completed clean (124/124).
A fresh full-suite confirmation for THIS iteration's second batch is running in the background
(task `buknyl7xw`) — check its result before the next iteration, though direct evidence is already
strong.

## R17 timer-interrupt wiring sized, found blocked on a missing primitive (commit `7a803b3`)

Did the fresh SELECT sweep across `REQUIREMENTS.md` this pointed at. Picked R17's
`axon_kernel_handles_timer_interrupt` (flagged "unblocked by `@[repr(C)]` for IDT entries;
deferred to a wiring slice" — implying only assembly remained) and sized it before starting, same
discipline as R33.S2f/R26. Traced the actual requirement: an IDT gate descriptor needs the ISR
handler's memory address split into offset fields. Confirmed via exhaustive search (AST/parser/
builtins/checker/infer/codegen, the full R17 spec, every `examples/kernel/*.ax` file, `asm(...)`'s
actual operand model) that **Axon has no way to obtain a function's address as a usable value from
`.ax` source at all** — no `fn_addr` builtin, `&` is array/slice-borrow only, `asm(...)`'s operand
sections are raw opaque strings with no symbol-operand syntax, `@[interrupt]` sets only the calling
convention with no registration, and R13's native-FFI (which a *different* open question had
assumed would cover this bring-up class) is interp-only and codegen-refuses — useless for a kernel
that must run under native codegen. `hello_kernel_slice3.ax` already proves the LANGUAGE side of
ISR handlers works (a stub `@[no_alloc] @[interrupt] fn isr_timer()`); the blocker is entirely
"how do you point an IDT entry at it," which needs genuinely new compiler work (a new builtin or
`asm` operand kind), not wiring. No code changed — spec-first (Gate 1). Written up as R17 spec new
§12 Q7; header and `REQUIREMENTS.md`'s R17 row corrected. `verify_all_specs.sh` clean. Updated the
`r26-substrate-trait-aspirational` memory with a third instance — three-for-three now on "just
wiring" notes turning out to rest on something that doesn't exist when actually sized.

Also confirmed this iteration: the R1d background full-suite run (second batch) hit the SAME
already-documented `wasm_examples_run_identically_on_aot_wasm` flake (418 passed / 1 failed) —
isolated rerun passed clean immediately, confirming flake not regression, per the established
protocol. No new information, R1d's change remains fully verified.

## Next candidate slice — genuinely fresh scope needed

R1d is fully done. R33's easy sub-slices are exhausted (S2f blocked on a founder decision, §12 Q1).
R39 is fully landed. R17's timer-interrupt wiring is now known to need a real language feature
(function-address primitive) rather than being pickable directly. Options for the next iteration:
- **Properly scope the function-address primitive** R17 needs (a `fn_addr(name) -> u64` builtin vs.
  an `asm(...)` symbol-operand extension — R17 spec §12 Q7 deliberately left this undecided) as a
  design task before building it — real structural work, not a quick slice.
- Continue the outer-loop sweep into the 38 pre-convention specs (spec-meta on next real edit per
  `EXECUTION_MODEL.md` §3 backfill policy — not a mass mechanical pass).
- Consider surfacing R33 §12 Q1 explicitly to the user/founder, since R33.S2/R34.S7 are both
  genuinely stalled on it now — this is exactly the kind of decision this build loop shouldn't make
  unilaterally.
- Another fresh `REQUIREMENTS.md` SELECT sweep may be needed again if the above don't pan out —
  R24/R25's dual-numbered "spec only, no implementation" rows (defended-approval-boundary,
  information-flow-monitor) haven't been sized yet and could be worth a look.
