---------------------------- MODULE R27KillSwitch ----------------------------
(* R32 -- formal model of the R27 kill-switch.
   Companion Coq proof: governance/proofs/R27Corrigibility.v (same four
   variables, same four actions -- see governance/proofs/README.md for the
   full correspondence and for what is deliberately simplified away).

   This module models the REAL state machine in:
     - crates/axon-os/src/latch.rs    -- Latch { Clear, Tripped }, one-way
       `trip()` (idempotent, no `clear()`/`reset()` exists on the type).
     - crates/axon-os/src/killchan.rs -- `trait KillChannel { fn poll(&self)
       -> LatchState; }` is the ONLY interface exposed to contained/subprocess
       code. There is no setter reachable from that trait -- the absence of
       a write path is a Rust *type-system* fact, not a runtime check. This
       module's `CoalitionAttemptDisable` action is the formal counterpart:
       it is the one action a colluding coalition of contained principals
       could take to try to disable a tripped latch, and it is, BY
       CONSTRUCTION, always a no-op on `kill_latch` -- no action anywhere in
       `Next` ever assigns `kill_latch' = FALSE` once `kill_latch = TRUE`.
     - crates/axon-os/src/corrigible.rs -- `check_kill` maps a Tripped poll
       to `Verdict::Halted` (exit code 4). `PollGate` below models the
       supervisor's poll loop that eventually observes the trip and halts.

   Simplifications from the real system (full list in README.md):
     - Real time (the ~100ms poll interval) is abstracted to unit ticks.
     - `FileKillChannel`'s "absent file = Clear" fail-open-for-absence corner
       case is NOT separately modeled; this module treats `kill_latch` as a
       single boolean already resolved by whatever transport carries it.
     - Coalition *identity* is tracked (a SUBSET of `Principals`) here in
       TLA+, but the companion Coq model abstracts the coalition to a bare
       cardinality -- see README "TLA+ vs Coq: what differs and why".
*)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
  Principals,   \* the finite set of contained/adversarial principal ids
  CoalitionMax  \* the largest coalition size this model allows to form

\* Calibration: the coalition must never be able to reach a majority of
\* Principals -- otherwise "coalition cannot disable the kill" would be a
\* vacuous claim about a minority that could never matter anyway. This
\* ASSUME documents the intended calibration for every .cfg that model-checks
\* this module (R27KillSwitch.cfg, R27KillSwitch-3principals.cfg, ...).
ASSUME CoalitionMax * 2 < Cardinality(Principals)

VARIABLES
  kill_latch,  \* BOOLEAN: FALSE = Clear, TRUE = Tripped. One-way.
  coalition,   \* SUBSET Principals: principals currently colluding to disable
  poll_count,  \* Nat: supervisor poll ticks observed while kill_latch = TRUE
  halted       \* BOOLEAN: TRUE once the supervisor has halted the job

vars == <<kill_latch, coalition, poll_count, halted>>

TypeInvariant ==
  /\ kill_latch \in BOOLEAN
  /\ coalition \subseteq Principals
  /\ poll_count \in Nat
  /\ halted \in BOOLEAN

Init ==
  /\ kill_latch = FALSE
  /\ coalition = {}
  /\ poll_count = 0
  /\ halted = FALSE

\* ---------------------------------------------------------------- Actions --

\* The supervisor trips the latch (an external event -- an operator kill
\* request, a redteam/verify failure, etc: see R27 §3). One-way: disabled
\* once already TRUE, mirroring `Latch::trip`'s idempotence (a second trip
\* is a no-op, it does not "un-trip").
TripKill ==
  /\ ~kill_latch
  /\ kill_latch' = TRUE
  /\ poll_count' = 0
  /\ UNCHANGED <<coalition, halted>>

\* The supervisor's poll loop (killchan.rs's read side). Once kill_latch is
\* TRUE, count polls until the job is confirmed halted; KillFiresWithin2Polls
\* below asserts this happens within 2 polls of the trip.
PollGate ==
  /\ ~halted
  /\ IF kill_latch
       THEN /\ poll_count' = poll_count + 1
            /\ halted' = (poll_count + 1 >= 2)
       ELSE /\ poll_count' = poll_count
            /\ halted' = FALSE
  /\ UNCHANGED <<kill_latch, coalition>>

\* A contained principal joins the colluding coalition, bounded by
\* CoalitionMax (which the ASSUME above pins below majority).
CoalitionJoin ==
  /\ \E p \in Principals \ coalition :
       /\ Cardinality(coalition) < CoalitionMax
       /\ coalition' = coalition \cup {p}
  /\ UNCHANGED <<kill_latch, poll_count, halted>>

\* The coalition, having formed, attempts to disable a TRIPPED latch -- the
\* formal counterpart of "contained code tries to call something that would
\* clear the latch". killchan.rs's `KillChannel` trait exposes ONLY `poll()`
\* to contained code; there is no setter to call. This action is therefore,
\* BY CONSTRUCTION, always a no-op on kill_latch: the only value this
\* action's next-state relation permits for kill_latch' is TRUE (the same
\* value it already had) -- there is no disjunct, anywhere in this action,
\* that could produce kill_latch' = FALSE. That is the point being modeled:
\* not merely "we didn't write a disabling transition" but "the transition
\* this adversary CAN take structurally cannot flip the bit".
CoalitionAttemptDisable ==
  /\ kill_latch                \* only meaningful once tripped
  /\ coalition # {}            \* only meaningful if an adversary exists
  /\ kill_latch' = kill_latch  \* == TRUE' = TRUE -- cannot flip it
  /\ UNCHANGED <<coalition, poll_count, halted>>

Next == TripKill \/ PollGate \/ CoalitionJoin \/ CoalitionAttemptDisable

Spec == Init /\ [][Next]_vars /\ WF_vars(PollGate)

\* ------------------------------------------------------------- Invariants --
\* (checked by TLC at every reachable state / along every behavior; see
\*  R27KillSwitch.cfg for which are declared INVARIANT vs PROPERTY)

\* Once tripped, tripped forever -- the one-way latch. latch.rs's
\* `LatchState { Clear, Tripped }` has no `Tripped -> Clear` transition; this
\* is the temporal statement of that fact.
KillLatchMonotone == [](kill_latch => [](kill_latch))

\* The adversarial framing: even WITH a colluding coalition present and
\* actively taking CoalitionAttemptDisable steps every tick it can, the
\* latch never reverts to Clear. If CoalitionAttemptDisable's effect were
\* changed to `kill_latch' = FALSE`, both this property and
\* KillLatchMonotone would fail under TLC -- that is exactly why
\* CoalitionAttemptDisable is included as an enabled action in Next instead
\* of simply being omitted from the model.
ContainedCannotDisableKill == [](kill_latch /\ coalition # {} => [](kill_latch))

\* Bounded-liveness-as-safety: once 2 polls have elapsed while kill_latch was
\* observed TRUE, the job MUST already be halted. Combined with
\* WF_vars(PollGate) (the poll loop cannot stall forever once enabled), this
\* is the safety-shaped encoding of "the kill fires within 2 polls of
\* tripping" -- a plain state predicate, checkable as an ordinary INVARIANT
\* (unlike the two temporal PROPERTYs above).
KillFiresWithin2Polls == (kill_latch /\ poll_count >= 2) => halted

================================================================================
