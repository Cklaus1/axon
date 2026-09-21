---
id: CX-35
title: "Reflex Serving and Model-State Runtime: deployment topology, admission and bootstrap"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-05", "CX-06", "CX-13", "CX-16", "CX-34"]
first_stage: M2
implementation_evidence: []
---

# CX-35 — Reflex Serving and Model-State Runtime

## 0. What this document is, and where it lives

This is a **new CX spec written in the live repository**, at
`governance/cortex-v015/CX-35-reflex-serving.md`. It is deliberately NOT placed
inside the vendored package, because `scripts/cortex_package_gate.sh` verifies
`SHA256SUMS_v0_15.json` **in both directions** — "every listed file hashes to
its recorded digest, AND no unlisted file has appeared under the package root"
(`scripts/cortex_package_gate.sh:22-27`). A CX-35 written under
`docs/axon_cortex_v0_15/axon-cortex-build-v0_15/specs/` would break the intake
gate on its first run. It follows that:

* `specs/INDEX.md` and `spec_manifest.json` inside the package cannot be
  updated to list CX-35. The package's spec set stops at CX-34
  (`docs/axon_cortex_v0_15/axon-cortex-build-v0_15/specs/INDEX.md:93`).
* CX-35 is therefore an **Axon-side proposal about a third-party package**,
  the same status the rest of `governance/cortex-v015/` carries. See §13 X-001.

**Nothing in this specification is implemented.** Zero lines of Reflex code
exist in this repository (§2). Every normative statement below is a design
commitment, not a description of behaviour. Where a design commitment depends on
a fact I could not establish, the fact is marked **UNVERIFIED** and the
commitment is marked **UNKNOWN**, not filled in to look finished.

---

## 1. Purpose and the boundary this spec owns

The v0.15 package already owns the *decision* layer: a backend-neutral Reflex
ABI, the Reflex Runtime / Reflex Model split with `StateHandle`
(`specs/CX-05-reflex-inference.md:83-96`), dynamic candidate sets
(`build/REFLEX_BACKEND_BAKEOFF.md:35-50`), the four-family bakeoff
(`build/REFLEX_BACKEND_BAKEOFF.md:22-33`), probability provenance
(`build/REFLEX_CONFORMANCE.md:46-58`), calibration and abstention
(`specs/CX-06-routing-calibration.md`), the conformance contract and
`BackendFeatureManifest` (`build/REFLEX_CONFORMANCE.md:60-79`), external
dependency adoption (`build/DEPENDENCY_ADOPTION.md`), scheduler integration and
the shared capability registry (`specs/CX-34-…:26-48`), and the explicit
statement that a provider-native TypeSafe/Jev backend is *one* backend the core
does not depend on (`build/REFLEX_BACKEND_BAKEOFF.md:33`).

What no package document owns is the **serving topology**: where a Reflex
backend physically runs relative to the caller, what changes when it moves, and
how the first working instance is bootstrapped from nothing. CX-35 owns that
boundary and one consequence of it:

> **MiCode talks to an Axon Reflex client. It never links, imports, configures
> or names a Reflex backend implementation, and in particular never depends on
> TypeSafe/Jev.**

That is not a new architectural idea in this repository; it is the pattern
already in production at the adjacent boundary. `crates/cortex-policy-adapter`
exists "as a separate executable rather than a library MiCode links so that
neither repository depends on the other. MiCode knows the protocol; this knows
Cortex" (`crates/cortex-policy-adapter/src/main.rs:6-9`). CX-35 is the same
move for inference, and §5 reuses that executable's hard-won failure conventions
rather than reinventing them.

---

## 2. Reconnaissance — what exists live, with citations

### 2.1 Reflex: nothing

Searched `crates/`, `governance/specs/` and `spec/` for `reflex`, `StateHandle`,
`logit`, `calibrat`, backend registry:

| term | live hits |
|---|---|
| `reflex` | 2, both English prose in comments — `crates/axon-core/src/checker.rs:4319` ("the Rust and Java reflexes"), `crates/axon-core/tests/cli_run.rs:5600` |
| `StateHandle` | 0 |
| `logit` | 0 |
| `calibrat*` | 0 in `crates/**/*.rs` |
| backend *registry* | 0 — `backend` occurs only as codegen/LLVM/gfx backend naming |
| `typesafe` / `jev` / `openrouter` | `crates/axon-ai/src/lib.rs` and its README only, as comments naming OpenAI-compatible gateways |

**Verdict: greenfield.** No Reflex ABI, no backend registry, no model-serving
code or spec exists. Any statement that some part of this is "already there" is
false.

### 2.2 What exists that CX-35 must build on rather than duplicate

| need | live owner | evidence class |
|---|---|---|
| cross-process control boundary with a versioned protocol | `crates/cortex-policy-adapter/src/main.rs:47` (`PROTOCOL_VERSION: u64 = 1`), `:154` (mismatch = infrastructure failure) | **a gate runs it** — `scripts/gate.sh:121`, unconditional |
| infrastructure-failure vs decision distinction | same, `:50-55` — exit 2 with **no decision on stdout**; "printing a refusal here would report a broken adapter as a strict policy" | same gate |
| bounded, strictly-parsed untrusted input | same, `:117` (`MAX_REQUEST = 1<<20`), `:148` (`axon_cortex::parse_strict`, refusing duplicate JSON keys — the measured `principal:intruder`/`principal:agent` case at `:133-147`) | same gate |
| authority that does not survive its state | same, `:104-110` — `--grant-snapshot` required; inferring it from the request made `StaleSnapshot` unreachable | same gate |
| LLM provider adapters over two wire codecs | `crates/axon-ai/src/lib.rs:223` (`enum Provider`), `:240-260` (base URL / endpoint per provider), `:507` (`complete_with_model_usage` → `(text, tokens)`) | source reads that way; `axon-ai` unit tests |
| egress pinning for model calls | `crates/axon-ai/src/lib.rs:116` (`pin_net_allowlist`), called from `crates/axon-core/src/interp.rs:2195` | source + the `.env`-exfiltration case recorded at `crates/axon-ai/src/lib.rs:66-84` |
| deterministic model replay by `(prompt, model)` | `crates/axon-core/src/interp/provenance.rs:489-499`, dispatch at `crates/axon-core/src/interp/builtins.rs:5633` | source; interpreter-only (see §4) |
| mock model responses | `crates/axon-ai/src/lib.rs:364` (`ai_mock_enabled`), `crates/axon-core/src/interp.rs:3571` | source |
| per-principal token/cost metering | `crates/axon-core/src/kernel.rs:689` (`LlmGateway`), `:717` (`call_cost`) | source |
| host seam that record/replay already intercepts | `crates/axon-core/src/host.rs` (`AxonHost`) — `IMPLEMENTATION_MAP.md` §2 names it: "**extend here, do not add an AIR crate**" | reconnaissance verdict |
| local HTTP service precedent | `crates/axon-web/src/main.rs:9-11` — binds `127.0.0.1:{port}` | source |
| gate-execution record for CX gates | `governance/cortex_gate_execution_registry.json`, `gates: []`, validated unconditionally (`scripts/gate.sh:92` per `IMPLEMENTATION_MAP.md` §4) | a gate runs it |

