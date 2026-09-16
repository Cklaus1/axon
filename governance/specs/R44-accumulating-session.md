# R44 — The accumulating typed session

**Spec ID:** `R44-accumulating-session`
**Status:** Draft — §12 Q1 (the persistence fork) must be confirmed against a Slice-0 spike before Slice 1 opens
**Risk class:** Structural. Introduces a process that outlives a single program execution — a new lifetime
in a compiler that has only ever been one-shot. Not a language feature: no `.ax` syntax changes.
**Author / date:** 2026-09-16, from `AXON_FOR_RLM.md` §5, whose gate (§"honest sequencing" step 2) is passed.

```spec-meta
id: R44-accumulating-session
status-claim: Draft
depends-on: R7b-axonhost, R6-capability-security
blocks: none
blocked-by: R44 §12 Q1 (persistence fork must clear a Slice-0 spike before Slice 1 opens)
supersedes: none
related: R15-resume-runtime, R41-polyglot-runtime, R38-embedded-agent-runtime, R28-capability-audit-ledger, R42-stdlib-gaps
reserves: E2400-E2404, confirmed free at spec time (grepped `E2[0-9]{3}` across crates/ and every
  governance/specs `reserves:` line — taken bands are E20xx [R41], E21xx [R16], E22xx [R42/R43],
  E23xx [eBPF], E37xx [R37]; E24xx is the next contiguous free band)
```

---

## 1. Why this spec exists

`axon run` compiles a file and executes it. There is no namespace between invocations, so Axon fails
the defining RLM property — **bind a name in one call, read it in the next**. `AXON_FOR_RLM.md` §5
states the consequence precisely:

> As it stands it is a `run_code` tool, which is `RLM_MODE_SPEC.md` §10's *stateless alternative*.

Four of that document's five recommendations have landed. §1/§2/§3 are verifiable today —

```
$ axon run mut.ax
I0002  `mut` is not an Axon keyword and was ignored — bindings are already reassignable
       help: drop it: `let x = …`, then assign with `x = …`
```

— and §5 is the one that changes what Axon *is* rather than how well it does what it already does.

### 1.1 Why this is not "add a REPL"

A REPL is the shape an *interpreted, dynamically typed* language offers. Copying it would waste the
only structural advantage Axon has here. The differentiated property is:

> **Every prior binding in the session is re-type-checked before the new cell runs.**
> Using a binding at the wrong type is a compile error before anything executes.

Python's kernel cannot do that, and the measurement already recorded the cost. On the Python side of
the `tasks_hard` run the model reused `rows`, guessed its shape wrong, and scored **3/5 against
stateless's 5/5** — *because a name carries no type*. Statefulness made Python worse. A session that
carries types is the only shape in which state is a net win, and it is the entire argument for
building this rather than a REPL.

**This spec is therefore about the checker's reach, not about a prompt.** The prompt is incidental;
"the accumulated module is re-checked in full on every cell" is the product.

### 1.2 What is already in place

Sized against the code, not the summary:

* **Top-level `let` works.** `let top = 41` followed by `fn main() { println(to_str(top + 1)) }`
  prints `42`. So a session accumulates ordinary module items — there is no new binding form to design.
* **Globals initialise separately from `main`.** `Interp::init_globals()` exists and is already called
  independently of the entry point by `run_named_fn_as_bool` (`interp.rs`), which builds an `Interp`,
  initialises globals, and calls an arbitrary named function. A cell executor is that call shape.
* **The whole-module check is one function.** `run_check_pipeline_located(&mut program, &src, &file)`
  already type-checks an entire `Program` and returns located diagnostics. Re-checking the accumulated
  module is calling it on a bigger `Program`, not writing a new analysis.

