# Tech Spec — R39: Typed Execution Graph (formalize Axon's own governance state)

**Spec ID:** `R39-typed-execution-graph` (hardens `governance/BUILD_PROTOCOL.md`'s inner/outer loop
and `governance/EXECUTION_MODEL.md`'s task-DAG/evidence-graph/knowledge-graph sections — currently
markdown + grep + `scripts/verify_all_specs.sh` — into typed schemas + a real, queryable state store)
**Status:** Implementing (Slice 1 landed 2026-07-18; Slices 2, 3, 5 and the re-scoped Slice 4
all landed 2026-07-19 — per git commit dates 03d2097/407c078/09e045d/5bddc6e. Corrected
2026-07-31: an earlier same-day correction fixed only Slice 5's date and left Slices 2-3
mis-grouped under 07-18 and Slice 4 under a fabricated 07-20; no R39 commit exists on 07-20
at all. Extended 2026-07-31 by an ASI-trajectory review: two new slices are SCOPED BUT NOT
STARTED — Slice 6 (ground-truth checks against git rather than markdown) and Slice 7 (gate
provenance + a hash-chained verify sidecar, closing the §7.2 laundering path); §7 gains an
explicit threat model whose central limit is that R39 as landed assumes a CARELESS author, not one
optimizing for the acceptance gate) — §12 Q1 (build now vs. wait) resolved
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
(rendered `axon-gov status`) landed later the same day, re-scoped — see §6. Scoped to Axon's *own* governance (this repo's
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
>
> **Correction 2026-07-31: "always-on" is not yet true.** Nothing runs any R39 check
> automatically — `scripts/gate.sh` and `.github/workflows/` (ci.yml/ios.yml/tee.yml) contain no
> reference to `verify_all_specs.sh` or any `r39_*` script; the store is gitignored and
> regenerated only on manual invocation. The checks are mechanical *when a human/agent chooses to
> run them* — the exact failure mode §1 documents as this spec's motivation. Wiring them into
> gate.sh/CI is now a named residual item, §12 Q4(c).

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

> **Correction 2026-07-31:** the `claim` verb above is STRUCK from R39 scope. No slice built it,
> and it is architecturally incompatible with the landed store design: `specs.jsonl` is a pure
> function of the markdown tree (§12 Q2, §6 Slice 3), so nothing may write claims into the store
> directly. The way to register a claim is what the next paragraph already describes — author it
> in the spec's markdown, and the tooling re-derives and re-runs it. A tool-mediated write path
> would be a Q3-scale redesign (a second writable store or markdown mutation), not an R39 slice.
> Additionally (correction 2026-07-31), the "checked in CI" phrase above is aspirational: no R39
> check runs in CI or `scripts/gate.sh` today — see the one-line-scope correction and §12 Q4(c).

A `truth:` correction commit becomes: edit the claim's evidence result via the tool (which re-runs
the command and records the real output), not hand-editing a markdown table cell and hoping the
prose stays honest until the next manual sweep.

### 4. Architecture — the seven subgraphs, scoped down to what this repo actually needs

