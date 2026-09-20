# Axon Cortex

A control loop that repairs one function and can prove it did.

Cortex observes a workspace, chooses a typed action, checks whether the acting
principal is allowed to take it, executes it, and submits the result to a check
it never shows the thing that produced the result. A language model contributes
one thing to that loop — the text of a repair — and nothing else.

```bash
cargo build -p axon-cortex --bins --features ai

cortex locate --file broken.ax --check hidden_completion   # what is broken?
cortex repair --file broken.ax --check hidden_completion \
              --write-prefix broken.ax --generator ai:claude-haiku-4-5-20251001
```

```
from failing check(s) visible_repro: trying double (1.00)
verified_done after 3 step(s) — repaired `double`
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

**The adjudicator** must be able to witness the repair. A check that already
passes would accept the file unchanged, so it is refused (exit 26) rather than
run — measured, that case was 54 of 80 runs reporting success having changed
nothing. The check named by `--check` can never itself be patched: rewriting
the grader to `assert(true)` passes every later adjudication while the defect
stands, which is this crate's headline claim exactly inverted.

**The loop** acts on what a check ACTUALLY said. A file that compiles with
warnings is repaired like any other — it was not, for a while: selection ran a
check, running a check changes no bytes, and stuck-detection fired before a
patch was ever proposed. Every fixture here compiled clean; most real Axon code
does not.

**The episode** cannot leave the workspace worse than it found it. A patch that
makes the file stop compiling is undone, and the revert is recorded rather than
performed silently: an episode that hid its reverts would make a generator that
breaks the build indistinguishable from one that never proposed anything.

**Localization** ranks the functions the defect might be in, by Ochiai over
the call spectrum — the PASSING checks are evidence too, because a function
every check exercises is poor evidence for a defect. Measured against real code rather
than the fixture (`benchmarks/`), the top candidate is the sole most-suspicious
one in 38.6% of cases and the truth reaches rank 3 in 93.2%, so a run walks the
top three rather than stopping at one. A test is never a candidate. A wrong candidate costs budget and never costs correctness:
the hidden check adjudicates every attempt, and each attempt starts from the
state the run found.

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
| 25 | no repair target could be established, or the named symbol does not exist |
| 26 | `--check` already passes — it cannot witness a repair |

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