**Two visibility caveats, checked rather than assumed.** `init_globals` is a private method
(`interp.rs:2600`, no `pub`) and `run_check_pipeline_located` is a private free function in
**`main.rs`**, not in the library. So the session driver either lives in `main.rs` alongside the other
`cmd_*` functions — the path of least resistance, and correct for v1 — or both are lifted into
`axon-core` first. Lifting is the better shape if R38 ("Axon Embedded") is ever picked up, since an
embeddable session needs them library-side; it is **not** required by this spec and should not be
bundled into it. Named here so the choice is deliberate rather than discovered in Slice 1.
* **A long-lived, line-oriented host loop already ships.** `run_suspendable_stdio` (R15) keeps an
  interpreter alive across host round-trips. The session driver is the same lifetime, differently framed.

So the missing piece is narrow: **a process that holds one `Interp` across cells, and a protocol for
feeding it cells.** That is the whole of the work, and it is why the persistence fork (§2) is the only
decisive question.

---

## 2. The decisive fork — how does a binding made in cell N survive into cell N+1?

Three designs, and the choice is expensive to reverse because it fixes the process model.

### (a) Re-execute the whole accumulated module each cell

Concatenate cells; run the result. No persistence mechanism at all, and it needs no new lifetime.

**Rejected: it re-runs side effects.** A cell that writes a file writes it again on every subsequent
cell; a cell that POSTs, POSTs again. For a host whose entire purpose is running model-authored code
that touches the world, silently repeating every prior effect on every turn is not a performance
characteristic, it is a correctness failure. Cost is also O(n²) in cells.

Worth stating because it is the design anyone reaches for first, and it is cheap enough that its
failure mode has to be named rather than assumed obvious.

### (b) Persist a live `Interp` across cells in one long-lived process — **CHOSEN**

The session is a process. It holds one `Interp`; each cell appends items to the accumulated `Program`,
re-checks the whole module, and executes only the new tail against the live interpreter state.

**Why it wins: no value ever crosses a boundary, so no value ever needs to be serialisable.** This is
the load-bearing property. Four `Value` variants are not plain data —

| Variant | Why it cannot be serialised |
|---|---|
| `Closure` | captures `Rc<RefCell<Env>>`; the capture is persistent across calls by design (AUDIT T40) |
| `Chan` | `Rc<RefCell<VecDeque<Value>>>` — identity-shared mutable state |
| `Dict` | `Rc<RefCell<BTreeMap<..>>>` — identity-shared mutable state |
| `Handle` | a slab index into live interpreter-side state (R13); unforgeable *because* it is bound to one `Interp` |

— and under (b) none of that matters, because the `Rc`s stay in the same heap the whole session.

Semantics are preserved by construction rather than by a re-implementation that must be kept in sync.
That is the same reasoning R12 used to reject a preemptive scheduler and R10 used to make the
interpreter the equivalence oracle: **a second mechanism that must agree with the first is an I-2
divergence risk.**

### (c) Serialise top-level values to a session file between one-shot CLI invocations

Keeps the existing one-shot CLI shape — each cell is still `axon <something> file.ax`.

**Rejected on the same table.** It carries `Int`/`Str`/`Array`/`Struct` fine and cannot carry the
other four at all. `Dict` alone disqualifies it — it is ordinary in model-written code. The precedent
is direct and recent: **R15 Slice 2** crossed full `Value` payloads over a suspend via a deep-clone
`SendValue` and **refused a `Chan` payload with a clear error rather than corrupting it**. Applying
that precedent here would mean a session in which a `Dict` binding cannot survive a cell — a hole in
the headline feature, in the values most likely to hold the session's accumulated work.

**(c) is retained as a fallback only if §12 Q1's spike shows (b) cannot hold the process model**, in
which case the refusal set is explicit and documented, not discovered.

---

## 3. Surface

No `.ax` syntax changes. One new verb.

```bash
axon session                      # start an interactive session on stdin/stdout
axon session --protocol jsonl     # line-oriented protocol for a host driver (the RLM case)
axon session --require-contained  # composes with R44's sibling (AXON_FOR_RLM §4)
axon session --record run.journal # one journal for the WHOLE session (§6)
```

A **cell** is a fragment of Axon source: zero or more module items (`let` / `fn` / `type` / `mod`),
optionally followed by a trailing expression. The trailing expression is the cell's *value* and is
what gets printed — the one affordance borrowed from REPLs, because a session whose every cell must
declare a function to see a number is not usable.

