# Tech Spec — R25: Information-Flow / Egress Monitor ("what can LEAK")

**Spec ID:** `R25-information-flow-monitor`
**Status:** 📝 Draft (2026-06-27; **corrected 2026-07-31**, three passes — see the ⚠ CORRECTIONS block
below. The 3rd pass re-scopes R25 v1 from a *security deliverable* to a **mechanism + data model**
milestone; it does **not** satisfy `VISION_OS` G4 — §1.7, §11.)

> **⚠ CORRECTIONS (2026-07-31, pre-build review against the real tree — do not build from the
> uncorrected claims):**
> 1. **The L1 mediation premise was FALSE.** Earlier drafts claimed R21's runtime has per-action
>    ingress/egress wrappers (`fs_read`/`fs_write`/`net`/`exec`) the monitor could hook. It does
>    **not**: `crates/axon-os/src/runtime.rs` `run_sandboxed` (≈lines 291–425) spawns the `axon`
>    interpreter **once** as a subprocess (via `wrap_in_sandbox`), all I/O happens **inside** that
>    subprocess, the supervisor sees only the exit code + stderr sniffing, and exactly **one** `run`
>    `RawEvent` is recorded. No value ever crosses the supervisor boundary in either direction.
>    §1.5/§1.6/§4.1/§4.6/§4.9/§6-S7/§7 are re-scoped accordingly: the per-action mediation seam is
>    real at L1 only through the injected `EgressMonitor` trait + `MockRuntime`; real-runtime in-run
>    enforcement is the named **L1.5 I/O-proxy** follow-up (§4.9, §12) — it is a **protocol + axon-core
>    change**, not "just wiring."
> 2. **Verify-tamper exit code renumbered 9 → 11.** R27 took exit 9 for `Verdict::ResourceBound`;
>    `VerifyMismatch` moved to **exit 11** (`crates/axon-os/src/verdict.rs`). §5.2/§8 updated.
> 3. **Spec-ID collision:** `R25` is dual-claimed (`R25-zephyr-target.md`, Slice 1 landed) — the known
>    parallel-track dual-claim class (R21/R22/R23/R24 are also dual). Always use the full slug
>    `R25-information-flow-monitor` in cross-references; the gate script is named
>    `r25_ifc_acceptance_gate.sh` (not the ambiguous `r25_acceptance_gate.sh`).
> 5. **(3rd pass, 2026-07-31 — ASI-trajectory review; read with #1)** The corrections above fixed
>    *mechanism* honesty; this pass fixes **population** honesty. Every "honest residual" this spec
>    declares is, under the repo's own thesis (`ROADMAP.md` line 53: typed Axon is **an IR, not a
>    human-authored surface**; `R22-intent-approve-gateway` is the landed LLM synthesizer that emits
>    the `.ax` + `.axjob` R21 runs), the **expected behavior of the population under test**, not an
>    edge case. Consequences taken here, in order of severity:
>    (a) **R25 v1 is NOT a security deliverable and MUST NOT mark `VISION_OS` G4** — §1.3/§1.5/§11
>    re-scoped to *mechanism + data model* with an explicit guarantee card (§1.7).
>    (b) **Provenance was program-supplied and failed OPEN** (`value_id: None ⇒ trust the claim`),
>    contradicting I-4. Fixed by the **run high-water label** (§2.1, §4.1, §5.1, I-4, I-9): after any
>    ingress stamped ≥ Confidential, an unprovenanced egress is evaluated at the high-water label.
>    (c) **The R21 sandbox fence this spec cited as its residual containment is escapable from inside
>    a generated program** (verified against the built interpreter, §12.2) — the §4.9/§8 claims are
>    footnoted, not relied on.
>    (d) The covert bound counted **emissions per run** — re-specced to **bytes AND count, over a
>    lineage** (§3.3, §4.8, I-6). Declassify was unmetered and content-blind — now digested, counted
>    and capped (§3.4, §4.4). The acceptance gate was grep-shaped — a **mutation step** is now
>    mandatory (§10 step 3d). The `.axio` was author-written — now AST-derived with a **containment**
>    binding check (§5.3, §10 step 3c).
>    (e) Machinery the repo already ships and this spec under-used is scoped as named future slices,
>    **not** folded into v1: an in-process mediation seam (§12.3 F1), a static taint pass (§12.3 F2),
>    ledger/live-monitor integration (§12.3 F3). Open strategic questions are in §12.4 — unanswered
>    on purpose.
> 4. **(2nd pass, same day)** The first-pass §2.1 `EgressMonitor` trait carried **no value
>    identity**, so `on_egress` saw only the program's *claimed* label and the pinned
>    `mislabeled_secret_still_caught_at_source_stamp` test was unimplementable. The trait now mints
>    an opaque **value-id** at `stamp_ingress` and `on_egress` resolves the supervisor-stamped label
>    by id (§2.1, §4.1, §5.1, §6-S7). The MockRuntime scripted-action model is enumerated in full
>    (§2.1), the A2 fixture is machine-derived from a shipped `.axio` declared-I/O artifact bound to
>    the `.ax` source by a gate check (§5.3, §7, §10), the `mediation:"none"` marker is carried as a
>    **chained flow event** (inside the tamper chain, §3.5), and the L1 quickstart/examples are
>    pinned **network-free** (a live unmediated POST attempt was both nondeterministic and the very
>    leak the spec exists to refuse; §5.3, §9).
**Implements:** `VISION_OS.md` §4.2 (the co-equal pillar *"what can LEAK — information flow &
egress"*) and gap **G4**. Promotes the **already-shipped confidentiality lattice** (the value-level
`Secret`/`Public` types in `examples/stdlib/tainted.ax`, the `Source`/`Tainted` trust lattice in
`examples/stdlib/source.ax`, and the grant `Label` ceiling in `crates/axon-os/src/grant.rs`) from a
*type a program may choose to use* into a **TCB-enforced egress monitor**: secrets are **labeled**,
labels **propagate** through computation, and a **correctly-labeled** value **provably cannot reach a
sink below its clearance — even through a capability the program was legitimately granted.**
**Audience:** an implementer who builds *strictly* against this document and reads only it.

> **THREAT-MODEL HEADLINE — read this before the guarantee.** R25 v1 (the **L1** rung, §1.5)
> assumes values are **correctly labeled by their producer**. The guarantee is therefore
> **CONDITIONAL on honest labeling**: against a benign-but-careless program it is a real no-write-down
> monitor, but against an **adversarial synthesizer** (the R22 code-author) that deliberately
> **mislabels** a secret as `Public` or **omits** the label entirely, the explicit-flow guard sees a
> low label and *allows* the egress — so the **primary exfiltration channel is uncontained until L2**
> (interpreter-stamped labels, §4.9). v1's mitigations against this are (a) the **supervisor-stamped
> source labels** of §1.6 (the producer does **not** get to label born-confidential inputs) and
> (b) the **covert-bandwidth bound** of §4.8 — *but* the bound itself is bypassable by the same
> mislabel move (§4.8, last paragraph). State this plainly to any operator: **L1 protects against
> *honest mistakes and a granted-but-trusted program leaking a correctly-labeled secret*, not against
> a synthesizer that lies about labels.** Closing that is L2 and is the load-bearing follow-up.
>
> **THE POPULATION PROBLEM (3rd pass, 2026-07-31 — this is the finding that re-scopes the whole
> spec).** The paragraph above describes the protected population as "honest mistakes and a
> granted-but-trusted program." Under this repo's own committed thesis **that population is empty by
> construction**: `ROADMAP.md` line 53 makes typed `.ax` an **IR, "not a human-authored surface"**,
> and the landed `R22-intent-approve-gateway` is the LLM synthesizer that authors ~100% of the
> programs R21/R25 run. So the *excluded* case (a model-authored program that may mislabel) is not an
> edge case — **it is the entire population under test.** Therefore, stated plainly and normatively:
> **R25 v1's security delivery against the actual code population is approximately zero.** What R25
> v1 *does* deliver, and what it must be judged on, is the **mechanism + data model + audited seam**
> that L1.5/L2 make load-bearing. See the guarantee card (§1.7), which is machine-readable and MUST
> ship with the crate. **`VISION_OS` G4 ("secrets provably can't reach sub-clearance sinks") and v1's
> "info-flow bound certificate-checked and attested" done-when MUST NOT be marked satisfied by R25
> v1** (§11).

> **Read this framing first.** A capability bound limits *which* channels exist, not *what flows
> through a granted one* (`VISION_OS.md` §4.2). An AI legitimately granted `net` to one endpoint can
> exfiltrate arbitrary secrets through it. R21's gate proves "the program may only touch the net host
> X"; it says **nothing** about *what bytes* go to X. "Can confidential data leave?" is an
> information-flow (non-interference) problem, **not** a capability one — a different, harder axis. R25
> adds the missing axis: a `Labeled<T>` value carries its confidentiality level, the level propagates
> stickily through `combine`/`derive`, and a TCB-owned `sink_guard` consulted **before every egress**
> (`fs_write`/`net`/`exec`) refuses a value whose clearance does not dominate the sink's — wiring the
> decision into R21's existing tamper-evident `AuditEvent.label` field (`crates/axon-os/src/record.rs`).
> **Honesty up front (§1.2): full non-interference is genuinely hard and OUT of scope.** R25 specs a
> *tractable, sound fragment* — explicit-flow label propagation over a labeled-value type + a sink
> check — enforced **as a library monitor first** (like the shipped stdlib value types), with a clearly
> marked path to interpreter/kernel enforcement. It does **not** track implicit flows; residual covert
> bandwidth on a channel that must stay open is **rate-limited and monitored, not assumed closed.**

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey + smoke test through the actual CLI | §5, §7 | `acc_a1_smoke_label_propagate_egress_denied` |
| **A2** | Real runnable example artifact — at L1 the fixture is machine-derived from the shipped `.axio` declared-I/O artifact, gate-bound to the `.ax` source (§10 3c); the real-subprocess leak is L1.5 | §5.3, §7 | `acc_a2_example_exfil_denied_and_public_allowed` |
| **A3** | Quickstart whose exact commands are executed by a test | §9, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Hermetic, isolated execution + hard timeout, canonical entrypoint | §4.6, §7 | `acc_a4_hermetic_isolated_timeout` |
| **A5** | Deterministic & reproducible (byte-identical flow record across runs) | §4.7, §7 | `acc_a5_deterministic_byte_identical` |
| **A6** | Integrity: fail-closed sink guard + tamper-evident labeled egress record | §3.5, §4.5, §7 | `acc_a6_flow_record_tamper_detected` |
| **Core** | A secret value CANNOT reach a sink below its clearance — even with the capability granted | §4.3, §7 | `secret_cannot_reach_sink_below_clearance_even_with_cap` |
| **Core** | Sticky taint: combine(secret, public) = secret (label propagates, never drops) | §4.2, §7 | `combine_is_sticky_label_never_drops` |
| **Core** | A mislabeled/omitted-label secret is caught because the SUPERVISOR stamps the source label (not the program) | §1.6, §3.3, §4.1, §7 | `mislabeled_secret_still_caught_at_source_stamp` |
| **Core** | An un-declassified secret to a public sink is REFUSED with a distinct exit code | §4.3, §7 | `undeclassified_secret_to_public_sink_refused` |
| **Core** | Declassification is explicit, privileged, and AUDITED (never implicit) | §4.4, §7 | `declassify_is_explicit_privileged_and_audited` |
| **Core** | A steganographic / covert drip is BOUNDED (rate-limited + monitored, not assumed closed) | §4.8, §7 | `covert_drip_is_bandwidth_bounded` |
| **Core** *(3rd pass)* | Provenance fails CLOSED: an unprovenanced egress after a secret ingress is denied at the run high-water label, never at the program's claim | §2.1, §4.1, §7, I-9 | `unprovenanced_egress_after_secret_ingress_is_denied` |
| **Core** *(3rd pass)* | The blessed laundering channel is metered + content-attributed (a redaction is distinguishable from a dump) | §3.4, §4.4, §7 | `declassify_volume_is_bounded` |
| **Core** *(3rd pass)* | The covert bound is in BYTES and accumulates over a LINEAGE, and charges denials | §3.3, §4.8, §7, I-6 | `covert_bound_is_bytes_and_lineage_scoped` |
| **Honesty** *(3rd pass)* | The shipped guarantee card names the protected population and does NOT claim G4 | §1.7, §10 3e | `guarantee_card_does_not_overclaim` |
| **Gate** | The acceptance gate itself fails if any check above is missing/stubbed | §10 | `scripts/r25_ifc_acceptance_gate.sh` |
| **Gate** *(3rd pass)* | The gate is MUTATION-tested, not grep-satisfied: inverting the guard/stamp/join must turn the suite red | §10 3d | `scripts/r25_ifc_acceptance_gate.sh` step 3d |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does
R25 is an **information-flow monitor** that sits at the egress boundary of a supervised run and:

1. **Labels** values with a confidentiality level (`Public < Internal < Confidential < Secret`,
   matching the **4-rung** `cl_*` ladder in `examples/stdlib/tainted.ax`). This is `axon-ifc`'s own
   4-rung lattice (§3.1); the shipped **3-rung** grant `Label` ceiling in `crates/axon-os/src/grant.rs`
   injects via an explicit total `From<axon_os::grant::Label>` (the TCB `Label` lattice is not
   edited; the two stay distinct types. NOTE, corrected 2026-07-31: `axon-os` **is** edited — the
   `EgressMonitor` trait + seam of §2.1 — the "no TCB edit" claim is scoped to the lattice only).
2. **Propagates** labels through explicit data flow: `combine`/`derive` of two labeled values yields a
   value at the **higher (more restrictive)** label — taint is sticky, exactly `secret_combine`'s
   "take the max level" rule (`tainted.ax` lines 153–158).
3. **Gates every sink.** Before any egress (`fs_write`, `net`, `exec`), a TCB-owned
   `sink_guard(value_label, sink_clearance)` returns `Allow | Deny` and the supervisor **refuses** any
   value whose clearance does not dominate the sink's — *even when the capability to use that sink was
   granted* (R21 admitted the channel; R25 governs the bytes). This is `secret_can_flow_to`'s rule
   (`tainted.ax` line 139) **promoted to a mandatory, fail-closed check.**
4. **Declassifies only explicitly.** Lowering a value's label is a **privileged, named, audited
   operation** (`declassify`) gated on a clearance authority — never an implicit side effect of
   computation. This is `secret_declassify`'s `Result` (`tainted.ax` line 145) made the **only** way a
   label drops, and every use is recorded.
5. **Audits** every flow decision (allow / deny / declassify) into R21's existing **hash-chained,
   tamper-evident** record via the `AuditEvent.label` field (`record.rs` lines 41–49), so the egress
   ledger is replayable and integrity-checkable.
6. **Bounds covert bandwidth.** Where a channel must stay open (a granted `net` sink that legitimately
   emits *some* data), the monitor **counts and rate-limits** the volume of distinct
   secret-derived emissions per run (a coarse bandwidth ceiling), refusing past the cap — covert
   leakage is *monitored and bounded*, never *assumed closed*.

### 1.2 What it explicitly does NOT do (out of scope for R25 — and WHY)
Information-flow control is genuinely hard; R25 is deliberately a **sound, tractable fragment**, not
full non-interference. The boundaries are load-bearing, not laziness:

- **No full non-interference / no implicit-flow tracking.** R25 tracks **explicit** flows (a labeled
  value derived from / combined with another labeled value). It does **not** track *implicit* flows —
  a secret influencing control flow (`if secret > 0 { write 1 } else { write 0 }`) leaks one bit
  *without* the written value ever being a labeled-secret derivation. Tracking every implicit flow
  needs a program-counter label / dependency-tracking taint engine threaded through the interpreter,
  which is the **kernel-enforcement follow-up** (§1.5). R25's contribution to implicit flows is the
  **bandwidth bound** (§4.8): we cannot prove zero implicit leakage, so we *cap and monitor* it.
