# TEL — whole-task economic evidence

**Owners:** CX-10 evidence, CX-13 resources, CX-21 evaluation, CX-32 evidence index, CX-34 scheduling. **Tasks:** B243/B244/B245. **Supporting reference:** RES-CURSOR.

The source already has AI token usage and benchmark-level input/output/cache/cost fields plus candidate-ID join checks. v0.21 unifies those into runtime evidence; it does not claim telemetry must be built from nothing. The authoritative integer principal budget in `axon-core::kernel` remains the spend authority. A richer observational record must not silently replace it or double-charge it.

## Usage record

Bind task, episode, attempt, candidate/branch and unique call ID, parent call where applicable, principal, provider/model/runtime revision, request and response identity, price revision/currency, usage completeness, latency and outcome. Use stable IDs for joins; never join concurrent candidate results by list position.

Separate mutually exclusive uncached input, cache-read input, cache-write input and generated output, documenting the provider's actual accounting. If an upstream total input field includes cached tokens, derive disjoint categories once and retain the original record; do not add both total and cached as separate charges. Record embeddings, GPU/CPU allocation, verifier/critic/router calls, retry/fallback and cancelled branches. Different units use declared conversion and rounding; do not mix dollars, micro-units and provider token rates.

Missing usage is `UNKNOWN` or `PARTIAL`, not zero. Carry a reserved ceiling until reconciled. Known zero is allowed only when the source explicitly supports it. An unpriced operation cannot be declared free because `output_tokens=0`. Price revisions are immutable evidence; no current market prices are hardcoded into the release criteria.

## Metric denominator

Report every assigned task: success, failure, abstention, cancellation and unresolved outcome. Pair tasks across arms before comparing cost and quality. “Cost per accepted successful task” is useful only alongside assignment count, failure cost and coverage; otherwise a router can appear cheaper by refusing hard tasks or excluding failed calls.

Capture per-task total cost, success/quality, wall time and p50/p95 across an appropriate task set, with cold/warm cache and queue/resource conditions. The scope includes all orchestration turns and changed retries, not just one shortened prompt. A comparison must preserve required outcome quality within a preregistered margin and include uncertainty. Do not select a savings claim from a few warm cached states and call it overall production savings.

## Reservation and replay

Existing resource authority reserves the maximum allowed cascade cost, then reconciles actual complete usage once by call identity. An unknown or cancelled call retains its liability until the authority receives a terminal accounting event. Cross-principal calls cannot borrow another tenant's cache/spend identity. Replay consumes recorded costs and results without spending again.

Required falsifiers include duplicated call IDs, mixed task/principal ledgers, negative or nonintegral counters, overlapping input categories, missing price identity, provider totals inconsistent with subcounts, cancelled-but-billed calls, absent usage and success-only reports. Model-free fixtures cover a strict simplified micro-unit tariff; real provider billing reconciliation requires separate adapters and evidence.
