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
//!
//! Local variables are also resolved to frame SLOTS here (AX-53). Every fn
//! body and every lambda body cloned into the table is a frame; each binding
//! it introduces (a parameter, `goal_met`, `let`/`own`/`ref`, a `for` or
//! pattern binder, a captured variable) gets the index the binding occupies
//! in the frame's `Env` at run time, and each name node that reads or assigns
//! a local gets the slot of the binding it sees. The allocation mirrors the
//! evaluator's scopes exactly: a block, a loop iteration, a match arm and a
//! `while let`/`for` binder each push a scope whose slots start where the
//! enclosing scope's live slots end, and are released when it pops, so a
//! scope's slots are always at or above the env's length when it is entered.
//! The env stores each binding's sym beside its value, so a slot access is
//! checked against the expected sym, and any miss falls back to the by-name
//! scan — a slot is a fast path, never a second source of truth.
//!
//! Code that runs in an env not laid out by this table keeps the by-name
//! lookup ([`NAMED`]): module-level `let` initialisers, refinement and
//! `@[verify]` predicates (evaluated in synthetic envs), handler arms (run on
//! a snapshot of the defining env), and every node outside the table.

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
/// The sym of an env slot that holds no binding (a slot below one defined out
/// of order; see `Env::define_var`). Never interned, so it equals no name.
pub(super) const SYM_NONE: Sym = Sym(u32::MAX);

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

/// Slot of a name node that is resolved by name at run time: the env is
/// scanned for the innermost binding of its sym (see the module docs).
pub(super) const NAMED: u32 = u32::MAX;
/// Slot of a name node in a slotted frame that names no local of the frame (a
/// global, a fn, a builtin): the env holds no binding of it, so lookups skip
/// the env entirely.
pub(super) const NOT_LOCAL: u32 = u32::MAX - 1;

/// A frame's static scope chain while its body is resolved: per scope, the
/// slot it starts at and its bindings in binding order. Mirrors the `Env`
/// the body runs in (see the module docs).
struct Frame {
    scopes: Vec<(u32, Vec<(Sym, u32)>)>,
    next: u32,
}

impl Frame {
    /// A frame whose base scope binds `names` to slots `0..`, in order.
    fn with_base(names: impl IntoIterator<Item = Sym>) -> Frame {
        let mut f = Frame {
            scopes: vec![(0, Vec::new())],
            next: 0,
        };
        names.into_iter().for_each(|s| f.bind_positional(s));
        f
    }

    fn push(&mut self) {
        self.scopes.push((self.next, Vec::new()));
    }

    fn pop(&mut self) {
        let (start, _) = self.scopes.pop().expect("balanced scopes");
        self.next = start;
    }

    /// The slot of the innermost visible binding of `s`.
    fn lookup(&self, s: Sym) -> u32 {
        for (_, names) in self.scopes.iter().rev() {
            if let Some(&(_, slot)) = names.iter().rev().find(|(k, _)| *k == s) {
                return slot;
            }
        }
        NOT_LOCAL
    }

    /// Bind `s` in the current scope the way `Env::define` does: re-binding
    /// a name of the same scope reuses its slot.
    fn bind(&mut self, s: Sym) -> u32 {
        let (_, names) = self.scopes.last_mut().expect("a frame has a scope");
        if let Some(&(_, slot)) = names.iter().find(|(k, _)| *k == s) {
            return slot;
        }
        let slot = self.next;
        names.push((s, slot));
        self.next += 1;
        slot
    }

    /// Bind `s` to the next slot even if the scope binds it already: the
    /// evaluator stores parameters (and `goal_met`) by position.
    fn bind_positional(&mut self, s: Sym) {
        let (_, names) = self.scopes.last_mut().expect("a frame has a scope");
        names.push((s, self.next));
        self.next += 1;
    }
}

fn lookup(fr: &Option<Frame>, s: Sym) -> u32 {
    fr.as_ref().map_or(NAMED, |f| f.lookup(s))
}

fn bind(fr: &mut Option<Frame>, s: Sym) -> u32 {
    fr.as_mut().map_or(NAMED, |f| f.bind(s))
}

