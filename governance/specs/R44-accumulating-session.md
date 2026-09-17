# R44 — The accumulating typed session

**Spec ID:** `R44-accumulating-session`
**Status:** Landed — all slices (0–5) complete (2026-09-16/17). §12 Q1 resolved: v1 = (c). §12 Q1 resolved: v1 = (c). Slice 2 is the product.
**Risk class:** Structural. Changes where a session's bindings live (module scope → `main`'s scope) and,
in its v2 stage, the lifetime discipline of `Interp`. Not a language feature: no `.ax` syntax changes.
**Author / date:** 2026-09-16, from `AXON_FOR_RLM.md` §5.
**Review:** adversarial review folded in 2026-09-16. It found the persistence model as first written
**cannot borrow-check** (§2.2), the redefinition rule **type-unsound** (§4.2), the whole-module re-check
**not idempotent** (§4.3), and — the one that mattered most — that this spec's decisive fork had already
been **prototyped, specced and tested in-repo** and the first draft did not know (§1.1). Four further
corrections in §5, §9, §11, §13. A fifth defect, fatal to the first draft's §3 and not caught by the
review, was found by running the example: **module-scope bindings cannot be assigned** (§1.2).

```spec-meta
id: R44-accumulating-session
status-claim: Landed
depends-on: R7b-axonhost, R6-capability-security
blocks: none
blocked-by: none
supersedes: tasks/spec-rlm-accumulator.md (DRAFT 2026-08-07 — absorbed; its N1 is this spec's §4 S1,
  its N2 has since landed, its prototype is this spec's v1 substrate)
related: R15-resume-runtime, R41-polyglot-runtime, R38-embedded-agent-runtime, R28-capability-audit-ledger, R42-stdlib-gaps, R43-bytes-and-binary
evidence: scripts/r44_acceptance_gate.sh (Slice 0 hazards + Slices 1-5, ALL PASS 2026-09-17; refuses to run at all against a binary without the verb, so its negative assertions cannot pass vacuously)
reserves: E2400-E2404, confirmed free at spec time (grepped `E2[0-9]{3}` across crates/ and every
  governance/specs `reserves:` line — taken bands are E20xx [R41], E21xx [R16], E22xx [R42/R43],
  E23xx [eBPF], E37xx [R37]; E24xx is the next contiguous free band)
```

---

## 1. Why this spec exists

`axon run` compiles a file and executes it. There is no namespace between invocations, so Axon fails
the defining RLM property — **bind a name in one call, read it in the next**. `AXON_FOR_RLM.md` §5:

> As it stands it is a `run_code` tool, which is `RLM_MODE_SPEC.md` §10's *stateless alternative*.

Four of that document's five recommendations have landed (§1/§2/§3 verifiably: `axon run` on a `let mut`
emits a located `I0002` carrying a repair hint). §5 is the one that changes what Axon *is*.

### 1.1 This is NOT greenfield — correcting the first draft's largest error

The first draft of this spec analysed the decisive fork (§2) from first principles and chose a design,
unaware that **the rejected option was already built, specced, and under test in this repo**:

| Prior art | Where | What it is |
|---|---|---|
| Working prototype | `interp.rs:2195-2275` | `AXON_DUMP_BINDINGS` / `AXON_DUMP_SHAPES`, commented *"PROTOTYPE (RLM session option 2)"* |
| Dedicated field | `interp.rs:643-645` | `Interp.main_locals` — snapshots `main`'s top-level locals for exactly this purpose |
| Prior spec | `tasks/spec-rlm-accumulator.md` | DRAFT 2026-08-07, from a previous fable review; N1 (session scoping) + N2 (`+` concatenation) |
| Tests | `cli_run.rs:20533+` | `dicts_round_trip_through_a_dump_and_aliases_are_refused` |
| Downstream reference | `R43-bytes-and-binary.md:249` | already reasons about "the session dump" as a shipped thing |

**The first draft's §2 rejected value-serialisation on the grounds that `Dict` "cannot be carried at
all", and the prototype carries `Dict`.** It refuses only *aliased* dicts — two bindings reaching the
same `Rc<RefCell<..>>`, which would reconstruct as two independent dicts and silently break write
visibility — with the R15 `Chan` precedent cited in its own comments. The disqualifier is **aliasing,
not `Dict`**. A spec that rules out an option on a property the in-repo implementation of that option
does not have is not analysis, it is invention.

This spec therefore **supersedes `tasks/spec-rlm-accumulator.md`** and is scoped as: *promote the
prototype to a specified, gated feature, and add the property it does not have — whole-module
re-checking.*

### 1.2 The wall that breaks the obvious design

