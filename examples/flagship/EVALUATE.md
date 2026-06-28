# Evaluate Axon in 5 minutes

You were handed this because someone claims Axon sandboxes AI-written code *by
construction*. You are right to be skeptical. This note is built for that — it tells
you the one claim, how to test it yourself in one command, where to be suspicious, and
how to break it. No slides.

## The claim (one sentence)

> An Axon function annotated `@[contained(fs: F, net: N, exec: E)]` cannot read a file,
> reach the network, or spawn a process outside what it declared — and a violation is a
> **compile error**, caught before the code runs once, with a policy **derived from the
> code** so it can't drift out of sync with it.

Not a runtime monitor. Not a Docker flag. A type-system property checked at build time.

## Run it (≈2 min)

From a fresh clone (needs Rust + `cargo`; no LLVM, no GPU):

```bash
./flagship --ci
```

It builds what's missing and runs two things:

1. **The four-layer demo.** An "evil" agent (`agent_task_evil.ax`) declared
   `@[contained(fs: [], net: [], exec: none)]` whose body reads `/etc/passwd`, calls an
   LLM, and shells out to `curl`. Watch it produce **3× E1001 and refuse to build**.
   Then a kill-switch halts a running agent in <1s, an audit log verifies, a kernel
   image is attested.
2. **The Docker+seccomp foil** (`compare_docker.sh`). The same three escapes in a real
   container with a hand-written seccomp profile. It blocks **1 of 3**; Axon blocks
   **3 of 3**. The script explains exactly why the other two survive seccomp.

## What to actually look at (≈2 min)

- `examples/flagship/agent_task_evil.ax` — read the source. The forbidden calls are
  right there in the body. Confirm for yourself the program is well-formed Axon and the
  *only* thing stopping it is the capability check, not a parse error.
- Run `cargo run -p axon-core --no-default-features --bin axon -- check examples/flagship/agent_task_evil.ax`
  yourself. Three `E1001`s, exit 2.
- Now **try to defeat it**: edit the evil agent to launder a forbidden effect through a
  helper function, a string-interpolated argument, an `import`, a `with`/`spawn` block,
  or an `impl` method. See if you can reach a builtin *without* an E1001. (Several such
  holes existed and were closed; the next one is a real finding.)

## Where to be suspicious (read this before believing it)

The honest boundaries are in **[THREAT_MODEL.md](THREAT_MODEL.md)**. The short version
of what this does **not** claim:

- **The compiler is in the TCB.** The guarantee is only as good as the `axon` binary.
  No reproducible build / verifying compiler yet. If you don't trust the compiler, you
  don't get the property — that's the honest top risk.
- **No covert-channel protection.** `@[contained]` governs *explicit* effects, not
  timing/cache/resource side channels. A confined agent can still leak bits through how
  long it runs.
- **The demo's attestation default is a software-TPM stand-in** (no memory encryption).
  It proves *which kernel booted*, not confidentiality of guest RAM — that needs
  SEV-SNP/TDX hardware.
- The guest kernel boots to ready state but has partial interrupt/SMP support; the
  "formally verified" language in the design docs is the *goal*, not a finished proof of
  the kernel itself.

## The one thing that makes this different from Docker+seccomp

A container's seccomp profile is a **separate artifact you maintain by hand**, enforced
**at the syscall trap** (the escape code already ran up to that point), at **syscall
granularity** ("allow `openat`" — it can't say "open anything but `/etc/passwd`"). Axon's
policy **is the code**, checked **before** anything runs, expressed over **capabilities**
(`net: ["api.anthropic.com"]`, not "allow `connect`"). Edit the agent and the policy
regenerates from the types — it can't silently drift. The foil demonstrates all three
gaps live.

## If you find a hole

That's the most valuable outcome of this evaluation — more than any feature. The four
best attacks are listed in THREAT_MODEL.md §7. If one works, it's a finding against a
named layer. Tell us what you broke.
