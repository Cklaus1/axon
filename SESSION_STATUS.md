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

## Next candidate slice

- R1d Slice 2 (migrate more inline-IR builtins into the `BUILTIN_EXTERNS` registry) — real feature
  work, higher risk (touches codegen internals), pick ONE simple candidate rather than a batch.
- Or: continue the outer-loop sweep into the 38 pre-convention specs (spec-meta on next real edit
  per `EXECUTION_MODEL.md` §3 backfill policy — not a mass mechanical pass), or pick a fresh
  `REQUIREMENTS.md` row.
- Or (bigger, separate item): harden R30's gate against contention-flakiness if it starts blocking
  real deploys.
