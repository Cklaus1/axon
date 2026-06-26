# Tech Spec — R22: Intent→Approve Gateway (Vision-OS v1 front half)

**Spec ID:** `R22-intent-approve-gateway`
**Status:** 📝 Draft (2026-06-26)
**Implements:** `VISION_OS.md` v1 §6 ("dynamic synthesized userland") — the *front half* of the
loop whose *back half* is `R21-axon-os-supervisor.md`. R22 turns prose intent into a
**proven-bounded job** (`.axjob` + `.ax`) that R21 runs; the human approves the **proof**, not the
code.
**Audience:** an implementer who builds *strictly* against this document and reads only it.

> **Read this framing first.** R22 is the synthesis + approval front-end. Its output is exactly the
> input R21 consumes: a `.axjob` manifest + an `.ax` program, plus a signed-by-the-human **Approval
> token** R21 honors. R22 does **not** run, enforce, or sandbox anything — that is R21's job (§1.2).
> The two compose into the full "state intent → synthesize → prove-bound → approve → run → audit"
> loop. **R22 depends on R21's data model** (`JobManifest`, `Grant`, `DeclaredEffects`) and reuses it
> verbatim — it does not redefine those types.

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey + smoke test through the actual CLI | §5, §7 | `acc_a1_smoke_intent_to_approval` |
| **A2** | Real runnable example artifact (intent files, not toys in the test dir) | §5.6, §7 | `acc_a2_example_intents_compile_and_run` |
| **A3** | Quickstart whose exact commands are executed by a test | §9, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Hermetic, isolated synthesis + canonical entrypoint, hard timeout | §4.5, §7 | `acc_a4_synthesis_isolated_timeout` |
| **A5** | Deterministic (replay/mock LLM ⇒ byte-identical job + token) | §4.6, §7 | `acc_a5_deterministic_compile` |
| **A6** | Integrity: least-privilege grant inference, fail-closed, tamper-evident Approval token | §4.2, §4.4, §7 | `acc_a6_approval_token_binds_and_tamper_detected` |
| **Core** | Synthesized program's declared effects ⊆ inferred grant (admissible by construction) | §4.3, §7 | `synthesized_job_is_self_admissible` |
| **Core** | Grant is least-privilege: never broader than the program needs or the intent caps allow | §4.2, §7 | `grant_is_least_privilege` |
| **Core** | Confidence/low-quality synthesis is REFUSED, not shipped | §4.3, §7 | `low_confidence_synthesis_refused` |
| **Core** | Approval binds the EXACT (program, grant) digest; any edit invalidates it | §4.4, §7 | `approval_invalidated_by_any_edit` |
| **Gate** | The acceptance gate fails if any check above is missing/stubbed | §10 | `scripts/r22_acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does
R22 is a CLI that turns a human's prose **intent** into a runnable, **proven-bounded** job:

1. **Compile intent → program.** Parse an `.intent.md` (prose goal + named sections), call an LLM to
   synthesize an `.ax` program (reusing the Phase-10 `axon intent compile` generation path), and lift
   the result into a typed, type-checked program.
2. **Infer a least-privilege grant.** From the program's *declared* effect row + the intent's stated
   capability bounds, compute the **smallest** `Grant` that admits the program — never broader.
3. **Prove admissibility.** Assert (via R21's gate semantics) that the synthesized program's declared
   effects ⊆ the inferred grant. If the program could exceed the grant, **refuse** (do not ship).
4. **Present for approval.** Render a legible, quantified summary ("this will be allowed to … and may
   NOT …; budget …; confidentiality ceiling …") plus confidence/uncertainty markers on fuzzy
   resolutions, and the risk level. The human approves the **resolved (program, grant)** — not the
   prose.
5. **Emit an Approval token** that cryptographically binds the exact `(program, grant)` content. R21
   honors a valid token and refuses to run a job whose token doesn't match (tamper-evident handoff).

The end-state: `intent.md → {program.ax, job.axjob, job.approval}` — a triple R21 runs under a bound
the human reviewed and signed.

### 1.2 What it explicitly does NOT do (out of scope for R22)
- **No execution / enforcement / sandboxing.** R22 never runs the program. R21 does. R22's job ends
  at the approved triple.
- **No new capability/grant/effect types.** R22 imports R21's `Grant`/`JobManifest`/`DeclaredEffects`
  and the gate's `admit` semantics. If R21's types change, R22 follows.
- **No proof *certificates*.** R22's admissibility check uses R21's gate (declared effects ⊆ grant),
  which trusts the effect-row extractor; certificate-checked proofs are `R23` (documented gap).
- **No cryptographic *signing* / identity.** The Approval token is a **content-binding digest +
  recorded human decision** (tamper-evident: any edit to program/grant invalidates it), **not** a
  PKI signature. Hardware-rooted signed approval is `VISION_OS` §5 G6. A6 documents exactly what is
  vs isn't authenticated.
- **No multi-party / risk-proportional approval friction.** R22 ships single-approver + a *recorded*
  risk level; the defended, multi-sig, friction-scaled approval boundary is `R24` (G7). R22 leaves
  the hook (`risk` field + an `--approvers N` stub that errors if N>1).
- **No live model required to build/test.** All LLM calls go through R21/axon-core's deterministic
  mock + replay (A5); a live key is optional and only exercised by an opt-in test.

### 1.3 Persona / ICP
A **non-programmer or operator** who can state *what they want* in prose but should not (and cannot)
write or audit the Axon code. They review a **legible bound and a risk summary**, approve or reject,
and never read the AST unless they choose to.

### 1.4 Interface & tech constraints
- **Interface:** CLI `axon-intent` (subcommands `compile`/`review`/`approve`/`emit`), plus a library
  crate.
- **Language/deps:** Rust, new workspace crate `crates/axon-intent`. Reuses `axon-os` (R21) types,
  `axon-core` (the `axon intent compile` path + type-check), `sha2`, `serde_json`. **No** new heavy
  deps.
- **Perf/security:** core logic pure/I-O-free; synthesis (the LLM call) is the single impure seam,
  isolated + time-bounded; **least-privilege by construction**; **fail closed** (refuse on low
  confidence, non-admissible synthesis, or grant-exceeds-intent).

### 1.5 Domain-specific risks (what matters most here)
- **Privilege creep:** the synthesizer asks for (or the program needs) more capability than the intent
  warrants. → least-privilege inference + intent-cap ceiling (§4.2), `grant_is_least_privilege`.
- **Silent low-quality synthesis:** the LLM emits a plausible-but-wrong or under-specified program
  that still type-checks. → confidence gate + refuse-not-ship (§4.3), `low_confidence_synthesis_refused`.
- **Approval/code skew:** the human approves one thing, a different thing runs. → content-binding
  Approval token (§4.4), `approval_invalidated_by_any_edit`.
- **Non-reproducible synthesis:** can't audit what was generated. → mock/replay determinism (§4.6).

---

## §2 — Architecture & modules

New crate `crates/axon-intent/`. **Core logic is pure; the LLM synthesis call is the only impure
seam, behind the `Synthesizer` trait** (mockable).

```
crates/axon-intent/src/
  intent.rs        Parse + validate an `.intent.md` → Intent (sections, declared caps).   [PURE]
  synth.rs         `trait Synthesizer` (intent → ProgramDraft) + real impl + MockSynth.    [I/O — only impure module]
  grant_infer.rs   Least-privilege Grant inference from DeclaredEffects ∩ intent caps.      [PURE]
  admit.rs         Admissibility proof: reuse R21 gate (declared ⊆ grant) → Admit|Refuse.   [PURE]
  confidence.rs    Confidence scoring of a ProgramDraft → Ship|Refuse{reason}.              [PURE]
  approval.rs      ApprovalRequest render + ApprovalToken build/verify (content binding).   [PURE]
  pipeline.rs      Orchestrates compile→infer→admit→score→request. Generic over Synthesizer. [PURE core, I/O injected]
  emit.rs          Write the {program.ax, job.axjob, job.approval} triple to disk.          [I/O — thin]
  cli.rs           Arg parse → dispatch → human-facing output + exit codes.                 [I/O — thin]
  lib.rs           Public API re-exports (imports axon-os types).                           [—]
  main.rs          fn main() → cli::run(env::args).                                          [I/O — thin]