```
> let rows = [3, 1, 2]
> fn total(xs: &[i64]) -> i64 { arr_sum_by(xs, |v| v) }
> total(&rows)
6
> let rows = "now a string"
E2400  redefining `rows` as `str` breaks 1 earlier binding in this session
       help: `total(&rows)` at cell 3 requires `[i64]`; rename, or redefine `total` in the same cell
```

That last diagnostic is the product. It is the thing Python's kernel structurally cannot say.

---

## 4. Semantics

| # | Behaviour | Rule |
|---|---|---|
| S1 | **Accumulation** | Items in cell N join the accumulated `Program`. Order is cell order. |
| S2 | **Whole-module check** | Every cell re-runs `run_check_pipeline_located` over the **entire** accumulated program, not the new tail. This is the feature; it is not an optimisation target (§10). |
| S3 | **Tail-only execution** | Only items introduced by cell N execute. Prior cells are never re-executed, so no side effect repeats. |
| S4 | **Check-before-execute** | A cell that fails the check does not execute, and **does not accumulate**. The session state after a failed cell is byte-identical to before it. |
| S5 | **Redefinition replaces, and must not break the past** | Redefining a name replaces it. If any *earlier* accumulated item no longer type-checks against the new definition, that is **E2400** and S4 applies. Shadowing is not offered: two live meanings for one name in an audit-facing session is the ambiguity the type system exists to remove. |
| S6 | **Trailing expression** | A cell's trailing expression is evaluated after its items and rendered with the existing display path. A cell with no trailing expression prints nothing. |
| S7 | **Failure isolation** | A runtime panic in cell N leaves cells 1..N-1 intact and the session live. The failing cell does not accumulate (S4). Bindings it created before panicking are discarded (§12 Q2). |
| S8 | **Effects are per-cell** | `@[contained]`, effect rows, `AXON_ALLOWED_EFFECTS` and `--require-contained` apply to each cell as they would to a program. A session cannot be used to launder an effect past a ceiling by splitting it across cells. |

### 4.1 S5 is the whole spec, stated as a test

```
cell 1:  fn f() -> i64 { 1 }
cell 2:  fn g() -> i64 { f() + 1 }
cell 3:  fn f() -> str { "one" }      ← E2400: cell 2's `g` no longer type-checks
```

A dynamic kernel accepts cell 3 and fails at cell 4 when `g()` is next called — or worse, returns a
wrong answer. Axon refuses cell 3, names `g`, and leaves the session in its cell-2 state. **If the
implementation cannot produce this, the spec is not done, regardless of what else works.**

---

## 5. Audit, replay and containment

The item most likely to be missed, so it is a requirement rather than a note.

| # | Rule |
|---|---|
| A1 | A session is **one run-id**, stamped once at session start — not one per cell. Every provenance record carries a `cell` index. |
| A2 | `--record` writes **one journal for the whole session**. `AXON_RECORD` and a session are not mutually exclusive; cell boundaries are journal events. |
| A3 | `axon replay` of a session journal reproduces **the whole session**, cell by cell, and diverges (exit 11) on the first cell whose host interactions depart — the existing divergence machinery, unchanged. |
| A4 | The R28 capability ledger flushes **once at session end** and its integrity check covers every cell. A session must not be a way to make capability use less legible than a single run. |
| A5 | `axon trace --ai` attributes AI calls to `(fn, src, principal)` as today, with `src` naming the session and the cell. Per `a9261f1`, a summary must not merge records across the thing it attributes — a session adds a dimension and must not lose one. |

A2/A3 are what keep this from regressing the auditability story. A session that cannot be replayed
would trade Axon's strongest differentiator for its missing one.

---

## 6. Error codes

Reserved band **E2400–E2404**, confirmed free at spec time. Exact allocation settles at implementation;
re-grep before use.