Verified by running it, not by reading:

```
let rows = [1]
fn main() { rows = arr_concat(&rows, &[2]) }

E0001  cannot assign to function name `rows` — only mutable local bindings can be reassigned
```

Module-level `let` registers as `Symbol::Fn` and assignment to a non-`Symbol::Local` is refused. So
**accumulating a session's bindings as module-level `let` items — the first draft's S1 — makes the
single most common statement a model writes (`rows = rows + [record]`) permanently illegal.**
`tasks/spec-rlm-accumulator.md` found this and its N1 is the fix: a cell's bindings compose **inside
`main`**, where mutation is legal. That is adopted here as §4 S1.

Its sibling N2 (`+` on `str` and `[T]`) **has since landed** — `"a" + "b"` → `ab` and
`rows = rows + [2]` both run today. Only N1 remains open.

### 1.3 The product, stated once

Not a REPL — that is the shape an interpreted, dynamically typed language offers, and copying it wastes
the only structural advantage Axon has. The product is:

> **Every prior binding in the session is re-type-checked before the new cell runs.**

Python's kernel cannot do that, and the measurement recorded the cost: on the Python side of the
`tasks_hard` run the model reused `rows`, guessed its shape wrong, and scored **3/5 against stateless's
5/5** — *because a name carries no type*. Statefulness made Python **worse**. A session that carries
types is the only shape in which state is a net win.

**This spec is about the checker's reach.** Persistence (§2) is the substrate; the re-check is the point.

---

## 2. The decisive fork — REOPENED by review

How does a binding made in cell N survive into cell N+1? The first draft chose (b) and called the work
"narrow". Review established it is not narrow; it is currently **unimplementable**.

### 2.1 (a) Re-execute the whole accumulated module each cell — rejected

Concatenate cells; run the result. **Rejected: it re-runs every prior side effect.** A cell that writes
a file writes it again on every subsequent cell. For a host whose purpose is running model-authored code
that touches the world, that is a correctness failure, not a slow path. O(n²) in cells besides.

### 2.2 (b) Persist one live `Interp` across cells — BLOCKED, not chosen

**`Interp<'p>` borrows the `Program`.** `interp.rs:469-477`:

```rust
pub struct Interp<'p> {
    fns: HashMap<String, &'p FnDef>,
    structs: HashMap<String, &'p TypeDef>,
    enums: HashMap<String, &'p EnumDef>,
    methods: HashMap<(String, String), &'p FnDef>,
    global_defs: Vec<(String, &'p Expr)>,
    …
}
```

built by `Interp::build(program: &'p Program)` (`interp.rs:2490`). Two independent blockers follow:

1. **Appending to the accumulated `Program` while an `Interp` borrows it is rejected by borrowck.**
   `program.items` is a `Vec<Item>`; pushing may reallocate, and the `Interp` holds references into it.
2. **The whole-module re-check needs `&mut program`** — `run_check_pipeline_located(&mut program, …)`
   (`main.rs:4464`) — while the live `Interp` holds it immutably.

The first draft cited `run_named_fn_as_bool` as evidence for (b). It is evidence *against*: it
**rebuilds** an `Interp` per call and re-runs `init_globals`, which `mem::take`s `global_defs` and
evaluates **all** of them — re-running every prior top-level `let`'s side effects, which is precisely
what (a) was rejected over. There is no incremental "evaluate only cell N's new lets" path.

§12 Q1 previously framed the risk as `RefCell` stale borrows. **That was the wrong hazard.** The
blocker is the `'p` lifetime, visible in the signature the draft itself quoted.

### 2.3 (c) Materialise values between one-shot runs — **CHOSEN for v1**

Each cell runs as an ordinary program; on success its bindings are written back as source literals and
prepended to the next cell. **This is the shipped prototype** (`interp.rs:2195-2275`).

It survives the objection the first draft raised against it, and it has a property neither (a) nor (b)
has, which review surfaced indirectly and is decisive:

> **Materialisation pins a binding's type by construction.**

Under (b), the accumulated text still says `let x = make()` while the heap holds the old value — so
redefining `make` silently re-types `x` with no error (§4.2, the soundness hole). Under (c), `x` was
written back as the literal `1`; re-checking types it `i64` because it *is* `i64`. **Text and heap
cannot diverge, because there is only text.**

Its refusal set is real and must be *reported, not hidden*: closures, `Chan`, aliased dicts, `Handle`.
The prototype already ships the answer — `AXON_DUMP_SHAPES` describes **every** binding including the
ones it could not persist, on the reasoning that *a name the model can see but not reuse is the case it
most needs told about*. That becomes §4 S10.