crates/axon-intent/tests/
  acceptance.rs    The A1–A6 + Core checks (named exactly per §0).
examples/intents/
  summarize.intent.md     A real, runnable intent → compiles to an admissible job (A2).
  overbroad.intent.md     A real intent whose ask exceeds least-privilege → grant clamped (A2).
  vague.intent.md         A real under-specified intent → low-confidence REFUSAL (A2 negative).
scripts/r22_acceptance_gate.sh
README-axon-intent.md      Quickstart whose commands a test executes (A3).
```

**Dependency graph (acyclic; → = uses):**
```
main → cli → pipeline → {intent, synth, grant_infer, admit, confidence, approval}
                        emit → approval
grant_infer → (axon-os::Grant, DeclaredEffects)
admit → (axon-os::gate)           // reuse R21's admit semantics — do NOT reimplement
approval → (sha2)
synth → (axon-core: `axon intent compile`, type-check)   // the ONLY edge to a model/process
```
**Rule:** nothing under `intent/grant_infer/admit/confidence/approval/pipeline` may do I/O, read the
clock, or call random. The seed and any model output are injected. Only `synth.rs`, `emit.rs`,
`cli.rs`, `main.rs` touch the outside world.

---

## §3 — Data model

### 3.1 `Intent` (parsed from `.intent.md`)
```
Intent {
    goal:        String,        // one-sentence objective (the "# Intent" / first prose line)
    inputs:      Vec<String>,   // "## Inputs" bullet lines (data the job reads)
    outputs:     Vec<String>,   // "## Outputs" bullet lines (what it produces)
    cap_ceiling: CapCeiling,    // "## Allowed" section → the MAX caps the human will permit
    budget:      Budget,        // "## Budget" section → ResBudget (calls/tokens/cost_micro)
    seed:        u64,           // "## Seed" or default 42 (deterministic synthesis, A5)
}
CapCeiling { fs_read: Vec<PathPrefix>, fs_write: Vec<PathPrefix>, net: Vec<Host>, exec: ExecPolicy,
             max_label: Label }   // the intent's explicit upper bound on authority
