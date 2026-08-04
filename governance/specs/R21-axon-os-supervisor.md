# Tech Spec — R21: `axon-os` Containment Supervisor (Vision-OS v1 core)

**Spec ID:** `R21-axon-os-supervisor`
**Status:** ✅ Landed 100% (re-verified 2026-07-18) — grant algebra (`grant.rs`), admission gate
(fail-closed on declared-effects ⊆ grant), hash-chained tamper-evident run records, deterministic
replay (`replay.rs`), attenuated Principal minting all implemented and tested (`cargo test -p
axon-os`: 88 tests, 1 intentionally ignored, 0 failed). Landed via commits `1e7be26`→`9f12499`
("R21 100%") + the runtime capability-ceiling fix `59f84c0`. This header said "Draft" long after
the code shipped — the same staleness class as R17/R22/R23/R31/R32, caught by the same outer-loop
sweep (`EXECUTION_MODEL.md` §2).
**Implements:** `VISION_OS.md` v1 "Containment Substrate" — the beating heart of the
intent→**prove-bound**→enforce→audit→replay loop, as a runnable artifact built on shipped
Axon primitives (Principal/Budget/Sandbox/provenance/replay/R20 mint proofs).
**Audience:** an implementer who builds *strictly* against this document and reads only it.

```spec-meta
id: R21-axon-os-supervisor
status-claim: Landed
depends-on: R20-smt-capability-proofs
blocks: none
blocked-by: none
supersedes: none
related: R22-intent-approve-gateway, R26-confidential-microvm-substrate, R27-corrigibility-resource-bounds, R36-full-asi-os
conflicts-with: none
reserves: none (dual-numbered with R21-decimal.md — see scripts/verify_all_specs.sh KNOWN_DUAL comment; reconciled 2026-07-18, both specs genuinely independent)
evidence: cargo test -p axon-os --no-default-features (88 tests, re-verified 2026-07-18)
```

