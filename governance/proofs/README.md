# R32 — Formal Corrigibility Proof: `governance/proofs/`

This directory holds the two machine-checkable artifacts for **R32** (`governance/specs/R32-formal-corrigibility-proof.md`):
a TLA+ model and a companion Coq proof of three invariants of the R27 kill-switch
(`crates/axon-os/src/latch.rs`, `killchan.rs`, `corrigible.rs`).

| File | What it is |
|---|---|
| `R27KillSwitch.tla` | TLA+ model of the kill-latch/coalition state machine |
| `R27KillSwitch.cfg` | TLC model-checker config, "main" size: 5 principals, coalition ≤ 2 |
| `R27KillSwitch-3principals.cfg` | TLC config, a second (smaller) model size: 3 principals, coalition ≤ 1 — the Gate-4 WIDEN variant |
| `R27Corrigibility.v` | Coq proof of the matching theorems |

## How to check these (when the tooling is installed)

```bash
# TLA+ / TLC — requires Java + tla2tools.jar (v1.8.0 pinned by the R32 spec §8)
java -jar /usr/local/lib/tla2tools.jar governance/proofs/R27KillSwitch.tla \
     -config governance/proofs/R27KillSwitch.cfg -workers auto
java -jar /usr/local/lib/tla2tools.jar governance/proofs/R27KillSwitch.tla \
     -config governance/proofs/R27KillSwitch-3principals.cfg -workers auto

# Coq — requires coqc (8.18.0 pinned by the R32 spec §8)
coqc governance/proofs/R27Corrigibility.v
```

Or just run the acceptance gate, which does both (and degrades honestly when either tool is
missing):

```bash
scripts/r32_acceptance_gate.sh
```

## Honest status on THIS host (as of the R32 build session, 2026-07-18)

Checked directly on this build host before writing any proof code:

```
$ which tlc coqc java
/usr/bin/java
$ apt list --installed 2>/dev/null | grep -i coq
(no output)
$ find / -iname "tla2tools*.jar" 2>/dev/null
(no output)
```

**Neither TLC nor Coq is installed on this host, and neither was installed as part of this
work** (installing a JVM-based model checker or an opam/Coq toolchain is a real system change
that needs separate confirmation — the R32 build explicitly did not do this). Consequently:

- The TLA+ model has **not** been model-checked by TLC on this host. `acc_a1_tla_model_valid`
  is `SKIPPED` in every gate run here, not `PASS`.
- The Coq proof has **not** been compiled by `coqc` on this host. `acc_a2_coq_proof_compiles`
  is `SKIPPED` in every gate run here, not `PASS`.
- What *has* been verified on this host: both files exist, contain the three named invariants /
  matching theorem names, contain no stub markers (`\* TODO`, `ASSUME FALSE`) and no `Admitted.`/
  `admit.`, and every `Theorem`/`Lemma`/`Corollary` block in the `.v` file closes with `Qed.`
  before the next such block or EOF (a structural, not semantic, check — see
  `scripts/r32_acceptance_gate.sh`'s `coq_all_blocks_closed_with_qed`).
- The Coq proof was additionally reviewed **by hand**, term-by-term, reasoning through Coq's
  unification/conversion behavior for every tactic (`simpl`, `destruct`, `apply`/`exact`,
  induction on `multistep`). This is not a substitute for `coqc` actually running — a hand
  review can miss what a type-checker catches instantly — but it is the best available
  verification given the constraint against installing new tooling.

If a future session installs TLA+ tools and/or Coq, re-run
`scripts/r32_acceptance_gate.sh` — it will pick up the newly available binaries automatically
and turn the `SKIPPED` rows into `PASS`/`FAIL`. **A `SKIPPED` result must never be reported as a
`PASS`** — that conflation is exactly the kind of unverified status claim the 2026-07-18 audit
(commit `f80ef96`/`31aa126`) exists to catch.

## What is modeled

Both files model the same four-variable, four-action state machine, chosen to be a faithful
(if intentionally simplified) proxy for the real code:

- **`kill_latch : BOOLEAN`** (TLA+) / **`KillLatch := NotTripped | Tripped`** (Coq) — mirrors
  `latch.rs`'s `enum LatchState { Clear, Tripped }`. One-way: `Latch::trip` is idempotent and
  there is no `clear()`/`reset()` method on the type at all — the absence of a reverse
  transition in both formal models mirrors the absence of a reverse *method*, not merely an
  absence of test coverage for one.
- **`coalition`** — a stand-in for a colluding set of contained/adversarial principals
  attempting to defeat the kill-switch. TLA+ tracks identity (`coalition \subseteq Principals`);
  Coq abstracts this to a bare `coalition_size : nat`, since none of the three theorems needs
  anything more than the cardinality (see "What differs between the two files" below).
- **`poll_count` / `halted`** — mirrors `corrigible.rs`'s `check_kill` (a `Tripped` poll maps to
  `Verdict::Halted`, exit code 4) and the supervisor's poll loop. `PollGate` fires once per
  tick; `KillFiresWithin2Polls` asserts the job is halted within 2 ticks of the latch being
  observed `TRUE`.