```
`.intent.md` serialized form (prose + named sections; the parser reads exactly these headers):
```markdown
# Intent
Summarize ./data/report.txt into ./out/summary.txt.

## Inputs
- ./data/report.txt

## Outputs
- ./out/summary.txt

## Allowed
- fs_read: ./data/
- fs_write: ./out/
- net: none
- exec: none
- max_label: internal

## Budget
- calls: 100
- tokens: 50000
- cost_micro: 1000000

## Seed
- 42
```
Validation (fail → `Refusal{Malformed}`, exit 2): `# Intent` present and non-empty; `## Allowed`
present; every cap-ceiling path has **no `..` component**; budget fields ≥ 0; `exec ∈ {none, any}`;
`max_label ∈ {public, internal, secret}`; `seed` is a u64. A missing `## Allowed` is **fail-closed**
(no ceiling ⇒ refuse; the human must state what they permit) — *not* "allow everything."

### 3.2 `ProgramDraft` (output of the synthesizer)
```
ProgramDraft {
    source:      String,          // the synthesized .ax source
    generated:   bool,            // true = LLM-authored; false = lifted verbatim from the intent's fenced ```axon block
    fences_stripped: bool,        // whether ```axon fences were stripped from model output
    raw_meta:    SynthMeta,       // confidence inputs (see 3.3)
}
SynthMeta {
    typechecks:        bool,      // did `axon check` pass?
    declared:          DeclaredEffects,   // R21 type, extracted from the synthesized program
    has_entry:         bool,      // a `fn main`/declared entry exists
    resolved_outputs:  Vec<String>,       // outputs the program actually writes (for cross-check vs intent)
    uncertainty_notes: Vec<String>,       // fuzzy resolutions the synthesizer flagged
}
```

### 3.3 `Confidence` verdict
```
Confidence = Ship { score: u8 /*0..100*/ } | Refuse { reason: String }
```
Refuse reasons (each is a named negative test): `does-not-typecheck`, `no-entry-point`,
`outputs-mismatch` (program writes outputs the intent didn't ask for), `empty-or-stub-body`,
`below-threshold` (score < `CONFIDENCE_FLOOR`, default 60).

### 3.4 `ApprovalRequest` (rendered for the human; the legible bound)
```
ApprovalRequest {
    program_digest: String,   // "axsha256:"+sha256(program source)
    grant:          Grant,    // the inferred least-privilege grant (R21 type)
    grant_digest:   String,   // "axsha256:"+sha256(canonical grant)
    intent_goal:    String,
    risk:           Risk,     // Low|Medium|High|Critical, derived per Phase-11 rules (effects+budget+irreversibility)
    confidence:     u8,
    legible:        String,   // the human-readable "may / may NOT / budget / label" rendering
    uncertainty:    Vec<String>,
}
```

### 3.5 `ApprovalToken` (the tamper-evident handoff to R21; JSON, schema `axon-approval/1`)
```
ApprovalToken {
    schema:         "axon-approval/1",
    program_digest: String,   // binds the EXACT program source
    grant_digest:   String,   // binds the EXACT grant
    approved_by:    String,   // operator id supplied at approve time (recorded, not authenticated)
    decision:       "approved" | "rejected",
    risk:           Risk,
    token_digest:   String,   // "axtok1:"+sha256(canonical(program_digest,grant_digest,approved_by,decision,risk))
}
```
**R21 handoff contract (R21 honors this):** R21's `run` MUST, when a `.approval` is present, recompute
`program_digest` and `grant_digest` from the on-disk `(program, grant)` and assert they equal the
token's, and that `decision=="approved"`, and that `token_digest` re-hashes correctly — else
`Verdict::Denied{"unapproved or edited after approval"}` exit 8, **before** running. (This is the one
small addition R21 gains; specify it here, implement the R21 side under R21's gate.)

### 3.6 `Refusal` + exit codes (consistent with R21's carved scheme)
```
Refusal =
  | Malformed { reason }            → exit 2    // bad intent file / usage
  | LowConfidence { reason, score } → exit 5    // synthesis refused (carve: a new "policy refusal" code)
  | NotAdmissible { axis, reason }  → exit 8    // synthesized program could exceed the inferred grant
  | GrantExceedsCeiling { axis }    → exit 8    // inference would need more than the intent permits
  | Rejected                        → exit 3    // the human rejected at approval