fn push(fr: &mut Option<Frame>) {
    if let Some(f) = fr {
        f.push();
    }
}

fn pop(fr: &mut Option<Frame>) {
    if let Some(f) = fr {
        f.pop();
    }
}

/// The code of a closure: its parameters and body. Shared (`Rc`) by every
/// closure value made from the same lambda, so creating or cloning a closure
/// never copies its body. Only ever built inside an `Rc` and never moved out
/// of it, so the nodes of `body` stay put for as long as the code lives (the
/// resolution table and `compiled` rely on that).
pub struct ClosureCode {
    pub(super) params: Box<[Sym]>,
    pub(super) body: Expr,
    /// The slot of the first parameter when `body` is resolved as a slotted
    /// frame (it follows the captured variables, slots `0..`); `None` when it
    /// is resolved by name.
    pub(super) param_base: Option<u32>,
    /// R50 S4: the body compiled for the bytecode engine, filled on its
    /// first run under `AXON_ENGINE=vm`. `Some` only for the codes the
    /// resolution table builds ([`Resolution::lambda`]); `None` for
    /// [`LambdaInfo::of`] codes, `fn_value` forwarders and `SendValue` deep
    /// copies, whose bodies run on the tree-walker.
    pub(super) compiled: Option<super::vm::LambdaBody>,
    /// R50 S4: `vm: tree <anon>: unresolved lambda` was printed for this
    /// code (one with `compiled: None`; spec §3, once per code instance).
    pub(super) traced: std::cell::Cell<bool>,
}

impl ClosureCode {
    /// The code of a closure whose body runs on the tree-walker under both
    /// engines (`compiled: None`).
    pub(super) fn unresolved(params: Box<[Sym]>, body: Expr) -> ClosureCode {
        ClosureCode {
            params,
            body,
            param_base: None,
            compiled: None,
            traced: std::cell::Cell::new(false),
        }
    }
}

// Written out so the R50 fields leave the output as it was.
impl std::fmt::Debug for ClosureCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClosureCode")
            .field("params", &self.params)
            .field("body", &self.body)
            .field("param_base", &self.param_base)
            .finish()
    }
}

/// A lambda node's resolution: its shared code, and the variables of the
/// defining scope its body may read or assign (its free variables, nested
/// lambdas included) with their slots there. Creating the closure captures
/// exactly those of them that are bound in the defining environment (AX-40);
/// a free name that is not bound there is a global, a fn or a builtin,
/// reached the same way at call time. In a slotted frame only the free names
/// that are locals of the frame are listed, and they become the closure
/// frame's slots `0..` in this order.
pub(super) struct LambdaInfo {
    pub(super) code: Rc<ClosureCode>,
    pub(super) captures: Box<[(Sym, u32)]>,
}