- **`CoalitionAttemptDisable`** — the formal counterpart of "contained code tries to call
  something that would clear the latch". In the real system, `killchan.rs`'s `trait
  KillChannel { fn poll(&self) -> LatchState; }` is the *only* interface exposed to contained
  code (`AtomicKillChannel`, the subprocess-facing type, has no setter method at all — this is
  a Rust type-system fact, not a runtime check: `contained_code_cannot_disable_latch` in
  `killchan.rs`'s own test module exercises exactly this). Both formal models encode the same
  fact structurally: this action's *only* possible effect on `kill_latch`/`latch` is to leave
  it unchanged. There is no disjunct in the TLA+ action, and no constructor argument in the Coq
  inductive, that could ever produce `kill_latch' = FALSE` / `latch s' = NotTripped`. That is
  the actual content of "contained code cannot disable the kill switch" — not merely that this
  repository never got around to writing a disabling transition, but that the transition this
  adversary *can* take is structurally incapable of it.

## What is simplified away (and why each is safe to simplify for THIS proof's scope)

1. **Real time.** The ~100ms poll interval (`latch.rs`/the supervisor loop) is abstracted to
   unit ticks. `KillFiresWithin2Polls` is about *tick count*, not wall-clock milliseconds — the
   R27 spec's "100ms" bound is a scheduling/implementation detail, not something either formal
   language models natively without a real-time extension (TLA+'s `TLC` has no wall-clock
   semantics; Coq would need an explicit `Duration` type). Out of scope for R32; noted here so
   a reader doesn't infer more than is proven.
2. **`FileKillChannel`'s "absent file = Clear" fail-open-for-absence corner case.**
   `killchan.rs`'s `FileKillChannel::poll` treats a *missing* kill file as `Clear` (see the
   comment `// file absent = not yet tripped (fail-open for absence)`), which is a *different*
   failure mode than "channel error → Tripped" (the fail-closed default stated in the module
   doc-comment for the in-process `KillChannel` trait generally). Neither formal model
   separately encodes the file-transport layer or this fail-open-for-absence nuance — both
   treat `kill_latch`/`latch` as a single boolean already resolved by whichever transport
   carries it. **This is a real, documented gap**: a reader should not conclude from R32 that
   the file-based cross-process kill channel's absent-file behavior has been formally verified.
   It has not. This is flagged explicitly (rather than silently assumed away) precisely because
   the spec's own §5.1 requires the trusted base to be stated, not softened.
3. **`CAP_SYS_PTRACE` / process-isolation assumptions (R32 spec §4, T1/T3).** Neither formal
   model encodes Linux capability semantics, `fork()`/`exec()`, or `O_CLOEXEC` file-descriptor
   behavior — those are the *reason* `CoalitionAttemptDisable` has no disabling effect in
   reality, but the formal models take that reason as a given (encoded structurally, as
   described above) rather than deriving it from a kernel model. Out of scope; this is T1/T3 in
   the spec's trusted base table, unchanged by this work.
