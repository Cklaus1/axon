# Tech Spec — R3c: AI call-budget meter (`@[ai(policy(budget: N))]` → E1301)

**Spec ID:** `R3c-ai-budget-meter` (advances `REQUIREMENTS.md` R3; first concrete slice of the Phase-7 `Budget` Tier-1 type)
**Status:** Shipped (2026-06-02)
**Risk class:** Standard
**Author / date:** autonomous build, 2026-06-02

---

### 1. Motivation

Today an `@[ai(policy)]` fn can call `ai_complete` an unbounded number of times — the cost is invisible (ROADMAP F4: *"No budget meter — token cost per `ai_complete` call is invisible; `max_evals` is a poor proxy"*). An adaptive/agent loop that fans out AI calls can run away with no language-level ceiling. This slice adds the **first concrete `Budget`**: a per-fn AI-call budget declared in source (`@[ai(policy(budget: N))]`); the interpreter meters every `ai_complete` the fn makes and **halts the (N+1)th call with E1301**. The user-visible win: a fn can declare *"I may make at most N model calls"* and the runtime enforces it — the cost meter the TCB (`cost_meter`, Phase 7) is built from, in its smallest honest form.

### 2. Requirement link

`REQUIREMENTS.md` **R3** — its residual: *"budget gate (E1301, Phase-7 Budget)."* Also ROADMAP **F4** and the Phase-7 `Budget<R…>` Tier-1 type (§6 type table). This slice delivers the *call-count* budget (deterministic, offline-capable) — not per-token cost, which needs a live model's token accounting (deferred, §12 Q1).

### 3. Surface (what the user writes)

```axon
// A fn that may make AT MOST 2 AI calls. The 3rd is E1301 (budget exhausted).
@[ai(policy(tier: cheap, budget: 2))]
fn summarize_each(items: [str]) -> str {
    let a = match ai_complete(items[0]) { Ok(s) => s  Err(_) => "" }   // call 1/2 — ok
    let b = match ai_complete(items[1]) { Ok(s) => s  Err(_) => "" }   // call 2/2 — ok
    let c = match ai_complete(items[2]) { Ok(s) => s  Err(_) => "" }   // call 3 — E1301, halts
    c
}
```