```
Exit 0 = an approved triple was emitted. (5 reuses Axon's AI-policy band; 3/8 reuse the carved scheme.)

---

## §4 — Core logic / algorithms

### 4.1 Compile (`pipeline::compile`) — orchestration, synthesis via `Synthesizer`
```
fn compile(intent: &Intent, synth: &impl Synthesizer) -> Result<ProgramDraft, Refusal>
```
1. If the intent embeds a fenced ```axon block AND `AXON_INTENT_GEN` is off → **lift** it verbatim
   (`generated=false`); else call `synth.synthesize(intent)` (`generated=true`).
2. Strip accidental ```axon fences from model output (`fences_stripped`).
3. Type-check the result (`synth.typecheck(source)`), populate `SynthMeta` (declared effects via the
   R21 extractor, entry presence, resolved outputs).
4. Return the `ProgramDraft`. (No confidence decision yet — §4.3.)

### 4.2 Least-privilege grant inference (`grant_infer::infer`) — Core, fail-closed
```
fn infer(declared: &DeclaredEffects, ceiling: &CapCeiling, budget: &Budget) -> Result<Grant, Refusal>
```
The grant is the **intersection of what the program needs and what the intent permits** — never more:
1. For each axis, the inferred allowlist is the **narrowest** set that covers the program's *actual*
   targets AND is ⊆ the ceiling:
   - `fs_read`  = (program's read targets) ∩ ceiling.fs_read, each clamped to the **most specific
     ceiling prefix** that contains it; if a program read target is **not** under any ceiling prefix
     → `GrantExceedsCeiling{fs_read}` (the intent forbids it) exit 8.
   - same for `fs_write`, `net`.
   - `exec`: granted only if the program declares EXEC **and** ceiling.exec=any; else none.
2. `max_label = min(program.declared.max_label, ceiling.max_label)`; if program needs > ceiling →
   `GrantExceedsCeiling{label}`.
3. `budget` = the intent's budget (the human's stated cap) — never inflated.
4. **Least-privilege invariant** (`grant_is_least_privilege`): the inferred grant grants an axis iff
   the program *declares* it; an axis the program never uses is **empty/none** even if the ceiling
   permits it. (Permission the program doesn't need is never granted.)

### 4.3 Admissibility + confidence (`admit::prove`, `confidence::score`) — Core, refuse-not-ship
1. `admit::prove(program, grant)` = R21's `gate::admit` with the program's declared effects and the
   inferred grant. **By construction of §4.2 this must return `Admit`** — if it ever returns `Deny`,
   that is a *bug or an adversarial program* and we **Refuse** (`NotAdmissible`, exit 8), never ship.
   (`synthesized_job_is_self_admissible` proves the happy path; the adversarial test forces a Deny via
   a mock declaring more than it was inferred and asserts refusal.)
2. `confidence::score(draft)`:
   - hard refusals first: `!typechecks` → Refuse(does-not-typecheck); `!has_entry` →
     Refuse(no-entry-point); `resolved_outputs ⊄ intent.outputs` → Refuse(outputs-mismatch); empty/stub
     body → Refuse(empty-or-stub-body).
   - else a score in 0..100 from a **fixed, deterministic rubric** (typechecks=+40, entry=+20,
     outputs-match=+20, no-uncertainty-notes=+20; documented constants); `score < CONFIDENCE_FLOOR`
     (60) → Refuse(below-threshold) exit 5. **No clock/random — the rubric is pure.**

### 4.4 Approval token (`approval::build` / `approval::verify`) — Core, tamper-evident (A6)
- `render(draft, grant, intent) -> ApprovalRequest`: compute `program_digest`/`grant_digest`, derive
  `risk` (Phase-11 rule: Exec→Critical, Net+FS→High, Net|FS→Medium, pure→Low; `--risk` may only
  raise), build the **legible** string ("This program WILL be allowed to: read ./data/, write ./out/.
  It may NOT: use the network, spawn processes. Budget ≤ … . Confidentiality ceiling: internal. Risk:
  Low. Confidence: 92/100.").
- `build(request, approved_by, decision) -> ApprovalToken`: bind both digests + decision + risk into
  `token_digest`.
- `verify(token, program_src, grant) -> Result<(), Mismatch>`: recompute both digests from the *actual*
  on-disk content and the `token_digest`; any mismatch → `Err` (this is what R21 calls on handoff).
  **Editing the program or grant by one byte after approval invalidates the token**
  (`approval_invalidated_by_any_edit`).

### 4.5 Hermetic, isolated synthesis (`RealSynthesizer`) — the impure seam (A4)
- Synthesis invokes the canonical `axon` entrypoint (resolved by absolute path from `AXON_BIN`, not
  ambient PATH) running `axon intent compile` in a **fresh subprocess** with `AXON_INTENT_GEN=1`,
  wrapped in a **hard timeout** (`AXON_INTENT_TIMEOUT_MS`, default 60000); on expiry the child group
  is killed (RAII guard) → `Refusal::LowConfidence{"synthesis timed out"}` (fail closed). Type-check
  likewise via `axon check` in a bounded subprocess. No leaked handles; minimal explicit environment.

### 4.6 Determinism (A5)
- Volatile inputs are **only**: the intent's `seed`, and the model. For tests + audit, synthesis runs
  through axon-core's **mock/replay** (`AXON_AI_MOCK=1` or `AXON_AI_REPLAY=<file>`) so a given
  (intent, seed) yields a **byte-identical** `ProgramDraft` and therefore a byte-identical
  `(program, grant, token)` triple. Core logic (infer/admit/score/approval) is otherwise a pure
  function of (intent, draft). **Contract:** `acc_a5` runs `compile` twice under mock and asserts the
  emitted triple bytes are identical. The live-model path is non-deterministic by nature and is an
  opt-in test only, never in the gate.

---

## §5 — Public API / interface contract

### 5.1 Library API (`lib.rs`)
```
pub fn compile(intent:&Intent, synth:&impl Synthesizer) -> Result<ProgramDraft, Refusal>;
pub fn infer_grant(declared:&DeclaredEffects, ceiling:&CapCeiling, budget:&Budget) -> Result<Grant, Refusal>;
pub fn prove_admissible(declared:&DeclaredEffects, grant:&Grant) -> Result<(), Refusal>;
pub fn score(draft:&ProgramDraft, intent:&Intent) -> Confidence;
pub fn render_request(draft:&ProgramDraft, grant:&Grant, intent:&Intent) -> ApprovalRequest;
pub fn build_token(req:&ApprovalRequest, approved_by:&str, decision:Decision) -> ApprovalToken;
pub fn verify_token(token:&ApprovalToken, program_src:&str, grant:&Grant) -> Result<(), Mismatch>;
pub trait Synthesizer {
    fn synthesize(&self, intent:&Intent) -> Result<String, SynthErr>;   // → .ax source
    fn typecheck(&self, source:&str) -> SynthMeta;
}
```

### 5.2 CLI (`axon-intent`; every subcommand has `--help`; output legible, not just exit codes)
```
axon-intent compile <intent.md> [--out DIR] [--seed N]
    Synthesize → infer least-privilege grant → prove admissible → score. On success writes
    <DIR>/<name>.ax + <name>.axjob and prints the ProgramDraft summary + "ADMISSIBLE, confidence
    92/100, risk Low". On refusal prints the reason in plain English and exits per §3.6. Does NOT
    approve or run.

