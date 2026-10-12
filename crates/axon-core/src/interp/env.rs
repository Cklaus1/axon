//! The lexical environment (`Env`) and its binding storage (`Bindings`),
//! R51 S2.1 (spec §4 Fork 2).
//!
//! A sealed run keeps a per-binding TAINT next to every binding's value
//! (`interp/taint.rs`): a parallel `Vec<u8>` indexed by the same slot, present
//! only in a sealed run. The storage is private to [`Bindings`] in this module,
//! so no code outside it can touch the vectors (a raw `env.vars` does not
//! compile), and every method that writes a binding takes the taint it stores
//! (`t`): there is no taint-free binding writer, so no site can drop a taint by
//! omission (v0.22's rule). Methods that only remove bindings (scope pop, clear,
//! release) truncate both vectors together. The two vectors are checked to be
//! in step after every method in debug builds; in a sealed run (the store
//! present) after every taint write and on every taint read in release builds
//! too, and a mismatch panics, fail-closed. Each writer has a `_t::<T>` form
//! for a caller that knows statically whether the run is sealed (the bytecode
//! engine, the evaluator's two instantiations): its unsealed form carries no
//! store code at all, so an unsealed run pays no branch on the store.
//!
//! Whether an Env carries a taint store is a property of the run, not of the
//! Env: an Env is made only by the `Interp` factory (`Interp::new_env`,
//! `Interp::env_from_snapshot`, the frame pool), which gives every Env of a
//! sealed run a store and no Env of an unsealed run one. Reading a taint from
//! an Env without a store panics: a missing store never reads as 0.

use super::*;

/// The bindings of one call frame: every visible binding in one vector
/// (outermost first), `marks` holding the start index of each pushed scope (the
/// base scope starts at 0 and has no mark), and in a sealed run the taint of
/// each binding at the same index.
///
/// Flat rather than one `HashMap` per scope because a call frame holds a
/// handful of names, and pushing a scope allocates nothing. Semantics are the
/// per-scope-map ones exactly: `define` replaces a same-named binding in the
/// CURRENT scope (so a loop re-binding a name does not grow the stack) and
/// shadows outer ones; lookups see the innermost binding first.
///
/// A binding's index in `vars` is the frame SLOT [`Resolution`] gave it
/// (AX-53), so a resolved variable access is `vars[slot]`: one index and one
/// sym compare, independent of how many locals are in scope. Every binding
/// keeps its sym, which serves code resolved by name ([`NAMED`]: a reverse
/// scan, the innermost binding first), [`Bindings::snapshot`], and the fallback
/// when a slot does not hold the expected binding. A binding's value and its
/// taint are read through the one index the lookup returns
/// ([`Bindings::get_var_idx`], [`Bindings::find`]), never through two lookups.
pub(super) struct Bindings {
    vars: Vec<(Sym, Value)>,
    /// The taint of each binding: as long as `vars` in a sealed run (`store`),
    /// empty and never allocated otherwise. A plain vector rather than an
    /// `Option` so the methods that only remove bindings truncate it without
    /// a branch on the store (R50's call-path budget, spec §10).
    taints: Vec<u8>,
    /// Whether this Env carries a taint store: exactly in a sealed run.
    store: bool,
    marks: Vec<usize>,
}

/// A copy of the visible bindings, each name once (inner shadows outer), with
/// their taints in a sealed run: what a handler arm, a continuation replay and
/// the module-level lets are built from.
#[derive(Clone)]
pub(super) struct Snapshot {
    vars: Vec<(Sym, Value)>,
    taints: Option<Vec<u8>>,
}

impl Snapshot {
    /// The bindings with their taints: `None` for the taints outside a sealed
    /// run.
    pub(super) fn into_parts(self) -> (Vec<(Sym, Value)>, Option<Vec<u8>>) {
        (self.vars, self.taints)
    }
}

/// What a closure's capture cell holds (AX-40, T40): the free variables'
/// bindings and, in a sealed run, the taint of each (R51 S2.3, TaintPort-5),
/// in step by construction: the fields are private to this module, every
/// constructor takes the taints with the bindings, and the only writers are
/// the closure call's lend, copy and write-back ([`Bindings::lend_in`],
/// [`Bindings::copy_in`], [`Bindings::lend_back`], [`Bindings::write_back`]),
/// which move or copy the taints with the values. Reads see the bindings
/// (`Deref` to the slice). The bindings sit in a [`Held`] so that, on wasm32,
/// the cell's AX-59 bounded `Drop` (below) drops them through
/// `drop_bounded` (R50 S12): the cell is the last thing a closure chain's
/// drop glue passes before the captured values.
#[derive(Debug, Default)]
pub struct Captures {
    vars: Held<Vec<(Sym, Value)>>,
    /// As long as `vars` when the cell was made in a sealed run, else empty
    /// (and never allocated).
    taints: Vec<u8>,
}

#[cfg(target_arch = "wasm32")]
impl Drop for Captures {
    #[inline]
    fn drop(&mut self) {
        // Moved out whole (an empty cell may still own a lent frame's
        // capacity); what `take` leaves allocates nothing.
        super::bounded_drop::drop_bounded(std::mem::take(&mut *self.vars));
    }
}

impl Captures {
    /// A cell made outside a sealed run: a capture-free fn value
    /// (`fn_value`), or a closure rebuilt from a `SendValue` (which carries
    /// no taint). A sealed frame that lends or copies such a cell with any
    /// binding in it stops ([`Captures::taints_for`]).
    pub(super) fn plain(vars: Vec<(Sym, Value)>) -> Self {
        Captures {
            vars: Held::new(vars),
            taints: Vec::new(),
        }
    }