> **Read this framing first.** This spec is deliberately small. It is **not** the whole OS —
> no micro-VM, no bare metal, no info-flow propagation, no model synthesis (see §1.2). It is
> the one vertical slice that makes the containment thesis a real program a user can run today:
> *give me an untrusted Axon program + a capability grant, and I will provably bound, enforce,
> audit, and replay what it does — failing closed on any over-reach.*

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey + smoke test through the actual CLI | §5, §7 | `acc_a1_smoke_user_journey` |
| **A2** | Real runnable example artifact (not a toy in the test dir) | §5.6, §7 | `acc_a2_example_jobs_run_and_overreach_denied` |
| **A3** | Quickstart whose exact commands are executed by a test | §9, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Hermetic, isolated execution + hard timeout, canonical entrypoint | §4.4, §7 | `acc_a4_hermetic_isolated_timeout` |
| **A5** | Deterministic & reproducible (byte-identical across runs) | §4.5, §7 | `acc_a5_deterministic_byte_identical` |
| **A6** | Integrity: tamper-evident run record, fail-closed validation | §3.4, §4.3, §7 | `acc_a6_record_tamper_detected` |
| **Core** | Static admission gate (declared effects ⊆ grant), fail closed | §4.1, §7 | `gate_denies_effect_outside_grant` |
| **Core** | Runtime enforcement: over-reach fails closed with distinct code | §4.2, §7 | `runtime_overreach_fails_closed` |
| **Core** | Attenuation: supervisor cannot grant more than it holds | §4.1, §7 | `mint_cannot_exceed_supervisor_grant` |
| **Core** | Replay reproduces + verifies the record | §4.6, §7 | `replay_reproduces_and_verifies` |
| **Gate** | The acceptance gate itself fails if any check above is missing/stubbed | §10 | `scripts/acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does
`axon-os` is a CLI supervisor that runs an untrusted Axon program under a **declared capability +
budget grant**, and:

1. **Statically admits or rejects** the program — proves its declared effect row ⊆ the grant
   (fail closed, no execution, if it could exceed the grant).
2. **Mints a Principal** holding *exactly* the grant — attenuation is by construction (R20-proven:
   the supervisor cannot mint authority it does not itself hold).
3. **Runs the program sandboxed** to the grant's effect ceiling + budget meter + a fixed seed.
4. **Audits** every capability-bearing action into a **hash-chained, tamper-evident run record**.
5. **Fails closed** on any runtime over-reach (effect ∉ ceiling, budget exhausted, refinement
   violation) with a distinct exit code and an audited reason.
6. **Replays** any run deterministically and **verifies** the record's integrity.

### 1.2 What it explicitly does NOT do (out of scope for R21)
- **No micro-VM / bare-metal substrate.** R21 runs the program as an isolated host **subprocess**
  (A4). The substrate dial (`VISION_OS.md` §5) is future work.
- **No proof certificates / Z3-out-of-TCB** (`VISION_OS.md` §4.1 G2). R21's static gate uses the
  shipped effect-row check; it does not emit/verify SMT certificates. Documented trust gap.
- **No information-flow *propagation*** (`VISION_OS.md` §4.2 G4). R21 records a coarse confidentiality
  `max_label` on the grant and refuses a program declaring a higher label, but does not track label
  flow through computation. Documented.
- **No model synthesis loop.** The program is *provided*; the intent→synthesize half is a separate
  spec. R21 is the *run-and-contain* half.
- **No hardware root of trust / remote attestation / signatures** (`VISION_OS.md` §5 G6). The record
  is tamper-*evident* (hash-chained, content-addressed) but **not** cryptographically *signed* — A6
  documents exactly what is vs isn't authenticated.
- **No GPU/UI, no general drivers, no networking stack of our own.**

### 1.3 Persona / ICP
A **security-conscious operator** running an AI-authored or otherwise-untrusted Axon task on their
machine, who needs a *provable, audited, replayable* bound on what it can do — not a sandbox they have
to trust by reputation.

### 1.4 Interface & tech constraints
- **Interface:** a CLI binary `axon-os` (subcommands `run`/`replay`/`verify`/`explain`), plus a
  library crate.
- **Language/deps:** Rust, new workspace crate `crates/axon-os`. Allowed deps: `sha2` (already in the
  workspace), `serde`+`serde_json` for the record, a TOML parser already in the workspace if present
  else hand-rolled (see §3.1), and `axon-core`/the `axon` binary as the runtime. **No** new heavy
  deps without justification.
- **Perf/security:** core logic is pure and I/O-free; all untrusted execution is in an isolated
  subprocess with a hard timeout; fail closed on every ambiguity.

---

## §2 — Architecture & modules

New crate `crates/axon-os/`. **Core logic is pure (no I/O, no clock/random); all I/O and process
spawning live behind the `Runtime` trait** so the supervisor is fully testable with a mock.

```
crates/axon-os/src/
  manifest.rs     Parse + validate a `.axjob` job manifest → JobManifest.            [PURE]
  grant.rs        The Grant model (caps, budget, label) + subset/admission algebra.   [PURE]
  gate.rs         Static admission gate: declared effect row ⊆ grant → Admit|Deny.    [PURE]
  record.rs       RunRecord + AuditEvent: hash-chained build + verify (sha2).         [PURE]
  verdict.rs      Verdict enum + exit-code mapping.                                    [PURE]
  supervisor.rs   Orchestrates gate→mint→run→meter→record. Generic over Runtime.       [PURE core, I/O injected]
  runtime.rs      `trait Runtime` (the seam) + `AxonCoreRuntime` real impl.            [I/O — the only impure module]
  replay.rs       Deterministic replay + record verification.                          [PURE core, Runtime injected]
  cli.rs          Arg parse → command dispatch → human-facing output + exit codes.    [I/O — thin]
  lib.rs          Public API re-exports.                                               [—]
  main.rs         `fn main()` → cli::run(std::env::args).                              [I/O — thin]
crates/axon-os/tests/
  acceptance.rs   The A1–A6 + Core acceptance checks (named exactly per §0).
examples/jobs/
  summarize.axjob + summarize.ax   A real, runnable allowed job (A2).
  overreach.axjob + overreach.ax   A real job that tries to exceed its grant (A2 negative).
scripts/acceptance_gate.sh         The pinned gate (§10).
README-axon-os.md                  Quickstart whose commands a test executes (A3).
```

**Dependency graph (acyclic; arrows = "imports/uses"):**
```
main → cli → supervisor → {gate → grant, record → verdict, runtime}
                         replay → {record, runtime}