axon-intent review <name>.axjob [--program <name>.ax]
    Print the legible ApprovalRequest (the "may / may NOT / budget / label / risk / confidence"
    block + uncertainty notes). No side effects. Exit 0.

axon-intent approve <name>.axjob --by <operator-id> [--accept|--reject]
    Render the request, record the decision, and on --accept write <name>.approval (the
    ApprovalToken). Prints "✓ approved — token bound to program X, grant Y" or "✗ rejected". Exit
    0 (approved) / 3 (rejected). --approvers N with N>1 → exit 2 "multi-party approval is R24".

axon-intent emit <intent.md> --by <operator-id> [--out DIR]
    Convenience: compile → review (printed) → (interactive or --yes) approve, producing the full
    {.ax, .axjob, .approval} triple ready for `axon-os run`. Exit per the first failing stage.
```
Bad usage / missing file → exit 2 with a helpful, specific message.

### 5.6 Shipped example artifacts (A2 — real, in `examples/intents/`, runnable immediately)
- `summarize.intent.md` → compiles to an admissible job (fs_read ./data/, fs_write ./out/, no net),
  confidence ≥ floor, risk Low → approvable → **`axon-os run` completes** (the cross-spec journey).
- `overbroad.intent.md` → the intent's `## Allowed` permits net, but the synthesized program never
  uses net → the inferred grant has **net=∅** (least-privilege; `grant_is_least_privilege`), and the
  legible summary shows "may NOT: use the network."