    /// The cell's taints for a frame with a store (`sealed`): exactly one per
    /// binding, or the run stops rather than read a capture's taint as 0.
    #[inline(always)]
    fn taints_for(&self, sealed: bool) -> &[u8] {
        if sealed {
            if self.taints.len() != self.vars.len() {
                no_store("a capture cell without a taint per binding in a sealed frame");
            }
        } else {
            debug_assert!(
                self.taints.is_empty(),
                "R51 taint store: a capture cell with taints in an unsealed frame"
            );
        }
        &self.taints
    }

    /// The taint of capture `i` (a cell made in a sealed run).
    #[cfg(test)]
    pub(super) fn taint(&self, i: usize) -> u8 {
        self.taints_for(true)[i]
    }
}

impl std::ops::Deref for Captures {
    type Target = [(Sym, Value)];
    #[inline(always)]
    fn deref(&self) -> &[(Sym, Value)] {
        &self.vars
    }
}

/// A call frame: its [`Bindings`] and the operand stack of a bytecode-engine
/// activation running on it (R50): `vm::exec` takes the stack for the
/// activation and hands it back emptied, so a pooled frame brings a grown stack
/// with it. The tree-walker never touches the stack.
///
/// Each call frame (a fn call, a closure call) has its own `Env`; it derefs to
/// its bindings, so `env.define(..)` is a [`Bindings`] method.
pub(super) struct Env {
    bindings: Bindings,
    pub(super) stack: Vec<Value>,
}

impl std::ops::Deref for Env {
    type Target = Bindings;
    #[inline(always)]
    fn deref(&self) -> &Bindings {
        &self.bindings
    }
}

impl std::ops::DerefMut for Env {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Bindings {
        &mut self.bindings
    }
}

impl Env {
    /// A store-less frame for the unit tests that evaluate against an Env
    /// with no `Interp` (the VM's pure-expression tests). Every other Env
    /// comes from the factory.
    #[cfg(test)]
    pub(super) fn new() -> Self {
        Self::with_store(false)
    }

    /// An empty frame, with a taint store iff `taints` (a sealed run).
    /// Private: only the factory below calls it.
    fn with_store(taints: bool) -> Self {
        Env {
            bindings: Bindings {
                vars: Vec::new(),
                taints: Vec::new(),
                store: taints,
                marks: Vec::new(),
            },
            stack: Vec::new(),
        }
    }

    /// An env whose single base scope is snapshot `snap`, to run a
    /// handler-arm body or a continuation replay in its defining environment.
    /// The snapshot binds each name at most once. Private: only the factory
    /// below calls it.
    fn from_snapshot(snap: Snapshot) -> Self {
        let store = snap.taints.is_some();
        let env = Env {
            bindings: Bindings {
                vars: snap.vars,
                taints: snap.taints.unwrap_or_default(),
                store,
                marks: Vec::new(),
            },
            stack: Vec::new(),
        };
        if store {
            env.check_store();
        }
        env.check();
        env
    }
}

// ── The Env factory (R51 S2.2) ───────────────────────────────────────────────
//
// Every Env of a run is made here, by the `Interp`, which knows whether the run
// is sealed (`seal.active`, fixed at build): a sealed run's Envs carry a taint
// store, an unsealed run's never do. `Env`'s constructors are private to this
// module, so no other code can make an Env that disagrees with its run.
impl Interp<'_> {
    /// A fresh Env for this run: a predicate env, the module-level lets.
    #[inline(always)]
    pub(super) fn new_env(&self) -> Env {
        Env::with_store(self.seal.active)
    }

    /// An Env whose single base scope is `snap`, a snapshot taken in this
    /// run. A snapshot whose store does not match the run (a store-less one
    /// in a sealed run) stops it: its taints would otherwise be lost.
    pub(super) fn env_from_snapshot(&self, snap: Snapshot) -> Env {
        if snap.taints.is_some() != self.seal.active {
            no_store("a snapshot whose taint store does not match the run");
        }
        Env::from_snapshot(snap)
    }

    /// AX-54: an empty call frame from the pool of finished calls' frames, so
    /// a call does not allocate its bindings and scope marks. Boxed, so
    /// taking and returning one moves a pointer, not the frame. Every pooled
    /// frame was made by [`Interp::new_env`] for this run and emptied on
    /// give, so it comes back empty, with a store exactly in a sealed run.
    #[inline(always)]
    pub(super) fn take_frame(&self) -> Box<Env> {
        if let Some(env) = self.frame_spare.take() {
            #[cfg(debug_assertions)]
            self.check_fresh_frame(&env);
            return env;
        }
        self.take_pooled_frame()
    }

    #[inline(never)]
    fn take_pooled_frame(&self) -> Box<Env> {
        match self.env_pool.borrow_mut().pop() {
            Some(env) => {
                #[cfg(debug_assertions)]
                self.check_fresh_frame(&env);
                env
            }
            None => Box::new(self.new_env()),
        }
    }

    /// The debug check of a pooled frame [`Interp::take_frame`] hands out:
    /// empty, with a store exactly in a sealed run. Out of line, so it adds
    /// nothing to the frame of the call path that takes one (AX-56).
    #[cfg(debug_assertions)]
    #[inline(never)]
    fn check_fresh_frame(&self, env: &Env) {
        assert!(env.is_fresh() && env.has_store() == self.seal.active);
    }

    /// Return a finished call's frame to the spare slot (the slot's previous
    /// frame to the pool), emptied here, its taints with its bindings
    /// (dropping the bindings where dropping the frame did); a frame that
    /// grew unusually large is not kept.
    #[inline(always)]
    pub(super) fn give_frame(&self, mut env: Box<Env>) {
        env.clear();
        self.keep_frame(env);
    }

    /// [`Interp::give_frame`] for a caller that knows the run (see
    /// [`Bindings::guard`]): the bytecode engine's calls.
    #[inline(always)]
    pub(super) fn give_frame_t<const T: bool>(&self, mut env: Box<Env>) {
        env.clear_t::<T>();
        self.keep_frame(env);
    }

    #[inline(always)]
    fn keep_frame(&self, env: Box<Env>) {
        if env.capacity() > 1024 {
            return;
        }
        if let Some(prev) = self.frame_spare.replace(Some(env)) {
            self.pool_frame(prev);
        }
    }

    #[inline(never)]
    fn pool_frame(&self, env: Box<Env>) {
        let mut pool = self.env_pool.borrow_mut();
        if pool.len() < 64 {
            pool.push(env);
        }
    }
}