### 2.3 The transport facts that constrain §7

* `AxonHost::http_get` / `http_post` **default to denied** and return
  "`requires the asi-runtime feature or a network-capable host`"
  (`crates/axon-core/src/host.rs:94-110`). The real implementations are
  `#[cfg(feature = "asi-runtime")]` (`crates/axon-core/src/host.rs:269,292`).
* `asi-runtime` appears in **zero** files under `scripts/` and
  `.github/workflows/` (grep, both directories, no hits). CLAUDE.md states the
  same: a feature "**NOTHING in `scripts/` or `.github/workflows/` compiles**".
* Consequence, and it is load-bearing for this spec: **an `.ax` program in any
  gated or CI build cannot make an outbound HTTP call at all.** A Reflex client
  reachable from Axon source over HTTP is therefore not merely unbuilt, it is
  uncompiled in every configuration this repository verifies. See §7 and §13
  X-004.
* `crates/axon-web` has zero occurrences of `auth`, `token`, `tenant` or
  `principal` in `server.rs` or `main.rs`. The existing local HTTP precedent is
  **unauthenticated**, which `build/DEPENDENCY_ADOPTION.md:58-60` explicitly
  disqualifies from protected-runtime adoption.

---

## 3. The central design constraint

The naive version of this spec asserts that Embedded, LocalSidecar and
RemoteService have "the same semantics". **That assertion is the defect this
repository spent a day removing**, and CX-35 refuses to reintroduce it.

The live finding, recorded in `governance/cortex-v015/DISCREPANCIES.md`: the
interpreter and the native engine both presented as honouring the same ambient
controls, and native silently ignored five of them — the effect ceiling, the AI
token budget, the replay cache, the record journal and the RNG seed. Two
concrete measurements from that register:

```
axon build c.ax -o cbin                 # no ceiling set — emits happily
AXON_ALLOWED_EFFECTS=Pure ./cbin        # every effect performed, exit 0
AXON_ALLOWED_EFFECTS=Pure axon run c.ax # exit 8
```
(`DISCREPANCIES.md`, "CORRECTION (post-audit)")

and, for `AXON_AI_REPLAY`: "interp served from cache with no key; native ignored
the cache and reached for the live API" (`DISCREPANCIES.md`, post-audit closure
table, D-010) — a **live billed model call while the operator believed the run
was replayed**. That is precisely the failure a Reflex serving topology can
reproduce at a larger blast radius, because a sidecar or a remote service is one
more engine with its own idea of what a declared control means.

The remedy that worked was **not** making the engines identical. Three parts:

**(a) A per-control, per-engine matrix with a CLOSED state set.** From
`scripts/completeness.py:154-156`:

```python
CONTROL_STATES = {"enforced", "explicitly-refused", "not-applicable",
                  "unknown", "silently-ignored"}
```

`silently-ignored` is named on purpose. `AXON-COMPLETENESS.json`'s
`legend.controls` states why: it "is the defect state and is nameable on purpose
— a control an engine neither honours nor refuses is the shape that produced
every divergence found so far."

**(b) Explicit refusal over silent approximation.** An engine that cannot
faithfully implement a declared control refuses. What that buys, and what it
does not, is stated in the register and is quoted here because CX-35 inherits
the same limit verbatim: "Native now refuses five controls it cannot honour.
That makes the safety BOUNDARY equivalent across engines … and it does not make
the CAPABILITY equivalent." Every such matrix cell reads `explicitly-refused`,
never `enforced`.

**(c) A gate that will not let a thing be called complete while a cell is
unknown or silently-ignored** (`scripts/completeness.py:167-175`):

```python
if c.get("status") == "resolved":
    bad = {k: v for k, v in eng.items()
           if v in ("unknown", "silently-ignored")}
```

CX-35 inherits all three. The package's own CX-13 already states the principle
for tiers — "**G13-tier:** unsupported/native profiles cannot inherit
interpreter-only guarantees"
(`specs/CX-13-os-runtime.md:65`) — so this is an extension of a package rule,
not a foreign import.

---

## 4. Invariant set — what is identical in all three modes

These properties MUST hold identically in Embedded, LocalSidecar and
RemoteService. A backend that cannot satisfy all of them is **not admitted in
any mode**; there is no "admitted only embedded" relaxation, because the whole
value of the client boundary is that a caller need not know the mode.

| id | invariant | why it can really be invariant |
|---|---|---|
| **I35-1 Decision determinism under a pinned revision** | Given identical `(canonical_encoded_state, question, candidate manifest incl. `order_digest`, sampling policy, model revision, adapter revision, tokenizer revision, preprocessing manifest)`, the selected candidate and any reported distribution are identical. A backend whose sampling is stochastic declares that in its `BackendFeatureManifest` and pins a seed; a backend that can pin neither declares `determinism: none` and is refused for protected use. | Determinism is a property of the computation, not of where the process runs. Transport cannot make it true or false — it can only fail, which is I35-6. |
| **I35-2 Provenance record** | Every response carries probability origin from the closed CX-05 set (`NativeOptionLogit`, `SequenceLikelihood`, `GeneratedEstimate`, `EntropyDerived`, `EnsembleEstimate`, `CalibratedEmpirical`, `ProviderReportedDistribution`, `Unavailable`), the `EffectiveInputReceipt`, all four revision identities, and the candidate `order_digest`. A backend producing only a label MUST NOT invent a probability (`specs/CX-05-reflex-inference.md:25`). | It is a reporting obligation on the adapter, which exists in every mode. |
| **I35-3 Calibration and abstention contract** | `NONE` / `OBSERVE_MORE` / `ESCALATE` / `BLOCKED` and `NoneSuitable` / `NeedMoreObservation` / `UnauthorizedCandidate` / `InferenceUnavailable` remain distinct outcomes and are never collapsed into a forced choice or into a transport error. A calibration artifact is valid only for the declared `(model revision, encoder revision, candidate construction policy, order policy)` tuple. | Same argument as I35-2: it is a typed-result obligation. Transport may add ways to fail, never a way to answer. |
| **I35-4 Principal isolation** | A request is decided under exactly one principal. No cache entry, `StateHandle`, KV block, warm prefix or calibration artifact is reachable across principals. Cross-principal reuse is a refusal, never a cache miss. | Required in all modes, and the strictest mode's rule is the only safe common rule. Embedded has the *most* ways to violate it (one address space), which is why it is invariant rather than mode-dependent. |
| **I35-5 No authority crosses the Reflex boundary** | A Reflex response is DATA. It never authorizes, names, or triggers an effect. Acting on it goes through the existing executor/grant path — `CortexAction` + `EditGrant` (`crates/axon-cortex/src/action.rs:100`, `crates/cortex-policy-adapter/src/main.rs:181-200`). Preserves `CX00-R1` (`specs/CX-00-system-contract.md:27`) and I-11/I-12 (`governance/ARCHITECTURE_INVARIANTS.md:56-61`). | It is a refusal to wire something, and a refusal is portable. |
| **I35-6 Infrastructure failure is not a decision** | A transport, load, admission, budget or version failure produces a *failure* outcome, distinguishable by the caller from an abstention and from a decision. This is the `cortex-policy-adapter` rule verbatim: exit non-zero with **no decision on stdout**, because "a malformed request means nothing was decided" (`crates/cortex-policy-adapter/src/main.rs:50-55`). | Purely a protocol obligation. |
| **I35-7 Revision pinning for the life of an episode** | Model / adapter / tokenizer / encoder / calibration / protocol revisions are pinned per episode; a change starts a new epoch (`CX00-R7`, `specs/CX-00-system-contract.md:33`). A backend that can only offer a floating alias is admitted for research only and its outputs are marked non-reproducible (`build/DEPENDENCY_ADOPTION.md:66-68`). | The pin is recorded by the client, which exists in all modes. |