- `vague.intent.md` → an under-specified goal yielding a stub/non-typechecking draft → **REFUSED**
  (exit 5), no triple emitted (the headline negative demo).

---

## §6 — Build order (TDD: write the named test first, see it fail, make it pass; green before next)

- **S1 — Intent parse/validate.** `intent.rs`. Tests: parse the example; reject missing `# Intent`,
  missing `## Allowed` (fail-closed), `..` in a cap path, bad `exec`/`max_label`, negative budget.
- **S2 — Least-privilege grant inference.** `grant_infer.rs` over R21 types. Tests:
  `grant_is_least_privilege` (unused axis ⇒ empty even if ceiling allows), `GrantExceedsCeiling` when
  a program target is outside the ceiling, label-min, budget-not-inflated.
- **S3 — Admissibility + confidence.** `admit.rs` (reusing R21 gate), `confidence.rs`. Tests:
  `synthesized_job_is_self_admissible`; `low_confidence_synthesis_refused` for each refuse reason;
  the deterministic rubric scores.
- **S4 — Approval token.** `approval.rs`. Tests: `acc_a6_approval_token_binds_and_tamper_detected`,
  `approval_invalidated_by_any_edit` (flip one byte of program OR grant → verify fails); reject path.
- **S5 — Pipeline over MockSynth.** `pipeline.rs` + `synth::MockSynth`. Tests: compile→infer→admit→
  score→request happy path; refusal short-circuits (no token emitted on a refused draft — call-count
  assert).
- **S6 — RealSynthesizer + hermetic exec.** Wire §4.5. Tests: `acc_a4_synthesis_isolated_timeout`
  (a synth that hangs is killed at the timeout → fail-closed refusal, no leaked child); mock-backed
  `acc_a5_deterministic_compile`.
- **S7 — CLI + emit + human output.** `cli.rs`, `emit.rs`, `main.rs`. Tests: `--help` on every
  subcommand; usage error → exit 2; the four subcommands' outputs.
- **S8 — Examples + smoke + quickstart + R21 handoff.** `examples/intents/*`, `README-axon-intent.md`,
  and the R21-side token check. Tests: `acc_a1_smoke_intent_to_approval`,
  `acc_a2_example_intents_compile_and_run`, `acc_a3_quickstart_commands_execute`, and a cross-spec
  test that `axon-os run` **refuses** a job whose program was edited after approval.
- **S9 — Acceptance gate.** `scripts/r22_acceptance_gate.sh` (§10). Green = done.

---

## §7 — Test plan (happy + **adversarial**; every named test is normative)

**Unit / core (pure, fast):**
- `intent_rejects_malformed` — each of: no `# Intent`, no `## Allowed`, `fs_read=["../etc"]`,
  `exec="root"`, `tokens=-1` → `Malformed` exit 2.
- `grant_is_least_privilege` — ceiling permits net+exec; synthesized program uses only fs_read →
  inferred grant has net=∅, exec=none, fs_write=∅. Permission the program doesn't need is never granted.