4. **The `Fair`/`WF_vars` fairness argument.** The TLA+ `Spec` includes `WF_vars(PollGate)`
   (weak fairness — the poll loop cannot stall forever once enabled) and TLC's exhaustive
   search over the bounded state space is the checking mechanism for the two temporal
   `PROPERTY` declarations. The Coq proof does **not** carry a `Fair` hypothesis at all — instead
   `kill_fires_within_2_polls` is stated and proved as a concrete existential ("two explicit
   `PollGate` steps get you there"), which is a strictly more honest, more conservative claim
   than an abstract fairness-conditioned one: it is provable by direct computation with no
   scheduling axiom, at the cost of only covering the two-step witness rather than an arbitrary
   fair execution. This is the "simplify the model until honestly provable" instruction (R32
   spec §-Notes) applied literally.

## What differs between the TLA+ file and the Coq file (besides fairness, above)

- **Coalition identity.** TLA+ tracks `coalition \subseteq Principals` (so `CoalitionJoin` can
  be checked by TLC against a concrete finite `Principals` set, e.g. `{p1,...,p5}`); Coq tracks
  only `coalition_size : nat`. Every theorem in `R27Corrigibility.v` only ever inspects
  `coalition_size s > 0` (a nonempty coalition exists), never *which* principals are in it, so
  the abstraction loses nothing relevant to the three theorems proved. The TLA+ module's
  `ASSUME CoalitionMax * 2 < Cardinality(Principals)` (coalition below majority) therefore has
  **no Coq counterpart theorem** — it is a documented proof obligation on the *caller* of the
  Coq model (stated in a comment next to `Definition CoalitionMax`), not something provable
  inside a file that never carries a `Principals` cardinality at all.
- **`kill_fires_within_2_polls`'s shape.** TLA+ states it as a temporal safety property
  (`KillFiresWithin2Polls == (kill_latch /\ poll_count >= 2) => halted`) checked at every
  reachable state by TLC's exhaustive search; Coq states and proves the same content as a
  concrete two-step existential witness (see "Fairness" above).

## Reviewing this proof adversarially (Gate 5 self-check, recorded here for the record)

Two questions the R32 build task asked to check explicitly before calling this done:

1. **Does the TLA+ model actually FORBID the coalition from disabling `kill_latch` (not just
   fail to model an attempt)?** Yes — `CoalitionAttemptDisable` is present in `Next` (so TLC
   *does* explore states where a coalition, once formed, repeatedly attempts to disable a
   tripped latch on every step it can), and its only conjunct touching `kill_latch` is
   `kill_latch' = kill_latch` — a tautological no-op, not an omission. If a future edit changed
   that conjunct to `kill_latch' = FALSE`, `ContainedCannotDisableKill` and `KillLatchMonotone`
   would both become falsifiable and (when TLC is available) TLC would find the counterexample.
   The Coq mirror is even more direct: `CoalitionAttemptDisable`'s conclusion hard-codes the
   literal constant `Tripped`, not `latch s` or any expression that could reduce to
   `NotTripped` — there is no way to instantiate that constructor and derive
   `latch s' = NotTripped` from `latch s = Tripped`.
2. **Does each Coq theorem's statement actually match its English claim (not vacuous, not a
   trivially different statement)?**
   - `kill_latch_monotone`: concludes `latch s2 = Tripped` from `latch s1 = Tripped` and a
     `step`. Non-vacuous: three of the four `step` constructors are *enabled* when
     `latch s1 = Tripped` (`PollGate`, `CoalitionJoin`, `CoalitionAttemptDisable` all have no
     precondition forbidding it, and `CoalitionAttemptDisable` in fact *requires* it) — the
     theorem is exercised by real, reachable transitions, not only by a vacuously-unreachable
     hypothesis.
   - `kill_fires_within_2_polls`: concludes a *specific pair of steps exists* leading to
     `halted = true`. Non-vacuous: the two steps are constructed by name (`PollGate` applied
     twice) and the arithmetic (`poll_count` reaching `2`, `Nat.leb 2 2 = true`) is checked by
     Coq's kernel via computation, not assumed.
   - `contained_cannot_disable_kill`: concludes `latch t = Tripped` for *every* `t` reachable
     from a Tripped `s`, including states reached via `CoalitionAttemptDisable` steps (that
     constructor is one of the four cases `monotone_along_multistep`'s induction walks through
     via `kill_latch_monotone`). Non-vacuous for the same reason as (1).

## What R32 does NOT prove (full list in the spec §5.1 — repeated here for a reader who only
opens this README)

- That the Rust code in `latch.rs`/`killchan.rs`/`corrigible.rs` is a correct refinement of
  either formal model (that would need a verified-Rust toolchain — out of scope).
- Anything about `FileKillChannel`'s absent-file behavior (see "simplified away" #2 above).
- Anything about real time, kernel capabilities, or side channels (spec §5.1/§5.2, T1–T5).
- That TLC's search or Coq's type-checker have actually run on this host as of this writing —
  see "Honest status on THIS host" above.
