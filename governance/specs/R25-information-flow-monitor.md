# Tech Spec — R25: Information-Flow / Egress Monitor ("what can LEAK")

**Spec ID:** `R25-information-flow-monitor`
**Status:** 📝 Draft (2026-06-27)
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
| **A2** | Real runnable example artifact (a real job that leaks, not a toy in the test dir) | §5.6, §7 | `acc_a2_example_exfil_denied_and_public_allowed` |
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
| **Gate** | The acceptance gate itself fails if any check above is missing/stubbed | §10 | `scripts/r25_acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does
R25 is an **information-flow monitor** that sits at the egress boundary of a supervised run and:

1. **Labels** values with a confidentiality level (`Public < Internal < Confidential < Secret`,
   matching the **4-rung** `cl_*` ladder in `examples/stdlib/tainted.ax`). This is `axon-ifc`'s own
   4-rung lattice (§3.1); the shipped **3-rung** grant `Label` ceiling in `crates/axon-os/src/grant.rs`
   injects via an explicit total `From<axon_os::grant::Label>` (no TCB edit; the two stay distinct types).
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
  the program (or the R22 synthesizer) constructs values as `Labeled<T>` and the supervisor consults
  `sink_guard` at the egress wrapper. What is **library-enforced** vs **needs interpreter wiring** is
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
to call a model) but processes **confidential** input. They need a *provable, audited* guarantee that
the confidential bytes cannot egress through the granted channel — and a *bounded, monitored* residual
where some egress is unavoidable — without having to trust the AI's good behavior.

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
| **L1 — R25 v1** | TCB-owned `sink_guard` the **supervisor mandatorily consults at every egress wrapper**; explicit `Labeled<T>` propagation; audited declassify; bandwidth bound. **Library-enforced**: the program/synthesizer labels values; the egress wrapper cannot be bypassed *for the sinks the supervisor mediates*. | **THIS SPEC** |
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

This is the policy-side input twin of §3.3's *sink*-side `SinkClearance`: the policy now declares both
*how secret each born input is* (input rules, stamped by the supervisor) and *how secret each sink may
receive* (sink rules, checked by the guard).

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
  exfil.axjob + exfil.ax       A real job granted `net` that tries to leak a SECRET (A2 negative).
  redact.axjob + redact.ax     The same job that DECLASSIFIES (redacts) first → allowed (A2 positive).
scripts/r25_acceptance_gate.sh  The pinned gate (§10).
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
one R21-side touchpoint (§4.9: the supervisor calling `sink_guard` at its egress wrapper) is wired by
having R21 accept an injected `&dyn EgressMonitor` *interface defined in axon-os*, which `axon-ifc`
implements — the trait lives low, the implementation lives high (no cycle).

---

## §3 — Data model

### 3.1 `Label` — the flow lattice (axon-ifc's OWN 4-rung lattice + a total `From<axon_os::Label>`)
The confidentiality lattice is a total order; a higher label is **more restrictive** (more secret).
**Contradiction resolved (do NOT hand-wave an "alias band"):** the shipped `axon-os::grant::Label` is
**3-rung** (`Public=0 < Internal=1 < Secret=2`, `grant.rs`/`record.rs`) while the stdlib ladder of
`examples/stdlib/tainted.ax` is **4-rung** (`cl_public=0 < cl_internal=1 < cl_confidential=2 <
cl_secret=3`). R25 does **not** edit the TCB `axon-os::Label` (no TCB delta is taken here); instead
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
}
Origin = Source { kind: String }   // e.g. "fs_read:./secret/"  — the labeled input
       | Combined                  // derived by joining ≥2 labeled values (sticky)
       | Declassified { by: String, from: Label }   // an audited label-lowering (§3.4)
```
Constructors mirror the shipped value-level forms (`tainted.ax` `secret`/`secret_public`/…):
`labeled_public(v)`, `labeled_internal(v)`, `labeled_confidential(v)`, `labeled_secret(v)`,
`labeled(v, level)`. The serialized form (`axon-ifc-labeled/1`, JSON) is `{value, label, origin}`.

