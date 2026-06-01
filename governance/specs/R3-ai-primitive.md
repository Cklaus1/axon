# Tech Spec — R3: AI as a Language Primitive

**Status:** ✅ Reviewed (2026-06-01)
**Requirement:** `../REQUIREMENTS.md` R3 — *AI as language primitive: `std.ai`, model routing, `#[ai(policy)]`, deterministic fallback.*
**Decisive fork (from `README.md`):** *Reproducibility vs. capability.* Does `#[ai(policy)]` + model routing record enough to **replay** a run deterministically? **→ Resolved below: YES. The provenance schema is settled first; routing is built on top of it.**

---

## 1. Motivation

Today AI is reachable from Axon only through a fixed family of builtins — `ai_complete`, `ai_extract_uncertain_i64`, `ai_extract_uncertain_f64` (`interp.rs` ~4478–4535; `crates/axon-ai/src/lib.rs`) — each hard-wired to a single model string (`"claude-sonnet-4-6"`, appears 6× in `crates/axon-ai`). There is:

- **no model routing** — the program cannot say "use a cheap model for this, a strong one for that";
- **no `#[ai(policy)]`** — a call cannot be gated by a declared policy (max cost, allowed models, required fallback);
- **no mandatory deterministic fallback** — offline, the only "fallback" is `AXON_AI_MOCK=1`, a global env toggle that replaces *all* calls with a canned string (`interp.rs:4617`, `ai_mock_enabled()`), not a per-call typed default;
- **no replay** — an AI call records nothing about *which* model/version/prompt produced a value, so `axon trace` (and the PRD's "auditable" claim) cannot reproduce or even explain an AI-derived result.

The last point is the fork. The PRD promises AI **reproducibility**; an AI primitive that doesn't pin its inputs makes every downstream "audit"/"replay" claim hollow. So this spec resolves the **provenance schema for AI calls first**, then defines routing and policy as layers that *populate* that schema. Routing without the schema would have to be retrofitted — expensive once merged.

This spec is **design-only**. Per `README.md` / `BUILD_PROTOCOL.md` Gate 1, no implementation code lands until this reaches **Reviewed**. It also deliberately scopes *what can be settled now* vs *what depends on Phase 7 (`LLM<Capabilities>`, `Budget`) and Phase 9 (replay engine)* — see §12.

---

## 2. Requirement link

`../REQUIREMENTS.md` **R3** (currently 40%, ⚠️ Partial). Quoted acceptance:

> *Policy-gated call honored; fallback fires offline; routing picks tier by cost.*

This spec adds a fourth, fork-driven criterion the requirement's "Gap" column implies but doesn't state:

> *Every AI call pins (model, version, params-hash, prompt-hash, seed-if-any, response-hash) into a typed provenance record, sufficient for `axon trace` to attribute the value and for a future replay engine (R9/F2) to memoize it.*

Overlaps **R4** (provenance must be un-opt-out-able) and **R9/F2** (replay). This spec owns the **AI-call schema**; R4 owns *injection enforcement*; R9 owns the *replay executor*. Boundaries stated in §7.

---

## 3. Surface (what the user writes)

### 3.1 Policy attribute

```axon
// A policy names the cost ceiling, the allowed model tier, and the MANDATORY
// fallback that fires when no allowed model is reachable (offline, over-budget,
// or policy-denied). The fallback is a pure Axon expression — never an AI call.
@[ai(policy(tier: cheap, max_cost_usd: 0.01, fallback: "unknown"))]
fn classify(text: str) -> str {
    ai_complete("Classify the sentiment of: {text}")?
}
```

### 3.2 Routing — explicit tier on the call

```axon
// Tier is a closed enum: cheap | balanced | strong. The router maps tier →
// concrete (model, version) via a host-side table (§4.4), not a user string,
// so a program is portable across model generations.
let summary = ai_complete(prompt, tier: balanced)?     // tier is an optional named arg
let label   = ai_extract_uncertain_i64(prompt, tier: cheap)?
```

### 3.3 Deterministic fallback (the common *error* case)

```axon
// Offline / over-budget / no allowed model → the policy fallback is returned as
// a normal Ok value (NOT an Err, NOT a panic), so the program stays total.
// `ai_complete` without an enclosing @[ai(policy)] and with no reachable model
// is an ERROR (E1300) — a fallback must be declared to run offline.
@[ai(policy(fallback: "neutral"))]
fn label(text: str) -> str { ai_complete("...") ?? "" }   // ?? = fallback-or
```

### 3.4 CLI surface

```
axon run prog.ax                         # uses live models if ANTHROPIC_API_KEY + --features asi-runtime
AXON_AI_MOCK=1 axon run prog.ax          # existing global stub (kept; §11)
axon ai policy prog.ax                   # NEW: print the resolved policy per @[ai]-annotated fn (JSON)
axon trace --ai <run>                    # NEW: show AI-call provenance rows (model, hashes, cost)
```

The `tier:` named argument and `@[ai(policy(...))]` attribute are **new parser surface**; `??` (fallback-or) is proposed in §12 as optional sugar, not load-bearing.

---

## 4. Semantics (what it does)

### 4.1 Behavior table — a single `ai_complete(prompt, tier)` call

| Input class | Behavior |
|---|---|
| Live, API key present, model reachable, within budget | Call the routed `(model, version)`; return `Ok(reply)`; **append one `AiCall` provenance record** (§4.3). |
| `AXON_AI_MOCK=1` | Return the deterministic stub (unchanged from today); provenance record stamped `mode: "mock"`. |
| Offline / no API key, policy declares a fallback | Return `Ok(fallback)`; provenance record stamped `mode: "fallback"`, `reason: "<why>"`. |
| Offline / no API key, **no** policy fallback | `Err`/panic path — see §6 E1300. A program that wants to run offline MUST declare a fallback. |
| Over `max_cost_usd` for the policy | Do **not** call; behave as the offline-with-fallback row (`mode: "fallback"`, `reason: "budget"`). If no fallback → E1301. |
| `tier:` names a tier with no configured model | E1302 at check time (closed enum, host table validated at startup). |
| Policy forbids the routed model | Treated as "no allowed model" → fallback or E1300. |

### 4.2 Tier → model resolution

`tier` is a **closed enum** `{cheap, balanced, strong}`. A host-side table (config, not user code) maps each tier to a concrete `(model, version)`. Resolution order:

1. Per-call `tier:` arg, if present.
2. Else the enclosing `@[ai(policy(tier: …))]`.
3. Else the default tier (`balanced`).

The user never writes a raw model string — that is what makes a program reproducible across model generations *and* lets the host pin the exact version into provenance.

### 4.3 The `AiCall` provenance record (the fork resolution — settle this FIRST)

Every AI call (live, mock, or fallback) appends **one** NDJSON record to the existing provenance log (same file/format as `append_provenance_jsonl`, `interp.rs:4663`), with `event: "ai_call"` and this payload schema (v1):

| Field | Meaning | Why it's load-bearing |
|---|---|---|
| `ts_ms` | wall-clock (existing field) | ordering |
| `fn` | calling Axon fn name (existing) | attribution |
| `src` | program path (existing, BUG_HUNT #4) | multi-program separation |
| `event` | `"ai_call"` | distinguishes from `@[adaptive]` score rows |
| `tier` | resolved tier enum | routing audit |
| `model` | concrete model id | replay pins the exact model |
| `model_version` | provider version string | replay across model updates |
| `params_hash` | hash of (max_tokens, temperature, tool schema, …) | replay pins call params |
| `prompt_hash` | SHA-256 of the exact prompt sent | replay memo key; avoids logging PII verbatim |
| `mode` | `live` \| `mock` \| `fallback` | honest provenance: a fallback is NOT a model output |
| `reason` | fallback/budget reason, else `""` | explains non-live modes |
| `cost_usd` | metered cost (0 for mock/fallback) | budget audit (F4) |
| `response_hash` | SHA-256 of the response (live only) | replay verification |

**Decision:** prompt and response are stored as **hashes, not plaintext**, in the default log (privacy + size); a future opt-in `--ai-record-verbatim` (R9) may store full text for replay. The hash is sufficient as a replay **memo key** (F2): the replay engine maps `prompt_hash → recorded response`. This is the minimum that makes replay *possible* without committing to a storage policy now (that's R9's call, §12).

### 4.4 Determinism

A live model call is **not** deterministic and this spec does not pretend otherwise (honesty rule). Determinism is delivered in two honest ways: (a) `mock` mode is fully deterministic; (b) replay mode (R9) re-serves recorded responses keyed by `prompt_hash`. The `seed` field is reserved but **not** claimed to force provider determinism (providers don't guarantee it); it records any seed we *did* send. See §12.

---

## 5. Type rules

- `@[ai(policy(...))]` is an **attribute on a fn**, parsed like the existing deferred attrs (`builtins.rs` `DEFERRED_ATTRS`); it does not change the fn's type. The checker validates the policy literal's shape (tier ∈ enum, costs ≥ 0, fallback type-compatible with the fn's AI-call return).
- `tier:` is a **named argument** on the `ai_*` builtins, of a new closed enum type `AiTier`. `parse_type_str` / `builtin_sigs` learn `AiTier`. It composes with nothing generic — it is a leaf enum.
- The **fallback expression's type must unify with the `ai_*` call's success type** (`str` for `ai_complete`, `i64`/`f64` for the extract family). This is a new constraint emitted at the policy site (mirrors how `constrain` is used in `infer.rs`). Mismatch → E1303.
- Return types of the `ai_*` builtins are **unchanged** (`Result<_, str>` / `Result<Uncertain<_>, str>`), preserving all existing call sites.

---

## 6. Error codes

| Code | Trigger | Message shape |
|---|---|---|
| **E1300** | An `ai_*` call is unreachable (offline / no key / model denied) and **no** fallback is in scope | `` `ai_complete` cannot run: no model reachable and no @[ai(policy(fallback: …))] in scope — declare a fallback to run offline `` |
| **E1301** | Call would exceed `max_cost_usd` and no fallback declared | `` `ai_*` call denied by budget (cap ${cap}); declare a fallback or raise the cap `` |
| **E1302** | `tier:` resolves to a tier with no host-configured model | `` unknown AI tier `{tier}` — configured tiers: cheap, balanced, strong `` |
| **E1303** | Policy `fallback` type ≠ the call's success type | `` @[ai] fallback has type `{found}`, but the call returns `{expected}` `` |
| **W1310** | Live AI call made by a fn with **no** `@[ai(policy)]` (allowed, but un-metered/un-pinned) | `` AI call in `{fn}` has no @[ai(policy)] — cost is unmetered and the call is harder to audit `` |

Codes occupy a new `E13xx` / `W13xx` band (AI-primitive), invented here per I-14, not improvised in code.

---

## 7. Invariants touched

- **I-2 (interpreter is reference):** all semantics defined for the interpreter first; codegen AI-call provenance is explicitly deferred (parity gap to be logged, like #33/#36). **Preserved** (interp leads).
- **I-8/I-9 (success signal / no silent degenerate):** a `fallback` returning a canned value is a **success-signal risk** — it could masquerade as a real model answer. Mitigated by `mode: "fallback"` in provenance + `W1310`; a fallback is never silently indistinguishable from a live answer in the audit trail. **Preserved.**
- **I-10 (determinism is available):** §4.4 commits to determinism via mock + replay, not by claiming live calls are deterministic. The `seed` field records what we sent but does not claim to force reproducibility. **Preserved.**
- **I-13 (un-opt-out-able provenance):** this spec *extends* the I-13 invariant from `@[adaptive]`/`@[goal]`/`@[agent]` to AI calls — every AI call appends one NDJSON row. The record is appended by the interpreter at the call site, not by user cooperation. R4 (injection enforcement) handles the codegen side. **Preserved.**
- **I-11 (capability boundary):** `@[ai(policy)]` works within the existing `@[contained]` capability boundary (`capabilities.rs` lines 59–65 already classify `ai_*` calls as `Net`). The policy adds a cost/timeout gate but does not widen the I/O path. **Preserved.**
- **I-14 (stable error codes):** new E13xx band, defined here. **Preserved.**
- **I-2 / R4 overlap (un-opt-out-able provenance):** this spec *defines* the AI record; R4 *enforces* that it cannot be suppressed. This spec must not provide an opt-out flag for the record itself (only for verbatim text). **Preserved.**
- **New invariant proposed (I-15 candidate):** *Every AI call appears in provenance exactly once, including mock and fallback.* Stated here for adoption when implemented.

---

## 8. Test plan (maps 1:1 to §4)

Red test that must fail first: **`ai_call_appends_provenance_record`** — assert that a single mocked `ai_complete` writes exactly one `event:"ai_call"` NDJSON row with the §4.3 fields. Fails today (no such record is written).

- [ ] **Unit:** tier resolution order (call > policy > default); policy-literal validation; `params_hash`/`prompt_hash` stability for identical inputs.
- [ ] **Integration:** `AXON_AI_MOCK=1` run emits `mode:"mock"` rows; offline-with-fallback emits `mode:"fallback"` + reason; over-budget emits `reason:"budget"`.
- [ ] **CLI e2e:** `axon ai policy prog.ax` prints resolved policy JSON; `axon trace --ai` lists the rows; exit codes per `ARCHITECTURE_INVARIANTS` I-8.
- [ ] **Adversarial:** offline + no fallback → E1300 (not a silent canned value); cost cap = 0 → E1301/fallback; `tier: bogus` → E1302; fallback type mismatch → E1303.
- [ ] **Property:** for any prompt `p`, two mock calls with identical `(prompt, tier, params)` produce identical `prompt_hash`/`params_hash` (memo-key stability — the replay precondition).
- [ ] **Parity (interp↔codegen):** deferred — codegen AI-call provenance is a tracked gap (§11); the parity test is written but `#[ignore]`d with the finding number until R1's build lands.
- [ ] **Journey/red-team:** a fallback value must be visibly distinguishable from a live answer in `axon trace --ai` (no silent masquerade — I-9).

## 9. Acceptance criteria (the done gate)

R3 may move toward DONE on this slice when **all** pass:

- [ ] `ai_call_appends_provenance_record` passes (the schema exists and is written).
- [ ] `tier_resolution_prefers_call_over_policy_over_default` passes.
- [ ] `offline_without_fallback_errors_E1300` passes (no silent canned value).
- [ ] `offline_with_fallback_returns_fallback_mode` passes (total program offline).
- [ ] `over_budget_call_uses_fallback_or_E1301` passes.
- [ ] `mock_call_prompt_hash_is_stable` passes (replay memo-key precondition).
- [ ] `axon ai policy` emits stable JSON (schema versioned).

**Note on `goal_clear` vs the JSONL file:** the existing `goal_clear` builtin clears only the in-memory provenance store (`interp.rs:3775`, `self.provenance` / `self.provenance_inputs`). It does **not** delete or truncate the `provenance.jsonl` file on disk. If implementation follows existing `goal_clear` behavior, the AI `event:"ai_call"` NDJSON rows in the file are append-only and never cleared by `goal_clear`. This is an append-only log by design — the in-memory clear handles `goal_run` isolation; the file serves as an audit trail that is intentionally append-only. If the JSONL file must also be cleared (e.g. for disk-growth management or compliance), a separate `axon provenance trim` CLI command should be built, not entangled with `goal_clear`.

(Routing-by-*cost* — "router picks the cheapest allowed model that satisfies the tier" — is acceptance for the **R3-routing** follow-slice, which depends on the Phase-7 `Budget`/`LLM<Caps>` cost model; see §12. This spec delivers tier→model + the schema; cost-optimal routing layers on after Phase 7.)

## 10. Performance budget

N/A for correctness. One note: `prompt_hash`/`response_hash` are SHA-256 over strings already crossing a network boundary — negligible vs the API round-trip. The provenance append is one line, same cost as the existing `@[adaptive]` log.

## 11. Rollout & rollback

- **Feature-flagged:** live calls stay behind `--features asi-runtime` (unchanged). The **schema + policy + tier parsing + fallback** land *without* that feature (they work under `AXON_AI_MOCK=1` and offline), so the bulk is testable in the default `--no-default-features` build.
- **Decomposed for revertibility:** (1) `AiCall` record + write path; (2) `@[ai(policy)]` parse + check; (3) `tier:` arg + routing table; (4) fallback semantics + E13xx. Each is an independently revertible commit; `git revert` of any leaves a green tree.
- **Blast radius:** a wrong fallback could mask a model outage as success — bounded by the mandatory `mode:"fallback"` provenance + `W1310`, so it's always auditable. No existing call site changes type, so back-compat is total.
- **Codegen:** AI-call provenance in the native path is deferred to a tracked parity finding (interp is reference, I-2), consistent with the codegen-build constraint.

## 12. Open questions

Blocking (must resolve before building the dependent slice):
- **Q1 (verbatim storage — R9):** does replay need full prompt/response text, or is `prompt_hash → response` memoization enough? This spec assumes hash-keyed memo; if R9 needs verbatim, the schema gains an opt-in `--ai-record-verbatim` and a storage/PII policy. *Blocks the replay slice, not this one.*
- **Q2 (cost model — Phase 7):** `max_cost_usd` and cost-optimal routing need a real per-model price table and the `Budget<R…>` meter (F4). Until Phase 7, `max_cost_usd` is enforced only as a **pre-call estimate** (tokens × static price), and "routing picks tier by cost" is **not** claimed DONE. *Blocks the routing-by-cost acceptance row.*

Non-blocking:
- **Q3 (seed):** providers don't guarantee seeded determinism; the `seed` field records what we sent but we do not claim it forces reproducibility. Revisit if a provider ships deterministic sampling.
- **Q4 (`??` fallback-or sugar):** the §3.3 `??` operator is ergonomic, not required — the policy `fallback` already covers it. Defer to a language-sugar tick.
- **Q5 (I-15 adoption):** "every AI call in provenance exactly once" — propose for the invariants file when this implements.

---

## Review note (2026-06-01)

Reviewed against SPEC_TEMPLATE.md 12-section structure, CODE_REVIEW_RUBRIC.md, and ARCHITECTURE_INVARIANTS.md. Verified all claims against the codebase. **Fixes applied:**

1. **Stale line numbers corrected.** `interp.rs` line references were off by ~240 lines due to subsequent commits. Fixed: AI builtins `~4478-4535` (was ~4231-4290), `ai_mock_enabled()` at 4617 (was 4234), `append_provenance_jsonl` at 4663 (was 4416). Also corrected `axon-ai` crate path from `axon-ai/src/lib.rs` to `crates/axon-ai/src/lib.rs` (its workspace location) and `claude-sonnet-4-6` count from 5x to 6x.

2. **Missing invariant citations added.** §7 was missing I-10 (determinism), I-13 (un-opt-out-able provenance), and I-11 (capability boundary). All three are directly relevant: the spec's §4.4 is about determinism, the entire spec is about extending provenance, and `@[ai(policy)]` works within the existing `@[contained]` network gate. Added citations to §7.

3. **`goal_clear` append-only note added.** The existing `goal_clear` builtin clears only in-memory state, not the `provenance.jsonl` file on disk. This means AI call records in the NDJSON file are append-only — a design choice (audit trail), not a gap. Added a clarifying note in §9 (Acceptance criteria) stating the intent so implementation doesn't silently change this.

4. **Error codes verified clean.** E1300-E1303 / W1310 occupy a completely unused band (E12xx and above are reserved by other specs but not yet assigned in code). No conflicts found.

5. **Design claim verified.** `axon-ai/src/lib.rs` exists and contains `claude-sonnet-4-6` hardcoded 6 times across its 3 call sites. `DEFERRED_ATTRS` in `builtins.rs` contains `"ai"` (deferred attr). `append_provenance_jsonl` and `read_provenance` both exist at the (corrected) line numbers cited.

**No substantive gaps remain.** The spec is ready for implementation.
