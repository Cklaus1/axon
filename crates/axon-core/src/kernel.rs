//! Phase 7 (R12) kernel runtime services — Slice 1: `principal_authority`.
//!
//! The userland oracle (`examples/stdlib/principal_mint.ax`) proves the
//! *semantics* of capability attenuation as pure values. This module is the
//! *kernel* counterpart: a runtime REGISTRY of live `Principal`s the interpreter
//! tracks for a program, with the R11 attenuation enforced in the kernel mint
//! path — `mint` returns a handle into the registry, and the registry keeps the
//! live tree (id → principal, with parent links) for audit lineage.
//!
//! I-2: the observable semantics here are byte-identical to the oracle —
//!   • child cap_X = want_X ∧ parent.X          (escalation unrepresentable)
//!   • grant = clamp(budget_grant, 0, parent_remaining)  (no over-grant)
//!   • parent.budget.used += grant              (carved, not conjured)
//! A kernel-vs-userland parity test pins this (R12 §7).
//!
//! Q3 (R12 §9): this lives in its own module (not `interp.rs`) so the codegen
//! build is untouched; the interpreter owns a `RefCell<PrincipalRegistry>` and
//! the `principal_*` builtins drive it. Handles are plain `i64` so they flow
//! through the existing value/type machinery with no new `Value` variant.

/// A budget: spent-so-far against a cap. Mirrors `budget.ax` / the oracle's
/// inlined `Budget`. `remaining` clamps at 0 (never negative).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub used: i64,
    pub cap: i64,
}

impl Budget {
    fn new(cap: i64) -> Self {
        Budget { used: 0, cap }
    }
    fn spend(self, amount: i64) -> Self {
        Budget {
            used: self.used + amount,
            cap: self.cap,
        }
    }
    fn remaining(self) -> i64 {
        let r = self.cap - self.used;
        if r < 0 {
            0
        } else {
            r
        }
    }
    fn exhausted(self) -> bool {
        self.used >= self.cap
    }
}

/// A live principal in the kernel registry: capabilities + budget + lineage.
/// `parent` is the handle of the minting principal, or `None` for a root.
#[derive(Debug, Clone)]
pub struct Principal {
    pub name: String,
    pub parent: Option<usize>,
    pub net: bool,
    pub fs_write: bool,
    pub exec: bool,
    pub budget: Budget,
}

impl Principal {
    /// Does this principal hold the named capability? Unknown names → false
    /// (deny by default), matching the oracle's `holds`.
    pub fn holds(&self, cap: &str) -> bool {
        match cap {
            "net" => self.net,
            "fs_write" => self.fs_write,
            "exec" => self.exec,
            _ => false,
        }
    }
}

/// The kernel registry of live principals. Append-only over a run: a handle is an
/// index that never moves, so lineage links stay valid. Mutating a principal
/// (budget debit on mint/spend) updates it in place.
#[derive(Debug, Default)]
pub struct PrincipalRegistry {
    principals: Vec<Principal>,
}

impl PrincipalRegistry {
    pub fn new() -> Self {
        PrincipalRegistry {
            principals: Vec::new(),
        }
    }

    /// Register a ROOT principal — the originating authority. Holds exactly the
    /// caps given and the full budget; everything below can only attenuate.
    /// Returns its handle.
    pub fn root(
        &mut self,
        name: String,
        net: bool,
        fs_write: bool,
        exec: bool,
        budget_cap: i64,
    ) -> usize {
        let p = Principal {
            name,
            parent: None,
            net,
            fs_write,
            exec,
            budget: Budget::new(budget_cap.max(0)),
        };
        self.principals.push(p);
        self.principals.len() - 1
    }

    /// MINT an attenuated child of `parent_handle`. ATTENUATION BY CONSTRUCTION
    /// (byte-identical to the oracle's `mint`):
    ///   • child cap_X = want_X ∧ parent.X      (escalation unrepresentable)
    ///   • grant = clamp(budget_grant, 0, parent_remaining)  (no over-grant)
    ///   • parent.budget.used += grant          (carved from the parent)
    /// Returns the child's handle, or `None` if `parent_handle` is unknown (a
    /// defense-in-depth guard — the caller surfaces E1601).
    pub fn mint(
        &mut self,
        parent_handle: usize,
        child_name: String,
        want_net: bool,
        want_fs_write: bool,
        want_exec: bool,
        budget_grant: i64,
    ) -> Option<usize> {
        let parent = self.principals.get(parent_handle)?.clone();
        let c_net = want_net && parent.net;
        let c_fs = want_fs_write && parent.fs_write;
        let c_exec = want_exec && parent.exec;
        // clamp(budget_grant, 0, parent_remaining)
        let grant = budget_grant.max(0).min(parent.budget.remaining());
        // Carve the grant from the parent (debit in place).
        self.principals[parent_handle].budget = parent.budget.spend(grant);
        let child = Principal {
            name: child_name,
            parent: Some(parent_handle),
            net: c_net,
            fs_write: c_fs,
            exec: c_exec,
            budget: Budget::new(grant),
        };
        self.principals.push(child);
        Some(self.principals.len() - 1)
    }