| Code | Meaning |
|---|---|
| `E2400` | redefinition breaks an earlier accumulated item (§4.1) — the headline diagnostic |
| `E2401` | cell references a name never bound in this session (distinct from a plain unresolved name: the help should say whether an *earlier cell failed to accumulate*, which is the confusing case) |
| `E2402` | cell is not a valid fragment (parse tier, session-specific framing) |
| `E2403` | session protocol error (malformed cell frame under `--protocol jsonl`) |
| `E2404` | reserved for the (c)-fallback refusal set, unused if §2 (b) holds |

No new exit codes. A failing cell is exit 2 in one-shot mode, matching `check`/`run`; in interactive
mode the session stays live and the code is carried in the protocol frame.

---

## 7. Invariants touched

* **I-2 (one execution semantics).** Upheld and load-bearing: (b) reuses the interpreter rather than
  re-implementing state transfer. **Native codegen does not participate** — a session is interpreter-only
  and `axon build` is unaffected. No parity harness is needed because there is no second engine to diverge
  from; that is a consequence of the §2 choice, and is the reason it should not be revisited casually.
* **I-11 (capability boundary).** Upheld via S8. Explicitly tested, not assumed.
* **I-13 (provenance is not opt-out-able).** Upheld via A1–A5.

---

## 8. Slices

| Slice | Content | Gate |
|---|---|---|
| **0** | **Spike, kill-gated.** Hold one `Interp` across two cells in one process; bind a `Dict` in cell 1, mutate it in cell 2. Answers §12 Q1 and nothing else. | The `Dict` mutation is visible in cell 2. If it is not, (b) is wrong and §2 reopens **before** Slice 1. |
| **1** | Accumulate + whole-module re-check + tail execution (S1–S4). No redefinition handling yet — a redefinition is refused outright. | S1–S4 tests; §4.1 cells 1–2 pass, cell 3 refused (by any diagnostic). |
| **2** | **S5 + E2400** — redefinition replaces, and breaking an earlier item is named with the *earlier* item's identity. | §4.1 exactly, including that the session is byte-identical to its cell-2 state afterward. |
| **3** | Trailing-expression values (S6), failure isolation (S7). | A panic in cell N leaves N-1 usable. |
| **4** | Audit/replay/containment (§5 A1–A5, S8). | A recorded session replays whole; a split effect is refused at the ceiling. |
| **5** | `--protocol jsonl` host driver. | An external host drives 20 cells and reads per-cell results. |

Slices 1–2 are the product. 3–5 make it usable and keep the audit story intact.

---

## 9. Stop condition

```
DONE = §4.1 passes EXACTLY — cell 3 refused, `g` named, session state unchanged
   AND no prior cell's side effect re-runs when a later cell executes (S3), proved by a cell
       that appends to a file and a later cell that does not
   AND a Dict/Closure/Chan binding survives a cell boundary with IDENTITY preserved, not a copy
   AND a recorded session replays whole (A3), and a tampered cell diverges at that cell with exit 11
   AND a split effect is refused at the ceiling (S8), proving a session is not an effect-laundering seam
   AND the full suite is green IN EVERY CONFIGURATION THE GATE RUNS, each reported with its
       configuration — per R42 §12.1, "the suite passes" is not a claim until it names which
       suite, built how
   AND measured per-task, not as a total: the session arm is reported against the stateless arm
       task by task, with the stateless control's own score printed alongside
```

The last clause is not optional. The arm sweeps were **stopped on 2026-09-08** because the thesis came
out unsupported and the stateless control *won* the error column — a metric a control can win is
measuring structure, not reuse. **Do not justify this spec with a projected score, and do not report
one from the existing harness without fixing it first.** Build it because the capability is absent and
its gate is passed; measure it separately, afterwards, with a control that is inert on the metric.

---

## 10. Performance budget

Re-checking the whole module every cell is O(cells × items). At a session length a model actually
produces (tens of cells, hundreds of items) this is milliseconds, and `run_check_pipeline_located`
already runs on every `axon run`.

**Deliberately not optimised, and deliberately not made incremental.** Incremental re-check is a second
analysis that must agree with the first — the I-2 failure mode. If profiling ever demands it, the
honest move is to make the whole check faster, not to add a cache that can disagree with it. Revisit
only with a measurement, and record the measurement here.

---