### 2.4 (d) Refactor `Interp` off `&'p Program` — the fourth option, v2's gate

Rc/arena/append-only-`Box` items, so the interpreter no longer borrows the program it runs. This is the
option the first draft never considered, and it is what (b) actually costs. It buys what (c) cannot
carry — closures and aliased mutable state across cells — at the price of a lifetime refactor of the
interpreter's core struct.

### 2.5 Resolution

**Staged, and the stages are independently valuable:**

* **v1 = (c).** Ships on an existing prototype. Delivers §1.3's product — the re-check is orthogonal to
  how values persist. Pins types for free. Refusal set is documented and *announced* (S10).
* **v2 = (b), gated on (d).** Opened only if measurement shows the refusal set actually bites. Not
  specced here; §12 Q4.

The first draft's argument for (b) — "no value crosses a boundary, so semantics hold by construction" —
remains true and remains the reason v2 is the eventual shape. It is just not reachable without (d).

---

## 3. Surface

No `.ax` syntax changes. One new verb.

```bash
axon session                      # interactive session on stdin/stdout
axon session --protocol jsonl     # line-oriented protocol for a host driver (the RLM case)
axon session --require-contained  # composes with AXON_FOR_RLM §4 (separate spec)
axon session --record run.journal # one journal for the WHOLE session (§5)
```

A **cell** is a fragment: zero or more module items (`fn` / `type`), zero or more statements (including
`let` and assignment), optionally a trailing expression whose value is displayed.

```
> let rows = [3, 1, 2]
> fn total(xs: &[i64]) -> i64 { arr_sum_by(xs, |v| v) }
> total(&rows)
6
> rows = rows + [10]              // legal: cell statements live in main's scope (§1.2)
> total(&rows)
16
> fn total(xs: &[i64]) -> str { "oops" }
E2400  redefining `total` breaks 1 earlier item in this session
       help: `total(&rows)` at cell 3 requires `-> i64`; rename, or update the caller in the same cell
```

The `rows = rows + [10]` line is the one that does not work today and is the reason §4 S1 exists.

---

## 4. Semantics

| # | Behaviour | Rule |
|---|---|---|
| S1 | **Where bindings live** | A cell's `let`s and statements compose **inside `main`**, not at module scope — module-scope bindings cannot be assigned (§1.2, E0001). `fn` / `type` / `impl` / `trait` items accumulate at module scope as usual. |
| S2 | **Whole-module check** | Every cell re-checks the **entire** accumulated program. This is the feature, not an optimisation target (§10). |
| S3 | **Tail-only execution** | Prior cells are not re-executed. Prior *values* arrive materialised (§2.3), not by re-running the code that made them, so no side effect repeats. |
| S4 | **Check-before-execute** | A cell failing the check does not execute and does not accumulate. State after is byte-identical to before. |
| S5 | **Redefinition replaces, and must not break the past** | Redefining a name replaces it. If any earlier accumulated **item** stops type-checking, that is **E2400** and S4 applies. Shadowing is not offered. |
| S6 | **Trailing expression** | Evaluated after the cell's statements, rendered via the existing display path. None ⇒ no output. |
| S7 | **Failure isolation** | A runtime panic in cell N leaves cells 1..N-1 intact and the session live. **Scope is bounded — see §4.4.** |
| S8 | **Effects are per-cell** | `@[contained]`, effect rows, `AXON_ALLOWED_EFFECTS`, `--require-contained` apply per cell. A session must not launder an effect past a ceiling by splitting it across cells. |
| S9 | **The re-check must be idempotent** | See §4.3 — it is not, today. |
| S10 | **Non-persistable bindings are NAMED, never silently dropped** | A binding that cannot cross a cell (closure / `Chan` / aliased dict / `Handle`) is reported with its name, its shape, and the reason. The prototype's `AXON_DUMP_SHAPES` is this; it is promoted from a debug env var to a guaranteed part of the cell result. |
| S11 | **No warning storm** | Prelude bindings must not emit `W0006 unused variable` per cell. Inherited from the prior spec's known-costs list; a warning that grows with session length is its own defect. |

### 4.1 S5 is the whole spec, stated as a test

```
cell 1:  fn f() -> i64 { 1 }
cell 2:  fn g() -> i64 { f() + 1 }
cell 3:  fn f() -> str { "one" }      ← E2400: cell 2's `g` no longer type-checks
```

A dynamic kernel accepts cell 3 and fails at cell 4, or returns a wrong answer. Axon refuses cell 3,
names `g`, and leaves the session in its cell-2 state. **If the implementation cannot produce this, the
spec is not done.**