    /// Read a principal by handle (None if unknown).
    pub fn get(&self, handle: usize) -> Option<&Principal> {
        self.principals.get(handle)
    }

    /// Remaining budget of a principal (0 if unknown).
    pub fn budget_remaining(&self, handle: usize) -> i64 {
        self.principals
            .get(handle)
            .map(|p| p.budget.remaining())
            .unwrap_or(0)
    }

    /// Debit `amount` from a principal's own budget; returns its new remaining
    /// (or 0 if unknown). Caps are untouched — only the carved budget is consumed.
    pub fn spend(&mut self, handle: usize, amount: i64) -> i64 {
        if let Some(p) = self.principals.get_mut(handle) {
            p.budget = p.budget.spend(amount.max(0));
            p.budget.remaining()
        } else {
            0
        }
    }

    /// Authorization gate: an action needing these caps is authorized iff the
    /// principal holds every one AND is not budget-exhausted. Mirrors the
    /// oracle's `authorize`. Unknown handle → false.
    pub fn authorize(
        &self,
        handle: usize,
        needs_net: bool,
        needs_fs_write: bool,
        needs_exec: bool,
    ) -> bool {
        let Some(p) = self.principals.get(handle) else {
            return false;
        };
        let caps =
            (!needs_net || p.net) && (!needs_fs_write || p.fs_write) && (!needs_exec || p.exec);
        caps && !p.budget.exhausted()
    }

    /// Whether `parent_handle` could mint a child wanting these caps + budget at
    /// all (holds every requested cap, has budget to carve). The explicit gate;
    /// `mint` is total and safe without it. Mirrors the oracle's `can_mint`.
    pub fn can_mint(
        &self,
        parent_handle: usize,
        want_net: bool,
        want_fs_write: bool,
        want_exec: bool,
        budget_grant: i64,
    ) -> bool {
        let Some(p) = self.principals.get(parent_handle) else {
            return false;
        };
        let caps_ok =
            (!want_net || p.net) && (!want_fs_write || p.fs_write) && (!want_exec || p.exec);
        let budget_ok = budget_grant > 0 && p.budget.remaining() > 0;
        caps_ok && budget_ok
    }
}

// ── Phase 7 (R12) Slice 2: cooperative scheduler (fiber run-queue) ───────────
//
// A run-queue of fibers the interpreter runs cooperatively. Per R12 §3 the model
// is COOPERATIVE over the interpreter's existing eager-spawn machinery — NOT a
// preemptive OS-thread scheduler (which would be a second execution model and an
// I-2 drift risk). Each fiber is a (named user fn, i64 arg); the scheduler runs
// ready fibers in a deterministic round-robin order (a function of spawn order +
// AXON_SEED), collecting each result. A fiber that PANICS is caught and recorded
// as failed — it does not abort the process — which is what makes the Slice-3
// supervisor able to observe and restart it.
//
// The fiber BODIES are run by the interpreter (it owns `call_fn`); this module
// owns the queue + the scheduling order + result/failure bookkeeping, so the
// codegen build stays untouched (Q3) and the scheduling policy is unit-testable
// without the interpreter.

/// The lifecycle state of one fiber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FiberState {
    /// Queued, not yet run.
    Ready,
    /// Ran to completion with this i64 result.
    Done(i64),
    /// Panicked/halted during its run; carries the failure message. Observable
    /// (Slice 3 restarts on it), not a process abort.
    Failed(String),
    /// R15 Slice 3: PARKED at a `host_await` suspension, waiting for the host to
    /// resume it. Carries the opaque suspension token (the host-owned handle the
    /// resume is keyed by, §12 Q2 — an unforgeable `u64`). A Suspended fiber is
    /// NOT Ready (the scheduler must not re-run it) and NOT terminal (it isn't
    /// counted in the done/failed tally) — it is waiting off-queue until
    /// `resume(id)` moves it back to Ready. This is the scheduler-level model of
    /// the suspend the interpreter's worker-thread substrate realizes; it lets a
    /// supervisor SEE that a fiber is parked (vs done/failed/runnable) rather than
    /// the suspension being invisible to the scheduler.
    Suspended(u64),
}

/// One fiber: a named user function to call with an i64 argument, plus its state.
#[derive(Debug, Clone)]
pub struct Fiber {
    pub fn_name: String,
    pub arg: i64,
    pub state: FiberState,
}

