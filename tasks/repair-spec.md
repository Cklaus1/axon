# S1: one governed repair episode (R1-R8)

Source: the user's go-ahead (2026-10-17) on "build one real, governed self-repair episode end to end, using the
parts that already exist". Base: `main` f83c6f86, branch `repair/int`, worktree `/home/cklaus/projects/axon-lanes/rep-int`.
Seams were mapped by two read-only scouts before this spec was written (`agent://SeamCortexRepair`,
`agent://SeamAuthorityLoop`); file:line anchors below are from that pass at f83c6f86.

Why this slice: the user's ASI OS v0.3.0 pack (`build/FIRST_TASK.md`) and Cortex v0.23 (B291, B300) both name "one real
local test-repair path through existing owners" as the first build, with optimization record-only. Measured: the plain
micode agent matches or beats Cortex (held-out 47/47 vs 41/47, p=0.03; every miss a fix-writing failure), Cortex's
honesty edge is the hidden check, and diagnostics reach the generator as text capped at 2 KB.

Acceptance proofs (the pack's, on real code): (P1) a good candidate passes; (P2) a bad candidate fails; (P3) an
unauthorized capability is blocked before dispatch; (P4) the accepted artifact's digest equals the acceptance-evidence
digest; (P5) budget or deadline exhaustion → no new model calls.

Shared acceptance for every item: `scripts/gate.sh` shows no new failure against the baseline;
`python3 -B scripts/v022_refusal_coverage.py` and `scripts/v022_g01_mutations.py` stay green, every new refusal site
carrying a killed mutation row or a reasoned exemption and every moved anchor re-pinned; `cargo fmt --all --check`;
`python3 -B scripts/pci_delta.py --check` (pci theme per `crates/*/src` commit); `AXON-COMPLETENESS.json` rows corrected
and `.md` regenerated (integrator, last); `CHANGELOG.md` section (integrator).

## Defect that makes R1 P0 (reproduced 2026-10-17, f83c6f86)

`cortex repair --workspace W --file broken.ax --symbol double --check hidden_completion --write-prefix broken.ax
--candidates 1 --budget 4 --generator 'literal:\n    let _ = write_file("/var/tmp/r51-tmp/PWNED", "…")\n    n * 2\n'`
→ exit 0 `verified_done`, the patch is KEPT, and `/var/tmp/r51-tmp/PWNED` exists (33 bytes). A model-proposed candidate
runs with the operator's full host authority during its checks; the pristine `double` declares no effect, nothing
objects. `registered_runner` (`axon-cortex/src/bin/cortex.rs:287-324`) sets none of `with_effect_ceiling`,
`with_timeout`, `with_max_output`, `with_clean_env`.

## R1 — Candidates cannot exceed the authority the original file holds (P0, security; critical path)

- **R1a — episode grant.** Before the walk, cortex computes the episode grant once: the pristine target file's declared
  effects (`axon_os::runtime::scan_effects`, the same deny-by-default scan `supervise` uses) intersected with the
  operator's optional `--effect-ceiling <list>` (narrows only). It is the least grant in the sense of invariant I-12: a
  repair may not acquire a capability the code did not already hold. Reuse `axon_intent::infer_grant` if its
  `(DeclaredEffects, CapCeiling, Budget)` shape fits without a fake ceiling; otherwise intersect via `axon_os` types.
  The grant (canonical, `axon_os::canonical_grant`) and its digest go into the episode as a new event before the first
  proposal, and into `cortex-repair/1` as `grant`.
- **R1b — admission before dispatch.** After a candidate body is patched and BEFORE any check or verify spawns for it,
  cortex scans the patched file; effects ⊄ grant (`axon_os::admit`) → the patch is reverted, an `ActionDenied{axis}`
  event is recorded, the refusal is fed back to the generator as a rejected attempt (reason `capability`, detail names
  the axis), and the attempt counts against the proposal budget. No `axon test` / `axon check` of the candidate runs.
- **R1c — runtime ceiling (defense in depth).** Every check spawn runs with `AXON_ALLOWED_EFFECTS` = the grant
  (`LocalInterpreterExecutor::with_effect_ceiling`) and `with_clean_env`. The textual scan can be laundered; the
  interpreter ceiling is the second fence. An empty grant is `AXON_ALLOWED_EFFECTS=` (deny all), not unset.
- **R1d — undeclarable pristine.** If the pristine scan is `DeclaredEffects::unknown()` (e.g. the file has a `mod`),
  the grant is maximal unless `--effect-ceiling` narrows it; the episode records `grant_basis: "unknown"` and stderr says
  so once. No silent containment claim.
- Tests (fail-first): the PWNED reproduction as a `tests/cli.rs` row — today exit 0 and the file exists; after: the file
  never exists, the episode has `action_denied` with axis `fs_write`, exit ≠ 0. A control: a correct candidate on the
  same fixture still exits 0 (P1). A laundering row: a body that reaches an effect the scan misses (if one exists) is
  stopped by the runtime ceiling — or, if none can be found, the row asserts the ceiling env reaches the check.
  `--effect-ceiling` narrowing row. Fabric executor path unchanged (its own grant registry), regression only.
- Cargo: `axon-cortex` gains path deps `axon-os` (and `axon-intent` if used). No cycle: neither depends on cortex.

## R2 — Every candidate execution is bounded and killed as a group (P0, containment)

- `registered_runner` sets `with_timeout` (new `--check-timeout-ms`, default 60000), `with_max_output` (1 MiB, the
  generator's cap), `with_clean_env`.
- `run_limited` (`runner.rs:3019-3122`) spawns through `axon_jobctl::Job` (own process group, `killpg` at the deadline,
  bounded pipe drains), replacing `child.kill()` on the direct child.
- The unbounded `axon check` spawns in `observe` (~457), `checker_exit_clean` (~1704) and `visible_run`'s compile path
  (~1738) go through the same bounded path.
- A timed-out candidate check is a rejected attempt with `timed_out` feedback (not an episode-ending Blocked), so the
  walk continues; a timed-out PRISTINE pre-flight stays exit 22.
- Tests (fail-first): a candidate body that never returns → today the CLI hangs (test's outer guard fires); after: the
  attempt is rejected within timeout+grace and the next proposal runs. Group kill: a fixture whose pristine declares
  `exec` so R1 admits a body that backgrounds a grandchild holding the pipes; after the timeout no grandchild survives
  and the loop is not stalled (pattern: `kg2_a_leftover_holding_the_generators_pipes_does_not_stall_the_loop`).

## R3 — Episode budget and deadline: exhaustion means no new model calls (P0, P5)

- New `--max-proposals N` (total generator calls across the whole walk, all candidates) and `--deadline-ms N` (episode
  wall clock from the first proposal). Existing `--budget` (steps per candidate) unchanged.
- On exhaustion: no further `gen.propose`; an in-flight generator job is killed at the deadline (jobctl); outcome
  `budget_exhausted`, exit 20, `detail` = `proposals` | `deadline` | `steps`; `cortex-repair/1` gains
  `budget:{max_proposals, deadline_ms, proposals_used, elapsed_ms, exhausted_by}`.
- Tests (fail-first): `cmd:` generator that logs each call and always answers wrong → with `--max-proposals 2
  --candidates 3` exactly 2 lines in `calls.log`, exit 20 `proposals`. Slow generator with `--deadline-ms` → exit 20
  `deadline`, no call starts after the deadline, the in-flight generator's process group is gone. R1-refused proposals
  count.

## R4 — Diagnostics reach the generator as structured data (P1, capability)

- `RejectedAttempt.feedback: Option<String>` (`generate.rs:144-153`) becomes a typed `Feedback { diagnostics:
  Vec<Diag{file,line,col,code,severity,message,help,expected,found}>, failed_tests: Vec<{name,message}>, refusal:
  Option<{axis,detail}>, timed_out: bool, note: Option<String> }`, parsed from `axon check --json` / `axon test --json`
  (`axon-diag/1`, `axon-core/src/lib.rs:282-345`) instead of `render_diagnostics` prose (`runner.rs:2165-2204`, which
  drops `schema,file,col,expected,found`).
- `build_prompt` (`generate.rs:264-361`) renders it as one compact JSON block per rejected attempt; caps move from one
  2 KB blob to per-field caps (message/help 300 B, ≤8 diagnostics, ≤8 failed tests) with an explicit `truncated` count.
- Grader secrecy unchanged: the hidden check's name, body and messages never reach the prompt
  (`cxg_c14_a_rejected_attempt_is_fed_back_without_leaking_the_grader` and `tests/cli.rs:1357` stay green, plus a new
  row asserting no hidden-check text in the JSON block).
- The `ai:` in-process generator and `cmd:` generators get the same prompt. The Fabric executor's empty `messages`
  (`runner.rs:~3525`) is unchanged (opportunity).
- Tests (fail-first): a `cmd:` generator that copies its stdin to a file; a first wrong candidate that type-errors →
  the second prompt contains a JSON block with the diagnostic's `col`, `expected` and `found` — red today.

## R5 — Accepted artifact digest equals the acceptance evidence (P1, P4)

- The `Verified` event gains `graded_sha256`: sha256 of the exact bytes the hidden check graded — digest taken before
  the check spawns and re-taken after it returns; a difference fails closed (`passed:false`, detail "workspace changed
  during the check").
- `cortex-repair/1` gains `accepted: {path, sha256}` on exit 0 and 27 (patch kept), `null` otherwise.
- Invariant, tested: exit 0 ⇒ `accepted.sha256 == graded_sha256 of the passing Verified event == sha256(file on disk
  after exit)`; the existing reader-verification row (`cli_ships_evidence_a_reader_can_verify`, `tests/cli.rs:1848`)
  extends to it. `run_ab.py` `grade()` asserts its `final_sha` equals `accepted.sha256` and marks the row
  `evidence_mismatch` (counted as not verified) otherwise.
- Tests (fail-first): field absent today → red. A `CheckExecutor` test double that edits the file mid-check → verified
  false.

## R6 — Record-only trial ledger in axon-loop (P2)

- `axon-cortex` cannot link `axon-loop` (axon-loop → axon-loop-contracts → axon-cortex). The caller is the benchmark
  workflow: `run_ab.py --loop-store DIR` records every arm-B trial into an axon-loop store through `intake episode`,
  reusing `promote.py`'s `Evidence.trial` builder (`promote.py:390-486`): the episode's `output_workspace_ref` and the
  check request/receipt's workspace refs are the accepted artifact's `acf1:` digest; no `evl`, `admit` or pointer
  transition (record-only, ADR-007 of the pack).
- Honest scope: the verifier attestation is signed by the harness's own key, so this is a tamper-evident ledger of
  trials for a future learner, not independent verification. It must say so in its README section.
- Tests: `promote.py check`-style offline test: a recorded trial round-trips; a trial whose artifact digest differs
  between episode and receipt is refused by intake (exit 4) — through the real `axon-loop` binary.

## R7 — Pre-registered evaluation (P1)

- **R7a regression (paid, small):** `scorecard.py regress` on the receipt corpus (115 tasks) with the S1 build. Rule
  (existing PREREG-regress): not worse than 108/115 verified, 0 false successes. Plus an S1-specific count: episodes
  where R1 denied a candidate; any task verified at baseline and not verified under S1 because of a denial is a
  regression to investigate before merge.
- **R7b fresh held-out (paid):** author `heldout2` with the existing builders from source files no other set uses, freeze
  sha256s, and commit `PREREG-heldout2.md` (arms, n, rule, analysis) BEFORE any S1 candidate runs on it. Arms:
  incumbent (cortex at f83c6f86) vs S1, same model (Haiku 5.5 via micode), paired. Analysis: verified count, false
  successes, McNemar exact on discordant pairs. Verdict PASS / FAIL / INCONCLUSIVE; "no improvement claim is forced";
  if both arms ≥ 95 % verified the verdict is INCONCLUSIVE (no headroom).
- `hard-once` is NOT spent (needs-human NH-R2).

## R8 — Docs and tracker (P2, integrator)

`crates/axon-cortex/README.md` (+ USAGE text in `bin/cortex.rs`): grant, ceiling, timeouts, budget, `accepted`;
CHANGELOG; tracker rows (`cortex repair` loop, `crate axon-loop` caller, `crate axon-cortex`/repair containment);
opportunities.

## needs-human (excluded from the autonomous run)

- **NH-R1 episode approval token.** For unattended episodes, an operator would approve an envelope (task, write
  prefixes, grant, check digest, budget) with a keyed token that a supervisor verifies. Per-candidate `axon-approval/3`
  tokens are the wrong shape: they bind entry bytes, so each patch would need a fresh approval, and an automatic one is
  the machine approving itself. Recommendation: build after S1, as an `axon-approval` sibling schema reusing the G5 key
  derivation. Until then the operator's command line is the authority, and deploying the accepted artifact uses the
  existing human `axon ast approve` / `axon-intent approve`.
- **NH-R2 spend `hard-once` (n=19) on S1.** It can be scored once per candidate and is the final judge. Recommendation:
  keep it for a candidate that has passed `heldout2`.

## Out of scope / open

Learned repair policy; Forge SIR / AIR / IntentIR types; keyed sealing of the cortex episode (`axrmac1`); USD metering
inside the loop for `cmd:` generators (harness `--cap-usd` stays); Fabric executor feedback; a richer edit vocabulary
than whole-body replacement.
