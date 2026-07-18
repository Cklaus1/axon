# Tech Spec — R39: Typed Execution Graph (formalize Axon's own governance state)

**Spec ID:** `R39-typed-execution-graph` (hardens `governance/BUILD_PROTOCOL.md`'s inner/outer loop
and `governance/EXECUTION_MODEL.md`'s task-DAG/evidence-graph/knowledge-graph sections — currently
markdown + grep + `scripts/verify_all_specs.sh` — into typed schemas + a real, queryable state store)
**Status:** Draft (2026-07-18) — PRD, not yet founder-committed. Scoped to Axon's *own* governance
(this repo's specs/requirements/build state), not a general-purpose product for other projects — see
§5 non-goals and R40 for that larger, explicitly-deferred question.
**Risk class:** Additive (governance tooling; changes how status gets recorded and checked, not
what gets built; zero runtime/compiler/TCB surface)
**Author / date:** cklaus (research agent draft, prompted by a user design proposal), 2026-07-18

```spec-meta
id: R39-typed-execution-graph
status-claim: Draft
depends-on: none
blocks: none
blocked-by: R39 §12 Q1 (founder go/no-go — see below; this is genuinely optional tooling, not a
  requirement gate)
supersedes: none
related: R40-ai-native-research-compiler
conflicts-with: none
reserves: none
evidence: none (Draft; scripts/verify_all_specs.sh is the pre-existing lightweight version this
  spec proposes hardening, already exercised throughout the 2026-07-18 session)
```

> **One-line scope:** replace "a human or AI reads REQUIREMENTS.md, ROADMAP.md, and 56 spec files
> and hopes nothing has drifted" with "a schema-validated store that makes a stale claim, a
> dangling cross-reference, or an unverified status a mechanical, always-on check" — for *this
> repo's own* governance, not as a product other teams install.

---

### 1. Motivation

This session ran ~20 consecutive iterations of `governance/ASI_BUILD_LOOP.md`. A disproportionate
fraction of the *real, load-bearing* work found was not new features — it was **truth drift**:
R28's audit ledger silently only logging AI calls (found by hand, no gate caught it), R17/R21/R22/
R23/R26/R27/R28/R29/R31/R32/R12/R14 all carrying a stale "Draft" header on specs whose own bodies
(or `REQUIREMENTS.md`) already said "Landed," two dangling `governance/specs/` filenames in R31
and R34's own dependency lists, a gate script (`r32_acceptance_gate.sh`) whose TLC success/failure
check had a false-negative bug that had never been exercised because the check was always SKIPPED,
and R30's gate being red under contention and reported as "hung" when it actually completes fine
in isolation. Every one of these was found by a human/agent manually reading files and re-running
commands — `scripts/verify_all_specs.sh` (built mid-session, `EXECUTION_MODEL.md` §3-4) already
mechanizes *some* of this (duplicate spec numbers, spec-meta first-word/prose-status mismatches,
dangling `evidence:` script paths) but is a ~200-line bash/awk script operating on markdown regex,
not a typed, queryable state store — it can't answer "which specs are blocked on an open question,"
can't validate a DAG for cycles, and treats every claim as a flat string rather than a typed,
versioned assertion with its own re-run history.

The user's proposed architecture (objective/claim graph, decision graph, experiment graph, task
DAG, evidence graph, knowledge graph, provenance graph, with confidence as typed per-edge metadata
rather than a standalone score) is the right shape for closing this gap **for Axon's own governance
specifically** — it is, in fact, almost exactly what `EXECUTION_MODEL.md` §1-3 already sketches in
prose (task DAG = spec §13, evidence graph = spec §14 + REQUIREMENTS.md's evidence column,
knowledge graph = `spec-meta` front-matter), just not yet typed, validated by a real schema, or
backed by anything richer than grep.

### 2. Requirement link

No existing `REQUIREMENTS.md` row names this directly — it is process/tooling for how every other
row's status gets recorded and checked, closest in spirit to `BUILD_PROTOCOL.md`'s own "Gate 7:
VERIFY" and "the OUTER LOOP" sections, which this spec's §6 phased slices extend rather than
replace.

### 3. The product story (target state)

```bash
# Instead of grepping REQUIREMENTS.md and re-reading 56 spec files by hand:
axon-gov status                          # every spec's claim, its evidence command, last-run result, staleness age
axon-gov status --stale-only             # only claims whose evidence hasn't been re-run since the cited commit
axon-gov verify R32                      # re-run R32's evidence command NOW, diff against its claim, exit non-zero on drift
axon-gov graph --blocked-by-open-question  # every DAG node whose blocked-by names a §12 Q, not a spec
axon-gov graph --cycles                  # DAG validation: are there any dependency cycles across spec-meta depends-on?
axon-gov claim R28 "capability audit ledger records all effect classes"
                                          # ^ typed claim with a verify command, checked in CI, not just asserted in prose
```