### 4.2 The soundness hole S5 must not reintroduce

Review found the first draft's S5 unsound. `infer.rs:1662-1671` types module-level `let`s by
**re-inferring their bodies** against the current program. Under a live-`Interp` design (b):

```
cell 1:  fn make() -> i64 { 1 }
         let x = make()              // live value: Int(1)
cell 2:  fn make() -> str { "s" }    // every earlier ITEM still type-checks;
                                     // `let x = make()` re-infers x : str. No E2400.
cell 3:  str_len(x)                  // type-checks; runtime holds Int(1). Wrong answer.
```

The re-checked text and the live heap diverge, **with a false compile-time blessing on top** — the exact
Python failure mode this spec claims to eliminate.

**v1 is immune by construction** (§2.3): `x` is materialised as the literal `1`, so cell 2 re-types it
`i64` because it is. **This immunity is a requirement, not a happy accident** — if v2 (b) is ever built,
it must pin each binding's type at execution time and fire E2400 when a redefinition would change a
*live binding's* pinned type, not merely when an item stops checking. Recorded here so v2 cannot be
built without confronting it.

### 4.3 S9 — the re-check is not idempotent today

`run_check_pipeline_located` **mutates** the Program. `load_use_decls` (`lib.rs:483-534`) *prepends*
every imported module's items into `program.items`, and its `already_loaded` set is a **local per call**
(`lib.rs:513`). The `UseDecl` items remain. So checking the same accumulated Program twice re-loads and
re-prepends → duplicate `fn` definitions → **E0002 on cell 2 for any session that used `use` in cell 1**.
`fill_captures` (`main.rs:4618`) likewise mutates lambda capture lists.

The first draft claimed re-checking "is calling it on a bigger `Program`, not writing a new analysis".
That is false. Either the session re-parses accumulated **source** into a fresh `Program` each cell (v1
does this naturally, since v1 is text), or the pipeline needs an idempotent mode. **Slice 1 must prove
idempotency with a session whose cell 1 contains a `use`.**

### 4.4 S7's honest scope — what rollback cannot undo

"Byte-identical" holds for **check-tier** failures (S4). For a **runtime** failure it does not, and the
first draft's Q2 promise ("all-or-nothing is the rule a user can state") was false. A panicking cell may
already have:

* mutated a **pre-existing** `Dict`/`Chan` through its shared `Rc<RefCell<..>>` — sharing is the
  documented design (`interp.rs:36+`);
* written files, sent HTTP, appended provenance, spent AI budget (`ai_cost_micro`);
* tripped `corrigible_halted` — a **deliberately one-way latch** (`interp.rs:507-514`) that rollback
  is **forbidden** to clear.

**S7 therefore states:** the cell's items and new bindings are discarded; **effects and mutations to
prior shared state are not undone**; the corrigibility latch survives rollback by design. Saying so is
the requirement — a session that claimed transactionality it does not have would be worse than one that
never claimed it.

---

## 5. Audit, replay and containment

| # | Rule |
|---|---|
| A1 | A session is **one run-id**, stamped at session start, not one per cell. Every provenance record carries a `cell` index. |
| A2 | `--record` writes **one journal for the whole session**; cell boundaries are journal events. `--transcript` records the cells AND the flags the session ran under — a session recorded with `--require-contained` has cells refused at check time that performed no I/O, so replaying the same transcript without the flag runs them, reaches the world, and diverges (exit 11, verified). The pair is self-contained only with the flags, and the header now carries them. |
| A3 | Replaying a session reproduces it cell by cell, diverging (exit 11) at the first departing cell. **This is NEW machinery, not "the existing divergence machinery unchanged"** — a host journal records `AxonHost` calls and their outcomes, *not the cells' code*, so the session transcript must also be persisted and fed back. A2's cell-boundary events are themselves a journal format change. Scoped in Slice 4; sized honestly here because the first draft understated it. |
| A4 | The R28 ledger flushes **once at session end**; its integrity check covers every cell. |
| A5 | `axon trace --ai` keeps `(fn, src, principal)` attribution and adds the cell. Per `a9261f1`, a summary must not merge records across the thing it attributes — a session adds a dimension and must not lose one. |

---

## 6. Error codes

Reserved band **E2400–E2404**. Re-grep before allocating.

