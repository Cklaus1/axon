# Tech Spec — R4: Three Code Zones + Compiler-Enforced Provenance

**Status:** ✅ Reviewed (2026-06-01)
**Requirement:** `../REQUIREMENTS.md` R4 — *Three code zones: Static / Adaptive / Agent + compiler-enforced provenance.*
**Decisive fork:** *How is provenance made **un-opt-out-able** (I-13), uniformly across the interpreter AND codegen — and what behaviorally distinguishes the three zones (Static / Adaptive / Agent) plus `@[experiment]`?* Today injection is interpreter-only and `@[experiment]` is a no-op synonym. **→ Resolved below.**

> Note: R4 is not in the original four "high-risk unbuilt" specs (it is 55% built), but its hard part — making provenance injection un-opt-out-able and engine-uniform — is genuinely unspecified, and REQUIREMENTS.md already references `governance/specs/R4-code-zones.md (TODO)`. This drafts it.

---

## 1. Motivation

R4 is 55% (⚠️ Partial). What exists: `@[adaptive]`/`@[agent]`/`@[goal]` parse as deferred attrs (`builtins.rs` `DEFERRED_ATTRS`), and the **interpreter** injects provenance for `@[adaptive]` automatically — `call_fn` checks `f.attrs.iter().any(|a| a.name == "adaptive")` (`interp.rs:701`) and records every return into the in-memory `provenance` store (`interp.rs:187`), with on-disk JSONL via `append_provenance_jsonl` (`interp.rs:4663`). The user cannot turn this off: it is keyed on the *annotation*, not on user cooperation. That is I-13 working — **in one engine.**

Three gaps make R4 incomplete, and they are exactly the fork:

1. **Engine non-uniformity (the I-13 hole):** the invariant text itself admits *"Partially true today; codegen-side enforcement is R4 work."* Codegen does not inject provenance. A program built native (option B once R1 lands) silently loses the un-opt-out-able guarantee — the worst kind of gap, because the guarantee *looks* present (it works in the interpreter, the path everyone tests). Codegen's `has_adaptive_attr` (codegen/mod.rs:72) is a predicate-only check that gates IR emission, but the injection is still a compile-time IR pattern with no conformance tripwire.
2. **`@[experiment]` is a no-op synonym:** it sits in `DEFERRED_ATTRS` (builtins.rs:1178) but nothing branches on it — the interp checks `"adaptive"` at interp.rs:701 and never matches `"experiment"`. The PRD's "three zones + experiment" distinction does not exist in behavior.
3. **`@[agent]` action logging is not mandatory:** agents are the highest-trust zone, yet no compiler-injected action log is enforced; provenance covers `@[adaptive]` returns only.

This spec resolves the fork by defining (a) the **zone taxonomy** precisely, (b) the **injection contract** that both engines must satisfy, and (c) a **conformance test** that fails for either engine if injection is absent — so the guarantee can't silently rot in codegen.

Design-only; no code until **Reviewed** (Gate 1). Interpreter is reference (I-2); the codegen half is specified-not-built and R1-gated for *native* enforcement, but the *contract* and the interpreter conformance are settle-and-testable now.

---

## 2. Requirement link

`../REQUIREMENTS.md` **R4** (55%, ⚠️ Partial). Quoted acceptance:

> *Adaptive fn cannot compile without provenance injection; agent action log mandatory.*

Two binary gates:
1. **No-opt-out:** an `@[adaptive]`/`@[experiment]` fn that executes *always* produces provenance; there is no flag, no code path, no engine that runs it un-logged. "cannot compile without provenance injection" → the injection is a compiler pass, not a library call a user can omit.
2. **Agent log mandatory:** every `@[agent]` action (tool call / decision) produces an audit record, compiler-injected.