### 4.1 What is deliberately NOT invariant

Latency; tail latency; cancellation semantics; state/KV residency and lifetime;
failure modes; crash isolation; memory pressure coupling; GPU availability;
warm-cache hit rate; upgrade atomicity; and the observability channel. Asserting
any of these as mode-invariant would be the "same semantics" claim §3 rejects.
They are §5's matrix.

---

## 5. Mode-dependent matrix

Modes:

* **Embedded** — the backend executes in the caller's process (a linked
  library, or an in-process mock/deterministic backend).
* **LocalSidecar** — a separate process on the same host, reached over a local
  IPC seam. The live precedent is `cortex-policy-adapter`: one JSON request on
  stdin, one JSON decision on stdout, exit 2 with no decision for
  infrastructure failure (`crates/cortex-policy-adapter/src/main.rs:1-5,50-55`).
* **RemoteService** — a network service, possibly a third-party provider
  endpoint. Includes the Phase-1 TypeSafe/OpenRouter adapter.

**How to read the cells.** Each cell is the NORMATIVE REQUIRED state for that
capability in that mode, in the closed set of `scripts/completeness.py:154-156`:
`enforced` / `explicitly-refused` / `not-applicable` / `unknown` /
`silently-ignored`. `silently-ignored` is never a permitted requirement — it
appears in the legend only so that a measured cell can be recorded as the defect
it is. `unknown` is used where I could not establish what the mode can do, and
those cells are enumerated again in §12.

**These cells describe requirements, not measurements.** Nothing is implemented
(§2.1), so the *verification* state of every cell is `unknown` today. Conflating
"specified" with "verified" is the collapse `scripts/cortex_package_gate.sh`
exists to prevent, and this paragraph is the guard against committing it here.

| capability | Embedded | LocalSidecar | RemoteService | note |
|---|---|---|---|---|
| decision determinism (I35-1) | enforced | enforced | enforced | invariant; a mode that cannot must refuse admission |
| provenance record (I35-2) | enforced | enforced | enforced | invariant |
| abstention/calibration contract (I35-3) | enforced | enforced | enforced | invariant |
| principal isolation (I35-4) | enforced | enforced | enforced | invariant; hardest in Embedded |
| no authority crosses (I35-5) | enforced | enforced | enforced | invariant |
| infra-failure ≠ decision (I35-6) | enforced | enforced | enforced | invariant |
| revision pinning (I35-7) | enforced | enforced | enforced | invariant; RemoteService records the *resolved* identity the provider returns |
| **hard latency bound** | explicitly-refused | explicitly-refused | explicitly-refused | no mode may promise one. Refusal, not silence: a caller asking for `deadline_ms` gets a *best-effort* flag plus I35-6 failure on expiry, never a guarantee |
| **cancellation actually stops work** | explicitly-refused | enforced | unknown | Embedded: a synchronous in-process call has no seam to interrupt. LocalSidecar: process-group kill is the live remedy from D-008 — `setsid` + `killpg`, because `child.kill()` left grandchildren alive (`DISCREPANCIES.md` D-008, measured). RemoteService: provider-dependent; most HTTP providers keep billing after a client disconnect — **UNVERIFIED for any specific provider** |
| **cancellation stops billing** | not-applicable | enforced | unknown | see above; a mode that cannot must say so in its manifest so CX-06 budget accounting does not silently over-credit |
| **crash isolation from the caller** | explicitly-refused | enforced | enforced | Embedded: a backend segfault or OOM takes the caller down, and the spec refuses to pretend otherwise |
| **memory pressure isolated from caller** | explicitly-refused | enforced | enforced | Embedded shares an allocator and an address space |
| **KV / prefix state residency across calls** | enforced | enforced | unknown | RemoteService: `StateHandle` may be EMULATED per-request (`build/REFLEX_RUNTIME.md:55-60`), and the receipt must let a benchmark tell real reuse from API-shaped batching |
| **`StateHandle` survives process restart** | explicitly-refused | explicitly-refused | unknown | a handle is bound to an expiry and refuses after any binding change (`specs/CX-05-reflex-inference.md:90`); surviving a restart would require durable KV nobody has specified |
| **GPU placement controllable by the caller** | enforced | enforced | explicitly-refused | RemoteService: the caller cannot place a provider's weights; requesting placement there is a refusal, never a hint that is dropped |
| **CPU/GPU residency observable** | enforced | enforced | unknown | provider APIs generally do not report it |
| **model load / unload on demand** | enforced | enforced | explicitly-refused | RemoteService: load is the provider's; `unload` refuses rather than returning success having done nothing |
| **hot-swap without dropping in-flight requests** | explicitly-refused | enforced | unknown | Embedded: swapping a linked backend under a live call is not offered. LocalSidecar: two processes + drain. Remote: provider deployment semantics, opaque |
| **batching across principals** | explicitly-refused | explicitly-refused | explicitly-refused | I35-4; a shared batch is a shared cache with extra steps |
| **batching within one principal** | enforced | enforced | enforced | batch identity by `QuestionId`, never array position (`build/REFLEX_CONFORMANCE.md:24,44`) |
| **concurrency limit enforced** | unknown | enforced | enforced | Embedded depends on the caller's own scheduler — the live cooperative scheduler is interpreter-only (`crates/axon-core/src/kernel.rs`), so the answer differs per engine and is not established |
| **per-principal budget/metering enforced at the seam** | enforced | enforced | enforced | `LlmGateway` is the live shape (`crates/axon-core/src/kernel.rs:689,717`). Note it is **interpreter-only** — see §13 X-003 |
| **egress host pinning** | enforced | enforced | enforced | `pin_net_allowlist` (`crates/axon-ai/src/lib.rs:116`) is the live mechanism; a RemoteService adapter that resolves its host from ambient config re-opens the measured `.env` exfiltration path (`crates/axon-ai/src/lib.rs:66-84`) |
| **transport authentication** | not-applicable | unknown | enforced | LocalSidecar over stdin/stdout inherits process-spawn authority and needs none; over a loopback socket it does, and no live component does this — `axon-web` binds `127.0.0.1` with no auth (`crates/axon-web/src/main.rs:9-11`, zero auth hits in `server.rs`) |
| **request payload bounded** | enforced | enforced | enforced | `MAX_REQUEST = 1<<20` is the live convention (`crates/cortex-policy-adapter/src/main.rs:117`); Reflex payloads are larger and the bound must be re-derived, not copied |
| **strict parse (duplicate keys refused)** | not-applicable | enforced | enforced | `axon_cortex::parse_strict` (`crates/cortex-policy-adapter/src/main.rs:148`); the measured duplicate-key case at `:133-147` is why |
| **replay of a decision without a live call** | unknown | unknown | unknown | `AXON_AI_REPLAY` exists and is **interpreter-only**; native ignored it and made a live billed call (D-010). No Reflex equivalent exists. This row is the single most dangerous one in the table and is **UNKNOWN in all three modes** |
| **observability: per-call latency breakdown** | enforced | enforced | unknown | prefill vs incremental question vs candidate cost (`build/REFLEX_RUNTIME.md:87-97`); a remote provider usually reports one number |
| **observability: token/cost accounting** | enforced | enforced | enforced | live precedent returns `(text, tokens)` (`crates/axon-ai/src/lib.rs:507`); SDK-side retries must be counted against the same parent budget (`build/DEPENDENCY_ADOPTION.md:88`) |

