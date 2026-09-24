# RTR — eligible model and role selection

**Owners:** CX-06 and CX-34; grants stay CX-03. **Tasks:** B237/B238/B248; optional B253. **References:** RES-ROUTING.

TinyRouter/TRINITY explore selecting model/role combinations with a compact learned selector. Semantic Router separates routing signals, decisions and model algorithms. These are candidate implementations under existing Axon routing ownership, not three mandatory sequential services and not authority providers.

## Two-stage decision

First, deterministic eligibility resolves capability grants, model/data policy, provider availability, host/runtime compatibility, requested schema support, budget and required state continuity. Filter before sending private prompts, candidates or retrieval content to any remote classifier. Second, an optional learned policy ranks the remaining candidates. Empty eligibility returns a typed block; no “best available” fallback may weaken privacy or authorization.

The source `cortex-policy-adapter` enforces a supplied grant snapshot. Its positive response is not a learned model route, blanket authentication or execution evidence. Keep that adapter deterministic and separate from learned rankings.

## Route record

Bind task/attempt, principal, session, eligible-set digest, registry epoch, router policy and feature projection, selected model/provider/role/reasoning effort, runtime revision, fallback plan, remaining budget/deadline and state-transfer method. Record cold-start, queueing and cache loss from switching. KV caches are not generally interchangeable across models; do not assume text/model changes preserve them. A private session can move only through an explicitly authorized handoff.

The first provider is an existing deterministic incumbent. A learned adapter is disabled/shadow until qualified for its exact domain and pool. New model discovery creates a quarantined registry proposal, not auto-activation or implicit access to private tasks. Revocations are checked at dispatch even if an earlier score was high.

## Bound self-reference

A decision router must not recursively route its own routing decision. Set a maximum selector depth, model switches, retries and total fallback cost. A selector transport error uses the declared eligible incumbent or refuses. No fallback impersonates another principal or omits required verification.

## Comparison design

Pin source/model/router revisions and split tasks before fitting. Compare with applicable static, cheapest-eligible, rules and best-single controls using equivalent whole-task budgets and independent outcomes. Count selector inference, feature encoding, cache misses, retries, verifier calls and provider queueing. Report results by task family and stateful versus fresh sessions, including failures and OOD cases.

A trained router improving one domain does not establish a general win. CLM retrieval and TinyRouter model-role selection may coexist or compete; select composition only by controlled evidence. ANN-scale action retrieval and specialized router training are optional, not prerequisites for deterministic routing safety.
