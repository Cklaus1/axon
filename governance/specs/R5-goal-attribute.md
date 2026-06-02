# Tech Spec — R5: `#[goal]` Attribute Sugar (train → holdout → gate)

**Status:** ✅ Reviewed (2026-06-02)
**Requirement:** `../REQUIREMENTS.md` R5 — *Goal-directed optimization: `#[goal]` type, strategies, deterministic test sets, eval hierarchy.*
**Decisive fork:** *What does annotating a function `#[goal(...)]` MEAN at runtime — what does calling that function do, and how does it wire the already-shipped primitives (`goal_run` optimizer + `goal_eval` held-out evaluator) into one train→holdout→gate loop without the user hand-writing it?* The primitives exist; the sugar that composes them does not. **→ Resolved below.**

---

## 1. Motivation

R5 (70%) ships four optimizer strategies (`goal_run`/`_continue`/`_random`/`_multistart`) and the eval-hierarchy primitive `goal_eval(metric, input)` (held-out evaluation: snapshot the provenance, run the metric on a held-out point, restore — so the held-out score never pollutes the training probes). What's missing per REQUIREMENTS.md is *"first-class `#[goal(...)]` attribute that auto-wires the train/holdout/gate loop (the primitive exists; the sugar doesn't)."*

Today a user must hand-write: `goal_run(metric, target, budget)` then `goal_eval(metric, holdout)` then compare to target. That is the canonical ML loop (train on a budget, evaluate on held-out data, gate on a threshold) and it should be a single annotation — and, crucially, it should be *un-hand-wire-able-wrong*: the holdout eval must not pollute training (it doesn't — `goal_eval` already snapshots/restores), and the gate must be honest (compare the held-out score, not the training best).

## 2. Requirement link

`../REQUIREMENTS.md` **R5** (70%). Quoted gap: *"first-class `Goal` value/type + a `#[goal(metric, test_set, target)]` attribute that auto-wires the train/holdout/gate loop."* This slice delivers the **attribute sugar** over the existing primitives. (A first-class `Goal` *value type* — passable, composable — is a larger Phase-7 item, out of scope here; this is the attribute, which is what the acceptance line names.)

## 3. Surface (what the user writes)

```axon
@[adaptive]
fn quality(x: i64) -> i64 { 100 - (x - 7) * (x - 7) }   // peak at x=7 → 100

// A goal fn: optimize `quality` toward 100 for up to 40 evals, then gate on a
// HELD-OUT point. The body runs after the loop; `goal_met` is in scope (1 if the
// held-out score cleared target, else 0). The fn returns whatever its body does.
@[goal(metric: quality, target: 100, max_evals: 40, holdout: 7)]
fn tune() -> i64 {
    goal_met        // 1 if the held-out eval at x=7 met target 100, else 0
}

fn main() -> i64 { tune() }      // → 1 (held-out x=7 scores 100, meets target)
```

Attribute args (flat `key: value`, the existing attr grammar — no list literals needed for v1):
- `metric` — the name of an `@[adaptive]` fn to optimize (required).
- `target` — the score threshold to reach/clear (required, numeric).
- `max_evals` — optimization budget (optional, default 50).
- `holdout` — a single held-out i64 input the gate evaluates on (optional; if omitted the gate uses `goal_best_input` — the best *training* input, a weaker gate, and a W1510-style note that no holdout was given).

## 4. Semantics

### 4.1 Desugaring (the fork resolution)

A `#[goal(metric: M, target: T, max_evals: E, holdout: H)]` fn, **when called**, runs this before its body:

1. **Train:** `goal_run(M, T, E)` — optimize M toward T for up to E evals (the existing hill-climb; provenance accumulates).
2. **Holdout:** `s = goal_eval(M, H)` — evaluate M on the held-out input H **without** recording it as a training probe (the existing snapshot/restore). If no `holdout`, `s = goal_best_score(M)` (training best — weaker, noted).
3. **Gate:** `goal_met = if reached(s, T) { 1 } else { 0 }`, where `reached` is `s >= T` for a maximization target (the optimizer treats the target as a value to reach; `>=` is the gate). `goal_met` (an `i64`) is injected as a binding visible to the fn body.
4. **Body:** the fn body runs with `goal_met` in scope and returns normally.

The whole loop is keyed on the annotation — a user cannot call a `#[goal]` fn and *skip* training, and the holdout eval is structurally non-polluting (it inherits `goal_eval`'s snapshot/restore). That is the "auto-wired, can't-hand-wire-wrong" property.

### 4.2 Validation (checker)

- `metric` must name a defined fn that is `@[adaptive]` → else **E1500** (`#[goal] metric` `{M}` is not an `@[adaptive]` fn).
- `target` must be numeric → else **E1501**.
- `max_evals`/`holdout`, if present, must be integers → else **E1501**.
- A `#[goal]` fn that references `goal_met` is the normal case; referencing it from a non-goal fn is just an undefined name (existing E0001), no special handling.

### 4.3 Determinism

Under `AXON_SEED`, the optimizer is deterministic (existing), and `goal_eval` is a pure call on the metric → the whole loop is reproducible. Two runs of a `#[goal]` fn with the same seed produce the same `goal_met`.

### 4.4 Behavior table

| Case | Behavior |
|---|---|
| `#[goal]` fn called, holdout meets target | trains, holdout eval ≥ target, `goal_met = 1`, body runs |
| holdout below target | `goal_met = 0`, body runs (the gate is reported, not enforced — the body decides) |
| `metric` not `@[adaptive]` | **E1500** at check time |
| `target`/`max_evals`/`holdout` non-numeric | **E1501** at check time |
| no `holdout` given | gate uses training best (`goal_best_score`); still produces `goal_met` |

## 5. Type rules

`#[goal]` does not change the fn's type. `goal_met` is an injected `i64` binding in the fn body's scope. The fn's params: a `#[goal]` fn takes **no params** in v1 (it is an entry point that runs a loop); a `#[goal]` fn with params is **E1504** (reserved).

**Implementation note (call_fn ordering):** the existing arg-count check in `call_fn` fires before any desugaring; since a v1 `#[goal]` fn is nullary, a normal `tune()` call passes 0 args = 0 params and the check is satisfied. The E1504 "no params" rule is enforced at **check time** (`checker.rs`), not in `call_fn` — so the runtime never sees a param-carrying `#[goal]` fn. The desugaring hook in `call_fn` runs **after** param binding and **before** body eval, injecting `goal_met` via `env.define("goal_met", Value::Int(...))` (same mechanism as param binding). The loop runs on **every** call (no memoization). `goal_eval(metric, holdout)` passes the single i64 `holdout` as the metric's input — v1 targets single-i64-arg `@[adaptive]` metrics; a multi-arg metric uses only its first dim (noted, not an error).

Return type is whatever the body yields. Validation (E1500/E1503/E1504) lives in `checker.rs` (a check-time concern); the interpreter only runs the desugaring.

## 6. Error codes

New **E15xx** band (goal sugar — note R4 also reserved E15xx for code-zones conformance; this spec uses E1500-E1502, R4 used E1501 for codegen conformance — **deconflict: R5 uses E1500/E1503/E1504**, see below to avoid collision).

| Code | Trigger | Message |
|---|---|---|
| **E1500** | `#[goal(metric: M)]` where M is not an `@[adaptive]` fn | `` #[goal] metric `{M}` must be an @[adaptive] fn `` |
| **E1503** | `#[goal]` numeric arg (target/max_evals/holdout) is not an integer/number | `` #[goal] `{key}` must be a number, got `{val}` `` |
| **E1504** | `#[goal]` fn declares parameters (reserved — goal fns are nullary in v1) | `` #[goal] fn `{name}` must take no parameters `` |

*(Deconflicted from R4's E1501/E1502 which are codegen-conformance + experiment-label. R5 takes E1500/E1503/E1504.)*

## 7. Invariants touched

- **I-2 (interpreter is reference):** the desugaring is interpreter-side; codegen parity is deferred (like R4). **Preserved.**
- **I-8/I-9 (success signal):** the gate compares the **held-out** score, never the training best (unless no holdout, which is noted) — so `goal_met` cannot be inflated by overfitting. This is the honest-signal core. **Preserved+.**
- No invariant changes — sugar over shipped primitives.

## 8. Test plan

Red test that must fail first: **`goal_attribute_trains_and_gates_on_holdout`** — a `#[goal(metric: quality, target: 100, holdout: 7)]` fn returns `goal_met`; assert it is 1 (the held-out x=7 scores 100). Fails today (no `#[goal]` desugaring, `goal_met` is an undefined name).

- [ ] **Unit/interp:** the loop runs `goal_run` then `goal_eval`; `goal_met` reflects the held-out gate; under `AXON_SEED` the result is deterministic.
- [ ] **CLI e2e:** a `#[goal]` example runs and exits with the gate verdict.
- [ ] **Validation:** `metric` not `@[adaptive]` → E1500; non-numeric arg → E1503; `#[goal]` fn with params → E1504.
- [ ] **Honesty:** a metric that overfits the training probes but scores poorly on the holdout produces `goal_met = 0` (the gate is held-out, not training).

## 9. Acceptance criteria

R5 rises toward DONE when **all** pass:
- [ ] `goal_attribute_trains_and_gates_on_holdout` (the train→holdout→gate loop wired).
- [ ] `goal_attribute_gate_is_zero_when_holdout_misses_target`.
- [ ] `goal_metric_must_be_adaptive_e1500`.
- [ ] determinism under a fixed seed.

R5 may rise 70% → ~85% on this slice (the attribute sugar; the first-class `Goal` *value type* remains Phase-7).

## 10. Scope / non-goals

- **In:** `#[goal(metric, target, max_evals, holdout)]` desugaring (interp), `goal_met` injection, E1500/E1503/E1504 validation, tests, an example.
- **Out:** a first-class `Goal` value type (Phase-7); list-literal `test_set: [...]` in attrs (the attr grammar is flat key:value — `holdout` is a single point in v1); codegen parity; per-strategy selection in the attr (uses `goal_run`'s default hill-climb).
