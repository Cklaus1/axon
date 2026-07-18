# R38 — Axon Embedded: the safe execution substrate for AI-generated automations inside other products

**Spec ID:** `R38-embedded-agent-runtime` (new requirement row; strategic/platform bet. Ties to ROADMAP §2.1
"the language is an IR, not a human-authored surface", §0 Containment pillar, `examples/flagship/` — the
sandbox value wedge — and R8 machine-stable diagnostics / R15 `host_await` / R28 audit ledger)
**Status:** Draft — strategic proposal; Slice 0 is a kill-gated spike, nothing past it opens without the
founder's wedge decision (§12 Q1)
**Risk class:** Structural (a product direction and a public embedding contract, not a language feature)
**Author / date:** strategic-research agent, 2026-07-18

```spec-meta
id: R38-embedded-agent-runtime
status-claim: Draft
depends-on: R8-diagnostic-schema, R15-resume-runtime, R28-capability-audit-ledger, R3c-ai-budget-meter, R7b-axonhost, R6-capability-security
blocks: none
blocked-by: R38 §12 Q1 (founder wedge decision — gates everything past the kill-gated Slice 0); R38 Slice-0 generation-viability kill threshold (§9)
supersedes: none
related: R18-provenance-ledger, R22-intent-approve-gateway, R27-corrigibility-resource-bounds, R16-axon-ui, R36-full-asi-os, R37-nano-micro-asi-kernel
conflicts-with: none
reserves: one embed-registry E-block, number TBD at implementation (must avoid E2300–E2302 [eBPF] and E1810 [TEE], per §6)
evidence: none (Draft; Slice-0 gates are embed_ceiling_refuses_undeclared_capability_via_api + embed_generation_viability_measured per §9)
```

(Edge notes: `depends-on` = the landed machinery §3 packages — `@[contained]`/E1001 (R6 lineage +
ASI 3.6), the `AxonHost` seam + `host_await` tool boundary (R7b/R15), the hash-chained audit
ledger (R28), the budget meter (R3c), and `axon-diag/1` (R8, the repair-loop contract). `related`
holds R18 (the directly-comparable competing wedge — R38's audit chains are R18's ingestion-free
event source if both proceed) and R22-intent-approve-gateway (whose `Grant` model is the same
I-11 authority-moves-outward direction §7 names).)

> **One-line thesis:** every product company is adding "describe it and the AI builds it" automation
> features, and every one of them hits the exact problem the flagship demo names — *how do you run
> model-written code inside your product without trusting it?* Axon Embedded packages the **already-landed**
> interpreter + capability checker + budget meter + replay + audit chain as an **embeddable runtime SDK**
> (Rust crate / C ABI / wasm module) that a host product drops in. The host registers typed tools with
> declared effects; the host's own LLM emits `.ax` against that tool surface; the compiler refuses any
> undeclared capability **before the code runs once**; execution is metered, replayable, audit-chained, and
> kill-switchable. Nobody at the host company — and none of their end users — ever writes Axon by hand.
> **This is ROADMAP §2.1 taken seriously as a distribution strategy:** if the language is an IR for machine
> generation, the embedded SDK is the channel through which that IR reaches users who will never adopt a
> language.

---

### 1. Motivation

The flagship demo (`examples/flagship/`) proved the guarantee: a real prompt-injected LLM wrote three
exfiltration channels and the compiler refused all three before execution; ~28/40 CVE-Bench critical CVEs
are a bug class Axon prevents by construction. But the flagship's implied buyer — a team that *adopts Axon
as their language* — is the hardest possible sale. Language adoption is a decade-scale bet and the graveyard
is full of better languages.

Meanwhile a buyer with the same problem exists **today, in volume, with budget**: product teams shipping
"AI builds your automation / workflow / report / integration" features inside an existing SaaS or internal
platform. Their current options are all bad:

