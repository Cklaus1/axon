# Axon Execution Model — Task DAG, Evidence Graph, Knowledge Graph, and the Two Loops

**What this is:** the reference for the three machine-greppable structures every spec now carries
(`SPEC_TEMPLATE.md` front-matter + §13 + §14) and for the inner/outer loop split in
`BUILD_PROTOCOL.md`. It exists because of concrete, repeatedly-verified failures — not process
theory. Every element below traces to one of them; anything that didn't got cut.

**The failure record this fixes (all found in one 2026-07-18 audit session, by hand):**

| Failure | What happened | Which structure would have caught it |
|---|---|---|
| **R17 stale-pessimistic** | REQUIREMENTS.md recorded 1% while Slices 0–3 were shipped with passing gates | Evidence graph re-run (§2): gate passes ↔ claim says "not started" → flagged divergence |
| **R31 stale-pessimistic** | Spec header said "Draft", ROADMAP listed it under "Forward"; actually landed, gate `scripts/r31_acceptance_gate.sh` ALL PASS, feat commit `d2d6dd4` | Same: `status-claim: Draft` + a passing `evidence:` command is a contradiction the verifier flags |
| **R21/R22/R23 dual-numbering** | Two independent specs each claimed the same number; found only by a human grepping filenames | Knowledge graph (§3): unique-`id` lint in `verify_all_specs.sh`; full-slug references everywhere |
| **R28 audit-ledger bug** | `crates/axon-audit` only ever logged AI calls, never FS/Net/Exec/Random/IO, despite its own `EffectKind` enum defining all six — undetected until a human read behavior against the claim | Outer loop step 3 (`BUILD_PROTOCOL.md`): the *gate-bug* class — a gate too narrow for its claim. The Evidence table forces the claim to name its command, which is what makes "does the command actually test the claim?" an askable review question |
| **R19–R34 wave lapse** | Gate 7's "update REQUIREMENTS.md" prose instruction silently lapsed for ~16 specs over weeks | The outer loop itself (§4): a periodic sweep with a mandated cadence, instead of an honor-system sentence |

---

## §1 Task DAG (spec §13)

**What it is:** every spec's internal slice/gate structure plus its cross-spec dependencies, as a
table a script can parse — not a graph database, just consistent rows.

- **Node** = `R<n>.S<k>` (a slice of this spec) or, when referenced from another spec, a full-slug
  node like `R17-freestanding-substrate.S1`.
- **Edges** = the `Depends-on / blocked-by` column: other nodes, full spec IDs, or an open
  question (`R36 §12 Q1`) when a *decision* rather than a spec blocks the node. Cross-spec edges
  also appear (coarser) in the `spec-meta` keys `depends-on` / `blocks`.
- **Gate** = the named test or script that turns the node green, invented at spec time (Gate 1),
  exactly like error codes. A node without a named gate is unverifiable by construction — it can
  only ever have a prose status, which is the thing that rots.
- **Status** ∈ `todo | in-progress | landed | killed`. Any `landed` requires a §14 Evidence row.

**Worked example — R36's slice table (`governance/specs/R36-full-asi-os.md` §6), which already had
this shape informally and is the pattern the convention was extracted from:**

| Node | Depends-on / blocked-by | Gate | Status |
|---|---|---|---|
| R36.S0 | — (truth reconciliation) | all six `scripts/r2{6..31}_acceptance_gate.sh` green in one clean run | todo |
| R36.S1 | R36.S0 | `kernel_enforce_test` from ring 3 (deny + permit) | todo |
| R36.S2 | R36.S1 | `scripts/r36_acceptance_gate.sh` (the §2 headline sentence e2e) | todo |
| R36.S3 | R36.S2; **blocked-by R36 §12 Q1** (founder two-kernels decision) | golden-IR + boot gate | todo |
| R36.S4 | R36.S0 (parallel to S1–S3; hardware-gated) | attestation report verifies against vendor cert chain | todo |
| R36.S5 | R36.S2 | A1-style CLI smoke | todo |

What this buys, concretely: `R36.S3 blocked-by Q1` is now grep-able — an autonomous tick that
picks up S3 while Q1 is open is mechanically detectable, instead of depending on the builder
re-reading §12 prose. And `R36.S0`'s gate being "the six constituent scripts" means the R26
env-leak gate bug (found 2026-07-18) is *inside* a named node instead of a footnote.

## §2 Evidence graph (spec §14 + the REQUIREMENTS.md wave table)

**The rule:** *a claim without a re-runnable evidence pointer is not a valid status.*