impl LambdaInfo {
    /// Resolve a lambda node that is not in a [`Resolution`] table.
    pub(super) fn of(params: &[LambdaParam], body: &Expr) -> LambdaInfo {
        LambdaInfo {
            code: Rc::new(ClosureCode::unresolved(
                params.iter().map(|p| intern(&p.name)).collect(),
                body.clone(),
            )),
            captures: free_vars(params, body)
                .iter()
                .map(|&s| (s, NAMED))
                .collect(),
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

/// A user fn or impl method, with everything a call decides from its
/// declaration resolved once (AX-18, AX-54): parameter syms, per-parameter
/// argument coercion and the attribute-driven per-activation steps.
pub(super) struct FnEntry<'p> {
    pub(super) def: &'p FnDef,
    pub(super) params: Box<[Sym]>,
    /// Per parameter: whether its declared type is NOT `Uncertain`/`Temporal`
    /// (so a soft argument unwraps to its inner value), and the sized-int
    /// width an argument is coerced to (R19).
    pub(super) param_coerce: Box<[(bool, Option<IntWidth>)]>,
    /// Some parameter is `&mut` (see `Interp::call_fn`).
    pub(super) has_ref_mut: bool,
    /// `@[agent]`: the enclosing agent for everything it calls (R4/I-13).
    pub(super) is_agent: bool,
    /// The fn the `current_fn` readers find under this name carries
    /// `@[ai(...)]`, so an `ai_complete` in this activation may meter against
    /// `Interp::ai_calls_this_fn` (R3c). Without it the counter is never read
    /// or written while this fn is current, so its save/reset/restore is inert.
    pub(super) ai_metered: bool,
    /// `@[corrigible]` (R9).
    pub(super) corrigible: bool,
    /// `@[adaptive]` (R4 zone).
    pub(super) adaptive: bool,
    /// `@[experiment(label)]`'s label (R4 zone).
    pub(super) experiment: Option<String>,
    /// Has a `@[goal(...)]` attribute (R5).
    pub(super) has_goal: bool,
    /// The declared return type is a plain scalar (`i64`/`i32`/`f64`/`bool`),
    /// so a soft result unwraps at the return boundary.
    pub(super) ret_is_scalar: bool,
    /// Named `main` (the binding-dump capture applies to it).
    pub(super) is_main: bool,
    /// No attribute-driven step around the body: not `@[agent]`, `@[ai]`
    /// metered, `@[corrigible]`, zoned, `@[goal]`, `@[verify]` or `main`
    /// (`Interp::call_fn_in` takes its common path).
    pub(super) plain: bool,
    /// Has a zone (`adaptive`/`experiment`) or `@[verify]` step after the
    /// body (`Interp::finish_call_cold`).
    pub(super) has_epilogue: bool,
    /// R50: the body compiled for the bytecode engine, filled on its second
    /// run under `AXON_ENGINE=vm`, or its first when it is hot (S9,
    /// `FnEntry::hot`). `Some` only for the entries
    /// `Interp::build` puts in `fn_table`; an owned entry built per call (a
    /// def missing from `fn_of_def`) is `None` and runs on the tree-walker.
    pub(super) compiled: Option<std::cell::OnceCell<super::vm::Body<'p>>>,
    /// R50 S9: the body has been entered once, on the tree.
    pub(super) entered: std::cell::Cell<bool>,
    /// R50 S9: the body is hot on entry ([`Resolution::hot_fn`]), so it
    /// compiles on its first entry; set by `Interp::build` for table entries.
    pub(super) hot: bool,
}

impl<'p> FnEntry<'p> {
    /// `fns` is the interpreter's by-name fn map, which the `current_fn`
    /// readers (`Interp::current_ai_budget` & co.) consult.
    pub(super) fn new(def: &'p FnDef, fns: &HashMap<String, &FnDef>) -> Self {
        use crate::ast::AxonType;
        let has_attr = |name: &str| def.attrs.iter().any(|a| a.name == name);
        let mut entry = FnEntry {
            def,
            params: def.params.iter().map(|p| intern(&p.name)).collect(),
            param_coerce: def
                .params
                .iter()
                .map(|p| {
                    let soft = matches!(
                        &p.ty,
                        AxonType::Generic { base, .. } if base == "Uncertain" || base == "Temporal"
                    );
                    (!soft, super::axon_type_to_width(&p.ty))
                })
                .collect(),
            has_ref_mut: def
                .params
                .iter()
                .any(|p| matches!(p.ty, AxonType::RefMut(_))),
            is_agent: has_attr("agent"),
            ai_metered: fns
                .get(&def.name)
                .is_some_and(|f| f.attrs.iter().any(|a| a.name == "ai")),
            corrigible: has_attr("corrigible"),
            adaptive: has_attr("adaptive"),
            experiment: def
                .attrs
                .iter()
                .find(|a| a.name == "experiment")
                .map(|a| a.args.first().cloned().unwrap_or_default()),
            has_goal: has_attr("goal"),
            ret_is_scalar: matches!(
                &def.return_type,
                Some(AxonType::Named(n)) if matches!(n.as_str(), "i64" | "i32" | "f64" | "bool")
            ),
            is_main: def.name == "main",
            has_epilogue: has_attr("adaptive") || has_attr("experiment") || def.verify.is_some(),
            plain: false,
            compiled: None,
            entered: std::cell::Cell::new(false),
            hot: false,
        };
        entry.plain = !(entry.is_agent
            || entry.ai_metered
            || entry.corrigible
            || entry.adaptive
            || entry.has_goal
            || entry.is_main
            || entry.has_epilogue);
        entry
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
/// Indexes `Resolution::lits` (a string or decimal literal).
const LIT_TAG: u32 = 3 << 30;

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
/// compare, with no `RefCell` borrow. Each entry is `(node address, payload,
/// frame slot)`; the slot is only meaningful for a name node (an untagged
/// payload) and is [`NAMED`] for every other node.
pub(super) struct Resolution {
    slots: Box<[(usize, u32, u32)]>,
    shift: u32,
    lambdas: Vec<LambdaInfo>,
    records: Vec<Record>,
    /// The values of the literals whose `Value` owns an allocation (strings,
    /// AX-47; decimals, AX-55), made once: evaluating one clones an `Rc`
    /// instead of allocating (and copying the text). Int, float and bool
    /// literals are built in place by the evaluator, which is cheaper than a
    /// probe of this table.
    lits: Vec<Value>,
    /// R50 S9: the fns (by `FnDef` address, sorted) whose body is hot on
    /// entry (`vm::long_for`).
    hot_fns: Vec<usize>,
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
            loops: 0,
            hot: 0,
            hot_fns: Vec::new(),
            lambdas: Vec::new(),
            records: Vec::new(),
            lits: Vec::new(),
            owner: String::new(),
            next_lambda: 0,
        };
        // R50 §3: lambdas reached through module items are `<module>`'s,
        // numbered across those items in source order.
        let mut module_lambdas = 0;
        for item in &program.items {
            match item {
                Item::FnDef(f) => b.add_fn(f, f.name.clone()),
                Item::ImplBlock(blk) => {
                    let ty = super::type_name_of(&blk.for_type);
                    for m in &blk.methods {
                        b.add_fn(m, format!("{ty}::{}", m.name));
                    }
                }
                Item::LetDef { value, .. } => b.module_item(value, &mut module_lambdas),
                Item::RefineDef(r) => b.module_item(&r.predicate, &mut module_lambdas),
                Item::TypeDef(t) => {
                    if let Some(p) = &t.refinement {
                        b.module_item(p, &mut module_lambdas);
                    }
                }
                Item::EnumDef(_) | Item::ModDecl(_) | Item::UseDecl(_) | Item::TraitDef(_) => {}
            }
        }
        let cap = (b.entries.len() * 2).next_power_of_two().max(16);
        let mut slots = vec![(0usize, 0u32, NAMED); cap].into_boxed_slice();
        let shift = 64 - cap.trailing_zeros();
        for entry in b.entries {
            let mut i = slot_of(entry.0, shift);
            while slots[i].0 != 0 {
                debug_assert!(slots[i].0 != entry.0, "every node is resolved once");
                i = (i + 1) & (cap - 1);
            }
            slots[i] = entry;
        }
        b.hot_fns.sort_unstable();
        Resolution {
            slots,
            shift,
            lambdas: b.lambdas,
            records: b.records,
            lits: b.lits,
            hot_fns: b.hot_fns,
        }
    }

    /// R50 S9: whether `def`'s body is hot on entry: it holds a `while`, a
    /// `while let`, a `for` that [`super::vm::long_for`] calls long, or a loop
    /// inside a loop, lambda bodies inside it included.
    pub(super) fn hot_fn(&self, def: &FnDef) -> bool {
        self.hot_fns
            .binary_search(&(def as *const FnDef as usize))
            .is_ok()
    }

    #[inline]
    fn get(&self, key: usize) -> Option<(u32, u32)> {
        let mask = self.slots.len() - 1;
        let mut i = slot_of(key, self.shift);
        loop {
            let (k, payload, slot) = self.slots[i];
            if k == key {
                return Some((payload, slot));
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
            Some((p, _)) if p & TAG_MASK == tag => Some((p & !TAG_MASK) as usize),
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

    /// The sym and frame slot of the variable name node `e` carries (`name`,
    /// passed by the caller, which has already matched the node): a slot
    /// index, [`NOT_LOCAL`], or [`NAMED`] (also for a node outside the
    /// table, whose name is interned here).
    #[inline]
    pub(super) fn var(&self, e: &Expr, name: &str) -> (Sym, u32) {
        match self.get(e as *const Expr as usize) {
            Some((s, slot)) if s & TAG_MASK == 0 => (Sym(s), slot),
            _ => (intern(name), NAMED),
        }
    }

    /// [`Resolution::var`] for a binding pattern `Pattern::Ident(name)`.
    #[inline]
    pub(super) fn pat_var(&self, p: &Pattern, name: &str) -> (Sym, u32) {
        match self.get(p as *const Pattern as usize) {
            Some((s, slot)) if s & TAG_MASK == 0 => (Sym(s), slot),
            _ => (intern(name), NAMED),
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

    /// The value of string or decimal literal node `e`, if it is in the table.
    #[inline]
    pub(super) fn lit(&self, e: &Expr) -> Option<&Value> {
        self.tagged(e as *const Expr as usize, LIT_TAG)
            .map(|i| &self.lits[i])
    }
}

#[inline]
fn slot_of(key: usize, shift: u32) -> usize {
    ((key as u64).wrapping_mul(FX_K) >> shift) as usize
}

struct Builder<'a, 'd> {
    structs: &'a HashMap<String, &'d TypeDef>,
    enums: &'a HashMap<String, &'d EnumDef>,
    entries: Vec<(usize, u32, u32)>,
    /// R50 S9: loops seen so far, and those that make the body holding them
    /// hot on entry (`vm::long_for`); a body is hot when the second count
    /// grows while it is resolved.
    loops: u32,
    hot: u32,
    hot_fns: Vec<usize>,
    lambdas: Vec<LambdaInfo>,
    records: Vec<Record>,
    lits: Vec<Value>,
    /// R50 §3: the owner of the lambdas being resolved (a fn's trace name,
    /// `<fn>::verify`, `<module>`), and the 0-based source (pre-)order
    /// position of the owner's next lambda, nested ones included.
    owner: String,
    next_lambda: u32,
}

impl Builder<'_, '_> {
    /// A fn body is a slotted frame: `call_fn_in` binds the parameters by
    /// position, then `goal_met`, then evaluates the body. A `@[verify]`
    /// predicate runs in a synthetic env, so it is resolved by name. `name`
    /// is the fn's trace name (`Type::method` for an impl method).
    fn add_fn(&mut self, f: &FnDef, name: String) {
        let base = f
            .params
            .iter()
            .map(|p| intern(&p.name))
            .chain([SYM_GOAL_MET]);
        self.next_lambda = 0;
        self.owner = name;
        let hot = self.hot;
        self.expr(&f.body, &mut Some(Frame::with_base(base)));
        if self.hot > hot {
            self.hot_fns.push(f as *const FnDef as usize);
        }
        if let Some(v) = &f.verify {
            self.next_lambda = 0;
            self.owner.push_str("::verify");
            self.expr(&v.predicate, &mut None);
        }
    }

    /// A module item's expression (a module `let`, a `refine` predicate, a
    /// type's refinement), resolved by name; its lambdas are `<module>`'s,
    /// `count` being the number already seen in earlier module items.
    fn module_item(&mut self, e: &Expr, count: &mut u32) {
        "<module>".clone_into(&mut self.owner);
        self.next_lambda = *count;
        self.expr(e, &mut None);
        *count = self.next_lambda;
    }

    fn name(&mut self, e: &Expr, name: &str, slot: u32) {
        self.entries
            .push((e as *const Expr as usize, table_sym(name), slot));
    }

    fn stmts(&mut self, stmts: &[crate::ast::Stmt], fr: &mut Option<Frame>) {
        stmts.iter().for_each(|s| self.expr(&s.expr, fr));
    }

    /// `stmts` run as a loop body: one scope per iteration (`run_loop_body`).
    fn loop_body(&mut self, stmts: &[crate::ast::Stmt], fr: &mut Option<Frame>) {
        push(fr);
        self.stmts(stmts, fr);
        pop(fr);
    }

    /// Record the name nodes, string literals, struct literals and binding
    /// patterns under `e`, with the slots of the variables they bind or name
    /// when `fr` is a slotted frame (`None`: resolved by name). Scopes are
    /// pushed and bindings made where the evaluator makes them, in
    /// evaluation order. Each lambda gets its own cloned code, resolved as a
    /// frame of its own; a lambda nested in another one runs only from its
    /// parent's clone, so the original's body is not resolved.
    fn expr(&mut self, e: &Expr, fr: &mut Option<Frame>) {
        match e {
            Expr::Ident(n) => {
                let slot = lookup(fr, intern(n));
                self.name(e, n, slot);
            }
            Expr::Let { name, value, .. }
            | Expr::Own { name, value, .. }
            | Expr::RefBind { name, value, .. } => {
                self.expr(value, fr);
                let slot = bind(fr, intern(name));
                self.name(e, name, slot);
            }
            Expr::Assign { name, value } => {
                self.expr(value, fr);
                let slot = lookup(fr, intern(name));
                self.name(e, name, slot);
            }
            Expr::AssignTo { place, value } => {
                self.expr(value, fr);
                self.expr(place, fr);
            }
            Expr::FieldAccess { receiver, field } => {
                self.expr(receiver, fr);
                self.name(e, field, NAMED);
            }
            Expr::Literal(lit @ (Literal::Str(_) | Literal::Decimal(_))) => {
                let idx = tagged_index(self.lits.len(), LIT_TAG);
                self.lits.push(super::lit_to_val(lit));
                self.entries.push((e as *const Expr as usize, idx, NAMED));
            }
            Expr::Literal(_)
            | Expr::None
            | Expr::Break
            | Expr::Continue
            | Expr::InlineAsm { .. } => {}
            Expr::StructLit { name, fields } => {
                fields.iter().for_each(|(_, v)| self.expr(v, fr));
                let idx = tagged_index(self.records.len(), RECORD_TAG);
                let lit = RecordLit::of(name, fields, self.structs, self.enums);
                self.records.push(Record::Lit(lit));
                self.entries.push((e as *const Expr as usize, idx, NAMED));
            }
            Expr::Lambda { params, body, .. } => self.lambda(e, params, body, fr),
            Expr::Block(stmts) => {
                push(fr);
                self.stmts(stmts, fr);
                pop(fr);
            }
            Expr::Match { subject, arms } => {
                self.expr(subject, fr);
                for a in arms {
                    push(fr);
                    self.pattern(&a.pattern, fr);
                    if let Some(g) = &a.guard {
                        self.expr(g, fr);
                    }
                    self.expr(&a.body, fr);
                    pop(fr);
                }
            }
            Expr::While { cond, body } => {
                self.loops += 1;
                self.hot += 1;
                self.expr(cond, fr);
                self.loop_body(body, fr);
            }
            Expr::WhileLet {
                pattern,
                expr,
                body,
            } => {
                self.loops += 1;
                self.hot += 1;
                self.expr(expr, fr);
                push(fr);
                self.pattern(pattern, fr);
                self.loop_body(body, fr);
                pop(fr);
            }
            Expr::For {
                var,
                start,
                end,
                body,
                inclusive,
            } => {
                self.loops += 1;
                let loops = self.loops;
                self.expr(start, fr);
                self.expr(end, fr);
                push(fr);
                let slot = bind(fr, intern(var));
                self.name(e, var, slot);
                self.loop_body(body, fr);
                pop(fr);
                if self.loops > loops || super::vm::long_for(start, end, *inclusive) {
                    self.hot += 1;
                }
            }
            // Handler arms run on a snapshot of the defining env (the arms
            // themselves on run-time clones), so they are resolved by name;
            // the handled body runs in this frame.
            Expr::WithHandler { handler, body } => {
                if let HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                    for a in arms.iter().chain(return_arm.as_deref()) {
                        self.pattern(&a.binding, &mut None);
                        self.expr(&a.body, &mut None);
                    }
                }
                self.expr(body, fr);
            }
            Expr::Select(arms) => {
                for a in arms {
                    self.expr(&a.recv, fr);
                    self.expr(&a.body, fr);
                }
            }
            Expr::If { cond, then, else_ } => {
                self.expr(cond, fr);
                self.expr(then, fr);
                if let Some(b) = else_ {
                    self.expr(b, fr);
                }
            }
            Expr::Call { callee, args, .. } => {
                self.expr(callee, fr);
                args.iter().for_each(|a| self.expr(a, fr));
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.expr(receiver, fr);
                args.iter().for_each(|a| self.expr(a, fr));
            }
            Expr::BinOp { left, right, .. } => {
                self.expr(left, fr);
                self.expr(right, fr);
            }
            Expr::Index { receiver, index } => {
                self.expr(receiver, fr);
                self.expr(index, fr);
            }
            Expr::UnaryOp { operand: b, .. }
            | Expr::Question(b)
            | Expr::Spawn(b)
            | Expr::Comptime(b)
            | Expr::Ok(b)
            | Expr::Err(b)
            | Expr::Some(b) => self.expr(b, fr),
            Expr::Return(inner) => {
                if let Some(b) = inner {
                    self.expr(b, fr);
                }
            }
            Expr::Tuple(xs) | Expr::Array(xs) => xs.iter().for_each(|x| self.expr(x, fr)),
            Expr::FmtStr { parts } => {
                for p in parts {
                    if let crate::ast::FmtPart::Expr(inner) = p {
                        self.expr(inner, fr);
                    }
                }
            }
        }
    }

    /// Give lambda node `e` its cloned code and capture list. In a slotted
    /// frame the clone's body is a frame of its own, laid out the way
    /// `call_closure_owned_by` builds its env: the captured variables (slots
    /// `0..`, the capture cell) as the base scope, then a scope holding the
    /// parameters by position.
    fn lambda(&mut self, e: &Expr, params: &[LambdaParam], body: &Expr, fr: &mut Option<Frame>) {
        let free = free_vars(params, body);
        let param_syms: Box<[Sym]> = params.iter().map(|p| intern(&p.name)).collect();
        let (captures, mut inner): (Box<[(Sym, u32)]>, Option<Frame>) = match fr {
            Some(f) => {
                let captures: Box<[(Sym, u32)]> = free
                    .iter()
                    .map(|&s| (s, f.lookup(s)))
                    .filter(|&(_, slot)| slot != NOT_LOCAL)
                    .collect();
                let mut inner = Frame::with_base(captures.iter().map(|&(s, _)| s));
                inner.push();
                param_syms.iter().for_each(|&s| inner.bind_positional(s));
                (captures, Some(inner))
            }
            None => (free.iter().map(|&s| (s, NAMED)).collect(), None),
        };
        let param_base = inner
            .as_ref()
            .map(|_| u32::try_from(captures.len()).expect("fewer than 2^32 captures"));
        // Numbered before the body is resolved: source pre-order.
        let name = format!("{}::lambda#{}", self.owner, self.next_lambda);
        self.next_lambda += 1;
        let code = Rc::new(ClosureCode {
            params: param_syms,
            body: body.clone(),
            param_base,
            compiled: Some(super::vm::LambdaBody::new(name)),
            traced: std::cell::Cell::new(false),
        });
        let hot = self.hot;
        self.expr(&code.body, &mut inner);
        if self.hot > hot {
            if let Some(lb) = &code.compiled {
                lb.set_hot();
            }
        }
        let idx = tagged_index(self.lambdas.len(), LAMBDA_TAG);
        self.lambdas.push(LambdaInfo { code, captures });
        self.entries.push((e as *const Expr as usize, idx, NAMED));
    }

    /// Record every binding `Pattern::Ident` (bound in the current scope of
    /// `fr`, in the order `match_pattern` binds) and every struct pattern.
    fn pattern(&mut self, p: &Pattern, fr: &mut Option<Frame>) {
        let key = p as *const Pattern as usize;
        match p {
            Pattern::Ident(n) => {
                let slot = bind(fr, intern(n));
                self.entries.push((key, table_sym(n), slot));
            }
            Pattern::Some(q) | Pattern::Ok(q) | Pattern::Err(q) => self.pattern(q, fr),
            Pattern::Struct { name, fields } => {
                let idx = tagged_index(self.records.len(), RECORD_TAG);
                self.records.push(Record::Pat(RecordPat::of(name, fields)));
                self.entries.push((key, idx, NAMED));
                fields.iter().for_each(|(_, q)| self.pattern(q, fr));
            }
            Pattern::Tuple(ps) => ps.iter().for_each(|q| self.pattern(q, fr)),
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