| Today's answer | What it actually gives you |
|---|---|
| V8 isolates / wasm sandbox (Figma-plugins model, Cloudflare Workers) | **Memory + syscall** isolation. Zero app-level capability granularity: any API the host exposes to the sandbox is fully reachable by any generated program. No per-automation least-privilege, no budget semantics, no deterministic replay, no typed audit. |
| JSON/DSL no-code workflow graphs (Zapier-class) | Safe, but an expressiveness ceiling the "AI writes it" wave immediately exceeds — models want to write *code* (the CodeAct result: code-as-action beats JSON tool-calls on capability, but running that code is the unsolved half). |
| "The model's code is reviewed" (human or LLM judge) | Trust, not proof. Doesn't scale past demo volume; the flagship shows a model under injection defeats it. |
| Starlark / Lua embedding | Mature, but their safety answer to effects is **"none"** (Starlark is deterministic *because it can't do I/O*). The interesting automations are effectful. There is no typed capability grant, no budget, no audit chain, no replay of effectful tool calls. |

Axon's landed machinery is precisely the missing middle: **effectful but contained**. R38 asks whether to
productize it as the embeddable substrate — "the runtime your product uses to run what its AI writes" —
rather than (or before) selling the language itself.

**The user-visible win in two sentences:** a host product can let its AI generate arbitrary effectful
automations for end users, show each user a *provable* capability card ("this automation may: read your CRM,
send ≤ 5 emails via api.host.com; it may not: touch files, spawn processes, call any other host"), and have
the compiler — not a policy hope — make the card true. When something goes wrong, the host replays the exact
run deterministically and shows the hash-chained audit trail.

### 2. Requirement link

Opens a new `REQUIREMENTS.md` row **R38** (platform-vision / product bucket, alongside R16/R17/R18). No
language-semantics change. It advances ROADMAP §0 **Containment** (the pillar productized) and §2.1 (the
two-track thesis: typed `.ax` as machine-generated IR; the approval-of-resolved-capabilities step is the
§2.4 "approve the AST" discipline transplanted into someone else's product). Acceptance (full vision): *a
third-party product, whose team never writes Axon, ships an AI-automation feature where every generated
automation is capability-refused/metered/replayed/audited by the embedded Axon runtime, and its end users
approve legible capability cards.* v1 acceptance is Slice 0 + Slice 1 (§9).

> **Hard truths up front (the graveyard framing, per R18's discipline):**
> 1. **This is a two-sided ask.** The host must embed a new runtime **and** their model must emit valid
>    Axon. The second half is the existential risk: if frontier models cannot produce checking-clean Axon
>    for realistic automation tasks at a high rate (with a structured-diagnostic repair loop), the wedge is
>    dead regardless of how good the sandbox is. Slice 0 measures this with a pre-registered kill threshold
>    (§9) — it is *the* hypothesis, not an implementation detail.
> 2. **"Isolates are good enough" is often true.** For hosts whose exposed tool surface is already coarse
>    and low-stakes, wasm + API-key scoping genuinely suffices. The buyer with real pain runs
>    *consequential* tools (payments, record mutation, outbound comms) against *untrusted-tenant* AI code.
>    The ICP must be qualified on that pain, not on enthusiasm for the demo.
> 3. **An embedding contract is a ball and chain.** Embedding customers demand a frozen surface, semver,
>    migration guarantees. Axon is pre-1.0 and the ASI-research track churns it weekly. Shipping an
>    "Embedded profile" freeze constrains the research language or forks it — a real, permanent tax (§12 Q3).
> 4. **The stdlib is thin for real automations.** HTTP/JSON/dates/regex ergonomics are partial. That gap is
>    honest build work no landed feature covers.
> 5. **Focus.** This is another platform front next to the language, R16 (UI), R17 (bare metal), R18
>    (ledger), and the three directions under parallel research (ASI UI, full ASI OS, nano-kernel). The
>    scarce resource is focus; R38 exists to be *compared*, and §12 Q1 gates everything past Slice 0.

### 3. What is genuinely landed vs. derived vs. greenfield (honest reuse accounting)

The R18 post-review correction taught the lesson: count reuse against *code*, not concepts. Verified against
the tree at spec time:

| Embedded-runtime need | Shipped? | Reality |
|---|---|---|
| Sub-second, no-LLVM, embeddable interpreter | ✅ shipped | `interp.rs`; `cargo build -p axon-core --no-default-features` ≈ sub-second; 90/90 builtin coverage; already compiled as a `wasm32-unknown-unknown` cdylib with a bare C ABI (`crates/axon-wasm` — `axon_alloc`/`axon_eval`/output read-back, zero JS glue) and at 28/28 interp parity on `wasm32-wasip1` |
| Compile-time capability refusal against a host-declared ceiling | ✅ shipped | `@[contained]` E1001/E1004: transitive (no helper laundering), path-traversal-safe, env-var-denied, fail-closed on dynamic targets; Phase 6 effect rows + E1310 anti-laundering as the successor surface |
| Host-mediated tool calls (the embedding seam) | ✅ shipped | the `AxonHost` seam (every host touchpoint routes through a thread-local host trait; `DefaultHost` = std) + R15 `host_await`/`host_await_val` (a program genuinely suspends, the host answers, it resumes — on native, wasip1, and the browser via Asyncify). **This is exactly the tool-call boundary an embedded runtime needs, already built.** |
| Budget / metering | ✅ shipped | E1301 per-fn AI-call budgets; kernel per-token cost meter (`ai_cost_spent`, real `cost_usd` in provenance); kernel `Goal` budget exhaustion (exit 7); `AXON_MAX_DEPTH` recursion ceiling |
| Deterministic replay | ✅ shipped | `AXON_SEED` + `AXON_AI_REPLAY` (memoized `(prompt,model)` → byte-identical re-run); run-ids + `axon trace --replay` |
| Tamper-evident audit | ✅ shipped | R28 hash-chained capability ledger (FS/Net/AI/Exec/Random/IO), `axon-os audit verify`; mandatory I-13 provenance |
| Kill-switch | ✅ shipped | R27 one-way latch, exit 4, < 1 s; `@[corrigible]` |
| Structured diagnostics for an LLM repair loop | ✅ shipped | R8 `axon-diag/1`: versioned JSON with code/line/col/`expected`/`found`/`help` — purpose-built for machine consumption; `axon intent compile` already runs an LLM→check loop |
| **A stable embedding API** (`Runtime::builder()`, typed tool registration, per-run ceilings as *data* not source attributes) | ❌ **greenfield** | today the ceiling lives in `@[contained]` source attributes and env vars; an embedder needs to impose it *from outside* the program, per run, via API. This is the core Slice-0 build: a thin crate (`axon-embed`) wrapping interp + checker, injecting the ceiling and tool schema |
| **Host-tool typing** (declare a tool's arg/return schema + effect row; generated code type-checks against it) | ⚠️ partial | `host_await_val` is typed `T -> U` at the call site, but there is no *registry* of named host tools with schemas the checker validates calls against — derived work on shipped pieces |
| Embedded-profile language freeze + conformance suite + semver | ❌ greenfield | and politically expensive (§12 Q3) |
| Automation-grade stdlib (HTTP/JSON/date/regex ergonomics) | ⚠️ partial | dict/str landed; JSON/date/regex thin |
| Generation viability (models emit valid Axon reliably) | ❌ **unvalidated** | the load-bearing hypothesis; Slice 0 measures it |

**Net:** unlike R18 (where the "ledger" turned out to be mostly greenfield), the enforcement core here — the
thing the product *sells* — is shipped and red-teamed. The greenfield is packaging (embed API, profile
freeze, stdlib polish) plus one falsifiable hypothesis (generation viability). That is a materially better
reuse ratio than R16/R17/R18 at their spec time.

### 4. Surface (what the embedder writes — not what end users see)

Rust-level sketch (C ABI and wasm exports mirror it; contract frozen at Slice 2, not before):

```rust
let rt = axon_embed::Runtime::builder()
    // Typed tools: name, arg/return schema, effect row, host handler.
    .tool("crm.lookup",  schema!((str) -> str), effects![Net("api.internal")], |req| host.crm(req))
    .tool("email.send",  schema!((str, str) -> bool), effects![Net("mail.internal")], |req| host.mail(req))
    // The per-run ceiling, imposed from OUTSIDE the program (the host, not the code, holds authority):
    .ceiling(caps! { fs: none, net: ["api.internal", "mail.internal"], exec: none })
    .budget(Budget { tool_calls: 50, ai_usd_micro: 50_000, wall_ms: 2_000, max_depth: 500 })
    .build()?;

// 1. Check: capability + type refusal BEFORE first run. Diagnostics are axon-diag/1 JSON —
//    feed them straight back to the generating model for repair.
let program = rt.check(generated_ax_source)?;          // Err(diags) = the repair-loop input

// 2. The legible capability card (what the end user approves — §2.4 transplanted):
let card = program.capability_card();                   // tools used, hosts, effect rows, budget

// 3. Run: every tool call surfaces as a host_await suspension the host answers.
let run = rt.run(&program, input)?;                     // output + audit chain + replay handle

// 4. Later: deterministic replay + tamper-evident audit for support/compliance.
let rerun = rt.replay(run.handle())?;                    // byte-identical
assert!(run.audit_chain().verify());
```

The error case is the product: `rt.check` on a generated program that calls `read_file`, an unregistered
tool, an unlisted host, or exceeds the ceiling returns E1001-class diagnostics — the automation is refused
before it exists as a runnable thing, and the diagnostic is machine-readable enough for the model to repair
or for the UI to explain.

**Server-side generation loop** (the other half of the SDK): `axon-embed` also exposes the
prose→skeleton→generate→check→repair loop that `axon intent compile` + `axon-diag/1` already implement, so a
host can go from end-user prose to a checking-clean, ceiling-conformant program without building the loop
themselves.

### 5. Who the buyer is (and who it is not)

- **Primary ICP:** product/platform teams adding AI-authored automations over **consequential tools**
  (CRM/support/fintech/ops SaaS; internal-tools platforms; agent-infrastructure vendors adopting
  code-as-action). Qualifier: they run *untrusted-tenant* generated code against tools that mutate records,
  move money, or send communications — the population for whom "isolate + API key" is visibly not enough.
- **Secondary:** agent-framework vendors wanting a safe tool-composition language (the CodeAct execution
  half) they can ship without building a sandbox story themselves.
- **Not the buyer:** teams happy with JSON tool-calling; teams whose sandbox needs are coarse; anyone who
  must run *existing* Python/JS agent code unchanged (that is the sidecar idea, rejected in §13a — it
  forfeits the compile-time guarantee that is the whole moat).

Relationship to the flagship: same guarantee, different distribution. The flagship sells the guarantee to
Axon *authors*; R38 sells it to hosts whose *models* are the authors — which is the only author population
that scales, and the one ROADMAP §2.1 already declared canonical.

### 6. Error codes

No new language diagnostics. The embed API surfaces existing codes (E1001/E1004 capability, E1310 effects,
E1301 budget, exit 4/6/7/8 runtime classes) through `axon-diag/1`. Slice 0 allocates one new block for
embed-registry errors (unknown tool name, tool-schema mismatch, ceiling-file malformed) from the next free
E-range at implementation time (E2300–E2302 and E1810 are reserved by other tracks; do not collide).

### 7. Invariants touched

- **I-11 (capability boundary)** — *elevated*: the boundary becomes host-imposed data (the ceiling), not
  only source attributes. The refusal semantics do not change; the authority source moves outward. This is
  the same direction R22-intent-approve's `Grant` model already points.
- **I-13 (provenance not opt-out-able)** — preserved and productized (the audit chain is a sellable
  artifact, as in R18, but generated by *our* runtime rather than ingested).
- **I-10 (determinism available)** — load-bearing for the replay feature.
- **I-2 (interp/codegen parity)** — untouched; the embedded runtime is interp-only by design (codegen's
  E0910 refusals of interp-only builtins are irrelevant inside the SDK).
- No invariant is weakened; no TCB component changes.

### 8. Test plan

- [ ] Unit: ceiling-as-data equivalence — a program refused under source `@[contained(...)]` is refused
      identically under the API-imposed ceiling (same codes), and vice versa.
- [ ] Integration: tool registry — a generated program calling a registered tool within the ceiling runs;
      an unregistered tool / unlisted host / schema-mismatched call is refused at `check`.
- [ ] Adversarial (the flagship transplanted): the prompt-injected-LLM corpus
      (`agent_task_llm_generated.ax` + fresh live generations) run through `rt.check` — all exfil channels
      refused; add a red-team pass specifically against the *embed* seam (host handler confusion, ceiling
      TOCTOU between check and run, tool-name spoofing).
- [ ] Property: `run` → `replay` byte-identical (seed + AI-replay cache honored through the embed API);
      audit chain verifies; a tampered entry fails.
- [ ] Journey (Slice 1): prose → generate → repair loop → capability card → approve → run → audit view,
      end-to-end in the reference host.
- [ ] **The hypothesis test (Slice 0, pre-registered):** generation-viability harness per §9.

### 9. Acceptance criteria (per slice, kill-gated)

**Slice 0 — spike (two halves, both must pass; either can kill):**
- [ ] `embed_ceiling_refuses_undeclared_capability_via_api` — a minimal `axon-embed` crate imposes a
      ceiling + tool registry from outside the source and refuses the flagship exfil corpus (no source
      attributes needed).
- [ ] `embed_generation_viability_measured` — N ≥ 30 realistic automation tasks × a frontier model emitting
      Axon against a documented tool schema, with ≤ 3 `axon-diag/1` repair rounds. **Pre-registered kill
      threshold: < 80% of tasks reaching checking-clean + behaviorally-correct kills the wedge** (record the
      number either way; do not move the goalposts after seeing it). Compare against the same tasks in
      Python-in-isolate as the honesty baseline.
- [ ] A green Slice 0 authorizes the §12 Q1 comparison — **not** Slices 1+.

**Slice 1 — reference host (the demo that sells):** a small automation-builder web app (reuse `axon-web`
patterns): end-user prose → generated program → **capability card approval pane** → metered run → audit
trail + one-click deterministic replay. Acceptance: `embed_reference_host_journey` e2e; the card provably
matches enforcement (a card/enforcement divergence is a red-team finding, not a docs bug).

**Slice 2 — the contract:** Embedded-profile language subset spec + conformance suite + semver policy; wasm
(browser + wasip1) and C-ABI packaging at parity with the Rust crate.

**Slice 3 — design partners:** 1–2 real external hosts integrate; their friction list becomes the next spec
(same protocol as ROADMAP §9.5).

### 10. Performance budget

Slice 0: `rt.check` p95 < 150 ms for a 200-line generated program (it is inside an end-user interactive
loop); `rt.run` overhead vs bare `axon run` < 10%. Interpreter throughput is otherwise already accepted
(native AOT is out of scope for embedded v1).

### 11. Rollout & rollback

Ships as a separate crate (`crates/axon-embed`) + the wasm/C packaging; the language, CLI, and existing
products are untouched. Slice 0 is a spike branch; killing it deletes one crate and one harness. The
research-track language keeps evolving; only the (Slice-2) frozen profile carries compatibility duty — so
rollback before Slice 2 has near-zero blast radius, and *that* is why the freeze decision is sequenced last.

### 12. Open questions

1. **(THE gate — founder)** Is R38 the wedge, versus the three directions under parallel research (ASI
   UI / full ASI OS / nano-kernel) and R18? Note the structural argument: R38 is the only candidate that
   monetizes the *already-red-teamed enforcement core* without asking anyone to adopt a language, and it
   composes with R18 later (the audit chains R38 emits are exactly R18's ingestion-free event source). But
   it is a B2B SDK business — sales-heavy, integration-heavy — which may not be the company the founder
   wants to build. Decide before Slice 1.
2. **Generation viability threshold** — is 80%/≤3-repairs the right kill line, and which model tier is the
   reference? (A wedge that only works on frontier-strong models has a cost floor.)
3. **Freeze vs fork** — does the Embedded profile freeze constrain the ASI research language, or fork a
   subset grammar? Default: subset-freeze with the research track a strict superset; revisit at Slice 2.
4. **Pricing shape** — per-run metering (the budget meter already counts the billable unit) vs seat/SDK
   license. Note the meter's µ$ accounting was built for exactly this.
5. **Does the tool registry subsume `@[contained]`?** Long-term the host-imposed ceiling + typed tool
   registry may be the *primary* containment surface (attributes remain for hand-written Axon). If so, that
   is an I-11 evolution to spec properly, not slip in.

### 13. Alternatives considered and rejected (one paragraph each, per the research brief)

**(a) Capability-policy sidecar wrapping Python/JS/Go agents.** Reaches the largest population but forfeits
the differentiator: a runtime proxy on foreign tool-calls is enforcement-after-the-fact with no compile-time
refusal, no transitivity, no typed effect rows — exactly the "policy separate from code" pattern the
flagship's Docker foil demolishes. It lands Axon in the crowded agent-firewall/MCP-gateway field holding a
commodity product, with the compiler (the actual asset) contributing almost nothing. Rejected: wrong side of
our own moat.

**(b) Hosted attested sandbox cloud (E2B/Modal-class, differentiated by compile-time guarantees).** Same bet
as R38 with strictly worse economics: all of R38's adoption risk *plus* greenfield multi-tenant infra
against well-funded incumbents whose product is the infra. R38 rides the host's infrastructure instead; a
hosted tier can graduate out of R38 later if design partners demand it. Rejected as the lead; kept as a
possible Slice-4.

**(c) Formal-verification-as-a-service for arbitrary LLM-generated code.** The SMT machinery (R9/R20, and
R23-proof-certificates) proves properties of *Axon* programs because the language was shaped to make the
fragments tractable; arbitrary Python/JS verification is a different, mostly unsolved research problem. The
asset does not transfer; selling it as if it did would be dishonest. Rejected.

**(d) Physical-actuator safety interlocks (robotics).** Seductive fit on paper — R25 (Zephyr/Cortex-M) and
R22 (`native::modbus`) are landed, refinement types + runtime exit-6 enforcement could type command
envelopes, and the LLM-policy-on-robots wave is real. But functional-safety markets buy *certification*
(IEC 61508/ISO 13849), not novel languages; interlocks live in certified hardware/PLC layers by regulation;
sales cycles are multi-year against incumbents whose moat is paperwork we don't have. A great *vertical for
later* (via R38: an embedded runtime a robotics host embeds above the certified layer), a graveyard as a
company-defining wedge today. Rejected as lead.

**(e) EU-AI-Act compliance-evidence packs.** Timely (high-risk-system obligations phasing in now) and close
to landed machinery (R26 attestation, R28 ledger, Phase-11 risk typing, replay): auto-generate a signed
conformity-evidence bundle per deployed agent. But standalone, it inherits the fatal coupling — evidence is
only generatable for workloads *running on Axon* — so it is a **feature of R38** (the audit/replay/attest
chains the embedded runtime emits, rendered against regulatory articles), not an independent wedge.
Folded into R38's Slice-3+ value story.