Dependencies: **I-13** (the invariant this realizes), **I-2** (interpreter reference; codegen must match), **R1** (native *enforcement* — but not the interpreter conformance), **R3** (the `AuditEvent`/provenance schema overlaps the AI-call record), **I-11** (agent actions cross the capability boundary).

---

## 3. Surface (what the user writes)

```axon
// STATIC zone — the default. No annotation. Pure, no provenance, no special
// treatment. The compiler treats it as ordinary code.
fn score(x: i64) -> i64 { x * x }

// ADAPTIVE zone — @[adaptive]. The compiler INJECTS return-value provenance at
// every call. Eligible for goal_run hill-climb. Un-opt-out-able.
@[adaptive]
fn tune(x: i64) -> i64 { 100 - (x - 7) * (x - 7) }

// EXPERIMENT zone — @[experiment]. Like adaptive (provenance injected) but
// flagged as NOT graduation-eligible: an experiment's results are recorded for
// comparison but never auto-promoted. Distinct `zone:"experiment"` in the log.
@[experiment("baseline-v2")]
fn variant(x: i64) -> i64 { x + 1 }

// AGENT zone — @[agent]. Highest trust. The compiler injects an AUDIT record
// for every tool call / decision the agent makes, not just returns. Mandatory.
@[agent]
fn planner(goal: str) -> str { /* tool calls are each logged */ "" }
```

```
axon zones prog.ax          # NEW: print each fn's zone + whether provenance is injected (JSON)
axon trace --zone agent <run>   # filter provenance by zone
axon verify-provenance prog.ax  # NEW: static check that every adaptive/agent fn WILL inject (E15xx)
```

---

## 4. Semantics

### 4.1 The zone taxonomy (fork resolution part 1)

| Zone | Annotation | Provenance injected | Graduation-eligible | Audit log |
|---|---|---|---|---|
| **Static** | (none) | No | — | No |
| **Adaptive** | `@[adaptive]` | Return value, every call | Yes (`goal_run`) | No |
| **Experiment** | `@[experiment(label)]` | Return value, every call, tagged `zone:"experiment"` + label | **No** (recorded, never auto-promoted) | No |
| **Agent** | `@[agent]` | Every tool call + decision (not just return) | — | **Yes, mandatory** |

The distinction `@[experiment]` vs `@[adaptive]` is **behavioral**: both inject return provenance, but experiment records carry a `zone:"experiment"` + label and are excluded from `goal_run`'s "best" computation (an experiment is a *comparison baseline*, not an optimization target). This makes the PRD's third zone real instead of a synonym.

### 4.2 The injection contract (fork resolution part 2 — the engine-uniform part)

**Decision:** provenance injection is a **compiler pass over the AST**, run before execution by *both* engines, not a runtime library call. The pass is keyed on the zone annotation (`f.attrs`), exactly as `interp.rs:701` already does for `@[adaptive]` — but lifted to a shared pass both `interp` and `codegen` consume, so neither can skip it.

- **Interpreter:** already conforms for `@[adaptive]` (the existing `is_adaptive` check). This spec *extends* it to `@[experiment]` (tagged) and `@[agent]` (action log), and *names* the existing behavior as the reference.
- **Codegen:** must emit the same injection at every call site of a zoned fn. Until R1's native build lands, this is specified-not-built; the **conformance test** (§8) is the guard that codegen, once buildable, matches.
- **No-opt-out mechanism:** because injection is keyed on the annotation and performed by the compiler, a user cannot write a zoned fn that doesn't inject — the only way to avoid provenance is to not annotate (i.e., be Static). There is no `#[no_provenance]` escape; proposing one would be an I-13 violation and is explicitly rejected (§7).

### 4.3 The provenance/audit record (overlaps R3 §4.3)

Reuses the existing NDJSON provenance log (`append_provenance_jsonl`). Zone records add a `zone` field:

