//! Interned names and the interpreter's per-node name resolution (AX-18).
//!
//! Variables are bound and looked up by [`Sym`], an interned `u32`, so an
//! environment scan compares integers instead of strings. The sym of the name
//! an AST node carries comes from a [`Resolution`] table built once per
//! interpreter: it maps the ADDRESS of every name-bearing node to its sym, so
//! the evaluator gets a node's sym with one integer-keyed probe and no string
//! hashing or comparison.
//!
//! Address keys are sound because every node in the table is pinned for the
//! life of the [`super::Interp`] that owns the table:
//! * nodes of the `&'p Program` the interpreter was built from — borrowed for
//!   `'p`, so never moved, mutated or freed while the interpreter exists;
//! * nodes of the lambda bodies the table itself owns (each lambda body is
//!   cloned once, into an `Rc<ClosureCode>` held by the table).
//!
//! A node that is NOT in the table (an AST clone made at run time: a handler
//! arm, a `SendValue` closure, a fn-value forwarder) cannot share an address
//! with a pinned node, since both are live allocations. Its lookup misses and
//! falls back to interning the name, so a hit never trusts a reused address.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::Rc;

use crate::ast::{Expr, FnDef, HandlerExpr, Item, LambdaParam, Pattern, Program};

/// An interned identifier: equal names have equal syms, for the life of the
/// thread. Values are `!Send`, so an interpreter never crosses threads and a
/// thread-local interner is shared by every interpreter that can exchange
/// values.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Sym(u32);

impl Sym {
    pub(super) fn index(self) -> usize {
        self.0 as usize
    }
}

/// Names the interpreter itself binds, interned first so their syms are
/// constants (see the `SYM_*` consts, which index this list).
const PREDEFINED: [&str; 5] = ["_", "goal_met", "value", "confidence", "source_tag"];
pub(super) const SYM_UNDERSCORE: Sym = Sym(0);
pub(super) const SYM_GOAL_MET: Sym = Sym(1);
pub(super) const SYM_VALUE: Sym = Sym(2);
pub(super) const SYM_CONFIDENCE: Sym = Sym(3);
pub(super) const SYM_SOURCE_TAG: Sym = Sym(4);

struct Interner {
    ids: FxHashMap<Rc<str>, Sym>,
    names: Vec<Rc<str>>,
}

impl Interner {
    fn new() -> Self {
        let mut i = Interner {
            ids: FxHashMap::default(),
            names: Vec::new(),
        };
        for name in PREDEFINED {
            i.insert(name);
        }
        i
    }

    fn insert(&mut self, name: &str) -> Sym {
        let s = Sym(u32::try_from(self.names.len()).expect("fewer than 2^32 distinct names"));
        let rc: Rc<str> = Rc::from(name);
        self.names.push(Rc::clone(&rc));
        self.ids.insert(rc, s);
        s
    }
}

thread_local! {
    static INTERNER: RefCell<Interner> = RefCell::new(Interner::new());
}

/// The sym of `name`, interning it on first sight.
pub(super) fn intern(name: &str) -> Sym {
    INTERNER.with(|cell| {
        let mut i = cell.borrow_mut();
        match i.ids.get(name) {
            Some(&s) => s,
            None => i.insert(name),
        }
    })
}

/// The name `s` was interned from.
pub(super) fn sym_name(s: Sym) -> Rc<str> {
    INTERNER.with(|cell| Rc::clone(&cell.borrow().names[s.index()]))
}

const FX_K: u64 = 0xf135_7aea_2e62_a9c5;

/// The rustc-hash (Fx) multiply-rotate hash: a few instructions per word,
/// against SipHash's ~100 for a short key. Not DoS-resistant, which does not
/// matter for keys taken from the program being run.
#[derive(Default, Clone, Copy)]
pub(super) struct FxHasher(u64);

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = self.0.wrapping_add(word).wrapping_mul(FX_K);
    }
}

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for w in words {
            self.add(u64::from_le_bytes(*w));
        }
        if !rest.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(buf));
        }
    }
    fn write_u8(&mut self, n: u8) {
        self.add(u64::from(n));
    }
    fn write_u32(&mut self, n: u32) {
        self.add(u64::from(n));
    }
    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }
    fn finish(&self) -> u64 {
        self.0.rotate_left(26)
    }
}

pub(super) type FxHashMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

/// The code of a closure: its parameters and body. Shared (`Rc`) by every
/// closure value made from the same lambda, so creating or cloning a closure
/// never copies its body.
#[derive(Debug)]
pub struct ClosureCode {
    pub(super) params: Box<[Sym]>,
    pub(super) body: Expr,
}

/// A lambda node's resolution: its shared code, and the names its body may
/// read or assign from the defining scope (its free variables, nested lambdas
/// included). Creating the closure captures exactly those of them that are
/// bound in the defining environment (AX-40); a free name that is not bound
/// there is a global, a fn or a builtin, reached the same way at call time.
pub(super) struct LambdaInfo {
    pub(super) code: Rc<ClosureCode>,
    pub(super) free: Box<[Sym]>,
}