/// The cooperative fiber scheduler. Holds the run-queue; the interpreter drives
/// `next_ready()` / records outcomes via `complete`/`fail`. Deterministic: fibers
/// run in spawn order rotated by the seed (round-robin start offset), so a given
/// AXON_SEED always yields the same interleaving.
#[derive(Debug, Default)]
pub struct Scheduler {
    fibers: Vec<Fiber>,
    /// Round-robin start offset, derived from AXON_SEED — makes the schedule
    /// order seed-reproducible without being trivially spawn-order.
    seed_offset: usize,
    /// How many full run passes have executed (for observability/tests).
    pub passes: u64,
}

impl Scheduler {
    pub fn new(seed_offset: usize) -> Self {
        Scheduler {
            fibers: Vec::new(),
            seed_offset,
            passes: 0,
        }
    }

    /// Spawn a fiber (named fn + arg); returns its fiber id (a stable index).
    pub fn spawn(&mut self, fn_name: String, arg: i64) -> usize {
        self.fibers.push(Fiber {
            fn_name,
            arg,
            state: FiberState::Ready,
        });
        self.fibers.len() - 1
    }

    /// The deterministic order in which to run the currently-Ready fibers: spawn
    /// order rotated by `seed_offset` (round-robin), so the schedule is a pure
    /// function of spawn order + AXON_SEED. Returns fiber ids.
    pub fn ready_order(&self) -> Vec<usize> {
        let ready: Vec<usize> = self
            .fibers
            .iter()
            .enumerate()
            .filter(|(_, f)| f.state == FiberState::Ready)
            .map(|(i, _)| i)
            .collect();
        if ready.is_empty() {
            return ready;
        }
        let n = ready.len();
        let off = self.seed_offset % n;
        let mut out = Vec::with_capacity(n);
        for k in 0..n {
            out.push(ready[(off + k) % n]);
        }
        out
    }

    /// A fiber's fn name + arg (for the interpreter to run it). None if id unknown.
    pub fn fiber_call(&self, id: usize) -> Option<(String, i64)> {
        self.fibers.get(id).map(|f| (f.fn_name.clone(), f.arg))
    }

    /// Record a fiber's successful completion with its result.
    pub fn complete(&mut self, id: usize, result: i64) {
        if let Some(f) = self.fibers.get_mut(id) {
            f.state = FiberState::Done(result);
        }
    }

    /// Record a fiber's failure (panic/halt during its run) — observable, not fatal.
    pub fn fail(&mut self, id: usize, msg: String) {
        if let Some(f) = self.fibers.get_mut(id) {
            f.state = FiberState::Failed(msg);
        }
    }

    /// Reset a fiber to Ready (Slice 3 supervisor restart). No-op if id unknown.
    pub fn restart(&mut self, id: usize) {
        if let Some(f) = self.fibers.get_mut(id) {
            f.state = FiberState::Ready;
        }
    }

    /// R15 Slice 3: PARK a fiber at a `host_await` suspension under the opaque
    /// `token` the host will resume it by. It leaves the Ready set (so the
    /// scheduler won't re-run it) but is not terminal. No-op if id unknown.
    pub fn suspend(&mut self, id: usize, token: u64) {
        if let Some(f) = self.fibers.get_mut(id) {
            f.state = FiberState::Suspended(token);
        }
    }

    /// R15 Slice 3: UNPARK a suspended fiber — move it back to Ready so the next
    /// scheduling pass re-runs it (the resume value is fed by the interpreter's
    /// substrate, not modeled here). Returns `true` iff the fiber was actually
    /// Suspended (a resume of a non-suspended / unknown id is a no-op false — the
    /// host-side "unknown/already-finished token" error of §6).
    pub fn resume(&mut self, id: usize) -> bool {
        match self.fibers.get_mut(id) {
            Some(f) if matches!(f.state, FiberState::Suspended(_)) => {
                f.state = FiberState::Ready;
                true
            }
            _ => false,
        }
    }

    /// The suspension token a fiber is parked under, if it is Suspended.
    pub fn suspended_token(&self, id: usize) -> Option<u64> {
        match self.fibers.get(id).map(|f| &f.state) {
            Some(FiberState::Suspended(t)) => Some(*t),
            _ => None,
        }
    }

    pub fn state(&self, id: usize) -> Option<&FiberState> {
        self.fibers.get(id).map(|f| &f.state)
    }

    /// A completed fiber's result (0 if not Done / unknown).
    pub fn result(&self, id: usize) -> i64 {
        match self.fibers.get(id).map(|f| &f.state) {
            Some(FiberState::Done(v)) => *v,
            _ => 0,
        }
    }

    /// Whether a fiber failed.
    pub fn failed(&self, id: usize) -> bool {
        matches!(
            self.fibers.get(id).map(|f| &f.state),
            Some(FiberState::Failed(_))
        )
    }