- `grant_exceeds_ceiling_refused` — program reads `./secret/` not under any ceiling prefix →
  `GrantExceedsCeiling{fs_read}` exit 8.
- `synthesized_job_is_self_admissible` — the inferred grant admits the program (R21 gate → Admit).
- `non_admissible_refused` (adversarial) — a MockSynth returns a program declaring `{NET}` while
  inference produced no net (forced skew) → `NotAdmissible` exit 8, **no triple emitted**.
- `low_confidence_synthesis_refused` — one test per refuse reason (does-not-typecheck, no-entry,
  outputs-mismatch, stub-body, below-threshold) → exit 5, no triple.
- `acc_a6_approval_token_binds_and_tamper_detected` + `approval_invalidated_by_any_edit` — build a
  token, then mutate (a) one program byte, (b) one grant field, (c) the token's decision field; each
  → `verify` Err.

**Integration (real `axon` subprocess, mock LLM):**
- `acc_a4_synthesis_isolated_timeout` — a synth stub that sleeps past the timeout is killed; refusal;
  process gone; no leaked handle.
- `acc_a5_deterministic_compile` — `compile` the example twice under `AXON_AI_MOCK=1`; assert the
  emitted `{.ax,.axjob,.approval}` bytes are identical across runs.
- `acc_a2_example_intents_compile_and_run` — `summarize.intent.md` → emits a triple →
  **`axon-os run` (R21) completes exit 0** with the output artifact; `overbroad` → grant shows net=∅;
  `vague` → refused exit 5, no triple.

**Cross-spec handoff (R22 → R21):**
- `r21_refuses_edited_program_after_approval` — emit a triple, approve, then append a byte to the
  `.ax`; `axon-os run` → `Denied` exit 8 ("unapproved or edited after approval"). Proves the token
  binds.

**User-journey smoke (A1 — drives the REAL CLI exactly as the operator would, via subprocess):**
- `acc_a1_smoke_intent_to_approval`: (1) `axon-intent compile summarize.intent.md --out <tmp>` →
  asserts "ADMISSIBLE, confidence ≥60, risk Low" + the `.ax`/`.axjob` artifacts; (2) `axon-intent
  review …` → asserts the legible "may / may NOT / budget" text; (3) `axon-intent approve … --by op
  --accept` → asserts "✓ approved" + the `.approval` artifact; (4) `axon-os run` on the triple →
  "✓ completed" + output file; (5) `axon-intent compile vague.intent.md` → asserts the plain-English
  REFUSAL + **no** artifacts written. Each step asserts stdout text AND on-disk artifact.

**Quickstart (A3):**
- `acc_a3_quickstart_commands_execute` — extracts the fenced block from `README-axon-intent.md` and
  runs each line verbatim against the built binaries; documented outputs hold.

---

## §8 — Invariants & edge cases

**Invariants (assert in tests):**
- **I-1 Refuse, don't downgrade.** A low-confidence, non-typechecking, or non-admissible synthesis
  **emits no triple** — never a "best effort" program. Fail closed.
- **I-2 Least privilege.** The inferred grant grants an axis iff the program declares it AND the
  intent permits it; `grant ⊆ intent.ceiling` and `grant ⊆ program-need` on every axis.
- **I-3 Self-admissible by construction.** Any emitted triple satisfies R21's gate (`declared ⊆
  grant`); R21 will never deny an R22-approved job for capability reasons (it may still deny on
  runtime over-reach if the program *lies* about its effects — which R23 certificates close).
- **I-4 Approval binds content.** The token binds the exact `(program, grant)` digests; any post-
  approval edit invalidates it and R21 refuses to run (exit 8).
- **I-5 Determinism.** Same (intent, seed) under mock ⇒ byte-identical triple.
- **I-6 Fail-closed defaults.** No `## Allowed` ⇒ refuse (not allow-all); unknown program effect
  declaration ⇒ full set (deny-by-default, inherited from R21).

**Edge cases (named, with resolution):**
- Intent permits more than the program needs → grant clamps to need (I-2), not the ceiling.
- Program needs more than the intent permits → `GrantExceedsCeiling` exit 8 (the human must widen the
  intent, explicitly).
- Lifted (non-generated) program from a fenced block → same admissibility + confidence gates apply
  (a hand-written block is not exempt).