manifest → grant
gate → manifest, grant
record → verdict
runtime → (axon-core / `axon` subprocess)   [the ONLY edge to the outside world]
```
Rule the implementer MUST hold: **nothing under `manifest/grant/gate/record/verdict/supervisor/replay`
may perform I/O, read the clock, or call random.** Volatile inputs (seed, time, the run-id source) are
passed in explicitly (A5). Only `runtime.rs`, `cli.rs`, `main.rs` touch the outside world.

---

## §3 — Data model

### 3.1 `JobManifest` (parsed from a `.axjob` TOML file)
```
JobManifest {
    program:  PathBuf,   // path to the .ax program, resolved relative to the manifest's dir
    intent:   String,    // human prose: what the job is for (shown by `explain`, recorded)
    seed:     u64,       // the fixed RNG seed for deterministic execution (A5)
    grant:    Grant,
}
```
`.axjob` serialized form (TOML; if no workspace TOML dep exists, hand-roll a parser for exactly this
flat schema — keys below, no nesting beyond `[grant]`/`[grant.budget]`):
```toml
program = "summarize.ax"
intent  = "Summarize ./data/report.txt into ./out/; no network."
seed    = 42
[grant]
fs_read   = ["./data/"]
fs_write  = ["./out/"]
net       = []            # allowlist of hosts; [] = no net
exec      = "none"        # "none" | "any"
max_label = "internal"    # confidentiality ceiling: public < internal < secret
[grant.budget]
calls       = 100         # max capability-bearing actions
tokens      = 50000       # max model tokens
cost_micro  = 1000000     # max metered cost in micro-USD
```
Validation (fail → `Verdict::Malformed`, exit 2, with a line/field-specific message):
- `program` exists and ends in `.ax`; `seed` is a u64; `exec ∈ {none, any}`;
  `max_label ∈ {public, internal, secret}`; every `fs_read`/`fs_write` is a non-empty path prefix
  **with no `..` component** (path-traversal denied, mirroring E1001); budget fields are ≥ 0.

### 3.2 `Grant`
```
Grant {
    fs_read:   Vec<PathPrefix>,   // allowlisted read prefixes
    fs_write:  Vec<PathPrefix>,   // allowlisted write prefixes
    net:       Vec<Host>,         // allowlisted hosts ("*.x.com" glob ok)
    exec:      ExecPolicy,        // None | Any
    max_label: Label,             // Public(0) < Internal(1) < Secret(2)
    budget:    Budget,
}
Budget { calls: i64, tokens: i64, cost_micro: i64 }   // mirrors examples/stdlib budget ResBudget
```
**Effect-set view** used by the gate: a `Grant` induces an allowed effect set
`{FS_READ?, FS_WRITE?, NET?, EXEC?}` (a capability axis is "present" iff its allowlist is non-empty /
exec≠none). `grant.allows(effect)` is the admission predicate.

### 3.3 `DeclaredEffects` (extracted from the program)
```
DeclaredEffects {
    row:        EffectSet,   // {FS_READ, FS_WRITE, NET, EXEC} the program declares it may perform
    max_label:  Label,       // highest confidentiality the program declares it handles
}
```
Source of truth: the program's effect-row annotations / `@[contained]` spec, obtained via the
`Runtime::declared_effects(program)` call (real impl: `axon ast review --json`, parse the effect
field; see §4.7). A program with **no** declaration is treated as declaring the **full** effect set
(deny-by-default: unknown ⇒ maximal, so it must be granted everything or be denied).

### 3.4 `RunRecord` + `AuditEvent` (the tamper-evident artifact; JSON, schema `axon-os-record/1`)
```
RunRecord {
    schema:          "axon-os-record/1",
    run_id:          String,        // caller-supplied unique id (A5: injected, not generated in core)
    manifest_digest: String,        // "axsha256:" + sha256(canonical manifest bytes)
    seed:            u64,
    events:          Vec<AuditEvent>,
    verdict:         Verdict,
    record_digest:   String,        // "axrec2:" + the sealed hash-chain head (see §4.3)
}
AuditEvent {
    seq:       u64,        // 0,1,2,… monotonic
    action:    String,     // e.g. "fs_write", "net", "exec", "model_call"
    target:    String,     // the concrete object (path/host/model), or "" 
    caps_used: EffectSet,  // which capability axis this exercised
    label:     Label,      // confidentiality of the data involved
    prev_hash: String,     // the previous event's `hash` (or manifest_digest for seq 0)
    hash:      String,     // sha256(prev_hash ‖ "\x1f" ‖ canonical(seq,action,target,caps_used,label))
}
```
`canonical(...)` = the fields joined by `\x1f` in the fixed order listed, UTF-8, no whitespace.
The chain makes the record tamper-evident: altering any event (or reordering) breaks every
subsequent `hash` and the `record_digest`. **Authenticated:** integrity/ordering of the recorded
events + the manifest they ran under. **NOT authenticated (documented for A6):** *who* produced the
record (no signature) and *that the recorder saw every action* (the recorder is trusted; a HW root of
trust + signing is `VISION_OS.md` §5 G6, out of scope).

### 3.5 `Verdict` + exit codes (§4 maps every outcome to exactly one)
```
Verdict =
  | Completed { value: i64 }          → exit 0
  | Malformed { reason: String }      → exit 2   // bad manifest/usage
  | Denied    { reason, axis }        → exit 8   // static gate OR runtime capability/sandbox violation
  | BudgetExhausted { axis }          → exit 7
  | RefineViolation { reason }        → exit 6
  | VerifyMismatch  { detail }        → exit 9   // record tamper / replay divergence