Following the user's model, but pruned to what R39 (governance for *one* repo, not a general
research platform) actually requires — sections in brackets are explicitly **not** built in R39
(they belong to R40's larger, un-committed scope):

| Subgraph | R39 scope | Backing today (informal) |
|---|---|---|
| Objective / claim graph | `REQUIREMENTS.md` rows + spec `Requirement link` sections, typed — **NOT DELIVERED by Slices 1-5 (correction 2026-07-31): no script parses `REQUIREMENTS.md`, and the `axon-gov-spec/1` record carries no REQUIREMENTS-row or Requirement-link data; see §12 Q4** | prose + a markdown table |
| Task DAG | spec §13 tables (node / depends-on / gate / status) — **NOT DELIVERED by Slices 1-5; see correction under §6 Slice 2 and §12 Q4** | markdown tables, `verify_all_specs.sh` checks names only |
| Evidence graph | spec §14 tables (claim / verify command / expected / last-verified / result) — **NOT DELIVERED by Slices 1-5; see correction under §6 Slice 2 and §12 Q4** | markdown tables, not re-run automatically |
| Knowledge graph | `spec-meta` front-matter (id/depends-on/blocks/related/reserves) | markdown fenced block, parsed by regex |
| [Decision graph] | not built — Axon's specs already carry "§12 Open questions" informally; formalizing decision *state machines* (PROPOSED→REVIEWED→APPROVED…) is R40 scope, not needed to close this session's actual failure modes |
| [Experiment graph] | not built — Axon's specs are feature specs with acceptance tests, not scientific experiments with baseline/treatment/seeds; this concept doesn't map onto what this repo does |
| [Provenance graph] | **PROMOTED from "nice-to-have" 2026-07-31** — see §6 Slice 7 (planned) and §12 Q6. The original deferral ("git commit hashes are cited in every evidence row, formalizing them is a nice-to-have") was written under the careless-author threat model of §7.1; under the optimizing-author model it is the load-bearing gap, because the Slice-3 sidecar is a plain rewritable gitignored JSONL that the same actor authoring the claim can append to, truncate, or retroactively rewrite undetected. This repo already ships the exact primitive needed: `crates/axon-audit/src/lib.rs` is a landed, tested append-only JSONL ledger where every entry carries `prev_hash: [u8; 32]` plus a SHA-256 commitment over content+prev_hash, with `verify_chain()` run on open and a tampered chain returning `Err` (verified 2026-07-31). Reuse, not new design. | git commit hashes in markdown cells + an unchained sidecar |

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

**Stated invariants (added 2026-07-31; these are limits, not aspirations — a later pass may not
quietly relax them):**

- **I1 — `--run` is ATTENDED-ONLY and MUST NOT be wired into unattended CI.**
  `verify_all_specs.sh` executes `bash "$ROOT/$script"` where `$script` comes from a spec's own
  `evidence:` field. The *path* is constrained (`^scripts/[A-Za-z0-9_.-]+\.sh$`); the *contents*
  are not constrained at all, and the normal commit shape in this repo lands a spec and the gate
  script its `evidence:` field names in the same agent-authored commit (verified: 5bddc6e, 09e045d,
  372fcc8, 4066538). So `--run` is arbitrary code execution driven by agent-authored markdown. Only
  the static, jq-only checks (`verify_all_specs.sh` without `--run`, `r39_slice2_validate.sh`,
  `r39_slice5_dag_check.sh`) are CI-eligible — see §12 Q4(c), which is scoped to exactly those
  three and to no `--run`. If `--run` ever must go unattended, each evidence script has to execute
  under a sandbox first (§12 Q6).
- **I2 — the evidence-token character class is a security boundary, not a parsing convenience.**
  `verify_all_specs.sh` line ~186 does `expanded=$(eval echo "$ROOT/$tok")` on a token lifted out
  of agent-authored markdown. This is safe *today* only because the extraction class
  `[A-Za-z0-9_{}.,…-]` excludes `$`, backtick, `;` and `/`. That invariant is load-bearing and was
  undocumented until now: widening the class by one character to accommodate a new filename
  convention converts the static path — the path destined for CI under Q4(c) — into command
  injection. The fix is to delete the boundary rather than document it: replace the `eval` with a
  bash array plus native brace expansion. Tracked in §12 Q6; until then, a fixture test asserting
  that a token containing `$(`, a backtick, or `;` is rejected rather than evaluated is a
  prerequisite for landing Q4(c).

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
2. **Validator, ported from `verify_all_specs.sh` — LANDED 2026-07-19 (date corrected
   2026-07-31 against git, commit 03d2097; previously mis-stated as 07-18).** `scripts/r39_slice2_validate.sh
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
   **Correction 2026-07-31:** Slice 1 deferred §13/§14 *table* parsing "to Slice 2 below", but
   Slice 2 as landed is only the port of the spec-meta checks — `r39_slice2_validate.sh`'s own
   header lists exactly the spec-meta fields it reads, and the `axon-gov-spec/1` store record
   carries no §13 task-DAG or §14 evidence-graph table rows. No later slice picked the deferral
   up. **Amended 2026-07-31 (second pass):** the first correction undercounted — it is *three* of
   §4's four in-scope subgraphs that remain untyped, not two: the Objective/claim graph
   (`REQUIREMENTS.md` rows + Requirement-link sections, §4 row 1) was also never delivered by any
   slice. Only the knowledge graph (spec-meta) is actually typed. Tracked explicitly as §12 Q4
   (residual scope: a Slice 6 or R40), not silently closed.
3. **Live re-run — LANDED 2026-07-19 (date corrected 2026-07-31 against git, commit 407c078;
   previously mis-stated as 07-18).** `scripts/verify_all_specs.sh --run TARGET --record-jsonl
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
4. **`axon-gov status`: the render — RE-SCOPED and LANDED 2026-07-19 (date corrected
   2026-07-31 against git, commit 5bddc6e; the previously-stated 07-20 date was wrong — no R39
   commit exists on 07-20).** The original gate ("a strict superset of what `SESSION_STATUS.md` currently
   records by hand") doesn't hold up: `SESSION_STATUS.md` is 1000+ lines (1077 as of 2026-07-31) of iteration-by-iteration
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
   **Landed 2026-07-19** (commit 5bddc6e): `scripts/r39_render_status.sh [STORE_JSONL] [--verify-results PATH]
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
   (`SESSION_STATUS.md` is 1000+ lines (1077 as of 2026-07-31) of iteration-by-iteration narrative reasoning, not just
   structured facts a typed store can render). Landed 2026-07-19 (same day, later commit) with
   the gate corrected — see above.

6. **Ground-truth checks (git, not markdown) — PLANNED, not started (scoped 2026-07-31).** Every
   check landed in Slices 1-5 is *document-internal*: spec-meta id vs. filename, status-claim vs.
   prose `**Status:**` word, edge targets vs. the set of spec filenames, `blocked_by` vs. the target
   spec's own §12 prose. `r39_slice2_validate.sh` reads only the typed store, which is itself a pure
   function of the same markdown. Nothing in R39 compares any claim to ground truth *outside* the
   markdown corpus. That is an adequate proxy for truth against a careless author and a poor one
   against a competent one (§7.1) — making two sentences in a file you are writing agree with each
   other is free. **This spec's own header is the proof:** a prior pass asserted Slice 4 landed
   `2026-07-20`, a date no R39 commit supports, and an earlier same-day correction fixed only Slice
   5's date and left three others wrong. Every R39 check passed clean throughout, and the
   fabrication was caught by a human 12 days later. Meanwhile the tree makes it mechanically
   checkable: 73 distinct `[0-9a-f]{7,40}` tokens are cited across `governance/specs/*.md`
   (verified 2026-07-31) and spot-checks resolve in git. **Planned checks:** (a) for every cited
   hash, `git cat-file -e <h>^{commit}`; (b) where a `LANDED <date>`/`landed <date>` claim appears
   within N lines of a cited hash, assert the commit's author-date matches
   (`git show -s --format=%ad`) — this is the sub-second check that would have caught the 07-20
   fabrication at authorship time; (c) assert the cited commit actually touched a file relevant to
   the spec (`git show --name-only <h>` intersecting `governance/specs/<spec>.md` or the spec's
   `evidence:` scripts), which catches a real-but-unrelated hash pasted purely to satisfy (a).
   **Why this class matters more over time:** it keeps working as the author gets stronger, because
   the author does not control git history. Ownership fork in §12 Q4/Q5.
7. **Gate provenance + chained verify sidecar — PLANNED, not started (scoped 2026-07-31).** Closes
   the §7.2 laundering path. Two halves. (a) **Gate identity:** bump the verify record to
   `axon-gov-verify/2`, adding `gate_sha256` (content hash of the executed script), `gate_commit`
   (last commit touching it), `coauthored` (true when the gate script was modified in the same
   commit as the spec's status-claim — a one-line `git log` check, and the canonical
   optimize-against-your-own-gate signature), and `output_sha256` (digest of captured stdout, so a
   gate whose assertion set silently shrank is visible as a changed digest at unchanged `PASS`).
   Add a validator finding for "`gate_sha256` changed while `result` stayed `PASS` and the
   status-claim strengthened." Then make `r39_render_status.sh` label the *evidence class* rather
   than a bare result — `PASS (self-attested gate, co-authored @ 5bddc6e)` vs. `PASS (gate unchanged
   since <commit>)`. **The render must never read stronger than the evidence behind it; today it
   reads considerably stronger.** (b) **Chained sidecar:** back the Slice-3 sidecar with
   `crates/axon-audit`'s `prev_hash`+`entry_hash` scheme (~10 lines with `sha256sum` in the bash
   writer, since the ledger already speaks JSONL), and have `r39_render_status.sh` REFUSE to render
   verify results from a sidecar whose chain does not verify, rather than rendering them unmarked.
   This also retires §4's deferred provenance-graph row for near-zero cost, and is on-thesis: the
   governance layer should be held to the audit standard the language already enforces on programs.
   Ownership fork in §12 Q6.

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

#### 7.1 Stated threat model — and the assumption inside it that expires (added 2026-07-31)

**R39 as landed assumes a CARELESS author, not an author optimizing for the acceptance gate.**
This was never written down; it is stated here as a limit so a later pass cannot inherit it
silently.

Every failure mode §1 catalogues is an *inattention* artifact — a stale `Draft` header, a dangling
filename, a spec-meta word that doesn't match a prose word. So are all four bugs Slice 2's gate
deliberately injects (dangling `depends-on`, status/prose mismatch, missing evidence script, new
duplicate number). None is a bug a competent author would produce, and none survives that author
simply setting the spec-meta field and the prose sentence to the same word. The checks are real and
worth having; the *class* of adversary they model is narrow.

Two structural consequences follow, and both worsen as spec-authorship throughput rises:

- **The author controls the entire loop.** A spec-authoring agent writes the spec, writes the
  `evidence:` pointer, writes the gate script that pointer names, and lands all three in one
  commit. That is not hypothetical — it is the *empirically normal* commit shape in this repo,
  verified 2026-07-31: 5bddc6e (R39 Slice 4 spec + `r39_render_status.sh` + `r39_slice4_gate.sh`),
  09e045d (R39 Slice 5 spec + its gate), 372fcc8 and 4066538 (R33 spec + `r33_acceptance_gate.sh`).
- **Document self-consistency stops being a proxy for truth.** §6 Slice 6 covers this half; the
  fix is at least one check whose ground truth lives in git rather than in the corpus.

**R39 therefore depends on a human (or an independent agent) reviewing generated governance
artifacts.** It is not a substitute for that review and does not degrade gracefully without it: a
clean `verify_all_specs` run and a green `GOVERNANCE_STATUS.md` assert that the corpus is
*internally consistent*, in a shape readers will reasonably take as "the governance state is true."
Where that gap is closed, §9 now says so explicitly (internally-consistent vs. externally-grounded
checks).

#### 7.2 Concrete consequence: Slice 3/4 launder a self-authored gate into a typed `PASS`

This is the sharpest instance of §7.1 and the reason §6 Slice 7 exists.

`record_verify()` (`verify_all_specs.sh` lines ~244-254) emits
`{schema, spec, command, result, exit_code, ts, commit}`. **`commit` is repo HEAD — it is not the
identity of the gate script that produced the `PASS`.** There is no content hash of the script, no
count of assertions it made, no digest of its output. `r39_render_status.sh` (line ~47) then
renders `"\(.result) @ \(.ts) (commit \(.commit))"` into the "Last verify-run" column of
`GOVERNANCE_STATUS.md` as the authoritative structured status.

So a gate script that was quietly narrowed produces a `PASS @ ts (commit …)` row that is
**byte-identical** to one produced by a strong gate, and Slice 4 renders both as green. §1 already
names the worst instance of this class as having *actually occurred*:
`r32_acceptance_gate.sh`'s TLC success/failure check carried a false-negative bug "that had never
been exercised because the check was always SKIPPED" — a gate reporting success while asserting
nothing, found by hand. Slice 3 would have recorded it `result=PASS, exit_code=0`; Slice 4 would
have rendered it green.

§7's honest hedge above does not cover this. It disclaims *implementation* drift, but Slice 3
exists precisely to re-run implementation gates, and Slice 3's own gate treats gate-`PASS` as
corroborating truth ("reproduces PASS for all three, matching this session's own hand-verified
record"). Until §6 Slice 7 lands, **the verify records and the rendered status column are
self-attestation, and should be read as such** — not as independent evidence. §12 Q1's resolution
rests partly on these records ("a re-run history no markdown table carried"), so this is not a
peripheral slice; Q1 is annotated accordingly.

#### 7.3 Execution risk (previously unnamed anywhere in this spec)

§7 as originally written was entirely about maintenance tax and false-positive fatigue. Execution
risk was not named. `--run` executes agent-authored scripts with full ambient authority, and §12
Q4(c) proposes wiring R39 into `gate.sh`/CI. The audience makes this load-bearing rather than
theoretical: `governance/BUILD_PROTOCOL.md` states its audience is "the autonomous builder (and any
human who joins)," and `governance/ASI_BUILD_LOOP.md` is designed to run as a standing autonomous
loop. Q4(c) lists only the three static jq-only checks — the right instinct — but the spec never
stated the *invariant*, so nothing stopped a later pass from adding `--run all` "for
completeness," at which point any actor able to land a spec plus a script gets code execution in CI
with whatever credentials CI holds. The invariants are now stated as §5 I1 and I2.

### 8. Error codes / exit codes

None reserved — `axon-gov` *would be* a new, separate CLI tool (governance tooling, not the Axon
compiler or runtime), with its own exit-code space independent of Axon's E1xxx/E2xxx/E3xxx blocks.
(Worded conditionally as of 2026-07-31: whether a separate `axon-gov` tool exists at all is §12
Q3, still open — what actually shipped across all five slices is bash extensions of
`scripts/verify_all_specs.sh` plus sibling `scripts/r39_*.sh` scripts, using the ordinary
0/1/2 shell exit conventions documented in the scripts' headers — all except
`r39_slice3_gate.sh`, whose header carries no explicit exit-codes line.)

### 9. Acceptance (definition of done for the R39 row)

**Read this section under §7.1: every criterion below is an INTERNAL-CONSISTENCY check. As of
2026-07-31 the externally-grounded count is ZERO** — no landed R39 check compares any claim to
anything outside the markdown corpus. §6 Slices 6-7 are the criteria that would change that; until
they land or are re-scoped out, "0 findings" means "the corpus agrees with itself," not "the
governance state is true." Restate this split whenever a criterion is added: *internal consistency:
N checks · externally grounded: M checks*.

- **Slice 1 landed 2026-07-18**: every spec with `spec-meta` parses into `governance/state/specs.jsonl`
  (`scripts/r39_slice1_gate.sh` ALL CHECKS PASSED). **Coverage restated 2026-07-31 — the original
  "parses 100% of specs that carry `spec-meta` (61/61)" reads as full coverage, but the qualifier
  carries the whole claim. Verified against the tree: 24 of 62 `governance/specs/R*.md` files carry
  a `spec-meta` block; the other 38 are stored as `pre_convention: true` with a null `id` and
  empty edge arrays, so Slice 2's validator, Slice 5's DAG and blocked-by checks, and Slice 4's
  render all SKIP them. Real typed coverage is 24/62 (39%), not 100%.** The gate currently treats
  the pre-convention count as a *constant to match* ("38, matching `verify_all_specs.sh`'s own
  count exactly") rather than a debt to ratchet down, so the count can GROW with the gate still
  green — a new spec written without `spec-meta` gets zero governance coverage and costs nothing to
  produce. Mitigating fact today: the drift is legacy, not new — every metaless spec was created on
  or before 2026-06-28 and everything since carries meta. But that is a convention held by author
  discipline alone, and author discipline is the resource §7.1 expects to thin first.
- **[OPEN as of 2026-07-31 — pre-convention ratchet]** Record 38 as a ceiling and fail the gate if
  it INCREASES, so opting a new spec out of governance is a hard finding rather than a silent skip.
  Stricter, cheap variant: fail on any spec file whose first git commit post-dates 2026-06-28 and
  lacks `spec-meta`. Owned by §12 Q5.
- **Slice 2 landed 2026-07-19** (date corrected 2026-07-31 against git): the ported validator (`scripts/r39_slice2_validate.sh`) finds every
  real bug the current `verify_all_specs.sh` finds, confirmed by a side-by-side run reporting
  identical findings on the current (clean) tree AND on a synthetic fixture with 4 injected real
  bugs (`scripts/r39_slice2_gate.sh` ALL CHECKS PASSED, 10/10). This is a sound *port* test and is
  correctly scoped as one — but note that all 4 injected bugs are inattention artifacts (§7.1), so
  the fixture suite is calibrated against a careless author only.
- **[OPEN as of 2026-07-31 — adversarial fixture]** Add at least one fixture whose meta and prose
  AGREE but which is still wrong: a spec citing a commit hash that does not exist, and an
  `evidence:` script that exits 0 while asserting nothing. This calibrates the gate suite against
  an optimizing author rather than only a careless one, and is the acceptance-side companion to §6
  Slice 6. Owned by §12 Q5.
- **Slice 3 landed 2026-07-19** (date corrected 2026-07-31 against git): live evidence re-run against R32, R33, R34 reproduces this
  session's hand-verified results exactly (`scripts/r39_slice3_gate.sh` ALL CHECKS PASSED, 11/11).
- **Slice 5 landed 2026-07-19** (ahead of Slice 4 — see §6 slice 4 for why): DAG cycle detection
  and blocked-by open-question staleness checking, including R36's own real example
  (`scripts/r39_slice5_gate.sh` ALL CHECKS PASSED, 10/10).
- **Slice 4 landed 2026-07-19** (date corrected 2026-07-31 against git — the previously-stated
  07-20 was wrong), re-scoped: `GOVERNANCE_STATUS.md` is a purely-generated
  structured-facts index (NOT a superset of `SESSION_STATUS.md`'s narrative — see §6 slice 4 for
  why that original gate didn't hold) (`scripts/r39_slice4_gate.sh` ALL CHECKS PASSED, 8/8). All
  five R39 slices are now landed — **but note (correction 2026-07-31, amended same day): "all
  five slices landed" does NOT mean all of §4's in-scope subgraphs are typed. Three of the four
  remain untyped: the Objective/claim graph (REQUIREMENTS.md rows + Requirement-link sections),
  the §13 task-DAG, and the §14 evidence-graph tables — only the knowledge graph (spec-meta) is
  typed. Residual scope, tracked as §12 Q4.**
- **[OPEN as of 2026-07-31 — owned by §12 Q4]** Documented, honestly, in
  `governance/EXECUTION_MODEL.md` as the "typed" successor to its own §1-3 prose sketch — not a
  separate, competing document. Unmet: `EXECUTION_MODEL.md` (last touched 2026-07-18, before
  Slices 4-5) contains no reference to R39 or its scripts, and no §6 slice owned this bullet, so
  landing all five slices could never have produced it. Folded into §12 Q4 as a named residual
  task so it cannot dead-end silently a second time.

### 12. Open questions

- **Q1 (the decisive fork): build this now, or continue with markdown + `verify_all_specs.sh`?**
  **Resolved 2026-07-18, speculatively, per this section's own suggestion**: built Slice 1 as the
  cheap spike (zero behavior change to the existing bash validator — it's the same script, one new
  opt-in flag). The residual soft half ("whether to continue investing in Slices 2-5") is
  **resolved 2026-07-31, in favor of the investment** — resting (per a same-day adversarial
  re-review) on what the slices deliver that the bash checks alone did not, not on incidental
  bugs found while building them: Slice 5's cycle detection and blocked-by staleness check are
  genuinely new detection (its blocked-by check flags R36 §12 Q1 for real, something no prior
  check could ask), and Slice 3's timestamped, commit-hashed verify records are a re-run history
  no markdown table carried. (The load-flake bug found sizing Slice 2 and the two bugs in Slice
  5's own first draft, previously cited here, are evidence of engineering attention, not of the
  typed store's value — Slice 2 itself is by design finding-identical to the bash validator.)
  Q1 is fully closed; the marginal-value question for the *remaining* untyped subgraphs is Q4's,
  not this one's. **Qualified 2026-07-31 (ASI-trajectory pass):** the second limb of that
  resolution — "Slice 3's timestamped, commit-hashed verify records are a re-run history no
  markdown table carried" — is true but weaker than it reads. Those records carry no identity for
  the gate script that produced the `PASS`, so they are self-attestation rather than independent
  evidence (§7.2). Q1's *decision* does not change (Slice 5's cycle/blocked-by detection is
  genuinely new detection and stands on its own), but its evidentiary weight rests on §6 Slice 7
  landing. If Slice 7 is re-scoped out, this limb of Q1 should be struck rather than left standing.
- **Q2: SQLite vs. JSONL-alongside-markdown?** JSONL is simpler and keeps markdown as the
  source-of-truth (JSONL is a generated index, discardable/regeneratable); SQLite is more queryable
  but adds a binary artifact to a text-only repo. Leaning JSONL for Slice 1, revisit if query
  complexity (Slice 5's DAG traversal) demands it. **Resolved 2026-07-31, in JSONL's favor: the
  revisit condition was evaluated — Slice 5's DAG traversal landed on the JSONL store
  (`scripts/r39_slice5_dag_check.sh` takes STORE_JSONL) without needing SQLite.** If §12 Q4's
  residual table typing ever strains JSONL, that would be a new question, not a reopening of
  this one.
- **Q3: does `axon-gov` live in this repo (`scripts/` or a new `tools/axon-gov/` crate) or is it
  itself an Axon program** (dogfooding the language on its own governance data)? Leaning "an Axon
  program" is the more on-thesis choice long-term (Axon already has `Dict`, JSON parsing, and file
  I/O) but a Python/Rust prototype for Slice 1 is lower-risk to validate the schema shape first.
  **Update 2026-07-31:** the "Python/Rust prototype" framing is itself stale — what actually
  shipped, for five slices running, is a third option: bash extensions of the existing
  `verify_all_specs.sh` plus sibling `scripts/r39_*.sh` scripts. The interim answer has been
  "extend the bash validator" every time; the real remaining fork is bash-forever vs. a Rust
  crate vs. an Axon-program dogfood. Still open.
- **Q4 (opened 2026-07-31; extended same day by a second adversarial pass — the residual scope
  left behind by "all five slices landed"):** items from this spec's own §4/§9/one-line scope
  that were never delivered by any slice and had no owner: (a) **§13 task-DAG and §14
  evidence-graph table typing** — deferred from Slice 1 "to Slice 2", but Slice 2 as landed only
  ported the spec-meta checks, and no later slice picked the deferral up (see the correction
  under §6 Slice 2); (b) **the `EXECUTION_MODEL.md` documentation bullet in §9** — required by
  the definition of done but assigned to no slice; (c) **the "always-on" wiring** — no R39 check
  runs automatically anywhere: `scripts/gate.sh` and CI (`.github/workflows/ci.yml`) reference
  neither `verify_all_specs.sh` nor any `r39_*` script, so the one-line scope's "always-on" and
  §3's "checked in CI" pass vacuously; the fix is cheap (run `verify_all_specs.sh` +
  `r39_slice2_validate.sh` + `r39_slice5_dag_check.sh` — all sub-second, jq-only — as a gate.sh
  and/or ci.yml stage); (d) **the Objective/claim graph (§4 row 1)** — `REQUIREMENTS.md` rows +
  Requirement-link sections were in scope but no script parses `REQUIREMENTS.md` and the store
  record carries no such data; relatedly, no check catches spec↔`REQUIREMENTS.md` status
  divergence (the R39 row itself drifted from this spec, caught only by hand on 2026-07-31) —
  spec↔REQUIREMENTS consistency is a natural first claim-graph check.
  **Q4 SPLIT 2026-07-31 (ASI-trajectory pass) — (b) and (c) are unbundled from this fork and
  should land now, independent of the (a)/(d) scope decision.** The original single fork ("land
  (a)-(d) as a Slice 6, or re-scope (a)/(d) to R40") coupled them backwards: (a) and (d) increase
  check *coverage*, while (c) is what makes coverage nonzero at all — an un-run check is exactly
  equivalent to no check, and the gap between spec-authorship rate and manual-sweep rate is the
  variable that worsens fastest as agent iteration accelerates. The spec itself already concedes
  (b) and (c) are "hours, not slices." `BUILD_PROTOCOL.md` meanwhile already asserts a cadence the
  tooling does not enforce ("Static lint (step 1) is cheap enough to run at every Gate 6 alongside
  the test suite"), so the protocol prose is currently drifted from the tooling — R39's own failure
  mode, one level up. **Action for (c):** add `verify_all_specs.sh` + `r39_slice2_validate.sh` +
  `r39_slice5_dag_check.sh` (all sub-second, jq-only) as a `gate.sh` stage and a `ci.yml` job,
  explicitly WITHOUT `--run`, per §5 I1, and after the I2 fixture test exists. **Q4 remains open
  solely as the (a)/(d) R39-vs-R40 fork.** Until (a)-(d) are landed or re-scoped, R39's status
  stays Implementing, not Landed — the §9 definition of done is not met.
- **Q5 (opened 2026-07-31, ASI-trajectory pass): who owns the externally-grounded checks — a Slice
  6 here, or R40?** Bundles the three §9-adjacent items whose ground truth lives outside the
  markdown corpus: §6 Slice 6's git hash/date/relevance checks, the pre-convention ratchet (freeze
  38 as a ceiling, or the stricter first-commit-date rule), and the adversarial fixture (meta and
  prose agree but the cited commit doesn't exist / the evidence script asserts nothing). They share
  one property that argues for keeping them together and here rather than deferring to R40: they
  are the only R39 checks that keep working as the author gets stronger, because the author does
  not control git history. Cost is low (a `git cat-file -e` loop and a date comparison); the real
  fork is whether R39's scope statement ("formalizes the three structures that already exist as
  conventions") stretches to cover checks against git, or whether that is a new spec. **Open.**
- **Q6 (opened 2026-07-31, ASI-trajectory pass): how far does gate provenance go — record-level
  (`axon-gov-verify/2`), chained sidecar, or sandboxed execution?** Three escalating rungs on the
  §7.2 laundering path, and it is not obvious where to stop. (i) `axon-gov-verify/2` with
  `gate_sha256`/`gate_commit`/`coauthored`/`output_sha256` plus an evidence-class label in the
  render — cheap, shell-local, closes the byte-identical-PASS problem. (ii) Chaining the sidecar
  via `crates/axon-audit`'s `prev_hash`+`entry_hash` scheme, making a fabricated verify history
  break the chain instead of being undetectable — also cheap, and retires §4's provenance-graph
  row. (iii) Sandboxing evidence-script execution, which is what §5 I1 would require before `--run`
  could ever go unattended, and which is the on-thesis dogfood: Axon ships `@[contained]`,
  `Sandbox<P>`, and a capability audit ledger, and the governance layer arguably should be held to
  the standard the language enforces on programs — but (iii) is Q3-scale (it presupposes an
  `axon-gov` that is an Axon program, not a bash script) and should not block (i)/(ii).
  Recommendation of record: land (i)+(ii) as §6 Slice 7; leave (iii) to Q3's bash-vs-Rust-vs-Axon
  fork. **Open.** Also folded here: replacing the §5 I2 `eval` with a bash array + native brace
  expansion, which deletes that boundary rather than documenting it.