| Field | Adaptive | Experiment | Agent |
|---|---|---|---|
| `event` | `"adaptive_return"` | `"experiment_return"` | `"agent_action"` |
| `zone` | `"adaptive"` | `"experiment"` | `"agent"` |
| `fn`, `src`, `ts_ms` | ✓ (existing) | ✓ | ✓ |
| `score` / return | ✓ | ✓ | (n/a) |
| `label` | — | the experiment label | — |
| `action` | — | — | tool name / decision |
| `caps_used` | — | — | the capability the action exercised (I-11 link) |

Agent records share the `AuditEvent` shape R3's `ai_call` record uses — one audit stream, multiple event types.

### 4.4 Behavior table

| Input class | Behavior |
|---|---|
| Static fn executes | No provenance. Ordinary code. |
| `@[adaptive]` fn executes (interp) | Return recorded; `zone:"adaptive"`. **Conforms today** (`interp.rs:701`). |
| `@[experiment(l)]` fn executes | Return recorded `zone:"experiment"`, `label:l`; **excluded** from `goal_run` best. |
| `@[agent]` fn makes a tool call | One `agent_action` audit record per call, with `caps_used`. |
| `@[adaptive]` fn built via codegen (post-R1) | Must inject identically; **conformance test E15xx guards** (specified-not-built). |
| User attempts a `#[no_provenance]`-style opt-out | No such construct exists; rejected by design (I-13). |
| `@[agent]` fn with no audit injection (a codegen bug) | **Conformance fail → E1501** at the verify-provenance gate. |
| `@[experiment]` result fed to `goal_run` as the optimization target | Ignored for "best"; `goal_run` documents it skips experiment-zone records (W1510). |

### 4.5 Determinism

Provenance content is deterministic under the existing seed/mock controls (BUG_HUNT #11, R3 mock). Record *ordering* is call-order, already guaranteed by the append-only log. Two identical runs produce identical provenance — required for R9 replay.

---

## 5. Type rules

Minimal. Zones are attributes, not types; they do not change inference. One **checker** addition: `@[experiment]` takes a required string label (`@[experiment("name")]`), validated like `@[verify]`'s predicate is (a parse-time shape check, not a type rule). `@[agent]`/`@[adaptive]` remain nullary. No `parse_type_str` change. (A future `Zone` reflected into the type system — so a Static fn can't call an Agent fn without acknowledging the effect — is noted §12 Q3, deferred; that is Phase-6 effect-row territory.)

## 6. Error codes

New **E15xx / W15xx** band (code-zones / provenance — follows E14xx self-improving), per I-14.

| Code | Trigger | Message shape |
|---|---|---|
| **E1501** | A zoned fn (`@[adaptive]`/`@[experiment]`/`@[agent]`) reaches an engine that would execute it without injecting provenance (conformance check) | `` `{fn}` is @[{zone}] but the {engine} would run it without provenance injection — I-13 violation `` |
| **E1502** | `@[experiment]` without its required string label | `` @[experiment] needs a label: `@[experiment("name")]` `` |
| **E1503** | Two fns annotated with conflicting zones (e.g. both `@[adaptive]` and `@[agent]`) | `` `{fn}` has conflicting zones {z1}+{z2}; a fn is in exactly one zone `` |
| **W1510** | `goal_run` target resolves to an `@[experiment]` fn (excluded from "best") | `` `{fn}` is @[experiment] — its records are baselines, excluded from goal_run's best; use @[adaptive] to optimize `` |
| **W1511** | `@[agent]` fn makes no tool calls (audit log will be empty) | `` `{fn}` is @[agent] but performs no logged actions — is the annotation intended? `` |

## 7. Invariants touched