| Code | Meaning |
|---|---|
| `E2400` | redefinition breaks an earlier accumulated item (§4.1) — the headline diagnostic |
| `E2401` | cell references a name never bound in this session; help must distinguish *never bound* from *bound but not persistable* (S10) and from *an earlier cell failed to accumulate* |
| `E2402` | cell is not a usable fragment. **Landed 2026-09-17** for the case that actually bites: a cell declaring its own `fn main`. That collides with the one the session composes, and the raw diagnostic — "the name `main` is defined more than once" — blames a duplicate the author never wrote and cannot see. Not exotic: a model writing Axon knows programs have a `main`, and model-written code is this feature's audience. Refused rather than silently unwrapped, because a session that rewrites what you typed is worse than one that explains itself |
| `E2403` | session protocol error (malformed frame under `--protocol jsonl`) |
| `E2404` | a binding could not cross the cell boundary and was named rather than dropped (S10) — a **note**, not an error, unless the next cell references it |

No new exit codes. A failing cell is exit 2 in one-shot mode; interactive sessions stay live and carry
the code in the protocol frame.

---

## 7. Invariants touched

* **I-2.** v1 adds no second execution semantics: a cell is an ordinary program run. Native codegen does
  not participate; `axon build` is unaffected. v2 (b)+(d) would touch `Interp`'s core and must re-argue this.
* **I-11.** Upheld via S8, tested not assumed.
* **I-13.** Upheld via A1–A5.

---

## 8. Slices

| Slice | Content | Gate |
|---|---|---|
| **0** | ✅ **CLEARED 2026-09-16.** Spiked all four hazards against a composed session, not a mock: H1 assignment to a persisted binding, H2 a `use` in cell 1, H3 type pinning, H4 what redefinition does today. | **PASSED.** H1 `rows = rows + [99]` → len 2→3, sum 102. H2 no E0002. H3 `let x = 1` materialised; `x + 1` = 2 after `make` was redefined. H4 E0002+E0102 — Slice 2's work, as specced. |
| **1** | ✅ **LANDED 2026-09-16.** `axon session` verb; S1 bindings-in-`main`; S2/S3/S4; S9 idempotency; S10 non-persistable reporting (pulled forward from Slice 3 — it fell out of the materialiser for free); S11 no warning storm. | **PASSED.** 8 regression tests, each RED against a HEAD-built binary. |
| **2** | ✅ **LANDED 2026-09-17.** S5 + E2400. Items became named, replaceable units (`SessionItem`) with per-item line spans, so a check error can be attributed to the item that owns it and promoted when that item belongs to an earlier cell. | **PASSED.** §4.1 exactly: cell 3 refused, `g` named with its cell, cell 4's `g()` still returns 2. |
| **3** | ✅ **LANDED 2026-09-17.** S6 trailing values (via a reserved capture binding + a conservative rewrite with a type-check probe and unwrapped retry); S7 + §4.4 honest failure scoping. (S10 landed early in Slice 1.) | **PASSED.** Scalar/array/str values display; a `println` cell shows no unit; a panic leaves cell N−1 usable and the message no longer claims the world was restored. |
| **4** | ✅ **LANDED 2026-09-17.** A1 one run-id + cell-indexed provenance; A2 one journal per session; A3 `--transcript` + replay/divergence; A4 ledger flush at session end; A5 falls out of the cell field. S8 was already true and is now pinned. | **PASSED.** A tampered cell diverges at event 0 with exit 11; `AXON_ALLOWED_EFFECTS=Pure` refuses `println` inside a cell. |
| **5** | ✅ **LANDED 2026-09-17.** JSON-framed input (`{"cell": "…"}`), E2403 on a malformed frame, `axon-session/1` reply per cell. | **PASSED.** 21 cells driven with state carried throughout; a refused cell is reported without ending the session. |

---

## 9. Stop condition

```
DONE = §4.1 passes EXACTLY — cell 3 refused, `g` named, session state unchanged
   AND §1.2 passes — `rows = rows + [record]` works and the value survives to the next cell
   AND §4.3 passes — a session whose cell 1 contains a `use` does not E0002 on cell 2
   AND no prior cell's side effect re-runs when a later cell executes (S3)
   AND a non-persistable binding is NAMED with its shape and reason (S10), never silently absent
   AND §4.4 is documented in the user-facing text, not just here — rollback must not claim
       transactionality it does not have
   AND a recorded session replays whole (A3), and a tampered cell diverges at that cell with exit 11
   AND a split effect is refused at the ceiling (S8)
   AND the full suite is green IN EVERY CONFIGURATION THE GATE RUNS, each reported with its
       configuration — per R42 §12.1, "the suite passes" is not a claim until it names which
       suite, built how
   AND measured per-task against the stateless arm, with the stateless control's score printed
```

The last clause is not optional. The arm sweeps were **stopped on 2026-09-08** because the thesis came
out unsupported and the stateless control *won* the error column — a metric a control can win is
measuring structure, not reuse. **Do not justify this spec with a projected score.** Build it because
the capability is absent and its gate is passed; measure afterwards, with a control inert on the metric.

