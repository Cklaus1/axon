# Making Axon the best RLM engine — five changes, ordered by measured impact

Derived from the Axon spike in `spikes/rlm-engine/src/axon_engine.rs` (2026-08-06). Every claim
below is tied to a measurement; nothing here is inferred from reading the source.

> **[REVIEWED 2026-08-06 — build-loop Step 1.]** The harness that produced every number here lives
> in a *different repo*: `/home/cklaus/projects/aicoding/atlas/spikes/rlm-engine`, alongside
> `RLM_MODE_SPEC.md`. Nothing under `spikes/` exists in the axon tree. Changes land here;
> measurements re-run there. Both trees are committed to separately and neither may be committed to
> on the other's behalf.
>
> **All five measured claims were re-verified independently against `axon 0.1.0 (c6e5eb5)` before
> any were built on.** Each is confirmed. Two are confirmed but *misdescribed* in ways that would
> have produced the wrong build — see the `[REVISED]` markers on §2 and §3. One prerequisite the
> spec does not list was found by the same pass and added as §2b.
>
> Re-verification transcript (all reproduced at HEAD):
>
> | claim | verdict |
> |---|---|
> | §1 no `help` at parse tier | ✅ `E0000` JSON has no `help` key |
> | §2 `run` carries no `help` | ✅ — and no `file`/`line`/`col` either; see §2 `[REVISED]` |
> | §3 `let mut count` → unrepairable | ✅ `unexpected token: Ident("count"), expected Eq` |
> | §4 unannotated `/etc/passwd` read passes | ✅ passes `check` **and runs**, printing the file |
> | §5 no session | ✅ 22 CLI verbs, no `repl`/`session`; every `run` is a fresh process |
>
> One correction to §4's evidence, not its conclusion: the obvious test program does not compile
> (`read_file` returns `Result<str,str>`, so `println(read_file(…))` is a type error, E0102/E0306).
> A *well-typed* unannotated read is required to test the claim, and it passes `check` with exit 0
> and then prints `/etc/passwd`. The claim holds; the naive repro would have "confirmed" it for the
> wrong reason.

**Baseline measured:** R9 0/8 zero-shot → 0/8 after repair (worse than Rhai's 1/8, and for a
different reason — the model emitted D and verbatim Python, not bad Axon). With a README-derived
primer, 3/8. Compile-time containment 4/4 with positive controls, **including the network row
Landlock ABI 3 could not fill.**

---

## 1. Put `help` on parse diagnostics — the single highest-leverage change

**Measured:** `help` is present at the type tier (`E0102`, `E0307`) and the unknown-method tier
(`E0403`), and **absent at the parse tier (`E0000`)**. And **100% of the model's failures were parse
errors.**

So Axon's best diagnostic feature never fired. The dominant failure was `let mut count` — a Rust
habit — which produces:

```
E0000  unexpected token: Ident("count"), expected Eq
```

A model reading that does not learn that `mut` is the problem. Compare what the type tier already
does well:

```
E0307  help: the function declares `-> i64`, but the body produces `str` —
       adjust the final expression (or change the declared return type)
```

**Recommendation:** add `help` to parse errors, keyed on the *token that was actually seen*, and
target the specific foreign-language habits models bring. `mut`, `const`, `:=`, `function`, `def`,
`;`, `var`, `->` in the wrong position, type annotations in unsupported positions.

**Why this is first:** it is the difference between a dead-end error and a repairable one. It also
makes the check-then-run hypothesis *testable* — the spike could not test it, because at the parse
tier `check` knows nothing `run` doesn't, so the measured delta of 0 says nothing about the idea.

---

## 2. Stop `axon run` stripping `help`

**Measured:** `run` carries no `help` at any tier, while `check` carries it at two of three.

Whatever value the diagnostics have is discarded on the path most callers use. A model in a
run-and-see loop — which is what every other engine in the benchmark does — never sees the guidance
that exists.

Cheap, and it makes the two loops comparable on their merits rather than on which one happens to
preserve the payload.