/// A taint read or a desync on an Env whose store is absent or out of step:
/// the sealed run stops here rather than read a taint it does not have.
#[cold]
#[inline(never)]
fn no_store(what: &str) -> ! {
    panic!("R51 taint store: {what}")
}

impl Bindings {
    /// The two vectors are in step: an unsealed Env's taint vector is empty,
    /// a sealed Env's is as long as its bindings. Debug builds only; release
    /// builds check a sealed Env after every taint write and on every taint
    /// read ([`Bindings::check_store`]). The assertion is out of line, so it
    /// adds nothing to the frames of the call paths that inline the writers
    /// (wasm32 stack budget, AX-56).
    #[inline(always)]
    fn check(&self) {
        #[cfg(debug_assertions)]
        self.check_in_step();
    }

    #[cfg(debug_assertions)]
    #[inline(never)]
    fn check_in_step(&self) {
        assert!(
            if self.store {
                self.taints.len() == self.vars.len()
            } else {
                self.taints.is_empty()
            },
            "R51 taint store: the taint vector is out of step with the bindings"
        );
    }

    /// The release-build check of a sealed Env (the store present): a
    /// mismatch stops the run, fail-closed. Called only where the store was
    /// just written or is about to be read, so an unsealed run never pays it.
    #[inline(always)]
    fn check_store(&self) {
        if self.taints.len() != self.vars.len() {
            no_store("the taint vector is out of step with the bindings");
        }
    }

    /// The store guard of a `_t::<T>` writer. `T = true` is the sealed
    /// instantiation (the sealed evaluator): the Env must carry a store, in
    /// every build. `T = false` is a caller that can only run unsealed (the
    /// bytecode engine, which refuses to compile in a sealed run, and the
    /// unsealed evaluator, entered only when `seal.active` is false): its
    /// writer compiles to the store-less code alone, with no branch, and the
    /// guard is a debug assertion. A `T = false` write into a sealed Env
    /// would leave the taint vector short, which the next sealed write or
    /// taint read stops on.
    #[inline(always)]
    fn guard<const T: bool>(&self, t: u8) {
        if T {
            if !self.store {
                no_store("a sealed write to an Env that has no taint store");
            }
        } else {
            #[cfg(debug_assertions)]
            self.guard_unsealed(t);
            #[cfg(not(debug_assertions))]
            let _ = t;
        }
    }

    /// The debug assertion of [`Bindings::guard`]'s `T = false` writer, out
    /// of line like [`Bindings::check_in_step`].
    #[cfg(debug_assertions)]
    #[inline(never)]
    fn guard_unsealed(&self, t: u8) {
        assert!(
            !self.store && t == 0,
            "R51 taint store: an unsealed-only write in a sealed Env"
        );
    }

    /// Whether this Env carries a taint store (exactly in a sealed run).
    #[inline(always)]
    pub(super) fn has_store(&self) -> bool {
        self.store
    }

    /// How many bindings (pads included) the frame holds.
    #[inline(always)]
    pub(super) fn len(&self) -> usize {
        self.vars.len()
    }

    /// The number of pushed scopes.
    #[cfg(test)]
    pub(super) fn depth(&self) -> usize {
        self.marks.len()
    }

    /// The binding vector's capacity (the frame pool keeps no frame that grew
    /// unusually large).
    #[inline(always)]
    pub(super) fn capacity(&self) -> usize {
        self.vars.capacity()
    }

    /// Whether the frame is fresh: no binding and no scope.
    #[inline(always)]
    pub(super) fn is_fresh(&self) -> bool {
        self.vars.is_empty() && self.marks.is_empty()
    }

    /// Bind a fresh frame's parameters: each `(name, value)` of `params` is
    /// the next binding (parameter `i` in slot `i`, AX-53), carrying taint
    /// `taint(i)`. One `extend` over a trusted-length iterator reserves once
    /// instead of a capacity check per push (cost only, R50 S5); `taint` is
    /// called only when the frame has a store.
    #[inline(always)]
    pub(super) fn bind_fresh(
        &mut self,
        params: impl Iterator<Item = (Sym, Value)>,
        taint: impl FnMut(usize) -> u8,
    ) {
        debug_assert!(self.is_fresh());
        self.vars.extend(params);
        if self.store {
            self.taints.extend((0..self.vars.len()).map(taint));
            self.check_store();
        }
        self.check();
    }

    /// Room for `n` more bindings.
    #[inline(always)]
    pub(super) fn reserve(&mut self, n: usize) {
        self.vars.reserve(n);
        if self.store {
            self.taints.reserve(n);
        }
    }

    /// [`Bindings::reserve`] for a caller that knows the run (see
    /// [`Bindings::guard`]).
    #[inline(always)]
    pub(super) fn reserve_t<const T: bool>(&mut self, n: usize) {
        self.guard::<T>(0);
        self.vars.reserve(n);
        if T {
            self.taints.reserve(n);
        }
    }