### 5.1 The refusal rule

> A mode that cannot honour a declared control MUST refuse — at admission if the
> incapacity is static, at call time if it is dynamic — with a distinct, named
> outcome. It MUST NOT degrade quietly, approximate, or return success having
> done nothing.

Two refusal points, because they fail differently:

* **Admission-time refusal.** The backend's `BackendFeatureManifest`
  (`build/REFLEX_CONFORMANCE.md:60-79`) is validated against the deployment's
  required-control set before any dispatch. A manifest that omits a control is
  treated as *not supported*, never as supported — absence is not a passing
  gate (`CX00-R4`, `specs/CX-00-system-contract.md:30`). The refusal names the
  control and the mode.
* **Call-time refusal.** A request carrying a control the active backend cannot
  honour (`deadline_ms` to a backend with no cancellation; `placement: gpu` to
  RemoteService; `reuse_state: required` to an emulating adapter) is refused
  with the control named. It is *not* served with the control dropped.

**Which error surface this uses — UNVERIFIED, and deliberately unresolved.**
`specs/CX-00-system-contract.md:46` forbids this package from allocating new
Axon numeric diagnostic or exit codes: "Represent errors using symbolic Cortex
categories plus phase, retryability, side-effect status and evidence references
… Map categories into existing namespaces only after the ledger is inspected."
The ledger has now been inspected — `governance/EXIT_CODES.md` records 0–15 and
101 claimed, "**Next free: 16**" — so a mapping is *possible*, but choosing one
is an owner decision, not a spec author's. Three candidate surfaces, with what
each would cost:

1. **Symbolic category only**, carried in the response envelope, no process exit
   code. Works for Embedded and for a library client; says nothing to a
   supervisor that branches on a number.
2. **Reuse exit 2** ("a refused run configuration",
   `governance/EXIT_CODES.md`), which is exactly what `cortex-policy-adapter`
   already does for infrastructure failure (`:54`). Consistent, and makes a
   refused control indistinguishable from a malformed request.
3. **Claim exit 16**, the next free code, for "Reflex control refused". Requires
   a `reserves:` entry and a same-commit ledger row per `EXIT_CODES.md`, and per
   `governance/SPEC_TEMPLATE.md` the R-spec `spec-meta` convention.

CX-35 does not pick. **UNKNOWN until the owner rules.** What it does require is
that whichever surface is chosen, the outcome is *nameable and distinct from a
decision and from an abstention* (I35-6).

### 5.2 Where this matrix should live

Not in a new file. `governance/cortex-v015/DISCREPANCIES.md` D-007 records that
four governance registries already exist and names consolidation, not extension,
as the live job; `AXON-COMPLETENESS.json`'s `legend.controls` says the control
matrix "lives here rather than in a new file because this repository already
carries four governance registries and D-007 records that as a problem, not a
pattern to extend."

**Proposal:** when Reflex serving is implemented, the rows above become rows in
`AXON-COMPLETENESS.json`'s `controls` array, with the engine columns reinterpreted
as modes. **Blocker, verified by reading the gate:** `scripts/completeness.py:157`
hardcodes `ENGINES = {"interpreter", "native", "wasm", "guest"}` and fails any
control row that does not state all four. A mode-keyed row cannot be added
without a generator change. The reverse direction is fine — `completeness.py:186-194`
requires every *env-registry* var to have a control row, but permits control rows
that are not env vars. See §13 X-002.

---

## 6. Canonical `ReflexBackend` interface

Backend-neutral, mode-neutral, and deliberately smaller than the surface the
runtime exposes to Cortex: the runtime owns scheduling, budgets, cache lifetime
and response mapping; the model owns score production
(`specs/CX-05-reflex-inference.md:92`).

```text
ReflexBackend {
  // Negotiation. Called before any dispatch; the result is validated against
  // the deployment's required-control set (§5.1). Cheap, side-effect-free.
  manifest() -> BackendFeatureManifest

  // Liveness and capacity. Separate from manifest() because one is identity
  // and the other is state (§8).
  health()    -> Health
  readiness() -> Readiness
  models()    -> [ModelDescriptor]          // discovery; identity + revision

  // Lifecycle. unload() REFUSES where it cannot act (RemoteService), rather
  // than returning success having done nothing.
  load(ModelRef, Placement)   -> Result<ModelHandle, Refusal>
  unload(ModelHandle)         -> Result<(), Refusal>

  // The two-stage optimization interface from CX-05 §v0.6. A backend with no
  // reusable state EMULATES encode_state in one call and says so in the
  // receipt, so a benchmark can tell real reuse from API-shaped batching
  // (build/REFLEX_RUNTIME.md:55-60).
  encode_state(StateInput, PreprocessingPolicy, PrincipalScope)
      -> Result<StateHandle | EmulatedStateHandle, Refusal>
  decide_batch(StateHandle, QuestionBatch, CallControls)
      -> Result<DecisionBatchResult, Refusal>
  release_state(StateHandle) -> Result<(), Refusal>

  // Cooperative cancellation. A backend that cannot interrupt returns
  // Refusal::ControlUnsupported at ADMISSION for `cancellable`, not a no-op
  // here (§5.1).
  cancel(RequestId) -> Result<(), Refusal>
}
```

`CallControls` carries exactly the controls §5's matrix rows name —
`deadline_ms`, `cancellable`, `reuse_state: required | preferred | forbidden`,
`placement`, `max_concurrency`, `budget_ref`, `replay_mode` — so that the set of
things a backend can refuse is enumerable rather than open. Adding a control is
a matrix row plus a manifest field, in the same change.

`Refusal` is a closed enum. It must at minimum separate: `ControlUnsupported`,
`RevisionMismatch`, `StaleStateHandle`, `CrossPrincipal`, `BudgetExhausted`,
`PayloadTooLarge`, `MalformedRequest`, `BackendUnavailable`, `NotAdmitted`. The
live `Refusal` in `crates/axon-cortex/src/runner.rs` is the naming precedent
(`WrongPrincipal`, `StaleSnapshot`, `NotInCatalog` — cited at
`crates/cortex-policy-adapter/src/main.rs:190-196`); whether the Reflex one
should extend it or be a sibling is **UNKNOWN** (§12).

