# v0.20 donor → v0.22 programme: port ledger

The v0.20 upgrade (`upgrade/cortex-v0_20`, `/home/cklaus/projects/axon`, final
`5891b8d`, tag `release-verified/5891b8d`) is **superseded**: the v0.22 package
contains the whole v0.20 package, and v0.22 is the programme line
(`governance/AUTONOMOUS-BUILD.md`). The donor was **not merged**. Proven fixes were
ported one at a time onto `v022/stage2-port` (base `v022/stage2` @ `262c44a`), and
nothing on this line was renumbered.

**ID rule.** Where a donor ID already names something else on v0.22, the donor
record gets a NEW v0.22 ID with `ported_from: <ID>@upgrade/cortex-v0_20`. Old
references stay resolvable: donor IDs resolve on the donor branch, v0.22 IDs on
this one.

## Commits

| v0.22 commit | donor commit(s) | what | port kind |
|---|---|---|---|
| `10aec998` | `8953b8b` | `-O2` on both `wasm-opt --asyncify` sites — **the incident's root-cause fix** | cherry-pick, clean |
| `7a017814` | `60aef9f` | borrowed-source ABI (`axon_eval_borrowed` + `axon_free`); latent UB, not the cause | cherry-pick, clean |
| `79ce9ab7` | `dc738cf` | `lib_bounded_run.sh` (cgroup memory + swap + deadline + descendants; RESOURCE_EXHAUSTED from `memory.events`) + 8-check regression | cherry-pick, clean |
| `55f70aa1` | `9e8e4da` | `scripts/lib/child_exit.sh`; 9 parity harnesses + live QEMU exit check | cherry-pick, clean |
| `9c5912e0` | `ebf389e` | a present-but-failing wasm build / `wasm-opt` is FAIL, not SKIP | cherry-pick, clean |
| `52e9e134` | `44a3e96` | browser page refuses an unstamped / stale / non-O2 Asyncify artifact | cherry-pick, clean |
| `d4859f1d` | `900fdc2` | managed-run receipts tally only cargo `test result:` lines | cherry-pick, clean |
| `6de01247` | `605d8b9` | `ai_extract_uncertain_*` stamped AI-sourced (source_tag 1) | cherry-pick, clean |
| `624dec42` | `3827d2f` | reflex: strict response parse; refuse a request with no `req_id` | cherry-pick, clean (v0.22 only added `pub mod shortlist`) |
| `ea9ba38d` | `dd2ccf0` + `3afc099` | derived Uncertain keeps the least-trusted tag, both engines, via `build_wrappers` (R1e) | code hunks only (the donor commit also edited donor-only docs) |
| this commit | `7ac00f3` (test part), `5891b8d` (R15 part), `d2c98fb`/`f982c16`/`5891b8d` (incident) | D-001 `""`-deny-all pin; R15 binaryen reconciliation; incident record with v0.22 IDs; ledgers | semantic |

**Not ported, and why.**

| donor commit | reason |
|---|---|
| `7fce4d2`, `90c4c61` | v0.20 package vendoring + its package-gate section. Superseded by the v0.22 package gate (Stage 6 / B283–B284). The `90c4c61` hardening (never run package code after an integrity failure; clear stale validator reports; refuse symlinks; `--ref*` self-grep) **applies to the v0.22 package gate too**: tracked as a Stage-6 input, not dropped |
| `81ab860`, `432d858`, `4b97b71`, `313f971` | v0.20 upgrade record and matrix. Superseded by `.axon-v022/UPGRADE_V0_22.md` |
| `a25c581` | `CLAUDE.md` Design Reference path. Stage-1 records the same failure (`claude_md_claims_are_true`); ported separately as its own commit |
| `cb9bc1b`, `0b664bd`, `376a7e7` | donor completeness entries. Re-issued here under new IDs (below), not cherry-picked |

## False greens (`AXON-COMPLETENESS.json`)

| donor | v0.22 | v0.22 commit | subject |
|---|---|---|---|
| FG-041@upgrade/cortex-v0_20 | **FG-044** | `d4859f1d` | receipt tallied non-cargo "N passed" text |
| FG-042@upgrade/cortex-v0_20 | **FG-045** | `55f70aa1` | parity passed a child killed after printing |
| FG-043@upgrade/cortex-v0_20 | **FG-046** | `9c5912e0` | asyncify harness reported a failed `wasm-opt` as a skip |

v0.22's own FG-041 (kernel allow path vacuous) and FG-042 (Fabric `qualification()`
accepts unsigned PASS_WITH_BLOCKED) are unchanged and still **open** (Stage 3).

## Discrepancies (`governance/cortex-v015/DISCREPANCIES.md`)

| donor | v0.22 | state |
|---|---|---|
| D-014@upgrade/cortex-v0_20 | **D-022** | fixed (`6de01247`, `ea9ba38d`) |
| D-015@upgrade/cortex-v0_20 | **D-023** | fixed (`624dec42`) |
| D-016…D-020@upgrade/cortex-v0_20 | **D-024…D-028** | open, unchanged by porting |
| D-021@upgrade/cortex-v0_20 | — | package-internal naming (v0.20 PROTOCOLS); re-check against v0.22 at Stage 6 |
| D-001 (shared, pre-existing) | D-001 | still stale in the v0.22 package; repo behaviour pinned by test |
