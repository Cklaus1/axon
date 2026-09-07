# Making Axon the best RLM engine — five changes, ordered by measured impact

Derived from the Axon spike in `spikes/rlm-engine/src/axon_engine.rs` (2026-08-06). Every claim
below is tied to a measurement; nothing here is inferred from reading the source.

**UPDATE 2026-08-06 — fluency is largely solved, and it changes this document's premise.**
A purpose-built language card reaches **7/8**, stable across three card runs and three primed-repair
runs, with a same-day README control at 3/8 so it is not drift. The arc: **0/8 zero-shot → 3/8 README
→ 5/8 card → 7/8 card + a character-literals section**, against Lua's 6/8 primed and Python's 8/8
zero-shot. The single remaining failure is `mut` in a string accumulator, and it was deliberately not
chased: at n=8 tasks, another card line for that shape fits the test set rather than teaching the
language.

**What that revises.** This document's closing argument was *"a boundary the model cannot write code
for is a boundary that does not exist"*, resting on 0/8. **Zero-shot is still 0/8, so the sentence
stands — but the conclusion I drew from it does not.** A language card is a fixed, one-time cost in a
system prompt, amortised across a whole session; it is a legitimate fix rather than a workaround. At
7/8 primed, Axon's containment story is reachable in practice, and recommendation 2's gate is
**passed**.

**What it does NOT revise:** Axon still has no persistent state, so it is not an RLM engine — it is a
stateless code-execution surface with a compile-time gate, which is `RLM_MODE_SPEC.md` §10's
alternative rather than RLM. §5 below is still the change that would alter that.

---

**Baseline measured (original run):** R9 0/8 zero-shot → 0/8 after repair (worse than Rhai's 1/8, and for a
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

**Why this was ranked first:** it is the difference between a dead-end error and a repairable one, and
it makes the check-then-run hypothesis testable at all.

> **Demoted 2026-08-06, on evidence.** The 7/8 run reproduced the earlier negative result on a case
> where the advice is unambiguous: for the remaining `mut` failure **the compiler already emits a
> perfect diagnostic — code, location, and the fix — and the repair round still does not apply it**,
> in either the unprimed or the primed arm.
>
> So better diagnostics are not the lever this document assumed. **The model does not fail for lack
> of a good error message; it fails to act on one it already has.** That is a fifth failure class,
> distinct from the four in `NOTES.md`: not learnability (a primer *does* work — 0/8 → 7/8), not
> fluency, not capability (there is something to do), and not host-convention. Call it
> **advice-blindness**, and note that it makes a *prompt-side* fix (the card) beat an
> *error-side* fix (better diagnostics) on the same defect.
>
> Recommendation 1 is still worth doing — a parse error naming `mut` is better than one naming a
> token — but it should be expected to help a **human** reader more than a model, and it is no longer
> the highest-leverage item. **The card is.**

---

## 2. Stop `axon run` stripping `help`

**Measured:** `run` carries no `help` at any tier, while `check` carries it at two of three.

Whatever value the diagnostics have is discarded on the path most callers use. A model in a
run-and-see loop — which is what every other engine in the benchmark does — never sees the guidance
that exists.

Cheap, and it makes the two loops comparable on their merits rather than on which one happens to
preserve the payload.

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