- `budget: N` is a non-negative integer field of the `@[ai(policy(...))]` group, parsed like `tier:`/`fallback:` (the parser flattens the group, so the interp reads the attr arg `"budget: N"`).
- A fn with **no** `budget:` is **unmetered** (today's behavior — back-compat; the budget is opt-in).
- The budget is **per-fn-activation**: it counts the `ai_complete` calls made *while that fn is the current fn*, reset each time the fn is entered (so calling a budgeted fn in a loop gives each call its own budget — the budget is a property of the activation, not a global).

### 4. Semantics (what it does)

| Input class | Behavior |
|---|---|
| `@[ai(policy(budget: 2))]`, fn makes 2 `ai_complete` calls | Both proceed; meter ticks 1, 2. Clean. |
| Same fn makes a 3rd `ai_complete` | **E1301** — the 3rd call halts with `Flow::Panic(E1301)` before any model dispatch (mock/live/fallback). The first two are unaffected. |
| `@[ai(policy)]` with no `budget:` | Unmetered — unbounded calls, today's behavior (back-compat). |
| `budget: 0` | The *first* `ai_complete` is E1301 (zero calls allowed). A fn that declares `budget: 0` may make no AI calls. |
| `budget:` on a fn that makes no AI calls | No effect — the meter is never consulted. |
| Negative or non-integer `budget:` value | Parse-time/encounter-time: treated as **absent** (unmetered) with a `W1311` warning — a malformed budget must not silently *enforce* a wrong number nor crash. |
| Nested: budgeted fn A calls budgeted fn B | Each fn meters **its own** `ai_complete` calls (the meter keys on the current fn). A's budget does not cover B's calls and vice-versa. |
| Mock (`AXON_AI_MOCK=1`) | The meter ticks identically — E1301 fires on the (N+1)th mock call too (the budget is about call *count*, deterministic regardless of mode). |

The meter is checked **before** the W1310 unmetered-warning and before model dispatch, so an over-budget call produces E1301, not a stray mock/live response or a W1310.

### 5. Type rules

N/A. `budget:` is an attribute field, not a type. No inference change. (The first-class `Budget<R…>` *value type* — a budget you can pass, split, and thread — is the larger Phase-7 work this slice is the runtime-meter precursor to; §12 Q2.)

### 6. Error codes

| Code | Trigger | Message shape |
|---|---|---|
| **E1301** | An `ai_complete` call would exceed the enclosing fn's `@[ai(policy(budget: N))]` | `` `{fn}` exceeded its AI budget of {N} call(s) — raise the budget or reduce ai_complete calls `` |
| **W1311** | A `budget:` field whose value is not a non-negative integer | `` @[ai(policy(budget: …))] on `{fn}` is not a non-negative integer — ignored (fn runs unmetered) `` |

E1301 was reserved in the R3 spec; W1311 is new in the E13xx AI band (per I-14).

### 7. Invariants touched

- **I-2 (interpreter is reference):** the meter lives in the interpreter `ai_complete` dispatch — the reference semantics. Codegen's `ai_complete` does not meter yet (parity gap, noted §12 Q3 + `#[ignore]`d parity intent); deferred because native AI calls are not the tested path. **Preserved (interp); codegen parity deferred.**
- **I-8/I-9 (success signal):** E1301 is a `Flow::Panic` → non-zero exit, on stderr — a real failure, not a silent cap. **Preserved.**
- **I-14 (stable codes):** E1301 (reserved) realized; W1311 added. **Preserved.**
- No invariant changed.

### 8. Test plan

Red test that must fail first: **`ai_budget_halts_third_call_e1301`** — a fn `@[ai(policy(budget: 2))]` that makes 3 `ai_complete` calls under `AXON_AI_MOCK=1`; assert exit ≠ 0 and stderr contains `E1301`, and that the first two calls' provenance records were written (the budget halts the 3rd, not the run from the start). Fails today: there is no budget field, so all 3 calls proceed.

- [ ] Unit: budget parse — `"budget: 2"` → `Some(2)`; `"budget: -1"`/`"budget: x"` → `None` + W1311 path.
- [ ] Integration (interp): a budgeted fn making ≤N calls is clean; the (N+1)th is E1301; `budget: 0` blocks the first call.
- [ ] CLI e2e (observable: exit code + E1301 on stderr): the §3 example over-budget → exit 2, E1301.
- [ ] Adversarial: nested budgeted fns each meter independently; an unmetered fn (no budget) still unbounded; malformed budget → W1311, unmetered (not a crash, not a wrong enforcement).
- [ ] Property: for any N≥0 and any call count C, the fn makes exactly `min(C, N)` successful `ai_complete` calls before E1301 (or all C if unmetered).
- [ ] Parity (interp↔codegen): deferred (`#[ignore]`) — codegen `ai_complete` metering is Phase-7 ABI work.

### 9. Acceptance criteria (the done gate)

R3 advances when **all** pass — **ALL DONE 2026-06-02**:
- [x] `ai_budget_halts_third_call_e1301` passes (the headline — the (N+1)th call halts; the first two still execute, proven by 2 ai_call provenance records).
- [x] `ai_budget_zero_blocks_first_call` passes (`budget: 0` boundary).
- [x] `ai_budget_absent_is_unmetered` passes (back-compat — no budget → unbounded; 5 calls clean).
- [x] `ai_budget_malformed_warns_w1311_and_runs_unmetered` passes (malformed → ignored, not enforced wrong).
- [x] Per-activation reset verified (a budgeted fn called twice gets a fresh budget each time — `AiBudgetGuard` saves/restores the count).

### 10. Performance budget

N/A. The meter is one `RefCell<usize>` increment + compare per `ai_complete` call — negligible against a model call. Non-AI code pays nothing (the meter is only touched in the `ai_complete` builtin).

### 11. Rollout & rollback

Small, revertible. Additive: a new `current_ai_budget()` reader + a per-activation counter threaded like `current_call_tier`. A `git revert` leaves a green tree (no schema or type change; the attr field is ignored by everything else). Blast radius: only fns that declare `budget:` change behavior; every existing program is unmetered exactly as before.

### 12. Open questions

- **Q1 (per-token cost):** real `Budget<usd>` needs a live model's token/cost accounting; this slice meters *call count* (deterministic, offline). Per-token cost lands with the live `LLM<Caps>` gateway (Phase 7). Noted, non-blocking.
- **Q2 (first-class `Budget` value):** a `Budget` you can construct, split across sub-agents, and thread as a value is the full Tier-1 type. This slice is the *runtime meter* it sits on. Non-blocking.
- **Q3 (codegen parity):** native `ai_complete` does not meter; an interp↔codegen parity test is deferred (Phase-7 ABI). Stated, not hidden.