### 3.3 `FlowPolicy` — the per-sink clearance ceiling + bandwidth ceiling
A sink's **clearance** is the *highest label it may receive*. An external `net`/`fs_write` to an
untrusted destination has clearance `Public(0)` (it may receive only public data — the classic
exfiltration guard, `tainted.ax` lines 277–283). The policy maps each egress sink to its clearance and
a covert-bandwidth cap:
```
FlowPolicy {
    inputs:       Vec<InputClass>,      // per source: how secret is a value BORN here? (supervisor stamps; §1.6)
    sinks:        Vec<SinkClearance>,   // per concrete sink: how secret may the bytes be?
    covert_cap:   u32,                  // max distinct secret-derived emissions per run (§4.8); 0 = none
    input_default: Label,               // born-label for any source not matched; FAIL-CLOSED default = Secret(3)
    default:      Label,                // clearance for any sink not listed; FAIL-CLOSED default = Public(0)
}
InputClass   { source: Source, prefix: String, born: Label }   // the supervisor's source-stamp rule (§1.6)
Source = FsRead | AiComplete | NetRead | ReadLine              // the ingress axes (where labels are BORN)
SinkClearance { axis: Axis, target: String, clearance: Label }
Axis = FsWrite | Net | Exec            // the egress axes (the sinks; mirrors EffectSet)
```
Serialized form (`.axflow` TOML, sitting alongside R21's `.axjob`):
```toml
covert_cap    = 16
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
`covert_cap` ≥ 0; `prefix`/`target` have no `..` component (path traversal, mirroring R21 §3.1 / E1001).
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

### 3.5 `FlowRecord` + the labeled `AuditEvent` (tamper-evident; reuse R21's chain)
R25 does **not** invent a new chain — it emits R21 `RawEvent`s (which already carry a `label` field,
`record.rs` lines 20–37) and seals them with `axon_os::record::build`, producing a standard
`RunRecord` (schema `axon-os-record/1`) whose events are the **flow decisions**:
```
A flow decision becomes a RawEvent:
  action  = "egress_allow" | "egress_deny" | "declassify" | "covert_drip" | "covert_blocked"
  target  = the concrete sink (host/path) or "" for declassify
  caps_used = the egress axis (EffectSet) the decision concerned
  label   = the value's label at the decision point (e.g. "secret" on a denied egress)
```
Because R25 reuses `axon_os::record::{build, verify}`, the chain is **already** tamper-evident: any
mutation/reorder/drop of a flow event breaks `record_digest` and `verify` (proven by `record.rs`'s
`acc_a6_record_tamper_detected`; R25's `acc_a6_flow_record_tamper_detected` exercises the same chain
over *flow* events). **Authenticated:** integrity + ordering of the recorded flow decisions.
**NOT authenticated (documented for A6):** *who* produced the record (no signature) and *that the
monitor saw every flow* (L1 trusts the program to route egress through the supervisor's wrappers; the
unbypassable version is L2, §4.9). No HW root of trust (`VISION_OS.md` §5 G6).

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
- **L1, this spec — supervisor-stamped at the ingress wrapper:** for every value entering through a
  mediated R21 ingress wrapper (`fs_read`/`ai_complete`/`net_read`/`read_line`), the monitor applies the
  matching `InputClass` and stamps the born label *before handing the value to the program*. **A
  synthesizer that mislabels or omits its in-program `Labeled<T>` cannot defeat this** — the guard reads
  the supervisor-stamped label, not the program's claim. This is `mislabeled_secret_still_caught_at_
  source_stamp`.
- **L1 residual / L2 deferred:** a value the program **synthesizes internally** (never crossing a
  mediated ingress) still has only its program-chosen label — closing *that* needs the interpreter to
  stamp every value (L2, §4.9); the §4.8 covert bound is the interim cap on it.

### 4.2 Label propagation — the sticky-taint rule (`labeled::combine`/`derive`) — Core
Step by step, the rule mirrors `secret_combine`'s "take the max level" (`tainted.ax` 153–158) and
`src_join`'s "least-trusted wins" *shape* (`source.ax` 41–43), but on the **confidentiality** lattice
joining **upward** (to *more* restrictive):
1. `combine(a: Labeled<T>, b: Labeled<U>, f) -> Labeled<V>`: the result value is `f(a.value, b.value)`
   and its label is `label::join(a.label, b.label)` — the **higher** (more restrictive) of the two,
   `origin = Combined`. **Taint is STICKY and MONOTONE-UP: combining a Secret with a Public yields a
   Secret; a label NEVER drops through `combine`/`derive`.** (Lowering happens *only* via the audited
   `declassify`, §4.4.) This is the headline `combine_is_sticky_label_never_drops`.
2. `derive(a, f) -> Labeled<V>`: a unary transform preserves the label (`tainted_map`'s
   trust-preservation, `tainted.ax` 78–80, applied to confidentiality) — `origin` unchanged.
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
   only then is the lowered `Labeled{to, origin: Declassified{by, from}}` returned. Because
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
timeout, no leaked handles — R21 §4.4). R25's contribution is the **egress wrapper**: each
capability-bearing action the runtime would perform is routed through `monitor::on_egress` →
`sink_guard` → `meter` → `flowrec` **before** it executes. A guarded-denied egress is **not performed**
(fail closed) and is recorded as `egress_deny`. `acc_a4_hermetic_isolated_timeout` reuses R21's runaway-
program test through the R25 `run` path, asserting the child is killed and no egress leaked.

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
1. The `CovertMeter` counts, per run, the number of **distinct emissions to a non-Secret sink whose
   value is derived from a Secret input** (an emission the guard *allowed* because the program
   declassified-or the value's explicit label was already public, but whose provenance graph touched a
   secret). This is a coarse over-approximation: a high count of public-but-secret-adjacent emissions
   is the bandwidth available to a drip.
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

### 4.9 R21 / interpreter touchpoints (what is library-enforced vs needs wiring — be precise)
- **L1, library-enforced (this spec):** The supervisor's egress points (`fs_write`/`net`/`exec`
  wrappers in R21's runtime) call an injected `EgressMonitor` **interface declared in `axon-os`** (the
  trait lives low; `axon-ifc` provides the impl — so `axon-os` has **no** dependency on `axon-ifc`,
  preserving the dependency direction §2). The program/synthesizer constructs `Labeled<T>` values and
  hands them to the egress API. *Within the sinks the supervisor mediates*, the guard is mandatory and
  unbypassable. **Gap (honest):** a program that performs raw I/O *not* through the supervisor's
  wrappers is outside L1's reach — which is exactly why the program runs *sandboxed by R21* (it can
  only reach the granted sinks, all of which are mediated).
- **L2, deferred (named follow-up):** automatic label stamping inside the **interpreter** —
  `fs_read`/`ai_complete`/`read_line` builtins stamp source labels; arithmetic/string builtins
  propagate `join` automatically; the sink builtins call `sink_guard` unconditionally. This removes the
  "program forgot to label" gap and is the path to true kernel enforcement. R25 v1 specs the data model
  + guard so that L2 is a *wiring* change (stamp + propagate + call the same `sink_guard`), not a
  redesign.
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

// The egress monitor the supervisor consults (impl of the axon-os-declared trait):
pub struct Monitor { policy: FlowPolicy, meter: CovertMeter, events: Vec<RawEvent> }
impl axon_os::EgressMonitor for Monitor {
    fn on_egress(&mut self, axis:Axis, target:&str, value_label:Label) -> Decision; // guard → meter → record
}
```

### 5.2 CLI (`axon-ifc`; every subcommand has `--help`; output legible, not just exit codes)
```
axon-ifc explain <job.axjob> <policy.axflow>
    Print, in plain English, the EGRESS bound: "Sinks: net api.model.com may receive PUBLIC only;
    fs_write ./out/ may receive up to CONFIDENTIAL. Covert bandwidth cap: 16 emissions. Any unlisted
    sink: PUBLIC (fail-closed)." Performs NO execution. Exit 0.

axon-ifc check <value-label> <sink: axis:target> [--policy policy.axflow]
    A one-shot sink-guard query: "✓ ALLOW: internal may flow to fs_write:./out/ (clearance
    confidential)" or "✗ DENY: secret may NOT flow to net:api.model.com (clearance public)". Exit 0 / 8.
    The CLI surface of `sink_guard`, for operators to reason about a flow before running.

axon-ifc run <job.axjob> <policy.axflow> [--run-id ID] [--out DIR] [--declass-authority FILE]
    Compose over R21's supervisor with the R25 egress monitor: gate (R21) → run sandboxed → guard every
    egress (R25) → write the flow RunRecord to <DIR>/<run-id>.json. Prints the verdict in plain English
    ("✓ completed" / "⚠ DENIED: secret may not flow to net:… (axis: net)" / "⚠ DENIED: covert
    bandwidth cap exceeded"). Exit = verdict code (§3.6).

axon-ifc verify <record.json>
    Recompute the flow hash chain (= axon_os::record::verify); "✓ intact" / "✗ TAMPERED at event N".
    Exit 0 / 9. No execution.

axon-ifc declassify <value-label> --to <label> --by <holder> --clearance <label>
    The audited declassify op from the CLI: prints "✓ declassified secret→public by alice (audited)" +
    the audit event, or "✗ DENIED: alice (clearance internal) may not declassify secret". Exit 0 / 8.
```
Usage/`--help` on a bad invocation → exit 2 with a helpful message naming the expected form.

### 5.6 Shipped example artifacts (A2 — real, in `examples/flows/`, runnable immediately)
- `exfil.ax` + `exfil.axjob` + `exfil.axflow`: an agent **granted `net`** to `api.model.com` reads a
  `Confidential` input, processes it, and tries to POST the secret-derived result to the model endpoint
  (clearance `public`). R21 *admits* the job (the net capability is granted); **R25 DENIES the egress
  (exit 8)** — the headline negative demo: *the capability is granted, the leak is still refused.* The
  flow record shows the `egress_deny` event labeled `confidential`.
- `redact.ax` (+ `.axjob`/`.axflow`): the *same* job but it first **declassifies** (redacts) the value
  via an authorized holder — the audited `declassify` lowers the label to `public`, the now-public
  result is `Allow`ed to the model endpoint, and the record shows the `declassify` event then the
  `egress_allow`. (Positive demo: the bytes *may* leave, but only after an explicit, audited
  declassification — never implicitly.)

---

## §6 — Build order (each slice ends green before the next; TDD: test first, see it fail, make it pass)

- **S1 — Lattice + labeled values.** `label.rs`, `labeled.rs`. Tests: lattice order + `dominates` +
  `join` (= max); `combine_is_sticky_label_never_drops`; `derive` preserves label; unknown label
  fail-closes to Secret. Green.
- **S2 — Policy parse/validate.** `policy.rs`. Tests: parse the example `.axflow`; reject bad axis,
  bad clearance, `..` target, negative cap; missing sink → fail-closed `default` (Public).
- **S3 — The sink guard.** `guard.rs`. Tests: `secret_cannot_reach_sink_below_clearance_even_with_cap`
  (Deny regardless of an asserted-granted capability), allow-when-dominated, unknown-label-denies,
  `undeclassified_secret_to_public_sink_refused`.
- **S4 — Declassify (explicit/privileged/audited).** `declassify.rs`. Tests:
  `declassify_is_explicit_privileged_and_audited` (under-cleared holder refused; success emits the
  audit event; raise-attempt refused; combine/derive never lower a label).
- **S5 — Covert meter.** `meter.rs`. Tests: `covert_drip_is_bandwidth_bounded` (the (cap+1)-th secret-
  adjacent emission refused; record shows the count).
- **S6 — Flow record (reuse R21 chain).** `flowrec.rs`. Tests: `acc_a6_flow_record_tamper_detected`
  (mutate/drop/reorder a flow event → `verify` Err); equal inputs → equal digest.
- **S7 — Monitor over R21's Runtime (egress guard + ingress source-stamp).** `monitor.rs` + the
  `axon-os`-declared `EgressMonitor`/ingress-stamp seam + a `MockRuntime`. Tests: each egress routed
  guard→meter→record; a denied egress is **not performed** (assert via the mock's side-effect counter);
  deny-before-execute; **`mislabeled_secret_still_caught_at_source_stamp`** — the mock program reads a
  `./secret/`-prefixed input but (adversarially) labels it `Public` in-program and emits it to a
  `Public` sink; assert the supervisor's ingress stamp made it `Confidential` so the egress is **Denied
  exit 8** regardless of the program's lie.
- **S8 — `axon-ifc run`/`explain`/`check`/`verify`/`declassify` CLI + human output.** `cli.rs`,
  `main.rs`. Tests: `acc_a5_deterministic_byte_identical`, `acc_a4_hermetic_isolated_timeout` (over
  R21's runtime), `--help` on every subcommand, usage error → exit 2.
- **S9 — Example artifacts + smoke + quickstart.** `examples/flows/*`, `README-axon-ifc.md`. Tests:
  `acc_a1_smoke_label_propagate_egress_denied`, `acc_a2_example_exfil_denied_and_public_allowed`,
  `acc_a3_quickstart_commands_execute`.
- **S10 — Acceptance gate.** `scripts/r25_acceptance_gate.sh` (§10). Green = done.

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
  `Confidential` by the **supervisor's** stamp), but the program adversarially constructs its in-program
  `Labeled<T>` as `Public` (or omits the label); on egress to a `Public` sink the guard reads the
  **supervisor-stamped** `Confidential` label and → `Deny{axis}` exit 8. Asserts the program's claimed
  label is ignored at a mediated ingress (§1.6, §4.1). Also asserts the honest negative residual: a
  value the program *synthesizes internally* is **not** caught by source-stamping (documented L2 gap).

**Integration (real `axon` subprocess via R21's runtime, mock-free egress decisions):**
- `acc_a4_hermetic_isolated_timeout` — a runaway program run through `axon-ifc run` is killed at the
  R21 timeout; process gone; no egress leaked; verdict Denied(timeout).
- `acc_a5_deterministic_byte_identical` — run the example twice with the same `--run-id`/seed/policy;
  the two flow `RunRecord` JSON bytes are identical.
- `acc_a6_flow_record_tamper_detected` — build a flow record; for a mid-chain flow event mutate a
  field, drop it, reorder, insert → `verify` → `VerifyMismatch` (reusing R21's `record::verify`).
- `acc_a2_example_exfil_denied_and_public_allowed` — `exfil.axjob`+`exfil.axflow` → exit 8 + the record
  shows the denied `egress_deny` event labeled `confidential` + **no network actually occurred**;
  `redact.*` → exit 0 + a `declassify` event then `egress_allow`, output produced.

**User-journey smoke (A1 — drives the REAL CLI exactly as the operator would, via subprocess):**
- `acc_a1_smoke_label_propagate_egress_denied`: (1) `axon-ifc explain exfil.axjob exfil.axflow` →
  asserts the legible egress bound ("net api.model.com: PUBLIC only"); (2) `axon-ifc check secret
  net:api.model.com --policy exfil.axflow` → asserts "✗ DENY" exit 8; (3) `axon-ifc run exfil.axjob
  exfil.axflow --run-id demo --out <tmp>` → asserts "⚠ DENIED: … may not flow to net:… (axis: net)"
  exit 8 + the `egress_deny` event in `<tmp>/demo.json`; (4) `axon-ifc verify <tmp>/demo.json` →
  "✓ intact"; (5) `axon-ifc run redact.axjob redact.axflow …` → "✓ completed" + a `declassify` then
  `egress_allow` event. Each step asserts **stdout text AND the on-disk artifact**, not just exit codes.

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
  unparsable policy ⇒ Malformed (exit 2). Every ambiguity denies; none silently allows.
- **I-5 Tamper-evidence (inherited).** The flow record reuses R21's hash chain; any mutation/reorder/
  drop breaks `record_digest` and `verify` detects it. The recorder is trusted; the record is
  integrity-checkable (no signature — documented, A6).
- **I-6 Bounded covert bandwidth.** Secret-adjacent emissions to a non-Secret sink are counted and
  capped; past `covert_cap` they are refused; every one is recorded. (Bandwidth is *bounded + monitored*,
  not *proven zero* — the honest limit of L1.)
- **I-7 Determinism.** Same (labeled inputs, policy, seed) ⇒ byte-identical flow record (no ambient
  clock/random in core).
- **I-8 Orthogonal axes preserved.** Confidentiality (R25) and trust/integrity (`source.ax`) are kept
  separate — R25 never conflates "low trust" with "high confidentiality"; a value can be low-trust *and*
  public, or high-trust *and* secret.

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
- A program performing raw I/O **outside** the supervisor's mediated sinks → out of L1's reach **but**
  the program is R21-sandboxed to only the granted (all mediated) sinks; the unbypassable version is L2
  (§4.9) — stated, not hidden.
- Corrupt/edited stored flow record on `verify` → exit 9, never a silent pass (inherited from R21).

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

# 3. Run a job that's GRANTED net but tries to LEAK a confidential value — watch R25 refuse the
#    egress even though the capability is granted (exit 8, audited):
axon-ifc run examples/flows/exfil.axjob examples/flows/exfil.axflow --run-id demo --out ./runs
echo "exit=$?"

# 4. Confirm the flow record (the egress ledger) hasn't been tampered with:
axon-ifc verify ./runs/demo.json

# 5. Run the version that DECLASSIFIES (redacts) first — now the public result may leave, but only
#    after an explicit, audited declassification:
axon-ifc run examples/flows/redact.axjob examples/flows/redact.axflow --out ./runs
```

---

## §10 — Acceptance gate (pinned; FAILS if any check is missing or stubbed)

`scripts/r25_acceptance_gate.sh` is the single source of "done." It MUST:
1. **Presence check** — `grep` the test sources and assert every named check from §0 exists:
   `acc_a1_smoke_label_propagate_egress_denied`, `acc_a2_example_exfil_denied_and_public_allowed`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_hermetic_isolated_timeout`,
   `acc_a5_deterministic_byte_identical`, `acc_a6_flow_record_tamper_detected`,
   `secret_cannot_reach_sink_below_clearance_even_with_cap`, `combine_is_sticky_label_never_drops`,
   `undeclassified_secret_to_public_sink_refused`, `declassify_is_explicit_privileged_and_audited`,
   `covert_drip_is_bandwidth_bounded`, `mislabeled_secret_still_caught_at_source_stamp`. Any missing
   name → **gate fails**.
2. **Anti-stub check** — assert each acceptance test body contains a real assertion and is not
   `#[ignore]`d / `todo!()` / `assert!(true)` (grep for those anti-patterns → fail).
3. **Dependency-direction check** — assert (via `cargo tree -p axon-os`) that `axon-os` does **NOT**
   depend on `axon-ifc` (the cycle would invert the egress layering); if it does → **fail**.
4. **Run** `cargo test -p axon-ifc` (all green) **and** execute the §9 quickstart block against the
   built binary (A3) **and** run `acc_a1` driving the real CLI.
5. **Reproducibility** — run `acc_a5` twice and diff the two flow records byte-for-byte.
6. Exit 0 only if all of the above pass; print which check failed otherwise.
Wire `r25_acceptance_gate.sh` into the repo's `gate.sh --strict`.

---

## §11 — Definition of Done
**Per slice (S1–S10):** the slice's named tests were written first, were seen to fail, now pass; the
full `axon-ifc` suite is green; no regression in the workspace.
**Per milestone (R25 complete):** `cargo build -p axon-ifc` produces the `axon-ifc` binary; the real
example flows run end-to-end; **`acc_a1` passes through the real CLI**; a `Secret` value is refused at a
sink below its clearance **even with the capability granted** (`secret_cannot_reach_sink_below_
clearance_even_with_cap`); taint is sticky (`combine_is_sticky_label_never_drops`); a mislabeled/
omitted-label secret read through a mediated ingress is caught by the supervisor's source-stamp
(`mislabeled_secret_still_caught_at_source_stamp`); declassification is
explicit/privileged/audited; covert bandwidth is bounded; the flow record is deterministic
(`acc_a5`) and tamper-evident (`acc_a6`); `axon-os` does **not** depend on `axon-ifc`; and
`scripts/r25_acceptance_gate.sh` exits 0 with every §0 check green. Only then is R25 done.

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
- **Declassify is the ONLY lowering path** (I-3). There must be no other function — not `combine`, not
  `derive`, not the guard — that can lower a label. If you find yourself lowering a label anywhere else,
  that is a soundness hole.
```