    /// Count of fibers in each terminal state (done, failed) — for the run summary.
    pub fn tally(&self) -> (usize, usize) {
        let mut done = 0;
        let mut failed = 0;
        for f in &self.fibers {
            match f.state {
                FiberState::Done(_) => done += 1,
                FiberState::Failed(_) => failed += 1,
                // Ready and Suspended are non-terminal — neither done nor failed.
                FiberState::Ready | FiberState::Suspended(_) => {}
            }
        }
        (done, failed)
    }

    pub fn len(&self) -> usize {
        self.fibers.len()
    }
    pub fn is_empty(&self) -> bool {
        self.fibers.is_empty()
    }
}

// ── Phase 7 (R12) Slice 3: live supervisor_root ──────────────────────────────
//
// Wires `supervisor_tree.ax`'s pure OTP restart logic to the Slice-2 scheduler:
// a supervised fiber that fails is ACTUALLY restarted per its strategy
// (one_for_one/all/rest_for_one), with the max-restart-intensity latch tripping
// a real halt on a crash loop. The pure `restart_set` core is byte-identical to
// the oracle (I-2); the live half is the `Supervisor` struct that tracks the
// ordered children + restart count + halt latch, applied against the Scheduler.

/// OTP restart strategy tags (match the oracle's 0/1/2 encoding).
pub const ONE_FOR_ONE: i64 = 0;
pub const ONE_FOR_ALL: i64 = 1;
pub const REST_FOR_ONE: i64 = 2;

/// The pure OTP core: which child INDICES (into the supervised set, 0..n) restart
/// when `failed` crashes, given the strategy. Byte-identical to the oracle's
/// `restart_set`. An out-of-range `failed` restarts nothing.
pub fn restart_set(strategy: i64, failed: i64, n_children: i64) -> Vec<i64> {
    if failed < 0 || failed >= n_children {
        Vec::new()
    } else if strategy == ONE_FOR_ALL {
        (0..n_children).collect()
    } else if strategy == REST_FOR_ONE {
        (failed..n_children).collect()
    } else {
        vec![failed]
    }
}

/// A live supervisor: an OTP strategy + a max-restart-intensity latch, overseeing
/// an ORDERED set of supervised fibers (by their scheduler fiber id). When a
/// child fails, `restart_set` decides which siblings re-queue; exceeding
/// `max_restarts` latches `halted` (the crash-loop backoff → exit 4).
#[derive(Debug, Clone)]
pub struct Supervisor {
    pub strategy: i64,
    pub restarts: i64,
    pub max_restarts: i64,
    pub halted: bool,
    /// Scheduler fiber ids of the supervised children, in start order. The index
    /// into THIS vec is the "child index" `restart_set` reasons about.
    pub children: Vec<usize>,
}

impl Supervisor {
    pub fn new(strategy: i64, max_restarts: i64) -> Self {
        Supervisor {
            strategy,
            restarts: 0,
            max_restarts,
            halted: false,
            children: Vec::new(),
        }
    }

    /// Register a supervised child fiber (by scheduler id), in start order.
    /// Returns its child index.
    pub fn supervise(&mut self, fiber_id: usize) -> usize {
        self.children.push(fiber_id);
        self.children.len() - 1
    }

    pub fn alive(&self) -> bool {
        !self.halted
    }

    /// Observe one child failure (the child at `child_idx` in `children`). Mirrors
    /// the oracle's `on_failure`: a halted supervisor restarts nothing; otherwise
    /// the restart count increments (one failure = one restart EVENT regardless of
    /// how many siblings it takes down), and if that exceeds `max_restarts` the
    /// supervisor latches halted and restarts nothing this round. Returns the
    /// scheduler fiber ids to restart (already mapped from child indices).
    pub fn on_failure(&mut self, child_idx: i64) -> Vec<usize> {
        if self.halted {
            return Vec::new();
        }
        self.restarts += 1;
        if self.restarts > self.max_restarts {
            self.halted = true;
            return Vec::new();
        }
        let n = self.children.len() as i64;
        restart_set(self.strategy, child_idx, n)
            .into_iter()
            .filter_map(|ci| self.children.get(ci as usize).copied())
            .collect()
    }
}

// ── Phase 7 (R12) Slice 4: durable Store<T,C> ────────────────────────────────
//
// Promotes store.ax's consistency variants to a runtime store that PERSISTS
// across a process (the Lifetime axis the oracle left orthogonal). Persistence
// is an NDJSON append log of `(op_id, delta)` rows per store key — replayable and
// deterministic (R12 §4 Q2: reuse the provenance NDJSON machinery). On open, the
// log is replayed to rebuild state; under `linearizable` a retried op_id is
// deduped EVEN ACROSS the process boundary (the `seen` set is reconstructed from
// the log), under `at_least_once` every logged op re-applies. The value-logic is
// byte-identical to the oracle's `apply` (I-2); only the durability is new.

/// Consistency tags (match the oracle's 0/1 encoding).
pub const AT_LEAST_ONCE: i64 = 0;
pub const LINEARIZABLE: i64 = 1;

