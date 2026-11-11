# release-contract - spec (RC-1..RC-6)

**Status: PARTIAL 2026-11-11 on rc/l a7074c4e (RC-1 98fda905; RC-2 2fb3649c, PCI note a7074c4e; RC-3 0901465b; RC-5 0012dff6;
RC-4 664f8f1c, 70f7e900; g01 M5260-M5268 killed; `gate.sh --lane` 458 units passed at a7074c4e). RC-6 is not built (NH-1 (a): it waits on the 10-green streak), so O-CI-01 stays open. Not yet
observed: the `release-dry/<sha8>` run (the integrator's push) and the first tag (NH-4 (a), the human's go).** AUTHORIZED 2026-11-11
for RC-1..RC-5 (after Step 1 review: AuditRelease 13, DevloopRelease 7 findings, all accepted). Build authorized by the human
2026-11-11; NH-1..NH-4 decided the same day (§8). RC-6 waits on the 10-green streak (NH-1 (a)); no tag is published without the
human's go (NH-4 (a)).

Author: `ReleaseContract` (2026-11-11), at main f14bc25f in worktree `specs-next2`. Scope approved by the human 2026-11-11 ("do it",
item #3: a release and stability contract). Probes: `/var/tmp/r51-tmp/ReleaseContract/`, reference binary
`/var/tmp/r51-tmp/sn2-bin/axon` (`axon 0.1.0 (f14bc25f)`, interpreter build). No cargo build, no test run, no model call, no repo
setting changed. Every "after" in this spec is the design's prediction.

### Step 1 review

Reviews: `agent://AuditRelease` (RCA-1..13, conformance) and `agent://DevloopRelease` (RCD-1..7, dev loop). Before accepting a
finding I checked it: re-read every cited line at f14bc25f and re-ran the probes it rests on (§9, rows `r1`-`r6`). **Accepted 20,
rejected 0** (RCA-2 and RCD-1/RCD-2 overlap, as do RCA-8 and RCD-6; each pair is fixed once).

| finding | sev | disposition | where |
|---|---|---|---|
| RCA-1: `gh api … -f head_sha=…` is a POST (`gh api --help`: "`POST` if any parameters were added"); the CI-green check never ran | blocker | accepted; observed: the `-f` form returns HTTP 404, the `-X GET …?head_sha=…&event=push&branch=main&status=success` form returns `total_count` 1 for 0b1183d7 (§9 r1) | RC-4 step 2 |
| RCA-2 / RCD-1 / RCD-2: the dry run cannot be dispatched from a branch before `release.yml` is on `main` [INFERENCE: GitHub docs, not observed here], and its ancestry and CI checks fail on any non-`main` sha | blocker | accepted: a `release-dry/**` push trigger (always a dry run; a push runs the workflow file of the pushed commit) plus `workflow_dispatch` once on `main`; in a dry run the ancestry and CI checks print `SKIPPED (dry run)` | RC-4 steps 1-2, acceptance |
| RCA-3: RC-3 rule 4 cannot tell inheriting crates from the twelve self-versioned ones | major | accepted: rule 4 reads `[workspace] members` and each member manifest under `--root`; only `version.workspace = true` members are held to the release version; control added | RC-3 |
| RCA-4: rewriting the last line's sha in `card-versions.txt` passes RC-2's test with no bump | major | accepted: release headings carry `(card N, <sha8>)`; RC-3 rule 3 refuses a heading whose sha8 is not line N's prefix; test + M5267. Between releases the in-place rewrite of an unreleased line stays legal (nothing shipped it) | RC-1 rule 6, RC-2, RC-3, §4 |
| RCA-5: TP-1 (`CHANGELOG.md:20`) is tagged, not an untagged fix | minor | accepted; the untagged example is TP-3's "`b + 1` is E0102 instead of a run panic" (`:22`) | §0, D2 |
| RCA-6: a `Migration:` note exists (`:182`, `:187`) | minor | accepted | §0 |
| RCA-7: the marker has suffix variants, no bold plural; `:541` "Behaviour changes:" is unbolded | minor | accepted (19 lines carry `**Behaviour change`, 13 the exact form); the bold prefix with any suffix is the marker, and `spec/stability.md` requires the bold form | RC-1 rule 2, RC-3 rule 2 |
| RCA-8 / RCD-6: the planted `AXON_GIT_SHA` either is cleared by the script or makes step 3 a no-op | minor | accepted: `release_package.sh` refuses (rc 1, before building) when `AXON_GIT_SHA` is set, unless `--keep-env`; the plant is the offline `--keep-env` run; `release.yml` never passes it | RC-4 |
| RCA-9: E0901's search uses `$HOME/.axon/lib` and `<bin>/../lib/axon`, so the exit code depends on the host | minor | accepted; observed: `bandit_ucb.ax` rc 2 with an empty `HOME`, rc 0 with `bandit.ax` in `$HOME/.axon/lib` (§9 r3) | RC-5 |
| RCA-10: the 10-run command lists in-progress runs; NH-1 (a) costs a second full run on `main` | minor | accepted: `--status completed`; NH-1 (a) cost amended | RC-6, NH-1 |
| RCA-11: `authoring-eval-spec.md:580-581` is the HEAD line; the worktree copy (another agent's uncommitted edit) has it at `:586-587` | minor | accepted: the builder re-anchors to the committed text | §0, §6 |
| RCA-12: an `<outdir>` inside the checkout that is not gitignored makes `build.rs` report `-dirty` (`build.rs:83-86`) | minor | accepted: `<outdir>` outside the checkout (CI `$RUNNER_TEMP/dist`) or under the ignored `/dist/` (`.gitignore:46`) | RC-4 |
| RCA-13: "above `## Batch 7`" goes stale | minor | accepted: above the newest batch heading at the first release | RC-1 rule 6 |
| RCD-3: a new `par` stage needs a `governance/gate-lanes.json` row, or `gate_lane.py check` fails (its `about`, `:3`; `stage_gate_lane`, `gate.sh:673-676`) | major | accepted | RC-3, §6 |
| RCD-4: §6 never runs the lane, and g01 refuses an uncommitted tree (`v022_g01_mutations.py:49`) | major | accepted: commit, then targeted tests + g01 `--only` per item, then `gate.sh --lane` once | §3, §6 |
| RCD-5: the cost table hid the network/integrator parts and copied the strict figure | major | accepted: table rebuilt; no workflow linter is installed (`command -v actionlint shellcheck act yamllint` rc 1, §9 r4) | §5 |
| RCD-7: RC-3's `--section-has-behaviour-change` query had no test and no row | minor | accepted: test + M5268 | RC-3, §4 |

## 0. Source entries

| item | entry (tasks/opportunities.md) | sev |
|---|---|---|
| RC-1..RC-5 | O-PM-04 (`:3551-3555`): no release workflow, version `0.1.0`, no edition/deprecation policy, the card is not versioned | medium |
| RC-6 | O-CI-01 (`:3572-3580`): ci-hygiene NH-1 asks for 10 consecutive green push runs on `main` before the Linux jobs become required | high |

Consumer that already waits on this spec: `tasks/authoring-eval-spec.md:580-581` (HEAD text): "Until O-PM-04 defines releases, a
'release' is a commit the integrator labels." `[REVISED: RCA-11]` In the worktree that spec has uncommitted edits by another agent and
the same text sits at `:586-587`; the builder re-anchors to whichever is committed at build time. The AE harness pins `card_sha256`
per run (`crates/axon-intent/benchmarks/genviab/ae.py:117-121`, `:214`) and refuses `--expect-card-change` when the cards are
byte-equal (`:561-563`).

### What exists at f14bc25f (read, not guessed)

- **Version.** One number: `[workspace.package] version = "0.1.0"` (`Cargo.toml:36-37`). `axon-core` inherits it
  (`crates/axon-core/Cargo.toml:3`). `axon --version` prints `CARGO_PKG_VERSION (AXON_GIT_SHA)` (`crates/axon-core/src/main.rs:31`,
  observed `axon 0.1.0 (f14bc25f)`). `AXON_GIT_SHA` is the short HEAD sha, `-dirty` when the tree is dirty, `unknown` without git or
  when git's toplevel is another repo; a packager may set it (`crates/axon-core/build.rs:4-13`, `:19-24`, `:44-60`). Twelve crates pin
  their own `version = "0.1.0"` instead of the workspace's (`axon-ai`, `axon-certcheck`, `axon-cortex`, `axon-intent`, `axon-ledger`,
  `axon-loop`, `axon-loop-contracts`, `axon-os`, `axon-reflex`, `axon-signal`, `axon-web`, `cortex-policy-adapter`).
- **Consumers of `--version`.** `ae.py:118-119` takes the parenthesised sha; `scripts/ae_weekly.py:80-83` takes the text between the
  last `(` and `)` and refuses a `--version` that names no commit (`:93-94`, `:150`); `scripts/v022_g01_mutations.py:6850-6852`
  compares the whole string between binaries. None parses the version number.
- **Releases and tags.** None. `gh release list` prints nothing; the remote has no tags (`gh api repos/Cklaus1/axon/tags` empty);
  the only local tag is `ax-findings-base`. Workflows: `ci.yml`, `ios.yml`, `tee.yml`; none triggers on a tag. The repo is public.
- **"Release" today** means `scripts/release_check.sh` (239 lines): it reads evidence, never runs tests. Default mode prints
  `RELEASE-VERIFIED <sha>` when the tree is clean, HEAD is the target, a strict gate receipt bound to the commit exists in
  `.axon-runs/` with a clean `AXON_*` environment, 0 open false greens, the completeness and claims gates pass, and the crate
  manifest is complete (`:141-239`). `.axon-runs/` is gitignored (`.gitignore:129-130`), so CI can never see those receipts.
  `scripts/install.sh:61-75` builds from source (codegen + runtimes) into `$PREFIX`; there is no binary distribution.
- **The card.** `axon card` prints the card generated from `BUILTINS` (`crates/axon-core/src/card.rs:1-13`, `card()` `:609-630`);
  `--sha256` prints its sha256 (`main.rs:5600-5612`, verb `:505-514`, dispatch `:1427`). The card holds "no version string, so its
  sha256 changes only when its text does" (`card.rs:11-13`). Tests: every builtin signed and determinism (`card.rs:701-720`), every
  example type-checks (`:667-699`), `--sha256` is the sha of stdout (`crates/axon-core/tests/cli_run.rs:26439-26472`). Observed sha
  at f14bc25f: `b70a0b560b5b84eb1873629cb6913caa902554ce29d29ef8d83834907bfeb05c`, byte-identical to the d07c7429 musl build of
  2026-11-05. The genviab PREREG recorded `1469cc4e…f98f` at draft time (`crates/axon-intent/benchmarks/genviab/PREREG-genviab.md:29`),
  so the card text has changed at least once since its consumers started pinning it. `card.rs` has 2 commits (3df08219, fed67f91,
  both 2026-10-31); `builtins.rs`, whose signatures and docs feed the card, has 6 since.
- **`axon card --version`** already exists by clap's `propagate_version` (`main.rs:33-39`) and prints `axon-card 0.1.0 (f14bc25f)`,
  the binary version, not a card version. `axon reference --json` carries `"version": "0.1.0"` (schema `axon-reference/1`).
- **Behaviour changes are recorded** in CHANGELOG batch bullets with the bold marker `**Behaviour change`: 13 exact
  `**Behaviour change:**`, 19 lines with any suffix (`[REVISED: RCA-7]` e.g. `**Behaviour change (NH-1 (a)):**` `:20`,
  `**Behaviour change, arm c:**` `:173`, `**Behaviour change, native: …**` `:214`, `**Behaviour change (axon-ai):**` `:481`); one
  unbolded "Behaviour changes:" at `:541`. The convention, read from those bullets `[REVISED: RCA-5]`: a refusal of a program that
  **checked clean and panicked** is the fix and is not tagged (TP-3's "`b + 1` is E0102 instead of a run panic", `:22`); a refusal
  of a program that **ran without fault** is tagged (TP-1's refusal "even where the run printed the value", `:20`; dead arms `n3`,
  `s2`, `e3`, `:21-23`; `:159-160`). Batches also state a corpus sweep: "The 717 tracked `.ax` files check with the same exit codes
  before and after this batch" (`:23`; `:142` adds `check --require-contained` and `run`; `:164` says 712). There are 87 `## `
  headings, none a version. `[REVISED: RCA-6]` A `Migration:` note exists twice (`:182` CE-8, `:187` CE-6, the second bold) and
  follows no rule.
- **Stability/edition text.** None that defines a policy. `grep -rniE 'edition|deprecat|stabilit|semver'` over `spec/`,
  `ROADMAP.md`, `VISION.md`, `governance/specs/` finds: the `@[contained]` deprecation notice E1316, emitted only under
  `--effects-strict` (`crates/axon-core/src/effects.rs:292-311`, `error.rs:140`); "may be deprecated in Phase 4" (`spec/stdlib.md:368`);
  R38's warning that an embedding contract ("frozen surface, semver, migration guarantees") is "a ball and chain" while Axon is
  pre-1.0 (`governance/specs/R38-embedded-agent-runtime.md:113-115`, slice 2 `:505`).
- **Declaration syntax today.** Attributes precede items only (`spec/grammar.ebnf:84`, `:102-103`); there is no module-level
  attribute and no manifest file (`git ls-files` has no `axon.toml`). An unknown attribute is warning W0001 and the program runs
  (probes p1/p2, §9).
- **CI and required checks.** Ruleset 26386102 on `refs/heads/main`: `non_fast_forward`, `deletion`, enforcement active, no bypass
  actors; no branch protection (HTTP 404). ci-hygiene NH-1 was decided 2026-11-05: require "the three Linux jobs plus Docs & spec
  hygiene (never iOS) only after 10 consecutive green push runs on main" (`tasks/ci-hygiene-spec.md:370-372`). That NH estimated the
  CI wall at "5-7 min" (`:352`). Measured now: 2383-3624 s per run (§9). Streak at 2026-11-11 08:0x UTC: 6 consecutive successes
  (1fda5a5e, 4199c91c, 9919af87, 9ccf5542, 1f71623d, 0b1183d7), f14bc25f in progress, 8d13c902 the last failure.

### Out of scope (logged, not dropped)

| what | why it is not here | where it lives |
|---|---|---|
| crates.io / package-manager publishing | not asked; no crate is published and twelve crates pin their own versions | - |
| signing releases or dependencies | needs a key-custody decision | O-PM-03 (`opportunities.md:3545-3549`) |
| non-Linux binaries (macOS, Windows, aarch64) | not trivial: no macOS runner is used outside `ios.yml`, and cross-built binaries cannot be smoke-run here | a later spec |
| a codegen (`axon build`) release binary | needs LLVM 23 and the runtime staticlibs laid out as `install.sh:16-20` documents; the interpreter binary refuses `axon build` with rc 1 and a hint (probe p6) | `scripts/install.sh` stays the codegen install path |
| 1.0 criteria and long-term support | a product decision for later; this spec makes 0.x releases mechanical | - |
| making the twelve self-versioned crates use `version.workspace` | none of them is the language version; changing them moves `Cargo.lock` with no user effect | - |
| a program-level edition declaration | product decision; NH-3 recommends not now | §8 NH-3 |
| AE harness keyed on tags (`ae_weekly.py` base = last tag) | that harness's spec owns it; RC-1 only defines the term | `tasks/authoring-eval-spec.md` follow-up |

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P1 | RC-1, RC-2, RC-3 | the contract itself: what a release and a breaking change are, a card version that cannot drift silently, and a mechanical check that every recorded behaviour change bumps the version |
| P2 | RC-4, RC-5 | the release artifact: a tag-triggered workflow that builds, smoke-tests and publishes the interpreter with checksums, and a corpus compatibility diff between releases |
| P3 (needs-human timing) | RC-6 | required CI checks on `main`: decided in principle (ci-hygiene NH-1), but it changes the integrator's push flow and its timing is the human's call (NH-1) |

## 2. Items

### RC-1 (P1) A written release and stability policy

**Anchors.** No policy text exists (§0). The vocabulary to reuse: `**Behaviour change:**` bullets (`CHANGELOG.md:20-23`), the
tracked-corpus sweep (`:23`, `:142`), `release_check.sh` RELEASE-VERIFIED (`scripts/release_check.sh:233-239`), the E1316
deprecation precedent (`effects.rs:292-311`).

**Change.** New file `spec/stability.md` (~80 lines, prose, no ```` ```axon ```` blocks), linked from `spec/axon-for-llms.md:3-8`
(one sentence) and `README.md`. It states, as decided in §7:
1. **Release.** A release is an annotated tag `vX.Y.Z` on a commit of `main` whose `ci.yml` push run succeeded and for which
   `scripts/release_check.sh <sha>` printed `RELEASE-VERIFIED` on the integrator host. `X.Y.Z` is `[workspace.package] version`
   (D1). Only the newest release is supported (D8).
2. **Breaking change** (D2). A program that checked clean **and** ran without fault (exit 0 or its own exit code, no panic, no
   runtime error exit) now (a) is refused by `axon check`, or (b) gives another stdout or exit code for the same input and
   environment. A program that checked clean and then panicked, and is now refused at check, is a soundness fix and is not breaking.
   Every breaking change gets a bullet carrying the bold marker `**Behaviour change` (any suffix, e.g. `**Behaviour change (axon-ai):**`)
   in its batch, as today; unbolded text is not a marker.
3. **Version rule** (pre-1.0, subject to NH-2). A release whose CHANGELOG section holds a `**Behaviour change` marker line, or whose
   card version differs from the previous release's, bumps MINOR (PATCH := 0). Any other release bumps PATCH. RC-3 enforces it.
4. **Card version** (D3). `axon card --card-version` prints an integer that increases by 1 whenever the card text changes. The
   CHANGELOG release heading names it.
5. **Migration diagnostics** (D4). When syntax or a builtin is removed or renamed, the checker keeps a dedicated diagnostic for the
   old form, naming the new form, for at least the next MINOR release. A form is never re-read with a new meaning silently.
6. **CHANGELOG release heading.** `## vX.Y.Z - YYYY-MM-DD (card N, <sha8>)`, where `<sha8>` is the first 8 hex of line N of
   `card-versions.txt`, inserted above the batch headings the release covers. Everything above the newest release heading is
   unreleased. `[REVISED: RCA-13]` The first release heading goes above the newest batch heading at the time of the first release; the
   history below it is pre-release and exempt from rule 3.

**Fail-first test.** None (docs). RC-3's checker enforces rules 3, 4 and 6; RC-2 enforces 4.

**g01 rows.** None (no code).

**Acceptance.** `spec/stability.md` exists with the six rules; `spec/axon-for-llms.md` and `README.md` link it;
`cargo test --locked -p axon-core --no-default-features --test doc_snippets` passes (the new file holds no axon block, and
`axon-for-llms.md`/`README.md` are in its list, `crates/axon-core/tests/doc_snippets.rs:35-38`).

### RC-2 (P1) The card has a version, and a text change without a bump fails a test

**Anchors.** `card.rs:11-13` (no version string), `:609-630` (`card()`), tests `:634`, `:701-720`; `main.rs:505-514` (`Card {
sha256 }`), `:1427`, `:5600-5612` (`cmd_card`); `cli_run.rs:26439-26472`.

**Reproduction (f14bc25f).**

| probe | command | observed | after RC-2 |
|---|---|---|---|
| c1 | `axon card --sha256` | rc 0, `b70a0b56…eb05c` | same bytes (the card text does not change, D3) |
| c2 | `axon card --card-version` | rc 2, `error: unexpected argument '--card-version' found` | rc 0, `1` |
| c3 | `axon card --version` | rc 0, `axon-card 0.1.0 (f14bc25f)` | unchanged (clap's binary version) |
| c4 | edit one curated line of `card.rs`, keep the version | nothing fails today [INFERENCE: no test pins the sha] | `the_card_hash_is_pinned_to_its_version` fails |

**Change.**
1. `card.rs`: `pub const CARD_VERSION: u32 = 1;` with a doc line pointing at `card-versions.txt`. The card text is unchanged, so
   `card.rs:11-13` stays true; amend it to say the version lives beside the text, not in it.
2. New file `crates/axon-core/card-versions.txt`, one line per card version: `<N> <sha256 of card()>`. First line:
   `1 b70a0b560b5b84eb1873629cb6913caa902554ce29d29ef8d83834907bfeb05c` (the observed sha at f14bc25f). Cards before versioning are
   "card 0" and are not listed. `[REVISED: RCA-4]` A line that a release heading names is frozen: RC-3 rule 3 refuses a heading whose
   `<sha8>` is not that line's prefix, so rewriting a released line fails the ledger. The unreleased last line may be rewritten.
3. `main.rs`: `Card { sha256, card_version }` with `#[arg(long, conflicts_with = "sha256")] card_version: bool`; `cmd_card`
   prints `CARD_VERSION` and a newline.
4. A unit test in `card.rs` reads the file with `include_str!("../card-versions.txt")`.

**Fail-first tests.**
- `card::tests::the_card_hash_is_pinned_to_its_version` (lib): parses `card-versions.txt`. Versions are `1, 2, …` with no gap, shas
  are distinct 64-hex, the last version equals `CARD_VERSION`, and the last sha equals sha256 of `card()`. The failure message names
  both shas and says "bump CARD_VERSION and append a line". Marker `ATTACK: the card text changed without a card version bump`.
  It fails on base: the file and the const do not exist. The fail-first that matters is g01 M5260 (a text edit with no bump).
  `[REVISED: RCA-4]` This test alone accepts a rewritten last line; the released-line half of "append-only" is RC-3's
  `a_released_card_line_rewritten_is_refused` (M5267).
- `cli_run::axon_card_prints_its_version_number`: `axon card --card-version` gives rc 0 and stdout equal to the last line's `N` plus
  `\n`; `--card-version --sha256` gives rc 2. Marker `ATTACK: axon card does not print its card version`. Fails on base (probe c2).

**g01 rows.** M5260, M5261 (§4).

**Acceptance.** Both tests pass. `axon card --sha256` is byte-equal to f14bc25f's (so neither the AE pins nor the round-0 prompt shas
move, and AE needs no `--expect-card-change`). The existing card tests pass unchanged.

### RC-3 (P1) A release ledger: every behaviour change and card change bumps the version

**Anchors.** `CHANGELOG.md` headings (87, none a version); `Cargo.toml:36-37`; `Cargo.lock` (workspace crates' `version`
entries; `cargo build --locked` refuses a stale lock); precedent for a Python ledger with a cargo test: `scripts/spec_ledger.py`
(gate P lane, `scripts/gate.sh:689-693`, `:720`) and `crates/axon-core/tests/pci_delta_note.rs:11-35` (spawns a script through
`script_spawn`, asserts `PASS`, plus a planted-failure control at `:37-59`).

**Change.** New `scripts/release_ledger.py` (stdlib only, offline). `[REVISED: RCA-3]` Inputs: `--root PATH` (default: the repo
root; tests point it at a fixture tree holding `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`, member `Cargo.toml`s and
`crates/axon-core/card-versions.txt`) and `[--tag vX.Y.Z]`. It parses release headings
(`^## v(\d+)\.(\d+)\.(\d+) - \d{4}-\d{2}-\d{2} \(card (\d+), ([0-9a-f]{8})\)$`) and the sections between them, and refuses:
1. release versions that do not strictly decrease top to bottom;
2. `[REVISED: RCA-7]` a section with a line containing `**Behaviour change` (any suffix: `CHANGELOG.md:20`, `:173`, `:214`, `:481`),
   or a card number other than the next-older release's, whose version does not bump MINOR over the next-older release (for 0.x;
   `X` bumps once NH-2 picks 1.0 rules), or a MINOR bump whose PATCH is not 0;
3. a heading card number that is not a version listed in `card-versions.txt`; `[REVISED: RCA-4]` a heading `<sha8>` that is not the
   prefix of line N's sha; in `--tag` mode, a newest heading whose card number is not the file's last version (between releases the
   file may run ahead of the newest heading: that is an unreleased card change);
4. `[REVISED: RCA-3]` when any release heading exists: the newest heading's version differs from `Cargo.toml`
   `[workspace.package] version`; or a `[workspace] members` crate whose own `Cargo.toml` says `version.workspace = true` has
   another version in `Cargo.lock`. Members with their own `version = …` are ignored;
5. `--tag vX.Y.Z` mode (release workflow, RC-4): the tag differs from the newest heading or the Cargo version; or there is unreleased
   text above the newest heading (the release must cover every batch). Exit 0 prints `release_ledger: PASS (<n> releases)`; any
   refusal prints one line per problem on stderr and exits 1. With no release heading the default mode passes (nothing released),
   and `--tag` mode refuses.
6. `[REVISED: RCD-7]` Query mode `--section-has-behaviour-change vX.Y.Z` prints `1` when that release's section has a marker line
   (rule 2's test) and `0` otherwise, rc 0; rc 1 when no such heading exists. RC-4 feeds it to RC-5's diff.

Wire it as `par "release ledger" stage_release_ledger` next to `stage_spec_ledger` (`gate.sh:689-720`), `[REVISED: RCD-3]` and add
`"release ledger"` to `governance/gate-lanes.json` `"par"` after `"spec ledger"` (`gate_lane.py check` fails on a par stage with no
row, `governance/gate-lanes.json:3`). CI runs the script through the new cargo test (Check & Test runs every `axon-core` test,
`.github/workflows/ci.yml:93`).

**Fail-first tests** (new `crates/axon-core/tests/release_ledger.rs`, the `pci_delta_note.rs` pattern, fixtures written to a temp
dir; every test fails on base because the script does not exist):
- `the_repo_changelog_and_versions_agree`: the repo's own files, default mode, `PASS`.
- `a_behaviour_change_without_a_minor_bump_is_refused`: `v0.1.0` then `v0.1.1` whose section has a `**Behaviour change:**` bullet:
  rc 1, stderr names `v0.1.1`. Marker `ATTACK: a behaviour change shipped without a MINOR bump`.
- `a_card_change_without_a_minor_bump_is_refused`: `v0.1.0 (card 1)` then `v0.1.1 (card 2)`: rc 1. Marker `ATTACK: a card change
  shipped without a MINOR bump`.
- `a_tag_that_is_not_the_cargo_version_is_refused`: `--tag v0.2.0` with Cargo `0.1.1`: rc 1; and unreleased text above the newest
  heading in `--tag` mode: rc 1. Marker `ATTACK: a tag was accepted that is not the released version`.
- `a_stale_lock_version_is_refused`: heading and Cargo `0.2.0`, inheriting member `axon-core` at `0.1.0` in `Cargo.lock`: rc 1
  (marker as above).
- `[REVISED: RCA-4]` `a_released_card_line_rewritten_is_refused`: heading `v0.2.0 (card 1, b70a0b56)` while line 1 of the
  fixture's `card-versions.txt` holds another sha: rc 1, stderr names card 1. Marker `ATTACK: a released card version's sha was
  rewritten`.
- `[REVISED: RCD-7]` `the_section_query_reports_a_behaviour_change`: `--section-has-behaviour-change v0.2.0` prints `1` for a section
  with `**Behaviour change (x):**` and `0` for a section without; an absent version gives rc 1. Marker `ATTACK: the release ledger
  hides a behaviour change from the compat diff`.
- Controls in `release_ledger_controls_pass`: a PATCH release with no behaviour change; a MINOR release with one; no release heading
  at all (default mode PASS, `--tag` refused); `[REVISED: RCA-3]` a self-versioned member at `0.1.0` in `Cargo.lock` while the
  release is `0.2.0` (PASS); `[REVISED: RCA-4]` an unreleased last line rewritten (PASS).

**g01 rows.** M5262-M5265, M5267, M5268 (§4).

**Acceptance.** The tests pass; `python3 -B scripts/release_ledger.py` prints `PASS (0 releases)` on the tree before the first
release; the gate P lane lists the stage.

### RC-4 (P2) A tag-triggered release workflow for the Linux interpreter, with checksums

**Anchors.** No release workflow (§0). Build precedent: the static musl interpreter in `scripts/build-guest-image.sh:195-204`
(`-C target-feature=+crt-static`, `--target x86_64-unknown-linux-musl --no-default-features --bin axon --release`, then `file |
grep static`). CI toolchain pattern: `rustup toolchain install --profile minimal` reads `rust-toolchain.toml` (`ci.yml:58-65`),
musl target with retries (`ci.yml:216-229`). `ring` (via `reqwest` → `rustls`, from `axon-domain`) has `cc` as a build dependency
for the musl target (cargo tree, §9), so the runner needs a musl C compiler (`musl-tools`; the dev host has `/usr/bin/musl-gcc`).

**Change.** New `scripts/release_package.sh <outdir> [--keep-env]` (runnable on the dev host and in CI) and
`.github/workflows/release.yml`.
1. `[REVISED: RCD-1]` Triggers: `push: tags: ['v*.*.*']`; `push: branches: ['release-dry/**']` (always a dry run: a push runs the
   workflow file of the pushed commit, so this works before `release.yml` is on `main`); `workflow_dispatch` with input `dry_run`
   (default `true`), usable once the file is on `main`. `permissions: contents: write, actions: read`. One job on `ubuntu-24.04`,
   `actions/checkout@v4` with `fetch-depth: 0`.
2. Preconditions on a tag (fail the job, publish nothing): the tagged commit is an ancestor of `origin/main`; `[REVISED: RCA-1]`
   with `env: GH_TOKEN: ${{ github.token }}`, `gh api -X GET
   "repos/$GITHUB_REPOSITORY/actions/workflows/ci.yml/runs?head_sha=$GITHUB_SHA&event=push&branch=main&status=success" --jq
   .total_count` is ≥ 1; `python3 -B scripts/release_ledger.py --tag $GITHUB_REF_NAME` passes. `[REVISED: RCD-2]` In a dry run the
   ancestry and CI checks print `SKIPPED (dry run): <check>` and do not fail, and the ledger runs in default mode.
3. `release_package.sh`: `[REVISED: RCA-8]` refuses (rc 1, before building, naming the variable) when `AXON_GIT_SHA` is set, unless
   `--keep-env`; `[REVISED: RCA-12]` refuses an `<outdir>` inside the checkout unless it is under the ignored `/dist/`
   (`.gitignore:46`; otherwise `build.rs:83-86` sees an untracked file and embeds `-dirty`); CI uses `$RUNNER_TEMP/dist`. In CI the
   workflow installs `musl-tools` (3 apt attempts, as `ci.yml:225-228`) and runs `rustup target add x86_64-unknown-linux-musl`;
   the script then runs `RUSTFLAGS="-C target-feature=+crt-static" cargo build --locked --release -p axon-core
   --no-default-features --bin axon --target x86_64-unknown-linux-musl`.
4. Smoke (each a hard failure): `file` says `static`; `axon --version` equals `axon $VER ($(git rev-parse --short HEAD))` (no
   `-dirty`, no `unknown`); `axon card --sha256` equals the last sha in `card-versions.txt` and `--card-version` its number;
   `examples/hello.ax` runs from its directory with rc 0 and stdout `Hello, Axon!`; RC-5's compat sweep with this binary.
5. Package: `axon-$VER-x86_64-unknown-linux-musl.tar.gz` holding `axon`, `LICENSE`, `card.md` (the binary's `axon card` output) and
   `compat.tsv` (RC-5); `SHA256SUMS` over the tarball and `compat.tsv`. Dry run: `actions/upload-artifact` only. Tag run:
   `gh release create $TAG --verify-tag --notes-file <the CHANGELOG section of $TAG>` with the three files. `release.yml` never
   passes `--keep-env`.

**Reproduction.** p7 (§9): the musl interpreter built by `build-guest-image.sh` at d07c7429 is `static-pie linked`, runs `axon run`
(rc 0) and prints the same card sha as f14bc25f. The release workflow itself cannot be run before it exists.

**Fail-first test.** A workflow has no local test, and no workflow linter is installed (§9 r4). `[REVISED: RCD-6]` The planted
failure is offline: on the dev host, `AXON_GIT_SHA=deadbeef scripts/release_package.sh <tmp> --keep-env` exits 1 naming the
version line (shows the smoke judges), and the same without `--keep-env` exits 1 before building. The ledger part is RC-3's tests.

**g01 rows.** None (workflow and shell glue; the judging logic is RC-3's and RC-5's scripts, which have rows).

**Acceptance.** `[REVISED: RCD-1]` (1) `release_package.sh /var/tmp/…/dist` on the dev host passes all smoke checks; both planted
runs exit 1 as above. (2) The integrator pushes the integrated head as `release-dry/<sha8>`; that run succeeds (precondition lines
print `SKIPPED (dry run)`) and its artifact's `SHA256SUMS` verifies (`sha256sum -c`). Run id in the build log. (3) The first real tag
(NH-4) publishes a release whose tarball's `axon --version` names the tag's version and commit.

### RC-5 (P2) A corpus compatibility diff between releases

**Anchors.** The batch sweeps (`CHANGELOG.md:23`, `:142`, `:164`) are run by hand from scratch scripts outside the repo
(`/var/tmp/r51-tmp/Soundness4/sweep.sh`, cited in `tasks/tuple-patterns-spec.md:364`). There is no record of what a release checked.

**Reproduction (§9 p8).** The sweep (`axon check` of all 717 tracked `.ax` files, 12 parallel, from each file's directory) took
0.70 s with the f14bc25f interpreter: 626 rc 0, 91 rc 2. Against the d07c7429 musl binary: rc identical for all 717; the combined
output differs for 6 files, all E0901 `module ... not found` whose search list prints the binary's own install directory
(`examples/asi/bandit_ucb.ax`, `persistent_bandit.ax`, `safe_bandit.ax`, `safe_self_improve.ax`, `examples/sealed/suite/
accept.ax`, `tasks/r51-s6/q1/suite/accept.ax`). So a byte comparison of diagnostics would report drift that is only the path (D6).

**Change.** New `scripts/compat_sweep.py`:
- `--bin AXON --out TSV`: for each `git ls-files '*.ax'`, `axon check --json <basename>` from the file's directory, with
  `AXON_*` removed from the environment, `[REVISED: RCA-9]` `HOME` set to an empty temp dir and the binary copied into a temp dir with
  no `../lib/axon` (E0901 searches `$HOME/.axon/lib` and `<bin>/../lib/axon`; §9 r3), and a 60 s timeout; rows
  `path<TAB>rc<TAB>first error code or ->`, sorted.
- `--diff OLD NEW --section-has-behaviour-change {0,1}`: lists files in both whose rc went 0 → nonzero (`NEWLY REFUSED`) or whose
  first code changed, and files whose rc went nonzero → 0 (`NEWLY ACCEPTED`, informational). Exit 1 if any file is newly refused and
  the flag is 0.
- RC-4 step 4: `gh release download <previous tag> -p compat.tsv` (skipped for the first release), sweep with the release binary,
  diff with the flag from `release_ledger.py --section-has-behaviour-change $TAG` (RC-3 mode 6).

**Fail-first tests** (in `release_ledger.rs`, fixtures only, no binary): `a_tracked_program_newly_refused_needs_a_behaviour_change`
(OLD `a.ax 0 -`, NEW `a.ax 2 E0102`, flag 0: rc 1, stdout names `a.ax`; marker `ATTACK: a newly refused tracked program shipped
without a behaviour change`); control `compat_diff_controls_pass` (same flag 1: rc 0; a newly accepted file alone: rc 0; a diagnostic
text change with the same rc and code: rc 0).

**g01 rows.** M5266 (§4).

**Acceptance.** The tests pass; RC-4's dry run attaches `compat.tsv` with 717+ rows; the first release's notes state the sweep counts.

Limit, stated in `spec/stability.md`: the sweep measures rule 2(a) on the tracked corpus only. Rule 2(b) (changed run output) stays
covered by the golden-output tests in `cli_run.rs` and the batch `run` sweeps; a `run` sweep of the corpus is not mechanised here
because many tracked programs read stdin, the network or the clock [INFERENCE: not counted].

### RC-6 (P3, needs-human timing NH-1) The Linux CI jobs become required checks on `main`

**Anchors.** Ruleset 26386102 (`gh api repos/Cklaus1/axon/rulesets/26386102`: `non_fast_forward`, `deletion`, include
`refs/heads/main`, `bypass_actors: []`). Decision: `tasks/ci-hygiene-spec.md:370-372`. Direct pushes are bound too: "After all
required status checks pass, any commits must either be pushed to another branch and then merged or pushed directly to the
protected branch" (GitHub docs, *About protected branches*, read 2026-11-11). `ci.yml` triggers: `push` on `main`, `pull_request` to
`main`, `workflow_dispatch` (`ci.yml:3-9`). Contexts are the job names, exactly as GitHub reports them (§9): `Docs & spec hygiene (no
build)`, `Check & Test (no LLVM)`, `Test the whole workspace as root (no LLVM)`, `Codegen build + tests (LLVM 23) — NOT the parity
suite` (an em dash).

**Change (after NH-1 says when and how).**
1. Precondition, checked by the integrator with `[REVISED: RCA-10]` `gh run list --branch main --workflow ci.yml --event push
   --status completed --limit 10 --json headSha,conclusion`: the newest 10 completed push runs are all `success`, except that a run
   failing only at `Install LLVM 23` after 3 logged attempts does not reset the count (ci-hygiene `:357-359`).
2. NH-1 (a): `ci.yml` `on.push.branches: [main, 'staging/**']`. The integrator pushes `staging/<sha8>`, waits for that run, then
   pushes the same sha to `main` (a fast-forward), so the commit already carries the four checks. `[REVISED: RCA-10]` The push to
   `main` starts a second full run of the same sha; the streak and RC-4's `branch=main` check wait for that one.
3. `gh api -X PUT repos/Cklaus1/axon/rulesets/26386102` keeping the two rules and adding `{"type":"required_status_checks",
   "parameters":{"strict_required_status_checks_policy":false,"required_status_checks":[{"context":...}x4]}}`. Never iOS.
4. Emergency path, written into `spec/stability.md`: the owner sets the ruleset's `enforcement` to `disabled` with one `gh api` call
   and back afterwards; each use is logged in CHANGELOG. [INFERENCE: a personal-account repo cannot list bypass actors other than
   by role; not checked.]

**Fail-first test.** A repo setting has no unit test. Acceptance by observation: after step 3, a push to `main` of a commit with no CI
run is refused with `GH013`/`GH006` [INFERENCE: the exact code], and a push of a staging-green sha is accepted. Both pushes logged.

**g01 rows.** None.

**Acceptance.** `gh api …/rulesets/26386102 --jq '[.rules[].type]'` lists `required_status_checks`; the two observations above.

## 3. Shared mechanics

- **Card change procedure** (after RC-2): any commit that changes the card text bumps `CARD_VERSION`, appends one line to
  `card-versions.txt`, and says `card N` in its CHANGELOG bullet. The next AE run across that commit uses `--expect-card-change`
  (`ae.py:561-563`) and makes a full set of uncached round-0 calls [INFERENCE: the round-0 prompt is card + task, `ae.py:124`].
- **Release procedure** (integrator, after RC-1..RC-5): pick a CI-green `main` commit with `RELEASE-VERIFIED`; in one commit bump
  `[workspace.package] version`, refresh `Cargo.lock` (`cargo update -w --offline`), insert the CHANGELOG heading; push, wait for
  the `main` push run to be green, run `release_check.sh` on it, `git tag -a vX.Y.Z`, push the tag. The workflow does the rest.
- g01 rows (`scripts/v022_g01_mutations.py`, tuple format of `:16053-16073`: `(id, title, file, old, new, crate, test args, killer)`)
  and `ATTACK:` markers (`scripts/v022_attack_markers.py`, `ATTACK_MARKERS.update({..})` as at `:4452-4466`) land in each fix
  commit. `[REVISED: RCD-4]` The g01 runner refuses an uncommitted tree (`v022_g01_mutations.py:49`), so `python3
  scripts/v022_g01_mutations.py --only=<the item's rows>` runs after each item's commit. The builder writes the exact `old` text after
  the code lands. The strict gate only checks that rows anchor (`stage_g01_anchors`); the lane `--only` run is the kill check.
- No interpreter behaviour changes in RC-1..RC-6, so no corpus sweep is needed per item; RC-5's sweep is run once by hand with the
  lane binary to fill §9's "after".

## 4. g01 rows (block M5260-M5279)

Block M5260-M5279 is assigned to this spec by Main. At f14bc25f no `M526x`/`M527x` appears in `scripts/`, `crates/` or `tasks/`
(highest row M5231, `scripts/v022_g01_mutations.py:16099`). `[REVISED: RCA-4, RCD-7]` Nine ids are used and eleven are left.

| id | item | guard removed by the mutant | file | crate / args | killer | marker |
|---|---|---|---|---|---|---|
| M5260 | RC-2 | one curated card line edited (mutant: a rule sentence in `CURATED` reworded), `CARD_VERSION` unchanged | `crates/axon-core/src/card.rs` | axon-core `--no-default-features --lib` | `card::tests::the_card_hash_is_pinned_to_its_version` | `ATTACK: the card text changed without a card version bump` |
| M5261 | RC-2 | `cmd_card` prints `CARD_VERSION` (mutant: prints `0`) | `crates/axon-core/src/main.rs` | axon-core `--no-default-features --test cli_run` | `axon_card_prints_its_version_number` | `ATTACK: axon card does not print its card version` |
| M5262 | RC-3 | the MINOR-bump rule for a behaviour change (mutant: `if False and behaviour_changed`) | `scripts/release_ledger.py` | axon-core `--no-default-features --test release_ledger` | `a_behaviour_change_without_a_minor_bump_is_refused` | `ATTACK: a behaviour change shipped without a MINOR bump` |
| M5263 | RC-3 | the MINOR-bump rule for a card change (mutant: card numbers never compared) | same | same | `a_card_change_without_a_minor_bump_is_refused` | `ATTACK: a card change shipped without a MINOR bump` |
| M5264 | RC-3 | tag == newest heading == Cargo version (mutant: the tag comparison dropped) | same | same | `a_tag_that_is_not_the_cargo_version_is_refused` | `ATTACK: a tag was accepted that is not the released version` |
| M5265 | RC-3 | `Cargo.lock` version of the inheriting crates (mutant: lock not read) | same | same | `a_stale_lock_version_is_refused` | same as M5264 |
| M5266 | RC-5 | the newly-refused rule (mutant: rc 0 → nonzero not reported) | `scripts/compat_sweep.py` | same | `a_tracked_program_newly_refused_needs_a_behaviour_change` | `ATTACK: a newly refused tracked program shipped without a behaviour change` |
| M5267 | RC-3 | `[REVISED: RCA-4]` heading `<sha8>` vs line N of `card-versions.txt` (mutant: the sha8 comparison dropped) | `scripts/release_ledger.py` | axon-core `--no-default-features --test release_ledger` | `a_released_card_line_rewritten_is_refused` | `ATTACK: a released card version's sha was rewritten` |
| M5268 | RC-3 | `[REVISED: RCD-7]` the section query (mutant: always prints `1`) | same | same | `the_section_query_reports_a_behaviour_change` | `ATTACK: the release ledger hides a behaviour change from the compat diff` |

## 5. Gates and cost budget ($0: no paid call, no AI path touched; Actions minutes are free on this public repo)

`[REVISED: RCD-5]` Split by who can run it.

| stage | wall | who / where | source |
|---|---|---|---|
| targeted tests per item: `--lib card::`, `--test cli_run axon_card`, `--test release_ledger`, `--test doc_snippets` | [INFERENCE: < 30 s each warm; ~30 s cold link for the new target] | lane, offline | `cli_run` `--no-run` warm 4.4 s (`tasks/tuple-patterns-spec.md:439`) |
| `release_ledger.py` / `spec_ledger.py` | < 1 s | lane, offline | "well under a second" (`gate.sh:690`) |
| compat sweep, 717 files | 0.70 s (debug interpreter), 0.52 s (musl release) | lane, offline | measured (§9 p8) |
| 9 g01 rows | ~2-3 min | lane, offline, committed tree | ~15 s per row (`tasks/tuple-patterns-spec.md:445`) |
| `scripts/gate.sh --lane`, once after RC-5 | [INFERENCE: ~10-15 min warm, dominated by `cli_run` ~8 min (`tasks/tuple-patterns-spec.md:441`)] | lane, offline | P group + S16 targets the change selects |
| `release_package.sh` on the dev host + 2 planted runs | [INFERENCE: 5-10 min cold, 1-2 min warm; the planted runs refuse before or right after the build] | lane, offline (`/usr/bin/musl-gcc` present) | S22's warm release build + vm_parity took 86.6 s (`.doc-build/b5-loop/gate-stages.1275991.tsv`) |
| `release.yml` lint | none: no `actionlint`/`shellcheck`/`act`/`yamllint` installed | - | §9 r4 |
| RC-4 dry run (`release-dry/` push, one job, no cache) | [INFERENCE: 15-25 min; the CI `cargo test` step alone is 1302 s] | integrator, network, once | §9 ci |
| `scripts/gate.sh --strict` once for RC-1..RC-5 | 2634-3273 s idle; 83m35s under 3-lane load; budget 1 run + 1 retry | integrator | `tasks/tuple-patterns-spec.md:446` (citing enable-gaps §8 and `build-loop-checker.md:43`) |
| RC-6 | one `gh api` call; then every integration waits for the staging run and the second `main` run, 2383-3624 s each | human (NH-1), then integrator, permanently | measured run walls (§9) |

Lane cost [INFERENCE: estimates]: RC-1 ~80 lines of prose; RC-2 ~25 lines of Rust + 2 tests (~60 lines); RC-3 ~170 lines of
Python + ~250 lines of test; RC-4 ~90-line workflow + ~70-line script; RC-5 ~90 lines of Python + 2 tests. About 5-6 h of
implementation, then one dry run and one strict run by the integrator; $0.

## 6. Sequencing and lanes

`[REVISED: RCD-4]` One lane, offline, $0, commits in order: RC-1 (docs) → RC-2 (card) → RC-3 (ledger) → RC-5 (sweep, needs RC-3's
query mode) → RC-4 (script and workflow, uses RC-2/3/5). RC-6 is independent of the code and waits on NH-1 and the 10-green
precondition. The first tag waits on NH-2 and NH-4.
Files: `spec/stability.md` (new), `spec/axon-for-llms.md:3-8`, `README.md`, `crates/axon-core/src/card.rs`,
`crates/axon-core/card-versions.txt` (new), `crates/axon-core/src/main.rs:505-514,1427,5600-5612`, `crates/axon-core/tests/
cli_run.rs` (append), `crates/axon-core/tests/release_ledger.rs` (new), `scripts/release_ledger.py`, `scripts/compat_sweep.py`,
`scripts/release_package.sh` (new), `.github/workflows/release.yml` (new), `scripts/gate.sh:689-720`, `governance/gate-lanes.json`
(`"par"`), g01/marker files, `CHANGELOG.md`. Integrator-owned: `tasks/opportunities.md`, the ruleset, the `release-dry/` push, the
tag. Cross-batch: anyone changing the card text after RC-2 lands must follow §3 (the RC-2 test makes that unmissable). The AE harness
spec should replace "a commit the integrator labels" (`authoring-eval-spec.md:580-581` at HEAD; `[REVISED: RCA-11]` re-anchor to the
committed text at build time) with "a `v*` tag" once RC-4 lands (that spec's owner).

Order of work: (1) write each item's tests and run them on base (every RC-2/3/5 test fails: missing flag, file or script).
(2) Implement and commit each item; on the committed tree run its targeted tests and its g01 `--only` rows. (3) RC-4:
`release_package.sh` built and smoke-run on the dev host, both planted runs, then `release.yml` committed without a GitHub run.
(4) `scripts/gate.sh --lane --plan`, then `scripts/gate.sh --lane` once (P group: spec ledger, release ledger, gate-lane check,
G01 anchors, fmt; S16 targets the change selects: `cli_run`, `release_ledger`, `doc_snippets`); this replaces a separate full
`cli_run`. (5) Hand to the integrator: one `release-dry/<sha8>` push, one strict run at the integrated head, merge. RC-6's `ci.yml`
trigger edit (NH-1 (a)) has no cargo effect and rides in the next strict batch.

## 7. Engineering decisions (decided here; canon for the build)

These change no program's check or run result. NH-1..NH-4 (§8) are the product and repo-policy questions.

1. **D1 - the release version is `[workspace.package] version`** (`Cargo.toml:37`), the number `axon --version` already prints
   (`main.rs:31`). The twelve self-versioned crates are left alone (§0 out of scope).
2. **D2 - "breaking" is defined by programs that worked.** A program that checked clean and ran without fault now refuses, or prints
   or exits differently. A check-clean panic turned into a refusal is a fix. This is the convention the CHANGELOG already follows
   (`[REVISED: RCA-5]` `:22` untagged "E0102 instead of a run panic"; `:20` tagged because the run printed the value; `:21-23` tagged
   dead-arm refusals), so no past entry is reclassified.
3. **D3 - the card version lives beside the card text, not in it.** RC-2 then changes no card byte, so no AE pin, round-0 prompt sha
   or replay cache moves just to introduce versioning, and `card.rs:11-13` ("its sha256 changes only when its text does") stays true.
   A model reading the card gains nothing from a number it cannot declare (NH-3 (a)). If NH-3 picks a declaration, the card must
   print the version it accepts, and that is one card change under §3.
4. **D4 - migration diagnostics instead of silent re-reading.** A removed or renamed form keeps a dedicated diagnostic naming the
   replacement for at least the next MINOR release. Precedents: E1316 for `@[contained]` (`effects.rs:292-311`) and CS4-2's E0000
   "with help, instead of silently re-reading the arms" (`CHANGELOG.md:160`). This adds diagnostics only, never refuses a program
   the new rules accept.
5. **D5 - the release binary is a static musl interpreter.** It is the binary the guest image already builds and checks for static
   linking (`build-guest-image.sh:195-204`), and the observed d07c7429 build runs on this host (p7). A glibc build would carry the
   runner's glibc floor (the 9ccf5542 release binary on this host needs `GLIBC_2.39`, p7).
6. **D6 - the compat sweep compares rc and first error code, never diagnostic bytes**, because E0901's search list embeds the binary's
   install directory (6 of 717 files, p8).
7. **D7 - two keys to release.** CI cannot read `.axon-runs/` (gitignored), so the workflow checks what GitHub can see (a green push
   run of the sha on `main`, by the `-X GET` query of RC-4 step 2, and the ledger), and the procedure (§3) requires
   `release_check.sh` RELEASE-VERIFIED on the integrator host
   before the tag is pushed. Neither key alone publishes. [INFERENCE: the newest run directory in the main checkout's `.axon-runs/`
   is from 2026-09-25 by mtime, so RELEASE-VERIFIED at a current commit likely needs a fresh strict run; not run.]
8. **D8 - only the newest release is supported**; no backport branches while 0.x.
9. **D9 - releases are not signed** (O-PM-03); `SHA256SUMS` detects corruption, not tampering, and the release notes say so.

## 8. Needs human

1. **NH-1 - When and how the Linux jobs become required (RC-6).** The rule (10 green, the three Linux jobs plus Docs & spec hygiene,
   never iOS) is already decided (`ci-hygiene-spec.md:370-372`). New facts: the CI wall is 40-60 min per run (2383-3624 s measured),
   not the 5-7 min that NH estimated, and the streak is 6 with f14bc25f running (≈ 1-2 more days at ~3 pushes/day [INFERENCE]).
   Options:
   - (a) When the streak reaches 10: add the `staging/**` push trigger and the ruleset rule; the integrator pushes a staging branch,
     waits for green, fast-forwards `main`. The sha on `main` is the sha CI tested. Cost: each integration lands 40-60 min later,
     `[REVISED: RCA-10]` plus a second full CI run of the same sha on `main` (Actions minutes are free on this public repo); a release
     waits for that second run.
   - (b) Same timing, PR flow: every integration is a PR to `main` (the existing `pull_request` trigger), merged after green. No
     `ci.yml` change, but CI tests the PR merge ref and `main` gets a new merge commit, so the tested sha and the landed sha differ.
   - (c) Do not require checks; rely on RC-4 refusing to release a red commit, and keep direct pushes.
   **Recommendation: (a)**, as soon as the 10-green precondition holds. It keeps the integrator's local-merge workflow, the landed
   sha is the tested sha, and a red run can no longer advance `main`. (c) leaves `main` free to go red between releases, which is
   how 303 red runs accumulated (O-CI-01).
   `[DECIDED by human 2026-11-11: (a) staging/** first, once the streak reaches 10. RC-6 is not in this build.]`
2. **NH-2 - Version scheme.** Options:
   - (a) 0.x semver: MINOR for any behaviour change or card change, PATCH otherwise; 1.0 later by decision.
   - (b) CalVer `YYYY.M.N`: the date says how old a binary is, but nothing says whether an upgrade can break a program.
   - (c) Editions (`2026` + patch): a new edition per breaking change; heavy for a language with a behaviour change in most
     soundness batches (13 tagged bullets so far).
   **Recommendation: (a).** It is what Cargo and the existing `0.1.0` already express, a model or harness can compare two versions
   and know if programs may break (MINOR differs), and RC-3 can check it mechanically. Expect MINOR to move often while 0.x.
   `[DECIDED by human 2026-11-11: (a) 0.x semver.]`
3. **NH-3 - Can a program declare the card/edition it was written against?** Today `@[edition(e2026)]` and `@[edition("2026-11")]`
   on `main` check with W0001 "unknown attribute" and run (p1, p2); attributes attach to items only (`grammar.ebnf:84`, `:102`).
   Options:
   - (a) No declaration now. The card version and sha live in transcripts (`ae.py:121`), release notes and `card-versions.txt`; D4
     makes old syntax fail loudly with the new form named.
   - (b) An attribute on `main`, e.g. `@[card(2)]`: the checker warns (or refuses) when it differs from `CARD_VERSION`. Old binaries
     already accept it with W0001, so it is forward-tolerant.
   - (c) A header comment `// axon card 2`, read by the checker: invisible to the grammar, easy to forget.
   - (d) A manifest file (`axon.toml`): none exists; adds a second file to the "one `.ax` file" program model.
   **Recommendation: (a).** There is one card line today, so a declaration with one legal value tests nothing; when a second card
   version removes syntax, D4's diagnostics tell the model what to write, which a version mismatch warning does not. Deferring is
   free: (b) can be added later and old binaries tolerate it.
   `[DECIDED by human 2026-11-11: (a) no declaration now.]`
4. **NH-4 - Who tags, how often, and the first release.** Options:
   - (a) The integrator tags at most weekly, on a RELEASE-VERIFIED, CI-green commit after a batch; the first tag `v0.1.0` (or
     `v0.2.0` if a behaviour-change batch lands first) needs the human's explicit go, because it is the first public binary.
   - (b) A tag after every batch.
   - (c) Tags only on the human's request.
   **Recommendation: (a).** Weekly matches the AE weekly cadence (`authoring-eval-spec.md:577-578` at HEAD), keeps releases cheap,
   and the first publish stays a human decision.
   `[DECIDED by human 2026-11-11: (a) the integrator tags at most weekly; the first public tag needs the human's explicit go.]`

## 9. Verification log (f14bc25f; `/var/tmp/r51-tmp/ReleaseContract/`)

| probe | command | observed |
|---|---|---|
| p0 | `axon --version`; `axon card --sha256`; `axon card \| wc -l -c` | `axon 0.1.0 (f14bc25f)`; `b70a0b56…eb05c`; 534 lines, 31,582 B |
| c2 | `axon card --card-version` | rc 2, `unexpected argument '--card-version'` (tip: `--version`) |
| c3 | `axon card --version` | `axon-card 0.1.0 (f14bc25f)` |
| p1 | `@[edition(e2026)]` on `fn main` | check rc 0 + W0001 `unknown attribute @[edition]`; run prints `hi`, rc 0 |
| p2 | `@[edition("2026-11")]` on `fn main` | same as p1 (a string argument parses, though `grammar.ebnf:103` lists IDENT only) |
| p4 | `// axon card 1` header comment | check 0, run 0 |
| p5 | an attribute followed by EOF | E0000 `unexpected end of input`, rc 2 |
| p6 | `axon build p4_comment_header.ax` (interpreter build) | rc 1, "requires building axon with the `codegen` feature" + hint |
| p7 | `.doc-build/b5-loop/x86_64-unknown-linux-musl/release/axon` | `static-pie linked`, `axon 0.1.0 (d07c7429)`, runs p4 (rc 0), card sha `b70a0b56…` (= f14bc25f). `/opt/axon-unattended/9ccf5542/bin/axon`: dynamic, needs up to `GLIBC_2.39`; host glibc 2.43 |
| p8 | `Soundness4/sweep.sh` (check, 717 files) with sn2 and with the p7 binary | 0.70 s / 0.52 s; 626 rc 0, 91 rc 2; rc equal for all 717; 6 output differences, all E0901 search paths naming the binary's directory |
| ci | `gh run list --branch main --workflow ci.yml --event push --limit 10` | f14bc25f in_progress; 6 success (0b1183d7 … 1fda5a5e); 8d13c902 failure. Run walls 2383-3624 s; job steps at 0b1183d7: Check & Test `cargo test` 1302 s, Codegen `cargo test` 1275 s |
| ci | `gh run view 42973100658 --json jobs` | job names as listed in RC-6 (Codegen uses `—`) |
| rs | `gh api …/rulesets/26386102`; `…/branches/main/protection` | rules `non_fast_forward`, `deletion`, `bypass_actors: []`; protection 404 |
| tags | `gh release list`; `gh api …/tags`; `git tag` | empty; empty; `ax-findings-base` |
| tree | `cargo tree --offline --locked -p axon-core --no-default-features --target x86_64-unknown-linux-musl -i cc` | `cc` ← `ring` ← `rustls` ← `reqwest` ← `axon-domain` ← `axon-core` (build dependency) |
| fg | `AXON-COMPLETENESS.json` `false_greens` | 84 entries, 0 open |
| ledger | `python3 -B scripts/spec_ledger.py` before writing | PASS (0 problems, 35 specs) |
| r1 | `gh api …/workflows/ci.yml/runs -f head_sha=<0b1183d7 full>` vs `gh api -X GET "…/runs?head_sha=…&event=push&branch=main&status=success" --jq .total_count` | HTTP 404 (`-f` makes it a POST) vs `1` (Step 1, RCA-1) |
| r2 | `gh run list … --event push --status completed --limit 10` | 0b1183d7, 1f71623d, 9ccf5542, 9919af87, 4199c91c, 1fda5a5e success; 8d13c902, d07c7429, 10ed1406, 0e76271d failure (f14bc25f not yet completed) |
| r3 | `examples/asi/bandit_ucb.ax`, binary copied to a temp dir, `HOME` = empty temp dir; then `stdlib/bandit.ax` copied into `$HOME/.axon/lib` | `check` rc 2 (E0901 lists `$HOME/.axon/lib/bandit.ax`, `<bin>/../lib/axon/bandit.ax`, `./bandit.ax`); then rc 0 |
| r4 | `command -v actionlint shellcheck act yamllint` | none found, rc 1 |
| r5 | `grep -c '\*\*Behaviour change' CHANGELOG.md`; exact `**Behaviour change:**` | 19; 13 (suffix forms at `:20`, `:173`, `:214`, `:481`); `:541` unbolded "Behaviour changes:" |
| r6 | `python3 -B scripts/spec_ledger.py` after the Step 1 revision | PASS |

Not observed: anything marked `[INFERENCE]`; every "after" column; the cold release build time; whether `musl-tools` is preinstalled
on `ubuntu-24.04` runners; whether `workflow_dispatch` refuses a workflow absent from the default branch (GitHub docs, RCA-2/RCD-1);
the exact GitHub error code for a refused push; whether a personal-account ruleset can list a bypass role; whether
`scripts/release_check.sh` prints RELEASE-VERIFIED at a current commit (not run: `scripts/completeness.py` rewrites
`AXON-COMPLETENESS.md`, `scripts/completeness.py:26`, `:493`).

### Build log (lane `rc/l`, 2026-11-11, base a01db50c)

| item | commit | fail-first on base | after | g01 |
|---|---|---|---|---|
| RC-1 | 98fda905 | none (docs) | `doc_snippets` 2 passed | - |
| RC-2 | 2fb3649c | `card::tests` did not compile (no `card-versions.txt`, no `CARD_VERSION`); `axon_card_prints_its_version_number` FAILED | both pass; `axon card --sha256` = `b70a0b56…eb05c` (unchanged), `--card-version` = `1`, `axon card --version` unchanged | M5260, M5261 killed (2/2, 24 s) |
| RC-3 | 0901465b | all 8 `release_ledger` tests FAILED (no script) | 8 pass; repo: `release_ledger: PASS (0 releases)`; `gate_lane.py check` OK | M5262-M5265, M5267, M5268 killed (6/6, 54 s) |
| RC-5 | 0012dff6 | `a_tracked_program_newly_refused_needs_a_behaviour_change`, `compat_diff_controls_pass` FAILED (no script) | 10 pass; sweep with the lane debug binary: 717 files, 626 rc 0, 91 refused, 0.44 s; 6 E0901 | M5266 killed (1/1) |
| RC-4 | 664f8f1c, 70f7e900 | planted runs below | see below | none (spec) |

RC-4 local run on this host (`/var/tmp/r51-tmp/rc-pkg/`): `scripts/release_package.sh /var/tmp/r51-tmp/rc-pkg/dist` at 664f8f1c,
rc 0 in 62 s (release build 61 s): `axon 0.1.0 (664f8f1c)`, `static-pie linked`, card sha `b70a0b56…` and card 1, hello rc 0;
`sha256sum -c SHA256SUMS` OK for the tarball and `compat.tsv`; the tarball holds `axon`, `LICENSE`, `card.md`, `compat.tsv`; its
`compat.tsv` equals the debug sweep's byte for byte (rc and first code of all 717 files). Planted: `AXON_GIT_SHA=deadbeef
scripts/release_package.sh <tmp>` rc 1 before building (names `AXON_GIT_SHA`); with `--keep-env` rc 1 after the build: `version
line: got 'axon 0.1.0 (deadbeef)', want 'axon 0.1.0 (664f8f1c)'`; an outdir inside the checkout rc 1, nothing created.
`release.yml` was parsed (PyYAML) and every `run:` passed `bash -n`; the notes extractor was run on a fixture CHANGELOG. No
workflow linter exists here (§9 r4), so the first real observation is the integrator's `release-dry/<sha8>` run.

`[REVISED: build 2026-11-11]` choices the spec left open, each the conservative one:
- M5260's mutant rewords "non-zero" to "nonzero" in the `main` rule line of `CURATED`. M5265's mutant drops the lock comparison
  (`if False and locked.get(name) != cargo_version`) rather than "lock not read", because an unread lock reports every inheriting
  crate and the killer would still see rc 1.
- `release_ledger.py` treats a MAJOR bump (X changes) as satisfying the MINOR rule and requires MINOR and PATCH 0 for it; it never
  happens while 0.x. The oldest release's section runs to the end of `CHANGELOG.md` (the pre-release history), so the query
  answers `1` for it; no compat diff exists for the first release.
- `compat_sweep.py` records a timeout as rc 124, code `TIMEOUT`; a refusal with no JSON error code as `-`. A refusal whose first
  code changed is listed `CODE CHANGED` and never fails the diff (rule 2(a) is about programs that checked clean).
- `release_package.sh` takes the binary path from cargo's `--message-format=json` artifact record, never a `target/` path
  (`harness_binaries.rs` refuses scripts that guess a profile dir). The archive is reproducible (sorted, owner 0, the commit's time,
  `gzip -n`). The compat diff runs in the workflow (it needs the previous release's `compat.tsv` over the network), not in the
  script; in a dry run its flag is the query for the Cargo version, `1` when that heading does not exist yet.
- `workflow_dispatch` publishes only when `dry_run` is false AND the ref is a tag; any branch is a dry run.
- The CHANGELOG batch heading is unnumbered (`## Batch: release-contract ...`); the integrator numbers it at merge.
- RC-2 touches `crates/axon-core/src`, so it needs a PCI theme (`governance/notes/pci-themes/2fb3649c.json`, CLI only, no
  pass/fail moves) and a regenerated `v022-pci-delta.md` (row list +M5260-M5261); the first `gate.sh --lane` failed at S06
  (`pci_delta_note`) until that landed.