---

## 10. Performance budget

Re-checking the whole module every cell is O(cells × items) — milliseconds at session lengths a model
produces, and `run_check_pipeline_located` already runs on every `axon run`.

**Deliberately not made incremental.** An incremental re-check is a second analysis that must agree with
the first — the I-2 failure mode. If profiling demands it, make the whole check faster rather than add a
cache that can disagree. Revisit only with a measurement, recorded here.

One real risk inherited from the prior spec: **repeated string/array concatenation in an accumulation
loop may be O(n²)** now that `+` is defined on both. Measure before a session invites that pattern.

---

## 11. Rollout & rollback

Additive. New verb; `run`/`check`/`build` untouched, so rollback is removing the verb.

The first draft claimed "a session transcript concatenates into an ordinary module `axon check`
accepts". **False, two ways:** `Item` has no expression variant (`ast.rs:19-37`), so a trailing
expression is a parse error; and a session containing a redefinition concatenates into a module with two
`fn f` definitions → E0002. The salvageable and *useful* version: **the accumulated program a session
exports — post-replacement, statements composed into `main` — must be a file `axon check` accepts.**
That is worth keeping as a test, because an exportable session is how a session's work leaves the session.

---

## 12. Open questions

| # | Question | Default if undecided |
|---|---|---|
| **Q1** | ✅ **RESOLVED 2026-09-16 by the Slice-0 spike.** v1 = (c). All four hazards passed; §4.3 and §4.2 turned out not to bite v1 *at all* — a fresh parse per cell makes the re-check idempotent for free, and materialisation pins types for free. Both are now regression-tested rather than assumed. | — |
| Q2 | ✅ **RESOLVED and IMPLEMENTED 2026-09-17.** Every cell now carries its outcome in the transcript — `ok`, `REFUSED at parse/check — did not execute, did not accumulate`, or `FAILED at runtime — bindings discarded, but any effects it already had were NOT undone`. The three are kept distinct because they mean different things about the WORLD. The marker is an Axon comment, so a marked transcript still replays (tested); and the outcome is appended AFTER the cell body, so a process that dies mid-cell leaves the body unmarked — itself the honest record that the cell did not complete. | — |
| Q3 | Does `--require-contained` belong here or in its own spec? | Its own — it is meaningful for one-shot `check` too. S8 only requires they compose. |
| Q4 | When does v2 (b)+(d) open? | Only when measurement shows S10's refusal set actually bites — i.e. real sessions lose real work to closures/aliased dicts. Not before. |
| Q5 | Does a session persist to disk for resumption after the process exits? | Under v1 this is nearly free (the session *is* text). Ship it if Slice 1 gets it for nothing; do not build machinery for it. |
| Q6 | `mod` imports per cell or once? | Per cell, matching `run` — but §4.3 makes this load-bearing, not cosmetic. |
| Q7 | S1 moves bindings into `main`'s scope. The prior spec flagged the cost: **declared `fn`s can no longer see session bindings** (they could via the globals fallback, `eval.rs:74`). Accept? | Accept — it forces parameters and fails closed. But it is a behaviour change and must be in the user-facing text, not only here. |

---

## 13. Dependency DAG

```
R44 Slice 0 (spike — kill-gate Q1 against §4.3 + §1.2)
  └── Slice 1 (S1 bindings-in-main, S2/S3/S4, S9 idempotency, S11)
        └── Slice 2 (S5 + E2400)          ← the product
              ├── Slice 3 (trailing values, §4.4 scoping, S10 reporting)
              ├── Slice 4 (audit/replay/containment — incl. A3's new machinery)
              └── Slice 5 (jsonl protocol)

AXON_FOR_RLM §4 (--require-contained)  — independent, composes at S8
(d) Interp lifetime refactor ──► v2 (b)   — NOT scoped here; Q4 gates it
```

R44 is a leaf: nothing blocks it, it blocks nothing. That is the main argument for doing it before any
of the six Draft platform specs.

**R15 is cited but deliberately NOT a dependency edge.** A cell runs to completion; nothing suspends
mid-cell. R15 is *precedent* (its `SendValue` `Chan` refusal is the model the prototype already follows)
and was mis-cited in the first draft as a *shape* — `run_suspendable_stdio` inverts control the wrong
way: the program calls `host_await` and the host supplies **data**, whereas a session is the host
supplying **code**. Recorded so the false edge is not re-added.

---

## 14. Evidence ledger

Slice 0 cleared and Slice 1 landed 2026-09-16. Slices 2–5 remain.

