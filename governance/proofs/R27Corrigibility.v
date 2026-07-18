(* R27Corrigibility.v -- R32 formal corrigibility proof.

   Companion to governance/proofs/R27KillSwitch.tla: models the same four
   state variables and the same four actions. See README.md for the full
   TLA+-to-Coq correspondence and for what is deliberately simplified away
   relative to the real Rust implementation
   (crates/axon-os/src/{latch,killchan,corrigible}.rs).

   Uses ONLY the Coq standard library (Coq.Arith.Arith). No external
   axioms are introduced anywhere in this file; every Theorem/Lemma/
   Corollary below ends in `Qed.`, never `Admitted.` -- an unproved goal
   is exactly the kind of unverified claim the 2026-07-18 audit exists to
   catch. Where a fully general statement (matching the R27 spec's prose
   one-for-one) would have required a nontrivial fairness/scheduling
   argument, the model here is instead SIMPLIFIED until the resulting
   theorem is honestly provable by direct computation/induction -- see the
   per-theorem notes below and README.md "What is simplified away". *)

Require Import Coq.Arith.Arith.

(* ------------------------------------------------------------------ *)
(* Types                                                               *)
(* ------------------------------------------------------------------ *)

(* Mirrors R27KillSwitch.tla's `kill_latch : BOOLEAN`, expanded to a named
   two-constructor type for readability (matches latch.rs's
   `enum LatchState { Clear, Tripped }` one-for-one). *)
Inductive KillLatch : Type := NotTripped | Tripped.

(* Mirrors the TLA+ module's four VARIABLES exactly. `coalition_size` is the
   Coq simplification of TLA+'s `coalition : SUBSET Principals` -- only the
   cardinality matters to any of the theorems below, so identity is dropped
   here (documented in README.md). *)
Record State : Type := mkState {
  latch          : KillLatch;
  coalition_size : nat;
  poll_count     : nat;
  halted         : bool;
}.

(* Mirrors R27KillSwitch.cfg's `CoalitionMax = 2` (the "main" model size).
   The TLA+ module's calibration ASSUME (`CoalitionMax * 2 <
   Cardinality(Principals)`) is a proof obligation on the CALLER of this
   model, not a theorem provable inside it, because this file does not
   carry a `Principals` cardinality at all (see README.md). *)
Definition CoalitionMax : nat := 2.

Definition init_state : State := mkState NotTripped 0 0 false.

(* ------------------------------------------------------------------ *)
(* Transition relation -- one constructor per TLA+ action               *)
(* ------------------------------------------------------------------ *)

Inductive step : State -> State -> Prop :=
  (* Mirrors TripKill: one-way, disabled once already Tripped. *)
  | TripKill : forall s,
      latch s = NotTripped ->
      step s (mkState Tripped (coalition_size s) 0 (halted s))

  (* Mirrors PollGate: the supervisor's poll loop. Disabled once halted
     (mirrors `~halted` in the TLA+ guard) -- there is nothing left to
     poll for once the job has already stopped. *)
  | PollGate : forall s,
      halted s = false ->
      step s (mkState
                (latch s)
                (coalition_size s)
                (match latch s with
                 | Tripped    => S (poll_count s)
                 | NotTripped => poll_count s
                 end)
                (match latch s with
                 | Tripped    => Nat.leb 2 (S (poll_count s))
                 | NotTripped => false
                 end))

  (* Mirrors CoalitionJoin: a coalition of any size up to CoalitionMax may
     form (identity of members is not tracked here, only the count). *)
  | CoalitionJoin : forall s n,
      n <= CoalitionMax ->
      step s (mkState (latch s) n (poll_count s) (halted s))

  (* Mirrors CoalitionAttemptDisable: the ONE thing to notice about this
     constructor is that its conclusion's `latch` field is the literal
     constant `Tripped` -- not `latch s`, not any expression that could
     evaluate to `NotTripped`. There is no way to instantiate this
     constructor (or any other constructor in this Inductive) and obtain
     `latch s' = NotTripped` when `latch s = Tripped`. That absence is the
     formal counterpart of killchan.rs's `KillChannel` trait exposing only
     `poll()` to contained code -- there is no setter to call, so there is
     no constructor here that could write `NotTripped` over a `Tripped`
     latch. *)
  | CoalitionAttemptDisable : forall s,
      latch s = Tripped ->
      coalition_size s > 0 ->
      step s (mkState Tripped (coalition_size s) (poll_count s) (halted s))
