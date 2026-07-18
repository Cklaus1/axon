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

**Caveat for reproducibility:** the TLC jar and Coq installation are host-level state, not
committed to the repo (can't be — `tla2tools.jar` is a 4.5MB binary, `apt install coq` is a system
package). A fresh clone/CI run without these installed will see R32 fall back to 18/20 + 2 SKIPPED
again until someone repeats this setup. Worth a future slice: document this setup step somewhere
CI-visible (`ENVIRONMENTS.md`?) so it isn't a one-off tribal-knowledge install.

## Next candidate slice

- Document the TLC/Coq setup step in `ENVIRONMENTS.md` so R32's gate stays reproducible off this
  one host, or continue the outer-loop sweep into the 53 pre-convention specs, or pick the next
  highest (load-bearing × cheap-to-close) `REQUIREMENTS.md` row.