**Slice 0 — the kill-gated spike.** Composed a real session (module items accumulated as text,
bindings materialised back) and ran all four hazards:

| Hazard | Result |
|---|---|
| H1 §1.2 — assignment to a persisted binding | PASS — `rows = rows + [99]` → len 2→3, then sum 102 in a later cell |
| H2 §4.3 — a `use` in cell 1 breaking cell 2 | PASS — no E0002; a fresh parse per cell makes the re-check idempotent |
| H3 §4.2 — type pinning | PASS — `let x = 1` materialised; `x + 1` = 2 after `make` was redefined |
| H4 — redefinition today | E0002 + E0102, i.e. Slice 2's work, as specced |

§4.3 and §4.2 turned out **not to bite v1 at all**. Both are now regression-tested rather than
assumed, because a property that holds by accident is one a later refactor removes silently.

**Slice 5 — landed.** The jsonl host driver, and the bug that made it unusable.

`--protocol jsonl` took a raw line and turned every `\n` in it into a real newline. That splits
`println("a\nb")` into two lines, the second of which the composer indents into the middle of the
program — **so the cell that ran was not the cell the host sent**, and it reported success. A `\n`
escape in a string is common enough that the protocol was unusable for exactly the hosts it exists to
serve, and nothing said so.

The frame is now JSON, so JSON's own escaping carries the cell intact and a multi-line cell rides in
one frame. A malformed frame is refused with **E2403** and a specific reason — "malformed" alone tells
a driver nothing about which of its frames to fix — rather than guessed at, because guessing means
running a mangled cell and reporting `ok:true`.

**Slice 4 — landed.** Audit, replay and containment.

Two findings, both the defect class this cycle has been chasing:

* **`AXON_RECORD=… axon session` wrote no journal and said nothing about it.** The user asked to
  record and got silence, which reads as "recorded" until someone goes looking for the file. The
  journal is now installed once at session start, so a session produces ONE journal covering every
  cell — A2 as specced.
* **A3 needed less machinery than §5 claimed, and one thing §5 missed.** Replay works today: feed the
  same cells back under `AXON_REPLAY` and the session reproduces with the environment stripped; a
  tampered cell diverges at the first departing event with exit 11. But a journal records what the
  session TOUCHED, not what it RAN, so `AXON_RECORD` alone leaves an auditor holding half the
  evidence **and no way to know it**. `--transcript PATH` writes each cell as it runs — before it
  runs, and whether or not it succeeds, since a transcript that omitted the failing cell would replay
  a different session and the journal would diverge with no clue why. The `(journal, transcript)`
  pair is self-contained.

A1 stamps one run-id per session and a `cell` index on every provenance record. The stamp is inert
outside a session (0 means "not a session", nothing is emitted), and a test with a deliberately
always-emitting mutant confirms that guard works — otherwise every existing provenance consumer would
see a new field on records that have no cells.

S8 turned out to be **already true**: each cell is an ordinary program run, so `AXON_ALLOWED_EFFECTS`
applies to it unchanged. It is pinned by a test rather than claimed, because "a session cannot launder
an effect across cells" is exactly the kind of property that holds until someone optimises the cell
loop.

**Slice 3 — landed.** Trailing-expression values and honest failure scoping.

The value is captured through a reserved binding and stripped from the prelude, so it is the cell's
answer rather than something the user bound. The rewrite that produces it is deliberately
conservative — and additionally **type-checks a candidate program first and retries unwrapped if it
does not check**, so the heuristic cannot cost a working cell. That probe's diagnostics are silenced:
they are about a program the session wrote on the user's behalf, in a temp file the user never named.

§4.4 became user-facing, which was the point of writing it down. A runtime failure used to report
*"session is unchanged"* — true for a cell refused at check time, false for one that already wrote a
file. It now says the bindings were discarded but *"anything it already did to the outside world was
not undone"*, and a test asserts the written file really does survive, so the claim is checked in
both directions rather than merely softened.

**Slice 2 — landed (the product).** Accumulated items became named, replaceable units with per-item
line spans in the composed program, so a check error can be attributed to the item that owns it and
promoted to **E2400** when that item belongs to an earlier cell. §4.1 now reads:

```
[E2400] redefining `f` breaks `g`, defined earlier in this session (cell 2)
        — type mismatch in arithmetic operands (expected i64), found str
note: the session is unchanged — `f` still has its previous definition
```

Three things that were not obvious until it ran:

* The checker reports this break **three times** — twice located inside `g`, once with no resolvable
  span at all. Printing all three tells the reader nothing the first told them, so knock-ons are
  suppressed once the cause is named. Only *unlocated* ones: a located error elsewhere in the cell is
  a separate mistake and is still shown.