impl LambdaInfo {
    /// Resolve a lambda node that is not in a [`Resolution`] table.
    pub(super) fn of(params: &[LambdaParam], body: &Expr) -> LambdaInfo {
        LambdaInfo {
            code: Rc::new(ClosureCode {
                params: params.iter().map(|p| intern(&p.name)).collect(),
                body: body.clone(),
            }),
            free: free_vars(params, body),
        }
    }
}

/// The free variables of `|params| body`, in sym order.
fn free_vars(params: &[LambdaParam], body: &Expr) -> Box<[Sym]> {
    let bound: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
    let mut free = HashSet::new();
    crate::resolver::collect_free_vars(body, &bound, &mut free);
    let mut syms: Vec<Sym> = free.iter().map(|n| intern(n)).collect();
    syms.sort_unstable();
    syms.into_boxed_slice()
}

/// A user fn or impl method, with its parameter syms resolved once.
pub(super) struct FnEntry<'p> {
    pub(super) def: &'p FnDef,
    pub(super) params: Box<[Sym]>,
    /// Some parameter is `&mut` (see `Interp::call_fn`).
    pub(super) has_ref_mut: bool,
}

impl<'p> FnEntry<'p> {
    pub(super) fn new(def: &'p FnDef) -> Self {
        FnEntry {
            def,
            params: def.params.iter().map(|p| intern(&p.name)).collect(),
            has_ref_mut: def
                .params
                .iter()
                .any(|p| matches!(p.ty, crate::ast::AxonType::RefMut(_))),
        }
    }
}

/// `Interp::callees` entries: not resolved yet / proven to name no builtin
/// and no user fn / `CALLEE_FN + i` names `fn_table[i]`.
pub(super) const CALLEE_UNKNOWN: u32 = 0;
pub(super) const CALLEE_NOT_FN: u32 = 1;
pub(super) const CALLEE_FN: u32 = 2;

/// Payload tag: the low 31 bits index `Resolution::lambdas` instead of
/// holding a sym.
const LAMBDA_TAG: u32 = 1 << 31;

/// The frozen node-address → resolution table (see the module docs).
///
/// Open addressing with linear probing over a power-of-two slot array at most
/// half full; address 0 marks an empty slot (no node lives there). Built once
/// and never mutated, so a lookup is a multiply, a shift and usually one
/// compare, with no `RefCell` borrow.
pub(super) struct Resolution {
    slots: Box<[(usize, u32)]>,
    shift: u32,
    lambdas: Vec<LambdaInfo>,
}

impl Resolution {
    /// Resolve every name-bearing node of `program`, and of the lambda bodies
    /// this table clones and owns.
    pub(super) fn build(program: &Program) -> Resolution {
        let mut b = Builder::default();
        for item in &program.items {
            match item {
                Item::FnDef(f) => b.add_fn(f),
                Item::ImplBlock(blk) => blk.methods.iter().for_each(|m| b.add_fn(m)),
                Item::LetDef { value, .. } => b.add_root(value),
                Item::RefineDef(r) => b.add_root(&r.predicate),
                Item::TypeDef(t) => {
                    if let Some(p) = &t.refinement {
                        b.add_root(p);
                    }
                }
                Item::EnumDef(_) | Item::ModDecl(_) | Item::UseDecl(_) | Item::TraitDef(_) => {}
            }
        }
        let cap = (b.entries.len() * 2).next_power_of_two().max(16);
        let mut slots = vec![(0usize, 0u32); cap].into_boxed_slice();
        let shift = 64 - cap.trailing_zeros();
        for (key, payload) in b.entries {
            let mut i = slot_of(key, shift);
            while slots[i].0 != 0 {
                debug_assert!(slots[i].0 != key, "every node is resolved once");
                i = (i + 1) & (cap - 1);
            }
            slots[i] = (key, payload);
        }
        Resolution {
            slots,
            shift,
            lambdas: b.lambdas,
        }
    }

    #[inline]
    fn get(&self, key: usize) -> Option<u32> {
        let mask = self.slots.len() - 1;
        let mut i = slot_of(key, self.shift);
        loop {
            let (k, payload) = self.slots[i];
            if k == key {
                return Some(payload);
            }
            if k == 0 {
                return None;
            }
            i = (i + 1) & mask;
        }
    }

    /// The sym of the name node `e` carries (`name`, passed by the caller,
    /// which has already matched the node). Falls back to interning `name`
    /// for a node outside the table.
    #[inline]
    pub(super) fn sym(&self, e: &Expr, name: &str) -> Sym {
        match self.get(e as *const Expr as usize) {
            Some(p) if p & LAMBDA_TAG == 0 => Sym(p),
            _ => intern(name),
        }
    }

