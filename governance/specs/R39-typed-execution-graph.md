# Tech Spec — R39: Typed Execution Graph (formalize Axon's own governance state)

**Spec ID:** `R39-typed-execution-graph` (hardens `governance/BUILD_PROTOCOL.md`'s inner/outer loop
and `governance/EXECUTION_MODEL.md`'s task-DAG/evidence-graph/knowledge-graph sections — currently
markdown + grep + `scripts/verify_all_specs.sh` — into typed schemas + a real, queryable state store)
**Status:** Implementing (Slices 1-3, 5 landed 2026-07-18; Slice 4 landed 2026-07-20, re-scoped) — §12 Q1 (build now vs. wait) resolved
speculatively in favor of "build the cheap spike": Slice 1 (schema + parser) took the form of a
`--export-jsonl` mode added to the *existing* `scripts/verify_all_specs.sh` (not a new, separately-
drifting parser — it reuses the exact same per-spec extraction pass that already computes the
validation findings), writing one JSON record per spec (schema `axon-gov-spec/1`) to
`governance/state/specs.jsonl` (gitignored — a regeneratable index, not a second source of truth,
per §12 Q2). Slice 2 (`scripts/r39_slice2_validate.sh`) ports the bash validator's checks to read
that typed store instead of re-parsing markdown; a synthetic-fixture regression gate
(`scripts/r39_slice2_gate.sh`) proves it finds the same bugs the bash version finds, on both the
real (clean) tree and a fixture with 4 deliberately injected findings. Sizing Slice 2 surfaced and
fixed a real, unrelated bug in `verify_all_specs.sh` itself: under this host's heavy load (~50
load-average on 32 cores) the dangling-edge check's per-reference forked `grep -qx` calls could
transiently fail, producing a false "unknown spec" finding on a spec id that in fact existed —
fixed by replacing the forked lookup with a pure-bash associative-array membership check (0
findings across 8 consecutive reruns post-fix, vs. flaky pre-fix). Slice 3 (live re-run) extends
`verify_all_specs.sh --run TARGET` with a `--record-jsonl PATH` flag: each evidence command it
actually re-runs gets one JSON record (schema `axon-gov-verify/1`: spec, command, result,
exit_code, ISO-8601 UTC timestamp, short git commit hash) appended to a sidecar file, deliberately
kept separate from `specs.jsonl` (which stays a pure function of the markdown tree; a verify-run
record is evidence of an action taken, not something re-derivable from the tree alone). No separate
`axon-gov` binary yet (§12 Q3 still open) — Slice 3 continues Slices 1-2's pattern of extending the
existing bash validator. **Slice 5 (DAG cycle + blocked-by staleness, landed out of order ahead of
Slice 4 — cleanly specified where Slice 4's own "strict superset of hand-maintained
SESSION_STATUS.md prose" gate needed design work first)**: `scripts/r39_slice5_dag_check.sh`
builds one directed graph from every spec's `depends_on`/`blocks` edges and DFS-detects cycles, and
separately checks every non-empty `blocked_by` matching `R<id> §<N> Q<k>` against whether that
question is actually marked resolved in the target spec's own §N section. Sizing/building it found
and fixed two real bugs in the check's own first draft (not in any spec): a naive substring match
on "resolved" false-positived on R40's real §12 Q1 text ("**Unresolved**, deliberately"), and a
bullet-matcher that assumed every spec labels its open questions `**Qn**` in bold, when R37/R38 in
fact use plain `1./2./3.` numbering — both fixed before landing (word-boundary match excluding
"un-"; a plain-numbered-item fallback when no bold `**Qn**` labels exist in a section). Slice 4
(rendered `axon-gov status`) remains not started. Scoped to Axon's *own* governance (this repo's
specs/requirements/build state), not a general-purpose product for other projects — see §5
non-goals and R40 for that larger, explicitly-deferred question.
**Risk class:** Additive (governance tooling; changes how status gets recorded and checked, not
what gets built; zero runtime/compiler/TCB surface)
**Author / date:** cklaus (research agent draft, prompted by a user design proposal), 2026-07-18

