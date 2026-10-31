# axon-intent — the Intent→Approve Gateway (R22)

Turn a human's **prose intent** into a runnable, **proven-bounded** job: synthesize
an `.ax` program, infer the **smallest** capability grant that admits it, prove
admissibility, score the synthesis, and emit a **tamper-evident Approval token**
that binds the exact `(program, grant)`. The human approves the **proof**, not the
code. `axon-os` (R21) then runs the job under that approved bound.

This is the front half of the Vision-OS v1 loop: *state intent → synthesize →
prove-bound → approve → run → audit*. Spec:
`governance/specs/R22-intent-approve-gateway.md`.

The gateway never runs, enforces, or sandboxes anything — that is R21's job. It
**fails closed**: a low-confidence, non-admissible, or over-reaching synthesis is
**refused**, never best-efforted.

## Quickstart

```bash
# Build
cargo build -p axon-intent --bin axon-intent
cargo build -p axon-os --bin axon-os

# 1. Turn prose intent into a proven-bounded job (synthesize -> least-privilege grant -> prove):
AXON_AI_MOCK=1 target/debug/axon-intent compile examples/intents/summarize.intent.md --out ./jobs

# 2. See, in plain English, exactly what it will be allowed to do + the risk:
target/debug/axon-intent review ./jobs/summarize.axjob --program ./jobs/summarize.ax

# 3. Approve the PROOF (binds the exact program+grant+import closure; needs the
#    interpreter: AXON_BIN=<absolute path to axon>):
target/debug/axon-intent approve ./jobs/summarize.axjob --by alice --accept

# 4. Watch an under-specified intent get REFUSED, not best-efforted (exit 5):
AXON_AI_MOCK=1 target/debug/axon-intent compile examples/intents/vague.intent.md ; echo "exit=$?"
```

## What an intent looks like

An `.intent.md` is prose plus named sections. `## Allowed` is the human's explicit
upper bound on authority — there is no default-allow; a missing `## Allowed` is
refused.

```markdown
# Intent
Summarize ./data/report.txt into ./out/summary.txt without using the network.

## Inputs
- ./data/report.txt

## Outputs
- ./out/summary.txt

## Allowed
- fs_read: ./data/
- fs_write: ./out/
- net: none
- exec: none
- max_label: internal

## Budget
- calls: 100
- tokens: 50000
- cost_micro: 1000000

## Seed
- 42
```

`overbroad.intent.md` PERMITS the network, but the synthesized program never
reaches it — so the inferred grant has `net=∅` (least privilege: permission the
program doesn't need is never granted). `vague.intent.md` is under-specified and
is **refused** (exit 5), no triple emitted.

### `## Examples` (optional)

Examples are the generation loop's visible acceptance check: each example is
one or more runs of the program, each run naming input fixture files (a path
under an `fs_read` prefix → its contents, staged before the run), expected
output files (a path under an `fs_write` prefix → exact contents), the expected
exit status (default 0) and optionally the exact expected stdout. A run sees
the files earlier runs of the same example left behind.

````markdown
## Examples

### counts lines
- input ./data/report.txt:
```fixture
a
b
```
- output ./out/summary.txt: "2 lines"

### keeps a running total
#### run 1
- input ./data/n.txt: "1"
- output ./out/total.txt: "1"
#### run 2
- input ./data/n.txt from fixtures/two.txt
- output ./out/total.txt: "3"
- exit: 0
- stdout: "done\n"
````

Contents are a ```` ```fixture ```` fence (never ```` ```axon ````, which is
lifted as the program), one JSON string literal, or `from <file>` — a file
under the intent file's own directory: absolute paths, `..` and symlinks are
refused, and parsing reads no other host file. Each fixture is capped at 64 KB.
A malformed example is refused (exit 2) naming its line. **Fixture contents are
sent to the model and kept in the generation transcript** — never put a secret
in one. An intent without examples still compiles; its confidence then carries
the named deduction `no-examples`.

## Determinism

Under `AXON_AI_MOCK=1` synthesis is offline + deterministic: the same
`(intent, seed)` yields a byte-identical `{.ax, .axjob, .approval}` triple. The
live-model path is opt-in and never in the acceptance gate.

## Exit codes

`0` ok · `2` usage/malformed · `3` rejected by the operator · `5` low-confidence
refusal · `8` not-admissible / grant-exceeds-ceiling.

## The R21 handoff (tamper-evidence)

The `.approval` token (`axon-approval/3`) binds the exact `(program, grant)`
digests and the program's import closure (`imports_digest`, from
`$AXON_BIN imports <program> --json` in the job's own environment). Editing the
program or grant by one byte, or rewriting an imported module, after approval
invalidates it: `axon-os run` refuses the job (exit 8) before it runs.

With `AXON_APPROVAL_KEY` (hex, ≥16 bytes) set, `approve` emits a keyed
`axmac1:` token and a keyed run accepts only those; without it the token is an
unkeyed `axtok3:` digest, accepted only by a keyless run and labelled
"integrity only". Legacy `axon-approval/1` tokens are refused under a key and,
keyless, accepted only for a program that imports nothing ("re-approve: this
token predates import binding" otherwise). Formula: R22 spec §3.5. The binding
check is `axon_intent::approval::verify_token` (which delegates to axon-os's
`verify_approval`) for program and grant. `axon_intent::cli::verify_triple` is
the whole check on a triple on disk: axon-os's `authorize` (the `/1` rule
included) plus the import closure the program loads now, compared with the one
the token binds. It therefore starts the interpreter (`axon imports`, via
`AXON_BIN`) for every `/3` token, and returns `Err` when it cannot determine the
closure or the closure changed since approval (`verify_triple_with` takes the
runtime explicitly).