    /// Open a scope.
    #[inline(always)]
    pub(super) fn push(&mut self) {
        self.marks.push(self.vars.len());
    }

    /// Close the innermost scope, dropping its bindings and their taints.
    /// (Truncating the always-empty taint vector of an unsealed Env is a
    /// compare, no branch on the store.)
    #[inline(always)]
    pub(super) fn pop(&mut self) {
        self.pop_t::<true>();
    }

    /// [`Bindings::pop`] for a caller that knows the run (see
    /// [`Bindings::guard`]): the unsealed form leaves the (empty) taint
    /// vector alone. (A remover's `true` form is correct in any run.)
    #[inline(always)]
    pub(super) fn pop_t<const T: bool>(&mut self) {
        let start = self.marks.pop().unwrap_or(0);
        self.vars.truncate(start);
        if T {
            self.taints.truncate(start);
        }
        self.check();
    }

    /// Empty the frame for reuse (the taint store stays, emptied). A scalar
    /// binding has nothing to drop, so it is forgotten instead of going
    /// through `Value`'s drop glue; every other binding is dropped (cost
    /// only). A frame of scalars alone (the common call's) is emptied by its
    /// length, inline; any other out of line.
    #[inline(always)]
    pub(super) fn clear(&mut self) {
        self.clear_t::<true>();
    }

    /// [`Bindings::clear`] for a caller that knows the run (see
    /// [`Bindings::guard`]): the unsealed form leaves the (empty) taint
    /// vector alone.
    #[inline(always)]
    pub(super) fn clear_t<const T: bool>(&mut self) {
        if self.vars.iter().all(|(_, v)| is_scalar(v)) && self.marks.is_empty() {
            // SAFETY: a length of 0 is within capacity and leaves no
            // element initialised past it to be read; the elements it
            // forgets are scalars, which own nothing, so forgetting one
            // is what dropping it does.
            unsafe { self.vars.set_len(0) };
            if T {
                self.taints.clear();
            }
        } else {
            self.clear_slow();
        }
        self.check();
    }

    #[inline(never)]
    fn clear_slow(&mut self) {
        while let Some((_, v)) = self.vars.pop() {
            forget_scalar(v);
        }
        self.taints.clear();
        self.marks.clear();
    }

    /// Drop the bindings from index `len` on, as `vars.truncate(len)` does,
    /// forgetting scalars as [`Bindings::clear`] does (R50 S4, cost only).
    #[inline]
    pub(super) fn release_to(&mut self, len: usize) {
        while self.vars.len() > len {
            let Some((_, v)) = self.vars.pop() else { break };
            forget_scalar(v);
        }
        self.taints.truncate(len);
        self.check();
    }

    /// Bind `name` to `val` carrying taint `t` in the current scope,
    /// replacing a same-named binding of this scope.
    pub(super) fn define(&mut self, name: Sym, val: Value, t: u8) {
        if self.store {
            self.define_t::<true>(name, val, t)
        } else {
            self.define_t::<false>(name, val, t)
        }
    }

    /// [`Bindings::define`] for a caller that knows the run (see
    /// [`Bindings::guard`]).
    #[inline]
    pub(super) fn define_t<const T: bool>(&mut self, name: Sym, val: Value, t: u8) {
        let start = self.marks.last().copied().unwrap_or(0);
        match self.vars[start..].iter().position(|(k, _)| *k == name) {
            Some(off) => self.set_t::<T>(start + off, val, t),
            None => self.push_var_t::<T>(name, val, t),
        }
    }

    /// Bind `name` to `val` carrying taint `t` as the NEXT binding (slot
    /// `len()`): a parameter, `goal_met`, a slotted closure parameter. The
    /// caller knows no same-named binding of the current scope is at or above
    /// that slot (a fresh frame or a fresh scope).
    #[inline(always)]
    pub(super) fn push_var(&mut self, name: Sym, val: Value, t: u8) {
        if self.store {
            self.push_var_t::<true>(name, val, t)
        } else {
            self.push_var_t::<false>(name, val, t)
        }
    }

    /// [`Bindings::push_var`] for a caller that knows the run (see
    /// [`Bindings::guard`]).
    #[inline(always)]
    pub(super) fn push_var_t<const T: bool>(&mut self, name: Sym, val: Value, t: u8) {
        self.guard::<T>(t);
        self.vars.push((name, val));
        if T {
            self.taints.push(t);
            self.check_store();
        }
        self.check();
    }

    /// Overwrite binding `i` (value and taint).
    #[inline(always)]
    fn set_t<const T: bool>(&mut self, i: usize, val: Value, t: u8) {
        self.guard::<T>(t);
        self.vars[i].1 = val;
        if T {
            self.taints[i] = t;
            self.check_store();
        }
        self.check();
    }

    /// Bind `name` at frame slot `slot` (see [`Resolution::var`]), or by name
    /// for [`NAMED`], carrying taint `t`. A slot is in the current scope
    /// (`Resolution` allocates a scope's slots above every live one when the
    /// scope is entered), so writing it replaces a same-named binding of this
    /// scope as `define` does. A slot holding another binding (an env not laid
    /// out the way the table assumed) falls back to `define`; the reads'
    /// fallback finds it. A gap below the slot is padded with unnamed
    /// bindings (taint 0; no read names them).
    #[inline]
    pub(super) fn define_var(&mut self, name: Sym, slot: u32, val: Value, t: u8) {
        if self.store {
            self.define_var_t::<true>(name, slot, val, t)
        } else {
            self.define_var_t::<false>(name, slot, val, t)
        }
    }

