# Axon releases and stability

What a release is, what counts as a breaking change, how the version number moves, and what an upgrade promises a program
written against an older Axon. Axon is pre-1.0: versions follow 0.x semver (decided 2026-11-11). Spec:
`tasks/release-contract-spec.md` (RC-1..RC-5); the checks named below are mechanical, not advisory.

## 1. Release

A release is an annotated tag `vX.Y.Z` on a commit of `main` whose `ci.yml` push run succeeded and for which
`scripts/release_check.sh <sha>` printed `RELEASE-VERIFIED` on the integrator host. `X.Y.Z` is the `[workspace.package] version`
in `Cargo.toml`, the number `axon --version` prints. Only the newest release is supported: there are no backport branches
while Axon is 0.x.

Two keys release a commit. The release workflow (`.github/workflows/release.yml`) checks what GitHub can see: the tagged commit
is on `main`, its `ci.yml` push run on `main` succeeded, and `scripts/release_ledger.py --tag` passes. The integrator checks what
only the integrator host can see: `RELEASE-VERIFIED` from a strict gate receipt bound to the commit. Neither key alone publishes.

Cadence: the integrator tags at most weekly, on a `RELEASE-VERIFIED`, CI-green commit after a batch. The first public tag needs
the human's explicit go. A release publishes one artifact, a static x86_64 Linux (musl) interpreter, with `SHA256SUMS`.
Releases are not signed: `SHA256SUMS` detects corruption, not tampering.

## 2. Breaking change

A change is breaking when a program that checked clean **and** ran without fault (exit 0 or its own exit code, no panic, no
runtime error exit) now

- (a) is refused by `axon check`, or
- (b) gives another stdout or exit code for the same input and environment.

A program that checked clean and then panicked, and is now refused at check, gets a soundness fix: that is not breaking.

Every breaking change gets a CHANGELOG bullet carrying the bold marker `**Behaviour change` in its batch, with any suffix
(`**Behaviour change:**`, `**Behaviour change (axon-ai):**`, `**Behaviour change, arm c:**`). Unbolded text is not a marker.

## 3. Version rule (0.x)

A release whose CHANGELOG section holds a `**Behaviour change` marker line, or whose card version differs from the previous
release's, bumps MINOR and sets PATCH to 0. Any other release bumps PATCH. Comparing two versions therefore tells a model or a
harness whether programs may break: they may when MINOR differs. While 0.x, expect MINOR to move often. The 1.0 rules are a later
decision.

`scripts/release_ledger.py` enforces this rule and rules 4 and 6 (gate P lane, and the `release_ledger` cargo test that CI runs).

## 4. Card version

`axon card --card-version` prints an integer that increases by 1 whenever the text of `axon card` changes. The number is
`CARD_VERSION` in `crates/axon-core/src/card.rs`; it lives beside the card text, never in it, so the card's sha256 changes only
when its text does. `crates/axon-core/card-versions.txt` lists one line per card version, `<N> <sha256 of the card>`; cards before
versioning are card 0 and are not listed. A commit that changes the card text bumps `CARD_VERSION`, appends one line, and says
`card N` in its CHANGELOG bullet; a test fails otherwise. A line that a release heading names is frozen; the unreleased last
line may be rewritten.

A program does not declare the card or edition it was written against (decided 2026-11-11): there is no edition attribute and
no manifest. Rule 5 makes old syntax fail loudly with the new form named instead.

## 5. Migration diagnostics

When syntax or a builtin is removed or renamed, the checker keeps a dedicated diagnostic for the old form, naming the new form,
for at least the next MINOR release. A form is never re-read with a new meaning silently. Precedent: E1316 for the deprecated
`@[contained]` (under `--effects-strict`). A migration diagnostic only adds help; it never refuses a program the new rules accept.

## 6. CHANGELOG release heading

A release inserts the heading

    ## vX.Y.Z - YYYY-MM-DD (card N, <sha8>)

where `N` is the release's card version and `<sha8>` the first 8 hex of line N's sha in `card-versions.txt`, above the batch
headings the release covers. Everything above the newest release heading is unreleased; a tagged release covers every batch, so
nothing unreleased sits above its heading. The first release heading goes above the newest batch heading at the time of the
first release; the history below it is pre-release and exempt from rule 3.

## 7. Compatibility sweep

Each release runs `scripts/compat_sweep.py`: `axon check` of every tracked `.ax` file with the release binary (empty `HOME`, no
`AXON_*` variables, the binary copied away from any `lib/axon`), recording each file's exit code and first error code in
`compat.tsv`, which the release attaches. The sweep is diffed against the previous release's `compat.tsv`: a file that checked
clean before and is refused now fails the release unless the release's CHANGELOG section holds a behaviour-change marker.
Diagnostic text is never compared (it embeds install paths).

The sweep measures rule 2(a) on the tracked corpus only. Rule 2(b), changed run output, stays covered by the golden-output tests
in `crates/axon-core/tests/cli_run.rs` and the batch `run` sweeps; a `run` sweep of the corpus is not mechanised, because many
tracked programs read stdin, the network or the clock.
