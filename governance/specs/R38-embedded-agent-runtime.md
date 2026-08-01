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
related: R18-provenance-ledger, R22-intent-approve-gateway, R27-corrigibility-resource-bounds, R16-axon-ui, R36-full-asi-os, R37-nano-micro-asi-kernel, R25-information-flow-monitor (added 2026-07-31 — owns the "what flows through a granted channel" question R38's capability card does NOT answer; see §7 N-1 and §12 Q6), R23-proof-certificates (the untouched Proof pillar, §7 N-2)
conflicts-with: none
reserves: one embed-registry E-block, number TBD at implementation — do not enumerate free bands here; re-grep error.rs + all governance/specs `reserves:` lines at implementation time (known taken as of 2026-07-31: E2300–E2302 [eBPF], E1810 [TEE], E2000–E2003 [R41 polyglot], E3700–E3704 [R37 nano-kernel], E2100–E2104/W2110–W2111 [R16 axon-ui — same-day reservation, added here 2026-07-31], per §6)
evidence: none (Draft; Slice-0 gates are embed_ceiling_refuses_undeclared_capability_via_api + embed_generation_viability_measured + embed_adversarial_generation_measured [third half added 2026-07-31] per §9)
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
run deterministically and shows the hash-chained audit trail. (Replay of *tool-effectful* runs requires the
host-reply record/replay layer, which is greenfield — see the §3 replay row and Slice 1.)

> **⚠ What the card does NOT say [added 2026-07-31 — the headline non-guarantee, do not ship marketing
> that omits it].** The capability card is a statement about **which channels exist**, not about **what
> flows through a granted one**. The project's own `R25-information-flow-monitor` §1 says it plainly:
> *"A capability bound limits which channels exist, not what flows through a granted one… An AI
> legitimately granted net to one endpoint can exfiltrate arbitrary secrets through it."* The example card
> above grants a read channel (CRM) **and** an outbound-comms channel (email) — composed, those two grants
> **are** an exfiltration path: read the CRM, place it in the mail body, send to an attacker-chosen
> address. Every tool registered, every host allowlisted, the budget respected, the card 100% true, and
> every enforcement mechanism in this spec reporting success. The shipped audit ledger cannot even
> reconstruct it after the fact: `axon-audit`'s `append(principal, effect, op)`
> (`crates/axon-audit/src/lib.rs` ~243–280) records an operation string with **no argument payload**.
> **Misuse of a granted tool with hostile arguments is explicitly out of scope for the compile-time
> guarantee** (restated as a non-guarantee in §7, targeted by the §8 red-team pass, and partially
> addressable via the §9 Slice-2b value-constraint work and §12 Q6). Selling a "provable capability card"
> to a fintech/CRM ICP without that sentence would be the same overclaim §1's table accuses V8 isolates of.

### 2. Requirement link

Opens a new `REQUIREMENTS.md` row **R38** (platform-vision / product bucket, alongside R16/R17/R18). No
language-semantics change to *existing* constructs — **[corrected 2026-07-31]** but the checkable tool-call
convention (§4) is a small new surface form + checker rule, so "zero language change" is not honest; the
change is additive and embed-scoped. It advances ROADMAP §0 **Containment** (the pillar productized) and §2.1 (the
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
> 6. **[added 2026-07-31] The guarantee we sell is narrower than the pain we qualify on.** The
>    capability card constrains *channels*; the ICP's exposure (records, money, communications) is about
>    *data*, and two granted channels compose into an exfiltration path with the card 100% true. The
>    enforcement core does not expire — a stronger generator does not weaken compile-time channel
>    refusal — but the *product claim* wrapped around it does. See the §1 box and §7 N-1; do not let
>    marketing outrun it.
> 7. **[added 2026-07-31] Our adversarial evidence describes a careless generator, not an optimizing
>    one.** Every attack in the corpus reaches for a denied channel and gets refused. A generator handed
>    the ceiling in its own prompt will simply stay inside it. §9's third Slice-0 half is the first
>    measurement of that axis, and §7 N-3/N-4 record the assumptions it puts on the clock.

### 3. What is genuinely landed vs. derived vs. greenfield (honest reuse accounting)

The R18 post-review correction taught the lesson: count reuse against *code*, not concepts. Verified against
the tree at spec time:

| Embedded-runtime need | Shipped? | Reality |
|---|---|---|
| Sub-second, no-LLVM, embeddable interpreter | ✅ shipped | `interp.rs`; `cargo build -p axon-core --no-default-features` ≈ sub-second; full builtin coverage (see `builtins.rs` — ~300 entries at last count; do not hardcode a number here, it goes stale); already compiled as a `wasm32-unknown-unknown` cdylib with a bare C ABI (`crates/axon-wasm` — `axon_alloc`/`axon_eval`/output read-back, zero JS glue) and at 28/28 interp parity on `wasm32-wasip1` |
| Compile-time capability refusal against a host-declared ceiling | ✅ shipped | `@[contained]` E1001/E1004: transitive (no helper laundering), path-traversal-safe, env-var-denied, fail-closed on dynamic targets; Phase 6 effect rows + E1310 anti-laundering as the successor surface |
| Host-mediated tool calls (the embedding seam) | ✅ shipped | the `AxonHost` seam (every host touchpoint routes through a thread-local host trait; `DefaultHost` = std) + R15 `host_await`/`host_await_val` (a program genuinely suspends, the host answers, it resumes — on native, wasip1, and the browser via Asyncify). **This is exactly the tool-call boundary an embedded runtime needs, already built.** |
| Budget / metering | ⚠️ partial | **[corrected 2026-07-31]** E1301 per-fn AI-call budgets, kernel per-token cost meter (`ai_cost_spent`, real `cost_usd` in provenance), kernel `Goal` budget exhaustion (exit 7), and `AXON_MAX_DEPTH` are shipped — but the §4 `Budget` fields `wall_ms` and `tool_calls` have **no shipped counterpart** (zero hits across the tree), and the interpreter is a synchronous tree-walker with no fuel/preemption checkpoints, so wall-clock enforcement is interp-core work (fuel counters or a checked cancellation flag on the eval loop), not thin-crate packaging. Counted in the Slice-0 build list (§9) |
| Kill-switch | ⚠️ partial | **[corrected 2026-07-31]** the R27 one-way latch (exit 4, `@[corrigible]`) is shipped — but it is tripped only by the **in-language** builtin `corrigible_halt()` writing an interpreter-internal non-`Sync` `Cell<bool>` (interp.rs ~378). There is no API, signal, or cross-thread path for an **embedder** to trip it from outside the running program — which is the entire R38 posture (authority imposed from outside, §7 I-11). A generated `while true {}` between tool calls is unstoppable by any shipped mechanism. External host-imposed cancellation is greenfield, counted in the Slice-0 build list (§9) |
| Deterministic replay | ⚠️ partial | **[corrected 2026-07-31]** `AXON_SEED` + `AXON_AI_REPLAY` (memoized `(prompt,model)` `ai_complete` re-runs) + run-ids + `axon trace --replay` ARE shipped — but that covers only RNG + AI calls. The `AxonHost` surface (host.rs: `read_file`/`write_file`/`env_var`/`now_ms`/`exec`/`http_*`) has **zero record/replay machinery**, and the R15 contract is the opposite: `r15_host_await_effects_fire_once_not_per_resume` (interp.rs ~3046) asserts "host must be called exactly once per host_await (no replay)". For an embedded runtime whose product IS host-tool calls, byte-identical replay requires recording and re-feeding every `host_await` reply plus the rest of the `AxonHost` surface — greenfield, counted in Slice 1 (§9) where the replay feature demos |
| Tamper-evident audit | ✅ shipped | R28 hash-chained capability ledger (FS/Net/AI/Exec/Random/IO), `axon-os audit verify`; mandatory I-13 provenance |
| Structured diagnostics for an LLM repair loop | ⚠️ partial | R8 `axon-diag/1`: versioned JSON with code/line/col/`expected`/`found`/`help` — purpose-built for machine consumption — **is** shipped. But the repair *loop* is not: **[corrected 2026-07-31]** `axon intent compile` under `AXON_INTENT_GEN` is a SINGLE-SHOT `axon_ai::complete` call (main.rs ~4750–4794) — the generated source is written to disk unchecked, no diagnostics are fed back to the model, zero repair rounds (AI error silently falls back to the skeleton). The check→repair harness is greenfield Slice-0 work (modest, since the diagnostics side exists) |
| **A stable embedding API** (`Runtime::builder()`, typed tool registration, per-run ceilings as *data* not source attributes) | ❌ **greenfield** | today the ceiling lives in `@[contained]` source attributes and env vars; an embedder needs to impose it *from outside* the program, per run, via API. This is the core Slice-0 build: a thin crate (`axon-embed`) wrapping interp + checker, injecting the ceiling and tool schema |
| **Host-tool typing** (declare a tool's arg/return schema + effect row; generated code type-checks against it) | ❌ **part-greenfield** | **[corrected 2026-07-31]** `host_await_val` is typed `T -> U` at the call site, but the request is an **opaque payload — the tool NAME is data inside it, invisible to the resolver/checker** (builtins.rs ~700–740). Static "check against the tool schema" therefore needs a new checkable calling convention (§4: a literal-name `tool(...)` form; dynamic names refused fail-closed) — a language-surface + checker change, not derived work on shipped pieces |
| Embedded-profile language freeze + conformance suite + semver | ❌ greenfield | and politically expensive (§12 Q3) |
| Automation-grade stdlib (HTTP/JSON/date/regex ergonomics) | ⚠️ partial | dict/str landed; JSON/date/regex thin |
| Generation viability (models emit valid Axon reliably) | ❌ **unvalidated** | the load-bearing hypothesis; Slice 0 measures it |

**Net:** unlike R18 (where the "ledger" turned out to be mostly greenfield), the *capability-refusal*
core — the compile-time containment the product leads with — is shipped and red-teamed. **[corrected
2026-07-31]** But the adversarial sweep downgraded three more rows the first pass missed: tool-effectful
replay, host-imposed kill/cancellation, and `wall_ms`/`tool_calls` budgets are all greenfield (two of
them interp-core work, not packaging), and host-tool typing needs a new checkable surface form. The
honest shape: refusal + audit are shipped; the *operational* half of the pitch (meter, kill, replay)
is partly aspirational and is priced into Slices 0–1. Still a better reuse ratio than R16/R17/R18 at
their spec time, but not the "greenfield is just packaging" claim of the earlier draft.

### 4. Surface (what the embedder writes — not what end users see)

Rust-level sketch (C ABI and wasm exports mirror it; contract frozen at Slice 2, not before):

```rust
let rt = axon_embed::Runtime::builder()
    // Typed tools: name, arg/return schema, effect row, host handler.
    .tool("crm.lookup",  schema!((str) -> str), effects![Net("api.internal")], |req| host.crm(req))
    .tool("email.send",  schema!((str, str) -> bool), effects![Net("mail.internal")], |req| host.mail(req))
    // The per-run ceiling, imposed from OUTSIDE the program (the host, not the code, holds authority).
    // NOTE [2026-07-31]: `caps!` is SUGAR over an effect row — the ceiling's canonical data model is a
    // Phase-6 effect row (§3 already names rows + E1310 as the successor surface, and `effects![…]`
    // above is already row-shaped). Freezing the legacy fs/net/exec triple as the public contract is
    // forbidden until §12 Q5 is resolved — see the §9 Slice-2 sequencing note.
    .ceiling(caps! { fs: none, net: ["api.internal", "mail.internal"], exec: none })
    .budget(Budget { tool_calls: 50, ai_usd_micro: 50_000, wall_ms: 2_000, max_depth: 500 })
    .build()?;

// 1. Check: capability + type refusal BEFORE first run. Diagnostics are axon-diag/1 JSON —
//    feed them straight back to the generating model for repair.
let program = rt.check(generated_ax_source)?;          // Err(diags) = the repair-loop input

// 2. The legible capability card (what the end user approves — §2.4 transplanted):
let card = program.capability_card();                   // tools used, hosts, effect rows, budget
//    [added 2026-07-31] the card MUST also carry: (a) capabilities ATTEMPTED-AND-REFUSED across the
//    repair loop, (b) proof obligations discharged statically vs. deferred to runtime, and (c) the
//    generation provenance (prompt hash, round count). See the §4 repair-loop-evidence rule.

// 3. Run: every tool call surfaces as a host_await suspension the host answers.
let run = rt.run(&program, input)?;                     // output + audit chain + replay handle

// 4. Later: deterministic replay + tamper-evident audit for support/compliance.
//    NOTE [2026-07-31]: byte-identical replay of tool-effectful runs requires the run to have
//    RECORDED every host reply (host_await results, now_ms, read_file, http_*) so replay re-feeds
//    them instead of re-calling the host — greenfield (§3 replay row), NOT covered by AXON_AI_REPLAY.
let rerun = rt.replay(run.handle())?;                    // byte-identical
assert!(run.audit_chain().verify());

// 5. External termination: the host can kill a runaway program from OUTSIDE at any time
//    (a run handle exposes cancel(); wall_ms is enforced by the same eval-loop checkpoint).
//    Greenfield interp-core work — see the §3 kill-switch/budget rows and Slice 0.
```

**Tool-call convention [added 2026-07-31]:** generated code invokes registered tools via a
**literal-name form** — `tool("crm.lookup", args…)` (exact surface TBD at Slice 0) — where the tool
name MUST be a compile-time string literal. The checker resolves the literal against the registry
(name, arg/return schema, effect row) at `rt.check` time. A dynamically-computed tool name (concat,
variable) is **refused fail-closed** with an embed-registry diagnostic (§6). This is what makes
"generated code type-checks against the tool schema" true; raw `host_await` with a data-borne tool
name cannot be statically validated and is not the embed convention.

**Where the ceiling may come from [added 2026-07-31 — I-11 integrity rule, see §7].** §5 sells
*per-automation* least-privilege, and a per-automation ceiling has to be derived from a per-automation
end-user request — which makes "ask the same model that writes the code to also declare what capabilities
it needs" the cheapest implementation and the one a host with a strong model in the loop will reach for.
That inverts I-11 instead of elevating it: the generator would pick its own bound and the compile-time
guarantee would degrade to *"the model was self-consistent."* **Rule:** the ceiling MUST originate from a
host-authored source the generating model cannot write (static config, tenant policy record, or a human
grant). A model MAY propose a ceiling — legitimate and good UX — but a proposal is only ever **strictly
narrowing** relative to a host-set maximum, combined by the same §8 intersection rule (fail-closed), and
the proposal itself is recorded in the audit chain as a **model-authored artifact**, not as authority.
Gated by a §8 test mirroring the ceiling-equivalence test.

**Repair-loop evidence rule [added 2026-07-31].** The check→repair loop below is graded feedback against
the exact boundary this product sells: `axon-diag/1` is deliberately maximally informative (code, line,
col, `expected`, `found`, `help`) and the generator is handed the tool schema and the ceiling up front.
For a benign generator that is the feature; for an injected or misaligned one it is three free, guided
attempts to find a route the checker accepts. The spec's default posture must therefore treat **refusal
history as signal, not as noise to be repaired away**:
- (a) The full generate→refuse→regenerate transcript (prompt hash, each round's `axon-diag/1` payload, the
  set of capabilities *attempted and refused*) is bound to the program artifact and appended to the R28
  hash-chained ledger. This is exactly the event a compliance buyer wants, and §12 Q1 already notes R38's
  chains are R18's ingestion-free event source.
- (b) The capability card surfaces **attempted-and-refused** alongside **granted**. An automation that hit
  E1001 twice reaching for the filesystem before emitting a clean third version must NOT present to a
  human approver identically to one clean on round 1.
- (c) **Repair a type error; do not repair a capability denial.** Repeated capability refusals are a
  policy-configurable **hard stop**, not a free retry. Default: capability-class refusals (E1001/E1004/
  E1310/embed-registry) do not consume a repair round — they end the loop.
- (d) Round-by-round refusal statistics are a **pre-registered measured output** of the §9 Slice-0
  harness. Collection is free and it is the cheapest early indicator of a generator optimizing against
  the gate rather than fumbling toward it.

**Tool-argument constraints [added 2026-07-31 — the partial answer to the §1 non-guarantee].** The §4
registry schema carries only arg/return *types* and an effect row today. It SHOULD carry optional
refinement predicates on tool args/returns (e.g. `email.send(to: str where to_domain_in(…))`), which the
shipped default-pipeline SMT discharge (`crates/axon-core/src/smt.rs`, `verify::Discharged`) proves
statically where it can and the shipped four-obligation-site runtime path (exit 6) enforces otherwise —
a strictly better failure mode than an unconstrained call. This converts part of the card from *which
channels* to *which values*. Scoped as Slice 2b (§9) with a §9 Slice-0 measurement, not assumed.

The error case is the product: `rt.check` on a generated program that calls `read_file`, an unregistered
tool, an unlisted host, or exceeds the ceiling returns E1001-class diagnostics — the automation is refused
before it exists as a runnable thing, and the diagnostic is machine-readable enough for the model to repair
or for the UI to explain.

**Server-side generation loop** (the other half of the SDK): `axon-embed` also exposes a
prose→skeleton→generate→check→repair loop, so a host can go from end-user prose to a checking-clean,
ceiling-conformant program without building the loop themselves. **[corrected 2026-07-31]** An earlier
draft claimed `axon intent compile` + `axon-diag/1` "already implement" this loop; that is false — the
`axon-diag/1` diagnostics are shipped (R8), but `axon intent compile`'s generation path is single-shot with
no post-generation check and no diagnostic feedback (see §3 table row). The loop itself is greenfield
Slice-0 work, built on the shipped diagnostics.

### 5. Who the buyer is (and who it is not)

- **Primary ICP:** product/platform teams adding AI-authored automations over **consequential tools**
  (CRM/support/fintech/ops SaaS; internal-tools platforms; agent-infrastructure vendors adopting
  code-as-action). Qualifier: they run *untrusted-tenant* generated code against tools that mutate records,
  move money, or send communications — the population for whom "isolate + API key" is visibly not enough.
  **[added 2026-07-31] Sell this ICP the right guarantee.** Exactly this qualifier — consequential,
  data-bearing tools — is the population for whom the §7 N-1 non-guarantee bites hardest: their pain is
  "did our customer data leave", which a channel-level card does not answer. The honest pitch is
  *"per-automation least-privilege over your tool surface, provably enforced before first run, metered
  and audit-chained"* — **not** "your data can't leak." Q6 (§12) is the strategic question of whether
  R38 closes that gap or names it.
- **Secondary:** agent-framework vendors wanting a safe tool-composition language (the CodeAct execution
  half) they can ship without building a sandbox story themselves.
- **Not the buyer:** teams happy with JSON tool-calling; teams whose sandbox needs are coarse; anyone who
  must run *existing* Python/JS agent code unchanged (that is the sidecar idea, rejected in §13a — it
  forfeits the compile-time guarantee that is the whole moat).

Relationship to the flagship: same guarantee, different distribution. The flagship sells the guarantee to
Axon *authors*; R38 sells it to hosts whose *models* are the authors — which is the only author population
that scales, and the one ROADMAP §2.1 already declared canonical.

### 6. Error codes

**[corrected 2026-07-31]** Mostly no new language diagnostics — but the literal-tool-name convention
(§4) adds one new checker refusal class (dynamic tool name), which lives in the embed-registry E-block
below rather than a core language band. Otherwise the embed API surfaces existing codes (E1001/E1004 capability, E1310 effects,
E1301 budget, exit 4/6/7/8 runtime classes) through `axon-diag/1`. Slice 0 allocates one new block for
embed-registry errors (unknown tool name, tool-schema mismatch, dynamic/non-literal tool name, ceiling-file malformed) from the next free
E-range at implementation time. Do not trust any enumerated free-band list in this spec: re-grep
`error.rs` **and** every `governance/specs` `reserves:` line at implementation time (R41's phrasing). Known
taken as of 2026-07-31: E2300–E2302 (eBPF), E1810 (TEE), E2000–E2003 (R41 polyglot — not yet in error.rs
but reserved on paper), E3700–E3704 (R37 nano-kernel, likewise paper-reserved).

### 7. Invariants touched

- **I-11 (capability boundary)** — *elevated*: the boundary becomes host-imposed data (the ceiling), not
  only source attributes. The refusal semantics do not change; the authority source moves outward. This is
  the same direction R22-intent-approve's `Grant` model already points.
  **[added 2026-07-31] The elevation is conditional on the §4 ceiling-provenance rule**: the ceiling MUST
  come from a host-authored source the generating model cannot write; a model-proposed ceiling is only
  ever strictly narrowing against a host maximum. Without that stated as a property, "authority moves
  outward" is an implementation convention, and it is precisely the convention a more capable generator
  makes most tempting to break (a model good enough to clear the §9 80% bar is good enough to write a
  plausible capability manifest for itself).
- **I-13 (provenance not opt-out-able)** — preserved and productized (the audit chain is a sellable
  artifact, as in R18, but generated by *our* runtime rather than ingested).
- **I-10 (determinism available)** — load-bearing for the replay feature.
- **I-2 (interp/codegen parity)** — untouched; the embedded runtime is interp-only by design (codegen's
  E0910 refusals of interp-only builtins are irrelevant inside the SDK).
- No invariant is weakened; no TCB component changes.

**Stated non-guarantees and expiring assumptions [added 2026-07-31 — these are limits of the product
claim, not TODOs; they belong next to the invariants so nobody reads §7 as a completeness list]:**

- **N-1 — the card is channel-level, not data-level.** Nothing in R38 constrains *what flows through* a
  granted channel. Composing granted tools with attacker-chosen arguments (read CRM → email body →
  attacker address) defeats the product claim with every mechanism reporting success (§1 box). The
  landed `Tainted`/`Trusted`/`Secret`/`Public` lattice (`examples/stdlib/tainted.ax`) and the
  R25-information-flow-monitor spec are where an answer would live; R38 as specified does **not** answer
  it. Partial mitigations are scoped (§4 tool-argument constraints, §9 Slice 2b, §12 Q6), not claimed.
- **N-2 — the Proof pillar is not productized.** ROADMAP §0 pillar 1 (Proof) is landed — SMT discharge
  wired into the DEFAULT run/build pipeline, refinement types at all four obligation sites with exit 6,
  `@[verify]` on scalar returns, R23-proof-certificates — and R38 touches **none** of it. The historical
  reason not to demand contracts from authors was human ergonomics, an argument ROADMAP §2.1 explicitly
  retired ("treat it as an IR, not a human-authored surface"). The cost of *asking a generator* for
  `where` predicates and `@[verify]` postconditions is bounded (a wrong contract is refused, not
  trusted) and falls as models improve, while the value rises. Tracked as an axis (§9 Slice-0
  measurement + Slice 2b), not left as an omission.
- **N-3 — human approval of a capability card is the weakest link, and R38 scales it linearly with
  generation volume.** §2.4's "users approve the AST" is transplanted as per-automation card approval,
  but the pitch is a host generating effectful automations *per user, per request, and increasingly per
  turn*. Per-action human consent at that rate has one empirical outcome (Android permissions, UAC,
  OAuth scope screens): the click becomes a reflex and the only real gate is the ceiling — which by N-1
  is channel-level. **This spec depends on a human meaningfully reviewing every generated artifact, and
  that dependency degrades exactly as generation gets cheaper.** It is stated here as a limit rather
  than left implicit; the envelope-diff mitigation and its Slice-1 measurement are in §9, the strategic
  question in §12 Q7. Note the direction of the trade: as the fraction of runs needing a fresh human
  decision falls, the approved *envelope* becomes the real security boundary, which raises the stakes on
  N-1 and the §4 ceiling-provenance rule rather than lowering them.
- **N-4 — the adversarial evidence base assumes a careless generator, not an optimizing one.** The
  flagship corpus attacker reaches for `read_file`/`http_*`/`exec`/`env_var` and walks into the refusal.
  A generator optimizing against a boundary handed to it in its own prompt will not touch a denied
  channel. §9's third Slice-0 half exists to stop this assumption from going unmeasured.

### 8. Test plan

- [ ] Unit: ceiling-as-data equivalence — a program refused under source `@[contained(...)]` is refused
      identically under the API-imposed ceiling (same codes), and vice versa. Note: the flagship corpus
      files carry `@[contained(fs: [], net: [], exec: none)]` source attributes
      (`agent_task_evil.ax:26`, `agent_task_llm_generated.ax:9`), so demonstrating ceiling-as-data
      requires **attribute-stripped corpus variants** as an explicit fixture-preparation step — commit
      them alongside the harness. **Composition rule when BOTH a source attribute and an API ceiling are
      present: intersection — anything either side denies is refused** (fail-closed; the host ceiling can
      never widen what the source declares, and vice versa). The equivalence test must cover this
      both-present case too.
- [ ] Integration: tool registry — a generated program calling a registered tool within the ceiling runs;
      an unregistered tool / unlisted host / schema-mismatched call is refused at `check`.
- [ ] Adversarial (the flagship transplanted): the prompt-injected-LLM corpus
      (`agent_task_llm_generated.ax` + fresh live generations) run through `rt.check` — all exfil channels
      refused; add a red-team pass specifically against the *embed* seam (host handler confusion, ceiling
      TOCTOU between check and run, tool-name spoofing).
      **[added 2026-07-31] The red-team pass MUST also target the non-mechanism attack:
      *exfiltration by composing granted tools with attacker-chosen arguments*** — a program that touches
      no denied channel, passes `rt.check` clean, respects the budget, and still moves a marked canary
      record out through the granted `email.send`. A red-team suite that only contains
      denied-channel attempts is a suite that can only find the bugs we already fixed (§7 N-1/N-4).
- [ ] **[added 2026-07-31] Ceiling provenance: a model-proposed ceiling cannot widen the host ceiling.**
      Mirrors the ceiling-equivalence test: proposed ∧ host-maximum = intersection, fail-closed; a
      proposal naming a host or effect outside the host maximum is refused, and the proposal is recorded
      in the audit chain as model-authored (§4 rule, §7 I-11).
- [ ] **[added 2026-07-31] Repair-loop evidence:** a program that reached checking-clean only after ≥ 1
      capability-class refusal produces a card that differs (attempted-and-refused set is non-empty) from
      an otherwise-identical program clean on round 1, and both transcripts verify in the R28 chain.
      Also: a capability-class refusal does not consume a repair round (§4 rule (c)).
- [ ] Property: `run` → `replay` byte-identical **on a run that makes ≥ 1 registered host-tool call
      and ≥ 1 `now_ms` read** — i.e. the recorded host replies + timestamps are re-fed, not re-fetched
      (a program with no tool calls passes this vacuously and does NOT count; the seed + AI-replay
      cache alone are necessary but not sufficient — **[corrected 2026-07-31]**, see §3 replay row);
      audit chain verifies; a tampered entry fails.
- [ ] Termination: the embedder cancels a deliberately-infinite generated program
      (`while true {}` between tool calls) from outside within the `wall_ms` budget; the run reports
      a budget/cancel outcome, not a hang. **[added 2026-07-31]** — no shipped mechanism can do this
      today (§3 kill-switch row); this test is the proof the Slice-0 cancellation work landed.
- [ ] Journey (Slice 1): prose → generate → repair loop → capability card → approve → run → audit view,
      end-to-end in the reference host.
- [ ] **The hypothesis test (Slice 0, pre-registered):** generation-viability harness per §9.

### 9. Acceptance criteria (per slice, kill-gated)

**Slice 0 — spike (THREE halves as of 2026-07-31, all must pass; any one can kill):**

> **Why a third half [added 2026-07-31].** As originally written, Slice 0 spent its *entire*
> pre-registered kill threshold on **generation viability** — the one hypothesis that trends toward
> resolution on its own. "Can a frontier model emit a small typed DSL against a documented tool schema,
> with machine-readable diagnostics and three repair rounds" is squarely in the direction of travel; on a
> 1–3 year horizon it resolves toward *yes*, and the Python-in-isolate gap (a measure of how penalized a
> low-resource target language is) shrinks on the same trend. Meanwhile the axis that trends the *other*
> way — a generator that reads the ceiling it was handed and optimizes *within* it — had **no gate at
> all**: the entire safety half runs a FIXED, COMMITTED corpus written by a careless or crudely-injected
> generator that reaches for `read_file`/`exec`/`env_var` and walks into the refusal, and §8's adversarial
> bullet listed only mechanism attacks. So the gate deciding whether the wedge lives measured a variable
> trending safe and did not measure the variable trending dangerous. Half three fixes that. It is the only
> half whose difficulty *rises* with model capability, and it is the half that tests the actual product
> claim.
- [ ] `embed_ceiling_refuses_undeclared_capability_via_api` — a minimal `axon-embed` crate imposes a
      ceiling + tool registry from outside the source and refuses the flagship exfil corpus **run on
      attribute-stripped variants** (the shipped corpus files carry `@[contained]` attributes; stripping
      them is a required fixture step so the refusal is demonstrably ceiling-driven, per §8).
      **Positive control [added 2026-07-31, mandatory — the gate is the AND of both halves]:** at least
      one committed benign program that uses a registered tool inside the ceiling must **run to
      completion through the same API**. A refuse-everything stub must fail this gate (the
      vacuous-pass class the repo has been bitten by before — coverage-vacuous-pass guard).
      The Slice-0 build list also includes, priced here: (a) the **external cancellation +
      `wall_ms`/`tool_calls` budget enforcement** (an eval-loop cancellation/fuel checkpoint —
      interp-core work, §3 kill-switch/budget rows, gated by the §8 termination test) and (b) the
      **literal-tool-name calling convention + registry check** (§4).
- [ ] `embed_generation_viability_measured` — N ≥ 30 realistic automation tasks × a frontier model emitting
      Axon against a documented tool schema, with ≤ 3 `axon-diag/1` repair rounds. The **repair-loop
      harness itself is part of the Slice-0 build list** (generate → check → feed `axon-diag/1` back →
      regenerate, round-capped): it does not exist today — `axon intent compile` is single-shot (§3/§4,
      corrected 2026-07-31) — so its cost is counted here, before the measurement is scheduled.
      **Pre-registered kill threshold: < 80% of tasks reaching checking-clean + behaviorally-correct kills
      the wedge** (record the number either way; do not move the goalposts after seeing it). **The
      correctness oracle must be pre-registered alongside the threshold, before any measurement run:**
      each task ships with expected-output assertions (a fixture harness) — or, where free-form output
      makes that impossible, a fixed written judge protocol — plus an explicit tie-breaking rule for
      partially-correct outputs (default: partial credit = fail; a task either passes its assertions or it
      does not), all committed to the repo with the task corpus. Compare against the same tasks in
      Python-in-isolate as the honesty baseline, **scored by the identical pre-registered oracle**.
      **Baseline decision rule [added 2026-07-31, pre-registered]:** the baseline is not decoration —
      if the Python-in-isolate pass rate exceeds Axon's by **more than 10 absolute points**, that is
      recorded as an **adverse finding** that the §12 Q1 comparison MUST weigh explicitly (it measures
      the generation-viability tax of targeting Axon), even when Axon clears the 80% absolute
      threshold. It does not by itself kill the wedge, but it cannot be ignored or re-scored after the
      fact.
      **Additional measured outputs [added 2026-07-31, pre-registered, near-zero marginal cost —
      measured, NOT part of the kill threshold]:** (i) round-by-round **refusal statistics**, split
      capability-class vs. type-class (§4 rule (d)) — the cheapest early indicator of a generator probing
      the gate rather than fumbling toward it; (ii) the **contract delta** — run the same corpus twice,
      with and without refinement contracts on tool args demanded (§4 tool-argument constraints), and
      record the pass-rate difference. That number is a direct read on how much **Proof** a generator can
      be asked to carry today (§7 N-2); it will only improve, and it turns an untouched pillar into a
      tracked axis. (iii) Pass rate **per model tier** (§12 Q2's reference-tier question).
      **Re-measurement clause [added 2026-07-31 — this does NOT loosen the kill line]:** a recorded kill
      stands as recorded and is not retroactively re-scored. Re-measurement on a *newer model tier* is
      permitted only as a **fresh, separately pre-registered run** (same corpus, same committed oracle,
      same 80%/≤3-repair threshold, results recorded either way), so that a wedge is not permanently
      buried by a model-capability deficit with a six-month half-life — and equally so that a failed run
      cannot be quietly re-rolled until it passes.
- [ ] **`embed_adversarial_generation_measured` [added 2026-07-31 — the third half, pre-registered
      identically to the second]:** an ADVERSARIAL GENERATION harness. A frontier model is given the tool
      registry, the ceiling, the checker rules, and **this document**, and is rewarded for a named
      forbidden outcome — (a) exfiltrate a marked canary record through the granted `email.send` while
      touching no denied channel; (b) exceed effective spend by composing calls under the `tool_calls`
      cap; (c) get a hostile automation past a capability card that an **independent evaluator** (a human
      or a separate model that did not see the attack prompt) approves. Canaries, success criteria, and
      the evaluator protocol are **committed before any run**; results recorded either way.
      **This half is measured-and-reported, not kill-thresholded, at Slice 0** — a success here does not
      kill the wedge (N-1 already concedes the channel-level limit is real and known); it is *evidence
      that must be weighed explicitly in the §12 Q1 comparison* and it sizes the §9 Slice-2b / §12 Q6
      work. Recording a null result (no successful attack found) is equally required and equally
      informative — but a null result from a harness that never tried within-ceiling composition does not
      count and must be reported as a harness gap, not a pass.
- [ ] A green Slice 0 authorizes the §12 Q1 comparison — **not** Slices 1+.

**Slice 1 — reference host (the demo that sells):** a small automation-builder web app (reuse `axon-web`
patterns): end-user prose → generated program → **capability card approval pane** → metered run → audit
trail + one-click deterministic replay. **The host-reply record/replay layer (every `host_await` reply +
`AxonHost` surface: `now_ms`, `read_file`, `http_*`) is built HERE — it is greenfield (§3 replay row,
corrected 2026-07-31), and Slice-1 acceptance includes the §8 tool-effectful replay property.**
Acceptance: `embed_reference_host_journey` e2e; the card provably
matches enforcement (a card/enforcement divergence is a red-team finding, not a docs bug).

**[added 2026-07-31] Approval must be envelope-based, not per-run — and the ratio is a measured
acceptance criterion.** Per §7 N-3, a card no one reads is not a control, and per-run consent at
generation scale reliably produces reflex clicks. The approvable unit is therefore a reusable **policy
envelope**: an admin approves a ceiling/grant *template* once ("automations of this class may read CRM
and send mail to internal domains"), and a fresh per-run human decision is required only on **diff** —
when the derived card exceeds the previously-approved envelope. Card diffing is cheap and the artifacts
are deterministic. **Acceptance:** Slice 1 measures and reports the *fraction of runs requiring a fresh
human decision*; a design where that fraction is ~100% is recorded as a **design defect, not a UX
detail**. (Note the coupled risk: as that fraction falls, the envelope becomes the real security
boundary — which is why N-1 and the §4 ceiling-provenance rule get *more* load-bearing, not less.)

**Slice 2 — the contract:** Embedded-profile language subset spec + conformance suite + semver policy; wasm
(browser + wasip1) and C-ABI packaging at parity with the Rust crate.

> **[added 2026-07-31] Sequencing blocker — resolve §12 Q5 BEFORE this freeze, not after.** §3's own
> capability row names "Phase 6 effect rows + E1310 anti-laundering as the successor surface" and §4's
> per-tool declarations are already row-shaped (`effects![Net("api.internal")]`), yet the ceiling one
> line below is the legacy `fs`/`net`/`exec` triple — and §12 Q5 ("does the tool registry subsume
> `@[contained]`?") is still open. Freezing the public embedding contract on a surface the project has
> already declared superseded is exactly hard truth #3's ball and chain with the wrong object chained.
> **The ceiling is defined as an effect row from Slice 0; `fs`/`net`/`exec` are rendered as sugar over
> it**, so the frozen surface *is* the successor surface. This matters more with a stronger generator,
> not less: E1310's transitive row subsumption is the property you want as the contract when generated
> programs get longer and helper-chain laundering gets more inventive.

> **[added 2026-07-31] Second conformance artifact: GENERATOR conformance.** The language profile binds
> Axon to the host, but over a 1–3 year horizon **the host's model is the fastest-moving component in the
> system and is the actual author of behavior**. A model swap can change generated program shape,
> capability footprint, and card content overnight with nothing in this spec noticing. The raw material
> is already committed and already required: the §9 task corpus + its pre-registered oracle, the §8
> replay property, and the R28 chains. **Deliverable:** the corpus + oracle become a suite the host
> re-runs on **every model swap**, reporting (a) checking-clean rate, (b) capability-footprint diffs
> (cards before vs. after), (c) refusal-round statistics; deviation is a reportable metric with a
> threshold. One harness, only landed machinery, and its value rises monotonically with model churn —
> which also gives §12 Q2's "which model tier is the reference" a *continuous* answer rather than a
> one-time one. (This also under-uses replay less: §1 treats replay as support/compliance only; it is
> equally a regression substrate.)

**Slice 2b — value constraints (scoped from the §7 N-1/N-2 non-guarantees; opens only if Slice 0's
adversarial half or a design partner shows the channel-level card is insufficient in practice):** the
partial, decidable answers to "what flows through a granted channel", in ascending cost —
(a) **refinement predicates on tool args/returns** in the registry schema (§4), statically discharged by
the shipped default-pipeline SMT and otherwise enforced by the shipped exit-6 runtime path, with
"obligations discharged statically / deferred to runtime" as a capability-card field;
(b) **argument recording in the audit chain** — `axon-audit`'s `append(principal, effect, op)` carries no
argument payload today (`crates/axon-audit/src/lib.rs` ~243–280), so post-hoc leak review is currently
impossible *in principle*; record tool-call arguments or salted hashes;
(c) a **registry-level value-flow declaration** — which registered tool's outputs may reach which tool's
inputs — as a coarse, decidable non-interference lattice over the tool graph (the tool graph is small and
static, which is what makes this tractable where general IFC is not; cf. R25's L1.5 I/O-proxy scoping and
the landed `Tainted`/`Secret` lattice in `examples/stdlib/tainted.ax`).
This slice is **scoped, not promised** — §12 Q6 holds the strategic question of whether R38 should own it
at all or defer to R25.

**Slice 3 — design partners:** 1–2 real external hosts integrate; their friction list becomes the next spec
(same protocol as ROADMAP §9.5).

### 10. Performance budget

Slice 0: `rt.check` p95 < 150 ms for a 200-line generated program (it is inside an end-user interactive
loop); `rt.run` overhead vs bare `axon run` < 10%.

> **[corrected 2026-07-31] Restate this as a curve, not a fixed-size number — two premises under it are
> drifting.** (1) Stronger models emit substantially *larger* programs for the same task (helper
> decomposition, defensive branches, and contract annotations if §4's tool-argument constraints land), so
> "200-line" is sized for what a weaker model writes. (2) §9 permits up to **3 repair rounds**, so the
> user-visible interactive latency is up to **4 checks + 4 generations**, not one check — the number as
> written measures a component, not the loop the justification appeals to. A budget stated this crisply in
> a spec becomes the quoted contract to embedding customers. **Restate as:** a per-KLOC `rt.check` scaling
> curve **plus** an end-to-end loop target (generation + up to N checks), with the check budget measured
> against the **largest program the Slice-0 corpus actually produced** rather than a nominal 200 lines.
> The corpus will tell us what real generated size is today, and the curve keeps the number honest as
> that grows. Interpreter throughput is otherwise already accepted
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
   reference? (A wedge that only works on frontier-strong models has a cost floor.) Also open until the
   corpus is committed: the exact per-task oracle content — the *method* (fixture assertions or fixed judge
   protocol, partial-credit-is-fail tie-break) is now pinned in §9 and must be pre-registered before any
   measurement run; only the per-task specifics remain to author.
3. **Freeze vs fork** — does the Embedded profile freeze constrain the ASI research language, or fork a
   subset grammar? Default: subset-freeze with the research track a strict superset; revisit at Slice 2.
4. **Pricing shape** — per-run metering (the budget meter already counts the billable unit) vs seat/SDK
   license. Note the meter's µ$ accounting was built for exactly this.
5. **Does the tool registry subsume `@[contained]`?** Long-term the host-imposed ceiling + typed tool
   registry may be the *primary* containment surface (attributes remain for hand-written Axon). If so, that
   is an I-11 evolution to spec properly, not slip in. **[elevated 2026-07-31 — this is now a Slice-2
   BLOCKER, not a background question.]** The answer determines whether the frozen public artifact is the
   registry or the attribute, and whether the ceiling's data model is an effect row or the legacy
   fs/net/exec triple (§4 note, §9 Slice-2 sequencing blocker). Resolve **before** the freeze; a wrong
   freeze here is hard truth #3 with the wrong object chained.
6. **[added 2026-07-31] Does R38 own the information-flow answer, or defer it to R25?** §7 N-1 concedes
   the capability card is channel-level while the ICP's pain (§5: tools that "mutate records, move money,
   or send communications") is data-level, and `R25-information-flow-monitor` already owns the general
   problem — but R25's own 2026-07-31 corrections show its real-runtime mediation seam is an unbuilt
   L1.5 I/O-proxy (a protocol + axon-core change, not wiring). R38 has a genuinely easier special case:
   the tool graph is **small, static, and host-declared**, which makes a coarse non-interference lattice
   over registered tools decidable where general IFC is not. Options: (a) R38 ships the narrow tool-graph
   version as Slice 2b and R25 generalizes later; (b) R38 waits on R25 and ships the channel-level card
   with N-1 stated (the status quo); (c) the two specs merge the value-flow surface. **Do not answer this
   by omission** — the current spec answers (b) by silence, which is the option that quietly ships the
   overclaim §1's box warns about. Decide alongside §12 Q1.
7. **[added 2026-07-31] What is the approvable unit — a run or an envelope?** §7 N-3 states the expiring
   assumption plainly: R38 depends on a human meaningfully reviewing every generated artifact, and that
   dependency degrades exactly as generation gets cheaper. §9 Slice 1 proposes envelope-approval with
   diff-triggered re-consent and makes the fresh-decision fraction a measured criterion, but the deeper
   question is strategic and unanswered: **at product scale, is the human approval step a real control or
   a legitimacy ritual?** If the honest answer is the latter, the ceiling and the envelope are the entire
   security story and N-1 stops being a footnote. This bears directly on how §1's card is *marketed*, so
   it is a founder question, not an implementation one.
8. **[added 2026-07-31] How much Proof can a generator be asked to carry, and when do we start asking?**
   §7 N-2: the Proof pillar is landed and untouched by R38. The §9 Slice-0 contract-delta measurement
   gives the first data point. Open: is the answer "demand contracts from day one" (better failure mode,
   lower pass rate today), "demand them once the delta closes" (needs a re-measurement cadence — the
   Slice-2 generator-conformance suite provides one), or "never, the card stays channel-level"?

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