    /// [`Bindings::define_var`] for a caller that knows the run (see
    /// [`Bindings::guard`]).
    #[inline]
    pub(super) fn define_var_t<const T: bool>(&mut self, name: Sym, slot: u32, val: Value, t: u8) {
        let i = slot as usize;
        // The common case: the slot is the next binding (a parameter, a
        // `let` in a fresh scope). `i == len` implies `i >= start` and
        // `slot != NAMED`, so this is the `None` arm below with nothing to
        // fill (cost only, AX-53).
        if i == self.vars.len() {
            return self.push_var_t::<T>(name, val, t);
        }
        let start = self.marks.last().copied().unwrap_or(0);
        if slot == NAMED || i < start {
            return self.define_t::<T>(name, val, t);
        }
        match self.vars.get_mut(i) {
            Some(b) if b.0 == name || b.0 == SYM_NONE => {
                b.0 = name;
                self.set_t::<T>(i, val, t);
            }
            Some(_) => self.define_t::<T>(name, val, t),
            None => {
                self.vars.resize_with(i, || (SYM_NONE, Value::Unit));
                if T {
                    self.taints.resize(i, 0);
                }
                self.push_var_t::<T>(name, val, t);
            }
        }
    }

    /// The index of the innermost binding of `name` (the name scan).
    #[inline]
    pub(super) fn find(&self, name: Sym) -> Option<usize> {
        self.vars.iter().rposition(|(k, _)| *k == name)
    }

    /// The innermost binding of `name`'s value.
    #[inline]
    pub(super) fn get(&self, name: Sym) -> Option<&Value> {
        self.find(name).map(|i| &self.vars[i].1)
    }

    /// The index of the binding variable `name` resolved to `slot` (see
    /// [`Resolution::var`]) reads: the slot, if it holds `name`; none for
    /// [`NOT_LOCAL`]; else the innermost binding of `name`. The one lookup
    /// both the value and the taint of a variable are read through.
    #[inline(always)]
    pub(super) fn get_var_idx(&self, name: Sym, slot: u32) -> Option<usize> {
        let i = slot as usize;
        match self.vars.get(i) {
            Some((k, _)) if *k == name => Some(i),
            _ if slot == NOT_LOCAL => None,
            _ => self.find(name),
        }
    }

    /// The value of the binding [`Bindings::get_var_idx`] finds.
    #[inline(always)]
    pub(super) fn get_var(&self, name: Sym, slot: u32) -> Option<&Value> {
        self.get_var_idx(name, slot).map(|i| &self.vars[i].1)
    }

    /// The taint of binding `i` (an index a lookup returned). Panics on an
    /// Env without a store (a missing store never reads as 0) and on a store
    /// out of step with the bindings, in every build.
    #[inline]
    pub(super) fn taint(&self, i: usize) -> u8 {
        if !self.store {
            no_store("a taint read from an Env that has no taint store");
        }
        self.check_store();
        self.taints[i]
    }

    /// Update the binding [`Bindings::get_var_idx`] finds to `val` carrying
    /// taint `t` (the binding takes the taint of the value now in it);
    /// returns false if none exists. The caller knows the run (see
    /// [`Bindings::guard`]).
    #[inline]
    pub(super) fn assign_var_t<const T: bool>(
        &mut self,
        name: Sym,
        slot: u32,
        val: Value,
        t: u8,
    ) -> bool {
        match self.get_var_idx(name, slot) {
            Some(i) => {
                self.set_t::<T>(i, val, t);
                true
            }
            None => false,
        }
    }

    /// Mutable access to binding `i` for a write INTO the value it holds (a
    /// place assignment, an in-place update): the binding then carries `t`
    /// too (OR'd into its taint).
    #[inline(always)]
    pub(super) fn value_mut(&mut self, i: usize, t: u8) -> &mut Value {
        if self.store {
            self.value_mut_t::<true>(i, t)
        } else {
            self.value_mut_t::<false>(i, t)
        }
    }

    /// [`Bindings::value_mut`] for a caller that knows the run (see
    /// [`Bindings::guard`]).
    #[inline(always)]
    pub(super) fn value_mut_t<const T: bool>(&mut self, i: usize, t: u8) -> &mut Value {
        self.guard::<T>(t);
        if T {
            self.taints[i] |= t;
            self.check_store();
        }
        self.check();
        &mut self.vars[i].1
    }

    /// [`Bindings::value_mut_t`] of the innermost binding of `name`, for a
    /// caller that knows the run (see [`Bindings::guard`]).
    #[inline]
    pub(super) fn get_mut_t<const T: bool>(&mut self, name: Sym, t: u8) -> Option<&mut Value> {
        let i = self.find(name)?;
        Some(self.value_mut_t::<T>(i, t))
    }

    /// [`Bindings::value_mut`] of the binding [`Bindings::get_var_idx`] finds.
    #[inline(always)]
    pub(super) fn get_var_mut(&mut self, name: Sym, slot: u32, t: u8) -> Option<&mut Value> {
        let i = self.get_var_idx(name, slot)?;
        Some(self.value_mut(i, t))
    }

    /// [`Bindings::get_var_mut`] for a caller that knows the run (see
    /// [`Bindings::guard`]).
    #[inline(always)]
    pub(super) fn get_var_mut_t<const T: bool>(
        &mut self,
        name: Sym,
        slot: u32,
        t: u8,
    ) -> Option<&mut Value> {
        let i = self.get_var_idx(name, slot)?;
        Some(self.value_mut_t::<T>(i, t))
    }

    /// Replace binding `i`'s value with `val` carrying taint `t`, returning
    /// the old value (a `&mut` argument moved out and back). The `&mut` call
    /// path that uses it is the bytecode engine's, so this is the `T` form
    /// alone (see [`Bindings::guard`]).
    #[inline(always)]
    pub(super) fn replace_t<const T: bool>(&mut self, i: usize, val: Value, t: u8) -> Value {
        self.guard::<T>(t);
        let old = std::mem::replace(&mut self.vars[i].1, val);
        if T {
            self.taints[i] = t;
            self.check_store();
        }
        self.check();
        old
    }

