# CORRECTION: I wired two gates on evidence from a contaminated checkout

Earlier this session I ran `scripts/r27_acceptance_gate.sh` and
`scripts/r29_acceptance_gate.sh`, observed exit 0 (7s and 4s), and wired both
into `scripts/gate.sh` (commit `57771c0`) citing those runs as the evidence.

That evidence does not support the conclusion.

## The differential

| checkout | r29 result |
|---|---|
| `/home/cklaus/projects/axon` (my main working tree) | **exit 0** |
| a worktree created fresh from the same commit | **exit 101**, 2 tests failed |

Same commit. Same script. Opposite results.

## Cause

`.gitignore:57` lists `examples/jobs/out/`.

A git worktree is created from tracked content, so an IGNORED directory does
not appear in a new one. My main checkout has `examples/jobs/out/` left over
from an earlier run; a fresh clone does not. `examples/jobs/summarize.axjob`
writes `./out/summary.txt`, takes its `Err` arm when the directory is absent,
and returns `value=1` — failing the two `axon-os` acceptance tests that r29
depends on.

So the gate is green on my machine and red everywhere else, and being ignored
is exactly why `git status` never showed me the difference.

## What this invalidates

* The claim "r27 and r29 pass, measured" is true only of a contaminated tree.
* `scripts/gate.sh` as committed at `3302160` will FAIL on a fresh clone, which
  is a regression I introduced while fixing a different class of defect.
* It does NOT invalidate the reason for wiring them. `REQUIREMENTS.md` cites
  both as evidence those requirements landed, and nothing invoked either. That
  finding stands; only my verification of their passing does not.

## The irony, recorded because it is the useful part

This session's entire thesis is that a success signal must be traced to the
predicate that justifies it. I traced these two to a predicate that RAN — and
did not ask whether the environment it ran in was representative. "The check
executed" is not the last question. "The check executed against the state a
fresh consumer would have" is.

`prefer real end-to-end cases over synthetic demonstrations` — a run in a
long-lived working tree is closer to a synthetic demonstration than it looks,
because months of ignored artefacts accumulate in it silently.

## Not fixed here, and why

The right repair is NOT a `.gitkeep`: `examples/jobs/out/` is build output and
belongs in `.gitignore`. The job or its test should create the directory it
writes to. That is a source change, and I cannot currently run anything to
verify it (the Bash safety classifier is down; only `git` and file reads are
available). Committing an unverified fix to a defect I just discovered by
catching an unverified claim would repeat the mistake in the same file.

Recorded now; repaired and verified when the tooling returns.

## Status

* `r27` / `r29` remain wired. Reverting would restore two unbacked requirement
  claims to hide a real failure — worse than a visible red.
* Treat `scripts/gate.sh` as KNOWN-RED on a fresh clone until the `out/`
  dependency is fixed and re-verified in a pristine worktree.

## The class, not the instance

`r29` is one instance. The defect is in the METHOD, so every gate wired this
session on the same method carries the same doubt. Scored by where its evidence
was actually produced:

| gate | evidence produced in | status of that evidence |
|---|---|---|
| `r39_slice3_gate.sh` | a worktree created fresh by a subagent | **clean** |
| `r39_slice4_gate.sh` | same | **clean** |
| `r39_slice5_gate.sh` | same | **clean** |
| `r26_acceptance_gate.sh` | my main checkout | UNRELIABLE |
| `r27_acceptance_gate.sh` | my main checkout | UNRELIABLE — independently shown to fail elsewhere |
| `r28_acceptance_gate.sh` | my main checkout | UNRELIABLE |
| `r29_acceptance_gate.sh` | my main checkout | **DISPROVEN** — fails in a fresh worktree |
| `r31_acceptance_gate.sh` | my main checkout | UNRELIABLE |
| `r23_acceptance_gate.sh` | a long-lived gate worktree | UNRELIABLE |

Six of nine wirings rest on evidence from a tree that differs from a fresh
clone in ways `git status` cannot show. One of those six is already disproven;
the other five are simply unmeasured, which is a different claim from "they
fail" and should not be reported as one.

Note the asymmetry that makes this worth writing down: the three CLEAN results
are clean for a reason that has nothing to do with my care. They were produced
by a subagent, and a subagent was given a fresh worktree because of an
UNRELATED concern — keeping concurrent agents from mutating one checkout. The
isolation that protects a parallel run from its siblings is the same isolation
that makes its measurement trustworthy. I got the better evidence by accident.

## A second instance, already in hand

`axon_kernel_gate.sh` skipped its microVM layer for a missing `dist/guest/`
image while firecracker and KVM were both present. `dist/` is also ignored. So
this is not a quirk of one job's output directory: it is what happens whenever
a check depends on generated state that `.gitignore` correctly excludes.

## CORRECTION to the paragraph above: that static test does not work

I proposed: *"does this gate read a path that appears in `.gitignore`?"* — and
then tested it against the case that motivated it.

    grep -c "out/" scripts/r29_acceptance_gate.sh   ->   0

It returns NO for the one gate known to be affected. The dependency is three
hops away, not in the gate at all:

    r29_acceptance_gate.sh
      -> cargo test -p axon-os              (line 59, the WHOLE crate)
        -> crates/axon-os/tests/acceptance.rs
          -> examples/jobs/summarize.axjob   fs_write = ["./out/"]

A one-hop grep cannot see that, which is the same shape as the one-hop
invocation probe that under-reported orphans by more than half. I proposed a
cheap test one tick after being burned by a cheap test. Recording it rather
than quietly deleting it, because the reflex is the point.

## What the transitive analysis DOES establish

Mapping each wired gate to what it actually executes, then checking those
targets for ignored-path dependencies:

| gate | executes | ignored-path dependency |
|---|---|---|
| `r27` | `cargo test -p axon-os` (whole crate) | **YES** — `tests/acceptance.rs` → `./out/` |
| `r29` | `cargo test -p axon-os` (whole crate) | **YES** — same path |
| `r26` | `cargo test -p axon-attest` | none found |
| `r28` | `cargo test -p axon-audit` | none found |
| `r31` | `cargo test -p axon-attest -p axon-vm` | none found |

r27 and r29 share ONE cause, and it is the same one: both run the entire
`axon-os` suite rather than a targeted target, so both inherit the acceptance
tests' dependency on a directory `.gitignore` excludes. That is consistent with
the independent empirical result — a subagent saw both fail in a fresh
worktree — arrived at from the opposite direction.

The three "none found" rows needed a second look, and the reason is worth
keeping. A naive grep DID match `axon-attest` and `axon-vm`:

    crates/axon-vm/src/main.rs:88    /// Guest kernel image (default: dist/guest/vmlinuz)
    crates/axon-attest/src/lib.rs:831   let job = b"summarize --input ./data/ --out ./out/";

Neither is a filesystem read. The first is a DOC COMMENT describing a CLI
default; the second is a byte-string used as test DATA to be hashed. Mention is
not dependency — precisely the comment-vs-caller distinction the evidence
checker had to learn for invocation, reappearing here for filesystem state.

So: the risk is concentrated in r27 and r29, with a named mechanism. r26, r28
and r31 have no identified mechanism — but they remain UNMEASURED, and "no
mechanism found by grep" is a weaker claim than "verified in a clean tree".
Only a pristine run settles them.
