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

use super::{EnumVal, Fields, IntWidth, Value};
use crate::ast::{
    EnumDef, Expr, FnDef, HandlerExpr, Item, LambdaParam, Literal, Pattern, Program, TypeDef,
    TypeField,
};

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

/// Names the interpreter itself binds or builds records with, interned first
/// so their syms are constants (see the `SYM_*` consts, which index this list).
const PREDEFINED: [&str; 7] = [
    "_",
    "goal_met",
    "value",
    "confidence",
    "source_tag",
    "Uncertain",
    "Temporal",
];
pub(super) const SYM_UNDERSCORE: Sym = Sym(0);
pub(super) const SYM_GOAL_MET: Sym = Sym(1);
pub(super) const SYM_VALUE: Sym = Sym(2);
pub(super) const SYM_CONFIDENCE: Sym = Sym(3);
pub(super) const SYM_SOURCE_TAG: Sym = Sym(4);
pub(super) const SYM_UNCERTAIN: Sym = Sym(5);
pub(super) const SYM_TEMPORAL: Sym = Sym(6);

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

/// The sym of `name` if it has been interned, without interning it.
pub(super) fn lookup_sym(name: &str) -> Option<Sym> {
    INTERNER.with(|cell| cell.borrow().ids.get(name).copied())
}

/// The name `s` was interned from.
pub(super) fn sym_name(s: Sym) -> Rc<str> {
    INTERNER.with(|cell| Rc::clone(&cell.borrow().names[s.index()]))
}

/// Whether `s` was interned from `name`, without allocating.
pub(super) fn sym_is(s: Sym, name: &str) -> bool {
    INTERNER.with(|cell| &*cell.borrow().names[s.index()] == name)
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

/// Payload tags: the top two bits say what the low 30 index. Untagged, the
/// payload is a sym.
const TAG_MASK: u32 = 3 << 30;
/// Indexes `Resolution::lambdas`.
const LAMBDA_TAG: u32 = 1 << 30;
/// Indexes `Resolution::records` (a struct literal or a struct pattern).
const RECORD_TAG: u32 = 2 << 30;
/// Indexes `Resolution::strs` (a string literal).
const STR_TAG: u32 = 3 << 30;

/// What a struct literal or struct pattern names. A `::`-qualified name is
/// an enum variant, any other a struct.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RecordKind {
    Struct(Sym),
    Enum(Sym, Sym),
}

impl RecordKind {
    fn of(name: &str) -> RecordKind {
        match name.split_once("::") {
            Some((e, v)) => RecordKind::Enum(intern(e), intern(v)),
            None => RecordKind::Struct(intern(name)),
        }
    }

    /// Whether `v` is a record of this kind; its fields if so.
    #[inline]
    pub(super) fn fields_of(self, v: &Value) -> Option<&Fields> {
        match (self, v) {
            (RecordKind::Struct(n), Value::Struct(s)) if s.name == n => Some(&s.fields),
            (RecordKind::Enum(e, var), Value::Enum(x)) if x.enum_name == e && x.variant == var => {
                Some(&x.fields)
            }
            _ => None,
        }
    }
}

/// A struct literal node's resolution (AX-47): everything construction needs
/// that does not depend on the field values, so building a record is one
/// allocation for the fields and one for the record.
pub(super) struct RecordLit {
    pub(super) kind: RecordKind,
    /// The field names of the value, in storage order: the declaring
    /// definition's order for the fields the literal gives, then any the
    /// literal gives that the definition does not declare. A repeated field
    /// appears once (the last value wins, as a map insert did).
    pub(super) names: Box<[Sym]>,
    /// Per literal field, in source (= evaluation) order: its index in
    /// `names` and the width to coerce its value to (R19: a non-i64 integer
    /// field of a declared struct).
    pub(super) slots: Box<[(u32, Option<IntWidth>)]>,
    /// `slots[i].0 == i` for every field: the values can be pushed in turn.
    pub(super) in_order: bool,
    /// A field-less literal's value (a unit variant, `S {}`), built once and
    /// shared: records are immutable unless written through `Rc::make_mut`.
    /// `None` for a struct with a whole-struct refinement, which is checked
    /// at every construction.
    pub(super) empty: Option<Value>,
}