    /// The bytecode engine's `for` re-bind (R50): when the innermost scope
    /// holds exactly the loop variable `name` at `slot` as an `Int`, set it
    /// to `i` carrying taint `t` in place and return true. The engine runs
    /// unsealed only, so this is the `T` form alone (see
    /// [`Bindings::guard`]).
    #[inline(always)]
    pub(super) fn rebind_loop_int_t<const T: bool>(
        &mut self,
        name: Sym,
        slot: u32,
        i: i64,
        t: u8,
    ) -> bool {
        let Some(&m) = self.marks.last() else {
            return false;
        };
        if self.vars.len() != m + 1 || slot as usize != m {
            return false;
        }
        self.guard::<T>(t);
        match self.vars.last_mut() {
            Some((s, Value::Int(n))) if *s == name => {
                *n = i;
                if T {
                    self.taints[m] = t;
                    self.check_store();
                }
                self.check();
                true
            }
            _ => false,
        }
    }

    /// All visible bindings, each name once (inner shadows outer), with their
    /// taints in a sealed run.
    pub(super) fn snapshot(&self) -> Snapshot {
        let mut vars = Vec::with_capacity(self.vars.len());
        let mut taints = self.store.then(|| {
            self.check_store();
            Vec::with_capacity(self.vars.len())
        });
        for (i, (k, v)) in self.vars.iter().enumerate() {
            if *k != SYM_NONE && !self.vars[i + 1..].iter().any(|(inner, _)| inner == k) {
                vars.push((*k, v.clone()));
                if let Some(out) = &mut taints {
                    out.push(self.taints[i]);
                }
            }
        }
        Snapshot { vars, taints }
    }

    /// A closure's capture cell (AX-40): the binding of each free variable
    /// `(name, slot)` bound here, with its taint when this Env carries a store
    /// (a sealed run), value and taint read through the one index.
    pub(super) fn capture(&self, frees: impl Iterator<Item = (Sym, u32)>) -> Captures {
        let mut vars = Vec::new();
        let mut taints = Vec::new();
        for (s, slot) in frees {
            if let Some(i) = self.get_var_idx(s, slot) {
                vars.push((s, self.vars[i].1.clone()));
                if self.store {
                    taints.push(self.taint(i));
                }
            }
        }
        Captures {
            vars: Held::new(vars),
            taints,
        }
    }

    /// AX-31's lend: the closure's capture cell `cell` becomes this empty
    /// frame's base scope, each binding with the taint the cell holds for it
    /// (the frame's empty vectors wait in the cell; they swap back in
    /// [`Bindings::lend_back`], so both keep their capacity).
    #[inline]
    pub(super) fn lend_in(&mut self, cell: &mut Captures) {
        debug_assert!(self.vars.is_empty() && self.marks.is_empty());
        if self.store {
            cell.taints_for(true);
            std::mem::swap(&mut self.taints, &mut cell.taints);
        } else {
            cell.taints_for(false);
        }
        std::mem::swap(&mut self.vars, &mut *cell.vars);
        if self.store {
            self.check_store();
        }
        self.check();
    }

    /// The end of a lent call: drop everything above the base scope (`base`
    /// bindings) and swap the base scope back into the cell, its taints with
    /// it (what each captured name holds now carries what the body gave it),
    /// leaving the frame's empty vectors here.
    #[inline]
    pub(super) fn lend_back(&mut self, base: usize, cell: &mut Captures) {
        self.release_to(base);
        self.marks.clear();
        std::mem::swap(&mut self.vars, &mut *cell.vars);
        debug_assert!(self.vars.is_empty());
        if self.store {
            std::mem::swap(&mut self.taints, &mut cell.taints);
            debug_assert!(self.taints.is_empty());
        }
        self.check();
    }

    /// The copy path of a closure call: the capture cell's bindings become
    /// this empty frame's base scope as copies, each with the taint the cell
    /// holds for it, with room for `extra` more (the parameters).
    #[inline]
    pub(super) fn copy_in(&mut self, cell: &Captures, extra: usize) {
        debug_assert!(self.vars.is_empty() && self.marks.is_empty());
        self.vars.reserve(cell.vars.len() + extra);
        self.vars.extend_from_slice(&cell.vars);
        if self.store {
            self.taints.reserve(cell.vars.len() + extra);
            self.taints.extend_from_slice(cell.taints_for(true));
            self.check_store();
        } else {
            cell.taints_for(false);
        }
        self.check();
    }