- **I-13 (provenance not opt-out-able):** this spec is I-13's completion. It (a) names the existing interpreter behavior as the reference, (b) defines the engine-uniform injection contract, (c) adds the conformance gate (E1501) that fails if *either* engine would skip injection. **The "Partially true today" caveat in I-13's own text is what this closes.** No opt-out construct exists or will (§4.2). **Realized + strengthened.**
- **I-2 (interpreter reference):** interpreter conformance is testable now; codegen conformance is the parity obligation, R1-gated for native but specified so it can't silently diverge. **Preserved.**
- **I-11 (capability boundary):** `@[agent]` action records carry `caps_used`, tying every agent action to the capability it exercised — provenance and capability audit become one stream. **Strengthened.**
- **I-14 (stable codes):** E15xx band defined here. **Preserved.**
- **No invariant changed** — R4 *fulfills* I-13 rather than altering it.

## 8. Test plan (maps 1:1 to §4.4)

Red test that must fail first: **`experiment_zone_is_distinct_from_adaptive`** — an `@[experiment("b")]` fn and an `@[adaptive]` fn, both run; assert the experiment's records carry `zone:"experiment"` + label AND are excluded from `goal_run`'s best, while the adaptive's are included. Fails today: `@[experiment]` is a no-op synonym (nothing reads it), so the two are indistinguishable.

- [ ] **Unit:** zone classification from `f.attrs` (exactly one zone; conflict → E1503); `@[experiment]` label required (E1502).
- [ ] **Integration:** `@[adaptive]` injects (existing behavior, now under an R4-named test); `@[experiment]` injects tagged + excluded from best; `@[agent]` emits one `agent_action` per tool call.
- [ ] **CLI e2e:** `axon zones prog.ax` lists each fn's zone + injection status; `verify-provenance` exits non-zero with E1501 on a (simulated) non-injecting engine.
- [ ] **Adversarial (the I-13 core):** there is no surface to opt out — a test asserts no attribute/flag suppresses injection for a zoned fn; `goal_run` on an experiment fn → W1510, not silent inclusion.
- [ ] **Property:** for any zoned fn and any input, executing it produces ≥1 provenance record (the no-opt-out property, quantified).
- [ ] **Parity (interp↔codegen):** the conformance test runs the SAME zoned program through both engines and asserts identical provenance; `#[ignore]`d for codegen until R1, but written now so it fails loudly the moment native builds and skips injection.
- [ ] **Journey/red-team:** an author tries to "hide" an adaptive fn's tuning from the audit by restructuring code — every path that *executes* the fn still logs, because injection is call-site, compiler-driven.

## 9. Acceptance criteria (the done gate)

R4 advances from 55% when **all** pass:

