# Axon Cortex

A control loop that repairs one function and can prove it did.

Cortex observes a workspace, chooses a typed action, checks whether the acting
principal is allowed to take it, executes it, and submits the result to a check
it never shows the thing that produced the result. A language model contributes
one thing to that loop — the text of a repair — and nothing else.

```bash
cargo build -p axon-cortex --bins --features ai

cortex repair --file broken.ax --check hidden_completion \
              --write-prefix broken.ax --generator ai:claude-haiku-4-5-20251001
```

```
localized `double` from failing check(s): visible_repro
verified_done after 3 step(s)
```

## What each piece refuses to do

**The observer** reports what it could not establish. `Observed::Unknown`
carries the reason, so a fact the checker could not determine stays
distinguishable from one it determined to be false — and selection blocks on
it rather than treating "unknown" as "fine".

**The action catalog** is a closed enum whose parameters are determined by the
action. `claim_done` has no path to write to, `run_check` has no patch payload,
and an action outside the set cannot be spelled. Three `compile_fail` doctests
prove it, so the claim is checked by `cargo test` rather than asserted in a
comment.

**Authority** is a grant pinned to the state it was issued over, and it is
checked *before* the generator is asked. Whether this principal may edit this
path does not depend on the body, so asking afterwards would spend a model call
on an edit that could never apply — then report it as missing content when what
is missing is permission.

**The generator** receives a function body and the observation, and returns a
string. It holds no tool, chooses no action, and never learns the name of the
check that grades it. A retry is told which bodies were already rejected and
roughly how — it did not compile, or it compiled and did not fix the problem —
and nothing the grader said. A generator that could read why it failed the
hidden check would be writing against the grader, at which point passing it
stops being evidence.

**The episode** cannot leave the workspace worse than it found it. A patch that
makes the file stop compiling is undone, and the revert is recorded rather than
performed silently: an episode that hid its reverts would make a generator that
breaks the build indistinguishable from one that never proposed anything.

**Localization** returns a candidate set. Several candidates is reported as
ambiguous, never resolved by picking the first, because repairing the wrong
function fails every attempt without ever saying why.

## Exit codes

One per outcome. A single non-zero would say something went wrong and nothing
about whether to widen a grant, supply a generator, or fix a PATH.

| code | meaning |
|---|---|
| 0 | a hidden check accepted the repair — not "the loop finished" |
| 2 | usage error; nothing was decided or run |
| 20 | budget exhausted |
| 21 | going in circles — the generator is not helping |
| 22 | the checker could not run |
| 23 | authority: the grant does not cover this |
| 24 | no usable proposal |
| 25 | no repair target could be established |

## Generators

| spec | use |
|---|---|
| `none` | observe and adjudicate only; a needed patch reports exit 24 |
| `ai:MODEL` | a language model (`--features ai`) |
| `literal:BODY` | a body you already know, pushed through the same grant check and the same hidden check a model's would face |

`literal:` is not a test double. Knowing the answer is a reason to skip the
model, not a reason to skip the adjudication — and hand-editing the file runs
neither.

## Tests

```bash
cargo test -p axon-cortex --features ai
```

The conformance suite's names say what each one pins. The load-bearing ones are
the controls: a wrong-but-compiling patch must never exit 0, a compiling patch
must NOT be reverted, an explicit `--symbol` must bypass localization, and the
generator must not be asked for a body that could never be applied. Each exists
because without it the property above is satisfied by code that does nothing.