- Synthesis returns ```axon-fenced output → fences stripped before type-check.
- `--approvers N>1` → exit 2 pointing at R24 (don't silently single-approve a high-risk job).
- Live model unavailable / no key → mock path still compiles+tests; the live path is opt-in only.
- A program that type-checks but writes an output the intent didn't list → `outputs-mismatch` refusal
  (prevents scope-expansion smuggled past the human).

---

## §9 — Quickstart (`README-axon-intent.md`; these exact commands are executed by `acc_a3`)
```bash
# Build
cargo build -p axon-intent --bin axon-intent
cargo build -p axon-os --bin axon-os

# 1. Turn prose intent into a proven-bounded job (synthesize → least-privilege grant → prove):
AXON_AI_MOCK=1 axon-intent compile examples/intents/summarize.intent.md --out ./jobs

# 2. See, in plain English, exactly what it will be allowed to do + the risk:
axon-intent review ./jobs/summarize.axjob --program ./jobs/summarize.ax

# 3. Approve the PROOF (binds the exact program+grant):
axon-intent approve ./jobs/summarize.axjob --by alice --accept

# 4. Run it under that approved bound (R21 honors the approval token):
axon-os run ./jobs/summarize.axjob --out ./runs

# 5. Watch an under-specified intent get REFUSED, not best-efforted (exit 5):
AXON_AI_MOCK=1 axon-intent compile examples/intents/vague.intent.md ; echo "exit=$?"
```

---

## §10 — Acceptance gate (pinned; FAILS if any check is missing or stubbed)

`scripts/r22_acceptance_gate.sh` is the single source of "done." It MUST:
1. **Presence check** — assert every §0 check name exists in the test sources:
   `acc_a1_smoke_intent_to_approval`, `acc_a2_example_intents_compile_and_run`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_synthesis_isolated_timeout`,
   `acc_a5_deterministic_compile`, `acc_a6_approval_token_binds_and_tamper_detected`,
   `synthesized_job_is_self_admissible`, `grant_is_least_privilege`,
   `low_confidence_synthesis_refused`, `approval_invalidated_by_any_edit`. Missing → **gate fails**.
2. **Anti-stub check** — each acceptance test body has a real assertion and is not `#[ignore]`d /
   `todo!()` / `assert!(true)` (grep these anti-patterns → fail).
3. **Run** `cargo test -p axon-intent` (all green) + the §9 quickstart block against the built
   binaries (A3) + `acc_a1` driving the real CLI + the cross-spec
   `r21_refuses_edited_program_after_approval`.
4. **Reproducibility** — run `acc_a5` twice and diff the emitted triples byte-for-byte.
5. Exit 0 only if all pass; else print which check failed. Wire into `gate.sh --strict`.

---

## §11 — Definition of Done
**Per slice (S1–S9):** the slice's named tests were written first, seen to fail, now pass; full
`axon-intent` suite green; no workspace regression.
**Per milestone (R22 complete):** `cargo build -p axon-intent` produces `axon-intent`; the real
example intents compile (or refuse) end-to-end; **`acc_a1` passes through the real CLI**;
reproducibility (`acc_a5`) and approval-binding/tamper-evidence (`acc_a6`,
`approval_invalidated_by_any_edit`) hold; an under-specified intent is refused (exit 5); a post-
approval edit is rejected by R21 (cross-spec); and `scripts/r22_acceptance_gate.sh` exits 0 with every
§0 check green.

---

## §12 — Notes for the implementer (do NOT deviate without updating this spec)
- **Reuse R21's types and gate** (`Grant`, `JobManifest`, `DeclaredEffects`, `gate::admit`). Do not
  reimplement capability/admission logic — import it.
- Keep `intent/grant_infer/admit/confidence/approval/pipeline` pure. `std::fs`/`SystemTime`/`rand`/
  `std::env` belong only in `synth/emit/cli`.
- The `Synthesizer` trait is the seam: build S1–S5 entirely against `MockSynth`; only S6 touches a
  model.
- **Refuse, never downgrade** (I-1). There is no "ship a lower-quality program" path.
- **Least privilege, never the ceiling** (I-2): grant what the program needs ∩ what the intent
  permits — not the maximum the intent would allow.
- The Approval token is content-binding, **not** a signature (A6). Do not claim authentication R22
  doesn't provide; HW-rooted signing is `VISION_OS` §5 G6 / a later spec.
- The one R21-side addition (honor the approval token on handoff, §3.5) is implemented under R21's
  gate, not here — but it is part of R22's Definition of Done via the cross-spec test.