- [x] `experiment_zone_is_distinct_from_adaptive` passes (the no-op-synonym gap closed). **DONE** (cli_run.rs, `27e2829`→this slice): experiment injects tagged `zone:"experiment"`+label (I-13) but is excluded from `goal_run`'s in-memory best store.
- [x] `experiment_records_excluded_from_goal_run_best` passes (exclusion). **DONE** as `experiment_records_survive_axon_trace_and_stay_out_of_best` (goal_run on an experiment fn returns target, not an optimized value; records still readable by `axon trace`). *W1510 explicit warning: deferred — exclusion is silent-correct for now.*
- [x] `agent_action_log_is_mandatory` passes (one record per tool call — the requirement's "agent action log mandatory"). **DONE** — `call_builtin` injects an `event:"agent_action"` record (`zone:"agent"`, `action`=tool name, `caps_used`=capability kind via `capability_of_builtin`) whenever a capability-bearing builtin is called inside an `@[agent]` fn. Compiler-injected at the call site (keyed on `current_agent_fn`), so it is un-opt-out-able (I-13); a non-agent fn doing the identical call logs nothing. cli_run `agent_actions_are_mandatorily_logged`.
- [ ] `zoned_fn_has_no_opt_out` passes (the I-13 core — no construct suppresses injection).
- [ ] `conflicting_zones_rejected` passes (E1503).
- [ ] `experiment_label_required` passes (E1502).
- [ ] **(R1-gated)** `interp_codegen_provenance_parity` passes — *blocked until R1; written + `#[ignore]`d so native injection can't silently regress.*

R4 may rise 55% → ~80% on the interpreter slice (experiment distinct, agent log, no-opt-out all testable now); the final ~20% is codegen conformance, R1-gated.

## 10. Performance budget

Injection is one log append per zoned-fn call — the cost already paid by `@[adaptive]` today (`append_provenance_jsonl`). Static code (the default, the hot path) pays nothing — zones are opt-in by annotation. No new budget.

## 11. Rollout & rollback

- **Decomposed:** (1) zone classifier + E1502/E1503 (pure analysis, no behavior change); (2) `@[experiment]` distinct semantics (tag + goal_run exclusion); (3) `@[agent]` action-log injection; (4) the conformance gate + codegen parity test (interp-side now, codegen `#[ignore]`d). Each reverts to a green tree. (1) and the existing `@[adaptive]` behavior are untouched, so back-compat holds.
- **Blast radius:** the `@[experiment]` exclusion changes `goal_run`'s "best" for programs that (mis)used `@[experiment]` expecting optimization — but since `@[experiment]` is a no-op today, no existing program depends on it being optimized. Low risk, and W1510 makes the change visible.
- **Codegen / R1:** native injection enforcement is R1-gated; the interpreter (reference) ships the full guarantee, and the parity test is the tripwire that fires the instant native builds without injecting.

## 12. Open questions

Blocking the codegen slice (R1-dependent):
- **Q1 (R1 native build):** engine-uniform injection in *native* binaries needs codegen, which needs R1 (`BUILD_DIAGNOSIS.md`). Until then the guarantee is interpreter-complete + a `#[ignore]`d parity tripwire. The honest state: I-13 is *fully* real on the interpreter, *specified-and-guarded* for codegen. Stated plainly.

Blocking the agent slice (overlaps R3/Phase-7):
- **Q2 (`@[agent]` action model):** "every tool call logged" presumes a tool-call mechanism. Today there is no first-class `Tool`/`Agent` runtime (Phase 7, F12 userland `agent.ax` is a state machine, not language-level). So agent-action injection is specified against the *future* tool surface; the interpreter slice that ships now is `@[experiment]` distinctness + the no-opt-out guarantee for adaptive/experiment. Agent-action logging lands with Phase-7 tools. *Blocks the agent acceptance row, not the rest.*

Non-blocking:
- **Q3 (Zone in the type system):** reflecting zone as an effect so a Static fn calling an Agent fn is a checked effect — Phase-6 effect-row work (`R6`/effects spec), not R4. Deferred.
- **Q4 (I-13 text update):** when this implements, update I-13 to drop its "Partially true today" caveat and cite this spec. Noted.

## Review note (2026-06-01)

Status: **Reviewed** — no blocking issues. All 12 SPEC_TEMPLATE sections substantively filled; decisive fork (I-13 engine-uniform provenance) resolved with decision + rationale + rejected alternative. E15xx band is unused across all crates; codes are consistent with the E10xx–E16xx banding (I-14). All cited source symbols verified present in the codebase: `DEFERRED_ATTRS` (builtins.rs:1175), `"experiment"` at line 1178, `has_adaptive_attr` (codegen/mod.rs:72), `provenance` store (interp.rs:187), `append_provenance_jsonl` (interp.rs:4663).

**Line numbers fixed (stale, codebase shifted ~240 lines):**
- `interp.rs:665` → `interp.rs:701` (adaptive attribute check in `call_fn`)
- `interp.rs:672` → `interp.rs:701` (same injection point, different citation)

No changes to the spec's substance were needed beyond the line-number corrections. No new issues were found that would block Reviewed status.
- **Q5 (provenance log unification):** R3 (`ai_call`), R4 (zone records), R10 (verification records) all append to one NDJSON stream with an `event` discriminator — a unified `AuditEvent` schema should be factored once, not three times. Cross-spec cleanup, noted for whichever lands second.