---

## 7. Transport-neutral request/response contract

The wire contract is one versioned envelope, identical in all three modes. The
mode chooses a carrier, never a schema.

```text
ReflexRequest {
  protocol_version,            // integer; mismatch = infrastructure failure
  request_id,
  principal,                   // I35-4; one principal per request, no default
  episode_id, epoch_id,        // I35-7
  pins { model_revision, adapter_revision, tokenizer_revision,
         encoder_revision, calibration_ref? },
  state { canonical_encoding_version, state_digest, payload | state_handle },
  questions[ { question_id, family, candidate_manifest, order_digest,
               dependency_class } ],
  controls: CallControls,
  budget_ref
}

ReflexResponse =
    Decision { request_id, results: Map<QuestionId, QuestionResult>,
               effective_input_receipt, backend_manifest_ref,
               resolved_identity, usage, timings }
  | Refusal  { request_id?, refusal, control?, detail }
```

Rules taken directly from live experience rather than invented:

* **Protocol version is checked first and a mismatch is an infrastructure
  failure**, not a refusal and not a best-effort translation
  (`crates/cortex-policy-adapter/src/main.rs:154-157`).
* **Strict parsing.** Duplicate keys refuse. The measured case is in the live
  source: a request naming `principal: intruder` and `principal: agent` was read
  one way by a reviewer and decided the other way by `serde_json`'s last-wins
  (`crates/cortex-policy-adapter/src/main.rs:133-147`).
* **Bounded payload**, with the bound stated and the overflow an infrastructure
  failure, because an unbounded read fails as an allocation abort — "a non-2 exit
  with no decision and no message, which is the one outcome this program is built
  to never produce" (`crates/cortex-policy-adapter/src/main.rs:112-116`).
* **No defaults for identity fields.** `principal`, `epoch_id` and the pins are
  required; the live lesson is that defaulting a grant field made its refusal
  *unreachable* (`crates/cortex-policy-adapter/src/main.rs:88-110`).
* **The workspace does not cross.** As with the policy adapter, a Reflex backend
  receives a canonically encoded observation projection and candidate manifest,
  not raw repository bytes — "handing it file contents would make it a second
  filesystem agent" (`crates/cortex-policy-adapter/src/main.rs:20-23`).

**Carriers.** Embedded: a direct call, same structs. LocalSidecar: one JSON
object in on stdin, one out on stdout, per invocation — the existing, gated
pattern. RemoteService: HTTP + JSON.

**The HTTP carrier is unavailable from Axon source in every gated build** (§2.3):
`http_get`/`http_post` are `#[cfg(feature = "asi-runtime")]`
(`crates/axon-core/src/host.rs:269,292`) and no file under `scripts/` or
`.github/workflows/` names that feature. So RemoteService, reached from `.ax`,
is not merely unimplemented — it does not compile in anything this repository
verifies. A Rust-side client inside `axon-cortex` avoids that (the crate already
reaches the network through `axon-ai`), and is the recommended Phase-1 shape.
**UNVERIFIED:** whether `axon-cortex`'s `ai` feature — the one `scripts/gate.sh`
does build (`:130-133`) — transitively provides a usable HTTP client for a
non-`axon-ai` endpoint. I did not build anything to check, per instruction.

---

## 8. Health, readiness and model discovery

Three separate questions that a single `/health` conflates, and the conflation
has a known failure shape in this repository: a check that returns green while
testing nothing.

| probe | question | wrong answer costs |
|---|---|---|
| `health()` | is the process alive and its own invariants intact? | a restart loop, or a hung process treated as live |
| `readiness()` | can it accept a request *now* — model resident, queue below limit, budget available? | requests admitted into a queue that will time out, burning a budget for nothing |
| `models()` | which model identities and revisions are resident, at what revision, on what placement? | the pin in the request and the weights in memory diverge silently — the I35-7 failure |

Requirements:

* `readiness()` MUST distinguish *not ready yet* (loading) from *not ready ever*
  (unadmitted, revoked, refused control). A client retrying the second is
  burning budget on a decided no.
* `models()` returns the **resolved** identity, not the requested one. For a
  RemoteService provider that resolves an alias, the resolved string is what is
  recorded, per `build/DEPENDENCY_ADOPTION.md:66-68`.
* A backend that cannot answer a probe returns `unknown` for that probe. It does
  not return healthy. This is the same `CX00-R4` rule as §5.1.
* Probes are NOT a substitute for admission. A healthy unadmitted backend is
  still refused (`G34-authorized-candidates`,
  `specs/CX-34-…:138` — candidates are rechecked at dispatch, not only at
  enumeration).

---

## 9. Model lifecycle, shared state and residency

**Load/unload.** `load(ModelRef, Placement)` is explicit. An implicit
load-on-first-use is forbidden for protected use: it makes the first request's
latency unbounded and hides which revision was resolved. `unload` refuses in
RemoteService (§5) rather than reporting success.

**Shared state.** `StateHandle` semantics come from CX-05 and are not restated
here except for what serving adds:

* A handle is bound to `(state_digest, model/tokenizer/adapter revision,
  preprocessing manifest, principal scope, expiry)` and refuses after any
  binding change (`specs/CX-05-reflex-inference.md:90`). Serving adds: it is
  also bound to the **backend instance**. A handle minted by one sidecar process
  is invalid at another, and after a restart of the same one.
* `reuse_state: required` is a control a backend may refuse (§6). An adapter
  that emulates reuse MUST report emulation in the receipt; a benchmark that
  cannot distinguish real KV reuse from per-request repacking cannot measure the
  thing the bakeoff exists to measure (`build/REFLEX_RUNTIME.md:59-60`).

**KV residency and eviction.** Eviction policy, high-water marks and per-principal
KV quotas are mode-dependent and **UNKNOWN** for RemoteService. What is
invariant: eviction is never silent from the caller's perspective — a decision
computed after an eviction reports a cold receipt, so a latency regression is
attributable rather than mysterious.

---

## 10. Isolation, batching, concurrency and placement

**Tenant/principal isolation (I35-4).** One principal per request; no shared
mutable cache without an explicitly reviewed shared-cache policy
(`build/REFLEX_RUNTIME.md:48`). Embedded is the dangerous mode precisely because
sharing is free there: a process-global prefix cache is the default thing a
performance-minded implementer writes. The test is therefore adversarial rather
than affirmative — see G35-isolation.

**Batching.** Within one principal, keyed by `QuestionId`, never by array
position, with duplicate IDs refusing the request and unknown returned IDs an
error (`build/REFLEX_CONFORMANCE.md:24,44`). Across principals: forbidden in
every mode. Note that batching *changes the computation* when it joins
previously isolated prompts — the package calls that "a model/policy change, not
a compiler optimization" (`specs/CX-05-reflex-inference.md:106`), and it
re-enters calibration.

**Concurrency.** The limit is enforced at the seam that owns the budget. In
Embedded, the caller's scheduler is that seam, and the live cooperative scheduler
is interpreter-only — so the Embedded cell is `unknown` (§5) rather than
optimistic.

