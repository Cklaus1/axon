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
        Budget { used: self.used + amount, cap: self.cap }
    }
    fn remaining(self) -> i64 {
        let r = self.cap - self.used;
        if r < 0 { 0 } else { r }
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
        PrincipalRegistry { principals: Vec::new() }
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
        self.principals.get(handle).map(|p| p.budget.remaining()).unwrap_or(0)
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
        let Some(p) = self.principals.get(handle) else { return false };
        let caps = (!needs_net || p.net) && (!needs_fs_write || p.fs_write) && (!needs_exec || p.exec);
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
        let Some(p) = self.principals.get(parent_handle) else { return false };
        let caps_ok = (!want_net || p.net) && (!want_fs_write || p.fs_write) && (!want_exec || p.exec);
        let budget_ok = budget_grant > 0 && p.budget.remaining() > 0;
        caps_ok && budget_ok
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
        assert!(!grand.exec, "child never had exec → grandchild can't get it");
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
        assert!(!reg.authorize(r, false, true, false), "needs fs_write, lacks it");
        assert!(reg.can_mint(r, true, false, false, 10));
        assert!(!reg.can_mint(r, true, true, false, 10), "wants fs_write, lacks it");
        reg.spend(r, 100);
        assert!(!reg.authorize(r, true, false, false), "budget exhausted → denied");
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
}
