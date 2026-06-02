# Tech Spec — R3b: per-call `tier:` named argument

**Status:** ✅ Reviewed (2026-06-02)
**Requirement:** `../REQUIREMENTS.md` R3 — *AI as primitive; model routing.* Completes R3 §4.2 resolution **step 1** (per-call tier), which `R3-ai-primitive.md` deferred for lack of named-arg grammar.
**Decisive fork:** *How is `ai_complete(prompt, tier: cheap)` parsed and resolved — without a sweeping general named-argument grammar change that ripples through infer/checker/codegen/interp?* **→ Resolved: a scoped, additive `tier` field on `Expr::Call`, defaulting to `None`, so every existing consumer is untouched.**

---

## 1. Motivation

R3 tier routing (`8f6615a`) resolves the AI tier from the enclosing `@[ai(policy(tier:))]` (steps 2-3 of §4.2), defaulting to `balanced`. The missing **step 1** is the per-*call* override: `ai_complete(prompt, tier: cheap)`. The R3 spec deferred it because Axon has **no named-argument call grammar** — and adding a general one (turning `Call.args` into `Vec<(Option<name>, Expr)>`) would touch every consumer of a call's args (parser, infer, checker, codegen, interp). That is disproportionate for one keyword.

## 2. The fork resolution: scoped, additive

**Decision: add a single optional `tier: Option<String>` field to `Expr::Call`.** The parser recognizes a *trailing* `tier: <ident>` argument and routes it to that field instead of the positional `args` vec; everything else is unchanged. Because the field defaults to `None`, every existing `Expr::Call { callee, args }` construction and match either gets `tier: None` or `..` — no behavioral change anywhere. The interpreter reads `call.tier` for `ai_*` calls; all other calls ignore it.

This is *not* a general named-arg facility (only `tier:` is recognized, only as a trailing arg) — it is the smallest change that delivers the acceptance, explicitly scoped so it can later generalize without rework.

## 3. Surface

```axon
@[ai(policy(tier: balanced, fallback: "x"))]
fn summarize(text: str) -> str {
    // per-call override: this ONE call uses cheap, the policy default is balanced
    let quick = ai_complete("tl;dr: {text}", tier: cheap)?
    let full  = ai_complete("full summary: {text}")?   // uses policy tier (balanced)
    full
}
```

Resolution order (now complete, R3 §4.2): **per-call `tier:`** > enclosing `@[ai(policy(tier:))]` > default `balanced`. An unknown per-call tier name → E1302 (same as the policy path).

## 4. Semantics

### 4.1 Parsing

In `parse_args`, when an argument is the shape `IDENT : ATOM` where `IDENT == "tier"` (a peek: ident followed by `:`), consume it as the call's `tier` rather than a positional arg. It must be **trailing** (after positional args); a `tier:` arg followed by more positionals is a parse error (keeps it simple, matches the surface). Non-`tier` `ident:` is left alone (parses as today — currently nothing else uses it, so it would be a normal expression / error, unchanged).

### 4.2 Resolution (interp)

`ai_complete`'s tier resolution gains step 1: if `call.tier` is `Some(name)`, parse it via `ai_routing::Tier::parse` (unknown → E1302) and use it; else fall through to the existing policy/default resolution (`current_ai_tier`). The resolved tier picks the concrete model, stamped into the AiCall provenance exactly as today — so a per-call `cheap` records `tier:"cheap"` + the cheap model.

### 4.3 Determinism / invariants

No invariant change. The provenance record is still one-per-call with the *resolved* tier; per-call resolution is a pure function of the call node. I-2 preserved (interp owns resolution).

## 5. Type rules

`tier:` does not affect the call's type — it's metadata on the `ai_*` builtin call, stripped before arg-type checking. The positional args (the prompt) type exactly as today.

## 6. Error codes

None new — an unknown per-call tier reuses **E1302** (unknown AI tier), already defined.

## 7. Test plan

Red test that must fail first: **`per_call_tier_overrides_policy`** — a fn with `@[ai(policy(tier: balanced))]` makes two `ai_complete` calls, one with `tier: cheap` and one without; assert the provenance records show `cheap`/haiku for the first and `balanced`/sonnet for the second. Fails today (`tier:` doesn't parse as a call arg).

- [ ] **Parse:** `ai_complete(p, tier: cheap)` parses with `tier=Some("cheap")`, `args=[p]`.
- [ ] **Resolution:** per-call tier beats the policy tier (the override); absent → policy/default.
- [ ] **Unknown:** `tier: turbo` → E1302.
- [ ] **Back-compat:** every existing call (no `tier:`) is unaffected — the full suite passes (the `tier: None` default).

## 8. Acceptance criteria

- [ ] `per_call_tier_overrides_policy` passes.
- [ ] `unknown_per_call_tier_is_e1302` passes.
- [ ] the full suite is green (additive field, no consumer change).

R3 may rise 68% → ~75% — this completes the tier-routing resolution order; the budget gate (E1301, Phase-7 `Budget`) remains.

## 9. Scope / non-goals

- **In:** `Expr::Call.tier: Option<String>`; trailing-`tier:` parsing; interp step-1 resolution; tests.
- **Out:** a general named-argument grammar (this is `tier:`-only, by design); named args on user fns; `tier:` on the `ai_extract_*` family (same mechanism, can follow; v1 does `ai_complete`).