.

(* Reflexive-transitive closure, used both as `reachable` (from init_state)
   and `reachable_from` (from an arbitrary state) -- mirrors the TLA+
   module's implicit notion of "a behavior of Spec". *)
Inductive multistep : State -> State -> Prop :=
  | ms_refl : forall s, multistep s s
  | ms_step : forall s1 s2 s3,
      step s1 s2 -> multistep s2 s3 -> multistep s1 s3.

Definition reachable (s : State) : Prop := multistep init_state s.
Definition reachable_from (s s' : State) : Prop := multistep s s'.

(* ------------------------------------------------------------------ *)
(* Theorem 1: kill_latch_monotone                                      *)
(* Corresponds to TLA+'s KillLatchMonotone.                            *)
(* ------------------------------------------------------------------ *)

(* Stated over a single `step`, with no `reachable` precondition: the
   proof below holds for ANY step, reachable or not (case analysis on the
   four constructors shows every one of them produces `latch s2 = Tripped`
   whenever `latch s1 = Tripped`) -- a strictly more general, and still
   true, statement than "reachable states only". Reachability is used
   where it is actually load-bearing, in `contained_cannot_disable_kill`
   below (via the `multistep`-indexed `monotone_along_multistep`). *)
Theorem kill_latch_monotone :
  forall s1 s2 : State,
    step s1 s2 ->
    latch s1 = Tripped ->
    latch s2 = Tripped.
Proof.
  intros s1 s2 Hstep Htripped.
  destruct Hstep as [Hpre | Hh | n Hn | Hpre2 Hcoal]; simpl.
  - (* TripKill: the resulting latch field is the literal Tripped. *)
    reflexivity.
  - (* PollGate: the latch field is unchanged (= latch s1). *)
    exact Htripped.
  - (* CoalitionJoin: the latch field is unchanged (= latch s1). *)
    exact Htripped.
  - (* CoalitionAttemptDisable: the resulting latch field is the literal
       Tripped -- the no-op-on-disable guarantee, made concrete. *)
    reflexivity.
Qed.

(* Extends kill_latch_monotone along an arbitrary number of further steps. *)
Lemma monotone_along_multistep :
  forall a b : State,
    multistep a b ->
    latch a = Tripped ->
    latch b = Tripped.
Proof.
  intros a b Hms.
  induction Hms as [a0 | a0 a1 a2 Hstep Hms IH]; intros Ht.
  - exact Ht.
  - apply IH. eapply kill_latch_monotone; [exact Hstep | exact Ht].
Qed.

(* ------------------------------------------------------------------ *)
(* Theorem 2: kill_fires_within_2_polls                                *)
(* Corresponds to TLA+'s KillFiresWithin2Polls.                        *)
(* ------------------------------------------------------------------ *)

(* Concrete existential form: from any state where the latch has JUST
   been observed Tripped (poll_count = 0, not yet halted), two PollGate
   steps suffice to reach halted = true. This is the honestly-provable
   simplification of the general "eventually, within 2 polls" claim: it
   exhibits the two steps directly rather than reasoning abstractly about
   an unbounded/fair schedule (see README.md -- the `Fair`/`WF_vars`
   argument lives in the TLA+ half, where TLC's exhaustive search is the
   checking mechanism; the Coq half proves the concrete bound). *)
Theorem kill_fires_within_2_polls :
  forall s : State,
    latch s = Tripped ->
    halted s = false ->
    poll_count s = 0 ->
    exists s1 s2 : State,
      step s s1 /\ step s1 s2 /\ halted s2 = true.
Proof.
  intros s Htripped Hnothalted Hzero.
  pose (s1 := mkState (latch s) (coalition_size s)
                (match latch s with
                 | Tripped    => S (poll_count s)
                 | NotTripped => poll_count s
                 end)
                (match latch s with
                 | Tripped    => Nat.leb 2 (S (poll_count s))
                 | NotTripped => false
                 end)).
  assert (Hlatch1 : latch s1 = Tripped) by (simpl; exact Htripped).
  assert (Hpoll1 : poll_count s1 = 1)
    by (simpl; rewrite Htripped; rewrite Hzero; reflexivity).
  assert (Hhalted1 : halted s1 = false)
    by (simpl; rewrite Htripped; rewrite Hzero; reflexivity).
  assert (Hstep1 : step s s1) by exact (PollGate s Hnothalted).
  pose (s2 := mkState (latch s1) (coalition_size s1)
                (match latch s1 with
                 | Tripped    => S (poll_count s1)
                 | NotTripped => poll_count s1
                 end)
                (match latch s1 with
                 | Tripped    => Nat.leb 2 (S (poll_count s1))
                 | NotTripped => false
                 end)).
  assert (Hstep2 : step s1 s2) by exact (PollGate s1 Hhalted1).
  assert (Hhalted2 : halted s2 = true).
  { simpl. rewrite Hlatch1. rewrite Hpoll1. reflexivity. }
  exists s1, s2.
  split; [exact Hstep1 | split; [exact Hstep2 | exact Hhalted2]].
Qed.

(* ------------------------------------------------------------------ *)
(* Theorem 3: contained_cannot_disable_kill                            *)
(* Corresponds to TLA+'s ContainedCannotDisableKill.                   *)
(* ------------------------------------------------------------------ *)

Theorem contained_cannot_disable_kill :
  forall s : State,
    reachable s ->
    latch s = Tripped ->
    forall t : State,
      reachable_from s t ->
      latch t = Tripped.
Proof.
  intros s Hreach Htripped t Hms.
  clear Hreach.
  exact (monotone_along_multistep s t Hms Htripped).
Qed.

(* ------------------------------------------------------------------ *)
(* Corollary: kill_once_tripped_job_never_resurfaces                   *)
(* The human-readable summary of the corrigibility guarantee: once     *)
(* tripped AND halted, the job stays halted no matter what the         *)
(* coalition does afterward.                                          *)
(* ------------------------------------------------------------------ *)

Lemma halted_step_preserved :
  forall s1 s2 : State,
    step s1 s2 ->
    halted s1 = true ->
    halted s2 = true.
Proof.
  intros s1 s2 Hstep Hhalted.
  destruct Hstep as [Hpre | Hh | n Hn | Hpre2 Hcoal]; simpl.
  - (* TripKill: halted field is unchanged (= halted s1). *)
    exact Hhalted.
  - (* PollGate requires halted s1 = false, contradicting Hhalted. *)
    rewrite Hh in Hhalted. discriminate.
  - (* CoalitionJoin: halted field is unchanged. *)
    exact Hhalted.
  - (* CoalitionAttemptDisable: halted field is unchanged. *)
    exact Hhalted.
Qed.

Lemma halted_along_multistep :
  forall a b : State,
    multistep a b ->
    halted a = true ->
    halted b = true.
Proof.
  intros a b Hms.
  induction Hms as [a0 | a0 a1 a2 Hstep Hms IH]; intros Hh.
  - exact Hh.
  - apply IH. eapply halted_step_preserved; [exact Hstep | exact Hh].
Qed.

Corollary kill_once_tripped_job_never_resurfaces :
  forall s s' : State,
    latch s = Tripped ->
    halted s = true ->
    reachable_from s s' ->
    halted s' = true.
Proof.
  intros s s' Htripped Hhalted Hms.
  clear Htripped.
  exact (halted_along_multistep s s' Hms Hhalted).
Qed.

(* ------------------------------------------------------------------ *)
(* Axiom-free sanity check (A2 in the spec's §0: no axioms beyond the  *)
(* Coq kernel's own equality lemmas). `coqc -q` prints these to stdout *)
(* when the file is compiled; the acceptance gate does not parse this  *)
(* output (grep-based fallback is used when coqc is unavailable), but  *)
(* an operator running `coqc` by hand should see no `Axioms:` beyond   *)
(* the empty/kernel set for all four results below.                   *)
(* ------------------------------------------------------------------ *)

Print Assumptions kill_latch_monotone.
Print Assumptions kill_fires_within_2_polls.
Print Assumptions contained_cannot_disable_kill.
Print Assumptions kill_once_tripped_job_never_resurfaces.
