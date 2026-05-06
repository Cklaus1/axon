# Mitosis Audit — Axon repo (2026-05-06)

Forcing-function output: what the proposed `mitosis` tool would produce
if pointed at the Axon repo today.  Five-judge pipeline; verdict from
the orchestrator at the end.

---

## Cartographer (quantitative)

| Subsystem | Path | LoC | Inbound deps | Outbound deps | Self-contained tests? |
|---|---|---|---|---|---|
| `axon-ai` | `crates/axon-ai/` | ~950 | linked-into compiled binaries via C ABI; called by codegen builtins.rs | `reqwest`, `serde_json` | none (relies on integration tests via examples) |
| `axon-rt` provenance | `crates/axon-rt/src/provenance.rs` | 330 | called from the Axon runtime + `axon trace replay` | `serde_json`, `std` | yes (in-file tests) |
| `axon-rt` goal-loop | `crates/axon-rt/src/goal.rs` | 448 | called from `goal_run` builtin | calls into `axon-rt::adaptive_registry` | yes |
| `examples/asi/llm_proxy.py` | Python | ~? | invoked via `ANTHROPIC_BASE_URL` env-var; transparent to compiled binaries | stdlib only | none |
| `examples/asi/{bench,watch,analyze}` | Bash + Python | ~? | invoked from `run.sh`; consume provenance log | stdlib | none |
| Codegen IR shim | `crates/axon-core/src/codegen/{ir.rs,ir_inkwell.rs}` | ~1,330 | called from migrated codegen modules | `inkwell` | unit tests in `ir_inkwell.rs` |

**Coupling summary**: `axon-ai` and `provenance` are weakly coupled to
core (cross C-ABI boundary).  `llm_proxy.py` is *fully decoupled*
(env-var injection point).  `bench/watch/analyze` are *fully
decoupled* (read on-disk JSONL).  IR shim is heavily entangled with
codegen (every codegen module imports from it).

---

## Cohesion judge (qualitative)

For each candidate: does this represent ONE coherent abstraction?

| Candidate | Coherent abstraction? | Verdict |
|---|---|---|
| `axon-ai` | Yes — "Rust client for Anthropic's API with typed-extraction helpers" | ✅ ready-named library |
| `provenance.rs` | Yes — "structured-event log writer + replay reader" | ✅ generic event-store |
| `goal.rs` (live hill-climb) | Partial — tightly coupled to Axon's `@[adaptive]` ABI | ❌ would need API redesign to be reusable |
| `llm_proxy.py` | Yes — "transparent HTTP proxy that records LLM cost/latency/tokens to JSONL" | ✅ standalone observability tool |
| `bench/watch/analyze` | Yes — "score-trajectory analysis suite for AI optimization runs" | ✅ ready-named tool |
| IR shim | Partial — coherent design but tightly bound to inkwell + axon's specific codegen | ❌ research artifact, not a library |

---

## API + comms designer

For ✅ candidates only.

### `axon-ai` → `claude-rs` (or `anthropic-rs`)

- **API surface** (proposed):
  ```rust
  pub struct Client { /* api_key, base_url, http */ }
  pub fn complete(&self, prompt: &str) -> Result<String, Error>
  pub fn complete_typed::<T>(&self, prompt: &str) -> Result<T, Error>
      // T: I64, F64, Bool, Uncertain<i64>, Uncertain<f64>
  ```
- **Comms**: linked Rust library (no IPC).
- **Stability**: 0.x — the typed-extraction shape may want to absorb
  "tool use" / "structured output" features in time.
- **Existing alternatives**: `anthropic-sdk-rs` (community), official
  Anthropic SDKs.  Differentiator is the typed-extraction helpers
  with confidence return; that's novel.

### `provenance.rs` → `evlog`

- **API surface**:
  ```rust
  pub struct Logger { path: PathBuf }
  pub fn record(&self, event: &Event) -> Result<()>
  pub fn replay(path: &Path, filter: F) -> impl Iterator<Item = Event>
      where F: Fn(&Event) -> bool
  ```
- **Comms**: linked library + on-disk JSONL format that other tools
  can read (so non-Rust clients work).
- **Stability**: file format pinned at v1; library API 0.x.

### `llm_proxy.py` → `llm-proxy`

- **API surface**: HTTP — `ANTHROPIC_BASE_URL=http://localhost:N` ⇒
  proxy logs every call to `~/.cache/llm-proxy/calls.jsonl` then
  forwards.  CLI: `llm-proxy serve --port N --upstream URL`.
- **Comms**: HTTP reverse-proxy.  No language coupling.
- **Stability**: pin the on-disk format; the rest is transparent.

### `bench/watch/analyze` → `score-trace`

- **API surface**: CLI tools that consume any JSONL with `(timestamp,
  score)` records.  Decoupled from Axon's specific provenance format.
- **Comms**: filesystem (read-only).
- **Stability**: tools 0.x; input format documented but generous.

---

## Red-team

For each ✅ candidate: what breaks?  What's the cost?  Probability the
parent rebuilds in 6 months?

