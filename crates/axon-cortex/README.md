# Axon Cortex

A control loop that repairs one function and can prove it did.

Cortex observes a workspace, chooses a typed action, checks whether the acting
principal is allowed to take it, executes it, and submits the result to a check
it never shows the thing that produced the result. A language model contributes
one thing to that loop — the text of a repair — and nothing else.

```bash
cargo build -p axon-core --no-default-features --bin axon   # the checker
cargo build -p axon-cortex --bins          # add --features ai for ai:MODEL, model rankers and the audit

cortex locate --file broken.ax --check hidden_completion   # what is broken?
cortex repair --file broken.ax --check hidden_completion \
              --write-prefix broken.ax --generator 'literal:n * 2'
```

```
from failing check(s) visible_repro: trying double (1.00)
verified_done after 3 step(s) — repaired `double`
```

(`fixtures/broken.ax`, run from a copy of it; `--axon PATH` names the
checker when `axon` is not on `PATH`.) The user guide, with every outcome
demonstrated, is [`docs/CORTEX.md`](../../docs/CORTEX.md); a runnable example
is [`examples/cortex/`](../../examples/cortex/).

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
why, as typed JSON: the VISIBLE checks' structured diagnostics (`axon-diag/1`
fields, the file paths rewritten to the workspace file), the visible tests
still failing, or — for a refused proposal — `refusal.capabilities`. The first
prompt's report of the visible tests on the file as found stays plain text,
byte for byte as before typed feedback: every task pays for that prompt, and
the same report as JSON cost ~31 input tokens on each. Nothing
the hidden check said reaches it. A generator that could read why it failed
the hidden check would be writing against the grader, at which point passing
it stops being evidence.

**The capability fence** keeps a candidate from acquiring host authority the
original code did not hold. Before the walk the pinned axon's `axon caps` (the
classifier `@[contained]` uses) runs on the unpatched file. Each target's
static grant is what that function transitively reaches. First, a candidate
body must stay inside the function it replaces: spliced in, the target is
re-located (strings and comments read as the Axon lexer reads them) and must
end exactly where the body does, so a body that closes the function and
appends items of its own, or opens a brace, string or comment it does not
close, is refused — `outside_symbol`, an `action_denied` event with reason
`body_outside_symbol`, nothing written or run. Then a candidate body
reaching anything else (`fs:write`, `net`, `exec`, a non-free `effect:NAME`,
or something the walker cannot classify) is refused BEFORE it runs — a
rejected attempt, `capability_denied`, fed back to the generator. Both count
against `--max-proposals`. Every check also runs under a runtime fence derived
from the whole file: `AXON_ALLOWED_EFFECTS` = Chan, IO, Pure, Random, Time plus
the file's own effects, and `AXON_ALLOWED_FS_READ` / `AXON_ALLOWED_FS_WRITE` /
`AXON_ALLOWED_NET` set empty (deny all) for each kind the file does not use. A
file whose own tests fail under that fence stops with `fence_conflict` (exit
22) before anything else runs. `--json` reports it as `grant`, and `cortex
grant --file F [--symbol S]` prints it without running anything. An
unclassifiable file (basis `unknown`) gets no fence, said in one stderr line;
the Fabric executor does not take the fence (`applied: false`).

**Every execution is bounded.** Each check is its own process group, killed
at `--check-timeout-ms` (default 60 s) with its output capped at 1 MiB; a
candidate past a bound is rejected and undone, and only the unpatched file
past a bound blocks (22). The walk itself has hard limits:
`--max-proposals N` caps generator calls across every candidate and
`--deadline-ms N` stops new generator calls at a wall-clock deadline; either
ends `budget_exhausted` (20) with `detail` `proposals` / `deadline`, reported
under `budget` in `--json` — unless the walk had already reached an
adjudicated state (below), which it then reports as exit 27.