* An annotated item must key on its **name**, not its first line, or `@[test] fn t()` re-run in a
  later cell stacks duplicates until E0002.
* Replacement happens **in place**, preserving source order, so "defined earlier in this session" is
  literally true rather than an approximation.

**Slice 1 — landed.** `axon session` (+ `--protocol jsonl`, `--show-program`); the prototype's
materialiser promoted from env-var-only to an API (`set_session_capture` / `take_session_result`);
S1 bindings-in-`main`; S4 failed cells do not accumulate; S10 non-persistable bindings named (pulled
forward — it fell out of the materialiser for free); S11 no warning storm.

28 regression tests (8 Slice 1, 5 Slice 2, 6 Slice 3, 4+1 Slice 4, 5 Slice 5), **each RED against the prior commit** except the guards noted below.

Two of them were initially worthless and only mutation testing found it:

* the warning-storm test passed vacuously — with no `session` verb the binary emits nothing, which
  satisfies "zero warnings" trivially. It now asserts the session *ran* before asserting it was quiet.
* the over-suppression guard survived a deliberately blanket-suppressing mutant **twice**. First
  because its cell redefined a name harmlessly, so nothing was ever blamed and the suppression path
  was never entered; then because the unrelated error it used was *resolver*-phase, emitted before any
  blame exists and therefore unsuppressable either way. It now uses a checker-phase error in a cell
  that genuinely breaks an earlier item, and kills the mutant.

Slice 3 added two more of the same kind, both caught the same way. The probe-silence guard survived a
deliberately un-silenced mutant because its cell's trailing expression *referenced* the prelude
binding — so the probe program had no unused binding, emitted no warning, and the test passed either
way. It now uses a trailing expression that leaves the prelude binding unread.

Recorded because all four failures share a shape: an assertion that passes on correct code **and** on
the broken code it was written to catch. RED-against-the-prior-commit does not distinguish them —
running the mutant does. Guard tests for defects a feature *introduces* are exactly the ones
RED-proving cannot validate, because they pass before the feature exists.

Two defects found while building, both worth recording because neither would have surfaced from
reading:

* `SESSION_RESULT` was first a `thread_local`. `on_deep_stack` runs the program on a separate thread
  (the interpreter needs a bigger stack), so every cell read `None` and the symptom was *"every cell
  failed at runtime"* — a misleading enough presentation to be worth the comment now in the source.
* `value_as_literal`'s skip reason was the Rust `Debug` of the value, so a closure that could not
  persist reported `Closure { params: ["x"], body: BinOp { op: Add, … } }`. It is a shape now — the
  message exists to tell a reader what happened to their binding.

| Claim | Evidence | Where |
|---|---|---|
| Module-scope bindings cannot be assigned | `let rows = [1]` + `rows = arr_concat(…)` → `E0001 cannot assign to function name` | run 2026-09-16 |
| `+` on `str` and `[T]` has landed | `"a" + "b"` → `ab`; `rows = rows + [2]` → len 2 | run 2026-09-16 |
| A session prototype already exists | `AXON_DUMP_BINDINGS`/`AXON_DUMP_SHAPES`, `Interp.main_locals` | `interp.rs:643, 2195-2275` |
| …and it carries Dicts, refusing only aliased ones | alias detection + skip-with-reason, R15 `Chan` precedent in-comment | `interp.rs:2233-2275`; test `cli_run.rs:20533` |
| `Interp` borrows the `Program` | `Interp<'p>` holds `&'p FnDef`/`&'p Expr`; `build(program: &'p Program)` | `interp.rs:469-477, 2490` |
| The re-check mutates the Program | `load_use_decls` prepends imports; `already_loaded` is per-call | `lib.rs:483-534` |
| Module-level `let`s are typed by re-inferring their bodies | `infer_program` → `infer_expr(value, …)` per `Item::LetDef` | `infer.rs:1662-1671` |
| `corrigible_halted` is a one-way latch | documented as such | `interp.rs:507-514` |
| `Item` has no expression variant | enum listing | `ast.rs:19-37` |
| `init_globals` is private; `run_check_pipeline_located` is private in main.rs | signatures | `interp.rs:2600`; `main.rs:4464` |
| E24xx is free | zero hits outside this spec across `crates/` + `governance/specs/` | grep 2026-09-16 |
| AXON_FOR_RLM §1/§2/§3 landed; §4 has not | `axon run` emits located `I0002`; uncontained `/etc/passwd` read passes `check` exit 0 | runs 2026-09-16 |