    /// The copy path's write-back: the base scope's first `base` values,
    /// positionally into the capture cell, and their taints into the cell's
    /// (a re-entrant call may have written the cell meanwhile; this call's
    /// write-back wins).
    #[inline]
    pub(super) fn write_back(&self, base: usize, cell: &mut Captures) {
        for ((_, slot), (_, v)) in cell.vars.iter_mut().zip(&self.vars[..base]) {
            *slot = v.clone();
        }
        if self.store {
            self.check_store();
            cell.taints_for(true);
            for (slot, t) in cell.taints.iter_mut().zip(&self.taints[..base]) {
                *slot = *t;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealed_env() -> Env {
        Env::with_store(true)
    }

    fn taints(env: &Env) -> Vec<u8> {
        assert!(env.bindings.store, "a sealed env has a store");
        env.bindings.taints.clone()
    }

    /// R51 S2.1 drift: binding storage is private to this file. Rust privacy
    /// is the enforcement (a raw `env.vars`, `env.marks` or taint access
    /// outside `interp/env.rs` does not compile, because the fields of
    /// `Bindings` and `Env::bindings` are private to this module). This test
    /// guards the privacy itself: it fails if a field is made visible, or if
    /// a `Bindings`/`Snapshot` value is built (or its storage named) anywhere
    /// else under `src/interp`.
    #[test]
    fn binding_storage_is_private_to_the_env_module() {
        let src = include_str!("env.rs");
        let body = |head: &str| -> &str {
            let at = src
                .find(head)
                .unwrap_or_else(|| panic!("`{head}` not found"));
            let rest = &src[at..];
            &rest[..rest.find("\n}\n").expect("struct end")]
        };
        for head in [
            "pub(super) struct Bindings {",
            "pub(super) struct Env {",
            "pub(super) struct Snapshot {",
        ] {
            for line in body(head).lines().skip(1) {
                let l = line.trim_start();
                if l.starts_with("///") || l.is_empty() {
                    continue;
                }
                let storage = ["vars:", "taints:", "marks:", "bindings:"].iter().any(|f| {
                    l.trim_start_matches("pub(super) ")
                        .trim_start_matches("pub ")
                        .starts_with(f)
                });
                assert!(
                    !(storage && l.starts_with("pub")),
                    "`{head}` exposes its storage: `{l}` (R51 S2.1: binding storage stays private to interp/env.rs)"
                );
            }
        }
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/interp"));
        let mut offenders = Vec::new();
        let mut stack = vec![root.to_path_buf(), root.with_extension("rs")];
        while let Some(p) = stack.pop() {
            if p.is_dir() {
                for e in std::fs::read_dir(&p).expect("read_dir") {
                    stack.push(e.expect("entry").path());
                }
                continue;
            }
            if p.extension().is_none_or(|x| x != "rs") || p.ends_with("interp/env.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&p).expect("read");
            for (n, line) in text.lines().enumerate() {
                if ["Bindings {", "Snapshot {", ".taints", ".bindings."]
                    .iter()
                    .any(|pat| line.contains(pat))
                {
                    offenders.push(format!("{}:{}: {}", p.display(), n + 1, line.trim()));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "binding storage named outside interp/env.rs:\n{}",
            offenders.join("\n")
        );
    }

    /// Every method keeps the taint vector in step with the bindings: scope
    /// push/pop, `define` replace, `define_var` padding and fallback,
    /// `release_to`, `clear` (both paths), the closure lend and copy paths.
    #[test]
    fn every_bindings_method_keeps_the_taints_in_step() {
        let (a, b, c) = (intern("s2_a"), intern("s2_b"), intern("s2_c"));
        let mut env = sealed_env();
        env.define(a, Value::Int(1), 1);
        env.push();
        env.define(b, Value::Int(2), 2);
        env.define(b, Value::Int(3), 4);
        assert_eq!(
            taints(&env),
            vec![1, 4],
            "define replaces the taint with the value"
        );
        env.define_var(c, 5, Value::Int(9), 2);
        assert_eq!(taints(&env), vec![1, 4, 0, 0, 0, 2], "padding is untainted");
        assert!(env.assign_var_t::<true>(c, 5, Value::Int(10), 1));
        assert_eq!(
            taints(&env),
            vec![1, 4, 0, 0, 0, 1],
            "assignment sets the taint"
        );
        *env.get_var_mut(a, 0, 2).expect("bound") = Value::Int(5);
        assert_eq!(taints(&env)[0], 3, "a write into a value ORs the taint in");
        env.pop();
        assert_eq!(taints(&env), vec![3]);
        env.push();
        env.define(b, Value::Str(Rc::new("x".into())), 4);
        env.release_to(1);
        assert_eq!(taints(&env), vec![3]);
        env.define(b, Value::Str(Rc::new("y".into())), 4);
        env.clear();
        assert_eq!(taints(&env), Vec::<u8>::new(), "clear_slow");
        env.define(a, Value::Int(1), 4);
        env.clear();
        assert_eq!(taints(&env), Vec::<u8>::new(), "scalar clear");
        // A capture cell keeps each binding's taint, through the lend (moved
        // in and back), the copy (copied in) and the write-back (TaintPort-5).
        env.define(a, Value::Int(1), 2);
        env.define(b, Value::Int(2), 5);
        let mut cell = env.capture([(a, 0), (b, 1)].into_iter());
        assert_eq!((cell.len(), cell.taint(0), cell.taint(1)), (2, 2, 5));
        env.clear();
        env.lend_in(&mut cell);
        assert_eq!(taints(&env), vec![2, 5], "the lend moves the taints in");
        assert!(env.assign_var_t::<true>(a, 0, Value::Int(7), 1));
        env.push();
        env.push_var(c, Value::Int(3), 1);
        env.lend_back(2, &mut cell);
        assert!(env.is_fresh());
        assert_eq!(taints(&env), Vec::<u8>::new());
        assert_eq!(
            (cell.taint(0), cell.taint(1)),
            (1, 5),
            "and back, as the body left them"
        );
        env.copy_in(&cell, 1);
        assert_eq!(taints(&env), vec![1, 5], "the copy copies them");
        assert!(env.assign_var_t::<true>(b, 1, Value::Int(8), 3));
        env.write_back(2, &mut cell);
        assert_eq!(
            (cell.taint(0), cell.taint(1)),
            (1, 3),
            "the write-back writes them"
        );
        let snap = env.snapshot();
        let back = Env::from_snapshot(snap);
        assert_eq!(taints(&back), vec![1, 3], "a snapshot carries its taints");
    }

    /// A capture cell made without taints (a closure rebuilt from a
    /// `SendValue`) stops a sealed frame that lends or copies it, rather than
    /// reading its captures as untainted.
    #[test]
    #[should_panic(expected = "a capture cell without a taint per binding")]
    fn a_capture_cell_without_taints_panics_in_a_sealed_frame() {
        let mut env = sealed_env();
        let cell = Captures::plain(vec![(intern("s2_pc"), Value::Int(1))]);
        env.copy_in(&cell, 0);
    }

    /// The name-scan fallback returns an index, so a variable's value and
    /// its taint are read through the one lookup.
    #[test]
    fn the_slot_fallback_reads_value_and_taint_at_one_index() {
        let (a, b) = (intern("s2_fa"), intern("s2_fb"));
        let mut env = sealed_env();
        env.define(a, Value::Int(1), 1);
        env.define(b, Value::Int(2), 2);
        // Slot 0 holds `a`, not `b`: the fallback scan finds `b` at 1.
        let i = env.get_var_idx(b, 0).expect("found by name");
        assert_eq!(i, 1);
        assert!(matches!(env.get_var(b, 0), Some(Value::Int(2))));
        assert_eq!(taints(&env)[i], 2);
        assert_eq!(
            env.get_var_idx(b, 1),
            Some(1),
            "the slot itself when it holds the name"
        );
        assert_eq!(
            env.get_var_idx(b, NOT_LOCAL),
            None,
            "a non-local is never a binding"
        );
    }

    /// In a sealed run (the store present) an out-of-step store stops the run
    /// in release builds too: fail-closed.
    #[test]
    #[should_panic(expected = "out of step")]
    fn an_out_of_step_store_panics_in_every_build() {
        let mut env = sealed_env();
        env.define(intern("s2_d"), Value::Int(1), 0);
        env.bindings.taints.push(0);
        env.push_var(intern("s2_e"), Value::Int(2), 0);
    }

    // ── R51 S2.2: the Env factory ────────────────────────────────────────────

    /// A program whose candidate `fn solve` is loaded from the SEALED
    /// directory `/<tag>-sealed`, with that directory. The run is built with
    /// [`Interp::build_with`] (the engine already chosen), which computes the
    /// seal state without the engine pin `build_in` applies, so the
    /// factory's sealed branch is observable without a run.
    fn sealed_prog(tag: &str) -> (Program, std::path::PathBuf) {
        use crate::span::intern_source;
        let suite = "fn t() -> i64 { solve() }\n";
        let cand = "fn solve() -> i64 { 41 }\n";
        let sdir = format!("/{tag}-sealed");
        let s = crate::parse_source_in(suite, intern_source(&format!("/{tag}-suite/h.ax"), suite))
            .expect("suite parses");
        let c = crate::parse_source_in(cand, intern_source(&format!("{sdir}/f.ax"), cand))
            .expect("candidate parses");
        let prog = Program {
            items: s.items.into_iter().chain(c.items).collect(),
        };
        (prog, std::path::PathBuf::from(sdir))
    }

    /// Every Env a sealed run makes carries a taint store, and every Env an
    /// unsealed run makes carries none: fresh envs, snapshot envs, and pooled
    /// frames, which come back from `give_frame` emptied (taints with them)
    /// and still with the store.
    #[test]
    fn the_factory_gives_a_store_exactly_in_a_sealed_run() {
        let (prog, sdir) = sealed_prog("r51-s22-factory");
        let sealed = Interp::build_with(&prog, std::slice::from_ref(&sdir), Engine::Tree);
        let unsealed = Interp::build_with(&prog, &[], Engine::Tree);
        assert!(sealed.seal.active && !unsealed.seal.active);
        let x = intern("s22_x");
        for (ix, store) in [(&sealed, true), (&unsealed, false)] {
            let mut env = ix.new_env();
            assert_eq!(env.has_store(), store, "new_env");
            env.define(x, Value::Int(1), if store { 2 } else { 0 });
            let back = ix.env_from_snapshot(env.snapshot());
            assert_eq!(back.has_store(), store, "env_from_snapshot");
            let mut frame = ix.take_frame();
            assert_eq!(frame.has_store(), store, "a fresh pooled frame");
            frame.define(
                x,
                Value::Str(Rc::new("held".into())),
                if store { 1 } else { 0 },
            );
            ix.give_frame(frame);
            let again = ix.take_frame();
            assert!(again.is_fresh(), "a given frame is emptied");
            assert_eq!(
                again.has_store(),
                store,
                "a reused frame keeps the run's store"
            );
            if store {
                assert_eq!(
                    taints(&again),
                    Vec::<u8>::new(),
                    "its taints go with its bindings"
                );
            }
        }
    }

    /// A taint read from an Env without a store panics: a missing store never
    /// reads as 0.
    #[test]
    #[should_panic(expected = "no taint store")]
    fn a_taint_read_without_a_store_panics() {
        let (prog, _) = sealed_prog("r51-s22-nostore");
        let unsealed = Interp::build_with(&prog, &[], Engine::Tree);
        let mut env = unsealed.new_env();
        env.define(intern("s22_y"), Value::Int(1), 0);
        let i = env.get_var_idx(intern("s22_y"), 0).expect("bound");
        let _ = env.taint(i);
    }

    /// A sealed run handed a store-less snapshot stops rather than run the
    /// handler arm with every taint dropped.
    #[test]
    #[should_panic(expected = "does not match the run")]
    fn a_storeless_snapshot_in_a_sealed_run_panics() {
        let (prog, sdir) = sealed_prog("r51-s22-snap");
        let sealed = Interp::build_with(&prog, std::slice::from_ref(&sdir), Engine::Tree);
        let unsealed = Interp::build_with(&prog, &[], Engine::Tree);
        let snap = unsealed.new_env().snapshot();
        let _ = sealed.env_from_snapshot(snap);
    }
}
