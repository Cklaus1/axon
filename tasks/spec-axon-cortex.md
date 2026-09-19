# Spec — Axon Cortex M0/M1 vertical slice

**Source package:** `docs/axon_cortex_v0_7/axon-cortex-build-v0_7/` (v0.7, 20 CX
specs, 69 work packages, 122 gates). This file is a SYNTHESIS: every task below
cites the source section it derives from, and the source is authoritative. Per
the build-loop rule for synthesized specs, open the cited section before
writing code — a one-line summary is lossy exactly where it matters.

**Basis warning, load-bearing:** the package was written against
`axon-docs.pdf` dated 2026-09-17 and predates ~115 commits on this branch.
`EXISTING_AXON_MAP.md` says its own repository paths are "documented, not
rerun". Every claim about existing Axon must be verified against the live repo
before anything is built on it.

---

## Step 1 — Adversarial review findings

### R1. 44 of 69 work packages are not autonomously buildable [REVISED: in-scope → out-of-scope]

M2–M9 plus the Research/Compiler/OS-K lanes require model training, external
corpora, live-model budget, backend bakeoffs and held-out benchmark suites.
An autonomous loop can produce their *scaffolding* but not their *result*, and
scaffolding that reports a stage "built" while no model was trained is the
worst outcome available — it is the "planned gate is not a passed gate" failure
the package itself warns about. Excluded from this run; see Step 2 D1.

### R2. B00 is owner-reviewed by its own text [REVISED: task → needs-human]

B00's output is "Owner-reviewed naming, scope and CX-to-repository ID mapping".
Naming the product and allocating IDs against Axon's `R*`/diagnostic/exit-code
namespaces is a product decision with external consequences. Tagged
`needs-human`; its dependents that need only the *local* `CX-*` names proceed,
since those are package-local by the package's own statement.

### R3. B15/B16 need credentials and spend [REVISED: task → needs-human]

"Add an authorized existing-model backend" and "Compare the real-model vertical
slice to controls" require an API key and incur per-call cost. Neither is an
engineering choice. The deterministic path (B11 mock adapter, B14 conformance)
delivers the same plumbing proof without them, which is the package's own
sequencing: "A scripted golden test proves the plumbing; varied model-driven
tasks establish solver behavior. Keep these results separate."

### R4. The 122 gates are NOT_RUN and several presuppose absent subsystems

`gate_manifest.json` reports `implementation_status: "Not implemented in this
package"` and `product_result: "NOT_RUN"` for all 122. Only the 45 gates on
CX-00/01/02/03/04/10/13/19 are in M0/M1 scope. A gate is implemented here only
when it is a real test with a real failure mode; an unimplementable gate is
logged, never marked passed.

### R5. Reuse is under-specified against a repo that moved

The package proposes modules (observer, grants, executor, AIR scheduler,
evidence/replay) that substantially exist: `AxonHost` + journals, `capabilities.rs`,
principal/sandbox registry, `axon-os` supervisor/ledger, `replay.rs`. Building
parallel versions would duplicate the TCB. Every task must first check the
existing seam and extend it, per the package's own "Do not create sixteen
crates because there are sixteen specs".

### R6. "Denial-first catalog" now has a house convention it must not contradict

CX-03's grant/catalog work lands next to the capability policy that shipped on
2026-09-19: profiles carry posture, and primitives are unambiguous (`""` denies,
`"*"` is unrestricted). A Cortex grant catalog must reuse
`axon_os::profile::Profile` and `axon_os::grant::Grant` rather than introduce a
third authority vocabulary.

---

## Step 2 — Decisions

### D1. Scope of this run — ENGINEERING, adopted as canon

Build **M0 + M1 minus the needs-human items**: the package's own "first
executable vertical slice". Rationale: it is the one tranche that produces a
running artifact rather than scaffolding, `BOOTSTRAP_PROMPT.md` recommends
repository intake over a sprint, and every later milestone depends on it.
M2–M9 remain unbuilt and are reported, not silently dropped.