**The accepted artifact is bound to its evidence.** The bytes the hidden check
graded are digested before it spawns and re-read after the last acceptance
check; a file changed in between is never accepted. `--json` reports
`accepted: {path, sha256}` on exits 0 and 27, where `sha256` is the digest the
deciding verdict graded. An adjudicated result is never discarded by later,
worse attempts: a walk that reaches an adjudicated state goes on looking for
a clean repair, and if it then ends without one, for any reason (walk or step
budget, no progress, the early stop, …), it reports that adjudication (exit
27) with its graded bytes put back, and names what ended the walk in
`stopped_by` (`{outcome, exit_code, detail}`; `budget.exhausted_by` still names
a walk limit). The file is read once more before the run reports, and if it
is no longer those bytes (changed after acceptance, or not put back) the run
fails closed: exit 22 `blocked`, `accepted: null`, `stopped_by: null`. Every
restore (between attempts, at exit, and this one) puts the found bytes back
only over bytes this run wrote (a `patch_applied` digest or the adjudicated
bytes): a file another writer changed, or one that cannot be read, is left in
place and the reason says "changed by another writer; not restored" or "could
not be read; not restored"; a restore whose write fails reports the io error.
A walk that cannot restore the found bytes between attempts ends `blocked`
there, and the run still settles and reports it this way.
The `--label-log` row is written after that final check, so it records the
exit the run exits with.

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
breaks the build indistinguishable from one that never proposed anything. A
run that ends in any non-zero exit except 27 writes the file back exactly as
it found it.

**Localization** ranks the functions the defect might be in, by Ochiai over
the call spectrum — the PASSING checks are evidence too, because a function
every check exercises is poor evidence for a defect. Measured against real code rather
than the fixture — 111 trials across five defect classes, see `benchmarks/` —
the top candidate is the sole most-suspicious one in 31.5% of cases while the
truth reaches rank 3 in 89.2%, which is why a run walks the top three
(`--candidates`, default 3; `--budget`, default 8 steps per candidate) rather
than stopping at one. The walk is breadth-first: every candidate gets one
proposal before any gets a second, then each candidate is resumed in rank
order with the rest of its own budget and its own rejected attempts, so no
candidate gets more than it would have had alone (see `docs/CORTEX.md`).
Before the walk, `--ranker` (`spectrum` | `openrouter:MODEL` |
`decision:MODEL`; env `CORTEX_RANKER`; default a Haiku 5.5 ranking when
`OPENROUTER_API_KEY` is set) may reorder those candidates but never add or
drop one, never sees the hidden check, and falls back to the spectrum order
on any failure with one stderr line and a `ranker` object in `--json`
(`src/rank.rs`). From the ranking alone it derives one line of generator
advice, `escalation` in `--json`: `cheap` whenever a ranker answered,
`unavailable` with the reason otherwise, never `strong` (DecisionBench's
route rule cannot produce one here; see `docs/CORTEX.md`).
After a VERIFIED fix, `--audit` (`off` | `openrouter:MODEL` |
`decision:MODEL`; env `CORTEX_AUDIT`; default Haiku 5.5) asks one model
whether the patch games the tests, using DecisionBench's `tamper` question,
and prints `audit SPEC: clean|suspect (...)` or `audit SPEC unavailable:
REASON`, recorded as `audit` in `--json`. It is review only: it never
changes the verdict, the exit code or the file, and it never sees the hidden
check (`src/audit.rs`). The ranker and the audit share one OpenRouter client
(`src/openrouter.rs`).
`--symbol` skips localization and tries only that function. A test is never
a candidate. A wrong candidate costs budget and never costs correctness: the
hidden check adjudicates every attempt, and each attempt starts from the state
the run found.

`--label-log PATH` (env `CORTEX_LABEL_LOG`; the flag wins; off when neither)
appends one `cortex-label/1` JSON line per run that reached the walk: the
file's sha256 as found, the check, the spectrum ranking with scores, the
ranker's report, every attempt in order, proposals per candidate, the outcome
and exit code, and `truth`, the repaired symbol (null unless `verified_done`).
The line is written after the verdict, in one `O_APPEND` write to a file
created 0600; a failed write is one stderr line and never changes the exit
code. The file's text is recorded only with `--label-source`, because source
can be sensitive (`src/label.rs`; fields, privacy note and the converter to
LocRank and DecisionBench items in `benchmarks/labels/`).