```
These reuse Axon's carved exit scheme (6 refine, 7 budget, 8 sandbox) so codes are consistent across
the stack; 9 is new and owned by `axon-os`.

---

## §4 — Core logic / algorithms

### 4.1 The static admission gate (`gate::admit`)  — Core, fail-closed
Input: `JobManifest`, `DeclaredEffects`. Output: `Admission = Admit | Deny{reason, axis}`.
Steps, in order; the **first** failure denies (no execution happens on a Deny):
1. For each effect `e ∈ declared.row`: if `!grant.allows(e)` → `Deny{ "program may perform {e} but the grant withholds it", axis=e }`.
2. If `declared.max_label > grant.max_label` → `Deny{ "program handles {label} data above the grant ceiling {grant.max_label}", axis=Label }`.
3. Else `Admit`.
**Attenuation invariant (Core test `mint_cannot_exceed_supervisor_grant`):** the supervisor runs under
its *own* grant `S`; the job grant `J` it hands a program must satisfy `J ⊆ S` on every axis (caps
subset, budget ≤, label ≤). `gate::admit` is called with the *effective* grant `J ∩ S`; a manifest
asking for more than `S` is clamped to `S` (you cannot delegate authority you lack — the R20 mint
property at the supervisor boundary). This clamp is computed by `grant::intersect(J, S)`.

### 4.2 The run pipeline (`supervisor::run`) — Core orchestration, I/O via `Runtime`
```
fn run(manifest, supervisor_grant, run_id, rt: &impl Runtime) -> RunRecord
```
1. `declared = rt.declared_effects(&manifest.program)`  (deny-by-default on error/empty → full set).
2. `eff = grant::intersect(manifest.grant, supervisor_grant)`.
3. `admission = gate::admit(&manifest, &declared, &eff)`.
   - On `Deny{reason,axis}` → build a 1-event record (the denial) + `Verdict::Denied`; **return without
     executing the program.** (Fail closed BEFORE run.)
4. `principal = rt.mint_principal(&eff)`  // holds exactly `eff`; attenuation guaranteed by the runtime.
5. `outcome = rt.run_sandboxed(&manifest.program, &principal, eff.effect_set(), &eff.budget, manifest.seed)`
   - The runtime enforces the ceiling + budget + seed and returns `RunOutcome { events, verdict }`,
     where each event is an observed capability-bearing action and `verdict` is one of the §3.5
     variants (the runtime maps exit 6/7/8 from the sandboxed process to RefineViolation/Budget/Denied).
6. Build the hash chain from `outcome.events` (§4.3), seal with `outcome.verdict`, return the
   `RunRecord`.
The supervisor performs **no** I/O itself; it is pure given `rt`. This is what makes the mock-Runtime
tests in §7 possible.

### 4.3 Record construction & verification (`record::build` / `record::verify`) — Core
- `build(run_id, manifest, seed, events, verdict)`:
  `manifest_digest = "axsha256:"+sha256(canonical_manifest_bytes)`; fold events into the chain
  (`prev_hash` of seq 0 = `manifest_digest`); then fold a terminal **seal** over
  `(run_id, seed, canonical_verdict)`; `record_digest = "axrec2:"+seal_hash`.
- `verify(record) -> Result<(), VerifyMismatch>`: recompute `manifest_digest` is **not** possible
  without the manifest, so `verify` recomputes the **event chain** from the stored events +
  `manifest_digest`, recomputes the seal, and asserts every `hash` and the final `record_digest`
  match. Any mutation (changed field, dropped/reordered/inserted event, **or a rewritten
  run_id / seed / verdict**) → `Err(VerifyMismatch{which})`. **Pure, no I/O.**
- **AUDIT T47 (finding P6-EXIT-03).** The chain originally stopped at the event head
  (`"axrec1:"`), leaving `run_id`, `seed` and `verdict` **outside** it. Executed against a real
  sealed record: rewriting the verdict from `Completed{value:3}` to `Denied{axis:"sandbox"}`,
  the seed from 42 to 999 and the run_id to another run's id still verified `✓ intact` with a
  byte-identical digest and exit 0 — while tampering any chained *event* field correctly gave
  exit 11. The record was tamper-evident for the fields nobody needs to forge and silent on the
  one it exists to attest. `verify` now **refuses** an `axrec1:` record rather than accepting it:
  an attacker chooses which format to present, so accepting the pre-seal form is a free
  downgrade back to an unauthenticated verdict.

### 4.4 Hermetic isolated execution (`AxonCoreRuntime::run_sandboxed`) — the impure seam (A4)
- The program runs in a **fresh subprocess** invoking the canonical `axon` entrypoint (resolved once,
  by absolute path from config/env `AXON_BIN`, not via ambient PATH search at call time).
- A **hard wall-clock timeout** (`AXON_OS_TIMEOUT_MS`, default 30000) wraps the child; on expiry the
  child process group is killed and the run is `Verdict::Denied{"timeout", axis=Time}` (fail closed).
  No leaked child handles/threads (RAII guard that kills on drop).
- The sandbox + principal + seed are imposed **externally** by generating a tiny wrapper program that
  mints the principal and runs the user program inside `sandbox_run` with the effect ceiling, then
  invoking `axon run` on the wrapper with `AXON_SEED=<seed>` and an audit-log path; the audit JSONL +
  exit code are parsed back into `RunOutcome`. (See §4.7 for the exact axon-core touchpoints.)
- No reliance on cwd-relative ambient state: paths are resolved against the manifest dir; the child
  gets a minimal, explicit environment.

### 4.5 Determinism (A5)
- The **only** volatile inputs are `seed` (manifest), `run_id` (injected by `cli`, e.g. derived from
  the manifest digest + an explicit `--run-id`, never from clock/random in core), and the program's
  own behavior. Core logic is otherwise a pure function of (manifest, supervisor_grant, declared
  effects, run outcome).
- **Contract:** two `run`s of the same job with the same seed produce **byte-identical** `RunRecord`
  JSON (modulo nothing — there is no timestamp field; if a wall-time is ever added it must live
  outside the hashed region and outside the equality check). The runtime must propagate `AXON_SEED`
  so the interpreter's `random_*` is reproducible (the engine already supports this).

### 4.6 Replay (`replay::run`)
`axon-os replay <run-id>` loads the stored `RunRecord` + its manifest, re-executes via the same
pipeline with the **recorded seed**, and asserts the new record is byte-identical to the stored one;
divergence → `Verdict::VerifyMismatch` exit 9. It also runs `record::verify` on the stored record
first (tamper check before trusting it). This is the deterministic-audit guarantee.

### 4.7 `axon-core` touchpoints (the implementer wires exactly these; everything else is `axon-os`)
- **Declared effects:** `axon ast review --json <program>` → read the effect-row/attrs field.
  (Fallback if unavailable: parse `@[contained(...)]` / `| {…}` annotations from source; absent ⇒
  full set.)
- **Sandbox + principal + budget:** compose `principal_root`/`principal_mint` (R20-proven attenuation)
  + `sandbox_create(principal, ceiling)` + `sandbox_run("entry", arg)` in the generated wrapper; the
  interpreter raises `SandboxViolation`→exit 8, budget→exit 7, refine→exit 6 (already implemented).
- **Audit stream:** the provenance JSONL (`agent_action`/`ai_call` records carry effect_row +
  principal) is the event source; map each to an `AuditEvent`.
- **Seed:** `AXON_SEED` env.
The `Runtime` trait is the firewall: `AxonCoreRuntime` is the *only* place these touchpoints appear,
so the rest of `axon-os` is unit-testable with a `MockRuntime`.

---

## §5 — Public API / interface contract

### 5.1 Library API (`lib.rs`)
```
pub fn admit(manifest:&JobManifest, declared:&DeclaredEffects, grant:&Grant) -> Admission;
pub fn run(manifest:&JobManifest, supervisor_grant:&Grant, run_id:&str, rt:&impl Runtime) -> RunRecord;
pub fn verify(record:&RunRecord) -> Result<(), VerifyMismatch>;
pub fn replay(run_id:&str, store_dir:&Path, rt:&impl Runtime) -> Result<RunRecord, VerifyMismatch>;
pub trait Runtime {
    fn declared_effects(&self, program:&Path) -> DeclaredEffects;
    fn mint_principal(&self, grant:&Grant) -> PrincipalHandle;
    fn run_sandboxed(&self, program:&Path, p:&PrincipalHandle, ceiling:EffectSet,
                     budget:&Budget, seed:u64) -> RunOutcome;
}
```

### 5.2 CLI (every subcommand has `--help`; output is human-legible, not just exit codes)
```
axon-os explain <job.axjob>
    Print the LEGIBLE grant (VISION_OS §4.4): "This program may: read ./data/, write ./out/;
    may NOT: use the network, spawn processes. Budget: ≤100 calls / 50k tokens / $1.00.
    Confidentiality ceiling: internal." + the static gate verdict (ADMIT / DENY+reason).
    Performs NO execution. Exit 0 if admitted, 8 if denied.   [pre-approval simulation]