### D2. Where Cortex lives — ENGINEERING, adopted

One new crate, `crates/axon-cortex`, holding contracts/observer/grants/executor/
evidence. Not sixteen crates. Authority-sensitive code in Rust; task logic in
Axon where the slice needs a program. Justification: the package proposes
exactly this, and a single crate keeps the TCB surface reviewable.

### D3. Namespace — ENGINEERING, adopted

`CX-*` IDs stay package-local. No new `R*` requirement IDs, no new `E****`
diagnostic codes, no new exit codes, no new `AXON_*` env var unless a task
genuinely needs one (and then with its `env_registry.rs` row, which is gated
both ways). Gate tests are named `cxg_<gate-id>` so they are greppable without
squatting anything.

### D4. Reuse over reimplementation — ENGINEERING, adopted

| Cortex concept | Existing seam to extend |
|---|---|
| world observation | `AxonHost` journal + `axon replay` |
| capabilities/grants | `axon_os::grant::Grant`, `axon_os::profile::Profile`, `capabilities.rs` |
| isolated execution | `sandbox_create_scoped` + `axon-os` supervisor |
| evidence/replay | host journals, `axon-audit` ledger, `record.rs` |
| independent verification | `axon check`/`axon test` as an out-of-band checker |

### D5. What "done" means for this run — ENGINEERING, adopted

The deterministic end-to-end conformance run (B14) is the smoke test: a
resettable copy of a broken Axon fixture is observed, a patch is proposed under
an explicit grant, applied in the copy, checked, independently verified and
replayed — with a concrete success signal. Live-model comparison is out.

### needs-human (excluded from the autonomous run)

- **B00** — product naming and CX→repository ID mapping (R2).
- **B15, B16** — authorized live-model backend: credentials + spend (R3).
- **M2–M9, Research, Compiler, OS-K lanes** — 44 packages requiring model
  training, corpora, bakeoffs and benchmark budget (R1).

---

## Tasks (in-scope)

Each cites its source. Status is maintained in `tasks/todo-cortex.md`; commit
history is the source of truth.

| ID | Source | Task | Depends on |
|---|---|---|---|
| C1 | CX-00 §contract, EXISTING_AXON_MAP | Verify the package's "existing Axon" claims against the live repo; record a reuse/verify/extend matrix with actual evidence | — |
| C2 | CX-10 schemas, `schemas/PROTOCOLS.md` | Typed event/manifest contracts: snapshot, observation, action, evidence | C1 |
| C3 | B03, BUILD_PLAN §vertical slice | Resettable broken-Axon fixture + pinned visible checks and hidden completion tests | C2 |
| C4 | CX-02, B05 | Partial software observation over a snapshot (never whole-world) | C2 |
| C5 | CX-03, B06 | Denial-first action catalog over `Grant`/`Profile` | C2 |
| C6 | CX-03, B08 | Patch transaction applied to a COPY, with snapshot revalidation | C3, C5 |
| C7 | CX-04, B09 | Registered check execution with durable action states | C3, C5 |
| C8 | CX-05 interface, B11 | Deterministic mock decision adapter (no model) | C4, C5 |
| C9 | CX-01, B12 | Independent final verification, out-of-band from the actor | C6, C7 |
| C10 | CX-10, B13 | Evidence events + exact replay of the episode | C6, C7 |
| C11 | B14, BUILD_PLAN §vertical slice | Deterministic end-to-end conformance run — THE SMOKE TEST | C8, C9, C10 |
| C12 | gate_manifest CX-00/02/03 | Implement the negative gates that are real tests: denied authority edit, stale snapshot, out-of-catalog action | C11 |

**Critical path:** C2 (contracts). Everything downstream serialises through the
event/manifest types, and changing them later rewrites every consumer. Extra
gate beyond the normal regression test: a round-trip proof (serialize →
deserialize → re-serialize is byte-identical) plus a rejection proof for each
malformed shape.