/// In-memory state of one store, rebuilt by replaying its log. Pure value-logic
/// (the oracle's `apply`), with file I/O kept in the interpreter.
#[derive(Debug, Clone)]
pub struct Store {
    pub value: i64,
    pub consistency: i64,
    pub seen: Vec<i64>,
    pub version: i64,
}

impl Store {
    pub fn new(consistency: i64) -> Self {
        Store {
            value: 0,
            consistency,
            seen: Vec::new(),
            version: 0,
        }
    }

    pub fn has_applied(&self, id: i64) -> bool {
        self.seen.contains(&id)
    }

    /// Apply op `op_id` with effect `delta` — the consistency-defining op,
    /// byte-identical to the oracle's `apply`. Returns whether the op was
    /// actually applied (false = deduped under linearizable), so the caller knows
    /// whether to APPEND it to the durable log (a deduped retry must NOT be
    /// logged twice, or replay would re-dedup correctly but the log would grow).
    pub fn apply(&mut self, op_id: i64, delta: i64) -> bool {
        if self.consistency == LINEARIZABLE {
            if self.has_applied(op_id) {
                false // dedup: exactly-once (cross-process, since `seen` is replayed)
            } else {
                self.value += delta;
                self.seen.push(op_id);
                self.version += 1;
                true
            }
        } else {
            // at_least_once: apply unconditionally; no dedup, no version.
            self.value += delta;
            true
        }
    }
}

// ── Phase 7 (R12) Slice 5: kernel Goal<M> + LLM<Caps> ────────────────────────
//
// The consumers that assemble the lower slices. The kernel `LlmGateway` mediates
// every AI call with PER-TOKEN cost metering (llm_gateway.ax's semantics), but
// scoped to a Slice-1 PRINCIPAL: the cost is debited from the principal's carved
// budget, so authority (Slice 1) and spend (the cost meter) are one model. On
// overrun the gateway returns its fallback and latches (graceful degrade, not a
// crash) — byte-identical to the oracle (I-2). A kernel `Goal` runs an objective
// under the same principal budget and refuses to exceed it (E1604).

/// A principal-scoped LLM gateway: per-token cost metering against a principal's
/// budget, with a graceful fallback + latch on overrun. `rate_micro` is µ$ per
/// 1000 tokens (integer µ$ keeps the arithmetic exact + tests deterministic).
#[derive(Debug, Clone)]
pub struct LlmGateway {
    pub model: String,
    pub rate_micro: i64,
    /// The principal whose budget bounds this gateway's spend (Slice 1 handle).
    pub principal: usize,
    pub fallback: String,
    pub halted: bool,
    /// µ$ spent through THIS gateway (for observability; the authoritative cap is
    /// the principal's budget).
    pub spent_micro: i64,
}

impl LlmGateway {
    pub fn new(model: String, rate_micro: i64, principal: usize, fallback: String) -> Self {
        LlmGateway {
            model,
            rate_micro,
            principal,
            fallback,
            halted: false,
            spent_micro: 0,
        }
    }

    /// µ$ cost of a call of `tokens` tokens (rate is per 1000 tokens). Negative
    /// token counts clamp to 0.
    pub fn call_cost(&self, tokens: i64) -> i64 {
        let t = tokens.max(0);
        self.rate_micro * t / 1000
    }
}

/// R12b: a principal-scoped, budgeted objective runner. A `KernelGoal` records
/// the `@[adaptive]` metric `name` and `target` to optimize, the owning Slice-1
/// `principal` (whose `Budget` bounds total spend), how many evaluations have
/// been charged so far (`evals_spent`), and the best score observed. The interp
/// runs the EXISTING optimizer (`run_goal`) for `min(requested, budget_remaining)`
/// evaluations, debits the principal's budget, and refuses to exceed it (E1604,
/// exit 7). Authority (Slice 1) and spend are ONE model — a goal can never spend
/// beyond its principal's grant. See governance/specs/R12b-kernel-goal.md.
#[derive(Debug, Clone)]
pub struct KernelGoal {
    pub principal: usize,
    pub name: String,
    pub target: f64,
    pub evals_spent: i64,
    pub best_score: f64,
}