**Placement.** `Placement { cpu | gpu(device) | auto }` is a control, refusable
per §5.1. A refused placement is never downgraded to `auto`: silently running a
GPU-sized workload on CPU is the performance-analogue of the D-002 shape —
nothing fails, and the number the operator reads means something else.

---

## 11. Adapters, fallback, hot-swap, versioning, observability, admission

**Provider adapters.** Each is an `AdoptionManifest`
(`build/DEPENDENCY_ADOPTION.md:13-27`) with pinned source, transitive model /
tokenizer / encoder revisions, license and security profile, and — critically —
**visible SDK transformations**: an SDK that retries, normalizes probabilities,
truncates prompts, remaps candidates or silently falls back to another model is
evaluated *as part of that backend* (`build/DEPENDENCY_ADOPTION.md:62-64`).
Retries consume the parent budget and stay visible (`:88`). Live precedent for
the two-codec shape: `crates/axon-ai/src/lib.rs:223-260`.

**Local fallback policy.** A fallback is a **new dispatch** and independently
re-validates against the fallback backend's manifest
(`build/REFLEX_CONFORMANCE.md:79`). Additional CX-35 rules:

* A fallback never upgrades a refusal into an answer. If the primary refused for
  authority, budget, revision or control reasons, the fallback is not attempted.
  Only *availability* failures are fallback-eligible.
* A fallback result carries the fallback's own provenance and calibration
  reference. A calibration artifact fitted on the primary does not travel.
* "Fell back" is a first-class field in the response, not a log line. The live
  analogue: `axon trace --ai` already distinguishes `live/mock/replay/fallback`
  per call (CLAUDE.md, `axon trace --ai`), and that distinction exists because
  collapsing it is how a mock result gets read as a live one.
* An offline deployment declares `fallback: none`, and a primary failure is
  I35-6, not a quietly cheaper answer.

**Hot-swap.** Swapping a backend revision is an epoch boundary (I35-7). In-flight
requests either complete on the old revision or fail; they are never completed on
the new one, because the response's pins would then not describe the computation.
Embedded refuses hot-swap (§5); LocalSidecar drains and replaces the process.

**Versioning.** Three independently versioned things, and conflating them is the
predictable bug: the **protocol version** (wire envelope), the **adapter
revision** (the code translating to a backend), and the **model revision** (the
weights). A response records all three plus the resolved provider identity.

**Observability.** Per `build/REFLEX_RUNTIME.md:87-97`, timings are reported
separately for state encode/prefill, incremental question, incremental candidate,
scheduler overhead, and end-to-end; plus usage, cost, cache warmth, fallback
status and refusal counts by `Refusal` variant. **"One API request" is never
reported as "one forward pass" without backend evidence**
(`build/REFLEX_RUNTIME.md:99`). Refusals are counted by variant because an
aggregate error rate makes a control-refusal storm look like flakiness.

**Admission to the shared capability registry.** A Reflex backend becomes
schedulable only as a `CapabilityArtifact` in CX-34's registry
(`specs/CX-34-…:26-48`), through the existing admission path — "registry
publication never bypasses CX-11 evidence/admission" (`:122`). CX-35 adds the
serving-specific admission inputs: the deployment mode, the §5 matrix row-set the
backend satisfies, the controls it refuses, and its `AdoptionManifest`. The
scheduler's hard-constraint filter (`G34-hard-before-soft`, `:140`) MUST include
mode-dependent control satisfaction, so a capability that cannot be cancelled is
not selected for a deadline-bearing operation on a latency score.

**MiCode.** Per CX-16's export contract, MiCode may consume "Reflex backend
manifests/adapters" (`specs/CX-16-…:44`) — and "Approved by Axon" never means
"authorized in MiCode" (`:49`). CX-35's boundary statement is the operational
form of that: MiCode holds a client and a protocol version. It does not hold a
backend name, a provider key, or a Jev dependency.

---

## 12. Bootstrap sequence

Each phase states an **exit criterion that is a measurement**, because a phase
that exits on "it works" is how an unrun gate becomes a passed one.

| phase | build | exit criterion |
|---|---|---|
| **1. Reflex ABI → TypeSafe/OpenRouter adapter** | The envelope of §7, the `ReflexBackend` of §6, a deterministic in-process mock backend, and one RemoteService adapter over the existing OpenAI-compatible codec (`crates/axon-ai/src/lib.rs:223-260`; OpenRouter is named there as an OpenAI-compatible gateway). Family A, generative, `GeneratedEstimate` provenance only. | The mock and the provider adapter both pass one conformance suite. The provider's responses carry `GeneratedEstimate`, never `NativeOptionLogit` — a forged origin fails the suite (`G05-schema`). A destructive control (shuffled state) measurably changes the answer distribution, or the backend is flagged state-insensitive (`build/REFLEX_BACKEND_BAKEOFF.md:52-67`). |
| **2. Local direct-logit / encoder backend** | Family B/C as LocalSidecar. Real `NativeOptionLogit` / `SequenceLikelihood` provenance, multi-token candidate scoring by a declared method (`specs/CX-05-…:39`). | The same corpus runs on Phase-1 and Phase-2 backends through an unchanged client. Cancellation demonstrably stops work AND billing in LocalSidecar (the D-008 process-group remedy, differentially verified). |
| **3. Specialized Reflex models** | Family D, task-specialized heads over pinned bases; CX-22/CX-23 own the compilation, CX-35 owns only how they are served and admitted. | A specialized model is admitted through CX-11 and scheduled by CX-34 for its applicable operations, with a *tested rollback* to the previous revision. |
| **4. Axon-native General Reflex** | An Axon-owned general decision model behind the same ABI. | It competes in the bakeoff without a client change, and loses or wins on protected task evidence rather than on being ours. |
| **5. Latent / model-state ABI** | Extend `StateHandle` from an opaque optimization capability to a first-class latent-state object: durable, transferable between compatible backends, inspectable. | An honest statement of what this would require, not a plan: durable cross-process KV, a cross-backend state compatibility relation, and a security story for a latent that encodes observation content. **None of the three has a design today.** Phase 5 is a direction, and the earliest phase whose feasibility I would not assert. |

**Phase ordering is not negotiable in one respect.** Phase 1 exists to freeze the
ABI against a backend Axon does not control. Building Phase 2 first would let the
ABI accrete the local backend's conveniences, and the "backend-neutral" claim
would be untested at the point it is most load-bearing.

---

## 13. Acceptance gates

Stated in the package's gate style. None is implemented; per
`governance/cortex_gate_execution_registry.json`, each becomes a row in that
registry — naming a repo-owned script and an invoker that actually runs — at the
moment it is implemented, and not before.

**G35-invariant-parity:** the same decision corpus, the same pins, run through
Embedded, LocalSidecar and RemoteService backends of the same model revision,
produces identical selected candidates, identical provenance origins and
identical abstention outcomes. Divergence fails. Latency, timings and cache
receipts are compared but MUST NOT be asserted equal.

**G35-matrix-closed:** every capability row in §5 has a state from the closed set
for all three modes, and no cell reads `silently-ignored`. A row missing a mode
fails. A deployment declaring a control no admitted backend supports fails at
admission, not at call time.