A `truth:` correction commit becomes: edit the claim's evidence result via the tool (which re-runs
the command and records the real output), not hand-editing a markdown table cell and hoping the
prose stays honest until the next manual sweep.

### 4. Architecture — the seven subgraphs, scoped down to what this repo actually needs

Following the user's model, but pruned to what R39 (governance for *one* repo, not a general
research platform) actually requires — sections in brackets are explicitly **not** built in R39
(they belong to R40's larger, un-committed scope):

| Subgraph | R39 scope | Backing today (informal) |
|---|---|---|
| Objective / claim graph | `REQUIREMENTS.md` rows + spec `Requirement link` sections, typed | prose + a markdown table |
| Task DAG | spec §13 tables (node / depends-on / gate / status) | markdown tables, `verify_all_specs.sh` checks names only |
| Evidence graph | spec §14 tables (claim / verify command / expected / last-verified / result) | markdown tables, not re-run automatically |
| Knowledge graph | `spec-meta` front-matter (id/depends-on/blocks/related/reserves) | markdown fenced block, parsed by regex |
| [Decision graph] | not built — Axon's specs already carry "§12 Open questions" informally; formalizing decision *state machines* (PROPOSED→REVIEWED→APPROVED…) is R40 scope, not needed to close this session's actual failure modes |
| [Experiment graph] | not built — Axon's specs are feature specs with acceptance tests, not scientific experiments with baseline/treatment/seeds; this concept doesn't map onto what this repo does |
| [Provenance graph] | partially exists already (git commit hashes cited in every evidence row) — formalizing it as a queryable graph (not just a hash in a markdown cell) is a nice-to-have, deferred to a later slice if R39 proves valuable |

### 5. Scope / non-goals (explicit, so this doesn't creep into R40)

- **Not** a natural-language front end. No parser for prose intent, no LLM-authored mutation
  proposals validated against a schema. Specs are still hand/agent-authored markdown; R39 only
  formalizes the three structures (`§13`/`§14`/`spec-meta`) that already exist as *conventions*
  into something a real parser + validator can check, plus a thin CLI to query/re-run them.
- **Not** a general-purpose tool for other repos. No plugin system, no config format for "bring
  your own project schema." If it proves valuable, generalizing it is R40's question, not this
  spec's.
- **Not** an event-sourced database, not a graph database. A SQLite file (or, for the true MVP, a
  set of JSONL files alongside the existing markdown, generated *from* the markdown by parsing the
  existing `§13`/`§14`/`spec-meta` conventions) is sufficient at this repo's scale (~90 specs).
- **Not** a replacement for human-/agent-authored spec prose. The narrative sections of a spec
  (Motivation, product story, open questions) stay exactly as they are; only the three
  already-structured tables get a real parser and validator.
- **Not** a decision/experiment/confidence-scoring system. This repo builds features against
  acceptance tests, not scientific hypotheses against seeded experiments — importing that
  vocabulary wholesale would be solving a problem this repo doesn't have.

### 6. Phased slices (each independently shippable, gated)

1. **Schema + parser.** Define JSON Schema (or a small Rust/Python struct set) for the three
   existing conventions (`spec-meta` front-matter, §13 DAG rows, §14 evidence rows) exactly as
   `SPEC_TEMPLATE.md` already documents them. Parse all ~90 specs into a single `governance.db`
   (SQLite) or `governance/state/*.jsonl`. **Gate:** parses 100% of specs that already carry
   `spec-meta` (the ~40 done this session) without error; specs without it are cleanly recorded as
   `pre-convention` (matching `verify_all_specs.sh`'s existing behavior, not a regression).
2. **Validator, ported from `verify_all_specs.sh`.** Reimplement the existing bash/awk checks
   (duplicate numbers, dangling `depends-on`/`related`/`blocks` references, status-claim vs. prose
   mismatch, dangling evidence-script paths) against the typed store instead of regex. **Gate:**
   byte-identical findings to the current `verify_all_specs.sh` on today's tree (a regression test:
   the new validator must not find *fewer* real bugs than the bash version, and any *new* finding
   must be manually confirmed real before the port is trusted).
3. **`axon-gov verify <spec>`: live re-run.** For a spec with a well-formed `evidence:` command,
   actually execute it, capture exit code + a result summary, and update the store's `last-verified`
   timestamp + commit hash automatically (replacing "re-verified 2026-07-18" prose with a real,
   automatically-dated field). **Gate:** running it against R32/R33/R34 (already re-verified by hand
   this session) reproduces the same pass/fail/skip results recorded in their evidence tables.
4. **`axon-gov status`: the render.** Regenerate a `GOVERNANCE_STATUS.md` snapshot from the
   authoritative store (mirroring the user's "the LLM should not rewrite RESEARCH_STATUS.md
   directly; the harness generates it from validated state" principle) instead of `SESSION_STATUS.md`
   being hand-maintained prose. **Gate:** the generated file is a strict superset of what
   `SESSION_STATUS.md` currently records by hand, and nothing in it can silently drift from the
   store (it's regenerated, not edited).
5. **DAG cycle + orphan detection.** Validate `depends-on`/`blocks`/`blocked-by` edges form a DAG
   (no cycles) and that every `blocked-by` naming an open question (`R36 §12 Q1`) is checked against
   whether that question has actually been marked resolved in the spec's own §12 section. **Gate:**
   a synthetic test fixture with an intentional cycle is rejected; R36's real `blocked-by: R36 §12
   Q1` is correctly reported as still-blocking (Q1 unresolved).

Slices 1-3 alone would have mechanically caught 4 of the ~8 real bugs this session found by hand
(the stale headers, the dangling filenames, the false-negative gate bug — not the R28 silent-log
bug itself, which required reading actual Rust source, something outside this spec's scope by
design).

### 7. Biggest open risk / cost

**Maintenance tax vs. value delivered.** A schema + parser + validator + regenerated status file is
real, ongoing engineering surface (schema migrations when a spec's shape changes, parser bugs,
false-positive validator findings eroding trust the same way a flaky test does). The honest test:
would Slices 1-3 alone, run *today* against the tree as it existed before this session's manual
sweep, have found the R17/R21-R34/R12/R14 stale-header class and the two dangling filenames
*without* a human/agent first reading every file by hand? Yes for the mechanical parts (status-claim
vs. prose mismatch is exactly what the existing bash validator already does, ported). No for the
"is the underlying code actually landed" question — that always requires reading real source/running
real tests, which no schema can automate away. **This spec closes documentation-drift bugs, not
implementation-drift bugs.** That's a real, bounded value proposition, not "solves the staleness
problem" — state it as such, don't oversell it.

### 8. Error codes / exit codes

None reserved — `axon-gov` is a new, separate CLI tool (governance tooling, not the Axon compiler
or runtime), with its own exit-code space independent of Axon's E1xxx/E2xxx/E3xxx blocks.

### 9. Acceptance (definition of done for the R39 row)

- Slices 1-2 landed: every spec with `spec-meta` parses into the typed store; the ported validator
  finds every real bug the current `verify_all_specs.sh` finds, confirmed by a side-by-side run
  reporting identical findings on the current tree.
- Slice 3 landed: `axon-gov verify` against at least R32, R33, R34 reproduces this session's
  hand-verified results exactly.
- Documented, honestly, in `governance/EXECUTION_MODEL.md` as the "typed" successor to its own §1-3
  prose sketch — not a separate, competing document.

### 12. Open questions

- **Q1 (the decisive fork): build this now, or continue with markdown + `verify_all_specs.sh`?**
  Not resolved. The case for now: this session's own failure record is the strongest evidence this
  spec could ever have, freshly gathered, and `verify_all_specs.sh` already proves the lightweight
  version pays for itself. The case for waiting: it's real engineering time spent on tooling instead
  of the language/compiler/product itself, and the bash version, while cruder, has so far caught
  everything Slices 1-2 would additionally catch except by having a human explicitly ask it to. This
  is a genuine "smallest reversible step" candidate — Slice 1 alone (schema + parser, no behavior
  change to the existing bash validator) is cheap enough to build speculatively and evaluate before
  committing to Slices 2-5.
- **Q2: SQLite vs. JSONL-alongside-markdown?** JSONL is simpler and keeps markdown as the
  source-of-truth (JSONL is a generated index, discardable/regeneratable); SQLite is more queryable
  but adds a binary artifact to a text-only repo. Leaning JSONL for Slice 1, revisit if query
  complexity (Slice 5's DAG traversal) demands it.
- **Q3: does `axon-gov` live in this repo (`scripts/` or a new `tools/axon-gov/` crate) or is it
  itself an Axon program** (dogfooding the language on its own governance data)? Leaning "an Axon
  program" is the more on-thesis choice long-term (Axon already has `Dict`, JSON parsing, and file
  I/O) but a Python/Rust prototype for Slice 1 is lower-risk to validate the schema shape first.