- **No interpreter-wide automatic taint propagation.** R25 v1 is a **userland-enforceable monitor**:
  the program (or the R22 synthesizer) constructs values as `Labeled<T>` and a monitor-consulting
  runtime consults `sink_guard` at each mediated egress (at L1 that runtime is the §2.1 mock seam —
  the real runtime mediates nothing in-run, CORRECTIONS #1). What is
  **library-enforced** vs **needs interpreter wiring** is
  stated explicitly in §4.9 — do not blur it. The kernel-enforced version (every interpreter value
  carries a label, propagation is automatic, `sink_guard` is unbypassable) is the follow-up.
- **No timing / cache / power side channels.** "What can be SENSED" is a *different* pillar
  (`VISION_OS.md` §4.3). R25 is confidentiality of *data flow*, not micro-architectural leakage.
- **No cryptographic signing of labels / no PKI.** The flow record is tamper-*evident* (hash-chained,
  reusing R21's `record.rs`), **not** signed. A6 documents exactly what is vs isn't authenticated.
- **No declassification *policy language*.** Declassification is an explicit privileged op gated on a
  clearance authority; *who* may hold that authority and *under what governance* is R22's approval / a
  policy spec, not R25. R25 enforces "a label only drops through an audited `declassify` call by a
  holder of sufficient clearance," nothing richer.
- **No new capability/grant types.** R25 imports R21's `Label`/`Grant`/`EffectSet` and the
  `AuditEvent` record verbatim; it adds the *orthogonal* confidentiality-flow axis on top, never
  redefining the capability axis.

### 1.3 Persona / ICP
A **security-conscious operator** running an AI-authored Axon task that legitimately needs `net` (e.g.
to call a model) but processes **confidential** input. They *want* a *provable, audited* guarantee that
the confidential bytes cannot egress through the granted channel — and a *bounded, monitored* residual
where some egress is unavoidable — without having to trust the AI's good behavior.

**What R25 v1 actually sells them (corrected 2026-07-31, 3rd pass — the previous wording sold the
L2 guarantee under the L1 rung).** R25 v1 does **not** deliver "without having to trust the AI's good
behavior" — that phrase describes **L2**, and against the model-authored population of the population
paragraph above it is exactly the property v1 lacks. R25 v1 sells the operator:
1. an **explicit, legible egress bound** they can read before running (`explain`) and query
   (`check`) — real, and useful for reasoning about a *policy* even when the program is untrusted;
2. a **tamper-evident flow ledger** of whatever decisions were mediated, plus an honest
   `mediation:"none"` marker inside the chain when nothing was (§3.5) — so a record never overclaims;
3. an **audited, privileged, metered declassify** as the sole label-lowering path (§3.4/§4.4);
4. the **data model + `sink_guard` + seam** that L1.5/L2 make unbypassable, unchanged.
The operator's **residual trust decisions** are, explicitly: (i) the synthesizer's honesty about
labels and provenance until L2; (ii) the `--declass-authority` file they grant (§5.2 — and with R22
synthesizing jobs at machine rate, the realistic failure is one broad authority granted once rather
than curated per job; `explain` MUST print the authority in force); and (iii) that a human actually
reads the per-run records (§1.8 — an assumption this spec now states as an expiring limit rather
than leaving implicit).

### 1.4 Interface & tech constraints
- **Interface:** a CLI binary `axon-ifc` (subcommands `label`/`check`/`run`/`verify`/`explain`), plus
  a library crate. The `run` path composes over R21's supervisor (R25 is the egress layer R21 lacks).
- **Language/deps:** Rust, new workspace crate `crates/axon-ifc`. Allowed deps: `sha2` (already in the
  workspace, used by `record.rs`), `serde`+`serde_json` for the records, and **`axon-os` (R21)** for
  `Label`, `Grant`, `EffectSet`, `Verdict`, and the `record::{build,verify,RawEvent,AuditEvent}`
  hash-chain. **No** new heavy deps. **Dependency direction (§2):** `axon-ifc` depends **on**
  `axon-os`; `axon-os` **never** depends on `axon-ifc` (R25 is a leaf above the supervisor).
- **Perf/security:** the lattice + propagation + sink-guard core is **pure** (no I/O, no clock,
  no random); all untrusted execution is in R21's isolated subprocess with a hard timeout; **fail
  closed** on every ambiguity (unknown label ⇒ treated as the most restrictive; missing clearance ⇒
  deny).

### 1.5 Enforcement maturity ladder (be honest about which rung R25 v1 is on)
| Rung | What | R25 v1? |
|---|---|---|
| **L0** (shipped) | Value-level lattice a program *may* use (`tainted.ax`, `source.ax`) — advisory | done (the floor R25 stands on) |
| **L1 — R25 v1** *(re-scoped 2026-07-31)* | TCB-owned `sink_guard` + explicit `Labeled<T>` propagation + audited declassify + bandwidth bound as a **library + CLI** (`check`/`declassify`/`verify`/`explain` are fully real), with the mandatory-consult mediation seam (`EgressMonitor` trait in `axon-os`) exercised **through `MockRuntime` only** — the real `AxonCoreRuntime` runs the interpreter as one opaque subprocess and has **no per-action wrappers to hook** (see CORRECTIONS #1). | **THIS SPEC** |
| **L1.5 — follow-up (BLOCKER for real-runtime in-run enforcement)** | **I/O-proxy mediation**: the interpreter subprocess routes every capability builtin (`fs_read`/`fs_write`/`net`/`exec`/`ai_complete`/`read_line`) to the supervisor over an IPC protocol, so the supervisor actually *sees* each ingress/egress value and can stamp/guard it. Requires a specced protocol + `axon-core` interpreter changes + failure/perf semantics — **a design, not a wiring change**. Must land before any real-runtime in-run guarantee is claimed. | deferred (§4.9, §12) |
| **L2 — follow-up** | **Interpreter-wired** automatic propagation: every interpreter value carries a label, propagation is automatic through all builtins, `sink_guard` is unbypassable because the interpreter itself stamps labels. Closes the "program forgets to label" gap. | deferred (§4.9, named) |
| **L3 — follow-up** | **Implicit-flow / PC-label** tracking (full-ish non-interference). | deferred (§1.2) |

R25 v1 delivers L1 honestly and names exactly where L2/L3 begin. The bandwidth bound (§4.8) is the
*partial* defense against the L3 gap we cannot yet close.

### 1.6 The v1 bridge that does NOT trust the synthesizer's labels (supervisor source-stamping)
The single most dangerous L1 gap (§1.5 headline) is that the **program** chooses its own source labels —
a lying synthesizer just labels a secret `Public`. R25 v1 closes the *source* half of this **without**
waiting for L2 by making the **SUPERVISOR — not the program — stamp the born-on label at the R21
ingress wrappers**, from the `FlowPolicy`'s **input-classification rules** (not from anything the
program asserts):
- Every value the supervisor *hands into* the run through an R21 wrapper (`fs_read`, `ai_complete`,
  `net` read, `read_line`) is **born-labeled by the monitor** according to `FlowPolicy`: e.g. any value
  read from a path under a **confidential-prefixed** input rule is born `Confidential` at the wrapper,
  an `ai_complete`/`net` response is born at its rule's level, an un-ruled source is born **fail-closed
  to `Secret`**. The program **cannot label it lower** — `combine`/`derive` are monotone-up (§4.2) and
  the only lowering path is the audited `declassify` (§4.4), which records.
- This means a synthesizer that **mislabels** the in-program `Labeled<T>` (or omits it) **cannot make
  a born-confidential input look public to the sink guard**: the guard reads the supervisor-stamped
  label that rode in with the value, not the program's claimed one.
- **Honest residual:** this protects only values that **enter through a mediated R21 ingress wrapper**.
  A secret the program **synthesizes internally** (e.g. reconstructs from a constant table) has no
  supervisor-stamped source and is still program-labeled — that residual is L2's job (interpreter-wide
  stamping) and the §4.8 bound is its interim cap. The `FlowPolicy` input-classification rules and the
  ingress-stamp seam are part of L1 and tested by `mislabeled_secret_still_caught_at_source_stamp`.
- **Corrected 2026-07-31:** "the R21 ingress wrappers" above are the **seam this spec declares**
  (§2.1's `EgressMonitor`/ingress-stamp trait), **not** wrappers that exist in the shipped
  `AxonCoreRuntime` — the real runtime crosses no values at all (CORRECTIONS #1). At L1 the
  stamp-then-guard behavior is real and tested **through `MockRuntime`**; stamping real subprocess
  I/O awaits the L1.5 I/O-proxy. State this plainly to any operator.

This is the policy-side input twin of §3.3's *sink*-side `SinkClearance`: the policy now declares both
*how secret each born input is* (input rules, stamped by the supervisor) and *how secret each sink may
receive* (sink rules, checked by the guard).

### 1.7 Guarantee card (normative; ships with the crate as `axon-ifc --guarantee`, machine-readable)
The card is the anti-overclaim artifact: it names the protected population, not just the mechanism.
```json
{ "schema": "axon-ifc-guarantee/1",
  "rung": "L1",
  "protects_against": ["honest mislabeling by a careless program",
                       "a trusted, correctly-labeling program leaking through a granted capability",
                       "policy-level operator error (an over-cleared sink), via explain/check"],
  "does_NOT_protect_against": ["a synthesizer that lies about a value's label",
                               "a synthesizer that lies about or omits a value's PROVENANCE",
                               "any flow inside the real interpreter subprocess (nothing is mediated at L1)",
                               "implicit flows, covert timing/side channels",
                               "reconstruction of a secret via untracked arithmetic"],
  "protected_population": "programs that label and report provenance honestly",
  "population_note": "Under ROADMAP §2.1 + R22, ~100% of programs under this monitor are model-authored, i.e. OUTSIDE the protected population. See the THREAT-MODEL HEADLINE.",
  "satisfies_VISION_OS_G4": false,
  "certificate_checked": false }
```
`satisfies_VISION_OS_G4` MUST remain `false` until L2 lands; `certificate_checked` MUST remain
`false` until the static pass of §12.3 F2 lands. Flipping either is a spec change, not an
implementation choice (§10 step 3e asserts both are false).

### 1.8 Stated expiring assumption — human review of per-run artifacts does NOT scale
Every human-facing affordance in R25 v1 is **single-shot**: `explain <job> <policy>`,
`check <label> <sink>`, `verify <record.json>`, and one sealed `RunRecord` per run at
`<DIR>/<run-id>.json`. The residuals R25 knowingly leaves open — declassify volume, drip counts up to
`covert_cap`, and `mediation:"none"` records that attest nothing about in-run flow — are surfaced
**only inside an individual record a human is presumed to open**, and R25 v1 defines **no trend, no
threshold, and no alarm anywhere**. **This assumption expires the moment job volume exceeds operator
reading capacity**, which R22-synthesized jobs do immediately. It is recorded here as a *stated
limit*, not left implicit; the aggregation work is scoped as §12.3 F3 (emit flow decisions to the R28
`axon-audit` chained ledger so the R29 continuous compliance monitor can trip on them) and is
explicitly **not** in v1.

---

## §2 — Architecture & modules

New crate `crates/axon-ifc/`. **Core logic (lattice, propagation, sink-guard, flow-record build) is
pure (no I/O, no clock/random); all I/O and process spawning are delegated to R21's `Runtime` seam**,
so the monitor is fully testable with a mock. R25 reuses `axon-os`'s `record::{build,verify}` for the
tamper-evident chain rather than reinventing it.

```
crates/axon-ifc/src/
  label.rs        The flow lattice: OWN 4-rung Label + From<axon_os::Label>, dominates/join. [PURE — own type, 3→4-rung bridge]
  labeled.rs      Labeled<T> value model + combine/derive (sticky propagation).        [PURE]
  policy.rs       FlowPolicy: per-sink clearance map + bandwidth ceiling.              [PURE]
  guard.rs        sink_guard(value_label, sink_clearance) -> Allow|Deny{reason}.       [PURE — the TCB decision]
  declassify.rs   Declassify authority + the audited, privileged label-lowering op.    [PURE]
  meter.rs        Covert-bandwidth meter: count secret-derived emissions, cap.         [PURE]
  flowrec.rs      FlowRecord: wraps axon-os RawEvent/build/verify with flow decisions. [PURE — reuses axon-os::record]
  monitor.rs      Orchestrates: for each egress, guard → meter → record. Over Runtime. [PURE core, I/O injected]
  cli.rs          Arg parse → dispatch → human output + exit codes.                    [I/O — thin]
  lib.rs          Public API re-exports (imports axon-os types).                       [—]
  main.rs         fn main() → cli::run(std::env::args).                                [I/O — thin]
crates/axon-ifc/tests/
  acceptance.rs   The A1–A6 + Core acceptance checks (named exactly per §0).
examples/flows/
  exfil.axjob + exfil.ax + exfil.axflow + exfil.axio    A real job granted `net` whose DECLARED I/O leaks a SECRET (A2 negative; .axio = the machine-readable declared-I/O sequence, §5.3).
  redact.axjob + redact.ax + redact.axflow + redact.axio The same job that DECLASSIFIES (redacts) first → allowed (A2 positive).
scripts/r25_ifc_acceptance_gate.sh  The pinned gate (§10).
README-axon-ifc.md             Quickstart whose commands a test executes (A3).
```

**Dependency graph (acyclic; arrows = "imports/uses"). R25 is a LEAF above axon-os:**
```
main → cli → monitor → {guard → label, meter → policy, flowrec → (axon-os::record), declassify}
                       labeled → label
policy → label
flowrec → (axon-os::record {build, verify, RawEvent, AuditEvent})   [reuse — do NOT reinvent the chain]
label  → (axon-os::grant::Label)                                    [bridge only: From<3-rung> → own 4-rung; NOT a reuse of ordering]
monitor → (axon-os::Runtime / Verdict)                              [the supervisor egress seam]

   axon-ifc  ──depends on──▶  axon-os        ✅  (R25 is the egress layer above the supervisor)
   axon-os   ──╳── axon-ifc                  ❌  FORBIDDEN — would create a cycle; axon-os stays a lower leaf
```
**Rule the implementer MUST hold:** nothing under `label/labeled/policy/guard/declassify/meter/flowrec/
monitor` may perform I/O, read the clock, or call random. Volatile inputs (the run-id, the supervisor's
Runtime) are injected (A5). Only `cli.rs`, `main.rs` touch the outside world; all execution I/O is
delegated through R21's `Runtime` trait. **`axon-os` must not gain a dependency on `axon-ifc`** — the
one R21-side touchpoint (§2.1/§4.9: `MockRuntime` consulting an injected `EgressMonitor` at its
scripted ingress/egress points — the real `AxonCoreRuntime` has **no** per-action seam at L1,
CORRECTIONS #1) is wired by having `axon-os` declare the `EgressMonitor` *trait*, which `axon-ifc`
implements — the trait lives low, the implementation lives high (no cycle).

### 2.1 The declared `axon-os` delta (added 2026-07-31 — this spec DOES edit `axon-os`)
The `EgressMonitor`/ingress-stamp seam does **not** exist in the shipped `axon-os` (grep finds no
`EgressMonitor`/`on_egress`/ingress hook anywhere in `crates/axon-os/src/`). Building S7 therefore
**requires this enumerated `axon-os` change** — do not discover it mid-slice:
- **New file `crates/axon-os/src/egress.rs`** declaring the trait (axes/labels as plain
  `&str`/`u8`, value identity as an opaque `u64`, so the trait stays lattice-agnostic and `axon-os`
  gains no `axon-ifc` types). **Corrected 2026-07-31, 2nd pass (CORRECTIONS #4):** the first draft
  returned a bare `u8` from `stamp_ingress` and passed only `value_label` (the *program's claim*)
  to `on_egress` — no identity linked a stamp to a later egress, so the monitor could never look up
  the label it had stamped and the pinned mislabel test was vacuous. The seam carries a value-id:
  ```rust
  pub trait EgressMonitor {
      /// Stamp the born label for a value entering at (source, target). The monitor records the
      /// stamped label INTERNALLY (keyed by the returned id) and returns an opaque value-id the
      /// runtime must carry alongside the value.
      fn stamp_ingress(&mut self, source: &str, target: &str) -> u64 /* value-id */;
      /// Guard an egress. `value_id` = the id minted at a mediated ingress (None when the value
      /// has no mediated-ingress provenance); `claimed_label` = the program/fixture's claim.
      /// The monitor MUST resolve the supervisor-stamped label by id and decide on
      /// join(stamped, claimed) — the claim may only RAISE the effective label, never lower it.
      /// A None id does **NOT** fall back to the claimed label — see the HIGH-WATER RULE below
      /// (corrected 2026-07-31, 3rd pass; falling back to the claim failed OPEN, contradicting I-4).
      fn on_egress(&mut self, axis: &str, target: &str, value_id: Option<u64>,
                   claimed_label: u8) -> bool /* allow */;
  }
  ```
  **THE HIGH-WATER RULE (added 2026-07-31, 3rd pass — normative; provenance is MONITOR-derived,
  never callee-asserted).** The 2nd-pass seam made the *label* unforgeable but left *provenance*
  program-supplied: `value_id` and its association with an egress are asserted by the caller (at L1
  by the `.axio`-derived script's `uses_ingress`; at L1.5/L2 by the interpreter acting on the
  program's data flow). An optimizing generator does not lie about `claimed_label` — the one lie the
  monitor overrides — it lies about **provenance**: emit with `uses_ingress = None`, or point it at a
  `Public` ingress, and `join(stamped, claimed)` resolves to `Public`. That left unknown *label*
  failing closed to `Secret` while unknown *provenance* failed **open to the program's claim** — a
  direct contradiction of **I-4** ("Every ambiguity denies; none silently allows"). Because §4.9 has
  L1.5/L2 reuse this same seam and `sink_guard`, the flaw would be baked in now, not later.
  Therefore the `Monitor` (§5.1) keeps a **run-scoped high-water label** `high_water: Label`,
  initialized `Public` and raised (`join`) by **every** `stamp_ingress`. Then:
  - `on_egress` with a `value_id` the monitor **itself minted** ⇒ effective label =
    `join(stamped, claimed)` as before (the claim may only raise).
  - `on_egress` with `value_id = None`, **or** an id the monitor did **not** mint ⇒ effective label =
    `join(high_water, claimed)` — **the claim is not trusted to lower anything.** Once any ingress in
    the run has been stamped ≥ `Confidential`, every unprovenanced egress is evaluated at that
    high-water level and is denied to a lower-cleared sink.
  This is a **pure, deterministic, monitor-side** change: no interpreter work, no protocol, no A5
  (determinism) impact. It converts the L2 residual from "silently uncaught" into a **coarse but
  sound run-granularity bound**, and `declassify` (§3.4) is the already-sanctioned relief valve for
  the false positives it deliberately creates (a run that reads a secret and then wants to emit
  something unprovenanced must declassify explicitly, with an audit event — the correct posture).
  It does **not** close L2: within a run a program can still avoid ever touching a mediated ingress,
  in which case `high_water` stays `Public`. Pinned by
  `unprovenanced_egress_after_secret_ingress_is_denied` (§7).
- **`MockRuntime`** (`runtime.rs`, `#[cfg(any(test, feature="mock"))]`) — the shipped mock is a
  **canned-outcome stub** (`run_sandboxed` returns `self.outcome.clone()` and bumps a `Cell`
  counter; it has NO per-action I/O model), so the delta is enumerated in full, not hand-waved
  (corrected 2026-07-31, 2nd pass):
  - **Scripted-action model:** `pub enum ScriptedAction { Ingress { source: String, target: String },
    Egress { axis: String, target: String, uses_ingress: Option<usize> /* index of the Ingress
    whose value-id this egress carries */, claimed_label: u8 } }` plus a `script:
    Vec<ScriptedAction>` field, walked in order by `run_sandboxed`.
  - **Injected monitor + interior mutability decision:** the `Runtime` trait's `&self` signatures
    are **NOT changed**; the mock holds `monitor: Option<RefCell<Box<dyn EgressMonitor>>>` —
    interior mutability (matching the mock's existing `Cell` counters) reconciles `&self` with the
    trait's `&mut self` methods.
  - **Semantics:** each `Ingress` → `stamp_ingress` (the returned value-id is remembered by script
    index); each `Egress` → `on_egress(axis, target, id_of(uses_ingress), claimed_label)`. An
    **allowed** egress bumps a new `egress_performed: Cell<usize>` side-effect counter (the counter
    `acc_a2` asserts) and yields an `egress_allow` `RawEvent`; a **denied** egress is **not
    performed** (counter untouched), yields an `egress_deny` `RawEvent`, and the returned
    `RunOutcome.verdict` becomes `Denied{reason, axis}` (first denial wins). Absent monitor ⇒ every
    scripted egress is performed and the canned outcome is returned unchanged
    (`mock_runtime_noop_when_no_monitor`).
- **`AxonCoreRuntime` is NOT changed at L1** — it has no per-action seam to consult (CORRECTIONS
  #1); wiring it is the L1.5 I/O-proxy.
- **Pinned axon-os-side tests** (in `axon-os`, part of S7's definition of done):
  `mock_runtime_consults_injected_monitor` (stamp + guard called in order; denied egress not
  performed) and `mock_runtime_noop_when_no_monitor` (absent monitor ⇒ unchanged behavior).
- The §10 dependency-direction check only proves the absence of a Cargo cycle; the tests above are
  what prove the seam exists and behaves. The "no TCB edit" language elsewhere in this spec is scoped
  to the `Label` *lattice*: `axon-os` the crate **is** edited, and the delta is exactly this list.

---

## §3 — Data model

### 3.1 `Label` — the flow lattice (axon-ifc's OWN 4-rung lattice + a total `From<axon_os::Label>`)
The confidentiality lattice is a total order; a higher label is **more restrictive** (more secret).
**Contradiction resolved (do NOT hand-wave an "alias band"):** the shipped `axon-os::grant::Label` is
**3-rung** (`Public=0 < Internal=1 < Secret=2`, `grant.rs`/`record.rs`) while the stdlib ladder of
`examples/stdlib/tainted.ax` is **4-rung** (`cl_public=0 < cl_internal=1 < cl_confidential=2 <
cl_secret=3`). R25 does **not** edit the TCB `axon-os::Label` (no *lattice* delta is taken here —
but note §2.1: `axon-os` does gain the `EgressMonitor` trait/seam, a declared crate delta); instead
**`axon-ifc` defines its OWN 4-rung `Label`** matching the stdlib ladder, and provides an **explicit,
total `From<axon_os::grant::Label>` mapping** so the supervisor's 3-rung grant ceiling injects cleanly:
```rust
// axon-ifc's own lattice (label.rs) — the flow axis, distinct from the 3-rung grant ceiling:
pub enum Label { Public=0, Internal=1, Confidential=2, Secret=3 }   // ⊑ by numeric order

// the ONLY bridge from the shipped 3-rung grant Label — total, monotone, audited at the seam:
impl From<axon_os::grant::Label> for Label {
    fn from(l: axon_os::grant::Label) -> Label = match l {
        axon_os::grant::Label::Public   => Label::Public,
        axon_os::grant::Label::Internal => Label::Internal,
        axon_os::grant::Label::Secret   => Label::Secret,   // 3-rung Secret ↦ 4-rung Secret (NOT Confidential)
    }
}   // NOTE: the mapping is INTO the 4-rung lattice; `Confidential` has no 3-rung pre-image, so a grant
    // ceiling can never *produce* a Confidential — Confidential arises only from a FlowPolicy input rule.
```
```
Label (flow lattice, ⊑ = "may flow to / no more restrictive than"):
   Public(0)  ⊑  Internal(1)  ⊑  Confidential(2)  ⊑  Secret(3)
```
The grant ceiling (3-rung) and the flow label (4-rung) are kept as **distinct types** bridged only by
the `From` above; R25 never silently equates them. (If a future maintainer instead wants the single
TCB lattice, that is **option (a): a declared TCB delta to `axon-os::grant::Label` adding `Confidential`
between `Internal` and `Secret` — updating `parse`/`as_str`, the `Ord`/`<` ordering, the `max`/join, and
`record.rs`'s label (de)serialization — and is OUT of scope for R25 v1, which takes option (b) above to
avoid mutating the shipped supervisor.)
Operations (pure, total):
- `dominates(a, b) : bool` — `a ⊒ b` (clearance `a` may read a value at level `b`). Exactly
  `secret_can_flow_to`'s `reader_clearance >= s.level` (`tainted.ax` line 139), as the lattice order.
- `join(a, b) : Label` — the **least upper bound** = the *higher* (more restrictive) of the two.
  Exactly `secret_combine`'s "take the max level" (`tainted.ax` lines 155–158). This is the sticky-
  taint operation.
- `parse(s)` / `as_str()` — `"public"|"internal"|"confidential"|"secret"`; an **unknown** token is
  **fail-closed** to `Secret` (most restrictive), never `Public`.

### 3.2 `Labeled<T>` — a value paired with its confidentiality level
```
Labeled<T> {
    value: T,
    label: Label,        // the value's confidentiality level
    origin: Origin,      // how the label was acquired (for the audit trail)
    secret_adjacent: bool, // STICKY provenance bit (added 2026-07-31; the §4.8 meter's predicate):
                           // true iff this value's provenance graph ever touched a label
                           // ≥ Confidential. Set true at birth when label ≥ Confidential; OR-ed
                           // through combine; preserved by derive AND by declassify (lowering the
                           // label never clears it). NEVER cleared by any operation.
}
Origin = Source { kind: String }   // e.g. "fs_read:./secret/"  — the labeled input
       | Combined                  // derived by joining ≥2 labeled values (sticky)
       | Declassified { by: String, from: Label }   // an audited label-lowering (§3.4)
```
`Origin` alone is NOT sufficient provenance for the covert meter — `Combined` carries no ancestry,
so a declassified-secret combined with anything would lose the `Declassified{from: Secret}` evidence.
`secret_adjacent` is the explicit, sticky evidence bit; §4.8's meter is defined **over it**, and S1's
propagation tests assert its stickiness alongside the label's.
Constructors mirror the shipped value-level forms (`tainted.ax` `secret`/`secret_public`/…):
`labeled_public(v)`, `labeled_internal(v)`, `labeled_confidential(v)`, `labeled_secret(v)`,
`labeled(v, level)`. The serialized form (`axon-ifc-labeled/1`, JSON) is
`{value, label, origin, secret_adjacent}`.

### 3.3 `FlowPolicy` — the per-sink clearance ceiling + bandwidth ceiling
A sink's **clearance** is the *highest label it may receive*. An external `net`/`fs_write` to an
untrusted destination has clearance `Public(0)` (it may receive only public data — the classic
exfiltration guard, `tainted.ax` lines 277–283). The policy maps each egress sink to its clearance and
a covert-bandwidth cap:
```
FlowPolicy {
    inputs:       Vec<InputClass>,      // per source: how secret is a value BORN here? (supervisor stamps; §1.6)
    sinks:        Vec<SinkClearance>,   // per concrete sink: how secret may the bytes be?
    covert_cap:       u32,              // max secret-adjacent emissions ATTEMPTED (allow + deny), §4.8; 0 = none
    covert_byte_cap:  u64,              // max secret-adjacent emitted BYTES (added 3rd pass); 0 = none
    covert_scope:     Scope,            // Run | Lineage  — what the two caps accumulate over (default Lineage)
    declass_cap:      u32,              // max declassify ops in scope (added 3rd pass); 0 = none
    declass_byte_cap: u64,              // max declassified BYTES in scope (added 3rd pass); 0 = none
    input_default: Label,               // born-label for any source not matched; FAIL-CLOSED default = Secret(3)
    default:      Label,                // clearance for any sink not listed; FAIL-CLOSED default = Public(0)
}
Scope = Run | Lineage   // Lineage = accumulated across every run sharing a lineage_root, R27-style
InputClass   { source: Source, prefix: String, born: Label }   // the supervisor's source-stamp rule (§1.6)
Source = FsRead | AiComplete | NetRead | ReadLine              // the ingress axes (where labels are BORN)
SinkClearance { axis: Axis, target: String, clearance: Label }
Axis = FsWrite | Net | Exec            // the egress axes (the sinks; mirrors EffectSet)
```
Serialized form (`.axflow` TOML, sitting alongside R21's `.axjob`):
```toml
covert_cap       = 16        # ATTEMPTS (allowed + denied), not just allowed emissions (§4.8, 3rd pass)
covert_byte_cap  = 65536     # and a BYTE ceiling — 16 emissions of a megabyte each is not a bound
covert_scope     = "lineage" # accumulate across runs sharing a lineage_root, not per-run (§4.8)
declass_cap      = 8         # declassify is metered too — it is the blessed laundering channel (§4.4)
declass_byte_cap = 4096
input_default = "secret"     # an un-ruled source is born SECRET (fail-closed — the program can't under-label it)
default       = "public"     # any unlisted sink may receive only public data (fail-closed)
[[input]]
source = "fs_read"
prefix = "./secret/"
born   = "confidential"      # ANY value read under ./secret/ is BORN confidential — the SUPERVISOR stamps it,
                             # not the program; a synthesizer cannot relabel it down (§1.6)
[[input]]
source = "ai_complete"
prefix = ""                  # all model responses
born   = "internal"
[[sink]]
axis      = "net"
target    = "api.model.com"
clearance = "public"         # the model endpoint may receive only PUBLIC bytes (no exfil)
[[sink]]
axis      = "fs_write"
target    = "./out/"
clearance = "confidential"   # the local out dir may hold confidential results
```
Validation (fail → `Verdict::Malformed`, exit 2): `axis ∈ {fs_write, net, exec}`; `source ∈ {fs_read,
ai_complete, net_read, read_line}`; `clearance`/`default`/`born`/`input_default` ∈ the four-rung ladder;
`covert_cap`/`declass_cap` parse as `u32` and `covert_byte_cap`/`declass_byte_cap` as `u64` — a
negative or non-integer TOML value is `Malformed` at parse (the `≥ 0` bound is enforced by the type,
not a runtime rule); `covert_scope ∈ {run, lineage}` (absent ⇒ `lineage`, the stricter default;
an unknown token is `Malformed`, never silently `run`); `prefix`/`target` have no `..` component
(path traversal, mirroring R21 §3.1 / E1001).
A **missing** sink entry ⇒ `default` (fail-closed `Public`), never "allow"; a **missing** input rule ⇒
`input_default` (fail-closed `Secret`), so an un-ruled source is born most-restrictive (§1.6).

### 3.4 `DeclassAuthority` + the declassification op (explicit, privileged, audited)
Declassification is the **only** way a label drops, and it is never implicit:
```
DeclassAuthority { clearance: Label, holder: String }   // who may declassify, and up to what level
declassify(v: Labeled<T>, auth: &DeclassAuthority, to: Label) -> Result<Labeled<T>, DeclassDenied>
```
Rule (mirrors `secret_declassify`, `tainted.ax` line 145, made privileged + audited): succeeds **iff**
`auth.clearance ⊒ v.label` (the holder is cleared to *see* the secret) **and** `to ⊑ v.label` (you may
only lower, never raise). On success the result's `origin = Declassified{by: auth.holder, from:
v.label}` and a **declassify event is emitted to the flow record** (§3.5) — an un-audited declassify is
impossible because `declassify` is the sole label-lowering path and always records. On failure →
`DeclassDenied{reason}` → `Verdict::Denied` exit 8 (no silent downgrade).

**Declassify is the blessed laundering channel — it MUST be content-attributed and metered (added
2026-07-31, 3rd pass).** As specced through the 2nd pass, `declassify` succeeded on two ordinal
checks only, with **nothing bounding how many values, how many bytes, or which values** a holder may
lower, and §1.2 explicitly puts declassification *policy* out of scope. The audit event is a
`RawEvent{action,target,caps_used,label}` (`crates/axon-os/src/record.rs` lines 20–37) — **no value
digest, no size** — so the ledger records only *that* a Secret→Public lowering occurred by `alice`,
never whether it was a redacted name or the entire corpus. §4.4's "an un-audited declassify is
impossible" is therefore true but **thin: the audit is non-discriminating.** For a generator
optimizing against the gate, the shortest path is not to defeat `sink_guard` at all — it is to route
everything through the one API this spec designates as legitimate, producing an `Allow` and a clean
record. Three normative consequences:
1. **Content attribution.** Every `declassify`, `egress_allow` and `egress_deny` event carries a
   canonical **content digest + byte count** of the value at the decision point, so the ledger
   distinguishes a redaction from a dump. Because the `axon-os-record/1` schema is reused verbatim
   (§3.5, no fork), these fold into the event's existing string field in a canonical, deterministic
   form: `target = "<target>#sha256:<hex16>+<bytes>"` (`<hex16>` = the first 16 hex chars of the
   SHA-256 of the value's canonical bytes; `sha2` is already a dep). Digesting is pure and
   deterministic ⇒ no A5 impact. A value the monitor cannot serialize digests as
   `sha256:unavailable+<bytes>` — never omitted.
2. **Metering.** Declassified bytes/ops are counted against `declass_cap`/`declass_byte_cap` in
   `FlowPolicy.covert_scope`; exceeding either ⇒ `DeclassDenied{reason:"declassify ceiling"}` →
   exit 8. Note §3.2 already preserves `secret_adjacent` **through** declassify, so §4.8's meter need
   only additionally count **at the declassify point**, not merely at emission.
3. **Authority visibility.** `axon-ifc explain` MUST print the `--declass-authority` in force
   (holder + clearance) — the operator's residual trust decision (§1.3) must be legible, not opaque.
Pinned by `declassify_volume_is_bounded` (§7). *(What R25 still does NOT add is a declassification
**policy language** — who may hold an authority, under what governance — which remains R22's approval
surface per §1.2. Metering a channel is not governing it; see §12.4 Q3.)*

### 3.5 `FlowRecord` + the labeled `AuditEvent` (tamper-evident; reuse R21's chain)
R25 does **not** invent a new chain — it emits R21 `RawEvent`s (which already carry a `label` field,
`record.rs` lines 20–37) and seals them with `axon_os::record::build`, producing a standard
`RunRecord` (schema `axon-os-record/1`) whose events are the **flow decisions**:
```
A flow decision becomes a RawEvent:
  action  = "mediation" | "egress_allow" | "egress_deny" | "declassify" | "covert_drip" | "covert_blocked"
  target  = the concrete sink (host/path) or "" for declassify/mediation
  caps_used = the egress axis (EffectSet) the decision concerned (empty for mediation)
  label   = the value's label at the decision point (e.g. "secret" on a denied egress)
```
**The mediation marker's carrier (added 2026-07-31, 2nd pass — do not smuggle it outside the
chain):** the honest `mediation:"none"` marker of §4.6 is **not** a new `RunRecord` field (the
`axon-os-record/1` schema is reused verbatim, no fork) and **not** an unauthenticated sidecar — it
is the **first chained flow event**, `RawEvent{action:"mediation", target:"", caps_used:∅,
label:"none"}` (label `"mock-seam"` for a mock-seam record). Because it sits inside the hash chain,
flipping it (e.g. `"none"` → a stronger claim) breaks `record_digest` and `verify` detects it —
`acc_a6` explicitly covers mutating this event.
Because R25 reuses `axon_os::record::{build, verify}`, the chain is **already** tamper-evident: any
mutation/reorder/drop of a flow event breaks `record_digest` and `verify` (proven by `record.rs`'s
`acc_a6_record_tamper_detected`; R25's `acc_a6_flow_record_tamper_detected` exercises the same chain
over *flow* events). **Authenticated:** integrity + ordering of the recorded flow decisions.
**NOT authenticated (documented for A6; rewritten 2026-07-31, 2nd pass for the L1 reality):** *who*
produced the record (no signature), and *completeness relative to real I/O*. Be precise about what
an L1 record even attests: a **real-runtime** record contains **no in-run flow decisions at all**
(the real runtime mediates nothing per-action, CORRECTIONS #1) — it attests supervision + sealing
only, and says so via the chained `mediation:"none"` event above; a **mock-seam** record attests
the flow decisions over the *scripted* actions, not over a real subprocess's I/O. The unbypassable
real-runtime version is L1.5/L2 (§4.9). No HW root of trust (`VISION_OS.md` §5 G6).

### 3.6 `FlowVerdict` / exit codes (reuse R21's carved scheme — never invent a new band)
R25 reuses `axon_os::verdict::Verdict` so codes are consistent across the OS stack:
```
Verdict::Completed { value }        → exit 0   // ran; no flow violation
Verdict::Malformed { reason }       → exit 2   // bad .axflow / usage
Verdict::Denied { reason, axis }    → exit 8   // EGRESS REFUSED: secret-to-low-sink OR covert-cap OR declassify-denied
Verdict::BudgetExhausted { axis }   → exit 7   // (inherited from R21; not R25-specific)
```
An **egress denial** (secret reaching a sink below its clearance, an un-declassified secret to a public
sink, a declassify by an unauthorized holder, or a covert-cap overflow) is `Denied{axis}` **exit 8** —
the same fail-closed code R21 uses for a capability/sandbox refusal, because to the operator both are
"the supervisor refused an over-reach." The *reason* string distinguishes them ("confidential data may
not flow to net:api.model.com (clearance public)" vs the cap-refusal reasons).

---

## §4 — Core logic / algorithms

### 4.1 Labeling an input (`labeled::*`) — Core
A value acquires its label at its **source**, and **the SUPERVISOR — not the program — stamps it** from
the `FlowPolicy` input-classification rules (§1.6, §3.3): an `fs_read` from a path under a
confidential-prefixed input rule is born `Labeled{label: Confidential, origin: Source{"fs_read:<path>"}}`;
an `ai_complete`/`net` response is born at its rule's level; an un-ruled source is born
`input_default` (fail-closed `Secret`). A program-supplied constant is `Public`. (Orthogonally a value
carries the *trust* tag of `source.ax` — R25 governs the **confidentiality** axis; trust/integrity is
`source.ax`'s axis and is **not** conflated.)
- **L1, this spec — supervisor-stamped at the ingress SEAM (corrected 2026-07-31):** for every value
  entering through a mediated ingress (`fs_read`/`ai_complete`/`net_read`/`read_line` **as presented
  at the §2.1 `EgressMonitor::stamp_ingress` seam** — at L1 that seam is driven by `MockRuntime`; the
  shipped `AxonCoreRuntime` crosses no values, CORRECTIONS #1), the monitor applies the matching
  `InputClass` and stamps the born label *before the value reaches the program side*. **A synthesizer
  that mislabels or omits its in-program `Labeled<T>` cannot defeat this at a mediated ingress** — the
  guard reads the supervisor-stamped label, not the program's claim. **Mechanically (2nd-pass
  correction, §2.1):** `stamp_ingress` mints an opaque value-id and the monitor records the stamped
  label keyed by it; the id rides with the value to `on_egress`, which resolves the stamp by id and
  decides on `join(stamped, claimed)` — the claim can only raise, never lower. This is
  `mislabeled_secret_still_caught_at_source_stamp` (an S7 mock-seam test). A born label ≥ Confidential
  also sets `secret_adjacent = true` (§3.2).
- **L1 residual / L2 deferred (NARROWED 2026-07-31, 3rd pass by the high-water rule):** a value the
  program **synthesizes internally** (never crossing a mediated ingress) still has only its
  program-chosen label. But the program no longer gets to have that claim *believed* once the run has
  touched a secret: per §2.1's **high-water rule**, an egress whose `value_id` is `None` or unminted
  is evaluated at `join(high_water, claimed)`, so after any ≥ `Confidential` ingress an unprovenanced
  emission to a low sink is **denied**. The remaining, honestly-stated residual is a run that touches
  **no** mediated ingress at all (`high_water` stays `Public`) — closing *that* needs the interpreter
  to stamp every value (L2, §4.9), with the §4.8 covert bound as the interim cap.

### 4.2 Label propagation — the sticky-taint rule (`labeled::combine`/`derive`) — Core
Step by step, the rule mirrors `secret_combine`'s "take the max level" (`tainted.ax` 153–158) and
`src_join`'s "least-trusted wins" *shape* (`source.ax` 41–43), but on the **confidentiality** lattice
joining **upward** (to *more* restrictive):
1. `combine(a: Labeled<T>, b: Labeled<U>, f) -> Labeled<V>`: the result value is `f(a.value, b.value)`
   and its label is `label::join(a.label, b.label)` — the **higher** (more restrictive) of the two,
   `origin = Combined`. **Taint is STICKY and MONOTONE-UP: combining a Secret with a Public yields a
   Secret; a label NEVER drops through `combine`/`derive`.** (Lowering happens *only* via the audited
   `declassify`, §4.4.) This is the headline `combine_is_sticky_label_never_drops`. The result's
   `secret_adjacent = a.secret_adjacent || b.secret_adjacent || (join ≥ Confidential)` — the
   provenance bit is OR-sticky (§3.2), never cleared.
2. `derive(a, f) -> Labeled<V>`: a unary transform preserves the label (`tainted_map`'s
   trust-preservation, `tainted.ax` 78–80, applied to confidentiality) — `origin` and
   `secret_adjacent` unchanged.
3. **Fail-closed join:** if either operand's label is unknown/unparsable, the result is `Secret`
   (most restrictive). There is no path by which a derivation produces a *lower* label than its most-
   secret input.

### 4.3 The sink guard — the TCB egress decision (`guard::sink_guard`) — Core, fail-closed
This is the heart of R25. Input: the egressing value's `label`, the resolved `sink_clearance` (from
`FlowPolicy`). Output: `Allow | Deny{reason, axis}`.
```
fn sink_guard(value_label: Label, sink_clearance: Label) -> Decision
  = if sink_clearance.dominates(value_label) { Allow }      // clearance ⊒ value-label ⇒ may receive
    else { Deny { "value labeled {value_label} may not flow to a sink cleared only to {sink_clearance}",
                  axis } }                                   // the no-write-down rule
```
This is `secret_can_flow_to` (`tainted.ax` 139) **as a mandatory, fail-closed check at the sink**:
- **The capability being granted is IRRELEVANT to this decision.** R21's gate already proved the
  program may *use* `net:api.model.com`. R25 asks the **orthogonal** question — *may THESE bytes go
  there?* — and a `Secret`-labeled value to a `Public`-clearance sink is `Deny` **even though the net
  capability is fully granted.** This is the entire point of the pillar (`VISION_OS.md` §4.2) and the
  headline `secret_cannot_reach_sink_below_clearance_even_with_cap`.
- **No-write-down at every axis.** The guard is consulted by `monitor::on_egress` before each
  `fs_write`/`net`/`exec`. A higher-labeled value to a lower-cleared sink is always refused.
- **Fail-closed defaults:** an unlisted sink → `FlowPolicy.default` (Public); an unknown value label →
  treated as Secret. Either ambiguity therefore *denies*, never *allows*.
Error/fail-closed cases: (a) value `Secret` → sink `Public` ⇒ Deny (the canonical exfil case);
(b) value label unparsable ⇒ Secret ⇒ Deny unless sink is Secret; (c) sink not in policy ⇒ default
Public ⇒ Deny for any non-public value; (d) `exec` sink: an arg/stdin to a spawned process is an
egress and is guarded identically.

### 4.4 Declassification — explicit, privileged, audited (`declassify::declassify`) — Core
The **only** label-lowering path (§3.4). Algorithm:
1. **Authority check:** `auth.clearance.dominates(v.label)` — the holder must be cleared to *see* the
   secret it declassifies. Else `Deny` exit 8 (`declassify_is_explicit_privileged_and_audited`
   asserts an under-cleared holder is refused).
2. **Direction check:** `to ⊑ v.label` — declassify may only *lower*. A "declassify" that *raises* is a
   bug → `Deny`.
3. **Emit the audit event FIRST, then lower:** a `declassify` `RawEvent` (action=`"declassify"`,
   label=`v.label` before, plus the `from`/`to`/`by` in the reason) is appended to the flow record;
   only then is the lowered `Labeled{to, origin: Declassified{by, from}}` returned — with
   `secret_adjacent` **preserved** (declassify lowers the label, never the provenance bit; §3.2), so
   the §4.8 meter still counts the declassified value's emissions — **and, per the 3rd-pass
   correction in §3.4, the declassify itself is digested, byte-counted, and charged against
   `declass_cap`/`declass_byte_cap` at this point**, so the blessed lowering channel is bounded and
   content-attributed rather than merely recorded. Because
   `declassify` is the sole lowering path *and* always records, **an un-audited declassify is
   impossible** — there is no code path that lowers a label without an event.
4. **No implicit declassify:** `combine`/`derive`/`sink_guard` never lower a label; only this function
   does, and only with authority. This closes the "launder a secret to public by passing it through a
   helper" class (the transitive-laundering hole class noted in the project memory): laundering can't
   *lower* the label, and the sink guard sees the *propagated* (still-secret) label regardless of how
   many pure transforms it passed through.

### 4.5 Flow-record construction & verification (`flowrec::*`) — Core (reuse R21)
`flowrec::seal(run_id, manifest, seed, flow_events, verdict)` = `axon_os::record::build(...)` over the
flow `RawEvent`s; `flowrec::verify` = `axon_os::record::verify`. R25 adds **no** new crypto — it
inherits the SHA-256 hash chain and its tamper-evidence wholesale (`record.rs` 113–201). The R25
acceptance test `acc_a6_flow_record_tamper_detected` mutates/reorders/drops a *flow* event and asserts
`verify` → `Err` (the same guarantee `record.rs`'s own `acc_a6` proves for capability events).

### 4.6 Hermetic isolated execution (`axon-ifc run`) — the impure seam (A4)
R25 does **not** re-implement isolation: `axon-ifc run` composes over R21's `Runtime` /
`AxonCoreRuntime` (`runtime.rs`), which already runs the program in a **fresh, time-bounded
subprocess** (`AXON_OS_TIMEOUT_MS`, canonical `axon` entrypoint by absolute path, child-group kill on
timeout, no leaked handles — R21 §4.4).
**Corrected 2026-07-31 (was a false premise):** the real runtime performs **no per-action
capability-bearing actions the monitor could intercept** — it spawns the interpreter once and all I/O
happens inside the subprocess (CORRECTIONS #1). So at L1, `axon-ifc run` against the real runtime
delivers: R21's gate + whole-run isolation + a sealed flow record whose events are the run event plus
any CLI-level flow decisions, with the record carrying an honest `mediation:"none"` marker — carried
as the **first chained flow event** (`action="mediation"`, §3.5), inside the tamper chain, zero
`axon-os-record/1` schema change — it does
**not** and cannot deny an in-run egress. The per-action `monitor::on_egress` → `sink_guard` →
`meter` → `flowrec` path (a guarded-denied egress **not performed**, recorded as `egress_deny`) is
real at L1 only through the §2.1 seam under `MockRuntime` (S7), and reaches the real runtime at L1.5.
`acc_a4_hermetic_isolated_timeout` reuses R21's runaway-program test through the R25 `run` path,
asserting the child is killed and the record sealed (this part IS real-runtime at L1).

### 4.7 Determinism (A5)
The only volatile inputs are the `run_id` (injected by `cli`, never clock/random in core), the `seed`
(propagated to the interpreter, R21's `AXON_SEED`), and the program's own behavior. Core (lattice,
propagation, guard, declassify, flow-record build) is otherwise a **pure function** of (labeled inputs,
policy, run outcome). **Contract:** two `run`s of the same job + policy + seed produce a **byte-
identical** flow `RunRecord` JSON (R21's record has no timestamp; R25 adds none).
`acc_a5_deterministic_byte_identical` runs the example twice and diffs the records byte-for-byte.

### 4.8 Covert-bandwidth bounding (`meter::*`) — the partial defense for the L3 gap — Core
We cannot prove zero implicit/covert leakage (§1.2). Where a channel must stay open (a granted `net`
sink that legitimately emits public-derived data), a malicious program could **drip** secret bits
through low-bandwidth covert encodings the explicit-flow monitor doesn't see. R25's defense is to
**bound and monitor**, not assume closed:
**SCALE CORRECTION (2026-07-31, 3rd pass — the v1 meter was calibrated for a careless program, not
an orchestrating one).** As specced through the 2nd pass the meter counted "the number of distinct
allowed emissions ... per run." Three assumptions in that sentence are already false for a
model-driven agent, and the fixes below are normative:
- **An emission was unbounded in size.** Sixteen allowed emissions of a megabyte each is unbounded
  bandwidth; nothing bounded payload bytes. ⇒ the meter's unit is now **bytes AND count**
  (`covert_byte_cap` + `covert_cap`, §3.3); exceeding **either** refuses.
- **The meter was run-scoped and the supervisor is per-run**, so an agent that splits exfiltration
  across N cheap runs multiplied the cap by N with **no accounting anywhere**. ⇒ the caps accumulate
  over `FlowPolicy.covert_scope`, default **`Lineage`**, following R27's `ResourceLedger` pattern
  (`crates/axon-os/src/ledger.rs` — append-only, checked arithmetic, accumulated against a
  `lineage_root` rather than a run). A second run under the same `lineage_root` **inherits** the
  count and bytes.
- **Denials were free.** The meter counted only *allowed* emissions, so a refused attempt cost the
  program nothing — see the verdict-channel note at the end of this section. ⇒ the meter counts
  **attempts (allow + deny)**, not allows.
*(Where the accumulated lineage counters live, and how a per-run pure core reads them without
violating §2's purity rule, is deliberately left open — see §12.4 Q2. The v1-safe answer is that the
prior counts are an **injected input** to the pure core, exactly like `run_id`, read/written by
`cli.rs`. Emitting the counters to the R28 ledger for live monitoring is §12.3 F3, not v1.)*

1. The `CovertMeter` counts, over `covert_scope`, the **attempted emissions (allowed + denied) and
   their bytes, to a non-Secret sink
   whose value has `secret_adjacent = true`** (§3.2 — the sticky bit set at a ≥ Confidential birth and
   OR-propagated through every `combine`/`derive`/`declassify`, never cleared). This is the precise,
   computable form of "the provenance graph touched a secret": `Origin` alone cannot express it
   (`Combined` has no ancestry — see §3.2), so the meter's predicate is **defined over the bit, not
   over `Origin`**. It is a coarse over-approximation: a high count of public-but-secret-adjacent
   emissions is the bandwidth available to a drip.
2. When the count exceeds `FlowPolicy.covert_cap`, the next such emission is **refused**
   (`covert_blocked` event, `Verdict::Denied{axis}` exit 8) — the channel is rate-limited, not closed.
3. Every counted emission is recorded (`covert_drip` event with the running count) so the residual
   bandwidth is **monitored and auditable**, not invisible.
`covert_drip_is_bandwidth_bounded` constructs a program that declassifies-and-emits in a loop and
asserts the (N+1)-th emission past `covert_cap` is refused, and that the record shows the rising count.
**Honesty:** this bounds *bandwidth*, it does not *prove* non-interference; it is the named, partial
mitigation for the implicit-flow gap R25 v1 does not close (§1.2).
**Honesty about the meter's OWN limit (the same mislabel move bypasses it):** in L1 the provenance graph
only has an edge for values that actually went through `combine`/`derive`/`declassify`. A value a
program **reconstructs from a secret via untracked arithmetic** (reads the secret, computes a new value
with raw integer ops that never call `combine`/`derive`, then emits it as a fresh `Public` literal) has
**no provenance edge back to the secret** — the meter sees it as ordinary public data and does **not
count it**. So the covert-bandwidth bound is itself bypassable by *exactly the same mislabel/omit-label
move* as the source-stamp gap (§1.6): it caps only the secret-adjacency the explicit-flow graph can
*see*. Like §1.6's residual, fully closing it requires L2's interpreter-wide automatic propagation
(every value carries and joins a label through every builtin, so untracked arithmetic cannot strip
provenance). The bound is real for honest/careless programs and a coarse cap otherwise — not a proof.
**Partially narrowed 2026-07-31, 3rd pass:** the high-water rule (§2.1) means the reconstructed value,
emitted with no minted `value_id`, is now evaluated at `join(high_water, claimed)` and denied to a low
sink once the run has read a secret — so the untracked-arithmetic bypass costs the adversary the
ability to emit *anything* unprovenanced after an ingress. It does **not** close the class (a program
that never touches a mediated ingress, or that declassifies within the ceiling, still gets through);
L2 remains the real fix.

**The monitor's OWN VERDICT is an unmetered channel (added 2026-07-31, 3rd pass).** A denied egress
terminates the run and sets `RunOutcome.verdict = Denied` (§2.1, "first denial wins"), surfacing as
**exit 8 versus exit 0** (§3.6) — directly observable by whoever launched the run. A program can
therefore branch on a secret and choose *whether* to attempt an egress the guard will refuse,
encoding **one bit per run into the supervisor's own verdict**; `axon-ifc check` is a second, explicit
policy oracle in the same shape. One bit per run is negligible for a careless program and
**non-negligible for an agent scripting thousands of cheap runs** — the trajectory assumption this
spec is being judged against. It is listed here rather than left unmentioned. Mitigation taken in v1:
**a denial is evidence of a secret-dependent decision, not a safe outcome**, so `egress_deny` is
charged against the same `covert_scope` ceiling as the allowed drip (the "attempts, not allows" rule
above) — repeated deny-terminated runs under one lineage therefore trip the bound rather than being
free. This does not *close* the channel (nothing at L1 can); it prices it.

### 4.9 R21 / interpreter touchpoints (what is library-enforced vs needs wiring — be precise)
**REWRITTEN 2026-07-31 (the previous version rested on a false premise):** earlier drafts said "the
supervisor's egress points (`fs_write`/`net`/`exec` wrappers in R21's runtime) call an injected
`EgressMonitor`" and that "L2 is a wiring change, not a redesign." Neither is true against the real
tree: `AxonCoreRuntime::run_sandboxed` spawns the interpreter **once**, all I/O happens inside the
subprocess, no value crosses the supervisor boundary in either direction, and no
`EgressMonitor`/ingress seam exists anywhere in `axon-os` (CORRECTIONS #1). The corrected ladder:
- **L1, this spec — library + CLI + mock-seam:** the pure core (S1–S6), the `EgressMonitor` trait
  **declared in `axon-os`** (§2.1 — the trait lives low; `axon-ifc` provides the impl, so `axon-os`
  has **no** dependency on `axon-ifc`, preserving §2's direction), consulted at L1 by **`MockRuntime`
  only**, and the `axon-ifc` CLI (`explain`/`check`/`declassify`/`verify` fully real; `run` = R21
  supervision + sealed record, §4.6). *Within the sinks a monitor-consulting runtime mediates*, the
  guard is mandatory and unbypassable — at L1 that is the mock; the real runtime mediates nothing
  in-run. ~~R21's sandbox still bounds *which* channels the subprocess can reach.~~ **STRUCK
  2026-07-31, 3rd pass — do NOT cite this fence.** R25 must not lean on a fence a synthesizer can
  step over, and this one is escapable **from inside a generated program** (verified against the
  built interpreter; full detail + the fix R21 owes in **§12.2**). Until the R21 attenuation fix
  lands, the honest statement is: *at L1, R25 bounds nothing about the real subprocess's I/O, and the
  capability ceiling it would otherwise defer to is itself bypassable.* This is a **precondition** on
  R25's stated residual, not a footnote to it.
- **The .ax ↔ monitor value boundary (explicit, so no one discovers the gap mid-build):** L1 defines
  **no `.ax` builtins** for labels — no `labeled()`, no `declassify()`, no labeled egress wrappers in
  the interpreter, and no serialization of `Labeled<T>` across the process boundary. An `.ax` program
  cannot interact with the monitor at all at L1; the example flows (§5.3) exercise the monitor only
  through the CLI and through policy-driven decisions at the mock seam. The in-program surface (its
  builtins + the serialized crossing format for `{value,label,origin,secret_adjacent}`) is specced
  together with L1.5/L2, not here.
- **L1.5, deferred BLOCKER (real-runtime in-run enforcement):** the I/O-proxy — the interpreter
  subprocess routes each capability builtin to the supervisor over an IPC protocol so ingress values
  can be stamped and egress values guarded for real. Requires: the protocol spec (framing, per-axis
  payloads, deny semantics = builtin returns a capability error, timeout/failure = fail-closed deny),
  `axon-core` interpreter changes, and perf accounting. **A design effort, not wiring.** Until it
  lands, no real-runtime in-run flow guarantee may be claimed.
- **L2, deferred (named follow-up):** automatic label stamping inside the **interpreter** —
  `fs_read`/`ai_complete`/`read_line` builtins stamp source labels; arithmetic/string builtins
  propagate `join` automatically; the sink builtins call `sink_guard` unconditionally. This removes
  the "program forgot to label" gap and is the path to true kernel enforcement. R25 v1 specs the data
  model + guard so L2 reuses the same `sink_guard`; the interpreter-side work is L1.5+L2's own spec.
- **L3, deferred:** PC-label / implicit-flow tracking (§1.2). R25 v1's bandwidth bound is the interim.

---

## §5 — Public API / interface contract

### 5.1 Library API (`lib.rs`)
```
// The lattice + labeled values (pure):
pub fn labeled<T>(v:T, level:Label) -> Labeled<T>;
pub fn combine<T,U,V>(a:Labeled<T>, b:Labeled<U>, f:impl Fn(T,U)->V) -> Labeled<V>;   // sticky join
pub fn derive<T,V>(a:Labeled<T>, f:impl Fn(T)->V) -> Labeled<V>;                      // label-preserving
pub fn dominates(clearance:Label, value:Label) -> bool;    // may a clearance read a value-label?
pub fn join(a:Label, b:Label) -> Label;                    // least upper bound (more restrictive)

// The TCB sink decision (pure, fail-closed):
pub fn sink_guard(value_label:Label, sink_clearance:Label) -> Decision;   // Allow | Deny{reason,axis}

// Declassification — the ONLY label-lowering path (explicit, privileged, audited):
pub fn declassify<T>(v:Labeled<T>, auth:&DeclassAuthority, to:Label)
    -> Result<(Labeled<T>, RawEvent /*the audit event*/), DeclassDenied>;

// Policy + flow record (reuse axon-os::record under the hood):
pub fn parse_policy(s:&str) -> Result<FlowPolicy, Malformed>;
pub fn sink_clearance(policy:&FlowPolicy, axis:Axis, target:&str) -> Label;   // fail-closed to default
pub fn seal(run_id:&str, manifest:&JobManifest, seed:u64, events:&[RawEvent], v:Verdict) -> RunRecord;
pub fn verify(rec:&RunRecord) -> Result<(), VerifyMismatch>;   // = axon_os::record::verify

// The egress monitor the supervisor consults (impl of the axon-os-declared trait, §2.1):
pub struct Monitor { policy: FlowPolicy, meter: CovertMeter, events: Vec<RawEvent>,
                     stamps: HashMap<u64 /*value-id*/, Label>,  // supervisor-stamped born labels
                     high_water: Label,                         // §2.1 HIGH-WATER RULE; init Public,
                                                                // join-raised by EVERY stamp_ingress
                     next_id: u64 }                             // deterministic id mint (A5: no random)
impl axon_os::EgressMonitor for Monitor {
    // Resolve the matching FlowPolicy InputClass → born label; record it under a fresh id;
    // high_water = join(high_water, born).
    fn stamp_ingress(&mut self, source:&str, target:&str) -> u64;
    // MINTED id      => effective = join(stamps[id], claimed)
    // None / UNMINTED => effective = join(high_water, claimed)   <-- fail-CLOSED provenance (§2.1, I-9);
    //                    the claim may only RAISE the effective label, NEVER lower it, and is never
    //                    trusted alone. (The pre-3rd-pass "fall back to the claim" was fail-OPEN.)
    // then guard → meter (attempts+bytes, §4.8) → record (digested target, §3.4).
    fn on_egress(&mut self, axis:&str, target:&str, value_id:Option<u64>, claimed_label:u8) -> bool;
}
```

### 5.2 CLI (`axon-ifc`; every subcommand has `--help`; output legible, not just exit codes)
```
axon-ifc explain <job.axjob> <policy.axflow>
    Print, in plain English, the EGRESS bound: "Sinks: net api.model.com may receive PUBLIC only;
    fs_write ./out/ may receive up to CONFIDENTIAL. Covert bandwidth cap: 16 attempts / 65536 bytes
    per LINEAGE. Declassify cap: 8 ops / 4096 bytes. Declassify authority in force: alice
    (clearance SECRET) — this is YOUR residual trust decision (§1.3). Any unlisted sink: PUBLIC
    (fail-closed)." Performs NO execution. Exit 0. MUST also print the §1.7 guarantee-card summary
    line ("R25 L1: does NOT protect against a synthesizer that lies about labels or provenance").

axon-ifc --guarantee
    Print the §1.7 guarantee card as JSON (schema axon-ifc-guarantee/1) — the machine-readable
    anti-overclaim artifact naming the PROTECTED POPULATION, not just the mechanism. Exit 0.

axon-ifc check <value-label> <sink: axis:target> [--policy policy.axflow]
    A one-shot sink-guard query: "✓ ALLOW: internal may flow to fs_write:./out/ (clearance
    confidential)" or "✗ DENY: secret may NOT flow to net:api.model.com (clearance public)". Exit 0 / 8.
    The CLI surface of `sink_guard`, for operators to reason about a flow before running.

axon-ifc run <job.axjob> <policy.axflow> [--run-id ID] [--out DIR] [--declass-authority FILE]
    Compose over R21's supervisor with the R25 egress monitor: gate (R21) → run sandboxed → guard
    every MEDIATED egress (R25) → write the flow RunRecord to <DIR>/<run-id>.json. Prints the verdict
    in plain English ("✓ completed" / "⚠ DENIED: secret may not flow to net:… (axis: net)" / "⚠
    DENIED: covert bandwidth cap exceeded"). Exit = verdict code (§3.6).
    L1 HONESTY (corrected 2026-07-31, §4.6): against the real AxonCoreRuntime no egress is mediated
    (no per-action seam exists); the record carries the chained mediation:"none" flow event (§3.5 —
    inside the hash chain, so it is tamper-evident) and the per-egress deny path is
    exercised via the injected mock seam in tests. Real-runtime in-run denial is L1.5.

axon-ifc verify <record.json>
    Recompute the flow hash chain (= axon_os::record::verify); "✓ intact" / "✗ TAMPERED at event N".
    Exit 0 / 11 (Verdict::VerifyMismatch — renumbered from 9 by R27, which took 9 for
    ResourceBound; crates/axon-os/src/verdict.rs). No execution.

axon-ifc declassify <value-label> --to <label> --by <holder> --clearance <label>
    The audited declassify op from the CLI: prints "✓ declassified secret→public by alice (audited)" +
    the audit event, or "✗ DENIED: alice (clearance internal) may not declassify secret". Exit 0 / 8.
```
Usage/`--help` on a bad invocation → exit 2 with a helpful message naming the expected form.

### 5.3 Shipped example artifacts (A2 — real, in `examples/flows/`; renumbered from "5.6" 2026-07-31)

**How the examples meet the monitor at L1 (the boundary, stated — see §4.9):** the `.ax` files are
real R21 jobs, but at L1 an `.ax` program has **no** in-language way to construct `Labeled<T>` or call
`declassify` — those are Rust/CLI surfaces. The A2 flow decisions are produced by (a) `axon-ifc check`
/ `declassify` against the example's `.axflow`, and (b) the S7 mock-seam fixture that replays the
example's **declared I/O sequence**. **The declared I/O has a defined, machine-readable source
(corrected 2026-07-31, 2nd pass — without it A2 was vacuous):** each example ships an `.axio`
sidecar (TOML: an ordered list of `[[io]]` entries, `kind = "ingress"|"egress"`, `source`/`axis`,
`target`, and for an egress `uses_ingress` + `claimed_label` — exactly the §2.1 `ScriptedAction`
shape). The A2 fixture **parses the shipped `.axio`** to build the `MockRuntime` script — it is
NOT a hand-written sequence living only in the test — and the §10 gate's **artifact-binding check
(step 3c)** fails on divergence between the `.axio` and the `.ax` source. The end-to-end
real-subprocess version of these demos is gated on L1.5.

**THE `.axio` MUST BE AST-DERIVED, NOT AUTHOR-WRITTEN (added 2026-07-31, 3rd pass).** As specced
through the 2nd pass, A2's non-vacuity rested on an `.axio` **the author ships**, bound to the source
by step 3c's *presence* checks: every declared `target` must appear verbatim as a string literal in
the `.ax`, and the `.ax` must contain no live net builtin. Both are **presence-directional** — they
constrain what the `.axio` *declares* to be findable in the source, and never that everything the
source *can reach* is declared. A generated program computes its endpoint (`str_concat`, string
interpolation — the interpolation-laundering class has bitten this repo before) or parks matching
literals in dead code, and the `.axio` **legitimately understates its real egress set while passing
the gate**. That is the same trust-the-generator failure as the label problem, one layer down: the
adversary supplies both the labels *and* the description of its own I/O. Normative consequences:
1. **Derive it.** The `.axio` MUST be produced by a **machine extractor over the `.ax` AST**, not
   hand-authored: `Runtime::declared_effects` / `scan_effects`
   (`crates/axon-os/src/runtime.rs:278-283`) and R22's `DeclaredEffects` extractor already enumerate
   capability call sites and are the starting point. A checked-in `.axio` is a *cached* extractor
   output, regenerable and diffable.
2. **Bind by CONTAINMENT, not presence.** §10 step 3c is inverted: **every egress the AST can reach
   MUST appear in the `.axio`**; an AST-reachable egress missing from the `.axio` fails the gate.
   (Extras are also a failure — they mean the artifact drifted.)
3. **Say what this is worth.** A source-level scan is itself **defeasible** — R21's own comment
   concedes "the static gate ... could be fooled" (`runtime.rs:299-305`) — so containment-binding
   raises the cost of understating I/O without closing it. That limitation is precisely the argument
   for the static taint pass scoped in **§12.3 F2**, and it is why `certificate_checked` stays
   `false` in the §1.7 card.
- `exfil.ax` + `exfil.axjob` + `exfil.axflow` + `exfil.axio`: an agent **granted `net`** to
  `api.model.com` reads a `Confidential` input, processes it, and — per its **declared I/O**
  (`exfil.axio`) — POSTs the secret-derived result to the model endpoint (clearance `public`). R21
  *admits* the job (the net capability is granted); **R25 DENIES the egress (exit 8) at the mock
  seam** — the headline negative demo: *the capability is granted, the leak is still refused.* The
  flow record shows the `egress_deny` event labeled `confidential`.
  **L1 real-run behavior is PINNED network-free (corrected 2026-07-31, 2nd pass):** at L1 nothing
  mediates the real subprocess (CORRECTIONS #1), so a live `http_post` in `exfil.ax` would (a) make
  the spec's own headline demo actually *attempt* the exfiltration it exists to refuse, unguarded,
  and (b) make the run verdict a hostage of DNS/proxy nondeterminism (the supervisor's verdict is
  stderr-sniffed). Therefore at L1 `exfil.ax` **computes the secret-derived payload and prints the
  intended egress** (deterministic, exits 0 under supervision) — the POST exists only in the
  declared I/O, where the mock seam denies it. The live-POST variant of `exfil.ax` is an L1.5
  deliverable (§12.1).
- `redact.ax` (+ `.axjob`/`.axflow`/`.axio`): the *same* job but it first **declassifies** (redacts)
  the value via an authorized holder — the audited `declassify` lowers the label to `public`, the
  now-public result is `Allow`ed to the model endpoint (at the mock seam; the same network-free L1
  real-run pin as `exfil.ax` applies), and the record shows the `declassify` event then the
  `egress_allow`. (Positive demo: the bytes *may* leave, but only after an explicit, audited
  declassification — never implicitly.)

---

## §6 — Build order (each slice ends green before the next; TDD: test first, see it fail, make it pass)

- **S1 — Lattice + labeled values.** `label.rs`, `labeled.rs`. Tests: lattice order + `dominates` +
  `join` (= max); `combine_is_sticky_label_never_drops`; `derive` preserves label; unknown label
  fail-closes to Secret; **`secret_adjacent` stickiness** (§3.2 — set at ≥ Confidential birth,
  OR-propagated through combine, preserved by derive/declassify, never cleared; round-trips through
  the `axon-ifc-labeled/1` serialization). Green.
- **S2 — Policy parse/validate.** `policy.rs`. Tests: parse the example `.axflow`; reject bad axis,
  bad clearance, `..` target, negative cap; missing sink → fail-closed `default` (Public).
- **S3 — The sink guard.** `guard.rs`. Tests: `secret_cannot_reach_sink_below_clearance_even_with_cap`
  (Deny regardless of an asserted-granted capability), allow-when-dominated, unknown-label-denies,
  `undeclassified_secret_to_public_sink_refused`.
- **S4 — Declassify (explicit/privileged/audited).** `declassify.rs`. Tests:
  `declassify_is_explicit_privileged_and_audited` (under-cleared holder refused; success emits the
  audit event; raise-attempt refused; combine/derive never lower a label) **and (3rd pass)
  `declassify_volume_is_bounded`** (`declass_cap`/`declass_byte_cap` enforced; the content digest +
  byte count are present, distinguishing, and deterministic — §3.4).
- **S5 — Covert meter.** `meter.rs`. Tests: `covert_drip_is_bandwidth_bounded` (the (cap+1)-th
  `secret_adjacent` emission refused; record shows the count; a declassified-then-combined value is
  still counted — the §3.2 bit, not `Origin`, is the predicate) **and (3rd pass)
  `covert_bound_is_bytes_and_lineage_scoped`** (byte cap binds; a second run under the same
  `lineage_root` inherits the counters; denied attempts are charged).
- **S6 — Flow record (reuse R21 chain).** `flowrec.rs`. Tests: `acc_a6_flow_record_tamper_detected`
  (mutate/drop/reorder a flow event → `verify` Err); equal inputs → equal digest.
- **S7 — Monitor over R21's Runtime (egress guard + ingress source-stamp; MOCK-SEAM ONLY at L1).**
  `monitor.rs` + **building the §2.1 axon-os delta** (the seam does not pre-exist — new
  `crates/axon-os/src/egress.rs` trait, `MockRuntime` monitor consultation, and the two pinned
  axon-os-side tests `mock_runtime_consults_injected_monitor` / `mock_runtime_noop_when_no_monitor`).
  `AxonCoreRuntime` is NOT touched (L1.5). Tests: each egress routed
  guard→meter→record; a denied egress is **not performed** (assert via the mock's `egress_performed`
  side-effect counter, §2.1); deny-before-execute;
  **`mislabeled_secret_still_caught_at_source_stamp`** — the scripted fixture has an `Ingress` under
  `./secret/` (stamped `Confidential`, value-id minted) followed by an `Egress` to a `Public` sink
  whose `uses_ingress` points at that ingress and whose `claimed_label` is (adversarially) `Public`;
  assert the monitor resolves the supervisor's stamp **by value-id**, decides on
  `join(Confidential, Public) = Confidential`, and the egress is **Denied exit 8** regardless of the
  program's lie — the lie *reaches* the monitor (as `claimed_label`) and is overridden, so the test
  is non-vacuous. **(3rd pass)** the same slice implements the §2.1 **high-water rule** (the
  `high_water` field, join-raised at every `stamp_ingress`, consulted for `None`/unminted ids) and
  its pinned test `unprovenanced_egress_after_secret_ingress_is_denied`, plus the widened
  three-variant adversary in `mislabeled_secret_still_caught_at_source_stamp` (§7).
- **S8 — `axon-ifc run`/`explain`/`check`/`verify`/`declassify` CLI + human output.** `cli.rs`,
  `main.rs`. Tests: `acc_a5_deterministic_byte_identical`, `acc_a4_hermetic_isolated_timeout` (over
  R21's runtime), `--help` on every subcommand, usage error → exit 2.
- **S9 — Example artifacts + smoke + quickstart.** `examples/flows/*`, `README-axon-ifc.md`. The
  `.axio` sidecars are **generated by the AST extractor, not hand-written** (§5.3, 3rd pass), and the
  §1.7 guarantee card ships with `axon-ifc --guarantee` (test:
  `guarantee_card_does_not_overclaim` — asserts `satisfies_VISION_OS_G4 == false`,
  `certificate_checked == false`, and that `protected_population` + `population_note` are present).
  Tests:
  `acc_a1_smoke_label_propagate_egress_denied`, `acc_a2_example_exfil_denied_and_public_allowed`,
  `acc_a3_quickstart_commands_execute`.
- **S10 — Acceptance gate.** `scripts/r25_ifc_acceptance_gate.sh` (§10). Green = done.

---

## §7 — Test plan (happy + **adversarial**; every named test is normative)

**Unit / core (pure, fast):**
- `combine_is_sticky_label_never_drops` — `combine(Secret, Public)` → `Secret`; `join` is the max on
  every pair; a chain of `derive`s never lowers the label. Mirrors `secret_combine` taking the max
  (`tainted.ax` 153–158).
- `secret_cannot_reach_sink_below_clearance_even_with_cap` (headline) — a `Secret`-labeled value to a
  `net` sink cleared `Public` → `Deny{axis=net}`, **with the test asserting the net capability is (in
  the fixture) fully granted** — the leak is refused *anyway*. This is the whole pillar.
- `undeclassified_secret_to_public_sink_refused` — a `Confidential` value that was **not** declassified
  → `Deny` exit 8 to a `Public` sink (the `tainted.ax` 277–283 exfil case, made mandatory).
- `declassify_is_explicit_privileged_and_audited` — (a) holder with clearance `Internal` declassifying
  a `Secret` → refused; (b) authorized holder → success **and** a `declassify` audit event is emitted;
  (c) a "declassify" that tries to *raise* → refused; (d) no `combine`/`derive`/guard path lowers a
  label (assert the only lowering API is `declassify`).
- `covert_drip_is_bandwidth_bounded` — emit `covert_cap` secret-adjacent public values (allowed,
  counted), then the next → `Deny{axis}` exit 8 (`covert_blocked`); the record shows the rising count.
- `policy_rejects_malformed` — bad axis, bad clearance, `..` target, negative cap → `Malformed` exit 2;
  a missing sink resolves to the fail-closed `default` (Public), never "allow."
- `unknown_label_fails_closed` — an unparsable value label is treated as `Secret` (denied to any non-
  secret sink), never `Public`.
- `mislabeled_secret_still_caught_at_source_stamp` (headline — the adversarial-synthesizer case) — a
  value enters through the `fs_read` ingress under a `./secret/`-prefixed `InputClass` (born
  `Confidential` by the **supervisor's** stamp, recorded under the minted value-id, §2.1), but the
  program adversarially claims `Public` (or omits the label); on egress to a `Public` sink the
  `on_egress` call carries the value-id + the lying `claimed_label = Public`, the monitor resolves
  the **supervisor-stamped** `Confidential` by id, `join(stamped, claimed) = Confidential` →
  `Deny{axis}` exit 8. Asserts the program's claim cannot LOWER the effective label at a mediated
  ingress (§1.6, §4.1) while the lie itself demonstrably reaches the monitor (non-vacuous). Also
  asserts the honest negative residual: a value the program *synthesizes internally* (egress with
  `value_id = None`) is **not** caught by *source-stamping* — it is caught, if at all, by the
  high-water rule below (documented L2 gap).
  **WIDENED 2026-07-31, 3rd pass:** this test as previously written scripted a **half-honest**
  adversary — one that lies about `claimed_label` while *honestly* reporting `uses_ingress` — i.e.
  the single attack shape the design defeats, under a name that claims the adversarial-synthesizer
  case. The test MUST now cover an adversary lying about **BOTH**: (a) `claimed_label = Public` with
  an honest `uses_ingress` (the original case — denied by the stamp), **and** (b) `claimed_label =
  Public` with `uses_ingress = None`, **and** (c) `claimed_label = Public` with `uses_ingress`
  pointing at an unrelated `Public` ingress. All three MUST deny.
- `unprovenanced_egress_after_secret_ingress_is_denied` (headline, added 3rd pass — the §2.1
  high-water rule) — script an `Ingress` under `./secret/` (born `Confidential`) followed by an
  `Egress` to a `Public` sink with `value_id = None` and `claimed_label = Public`; assert the monitor
  evaluates `join(high_water=Confidential, Public) = Confidential` → `Deny{axis}` exit 8. Also assert
  the **converse** (no false-positive-by-default): with **no** ingress ≥ Confidential in the run,
  `high_water` stays `Public` and the same unprovenanced public egress is **allowed** — so the rule
  is a high-water bound, not a blanket deny. And assert the relief valve: after an audited
  `declassify` by an authorized holder, the emission is allowed and the record shows the declassify
  event first.
- `declassify_volume_is_bounded` (added 3rd pass, §3.4) — (a) declassify ops past `declass_cap` →
  `DeclassDenied` exit 8; (b) a single declassify whose value exceeds `declass_byte_cap` → refused;
  (c) every `declassify`/`egress_allow`/`egress_deny` event's `target` carries the
  `#sha256:<hex16>+<bytes>` content attribution, and two **different** values of the same size
  produce **different** digests (i.e. the ledger can distinguish a redaction from a dump — the
  property §3.4 exists for); (d) digests are deterministic across runs (no A5 regression).
- `covert_bound_is_bytes_and_lineage_scoped` (added 3rd pass, §4.8) — (a) emissions under
  `covert_cap` but exceeding `covert_byte_cap` are refused (a byte cap really binds); (b) a **second
  run** under the same `lineage_root` **inherits** the prior count and bytes and trips the ceiling
  that the first run left just under (the split-across-runs attack); (c) **denied** attempts are
  charged too — N consecutive deny-terminated runs in one lineage trip the bound (the verdict-channel
  pricing, §4.8).

**Integration (re-scoped 2026-07-31 — the real `axon` subprocess exercises isolation/record/CLI;
per-egress decisions go through the §2.1 mock seam, because the real runtime crosses no values,
CORRECTIONS #1):**
- `acc_a4_hermetic_isolated_timeout` (aligned with the corrected §4.6, 2026-07-31 2nd pass) — a
  runaway program run through `axon-ifc run` is killed at the R21 timeout; the child process is gone
  and the flow record is sealed with verdict Denied(timeout). ("No egress leaked" is NOT assertable
  at L1 — the supervisor sees only exit code + stderr, CORRECTIONS #1; that assertion is parked on
  the L1.5 I/O-proxy, §12.1.)
- `acc_a5_deterministic_byte_identical` — run the example twice with the same `--run-id`/seed/policy;
  the two flow `RunRecord` JSON bytes are identical. **Explicitly a network-free run:** the example
  it drives is `exfil.axjob`, whose L1 real-run body is pinned network-free (§5.3) — byte-identity
  is never at the mercy of DNS/proxy nondeterminism.
- `acc_a6_flow_record_tamper_detected` — build a flow record; for a mid-chain flow event mutate a
  field, drop it, reorder, insert → `verify` → `VerifyMismatch` (reusing R21's `record::verify`);
  **also** flips the leading `mediation` event's label (`"none"` → `"full"`) and asserts
  `VerifyMismatch` — the mediation marker lives inside the chain (§3.5), not in a mutable sidecar.
- `acc_a2_example_exfil_denied_and_public_allowed` (re-scoped 2026-07-31; declared-I/O source pinned
  in the 2nd pass) — the exfil scenario is driven through a supervisor constructed with a
  `MockRuntime` whose script is **parsed from the shipped `examples/flows/exfil.axio`** (read under
  `./secret/` → POST to `net:api.model.com` with `claimed_label = public`; §5.3) — NOT a
  hand-written sequence in the test, so the fixture cannot silently diverge from the artifact (the
  §10 step-3c gate check enforces the `.axio` ↔ `.ax` binding) — with `exfil.axflow` as the policy:
  assert exit 8, the record's `egress_deny` event labeled `confidential`, and **no egress
  performed** (the mock's `egress_performed` counter — the honest form of "no network occurred": at
  L1 no real-subprocess mediation exists); the `redact.axio` fixture → exit 0 + a `declassify`
  event then `egress_allow`. The real-subprocess end-to-end version of this test is an L1.5
  deliverable, named in §12.

**User-journey smoke (A1 — drives the REAL CLI exactly as the operator would, via subprocess):**
- `acc_a1_smoke_label_propagate_egress_denied`: (1) `axon-ifc explain exfil.axjob exfil.axflow` →
  asserts the legible egress bound ("net api.model.com: PUBLIC only"); (2) `axon-ifc check secret
  net:api.model.com --policy exfil.axflow` → asserts "✗ DENY" exit 8; (3, re-scoped 2026-07-31)
  `axon-ifc run exfil.axjob exfil.axflow --run-id demo --out <tmp>` → asserts the run is R21-supervised
  and a flow `RunRecord` is sealed at `<tmp>/demo.json` carrying the chained `mediation:"none"`
  flow event (§3.5, §4.6 — the real
  runtime cannot mediate in-run egress at L1; the in-run "⚠ DENIED" assertion is an L1.5
  deliverable, §12); (4) `axon-ifc verify <tmp>/demo.json` → "✓ intact"; (5, re-scoped)
  `axon-ifc declassify secret --to public --by alice --clearance secret` → "✓ declassified … (audited)"
  + the audit event printed. Each step asserts **stdout text AND the on-disk artifact**, not just
  exit codes.

**Quickstart (A3):**
- `acc_a3_quickstart_commands_execute` — extracts the fenced block from `README-axon-ifc.md` and runs
  each line verbatim against the built binary; documented outputs hold.

---

## §8 — Invariants & edge cases

**Invariants (must always hold; assert in tests):**
- **I-1 No-write-down (the core guarantee).** No value egresses to a sink whose clearance does not
  dominate the value's label — `∀ egress: sink_clearance ⊒ value_label`, **independent of which
  capability was granted.** A violation is `Denied` exit 8, audited, and the egress is **not performed**.
- **I-2 Sticky, monotone-up taint.** `combine`/`derive` never lower a label; `join` is the least upper
  bound. The label out of a computation is ⊒ the most-secret label in. (`tainted.ax` `secret_combine`
  semantics, enforced.)
- **I-3 Declassification is the sole, audited, privileged lowering path.** A label drops **only** via
  `declassify`, **only** by a holder whose clearance dominates the value, **only** downward, and
  **always** with an emitted audit event. No implicit declassify exists.
- **I-4 Fail-closed defaults.** Unknown label ⇒ Secret; unlisted sink ⇒ policy `default` (Public);
  unparsable policy ⇒ Malformed (exit 2); **unknown/absent provenance ⇒ the run high-water label
  (I-9), never the program's claim**. Every ambiguity denies; none silently allows. *(The last clause
  was violated by the pre-3rd-pass seam, where unknown label failed closed but unknown provenance
  failed open — see §2.1.)*
- **I-5 Tamper-evidence (inherited).** The flow record reuses R21's hash chain; any mutation/reorder/
  drop breaks `record_digest` and `verify` detects it. The recorder is trusted; the record is
  integrity-checkable (no signature — documented, A6).
- **I-6 Bounded covert bandwidth.** Secret-adjacent **attempts** (allowed *and* denied) to a
  non-Secret sink are counted **in both operations and bytes**, accumulated over
  `FlowPolicy.covert_scope` (default **Lineage**, not per-run); past `covert_cap` **or**
  `covert_byte_cap` they are refused; every one is recorded with a content digest (§3.4). Declassify
  is separately capped (`declass_cap`/`declass_byte_cap`). (Bandwidth is *bounded + monitored*, not
  *proven zero* — the honest limit of L1. Aggregation/alarming across the lineage is §12.3 F3,
  **not** in v1: a tripped ceiling is visible in a record nobody may be reading, §1.8.)
- **I-7 Determinism.** Same (labeled inputs, policy, seed) ⇒ byte-identical flow record (no ambient
  clock/random in core).
- **I-8 Orthogonal axes preserved.** Confidentiality (R25) and trust/integrity (`source.ax`) are kept
  separate — R25 never conflates "low trust" with "high confidentiality"; a value can be low-trust *and*
  public, or high-trust *and* secret.
- **I-9 Provenance is monitor-derived, never callee-asserted (added 2026-07-31, 3rd pass).** A value's
  association with a mediated ingress is established **only** by an id the monitor itself minted. An
  egress presenting `None` or an unminted id is evaluated at the run **high-water** label
  (`join` of every ingress stamp so far), so a program can raise its effective label but can **never
  lower it by omitting or misdirecting provenance**. The claim is an input to a `join`, never a
  substitute for evidence.
- **I-10 No overclaim (added 2026-07-31, 3rd pass).** No artifact R25 ships — record, CLI output,
  README, or DoD — may state a guarantee stronger than the §1.7 guarantee card, and the card's
  `satisfies_VISION_OS_G4` / `certificate_checked` are `false` at L1. `mediation:"none"` records
  attest supervision + sealing only (§3.5).

**Edge cases the implementer MUST handle (named, with resolution):**
- A program **with the net capability fully granted** that emits a Secret → **Denied** (I-1) — the
  capability is irrelevant to the flow decision (the entire pillar; do not let "but it's granted" leak
  a byte).
- A value laundered through N pure `derive`s before a sink → the sink sees the **propagated** (still-
  secret) label; laundering can't lower it (I-2). Closes the transitive-laundering class for flow.
- A declassify by an **under-cleared** holder → Denied exit 8 (I-3), never a silent downgrade.
- An **unlisted** sink target → `default` clearance (Public), so any non-public value is denied (I-4) —
  no "default allow" hole.
- A program that drips secret bits as "public" values in a loop → counted; past `covert_cap`, refused
  (I-6). The residual ≤ `covert_cap` emissions is the honestly-documented L3 gap, monitored in the record.
- `exec` egress: a secret passed as a process arg/stdin is guarded identically to `net`/`fs_write`
  (exec is a sink, not exempt).
- A program performing raw I/O **outside** the supervisor's mediated sinks → out of L1's reach.
  ~~**but** the program is R21-sandboxed to only the granted (all mediated) sinks~~ — **that clause is
  STRUCK (2026-07-31, 3rd pass): the R21 sandbox ceiling is escapable from inside a generated
  program (§12.2), so it may not be cited as the residual containment.** The honest resolution: at L1
  this case is **uncontained**, and the unbypassable version is L1.5/L2 (§4.9), gated additionally on
  R21 landing the `sandbox_run` attenuation fix (§12.2) — stated, not hidden.
- Corrupt/edited stored flow record on `verify` → exit 11 (`Verdict::VerifyMismatch` — renumbered
  from 9 by R27, corrected 2026-07-31), never a silent pass (inherited from R21).

---

## §9 — Quickstart (`README-axon-ifc.md`; these exact commands are executed by `acc_a3`)
```bash
# Build
cargo build -p axon-ifc --bin axon-ifc
cargo build -p axon-os  --bin axon-os    # R25 composes over the R21 supervisor

# 1. See, in plain English, exactly what each egress sink may RECEIVE (no execution):
axon-ifc explain examples/flows/exfil.axjob examples/flows/exfil.axflow

# 2. Ask the sink guard directly — may a SECRET go to the model endpoint? (no, exit 8):
axon-ifc check secret net:api.model.com --policy examples/flows/exfil.axflow ; echo "exit=$?"

# 3. Supervise the job that's GRANTED net but whose DECLARED I/O would leak a confidential value.
#    HERMETIC at L1 (2nd-pass correction): exfil.ax's real-run body is pinned NETWORK-FREE (§5.3) —
#    it computes and prints the intended egress, deterministically, exit 0; the POST exists only in
#    exfil.axio, where the mock seam denies it (the guard's denial is shown by step 2's `check`).
#    The run is R21-sandboxed and a flow record is sealed carrying the chained mediation:"none"
#    event — in-run per-egress denial against the real runtime is the L1.5 follow-up:
axon-ifc run examples/flows/exfil.axjob examples/flows/exfil.axflow --run-id demo --out ./runs
echo "exit=$?"

# 4. Confirm the flow record (the egress ledger) hasn't been tampered with (tamper => exit 11):
axon-ifc verify ./runs/demo.json

# 5. The explicit, audited declassification path — the ONLY way a label may drop:
axon-ifc declassify secret --to public --by alice --clearance secret
```

---

## §10 — Acceptance gate (pinned; FAILS if any check is missing or stubbed)

`scripts/r25_ifc_acceptance_gate.sh` is the single source of "done." It MUST:
1. **Presence check** — `grep` the test sources and assert every named check from §0 exists:
   `acc_a1_smoke_label_propagate_egress_denied`, `acc_a2_example_exfil_denied_and_public_allowed`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_hermetic_isolated_timeout`,
   `acc_a5_deterministic_byte_identical`, `acc_a6_flow_record_tamper_detected`,
   `secret_cannot_reach_sink_below_clearance_even_with_cap`, `combine_is_sticky_label_never_drops`,
   `undeclassified_secret_to_public_sink_refused`, `declassify_is_explicit_privileged_and_audited`,
   `covert_drip_is_bandwidth_bounded`, `mislabeled_secret_still_caught_at_source_stamp`,
   and (added 3rd pass) `unprovenanced_egress_after_secret_ingress_is_denied`,
   `declassify_volume_is_bounded`, `covert_bound_is_bytes_and_lineage_scoped`,
   `guarantee_card_does_not_overclaim`. Any missing name → **gate fails**.
   **Note on what steps 1–3c can and cannot do (added 2026-07-31, 3rd pass):** every non-vacuity
   check in this gate is a **grep** — a syntactic proxy for a semantic property — while §12's stated
   audience is "an implementer who builds strictly against this document," which in this repo is an
   autonomous agent whose reward signal is `r25_ifc_acceptance_gate.sh` exiting 0. A stronger
   implementer satisfies all of these greps with tests asserting **weaker** properties than the names
   imply; this spec's own 2nd-pass `mislabeled_secret_still_caught_at_source_stamp` was already that
   shape (named for the adversarial-synthesizer case, testing a half-honest one). Step **3d** is the
   grep-immune control and is **not optional**.
2. **Anti-stub check** — assert each acceptance test body contains a real assertion and is not
   `#[ignore]`d / `todo!()` / `assert!(true)` (grep for those anti-patterns → fail).
3. **Dependency-direction check** — assert (via `cargo tree -p axon-os`) that `axon-os` does **NOT**
   depend on `axon-ifc` (the cycle would invert the egress layering); if it does → **fail**.
3b. **Seam-presence check (added 2026-07-31)** — the direction check alone only proves no Cargo
   cycle; additionally assert the §2.1 axon-os delta exists and behaves: grep for the
   `EgressMonitor` trait in `crates/axon-os/src/` and for the pinned axon-os-side tests
   `mock_runtime_consults_injected_monitor` + `mock_runtime_noop_when_no_monitor`, and run
   `cargo test -p axon-os` green. Any missing → **fail**.
3c. **Artifact-binding check (added 2026-07-31, 2nd pass — keeps A2 non-vacuous at L1)** — for each
   example flow, assert (a) `examples/flows/<name>.axio` exists and parses; (b) **CONTAINMENT
   (strengthened 2026-07-31, 3rd pass — was presence-only, and therefore one-directional):**
   re-run the **AST extractor** (§5.3 — `scan_effects` / R22's `DeclaredEffects`) over the `.ax`
   source and assert the checked-in `.axio` **contains every egress the AST can reach** — an
   AST-reachable egress missing from the `.axio` → **fail** (that was the understatement hole),
   and an `.axio` entry with no AST counterpart → **fail** (drift). The old presence check (every
   declared `target` appears verbatim in the source) is retained as a cheap additional assertion but
   is **not** sufficient on its own; (c) the **network-free L1 pin** (§5.3): the `.ax` source contains
   no live net builtin call (`http_get`/`http_post`/`http_sse`); and (d) the acc_a2 test source builds
   its `MockRuntime` script by parsing the `.axio` (grep for the parse call), not from an inline
   literal sequence. Any divergence → **fail**. (Still best-effort and honestly labeled: a
   source-level scan is itself defeasible — R21's own comment concedes "the static gate ... could be
   fooled" — so containment raises the cost of understating I/O without closing it. Full
   artifact↔behavior identity is the L1.5 real-subprocess test; a real certificate is §12.3 F2.)
3d. **Mutation / negative-control check (added 2026-07-31, 3rd pass — the ONLY step in this gate a
   grep cannot satisfy; MUST NOT be dropped or made advisory).** Apply each of the following source
   mutations in a scratch copy, rebuild, run `cargo test -p axon-ifc`, and assert the suite goes
   **RED**; restore. A mutant that **survives** (suite still green) → **gate fails**, naming it:
   - **M1** — invert `label::dominates` (`a ⊒ b` → `a ⊏ b`). Kills: the entire sink guard.
   - **M2** — make `on_egress` decide on `claimed_label` alone (drop the stamp lookup **and** the
     high-water fallback). Kills: `mislabeled_secret_still_caught_at_source_stamp`,
     `unprovenanced_egress_after_secret_ingress_is_denied`.
   - **M3** — replace `label::join` with `min` (the greatest lower bound). Kills: sticky taint.
   - **M4** — make the `None`/unminted-id path fall back to the claim (the pre-3rd-pass fail-OPEN
     behavior). Kills: `unprovenanced_egress_after_secret_ingress_is_denied` specifically — this
     mutant is the regression guard for I-9.
   - **M5** — drop the declassify meter (accept unlimited ops/bytes). Kills:
     `declassify_volume_is_bounded`.
   A handful of `sed` + `cargo test` invocations; it tests the properties §0 actually claims rather
   than the presence of their names. *(Repo precedent: "verify new checks against a real corpus" —
   contrived fixtures pass checks that real inputs break.)*
3e. **Anti-overclaim check (added 2026-07-31, 3rd pass).** Assert `axon-ifc --guarantee` emits schema
   `axon-ifc-guarantee/1` with `satisfies_VISION_OS_G4 == false` and `certificate_checked == false`,
   and that `does_NOT_protect_against` names both the label lie **and** the provenance lie. Assert no
   file under `governance/`, `README-axon-ifc.md`, or `VISION_OS.md` marks **G4** satisfied while
   citing `R25-information-flow-monitor` (grep). Any violation → **fail**.
4. **Run** `cargo test -p axon-ifc` (all green) **and** execute the §9 quickstart block against the
   built binary (A3) **and** run `acc_a1` driving the real CLI.
5. **Reproducibility** — run `acc_a5` twice and diff the two flow records byte-for-byte.
6. Exit 0 only if all of the above pass; print which check failed otherwise.
Wire `r25_ifc_acceptance_gate.sh` into the repo's `gate.sh --strict`.

---

## §11 — Definition of Done
**Per slice (S1–S10):** the slice's named tests were written first, were seen to fail, now pass; the
full `axon-ifc` suite is green; no regression in the workspace.
**Per milestone (R25 complete):** `cargo build -p axon-ifc` produces the `axon-ifc` binary; the real
example flows run end-to-end; **`acc_a1` passes through the real CLI**; a `Secret` value is refused at a
sink below its clearance **even with the capability granted** (`secret_cannot_reach_sink_below_
clearance_even_with_cap`); taint is sticky (`combine_is_sticky_label_never_drops`); a mislabeled/
omitted-label secret read through a mediated ingress is caught by the supervisor's source-stamp
**when the mediated-ingress provenance is honestly reported, and by the run high-water label when it
is not** (`mislabeled_secret_still_caught_at_source_stamp` +
`unprovenanced_egress_after_secret_ingress_is_denied`) — the precondition is named because without it
this line reads as a guarantee against a lying synthesizer, which L1 does not provide;
declassification is explicit/privileged/audited **and metered + content-attributed**
(`declassify_volume_is_bounded`); covert bandwidth is bounded **in bytes over a lineage, charging
denials** (`covert_bound_is_bytes_and_lineage_scoped`); the flow record is deterministic
(`acc_a5`) and tamper-evident (`acc_a6`); `axon-os` does **not** depend on `axon-ifc`; the §1.7
guarantee card ships and does not overclaim (`guarantee_card_does_not_overclaim`); and
`scripts/r25_ifc_acceptance_gate.sh` exits 0 with every §0 check green **including the mutation step
3d and the anti-overclaim step 3e**. Only then is R25 **v1** done.

**DoD PROHIBITION (normative, added 2026-07-31, 3rd pass).** R25 v1 being "done" means **the
mechanism + data model milestone is done**, not that the *"what can LEAK"* pillar is closed. No
document — `VISION_OS.md`, `ROADMAP.md`, a status file, a release note, or a gate script — may mark
**G4** ("secrets provably can't reach sub-clearance sinks") or v1's *"info-flow bound
certificate-checked and attested"* done-when as satisfied on the strength of R25 v1. Both remain
**open until L2 lands** (interpreter-stamped labels — the only rung that survives a synthesizer that
lies), and `certificate_checked` additionally until the static pass of §12.3 F2 lands. §10 step 3e
enforces this by grep. Marking G4 done from L1 is the single highest-severity failure mode of this
spec, because L1's mechanism is genuinely good and therefore easy to mistake for the guarantee.

---

## §12 — Notes for the implementer (do NOT deviate without updating this spec)
- **Reuse R21's `Grant`, `EffectSet`, `Verdict`, and `record::{build,verify,RawEvent,
  AuditEvent}`** (`crates/axon-os/src/{grant,verdict,record}.rs`). Do **not** reinvent the exit-code
  scheme or the hash chain — import them. The `AuditEvent` already carries a `label` field; that field
  is R25's egress ledger. **The lattice is the ONE exception (§3.1):** the shipped grant `Label` is
  3-rung and R25's flow lattice is 4-rung, so `axon-ifc` defines its **own** 4-rung `Label` and bridges
  the grant ceiling in with a total `From<axon_os::grant::Label>` — do **not** edit the TCB
  `axon-os::Label` (that 4th-rung TCB delta is the deferred option (a)), and do **not** alias the two
  types together.
- **Reuse the shipped value-level semantics verbatim, just enforced:** `join` = `secret_combine`'s max
  (`examples/stdlib/tainted.ax` 153–158); `dominates` = `secret_can_flow_to` (line 139); the audited
  `declassify` = `secret_declassify` (line 145) + the authority check + the audit event. R25's job is
  to make these **mandatory at the TCB egress**, not to invent new logic.
- **Keep `label/labeled/policy/guard/declassify/meter/flowrec/monitor` pure.** If you reach for
  `std::fs`, `SystemTime`, `rand`, or `std::env` there, you are in the wrong module — it belongs in
  `cli.rs`/`main.rs` or behind R21's `Runtime`.
- **Dependency direction is sacred (§2):** `axon-ifc → axon-os`, never the reverse. The supervisor
  consults the egress monitor through a trait **declared in `axon-os`** and implemented in `axon-ifc`
  (trait low, impl high) so there is no cycle. The gate (§10 step 3) enforces this.
- **Fail closed, always (I-4):** unknown label ⇒ Secret; unlisted sink ⇒ Public default; ambiguity ⇒
  Deny. Never "allow on doubt."
- **Be honest about the fragment.** R25 v1 is L1 (library-enforced explicit-flow + mandatory sink guard
  + bounded covert bandwidth). It is **not** full non-interference: no implicit-flow tracking, no
  interpreter-wide auto-propagation, no side channels (§1.2). Those are L2/L3 follow-ups and
  `VISION_OS.md` §4.3 — named, not silently implied. Do not claim a guarantee R25 doesn't provide.
  **And be honest about the POPULATION, not just the fragment (3rd pass):** the fragment's protected
  population — programs that label and report provenance honestly — is empty under this repo's own
  IR thesis (§1.5 headline). Ship the §1.7 guarantee card, obey the §11 G4 prohibition, and treat
  §12.4's questions as unanswered rather than resolving them in code.
- **Declassify is the ONLY lowering path** (I-3). There must be no other function — not `combine`, not
  `derive`, not the guard — that can lower a label. If you find yourself lowering a label anywhere else,
  that is a soundness hole.

### 12.1 Open blockers / corrections ledger (added 2026-07-31)
- **BLOCKER — L1.5 I/O-proxy (real-runtime in-run mediation).** The shipped `AxonCoreRuntime` has no
  per-action ingress/egress seam (CORRECTIONS #1; `runtime.rs` `run_sandboxed` is a single opaque
  subprocess). Real-runtime enforcement of the sink guard requires an interpreter↔supervisor IPC
  protocol + `axon-core` changes — its own spec, to be written before any real-runtime in-run
  guarantee is claimed. Deferred deliverables parked on it: the real-subprocess `acc_a2` end-to-end,
  acc_a1's in-run "⚠ DENIED" step, the `.ax`-side `Labeled`/`declassify` surface with its
  serialized crossing format (§4.9), the **"no egress leaked" assertion of acc_a4** (unverifiable
  at L1 — the supervisor sees only exit code + stderr; §7, 2nd pass), and the **live-POST variant
  of `exfil.ax`** (the L1 example is pinned network-free, §5.3 — a real unmediated POST attempt
  would itself be the leak).
- **Declared axon-os delta (§2.1).** R25 edits `axon-os` (the `EgressMonitor` trait + `MockRuntime`
  consultation + two pinned tests). "No TCB edit" is scoped to the `Label` lattice only.
- **Exit-code correction.** Verify-tamper is exit **11** (`Verdict::VerifyMismatch`), not 9 — R27
  renumbered (9 = `ResourceBound`). Never invent a new band; track `verdict.rs`.
- **Spec-ID collision.** `R25` is dual-claimed with `R25-zephyr-target`; use the full slug everywhere
  and the disambiguated gate name `r25_ifc_acceptance_gate.sh`.
- **RE-SCOPED 2026-07-31, 3rd pass — the L1.5 BLOCKER above is NARROWER than stated.** The claim
  "requires an interpreter↔supervisor IPC protocol + `axon-core` changes — its own spec" is true of
  the **supervisor boundary** but false of the point R21 actually relies on for containment; see
  **§12.3 F1**, which splits L1.5 into an in-process seam (reachable) and an out-of-process proxy
  (the design effort). This does **not** move anything into v1.

### 12.2 PRECONDITION owed by R21 — the sandbox ceiling is escapable from inside a generated program
R25 previously leaned on R21's sandbox twice (§4.9 "R21's sandbox still bounds *which* channels the
subprocess can reach"; §8's edge case "the program is R21-sandboxed to only the granted (all
mediated) sinks"). **Both sentences are struck** because the fence does not hold against a program
that reads the in-repo enforcement source — which a synthesizer does. The mechanism, verified against
the **built interpreter**, not inferred:
- The fence is `AxonCoreRuntime::run_sandboxed` → `wrap_in_sandbox`
  (`crates/axon-os/src/runtime.rs:253-275, 299-318`; its own comment: *"THIS is what actually
  contains"*) → the per-builtin ceiling check at `crates/axon-core/src/interp/builtins.rs:185-207`.
- That check **exempts `sandbox_create`/`sandbox_run` by name** (to avoid infinite regress), and
  `sandbox_run` **replaces** the active ceiling with the new sandbox's allowed set with **no
  intersection against the currently active one** (`let prev_sandbox =
  self.active_sandbox.replace(sb_handle)`, `builtins.rs:2836`).
- **Observed:** a `__job_entry` calling `random_i64` under an empty outer ceiling is refused
  (*"sandbox violation: builtin `random_i64` requires effect `Random` which is not in the active
  sandbox's allowed set {}"*, exit 8); the **identical** program that first calls
  `principal_root(...)` + `sandbox_create(p, "Net,AI,IO,Random,Chan")` + `sandbox_run(sb, "leak", 0)`
  **executes the effect and prints**.
This is **not R25's bug** — it is R21/Phase-9's — but it voids the residual R25 documented as its
safety net and it blocks any in-interpreter enforcement design (including §12.3 F1). **Raise it to
R21/Phase-9 as a containment defect.** The fix R21 owes, stated so R25 can depend on it:
`sandbox_run` MUST **intersect** the requested allowed-set with the currently active ceiling
(monotone attenuation only — the same J∩S discipline R21 already applies to principal grants), and
`sandbox_create` SHOULD be refused while a ceiling is active unless the requested set is a subset.
Until that lands, **R25 cites no capability fence at all** (§4.9, §8), and §12.3 F1 is blocked on it.

### 12.3 Named future slices (scoped, NOT in v1 — machinery the repo already ships)
Each is a scoped follow-up with a named starting point in the tree, not an aspiration. None is folded
into v1; none may be cited as delivering a v1 guarantee.
- **F1 — L1.5a: in-process sink guard at the builtin-dispatch seam.** §12.1 calls L1.5 a blocker
  needing IPC because "the supervisor sees no values" — true of the *supervisor boundary*, false of
  the point R21 relies on: `crates/axon-core/src/interp/builtins.rs:185-207` intercepts **every**
  builtin call by name **with the real argument `Value`s in hand** and refuses it when its effect row
  exceeds the active ceiling — the site R21's own comment calls "what actually contains"
  (`runtime.rs:299-305`). R21 already ships policy *into* the process by source injection
  (`wrap_in_sandbox`) and env (`AXON_SEED`, `AXON_OS_TIMEOUT_MS`). A `sink_guard` consult at that
  site, with the `.axflow` loaded from an env-named path and flow `RawEvent`s appended to a file the
  supervisor seals, delivers **real per-egress denial and real ingress stamping with zero protocol**,
  no `Labeled<T>` crossing format, and no cross-process perf question. The trust argument is the one
  R21 already accepted for capabilities: the interpreter is TCB. Split accordingly:
  **F1a = in-process seam (reachable now); F1b = out-of-process I/O-proxy, only if/when the
  interpreter must leave the TCB** (that remains the design effort §12.1 describes).
  **Preconditions, both hard:** (i) the §12.2 sandbox-escape fix, and (ii) the policy path + flow-event
  sink must be unreachable and unforgeable from program builtins. §2.1's `EgressMonitor` trait stays
  the abstract seam so the L1 mock tests carry over unchanged.
- **F2 — static explicit-flow pass over the `.ax` AST (the only path to a *certificate*).** R25 v1
  analyses the generated AST **nowhere** (§4.9: "an `.ax` program cannot interact with the monitor at
  all"; §10 step 3c is greps), while §4.8's honest limit is exactly what
  `crates/axon-core/src/checker.rs` **already computes on the trust axis**: a taint fixpoint with
  `taint_returning_params`, transitive param→sink taint, local-binding taint including field/index
  reads, and MethodCall coverage (`checker.rs:345-360, 4529-4640`) — built to close the
  launder-through-a-helper class, and it **follows plain arithmetic and string interpolation**, which
  the runtime label graph cannot. Two facts make this the highest-leverage unused lever: every
  program is machine-generated, so the typed AST is **always** available before execution
  (`ROADMAP.md` §2.4 already designates it "the legal/audit artifact"), and `VISION_OS` v1 demands
  the info-flow bound be **certificate-checked** — i.e. static — which a runtime monitor cannot
  deliver by construction. Scope: reuse the checker's taint fixpoint on the **confidentiality** axis,
  with `FlowPolicy` supplying source/sink classification, refusing at `axon check` time under a new
  E-code, and emitting the derived flow set as a machine-generated artifact (which also **replaces**
  the §5.3 `.axio` extractor). Even as an advisory pre-pass it strictly dominates §10 step 3c's grep.
  This is the one part of R25's problem a stronger generator **cannot** escape by lying in a runtime
  argument — the program's own AST betrays the flow.
- **F3 — ledger + live-monitor integration (the answer to §1.8).** R25 v1 emits
  `covert_drip`/`covert_blocked`/`declassify`/`egress_deny` **only** into a per-run sealed record that
  nothing aggregates or watches. The repo ships both halves: `crates/axon-audit` (append-only,
  hash-chained JSONL capability ledger, `append_global`) and `crates/axon-os/src/monitor.rs` (R29
  continuous compliance monitor over that ledger, which trips the R27 kill switch on violation,
  fail-closed by design). Scope: emit flow decisions to the R28 ledger **in addition to** the sealed
  record, and specify an R29 rule set over them — declassify by an unexpected holder; drip count or
  bytes over threshold; N consecutive deny-terminated runs in one lineage ⇒ **trip**. Acceptance:
  a drip exceeding the ceiling is **visible to the live monitor**, not merely present in a file.
  (This is also where the §4.8 lineage counters would naturally live; see §12.4 Q2.)

### 12.4 Open strategic questions (UNRESOLVED — do not invent an answer while implementing)
These are recorded because guessing at them in code would silently re-scope the spec. Each needs an
explicit decision (spec amendment) before the affected work starts.
- **Q1 — Should R25 v1 ship at all as a standalone milestone, or only as the substrate of L1.5/L2?**
  Given the population problem (§1.5 headline), an L1-only release delivers a real mechanism with
  ~zero security effect against the actual code population, and its main risk is being *mistaken*
  for the guarantee. The §1.7 card + §11 prohibition are the mitigation taken here; whether that is
  sufficient, or whether R25 should be held until F1a lands, is an **owner decision**, not the
  implementer's.
- **Q2 — Where do lineage-scoped counters live, and who is authoritative?** §4.8 now accumulates over
  a lineage, but §2's purity rule forbids the core reading state. The v1-safe reading is "injected
  input, like `run_id`, read/written by `cli.rs`" — but that makes the counter file a **new integrity
  target** (a program that can truncate it resets the bound), which argues for R27's `ResourceLedger`
  or the R28 chained ledger (F3) as the authority. Unresolved: ownership, tamper model, and what
  happens when the counter store is missing (fail closed = every run denied; fail open = no bound).
- **Q3 — Does declassify need a *policy language* after all?** §1.2 puts it out of scope and defers to
  R22 approval, but §3.4's finding is that declassify is the shortest path *through* R25 for an
  optimizing generator. Metering (taken here) prices the channel; it does not govern *who* may hold
  an authority or *for what*. Whether that governance belongs in R22, in a new spec, or as an R25 v2
  surface is open.
- **Q4 — What is the monitor's own verdict channel worth, and can it ever be closed?** §4.8 prices
  denials into the covert meter, but exit-code observability is inherent to a supervisor that reports
  outcomes. Closing it would require uniform verdicts (denials indistinguishable from success), which
  destroys operator legibility — a direct conflict with §1.3. Unresolved trade.
- **Q5 — Who audits the auditor when nobody reads the records?** §1.8 states the expiring assumption;
  F3 proposes machinery. But an R29 rule set is itself a policy an optimizing generator can be
  shaped around, and R29 tripping the R27 kill switch turns a false positive into a halted fleet.
  The threshold-setting problem is unowned.