axon-os run <job.axjob> [--run-id ID] [--out DIR] [--supervisor-grant FILE]
    Gate → mint → run sandboxed → write record to <DIR>/<run-id>.json. Prints the verdict in
    plain English ("✓ completed (value=…)" / "⚠ DENIED: net withheld by grant" / "budget
    exhausted: tokens" …) + the run-id. Exit = verdict code (§3.5).

axon-os verify <record.json>
    Recompute the hash chain; print "✓ intact" or "✗ TAMPERED at event N: <field>". Exit 0 / 9.
    No execution.

axon-os replay <run-id> [--store DIR]
    verify the stored record, re-run with the recorded seed, assert byte-identical. Print
    "✓ replay identical" / "✗ DIVERGED: <detail>". Exit 0 / 9.
```
Usage/`--help` on a bad invocation → exit 2 with a helpful message naming the expected form.

### 5.6 Shipped example artifacts (A2 — real, in `examples/jobs/`, runnable immediately)
- `summarize.ax` + `summarize.axjob`: reads `./data/report.txt`, writes `./out/summary.txt`, **no
  net** — declares `| {FS_READ, FS_WRITE}`, grant matches → **Completed**.
- `overreach.axjob` (+ `overreach.ax`): the *same* program but its `.ax` also attempts a `net` action
  while the grant withholds `net` → **Denied exit 8** (the headline negative demo: the supervisor
  refuses the over-reach, audited).

---

## §6 — Build order (each slice ends green before the next; TDD: test first, see it fail, make it pass)

- **S1 — Data model + manifest parse/validate.** `verdict.rs`, `grant.rs`, `manifest.rs`. Tests:
  parse the valid example; reject each malformed case (bad exec, `..` path, negative budget, missing
  program) → `Malformed`. Green.
- **S2 — Grant algebra.** `grant::allows`, `grant::intersect`, subset/label ordering. Tests:
  intersection clamps to the smaller on every axis; label ordering; `mint_cannot_exceed_supervisor_grant`.
- **S3 — Static gate.** `gate::admit`. Tests: `gate_denies_effect_outside_grant`,
  admit-when-subset, label-ceiling deny, deny-by-default on empty declaration.
- **S4 — Record + verify.** `record::build`/`verify`. Tests: chain builds; `acc_a6_record_tamper_detected`
  (mutate each field/reorder → VerifyMismatch); equal inputs → equal digest.
- **S5 — Supervisor over a MockRuntime.** `supervisor::run` + `runtime::MockRuntime`. Tests:
  admit→run→record happy path; `runtime_overreach_fails_closed` (mock returns exit-8 outcome →
  Denied, recorded); deny-before-run (gate Deny ⇒ MockRuntime.run_sandboxed is **never called** —
  assert via a call counter).
- **S6 — `AxonCoreRuntime` (the real seam) + hermetic exec.** Wire §4.7. Tests:
  `acc_a4_hermetic_isolated_timeout` (a `while true {}` program is killed at the timeout, exit
  Denied, no leaked child).
- **S7 — CLI + explain/run/verify/replay + human output.** `cli.rs`, `main.rs`. Tests:
  `acc_a5_deterministic_byte_identical`, `replay_reproduces_and_verifies`, `--help` on every
  subcommand, usage error → exit 2.
- **S8 — Example artifacts + smoke + quickstart.** `examples/jobs/*`, `README-axon-os.md`. Tests:
  `acc_a1_smoke_user_journey`, `acc_a2_example_jobs_run_and_overreach_denied`,
  `acc_a3_quickstart_commands_execute`.
- **S9 — Acceptance gate.** `scripts/acceptance_gate.sh` (§10). Green = done.

---

## §7 — Test plan (happy + **adversarial**; every named test is normative)

**Unit / core (pure, fast):**
- `manifest_rejects_malformed` — each of: non-`.ax` program, `exec="root"`, `fs_write=["../etc"]`
  (path traversal), `tokens=-1`, missing file → `Malformed` exit 2.
- `gate_denies_effect_outside_grant` — declared `{NET}` vs grant without net → `Deny{axis=NET}`,
  and **assert no run occurs**.
- `mint_cannot_exceed_supervisor_grant` — manifest asks net+exec; supervisor grant has neither;
  `intersect` clamps to ∅; gate denies. Authority cannot be manufactured.
- `gate_label_ceiling` — program max_label=secret, grant=internal → Deny.
- `runtime_overreach_fails_closed` — MockRuntime yields each of exit 6/7/8 → Verdict
  RefineViolation/BudgetExhausted/Denied, each recorded as the sealing verdict.
- `acc_a6_record_tamper_detected` — for every field of a mid-chain event, mutate it and assert
  `verify` returns `VerifyMismatch` pointing at that event; also drop, reorder, and insert an event.
- `deny_before_run` — gate Deny ⇒ `Runtime::run_sandboxed` call-count == 0.

**Integration (real `axon` subprocess):**
- `acc_a4_hermetic_isolated_timeout` — runaway program killed at `AXON_OS_TIMEOUT_MS`; process gone;
  verdict Denied(timeout).
- `acc_a5_deterministic_byte_identical` — run the example twice with the same `--run-id` and seed;
  assert the two `RunRecord` JSON bytes are identical.
- `replay_reproduces_and_verifies` — run, then `replay`; assert identical + `verify` ✓; then corrupt
  the stored record and assert `replay`/`verify` → exit 9.
- `acc_a2_example_jobs_run_and_overreach_denied` — `summarize.axjob` → exit 0 + `./out/summary.txt`
  exists; `overreach.axjob` → exit 8 + the record shows the denied `net` event + **no** network
  actually occurred.

**User-journey smoke (A1 — drives the REAL CLI exactly as the operator would, via subprocess):**
- `acc_a1_smoke_user_journey`: (1) `axon-os explain summarize.axjob` → asserts the legible grant text
  + "ADMIT"; (2) `axon-os run summarize.axjob --run-id demo --out <tmp>` → asserts "✓ completed" +
  the output artifact; (3) `axon-os verify <tmp>/demo.json` → "✓ intact"; (4) `axon-os replay demo
  --store <tmp>` → "✓ replay identical"; (5) `axon-os run overreach.axjob` → "⚠ DENIED: net …" exit 8.
  Each step asserts **stdout text AND the on-disk artifact**, not just exit codes.

**Quickstart (A3):**
- `acc_a3_quickstart_commands_execute` — extracts the fenced command block from `README-axon-os.md`
  and executes each line verbatim against the built binary; all succeed with the documented output.

---

## §8 — Invariants & edge cases

**Invariants (must always hold; assert in tests):**
- **I-1 Fail-closed admission.** No program executes unless `gate::admit` returned `Admit`. A Deny
  produces a record with the denial event and *zero* program effects.
- **I-2 No authority manufacture.** The effective grant is always `J ∩ S` (`J ⊆ S`); a child Principal
  never holds a capability the supervisor lacks (R20 attenuation, enforced at the supervisor edge).
- **I-3 Tamper-evidence.** Any mutation/reorder of a recorded event changes `record_digest`; `verify`
  detects it. The recorder is trusted; the record is integrity-checkable.
- **I-4 Determinism.** Same (manifest, seed) ⇒ byte-identical record (no ambient clock/random in core).
- **I-5 Distinct fail-closed codes.** capability/sandbox=8, budget=7, refine=6, tamper/divergence=9,
  malformed/usage=2 — never collapse to a generic failure; the reason is always audited.
- **I-6 Hermetic.** Untrusted execution is always an isolated, time-bounded subprocess; no leaked
  handles; the canonical entrypoint is resolved unambiguously.

**Edge cases the implementer MUST handle (named, with the resolution):**
- Program with **no** effect declaration → treat as full effect set (deny-by-default); only runs if
  granted everything.
- Empty grant (`[grant]` all empty, exec=none) + a pure program → Admit, runs, completes.
- Manifest grant **exceeds** supervisor grant → clamp (I-2), do **not** error; the clamp may then
  cause a gate Deny — that's correct.
- Budget `0` on an axis the program uses → first such action → `BudgetExhausted` exit 7.
- Timeout mid-action → Denied(timeout); the partial record up to the kill is still verifiable.
- A `..` or symlink-escaping path in a grant prefix → rejected at manifest validation (path traversal,
  fail closed) — never resolved.
- Corrupt/edited stored record on `replay`/`verify` → exit 9, never a silent pass.

---

## §9 — Quickstart (`README-axon-os.md`; these exact commands are executed by `acc_a3`)
```bash
# Build
cargo build -p axon-os --bin axon-os

# 1. See, in plain English, exactly what a job is allowed to do (no execution):
axon-os explain examples/jobs/summarize.axjob

# 2. Run it under that proven bound; get an audited, replayable record:
axon-os run examples/jobs/summarize.axjob --run-id demo --out ./runs

# 3. Confirm the record hasn't been tampered with:
axon-os verify ./runs/demo.json

# 4. Reproduce the run deterministically and prove it matches:
axon-os replay demo --store ./runs

# 5. Watch the supervisor REFUSE an over-reaching job (exit 8, audited):
axon-os run examples/jobs/overreach.axjob --out ./runs ; echo "exit=$?"
```

---

## §10 — Acceptance gate (pinned; FAILS if any check is missing or stubbed)

`scripts/acceptance_gate.sh` is the single source of "done." It MUST:
1. **Presence check** — `grep` the test sources and assert every named check from §0 exists:
   `acc_a1_smoke_user_journey`, `acc_a2_example_jobs_run_and_overreach_denied`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_hermetic_isolated_timeout`,
   `acc_a5_deterministic_byte_identical`, `acc_a6_record_tamper_detected`,
   `gate_denies_effect_outside_grant`, `runtime_overreach_fails_closed`,
   `mint_cannot_exceed_supervisor_grant`, `replay_reproduces_and_verifies`.
   Any missing name → **gate fails**.
2. **Anti-stub check** — assert each acceptance test body contains a real assertion and is not
   `#[ignore]`d / `todo!()` / `assert!(true)` (grep for those anti-patterns → fail).
3. **Run** `cargo test -p axon-os` (all green) **and** execute the §9 quickstart block against the
   built binary (A3) **and** run `acc_a1` driving the real CLI.
4. **Reproducibility** — run `acc_a5` twice and diff the two records byte-for-byte.
5. Exit 0 only if all of the above pass; print which check failed otherwise.
Wire `acceptance_gate.sh` into the repo's `gate.sh --strict`.

---

## §11 — Definition of Done
**Per slice (S1–S9):** the slice's named tests were written first, were seen to fail, now pass; the
full `axon-os` suite is green; no regression in the workspace.
**Per milestone (R21 complete):** `cargo build -p axon-os` produces the `axon-os` binary; the real
example jobs run end-to-end; **`acc_a1` passes through the real CLI**; reproducibility (`acc_a5`) and
tamper-evidence (`acc_a6`) hold; the over-reach job is denied (exit 8) and audited; and
`scripts/acceptance_gate.sh` exits 0 with every §0 check green. Only then is R21 done.

---

## §12 — Notes for the implementer (do NOT deviate without updating this spec)
- Keep `manifest/grant/gate/record/verdict/supervisor/replay` **pure**. If you reach for
  `std::fs`, `SystemTime`, `rand`, or `std::env` there, you are in the wrong module — it belongs in
  `runtime.rs`/`cli.rs`.
- The `Runtime` trait is the seam that makes the core testable. Build S1–S5 entirely against
  `MockRuntime`; only S6 touches `axon`.
- Exit codes are a contract (§3.5). Never collapse a fail-closed outcome into a generic error.
- The record has **no timestamp** (determinism). If you think you need one, put it outside the hashed
  region and outside every equality assertion — or don't add it.
- Trust gaps are documented on purpose (§1.2): no signatures, no SMT certificates, no info-flow
  propagation, no VM isolation in R21. Do not silently "improve" past the spec; those are separate
  specs with their own gates.