impl RecordLit {
    /// Resolve the literal `name { fields }` against the program's types.
    pub(super) fn of(
        name: &str,
        fields: &[(String, Expr)],
        structs: &HashMap<String, &TypeDef>,
        enums: &HashMap<String, &EnumDef>,
    ) -> RecordLit {
        let kind = RecordKind::of(name);
        let declared: Option<&[TypeField]> = match name.split_once("::") {
            Some((e, v)) => enums
                .get(e)
                .and_then(|d| d.variants.iter().find(|x| x.name == v))
                .map(|x| x.fields.as_slice()),
            None => structs.get(name).map(|d| d.fields.as_slice()),
        };
        let given = |n: &str| fields.iter().any(|(f, _)| f == n);
        let mut names: Vec<Sym> = Vec::with_capacity(fields.len());
        for tf in declared.unwrap_or(&[]) {
            let s = intern(&tf.name);
            if given(&tf.name) && !names.contains(&s) {
                names.push(s);
            }
        }
        for (f, _) in fields {
            let s = intern(f);
            if !names.contains(&s) {
                names.push(s);
            }
        }
        // Only a struct's declared fields coerce; an enum variant's never did.
        let widths = structs.get(name).map(|d| d.fields.as_slice());
        let slots: Box<[(u32, Option<IntWidth>)]> = fields
            .iter()
            .map(|(f, _)| {
                let s = intern(f);
                let idx = names
                    .iter()
                    .position(|&n| n == s)
                    .expect("every given field has a slot");
                let width = widths
                    .and_then(|ds| ds.iter().find(|d| &d.name == f))
                    .and_then(|d| super::axon_type_to_width(&d.ty));
                (u32::try_from(idx).expect("fewer than 2^32 fields"), width)
            })
            .collect();
        let in_order = slots.len() == names.len()
            && slots.iter().enumerate().all(|(i, &(s, _))| s as usize == i);
        let refined = structs.get(name).is_some_and(|d| d.refinement.is_some());
        let empty = (fields.is_empty() && !refined).then(|| match kind {
            RecordKind::Struct(n) => Value::record(n, Fields::default()),
            RecordKind::Enum(enum_name, variant) => Value::Enum(Rc::new(EnumVal {
                enum_name,
                variant,
                fields: Fields::default(),
            })),
        });
        RecordLit {
            kind,
            names: names.into_boxed_slice(),
            slots,
            in_order,
            empty,
        }
    }
}

/// A struct pattern node's resolution: what it names and its field syms, in
/// the pattern's order.
pub(super) struct RecordPat {
    pub(super) kind: RecordKind,
    pub(super) fields: Box<[Sym]>,
}

impl RecordPat {
    /// Resolve the pattern `name { fields }`.
    pub(super) fn of(name: &str, fields: &[(String, Pattern)]) -> RecordPat {
        RecordPat {
            kind: RecordKind::of(name),
            fields: fields.iter().map(|(f, _)| intern(f)).collect(),
        }
    }
}

/// A [`Resolution::records`] entry.
enum Record {
    Lit(RecordLit),
    Pat(RecordPat),
}

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
    records: Vec<Record>,
    /// String literals' values, made once (AX-47): evaluating one clones an
    /// `Rc` instead of allocating and copying the text.
    strs: Vec<Rc<String>>,
}