```spec-meta
id: R39-typed-execution-graph
status-claim: Implementing
depends-on: none
blocks: none
blocked-by: none
supersedes: none
related: R40-ai-native-research-compiler
conflicts-with: none
reserves: none
evidence: scripts/r39_slice1_gate.sh; scripts/r39_slice2_gate.sh; scripts/r39_slice3_gate.sh; scripts/r39_slice4_gate.sh; scripts/r39_slice5_gate.sh
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

1. **Schema + parser — LANDED 2026-07-18.** `spec-meta` front-matter (§13/§14 table parsing
   deferred to Slice 2 below — genuinely harder to parse reliably from free-form markdown tables,
   and not required by this slice's own named gate). Implemented as `scripts/verify_all_specs.sh
   --export-jsonl PATH`, reusing its existing per-spec extraction pass (not a second, independently-
   drifting parser) to write one JSON record per spec (schema `axon-gov-spec/1`) to
   `governance/state/specs.jsonl` (gitignored — JSONL over SQLite per §12 Q2, a regeneratable index
   not committed to the repo). **Gate (`scripts/r39_slice1_gate.sh`, 9 checks):** parses 100% of
   specs that carry `spec-meta` without error (61/61 spec files produce exactly one well-formed
   JSON line each, zero silently dropped); specs without it are recorded as `pre_convention:true`
   with a null `id` (38, matching `verify_all_specs.sh`'s own count exactly — cross-checked, not
   just asserted); a known spec's edges (R33's `depends_on`) spot-checked to round-trip correctly.
   KNOWN LIMITATION (inherited from the shared extraction, not introduced by the export): a
   multi-line spec-meta field value (e.g. a wrapped `reserves:`/`blocked-by:` line) is captured
   from its first line only — documented in the script's own header, not silently hidden.
2. **Validator, ported from `verify_all_specs.sh` — LANDED 2026-07-18.** `scripts/r39_slice2_validate.sh
   [STORE_JSONL]` reimplements the existing checks (duplicate numbers with the same `KNOWN_DUAL`
   allowlist, spec-meta id vs. filename, status-claim vs. prose mismatch, non-Draft-requires-evidence,
   dangling evidence-script paths, dangling `depends-on`/`blocks`/`supersedes`/`conflicts-with`/`related`
   references — `blocked-by` intentionally excluded from the dangling-edge check, matching the bash
   version, since it may legitimately name an open question rather than a spec id) against the typed
   store's already-extracted fields, reading no markdown directly. **Gate
   (`scripts/r39_slice2_gate.sh`, 10 checks):** (A) on the real tree, the bash validator and the ported
   validator produce the exact same set of `FINDING:` lines (0 vs. 0, since the tree is clean) and
   agree on the pre-convention count (38 vs. 38); (B) on a synthetic scratch fixture with 4
   deliberately injected real bugs (a dangling `depends-on`, a status-claim/prose mismatch, a missing
   evidence script, and a NEW duplicate spec number not on `KNOWN_DUAL`), both validators catch all 4
   and produce byte-identical finding sets (module one cosmetic normalization: the bash version's
   duplicate-number message lists filenames with `.md`, the typed store's `file` field is deliberately
   bare — same finding, stripped before comparison); a real `KNOWN_DUAL` prefix (R21) is confirmed to
   still warn rather than finding in both, proving the allowlist ported correctly rather than being
   silently dropped. Sizing this slice found and fixed an unrelated, real bug in
   `verify_all_specs.sh` itself: under heavy host load the dangling-edge check's per-reference forked
   `grep -qx` could transiently fail and read as a false "unknown spec" finding — fixed with a
   pure-bash associative-array lookup (0 forks instead of hundreds), 8/8 clean reruns post-fix at the
   same load level that produced 3 different phantom findings across 3 pre-fix runs.
3. **Live re-run — LANDED 2026-07-18.** `scripts/verify_all_specs.sh --run TARGET --record-jsonl
   PATH` (no separate `axon-gov` binary yet, per §12 Q3): for each evidence command actually
   re-run, appends one JSON record (schema `axon-gov-verify/1`: `spec`, `command`, `result`
   [`PASS`/`FAIL`/`SKIP_NO_EVIDENCE`/`SKIP_NOT_A_SCRIPT_POINTER`], `exit_code`, ISO-8601 UTC `ts`,
   short git `commit`) to a sidecar file kept separate from `specs.jsonl` (a verify-run record is
   evidence of an action taken, not re-derivable from the markdown tree the way `specs.jsonl` is).
   **Gate (`scripts/r39_slice3_gate.sh`, 11 checks):** a synthetic PASS-fixture spec records
   `result=PASS, exit_code=0`; a synthetic FAIL-fixture spec (exit 7) records `result=FAIL,
   exit_code=7` (the real code, not just "nonzero"); every record's timestamp and commit hash are
   well-formed; **running it against R32, R33, and R34 for real reproduces PASS for all three**,
   matching this session's own hand-verified record of their acceptance gates being green;
   `--record-jsonl` without `--run` is a hard usage error (exit 2), not a silent no-op.
4. **`axon-gov status`: the render — RE-SCOPED 2026-07-20 (design-only correction, no code yet
   in this edit).** The original gate ("a strict superset of what `SESSION_STATUS.md` currently
   records by hand") doesn't hold up: `SESSION_STATUS.md` is 700+ lines of iteration-by-iteration
   *narrative* — decision rationale, "why we chose X over Y," investigation findings — and the
   typed store (Slices 1/3) only carries *structured* facts (spec id/status-claim/prose-status,
   evidence commands, verify-run results). A generated file can be a strict superset of the
   store's structured facts; it cannot reproduce hand-written narrative reasoning, and trying to
   would mean either (a) making the store carry free-text narrative too — which defeats the
   "typed, queryable" point of R39 in the first place — or (b) the render silently failing its own
   gate forever. Neither is the fix; the gate was wrong, not the render.
   **Corrected scope:** `GOVERNANCE_STATUS.md` is a NEW, separate, purely-generated artifact
   (`scripts/r39_render_status.sh`, reading `governance/state/specs.jsonl` + the Slice-3
   verify-results sidecar if present) — it does **not** replace or subsume `SESSION_STATUS.md`,
   which keeps recording narrative exactly as it does today. What it exists for: every STRUCTURED
   status claim currently only reachable by hand-grepping `REQUIREMENTS.md` + 60+ spec files +
   scattered `SESSION_STATUS.md` mentions, in one place, sourced ONLY from the store (never
   hand-typed, so it cannot silently drift from what the store actually says) — id, status-claim,
   prose-status-word, whether they match (the exact mismatch class `verify_all_specs.sh`/
   `r39_slice2_validate.sh` already catch), and the most recent verify-run result/timestamp/commit
   if one exists. **Gate:** the render is a strict superset of the store's structured facts (every
   spec-meta-carrying spec appears exactly once with its real fields; nothing hand-added, nothing
   silently dropped); a synthetic fixture with an injected status-claim/prose mismatch shows up
   flagged in the render, matching what the existing validators already report.
   **Landed 2026-07-20**: `scripts/r39_render_status.sh [STORE_JSONL] [--verify-results PATH]
   [--out PATH]` renders `governance/state/GOVERNANCE_STATUS.md` (gitignored, regenerated not
   committed, matching `specs.jsonl`'s own precedent). Gated by `scripts/r39_slice4_gate.sh` (8
   checks): every spec-meta-carrying spec appears exactly once with real fields (nothing dropped,
   nothing invented); the spec-with-meta and pre-convention counts both match
   `verify_all_specs.sh`'s own report exactly; a synthetic status-claim/prose mismatch renders
   flagged, matching what `verify_all_specs.sh` itself reports for the same fixture (no second,
   differently-worded notion of "wrong"); a clean spec doesn't false-positive; when a Slice-3
   verify-results sidecar has multiple records for one spec, the render shows the MOST RECENT one,
   not a stale earlier result; regenerating twice against an unchanged store produces
   byte-identical output except the timestamp line (a pure function of the store, no hidden
   accumulated state).
5. **DAG cycle + blocked-by staleness — LANDED 2026-07-19 (out of numeric order, ahead of Slice
   4).** `scripts/r39_slice5_dag_check.sh [STORE_JSONL] [--specs-dir DIR]` builds one directed
   "must-happen-before" graph from every spec's `depends_on`/`blocks` edges (already-typed arrays
   from Slice 1's store — no markdown re-parsing for this half) and 3-color DFS-detects cycles.
   Separately, for every non-empty `blocked_by` matching `R<id> §<N> Q<k>`, it reads the *target*
   spec's markdown directly (deliberately — "is this question marked resolved in prose" is not
   information any landed slice has typed yet, so this is genuinely new extraction, not a second
   parser duplicating existing logic) to isolate the `Q<k>` bullet's text and check whether it's
   actually marked resolved. **Gate (`scripts/r39_slice5_gate.sh`, 10 checks):** the real tree is
   clean and R36's real `blocked-by: R36 §12 Q1` is correctly reported as still-blocking (Q1
   unresolved) — the spec's own named example; a synthetic 2-spec `depends-on` cycle and a
   synthetic `blocks`-edge cycle are both rejected; a synthetic blocked-by naming an
   already-resolved question is flagged stale; a synthetic blocked-by naming a genuinely
   unresolved question is correctly NOT flagged; a synthetic target using plain `1./2./3.`
   numbering (no bold `**Qn**` labels) still resolves a `Qn`-labeled blocked-by correctly.
   **Building this found two real bugs in the check's own first draft** (against real specs, not
   contrived): a naive `grep -qi resolved` substring match false-positived on R40's actual §12 Q1
   text ("**Unresolved**, deliberately") — fixed with a word-boundary match excluding an "un-"
   prefix; and the bullet-matcher assumed every spec bold-labels its questions `**Qn**`, but
   R37/R38 in fact use plain `1./2./3.` numbering — fixed with a plain-numbered-item fallback.
   Slice 4 (`axon-gov status`: the render) was not started at the time — its own gate ("a strict
   superset of what `SESSION_STATUS.md` currently records by hand") needed design work first
   (`SESSION_STATUS.md` is 500+ lines of iteration-by-iteration narrative reasoning, not just
   structured facts a typed store can render). Landed 2026-07-20 with the gate corrected — see
   above.

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

- **Slice 1 landed 2026-07-18**: every spec with `spec-meta` parses into `governance/state/specs.jsonl`
  (`scripts/r39_slice1_gate.sh` ALL CHECKS PASSED).
- **Slice 2 landed 2026-07-18**: the ported validator (`scripts/r39_slice2_validate.sh`) finds every
  real bug the current `verify_all_specs.sh` finds, confirmed by a side-by-side run reporting
  identical findings on the current (clean) tree AND on a synthetic fixture with 4 injected real
  bugs (`scripts/r39_slice2_gate.sh` ALL CHECKS PASSED, 10/10).
- **Slice 3 landed 2026-07-18**: live evidence re-run against R32, R33, R34 reproduces this
  session's hand-verified results exactly (`scripts/r39_slice3_gate.sh` ALL CHECKS PASSED, 11/11).
- **Slice 5 landed 2026-07-19** (ahead of Slice 4 — see §6 slice 4 for why): DAG cycle detection
  and blocked-by open-question staleness checking, including R36's own real example
  (`scripts/r39_slice5_gate.sh` ALL CHECKS PASSED, 10/10).
- **Slice 4 landed 2026-07-20**, re-scoped: `GOVERNANCE_STATUS.md` is a purely-generated
  structured-facts index (NOT a superset of `SESSION_STATUS.md`'s narrative — see §6 slice 4 for
  why that original gate didn't hold) (`scripts/r39_slice4_gate.sh` ALL CHECKS PASSED, 8/8). All
  five R39 slices are now landed.
- Documented, honestly, in `governance/EXECUTION_MODEL.md` as the "typed" successor to its own §1-3
  prose sketch — not a separate, competing document.

### 12. Open questions

- **Q1 (the decisive fork): build this now, or continue with markdown + `verify_all_specs.sh`?**
  **Resolved 2026-07-18, speculatively, per this section's own suggestion**: built Slice 1 as the
  cheap spike (zero behavior change to the existing bash validator — it's the same script, one new
  opt-in flag). Whether to continue investing in Slices 2-5 remains a live, softer question — Slice
  1 alone already gives a queryable typed index; Slice 2 (porting the validator itself onto that
  index) is real additional engineering surface with no new evidence yet that it's worth it beyond
  what Slice 1 + the existing bash checks already provide together.
- **Q2: SQLite vs. JSONL-alongside-markdown?** JSONL is simpler and keeps markdown as the
  source-of-truth (JSONL is a generated index, discardable/regeneratable); SQLite is more queryable
  but adds a binary artifact to a text-only repo. Leaning JSONL for Slice 1, revisit if query
  complexity (Slice 5's DAG traversal) demands it.
- **Q3: does `axon-gov` live in this repo (`scripts/` or a new `tools/axon-gov/` crate) or is it
  itself an Axon program** (dogfooding the language on its own governance data)? Leaning "an Axon
  program" is the more on-thesis choice long-term (Axon already has `Dict`, JSON parsing, and file
  I/O) but a Python/Rust prototype for Slice 1 is lower-risk to validate the schema shape first.