Every status assertion — spec header `Landed`/`Shipped`/`Slices 0–3 done`/`70%`, a §13 row marked
`landed`, a REQUIREMENTS.md `%` — must be backed by a row:

| Claim | Verify command | Expected | Last verified (commit @ date) | Result |
|---|---|---|---|---|
| "R28 landed incl. all-capability audit" | `scripts/r28_acceptance_gate.sh` | exit 0 | `0bfa74d` @ 2026-07-18 | PASS |

- **Verify command** must be executable from the repo root by anyone (script, `cargo test -p …
  <name>`, or an `axon …` invocation asserting exit code/stdout). "I checked" is not a command.
- **Last verified** is a real commit hash + date at which the command was *actually run and
  observed*. It is allowed — expected — to go stale; the outer loop's job is to re-run and
  re-date it. An Evidence row whose date predates major related churn is a *warning*, not a lie;
  a status with no row at all is invalid.
- The **REQUIREMENTS.md "R19–R34 wave" table already uses this format** (its Evidence column:
  gate script + commit + "re-run 2026-07-18") — it is the matrix-level instance of the same graph
  and future REQUIREMENTS.md status edits must keep that column populated the same way. (That
  table is exactly what the 2026-07-18 manual audit produced; this section makes producing it a
  standing requirement instead of a one-off heroic effort.)
- Directionality: the graph catches **both** drift directions. `Result: PASS` under
  `status-claim: Draft` is the R31 case; `Result: FAIL` under `Landed` is the classic case. Both
  are outer-loop findings requiring a `truth:` correction commit.
- Limit, stated honestly: evidence proves *the gate passes*, not *the gate is right*. The R28 bug
  lived below a passing gate. That class is only catchable at review time by asking "does this
  command actually exercise the claim's nouns?" — which is why the claim and command sit in the
  same row, adjacent, instead of in different files.

## §3 Knowledge graph (`spec-meta` front-matter)

Every spec carries, immediately after its prose header:

```spec-meta
id: R<n>-<slug>
status-claim: Draft | Reviewed | Implementing | Shipped | Landed | ...
depends-on: <full spec IDs, comma-separated, or none>
blocks: <specs that cannot proceed until this lands>
blocked-by: <specs or open questions blocking this>
supersedes: <specs this replaces>
related: <specs sharing surface/thesis; competing platform fronts go here>
conflicts-with: <specs that cannot both hold as written — incl. numbering/code collisions>
reserves: <E-code blocks / exit codes claimed, e.g. E37xx, exit 7>
evidence: <the acceptance-gate command, or none (valid only while Draft)>
```

Conventions:
- **Full slugs always** (`R21-axon-os-supervisor`, never `R21`) — bare numbers are ambiguous
  precisely because of the dual-numbering incident.
- `related` is where "R36 explicitly converges R17+R21+R26–R31" and "R37 forks the design space
  from R17" become one grep instead of a close reading of §1 prose in three files.
- `reserves` exists because code-block collisions have the same shape as number collisions
  (Phase 6's nominal E1300–E1308 collided with AI-policy E1300–E1302; R38 §6 had to tiptoe around
  E2300–2302/E1810 by prose). Declared reservations are lintable.
- Backfill status: **R36/R37/R38 carry the block** (done 2026-07-18, as the format's validation).
  Pre-existing specs are grandfathered — the verifier reports them as `pre-convention`, and each
  gets the block whenever it is next edited for any reason. Do not mass-backfill mechanically; a
  wrong edge is worse than a missing one.

## §4 The two loops

- **Inner loop** = `BUILD_PROTOCOL.md`'s 8 gates. One feature → one green gated commit. It keeps
  a *feature* honest at the moment it lands.
- **Outer loop** = the periodic evidence sweep (`BUILD_PROTOCOL.md`, "The OUTER LOOP" section).
  It keeps the *ledger of claims* honest over time: static-lint the knowledge graph, re-run the
  evidence graph, diff against claims, and land `truth:` correction commits in the same session.
  Cadence/triggers and the divergence-handling discipline (correction commit; gap entry for
  regressions; gate-bug → new inner-loop tick) are normative there, not here.

**Mechanization:** `scripts/verify_all_specs.sh` implements the static lint (duplicate numbers,
dangling edges, missing evidence scripts, status/claim mismatch, non-Draft-without-evidence) for
all specs, and `--run <Rn|all>` re-executes evidence commands for specs that have a clean
`evidence:` pointer (the R26–R31 gate scripts are the easy first targets; note R30 exceeds 4 min
on WSL2 — run its constituents). Specs without `spec-meta` are listed as `pre-convention`, which
doubles as the backfill worklist.