impl KernelGoal {
    pub fn new(principal: usize, name: String, target: f64) -> Self {
        // best_score starts at `target` — the `goal_best_score` convention for a
        // goal with no recorded evaluations yet.
        KernelGoal {
            principal,
            name,
            target,
            evals_spent: 0,
            best_score: target,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_mint_matches_oracle_subset() {
        // Oracle test_mint_subset_works, in the kernel registry.
        let mut reg = PrincipalRegistry::new();
        let r = reg.root("root".into(), true, true, true, 100);
        let c = reg.mint(r, "child".into(), true, false, true, 40).unwrap();
        let child = reg.get(c).unwrap();
        assert!(child.net);
        assert!(!child.fs_write);
        assert!(child.exec);
        assert_eq!(child.budget.cap, 40);
        assert_eq!(child.parent, Some(r));
    }

    #[test]
    fn kernel_mint_cannot_escalate() {
        // Parent lacks net; child requesting net does NOT get it — escalation is
        // unrepresentable (want_net ∧ parent.net == false). Oracle parity.
        let mut reg = PrincipalRegistry::new();
        let r = reg.root("root".into(), false, true, false, 100);
        let c = reg.mint(r, "child".into(), true, true, true, 30).unwrap();
        let child = reg.get(c).unwrap();
        assert!(!child.net, "requested but denied — parent had no net");
        assert!(child.fs_write);
        assert!(!child.exec);
        assert!(!child.holds("net"));
    }

    #[test]
    fn kernel_budget_is_carved_from_parent() {
        // The grant is debited from the parent and capped in the child; the two
        // live budgets sum to ≤ the original (carve, not conjure). Oracle parity.
        let mut reg = PrincipalRegistry::new();
        let r = reg.root("root".into(), true, false, false, 100);
        let c = reg.mint(r, "child".into(), true, false, false, 40).unwrap();
        assert_eq!(reg.budget_remaining(r), 60, "parent debited by exactly 40");
        assert_eq!(reg.get(c).unwrap().budget.cap, 40);
        // child spends its whole grant → exhausted, 0 remaining.
        let rem = reg.spend(c, 40);
        assert_eq!(rem, 0);
        assert!(reg.budget_remaining(c) + reg.budget_remaining(r) <= 100);
    }

    #[test]
    fn kernel_overgrant_is_clamped() {
        // Root with 50 left asked to grant 200 → clamped to 50; parent → 0.
        // Authority cannot be manufactured (E1601 territory, here structurally safe).
        let mut reg = PrincipalRegistry::new();
        let r = reg.root("root".into(), true, true, true, 50);
        let c = reg.mint(r, "greedy".into(), true, true, true, 200).unwrap();
        assert_eq!(reg.get(c).unwrap().budget.cap, 50);
        assert_eq!(reg.budget_remaining(r), 0);
    }

    #[test]
    fn kernel_chain_stays_attenuated() {
        // root(net,fs,exec) → child(net,fs) → grand(net): a cap dropped at a hop
        // can never be regained. Oracle test_chain_stays_attenuated.
        let mut reg = PrincipalRegistry::new();
        let r = reg.root("root".into(), true, true, true, 100);
        let c = reg.mint(r, "child".into(), true, true, false, 60).unwrap();
        let g = reg.mint(c, "grand".into(), true, true, true, 30).unwrap();
        let grand = reg.get(g).unwrap();
        assert!(grand.net, "child had net");
        assert!(grand.fs_write, "child had fs");
        assert!(
            !grand.exec,
            "child never had exec → grandchild can't get it"
        );
        assert_eq!(grand.budget.cap, 30);
    }

    #[test]
    fn kernel_no_cap_root_seals_the_floor() {
        // A sandboxed root holds nothing; every child it mints holds nothing.
        let mut reg = PrincipalRegistry::new();
        let r = reg.root("sandbox".into(), false, false, false, 100);
        let c = reg.mint(r, "child".into(), true, true, true, 50).unwrap();
        let child = reg.get(c).unwrap();
        assert!(!child.net && !child.fs_write && !child.exec);
    }

    #[test]
    fn kernel_authorize_and_can_mint_gates() {
        let mut reg = PrincipalRegistry::new();
        let r = reg.root("root".into(), true, false, false, 100);
        assert!(reg.authorize(r, true, false, false), "has net + budget");
        assert!(
            !reg.authorize(r, false, true, false),
            "needs fs_write, lacks it"
        );
        assert!(reg.can_mint(r, true, false, false, 10));
        assert!(
            !reg.can_mint(r, true, true, false, 10),
            "wants fs_write, lacks it"
        );
        reg.spend(r, 100);
        assert!(
            !reg.authorize(r, true, false, false),
            "budget exhausted → denied"
        );
        assert!(!reg.can_mint(r, true, false, false, 10), "no budget left");
    }

    #[test]
    fn kernel_unknown_handle_is_safe() {
        // Defense-in-depth (E1601): operations on a bogus handle never panic and
        // never grant — mint returns None, predicates return false/0.
        let mut reg = PrincipalRegistry::new();
        assert!(reg.mint(999, "x".into(), true, true, true, 10).is_none());
        assert!(!reg.authorize(999, true, false, false));
        assert_eq!(reg.budget_remaining(999), 0);
        assert!(reg.get(999).is_none());
    }

    // ── Slice 2: cooperative scheduler ───────────────────────────────────────

    #[test]
    fn scheduler_round_robin_is_seed_deterministic() {
        // Same spawn order + same seed offset ⇒ same schedule order (reproducible
        // under AXON_SEED, the gate's determinism requirement).
        let mut s = Scheduler::new(0);
        for i in 0..4 {
            s.spawn("w".into(), i);
        }
        assert_eq!(s.ready_order(), vec![0, 1, 2, 3], "offset 0 = spawn order");

        let mut s2 = Scheduler::new(2);
        for i in 0..4 {
            s2.spawn("w".into(), i);
        }
        // offset 2 rotates the start: [2,3,0,1].
        assert_eq!(
            s2.ready_order(),
            vec![2, 3, 0, 1],
            "offset rotates deterministically"
        );
        // Identical re-run is identical (no Date/random in the order).
        let mut s3 = Scheduler::new(2);
        for i in 0..4 {
            s3.spawn("w".into(), i);
        }
        assert_eq!(s2.ready_order(), s3.ready_order(), "same seed ⇒ same order");
    }

    #[test]
    fn scheduler_fanout_collect_and_failure_is_observable() {
        // N fibers fan out; each completes with a result; one fails. The failure
        // is OBSERVABLE (recorded), not a process abort — the Slice-3 supervisor
        // restarts on it.
        let mut s = Scheduler::new(0);
        let a = s.spawn("w".into(), 1);
        let b = s.spawn("w".into(), 2);
        let c = s.spawn("w".into(), 3);
        // Drive them (the interpreter would call the fns; here we record outcomes).
        for id in s.ready_order() {
            let (_name, arg) = s.fiber_call(id).unwrap();
            if arg == 2 {
                s.fail(id, "boom".into());
            } else {
                s.complete(id, arg * 10);
            }
        }
        assert_eq!(s.result(a), 10);
        assert!(s.failed(b), "the failing fiber is recorded, not fatal");
        assert_eq!(s.result(c), 30);
        assert_eq!(s.tally(), (2, 1), "2 done, 1 failed");
        // Only the failed fiber is Ready again after a restart (Slice-3 hook).
        s.restart(b);
        assert_eq!(
            s.ready_order(),
            vec![b],
            "restart re-queues only the failed fiber"
        );
    }

    #[test]
    fn scheduler_suspend_parks_off_queue_and_resume_requeues() {
        // R15 Slice 3: a fiber parked at a host_await suspension leaves the Ready
        // set (the scheduler must not re-run a parked fiber) but is NOT terminal
        // (not counted done/failed); resume() unparks it back to Ready.
        let mut s = Scheduler::new(0);
        let a = s.spawn("w".into(), 1);
        let b = s.spawn("w".into(), 2);
        // Park `a` under token 7.
        s.suspend(a, 7);
        assert_eq!(s.suspended_token(a), Some(7), "a carries its suspension token");
        assert_eq!(
            s.ready_order(),
            vec![b],
            "a is parked → only b is Ready (the scheduler won't re-run a)"
        );
        assert_eq!(
            s.tally(),
            (0, 0),
            "a Suspended fiber is non-terminal — not done, not failed"
        );
        // Resume `a` → back to Ready, both runnable again.
        assert!(s.resume(a), "resume of a Suspended fiber succeeds");
        assert_eq!(s.suspended_token(a), None, "no longer parked after resume");
        assert_eq!(
            s.ready_order(),
            vec![a, b],
            "resume re-queues a into the Ready set (spawn order)"
        );
        // A second resume (already Ready) is a no-op false — the §6 "unknown/
        // already-finished token" host-side error, not a panic.
        assert!(!s.resume(a), "resuming a non-suspended fiber is a no-op false");
        // A resume of an unknown id is likewise a no-op false.
        assert!(!s.resume(999), "resume of an unknown fiber id is a no-op false");
    }

    // ── Slice 3: live supervisor ─────────────────────────────────────────────

    #[test]
    fn restart_set_matches_oracle() {
        // Byte-identical to supervisor_tree.ax's @[test]s.
        assert_eq!(restart_set(ONE_FOR_ONE, 1, 4), vec![1]);
        assert_eq!(restart_set(ONE_FOR_ALL, 2, 4), vec![0, 1, 2, 3]);
        assert_eq!(restart_set(REST_FOR_ONE, 2, 5), vec![2, 3, 4]);
        assert_eq!(
            restart_set(REST_FOR_ONE, 0, 3),
            vec![0, 1, 2],
            "first under rest = all"
        );
        assert_eq!(restart_set(REST_FOR_ONE, 3, 4), vec![3], "last = just last");
        assert!(
            restart_set(ONE_FOR_ONE, 9, 4).is_empty(),
            "out-of-range = nothing"
        );
        assert!(restart_set(ONE_FOR_ALL, -1, 4).is_empty());
    }

    #[test]
    fn supervisor_on_failure_counts_and_maps_to_fiber_ids() {
        // children at scheduler ids [10, 11, 12]; child 1 (fiber 11) fails under
        // one_for_one → restart {11}; the count bumps; supervisor stays alive.
        let mut sup = Supervisor::new(ONE_FOR_ONE, 3);
        sup.supervise(10);
        sup.supervise(11);
        sup.supervise(12);
        let to_restart = sup.on_failure(1);
        assert_eq!(
            to_restart,
            vec![11],
            "maps child index → scheduler fiber id"
        );
        assert_eq!(sup.restarts, 1);
        assert!(sup.alive());
    }

    #[test]
    fn supervisor_max_intensity_halts_and_latches() {
        // max_restarts=2: the 3rd failure trips the latch and restarts nothing.
        // Oracle test_max_intensity_halts_and_latches.
        let mut sup = Supervisor::new(ONE_FOR_ALL, 2);
        sup.supervise(0);
        sup.supervise(1);
        sup.supervise(2);
        assert!(!sup.on_failure(0).is_empty(), "restart 1");
        assert!(!sup.on_failure(0).is_empty(), "restart 2");
        assert!(sup.alive());
        let r3 = sup.on_failure(0); // restart 3 > 2 → halt
        assert!(!sup.alive(), "the tripping failure halts");
        assert!(r3.is_empty(), "nothing restarted on the trip");
        // Latched: a later failure still restarts nothing, count frozen.
        let r4 = sup.on_failure(1);
        assert!(!sup.alive());
        assert!(r4.is_empty());
        assert_eq!(sup.restarts, 3, "count frozen at the trip");
    }

    // ── Slice 4: durable Store consistency (value-logic == oracle) ───────────

    #[test]
    fn store_retry_diverges_by_consistency() {
        // THE headline (oracle test_retry_diverges_by_consistency): the same
        // retried op produces DIFFERENT state under the two variants.
        let mut a = Store::new(AT_LEAST_ONCE);
        assert!(a.apply(7, 100));
        assert!(a.apply(7, 100), "at_least_once re-applies a retry");
        assert_eq!(a.value, 200);

        let mut l = Store::new(LINEARIZABLE);
        assert!(l.apply(7, 100));
        assert!(
            !l.apply(7, 100),
            "linearizable dedups a retry (apply returns false)"
        );
        assert_eq!(l.value, 100);
        assert_ne!(
            a.value, l.value,
            "the divergence is the whole point of the axis"
        );
    }

    #[test]
    fn store_linearizable_version_is_total_order() {
        // Each DISTINCT applied op bumps version by 1; a deduped retry does not.
        let mut s = Store::new(LINEARIZABLE);
        s.apply(1, 5);
        s.apply(2, 5);
        let applied = s.apply(2, 5); // retry → deduped
        assert_eq!(s.version, 2, "version counts applied ops, not retries");
        assert!(!applied);
        assert!(s.has_applied(1) && s.has_applied(2));
    }

    #[test]
    fn store_at_least_once_keeps_no_version() {
        let mut s = Store::new(AT_LEAST_ONCE);
        s.apply(1, 5);
        s.apply(2, 5);
        assert_eq!(s.version, 0, "at_least_once maintains no total order");
        assert!(!s.has_applied(1), "at_least_once does not track seen ids");
    }

    // ── Slice 5: LLM gateway cost metering (== oracle call_cost) ─────────────

    #[test]
    fn llm_cost_scales_with_tokens() {
        // Per-token cost (the F4 property vs R3c's per-call meter): a 10000-token
        // call costs 10× a 1000-token call. Oracle test_cost_scales_with_tokens.
        let g = LlmGateway::new("haiku".into(), 1000, 0, "[fb]".into());
        assert_eq!(g.call_cost(1000), 1000);
        assert_eq!(g.call_cost(10000), 10000);
        assert_eq!(g.call_cost(10000), g.call_cost(1000) * 10);
        assert_eq!(g.call_cost(-5), 0, "negative tokens clamp to 0");
    }

    #[test]
    fn llm_gateway_spend_against_principal_budget() {
        // The kernel gateway debits a PRINCIPAL's budget — authority (Slice 1) and
        // spend are one model. An affordable call debits; an overrun does not and
        // leaves budget intact (the interpreter latches the gateway).
        let mut reg = PrincipalRegistry::new();
        let p = reg.root("agent".into(), true, false, false, 50_000);
        let g = LlmGateway::new("haiku".into(), 5000, p, "[fb]".into());
        // 2000-token call = 10000 µ$ ≤ 50000 → affordable; debit the principal.
        let cost = g.call_cost(2000);
        assert_eq!(cost, 10_000);
        assert!(reg.budget_remaining(p) >= cost, "principal can afford it");
        reg.spend(p, cost);
        assert_eq!(
            reg.budget_remaining(p),
            40_000,
            "spend debited from the principal"
        );
        // An overrun call (60000 > 40000 left) is not afforded.
        assert!(
            g.call_cost(12000) > reg.budget_remaining(p),
            "overrun exceeds the budget"
        );
    }
}