**G35-refusal-distinct:** for each refusable control, a request carrying it
against a backend lacking it produces a named refusal that the client can
distinguish from (a) a decision, (b) an abstention, (c) a transport error. The
degradation path — serving the request with the control dropped — fails the gate.
Mutation check: remove the refusal and the gate must go red.

**G35-isolation:** two principals, interleaved requests against one Embedded
backend, with identical states and identical prompts. No cache entry,
`StateHandle` or KV block is reachable across them; a handle minted by principal
A and presented by B refuses. The affirmative version of this test passes on a
shared cache and is therefore not the test.

**G35-no-authority:** a Reflex response containing a fabricated grant, a
privileged-looking instruction, a tool call, or a path is data only. It produces
no effect and no capability. Repository text containing fake system/tool messages
cannot become adapter instructions (`G05-prompt-boundary`).

**G35-fallback-visible:** a primary failure that triggers a fallback is reported
as a fallback in the response, with the fallback's own provenance and no
inherited calibration reference. An authority, budget, revision or control
refusal never triggers a fallback.

**G35-pin-integrity:** a request whose pins do not match the resident model,
tokenizer, encoder or adapter revision refuses. A provider that resolves a
floating alias records the resolved identity; a protected-use request against an
unpinnable backend refuses.

**G35-nonvacuity:** the conformance suite asserts a positive count of executed
cases per mode. A mode whose cases all skip fails. (This repository has shipped a
harness that exited 0 having compared nothing; `scripts/cortex_package_gate.sh`
carries the same floor for the same reason.)

---

## 14. What could make this spec wrong

Each assumption, and the observation that would falsify it.

1. **That determinism (I35-1) is achievable across modes at all.** Falsified if a
   candidate backend's own numerics differ run-to-run on identical inputs — batch
   composition changing floating-point reduction order on GPU is the classic case.
   *Detection:* Phase-1 exit criterion runs the same request twice, and once with a
   different sibling batch, on each backend. If a backend cannot be deterministic,
   I35-1 must weaken to "deterministic under a declared tolerance", and every gate
   that compares decisions must gain a preregistered tolerance — which is a
   materially weaker spec and should be recorded as such rather than absorbed.
2. **That principal isolation is enforceable in Embedded.** Falsified by any
   candidate library with process-global cache state and no partition key.
   *Detection:* G35-isolation. If it fails, Embedded must be dropped as a mode for
   multi-principal deployments — not "mitigated".
3. **That the `cortex-policy-adapter` one-shot stdin/stdout shape scales to
   Reflex payloads.** A policy request is "a handful of short fields"
   (`crates/cortex-policy-adapter/src/main.rs:112-116`); an encoded observation
   plus a hundred candidates is not, and process-per-request destroys exactly the
   KV reuse §9 exists for. *Detection:* Phase-2 performance matrix. If per-request
   spawn dominates, LocalSidecar needs a persistent process and a socket — which
   pulls in the transport-authentication cell that is currently `unknown`.
4. **That RemoteService can honour I35-7 (revision pinning).** Falsified by a
   provider that silently updates weights behind a stable model string.
   *Detection:* a periodic canary — the same pinned request, the same seed,
   compared against a stored response digest. A drift means the pin was fiction,
   and every calibration artifact fitted against it is void.
5. **That the four-family bakeoff produces a winner worth serving.** Falsified if
   every backend is state-insensitive on the destructive controls
   (`build/REFLEX_BACKEND_BAKEOFF.md:52-67`). *Detection:* those controls, run
   before quality is interpreted. This would not make CX-35 wrong so much as
   premature — serving topology for a capability that does not work is effort
   spent in the wrong place.