impl Resolution {
    /// Resolve every name-bearing node of `program`, and of the lambda bodies
    /// this table clones and owns, against the program's struct and enum
    /// definitions (the interpreter's own maps, so both agree on which
    /// definition a name means).
    pub(super) fn build(
        program: &Program,
        structs: &HashMap<String, &TypeDef>,
        enums: &HashMap<String, &EnumDef>,
    ) -> Resolution {
        let mut b = Builder {
            structs,
            enums,
            entries: Vec::new(),
            lambdas: Vec::new(),
            records: Vec::new(),
            strs: Vec::new(),
        };
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
            records: b.records,
            strs: b.strs,
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

    /// The payload index of node `key` if it carries tag `tag`.
    #[inline]
    fn tagged(&self, key: usize, tag: u32) -> Option<usize> {
        match self.get(key) {
            Some(p) if p & TAG_MASK == tag => Some((p & !TAG_MASK) as usize),
            _ => None,
        }
    }

    /// The sym of the name node `e` carries (`name`, passed by the caller,
    /// which has already matched the node). Falls back to interning `name`
    /// for a node outside the table.
    #[inline]
    pub(super) fn sym(&self, e: &Expr, name: &str) -> Sym {
        match self.tagged(e as *const Expr as usize, 0) {
            Some(s) => Sym(s as u32),
            None => intern(name),
        }
    }

    /// [`Resolution::sym`] for a binding pattern `Pattern::Ident(name)`.
    #[inline]
    pub(super) fn pat_sym(&self, p: &Pattern, name: &str) -> Sym {
        match self.tagged(p as *const Pattern as usize, 0) {
            Some(s) => Sym(s as u32),
            None => intern(name),
        }
    }

    /// The resolution of lambda node `e`, if it is in the table.
    #[inline]
    pub(super) fn lambda(&self, e: &Expr) -> Option<&LambdaInfo> {
        self.tagged(e as *const Expr as usize, LAMBDA_TAG)
            .map(|i| &self.lambdas[i])
    }

    /// The resolution of struct literal node `e`, if it is in the table.
    #[inline]
    pub(super) fn record_lit(&self, e: &Expr) -> Option<&RecordLit> {
        match self.tagged(e as *const Expr as usize, RECORD_TAG) {
            Some(i) => match &self.records[i] {
                Record::Lit(r) => Some(r),
                Record::Pat(_) => None,
            },
            None => None,
        }
    }

    /// The resolution of struct pattern node `p`, if it is in the table.
    #[inline]
    pub(super) fn record_pat(&self, p: &Pattern) -> Option<&RecordPat> {
        match self.tagged(p as *const Pattern as usize, RECORD_TAG) {
            Some(i) => match &self.records[i] {
                Record::Pat(r) => Some(r),
                Record::Lit(_) => None,
            },
            None => None,
        }
    }

    /// The value of string literal node `e`, if it is in the table.
    #[inline]
    pub(super) fn str_lit(&self, e: &Expr) -> Option<&Rc<String>> {
        self.tagged(e as *const Expr as usize, STR_TAG)
            .map(|i| &self.strs[i])
    }
}

#[inline]
fn slot_of(key: usize, shift: u32) -> usize {
    ((key as u64).wrapping_mul(FX_K) >> shift) as usize
}

struct Builder<'a, 'd> {
    structs: &'a HashMap<String, &'d TypeDef>,
    enums: &'a HashMap<String, &'d EnumDef>,
    entries: Vec<(usize, u32)>,
    lambdas: Vec<LambdaInfo>,
    records: Vec<Record>,
    strs: Vec<Rc<String>>,
}

impl Builder<'_, '_> {
    fn add_fn(&mut self, f: &FnDef) {
        self.add_root(&f.body);
        if let Some(v) = &f.verify {
            self.add_root(&v.predicate);
        }
    }