## Exit codes

One per outcome. A single non-zero would say something went wrong and nothing
about whether to widen a grant, supply a generator, or fix a PATH.

| code | meaning |
|---|---|
| 0 | the hidden check accepted the repair AND no check in the file still fails — not "the loop finished" |
| 2 | usage error, including a `--check` that names no `@[test]` in the file; nothing was decided |
| 20 | budget exhausted |
| 21 | going in circles — the generator is not helping |
| 22 | the environment is wrong: the checker could not run (missing or changed `--axon`, registry mismatch), the capability grant could not be derived, or `fence_conflict` — a test of the unpatched file needs a capability the derived fence denies |
| 23 | authority: the grant does not cover this |
| 24 | no usable proposal |
| 25 | no repair target could be established, or the named symbol does not exist |
| 26 | `--check` already passes — it cannot witness a repair |
| 27 | adjudicated, not clean: the check passed and nothing regressed, but checks that were already failing still fail. The patch is KEPT — also when the walk went on and ended short of a clean repair for another reason, which `stopped_by` names |

A check passes only with a completion token: every check child runs with
`--completion-key-stdin` and a fresh per-run key, so a body that calls
`exit(0)` (which plain `axon test` reports as `ok`) or prints a forged result
line is not accepted. See [`docs/CORTEX.md`](../../docs/CORTEX.md).

## Generators

| spec | use |
|---|---|
| `none` (default) | observe and adjudicate only; a needed patch reports exit 24 |
| `ai:MODEL` | a language model through `axon-ai`; only in a build with `--features ai` (otherwise exit 2). Key from `ANTHROPIC_API_KEY` or `AXON_AI_API_KEY` |
| `cmd:PATH` | your own program — prompt on its stdin, proposed body on its stdout; 120 s per call (`AXON_CORTEX_GENERATOR_TIMEOUT_MS`), then killed with its process group |
| `literal:BODY` | a body you already know, pushed through the same grant check and the same hidden check a model's would face |

`cmd:` exists so Cortex can be driven by whatever model you already have — a
vendor CLI, a local server, a shell script — with no credentials in this
process and no provider baked into the crate. It does not weaken the
discipline that a model never chooses an action: YOU name the command, on the
command line, before any model is involved. A model cannot select it, change
it, or reach past it; what comes back is text, validated and adjudicated like
any other proposal.

The program runs as a process-group job: the deadline, or a termination signal
to `cortex` (which then dies by that signal), kills it and everything it
started. What it leaves running after answering is not killed, and its output
is waited for at most 2 s. It is outside the terminal's foreground group, so a
program that reads `/dev/tty` is stopped until the deadline.

`literal:` is not a test double. Knowing the answer is a reason to skip the
model, not a reason to skip the adjudication — and hand-editing the file runs
neither.

The prompt carries the file with the hidden check cut out (signature, types,
siblings), the visible failing tests and what they print, and for each
rejected attempt the visible tests' output or the compiler's diagnostics -
never the hidden check's name, source or output. Where that view cannot be
built, the body-only prompt is used and `generator_context` in `--json` says
why. Details: [`docs/CORTEX.md`](../../docs/CORTEX.md#what-a-generator-sees-and-what-it-never-sees).

## Tests

```bash
cargo test -p axon-cortex --features ai
```

The conformance suite's names say what each one pins. The load-bearing ones are
the controls: a wrong-but-compiling patch must never exit 0, a compiling patch
must NOT be reverted, an explicit `--symbol` must bypass localization, and the
generator must not be asked for a body that could never be applied. Each exists
because without it the property above is satisfied by code that does nothing.