## 11. Rollout & rollback

Additive. A new verb; `run`/`check`/`build` are untouched, so rollback is removing the verb. No `.ax`
source written against a session is invalid outside one — a session transcript concatenates into an
ordinary module, which is a property worth keeping as a test (**a session's accumulated program must
be a file `axon check` accepts**).

---

## 12. Open questions

| # | Question | Default if undecided |
|---|---|---|
| **Q1** | **Can one `Interp` be held across cells without stale-borrow hazards?** `interp.rs` uses `RefCell` pervasively, and R15's §3 risk note flags exactly this for cross-call-boundary state. Slice 0 exists to answer it. | (b). Fall back to (c) with a documented refusal set only if the spike fails. |
| Q2 | A cell that panics **midway** — are bindings it created before the panic discarded, or kept? S7 says discarded (all-or-nothing per cell). Is that right when the panic is in a trailing expression *after* several successful `let`s? | Discarded. All-or-nothing is the rule a user can state; "some of cell 4 happened" is not. |
| Q3 | Does `--require-contained` (AXON_FOR_RLM §4, unbuilt) belong in this spec or its own? | Its own — it is meaningful for one-shot `check` too. Ship independently; S8 only requires they compose. |
| Q4 | Should a session persist to disk so it can be resumed after the process exits? | No for v1. That reopens the whole serialisation problem §2(c) was rejected over. Revisit only with a concrete ask. |
| Q5 | Multi-file / `mod` imports inside a session — does `AXON_PATH` resolution happen per cell or once? | Per cell, matching `run`. Flagged because a cached resolution would be a way for cell N to see a module cell 1 could not. |

---

## 13. Dependency DAG

```
R44 Slice 0 (spike, kill-gate Q1)
  └── Slice 1 (accumulate + whole-module check + tail exec)
        └── Slice 2 (S5 + E2400)          ← the product
              ├── Slice 3 (trailing values, failure isolation)
              ├── Slice 4 (audit/replay/containment)   [needs R28 ledger, landed]
              └── Slice 5 (jsonl protocol)              [needs Slice 3]

AXON_FOR_RLM §4 (--require-contained)  — independent, composes at S8, ship in parallel
```

No spec blocks R44. R44 blocks nothing. It is a leaf, which is the main argument for doing it now
rather than after any of the six Draft platform specs.

**R15 is deliberately NOT a dependency edge**, though it is cited three times. A cell runs to
completion and returns; nothing suspends mid-cell, so the resume runtime is not on this path. R15 is
*precedent* (its `SendValue` refusal is the model for §2's rejection of serialisation) and a *shape*
(`run_suspendable_stdio` proves the interpreter survives a long-lived loop). Recorded because citing
a spec heavily and then not depending on it looks like an omission, and a false edge pollutes the DAG
for everyone downstream.

---

## 14. Evidence ledger

Empty — Draft. No implementation code before §12 Q1 clears Slice 0 (`BUILD_PROTOCOL.md` Gate 1).

| Claim | Evidence | Commit |
|---|---|---|
| Top-level `let` works today | `let top = 41` + `fn main()` reading it prints `42` | verified 2026-09-16, pre-spec |
| `init_globals` is separable from `main` | `interp::run_named_fn_as_bool` builds an `Interp`, calls `init_globals`, then an arbitrary fn | `crates/axon-core/src/interp.rs` |
| Four `Value` variants are not plain data | `Closure` / `Chan` / `Dict` / `Handle` all hold `Rc<RefCell<..>>` or a live slab index | `crates/axon-core/src/interp.rs:36+` |
| Refusing a non-transferable value over a boundary is the house precedent | R15 Slice 2 `SendValue` refuses a `Chan` payload rather than corrupting it | `b097c0e` |
| AXON_FOR_RLM §1/§2/§3 have landed | `axon run mut.ax` emits a located `I0002` with `help` | verified 2026-09-16, pre-spec |
| AXON_FOR_RLM §4 has NOT landed | an uncontained `read_file("/etc/passwd")` passes `axon check` silently, exit 0 | verified 2026-09-16, pre-spec |