    /// Record the name nodes, string literals and struct literals under
    /// `root`; give each outermost lambda under it its own cloned code, and
    /// resolve that clone the same way. A lambda nested in another one runs
    /// only from its parent's clone, so cloning it again here would only make
    /// the clones nest exponentially. Match guards (which `walk_expr` does not
    /// enter) are resolved as roots of their own.
    fn add_root(&mut self, root: &Expr) {
        let mut lambdas: Vec<&Expr> = Vec::new();
        let mut guards: Vec<&Expr> = Vec::new();
        let mut patterns: Vec<&Pattern> = Vec::new();
        let mut records: Vec<&Expr> = Vec::new();
        crate::ast::walk_expr(root, &mut |e| {
            let name = match e {
                Expr::Ident(n)
                | Expr::Let { name: n, .. }
                | Expr::Own { name: n, .. }
                | Expr::RefBind { name: n, .. }
                | Expr::Assign { name: n, .. }
                | Expr::For { var: n, .. }
                | Expr::FieldAccess { field: n, .. } => n,
                Expr::Literal(Literal::Str(s)) => {
                    let idx = tagged_index(self.strs.len(), STR_TAG);
                    self.strs.push(Rc::new(s.clone()));
                    self.entries.push((e as *const Expr as usize, idx));
                    return;
                }
                Expr::StructLit { .. } => {
                    records.push(e);
                    return;
                }
                Expr::Lambda { .. } => {
                    lambdas.push(e);
                    return;
                }
                Expr::Match { arms, .. } => {
                    for a in arms {
                        patterns.push(&a.pattern);
                        guards.extend(a.guard.as_ref());
                    }
                    return;
                }
                Expr::WhileLet { pattern, .. } => {
                    patterns.push(pattern);
                    return;
                }
                Expr::WithHandler { handler, .. } => {
                    if let HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                        for a in arms.iter().chain(return_arm.as_deref()) {
                            patterns.push(&a.binding);
                        }
                    }
                    return;
                }
                _ => return,
            };
            self.entries
                .push((e as *const Expr as usize, table_sym(name)));
        });
        for p in patterns {
            self.add_pattern(p);
        }
        for e in records {
            let Expr::StructLit { name, fields } = e else {
                unreachable!("collected as a struct literal")
            };
            let idx = tagged_index(self.records.len(), RECORD_TAG);
            let lit = RecordLit::of(name, fields, self.structs, self.enums);
            self.records.push(Record::Lit(lit));
            self.entries.push((e as *const Expr as usize, idx));
        }
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
            let idx = tagged_index(self.lambdas.len(), LAMBDA_TAG);
            self.lambdas.push(info);
            self.entries.push((key, idx));
        }
    }

    /// Record every binding `Pattern::Ident` and every struct pattern in `p`.
    fn add_pattern(&mut self, p: &Pattern) {
        let key = p as *const Pattern as usize;
        match p {
            Pattern::Ident(n) => self.entries.push((key, table_sym(n))),
            Pattern::Some(q) | Pattern::Ok(q) | Pattern::Err(q) => self.add_pattern(q),
            Pattern::Struct { name, fields } => {
                let idx = tagged_index(self.records.len(), RECORD_TAG);
                self.records.push(Record::Pat(RecordPat::of(name, fields)));
                self.entries.push((key, idx));
                fields.iter().for_each(|(_, q)| self.add_pattern(q));
            }
            Pattern::Tuple(ps) => ps.iter().for_each(|q| self.add_pattern(q)),
            Pattern::Wildcard | Pattern::Literal(_) | Pattern::None => {}
        }
    }
}

/// `intern(name)` as a table payload (which must carry no tag).
fn table_sym(name: &str) -> u32 {
    let s = intern(name);
    assert!(s.0 & TAG_MASK == 0, "fewer than 2^30 distinct names");
    s.0
}

/// Index `i` of a side table as a payload tagged `tag`.
fn tagged_index(i: usize, tag: u32) -> u32 {
    let i = u32::try_from(i).expect("fewer than 2^30 table entries");
    assert!(i & TAG_MASK == 0, "fewer than 2^30 table entries");
    i | tag
}