    /// [`Resolution::sym`] for a binding pattern `Pattern::Ident(name)`.
    #[inline]
    pub(super) fn pat_sym(&self, p: &Pattern, name: &str) -> Sym {
        match self.get(p as *const Pattern as usize) {
            Some(s) if s & LAMBDA_TAG == 0 => Sym(s),
            _ => intern(name),
        }
    }

    /// The resolution of lambda node `e`, if it is in the table.
    #[inline]
    pub(super) fn lambda(&self, e: &Expr) -> Option<&LambdaInfo> {
        match self.get(e as *const Expr as usize) {
            Some(p) if p & LAMBDA_TAG != 0 => Some(&self.lambdas[(p & !LAMBDA_TAG) as usize]),
            _ => None,
        }
    }
}

#[inline]
fn slot_of(key: usize, shift: u32) -> usize {
    ((key as u64).wrapping_mul(FX_K) >> shift) as usize
}

#[derive(Default)]
struct Builder {
    entries: Vec<(usize, u32)>,
    lambdas: Vec<LambdaInfo>,
}

impl Builder {
    fn add_fn(&mut self, f: &FnDef) {
        self.add_root(&f.body);
        if let Some(v) = &f.verify {
            self.add_root(&v.predicate);
        }
    }

    /// Record the name nodes under `root`; give each outermost lambda under it
    /// its own cloned code, and resolve that clone the same way. A lambda
    /// nested in another one runs only from its parent's clone, so cloning it
    /// again here would only make the clones nest exponentially. Match guards
    /// (which `walk_expr` does not enter) are resolved as roots of their own.
    fn add_root(&mut self, root: &Expr) {
        let mut lambdas: Vec<&Expr> = Vec::new();
        let mut guards: Vec<&Expr> = Vec::new();
        crate::ast::walk_expr(root, &mut |e| {
            let name = match e {
                Expr::Ident(n)
                | Expr::Let { name: n, .. }
                | Expr::Own { name: n, .. }
                | Expr::RefBind { name: n, .. }
                | Expr::Assign { name: n, .. }
                | Expr::For { var: n, .. } => n,
                Expr::Lambda { .. } => {
                    lambdas.push(e);
                    return;
                }
                Expr::Match { arms, .. } => {
                    for a in arms {
                        add_pattern(&mut self.entries, &a.pattern);
                        guards.extend(a.guard.as_ref());
                    }
                    return;
                }
                Expr::WhileLet { pattern, .. } => {
                    add_pattern(&mut self.entries, pattern);
                    return;
                }
                Expr::WithHandler { handler, .. } => {
                    if let HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                        for a in arms.iter().chain(return_arm.as_deref()) {
                            add_pattern(&mut self.entries, &a.binding);
                        }
                    }
                    return;
                }
                _ => return,
            };
            self.entries
                .push((e as *const Expr as usize, table_sym(name)));
        });
        for g in guards {
            self.add_root(g);
        }
        let mut nested: HashSet<usize> = HashSet::new();
        for l in &lambdas {
            if let Expr::Lambda { body, .. } = l {
                crate::ast::walk_expr(body, &mut |e| {
                    if matches!(e, Expr::Lambda { .. }) {
                        nested.insert(e as *const Expr as usize);
                    }
                });
            }
        }
        for l in lambdas {
            let key = l as *const Expr as usize;
            if nested.contains(&key) {
                continue;
            }
            let Expr::Lambda { params, body, .. } = l else {
                unreachable!("collected as a lambda")
            };
            let info = LambdaInfo::of(params, body);
            self.add_root(&info.code.body);
            let idx = u32::try_from(self.lambdas.len()).expect("fewer than 2^31 lambdas");
            assert!(idx & LAMBDA_TAG == 0, "fewer than 2^31 lambdas");
            self.lambdas.push(info);
            self.entries.push((key, idx | LAMBDA_TAG));
        }
    }
}

/// `intern(name)` as a table payload (which must not carry `LAMBDA_TAG`).
fn table_sym(name: &str) -> u32 {
    let s = intern(name);
    assert!(s.0 & LAMBDA_TAG == 0, "fewer than 2^31 distinct names");
    s.0
}

/// Record every binding `Pattern::Ident` in `p`.
fn add_pattern(entries: &mut Vec<(usize, u32)>, p: &Pattern) {
    match p {
        Pattern::Ident(n) => entries.push((p as *const Pattern as usize, table_sym(n))),
        Pattern::Some(q) | Pattern::Ok(q) | Pattern::Err(q) => add_pattern(entries, q),
        Pattern::Struct { fields, .. } => fields.iter().for_each(|(_, q)| add_pattern(entries, q)),
        Pattern::Tuple(ps) => ps.iter().for_each(|q| add_pattern(entries, q)),
        Pattern::Wildcard | Pattern::Literal(_) | Pattern::None => {}
    }
}