6. **That MiCode's boundary can stay protocol-only.** Falsified the first time
   MiCode needs a backend-specific capability (a provider's tool-calling mode, a
   local model's streaming) that the neutral ABI does not express. *Detection:*
   any MiCode-side config naming a backend. That is the canary for this whole
   document's central claim, and it should be watched for, not assumed away.
7. **That `AXON-COMPLETENESS.json` is the right home for the mode matrix (§5.2).**
   Falsified by the generator's hardcoded four-engine set
   (`scripts/completeness.py:157`) if the owner declines to change it. Then the
   matrix needs a home, and D-007 says a fifth registry is the wrong answer —
   leaving it inside this spec, ungated, which is strictly weaker.
8. **That "no new exit code" (§5.1) is sustainable.** Falsified if a supervisor
   needs to branch on a Reflex refusal. *Detection:* the first supervisor that
   parses stderr to find out. Parsing a message for a status is the shape
   `governance/EXIT_CODES.md` exists to prevent.
9. **That nothing Reflex-shaped exists in the live repo (§2.1).** My search was
   over `crates/`, `governance/specs/` and `spec/` with the terms in §2.1. A
   differently named prototype — under `docs/`, in a branch, or in another
   repository — would not appear. *Detection:* any subsequent grep that finds one.
   I claim only what I searched for.

---

## 15. Conflicts and proposals

Filed in the style of `governance/cortex-v015/DISCREPANCIES.md`. A disagreement
between two documents is not a discrepancy until one of them is measured against
code, so each record states its evidence class.

### X-001 — CX-35 cannot be added to the v0.15 package

**Package state.** `specs/INDEX.md` ends at CX-34 (`:93`); `spec_manifest.json`
(schema `cortex-spec-package/1`) lists specs by path; `SHA256SUMS_v0_15.json`
pins 113 files and `scripts/cortex_package_gate.sh:22-27` checks it **in both
directions** — an unlisted file appearing under the package root fails the gate.

**Consequence.** A CX-35 inside the package breaks the intake gate; editing
`INDEX.md` or `spec_manifest.json` breaks the digest check. This file's location
is the resolution, not an oversight.

**Proposal.** Treat `governance/cortex-v015/` as the authoritative location for
Axon-authored CX specs, and record in `IMPLEMENTATION_MAP.md` that CX numbering
now spans two locations: CX-00..CX-34 vendored, CX-35+ live. Upstream may adopt
CX-35 in a future release; until then the package's own `CX IDs do not reserve
live Axon R IDs` note (`specs/INDEX.md:3`) should gain a symmetric sentence.

**Evidence class.** Verified by reading the gate script and the manifest. Not
verified by executing `cortex_package_gate.sh` (strict gate in flight; no builds
per instruction).

### X-002 — the control matrix generator is hardcoded to four ENGINES

**Live implementation.** `scripts/completeness.py:157`:
`ENGINES = {"interpreter", "native", "wasm", "guest"}`, and `:160-164` fails any
control row not stating all four.

**Conflict.** §5's matrix is keyed by deployment MODE, not engine. The natural
home for it — `AXON-COMPLETENESS.json`'s `controls` array, per its own
`legend.controls` rationale and D-007 — cannot accept a mode-keyed row.

**Proposal.** Generalize the axis: each control row declares its own axis
(`axis: "engine" | "mode"`) and the required key set is looked up from that axis,
preserving the both-directions env-registry check at `:186-194` for engine-axis
rows only. Owner decision; deliberately not taken here.

**Evidence class.** Verified by reading the source. Not executed.

### X-003 — `LlmGateway` metering is interpreter-only; serving needs it at the seam

**Live implementation.** `crates/axon-core/src/kernel.rs:689,717` — per-principal
per-token cost metering, and the kernel builtins it backs are interpreter-only
(`CLAUDE.md`, Phase 7: "interp-only, codegen E0910-refused").

**Conflict.** §5 requires per-principal budget enforcement at the Reflex seam in
all three modes. In LocalSidecar and RemoteService the seam is a separate process
that does not run the interpreter's kernel, so today's mechanism does not reach
it.

**Why it matters — this is the D-010 shape.** A budget an operator believes is in
force, and a process that never consults it, is exactly "a live billed model call
while the operator believed the run was replayed" with the noun changed.

**Proposal.** The Reflex client, not the backend, owns metering: it debits before
dispatch and reconciles against reported usage after, refusing when the debit
would exceed the principal's budget — the same pre-dispatch-estimate discipline
`AXON_BUDGET_TOKENS` already uses (E1303 before any model dispatch, per
CLAUDE.md). A backend's self-reported usage is evidence, never the authority.

**Evidence class.** Verified by grep + reading `kernel.rs`. Not verified by
running a metered call.

### X-004 — the HTTP transport is uncompiled in every gated build

> **Verified against resolved Cargo features, not just the feature name.**
> A grep showing `asi-runtime` absent from `scripts/` and `.github/workflows/`
> does not prove the feature is off: Cargo features arrive through defaults and
> dependency edges as well as command lines. Checked properly:
> `axon-core`'s `default = ["codegen"]` does not include `asi-runtime`; no
> workspace crate requests it (the only dependant, `axon-wasm`, takes
> `default-features = false`); and `cargo tree -p axon-core -e features`
> resolves **zero** occurrences of `asi-runtime`. So the conclusion holds — the
> `#[cfg(feature = "asi-runtime")]` HTTP paths are not compiled by any gated
> build.
>
> One correction that changes Phase 1's options for the better: `reqwest` IS
> compiled into the default build, arriving via `axon-domain`'s default
> features (`cargo tree -i reqwest` shows the edge). So "no HTTP client is
> available" would have been wrong. The client is present; only axon-core's
> `asi-runtime` code paths are absent. This also settles the open question of
> whether a usable HTTP client exists for a Phase 1 adapter: it does, without
> enabling a new feature.


**Live implementation.** `crates/axon-core/src/host.rs:94-110` (denied by
default), `:269,292` (`#[cfg(feature = "asi-runtime")]`). Zero occurrences of
`asi-runtime` under `scripts/` or `.github/workflows/`.

**Conflict.** `build/DEPENDENCY_ADOPTION.md:58-60` assumes a protected hosted
backend is reachable and requires authentication, TLS, isolation and budgets of
it. In this repository, an `.ax` program in a gated build cannot make the call at
all, so none of those requirements has ever been exercised.

**Proposal.** Phase 1's RemoteService adapter lives in Rust inside `axon-cortex`
(whose `ai` feature `scripts/gate.sh:130-133` does build and test) rather than behind the
`.ax` HTTP builtins, and CX-35 does not depend on `asi-runtime` being wired into
CI. Separately, the `asi-runtime` coverage gap is a pre-existing live finding
worth its own record.

**Evidence class.** Verified by grep in both directions. Not verified by building.

### X-005 — the existing local-HTTP precedent is unauthenticated

**Live implementation.** `crates/axon-web/src/main.rs:9-11` binds
`127.0.0.1:{port}`; `server.rs` and `main.rs` contain no `auth`, `token`,
`tenant` or `principal`.

**Conflict.** `build/DEPENDENCY_ADOPTION.md:58-60`: "A sample endpoint being
unauthenticated or broadly network-accessible is not an Axon deployment
recommendation. Protected-runtime adoption requires authenticated principals,
transport security where applicable, tenant/cache isolation, budgets,
observability, revocation."

**Proposal.** A socket-based LocalSidecar MUST NOT copy the `axon-web` shape.
Either keep the one-shot stdin/stdout carrier, where the principal is established
by process-spawn authority, or specify authentication before a socket carrier is
built. §5's `transport authentication / LocalSidecar` cell is `unknown` for
exactly this reason.

**Evidence class.** Verified by grep. Not verified by running the server.

### X-006 — `implementation_evidence: []` must stay empty

**Package convention.** Every CX spec carries `implementation_evidence: []`, and
`scripts/cortex_package_gate.sh` enforces the analogous honesty rule for gates
and tasks: a non-default claim requires a registry row naming a script that
exists and is invoked by something that runs.

**Note, not a conflict.** CX-35 keeps the empty list. When a G35 gate is
implemented, the row goes in `governance/cortex_gate_execution_registry.json`
per its `how_to_add_a_row`, and the vendored `gate_manifest.json` is not touched
(it is SHA-pinned, and the package validator hard-fails on any `product_result`
other than `NOT_RUN`). Recorded here so the next author does not reach for the
manifest.

---

## 16. Register of UNKNOWN and UNVERIFIED items

Everything this spec does not know, in one place, so it is actionable rather than
scattered.

**UNKNOWN matrix cells** (§5) — required states not established:

| capability | mode | why unknown |
|---|---|---|
| cancellation stops work | RemoteService | provider-dependent; no provider examined |
| cancellation stops billing | RemoteService | same |
| KV/prefix residency across calls | RemoteService | emulated vs real reuse is per-provider |
| `StateHandle` survives restart | RemoteService | no provider durable-state semantics examined |
| CPU/GPU residency observable | RemoteService | provider APIs generally do not report it |
| hot-swap without dropping in-flight | RemoteService | provider deployment semantics opaque |
| concurrency limit enforced | Embedded | depends on the caller's scheduler; the live one is interpreter-only |
| transport authentication | LocalSidecar | depends on carrier choice (stdin/stdout vs socket), undecided — X-005 |
| replay without a live call | **all three** | no Reflex replay mechanism exists; the live `AXON_AI_REPLAY` is interpreter-only and was ignored by native (D-010) |
| observability: latency breakdown | RemoteService | providers usually report one aggregate number |

**UNVERIFIED statements** — believed, not established:

1. The error/exit surface for a control refusal (§5.1). Three candidates, no
   owner decision, and `CX-00:46` forbids allocating a code here.
2. Whether `axon-cortex`'s `ai` feature transitively provides an HTTP client
   usable for a non-`axon-ai` endpoint (§7). No build was run, per instruction.
3. Whether `Refusal` in `crates/axon-cortex/src/runner.rs` should be extended or
   paralleled for Reflex (§6). I read its variants only through the
   `cortex-policy-adapter` call sites, not the enum definition.
4. That no Reflex-shaped prototype exists outside `crates/`,
   `governance/specs/` and `spec/` (§2.1, §14.9).
5. Phase 5 feasibility in every respect (§12). Durable cross-process KV, a
   cross-backend state-compatibility relation, and the security story for a
   latent encoding observation content are all absent, and I would not assert any
   of the three is tractable.
6. Every §5 cell's *verification* state. Nothing is implemented, so nothing is
   measured; the cells are requirements. The moment one is implemented, its
   measured state must be recorded separately from its required state, because
   conflating the two is the defect this whole document is shaped around.
