# DRAFT — LSP multi-file residue and serde-json lint coverage (L1-L3)

**This is a DRAFT. It MUST pass a Step 1 adversarial review before any build.**

Source: `tasks/opportunities.md` "gaps run (2026-10-15)", items at :2494-2496 (LSP residue) and :2518-2520 (clippy).
Base: `main` 71d6f503 (branch `gaps/int`). Re-measured 2026-10-17 by reading the code and by driving `axon lsp` over
stdio as `tests/lsp_multi_file.rs` does (`AXON_PATH=<dir>`, `AXON_PATH_EXCLUSIVE=1`), using a `git archive` copy of HEAD
in `/var/tmp`, an outside target dir and a `--no-default-features --features serde-json` build. Unobserved = [INFERENCE].

[REVISED: Step 1 review (ReviewLsp, 2026-10-17). Base is now 43fa1857; `git diff 71d6f503 43fa1857` touches only the
four `tasks/*-spec.md` drafts, so every code line number below still holds (spot-checked: `lsp.rs:72-258,710-833`,
`lib.rs:425-430,567-695,1030-1036`, `gate.sh:16-17,360-362,786-798,965,974`). The four lints were re-observed on a
`git archive` of 43fa1857.]

Mission: an `axon lsp` diagnostic from an imported module is anchored on the `use` that brought the module in. The
public byte-offset definition API says which file its answer is in. The serde-json configuration, the only one that
compiles `lsp.rs`, is lint-gated in both the full gate and the fast tier.
[REVISED: was "the public byte-offset definition API says which file its answer is in" → "the server and the
byte-offset API share one definition lookup", reason: L2 is reduced; see L2.]

Shared acceptance: `scripts/gate.sh` shows no new failure against the baseline. `scripts/g01_anchors.py` and
`scripts/v022_g01_mutations.py` stay green. The lib.rs rows M250/M251/M260/M436 are text-anchored inside the loader
and must still match. `axon imports --json` and `imports_digest` stay byte-identical, so G1's binding is untouched. The
`AXON-COMPLETENESS.json` row "MULTI-FILE source identity" gets its gap text updated and `AXON-COMPLETENESS.md` is
regenerated (integrator, last). Also required: a CHANGELOG entry and the two opportunities items marked done or
dropped.

[REVISED: added, reason: `scripts/pci_delta.py --check` (fast tier, `gate.sh:333`) FAILS on any commit under
`crates/axon-core/src` that `governance/notes/pci-themes/` does not classify, and every item here edits `lsp.rs` or
`lib.rs`.] Each lane commit under `crates/axon-core/src` gets `governance/notes/pci-themes/<hash8>.json`
(`{"theme": "gaps LSP residue", "text": "TOOLING (axon lsp only; no change in what any run computes, accepts or
refuses): …"}`, the G3 precedent). For L1a the text must also say "the loader records which entry `use` reached each
module; the digest, `axon imports` and every CLI output are unchanged". The lane runs `python3 tasks/r51-pci-regen.py`,
then `python3 -B scripts/pci_delta.py --check` is green at the lane head. The integrator regenerates once more after
the merge. The note has a merge driver, and the theme files are one per commit, so they do not conflict.

Sequencing: L3 first, so that `lsp.rs` is lint-clean before L2/L1 add code under the new stage. Then L2, then L1. One
lane, because all three edit `lsp.rs`.

## L3 — lint the serde-json configuration (P3)

Current state (measured):
- `cargo clippy --locked --no-default-features --features serde-json -p axon-core -- -D warnings` fails with exactly
  four errors: needless `&mut` at `lsp.rs:115:52` (`resolve_program`) and `lsp.rs:145:29` (`infer_program`); `loop`
  → `while let` at `lsp.rs:274:5`; redundant closure at `lsp.rs:543:53`.

  (The opportunities note omits `--no-default-features`; the command above is the one measured.)
- After those four were fixed in the scratch copy, clippy was clean for the lib and for `--all-targets`. There are no
  further lints in `lsp::tests`, `tests/lsp_multi_file.rs`, `program_json_tests` or main.rs's serde-json paths.
- Why no gate saw them: `pub mod lsp` is `#[cfg(feature = "serde-json")]` (`lib.rs:75-76`), and no axon-core clippy
  run enables that feature:
  - `gate.sh:805`: lib, no-default-features.
  - `gate.sh:965`: `--strict`, no-default-features, all-targets.
  - `gate.sh:974`: `--strict`, default features (codegen).
  - `gate.sh:1302`: smt.
  - `gate.sh:361`: fast tier.
  - `.github/workflows/ci.yml:100`: no-default-features.

  The serde-json stage (`gate.sh:786-788`) runs only `cargo check`.

Change:
- **L3a.** Fix the four lints. No behaviour change: `program` stays `mut` because `load_use_decls` and `fill_captures`
  still mutate it.
- **L3b.** In `gate.sh:787-788`, replace the `cargo check` with:
  `cargo clippy --locked --no-default-features --features serde-json -p axon-core --all-targets -- -D warnings
  || fail "serde-json clippy (axon lsp / axon parse --json)"`.
  Rationale: clippy performs the same type-check over a superset of targets (lib+bin+tests), so it replaces the check
  rather than adding a second pass. Also update the comment at :795-798 ("the check above proves lsp.rs BUILDS") and
  the header list at :16-17.
  [REVISED: coverage claim CONFIRMED. `cargo check` with no target flag checks the default targets, lib + bin `axon`.
  `--all-targets` adds the lib test, bin test, integration test, bench and example targets to those two. clippy runs
  the rustc front end that `check` runs, and `-D warnings` additionally turns plain rustc warnings into errors, so it is
  stricter. Nothing that `check` covered is lost. The clippy-coverage stage (`gate.sh:862`) greps `-p <crate>`
  out of gate.sh, and the new line keeps `-p axon-core`, so it stays satisfied. No anchor in `g01_anchors.py`,
  `v022_g01_mutations.py`, `v022_refusal_coverage.py` or `AXON-COMPLETENESS.json` cites these gate.sh lines.]
- **L3c.** In the fast tier, when `axon-core` is among the changed crates (`gate.sh:360-362`), run the same command.
  Rationale: lanes accept on `--fast`. Without this, an `lsp.rs` lint first appears at the integrator's full gate.

Cost (measured on this host: 32 cores, load average 10-19 from concurrent lanes): cold (deps + axon-core, lib only)
23.7 s; warm re-lint of the lib 6.0 s; warm `--all-targets` 10.7 s.
[REVISED: dev-loop, re-measured by ReviewLsp, load 14-15, fresh target dir. Cold, unfixed: 15.6 s to the four errors
(lib and lib-test both fail). After the fixes, `--all-targets` with deps already checked: 9.4 s, so a cold clean run
costs about 25 s. Warm after `touch lsp.rs`: 2.9 s. The serde-json `cargo check` it replaces: 7.2 s even after the
clippy run, because clippy's workspace artifacts are not shared with `check`.]

- Full gate: net +3-18 s (it removes the 7 s check and adds the 10-25 s clippy). [REVISED: was "about the same wall
  time [INFERENCE]", reason: measured.]
- Fast tier: +3-11 s warm, about 25 s cold, on every run that touches axon-core. The fast tier builds no other
  serde-json dependency set, so it pays the cold cost once per target dir.
- It competes for the target-dir lock (gate stages run serially) and for CPU with parallel lane builds.
- **Budget:** 30 s cold, 12 s warm per run, on CPU plus the target-dir lock. It is re-run by any axon-core change. In
  `--fast` it queues before `cargo test -p axon-core` (`gate.sh:367`), which is minutes long, so it adds under 10% to
  the tier.

Acceptance:
- **Fail-first:** the command fails on 71d6f503 with the four lints (observed above) and exits 0 after L3a.
- **Mutation:** put back `.map(|t| format_type(t))` in `format_fn_hover`. Both the full-gate stage and the fast-tier
  stage must fail, then revert.
- **Fast tier:** `scripts/gate.sh --fast` on a branch that changes `crates/axon-core` prints the serde-json clippy
  stage.

## L2 — `compute_definition` and the server share one lookup (P4) [REVISED: was "`compute_definition` names its file (P3)" → reduced to the dedupe, P4]

[REVISED: reason. The `DefinitionSite` API serves no caller. The only one is a unit test (`lsp.rs:1156`), and no crate
or sibling checkout uses `axon_core::lsp` except `run_lsp`. Its fail-first test and its first mutation exist only to
test the new struct, so they are circular. The wrong-file risk is hypothetical. The duplicate item loop
(`lsp.rs:722-734` vs `:1047-1058`) is real dead weight, though: two lookups that can drift. Canon: keep
`compute_definition`'s signature (`Option<span::Span>`). Its doc comment says the span can index an imported module
(resolve with `span::source_path_of`/`source_map_of`, as `foreign_file` does). `compute_definition_lsp` delegates to it
and keeps its own file resolution (:736-745). `DefinitionSite` is NOT built. The text below is the original draft,
kept for the record. Its Change and Acceptance are replaced by the "Reduced L2" block at the end of this section.]

Current state:
- `pub fn compute_definition(program, source, byte_offset) -> Option<span::Span>` is at `lsp.rs:1040-1064`, under the
  heading "Public byte-offset API (for testing and external consumers)" (:981).
- Its only caller is `lsp_definition_no_crash` (`lsp.rs:1150-1160`), which throws the result away (`let _ = def`).
- Nothing else calls it. The only `axon_core::lsp` use in the workspace and in the sibling axon checkouts under
  `~/projects` is `main.rs:6250` (`run_lsp`).
- The returned `Span` does carry `source: SourceId` (`span.rs:29-41`), which `span::source_path_of` resolves
  (`span.rs:186`). Nothing says so, though. The signature invites a caller to convert the span against `source`, which
  is the G3c wrong-file defect again.
- The server does not use this function. `compute_definition_lsp` (`lsp.rs:710-749`) repeats the same item loop
  (:722-734 vs :1047-1058) and resolves the file itself (:736-745).

Change:
- `compute_definition` returns `Option<DefinitionSite>`, with
  `pub struct DefinitionSite { pub file: Option<String>, pub span: span::Span }`.
  - `file` is `None` when the span indexes `source`.
  - Otherwise `file` is the imported module's path, from `span::source_path_of`.
- `compute_definition_lsp` becomes: position → offset, then `compute_definition`, then JSON. For a foreign site, the
  range comes from `span::source_map_of(site.span.source)`. The duplicate loop is deleted.
- Rationale: one lookup instead of two, and the public result can no longer be read against the wrong text. The API
  stays rather than being deleted because it is the in-process way to test the lookup without spawning a server, and
  L2's test needs that.

Acceptance:
- **Fail-first unit test** in `lsp::tests` (it fails to compile today):
  - Parse `mod lib\nuse lib.{ok_one}\nfn main() -> i64 { ok_one() }\n` with `parse_source_located`.
  - Run `load_use_decls(&mut p, &[dir])` with an explicit search dir, so no env mutation is needed.
  - `compute_definition` at the `ok_one` call returns `file == Some(<dir>/lib.ax)`, and the span converted against
    lib.ax's text is line 0.
  - A local fn returns `file == None`.
- `lsp_definition_no_crash` asserts its result instead of discarding it.
- **Mutation:**
  - `file: None` unconditionally → the new test goes red.
  - `compute_definition_lsp` ignoring `site.file` → `definition_of_an_imported_fn_jumps_into_its_own_file`
    (`lsp_multi_file.rs:149-166`) goes red.

**Reduced L2 (canon, replaces the Change/Acceptance above):**
- `compute_definition_lsp` = position → offset, then `compute_definition(program, source, offset)`, then the existing
  `foreign_file` branch (:736-745). The loop at :722-734 is deleted. Both loops are identical today (verified
  line by line, including the dummy-span skip), so behaviour does not change.
- The doc comment of `compute_definition` says the span may index an imported module's file, which
  `span::source_path_of`/`source_map_of` resolve.
- `lsp_definition_no_crash` asserts `Some` and that the span lies inside `src` (today `let _ = def`).
- Acceptance: no fail-first test exists (this is a refactor). `definition_of_an_imported_fn_jumps_into_its_own_file`
  (`lsp_multi_file.rs:149-166`) and `lsp_definition_no_crash` stay green. Mutation: make the delegating
  `compute_definition_lsp` drop the `foreign_file` branch, and the multi-file test goes red.
- Cost: one `lsp_multi_file` run plus one `lsp::tests` run, both already in the L1 loop.

## L1 — imported diagnostics anchor on the `use` that brought the module in (P3)

Current state:
- `push_diagnostics` (`lsp.rs:753-795`) places a foreign diagnostic at `use_line_range(source, &path)`
  (`lsp.rs:808-833`). That function:
  - scans text lines that start with `use `;
  - turns the segments into `a/b.ax`;
  - matches with `path == rel || path.ends_with("/" + rel)`;
  - falls back to the first `use` line when nothing matches.
- Driven over stdio, this misanchors in three ways. Each time the `relatedInformation` location itself was correct.
  - **(a) Transitive.**
    - main: `mod a\nmod b\nuse a.{fa}\nuse b.{fb}\nfn main() -> i64 { fa() + fb() }\n`
    - b.ax: `mod c\nuse c.{fc}\nfn fb() -> i64 { fc() }\n`
    - c.ax: a type error on its line 2.

    Both of c.ax's diagnostics land on line 2 (`use a`) instead of line 3 (`use b`). The comment at :806-807 ("the
    first `use` brought it in") is wrong whenever that `use` is not the first.
  - **(b) Block comment.** main: `mod lib\n/*\nuse lib.{ok_one}\n*/\nuse lib.{ok_one}\n…`. The diagnostic lands on
    line 2, inside the comment, instead of line 4. Block comments are tokens (`token.rs:228-234`). Raw strings can span
    lines (`token.rs:212`) [INFERENCE: same false match there, not driven].
  - **(c) Path suffix.** `use lib.{ok_one}` (line 2) is followed by `use util.lib.{uf}` (line 3), and the error is in
    `util/lib.ax`. The diagnostic lands on line 2, because `…/util/lib.ax` ends with `/lib.ax`.
- The loader already knows which direct `use` reached each module:
  - `load_use_decls_recorded` calls `load_module_recursive` once per entry `use` path (`lib.rs:674-685`).
  - That call pushes every module it merges into `closure.modules`. Modules are constructed at `lib.rs:1030` and carry
    their `source` (`lib.rs:587-589`).
  - The LSP calls plain `load_use_decls` (`lsp.rs:104`), which throws that record away.

Change:
- **L1a.** `LoadedModule` gains `pub via: Vec<String>`: the entry-file `use` path whose load first merged the module
  (its own path for a direct import).
  - The constructor leaves it empty. `load_use_decls_recorded` fills it from `closure.modules.len()` before and after
    each call, so the first loader wins on a diamond, deterministically in entry order.
  - [REVISED: added, reason: with "first loader wins" alone, L1 REGRESSES a case that is correct today. Entry
    `mod a\nmod c\nuse a.{fa}\nuse c.{fc}`, where a.ax itself does `use c`: the loader merges c inside `use a`'s call
    (`lib.rs:1012-1023`), and `use c`'s own call returns at `already_loaded` (:883), so c's via would be `["a"]` and
    its diagnostic would anchor on `use a`. Today's path match anchors it on `use c` (traced through `lib.rs:674-685`
    and `lsp.rs:808-826`; [INFERENCE] not driven).] **Direct import wins:** after the loop, a module whose `name`
    equals an entry `use` path joined with `::` gets that path as `via`. Only a module that no entry `use` names
    directly keeps the first loader.
  - It is not part of the digest (`lib.rs:615-620`), and it is not in `axon-imports/1`: `closure_modules_json`
    (`main.rs:8449`) picks its fields explicitly.
    [REVISED: CONFIRMED against every reader of `LoadedModule`/`ImportClosure`. Each one picks named fields:
    `digest()` (`lib.rs:615-620`, name/bytes/sealed), the `AXON_REQUIRE_IMPORTS_DIGEST` refusal (`main.rs:993`,
    names), the text `axon imports` (`main.rs:1307`), `closure_modules_json` (`main.rs:8449-8465`), the lock
    comparison (`main.rs:8953-8956`, name/hash), the `check --json` origin (`main.rs:8333,8387`, `module_of`), and the
    lock verify (`main.rs:1601,1700`). `LoadedModule` has no derive (no Debug/Serialize leak), and its only struct
    literal is `lib.rs:1030`; no other crate constructs it. axon-os reads the digest through the `axon imports --json`
    subprocess (`runtime.rs:1219`), so it is unaffected. The loader's mutation anchors M250/M251/M260/M436 lie at
    `lib.rs:936-1062` outside the edited lines. `in_sealed(&candidate),` (M260) must stay the only line of that text,
    so the new field must not be computed with a copy of it. Approval-residue's `imports_digest_mismatch(&ImportClosure)`
    (AR4/AR5, main.rs) reads the same struct and is unaffected by an added field.]
  - Add `ImportClosure::via_of(source) -> Option<&[String]>` beside `module_of` (:624-632).
- **L1b.** `analyse_source` calls `load_use_decls_recorded`. `AnalysisResult` (`lib.rs:425-430`) carries the
  `ImportClosure`, and `push_diagnostics` receives it.
  [REVISED: detail. `push_diagnostics` has three callers (`lsp.rs:339,355,370`). didClose (:370) passes no
  diagnostics and no result, so the new parameter is `Option<&ImportClosure>` (or the `AnalysisResult`), and the
  parse-error result (:89) carries an empty closure.]
- **L1c.** Replace `use_line_range` with a token locator:
  - Run `lexer::Lexer::tokenize(source)` (`lexer.rs:55`).
  - Each `Token::Use` followed by `Ident ((Dot|ColonColon) Ident)*` is compared segment for segment with `via`.
    [REVISED: grammar, reason: `parse_use` (`parser.rs:1838-1856`) also consumes a separator right before `{`
    (`use a.b.{x}`, `use a::{x}`). The locator steps over `(Dot|ColonColon)? LBrace … RBrace`.] Several `use` lines
    with the same path (`use lib.{a}` then `use lib.{b}`): the first one is the anchor, deterministically.
  - The range runs from `use` to the declaration's last token (the closing brace or the last ident). UTF-16 columns
    come from `byte_offset_to_lsp_pos`.
  - The token stream holds no comments and no string contents, which removes (b). Segment equality removes (c).
  - No match is not expected, since the open document parsed and every `use` token is then a `UseDecl`. If it happens,
    the anchor is 0:0, the same as today's empty-source fallback.
  - Rationale for a token scan over an AST span: see H1 (decided: no span this round).

Acceptance:
- **Three fail-first tests** in `tests/lsp_multi_file.rs` that drive the real server, one per fixture (a)/(b)/(c)
  exactly as above, plus one regression guard (d) [REVISED: added]: the direct-import fixture in L1a, error in c.ax,
  anchored on the `use c` line. (d) passes today and must still pass after L1. Each of the four asserts:
  - the anchor line;
  - that `relatedInformation` still names the module's own URI and line;
  - that no other diagnostic is published. The fixtures include `mod` declarations so that E0003 does not fire.
- The existing three tests stay green.
- **Mutation** (each must turn its own test red):
  - `via` ignored, anchoring on the first `use` → (a).
  - The text scan restored → (b).
  - `ends_with` matching instead of segment equality → (c).
  - [REVISED: added] Drop the "direct import wins" pass (first loader only) → (d).
- **G1 untouched:** `axon_imports_reports_the_loaded_closure` (`cli_run.rs:3822`),
  `require_imports_digest_refuses_any_other_closure_before_running` (:3889) and
  `imports_closure_agrees_with_transitive_resolution_on_every_example` (:4003) stay green.

## Dropped on re-measurement

- **D1 — "hover and completion read imported items' spans against the open document."** Neither of them reads a span.
  - `compute_hover_lsp` (`lsp.rs:484-539`) looks the word under the cursor up by name in `fn_sigs`, `struct_fields`
    and `program.items`.
  - `compute_completions` (`lsp.rs:646-686`) ignores `_source` and `_byte_offset` and lists the names in `fn_sigs`.
  - Driven with `use lib.{ok_one, Pt, Color, Show}`, all were correct: hover gave `fn ok_one(a: i64) -> i64`,
    `type Pt = { x: i64, y: i64 }`, `enum Color` and `trait Show`; completion returned 347 items including `ok_one`;
    definition of `Pt`, `Color` and `Show` landed in lib.ax (G3c holds).

## Needs human

- **H1 — add a `span` to `UseDecl` (`ast.rs:161-166`)?** [DECIDED by human, 2026-10-17: no span on `use`
  declarations this round. L1 uses the token scan. Locating E0003/E1800 stays a separate future spec, owned with the
  seal-check work.]

  What a span would buy:
  - The resolver's E0003/E1800 for a `use` carry only a file (`resolver.rs:1630-1670`, `.with_file` and no span).
    Today they surface at LSP 0:0, and `axon check --json` prints no `line` (both observed). A span would locate them.
  - The LSP could read the span instead of scanning tokens.

  What it would cost:
  - **AST JSON.** `axon parse` output gains a `"span"` key in every `UseDecl`. `#[serde(default)]` keeps old JSON
    loadable (the same pattern as `span.rs:36-40`). The only in-repo consumers are the one-way printer
    (`main.rs:1183`) and `program_json_tests` (`lib.rs:202-226`, whose source has no `use`). The constructors to change
    are `parser.rs:1855` and the tests at `resolver.rs:3825` and `resolver.rs:3841`.
  - **The `--seal` check rests on the opposite premise.**
    - `main.rs:6835-6838`: "The candidate's `use` declarations carry no span, so the merged check cannot tell them from
      the operator's".
    - `seal_split.rs:5-10,25`: a spanless merged diagnostic goes to the backstop.
    - The equivalence record M3260 (`v022_g01_mutations.py:11341-11357`, and
      `governance/status/v022-psv-paired-disable.json:2590`) states "a UseDecl carries no span" in its `all_paths`
      argument.
    - With a span, the operator's own E0003 moves from the backstop to the shown diagnostics. The M3260/M3456 cells and
      that argument would need re-running and rewriting [INFERENCE: not weaker, but certified reasoning changes].

  **Recommendation:** keep the AST as it is. L1's token scan fixes every observed LSP misanchor with no schema or
  seal-check impact. Log "locate E0003/E1800 via a `UseDecl` span" as its own spec, owned with the seal-check work.

## Owned files

`crates/axon-core/src/lsp.rs`; `crates/axon-core/src/lib.rs` (`LoadedModule.via`, `ImportClosure::via_of`, the
`AnalysisResult` field); `crates/axon-core/tests/lsp_multi_file.rs`; `scripts/gate.sh`; `CHANGELOG.md`;
`AXON-COMPLETENESS.json`/`.md` (integrator); `tasks/opportunities.md`;
`governance/notes/pci-themes/<hash8>.json` (one per lane commit under `crates/axon-core/src`) [REVISED: added];
`governance/notes/v022-pci-delta.md` (regenerated by the lane; the integrator regenerates after the merge) [REVISED:
added]. Not owned: `ast.rs`, `parser.rs`, `resolver.rs`, `main.rs`, `seal_split.rs`, `.github/workflows/ci.yml`.

## Lane plan

One lane, LSP. Order: L3a → L3b/L3c → L2 (reduced) → L1a → L1b/L1c. All of them edit `lsp.rs`.

| Item | Files |
|---|---|
| L3a | `crates/axon-core/src/lsp.rs` (:115, :145, :274, :543) |
| L3b | `scripts/gate.sh` :16-17 (header), :786-798 (serde-json stage and its comment) |
| L3c | `scripts/gate.sh` :356-362 (fast-tier axon-core branch) |
| L2 | `crates/axon-core/src/lsp.rs` (:710-749, :1036-1064, :1150-1160) |
| L1 | `crates/axon-core/src/lib.rs` (:425-430, :578-590, :624-695, :1030-1036); `crates/axon-core/src/lsp.rs` (:72-112, :253-257, :339-370, :753-833); `crates/axon-core/tests/lsp_multi_file.rs` (new tests, appended) |
| all | `governance/notes/pci-themes/<hash8>.json` (new files), `governance/notes/v022-pci-delta.md` (regenerated), `CHANGELOG.md`, `tasks/opportunities.md`; integrator: `AXON-COMPLETENESS.json`/`.md` |

Cross-spec contention, checked against the other three drafts on 2026-10-17:
- `scripts/gate.sh`. Harness edits comments at :1002, :1321 and :1336 (`harness-hygiene-spec.md:170`). Killgroup adds
  `-p axon-jobctl` to the runtime clippy line at :835-842 (`killgroup-spec.md:44`). This spec edits :16-17, :356-362
  and :786-798. The hunks are disjoint, so git merges them without conflict. The integrator re-checks after the merge
  that the clippy-coverage grep (:862) still sees every `-p`.
- `crates/axon-core/src/*`. Approval AR4/AR5 own `main.rs`/`error.rs`/`env_registry.rs`. Harness owns `main.rs`
  (`try_link_wasm`) and `interp.rs`. Neither touches `lsp.rs` or `lib.rs`. Every lane that commits under
  `crates/axon-core/src` adds its own pci-theme files. The PCI note is regenerated by the integrator after each merge.
- Shared, integrator last: `CHANGELOG.md`, `AXON-COMPLETENESS.json`/`.md`, `governance/notes/v022-pci-delta.md`.
- This spec adds no dependency, so it does not touch `Cargo.toml`/`Cargo.lock` (Killgroup's new crate is
  integrator-owned).

## Gates (lane, before handing to the integrator)

Prefix every command with `CARGO_TARGET_DIR` outside the worktree.

| Gate | Cost (measured unless marked) |
|---|---|
| `cargo clippy --locked --no-default-features --features serde-json -p axon-core --all-targets -- -D warnings` | [REVISED: dev-loop] 2.9 s warm, about 25 s cold (measured). Budget 30 s. Re-run on every `lsp.rs`/`lib.rs` edit. |
| `cargo clippy --locked --no-default-features -p axon-core -- -D warnings` (lib.rs changed) | [INFERENCE] under 1 min warm. Already inside `--fast` (`gate.sh:361`, `--all-targets`), so it is not run separately. |
| `cargo test --locked --no-default-features --features serde-json -p axon-core --test lsp_multi_file` | 24 s to build the serde-json `axon` bin cold; 2.4 s warm (3 tests today, 7 after L1). Budget 1 min. |
| `cargo test --locked --no-default-features --features serde-json -p axon-core --lib lsp::tests` | not measured [INFERENCE: minutes cold; it compiles the whole lib test target]. Budget 5 min cold, 1 min warm. |
| `cargo test --locked --no-default-features -p axon-core --test cli_run` filtered to the three G1 tests above | not measured [INFERENCE: minutes cold; cli_run.rs is over 23k lines]. Budget 8 min cold. Run once, after L1a. |
| `python3 -B scripts/g01_anchors.py`; `python3 -B scripts/v022_g01_mutations.py` (anchor check only); `python3 -B scripts/pci_delta.py --check` | not measured [INFERENCE: seconds]. They also run in `--fast` (`gate.sh:330-333`). |
| Mutation runs (L1 ×4, L2 ×1, L3 ×1) [REVISED: was L1 ×3, L2 ×2] | each one re-runs the single affected test; about 1 min each warm [INFERENCE]. Budget 10 min total. |
| `scripts/gate.sh --fast` (Balanced run mode: the lane's acceptance gate) | [REVISED: added] not measured here; it is dominated by `cargo test -p axon-core --no-default-features` (`gate.sh:374`). L3c adds 3-25 s. It does NOT run `lsp::tests`, `lsp_multi_file` or `cli_run` with serde-json, so the three rows above stay mandatory in the lane. |

The integrator runs `scripts/gate.sh` once, after merge. It is the expensive gate, and its cost is unchanged apart from
L3b (net +3-18 s, measured above).

## Non-goals

- Locating E0003/E1800 (depends on H1, decided against for this round). Definition for impl methods. A `relatedInformation` chain through intermediate
  modules. The `compute_hover`/`compute_hover_lsp` duplication (`lsp.rs:986-1034` vs `lsp.rs:484-539`).
- `use a.{nope}` published no diagnostic in the LSP (observed once; the CLI was not checked). Log it to opportunities.
- E0003 at 0:0 when the entry does not `mod`-declare a module that an imported module `use`s (observed with fixture
  (a) minus `mod c`): this belongs to the approval-residue item on `use` without `mod` (opportunities :2515-2516).
- serde-json clippy in CI (`ci.yml:100`): `gate.sh` is the authority, and CI parity is a separate decision. A
  default-features + serde-json clippy run: it needs LLVM, and the codegen paths do not touch `lsp.rs`.

## Open questions for Step 1

- **Q1.** Carrying the whole `ImportClosure` in `AnalysisResult` keeps every module's bytes for each open document.
  The alternative is a narrow `SourceId → via` map. Default: the closure, because it reuses `module_of`/`via_of` and
  the server holds one result per open URI.
  [REVISED: RESOLVED as canon: the closure. Rationale: the bytes are the module text the span interner already holds,
  one result is kept per URI, and a second map type would duplicate `module_of`'s lookup.]
- **Q2.** Is +11-24 s per fast-tier run that touches axon-core acceptable (L3c)? Default: yes. The same tier already
  runs the full axon-core test suite.
  [REVISED: RESOLVED as canon: yes. Measured +3 s warm, about 25 s cold, under 10% of a tier that runs the whole
  axon-core suite. Without it, a Balanced-mode lane, which accepts on `--fast`, never lints `lsp.rs`.]