| Candidate | Risk | Probability of regret | Verdict |
|---|---|---|---|
| `axon-ai` → `claude-rs` | Cost: keep two repos in sync if axon needs new typed-extraction shapes. Risk: community competitors (`anthropic-sdk-rs`) may capture the niche first | LOW (decoupled enough; adding to upstream is straightforward) | **PROCEED — but depend on it from axon, don't fork** |
| `provenance.rs` → `evlog` | Cost: format-version drift between axon-rt and evlog readers | LOW | **PROCEED** |
| `llm_proxy.py` → `llm-proxy` | Cost: ~0 (it's already standalone). Risk: someone else ships a better OSS one (Helicone, LangSmith). Spin-out signals "we've stopped investing" | MEDIUM (commodity space) | **PROCEED only if you're committed to maintaining as a public tool. Otherwise leave in-repo as `tools/llm-proxy/`** |
| `bench/watch/analyze` → `score-trace` | Cost: ~0. Risk: too generic, overlaps with W&B / TensorBoard / sacred — those have years of head start | HIGH (commodity space) | **DO NOT SPIN OUT — keep in-repo as `tools/score-trace/`. Document the JSONL contract so external authors can use it without depending on us** |

### Special case: IR shim

- Verdict: ❌ not an extraction candidate (research artifact) but ✅
  needs **branch hygiene**.  Move `crates/axon-core/src/codegen/{ir.rs,
  ir_inkwell.rs,MIGRATION.md,IR_REARCH.md}` + the migration commits to
  a `research/ir-shim-2026-05` branch off `main`.  Keep the
  empirical findings (bounded RSS but unchanged time) as a documented
  null result.  This is what an `mitosis branch-hygiene` advisor would
  flag.

---

## Migration planner — one example, top-ranked candidate

### Spinning `provenance.rs` → `evlog`

**Step 1** — extract files (clean cut)
- New repo `evlog/` (or `tools/evlog/` in a workspace if you want a
  monorepo for now).
- Copy `crates/axon-rt/src/provenance.rs` + its tests verbatim.
- Cargo.toml: rename crate `evlog`, version 0.1.0, no deps beyond
  `serde_json` + `std`.

**Step 2** — neutralize Axon-specific names in the public API
- `Record` → `Event` (more generic).
- `provenance_records_for(name)` → `events_filter(by_name: &str)` or
  similar.  Generic over filter predicate.
- Document the JSONL field set as v1 stable.

**Step 3** — wire axon-rt to depend on evlog
- `crates/axon-rt/Cargo.toml`: add `evlog = { path = "../../evlog" }`
  (or version-pinned once published).
- `crates/axon-rt/src/provenance.rs` shrinks to a thin wrapper that
  imports evlog::{Logger, Event} and re-exports under axon-rt's old
  names for source-compat.
- Run axon-rt tests; should be green.

**Step 4** — rollback marker
- After 1 release cycle of evlog being green: delete the wrapper in
  axon-rt, callers import from evlog directly.
- If anything breaks: revert the dependency change and re-inline.

**Total LoC moved**: ~330.  **Estimated effort**: 1 day.  **Net win**:
provenance becomes reusable by other AI-loop projects without
depending on axon-rt; axon-rt tracks an upstream that's pre-stabilized.

---

## Orchestrator's verdict

**Three concrete actions for the Axon repo**, ranked by ROI:

1. **Spin out `provenance.rs` → `evlog`** (1 day, low risk, high
   reuse value).  The cleanest extraction in the audit.

2. **Branch-hygiene the IR shim research**: move the shim work to
   `research/ir-shim-2026-05` off main.  Keep `merge-asi-layer3`
   reserved for the genuine ASI layer-3 merge.  Document findings as
   a null result.  This is not a spin-out per se but is what
   Mitosis's "branch hygiene advisor" should call out.

3. **Spin out `axon-ai` → `claude-rs`** (2-3 days, low risk).
   Adds a public artifact that other Rust AI projects could adopt.
   Watch the niche — if `anthropic-sdk-rs` gets traction, defer
   and depend on it instead.

**Do NOT spin out**: `bench/watch/analyze` (commodity space, in-repo
is the right place), `llm_proxy.py` (same), `goal.rs` (tightly bound
to `@[adaptive]` ABI; would need API redesign first).

---

## Self-test for the Mitosis tool itself

If you build `mitosis` as its own repo, the test of the tool's own
quality is: run it on Axon and compare output to this hand-audited
file.  An agent-driven version that produces ≥80% overlap with the
verdicts above is shipping-quality.  Anything less is noise.

Empirical predictions Mitosis-the-tool should produce:
- Identifies `provenance.rs` as #1 spinout candidate ✓
- Flags IR shim as branch-hygiene, not extraction ✓
- Rejects `bench/watch/analyze` as commodity-space ✓
- Recommends `axon-ai` with the "watch the niche first" caveat ✓
- Does NOT recommend `goal.rs` extraction without API redesign ✓