> **[REVISED: scope widened; mechanism corrected. Confirmed, but this is not a policy difference —
> it is a lossy string round-trip, and it costs more than `help`.]**
>
> The title reads as though `run` decided not to carry `help`. It didn't. `cmd_run`
> (`crates/axon-core/src/main.rs:3578`) calls `run_check_pipeline`, the wrapper at `main.rs:4146`
> whose entire body flattens every typed diagnostic to `format!("[{code}] {message}")` — because it
> passes `""` for the source text, so no span can resolve. `emit_error` (`main.rs:4772`) then
> *re-derives* JSON from that string via `diag_schema::diagnostic_json`, which can only recover a
> `help` that was already inside `message` as a `help:` line. Typed diagnostics keep `help` in a
> `help: Option<String>` field (`lib.rs:167`), so it is dropped, along with `file`, `line`, `col`,
> `expected` and `found`. The located variant `run_check_pipeline_located` already exists two
> functions away and is what `check` uses.
>
> So: **`run` loses the location too, not just the help** — a model gets neither what is wrong nor
> where. And the fix is smaller than the spec implies: pass the source through and emit the typed
> diagnostic, deleting the flattening wrapper rather than adding a parallel path.
>
> **And a second hole at the tier that matters most.** `cmd_run` parses with `parse_source`
> (`main.rs:3600`), not `parse_source_located`, so at the **parse tier** `run` emits bare prose —
> `error: parse error: unexpected token…` — with no JSON, no code, and no location at all. Since
> 100% of the model's failures are parse errors, this half is the whole of §2's value, and
> implementing §2 as literally written ("stop stripping `help`") would leave it in place. §1's new
> help text is worth exactly nothing to a run-and-see host until this is fixed, which is why the two
> are sequenced together rather than ordered 1-then-2.

---

## 2b. `@[contained]`'s own `help` is not machine-readable — added by review, not in the original

**Measured at HEAD**, not in the original spec. The containment refusal emits:

```json
{"schema":"axon-diag/1","code":"E1001","message":"`read_file(\"/etc/passwd\")` is not permitted by
 @[contained] (allowed prefixes: \"./data/\")\n  help: Add `read(\"/etc/\")` to the existing
 `fs: [...]` clause"}
```

The help text is **inside `message`**, not in the `help` field, even though `PipelineDiagnostic`
has one and `E0307` populates it correctly. A consumer that reads `help` — which is the entire
point of the versioned schema, and which §4's host must do — sees no help on the one diagnostic
§4 exists to produce. `diag_schema::split_help` already extracts `help:`-prefixed lines, so the two
emission paths disagree about where help lives.

This is a prerequisite for §4 being useful rather than merely correct, and it is cheap.

---

## 3. Accept the syntax models actually emit, or refuse it by name

**Measured:** the primed arm still scored 3/8, still all parse errors, still `let mut count`.

A primer told the model the rules and it wrote `mut` anyway, because a language model's priors are
not overwritten by one paragraph. Two options, and either beats the current state:

- **Accept `mut` as a no-op** where bindings are already immutable, so the habit is harmless; or
- **Refuse it by name**: `E0000: `mut` is not an Axon keyword — bindings are immutable by default;
  write `let count = ...``

The second is better. Silently accepting a keyword that means something elsewhere teaches the wrong
model of the language, and the whole premise of a compile-time boundary is that the compiler is the
authority on what the code means.

> **[REVISED: merged into §1 as its first case. Not a separate change — the same change, applied to
> the highest-frequency token.]**
>
> §3's recommended option ("refuse it by name: `` `mut` is not an Axon keyword — bindings are
> immutable by default ``") *is* §1's mechanism — help keyed on the token actually seen — evaluated
> at `mut`. Building them as two tasks means editing one function twice and writing the keyword
> table twice. They are one task with `mut` as its first and best-evidenced entry.
>
> The recommendation itself survives review unchanged: **refuse by name, do not accept as a no-op.**
> Accepting `mut` silently would also be the more dangerous of the two here specifically, because a
> model that gets away with `mut` learns nothing and the next token it invents (`const`, `var`,
> `let mut ref`) has no such accommodation waiting — the accommodation does not generalise, and the
> diagnostic does.

---

## 4. `--require-contained` — make containment the default for model-written code

**Measured:** the same `/etc/passwd` read with **no** `@[contained]` annotation **passes check.**

Containment is opt-in, so the compiler is a boundary only over code that asked to be bounded. For an
RLM host that is the wrong default: the host would have to inject the annotation into model-written
source, which is exactly the fragile, rewrite-the-model's-output pattern that
`RLM_MODE_SPEC.md` §3 was trying to avoid.

**Recommendation:** a flag (`axon check --require-contained`, or a policy file) that treats every
unannotated function as `@[contained(fs: [], net: [], exec: none)]` and requires an explicit
annotation to widen. The host then passes a flag instead of editing source, and the annotation
becomes a *grant* rather than an *opt-in*.

This is the difference between "§3's gate problem relocated" and "§3's gate problem solved".

> **[REVISED: recommendation upheld; three under-specifications resolved in Step 2 — one of them
> security-relevant.]**
>
> The claim and the recommendation both survive. But "treats every unannotated function as
> `@[contained(fs: [], net: [], exec: none)]`" leaves three questions the implementation cannot
> avoid answering, and one wrong answer makes the flag decorative:
>
> 1. **Which verbs honour it?** If `--require-contained` is a `check`-only flag, an RLM host that
>    calls `axon run` — the verb §2 establishes every other engine's loop uses — is not protected at
>    all, and the gate is bypassed by using the more convenient command. Resolved in Step 2 (D3).
> 2. **Mixed programs.** A program with some annotated and some unannotated functions: does the
>    unannotated part get zero caps (per-function default) or does the presence of any annotation
>    opt the file in? Resolved in Step 2 (D4).
> 3. **`main` itself**, which is unannotated in every program in `examples/`. Resolved in Step 2 (D4).

---

## 5. An accumulating session — the change that would make Axon an RLM engine at all

**Measured:** `axon run` compiles a file and executes it. There is no REPL and no namespace between
calls, so Axon fails the defining RLM property (bind a name in one call, read it in the next). As it
stands it is a `run_code` tool, which is `RLM_MODE_SPEC.md` §10's *stateless alternative*.

**Recommendation:** a session mode where each cell **appends declarations to an accumulating
module**; `check` validates the whole accumulated module, and `run` executes only the new tail.

This is not a workaround — it is the shape a *compiled, statically typed* language can offer that an
interpreter cannot:

> **Every prior binding in the session is re-type-checked before the new cell runs.**

Python's kernel cannot do that. It is also the direct answer to the failure D2 measured on the Python
side: the model reused `rows` and guessed its shape wrong, scoring 3/5 against stateless's 5/5,
because a name carries no type. In an accumulating typed session, **using a binding at the wrong type
is a compile error before anything executes** — requirement 6 (namespace-aware compaction) satisfied
by the type system instead of by an inventory the host has to build and pay for every turn.

If Axon builds one thing from this list beyond the diagnostics, this is it.

> **[REVISED: claim confirmed; item left GATED and unbuilt this run — it is four changes, not one,
> and the spec itself gates it behind a measurement that has not been taken.]**
>
> "No REPL and no namespace between calls" is confirmed: 22 CLI verbs, none of them `repl` or
> `session`. But "each cell appends declarations to an accumulating module; `check` validates the
> whole module, `run` executes only the new tail" is a session store *plus* an accumulation model
> *plus* new execution semantics *plus* a CLI surface — and the third has no meaning until someone
> decides what a cell is. Axon programs are whole programs with a `main`, and `run` runs `main`;
> "execute only the new tail" is not a small delta on that, it is a second execution mode.
>
> That is not an argument against building it. It is an argument against building it *before* the
> measurement the spec itself says gates it, which is exactly what the sequencing section says and
> is upheld here. See D6.

---

## What none of this fixes

**R9 is a precondition, and it is upstream of everything above.** The spike's conclusion generalises:

> A boundary the model cannot write code for is a boundary that does not exist.

0/8 zero-shot is the whole of that argument. Axon can have the best containment story in the
benchmark — and on the network row it measurably does — and still be unusable, because containment
you cannot invoke is not containment.

The Lua result says how much is recoverable: **one clause of primer moved Lua from 0–1/8 to 6/8 on
all three runs.** That is the encouraging precedent. The discouraging one is that Lua's problem was a
*host-convention mismatch* over a language the model already knew, whereas Axon's is that the model
has essentially never seen the language. Those are not the same distance.

So the honest sequencing for Axon-as-RLM-engine is:

1. diagnostics (1, 2, 3) — cheap, and they make every later measurement interpretable;
2. re-run R9 with a proper LLM-shaped language card, not a README excerpt, and see where fluency
   actually lands;
3. only if that clears a usable bar, build the accumulating session (5) and `--require-contained`
   (4), which are the two that would make Axon genuinely differentiated rather than merely safe.

Step 2 is a gate, not a formality. If a good primer leaves Axon at 3/8, the containment quality does
not matter for this use.

> **[REVISED: the sequencing is adopted as the build DAG verbatim. Two additions — the gate is
> executable here, and at n=8 a single run cannot decide it.]**
>
> **The gate can actually be run.** The spike asks a gateway at `http://127.0.0.1:3456/v1/messages`,
> and that gateway answers at review time. So "re-run R9 with a proper LLM-shaped language card" is
> a task this build can execute, not a handoff. What it cannot do is decide what "a usable bar"
> means — that is a product judgement about whether to spend the §4/§5 effort, and it is tagged
> `needs-human` (D6) rather than guessed at.
>
> **A single R9 run cannot clear or fail the gate.** R9 is 8 tasks, one shot each, against a
> stochastic model — one task is 12.5 percentage points, so 3/8 → 5/8 is two tasks and well inside
> the noise of a single sample. The spec's own strongest evidence respects this: Lua is reported as
> "6/8 on **all three runs**". Any number this build reports for the gate is therefore reported as
> **three runs with the per-run spread shown**, never as one number, and a decision is only proposed
> where the spread does not straddle the bar. See D5.

---

## Decisions — build-loop Step 2, 2026-08-06

Every open question in this document, resolved or classified. Engineering decisions are **canon**
from here. `needs-human` items are proposed but **not adopted and not built**.

### D1 — §1's help is keyed on the token seen, from a closed table (engineering)

The parser already reports `unexpected token: Ident("count"), expected Eq`, so both halves of the
key — what was seen and what was wanted — are in hand. Help is a **pure function of that pair**,
from a closed table, defaulting to `None`. Options considered: (a) free-form help written per parse
error site — rejected, the parse errors are raised in dozens of places and the table would not stay
consistent; (b) an LLM-generated hint — rejected outright, a compiler diagnostic must be
deterministic and offline; (c) **closed table, pure function** — chosen.

The table's entries are the foreign-language habits §1 names, each with a rewrite:
`mut`, `const`, `var`, `let` (as `:=`), `function`, `def`, `fn`-with-`;`, and `->` in a `let`.
`mut` is first and is the only one with direct measured evidence; the rest are named by §1 and cost
one table row each.

### D2 — §2 is fixed by deleting the lossy path, not by adding a second one (engineering)

`cmd_run` will parse with `parse_source_located` and check with `run_check_pipeline_located`,
emitting `PipelineDiagnostic::json()` — the same call `check` makes. The flattening wrapper
`run_check_pipeline` is **deleted** once its last caller is gone, rather than left beside the
located variant: two functions that differ only in how much they throw away is how this bug
happened, and the inner-loop rule is that obsolete paths are removed, not shimmed. If a caller
remains that genuinely wants strings, it converts at its own call site.

### D3 — `--require-contained` binds every verb that admits a program (engineering, security-relevant)

`check`, `run`, and `build`. A `check`-only flag would be bypassed by calling `run`, which §2
establishes is the verb an RLM host's loop actually uses — the gate would then protect only the
callers who were already being careful. This is the one under-specification in §4 whose wrong
answer makes the feature decorative rather than merely incomplete.

### D4 — the default is per-function, and it includes `main` (engineering)

An unannotated function is treated as `@[contained(fs: [], net: [], exec: none)]`, independently of
whether any *other* function in the file is annotated. Alternatives rejected: file-level opt-in
(any annotation opts the whole file in) would mean adding a `@[contained]` to one helper silently
widens every other function — the opposite of least privilege; exempting `main` would leave the
entry point, where model-written top-level code lives, as the one unguarded place.

### D5 — the gate measurement is three runs, reported with its spread (engineering)

R9 is 8 tasks × 1 shot against a stochastic model: one task is 12.5pp. A single run cannot
distinguish a real move from noise, and the spec's own strongest citation ("Lua 6/8 on all three
runs") is three runs. So the gate number is always reported as three runs plus the per-run spread,
and no recommendation is proposed when the spread straddles the bar.

### D6 — `needs-human`: the bar, and therefore §4 and §5 — NOT BUILT THIS RUN

**The question:** §5's sequencing says "**only if** that clears a usable bar, build the accumulating
session (5) and `--require-contained` (4)". The bar is never given a number, and the choice is not
an engineering one — it is whether Axon-as-RLM-engine is worth the §4+§5 investment, which is a
product judgement with the spec's own "if a good primer leaves Axon at 3/8, the containment quality
does not matter for this use" as the stated stopping rule.

**What this run does:** builds §1, §2, §2b, §3 (the diagnostics, which the sequencing places
*before* the gate and does not condition on it), takes the D5 measurement, and reports. It does
**not** build §4 or §5, and specifically does not build §4 behind a default-off flag — a flag
nobody has decided to turn on is built-but-uncalled code, not a partial win.

**One tension worth the human's attention, surfaced not acted on:** §4 has value to Axon as a
*language* — containment defaulting to on for untrusted source — that does not depend on the RLM
verdict at all. A reader may well want it whatever the gate says. That is precisely why it is a
decision and not an inference.
